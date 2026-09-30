//! Embedded OpenType container parsing. Header inspection works on a prefix;
//! the font payload can arrive later without retaining a second copy of it.

use std::ops::Range;

const HEADER_FIXED_SIZE: usize = 82;
const MAGIC_OFFSET: usize = 34;
const EMBEDDING_FLAGS_OFFSET: usize = 32;
const XOR_FLAG: u32 = 0x1000_0000;
const MTX_FLAG: u32 = 0x0000_0004;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EotHeader {
    pub total_size: usize,
    pub font_data: Range<usize>,
    pub root_string: Range<usize>,
    pub version: u32,
    pub flags: u32,
    pub embedding_flags: u16,
    pub root_checksum: Option<u32>,
}

impl EotHeader {
    pub fn mtx_header(&self, prefix: &[u8]) -> Result<Option<super::mtx::MtxHeader>, &'static str> {
        if self.flags & MTX_FLAG == 0 {
            return Err("EOT font data is not MTX compressed");
        }
        let start = self.font_data.start;
        let available = prefix.len().saturating_sub(start).min(self.font_data.len());
        if available < 10 {
            return Ok(None);
        }
        let mut header = [0; 10];
        header.copy_from_slice(&prefix[start..start + 10]);
        if self.flags & XOR_FLAG != 0 {
            for byte in &mut header {
                *byte ^= 0x50;
            }
        }
        super::mtx::parse_prefix(&header, self.font_data.len())
    }

    /// Decode one complete MTX block as soon as its bytes have arrived. Other
    /// blocks, and the rest of the EOT resource, need not be available yet.
    pub fn decode_mtx_stream(
        &self,
        prefix: &[u8],
        stream_index: usize,
    ) -> Result<Option<Vec<u8>>, &'static str> {
        let mtx = match self.mtx_header(prefix)? {
            Some(header) => header,
            None => return Ok(None),
        };
        let range = mtx
            .streams
            .get(stream_index)
            .ok_or("invalid MTX stream index")?;
        let start = self.font_data.start + range.start;
        let end = self.font_data.start + range.end;
        if prefix.len() < end {
            return Ok(None);
        }
        if self.flags & XOR_FLAG != 0 {
            let mut block = prefix[start..end].to_vec();
            for byte in &mut block {
                *byte ^= 0x50;
            }
            super::lzcomp::decode(&block).map(Some)
        } else {
            super::lzcomp::decode(&prefix[start..end]).map(Some)
        }
    }
}

/// Routes contiguous EOT payload bytes to the three MTX streams without
/// retaining their compressed representation. The caller may begin as soon as
/// the EOT and ten-byte MTX headers have arrived.
pub struct MtxStreamDecoder {
    ranges: [Range<usize>; 3],
    cursor: usize,
    index: usize,
    xor: bool,
    decoders: [super::lzcomp::StreamDecoder; 3],
    blocks: [Vec<u8>; 3],
}

impl MtxStreamDecoder {
    pub fn new(eot: &EotHeader, mtx: &super::mtx::MtxHeader) -> Self {
        let ranges = mtx
            .streams
            .clone()
            .map(|range| (eot.font_data.start + range.start)..(eot.font_data.start + range.end));
        Self {
            cursor: ranges[0].start,
            ranges,
            index: 0,
            xor: eot.flags & XOR_FLAG != 0,
            decoders: std::array::from_fn(|_| super::lzcomp::StreamDecoder::new()),
            blocks: std::array::from_fn(|_| Vec::new()),
        }
    }

