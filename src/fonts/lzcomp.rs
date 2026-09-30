//! MTX LZCOMP stream decoder. This yields one CTF data block; converting CTF
//! tables into an SFNT font is a separate step.

const PREFIX_SIZE: usize = 2 * 32 * 96 + 4 * 256;
const MAX_BLOCK_SIZE: usize = 64 * 1024 * 1024;

#[cfg(test)]
struct Bits<'a> {
    data: &'a [u8],
    offset: usize,
}

#[cfg(test)]
impl Bits<'_> {
    fn bit(&mut self) -> Result<usize, &'static str> {
        let byte = *self
            .data
            .get(self.offset / 8)
            .ok_or("truncated LZCOMP stream")?;
        let value = usize::from((byte >> (7 - self.offset % 8)) & 1);
        self.offset += 1;
        Ok(value)
    }

    fn value(&mut self, count: usize) -> Result<usize, &'static str> {
        let mut value = 0;
        for _ in 0..count {
            value = (value << 1) | self.bit()?;
        }
        Ok(value)
    }
}

#[derive(Clone, Copy)]
struct Node {
    parent: usize,
    left: usize,
    right: usize,
    symbol: Option<usize>,
    weight: u64,
}

struct Huffman {
    nodes: Vec<Node>,
    leaves: Vec<usize>,
}

impl Huffman {
    fn new(alphabet: usize) -> Self {
        let mut nodes = vec![
            Node {
                parent: 0,
                left: 0,
                right: 0,
                symbol: None,
                weight: 0,
            };
            alphabet * 2
        ];
        let mut leaves = vec![0; alphabet];
        for i in 1..alphabet * 2 {
            nodes[i].parent = i / 2;
            if i < alphabet {
                nodes[i].left = i * 2;
                nodes[i].right = i * 2 + 1;
            } else {
                nodes[i].symbol = Some(i - alphabet);
                leaves[i - alphabet] = i;
            }
        }
        let mut tree = Self { nodes, leaves };
        for i in alphabet..alphabet * 2 {
            tree.nodes[i].weight = 1;
        }
        for i in (1..alphabet).rev() {
            tree.nodes[i].weight = tree.nodes[i * 2].weight + tree.nodes[i * 2 + 1].weight;
        }
        if (257..512).contains(&alphabet) {
            tree.bump(256);
            tree.bump(257);
            for _ in 0..12 {
                tree.bump(alphabet - 3);
            }
            for _ in 0..6 {
                tree.bump(alphabet - 2);
            }
        } else {
            for _ in 0..2 {
                for symbol in 0..alphabet {
                    tree.bump(symbol);
                }
            }
        }
        tree
    }

    fn bump(&mut self, symbol: usize) {
        let mut current = self.leaves[symbol];
        while current != 1 {
            let weight = self.nodes[current].weight;
            let mut first = current;
            while first > 1 && self.nodes[first - 1].weight == weight {
                first -= 1;
            }
            if first > 1 && first != current {
                let first_parent = self.nodes[first].parent;
                let current_parent = self.nodes[current].parent;
                self.nodes.swap(first, current);
                self.nodes[first].parent = first_parent;
                self.nodes[current].parent = current_parent;
                for index in [first, current] {
                    let node = self.nodes[index];
                    if let Some(code) = node.symbol {
                        self.leaves[code] = index;
                    } else {
                        self.nodes[node.left].parent = index;
                        self.nodes[node.right].parent = index;
                    }
                }
                current = first;
            }
            self.nodes[current].weight += 1;
            current = self.nodes[current].parent;
        }
        self.nodes[1].weight += 1;
    }

    #[cfg(test)]
    fn read(&mut self, bits: &mut Bits<'_>) -> Result<usize, &'static str> {
        let mut node = 1;
        loop {
            let entry = self.nodes.get(node).ok_or("invalid LZCOMP Huffman tree")?;
            if let Some(symbol) = entry.symbol {
                self.bump(symbol);
                return Ok(symbol);
            }
            node = if bits.bit()? == 0 {
                entry.left
            } else {
                entry.right
            };
        }
    }

    fn read_partial(
        &mut self,
        bits: &mut InputBits,
        node: &mut usize,
    ) -> Result<Option<usize>, &'static str> {
        loop {
            let entry = self.nodes.get(*node).ok_or("invalid LZCOMP Huffman tree")?;
            if let Some(symbol) = entry.symbol {
                self.bump(symbol);
                *node = 1;
                return Ok(Some(symbol));
            }
            let Some(bit) = bits.bit() else {
                return Ok(None);
            };
            *node = if bit == 0 { entry.left } else { entry.right };
        }
    }
}

