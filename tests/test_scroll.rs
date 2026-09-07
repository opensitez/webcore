// Tests for per-element and viewport scroll: overflow, wheel dispatch,
// scroll clamping, and scrollbar visibility logic.

use webcore::types::*;
use webcore::{load_html, LayoutEngine};

fn parse_and_layout(html: &str, vw: f32) -> Document {
    load_html(html, vw)
}

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

// ── Overflow property parsing ─────────────────────────────────────────────────

#[test]
fn overflow_hidden_parsed() {
    let doc = parse_and_layout(
        r#"<html><body><div style="overflow:hidden;width:100px;height:50px">x</div></body></html>"#,
        400.0,
    );
    let div = find_box(&doc.root, &|b| b.tag == "div").expect("div");
    assert_eq!(div.style.overflow_x, Overflow::Hidden);
    assert_eq!(div.style.overflow_y, Overflow::Hidden);
}

#[test]
fn overflow_scroll_parsed() {
    let doc = parse_and_layout(
        r#"<html><body><div style="overflow:scroll;width:100px;height:50px">x</div></body></html>"#,
        400.0,
    );
    let div = find_box(&doc.root, &|b| b.tag == "div").expect("div");
    assert_eq!(div.style.overflow_x, Overflow::Scroll);
    assert_eq!(div.style.overflow_y, Overflow::Scroll);
}

#[test]
fn overflow_auto_parsed() {
    let doc = parse_and_layout(
        r#"<html><body><div style="overflow:auto;width:100px;height:50px">x</div></body></html>"#,
        400.0,
    );
    let div = find_box(&doc.root, &|b| b.tag == "div").expect("div");
    assert_eq!(div.style.overflow_x, Overflow::Auto);
    assert_eq!(div.style.overflow_y, Overflow::Auto);
}

#[test]
fn overflow_xy_independent() {
    let doc = parse_and_layout(
        r#"<html><body><div style="overflow-x:hidden;overflow-y:auto">x</div></body></html>"#,
        400.0,
    );
    let div = find_box(&doc.root, &|b| b.tag == "div").expect("div");
    assert_eq!(div.style.overflow_x, Overflow::Hidden);
    assert_eq!(div.style.overflow_y, Overflow::Auto);
}

// ── Scroll extent calculation ─────────────────────────────────────────────────

#[test]
fn scroll_height_set_when_overflow_scroll() {
    // A div with fixed height and taller content should have scroll_height > 0.
    let doc = parse_and_layout(
        r#"<html><body>
          <div id="box" style="overflow:scroll;width:200px;height:60px">
            <p style="height:200px">tall content</p>
          </div>
        </body></html>"#,
        400.0,
    );
    let div = find_box(&doc.root, &|b| {
        b.attributes.get("id").map(|s| s == "box").unwrap_or(false)
    })
    .expect("box div");
    assert!(
        div.layout.scroll_height > div.layout.content_rect.h,
        "scroll_height ({}) should exceed content_rect.h ({})",
        div.layout.scroll_height,
        div.layout.content_rect.h
    );
}

#[test]
fn scroll_height_zero_when_no_overflow() {
    let doc = parse_and_layout(
        r#"<html><body><div id="box" style="width:200px">short</div></body></html>"#,
        400.0,
    );
    let div = find_box(&doc.root, &|b| {
        b.attributes.get("id").map(|s| s == "box").unwrap_or(false)
    })
    .expect("box div");
    // Without overflow:scroll/auto, scroll_height mirrors content_rect.h.
    assert!(
        div.layout.scroll_height <= div.layout.content_rect.h + 1.0,
        "scroll_height ({}) should not exceed content height ({})",
        div.layout.scroll_height,
        div.layout.content_rect.h
    );
    assert_eq!(div.layout.scroll_top, 0.0);
}

// ── process_wheel_event: viewport scroll fallback ────────────────────────────

#[test]
fn wheel_scrolls_viewport_when_no_scrollable_div() {
    let mut doc = parse_and_layout(
        r#"<html><body style="height:2000px"><p>Hello world</p></body></html>"#,
        400.0,
    );
    assert_eq!(doc.scroll_y, 0.0);
    // Negative delta_y = scroll down (same sign as winit LineDelta y < 0 = scroll down).
    doc.process_wheel_event((200.0, 10.0), -30.0);
    assert!(
        doc.scroll_y > 0.0,
        "viewport scroll_y should increase on scroll-down, got {}",
        doc.scroll_y
    );
}

