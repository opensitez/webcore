//! Standalone browser windows and their native UI.
//!
//! `BrowserView` remains the embedded-page API. Both modes use the same page
//! loader, DOM, layout, controls, resource cache, and animation scheduler.

mod app;
mod document;
mod input;

pub use document::{BrowserDocument, DocumentRead, DocumentWrite};

use crate::PageLoadOptions;
use std::sync::mpsc;
use std::time::Duration;

/// Guest-language callbacks run between browser work, with no DOM lock held.
/// The browser owns OS input, frame scheduling, resources and presentation.
pub trait BrowserGuest {
    fn context_ready(&mut self) {}
    fn context_closed(&mut self) {}
    fn dispatch_pending(&mut self) -> bool;
    fn next_wakeup(&self) -> Option<Duration> {
        None
    }
}

pub(super) enum BrowserCommand {
    Navigate(String),
    Back,
    Forward,
    Reload,
    NewTab,
    ActivateTab(usize),
    CloseTab(usize),
    Inspector(bool),
    Debug(String, mpsc::Sender<String>),
    Close,
}

/// In-process control of a running browser. Requests are queued to its GUI
/// thread without a socket; the browser owns processing and frame scheduling.
#[derive(Clone)]
pub struct BrowserHandle {
    pub(super) sender: mpsc::Sender<BrowserCommand>,
    pub(super) wake: winit::event_loop::EventLoopProxy<()>,
}

impl BrowserHandle {
    fn send(&self, command: BrowserCommand) -> bool {
        self.sender.send(command).is_ok() && self.wake.send_event(()).is_ok()
    }

    pub fn navigate(&self, url: impl Into<String>) -> bool {
        self.send(BrowserCommand::Navigate(url.into()))
    }
    pub fn back(&self) -> bool {
        self.send(BrowserCommand::Back)
    }
    pub fn forward(&self) -> bool {
        self.send(BrowserCommand::Forward)
    }
    pub fn reload(&self) -> bool {
        self.send(BrowserCommand::Reload)
    }
    pub fn new_tab(&self) -> bool {
        self.send(BrowserCommand::NewTab)
    }
    pub fn activate_tab(&self, index: usize) -> bool {
        self.send(BrowserCommand::ActivateTab(index))
    }
    pub fn close_tab(&self, index: usize) -> bool {
        self.send(BrowserCommand::CloseTab(index))
    }
    pub fn show_inspector(&self, visible: bool) -> bool {
        self.send(BrowserCommand::Inspector(visible))
    }
    pub fn close(&self) -> bool {
        self.send(BrowserCommand::Close)
    }

    /// Use the documented inspector command protocol directly in-process.
    /// Waiting for the reply on the browser's GUI thread would deadlock; poll
    /// it from a host task or another thread instead.
    pub fn inspect(&self, request: impl Into<String>) -> Option<mpsc::Receiver<String>> {
        let (reply, receiver) = mpsc::channel();
        self.send(BrowserCommand::Debug(request.into(), reply))
            .then_some(receiver)
    }
}

/// Native browser UI, independent of the page's WHATWG Window and Document.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BrowserChrome {
    pub tabs: bool,
    pub address_bar: bool,
    pub navigation_buttons: bool,
}

impl Default for BrowserChrome {
    fn default() -> Self {
        Self {
            tabs: true,
            address_bar: true,
            navigation_buttons: true,
        }
    }
}

impl BrowserChrome {
    /// A content-only native window. Navigation and session history remain active.
    pub const NONE: Self = Self {
        tabs: false,
        address_bar: false,
        navigation_buttons: false,
    };
}

#[derive(Clone, Debug, Default)]
pub struct BrowserDebugOptions {
    pub port: Option<u16>,
    pub chrome_port: u16,
    pub chrome_srgb: bool,
}

/// Options for a native browser window, not author-visible DOM properties.
#[derive(Clone, Debug, Default)]
pub struct BrowserUrlOverrides {
    /// URL opened by the new-tab button and by a window without an initial URL.
    /// Mirrors the `newtab` entry of Chrome's `chrome_url_overrides` manifest
    /// field; this configures the host browser, not a page's WHATWG Window.
    pub newtab: Option<String>,
}

impl BrowserUrlOverrides {
    pub(crate) fn new_tab_url(&self) -> &str {
        self.newtab
            .as_deref()
            .filter(|url| !url.trim().is_empty())
            .unwrap_or("about:newtab")
    }
}

/// Options for a native browser window, not author-visible DOM properties.
#[derive(Clone, Debug)]
pub struct BrowserOptions {
    pub title: String,
    pub width: f32,
    pub height: f32,
    pub chrome: BrowserChrome,
    pub url_overrides: BrowserUrlOverrides,
    pub loading: PageLoadOptions,
    pub debug: BrowserDebugOptions,
}

impl Default for BrowserOptions {
    fn default() -> Self {
        Self {
            title: "Webcore".into(),
            width: 1280.0,
            height: 900.0,
            chrome: BrowserChrome::default(),
            url_overrides: BrowserUrlOverrides::default(),
            loading: PageLoadOptions::default(),
            debug: BrowserDebugOptions::default(),
        }
    }
}

/// A standalone native browser. Construct and run it on the GUI thread.
pub struct Browser {
    runtime: app::BrowserRuntime,
}

impl Browser {
    /// Create the browser context. The native window opens when `run` enters
    /// the OS event loop; navigation can begin before window initialization.
    pub fn open(options: BrowserOptions) -> Result<Self, winit::error::EventLoopError> {
        Ok(Self {
            runtime: app::BrowserRuntime::new(options)?,
        })
    }

    pub fn navigate(&mut self, url: impl Into<String>) {
        self.runtime.navigate(url.into());
    }

    /// Adopt the host bridge's live document as the initial browsing context.
    /// The handle continues to address the same nodes and event listeners.
    pub fn attach_document(&mut self, document: BrowserDocument) -> crate::window::WindowId {
        self.runtime.attach_document(document)
    }

    pub fn set_guest(&mut self, guest: impl BrowserGuest + 'static) {
        self.runtime.set_guest(Box::new(guest));
    }

    pub fn handle(&self) -> BrowserHandle {
        self.runtime.handle()
    }

    pub fn show_inspector(&mut self, visible: bool) {
        self.runtime.show_inspector(visible);
    }

    /// Own the window, input, browser UI, and page scheduling until it closes.
    pub fn run(self) -> Result<(), winit::error::EventLoopError> {
        self.runtime.run()
    }
}

/// Run the existing non-windowed inspector service. Standalone GUI clients
/// should use `Browser`; this service is retained for existing integrations.
pub fn run_debug_server(url: Option<String>, options: BrowserOptions) {
    app::run_debug_server(url, options);
}

pub(crate) fn inspect_view(view: &mut crate::BrowserView, request: &str) -> String {
    app::inspect_view(view, request)
}
