//! WOFF2 → sfnt.
//!
//! WOFF1 wraps each table in zlib and is a thin container; WOFF2 re-encodes the
//! font. Three things differ and all three have to be undone here:
//!
//!   * one Brotli stream over ALL table data rather than per-table zlib,
//!   * a table directory using variable-length integers and known-tag indices,
//!   * table transforms — `glyf`/`loca` split outlines across parallel streams,
//!     and `hmtx` can omit side bearings that must be reconstructed from glyph
//!     x-mins.
//!
//! Decoding here rather than through a library keeps the font on the same
//! streaming path as every other resource.

/// Byte reader that refuses to run off the end.
struct Reader<'a> {
    d: &'a [u8],
    p: usize,
}

impl<'a> Reader<'a> {
    fn new(d: &'a [u8]) -> Self {
        Self { d, p: 0 }
    }
    fn left(&self) -> usize {
        self.d.len().saturating_sub(self.p)
    }
    fn u8(&mut self) -> Option<u8> {
        let v = *self.d.get(self.p)?;
        self.p += 1;
        Some(v)
    }
    fn u16(&mut self) -> Option<u16> {
        let b = self.take(2)?;
        Some(u16::from_be_bytes([b[0], b[1]]))
    }
    fn i16(&mut self) -> Option<i16> {
        self.u16().map(|v| v as i16)
    }
    fn u32(&mut self) -> Option<u32> {
        let b = self.take(4)?;
        Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.p.checked_add(n)?;
        let s = self.d.get(self.p..end)?;
        self.p = end;
        Some(s)
    }
    /// `UIntBase128` — 1–5 bytes, seven bits each, high bit continues.
    fn base128(&mut self) -> Option<u32> {
        let mut v: u32 = 0;
        for i in 0..5 {
            let b = self.u8()?;
            // Leading zeroes and overflow are both malformed.
            if i == 0 && b == 0x80 {
                return None;
            }
            if v & 0xfe00_0000 != 0 {
                return None;
            }
            v = (v << 7) | (b & 0x7f) as u32;
            if b & 0x80 == 0 {
                return Some(v);
            }
        }
        None
    }
    /// `255UInt16` — a short integer in one to three bytes (WOFF2 §5.1).
    fn u255(&mut self) -> Option<u16> {
        let code = self.u8()?;
        match code {
            253 => self.u16(),
            255 => Some(self.u8()? as u16 + 253),
            254 => Some(self.u8()? as u16 + 506),
            v => Some(v as u16),
        }
    }
}

/// The 63 tags WOFF2 can name by index; 63 means a tag follows literally.
const KNOWN_TAGS: [&[u8; 4]; 63] = [
    b"cmap", b"head", b"hhea", b"hmtx", b"maxp", b"name", b"OS/2", b"post", b"cvt ", b"fpgm",
    b"glyf", b"loca", b"prep", b"CFF ", b"VORG", b"EBDT", b"EBLC", b"gasp", b"hdmx", b"kern",
    b"LTSH", b"PCLT", b"VDMX", b"vhea", b"vmtx", b"BASE", b"GDEF", b"GPOS", b"GSUB", b"EBSC",
    b"JSTF", b"MATH", b"CBDT", b"CBLC", b"COLR", b"CPAL", b"SVG ", b"sbix", b"acnt", b"avar",
    b"bdat", b"bloc", b"bsln", b"cvar", b"fdsc", b"feat", b"fmtx", b"fvar", b"gvar", b"hsty",
    b"just", b"lcar", b"mort", b"morx", b"opbd", b"prop", b"trak", b"Zapf", b"Silf", b"Glat",
    b"Gloc", b"Feat", b"Sill",
];

struct TableEntry {
    tag: [u8; 4],
    /// Length of the table once reconstructed.
    orig_len: u32,
    /// Length as stored in the Brotli stream.
    xform_len: u32,
    transformed: bool,
}