#[derive(Default)]
struct InputBits {
    bytes: Vec<u8>,
    offset: usize,
}

impl InputBits {
    fn bit(&mut self) -> Option<usize> {
        let byte = *self.bytes.get(self.offset / 8)?;
        let value = usize::from((byte >> (7 - self.offset % 8)) & 1);
        self.offset += 1;
        Some(value)
    }

    fn discard_consumed(&mut self) {
        let bytes = self.offset / 8;
        if bytes > 0 {
            self.bytes.drain(..bytes);
            self.offset %= 8;
        }
    }
}

#[derive(Clone, Copy)]
enum Phase {
    Flag,
    Size {
        read: u8,
        value: usize,
    },
    Symbol {
        node: usize,
    },
    Length {
        groups: usize,
        code: usize,
        value: usize,
        extras: u8,
        node: usize,
    },
    Distance {
        remaining: usize,
        distance: usize,
        value: usize,
        node: usize,
    },
    Done,
}

/// Incrementally decompresses one MTX LZCOMP block. `push` returns only newly
/// available CTF bytes; neither consumed compressed input nor prior output is
/// retained. The dictionary remains until the block ends, as LZ copies need it.
pub struct StreamDecoder {
    input: InputBits,
    received: usize,
    phase: Phase,
    run_length: bool,
    decoded_size: usize,
    produced: usize,
    ranges: usize,
    duplicate_two: usize,
    symbol_tree: Option<Huffman>,
    length_tree: Huffman,
    distance_tree: Huffman,
    history: Vec<u8>,
    escape: Option<u8>,
    rle_state: u8,
    rle_count: u8,
}

impl Default for StreamDecoder {
    fn default() -> Self {
        Self {
            input: InputBits::default(),
            received: 0,
            phase: Phase::Flag,
            run_length: false,
            decoded_size: 0,
            produced: 0,
            ranges: 0,
            duplicate_two: 0,
            symbol_tree: None,
            length_tree: Huffman::new(8),
            distance_tree: Huffman::new(8),
            history: dictionary_prefix(),
            escape: None,
            rle_state: 0,
            rle_count: 0,
        }
    }
}

