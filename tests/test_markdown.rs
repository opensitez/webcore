// Ported from cpptests/test_markdown.cpp
// Tests for Markdown parsing, serialization, and round-trip.
// Widget API tests (MdWidget) are skipped — require wxHtmlEditWidget.

use webcore::types::*;
use webcore::{load_html, parse_html, parse_markdown, serialize_markdown, LayoutEngine};

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn find_box<'a, F: Fn(&WebCore) -> bool>(root: &'a WebCore, pred: &F) -> Option<&'a WebCore> {
    if pred(root) {
        return Some(root);
    }
    for child in &root.children {
        if let Some(b) = find_box(child, pred) {
            return Some(b);
        }
    }
    None
}

fn count_boxes<F: Fn(&WebCore) -> bool>(root: &WebCore, pred: &F) -> usize {
    let mut n = if pred(root) { 1 } else { 0 };
    for child in &root.children {
        n += count_boxes(child, pred);
    }
    n
}

fn find_tag<'a>(doc: &'a Document, tag: &str) -> Option<&'a WebCore> {
    let t = tag.to_string();
    find_box(&doc.root, &|b| b.tag == t)
}

fn get_text(b: &WebCore) -> String {
    let mut result = String::new();
    for run in &b.layout.inline_runs {
        let end = run.text_offset + run.length;
        if end <= b.text.len() {
            result.push_str(&b.text[run.text_offset..end]);
        }
    }
    for child in &b.children {
        result.push_str(&get_text(child));
    }
    result
}

// ============================================================
// ATX Headings
// ============================================================

#[test]
fn md_atx_heading1() {
    let doc = parse_markdown("# Hello");
    let h = find_tag(&doc, "h1");
    assert!(h.is_some(), "h1 not found");
    assert!(get_text(h.unwrap()).contains("Hello"));
}

#[test]
fn md_atx_heading2() {
    let doc = parse_markdown("## World");
    let h = find_tag(&doc, "h2");
    assert!(h.is_some(), "h2 not found");
    assert!(get_text(h.unwrap()).contains("World"));
}

#[test]
fn md_atx_heading6() {
    let doc = parse_markdown("###### Deep");
    let h = find_tag(&doc, "h6");
    assert!(h.is_some(), "h6 not found");
    assert!(get_text(h.unwrap()).contains("Deep"));
}

#[test]
fn md_atx_heading_trailing_hashes() {
    let doc = parse_markdown("## Title ##");
    let h = find_tag(&doc, "h2");
    assert!(h.is_some(), "h2 not found");
    assert!(
        get_text(h.unwrap()).contains("Title"),
        "Text should contain 'Title', got: {:?}",
        get_text(h.unwrap())
    );
}

// ============================================================
// Setext Headings
// ============================================================

#[test]
fn md_setext_heading1() {
    let doc = parse_markdown("Title\n=====");
    let h = find_tag(&doc, "h1");
    assert!(h.is_some(), "h1 not found");
    let h = h.unwrap();
    assert!(get_text(h).contains("Title"));
    assert_eq!(h.data.get("md-heading").map(|s| s.as_str()), Some("setext"));
}

#[test]
fn md_setext_heading2() {
    let doc = parse_markdown("Subtitle\n--------");
    let h = find_tag(&doc, "h2");
    assert!(h.is_some(), "h2 not found");
    assert!(get_text(h.unwrap()).contains("Subtitle"));
}

// ============================================================
// Paragraphs
// ============================================================

#[test]
fn md_simple_paragraph() {
    let doc = parse_markdown("Hello world");
    let p = find_tag(&doc, "p");
    assert!(p.is_some(), "p not found");
    assert!(get_text(p.unwrap()).contains("Hello world"));
}

#[test]
fn md_two_paragraphs() {
    let doc = parse_markdown("First\n\nSecond");
    let count = count_boxes(&doc.root, &|b| b.tag == "p");
    assert_eq!(count, 2, "Expected 2 paragraphs, got {}", count);
}

#[test]
fn md_paragraph_soft_break() {
    let doc = parse_markdown("Line 1\nLine 2");
    let count = count_boxes(&doc.root, &|b| b.tag == "p");
    assert_eq!(count, 1, "Soft break should not create 2 paragraphs");
    let p = find_tag(&doc, "p").unwrap();
    let text = get_text(p);
    assert!(text.contains("Line 1"), "text: {:?}", text);
    assert!(text.contains("Line 2"), "text: {:?}", text);
}

// ============================================================
// Inline formatting
// ============================================================

#[test]
fn md_bold() {
    let doc = parse_markdown("This is **bold** text");
    let p = find_tag(&doc, "p").expect("p not found");
    let found = p
        .layout
        .inline_runs
        .iter()
        .any(|r| r.style.font_weight == FontWeight::Bold);
    assert!(found, "No bold run found");
}

#[test]
fn md_italic() {
    let doc = parse_markdown("This is *italic* text");
    let p = find_tag(&doc, "p").expect("p not found");
    let found = p
        .layout
        .inline_runs
        .iter()
        .any(|r| r.style.font_style == FontStyle::Italic);
    assert!(found, "No italic run found");
}

#[test]
fn md_bold_italic() {
    let doc = parse_markdown("***bold italic***");
    let p = find_tag(&doc, "p").expect("p not found");
    let found = p.layout.inline_runs.iter().any(|r| {
        r.style.font_weight == FontWeight::Bold && r.style.font_style == FontStyle::Italic
    });
    assert!(found, "No bold+italic run found");
}

#[test]
fn md_strikethrough() {
    let doc = parse_markdown("~~deleted~~");
    let p = find_tag(&doc, "p").expect("p not found");
    let found = p
        .layout
        .inline_runs
        .iter()
        .any(|r| r.style.text_decoration.strikethrough);
    assert!(found, "No strikethrough run found");
}

#[test]
fn md_inline_code() {
    let doc = parse_markdown("Use `printf()` here");
    let p = find_tag(&doc, "p").expect("p not found");
    let found = p
        .layout
        .inline_runs
        .iter()
        .any(|r| r.style.font_family == "monospace");
    assert!(found, "No monospace run found");
}

#[test]
fn md_underscore_bold() {
    let doc = parse_markdown("__bold__");
    let p = find_tag(&doc, "p").expect("p not found");
    let found = p
        .layout
        .inline_runs
        .iter()
        .any(|r| r.style.font_weight == FontWeight::Bold);
    assert!(found, "No bold run found");
}

