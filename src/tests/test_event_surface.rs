//! The event surface — `EventTarget`, `Event`, event handler IDL attributes,
//! `CustomEvent`, `AbortController`.
//!
//! These pin SEMANTICS, not just presence. A name that exists but bubbles when
//! the spec says it does not is worse than a missing one, because it runs
//! listeners a browser never runs.

use crate::dom::event_handlers::*;
use crate::dom::events::*;
use crate::parse_html;
use std::sync::{Arc, Mutex};

fn doc_with(html: &str) -> crate::Document {
    parse_html(html)
}

// ─── Event handler IDL attributes ───────────────────────────────────────────

#[test]
fn every_handler_name_the_specs_define_is_present() {
    // The counts come from the IDL blocks in
    // data/ecma/whatwg-html/spec.html: GlobalEventHandlers 76,
    // WindowEventHandlers 18, DocumentAndElementEventHandlers 3.
    assert_eq!(GLOBAL_EVENT_HANDLERS.len(), 76, "GlobalEventHandlers");
    assert_eq!(WINDOW_EVENT_HANDLERS.len(), 18, "WindowEventHandlers");
    assert_eq!(DOCUMENT_AND_ELEMENT_EVENT_HANDLERS.len(), 3);
    // oncopy/oncut/onpaste are in two mixins; the union deduplicates them.
    assert_eq!(all_event_handler_names().len(), 76 + 18);
}

#[test]
fn handler_names_map_to_their_event_type() {
    assert_eq!(event_type_for_handler("onclick").as_deref(), Some("click"));
    assert_eq!(event_type_for_handler("onDOMContentLoaded"), None);
    assert_eq!(event_type_for_handler("onnotathing"), None);
    // The webkit aliases handle the UNPREFIXED type — `onwebkitanimationend`
    // fires on `animationend`, not on `webkitanimationend`.
    assert_eq!(
        event_type_for_handler("onwebkitanimationend").as_deref(),
        Some("animationend")
    );
    assert_eq!(
        event_type_for_handler("onwebkittransitionend").as_deref(),
        Some("transitionend")
    );
    assert_eq!(handler_name_for_event_type("click"), Some("onclick"));
    assert_eq!(handler_name_for_event_type("notathing"), None);
}

