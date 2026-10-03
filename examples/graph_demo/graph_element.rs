use std::f32::consts::PI;

use webcore::canvas::{Color, LineCap, parse_color_css};
use webcore::types::CustomElement;
use webcore::Document;

pub struct GraphElement;

impl GraphElement {
    fn size(document: &Document, element: u32) -> (u32, u32) {
        let dimension = |name: &str, default| {
            document
                .get_attribute(element, name)
                .and_then(|s| s.parse::<u32>().ok())
                .filter(|&n| n > 0)
                .unwrap_or(default)
        };
        (dimension("data-width", 340), dimension("data-height", 190))
    }

    fn canvas(document: &Document, element: u32) -> Option<u32> {
        document
            .child_nodes(element)
            .into_iter()
            .find(|&id| document.tag_name(id) == Some("canvas"))
    }

    fn draw(document: &mut Document, element: u32) {
        let Some(canvas) = Self::canvas(document, element) else {
            return;
        };
        let (width, height) = Self::size(document, element);
        let width = width as f32;
        let height = height as f32;
        let chart_type = document
            .get_attribute(element, "data-type")
            .unwrap_or_else(|| "bar".into());
        let values = document
            .get_attribute(element, "data-values")
            .unwrap_or_default()
            .split(',')
            .filter_map(|s| s.trim().parse::<f32>().ok())
            .collect::<Vec<_>>();
        let colors = document
            .get_attribute(element, "data-colors")
            .unwrap_or_default()
            .split(',')
            .filter_map(|s| parse_color_css(s.trim()))
            .collect::<Vec<_>>();
        let palette = [
            Color::rgb(78, 121, 167),
            Color::rgb(242, 142, 43),
            Color::rgb(89, 161, 79),
            Color::rgb(225, 87, 89),
        ];
        let color_at = |i: usize| {
            if colors.is_empty() {
                palette[i % palette.len()]
            } else {
                colors[i % colors.len()]
            }
        };
        document.with_canvas_2d(canvas, |ctx| {
            ctx.clear_rect(0.0, 0.0, width, height);
            ctx.set_fill_color(Color::rgb(22, 27, 34));
            ctx.fill_rect(0.0, 0.0, width, height);
            if values.is_empty() {
                return;
            }

            let margin = 10.0;
            let plot_w = (width - 2.0 * margin).max(1.0);
            let plot_h = (height - 2.0 * margin).max(1.0);
            let maximum = values.iter().copied().fold(0.0_f32, f32::max).max(1.0);
            let n = values.len();
            match chart_type.as_str() {
                "bar" => {
                    let step = plot_w / n as f32;
                    for (i, value) in values.iter().enumerate() {
                        ctx.set_fill_color(color_at(i));
                        let bar_h = (value / maximum) * plot_h;
                        ctx.fill_rect(
                            margin + i as f32 * step + 2.0,
                            height - margin - bar_h,
                            (step - 4.0).max(1.0),
                            bar_h,
                        );
                    }
                }
                "hbar" => {
                    let step = plot_h / n as f32;
                    for (i, value) in values.iter().enumerate() {
                        ctx.set_fill_color(color_at(i));
                        ctx.fill_rect(
                            margin,
                            margin + i as f32 * step + 2.0,
                            value / maximum * plot_w,
                            (step - 4.0).max(1.0),
                        );
                    }
                }
                "scatter" => {
                    let step = plot_w / n.saturating_sub(1).max(1) as f32;
                    for (i, value) in values.iter().enumerate() {
                        ctx.set_fill_color(color_at(i));
                        let x = margin + i as f32 * step;
                        let y = height - margin - value / maximum * plot_h;
                        ctx.begin_path();
                        ctx.arc(x, y, 4.0, 0.0, 2.0 * PI, false);
                        ctx.fill();
                    }
                }
                "line" | "area" => {
                    let step = plot_w / n.saturating_sub(1).max(1) as f32;
                    if chart_type == "area" {
                        ctx.begin_path();
                        ctx.move_to(margin, height - margin);
                        for (i, value) in values.iter().enumerate() {
                            ctx.line_to(
                                margin + i as f32 * step,
                                height - margin - value / maximum * plot_h,
                            );
                        }
                        ctx.line_to(margin + plot_w, height - margin);
                        ctx.close_path();
                        let color = color_at(0);
                        ctx.set_fill_color(Color::rgba(color.r, color.g, color.b, 50));
                        ctx.fill();
                    }
                    ctx.begin_path();
                    for (i, value) in values.iter().enumerate() {
                        let x = margin + i as f32 * step;
                        let y = height - margin - value / maximum * plot_h;
                        if i == 0 { ctx.move_to(x, y) } else { ctx.line_to(x, y) }
                    }
                    ctx.set_stroke_color(color_at(0));
                    ctx.set_line_width(2.0);
                    ctx.stroke();
                }
                "pie" | "donut" => {
                    let total = values.iter().sum::<f32>().abs().max(1.0);
                    let cx = width / 2.0;
                    let cy = height / 2.0 + 10.0;
                    let radius = (width.min(height) / 2.0 - 30.0).max(20.0);
                    let mut angle = -PI / 2.0;
                    for (i, value) in values.iter().enumerate() {
                        let next = angle + value / total * 2.0 * PI;
                        ctx.begin_path();
                        ctx.move_to(cx, cy);
                        ctx.arc(cx, cy, radius, angle, next, false);
                        ctx.close_path();
                        ctx.set_fill_color(color_at(i));
                        ctx.fill();
                        angle = next;
                    }
                    if chart_type == "donut" {
                        ctx.begin_path();
                        ctx.arc(cx, cy, radius * 0.55, 0.0, 2.0 * PI, false);
                        ctx.set_fill_color(Color::rgb(22, 27, 34));
                        ctx.fill();
                    }
                }
                "gauge" => {
                    let percent = ((values[0] - 95.0) / 5.0).clamp(0.0, 1.0);
                    let cx = width / 2.0;
                    let cy = height / 2.0 + 20.0;
                    let radius = (width.min(height) / 2.0 - 30.0).max(20.0);
                    ctx.set_line_width(10.0);
                    ctx.set_line_cap(LineCap::Round);
                    ctx.begin_path();
                    ctx.arc(cx, cy, radius, PI, 2.0 * PI, false);
                    ctx.set_stroke_color(Color::rgb(40, 45, 55));
                    ctx.stroke();
                    ctx.begin_path();
                    ctx.arc(cx, cy, radius, PI, PI + percent * PI, false);
                    ctx.set_stroke_color(color_at(0));
                    ctx.stroke();
                }
                _ => {}
            }
        });
    }
}

impl CustomElement for GraphElement {
    fn observed_attributes(&self) -> &[&str] {
        &["data-type", "data-values", "data-colors", "data-width", "data-height"]
    }

    fn construct(&self, document: &mut Document, element: u32) {
        let canvas = document.create_element("canvas");
        let (width, height) = Self::size(document, element);
        document.set_attribute(canvas, "width", &width.to_string());
        document.set_attribute(canvas, "height", &height.to_string());
        document.append_child(element, canvas);
    }

    fn connected_callback(&self, document: &mut Document, element: u32) {
        Self::draw(document, element);
    }

    fn attribute_changed_callback(
        &self,
        document: &mut Document,
        element: u32,
        name: &str,
        _old_value: Option<&str>,
        _new_value: Option<&str>,
    ) {
        if matches!(name, "data-width" | "data-height") {
            if let Some(canvas) = Self::canvas(document, element) {
                let (width, height) = Self::size(document, element);
                document.set_attribute(canvas, "width", &width.to_string());
                document.set_attribute(canvas, "height", &height.to_string());
            }
        }
        Self::draw(document, element);
    }
}