impl StreamDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, input: &[u8]) -> Result<Vec<u8>, &'static str> {
        self.received = self
            .received
            .checked_add(input.len())
            .filter(|&size| size <= MAX_BLOCK_SIZE)
            .ok_or("LZCOMP input exceeds size limit")?;
        self.input.bytes.extend_from_slice(input);
        let mut output = Vec::new();
        loop {
            match self.phase {
                Phase::Flag => {
                    let Some(bit) = self.input.bit() else { break };
                    self.run_length = bit != 0;
                    self.phase = Phase::Size { read: 0, value: 0 };
                }
                Phase::Size {
                    mut read,
                    mut value,
                } => {
                    while read < 24 {
                        let Some(bit) = self.input.bit() else { break };
                        value = (value << 1) | bit;
                        read += 1;
                    }
                    if read < 24 {
                        self.phase = Phase::Size { read, value };
                        break;
                    }
                    if value > MAX_BLOCK_SIZE {
                        return Err("LZCOMP block exceeds size limit");
                    }
                    self.decoded_size = value;
                    self.ranges = 1;
                    while (1usize << (3 * self.ranges)) < value {
                        self.ranges += 1;
                    }
                    self.duplicate_two = 256 + 8 * self.ranges;
                    self.symbol_tree = Some(Huffman::new(self.duplicate_two + 3));
                    self.next_symbol();
                }
                Phase::Symbol { mut node } => {
                    let Some(symbol) = self
                        .symbol_tree
                        .as_mut()
                        .ok_or("missing LZCOMP symbol tree")?
                        .read_partial(&mut self.input, &mut node)?
                    else {
                        self.phase = Phase::Symbol { node };
                        break;
                    };
                    if symbol < 256 {
                        self.emit(symbol as u8, &mut output)?;
                        self.next_symbol();
                    } else if (self.duplicate_two..self.duplicate_two + 3).contains(&symbol) {
                        let distance = (symbol - self.duplicate_two + 1) * 2;
                        let index = self
                            .history
                            .len()
                            .checked_sub(distance)
                            .ok_or("invalid LZCOMP duplicate distance")?;
                        let byte = self.history[index];
                        self.emit(byte, &mut output)?;
                        self.next_symbol();
                    } else {
                        let code = (symbol - 256) % 8;
                        let groups = (symbol - 256) / 8 + 1;
                        if groups > self.ranges {
                            return Err("invalid LZCOMP distance range");
                        }
                        self.phase = Phase::Length {
                            groups,
                            code,
                            value: code & 3,
                            extras: 0,
                            node: 1,
                        };
                    }
                }
                Phase::Length {
                    groups,
                    mut code,
                    mut value,
                    mut extras,
                    mut node,
                } => {
                    while code & 4 != 0 {
                        extras += 1;
                        if extras > 12 {
                            return Err("invalid LZCOMP length");
                        }
                        let Some(symbol) =
                            self.length_tree.read_partial(&mut self.input, &mut node)?
                        else {
                            self.phase = Phase::Length {
                                groups,
                                code,
                                value,
                                extras: extras - 1,
                                node,
                            };
                            self.input.discard_consumed();
                            return Ok(output);
                        };
                        code = symbol;
                        value = (value << 2) | (code & 3);
                    }
                    self.phase = Phase::Distance {
                        remaining: groups,
                        distance: 0,
                        value,
                        node: 1,
                    };
                }
                Phase::Distance {
                    mut remaining,
                    mut distance,
                    value,
                    mut node,
                } => {
                    while remaining > 0 {
                        let Some(symbol) = self
                            .distance_tree
                            .read_partial(&mut self.input, &mut node)?
                        else {
                            self.phase = Phase::Distance {
                                remaining,
                                distance,
                                value,
                                node,
                            };
                            self.input.discard_consumed();
                            return Ok(output);
                        };
                        distance = (distance << 3) | symbol;
                        remaining -= 1;
                    }
                    distance += 1;
                    let count = value + 2 + usize::from(distance >= 512);
                    let start = self
                        .history
                        .len()
                        .checked_sub(distance + count - 1)
                        .ok_or("invalid LZCOMP copy distance")?;
                    if self.history.len() - PREFIX_SIZE + count > self.decoded_size {
                        return Err("LZCOMP copy exceeds block length");
                    }
                    for offset in 0..count {
                        let byte = *self
                            .history
                            .get(start + offset)
                            .ok_or("invalid LZCOMP copy source")?;
                        self.emit(byte, &mut output)?;
                    }
                    self.next_symbol();
                }
                Phase::Done => break,
            }
        }
        self.input.discard_consumed();
        Ok(output)
    }

    pub fn finish(&self) -> Result<(), &'static str> {
        if !matches!(self.phase, Phase::Done) {
            return Err("truncated LZCOMP stream");
        }
        if self.run_length && (self.escape.is_none() || self.rle_state != 0) {
            return Err("incomplete LZCOMP run-length data");
        }
        Ok(())
    }

    fn next_symbol(&mut self) {
        self.phase = if self.history.len() - PREFIX_SIZE == self.decoded_size {
            Phase::Done
        } else {
            Phase::Symbol { node: 1 }
        };
    }

    fn emit(&mut self, byte: u8, output: &mut Vec<u8>) -> Result<(), &'static str> {
        self.history.push(byte);
        if self.run_length {
            let before = output.len();
            append_rle(
                output,
                byte,
                &mut self.escape,
                &mut self.rle_state,
                &mut self.rle_count,
            )?;
            self.produced = self
                .produced
                .checked_add(output.len() - before)
                .filter(|&size| size <= MAX_BLOCK_SIZE)
                .ok_or("LZCOMP output exceeds size limit")?;
        } else {
            output.push(byte);
            self.produced += 1;
        }
        Ok(())
    }
}