#[test]
fn md_underscore_italic() {
    let doc = parse_markdown("_italic_");
    let p = find_tag(&doc, "p").expect("p not found");
    let found = p
        .layout
        .inline_runs
        .iter()
        .any(|r| r.style.font_style == FontStyle::Italic);
    assert!(found, "No italic run found");
}

// ============================================================
// Links
// ============================================================

#[test]
fn md_inline_link() {
    let doc = parse_markdown("[click me](https://example.com)");
    let p = find_tag(&doc, "p").expect("p not found");
    let found = p
        .layout
        .inline_runs
        .iter()
        .any(|r| r.style.href == "https://example.com");
    assert!(found, "No link run with correct href found");
}

#[test]
fn md_link_text() {
    let doc = parse_markdown("[click me](https://example.com)");
    let p = find_tag(&doc, "p").expect("p not found");
    assert!(get_text(p).contains("click me"));
}

// ============================================================
// Images
// ============================================================

#[test]
fn md_image() {
    let doc = parse_markdown("![alt text](image.png)");
    let img = find_box(&doc.root, &|b| b.tag == "img");
    assert!(img.is_some(), "img not found");
    let img = img.unwrap();
    assert_eq!(
        img.attributes.get("src").map(|s| s.as_str()),
        Some("image.png")
    );
    assert_eq!(img.data.get("md-alt").map(|s| s.as_str()), Some("alt text"));
}

// ============================================================
// Code blocks
// ============================================================

#[test]
fn md_fenced_code_block() {
    let doc = parse_markdown("```\ncode here\n```");
    let pre = find_tag(&doc, "pre").expect("pre not found");
    assert!(get_text(pre).contains("code here"));
}

#[test]
fn md_fenced_code_block_with_lang() {
    let doc = parse_markdown("```python\nprint('hi')\n```");
    let pre = find_tag(&doc, "pre").expect("pre not found");
    assert_eq!(pre.data.get("md-lang").map(|s| s.as_str()), Some("python"));
}

#[test]
fn md_tilde_fence() {
    let doc = parse_markdown("~~~\ncode\n~~~");
    let pre = find_tag(&doc, "pre").expect("pre not found");
    assert_eq!(pre.data.get("md-fence").map(|s| s.as_str()), Some("~~~"));
}

#[test]
fn md_fenced_code_multi_line() {
    let doc = parse_markdown("```\nline 1\nline 2\nline 3\n```");
    let pre = find_tag(&doc, "pre").expect("pre not found");
    let text = get_text(pre);
    assert!(text.contains("line 1"));
    assert!(text.contains("line 2"));
    assert!(text.contains("line 3"));
}

// ============================================================
// Blockquotes
// ============================================================

#[test]
fn md_blockquote() {
    let doc = parse_markdown("> Quote text");
    let bq = find_tag(&doc, "blockquote").expect("blockquote not found");
    let p = find_box(bq, &|b| b.tag == "p").expect("p inside blockquote not found");
    assert!(get_text(p).contains("Quote text"));
}

#[test]
fn md_nested_blockquote() {
    let doc = parse_markdown("> > Nested");
    let bq = find_tag(&doc, "blockquote").expect("outer blockquote not found");
    let inner = find_box(bq, &|b| b.tag == "blockquote");
    assert!(inner.is_some(), "Inner blockquote not found");
}

// ============================================================
// Unordered lists
// ============================================================

#[test]
fn md_unordered_list() {
    let doc = parse_markdown("- Item 1\n- Item 2\n- Item 3");
    let ul = find_tag(&doc, "ul").expect("ul not found");
    let li_count = ul.children.iter().filter(|c| c.tag == "li").count();
    assert_eq!(li_count, 3);
}

#[test]
fn md_unordered_list_bullet_preserved() {
    let doc = parse_markdown("* A\n* B");
    let ul = find_tag(&doc, "ul").expect("ul not found");
    assert_eq!(ul.data.get("md-bullet").map(|s| s.as_str()), Some("*"));
}

#[test]
fn md_unordered_list_plus() {
    let doc = parse_markdown("+ A\n+ B");
    let ul = find_tag(&doc, "ul").expect("ul not found");
    assert_eq!(ul.data.get("md-bullet").map(|s| s.as_str()), Some("+"));
}

// ============================================================
// Ordered lists
// ============================================================

#[test]
fn md_ordered_list() {
    let doc = parse_markdown("1. First\n2. Second\n3. Third");
    let ol = find_tag(&doc, "ol").expect("ol not found");
    let li_count = ol.children.iter().filter(|c| c.tag == "li").count();
    assert_eq!(li_count, 3);
}

#[test]
fn md_ordered_list_start_number() {
    let doc = parse_markdown("3. First\n4. Second");
    let ol = find_tag(&doc, "ol").expect("ol not found");
    assert_eq!(ol.data.get("md-start").map(|s| s.as_str()), Some("3"));
}

// ============================================================
// Thematic break
// ============================================================

#[test]
fn md_thematic_break_dash() {
    let doc = parse_markdown("---");
    assert!(find_tag(&doc, "hr").is_some(), "hr not found");
}

#[test]
fn md_thematic_break_star() {
    let doc = parse_markdown("***");
    let hr = find_tag(&doc, "hr").expect("hr not found");
    assert_eq!(hr.data.get("md-marker").map(|s| s.as_str()), Some("***"));
}

#[test]
fn md_thematic_break_underscore() {
    let doc = parse_markdown("___");
    assert!(find_tag(&doc, "hr").is_some(), "hr not found");
}

// ============================================================
// Tables
// ============================================================

#[test]
fn md_simple_table() {
    let doc = parse_markdown("| A | B |\n|---|---|\n| 1 | 2 |");
    assert!(find_tag(&doc, "table").is_some(), "table not found");
    assert!(
        find_box(&doc.root, &|b| b.tag == "th").is_some(),
        "th not found"
    );
    assert!(
        find_box(&doc.root, &|b| b.tag == "td").is_some(),
        "td not found"
    );
}

#[test]
fn md_table_alignment() {
    let doc = parse_markdown(
        "| Left | Center | Right |\n|:-----|:------:|------:|\n| a    | b      | c     |",
    );
    let table = find_tag(&doc, "table").expect("table not found");
    let align = table.data.get("md-align").expect("md-align not set");
    assert!(align.contains("center"), "md-align: {}", align);
    assert!(align.contains("right"), "md-align: {}", align);
}

// ============================================================
// Escaped characters
// ============================================================

