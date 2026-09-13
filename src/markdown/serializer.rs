// ============================================================
// Markdown Serializer — converts Box tree back to Markdown
// Mirrors MarkdownSerializer.cpp
// ============================================================

use crate::types::*;

// Collect all text from a box's inline_runs (and recursively children)
fn collect_inline_text(b: &WebCore) -> String {
    let mut result = String::new();
    for run in &b.layout.inline_runs {
        let end = run.text_offset + run.length;
        if end <= b.text.len() {
            result.push_str(&b.text[run.text_offset..end]);
        }
    }
    for child in &b.children {
        result.push_str(&collect_inline_text(child));
    }
    result
}

// Serialize inline content with Markdown formatting
fn serialize_inline(b: &WebCore, block_style: Option<&ComputedStyle>) -> String {
    let mut result = String::new();

    let block_is_bold = block_style
        .map(|s| s.font_weight == FontWeight::Bold)
        .unwrap_or(false);
    let block_is_italic = block_style
        .map(|s| s.font_style == FontStyle::Italic)
        .unwrap_or(false);
    let block_is_mono = block_style
        .map(|s| s.font_family == "monospace")
        .unwrap_or(false);

    let is_autolink = b.data.contains_key("md-autolink");

    for run in &b.layout.inline_runs {
        let end = run.text_offset + run.length;
        let chunk = if end <= b.text.len() {
            &b.text[run.text_offset..end]
        } else {
            ""
        };

        let is_bold = !block_is_bold && run.style.font_weight == FontWeight::Bold;
        let is_italic = !block_is_italic && run.style.font_style == FontStyle::Italic;
        let is_strike = run.style.text_decoration.strikethrough;
        let is_code = !block_is_mono && run.style.font_family == "monospace";
        let is_link = !run.style.href.is_empty();
        let is_highlight = run.style.background_color == Color::rgb(255, 255, 0)
            && b.data.contains_key("md-highlight");

        let mut prefix = String::new();
        let mut suffix = String::new();

        if is_code {
            if chunk.contains('`') {
                prefix = "`` ".to_string();
                suffix = " ``".to_string();
            } else {
                prefix = "`".to_string();
                suffix = "`".to_string();
            }
        } else {
            if is_highlight {
                prefix.push_str("==");
                suffix = format!("=={}", suffix);
            }
            if is_strike {
                prefix.push_str("~~");
                suffix = format!("~~{}", suffix);
            }
            if is_bold && is_italic {
                prefix.push_str("***");
                suffix = format!("***{}", suffix);
            } else {
                if is_bold {
                    let delim = b
                        .data
                        .get("md-bold-delim")
                        .map(|s| s.as_str())
                        .unwrap_or("**");
                    prefix.push_str(delim);
                    suffix = format!("{}{}", delim, suffix);
                }
                if is_italic {
                    let delim = b
                        .data
                        .get("md-italic-delim")
                        .map(|s| s.as_str())
                        .unwrap_or("*");
                    prefix.push_str(delim);
                    suffix = format!("{}{}", delim, suffix);
                }
            }
        }

        if is_link {
            let url = &run.style.href;
            if is_autolink {
                result.push_str(&format!("<{}{}{}>", prefix, chunk, suffix));
            } else if b.data.contains_key("md-ref-link") {
                let ref_id = b.data.get("md-ref-link").map(|s| s.as_str()).unwrap_or("");
                if b.data.contains_key("md-ref-shortcut") {
                    result.push_str(&format!("[{}{}{}]", prefix, chunk, suffix));
                } else {
                    result.push_str(&format!("[{}{}{}][{}]", prefix, chunk, suffix, ref_id));
                }
            } else {
                result.push_str(&format!("[{}{}{}]({})", prefix, chunk, suffix, url));
            }
        } else {
            result.push_str(&prefix);
            result.push_str(chunk);
            result.push_str(&suffix);
        }
    }

    // Process child boxes (images, etc.)
    for child in &b.children {
        if child.tag == "img" {
            let src = child
                .attributes
                .get("src")
                .map(|s| s.as_str())
                .unwrap_or("");
            let alt = child.data.get("md-alt").map(|s| s.as_str()).unwrap_or("");
            result.push_str(&format!("![{}]({})", alt, src));
        } else {
            result.push_str(&serialize_inline(child, None));
        }
    }

    result
}

