//! Compact Table Format directory inspection. MTX's primary LZCOMP block is
//! an SFNT-like table directory with several transformed table payloads.

use std::ops::Range;

const DIRECTORY_HEADER: usize = 12;
const TABLE_RECORD: usize = 16;
const MAX_TABLES: usize = 4096;
const MAX_HDMX_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Table {
    pub tag: [u8; 4],
    pub checksum: u32,
    pub bytes: Range<usize>,
}

pub struct Ctf<'a> {
    data: &'a [u8],
    pub sfnt_version: [u8; 4],
    pub tables: Vec<Table>,
}

fn be16(bytes: &[u8]) -> u16 {
    u16::from_be_bytes(bytes.try_into().unwrap())
}

fn be32(bytes: &[u8]) -> u32 {
    u32::from_be_bytes(bytes.try_into().unwrap())
}

pub struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Triplet {
    pub on_curve: bool,
    pub dx: i16,
    pub dy: i16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Point {
    pub x: i16,
    pub y: i16,
    pub on_curve: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GlyphKind<'a> {
    Simple {
        contour_endpoints: Vec<u16>,
        points: Vec<Point>,
    },
    Composite {
        components: &'a [u8],
        has_instructions: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Glyph<'a> {
    pub bounding_box: Option<[i16; 4]>,
    pub push_count: u16,
    pub code_size: u16,
    pub kind: GlyphKind<'a>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GlyphProgram<'a> {
    pub pushes: Vec<i16>,
    pub instructions: &'a [u8],
}

pub struct GlyphTables {
    pub glyf: Vec<u8>,
    pub loca: Vec<u8>,
}

impl<'a> Cursor<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    pub fn position(&self) -> usize {
        self.offset
    }

    pub fn take(&mut self, length: usize) -> Result<&'a [u8], &'static str> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or("CTF offset overflow")?;
        let bytes = self
            .bytes
            .get(self.offset..end)
            .ok_or("truncated CTF data")?;
        self.offset = end;
        Ok(bytes)
    }

    pub fn byte(&mut self) -> Result<u8, &'static str> {
        Ok(self.take(1)?[0])
    }

    pub fn peek_byte(&self) -> Result<u8, &'static str> {
        self.bytes
            .get(self.offset)
            .copied()
            .ok_or("truncated CTF data")
    }

    pub fn word(&mut self) -> Result<u16, &'static str> {
        Ok(be16(self.take(2)?))
    }

    pub fn signed_word(&mut self) -> Result<i16, &'static str> {
        Ok(i16::from_be_bytes(self.take(2)?.try_into().unwrap()))
    }

    pub fn compact_unsigned(&mut self) -> Result<u16, &'static str> {
        match self.byte()? {
            253 => self.word(),
            254 => Ok(506 + u16::from(self.byte()?)),
            255 => Ok(253 + u16::from(self.byte()?)),
            value => Ok(u16::from(value)),
        }
    }

    pub fn compact_signed(&mut self) -> Result<i16, &'static str> {
        let mut code = self.byte()?;
        if code == 253 {
            return self.signed_word();
        }
        let negative = code == 250;
        if negative {
            code = self.byte()?;
        }
        let magnitude = match code {
            254 => 500 + i32::from(self.byte()?),
            255 => 250 + i32::from(self.byte()?),
            0..=249 => i32::from(code),
            _ => return Err("invalid CTF compact signed code"),
        };
        let value = if negative { -magnitude } else { magnitude };
        i16::try_from(value).map_err(|_| "CTF compact signed value overflow")
    }

    pub fn triplet(&mut self, flag: u8) -> Result<Triplet, &'static str> {
        let index = flag & 0x7f;
        let (extra_bytes, x_bits, y_bits, x_base, y_base, x_negative, y_negative) = match index {
            0..=9 => (
                1,
                0,
                8,
                0,
                i32::from(index / 2) * 256,
                false,
                index & 1 == 0,
            ),
            10..=19 => (
                1,
                8,
                0,
                i32::from((index - 10) / 2) * 256,
                0,
                index & 1 == 0,
                false,
            ),
            20..=83 => {
                let group = index - 20;
                (
                    1,
                    4,
                    4,
                    1 + i32::from(group / 16) * 16,
                    1 + i32::from((group % 16) / 4) * 16,
                    group & 1 == 0,
                    group & 2 == 0,
                )
            }
            84..=119 => {
                let group = index - 84;
                (
                    2,
                    8,
                    8,
                    1 + i32::from(group / 12) * 256,
                    1 + i32::from((group % 12) / 4) * 256,
                    group & 1 == 0,
                    group & 2 == 0,
                )
            }
            120..=123 => (3, 12, 12, 0, 0, index & 1 == 0, index & 2 == 0),
            124..=127 => (4, 16, 16, 0, 0, index & 1 == 0, index & 2 == 0),
            _ => unreachable!(),
        };
        let mut bits = 0u64;
        for &byte in self.take(extra_bytes)? {
            bits = (bits << 8) | u64::from(byte);
        }
        let y_mask = (1u64 << y_bits) - 1;
        let x_mask = (1u64 << x_bits) - 1;
        let mut dx = ((bits >> y_bits) & x_mask) as i32 + x_base;
        let mut dy = (bits & y_mask) as i32 + y_base;
        if x_negative {
            dx = -dx;
        }
        if y_negative {
            dy = -dy;
        }
        Ok(Triplet {
            on_curve: flag & 0x80 == 0,
            dx: i16::try_from(dx).map_err(|_| "CTF triplet X out of range")?,
            dy: i16::try_from(dy).map_err(|_| "CTF triplet Y out of range")?,
        })
    }
}

