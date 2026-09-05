//! Web Open Font Format decoding.
//!
//! Layout and font matching should not know container details. They hand raw
//! `@font-face` bytes here and receive an sfnt/OTF/TTF byte stream suitable for
//! fontdb, or `None` when the container is malformed or unsupported.

mod woff1;
pub mod woff2;

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
