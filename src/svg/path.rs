//! SVG path, transform, and numeric-list parsing.

use tiny_skia::{Path, PathSegment, Transform};

pub(crate) fn flatten_path_points(path: &Path) -> Vec<(f32, f32)> {
    let mut points = Vec::new();
    let mut start = (0.0f32, 0.0f32);
    let mut current = (0.0f32, 0.0f32);
    for segment in path.segments() {
        match segment {
            PathSegment::MoveTo(p) => {
                start = (p.x, p.y);
                current = start;
                points.push(current);
            }
            PathSegment::LineTo(p) => {
                current = (p.x, p.y);
                points.push(current);
            }
            PathSegment::QuadTo(c, p) => {
                let end = (p.x, p.y);
                for (_, to) in flatten_quad_points(current, (c.x, c.y), end) {
                    points.push(to);
                }
                current = end;
            }
            PathSegment::CubicTo(c1, c2, p) => {
                let end = (p.x, p.y);
                for (_, to) in flatten_cubic_points(current, (c1.x, c1.y), (c2.x, c2.y), end) {
                    points.push(to);
                }
                current = end;
            }
            PathSegment::Close => {
                current = start;
                points.push(current);
            }
        }
    }
    points
}

fn flatten_quad_points(
    p0: (f32, f32),
    c: (f32, f32),
    p1: (f32, f32),
) -> Vec<((f32, f32), (f32, f32))> {
    let mut out = Vec::new();
    let mut prev = p0;
    for i in 1..=16 {
        let t = i as f32 / 16.0;
        let mt = 1.0 - t;
        let next = (
            mt * mt * p0.0 + 2.0 * mt * t * c.0 + t * t * p1.0,
            mt * mt * p0.1 + 2.0 * mt * t * c.1 + t * t * p1.1,
        );
        out.push((prev, next));
        prev = next;
    }
    out
}

fn flatten_cubic_points(
    p0: (f32, f32),
    c1: (f32, f32),
    c2: (f32, f32),
    p1: (f32, f32),
) -> Vec<((f32, f32), (f32, f32))> {
    let mut out = Vec::new();
    let mut prev = p0;
    for i in 1..=24 {
        let t = i as f32 / 24.0;
        let mt = 1.0 - t;
        let next = (
            mt.powi(3) * p0.0
                + 3.0 * mt.powi(2) * t * c1.0
                + 3.0 * mt * t.powi(2) * c2.0
                + t.powi(3) * p1.0,
            mt.powi(3) * p0.1
                + 3.0 * mt.powi(2) * t * c1.1
                + 3.0 * mt * t.powi(2) * c2.1
                + t.powi(3) * p1.1,
        );
        out.push((prev, next));
        prev = next;
    }
    out
}

pub(crate) fn path_polyline_length(points: &[(f32, f32)]) -> f32 {
    points
        .windows(2)
        .map(|pair| segment_length(pair[0], pair[1]))
        .sum()
}

pub(crate) fn point_at_path_distance(
    points: &[(f32, f32)],
    distance: f32,
) -> Option<(f32, f32, f32)> {
    let mut remaining = distance.max(0.0);
    for pair in points.windows(2) {
        let from = pair[0];
        let to = pair[1];
        let len = segment_length(from, to);
        if len <= f32::EPSILON {
            continue;
        }
        if remaining <= len {
            let t = remaining / len;
            let x = from.0 + (to.0 - from.0) * t;
            let y = from.1 + (to.1 - from.1) * t;
            return Some((x, y, (to.1 - from.1).atan2(to.0 - from.0)));
        }
        remaining -= len;
    }
    points.windows(2).last().and_then(|pair| {
        let from = pair[0];
        let to = pair[1];
        let len = segment_length(from, to);
        (len > f32::EPSILON).then(|| (to.0, to.1, (to.1 - from.1).atan2(to.0 - from.0)))
    })
}

fn segment_length(from: (f32, f32), to: (f32, f32)) -> f32 {
    ((to.0 - from.0).powi(2) + (to.1 - from.1).powi(2)).sqrt()
}

pub(crate) use webcanvas::canvas::svg_path::path_marker_subpaths;

pub(crate) fn number(value: &str) -> Option<f32> {
    let token = value
        .trim()
        .split(|c: char| c == ';' || c.is_whitespace())
        .next()
        .unwrap_or("");
    let end = token
        .char_indices()
        .take_while(|(_, c)| c.is_ascii_digit() || matches!(c, '.' | '+' | '-' | 'e' | 'E'))
        .last()
        .map(|(idx, c)| idx + c.len_utf8())
        .unwrap_or(0);
    if end == 0 {
        return None;
    }
    token[..end].parse::<f32>().ok().filter(|v| v.is_finite())
}

pub(crate) fn number_list(value: &str) -> Vec<f32> {
    value
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter_map(number)
        .collect()
}

pub(crate) fn parse_transform_list(value: &str) -> Option<Transform> {
    let mut rest = value.trim();
    let mut transform = Transform::identity();
    while !rest.is_empty() {
        let open = rest.find('(')?;
        let name = rest[..open].trim();
        let close_rel = rest[open + 1..].find(')')?;
        let args = number_list(&rest[open + 1..open + 1 + close_rel]);
        let local = match name {
            "matrix" if args.len() >= 6 => {
                Transform::from_row(args[0], args[1], args[2], args[3], args[4], args[5])
            }
            "translate" if !args.is_empty() => {
                Transform::from_translate(args[0], args.get(1).copied().unwrap_or(0.0))
            }
            "scale" if !args.is_empty() => {
                Transform::from_scale(args[0], args.get(1).copied().unwrap_or(args[0]))
            }
            "rotate" if args.len() >= 3 => Transform::from_rotate_at(args[0], args[1], args[2]),
            "rotate" if !args.is_empty() => Transform::from_rotate(args[0]),
            "skewX" if !args.is_empty() => Transform::from_skew(args[0].to_radians().tan(), 0.0),
            "skewY" if !args.is_empty() => Transform::from_skew(0.0, args[0].to_radians().tan()),
            _ => return None,
        };
        transform = transform.pre_concat(local);
        rest = rest[open + 1 + close_rel + 1..].trim_start();
    }
    Some(transform)
}

pub(crate) use webcanvas::canvas::svg_path::parse_path_data;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_arc_circle_path_has_expected_length() {
        let data = "M337.5,337.5 m-320,0 a320,320 0 1,1 640,0 a320,320 0 1,1 -640,0";
        let path = parse_path_data(data).expect("circle path parses");
        let points = flatten_path_points(&path);
        let length = path_polyline_length(&points);
        let expected = std::f32::consts::TAU * 320.0;
        assert!(
            (length - expected).abs() < 20.0,
            "length {length} should be close to circumference {expected}"
        );
    }
}