    /// `offset` is the absolute byte position in the EOT resource. Skipping or
    /// replaying bytes is an error; a fetcher should send each new range once.
    pub fn push(&mut self, offset: usize, mut bytes: &[u8]) -> Result<(), &'static str> {
        if offset != self.cursor {
            return Err("non-contiguous MTX stream input");
        }
        while !bytes.is_empty() {
            while self.index < self.ranges.len() && self.cursor == self.ranges[self.index].end {
                self.decoders[self.index].finish()?;
                self.index += 1;
            }
            if self.index == self.ranges.len() {
                return Err("MTX input exceeds declared payload length");
            }
            let count = bytes.len().min(self.ranges[self.index].end - self.cursor);
            let chunk = &bytes[..count];
            let decoded = if self.xor {
                let clear: Vec<_> = chunk.iter().map(|byte| byte ^ 0x50).collect();
                self.decoders[self.index].push(&clear)?
            } else {
                self.decoders[self.index].push(chunk)?
            };
            self.blocks[self.index].extend(decoded);
            self.cursor += count;
            bytes = &bytes[count..];
        }
        Ok(())
    }

    pub fn finish(mut self) -> Result<[Vec<u8>; 3], &'static str> {
        while self.index < self.ranges.len() && self.cursor == self.ranges[self.index].end {
            self.decoders[self.index].finish()?;
            self.index += 1;
        }
        if self.index != self.ranges.len() {
            return Err("truncated MTX stream input");
        }
        Ok(self.blocks)
    }
}

fn take<'a>(
    prefix: &'a [u8],
    total_size: usize,
    cursor: &mut usize,
    count: usize,
) -> Result<Option<&'a [u8]>, &'static str> {
    let end = cursor.checked_add(count).ok_or("EOT length overflow")?;
    if end > total_size {
        return Err("EOT header exceeds declared size");
    }
    if end > prefix.len() {
        return Ok(None);
    }
    let start = *cursor;
    *cursor = end;
    Ok(Some(&prefix[start..end]))
}

/// `Ok(None)` means the prefix ends before the complete header, not that the
/// container is invalid. The returned payload range is valid before its bytes
/// have arrived, so a resource loader can reserve it immediately.
pub fn parse_prefix(prefix: &[u8]) -> Result<Option<EotHeader>, &'static str> {
    if prefix.len() < HEADER_FIXED_SIZE {
        return Ok(None);
    }
    let le16 = |offset| u16::from_le_bytes(prefix[offset..offset + 2].try_into().unwrap());
    let le32 = |offset| u32::from_le_bytes(prefix[offset..offset + 4].try_into().unwrap());
    let total_size = le32(0) as usize;
    let font_size = le32(4) as usize;
    let version = le32(8);
    let flags = le32(12);
    let embedding_flags = le16(EMBEDDING_FLAGS_OFFSET);
    if total_size < HEADER_FIXED_SIZE || font_size == 0 || font_size > total_size {
        return Err("invalid EOT size");
    }
    if le16(MAGIC_OFFSET) != 0x504c {
        return Err("invalid EOT magic");
    }
    if !matches!(version, 0x0001_0000 | 0x0002_0001 | 0x0002_0002) {
        return Err("unsupported EOT version");
    }

    let mut cursor = 80;
    macro_rules! need {
        ($count:expr) => {
            match take(prefix, total_size, &mut cursor, $count)? {
                Some(bytes) => bytes,
                None => return Ok(None),
            }
        };
    }
    if need!(2) != [0, 0] {
        return Err("invalid EOT header padding");
    }
    for name_index in 0..4 {
        let len = u16::from_le_bytes(need!(2).try_into().unwrap()) as usize;
        if len % 2 != 0 {
            return Err("invalid EOT UTF-16 length");
        }
        need!(len);
        if (name_index < 3 || version >= 0x0002_0001) && need!(2) != [0, 0] {
            return Err("invalid EOT header padding");
        }
    }

    let mut root_string = cursor..cursor;
    if version >= 0x0002_0001 {
        let len = u16::from_le_bytes(need!(2).try_into().unwrap()) as usize;
        if len % 2 != 0 {
            return Err("invalid EOT root string length");
        }
        let start = cursor;
        need!(len);
        root_string = start..cursor;
    }
    let mut root_checksum = None;
    if version >= 0x0002_0002 {
        root_checksum = Some(u32::from_le_bytes(need!(4).try_into().unwrap()));
        need!(4); // EUDCCodePage.
        if need!(2) != [0, 0] {
            return Err("invalid EOT header padding");
        }
        let signature_len = u16::from_le_bytes(need!(2).try_into().unwrap()) as usize;
        need!(signature_len);
        need!(4); // EUDCFlags.
        let eudc_len = u32::from_le_bytes(need!(4).try_into().unwrap()) as usize;
        need!(eudc_len);
    }
    if cursor.checked_add(font_size) != Some(total_size) {
        return Err("EOT font data does not follow header");
    }
    Ok(Some(EotHeader {
        total_size,
        font_data: cursor..total_size,
        root_string,
        version,
        flags,
        embedding_flags,
        root_checksum,
    }))
}

