//! SVG support for webcore.
//!
//! SVG is parsed into a native tree and rasterized by webcore. The legacy
//! fallback module still owns compatibility helpers used by HTML call sites
//! during migration, but the default SVG image raster path no longer depends on
//! resvg.

pub mod fallback;
pub mod geometry;
pub mod paint;
pub mod parser;
pub mod tree;

pub use fallback::{load_background_images, rasterize_svg_intrinsic, rasterize_svg_to_rgba};
pub use geometry::{intrinsic_size_from_markup, PreserveAspectRatio, SvgLength, SvgViewBox};
pub use paint::rasterize_svg_document_to_rgba;
pub(crate) use paint::rasterize_svg_document_to_rgba_with_vars;
pub use parser::{parse_svg_document, SvgParseError};
pub use tree::{SvgAttribute, SvgDocument, SvgElementKind, SvgNode};

pub(crate) use fallback::{
    build_inline_svg_fallback, parse_px, parse_svg_length_px, parse_viewbox_value,
    prepare_svg_for_rasterization, style_px,
};
