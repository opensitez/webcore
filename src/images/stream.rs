//! Network image decoding with paintable PNG row previews.

use super::{DecodedImage, decode_image_bytes_ex, premultiply_rgba};
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::sync::{Arc, Mutex};

const PNG_MAGIC: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
const MAX_PREVIEW_PIXELS: u64 = 8 * 1024 * 1024;

struct CaptureState<R> {
    source: R,
    bytes: Vec<u8>,
    position: usize,
}

struct CapturingReader<R>(Arc<Mutex<CaptureState<R>>>);

impl<R> Clone for CapturingReader<R> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<R: Read> Read for CapturingReader<R> {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        let mut state = self.0.lock().unwrap();
        if out.is_empty() {
            return Ok(0);
        }
        if state.position < state.bytes.len() {
            let available = state.bytes.len() - state.position;
            let count = available.min(out.len());
            out[..count].copy_from_slice(&state.bytes[state.position..state.position + count]);
            state.position += count;
            return Ok(count);
        }
        if state.position != state.bytes.len() {
            return Ok(0);
        }
        let limit = out.len().min(16 * 1024);
        let count = state.source.read(&mut out[..limit])?;
        state.bytes.extend_from_slice(&out[..count]);
        state.position += count;
        Ok(count)
    }
}

impl<R: Read> Seek for CapturingReader<R> {
    fn seek(&mut self, from: SeekFrom) -> std::io::Result<u64> {
        let mut state = self.0.lock().unwrap();
        let base = match from {
            SeekFrom::Start(position) => position as i128,
            SeekFrom::Current(offset) => state.position as i128 + offset as i128,
            SeekFrom::End(offset) => {
                let CaptureState { source, bytes, .. } = &mut *state;
                source.read_to_end(bytes)?;
                state.bytes.len() as i128 + offset as i128
            }
        };
        let position = usize::try_from(base)
            .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
        while state.bytes.len() < position {
            let mut chunk = [0; 16 * 1024];
            let limit = (position - state.bytes.len()).min(chunk.len());
            let count = state.source.read(&mut chunk[..limit])?;
            if count == 0 {
                break;
            }
            state.bytes.extend_from_slice(&chunk[..count]);
        }
        state.position = position;
        Ok(position as u64)
    }
}

fn decode_png_rows<R: Read>(
    source: CapturingReader<R>,
    on_dimensions: &mut impl FnMut(u32, u32),
    on_preview: &mut impl FnMut(DecodedImage),
) -> Result<Option<DecodedImage>, String> {
    let mut decoder = png::Decoder::new(BufReader::new(source));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(|error| error.to_string())?;
    let (width, height) = (reader.info().width, reader.info().height);
    if width > 0 && height > 0 {
        on_dimensions(width, height);
    }
    let (color, depth) = reader.output_color_type();
    if reader.info().is_animated()
        || depth != png::BitDepth::Eight
        || (width as u64) * (height as u64) > MAX_PREVIEW_PIXELS
        || width == 0
        || height == 0
    {
        return Ok(None);
    }
    let mut pixels = vec![0; width as usize * height as usize * 4];
    if reader.info().interlaced {
        let thresholds = adam7_preview_rows(width, height);
        let mut row_pixels = Vec::new();
        let mut rows_read = 0;
        while let Some(row) = reader
            .next_interlaced_row()
            .map_err(|error| error.to_string())?
        {
            let png::InterlaceInfo::Adam7(info) = row.interlace() else {
                return Err("expected Adam7 row".to_string());
            };
            let samples = row.data().len() / color.samples();
            row_pixels.resize(samples * 4, 0);
            convert_row(row.data(), &mut row_pixels, color)?;
            png::splat_interlaced_row(&mut pixels, width as usize * 4, &row_pixels, info, 32);
            rows_read += 1;
            if thresholds.contains(&rows_read) {
                on_preview(DecodedImage::Raster(
                    Arc::new(pixels.clone()),
                    width,
                    height,
                ));
            }
        }
        reader.finish().map_err(|error| error.to_string())?;
        return Ok(Some(DecodedImage::Raster(Arc::new(pixels), width, height)));
    }
    let preview_count = if (width as u64) * (height as u64) > 1024 * 1024 {
        1
    } else {
        2
    };
    let mut emitted = 0;
    for row_index in 0..height {
        let row = reader
            .next_row()
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "PNG ended before all rows were decoded".to_string())?;
        let target = &mut pixels[row_index as usize * width as usize * 4
            ..(row_index as usize + 1) * width as usize * 4];
        convert_row(row.data(), target, color)?;
        if emitted < preview_count
            && row_index + 1 < height
            && (row_index + 1) as u64 * (preview_count + 1) as u64
                >= height as u64 * (emitted + 1) as u64
        {
            on_preview(DecodedImage::Raster(
                Arc::new(pixels.clone()),
                width,
                height,
            ));
            emitted += 1;
        }
    }
    reader.finish().map_err(|error| error.to_string())?;
    Ok(Some(DecodedImage::Raster(Arc::new(pixels), width, height)))
}

fn adam7_preview_rows(width: u32, height: u32) -> Vec<usize> {
    const PASSES: [(u32, u32, u32, u32); 7] = [
        (0, 8, 0, 8),
        (4, 8, 0, 8),
        (0, 4, 4, 8),
        (2, 4, 0, 4),
        (0, 2, 2, 4),
        (1, 2, 0, 2),
        (0, 1, 1, 2),
    ];
    let mut rows = 0usize;
    let mut thresholds = Vec::with_capacity(2);
    for (pass, (x_offset, _, y_offset, y_step)) in PASSES.into_iter().enumerate() {
        if width > x_offset && height > y_offset {
            rows += (height - y_offset).div_ceil(y_step) as usize;
        }
        if pass == 0 || pass == 4 {
            thresholds.push(rows);
        }
    }
    thresholds.retain(|threshold| *threshold > 0 && *threshold < rows);
    thresholds.dedup();
    thresholds
}

