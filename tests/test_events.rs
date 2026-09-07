// Tests for the event system in src/dom/mod.rs.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use webcore::dom::*;
use webcore::parse_html;
use webcore::types::*;

struct EventListeners {
    next_id: u32,
    entries: Vec<(
        u32,
        String,
        HtmlEventType,
        Box<dyn FnMut(&mut HtmlEvent, &mut WebCore)>,
    )>,
}

impl EventListeners {
    fn new() -> Self {
        Self {
            next_id: 1,
            entries: Vec::new(),
        }
    }

    fn add(
        &mut self,
        selector: &str,
        event_type: HtmlEventType,
        handler: Box<dyn FnMut(&mut HtmlEvent, &mut WebCore)>,
    ) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        self.entries
            .push((id, selector.to_string(), event_type, handler));
        id
    }

    fn remove(&mut self, id: u32) {
        self.entries.retain(|(entry_id, _, _, _)| *entry_id != id);
    }

    fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn dispatch(&mut self, root: &mut WebCore, mut evt: HtmlEvent) -> bool {
        let mut path = Vec::new();
        collect_path_ids(root, evt.target, &mut path);
        for target_id in path {
            for (_, selector, event_type, handler) in &mut self.entries {
                if *event_type == evt.event_type
                    && selector_matches_target(root, selector, target_id)
                {
                    handler(&mut evt, root);
                    if evt.propagation_stopped {
                        return evt.default_prevented;
                    }
                }
            }
        }
        evt.default_prevented
    }
}

fn collect_path_ids(node: &WebCore, target_id: u32, out: &mut Vec<u32>) -> bool {
    if node.node_id == target_id {
        out.push(node.node_id);
        return true;
    }
    for child in &node.children {
        if collect_path_ids(child, target_id, out) {
            out.push(node.node_id);
            return true;
        }
    }
    false
}

fn selector_matches_target(root: &WebCore, selector: &str, target_id: u32) -> bool {
    selector == "*"
        || query_selector_all(root, selector)
            .iter()
            .any(|node| node.node_id == target_id)
}

fn add_doc_listener(
    doc: &mut Document,
    selector: &str,
    event_type: HtmlEventType,
    handler: webcore::dom::events::EventHandler,
) {
    let ids: Vec<u32> = if selector == "*" {
        vec![doc.root.node_id]
    } else {
        query_selector_all(&doc.root, selector)
            .iter()
            .map(|node| node.node_id)
            .collect()
    };
    for id in ids {
        doc.add_event_listener(
            id,
            event_type.as_str(),
            handler_box_clone_unavailable(handler),
            webcore::dom::events::ListenerOptions::default(),
        );
        break;
    }
}

fn handler_box_clone_unavailable(
    handler: webcore::dom::events::EventHandler,
) -> webcore::dom::events::EventHandler {
    handler
}

#[test]
fn event_listener_registration() {
    let mut listeners = EventListeners::new();
    let id = listeners.add("#btn", HtmlEventType::Click, Box::new(|_, _| {}));
    assert!(id > 0);
    assert!(!listeners.is_empty());

    listeners.remove(id);
    assert!(listeners.is_empty());
}

