//! Opt-in browser instrumentation, usable without an external debug socket.
//!
//! This is Webcore's documented custom debug protocol, not CDP and not a
//! substitute for the page's WHATWG DOM API. Calls run on the view's owner
//! thread. A remote transport or host can use the same command payloads.

/// Inspect or interact with an embedded view using the commands documented
/// in `examples/debugserver.md`. Navigation requests begin asynchronously.
pub fn command(view: &mut crate::BrowserView, request: &str) -> String {
    crate::browser::inspect_view(view, request)
}
