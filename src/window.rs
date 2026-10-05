//! WHATWG HTML §7 — browsing contexts and the `Window` interface.
//!
//! A browser hands out windows, not just documents: `window.open()` creates a
//! browsing context AND its initial document, `document.defaultView` points
//! back, and `window.close()` ends both. Without this layer a document has no
//! context to live in, which is the same gap the other engine documents having
//! had.
//!
//! WHERE THIS DIFFERS FROM THE IDL, AND WHY
//!
//! In a browser `open` is a method on an existing `Window`, because the user
//! agent already made a tab before a byte of script ran. A toolkit has no tab —
//! it may legitimately have no window at all — so the FIRST `open` is a free
//! function. Everything after is standard.
//!
//! `innerWidth` and `innerHeight` are separate readonly attributes and are
//! spelled that way here, not bundled into one size call: they are two members
//! in the IDL and a caller may want either. Same for `screenX` / `screenY`.
//!
//! A VB form, a Pascal form and a Flutter window are all "a window": the form's
//! controls live in its document, the same context with different content.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use crate::dom::registry::{self, DocumentId};

/// A browsing context handle — what `open()` returns.
pub type WindowId = u64;

/// One browsing context: its document, its position, its lifecycle.
pub struct BrowsingContext {
    pub id: WindowId,
    /// `window.document`. The context holds the handle; the tree is the
    /// document's.
    pub document_id: DocumentId,
    /// `window.name`, the `target` of `open()`.
    pub name: String,
    /// `screenX` / `screenY`. Unlike the size, these have no document
    /// counterpart — a page cannot see where its window sits on screen.
    pub screen_x: f64,
    pub screen_y: f64,
    /// The host viewport. The document also stores layout dimensions, but
    /// layout may grow its height to the laid-out content. Window APIs report
    /// the viewport itself.
    pub viewport_w: f64,
    pub viewport_h: f64,
    /// `window.devicePixelRatio`.
    pub device_pixel_ratio: f64,
    /// `window.closed`.
    pub closed: bool,
}

/// `window.visualViewport`, projected from the layout viewport state this
/// engine already owns. There is no pinch-zoom viewport split yet, so `scale`
/// is `1` and visual/layout dimensions are the same.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VisualViewport {
    pub width: f64,
    pub height: f64,
    pub scale: f64,
    pub offset_left: f64,
    pub offset_top: f64,
    pub page_left: f64,
    pub page_top: f64,
}

#[derive(Default)]
struct Contexts {
    windows: HashMap<WindowId, BrowsingContext>,
    order: Vec<WindowId>,
    next_id: WindowId,
    /// `window.screen`. Read-only to script, so a host sets it once.
    screen: Option<(f64, f64)>,
}

fn contexts() -> &'static Mutex<Contexts> {
    static CTX: OnceLock<Mutex<Contexts>> = OnceLock::new();
    CTX.get_or_init(|| Mutex::new(Contexts::default()))
}

/// Parse a `windowFeatures` string — `"width=800,height=600,left=10"`.
///
/// The spec's own comma-separated `name=value` form, ignoring what it does not
/// know, exactly as a user agent does.
fn parse_features(features: &str) -> HashMap<String, f64> {
    let mut out = HashMap::new();
    for part in features.split(',') {
        let Some((k, v)) = part.split_once('=') else {
            continue;
        };
        if let Ok(n) = v.trim().parse::<f64>() {
            out.insert(k.trim().to_ascii_lowercase(), n);
        }
    }
    out
}