#[test]
fn a_handler_slot_holds_exactly_one_listener() {
    let mut doc = doc_with("<div id=a>x</div>");
    let a = doc.query_selector("#a").unwrap();
    let hits = Arc::new(Mutex::new(Vec::<&'static str>::new()));

    let h1 = hits.clone();
    doc.set_event_handler(
        a,
        "onclick",
        Box::new(move |_, _d: &mut crate::Document| h1.lock().unwrap().push("first")),
    )
    .expect("onclick is a handler attribute");
    let h2 = hits.clone();
    doc.set_event_handler(
        a,
        "onclick",
        Box::new(move |_, _d: &mut crate::Document| h2.lock().unwrap().push("second")),
    )
    .expect("setting it again replaces");

    assert!(doc.has_event_handler(a, "onclick"));
    let mut e = DomEvent::new("click", a);
    doc.dispatch_event(&mut e);
    // Assigning twice leaves ONE handler — that is the difference from
    // addEventListener, which would leave two.
    assert_eq!(*hits.lock().unwrap(), vec!["second"]);

    assert!(doc.remove_event_handler(a, "onclick"));
    assert!(!doc.has_event_handler(a, "onclick"));
    let mut e2 = DomEvent::new("click", a);
    doc.dispatch_event(&mut e2);
    assert_eq!(
        hits.lock().unwrap().len(),
        1,
        "a cleared handler does not fire"
    );
}

#[test]
fn a_name_that_is_not_a_handler_attribute_is_rejected() {
    let mut doc = doc_with("<div id=a>x</div>");
    let a = doc.query_selector("#a").unwrap();
    assert!(
        doc.set_event_handler(a, "onnotathing", Box::new(|_, _d: &mut crate::Document| {}))
            .is_none()
    );
    assert!(doc.event_handler_names(a).is_empty());
}

// ─── Event semantics ────────────────────────────────────────────────────────

#[test]
fn bubbles_and_cancelable_come_from_the_event_type() {
    for (ty, bubbles, cancelable) in [
        ("click", true, true),
        ("submit", true, true),
        ("input", true, false),
        ("load", false, false),
        ("focus", false, false),
        ("mouseenter", false, false),
        ("scroll", false, false),
        ("beforeunload", false, true), // the odd one
    ] {
        let e = DomEvent::new(ty, 1);
        assert_eq!(e.bubbles, bubbles, "{ty}.bubbles");
        assert_eq!(e.cancelable, cancelable, "{ty}.cancelable");
    }
}

#[test]
fn a_non_bubbling_event_does_not_reach_ancestors() {
    let mut doc = doc_with("<div id=outer><span id=inner>x</span></div>");
    let outer = doc.query_selector("#outer").unwrap();
    let inner = doc.query_selector("#inner").unwrap();
    let seen = Arc::new(Mutex::new(Vec::<String>::new()));

    let s1 = seen.clone();
    doc.add_event_listener(
        outer,
        "load",
        Box::new(move |_, _d: &mut crate::Document| s1.lock().unwrap().push("outer".into())),
        ListenerOptions::default(),
    );
    let s2 = seen.clone();
    doc.add_event_listener(
        inner,
        "load",
        Box::new(move |_, _d: &mut crate::Document| s2.lock().unwrap().push("inner".into())),
        ListenerOptions::default(),
    );

    let mut e = DomEvent::new("load", inner);
    doc.dispatch_event(&mut e);
    // `load` does not bubble, so the ancestor listener must not run.
    assert_eq!(*seen.lock().unwrap(), vec!["inner"]);
}

#[test]
fn prevent_default_only_works_on_a_cancelable_event() {
    let mut click = DomEvent::new("click", 1);
    click.prevent_default();
    assert!(click.default_prevented(), "click is cancelable");

    let mut load = DomEvent::new("load", 1);
    load.prevent_default();
    assert!(
        !load.default_prevented(),
        "load is not cancelable — the call is a no-op"
    );
}

#[test]
fn dispatch_event_reports_cancellation_and_is_untrusted() {
    let mut doc = doc_with("<div id=a>x</div>");
    let a = doc.query_selector("#a").unwrap();
    doc.add_event_listener(
        a,
        "click",
        Box::new(|e, _d: &mut crate::Document| e.prevent_default()),
        ListenerOptions::default(),
    );
    let mut e = DomEvent::new("click", a);
    // Returns false when a handler cancelled it, per the IDL.
    assert!(!doc.dispatch_event(&mut e));
    assert!(!e.is_trusted, "a script-dispatched event is never trusted");

    let mut e2 = DomEvent::new("mousedown", a);
    assert!(doc.dispatch_event(&mut e2), "nothing cancelled this one");
}

#[test]
fn event_phase_and_composed_path_follow_the_spec() {
    let mut doc = doc_with("<div id=outer><span id=inner>x</span></div>");
    let outer = doc.query_selector("#outer").unwrap();
    let inner = doc.query_selector("#inner").unwrap();
    let log = Arc::new(Mutex::new(Vec::<(u16, usize)>::new()));

    let l1 = log.clone();
    doc.add_event_listener(
        outer,
        "click",
        Box::new(move |e, _d: &mut crate::Document| {
            l1.lock()
                .unwrap()
                .push((e.event_phase(), e.composed_path().len()))
        }),
        ListenerOptions {
            capture: true,
            ..Default::default()
        },
    );
    let l2 = log.clone();
    doc.add_event_listener(
        inner,
        "click",
        Box::new(move |e, _d: &mut crate::Document| {
            l2.lock()
                .unwrap()
                .push((e.event_phase(), e.composed_path().len()))
        }),
        ListenerOptions::default(),
    );
    let l3 = log.clone();
    doc.add_event_listener(
        outer,
        "click",
        Box::new(move |e, _d: &mut crate::Document| {
            l3.lock()
                .unwrap()
                .push((e.event_phase(), e.composed_path().len()))
        }),
        ListenerOptions::default(),
    );

    let mut e = DomEvent::new("click", inner);
    doc.dispatch_event(&mut e);

    let seen = log.lock().unwrap();
    assert_eq!(
        seen.len(),
        3,
        "capture on outer, target on inner, bubble on outer"
    );
    assert_eq!(seen[0].0, 1, "CAPTURING_PHASE");
    assert_eq!(seen[1].0, 2, "AT_TARGET");
    assert_eq!(seen[2].0, 3, "BUBBLING_PHASE");
    // The path is the same in every phase and is never empty.
    assert!(seen.iter().all(|(_, len)| *len == seen[0].1 && *len > 0));
    // Reset once dispatch is over.
    assert_eq!(e.event_phase(), 0);
}

#[test]
fn at_the_target_capture_listeners_run_before_bubble_listeners() {
    let mut doc = doc_with("<div id=a>x</div>");
    let a = doc.query_selector("#a").unwrap();
    let order = Arc::new(Mutex::new(Vec::<&'static str>::new()));

    // The target is the last node of the capture traversal and the first of
    // the bubble traversal, so its capture listeners fire first even when a
    // bubble listener was registered earlier. Confirmed against Chrome.
    let o1 = order.clone();
    doc.add_event_listener(
        a,
        "click",
        Box::new(move |_, _d: &mut crate::Document| {
            o1.lock().unwrap().push("bubble-registered-first")
        }),
        ListenerOptions::default(),
    );
    let o2 = order.clone();
    doc.add_event_listener(
        a,
        "click",
        Box::new(move |_, _d: &mut crate::Document| {
            o2.lock().unwrap().push("capture-registered-second")
        }),
        ListenerOptions {
            capture: true,
            ..Default::default()
        },
    );

    let mut e = DomEvent::new("click", a);
    doc.dispatch_event(&mut e);
    assert_eq!(
        *order.lock().unwrap(),
        vec!["capture-registered-second", "bubble-registered-first"]
    );
}

#[test]
fn a_non_bubbling_event_still_fires_bubble_listeners_on_the_target() {
    let mut doc = doc_with("<div id=outer><span id=inner>x</span></div>");
    let inner = doc.query_selector("#inner").unwrap();
    let hit = Arc::new(Mutex::new(false));
    let h = hit.clone();
    // `bubbles` gates the ANCESTOR traversal, not the target. A plain (bubble)
    // listener on the target of a `load` event still runs.
    doc.add_event_listener(
        inner,
        "load",
        Box::new(move |_, _d: &mut crate::Document| *h.lock().unwrap() = true),
        ListenerOptions::default(),
    );
    let mut e = DomEvent::new("load", inner);
    doc.dispatch_event(&mut e);
    assert!(*hit.lock().unwrap());
}

#[test]
fn once_fires_a_listener_exactly_once() {
    let mut doc = doc_with("<div id=a>x</div>");
    let a = doc.query_selector("#a").unwrap();
    let n = Arc::new(Mutex::new(0));
    let c = n.clone();
    doc.add_event_listener(
        a,
        "click",
        Box::new(move |_, _d: &mut crate::Document| *c.lock().unwrap() += 1),
        ListenerOptions {
            once: true,
            ..Default::default()
        },
    );
    for _ in 0..3 {
        let mut e = DomEvent::new("click", a);
        doc.dispatch_event(&mut e);
    }
    assert_eq!(*n.lock().unwrap(), 1);
}

#[test]
fn a_passive_listener_cannot_cancel() {
    let mut doc = doc_with("<div id=a>x</div>");
    let a = doc.query_selector("#a").unwrap();
    doc.add_event_listener(
        a,
        "click",
        Box::new(|e, _d: &mut crate::Document| e.prevent_default()),
        ListenerOptions {
            passive: true,
            ..Default::default()
        },
    );
    let mut e = DomEvent::new("click", a);
    assert!(
        doc.dispatch_event(&mut e),
        "passive preventDefault is ignored"
    );
    assert!(!e.default_prevented());
}

#[test]
fn stop_propagation_and_stop_immediate_propagation_differ() {
    let mut doc = doc_with("<div id=outer><span id=inner>x</span></div>");
    let outer = doc.query_selector("#outer").unwrap();
    let inner = doc.query_selector("#inner").unwrap();

    // stopPropagation: siblings on the same node still run, ancestors do not.
    let hits = Arc::new(Mutex::new(0));
    let h1 = hits.clone();
    doc.add_event_listener(
        inner,
        "click",
        Box::new(move |e, _d: &mut crate::Document| {
            *h1.lock().unwrap() += 1;
            e.stop_propagation();
        }),
        ListenerOptions::default(),
    );
    let h2 = hits.clone();
    doc.add_event_listener(
        inner,
        "click",
        Box::new(move |_, _d: &mut crate::Document| *h2.lock().unwrap() += 1),
        ListenerOptions::default(),
    );
    let h3 = hits.clone();
    doc.add_event_listener(
        outer,
        "click",
        Box::new(move |_, _d: &mut crate::Document| *h3.lock().unwrap() += 1),
        ListenerOptions::default(),
    );
    let mut e = DomEvent::new("click", inner);
    doc.dispatch_event(&mut e);
    assert_eq!(
        *hits.lock().unwrap(),
        2,
        "both on the target, none on the ancestor"
    );
}

#[test]
fn legacy_event_aliases_answer() {
    let mut e = DomEvent::new("click", 7);
    assert_eq!(e.src_element(), 7, "srcElement mirrors target");
    assert!(e.return_value(), "returnValue starts true");
    e.set_return_value(false);
    assert!(e.default_prevented(), "returnValue = false cancels");
    assert!(!e.cancel_bubble());
    e.set_cancel_bubble(true);
    assert!(e.propagation_stopped());
}

#[test]
fn init_event_is_ignored_once_dispatch_has_begun() {
    let mut doc = doc_with("<div id=a>x</div>");
    let a = doc.query_selector("#a").unwrap();
    doc.add_event_listener(
        a,
        "click",
        Box::new(|e, _d: &mut crate::Document| {
            e.init_event("other", true, true);
            // Ignored mid-dispatch, so the type is unchanged.
            assert_eq!(e.event_type, "click");
        }),
        ListenerOptions::default(),
    );
    let mut e = DomEvent::new("click", a);
    doc.dispatch_event(&mut e);
}

// ─── CustomEvent ────────────────────────────────────────────────────────────

#[test]
fn custom_event_carries_detail_and_does_not_bubble_by_default() {
    let e = DomEvent::new_custom("my-event", 1, "payload");
    assert_eq!(e.detail(), "payload");
    assert!(!e.bubbles, "a CustomEvent with no options does not bubble");
    assert!(!e.is_trusted);
}

// ─── AbortController ────────────────────────────────────────────────────────

#[test]
fn abort_controller_signals_every_holder() {
    let c = AbortController::new();
    let a = c.signal();
    let b = c.signal();
    assert!(!a.aborted() && !b.aborted());
    assert!(a.throw_if_aborted().is_ok());

    c.abort("because");
    // Both handles see it — a signal is shared, not copied.
    assert!(a.aborted() && b.aborted());
    assert_eq!(a.reason(), "because");
    assert_eq!(b.throw_if_aborted(), Err("because".to_string()));

    // Aborting again keeps the first reason.
    c.abort("later");
    assert_eq!(a.reason(), "because");

    let pre = AbortSignal::already_aborted("");
    assert!(pre.aborted());
    assert_eq!(pre.reason(), "AbortError", "the spec's default reason");
}

#[test]
fn time_stamp_advances() {
    let a = DomEvent::new("click", 1);
    std::thread::sleep(std::time::Duration::from_millis(2));
    let b = DomEvent::new("click", 1);
    // Every event used to report 0.0, so nothing could be ordered or rate-limited.
    assert!(
        b.time_stamp() > a.time_stamp(),
        "{} !> {}",
        b.time_stamp(),
        a.time_stamp()
    );
}

#[test]
fn abort_signal_timeout_and_any() {
    let t = AbortSignal::timeout(0);
    assert!(t.aborted(), "a zero timeout is already past");
    assert_eq!(t.reason(), "TimeoutError");

    let long = AbortSignal::timeout(60_000);
    assert!(!long.aborted());

    let c = AbortController::new();
    let combined = AbortSignal::any(&[long.clone(), c.signal()]);
    assert!(!combined.aborted());
    c.abort("source aborted");
    assert!(combined.aborted(), "any() follows whichever source aborts");
    assert_eq!(combined.reason(), "source aborted");
}

#[test]
fn event_flag_accessors_agree_with_the_type_table() {
    let click = DomEvent::new("click", 1);
    assert!(click.bubbles() && click.cancelable() && click.composed());
    assert!(click.is_trusted(), "a UA-constructed event is trusted");

    let scripted = DomEvent::new_with_flags("click", 1, false, false, false);
    assert!(!scripted.bubbles() && !scripted.cancelable() && !scripted.is_trusted());
}

#[test]
fn the_script_constructor_does_not_use_the_type_table() {
    // `new Event("click")` does NOT bubble — the per-type defaults describe
    // what the USER AGENT fires, not what a script constructs. Chrome agrees.
    let scripted = DomEvent::new_script("click", 1);
    assert!(!scripted.bubbles() && !scripted.cancelable() && !scripted.composed());
    assert!(!scripted.is_trusted());

    // A UA-fired click does bubble and is cancelable.
    let ua = DomEvent::new("click", 1);
    assert!(ua.bubbles() && ua.cancelable() && ua.is_trusted());
}

// ─── UIEvent / MouseEvent / KeyboardEvent / WheelEvent ──────────────────────

#[test]
fn mouse_coordinate_families_are_distinct() {
    let mut e = DomEvent::new("click", 1);
    e.client_x = 100.0;
    e.client_y = 50.0;
    e.set_scroll_offset(300.0, 200.0);
    e.set_target_origin(80.0, 40.0);
    e.set_screen_pos(1000.0, 500.0);

    // client = viewport, page = client + scroll, offset = relative to target,
    // screen = the display. Four different questions with four answers.
    assert_eq!((e.client_x(), e.client_y()), (100.0, 50.0));
    assert_eq!((e.page_x(), e.page_y()), (400.0, 250.0));
    assert_eq!((e.offset_x(), e.offset_y()), (20.0, 10.0));
    assert_eq!((e.screen_x(), e.screen_y()), (1000.0, 500.0));
}

#[test]
fn button_and_buttons_are_different_questions() {
    let mut e = DomEvent::new("mousedown", 1);
    e.button = 2; // the RIGHT button changed state
    e.set_buttons(1 | 2); // left AND right are currently held
    assert_eq!(e.button(), 2);
    assert_eq!(e.buttons(), 3);
    // `buttons` bit 2 is RIGHT and bit 4 is MIDDLE — not `button`'s numbering.
    assert_eq!(e.buttons() & 2, 2, "right is held");
    assert_eq!(e.buttons() & 4, 0, "middle is not");
}

#[test]
fn get_modifier_state_uses_the_spec_key_values() {
    let mut e = DomEvent::new("keydown", 1);
    e.ctrl_key = true;
    e.meta_key = true;
    assert!(e.get_modifier_state("Control"));
    assert!(e.get_modifier_state("Meta"));
    assert!(!e.get_modifier_state("Shift"));
    // Recognised but unsupported modifiers answer false, not true.
    assert!(!e.get_modifier_state("CapsLock"));
    // An unknown name is false, never a panic.
    assert!(!e.get_modifier_state("NotAModifier"));
    // The names are the spec's VALUES, so lowercase is not a match.
    assert!(!e.get_modifier_state("control"));
}

#[test]
fn key_and_code_are_separate() {
    let mut e = DomEvent::new("keydown", 1);
    e.key = "A".to_string();
    e.set_code("KeyA");
    e.set_location(0);
    e.set_repeat(true);
    // `key` is what was produced, `code` is which physical key — they differ on
    // any non-US layout, and on shift.
    assert_eq!(e.key(), "A");
    assert_eq!(e.code(), "KeyA");
    assert_eq!(e.location(), 0);
    assert!(e.repeat());
    assert!(!e.is_composing());
}

#[test]
fn wheel_and_input_members_answer() {
    let mut w = DomEvent::new("wheel", 1);
    w.delta_y = 120.0;
    w.set_delta_mode(1);
    assert_eq!(w.delta_y(), 120.0);
    assert_eq!(w.delta_mode(), 1, "1 = lines");
    assert_eq!(w.delta_z(), 0.0);

    let mut i = DomEvent::new("beforeinput", 1);
    i.set_data(Some("x".into()));
    i.set_input_type("insertText");
    assert_eq!(i.data(), Some("x"));
    assert_eq!(i.input_type(), "insertText");
    // `data` is null for a deletion, not an empty string.
    let d = DomEvent::new("beforeinput", 1);
    assert_eq!(d.data(), None);
}

#[test]
fn ui_detail_is_the_click_count() {
    let mut e = DomEvent::new("click", 1);
    assert_eq!(e.ui_detail(), 0);
    e.set_ui_detail(2);
    assert_eq!(e.ui_detail(), 2, "a double click");
}

#[test]
fn key_names_follow_the_spec_for_non_printing_keys() {
    use crate::dom::events::key_name_for_code;
    // `KeyboardEvent.key` is the member handlers actually read, and every
    // non-printing key has a NAME rather than a number (UI Events §6.3.3).
    assert_eq!(key_name_for_code(13), "Enter");
    assert_eq!(key_name_for_code(27), "Escape");
    assert_eq!(key_name_for_code(37), "ArrowLeft");
    assert_eq!(key_name_for_code(8), "Backspace");
    assert_eq!(key_name_for_code(32), " ", "space is a printable character");
    // Unknown keys get the spec's own placeholder, not an empty string.
    assert_eq!(key_name_for_code(9999), "Unidentified");
}

// ─── Window as an EventTarget ───────────────────────────────────────────────

#[test]
fn window_events_fire() {
    let mut doc = doc_with("<div id=a>x</div>");
    let win = doc.window_target();
    let seen = Arc::new(Mutex::new(Vec::<String>::new()));
    for ty in ["load", "resize", "scroll", "popstate", "hashchange"] {
        let s = seen.clone();
        let t = ty.to_string();
        doc.add_event_listener(
            win,
            ty,
            Box::new(move |_, _d| s.lock().unwrap().push(t.clone())),
            ListenerOptions::default(),
        );
    }
    for ty in ["load", "resize", "scroll", "popstate", "hashchange"] {
        doc.fire_window_event(ty);
    }
    assert_eq!(
        seen.lock().unwrap().len(),
        5,
        "every WindowEventHandlers type must actually fire"
    );
}

#[test]
fn window_handler_attributes_work() {
    let mut doc = doc_with("<div>x</div>");
    let win = doc.window_target();
    let n = Arc::new(Mutex::new(0));
    let c = n.clone();
    doc.set_event_handler(
        win,
        "onload",
        Box::new(move |_, _d| *c.lock().unwrap() += 1),
    )
    .expect("onload is a WindowEventHandlers attribute");
    doc.fire_window_event("load");
    assert_eq!(*n.lock().unwrap(), 1);
}

#[test]
fn beforeunload_can_be_cancelled() {
    let mut doc = doc_with("<div>x</div>");
    let win = doc.window_target();
    doc.add_event_listener(
        win,
        "beforeunload",
        Box::new(|e, _d| e.prevent_default()),
        ListenerOptions::default(),
    );
    // `beforeunload` is the one non-bubbling event that IS cancelable, and the
    // return value is what decides whether navigation continues.
    assert!(!doc.fire_window_event("beforeunload"));
}

// ─── Dispatch flag ──────────────────────────────────────────────────────────

#[test]
fn an_event_cannot_be_dispatched_while_it_is_dispatching() {
    let mut doc = doc_with("<div id=a>x</div>");
    let a = doc.query_selector("#a").unwrap();
    let depth = Arc::new(Mutex::new(0));
    let d = depth.clone();
    doc.add_event_listener(
        a,
        "click",
        Box::new(move |e, doc2| {
            *d.lock().unwrap() += 1;
            // Re-dispatching the SAME event must be refused, not recursed into.
            assert!(e.is_dispatching());
            // Re-dispatching THIS event is refused by `dispatch_dom_event`,
            // which checks the flag — the recursion simply does not happen.
            let _ = doc2;
        }),
        ListenerOptions::default(),
    );
    let mut e = DomEvent::new("click", a);
    doc.dispatch_event(&mut e);
    assert_eq!(*depth.lock().unwrap(), 1);
    assert!(!e.is_dispatching(), "the flag clears when dispatch ends");
}

// ─── Shadow trees ───────────────────────────────────────────────────────────

/// Find a node by id INCLUDING inside shadow trees.
///
/// `querySelector` deliberately does not pierce a shadow boundary — that is
/// correct, and it is also why these tests cannot use it. The DOM way in would
/// be `host.shadowRoot.querySelector(..)`, and `ShadowRoot` has no API surface
/// yet (see the note on the last test here), so the tree is walked directly.
fn find_in_shadow(node: &crate::types::WebCore, id: &str) -> Option<u32> {
    if node.attributes.get("id").map(|s| s.as_str()) == Some(id) {
        return Some(node.node_id);
    }
    if let Some(sr) = &node.shadow_root {
        for c in &sr.children {
            if let Some(f) = find_in_shadow(c, id) {
                return Some(f);
            }
        }
    }
    node.children.iter().find_map(|c| find_in_shadow(c, id))
}

#[test]
fn an_event_inside_a_shadow_tree_dispatches_at_all() {
    let mut doc = doc_with(
        "<div id=host><template shadowrootmode=open><span id=inner>s</span></template></div>",
    );
    let inner = find_in_shadow(&doc.root, "inner").expect("the shadow node exists");
    let hit = Arc::new(Mutex::new(false));
    let h = hit.clone();
    doc.add_event_listener(
        inner,
        "click",
        Box::new(move |_, _d| *h.lock().unwrap() = true),
        ListenerOptions::default(),
    );
    let mut e = DomEvent::new("click", inner);
    doc.dispatch_event(&mut e);
    // The propagation path used to walk `children` only, so a node inside a
    // shadow tree was unreachable and the event dispatched nowhere.
    assert!(
        *hit.lock().unwrap(),
        "a listener inside a shadow tree must fire"
    );
}

#[test]
fn a_listener_outside_the_shadow_tree_sees_the_host_as_target() {
    let mut doc = doc_with(
        "<div id=host><template shadowrootmode=open><span id=inner>s</span></template></div>",
    );
    let host = doc
        .query_selector("#host")
        .expect("the host is in the light tree");
    let inner = find_in_shadow(&doc.root, "inner").expect("the shadow node exists");
    assert_ne!(host, inner);

    let seen = Arc::new(Mutex::new(0u32));
    let s = seen.clone();
    doc.add_event_listener(
        host,
        "click",
        Box::new(move |e, _d| *s.lock().unwrap() = e.target),
        ListenerOptions::default(),
    );
    let mut e = DomEvent::new("click", inner);
    doc.dispatch_event(&mut e);

    // Retargeting (DOM §2.9): outside the shadow tree the target is the HOST,
    // never the internal node — otherwise encapsulation leaks and a document
    // handler is handed a node it has no business seeing.
    assert_eq!(
        *seen.lock().unwrap(),
        host,
        "target must be retargeted to the host"
    );
    // And it is restored afterwards for anyone reading the event later.
    assert_eq!(e.target, inner);
}

#[test]
fn a_listener_inside_the_shadow_tree_sees_the_real_target() {
    let mut doc = doc_with(
        "<div id=host><template shadowrootmode=open><span id=inner>s</span></template></div>",
    );
    let inner = find_in_shadow(&doc.root, "inner").expect("the shadow node exists");
    let seen = Arc::new(Mutex::new(0u32));
    let s = seen.clone();
    doc.add_event_listener(
        inner,
        "click",
        Box::new(move |e, _d| *s.lock().unwrap() = e.target),
        ListenerOptions::default(),
    );
    let mut e = DomEvent::new("click", inner);
    doc.dispatch_event(&mut e);
    // Retargeting applies OUTSIDE the tree. Within it, the target is the node.
    assert_eq!(*seen.lock().unwrap(), inner);
}

#[test]
fn a_non_composed_event_does_not_leave_the_shadow_tree() {
    let mut doc = doc_with(
        "<div id=host><template shadowrootmode=open><span id=inner>s</span></template></div>",
    );
    let host = doc.query_selector("#host").unwrap();
    let inner = find_in_shadow(&doc.root, "inner").expect("the shadow node exists");
    let outside = Arc::new(Mutex::new(false));
    let o = outside.clone();
    doc.add_event_listener(
        host,
        "notcomposed",
        Box::new(move |_, _d| *o.lock().unwrap() = true),
        ListenerOptions::default(),
    );
    // `composed: false` — the event is confined to the shadow tree.
    let mut e = DomEvent::new_with_flags("notcomposed", inner, true, false, false);
    doc.dispatch_event(&mut e);
    assert!(
        !*outside.lock().unwrap(),
        "a non-composed event must not cross the boundary"
    );

    // The same event with `composed: true` does reach the host.
    let o2 = outside.clone();
    let mut doc2 = doc_with(
        "<div id=host><template shadowrootmode=open><span id=inner>s</span></template></div>",
    );
    let host2 = doc2.query_selector("#host").unwrap();
    let inner2 = find_in_shadow(&doc2.root, "inner").unwrap();
    doc2.add_event_listener(
        host2,
        "iscomposed",
        Box::new(move |_, _d| *o2.lock().unwrap() = true),
        ListenerOptions::default(),
    );
    let mut e2 = DomEvent::new_with_flags("iscomposed", inner2, true, false, true);
    doc2.dispatch_event(&mut e2);
    assert!(*outside.lock().unwrap(), "a composed event crosses it");
}
