//! SVG path, transform, and numeric-list parsing.

use tiny_skia::{Path, PathBuilder, PathSegment, Transform};

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

pub(crate) struct MarkerSubpath {
    pub(crate) points: Vec<(f32, f32)>,
    pub(crate) closed: bool,
}

pub(crate) fn path_marker_subpaths(data: &str) -> Vec<MarkerSubpath> {
    let mut p = PathDataParser {
        data,
        pos: 0,
        cmd: 'M',
        x: 0.0,
        y: 0.0,
        sx: 0.0,
        sy: 0.0,
        last_cubic_ctrl: None,
        last_quad_ctrl: None,
    };
    let mut subpaths = Vec::new();
    let mut points: Vec<(f32, f32)> = Vec::new();
    let mut closed = false;
    while p.skip_separators() {
        if let Some(c) = p.peek_cmd() {
            p.cmd = c;
            p.pos += c.len_utf8();
        }
        let relative = p.cmd.is_ascii_lowercase();
        match p.cmd.to_ascii_uppercase() {
            'M' => {
                let Some((x, y)) = p.pair(relative) else {
                    break;
                };
                if points.len() >= 2 {
                    subpaths.push(MarkerSubpath { points, closed });
                }
                points = Vec::new();
                closed = false;
                p.x = x;
                p.y = y;
                p.sx = x;
                p.sy = y;
                points.push((x, y));
                p.cmd = if relative { 'l' } else { 'L' };
                p.clear_controls();
            }
            'L' => {
                let Some((x, y)) = p.pair(relative) else {
                    break;
                };
                p.x = x;
                p.y = y;
                points.push((x, y));
                p.clear_controls();
            }
            'H' => {
                let Some(mut x) = p.num() else { break };
                if relative {
                    x += p.x;
                }
                p.x = x;
                points.push((p.x, p.y));
                p.clear_controls();
            }
            'V' => {
                let Some(mut y) = p.num() else { break };
                if relative {
                    y += p.y;
                }
                p.y = y;
                points.push((p.x, p.y));
                p.clear_controls();
            }
            'C' => {
                if p.pair(relative).is_none() || p.pair(relative).is_none() {
                    break;
                }
                let Some((x, y)) = p.pair(relative) else {
                    break;
                };
                p.x = x;
                p.y = y;
                points.push((x, y));
                p.clear_controls();
            }
            'S' | 'Q' => {
                if p.pair(relative).is_none() {
                    break;
                }
                let Some((x, y)) = p.pair(relative) else {
                    break;
                };
                p.x = x;
                p.y = y;
                points.push((x, y));
                p.clear_controls();
            }
            'T' => {
                let Some((x, y)) = p.pair(relative) else {
                    break;
                };
                p.x = x;
                p.y = y;
                points.push((x, y));
                p.clear_controls();
            }
            'A' => {
                if p.num().is_none()
                    || p.num().is_none()
                    || p.num().is_none()
                    || p.flag().is_none()
                    || p.flag().is_none()
                {
                    break;
                }
                let Some((x, y)) = p.pair(relative) else {
                    break;
                };
                p.x = x;
                p.y = y;
                points.push((x, y));
                p.clear_controls();
            }
            'Z' => {
                points.push((p.sx, p.sy));
                p.x = p.sx;
                p.y = p.sy;
                closed = true;
                p.clear_controls();
            }
            _ => break,
        }
    }
    if points.len() >= 2 {
        subpaths.push(MarkerSubpath { points, closed });
    }
    subpaths
}

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

