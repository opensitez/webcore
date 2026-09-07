use crate::layout::LayoutEngine;
use crate::tests::harness::{find_box, parse, parse_and_layout};
use crate::types::*;

pub fn find_by_id<'a>(node: &'a WebCore, id: &str) -> Option<&'a WebCore> {
    find_box(node, &|b| {
        b.attributes.get("id").map(|s| s == id).unwrap_or(false)
    })
}

fn find_by_id_mut<'a>(node: &'a mut WebCore, id: &str) -> Option<&'a mut WebCore> {
    if node.attributes.get("id").map(|s| s == id).unwrap_or(false) {
        return Some(node);
    }
    for child in &mut node.children {
        if let Some(found) = find_by_id_mut(child, id) {
            return Some(found);
        }
    }
    None
}

#[test]
fn test_grid_auto_tracks() {
    let html = r#"
        <div style="display: grid; grid-template-columns: auto auto; width: 400px; font-size: 16px;">
            <div id="c1" style="background: red;">Longer Text Content</div>
            <div id="c2" style="background: blue;">Short</div>
        </div>
    "#;
    let doc = parse_and_layout(html, 400.0);
    let c1 = find_by_id(&doc.root, "c1").unwrap();
    let c2 = find_by_id(&doc.root, "c2").unwrap();

    // With auto auto, items should take their content width.
    // "Longer Text Content" is ~19 chars. "Short" is 5 chars.
    // In current buggy implementation, they might get 200px each (equal share).
    assert!(c1.layout.border_rect.w > c2.layout.border_rect.w);
}

#[test]
fn test_grid_item_stretch_background() {
    let html = r#"
        <div style="display: grid; grid-template-columns: 100px 100px; width: 200px; font-size: 16px;">
            <div id="c1" style="background: red; display: inline-block;">Small</div>
            <div id="c2" style="background: blue;">Block</div>
        </div>
    "#;
    let doc = parse_and_layout(html, 200.0);
    let c1 = find_by_id(&doc.root, "c1").unwrap();
    let c2 = find_by_id(&doc.root, "c2").unwrap();

    // Default justify-self is stretch.
    // c1 is inline-block, so it might shrink to fit if not forced to stretch.
    // c2 is block, so it should stretch naturally.
    assert_eq!(c1.layout.border_rect.w, 100.0);
    assert_eq!(c2.layout.border_rect.w, 100.0);
}

#[test]
fn test_grid_fr_with_auto() {
    let html = r#"
        <div style="display: grid; grid-template-columns: auto 1fr; width: 400px; gap: 0;">
            <div id="c1" style="width: 100px;">Fixed</div>
            <div id="c2">Flexible</div>
        </div>
    "#;
    let doc = parse_and_layout(html, 400.0);
    let c1 = find_by_id(&doc.root, "c1").unwrap();
    let c2 = find_by_id(&doc.root, "c2").unwrap();

    // c1 should be 100px, c2 should be 300px.
    // In current buggy implementation, if there is an 'fr', 'auto' gets 0.
    assert_eq!(c1.layout.border_rect.w, 100.0);
    assert_eq!(c2.layout.border_rect.w, 300.0);
}

// ── Subgrid tests ─────────────────────────────────────────────────────────────

#[test]
fn subgrid_columns_parsed() {
    use crate::css::parse_track_list;
    let mut dummy = Vec::new();
    let tracks = parse_track_list("subgrid", &mut dummy);
    assert_eq!(tracks.len(), 1);
    assert!(
        tracks[0].is_subgrid(),
        "subgrid keyword should produce a Subgrid sentinel"
    );
}

#[test]
fn subgrid_columns_flag_set_in_style() {
    let doc = parse_and_layout(
        r#"<html><body>
          <div style="display:grid; grid-template-columns: 100px 200px; width:400px">
            <div id="sub" style="display:grid; grid-template-columns:subgrid; grid-column:1/3">
              <div id="a">A</div>
              <div id="b">B</div>
            </div>
          </div>
        </body></html>"#,
        400.0,
    );
    let sub = find_by_id(&doc.root, "sub").expect("sub");
    assert!(
        sub.style.subgrid_columns,
        "subgrid_columns flag must be set"
    );
    assert!(!sub.style.subgrid_rows, "subgrid_rows must not be set");
}

#[test]
fn grid_repeat_count_can_come_from_same_rule_custom_property() {
    let doc = parse_and_layout(
        r#"<html><body style="margin:0">
          <style>
            .grid {
              --cols: 12;
              display: grid;
              grid-template-columns: repeat(var(--cols), 1fr);
              column-gap: 24px;
              width: 888px;
            }
            .hero {
              display: grid;
              grid-template-columns: subgrid;
              grid-column: 1 / -1;
            }
            .lead { grid-column: 1 / 9; height: 20px; }
            .rail { grid-column: 9 / 13; height: 20px; }
          </style>
          <section class="grid">
            <div id="hero" class="hero">
              <div id="lead" class="lead"></div>
              <div id="rail" class="rail"></div>
            </div>
          </section>
        </body></html>"#,
        1000.0,
    );

    let hero = find_by_id(&doc.root, "hero").expect("hero");
    let lead = find_by_id(&doc.root, "lead").expect("lead");
    let rail = find_by_id(&doc.root, "rail").expect("rail");

    assert!(
        (hero.layout.content_rect.w - 888.0).abs() < 0.5,
        "subgrid item should inherit the 12-column parent width, got {}",
        hero.layout.content_rect.w
    );
    assert!(
        (lead.layout.content_rect.w - 584.0).abs() < 0.5,
        "lead spans 8 tracks plus 7 gaps: got {}",
        lead.layout.content_rect.w
    );
    assert!(
        (rail.layout.content_rect.x - 608.0).abs() < 0.5,
        "rail should start after 8 tracks plus 8 gaps: got x={}",
        rail.layout.content_rect.x
    );
}