/// `window.open(url, target, features)` → a new browsing context with a fresh
/// document.
///
/// `url` is accepted and ignored: there is no navigation here, so every window
/// opens the spec's initial `about:blank`.
pub fn open(target: &str, features: &str) -> WindowId {
    let f = parse_features(features);
    let document_id = registry::new_document(target);
    let width = f.get("width").copied().unwrap_or(800.0);
    let height = f.get("height").copied().unwrap_or(600.0);
    // The viewport IS the window size — set once, read back from there.
    registry::with_document(document_id, |d| d.set_viewport(width as f32, height as f32));

    let mut ctx = match contexts().lock() {
        Ok(c) => c,
        Err(_) => return 0,
    };
    ctx.next_id += 1;
    let id = ctx.next_id;
    ctx.windows.insert(
        id,
        BrowsingContext {
            id,
            document_id,
            name: target.to_string(),
            screen_x: f.get("left").copied().unwrap_or(0.0),
            screen_y: f.get("top").copied().unwrap_or(0.0),
            viewport_w: width,
            viewport_h: height,
            device_pixel_ratio: 1.0,
            closed: false,
        },
    );
    ctx.order.push(id);
    id
}

/// Give an EXISTING document its top-level browsing context — the tab the user
/// agent already made.
///
/// `open()` creates a context AND a document, which is right for
/// `window.open()`. It cannot serve the AMBIENT document, the one a program
/// actually runs in, which exists before anything opens anything. In a browser
/// that document always sits in a top-level traversable because the tab
/// predates the script. Without this, `document.defaultView` is null for every
/// program and the main window cannot be named or closed.
///
/// Idempotent, and it must be: a second context over one document would be a
/// second tab showing the same page.
pub fn adopt(document_id: DocumentId, name: &str) -> WindowId {
    let (viewport_w, viewport_h) = registry::with_document(document_id, |doc| {
        (doc.viewport_w as f64, doc.viewport_h as f64)
    })
    .unwrap_or((
        registry::DEFAULT_VIEWPORT_WIDTH as f64,
        registry::DEFAULT_VIEWPORT_HEIGHT as f64,
    ));
    let mut ctx = match contexts().lock() {
        Ok(c) => c,
        Err(_) => return 0,
    };
    if let Some(existing) = ctx
        .windows
        .values()
        .find(|w| w.document_id == document_id)
        .map(|w| w.id)
    {
        return existing;
    }
    ctx.next_id += 1;
    let id = ctx.next_id;
    ctx.windows.insert(
        id,
        BrowsingContext {
            id,
            document_id,
            name: name.to_string(),
            screen_x: 0.0,
            screen_y: 0.0,
            viewport_w,
            viewport_h,
            device_pixel_ratio: 1.0,
            closed: false,
        },
    );
    ctx.order.push(id);
    id
}

/// Apply supported native window features to an existing initial context.
pub fn configure_features(id: WindowId, features: &str) {
    let features = parse_features(features);
    let width = features
        .get("width")
        .copied()
        .unwrap_or_else(|| inner_width(id));
    let height = features
        .get("height")
        .copied()
        .unwrap_or_else(|| inner_height(id));
    resize_to(id, width, height);
    let x = features
        .get("left")
        .copied()
        .unwrap_or_else(|| screen_x(id));
    let y = features.get("top").copied().unwrap_or_else(|| screen_y(id));
    move_to(id, x, y);
}

/// Borrow a browsing context.
pub fn with_window<T>(id: WindowId, f: impl FnOnce(&BrowsingContext) -> T) -> Option<T> {
    let ctx = contexts().lock().ok()?;
    ctx.windows.get(&id).map(f)
}

/// `window.document`.
pub fn document(id: WindowId) -> Option<DocumentId> {
    with_window(id, |w| w.document_id)
}

/// `document.defaultView` — the window a document is displayed in.
pub fn default_view(document_id: DocumentId) -> Option<WindowId> {
    let ctx = contexts().lock().ok()?;
    ctx.windows
        .values()
        .find(|w| w.document_id == document_id && !w.closed)
        .map(|w| w.id)
}

/// `window.name`.
pub fn name(id: WindowId) -> String {
    with_window(id, |w| w.name.clone()).unwrap_or_default()
}

/// `window.closed`. An unknown handle reads as closed — which is what a stale
/// reference to a gone window should say.
pub fn closed(id: WindowId) -> bool {
    with_window(id, |w| w.closed).unwrap_or(true)
}

