//! Standalone native browser runtime. Page work remains in BrowserView.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::{Arc, mpsc};

use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::Window;

use tiny_skia::{Pixmap, PixmapPaint, Transform};

use crate::dom::{self, HtmlEventType};
use crate::platform::{Platform, VideoForeground, VideoLayerFrame};
use crate::renderer::display_list::{DisplayList, ImageRef, PaintCmd};
use crate::renderer::display_list_builder::{
    build_display_list_full_with_font_system, build_display_list_viewport,
};
use crate::types::{CssLength, Display, ObjectFit, Overflow, Position, Rect, WebCore};
use crate::{Document, Renderer, parse_html_with_hooks};

#[path = "debug.rs"]
mod debug;
#[path = "inspector.rs"]
mod inspector;
#[path = "presentation.rs"]
mod presentation;
use super::BrowserOptions;
use debug::*;
use inspector::*;
use presentation::*;

// ─── Layout constants ─────────────────────────────────────────────────────────

const TAB_BAR_HEIGHT: f32 = 36.0;
const NAV_BAR_HEIGHT: f32 = 44.0;
const TAB_MAX_W: f32 = 220.0;
const TAB_MIN_W: f32 = 80.0;

// ─── New-tab page ─────────────────────────────────────────────────────────────

const NEW_TAB_URL: &str = "about:newtab";

fn chrome_height(chrome: super::BrowserChrome) -> f32 {
    (if chrome.tabs { TAB_BAR_HEIGHT } else { 0.0 })
        + if chrome.address_bar || chrome.navigation_buttons {
            NAV_BAR_HEIGHT
        } else {
            0.0
        }
}

fn update_address_from_navigation(address: &mut String, url: &str, focused: bool) {
    // A navigation updates the committed URL, not an in-progress address edit.
    if !focused {
        address.clear();
        address.push_str(url);
    }
}

#[cfg(test)]
mod ownership_tests {
    use super::*;

    fn history_tab() -> Tab {
        Tab {
            context_id: None,
            url: "about:requested".into(),
            title: String::new(),
            history: vec!["about:first".into(), "about:requested".into(), "about:later".into()],
            history_ids: vec![1, 2, 3],
            next_history_id: 4,
            hist_i: 1,
            view: crate::BrowserView::new(320.0, 200.0, Default::default()),
            loading: false,
        }
    }

    #[test]
    fn redirect_replaces_entry_and_preserves_back_forward_history() {
        let mut tab = history_tab();
        tab.commit_navigation_url("about:hop".into(), false);
        tab.commit_navigation_url("about:final".into(), false);
        assert_eq!(tab.history, ["about:first", "about:final", "about:later"]);
        assert_eq!(tab.history_ids, [1, 2, 3]);
        assert_eq!(tab.next_history_id, 4);
        assert!(tab.can_back() && tab.can_forward());
        tab.hist_i -= 1;
        assert_eq!(tab.history[tab.hist_i], "about:first");
        tab.hist_i += 1;
        assert_eq!(tab.history[tab.hist_i], "about:final");
        tab.hist_i += 1;
        assert_eq!(tab.history[tab.hist_i], "about:later");
    }

    #[test]
    fn document_navigation_pushes_once_even_when_redirect_arrives_in_same_poll() {
        let mut tab = history_tab();
        tab.commit_navigation_url("about:destination".into(), true);
        assert_eq!(tab.history, ["about:first", "about:requested", "about:destination"]);
        assert_eq!(tab.history_ids, [1, 2, 4]);
        tab.commit_navigation_url("about:final".into(), false);
        assert_eq!(tab.history, ["about:first", "about:requested", "about:final"]);
        assert_eq!(tab.history_ids, [1, 2, 4]);
        tab.commit_navigation_url("about:final#section".into(), true);
        assert_eq!(tab.history[tab.hist_i - 1], "about:final");
        assert_eq!(tab.history_ids, [1, 2, 4, 5]);
    }