#[test]
fn grid_full_span_subgrid_with_fluid_image_does_not_inflate_fr_tracks() {
    let mut doc = parse(
        r#"<html><body style="margin:0">
          <style>
            .grid {
              --cols: 12;
              display: grid;
              grid-template-columns: repeat(var(--cols), 1fr);
              column-gap: 24px;
              width: 888px;
            }
            .hero.yf-1nj5315 {
              --img-aspect-ratio: 16 / 9;
              --img-object-fit: cover;
              --img-width: 100%;
              display: grid;
              grid-template-columns: subgrid;
              grid-column: 1 / -1;
            }
            @media only screen and (min-width: 768px) {
              .lead.yf-1nj5315 { grid-column: 1 / 7; grid-row: 1; }
            }
            @media only screen and (min-width: 1050px) {
              .lead.yf-1nj5315 { grid-column: 1 / 13; }
            }
            @media only screen and (min-width: 1280px) {
              .lead.yf-1nj5315 { grid-column: 1 / 9; }
            }
            .lead-story.yf-mr8u9k {
              display: grid;
              grid-template-columns: 1fr;
              gap: 16px;
            }
            @media only screen and (min-width: 768px) {
              .lead-story.yf-mr8u9k {
                grid-template-columns: 1fr 1fr;
                gap: 24px;
                align-items: start;
              }
            }
            @media only screen and (min-width: 1280px) {
              .lead-story.yf-mr8u9k {
                grid-template-columns: 1fr;
                gap: 24px;
                align-items: unset;
              }
            }
            .story-img-wrapper {
              position: relative;
              height: min-content;
              width: 100%;
            }
            .story-img { display: block; width: var(--img-width, unset); height: auto; }
            img.yf-1ev0m0b {
              aspect-ratio: var(--img-aspect-ratio, unset);
              object-fit: var(--img-object-fit, unset);
            }
            .rail { grid-column: 9 / 13; height: 20px; }
          </style>
          <section id="grid" class="grid">
            <div id="hero" class="hero yf-1nj5315">
              <section id="lead" class="section lead yf-1nj5315">
                <div id="lead-story" class="lead-story yf-mr8u9k">
                  <div id="visual" class="visual">
                    <a id="wrap" class="story-img-wrapper">
                      <img id="pic" class="story-img yf-1ev0m0b" src="" width="1312" height="738">
                    </a>
                  </div>
                </div>
              </section>
              <div id="rail" class="rail"></div>
            </div>
          </section>
        </body></html>"#,
    );
    let pic = find_by_id_mut(&mut doc.root, "pic").expect("pic");
    pic.image_width = 1312;
    pic.image_height = 738;
    pic.image_data = Some(std::sync::Arc::new(vec![0xff; 1312 * 738 * 4]));
    LayoutEngine::new().layout(&mut doc, 1280.0);

    let grid = find_by_id(&doc.root, "grid").expect("grid");
    let hero = find_by_id(&doc.root, "hero").expect("hero");
    let lead = find_by_id(&doc.root, "lead").expect("lead");
    let lead_story = find_by_id(&doc.root, "lead-story").expect("lead story");
    let visual = find_by_id(&doc.root, "visual").expect("visual");
    let wrap = find_by_id(&doc.root, "wrap").expect("wrap");
    let pic = find_by_id(&doc.root, "pic").expect("pic");
    let rail = find_by_id(&doc.root, "rail").expect("rail");

    assert!(
        (grid.layout.content_rect.w - 888.0).abs() < 0.5,
        "outer grid should keep its definite width, got {}",
        grid.layout.content_rect.w
    );
    assert!(
        (hero.layout.content_rect.w - 888.0).abs() < 0.5,
        "full-span subgrid should not grow to image natural width, got {}",
        hero.layout.content_rect.w
    );
    assert!(
        (lead.layout.content_rect.w - 584.0).abs() < 0.5,
        "lead should use 8 inherited tracks plus 7 gaps, got {}",
        lead.layout.content_rect.w
    );
    assert!(
        (lead_story.layout.content_rect.w - lead.layout.content_rect.w).abs() < 0.5,
        "inner lead-story grid should stay constrained by the lead column, got {} vs {}",
        lead_story.layout.content_rect.w,
        lead.layout.content_rect.w
    );
    assert!(
        (visual.layout.content_rect.w - lead.layout.content_rect.w).abs() < 0.5,
        "visual grid item should stay constrained by the lead column, got {} vs {}",
        visual.layout.content_rect.w,
        lead.layout.content_rect.w
    );
    assert!(
        (wrap.layout.content_rect.w - lead.layout.content_rect.w).abs() < 0.5,
        "fluid image wrapper should stay constrained by the lead column, got {} vs {}",
        wrap.layout.content_rect.w,
        lead.layout.content_rect.w
    );
    assert!(
        (pic.layout.content_rect.w - lead.layout.content_rect.w).abs() < 0.5,
        "fluid image should resolve against the lead column span, got {} vs {}",
        pic.layout.content_rect.w,
        lead.layout.content_rect.w
    );
    assert!(
        (rail.layout.content_rect.x - 608.0).abs() < 0.5,
        "rail should start in column 9, got x={}",
        rail.layout.content_rect.x
    );
}

#[test]
fn subgrid_children_align_to_parent_tracks() {
    // Parent: 3 columns of 100px each. Subgrid spans all 3 and also has 3 children.
    // Each child should be 100px wide (inheriting parent track sizes).
    let doc = parse_and_layout(
        r#"<html><body style="margin:0">
          <div id="parent" style="display:grid; grid-template-columns:100px 100px 100px;
                                  column-gap:0; width:300px; margin:0">
            <div id="sub" style="display:grid; grid-template-columns:subgrid;
                                  grid-column:1/4; margin:0; padding:0">
              <div id="a" style="margin:0">A</div>
              <div id="b" style="margin:0">B</div>
              <div id="c" style="margin:0">C</div>
            </div>
          </div>
        </body></html>"#,
        400.0,
    );
    let a = find_by_id(&doc.root, "a").expect("a");
    let b = find_by_id(&doc.root, "b").expect("b");
    let c = find_by_id(&doc.root, "c").expect("c");

    assert!(
        (a.layout.border_rect.w - 100.0).abs() < 2.0,
        "subgrid child a width should be 100px, got {}",
        a.layout.border_rect.w
    );
    assert!(
        (b.layout.border_rect.w - 100.0).abs() < 2.0,
        "subgrid child b width should be 100px, got {}",
        b.layout.border_rect.w
    );
    assert!(
        (c.layout.border_rect.w - 100.0).abs() < 2.0,
        "subgrid child c width should be 100px, got {}",
        c.layout.border_rect.w
    );

    // Children should be horizontally sequential
    assert!(
        b.layout.border_rect.x > a.layout.border_rect.x,
        "b should be right of a"
    );
    assert!(
        c.layout.border_rect.x > b.layout.border_rect.x,
        "c should be right of b"
    );
}

#[test]
fn subgrid_children_x_positions_match_parent_tracks() {
    // Parent columns: 50px, 150px, 100px with 0 gap.
    // Subgrid spans columns 1-3, its children should align at x=0, x=50, x=200.
    let doc = parse_and_layout(
        r#"<html><body style="margin:0">
          <div id="parent" style="display:grid; grid-template-columns:50px 150px 100px;
                                  column-gap:0; width:300px; margin:0">
            <div id="sub" style="display:grid; grid-template-columns:subgrid;
                                  grid-column:1/4; margin:0; padding:0">
              <div id="a" style="margin:0">A</div>
              <div id="b" style="margin:0">B</div>
              <div id="c" style="margin:0">C</div>
            </div>
          </div>
        </body></html>"#,
        400.0,
    );
    let a = find_by_id(&doc.root, "a").expect("a");
    let b = find_by_id(&doc.root, "b").expect("b");
    let c = find_by_id(&doc.root, "c").expect("c");

    assert!(
        (a.layout.border_rect.w - 50.0).abs() < 2.0,
        "a width={}",
        a.layout.border_rect.w
    );
    assert!(
        (b.layout.border_rect.w - 150.0).abs() < 2.0,
        "b width={}",
        b.layout.border_rect.w
    );
    assert!(
        (c.layout.border_rect.w - 100.0).abs() < 2.0,
        "c width={}",
        c.layout.border_rect.w
    );
}

