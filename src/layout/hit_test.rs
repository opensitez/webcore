//! Hit testing: map screen coordinates ↔ text offsets.
//! Ported from C++ HitTest.cpp.

use crate::layout::inline_layout::collect_flat_text;
use crate::types::*;

// ─── Character-width approximation ───────────────────────────────────────────
// Matches the renderer's approx_text_width_ls — same coefficients.

fn approx_char_width(ch: char, font_px: f32) -> f32 {
    let base = font_px * 0.55;
    if "iIlj1!|:;,.'`".contains(ch) {
        base * 0.45
    } else if "mwMW".contains(ch) {
        base * 1.20
    } else if ch == ' ' {
        base * 0.35
    } else {
        base
    }
}

fn floor_char_boundary(s: &str, mut idx: usize) -> usize {
    while idx > 0 && !s.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

// ─── Measure a range [start, end) across inline runs ─────────────────────────

fn measure_run_range(
    text: &str,
    runs: &[InlineRun],
    start: usize,
    end: usize,
    extra_per_word: f32,
) -> f32 {
    if start >= end {
        return 0.0;
    }
    let mut w = 0.0f32;
    for run in runs {
        let seg_start = start.max(run.text_offset);
        let seg_end = end.min(run.text_offset + run.length);
        if seg_start >= seg_end {
            continue;
        }

        let font_px = run.style.font_size_px(16.0, 16.0);
        let letter_s = run.style.letter_spacing.resolve(font_px, 0.0, 16.0);
        let word_s = run.style.word_spacing.resolve(font_px, 0.0, 16.0);

        let s = floor_char_boundary(text, seg_start.min(text.len()));
        let e = floor_char_boundary(text, seg_end.min(text.len()));
        if s >= e {
            continue;
        }

        for ch in text[s..e].chars() {
            if ch == '\n' || ch == '\r' {
                continue;
            }
            w += approx_char_width(ch, font_px) + letter_s;
            if ch == ' ' {
                w += word_s + extra_per_word;
            }
        }
    }
    w
}

// ─── Caret X from offset ──────────────────────────────────────────────────────

/// Returns the X pixel position of the caret at `offset` within `line`.
/// `text` is the flat text of the containing box.
pub fn get_caret_x(text: &str, runs: &[InlineRun], line: &LayoutLine, offset: usize) -> f32 {
    let line_end = line.text_start + line.text_length;
    let offset = offset.min(line_end);

    // Fast path: use pre-computed char_x positions (shaped at the same physical
    // pixel size as the renderer, giving pixel-accurate caret placement).
    if !line.char_x.is_empty() {
        let idx = offset
            .saturating_sub(line.text_start)
            .min(line.char_x.len() - 1);
        return line.x + line.char_x[idx];
    }

    // BiDi path
    if !line.visual_segments.is_empty() {
        let mut x = line.x;
        for vs in &line.visual_segments {
            let seg_start = vs.logical_start;
            let seg_end = (vs.logical_start + vs.length).min(line_end);
            if seg_start >= seg_end {
                continue;
            }
            let is_rtl = (vs.level & 1) != 0;
            let seg_w =
                measure_run_range(text, runs, seg_start, seg_end, line.extra_space_per_word);
            if offset >= seg_start && offset <= seg_end {
                return if is_rtl {
                    x + measure_run_range(text, runs, offset, seg_end, line.extra_space_per_word)
                } else {
                    x + measure_run_range(text, runs, seg_start, offset, line.extra_space_per_word)
                };
            }
            x += seg_w;
        }
        return x;
    }

    // LTR path
    line.x
        + measure_run_range(
            text,
            runs,
            line.text_start,
            offset,
            line.extra_space_per_word,
        )
}

// ─── Offset from X ────────────────────────────────────────────────────────────

/// Returns the text byte-offset closest to `x` pixels within `line`.
pub fn get_offset_from_x(text: &str, runs: &[InlineRun], line: &LayoutLine, x: f32) -> usize {
    if x <= line.x {
        return line.text_start;
    }

    let line_end = line.text_start + line.text_length;

    // Fast path: use pre-computed char_x positions for pixel-accurate click mapping.
    // char_x[i] is the x position (relative to line.x) at byte offset line.text_start+i.
    if !line.char_x.is_empty() {
        let rel_x = x - line.x;
        let range_end = line.char_x.len() - 1; // last entry is end-of-line position
        let line_start = line.text_start;
        let measure_end = line_start + range_end;
        // Walk character boundaries, return where rel_x falls before the midpoint.
        let flat_slice_s = floor_char_boundary(text, line_start.min(text.len()));
        let flat_slice_e = floor_char_boundary(text, measure_end.min(text.len()));
        let mut byte_off = line_start;
        for ch in text[flat_slice_s..flat_slice_e].chars() {
            let i = byte_off - line_start;
            let next = byte_off + ch.len_utf8();
            let ni = (next - line_start).min(line.char_x.len() - 1);
            let x0 = line.char_x[i];
            let x1 = line.char_x[ni];
            if rel_x < x0 + (x1 - x0) / 2.0 {
                return byte_off;
            }
            byte_off = next;
        }
        return measure_end;
    }

    // Strip trailing newline from measurement
    let measure_end = if line_end > line.text_start
        && line_end <= text.len()
        && text.as_bytes().get(line_end - 1) == Some(&b'\n')
    {
        line_end - 1
    } else {
        line_end
    };

    let range_len = measure_end.saturating_sub(line.text_start);
    if range_len == 0 {
        return line.text_start;
    }

    // Build per-byte-offset character widths
    let mut char_widths = vec![0.0f32; range_len];

    for run in runs {
        let seg_start = line.text_start.max(run.text_offset);
        let seg_end = measure_end.min(run.text_offset + run.length);
        if seg_start >= seg_end {
            continue;
        }

        let font_px = run.style.font_size_px(16.0, 16.0);
        let letter_s = run.style.letter_spacing.resolve(font_px, 0.0, 16.0);
        let word_s = run.style.word_spacing.resolve(font_px, 0.0, 16.0);

        let s = floor_char_boundary(text, seg_start.min(text.len()));
        let e = floor_char_boundary(text, seg_end.min(text.len()));
        if s >= e {
            continue;
        }

        let mut byte_off = seg_start;
        for ch in text[s..e].chars() {
            if ch != '\n' && ch != '\r' {
                let idx = byte_off - line.text_start;
                if idx < range_len {
                    let mut cw = approx_char_width(ch, font_px) + letter_s;
                    if ch == ' ' {
                        cw += word_s;
                    }
                    char_widths[idx] = cw;
                }
            }
            byte_off += ch.len_utf8();
        }
    }

    // Add justified extra spacing on spaces
    if line.extra_space_per_word > 0.0 {
        let s = floor_char_boundary(text, line.text_start.min(text.len()));
        let e = floor_char_boundary(text, measure_end.min(text.len()));
        let mut byte_off = line.text_start;
        for ch in text[s..e].chars() {
            if ch == ' ' {
                let idx = byte_off - line.text_start;
                if idx < range_len {
                    char_widths[idx] += line.extra_space_per_word;
                }
            }
            byte_off += ch.len_utf8();
        }
    }

    let rel_x = x - line.x;

    // BiDi path
    if !line.visual_segments.is_empty() {
        let mut cur_x = 0.0f32;
        for vs in &line.visual_segments {
            let seg_start = vs.logical_start;
            let seg_end = (vs.logical_start + vs.length).min(measure_end);
            if seg_start < line.text_start || seg_start >= seg_end {
                continue;
            }
            let is_rtl = (vs.level & 1) != 0;
            let seg_w: f32 = (seg_start..seg_end)
                .map(|i| char_widths.get(i - line.text_start).copied().unwrap_or(0.0))
                .sum();

            if rel_x < cur_x + seg_w {
                let s = floor_char_boundary(text, seg_start.min(text.len()));
                let e = floor_char_boundary(text, seg_end.min(text.len()));
                if is_rtl {
                    let mut seg_x = cur_x + seg_w;
                    let mut byte_off = seg_start;
                    for ch in text[s..e].chars() {
                        let cw = char_widths
                            .get(byte_off - line.text_start)
                            .copied()
                            .unwrap_or(0.0);
                        seg_x -= cw;
                        if rel_x >= seg_x && rel_x < seg_x + cw {
                            return if rel_x < seg_x + cw / 2.0 {
                                byte_off + ch.len_utf8()
                            } else {
                                byte_off
                            };
                        }
                        byte_off += ch.len_utf8();
                    }
                    return seg_start;
                } else {
                    let mut seg_x = cur_x;
                    let mut byte_off = seg_start;
                    for ch in text[s..e].chars() {
                        let cw = char_widths
                            .get(byte_off - line.text_start)
                            .copied()
                            .unwrap_or(0.0);
                        if rel_x < seg_x + cw / 2.0 {
                            return byte_off;
                        }
                        seg_x += cw;
                        byte_off += ch.len_utf8();
                    }
                    return seg_end;
                }
            }
            cur_x += seg_w;
        }
        return measure_end;
    }

    // LTR path
    let mut cur_x = 0.0f32;
    let mut byte_off = line.text_start;
    let s = floor_char_boundary(text, line.text_start.min(text.len()));
    let e = floor_char_boundary(text, measure_end.min(text.len()));
    for ch in text[s..e].chars() {
        let cw = char_widths
            .get(byte_off - line.text_start)
            .copied()
            .unwrap_or(0.0);
        if rel_x < cur_x + cw / 2.0 {
            return byte_off;
        }
        cur_x += cw;
        byte_off += ch.len_utf8();
    }
    measure_end
}

// ─── Hit result ───────────────────────────────────────────────────────────────

/// Result of a hit test: identifies which box was hit and where within it.
pub struct HitResult {
    /// Stable node identity (survives tree mutations).
    pub node_id: u32,
    /// Byte offset within that box's flat text.
    pub local_offset: usize,
}

// ─── Recursive box hit test ───────────────────────────────────────────────────
//
// NOTE: In the Rust layout engine ALL coordinates (border_rect, content_rect,
// line.x, line.y) are in ABSOLUTE document space — unlike the C++ where they
// were relative to the parent's content area.  The hit test therefore works
// entirely in absolute coordinates and does NOT subtract content_rect offsets
// when recursing into children.

/// The point mapped into `node`'s own coordinate system.
///
/// css-transforms-1 §transform-rendering: a transform establishes a new local
/// coordinate system for the element and its descendants, and a point is
/// mapped into it by pre-multiplying with the INVERSE of the matrix. Testing
/// the raw document point against the untransformed `border_rect` made every
/// translated, rotated or scaled control unclickable where it is DRAWN — and
/// clickable where it is not.
///
/// ⛔ `vw`/`vh` inside a `translate()` resolve against a zero viewport here:
/// the hit-test entry points are public API that carries no viewport, and the
/// layout engine that has one is not on this path. Percentages — the reference
/// box — and `px`/`em` are exact. Recorded in `cssgaps.md`.
pub(crate) fn to_local(node: &WebCore, pt: (f32, f32)) -> (f32, f32) {
    if node.style.css_transform.ops.is_empty() {
        return pt;
    }
    let m = crate::renderer::display_list_builder::compute_transform_matrix(
        &node.style,
        &node.layout.border_rect,
        &crate::types::TransformCtx {
            font_px: node.style.font_size_px(16.0, 16.0),
            root_font_px: 16.0,
            viewport_w: 0.0,
            viewport_h: 0.0,
        },
    );
    let det = m[0] * m[3] - m[1] * m[2];
    // A singular matrix (`scale(0)`) collapses the box to nothing; there is no
    // point to map back, and the box cannot be hit.
    if det.abs() < 1e-6 {
        return pt;
    }
    let (x, y) = (pt.0 - m[4], pt.1 - m[5]);
    ((m[3] * x - m[2] * y) / det, (-m[1] * x + m[0] * y) / det)
}

fn hit_test_impl(node: &WebCore, doc_pt: (f32, f32), _button: u8) -> Option<HitResult> {
    if matches!(node.style.display, Display::None) {
        return None;
    }
    // `inert` blocks pointer interaction for the whole subtree (HTML §6.7).
    // Returning here rather than checking ancestors IS the inherited rule:
    // the walk is top-down, so a subtree it never enters is one no descendant
    // of can be hit.
    if node.attributes.contains_key("inert") {
        return None;
    }
    if !point_inside_clip_path(node, doc_pt.0, doc_pt.1) {
        return None;
    }

    // Adjust for this node's own scroll offset (rare — only scrollable boxes)
    let px = doc_pt.0 + node.layout.scroll_left;
    let py = doc_pt.1 + node.layout.scroll_top;

    // Note: inline-content hit test is performed after child checks so that
    // block children (e.g. buttons) receive hits even when the parent has
    // inline text nodes. See fallback handling below.

    let children_are_clipped = children_clipped_at(node, px, py);
    if !children_are_clipped {
        // Pass 0: positioned children with z-index > 0 (highest z-index first).
        // These paint on top of everything else and should receive hits first.
        // This handles CSS dropdowns (z-index:99999) that overlap sibling content.
        {
            let mut zi_children: Vec<(i32, usize)> = Vec::new();
            for (i, child) in node.children.iter().enumerate() {
                if child.attributes.contains_key("inert") {
                    continue;
                }
                if child.style.is_positioned() && child.style.z_index > 0 {
                    zi_children.push((child.style.z_index, i));
                }
            }
            if !zi_children.is_empty() {
                // Highest z-index first
                zi_children.sort_by(|a, b| b.0.cmp(&a.0));
                for &(_, idx) in &zi_children {
                    let child = &node.children[idx];
                    if child.tag == "::before" || child.tag == "::after" {
                        continue;
                    }
                    if child.layout.border_rect.h <= 0.0
                        && matches!(
                            child.style.overflow_y,
                            crate::types::Overflow::Hidden | crate::types::Overflow::Clip
                        )
                    {
                        continue;
                    }
                    let (cx, cy) = to_local(child, (px, py));
                    let b = &child.layout.border_rect;
                    if cx >= b.x && cx < b.x + b.w && cy >= b.y && cy < b.y + b.h {
                        if let Some(r) = hit_test_impl(child, (cx, cy), _button) {
                            return Some(r);
                        }
                        if point_inside_clip_path(child, cx, cy) {
                            return Some(HitResult {
                                node_id: child.node_id,
                                local_offset: 0,
                            });
                        }
                    }
                }
            }
        }

        // Pass 1: deepest child whose borderRect (absolute) contains the point
        for child in node.children.iter().rev() {
            // ⛔ Skipped HERE, not by the recursive call: each of these loops falls
            // back to `return Some(child.node_id)` when the recursion finds nothing
            // deeper, so a child that refused the hit would be returned anyway.
            if child.attributes.contains_key("inert") {
                continue;
            }
            // display:contents elements are transparent — recurse into their children directly
            if matches!(child.style.display, crate::types::Display::Contents) {
                if let Some(r) = hit_test_impl(child, (px, py), _button) {
                    return Some(r);
                }
                continue;
            }
            // Skip elements with 0 height and overflow:hidden — they're collapsed (e.g. hidden dropdowns)
            if child.layout.border_rect.h <= 0.0
                && matches!(
                    child.style.overflow_y,
                    crate::types::Overflow::Hidden | crate::types::Overflow::Clip
                )
            {
                continue;
            }
            // Skip ::before/::after pseudo-elements for hit testing — they're decorative
            if child.tag == "::before" || child.tag == "::after" {
                continue;
            }
            let (cx, cy) = to_local(child, (px, py));
            let b = &child.layout.border_rect;
            let in_border = cx >= b.x && cx < b.x + b.w && cy >= b.y && cy < b.y + b.h;
            if in_border || point_in_children_clip_area(child, cx, cy) {
                if let Some(r) = hit_test_impl(child, (cx, cy), _button) {
                    return Some(r);
                }
                if in_border && point_inside_clip_path(child, cx, cy) {
                    return Some(HitResult {
                        node_id: child.node_id,
                        local_offset: 0,
                    });
                }
            }
        }

        // Pass 2: children whose marginRect contains the point (gap / margin areas)
        for child in node.children.iter().rev() {
            // ⛔ Skipped HERE, not by the recursive call: each of these loops falls
            // back to `return Some(child.node_id)` when the recursion finds nothing
            // deeper, so a child that refused the hit would be returned anyway.
            if child.attributes.contains_key("inert") {
                continue;
            }
            let (cx, cy) = to_local(child, (px, py));
            let b = &child.layout.border_rect;
            let in_border = cx >= b.x && cx < b.x + b.w && cy >= b.y && cy < b.y + b.h;
            if in_border {
                continue;
            }
            let m = &child.layout.margin_rect;
            if cx >= m.x && cx < m.x + m.w && cy >= m.y && cy < m.y + m.h {
                if let Some(r) = hit_test_impl(child, (cx, cy), _button) {
                    return Some(r);
                }
            }
        }

        // Pass 3: X-range only — handles margin-collapse overflow
        for child in node.children.iter().rev() {
            // ⛔ Skipped HERE, not by the recursive call: each of these loops falls
            // back to `return Some(child.node_id)` when the recursion finds nothing
            // deeper, so a child that refused the hit would be returned anyway.
            if child.attributes.contains_key("inert") {
                continue;
            }
            let (cx, cy) = to_local(child, (px, py));
            let m = &child.layout.margin_rect;
            let in_margin = cx >= m.x && cx < m.x + m.w && cy >= m.y && cy < m.y + m.h;
            if in_margin {
                continue;
            }
            let b = &child.layout.border_rect;
            if cx >= b.x && cx < b.x + b.w {
                if let Some(r) = hit_test_impl(child, (cx, cy), _button) {
                    return Some(r);
                }
            }
        }
    }

    // Fallback: If no children hit, but this node contains the point
    let b = &node.layout.border_rect;
    if px >= b.x && px < b.x + b.w && py >= b.y && py < b.y + b.h {
        // If this node has inline content, return the inline hit offset
        // (caret/text hit). Otherwise select the node itself.
        if !node.layout.line_cache.is_empty() {
            let flat = collect_flat_text(node);
            let line = snap_to_line(&node.layout.line_cache, py);
            let off = get_offset_from_x(&flat, &node.layout.inline_runs, line, px);
            return Some(HitResult {
                node_id: node.node_id,
                local_offset: off,
            });
        }
        return Some(HitResult {
            node_id: node.node_id,
            local_offset: 0,
        });
    }

    None
}

fn children_clipped_at(node: &WebCore, px: f32, py: f32) -> bool {
    let Some((left, right, top, bottom)) = children_clip_bounds(node) else {
        return false;
    };
    px < left || px >= right || py < top || py >= bottom
}

fn point_in_children_clip_area(node: &WebCore, px: f32, py: f32) -> bool {
    children_clip_bounds(node)
        .map(|(left, right, top, bottom)| px >= left && px < right && py >= top && py < bottom)
        .unwrap_or(false)
}

fn children_clip_bounds(node: &WebCore) -> Option<(f32, f32, f32, f32)> {
    let clip_x = matches!(
        node.style.overflow_x,
        Overflow::Hidden | Overflow::Clip | Overflow::Scroll | Overflow::Auto
    );
    let clip_y = matches!(
        node.style.overflow_y,
        Overflow::Hidden | Overflow::Clip | Overflow::Scroll | Overflow::Auto
    );
    if !clip_x && !clip_y {
        return None;
    }
    let clip_margin = overflow_clip_margin_px(node);
    let margin_x = if matches!(node.style.overflow_x, Overflow::Clip) {
        clip_margin
    } else {
        0.0
    };
    let margin_y = if matches!(node.style.overflow_y, Overflow::Clip) {
        clip_margin
    } else {
        0.0
    };
    let p = &node.layout.padding_rect;
    let left = if clip_x {
        p.x - margin_x
    } else {
        f32::NEG_INFINITY
    };
    let right = if clip_x {
        p.x + p.w + margin_x
    } else {
        f32::INFINITY
    };
    let top = if clip_y {
        p.y - margin_y
    } else {
        f32::NEG_INFINITY
    };
    let bottom = if clip_y {
        p.y + p.h + margin_y
    } else {
        f32::INFINITY
    };
    Some((left, right, top, bottom))
}

fn overflow_clip_margin_px(node: &WebCore) -> f32 {
    if !matches!(node.style.overflow_x, Overflow::Clip)
        && !matches!(node.style.overflow_y, Overflow::Clip)
    {
        return 0.0;
    }
    let font_px = node.style.font_size_px(16.0, 16.0);
    node.style
        .overflow_clip_margin
        .split_whitespace()
        .find_map(crate::css::parse_length_checked)
        .map(|length| {
            length
                .resolve(font_px, node.layout.padding_rect.w, 16.0)
                .max(0.0)
        })
        .unwrap_or(0.0)
}

fn point_inside_clip_path(node: &WebCore, x: f32, y: f32) -> bool {
    let b = &node.layout.border_rect;
    let font_px = node.style.font_size_px(16.0, 16.0);
    match node.style.clip_path.kind {
        ClipPathKind::None => true,
        ClipPathKind::Inset => {
            let top = node.style.clip_path.inset_top.resolve(font_px, b.h, 16.0);
            let right = node.style.clip_path.inset_right.resolve(font_px, b.w, 16.0);
            let bottom = node
                .style
                .clip_path
                .inset_bottom
                .resolve(font_px, b.h, 16.0);
            let left = node.style.clip_path.inset_left.resolve(font_px, b.w, 16.0);
            x >= b.x + left && x < b.x + b.w - right && y >= b.y + top && y < b.y + b.h - bottom
        }
        ClipPathKind::Circle => {
            let reference = b.w.min(b.h);
            let r = node
                .style
                .clip_path
                .circle_radius
                .resolve(font_px, reference, 16.0)
                .max(0.0);
            let cx = b.x + node.style.clip_path.center_x.resolve(font_px, b.w, 16.0);
            let cy = b.y + node.style.clip_path.center_y.resolve(font_px, b.h, 16.0);
            let dx = x - cx;
            let dy = y - cy;
            dx * dx + dy * dy <= r * r
        }
        ClipPathKind::Ellipse => {
            let rx = node
                .style
                .clip_path
                .ellipse_rx
                .resolve(font_px, b.w, 16.0)
                .max(0.0);
            let ry = node
                .style
                .clip_path
                .ellipse_ry
                .resolve(font_px, b.h, 16.0)
                .max(0.0);
            if rx <= 0.0 || ry <= 0.0 {
                return false;
            }
            let cx = b.x + node.style.clip_path.center_x.resolve(font_px, b.w, 16.0);
            let cy = b.y + node.style.clip_path.center_y.resolve(font_px, b.h, 16.0);
            let nx = (x - cx) / rx;
            let ny = (y - cy) / ry;
            nx * nx + ny * ny <= 1.0
        }
        ClipPathKind::Polygon => point_inside_clip_polygon(node, x, y, font_px),
    }
}

fn point_inside_clip_polygon(node: &WebCore, x: f32, y: f32, font_px: f32) -> bool {
    let b = node.layout.border_rect;
    let points = &node.style.clip_path.points;
    if points.len() < 3 {
        return false;
    }

    let px = x - b.x;
    let py = y - b.y;
    let mut inside = false;
    let mut prev = points.len() - 1;
    for i in 0..points.len() {
        let (xi, yi) = (
            points[i].0.resolve(font_px, b.w, 16.0),
            points[i].1.resolve(font_px, b.h, 16.0),
        );
        let (xj, yj) = (
            points[prev].0.resolve(font_px, b.w, 16.0),
            points[prev].1.resolve(font_px, b.h, 16.0),
        );
        if (yi > py) != (yj > py) && (yj - yi).abs() > f32::EPSILON {
            let edge_x = (xj - xi) * (py - yi) / (yj - yi) + xi;
            if px < edge_x {
                inside = !inside;
            }
        }
        prev = i;
    }
    inside
}

fn snap_to_line(lines: &[LayoutLine], y: f32) -> &LayoutLine {
    if lines.is_empty() {
        panic!("snap_to_line called with empty lines");
    }

    let mut lo = 0usize;
    let mut hi = lines.len() - 1;

    while lo <= hi {
        let mid = (lo + hi) / 2;
        let l = &lines[mid];

        if y < l.y {
            if mid == 0 {
                return &lines[0];
            }
            hi = mid - 1;
        } else if y >= l.y + l.height {
            lo = mid + 1;
        } else {
            return l;
        }
    }

    // Snap to the closest line if we're between lines
    if lo >= lines.len() {
        return lines.last().unwrap();
    }
    &lines[lo]
}

// ─── Public API ───────────────────────────────────────────────────────────────

/// Map a document-space (x, y) point to the box and byte-offset it hits.
///
/// The point is relative to the document origin (top-left of viewport content),
/// before any scroll offset is applied — i.e. `(mouse_x + scroll_x, mouse_y + scroll_y)`.
pub fn point_to_hit(root: &WebCore, doc_pt: (f32, f32), button: u8) -> Option<HitResult> {
    // Coordinates are absolute — pass directly (root.layout.content_rect is always 0,0).
    // The root's OWN transform has no parent to undo it, so it is undone here.
    hit_test_impl(root, to_local(root, doc_pt), button)
}

/// Map a (node_id, local_byte_offset) back to a document-space (x, y) point
/// for caret or selection-anchor rendering.
///
/// Pass `scroll_x = 0, scroll_y = 0` to get absolute document coordinates.
pub fn offset_to_point(
    root: &WebCore,
    target_id: u32,
    local_offset: usize,
    scroll_x: f32,
    scroll_y: f32,
) -> Option<(f32, f32)> {
    find_and_measure(root, target_id, local_offset, scroll_x, scroll_y)
}

fn find_and_measure(
    node: &WebCore,
    target_id: u32,
    local_offset: usize,
    scroll_x: f32,
    scroll_y: f32,
) -> Option<(f32, f32)> {
    if node.node_id == target_id {
        let flat = collect_flat_text(node);
        return Some(caret_point_in_box(
            &flat,
            &node.layout.inline_runs,
            &node.layout.line_cache,
            local_offset,
            scroll_x,
            scroll_y,
        ));
    }

    // If this box is a block container with inline content (has line_cache),
    // check whether the target is an inline descendant laid out here.
    if !node.layout.line_cache.is_empty() {
        let mut acc: usize = 0;
        if let Some(abs_off) = inline_offset_of_by_id(node, target_id, local_offset, &mut acc) {
            let flat = collect_flat_text(node);
            return Some(caret_point_in_box(
                &flat,
                &node.layout.inline_runs,
                &node.layout.line_cache,
                abs_off,
                scroll_x,
                scroll_y,
            ));
        }
    }

    for child in &node.children {
        if let Some(pt) = find_and_measure(child, target_id, local_offset, scroll_x, scroll_y) {
            return Some(pt);
        }
    }
    None
}

/// Like inline_offset_of but matches by node_id instead of pointer.
fn inline_offset_of_by_id(
    node: &WebCore,
    target_id: u32,
    local_offset: usize,
    acc: &mut usize,
) -> Option<usize> {
    if node.node_id == target_id {
        return Some(*acc + local_offset);
    }
    if matches!(
        node.style.display,
        Display::InlineBlock | Display::InlineFlex | Display::InlineGrid
    ) {
        return None;
    }
    if matches!(node.style.display, Display::None) {
        return None;
    }
    if node.is_text_node() {
        *acc += node.text.len();
        return None;
    }
    if !node.text.is_empty() {
        *acc += node.text.len();
    }
    for child in &node.children {
        if let Some(r) = inline_offset_of_by_id(child, target_id, local_offset, acc) {
            return Some(r);
        }
    }
    None
}

fn caret_point_in_box(
    flat: &str,
    runs: &[InlineRun],
    lines: &[LayoutLine],
    local_offset: usize,
    scroll_x: f32,
    scroll_y: f32,
) -> (f32, f32) {
    // line.x / line.y are absolute document coordinates
    for line in lines {
        let line_end = line.text_start + line.text_length;
        if local_offset >= line.text_start && local_offset <= line_end {
            let cx = get_caret_x(flat, runs, line, local_offset);
            return (cx - scroll_x, line.y - scroll_y);
        }
    }
    if let Some(last) = lines.last() {
        let cx = get_caret_x(flat, runs, last, local_offset);
        return (cx - scroll_x, last.y - scroll_y);
    }
    (0.0, 0.0)
}

/// Find the deepest box at a document-space point. Returns node_id.
pub fn hit_test_box_at(root: &WebCore, doc_pt: (f32, f32), button: u8) -> u32 {
    deepest_box_at(root, to_local(root, doc_pt), button).unwrap_or(root.node_id)
}

fn deepest_box_at(node: &WebCore, pt: (f32, f32), _button: u8) -> Option<u32> {
    if matches!(node.style.display, Display::None) {
        return None;
    }
    if !point_inside_clip_path(node, pt.0, pt.1) {
        return None;
    }
    // ⛔ The SECOND tree walker — `hit_test_impl` above is not the only road
    // in, and a guard on one of them is invisible to every test that drives
    // the other. The check is per-CHILD in the loop below rather than here:
    // this function's only caller answers `unwrap_or(root.node_id)`, so a
    // guard at this point cannot change what an inert root returns.
    let (px, py) = (
        pt.0 + node.layout.scroll_left,
        pt.1 + node.layout.scroll_top,
    );
    if children_clipped_at(node, px, py) {
        return None;
    }
    for child in node.children.iter().rev() {
        // ⛔ Skipped HERE, not by the recursive call: each of these loops falls
        // back to `return Some(child.node_id)` when the recursion finds nothing
        // deeper, so a child that refused the hit would be returned anyway.
        if child.attributes.contains_key("inert") {
            continue;
        }
        // The second walker needs the same mapping — see `to_local`.
        let (cx, cy) = to_local(child, (px, py));
        let m = &child.layout.margin_rect;
        let in_margin = cx >= m.x && cx < m.x + m.w && cy >= m.y && cy < m.y + m.h;
        if in_margin || point_in_children_clip_area(child, cx, cy) {
            if let Some(r) = deepest_box_at(child, (cx, cy), _button) {
                return Some(r);
            }
            if in_margin {
                return Some(child.node_id);
            }
        }
    }
    None
}

/// Find a link URL at a document-space point, if any.
pub fn hit_test_link(root: &WebCore, doc_pt: (f32, f32), button: u8) -> Option<String> {
    let doc_pt = to_local(root, doc_pt);
    // 1. Try hitting text content (inline runs)
    if let Some(hit) = hit_test_impl(root, doc_pt, button) {
        fn find_node(node: &WebCore, id: u32) -> Option<&WebCore> {
            if node.node_id == id {
                return Some(node);
            }
            for child in &node.children {
                if let Some(f) = find_node(child, id) {
                    return Some(f);
                }
            }
            None
        }
        if let Some(node) = find_node(root, hit.node_id) {
            for run in &node.layout.inline_runs {
                if hit.local_offset >= run.text_offset
                    && hit.local_offset < run.text_offset + run.length
                {
                    if !run.style.href.is_empty() {
                        return Some(run.style.href.clone());
                    }
                }
            }
        }
    }

    // 2. Fallback: find the deepest box and search up for 'href' attribute
    let target_id = hit_test_box_at(root, doc_pt, button);
    if target_id == 0 {
        return None;
    }
    find_href_up_by_id(root, target_id)
}

fn find_href_up_by_id(root: &WebCore, target_id: u32) -> Option<String> {
    fn walk(node: &WebCore, target_id: u32, path: &mut Vec<u32>) -> bool {
        path.push(node.node_id);
        if node.node_id == target_id {
            return true;
        }
        for child in &node.children {
            if walk(child, target_id, path) {
                return true;
            }
        }
        path.pop();
        false
    }
    let mut path = Vec::new();
    if !walk(root, target_id, &mut path) {
        return None;
    }
    // Walk path in reverse (target → root) looking for href
    for &nid in path.iter().rev() {
        fn find_node(node: &WebCore, id: u32) -> Option<&WebCore> {
            if node.node_id == id {
                return Some(node);
            }
            for child in &node.children {
                if let Some(f) = find_node(child, id) {
                    return Some(f);
                }
            }
            None
        }
        if let Some(node) = find_node(root, nid) {
            if let Some(href) = node.attributes.get("href") {
                if !href.is_empty() {
                    return Some(href.clone());
                }
            }
        }
    }
    None
}
