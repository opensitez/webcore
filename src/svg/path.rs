//! SVG path, transform, and numeric-list parsing.

use tiny_skia::{Path, PathSegment, Transform};

struct MeasuredSegment {
    from: (f32, f32),
    to: (f32, f32),
    end_distance: f32,
    length: f32,
    angle: f32,
}

pub(crate) struct PathMetrics {
    segments: Vec<MeasuredSegment>,
    length: f32,
}

impl PathMetrics {
    fn push(&mut self, from: (f32, f32), to: (f32, f32)) {
        let length = segment_length(from, to);
        if !length.is_finite() || length <= f32::EPSILON {
            return;
        }
        self.length += length;
        self.segments.push(MeasuredSegment {
            from,
            to,
            end_distance: self.length,
            length,
            angle: (to.1 - from.1).atan2(to.0 - from.0),
        });
    }

    pub(crate) fn length(&self) -> f32 {
        self.length
    }

    pub(crate) fn point_at_distance(&self, distance: f32) -> Option<(f32, f32, f32)> {
        if distance.is_nan() {
            return None;
        }
        let distance = distance.clamp(0.0, self.length);
        let index = self.segments.partition_point(|segment| segment.end_distance < distance);
        let segment = self.segments.get(index)?;
        let start_distance = if index == 0 { 0.0 } else { self.segments[index - 1].end_distance };
        let t = ((distance - start_distance) / segment.length).clamp(0.0, 1.0);
        Some((
            segment.from.0 + (segment.to.0 - segment.from.0) * t,
            segment.from.1 + (segment.to.1 - segment.from.1) * t,
            segment.angle,
        ))
    }
}

pub(crate) fn measure_path(path: &Path) -> PathMetrics {
    let mut metrics = PathMetrics { segments: Vec::new(), length: 0.0 };
    let mut start = (0.0f32, 0.0f32);
    let mut current = (0.0f32, 0.0f32);
    for segment in path.segments() {
        match segment {
            PathSegment::MoveTo(p) => {
                start = (p.x, p.y);
                current = start;
            }
            PathSegment::LineTo(p) => {
                let end = (p.x, p.y);
                metrics.push(current, end);
                current = end;
            }
            PathSegment::QuadTo(c, p) => {
                let end = (p.x, p.y);
                flatten_quad_points(current, (c.x, c.y), end, &mut metrics);
                current = end;
            }
            PathSegment::CubicTo(c1, c2, p) => {
                let end = (p.x, p.y);
                flatten_cubic_points(current, (c1.x, c1.y), (c2.x, c2.y), end, &mut metrics);
                current = end;
            }
            PathSegment::Close => {
                metrics.push(current, start);
                current = start;
            }
        }
    }
    metrics
}

fn flatten_quad_points(
    p0: (f32, f32),
    c: (f32, f32),
    p1: (f32, f32),
    metrics: &mut PathMetrics,
) {
    let mut prev = p0;
    for i in 1..=16 {
        let t = i as f32 / 16.0;
        let mt = 1.0 - t;
        let next = (
            mt * mt * p0.0 + 2.0 * mt * t * c.0 + t * t * p1.0,
            mt * mt * p0.1 + 2.0 * mt * t * c.1 + t * t * p1.1,
        );
        metrics.push(prev, next);
        prev = next;
    }
}

fn flatten_cubic_points(
    p0: (f32, f32),
    c1: (f32, f32),
    c2: (f32, f32),
    p1: (f32, f32),
    metrics: &mut PathMetrics,
) {
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
        metrics.push(prev, next);
        prev = next;
    }
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
        let length = measure_path(&path).length();
        let expected = std::f32::consts::TAU * 320.0;
        assert!(
            (length - expected).abs() < 20.0,
            "length {length} should be close to circumference {expected}"
        );
    }

    #[test]
    fn measured_paths_do_not_connect_moveto_subpaths() {
        let path = parse_path_data("M0 0 L10 0 M100 100 L100 110 M500 500").unwrap();
        let metrics = measure_path(&path);
        assert_eq!(metrics.length(), 20.0);
        assert_eq!(metrics.point_at_distance(5.0), Some((5.0, 0.0, 0.0)));
        assert_eq!(metrics.point_at_distance(10.0), Some((10.0, 0.0, 0.0)));
        assert_eq!(metrics.point_at_distance(15.0), Some((100.0, 105.0, std::f32::consts::FRAC_PI_2)));
        assert_eq!(metrics.point_at_distance(100.0), Some((100.0, 110.0, std::f32::consts::FRAC_PI_2)));
        assert_eq!(metrics.point_at_distance(-1.0), metrics.point_at_distance(0.0));
        assert_eq!(metrics.point_at_distance(f32::NAN), None);
    }

    #[test]
    fn measured_paths_preserve_close_and_ignore_zero_length_segments() {
        let path = parse_path_data("M0 0 L3 0 L3 4 Z M50 50 L50 50").unwrap();
        let metrics = measure_path(&path);
        assert_eq!(metrics.length(), 12.0);
        let (x, y, _) = metrics.point_at_distance(9.5).unwrap();
        assert!((x - 1.5).abs() < 0.001 && (y - 2.0).abs() < 0.001);
        assert_eq!(metrics.point_at_distance(12.0).unwrap().0, 0.0);
    }
}