pub(crate) fn parse_path_data(data: &str) -> Option<Path> {
    let mut p = PathDataParser {
        data,
        pos: 0,
        cmd: 'M',
        x: 0.0,
        y: 0.0,
        sx: 0.0,
        sy: 0.0,
        last_cubic_ctrl: None,
        last_quad_ctrl: None,
    };
    let mut b = PathBuilder::new();
    while p.skip_separators() {
        if let Some(c) = p.peek_cmd() {
            p.cmd = c;
            p.pos += c.len_utf8();
        }
        let relative = p.cmd.is_ascii_lowercase();
        match p.cmd.to_ascii_uppercase() {
            'M' => {
                let (x, y) = p.pair(relative)?;
                b.move_to(x, y);
                p.x = x;
                p.y = y;
                p.sx = x;
                p.sy = y;
                p.cmd = if relative { 'l' } else { 'L' };
                p.clear_controls();
            }
            'L' => {
                let (x, y) = p.pair(relative)?;
                b.line_to(x, y);
                p.x = x;
                p.y = y;
                p.clear_controls();
            }
            'H' => {
                let mut x = p.num()?;
                if relative {
                    x += p.x;
                }
                b.line_to(x, p.y);
                p.x = x;
                p.clear_controls();
            }
            'V' => {
                let mut y = p.num()?;
                if relative {
                    y += p.y;
                }
                b.line_to(p.x, y);
                p.y = y;
                p.clear_controls();
            }
            'C' => {
                let (x1, y1) = p.pair(relative)?;
                let (x2, y2) = p.pair(relative)?;
                let (x, y) = p.pair(relative)?;
                b.cubic_to(x1, y1, x2, y2, x, y);
                p.x = x;
                p.y = y;
                p.last_cubic_ctrl = Some((x2, y2));
                p.last_quad_ctrl = None;
            }
            'S' => {
                let (x1, y1) = p
                    .last_cubic_ctrl
                    .map(|(cx, cy)| (p.x * 2.0 - cx, p.y * 2.0 - cy))
                    .unwrap_or((p.x, p.y));
                let (x2, y2) = p.pair(relative)?;
                let (x, y) = p.pair(relative)?;
                b.cubic_to(x1, y1, x2, y2, x, y);
                p.x = x;
                p.y = y;
                p.last_cubic_ctrl = Some((x2, y2));
                p.last_quad_ctrl = None;
            }
            'Q' => {
                let (x1, y1) = p.pair(relative)?;
                let (x, y) = p.pair(relative)?;
                b.quad_to(x1, y1, x, y);
                p.x = x;
                p.y = y;
                p.last_quad_ctrl = Some((x1, y1));
                p.last_cubic_ctrl = None;
            }
            'T' => {
                let (x1, y1) = p
                    .last_quad_ctrl
                    .map(|(qx, qy)| (p.x * 2.0 - qx, p.y * 2.0 - qy))
                    .unwrap_or((p.x, p.y));
                let (x, y) = p.pair(relative)?;
                b.quad_to(x1, y1, x, y);
                p.x = x;
                p.y = y;
                p.last_quad_ctrl = Some((x1, y1));
                p.last_cubic_ctrl = None;
            }
            'A' => {
                let rx = p.num()?.abs();
                let ry = p.num()?.abs();
                let angle = p.num()?;
                let large_arc = p.flag()?;
                let sweep = p.flag()?;
                let (x, y) = p.pair(relative)?;
                arc_to_cubic(&mut b, p.x, p.y, rx, ry, angle, large_arc, sweep, x, y);
                p.x = x;
                p.y = y;
                p.clear_controls();
            }
            'Z' => {
                b.close();
                p.x = p.sx;
                p.y = p.sy;
                p.clear_controls();
            }
            _ => return None,
        }
    }
    b.finish()
}

struct PathDataParser<'a> {
    data: &'a str,
    pos: usize,
    cmd: char,
    x: f32,
    y: f32,
    sx: f32,
    sy: f32,
    last_cubic_ctrl: Option<(f32, f32)>,
    last_quad_ctrl: Option<(f32, f32)>,
}

impl<'a> PathDataParser<'a> {
    fn skip_separators(&mut self) -> bool {
        while let Some(c) = self.peek() {
            if c.is_whitespace() || c == ',' {
                self.pos += c.len_utf8();
            } else {
                break;
            }
        }
        self.pos < self.data.len()
    }

    fn peek_cmd(&self) -> Option<char> {
        self.peek().filter(|c| c.is_ascii_alphabetic())
    }

    fn peek(&self) -> Option<char> {
        self.data[self.pos..].chars().next()
    }

    fn pair(&mut self, relative: bool) -> Option<(f32, f32)> {
        let mut x = self.num()?;
        let mut y = self.num()?;
        if relative {
            x += self.x;
            y += self.y;
        }
        Some((x, y))
    }

    fn flag(&mut self) -> Option<bool> {
        self.skip_separators();
        match self.peek()? {
            '0' => {
                self.pos += 1;
                Some(false)
            }
            '1' => {
                self.pos += 1;
                Some(true)
            }
            _ => None,
        }
    }

    fn clear_controls(&mut self) {
        self.last_cubic_ctrl = None;
        self.last_quad_ctrl = None;
    }

    fn num(&mut self) -> Option<f32> {
        self.skip_separators();
        let start = self.pos;
        let mut saw_digit = false;
        let mut saw_exp = false;
        while let Some(c) = self.peek() {
            let ok = if c.is_ascii_digit() {
                saw_digit = true;
                true
            } else if matches!(c, '+' | '-') {
                self.pos == start || saw_exp
            } else if c == '.' {
                true
            } else if matches!(c, 'e' | 'E') {
                saw_exp = true;
                true
            } else {
                false
            };
            if !ok {
                break;
            }
            if saw_exp && !matches!(c, 'e' | 'E') {
                saw_exp = false;
            }
            self.pos += c.len_utf8();
        }
        if !saw_digit || self.pos == start {
            return None;
        }
        self.data[start..self.pos].parse::<f32>().ok()
    }
}

