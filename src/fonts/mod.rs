//! Compatibility facade for web font decoding in `webmedia::font`.

pub use webmedia::font::{WOFF1_MAGIC, WOFF2_MAGIC, ctf, decode, eot, lzcomp, mtx, woff2};

#[cfg(test)]
pub(crate) fn compressed_eot_fixture() -> Vec<u8> {
    match std::env::var("WEBCORE_EOT_FIXTURE") {
        Ok(path) => std::fs::read(path).expect("read EOT test fixture"),
        Err(_) => include_bytes!("../tests/fixtures/fonts/roboto-v20-latin-regular.eot").to_vec(),
    }
}