#[test]
fn md_escaped_asterisk() {
    let doc = parse_markdown("\\*not italic\\*");
    let p = find_tag(&doc, "p").expect("p not found");
    let text = get_text(p);
    assert!(text.contains("*not italic*"), "text: {:?}", text);
    // Should NOT be italic
    assert!(!p
        .layout
        .inline_runs
        .iter()
        .any(|r| r.style.font_style == FontStyle::Italic));
}

// ============================================================
// Serialization (Box tree → Markdown)
// ============================================================

#[test]
fn md_serialize_heading() {
    let doc = parse_markdown("# Hello");
    let md = serialize_markdown(&doc);
    assert!(md.contains("# Hello"), "md: {:?}", md);
}

#[test]
fn md_serialize_paragraph() {
    let doc = parse_markdown("Simple text");
    let md = serialize_markdown(&doc);
    assert!(md.contains("Simple text"), "md: {:?}", md);
}

#[test]
fn md_serialize_code_block() {
    let doc = parse_markdown("```python\ncode\n```");
    let md = serialize_markdown(&doc);
    assert!(md.contains("```python"), "md: {:?}", md);
    assert!(md.contains("code"), "md: {:?}", md);
}

#[test]
fn md_serialize_hr() {
    let doc = parse_markdown("---");
    let md = serialize_markdown(&doc);
    assert!(md.contains("---"), "md: {:?}", md);
}

#[test]
fn md_serialize_blockquote() {
    let doc = parse_markdown("> Quote");
    let md = serialize_markdown(&doc);
    assert!(md.contains("> "), "md: {:?}", md);
    assert!(md.contains("Quote"), "md: {:?}", md);
}

#[test]
fn md_serialize_unordered_list() {
    let doc = parse_markdown("- A\n- B");
    let md = serialize_markdown(&doc);
    assert!(md.contains("- A"), "md: {:?}", md);
    assert!(md.contains("- B"), "md: {:?}", md);
}

#[test]
fn md_serialize_ordered_list() {
    let doc = parse_markdown("1. First\n2. Second");
    let md = serialize_markdown(&doc);
    assert!(md.contains("1. "), "md: {:?}", md);
    assert!(md.contains("2. "), "md: {:?}", md);
}

// ============================================================
// Round-trip tests (Markdown → Box tree → Markdown)
// ============================================================

#[test]
fn md_roundtrip_heading() {
    let input = "# Hello World";
    let doc = parse_markdown(input);
    let output = serialize_markdown(&doc);
    assert!(
        output.trim().contains("# Hello World"),
        "output: {:?}",
        output
    );
}

#[test]
fn md_roundtrip_setext_heading() {
    let input = "Title\n=====";
    let doc = parse_markdown(input);
    let output = serialize_markdown(&doc);
    assert!(output.contains("Title\n"), "output: {:?}", output);
    assert!(output.contains("====="), "output: {:?}", output);
}

#[test]
fn md_roundtrip_code_block() {
    let input = "```python\ndef hello():\n    pass\n```";
    let doc = parse_markdown(input);
    let output = serialize_markdown(&doc);
    assert!(output.contains("```python"), "output: {:?}", output);
    assert!(output.contains("def hello():"), "output: {:?}", output);
    assert!(output.contains("```"), "output: {:?}", output);
}

#[test]
fn md_roundtrip_tilde_fence() {
    let input = "~~~\ncode\n~~~";
    let doc = parse_markdown(input);
    let output = serialize_markdown(&doc);
    assert!(output.contains("~~~"), "output: {:?}", output);
    assert!(output.contains("code"), "output: {:?}", output);
}

#[test]
fn md_roundtrip_thematic_break() {
    let input = "***";
    let doc = parse_markdown(input);
    let output = serialize_markdown(&doc);
    assert!(output.contains("***"), "output: {:?}", output);
}

#[test]
fn md_roundtrip_bullet_style() {
    let input = "* A\n* B";
    let doc = parse_markdown(input);
    let output = serialize_markdown(&doc);
    assert!(output.contains("* A"), "output: {:?}", output);
    assert!(output.contains("* B"), "output: {:?}", output);
}

#[test]
fn md_roundtrip_table() {
    let input = "| A | B |\n|---|---|\n| 1 | 2 |";
    let doc = parse_markdown(input);
    let output = serialize_markdown(&doc);
    assert!(output.contains("| A"), "output: {:?}", output);
    assert!(output.contains("| 1"), "output: {:?}", output);
    assert!(output.contains("---"), "output: {:?}", output);
}

#[test]
fn md_roundtrip_bold() {
    let input = "This is **bold** text";
    let doc = parse_markdown(input);
    let output = serialize_markdown(&doc);
    assert!(output.contains("**bold**"), "output: {:?}", output);
}

#[test]
fn md_roundtrip_italic() {
    let input = "This is *italic* text";
    let doc = parse_markdown(input);
    let output = serialize_markdown(&doc);
    assert!(output.contains("*italic*"), "output: {:?}", output);
}

#[test]
fn md_roundtrip_inline_code() {
    let input = "Use `code` here";
    let doc = parse_markdown(input);
    let output = serialize_markdown(&doc);
    assert!(output.contains("`code`"), "output: {:?}", output);
}

#[test]
fn md_roundtrip_link() {
    let input = "[click](https://example.com)";
    let doc = parse_markdown(input);
    let output = serialize_markdown(&doc);
    assert!(
        output.contains("[click](https://example.com)"),
        "output: {:?}",
        output
    );
}

#[test]
fn md_roundtrip_image() {
    let input = "![alt](pic.png)";
    let doc = parse_markdown(input);
    let output = serialize_markdown(&doc);
    assert!(output.contains("![alt](pic.png)"), "output: {:?}", output);
}

// ============================================================
// Edge cases
// ============================================================

#[test]
fn md_empty_input() {
    let doc = parse_markdown("");
    assert_eq!(doc.root.tag, "body", "root tag should be body");
}

#[test]
fn md_only_whitespace() {
    let doc = parse_markdown("   \n\n   ");
    assert_eq!(doc.root.tag, "body");
}

#[test]
fn md_multiple_blank_lines() {
    let doc = parse_markdown("A\n\n\n\nB");
    let count = count_boxes(&doc.root, &|b| b.tag == "p");
    assert_eq!(count, 2, "Expected 2 paragraphs, got {}", count);
}

// ============================================================
// Indented code blocks
// ============================================================

#[test]
fn md_indented_code_block() {
    let doc = parse_markdown("    code line 1\n    code line 2");
    let pre = find_tag(&doc, "pre").expect("pre not found");
    let text = get_text(pre);
    assert!(text.contains("code line 1"), "text: {:?}", text);
    assert!(text.contains("code line 2"), "text: {:?}", text);
}

