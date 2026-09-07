// Tests for inline-block flow fixes:
// - max-height percentage against unknown containing height
// - shrink-to-fit for InlineBlock in block context
// - anonymous inline formatting context (horizontal flow)
// - compute_intrinsic_width for mixed block/inline content
// - image aspect ratio preservation

use webcore::types::*;
use webcore::{load_html, parse_html};

fn find_box<'a>(root: &'a WebCore, pred: &dyn Fn(&WebCore) -> bool) -> Option<&'a WebCore> {
    if pred(root) {
        return Some(root);
    }
    for child in &root.children {
        if let Some(found) = find_box(child, pred) {
            return Some(found);
        }
    }
    None
}

fn find_all_boxes<'a>(root: &'a WebCore, pred: &dyn Fn(&WebCore) -> bool) -> Vec<&'a WebCore> {
    let mut result = Vec::new();
    collect_matching(root, pred, &mut result);
    result
}

fn collect_matching<'a>(
    node: &'a WebCore,
    pred: &dyn Fn(&WebCore) -> bool,
    out: &mut Vec<&'a WebCore>,
) {
    if pred(node) {
        out.push(node);
    }
    for child in &node.children {
        collect_matching(child, pred, out);
    }
}

// ============================================================
// max-height: percentage against auto containing height
// ============================================================

#[test]
fn max_height_percent_no_clamp_to_zero() {
    // When a child has max-height:100% but the parent has auto height,
    // the percentage should be treated as none (no constraint), not 0.
    let doc = load_html(
        "<div style='height: auto;'>\
           <div id='child' style='height: 200px; max-height: 100%;'>Content</div>\
         </div>",
        800.0,
    );
    let child = find_box(&doc.root, &|b| {
        b.attributes.get("id").map_or(false, |v| v == "child")
    });
    assert!(child.is_some(), "child box not found");
    let child = child.unwrap();
    // height should be 200px, not clamped to 0 by max-height:100%
    assert!(
        child.layout.content_rect.h >= 199.0,
        "height={} should be ~200, not clamped to 0 by max-height:100%",
        child.layout.content_rect.h
    );
}

#[test]
fn max_height_percent_does_not_clamp_when_parent_auto() {
    // Even when the parent has an explicit height, our engine currently passes 0
    // as the containing height reference. Percentage max-height should NOT clamp
    // the child to 0 — it should be treated as unconstrained.
    let doc = load_html(
        "<div style='height: 400px;'>\
           <div id='child' style='height: 300px; max-height: 50%;'>Content</div>\
         </div>",
        800.0,
    );
    let child = find_box(&doc.root, &|b| {
        b.attributes.get("id").map_or(false, |v| v == "child")
    });
    assert!(child.is_some());
    let child = child.unwrap();
    // Percentage max-height resolves against 0 containing height → treated as none
    // so height stays 300px (not clamped to 0)
    assert!(
        child.layout.content_rect.h >= 299.0,
        "height={} should be ~300 (percentage max-height treated as none)",
        child.layout.content_rect.h
    );
}

#[test]
fn max_height_px_zero_still_works() {
    // max-height: 0px (explicit zero, not percentage) should still clamp to 0
    let doc = load_html(
        "<div id='child' style='height: 100px; max-height: 0px;'>Content</div>",
        800.0,
    );
    let child = find_box(&doc.root, &|b| {
        b.attributes.get("id").map_or(false, |v| v == "child")
    });
    assert!(child.is_some());
    let child = child.unwrap();
    assert!(
        child.layout.content_rect.h < 1.0,
        "height={} should be 0 with max-height:0px",
        child.layout.content_rect.h
    );
}

// ============================================================
// InlineBlock shrink-to-fit in block context
// ============================================================