/// `window.close()` — ends the context AND drops the document it was showing.
pub fn close(id: WindowId) {
    let document_id = {
        let mut ctx = match contexts().lock() {
            Ok(c) => c,
            Err(_) => return,
        };
        match ctx.windows.get_mut(&id) {
            Some(w) if !w.closed => {
                w.closed = true;
                w.document_id
            }
            _ => return,
        }
    };
    // Outside the lock: closing a window must not hold the context table while
    // the document table is taken.
    registry::close_document(document_id);
}

/// `window.focus()`. Brings the context to the front of the open list, which is
/// the only ordering a windowing host needs from here.
pub fn focus(id: WindowId) {
    if let Ok(mut ctx) = contexts().lock() {
        if let Some(pos) = ctx.order.iter().position(|w| *w == id) {
            let w = ctx.order.remove(pos);
            ctx.order.push(w);
        }
    }
}

/// `window.innerWidth` — the viewport width, read back off the document.
pub fn inner_width(id: WindowId) -> f64 {
    with_window(id, |w| if w.closed { 0.0 } else { w.viewport_w }).unwrap_or(0.0)
}

/// `window.innerHeight`.
pub fn inner_height(id: WindowId) -> f64 {
    with_window(id, |w| if w.closed { 0.0 } else { w.viewport_h }).unwrap_or(0.0)
}

/// `window.matchMedia(query)`.
pub fn match_media(id: WindowId, query: &str) -> Option<crate::dom::api::MediaQueryList> {
    document(id).and_then(|d| registry::with_document(d, |doc| doc.match_media(query)))
}

/// `window.resizeTo(width, height)`. Writes the document's viewport, which is
/// what `innerWidth`/`innerHeight` then report — one measurement, not two.
pub fn resize_to(id: WindowId, width: f64, height: f64) {
    update_viewport(id, width, height);
}

/// Update viewport state without synchronously laying out with a second engine.
/// The owning BrowserView consumes dirty geometry at its next frame boundary.
pub fn update_viewport(id: WindowId, width: f64, height: f64) {
    if let Ok(mut ctx) = contexts().lock() {
        if let Some(w) = ctx.windows.get_mut(&id) {
            if !w.closed {
                w.viewport_w = width;
                w.viewport_h = height;
            }
        }
    }
    if let Some(d) = document(id) {
        registry::with_document(d, |doc| {
            if doc.viewport_w != width as f32 || doc.viewport_h != height as f32 {
                crate::css::cascade::mark_layout_subtree_dirty(&mut doc.root);
                doc.viewport_w = width as f32;
                doc.viewport_h = height as f32;
            }
        });
    }
}

/// `window.devicePixelRatio`.
pub fn device_pixel_ratio(id: WindowId) -> f64 {
    with_window(id, |w| w.device_pixel_ratio).unwrap_or(1.0)
}

/// Tell the browser the device pixel ratio. Not an IDL setter — the attribute is
/// readonly to page script — this is the host informing the engine.
pub fn set_device_pixel_ratio(id: WindowId, ratio: f64) {
    if let Ok(mut ctx) = contexts().lock() {
        if let Some(w) = ctx.windows.get_mut(&id) {
            if ratio.is_finite() && ratio > 0.0 {
                w.device_pixel_ratio = ratio;
            }
        }
    }
}

/// `window.visualViewport`.
pub fn visual_viewport(id: WindowId) -> Option<VisualViewport> {
    let (document_id, closed, width, height) = with_window(id, |w| {
        (w.document_id, w.closed, w.viewport_w, w.viewport_h)
    })?;
    if closed {
        return None;
    }
    registry::with_document(document_id, |doc| VisualViewport {
        width,
        height,
        scale: 1.0,
        offset_left: 0.0,
        offset_top: 0.0,
        page_left: f64::from(doc.scroll_x),
        page_top: f64::from(doc.scroll_y),
    })
}

/// `window.screenX`.
pub fn screen_x(id: WindowId) -> f64 {
    with_window(id, |w| w.screen_x).unwrap_or(0.0)
}

