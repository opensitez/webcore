//! Browser integration for the shared WHATWG Canvas implementation.

pub use webcanvas::canvas::*;

pub mod effects;
#[cfg(test)]
mod surface_tests;
mod syntax_adapter;

pub(crate) use syntax_adapter::browser_canvas_syntax;

pub fn parse_color_css(css: &str) -> Option<Color> {
    syntax_adapter::parse_canvas_color(css)
}