fn webp_header_dimensions<R: Read>(mut source: CapturingReader<R>) -> Option<(u32, u32)> {
    let mut header = [0; 20];
    source.read_exact(&mut header).ok()?;
    if &header[..4] != b"RIFF" || &header[8..12] != b"WEBP" {
        return None;
    }
    let chunk_size = u32::from_le_bytes(header[16..20].try_into().ok()?);
    let (width, height) = match &header[12..16] {
        b"VP8X" if chunk_size >= 10 => {
            let mut data = [0; 10];
            source.read_exact(&mut data).ok()?;
            let width = 1 + u32::from_le_bytes([data[4], data[5], data[6], 0]);
            let height = 1 + u32::from_le_bytes([data[7], data[8], data[9], 0]);
            (width, height)
        }
        b"VP8L" if chunk_size >= 5 => {
            let mut data = [0; 5];
            source.read_exact(&mut data).ok()?;
            if data[0] != 0x2f {
                return None;
            }
            let packed = u32::from_le_bytes(data[1..5].try_into().ok()?);
            ((packed & 0x3fff) + 1, ((packed >> 14) & 0x3fff) + 1)
        }
        b"VP8 " if chunk_size >= 10 => {
            let mut data = [0; 10];
            source.read_exact(&mut data).ok()?;
            if data[3..6] != [0x9d, 0x01, 0x2a] {
                return None;
            }
            let width = u16::from_le_bytes([data[6], data[7]]) & 0x3fff;
            let height = u16::from_le_bytes([data[8], data[9]]) & 0x3fff;
            (u32::from(width), u32::from(height))
        }
        _ => return None,
    };
    (width > 0 && height > 0 && u64::from(width) * u64::from(height) <= u32::MAX as u64)
        .then_some((width, height))
}

fn decode_bmp_previews<R: Read>(
    capture: CapturingReader<R>,
    width: u32,
    height: u32,
    on_preview: &mut impl FnMut(DecodedImage),
) {
    if u64::from(width) * u64::from(height) > MAX_PREVIEW_PIXELS || height < 3 {
        return;
    }
    let mut reader = capture.clone();
    if reader.seek(SeekFrom::Start(0)).is_err() {
        return;
    }
    let mut header = [0u8; 54];
    if reader.read_exact(&mut header).is_err() || &header[..2] != b"BM" {
        return;
    }
    let dib_size = u32::from_le_bytes(header[14..18].try_into().unwrap());
    let stored_width = i32::from_le_bytes(header[18..22].try_into().unwrap());
    let stored_height = i32::from_le_bytes(header[22..26].try_into().unwrap());
    let planes = u16::from_le_bytes(header[26..28].try_into().unwrap());
    let depth = u16::from_le_bytes(header[28..30].try_into().unwrap());
    let compression = u32::from_le_bytes(header[30..34].try_into().unwrap());
    if dib_size < 40
        || stored_width != width as i32
        || stored_height.unsigned_abs() != height
        || planes != 1
        || !matches!(depth, 24 | 32)
        || compression != 0
    {
        return;
    }
    let pixel_offset = u32::from_le_bytes(header[10..14].try_into().unwrap()) as usize;
    let stride = ((width as usize * depth as usize + 31) / 32) * 4;
    let Some(pixel_end) = stride
        .checked_mul(height as usize)
        .and_then(|size| pixel_offset.checked_add(size))
    else {
        return;
    };
    if pixel_offset < 14 + dib_size as usize || pixel_end > 40 * 1024 * 1024 {
        return;
    }
    let preview_count = if u64::from(width) * u64::from(height) > 1024 * 1024 {
        1
    } else {
        2
    };
    for preview in 1..=preview_count {
        let rows = (height as usize * preview) / (preview_count + 1);
        let target = pixel_offset + rows * stride;
        if target >= pixel_end {
            break;
        }
        let captured = capture.0.lock().unwrap().bytes.len();
        if captured < target {
            let mut reader = capture.clone();
            if reader.seek(SeekFrom::Start(captured as u64)).is_err() {
                break;
            }
            let mut remaining = target - captured;
            let mut chunk = [0u8; 16 * 1024];
            while remaining > 0 {
                let read_len = remaining.min(chunk.len());
                match reader.read(&mut chunk[..read_len]) {
                    Ok(0) | Err(_) => return,
                    Ok(count) => remaining -= count,
                }
            }
        }
        let snapshot = {
            let state = capture.0.lock().unwrap();
            if state.bytes.len() >= pixel_end {
                break;
            }
            let mut snapshot = vec![0; pixel_end];
            let len = state.bytes.len().min(pixel_end);
            snapshot[..len].copy_from_slice(&state.bytes[..len]);
            snapshot
        };
        if let Some(decoded @ DecodedImage::Raster(..)) = decode_image_bytes_ex(&snapshot) {
            on_preview(decoded);
        }
    }
}