fn arc_to_cubic(
    b: &mut PathBuilder,
    x1: f32,
    y1: f32,
    mut rx: f32,
    mut ry: f32,
    x_axis_rotation: f32,
    large_arc: bool,
    sweep: bool,
    x2: f32,
    y2: f32,
) {
    if rx == 0.0 || ry == 0.0 || ((x1 - x2).abs() < f32::EPSILON && (y1 - y2).abs() < f32::EPSILON)
    {
        b.line_to(x2, y2);
        return;
    }

    let phi = x_axis_rotation.to_radians();
    let cos_phi = phi.cos();
    let sin_phi = phi.sin();
    let dx = (x1 - x2) / 2.0;
    let dy = (y1 - y2) / 2.0;
    let x1p = cos_phi * dx + sin_phi * dy;
    let y1p = -sin_phi * dx + cos_phi * dy;

    let lambda = x1p.powi(2) / rx.powi(2) + y1p.powi(2) / ry.powi(2);
    if lambda > 1.0 {
        let scale = lambda.sqrt();
        rx *= scale;
        ry *= scale;
    }

    let rx2 = rx.powi(2);
    let ry2 = ry.powi(2);
    let x1p2 = x1p.powi(2);
    let y1p2 = y1p.powi(2);
    let denom = rx2 * y1p2 + ry2 * x1p2;
    if denom == 0.0 {
        b.line_to(x2, y2);
        return;
    }
    let sign = if large_arc == sweep { -1.0 } else { 1.0 };
    let coef = sign
        * ((rx2 * ry2 - rx2 * y1p2 - ry2 * x1p2) / denom)
            .max(0.0)
            .sqrt();
    let cxp = coef * (rx * y1p / ry);
    let cyp = coef * (-ry * x1p / rx);
    let cx = cos_phi * cxp - sin_phi * cyp + (x1 + x2) / 2.0;
    let cy = sin_phi * cxp + cos_phi * cyp + (y1 + y2) / 2.0;

    let theta1 = angle_between(1.0, 0.0, (x1p - cxp) / rx, (y1p - cyp) / ry);
    let mut delta = angle_between(
        (x1p - cxp) / rx,
        (y1p - cyp) / ry,
        (-x1p - cxp) / rx,
        (-y1p - cyp) / ry,
    );
    if !sweep && delta > 0.0 {
        delta -= std::f32::consts::TAU;
    } else if sweep && delta < 0.0 {
        delta += std::f32::consts::TAU;
    }

    let segments = (delta.abs() / (std::f32::consts::FRAC_PI_2))
        .ceil()
        .max(1.0) as usize;
    let step = delta / segments as f32;
    for i in 0..segments {
        let t1 = theta1 + i as f32 * step;
        arc_segment_to_cubic(b, cx, cy, rx, ry, cos_phi, sin_phi, t1, t1 + step);
    }
}

fn angle_between(ux: f32, uy: f32, vx: f32, vy: f32) -> f32 {
    let dot = ux * vx + uy * vy;
    let len = ((ux * ux + uy * uy) * (vx * vx + vy * vy)).sqrt();
    let angle = (dot / len).clamp(-1.0, 1.0).acos();
    if ux * vy - uy * vx < 0.0 {
        -angle
    } else {
        angle
    }
}

fn arc_segment_to_cubic(
    b: &mut PathBuilder,
    cx: f32,
    cy: f32,
    rx: f32,
    ry: f32,
    cos_phi: f32,
    sin_phi: f32,
    t1: f32,
    t2: f32,
) {
    let delta = t2 - t1;
    let alpha = (4.0 / 3.0) * (delta / 4.0).tan();
    let (sin_t1, cos_t1) = t1.sin_cos();
    let (sin_t2, cos_t2) = t2.sin_cos();
    let p1 = (cos_t1 - alpha * sin_t1, sin_t1 + alpha * cos_t1);
    let p2 = (cos_t2 + alpha * sin_t2, sin_t2 - alpha * cos_t2);
    let p = (cos_t2, sin_t2);
    let map = |x: f32, y: f32| {
        (
            cx + rx * x * cos_phi - ry * y * sin_phi,
            cy + rx * x * sin_phi + ry * y * cos_phi,
        )
    };
    let (x1, y1) = map(p1.0, p1.1);
    let (x2, y2) = map(p2.0, p2.1);
    let (x, y) = map(p.0, p.1);
    b.cubic_to(x1, y1, x2, y2, x, y);
}