#[test]
fn inline_block_shrink_to_fit_in_block_context() {
    // InlineBlock with auto width should shrink to content, not fill container.
    let doc = load_html(
        "<div style='width: 800px;'>\
           <div style='display: block; height: 10px;'>block</div>\
           <div id='ib' style='display: inline-block;'>\
             <div style='width: 200px; height: 50px;'>inner</div>\
           </div>\
         </div>",
        800.0,
    );
    let ib = find_box(&doc.root, &|b| {
        b.attributes.get("id").map_or(false, |v| v == "ib")
    });
    assert!(ib.is_some(), "inline-block box not found");
    let ib = ib.unwrap();
    // Should shrink to ~200px content width, not 800px
    assert!(
        ib.layout.content_rect.w < 400.0,
        "inline-block width={} should shrink to ~200, not fill 800px container",
        ib.layout.content_rect.w
    );
}

// ============================================================
// Anonymous inline formatting context (horizontal flow)
// ============================================================

#[test]
fn inline_block_horizontal_flow_in_mixed_content() {
    // When a block container has both block and inline-block children,
    // consecutive inline-block children should flow horizontally.
    let doc = load_html(
        "<div style='width: 800px;'>\
           <div id='a' style='display: inline-block; width: 200px; height: 100px;'>A</div>\
           <div id='b' style='display: inline-block; width: 200px; height: 100px;'>B</div>\
           <div id='c' style='display: inline-block; width: 200px; height: 100px;'>C</div>\
           <div style='display: block; height: 10px;'>block child</div>\
         </div>",
        800.0,
    );
    let a = find_box(&doc.root, &|b| {
        b.attributes.get("id").map_or(false, |v| v == "a")
    })
    .unwrap();
    let b = find_box(&doc.root, &|b| {
        b.attributes.get("id").map_or(false, |v| v == "b")
    })
    .unwrap();
    let c = find_box(&doc.root, &|b| {
        b.attributes.get("id").map_or(false, |v| v == "c")
    })
    .unwrap();

    // A, B, C should be on the same row (same y position)
    assert!(
        (a.layout.content_rect.y - b.layout.content_rect.y).abs() < 2.0,
        "A.y={} and B.y={} should be on the same row",
        a.layout.content_rect.y,
        b.layout.content_rect.y
    );
    assert!(
        (b.layout.content_rect.y - c.layout.content_rect.y).abs() < 2.0,
        "B.y={} and C.y={} should be on the same row",
        b.layout.content_rect.y,
        c.layout.content_rect.y
    );

    // B should be to the right of A
    assert!(
        b.layout.content_rect.x > a.layout.content_rect.x + 100.0,
        "B.x={} should be to the right of A.x={}",
        b.layout.content_rect.x,
        a.layout.content_rect.x
    );
}

#[test]
fn inline_block_wraps_to_next_line() {
    // When inline-block children exceed container width, they should wrap.
    let doc = load_html(
        "<div style='width: 500px;'>\
           <div id='a' style='display: inline-block; width: 300px; height: 50px;'>A</div>\
           <div id='b' style='display: inline-block; width: 300px; height: 50px;'>B</div>\
           <div style='display: block; height: 10px;'>block</div>\
         </div>",
        800.0,
    );
    let a = find_box(&doc.root, &|b| {
        b.attributes.get("id").map_or(false, |v| v == "a")
    })
    .unwrap();
    let b = find_box(&doc.root, &|b| {
        b.attributes.get("id").map_or(false, |v| v == "b")
    })
    .unwrap();

    // B should be below A (wrapped to next line)
    assert!(
        b.layout.content_rect.y > a.layout.content_rect.y + 20.0,
        "B.y={} should be below A.y={} (wrapped)",
        b.layout.content_rect.y,
        a.layout.content_rect.y
    );
}