/// `window.screenY`.
pub fn screen_y(id: WindowId) -> f64 {
    with_window(id, |w| w.screen_y).unwrap_or(0.0)
}

/// `window.moveTo(x, y)`.
pub fn move_to(id: WindowId, x: f64, y: f64) {
    if let Ok(mut ctx) = contexts().lock() {
        if let Some(w) = ctx.windows.get_mut(&id) {
            w.screen_x = x;
            w.screen_y = y;
        }
    }
}

/// `window.screen` — the display, as `(width, height)`.
///
/// Read-only to script, so a host sets it; until one does, the answer is the
/// window's own size, which is the honest floor for a toolkit that may have no
/// display information at all.
pub fn screen(id: WindowId) -> (f64, f64) {
    if let Ok(ctx) = contexts().lock() {
        if let Some(s) = ctx.screen {
            return s;
        }
    }
    (inner_width(id), inner_height(id))
}

/// Tell the browser how big the display is. Not an IDL member — `window.screen`
/// is read-only to a page — this is the host informing the engine.
pub fn set_screen(width: f64, height: f64) {
    if let Ok(mut ctx) = contexts().lock() {
        ctx.screen = Some((width, height));
    }
}

/// Every open browsing context, front-most last.
pub fn open_windows() -> Vec<WindowId> {
    match contexts().lock() {
        Ok(ctx) => ctx
            .order
            .iter()
            .copied()
            .filter(|id| ctx.windows.get(id).map(|w| !w.closed).unwrap_or(false))
            .collect(),
        Err(_) => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::BrowserDocument;

    #[test]
    fn adopted_context_preserves_live_document_and_viewport() {
        let mut doc = crate::parse_html("<body><div id='status'>Before</div></body>");
        doc.set_viewport(375.0, 240.0);
        let handle = BrowserDocument::new(doc);
        let document_id = registry::register_document(handle.clone());
        let context = adopt(document_id, "form");
        assert_eq!(adopt(document_id, "ignored"), context);
        assert_eq!(default_view(document_id), Some(context));
        assert_eq!(document(context), Some(document_id));
        assert_eq!(
            (inner_width(context), inner_height(context)),
            (375.0, 240.0)
        );

        registry::with_document(document_id, |doc| {
            let status = doc.get_element_by_id("status").unwrap();
            doc.set_text_content(status, "After");
        });
        let doc = handle.read();
        let status = doc.get_element_by_id("status").unwrap();
        assert_eq!(doc.text_content(status), "After");
        drop(doc);
        close(context);
    }

    #[test]
    fn resize_and_features_update_the_shared_document() {
        let handle = BrowserDocument::new(crate::parse_html("<body>Form</body>"));
        let document_id = registry::register_document(handle.clone());
        let context = adopt(document_id, "form");
        configure_features(context, "width=640,height=480,left=12,top=23");
        assert_eq!(
            (inner_width(context), inner_height(context)),
            (640.0, 480.0)
        );
        assert_eq!((screen_x(context), screen_y(context)), (12.0, 23.0));
        {
            let doc = handle.read();
            assert_eq!((doc.viewport_w, doc.viewport_h), (640.0, 480.0));
        }
        update_viewport(context, 320.0, 200.0);
        let viewport = visual_viewport(context).unwrap();
        assert_eq!((viewport.width, viewport.height), (320.0, 200.0));
        close(context);
    }

    #[test]
    fn closing_context_unregisters_document_but_retains_host_ownership() {
        let handle = BrowserDocument::new(crate::parse_html("<body>Form</body>"));
        let document_id = registry::register_document(handle.clone());
        let context = adopt(document_id, "form");
        close(context);
        close(context);
        assert!(closed(context));
        assert!(!registry::is_open(document_id));
        assert_eq!(default_view(document_id), None);
        assert!(!open_windows().contains(&context));
        assert!(visual_viewport(context).is_none());
        let doc = handle.read();
        assert!(doc.get_node(doc.root.node_id).is_some());
    }
}