fn permitted_embedding(fs_type: u16) -> bool {
    let levels = fs_type & 0x000e;
    (levels == 0 || levels & 0x0008 != 0) && fs_type & 0x0200 == 0
}

fn root_matches(data: &[u8], header: &EotHeader, page_url: &str) -> bool {
    let root = &data[header.root_string.clone()];
    if let Some(expected) = header.root_checksum {
        let actual = root
            .iter()
            .fold(0u32, |sum, &byte| sum.wrapping_add(u32::from(byte)))
            ^ 0x5047_5342;
        if actual != expected {
            return false;
        }
    }
    if root.is_empty() {
        return true;
    }
    let units: Vec<u16> = root
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    let Ok(page) = reqwest::Url::parse(page_url) else {
        return false;
    };
    units.split(|&unit| unit == 0).any(|candidate| {
        let Ok(candidate) = String::from_utf16(candidate) else {
            return false;
        };
        let Ok(allowed) = reqwest::Url::parse(&candidate) else {
            return false;
        };
        let path = allowed.path();
        page.scheme() == allowed.scheme()
            && page.host_str() == allowed.host_str()
            && page.port_or_known_default() == allowed.port_or_known_default()
            && (path == "/"
                || page.path() == path
                || (page.path().starts_with(path)
                    && (path.ends_with('/')
                        || page.path().as_bytes().get(path.len()) == Some(&b'/'))))
    })
}

fn sfnt_embedding_flags(font: &[u8]) -> Option<u16> {
    let count = usize::from(u16::from_be_bytes(font.get(4..6)?.try_into().ok()?));
    for record in font
        .get(12..12usize.checked_add(count.checked_mul(16)?)?)?
        .chunks_exact(16)
    {
        if &record[..4] == b"OS/2" {
            let offset =
                usize::try_from(u32::from_be_bytes(record[8..12].try_into().ok()?)).ok()?;
            let length =
                usize::try_from(u32::from_be_bytes(record[12..16].try_into().ok()?)).ok()?;
            if length < 10 {
                return None;
            }
            let flags = font.get(offset.checked_add(8)?..offset.checked_add(10)?)?;
            return Some(u16::from_be_bytes(flags.try_into().ok()?));
        }
    }
    Some(0)
}

/// Decode an EOT for one document. The page URL and both copies of the
/// embedding rights are checked before the resulting font can be registered.
pub fn decode_for_page(data: &[u8], page_url: &str) -> Option<Vec<u8>> {
    decode_for_page_with_mtx_blocks(data, page_url, None)
}

/// Check the container-specific permissions before reusing a decoded SFNT.
/// SFNT embedding flags are validated when that SFNT first enters the cache.
pub(crate) fn permits_page(data: &[u8], page_url: &str) -> bool {
    parse_prefix(data)
        .ok()
        .flatten()
        .is_some_and(|header| permits_page_with_header(data, &header, page_url))
}

fn permits_page_with_header(data: &[u8], header: &EotHeader, page_url: &str) -> bool {
    data.len() == header.total_size
        && permitted_embedding(header.embedding_flags)
        && root_matches(data, header, page_url)
}