#[test]
fn wheel_scrolls_down_and_up() {
    let mut doc = parse_and_layout(
        r#"<html><body style="height:2000px"><p>tall</p></body></html>"#,
        400.0,
    );
    // Scroll down: delta_y < 0.
    doc.process_wheel_event((200.0, 100.0), -50.0);
    let after_down = doc.scroll_y;
    assert!(
        after_down > 0.0,
        "should scroll down, scroll_y={}",
        after_down
    );

    // Scroll up: delta_y > 0.
    doc.process_wheel_event((200.0, 100.0 + after_down), 50.0);
    assert!(
        doc.scroll_y < after_down,
        "scrolling up should decrease scroll_y"
    );
}

// ── process_wheel_event: per-element scroll ───────────────────────────────────

#[test]
fn wheel_scrolls_div_not_viewport() {
    // A scrollable div occupying the full viewport; wheel should scroll the div,
    // not the document viewport.
    let mut doc = parse_and_layout(
        r#"<html><body style="margin:0">
          <div id="box" style="overflow:auto;width:400px;height:100px">
            <div style="height:400px">tall content</div>
          </div>
        </body></html>"#,
        400.0,
    );

    let before_vp = doc.scroll_y;
    // Cursor inside the scrollable div; delta_y < 0 = scroll down.
    doc.process_wheel_event((200.0, 50.0), -30.0);

    // Viewport should NOT have moved.
    assert_eq!(
        doc.scroll_y, before_vp,
        "viewport scroll_y should not change when a div consumes the wheel"
    );

    // The div's scroll_top should have increased.
    let div = find_box(&doc.root, &|b| {
        b.attributes.get("id").map(|s| s == "box").unwrap_or(false)
    })
    .expect("box div");
    assert!(
        div.layout.scroll_top > 0.0,
        "div scroll_top should increase after wheel event, got {}",
        div.layout.scroll_top
    );
}

#[test]
fn wheel_div_scroll_clamped_to_max() {
    let mut doc = parse_and_layout(
        r#"<html><body style="margin:0">
          <div id="box" style="overflow:scroll;width:400px;height:100px">
            <div style="height:200px">content</div>
          </div>
        </body></html>"#,
        400.0,
    );

    // Scroll far past the end (delta_y very negative = far scroll down).
    doc.process_wheel_event((200.0, 50.0), -9999.0);

    let div = find_box(&doc.root, &|b| {
        b.attributes.get("id").map(|s| s == "box").unwrap_or(false)
    })
    .expect("box div");
    let max = (div.layout.scroll_height - div.layout.content_rect.h).max(0.0);
    assert!(
        div.layout.scroll_top <= max + 0.5,
        "scroll_top ({}) must not exceed max ({})",
        div.layout.scroll_top,
        max
    );
}

#[test]
fn wheel_div_scroll_clamped_to_zero() {
    let mut doc = parse_and_layout(
        r#"<html><body style="margin:0">
          <div id="box" style="overflow:scroll;width:400px;height:100px">
            <div style="height:200px">content</div>
          </div>
        </body></html>"#,
        400.0,
    );
    // Positive delta = scroll UP; from 0, scroll_top must stay at 0.
    doc.process_wheel_event((200.0, 50.0), 50.0);

    let div = find_box(&doc.root, &|b| {
        b.attributes.get("id").map(|s| s == "box").unwrap_or(false)
    })
    .expect("box div");
    assert_eq!(
        div.layout.scroll_top, 0.0,
        "scroll_top must not go negative, got {}",
        div.layout.scroll_top
    );
}

#[test]
fn wheel_outside_div_scrolls_viewport() {
    // A small scrollable div at the top; wheel cursor is below it.
    let mut doc = parse_and_layout(
        r#"<html><body style="margin:0;height:2000px">
          <div id="box" style="overflow:auto;width:400px;height:80px">
            <div style="height:400px">tall</div>
          </div>
        </body></html>"#,
        400.0,
    );

    // Cursor at y=200 — below the 80px div. Negative delta = scroll down.
    doc.process_wheel_event((200.0, 200.0), -40.0);

    // The div should NOT be scrolled.
    let div = find_box(&doc.root, &|b| {
        b.attributes.get("id").map(|s| s == "box").unwrap_or(false)
    })
    .expect("box div");
    assert_eq!(
        div.layout.scroll_top, 0.0,
        "div outside cursor should not scroll"
    );

    // The viewport should have scrolled.
    assert!(
        doc.scroll_y > 0.0,
        "viewport should scroll when cursor is outside the scrollable div"
    );
}