#[test]
fn md_indented_code_block_preserves_style() {
    let doc = parse_markdown("    indented code");
    let pre = find_tag(&doc, "pre").expect("pre not found");
    assert_eq!(
        pre.data.get("md-code-style").map(|s| s.as_str()),
        Some("indented")
    );
}

#[test]
fn md_indented_code_block_round_trip() {
    let input = "    line 1\n    line 2";
    let doc = parse_markdown(input);
    let output = serialize_markdown(&doc);
    assert!(output.contains("    line 1"), "output: {:?}", output);
    assert!(output.contains("    line 2"), "output: {:?}", output);
}

// ============================================================
// Task lists
// ============================================================

#[test]
fn md_task_list_unchecked() {
    let doc = parse_markdown("- [ ] Todo item");
    let ul = find_tag(&doc, "ul").expect("ul not found");
    assert!(
        ul.data.contains_key("md-task-list"),
        "md-task-list not set on ul"
    );
    let li = find_box(ul, &|b| b.tag == "li").expect("li not found");
    assert_eq!(
        li.data.get("md-task").map(|s| s.as_str()),
        Some("unchecked")
    );
}

#[test]
fn md_task_list_checked() {
    let doc = parse_markdown("- [x] Done item");
    let ul = find_tag(&doc, "ul").expect("ul not found");
    let li = find_box(ul, &|b| b.tag == "li").expect("li not found");
    assert_eq!(li.data.get("md-task").map(|s| s.as_str()), Some("checked"));
}

#[test]
fn md_task_list_mixed() {
    let doc = parse_markdown("- [x] Done\n- [ ] Todo\n- [x] Also done");
    let ul = find_tag(&doc, "ul").expect("ul not found");
    let li_count = ul.children.iter().filter(|c| c.tag == "li").count();
    assert_eq!(li_count, 3);
}

#[test]
fn md_task_list_round_trip() {
    let input = "- [ ] Todo\n- [x] Done";
    let doc = parse_markdown(input);
    let output = serialize_markdown(&doc);
    assert!(output.contains("[ ] Todo"), "output: {:?}", output);
    assert!(output.contains("[x] Done"), "output: {:?}", output);
}

#[test]
fn md_task_list_text() {
    let doc = parse_markdown("- [ ] Buy groceries");
    let li = find_box(&doc.root, &|b| b.tag == "li").expect("li not found");
    let text = get_text(li);
    assert!(text.contains("Buy groceries"), "text: {:?}", text);
}

// ============================================================
// Reference links
// ============================================================

#[test]
fn md_reference_link_full() {
    let doc = parse_markdown("[click here][example]\n\n[example]: https://example.com");
    let p = find_tag(&doc, "p").expect("p not found");
    let found = p
        .layout
        .inline_runs
        .iter()
        .any(|r| r.style.href == "https://example.com");
    assert!(found, "No run with href=https://example.com found");
}

#[test]
fn md_reference_link_collapsed() {
    let doc = parse_markdown("[example][]\n\n[example]: https://example.com");
    let p = find_tag(&doc, "p").expect("p not found");
    let found = p
        .layout
        .inline_runs
        .iter()
        .any(|r| r.style.href == "https://example.com");
    assert!(found, "No run with href=https://example.com found");
}

#[test]
fn md_reference_link_shortcut() {
    let doc = parse_markdown("[example]\n\n[example]: https://example.com");
    let p = find_tag(&doc, "p").expect("p not found");
    let found = p
        .layout
        .inline_runs
        .iter()
        .any(|r| r.style.href == "https://example.com");
    assert!(found, "No run with href=https://example.com found");
}

#[test]
fn md_reference_link_case_insensitive() {
    let doc = parse_markdown("[Click][EXAMPLE]\n\n[example]: https://example.com");
    let p = find_tag(&doc, "p").expect("p not found");
    let found = p
        .layout
        .inline_runs
        .iter()
        .any(|r| r.style.href == "https://example.com");
    assert!(found, "No run with href=https://example.com found");
}

// ============================================================
// Autolinks
// ============================================================

#[test]
fn md_autolink_url() {
    let doc = parse_markdown("<https://example.com>");
    let p = find_tag(&doc, "p").expect("p not found");
    let found = p
        .layout
        .inline_runs
        .iter()
        .any(|r| r.style.href == "https://example.com");
    assert!(found, "No run with autolink href found");
}

#[test]
fn md_autolink_email() {
    let doc = parse_markdown("<user@example.com>");
    let p = find_tag(&doc, "p").expect("p not found");
    let found = p
        .layout
        .inline_runs
        .iter()
        .any(|r| r.style.href == "mailto:user@example.com");
    assert!(found, "No run with mailto href found");
}

#[test]
fn md_autolink_round_trip() {
    let input = "<https://example.com>";
    let doc = parse_markdown(input);
    let output = serialize_markdown(&doc);
    assert!(
        output.contains("<https://example.com>"),
        "output: {:?}",
        output
    );
}

#[test]
fn md_autolink_email_round_trip() {
    let input = "<user@example.com>";
    let doc = parse_markdown(input);
    let output = serialize_markdown(&doc);
    assert!(
        output.contains("<user@example.com>"),
        "output: {:?}",
        output
    );
}

// ============================================================
// Highlight ==text==
// ============================================================

#[test]
fn md_highlight() {
    let doc = parse_markdown("This is ==highlighted== text");
    let p = find_tag(&doc, "p").expect("p not found");
    let found = p
        .layout
        .inline_runs
        .iter()
        .any(|r| r.style.background_color == Color::rgb(255, 255, 0));
    assert!(found, "No highlighted run found");
}

#[test]
fn md_highlight_round_trip() {
    let input = "This is ==highlighted== text";
    let doc = parse_markdown(input);
    let output = serialize_markdown(&doc);
    assert!(output.contains("==highlighted=="), "output: {:?}", output);
}

// ============================================================
// Footnotes
// ============================================================

#[test]
fn md_footnote_reference() {
    let doc = parse_markdown("Text with a footnote[^1].\n\n[^1]: This is the footnote.");
    let p = find_tag(&doc, "p").expect("p not found");
    assert!(
        p.data.contains_key("md-footnote-ref"),
        "md-footnote-ref not set on p"
    );
}

#[test]
fn md_footnote_definition() {
    let doc = parse_markdown("Text[^note].\n\n[^note]: The definition.");
    let fn_div = find_box(&doc.root, &|b| b.data.contains_key("md-footnotes"));
    assert!(fn_div.is_some(), "Footnote section div not found");
}