#[test]
fn subgrid_rows_flag_set_in_style() {
    let doc = parse_and_layout(
        r#"<html><body>
          <div style="display:grid; grid-template-rows:50px 50px; width:200px">
            <div id="sub" style="display:grid; grid-template-rows:subgrid; grid-row:1/3;
                                  grid-template-columns:1fr">
              <div id="x">X</div>
              <div id="y">Y</div>
            </div>
          </div>
        </body></html>"#,
        400.0,
    );
    let sub = find_by_id(&doc.root, "sub").expect("sub");
    assert!(sub.style.subgrid_rows, "subgrid_rows flag must be set");
}

#[test]
fn subgrid_both_axes() {
    // Subgrid can inherit both column and row tracks simultaneously.
    let doc = parse_and_layout(
        r#"<html><body style="margin:0">
          <div id="parent" style="display:grid;
                                  grid-template-columns:100px 100px;
                                  grid-template-rows:80px 80px;
                                  column-gap:0; row-gap:0; width:200px; margin:0">
            <div id="sub" style="display:grid;
                                  grid-template-columns:subgrid;
                                  grid-template-rows:subgrid;
                                  grid-column:1/3; grid-row:1/3;
                                  margin:0; padding:0">
              <div id="a" style="margin:0">A</div>
              <div id="b" style="margin:0">B</div>
              <div id="c" style="margin:0">C</div>
              <div id="d" style="margin:0">D</div>
            </div>
          </div>
        </body></html>"#,
        400.0,
    );
    let sub = find_by_id(&doc.root, "sub").expect("sub");
    assert!(sub.style.subgrid_columns, "subgrid_columns must be set");
    assert!(sub.style.subgrid_rows, "subgrid_rows must be set");

    let a = find_by_id(&doc.root, "a").expect("a");
    let b = find_by_id(&doc.root, "b").expect("b");
    assert!(
        (a.layout.border_rect.w - 100.0).abs() < 2.0,
        "a width={}",
        a.layout.border_rect.w
    );
    assert!(
        (b.layout.border_rect.w - 100.0).abs() < 2.0,
        "b width={}",
        b.layout.border_rect.w
    );
    // a and b should be in the same row, b to the right of a
    assert!(
        b.layout.border_rect.x > a.layout.border_rect.x,
        "b must be right of a"
    );
    assert!(
        (a.layout.border_rect.y - b.layout.border_rect.y).abs() < 2.0,
        "a and b same row"
    );
}

#[test]
fn subgrid_row_list_pattern() {
    // Classic "table via subgrid" pattern:
    // Parent: 4 fixed columns. Each row is a subgrid spanning all 4.
    let doc = parse_and_layout(
        r#"<html><body style="margin:0">
          <div id="parent" style="display:grid;
                grid-template-columns:48px 200px 100px 80px;
                column-gap:0; width:428px; margin:0">
            <div id="row1" style="display:grid; grid-template-columns:subgrid;
                                  grid-column:1/5; margin:0; padding:0">
              <div id="a">A</div>
              <div id="b">B</div>
              <div id="c">C</div>
              <div id="d">D</div>
            </div>
          </div>
        </body></html>"#,
        800.0,
    );
    let a = find_by_id(&doc.root, "a").expect("a");
    let b = find_by_id(&doc.root, "b").expect("b");
    let c = find_by_id(&doc.root, "c").expect("c");
    let d = find_by_id(&doc.root, "d").expect("d");

    assert!(
        (a.layout.border_rect.w - 48.0).abs() < 2.0,
        "a={}",
        a.layout.border_rect.w
    );
    assert!(
        (b.layout.border_rect.w - 200.0).abs() < 2.0,
        "b={}",
        b.layout.border_rect.w
    );
    assert!(
        (c.layout.border_rect.w - 100.0).abs() < 2.0,
        "c={}",
        c.layout.border_rect.w
    );
    assert!(
        (d.layout.border_rect.w - 80.0).abs() < 2.0,
        "d={}",
        d.layout.border_rect.w
    );

    assert!(
        (a.layout.border_rect.x - 0.0).abs() < 2.0,
        "ax={}",
        a.layout.border_rect.x
    );
    assert!(
        (b.layout.border_rect.x - 48.0).abs() < 2.0,
        "bx={}",
        b.layout.border_rect.x
    );
    assert!(
        (c.layout.border_rect.x - 248.0).abs() < 2.0,
        "cx={}",
        c.layout.border_rect.x
    );
    assert!(
        (d.layout.border_rect.x - 348.0).abs() < 2.0,
        "dx={}",
        d.layout.border_rect.x
    );
}

#[test]
fn subgrid_multi_row_alignment() {
    // Two subgrid rows both using the same 3 parent columns.
    // All cells in column 0 should share the same x=0; column 1 → x=100; column 2 → x=260.
    let doc = parse_and_layout(
        r#"<html><body style="margin:0">
          <div id="parent" style="display:grid;
                grid-template-columns:100px 160px 80px;
                column-gap:0; width:340px; margin:0">
            <div id="r1" style="display:grid; grid-template-columns:subgrid;
                                grid-column:1/4; margin:0; padding:0">
              <div id="r1a">R1A</div><div id="r1b">R1B</div><div id="r1c">R1C</div>
            </div>
            <div id="r2" style="display:grid; grid-template-columns:subgrid;
                                grid-column:1/4; margin:0; padding:0">
              <div id="r2a">R2A</div><div id="r2b">R2B</div><div id="r2c">R2C</div>
            </div>
          </div>
        </body></html>"#,
        800.0,
    );
    let r1a = find_by_id(&doc.root, "r1a").expect("r1a");
    let r1b = find_by_id(&doc.root, "r1b").expect("r1b");
    let r2a = find_by_id(&doc.root, "r2a").expect("r2a");
    let r2b = find_by_id(&doc.root, "r2b").expect("r2b");

    // Both rows' first cells should be at x=0
    assert!(
        (r1a.layout.border_rect.x - 0.0).abs() < 2.0,
        "r1a.x={}",
        r1a.layout.border_rect.x
    );
    assert!(
        (r2a.layout.border_rect.x - 0.0).abs() < 2.0,
        "r2a.x={}",
        r2a.layout.border_rect.x
    );
    // Both rows' second cells should be at x=100
    assert!(
        (r1b.layout.border_rect.x - 100.0).abs() < 2.0,
        "r1b.x={}",
        r1b.layout.border_rect.x
    );
    assert!(
        (r2b.layout.border_rect.x - 100.0).abs() < 2.0,
        "r2b.x={}",
        r2b.layout.border_rect.x
    );
    // Row 2 should be below row 1
    assert!(
        r2a.layout.border_rect.y > r1a.layout.border_rect.y,
        "r2 must be below r1"
    );
}

// ── Placement algorithm regression tests ─────────────────────────────────────

