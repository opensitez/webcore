//! HTML: the tokenizer, the tree-construction parser, and the pieces the
//! parse needs — charset sniffing, entity decoding, image loading, SVG.
//!
//! ⛔ DECLARES and RE-EXPORTS. This file held 3,304 lines; the folder around
//! it existed the whole time. Call sites say `crate::html::X`, so the glob
//! re-exports keep one path to each item.

use crate::css::{apply_property, ua_stylesheet};
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
pub mod images;
pub mod parser;
pub mod post_cascade;
pub mod post_process;
pub mod presentational;
pub mod public_api;
pub mod serializer;
pub mod streaming;
pub mod svg;
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
pub use head::*;
pub use html_children::*;
pub use images::*;
pub use parser::*;
pub use post_cascade::*;
pub use post_process::*;
pub use presentational::*;
pub use public_api::*;
pub use serializer::*;
pub use streaming::*;
pub use svg::*;
pub use table_normalize::*;
pub use tokenizer::*;
pub use validity::*;
