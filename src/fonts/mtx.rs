//! MicroType Express stream boundaries. Each of its three LZCOMP streams can
//! be consumed independently once its offset becomes available.

use std::ops::Range;

const HEADER_SIZE: usize = 10;
const MTX_VERSION: u8 = 3;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MtxHeader {
    pub copy_limit: usize,
    pub streams: [Range<usize>; 3],
}

fn be24(bytes: &[u8]) -> usize {
    (usize::from(bytes[0]) << 16) | (usize::from(bytes[1]) << 8) | usize::from(bytes[2])
}

/// Parse the header from the currently available prefix. `total_size` is the
/// EOT FontDataSize, so offsets are checked without waiting for every stream.
pub fn parse_prefix(prefix: &[u8], total_size: usize) -> Result<Option<MtxHeader>, &'static str> {
    if total_size < HEADER_SIZE {
        return Err("MTX payload is shorter than its header");
    }
    if prefix.len() < HEADER_SIZE {
        return Ok(None);
    }
    if prefix[0] != MTX_VERSION {
        return Err("unsupported MTX version");
    }
    let second = be24(&prefix[4..7]);
    let third = be24(&prefix[7..10]);
    if second < HEADER_SIZE || third < second || third > total_size {
        return Err("invalid MTX stream offsets");
    }
    Ok(Some(MtxHeader {
        copy_limit: be24(&prefix[1..4]),
        streams: [HEADER_SIZE..second, second..third, third..total_size],
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_stream_boundaries_are_available_from_the_header() {
        let data = [3, 0, 1, 244, 0, 0, 13, 0, 0, 17];
        assert_eq!(parse_prefix(&data[..9], 23), Ok(None));
        let header = parse_prefix(&data, 23).unwrap().unwrap();
        assert_eq!(header.copy_limit, 500);
        assert_eq!(header.streams, [10..13, 13..17, 17..23]);
    }

    #[test]
    fn malformed_or_out_of_order_streams_are_rejected() {
        assert!(parse_prefix(&[3, 0, 0, 1, 0, 0, 9, 0, 0, 17], 23).is_err());
        assert!(parse_prefix(&[3, 0, 0, 1, 0, 0, 18, 0, 0, 17], 23).is_err());
        assert!(parse_prefix(&[3, 0, 0, 1, 0, 0, 13, 0, 0, 24], 23).is_err());
        assert!(parse_prefix(&[2, 0, 0, 1, 0, 0, 13, 0, 0, 17], 23).is_err());
    }
}