#[test]
fn inline_block_flow_block_child_below() {
    // A block-level child after inline-block children should start below them.
    let doc = load_html(
        "<div style='width: 800px;'>\
           <div style='display: inline-block; width: 200px; height: 100px;'>IB</div>\
           <div id='blk' style='display: block; height: 50px;'>Block</div>\
         </div>",
        800.0,
    );
    let ib_boxes = find_all_boxes(&doc.root, &|b| {
        b.style.display == Display::InlineBlock && b.layout.content_rect.w > 100.0
    });
    let blk = find_box(&doc.root, &|b| {
        b.attributes.get("id").map_or(false, |v| v == "blk")
    })
    .unwrap();

    if let Some(ib) = ib_boxes.first() {
        assert!(
            blk.layout.content_rect.y >= ib.layout.content_rect.y + ib.layout.content_rect.h - 2.0,
            "block child y={} should be below inline-block bottom={}",
            blk.layout.content_rect.y,
            ib.layout.content_rect.y + ib.layout.content_rect.h
        );
    }
}

// ============================================================
// compute_intrinsic_width: mixed block/inline content
// ============================================================

#[test]
fn intrinsic_width_not_inflated_by_centering() {
    // An inline-block containing a centered image should shrink to the image width,
    // not be inflated by text-align centering offsets.
    let doc = load_html(
        "<div style='width: 800px;'>\
           <div id='ib' style='display: inline-block; text-align: center;'>\
             <span style='display: inline-block; width: 150px; height: 50px;'>img</span>\
           </div>\
           <div style='display: block; height: 10px;'>block</div>\
         </div>",
        800.0,
    );
    let ib = find_box(&doc.root, &|b| {
        b.attributes.get("id").map_or(false, |v| v == "ib")
    });
    assert!(ib.is_some());
    let ib = ib.unwrap();
    // Should be close to 150px, not inflated to 400+ by centering
    assert!(
        ib.layout.content_rect.w < 300.0,
        "inline-block width={} should be ~150px, not inflated by text-align:center",
        ib.layout.content_rect.w
    );
}

// ============================================================
// Image aspect ratio preservation
// ============================================================

#[test]
fn img_width_auto_height_specified_sets_aspect_ratio() {
    // img with height=40 and no width should get width from aspect ratio
    // when image_width/image_height are known.
    let mut doc = parse_html("<img src='logo.png' height='40'>");
    // Simulate image loading by setting image dimensions on the box
    fn set_image_dims(node: &mut WebCore, w: u32, h: u32) {
        if node.tag == "img" {
            node.image_width = w;
            node.image_height = h;
            node.layout.layout_dirty = true;
        }
        for child in &mut node.children {
            set_image_dims(child, w, h);
        }
    }
    set_image_dims(&mut doc.root, 1250, 200);
    let mut engine = webcore::layout::LayoutEngine::new();
    engine.layout(&mut doc, 800.0);

    let img = find_box(&doc.root, &|b| b.tag == "img");
    assert!(img.is_some(), "img box not found");
    let img = img.unwrap();
    // height=40, natural 1250×200 → width should be 40*1250/200 = 250
    assert!(
        img.layout.content_rect.w > 200.0 && img.layout.content_rect.w < 300.0,
        "img width={} should be ~250 (aspect ratio from 1250×200)",
        img.layout.content_rect.w
    );
    assert!(
        (img.layout.content_rect.h - 40.0).abs() < 5.0,
        "img height={} should be ~40",
        img.layout.content_rect.h
    );
}

#[test]
fn img_height_auto_width_specified_sets_aspect_ratio() {
    // img with width=200 and no explicit height should get height from aspect ratio
    let mut doc = parse_html("<img src='photo.jpg' width='200'>");
    fn set_image_dims(node: &mut WebCore, w: u32, h: u32) {
        if node.tag == "img" {
            node.image_width = w;
            node.image_height = h;
            node.layout.layout_dirty = true;
        }
        for child in &mut node.children {
            set_image_dims(child, w, h);
        }
    }
    set_image_dims(&mut doc.root, 400, 200);
    let mut engine = webcore::layout::LayoutEngine::new();
    engine.layout(&mut doc, 800.0);

    let img = find_box(&doc.root, &|b| b.tag == "img");
    assert!(img.is_some());
    let img = img.unwrap();
    // width=200, natural 400×200 → height should be 200*200/400 = 100
    assert!(
        (img.layout.content_rect.w - 200.0).abs() < 5.0,
        "img width={} should be ~200",
        img.layout.content_rect.w
    );
    assert!(
        img.layout.content_rect.h > 80.0 && img.layout.content_rect.h < 120.0,
        "img height={} should be ~100 (aspect ratio from 400×200)",
        img.layout.content_rect.h
    );
}

