use super::{CanvasSyntax, Color, CssFilters, FilterOp, Font, FontStyle, FontWeight};

pub(super) fn parse_canvas_color(css: &str) -> Option<Color> {
    crate::css::parse_color(css).map(|c| Color::rgba(c.r, c.g, c.b, c.a))
}

fn parse_canvas_font(css: &str) -> Option<Font> {
    let value = css.trim();
    if value.is_empty() {
        return None;
    }
    let mut style = crate::types::ComputedStyle::default();
    let before = style.font_size.clone();
    crate::css::apply_font_shorthand(&mut style, value);
    if style.font_family.is_empty() || style.font_size == before {
        return None;
    }
    Some(Font {
        family: style.font_family.clone(),
        size: style.font_size.resolve(16.0, 16.0, 16.0),
        weight: match style.font_weight {
            crate::types::FontWeight::Bold => FontWeight::Bold,
            _ => FontWeight::Normal,
        },
        style: match style.font_style {
            crate::types::FontStyle::Italic => FontStyle::Italic,
            _ => FontStyle::Normal,
        },
    })
}

fn parse_canvas_filter(css: &str) -> CssFilters {
    use crate::types::FilterOp as BrowserOp;
    let parsed = crate::css::parse_css_filter(css);
    CssFilters {
        ops: parsed
            .ops
            .into_iter()
            .map(|op| match op {
                BrowserOp::Blur(v) => FilterOp::Blur(v),
                BrowserOp::Brightness(v) => FilterOp::Brightness(v),
                BrowserOp::Contrast(v) => FilterOp::Contrast(v),
                BrowserOp::Grayscale(v) => FilterOp::Grayscale(v),
                BrowserOp::HueRotate(v) => FilterOp::HueRotate(v),
                BrowserOp::Invert(v) => FilterOp::Invert(v),
                BrowserOp::Opacity(v) => FilterOp::Opacity(v),
                BrowserOp::Saturate(v) => FilterOp::Saturate(v),
                BrowserOp::Sepia(v) => FilterOp::Sepia(v),
                BrowserOp::DropShadow {
                    dx,
                    dy,
                    blur,
                    color,
                } => FilterOp::DropShadow {
                    dx,
                    dy,
                    blur,
                    color: Color::rgba(color.r, color.g, color.b, color.a),
                },
            })
            .collect(),
    }
}

pub(crate) fn browser_canvas_syntax() -> CanvasSyntax {
    CanvasSyntax {
        parse_color: parse_canvas_color,
        parse_font: parse_canvas_font,
        parse_filter: parse_canvas_filter,
    }
}