#[test]
fn span_only_column_is_not_explicitly_placed() {
    // `grid-column: span 1` must NOT count as an explicit column placement.
    // Without this fix all items were treated as explicitly placed and stacked
    // on top of each other at (col=0, row=0).
    // Two items with `grid-column: span 1` in a 2-column grid should end up
    // side-by-side (different x positions), not overlapping.
    let doc = parse_and_layout(
        r#"<html><body style="margin:0">
          <div style="display:grid; grid-template-columns:100px 100px;
                      column-gap:0; width:200px; margin:0">
            <div id="a" style="grid-column:span 1">A</div>
            <div id="b" style="grid-column:span 1">B</div>
          </div>
        </body></html>"#,
        800.0,
    );
    let a = find_by_id(&doc.root, "a").expect("a");
    let b = find_by_id(&doc.root, "b").expect("b");
    assert!(
        (a.layout.border_rect.x - 0.0).abs() < 2.0,
        "a.x={}",
        a.layout.border_rect.x
    );
    assert!(
        (b.layout.border_rect.x - 100.0).abs() < 2.0,
        "b.x={}",
        b.layout.border_rect.x
    );
    // Same row
    assert!(
        (a.layout.border_rect.y - b.layout.border_rect.y).abs() < 2.0,
        "must be same row"
    );
}

#[test]
fn span_col_with_four_items_wraps_to_two_rows() {
    // Four items each `grid-column: span 1` in a 2-column grid.
    // Before the fix all four were placed at (0,0) and only the last was visible.
    // After the fix: items 1&2 in row 0, items 3&4 in row 1.
    let doc = parse_and_layout(
        r#"<html><body style="margin:0">
          <div style="display:grid; grid-template-columns:100px 100px;
                      column-gap:0; width:200px; margin:0">
            <div id="a" style="grid-column:span 1; height:50px">A</div>
            <div id="b" style="grid-column:span 1; height:50px">B</div>
            <div id="c" style="grid-column:span 1; height:50px">C</div>
            <div id="d" style="grid-column:span 1; height:50px">D</div>
          </div>
        </body></html>"#,
        800.0,
    );
    let a = find_by_id(&doc.root, "a").expect("a");
    let b = find_by_id(&doc.root, "b").expect("b");
    let c = find_by_id(&doc.root, "c").expect("c");
    let d = find_by_id(&doc.root, "d").expect("d");
    // Row 0: a at x=0, b at x=100
    assert!(
        (a.layout.border_rect.x - 0.0).abs() < 2.0,
        "a.x={}",
        a.layout.border_rect.x
    );
    assert!(
        (b.layout.border_rect.x - 100.0).abs() < 2.0,
        "b.x={}",
        b.layout.border_rect.x
    );
    assert!(
        (a.layout.border_rect.y - b.layout.border_rect.y).abs() < 2.0,
        "a,b same row"
    );
    // Row 1: c at x=0, d at x=100, both below row 0
    assert!(
        (c.layout.border_rect.x - 0.0).abs() < 2.0,
        "c.x={}",
        c.layout.border_rect.x
    );
    assert!(
        (d.layout.border_rect.x - 100.0).abs() < 2.0,
        "d.x={}",
        d.layout.border_rect.x
    );
    assert!(
        c.layout.border_rect.y > a.layout.border_rect.y,
        "c must be below a"
    );
    assert!(
        (c.layout.border_rect.y - d.layout.border_rect.y).abs() < 2.0,
        "c,d same row"
    );
}

#[test]
fn row_locked_auto_column_step2_placement() {
    // CSS Grid spec §8.5 step 2: items with a definite row (`grid-row: 1/2`)
    // but auto column should be locked to that row and auto-placed only in the
    // column axis — creating implicit columns if the explicit ones are full.
    // Two items with `grid-row: 1/2; grid-column: span 1` in a 1-column grid:
    // item 1 → col 1 (explicit), item 2 → col 2 (implicit).
    // Before the fix both were treated as explicitly placed and stacked at (0,0).
    let doc = parse_and_layout(
        r#"<html><body style="margin:0">
          <div style="display:grid; grid-template-columns:100px;
                      column-gap:0; width:300px; margin:0">
            <div id="a" style="grid-row:1/2; grid-column:span 1; height:40px">A</div>
            <div id="b" style="grid-row:1/2; grid-column:span 1; height:40px">B</div>
          </div>
        </body></html>"#,
        800.0,
    );
    let a = find_by_id(&doc.root, "a").expect("a");
    let b = find_by_id(&doc.root, "b").expect("b");
    // Both locked to row 0 → same y
    assert!(
        (a.layout.border_rect.y - b.layout.border_rect.y).abs() < 2.0,
        "both must be in the same row; a.y={} b.y={}",
        a.layout.border_rect.y,
        b.layout.border_rect.y
    );
    // Must be in different columns → different x
    assert!(
        (b.layout.border_rect.x - a.layout.border_rect.x).abs() > 50.0,
        "must be in different columns; a.x={} b.x={}",
        a.layout.border_rect.x,
        b.layout.border_rect.x
    );
}

// ─── col-span-full (grid-column: 1 / -1) in grids and subgrids ──────────────

#[test]
fn grid_col_span_full_spans_all_columns() {
    // grid-column: 1 / -1 should span all explicit columns
    let html = r#"
        <div style="display:grid; grid-template-columns: repeat(6, 1fr); width:600px; gap:0;">
            <div id="full" style="grid-column: 1 / -1; height:20px;"></div>
            <div id="one" style="height:20px;"></div>
        </div>
    "#;
    let doc = parse_and_layout(html, 600.0);
    let full = find_by_id(&doc.root, "full").unwrap();
    assert!(
        full.layout.content_rect.w > 500.0,
        "col-span-full must span all 6 columns, got width={}",
        full.layout.content_rect.w
    );
}

#[test]
fn subgrid_col_span_full_spans_inherited_columns() {
    // A subgrid child with grid-column: 1 / -1 should span ALL inherited columns,
    // not just 1.
    let html = r#"
        <div style="display:grid; grid-template-columns: repeat(6, 1fr); width:600px; gap:0;">
            <div id="sub" style="grid-column: 1 / -1; display:grid; grid-template-columns: subgrid;">
                <div id="inner-full" style="grid-column: 1 / -1; height:30px;"></div>
            </div>
        </div>
    "#;
    let doc = parse_and_layout(html, 600.0);
    let sub = find_by_id(&doc.root, "sub").unwrap();
    let inner = find_by_id(&doc.root, "inner-full").unwrap();
    assert!(
        sub.layout.content_rect.w > 500.0,
        "subgrid must span all 6 columns, got width={}",
        sub.layout.content_rect.w
    );
    assert!(
        inner.layout.content_rect.w > 500.0,
        "inner col-span-full in subgrid must span all inherited columns, got width={}",
        inner.layout.content_rect.w
    );
}