fn dictionary_prefix() -> Vec<u8> {
    let mut bytes = Vec::with_capacity(PREFIX_SIZE);
    for high in 0..32 {
        for low in 0..96 {
            bytes.extend_from_slice(&[high, low]);
        }
    }
    for byte in 0..=255 {
        bytes.extend_from_slice(&[byte; 4]);
    }
    bytes
}

fn append_rle(
    out: &mut Vec<u8>,
    value: u8,
    escape: &mut Option<u8>,
    state: &mut u8,
    count: &mut u8,
) -> Result<(), &'static str> {
    match *escape {
        None => *escape = Some(value),
        Some(marker) => match *state {
            0 if value == marker => *state = 1,
            0 => out.push(value),
            1 if value == 0 => {
                out.push(marker);
                *state = 0;
            }
            1 => {
                *count = value;
                *state = 2;
            }
            2 => {
                if out.len().saturating_add(usize::from(*count)) > MAX_BLOCK_SIZE {
                    return Err("LZCOMP output exceeds size limit");
                }
                out.extend(std::iter::repeat_n(value, usize::from(*count)));
                *state = 0;
            }
            _ => return Err("invalid LZCOMP run-length state"),
        },
    }
    Ok(())
}

/// Decode one independently compressed MTX data stream. The 24-bit length is
/// the LZ output length; optional run-length expansion can make CTF larger.
pub fn decode(data: &[u8]) -> Result<Vec<u8>, &'static str> {
    let mut decoder = StreamDecoder::new();
    let output = decoder.push(data)?;
    decoder.finish()?;
    Ok(output)
}

