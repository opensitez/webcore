//! Compatibility re-exports for the SVG subsystem.
//!
//! New code should use `crate::svg`. This module remains so existing HTML call
//! sites can move gradually without changing behavior.

pub use crate::svg::{load_background_images, rasterize_svg_intrinsic, rasterize_svg_to_rgba};