/// A fetcher may decompress MTX blocks while their bytes arrive. Rights are
/// still checked against the complete EOT resource and the requesting page.
pub fn decode_for_page_with_mtx_blocks(
    data: &[u8],
    page_url: &str,
    blocks: Option<[Vec<u8>; 3]>,
) -> Option<Vec<u8>> {
    let header = parse_prefix(data).ok()??;
    if !permits_page_with_header(data, &header, page_url) {
        return None;
    }
    let font = if header.flags & MTX_FLAG != 0 {
        let mtx = header.mtx_header(data).ok()??;
        let [primary, pushes, code] = if let Some(blocks) = blocks {
            blocks
        } else {
            let mut decoder = MtxStreamDecoder::new(&header, &mtx);
            let start = decoder.cursor;
            decoder
                .push(start, &data[start..header.font_data.end])
                .ok()?;
            decoder.finish().ok()?
        };
        super::ctf::Ctf::parse(&primary)
            .ok()?
            .reconstruct_sfnt(&pushes, &code)
            .ok()?
    } else {
        if blocks.is_some() {
            return None;
        }
        let mut bytes = data[header.font_data].to_vec();
        if header.flags & XOR_FLAG != 0 {
            for byte in &mut bytes {
                *byte ^= 0x50;
            }
        }
        bytes
    };
    super::woff2::validate_sfnt(&font)?;
    permitted_embedding(sfnt_embedding_flags(&font)?).then_some(font)
}

