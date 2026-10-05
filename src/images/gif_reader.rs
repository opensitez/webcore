use std::io::{self, BufReader, Read, Seek, SeekFrom};

const HEADER_LENGTH: usize = 13;
const COLOR_TABLE_FLAG: u8 = 0x80;
const COLOR_TABLE_SIZE_BITS: u8 = 0x07;
const DEFAULT_COLOR_COUNT: usize = 256;

/// GIF89a section 11 permits a decoder-supplied palette when none is supplied.
/// Present it virtually so the strict raster decoder can consume the original
/// compressed stream without copying or waiting for the complete file.
pub(super) struct GifReader<R> {
    source: R,
    prefix: Vec<u8>,
    position: u64,
}

pub(super) fn reader<R: Read + Seek>(mut source: R) -> io::Result<BufReader<GifReader<R>>> {
    let mut header = [0u8; HEADER_LENGTH];
    source.read_exact(&mut header)?;
    if !header.starts_with(b"GIF87a") && !header.starts_with(b"GIF89a") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid GIF header",
        ));
    }
    let has_palette = header[10] & COLOR_TABLE_FLAG != 0;
    if !has_palette {
        header[10] |= COLOR_TABLE_FLAG | COLOR_TABLE_SIZE_BITS;
    }
    let mut prefix = header.to_vec();
    if !has_palette {
        // Black and white are the recommended first entries. Local palettes
        // still override this table, and transparent indices remain untouched.
        prefix.extend_from_slice(&[0, 0, 0, 255, 255, 255]);
        for index in 2..DEFAULT_COLOR_COUNT {
            prefix.extend_from_slice(&[index as u8; 3]);
        }
    }
    Ok(BufReader::new(GifReader {
        source,
        prefix,
        position: 0,
    }))
}

impl<R: Read> Read for GifReader<R> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        let prefix_len = self.prefix.len() as u64;
        let count = if self.position < prefix_len {
            let start = self.position as usize;
            let count = output.len().min(self.prefix.len() - start);
            output[..count].copy_from_slice(&self.prefix[start..start + count]);
            count
        } else {
            self.source.read(output)?
        };
        self.position += count as u64;
        Ok(count)
    }
}

impl<R: Seek> Seek for GifReader<R> {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let prefix_len = self.prefix.len() as u64;
        let target = match from {
            SeekFrom::Start(target) => i128::from(target),
            SeekFrom::Current(delta) => i128::from(self.position) + i128::from(delta),
            SeekFrom::End(delta) => {
                let length = self.source.seek(SeekFrom::End(0))?;
                i128::from(length) + i128::from(prefix_len) - HEADER_LENGTH as i128
                    + i128::from(delta)
            }
        };
        let target = u64::try_from(target)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid GIF seek"))?;
        let source_offset = HEADER_LENGTH as u64 + target.saturating_sub(prefix_len);
        self.source.seek(SeekFrom::Start(source_offset))?;
        self.position = target;
        Ok(target)
    }
}