#[test]
fn md_footnote_round_trip() {
    let input = "Text[^1].\n\n[^1]: The footnote content.";
    let doc = parse_markdown(input);
    let output = serialize_markdown(&doc);
    assert!(output.contains("[^1]"), "output: {:?}", output);
    assert!(output.contains("footnote content"), "output: {:?}", output);
}

// ============================================================
// Definition lists
// ============================================================

#[test]
fn md_definition_list() {
    let doc = parse_markdown("Term\n: Definition");
    let dl = find_tag(&doc, "dl").expect("dl not found");
    let dt = find_box(dl, &|b| b.tag == "dt").expect("dt not found");
    let dd = find_box(dl, &|b| b.tag == "dd").expect("dd not found");
    assert!(get_text(dt).contains("Term"), "dt text: {:?}", get_text(dt));
    assert!(
        get_text(dd).contains("Definition"),
        "dd text: {:?}",
        get_text(dd)
    );
}

#[test]
fn md_definition_list_multiple() {
    let doc = parse_markdown("Term 1\n: Def 1\n\nTerm 2\n: Def 2");
    let dl = find_tag(&doc, "dl").expect("dl not found");
    let dt_count = dl.children.iter().filter(|c| c.tag == "dt").count();
    let dd_count = dl.children.iter().filter(|c| c.tag == "dd").count();
    assert_eq!(dt_count, 2, "Expected 2 dt, got {}", dt_count);
    assert_eq!(dd_count, 2, "Expected 2 dd, got {}", dd_count);
}

#[test]
fn md_definition_list_round_trip() {
    let input = "Term\n: Definition";
    let doc = parse_markdown(input);
    let output = serialize_markdown(&doc);
    assert!(output.contains("Term"), "output: {:?}", output);
    assert!(output.contains(": Definition"), "output: {:?}", output);
}

// ============================================================
// Raw HTML passthrough
// ============================================================

#[test]
fn md_raw_html_div() {
    let doc = parse_markdown("<div class=\"test\">\nContent\n</div>");
    // Find a box with md-raw-html
    let div = find_box(&doc.root, &|b| b.data.contains_key("md-raw-html"));
    assert!(div.is_some(), "Raw HTML box not found");
    let text = get_text(div.unwrap());
    assert!(text.contains("<div"), "text: {:?}", text);
    assert!(text.contains("Content"), "text: {:?}", text);
}

#[test]
fn md_raw_html_round_trip() {
    let input = "<div class=\"test\">\nContent\n</div>";
    let doc = parse_markdown(input);
    let output = serialize_markdown(&doc);
    assert!(
        output.contains("<div class=\"test\">"),
        "output: {:?}",
        output
    );
    assert!(output.contains("Content"), "output: {:?}", output);
}

// ============================================================
// Nested lists
// ============================================================

#[test]
fn md_nested_unordered_list() {
    let doc = parse_markdown("- Parent\n  - Child 1\n  - Child 2");
    let ul = find_tag(&doc, "ul").expect("ul not found");
    let found_nested = ul
        .children
        .iter()
        .any(|li| li.children.iter().any(|c| c.tag == "ul"));
    assert!(found_nested, "No nested ul found inside li");
}

#[test]
fn md_nested_ordered_in_unordered() {
    let doc = parse_markdown("- Item\n  1. Sub 1\n  2. Sub 2");
    let ul = find_tag(&doc, "ul").expect("ul not found");
    let found_nested = ul
        .children
        .iter()
        .any(|li| li.children.iter().any(|c| c.tag == "ol"));
    assert!(found_nested, "No nested ol found inside li");
}

// ============================================================
// HTML <pre> whitespace preservation
// These cover the fix in parse_children_into: newlines inside
// <pre> must not be collapsed to spaces.
// ============================================================

fn find_pre(doc: &Document) -> Option<&WebCore> {
    find_box(&doc.root, &|b| b.tag == "pre")
}

#[test]
fn html_pre_preserves_newlines() {
    // Newlines inside <pre> must be kept, not collapsed to spaces.
    let doc = parse_html("<pre>hello\nworld</pre>");
    let pre = find_pre(&doc).expect("pre not found");
    let text = pre.text_content();
    assert!(
        text.contains('\n'),
        "<pre> text should contain newline, got: {:?}",
        text
    );
    assert!(text.contains("hello"), "text: {:?}", text);
    assert!(text.contains("world"), "text: {:?}", text);
}

#[test]
fn html_pre_preserves_multiple_newlines() {
    let doc = parse_html("<pre>line1\nline2\nline3\n</pre>");
    let pre = find_pre(&doc).expect("pre not found");
    let text = pre.text_content();
    assert!(text.contains("line1"), "text: {:?}", text);
    assert!(text.contains("line2"), "text: {:?}", text);
    assert!(text.contains("line3"), "text: {:?}", text);
    // There should be at least 2 newlines separating the three lines.
    assert!(
        text.matches('\n').count() >= 2,
        "Expected at least 2 newlines, got: {:?}",
        text
    );
}

#[test]
fn html_pre_preserves_indentation() {
    let doc = parse_html("<pre>    indented\n    code</pre>");
    let pre = find_pre(&doc).expect("pre not found");
    let text = pre.text_content();
    assert!(
        text.contains("    indented"),
        "indentation lost: {:?}",
        text
    );
    assert!(text.contains("    code"), "indentation lost: {:?}", text);
}

#[test]
fn html_pre_strips_only_leading_newline() {
    // Per HTML spec, a single leading newline after <pre> is stripped;
    // subsequent content must be intact.
    let doc = parse_html("<pre>\nhello\nworld</pre>");
    let pre = find_pre(&doc).expect("pre not found");
    let text = pre.text_content();
    // "hello" should be present and the text should NOT start with a newline.
    assert!(text.contains("hello"), "text: {:?}", text);
    assert!(
        !text.starts_with('\n'),
        "leading newline not stripped: {:?}",
        text
    );
    // But the newline between hello and world must be kept.
    assert!(
        text.contains("hello\nworld") || (text.contains("hello") && text.contains('\n')),
        "internal newline lost: {:?}",
        text
    );
}

#[test]
fn html_p_collapses_whitespace() {
    // Outside <pre>, newlines in text must be collapsed to spaces (normal flow).
    let doc = parse_html("<p>hello\nworld</p>");
    let p = find_tag(&doc, "p").expect("p not found");
    let text = p.text_content();
    assert!(
        !text.contains('\n'),
        "newline should be collapsed in <p>, got: {:?}",
        text
    );
    assert!(text.contains("hello"), "text: {:?}", text);
    assert!(text.contains("world"), "text: {:?}", text);
}