impl<'a> Ctf<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Self, &'static str> {
        let header = data.get(..DIRECTORY_HEADER).ok_or("truncated CTF header")?;
        let sfnt_version: [u8; 4] = header[..4].try_into().unwrap();
        if !matches!(&sfnt_version, b"OTTO" | b"true" | b"typ1" | b"\0\x01\0\0") {
            return Err("invalid CTF font signature");
        }
        let count = usize::from(be16(&header[4..6]));
        if count == 0 || count > MAX_TABLES {
            return Err("invalid CTF table count");
        }
        let directory_end = DIRECTORY_HEADER + count * TABLE_RECORD;
        let directory = data.get(..directory_end).ok_or("truncated CTF directory")?;
        let mut tables = Vec::with_capacity(count);
        for record in directory[DIRECTORY_HEADER..].chunks_exact(TABLE_RECORD) {
            let tag: [u8; 4] = record[..4].try_into().unwrap();
            if tables.iter().any(|table: &Table| table.tag == tag) {
                return Err("duplicate CTF table");
            }
            let checksum = be32(&record[4..8]);
            let start = be32(&record[8..12]) as usize;
            let len = be32(&record[12..16]) as usize;
            let end = start.checked_add(len).ok_or("CTF table length overflow")?;
            if end > data.len() || (len != 0 && start < directory_end) {
                return Err("CTF table lies outside data block");
            }
            tables.push(Table {
                tag,
                checksum,
                bytes: start..end,
            });
        }
        Ok(Self {
            data,
            sfnt_version,
            tables,
        })
    }

    pub fn table(&self, tag: &[u8; 4]) -> Option<&'a [u8]> {
        let entry = self.tables.iter().find(|entry| &entry.tag == tag)?;
        self.data.get(entry.bytes.clone())
    }

    /// Reassemble a standard sfnt from all three decompressed MTX blocks.
    /// Metric tables with unimplemented CTF transforms are rejected rather
    /// than passed to the font engine in their compact form.
    pub fn reconstruct_sfnt(&self, push_data: &[u8], code: &[u8]) -> Result<Vec<u8>, String> {
        let maxp = self.table(b"maxp").ok_or("missing CTF maxp table")?;
        let glyph_count = usize::from(be16(maxp.get(4..6).ok_or("truncated CTF maxp table")?));
        let glyph_data = self.table(b"glyf").ok_or("missing CTF glyf table")?;
        let glyphs = parse_glyphs(glyph_data, glyph_count)?;
        let programs = decode_glyph_programs(&glyphs, push_data, code)?;
        let glyph_tables = reconstruct_glyph_tables(&glyphs, &programs)?;
        let mut glyf = Some(glyph_tables.glyf);
        let mut loca = Some(glyph_tables.loca);
        let mut tables = Vec::with_capacity(self.tables.len());
        for entry in &self.tables {
            let data = match &entry.tag {
                b"glyf" => glyf.take().ok_or("duplicate CTF glyf table")?,
                b"loca" => loca.take().ok_or("duplicate CTF loca table")?,
                b"cvt " => decode_cvt(self.table(&entry.tag).unwrap())?,
                b"hdmx" | b"VDMX" if entry.bytes.is_empty() => Vec::new(),
                b"hdmx" => decode_hdmx(self)?,
                b"VDMX" => decode_vdmx(self.table(b"VDMX").unwrap())?,
                b"head" => {
                    let mut head = self.table(b"head").unwrap().to_vec();
                    let format = head.get_mut(50..52).ok_or("truncated CTF head table")?;
                    format.copy_from_slice(&1u16.to_be_bytes());
                    head
                }
                _ => self.table(&entry.tag).unwrap().to_vec(),
            };
            tables.push((entry.tag, data));
        }
        let sfnt = super::woff2::build_sfnt(u32::from_be_bytes(self.sfnt_version), tables)
            .ok_or("could not assemble MTX sfnt")?;
        super::woff2::validate_sfnt(&sfnt).ok_or("invalid reconstructed MTX sfnt")?;
        Ok(sfnt)
    }
}

/// Expand the CTF `cvt ` table's variable-width signed deltas into the
/// big-endian 16-bit values stored in an SFNT control-value table.
pub fn decode_cvt(data: &[u8]) -> Result<Vec<u8>, &'static str> {
    let count = usize::from(be16(data.get(..2).ok_or("truncated CTF cvt header")?));
    let mut result = Vec::with_capacity(count * 2);
    let mut offset = 2;
    let mut previous = 0i16;
    for _ in 0..count {
        let code = *data.get(offset).ok_or("truncated CTF cvt value")?;
        offset += 1;
        let delta = if code < 238 {
            i16::from(code)
        } else if code == 238 {
            let bytes = data
                .get(offset..offset + 2)
                .ok_or("truncated CTF cvt word")?;
            offset += 2;
            i16::from_be_bytes(bytes.try_into().unwrap())
        } else {
            let remainder = i16::from(*data.get(offset).ok_or("truncated CTF cvt delta")?);
            offset += 1;
            if code <= 247 {
                -((i16::from(code) - 239) * 238 + remainder)
            } else {
                (i16::from(code) - 247) * 238 + remainder
            }
        };
        previous = previous.wrapping_add(delta);
        result.extend_from_slice(&previous.to_be_bytes());
    }
    if offset != data.len() {
        return Err("trailing CTF cvt data");
    }
    Ok(result)
}

