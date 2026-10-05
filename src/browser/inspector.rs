//! Native browser inspector presentation.
use super::*;

pub(super) fn color_swatch(val: &str) -> String {
    let v = val.trim();
    let is_color = v.starts_with('#') && (v.len() == 4 || v.len() == 7 || v.len() == 9)
        || v.starts_with("rgb")
        || v.starts_with("hsl")
        || matches!(
            v,
            "red"
                | "blue"
                | "green"
                | "white"
                | "black"
                | "gray"
                | "grey"
                | "orange"
                | "yellow"
                | "purple"
                | "pink"
                | "cyan"
                | "transparent"
        );
    if is_color && v != "transparent" {
        format!(
            "<span style='display:inline-block;width:10px;height:10px;border:1px solid #555;\
                 background:{};vertical-align:middle;margin-right:3px;border-radius:2px'></span>",
            escape_html(v)
        )
    } else {
        String::new()
    }
}

/// Collect one node_id per rendered line in the DOM tree.
/// Must exactly match the line output order of `build_dom_tree_html`.
pub(super) fn collect_dom_node_ids(
    node: &crate::WebCore,
    out: &mut Vec<u32>,
    depth: usize,
    max_depth: usize,
) {
    if depth > max_depth {
        return;
    }
    if node.tag == "#text" {
        return;
    }
    if matches!(node.style.display, crate::types::Display::None) {
        return;
    }
    let has_children = node
        .children
        .iter()
        .any(|c| c.tag != "#text" && !matches!(c.style.display, crate::types::Display::None));
    out.push(node.node_id); // opening tag
    for child in &node.children {
        collect_dom_node_ids(child, out, depth + 1, max_depth);
    }
    if has_children {
        out.push(node.node_id); // closing tag → same node
    }
}

/// Build DOM tree HTML. Returns (html_string, selected_line_index).
pub(super) fn build_dom_tree_html(
    root: &crate::WebCore,
    selected_nid: u32,
) -> (String, Option<usize>) {
    let mut html = String::new();
    let mut line_count = 0usize;
    let mut selected_line: Option<usize> = None;

    fn walk(
        node: &crate::WebCore,
        html: &mut String,
        depth: usize,
        selected_nid: u32,
        line: &mut usize,
        sel_line: &mut Option<usize>,
    ) {
        if node.tag == "#text" {
            return;
        }
        if matches!(node.style.display, crate::types::Display::None) {
            return;
        }
        if depth > 20 {
            return;
        }

        let indent = depth * 14;
        let is_selected = selected_nid != 0 && node.node_id == selected_nid;
        if is_selected {
            *sel_line = Some(*line);
        }

        let bg = if is_selected {
            "background:#264f78;"
        } else {
            ""
        };
        let id = node
            .attributes
            .get("id")
            .map(|v| {
                format!(
                    " <span style='color:#d7ba7d'>id=\"{}\"</span>",
                    escape_html(v)
                )
            })
            .unwrap_or_default();
        let cls = node
            .attributes
            .get("class")
            .map(|v| {
                format!(
                    " <span style='color:#9cdcfe'>class=\"{}\"</span>",
                    escape_html(&v.split_whitespace().take(4).collect::<Vec<_>>().join(" "))
                )
            })
            .unwrap_or_default();

        let has_children = node
            .children
            .iter()
            .any(|c| c.tag != "#text" && !matches!(c.style.display, crate::types::Display::None));
        let arrow = if has_children { "▼ " } else { "  " };

        html.push_str(&format!(
            "<div style='padding:1px 2px 1px {}px;white-space:nowrap;overflow:hidden;line-height:16px;\
             font:11px monospace;cursor:pointer;{bg}'>\
             <span style='color:#888'>{arrow}</span>\
             <span style='color:#569cd6'>&lt;{}</span>{id}{cls}<span style='color:#569cd6'>&gt;</span>\
             </div>\n",
            indent, escape_html(&node.tag)
        ));
        *line += 1;

        for child in &node.children {
            walk(child, html, depth + 1, selected_nid, line, sel_line);
        }

        // Closing tag for elements with children
        if has_children {
            html.push_str(&format!(
                "<div style='padding:1px 2px 1px {}px;line-height:16px;font:11px monospace;color:#569cd6'>\
                 &lt;/{}&gt;</div>\n",
                indent, escape_html(&node.tag)
            ));
            *line += 1;
        }
    }

    walk(
        root,
        &mut html,
        0,
        selected_nid,
        &mut line_count,
        &mut selected_line,
    );

    (html, selected_line)
}

