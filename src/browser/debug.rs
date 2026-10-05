//! Permanent browser instrumentation and comparison transport.
use super::*;

// ─── Remote debug server ──────────────────────────────────────────────────────
// All commands mirror debugserver.rs but operate on the active tab's document.

/// Escape a string for JSON output.
pub(super) fn dbg_walk_composed<'a, F: FnMut(&'a crate::WebCore)>(
    node: &'a crate::WebCore,
    f: &mut F,
) {
    f(node);
    for child in node.effective_children() {
        dbg_walk_composed(child, f);
    }
}

pub(super) fn dbg_composed_nodes_for_ids<'a>(
    root: &'a crate::WebCore,
    ids: &std::collections::HashSet<u32>,
) -> Vec<&'a crate::WebCore> {
    let mut out = Vec::new();
    dbg_walk_composed(root, &mut |node| {
        if node.node_id != 0 && ids.contains(&node.node_id) {
            out.push(node);
        }
    });
    out
}

pub(super) fn dbg_select_composed_with_pseudo<'a>(
    root: &'a crate::WebCore,
    doc: &Document,
    sel: &str,
) -> Vec<&'a crate::WebCore> {
    for tag in ["::before", "::after"] {
        if let Some(base) = sel.trim().strip_suffix(tag) {
            let base_ids: std::collections::HashSet<u32> =
                doc.query_selector_all(base.trim()).into_iter().collect();
            return dbg_composed_nodes_for_ids(root, &base_ids)
                .into_iter()
                .filter_map(|p| p.children.iter().find(|c| c.tag == tag))
                .collect();
        }
    }
    let ids: std::collections::HashSet<u32> = doc.query_selector_all(sel).into_iter().collect();
    dbg_composed_nodes_for_ids(root, &ids)
}

pub(super) fn dbg_get_composed_by_id<'a>(
    root: &'a crate::WebCore,
    node_id: u32,
) -> Option<&'a crate::WebCore> {
    let mut result = None;
    dbg_walk_composed(root, &mut |node| {
        if result.is_none() && node.node_id == node_id {
            result = Some(node);
        }
    });
    result
}

pub(super) fn dbg_json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c < '\x20' => out.push_str(&format!("\\u{:04x}", c as u32)),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

pub(super) fn profile_json(limit: usize) -> String {
    if !crate::profile::is_enabled() {
        return r#"{"ok":false,"error":"start browser with --profile"}"#.to_string();
    }
    let snapshot = crate::profile::snapshot();
    let phases: Vec<_> = crate::profile::Phase::ALL
        .iter()
        .filter_map(|phase| {
            let timing = snapshot.timings[*phase as usize];
            (timing.count > 0).then(|| {
                format!(
                    r#"{{"name":{},"count":{},"total_ms":{:.3},"max_ms":{:.3}}}"#,
                    dbg_json_escape(phase.name()),
                    timing.count,
                    timing.total_ns as f64 / 1_000_000.0,
                    timing.max_ns as f64 / 1_000_000.0,
                )
            })
        })
        .collect();
    let resources: Vec<_> = snapshot
        .resources
        .iter()
        .take(limit.min(128))
        .map(|resource| {
            format!(
                r#"{{"kind":{},"url":{},"source":{},"bytes":{},"ms":{:.3}}}"#,
                dbg_json_escape(resource.kind),
                dbg_json_escape(&resource.url),
                dbg_json_escape(resource.source),
                resource.bytes,
                resource.duration_ns as f64 / 1_000_000.0,
            )
        })
        .collect();
    format!(
        r#"{{"ok":true,"elapsed_ms":{:.3},"phases":[{}],"resources":[{}]}}"#,
        snapshot.elapsed.as_secs_f64() * 1000.0,
        phases.join(","),
        resources.join(","),
    )
}

pub(super) fn dbg_json_str(json: &str, key: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    value.get(key)?.as_str().map(str::to_owned)
}

fn dbg_key(key: &str) -> Option<(u32, Option<char>)> {
    Some(match key {
        "Enter" => (13, Some('\r')),
        "Tab" => (9, Some('\t')),
        "Backspace" => (8, None),
        "Delete" => (46, None),
        "Escape" => (27, None),
        "ArrowLeft" => (37, None),
        "ArrowRight" => (39, None),
        "ArrowUp" => (38, None),
        "ArrowDown" => (40, None),
        "Home" => (36, None),
        "End" => (35, None),
        "Space" => (32, Some(' ')),
        key => {
            let mut chars = key.chars();
            let ch = chars.next()?;
            if chars.next().is_some() {
                return None;
            }
            (ch as u32, Some(ch))
        }
    })
}

fn dbg_send_key(view: &mut crate::BrowserView, request: &str) -> String {
    let Some(key) = dbg_json_str(request, "key") else {
        return r#"{"ok":false,"error":"key needs key name"}"#.into();
    };
    let Some((code, ch)) = dbg_key(&key) else {
        return serde_json::json!({"ok":false,"error":format!("unknown key: {key}")}).to_string();
    };
    let phase = dbg_json_str(request, "phase");
    let events: &[crate::dom::HtmlEventType] = match phase.as_deref().unwrap_or("press") {
        "press" => &[
            crate::dom::HtmlEventType::KeyDown,
            crate::dom::HtmlEventType::KeyUp,
        ],
        "down" => &[crate::dom::HtmlEventType::KeyDown],
        "up" => &[crate::dom::HtmlEventType::KeyUp],
        _ => return r#"{"ok":false,"error":"key phase must be press, down or up"}"#.into(),
    };
    let mut changed = false;
    for &event in events {
        changed |= view.handle_key(
            event,
            code,
            ch,
            dbg_json_bool(request, "ctrl").unwrap_or(false),
            dbg_json_bool(request, "shift").unwrap_or(false),
            dbg_json_bool(request, "alt").unwrap_or(false),
            dbg_json_bool(request, "meta").unwrap_or(false),
        );
    }
    serde_json::json!({"ok":true,"changed":changed}).to_string()
}

fn chrome_key_params(request: &str, key_up: bool) -> Result<String, String> {
    // CDP Input.dispatchKeyEvent defines these modifier masks.
    const ALT: u32 = 1;
    const CONTROL: u32 = 2;
    const META: u32 = 4;
    const SHIFT: u32 = 8;
    let key = dbg_json_str(request, "key").ok_or("chrome-key needs key name")?;
    let (code, ch) = dbg_key(&key).ok_or_else(|| format!("unknown key: {key}"))?;
    let mut modifiers = 0;
    for (name, mask) in [
        ("alt", ALT),
        ("ctrl", CONTROL),
        ("meta", META),
        ("shift", SHIFT),
    ] {
        if dbg_json_bool(request, name).unwrap_or(false) {
            modifiers |= mask;
        }
    }
    let code = ch
        .filter(char::is_ascii_alphabetic)
        .map_or(code, |ch| ch.to_ascii_uppercase() as u32);
    let key = if key == "Space" { " ".to_owned() } else { key };
    let mut params = serde_json::json!({
        "type": if key_up { "keyUp" } else { "keyDown" },
        "key": key,
        "modifiers": modifiers,
        "windowsVirtualKeyCode": if code <= u8::MAX as u32 { code } else { 0 },
    });
    if !key_up
        && modifiers & (CONTROL | META | ALT) == 0
        && let Some(ch) = ch
    {
        params["text"] = serde_json::Value::String(ch.to_string());
    }
    Ok(params.to_string())
}

