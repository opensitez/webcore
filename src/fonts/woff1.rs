//! WOFF1 -> sfnt.
//!
//! WOFF1 keeps an explicit table directory and stores each table either raw or
//! zlib-compressed. The output is a normal sfnt wrapper.

use flate2::read::ZlibDecoder;

pub fn decode(data: &[u8]) -> Option<Vec<u8>> {
    if data.len() < 44 {
        return None;
    }

    let r32 = |off: usize| -> Option<u32> {
        Some(u32::from_be_bytes(data.get(off..off + 4)?.try_into().ok()?))
    };
    let r16 = |off: usize| -> Option<u16> {
        Some(u16::from_be_bytes(data.get(off..off + 2)?.try_into().ok()?))
    };

    let flavor = r32(4)?;
    let num_tables = r16(12)?;
    let total_sfnt = r32(16)? as usize;

    struct TableEntry {
        tag: [u8; 4],
        offset: usize,
        comp_length: usize,
        orig_length: usize,
        orig_checksum: u32,
    }

    let mut entries = Vec::with_capacity(num_tables as usize);
    for i in 0..num_tables as usize {
        let base = 44usize.checked_add(i.checked_mul(20)?)?;
        let tag: [u8; 4] = data.get(base..base + 4)?.try_into().ok()?;
        entries.push(TableEntry {
            tag,
            offset: r32(base + 4)? as usize,
            comp_length: r32(base + 8)? as usize,
            orig_length: r32(base + 12)? as usize,
            orig_checksum: r32(base + 16)?,
        });
    }

    let mut out = Vec::with_capacity(total_sfnt);
    out.extend_from_slice(&flavor.to_be_bytes());
    out.extend_from_slice(&num_tables.to_be_bytes());
    let n = num_tables as u32;
    let entry_sel = 31 - n.leading_zeros();
    let search_range = (1u16 << entry_sel) * 16;
    let range_shift = num_tables.checked_mul(16)?.checked_sub(search_range)?;
    out.extend_from_slice(&search_range.to_be_bytes());
    out.extend_from_slice(&(entry_sel as u16).to_be_bytes());
    out.extend_from_slice(&range_shift.to_be_bytes());

    let dir_size = 12usize.checked_add((num_tables as usize).checked_mul(16)?)?;
    let mut table_data: Vec<Vec<u8>> = Vec::new();
    let mut current_offset = dir_size;

    for entry in &entries {
        let raw = if entry.comp_length < entry.orig_length {
            let end = entry.offset.checked_add(entry.comp_length)?;
            let compressed = data.get(entry.offset..end)?;
            let mut decoder = ZlibDecoder::new(compressed);
            let mut decompressed = Vec::with_capacity(entry.orig_length);
            if std::io::Read::read_to_end(&mut decoder, &mut decompressed).is_err() {
                return None;
            }
            if decompressed.len() != entry.orig_length {
                return None;
            }
            decompressed
        } else {
            let end = entry.offset.checked_add(entry.orig_length)?;
            data.get(entry.offset..end)?.to_vec()
        };

        out.extend_from_slice(&entry.tag);
        out.extend_from_slice(&entry.orig_checksum.to_be_bytes());
        out.extend_from_slice(&(current_offset as u32).to_be_bytes());
        out.extend_from_slice(&(raw.len() as u32).to_be_bytes());

        let padded = raw.len().checked_add(3)? & !3;
        let mut padded_raw = raw;
        padded_raw.resize(padded, 0);
        current_offset = current_offset.checked_add(padded)?;
        table_data.push(padded_raw);
    }

    for td in table_data {
        out.extend_from_slice(&td);
    }

    Some(out)
}