fn serialize_block(b: &WebCore, out: &mut String, indent: usize, needs_blank_line: &mut bool) {
    let tag = b.tag.as_str();

    // Footnote section div
    if tag == "div" && b.data.contains_key("md-footnotes") {
        if *needs_blank_line {
            out.push('\n');
        }
        for child in &b.children {
            if child.tag == "hr" {
                continue; // Skip the hr
            }
            if child.tag == "p" {
                if let Some(fn_id) = child.data.get("md-footnote-def") {
                    let content = serialize_inline(child, None);
                    out.push_str(&format!("[^{}]: {}\n", fn_id, content));
                }
            }
        }
        *needs_blank_line = true;
        return;
    }

    // Raw HTML block
    if tag == "div" && b.data.contains_key("md-raw-html") {
        if *needs_blank_line {
            out.push('\n');
        }
        let html_text = collect_inline_text(b);
        out.push_str(&html_text);
        out.push('\n');
        *needs_blank_line = true;
        return;
    }

    // Headings
    if tag.len() == 2 && tag.starts_with('h') {
        let level_ch = tag.chars().nth(1).unwrap_or('1');
        if level_ch >= '1' && level_ch <= '6' {
            if *needs_blank_line {
                out.push('\n');
            }
            let level = level_ch as usize - '0' as usize;
            let content = serialize_inline(b, Some(&b.style));

            if let Some(heading_type) = b.data.get("md-heading") {
                if heading_type == "setext" && level <= 2 {
                    out.push_str(&content);
                    out.push('\n');
                    let under_char = if level == 1 { '=' } else { '-' };
                    let under_char = b
                        .data
                        .get("md-setext-char")
                        .and_then(|s| s.chars().next())
                        .unwrap_or(under_char);
                    // Use unicode-aware length for the underline
                    let char_count = content.chars().count();
                    for _ in 0..char_count {
                        out.push(under_char);
                    }
                    out.push('\n');
                    *needs_blank_line = true;
                    return;
                }
            }
            // ATX heading
            for _ in 0..level {
                out.push('#');
            }
            out.push(' ');
            out.push_str(&content);
            out.push('\n');
            *needs_blank_line = true;
            return;
        }
    }

    // Paragraph
    if tag == "p" {
        if *needs_blank_line {
            out.push('\n');
        }
        let content = serialize_inline(b, None);
        out.push_str(&content);
        out.push('\n');
        *needs_blank_line = true;
        return;
    }

    // Horizontal rule
    if tag == "hr" {
        if *needs_blank_line {
            out.push('\n');
        }
        let marker = b.data.get("md-marker").map(|s| s.as_str()).unwrap_or("---");
        out.push_str(marker);
        out.push('\n');
        *needs_blank_line = true;
        return;
    }

    // Code block (pre)
    if tag == "pre" {
        if *needs_blank_line {
            out.push('\n');
        }
        if b.data
            .get("md-code-style")
            .map(|s| s == "indented")
            .unwrap_or(false)
        {
            let code_text = collect_inline_text(b);
            for line in code_text.split('\n') {
                out.push_str("    ");
                out.push_str(line);
                out.push('\n');
            }
        } else {
            let fence = b.data.get("md-fence").map(|s| s.as_str()).unwrap_or("```");
            let lang = b.data.get("md-lang").map(|s| s.as_str()).unwrap_or("");
            out.push_str(fence);
            out.push_str(lang);
            out.push('\n');
            let code_text = collect_inline_text(b);
            out.push_str(&code_text);
            out.push('\n');
            out.push_str(fence);
            out.push('\n');
        }
        *needs_blank_line = true;
        return;
    }

    // Blockquote
    if tag == "blockquote" {
        if *needs_blank_line {
            out.push('\n');
        }
        let mut bq_out = String::new();
        let mut bq_needs_bl = false;
        for child in &b.children {
            serialize_block(child, &mut bq_out, 0, &mut bq_needs_bl);
        }
        for line in bq_out.split('\n') {
            if line.is_empty() {
                out.push_str(">\n");
            } else {
                out.push_str("> ");
                out.push_str(line);
                out.push('\n');
            }
        }
        *needs_blank_line = true;
        return;
    }

    // Definition list
    if tag == "dl" {
        if *needs_blank_line {
            out.push('\n');
        }
        for child in &b.children {
            if child.tag == "dt" {
                let content = serialize_inline(child, None);
                out.push_str(&content);
                out.push('\n');
            } else if child.tag == "dd" {
                let content = serialize_inline(child, None);
                out.push_str(": ");
                out.push_str(&content);
                out.push('\n');
            }
        }
        *needs_blank_line = true;
        return;
    }

    // Unordered / ordered list
    if tag == "ul" || tag == "ol" {
        if *needs_blank_line {
            out.push('\n');
        }
        let bullet = b.data.get("md-bullet").map(|s| s.as_str()).unwrap_or("-");
        let start_num: i32 = b
            .data
            .get("md-start")
            .and_then(|s| s.parse().ok())
            .unwrap_or(1);
        let mut item_num = start_num;

        for child in &b.children {
            if child.tag != "li" {
                continue;
            }
            let prefix = if tag == "ol" {
                let p = format!("{}. ", item_num);
                item_num += 1;
                p
            } else {
                format!("{} ", bullet)
            };

            // Task list prefix
            let task_prefix = if let Some(task) = child.data.get("md-task") {
                if task == "checked" { "[x] " } else { "[ ] " }
            } else {
                ""
            };

            let indent_str: String = std::iter::repeat(' ').take(indent).collect();
            let content = serialize_inline(child, None);
            out.push_str(&indent_str);
            out.push_str(&prefix);
            out.push_str(task_prefix);
            out.push_str(&content);
            out.push('\n');

            // Check for nested lists
            for nested in &child.children {
                if nested.tag == "ul" || nested.tag == "ol" {
                    let mut nested_bl = false;
                    serialize_block(nested, out, indent + prefix.len(), &mut nested_bl);
                }
            }
        }
        *needs_blank_line = true;
        return;
    }

    // Table
    if tag == "table" {
        if *needs_blank_line {
            out.push('\n');
        }
        let mut rows: Vec<Vec<String>> = Vec::new();
        let mut aligns: Vec<TextAlign> = Vec::new();

        if let Some(align_str) = b.data.get("md-align") {
            for a in align_str.split(',') {
                match a {
                    "center" => aligns.push(TextAlign::Center),
                    "right" => aligns.push(TextAlign::Right),
                    _ => aligns.push(TextAlign::Left),
                }
            }
        }

        for section in &b.children {
            for row in &section.children {
                if row.tag != "tr" {
                    continue;
                }
                let cells: Vec<String> = row
                    .children
                    .iter()
                    .map(|cell| serialize_inline(cell, Some(&cell.style)))
                    .collect();
                rows.push(cells);
            }
        }

        if rows.is_empty() {
            return;
        }

        let cols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
        if aligns.is_empty() {
            aligns.resize(cols, TextAlign::Left);
        }
        let mut widths: Vec<usize> = vec![3; cols];
        for row in &rows {
            for (i, cell) in row.iter().enumerate() {
                if i < cols {
                    widths[i] = widths[i].max(cell.len());
                }
            }
        }

        // Header row
        out.push('|');
        for i in 0..cols {
            let cell = rows[0].get(i).map(|s| s.as_str()).unwrap_or("");
            let pad = widths[i].saturating_sub(cell.len());
            out.push(' ');
            out.push_str(cell);
            for _ in 0..pad {
                out.push(' ');
            }
            out.push_str(" |");
        }
        out.push('\n');

        // Separator row
        out.push('|');
        for i in 0..cols {
            let align = aligns.get(i).copied().unwrap_or(TextAlign::Left);
            let sep: String = std::iter::repeat('-').take(widths[i]).collect();
            match align {
                TextAlign::Center => out.push_str(&format!(":{}:|", sep)),
                TextAlign::Right => out.push_str(&format!(" {}:|", sep)),
                _ => out.push_str(&format!(" {} |", sep)),
            }
        }
        out.push('\n');

        // Data rows
        for r in 1..rows.len() {
            out.push('|');
            for i in 0..cols {
                let cell = rows[r].get(i).map(|s| s.as_str()).unwrap_or("");
                let pad = widths[i].saturating_sub(cell.len());
                out.push(' ');
                out.push_str(cell);
                for _ in 0..pad {
                    out.push(' ');
                }
                out.push_str(" |");
            }
            out.push('\n');
        }

        *needs_blank_line = true;
        return;
    }

    // Default: serialize children
    for child in &b.children {
        serialize_block(child, out, indent, needs_blank_line);
    }
}

/// Serialize a Document's box tree back to Markdown.
pub fn serialize_markdown(doc: &Document) -> String {
    let mut out = String::new();
    let mut needs_blank_line = false;
    for child in &doc.root.children {
        serialize_block(child, &mut out, 0, &mut needs_blank_line);
    }
    // Trim trailing extra newlines to a single one
    while out.len() > 1 && out.ends_with('\n') && out[..out.len() - 1].ends_with('\n') {
        out.pop();
    }
    out
}