fn decode_uncompressed_metric(
    data: &[u8],
    supported_versions: &[u16],
    min_size: usize,
) -> Result<Vec<u8>, &'static str> {
    if data.len() < min_size {
        return Err("truncated CTF metric table");
    }
    let marker = be16(&data[..2]);
    let version = supported_versions
        .iter()
        .copied()
        .find(|&version| marker == u16::MAX - version)
        .ok_or("unsupported CTF metric transform")?;
    let mut table = data.to_vec();
    table[..2].copy_from_slice(&version.to_be_bytes());
    Ok(table)
}

struct MagnitudeBits<'a> {
    bytes: &'a [u8],
    bit: usize,
}

impl MagnitudeBits<'_> {
    fn read(&mut self) -> Result<i32, &'static str> {
        let mut magnitude = 0i32;
        loop {
            let byte = *self
                .bytes
                .get(self.bit / 8)
                .ok_or("truncated CTF metric bits")?;
            let one = (byte >> (self.bit % 8)) & 1 != 0;
            self.bit += 1;
            if !one {
                break;
            }
            magnitude = magnitude
                .checked_add(1)
                .ok_or("CTF metric magnitude overflow")?;
        }
        if magnitude == 0 {
            return Ok(0);
        }
        let byte = *self
            .bytes
            .get(self.bit / 8)
            .ok_or("truncated CTF metric sign")?;
        let negative = (byte >> (self.bit % 8)) & 1 != 0;
        self.bit += 1;
        Ok(if negative { -magnitude } else { magnitude })
    }
}

fn decode_hdmx(ctf: &Ctf<'_>) -> Result<Vec<u8>, &'static str> {
    let data = ctf.table(b"hdmx").ok_or("missing CTF hdmx table")?;
    if data.len() < 8 {
        return Err("truncated CTF hdmx header");
    }
    if be16(&data[..2]) == u16::MAX {
        return decode_uncompressed_metric(data, &[0], 8);
    }
    if be16(&data[..2]) != 0 {
        return Err("unsupported CTF hdmx version");
    }
    let records = usize::from(be16(&data[2..4]));
    let record_size =
        usize::try_from(be32(&data[4..8])).map_err(|_| "invalid CTF hdmx record size")?;
    let head = ctf.table(b"head").ok_or("missing CTF head table")?;
    let units_per_em = u64::from(be16(head.get(18..20).ok_or("truncated CTF head table")?));
    if units_per_em == 0 {
        return Err("zero CTF units per em");
    }
    let maxp = ctf.table(b"maxp").ok_or("missing CTF maxp table")?;
    let glyphs = usize::from(be16(maxp.get(4..6).ok_or("truncated CTF maxp table")?));
    let hhea = ctf.table(b"hhea").ok_or("missing CTF hhea table")?;
    let long_metrics = usize::from(be16(hhea.get(34..36).ok_or("truncated CTF hhea table")?));
    if long_metrics == 0 || long_metrics > glyphs {
        return Err("invalid CTF horizontal metric count");
    }
    let hmtx = ctf.table(b"hmtx").ok_or("missing CTF hmtx table")?;
    let metric_end = long_metrics
        .checked_mul(4)
        .and_then(|size| size.checked_add((glyphs - long_metrics) * 2))
        .ok_or("CTF horizontal metric size overflow")?;
    if hmtx.len() < metric_end {
        return Err("truncated CTF hmtx table");
    }
    let min_record_size = (glyphs + 2 + 3) & !3;
    if record_size < min_record_size || record_size % 4 != 0 {
        return Err("invalid CTF hdmx record size");
    }
    let header_end = records
        .checked_mul(2)
        .and_then(|size| size.checked_add(8))
        .ok_or("CTF hdmx record count overflow")?;
    let record_headers = data
        .get(8..header_end)
        .ok_or("truncated CTF hdmx records")?;
    let output_size = records
        .checked_mul(record_size)
        .and_then(|size| size.checked_add(8))
        .ok_or("CTF hdmx output size overflow")?;
    if output_size > MAX_HDMX_BYTES {
        return Err("CTF hdmx output exceeds size limit");
    }
    let mut output = Vec::with_capacity(output_size);
    output.extend_from_slice(&data[..8]);
    let mut bits = MagnitudeBits {
        bytes: &data[header_end..],
        bit: 0,
    };
    for record in record_headers.chunks_exact(2) {
        let ppem = u64::from(record[0]);
        output.extend_from_slice(record);
        for glyph in 0..glyphs {
            let metric = glyph.min(long_metrics - 1) * 4;
            let advance = u64::from(be16(&hmtx[metric..metric + 2]));
            let prediction = ((64 * ppem * advance + units_per_em / 2) / units_per_em + 32) / 64;
            let width = i64::try_from(prediction).map_err(|_| "CTF hdmx width overflow")?
                + i64::from(bits.read()?);
            output.push(u8::try_from(width).map_err(|_| "CTF hdmx width out of range")?);
        }
        output.resize(output.len() + record_size - glyphs - 2, 0);
    }
    if output.len() != output_size || (bits.bit + 7) / 8 != bits.bytes.len() {
        return Err("trailing CTF hdmx data");
    }
    Ok(output)
}

fn decode_vdmx(data: &[u8]) -> Result<Vec<u8>, &'static str> {
    if data.len() < 6 {
        return Err("truncated CTF VDMX header");
    }
    let version = be16(&data[..2]);
    if version == u16::MAX || version == u16::MAX - 1 {
        return decode_uncompressed_metric(data, &[0, 1], 6);
    }
    if version > 1 {
        return Err("unsupported CTF VDMX version");
    }
    let group_count = usize::from(be16(&data[2..4]));
    let ratio_count = usize::from(be16(&data[4..6]));
    if group_count == 0 || ratio_count == 0 {
        return Err("empty CTF VDMX groups");
    }
    let offset_start = 6 + ratio_count * 4;
    let header_end = offset_start + ratio_count * 2;
    let offset_bytes = data
        .get(offset_start..header_end)
        .ok_or("truncated CTF VDMX offsets")?;
    let source_offsets: Vec<usize> = offset_bytes
        .chunks_exact(2)
        .map(|bytes| usize::from(be16(bytes)))
        .collect();
    let mut input = *source_offsets
        .iter()
        .min()
        .ok_or("missing CTF VDMX group offset")?;
    if input < header_end || input >= data.len() {
        return Err("invalid CTF VDMX group offset");
    }
    let mut output = data[..header_end].to_vec();
    let mut group_offsets = Vec::with_capacity(group_count);
    for _ in 0..group_count {
        let header = data
            .get(input..input + 6)
            .ok_or("truncated CTF VDMX group")?;
        let records = usize::from(be16(&header[..2]));
        let max_multiplier = i32::from(i16::from_be_bytes(header[2..4].try_into().unwrap()));
        let min_multiplier = i32::from(i16::from_be_bytes(header[4..6].try_into().unwrap()));
        let group_size = records
            .checked_mul(6)
            .and_then(|size| size.checked_add(4))
            .ok_or("CTF VDMX output size overflow")?;
        if output.len().saturating_add(group_size) > MAX_HDMX_BYTES {
            return Err("CTF VDMX output exceeds size limit");
        }
        let output_offset = u16::try_from(output.len()).map_err(|_| "CTF VDMX offset overflow")?;
        group_offsets.push((input, output_offset));
        output.extend_from_slice(&header[..2]);
        let size_field = output.len();
        output.extend_from_slice(&[0, 0]);
        let mut bits = MagnitudeBits {
            bytes: &data[input + 6..],
            bit: 0,
        };
        let mut predicted_ppem = 8i32;
        for index in 0..records {
            let ppem = predicted_ppem
                .checked_add(bits.read()?)
                .ok_or("CTF VDMX ppem overflow")?;
            let ppem_word = u16::try_from(ppem).map_err(|_| "CTF VDMX ppem out of range")?;
            let max_prediction = (ppem * max_multiplier + 1024) / 2048;
            let min_prediction = -((ppem * min_multiplier + 1024) / 2048);
            let max = max_prediction
                .checked_add(bits.read()?)
                .ok_or("CTF VDMX maximum overflow")?;
            let min = min_prediction
                .checked_add(bits.read()?)
                .ok_or("CTF VDMX minimum overflow")?;
            let max = i16::try_from(max).map_err(|_| "CTF VDMX maximum out of range")?;
            let min = i16::try_from(min).map_err(|_| "CTF VDMX minimum out of range")?;
            if index == 0 {
                output[size_field] =
                    u8::try_from(ppem).map_err(|_| "CTF VDMX start size out of range")?;
            }
            output[size_field + 1] =
                u8::try_from(ppem).map_err(|_| "CTF VDMX end size out of range")?;
            output.extend_from_slice(&ppem_word.to_be_bytes());
            output.extend_from_slice(&max.to_be_bytes());
            output.extend_from_slice(&min.to_be_bytes());
            predicted_ppem = ppem.checked_add(1).ok_or("CTF VDMX ppem overflow")?;
        }
        input = input
            .checked_add(6 + bits.bit.div_ceil(8))
            .ok_or("CTF VDMX group offset overflow")?;
    }
    if input != data.len() {
        return Err("trailing CTF VDMX data");
    }
    for (index, source) in source_offsets.iter().enumerate() {
        let target = group_offsets
            .iter()
            .find(|(offset, _)| offset == source)
            .map(|(_, output)| *output)
            .ok_or("CTF VDMX ratio points outside groups")?;
        output[offset_start + index * 2..offset_start + index * 2 + 2]
            .copy_from_slice(&target.to_be_bytes());
    }
    Ok(output)
}

