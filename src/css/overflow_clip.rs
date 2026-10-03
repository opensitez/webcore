use crate::types::{ComputedStyle, CssLength, LayoutBox, Overflow, Rect};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OverflowClipBox {
    Content,
    Padding,
    Border,
}

pub(crate) fn parse_overflow_clip_margin(value: &str) -> Option<(OverflowClipBox, CssLength)> {
    let mut box_edge = None;
    let mut offset = None;
    for part in super::value_parse::split_css_values(value) {
        let edge = match part.as_str() {
            "content-box" => Some(OverflowClipBox::Content),
            "padding-box" => Some(OverflowClipBox::Padding),
            "border-box" => Some(OverflowClipBox::Border),
            _ => None,
        };
        if let Some(edge) = edge {
            if box_edge.replace(edge).is_some() {
                return None;
            }
        } else {
            if offset.is_some() || part.parse::<f32>().is_ok_and(|n| n != 0.0) {
                return None;
            }
            let length = super::parse_length_checked(&part)?;
            if length.has_percentage()
                || matches!(
                    length,
                    CssLength::Auto
                        | CssLength::None
                        | CssLength::Content
                        | CssLength::MinContent
                        | CssLength::MaxContent
                        | CssLength::FitContent
                        | CssLength::Stretch
                        | CssLength::FitContentArg(_)
                )
            {
                return None;
            }
            offset = Some(length);
        }
    }
    if box_edge.is_none() && offset.is_none() {
        return None;
    }
    Some((
        box_edge.unwrap_or(OverflowClipBox::Padding),
        offset.unwrap_or(CssLength::Zero),
    ))
}

pub(crate) fn overflow_clip_rect(
    style: &ComputedStyle,
    layout: &LayoutBox,
    scroll_x: f32,
    scroll_y: f32,
    font_px: f32,
    root_font_px: f32,
) -> Rect {
    let padding = layout.padding_rect;
    if style.overflow_x != Overflow::Clip && style.overflow_y != Overflow::Clip {
        return Rect::new(
            padding.x - scroll_x,
            padding.y - scroll_y,
            padding.w,
            padding.h,
        );
    }
    let (edge, length) = parse_overflow_clip_margin(&style.overflow_clip_margin)
        .unwrap_or((OverflowClipBox::Padding, CssLength::Zero));
    let selected = match edge {
        OverflowClipBox::Content => layout.content_rect,
        OverflowClipBox::Padding => padding,
        OverflowClipBox::Border => layout.border_rect,
    };
    let offset = length.resolve(font_px, padding.w, root_font_px);
    let horizontal = if style.overflow_x == Overflow::Clip {
        selected
    } else {
        padding
    };
    let vertical = if style.overflow_y == Overflow::Clip {
        selected
    } else {
        padding
    };
    let dx = if style.overflow_x == Overflow::Clip {
        offset
    } else {
        0.0
    };
    let dy = if style.overflow_y == Overflow::Clip {
        offset
    } else {
        0.0
    };
    Rect::new(
        horizontal.x - scroll_x - dx,
        vertical.y - scroll_y - dy,
        (horizontal.w + 2.0 * dx).max(0.0),
        (vertical.h + 2.0 * dy).max(0.0),
    )
}

pub(crate) fn overflow_clip_radii(
    style: &ComputedStyle,
    layout: &LayoutBox,
    font_px: f32,
    root_font_px: f32,
) -> ([f32; 4], [f32; 4]) {
    let rect = overflow_clip_rect(style, layout, 0.0, 0.0, font_px, root_font_px);
    crate::types::shape_box_radii(style, layout, rect, root_font_px)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overflow_clip_margin_uses_visual_box_and_one_length() {
        for value in [
            "10px",
            "-10px",
            "content-box",
            "content-box 10px",
            "10px content-box",
            "border-box -10px",
            "padding-box calc(100px - 50px)",
        ] {
            assert!(parse_overflow_clip_margin(value).is_some(), "{value}");
        }
        for value in [
            "margin-box",
            "inset(10px)",
            "50px 50px",
            "content-box border-box",
            "calc(100% - 50px)",
            "border-box calc(0.5em - 100%)",
            "10",
        ] {
            assert!(parse_overflow_clip_margin(value).is_none(), "{value}");
        }
    }

    #[test]
    fn overflow_clip_edge_follows_selected_box_and_negative_offset() {
        let mut layout = LayoutBox::default();
        layout.border_rect = Rect::new(0.0, 0.0, 140.0, 140.0);
        layout.padding_rect = Rect::new(10.0, 10.0, 120.0, 120.0);
        layout.content_rect = Rect::new(20.0, 20.0, 100.0, 100.0);
        let mut style = ComputedStyle::default();
        style.overflow_x = Overflow::Clip;
        style.overflow_y = Overflow::Clip;

        style.overflow_clip_margin = "content-box 5px".into();
        assert_eq!(
            overflow_clip_rect(&style, &layout, 0.0, 0.0, 16.0, 16.0),
            Rect::new(15.0, 15.0, 110.0, 110.0)
        );
        style.overflow_clip_margin = "border-box -10px".into();
        assert_eq!(
            overflow_clip_rect(&style, &layout, 0.0, 0.0, 16.0, 16.0),
            Rect::new(10.0, 10.0, 120.0, 120.0)
        );

        style.overflow_x = Overflow::Hidden;
        style.overflow_y = Overflow::Hidden;
        style.overflow_clip_margin = "border-box 5px".into();
        assert_eq!(
            overflow_clip_rect(&style, &layout, 0.0, 0.0, 16.0, 16.0),
            layout.padding_rect
        );
    }
}