// ============================================================
// Markdown demo source-pane scenario
// The markdown demo wraps the raw markdown in:
//   <pre contenteditable="true">ESCAPED_MARKDOWN</pre>
// and later calls doc.root.text_content() to recover the source.
// Newlines in the markdown must survive the HTML parse + text_content() round-trip.
// ============================================================

fn make_source_html(md: &str) -> String {
    let escaped = md
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    format!(
        r#"<!DOCTYPE html><html><head><style>
body {{ margin: 0; }}
pre  {{ font-family: monospace; font-size: 13px; padding: 12px;
        white-space: pre-wrap; }}
</style></head><body><pre contenteditable="true">{}</pre></body></html>"#,
        escaped
    )
}

#[test]
fn md_demo_source_pane_preserves_newlines() {
    let markdown = "# Hello\n\nThis is a paragraph.\n\n- item 1\n- item 2\n";
    let html = make_source_html(markdown);
    let doc = load_html(&html, 600.0);
    let recovered = doc.root.text_content();
    // Every line in the original markdown must appear in the recovered text.
    for line in markdown.lines() {
        assert!(
            recovered.contains(line),
            "Line {:?} missing from recovered text: {:?}",
            line,
            recovered
        );
    }
    // Newlines must be preserved — text must not be one long line.
    assert!(
        recovered.contains('\n'),
        "Newlines collapsed in source pane text_content: {:?}",
        recovered
    );
}

#[test]
fn md_demo_source_pane_code_block_preserved() {
    let markdown = "```rust\nfn main() {\n    println!(\"hi\");\n}\n```\n";
    let html = make_source_html(markdown);
    let doc = load_html(&html, 600.0);
    let recovered = doc.root.text_content();
    assert!(
        recovered.contains("fn main()"),
        "recovered: {:?}",
        recovered
    );
    assert!(recovered.contains("println!"), "recovered: {:?}", recovered);
    assert!(
        recovered.contains('\n'),
        "Newlines collapsed: {:?}",
        recovered
    );
}

#[test]
fn md_demo_preview_updates_from_source() {
    // Simulate the update_preview() call: extract text from source doc,
    // parse as markdown, lay out, verify the preview doc is valid.
    let markdown = "# Title\n\nSome text.\n";
    let html = make_source_html(markdown);
    let src_doc = load_html(&html, 495.0);
    let recovered = src_doc.root.text_content();

    // Parse recovered text as markdown for the preview pane.
    let mut prev_doc = parse_markdown(&recovered);
    LayoutEngine::new().layout(&mut prev_doc, 605.0);

    // The preview should contain the heading and paragraph.
    assert!(
        find_tag(&prev_doc, "h1").is_some(),
        "h1 missing from preview"
    );
    assert!(find_tag(&prev_doc, "p").is_some(), "p missing from preview");
    let h1 = find_tag(&prev_doc, "h1").unwrap();
    assert!(
        get_text(h1).contains("Title"),
        "h1 text: {:?}",
        get_text(h1)
    );
}

// ============================================================
// Heading em-based font sizes (make_box fix)
// Verify that the markdown parser uses em-based sizes matching
// the UA stylesheet so md output looks identical to HTML output.
// ============================================================

#[test]
fn md_heading_font_sizes_are_em_based() {
    // h1 should be CssLength::Em(2.0) — check via LayoutEngine resolution.
    // At default 16px root, h1 resolves to 32px.
    let doc = parse_markdown("# Heading");
    let h1 = find_tag(&doc, "h1").expect("h1 not found");
    // The style before layout uses Em; after layout the computed px value is set.
    // We check the pre-layout style directly.
    assert!(
        matches!(h1.style.font_size, CssLength::Em(_)),
        "h1 font_size should be Em, got: {:?}",
        h1.style.font_size
    );
    if let CssLength::Em(v) = h1.style.font_size {
        assert!(
            (v - 2.0).abs() < 0.01,
            "h1 Em factor should be 2.0, got {}",
            v
        );
    }
}

#[test]
fn md_h5_h6_different_sizes() {
    // Before the fix, h5 and h6 were both 14px. Now they must differ.
    let doc5 = parse_markdown("##### h5");
    let doc6 = parse_markdown("###### h6");
    let h5 = find_tag(&doc5, "h5").expect("h5 not found");
    let h6 = find_tag(&doc6, "h6").expect("h6 not found");
    // Both must use Em.
    assert!(matches!(h5.style.font_size, CssLength::Em(_)), "h5 not Em");
    assert!(matches!(h6.style.font_size, CssLength::Em(_)), "h6 not Em");
    // h5 (0.83em) must be larger than h6 (0.67em).
    if let (CssLength::Em(v5), CssLength::Em(v6)) = (&h5.style.font_size, &h6.style.font_size) {
        assert!(v5 > v6, "h5 ({}) should be larger than h6 ({})", v5, v6);
    }
}

#[test]
fn md_ul_padding_matches_ua() {
    // UA stylesheet gives ul/ol padding-left: 40px.
    let doc = parse_markdown("- item");
    let ul = find_tag(&doc, "ul").expect("ul not found");
    assert!(
        matches!(ul.style.padding_left, CssLength::Px(px) if (px - 40.0).abs() < 0.1),
        "ul padding_left should be 40px, got: {:?}",
        ul.style.padding_left
    );
}

// ============================================================
// Layout geometry tests — verify blocks are laid out as blocks
// (not inline). These tests catch the cascade-wipe bug where
// parse_markdown without a UA stylesheet causes layout() to
// strip all display:block styles, collapsing everything to y=0.
// ============================================================

fn parse_and_layout(md: &str) -> Document {
    let mut doc = parse_markdown(md);
    LayoutEngine::new().layout(&mut doc, 800.0);
    doc
}

#[test]
fn md_layout_blocks_have_nonzero_height() {
    let doc = parse_and_layout("# Heading\n\nParagraph text here.");
    let h1 = find_tag(&doc, "h1").expect("h1 not found");
    let p = find_tag(&doc, "p").expect("p not found");
    assert!(
        h1.layout.margin_rect.h > 0.0,
        "h1 should have nonzero height, got {}",
        h1.layout.margin_rect.h
    );
    assert!(
        p.layout.margin_rect.h > 0.0,
        "p should have nonzero height, got {}",
        p.layout.margin_rect.h
    );
}