    #[test]
    fn http_redirect_chain_commits_one_history_entry() {
        use std::time::{Duration, Instant};
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(10);
            let responses = [
                "HTTP/1.1 302 Found\r\nLocation: /hop\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string(),
                "HTTP/1.1 307 Temporary Redirect\r\nLocation: /final\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string(),
                {
                    let html = "<html><head><title>Destination</title></head><body>Final page</body></html>";
                    format!("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{html}", html.len())
                },
            ];
            for response in responses {
                let mut socket = loop {
                    match listener.accept() {
                        Ok((socket, _)) => break socket,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(Instant::now() < deadline, "redirect request timed out");
                            std::thread::sleep(Duration::from_millis(5));
                        }
                        Err(error) => panic!("{error}"),
                    }
                };
                socket.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                let mut request = BufReader::new(socket.try_clone().unwrap());
                loop {
                    let mut line = String::new();
                    assert!(request.read_line(&mut line).unwrap() > 0);
                    if line == "\r\n" { break; }
                }
                socket.write_all(response.as_bytes()).unwrap();
            }
        });
        let mut tab = history_tab();
        let requested = format!("{origin}/start");
        tab.url = requested.clone();
        tab.history[1] = requested.clone();
        tab.view.navigate_history(requested, 2, false);
        let deadline = Instant::now() + Duration::from_secs(10);
        while tab.view.is_loading() && Instant::now() < deadline {
            tab.view.poll();
            if tab.url != tab.view.url() {
                let url = tab.view.url().to_string();
                let new_navigation = tab.view.take_document_navigation();
                tab.commit_navigation_url(url, new_navigation);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        server.join().unwrap();
        assert!(!tab.view.is_loading());
        assert_eq!(tab.history, ["about:first".to_string(), format!("{origin}/final"), "about:later".to_string()]);
        assert_eq!(tab.history_ids, [1, 2, 3]);
        assert_eq!(tab.view.url(), format!("{origin}/final"));
        assert!(tab.view.document().is_some());
    }

    #[test]
    fn navigation_updates_address_without_replacing_an_edit() {
        let mut address = "http://localhost/old".to_string();
        update_address_from_navigation(&mut address, "http://localhost/new", false);
        assert_eq!(address, "http://localhost/new");
        address = "http://localhost/draft".to_string();
        update_address_from_navigation(&mut address, "http://localhost/redirect", true);
        assert_eq!(address, "http://localhost/draft");
    }

    #[test]
    fn address_normalization_preserves_urls_and_encodes_searches() {
        assert_eq!(
            normalize_url(" localhost:8080/page?q=1 ".into()),
            "http://localhost:8080/page?q=1"
        );
        assert_eq!(
            normalize_url("127.0.0.1/page".into()),
            "http://127.0.0.1/page"
        );
        assert_eq!(
            normalize_url("[::1]:8080/page".into()),
            "http://[::1]:8080/page"
        );
        assert_eq!(
            normalize_url("example.com/page".into()),
            "https://example.com/page"
        );
        assert_eq!(normalize_url("about:blank".into()), "about:blank");
        let search = reqwest::Url::parse(&normalize_url("cats & dogs #photos".into())).unwrap();
        assert_eq!(
            search.query_pairs().collect::<Vec<_>>(),
            vec![("q".into(), "cats & dogs #photos".into())]
        );
        assert!(search.fragment().is_none());
    }

    #[test]
    fn new_tab_override_uses_host_url_or_builtin_default() {
        let mut overrides = super::super::BrowserUrlOverrides::default();
        assert_eq!(overrides.new_tab_url(), NEW_TAB_URL);
        overrides.newtab = Some("http://localhost/start.html".into());
        assert_eq!(overrides.new_tab_url(), "http://localhost/start.html");
        overrides.newtab = Some(" ".into());
        assert_eq!(overrides.new_tab_url(), NEW_TAB_URL);
    }

    #[test]
    fn native_controls_reserve_only_visible_rows() {
        use crate::browser::BrowserChrome;
        assert_eq!(
            chrome_height(BrowserChrome::default()),
            TAB_BAR_HEIGHT + NAV_BAR_HEIGHT
        );
        assert_eq!(chrome_height(BrowserChrome::NONE), 0.0);
        assert_eq!(
            chrome_height(BrowserChrome {
                tabs: true,
                ..BrowserChrome::NONE
            }),
            TAB_BAR_HEIGHT
        );
        assert_eq!(
            chrome_height(BrowserChrome {
                address_bar: true,
                ..BrowserChrome::NONE
            }),
            NAV_BAR_HEIGHT
        );
        assert_eq!(
            chrome_height(BrowserChrome {
                navigation_buttons: true,
                ..BrowserChrome::NONE
            }),
            NAV_BAR_HEIGHT
        );
    }

    #[test]
    fn embedded_inspection_cannot_exit_host() {
        let mut view = crate::BrowserView::new(320.0, 240.0, crate::PageLoadOptions::default());
        let response = crate::inspection::command(&mut view, r#"{"cmd":"quit"}"#);
        assert!(response.contains(r#""ok":false"#));
        assert!(response.contains("owning context"));
    }

    #[test]
    fn address_input_replaces_selection_and_edits_at_caret() {
        let mut doc = parse_html_with_hooks(
            r#"<input id="url-input" value="https://old.example/">"#,
            "",
            |_, _| {},
        );
        let id = doc.query_selector("#url-input").unwrap();
        doc.focus(id);
        doc.select(id);
        for ch in "http://localhost/".chars() {
            doc.process_key_event(
                HtmlEventType::KeyDown,
                0,
                Some(ch),
                false,
                false,
                false,
                false,
            );
        }
        assert_eq!(doc.value(id), "http://localhost/");
        doc.process_key_event(HtmlEventType::KeyDown, 37, None, false, false, false, false);
        doc.process_key_event(HtmlEventType::KeyDown, 8, None, false, false, false, false);
        assert_eq!(doc.value(id), "http://localhos/");
        doc.process_key_event(
            HtmlEventType::KeyDown,
            65,
            Some('a'),
            true,
            false,
            false,
            false,
        );
        doc.process_key_event(HtmlEventType::KeyDown, 46, None, false, false, false, false);
        assert_eq!(doc.value(id), "");
    }
}

pub(super) struct BrowserRuntime {
    event_loop: EventLoop<()>,
    app: BrowserApp,
    debug: super::BrowserDebugOptions,
    handle: super::BrowserHandle,
}

impl BrowserRuntime {
    pub(super) fn new(options: BrowserOptions) -> Result<Self, winit::error::EventLoopError> {
        let event_loop = EventLoop::<()>::with_user_event().build()?;
        event_loop.set_control_flow(ControlFlow::Wait);
        let mut app = BrowserApp::new(event_loop.create_proxy(), &options);
        let (sender, receiver) = mpsc::channel();
        app.commands = Some(receiver);
        let handle = super::BrowserHandle {
            sender,
            wake: event_loop.create_proxy(),
        };
        app.cache_dir = options.loading.cache_dir.clone();
        app.no_images = !options.loading.load_images;
        app.initial_width = options.width.max(1.0);
        app.initial_height = options.height.max(1.0);
        app.width = app.initial_width;
        app.height = app.initial_height;
        Ok(Self {
            event_loop,
            app,
            debug: options.debug,
            handle,
        })
    }

    pub(super) fn navigate(&mut self, url: String) {
        self.app.navigate(url);
    }

    pub(super) fn attach_document(
        &mut self,
        document: super::BrowserDocument,
    ) -> crate::window::WindowId {
        let document_id = crate::dom::registry::register_document(document.clone());
        let context_id = crate::window::adopt(document_id, "");
        let tab = &mut self.app.tabs[self.app.active];
        tab.context_id = Some(context_id);
        tab.view.attach_document(document);
        tab.url = tab.view.url().to_string();
        tab.title = tab.view.title().to_string();
        tab.history = vec![tab.url.clone()];
        tab.history_ids = vec![tab.next_history_id];
        tab.next_history_id += 1;
        tab.hist_i = 0;
        tab.loading = false;
        self.app.url_text = tab.url.clone();
        context_id
    }

    pub(super) fn set_guest(&mut self, guest: Box<dyn super::BrowserGuest>) {
        self.app.guest = Some(guest);
    }

    pub(super) fn handle(&self) -> super::BrowserHandle {
        self.handle.clone()
    }
    pub(super) fn show_inspector(&mut self, visible: bool) {
        self.app.set_inspector(visible);
    }

    pub(super) fn run(mut self) -> Result<(), winit::error::EventLoopError> {
        if self.debug.chrome_port > 0 {
            self.app.chrome_port = self.debug.chrome_port;
            if let Some(path) = find_chrome() {
                let url = if self.app.tabs[self.app.active].url.is_empty() {
                    "about:blank"
                } else {
                    &self.app.tabs[self.app.active].url
                };
                match std::process::Command::new(path)
                    .arg(format!(
                        "--remote-debugging-port={}",
                        self.debug.chrome_port
                    ))
                    .args(
                        self.debug
                            .chrome_srgb
                            .then_some("--force-color-profile=srgb"),
                    )
                    .arg(format!(
                        "--window-size={},{}",
                        self.app.width as u32, self.app.height as u32
                    ))
                    .arg("--disable-extensions")
                    .arg("--blink-settings=scriptEnabled=false")
                    .arg("--no-first-run")
                    .arg("--no-default-browser-check")
                    .arg(format!(
                        "--user-data-dir=/tmp/browser-chrome-{}",
                        self.debug.chrome_port
                    ))
                    .arg(format!("--app={url}"))
                    .spawn()
                {
                    Ok(child) => self.app.chrome_process = Some(child),
                    Err(error) => eprintln!("[browser] Chrome launch failed: {error}"),
                }
            } else {
                eprintln!("[browser] Chrome not found");
            }
        }
        let _debug_server = if let Some(port) = self.debug.port {
            let (cmd_tx, cmd_rx) = mpsc::channel::<(String, mpsc::Sender<String>)>();
            self.app.debug_cmd_rx = Some(cmd_rx);
            match browser_debug_spawn_tcp(port, cmd_tx, self.event_loop.create_proxy()) {
                Ok(server) => Some(server),
                Err(error) => {
                    eprintln!("[debug] Failed to bind port {port}: {error}");
                    None
                }
            }
        } else {
            None
        };
        let result = self.event_loop.run_app(&mut self.app);
        crate::embedded_window::set_active_window(None);
        if let Some(guest) = self.app.guest.as_mut() {
            guest.context_closed();
        }
        for tab in &self.app.tabs {
            if let Some(context) = tab.context_id {
                crate::window::close(context);
            }
        }
        if let Some(mut child) = self.app.chrome_process.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        result
    }
}

pub(super) fn run_debug_server(url: Option<String>, options: BrowserOptions) {
    run_headless(
        url,
        options.debug.port.unwrap_or(9222),
        options.width,
        options.height,
        options.loading.cache_dir,
        !options.loading.load_images,
        options.debug.chrome_port,
    );
}

pub(super) fn inspect_view(view: &mut crate::BrowserView, request: &str) -> String {
    if dbg_json_str(request, "cmd").as_deref() == Some("quit") {
        return r#"{"ok":false,"error":"close the owning context through its handle"}"#.into();
    }
    let (width, height) = view.viewport_size();
    dispatch_view_command(view, width, height, request, &None, 0)
}

// ─── Tab ──────────────────────────────────────────────────────────────────────

struct Tab {
    context_id: Option<crate::window::WindowId>,
    url: String,
    title: String,
    history: Vec<String>,
    history_ids: Vec<u64>,
    next_history_id: u64,
    hist_i: usize,
    view: crate::BrowserView,
    loading: bool,
}

impl Tab {
    fn new(proxy: EventLoopProxy<()>) -> Self {
        let mut view = crate::BrowserView::new(1280.0, 720.0, Default::default());
        view.set_wake_callback(move || {
            let _ = proxy.send_event(());
        });
        Self {
            context_id: None,
            url: String::new(),
            title: "New Tab".into(),
            history: vec![],
            history_ids: vec![],
            next_history_id: 1,
            hist_i: 0,
            view,
            loading: false,
        }
    }
    fn can_back(&self) -> bool {
        self.hist_i > 0
    }
    fn can_forward(&self) -> bool {
        self.hist_i + 1 < self.history.len()
    }
    fn commit_navigation_url(&mut self, url: String, new_navigation: bool) {
        if self.history.get(self.hist_i) != Some(&url) {
            if !new_navigation && !self.history.is_empty() {
                // Redirects belong to the current navigation, including history traversal.
                self.history[self.hist_i] = url.clone();
            } else {
                self.history.truncate(self.hist_i + 1);
                self.history_ids.truncate(self.hist_i + 1);
                self.history.push(url.clone());
                let entry_id = self.next_history_id;
                self.next_history_id = self.next_history_id.wrapping_add(1);
                self.history_ids.push(entry_id);
                self.hist_i = self.history.len() - 1;
                self.view.set_history_entry(entry_id);
            }
        }
        self.url = url;
    }
    fn short_title(&self) -> String {
        let t = self.title.trim();
        let chars: Vec<char> = t.chars().collect();
        if chars.len() > 22 {
            format!("{}…", chars[..22].iter().collect::<String>())
        } else {
            chars.iter().collect()
        }
    }
}

// ─── Chrome click regions ─────────────────────────────────────────────────────

#[derive(Debug)]
enum ChromeHit {
    None,
    Back,
    Forward,
    Reload,
    UrlBar,
    Tab(usize),
    CloseTab(usize),
    NewTab,
}

// ─── App ──────────────────────────────────────────────────────────────────────

struct BrowserApp {
    window: Option<Arc<Window>>,
    platform: Option<Platform>,

    chrome_renderer: Option<Renderer>,
    chrome_settings: super::BrowserChrome,
    chrome_height: f32,
    window_title: String,
    loading_options: crate::PageLoadOptions,
    chrome_pixmap: Option<Pixmap>,
    chrome_pixmap_dirty: bool,

    tabs: Vec<Tab>,
    active: usize,

    chrome_doc: Option<Document>,

    // URL bar state
    url_text: String,
    url_focused: bool,

    mouse_pos: (f32, f32),
    width: f32,
    height: f32,
    initial_width: f32,
    initial_height: f32,

    // Inspector state
    inspect_mode: bool,
    inspect_node: u32,
    inspect_panel_pct: f32,  // panel WIDTH as fraction of window (0.3 = 30%)
    inspect_dragging: bool,  // true while dragging the vertical splitter
    inspect_tab: u8,         // 0=Styles, 1=Computed, 2=Box Model
    inspect_dom_split: f32,  // fraction of panel height for DOM tree (top), rest for tabs (bottom)
    inspect_dom_scroll: f32, // current scroll offset of the DOM tree

    proxy: EventLoopProxy<()>,
    initial_url: Option<String>,
    new_tab_url: String,
    cache_dir: Option<String>, // Some("snapshot_cache") when --cached
    no_images: bool,

    // Remote debug server (--debug-port)
    debug_cmd_rx: Option<mpsc::Receiver<(String, mpsc::Sender<String>)>>,
    chrome_port: u16,
    chrome_process: Option<std::process::Child>,
    last_memory_trace: Option<std::time::Instant>,
    last_profile_trace: Option<std::time::Instant>,
    native_video_composition: Option<NativeVideoComposition>,
    native_video_opacity: Option<(Arc<Vec<u8>>, bool)>,
    commands: Option<mpsc::Receiver<super::BrowserCommand>>,
    close_requested: bool,
    modifiers: ModifiersState,
    guest: Option<Box<dyn super::BrowserGuest>>,
}

impl BrowserApp {
    fn new(proxy: EventLoopProxy<()>, options: &BrowserOptions) -> Self {
        let chrome_height = chrome_height(options.chrome);
        Self {
            window: None,
            platform: None,
            chrome_renderer: (chrome_height > 0.0).then(Renderer::new),
            chrome_settings: options.chrome,
            chrome_height,
            window_title: options.title.clone(),
            loading_options: options.loading.clone(),
            chrome_pixmap: None,
            chrome_pixmap_dirty: true,
            tabs: vec![Tab::new(proxy.clone())],
            active: 0,
            chrome_doc: None,
            url_text: String::new(),
            url_focused: false,
            mouse_pos: (0.0, 0.0),
            width: 1280.0,
            height: 800.0,
            initial_width: 1280.0,
            initial_height: 800.0,
            inspect_mode: false,
            inspect_node: 0,
            inspect_panel_pct: 0.0,
            inspect_dragging: false,
            inspect_tab: 0,
            inspect_dom_split: 0.5,
            inspect_dom_scroll: 0.0,
            proxy,
            initial_url: None,
            new_tab_url: options.url_overrides.new_tab_url().to_string(),
            cache_dir: None,
            no_images: false,
            debug_cmd_rx: None,
            chrome_port: 0,
            chrome_process: None,
            last_memory_trace: None,
            last_profile_trace: None,
            native_video_composition: None,
            native_video_opacity: None,
            commands: None,
            close_requested: false,
            modifiers: ModifiersState::empty(),
            guest: None,
        }
    }

    fn content_h(&self) -> f32 {
        (self.height - self.chrome_height).max(0.0)
    }
    fn page_width(&self) -> f32 {
        if self.inspect_mode {
            (self.width * (1.0 - self.inspect_panel_pct)).max(100.0)
        } else {
            self.width
        }
    }

    fn start_view_navigation(&mut self, index: usize, url: String, entry_id: u64, restore: bool) {
        crate::profile::reset();
        self.last_profile_trace = None;
        let page_w = self.page_width();
        let content_h = self.content_h();
        let cache_dir = self.cache_dir.clone();
        let load_images = !self.no_images;
        let tab = &mut self.tabs[index];
        tab.view.resize(page_w, content_h);
        tab.view.set_options(crate::PageLoadOptions {
            cache_dir,
            load_images,
            ..self.loading_options.clone()
        });
        tab.view.navigate_history(url, entry_id, restore);
    }

    // ── Navigation ────────────────────────────────────────────────────────────

    fn navigate(&mut self, url: String) {
        let url = normalize_url(url);
        let tab = &mut self.tabs[self.active];
        if !tab.history.is_empty() {
            tab.history.truncate(tab.hist_i + 1);
            tab.history_ids.truncate(tab.hist_i + 1);
        }
        tab.history.push(url.clone());
        let entry_id = tab.next_history_id;
        tab.next_history_id = tab.next_history_id.wrapping_add(1);
        tab.history_ids.push(entry_id);
        tab.hist_i = tab.history.len() - 1;
        tab.url = url.clone();
        tab.title = "Loading…".into();
        tab.loading = true;
        self.url_text = url.clone();
        self.url_focused = false;
        self.start_view_navigation(self.active, url, entry_id, false);
        self.rebuild_chrome();
    }

    fn go_back(&mut self) {
        let tab = &mut self.tabs[self.active];
        if !tab.can_back() {
            return;
        }
        tab.hist_i -= 1;
        let url = tab.history[tab.hist_i].clone();
        let entry_id = tab.history_ids[tab.hist_i];
        tab.url = url.clone();
        tab.loading = true;
        self.url_text = url.clone();
        self.rebuild_chrome();
        self.start_view_navigation(self.active, url, entry_id, true);
    }

    fn go_forward(&mut self) {
        let tab = &mut self.tabs[self.active];
        if !tab.can_forward() {
            return;
        }
        tab.hist_i += 1;
        let url = tab.history[tab.hist_i].clone();
        let entry_id = tab.history_ids[tab.hist_i];
        tab.url = url.clone();
        tab.loading = true;
        self.url_text = url.clone();
        self.rebuild_chrome();
        self.start_view_navigation(self.active, url, entry_id, true);
    }

    fn reload(&mut self) {
        let url = self.tabs[self.active].url.clone();
        if url.is_empty() {
            return;
        }
        let Some(&entry_id) = self.tabs[self.active]
            .history_ids
            .get(self.tabs[self.active].hist_i)
        else {
            self.navigate(url);
            return;
        };
        self.tabs[self.active].loading = true;
        self.rebuild_chrome();
        self.start_view_navigation(self.active, url, entry_id, false);
    }

    fn new_tab(&mut self) {
        self.tabs.push(Tab::new(self.proxy.clone()));
        self.active = self.tabs.len() - 1;
        self.navigate(self.new_tab_url.clone());
        self.focus_address();
    }

    fn switch_tab(&mut self, i: usize) {
        if i >= self.tabs.len() {
            return;
        }
        self.active = i;
        self.url_text = self.tabs[i].url.clone();
        self.url_focused = false;
        self.tabs[self.active].view.invalidate_display();
        self.rebuild_chrome();
    }

    fn close_tab(&mut self, i: usize) {
        if i >= self.tabs.len() {
            return;
        }
        if self.tabs.len() == 1 {
            self.tabs[0] = Tab::new(self.proxy.clone());
            self.navigate(self.new_tab_url.clone());
            return;
        }
        self.tabs.remove(i);
        if i < self.active {
            self.active -= 1;
        } else if self.active >= self.tabs.len() {
            self.active = self.tabs.len() - 1;
        }
        self.url_text = self.tabs[self.active].url.clone();
        self.url_focused = false;
        self.tabs[self.active].view.invalidate_display();
        self.rebuild_chrome();
    }

    fn relayout_active(&mut self) {
        let ch = self.content_h();
        let page_w = self.page_width();
        self.tabs[self.active].view.resize(page_w, ch);
        self.tabs[self.active].view.relayout();
    }

    // ── Chrome ────────────────────────────────────────────────────────────────

    fn rebuild_chrome(&mut self) {
        if self.chrome_height == 0.0 {
            self.chrome_doc = None;
            self.chrome_pixmap = None;
            return;
        }
        let html = self.chrome_html();
        let selection = self.chrome_doc.as_ref().and_then(|doc| {
            let id = doc.query_selector("#url-input")?;
            Some((doc.selection_start(id)?, doc.selection_end(id)?))
        });
        let mut doc = parse_html_with_hooks(&html, "", |_, _| {});
        if self.url_focused {
            if let Some(id) = doc.query_selector("#url-input") {
                doc.focus(id);
                if let Some((start, end)) = selection {
                    doc.set_selection_range(id, start, end, None);
                }
            }
        }
        let w = self.width;
        let eng = self
            .chrome_renderer
            .get_or_insert_with(Renderer::new)
            .layout_engine();
        eng.viewport_h = self.chrome_height;
        eng.layout(&mut doc, w);
        self.chrome_renderer
            .as_mut()
            .expect("chrome renderer")
            .invalidate_display_list();
        self.chrome_doc = Some(doc);
        self.chrome_pixmap_dirty = true;
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn chrome_html(&self) -> String {
        let chrome_height = self.chrome_height;
        let tabs_display = if self.chrome_settings.tabs {
            "flex"
        } else {
            "none"
        };
        let nav_display =
            if self.chrome_settings.address_bar || self.chrome_settings.navigation_buttons {
                "flex"
            } else {
                "none"
            };
        let address_display = if self.chrome_settings.address_bar {
            "flex"
        } else {
            "none"
        };
        let buttons_display = if self.chrome_settings.navigation_buttons {
            "flex"
        } else {
            "none"
        };
        let n = self.tabs.len();
        // Give tabs a fair share, capped at max, but reserve space for new-tab button
        let avail = self.width - 40.0;
        let tab_w = (avail / n as f32).min(TAB_MAX_W).max(TAB_MIN_W);

        let mut tabs_html = String::new();
        for (i, tab) in self.tabs.iter().enumerate() {
            let active = i == self.active;
            let cls = if active { "tab active" } else { "tab" };
            let x_cls = if active { "tab-x ax" } else { "tab-x" };
            let title = escape_html(&tab.short_title());
            let fav_bg = domain_color(&tab.url);
            let fav_ch = domain_letter(&tab.url);
            let spinner = if tab.loading { "↻ " } else { "" };
            tabs_html.push_str(&format!(
                r#"<div class="{cls}" id="tab-{i}" style="max-width:{tab_w:.0}px;min-width:{TAB_MIN_W}px"><div class="fav" style="background:{fav_bg}">{fav_ch}</div><span class="tab-t">{spinner}{title}</span><span class="{x_cls}" id="tab-x-{i}">&#215;</span></div>"#
            ));
        }

        let tab = &self.tabs[self.active];
        let sec = if tab.url.starts_with("https://") {
            "<span class='lock'>&#128274;</span>"
        } else if tab.url.starts_with("http://") {
            "<span class='warn'>&#9888;</span>"
        } else {
            ""
        };
        let url_txt = if self.url_focused {
            escape_html(&self.url_text)
        } else {
            pretty_url(&tab.url)
        };
        let url_cls = if self.url_focused {
            "urlbar focused"
        } else {
            "urlbar"
        };
        let bd = if tab.can_back() { "btn" } else { "btn dis" };
        let fd = if tab.can_forward() { "btn" } else { "btn dis" };

        format!(
            r#"<!DOCTYPE html><html><head><style>
*{{box-sizing:border-box;margin:0;padding:0}}
body{{font-family:-apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,sans-serif;
      background:#1C1C1F;display:flex;flex-direction:column;height:{chrome_height}px;overflow:hidden}}
#tab-bar{{display:{tabs_display};align-items:flex-end;height:36px;background:#141416;
          padding:0 4px 0 4px;gap:1px;overflow:hidden}}
.tab{{display:flex;align-items:center;height:30px;padding:0 10px;
      border-radius:8px 8px 0 0;background:transparent;color:#6E7077;
      font-size:12px;cursor:pointer;gap:6px;flex-shrink:0;
      border:1px solid transparent;border-bottom:none;overflow:hidden}}
.tab.active{{background:#1C1C1F;color:#E8EAED;border-color:#2C2C2F;border-bottom-color:#1C1C1F}}
.fav{{width:15px;height:15px;border-radius:4px;flex-shrink:0;
      font-size:9px;font-weight:800;color:#fff;
      display:flex;align-items:center;justify-content:center}}
.tab-t{{flex:1;overflow:hidden;white-space:nowrap;min-width:0}}
.tab-x{{color:transparent;font-size:13px;width:18px;height:18px;flex-shrink:0;
        border-radius:4px;display:flex;align-items:center;justify-content:center}}
.ax{{color:#6E7077}}
#btn-new-tab{{width:28px;height:28px;border-radius:8px;color:#6E7077;font-size:20px;
              display:flex;align-items:center;justify-content:center;cursor:pointer;
              align-self:flex-end;margin-bottom:1px;flex-shrink:0;margin-left:2px}}
#nav-bar{{display:{nav_display};align-items:center;height:44px;background:#1C1C1F;padding:0 10px;gap:4px;
          border-top:1px solid #2C2C2F}}
.btn{{width:30px;height:30px;border-radius:50%;color:#C9CBD0;font-size:16px;
      display:flex;align-items:center;justify-content:center;cursor:pointer;flex-shrink:0}}
.dis{{color:#3A3B3E;cursor:default}}
.urlbar{{flex:1;height:32px;background:#2A2B2E;border:1.5px solid #3A3B3E;
         border-radius:16px;color:#E8EAED;font-size:13px;padding:0 14px;
         display:flex;align-items:center;overflow:hidden;cursor:text;margin:0 8px;gap:6px}}
.urlbar.focused{{border-color:#5E9CF8;background:#252628}}
.lock{{color:#5CB85C;font-size:11px;flex-shrink:0}}
.warn{{color:#F0AD4E;font-size:11px;flex-shrink:0}}
.url-t{{flex:1;min-width:0;width:100%;height:24px;border:0;padding:0;margin:0;background:transparent;
        font:inherit;color:#C9CBD0}}
.urlbar.focused .url-t{{color:#E8EAED}}
.cur{{color:#5E9CF8}}
#ext-btn{{width:30px;height:30px;border-radius:50%;color:#6E7077;font-size:19px;
          display:flex;align-items:center;justify-content:center;cursor:pointer;flex-shrink:0}}
#url-bar{{display:{address_display}}}
#btn-back,#btn-fwd,#btn-reload,#ext-btn{{display:{buttons_display}}}
</style></head><body>
<div id="tab-bar">{tabs_html}<div id="btn-new-tab">+</div></div>
<div id="nav-bar">
  <div class="{bd}" id="btn-back">&#8592;</div>
  <div class="{fd}" id="btn-fwd">&#8594;</div>
  <div class="btn" id="btn-reload">&#8635;</div>
  <div class="{url_cls}" id="url-bar">{sec}<input class="url-t" id="url-input" type="text" value="{url_txt}"></div>
  <div id="ext-btn">&#8942;</div>
</div></body></html>"#
        )
    }

    fn chrome_hit(&self, x: f32, y: f32) -> ChromeHit {
        let Some(doc) = &self.chrome_doc else {
            return ChromeHit::None;
        };
        let pt_in = |id: &str| -> bool {
            if let Some(b) = dom::query_selector(&doc.root, &format!("#{id}")) {
                let r = &b.layout.border_rect;
                x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h
            } else {
                false
            }
        };
        // Close buttons first (smaller, inside tab)
        for i in 0..self.tabs.len() {
            if pt_in(&format!("tab-x-{i}")) {
                return ChromeHit::CloseTab(i);
            }
        }
        if pt_in("btn-back") {
            return ChromeHit::Back;
        }
        if pt_in("btn-fwd") {
            return ChromeHit::Forward;
        }
        if pt_in("btn-reload") {
            return ChromeHit::Reload;
        }
        if pt_in("url-bar") {
            return ChromeHit::UrlBar;
        }
        if pt_in("btn-new-tab") {
            return ChromeHit::NewTab;
        }
        for i in 0..self.tabs.len() {
            if pt_in(&format!("tab-{i}")) {
                return ChromeHit::Tab(i);
            }
        }
        ChromeHit::None
    }

    // ── Render ────────────────────────────────────────────────────────────────

    fn draw(&mut self) {
        if self.inspect_mode && self.chrome_renderer.is_none() {
            self.chrome_renderer = Some(Renderer::new());
        }
        let profile_draw = crate::profile::span(crate::profile::Phase::BrowserDraw);
        let Some(platform) = self.platform.as_mut() else {
            return;
        };

        let chrome_renderer = &mut self.chrome_renderer;
        let chrome_doc = self.chrome_doc.as_mut();

        let t_draw = std::time::Instant::now();
        platform.render_full(|scale, pixmap| {
            let w_px = pixmap.width();
            let h_px = pixmap.height();
            let chrome_px  = ((self.chrome_height * scale) as u32).min(h_px);
            let content_px = h_px.saturating_sub(chrome_px);

            // Page painting replaces its entire viewport, including placeholders.
            // Keep the full clear for inspector panels that may fail to allocate.
            let background = tiny_skia::Color::from_rgba8(28, 28, 31, 255);
            if self.inspect_mode {
                pixmap.fill(background);
            } else if let Some(rect) = tiny_skia::Rect::from_xywh(
                0.0, 0.0, w_px as f32, chrome_px as f32,
            ) {
                let mut paint = tiny_skia::Paint::default();
                paint.set_color(background);
                pixmap.fill_rect(rect, &paint, Transform::identity(), None);
            }

            // Page content in sub-pixmap, blitted below chrome
            let inspect_on = self.inspect_mode;
            let inspect_pct = self.inspect_panel_pct;
            let inspect_tab = self.inspect_tab;
            let inspect_dom_scroll = &mut self.inspect_dom_scroll;
            let inspect_nid = self.inspect_node;
            let inspect_dom_split = self.inspect_dom_split;
            let page_w = if inspect_on {
                ((w_px as f32) * (1.0 - inspect_pct)).max(100.0) as u32
            } else {
                w_px
            };
            self.tabs[self.active]
                .view
                .resize(page_w as f32 / scale, content_px as f32 / scale);
            self.tabs[self.active]
                .view
                .paint_into(pixmap, 0, chrome_px as i32, scale);

            if let Some(doc) = self.tabs[self.active].view.document().as_deref() {
                // Inspector panel (right side): DOM tree on top, tabs on bottom
                if inspect_on {
                    let panel_x = page_w;
                    let panel_w = w_px.saturating_sub(page_w).max(1);
                    let panel_w_logical = panel_w as f32 / scale;
                    // Split: DOM tree top, tabs bottom
                    let dom_h = ((content_px as f32) * inspect_dom_split) as u32;
                    let tabs_h = content_px.saturating_sub(dom_h);

                    // DOM tree
                    let (dom_tree_body, selected_line) = build_dom_tree_html(&doc.root, inspect_nid);
                    let dom_html = format!(
                        "<html><head><style>\
                         body{{background:#1e1e1e;margin:0;padding:4px;font:11px monospace}}\
                         </style></head><body>{dom_tree_body}</body></html>"
                    );
                    let mut dom_doc = crate::parse_html(&dom_html);
                    { let eng = chrome_renderer.as_mut().expect("chrome renderer").layout_engine(); eng.layout(&mut dom_doc, panel_w_logical); }
                    // Scroll DOM tree to selected element
                    if let Some(line) = selected_line {
                        let target_y = line as f32 * 16.0;
                        let visible_h = dom_h as f32 / scale;
                        *inspect_dom_scroll = (target_y - visible_h * 0.3).max(0.0);
                    }
                    dom_doc.scroll_y = *inspect_dom_scroll;
                    if let Some(mut pm) = Pixmap::new(panel_w, dom_h.max(1)) {
                        pm.fill(tiny_skia::Color::from_rgba8(30, 30, 30, 255));
                        chrome_renderer.as_mut().expect("chrome renderer").render(&mut dom_doc, &mut pm, scale);
                        pixmap.draw_pixmap(panel_x as i32, chrome_px as i32, pm.as_ref(),
                            &PixmapPaint::default(), Transform::identity(), None);
                    }

                    // Tabs
                    let tabs_html = if inspect_nid != 0 {
                        if let Some(node) = doc.get_box_by_id(inspect_nid) {
                            build_inspect_panel_html(node, inspect_tab, Some(&doc.root))
                        } else {
                            "<html><body style='background:#1e1e1e;color:#666;padding:10px;font:11px monospace'>Right-click to inspect</body></html>".into()
                        }
                    } else {
                        "<html><body style='background:#1e1e1e;color:#666;padding:10px;font:11px monospace'>Right-click to inspect</body></html>".into()
                    };
                    let mut tabs_doc = crate::parse_html(&tabs_html);
                    { let eng = chrome_renderer.as_mut().expect("chrome renderer").layout_engine(); eng.layout(&mut tabs_doc, panel_w_logical); }
                    if let Some(mut pm) = Pixmap::new(panel_w, tabs_h.max(1)) {
                        pm.fill(tiny_skia::Color::from_rgba8(30, 30, 30, 255));
                        chrome_renderer.as_mut().expect("chrome renderer").render(&mut tabs_doc, &mut pm, scale);
                        pixmap.draw_pixmap(panel_x as i32, (chrome_px + dom_h) as i32, pm.as_ref(),
                            &PixmapPaint::default(), Transform::identity(), None);
                    }

                    // Vertical divider between page and panel
                    if let Some(r) = tiny_skia::Rect::from_xywh(panel_x as f32 - 1.0, chrome_px as f32, 2.0, content_px as f32) {
                        let mut p = tiny_skia::Paint::default();
                        p.set_color(tiny_skia::Color::from_rgba8(60, 60, 65, 255));
                        pixmap.fill_rect(r, &p, Transform::identity(), None);
                    }
                    // Horizontal divider between DOM and tabs
                    if let Some(r) = tiny_skia::Rect::from_xywh(panel_x as f32, (chrome_px + dom_h) as f32 - 1.0, panel_w as f32, 1.0) {
                        let mut p = tiny_skia::Paint::default();
                        p.set_color(tiny_skia::Color::from_rgba8(50, 50, 55, 255));
                        pixmap.fill_rect(r, &p, Transform::identity(), None);
                    }
                }
            }

            // Chrome rendered on top
            if let Some(doc) = chrome_doc {
                let chrome_h = chrome_px.max(1);
                let needs_chrome_pm = self.chrome_pixmap.as_ref().is_none_or(|pm| {
                    pm.width() != w_px || pm.height() != chrome_h
                });
                if needs_chrome_pm {
                    self.chrome_pixmap = Pixmap::new(w_px, chrome_h);
                    self.chrome_pixmap_dirty = true;
                }
                if let Some(pm) = self.chrome_pixmap.as_mut() {
                    if self.chrome_pixmap_dirty {
                        pm.fill(tiny_skia::Color::from_rgba8(28, 28, 31, 255));
                        chrome_renderer.as_mut().expect("chrome renderer").render(doc, pm, scale);
                        self.chrome_pixmap_dirty = false;
                    }
                    pixmap.draw_pixmap(0, 0, pm.as_ref(),
                        &PixmapPaint::default(), Transform::identity(), None);
                }
            }

            // Bottom chrome separator line
            if let Some(r) = (chrome_px > 0).then(|| tiny_skia::Rect::from_xywh(0.0, chrome_px as f32 - 1.0,
                    w_px as f32, 1.0)).flatten() {
                let mut p = tiny_skia::Paint::default();
                p.set_color(tiny_skia::Color::from_rgba8(20, 20, 22, 255));
                pixmap.fill_rect(r, &p, Transform::identity(), None);
            }
        });
        let draw_ms = t_draw.elapsed().as_millis();
        if std::env::var_os("WEBCORE_TRACE_RENDER").is_some() && draw_ms > 5 {
            eprintln!("[browser] Draw total: {draw_ms}ms");
        }
        drop(profile_draw);
        self.trace_memory_if_requested();
        if crate::profile::is_enabled() {
            let now = std::time::Instant::now();
            if self.last_profile_trace.is_none_or(|last| {
                now.saturating_duration_since(last) >= std::time::Duration::from_secs(5)
            }) {
                self.last_profile_trace = Some(now);
                eprintln!("{}", crate::profile::summary());
            }
        }
    }

    fn trace_memory_if_requested(&mut self) {
        if std::env::var_os("WEBCORE_TRACE_MEMORY").is_none() {
            return;
        }
        let now = std::time::Instant::now();
        if self.last_memory_trace.is_some_and(|last| {
            now.saturating_duration_since(last) < std::time::Duration::from_secs(2)
        }) {
            return;
        }
        self.last_memory_trace = Some(now);
        let Some(tab) = self.tabs.get(self.active) else {
            return;
        };
        let stats = tab.view.memory_stats();
        let process = process_memory_stats();
        let rss = process.map(|p| p.rss_bytes).unwrap_or(0);
        let vsz = process.map(|p| p.vsz_bytes).unwrap_or(0);
        eprintln!(
            concat!(
                "[browser][mem] rss={}MiB vsz={}MiB ",
                "viewport={}MiB surfaces={}MiB tiles={}MiB ",
                "display={}MiB dl_img={}MiB raw_cache={}MiB css_cache={}MiB decoded_cache={}MiB ",
                "dom_img={}MiB dom={}MiB layout={}MiB style={}MiB sheets={}MiB ",
                "nodes={} images={} styles={}"
            ),
            rss / 1024 / 1024,
            vsz / 1024 / 1024,
            stats.viewport_surface_bytes / 1024 / 1024,
            (stats.renderer_cached_content_surface_bytes + stats.renderer_cached_surface_bytes)
                / 1024
                / 1024,
            stats.tile_surface_bytes / 1024 / 1024,
            stats.display_list_estimated_bytes / 1024 / 1024,
            stats.display_list_image_bytes / 1024 / 1024,
            stats.raw_resource_cache_bytes / 1024 / 1024,
            stats.parsed_css_cache_bytes / 1024 / 1024,
            stats.decoded_image_cache_bytes / 1024 / 1024,
            stats.decoded_dom_image_bytes / 1024 / 1024,
            stats.dom_estimated_bytes / 1024 / 1024,
            stats.layout_estimated_bytes / 1024 / 1024,
            stats.style_estimated_bytes / 1024 / 1024,
            stats.stylesheet_estimated_bytes / 1024 / 1024,
            stats.dom_nodes,
            stats.image_nodes,
            stats.unique_styles,
        );
    }
}

// ─── winit application handler ────────────────────────────────────────────────

impl ApplicationHandler<()> for BrowserApp {
    fn resumed(&mut self, el: &winit::event_loop::ActiveEventLoop) {
        let win = Arc::new(
            el.create_window(
                Window::default_attributes()
                    .with_title(&self.window_title)
                    .with_inner_size(winit::dpi::LogicalSize::new(
                        self.initial_width.max(1.0),
                        self.initial_height.max(1.0),
                    )),
            )
            .unwrap(),
        );
        let platform = Platform::new_windowed(win.clone());
        crate::embedded_window::set_active_window(Some(win.clone()));
        self.width = platform.logical_width();
        self.height = platform.logical_height();
        self.window = Some(win);
        self.platform = Some(platform);
        if self.tabs[self.active].history.is_empty() {
            let start_url = self
                .initial_url
                .take()
                .unwrap_or_else(|| self.new_tab_url.clone());
            self.navigate(start_url);
        } else {
            let page_width = self.page_width();
            let content_height = self.content_h();
            self.tabs[self.active]
                .view
                .resize(page_width, content_height);
            self.rebuild_chrome();
        }
        if let Some(guest) = self.guest.as_mut() {
            guest.context_ready();
        }
        if self.guest.is_some() {
            self.tabs[self.active].view.relayout();
        }
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn user_event(&mut self, el: &winit::event_loop::ActiveEventLoop, _: ()) {
        self.drain_commands();
        let debug_redraw = self.drain_debug_commands();
        if self.close_requested {
            el.exit();
            return;
        }
        if debug_redraw {
            if let Some(w) = &self.window {
                w.request_redraw();
            }
        }
    }

    fn about_to_wait(&mut self, el: &winit::event_loop::ActiveEventLoop) {
        let _idle_profile = crate::profile::span(crate::profile::Phase::BrowserIdle);
        self.drain_commands();
        if self.close_requested {
            el.exit();
            return;
        }
        let page_w = self.page_width();
        let content_h = self.content_h();
        self.tabs[self.active].view.resize(page_w, content_h);
        if let Some(context) = self.tabs[self.active].context_id {
            if crate::window::inner_width(context) != page_w as f64
                || crate::window::inner_height(context) != content_h as f64
            {
                crate::window::update_viewport(context, page_w as f64, content_h as f64);
            }
            if let Some(platform) = self.platform.as_ref() {
                crate::window::set_device_pixel_ratio(context, platform.scale_factor() as f64);
            }
        }
        let guest_changed = self
            .guest
            .as_mut()
            .is_some_and(|guest| guest.dispatch_pending());
        if self.tabs[self.active]
            .context_id
            .is_some_and(crate::window::closed)
        {
            el.exit();
            return;
        }
        if guest_changed {
            self.tabs[self.active].view.relayout();
        }
        let needs_redraw = self.tabs[self.active].view.drive_idle(el) || guest_changed;
        if let Some(delay) = self.guest.as_ref().and_then(|guest| guest.next_wakeup()) {
            let deadline = std::time::Instant::now() + delay;
            match el.control_flow() {
                ControlFlow::Poll => {}
                ControlFlow::WaitUntil(current) if current <= deadline => {}
                _ => el.set_control_flow(ControlFlow::WaitUntil(deadline)),
            }
        }
        let selection_profile = crate::profile::span(crate::profile::Phase::BrowserVideoSelection);
        let mut candidate = self
            .platform
            .as_ref()
            .filter(|p| p.supports_video_layer())
            .and_then(|_| {
                self.tabs[self.active]
                    .view
                    .document()
                    .as_deref()
                    .and_then(|doc| {
                        composited_video_candidate(doc, page_w, content_h, self.chrome_height)
                    })
            });
        if let Some((_, frame)) = &candidate {
            let opaque = if let Some((last, opaque)) = &self.native_video_opacity {
                if Arc::ptr_eq(last, &frame.rgba) {
                    *opaque
                } else {
                    frame.is_opaque()
                }
            } else {
                frame.is_opaque()
            };
            self.native_video_opacity = Some((frame.rgba.clone(), opaque));
            if !opaque {
                candidate = None;
            }
        } else {
            self.native_video_opacity = None;
        }
        if let Some((id, frame)) = candidate.as_mut() {
            let unobstructed = self.tabs[self.active]
                .view
                .document()
                .as_deref()
                .and_then(|doc| {
                    find_node(&doc.root, *id).map(|video| {
                        video_is_unobstructed(&doc.root, *id, video.layout.content_rect)
                    })
                })
                .unwrap_or(false);
            if unobstructed {
                self.native_video_composition = None;
            } else {
                let scale = self.platform.as_ref().unwrap().scale_factor();
                let valid = self
                    .native_video_composition
                    .as_ref()
                    .is_some_and(|cached| {
                        cached.tab == self.active
                            && cached.video_id == *id
                            && cached.page_w == page_w
                            && cached.page_h == content_h
                            && cached.scale == scale
                    });
                if needs_redraw || !valid {
                    self.native_video_composition = build_native_video_composition(
                        &mut self.tabs[self.active].view,
                        self.active,
                        *id,
                        Rect::new(
                            frame.x,
                            frame.y - self.chrome_height,
                            frame.width,
                            frame.height,
                        ),
                        page_w,
                        content_h,
                        scale,
                        self.chrome_height,
                    );
                }
                if let Some(composition) = &self.native_video_composition {
                    frame.tint = Some(composition.tint);
                    frame.foreground = Some(composition.foreground.clone());
                } else {
                    candidate = None;
                }
            }
        } else {
            self.native_video_composition = None;
        }
        let candidate_id = candidate.as_ref().map(|(id, _)| *id);
        let old_overlay_ids: Vec<u32> = {
            let document = self.tabs[self.active].view.document();
            let mut active_videos = Vec::new();
            if let Some(doc) = document.as_deref() {
                collect_videos(&doc.root, &mut active_videos);
            }
            active_videos
                .iter()
                .filter(|node| node.external_video_overlay)
                .map(|node| node.node_id)
                .collect()
        };
        let candidate_already_overlaid =
            candidate_id.is_some_and(|id| old_overlay_ids.contains(&id));
        let mut overlay_changed = false;
        for id in old_overlay_ids {
            if Some(id) != candidate_id {
                overlay_changed |= self.tabs[self.active]
                    .view
                    .set_external_video_overlay(id, false);
            }
        }
        if let Some(id) = candidate_id.filter(|_| !candidate_already_overlaid) {
            overlay_changed |= self.tabs[self.active]
                .view
                .set_external_video_overlay(id, true);
        }
        if let Some(platform) = self.platform.as_mut() {
            platform.present_video_layer(candidate.as_ref().map(|(_, frame)| frame));
        }
        drop(selection_profile);
        let previous_loading = self.tabs[self.active].loading;
        let previous_title = self.tabs[self.active].title.clone();
        self.tabs[self.active].loading = self.tabs[self.active].view.is_loading();
        if !self.tabs[self.active].view.title().is_empty() {
            self.tabs[self.active].title = self.tabs[self.active].view.title().to_string();
        }
        let chrome_changed = previous_loading != self.tabs[self.active].loading
            || previous_title != self.tabs[self.active].title;
        if self.tabs[self.active].url != self.tabs[self.active].view.url() {
            let url = self.tabs[self.active].view.url().to_string();
            {
                let tab = &mut self.tabs[self.active];
                let new_navigation = tab.view.take_document_navigation();
                tab.commit_navigation_url(url, new_navigation);
                tab.title = tab.view.title().to_string();
                tab.loading = tab.view.is_loading();
            }
            update_address_from_navigation(
                &mut self.url_text,
                &self.tabs[self.active].url,
                self.url_focused,
            );
            self.rebuild_chrome();
        } else if chrome_changed {
            self.rebuild_chrome();
            if let Some(window) = &self.window {
                window.request_redraw();
            }
        }
        if needs_redraw || overlay_changed {
            if let Some(w) = &self.window {
                w.request_redraw();
            }
        }
    }

    fn window_event(
        &mut self,
        el: &winit::event_loop::ActiveEventLoop,
        _wid: winit::window::WindowId,
        event: WindowEvent,
    ) {
        let redraw = self.on_event(el, event);
        if redraw {
            if let Some(w) = &self.window {
                w.request_redraw();
            }
        }
    }
}

impl BrowserApp {
    fn focus_address(&mut self) {
        if !self.chrome_settings.address_bar {
            return;
        }
        self.url_focused = true;
        self.url_text = self.tabs[self.active].url.clone();
        self.rebuild_chrome();
        if let Some(doc) = self.chrome_doc.as_mut() {
            if let Some(id) = doc.query_selector("#url-input") {
                doc.focus(id);
                doc.select(id);
            }
        }
        self.chrome_pixmap_dirty = true;
    }

    fn edit_address(
        &mut self,
        key: u32,
        ch: Option<char>,
        ctrl: bool,
        shift: bool,
        alt: bool,
        meta: bool,
    ) {
        if let Some(doc) = self.chrome_doc.as_mut() {
            if let Some(id) = doc.query_selector("#url-input") {
                doc.process_key_event(
                    HtmlEventType::KeyDown,
                    key,
                    ch,
                    ctrl || meta,
                    shift,
                    alt,
                    meta,
                );
                self.url_text = doc.value(id);
                self.chrome_renderer
                    .as_mut()
                    .expect("chrome renderer")
                    .invalidate_display_list();
                self.chrome_pixmap_dirty = true;
            }
        }
    }

    fn activate_chrome_hit(&mut self, hit: ChromeHit) {
        if !matches!(hit, ChromeHit::UrlBar) && self.url_focused {
            self.url_focused = false;
            self.url_text = self.tabs[self.active].url.clone();
            self.rebuild_chrome();
        }
        match hit {
            ChromeHit::Back => self.go_back(),
            ChromeHit::Forward => self.go_forward(),
            ChromeHit::Reload => self.reload(),
            ChromeHit::NewTab => self.new_tab(),
            ChromeHit::Tab(index) => self.switch_tab(index),
            ChromeHit::CloseTab(index) => self.close_tab(index),
            ChromeHit::UrlBar => self.focus_address(),
            ChromeHit::None => {}
        }
    }

    fn set_inspector(&mut self, visible: bool) {
        self.inspect_mode = visible;
        self.inspect_panel_pct = if visible { 0.35 } else { 0.0 };
        if !visible {
            self.inspect_node = 0;
        }
        self.tabs[self.active].view.set_inspect_mode(visible);
        self.relayout_active();
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn drain_commands(&mut self) {
        let commands: Vec<_> = self
            .commands
            .as_ref()
            .map(|rx| rx.try_iter().collect())
            .unwrap_or_default();
        if commands.is_empty() {
            return;
        }
        let mut redraw = false;
        for command in commands {
            redraw |= !matches!(
                command,
                super::BrowserCommand::Debug(..) | super::BrowserCommand::Close
            );
            match command {
                super::BrowserCommand::Navigate(url) => self.navigate(url),
                super::BrowserCommand::Back => self.go_back(),
                super::BrowserCommand::Forward => self.go_forward(),
                super::BrowserCommand::Reload => self.reload(),
                super::BrowserCommand::NewTab => self.new_tab(),
                super::BrowserCommand::ActivateTab(index) => self.switch_tab(index),
                super::BrowserCommand::CloseTab(index) => self.close_tab(index),
                super::BrowserCommand::Inspector(visible) => self.set_inspector(visible),
                super::BrowserCommand::Debug(request, reply) => {
                    let cmd = dbg_json_str(&request, "cmd").unwrap_or_default();
                    let response = self.handle_debug_command(&request);
                    redraw |=
                        debug_cmd_requests_redraw(&cmd) && response.starts_with(r#"{"ok":true"#);
                    let _ = reply.send(response);
                }
                super::BrowserCommand::Close => self.close_requested = true,
            }
        }
        if redraw {
            if let Some(window) = &self.window {
                window.request_redraw();
            }
        }
    }

    fn drain_debug_commands(&mut self) -> bool {
        let Some(rx) = &self.debug_cmd_rx else {
            return false;
        };
        let mut cmds: Vec<(String, mpsc::Sender<String>)> = Vec::new();
        while let Ok(pair) = rx.try_recv() {
            cmds.push(pair);
        }
        let mut redraw = false;
        for (line, reply_tx) in cmds {
            let cmd = dbg_json_str(&line, "cmd").unwrap_or_default();
            let resp = self.handle_debug_command(&line);
            redraw |= debug_cmd_requests_redraw(&cmd) && resp.starts_with(r#"{"ok":true"#);
            let _ = reply_tx.send(resp);
        }
        redraw
    }

    fn on_event(&mut self, el: &winit::event_loop::ActiveEventLoop, event: WindowEvent) -> bool {
        let scale = self
            .platform
            .as_ref()
            .map(Platform::scale_factor)
            .unwrap_or(1.0);
        let position = match &event {
            WindowEvent::CursorMoved { position, .. } => {
                (position.x as f32 / scale, position.y as f32 / scale)
            }
            _ => self.mouse_pos,
        };
        let keyboard = matches!(event, WindowEvent::KeyboardInput { .. });
        if (keyboard && !self.url_focused)
            || (!keyboard && position.1 >= self.chrome_height && position.0 < self.page_width())
        {
            super::input::publish(
                &event,
                (position.0, position.1 - self.chrome_height),
                scale,
                self.modifiers,
            );
        }
        // Track modifier keys (Shift, Ctrl) via the renderer
        if matches!(event, WindowEvent::ModifiersChanged(_)) {
            self.tabs[self.active].view.handle_window_event(&event);
        }
        match event {
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
                false
            }
            WindowEvent::CloseRequested => {
                crate::embedded_window::set_active_window(None);
                el.exit();
                false
            }

            WindowEvent::Resized(sz) => {
                if let Some(p) = self.platform.as_mut() {
                    p.resize(sz.width, sz.height);
                }
                if let Some(p) = self.platform.as_ref() {
                    self.width = p.logical_width();
                    self.height = p.logical_height();
                }
                self.rebuild_chrome();
                self.relayout_active();
                true
            }

            WindowEvent::CursorMoved { position, .. } => {
                let sf = self
                    .platform
                    .as_ref()
                    .map(|p| p.scale_factor())
                    .unwrap_or(1.0);
                let (sx, sy) = (position.x as f32 / sf, position.y as f32 / sf);
                self.mouse_pos = (sx, sy);
                // Inspector vertical splitter drag
                if self.inspect_dragging {
                    self.inspect_panel_pct = (1.0 - sx / self.width).clamp(0.15, 0.7);
                    // Re-layout page at new width
                    let pw = self.page_width();
                    let ch = self.content_h();
                    self.tabs[self.active].view.resize(pw, ch);
                    self.tabs[self.active].view.relayout();
                    return true;
                }
                if sy >= self.chrome_height {
                    let csy = sy - self.chrome_height;
                    if self.tabs[self.active].view.handle_mouse_move(sx, csy) {
                        return true;
                    }
                    let ci = self.tabs[self.active].view.cursor_at(sx, csy);
                    if let Some(w) = &self.window {
                        use winit::window::CursorIcon;
                        let icon = match ci {
                            crate::CSSCursor::Pointer => CursorIcon::Pointer,
                            crate::CSSCursor::Text => CursorIcon::Text,
                            crate::CSSCursor::Move => CursorIcon::Move,
                            crate::CSSCursor::NotAllowed => CursorIcon::NotAllowed,
                            crate::CSSCursor::Grab => CursorIcon::Grab,
                            crate::CSSCursor::Grabbing => CursorIcon::Grabbing,
                            crate::CSSCursor::ColResize => CursorIcon::ColResize,
                            crate::CSSCursor::RowResize => CursorIcon::RowResize,
                            crate::CSSCursor::Crosshair => CursorIcon::Crosshair,
                            crate::CSSCursor::Help => CursorIcon::Help,
                            crate::CSSCursor::Wait => CursorIcon::Wait,
                            _ => CursorIcon::Default,
                        };
                        w.set_cursor(winit::window::Cursor::Icon(icon));
                    }
                }
                false
            }

            WindowEvent::CursorLeft { .. } => {
                self.tabs[self.active].view.handle_mouse_move(-1.0, -1.0)
            }

            WindowEvent::MouseInput { state, button, .. } => {
                let bt: u8 = match button {
                    MouseButton::Left => 0,
                    MouseButton::Middle => 1,
                    MouseButton::Right => 2,
                    _ => 0,
                };
                let (sx, sy) = self.mouse_pos;

                if state == ElementState::Pressed {
                    // Inspector vertical splitter drag start
                    if self.inspect_mode && bt == 0 {
                        let splitter_x = self.page_width();
                        if (sx - splitter_x).abs() < 5.0 && sy >= self.chrome_height {
                            self.inspect_dragging = true;
                            return true;
                        }
                    }
                    if sy < self.chrome_height {
                        let hit = self.chrome_hit(sx, sy);
                        if bt == 0 {
                            self.activate_chrome_hit(hit);
                        }
                    } else {
                        // Content area
                        if self.url_focused {
                            self.url_focused = false;
                            self.url_text = self.tabs[self.active].url.clone();
                            self.rebuild_chrome();
                        }
                        // If clicking in the inspector panel (right of page)
                        let pw = self.page_width();
                        if self.inspect_mode && sx > pw + 5.0 {
                            let panel_y = sy - self.chrome_height;
                            let content_h = self.content_h();
                            let dom_h = content_h * self.inspect_dom_split;
                            let panel_x = sx - pw;

                            if panel_y < dom_h {
                                // Click in DOM tree — select element by line (account for scroll)
                                let line_idx =
                                    ((panel_y + self.inspect_dom_scroll) / 16.0) as usize;
                                if let Some(doc) = self.tabs[self.active].view.document().as_deref()
                                {
                                    let mut nodes: Vec<u32> = Vec::new();
                                    collect_dom_node_ids(&doc.root, &mut nodes, 0, 20);
                                    if line_idx < nodes.len() {
                                        self.inspect_node = nodes[line_idx];
                                    }
                                }
                            } else {
                                // Click in tabs area
                                let tabs_y = panel_y - dom_h;
                                // Element bar ~28px, then tab bar ~26px
                                if tabs_y >= 28.0 && tabs_y < 56.0 {
                                    // 6 tabs: Styles | Computed | Box Model | DOM | Layout | Attrs
                                    let pw = self.width * self.inspect_panel_pct;
                                    let tab_w = pw / 6.0;
                                    self.inspect_tab = (panel_x / tab_w).min(5.0) as u8;
                                }
                            }
                            return true;
                        }
                        let csy = sy - self.chrome_height;
                        if bt == 0
                            && self.tabs[self.active]
                                .view
                                .handle_mouse_button_with_modifiers(
                                    HtmlEventType::MouseDown,
                                    sx,
                                    csy,
                                    bt,
                                    self.modifiers.control_key(),
                                    self.modifiers.shift_key(),
                                    self.modifiers.alt_key(),
                                    self.modifiers.super_key(),
                                )
                        {
                            return true;
                        }
                        // Deferred inspect setup (right-click)
                        if bt == 2 {
                            // First pass: read-only to get hit target
                            let hit_nid = {
                                if let Some(doc) = self.tabs[self.active].view.document().as_deref()
                                {
                                    let doc_pt = (sx + doc.scroll_x, csy + doc.scroll_y);
                                    crate::layout::hit_test::point_to_hit_scrolled(
                                        &doc.root,
                                        doc_pt,
                                        (doc.scroll_x, doc.scroll_y),
                                        2,
                                    )
                                    .map(|h| h.node_id)
                                    .filter(|&id| id != 0 && doc.get_box_by_id(id).is_some())
                                } else {
                                    None
                                }
                            };
                            if let Some(nid) = hit_nid {
                                self.inspect_node = nid;
                                self.inspect_mode = true;
                                if self.inspect_panel_pct < 0.15 {
                                    self.inspect_panel_pct = 0.35;
                                }
                                self.tabs[self.active].view.set_inspect_mode(true);
                            }
                        }
                    }
                    return true;
                } else {
                    // Released
                    if self.inspect_dragging {
                        self.inspect_dragging = false;
                        return true;
                    }
                    if sy >= self.chrome_height {
                        let csy = sy - self.chrome_height;
                        self.tabs[self.active]
                            .view
                            .handle_mouse_button_with_modifiers(
                                HtmlEventType::MouseUp,
                                sx,
                                csy,
                                bt,
                                self.modifiers.control_key(),
                                self.modifiers.shift_key(),
                                self.modifiers.alt_key(),
                                self.modifiers.super_key(),
                            );
                    }
                    return true;
                }
            }

            WindowEvent::MouseWheel { delta, .. } => {
                let (_, sy) = self.mouse_pos;
                if sy >= self.chrome_height {
                    let sf = self
                        .platform
                        .as_ref()
                        .map(|p| p.scale_factor())
                        .unwrap_or(1.0);
                    let (dx, dy) = match delta {
                        winit::event::MouseScrollDelta::LineDelta(x, y) => (
                            -x * super::input::WHEEL_LINE_SCROLL_PIXELS,
                            -y * super::input::WHEEL_LINE_SCROLL_PIXELS,
                        ),
                        winit::event::MouseScrollDelta::PixelDelta(p) => {
                            (-(p.x as f32) / sf, -(p.y as f32) / sf)
                        }
                    };
                    self.tabs[self.active].view.handle_wheel(dx, dy);
                    return true;
                }
                false
            }

            WindowEvent::KeyboardInput { event, .. } => {
                if event.state != ElementState::Pressed {
                    if self.url_focused {
                        return false;
                    }
                    let (_, _, key_code) = crate::embedded_window::key_fields(&event.logical_key);
                    let ch = match &event.logical_key {
                        Key::Character(text) => text.chars().next(),
                        Key::Named(NamedKey::Space) => Some(' '),
                        _ => None,
                    };
                    return self.tabs[self.active].view.handle_key(
                        HtmlEventType::KeyUp,
                        key_code as u32,
                        ch,
                        self.modifiers.control_key(),
                        self.modifiers.shift_key(),
                        self.modifiers.alt_key(),
                        self.modifiers.super_key(),
                    );
                }
                if self.modifiers.control_key() || self.modifiers.super_key() {
                    if let Key::Character(ch) = &event.logical_key {
                        if ch.eq_ignore_ascii_case("l") {
                            self.focus_address();
                            return true;
                        }
                        if ch.eq_ignore_ascii_case("t") && self.chrome_settings.tabs {
                            self.new_tab();
                            return true;
                        }
                        if ch.eq_ignore_ascii_case("w") && self.chrome_settings.tabs {
                            self.close_tab(self.active);
                            return true;
                        }
                    }
                }
                if self.url_focused {
                    match &event.logical_key {
                        Key::Named(NamedKey::Enter) => {
                            let url = self.url_text.clone();
                            self.navigate(url);
                        }
                        Key::Named(NamedKey::Escape) => {
                            self.url_focused = false;
                            self.url_text = self.tabs[self.active].url.clone();
                            self.rebuild_chrome();
                        }
                        key => {
                            let code = match key {
                                Key::Named(NamedKey::Backspace) => 8,
                                Key::Named(NamedKey::Delete) => 46,
                                Key::Named(NamedKey::ArrowLeft) => 37,
                                Key::Named(NamedKey::ArrowRight) => 39,
                                Key::Named(NamedKey::Home) => 36,
                                Key::Named(NamedKey::End) => 35,
                                _ => 0,
                            };
                            let text = match key {
                                Key::Character(text) => Some(text.as_str()),
                                _ => None,
                            };
                            if let Some(text) = text {
                                for ch in text.chars() {
                                    self.edit_address(
                                        code,
                                        Some(ch),
                                        self.modifiers.control_key(),
                                        self.modifiers.shift_key(),
                                        self.modifiers.alt_key(),
                                        self.modifiers.super_key(),
                                    );
                                }
                            } else {
                                self.edit_address(
                                    code,
                                    None,
                                    self.modifiers.control_key(),
                                    self.modifiers.shift_key(),
                                    self.modifiers.alt_key(),
                                    self.modifiers.super_key(),
                                );
                            }
                        }
                    }
                    return true;
                }
                // Global shortcuts
                match &event.logical_key {
                    Key::Named(NamedKey::BrowserBack) => {
                        self.go_back();
                        return true;
                    }
                    Key::Named(NamedKey::BrowserForward) => {
                        self.go_forward();
                        return true;
                    }
                    Key::Named(NamedKey::BrowserRefresh) => {
                        self.reload();
                        return true;
                    }
                    Key::Named(NamedKey::F12) => {
                        self.inspect_mode = !self.inspect_mode;
                        self.inspect_panel_pct = if self.inspect_mode { 0.35 } else { 0.0 };
                        if !self.inspect_mode {
                            self.inspect_node = 0;
                        }
                        self.tabs[self.active]
                            .view
                            .set_inspect_mode(self.inspect_mode);
                        return true;
                    }
                    Key::Named(NamedKey::Escape) if self.inspect_mode => {
                        self.inspect_mode = false;
                        self.inspect_panel_pct = 0.0;
                        self.inspect_node = 0;
                        self.tabs[self.active].view.set_inspect_mode(false);
                        return true;
                    }
                    _ => {}
                }
                // Route keyboard input through the browser view. The shell
                // translates platform keys; webcore owns focused controls.
                let ch = match &event.logical_key {
                    Key::Character(s) => s.chars().next(),
                    Key::Named(NamedKey::Space) => Some(' '),
                    Key::Named(NamedKey::Tab) => Some('\t'),
                    _ => None,
                };
                let (_, _, kc) = crate::embedded_window::key_fields(&event.logical_key);
                let kc = kc as u32;
                if kc != 0 || ch.is_some() {
                    let effective_kc = if kc != 0 {
                        kc
                    } else {
                        ch.unwrap_or(' ') as u32
                    };
                    let mut changed = self.tabs[self.active].view.handle_key(
                        crate::dom::HtmlEventType::KeyDown,
                        effective_kc,
                        ch,
                        self.modifiers.control_key(),
                        self.modifiers.shift_key(),
                        self.modifiers.alt_key(),
                        self.modifiers.super_key(),
                    );
                    if ch.is_some_and(|ch| !ch.is_control())
                        && !self.modifiers.control_key()
                        && !self.modifiers.super_key()
                    {
                        changed |= self.tabs[self.active].view.handle_key(
                            HtmlEventType::KeyPress,
                            effective_kc,
                            ch,
                            false,
                            self.modifiers.shift_key(),
                            self.modifiers.alt_key(),
                            false,
                        );
                    }
                    return changed;
                }
                false
            }

            WindowEvent::RedrawRequested => {
                self.draw();
                false
            }

            _ => false,
        }
    }
}

// ─── Helper functions ─────────────────────────────────────────────────────────

/// Return a deterministic accent color for a domain from a curated palette.
fn domain_color(url: &str) -> &'static str {
    const PALETTE: &[&str] = &[
        "#4285F4", "#EA4335", "#34A853", "#FBBC05", "#FF6D00", "#7C4DFF", "#00BCD4", "#E91E63",
        "#795548", "#5E81AC", "#3F51B5", "#009688", "#FF5722", "#8BC34A", "#CE422B",
    ];
    let d = extract_domain(url);
    let h = d.bytes().fold(5381usize, |a, b| {
        a.wrapping_mul(33).wrapping_add(b as usize)
    });
    PALETTE[h % PALETTE.len()]
}

/// First letter of the domain, uppercased — used as a favicon substitute.
fn domain_letter(url: &str) -> String {
    if url == NEW_TAB_URL || url.starts_with("about:") || url.is_empty() {
        return "+".to_string();
    }
    extract_domain(url)
        .chars()
        .next()
        .map(|c| c.to_ascii_uppercase().to_string())
        .unwrap_or_else(|| "?".to_string())
}

/// Generate a small color swatch if the value looks like a color.
fn extract_domain(url: &str) -> &str {
    let s = url
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("file://");
    s.split('/').next().unwrap_or(s)
}

/// Strip scheme for display; return empty string for new-tab / about pages.
fn pretty_url(url: &str) -> String {
    if url == NEW_TAB_URL || url.starts_with("about:") || url.is_empty() {
        return String::new();
    }
    let s = url
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    escape_html(s.trim_end_matches('/'))
}

/// Normalise a user-typed string into a full URL.
fn normalize_url(s: String) -> String {
    let s = s.trim().to_string();
    if s.is_empty() {
        return NEW_TAB_URL.to_string();
    }
    if s.starts_with("http://")
        || s.starts_with("https://")
        || s.starts_with("file://")
        || s.starts_with("about:")
    {
        return s;
    }
    if !s.chars().any(char::is_whitespace) {
        if let Ok(local) = reqwest::Url::parse(&format!("http://{s}")) {
            let loopback = local.host_str().is_some_and(|host| {
                host == "localhost"
                    || host.ends_with(".localhost")
                    || host
                        .trim_matches(['[', ']'])
                        .parse::<std::net::IpAddr>()
                        .is_ok_and(|address| address.is_loopback())
            });
            if loopback {
                return format!("http://{s}");
            }
        }
    }
    // Looks like a hostname?
    if !s.chars().any(char::is_whitespace) && s.contains('.') {
        return format!("https://{s}");
    }
    // Search query
    let mut search = reqwest::Url::parse("https://duckduckgo.com/").expect("valid search URL");
    search.query_pairs_mut().append_pair("q", &s);
    search.into()
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