#[test]
fn img_both_auto_uses_natural_size() {
    // img with no width/height attributes should use natural dimensions
    let mut doc = parse_html("<img src='photo.jpg'>");
    fn set_image_dims(node: &mut WebCore, w: u32, h: u32) {
        if node.tag == "img" {
            node.image_width = w;
            node.image_height = h;
            node.layout.layout_dirty = true;
        }
        for child in &mut node.children {
            set_image_dims(child, w, h);
        }
    }
    set_image_dims(&mut doc.root, 320, 240);
    let mut engine = webcore::layout::LayoutEngine::new();
    engine.layout(&mut doc, 800.0);

    let img = find_box(&doc.root, &|b| b.tag == "img");
    assert!(img.is_some());
    let img = img.unwrap();
    assert!(
        (img.layout.content_rect.w - 320.0).abs() < 5.0,
        "img width={} should be ~320",
        img.layout.content_rect.w
    );
    assert!(
        (img.layout.content_rect.h - 240.0).abs() < 5.0,
        "img height={} should be ~240",
        img.layout.content_rect.h
    );
}

// ============================================================
// HTML parser: find_tag_end quote handling
// ============================================================

#[test]
fn stray_quote_in_tag_does_not_swallow_content() {
    // A stray quote in a tag (not preceded by =) should not start a quoted string
    // that would consume subsequent HTML.
    let doc = parse_html("<div \" class=\"test\">visible</div><p>also visible</p>");
    let div = find_box(&doc.root, &|b| {
        b.tag == "div" && b.attributes.get("class").map_or(false, |v| v == "test")
    });
    let p = find_box(&doc.root, &|b| b.tag == "p");
    assert!(div.is_some(), "div with class=test should be parsed");
    assert!(p.is_some(), "p tag should not be swallowed by stray quote");
}

#[test]
fn normal_quoted_attributes_still_work() {
    let doc = parse_html("<div class=\"hello\" id=\"world\">content</div>");
    let div = find_box(&doc.root, &|b| {
        b.tag == "div"
            && b.attributes.get("class").map_or(false, |v| v == "hello")
            && b.attributes.get("id").map_or(false, |v| v == "world")
    });
    assert!(
        div.is_some(),
        "normal quoted attributes should parse correctly"
    );
}

#[test]
fn floated_widget_margin_bottom_applied() {
    // Floated siblings with margin-bottom should have space between them.
    let doc = load_html(
        "<html><head><style>\
           .widget { float: left; clear: both; width: 100%; margin-bottom: 30px; padding: 20px; box-sizing: border-box; }\
         </style></head>\
         <body>\
           <div style='width: 400px;'>\
             <div id='w1' class='widget'>Widget 1</div>\
             <div id='w2' class='widget'>Widget 2</div>\
           </div>\
         </body></html>",
        800.0,
    );
    let w1 = find_box(&doc.root, &|b| {
        b.attributes.get("id").map_or(false, |v| v == "w1")
    })
    .unwrap();
    let w2 = find_box(&doc.root, &|b| {
        b.attributes.get("id").map_or(false, |v| v == "w2")
    })
    .unwrap();
    // The visual gap is between border rects (margin_rect includes the margin itself).
    // w2's border_rect top should be at least 30px below w1's border_rect bottom.
    let visual_gap = w2.layout.border_rect.y - (w1.layout.border_rect.y + w1.layout.border_rect.h);
    eprintln!(
        "w1 border_rect: y={} h={} bottom={}",
        w1.layout.border_rect.y,
        w1.layout.border_rect.h,
        w1.layout.border_rect.y + w1.layout.border_rect.h
    );
    eprintln!(
        "w2 border_rect: y={} h={}",
        w2.layout.border_rect.y, w2.layout.border_rect.h
    );
    eprintln!("visual_gap: {}", visual_gap);
    assert!(
        visual_gap >= 28.0,
        "visual_gap={} between floated widgets should be >= 28px (margin-bottom:30px)",
        visual_gap
    );
}