#[test]
fn md_layout_blocks_stacked_vertically() {
    // If cascade wipes display:block, everything lands at y=0.
    // Use padding_rect (the visual box) because adjacent margin_rects collapse and overlap.
    let doc = parse_and_layout("# Heading\n\nParagraph text here.");
    let h1 = find_tag(&doc, "h1").expect("h1 not found");
    let p = find_tag(&doc, "p").expect("p not found");
    let h1_bottom = h1.layout.padding_rect.y + h1.layout.padding_rect.h;
    assert!(
        p.layout.padding_rect.y >= h1_bottom,
        "paragraph padding (y={}) should be below heading padding (bottom={})",
        p.layout.padding_rect.y,
        h1_bottom
    );
}

#[test]
fn md_layout_heading_display_is_block() {
    // After layout (which runs the cascade), h1 must still have display:block.
    let doc = parse_and_layout("# Heading");
    let h1 = find_tag(&doc, "h1").expect("h1 not found");
    assert_eq!(
        h1.style.display,
        Display::Block,
        "h1 display should be Block after layout"
    );
}

#[test]
fn md_layout_heading_font_px_larger_than_body() {
    // After layout, h1 font size resolves to 32px at 16px root.
    let doc = parse_and_layout("# Heading\n\nParagraph.");
    let h1 = find_tag(&doc, "h1").expect("h1 not found");
    let p = find_tag(&doc, "p").expect("p not found");
    let h1_px = h1.style.font_size_px(16.0, 16.0);
    let p_px = p.style.font_size_px(16.0, 16.0);
    assert!(
        h1_px > p_px,
        "h1 font ({} px) should be larger than p font ({} px)",
        h1_px,
        p_px
    );
    assert!(
        (h1_px - 32.0).abs() < 0.5,
        "h1 should be ~32px, got {}",
        h1_px
    );
}

#[test]
fn md_layout_list_items_stacked() {
    let doc = parse_and_layout("- Alpha\n- Beta\n- Gamma");
    let ul = find_tag(&doc, "ul").expect("ul not found");
    let items: Vec<&WebCore> = ul.children.iter().filter(|c| c.tag == "li").collect();
    assert_eq!(items.len(), 3, "should have 3 li items");
    for i in 1..items.len() {
        let prev_bottom = items[i - 1].layout.margin_rect.y + items[i - 1].layout.margin_rect.h;
        assert!(
            items[i].layout.margin_rect.y >= prev_bottom,
            "li[{}] (y={}) should be below li[{}] (bottom={})",
            i,
            items[i].layout.margin_rect.y,
            i - 1,
            prev_bottom
        );
    }
}

#[test]
fn md_layout_ul_display_is_block() {
    let doc = parse_and_layout("- item");
    let ul = find_tag(&doc, "ul").expect("ul not found");
    assert_eq!(
        ul.style.display,
        Display::Block,
        "ul display should be Block after layout"
    );
}

#[test]
fn md_layout_pre_display_is_block() {
    let doc = parse_and_layout("```\ncode\n```");
    let pre = find_tag(&doc, "pre").expect("pre not found");
    assert_eq!(
        pre.style.display,
        Display::Block,
        "pre display should be Block after layout"
    );
}

#[test]
fn md_layout_blockquote_display_is_block() {
    let doc = parse_and_layout("> quote");
    let bq = find_tag(&doc, "blockquote").expect("blockquote not found");
    assert_eq!(
        bq.style.display,
        Display::Block,
        "blockquote display should be Block after layout"
    );
}

#[test]
fn md_layout_multiple_headings_stacked() {
    // Use padding_rect: adjacent margin_rects collapse so their y-ranges overlap.
    let doc = parse_and_layout("# H1\n\n## H2\n\n### H3");
    let h1 = find_tag(&doc, "h1").expect("h1 not found");
    let h2 = find_tag(&doc, "h2").expect("h2 not found");
    let h3 = find_tag(&doc, "h3").expect("h3 not found");
    assert!(
        h2.layout.padding_rect.y >= h1.layout.padding_rect.y + h1.layout.padding_rect.h,
        "h2 (y={}) should be below h1 (bottom={})",
        h2.layout.padding_rect.y,
        h1.layout.padding_rect.y + h1.layout.padding_rect.h
    );
    assert!(
        h3.layout.padding_rect.y >= h2.layout.padding_rect.y + h2.layout.padding_rect.h,
        "h3 (y={}) should be below h2 (bottom={})",
        h3.layout.padding_rect.y,
        h2.layout.padding_rect.y + h2.layout.padding_rect.h
    );
}

#[test]
fn md_layout_table_display_is_table() {
    let doc = parse_and_layout("| A | B |\n|---|---|\n| 1 | 2 |");
    let table = find_tag(&doc, "table").expect("table not found");
    assert_eq!(
        table.style.display,
        Display::Table,
        "table display should be Table after layout"
    );
}

#[test]
fn md_roundtrip_complex_document() {
    // A complex markdown document should survive a full roundtrip without losing structure.
    let md = concat!(
        "# Title\n\n",
        "A paragraph with **bold** and *italic* text.\n\n",
        "## Section\n\n",
        "- First item\n",
        "- Second item\n",
        "- Third item\n\n",
        "```rust\nfn main() {}\n```\n\n",
        "> A blockquote\n\n",
        "| Col A | Col B |\n",
        "|-------|-------|\n",
        "| a     | b     |\n",
    );
    let doc1 = parse_markdown(md);
    let md2 = serialize_markdown(&doc1);
    let doc2 = parse_markdown(&md2);

    // Structure must survive roundtrip
    assert!(find_tag(&doc2, "h1").is_some(), "h1 lost in roundtrip");
    assert!(find_tag(&doc2, "h2").is_some(), "h2 lost in roundtrip");
    assert!(find_tag(&doc2, "ul").is_some(), "ul lost in roundtrip");
    assert!(find_tag(&doc2, "pre").is_some(), "pre lost in roundtrip");
    assert!(
        find_tag(&doc2, "blockquote").is_some(),
        "blockquote lost in roundtrip"
    );
    assert!(
        find_tag(&doc2, "table").is_some(),
        "table lost in roundtrip"
    );
    // Text content preserved
    assert_eq!(get_text(find_tag(&doc2, "h1").unwrap()), "Title");
}