pub(super) fn build_inspect_panel_html(
    node: &crate::WebCore,
    active_tab: u8,
    doc_root: Option<&crate::WebCore>,
) -> String {
    let s = &node.style;
    let id = node
        .attributes
        .get("id")
        .map(|v| format!("#{v}"))
        .unwrap_or_default();
    let cls = node
        .attributes
        .get("class")
        .map(|v| format!(".{}", v.split_whitespace().collect::<Vec<_>>().join(".")))
        .unwrap_or_default();

    let tab_style = |idx: u8| -> &'static str {
        if idx == active_tab {
            "color:#fff;border-bottom:2px solid #4fc3f7;padding:6px 12px;font-weight:600"
        } else {
            "color:#888;border-bottom:2px solid transparent;padding:6px 12px;cursor:pointer"
        }
    };

    let mut html = format!(
        r#"<html><head><style>
        body {{ background: #1e1e1e; color: #d4d4d4; font: 11px -apple-system, sans-serif; padding: 0; margin: 0; }}
        .panel {{ padding: 6px 10px; }}
        .elem-bar {{ background: #2d2d30; padding: 6px 10px; border-bottom: 1px solid #3e3e42;
                     font: 12px monospace; white-space: nowrap; overflow: hidden; }}
        .tab-bar {{ display: flex; background: #252526; border-bottom: 1px solid #3e3e42; font-size: 11px; }}
        .tag {{ color: #569cd6; }}
        .id {{ color: #d7ba7d; }}
        .cls {{ color: #9cdcfe; }}
        .attr-name {{ color: #9cdcfe; }}
        .attr-val {{ color: #ce9178; }}
        h3 {{ color: #ccc; font-size: 11px; font-weight: 600; text-transform: uppercase; letter-spacing: 0.5px;
             margin: 10px 0 6px 0; padding: 4px 0; border-bottom: 1px solid #3e3e42; }}
        .prop {{ color: #9cdcfe; }}
        .val {{ color: #ce9178; }}
        .computed-row {{ display: flex; padding: 1px 0; }}
        .computed-row .prop {{ min-width: 130px; }}
        .sel {{ color: #dcdcaa; font-weight: 600; }}
        .rule-src {{ color: #858585; font-size: 10px; }}
        .box-model {{ text-align: center; margin: 6px 0; font-size: 10px; }}
        .bm-margin {{ background: rgba(246,178,107,0.15); border: 1px dashed rgba(246,178,107,0.4); padding: 4px; position: relative; }}
        .bm-margin::before {{ content: 'margin'; position: absolute; top: 1px; left: 3px; color: #f6b26b; font-size: 9px; }}
        .bm-padding {{ background: rgba(147,196,125,0.15); border: 1px dashed rgba(147,196,125,0.4); padding: 4px; position: relative; }}
        .bm-padding::before {{ content: 'padding'; position: absolute; top: 1px; left: 3px; color: #93c47d; font-size: 9px; }}
        .bm-content {{ background: rgba(109,158,235,0.2); padding: 6px 4px; color: #6d9eeb; font-weight: 600; }}
        .bm-row {{ display: flex; align-items: center; }}
        .bm-side {{ flex: 0 0 30px; text-align: center; color: #aaa; }}
        .bm-center {{ flex: 1; text-align: center; }}
        .bm-top, .bm-bot {{ text-align: center; color: #aaa; padding: 2px 0; }}
        .rule-block {{ margin-bottom: 6px; padding: 4px 6px; background: #252526; border-radius: 3px; }}
        .rule-block .decl {{ padding-left: 12px; }}
        .rule-block .overridden {{ text-decoration: line-through; color: #666; }}
        .dom-tree {{ font: 11px monospace; padding: 4px 0; }}
        .dom-node {{ padding: 1px 0 1px 12px; white-space: nowrap; overflow: hidden; }}
    </style></head><body>"#
    );

    // ── Element breadcrumb bar ──
    html.push_str("<div class='elem-bar'>");
    html.push_str(&format!(
        "<span class='tag'>&lt;{}</span><span class='id'>{}</span><span class='cls'>{}</span>",
        escape_html(&node.tag),
        escape_html(&id),
        escape_html(&cls)
    ));
    for (k, v) in &node.attributes {
        if k == "id" || k == "class" || k == "style" {
            continue;
        }
        let short_v: String = v.chars().take(30).collect();
        html.push_str(&format!(
            " <span class='attr-name'>{}</span>=<span class='attr-val'>\"{}\"</span>",
            escape_html(k),
            escape_html(&short_v)
        ));
    }
    html.push_str("<span class='tag'>&gt;</span></div>");

    // ── Tab bar ──
    html.push_str(&format!(
        "<div class='tab-bar'>\
         <div style='{}'>Styles</div>\
         <div style='{}'>Computed</div>\
         <div style='{}'>Box Model</div>\
         <div style='{}'>DOM</div>\
         <div style='{}'>Layout</div>\
         <div style='{}'>Attrs</div>\
         </div>",
        tab_style(0),
        tab_style(1),
        tab_style(2),
        tab_style(3),
        tab_style(4),
        tab_style(5)
    ));

    html.push_str("<div class='panel'>");

    match active_tab {
        0 => {
            // ── Styles tab: matched CSS rules ──
            if !node.matched_rules.is_empty() {
                let mut seen_props: std::collections::HashSet<String> =
                    std::collections::HashSet::new();
                let mut rules_rev: Vec<_> = node.matched_rules.iter().collect();
                rules_rev.reverse();
                for rule in &rules_rev {
                    html.push_str("<div class='rule-block'>");
                    let src_label = if rule.source == "ua" {
                        " (user agent)"
                    } else {
                        ""
                    };
                    html.push_str(&format!(
                        "<div><span class='sel'>{}</span> <span class='rule-src'>sp:{}{}</span></div>",
                        escape_html(&rule.selector), rule.specificity, src_label
                    ));
                    for (prop, val) in &rule.declarations {
                        if prop.starts_with("--") {
                            continue;
                        }
                        let overridden = seen_props.contains(prop);
                        let cls = if overridden {
                            "decl overridden"
                        } else {
                            "decl"
                        };
                        let swatch = color_swatch(val);
                        html.push_str(&format!(
                            "<div class='{cls}'><span class='prop'>{prop}</span>: {swatch}<span class='val'>{}</span>;</div>",
                            escape_html(val)
                        ));
                    }
                    html.push_str("</div>");
                    for (prop, _) in &rule.declarations {
                        if !prop.starts_with("--") {
                            seen_props.insert(prop.clone());
                        }
                    }
                }
            } else {
                html.push_str("<div style='color:#666;padding:10px'>No matched rules (enable inspect before cascade)</div>");
            }
        }
        1 => {
            // ── Computed tab ──
            let bg = s.background_color;
            let bg_str = if bg.a > 0 {
                format!("#{:02x}{:02x}{:02x}", bg.r, bg.g, bg.b)
            } else {
                "transparent".into()
            };
            let props: Vec<(&str, String)> = vec![
                ("display", format!("{:?}", s.display)),
                ("position", format!("{:?}", s.position)),
                ("float", format!("{:?}", s.float)),
                ("box-sizing", format!("{:?}", s.box_sizing)),
                ("width", format!("{:?}", s.width)),
                ("height", format!("{:?}", s.height)),
                ("min-width", format!("{:?}", s.min_width)),
                ("max-width", format!("{:?}", s.max_width)),
                (
                    "overflow",
                    format!("{:?} / {:?}", s.overflow_x, s.overflow_y),
                ),
                ("flex-direction", format!("{:?}", s.flex_direction)),
                ("flex-wrap", format!("{:?}", s.flex_wrap)),
                (
                    "flex",
                    format!("{} {} {:?}", s.flex_grow, s.flex_shrink, s.flex_basis),
                ),
                ("align-items", format!("{:?}", s.align_items)),
                ("align-self", format!("{:?}", s.align_self)),
                ("justify-content", format!("{:?}", s.justify_content)),
                ("vertical-align", format!("{:?}", s.vertical_align)),
                ("text-align", format!("{:?}", s.text_align)),
                ("font-size", format!("{:.1}px", s.font_size_px(16.0, 16.0))),
                ("line-height", format!("{:?}", s.line_height)),
                (
                    "color",
                    format!("#{:02x}{:02x}{:02x}", s.color.r, s.color.g, s.color.b),
                ),
                ("background", bg_str),
                ("z-index", format!("{}", s.z_index)),
            ];
            for (name, val) in &props {
                html.push_str(&format!(
                    "<div class='computed-row'><span class='prop'>{name}</span><span class='val'>{val}</span></div>"
                ));
            }
            // Children summary
            let elem_children: Vec<_> = node
                .children
                .iter()
                .filter(|c| {
                    c.tag != "#text" && !matches!(c.style.display, crate::types::Display::None)
                })
                .collect();
            if !elem_children.is_empty() {
                html.push_str("<h3>Children</h3>");
                html.push_str("<div class='dom-tree'>");
                for child in &elem_children {
                    let cid = child
                        .attributes
                        .get("id")
                        .map(|v| format!("#{v}"))
                        .unwrap_or_default();
                    let ccls = child
                        .attributes
                        .get("class")
                        .map(|v| {
                            format!(
                                ".{}",
                                v.split_whitespace().take(3).collect::<Vec<_>>().join(".")
                            )
                        })
                        .unwrap_or_default();
                    html.push_str(&format!(
                        "<div class='dom-node'><span class='tag'>{}</span><span class='id'>{}</span><span class='cls'>{}</span> \
                         <span style='color:#666'>{:?} {:.0}x{:.0}</span></div>",
                        escape_html(&child.tag), escape_html(&cid), escape_html(&ccls),
                        child.style.display, child.layout.content_rect.w, child.layout.content_rect.h
                    ));
                }
                html.push_str("</div>");
            }
        }
        2 => {
            // ── Box Model tab ──
            let c = &node.layout.content_rect;
            let mt = node.layout.resolved_margin_top;
            let mr = node.layout.resolved_margin_right;
            let mb = node.layout.resolved_margin_bottom;
            let ml = node.layout.resolved_margin_left;
            let bt = node.layout.resolved_border_top;
            let bbr = node.layout.resolved_border_right;
            let bb = node.layout.resolved_border_bottom;
            let bbl = node.layout.resolved_border_left;
            let pt = node.layout.resolved_pad_top;
            let pr = node.layout.resolved_pad_right;
            let pb = node.layout.resolved_pad_bottom;
            let pll = node.layout.resolved_pad_left;
            html.push_str(&format!(
                "<div class='box-model'>\
                 <div class='bm-margin'>\
                   <div class='bm-top'>{mt:.0}</div>\
                   <div class='bm-row'><div class='bm-side'>{ml:.0}</div><div class='bm-center'>\
                     <div class='bm-padding'>\
                       <div class='bm-top'>{pt:.0}</div>\
                       <div class='bm-row'><div class='bm-side'>{pll:.0}</div>\
                         <div class='bm-content'>{:.0} x {:.0}</div>\
                       <div class='bm-side'>{pr:.0}</div></div>\
                       <div class='bm-bot'>{pb:.0}</div>\
                     </div>\
                   </div><div class='bm-side'>{mr:.0}</div></div>\
                   <div class='bm-bot'>{mb:.0}</div>\
                 </div></div>",
                c.w, c.h
            ));
            if bt > 0.0 || bbr > 0.0 || bb > 0.0 || bbl > 0.0 {
                html.push_str(&format!(
                    "<div style='color:#888;font-size:10px;text-align:center'>border: {bt:.0} {bbr:.0} {bb:.0} {bbl:.0}</div>"
                ));
            }
            html.push_str(&format!(
                "<div style='color:#666;font-size:10px;text-align:center'>position: ({:.0}, {:.0}) size: {:.0} x {:.0}</div>",
                c.x, c.y, node.layout.margin_rect.w, node.layout.margin_rect.h
            ));
        }
        3 => {
            // ── DOM tab: ancestor chain + children tree ──
            // Ancestor chain (class chain)
            html.push_str("<h3>Ancestor Chain</h3>");
            if let Some(root) = doc_root {
                let mut chain: Vec<String> = Vec::new();
                fn find_chain(
                    cur: &crate::WebCore,
                    target_id: u32,
                    chain: &mut Vec<String>,
                ) -> bool {
                    let id_str = cur
                        .attributes
                        .get("id")
                        .map(|v| format!("#{v}"))
                        .unwrap_or_default();
                    let cls_str = cur
                        .attributes
                        .get("class")
                        .map(|v| {
                            format!(
                                ".{}",
                                v.split_whitespace().take(3).collect::<Vec<_>>().join(".")
                            )
                        })
                        .unwrap_or_default();
                    let label = format!("{}{}{}", cur.tag, id_str, cls_str);
                    chain.push(label);
                    if cur.node_id == target_id {
                        return true;
                    }
                    for child in &cur.children {
                        if find_chain(child, target_id, chain) {
                            return true;
                        }
                    }
                    chain.pop();
                    false
                }
                find_chain(root, node.node_id, &mut chain);
                for (i, item) in chain.iter().enumerate() {
                    let indent = i * 12;
                    let is_last = i == chain.len() - 1;
                    let weight = if is_last {
                        "font-weight:600;color:#4fc3f7"
                    } else {
                        "color:#999"
                    };
                    html.push_str(&format!(
                        "<div style='padding-left:{}px;{}'>{}{}</div>",
                        indent,
                        weight,
                        if i > 0 { "└ " } else { "" },
                        escape_html(item)
                    ));
                }
            }

            // Children tree
            html.push_str("<h3>Children</h3>");
            if node.children.is_empty() {
                html.push_str("<div style='color:#666'>(no children)</div>");
            } else {
                html.push_str("<div class='dom-tree'>");
                for child in &node.children {
                    if child.tag == "#text" && child.text.trim().is_empty() {
                        continue;
                    }
                    let cid = child
                        .attributes
                        .get("id")
                        .map(|v| format!("#{v}"))
                        .unwrap_or_default();
                    let ccls = child
                        .attributes
                        .get("class")
                        .map(|v| {
                            format!(
                                ".{}",
                                v.split_whitespace().take(3).collect::<Vec<_>>().join(".")
                            )
                        })
                        .unwrap_or_default();
                    if child.tag == "#text" {
                        let preview: String = child.text.trim().chars().take(40).collect();
                        html.push_str(&format!(
                            "<div class='dom-node' style='color:#6a9955;font-style:italic'>\"{}\"</div>",
                            escape_html(&preview)
                        ));
                    } else {
                        let n_kids = child
                            .children
                            .iter()
                            .filter(|c| !(c.tag == "#text" && c.text.trim().is_empty()))
                            .count();
                        html.push_str(&format!(
                            "<div class='dom-node'><span class='tag'>{}</span><span class='id'>{}</span><span class='cls'>{}</span>\
                             <span style='color:#555'> {:?} {:.0}x{:.0} ({} children)</span></div>",
                            escape_html(&child.tag), escape_html(&cid), escape_html(&ccls),
                            child.style.display, child.layout.content_rect.w, child.layout.content_rect.h,
                            n_kids
                        ));
                    }
                }
                html.push_str("</div>");
            }
        }
        4 => {
            // ── Layout tab: detailed geometry + layout flags ──
            let l = &node.layout;
            html.push_str("<h3>Geometry</h3>");
            let rects: Vec<(&str, &crate::Rect)> = vec![
                ("content", &l.content_rect),
                ("padding", &l.padding_rect),
                ("border", &l.border_rect),
                ("margin", &l.margin_rect),
            ];
            for (name, r) in &rects {
                html.push_str(&format!(
                    "<div class='computed-row'><span class='prop'>{}</span><span class='val'>({:.1}, {:.1}) {:.1} × {:.1}</span></div>",
                    name, r.x, r.y, r.w, r.h
                ));
            }

            html.push_str("<h3>Resolved Box</h3>");
            let box_props: Vec<(&str, f32)> = vec![
                ("margin-top", l.resolved_margin_top),
                ("margin-right", l.resolved_margin_right),
                ("margin-bottom", l.resolved_margin_bottom),
                ("margin-left", l.resolved_margin_left),
                ("border-top", l.resolved_border_top),
                ("border-right", l.resolved_border_right),
                ("border-bottom", l.resolved_border_bottom),
                ("border-left", l.resolved_border_left),
                ("padding-top", l.resolved_pad_top),
                ("padding-right", l.resolved_pad_right),
                ("padding-bottom", l.resolved_pad_bottom),
                ("padding-left", l.resolved_pad_left),
                ("content-width", l.resolved_content_width),
                ("baseline", l.baseline),
            ];
            for (name, val) in &box_props {
                if *val != 0.0 {
                    html.push_str(&format!(
                        "<div class='computed-row'><span class='prop'>{}</span><span class='val'>{:.1}px</span></div>",
                        name, val
                    ));
                }
            }

            html.push_str("<h3>Scroll</h3>");
            html.push_str(&format!(
                "<div class='computed-row'><span class='prop'>scroll</span><span class='val'>{:.0} x {:.0} (top: {:.0}, left: {:.0})</span></div>",
                l.scroll_width, l.scroll_height, l.scroll_top, l.scroll_left
            ));

            html.push_str("<h3>Inline</h3>");
            html.push_str(&format!(
                "<div class='computed-row'><span class='prop'>line-cache</span><span class='val'>{} lines</span></div>",
                l.line_cache.len()
            ));
            html.push_str(&format!(
                "<div class='computed-row'><span class='prop'>inline-runs</span><span class='val'>{} runs</span></div>",
                l.inline_runs.len()
            ));
            for (i, line) in l.line_cache.iter().enumerate().take(10) {
                html.push_str(&format!(
                    "<div class='computed-row'><span class='prop'>line {}</span><span class='val'>y={:.0} h={:.0} w={:.0} chars={}</span></div>",
                    i, line.y, line.height, line.width, line.char_x.len()
                ));
            }

            html.push_str("<h3>Flags</h3>");
            html.push_str(&format!(
                "<div class='computed-row'><span class='prop'>layout_dirty</span><span class='val'>{}</span></div>",
                l.layout_dirty
            ));
            html.push_str(&format!(
                "<div class='computed-row'><span class='prop'>node_id</span><span class='val'>{}</span></div>",
                node.node_id
            ));
            if node.image_width > 0 {
                html.push_str(&format!(
                    "<div class='computed-row'><span class='prop'>image</span><span class='val'>{}x{} ({} bytes)</span></div>",
                    node.image_width, node.image_height, node.image_data.as_ref().map(|d| d.len()).unwrap_or(0)
                ));
            }
        }
        5 => {
            // ── Attributes tab ──
            html.push_str("<h3>Attributes</h3>");
            if node.attributes.is_empty() {
                html.push_str("<div style='color:#666'>(none)</div>");
            } else {
                let mut attrs: Vec<_> = node.attributes.iter().collect();
                attrs.sort_by_key(|(k, _)| k.as_str());
                for (k, v) in &attrs {
                    let swatch = if k.as_str() == "style" || k.contains("color") {
                        color_swatch(v)
                    } else {
                        String::new()
                    };
                    html.push_str(&format!(
                        "<div class='computed-row'><span class='prop'>{}</span>{}<span class='val' style='word-break:break-all'>{}</span></div>",
                        escape_html(k), swatch, escape_html(v)
                    ));
                }
            }

            // Data attributes
            if !node.data.is_empty() {
                html.push_str("<h3>Custom Data</h3>");
                let mut data: Vec<_> = node.data.iter().collect();
                data.sort_by_key(|(k, _)| k.as_str());
                for (k, v) in &data {
                    html.push_str(&format!(
                        "<div class='computed-row'><span class='prop'>{}</span><span class='val'>{}</span></div>",
                        escape_html(k), escape_html(v)
                    ));
                }
            }

            // Inline style
            if let Some(style_attr) = node.attributes.get("style") {
                html.push_str("<h3>Inline Style</h3>");
                for decl in style_attr.split(';') {
                    let decl = decl.trim();
                    if decl.is_empty() {
                        continue;
                    }
                    if let Some(colon) = decl.find(':') {
                        let prop = decl[..colon].trim();
                        let val = decl[colon + 1..].trim();
                        let swatch = color_swatch(val);
                        html.push_str(&format!(
                            "<div class='computed-row'><span class='prop'>{}</span>{}<span class='val'>{}</span></div>",
                            escape_html(prop), swatch, escape_html(val)
                        ));
                    }
                }
            }
        }
        _ => {}
    }

    html.push_str("</div></body></html>");
    html
}
