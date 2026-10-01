//! HTML: the tokenizer, the tree-construction parser, and the pieces the
//! parse needs — charset sniffing and entity decoding. Image loading lives in
//! `crate::images` and is re-exported here for existing callers.
//!
//! ⛔ DECLARES and RE-EXPORTS. This file held 3,304 lines; the folder around
//! it existed the whole time. Call sites say `crate::html::X`, so the glob
//! re-exports keep one path to each item.

use crate::types::Document;

pub mod arena_wiring;
pub mod charset;
pub mod default_display;
pub mod doctype;
pub mod entities;
pub mod entity_decode;
pub mod forms;
pub mod head;
pub mod html_children;
pub use crate::images;
pub mod parser;
pub mod post_process;
pub mod presentational;
pub mod public_api;
pub mod serializer;
pub mod streaming;
pub mod table_normalize;
pub mod tokenizer;
pub mod validity;

pub use arena_wiring::*;
pub use charset::*;
pub use default_display::*;
pub use doctype::*;
pub use entities::*;
pub use entity_decode::*;
pub use forms::*;
pub(crate) use head::*;
pub(crate) use html_children::*;
pub use images::*;
pub(crate) use parser::*;
pub use post_process::*;
pub(crate) use presentational::*;
pub use public_api::*;
pub use serializer::*;
pub use streaming::*;
pub(crate) use table_normalize::*;
pub(crate) use tokenizer::*;
pub use validity::*;
