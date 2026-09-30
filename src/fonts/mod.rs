//! Web font container decoding.
//!
//! Layout and font matching should not know container details. They hand raw
//! `@font-face` bytes here and receive an sfnt/OTF/TTF byte stream suitable for
//! fontdb, or `None` when the container is malformed or unsupported.

pub mod ctf;
pub mod eot;
pub mod lzcomp;
pub mod mtx;
mod woff1;
pub mod woff2;

#[cfg(test)]
pub(crate) fn compressed_eot_fixture() -> Vec<u8> {
    match std::env::var("WEBCORE_EOT_FIXTURE") {
        Ok(path) => std::fs::read(path).expect("read EOT test fixture"),
        Err(_) => include_bytes!("../tests/fixtures/fonts/roboto-v20-latin-regular.eot").to_vec(),
    }
}

/// WOFF2 magic bytes: `wOF2` (0x774F4632).
pub const WOFF2_MAGIC: [u8; 4] = [0x77, 0x4F, 0x46, 0x32];

/// WOFF1 magic bytes: `wOFF` (0x774F4646).
pub const WOFF1_MAGIC: [u8; 4] = [0x77, 0x4F, 0x46, 0x46];

/// Decode a WOFF container into raw sfnt bytes.
pub fn decode(data: &[u8]) -> Option<Vec<u8>> {
    if data.starts_with(&WOFF2_MAGIC) {
        woff2::decode(data)
    } else if data.starts_with(&WOFF1_MAGIC) {
        woff1::decode(data)
    } else {
        None
    }
}