#[test]
fn subgrid_col_span_full_with_sibling() {
    // In a subgrid spanning 7 of 12 columns, a child with col-span-full
    // should span all 7 inherited columns, not just 1.
    let html = r#"
        <div style="display:grid; grid-template-columns: repeat(12, 1fr); width:1200px; gap:0;">
            <div style="grid-column: span 5; height:50px;"></div>
            <div id="sub" style="grid-column: span 7; display:grid; grid-template-columns: subgrid;">
                <div id="first" style="grid-column: span 3; height:30px;"></div>
                <div id="rest" style="grid-column: 1 / -1; height:30px;"></div>
            </div>
        </div>
    "#;
    let doc = parse_and_layout(html, 1200.0);
    let sub = find_by_id(&doc.root, "sub").unwrap();
    let rest = find_by_id(&doc.root, "rest").unwrap();
    // sub spans 7 columns = 700px
    assert!(
        sub.layout.content_rect.w > 600.0,
        "subgrid spanning 7 columns should be ~700px, got {}",
        sub.layout.content_rect.w
    );
    // rest with col-span-full should also span all 7 inherited columns
    assert!(
        rest.layout.content_rect.w > 600.0,
        "col-span-full in subgrid must span all 7 inherited columns, got {}",
        rest.layout.content_rect.w
    );
}

#[test]
fn subgrid_column_locked_item_uses_explicit_columns() {
    // Item with explicit grid-column-start but auto row should be column-locked,
    // not auto-placed with span=1.
    let html = r#"
        <div style="display:grid; grid-template-columns: repeat(6, 1fr); width:600px; gap:0;">
            <div id="sub" style="grid-column: 1 / -1; display:grid; grid-template-columns: subgrid;">
                <div id="a" style="grid-column: span 2; height:30px;"></div>
                <div id="b" style="grid-column: 3 / 6; height:30px;"></div>
            </div>
        </div>
    "#;
    let doc = parse_and_layout(html, 600.0);
    let a = find_by_id(&doc.root, "a").unwrap();
    let b = find_by_id(&doc.root, "b").unwrap();
    // a: spans 2 columns = 200px
    assert!(
        a.layout.content_rect.w > 150.0,
        "a spanning 2 columns should be ~200px, got {}",
        a.layout.content_rect.w
    );
    // b: columns 3..6 = 3 columns = 300px
    assert!(
        b.layout.content_rect.w > 250.0,
        "b spanning columns 3-6 should be ~300px, got {}",
        b.layout.content_rect.w
    );
}

#[test]
fn nested_subgrid_col_span_full() {
    // Two levels of subgrid, innermost has col-span-full
    let html = r#"
        <div style="display:grid; grid-template-columns: repeat(12, 1fr); width:1200px; gap:0;">
            <div id="outer-sub" style="grid-column: span 6; display:grid; grid-template-columns: subgrid;">
                <div id="inner-sub" style="grid-column: 1 / -1; display:grid; grid-template-columns: subgrid;">
                    <div id="deep" style="grid-column: 1 / -1; height:20px;"></div>
                </div>
            </div>
        </div>
    "#;
    let doc = parse_and_layout(html, 1200.0);
    let outer = find_by_id(&doc.root, "outer-sub").unwrap();
    let inner = find_by_id(&doc.root, "inner-sub").unwrap();
    let deep = find_by_id(&doc.root, "deep").unwrap();
    // Each should span 6 columns = 600px
    assert!(
        outer.layout.content_rect.w > 500.0,
        "outer subgrid must be ~600px, got {}",
        outer.layout.content_rect.w
    );
    assert!(
        inner.layout.content_rect.w > 500.0,
        "inner subgrid with col-span-full must be ~600px, got {}",
        inner.layout.content_rect.w
    );
    assert!(
        deep.layout.content_rect.w > 500.0,
        "deep col-span-full in nested subgrid must be ~600px, got {}",
        deep.layout.content_rect.w
    );
}

#[test]
fn aol_trending_layout_pattern() {
    // Mimics the netscape.com trending section:
    // 12-col grid > [span-5] [span-7 subgrid > [span-3] [col-span-full subgrid]]
    let html = r#"
        <div style="display:grid; grid-template-columns: repeat(12, 1fr); width:1200px; column-gap:40px;">
            <div id="left" style="grid-column: span 5; height:100px;"></div>
            <div id="right" style="grid-column: span 7; display:grid; grid-template-columns: subgrid;">
                <div id="col3" style="grid-column: span 3; height:80px;"></div>
                <div id="articles" style="grid-column: 1 / -1; display:grid; grid-template-columns: subgrid;">
                    <div id="art1" style="grid-column: span 3; height:40px;"></div>
                    <div id="art2" style="grid-column: span 3; height:40px;"></div>
                </div>
            </div>
        </div>
    "#;
    let doc = parse_and_layout(html, 1200.0);
    let right = find_by_id(&doc.root, "right").unwrap();
    let articles = find_by_id(&doc.root, "articles").unwrap();
    let art1 = find_by_id(&doc.root, "art1").unwrap();

    // right spans 7 columns
    assert!(
        right.layout.content_rect.w > 400.0,
        "right panel (7 cols) too narrow: {}",
        right.layout.content_rect.w
    );
    // articles with col-span-full should span all 7 inherited columns
    assert!(
        articles.layout.content_rect.w > 400.0,
        "articles col-span-full in subgrid too narrow: {} (should match right={})",
        articles.layout.content_rect.w,
        right.layout.content_rect.w
    );
    // art1 with span 3 should be ~3/7 of articles width
    assert!(
        art1.layout.content_rect.w > 100.0,
        "art1 span-3 in nested subgrid too narrow: {}",
        art1.layout.content_rect.w
    );
}

// ── Spanning items must not inflate the tracks they cross ───────────────────

/// **CSS Grid §12.5 — a spanning item contributes its EXCESS, not its size
/// divided by its span.** Tracks are sized from single-span items first; a
/// spanning item then adds only what the tracks it crosses do not already
/// provide, shared among them. The old code divided the item's whole size by
/// its span and used that as a FLOOR on every crossed track, so one tall item
/// inflated unrelated short rows — on fr.wikipedia a 6810px sidebar pushed two
/// ~40px rows to 1811px each and shoved the page below the fold.
///
/// Numbers checked against Chrome on this exact fixture: head/tool 106,
/// body 300, grid 600. The old floor gave head/tool 200 and a 700px grid.
#[test]
fn grid_a_tall_spanning_item_does_not_inflate_short_rows() {
    let doc = parse_and_layout(
        r#"
        <style>
          * { margin: 0; padding: 0 }
          .g { display: grid; grid-template-columns: 800px 200px; }
          #head { grid-column: 1; grid-row: 1 }
          #tool { grid-column: 1; grid-row: 2 }
          #rail { grid-column: 2; grid-row: 1 / span 3; height: 600px }
          #body { grid-column: 1; grid-row: 3; height: 300px }
        </style>
        <div class="g">
          <div id="head">h</div>
          <div id="tool">t</div>
          <div id="rail">rail</div>
          <div id="body">b</div>
        </div>
    "#,
        1000.0,
    );
    let h = |id: &str| find_by_id(&doc.root, id).unwrap().layout.margin_rect.h;
    let y = |id: &str| find_by_id(&doc.root, id).unwrap().layout.margin_rect.y;
    assert!(
        (h("head") - h("tool")).abs() < 0.5,
        "the two auto rows share the excess equally: {} vs {}",
        h("head"),
        h("tool")
    );
    assert!(
        h("head") < 150.0,
        "row 1 must not take span-height/3 as a floor, got {}",
        h("head")
    );
    assert!(
        (h("head") - 106.0).abs() < 2.0,
        "Chrome gives 106, got {}",
        h("head")
    );
    assert_eq!(h("body"), 300.0, "row 3 keeps its explicit height");
    assert!(
        (y("body") - 212.0).abs() < 2.0,
        "Chrome puts row 3 at y=212, got {}",
        y("body")
    );
    assert_eq!(h("rail"), 600.0, "the spanning item keeps its height");
}