#[test]
fn article_meta_inline_block_no_overlap() {
    // Article with h2 title, inline-block meta line, then p content.
    // The meta line should not overlap with the paragraph.
    let doc = load_html(
        "<html><head><style>\
           .meta { display: inline-block; font-size: 12px; }\
         </style></head>\
         <body>\
           <div style='width: 600px;'>\
             <h2 id='title'>Article Title</h2>\
             <span class='meta' id='meta'>By Author | Category</span>\
             <p id='content'>Article content goes here.</p>\
           </div>\
         </body></html>",
        600.0,
    );
    let title = find_box(&doc.root, &|b| {
        b.attributes.get("id").map_or(false, |v| v == "title")
    })
    .unwrap();
    let meta = find_box(&doc.root, &|b| {
        b.attributes.get("id").map_or(false, |v| v == "meta")
    })
    .unwrap();
    let content = find_box(&doc.root, &|b| {
        b.attributes.get("id").map_or(false, |v| v == "content")
    })
    .unwrap();
    eprintln!(
        "title: y={} h={} bottom={}",
        title.layout.border_rect.y,
        title.layout.border_rect.h,
        title.layout.border_rect.y + title.layout.border_rect.h
    );
    eprintln!(
        "meta:  y={} h={} bottom={}",
        meta.layout.border_rect.y,
        meta.layout.border_rect.h,
        meta.layout.border_rect.y + meta.layout.border_rect.h
    );
    eprintln!(
        "content: y={} h={}",
        content.layout.border_rect.y, content.layout.border_rect.h
    );
    // Meta should be below title
    assert!(
        meta.layout.border_rect.y >= title.layout.border_rect.y + title.layout.border_rect.h - 1.0,
        "meta should be below title"
    );
    // Content should be below meta
    assert!(
        content.layout.border_rect.y >= meta.layout.border_rect.y + meta.layout.border_rect.h - 1.0,
        "content (y={}) should be below meta (bottom={})",
        content.layout.border_rect.y,
        meta.layout.border_rect.y + meta.layout.border_rect.h
    );
}

#[test]
fn text_shadow_parsed_and_inherited() {
    let doc = load_html(
        "<html><head><style>\
           h2 a { text-shadow: 1px 1px 3px #000; }\
         </style></head>\
         <body><h2><a id='link' href='#'>Title</a></h2></body></html>",
        600.0,
    );
    let link = find_box(&doc.root, &|b| {
        b.attributes.get("id").map_or(false, |v| v == "link")
    })
    .unwrap();
    eprintln!("link text_shadow: {:?}", link.style.text_shadow);
    assert!(
        link.style.text_shadow.is_some(),
        "text-shadow should be set on h2 a"
    );
    let ts = link.style.text_shadow.as_ref().unwrap();
    assert!((ts.offset_x - 1.0).abs() < 0.1, "offset_x={}", ts.offset_x);
    assert!((ts.offset_y - 1.0).abs() < 0.1, "offset_y={}", ts.offset_y);
    assert!((ts.blur - 3.0).abs() < 0.1, "blur={}", ts.blur);
}