fn read_bounding_box(cursor: &mut Cursor<'_>) -> Result<[i16; 4], &'static str> {
    Ok([
        cursor.signed_word()?,
        cursor.signed_word()?,
        cursor.signed_word()?,
        cursor.signed_word()?,
    ])
}

/// Parse CTF glyph records in glyph order. The records remain separate from
/// the push-data and instruction streams until SFNT reconstruction.
pub fn parse_glyphs<'a>(data: &'a [u8], count: usize) -> Result<Vec<Glyph<'a>>, String> {
    let mut cursor = Cursor::new(data);
    let mut glyphs = Vec::with_capacity(count);
    for index in 0..count {
        let start = cursor.position();
        let glyph = parse_glyph(&mut cursor, data)
            .map_err(|error| format!("CTF glyph {index} at byte {start}: {error}"))?;
        glyphs.push(glyph);
    }
    if cursor.position() != data.len() {
        return Err("trailing CTF glyph data".into());
    }
    Ok(glyphs)
}

/// Match each glyph's compact push values and remaining TrueType instructions
/// against the independently decompressed MTX data and code blocks.
pub fn decode_glyph_programs<'a>(
    glyphs: &[Glyph<'_>],
    push_data: &[u8],
    instructions: &'a [u8],
) -> Result<Vec<GlyphProgram<'a>>, String> {
    let mut pushes = Cursor::new(push_data);
    let mut code = Cursor::new(instructions);
    let mut programs = Vec::with_capacity(glyphs.len());
    for (index, glyph) in glyphs.iter().enumerate() {
        let mut values = Vec::with_capacity(usize::from(glyph.push_count));
        while values.len() < usize::from(glyph.push_count) {
            let remaining = usize::from(glyph.push_count) - values.len();
            match pushes
                .peek_byte()
                .map_err(|error| format!("glyph {index}: {error}"))?
            {
                251 | 252 => {
                    let width = if pushes
                        .byte()
                        .map_err(|error| format!("glyph {index}: {error}"))?
                        == 251
                    {
                        3
                    } else {
                        5
                    };
                    if values.len() < 2 || remaining < width {
                        return Err(format!("glyph {index}: invalid MTX hop sequence"));
                    }
                    let repeated = values[values.len() - 2];
                    for _ in 0..(width / 2) {
                        values.push(repeated);
                        values.push(
                            pushes
                                .compact_signed()
                                .map_err(|error| format!("glyph {index}: {error}"))?,
                        );
                    }
                    values.push(repeated);
                }
                _ => values.push(
                    pushes
                        .compact_signed()
                        .map_err(|error| format!("glyph {index}: {error}"))?,
                ),
            }
        }
        let bytes = code
            .take(usize::from(glyph.code_size))
            .map_err(|error| format!("glyph {index}: {error}"))?;
        programs.push(GlyphProgram {
            pushes: values,
            instructions: bytes,
        });
    }
    if pushes.position() != push_data.len() || code.position() != instructions.len() {
        return Err("trailing MTX glyph program data".into());
    }
    Ok(programs)
}