/// The spanning item still gets the room it needs: the grid as a whole must be
/// tall enough for it.
#[test]
fn grid_a_spanning_item_still_gets_its_height() {
    let doc = parse_and_layout(
        r#"
        <style>
          * { margin: 0; padding: 0 }
          .g { display: grid; grid-template-columns: 100px 100px; }
          #a { grid-column: 1; grid-row: 1 }
          #b { grid-column: 1; grid-row: 2 }
          #tall { grid-column: 2; grid-row: 1 / span 2; height: 400px }
        </style>
        <div class="g"><div id=a>a</div><div id=b>b</div><div id=tall>t</div></div>
    "#,
        400.0,
    );
    let g = find_box(&doc.root, &|b: &WebCore| {
        b.attributes
            .get("class")
            .map(|c| c.split_whitespace().any(|w| w == "g"))
            .unwrap_or(false)
    })
    .unwrap()
    .layout
    .margin_rect
    .h;
    assert!(
        g >= 400.0,
        "the grid must be tall enough for the spanning item, got {g}"
    );
}

// ── Column-axis track sizing (CSS Grid §12.5-12.7) ───────────────────────────

/// **§12.5 — a spanning item contributes its EXCESS on the COLUMN axis too.**
/// The row axis already did this; columns divided the item's max-content width
/// by its span and used the quotient as a FLOOR on every spanned track, which
/// inflated a narrow column and squeezed a wide one.
/// Chrome on this fixture: 180px / 120px. The floor gave 150 / 150.
#[test]
fn grid_a_spanning_item_contributes_only_its_column_excess() {
    let doc = parse_and_layout(
        r#"
        <style>
          * { margin:0; padding:0 }
          .g { display:grid; grid-template-columns: auto auto; width:300px }
        </style>
        <div class="g">
          <div id="a1" style="width:100px;height:10px;grid-column:1"></div>
          <div id="a2" style="width:40px;height:10px;grid-column:2"></div>
          <div id="a3" style="width:300px;height:10px;grid-column:1/3"></div>
        </div>
    "#,
        1000.0,
    );
    let x = |id: &str| find_by_id(&doc.root, id).unwrap().layout.margin_rect.x;
    // Column 1 is 180 wide, so column 2 starts at x=180 (Chrome: 180 / 120).
    assert!(
        (x("a2") - 180.0).abs() < 0.5,
        "column 1 must be 180 (100 + half the 160px excess), got col2 x={}",
        x("a2")
    );
}

/// **§12.5 — a `min-content` track uses the MIN-content contribution.**
/// Both track kinds used the item's max-content width, so `min-content` was a
/// synonym for `max-content`. Chrome on this fixture: 50px / 60px.
#[test]
fn grid_min_content_track_uses_the_min_content_contribution() {
    let doc = parse_and_layout(
        r#"
        <style>
          * { margin:0; padding:0 }
          .g { display:grid; grid-template-columns: min-content max-content; width:400px }
          .s { display:inline-block; height:10px }
        </style>
        <div class="g">
          <div id="b1" style="height:10px"><span class="s" style="width:50px"></span><span class="s" style="width:50px"></span></div>
          <div id="b2" style="height:10px"><span class="s" style="width:30px"></span><span class="s" style="width:30px"></span></div>
        </div>
    "#,
        1000.0,
    );
    let w = |id: &str| find_by_id(&doc.root, id).unwrap().layout.border_rect.w;
    let x = |id: &str| find_by_id(&doc.root, id).unwrap().layout.border_rect.x;
    assert!(
        (w("b1") - 50.0).abs() < 0.5,
        "min-content track = the widest unbreakable piece (50), got {}",
        w("b1")
    );
    assert!(
        (x("b2") - 50.0).abs() < 0.5,
        "column 2 starts after a 50px column 1, got {}",
        x("b2")
    );
    assert!(
        (w("b2") - 60.0).abs() < 0.5,
        "max-content track = 60, and §12.7 stretches only `auto` tracks, got {}",
        w("b2")
    );
}

/// **§12.6/§12.7 — a `max-content` track freezes at its growth limit; only
/// `auto` tracks take the leftover space.** Every intrinsic track used to get
/// an equal share of the free space unconditionally.
/// Chrome on this fixture: 100px / 300px. The equal share gave 230 / 170.
#[test]
fn grid_only_auto_tracks_take_the_leftover_space() {
    let doc = parse_and_layout(
        r#"
        <style>
          * { margin:0; padding:0 }
          .g { display:grid; grid-template-columns: max-content auto; width:400px }
        </style>
        <div class="g">
          <div id="c1" style="width:100px;height:10px;grid-column:1;grid-row:1"></div>
          <div id="c2" style="width:40px;height:10px;grid-column:2;grid-row:1"></div>
          <div id="c3" style="grid-column:2;grid-row:2;height:10px"></div>
        </div>
    "#,
        1000.0,
    );
    let w = |id: &str| find_by_id(&doc.root, id).unwrap().layout.border_rect.w;
    let x = |id: &str| find_by_id(&doc.root, id).unwrap().layout.border_rect.x;
    assert!(
        (x("c2") - 100.0).abs() < 0.5,
        "the max-content track stays at 100, got column 2 x={}",
        x("c2")
    );
    // c3 has no width of its own, so it stretches to the track and reports it.
    assert!(
        (w("c3") - 300.0).abs() < 0.5,
        "the auto track takes all 300 of the leftover space, got {}",
        w("c3")
    );
}

// ── Row-axis track sizing ────────────────────────────────────────────────────

/// **§12.7 "Stretch auto Tracks" applies to the BLOCK axis too.**
/// `align-content`'s initial value IS `stretch`, so a grid with a definite
/// height and `auto` rows must divide the leftover space equally among them.
/// It never did: the rows stayed at content height and the rest of the box was
/// wasted. Chrome on this fixture: rows 190 / 210 (30+160 and 50+160).
#[test]
fn grid_align_content_stretch_grows_auto_rows() {
    let doc = parse_and_layout(
        r#"
        <style>
          * { margin:0; padding:0 }
          .g { display:grid; grid-template-rows: auto auto; height:400px; width:200px }
        </style>
        <div class="g">
          <div id="d1" style="height:30px"></div>
          <div id="d2" style="height:50px"></div>
        </div>
    "#,
        1000.0,
    );
    let y = |id: &str| find_by_id(&doc.root, id).unwrap().layout.margin_rect.y;
    assert!(
        (y("d2") - 190.0).abs() < 0.5,
        "row 1 stretches from 30 to 190, so row 2 starts at y=190, got {}",
        y("d2")
    );
}