/// Decode a WOFF2 file into an sfnt (TrueType/OpenType) the font stack can read.
pub fn decode(data: &[u8]) -> Option<Vec<u8>> {
    let mut r = Reader::new(data);
    if r.take(4)? != b"wOF2" {
        return None;
    }
    let flavor = r.u32()?;
    let length = r.u32()? as usize;
    let num_tables = r.u16()? as usize;
    let _reserved = r.u16()?;
    let _total_sfnt_size = r.u32()? as usize;
    let total_compressed = r.u32()? as usize;
    let _major = r.u16()?;
    let _minor = r.u16()?;
    let meta_off = r.u32()?;
    let meta_len = r.u32()?;
    let meta_orig = r.u32()?;
    let priv_off = r.u32()?;
    let priv_len = r.u32()?;
    if length != data.len() || num_tables == 0 || num_tables > 4096 {
        return None;
    }

    // ── Table directory ──────────────────────────────────────────────────────
    let mut dir: Vec<TableEntry> = Vec::with_capacity(num_tables);
    for _ in 0..num_tables {
        let flags = r.u8()?;
        let idx = (flags & 0x3f) as usize;
        let xform_ver = (flags >> 6) & 0x3;
        let tag: [u8; 4] = if idx == 63 {
            let t = r.take(4)?;
            [t[0], t[1], t[2], t[3]]
        } else {
            **KNOWN_TAGS.get(idx)?
        };
        let orig_len = r.base128()?;
        let transformed = table_is_transformed(&tag, xform_ver)?;
        let xform_len = if transformed { r.base128()? } else { orig_len };
        dir.push(TableEntry {
            tag,
            orig_len,
            xform_len,
            transformed,
        });
    }

    // ── Collection directory (if flavor == ttcf, WOFF2 §4.2) ─────────────────
    let collection_dir = if flavor == 0x74746366 {
        let version = r.u32()?;
        let num_fonts = r.u255()? as usize;
        if num_fonts == 0 {
            return None;
        }
        let mut fonts = Vec::with_capacity(num_fonts);
        for _ in 0..num_fonts {
            let num_font_tables = r.u255()? as usize;
            let font_flavor = r.u32()?;
            let mut indices = Vec::with_capacity(num_font_tables);
            for _ in 0..num_font_tables {
                let idx = r.u255()? as usize;
                if idx >= dir.len() {
                    return None;
                }
                indices.push(idx);
            }
            fonts.push(CollectionFontEntry {
                flavor: font_flavor,
                table_indices: indices,
            });
        }
        // Table ordering constraints for font collections (WOFF2 §4.2, §5.5)
        for (i, entry) in dir.iter().enumerate() {
            if &entry.tag == b"glyf" {
                match dir.get(i + 1) {
                    Some(next) if &next.tag == b"loca" => {}
                    _ => return None,
                }
            }
        }
        for font in &fonts {
            let mut font_glyf = None;
            let mut font_loca = None;
            for &idx in &font.table_indices {
                if &dir[idx].tag == b"glyf" {
                    if font_glyf.is_some() {
                        return None;
                    }
                    font_glyf = Some(idx);
                } else if &dir[idx].tag == b"loca" {
                    if font_loca.is_some() {
                        return None;
                    }
                    font_loca = Some(idx);
                }
            }
            match (font_glyf, font_loca) {
                (Some(g), Some(l)) => {
                    if l != g + 1 {
                        return None;
                    }
                }
                (None, None) => {}
                _ => return None,
            }
        }
        Some(CollectionDirectory {
            _version: version,
            fonts,
        })
    } else {
        None
    };

    // ── One Brotli stream holding every table ────────────────────────────────
    let compressed_start = r.p;
    let compressed_end = compressed_start.checked_add(total_compressed)?;
    if compressed_end > data.len() {
        return None;
    }
    validate_side_blocks(
        data,
        compressed_end,
        meta_off,
        meta_len,
        meta_orig,
        priv_off,
        priv_len,
    )?;
    let comp = r.take(total_compressed)?;
    let mut raw: Vec<u8> = Vec::new();
    {
        use std::io::Read;
        let mut dec = brotli::Decompressor::new(comp, 8192);
        if dec.read_to_end(&mut raw).is_err() {
            return None;
        }
    }

    // Slice the decompressed stream into the tables, in directory order.
    let mut blobs: Vec<&[u8]> = Vec::with_capacity(dir.len());
    let mut off = 0usize;
    for e in &dir {
        let n = e.xform_len as usize;
        let end = off.checked_add(n)?;
        blobs.push(raw.get(off..end)?);
        off = end;
    }
    if off != raw.len() {
        return None;
    }

    // ── Validate loca tables (WOFF2 §5.3 conform-mustRejectLoca, §5.5 conform-tableOrdering) ─────
    for (i, e) in dir.iter().enumerate() {
        if &e.tag == b"glyf" {
            let loca_idx = if collection_dir.is_some() {
                i + 1
            } else {
                dir.iter().position(|t| &t.tag == b"loca")?
            };
            if loca_idx <= i {
                return None;
            }
            let loca_entry = dir.get(loca_idx)?;
            if loca_entry.tag != *b"loca" {
                return None;
            }
            if e.transformed != loca_entry.transformed {
                return None;
            }
            if loca_entry.transformed {
                if loca_entry.xform_len != 0 {
                    return None;
                }
                let glyf_blob = blobs.get(i)?;
                if glyf_blob.len() < 8 {
                    return None;
                }
                let num_glyphs = u16_at(glyf_blob, 4)? as u32;
                let index_format = u16_at(glyf_blob, 6)?;
                if index_format > 1 {
                    return None;
                }
                let expected_loca_orig_len =
                    (num_glyphs + 1) * if index_format == 0 { 2 } else { 4 };
                if loca_entry.orig_len != expected_loca_orig_len {
                    return None;
                }
            }
        } else if &e.tag == b"loca" {
            let has_preceding_glyf = dir[..i].iter().any(|t| &t.tag == b"glyf");
            if !has_preceding_glyf {
                return None;
            }
        }
    }

    // ── Undo the transforms ──────────────────────────────────────────────────
    let mut out_tables: Vec<([u8; 4], Vec<u8>)> = dir.iter().map(|e| (e.tag, Vec::new())).collect();
    for (i, e) in dir.iter().enumerate() {
        if &e.tag == b"glyf" && e.transformed {
            let loca_idx = if collection_dir.is_some() {
                i + 1
            } else {
                dir.iter().position(|t| &t.tag == b"loca")?
            };
            let index_to_loc = find_index_to_loc(&dir, &blobs);
            let (glyf, loca) = rebuild_glyf(blobs[i], index_to_loc)?;
            out_tables[i].1 = glyf;
            out_tables[loca_idx].1 = loca;
        } else if &e.tag == b"loca" && e.transformed {
            // Already reconstructed alongside glyf.
        } else if &e.tag == b"hmtx" && e.transformed {
            // Reconstructed below after prerequisite tables are populated.
        } else {
            let mut v = blobs[i].to_vec();
            v.truncate(e.orig_len as usize);
            out_tables[i].1 = v;
        }
    }

    // Reconstruct transformed hmtx tables
    for (i, e) in dir.iter().enumerate() {
        if &e.tag == b"hmtx" && e.transformed {
            let (glyf, loca, head, hhea, maxp) = if let Some(coll) = &collection_dir {
                let font = coll.fonts.iter().find(|f| f.table_indices.contains(&i))?;
                let glyf_idx = font
                    .table_indices
                    .iter()
                    .find(|&&idx| &dir[idx].tag == b"glyf")
                    .copied()?;
                let loca_idx = font
                    .table_indices
                    .iter()
                    .find(|&&idx| &dir[idx].tag == b"loca")
                    .copied()?;
                let head_idx = font
                    .table_indices
                    .iter()
                    .find(|&&idx| &dir[idx].tag == b"head")
                    .copied()?;
                let hhea_idx = font
                    .table_indices
                    .iter()
                    .find(|&&idx| &dir[idx].tag == b"hhea")
                    .copied()?;
                let maxp_idx = font
                    .table_indices
                    .iter()
                    .find(|&&idx| &dir[idx].tag == b"maxp")
                    .copied()?;
                (
                    &out_tables[glyf_idx].1[..],
                    &out_tables[loca_idx].1[..],
                    &out_tables[head_idx].1[..],
                    &out_tables[hhea_idx].1[..],
                    &out_tables[maxp_idx].1[..],
                )
            } else {
                (
                    find_out_table(&out_tables, b"glyf")?,
                    find_out_table(&out_tables, b"loca")?,
                    find_out_table(&out_tables, b"head")?,
                    find_out_table(&out_tables, b"hhea")?,
                    find_out_table(&out_tables, b"maxp")?,
                )
            };
            let num_glyphs = num_glyphs_from_maxp(maxp)?;
            let num_hmetrics = num_hmetrics_from_hhea(hhea)?;
            let xmins = glyf_x_mins(glyf, loca, index_to_loc_from_head(head), num_glyphs)?;
            let hmtx = rebuild_hmtx(blobs[i], num_hmetrics, num_glyphs, &xmins)?;
            out_tables[i].1 = hmtx;
        }
    }

    // Ensure every table has non-empty reconstructed data
    for (i, e) in dir.iter().enumerate() {
        if out_tables[i].1.is_empty() && e.orig_len != 0 {
            return None;
        }
    }

    if let Some(coll) = collection_dir {
        build_ttc(&coll, out_tables)
    } else {
        let sfnt = build_sfnt(flavor, out_tables)?;
        validate_sfnt(&sfnt)?;
        Some(sfnt)
    }
}

