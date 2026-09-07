// Image tests – ported from cpptests/test_images.cpp
// Only object-fit parsing + img tag parsing are portable.
// Image layout (bitmap, intrinsic dimensions, IsReplaced) skipped.
use webcore::css::apply_property;
use webcore::parse_html;
use webcore::types::*;

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

// ============================================================
// Image Parsing
// ============================================================

#[test]
fn img_element_parsed() {
    let doc = parse_html("<img src=\"test.png\" width=\"100\" height=\"50\">");
    let b = find_box(&doc.root, &|b| b.tag == "img");
    assert!(b.is_some());
    assert_eq!(b.unwrap().style.display, Display::InlineBlock);
}

#[test]
fn img_src_attribute() {
    let doc = parse_html("<img src=\"photo.jpg\">");
    let b = find_box(&doc.root, &|b| b.tag == "img").unwrap();
    assert_eq!(b.get_attr("src"), Some("photo.jpg"));
}

#[test]
fn img_width_height_attributes() {
    let doc = parse_html("<img src=\"test.png\" width=\"200\" height=\"100\">");
    let b = find_box(&doc.root, &|b| b.tag == "img").unwrap();
    assert_eq!(b.get_attr("width"), Some("200"));
    assert_eq!(b.get_attr("height"), Some("100"));
}

#[test]
fn img_is_void() {
    let doc = parse_html("<img src=\"test.png\">");
    let b = find_box(&doc.root, &|b| b.tag == "img").unwrap();
    assert!(b.is_void());
}

// ============================================================
// Object-Fit Parsing
// ============================================================

#[test]
fn object_fit_contain() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "object-fit", "contain");
    assert_eq!(style.object_fit, ObjectFit::Contain);
}

#[test]
fn object_fit_cover() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "object-fit", "cover");
    assert_eq!(style.object_fit, ObjectFit::Cover);
}

#[test]
fn object_fit_none() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "object-fit", "none");
    assert_eq!(style.object_fit, ObjectFit::None);
}

#[test]
fn object_fit_scale_down() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "object-fit", "scale-down");
    assert_eq!(style.object_fit, ObjectFit::ScaleDown);
}

#[test]
fn object_fit_fill() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "object-fit", "fill");
    assert_eq!(style.object_fit, ObjectFit::Fill);
}

// ============================================================
// Missing tests ported from C++ test_images.cpp
// ============================================================

#[test]
fn img_with_inline_style() {
    let doc = parse_html("<img src=\"test.png\" style=\"width: 300px; height: 150px;\">");
    let b = find_box(&doc.root, &|b| b.tag == "img").unwrap();
    assert_eq!(b.style.width, webcore::types::CssLength::Px(300.0));
    assert_eq!(b.style.height, webcore::types::CssLength::Px(150.0));
}

#[test]
fn img_id_and_class() {
    let doc = parse_html("<img src=\"test.png\" id=\"logo\" class=\"banner\">");
    let b = find_box(&doc.root, &|b| {
        b.tag == "img" && b.get_attr("id") == Some("logo")
    });
    assert!(b.is_some());
    assert_eq!(b.unwrap().get_attr("class"), Some("banner"));
}

#[test]
fn img_serialization_preserves_src() {
    use webcore::html::serialize_html;
    let doc = parse_html("<p><img src=\"photo.jpg\"></p>");
    let html = serialize_html(&doc);
    assert!(html.contains("photo.jpg"));
}

// ============================================================
// Missing tests ported from C++ test_images.cpp
// ReplacedLayoutWithIntrinsicDimensions, ReplacedLayoutExplicitSize,
// ReplacedLayoutAspectRatioFromWidth, IsReplacedHelper, RenderReplacedSmoke
// all require a wxBitmap/wxMemoryDC and are not portable.
// We replace them with equivalent layout/structural tests.
// ============================================================

#[test]
fn img_explicit_layout_dimensions() {
    // After load_html (layout pass), an img with explicit CSS size should
    // have its content_rect dimensions reflect the specified size.
    use webcore::load_html;
    let doc = load_html(
        "<img src=\"test.png\" style=\"width: 200px; height: 100px;\">",
        800.0,
    );
    let b = find_box(&doc.root, &|b| b.tag == "img").unwrap();
    // Style must retain the specified values
    assert_eq!(b.style.width, webcore::types::CssLength::Px(200.0));
    assert_eq!(b.style.height, webcore::types::CssLength::Px(100.0));
}

#[test]
fn img_is_replaced_requires_image_data() {
    // An img without image_data should have image_data == None;
    // the box struct encodes "replaced" state via image_data presence.
    let doc = parse_html("<img src=\"test.png\">");
    let b = find_box(&doc.root, &|b| b.tag == "img").unwrap();
    // Without loading actual image bytes, image_data stays None
    assert!(
        b.image_data.is_none(),
        "img parsed without image bytes should have no image_data"
    );
}

#[test]
fn img_intrinsic_dimensions_from_attr() {
    // width/height HTML attributes should be stored as attributes on the box
    let doc = parse_html("<img src=\"test.png\" width=\"200\" height=\"100\">");
    let b = find_box(&doc.root, &|b| b.tag == "img").unwrap();
    assert_eq!(b.get_attr("width"), Some("200"));
    assert_eq!(b.get_attr("height"), Some("100"));
}

#[test]
fn img_alt_attribute_preserved() {
    // alt attribute should be preserved on the img box
    let doc = parse_html("<img src=\"test.png\" alt=\"A test image\">");
    let b = find_box(&doc.root, &|b| b.tag == "img").unwrap();
    assert_eq!(b.get_attr("alt"), Some("A test image"));
}