/// The same stretch with no `grid-template-rows` at all — the implicit rows
/// take `grid-auto-rows: auto`, whose max sizing function is `auto`.
#[test]
fn grid_align_content_stretch_grows_implicit_auto_rows() {
    let doc = parse_and_layout(
        r#"
        <style>
          * { margin:0; padding:0 }
          .g { display:grid; height:400px; width:200px }
        </style>
        <div class="g">
          <div id="e1" style="height:30px"></div>
          <div id="e2" style="height:50px"></div>
        </div>
    "#,
        1000.0,
    );
    let y = |id: &str| find_by_id(&doc.root, id).unwrap().layout.margin_rect.y;
    assert!(
        (y("e2") - 190.0).abs() < 0.5,
        "Chrome puts row 2 at y=190, got {}",
        y("e2")
    );
}

/// A fixed row does not stretch; the single `auto` row absorbs everything.
/// Chrome on this fixture: rows 100 / 300.
#[test]
fn grid_stretch_skips_a_fixed_row() {
    let doc = parse_and_layout(
        r#"
        <style>
          * { margin:0; padding:0 }
          .g { display:grid; grid-template-rows: 100px auto; height:400px; width:200px }
        </style>
        <div class="g"><div id="i1"></div><div id="i2" style="height:50px"></div></div>
    "#,
        1000.0,
    );
    let y = |id: &str| find_by_id(&doc.root, id).unwrap().layout.margin_rect.y;
    let h = |id: &str| find_by_id(&doc.root, id).unwrap().layout.margin_rect.h;
    assert!(
        (y("i2") - 100.0).abs() < 0.5,
        "the fixed row stays 100 tall, got y={}",
        y("i2")
    );
    assert!(
        (h("i1") - 100.0).abs() < 0.5,
        "i1 stretches to the 100px row, got {}",
        h("i1")
    );
}

/// **`align-content: center` must NOT stretch** — §12.7 runs only for
/// `normal`/`stretch`. Chrome: rows stay 30/50 and the pair is centred in 400.
#[test]
fn grid_align_content_center_does_not_stretch() {
    let doc = parse_and_layout(
        r#"
        <style>
          * { margin:0; padding:0 }
          .g { display:grid; grid-template-rows: auto auto; height:400px; width:200px;
               align-content: center }
        </style>
        <div class="g">
          <div id="h1" style="height:30px"></div>
          <div id="h2" style="height:50px"></div>
        </div>
    "#,
        1000.0,
    );
    let y = |id: &str| find_by_id(&doc.root, id).unwrap().layout.margin_rect.y;
    let h = |id: &str| find_by_id(&doc.root, id).unwrap().layout.margin_rect.h;
    assert!(
        (h("h1") - 30.0).abs() < 0.5,
        "centred content keeps its row height, got {}",
        h("h1")
    );
    assert!(
        (y("h2") - y("h1") - 30.0).abs() < 0.5,
        "rows stay adjacent, got {} {}",
        y("h1"),
        y("h2")
    );
    assert!(
        (y("h1") - 160.0).abs() < 0.5,
        "the 80px of rows is centred in 400, got y={}",
        y("h1")
    );
}

/// **§7.2.3 — a percentage ROW resolves against the grid's HEIGHT, never its
/// width.** The code passed the column measure, so `grid-template-rows: 50%` in
/// an auto-height grid became half the grid's WIDTH. With an indefinite height
/// the percentage contributes nothing to the container size (§11.1 step 2), so
/// Chrome's grid here is 80 tall — 30 + 50 — not 30 + half of 200.
#[test]
fn grid_percentage_row_resolves_against_height_not_width() {
    let doc = parse_and_layout(
        r#"
        <style>
          * { margin:0; padding:0 }
          .g { display:grid; grid-template-rows: 50% auto; width:200px }
        </style>
        <div class="g">
          <div id="f1" style="height:30px"></div>
          <div id="f2" style="height:50px"></div>
        </div>
    "#,
        1000.0,
    );
    let g = find_box(&doc.root, &|b: &WebCore| {
        b.attributes
            .get("class")
            .map(|c| c.split_whitespace().any(|w| w == "g"))
            .unwrap_or(false)
    })
    .unwrap();
    assert!(
        (g.layout.border_rect.h - 80.0).abs() < 0.5,
        "the percentage row contributes nothing to an indefinite height: Chrome gives 80, got {}",
        g.layout.border_rect.h
    );
    // Step 3 then resolves the percentage against that 80: row 1 becomes 40.
    let y = |id: &str| find_by_id(&doc.root, id).unwrap().layout.margin_rect.y;
    assert!(
        (y("f2") - 40.0).abs() < 0.5,
        "Chrome puts row 2 at y=40, got {}",
        y("f2")
    );
}

/// **A grid area spans the gutters between its tracks, including the extra a
/// `justify-content: space-*` value puts there.** The span width used the bare
/// `column-gap`, so a spanning item came up short by `extra_gap * (span-1)`.
/// Chrome on this fixture: the span-2 item is 175 wide (50 + 75 + 50).
#[test]
fn grid_a_spanning_item_covers_the_distributed_gap() {
    let doc = parse_and_layout(
        r#"
        <style>
          * { margin:0; padding:0 }
          .g { display:grid; grid-template-columns: 50px 50px 50px; width:300px;
               justify-content: space-between }
        </style>
        <div class="g">
          <div id="j1" style="grid-column:1/3; height:10px"></div>
          <div id="j2" style="grid-column:3; height:10px"></div>
        </div>
    "#,
        1000.0,
    );
    let w = |id: &str| find_by_id(&doc.root, id).unwrap().layout.border_rect.w;
    let x = |id: &str| find_by_id(&doc.root, id).unwrap().layout.border_rect.x;
    assert!(
        (w("j1") - 175.0).abs() < 0.5,
        "the span-2 area covers 50 + 75 of distributed gap + 50, got {}",
        w("j1")
    );
    assert!(
        (x("j2") - 250.0).abs() < 0.5,
        "the last column is flush right, got {}",
        x("j2")
    );
}

// ── Baseline alignment (CSS Box Alignment §9) ────────────────────────────────

/// **`align-items: baseline` must actually align baselines.** It was parsed and
/// then mapped to `0.0`, silently equivalent to `start`. Two items in one row
/// share a baseline: each is pushed down by the group's largest ascent minus
/// its own. Chrome on this fixture: the un-padded item sits 20px lower, exactly
/// the padding that lifted its neighbour's text.
#[test]
fn grid_align_items_baseline_aligns_the_baselines() {
    let doc = parse_and_layout(
        r#"
        <style>
          * { margin:0; padding:0 }
          .g { display:grid; grid-template-columns:auto auto; align-items:baseline;
               width:400px; font:16px/20px monospace }
        </style>
        <div class="g">
          <div id="k1" style="padding-top:20px">x</div>
          <div id="k2">x</div>
        </div>
    "#,
        1000.0,
    );
    let y = |id: &str| find_by_id(&doc.root, id).unwrap().layout.margin_rect.y;
    assert!(
        (y("k2") - y("k1") - 20.0).abs() < 0.5,
        "the unpadded item drops by its neighbour's 20px padding, got {} vs {}",
        y("k2"),
        y("k1")
    );
}

