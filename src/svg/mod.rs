//! SVG support for webcore.
//!
//! SVG is parsed into a native tree and rasterized by webcore. HTML integration
//! builds source text only as parser input; paint uses `SvgDocument`.

pub mod animation;
pub(crate) mod condition;
pub mod geometry;
pub mod paint;
pub mod parser;
pub mod path;
pub mod resources;
pub mod source;
pub mod tree;
pub mod unsupported;

pub use animation::{
    SvgAccumulateMode, SvgAdditiveMode, SvgAnimateMotionRotate, SvgAnimateTransformType,
    SvgAnimationElement, SvgAnimationFillMode, SvgAnimationKind, SvgAnimationTime, SvgCalcMode,
    SvgRepeatCount,
};
pub use geometry::{
    PreserveAspectRatio, SvgLength, SvgViewBox, has_ratio_only, has_ratio_only_from_markup,
    intrinsic_size_from_markup,
};
pub(crate) use paint::rasterize_svg_document_to_rgba_with_dom;
pub use paint::{rasterize_svg_document_to_rgba, rasterize_svg_intrinsic, rasterize_svg_to_rgba};
pub use parser::{SvgParseError, parse_svg_document};
pub use resources::load_background_images;
pub use tree::{SvgAttribute, SvgDocument, SvgElementKind, SvgNode};
pub use unsupported::{SvgUnsupportedSummary, unsupported_summary};

pub(crate) use animation::{svg_document_with_animation_overrides, tick_svg_animations};
pub(crate) use source::build_inline_svg_source;