#[cfg(test)]
mod command_field_tests {
    #[test]
    fn key_command_supports_press_and_held_key_phases() {
        let mut doc = crate::parse_html("<input id=check type=checkbox>");
        let id = doc.get_element_by_id("check").unwrap();
        doc.focus(id);
        let mut view = crate::BrowserView::new(360.0, 180.0, crate::PageLoadOptions::default());
        view.attach_document(crate::browser::BrowserDocument::new(doc));
        super::dbg_send_key(&mut view, r#"{"key":"Space","phase":"down"}"#);
        assert!(!view.document().unwrap().get_node(id).unwrap().checkedness);
        assert_eq!(view.document().unwrap().active_box, id);
        super::dbg_send_key(&mut view, r#"{"key":"Space","phase":"up"}"#);
        assert!(view.document().unwrap().get_node(id).unwrap().checkedness);
        super::dbg_send_key(&mut view, r#"{"key":"Space"}"#);
        assert!(!view.document().unwrap().get_node(id).unwrap().checkedness);
        let invalid: serde_json::Value = serde_json::from_str(&super::dbg_send_key(
            &mut view,
            r#"{"key":"Space","phase":"invalid"}"#,
        ))
        .unwrap();
        assert_eq!(invalid["ok"], false);
        assert!(!view.document().unwrap().get_node(id).unwrap().checkedness);
        let state: serde_json::Value = serde_json::from_str(&super::dbg_control_state(
            &view.document().unwrap(),
            "#check",
        ))
        .unwrap();
        assert_eq!(state["controls"][0]["checked"], false);
    }
    #[test]
    fn chrome_key_encodes_modifiers_and_releases_without_text() {
        let request = r#"{"key":"a","ctrl":true,"shift":true,"meta":true}"#;
        let down: serde_json::Value =
            serde_json::from_str(&super::chrome_key_params(request, false).unwrap()).unwrap();
        assert_eq!(down["modifiers"], 14);
        assert_eq!(down["windowsVirtualKeyCode"], 65);
        assert!(down.get("text").is_none());
        let up: serde_json::Value =
            serde_json::from_str(&super::chrome_key_params(r#"{"key":"Space"}"#, true).unwrap())
                .unwrap();
        assert_eq!(up["type"], "keyUp");
        assert!(up.get("text").is_none());
        assert_eq!(super::dbg_key("م"), Some(('م' as u32, Some('م'))));
    }
    #[test]
    fn control_state_reports_live_values_and_utf16_selection() {
        let mut doc = crate::parse_html("<input id=t value=default>");
        let id = doc.get_element_by_id("t").unwrap();
        let node = doc.get_box_by_id_mut(id).unwrap();
        node.value_state = Some("a\u{1f600}b".into());
        node.input_sel_anchor = 1;
        node.input_cursor = 2;
        doc.focused_box = id;
        let state: serde_json::Value =
            serde_json::from_str(&super::dbg_control_state(&doc, "#t")).unwrap();
        let control = &state["controls"][0];
        assert_eq!(control["value"], "a\u{1f600}b");
        assert_eq!(control["selection_start"], 1);
        assert_eq!(control["selection_end"], 3);
        assert_eq!(control["focused"], true);
        assert!(control["picker"].is_null());
    }

    #[test]
    fn control_state_reports_browsed_picker_geometry_without_changing_value() {
        let mut renderer = crate::Renderer::new();
        let mut doc = renderer.load_html("<input id=t type=month value=2026-08>", 400.0);
        let id = doc.get_element_by_id("t").unwrap();
        doc.open_picker = id;
        doc.picker_calendar = Some((2027, 9));
        let state: serde_json::Value =
            serde_json::from_str(&super::dbg_control_state(&doc, "#t")).unwrap();
        let control = &state["controls"][0];
        assert_eq!(control["value"], "2026-08");
        assert_eq!(control["picker"]["kind"], "month");
        assert_eq!(control["picker"]["calendar"]["year"], 2027);
        assert_eq!(control["picker"]["calendar"]["month"], 9);
        assert_eq!(
            control["picker"]["height"].as_f64().unwrap(),
            f64::from(crate::widgets::MonthGrid::height())
        );
    }

    #[test]
    fn string_fields_are_not_confused_with_command_values() {
        for command in [
            r#"{"cmd":"key","key":"Enter"}"#,
            r#"{"key":"Enter","cmd":"key"}"#,
        ] {
            assert_eq!(
                super::dbg_json_str(command, "key").as_deref(),
                Some("Enter")
            );
        }
        assert_eq!(
            super::dbg_json_str(r#"{"cmd":"type","text":"\u0645\n\t\""}"#, "text").as_deref(),
            Some("م\n\t\"")
        );
    }
}

pub(super) fn dbg_json_num(json: &str, key: &str) -> Option<f32> {
    let needle = format!("\"{}\"", key);
    let pos = json.find(&needle)?;
    let after = &json[pos + needle.len()..];
    let after = after.trim_start().strip_prefix(':')?;
    let after = after.trim_start();
    let end = after
        .find(|c: char| !c.is_ascii_digit() && c != '.' && c != '-')
        .unwrap_or(after.len());
    after[..end].parse().ok()
}

pub(super) fn dbg_json_bool(json: &str, key: &str) -> Option<bool> {
    let needle = format!("\"{}\"", key);
    let pos = json.find(&needle)?;
    let after = &json[pos + needle.len()..];
    let after = after.trim_start().strip_prefix(':')?;
    let after = after.trim_start();
    if after.starts_with("true") {
        Some(true)
    } else if after.starts_with("false") {
        Some(false)
    } else {
        dbg_json_num(json, key).map(|value| value != 0.0)
    }
}

/// Read live form state rather than the reset defaults stored in attributes.
fn dbg_control_state(doc: &Document, selector: &str) -> String {
    let controls: Vec<_> = doc
        .query_selector_all(selector)
        .into_iter()
        .filter_map(|id| {
            let node = doc.get_node(id)?;
            let value = crate::types::input_value(node);
            let picker = if doc.open_picker == id {
                doc.picker_rect(id).map(|(x, y, width, height)| {
                    let kind = doc.picker_kind(id);
                    let calendar = matches!(
                        kind,
                        Some(crate::types::PickerKind::Calendar | crate::types::PickerKind::Month)
                    )
                    .then(|| {
                        let (year, month, _) = doc.picker_month(id);
                        serde_json::json!({"year":year,"month":month})
                    });
                    serde_json::json!({"kind":match kind {
                        Some(crate::types::PickerKind::Calendar) => "calendar",
                        Some(crate::types::PickerKind::Month) => "month",
                        Some(crate::types::PickerKind::Time) => "time",
                        Some(crate::types::PickerKind::Color) => "color",
                        None => "unknown",
                    },"x":x,"y":y,"width":width,"height":height,"calendar":calendar,
                    "time":doc.picker_time.as_ref().map(|draft| serde_json::json!({"value":draft.value(),"active_part":draft.active}))})
                })
            } else {
                None
            };
            let utf16_offset = |chars: usize| {
                value
                    .chars()
                    .take(chars)
                    .map(char::len_utf16)
                    .sum::<usize>()
            };
            Some(serde_json::json!({
                "node_id": id,
                "tag": node.tag,
                "value": value,
                "cursor_char": node.input_cursor,
                "anchor_char": node.input_sel_anchor,
                "selection_start": utf16_offset(node.input_cursor.min(node.input_sel_anchor)),
                "selection_end": utf16_offset(node.input_cursor.max(node.input_sel_anchor)),
                "focused": doc.focused_box == id,
                "disabled": doc.is_actually_disabled(id),
                "checked": node.checkedness,
                "indeterminate": node.data.get("indeterminate").is_some_and(|value| value == "true"),
                "scroll_left": node.layout.scroll_left,
                "scroll_top": node.layout.scroll_top,
                "caret_visible": doc.editor.caret_visible,
                "picker": picker,
            }))
        })
        .collect();
    serde_json::json!({"ok": true, "controls": controls}).to_string()
}

/// Node ids matching `query`, resolved with the engine's own selector engine.
pub(super) fn dbg_query_ids(doc: &Document, query: &str) -> std::collections::HashSet<u32> {
    doc.query_selector_all(query).into_iter().collect()
}

thread_local! {
    /// Last (selector, matching node ids) — see `dbg_matches_query`.
    static DBG_QUERY_MEMO: std::cell::RefCell<Option<(String, std::collections::HashSet<u32>)>> =
        const { std::cell::RefCell::new(None) };
}

/// Does `node` match `query`?
///
/// The fast path below only understands `tag#id.class`. Anything else — a
/// combinator, a pseudo-class, an attribute selector — is handed to the ENGINE's
/// selector matcher via `query_selector_all`, because this hand-rolled parser
/// used to answer `false` for those instead of admitting it could not tell:
/// `{"cmd":"find","selector":"table tbody"}` reported `count: 0` on a document
/// that plainly had one, which reads exactly like a missing element. A debug
/// tool that lies about the DOM is worse than no debug tool.
///
/// One-entry memo because every caller walks the whole tree with a single fixed
/// selector — without it this would be O(n²) per command.
pub(super) fn dbg_matches_query(doc: &Document, node: &crate::WebCore, query: &str) -> bool {
    if node.tag == "#text" {
        return false;
    }
    let query = query.trim();
    if query.contains([' ', '>', '+', '~', ':', '[', ',']) {
        if node.node_id == 0 {
            return false;
        }
        return DBG_QUERY_MEMO.with(|memo| {
            let mut memo = memo.borrow_mut();
            let stale = memo.as_ref().map(|(q, _)| q != query).unwrap_or(true);
            if stale {
                let ids: std::collections::HashSet<u32> =
                    doc.query_selector_all(query).into_iter().collect();
                *memo = Some((query.to_string(), ids));
            }
            memo.as_ref().unwrap().1.contains(&node.node_id)
        });
    }
    let mut tag_q = "";
    let mut id_q = "";
    let mut classes_q: Vec<&str> = Vec::new();
    let mut rest = query;
    if !rest.starts_with('#') && !rest.starts_with('.') {
        let end = rest
            .find(|c: char| c == '#' || c == '.')
            .unwrap_or(rest.len());
        tag_q = &rest[..end];
        rest = &rest[end..];
    }
    while !rest.is_empty() {
        if rest.starts_with('#') {
            rest = &rest[1..];
            let end = rest
                .find(|c: char| c == '#' || c == '.')
                .unwrap_or(rest.len());
            id_q = &rest[..end];
            rest = &rest[end..];
        } else if rest.starts_with('.') {
            rest = &rest[1..];
            let end = rest
                .find(|c: char| c == '#' || c == '.')
                .unwrap_or(rest.len());
            classes_q.push(&rest[..end]);
            rest = &rest[end..];
        } else {
            break;
        }
    }
    if !tag_q.is_empty() && !node.tag.eq_ignore_ascii_case(tag_q) {
        return false;
    }
    if !id_q.is_empty() {
        if node.attributes.get("id").map(|s| s.as_str()) != Some(id_q) {
            return false;
        }
    }
    if !classes_q.is_empty() {
        let cls = node
            .attributes
            .get("class")
            .map(|s| s.as_str())
            .unwrap_or("");
        let elem_classes: Vec<&str> = cls.split_whitespace().collect();
        for c in &classes_q {
            if !elem_classes.contains(c) {
                return false;
            }
        }
    }
    true
}

pub(super) fn dbg_collect_text(node: &crate::WebCore, out: &mut String) {
    if node.tag == "#text" {
        if !out.is_empty() && !out.ends_with(' ') {
            out.push(' ');
        }
        out.push_str(node.text.trim());
    }
    for child in &node.children {
        dbg_collect_text(child, out);
    }
}

pub(super) fn dbg_inspect_json(node: &crate::WebCore) -> String {
    let s = &node.style;
    let id = node.attributes.get("id").map(|v| v.as_str()).unwrap_or("");
    let cls = node
        .attributes
        .get("class")
        .map(|v| v.as_str())
        .unwrap_or("");
    let bg = s.background_color;
    let bg_str = if bg.a > 0 {
        format!("#{:02x}{:02x}{:02x}", bg.r, bg.g, bg.b)
    } else {
        "transparent".to_string()
    };
    let color_str = format!("#{:02x}{:02x}{:02x}", s.color.r, s.color.g, s.color.b);
    let mask_image = s.rare().mask_image_url.as_str();
    let svg_child_count = node
        .svg_document
        .as_ref()
        .map(|doc| doc.root.children.len())
        .unwrap_or(0);
    format!(
        concat!(
            r#"{{"tag":{0},"id":{1},"class":{2},"#,
            r#""content":{{"x":{3:.1},"y":{4:.1},"w":{5:.1},"h":{6:.1}}},"#,
            r#""padding":{{"x":{7:.1},"y":{8:.1},"w":{9:.1},"h":{10:.1}}},"#,
            r#""margin":{{"x":{11:.1},"y":{12:.1},"w":{13:.1},"h":{14:.1}}},"#,
            r#""display":{15},"position":{16},"#,
            r#""font_size":{17:.1},"text_align":{18},"color":{19},"background":{20},"#,
            r#""margin_trbl":[{21:.1},{22:.1},{23:.1},{24:.1}],"#,
            r#""padding_trbl":[{25:.1},{26:.1},{27:.1},{28:.1}],"#,
            r#""border_trbl":[{29:.1},{30:.1},{31:.1},{32:.1}],"#,
            r#""children":{33},"svg_parsed":{34},"svg_children":{35},"#,
            r#""mask_image":{36},"mask_loaded":{37},"mask_size":[{38},{39}]}}"#
        ),
        dbg_json_escape(&node.tag),
        dbg_json_escape(id),
        dbg_json_escape(cls),
        node.layout.content_rect.x,
        node.layout.content_rect.y,
        node.layout.content_rect.w,
        node.layout.content_rect.h,
        node.layout.padding_rect.x,
        node.layout.padding_rect.y,
        node.layout.padding_rect.w,
        node.layout.padding_rect.h,
        node.layout.margin_rect.x,
        node.layout.margin_rect.y,
        node.layout.margin_rect.w,
        node.layout.margin_rect.h,
        dbg_json_escape(&format!("{:?}", s.display)),
        dbg_json_escape(&format!("{:?}", s.position)),
        s.font_size_px(16.0, 16.0),
        dbg_json_escape(&format!("{:?}", s.text_align)),
        dbg_json_escape(&color_str),
        dbg_json_escape(&bg_str),
        node.layout.resolved_margin_top,
        node.layout.resolved_margin_right,
        node.layout.resolved_margin_bottom,
        node.layout.resolved_margin_left,
        node.layout.resolved_pad_top,
        node.layout.resolved_pad_right,
        node.layout.resolved_pad_bottom,
        node.layout.resolved_pad_left,
        node.layout.resolved_border_top,
        node.layout.resolved_border_right,
        node.layout.resolved_border_bottom,
        node.layout.resolved_border_left,
        node.children.iter().filter(|c| c.tag != "#text").count(),
        node.svg_document.is_some(),
        svg_child_count,
        dbg_json_escape(mask_image),
        node.mask_images
            .as_ref()
            .and_then(|images| images.first.as_ref())
            .is_some(),
        node.mask_images
            .as_ref()
            .and_then(|images| images.first.as_ref())
            .map_or(0, |image| image.width),
        node.mask_images
            .as_ref()
            .and_then(|images| images.first.as_ref())
            .map_or(0, |image| image.height),
    )
}

pub(super) fn dbg_svg_metrics_json(root: &crate::WebCore) -> String {
    let mut summary = crate::svg::SvgUnsupportedSummary::default();
    Document::walk_all(root, &mut |node| {
        if let Some(doc) = node.svg_document.as_ref() {
            summary.merge(crate::svg::unsupported_summary(doc));
        }
    });
    format!(
        r#"{{"documents":{},"unsupported_count":{},"clean":{},"unknown_elements":{},"unsupported_elements":{},"unsupported_attributes":{}}}"#,
        summary.documents,
        svg_metric_total(&summary),
        summary.is_empty(),
        dbg_count_map_json(&summary.unknown_elements),
        dbg_count_map_json(&summary.unsupported_elements),
        dbg_count_map_json(&summary.unsupported_attributes),
    )
}

pub(super) fn svg_metric_total(summary: &crate::svg::SvgUnsupportedSummary) -> usize {
    summary.unknown_elements.values().sum::<usize>()
        + summary.unsupported_elements.values().sum::<usize>()
        + summary.unsupported_attributes.values().sum::<usize>()
}

pub(super) fn dbg_count_map_json(map: &std::collections::BTreeMap<String, usize>) -> String {
    let mut parts = Vec::new();
    for (key, count) in map {
        parts.push(format!("{}:{}", dbg_json_escape(key), count));
    }
    format!("{{{}}}", parts.join(","))
}

pub(super) fn dbg_computed_json(node: &crate::WebCore) -> String {
    use std::fmt::Write;
    let s = &node.style;
    let id = node.attributes.get("id").map(|v| v.as_str()).unwrap_or("");
    let cls = node
        .attributes
        .get("class")
        .map(|v| v.as_str())
        .unwrap_or("");
    let bg = s.background_color;
    let bg_str = if bg.a > 0 {
        format!("#{:02x}{:02x}{:02x}", bg.r, bg.g, bg.b)
    } else {
        "transparent".to_string()
    };
    let c = s.color;
    let color_hex = format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b);
    let oc = s.outline_color;
    let outline_color_hex = format!("#{:02x}{:02x}{:02x}{:02x}", oc.r, oc.g, oc.b, oc.a);
    let mut buf = String::with_capacity(2048);
    let _ = write!(
        buf,
        r#"{{"node_id":{},"tag":{},"id":{},"class":{}"#,
        node.node_id,
        dbg_json_escape(&node.tag),
        dbg_json_escape(id),
        dbg_json_escape(cls)
    );
    let _ = write!(
        buf,
        r#","box":{{"content":[{:.1},{:.1},{:.1},{:.1}],"padding":[{:.1},{:.1},{:.1},{:.1}],"margin":[{:.1},{:.1},{:.1},{:.1}],"border":[{:.1},{:.1},{:.1},{:.1}]}}"#,
        node.layout.content_rect.x,
        node.layout.content_rect.y,
        node.layout.content_rect.w,
        node.layout.content_rect.h,
        node.layout.padding_rect.x,
        node.layout.padding_rect.y,
        node.layout.padding_rect.w,
        node.layout.padding_rect.h,
        node.layout.margin_rect.x,
        node.layout.margin_rect.y,
        node.layout.margin_rect.w,
        node.layout.margin_rect.h,
        node.layout.border_rect.x,
        node.layout.border_rect.y,
        node.layout.border_rect.w,
        node.layout.border_rect.h
    );
    let _ = write!(
        buf,
        r#","display":{},"position":{},"float":{}"#,
        dbg_json_escape(&format!("{:?}", s.display)),
        dbg_json_escape(&format!("{:?}", s.position)),
        dbg_json_escape(&format!("{:?}", s.float))
    );
    let _ = write!(
        buf,
        r#","visibility":{},"opacity":{:.2}"#,
        dbg_json_escape(&format!("{:?}", s.visibility)),
        s.opacity
    );
    let _ = write!(
        buf,
        r#","overflow":[{},{}],"box_sizing":{}"#,
        dbg_json_escape(&format!("{:?}", s.overflow_x)),
        dbg_json_escape(&format!("{:?}", s.overflow_y)),
        dbg_json_escape(&format!("{:?}", s.box_sizing))
    );
    let _ = write!(
        buf,
        r#","width":{},"height":{}"#,
        dbg_json_escape(&format!("{:?}", s.width)),
        dbg_json_escape(&format!("{:?}", s.height))
    );
    let _ = write!(
        buf,
        r#","grid_placement":{{"column":[{},{}],"row":[{},{}],"order":{}}}"#,
        s.grid_column_start, s.grid_column_end, s.grid_row_start, s.grid_row_end, s.order
    );
    let _ = write!(
        buf,
        r#","font_size":{:.1},"font_weight":{},"font_family":{}"#,
        s.font_size_px(
            crate::ComputedStyle::INITIAL_FONT_SIZE_PX,
            crate::ComputedStyle::INITIAL_FONT_SIZE_PX
        ),
        dbg_json_escape(&format!("{:?}", s.font_weight)),
        dbg_json_escape(&s.font_family)
    );
    let _ = write!(
        buf,
        r#","text_align":{},"vertical_align":{},"direction":{},"writing_mode":{}"#,
        dbg_json_escape(&format!("{:?}", s.text_align)),
        dbg_json_escape(&format!("{:?}", s.vertical_align)),
        dbg_json_escape(&format!("{:?}", s.direction)),
        dbg_json_escape(&format!("{:?}", s.writing_mode))
    );
    let _ = write!(
        buf,
        r#","color":{},"background":{}"#,
        dbg_json_escape(&color_hex),
        dbg_json_escape(&bg_str)
    );
    let _ = write!(
        buf,
        r#","outline":{{"width":{:.1},"style":{},"offset":{:.1},"color":{}}}"#,
        s.outline_width,
        dbg_json_escape(&format!("{:?}", s.outline_style)),
        s.outline_offset,
        dbg_json_escape(&outline_color_hex)
    );
    let _ = write!(
        buf,
        r#","flex_direction":{},"flex_wrap":{}"#,
        dbg_json_escape(&format!("{:?}", s.flex_direction)),
        dbg_json_escape(&format!("{:?}", s.flex_wrap))
    );
    let _ = write!(
        buf,
        r#","flex_grow":{},"flex_shrink":{},"align_items":{},"justify_content":{}"#,
        s.flex_grow,
        s.flex_shrink,
        dbg_json_escape(&format!("{:?}", s.align_items)),
        dbg_json_escape(&format!("{:?}", s.justify_content))
    );
    let _ = write!(
        buf,
        r#","css_padding":[{},{},{},{}]"#,
        dbg_json_escape(&format!("{:?}", s.padding_top)),
        dbg_json_escape(&format!("{:?}", s.padding_right)),
        dbg_json_escape(&format!("{:?}", s.padding_bottom)),
        dbg_json_escape(&format!("{:?}", s.padding_left))
    );
    let _ = write!(
        buf,
        r#","css_margin":[{},{},{},{}]"#,
        dbg_json_escape(&format!("{:?}", s.margin_top)),
        dbg_json_escape(&format!("{:?}", s.margin_right)),
        dbg_json_escape(&format!("{:?}", s.margin_bottom)),
        dbg_json_escape(&format!("{:?}", s.margin_left))
    );
    let _ = write!(
        buf,
        r#","resolved_padding":[{:.1},{:.1},{:.1},{:.1}]"#,
        node.layout.resolved_pad_top,
        node.layout.resolved_pad_right,
        node.layout.resolved_pad_bottom,
        node.layout.resolved_pad_left
    );
    let _ = write!(
        buf,
        r#","resolved_margin":[{:.1},{:.1},{:.1},{:.1}]"#,
        node.layout.resolved_margin_top,
        node.layout.resolved_margin_right,
        node.layout.resolved_margin_bottom,
        node.layout.resolved_margin_left
    );
    let _ = write!(
        buf,
        r#","checked":{},"selected":{},"dirty_checked":{},"dirty_selected":{}"#,
        node.checkedness, node.selectedness, node.dirty_checked, node.dirty_selectedness
    );
    let _ = write!(
        buf,
        r#", "scroll":{{"left":{:.2},"top":{:.2},"width":{:.2},"height":{:.2}}}"#,
        node.layout.scroll_left,
        node.layout.scroll_top,
        node.layout.scroll_width,
        node.layout.scroll_height
    );
    let _ = write!(
        buf,
        r#","border_collapse":{},"matched_rules":{},"line_count":{}}}"#,
        s.border_collapse,
        node.matched_rules.len(),
        node.layout.line_cache.len()
    );
    buf
}

pub(super) fn dbg_match_report_json(doc: &Document, node: &crate::WebCore) -> String {
    let hover_chain = crate::css::build_hover_chain(&doc.root, doc.hovered_box);
    let Some(report) = crate::css::debug_match_report_for_node(
        &doc.root,
        &doc.stylesheet,
        node.node_id,
        doc.viewport_w,
        doc.viewport_h,
        doc.focused_box,
        doc.keyboard_focus,
        &hover_chain,
        0,
        &doc.base_url,
    ) else {
        return format!(
            r#"{{"node_id":{},"ok":false,"error":"node not found"}}"#,
            node.node_id
        );
    };
    let rule_names = |indices: &[usize]| -> String {
        indices
            .iter()
            .filter_map(|idx| {
                doc.stylesheet.rules.get(*idx).map(|rule| {
                    format!(
                        r#"{{"idx":{},"selector":{},"pseudo":"{:?}"}}"#,
                        idx,
                        dbg_json_escape(&rule.original_selector),
                        rule.pseudo_element
                    )
                })
            })
            .collect::<Vec<_>>()
            .join(",")
    };
    format!(
        r#"{{"ok":true,"node_id":{},"tag":{},"id":{},"class":{},"candidate_count":{},"matched_count":{},"candidates":[{}],"matched":[{}],"hover":[{}],"active":[{}],"visited":[{}],"before":[{}],"after":[{}]}}"#,
        report.node_id,
        dbg_json_escape(&report.tag),
        dbg_json_escape(&report.id),
        dbg_json_escape(&report.class_attr),
        report.candidate_rule_indices.len(),
        report.matched_rule_indices.len(),
        rule_names(&report.candidate_rule_indices),
        rule_names(&report.matched_rule_indices),
        rule_names(&report.hover_rule_indices),
        rule_names(&report.active_rule_indices),
        rule_names(&report.visited_rule_indices),
        rule_names(&report.before_rule_indices),
        rule_names(&report.after_rule_indices)
    )
}

pub(super) fn dbg_height_dump_json(root: &crate::WebCore, limit: usize) -> String {
    let mut rows: Vec<(f32, String)> = Vec::new();
    fn walk(node: &crate::WebCore, ancestors: &mut Vec<String>, rows: &mut Vec<(f32, String)>) {
        if matches!(node.style.display, crate::types::Display::None) {
            return;
        }
        let mr = node.layout.margin_rect;
        let cr = node.layout.content_rect;
        let bottom = mr.y + mr.h;
        if bottom <= 0.0 {
            return;
        }
        let id = node.attributes.get("id").map(|v| v.as_str()).unwrap_or("");
        let cls = node
            .attributes
            .get("class")
            .map(|v| v.as_str())
            .unwrap_or("");
        let ancestor_json = ancestors
            .iter()
            .rev()
            .take(8)
            .rev()
            .map(|tag| dbg_json_escape(tag))
            .collect::<Vec<_>>()
            .join(",");
        rows.push((
            bottom,
            format!(
                r#"{{"node_id":{},"tag":{},"id":{},"class":{},"ancestors":[{}],"display":{},"position":{},"overflow_y":{},"visibility":{},"content":[{:.1},{:.1},{:.1},{:.1}],"margin":[{:.1},{:.1},{:.1},{:.1}],"bottom":{:.1}}}"#,
                node.node_id,
                dbg_json_escape(&node.tag),
                dbg_json_escape(id),
                dbg_json_escape(cls),
                ancestor_json,
                dbg_json_escape(&format!("{:?}", node.style.display)),
                dbg_json_escape(&format!("{:?}", node.style.position)),
                dbg_json_escape(&format!("{:?}", node.style.overflow_y)),
                dbg_json_escape(&format!("{:?}", node.style.visibility)),
                cr.x,
                cr.y,
                cr.w,
                cr.h,
                mr.x,
                mr.y,
                mr.w,
                mr.h,
                bottom
            ),
        ));
        ancestors.push(node.tag.clone());
        for child in &node.children {
            walk(child, ancestors, rows);
        }
        if let Some(shadow) = node.shadow_root.as_ref() {
            ancestors.push("#shadow-root".to_string());
            for child in &shadow.children {
                walk(child, ancestors, rows);
            }
            ancestors.pop();
        }
        ancestors.pop();
    }
    walk(root, &mut Vec::new(), &mut rows);
    rows.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    rows.truncate(limit.max(1));
    let elements = rows.into_iter().map(|(_, json)| json).collect::<Vec<_>>();
    format!(
        r#"{{"ok":true,"count":{},"scroll_height":{:.1},"elements":[{}]}}"#,
        elements.len(),
        Document::scroll_height(root),
        elements.join(",")
    )
}

pub(super) fn dbg_dump_box(depth: usize, node: &crate::WebCore, buf: &mut String) {
    use crate::types::Display;
    use std::fmt::Write;
    if matches!(node.style.display, Display::None) {
        return;
    }
    let indent = "  ".repeat(depth);
    let tag = if node.tag.is_empty() {
        "(box)"
    } else {
        &node.tag
    };
    let id = node
        .attributes
        .get("id")
        .map(|v| format!("#{v}"))
        .unwrap_or_default();
    let cls = node
        .attributes
        .get("class")
        .map(|v| {
            format!(
                ".{}",
                v.split_whitespace().take(3).collect::<Vec<_>>().join(".")
            )
        })
        .unwrap_or_default();
    let text_preview = if node.tag == "#text" && !node.text.is_empty() {
        let s: String = node.text.chars().take(40).collect();
        format!(" {:?}", s.trim())
    } else {
        String::new()
    };
    let _ = writeln!(
        buf,
        "{}{}{}{} [{:?} pos={:?} w={:?} h={:?}] c=[{:.0},{:.0} {:.0}x{:.0}] m=[{:.0},{:.0} {:.0}x{:.0}]{}",
        indent,
        tag,
        id,
        cls,
        node.style.display,
        node.style.position,
        node.style.width,
        node.style.height,
        node.layout.content_rect.x,
        node.layout.content_rect.y,
        node.layout.content_rect.w,
        node.layout.content_rect.h,
        node.layout.margin_rect.x,
        node.layout.margin_rect.y,
        node.layout.margin_rect.w,
        node.layout.margin_rect.h,
        text_preview
    );
    for child in node.effective_children() {
        dbg_dump_box(depth + 1, child, buf);
    }
}

pub(super) fn dbg_serialize_html(node: &crate::WebCore, buf: &mut String, depth: usize) {
    use std::fmt::Write;
    if node.tag == "#text" {
        let t = node.text.trim();
        if !t.is_empty() {
            let _ = write!(buf, "{}", t);
        }
        return;
    }
    let indent = "  ".repeat(depth);
    let _ = write!(buf, "{}<{}", indent, node.tag);
    for (k, v) in &node.attributes {
        let _ = write!(buf, " {}={}", k, dbg_json_escape(v));
    }
    let _ = write!(buf, ">");
    if !node.children.is_empty() {
        let _ = writeln!(buf);
        for child in &node.children {
            dbg_serialize_html(child, buf, depth + 1);
        }
        let _ = write!(buf, "{}</{}>", indent, node.tag);
    } else {
        let _ = write!(buf, "</{}>", node.tag);
    }
    let _ = writeln!(buf);
}

pub(super) fn dbg_selector_center(doc: &Document, selector: &str) -> Option<(f32, f32)> {
    dbg_select_composed_with_pseudo(&doc.root, doc, selector)
        .into_iter()
        .find_map(|node| {
            let r = doc.get_bounding_client_rect(node.node_id)?;
            if r.w > 0.0 && r.h > 0.0 {
                Some((r.x + r.w / 2.0, r.y + r.h / 2.0))
            } else {
                None
            }
        })
}

pub(super) fn dbg_element_scroll_offsets(
    doc: &Document,
) -> std::collections::HashMap<u32, (f32, f32)> {
    let mut offsets = std::collections::HashMap::new();
    dbg_walk_composed(&doc.root, &mut |node| {
        if node.node_id != 0 && (node.layout.scroll_left != 0.0 || node.layout.scroll_top != 0.0) {
            offsets.insert(
                node.node_id,
                (node.layout.scroll_left, node.layout.scroll_top),
            );
        }
    });
    offsets
}

impl BrowserApp {
    /// Handle a single remote debug command line against the active tab.
    pub(super) fn handle_debug_command(&mut self, line: &str) -> String {
        let line = line.trim();
        if line.is_empty() {
            return String::new();
        }
        let t0 = std::time::Instant::now();
        let cmd = dbg_json_str(line, "cmd").unwrap_or_default();
        let result = self.dispatch_debug_cmd(&cmd, line);
        if debug_cmd_requests_redraw(&cmd) && result.starts_with(r#"{"ok":true"#) {
            if let Some(w) = &self.window {
                w.request_redraw();
            }
        }
        let cmd_ms = t0.elapsed().as_micros() as f64 / 1000.0;
        if result.starts_with(r#"{"ok":true"#) && cmd != "perf" {
            let insert_pos = result.len() - 1;
            let mut r = result;
            r.insert_str(insert_pos, &format!(r#","cmd_ms":{:.2}"#, cmd_ms));
            r
        } else {
            result
        }
    }

    fn dispatch_debug_cmd(&mut self, cmd: &str, line: &str) -> String {
        match cmd {
            "browser-ui" => {
                let action = dbg_json_str(line, "action");
                match dbg_json_str(line, "action").as_deref() {
                    Some("click") => {
                        let point = if let Some(selector) = dbg_json_str(line, "selector") {
                            self.chrome_doc.as_ref().and_then(|doc| dom::query_selector(&doc.root, &selector))
                                .map(|node| { let r = node.layout.border_rect; (r.x + r.w / 2.0, r.y + r.h / 2.0) })
                        } else { dbg_json_num(line, "x").zip(dbg_json_num(line, "y")) };
                        let Some((x, y)) = point else { return r#"{"ok":false,"error":"missing browser control"}"#.into(); };
                        let hit = self.chrome_hit(x, y);
                        self.activate_chrome_hit(hit);
                    }
                    Some("type") if self.url_focused => {
                        for ch in dbg_json_str(line, "text").unwrap_or_default().chars() {
                            self.edit_address(0, Some(ch), false, false, false, false);
                        }
                    }
                    Some("submit") if self.url_focused => self.navigate(self.url_text.clone()),
                    Some("select-all") if self.url_focused => self.edit_address(65, Some('a'), true, false, false, false),
                    Some("key") if self.url_focused => {
                        let key = dbg_json_str(line, "key").unwrap_or_default();
                        let ch = key.chars().next().filter(|_| key.chars().count() == 1);
                        let code = match key.as_str() {
                            "Backspace" => 8,
                            "Delete" => 46,
                            "ArrowLeft" => 37,
                            "ArrowRight" => 39,
                            "Home" => 36,
                            "End" => 35,
                            _ => ch.map(u32::from).unwrap_or(0),
                        };
                        self.edit_address(code, ch,
                            dbg_json_bool(line, "ctrl").unwrap_or(false),
                            dbg_json_bool(line, "shift").unwrap_or(false),
                            dbg_json_bool(line, "alt").unwrap_or(false),
                            dbg_json_bool(line, "meta").unwrap_or(false));
                    }
                    Some(_) => return r#"{"ok":false,"error":"unsupported action or address not focused"}"#.into(),
                    None => {}
                }
                if action.is_some() {
                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                }
                let tabs = self
                    .tabs
                    .iter()
                    .enumerate()
                    .map(|(index, tab)| {
                        format!(
                            r#"{{"index":{index},"active":{},"url":{},"title":{},"loading":{}}}"#,
                            index == self.active,
                            json_quote(&tab.url),
                            json_quote(&tab.title),
                            tab.loading
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                let controls = self
                    .chrome_doc
                    .as_ref()
                    .map(|doc| {
                        [
                            "#url-input",
                            "#btn-new-tab",
                            "#btn-back",
                            "#btn-fwd",
                            "#btn-reload",
                        ]
                        .iter()
                        .filter_map(|selector| {
                            let node = dom::query_selector(&doc.root, selector)?;
                            let r = node.layout.border_rect;
                            Some(format!(
                                r#"{{"selector":{},"x":{},"y":{},"width":{},"height":{}}}"#,
                                json_quote(selector),
                                r.x,
                                r.y,
                                r.w,
                                r.h
                            ))
                        })
                        .collect::<Vec<_>>()
                        .join(",")
                    })
                    .unwrap_or_default();
                let displayed_address = self
                    .chrome_doc
                    .as_ref()
                    .and_then(|doc| doc.query_selector("#url-input").map(|id| doc.value(id)));
                format!(
                    r#"{{"ok":true,"active":{},"address":{},"displayed_address":{},"focused":{},"tabs":[{}],"controls":[{}]}}"#,
                    self.active,
                    json_quote(&self.url_text),
                    displayed_address
                        .as_deref()
                        .map(json_quote)
                        .unwrap_or_else(|| "null".into()),
                    self.url_focused,
                    tabs,
                    controls
                )
            }
            "compare-snapshot" => {
                let document = self.tabs[self.active].view.document();
                let Some(doc) = document.as_deref() else {
                    return r#"{"ok":false,"error":"no document"}"#.into();
                };
                let selector = dbg_json_str(line, "selector");
                let roots: std::collections::HashSet<u32> = selector
                    .as_ref()
                    .map(|sel| {
                        dbg_select_composed_with_pseudo(&doc.root, doc, sel)
                            .iter()
                            .map(|n| n.node_id)
                            .collect()
                    })
                    .unwrap_or_default();
                let mut id_counts = std::collections::HashMap::new();
                let mut hidden_nodes = std::collections::HashMap::new();
                Document::walk_all(&doc.root, &mut |n| {
                    if n.style.display == Display::None || n.style.opacity == 0.0 {
                        hidden_nodes.insert(
                            n.node_id,
                            if n.style.display == Display::None {
                                "display:none"
                            } else {
                                "opacity:0"
                            },
                        );
                    }
                    if let Some(id) = n.attributes.get("id").filter(|id| !id.is_empty()) {
                        *id_counts.entry(id.clone()).or_insert(0usize) += 1;
                    }
                });
                let mut elements = Vec::new();
                Document::walk_all(&doc.root, &mut |node| {
                    use crate::dom::arena::{NodeId, NodeType};
                    let Some(dom) = doc.arena.try_get(NodeId(node.node_id)) else {
                        return;
                    };
                    if dom.node_type != NodeType::Element {
                        return;
                    }
                    let mut selected = selector.is_none();
                    let mut path = Vec::new();
                    let mut anchor = String::new();
                    let mut hidden_by = if node.style.visibility {
                        String::new()
                    } else {
                        "visibility:hidden".to_string()
                    };
                    let mut current = NodeId(node.node_id);
                    while let Some(n) = doc.arena.try_get(current) {
                        selected |= roots.contains(&current.0);
                        if hidden_by.is_empty() {
                            if let Some(reason) = hidden_nodes.get(&current.0) {
                                hidden_by = format!("{reason} on {} (node {})", n.tag, current.0);
                            }
                        }
                        if anchor.is_empty() {
                            if let Some(id) = n
                                .attributes
                                .get("id")
                                .filter(|id| id_counts.get(*id) == Some(&1))
                            {
                                let suffix =
                                    path.iter().rev().cloned().collect::<Vec<_>>().join(" > ");
                                anchor = format!("{}|{}", id.len(), id);
                                anchor.push_str(&suffix);
                            }
                        }
                        if n.node_type == NodeType::Element {
                            let mut index = 1;
                            let mut sibling = n.prev_sibling;
                            while let Some(s) = doc.arena.try_get(sibling) {
                                if s.node_type == NodeType::Element && s.tag == n.tag {
                                    index += 1;
                                }
                                sibling = s.prev_sibling;
                            }
                            path.push(format!("{}:nth-of-type({index})", n.tag));
                        }
                        current = n.parent;
                    }
                    if !selected {
                        return;
                    }
                    path.reverse();
                    let s = &node.style;
                    let text = doc
                        .arena
                        .children(NodeId(node.node_id))
                        .filter_map(|id| {
                            let n = doc.arena.get(id);
                            (n.node_type == NodeType::Text).then_some(n.text.as_str())
                        })
                        .collect::<Vec<_>>()
                        .join(" ");
                    let text = text
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ")
                        .chars()
                        .take(160)
                        .collect::<String>();
                    let attrs = ["href", "src", "alt", "aria-label", "role", "name", "type"]
                        .iter()
                        .filter_map(|key| {
                            dom.attributes.get(*key).map(|value| {
                                format!("{}:{}", dbg_json_escape(key), dbg_json_escape(value))
                            })
                        })
                        .collect::<Vec<_>>()
                        .join(",");
                    elements.push(format!(r#"{{"path":{},"anchor":{},"text":{},"hidden_by":{},"attrs":{{{}}},"computed":{},"colors":{{"color":[{},{},{},{}],"background":[{},{},{},{}]}},"image":{{"src":{},"width":{},"height":{},"decoded":{}}}}}"#,
                        dbg_json_escape(&path.join(" > ")), dbg_json_escape(&anchor), dbg_json_escape(&text), dbg_json_escape(&hidden_by), attrs, dbg_computed_json(node),
                        s.color.r,s.color.g,s.color.b,s.color.a,
                        s.background_color.r,s.background_color.g,s.background_color.b,s.background_color.a,
                        dbg_json_escape(&node.resolved_src),node.image_width,node.image_height,
                        node.image_data.is_some() || node.svg_document.is_some()));
                });
                format!(
                    r#"{{"ok":true,"url":{},"loading":{},"chrome_port":{},"elements":[{}]}}"#,
                    dbg_json_escape(&self.tabs[self.active].url),
                    self.tabs[self.active].loading,
                    self.chrome_port,
                    elements.join(",")
                )
            }
            "chrome-eval" => {
                if self.chrome_port == 0 {
                    return r#"{"ok":false,"error":"start GUI with --chrome"}"#.to_string();
                }
                let Some(expression) = dbg_json_str(line, "expression") else {
                    return r#"{"ok":false,"error":"chrome-eval needs expression"}"#.to_string();
                };
                let params = format!(
                    r#"{{"expression":{},"returnByValue":true}}"#,
                    json_quote(&expression)
                );
                match cdp_send(self.chrome_port, "Runtime.evaluate", &params) {
                    Ok(response) => format!(r#"{{"ok":true,"cdp":{response}}}"#),
                    Err(error) => format!(r#"{{"ok":false,"error":{}}}"#, dbg_json_escape(&error)),
                }
            }
            "chrome-type" => {
                if self.chrome_port == 0 {
                    return r#"{"ok":false,"error":"start GUI with --chrome"}"#.to_string();
                }
                let Some(value) = dbg_json_str(line, "text") else {
                    return r#"{"ok":false,"error":"chrome-type needs text"}"#.to_string();
                };
                let params = format!(r#"{{"text":{}}}"#, json_quote(&value));
                match cdp_send(self.chrome_port, "Input.insertText", &params) {
                    Ok(response) => format!(r#"{{"ok":true,"cdp":{response}}}"#),
                    Err(error) => format!(r#"{{"ok":false,"error":{}}}"#, dbg_json_escape(&error)),
                }
            }
            "chrome-key" => {
                if self.chrome_port == 0 {
                    return r#"{"ok":false,"error":"start GUI with --chrome"}"#.to_string();
                }
                for key_up in [false, true] {
                    let params = match chrome_key_params(line, key_up) {
                        Ok(params) => params,
                        Err(error) => {
                            return serde_json::json!({"ok":false,"error":error}).to_string();
                        }
                    };
                    match cdp_send(self.chrome_port, "Input.dispatchKeyEvent", &params) {
                        Err(error) => {
                            return serde_json::json!({"ok":false,"error":error}).to_string();
                        }
                        Ok(response) => {
                            if let Ok(response) =
                                serde_json::from_str::<serde_json::Value>(&response)
                                && response.get("error").is_some()
                            {
                                return serde_json::json!({"ok":false,"cdp":response}).to_string();
                            }
                        }
                    }
                }
                r#"{"ok":true}"#.to_string()
            }
            "chrome-sync" => {
                if self.chrome_port == 0 {
                    return r#"{"ok":false,"error":"start GUI with --chrome"}"#.to_string();
                }
                let url = self.tabs[self.active].url.clone();
                let params = format!(r#"{{"url":{}}}"#, json_quote(&url));
                match cdp_send(self.chrome_port, "Page.navigate", &params) {
                    Ok(response) => format!(r#"{{"ok":true,"cdp":{response}}}"#),
                    Err(error) => format!(r#"{{"ok":false,"error":{}}}"#, dbg_json_escape(&error)),
                }
            }
            "chrome-screenshot" => {
                if self.chrome_port == 0 {
                    return r#"{"ok":false,"error":"start GUI with --chrome"}"#.to_string();
                }
                let path = dbg_json_str(line, "out")
                    .unwrap_or_else(|| "chrome_screenshot.png".to_string());
                match cdp_send(
                    self.chrome_port,
                    "Page.captureScreenshot",
                    r#"{"format":"png"}"#,
                ) {
                    Ok(response) => {
                        if let Some(data) = dbg_json_str(&response, "data") {
                            match base64_decode_std(&data).and_then(|bytes| {
                                std::fs::write(&path, bytes).map_err(|e| e.to_string())
                            }) {
                                Ok(()) => {
                                    format!(r#"{{"ok":true,"path":{}}}"#, dbg_json_escape(&path))
                                }
                                Err(error) => {
                                    format!(r#"{{"ok":false,"error":{}}}"#, dbg_json_escape(&error))
                                }
                            }
                        } else {
                            r#"{"ok":false,"error":"Chrome screenshot missing data"}"#.to_string()
                        }
                    }
                    Err(error) => format!(r#"{{"ok":false,"error":{}}}"#, dbg_json_escape(&error)),
                }
            }
            // ── Screenshot ───────────────────────────────────────────────────
            "screenshot" => {
                let path = dbg_json_str(line, "out").unwrap_or_else(|| "snapshot.png".to_string());
                if dbg_json_bool(line, "presented").unwrap_or(false) {
                    return match self.platform.as_ref() {
                        Some(platform) => match platform.save_presented_png(&path) {
                            Ok((width, height)) => format!(
                                r#"{{"ok":true,"path":{},"width":{},"height":{},"presented":true}}"#,
                                dbg_json_escape(&path),
                                width,
                                height,
                            ),
                            Err(error) => {
                                format!(r#"{{"ok":false,"error":{}}}"#, dbg_json_escape(&error),)
                            }
                        },
                        None => r#"{"ok":false,"error":"no GUI surface"}"#.to_string(),
                    };
                }
                let scale = dbg_json_num(line, "scale")
                    .map(|v| v.clamp(0.25, 4.0) as f32)
                    .unwrap_or(1.0);
                let view_w = self.width;
                let view_h = self.content_h();
                let phys_w = ((view_w as f32) * scale).ceil() as u32;
                let ch = ((view_h as f32) * scale).ceil() as u32;
                if self.tabs[self.active].view.document().is_none() {
                    return r#"{"ok":false,"error":"no document loaded"}"#.to_string();
                }
                let Some(mut pm) = tiny_skia::Pixmap::new(phys_w.max(1), ch.max(1)) else {
                    return r#"{"ok":false,"error":"pixmap alloc failed"}"#.to_string();
                };
                pm.fill(tiny_skia::Color::WHITE);
                self.tabs[self.active].view.resize(view_w, view_h);
                self.tabs[self.active].view.paint_into(&mut pm, 0, 0, scale);
                match pm.save_png(&path) {
                    Ok(_) => format!(
                        r#"{{"ok":true,"path":{},"width":{},"height":{},"scale":{}}}"#,
                        dbg_json_escape(&path),
                        phys_w,
                        ch,
                        scale
                    ),
                    Err(e) => format!(
                        r#"{{"ok":false,"error":{}}}"#,
                        dbg_json_escape(&e.to_string())
                    ),
                }
            }
            // ── Navigation ───────────────────────────────────────────────────
            "navigate" => match dbg_json_str(line, "url") {
                Some(u) => {
                    self.navigate(normalize_url(u));
                    format!(r#"{{"ok":true}}"#)
                }
                None => r#"{"ok":false,"error":"navigate needs url"}"#.to_string(),
            },
            "back" => {
                if self.tabs[self.active].can_back() {
                    self.go_back();
                    format!(
                        r#"{{"ok":true,"url":{}}}"#,
                        dbg_json_escape(&self.tabs[self.active].url)
                    )
                } else {
                    r#"{"ok":false,"error":"no back history"}"#.to_string()
                }
            }
            "forward" => {
                if self.tabs[self.active].can_forward() {
                    self.go_forward();
                    format!(
                        r#"{{"ok":true,"url":{}}}"#,
                        dbg_json_escape(&self.tabs[self.active].url)
                    )
                } else {
                    r#"{"ok":false,"error":"no forward history"}"#.to_string()
                }
            }
            "profile" => {
                if dbg_json_bool(line, "reset").unwrap_or(false) {
                    crate::profile::reset();
                }
                profile_json(dbg_json_num(line, "limit").unwrap_or(20.0) as usize)
            }
            // ── Browse tab list / switch ──────────────────────────────────────
            "tabs" => {
                let list: Vec<String> = self
                    .tabs
                    .iter()
                    .enumerate()
                    .map(|(i, t)| {
                        format!(
                            r#"{{"index":{},"active":{},"url":{},"title":{}}}"#,
                            i,
                            i == self.active,
                            dbg_json_escape(&t.url),
                            dbg_json_escape(&t.title)
                        )
                    })
                    .collect();
                format!(
                    r#"{{"ok":true,"count":{},"tabs":[{}]}}"#,
                    list.len(),
                    list.join(",")
                )
            }
            "switch-tab" => {
                if let Some(idx) = dbg_json_num(line, "index") {
                    self.switch_tab(idx as usize);
                    format!(r#"{{"ok":true,"index":{}}}"#, idx as usize)
                } else {
                    r#"{"ok":false,"error":"switch-tab needs index"}"#.to_string()
                }
            }
            // ── Resize ───────────────────────────────────────────────────────
            "resize" => {
                if let Some(w) = dbg_json_num(line, "width") {
                    self.width = w;
                    self.relayout_active();
                }
                format!(
                    r#"{{"ok":true,"width":{:.0},"height":{:.0}}}"#,
                    self.width,
                    self.content_h()
                )
            }
            // ── Scroll ───────────────────────────────────────────────────────
            "scroll" => {
                if self.tabs[self.active].view.document().is_none() {
                    r#"{"ok":false,"error":"no document"}"#.to_string()
                } else {
                    let before = self.tabs[self.active]
                        .view
                        .document()
                        .as_deref()
                        .map(dbg_element_scroll_offsets)
                        .unwrap_or_default();
                    let mut changed = false;
                    if dbg_json_num(line, "dx").is_some() || dbg_json_num(line, "dy").is_some() {
                        let dx = dbg_json_num(line, "dx").unwrap_or(0.0);
                        let dy = dbg_json_num(line, "dy").unwrap_or(0.0);
                        changed = self.tabs[self.active].view.scroll_by(dx, dy);
                    } else if let Some(y) = dbg_json_num(line, "y") {
                        changed = self.tabs[self.active].view.scroll_to(0.0, y);
                    }
                    let element_scroll = self.tabs[self.active]
                        .view
                        .document()
                        .as_deref()
                        .and_then(|doc| {
                            let after = dbg_element_scroll_offsets(doc);
                            after.iter().find_map(|(id, offset)| {
                                (before.get(id).copied().unwrap_or((0.0, 0.0)) != *offset)
                                    .then_some(*id)
                            })
                        });
                    format!(
                        r#"{{"ok":true,"changed":{},"scroll_x":{:.0},"scroll_y":{:.0},"element_scroll":{}}}"#,
                        changed,
                        self.tabs[self.active]
                            .view
                            .document()
                            .as_deref()
                            .map_or(0.0, |doc| doc.scroll_x),
                        self.tabs[self.active].view.scroll_y(),
                        element_scroll.map_or("null".to_string(), |id| id.to_string()),
                    )
                }
            }
            // ── Click ────────────────────────────────────────────────────────
            "click" => {
                let coords = if let Some(sel) = dbg_json_str(line, "selector") {
                    let document = self.tabs[self.active].view.document();
                    let doc = document.as_deref();
                    doc.and_then(|d| dbg_selector_center(d, &sel))
                        .ok_or_else(|| {
                            format!(
                                r#"{{"ok":false,"error":"no element matches {}"}}"#,
                                dbg_json_escape(&sel)
                            )
                        })
                } else if let (Some(x), Some(y)) =
                    (dbg_json_num(line, "x"), dbg_json_num(line, "y"))
                {
                    Ok((x, y))
                } else {
                    return r#"{"ok":false,"error":"click needs x,y or selector"}"#.to_string();
                };
                let (x, y) = match coords {
                    Ok(c) => c,
                    Err(e) => return e,
                };
                self.tabs[self.active]
                    .view
                    .handle_mouse_button_with_modifiers(
                        crate::dom::HtmlEventType::MouseDown,
                        x,
                        y,
                        0,
                        dbg_json_bool(line, "ctrl").unwrap_or(false),
                        dbg_json_bool(line, "shift").unwrap_or(false),
                        dbg_json_bool(line, "alt").unwrap_or(false),
                        dbg_json_bool(line, "meta").unwrap_or(false),
                    );
                self.tabs[self.active]
                    .view
                    .handle_mouse_button_with_modifiers(
                        crate::dom::HtmlEventType::MouseUp,
                        x,
                        y,
                        0,
                        dbg_json_bool(line, "ctrl").unwrap_or(false),
                        dbg_json_bool(line, "shift").unwrap_or(false),
                        dbg_json_bool(line, "alt").unwrap_or(false),
                        dbg_json_bool(line, "meta").unwrap_or(false),
                    );
                format!(r#"{{"ok":true,"x":{:.0},"y":{:.0}}}"#, x, y)
            }
            // ── Hover ────────────────────────────────────────────────────────
            "hover" => {
                let coords = if let Some(sel) = dbg_json_str(line, "selector") {
                    let document = self.tabs[self.active].view.document();
                    let doc = document.as_deref();
                    doc.and_then(|d| dbg_selector_center(d, &sel))
                        .ok_or_else(|| {
                            format!(
                                r#"{{"ok":false,"error":"no element matches {}"}}"#,
                                dbg_json_escape(&sel)
                            )
                        })
                } else if let (Some(x), Some(y)) =
                    (dbg_json_num(line, "x"), dbg_json_num(line, "y"))
                {
                    Ok((x, y))
                } else {
                    return r#"{"ok":false,"error":"hover needs x,y or selector"}"#.to_string();
                };
                let (x, y) = match coords {
                    Ok(c) => c,
                    Err(e) => return e,
                };
                let changed = self.tabs[self.active].view.handle_mouse_move(x, y);
                format!(r#"{{"ok":true,"changed":{}}}"#, changed)
            }
            // ── Type / Key ───────────────────────────────────────────────────
            "type" => match dbg_json_str(line, "text") {
                Some(text) => {
                    let mut any = false;
                    for ch in text.chars() {
                        if self.tabs[self.active].view.handle_key(
                            crate::dom::HtmlEventType::KeyDown,
                            ch as u32,
                            Some(ch),
                            false,
                            false,
                            false,
                            false,
                        ) {
                            any = true;
                        }
                    }
                    if any {}
                    format!(r#"{{"ok":true,"typed":{}}}"#, any)
                }
                None => r#"{"ok":false,"error":"type needs text"}"#.to_string(),
            },
            "key" => dbg_send_key(&mut self.tabs[self.active].view, line),
            // ── Find / Text / Attr / HTML ────────────────────────────────────
            "control-state" => {
                let selector = dbg_json_str(line, "selector").unwrap_or_default();
                self.tabs[self.active]
                    .view
                    .document()
                    .as_deref()
                    .map_or_else(
                        || r#"{"ok":false,"error":"no document"}"#.to_string(),
                        |doc| dbg_control_state(doc, &selector),
                    )
            }
            "find" => match dbg_json_str(line, "selector") {
                Some(sel) => {
                    let mut results = Vec::new();
                    if let Some(doc) = self.tabs[self.active].view.document().as_deref() {
                        Document::walk_all(&doc.root, &mut |node| {
                            if dbg_matches_query(doc, node, &sel) {
                                let id =
                                    node.attributes.get("id").map(|v| v.as_str()).unwrap_or("");
                                let cls = node
                                    .attributes
                                    .get("class")
                                    .map(|v| v.as_str())
                                    .unwrap_or("");
                                let r = &node.layout.content_rect;
                                results.push(format!(r#"{{"node_id":{},"tag":{},"id":{},"class":{},"x":{:.0},"y":{:.0},"w":{:.0},"h":{:.0}}}"#,
                                        node.node_id, dbg_json_escape(&node.tag), dbg_json_escape(id), dbg_json_escape(cls),
                                        r.x, r.y, r.w, r.h));
                            }
                        });
                    }
                    format!(
                        r#"{{"ok":true,"count":{},"elements":[{}]}}"#,
                        results.len(),
                        results.join(",")
                    )
                }
                None => r#"{"ok":false,"error":"find needs selector"}"#.to_string(),
            },
            "text" => match dbg_json_str(line, "selector") {
                Some(sel) => {
                    let mut texts = Vec::new();
                    if let Some(doc) = self.tabs[self.active].view.document().as_deref() {
                        Document::walk_all(&doc.root, &mut |node| {
                            if dbg_matches_query(doc, node, &sel) {
                                let mut t = String::new();
                                dbg_collect_text(node, &mut t);
                                texts.push(dbg_json_escape(&t));
                            }
                        });
                    }
                    format!(
                        r#"{{"ok":true,"count":{},"texts":[{}]}}"#,
                        texts.len(),
                        texts.join(",")
                    )
                }
                None => r#"{"ok":false,"error":"text needs selector"}"#.to_string(),
            },
            "attr" => match (dbg_json_str(line, "selector"), dbg_json_str(line, "name")) {
                (Some(sel), Some(name)) => {
                    let mut values = Vec::new();
                    if let Some(doc) = self.tabs[self.active].view.document().as_deref() {
                        Document::walk_all(&doc.root, &mut |node| {
                            if dbg_matches_query(doc, node, &sel) {
                                if let Some(v) = node.attributes.get(&name) {
                                    values.push(dbg_json_escape(v));
                                }
                            }
                        });
                    }
                    format!(
                        r#"{{"ok":true,"count":{},"values":[{}]}}"#,
                        values.len(),
                        values.join(",")
                    )
                }
                _ => r#"{"ok":false,"error":"attr needs selector and name"}"#.to_string(),
            },
            "html" => match dbg_json_str(line, "selector") {
                Some(sel) => {
                    let mut results = Vec::new();
                    if let Some(doc) = self.tabs[self.active].view.document().as_deref() {
                        Document::walk_all(&doc.root, &mut |node| {
                            if dbg_matches_query(doc, node, &sel) {
                                let mut buf = String::new();
                                dbg_serialize_html(node, &mut buf, 0);
                                results.push(dbg_json_escape(&buf));
                            }
                        });
                    }
                    format!(
                        r#"{{"ok":true,"count":{},"html":[{}]}}"#,
                        results.len(),
                        results.join(",")
                    )
                }
                None => r#"{"ok":false,"error":"html needs selector"}"#.to_string(),
            },
            // ── Inspect / Computed ───────────────────────────────────────────
            "inspect" => match dbg_json_str(line, "selector") {
                Some(sel) => {
                    let mut parts = Vec::new();
                    if let Some(doc) = self.tabs[self.active].view.document().as_deref() {
                        Document::walk_all(&doc.root, &mut |node| {
                            if dbg_matches_query(doc, node, &sel) {
                                parts.push(dbg_inspect_json(node));
                            }
                        });
                    }
                    format!(
                        r#"{{"ok":true,"count":{},"elements":[{}]}}"#,
                        parts.len(),
                        parts.join(",")
                    )
                }
                None => r#"{"ok":false,"error":"inspect needs selector"}"#.to_string(),
            },
            "svg-metrics" => {
                if let Some((doc, _renderer)) =
                    self.tabs[self.active].view.document_and_renderer_mut()
                {
                    format!(r#"{{"ok":true,"svg":{}}}"#, dbg_svg_metrics_json(&doc.root))
                } else {
                    r#"{"ok":false,"error":"no document"}"#.to_string()
                }
            }
            "computed" => match dbg_json_str(line, "selector") {
                Some(sel) => {
                    let mut parts = Vec::new();
                    if let Some(doc) = self.tabs[self.active].view.document().as_deref() {
                        Document::walk_all(&doc.root, &mut |node| {
                            if dbg_matches_query(doc, node, &sel) {
                                parts.push(dbg_computed_json(node));
                            }
                        });
                    }
                    format!(
                        r#"{{"ok":true,"count":{},"elements":[{}]}}"#,
                        parts.len(),
                        parts.join(",")
                    )
                }
                None => r#"{"ok":false,"error":"computed needs selector"}"#.to_string(),
            },
            "lines" => match dbg_json_str(line, "selector") {
                Some(sel) => {
                    let mut parts = Vec::new();
                    if let Some(doc) = self.tabs[self.active].view.document().as_deref() {
                        Document::walk_all(&doc.root, &mut |node| {
                            if dbg_matches_query(doc, node, &sel) {
                                let lines: Vec<String> = node.layout.line_cache.iter().map(|line| {
                                    format!(
                                        r#"{{"x":{},"y":{},"w":{},"h":{},"text_start":{},"text_length":{},"text_x_offset":{}}}"#,
                                        line.x, line.y, line.width, line.height,
                                        line.text_start, line.text_length, line.text_x_offset
                                    )
                                }).collect();
                                parts.push(format!(
                                    r#"{{"node_id":{},"lines":[{}]}}"#,
                                    node.node_id,
                                    lines.join(",")
                                ));
                            }
                        });
                    }
                    format!(
                        r#"{{"ok":true,"count":{},"elements":[{}]}}"#,
                        parts.len(),
                        parts.join(",")
                    )
                }
                None => r#"{"ok":false,"error":"lines needs selector"}"#.to_string(),
            },
            "height-dump" => {
                let limit = dbg_json_num(line, "limit").unwrap_or(40.0).max(1.0) as usize;
                if let Some(doc) = self.tabs[self.active].view.document().as_deref() {
                    dbg_height_dump_json(&doc.root, limit)
                } else {
                    r#"{"ok":false,"error":"no document"}"#.to_string()
                }
            }
            "inspect-mode" => {
                let on = dbg_json_str(line, "on")
                    .map(|v| !matches!(v.as_str(), "false" | "0"))
                    .or_else(|| dbg_json_num(line, "on").map(|v| v != 0.0))
                    .unwrap_or(true);
                self.set_inspector(on);
                format!(r#"{{"ok":true,"inspect_mode":{}}}"#, on)
            }
            "display-list-stats" => {
                if let Some(doc) = self.tabs[self.active].view.document().as_deref() {
                    let view_h = self.content_h();
                    let band =
                        crate::renderer::retained_paint_band_for_doc(doc, self.width, view_h);
                    let paint_top = band.y;
                    let paint_bottom = band.bottom();
                    let list = build_display_list_viewport(
                        &doc.root,
                        self.width,
                        view_h,
                        doc.scroll_x,
                        doc.scroll_y,
                        paint_top,
                        paint_bottom,
                        0,
                        0,
                        &std::collections::HashSet::new(),
                        &doc.base_url,
                    );
                    display_list_stats_json(
                        &list,
                        Some((paint_top, paint_bottom)),
                        self.width,
                        crate::Document::scroll_height(&doc.root),
                    )
                } else {
                    r#"{"ok":false,"error":"no document"}"#.to_string()
                }
            }
            "animated-images" => {
                if let Some(doc) = self.tabs[self.active].view.document().as_deref() {
                    animated_images_json(doc, self.content_h())
                } else {
                    r#"{"ok":false,"error":"no document"}"#.to_string()
                }
            }
            "image-states" => {
                if let Some(doc) = self.tabs[self.active].view.document().as_deref() {
                    let limit = dbg_json_num(line, "limit").unwrap_or(80.0).max(1.0) as usize;
                    image_states_json(doc, limit)
                } else {
                    r#"{"ok":false,"error":"no document"}"#.to_string()
                }
            }
            "animations" => {
                if let Some(doc) = self.tabs[self.active].view.document().as_deref() {
                    animations_json(doc, self.content_h())
                } else {
                    r#"{"ok":false,"error":"no document"}"#.to_string()
                }
            }
            "paint-dump" => {
                let x = dbg_json_num(line, "x").unwrap_or(0.0);
                let y = dbg_json_num(line, "y").unwrap_or(0.0);
                let w = dbg_json_num(line, "w").unwrap_or(self.width);
                let h = dbg_json_num(line, "h").unwrap_or(self.content_h());
                let limit = dbg_json_num(line, "limit").unwrap_or(80.0).max(1.0) as usize;
                let qx2 = x + w.max(0.0);
                let qy2 = y + h.max(0.0);
                let mut out = Vec::new();
                let view_w = self.width;
                let view_h = self.content_h();
                if let Some((doc, renderer)) =
                    self.tabs[self.active].view.document_and_renderer_mut()
                {
                    let font_system = Some(&mut renderer.font_system as *mut _);
                    let list = build_display_list_full_with_font_system(
                        &doc.root,
                        view_w,
                        view_h,
                        doc.scroll_x,
                        doc.scroll_y,
                        0,
                        0,
                        &std::collections::HashSet::new(),
                        &doc.base_url,
                        font_system,
                    );
                    for cmd in &list.commands {
                        if out.len() >= limit {
                            break;
                        }
                        match cmd {
                            PaintCmd::FillRect {
                                rect,
                                color,
                                radius,
                                radius_y,
                            } => {
                                if rect.x <= qx2
                                    && rect.right() >= x
                                    && rect.y <= qy2
                                    && rect.bottom() >= y
                                    && color.a > 0
                                {
                                    out.push(format!(
                                        r##"{{"kind":"fill","x":{:.1},"y":{:.1},"w":{:.1},"h":{:.1},"color":"#{:02x}{:02x}{:02x}{:02x}","radius":[{:.1},{:.1},{:.1},{:.1}],"radius_y":[{:.1},{:.1},{:.1},{:.1}]}}"##,
                                        rect.x, rect.y, rect.w, rect.h,
                                        color.r, color.g, color.b, color.a,
                                        radius[0], radius[1], radius[2], radius[3],
                                        radius_y[0], radius_y[1], radius_y[2], radius_y[3]
                                    ));
                                }
                            }
                            PaintCmd::Border {
                                rect,
                                widths,
                                colors,
                                styles,
                                ..
                            } => {
                                if rect.x <= qx2
                                    && rect.right() >= x
                                    && rect.y <= qy2
                                    && rect.bottom() >= y
                                    && widths.iter().any(|w| *w > 0.0)
                                {
                                    out.push(format!(
                                        r##"{{"kind":"border","x":{:.1},"y":{:.1},"w":{:.1},"h":{:.1},"widths":[{:.1},{:.1},{:.1},{:.1}],"colors":["#{:02x}{:02x}{:02x}{:02x}","#{:02x}{:02x}{:02x}{:02x}","#{:02x}{:02x}{:02x}{:02x}","#{:02x}{:02x}{:02x}{:02x}"],"styles":[{},{},{},{}]}}"##,
                                        rect.x, rect.y, rect.w, rect.h,
                                        widths[0], widths[1], widths[2], widths[3],
                                        colors[0].r, colors[0].g, colors[0].b, colors[0].a,
                                        colors[1].r, colors[1].g, colors[1].b, colors[1].a,
                                        colors[2].r, colors[2].g, colors[2].b, colors[2].a,
                                        colors[3].r, colors[3].g, colors[3].b, colors[3].a,
                                        styles[0], styles[1], styles[2], styles[3]
                                    ));
                                }
                            }
                            PaintCmd::Outline {
                                rect,
                                width,
                                color,
                                style,
                                offset,
                                ..
                            } => {
                                if rect.x <= qx2
                                    && rect.right() >= x
                                    && rect.y <= qy2
                                    && rect.bottom() >= y
                                    && *width > 0.0
                                    && color.a > 0
                                {
                                    out.push(format!(
                                        r##"{{"kind":"outline","x":{:.1},"y":{:.1},"w":{:.1},"h":{:.1},"width":{:.1},"color":"#{:02x}{:02x}{:02x}{:02x}","style":{},"offset":{:.1}}}"##,
                                        rect.x, rect.y, rect.w, rect.h,
                                        width,
                                        color.r, color.g, color.b, color.a,
                                        style,
                                        offset
                                    ));
                                }
                            }
                            PaintCmd::Text {
                                x: tx,
                                y: ty,
                                text,
                                font_family,
                                font_size,
                                font_weight,
                                decoration,
                                letter_spacing,
                                word_spacing,
                                ..
                            } => {
                                if *tx <= qx2
                                    && *tx + 1200.0 >= x
                                    && *ty <= qy2
                                    && *ty + *font_size >= y
                                {
                                    out.push(format!(
                                        r#"{{"kind":"text","x":{:.1},"y":{:.1},"font":{},"size":{:.1},"weight":{},"letter_spacing":{:.3},"word_spacing":{:.3},"underline":{},"overline":{},"strikethrough":{},"text":{}}}"#,
                                        tx,
                                        ty,
                                        dbg_json_escape(font_family),
                                        font_size,
                                        font_weight,
                                        letter_spacing,
                                        word_spacing,
                                        decoration.underline,
                                        decoration.overline,
                                        decoration.strikethrough,
                                        dbg_json_escape(&text.chars().take(120).collect::<String>())
                                    ));
                                }
                            }
                            PaintCmd::TextShadow {
                                x: tx,
                                y: ty,
                                text,
                                font_family,
                                font_size,
                                ..
                            } => {
                                if *tx <= qx2
                                    && *tx + 1200.0 >= x
                                    && *ty <= qy2
                                    && *ty + *font_size >= y
                                {
                                    out.push(format!(
                                        r#"{{"kind":"text-shadow","x":{:.1},"y":{:.1},"font":{},"size":{:.1},"text":{}}}"#,
                                        tx,
                                        ty,
                                        dbg_json_escape(font_family),
                                        font_size,
                                        dbg_json_escape(&text.chars().take(120).collect::<String>())
                                    ));
                                }
                            }
                            PaintCmd::Image { rect, data } => {
                                if rect.x <= qx2
                                    && rect.right() >= x
                                    && rect.y <= qy2
                                    && rect.bottom() >= y
                                {
                                    let stats = paint_dump_image_stats_json(data);
                                    out.push(format!(
                                        r#"{{"kind":"image","x":{:.1},"y":{:.1},"w":{:.1},"h":{:.1},{}}}"#,
                                        rect.x, rect.y, rect.w, rect.h, stats
                                    ));
                                }
                            }
                            PaintCmd::BackgroundImage {
                                container,
                                clip,
                                draw_w,
                                draw_h,
                                pos_x,
                                pos_y,
                                ..
                            } => {
                                if clip.x <= qx2
                                    && clip.right() >= x
                                    && clip.y <= qy2
                                    && clip.bottom() >= y
                                {
                                    out.push(format!(
                                        r#"{{"kind":"background-image","container":[{:.1},{:.1},{:.1},{:.1}],"clip":[{:.1},{:.1},{:.1},{:.1}],"draw_w":{:.1},"draw_h":{:.1},"pos_x":{:.1},"pos_y":{:.1}}}"#,
                                        container.x, container.y, container.w, container.h,
                                        clip.x, clip.y, clip.w, clip.h,
                                        draw_w, draw_h, pos_x, pos_y
                                    ));
                                }
                            }
                            PaintCmd::Gradient {
                                rect,
                                clip,
                                gradient_type,
                                angle,
                                stops,
                                ..
                            } => {
                                if clip.x <= qx2
                                    && clip.right() >= x
                                    && clip.y <= qy2
                                    && clip.bottom() >= y
                                {
                                    out.push(format!(
                                        r#"{{"kind":"gradient","x":{:.1},"y":{:.1},"w":{:.1},"h":{:.1},"clip":[{:.1},{:.1},{:.1},{:.1}],"gradient_type":{},"angle":{:.1},"stops":{},"resolved_stops":{}}}"#,
                                        rect.x, rect.y, rect.w, rect.h,
                                        clip.x, clip.y, clip.w, clip.h,
                                        gradient_type, angle, stops.len(),
                                        format!("[{}]", stops.iter().map(|(color, position)|
                                            format!(r#"{{"rgba":[{},{},{},{}],"position":{}}}"#, color.r, color.g, color.b, color.a, position)
                                        ).collect::<Vec<_>>().join(","))
                                    ));
                                }
                            }
                            PaintCmd::BoxShadow {
                                rect,
                                color,
                                offset_x,
                                offset_y,
                                blur,
                                spread,
                                inset,
                                ..
                            } => {
                                let sx = rect.x + offset_x - spread - blur * 4.0;
                                let sy = rect.y + offset_y - spread - blur * 4.0;
                                let sw = rect.w + spread * 2.0 + blur * 8.0;
                                let sh = rect.h + spread * 2.0 + blur * 8.0;
                                if sx <= qx2 && sx + sw >= x && sy <= qy2 && sy + sh >= y {
                                    out.push(format!(
                                        r##"{{"kind":"box-shadow","x":{:.1},"y":{:.1},"w":{:.1},"h":{:.1},"color":"#{:02x}{:02x}{:02x}{:02x}","offset_x":{:.1},"offset_y":{:.1},"blur":{:.1},"spread":{:.1},"inset":{}}}"##,
                                        rect.x, rect.y, rect.w, rect.h,
                                        color.r, color.g, color.b, color.a,
                                        offset_x, offset_y, blur, spread, inset
                                    ));
                                }
                            }
                            PaintCmd::BeginStackingContext { node_id, z_index } => {
                                out.push(format!(
                                    r#"{{"kind":"begin-stacking-context","node_id":{},"z_index":{}}}"#,
                                    node_id, z_index
                                ));
                            }
                            PaintCmd::EndStackingContext => {
                                out.push(r#"{"kind":"end-stacking-context"}"#.to_string());
                            }
                            PaintCmd::PushClip {
                                rect,
                                radius,
                                radius_y,
                            } => {
                                if rect.x <= qx2
                                    && rect.right() >= x
                                    && rect.y <= qy2
                                    && rect.bottom() >= y
                                {
                                    out.push(format!(
                                        r#"{{"kind":"push-clip","x":{:.1},"y":{:.1},"w":{:.1},"h":{:.1},"radius":[{:.1},{:.1},{:.1},{:.1}],"radius_y":[{:.1},{:.1},{:.1},{:.1}]}}"#,
                                        rect.x, rect.y, rect.w, rect.h,
                                        radius[0], radius[1], radius[2], radius[3],
                                        radius_y[0], radius_y[1], radius_y[2], radius_y[3]
                                    ));
                                }
                            }
                            PaintCmd::PushClipPath { points, even_odd } => {
                                out.push(format!(
                                    r#"{{"kind":"push-clip-path","points":{},"even_odd":{}}}"#,
                                    points.len(),
                                    even_odd
                                ));
                            }
                            PaintCmd::PushClipSvgPath {
                                path,
                                fill_rule,
                                origin,
                            } => {
                                out.push(format!(
                                    r#"{{"kind":"push-clip-svg-path","segments":{},"fill_rule":"{:?}","x":{:.1},"y":{:.1}}}"#,
                                    path.len(), fill_rule, origin.0, origin.1
                                ));
                            }
                            PaintCmd::PushTransform { node_id, transform } => {
                                out.push(format!(
                                    r#"{{"kind":"push-transform","node_id":{},"matrix":[{:.4},{:.4},{:.4},{:.4},{:.1},{:.1}]}}"#,
                                    node_id,
                                    transform[0], transform[1], transform[2], transform[3],
                                    transform[4], transform[5]
                                ));
                            }
                            PaintCmd::PopTransform => {
                                out.push(r#"{"kind":"pop-transform"}"#.to_string());
                            }
                            PaintCmd::PushOpacity { alpha } => {
                                out.push(format!(
                                    r#"{{"kind":"push-opacity","alpha":{:.3}}}"#,
                                    alpha
                                ));
                            }
                            PaintCmd::PopOpacity => {
                                out.push(r#"{"kind":"pop-opacity"}"#.to_string());
                            }
                            PaintCmd::PushFilter { filters } => {
                                out.push(format!(
                                    r#"{{"kind":"push-filter","filters":{}}}"#,
                                    filters.len()
                                ));
                            }
                            PaintCmd::PopFilter => {
                                out.push(r#"{"kind":"pop-filter"}"#.to_string());
                            }
                            PaintCmd::BackdropFilter { rect, filters, .. } => {
                                if rect.x <= qx2
                                    && rect.right() >= x
                                    && rect.y <= qy2
                                    && rect.bottom() >= y
                                {
                                    out.push(format!(
                                        r#"{{"kind":"backdrop-filter","x":{:.1},"y":{:.1},"w":{:.1},"h":{:.1},"filters":{}}}"#,
                                        rect.x, rect.y, rect.w, rect.h, filters.len()
                                    ));
                                }
                            }
                            PaintCmd::PushMask { rect, data, .. } => {
                                if rect.x <= qx2
                                    && rect.right() >= x
                                    && rect.y <= qy2
                                    && rect.bottom() >= y
                                {
                                    let (mw, mh) = match data {
                                        ImageRef::Owned(_, w, h) | ImageRef::Shared(_, w, h) => {
                                            (*w, *h)
                                        }
                                    };
                                    out.push(format!(
                                        r#"{{"kind":"push-mask","x":{:.1},"y":{:.1},"w":{:.1},"h":{:.1},"mask_w":{},"mask_h":{}}}"#,
                                        rect.x, rect.y, rect.w, rect.h, mw, mh
                                    ));
                                }
                            }
                            PaintCmd::PopMask => {
                                out.push(r#"{"kind":"pop-mask"}"#.to_string());
                            }
                            PaintCmd::PushBlendMode { mode } => {
                                out.push(format!(
                                    r#"{{"kind":"push-blend-mode","mode":{}}}"#,
                                    mode
                                ));
                            }
                            PaintCmd::PopBlendMode => {
                                out.push(r#"{"kind":"pop-blend-mode"}"#.to_string());
                            }
                            PaintCmd::PopClip => {
                                // Standalone pop clips are technically in the global display list,
                                // but in a filtered dump they hide the commands we are trying to
                                // inspect. Keep range dumps focused on drawable/entering commands.
                            }
                            _ => {}
                        }
                    }
                }
                format!(
                    r#"{{"ok":true,"count":{},"commands":[{}]}}"#,
                    out.len(),
                    out.join(",")
                )
            }
            "rule-search" => {
                let query = dbg_json_str(line, "query").unwrap_or_default();
                let limit = dbg_json_num(line, "limit").unwrap_or(20.0).max(1.0) as usize;
                let mut matches = Vec::new();
                if let Some(doc) = self.tabs[self.active].view.document().as_deref() {
                    for rule in &doc.stylesheet.rules {
                        if matches.len() >= limit {
                            break;
                        }
                        if rule.original_selector.contains(&query) {
                            let decls: Vec<String> = rule
                                .declarations
                                .iter()
                                .filter(|(k, _)| !k.starts_with("--"))
                                .map(|(k, v)| {
                                    format!("{}:{}", dbg_json_escape(k), dbg_json_escape(v))
                                })
                                .collect();
                            let selector_ast = format!("{:?}", rule.selectors);
                            let scope_selector = format!("{:?}", rule.scope_selector);
                            let scope_limit_selector = format!("{:?}", rule.scope_limit_selector);
                            let scopes = format!("{:?}", rule.scopes);
                            matches.push(format!(
                                r#"{{"selector":{},"selector_ast":{},"pseudo":"{:?}","specificity":{},"layer":{},"layer_rank":{},"media":{},"scope":{},"scope_limit":{},"scopes":{},"compiled":{},"compiled_important":{},"declarations":{{{}}}}}"#,
                                dbg_json_escape(&rule.original_selector),
                                dbg_json_escape(&selector_ast),
                                rule.pseudo_element,
                                rule.specificity,
                                dbg_json_escape(&rule.layer),
                                rule.layer_rank,
                            dbg_json_escape(&rule.media_condition.label()),
                                dbg_json_escape(&scope_selector),
                                dbg_json_escape(&scope_limit_selector),
                                dbg_json_escape(&scopes),
                                rule.compiled_decls.len(),
                                rule.compiled_important.len(),
                                decls.join(",")
                            ));
                        }
                    }
                }
                format!(
                    r#"{{"ok":true,"query":{},"count":{},"rules":[{}]}}"#,
                    dbg_json_escape(&query),
                    matches.len(),
                    matches.join(",")
                )
            }
            "style-debug" => match dbg_json_str(line, "selector") {
                Some(sel) => {
                    let limit = dbg_json_num(line, "limit").unwrap_or(20.0).max(1.0) as usize;
                    let query = dbg_json_str(line, "query").unwrap_or_default();
                    let mut parts = Vec::new();
                    if let Some(doc) = self.tabs[self.active].view.document().as_deref() {
                        Document::walk_all(&doc.root, &mut |node| {
                            if parts.len() >= limit {
                                return;
                            }
                            if dbg_matches_query(doc, node, &sel) {
                                let id = node.attributes.get("id").map(|s| s.as_str());
                                let class_attr = node
                                    .attributes
                                    .get("class")
                                    .map(|s| s.as_str())
                                    .unwrap_or("");
                                let classes: Vec<&str> = class_attr.split_whitespace().collect();
                                let mut candidates = Vec::new();
                                doc.stylesheet.candidate_rules(
                                    &node.tag,
                                    id,
                                    &classes,
                                    &mut candidates,
                                );
                                let candidate_selectors = candidates
                                    .iter()
                                    .take(24)
                                    .filter_map(|idx| doc.stylesheet.rules.get(*idx))
                                    .map(|rule| dbg_json_escape(&rule.original_selector))
                                    .collect::<Vec<_>>();
                                let filtered_candidates = if query.is_empty() {
                                    Vec::new()
                                } else {
                                    candidates
                                        .iter()
                                        .filter_map(|idx| doc.stylesheet.rules.get(*idx))
                                        .filter(|rule| rule.original_selector.contains(&query))
                                        .map(|rule| dbg_json_escape(&rule.original_selector))
                                        .collect::<Vec<_>>()
                                };
                                parts.push(format!(
                                    r#"{{"node_id":{},"tag":{},"id":{},"class":{},"candidate_count":{},"candidates":[{}],"filtered_candidates":[{}]}}"#,
                                    node.node_id,
                                    dbg_json_escape(&node.tag),
                                    dbg_json_escape(id.unwrap_or("")),
                                    dbg_json_escape(class_attr),
                                    candidates.len(),
                                    candidate_selectors.join(","),
                                    filtered_candidates.join(",")
                                ));
                            }
                        });
                    }
                    format!(
                        r#"{{"ok":true,"count":{},"elements":[{}]}}"#,
                        parts.len(),
                        parts.join(",")
                    )
                }
                None => r#"{"ok":false,"error":"style-debug needs selector"}"#.to_string(),
            },
            "match-debug" => match dbg_json_str(line, "selector") {
                Some(sel) => {
                    let mut parts = Vec::new();
                    let limit = dbg_json_num(line, "limit").unwrap_or(10.0).max(1.0) as usize;
                    if let Some(doc) = self.tabs[self.active].view.document().as_deref() {
                        Document::walk_all(&doc.root, &mut |node| {
                            if parts.len() >= limit {
                                return;
                            }
                            if dbg_matches_query(doc, node, &sel) {
                                parts.push(dbg_match_report_json(doc, node));
                            }
                        });
                    }
                    format!(
                        r#"{{"ok":true,"count":{},"elements":[{}]}}"#,
                        parts.len(),
                        parts.join(",")
                    )
                }
                None => r#"{"ok":false,"error":"match-debug needs selector"}"#.to_string(),
            },
            "node-id-stats" => {
                let mut total = 0usize;
                let mut zero = 0usize;
                let mut ids = std::collections::HashMap::<u32, usize>::new();
                if let Some(doc) = self.tabs[self.active].view.document().as_deref() {
                    Document::walk_all(&doc.root, &mut |node| {
                        total += 1;
                        if node.node_id == 0 {
                            zero += 1;
                        } else {
                            *ids.entry(node.node_id).or_insert(0) += 1;
                        }
                    });
                }
                let mut duplicates = ids
                    .into_iter()
                    .filter(|(_, count)| *count > 1)
                    .collect::<Vec<_>>();
                duplicates.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
                let dup_json = duplicates
                    .iter()
                    .take(20)
                    .map(|(id, count)| format!(r#"{{"id":{},"count":{}}}"#, id, count))
                    .collect::<Vec<_>>()
                    .join(",");
                format!(
                    r#"{{"ok":true,"total":{},"zero":{},"unique":{},"duplicate_ids":{},"duplicates":[{}]}}"#,
                    total,
                    zero,
                    total.saturating_sub(zero).saturating_sub(
                        duplicates
                            .iter()
                            .map(|(_, count)| count.saturating_sub(1))
                            .sum::<usize>()
                    ),
                    duplicates.len(),
                    dup_json
                )
            }
            "keyframes" => {
                let query = dbg_json_str(line, "query").unwrap_or_default();
                let limit = dbg_json_num(line, "limit").unwrap_or(20.0).max(1.0) as usize;
                let mut matches = Vec::new();
                if let Some(doc) = self.tabs[self.active].view.document().as_deref() {
                    for (name, stops) in &doc.stylesheet.keyframes {
                        if matches.len() >= limit {
                            break;
                        }
                        if !query.is_empty() && !name.contains(&query) {
                            continue;
                        }
                        let stop_json: Vec<String> = stops
                            .iter()
                            .map(|stop| {
                                let props: Vec<String> = stop
                                    .properties
                                    .iter()
                                    .map(|(k, v)| {
                                        format!("{}:{}", dbg_json_escape(k), dbg_json_escape(v))
                                    })
                                    .collect();
                                format!(
                                    r#"{{"offset":{:.6},"properties":{{{}}}}}"#,
                                    stop.offset,
                                    props.join(",")
                                )
                            })
                            .collect();
                        matches.push(format!(
                            r#"{{"name":{},"stops":[{}]}}"#,
                            dbg_json_escape(name),
                            stop_json.join(",")
                        ));
                    }
                } else {
                    return r#"{"ok":false,"error":"no document"}"#.to_string();
                }
                format!(
                    r#"{{"ok":true,"query":{},"count":{},"keyframes":[{}]}}"#,
                    dbg_json_escape(&query),
                    matches.len(),
                    matches.join(",")
                )
            }
            "rules" | "matched-rules" => match dbg_json_str(line, "selector") {
                Some(sel) => {
                    let mut results = Vec::new();
                    if let Some(doc) = self.tabs[self.active].view.document().as_deref() {
                        Document::walk_all(&doc.root, &mut |node| {
                            if dbg_matches_query(doc, node, &sel) {
                                let rules: Vec<String> = node.matched_rules.iter().map(|r| {
                                        let decls: Vec<String> = r.declarations.iter()
                                            .filter(|(k, _)| !k.starts_with("--"))
                                            .map(|(k, v)| format!("{}:{}", dbg_json_escape(k), dbg_json_escape(v)))
                                            .collect();
                                        format!(r#"{{"selector":{},"specificity":{},"source":{},"layer":{},"layer_rank":{},"declarations":{{{}}}}}"#,
                                            dbg_json_escape(&r.selector), r.specificity,
                                            dbg_json_escape(&r.source),
                                            dbg_json_escape(&r.layer), r.layer_rank, decls.join(","))
                                    }).collect();
                                let id =
                                    node.attributes.get("id").map(|v| v.as_str()).unwrap_or("");
                                let cls = node
                                    .attributes
                                    .get("class")
                                    .map(|v| v.as_str())
                                    .unwrap_or("");
                                results.push(format!(
                                    r#"{{"tag":{},"id":{},"class":{},"rules":[{}]}}"#,
                                    dbg_json_escape(&node.tag),
                                    dbg_json_escape(id),
                                    dbg_json_escape(cls),
                                    rules.join(",")
                                ));
                            }
                        });
                    }
                    format!(
                        r#"{{"ok":true,"count":{},"elements":[{}]}}"#,
                        results.len(),
                        results.join(",")
                    )
                }
                None => r#"{"ok":false,"error":"rules needs selector"}"#.to_string(),
            },
            // ── Mutation ─────────────────────────────────────────────────────
            "setstyle" => {
                match (
                    dbg_json_str(line, "selector"),
                    dbg_json_str(line, "prop"),
                    dbg_json_str(line, "value"),
                ) {
                    (Some(sel), Some(prop), Some(val)) => {
                        let mut count = 0usize;
                        if let Some(doc) = self.tabs[self.active].view.document_mut().as_deref_mut()
                        {
                            let hits = dbg_query_ids(doc, &sel);
                            for id in hits {
                                doc.set_style_property(id, &prop, &val);
                                count += 1;
                            }
                        }
                        if count > 0 {
                            self.relayout_active();
                        }
                        format!(r#"{{"ok":true,"modified":{}}}"#, count)
                    }
                    _ => {
                        r#"{"ok":false,"error":"setstyle needs selector, prop, value"}"#.to_string()
                    }
                }
            }
            "setattr" => {
                match (
                    dbg_json_str(line, "selector"),
                    dbg_json_str(line, "name"),
                    dbg_json_str(line, "value"),
                ) {
                    (Some(sel), Some(name), Some(val)) => {
                        let mut count = 0usize;
                        if let Some(doc) = self.tabs[self.active].view.document_mut().as_deref_mut()
                        {
                            let hits = dbg_query_ids(doc, &sel);
                            for id in hits {
                                doc.set_attribute(id, &name, &val);
                                count += 1;
                            }
                        }
                        if count > 0 {
                            self.relayout_active();
                        }
                        format!(r#"{{"ok":true,"modified":{}}}"#, count)
                    }
                    _ => {
                        r#"{"ok":false,"error":"setattr needs selector, name, value"}"#.to_string()
                    }
                }
            }
            // ── Tree ─────────────────────────────────────────────────────────
            "tree" => {
                let sel = dbg_json_str(line, "selector");
                let mut buf = String::new();
                if let Some(doc) = self.tabs[self.active].view.document().as_deref() {
                    match sel.as_deref() {
                        Some(sel) => {
                            let hit_ids: std::collections::HashSet<u32> =
                                dbg_select_composed_with_pseudo(&doc.root, doc, sel)
                                    .into_iter()
                                    .map(|hit| hit.node_id)
                                    .collect();
                            dbg_walk_composed(&doc.root, &mut |node| {
                                if hit_ids.contains(&node.node_id) {
                                    dbg_dump_box(0, node, &mut buf);
                                }
                            });
                        }
                        None => dbg_dump_box(0, &doc.root, &mut buf),
                    }
                }
                format!(r#"{{"ok":true,"tree":{}}}"#, dbg_json_escape(&buf))
            }
            // ── Highlight ────────────────────────────────────────────────────
            "highlight" => match dbg_json_str(line, "selector") {
                Some(sel) => {
                    let path =
                        dbg_json_str(line, "out").unwrap_or_else(|| "highlight.png".to_string());
                    let scale = dbg_json_num(line, "scale")
                        .map(|v| v.clamp(0.25, 4.0) as f32)
                        .unwrap_or(1.0);
                    let view_w = self.width;
                    let view_h = self.content_h();
                    let phys_w = ((view_w as f32) * scale).ceil() as u32;
                    let ch = ((view_h as f32) * scale).ceil() as u32;
                    let Some(mut pm) = tiny_skia::Pixmap::new(phys_w.max(1), ch.max(1)) else {
                        return r#"{"ok":false,"error":"pixmap alloc failed"}"#.to_string();
                    };
                    pm.fill(tiny_skia::Color::WHITE);
                    let mut count = 0usize;
                    self.tabs[self.active].view.resize(view_w, view_h);
                    self.tabs[self.active].view.paint_into(&mut pm, 0, 0, scale);
                    if let Some(doc) = self.tabs[self.active].view.document().as_deref() {
                        Document::walk_all(&doc.root, &mut |node| {
                            if dbg_matches_query(doc, node, &sel) {
                                crate::draw_inspect_overlay(node, &mut pm, 0.0, 0.0, scale);
                                count += 1;
                            }
                        });
                    }
                    match pm.save_png(&path) {
                        Ok(_) => format!(
                            r#"{{"ok":true,"path":{},"highlighted":{}}}"#,
                            dbg_json_escape(&path),
                            count
                        ),
                        Err(e) => format!(
                            r#"{{"ok":false,"error":{}}}"#,
                            dbg_json_escape(&e.to_string())
                        ),
                    }
                }
                None => r#"{"ok":false,"error":"highlight needs selector"}"#.to_string(),
            },
            // ── Misc ─────────────────────────────────────────────────────────
            "perf" => {
                let url = self.tabs[self.active].url.clone();
                let loading = self.tabs[self.active].loading;
                format!(
                    r#"{{"ok":true,"active_tab":{},"loading":{},"tabs":{}}}"#,
                    dbg_json_escape(&url),
                    loading,
                    self.tabs.len()
                )
            }
            "resource-states" => {
                let document = self.tabs[self.active].view.document();
                let Some(doc) = document.as_deref() else {
                    return r#"{"ok":false,"error":"no document"}"#.to_string();
                };
                format!(
                    r#"{{"ok":true,"loading":{},"pending_css":{},"pending_images":{},"images_in_flight":{},"css_rules":{},"linked_stylesheets":{},"loaded_stylesheet_slots":{},"doc_height":{:.0}}}"#,
                    self.tabs[self.active].loading,
                    doc.pending_stylesheets.is_some(),
                    doc.pending_images.is_some(),
                    doc.images_in_flight
                        .load(std::sync::atomic::Ordering::SeqCst),
                    doc.stylesheet.rules.len(),
                    doc.linked_stylesheets.len(),
                    doc.loaded_stylesheet_slots.len(),
                    crate::Document::scroll_height(&doc.root),
                )
            }
            "font-faces" => {
                let Some((mut document, renderer)) =
                    self.tabs[self.active].view.document_and_renderer_mut()
                else {
                    return r#"{"ok":false,"error":"no document"}"#.to_string();
                };
                let doc = &mut *document;
                let faces: Vec<_> = doc
                    .stylesheet
                    .font_faces
                    .iter()
                    .map(|face| {
                        let family = face.family.trim().trim_matches(['\'', '"']);
                        let query = fontdb::Query {
                            families: &[fontdb::Family::Name(family)],
                            weight: fontdb::Weight::NORMAL,
                            stretch: fontdb::Stretch::Normal,
                            style: fontdb::Style::Normal,
                        };
                        format!(
                            r#"{{"family":{},"src":{},"available":{}}}"#,
                            dbg_json_escape(family),
                            dbg_json_escape(&face.src),
                            renderer.font_system.db().query(&query).is_some(),
                        )
                    })
                    .collect();
                format!(
                    r#"{{"ok":true,"count":{},"faces":[{}]}}"#,
                    faces.len(),
                    faces.join(",")
                )
            }
            "stylesheet-slots" => {
                let document = self.tabs[self.active].view.document();
                let Some(doc) = document.as_deref() else {
                    return r#"{"ok":false,"error":"no document"}"#.to_string();
                };
                let mut slots = Vec::new();
                for (idx, sheet) in doc.document_stylesheets.iter().enumerate() {
                    match sheet {
                        crate::types::DocumentStylesheet::Inline { css, .. } => {
                            slots.push(format!(
                                r#"{{"idx":{},"kind":"inline","css_len":{},"loaded_rules":null}}"#,
                                idx,
                                css.len()
                            ));
                        }
                        crate::types::DocumentStylesheet::Linked { href, media } => {
                            let abs = crate::html::resolve_url(href, &doc.base_url);
                            let slot_rules = doc
                                .loaded_stylesheet_slots
                                .get(&idx)
                                .map(|sheet| sheet.rules.len());
                            let href_rules = doc
                                .loaded_linked_stylesheets
                                .get(&abs)
                                .map(|sheet| sheet.rules.len());
                            slots.push(format!(
                                r#"{{"idx":{},"kind":"linked","href":{},"resolved":{},"media":{},"slot_rules":{},"href_rules":{}}}"#,
                                idx,
                                dbg_json_escape(href),
                                dbg_json_escape(&abs),
                                dbg_json_escape(media),
                                slot_rules
                                    .map(|n| n.to_string())
                                    .unwrap_or_else(|| "null".to_string()),
                                href_rules
                                    .map(|n| n.to_string())
                                    .unwrap_or_else(|| "null".to_string()),
                            ));
                        }
                    }
                }
                format!(
                    r#"{{"ok":true,"base_url":{},"document_stylesheets":{},"linked_stylesheets":{},"loaded_slots":{},"loaded_hrefs":{},"slots":[{}]}}"#,
                    dbg_json_escape(&doc.base_url),
                    doc.document_stylesheets.len(),
                    doc.linked_stylesheets.len(),
                    doc.loaded_stylesheet_slots.len(),
                    doc.loaded_linked_stylesheets.len(),
                    slots.join(",")
                )
            }
            "resolve-css" => {
                let document = self.tabs[self.active].view.document();
                let Some(doc) = document.as_deref() else {
                    return r#"{"ok":false,"error":"no document"}"#.to_string();
                };
                let value = dbg_json_str(line, "value").unwrap_or_default();
                let resolved =
                    crate::css::resolve_var_references(&value, &doc.stylesheet.variables);
                format!(
                    r#"{{"ok":true,"value":{},"resolved":{},"variables":{}}}"#,
                    dbg_json_escape(&value),
                    dbg_json_escape(&resolved),
                    doc.stylesheet.variables.len()
                )
            }
            "stylesheet-vars" => {
                let document = self.tabs[self.active].view.document();
                let Some(doc) = document.as_deref() else {
                    return r#"{"ok":false,"error":"no document"}"#.to_string();
                };
                let query = dbg_json_str(line, "query").unwrap_or_default();
                let limit = dbg_json_num(line, "limit").unwrap_or(40.0).max(1.0) as usize;
                let mut vars = doc
                    .stylesheet
                    .variables
                    .iter()
                    .filter(|(name, value)| {
                        query.is_empty() || name.contains(&query) || value.contains(&query)
                    })
                    .collect::<Vec<_>>();
                vars.sort_by(|(a, _), (b, _)| a.cmp(b));
                let matched_count = vars.len();
                let entries = vars
                    .into_iter()
                    .take(limit)
                    .map(|(name, value)| {
                        format!(
                            r#"{{"name":{},"value":{}}}"#,
                            dbg_json_escape(name),
                            dbg_json_escape(value)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                format!(
                    r#"{{"ok":true,"query":{},"variables":{},"count":{},"entries":[{}]}}"#,
                    dbg_json_escape(&query),
                    doc.stylesheet.variables.len(),
                    matched_count,
                    entries
                )
            }
            "shadow-styles" => {
                let document = self.tabs[self.active].view.document();
                let Some(doc) = document.as_deref() else {
                    return r#"{"ok":false,"error":"no document"}"#.to_string();
                };
                fn walk_shadow_styles(
                    node: &crate::WebCore,
                    doc: &crate::Document,
                    out: &mut Vec<String>,
                ) {
                    if let Some(sr) = node.shadow_root.as_ref() {
                        let mut sheets = Vec::new();
                        for (idx, sheet) in sr.document_stylesheets.iter().enumerate() {
                            match sheet {
                                crate::types::DocumentStylesheet::Inline { css, .. } => {
                                    sheets.push(format!(
                                        r#"{{"idx":{},"kind":"inline","css_len":{}}}"#,
                                        idx,
                                        css.len()
                                    ));
                                }
                                crate::types::DocumentStylesheet::Linked { href, media } => {
                                    let abs = crate::html::resolve_url(href, &doc.base_url);
                                    let rules = doc
                                        .loaded_linked_stylesheets
                                        .get(&abs)
                                        .or_else(|| doc.loaded_linked_stylesheets.get(href))
                                        .map(|sheet| sheet.rules.len());
                                    sheets.push(format!(
                                        r#"{{"idx":{},"kind":"linked","href":{},"resolved":{},"media":{},"loaded_rules":{}}}"#,
                                        idx,
                                        dbg_json_escape(href),
                                        dbg_json_escape(&abs),
                                        dbg_json_escape(media),
                                        rules
                                            .map(|n| n.to_string())
                                            .unwrap_or_else(|| "null".to_string())
                                    ));
                                }
                            }
                        }
                        out.push(format!(
                            r#"{{"host_tag":{},"host_id":{},"host_class":{},"host_node_id":{},"shadow_node_id":{},"rules":{},"sheets":[{}]}}"#,
                            dbg_json_escape(&node.tag),
                            dbg_json_escape(node.attributes.get("id").map(String::as_str).unwrap_or("")),
                            dbg_json_escape(node.attributes.get("class").map(String::as_str).unwrap_or("")),
                            node.node_id,
                            sr.node_id,
                            sr.stylesheet.rules.len(),
                            sheets.join(",")
                        ));
                        for child in &sr.children {
                            walk_shadow_styles(child, doc, out);
                        }
                    }
                    for child in &node.children {
                        walk_shadow_styles(child, doc, out);
                    }
                }
                let mut roots = Vec::new();
                walk_shadow_styles(&doc.root, doc, &mut roots);
                format!(
                    r#"{{"ok":true,"count":{},"roots":[{}]}}"#,
                    roots.len(),
                    roots.join(",")
                )
            }
            "memory-stats" => {
                let stats = self.tabs[self.active].view.memory_stats();
                let process = process_memory_stats();
                format!(
                    r#"{{"ok":true,"process_rss_bytes":{},"process_vsz_bytes":{},"history_cache_entries":{},"history_cache_bytes":{},"viewport_surface_bytes":{},"renderer_cached_content_surface_bytes":{},"renderer_cached_surface_bytes":{},"tile_surface_bytes":{},"tile_count":{},"display_list_commands":{},"display_list_estimated_bytes":{},"display_list_inline_bytes":{},"display_list_heap_bytes":{},"display_list_text_bytes":{},"display_list_image_bytes":{},"display_list_vector_bytes":{},"raw_resource_cache_entries":{},"raw_resource_cache_bytes":{},"parsed_css_cache_entries":{},"parsed_css_cache_bytes":{},"decoded_image_cache_entries":{},"decoded_image_cache_bytes":{},"dom_nodes":{},"image_nodes":{},"unique_styles":{},"dom_estimated_bytes":{},"layout_estimated_bytes":{},"style_estimated_bytes":{},"line_cache_estimated_bytes":{},"matched_rules_estimated_bytes":{},"stylesheet_estimated_bytes":{},"decoded_dom_image_bytes":{}}}"#,
                    process.map(|p| p.rss_bytes).unwrap_or(0),
                    process.map(|p| p.vsz_bytes).unwrap_or(0),
                    stats.history_cache_entries,
                    stats.history_cache_bytes,
                    stats.viewport_surface_bytes,
                    stats.renderer_cached_content_surface_bytes,
                    stats.renderer_cached_surface_bytes,
                    stats.tile_surface_bytes,
                    stats.tile_count,
                    stats.display_list_commands,
                    stats.display_list_estimated_bytes,
                    stats.display_list_inline_bytes,
                    stats.display_list_heap_bytes,
                    stats.display_list_text_bytes,
                    stats.display_list_image_bytes,
                    stats.display_list_vector_bytes,
                    stats.raw_resource_cache_entries,
                    stats.raw_resource_cache_bytes,
                    stats.parsed_css_cache_entries,
                    stats.parsed_css_cache_bytes,
                    stats.decoded_image_cache_entries,
                    stats.decoded_image_cache_bytes,
                    stats.dom_nodes,
                    stats.image_nodes,
                    stats.unique_styles,
                    stats.dom_estimated_bytes,
                    stats.layout_estimated_bytes,
                    stats.style_estimated_bytes,
                    stats.line_cache_estimated_bytes,
                    stats.matched_rules_estimated_bytes,
                    stats.stylesheet_estimated_bytes,
                    stats.decoded_dom_image_bytes,
                )
            }
            "deep" => {
                let document = self.tabs[self.active].view.document();
                let Some(doc) = document.as_deref() else {
                    return r#"{"ok":false,"error":"no document"}"#.to_string();
                };
                let selector = dbg_json_str(line, "selector").unwrap_or_default();
                let nodes = crate::dom::query_selector_all(&doc.root, &selector);
                let mut items: Vec<String> = Vec::new();
                for node in nodes {
                    let cr = node.layout.content_rect;
                    let pr = node.layout.padding_rect;
                    let mr = node.layout.margin_rect;
                    let cls = node.attributes.get("class").cloned().unwrap_or_default();
                    let id = node.attributes.get("id").cloned().unwrap_or_default();
                    let children: Vec<String> = node.children.iter()
                        .filter(|c| c.tag != "#text" || !c.text.trim().is_empty())
                        .map(|c| {
                            let cc = c.layout.content_rect;
                            format!(r#"{{"tag":"{}","id":"{}","class":"{}","display":"{:?}","c":[{:.0},{:.0},{:.0},{:.0}]}}"#,
                                c.tag, c.attributes.get("id").unwrap_or(&String::new()),
                                c.attributes.get("class").unwrap_or(&String::new()),
                                c.style.display, cc.x, cc.y, cc.w, cc.h)
                        }).collect();
                    items.push(format!(
                        r#"{{"tag":"{}","id":"{}","class":"{}","content":[{:.0},{:.0},{:.0},{:.0}],"padding":[{:.0},{:.0},{:.0},{:.0}],"margin":[{:.0},{:.0},{:.0},{:.0}],"display":"{:?}","children":[{}],"image_w":{},"image_h":{}}}"#,
                        node.tag, id, cls,
                        cr.x, cr.y, cr.w, cr.h,
                        pr.x, pr.y, pr.w, pr.h,
                        mr.x, mr.y, mr.w, mr.h,
                        node.style.display,
                        children.join(","),
                        node.image_width, node.image_height
                    ));
                }
                format!(
                    r#"{{"ok":true,"count":{},"elements":[{}]}}"#,
                    items.len(),
                    items.join(",")
                )
            }
            "intrinsic" => {
                let document = self.tabs[self.active].view.document();
                let Some(doc) = document.as_deref() else {
                    return r#"{"ok":false,"error":"no document"}"#.to_string();
                };
                let selector = dbg_json_str(line, "selector").unwrap_or_default();
                let nodes = crate::dom::query_selector_all(&doc.root, &selector);
                let engine = crate::layout::LayoutEngine::new();
                let root_font_px = doc.root.style.font_size_px(16.0, 16.0);
                let mut items = Vec::new();
                for node in nodes {
                    let font_px = node.style.font_size_px(root_font_px, root_font_px);
                    let min_content =
                        engine.min_content_width_of_content(node, font_px, root_font_px);
                    let max_content =
                        engine.max_content_width_of_content(node, font_px, root_font_px);
                    let fit_content = engine.intrinsic_width(
                        &crate::types::CssLength::FitContent,
                        node,
                        node.layout.content_rect.w,
                        font_px,
                        root_font_px,
                        node.layout.content_rect.w,
                    );
                    items.push(format!(
                        r#"{{"tag":"{}","id":"{}","class":"{}","display":"{:?}","position":"{:?}","content_w":{:.1},"min_content":{:.1},"max_content":{:.1},"fit_content":{:.1}}}"#,
                        node.tag,
                        node.attributes.get("id").map(String::as_str).unwrap_or(""),
                        node.attributes.get("class").map(String::as_str).unwrap_or(""),
                        node.style.display,
                        node.style.position,
                        node.layout.content_rect.w,
                        min_content,
                        max_content,
                        fit_content
                    ));
                }
                format!(
                    r#"{{"ok":true,"count":{},"elements":[{}]}}"#,
                    items.len(),
                    items.join(",")
                )
            }
            "css" => {
                let document = self.tabs[self.active].view.document();
                let Some(doc) = document.as_deref() else {
                    return r#"{"ok":false,"error":"no document"}"#.to_string();
                };
                let selector = dbg_json_str(line, "selector").unwrap_or_default();
                let props_str = dbg_json_str(line, "props").unwrap_or_default();
                // Ids as well as nodes: the fallback resolver needs `&mut doc`,
                // so a node borrow cannot be held across it.
                let nodes = dbg_select_composed_with_pseudo(&doc.root, doc, &selector);
                let mut items: Vec<String> = Vec::new();
                let mut fallbacks: Vec<(usize, String)> = Vec::new();
                for (ni, node) in nodes.into_iter().enumerate() {
                    let mut kv: Vec<String> = Vec::new();
                    kv.push(format!(r#""tag":"{}""#, node.tag));
                    kv.push(format!(
                        r#""id":"{}""#,
                        node.attributes.get("id").unwrap_or(&String::new())
                    ));
                    kv.push(format!(
                        r#""class":"{}""#,
                        node.attributes.get("class").unwrap_or(&String::new())
                    ));
                    for prop in props_str.split(',') {
                        let prop = prop.trim();
                        if prop.is_empty() {
                            continue;
                        }
                        let val = match prop {
                            "display" => format!("{:?}", node.style.display),
                            "position" => format!("{:?}", node.style.position),
                            "width" => format!("{:?}", node.style.width),
                            "height" => format!("{:?}", node.style.height),
                            "content-rect" => {
                                let r = node.layout.content_rect;
                                format!("{:.1},{:.1} {:.1}x{:.1}", r.x, r.y, r.w, r.h)
                            }
                            "padding-rect" => {
                                let r = node.layout.padding_rect;
                                format!("{:.1},{:.1} {:.1}x{:.1}", r.x, r.y, r.w, r.h)
                            }
                            "margin-rect" => {
                                let r = node.layout.margin_rect;
                                format!("{:.1},{:.1} {:.1}x{:.1}", r.x, r.y, r.w, r.h)
                            }
                            "border-rect" => {
                                let r = node.layout.border_rect;
                                format!("{:.1},{:.1} {:.1}x{:.1}", r.x, r.y, r.w, r.h)
                            }
                            "line-count" => format!("{}", node.layout.line_cache.len()),
                            // The raw url() from the cascade, and whether the
                            // bytes for it ever arrived — the two halves of a
                            // background image, separately observable.
                            "background-image-url" => node.style.background_image_url.clone(),
                            "background-loaded" => format!("{}", node.bg_image_data.is_some()),
                            "background-px" => {
                                format!("{}x{}", node.bg_image_width, node.bg_image_height)
                            }
                            "svg-path" => node
                                .svg_tree_path
                                .as_ref()
                                .map(|path| {
                                    path.iter()
                                        .map(|part| part.to_string())
                                        .collect::<Vec<_>>()
                                        .join(".")
                                })
                                .unwrap_or_default(),
                            "svg-fill-stored" => node
                                .style
                                .svg_fill
                                .map(|c| format!("#{:02x}{:02x}{:02x}{:02x}", c.r, c.g, c.b, c.a))
                                .unwrap_or_else(|| "none".to_string()),
                            "svg-stroke-stored" => node
                                .style
                                .svg_stroke
                                .map(|c| format!("#{:02x}{:02x}{:02x}{:02x}", c.r, c.g, c.b, c.a))
                                .unwrap_or_else(|| "none".to_string()),
                            "svg-paint-flags" => {
                                format!("{}", node.style.rare().specified_svg_paint_props)
                            }
                            "matched-rule-count" => format!("{}", node.matched_rules.len()),
                            _ => String::new(),
                        };
                        if val.is_empty() {
                            fallbacks.push((ni, prop.to_string()));
                        }
                        kv.push(format!(
                            "{}:{}",
                            dbg_json_escape(prop),
                            dbg_json_escape(&val)
                        ));
                    }
                    items.push(format!("{{{}}}", kv.join(",")));
                }
                // Second pass for the properties the fast match did not know.
                drop(document);
                if !fallbacks.is_empty() {
                    if let Some((mut doc, renderer)) =
                        self.tabs[self.active].view.document_and_renderer_mut()
                    {
                        doc.update_computed_style(renderer.layout_engine());
                        for (ni, prop) in fallbacks {
                            let Some(nid) = items.get(ni).and_then(|_| {
                                dbg_select_composed_with_pseudo(&doc.root, &doc, &selector)
                                    .get(ni)
                                    .map(|n| n.node_id)
                            }) else {
                                continue;
                            };
                            let val = doc.computed_style_property_current(nid, &prop);
                            let empty =
                                format!("{}:{}", dbg_json_escape(&prop), dbg_json_escape(""));
                            let filled =
                                format!("{}:{}", dbg_json_escape(&prop), dbg_json_escape(&val));
                            items[ni] = items[ni].replace(&empty, &filled);
                        }
                    }
                }
                format!(
                    r#"{{"ok":true,"count":{},"elements":[{}]}}"#,
                    items.len(),
                    items.join(",")
                )
            }
            "bench" => {
                let n = dbg_json_num(line, "n").unwrap_or(1.0) as u32;
                let n = n.max(1).min(100);
                if self.tabs[self.active].view.document().is_none() {
                    return r#"{"ok":false,"error":"no document"}"#.to_string();
                }
                let mut layout_times = Vec::new();
                for _ in 0..n {
                    let t = std::time::Instant::now();
                    self.tabs[self.active].view.relayout();
                    layout_times.push(t.elapsed().as_micros() as f64 / 1000.0);
                }
                let avg = layout_times.iter().sum::<f64>() / layout_times.len() as f64;
                format!(
                    r#"{{"ok":true,"iterations":{},"layout_avg_ms":{:.1}}}"#,
                    n, avg
                )
            }
            // ── DOM mutation commands ─────────────────────────────────────
            "set-text" => {
                let mut document = self.tabs[self.active].view.document_mut();
                let Some(doc) = document.as_deref_mut() else {
                    return r#"{"ok":false,"error":"no document"}"#.to_string();
                };
                let selector = dbg_json_str(line, "selector").unwrap_or_default();
                let text = dbg_json_str(line, "text").unwrap_or_default();
                if let Some(node) = doc.query_selector(&selector) {
                    doc.set_text_content(node, &text);
                }
                drop(document);
                self.relayout_active();
                r#"{"ok":true}"#.to_string()
            }
            "add-class" => {
                let mut document = self.tabs[self.active].view.document_mut();
                let Some(doc) = document.as_deref_mut() else {
                    return r#"{"ok":false,"error":"no document"}"#.to_string();
                };
                let selector = dbg_json_str(line, "selector").unwrap_or_default();
                let cls = dbg_json_str(line, "class").unwrap_or_default();
                if let Some(node) = doc.query_selector(&selector) {
                    doc.class_list_add(node, &cls);
                }
                drop(document);
                self.relayout_active();
                r#"{"ok":true}"#.to_string()
            }
            "remove-class" => {
                let mut document = self.tabs[self.active].view.document_mut();
                let Some(doc) = document.as_deref_mut() else {
                    return r#"{"ok":false,"error":"no document"}"#.to_string();
                };
                let selector = dbg_json_str(line, "selector").unwrap_or_default();
                let cls = dbg_json_str(line, "class").unwrap_or_default();
                if let Some(node) = doc.query_selector(&selector) {
                    doc.class_list_remove(node, &cls);
                }
                drop(document);
                self.relayout_active();
                r#"{"ok":true}"#.to_string()
            }
            "toggle-class" => {
                let mut document = self.tabs[self.active].view.document_mut();
                let Some(doc) = document.as_deref_mut() else {
                    return r#"{"ok":false,"error":"no document"}"#.to_string();
                };
                let selector = dbg_json_str(line, "selector").unwrap_or_default();
                let cls = dbg_json_str(line, "class").unwrap_or_default();
                if let Some(node) = doc.query_selector(&selector) {
                    doc.class_list_toggle(node, &cls);
                }
                drop(document);
                self.relayout_active();
                r#"{"ok":true}"#.to_string()
            }
            // ── Event listeners query ────────────────────────────────────────
            "event-listeners" => {
                let document = self.tabs[self.active].view.document();
                let Some(doc) = document.as_deref() else {
                    return r#"{"ok":false,"error":"no document"}"#.to_string();
                };
                // There is one listener system now — the WHATWG one.
                let has_listeners = !doc.event_targets.is_empty();
                format!(r#"{{"ok":true,"listeners":{}}}"#, has_listeners)
            }
            // ── Force element state ──────────────────────────────────────────
            "force-state" => {
                let mut document = self.tabs[self.active].view.document_mut();
                let Some(doc) = document.as_deref_mut() else {
                    return r#"{"ok":false,"error":"no document"}"#.to_string();
                };
                let selector = dbg_json_str(line, "selector").unwrap_or_default();
                let state = dbg_json_str(line, "state").unwrap_or_default();
                if let Some(node) = crate::dom::query_selector(&doc.root, &selector) {
                    let nid = node.node_id;
                    match state.as_str() {
                        "hover" => {
                            doc.hovered_box = nid;
                            doc.hover_changed = true;
                        }
                        "focus" => {
                            doc.focused_box = nid;
                        }
                        "active" => {
                            doc.active_box = nid;
                        }
                        _ => {}
                    }
                }
                doc.style_dirty = true;
                drop(document);
                self.relayout_active();
                r#"{"ok":true}"#.to_string()
            }
            // ── Search DOM by text content ───────────────────────────────────
            "search" => {
                let document = self.tabs[self.active].view.document();
                let Some(doc) = document.as_deref() else {
                    return r#"{"ok":false,"error":"no document"}"#.to_string();
                };
                let query = dbg_json_str(line, "query")
                    .unwrap_or_default()
                    .to_lowercase();
                let mut results: Vec<String> = Vec::new();
                fn search_walk(
                    doc: &crate::Document,
                    node: &crate::WebCore,
                    q: &str,
                    results: &mut Vec<String>,
                ) {
                    if node.tag == "#text" && node.text.to_lowercase().contains(q) {
                        // ⛔ Ask the DOM. This read `node.parent`, a render-tree
                        // field the mutation APIs never maintained, so every
                        // script-created node reported `parent_id: 0`.
                        let pid = doc.parent_node(node.node_id);
                        results.push(format!(
                            r#"{{"node_id":{},"parent_id":{},"text":{}}}"#,
                            node.node_id,
                            pid,
                            dbg_json_escape(
                                &node.text.trim().chars().take(100).collect::<String>()
                            )
                        ));
                    }
                    for child in &node.children {
                        search_walk(doc, child, q, results);
                    }
                }
                search_walk(doc, &doc.root, &query, &mut results);
                format!(
                    r#"{{"ok":true,"count":{},"results":[{}]}}"#,
                    results.len(),
                    results.join(",")
                )
            }
            // ── Box model (Chrome-style) ─────────────────────────────────────
            "box-model" => {
                let document = self.tabs[self.active].view.document();
                let Some(doc) = document.as_deref() else {
                    return r#"{"ok":false,"error":"no document"}"#.to_string();
                };
                let selector = dbg_json_str(line, "selector").unwrap_or_default();
                if let Some(node) = crate::dom::query_selector(&doc.root, &selector) {
                    let l = &node.layout;
                    format!(
                        concat!(
                            r#"{{"ok":true,"tag":"{}","margin":{{"top":{:.1},"right":{:.1},"bottom":{:.1},"left":{:.1}}},"#,
                            r#""border":{{"top":{:.1},"right":{:.1},"bottom":{:.1},"left":{:.1}}},"#,
                            r#""padding":{{"top":{:.1},"right":{:.1},"bottom":{:.1},"left":{:.1}}},"#,
                            r#""content":{{"width":{:.1},"height":{:.1}}}}}"#
                        ),
                        node.tag,
                        l.resolved_margin_top,
                        l.resolved_margin_right,
                        l.resolved_margin_bottom,
                        l.resolved_margin_left,
                        l.resolved_border_top,
                        l.resolved_border_right,
                        l.resolved_border_bottom,
                        l.resolved_border_left,
                        l.resolved_pad_top,
                        l.resolved_pad_right,
                        l.resolved_pad_bottom,
                        l.resolved_pad_left,
                        l.content_rect.w,
                        l.content_rect.h,
                    )
                } else {
                    format!(
                        r#"{{"ok":false,"error":"no match: {}"}}"#,
                        dbg_json_escape(&selector)
                    )
                }
            }
            // ── Network log ──────────────────────────────────────────────────
            "network" => {
                // Return basic info about loaded resources
                let document = self.tabs[self.active].view.document();
                let Some(doc) = document.as_deref() else {
                    return r#"{"ok":false,"error":"no document"}"#.to_string();
                };
                let css_count = doc.linked_stylesheets.len();
                let mut img_count = 0u32;
                crate::Document::walk_all(&doc.root, &mut |b| {
                    if b.image_data.is_some() {
                        img_count += 1;
                    }
                });
                format!(
                    r#"{{"ok":true,"stylesheets":{},"images_loaded":{}}}"#,
                    css_count, img_count
                )
            }
            // ── HTML media element control/state ────────────────────────────
            "media" => {
                let selector = dbg_json_str(line, "selector").unwrap_or_default();
                let action = dbg_json_str(line, "action").unwrap_or_else(|| "state".to_string());
                let mut document = self.tabs[self.active].view.document_mut();
                let Some(doc) = document.as_deref_mut() else {
                    return r#"{"ok":false,"error":"no document"}"#.to_string();
                };
                let Some(id) = doc.query_selector(&selector) else {
                    return format!(
                        r#"{{"ok":false,"error":"no match: {}"}}"#,
                        dbg_json_escape(&selector)
                    );
                };
                if !doc.is_media_element(id) {
                    return format!(
                        r#"{{"ok":false,"error":"not media: {}"}}"#,
                        dbg_json_escape(&selector)
                    );
                }
                let changed = match action.as_str() {
                    "play" => doc.media_play(id),
                    "pause" => doc.media_pause(id),
                    "toggle" => doc.media_toggle_playback(id),
                    "load" => doc.media_load(id),
                    "seek" => dbg_json_num(line, "time")
                        .map(|time| doc.media_set_current_time(id, time))
                        .unwrap_or(false),
                    "volume" => dbg_json_num(line, "value")
                        .map(|volume| doc.media_set_volume(id, volume))
                        .unwrap_or(false),
                    "muted" => dbg_json_bool(line, "value")
                        .map(|muted| doc.media_set_muted(id, muted))
                        .unwrap_or(false),
                    "rate" => dbg_json_num(line, "value")
                        .map(|rate| doc.media_set_playback_rate(id, rate))
                        .unwrap_or(false),
                    "state" => true,
                    _ => false,
                };
                let src = doc.media_current_src(id).unwrap_or_default();
                let current_time = doc.media_current_time(id).unwrap_or(0.0);
                let duration = doc
                    .media_duration(id)
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "null".to_string());
                let ready = doc.media_ready_state(id).unwrap_or(0);
                let network = doc.media_network_state(id).unwrap_or(0);
                let paused = doc.media_paused(id).unwrap_or(true);
                let ended = doc.media_ended(id).unwrap_or(false);
                let volume = doc.media_volume(id).unwrap_or(1.0);
                let muted = doc.media_muted(id).unwrap_or(false);
                let tracks = doc.media_text_tracks(id).unwrap_or_default();
                let tracks_json = tracks
                    .iter()
                    .map(|track| {
                        format!(
                            r#"{{"kind":{},"label":{},"language":{},"src":{},"default":{}}}"#,
                            dbg_json_escape(&track.kind),
                            dbg_json_escape(&track.label),
                            dbg_json_escape(&track.language),
                            dbg_json_escape(&track.src),
                            track.default
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                if changed && action != "state" {
                    drop(document);
                    self.tabs[self.active].view.relayout();
                }
                format!(
                    r#"{{"ok":true,"changed":{},"node_id":{},"src":{},"currentTime":{:.3},"duration":{},"readyState":{},"networkState":{},"paused":{},"ended":{},"volume":{:.3},"muted":{},"textTracks":[{}]}}"#,
                    changed,
                    id,
                    dbg_json_escape(&src),
                    current_time,
                    duration,
                    ready,
                    network,
                    paused,
                    ended,
                    volume,
                    muted,
                    tracks_json
                )
            }
            // ── HTML output ────────────────────────────────────────────────
            "dom-html" => {
                let document = self.tabs[self.active].view.document();
                let Some(doc) = document.as_deref() else {
                    return r#"{"ok":false,"error":"no document"}"#.to_string();
                };
                let selector = dbg_json_str(line, "selector").unwrap_or_default();
                if let Some(node) = crate::dom::query_selector(&doc.root, &selector) {
                    let html = {
                        let mut s = String::new();
                        crate::html::serializer::serialize_box(node, &mut s);
                        s
                    };
                    format!(r#"{{"ok":true,"html":{}}}"#, dbg_json_escape(&html))
                } else {
                    format!(r#"{{"ok":false,"error":"no match"}}"#)
                }
            }
            // ── Viewport info ────────────────────────────────────────────────
            "viewport" => {
                let scroll_x = self
                    .tabs
                    .get(self.active)
                    .and_then(|t| t.view.document())
                    .map(|d| d.scroll_x)
                    .unwrap_or(0.0);
                let scroll_y = self
                    .tabs
                    .get(self.active)
                    .and_then(|t| t.view.document())
                    .map(|d| d.scroll_y)
                    .unwrap_or(0.0);
                let doc_h = self
                    .tabs
                    .get(self.active)
                    .and_then(|t| t.view.document())
                    .map(|d| Document::scroll_height(&d.root))
                    .unwrap_or(0.0);
                format!(
                    r#"{{"ok":true,"width":{:.0},"height":{:.0},"scroll_x":{:.1},"scroll_y":{:.1},"doc_height":{:.0},"scale":{:.1},"device_scale":{}}}"#,
                    self.width,
                    self.content_h(),
                    scroll_x,
                    scroll_y,
                    doc_h,
                    self.tabs[self.active].view.zoom(),
                    self.platform
                        .as_ref()
                        .map(|p| p.scale_factor())
                        .unwrap_or(1.0)
                )
            }
            // ── Accessibility tree ───────────────────────────────────────────
            "accessibility" | "a11y" => {
                let document = self.tabs[self.active].view.document();
                let Some(doc) = document.as_deref() else {
                    return r#"{"ok":false,"error":"no document"}"#.to_string();
                };
                fn a11y_walk(node: &crate::WebCore, depth: usize, out: &mut String) {
                    let role = match node.tag.as_str() {
                        "a" => "link",
                        "button" | "input" => "button",
                        "img" => "image",
                        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => "heading",
                        "nav" => "navigation",
                        "main" => "main",
                        "header" => "banner",
                        "footer" => "contentinfo",
                        "ul" | "ol" => "list",
                        "li" => "listitem",
                        "table" => "table",
                        "tr" => "row",
                        "td" | "th" => "cell",
                        "form" => "form",
                        "section" => "region",
                        "article" => "article",
                        "aside" => "complementary",
                        "#text" => {
                            if !node.text.trim().is_empty() {
                                "text"
                            } else {
                                return;
                            }
                        }
                        _ => {
                            let aria = node
                                .attributes
                                .get("role")
                                .map(|s| s.as_str())
                                .unwrap_or("");
                            if !aria.is_empty() { aria } else { "" }
                        }
                    };
                    if !role.is_empty() {
                        let indent = "  ".repeat(depth);
                        let label = node
                            .attributes
                            .get("aria-label")
                            .or(node.attributes.get("alt"))
                            .or(node.attributes.get("title"))
                            .cloned()
                            .unwrap_or_else(|| {
                                if node.tag == "#text" {
                                    node.text.trim().chars().take(50).collect()
                                } else {
                                    String::new()
                                }
                            });
                        out.push_str(&format!("{}{}: {}\n", indent, role, label));
                    }
                    for child in &node.children {
                        a11y_walk(child, depth + if !role.is_empty() { 1 } else { 0 }, out);
                    }
                }
                let mut tree = String::new();
                a11y_walk(&doc.root, 0, &mut tree);
                format!(r#"{{"ok":true,"tree":{}}}"#, dbg_json_escape(&tree))
            }
            // ── Measure distance between elements ────────────────────────────
            "measure" => {
                let document = self.tabs[self.active].view.document();
                let Some(doc) = document.as_deref() else {
                    return r#"{"ok":false,"error":"no document"}"#.to_string();
                };
                let from = dbg_json_str(line, "from").unwrap_or_default();
                let to = dbg_json_str(line, "to").unwrap_or_default();
                let a = crate::dom::query_selector(&doc.root, &from).map(|n| n.layout.border_rect);
                let b = crate::dom::query_selector(&doc.root, &to).map(|n| n.layout.border_rect);
                match (a, b) {
                    (Some(a), Some(b)) => {
                        let dx = b.x - (a.x + a.w); // gap between right of A and left of B
                        let dy = b.y - (a.y + a.h); // gap between bottom of A and top of B
                        let cx = (b.x + b.w / 2.0) - (a.x + a.w / 2.0); // center-to-center
                        let cy = (b.y + b.h / 2.0) - (a.y + a.h / 2.0);
                        format!(
                            r#"{{"ok":true,"gap_x":{:.1},"gap_y":{:.1},"center_dx":{:.1},"center_dy":{:.1}}}"#,
                            dx, dy, cx, cy
                        )
                    }
                    _ => r#"{"ok":false,"error":"one or both selectors not found"}"#.to_string(),
                }
            }
            // ── Web inspector UI ─────────────────────────────────────────────
            "inspector" | "devtools" => {
                format!(
                    r#"{{"ok":true,"message":"Connect browser to http://127.0.0.1:{}/inspector to use the web UI"}}"#,
                    self.debug_cmd_rx
                        .as_ref()
                        .map(|_| "debug-port")
                        .unwrap_or("?")
                )
            }
            // ── Structured DOM tree (JSON) ────────────────────────────────
            // ── Inspect by node_id ────────────────────────────────────────
            "inspect-node" => {
                let document = self.tabs[self.active].view.document();
                let Some(doc) = document.as_deref() else {
                    return r#"{"ok":false,"error":"no document"}"#.to_string();
                };
                let nid = dbg_json_num(line, "nid").unwrap_or(0.0) as u32;
                if let Some(node) =
                    dbg_get_composed_by_id(&doc.root, nid).or_else(|| doc.get_box_by_id(nid))
                {
                    let l = &node.layout;
                    let tag = &node.tag;
                    let id = node.attributes.get("id").map(|s| s.as_str()).unwrap_or("");
                    let cls = node
                        .attributes
                        .get("class")
                        .map(|s| s.as_str())
                        .unwrap_or("");
                    format!(
                        concat!(
                            r#"{{"ok":true,"tag":"{}","id":"{}","class":"{}","nid":{},"#,
                            r#""display":"{:?}","position":"{:?}","top":"{:?}","bottom":"{:?}","#,
                            r#""margin":{{"top":{:.1},"right":{:.1},"bottom":{:.1},"left":{:.1}}},"#,
                            r#""border":{{"top":{:.1},"right":{:.1},"bottom":{:.1},"left":{:.1}}},"#,
                            r#""padding":{{"top":{:.1},"right":{:.1},"bottom":{:.1},"left":{:.1}}},"#,
                            r#""content":{{"x":{:.1},"y":{:.1},"width":{:.1},"height":{:.1}}},"#,
                            r#""font_size":{:.1},"color":"{:02x}{:02x}{:02x}","bg":"{:02x}{:02x}{:02x}{:02x}","#,
                            r#""image":{{"width":{},"height":{},"bytes":{},"src":"{}"}}"#,
                            r#"}}"#
                        ),
                        tag,
                        id,
                        cls,
                        nid,
                        node.style.display,
                        node.style.position,
                        node.style.top,
                        node.style.bottom,
                        l.resolved_margin_top,
                        l.resolved_margin_right,
                        l.resolved_margin_bottom,
                        l.resolved_margin_left,
                        l.resolved_border_top,
                        l.resolved_border_right,
                        l.resolved_border_bottom,
                        l.resolved_border_left,
                        l.resolved_pad_top,
                        l.resolved_pad_right,
                        l.resolved_pad_bottom,
                        l.resolved_pad_left,
                        l.content_rect.x,
                        l.content_rect.y,
                        l.content_rect.w,
                        l.content_rect.h,
                        node.style.font_size_px(16.0, 16.0),
                        node.style.color.r,
                        node.style.color.g,
                        node.style.color.b,
                        node.style.background_color.r,
                        node.style.background_color.g,
                        node.style.background_color.b,
                        node.style.background_color.a,
                        node.image_width,
                        node.image_height,
                        node.image_data.as_ref().map(|d| d.len()).unwrap_or(0),
                        node.resolved_src.replace('"', "\\\""),
                    )
                } else {
                    format!(r#"{{"ok":false,"error":"node {} not found"}}"#, nid)
                }
            }
            "dom-tree" => {
                let document = self.tabs[self.active].view.document();
                let Some(doc) = document.as_deref() else {
                    return r#"{"ok":false,"error":"no document"}"#.to_string();
                };
                let max_depth = dbg_json_num(line, "depth").unwrap_or(3.0) as usize;
                let nid = dbg_json_num(line, "nid").map(|n| n as u32);
                let root_sel = dbg_json_str(line, "selector");
                // Find root node: by nid, by selector, or document root
                let root_node = if let Some(id) = nid {
                    dbg_get_composed_by_id(&doc.root, id)
                        .or_else(|| doc.get_box_by_id(id))
                        .unwrap_or(&doc.root)
                } else if let Some(sel) = root_sel {
                    dbg_select_composed_with_pseudo(&doc.root, &doc, &sel)
                        .into_iter()
                        .next()
                        .unwrap_or(&doc.root)
                } else {
                    &doc.root
                };
                fn tree_json(node: &crate::WebCore, depth: usize, max_depth: usize) -> String {
                    let tag = &node.tag;
                    let id = node.attributes.get("id").map(|s| s.as_str()).unwrap_or("");
                    let cls = node
                        .attributes
                        .get("class")
                        .map(|s| s.as_str())
                        .unwrap_or("");
                    let nid = node.node_id;
                    let cr = node.layout.content_rect;
                    let child_count = node
                        .effective_children()
                        .iter()
                        .filter(|c| !(c.tag == "#text" && c.text.trim().is_empty()))
                        .count();
                    let text_preview = if tag == "#text" {
                        let t: String = node.text.trim().chars().take(60).collect();
                        format!(
                            r#","text":"{}""#,
                            t.replace('\\', "\\\\").replace('"', "\\\"")
                        )
                    } else {
                        String::new()
                    };
                    let children_json = if depth < max_depth && child_count > 0 {
                        let kids: Vec<String> = node
                            .effective_children()
                            .iter()
                            .filter(|c| !(c.tag == "#text" && c.text.trim().is_empty()))
                            .map(|c| tree_json(c, depth + 1, max_depth))
                            .collect();
                        format!(r#","children":[{}]"#, kids.join(","))
                    } else if child_count > 0 {
                        format!(r#","child_count":{}"#, child_count)
                    } else {
                        String::new()
                    };
                    format!(
                        r#"{{"tag":"{}","id":"{}","class":"{}","nid":{},"rect":[{:.0},{:.0},{:.0},{:.0}]{}{}{}}}"#,
                        tag,
                        id,
                        cls,
                        nid,
                        cr.x,
                        cr.y,
                        cr.w,
                        cr.h,
                        text_preview,
                        if child_count > 0 {
                            format!(r#","count":{}"#, child_count)
                        } else {
                            String::new()
                        },
                        children_json
                    )
                }
                let json = tree_json(root_node, 0, max_depth);
                format!(r#"{{"ok":true,"tree":{}}}"#, json)
            }
            // ── DOM path (CSS selector chain to element) ─────────────────
            "dom-path" | "path" => {
                let document = self.tabs[self.active].view.document();
                let Some(doc) = document.as_deref() else {
                    return r#"{"ok":false,"error":"no document"}"#.to_string();
                };
                let selector = dbg_json_str(line, "selector").unwrap_or_default();
                let nodes = crate::dom::query_selector_all(&doc.root, &selector);
                let mut paths: Vec<String> = Vec::new();
                for node in nodes {
                    fn build_path(
                        root: &crate::WebCore,
                        target_id: u32,
                        path: &mut Vec<String>,
                    ) -> bool {
                        let id = root
                            .attributes
                            .get("id")
                            .map(|v| format!("#{v}"))
                            .unwrap_or_default();
                        let cls = root
                            .attributes
                            .get("class")
                            .map(|v| format!(".{}", v.split_whitespace().next().unwrap_or("")))
                            .unwrap_or_default();
                        path.push(format!("{}{}{}", root.tag, id, cls));
                        if root.node_id == target_id {
                            return true;
                        }
                        for child in &root.children {
                            if build_path(child, target_id, path) {
                                return true;
                            }
                        }
                        path.pop();
                        false
                    }
                    let mut p = Vec::new();
                    build_path(&doc.root, node.node_id, &mut p);
                    paths.push(dbg_json_escape(&p.join(" > ")));
                }
                format!(
                    r#"{{"ok":true,"count":{},"paths":[{}]}}"#,
                    paths.len(),
                    paths.join(",")
                )
            }
            // ── Parent / ancestor chain ──────────────────────────────────────
            "parent" => {
                let document = self.tabs[self.active].view.document();
                let Some(doc) = document.as_deref() else {
                    return r#"{"ok":false,"error":"no document"}"#.to_string();
                };
                let selector = dbg_json_str(line, "selector").unwrap_or_default();
                if let Some(node) = crate::dom::query_selector(&doc.root, &selector) {
                    fn ancestors(
                        root: &crate::WebCore,
                        target_id: u32,
                        chain: &mut Vec<String>,
                    ) -> bool {
                        if root.node_id == target_id {
                            let id = root.attributes.get("id").cloned().unwrap_or_default();
                            let cls = root.attributes.get("class").cloned().unwrap_or_default();
                            chain.push(format!(
                                r#"{{"tag":"{}","id":"{}","class":"{}","nid":{}}}"#,
                                root.tag, id, cls, root.node_id
                            ));
                            return true;
                        }
                        for child in &root.children {
                            if ancestors(child, target_id, chain) {
                                let id = root.attributes.get("id").cloned().unwrap_or_default();
                                let cls = root.attributes.get("class").cloned().unwrap_or_default();
                                chain.push(format!(
                                    r#"{{"tag":"{}","id":"{}","class":"{}","nid":{}}}"#,
                                    root.tag, id, cls, root.node_id
                                ));
                                return true;
                            }
                        }
                        false
                    }
                    let mut chain = Vec::new();
                    ancestors(&doc.root, node.node_id, &mut chain);
                    format!(r#"{{"ok":true,"chain":[{}]}}"#, chain.join(","))
                } else {
                    r#"{"ok":false,"error":"not found"}"#.to_string()
                }
            }
            // ── Hit test at coordinates ──────────────────────────────────────
            "hit" => {
                let document = self.tabs[self.active].view.document();
                let Some(doc) = document.as_deref() else {
                    return r#"{"ok":false,"error":"no document"}"#.to_string();
                };
                let x = dbg_json_num(line, "x").unwrap_or(0.0) as f32;
                let y = dbg_json_num(line, "y").unwrap_or(0.0) as f32;
                if let Some(hit) = crate::layout::hit_test::point_to_hit_scrolled(
                    &doc.root,
                    (x + doc.scroll_x, y + doc.scroll_y),
                    (doc.scroll_x, doc.scroll_y),
                    0,
                ) {
                    if let Some(node) = doc.get_box_by_id(hit.node_id) {
                        let id = node.attributes.get("id").cloned().unwrap_or_default();
                        let cls = node.attributes.get("class").cloned().unwrap_or_default();
                        format!(
                            r#"{{"ok":true,"nid":{},"tag":"{}","id":"{}","class":"{}"}}"#,
                            hit.node_id, node.tag, id, cls
                        )
                    } else {
                        format!(r#"{{"ok":true,"nid":{},"tag":"?"}}"#, hit.node_id)
                    }
                } else {
                    r#"{"ok":false,"error":"no hit"}"#.to_string()
                }
            }
            // ── Progressive layout benchmark ─────────────────────────────────
            "bench-progressive" => {
                let Some((full_ms, above_ms)) =
                    self.tabs[self.active].view.benchmark_progressive_layout()
                else {
                    return r#"{"ok":false,"error":"no document"}"#.to_string();
                };
                format!(
                    r#"{{"ok":true,"full_ms":{:.1},"above_fold_ms":{:.1}}}"#,
                    full_ms, above_ms
                )
            }
            "bench-render" => {
                let scale = dbg_json_num(line, "scale")
                    .map(|v| v.clamp(0.25, 4.0) as f32)
                    .unwrap_or(1.0);
                let dy = dbg_json_num(line, "dy").unwrap_or(500.0) as f32;
                let view_w = self.page_width();
                let view_h = self.content_h();
                let phys_w = (view_w * scale).ceil().max(1.0) as u32;
                let phys_h = (view_h * scale).ceil().max(1.0) as u32;
                let Some(mut pm) = tiny_skia::Pixmap::new(phys_w, phys_h) else {
                    return r#"{"ok":false,"error":"pixmap failed"}"#.to_string();
                };
                let view = &mut self.tabs[self.active].view;
                let original_y = view.scroll_y();
                let t0 = std::time::Instant::now();
                view.paint_into(&mut pm, 0, 0, scale);
                let first = t0.elapsed().as_micros() as f64 / 1000.0;
                view.scroll_by(0.0, dy);
                let t1 = std::time::Instant::now();
                view.paint_into(&mut pm, 0, 0, scale);
                let scroll = t1.elapsed().as_micros() as f64 / 1000.0;
                let t2 = std::time::Instant::now();
                view.paint_into(&mut pm, 0, 0, scale);
                let cached = t2.elapsed().as_micros() as f64 / 1000.0;
                let scroll_y = view.scroll_y();
                view.scroll_to(0.0, original_y);
                view.paint_into(&mut pm, 0, 0, scale);
                format!(
                    r#"{{"ok":true,"first_ms":{:.1},"scroll_ms":{:.1},"cached_ms":{:.1},"scroll_y":{:.0},"width":{},"height":{},"scale":{}}}"#,
                    first, scroll, cached, scroll_y, phys_w, phys_h, scale
                )
            }
            "quit" => {
                self.close_requested = true;
                r#"{"ok":true}"#.into()
            }
            _ => format!(
                r#"{{"ok":false,"error":"unknown command: {}"}}"#,
                dbg_json_escape(cmd)
            ),
        }
    }
}

pub(super) fn debug_cmd_requests_redraw(cmd: &str) -> bool {
    matches!(
        cmd,
        "navigate"
            | "switch-tab"
            | "resize"
            | "scroll"
            | "click"
            | "hover"
            | "type"
            | "key"
            | "set-text"
            | "add-class"
            | "remove-class"
            | "toggle-class"
            | "force-state"
            | "bench"
            | "bench-progressive"
    )
}

/// Owns the listener and its active clients for one browser context.
pub(super) struct DebugServer {
    address: std::net::SocketAddr,
    stopped: Arc<std::sync::atomic::AtomicBool>,
    clients: Arc<std::sync::Mutex<Vec<std::sync::Weak<std::net::TcpStream>>>>,
    listener: Option<std::thread::JoinHandle<()>>,
}

impl Drop for DebugServer {
    fn drop(&mut self) {
        self.stopped
            .store(true, std::sync::atomic::Ordering::Release);
        // Wake the blocking accept without a polling timer in the idle browser.
        let _ = std::net::TcpStream::connect_timeout(
            &self.address,
            std::time::Duration::from_millis(100),
        );
        if let Some(listener) = self.listener.take() {
            let _ = listener.join();
        }
        if let Ok(clients) = self.clients.lock() {
            for client in clients.iter().filter_map(std::sync::Weak::upgrade) {
                let _ = client.shutdown(std::net::Shutdown::Both);
            }
        }
    }
}

/// TCP transport for the documented custom inspector protocol, not CDP.
fn wait_debug_reply(
    receiver: &mpsc::Receiver<String>,
    stopped: &std::sync::atomic::AtomicBool,
    timeout: std::time::Duration,
) -> Option<String> {
    let deadline = std::time::Instant::now() + timeout;
    while !stopped.load(std::sync::atomic::Ordering::Acquire) {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return None;
        }
        match receiver.recv_timeout(remaining.min(std::time::Duration::from_millis(100))) {
            Ok(reply) => return Some(reply),
            Err(mpsc::RecvTimeoutError::Disconnected) => return None,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
    }
    None
}

pub(super) fn browser_debug_spawn_tcp(
    port: u16,
    cmd_tx: mpsc::Sender<(String, mpsc::Sender<String>)>,
    proxy: EventLoopProxy<()>,
) -> std::io::Result<DebugServer> {
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    let address = listener.local_addr()?;
    let stopped = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let clients = Arc::new(std::sync::Mutex::new(Vec::<
        std::sync::Weak<std::net::TcpStream>,
    >::new()));
    let thread_stopped = stopped.clone();
    let thread_clients = clients.clone();
    let thread = std::thread::spawn(move || {
        eprintln!("[debug] Listening on 127.0.0.1:{port}");
        while !thread_stopped.load(std::sync::atomic::Ordering::Acquire) {
            match listener.accept() {
                Ok((stream, _)) => {
                    if thread_stopped.load(std::sync::atomic::Ordering::Acquire) {
                        break;
                    }
                    let stream = Arc::new(stream);
                    if let Ok(mut clients) = thread_clients.lock() {
                        clients.retain(|client| client.strong_count() > 0);
                        clients.push(Arc::downgrade(&stream));
                    }
                    let cmd_tx = cmd_tx.clone();
                    let proxy = proxy.clone();
                    let client_stopped = thread_stopped.clone();
                    std::thread::spawn(move || {
                        let peer = stream
                            .peer_addr()
                            .map(|a| a.to_string())
                            .unwrap_or_default();
                        eprintln!("[debug] connect {peer}");
                        let Ok(read_stream) = stream.try_clone() else {
                            return;
                        };
                        let Ok(mut writer) = stream.try_clone() else {
                            return;
                        };
                        let mut reader = BufReader::new(read_stream);
                        let cmd_tx = cmd_tx.clone();
                        let proxy = proxy.clone();

                        // Peek first line to detect HTTP vs JSON
                        let mut first_line = String::new();
                        if reader.read_line(&mut first_line).is_err() {
                            return;
                        }

                        if first_line.starts_with("GET ") {
                            // HTTP request — serve the inspector web UI
                            // Read remaining headers (discard)
                            loop {
                                let mut h = String::new();
                                if reader.read_line(&mut h).is_err() || h.trim().is_empty() {
                                    break;
                                }
                            }
                            let path = first_line.split_whitespace().nth(1).unwrap_or("/");
                            let (content_type, body) = if path == "/api"
                                || path.starts_with("/api?")
                            {
                                // API endpoint for the inspector: extract cmd from query string
                                let query = path.split('?').nth(1).unwrap_or("");
                                let cmd_json =
                                    urldecode(query.strip_prefix("cmd=").unwrap_or("{}"));
                                let (reply_tx, reply_rx) = mpsc::channel();
                                let _ = cmd_tx.send((cmd_json, reply_tx));
                                let _ = proxy.send_event(());
                                let resp = wait_debug_reply(
                                    &reply_rx,
                                    &client_stopped,
                                    std::time::Duration::from_secs(10),
                                )
                                .unwrap_or_else(|| r#"{"ok":false,"error":"timeout"}"#.to_string());
                                ("application/json", resp)
                            } else {
                                ("text/html", INSPECTOR_HTML.to_string())
                            };
                            let response = format!(
                                "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n{}",
                                content_type,
                                body.len(),
                                body
                            );
                            let _ = writer.write_all(response.as_bytes());
                            let _ = writer.flush();
                        } else {
                            // JSON command protocol
                            let line = first_line;
                            if !line.trim().is_empty() {
                                let (reply_tx, reply_rx) = mpsc::channel();
                                let _ = cmd_tx.send((line.trim().to_string(), reply_tx));
                                let _ = proxy.send_event(());
                                if let Some(resp) = wait_debug_reply(
                                    &reply_rx,
                                    &client_stopped,
                                    std::time::Duration::from_secs(30),
                                ) {
                                    if !resp.is_empty() {
                                        let _ = writeln!(writer, "{}", resp);
                                        let _ = writer.flush();
                                    }
                                }
                            }
                            // Continue reading more JSON commands on same connection
                            for line in reader.lines() {
                                let line = match line {
                                    Ok(l) => l,
                                    Err(_) => break,
                                };
                                if line.trim().is_empty() {
                                    continue;
                                }
                                let (reply_tx, reply_rx) = mpsc::channel();
                                let _ = cmd_tx.send((line, reply_tx));
                                let _ = proxy.send_event(());
                                if let Some(resp) = wait_debug_reply(
                                    &reply_rx,
                                    &client_stopped,
                                    std::time::Duration::from_secs(30),
                                ) {
                                    if !resp.is_empty() {
                                        let _ = writeln!(writer, "{}", resp);
                                        let _ = writer.flush();
                                    }
                                }
                            }
                        }
                        eprintln!("[debug] disconnect {peer}");
                    });
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => {
                    eprintln!("[debug] accept error: {e}");
                    break;
                }
            }
        }
    });
    Ok(DebugServer {
        address,
        stopped,
        clients,
        listener: Some(thread),
    })
}

pub(super) fn find_chrome() -> Option<String> {
    let mac = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
    if std::path::Path::new(mac).exists() {
        return Some(mac.to_string());
    }
    for name in &[
        "google-chrome",
        "google-chrome-stable",
        "chromium",
        "chromium-browser",
    ] {
        if std::process::Command::new("which")
            .arg(name)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            return Some(name.to_string());
        }
    }
    None
}

/// The websocket URL of Chrome's PAGE target, from a `/json` listing.
///
/// The listing is a flat array of objects; each is scanned for `"type":"page"`
/// and its own `webSocketDebuggerUrl` taken, so a browser-UI target listed
/// first cannot capture the connection.
pub(super) fn cdp_page_target(body: &str) -> Option<String> {
    let mut fallback = None;
    for chunk in body.split("{\n").chain(body.split("},")) {
        let Some(ws) = chunk
            .split("\"webSocketDebuggerUrl\": \"")
            .nth(1)
            .or_else(|| chunk.split("\"webSocketDebuggerUrl\":\"").nth(1))
            .and_then(|s| s.split('"').next())
        else {
            continue;
        };
        let is_page = chunk.contains("\"type\": \"page\"") || chunk.contains("\"type\":\"page\"");
        if is_page {
            return Some(ws.to_string());
        }
        if fallback.is_none() {
            fallback = Some(ws.to_string());
        }
    }
    fallback
}

pub(super) fn cdp_send(chrome_port: u16, method: &str, params: &str) -> Result<String, String> {
    let list_url = format!("http://127.0.0.1:{}/json", chrome_port);
    let resp = reqwest::blocking::Client::new()
        .get(&list_url)
        .timeout(std::time::Duration::from_secs(2))
        .send()
        .map_err(|e| format!("CDP: {e}"))?;
    let body = resp.text().map_err(|e| e.to_string())?;
    // ⛔ Pick the PAGE target. The list also carries browser-UI targets — an
    // omnibox popup is routinely first — and taking whichever debugger URL
    // appeared first sent every command to the wrong target, which fails as
    // "No debugger URL" or, worse, silently evaluates against Chrome's own UI.
    let ws_url = cdp_page_target(&body).ok_or("No page target")?;
    // The CDP params travel into the helper as a quoted literal and are parsed
    // there, so their JSON stays JSON. Quoted the same way as everything else
    // that crosses this boundary — JSON string syntax is a subset of Python's,
    // so one quoter serves both.
    let params_py = json_quote(params);
    let script = format!(
        r#"
import socket,json,struct,random,base64
url="{ws_url}"
PARAMS={params_py}
p=url.replace("ws://","").split("/",1);hp=p[0].split(":")
s=socket.socket();s.settimeout(5);s.connect((hp[0],int(hp[1])))
path="/"+p[1] if len(p)>1 else "/"
key=base64.b64encode(random.randbytes(16)).decode()
s.sendall(f"GET {{path}} HTTP/1.1\r\nHost: {{hp[0]}}:{{hp[1]}}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {{key}}\r\nSec-WebSocket-Version: 13\r\n\r\n".encode())
r=b""
while b"\r\n\r\n" not in r: r+=s.recv(4096)
# ⛔ params is JSON, not a Python literal: interpolating it raw made
# `true`/`false`/`null` NameErrors, which broke every command that passed
# one — `returnByValue:true` among them.
msg=json.dumps({{"id":1,"method":"{method}","params":json.loads(PARAMS)}}).encode()
f=bytearray([0x81]);mk=random.randbytes(4);l=len(msg)
if l<126: f.append(0x80|l)
elif l<65536: f.append(0x80|126);f.extend(struct.pack(">H",l))
f.extend(mk);f.extend(bytearray(b^mk[i%4] for i,b in enumerate(msg)));s.sendall(bytes(f))
d=b""
while len(d)<2: d+=s.recv(4096)
pl=d[1]&0x7F;o=2
if pl==126: pl=struct.unpack(">H",d[2:4])[0];o=4
elif pl==127: pl=struct.unpack(">Q",d[2:10])[0];o=10
while len(d)<o+pl: d+=s.recv(65536)
print(d[o:o+pl].decode());s.close()
"#
    );
    let output = std::process::Command::new("python3")
        .arg("-c")
        .arg(&script)
        .output()
        .map_err(|e| format!("python3: {e}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

pub(super) fn base64_decode_std(s: &str) -> Result<Vec<u8>, String> {
    const T: &[u8; 128] = b"\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\x3e\xff\xff\xff\x3f\x34\x35\x36\x37\x38\x39\x3a\x3b\x3c\x3d\xff\xff\xff\xff\xff\xff\xff\x00\x01\x02\x03\x04\x05\x06\x07\x08\x09\x0a\x0b\x0c\x0d\x0e\x0f\x10\x11\x12\x13\x14\x15\x16\x17\x18\x19\xff\xff\xff\xff\xff\xff\x1a\x1b\x1c\x1d\x1e\x1f\x20\x21\x22\x23\x24\x25\x26\x27\x28\x29\x2a\x2b\x2c\x2d\x2e\x2f\x30\x31\x32\x33\xff\xff\xff\xff\xff";
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut buf = 0u32;
    let mut bits = 0;
    for &b in s.as_bytes() {
        if b == b'=' || b == b'\n' || b == b'\r' || b >= 128 {
            continue;
        }
        let v = T[b as usize];
        if v == 0xff {
            continue;
        }
        buf = (buf << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    Ok(out)
}

pub(super) fn urldecode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.bytes();
    while let Some(b) = chars.next() {
        if b == b'%' {
            let h1 = chars.next().unwrap_or(b'0');
            let h2 = chars.next().unwrap_or(b'0');
            let hex = [h1, h2];
            if let Ok(s) = std::str::from_utf8(&hex) {
                if let Ok(v) = u8::from_str_radix(s, 16) {
                    out.push(v as char);
                    continue;
                }
            }
            out.push('%');
            out.push(h1 as char);
            out.push(h2 as char);
        } else if b == b'+' {
            out.push(' ');
        } else {
            out.push(b as char);
        }
    }
    out
}

pub(super) const INSPECTOR_HTML: &str = r##"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<title>webcore Inspector</title>
<style>
*{box-sizing:border-box;margin:0;padding:0}
body{font:12px/1.4 Menlo,Monaco,"Courier New",monospace;background:#1e1e1e;color:#d4d4d4;display:flex;flex-direction:column;height:100vh}
#toolbar{background:#2d2d2d;padding:6px 12px;display:flex;gap:8px;align-items:center;border-bottom:1px solid #3e3e3e}
#toolbar input{flex:1;background:#3c3c3c;border:1px solid #555;color:#eee;padding:4px 8px;border-radius:3px;font:inherit}
#toolbar button{background:#0e639c;color:#fff;border:none;padding:4px 12px;border-radius:3px;cursor:pointer;font:inherit}
#toolbar button:hover{background:#1177bb}
#main{display:flex;flex:1;overflow:hidden}
#tree-panel{width:50%;overflow:auto;padding:8px;border-right:1px solid #3e3e3e}
#detail-panel{width:50%;overflow:auto;padding:8px}
.tn{cursor:pointer;padding:1px 0;white-space:nowrap}
.tn:hover{background:#264f78}
.tn.sel{background:#094771}
.tog{display:inline-block;width:14px;text-align:center;color:#888;cursor:pointer;user-select:none}
.tog:hover{color:#fff}
.tag{color:#569cd6}.an{color:#9cdcfe}.av{color:#ce9178}.tx{color:#6a9955;font-style:italic}
.dim{color:#555;font-size:10px}
h3{color:#ccc;margin:8px 0 4px;font-size:12px;border-bottom:1px solid #3e3e3e;padding-bottom:4px}
.prop{display:flex;padding:1px 0}.pn{color:#9cdcfe;min-width:160px}.pv{color:#ce9178}
.bm{text-align:center;padding:12px}
.bm .mb{border:1px dashed #f90;padding:8px;display:inline-block;position:relative}
.bm .bb{border:1px solid #fd5;padding:8px;background:#333}
.bm .pb{border:1px dashed #6c6;padding:8px;background:#2a3a2a}
.bm .cb{background:#264f78;padding:6px;color:#fff;min-width:50px}
.bm span{font-size:10px;color:#aaa}
.lbl{position:absolute;font-size:9px;color:#f90}
.lbl.t{top:-2px;left:50%;transform:translateX(-50%)}
.lbl.b{bottom:-2px;left:50%;transform:translateX(-50%)}
.lbl.l{left:2px;top:50%;transform:translateY(-50%)}
.lbl.r{right:2px;top:50%;transform:translateY(-50%)}
#console{height:120px;border-top:1px solid #3e3e3e;display:flex;flex-direction:column}
#co{flex:1;overflow:auto;padding:4px 8px;font-size:11px;color:#999}
#ci{background:#2d2d2d;border:none;border-top:1px solid #3e3e3e;color:#eee;padding:4px 8px;font:inherit}
</style>
</head>
<body>
<div id="toolbar">
  <input id="sel" placeholder="CSS selector" value="body" onkeydown="if(event.key==='Enter')doFind()">
  <button onclick="doFind()">Find</button>
  <button onclick="doScreenshot()">Screenshot</button>
  <button onclick="loadTree()">Reload Tree</button>
</div>
<div id="main">
  <div id="tree-panel"><i>Loading DOM tree...</i></div>
  <div id="detail-panel"><i>Click an element to inspect</i></div>
</div>
<div id="console">
  <div id="co"></div>
  <input id="ci" placeholder='{"cmd":"find","selector":"h1"}' onkeydown="if(event.key==='Enter')runCmd()">
</div>
<script>
async function api(c){const r=await fetch('/api?cmd='+encodeURIComponent(JSON.stringify(c)));return r.json()}
function log(m){const e=document.getElementById('co');e.innerHTML+='<div>'+(typeof m==='string'?m:JSON.stringify(m).slice(0,200))+'</div>';e.scrollTop=e.scrollHeight}

// ── DOM tree rendering ──
function renderNode(n, depth) {
  if (n.tag==='#text') {
    if (!n.text||!n.text.trim()) return '';
    const t = n.text.trim().slice(0,50);
    return `<div class="tn" style="padding-left:${depth*16}px" onclick="inspect(${n.nid})"><span class="tx">"${esc(t)}"</span></div>`;
  }
  const has = n.children ? n.children.length > 0 : (n.count||0) > 0;
  const tog = has ? `<span class="tog" onclick="event.stopPropagation();toggle(this,${n.nid},${depth+1})">▶</span>` : '<span class="tog"> </span>';
  let attrs = '';
  if (n.id) attrs += ` <span class="an">id</span>=<span class="av">"${esc(n.id)}"</span>`;
  if (n.class) attrs += ` <span class="an">class</span>=<span class="av">"${esc(n.class)}"</span>`;
  const dim = `<span class="dim"> ${n.rect[2]}x${n.rect[3]}</span>`;
  let html = `<div class="tn" style="padding-left:${depth*16}px" data-nid="${n.nid}" onclick="inspect(${n.nid})">${tog}<span class="tag">&lt;${n.tag}</span>${attrs}<span class="tag">&gt;</span>${dim}</div>`;
  if (n.children) {
    html += `<div class="kids" data-parent="${n.nid}">`;
    for (const c of n.children) html += renderNode(c, depth+1);
    html += '</div>';
  } else if (has) {
    html += `<div class="kids" data-parent="${n.nid}" style="display:none"></div>`;
  }
  return html;
}

async function toggle(el, nid, depth) {
  const kids = el.closest('.tn').nextElementSibling;
  if (!kids) return;
  if (kids.style.display === 'none') {
    if (!kids.innerHTML) {
      kids.innerHTML = '<i style="padding-left:'+depth*16+'px;color:#666">Loading...</i>';
      // Fetch subtree rooted at this node
      const r = await api({cmd:'dom-tree', nid: nid, depth: 2});
      if (r.ok && r.tree && r.tree.children) {
        kids.innerHTML = r.tree.children.map(c => renderNode(c, depth)).join('');
      } else {
        kids.innerHTML = `<i style="padding-left:${depth*16}px;color:#666">(empty)</i>`;
      }
    }
    kids.style.display = '';
    el.textContent = '▼';
  } else {
    kids.style.display = 'none';
    el.textContent = '▶';
  }
}

async function loadTree() {
  const r = await api({cmd:'dom-tree', depth: 2});
  if (!r.ok) { document.getElementById('tree-panel').innerHTML='<i>Error</i>'; return; }
  document.getElementById('tree-panel').innerHTML = renderNode(r.tree, 0);
  log('DOM tree loaded');
}

let curNid = 0;
let curTab = 'box';
let curData = null;

async function inspect(nid) {
  document.querySelectorAll('.tn.sel').forEach(e=>e.classList.remove('sel'));
  const el = document.querySelector(`.tn[data-nid="${nid}"]`);
  if (el) el.classList.add('sel');
  curNid = nid;
  curData = await api({cmd:'inspect-node', nid: nid});
  if (!curData.ok) { document.getElementById('detail-panel').innerHTML = '<i>Not found</i>'; return; }
  renderDetail();
}

function switchTab(t) { curTab = t; renderDetail(); }

function renderDetail() {
  const r = curData;
  if (!r || !r.ok) return;
  const dp = document.getElementById('detail-panel');
  const tabs = ['box','computed','dom','layout','attrs','styles'];
  let html = `<div style="background:#252526;border-bottom:1px solid #3e3e3e;display:flex;font-size:11px">`;
  for (const t of tabs) {
    const active = t===curTab;
    html += `<div onclick="switchTab('${t}')" style="padding:5px 10px;cursor:pointer;${active?'color:#fff;border-bottom:2px solid #4fc3f7':'color:#888;border-bottom:2px solid transparent'}">${t}</div>`;
  }
  html += `</div>`;
  html += `<div style="padding:6px 10px;background:#2d2d30;border-bottom:1px solid #3e3e42;font:12px monospace">&lt;${esc(r.tag)}&gt; ${r.id?'#'+esc(r.id):''} ${r.class?'.'+esc(r.class.split(' ').join('.')):''}</div>`;
  html += `<div style="padding:8px;overflow:auto">`;

  if (curTab==='box') {
    const m=r.margin, b=r.border, p=r.padding, c=r.content;
    html += `<div class="bm"><div class="mb"><span class="lbl t">${m.top}</span><span class="lbl b">${m.bottom}</span><span class="lbl l">${m.left}</span><span class="lbl r">${m.right}</span>`;
    html += `<div class="bb"><span>border ${b.top} ${b.right} ${b.bottom} ${b.left}</span><br><div class="pb"><span>padding ${p.top} ${p.right} ${p.bottom} ${p.left}</span><br>`;
    html += `<div class="cb">${c.width} × ${c.height}</div></div></div></div></div>`;
    html += `<div style="text-align:center;color:#666;font-size:10px">position: (${c.x}, ${c.y})</div>`;
  }
  else if (curTab==='computed') {
    for (const [k,v] of Object.entries(r)) {
      if (['ok','cmd_ms','margin','border','padding','content'].includes(k)) continue;
      html += `<div class="prop"><span class="pn">${k}</span><span class="pv">${typeof v==='object'?JSON.stringify(v):v}</span></div>`;
    }
  }
  else if (curTab==='dom') {
    // Fetch tree rooted at this node
    html += `<div id="dom-sub"><i>Loading...</i></div>`;
    dp.innerHTML = html + '</div>';
    (async()=>{
      const t = await api({cmd:'dom-tree', nid: curNid, depth: 3});
      const sub = document.getElementById('dom-sub');
      if (!sub) return;
      if (t.ok && t.tree) {
        // Ancestor chain
        let chain = `<h3 style="color:#ccc;font-size:11px;border-bottom:1px solid #3e3e3e;padding-bottom:4px">Ancestors</h3>`;
        let cur = t.tree;
        // Show the node's tag path
        chain += `<div style="color:#4fc3f7;font-weight:600">&lt;${esc(t.tree.tag)}&gt; #${t.tree.id} .${t.tree.class}</div>`;
        chain += `<h3 style="color:#ccc;font-size:11px;border-bottom:1px solid #3e3e3e;padding-bottom:4px;margin-top:8px">Children (${t.tree.count||0})</h3>`;
        if (t.tree.children) {
          for (const c of t.tree.children) {
            const cid = c.id ? '#'+c.id : '';
            const ccls = c.class ? '.'+c.class.split(' ').slice(0,3).join('.') : '';
            if (c.tag==='#text' && c.text) {
              chain += `<div style="color:#6a9955;font-style:italic;padding:1px 0">"${esc(c.text.slice(0,50))}"</div>`;
            } else {
              chain += `<div style="padding:1px 0;cursor:pointer" onclick="inspect(${c.nid})"><span class="tag">&lt;${c.tag}&gt;</span>${cid}${ccls} <span style="color:#555">${c.rect[2]}x${c.rect[3]} (${c.count||0} children)</span></div>`;
            }
          }
        }
        sub.innerHTML = chain;
      } else { sub.innerHTML = '<i>Error loading</i>'; }
    })();
    return;
  }
  else if (curTab==='layout') {
    const c=r.content, m=r.margin;
    html += `<h3 style="color:#ccc;font-size:11px">Geometry</h3>`;
    html += `<div class="prop"><span class="pn">content</span><span class="pv">(${c.x}, ${c.y}) ${c.width} × ${c.height}</span></div>`;
    html += `<div class="prop"><span class="pn">display</span><span class="pv">${r.display}</span></div>`;
    html += `<div class="prop"><span class="pn">position</span><span class="pv">${r.position}</span></div>`;
    html += `<div class="prop"><span class="pn">font-size</span><span class="pv">${r.font_size}px</span></div>`;
    html += `<div class="prop"><span class="pn">node_id</span><span class="pv">${r.nid}</span></div>`;
  }
  else if (curTab==='attrs') {
    // Fetch deep info for attributes
    html += `<div id="attrs-sub"><i>Loading...</i></div>`;
    dp.innerHTML = html + '</div>';
    (async()=>{
      const d = await api({cmd:'deep', selector: '*'});
      const sub = document.getElementById('attrs-sub');
      if (!sub) return;
      // Find our node - deep returns all, we need to search
      // Just show what we have from inspect-node
      let h = '';
      h += `<div class="prop"><span class="pn">tag</span><span class="pv">${r.tag}</span></div>`;
      if (r.id) h += `<div class="prop"><span class="pn">id</span><span class="pv">${r.id}</span></div>`;
      if (r.class) h += `<div class="prop"><span class="pn">class</span><span class="pv" style="word-break:break-all">${esc(r.class)}</span></div>`;
      h += `<div class="prop"><span class="pn">color</span><span class="pv"><span style="display:inline-block;width:12px;height:12px;background:${r.color};border:1px solid #555;vertical-align:middle;margin-right:4px"></span>${r.color}</span></div>`;
      h += `<div class="prop"><span class="pn">background</span><span class="pv"><span style="display:inline-block;width:12px;height:12px;background:${r.bg};border:1px solid #555;vertical-align:middle;margin-right:4px"></span>${r.bg}</span></div>`;
      sub.innerHTML = h;
    })();
    return;
  }
  else if (curTab==='styles') {
    html += `<div style="color:#888;padding:8px">Matched CSS rules are shown in the F12 browser inspector (Styles tab).<br><br>Use the console to query: <code>{"cmd":"rules","selector":".class"}</code></div>`;
  }
  html += '</div>';
  dp.innerHTML = html;
}

function esc(s){return String(s).replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/"/g,'&quot;')}

async function doFind() {
  const s = document.getElementById('sel').value;
  const r = await api({cmd:'find',selector:s});
  log(`find "${s}": ${r.count} results`);
  if (r.ok && r.count > 0) {
    const p = document.getElementById('tree-panel');
    p.innerHTML = r.elements.map(el =>
      `<div class="tn" onclick="inspect(0)"><span class="tag">&lt;${el.tag}</span>` +
      (el.id?` <span class="an">id</span>=<span class="av">"${el.id}"</span>`:'')+
      (el.class?` <span class="an">class</span>=<span class="av">"${esc(el.class)}"</span>`:'')+
      `<span class="tag">&gt;</span> <span class="dim">${el.w}x${el.h} @${el.x},${el.y}</span></div>`
    ).join('');
  }
}
async function doScreenshot(){await api({cmd:'screenshot',out:'/tmp/inspector.png'});log('Saved /tmp/inspector.png')}
async function runCmd(){const i=document.getElementById('ci');try{log(await api(JSON.parse(i.value)))}catch(e){log('Error: '+e.message)}i.value=''}

// Auto-load tree on page open
loadTree();
</script>
</body>
</html>"##;

/// Headless mode: load a URL, serve debug commands on TCP, no window.
pub(super) fn run_headless(
    url: Option<String>,
    port: u16,
    width: f32,
    height: f32,
    cache_dir: Option<String>,
    no_images: bool,
    chrome_port: u16,
) {
    use std::io::{BufRead, Write};

    let url = normalize_url(url.unwrap_or_else(|| "about:blank".into()));
    eprintln!("[headless] Loading {} ({}x{})", url, width, height);

    // Launch Chrome if requested
    let mut _chrome_process: Option<std::process::Child> = None;
    if chrome_port > 0 {
        if let Some(chrome_path) = find_chrome() {
            eprintln!("[headless] Launching Chrome on port {} ...", chrome_port);
            match std::process::Command::new(&chrome_path)
                .arg(format!("--remote-debugging-port={}", chrome_port))
                .arg(format!("--window-size={},{}", width as u32, height as u32))
                .arg("--disable-extensions")
                .arg("--disable-gpu")
                .arg("--blink-settings=scriptEnabled=false")
                .arg("--no-first-run")
                .arg("--no-default-browser-check")
                .arg(format!(
                    "--user-data-dir=/tmp/browser-chrome-{}",
                    chrome_port
                ))
                .arg(format!("--app={}", url))
                .spawn()
            {
                Ok(child) => {
                    eprintln!("[headless] Chrome launched (pid {})", child.id());
                    _chrome_process = Some(child);
                }
                Err(e) => eprintln!("[headless] Chrome not found: {e}"),
            }
            std::thread::sleep(std::time::Duration::from_secs(2)); // wait for Chrome to start
        } else {
            eprintln!("[headless] Chrome not found");
        }
    }

    // Load document through the same browser view/widget used by the GUI path.
    let fetch_start = std::time::Instant::now();
    let mut view = crate::BrowserView::new(
        width,
        height,
        crate::PageLoadOptions {
            cache_dir: cache_dir.clone(),
            load_images: !no_images,
            emit_preview: true,
            ..Default::default()
        },
    );
    view.load_until_ready(url.clone(), std::time::Duration::from_secs(30));

    let load_ms = fetch_start.elapsed().as_millis();
    let (node_count, rule_count) = view
        .document()
        .as_deref()
        .map(|doc| (doc.root.child_count(), doc.stylesheet.rules.len()))
        .unwrap_or((0, 0));
    eprintln!(
        "[headless] Loaded in {}ms ({} nodes, {} rules)",
        load_ms, node_count, rule_count
    );
    eprintln!("[headless] Debug server on http://127.0.0.1:{}", port);

    // TCP listener — blocks main thread
    let listener = match std::net::TcpListener::bind(format!("127.0.0.1:{}", port)) {
        Ok(listener) => listener,
        Err(err) => {
            eprintln!("[headless] Could not bind debug server on 127.0.0.1:{port}: {err}");
            return;
        }
    };
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
        let mut writer = stream;
        let mut first_line = String::new();
        if reader.read_line(&mut first_line).is_err() {
            continue;
        }

        if first_line.starts_with("GET ") {
            // HTTP — serve inspector or API
            loop {
                let mut h = String::new();
                if reader.read_line(&mut h).is_err() || h.trim().is_empty() {
                    break;
                }
            }
            let path = first_line.split_whitespace().nth(1).unwrap_or("/");
            let (ct, body) = if path.starts_with("/api") {
                let query = path.split('?').nth(1).unwrap_or("");
                let cmd_json = urldecode(query.strip_prefix("cmd=").unwrap_or("{}"));
                let resp = dispatch_view_command(
                    &mut view,
                    width,
                    height,
                    &cmd_json,
                    &cache_dir,
                    chrome_port,
                );
                ("application/json", resp)
            } else {
                ("text/html", INSPECTOR_HTML.to_string())
            };
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {ct}\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = writer.write_all(resp.as_bytes());
        } else {
            // JSON protocol
            let line = first_line.trim().to_string();
            if !line.is_empty() {
                let resp =
                    dispatch_view_command(&mut view, width, height, &line, &cache_dir, chrome_port);
                let _ = writeln!(writer, "{}", resp);
                let _ = writer.flush();
            }
            for line in reader.lines() {
                let Ok(line) = line else { break };
                if line.trim().is_empty() {
                    continue;
                }
                let resp =
                    dispatch_view_command(&mut view, width, height, &line, &cache_dir, chrome_port);
                let _ = writeln!(writer, "{}", resp);
                let _ = writer.flush();
            }
        }
    }
}

/// Quote a string as a JSON scalar.
pub(super) fn json_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

pub(super) fn paint_dump_image_stats_json(data: &ImageRef) -> String {
    let (bytes, w, h) = match data {
        ImageRef::Owned(bytes, w, h) => (bytes.as_slice(), *w, *h),
        ImageRef::Shared(bytes, w, h) => (bytes.as_slice(), *w, *h),
    };
    let mut nontransparent = 0usize;
    let mut min_x = w;
    let mut min_y = h;
    let mut max_x = 0u32;
    let mut max_y = 0u32;
    let mut color_sum = [0usize; 3];
    for (i, px) in bytes.chunks_exact(4).enumerate() {
        if px[3] == 0 {
            continue;
        }
        let x = (i as u32) % w;
        let y = (i as u32) / w;
        nontransparent += 1;
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
        color_sum[0] += px[0] as usize;
        color_sum[1] += px[1] as usize;
        color_sum[2] += px[2] as usize;
    }
    let bounds = if nontransparent == 0 {
        "null".to_string()
    } else {
        format!("[{min_x},{min_y},{max_x},{max_y}]")
    };
    let avg = if nontransparent == 0 {
        "[0,0,0]".to_string()
    } else {
        format!(
            "[{},{},{}]",
            color_sum[0] / nontransparent,
            color_sum[1] / nontransparent,
            color_sum[2] / nontransparent
        )
    };
    format!(
        r#""img_w":{},"img_h":{},"img_bytes":{},"nontransparent":{},"content_bounds":{},"avg_rgb":{}"#,
        w,
        h,
        bytes.len(),
        nontransparent,
        bounds,
        avg
    )
}

/// Format a layout coordinate for the debug protocol: the exact value, with
/// the trailing `.0` dropped so whole pixels stay readable.
pub(super) fn fmt_px(v: f32) -> String {
    if (v - v.round()).abs() < 0.001 {
        format!("{}", v.round() as i64)
    } else {
        format!("{:.3}", v)
    }
}

pub(super) fn animated_images_json(doc: &Document, viewport_h: f32) -> String {
    fn hidden_by_style(node: &crate::WebCore) -> bool {
        matches!(node.style.display, Display::None)
            || !node.style.visibility
            || node.style.opacity <= 0.0
    }

    fn interval(node: &crate::WebCore, scroll_y: f32) -> (f32, f32) {
        let r = node.layout.margin_rect;
        if matches!(node.style.position, Position::Fixed) {
            (scroll_y + r.y, scroll_y + r.y + r.h)
        } else {
            (r.y, r.y + r.h)
        }
    }

    fn child_clip(
        node: &crate::WebCore,
        scroll_y: f32,
        clip_top: f32,
        clip_bottom: f32,
    ) -> (f32, f32) {
        if matches!(
            node.style.overflow_y,
            Overflow::Hidden | Overflow::Clip | Overflow::Scroll | Overflow::Auto
        ) {
            let r = node.layout.content_rect;
            if r.h <= 0.0 {
                return (1.0, 0.0);
            }
            let (top, bottom) = if matches!(node.style.position, Position::Fixed) {
                (scroll_y + r.y, scroll_y + r.y + r.h)
            } else {
                (r.y, r.y + r.h)
            };
            (clip_top.max(top), clip_bottom.min(bottom))
        } else {
            (clip_top, clip_bottom)
        }
    }

    fn walk(
        node: &crate::WebCore,
        scroll_y: f32,
        viewport_h: f32,
        inherited_visible: bool,
        clip_top: f32,
        clip_bottom: f32,
        out: &mut Vec<String>,
    ) {
        let visible_branch = inherited_visible && !hidden_by_style(node);
        let (top, bottom) = interval(node, scroll_y);
        let r = node.layout.margin_rect;
        if let Some(animated) = node.animated_image.as_ref() {
            let visible = visible_branch
                && r.w > 0.0
                && r.h > 0.0
                && bottom >= scroll_y
                && top <= scroll_y + viewport_h
                && bottom >= clip_top
                && top <= clip_bottom;
            let src = node
                .attributes
                .get("src")
                .or_else(|| node.attributes.get("data-src"))
                .map(|s| s.as_str())
                .unwrap_or("");
            let class = node
                .attributes
                .get("class")
                .map(|s| s.as_str())
                .unwrap_or("");
            out.push(format!(
                r#"{{"nid":{},"tag":{},"class":{},"src":{},"visible":{},"frames":{},"frame":{},"x":{},"y":{},"w":{},"h":{},"clip_top":{},"clip_bottom":{}}}"#,
                node.node_id,
                dbg_json_escape(&node.tag),
                dbg_json_escape(class),
                dbg_json_escape(src),
                visible,
                animated.frames.len(),
                node.animated_image_frame,
                fmt_px(r.x),
                fmt_px(r.y),
                fmt_px(r.w),
                fmt_px(r.h),
                fmt_px(clip_top),
                fmt_px(clip_bottom)
            ));
        }
        if !visible_branch {
            return;
        }
        let (child_clip_top, child_clip_bottom) = child_clip(node, scroll_y, clip_top, clip_bottom);
        if child_clip_bottom < child_clip_top {
            return;
        }
        for child in &node.children {
            walk(
                child,
                scroll_y,
                viewport_h,
                visible_branch,
                child_clip_top,
                child_clip_bottom,
                out,
            );
        }
    }

    let mut items = Vec::new();
    walk(
        &doc.root,
        doc.scroll_y,
        viewport_h,
        true,
        doc.scroll_y,
        doc.scroll_y + viewport_h,
        &mut items,
    );
    let visible = items
        .iter()
        .filter(|item| item.contains(r#""visible":true"#))
        .count();
    format!(
        r#"{{"ok":true,"count":{},"visible":{},"items":[{}]}}"#,
        items.len(),
        visible,
        items.join(",")
    )
}

pub(super) fn image_states_json(doc: &Document, limit: usize) -> String {
    fn walk(node: &crate::WebCore, out: &mut Vec<String>, limit: usize) {
        if out.len() >= limit {
            return;
        }
        let has_element_image = node.image_data.is_some();
        let has_background = node.bg_image_data.is_some();
        let has_mask = node
            .mask_images
            .as_ref()
            .and_then(|images| images.first.as_ref())
            .is_some();
        let is_image_like = node.is_image_element()
            || node.tag == "video"
            || has_element_image
            || has_background
            || has_mask
            || !node.style.background_image_url.is_empty()
            || !node.style.rare().mask_image_url.is_empty();
        if is_image_like {
            let r = node.layout.border_rect;
            let src = if !node.resolved_src.is_empty() {
                node.resolved_src.as_str()
            } else {
                node.attributes
                    .get("src")
                    .or_else(|| node.attributes.get("poster"))
                    .or_else(|| node.attributes.get("data-src"))
                    .map(|s| s.as_str())
                    .unwrap_or("")
            };
            out.push(format!(
                concat!(
                    r#"{{"nid":{},"tag":{},"class":{},"src":{},"srcset":{},"#,
                    r#""element_loaded":{},"element_size":[{},{}],"element_bytes":{},"#,
                    r#""element_fallback":{},"buffer_size":[{},{}],"#,
                    r#""background_url":{},"background_loaded":{},"background_size":[{},{}],"#,
                    r#""mask_url":{},"mask_loaded":{},"mask_size":[{},{}],"#,
                    r#""rect":[{},{},{},{}]}}"#
                ),
                node.node_id,
                dbg_json_escape(&node.tag),
                dbg_json_escape(
                    node.attributes
                        .get("class")
                        .map(String::as_str)
                        .unwrap_or("")
                ),
                dbg_json_escape(src),
                dbg_json_escape(
                    node.attributes
                        .get("srcset")
                        .map(String::as_str)
                        .unwrap_or("")
                ),
                has_element_image,
                node.image_width,
                node.image_height,
                node.image_data.as_ref().map(|d| d.len()).unwrap_or(0),
                node.image_is_fallback,
                node.image_data_width,
                node.image_data_height,
                dbg_json_escape(&node.style.background_image_url),
                has_background,
                node.bg_image_width,
                node.bg_image_height,
                dbg_json_escape(&node.style.rare().mask_image_url),
                has_mask,
                node.mask_images
                    .as_ref()
                    .and_then(|images| images.first.as_ref())
                    .map_or(0, |image| image.width),
                node.mask_images
                    .as_ref()
                    .and_then(|images| images.first.as_ref())
                    .map_or(0, |image| image.height),
                fmt_px(r.x),
                fmt_px(r.y),
                fmt_px(r.w),
                fmt_px(r.h)
            ));
        }
        for child in &node.children {
            walk(child, out, limit);
            if out.len() >= limit {
                break;
            }
        }
    }

    let mut items = Vec::new();
    walk(&doc.root, &mut items, limit);
    let errors: Vec<String> = doc
        .image_load_errors
        .iter()
        .rev()
        .take(limit)
        .map(|(path, target, url, error)| {
            format!(
                r#"{{"path":{},"target":"{:?}","url":{},"error":{}}}"#,
                dbg_json_escape(&format!("{path:?}")),
                target,
                dbg_json_escape(url),
                dbg_json_escape(error)
            )
        })
        .collect();
    format!(
        r#"{{"ok":true,"count":{},"errors":{},"pending_channel":{},"in_flight":{},"images":[{}],"load_errors":[{}]}}"#,
        items.len(),
        doc.image_load_errors.len(),
        doc.pending_images.is_some(),
        doc.images_in_flight
            .load(std::sync::atomic::Ordering::SeqCst),
        items.join(","),
        errors.join(",")
    )
}

pub(super) fn animations_json(doc: &Document, viewport_h: f32) -> String {
    let mut items = Vec::new();
    for state in &doc.active_animations {
        let props = doc
            .stylesheet
            .keyframes
            .get(&state.animation.name)
            .map(|stops| {
                let mut props = Vec::<String>::new();
                for stop in stops.iter() {
                    for (prop, _) in &stop.properties {
                        if !props.iter().any(|p| p == prop) {
                            props.push(prop.clone());
                        }
                    }
                }
                props
            })
            .unwrap_or_default();
        let node = doc.get_box_by_id(state.element_id);
        let tag = node.map(|n| n.tag.as_str()).unwrap_or("");
        let rect = node.map(|n| n.layout.margin_rect).unwrap_or_default();
        let current: Vec<String> = doc
            .animation_overrides_for(state.element_id)
            .map(|props| {
                props
                    .iter()
                    .map(|(name, value)| {
                        format!("{}:{}", dbg_json_escape(name), dbg_json_escape(value))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let visible = node.is_some_and(|n| {
            !matches!(n.style.display, Display::None)
                && n.style.visibility
                && n.style.opacity > 0.0
                && rect.w > 0.0
                && rect.h > 0.0
                && rect.y + rect.h >= doc.scroll_y
                && rect.y <= doc.scroll_y + viewport_h
        });
        items.push(format!(
            r#"{{"nid":{},"tag":{},"name":{},"visible":{},"x":{},"y":{},"w":{},"h":{},"duration_ms":{},"delay_ms":{},"iterations":{},"paused":{},"properties":[{}],"current":{{{}}}}}"#,
            state.element_id,
            dbg_json_escape(tag),
            dbg_json_escape(&state.animation.name),
            visible,
            fmt_px(rect.x),
            fmt_px(rect.y),
            fmt_px(rect.w),
            fmt_px(rect.h),
            state.animation.duration_ms,
            state.animation.delay_ms,
            state.animation.iteration_count,
            state.animation.play_state_paused,
            props
                .iter()
                .map(|p| dbg_json_escape(p))
                .collect::<Vec<_>>()
                .join(","),
            current.join(",")
        ));
    }
    format!(
        r#"{{"ok":true,"needs_frame":{},"active":{},"items":[{}]}}"#,
        doc.needs_animation_frame,
        doc.active_animations.len(),
        items.join(",")
    )
}

pub(super) fn display_list_stats_json(
    list: &crate::renderer::display_list::DisplayList,
    paint_band: Option<(f32, f32)>,
    viewport_w: f32,
    doc_h: f32,
) -> String {
    let mut fills = 0usize;
    let mut borders = 0usize;
    let mut text = 0usize;
    let mut images = 0usize;
    let mut clips = 0usize;
    let mut clip_paths = 0usize;
    let mut transforms = 0usize;
    let mut layers = 0usize;
    let mut masks = 0usize;
    let mut opacity_layers = 0usize;
    let mut filter_layers = 0usize;
    let mut blend_layers = 0usize;
    let mut fixed_boundaries = 0usize;
    for cmd in list.commands.iter().chain(list.fixed_commands.iter()) {
        match cmd {
            PaintCmd::FillRect { .. } => fills += 1,
            PaintCmd::Border { .. } | PaintCmd::BorderImage { .. } => borders += 1,
            PaintCmd::Text { .. } => text += 1,
            PaintCmd::Image { .. }
            | PaintCmd::BackgroundImage { .. }
            | PaintCmd::ListMarker { image: Some(_), .. } => images += 1,
            PaintCmd::PushClip { .. } => clips += 1,
            PaintCmd::PushClipPath { .. } | PaintCmd::PushClipSvgPath { .. } => clip_paths += 1,
            PaintCmd::PushTransform { .. } => transforms += 1,
            PaintCmd::PushOpacity { .. } => {
                layers += 1;
                opacity_layers += 1;
            }
            PaintCmd::PushFilter { .. } => {
                layers += 1;
                filter_layers += 1;
            }
            PaintCmd::PushBlendMode { .. } => {
                layers += 1;
                blend_layers += 1;
            }
            PaintCmd::PushMask { .. } => masks += 1,
            PaintCmd::BeginFixedPosition => fixed_boundaries += 1,
            _ => {}
        }
    }
    let segmented = (fixed_boundaries > 0).then(|| {
        crate::renderer::compositor::PaintSegments::from_display_list(list, viewport_w, doc_h)
            .is_some()
    });
    let band_json = paint_band
        .map(|(top, bottom)| format!(r#","paint_top":{top:.1},"paint_bottom":{bottom:.1}"#))
        .unwrap_or_default();
    format!(
        r#"{{"ok":true,"commands":{},"fixed_commands":{},"fixed_boundaries":{},"segmented":{},"fills":{},"borders":{},"text":{},"images":{},"clips":{},"clip_paths":{},"transforms":{},"layers":{},"opacity_layers":{},"filter_layers":{},"blend_layers":{},"masks":{}{}}}"#,
        list.commands.len(),
        list.fixed_commands.len(),
        fixed_boundaries,
        segmented.map_or("null", |value| if value { "true" } else { "false" }),
        fills,
        borders,
        text,
        images,
        clips,
        clip_paths,
        transforms,
        layers,
        opacity_layers,
        filter_layers,
        blend_layers,
        masks,
        band_json
    )
}

#[derive(Clone, Copy, Debug)]
pub(super) struct ProcessMemoryStats {
    pub(super) rss_bytes: u64,
    pub(super) vsz_bytes: u64,
}

pub(super) fn process_memory_stats() -> Option<ProcessMemoryStats> {
    let pid = std::process::id().to_string();
    let output = std::process::Command::new("ps")
        .args(["-o", "rss=,vsz=", "-p", &pid])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let mut parts = text.split_whitespace();
    let rss_kib = parts.next()?.parse::<u64>().ok()?;
    let vsz_kib = parts.next()?.parse::<u64>().ok()?;
    Some(ProcessMemoryStats {
        rss_bytes: rss_kib.saturating_mul(1024),
        vsz_bytes: vsz_kib.saturating_mul(1024),
    })
}

pub(super) fn append_cmd_ms(result: String, started: std::time::Instant) -> String {
    let ms = started.elapsed().as_micros() as f64 / 1000.0;
    if result.ends_with('}') {
        format!(
            "{}{}\"cmd_ms\":{:.2}}}",
            &result[..result.len() - 1],
            if result.len() > 2 { "," } else { "" },
            ms
        )
    } else {
        result
    }
}

pub(super) fn dispatch_view_input_command(
    view: &mut crate::BrowserView,
    cmd: &str,
    line: &str,
    width: f32,
    height: f32,
) -> String {
    match cmd {
        "screenshot" => {
            let path = dbg_json_str(line, "out").unwrap_or_else(|| "snapshot.png".to_string());
            let scale = dbg_json_num(line, "scale")
                .map(|v| v.clamp(0.25, 4.0) as f32)
                .unwrap_or(1.0);
            let phys_w = (width * scale).ceil().max(1.0) as u32;
            let phys_h = (height * scale).ceil().max(1.0) as u32;
            let Some(mut pm) = tiny_skia::Pixmap::new(phys_w, phys_h) else {
                return r#"{"ok":false,"error":"pixmap failed"}"#.to_string();
            };
            pm.fill(tiny_skia::Color::WHITE);
            view.resize(width, height);
            view.paint_into(&mut pm, 0, 0, scale);
            match pm.save_png(&path) {
                Ok(_) => format!(
                    r#"{{"ok":true,"path":"{}","width":{},"height":{},"scale":{}}}"#,
                    path, phys_w, phys_h, scale
                ),
                Err(e) => format!(r#"{{"ok":false,"error":"{}"}}"#, e),
            }
        }
        "scroll" => {
            if let Some(y) = dbg_json_num(line, "y") {
                view.scroll_to(0.0, y as f32);
            } else {
                let dy = dbg_json_num(line, "dy").unwrap_or(0.0) as f32;
                view.scroll_by(0.0, dy);
            }
            format!(r#"{{"ok":true,"scroll_y":{:.0}}}"#, view.scroll_y())
        }
        "click" => {
            let coords = if let Some(sel) = dbg_json_str(line, "selector") {
                view.document()
                    .as_deref()
                    .and_then(|doc| dbg_selector_center(doc, &sel))
                    .ok_or_else(|| {
                        format!(
                            r#"{{"ok":false,"error":"no element matches {}"}}"#,
                            dbg_json_escape(&sel)
                        )
                    })
            } else if let (Some(x), Some(y)) = (dbg_json_num(line, "x"), dbg_json_num(line, "y")) {
                Ok((x, y))
            } else {
                return r#"{"ok":false,"error":"click needs x,y or selector"}"#.to_string();
            };
            let (x, y) = match coords {
                Ok(c) => c,
                Err(e) => return e,
            };
            for kind in [
                crate::dom::HtmlEventType::MouseDown,
                crate::dom::HtmlEventType::MouseUp,
            ] {
                view.handle_mouse_button_with_modifiers(
                    kind,
                    x,
                    y,
                    0,
                    dbg_json_bool(line, "ctrl").unwrap_or(false),
                    dbg_json_bool(line, "shift").unwrap_or(false),
                    dbg_json_bool(line, "alt").unwrap_or(false),
                    dbg_json_bool(line, "meta").unwrap_or(false),
                );
            }
            format!(r#"{{"ok":true,"x":{:.0},"y":{:.0}}}"#, x, y)
        }
        "hover" => {
            let coords = if let Some(sel) = dbg_json_str(line, "selector") {
                view.document()
                    .as_deref()
                    .and_then(|doc| dbg_selector_center(doc, &sel))
                    .ok_or_else(|| {
                        format!(
                            r#"{{"ok":false,"error":"no element matches {}"}}"#,
                            dbg_json_escape(&sel)
                        )
                    })
            } else if let (Some(x), Some(y)) = (dbg_json_num(line, "x"), dbg_json_num(line, "y")) {
                Ok((x, y))
            } else {
                return r#"{"ok":false,"error":"hover needs x,y or selector"}"#.to_string();
            };
            let (x, y) = match coords {
                Ok(c) => c,
                Err(e) => return e,
            };
            let changed = view.handle_mouse_move(x, y);
            format!(r#"{{"ok":true,"changed":{}}}"#, changed)
        }
        "type" => match dbg_json_str(line, "text") {
            Some(text) => {
                let mut any = false;
                for ch in text.chars() {
                    any |= view.handle_key(
                        crate::dom::HtmlEventType::KeyDown,
                        ch as u32,
                        Some(ch),
                        false,
                        false,
                        false,
                        false,
                    );
                }
                format!(r#"{{"ok":true,"typed":{}}}"#, any)
            }
            None => r#"{"ok":false,"error":"type needs text"}"#.to_string(),
        },
        "key" => dbg_send_key(view, line),
        "inspect-mode" => {
            let on = dbg_json_str(line, "on")
                .map(|v| !matches!(v.as_str(), "false" | "0"))
                .or_else(|| dbg_json_num(line, "on").map(|v| v != 0.0))
                .unwrap_or(true);
            view.set_inspect_mode(on);
            format!(r#"{{"ok":true,"inspect_mode":{}}}"#, on)
        }
        "resize" => {
            view.resize(width, height);
            r#"{"ok":true}"#.to_string()
        }
        "bench-progressive" => match view.benchmark_progressive_layout() {
            Some((full, above)) => format!(
                r#"{{"ok":true,"full_ms":{:.1},"above_fold_ms":{:.1}}}"#,
                full, above
            ),
            None => r#"{"ok":false,"error":"no document"}"#.to_string(),
        },
        "bench-render" => {
            let scale = dbg_json_num(line, "scale")
                .map(|v| v.clamp(0.25, 4.0) as f32)
                .unwrap_or(1.0);
            let dy = dbg_json_num(line, "dy").unwrap_or(500.0) as f32;
            let phys_w = (width * scale).ceil().max(1.0) as u32;
            let phys_h = (height * scale).ceil().max(1.0) as u32;
            let Some(mut pm) = tiny_skia::Pixmap::new(phys_w, phys_h) else {
                return r#"{"ok":false,"error":"pixmap failed"}"#.to_string();
            };
            let original_y = view.scroll_y();
            let t0 = std::time::Instant::now();
            view.paint_into(&mut pm, 0, 0, scale);
            let first = t0.elapsed().as_micros() as f64 / 1000.0;
            view.scroll_by(0.0, dy);
            let t1 = std::time::Instant::now();
            view.paint_into(&mut pm, 0, 0, scale);
            let scrolled = t1.elapsed().as_micros() as f64 / 1000.0;
            let t2 = std::time::Instant::now();
            view.paint_into(&mut pm, 0, 0, scale);
            let cached = t2.elapsed().as_micros() as f64 / 1000.0;
            view.scroll_to(0.0, original_y);
            format!(
                r#"{{"ok":true,"first_ms":{:.1},"scroll_ms":{:.1},"cached_ms":{:.1},"width":{},"height":{},"scale":{}}}"#,
                first, scrolled, cached, phys_w, phys_h, scale
            )
        }
        _ => r#"{"ok":false,"error":"unsupported browser command"}"#.to_string(),
    }
}

pub(super) fn dispatch_view_command(
    view: &mut crate::BrowserView,
    width: f32,
    height: f32,
    line: &str,
    _cache_dir: &Option<String>,
    chrome_port: u16,
) -> String {
    let cmd_start = std::time::Instant::now();
    let cmd = dbg_json_str(line, "cmd").unwrap_or_default();
    if cmd == "profile" {
        if dbg_json_bool(line, "reset").unwrap_or(false) {
            crate::profile::reset();
        }
        return append_cmd_ms(
            profile_json(dbg_json_num(line, "limit").unwrap_or(20.0) as usize),
            cmd_start,
        );
    }
    if cmd == "navigate" {
        let result = if let Some(new_url) = dbg_json_str(line, "url") {
            let new_url = normalize_url(new_url);
            crate::profile::reset();
            view.navigate(new_url.clone());
            format!(r#"{{"ok":true,"url":"{}"}}"#, view.url())
        } else {
            r#"{"ok":false,"error":"need url"}"#.to_string()
        };
        return append_cmd_ms(result, cmd_start);
    }
    if matches!(
        cmd.as_str(),
        "screenshot"
            | "scroll"
            | "click"
            | "hover"
            | "type"
            | "key"
            | "inspect-mode"
            | "resize"
            | "bench-progressive"
            | "bench-render"
    ) {
        return append_cmd_ms(
            dispatch_view_input_command(view, &cmd, line, width, height),
            cmd_start,
        );
    }

    let Some((mut document, renderer)) = view.document_and_renderer_mut() else {
        return r#"{"ok":false,"error":"no document"}"#.to_string();
    };
    let doc = &mut *document;
    let result = match cmd.as_str() {
        "step" | "tick" => {
            let ms = dbg_json_num(line, "ms").unwrap_or(100.0);
            if ms > 0.0 {
                std::thread::sleep(std::time::Duration::from_millis(ms as u64));
            }
            renderer.layout_engine().viewport_h = height;
            renderer.layout_engine().layout(doc, width);
            renderer.invalidate_display_list();
            format!(r#"{{"ok":true,"ms":{}}}"#, ms)
        }
        "control-state" => {
            dbg_control_state(doc, &dbg_json_str(line, "selector").unwrap_or_default())
        }
        "find" => {
            let sel = dbg_json_str(line, "selector").unwrap_or_default();
            let nodes = crate::dom::query_selector_all(&doc.root, &sel);
            let items: Vec<String> = nodes
                .iter()
                .map(|n| {
                    let b = n.layout.border_rect;
                    format!(
                        r#"{{"node_id":{},"tag":"{}","id":"{}","class":"{}","x":{},"y":{},"w":{},"h":{}}}"#,
                        n.node_id,
                        n.tag,
                        n.attributes.get("id").unwrap_or(&String::new()),
                        n.attributes.get("class").unwrap_or(&String::new()),
                        // ⛔ Not `as i32`. Truncating the rect threw away the
                        // sub-pixel part, so a box at 38.7 reported 38 and every
                        // comparison against a browser — which rounds — was off by
                        // one for no reason. Layout works in fractional pixels;
                        // an inspector that hides them cannot be used to diff.
                        fmt_px(b.x),
                        fmt_px(b.y),
                        fmt_px(b.w),
                        fmt_px(b.h)
                    )
                })
                .collect();
            format!(
                r#"{{"ok":true,"count":{},"elements":[{}]}}"#,
                items.len(),
                items.join(",")
            )
        }
        "height-dump" => {
            let limit = dbg_json_num(line, "limit").unwrap_or(40.0).max(1.0) as usize;
            dbg_height_dump_json(&doc.root, limit)
        }
        "display-list-stats" => {
            let band = crate::renderer::retained_paint_band_for_doc(doc, width, height);
            let paint_top = band.y;
            let paint_bottom = band.bottom();
            let list = build_display_list_viewport(
                &doc.root,
                width,
                height,
                doc.scroll_x,
                doc.scroll_y,
                paint_top,
                paint_bottom,
                0,
                0,
                &std::collections::HashSet::new(),
                &doc.base_url,
            );
            display_list_stats_json(
                &list,
                Some((paint_top, paint_bottom)),
                width,
                Document::scroll_height(&doc.root),
            )
        }
        "animated-images" => animated_images_json(doc, height),
        "image-states" => {
            let limit = dbg_json_num(line, "limit").unwrap_or(80.0).max(1.0) as usize;
            image_states_json(doc, limit)
        }
        "animations" => animations_json(doc, height),
        "paint-dump" => {
            let x = dbg_json_num(line, "x").unwrap_or(0.0);
            let y = dbg_json_num(line, "y").unwrap_or(0.0);
            let w = dbg_json_num(line, "w").unwrap_or(width);
            let h = dbg_json_num(line, "h").unwrap_or(height);
            let limit = dbg_json_num(line, "limit").unwrap_or(80.0).max(1.0) as usize;
            let qx2 = x + w.max(0.0);
            let qy2 = y + h.max(0.0);
            let font_system = Some(&mut renderer.font_system as *mut _);
            let list = build_display_list_full_with_font_system(
                &doc.root,
                width,
                height,
                doc.scroll_x,
                doc.scroll_y,
                0,
                0,
                &std::collections::HashSet::new(),
                &doc.base_url,
                font_system,
            );
            let mut out = Vec::new();
            for cmd in &list.commands {
                if out.len() >= limit {
                    break;
                }
                match cmd {
                    PaintCmd::FillRect {
                        rect,
                        color,
                        radius,
                        radius_y,
                    } => {
                        if rect.x <= qx2
                            && rect.right() >= x
                            && rect.y <= qy2
                            && rect.bottom() >= y
                            && color.a > 0
                        {
                            out.push(format!(
                                r##"{{"kind":"fill","x":{:.1},"y":{:.1},"w":{:.1},"h":{:.1},"color":"#{:02x}{:02x}{:02x}{:02x}","radius":[{:.1},{:.1},{:.1},{:.1}],"radius_y":[{:.1},{:.1},{:.1},{:.1}]}}"##,
                                rect.x, rect.y, rect.w, rect.h,
                                color.r, color.g, color.b, color.a,
                                radius[0], radius[1], radius[2], radius[3],
                                radius_y[0], radius_y[1], radius_y[2], radius_y[3]
                            ));
                        }
                    }
                    PaintCmd::Border {
                        rect,
                        widths,
                        colors,
                        styles,
                        ..
                    } => {
                        if rect.x <= qx2
                            && rect.right() >= x
                            && rect.y <= qy2
                            && rect.bottom() >= y
                            && widths.iter().any(|w| *w > 0.0)
                        {
                            out.push(format!(
                                r##"{{"kind":"border","x":{:.1},"y":{:.1},"w":{:.1},"h":{:.1},"widths":[{:.1},{:.1},{:.1},{:.1}],"colors":["#{:02x}{:02x}{:02x}{:02x}","#{:02x}{:02x}{:02x}{:02x}","#{:02x}{:02x}{:02x}{:02x}","#{:02x}{:02x}{:02x}{:02x}"],"styles":[{},{},{},{}]}}"##,
                                rect.x, rect.y, rect.w, rect.h,
                                widths[0], widths[1], widths[2], widths[3],
                                colors[0].r, colors[0].g, colors[0].b, colors[0].a,
                                colors[1].r, colors[1].g, colors[1].b, colors[1].a,
                                colors[2].r, colors[2].g, colors[2].b, colors[2].a,
                                colors[3].r, colors[3].g, colors[3].b, colors[3].a,
                                styles[0], styles[1], styles[2], styles[3]
                            ));
                        }
                    }
                    PaintCmd::Outline {
                        rect,
                        width,
                        color,
                        style,
                        offset,
                        ..
                    } => {
                        if rect.x <= qx2
                            && rect.right() >= x
                            && rect.y <= qy2
                            && rect.bottom() >= y
                            && *width > 0.0
                            && color.a > 0
                        {
                            out.push(format!(
                                r##"{{"kind":"outline","x":{:.1},"y":{:.1},"w":{:.1},"h":{:.1},"width":{:.1},"color":"#{:02x}{:02x}{:02x}{:02x}","style":{},"offset":{:.1}}}"##,
                                rect.x, rect.y, rect.w, rect.h,
                                width,
                                color.r, color.g, color.b, color.a,
                                style,
                                offset
                            ));
                        }
                    }
                    PaintCmd::Text {
                        x: tx,
                        y: ty,
                        text,
                        font_family,
                        font_size,
                        font_weight,
                        decoration,
                        letter_spacing,
                        word_spacing,
                        ..
                    } => {
                        if *tx <= qx2 && *tx + 1200.0 >= x && *ty <= qy2 && *ty + *font_size >= y {
                            out.push(format!(
                                r#"{{"kind":"text","x":{:.1},"y":{:.1},"font":{},"size":{:.1},"weight":{},"letter_spacing":{:.3},"word_spacing":{:.3},"underline":{},"overline":{},"strikethrough":{},"text":{}}}"#,
                                tx,
                                ty,
                                dbg_json_escape(font_family),
                                font_size,
                                font_weight,
                                letter_spacing,
                                word_spacing,
                                decoration.underline,
                                decoration.overline,
                                decoration.strikethrough,
                                dbg_json_escape(&text.chars().take(120).collect::<String>())
                            ));
                        }
                    }
                    PaintCmd::TextShadow {
                        x: tx,
                        y: ty,
                        text,
                        font_family,
                        font_size,
                        ..
                    } => {
                        if *tx <= qx2 && *tx + 1200.0 >= x && *ty <= qy2 && *ty + *font_size >= y {
                            out.push(format!(
                                r#"{{"kind":"text-shadow","x":{:.1},"y":{:.1},"font":{},"size":{:.1},"text":{}}}"#,
                                tx,
                                ty,
                                dbg_json_escape(font_family),
                                font_size,
                                dbg_json_escape(&text.chars().take(120).collect::<String>())
                            ));
                        }
                    }
                    PaintCmd::Image { rect, data } => {
                        if rect.x <= qx2 && rect.right() >= x && rect.y <= qy2 && rect.bottom() >= y
                        {
                            let stats = paint_dump_image_stats_json(data);
                            out.push(format!(
                                r#"{{"kind":"image","x":{:.1},"y":{:.1},"w":{:.1},"h":{:.1},{}}}"#,
                                rect.x, rect.y, rect.w, rect.h, stats
                            ));
                        }
                    }
                    PaintCmd::BackgroundImage {
                        container,
                        clip,
                        draw_w,
                        draw_h,
                        pos_x,
                        pos_y,
                        ..
                    } => {
                        if clip.x <= qx2 && clip.right() >= x && clip.y <= qy2 && clip.bottom() >= y
                        {
                            out.push(format!(
                                r#"{{"kind":"background-image","container":[{:.1},{:.1},{:.1},{:.1}],"clip":[{:.1},{:.1},{:.1},{:.1}],"draw_w":{:.1},"draw_h":{:.1},"pos_x":{:.1},"pos_y":{:.1}}}"#,
                                container.x, container.y, container.w, container.h,
                                clip.x, clip.y, clip.w, clip.h,
                                draw_w, draw_h, pos_x, pos_y
                            ));
                        }
                    }
                    PaintCmd::Gradient {
                        rect,
                        clip,
                        gradient_type,
                        angle,
                        stops,
                        ..
                    } => {
                        if clip.x <= qx2 && clip.right() >= x && clip.y <= qy2 && clip.bottom() >= y
                        {
                            out.push(format!(
                                r#"{{"kind":"gradient","x":{:.1},"y":{:.1},"w":{:.1},"h":{:.1},"clip":[{:.1},{:.1},{:.1},{:.1}],"gradient_type":{},"angle":{:.1},"stops":{},"resolved_stops":{}}}"#,
                                rect.x, rect.y, rect.w, rect.h,
                                clip.x, clip.y, clip.w, clip.h,
                                gradient_type, angle, stops.len(),
                                format!("[{}]", stops.iter().map(|(color, position)|
                                    format!(r#"{{"rgba":[{},{},{},{}],"position":{}}}"#, color.r, color.g, color.b, color.a, position)
                                ).collect::<Vec<_>>().join(","))
                            ));
                        }
                    }
                    PaintCmd::BoxShadow {
                        rect,
                        color,
                        offset_x,
                        offset_y,
                        blur,
                        spread,
                        inset,
                        ..
                    } => {
                        let sx = rect.x + offset_x - spread - blur * 4.0;
                        let sy = rect.y + offset_y - spread - blur * 4.0;
                        let sw = rect.w + spread * 2.0 + blur * 8.0;
                        let sh = rect.h + spread * 2.0 + blur * 8.0;
                        if sx <= qx2 && sx + sw >= x && sy <= qy2 && sy + sh >= y {
                            out.push(format!(
                                r##"{{"kind":"box-shadow","x":{:.1},"y":{:.1},"w":{:.1},"h":{:.1},"color":"#{:02x}{:02x}{:02x}{:02x}","offset_x":{:.1},"offset_y":{:.1},"blur":{:.1},"spread":{:.1},"inset":{}}}"##,
                                rect.x, rect.y, rect.w, rect.h,
                                color.r, color.g, color.b, color.a,
                                offset_x, offset_y, blur, spread, inset
                            ));
                        }
                    }
                    PaintCmd::BeginStackingContext { node_id, z_index } => {
                        out.push(format!(
                            r#"{{"kind":"begin-stacking-context","node_id":{},"z_index":{}}}"#,
                            node_id, z_index
                        ));
                    }
                    PaintCmd::EndStackingContext => {
                        out.push(r#"{"kind":"end-stacking-context"}"#.to_string());
                    }
                    PaintCmd::PushClip {
                        rect,
                        radius,
                        radius_y,
                    } => {
                        if rect.x <= qx2 && rect.right() >= x && rect.y <= qy2 && rect.bottom() >= y
                        {
                            out.push(format!(
                                r#"{{"kind":"push-clip","x":{:.1},"y":{:.1},"w":{:.1},"h":{:.1},"radius":[{:.1},{:.1},{:.1},{:.1}],"radius_y":[{:.1},{:.1},{:.1},{:.1}]}}"#,
                                rect.x, rect.y, rect.w, rect.h,
                                radius[0], radius[1], radius[2], radius[3],
                                radius_y[0], radius_y[1], radius_y[2], radius_y[3]
                            ));
                        }
                    }
                    PaintCmd::PushClipPath { points, even_odd } => {
                        out.push(format!(
                            r#"{{"kind":"push-clip-path","points":{},"even_odd":{}}}"#,
                            points.len(),
                            even_odd
                        ));
                    }
                    PaintCmd::PushClipSvgPath {
                        path,
                        fill_rule,
                        origin,
                    } => {
                        out.push(format!(
                            r#"{{"kind":"push-clip-svg-path","segments":{},"fill_rule":"{:?}","x":{:.1},"y":{:.1}}}"#,
                            path.len(), fill_rule, origin.0, origin.1
                        ));
                    }
                    PaintCmd::PushTransform { node_id, transform } => {
                        out.push(format!(
                            r#"{{"kind":"push-transform","node_id":{},"matrix":[{:.4},{:.4},{:.4},{:.4},{:.1},{:.1}]}}"#,
                            node_id,
                            transform[0], transform[1], transform[2], transform[3],
                            transform[4], transform[5]
                        ));
                    }
                    PaintCmd::PopTransform => {
                        out.push(r#"{"kind":"pop-transform"}"#.to_string());
                    }
                    PaintCmd::PushOpacity { alpha } => {
                        out.push(format!(r#"{{"kind":"push-opacity","alpha":{:.3}}}"#, alpha));
                    }
                    PaintCmd::PopOpacity => {
                        out.push(r#"{"kind":"pop-opacity"}"#.to_string());
                    }
                    PaintCmd::PushFilter { filters } => {
                        out.push(format!(
                            r#"{{"kind":"push-filter","filters":{}}}"#,
                            filters.len()
                        ));
                    }
                    PaintCmd::PopFilter => {
                        out.push(r#"{"kind":"pop-filter"}"#.to_string());
                    }
                    PaintCmd::BackdropFilter { rect, filters, .. } => {
                        if rect.x <= qx2 && rect.right() >= x && rect.y <= qy2 && rect.bottom() >= y
                        {
                            out.push(format!(
                                r#"{{"kind":"backdrop-filter","x":{:.1},"y":{:.1},"w":{:.1},"h":{:.1},"filters":{}}}"#,
                                rect.x, rect.y, rect.w, rect.h, filters.len()
                            ));
                        }
                    }
                    PaintCmd::PushMask { rect, data, .. } => {
                        if rect.x <= qx2 && rect.right() >= x && rect.y <= qy2 && rect.bottom() >= y
                        {
                            let (mw, mh) = match data {
                                ImageRef::Owned(_, w, h) | ImageRef::Shared(_, w, h) => (*w, *h),
                            };
                            out.push(format!(
                                r#"{{"kind":"push-mask","x":{:.1},"y":{:.1},"w":{:.1},"h":{:.1},"mask_w":{},"mask_h":{}}}"#,
                                rect.x, rect.y, rect.w, rect.h, mw, mh
                            ));
                        }
                    }
                    PaintCmd::PopMask => out.push(r#"{"kind":"pop-mask"}"#.to_string()),
                    PaintCmd::PushBlendMode { mode } => {
                        out.push(format!(r#"{{"kind":"push-blend-mode","mode":{}}}"#, mode));
                    }
                    PaintCmd::PopBlendMode => {
                        out.push(r#"{"kind":"pop-blend-mode"}"#.to_string());
                    }
                    PaintCmd::PopClip => out.push(r#"{"kind":"pop-clip"}"#.to_string()),
                    _ => {}
                }
            }
            format!(
                r#"{{"ok":true,"count":{},"commands":[{}]}}"#,
                out.len(),
                out.join(",")
            )
        }
        "tree" => {
            let mut buf = String::new();
            fn dump(n: &crate::WebCore, buf: &mut String, depth: usize) {
                let indent = "  ".repeat(depth);
                let id = n
                    .attributes
                    .get("id")
                    .map(|v| format!("#{v}"))
                    .unwrap_or_default();
                let cls = n
                    .attributes
                    .get("class")
                    .map(|v| {
                        format!(
                            ".{}",
                            v.split_whitespace().take(2).collect::<Vec<_>>().join(".")
                        )
                    })
                    .unwrap_or_default();
                let c = n.layout.content_rect;
                if n.tag == "#text" {
                    let t: String = n.text.trim().chars().take(40).collect();
                    if !t.is_empty() {
                        buf.push_str(&format!("{indent}#text \"{t}\"\n"));
                    }
                } else {
                    buf.push_str(&format!(
                        "{indent}{}{}{} [{:?}] {:.0}x{:.0}\n",
                        n.tag, id, cls, n.style.display, c.w, c.h
                    ));
                }
                for ch in &n.children {
                    dump(ch, buf, depth + 1);
                }
            }
            dump(&doc.root, &mut buf, 0);
            format!(r#"{{"ok":true,"tree":{}}}"#, dbg_json_escape(&buf))
        }
        "dom-tree" => {
            let nid = dbg_json_num(line, "nid").map(|n| n as u32);
            let depth = dbg_json_num(line, "depth").unwrap_or(3.0) as usize;
            let root_node = if let Some(id) = nid {
                doc.get_box_by_id(id).unwrap_or(&doc.root)
            } else {
                &doc.root
            };
            fn tj(n: &crate::WebCore, d: usize, mx: usize) -> String {
                let cc = n
                    .children
                    .iter()
                    .filter(|c| !(c.tag == "#text" && c.text.trim().is_empty()))
                    .count();
                let tp = if n.tag == "#text" {
                    let t: String = n.text.trim().chars().take(60).collect();
                    format!(
                        r#","text":"{}""#,
                        t.replace('\\', "\\\\").replace('"', "\\\"")
                    )
                } else {
                    String::new()
                };
                let ch = if d < mx && cc > 0 {
                    let k: Vec<String> = n
                        .children
                        .iter()
                        .filter(|c| !(c.tag == "#text" && c.text.trim().is_empty()))
                        .map(|c| tj(c, d + 1, mx))
                        .collect();
                    format!(r#","children":[{}]"#, k.join(","))
                } else if cc > 0 {
                    format!(r#","child_count":{cc}"#)
                } else {
                    String::new()
                };
                let r = n.layout.content_rect;
                format!(
                    r#"{{"tag":"{}","id":"{}","class":"{}","nid":{},"rect":[{:.0},{:.0},{:.0},{:.0}]{},"count":{}{}}}"#,
                    n.tag,
                    n.attributes.get("id").unwrap_or(&String::new()),
                    n.attributes.get("class").unwrap_or(&String::new()),
                    n.node_id,
                    r.x,
                    r.y,
                    r.w,
                    r.h,
                    tp,
                    cc,
                    ch
                )
            }
            format!(r#"{{"ok":true,"tree":{}}}"#, tj(root_node, 0, depth))
        }
        "inspect-node" => {
            let nid = dbg_json_num(line, "nid").unwrap_or(0.0) as u32;
            if let Some(n) = doc.get_box_by_id(nid) {
                let l = &n.layout;
                format!(
                    r#"{{"ok":true,"tag":"{}","id":"{}","class":"{}","nid":{},"display":"{:?}","position":"{:?}","top":"{:?}","bottom":"{:?}","margin":{{"top":{:.1},"right":{:.1},"bottom":{:.1},"left":{:.1}}},"border":{{"top":{:.1},"right":{:.1},"bottom":{:.1},"left":{:.1}}},"padding":{{"top":{:.1},"right":{:.1},"bottom":{:.1},"left":{:.1}}},"content":{{"x":{:.1},"y":{:.1},"width":{:.1},"height":{:.1}}},"font_size":{:.1},"color":"{:02x}{:02x}{:02x}","bg":"{:02x}{:02x}{:02x}{:02x}","image":{{"width":{},"height":{},"bytes":{},"src":"{}"}}}}"#,
                    n.tag,
                    n.attributes.get("id").unwrap_or(&String::new()),
                    n.attributes.get("class").unwrap_or(&String::new()),
                    nid,
                    n.style.display,
                    n.style.position,
                    n.style.top,
                    n.style.bottom,
                    l.resolved_margin_top,
                    l.resolved_margin_right,
                    l.resolved_margin_bottom,
                    l.resolved_margin_left,
                    l.resolved_border_top,
                    l.resolved_border_right,
                    l.resolved_border_bottom,
                    l.resolved_border_left,
                    l.resolved_pad_top,
                    l.resolved_pad_right,
                    l.resolved_pad_bottom,
                    l.resolved_pad_left,
                    l.content_rect.x,
                    l.content_rect.y,
                    l.content_rect.w,
                    l.content_rect.h,
                    n.style.font_size_px(16.0, 16.0),
                    n.style.color.r,
                    n.style.color.g,
                    n.style.color.b,
                    n.style.background_color.r,
                    n.style.background_color.g,
                    n.style.background_color.b,
                    n.style.background_color.a,
                    n.image_width,
                    n.image_height,
                    n.image_data.as_ref().map(|d| d.len()).unwrap_or(0),
                    n.resolved_src.replace('"', "\\\"")
                )
            } else {
                format!(r#"{{"ok":false,"error":"node not found"}}"#)
            }
        }
        "chrome-screenshot" => {
            if chrome_port == 0 {
                return r#"{"ok":false,"error":"no --chrome"}"#.to_string();
            }
            let params = r#"{"format":"png"}"#;
            match cdp_send(chrome_port, "Page.captureScreenshot", params) {
                Ok(resp) => {
                    // Extract base64 data from CDP response
                    if let Some(data_start) = resp.find("\"data\":\"") {
                        let data = &resp[data_start + 8..];
                        if let Some(end) = data.find('"') {
                            let b64 = &data[..end];
                            let path = dbg_json_str(line, "out")
                                .unwrap_or_else(|| "chrome_screenshot.png".to_string());
                            if let Ok(bytes) = base64_decode_std(b64) {
                                if std::fs::write(&path, &bytes).is_ok() {
                                    return format!(r#"{{"ok":true,"path":"{}"}}"#, path);
                                }
                            }
                        }
                    }
                    format!(r#"{{"ok":false,"error":"failed to extract screenshot"}}"#)
                }
                Err(e) => format!(r#"{{"ok":false,"error":"{}"}}"#, e),
            }
        }
        // Ask BOTH engines for the same selector's geometry and report only
        // where they disagree.
        //
        // Everything needed for this was already here — a CDP transport and a
        // layout query — but nothing joined them, so comparing against a real
        // browser meant hand-writing a fixture with a `getBoundingClientRect`
        // script in it, dumping the DOM, and diffing the text by hand. One
        // command does it, and it scales to any page rather than only to
        // fixtures written for the purpose.
        "compare" => {
            if chrome_port == 0 {
                return r#"{"ok":false,"error":"no --chrome"}"#.to_string();
            }
            let sel = dbg_json_str(line, "selector").unwrap_or_else(|| "*".to_string());
            let tol: f32 = dbg_json_str(line, "tolerance")
                .and_then(|t| t.parse().ok())
                .unwrap_or(0.5);

            // Optional: put BOTH engines into the same hover state first, so a
            // menu that only exists while hovered can be compared at all. A
            // static diff cannot see a dropdown, which is exactly where the
            // interesting differences live.
            if let Some(hsel) = dbg_json_str(line, "hover") {
                // Chrome: ask the page where the target is, then send a real
                // mouse move there. JS cannot set `:hover`; the input event can.
                let find_js = format!(
                    "(()=>{{const e=document.querySelector({hsel:?});if(!e)return '';\
                     const r=e.getBoundingClientRect();\
                     return (r.x+r.width/2)+','+(r.y+r.height/2)}})()"
                );
                let params = format!(
                    r#"{{"expression":{},"returnByValue":true}}"#,
                    json_quote(&find_js)
                );
                if let Ok(resp) = cdp_send(chrome_port, "Runtime.evaluate", &params) {
                    if let Some(v) = dbg_json_str(&resp, "value") {
                        let mut it = v.split(',');
                        if let (Some(x), Some(y)) = (it.next(), it.next()) {
                            let move_params = format!(
                                r#"{{"type":"mouseMoved","x":{},"y":{},"button":"none","buttons":0,"clickCount":0}}"#,
                                x.trim(),
                                y.trim()
                            );
                            let _ = cdp_send(chrome_port, "Input.dispatchMouseEvent", &move_params);
                        }
                    }
                }
                // Ours: the same move, then a full layout so the hover cascade
                // runs before anything is measured.
                if let Some((hx, hy)) = dbg_selector_center(doc, &hsel) {
                    let pt = (hx + doc.scroll_x, hy + doc.scroll_y);
                    doc.process_mouse_event(crate::dom::HtmlEventType::MouseMove, pt, 0);
                    renderer.layout_engine().layout(doc, width);
                }
            }

            // Key each match by its id, falling back to document order, so the
            // two lists line up even when a selector matches unnamed elements.
            // Document-relative, not viewport-relative: `getBoundingClientRect`
            // is relative to the viewport, our rects are absolute, and any page
            // tall enough to scroll made every y disagree by the scroll offset.
            let js = format!(
                "[...document.querySelectorAll({sel:?})].map((e,i)=>{{const r=e.getBoundingClientRect();                 return [e.id||('@'+i),r.x+window.scrollX,r.y+window.scrollY,r.width,r.height].join(',')}}).join(';')"
            );
            let params = format!(
                r#"{{"expression":{},"returnByValue":true}}"#,
                json_quote(&js)
            );
            let resp = match cdp_send(chrome_port, "Runtime.evaluate", &params) {
                Ok(r) => r,
                Err(e) => return format!(r#"{{"ok":false,"error":"{}"}}"#, e),
            };
            let Some(payload) = dbg_json_str(&resp, "value") else {
                return format!(
                    r#"{{"ok":false,"error":"no value from Chrome","raw":{}}}"#,
                    json_quote(&resp)
                );
            };
            let mut theirs: Vec<(String, [f32; 4])> = Vec::new();
            for rec in payload.split(';').filter(|r| !r.is_empty()) {
                let f: Vec<&str> = rec.split(',').collect();
                if f.len() == 5 {
                    let n = |i: usize| f[i].parse::<f32>().unwrap_or(f32::NAN);
                    theirs.push((f[0].to_string(), [n(1), n(2), n(3), n(4)]));
                }
            }
            let nodes = crate::dom::query_selector_all(&doc.root, &sel);
            let mut diffs: Vec<String> = Vec::new();
            for (i, n) in nodes.iter().enumerate() {
                let key = match n.attributes.get("id") {
                    Some(id) if !id.is_empty() => id.clone(),
                    _ => format!("@{i}"),
                };
                let Some((_, t)) = theirs.iter().find(|(k, _)| *k == key) else {
                    continue;
                };
                let b = n.layout.border_rect;
                let ours = [b.x, b.y, b.w, b.h];
                if ours.iter().zip(t.iter()).any(|(a, c)| (a - c).abs() > tol) {
                    diffs.push(format!(
                        r#"{{"key":"{}","tag":"{}","ours":{{"x":{},"y":{},"w":{},"h":{}}},"chrome":{{"x":{},"y":{},"w":{},"h":{}}}}}"#,
                        key, n.tag,
                        fmt_px(ours[0]), fmt_px(ours[1]), fmt_px(ours[2]), fmt_px(ours[3]),
                        fmt_px(t[0]), fmt_px(t[1]), fmt_px(t[2]), fmt_px(t[3])));
                }
            }
            format!(
                r#"{{"ok":true,"selector":"{}","tolerance":{},"ours":{},"chrome":{},"mismatched":{},"diffs":[{}]}}"#,
                sel,
                tol,
                nodes.len(),
                theirs.len(),
                diffs.len(),
                diffs.join(",")
            )
        }
        "chrome-sync" | "sync" => {
            if chrome_port == 0 {
                return r#"{"ok":false,"error":"no --chrome"}"#.to_string();
            }
            let scroll_y = doc.scroll_y;
            let params = format!(
                r#"{{"expression":"window.scrollTo(0,{});[window.scrollX,window.scrollY,document.title]","returnByValue":true}}"#,
                scroll_y as i32
            );
            match cdp_send(chrome_port, "Runtime.evaluate", &params) {
                Ok(resp) => format!(r#"{{"ok":true,"chrome_response":{}}}"#, resp),
                Err(e) => format!(r#"{{"ok":false,"error":"{}"}}"#, e),
            }
        }
        "box-model" => {
            let sel = dbg_json_str(line, "selector").unwrap_or_default();
            if let Some(n) = crate::dom::query_selector(&doc.root, &sel) {
                let l = &n.layout;
                format!(
                    r#"{{"ok":true,"tag":"{}","margin":{{"top":{:.1},"right":{:.1},"bottom":{:.1},"left":{:.1}}},"border":{{"top":{:.1},"right":{:.1},"bottom":{:.1},"left":{:.1}}},"padding":{{"top":{:.1},"right":{:.1},"bottom":{:.1},"left":{:.1}}},"content":{{"width":{:.1},"height":{:.1}}}}}"#,
                    n.tag,
                    l.resolved_margin_top,
                    l.resolved_margin_right,
                    l.resolved_margin_bottom,
                    l.resolved_margin_left,
                    l.resolved_border_top,
                    l.resolved_border_right,
                    l.resolved_border_bottom,
                    l.resolved_border_left,
                    l.resolved_pad_top,
                    l.resolved_pad_right,
                    l.resolved_pad_bottom,
                    l.resolved_pad_left,
                    l.content_rect.w,
                    l.content_rect.h
                )
            } else {
                r#"{"ok":false,"error":"not found"}"#.to_string()
            }
        }
        "dom-path" | "path" => {
            let sel = dbg_json_str(line, "selector").unwrap_or_default();
            if let Some(n) = crate::dom::query_selector(&doc.root, &sel) {
                fn bp(root: &crate::WebCore, tid: u32, p: &mut Vec<String>) -> bool {
                    let id = root
                        .attributes
                        .get("id")
                        .map(|v| format!("#{v}"))
                        .unwrap_or_default();
                    let cls = root
                        .attributes
                        .get("class")
                        .map(|v| format!(".{}", v.split_whitespace().next().unwrap_or("")))
                        .unwrap_or_default();
                    p.push(format!("{}{}{}", root.tag, id, cls));
                    if root.node_id == tid {
                        return true;
                    }
                    for c in &root.children {
                        if bp(c, tid, p) {
                            return true;
                        }
                    }
                    p.pop();
                    false
                }
                let mut p = Vec::new();
                bp(&doc.root, n.node_id, &mut p);
                format!(r#"{{"ok":true,"path":"{}"}}"#, p.join(" > "))
            } else {
                r#"{"ok":false,"error":"not found"}"#.to_string()
            }
        }
        "parent" => {
            let sel = dbg_json_str(line, "selector").unwrap_or_default();
            if let Some(n) = crate::dom::query_selector(&doc.root, &sel) {
                fn anc(r: &crate::WebCore, tid: u32, ch: &mut Vec<String>) -> bool {
                    if r.node_id == tid {
                        ch.push(format!(r#"{{"tag":"{}","nid":{}}}"#, r.tag, r.node_id));
                        return true;
                    }
                    for c in &r.children {
                        if anc(c, tid, ch) {
                            ch.push(format!(r#"{{"tag":"{}","nid":{}}}"#, r.tag, r.node_id));
                            return true;
                        }
                    }
                    false
                }
                let mut ch = Vec::new();
                anc(&doc.root, n.node_id, &mut ch);
                format!(r#"{{"ok":true,"chain":[{}]}}"#, ch.join(","))
            } else {
                r#"{"ok":false,"error":"not found"}"#.to_string()
            }
        }
        "hit" => {
            let x = dbg_json_num(line, "x").unwrap_or(0.0) as f32;
            let y = dbg_json_num(line, "y").unwrap_or(0.0) as f32;
            if let Some(hit) = crate::layout::hit_test::point_to_hit_scrolled(
                &doc.root,
                (x + doc.scroll_x, y + doc.scroll_y),
                (doc.scroll_x, doc.scroll_y),
                0,
            ) {
                if let Some(n) = doc.get_box_by_id(hit.node_id) {
                    format!(
                        r#"{{"ok":true,"nid":{},"tag":"{}","class":"{}"}}"#,
                        hit.node_id,
                        n.tag,
                        n.attributes.get("class").unwrap_or(&String::new())
                    )
                } else {
                    format!(r#"{{"ok":true,"nid":{}}}"#, hit.node_id)
                }
            } else {
                r#"{"ok":false,"error":"no hit"}"#.to_string()
            }
        }
        "search" => {
            let q = dbg_json_str(line, "query")
                .unwrap_or_default()
                .to_lowercase();
            let mut results = Vec::new();
            fn sw(n: &crate::WebCore, q: &str, r: &mut Vec<String>) {
                if n.tag == "#text" && n.text.to_lowercase().contains(q) {
                    r.push(format!(
                        r#"{{"nid":{},"text":"{}"}}"#,
                        n.node_id,
                        n.text
                            .trim()
                            .chars()
                            .take(60)
                            .collect::<String>()
                            .replace('"', "\\\"")
                    ));
                }
                for c in &n.children {
                    sw(c, q, r);
                }
            }
            sw(&doc.root, &q, &mut results);
            format!(
                r#"{{"ok":true,"count":{},"results":[{}]}}"#,
                results.len(),
                results.join(",")
            )
        }
        "viewport" => {
            let doc_h = Document::scroll_height(&doc.root);
            format!(
                r#"{{"ok":true,"width":{:.0},"height":{:.0},"doc_height":{:.0}}}"#,
                width, height, doc_h
            )
        }
        "network" => {
            let mut img = 0u32;
            crate::Document::walk_all(&doc.root, &mut |b| {
                if b.image_data.is_some() {
                    img += 1;
                }
            });
            format!(
                r#"{{"ok":true,"stylesheets":{},"images":{}}}"#,
                doc.linked_stylesheets.len(),
                img
            )
        }
        "a11y" | "accessibility" => {
            fn aw(n: &crate::WebCore, d: usize, o: &mut String) {
                let role = match n.tag.as_str() {
                    "a" => "link",
                    "button" | "input" => "button",
                    "img" => "image",
                    "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => "heading",
                    "nav" => "navigation",
                    "main" => "main",
                    "ul" | "ol" => "list",
                    "li" => "listitem",
                    "#text" => {
                        if !n.text.trim().is_empty() {
                            "text"
                        } else {
                            return;
                        }
                    }
                    _ => n.attributes.get("role").map(|s| s.as_str()).unwrap_or(""),
                };
                if !role.is_empty() {
                    let label = n
                        .attributes
                        .get("aria-label")
                        .or(n.attributes.get("alt"))
                        .cloned()
                        .unwrap_or_else(|| {
                            if n.tag == "#text" {
                                n.text.trim().chars().take(40).collect()
                            } else {
                                String::new()
                            }
                        });
                    o.push_str(&format!("{}{}: {}\n", "  ".repeat(d), role, label));
                }
                for c in &n.children {
                    aw(c, d + if !role.is_empty() { 1 } else { 0 }, o);
                }
            }
            let mut t = String::new();
            aw(&doc.root, 0, &mut t);
            format!(r#"{{"ok":true,"tree":{}}}"#, dbg_json_escape(&t))
        }
        "text" => {
            let sel = dbg_json_str(line, "selector").unwrap_or_default();
            let nodes = crate::dom::query_selector_all(&doc.root, &sel);
            let texts: Vec<String> = nodes
                .iter()
                .map(|n| {
                    format!(
                        "\"{}\"",
                        crate::dom::get_text_content(n).replace('"', "\\\"")
                    )
                })
                .collect();
            format!(
                r#"{{"ok":true,"count":{},"texts":[{}]}}"#,
                texts.len(),
                texts.join(",")
            )
        }
        "attr" => {
            let sel = dbg_json_str(line, "selector").unwrap_or_default();
            let name = dbg_json_str(line, "name").unwrap_or_default();
            let nodes = crate::dom::query_selector_all(&doc.root, &sel);
            let vals: Vec<String> = nodes
                .iter()
                .map(|n| {
                    format!(
                        "\"{}\"",
                        n.attributes
                            .get(&name)
                            .unwrap_or(&String::new())
                            .replace('"', "\\\"")
                    )
                })
                .collect();
            format!(
                r#"{{"ok":true,"count":{},"values":[{}]}}"#,
                vals.len(),
                vals.join(",")
            )
        }
        "setstyle" => {
            let sel = dbg_json_str(line, "selector").unwrap_or_default();
            let prop = dbg_json_str(line, "prop").unwrap_or_default();
            let val = dbg_json_str(line, "value").unwrap_or_default();
            if let Some(n) = doc.query_selector(&sel) {
                doc.set_style_property(n, &prop, &val);
            }
            renderer.layout_engine().layout(doc, width);
            r#"{"ok":true}"#.to_string()
        }
        "highlight" => {
            let sel = dbg_json_str(line, "selector").unwrap_or_default();
            let out = dbg_json_str(line, "out").unwrap_or_else(|| "highlight.png".to_string());
            let rh = Document::scroll_height(&doc.root).ceil() as u32;
            let rh = rh.max(1).min(4000);
            if let Some(mut pm) = tiny_skia::Pixmap::new(width as u32, rh) {
                pm.fill(tiny_skia::Color::WHITE);
                renderer.render(doc, &mut pm, 1.0);
                for node in crate::dom::query_selector_all(&doc.root, &sel) {
                    crate::draw_inspect_overlay(node, &mut pm, doc.scroll_x, doc.scroll_y, 1.0);
                }
                let _ = pm.save_png(&out);
                format!(r#"{{"ok":true,"path":"{}"}}"#, out)
            } else {
                r#"{"ok":false,"error":"pixmap failed"}"#.to_string()
            }
        }
        // ── Inspect (by selector or nid) ─────────────────────────────────
        "inspect" => {
            let sel = dbg_json_str(line, "selector").unwrap_or_default();
            let mut parts = Vec::new();
            Document::walk_all(&doc.root, &mut |node| {
                if dbg_matches_query(doc, node, &sel) {
                    parts.push(dbg_inspect_json(node));
                }
            });
            format!(
                r#"{{"ok":true,"count":{},"elements":[{}]}}"#,
                parts.len(),
                parts.join(",")
            )
        }
        "svg-metrics" => {
            format!(r#"{{"ok":true,"svg":{}}}"#, dbg_svg_metrics_json(&doc.root))
        }
        // ── Computed styles ──────────────────────────────────────────────
        "computed" => {
            let sel = dbg_json_str(line, "selector").unwrap_or_default();
            let mut parts = Vec::new();
            Document::walk_all(&doc.root, &mut |node| {
                if dbg_matches_query(doc, node, &sel) {
                    parts.push(dbg_computed_json(node));
                }
            });
            format!(
                r#"{{"ok":true,"count":{},"elements":[{}]}}"#,
                parts.len(),
                parts.join(",")
            )
        }
        // ── Matched CSS rules ────────────────────────────────────────────
        "rules" | "matched-rules" => {
            let sel = dbg_json_str(line, "selector").unwrap_or_default();
            let mut results = Vec::new();
            Document::walk_all(&doc.root, &mut |node| {
                if dbg_matches_query(doc, node, &sel) {
                    let rules: Vec<String> = node.matched_rules.iter().map(|r| {
                        let decls: Vec<String> = r.declarations.iter()
                            .filter(|(k, _)| !k.starts_with("--"))
                            .map(|(k, v)| format!("{}:{}", dbg_json_escape(k), dbg_json_escape(v)))
                            .collect();
                        format!(r#"{{"selector":{},"specificity":{},"source":{},"layer":{},"layer_rank":{},"declarations":{{{}}}}}"#,
                            dbg_json_escape(&r.selector), r.specificity,
                            dbg_json_escape(&r.source),
                            dbg_json_escape(&r.layer), r.layer_rank, decls.join(","))
                    }).collect();
                    let id = node.attributes.get("id").map(|v| v.as_str()).unwrap_or("");
                    let cls = node
                        .attributes
                        .get("class")
                        .map(|v| v.as_str())
                        .unwrap_or("");
                    results.push(format!(
                        r#"{{"tag":{},"id":{},"class":{},"rules":[{}]}}"#,
                        dbg_json_escape(&node.tag),
                        dbg_json_escape(id),
                        dbg_json_escape(cls),
                        rules.join(",")
                    ));
                }
            });
            format!(
                r#"{{"ok":true,"count":{},"elements":[{}]}}"#,
                results.len(),
                results.join(",")
            )
        }
        // ── Deep inspect ─────────────────────────────────────────────────
        "deep" => {
            let selector = dbg_json_str(line, "selector").unwrap_or_default();
            let nodes = crate::dom::query_selector_all(&doc.root, &selector);
            let mut items: Vec<String> = Vec::new();
            for node in nodes {
                let cr = node.layout.content_rect;
                let pr = node.layout.padding_rect;
                let mr = node.layout.margin_rect;
                let cls = node.attributes.get("class").cloned().unwrap_or_default();
                let id = node.attributes.get("id").cloned().unwrap_or_default();
                let before_display = node
                    .style
                    .before_style
                    .as_ref()
                    .map(|s| format!("{:?}", s.display))
                    .unwrap_or_default();
                let after_display = node
                    .style
                    .after_style
                    .as_ref()
                    .map(|s| format!("{:?}", s.display))
                    .unwrap_or_default();
                let children: Vec<String> = node.children.iter()
                    .filter(|c| c.tag != "#text" || !c.text.trim().is_empty())
                    .map(|c| {
                        let cc = c.layout.content_rect;
                        format!(r#"{{"tag":"{}","id":"{}","class":"{}","display":"{:?}","float":"{:?}","c":[{:.0},{:.0},{:.0},{:.0}]}}"#,
                            c.tag, c.attributes.get("id").unwrap_or(&String::new()),
                            c.attributes.get("class").unwrap_or(&String::new()),
                            c.style.display, c.style.float, cc.x, cc.y, cc.w, cc.h)
                    }).collect();
                items.push(format!(
                    r#"{{"tag":"{}","id":"{}","class":"{}","content":[{:.0},{:.0},{:.0},{:.0}],"padding":[{:.0},{:.0},{:.0},{:.0}],"margin":[{:.0},{:.0},{:.0},{:.0}],"display":"{:?}","float":"{:?}","before_content":{},"before_display":{},"after_content":{},"after_display":{},"children":[{}]}}"#,
                    node.tag, id, cls,
                    cr.x, cr.y, cr.w, cr.h,
                    pr.x, pr.y, pr.w, pr.h,
                    mr.x, mr.y, mr.w, mr.h,
                    node.style.display, node.style.float,
                    dbg_json_escape(&node.style.before_content),
                    dbg_json_escape(&before_display),
                    dbg_json_escape(&node.style.after_content),
                    dbg_json_escape(&after_display),
                    children.join(",")
                ));
            }
            format!(
                r#"{{"ok":true,"count":{},"elements":[{}]}}"#,
                items.len(),
                items.join(",")
            )
        }
        // ── CSS property query ───────────────────────────────────────────
        "css" => {
            let selector = dbg_json_str(line, "selector").unwrap_or_default();
            let props_str = dbg_json_str(line, "props").unwrap_or_default();
            doc.update_computed_style(renderer.layout_engine());
            // Ids, so the DOM's own resolver can answer every property rather
            // than a hardcoded handful. It knows ~47; the old match knew nine
            // and reported "(unknown)" for the rest, which made the inspector
            // useless for exactly the questions worth asking.
            let ids = crate::dom::query_selector_all_ids(&doc.root, &selector);
            let mut items: Vec<String> = Vec::new();
            for nid in ids {
                let (tag, id, cls) = match doc.get_box_by_id(nid) {
                    Some(n) => (
                        n.tag.clone(),
                        n.attributes.get("id").cloned().unwrap_or_default(),
                        n.attributes.get("class").cloned().unwrap_or_default(),
                    ),
                    None => continue,
                };
                let mut kv: Vec<String> = vec![
                    format!(r#""tag":"{}""#, tag),
                    format!(r#""id":"{}""#, id),
                    format!(r#""class":"{}""#, dbg_json_escape(&cls)),
                ];
                for prop in props_str.split(',') {
                    let prop = prop.trim();
                    if prop.is_empty() {
                        continue;
                    }
                    // Geometry the resolver does not carry.
                    let val = match prop {
                        "content-rect" | "padding-rect" | "margin-rect" | "border-rect" => {
                            match doc.get_box_by_id(nid) {
                                Some(n) => {
                                    let r = match prop {
                                        "content-rect" => n.layout.content_rect,
                                        "padding-rect" => n.layout.padding_rect,
                                        "margin-rect" => n.layout.margin_rect,
                                        _ => n.layout.border_rect,
                                    };
                                    format!("{:.1},{:.1} {:.1}x{:.1}", r.x, r.y, r.w, r.h)
                                }
                                None => String::new(),
                            }
                        }
                        "svg-path" => doc
                            .get_box_by_id(nid)
                            .and_then(|n| {
                                n.svg_tree_path.as_ref().map(|path| {
                                    path.iter()
                                        .map(|part| part.to_string())
                                        .collect::<Vec<_>>()
                                        .join(".")
                                })
                            })
                            .unwrap_or_default(),
                        "svg-fill-stored" => doc
                            .get_box_by_id(nid)
                            .and_then(|n| n.style.svg_fill)
                            .map(|c| format!("#{:02x}{:02x}{:02x}{:02x}", c.r, c.g, c.b, c.a))
                            .unwrap_or_else(|| "none".to_string()),
                        "svg-stroke-stored" => doc
                            .get_box_by_id(nid)
                            .and_then(|n| n.style.svg_stroke)
                            .map(|c| format!("#{:02x}{:02x}{:02x}{:02x}", c.r, c.g, c.b, c.a))
                            .unwrap_or_else(|| "none".to_string()),
                        "svg-paint-flags" => doc
                            .get_box_by_id(nid)
                            .map(|n| format!("{}", n.style.rare().specified_svg_paint_props))
                            .unwrap_or_default(),
                        "matched-rule-count" => doc
                            .get_box_by_id(nid)
                            .map(|n| format!("{}", n.matched_rules.len()))
                            .unwrap_or_default(),
                        _ => doc.computed_style_property_current(nid, prop),
                    };
                    kv.push(format!(
                        "{}:{}",
                        dbg_json_escape(prop),
                        dbg_json_escape(&val)
                    ));
                }
                items.push(format!("{{{}}}", kv.join(",")));
            }
            format!(
                r#"{{"ok":true,"count":{},"elements":[{}]}}"#,
                items.len(),
                items.join(",")
            )
        }
        // ── Benchmark ────────────────────────────────────────────────────
        "bench" => {
            let n = dbg_json_num(line, "n").unwrap_or(1.0) as u32;
            let n = n.max(1).min(100);
            let mut layout_times = Vec::new();
            for _ in 0..n {
                let t = std::time::Instant::now();
                renderer.layout_engine().layout(doc, width);
                layout_times.push(t.elapsed().as_micros() as f64 / 1000.0);
            }
            let avg = layout_times.iter().sum::<f64>() / layout_times.len() as f64;
            format!(
                r#"{{"ok":true,"iterations":{},"layout_avg_ms":{:.1}}}"#,
                n, avg
            )
        }
        "perf" => {
            let rules = doc.stylesheet.rules.len();
            let mut node_count = 0u32;
            Document::walk_all(&doc.root, &mut |_| {
                node_count += 1;
            });
            format!(
                r#"{{"ok":true,"nodes":{},"css_rules":{},"doc_height":{:.0}}}"#,
                node_count,
                rules,
                Document::scroll_height(&doc.root)
            )
        }
        "rule-search" => {
            let query = dbg_json_str(line, "query").unwrap_or_default();
            let limit = dbg_json_num(line, "limit").unwrap_or(20.0).max(1.0) as usize;
            let mut matches = Vec::new();
            for rule in &doc.stylesheet.rules {
                if matches.len() >= limit {
                    break;
                }
                if rule.original_selector.contains(&query) {
                    let decls: Vec<String> = rule
                        .declarations
                        .iter()
                        .filter(|(k, _)| !k.starts_with("--"))
                        .map(|(k, v)| format!("{}:{}", dbg_json_escape(k), dbg_json_escape(v)))
                        .collect();
                    let selector_ast = format!("{:?}", rule.selectors);
                    let scope_selector = format!("{:?}", rule.scope_selector);
                    let scope_limit_selector = format!("{:?}", rule.scope_limit_selector);
                    let scopes = format!("{:?}", rule.scopes);
                    matches.push(format!(
                        r#"{{"selector":{},"selector_ast":{},"pseudo":"{:?}","specificity":{},"layer":{},"layer_rank":{},"media":{},"scope":{},"scope_limit":{},"scopes":{},"compiled":{},"compiled_important":{},"declarations":{{{}}}}}"#,
                        dbg_json_escape(&rule.original_selector),
                        dbg_json_escape(&selector_ast),
                        rule.pseudo_element,
                        rule.specificity,
                        dbg_json_escape(&rule.layer),
                        rule.layer_rank,
                        dbg_json_escape(&rule.media_condition.label()),
                        dbg_json_escape(&scope_selector),
                        dbg_json_escape(&scope_limit_selector),
                        dbg_json_escape(&scopes),
                        rule.compiled_decls.len(),
                        rule.compiled_important.len(),
                        decls.join(",")
                    ));
                }
            }
            format!(
                r#"{{"ok":true,"query":{},"count":{},"rules":[{}]}}"#,
                dbg_json_escape(&query),
                matches.len(),
                matches.join(",")
            )
        }
        "match-debug" => match dbg_json_str(line, "selector") {
            Some(sel) => {
                let mut parts = Vec::new();
                let limit = dbg_json_num(line, "limit").unwrap_or(10.0).max(1.0) as usize;
                Document::walk_all(&doc.root, &mut |node| {
                    if parts.len() >= limit {
                        return;
                    }
                    if dbg_matches_query(doc, node, &sel) {
                        parts.push(dbg_match_report_json(doc, node));
                    }
                });
                format!(
                    r#"{{"ok":true,"count":{},"elements":[{}]}}"#,
                    parts.len(),
                    parts.join(",")
                )
            }
            None => r#"{"ok":false,"error":"match-debug needs selector"}"#.to_string(),
        },
        "node-id-stats" => {
            let mut total = 0usize;
            let mut zero = 0usize;
            let mut ids = std::collections::HashMap::<u32, usize>::new();
            Document::walk_all(&doc.root, &mut |node| {
                total += 1;
                if node.node_id == 0 {
                    zero += 1;
                } else {
                    *ids.entry(node.node_id).or_insert(0) += 1;
                }
            });
            let mut duplicates = ids
                .into_iter()
                .filter(|(_, count)| *count > 1)
                .collect::<Vec<_>>();
            duplicates.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
            let duplicate_extras = duplicates
                .iter()
                .map(|(_, count)| count.saturating_sub(1))
                .sum::<usize>();
            let dup_json = duplicates
                .iter()
                .take(20)
                .map(|(id, count)| format!(r#"{{"id":{},"count":{}}}"#, id, count))
                .collect::<Vec<_>>()
                .join(",");
            format!(
                r#"{{"ok":true,"total":{},"zero":{},"unique":{},"duplicate_ids":{},"duplicates":[{}]}}"#,
                total,
                zero,
                total.saturating_sub(zero).saturating_sub(duplicate_extras),
                duplicates.len(),
                dup_json
            )
        }
        "keyframes" => {
            let query = dbg_json_str(line, "query").unwrap_or_default();
            let limit = dbg_json_num(line, "limit").unwrap_or(20.0).max(1.0) as usize;
            let mut matches = Vec::new();
            for (name, stops) in &doc.stylesheet.keyframes {
                if matches.len() >= limit {
                    break;
                }
                if !query.is_empty() && !name.contains(&query) {
                    continue;
                }
                let stop_json: Vec<String> = stops
                    .iter()
                    .map(|stop| {
                        let props: Vec<String> = stop
                            .properties
                            .iter()
                            .map(|(k, v)| format!("{}:{}", dbg_json_escape(k), dbg_json_escape(v)))
                            .collect();
                        format!(
                            r#"{{"offset":{:.6},"properties":{{{}}}}}"#,
                            stop.offset,
                            props.join(",")
                        )
                    })
                    .collect();
                matches.push(format!(
                    r#"{{"name":{},"stops":[{}]}}"#,
                    dbg_json_escape(name),
                    stop_json.join(",")
                ));
            }
            format!(
                r#"{{"ok":true,"query":{},"count":{},"keyframes":[{}]}}"#,
                dbg_json_escape(&query),
                matches.len(),
                matches.join(",")
            )
        }
        "resolve-css" => {
            let value = dbg_json_str(line, "value").unwrap_or_default();
            let resolved = crate::css::resolve_var_references(&value, &doc.stylesheet.variables);
            format!(
                r#"{{"ok":true,"value":{},"resolved":{},"variables":{}}}"#,
                dbg_json_escape(&value),
                dbg_json_escape(&resolved),
                doc.stylesheet.variables.len()
            )
        }
        "stylesheet-vars" => {
            let query = dbg_json_str(line, "query").unwrap_or_default();
            let limit = dbg_json_num(line, "limit").unwrap_or(40.0).max(1.0) as usize;
            let mut vars = doc
                .stylesheet
                .variables
                .iter()
                .filter(|(name, value)| {
                    query.is_empty() || name.contains(&query) || value.contains(&query)
                })
                .collect::<Vec<_>>();
            vars.sort_by(|(a, _), (b, _)| a.cmp(b));
            let matched_count = vars.len();
            let entries = vars
                .into_iter()
                .take(limit)
                .map(|(name, value)| {
                    format!(
                        r#"{{"name":{},"value":{}}}"#,
                        dbg_json_escape(name),
                        dbg_json_escape(value)
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            format!(
                r#"{{"ok":true,"query":{},"variables":{},"count":{},"entries":[{}]}}"#,
                dbg_json_escape(&query),
                doc.stylesheet.variables.len(),
                matched_count,
                entries
            )
        }
        // ── Force element state ──────────────────────────────────────────
        "force-state" => {
            let selector = dbg_json_str(line, "selector").unwrap_or_default();
            let state = dbg_json_str(line, "state").unwrap_or_default();
            if let Some(node) = crate::dom::query_selector(&doc.root, &selector) {
                let nid = node.node_id;
                match state.as_str() {
                    "hover" => {
                        doc.hovered_box = nid;
                        doc.hover_changed = true;
                    }
                    "focus" => {
                        doc.focused_box = nid;
                    }
                    "active" => {
                        doc.active_box = nid;
                    }
                    _ => {}
                }
            }
            doc.style_dirty = true;
            renderer.layout_engine().layout(doc, width);
            r#"{"ok":true}"#.to_string()
        }
        // ── Set attribute ────────────────────────────────────────────────
        "setattr" => {
            match (
                dbg_json_str(line, "selector"),
                dbg_json_str(line, "name"),
                dbg_json_str(line, "value"),
            ) {
                (Some(sel), Some(name), Some(val)) => {
                    let mut count = 0usize;
                    let hits = dbg_query_ids(doc, &sel);
                    for id in hits {
                        doc.set_attribute(id, &name, &val);
                        count += 1;
                    }
                    if count > 0 {
                        renderer.layout_engine().layout(doc, width);
                    }
                    format!(r#"{{"ok":true,"modified":{}}}"#, count)
                }
                _ => r#"{"ok":false,"error":"setattr needs selector, name, value"}"#.to_string(),
            }
        }
        // ── Set text content ─────────────────────────────────────────────
        "set-text" => {
            let selector = dbg_json_str(line, "selector").unwrap_or_default();
            let text = dbg_json_str(line, "text").unwrap_or_default();
            if let Some(node) = doc.query_selector(&selector) {
                doc.set_text_content(node, &text);
            }
            renderer.layout_engine().layout(doc, width);
            r#"{"ok":true}"#.to_string()
        }
        // ── Add/remove/toggle class ──────────────────────────────────────
        "add-class" => {
            let selector = dbg_json_str(line, "selector").unwrap_or_default();
            let cls = dbg_json_str(line, "class").unwrap_or_default();
            if let Some(node) = doc.query_selector(&selector) {
                doc.class_list_add(node, &cls);
            }
            renderer.layout_engine().layout(doc, width);
            r#"{"ok":true}"#.to_string()
        }
        "remove-class" => {
            let selector = dbg_json_str(line, "selector").unwrap_or_default();
            let cls = dbg_json_str(line, "class").unwrap_or_default();
            if let Some(node) = doc.query_selector(&selector) {
                doc.class_list_remove(node, &cls);
            }
            renderer.layout_engine().layout(doc, width);
            r#"{"ok":true}"#.to_string()
        }
        "toggle-class" => {
            let selector = dbg_json_str(line, "selector").unwrap_or_default();
            let cls = dbg_json_str(line, "class").unwrap_or_default();
            if let Some(node) = doc.query_selector(&selector) {
                doc.class_list_toggle(node, &cls);
            }
            renderer.layout_engine().layout(doc, width);
            r#"{"ok":true}"#.to_string()
        }
        // Line boxes for a selector: the only view of what inline layout
        // actually produced — heights, baselines and text extents.
        "lines" => {
            let selector = dbg_json_str(line, "selector").unwrap_or_default();
            let nodes = crate::dom::query_selector_all(&doc.root, &selector);
            let mut items: Vec<String> = Vec::new();
            for node in nodes.into_iter().take(6) {
                let lines: Vec<String> = node.layout.line_cache.iter().map(|l| {
                    let visual: Vec<String> = l.visual_segments.iter().map(|seg| format!(
                        r#"{{"s":{},"len":{},"level":{},"x":{:.2},"w":{:.2}}}"#,
                        seg.logical_start, seg.length, seg.level, seg.x, seg.width
                    )).collect();
                    format!(
                        r#"{{"x":{:.2},"y":{:.2},"h":{:.2},"asc":{:.2},"desc":{:.2},"w":{:.2},"text_x":{:.2},"len":{},"visual":[{}]}}"#,
                        l.x,
                        l.y,
                        l.height,
                        l.ascent,
                        l.descent,
                        l.width,
                        l.text_x_offset,
                        l.text_length,
                        visual.join(",")
                    )
                }).collect();
                items.push(format!(
                    r#"{{"tag":"{}","id":"{}","font_px":{:.2},"line_height":"{:?}","content_h":{:.2},"lines":[{}]}}"#,
                    node.tag,
                    node.attributes.get("id").unwrap_or(&String::new()),
                    node.style.font_size_px(16.0, 16.0),
                    node.style.line_height,
                    node.layout.content_rect.h,
                    lines.join(",")));
            }
            format!(
                r#"{{"ok":true,"count":{},"elements":[{}]}}"#,
                items.len(),
                items.join(",")
            )
        }
        "quit" => r#"{"ok":false,"error":"close the owning context"}"#.into(),
        _ => format!(r#"{{"ok":false,"error":"unknown: {}"}}"#, cmd),
    };
    let ms = cmd_start.elapsed().as_micros() as f64 / 1000.0;
    if result.ends_with('}') {
        format!(
            "{}{}\"cmd_ms\":{:.2}}}",
            &result[..result.len() - 1],
            if result.len() > 2 { "," } else { "" },
            ms
        )
    } else {
        result
    }
}