fn decode_webp_preview<R: Read>(
    capture: CapturingReader<R>,
    width: u32,
    height: u32,
    on_preview: &mut impl FnMut(DecodedImage),
) -> Option<DecodedImage> {
    if u64::from(width) * u64::from(height) > MAX_PREVIEW_PIXELS {
        return None;
    }
    let animated = {
        let state = capture.0.lock().unwrap();
        if state.bytes.len() < 21 || &state.bytes[12..16] != b"VP8X" {
            return None;
        }
        state.bytes[20] & 0x02 != 0
    };
    let mut reader = capture.clone();
    if reader.seek(SeekFrom::Start(12)).is_err() {
        return None;
    }
    let riff_end = {
        let state = capture.0.lock().unwrap();
        let Some(size) = state.bytes.get(4..8) else {
            return None;
        };
        let Some(end) = (u32::from_le_bytes(size.try_into().unwrap()) as usize).checked_add(8)
        else {
            return None;
        };
        end
    };
    let mut position = 12usize;
    while position < riff_end.min(8 * 1024 * 1024) {
        let mut header = [0u8; 8];
        if reader.read_exact(&mut header).is_err() {
            return None;
        }
        let chunk_size = u32::from_le_bytes(header[4..8].try_into().unwrap()) as usize;
        let Some(next) = position
            .checked_add(8)
            .and_then(|end| end.checked_add(chunk_size))
            .and_then(|end| end.checked_add(chunk_size & 1))
        else {
            return None;
        };
        if next > riff_end || next > 8 * 1024 * 1024 {
            return None;
        }
        if (animated && &header[..4] == b"ANMF")
            || (!animated && matches!(&header[..4], b"VP8 " | b"VP8L"))
        {
            if reader.seek(SeekFrom::Start(next as u64)).is_err() {
                return None;
            }
            let mut bytes = {
                let state = capture.0.lock().unwrap();
                if state.bytes.len() < next || state.bytes.len() >= riff_end {
                    return None;
                }
                state.bytes[..next].to_vec()
            };
            let Ok(riff_size) = u32::try_from(next - 8) else {
                return None;
            };
            bytes[4..8].copy_from_slice(&riff_size.to_le_bytes());
            bytes[20] &= !(0x08 | 0x04);
            if animated {
                if let Ok(decoder) =
                    image::codecs::webp::WebPDecoder::new(std::io::Cursor::new(bytes))
                    && let Some(Ok(frame)) = image::AnimationDecoder::into_frames(decoder).next()
                {
                    let mut pixels = frame.into_buffer().into_raw();
                    premultiply_rgba(&mut pixels);
                    on_preview(DecodedImage::Raster(Arc::new(pixels), width, height));
                }
            } else if let Some(decoded @ DecodedImage::Raster(..)) = decode_image_bytes_ex(&bytes) {
                on_preview(decoded.clone());
                return Some(decoded);
            }
            return None;
        }
        if reader.seek(SeekFrom::Start(next as u64)).is_err() {
            return None;
        }
        position = next;
    }
    None
}

fn complete_webp_riff(bytes: &[u8]) -> bool {
    if bytes.len() < 12 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WEBP" {
        return false;
    }
    let Some(expected) =
        (u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize).checked_add(8)
    else {
        return false;
    };
    if expected != bytes.len() {
        return false;
    }
    let mut position = 12usize;
    while position < expected {
        let Some(end) = position.checked_add(8) else {
            return false;
        };
        let Some(header) = bytes.get(position..end) else {
            return false;
        };
        let size = u32::from_le_bytes(header[4..8].try_into().unwrap()) as usize;
        let Some(next) = position
            .checked_add(8)
            .and_then(|offset| offset.checked_add(size))
            .and_then(|offset| offset.checked_add(size & 1))
        else {
            return false;
        };
        if next > expected {
            return false;
        }
        position = next;
    }
    position == expected
}