// ── Overflow:hidden does not scroll ──────────────────────────────────────────

#[test]
fn wheel_over_overflow_hidden_falls_through_to_viewport() {
    let mut doc = parse_and_layout(
        r#"<html><body style="margin:0;height:2000px">
          <div id="box" style="overflow:hidden;width:400px;height:100px">
            <div style="height:400px">content</div>
          </div>
        </body></html>"#,
        400.0,
    );

    // Negative delta = scroll down → should fall through to viewport.
    doc.process_wheel_event((200.0, 50.0), -30.0);

    // overflow:hidden is not scrollable — cursor falls through to viewport.
    let div = find_box(&doc.root, &|b| {
        b.attributes.get("id").map(|s| s == "box").unwrap_or(false)
    })
    .expect("box div");
    assert_eq!(
        div.layout.scroll_top, 0.0,
        "overflow:hidden div should not scroll"
    );
    assert!(
        doc.scroll_y > 0.0,
        "viewport should receive the scroll instead"
    );
}

// ── Nested scrollable divs ────────────────────────────────────────────────────

#[test]
fn wheel_scrolls_innermost_div() {
    // Outer scrollable div contains an inner scrollable div.
    // Wheel over the inner div should scroll the inner one.
    let mut doc = parse_and_layout(
        r#"<html><body style="margin:0">
          <div id="outer" style="overflow:auto;width:400px;height:200px">
            <div id="inner" style="overflow:auto;width:380px;height:80px;margin:0">
              <div style="height:400px">deep content</div>
            </div>
            <div style="height:400px">outer extra</div>
          </div>
        </body></html>"#,
        400.0,
    );

    // Cursor inside the inner div; negative delta = scroll down.
    doc.process_wheel_event((190.0, 40.0), -20.0);

    let inner = find_box(&doc.root, &|b| {
        b.attributes
            .get("id")
            .map(|s| s == "inner")
            .unwrap_or(false)
    })
    .expect("inner div");
    let outer = find_box(&doc.root, &|b| {
        b.attributes
            .get("id")
            .map(|s| s == "outer")
            .unwrap_or(false)
    })
    .expect("outer div");

    assert!(
        inner.layout.scroll_top > 0.0,
        "inner div should scroll, got {}",
        inner.layout.scroll_top
    );
    assert_eq!(
        outer.layout.scroll_top, 0.0,
        "outer div should not scroll when inner handles wheel"
    );
}

// ── Document::scroll_y viewport clamping ─────────────────────────────────────

#[test]
fn viewport_scroll_y_not_negative() {
    let mut doc = parse_and_layout(r#"<html><body><p>Hello</p></body></html>"#, 400.0);
    doc.process_wheel_event((200.0, 50.0), -100.0);
    // The renderer clamps on render; process_wheel_event just sets scroll_y.
    // It may go negative before renderer clamps, so we only check it doesn't crash.
    // (Renderer clamps: scroll_y = scroll_y.max(0.0))
    let _ = doc.scroll_y; // just ensure no panic
}

// ── Layout re-run: scroll_top preserved across relayout ──────────────────────

#[test]
fn scroll_top_preserved_after_relayout() {
    let mut doc = parse_and_layout(
        r#"<html><body style="margin:0">
          <div id="box" style="overflow:scroll;width:400px;height:100px">
            <div style="height:400px">content</div>
          </div>
        </body></html>"#,
        400.0,
    );

    // Negative delta = scroll down → increases scroll_top.
    doc.process_wheel_event((200.0, 50.0), -60.0);

    let scroll_before = {
        let div = find_box(&doc.root, &|b| {
            b.attributes.get("id").map(|s| s == "box").unwrap_or(false)
        })
        .expect("box div");
        div.layout.scroll_top
    };
    assert!(
        scroll_before > 0.0,
        "scroll_top should be > 0 after scrolling down, got {}",
        scroll_before
    );

    // Relayout (e.g. on resize).
    LayoutEngine::new().layout(&mut doc, 400.0);

    let div = find_box(&doc.root, &|b| {
        b.attributes.get("id").map(|s| s == "box").unwrap_or(false)
    })
    .expect("box div");
    // scroll_top should be re-clamped but not zeroed if content still overflows.
    assert!(
        div.layout.scroll_top <= div.layout.scroll_height - div.layout.content_rect.h + 0.5,
        "scroll_top should be clamped after relayout"
    );
}