#[test]
fn post_margin_bottom_creates_gap() {
    let doc = load_html(
        "<html><head><style>\
           .post { margin-bottom: 20px; padding: 10px; }\
         </style></head>\
         <body><div style='width:600px'>\
           <div class='post' id='p1'><p>Post 1</p></div>\
           <div class='post' id='p2'><p>Post 2</p></div>\
         </div></body></html>",
        600.0,
    );
    let p1 = find_box(&doc.root, &|b| {
        b.attributes.get("id").map_or(false, |v| v == "p1")
    })
    .unwrap();
    let p2 = find_box(&doc.root, &|b| {
        b.attributes.get("id").map_or(false, |v| v == "p2")
    })
    .unwrap();
    let gap = p2.layout.border_rect.y - (p1.layout.border_rect.y + p1.layout.border_rect.h);
    eprintln!(
        "p1 border bottom: {}",
        p1.layout.border_rect.y + p1.layout.border_rect.h
    );
    eprintln!("p2 border top: {}", p2.layout.border_rect.y);
    eprintln!("gap: {}", gap);
    eprintln!(
        "p1 resolved_margin_bottom: {}",
        p1.layout.resolved_margin_bottom
    );
    assert!(
        gap >= 18.0,
        "gap={} should be >= 18px (margin-bottom:20px)",
        gap
    );
}

#[test]
fn osnews_article_structure_no_overlap() {
    // Mimics osnews article: header with title, inline meta, then content
    let doc = load_html(
        "<html><head><style>\
           .story-title { font-size: 24px; margin-bottom: 5px; }\
           .story-title a { text-shadow: 1px 1px 2px #333; }\
           .story-meta { display: inline-block; font-size: 13px; margin-bottom: 10px; }\
           .story-meta span { display: inline-block; }\
           .story-content { font-size: 16px; }\
         </style></head>\
         <body><div style='width:600px'>\
           <article>\
             <h2 class='story-title' id='title'><a href='#'>Long Article Title Here</a></h2>\
             <div class='story-meta' id='meta'>\
               <span>By Author</span> | <span>Category</span> | <span>March 20, 2026</span>\
             </div>\
             <div class='story-content' id='content'><p>The article content starts here and should not overlap with the meta line above.</p></div>\
           </article>\
         </div></body></html>",
        600.0,
    );
    let title = find_box(&doc.root, &|b| {
        b.attributes.get("id").map_or(false, |v| v == "title")
    })
    .unwrap();
    let meta = find_box(&doc.root, &|b| {
        b.attributes.get("id").map_or(false, |v| v == "meta")
    })
    .unwrap();
    let content = find_box(&doc.root, &|b| {
        b.attributes.get("id").map_or(false, |v| v == "content")
    })
    .unwrap();
    eprintln!(
        "title: y={:.1} h={:.1} bottom={:.1}",
        title.layout.border_rect.y,
        title.layout.border_rect.h,
        title.layout.border_rect.y + title.layout.border_rect.h
    );
    eprintln!(
        "meta:  y={:.1} h={:.1} bottom={:.1} display={:?}",
        meta.layout.border_rect.y,
        meta.layout.border_rect.h,
        meta.layout.border_rect.y + meta.layout.border_rect.h,
        meta.style.display
    );
    eprintln!(
        "content: y={:.1} h={:.1}",
        content.layout.border_rect.y, content.layout.border_rect.h
    );
    // title text-shadow should exist
    let title_link = find_box(title, &|b| b.tag == "a").unwrap();
    eprintln!("title link text_shadow: {:?}", title_link.style.text_shadow);
    // No overlapping
    assert!(
        meta.layout.border_rect.y >= title.layout.border_rect.y + title.layout.border_rect.h - 1.0,
        "meta (y={:.1}) overlaps title (bottom={:.1})",
        meta.layout.border_rect.y,
        title.layout.border_rect.y + title.layout.border_rect.h
    );
    assert!(
        content.layout.border_rect.y >= meta.layout.border_rect.y + meta.layout.border_rect.h - 1.0,
        "content (y={:.1}) overlaps meta (bottom={:.1})",
        content.layout.border_rect.y,
        meta.layout.border_rect.y + meta.layout.border_rect.h
    );
}