fn convert_row(source: &[u8], target: &mut [u8], color: png::ColorType) -> Result<(), String> {
    match color {
        png::ColorType::Rgba if source.len() == target.len() => {
            target.copy_from_slice(source);
            premultiply_rgba(target);
        }
        png::ColorType::Rgb if source.len() * 4 == target.len() * 3 => {
            for (rgb, rgba) in source.chunks_exact(3).zip(target.chunks_exact_mut(4)) {
                rgba.copy_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
            }
        }
        png::ColorType::Grayscale if source.len() * 4 == target.len() => {
            for (&gray, rgba) in source.iter().zip(target.chunks_exact_mut(4)) {
                rgba.copy_from_slice(&[gray, gray, gray, 255]);
            }
        }
        png::ColorType::GrayscaleAlpha if source.len() * 2 == target.len() => {
            for (gray_alpha, rgba) in source.chunks_exact(2).zip(target.chunks_exact_mut(4)) {
                let (gray, alpha) = (gray_alpha[0], gray_alpha[1]);
                let premultiplied = ((gray as u16 * alpha as u16) / 255) as u8;
                rgba.copy_from_slice(&[premultiplied, premultiplied, premultiplied, alpha]);
            }
        }
        _ => return Err("unsupported PNG row format".to_string()),
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn decode_reader_with_previews<R: Read>(
    source: R,
    on_preview: impl FnMut(DecodedImage),
) -> Result<DecodedImage, String> {
    decode_reader_with_previews_and_bytes(source, |_, _| {}, on_preview).map(|(decoded, _)| decoded)
}

fn decode_reader_with_previews_and_bytes<R: Read>(
    mut source: R,
    mut on_dimensions: impl FnMut(u32, u32),
    mut on_preview: impl FnMut(DecodedImage),
) -> Result<(DecodedImage, Vec<u8>), String> {
    let mut preview_decoded = None;
    let mut prefix = [0; 8];
    source
        .read_exact(&mut prefix)
        .map_err(|error| error.to_string())?;
    let capture = CapturingReader(Arc::new(Mutex::new(CaptureState {
        source,
        bytes: prefix.to_vec(),
        position: 0,
    })));
    if prefix.starts_with(&[0xff, 0x0a]) || prefix.starts_with(b"\0\0\0\x0cJXL ") {
        let image = webmedia::bitmap::jpeg_xl::decode_stream(
            capture.clone(),
            &mut on_dimensions,
            |image| on_preview(DecodedImage::Raster(Arc::new(image.rgba), image.width, image.height)),
        )?;
        let mut state = capture.0.lock().unwrap();
        let CaptureState { source, bytes, .. } = &mut *state;
        source.read_to_end(bytes).map_err(|error| error.to_string())?;
        return Ok((DecodedImage::Raster(Arc::new(image.rgba), image.width, image.height), std::mem::take(bytes)));
    }
    if &prefix == PNG_MAGIC {
        if let Ok(Some(decoded)) =
            decode_png_rows(capture.clone(), &mut on_dimensions, &mut on_preview)
        {
            let mut state = capture.0.lock().unwrap();
            let CaptureState { source, bytes, .. } = &mut *state;
            source
                .read_to_end(bytes)
                .map_err(|error| error.to_string())?;
            return Ok((decoded, std::mem::take(&mut state.bytes)));
        }
        let mut replay = capture.clone();
        let _ = replay.seek(SeekFrom::Start(0));
        if let Ok(decoder) = image::codecs::png::PngDecoder::new(BufReader::new(replay))
            && decoder.is_apng().ok() == Some(true)
        {
            let (width, height) = image::ImageDecoder::dimensions(&decoder);
            if (width as u64) * (height as u64) <= MAX_PREVIEW_PIXELS
                && let Ok(apng) = decoder.apng()
                && let Some(Ok(frame)) = image::AnimationDecoder::into_frames(apng).next()
            {
                let mut pixels = frame.into_buffer().into_raw();
                premultiply_rgba(&mut pixels);
                on_preview(DecodedImage::Raster(Arc::new(pixels), width, height));
            }
        }
    } else if prefix.starts_with(&[0xff, 0xd8]) {
        let mut decoder = zune_jpeg::JpegDecoder::new(BufReader::new(capture.clone()));
        if decoder.decode_headers().is_ok()
            && let Some((width, height)) = decoder.dimensions()
            && let (Ok(width), Ok(height)) = (u32::try_from(width), u32::try_from(height))
        {
            on_dimensions(width, height);
        }
    } else if prefix.starts_with(b"GIF87a") || prefix.starts_with(b"GIF89a") {
        if let Ok(reader) = super::gif_reader::reader(capture.clone())
            && let Ok(decoder) = image::codecs::gif::GifDecoder::new(reader)
        {
            let (width, height) = image::ImageDecoder::dimensions(&decoder);
            on_dimensions(width, height);
            if (width as u64) * (height as u64) <= MAX_PREVIEW_PIXELS
                && let Some(Ok(frame)) = image::AnimationDecoder::into_frames(decoder).next()
            {
                let mut pixels = frame.into_buffer().into_raw();
                premultiply_rgba(&mut pixels);
                on_preview(DecodedImage::Raster(Arc::new(pixels), width, height));
            }
        }
    } else if prefix.starts_with(b"BM") {
        if let Ok(decoder) = image::codecs::bmp::BmpDecoder::new(BufReader::new(capture.clone())) {
            let (width, height) = image::ImageDecoder::dimensions(&decoder);
            on_dimensions(width, height);
            decode_bmp_previews(capture.clone(), width, height, &mut on_preview);
        }
    } else if prefix.starts_with(b"RIFF") {
        if let Some((width, height)) = webp_header_dimensions(capture.clone()) {
            on_dimensions(width, height);
            preview_decoded = decode_webp_preview(capture.clone(), width, height, &mut on_preview);
        }
    }
    let mut state = capture.0.lock().unwrap();
    let CaptureState { source, bytes, .. } = &mut *state;
    source
        .read_to_end(bytes)
        .map_err(|error| error.to_string())?;
    let decoded = if let Some(preview) = preview_decoded {
        if !complete_webp_riff(&state.bytes) {
            return Err("incomplete WebP RIFF".to_string());
        }
        preview
    } else {
        decode_image_bytes_ex(&state.bytes).ok_or_else(|| "unsupported image bytes".to_string())?
    };
    Ok((decoded, std::mem::take(&mut state.bytes)))
}

pub(crate) fn fetch_decode_with_previews_cached(
    url: &str,
    cache_dir: Option<&str>,
    on_dimensions: impl FnMut(u32, u32),
    on_preview: impl FnMut(DecodedImage),
) -> Result<DecodedImage, String> {
    if let Some(cache_dir) = cache_dir
        && let Some(bytes) = crate::loading::cached_bytes_if_present(url, cache_dir)
    {
        return super::decode_image_bytes_arc(bytes)
            .ok_or_else(|| format!("unsupported cached image bytes from {url}"));
    }
    let fetch = |client: &reqwest::blocking::Client| {
        let response = client
            .get(url)
            .header(
                "Accept",
                "image/jxl,image/avif,image/webp,image/apng,image/*,*/*;q=0.8",
            )
            .header("Sec-Fetch-Dest", "image")
            .header("Sec-Fetch-Mode", "no-cors")
            .header("Sec-Fetch-Site", "cross-site")
            .send()
            .map_err(|error| (error.to_string(), true))?;
        if !response.status().is_success() {
            return Err((format!("HTTP {}", response.status()), false));
        }
        Ok(response)
    };
    let response = match fetch(&crate::http_client()) {
        Ok(response) => Ok(response),
        Err((_, true)) if url.starts_with("https://") => fetch(&crate::http_client_lenient()),
        Err(error) => Err(error),
    }
    .map_err(|(error, _)| error)?;
    let (decoded, bytes) =
        decode_reader_with_previews_and_bytes(response, on_dimensions, on_preview)?;
    if let Some(cache_dir) = cache_dir {
        crate::loading::cache_fetched_bytes(url, cache_dir, bytes);
    }
    Ok(decoded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn palette_free_gif_stream_publishes_transparent_preview_and_final() {
        let bytes = super::super::image_data_url_bytes(
            "data:image/gif;base64,R0lGODlhAQABAHAAACH5BAEAAAAALAAAAAABAAEAAAICRAEAOw==",
        )
        .unwrap();
        let mut previews = 0;
        let (final_image, captured) = decode_reader_with_previews_and_bytes(
            bytes.as_slice(),
            |width, height| assert_eq!((width, height), (1, 1)),
            |image| {
                previews += 1;
                let DecodedImage::Raster(pixels, width, height) = image else {
                    panic!("expected preview")
                };
                assert_eq!((width, height), (1, 1));
                assert_eq!(pixels.as_slice(), &[0, 0, 0, 0]);
            },
        )
        .unwrap();
        assert_eq!(previews, 1);
        assert_eq!(
            captured, bytes,
            "virtual palette must not rewrite cached bytes"
        );
        let DecodedImage::Raster(pixels, width, height) = final_image else {
            panic!("expected final")
        };
        assert_eq!((width, height), (1, 1));
        assert_eq!(pixels.as_slice(), &[0, 0, 0, 0]);
    }

    struct ChunkedReader {
        bytes: Vec<u8>,
        position: usize,
        consumed: Arc<AtomicUsize>,
    }

    impl Read for ChunkedReader {
        fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
            let count = out.len().min(7).min(self.bytes.len() - self.position);
            out[..count].copy_from_slice(&self.bytes[self.position..self.position + count]);
            self.position += count;
            self.consumed.store(self.position, Ordering::SeqCst);
            Ok(count)
        }
    }

    #[test]
    fn jpeg_xl_stream_reports_dimensions_and_preserves_cached_bytes() {
        let bytes = include_bytes!("../../../webmedia/tests/fixtures/jxl-alpha.jxl").to_vec();
        let consumed = Arc::new(AtomicUsize::new(0));
        let reader = ChunkedReader { bytes: bytes.clone(), position: 0, consumed: consumed.clone() };
        let mut dimensions = None;
        let (decoded, captured) = decode_reader_with_previews_and_bytes(
            reader,
            |width, height| {
                assert!(consumed.load(Ordering::SeqCst) < bytes.len());
                dimensions = Some((width, height));
            },
            |preview| { assert!(matches!(preview, DecodedImage::Raster(..))); },
        ).unwrap();
        let DecodedImage::Raster(pixels, width, height) = decoded else { panic!("expected raster"); };
        let regular = webmedia::bitmap::decode_raster(&bytes).unwrap();
        assert_eq!(dimensions, Some((width, height)));
        assert_eq!(pixels.as_slice(), regular.rgba);
        assert_eq!(captured, bytes);
    }

    #[test]
    fn png_rows_are_published_before_input_is_complete() {
        let image = image::RgbaImage::from_fn(64, 64, |x, y| {
            image::Rgba([(x * 17) as u8, (y * 19) as u8, (x ^ y) as u8, 255])
        });
        let mut output = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image)
            .write_to(&mut output, image::ImageFormat::Png)
            .unwrap();
        let bytes = output.into_inner();
        let consumed = Arc::new(AtomicUsize::new(0));
        let source = ChunkedReader {
            bytes: bytes.clone(),
            position: 0,
            consumed: consumed.clone(),
        };
        let mut preview_count = 0;
        let dimensions = std::cell::Cell::new(None);
        let (decoded, captured) = decode_reader_with_previews_and_bytes(
            source,
            |width, height| {
                assert!(consumed.load(Ordering::SeqCst) < bytes.len());
                dimensions.set(Some((width, height)));
            },
            |preview| {
                preview_count += 1;
                assert_eq!(dimensions.get(), Some((64, 64)));
                assert!(consumed.load(Ordering::SeqCst) < bytes.len());
                let DecodedImage::Raster(data, width, height) = preview else {
                    panic!("expected raster preview");
                };
                assert_eq!((width, height), (64, 64));
                assert_eq!(&data[(63 * 64 * 4)..(63 * 64 * 4 + 4)], &[0, 0, 0, 0]);
            },
        )
        .unwrap();
        assert!(preview_count > 0);
        assert_eq!(captured, bytes);
        let DecodedImage::Raster(data, width, height) = decoded else {
            panic!("expected final raster");
        };
        assert_eq!((width, height), (64, 64));
        assert_eq!(&data[..4], &[0, 0, 0, 255]);
        assert_eq!(&data[(63 * 64 * 4)..(63 * 64 * 4 + 4)], &[0, 173, 63, 255]);
    }

    #[test]
    fn transparent_png_stream_matches_regular_decode() {
        let image = image::RgbaImage::from_raw(2, 1, vec![200, 100, 50, 128, 0, 0, 0, 0]).unwrap();
        let mut output = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image)
            .write_to(&mut output, image::ImageFormat::Png)
            .unwrap();
        let bytes = output.into_inner();
        let streamed = decode_reader_with_previews(bytes.as_slice(), |_| {}).unwrap();
        let regular = decode_image_bytes_ex(&bytes).unwrap();
        let (DecodedImage::Raster(streamed, _, _), DecodedImage::Raster(regular, _, _)) =
            (streamed, regular)
        else {
            panic!("expected raster images");
        };
        assert_eq!(streamed, regular);
    }

    #[test]
    fn indexed_png_transparency_matches_regular_decode() {
        let mut bytes = Vec::new();
        let mut encoder = png::Encoder::new(&mut bytes, 64, 4);
        encoder.set_color(png::ColorType::Indexed);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_palette(&[200, 40, 10, 5, 150, 250][..]);
        encoder.set_trns(&[0, 128][..]);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&vec![0, 1].repeat(128)).unwrap();
        writer.finish().unwrap();
        let streamed = decode_reader_with_previews(bytes.as_slice(), |_| {}).unwrap();
        let regular = decode_image_bytes_ex(&bytes).unwrap();
        let (DecodedImage::Raster(streamed, _, _), DecodedImage::Raster(regular, _, _)) =
            (streamed, regular)
        else {
            panic!("expected raster images");
        };
        assert_eq!(streamed, regular);
        assert_eq!(&streamed[..4], &[0, 0, 0, 0]);
        assert_eq!(streamed[7], 128);
    }

    #[test]
    fn sixteen_bit_png_alpha_matches_regular_decode() {
        let mut bytes = Vec::new();
        let mut encoder = png::Encoder::new(&mut bytes, 2, 1);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Sixteen);
        let mut writer = encoder.write_header().unwrap();
        writer
            .write_image_data(&[
                0xff, 0xff, 0, 0, 0, 0, 0x80, 0, 0, 0, 0xff, 0xff, 0, 0, 0, 0,
            ])
            .unwrap();
        writer.finish().unwrap();
        let streamed = decode_reader_with_previews(bytes.as_slice(), |_| {}).unwrap();
        let regular = decode_image_bytes_ex(&bytes).unwrap();
        let (DecodedImage::Raster(streamed, _, _), DecodedImage::Raster(regular, _, _)) =
            (streamed, regular)
        else {
            panic!("expected raster images");
        };
        assert_eq!(streamed, regular);
    }

    #[test]
    fn rgb_and_grayscale_transparency_keys_match_regular_decode() {
        for color in [png::ColorType::Rgb, png::ColorType::Grayscale] {
            let mut bytes = Vec::new();
            let mut encoder = png::Encoder::new(&mut bytes, 2, 1);
            encoder.set_color(color);
            encoder.set_depth(png::BitDepth::Eight);
            let samples = match color {
                png::ColorType::Rgb => {
                    encoder.set_trns(&[0, 255, 0, 0, 0, 0][..]);
                    vec![255, 0, 0, 0, 0, 255]
                }
                _ => {
                    encoder.set_trns(&[0, 128][..]);
                    vec![128, 64]
                }
            };
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&samples).unwrap();
            writer.finish().unwrap();
            let streamed = decode_reader_with_previews(bytes.as_slice(), |_| {}).unwrap();
            let regular = decode_image_bytes_ex(&bytes).unwrap();
            let (DecodedImage::Raster(streamed, _, _), DecodedImage::Raster(regular, _, _)) =
                (streamed, regular)
            else {
                panic!("expected raster images");
            };
            assert_eq!(streamed, regular, "{color:?}");
            assert_eq!(streamed[3], 0);
            assert_eq!(streamed[7], 255);
        }
    }

    #[test]
    fn interlaced_png_previews_and_final_match_regular_decode() {
        let bytes = include_bytes!("testdata/interlaced_rgba.png");
        let consumed = Arc::new(AtomicUsize::new(0));
        let source = ChunkedReader {
            bytes: bytes.to_vec(),
            position: 0,
            consumed: consumed.clone(),
        };
        let mut previews = 0;
        let (streamed, captured) = decode_reader_with_previews_and_bytes(
            source,
            |width, height| assert_eq!((width, height), (96, 96)),
            |preview| {
                previews += 1;
                assert!(consumed.load(Ordering::SeqCst) < bytes.len());
                let DecodedImage::Raster(data, width, height) = preview else {
                    panic!("expected raster preview");
                };
                assert_eq!((width, height), (96, 96));
                assert_eq!(data[3], 128);
            },
        )
        .unwrap();
        assert_eq!(previews, 2);
        assert_eq!(captured, bytes);
        let regular = decode_image_bytes_ex(bytes).unwrap();
        let (DecodedImage::Raster(streamed, _, _), DecodedImage::Raster(regular, _, _)) =
            (streamed, regular)
        else {
            panic!("expected raster images");
        };
        assert_eq!(streamed, regular);
    }

    #[test]
    fn interlaced_indexed_transparency_matches_regular_decode() {
        let bytes = include_bytes!("testdata/interlaced_indexed.png");
        let consumed = Arc::new(AtomicUsize::new(0));
        let source = ChunkedReader {
            bytes: bytes.to_vec(),
            position: 0,
            consumed: consumed.clone(),
        };
        let mut previews = 0;
        let streamed = decode_reader_with_previews(source, |_| {
            previews += 1;
            assert!(consumed.load(Ordering::SeqCst) < bytes.len());
        })
        .unwrap();
        assert!(previews > 0);
        let regular = decode_image_bytes_ex(bytes).unwrap();
        let (DecodedImage::Raster(streamed, _, _), DecodedImage::Raster(regular, _, _)) =
            (streamed, regular)
        else {
            panic!("expected raster images");
        };
        assert_eq!(streamed, regular);
        assert!(streamed.chunks_exact(4).any(|pixel| pixel[3] == 0));
        assert!(streamed.chunks_exact(4).any(|pixel| pixel[3] == 255));
    }

    #[test]
    fn other_raster_headers_report_dimensions_before_eof() {
        let image = image::RgbaImage::from_fn(64, 64, |x, y| {
            image::Rgba([(x * 17) as u8, (y * 19) as u8, (x ^ y) as u8, 255])
        });
        for format in [
            image::ImageFormat::Gif,
            image::ImageFormat::WebP,
            image::ImageFormat::Bmp,
        ] {
            let mut output = std::io::Cursor::new(Vec::new());
            image::DynamicImage::ImageRgba8(image.clone())
                .write_to(&mut output, format)
                .unwrap();
            let bytes = output.into_inner();
            let consumed = Arc::new(AtomicUsize::new(0));
            let source = ChunkedReader {
                bytes: bytes.clone(),
                position: 0,
                consumed: consumed.clone(),
            };
            let mut dimensions = None;
            let (decoded, captured) = decode_reader_with_previews_and_bytes(
                source,
                |width, height| {
                    dimensions = Some((width, height));
                    assert!(
                        consumed.load(Ordering::SeqCst) < bytes.len(),
                        "{format:?} dimensions arrived after EOF"
                    );
                },
                |_| {},
            )
            .unwrap();
            assert_eq!(dimensions, Some((64, 64)), "{format:?}");
            assert_eq!(captured, bytes);
            assert!(matches!(decoded, DecodedImage::Raster(_, 64, 64)));
        }
    }

    #[test]
    fn uncompressed_bmp_rows_arrive_before_eof() {
        let image = image::RgbImage::from_fn(128, 128, |x, y| {
            image::Rgb([(x * 2) as u8, (y * 2) as u8, 80])
        });
        let mut output = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(image)
            .write_to(&mut output, image::ImageFormat::Bmp)
            .unwrap();
        let bytes = output.into_inner();
        let consumed = Arc::new(AtomicUsize::new(0));
        let source = ChunkedReader {
            bytes: bytes.clone(),
            position: 0,
            consumed: consumed.clone(),
        };
        let mut previews = 0;
        let (decoded, captured) = decode_reader_with_previews_and_bytes(
            source,
            |width, height| assert_eq!((width, height), (128, 128)),
            |preview| {
                previews += 1;
                assert!(consumed.load(Ordering::SeqCst) < bytes.len());
                let DecodedImage::Raster(pixels, width, height) = preview else {
                    panic!("expected BMP raster preview");
                };
                assert_eq!((width, height), (128, 128));
                assert!(pixels.chunks_exact(4).any(|pixel| pixel[3] == 255));
            },
        )
        .unwrap();
        assert!(previews > 0);
        assert_eq!(captured, bytes);
        let regular = decode_image_bytes_ex(&bytes).unwrap();
        let (DecodedImage::Raster(decoded, _, _), DecodedImage::Raster(regular, _, _)) =
            (decoded, regular)
        else {
            panic!("expected BMP raster images");
        };
        assert_eq!(decoded, regular);
    }

    #[test]
    fn animated_gif_first_frame_arrives_before_eof() {
        let bytes = include_bytes!("testdata/animated.gif");
        let consumed = Arc::new(AtomicUsize::new(0));
        let source = ChunkedReader {
            bytes: bytes.to_vec(),
            position: 0,
            consumed: consumed.clone(),
        };
        let mut previews = 0;
        let (decoded, captured) = decode_reader_with_previews_and_bytes(
            source,
            |width, height| assert_eq!((width, height), (64, 64)),
            |preview| {
                previews += 1;
                assert!(consumed.load(Ordering::SeqCst) < bytes.len());
                let DecodedImage::Raster(data, width, height) = preview else {
                    panic!("expected first-frame preview");
                };
                assert_eq!((width, height), (64, 64));
                assert_eq!(&data[..4], &[255, 0, 0, 255]);
            },
        )
        .unwrap();
        assert_eq!(previews, 1);
        assert_eq!(captured, bytes);
        assert!(matches!(decoded, DecodedImage::Animated(_)));
    }

    #[test]
    fn apng_first_frame_arrives_before_eof_and_stays_animated() {
        let mut bytes = Vec::new();
        let mut encoder = png::Encoder::new(&mut bytes, 64, 64);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_animated(2, 0).unwrap();
        let mut writer = encoder.write_header().unwrap();
        writer
            .write_image_data(&[255, 0, 0, 255].repeat(64 * 64))
            .unwrap();
        writer
            .write_image_data(&[0, 0, 255, 255].repeat(64 * 64))
            .unwrap();
        writer.finish().unwrap();
        let consumed = Arc::new(AtomicUsize::new(0));
        let source = ChunkedReader {
            bytes: bytes.clone(),
            position: 0,
            consumed: consumed.clone(),
        };
        let mut previews = 0;
        let (decoded, captured) = decode_reader_with_previews_and_bytes(
            source,
            |width, height| assert_eq!((width, height), (64, 64)),
            |preview| {
                previews += 1;
                assert!(consumed.load(Ordering::SeqCst) < bytes.len());
                let DecodedImage::Raster(data, width, height) = preview else {
                    panic!("expected APNG first frame");
                };
                assert_eq!((width, height), (64, 64));
                assert_eq!(&data[..4], &[255, 0, 0, 255]);
            },
        )
        .unwrap();
        assert_eq!(previews, 1);
        assert_eq!(captured, bytes);
        let DecodedImage::Animated(mut animated) = decoded else {
            panic!("expected animated PNG");
        };
        assert_eq!(&animated.frames[0].pixels[..4], &[255, 0, 0, 255]);
        assert!(animated.can_animate());
        assert!(super::super::expand_animated_image_to_size(
            &mut animated,
            64,
            64
        ));
        assert_eq!(animated.frames.len(), 2);
        assert_eq!(&animated.frames[1].pixels[..4], &[0, 0, 255, 255]);
    }

    #[test]
    fn animated_webp_reports_dimensions_and_preserves_animation() {
        let bytes = include_bytes!("testdata/animated.webp");
        let consumed = Arc::new(AtomicUsize::new(0));
        let source = ChunkedReader {
            bytes: bytes.to_vec(),
            position: 0,
            consumed: consumed.clone(),
        };
        let mut previews = 0;
        let (decoded, captured) = decode_reader_with_previews_and_bytes(
            source,
            |width, height| {
                assert_eq!((width, height), (64, 64));
                assert!(consumed.load(Ordering::SeqCst) < bytes.len());
            },
            |preview| {
                previews += 1;
                assert!(consumed.load(Ordering::SeqCst) < bytes.len());
                let DecodedImage::Raster(pixels, width, height) = preview else {
                    panic!("expected WebP first-frame preview");
                };
                assert_eq!((width, height), (64, 64));
                assert_eq!(&pixels[..4], &[255, 0, 0, 255]);
            },
        )
        .unwrap();
        assert_eq!(previews, 1);
        assert_eq!(captured, bytes);
        assert!(matches!(decoded, DecodedImage::Animated(_)));
    }

    #[test]
    fn extended_webp_paints_before_trailing_metadata_arrives() {
        let image = image::RgbaImage::from_pixel(64, 64, image::Rgba([30, 100, 180, 255]));
        let mut output = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image)
            .write_to(&mut output, image::ImageFormat::WebP)
            .unwrap();
        let simple = output.into_inner();
        assert_eq!(&simple[12..16], b"VP8L");
        let mut bytes = b"RIFF\0\0\0\0WEBPVP8X".to_vec();
        bytes.extend_from_slice(&10u32.to_le_bytes());
        bytes.extend_from_slice(&[0x08, 0, 0, 0, 63, 0, 0, 63, 0, 0]);
        bytes.extend_from_slice(&simple[12..]);
        bytes.extend_from_slice(b"EXIF");
        bytes.extend_from_slice(&4096u32.to_le_bytes());
        bytes.extend_from_slice(&vec![0u8; 4096]);
        let riff_size = (bytes.len() - 8) as u32;
        bytes[4..8].copy_from_slice(&riff_size.to_le_bytes());
        let consumed = Arc::new(AtomicUsize::new(0));
        let source = ChunkedReader {
            bytes: bytes.clone(),
            position: 0,
            consumed: consumed.clone(),
        };
        let mut previews = 0;
        let (decoded, captured) = decode_reader_with_previews_and_bytes(
            source,
            |width, height| assert_eq!((width, height), (64, 64)),
            |preview| {
                previews += 1;
                assert!(consumed.load(Ordering::SeqCst) < bytes.len());
                assert!(matches!(preview, DecodedImage::Raster(_, 64, 64)));
            },
        )
        .unwrap();
        assert_eq!(previews, 1);
        assert_eq!(captured, bytes);
        let regular = decode_image_bytes_ex(&bytes).unwrap();
        let (DecodedImage::Raster(decoded, _, _), DecodedImage::Raster(regular, _, _)) =
            (decoded, regular)
        else {
            panic!("expected WebP rasters");
        };
        assert_eq!(decoded, regular);
    }

    #[test]
    fn extended_webp_preview_does_not_accept_truncated_riff() {
        let image = image::RgbaImage::from_pixel(64, 64, image::Rgba([30, 100, 180, 255]));
        let mut output = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image)
            .write_to(&mut output, image::ImageFormat::WebP)
            .unwrap();
        let simple = output.into_inner();
        let mut bytes = b"RIFF\0\0\0\0WEBPVP8X".to_vec();
        bytes.extend_from_slice(&10u32.to_le_bytes());
        bytes.extend_from_slice(&[0x08, 0, 0, 0, 63, 0, 0, 63, 0, 0]);
        bytes.extend_from_slice(&simple[12..]);
        bytes.extend_from_slice(b"EXIF");
        bytes.extend_from_slice(&4096u32.to_le_bytes());
        bytes.extend_from_slice(&vec![0u8; 4096]);
        let riff_size = (bytes.len() - 8) as u32;
        bytes[4..8].copy_from_slice(&riff_size.to_le_bytes());
        bytes.truncate(bytes.len() - 2048);
        let consumed = Arc::new(AtomicUsize::new(0));
        let source = ChunkedReader {
            bytes,
            position: 0,
            consumed,
        };
        let mut previews = 0;
        let result = decode_reader_with_previews_and_bytes(
            source,
            |width, height| assert_eq!((width, height), (64, 64)),
            |_| previews += 1,
        );
        assert_eq!(previews, 1);
        assert!(result.is_err());
    }

    #[test]
    fn jpeg_stream_falls_back_to_complete_decode() {
        let image = image::RgbImage::from_raw(1, 1, vec![120, 80, 40]).unwrap();
        let mut output = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(image)
            .write_to(&mut output, image::ImageFormat::Jpeg)
            .unwrap();
        let mut previews = 0;
        let bytes = output.into_inner();
        let consumed = Arc::new(AtomicUsize::new(0));
        let source = ChunkedReader {
            bytes: bytes.clone(),
            position: 0,
            consumed: consumed.clone(),
        };
        let mut dimensions = None;
        let (decoded, captured) = decode_reader_with_previews_and_bytes(
            source,
            |width, height| {
                dimensions = Some((width, height));
                assert!(consumed.load(Ordering::SeqCst) < bytes.len());
            },
            |_| {
                previews += 1;
            },
        )
        .unwrap();
        assert_eq!(dimensions, Some((1, 1)));
        assert_eq!(captured, bytes);
        assert_eq!(previews, 0);
        assert!(matches!(decoded, DecodedImage::Raster(_, 1, 1)));
    }
}