#[test]
fn md_roundtrip_inline_formatting() {
    let md = "Text with **bold**, *italic*, ~~strike~~, and `code` inline.";
    let doc = parse_markdown(md);
    let md2 = serialize_markdown(&doc);
    let doc2 = parse_markdown(&md2);
    let p = find_tag(&doc2, "p").expect("p not found in roundtrip");
    // Bold run present
    let has_bold = p
        .layout
        .inline_runs
        .iter()
        .any(|r| r.style.font_weight == FontWeight::Bold);
    assert!(has_bold, "bold lost in roundtrip; serialized: {}", md2);
    // Italic run present
    let has_italic = p
        .layout
        .inline_runs
        .iter()
        .any(|r| r.style.font_style == FontStyle::Italic);
    assert!(has_italic, "italic lost in roundtrip; serialized: {}", md2);
    // Strikethrough run present
    let has_strike = p
        .layout
        .inline_runs
        .iter()
        .any(|r| r.style.text_decoration.strikethrough);
    assert!(
        has_strike,
        "strikethrough lost in roundtrip; serialized: {}",
        md2
    );
    // Code run present (monospace)
    let has_code = p
        .layout
        .inline_runs
        .iter()
        .any(|r| r.style.font_family == "monospace");
    assert!(has_code, "code lost in roundtrip; serialized: {}", md2);
}

#[test]
fn md_roundtrip_nested_list() {
    let md = "- Parent\n  - Child 1\n  - Child 2\n- Another parent\n";
    let doc = parse_markdown(md);
    let md2 = serialize_markdown(&doc);
    let doc2 = parse_markdown(&md2);
    // Should have an outer ul
    let ul = find_tag(&doc2, "ul").expect("ul lost in nested list roundtrip");
    // Should have at least 2 li children at top level
    let top_items: Vec<_> = ul.children.iter().filter(|c| c.tag == "li").collect();
    assert!(
        top_items.len() >= 2,
        "should have at least 2 top-level li items; got {}",
        top_items.len()
    );
}

#[test]
fn md_layout_demo_sample() {
    // Smoke test: the full SAMPLE_MD from the demo must parse and layout without panic,
    // and produce distinct vertical positions for major blocks.
    let sample = concat!(
        "# Markdown Editor\n\n",
        "This is a **live preview** of your Markdown content.\n\n",
        "## Features\n\n",
        "- **Bold**, *italic*, and ~~strikethrough~~\n",
        "- `Inline code` and code blocks\n\n",
        "## Code Block\n\n",
        "```cpp\nint main() {}\n```\n\n",
        "## Table\n\n",
        "| Feature | Status |\n",
        "|---------|--------|\n",
        "| Parsing | Done   |\n\n",
        "> The best way to predict the future\n\n",
        "---\n\n",
        "### Ordered List\n\n",
        "1. First item\n",
        "2. Second item\n",
    );
    let doc = parse_and_layout(sample);
    let h1 = find_tag(&doc, "h1").expect("h1 not found");
    let h2 = find_tag(&doc, "h2").expect("h2 not found");
    // h1 and h2 must be at different vertical positions
    assert!(h1.layout.margin_rect.h > 0.0, "h1 height is 0");
    assert!(
        h2.layout.margin_rect.y > h1.layout.margin_rect.y,
        "h2 not below h1"
    );
    // Table must have block geometry
    let table = find_tag(&doc, "table").expect("table not found");
    assert!(table.layout.margin_rect.h > 0.0, "table height is 0");
}

// ============================================================
// Inline formatting survives layout (the inline_runs bug)
// Before the fix, layout_inline_block collapsed all inline runs
// into a single #text with the block's base style, silently
// dropping bold, italic, link color, strikethrough, code font.
// ============================================================

fn parse_layout_find<'a>(doc: &'a Document, tag: &str) -> &'a WebCore {
    find_tag(doc, tag).unwrap_or_else(|| panic!("{} not found", tag))
}

#[test]
fn md_layout_preserves_bold() {
    let doc = parse_and_layout("A **bold** word.");
    let p = parse_layout_find(&doc, "p");
    let bold_run = p
        .layout
        .inline_runs
        .iter()
        .find(|r| r.style.font_weight == FontWeight::Bold);
    assert!(
        bold_run.is_some(),
        "bold run missing after layout; runs: {:?}",
        p.layout
            .inline_runs
            .iter()
            .map(|r| (
                &p.text[r.text_offset..r.text_offset + r.length],
                r.style.font_weight
            ))
            .collect::<Vec<_>>()
    );
}

#[test]
fn md_layout_preserves_italic() {
    let doc = parse_and_layout("A *italic* word.");
    let p = parse_layout_find(&doc, "p");
    let run = p
        .layout
        .inline_runs
        .iter()
        .find(|r| r.style.font_style == FontStyle::Italic);
    assert!(run.is_some(), "italic run missing after layout");
}

#[test]
fn md_layout_preserves_strikethrough() {
    let doc = parse_and_layout("A ~~struck~~ word.");
    let p = parse_layout_find(&doc, "p");
    let run = p
        .layout
        .inline_runs
        .iter()
        .find(|r| r.style.text_decoration.strikethrough);
    assert!(run.is_some(), "strikethrough run missing after layout");
}

#[test]
fn md_layout_preserves_code_font() {
    let doc = parse_and_layout("Use `code` here.");
    let p = parse_layout_find(&doc, "p");
    let run = p
        .layout
        .inline_runs
        .iter()
        .find(|r| r.style.font_family == "monospace");
    assert!(run.is_some(), "code (monospace) run missing after layout");
}

#[test]
fn md_layout_preserves_link_color() {
    let doc = parse_and_layout("See [link](https://example.com) here.");
    let p = parse_layout_find(&doc, "p");
    let run = p
        .layout
        .inline_runs
        .iter()
        .find(|r| !r.style.href.is_empty());
    assert!(run.is_some(), "link run (href) missing after layout");
    let run = run.unwrap();
    assert_eq!(run.style.href, "https://example.com");
    assert!(
        run.style.text_decoration.underline,
        "link should be underlined"
    );
}

#[test]
fn md_layout_heading_preserves_bold_italic() {
    // Heading text is itself bold, but also supports nested italic
    let doc = parse_and_layout("# Heading with *italic* word");
    let h1 = parse_layout_find(&doc, "h1");
    let italic_run = h1
        .layout
        .inline_runs
        .iter()
        .find(|r| r.style.font_style == FontStyle::Italic);
    assert!(
        italic_run.is_some(),
        "italic inside h1 missing after layout; runs: {:?}",
        h1.layout
            .inline_runs
            .iter()
            .map(|r| (
                &h1.text[r.text_offset..r.text_offset + r.length],
                r.style.font_style
            ))
            .collect::<Vec<_>>()
    );
}