#[cfg(test)]
fn decode_reference(data: &[u8]) -> Result<Vec<u8>, &'static str> {
    let mut bits = Bits { data, offset: 0 };
    let run_length = bits.bit()? != 0;
    let mut distance_tree = Huffman::new(8);
    let mut length_tree = Huffman::new(8);
    let decoded_size = bits.value(24)?;
    if decoded_size > MAX_BLOCK_SIZE {
        return Err("LZCOMP block exceeds size limit");
    }
    let mut ranges = 1;
    while (1usize << (3 * ranges)) < decoded_size {
        ranges += 1;
    }
    let duplicate_two = 256 + 8 * ranges;
    let mut symbol_tree = Huffman::new(duplicate_two + 3);
    let mut history = dictionary_prefix();
    let mut output = Vec::with_capacity(decoded_size);
    let mut escape = None;
    let mut rle_state = 0;
    let mut rle_count = 0;
    while history.len() - PREFIX_SIZE < decoded_size {
        let symbol = symbol_tree.read(&mut bits)?;
        let history_len = history.len();
        let (start, count) = if symbol < 256 {
            history.push(symbol as u8);
            (history_len, 1)
        } else if (duplicate_two..duplicate_two + 3).contains(&symbol) {
            let distance = (symbol - duplicate_two + 1) * 2;
            if distance > history_len {
                return Err("invalid LZCOMP duplicate distance");
            }
            let value = history[history_len - distance];
            history.push(value);
            (history_len, 1)
        } else {
            let mut length = (symbol - 256) % 8;
            let distance_groups = (symbol - 256) / 8 + 1;
            if distance_groups > ranges {
                return Err("invalid LZCOMP distance range");
            }
            let mut value = length & 3;
            let mut groups = 0;
            while length & 4 != 0 {
                groups += 1;
                if groups > 12 {
                    return Err("invalid LZCOMP length");
                }
                length = length_tree.read(&mut bits)?;
                value = (value << 2) | (length & 3);
            }
            let mut distance = 0usize;
            for _ in 0..distance_groups {
                distance = (distance << 3) | distance_tree.read(&mut bits)?;
            }
            distance += 1;
            let count = value + 2 + usize::from(distance >= 512);
            let start = history_len
                .checked_sub(distance + count - 1)
                .ok_or("invalid LZCOMP copy distance")?;
            if history_len - PREFIX_SIZE + count > decoded_size {
                return Err("LZCOMP copy exceeds block length");
            }
            for offset in 0..count {
                let byte = *history
                    .get(start + offset)
                    .ok_or("invalid LZCOMP copy source")?;
                history.push(byte);
            }
            (history_len, count)
        };
        if run_length {
            for &byte in &history[start..start + count] {
                append_rle(
                    &mut output,
                    byte,
                    &mut escape,
                    &mut rle_state,
                    &mut rle_count,
                )?;
            }
        } else {
            output.extend_from_slice(&history[start..start + count]);
        }
    }
    if run_length && (escape.is_none() || rle_state != 0) {
        return Err("incomplete LZCOMP run-length data");
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_has_the_specified_size_and_pattern() {
        let bytes = dictionary_prefix();
        assert_eq!(bytes.len(), PREFIX_SIZE);
        assert_eq!(&bytes[..6], &[0, 0, 0, 1, 0, 2]);
        assert_eq!(&bytes[PREFIX_SIZE - 4..], &[255; 4]);
    }

    #[test]
    fn real_mtx_streams_decode_incrementally() {
        let data = super::super::compressed_eot_fixture();
        let eot = super::super::eot::parse_prefix(&data).unwrap().unwrap();
        let mtx = eot.mtx_header(&data).unwrap().unwrap();
        let payload = &data[eot.font_data];
        let blocks: Vec<_> = mtx
            .streams
            .iter()
            .map(|range| {
                let block = &payload[range.clone()];
                let expected = decode_reference(block).unwrap();
                assert_eq!(decode(block).unwrap(), expected);
                for chunk_size in [1, 2, 3, 7, 31, 4096] {
                    let mut decoder = StreamDecoder::new();
                    let mut streamed = Vec::new();
                    for chunk in block.chunks(chunk_size) {
                        streamed.extend(decoder.push(chunk).unwrap());
                    }
                    decoder.finish().unwrap();
                    assert_eq!(streamed, expected, "chunk size {chunk_size}");
                }
                expected
            })
            .collect();
        assert!(!blocks[0].is_empty());
        assert_eq!(&blocks[0][..4], b"\0\x01\0\0");
        assert!(blocks.iter().map(Vec::len).sum::<usize>() > 1_000);
    }

    #[test]
    fn incomplete_incremental_header_is_rejected_on_finish() {
        let mut decoder = StreamDecoder::new();
        assert!(decoder.push(&[0]).unwrap().is_empty());
        assert_eq!(decoder.finish(), Err("truncated LZCOMP stream"));
    }

    #[test]
    fn zero_length_block_decodes_across_header_boundaries() {
        let mut decoder = StreamDecoder::new();
        for byte in [0, 0, 0, 0] {
            assert!(decoder.push(&[byte]).unwrap().is_empty());
        }
        decoder.finish().unwrap();
        assert_eq!(decode(&[0, 0, 0, 0]).unwrap(), Vec::<u8>::new());

        let mut rle = StreamDecoder::new();
        assert!(rle.push(&[0x80, 0, 0, 0]).unwrap().is_empty());
        assert_eq!(rle.finish(), Err("incomplete LZCOMP run-length data"));
    }

    #[test]
    fn literal_symbol_decodes_across_input_chunks() {
        let tree = Huffman::new(256 + 8 + 3);
        let mut code = Vec::new();
        let mut node = tree.leaves[65];
        while node != 1 {
            let parent = tree.nodes[node].parent;
            code.push(usize::from(tree.nodes[parent].right == node));
            node = parent;
        }
        code.reverse();
        let mut bits = vec![0; 25]; // run-length flag and 24-bit output size
        bits[24] = 1;
        bits.extend(code);
        let mut bytes = vec![0; bits.len().div_ceil(8)];
        for (index, bit) in bits.into_iter().enumerate() {
            bytes[index / 8] |= (bit as u8) << (7 - index % 8);
        }
        assert_eq!(decode_reference(&bytes).unwrap(), b"A");
        let mut decoder = StreamDecoder::new();
        let mut result = Vec::new();
        for byte in bytes {
            result.extend(decoder.push(&[byte]).unwrap());
        }
        decoder.finish().unwrap();
        assert_eq!(result, b"A");
    }
}