struct CollectionFontEntry {
    flavor: u32,
    table_indices: Vec<usize>,
}

struct CollectionDirectory {
    _version: u32,
    fonts: Vec<CollectionFontEntry>,
}

fn build_ttc(coll: &CollectionDirectory, out_tables: Vec<([u8; 4], Vec<u8>)>) -> Option<Vec<u8>> {
    let num_fonts = coll.fonts.len();
    let ttc_header_len = 12 + num_fonts * 4;

    let mut font_offsets = Vec::with_capacity(num_fonts);
    let mut curr_offset = ttc_header_len;
    for f in &coll.fonts {
        font_offsets.push(curr_offset as u32);
        curr_offset += 12 + f.table_indices.len() * 16;
    }

    let table_data_start = (curr_offset + 3) & !3;
    let mut table_offsets = Vec::with_capacity(out_tables.len());
    let mut table_checksums = Vec::with_capacity(out_tables.len());
    let mut data_offset = table_data_start;
    for (_, data) in &out_tables {
        table_offsets.push(data_offset as u32);
        table_checksums.push(checksum(data));
        data_offset = data_offset.checked_add((data.len().checked_add(3)?) & !3)?;
    }

    let mut out = Vec::with_capacity(data_offset);
    out.extend_from_slice(b"ttcf");
    out.extend_from_slice(&1u16.to_be_bytes()); // major
    out.extend_from_slice(&0u16.to_be_bytes()); // minor
    out.extend_from_slice(&(num_fonts as u32).to_be_bytes());
    for &off in &font_offsets {
        out.extend_from_slice(&off.to_be_bytes());
    }

    for f in &coll.fonts {
        let num_tables = f.table_indices.len();
        let (search_range, entry_selector, range_shift) = search_range_params(num_tables);
        out.extend_from_slice(&f.flavor.to_be_bytes());
        out.extend_from_slice(&(num_tables as u16).to_be_bytes());
        out.extend_from_slice(&search_range.to_be_bytes());
        out.extend_from_slice(&entry_selector.to_be_bytes());
        out.extend_from_slice(&range_shift.to_be_bytes());

        let mut sorted_indices = f.table_indices.clone();
        sorted_indices.sort_by_key(|&idx| out_tables[idx].0);

        for &idx in &sorted_indices {
            let (tag, ref data) = out_tables[idx];
            let offset = table_offsets[idx];
            let length = data.len() as u32;
            let csum = table_checksums[idx];
            out.extend_from_slice(&tag);
            out.extend_from_slice(&csum.to_be_bytes());
            out.extend_from_slice(&offset.to_be_bytes());
            out.extend_from_slice(&length.to_be_bytes());
        }
    }

    while out.len() < table_data_start {
        out.push(0);
    }

    for (i, (_, data)) in out_tables.iter().enumerate() {
        if out.len() != table_offsets[i] as usize {
            return None;
        }
        out.extend_from_slice(data);
        while out.len() % 4 != 0 {
            out.push(0);
        }
    }

    Some(out)
}

fn search_range_params(num_tables: usize) -> (u16, u16, u16) {
    let n = num_tables as u16;
    let mut entry_selector = 0u16;
    while (1u32 << (entry_selector + 1)) <= n as u32 {
        entry_selector += 1;
    }
    let search_range = (1u16 << entry_selector) * 16;
    let range_shift = n * 16 - search_range;
    (search_range, entry_selector, range_shift)
}

fn validate_side_blocks(
    data: &[u8],
    compressed_end: usize,
    meta_off: u32,
    meta_len: u32,
    meta_orig: u32,
    priv_off: u32,
    priv_len: u32,
) -> Option<()> {
    let file_len = data.len();
    let meta = validate_optional_block(file_len, meta_off, meta_len, meta_orig, true)?;
    let private = validate_optional_block(file_len, priv_off, priv_len, 0, false)?;

    // Extraneous data and alignment checks between blocks (WOFF2 §3)
    let mut current_end = compressed_end;

    if let Some((meta_start, meta_end)) = meta {
        let max_pad = (current_end + 3) & !3;
        if meta_start < current_end || meta_start > max_pad {
            return None;
        }
        if data[current_end..meta_start].iter().any(|&b| b != 0) {
            return None;
        }
        current_end = meta_end;
    }

    if let Some((priv_start, priv_end)) = private {
        let max_pad = (current_end + 3) & !3;
        if priv_start < current_end || priv_start > max_pad {
            return None;
        }
        if data[current_end..priv_start].iter().any(|&b| b != 0) {
            return None;
        }
        current_end = priv_end;
    }

    // Trailing padding beyond the last block: at most 3 null bytes for 4-byte alignment (WOFF2 §3)
    let max_file_len = (current_end + 3) & !3;
    if file_len < current_end || file_len > max_file_len {
        return None;
    }
    if data[current_end..file_len].iter().any(|&b| b != 0) {
        return None;
    }

    Some(())
}

fn validate_optional_block(
    file_len: usize,
    off: u32,
    len: u32,
    orig_len: u32,
    has_orig_len: bool,
) -> Option<Option<(usize, usize)>> {
    if off == 0 {
        if len != 0 || (has_orig_len && orig_len != 0) {
            return None;
        }
        return Some(None);
    }
    if len == 0 || (has_orig_len && orig_len == 0) {
        return None;
    }
    let start = off as usize;
    let end = start.checked_add(len as usize)?;
    if start >= file_len || end > file_len {
        return None;
    }
    Some(Some((start, end)))
}

fn table_is_transformed(tag: &[u8; 4], version: u8) -> Option<bool> {
    match (tag, version) {
        (b"glyf" | b"loca", 0) => Some(true),
        (b"glyf" | b"loca", 3) => Some(false),
        (b"glyf" | b"loca", _) => None,
        (b"hmtx", 0) => Some(false),
        (b"hmtx", 1) => Some(true),
        (b"hmtx", _) => None,
        (_, 0) => Some(false),
        _ => None,
    }
}