/// Decode only unrestricted, uncompressed EOT. MTX-compressed and root-bound
/// containers remain unsupported until their decoder and URL checks are wired.
pub fn decode_uncompressed(data: &[u8]) -> Option<Vec<u8>> {
    let header = parse_prefix(data).ok()??;
    if data.len() != header.total_size
        || header.flags & MTX_FLAG != 0
        || header.embedding_flags & 0x0002 != 0
        || !header.root_string.is_empty()
    {
        return None;
    }
    let mut font = data[header.font_data].to_vec();
    if header.flags & XOR_FLAG != 0 {
        for byte in &mut font {
            *byte ^= 0x50;
        }
    }
    matches!(
        font.get(..4),
        Some(b"OTTO" | b"ttcf" | b"true" | b"typ1" | b"\0\x01\0\0")
    )
    .then_some(font)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wrap_font(font: &[u8], version: u32, flags: u32) -> Vec<u8> {
        let mut header = vec![0; HEADER_FIXED_SIZE];
        header[8..12].copy_from_slice(&version.to_le_bytes());
        header[12..16].copy_from_slice(&flags.to_le_bytes());
        header[MAGIC_OFFSET..MAGIC_OFFSET + 2].copy_from_slice(&0x504cu16.to_le_bytes());
        for name_index in 0..4 {
            header.extend_from_slice(&0u16.to_le_bytes());
            if name_index < 3 || version >= 0x0002_0001 {
                header.extend_from_slice(&0u16.to_le_bytes());
            }
        }
        if version >= 0x0002_0001 {
            header.extend_from_slice(&0u16.to_le_bytes());
        }
        if version >= 0x0002_0002 {
            header.extend_from_slice(&[0; 8]);
            header.extend_from_slice(&0u16.to_le_bytes());
            header.extend_from_slice(&0u16.to_le_bytes());
            header.extend_from_slice(&0u32.to_le_bytes());
            header.extend_from_slice(&0u32.to_le_bytes());
        }
        let total_size = (header.len() + font.len()) as u32;
        header[0..4].copy_from_slice(&total_size.to_le_bytes());
        header[4..8].copy_from_slice(&(font.len() as u32).to_le_bytes());
        header.extend_from_slice(font);
        header
    }

    fn with_root(mut data: Vec<u8>, root: &str) -> Vec<u8> {
        let header = parse_prefix(&data).unwrap().unwrap();
        let encoded: Vec<u8> = root.encode_utf16().flat_map(u16::to_le_bytes).collect();
        let start = header.root_string.start;
        data[start - 2..start].copy_from_slice(&(encoded.len() as u16).to_le_bytes());
        data.splice(start..start, encoded.iter().copied());
        if header.version >= 0x0002_0002 {
            let checksum = encoded
                .iter()
                .fold(0u32, |sum, &byte| sum.wrapping_add(u32::from(byte)))
                ^ 0x5047_5342;
            data[start + encoded.len()..start + encoded.len() + 4]
                .copy_from_slice(&checksum.to_le_bytes());
        }
        let total_size = data.len() as u32;
        data[..4].copy_from_slice(&total_size.to_le_bytes());
        data
    }

    fn test_sfnt(fs_type: u16) -> Vec<u8> {
        let mut os2 = vec![0; 10];
        os2[8..10].copy_from_slice(&fs_type.to_be_bytes());
        super::super::woff2::build_sfnt(
            0x0001_0000,
            vec![
                (*b"head", vec![0; 54]),
                (*b"OS/2", os2),
                (*b"cmap", vec![0; 4]),
                (*b"hhea", vec![0; 36]),
                (*b"glyf", vec![0; 4]),
            ],
        )
        .unwrap()
    }

    #[test]
    fn strict_decode_checks_roots_checksums_and_embedding_rights() {
        let font = test_sfnt(0);
        let eot = with_root(
            wrap_font(&font, 0x0002_0002, 0),
            "https://example.com/articles/",
        );
        assert_eq!(
            decode_for_page(&eot, "https://example.com/articles/a"),
            Some(font.clone())
        );
        assert!(decode_for_page(&eot, "https://example.com/articles-malicious/a").is_none());
        assert!(decode_for_page(&eot, "https://example.com.evil/articles/a").is_none());
        assert!(decode_for_page(&eot, "http://example.com/articles/a").is_none());
        let mut tampered = eot.clone();
        let root = parse_prefix(&tampered).unwrap().unwrap().root_string;
        tampered[root.start] ^= 1;
        assert!(decode_for_page(&tampered, "https://example.com/articles/a").is_none());

        let mut restricted = eot.clone();
        restricted[EMBEDDING_FLAGS_OFFSET..EMBEDDING_FLAGS_OFFSET + 2]
            .copy_from_slice(&0x0002u16.to_le_bytes());
        assert!(decode_for_page(&restricted, "https://example.com/articles/a").is_none());

        let mut preview_only = eot.clone();
        preview_only[EMBEDDING_FLAGS_OFFSET..EMBEDDING_FLAGS_OFFSET + 2]
            .copy_from_slice(&0x0004u16.to_le_bytes());
        assert!(decode_for_page(&preview_only, "https://example.com/articles/a").is_none());

        let restricted_font = test_sfnt(0x0002);
        let restricted_eot = wrap_font(&restricted_font, 0x0002_0001, 0);
        assert!(decode_for_page(&restricted_eot, "https://example.com/").is_none());
    }

    #[test]
    fn header_can_be_parsed_before_payload_arrives() {
        let font = b"\0\x01\0\0test font payload";
        for version in [0x0001_0000, 0x0002_0001, 0x0002_0002] {
            let data = wrap_font(font, version, 0);
            let header_end = data.len() - font.len();
            assert_eq!(parse_prefix(&data[..HEADER_FIXED_SIZE - 1]), Ok(None));
            assert_eq!(parse_prefix(&data[..header_end - 1]), Ok(None));
            let header = parse_prefix(&data[..header_end]).unwrap().unwrap();
            assert_eq!(header.font_data, header_end..data.len());
            assert_eq!(decode_uncompressed(&data), Some(font.to_vec()));
        }
    }

    #[test]
    fn xor_and_malformed_lengths_do_not_escape_the_container() {
        let font = b"\0\x01\0\0test font payload";
        let mut encrypted = font.to_vec();
        for byte in &mut encrypted {
            *byte ^= 0x50;
        }
        let data = wrap_font(&encrypted, 0x0002_0002, XOR_FLAG);
        assert_eq!(decode_uncompressed(&data), Some(font.to_vec()));
        let compressed = wrap_font(font, 0x0002_0001, MTX_FLAG);
        assert!(decode_uncompressed(&compressed).is_none());
        let mut invalid = data.clone();
        invalid[4..8].copy_from_slice(&(u32::MAX).to_le_bytes());
        assert!(parse_prefix(&invalid).is_err());
        assert!(decode_uncompressed(&invalid).is_none());
    }

    #[test]
    fn compressed_stream_offsets_are_visible_before_the_payload_finishes() {
        let mut mtx = vec![3, 0, 1, 244, 0, 0, 13, 0, 0, 17];
        mtx.extend_from_slice(&[0; 13]);
        for flags in [MTX_FLAG, MTX_FLAG | XOR_FLAG] {
            let mut payload = mtx.clone();
            if flags & XOR_FLAG != 0 {
                for byte in &mut payload {
                    *byte ^= 0x50;
                }
            }
            let data = wrap_font(&payload, 0x0002_0001, flags);
            let header = parse_prefix(&data).unwrap().unwrap();
            let first_stream = header.font_data.start;
            assert_eq!(header.mtx_header(&data[..first_stream + 9]), Ok(None));
            let stream_header = header
                .mtx_header(&data[..first_stream + 10])
                .unwrap()
                .unwrap();
            assert_eq!(stream_header.streams, [10..13, 13..17, 17..23]);
        }
    }

    #[test]
    fn mtx_stream_assembler_handles_boundaries_and_xor() {
        let mut payload = vec![3, 0, 0, 0, 0, 0, 14, 0, 0, 18];
        payload.extend_from_slice(&[0; 12]); // three valid empty LZCOMP blocks
        for flags in [MTX_FLAG, MTX_FLAG | XOR_FLAG] {
            let mut encoded = payload.clone();
            if flags & XOR_FLAG != 0 {
                for byte in &mut encoded {
                    *byte ^= 0x50;
                }
            }
            let data = wrap_font(&encoded, 0x0002_0001, flags);
            let header = parse_prefix(&data).unwrap().unwrap();
            let mtx = header.mtx_header(&data).unwrap().unwrap();
            let mut decoder = MtxStreamDecoder::new(&header, &mtx);
            let start = decoder.cursor;
            assert!(decoder.push(start + 1, &data[start..start + 1]).is_err());
            for offset in start..header.font_data.end {
                decoder.push(offset, &data[offset..offset + 1]).unwrap();
            }
            assert_eq!(
                decoder.finish().unwrap(),
                [Vec::new(), Vec::new(), Vec::new()]
            );

            let mut truncated = MtxStreamDecoder::new(&header, &mtx);
            truncated
                .push(start, &data[start..header.font_data.end - 1])
                .unwrap();
            assert_eq!(truncated.finish(), Err("truncated MTX stream input"));
        }
    }

    #[test]
    fn real_compressed_eot_fixture_decodes() {
        let data = super::super::compressed_eot_fixture();
        let header = parse_prefix(&data).unwrap().unwrap();
        assert_eq!(header.total_size, data.len());
        let mtx = header.mtx_header(&data).unwrap().unwrap();
        assert_eq!(mtx.streams[2].end, header.font_data.len());
        if header.root_string.is_empty() && permitted_embedding(header.embedding_flags) {
            let font = decode_for_page(&data, "https://example.com/").unwrap();
            let mut fontdb = fontdb::Database::new();
            fontdb.load_font_data(font);
            assert!(fontdb.faces().next().is_some());
        }
        let first_end = header.font_data.start + mtx.streams[0].end;
        assert_eq!(
            header.decode_mtx_stream(&data[..first_end - 1], 0),
            Ok(None)
        );
        let first_block = header
            .decode_mtx_stream(&data[..first_end], 0)
            .unwrap()
            .unwrap();
        assert_eq!(&first_block[..4], b"\0\x01\0\0");
        let mut streamed = MtxStreamDecoder::new(&header, &mtx);
        let start = streamed.cursor;
        for (index, chunk) in data[start..header.font_data.end].chunks(7).enumerate() {
            streamed.push(start + index * 7, chunk).unwrap();
        }
        let blocks = streamed.finish().unwrap();
        for (index, block) in blocks.iter().enumerate() {
            assert_eq!(
                block,
                &header.decode_mtx_stream(&data, index).unwrap().unwrap()
            );
        }
        let mut encrypted_payload = data[header.font_data.clone()].to_vec();
        for byte in &mut encrypted_payload {
            *byte ^= 0x50;
        }
        let encrypted_eot = wrap_font(&encrypted_payload, 0x0002_0001, MTX_FLAG | XOR_FLAG);
        let encrypted_header = parse_prefix(&encrypted_eot).unwrap().unwrap();
        assert_eq!(
            encrypted_header
                .decode_mtx_stream(&encrypted_eot, 0)
                .unwrap()
                .unwrap(),
            first_block
        );
    }
}