#[test]
fn event_dispatch_bubbling() {
    let mut doc = parse_html(r#"<div id="parent"><p id="child">Click me</p></div>"#);
    let mut listeners = EventListeners::new();

    let parent_called = Arc::new(AtomicUsize::new(0));
    let child_called = Arc::new(AtomicUsize::new(0));

    let pc = parent_called.clone();
    listeners.add(
        "#parent",
        HtmlEventType::Click,
        Box::new(move |_, _| {
            pc.fetch_add(1, Ordering::SeqCst);
        }),
    );

    let cc = child_called.clone();
    listeners.add(
        "#child",
        HtmlEventType::Click,
        Box::new(move |_, _| {
            cc.fetch_add(1, Ordering::SeqCst);
        }),
    );

    let child_box = query_selector(&doc.root, "#child").unwrap();
    let mut evt = HtmlEvent::new(HtmlEventType::Click);
    evt.target = child_box.node_id;

    listeners.dispatch(&mut doc.root, evt);

    assert_eq!(child_called.load(Ordering::SeqCst), 1);
    assert_eq!(parent_called.load(Ordering::SeqCst), 1);
}

#[test]
fn event_stop_propagation() {
    let mut doc = parse_html(r#"<div id="parent"><p id="child">Click me</p></div>"#);
    let mut listeners = EventListeners::new();

    let parent_called = Arc::new(AtomicUsize::new(0));
    let pc = parent_called.clone();
    listeners.add(
        "#parent",
        HtmlEventType::Click,
        Box::new(move |_, _| {
            pc.fetch_add(1, Ordering::SeqCst);
        }),
    );

    listeners.add(
        "#child",
        HtmlEventType::Click,
        Box::new(|evt, _root| {
            evt.stop_propagation();
        }),
    );

    let child_box = query_selector(&doc.root, "#child").unwrap();
    let mut evt = HtmlEvent::new(HtmlEventType::Click);
    evt.target = child_box.node_id;

    listeners.dispatch(&mut doc.root, evt);

    assert_eq!(parent_called.load(Ordering::SeqCst), 0);
}

#[test]
fn event_prevent_default() {
    let mut doc = parse_html(r#"<div id="x">Test</div>"#);
    let mut listeners = EventListeners::new();

    listeners.add(
        "#x",
        HtmlEventType::Click,
        Box::new(|evt, _root| {
            evt.prevent_default();
        }),
    );

    let x_box = query_selector(&doc.root, "#x").unwrap();
    let mut evt = HtmlEvent::new(HtmlEventType::Click);
    evt.target = x_box.node_id;

    let prevented = listeners.dispatch(&mut doc.root, evt);
    assert!(prevented);
}

#[test]
fn event_selector_matching() {
    let mut doc = parse_html(r#"<div class="btn">A</div><div class="btn">B</div>"#);
    let mut listeners = EventListeners::new();

    let called_with = Arc::new(AtomicUsize::new(0));
    let cw = called_with.clone();

    listeners.add(
        ".btn",
        HtmlEventType::Click,
        Box::new(move |_, _| {
            cw.fetch_add(1, Ordering::SeqCst);
        }),
    );

    let div_ids: Vec<u32> = query_selector_all(&doc.root, ".btn")
        .iter()
        .map(|d| d.node_id)
        .collect();
    assert_eq!(div_ids.len(), 2);

    for nid in div_ids {
        let mut evt = HtmlEvent::new(HtmlEventType::Click);
        evt.target = nid;
        listeners.dispatch(&mut doc.root, evt);
    }

    assert_eq!(called_with.load(Ordering::SeqCst), 2);
}

// ─── Click fires on MouseUp (same target as MouseDown) ───────────────────────

#[test]
fn click_fires_on_mouseup_same_target() {
    let mut doc = webcore::load_html(
        r#"<div id="btn" style="width:100px;height:40px">Click</div>"#,
        400.0,
    );
    let click_count = Arc::new(AtomicUsize::new(0));
    let cc = click_count.clone();
    add_doc_listener(
        &mut doc,
        "#btn",
        HtmlEventType::Click,
        Box::new(move |_, _| {
            cc.fetch_add(1, Ordering::SeqCst);
        }),
    );
    doc.process_mouse_event(HtmlEventType::MouseDown, (50.0, 20.0), 0);
    doc.process_mouse_event(HtmlEventType::MouseUp, (50.0, 20.0), 0);
    assert_eq!(
        click_count.load(Ordering::SeqCst),
        1,
        "Click should fire once"
    );
}

#[test]
fn click_does_not_fire_on_different_target() {
    let mut doc = webcore::load_html(
        r#"<div id="a" style="width:100px;height:40px">A</div>
           <div id="b" style="width:100px;height:40px">B</div>"#,
        400.0,
    );
    let click_count = Arc::new(AtomicUsize::new(0));
    let cc = click_count.clone();
    add_doc_listener(
        &mut doc,
        "*",
        HtmlEventType::Click,
        Box::new(move |evt, _root| {
            evt.stop_propagation();
            cc.fetch_add(1, Ordering::SeqCst);
        }),
    );
    // Down on A, up on B — no click.
    doc.process_mouse_event(HtmlEventType::MouseDown, (50.0, 20.0), 0);
    doc.process_mouse_event(HtmlEventType::MouseUp, (50.0, 60.0), 0);
    assert_eq!(
        click_count.load(Ordering::SeqCst),
        0,
        "Click should not fire across targets"
    );
}

// ─── client_pos is set correctly ─────────────────────────────────────────────

#[test]
fn client_pos_set_on_events() {
    let mut doc = webcore::load_html(
        r#"<div id="box" style="width:200px;height:100px">Box</div>"#,
        400.0,
    );
    let got_pos = Arc::new(std::sync::Mutex::new((0.0f32, 0.0f32)));
    let gp = got_pos.clone();
    add_doc_listener(
        &mut doc,
        "#box",
        HtmlEventType::MouseMove,
        Box::new(move |evt, _root| {
            *gp.lock().unwrap() = (evt.client_x, evt.client_y);
        }),
    );
    doc.process_mouse_event(HtmlEventType::MouseMove, (80.0, 40.0), 0);
    let pos = *got_pos.lock().unwrap();
    assert!(
        (pos.0 - 80.0).abs() < 1.0,
        "client_pos.x should be ~80, got {}",
        pos.0
    );
    assert!(
        (pos.1 - 40.0).abs() < 1.0,
        "client_pos.y should be ~40, got {}",
        pos.1
    );
}

// ─── DblClick fires on two clicks within 400ms ───────────────────────────────

#[test]
fn dblclick_fires_within_400ms() {
    let mut doc = webcore::load_html(
        r#"<div id="btn" style="width:100px;height:40px">Dbl</div>"#,
        400.0,
    );
    let dbl_count = Arc::new(AtomicUsize::new(0));
    let click_count = Arc::new(AtomicUsize::new(0));
    let dc = dbl_count.clone();
    let cc = click_count.clone();
    add_doc_listener(
        &mut doc,
        "#btn",
        HtmlEventType::DblClick,
        Box::new(move |_, _| {
            dc.fetch_add(1, Ordering::SeqCst);
        }),
    );
    add_doc_listener(
        &mut doc,
        "#btn",
        HtmlEventType::Click,
        Box::new(move |_, _| {
            cc.fetch_add(1, Ordering::SeqCst);
        }),
    );
    // First click
    doc.process_mouse_event(HtmlEventType::MouseDown, (50.0, 20.0), 0);
    doc.process_mouse_event(HtmlEventType::MouseUp, (50.0, 20.0), 0);
    // Second click immediately after (within 400ms in tests)
    doc.process_mouse_event(HtmlEventType::MouseDown, (50.0, 20.0), 0);
    doc.process_mouse_event(HtmlEventType::MouseUp, (50.0, 20.0), 0);
    assert_eq!(
        dbl_count.load(Ordering::SeqCst),
        1,
        "DblClick should fire once"
    );
    assert_eq!(
        click_count.load(Ordering::SeqCst),
        2,
        "Click should fire on both ups"
    );
}

#[test]
fn dblclick_does_not_fire_on_different_targets() {
    let mut doc = webcore::load_html(
        r#"<div id="a" style="width:100px;height:40px">A</div>
           <div id="b" style="width:100px;height:40px">B</div>"#,
        400.0,
    );
    let dbl_count = Arc::new(AtomicUsize::new(0));
    let dc = dbl_count.clone();
    add_doc_listener(
        &mut doc,
        "*",
        HtmlEventType::DblClick,
        Box::new(move |_, _| {
            dc.fetch_add(1, Ordering::SeqCst);
        }),
    );
    doc.process_mouse_event(HtmlEventType::MouseDown, (50.0, 20.0), 0);
    doc.process_mouse_event(HtmlEventType::MouseUp, (50.0, 20.0), 0);
    doc.process_mouse_event(HtmlEventType::MouseDown, (50.0, 60.0), 0); // different element
    doc.process_mouse_event(HtmlEventType::MouseUp, (50.0, 60.0), 0);
    assert_eq!(
        dbl_count.load(Ordering::SeqCst),
        0,
        "DblClick should not fire on different targets"
    );
}

// ─── Drag state machine ───────────────────────────────────────────────────────

#[test]
fn drag_fires_after_threshold() {
    let mut doc = webcore::load_html(
        r#"<div id="card" style="width:200px;height:100px">Drag me</div>"#,
        400.0,
    );
    let start_count = Arc::new(AtomicUsize::new(0));
    let drag_count = Arc::new(AtomicUsize::new(0));
    let end_count = Arc::new(AtomicUsize::new(0));
    let sc = start_count.clone();
    let dc = drag_count.clone();
    let ec = end_count.clone();
    add_doc_listener(
        &mut doc,
        "#card",
        HtmlEventType::DragStart,
        Box::new(move |_, _| {
            sc.fetch_add(1, Ordering::SeqCst);
        }),
    );
    add_doc_listener(
        &mut doc,
        "#card",
        HtmlEventType::Drag,
        Box::new(move |_, _| {
            dc.fetch_add(1, Ordering::SeqCst);
        }),
    );
    add_doc_listener(
        &mut doc,
        "#card",
        HtmlEventType::DragEnd,
        Box::new(move |_, _| {
            ec.fetch_add(1, Ordering::SeqCst);
        }),
    );
    doc.process_mouse_event(HtmlEventType::MouseDown, (100.0, 50.0), 0);
    // Small move — below threshold, no DragStart yet.
    doc.process_mouse_event(HtmlEventType::MouseMove, (101.0, 50.0), 0);
    assert_eq!(
        start_count.load(Ordering::SeqCst),
        0,
        "DragStart should not fire below threshold"
    );
    // Move past 5px threshold.
    doc.process_mouse_event(HtmlEventType::MouseMove, (110.0, 50.0), 0);
    assert_eq!(
        start_count.load(Ordering::SeqCst),
        1,
        "DragStart should fire past threshold"
    );
    assert_eq!(
        drag_count.load(Ordering::SeqCst),
        1,
        "Drag should fire on same move as DragStart"
    );
    // Another move — only Drag fires.
    doc.process_mouse_event(HtmlEventType::MouseMove, (120.0, 50.0), 0);
    assert_eq!(
        start_count.load(Ordering::SeqCst),
        1,
        "DragStart fires exactly once"
    );
    assert_eq!(
        drag_count.load(Ordering::SeqCst),
        2,
        "Drag should fire on each move"
    );
    // Release.
    doc.process_mouse_event(HtmlEventType::MouseUp, (120.0, 50.0), 0);
    assert_eq!(
        end_count.load(Ordering::SeqCst),
        1,
        "DragEnd should fire on release"
    );
}

#[test]
fn click_suppressed_when_drag_occurred() {
    let mut doc = webcore::load_html(
        r#"<div id="card" style="width:200px;height:100px">Drag me</div>"#,
        400.0,
    );
    let click_count = Arc::new(AtomicUsize::new(0));
    let cc = click_count.clone();
    add_doc_listener(
        &mut doc,
        "#card",
        HtmlEventType::Click,
        Box::new(move |_, _| {
            cc.fetch_add(1, Ordering::SeqCst);
        }),
    );
    doc.process_mouse_event(HtmlEventType::MouseDown, (100.0, 50.0), 0);
    doc.process_mouse_event(HtmlEventType::MouseMove, (110.0, 50.0), 0); // past threshold
    doc.process_mouse_event(HtmlEventType::MouseUp, (110.0, 50.0), 0);
    assert_eq!(
        click_count.load(Ordering::SeqCst),
        0,
        "Click should be suppressed after drag"
    );
}

// ─── MouseEnter / MouseLeave are non-bubbling ─────────────────────────────────

#[test]
fn mouseenter_does_not_bubble() {
    let mut doc = webcore::load_html(
        r#"<div id="a" style="width:100px;height:50px">A</div>
           <div id="b" style="width:100px;height:50px">
             <div id="child" style="width:80px;height:40px">child</div>
           </div>"#,
        400.0,
    );
    let parent_enter = Arc::new(AtomicUsize::new(0));
    let child_enter = Arc::new(AtomicUsize::new(0));
    let pe = parent_enter.clone();
    let ce = child_enter.clone();
    add_doc_listener(
        &mut doc,
        "#b",
        HtmlEventType::MouseEnter,
        Box::new(move |_, _| {
            pe.fetch_add(1, Ordering::SeqCst);
        }),
    );
    add_doc_listener(
        &mut doc,
        "#child",
        HtmlEventType::MouseEnter,
        Box::new(move |_, _| {
            ce.fetch_add(1, Ordering::SeqCst);
        }),
    );
    // Start on A so hovered_box is set to A, then move to child inside B.
    doc.process_mouse_event(HtmlEventType::MouseMove, (50.0, 25.0), 0); // hover A
    doc.dispatch_over_out((40.0, 70.0)); // move to child inside B
                                         // Child should get MouseEnter; #b should NOT (non-bubbling means parent doesn't get child's Enter).
    assert_eq!(
        child_enter.load(Ordering::SeqCst),
        1,
        "Child should get MouseEnter"
    );
    assert_eq!(
        parent_enter.load(Ordering::SeqCst),
        0,
        "Parent #b should not get MouseEnter from child (non-bubbling)"
    );
}

#[test]
fn mouseleave_does_not_bubble() {
    let mut doc = webcore::load_html(
        r#"<div id="a" style="width:100px;height:50px">A</div>
           <div id="b" style="width:100px;height:50px">B</div>"#,
        400.0,
    );
    let a_leave = Arc::new(AtomicUsize::new(0));
    let parent_leave = Arc::new(AtomicUsize::new(0));
    let al = a_leave.clone();
    let pl = parent_leave.clone();
    add_doc_listener(
        &mut doc,
        "#a",
        HtmlEventType::MouseLeave,
        Box::new(move |_, _| {
            al.fetch_add(1, Ordering::SeqCst);
        }),
    );
    // body is ancestor of #a; MouseLeave should not bubble to it.
    add_doc_listener(
        &mut doc,
        "body",
        HtmlEventType::MouseLeave,
        Box::new(move |_, _| {
            pl.fetch_add(1, Ordering::SeqCst);
        }),
    );
    doc.process_mouse_event(HtmlEventType::MouseMove, (50.0, 25.0), 0);
    doc.dispatch_over_out((50.0, 75.0)); // move from A to B
    assert_eq!(
        a_leave.load(Ordering::SeqCst),
        1,
        "MouseLeave should fire on A"
    );
    assert_eq!(
        parent_leave.load(Ordering::SeqCst),
        0,
        "MouseLeave should not bubble to body"
    );
}

// ─── MouseOver / MouseOut via dispatch_over_out ───────────────────────────────

#[test]
fn mouseover_mouseout_dispatch_over_out() {
    // Two stacked divs: A occupies y 0-50, B occupies y 50-100.
    let mut doc = webcore::load_html(
        r#"<div id="a" style="width:100px;height:50px">A</div><div id="b" style="width:100px;height:50px">B</div>"#,
        400.0,
    );

    let over_count = Arc::new(AtomicUsize::new(0));
    let out_count = Arc::new(AtomicUsize::new(0));
    let oc = over_count.clone();
    let outc = out_count.clone();

    add_doc_listener(
        &mut doc,
        "#b",
        HtmlEventType::MouseOver,
        Box::new(move |_, _| {
            oc.fetch_add(1, Ordering::SeqCst);
        }),
    );
    add_doc_listener(
        &mut doc,
        "#a",
        HtmlEventType::MouseOut,
        Box::new(move |_, _| {
            outc.fetch_add(1, Ordering::SeqCst);
        }),
    );

    // Move onto A first so hovered_box is set to the A element.
    doc.process_mouse_event(HtmlEventType::MouseMove, (50.0, 25.0), 0);
    // Now move to B — should fire MouseOut on A and MouseOver on B.
    doc.dispatch_over_out((50.0, 75.0));

    assert_eq!(
        over_count.load(Ordering::SeqCst),
        1,
        "MouseOver on B should fire once"
    );
    assert_eq!(
        out_count.load(Ordering::SeqCst),
        1,
        "MouseOut on A should fire once"
    );
}

// ─── PointerOver / PointerOut via dispatch_over_out ──────────────────────────

#[test]
fn pointerover_pointerout_dispatch_over_out() {
    let mut doc = webcore::load_html(
        r#"<div id="a" style="width:100px;height:50px">A</div><div id="b" style="width:100px;height:50px">B</div>"#,
        400.0,
    );

    let pover_count = Arc::new(AtomicUsize::new(0));
    let pout_count = Arc::new(AtomicUsize::new(0));
    let poc = pover_count.clone();
    let potc = pout_count.clone();

    add_doc_listener(
        &mut doc,
        "#b",
        HtmlEventType::PointerOver,
        Box::new(move |_, _| {
            poc.fetch_add(1, Ordering::SeqCst);
        }),
    );
    add_doc_listener(
        &mut doc,
        "#a",
        HtmlEventType::PointerOut,
        Box::new(move |_, _| {
            potc.fetch_add(1, Ordering::SeqCst);
        }),
    );

    // Hover A first, then move to B.
    doc.process_mouse_event(HtmlEventType::MouseMove, (50.0, 25.0), 0);
    doc.dispatch_over_out((50.0, 75.0));

    assert_eq!(
        pover_count.load(Ordering::SeqCst),
        1,
        "PointerOver on B should fire once"
    );
    assert_eq!(
        pout_count.load(Ordering::SeqCst),
        1,
        "PointerOut on A should fire once"
    );
}

// ─── FocusIn / FocusOut via process_mouse_event(MouseDown) ───────────────────

#[test]
fn focusin_focusout_on_mouse_down_focus_change() {
    let mut doc = webcore::load_html(
        r#"<div id="a" tabindex="0" style="width:100px;height:50px">A</div><div id="b" tabindex="0" style="width:100px;height:50px">B</div>"#,
        400.0,
    );

    let focusin_fired = Arc::new(AtomicBool::new(false));
    let focusout_fired = Arc::new(AtomicBool::new(false));
    let fif = focusin_fired.clone();
    let fof = focusout_fired.clone();

    // FocusIn bubbles — stop propagation so it registers exactly once per dispatch.
    add_doc_listener(
        &mut doc,
        "*",
        HtmlEventType::FocusIn,
        Box::new(move |evt, _root| {
            fif.store(true, Ordering::SeqCst);
            evt.stop_propagation();
        }),
    );
    add_doc_listener(
        &mut doc,
        "*",
        HtmlEventType::FocusOut,
        Box::new(move |evt, _root| {
            fof.store(true, Ordering::SeqCst);
            evt.stop_propagation();
        }),
    );

    // Click A — gives it focus (no previous focus, so only FocusIn should fire).
    doc.process_mouse_event(HtmlEventType::MouseDown, (50.0, 25.0), 0);
    assert!(
        focusin_fired.load(Ordering::SeqCst),
        "FocusIn should fire when A gains focus"
    );
    assert!(
        !focusout_fired.load(Ordering::SeqCst),
        "FocusOut should not fire when there was no previous focus"
    );

    // Reset flags.
    focusin_fired.store(false, Ordering::SeqCst);

    // Click B — focus moves from A to B: FocusOut on A then FocusIn on B.
    doc.process_mouse_event(HtmlEventType::MouseDown, (50.0, 75.0), 0);
    assert!(
        focusin_fired.load(Ordering::SeqCst),
        "FocusIn should fire when B gains focus"
    );
    assert!(
        focusout_fired.load(Ordering::SeqCst),
        "FocusOut should fire when A loses focus"
    );
}

// ─── PointerDown / PointerUp / PointerMove via process_mouse_event ───────────

#[test]
fn pointer_down_up_move_events() {
    let mut doc = webcore::load_html(
        r#"<div id="box" style="width:200px;height:100px">Box</div>"#,
        400.0,
    );

    let down_count = Arc::new(AtomicUsize::new(0));
    let up_count = Arc::new(AtomicUsize::new(0));
    let move_count = Arc::new(AtomicUsize::new(0));
    let dc = down_count.clone();
    let uc = up_count.clone();
    let mc = move_count.clone();

    add_doc_listener(
        &mut doc,
        "#box",
        HtmlEventType::PointerDown,
        Box::new(move |_, _| {
            dc.fetch_add(1, Ordering::SeqCst);
        }),
    );
    add_doc_listener(
        &mut doc,
        "#box",
        HtmlEventType::PointerUp,
        Box::new(move |_, _| {
            uc.fetch_add(1, Ordering::SeqCst);
        }),
    );
    add_doc_listener(
        &mut doc,
        "#box",
        HtmlEventType::PointerMove,
        Box::new(move |_, _| {
            mc.fetch_add(1, Ordering::SeqCst);
        }),
    );

    doc.process_mouse_event(HtmlEventType::PointerDown, (100.0, 50.0), 0);
    doc.process_mouse_event(HtmlEventType::PointerMove, (110.0, 50.0), 0);
    doc.process_mouse_event(HtmlEventType::PointerUp, (110.0, 50.0), 0);

    assert_eq!(
        down_count.load(Ordering::SeqCst),
        1,
        "PointerDown should fire once"
    );
    assert_eq!(
        up_count.load(Ordering::SeqCst),
        1,
        "PointerUp should fire once"
    );
    assert_eq!(
        move_count.load(Ordering::SeqCst),
        1,
        "PointerMove should fire once"
    );
}

// ─── Wheel event dispatched manually ─────────────────────────────────────────

#[test]
fn wheel_event_dispatch() {
    let mut doc = parse_html(r#"<div id="scroll-area">content</div>"#);

    let wheel_count = Arc::new(AtomicUsize::new(0));
    let wc = wheel_count.clone();

    add_doc_listener(
        &mut doc,
        "#scroll-area",
        HtmlEventType::Wheel,
        Box::new(move |_, _| {
            wc.fetch_add(1, Ordering::SeqCst);
        }),
    );

    let scroll_box = query_selector(&doc.root, "#scroll-area").unwrap();
    let mut evt =
        webcore::dom::events::DomEvent::new(HtmlEventType::Wheel.as_str(), scroll_box.node_id);
    doc.dispatch_event(&mut evt);

    assert_eq!(
        wheel_count.load(Ordering::SeqCst),
        1,
        "Wheel event should fire once"
    );
}

// ─── DOMContentLoaded event dispatched manually ───────────────────────────────

#[test]
fn dom_content_loaded_event_dispatch() {
    let mut doc = parse_html("<p>hello</p>");

    let fired = Arc::new(AtomicBool::new(false));
    let f = fired.clone();

    add_doc_listener(
        &mut doc,
        "*",
        HtmlEventType::DOMContentLoaded,
        Box::new(move |_, _| {
            f.store(true, Ordering::SeqCst);
        }),
    );

    let mut evt = webcore::dom::events::DomEvent::new(
        HtmlEventType::DOMContentLoaded.as_str(),
        doc.root.node_id,
    );
    doc.dispatch_event(&mut evt);

    assert!(
        fired.load(Ordering::SeqCst),
        "DOMContentLoaded should have fired"
    );
}

// ─── Resize event dispatched manually ────────────────────────────────────────

#[test]
fn resize_event_dispatch() {
    let mut doc = parse_html("<body><p>page</p></body>");

    let fired = Arc::new(AtomicBool::new(false));
    let f = fired.clone();

    add_doc_listener(
        &mut doc,
        "*",
        HtmlEventType::Resize,
        Box::new(move |_, _| {
            f.store(true, Ordering::SeqCst);
        }),
    );

    let mut evt =
        webcore::dom::events::DomEvent::new(HtmlEventType::Resize.as_str(), doc.root.node_id);
    doc.dispatch_event(&mut evt);

    assert!(
        fired.load(Ordering::SeqCst),
        "Resize event should have fired"
    );
}