/// `head.indexToLocFormat`, needed to know how wide the rebuilt `loca` is.
fn find_index_to_loc(dir: &[TableEntry], blobs: &[&[u8]]) -> i16 {
    for (i, e) in dir.iter().enumerate() {
        if &e.tag == b"head" {
            if let Some(b) = blobs.get(i) {
                if b.len() >= 52 {
                    return i16::from_be_bytes([b[50], b[51]]);
                }
            }
        }
    }
    0
}

fn find_out_table<'a>(tables: &'a [([u8; 4], Vec<u8>)], tag: &[u8; 4]) -> Option<&'a [u8]> {
    tables.iter().find_map(|(t, data)| {
        if t == tag {
            Some(data.as_slice())
        } else {
            None
        }
    })
}

fn u16_at(data: &[u8], off: usize) -> Option<u16> {
    let b = data.get(off..off + 2)?;
    Some(u16::from_be_bytes([b[0], b[1]]))
}

fn i16_at(data: &[u8], off: usize) -> Option<i16> {
    u16_at(data, off).map(|v| v as i16)
}

fn u32_at(data: &[u8], off: usize) -> Option<u32> {
    let b = data.get(off..off + 4)?;
    Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

fn index_to_loc_from_head(head: &[u8]) -> i16 {
    if head.len() >= 52 {
        i16::from_be_bytes([head[50], head[51]])
    } else {
        0
    }
}

fn num_glyphs_from_maxp(maxp: &[u8]) -> Option<usize> {
    Some(u16_at(maxp, 4)? as usize)
}

fn num_hmetrics_from_hhea(hhea: &[u8]) -> Option<usize> {
    Some(u16_at(hhea, 34)? as usize)
}

fn loca_offsets(loca: &[u8], index_to_loc: i16, num_glyphs: usize) -> Option<Vec<u32>> {
    let count = num_glyphs.checked_add(1)?;
    let mut offsets = Vec::with_capacity(count);
    if index_to_loc == 0 {
        if loca.len() != count.checked_mul(2)? {
            return None;
        }
        for i in 0..count {
            offsets.push((u16_at(loca, i * 2)? as u32).checked_mul(2)?);
        }
    } else {
        if loca.len() != count.checked_mul(4)? {
            return None;
        }
        for i in 0..count {
            offsets.push(u32_at(loca, i * 4)?);
        }
    }
    for pair in offsets.windows(2) {
        if pair[0] > pair[1] {
            return None;
        }
    }
    Some(offsets)
}

fn glyf_x_mins(glyf: &[u8], loca: &[u8], index_to_loc: i16, num_glyphs: usize) -> Option<Vec<i16>> {
    let offsets = loca_offsets(loca, index_to_loc, num_glyphs)?;
    let mut xmins = Vec::with_capacity(num_glyphs);
    for i in 0..num_glyphs {
        let start = offsets[i] as usize;
        let end = offsets[i + 1] as usize;
        if start > end || end > glyf.len() {
            return None;
        }
        if start == end {
            xmins.push(0);
            continue;
        }
        if end - start < 10 {
            return None;
        }
        xmins.push(i16_at(glyf, start + 2)?);
    }
    Some(xmins)
}

fn rebuild_hmtx(
    data: &[u8],
    num_hmetrics: usize,
    num_glyphs: usize,
    xmins: &[i16],
) -> Option<Vec<u8>> {
    if num_hmetrics == 0 || num_hmetrics > num_glyphs || xmins.len() != num_glyphs {
        return None;
    }
    let mut r = Reader::new(data);
    let flags = r.u8()?;
    if flags == 0 || flags & !0x03 != 0 {
        return None;
    }

    let mut advance_widths = Vec::with_capacity(num_hmetrics);
    for _ in 0..num_hmetrics {
        advance_widths.push(r.u16()?);
    }

    let mut lsbs = Vec::with_capacity(num_hmetrics);
    if flags & 0x01 == 0 {
        for _ in 0..num_hmetrics {
            lsbs.push(r.i16()?);
        }
    }

    let trailing_count = num_glyphs - num_hmetrics;
    let mut trailing_lsbs = Vec::with_capacity(trailing_count);
    if flags & 0x02 == 0 {
        for _ in 0..trailing_count {
            trailing_lsbs.push(r.i16()?);
        }
    }
    if r.left() != 0 {
        return None;
    }

    let mut out = Vec::with_capacity(num_hmetrics * 4 + trailing_count * 2);
    for i in 0..num_hmetrics {
        out.extend_from_slice(&advance_widths[i].to_be_bytes());
        let lsb = if flags & 0x01 != 0 { xmins[i] } else { lsbs[i] };
        out.extend_from_slice(&lsb.to_be_bytes());
    }
    for i in num_hmetrics..num_glyphs {
        let lsb = if flags & 0x02 != 0 {
            xmins[i]
        } else {
            trailing_lsbs[i - num_hmetrics]
        };
        out.extend_from_slice(&lsb.to_be_bytes());
    }
    Some(out)
}

// ─── The glyf transform ───────────────────────────────────────────────────────

/// Rebuild `glyf` and `loca` from the transformed representation.
///
/// The transform splits outlines across parallel streams — contour counts,
/// point counts, flags, coordinate deltas, composites, bounding boxes and
/// instructions — and drops `loca` entirely, so both tables are reconstructed
/// glyph by glyph.
fn rebuild_glyf(data: &[u8], index_to_loc: i16) -> Option<(Vec<u8>, Vec<u8>)> {
    let mut h = Reader::new(data);
    let _reserved = h.u16()?;
    let option_flags = h.u16()?;
    let num_glyphs = h.u16()? as usize;
    let _index_format = h.u16()?;
    let n_contour_size = h.u32()? as usize;
    let n_points_size = h.u32()? as usize;
    let flag_size = h.u32()? as usize;
    let glyph_size = h.u32()? as usize;
    let composite_size = h.u32()? as usize;
    let bbox_size = h.u32()? as usize;
    let instr_size = h.u32()? as usize;
    // An optional stream marks simple glyphs whose contours may overlap (WOFF2 §5.1)
    let overlap_size = if option_flags & 1 != 0 {
        (num_glyphs + 7) / 8
    } else {
        0
    };

    let base = h.p;
    let mut at = base;
    let mut slice = |n: usize| -> Option<&[u8]> {
        let s = data.get(at..at + n)?;
        at += n;
        Some(s)
    };
    let mut n_contour = Reader::new(slice(n_contour_size)?);
    let mut n_points = Reader::new(slice(n_points_size)?);
    let flags_all = slice(flag_size)?;
    let mut glyph_str = Reader::new(slice(glyph_size)?);
    let mut composite = Reader::new(slice(composite_size)?);
    let bbox_all = slice(bbox_size)?;
    let instr_all = slice(instr_size)?;
    let overlap_all = if overlap_size > 0 {
        Some(slice(overlap_size)?)
    } else {
        None
    };

    // The total number of bytes in bboxBitmap is equal to 4 * floor((numGlyphs + 31) / 32) (WOFF2 §5.1)
    let bitmap_len = ((num_glyphs + 31) / 32) * 4;
    let bbox_bitmap = bbox_all.get(..bitmap_len)?;
    let mut bbox_vals = Reader::new(bbox_all.get(bitmap_len..)?);

    let mut flags_at = 0usize;
    let mut instr_at = 0usize;
    let mut glyf: Vec<u8> = Vec::with_capacity(data.len() * 2);
    let mut loca: Vec<u32> = Vec::with_capacity(num_glyphs + 1);

    for gid in 0..num_glyphs {
        loca.push(glyf.len() as u32);
        let n = n_contour.i16()?;
        let has_bbox = bbox_bitmap[gid / 8] & (0x80 >> (gid % 8)) != 0;

        if n == 0 {
            // An empty glyph occupies no bytes at all and must not have an explicit bbox.
            if has_bbox {
                return None;
            }
            continue;
        }

        if n < 0 {
            // ── Composite ────────────────────────────────────────────────────
            // Its bounding box is never derived, so it must be present.
            if !has_bbox {
                return None;
            }
            let b = bbox_vals.take(8)?;
            let start = glyf.len();
            glyf.extend_from_slice(&(-1i16).to_be_bytes());
            glyf.extend_from_slice(b);
            let mut have_instructions = false;
            loop {
                let flags = composite.u16()?;
                let glyph_index = composite.u16()?;
                glyf.extend_from_slice(&flags.to_be_bytes());
                glyf.extend_from_slice(&glyph_index.to_be_bytes());
                // ARG_1_AND_2_ARE_WORDS
                let arg_bytes = if flags & 0x0001 != 0 { 4 } else { 2 };
                glyf.extend_from_slice(composite.take(arg_bytes)?);
                // One of the scale forms may follow.
                let scale_bytes = if flags & 0x0008 != 0 {
                    2
                }
                // WE_HAVE_A_SCALE
                else if flags & 0x0040 != 0 {
                    4
                }
                // X_AND_Y_SCALE
                else if flags & 0x0080 != 0 {
                    8
                }
                // TWO_BY_TWO
                else {
                    0
                };
                if scale_bytes > 0 {
                    glyf.extend_from_slice(composite.take(scale_bytes)?);
                }
                if flags & 0x0100 != 0 {
                    have_instructions = true;
                } // WE_HAVE_INSTRUCTIONS
                if flags & 0x0020 == 0 {
                    break;
                } // MORE_COMPONENTS
            }
            if have_instructions {
                let len = glyph_str.u255()? as usize;
                glyf.extend_from_slice(&(len as u16).to_be_bytes());
                glyf.extend_from_slice(instr_all.get(instr_at..instr_at + len)?);
                instr_at += len;
            }
            pad4(&mut glyf, start);
            continue;
        }

        // ── Simple glyph ─────────────────────────────────────────────────────
        let n_contours = n as usize;
        let mut end_pts: Vec<u16> = Vec::with_capacity(n_contours);
        let mut total = 0usize;
        for _ in 0..n_contours {
            let pts = n_points.u255()? as usize;
            total += pts;
            if total == 0 || total > 0xffff {
                return None;
            }
            end_pts.push((total - 1) as u16);
        }

        // Coordinates arrive as (flag, delta) triplets: the flag's low seven
        // bits pick how many bytes follow and how they split between x and y.
        let mut xs: Vec<i16> = Vec::with_capacity(total);
        let mut ys: Vec<i16> = Vec::with_capacity(total);
        let mut on_curve: Vec<bool> = Vec::with_capacity(total);
        let (mut x, mut y) = (0i32, 0i32);
        for _ in 0..total {
            let f = *flags_all.get(flags_at)?;
            flags_at += 1;
            on_curve.push(f & 0x80 == 0);
            let (dx, dy) = triplet(&mut glyph_str, f & 0x7f)?;
            x += dx;
            y += dy;
            if x < i16::MIN as i32 || x > i16::MAX as i32 {
                return None;
            }
            if y < i16::MIN as i32 || y > i16::MAX as i32 {
                return None;
            }
            xs.push(x as i16);
            ys.push(y as i16);
        }

        let instr_len = glyph_str.u255()? as usize;
        let instructions = instr_all.get(instr_at..instr_at + instr_len)?;
        instr_at += instr_len;

        let start = glyf.len();
        glyf.extend_from_slice(&(n_contours as i16).to_be_bytes());
        if has_bbox {
            glyf.extend_from_slice(bbox_vals.take(8)?);
        } else {
            // Derived from the points, which is what the encoder omitted it for.
            let x0 = xs.iter().copied().min().unwrap_or(0);
            let y0 = ys.iter().copied().min().unwrap_or(0);
            let x1 = xs.iter().copied().max().unwrap_or(0);
            let y1 = ys.iter().copied().max().unwrap_or(0);
            for v in [x0, y0, x1, y1] {
                glyf.extend_from_slice(&v.to_be_bytes());
            }
        }
        for e in &end_pts {
            glyf.extend_from_slice(&e.to_be_bytes());
        }
        glyf.extend_from_slice(&(instr_len as u16).to_be_bytes());
        glyf.extend_from_slice(instructions);
        let has_overlap_bit = if let Some(bitmap) = overlap_all {
            bitmap
                .get(gid / 8)
                .map(|b| b & (0x80 >> (gid % 8)) != 0)
                .unwrap_or(false)
        } else {
            false
        };
        write_simple_outline(&mut glyf, &xs, &ys, &on_curve, has_overlap_bit);
        pad4(&mut glyf, start);
    }
    loca.push(glyf.len() as u32);

    // `loca` is short-format when head says so, and each offset is halved.
    let mut loca_bytes = Vec::with_capacity(loca.len() * 4);
    if index_to_loc == 0 {
        for v in &loca {
            if v % 2 != 0 || v / 2 > u16::MAX as u32 {
                return None;
            }
            loca_bytes.extend_from_slice(&((v / 2) as u16).to_be_bytes());
        }
    } else {
        for v in &loca {
            loca_bytes.extend_from_slice(&v.to_be_bytes());
        }
    }
    Some((glyf, loca_bytes))
}

/// Pad a glyph to a four-byte boundary, as `loca` offsets assume.
fn pad4(buf: &mut Vec<u8>, start: usize) {
    while (buf.len() - start) % 4 != 0 {
        buf.push(0);
    }
}

fn triplet(r: &mut Reader<'_>, code: u8) -> Option<(i32, i32)> {
    let flag = code as usize;
    if flag < 10 {
        // dx is zero; dy is one byte with the sign in the code.
        let b = r.u8()? as i32;
        let dy = ((flag & 14) << 7) as i32 + b;
        Some((0, with_sign(flag, dy)))
    } else if flag < 20 {
        let b = r.u8()? as i32;
        let dx = (((flag - 10) & 14) << 7) as i32 + b;
        Some((with_sign(flag, dx), 0))
    } else if flag < 84 {
        let b0 = (flag - 20) as i32;
        let b1 = r.u8()? as i32;
        let dx = 1 + (b0 & 0x30) + (b1 >> 4);
        let dy = 1 + ((b0 & 0x0c) << 2) + (b1 & 0x0f);
        Some((with_sign(flag, dx), with_sign(flag >> 1, dy)))
    } else if flag < 120 {
        let b0 = (flag - 84) as i32;
        let b1 = r.u8()? as i32;
        let b2 = r.u8()? as i32;
        let dx = 1 + ((b0 / 12) << 8) + b1;
        let dy = 1 + (((b0 % 12) >> 2) << 8) + b2;
        Some((with_sign(flag, dx), with_sign(flag >> 1, dy)))
    } else if flag < 124 {
        let b0 = r.u8()? as i32;
        let b1 = r.u8()? as i32;
        let b2 = r.u8()? as i32;
        let dx = (b0 << 4) + (b1 >> 4);
        let dy = ((b1 & 0x0f) << 8) + b2;
        Some((with_sign(flag, dx), with_sign(flag >> 1, dy)))
    } else {
        let b0 = r.u8()? as i32;
        let b1 = r.u8()? as i32;
        let b2 = r.u8()? as i32;
        let b3 = r.u8()? as i32;
        let dx = (b0 << 8) + b1;
        let dy = (b2 << 8) + b3;
        Some((with_sign(flag, dx), with_sign(flag >> 1, dy)))
    }
}

fn with_sign(flag: usize, baseval: i32) -> i32 {
    if flag & 1 != 0 { baseval } else { -baseval }
}

/// Write a simple glyph's flags and coordinates in the sfnt encoding.
///
/// The transform stores every delta at full width; the sfnt format packs them,
/// using a short form and a repeat flag. Emitting the long form for everything
/// is valid and keeps the reconstruction honest — no encoder cleverness where a
/// mistake would silently distort outlines.
fn write_simple_outline(
    buf: &mut Vec<u8>,
    xs: &[i16],
    ys: &[i16],
    on_curve: &[bool],
    has_overlap_bit: bool,
) {
    const ON_CURVE: u8 = 0x01;
    const X_SHORT: u8 = 0x02;
    const Y_SHORT: u8 = 0x04;
    const X_SAME_OR_POSITIVE: u8 = 0x10;
    const Y_SAME_OR_POSITIVE: u8 = 0x20;
    const OVERLAP_SIMPLE: u8 = 0x40;

    // Deltas between consecutive points, which is what the format stores.
    let n = xs.len();
    let mut dxs: Vec<i32> = Vec::with_capacity(n);
    let mut dys: Vec<i32> = Vec::with_capacity(n);
    let (mut px, mut py) = (0i32, 0i32);
    for i in 0..n {
        dxs.push(xs[i] as i32 - px);
        dys.push(ys[i] as i32 - py);
        px = xs[i] as i32;
        py = ys[i] as i32;
    }

    for i in 0..n {
        let mut f = if on_curve[i] { ON_CURVE } else { 0 };
        if has_overlap_bit && i == 0 {
            f |= OVERLAP_SIMPLE;
        }
        let dx = dxs[i];
        let dy = dys[i];
        if dx == 0 {
            f |= X_SAME_OR_POSITIVE;
        } else if (-255..=255).contains(&dx) {
            f |= X_SHORT;
            if dx > 0 {
                f |= X_SAME_OR_POSITIVE;
            }
        }
        if dy == 0 {
            f |= Y_SAME_OR_POSITIVE;
        } else if (-255..=255).contains(&dy) {
            f |= Y_SHORT;
            if dy > 0 {
                f |= Y_SAME_OR_POSITIVE;
            }
        }
        buf.push(f);
    }
    for i in 0..n {
        let dx = dxs[i];
        if dx == 0 {
            continue;
        }
        if (-255..=255).contains(&dx) {
            buf.push(dx.unsigned_abs() as u8);
        } else {
            buf.extend_from_slice(&(dx as i16).to_be_bytes());
        }
    }
    for i in 0..n {
        let dy = dys[i];
        if dy == 0 {
            continue;
        }
        if (-255..=255).contains(&dy) {
            buf.push(dy.unsigned_abs() as u8);
        } else {
            buf.extend_from_slice(&(dy as i16).to_be_bytes());
        }
    }
}

// ─── sfnt assembly ────────────────────────────────────────────────────────────

/// Assemble the reconstructed tables into an sfnt file.
fn build_sfnt(flavor: u32, mut tables: Vec<([u8; 4], Vec<u8>)>) -> Option<Vec<u8>> {
    // The directory is sorted by tag; the data may sit in any order, so it
    // follows the same one.
    tables.sort_by(|a, b| a.0.cmp(&b.0));
    for (tag, data) in &mut tables {
        if tag == b"head" && data.len() >= 12 {
            // The sfnt checksum covers table offsets and padding, so the
            // original value from the WOFF2 source is no longer valid after we
            // assemble a fresh container.
            data[8..12].fill(0);
        }
    }
    let n = tables.len() as u16;
    if n == 0 {
        return None;
    }

    // searchRange / entrySelector / rangeShift: the binary-search hints in the
    // header. Wrong values make some parsers reject the font outright.
    let mut entry_selector = 0u16;
    while (1u32 << (entry_selector + 1)) <= n as u32 {
        entry_selector += 1;
    }
    let search_range = (1u16 << entry_selector) * 16;
    let range_shift = n * 16 - search_range;

    let header = 12usize.checked_add(16usize.checked_mul(tables.len())?)?;
    let data_len = tables
        .iter()
        .map(|t| t.1.len().checked_add(3).map(|n| n & !3))
        .try_fold(0usize, |acc, n| acc.checked_add(n?))?;
    let mut out = Vec::with_capacity(header.checked_add(data_len)?);
    out.extend_from_slice(&flavor.to_be_bytes());
    out.extend_from_slice(&n.to_be_bytes());
    out.extend_from_slice(&search_range.to_be_bytes());
    out.extend_from_slice(&entry_selector.to_be_bytes());
    out.extend_from_slice(&range_shift.to_be_bytes());

    let mut offset = u32::try_from(header).ok()?;
    let mut records: Vec<(u32, u32)> = Vec::with_capacity(tables.len());
    for (_, data) in &tables {
        let len = u32::try_from(data.len()).ok()?;
        records.push((offset, len));
        let padded = u32::try_from(data.len().checked_add(3)? & !3).ok()?;
        offset = offset.checked_add(padded)?;
    }
    for (i, (tag, data)) in tables.iter().enumerate() {
        let (off, len) = records[i];
        out.extend_from_slice(tag);
        out.extend_from_slice(&checksum(data).to_be_bytes());
        out.extend_from_slice(&off.to_be_bytes());
        out.extend_from_slice(&len.to_be_bytes());
    }
    for (_, data) in &tables {
        out.extend_from_slice(data);
        while out.len() % 4 != 0 {
            out.push(0);
        }
    }
    if let Some(head_record) = find_table_record(&out, b"head") {
        let adjustment_offset = head_record.offset as usize + 8;
        if adjustment_offset + 4 <= out.len() {
            let adjustment = 0xB1B0_AFBAu32.wrapping_sub(checksum(&out));
            out[adjustment_offset..adjustment_offset + 4]
                .copy_from_slice(&adjustment.to_be_bytes());
        }
    }
    Some(out)
}

struct SfntTableRecord {
    offset: u32,
}

fn find_table_record(sfnt: &[u8], tag: &[u8; 4]) -> Option<SfntTableRecord> {
    if sfnt.len() < 12 {
        return None;
    }
    let count = u16::from_be_bytes([sfnt[4], sfnt[5]]) as usize;
    for i in 0..count {
        let base = 12 + i * 16;
        let record = sfnt.get(base..base + 16)?;
        if &record[0..4] == tag {
            let offset = u32::from_be_bytes([record[8], record[9], record[10], record[11]]);
            return Some(SfntTableRecord { offset });
        }
    }
    None
}

fn validate_sfnt(sfnt: &[u8]) -> Option<()> {
    if sfnt.len() < 12 {
        return None;
    }
    let flavor = &sfnt[0..4];
    if flavor != b"OTTO" && flavor != b"true" && flavor != [0x00, 0x01, 0x00, 0x00] {
        return None;
    }
    let count = u16::from_be_bytes([sfnt[4], sfnt[5]]) as usize;
    if count == 0 || sfnt.len() < 12 + 16 * count {
        return None;
    }

    let mut records = Vec::with_capacity(count);
    let mut has_head = false;
    let mut has_cmap = false;
    let mut has_horizontal_metrics = false;
    let mut has_outline = false;
    let mut prev_tag: Option<[u8; 4]> = None;
    for i in 0..count {
        let base = 12 + i * 16;
        let record = sfnt.get(base..base + 16)?;
        let tag = [record[0], record[1], record[2], record[3]];
        if let Some(prev) = prev_tag {
            if prev >= tag {
                return None;
            }
        }
        prev_tag = Some(tag);
        let offset = u32::from_be_bytes([record[8], record[9], record[10], record[11]]) as usize;
        let length = u32::from_be_bytes([record[12], record[13], record[14], record[15]]) as usize;
        let padded_end = offset.checked_add(length.checked_add(3)? & !3)?;
        if offset < 12 + 16 * count || offset.checked_add(length)? > sfnt.len() {
            return None;
        }
        if padded_end > sfnt.len() {
            return None;
        }
        records.push((offset, padded_end));
        has_head |= &tag == b"head";
        has_cmap |= &tag == b"cmap";
        has_horizontal_metrics |= &tag == b"hhea" || &tag == b"hmtx";
        has_outline |= &tag == b"glyf" || &tag == b"CFF " || &tag == b"CFF2";
    }
    records.sort_unstable();
    for pair in records.windows(2) {
        if pair[0].1 > pair[1].0 {
            return None;
        }
    }
    if !has_head || !has_cmap || !has_horizontal_metrics || !has_outline {
        return None;
    }
    if checksum(sfnt) != 0xB1B0_AFBA {
        return None;
    }
    Some(())
}

/// A table checksum: the sum of its big-endian u32 words, zero-padded.
fn checksum(data: &[u8]) -> u32 {
    let mut sum: u32 = 0;
    let mut chunks = data.chunks_exact(4);
    for c in &mut chunks {
        sum = sum.wrapping_add(u32::from_be_bytes([c[0], c[1], c[2], c[3]]));
    }
    let rem = chunks.remainder();
    if !rem.is_empty() {
        let mut last = [0u8; 4];
        last[..rem.len()].copy_from_slice(rem);
        sum = sum.wrapping_add(u32::from_be_bytes(last));
    }
    sum
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn push_base128(out: &mut Vec<u8>, value: u32) {
        assert!(value < 128);
        out.push(value as u8);
    }

    fn patch_u32(data: &mut [u8], off: usize, value: u32) {
        data[off..off + 4].copy_from_slice(&value.to_be_bytes());
    }

    #[test]
    fn unsupported_transformed_tables_fail_closed() {
        let mut compressed = Vec::new();
        {
            let mut writer = brotli::CompressorWriter::new(&mut compressed, 4096, 5, 22);
            writer.write_all(&[0]).unwrap();
        }

        let mut woff = Vec::new();
        woff.extend_from_slice(b"wOF2");
        woff.extend_from_slice(&0x0001_0000u32.to_be_bytes());
        woff.extend_from_slice(&0u32.to_be_bytes());
        woff.extend_from_slice(&1u16.to_be_bytes());
        woff.extend_from_slice(&0u16.to_be_bytes());
        woff.extend_from_slice(&28u32.to_be_bytes());
        woff.extend_from_slice(&(compressed.len() as u32).to_be_bytes());
        woff.extend_from_slice(&1u16.to_be_bytes());
        woff.extend_from_slice(&0u16.to_be_bytes());
        woff.extend_from_slice(&0u32.to_be_bytes());
        woff.extend_from_slice(&0u32.to_be_bytes());
        woff.extend_from_slice(&0u32.to_be_bytes());
        woff.extend_from_slice(&0u32.to_be_bytes());
        woff.extend_from_slice(&0u32.to_be_bytes());
        woff.push(0x40 | 3); // hmtx, transform version 1.
        push_base128(&mut woff, 4);
        push_base128(&mut woff, 1);
        woff.extend_from_slice(&compressed);

        assert!(
            decode(&woff).is_none(),
            "unsupported transformed metrics table must not be passed through as an sfnt"
        );
    }

    #[test]
    fn malformed_transformed_glyf_fails_closed() {
        let mut woff = Vec::new();
        woff.extend_from_slice(b"wOF2");
        woff.extend_from_slice(&0x0001_0000u32.to_be_bytes());
        woff.extend_from_slice(&0u32.to_be_bytes());
        woff.extend_from_slice(&1u16.to_be_bytes());
        woff.extend_from_slice(&0u16.to_be_bytes());
        woff.extend_from_slice(&28u32.to_be_bytes());
        woff.extend_from_slice(&0u32.to_be_bytes());
        woff.extend_from_slice(&1u16.to_be_bytes());
        woff.extend_from_slice(&0u16.to_be_bytes());
        woff.extend_from_slice(&0u32.to_be_bytes());
        woff.extend_from_slice(&0u32.to_be_bytes());
        woff.extend_from_slice(&0u32.to_be_bytes());
        woff.extend_from_slice(&0u32.to_be_bytes());
        woff.extend_from_slice(&0u32.to_be_bytes());
        woff.push(10); // glyf, transform version 0: transformed.
        push_base128(&mut woff, 1);

        assert!(
            decode(&woff).is_none(),
            "transformed glyf/loca must not register a corrupt sfnt"
        );
    }

    #[test]
    fn hmtx_transform_reconstructs_missing_bearings_from_glyf_xmins() {
        let data = [0x03, 0x00, 0x64, 0x00, 0xc8];
        let hmtx = rebuild_hmtx(&data, 2, 3, &[10, -5, 7]).expect("hmtx should rebuild");

        assert_eq!(
            hmtx,
            vec![
                0x00, 0x64, 0x00, 0x0a, // glyph 0: advance, xMin as lsb
                0x00, 0xc8, 0xff, 0xfb, // glyph 1: advance, xMin as lsb
                0x00, 0x07, // glyph 2: trailing lsb from xMin
            ]
        );
    }

    #[test]
    fn hmtx_transform_rejects_invalid_flags() {
        assert!(rebuild_hmtx(&[0x00, 0x00, 0x64], 1, 1, &[0]).is_none());
        assert!(rebuild_hmtx(&[0x04, 0x00, 0x64], 1, 1, &[0]).is_none());
    }

    #[test]
    fn transform_versions_are_table_specific() {
        assert_eq!(table_is_transformed(b"glyf", 0), Some(true));
        assert_eq!(table_is_transformed(b"glyf", 3), Some(false));
        assert_eq!(table_is_transformed(b"glyf", 1), None);
        assert_eq!(table_is_transformed(b"hmtx", 1), Some(true));
        assert_eq!(table_is_transformed(b"hmtx", 2), None);
        assert_eq!(table_is_transformed(b"cmap", 0), Some(false));
        assert_eq!(table_is_transformed(b"cmap", 1), None);
    }

    #[test]
    fn real_woff2_sample_passes_container_validation() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../data/wpt/fonts/kinter.woff2");
        let Ok(data) = std::fs::read(&path) else {
            return;
        };

        let _ = decode(&data);
    }

    #[test]
    fn woff2_header_length_must_match_file() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../data/wpt/fonts/kinter.woff2");
        let Ok(mut data) = std::fs::read(&path) else {
            return;
        };
        let bad_len = (data.len() as u32).wrapping_add(1);
        patch_u32(&mut data, 8, bad_len);

        assert!(decode(&data).is_none());
    }

    #[test]
    fn woff2_reconstructed_sfnt_size_does_not_reject_mismatched_totalsfntsize() {
        // WOFF2 §3.2 conform-mustNotRejectIncorrectTotalSize:
        // "User agents MUST NOT reject correctly decoded font file if the resulting
        // font file size doesn't match the totalSfntSize value encoded as part of the WOFF2 header."
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../data/wpt/fonts/kinter.woff2");
        let Ok(mut data) = std::fs::read(&path) else {
            return;
        };
        let bad_size = u32::from_be_bytes([data[16], data[17], data[18], data[19]]).wrapping_add(4);
        patch_u32(&mut data, 16, bad_size);

        assert!(decode(&data).is_some());
    }

    #[test]
    fn woff2_side_blocks_must_not_overlap_table_data() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../data/wpt/fonts/kinter.woff2");
        let Ok(mut data) = std::fs::read(&path) else {
            return;
        };
        patch_u32(&mut data, 28, 49); // metaOffset inside the compressed table stream.
        patch_u32(&mut data, 32, 4);
        patch_u32(&mut data, 36, 4);

        assert!(decode(&data).is_none());
    }

    #[test]
    fn side_block_validation_rejects_overlap_and_inconsistent_absence() {
        let b60 = vec![0; 60];
        let b90 = vec![0; 90];
        let b100 = vec![0; 100];
        assert!(validate_side_blocks(&b60, 60, 0, 0, 0, 0, 0).is_some());
        assert!(validate_side_blocks(&b100, 60, 0, 0, 0, 0, 0).is_none());
        assert!(validate_side_blocks(&b100, 60, 42, 8, 8, 0, 0).is_none());
        assert!(validate_side_blocks(&b100, 60, 20, 8, 8, 0, 0).is_none());
        assert!(validate_side_blocks(&b100, 60, 0, 4, 4, 0, 0).is_none());
        assert!(validate_side_blocks(&b100, 60, 70, 8, 8, 74, 8).is_none());
        assert!(validate_side_blocks(&b90, 60, 60, 10, 10, 72, 18).is_some());
    }

    #[test]
    fn build_sfnt_recomputes_head_checksum_adjustment() {
        let mut head = vec![0; 54];
        head[8..12].copy_from_slice(&0x1234_5678u32.to_be_bytes());
        let sfnt = build_sfnt(0x0001_0000, vec![(*b"head", head), (*b"maxp", vec![0; 6])])
            .expect("sfnt should assemble");

        assert_eq!(checksum(&sfnt), 0xB1B0_AFBA);
        let head_record = find_table_record(&sfnt, b"head").unwrap();
        let adjustment_offset = head_record.offset as usize + 8;
        assert_ne!(
            &sfnt[adjustment_offset..adjustment_offset + 4],
            &0x1234_5678u32.to_be_bytes()
        );
    }
}