/// Recreate the TrueType initial stack pushes followed by the remaining
/// instructions from the third MTX block.
pub fn encode_truetype_program(program: &GlyphProgram<'_>) -> Result<Vec<u8>, &'static str> {
    let mut result = Vec::new();
    let mut index = 0;
    while index < program.pushes.len() {
        let byte_values = (0..=255).contains(&i32::from(program.pushes[index]));
        let mut end = index + 1;
        while end < program.pushes.len()
            && end - index < 255
            && (0..=255).contains(&i32::from(program.pushes[end])) == byte_values
        {
            end += 1;
        }
        let count = end - index;
        if count <= 8 {
            result.push(if byte_values { 0xb0 } else { 0xb8 } + (count as u8 - 1));
        } else {
            result.push(if byte_values { 0x40 } else { 0x41 });
            result.push(count as u8);
        }
        for &value in &program.pushes[index..end] {
            if byte_values {
                result.push(value as u8);
            } else {
                result.extend_from_slice(&value.to_be_bytes());
            }
        }
        index = end;
    }
    result.extend_from_slice(program.instructions);
    if result.len() > u16::MAX as usize {
        return Err("TrueType glyph instruction length overflow");
    }
    Ok(result)
}

/// Rebuild standard TrueType outlines and 32-bit glyph offsets. The SFNT
/// assembler must set `head.indexToLocFormat` to 1 for this `loca` table.
pub fn reconstruct_glyph_tables(
    glyphs: &[Glyph<'_>],
    programs: &[GlyphProgram<'_>],
) -> Result<GlyphTables, &'static str> {
    const MAX_GLYF_BYTES: usize = 128 * 1024 * 1024;
    if glyphs.len() != programs.len() {
        return Err("MTX glyph/program count mismatch");
    }
    let mut glyf = Vec::new();
    let mut loca = Vec::with_capacity((glyphs.len() + 1) * 4);
    for (glyph, program) in glyphs.iter().zip(programs) {
        if program.pushes.len() != usize::from(glyph.push_count)
            || program.instructions.len() != usize::from(glyph.code_size)
        {
            return Err("MTX glyph/program lengths differ");
        }
        loca.extend_from_slice(
            &u32::try_from(glyf.len())
                .map_err(|_| "TrueType glyph offset overflow")?
                .to_be_bytes(),
        );
        let instructions = encode_truetype_program(program)?;
        let (contours, bounds) = match &glyph.kind {
            GlyphKind::Simple {
                contour_endpoints,
                points,
            } => {
                if contour_endpoints.is_empty() {
                    if !points.is_empty() || !instructions.is_empty() {
                        return Err("empty MTX glyph has outline or instructions");
                    }
                    continue;
                }
                let last = *contour_endpoints.last().unwrap() as usize;
                if last + 1 != points.len()
                    || contour_endpoints.windows(2).any(|pair| pair[0] >= pair[1])
                {
                    return Err("invalid MTX contour endpoints");
                }
                let computed = [
                    points.iter().map(|p| p.x).min().unwrap(),
                    points.iter().map(|p| p.y).min().unwrap(),
                    points.iter().map(|p| p.x).max().unwrap(),
                    points.iter().map(|p| p.y).max().unwrap(),
                ];
                let contours = i16::try_from(contour_endpoints.len())
                    .map_err(|_| "TrueType contour count overflow")?;
                (contours, glyph.bounding_box.unwrap_or(computed))
            }
            GlyphKind::Composite { .. } => (
                -1,
                glyph.bounding_box.ok_or("missing composite bounding box")?,
            ),
        };
        glyf.extend_from_slice(&contours.to_be_bytes());
        for value in bounds {
            glyf.extend_from_slice(&value.to_be_bytes());
        }
        match &glyph.kind {
            GlyphKind::Simple {
                contour_endpoints,
                points,
            } => {
                for &endpoint in contour_endpoints {
                    glyf.extend_from_slice(&endpoint.to_be_bytes());
                }
                glyf.extend_from_slice(&(instructions.len() as u16).to_be_bytes());
                glyf.extend_from_slice(&instructions);
                for point in points {
                    glyf.push(u8::from(point.on_curve));
                }
                let mut previous = 0i16;
                for point in points {
                    glyf.extend_from_slice(&point.x.wrapping_sub(previous).to_be_bytes());
                    previous = point.x;
                }
                previous = 0;
                for point in points {
                    glyf.extend_from_slice(&point.y.wrapping_sub(previous).to_be_bytes());
                    previous = point.y;
                }
            }
            GlyphKind::Composite {
                components,
                has_instructions,
            } => {
                if !has_instructions && !instructions.is_empty() {
                    return Err("composite glyph has unmarked instructions");
                }
                glyf.extend_from_slice(components);
                if *has_instructions {
                    glyf.extend_from_slice(&(instructions.len() as u16).to_be_bytes());
                    glyf.extend_from_slice(&instructions);
                }
            }
        }
        if glyf.len() % 2 != 0 {
            glyf.push(0);
        }
        if glyf.len() > MAX_GLYF_BYTES {
            return Err("TrueType glyph table exceeds size limit");
        }
    }
    loca.extend_from_slice(
        &u32::try_from(glyf.len())
            .map_err(|_| "TrueType glyph offset overflow")?
            .to_be_bytes(),
    );
    Ok(GlyphTables { glyf, loca })
}

fn parse_glyph<'a>(cursor: &mut Cursor<'a>, data: &'a [u8]) -> Result<Glyph<'a>, &'static str> {
    let marker = cursor.signed_word()?;
    let glyph = if marker == -1 {
        let bounding_box = Some(read_bounding_box(cursor)?);
        let components_start = cursor.position();
        let mut flags;
        loop {
            flags = cursor.word()?;
            cursor.word()?; // component glyph index
            cursor.take(if flags & 0x0001 != 0 { 4 } else { 2 })?;
            let transform_bytes = if flags & 0x0008 != 0 {
                2
            } else if flags & 0x0040 != 0 {
                4
            } else if flags & 0x0080 != 0 {
                8
            } else {
                0
            };
            cursor.take(transform_bytes)?;
            if flags & 0x0020 == 0 {
                break;
            }
        }
        let components = &data[components_start..cursor.position()];
        let (push_count, code_size) = if flags & 0x0100 != 0 {
            (cursor.compact_unsigned()?, cursor.compact_unsigned()?)
        } else {
            (0, 0)
        };
        Glyph {
            bounding_box,
            push_count,
            code_size,
            kind: GlyphKind::Composite {
                components,
                has_instructions: flags & 0x0100 != 0,
            },
        }
    } else {
        let (contours, bounding_box) = if marker == 0x7fff {
            let contours = cursor.signed_word()?;
            if contours < 0 {
                return Err("invalid CTF contour count");
            }
            (contours as usize, Some(read_bounding_box(cursor)?))
        } else if marker >= 0 {
            (marker as usize, None)
        } else {
            return Err("invalid CTF glyph marker");
        };
        let mut contour_endpoints: Vec<u16> = Vec::with_capacity(contours);
        for index in 0..contours {
            let encoded = cursor.compact_unsigned()?;
            let endpoint = if index == 0 {
                encoded
            } else {
                contour_endpoints[index - 1]
                    .checked_add(encoded)
                    .ok_or("CTF contour endpoint overflow")?
            };
            contour_endpoints.push(endpoint);
        }
        let point_count = contour_endpoints
            .last()
            .map_or(0, |last| usize::from(*last) + 1);
        let mut points = Vec::with_capacity(point_count);
        let (mut x, mut y) = (0i32, 0i32);
        let flags = cursor.take(point_count)?;
        for &flag in flags {
            let triplet = cursor.triplet(flag)?;
            x += i32::from(triplet.dx);
            y += i32::from(triplet.dy);
            points.push(Point {
                x: i16::try_from(x).map_err(|_| "CTF glyph X out of range")?,
                y: i16::try_from(y).map_err(|_| "CTF glyph Y out of range")?,
                on_curve: triplet.on_curve,
            });
        }
        let (push_count, code_size) = if contours == 0 {
            (0, 0)
        } else {
            (cursor.compact_unsigned()?, cursor.compact_unsigned()?)
        };
        Glyph {
            bounding_box,
            push_count,
            code_size,
            kind: GlyphKind::Simple {
                contour_endpoints,
                points,
            },
        }
    };
    Ok(glyph)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_and_overlapping_directory_records() {
        assert!(Ctf::parse(&[]).is_err());
        let mut data = vec![0; 28];
        data[..4].copy_from_slice(b"\0\x01\0\0");
        data[4..6].copy_from_slice(&1u16.to_be_bytes());
        data[12..16].copy_from_slice(b"head");
        data[20..24].copy_from_slice(&12u32.to_be_bytes());
        data[24..28].copy_from_slice(&4u32.to_be_bytes());
        assert!(Ctf::parse(&data).is_err());
    }

    #[test]
    fn real_primary_ctf_block_reconstructs_sfnt() {
        let data = super::super::compressed_eot_fixture();
        let eot = super::super::eot::parse_prefix(&data).unwrap().unwrap();
        let primary = eot.decode_mtx_stream(&data, 0).unwrap().unwrap();
        let ctf = Ctf::parse(&primary).unwrap();
        for table in &ctf.tables {
            if table.checksum == 0
                || matches!(
                    &table.tag,
                    b"glyf" | b"loca" | b"cvt " | b"hdmx" | b"VDMX" | b"head"
                )
            {
                continue;
            }
            let mut sum = 0u32;
            for chunk in ctf.table(&table.tag).unwrap().chunks(4) {
                let mut word = [0u8; 4];
                word[..chunk.len()].copy_from_slice(chunk);
                sum = sum.wrapping_add(u32::from_be_bytes(word));
            }
            assert_eq!(sum, table.checksum, "CTF table {:?} checksum", table.tag);
        }
        assert!(ctf.table(b"head").is_some());
        assert!(ctf.table(b"glyf").is_some());
        assert_eq!(ctf.table(b"loca"), Some(&[][..]));
        if let Some(cvt) = ctf.table(b"cvt ") {
            assert_eq!(
                decode_cvt(cvt).unwrap().len(),
                usize::from(be16(&cvt[..2])) * 2
            );
        }
        let maxp = ctf.table(b"maxp").unwrap();
        let glyph_count = usize::from(be16(maxp.get(4..6).unwrap()));
        let glyphs = parse_glyphs(ctf.table(b"glyf").unwrap(), glyph_count).unwrap();
        assert_eq!(glyphs.len(), glyph_count);
        let push_data = eot.decode_mtx_stream(&data, 1).unwrap().unwrap();
        let code = eot.decode_mtx_stream(&data, 2).unwrap().unwrap();
        let programs = decode_glyph_programs(&glyphs, &push_data, &code).unwrap();
        assert_eq!(programs.len(), glyph_count);
        let tables = reconstruct_glyph_tables(&glyphs, &programs).unwrap();
        assert_eq!(tables.loca.len(), (glyph_count + 1) * 4);
        assert_eq!(&tables.loca[..4], &[0; 4]);
        assert_eq!(
            u32::from_be_bytes(tables.loca[tables.loca.len() - 4..].try_into().unwrap()) as usize,
            tables.glyf.len()
        );
        let sfnt = ctf.reconstruct_sfnt(&push_data, &code).unwrap();
        assert_eq!(&sfnt[..4], b"\0\x01\0\0");
        let mut fontdb = fontdb::Database::new();
        fontdb.load_font_data(sfnt);
        assert!(fontdb.faces().next().is_some());
    }

    #[test]
    fn cvt_relative_values_and_wide_deltas() {
        let encoded = [0, 5, 3, 239, 1, 248, 0, 238, 0xff, 0x00, 0];
        let decoded = decode_cvt(&encoded).unwrap();
        let values: Vec<i16> = decoded
            .chunks_exact(2)
            .map(|pair| i16::from_be_bytes(pair.try_into().unwrap()))
            .collect();
        assert_eq!(values, [3, 2, 240, -16, -16]);
        assert!(decode_cvt(&encoded[..encoded.len() - 1]).is_err());
    }

    #[test]
    fn uncompressed_metric_fallback_restores_version() {
        assert_eq!(
            decode_uncompressed_metric(&[0xff, 0xff, 0, 0, 0, 0, 0, 0], &[0], 8),
            Ok(vec![0, 0, 0, 0, 0, 0, 0, 0])
        );
        assert_eq!(
            decode_uncompressed_metric(&[0xff, 0xfe, 0, 0, 0, 0], &[0, 1], 6),
            Ok(vec![0, 1, 0, 0, 0, 0])
        );
        assert!(decode_uncompressed_metric(&[0, 1, 0, 0, 0, 0], &[0, 1], 6).is_err());
    }

    #[test]
    fn compressed_hdmx_reconstructs_predicted_widths() {
        let mut head = vec![0; 54];
        head[18..20].copy_from_slice(&1000u16.to_be_bytes());
        let mut hhea = vec![0; 36];
        hhea[34..36].copy_from_slice(&2u16.to_be_bytes());
        let mut maxp = vec![0; 6];
        maxp[4..6].copy_from_slice(&3u16.to_be_bytes());
        let hmtx = [0x01, 0xf4, 0, 0, 0x03, 0xe8, 0, 0, 0, 0];
        let hdmx = [0, 0, 0, 1, 0, 0, 0, 8, 10, 11, 0x52];
        let sfnt = super::super::woff2::build_sfnt(
            0x0001_0000,
            vec![
                (*b"head", head),
                (*b"hhea", hhea),
                (*b"maxp", maxp),
                (*b"hmtx", hmtx.to_vec()),
                (*b"hdmx", hdmx.to_vec()),
            ],
        )
        .unwrap();
        let ctf = Ctf::parse(&sfnt).unwrap();
        assert_eq!(
            decode_hdmx(&ctf).unwrap(),
            [0, 0, 0, 1, 0, 0, 0, 8, 10, 11, 5, 11, 9, 0, 0, 0]
        );
        assert_eq!(
            MagnitudeBits {
                bytes: &[0],
                bit: 0
            }
            .read(),
            Ok(0)
        );
    }

    #[test]
    fn compressed_vdmx_rebuilds_groups_and_ratio_offsets() {
        let data = [
            0, 0, 0, 2, 0, 2, // version, groups, ratios
            0, 1, 1, 1, 0, 2, 1, 1, // ratio records
            0, 18, 0, 25, // compressed group offsets
            0, 1, 8, 0, 4, 0, 0, // group 1: 8 ppem, max 8, min -4
            0, 1, 0, 0, 0, 0, 1, // group 2: +1 ppem delta
        ];
        let result = decode_vdmx(&data).unwrap();
        assert_eq!(
            &result[..18],
            &[0, 0, 0, 2, 0, 2, 0, 1, 1, 1, 0, 2, 1, 1, 0, 18, 0, 28,]
        );
        assert_eq!(&result[18..28], &[0, 1, 8, 8, 0, 8, 0, 8, 0xff, 0xfc]);
        assert_eq!(&result[28..], &[0, 1, 9, 9, 0, 9, 0, 0, 0, 0]);
        assert!(decode_vdmx(&data[..data.len() - 1]).is_err());
    }

    #[test]
    fn compact_integer_encodings_and_truncation() {
        let mut values = Cursor::new(&[252, 255, 0, 254, 0, 253, 0x12, 0x34]);
        assert_eq!(values.compact_unsigned(), Ok(252));
        assert_eq!(values.compact_unsigned(), Ok(253));
        assert_eq!(values.compact_unsigned(), Ok(506));
        assert_eq!(values.compact_unsigned(), Ok(0x1234));
        assert_eq!(values.position(), 8);
        assert!(values.compact_unsigned().is_err());

        let mut signed = Cursor::new(&[249, 250, 20, 255, 0, 250, 254, 0, 253, 0x80, 0]);
        assert_eq!(signed.compact_signed(), Ok(249));
        assert_eq!(signed.compact_signed(), Ok(-20));
        assert_eq!(signed.compact_signed(), Ok(250));
        assert_eq!(signed.compact_signed(), Ok(-500));
        assert_eq!(signed.compact_signed(), Ok(i16::MIN));
        assert!(signed.compact_signed().is_err());
        assert!(Cursor::new(&[250, 251]).compact_signed().is_err());
        assert!(Cursor::new(&[253, 0]).compact_signed().is_err());
    }

    #[test]
    fn glyph_hint_lengths_follow_outline_data() {
        let data = [0, 1, 0, 11, 5, 3, 4, 0, 0];
        let glyphs = parse_glyphs(&data, 2).unwrap();
        assert_eq!(glyphs[0].push_count, 3);
        assert_eq!(glyphs[0].code_size, 4);
        assert_eq!(
            glyphs[0].kind,
            GlyphKind::Simple {
                contour_endpoints: vec![0],
                points: vec![Point {
                    x: 5,
                    y: 0,
                    on_curve: true,
                }],
            }
        );
        assert_eq!(glyphs[1].push_count, 0);
        assert_eq!(glyphs[1].code_size, 0);
        assert!(parse_glyphs(&data[..data.len() - 1], 2).is_err());
    }

    #[test]
    fn glyph_push_hops_and_instruction_streams() {
        let glyph = Glyph {
            bounding_box: None,
            push_count: 10,
            code_size: 2,
            kind: GlyphKind::Simple {
                contour_endpoints: vec![],
                points: vec![],
            },
        };
        let data = [7, 2, 251, 9, 252, 10, 11];
        let programs = decode_glyph_programs(&[glyph], &data, &[0x2a, 0x7f]).unwrap();
        assert_eq!(programs[0].pushes, [7, 2, 7, 9, 7, 9, 10, 9, 11, 9]);
        assert_eq!(programs[0].instructions, [0x2a, 0x7f]);
        assert!(decode_glyph_programs(&[], &[1], &[]).is_err());
    }

    #[test]
    fn truetype_push_instructions_preserve_values_and_code() {
        let program = GlyphProgram {
            pushes: vec![7, 2, 7, 9, 7, 9, 10, 9, 11, 9, -3, 300],
            instructions: &[0x2a, 0x7f],
        };
        assert_eq!(
            encode_truetype_program(&program).unwrap(),
            [
                0x40, 10, 7, 2, 7, 9, 7, 9, 10, 9, 11, 9, 0xb9, 0xff, 0xfd, 0x01, 0x2c, 0x2a, 0x7f,
            ]
        );
    }

    #[test]
    fn truetype_glyph_and_loca_records() {
        let glyphs = [
            Glyph {
                bounding_box: None,
                push_count: 0,
                code_size: 0,
                kind: GlyphKind::Simple {
                    contour_endpoints: vec![0],
                    points: vec![Point {
                        x: 5,
                        y: 0,
                        on_curve: true,
                    }],
                },
            },
            Glyph {
                bounding_box: None,
                push_count: 0,
                code_size: 0,
                kind: GlyphKind::Simple {
                    contour_endpoints: vec![],
                    points: vec![],
                },
            },
        ];
        let empty = GlyphProgram {
            pushes: vec![],
            instructions: &[],
        };
        let tables = reconstruct_glyph_tables(&glyphs, &[empty.clone(), empty]).unwrap();
        assert_eq!(tables.loca, [0, 0, 0, 0, 0, 0, 0, 20, 0, 0, 0, 20]);
        assert_eq!(&tables.glyf[..10], &[0, 1, 0, 5, 0, 0, 0, 5, 0, 0]);
        assert_eq!(&tables.glyf[10..], &[0, 0, 0, 0, 1, 0, 5, 0, 0, 0]);
    }

    #[test]
    fn triplet_encoding_ranges_and_curve_flags() {
        assert_eq!(
            Cursor::new(&[7]).triplet(0),
            Ok(Triplet {
                on_curve: true,
                dx: 0,
                dy: -7
            })
        );
        assert_eq!(
            Cursor::new(&[7]).triplet(1 | 0x80),
            Ok(Triplet {
                on_curve: false,
                dx: 0,
                dy: 7
            })
        );
        assert_eq!(
            Cursor::new(&[2]).triplet(10),
            Ok(Triplet {
                on_curve: true,
                dx: -2,
                dy: 0
            })
        );
        assert_eq!(
            Cursor::new(&[0x12]).triplet(23),
            Ok(Triplet {
                on_curve: true,
                dx: 2,
                dy: 3
            })
        );
        assert_eq!(
            Cursor::new(&[0, 0]).triplet(84),
            Ok(Triplet {
                on_curve: true,
                dx: -1,
                dy: -1
            })
        );
        assert_eq!(
            Cursor::new(&[0, 0, 0]).triplet(123),
            Ok(Triplet {
                on_curve: true,
                dx: 0,
                dy: 0
            })
        );
        assert_eq!(
            Cursor::new(&[0, 0, 0, 1]).triplet(127),
            Ok(Triplet {
                on_curve: true,
                dx: 0,
                dy: 1
            })
        );
        assert!(Cursor::new(&[]).triplet(0).is_err());
    }
}