/// The row grows to hold the whole baseline group: max-ascent + max-descent,
/// which is taller than either item when they lean opposite ways.
/// Chrome on this fixture: the grid is 60 tall, not 40.
#[test]
fn grid_a_baseline_group_makes_its_row_tall_enough() {
    let doc = parse_and_layout(
        r#"
        <style>
          * { margin:0; padding:0 }
          .g { display:grid; grid-template-columns:auto auto; align-items:baseline;
               width:400px; font:16px/20px monospace }
        </style>
        <div class="g">
          <div id="l1" style="padding-top:20px">x</div>
          <div id="l2" style="padding-bottom:20px">x</div>
        </div>
    "#,
        1000.0,
    );
    let g = find_box(&doc.root, &|b: &WebCore| {
        b.attributes
            .get("class")
            .map(|c| c.split_whitespace().any(|w| w == "g"))
            .unwrap_or(false)
    })
    .unwrap();
    let y = |id: &str| find_by_id(&doc.root, id).unwrap().layout.margin_rect.y;
    assert!(
        (y("l2") - y("l1") - 20.0).abs() < 0.5,
        "l2 drops 20px onto l1's baseline, got {} vs {}",
        y("l2"),
        y("l1")
    );
    assert!(
        (g.layout.border_rect.h - 60.0).abs() < 0.5,
        "the row is max-ascent + max-descent = 60, got {}",
        g.layout.border_rect.h
    );
}

/// `align-self: baseline` on the items reaches the same code path.
#[test]
fn grid_align_self_baseline_aligns_one_item() {
    let doc = parse_and_layout(
        r#"
        <style>
          * { margin:0; padding:0 }
          .g { display:grid; grid-template-columns:auto auto; width:400px; font:16px/20px monospace }
        </style>
        <div class="g">
          <div id="m1" style="align-self:baseline; padding-top:20px">x</div>
          <div id="m2" style="align-self:baseline">x</div>
        </div>
    "#,
        1000.0,
    );
    let y = |id: &str| find_by_id(&doc.root, id).unwrap().layout.margin_rect.y;
    assert!(
        (y("m2") - y("m1") - 20.0).abs() < 0.5,
        "align-self:baseline drops m2 by 20, got {} vs {}",
        y("m2"),
        y("m1")
    );
}

/// **`fit-content(X)` is a CEILING on a track, never a floor.** On the row axis
/// the clamp was fed through `track_to_px` into the "raise the row to this"
/// branch, so `fit-content(200px)` FORCED a 50px row to 200px.
/// Chrome on this fixture: rows 50 / 30, grid 80 tall.
#[test]
fn grid_fit_content_row_does_not_raise_the_row() {
    let doc = parse_and_layout(
        r#"
        <style>
          * { margin:0; padding:0 }
          .g { display:grid; grid-template-rows: fit-content(200px) auto; width:200px }
        </style>
        <div class="g">
          <div id="n1" style="height:50px"></div>
          <div id="n2" style="height:30px"></div>
        </div>
    "#,
        1000.0,
    );
    let y = |id: &str| find_by_id(&doc.root, id).unwrap().layout.margin_rect.y;
    let g = find_box(&doc.root, &|b: &WebCore| {
        b.attributes
            .get("class")
            .map(|c| c.split_whitespace().any(|w| w == "g"))
            .unwrap_or(false)
    })
    .unwrap();
    assert!(
        (y("n2") - 50.0).abs() < 0.5,
        "the fit-content row keeps its 50px of content, got row 2 at y={}",
        y("n2")
    );
    assert!(
        (g.layout.border_rect.h - 80.0).abs() < 0.5,
        "Chrome gives an 80px grid, got {}",
        g.layout.border_rect.h
    );
}

/// …and it does not clamp a track below the content either: an item taller than
/// the limit keeps its height, because the track's MIN sizing function is
/// `auto`. Chrome on this fixture: rows 100 / 30.
#[test]
fn grid_fit_content_row_does_not_cut_off_taller_content() {
    let doc = parse_and_layout(
        r#"
        <style>
          * { margin:0; padding:0 }
          .g { display:grid; grid-template-rows: fit-content(40px) auto; width:200px }
        </style>
        <div class="g">
          <div id="o1" style="height:100px"></div>
          <div id="o2" style="height:30px"></div>
        </div>
    "#,
        1000.0,
    );
    let y = |id: &str| find_by_id(&doc.root, id).unwrap().layout.margin_rect.y;
    assert!(
        (y("o2") - 100.0).abs() < 0.5,
        "the 100px item sets the row, the 40px limit cannot cut it, got y={}",
        y("o2")
    );
}

/// `fit-content()` on a COLUMN: the track is the content width clamped by the
/// limit, and it takes no part in §12.7 stretch — the `auto` sibling takes all
/// the leftover. Chrome on this fixture: 50px / 350px.
#[test]
fn grid_fit_content_column_does_not_stretch() {
    let doc = parse_and_layout(
        r#"
        <style>
          * { margin:0; padding:0 }
          .g { display:grid; grid-template-columns: fit-content(200px) auto; width:400px }
        </style>
        <div class="g">
          <div id="p1" style="width:50px;height:10px"></div>
          <div id="p2" style="height:10px"></div>
        </div>
    "#,
        1000.0,
    );
    let w = |id: &str| find_by_id(&doc.root, id).unwrap().layout.border_rect.w;
    let x = |id: &str| find_by_id(&doc.root, id).unwrap().layout.border_rect.x;
    assert!(
        (x("p2") - 50.0).abs() < 0.5,
        "the fit-content column stays 50 wide, got {}",
        x("p2")
    );
    assert!(
        (w("p2") - 350.0).abs() < 0.5,
        "the auto column takes the other 350, got {}",
        w("p2")
    );
}

/// The same ceiling rule for `grid-auto-rows: fit-content(X)` on the implicit
/// rows. Chrome on this fixture: rows 50 / 30, grid 80 tall.
#[test]
fn grid_auto_rows_fit_content_does_not_raise_the_rows() {
    let doc = parse_and_layout(
        r#"
        <style>
          * { margin:0; padding:0 }
          .g { display:grid; grid-auto-rows: fit-content(200px); width:200px }
        </style>
        <div class="g">
          <div id="q1" style="height:50px"></div>
          <div id="q2" style="height:30px"></div>
        </div>
    "#,
        1000.0,
    );
    let y = |id: &str| find_by_id(&doc.root, id).unwrap().layout.margin_rect.y;
    assert!(
        (y("q2") - 50.0).abs() < 0.5,
        "the implicit fit-content rows keep their content heights, got y={}",
        y("q2")
    );
}
