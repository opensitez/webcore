//! Phoenix Browser — a full browser demo built on the webcore engine.
//! Features: tabs, back/forward history, URL bar, async page/CSS/image loading,
//! link navigation, per-element scrolling, zoom, and an optional TCP debug server.
//!
//! Pass `--debug-port <n>` (e.g. `--debug-port 9222`) to enable the remote debug
//! server. All commands from debugserver.rs are supported (screenshot, find,
//! inspect, click, hover, type, key, scroll, navigate, tree, …); plus two
//! browser-specific commands: `tabs` and `switch-tab`. Use debugclient.sh or
//! debugclient.py to connect.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::{Arc, mpsc};

use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, NamedKey};
use winit::window::Window;

use tiny_skia::{Pixmap, PixmapPaint, Transform};

use webcore::dom::{self, HtmlEventType};
use webcore::platform::Platform;
use webcore::renderer::display_list::{ImageRef, PaintCmd};
use webcore::renderer::display_list_builder::{
    build_display_list_full_with_font_system, build_display_list_viewport,
};
use webcore::types::{Display, Overflow, Position};
use webcore::{Document, Renderer, parse_html_with_hooks, point_to_hit};

// ─── Layout constants ─────────────────────────────────────────────────────────

const CHROME_H: f32 = 80.0; // tab-bar (36) + nav-bar (44)
const TAB_MAX_W: f32 = 220.0;
const TAB_MIN_W: f32 = 80.0;

// ─── New-tab page ─────────────────────────────────────────────────────────────

const NEW_TAB_URL: &str = "about:newtab";

// ─── Tab ──────────────────────────────────────────────────────────────────────

struct Tab {
    url: String,
    title: String,
    history: Vec<String>,
    hist_i: usize,
    view: webcore::BrowserView,
    loading: bool,
}

impl Tab {
    fn new(proxy: EventLoopProxy<()>) -> Self {
        let mut view = webcore::BrowserView::new(1280.0, 720.0, Default::default());
        view.set_wake_callback(move || {
            let _ = proxy.send_event(());
        });
        Self {
            url: String::new(),
            title: "New Tab".into(),
            history: vec![],
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

    chrome_renderer: Renderer, // browser chrome
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
    cache_dir: Option<String>, // Some("snapshot_cache") when --cached
    no_images: bool,

    // Remote debug server (--debug-port)
    debug_cmd_rx: Option<mpsc::Receiver<(String, mpsc::Sender<String>)>>,
    last_memory_trace: Option<std::time::Instant>,
}

impl BrowserApp {
    fn new(proxy: EventLoopProxy<()>) -> Self {
        Self {
            window: None,
            platform: None,
            chrome_renderer: Renderer::new(),
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
            cache_dir: None,
            no_images: false,
            debug_cmd_rx: None,
            last_memory_trace: None,
        }
    }

    fn content_h(&self) -> f32 {
        (self.height - CHROME_H).max(0.0)
    }
    fn page_width(&self) -> f32 {
        if self.inspect_mode {
            (self.width * (1.0 - self.inspect_panel_pct)).max(100.0)
        } else {
            self.width
        }
    }

    fn start_view_navigation(&mut self, index: usize, url: String) {
        let page_w = self.page_width();
        let content_h = self.content_h();
        let cache_dir = self.cache_dir.clone();
        let load_images = !self.no_images;
        let tab = &mut self.tabs[index];
        tab.view.resize(page_w, content_h);
        tab.view.set_options(webcore::PageLoadOptions {
            cache_dir,
            load_images,
            ..Default::default()
        });
        tab.view.navigate(url);
    }

    // ── Navigation ────────────────────────────────────────────────────────────

    fn navigate(&mut self, url: String) {
        let url = normalize_url(url);
        let tab = &mut self.tabs[self.active];
        if !tab.history.is_empty() {
            tab.history.truncate(tab.hist_i + 1);
        }
        tab.history.push(url.clone());
        tab.hist_i = tab.history.len() - 1;
        tab.url = url.clone();
        tab.title = "Loading…".into();
        tab.loading = true;
        self.url_text = url.clone();
        self.url_focused = false;
        self.rebuild_chrome();
        self.start_view_navigation(self.active, url);
    }

    fn go_back(&mut self) {
        let tab = &mut self.tabs[self.active];
        if !tab.can_back() {
            return;
        }
        tab.hist_i -= 1;
        let url = tab.history[tab.hist_i].clone();
        tab.url = url.clone();
        tab.loading = true;
        self.url_text = url.clone();
        self.rebuild_chrome();
        self.start_view_navigation(self.active, url);
    }

    fn go_forward(&mut self) {
        let tab = &mut self.tabs[self.active];
        if !tab.can_forward() {
            return;
        }
        tab.hist_i += 1;
        let url = tab.history[tab.hist_i].clone();
        tab.url = url.clone();
        tab.loading = true;
        self.url_text = url.clone();
        self.rebuild_chrome();
        self.start_view_navigation(self.active, url);
    }

    fn reload(&mut self) {
        let url = self.tabs[self.active].url.clone();
        if url.is_empty() {
            return;
        }
        self.tabs[self.active].loading = true;
        self.rebuild_chrome();
        self.start_view_navigation(self.active, url);
    }

    fn new_tab(&mut self) {
        self.tabs.push(Tab::new(self.proxy.clone()));
        self.active = self.tabs.len() - 1;
        self.navigate(NEW_TAB_URL.to_string());
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
        if self.tabs.len() == 1 {
            self.tabs[0] = Tab::new(self.proxy.clone());
            self.navigate(NEW_TAB_URL.to_string());
            return;
        }
        self.tabs.remove(i);
        if self.active >= self.tabs.len() {
            self.active = self.tabs.len() - 1;
        }
        self.url_text = self.tabs[self.active].url.clone();
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
        let html = self.chrome_html();
        let mut doc = parse_html_with_hooks(&html, "", |_, _| {});
        let w = self.width;
        let eng = self.chrome_renderer.layout_engine();
        eng.viewport_h = CHROME_H;
        eng.layout(&mut doc, w);
        self.chrome_doc = Some(doc);
        self.chrome_pixmap_dirty = true;
    }

    fn chrome_html(&self) -> String {
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
        let caret = if self.url_focused {
            "<span class='cur'>|</span>"
        } else {
            ""
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
      background:#1C1C1F;display:flex;flex-direction:column;height:{CHROME_H}px;overflow:hidden}}
#tab-bar{{display:flex;align-items:flex-end;height:36px;background:#141416;
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
#nav-bar{{display:flex;align-items:center;height:44px;background:#1C1C1F;padding:0 10px;gap:4px;
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
.url-t{{flex:1;overflow:hidden;white-space:nowrap;color:#C9CBD0}}
.urlbar.focused .url-t{{color:#E8EAED}}
.cur{{color:#5E9CF8}}
#ext-btn{{width:30px;height:30px;border-radius:50%;color:#6E7077;font-size:19px;
          display:flex;align-items:center;justify-content:center;cursor:pointer;flex-shrink:0}}
</style></head><body>
<div id="tab-bar">{tabs_html}<div id="btn-new-tab">+</div></div>
<div id="nav-bar">
  <div class="{bd}" id="btn-back">&#8592;</div>
  <div class="{fd}" id="btn-fwd">&#8594;</div>
  <div class="btn" id="btn-reload">&#8635;</div>
  <div class="{url_cls}" id="url-bar">{sec}<span class="url-t">{url_txt}{caret}</span></div>
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
        let Some(platform) = self.platform.as_mut() else {
            return;
        };

        let chrome_renderer = &mut self.chrome_renderer;
        let chrome_doc = self.chrome_doc.as_mut();

        let t_draw = std::time::Instant::now();
        platform.render(|scale, pixmap| {
            pixmap.fill(tiny_skia::Color::from_rgba8(28, 28, 31, 255));

            let w_px = pixmap.width();
            let h_px = pixmap.height();
            let chrome_px  = ((CHROME_H * scale) as u32).min(h_px);
            let content_px = h_px.saturating_sub(chrome_px);

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

            if let Some(doc) = self.tabs[self.active].view.document() {
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
                    let mut dom_doc = webcore::parse_html(&dom_html);
                    { let eng = chrome_renderer.layout_engine(); eng.layout(&mut dom_doc, panel_w_logical); }
                    // Scroll DOM tree to selected element
                    if let Some(line) = selected_line {
                        let target_y = line as f32 * 16.0;
                        let visible_h = dom_h as f32 / scale;
                        *inspect_dom_scroll = (target_y - visible_h * 0.3).max(0.0);
                    }
                    dom_doc.scroll_y = *inspect_dom_scroll;
                    if let Some(mut pm) = Pixmap::new(panel_w, dom_h.max(1)) {
                        pm.fill(tiny_skia::Color::from_rgba8(30, 30, 30, 255));
                        chrome_renderer.render(&mut dom_doc, &mut pm, scale);
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
                    let mut tabs_doc = webcore::parse_html(&tabs_html);
                    { let eng = chrome_renderer.layout_engine(); eng.layout(&mut tabs_doc, panel_w_logical); }
                    if let Some(mut pm) = Pixmap::new(panel_w, tabs_h.max(1)) {
                        pm.fill(tiny_skia::Color::from_rgba8(30, 30, 30, 255));
                        chrome_renderer.render(&mut tabs_doc, &mut pm, scale);
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
                        chrome_renderer.render(doc, pm, scale);
                        self.chrome_pixmap_dirty = false;
                    }
                    pixmap.draw_pixmap(0, 0, pm.as_ref(),
                        &PixmapPaint::default(), Transform::identity(), None);
                }
            }

            // Bottom chrome separator line
            if let Some(r) = tiny_skia::Rect::from_xywh(0.0, chrome_px as f32 - 1.0,
                    w_px as f32, 1.0) {
                let mut p = tiny_skia::Paint::default();
                p.set_color(tiny_skia::Color::from_rgba8(20, 20, 22, 255));
                pixmap.fill_rect(r, &p, Transform::identity(), None);
            }
        });
        let draw_ms = t_draw.elapsed().as_millis();
        if std::env::var_os("WEBCORE_TRACE_RENDER").is_some() && draw_ms > 5 {
            eprintln!("[browser] Draw total: {draw_ms}ms");
        }
        self.trace_memory_if_requested();
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
                    .with_title("Phoenix Browser")
                    .with_inner_size(winit::dpi::LogicalSize::new(
                        self.initial_width.max(1.0),
                        self.initial_height.max(1.0),
                    )),
            )
            .unwrap(),
        );
        let platform = Platform::new_windowed(win.clone());
        self.width = platform.logical_width();
        self.height = platform.logical_height();
        self.window = Some(win);
        self.platform = Some(platform);
        let start_url = self
            .initial_url
            .take()
            .unwrap_or_else(|| NEW_TAB_URL.to_string());
        self.navigate(start_url);
    }

    fn user_event(&mut self, _el: &winit::event_loop::ActiveEventLoop, _: ()) {
        let debug_redraw = self.drain_debug_commands();
        if debug_redraw {
            if let Some(w) = &self.window {
                w.request_redraw();
            }
        }
    }

    fn about_to_wait(&mut self, el: &winit::event_loop::ActiveEventLoop) {
        let page_w = self.page_width();
        let content_h = self.content_h();
        self.tabs[self.active].view.resize(page_w, content_h);
        let needs_redraw = self.tabs[self.active].view.drive_idle(el);
        self.tabs[self.active].loading = self.tabs[self.active].view.is_loading();
        if !self.tabs[self.active].view.title().is_empty() {
            self.tabs[self.active].title = self.tabs[self.active].view.title().to_string();
        }
        if self.tabs[self.active].url != self.tabs[self.active].view.url() {
            let url = self.tabs[self.active].view.url().to_string();
            {
                let tab = &mut self.tabs[self.active];
                if tab.history.last() != Some(&url) {
                    tab.history.truncate(tab.hist_i + 1);
                    tab.history.push(url.clone());
                    tab.hist_i = tab.history.len() - 1;
                }
                tab.url = url;
                tab.title = tab.view.title().to_string();
                tab.loading = tab.view.is_loading();
            }
            self.url_text = self.tabs[self.active].url.clone();
            self.rebuild_chrome();
        }
        if needs_redraw {
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
        // Track modifier keys (Shift, Ctrl) via the renderer
        if matches!(event, WindowEvent::ModifiersChanged(_)) {
            self.tabs[self.active].view.handle_window_event(&event);
        }
        match event {
            WindowEvent::CloseRequested => {
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
                if sy >= CHROME_H {
                    let csy = sy - CHROME_H;
                    if self.tabs[self.active].view.handle_mouse_move(sx, csy) {
                        return true;
                    }
                    let ci = self.tabs[self.active].view.cursor_at(sx, csy);
                    if let Some(w) = &self.window {
                        use winit::window::CursorIcon;
                        let icon = match ci {
                            webcore::CSSCursor::Pointer => CursorIcon::Pointer,
                            webcore::CSSCursor::Text => CursorIcon::Text,
                            webcore::CSSCursor::Move => CursorIcon::Move,
                            webcore::CSSCursor::NotAllowed => CursorIcon::NotAllowed,
                            webcore::CSSCursor::Grab => CursorIcon::Grab,
                            webcore::CSSCursor::Grabbing => CursorIcon::Grabbing,
                            webcore::CSSCursor::ColResize => CursorIcon::ColResize,
                            webcore::CSSCursor::RowResize => CursorIcon::RowResize,
                            webcore::CSSCursor::Crosshair => CursorIcon::Crosshair,
                            webcore::CSSCursor::Help => CursorIcon::Help,
                            webcore::CSSCursor::Wait => CursorIcon::Wait,
                            _ => CursorIcon::Default,
                        };
                        w.set_cursor(winit::window::Cursor::Icon(icon));
                    }
                }
                false
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
                        if (sx - splitter_x).abs() < 5.0 && sy >= CHROME_H {
                            self.inspect_dragging = true;
                            return true;
                        }
                    }
                    if sy < CHROME_H {
                        // Unfocus URL bar if clicking outside it
                        let hit = self.chrome_hit(sx, sy);
                        if !matches!(hit, ChromeHit::UrlBar) && self.url_focused {
                            self.url_focused = false;
                            self.url_text = self.tabs[self.active].url.clone();
                            self.rebuild_chrome();
                        }
                        match hit {
                            ChromeHit::Back => {
                                self.go_back();
                            }
                            ChromeHit::Forward => {
                                self.go_forward();
                            }
                            ChromeHit::Reload => {
                                self.reload();
                            }
                            ChromeHit::NewTab => {
                                self.new_tab();
                            }
                            ChromeHit::Tab(i) => {
                                self.switch_tab(i);
                            }
                            ChromeHit::CloseTab(i) => {
                                self.close_tab(i);
                            }
                            ChromeHit::UrlBar => {
                                if !self.url_focused {
                                    self.url_focused = true;
                                    self.url_text = self.tabs[self.active].url.clone();
                                    self.rebuild_chrome();
                                }
                            }
                            ChromeHit::None => {}
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
                            let panel_y = sy - CHROME_H;
                            let content_h = self.content_h();
                            let dom_h = content_h * self.inspect_dom_split;
                            let panel_x = sx - pw;

                            if panel_y < dom_h {
                                // Click in DOM tree — select element by line (account for scroll)
                                let line_idx =
                                    ((panel_y + self.inspect_dom_scroll) / 16.0) as usize;
                                if let Some(doc) = self.tabs[self.active].view.document() {
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
                        let csy = sy - CHROME_H;
                        if bt == 0
                            && self.tabs[self.active].view.handle_mouse_button(
                                HtmlEventType::MouseDown,
                                sx,
                                csy,
                                bt,
                            )
                        {
                            return true;
                        }
                        // Deferred inspect setup (right-click)
                        if bt == 2 {
                            // First pass: read-only to get hit target
                            let hit_nid = {
                                if let Some(doc) = self.tabs[self.active].view.document() {
                                    let doc_pt = (sx + doc.scroll_x, csy + doc.scroll_y);
                                    point_to_hit(&doc.root, doc_pt, 2)
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
                    if sy >= CHROME_H {
                        let csy = sy - CHROME_H;
                        self.tabs[self.active].view.handle_mouse_button(
                            HtmlEventType::MouseUp,
                            sx,
                            csy,
                            bt,
                        );
                    }
                    return true;
                }
            }

            WindowEvent::MouseWheel { delta, .. } => {
                let (_, sy) = self.mouse_pos;
                if sy >= CHROME_H {
                    let sf = self
                        .platform
                        .as_ref()
                        .map(|p| p.scale_factor())
                        .unwrap_or(1.0);
                    let dy = match delta {
                        winit::event::MouseScrollDelta::LineDelta(_, y) => -y * 40.0,
                        winit::event::MouseScrollDelta::PixelDelta(p) => -(p.y as f32) / sf,
                    };
                    self.tabs[self.active].view.handle_wheel(0.0, dy);
                    return true;
                }
                false
            }

            WindowEvent::KeyboardInput { event, .. } => {
                if event.state != ElementState::Pressed {
                    return false;
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
                        Key::Named(NamedKey::Backspace) => {
                            self.url_text.pop();
                            self.rebuild_chrome();
                        }
                        Key::Character(s) => {
                            self.url_text.push_str(s);
                            self.rebuild_chrome();
                        }
                        _ => {}
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
                let kc = match &event.logical_key {
                    Key::Named(NamedKey::Backspace) => 8,
                    Key::Named(NamedKey::Delete) => 46,
                    Key::Named(NamedKey::Enter) => 13,
                    Key::Named(NamedKey::ArrowLeft) => 37,
                    Key::Named(NamedKey::ArrowRight) => 39,
                    Key::Named(NamedKey::Home) => 36,
                    Key::Named(NamedKey::End) => 35,
                    Key::Named(NamedKey::Space) => 32,
                    Key::Character(_) => 0,
                    _ => 0,
                };
                if kc != 0 || ch.is_some() {
                    let shifted = self.tabs[self.active].view.is_shift_held();
                    let effective_kc = if kc != 0 {
                        kc
                    } else {
                        ch.unwrap_or(' ') as u32
                    };
                    if self.tabs[self.active].view.handle_key(
                        webcore::dom::HtmlEventType::KeyDown,
                        effective_kc,
                        ch,
                        false,
                        shifted,
                        false,
                        false,
                    ) {
                        return true;
                    }
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
fn color_swatch(val: &str) -> String {
    let v = val.trim();
    let is_color = v.starts_with('#') && (v.len() == 4 || v.len() == 7 || v.len() == 9)
        || v.starts_with("rgb")
        || v.starts_with("hsl")
        || matches!(
            v,
            "red"
                | "blue"
                | "green"
                | "white"
                | "black"
                | "gray"
                | "grey"
                | "orange"
                | "yellow"
                | "purple"
                | "pink"
                | "cyan"
                | "transparent"
        );
    if is_color && v != "transparent" {
        format!(
            "<span style='display:inline-block;width:10px;height:10px;border:1px solid #555;\
                 background:{};vertical-align:middle;margin-right:3px;border-radius:2px'></span>",
            escape_html(v)
        )
    } else {
        String::new()
    }
}

/// Collect one node_id per rendered line in the DOM tree.
/// Must exactly match the line output order of `build_dom_tree_html`.
fn collect_dom_node_ids(
    node: &webcore::WebCore,
    out: &mut Vec<u32>,
    depth: usize,
    max_depth: usize,
) {
    if depth > max_depth {
        return;
    }
    if node.tag == "#text" {
        return;
    }
    if matches!(node.style.display, webcore::types::Display::None) {
        return;
    }
    let has_children = node
        .children
        .iter()
        .any(|c| c.tag != "#text" && !matches!(c.style.display, webcore::types::Display::None));
    out.push(node.node_id); // opening tag
    for child in &node.children {
        collect_dom_node_ids(child, out, depth + 1, max_depth);
    }
    if has_children {
        out.push(node.node_id); // closing tag → same node
    }
}

/// Build DOM tree HTML. Returns (html_string, selected_line_index).
fn build_dom_tree_html(root: &webcore::WebCore, selected_nid: u32) -> (String, Option<usize>) {
    let mut html = String::new();
    let mut line_count = 0usize;
    let mut selected_line: Option<usize> = None;

    fn walk(
        node: &webcore::WebCore,
        html: &mut String,
        depth: usize,
        selected_nid: u32,
        line: &mut usize,
        sel_line: &mut Option<usize>,
    ) {
        if node.tag == "#text" {
            return;
        }
        if matches!(node.style.display, webcore::types::Display::None) {
            return;
        }
        if depth > 20 {
            return;
        }

        let indent = depth * 14;
        let is_selected = selected_nid != 0 && node.node_id == selected_nid;
        if is_selected {
            *sel_line = Some(*line);
        }

        let bg = if is_selected {
            "background:#264f78;"
        } else {
            ""
        };
        let id = node
            .attributes
            .get("id")
            .map(|v| {
                format!(
                    " <span style='color:#d7ba7d'>id=\"{}\"</span>",
                    escape_html(v)
                )
            })
            .unwrap_or_default();
        let cls = node
            .attributes
            .get("class")
            .map(|v| {
                format!(
                    " <span style='color:#9cdcfe'>class=\"{}\"</span>",
                    escape_html(&v.split_whitespace().take(4).collect::<Vec<_>>().join(" "))
                )
            })
            .unwrap_or_default();

        let has_children = node
            .children
            .iter()
            .any(|c| c.tag != "#text" && !matches!(c.style.display, webcore::types::Display::None));
        let arrow = if has_children { "▼ " } else { "  " };

        html.push_str(&format!(
            "<div style='padding:1px 2px 1px {}px;white-space:nowrap;overflow:hidden;line-height:16px;\
             font:11px monospace;cursor:pointer;{bg}'>\
             <span style='color:#888'>{arrow}</span>\
             <span style='color:#569cd6'>&lt;{}</span>{id}{cls}<span style='color:#569cd6'>&gt;</span>\
             </div>\n",
            indent, escape_html(&node.tag)
        ));
        *line += 1;

        for child in &node.children {
            walk(child, html, depth + 1, selected_nid, line, sel_line);
        }

        // Closing tag for elements with children
        if has_children {
            html.push_str(&format!(
                "<div style='padding:1px 2px 1px {}px;line-height:16px;font:11px monospace;color:#569cd6'>\
                 &lt;/{}&gt;</div>\n",
                indent, escape_html(&node.tag)
            ));
            *line += 1;
        }
    }

    walk(
        root,
        &mut html,
        0,
        selected_nid,
        &mut line_count,
        &mut selected_line,
    );

    (html, selected_line)
}

fn build_inspect_panel_html(
    node: &webcore::WebCore,
    active_tab: u8,
    doc_root: Option<&webcore::WebCore>,
) -> String {
    let s = &node.style;
    let id = node
        .attributes
        .get("id")
        .map(|v| format!("#{v}"))
        .unwrap_or_default();
    let cls = node
        .attributes
        .get("class")
        .map(|v| format!(".{}", v.split_whitespace().collect::<Vec<_>>().join(".")))
        .unwrap_or_default();

    let tab_style = |idx: u8| -> &'static str {
        if idx == active_tab {
            "color:#fff;border-bottom:2px solid #4fc3f7;padding:6px 12px;font-weight:600"
        } else {
            "color:#888;border-bottom:2px solid transparent;padding:6px 12px;cursor:pointer"
        }
    };

    let mut html = format!(
        r#"<html><head><style>
        body {{ background: #1e1e1e; color: #d4d4d4; font: 11px -apple-system, sans-serif; padding: 0; margin: 0; }}
        .panel {{ padding: 6px 10px; }}
        .elem-bar {{ background: #2d2d30; padding: 6px 10px; border-bottom: 1px solid #3e3e42;
                     font: 12px monospace; white-space: nowrap; overflow: hidden; }}
        .tab-bar {{ display: flex; background: #252526; border-bottom: 1px solid #3e3e42; font-size: 11px; }}
        .tag {{ color: #569cd6; }}
        .id {{ color: #d7ba7d; }}
        .cls {{ color: #9cdcfe; }}
        .attr-name {{ color: #9cdcfe; }}
        .attr-val {{ color: #ce9178; }}
        h3 {{ color: #ccc; font-size: 11px; font-weight: 600; text-transform: uppercase; letter-spacing: 0.5px;
             margin: 10px 0 6px 0; padding: 4px 0; border-bottom: 1px solid #3e3e42; }}
        .prop {{ color: #9cdcfe; }}
        .val {{ color: #ce9178; }}
        .computed-row {{ display: flex; padding: 1px 0; }}
        .computed-row .prop {{ min-width: 130px; }}
        .sel {{ color: #dcdcaa; font-weight: 600; }}
        .rule-src {{ color: #858585; font-size: 10px; }}
        .box-model {{ text-align: center; margin: 6px 0; font-size: 10px; }}
        .bm-margin {{ background: rgba(246,178,107,0.15); border: 1px dashed rgba(246,178,107,0.4); padding: 4px; position: relative; }}
        .bm-margin::before {{ content: 'margin'; position: absolute; top: 1px; left: 3px; color: #f6b26b; font-size: 9px; }}
        .bm-padding {{ background: rgba(147,196,125,0.15); border: 1px dashed rgba(147,196,125,0.4); padding: 4px; position: relative; }}
        .bm-padding::before {{ content: 'padding'; position: absolute; top: 1px; left: 3px; color: #93c47d; font-size: 9px; }}
        .bm-content {{ background: rgba(109,158,235,0.2); padding: 6px 4px; color: #6d9eeb; font-weight: 600; }}
        .bm-row {{ display: flex; align-items: center; }}
        .bm-side {{ flex: 0 0 30px; text-align: center; color: #aaa; }}
        .bm-center {{ flex: 1; text-align: center; }}
        .bm-top, .bm-bot {{ text-align: center; color: #aaa; padding: 2px 0; }}
        .rule-block {{ margin-bottom: 6px; padding: 4px 6px; background: #252526; border-radius: 3px; }}
        .rule-block .decl {{ padding-left: 12px; }}
        .rule-block .overridden {{ text-decoration: line-through; color: #666; }}
        .dom-tree {{ font: 11px monospace; padding: 4px 0; }}
        .dom-node {{ padding: 1px 0 1px 12px; white-space: nowrap; overflow: hidden; }}
    </style></head><body>"#
    );

    // ── Element breadcrumb bar ──
    html.push_str("<div class='elem-bar'>");
    html.push_str(&format!(
        "<span class='tag'>&lt;{}</span><span class='id'>{}</span><span class='cls'>{}</span>",
        escape_html(&node.tag),
        escape_html(&id),
        escape_html(&cls)
    ));
    for (k, v) in &node.attributes {
        if k == "id" || k == "class" || k == "style" {
            continue;
        }
        let short_v: String = v.chars().take(30).collect();
        html.push_str(&format!(
            " <span class='attr-name'>{}</span>=<span class='attr-val'>\"{}\"</span>",
            escape_html(k),
            escape_html(&short_v)
        ));
    }
    html.push_str("<span class='tag'>&gt;</span></div>");

    // ── Tab bar ──
    html.push_str(&format!(
        "<div class='tab-bar'>\
         <div style='{}'>Styles</div>\
         <div style='{}'>Computed</div>\
         <div style='{}'>Box Model</div>\
         <div style='{}'>DOM</div>\
         <div style='{}'>Layout</div>\
         <div style='{}'>Attrs</div>\
         </div>",
        tab_style(0),
        tab_style(1),
        tab_style(2),
        tab_style(3),
        tab_style(4),
        tab_style(5)
    ));

    html.push_str("<div class='panel'>");

    match active_tab {
        0 => {
            // ── Styles tab: matched CSS rules ──
            if !node.matched_rules.is_empty() {
                let mut seen_props: std::collections::HashSet<String> =
                    std::collections::HashSet::new();
                let mut rules_rev: Vec<_> = node.matched_rules.iter().collect();
                rules_rev.reverse();
                for rule in &rules_rev {
                    html.push_str("<div class='rule-block'>");
                    let src_label = if rule.source == "ua" {
                        " (user agent)"
                    } else {
                        ""
                    };
                    html.push_str(&format!(
                        "<div><span class='sel'>{}</span> <span class='rule-src'>sp:{}{}</span></div>",
                        escape_html(&rule.selector), rule.specificity, src_label
                    ));
                    for (prop, val) in &rule.declarations {
                        if prop.starts_with("--") {
                            continue;
                        }
                        let overridden = seen_props.contains(prop);
                        let cls = if overridden {
                            "decl overridden"
                        } else {
                            "decl"
                        };
                        let swatch = color_swatch(val);
                        html.push_str(&format!(
                            "<div class='{cls}'><span class='prop'>{prop}</span>: {swatch}<span class='val'>{}</span>;</div>",
                            escape_html(val)
                        ));
                    }
                    html.push_str("</div>");
                    for (prop, _) in &rule.declarations {
                        if !prop.starts_with("--") {
                            seen_props.insert(prop.clone());
                        }
                    }
                }
            } else {
                html.push_str("<div style='color:#666;padding:10px'>No matched rules (enable inspect before cascade)</div>");
            }
        }
        1 => {
            // ── Computed tab ──
            let bg = s.background_color;
            let bg_str = if bg.a > 0 {
                format!("#{:02x}{:02x}{:02x}", bg.r, bg.g, bg.b)
            } else {
                "transparent".into()
            };
            let props: Vec<(&str, String)> = vec![
                ("display", format!("{:?}", s.display)),
                ("position", format!("{:?}", s.position)),
                ("float", format!("{:?}", s.float)),
                ("box-sizing", format!("{:?}", s.box_sizing)),
                ("width", format!("{:?}", s.width)),
                ("height", format!("{:?}", s.height)),
                ("min-width", format!("{:?}", s.min_width)),
                ("max-width", format!("{:?}", s.max_width)),
                (
                    "overflow",
                    format!("{:?} / {:?}", s.overflow_x, s.overflow_y),
                ),
                ("flex-direction", format!("{:?}", s.flex_direction)),
                ("flex-wrap", format!("{:?}", s.flex_wrap)),
                (
                    "flex",
                    format!("{} {} {:?}", s.flex_grow, s.flex_shrink, s.flex_basis),
                ),
                ("align-items", format!("{:?}", s.align_items)),
                ("align-self", format!("{:?}", s.align_self)),
                ("justify-content", format!("{:?}", s.justify_content)),
                ("vertical-align", format!("{:?}", s.vertical_align)),
                ("text-align", format!("{:?}", s.text_align)),
                ("font-size", format!("{:.1}px", s.font_size_px(16.0, 16.0))),
                ("line-height", format!("{:?}", s.line_height)),
                (
                    "color",
                    format!("#{:02x}{:02x}{:02x}", s.color.r, s.color.g, s.color.b),
                ),
                ("background", bg_str),
                ("z-index", format!("{}", s.z_index)),
            ];
            for (name, val) in &props {
                html.push_str(&format!(
                    "<div class='computed-row'><span class='prop'>{name}</span><span class='val'>{val}</span></div>"
                ));
            }
            // Children summary
            let elem_children: Vec<_> = node
                .children
                .iter()
                .filter(|c| {
                    c.tag != "#text" && !matches!(c.style.display, webcore::types::Display::None)
                })
                .collect();
            if !elem_children.is_empty() {
                html.push_str("<h3>Children</h3>");
                html.push_str("<div class='dom-tree'>");
                for child in &elem_children {
                    let cid = child
                        .attributes
                        .get("id")
                        .map(|v| format!("#{v}"))
                        .unwrap_or_default();
                    let ccls = child
                        .attributes
                        .get("class")
                        .map(|v| {
                            format!(
                                ".{}",
                                v.split_whitespace().take(3).collect::<Vec<_>>().join(".")
                            )
                        })
                        .unwrap_or_default();
                    html.push_str(&format!(
                        "<div class='dom-node'><span class='tag'>{}</span><span class='id'>{}</span><span class='cls'>{}</span> \
                         <span style='color:#666'>{:?} {:.0}x{:.0}</span></div>",
                        escape_html(&child.tag), escape_html(&cid), escape_html(&ccls),
                        child.style.display, child.layout.content_rect.w, child.layout.content_rect.h
                    ));
                }
                html.push_str("</div>");
            }
        }
        2 => {
            // ── Box Model tab ──
            let c = &node.layout.content_rect;
            let mt = node.layout.resolved_margin_top;
            let mr = node.layout.resolved_margin_right;
            let mb = node.layout.resolved_margin_bottom;
            let ml = node.layout.resolved_margin_left;
            let bt = node.layout.resolved_border_top;
            let bbr = node.layout.resolved_border_right;
            let bb = node.layout.resolved_border_bottom;
            let bbl = node.layout.resolved_border_left;
            let pt = node.layout.resolved_pad_top;
            let pr = node.layout.resolved_pad_right;
            let pb = node.layout.resolved_pad_bottom;
            let pll = node.layout.resolved_pad_left;
            html.push_str(&format!(
                "<div class='box-model'>\
                 <div class='bm-margin'>\
                   <div class='bm-top'>{mt:.0}</div>\
                   <div class='bm-row'><div class='bm-side'>{ml:.0}</div><div class='bm-center'>\
                     <div class='bm-padding'>\
                       <div class='bm-top'>{pt:.0}</div>\
                       <div class='bm-row'><div class='bm-side'>{pll:.0}</div>\
                         <div class='bm-content'>{:.0} x {:.0}</div>\
                       <div class='bm-side'>{pr:.0}</div></div>\
                       <div class='bm-bot'>{pb:.0}</div>\
                     </div>\
                   </div><div class='bm-side'>{mr:.0}</div></div>\
                   <div class='bm-bot'>{mb:.0}</div>\
                 </div></div>",
                c.w, c.h
            ));
            if bt > 0.0 || bbr > 0.0 || bb > 0.0 || bbl > 0.0 {
                html.push_str(&format!(
                    "<div style='color:#888;font-size:10px;text-align:center'>border: {bt:.0} {bbr:.0} {bb:.0} {bbl:.0}</div>"
                ));
            }
            html.push_str(&format!(
                "<div style='color:#666;font-size:10px;text-align:center'>position: ({:.0}, {:.0}) size: {:.0} x {:.0}</div>",
                c.x, c.y, node.layout.margin_rect.w, node.layout.margin_rect.h
            ));
        }
        3 => {
            // ── DOM tab: ancestor chain + children tree ──
            // Ancestor chain (class chain)
            html.push_str("<h3>Ancestor Chain</h3>");
            if let Some(root) = doc_root {
                let mut chain: Vec<String> = Vec::new();
                fn find_chain(
                    cur: &webcore::WebCore,
                    target_id: u32,
                    chain: &mut Vec<String>,
                ) -> bool {
                    let id_str = cur
                        .attributes
                        .get("id")
                        .map(|v| format!("#{v}"))
                        .unwrap_or_default();
                    let cls_str = cur
                        .attributes
                        .get("class")
                        .map(|v| {
                            format!(
                                ".{}",
                                v.split_whitespace().take(3).collect::<Vec<_>>().join(".")
                            )
                        })
                        .unwrap_or_default();
                    let label = format!("{}{}{}", cur.tag, id_str, cls_str);
                    chain.push(label);
                    if cur.node_id == target_id {
                        return true;
                    }
                    for child in &cur.children {
                        if find_chain(child, target_id, chain) {
                            return true;
                        }
                    }
                    chain.pop();
                    false
                }
                find_chain(root, node.node_id, &mut chain);
                for (i, item) in chain.iter().enumerate() {
                    let indent = i * 12;
                    let is_last = i == chain.len() - 1;
                    let weight = if is_last {
                        "font-weight:600;color:#4fc3f7"
                    } else {
                        "color:#999"
                    };
                    html.push_str(&format!(
                        "<div style='padding-left:{}px;{}'>{}{}</div>",
                        indent,
                        weight,
                        if i > 0 { "└ " } else { "" },
                        escape_html(item)
                    ));
                }
            }

            // Children tree
            html.push_str("<h3>Children</h3>");
            if node.children.is_empty() {
                html.push_str("<div style='color:#666'>(no children)</div>");
            } else {
                html.push_str("<div class='dom-tree'>");
                for child in &node.children {
                    if child.tag == "#text" && child.text.trim().is_empty() {
                        continue;
                    }
                    let cid = child
                        .attributes
                        .get("id")
                        .map(|v| format!("#{v}"))
                        .unwrap_or_default();
                    let ccls = child
                        .attributes
                        .get("class")
                        .map(|v| {
                            format!(
                                ".{}",
                                v.split_whitespace().take(3).collect::<Vec<_>>().join(".")
                            )
                        })
                        .unwrap_or_default();
                    if child.tag == "#text" {
                        let preview: String = child.text.trim().chars().take(40).collect();
                        html.push_str(&format!(
                            "<div class='dom-node' style='color:#6a9955;font-style:italic'>\"{}\"</div>",
                            escape_html(&preview)
                        ));
                    } else {
                        let n_kids = child
                            .children
                            .iter()
                            .filter(|c| !(c.tag == "#text" && c.text.trim().is_empty()))
                            .count();
                        html.push_str(&format!(
                            "<div class='dom-node'><span class='tag'>{}</span><span class='id'>{}</span><span class='cls'>{}</span>\
                             <span style='color:#555'> {:?} {:.0}x{:.0} ({} children)</span></div>",
                            escape_html(&child.tag), escape_html(&cid), escape_html(&ccls),
                            child.style.display, child.layout.content_rect.w, child.layout.content_rect.h,
                            n_kids
                        ));
                    }
                }
                html.push_str("</div>");
            }
        }
        4 => {
            // ── Layout tab: detailed geometry + layout flags ──
            let l = &node.layout;
            html.push_str("<h3>Geometry</h3>");
            let rects: Vec<(&str, &webcore::Rect)> = vec![
                ("content", &l.content_rect),
                ("padding", &l.padding_rect),
                ("border", &l.border_rect),
                ("margin", &l.margin_rect),
            ];
            for (name, r) in &rects {
                html.push_str(&format!(
                    "<div class='computed-row'><span class='prop'>{}</span><span class='val'>({:.1}, {:.1}) {:.1} × {:.1}</span></div>",
                    name, r.x, r.y, r.w, r.h
                ));
            }

            html.push_str("<h3>Resolved Box</h3>");
            let box_props: Vec<(&str, f32)> = vec![
                ("margin-top", l.resolved_margin_top),
                ("margin-right", l.resolved_margin_right),
                ("margin-bottom", l.resolved_margin_bottom),
                ("margin-left", l.resolved_margin_left),
                ("border-top", l.resolved_border_top),
                ("border-right", l.resolved_border_right),
                ("border-bottom", l.resolved_border_bottom),
                ("border-left", l.resolved_border_left),
                ("padding-top", l.resolved_pad_top),
                ("padding-right", l.resolved_pad_right),
                ("padding-bottom", l.resolved_pad_bottom),
                ("padding-left", l.resolved_pad_left),
                ("content-width", l.resolved_content_width),
                ("baseline", l.baseline),
            ];
            for (name, val) in &box_props {
                if *val != 0.0 {
                    html.push_str(&format!(
                        "<div class='computed-row'><span class='prop'>{}</span><span class='val'>{:.1}px</span></div>",
                        name, val
                    ));
                }
            }

            html.push_str("<h3>Scroll</h3>");
            html.push_str(&format!(
                "<div class='computed-row'><span class='prop'>scroll</span><span class='val'>{:.0} x {:.0} (top: {:.0}, left: {:.0})</span></div>",
                l.scroll_width, l.scroll_height, l.scroll_top, l.scroll_left
            ));

            html.push_str("<h3>Inline</h3>");
            html.push_str(&format!(
                "<div class='computed-row'><span class='prop'>line-cache</span><span class='val'>{} lines</span></div>",
                l.line_cache.len()
            ));
            html.push_str(&format!(
                "<div class='computed-row'><span class='prop'>inline-runs</span><span class='val'>{} runs</span></div>",
                l.inline_runs.len()
            ));
            for (i, line) in l.line_cache.iter().enumerate().take(10) {
                html.push_str(&format!(
                    "<div class='computed-row'><span class='prop'>line {}</span><span class='val'>y={:.0} h={:.0} w={:.0} chars={}</span></div>",
                    i, line.y, line.height, line.width, line.char_x.len()
                ));
            }

            html.push_str("<h3>Flags</h3>");
            html.push_str(&format!(
                "<div class='computed-row'><span class='prop'>layout_dirty</span><span class='val'>{}</span></div>",
                l.layout_dirty
            ));
            html.push_str(&format!(
                "<div class='computed-row'><span class='prop'>node_id</span><span class='val'>{}</span></div>",
                node.node_id
            ));
            if node.image_width > 0 {
                html.push_str(&format!(
                    "<div class='computed-row'><span class='prop'>image</span><span class='val'>{}x{} ({} bytes)</span></div>",
                    node.image_width, node.image_height, node.image_data.as_ref().map(|d| d.len()).unwrap_or(0)
                ));
            }
        }
        5 => {
            // ── Attributes tab ──
            html.push_str("<h3>Attributes</h3>");
            if node.attributes.is_empty() {
                html.push_str("<div style='color:#666'>(none)</div>");
            } else {
                let mut attrs: Vec<_> = node.attributes.iter().collect();
                attrs.sort_by_key(|(k, _)| k.as_str());
                for (k, v) in &attrs {
                    let swatch = if k.as_str() == "style" || k.contains("color") {
                        color_swatch(v)
                    } else {
                        String::new()
                    };
                    html.push_str(&format!(
                        "<div class='computed-row'><span class='prop'>{}</span>{}<span class='val' style='word-break:break-all'>{}</span></div>",
                        escape_html(k), swatch, escape_html(v)
                    ));
                }
            }

            // Data attributes
            if !node.data.is_empty() {
                html.push_str("<h3>Custom Data</h3>");
                let mut data: Vec<_> = node.data.iter().collect();
                data.sort_by_key(|(k, _)| k.as_str());
                for (k, v) in &data {
                    html.push_str(&format!(
                        "<div class='computed-row'><span class='prop'>{}</span><span class='val'>{}</span></div>",
                        escape_html(k), escape_html(v)
                    ));
                }
            }

            // Inline style
            if let Some(style_attr) = node.attributes.get("style") {
                html.push_str("<h3>Inline Style</h3>");
                for decl in style_attr.split(';') {
                    let decl = decl.trim();
                    if decl.is_empty() {
                        continue;
                    }
                    if let Some(colon) = decl.find(':') {
                        let prop = decl[..colon].trim();
                        let val = decl[colon + 1..].trim();
                        let swatch = color_swatch(val);
                        html.push_str(&format!(
                            "<div class='computed-row'><span class='prop'>{}</span>{}<span class='val'>{}</span></div>",
                            escape_html(prop), swatch, escape_html(val)
                        ));
                    }
                }
            }
        }
        _ => {}
    }

    html.push_str("</div></body></html>");
    html
}

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
    if s.is_empty() || s == NEW_TAB_URL || s.starts_with("about:") {
        return NEW_TAB_URL.to_string();
    }
    if s.starts_with("http://") || s.starts_with("https://") || s.starts_with("file://") {
        return s;
    }
    // Looks like a hostname?
    if !s.contains(' ') && (s.contains('.') || s.starts_with("localhost")) {
        return format!("https://{s}");
    }
    // Search query
    let query: String = s
        .chars()
        .map(|c| match c {
            ' ' => '+',
            c if c.is_alphanumeric() || "-_.~".contains(c) => c,
            _ => c,
        })
        .collect();
    format!("https://duckduckgo.com/?q={query}")
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

// ─── Remote debug server ──────────────────────────────────────────────────────
// All commands mirror debugserver.rs but operate on the active tab's document.

/// Escape a string for JSON output.
/// Select elements, allowing a trailing `::before` / `::after` to address the
/// synthetic pseudo-element child instead of the element itself.
fn dbg_select_with_pseudo<'a>(root: &'a webcore::WebCore, sel: &str) -> Vec<&'a webcore::WebCore> {
    for tag in ["::before", "::after"] {
        if let Some(base) = sel.trim().strip_suffix(tag) {
            let base = base.trim();
            let parents = webcore::dom::query_selector_all(root, base);
            return parents
                .into_iter()
                .filter_map(|p| p.children.iter().find(|c| c.tag == tag))
                .collect();
        }
    }
    webcore::dom::query_selector_all(root, sel)
}

fn dbg_json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c < '\x20' => out.push_str(&format!("\\u{:04x}", c as u32)),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

fn dbg_json_str(json: &str, key: &str) -> Option<String> {
    let needle = format!("\"{}\"", key);
    let pos = json.find(&needle)?;
    let after = &json[pos + needle.len()..];
    let after = after.trim_start().strip_prefix(':')?;
    let after = after.trim_start();
    if after.starts_with('"') {
        let s = &after[1..];
        let mut end = 0;
        let mut escaped = false;
        for c in s.chars() {
            if escaped {
                escaped = false;
                end += c.len_utf8();
                continue;
            }
            if c == '\\' {
                escaped = true;
                end += 1;
                continue;
            }
            if c == '"' {
                break;
            }
            end += c.len_utf8();
        }
        Some(
            s[..end]
                .replace("\\\"", "\"")
                .replace("\\n", "\n")
                .replace("\\\\", "\\"),
        )
    } else {
        None
    }
}

fn dbg_json_num(json: &str, key: &str) -> Option<f32> {
    let needle = format!("\"{}\"", key);
    let pos = json.find(&needle)?;
    let after = &json[pos + needle.len()..];
    let after = after.trim_start().strip_prefix(':')?;
    let after = after.trim_start();
    let end = after
        .find(|c: char| !c.is_ascii_digit() && c != '.' && c != '-')
        .unwrap_or(after.len());
    after[..end].parse().ok()
}

fn dbg_json_bool(json: &str, key: &str) -> Option<bool> {
    let needle = format!("\"{}\"", key);
    let pos = json.find(&needle)?;
    let after = &json[pos + needle.len()..];
    let after = after.trim_start().strip_prefix(':')?;
    let after = after.trim_start();
    if after.starts_with("true") {
        Some(true)
    } else if after.starts_with("false") {
        Some(false)
    } else {
        dbg_json_num(json, key).map(|value| value != 0.0)
    }
}

/// Node ids matching `query`, resolved with the engine's own selector engine.
fn dbg_query_ids(doc: &Document, query: &str) -> std::collections::HashSet<u32> {
    doc.query_selector_all(query).into_iter().collect()
}

thread_local! {
    /// Last (selector, matching node ids) — see `dbg_matches_query`.
    static DBG_QUERY_MEMO: std::cell::RefCell<Option<(String, std::collections::HashSet<u32>)>> =
        const { std::cell::RefCell::new(None) };
}

/// Does `node` match `query`?
///
/// The fast path below only understands `tag#id.class`. Anything else — a
/// combinator, a pseudo-class, an attribute selector — is handed to the ENGINE's
/// selector matcher via `query_selector_all`, because this hand-rolled parser
/// used to answer `false` for those instead of admitting it could not tell:
/// `{"cmd":"find","selector":"table tbody"}` reported `count: 0` on a document
/// that plainly had one, which reads exactly like a missing element. A debug
/// tool that lies about the DOM is worse than no debug tool.
///
/// One-entry memo because every caller walks the whole tree with a single fixed
/// selector — without it this would be O(n²) per command.
fn dbg_matches_query(doc: &Document, node: &webcore::WebCore, query: &str) -> bool {
    if node.tag == "#text" {
        return false;
    }
    let query = query.trim();
    if query.contains([' ', '>', '+', '~', ':', '[', ',']) {
        if node.node_id == 0 {
            return false;
        }
        return DBG_QUERY_MEMO.with(|memo| {
            let mut memo = memo.borrow_mut();
            let stale = memo.as_ref().map(|(q, _)| q != query).unwrap_or(true);
            if stale {
                let ids: std::collections::HashSet<u32> =
                    doc.query_selector_all(query).into_iter().collect();
                *memo = Some((query.to_string(), ids));
            }
            memo.as_ref().unwrap().1.contains(&node.node_id)
        });
    }
    let mut tag_q = "";
    let mut id_q = "";
    let mut classes_q: Vec<&str> = Vec::new();
    let mut rest = query;
    if !rest.starts_with('#') && !rest.starts_with('.') {
        let end = rest
            .find(|c: char| c == '#' || c == '.')
            .unwrap_or(rest.len());
        tag_q = &rest[..end];
        rest = &rest[end..];
    }
    while !rest.is_empty() {
        if rest.starts_with('#') {
            rest = &rest[1..];
            let end = rest
                .find(|c: char| c == '#' || c == '.')
                .unwrap_or(rest.len());
            id_q = &rest[..end];
            rest = &rest[end..];
        } else if rest.starts_with('.') {
            rest = &rest[1..];
            let end = rest
                .find(|c: char| c == '#' || c == '.')
                .unwrap_or(rest.len());
            classes_q.push(&rest[..end]);
            rest = &rest[end..];
        } else {
            break;
        }
    }
    if !tag_q.is_empty() && !node.tag.eq_ignore_ascii_case(tag_q) {
        return false;
    }
    if !id_q.is_empty() {
        if node.attributes.get("id").map(|s| s.as_str()) != Some(id_q) {
            return false;
        }
    }
    if !classes_q.is_empty() {
        let cls = node
            .attributes
            .get("class")
            .map(|s| s.as_str())
            .unwrap_or("");
        let elem_classes: Vec<&str> = cls.split_whitespace().collect();
        for c in &classes_q {
            if !elem_classes.contains(c) {
                return false;
            }
        }
    }
    true
}

fn dbg_collect_text(node: &webcore::WebCore, out: &mut String) {
    if node.tag == "#text" {
        if !out.is_empty() && !out.ends_with(' ') {
            out.push(' ');
        }
        out.push_str(node.text.trim());
    }
    for child in &node.children {
        dbg_collect_text(child, out);
    }
}

fn dbg_inspect_json(node: &webcore::WebCore) -> String {
    let s = &node.style;
    let id = node.attributes.get("id").map(|v| v.as_str()).unwrap_or("");
    let cls = node
        .attributes
        .get("class")
        .map(|v| v.as_str())
        .unwrap_or("");
    let bg = s.background_color;
    let bg_str = if bg.a > 0 {
        format!("#{:02x}{:02x}{:02x}", bg.r, bg.g, bg.b)
    } else {
        "transparent".to_string()
    };
    let color_str = format!("#{:02x}{:02x}{:02x}", s.color.r, s.color.g, s.color.b);
    let mask_image = s.rare().mask_image_url.as_str();
    let svg_child_count = node
        .svg_document
        .as_ref()
        .map(|doc| doc.root.children.len())
        .unwrap_or(0);
    format!(
        concat!(
            r#"{{"tag":{0},"id":{1},"class":{2},"#,
            r#""content":{{"x":{3:.1},"y":{4:.1},"w":{5:.1},"h":{6:.1}}},"#,
            r#""padding":{{"x":{7:.1},"y":{8:.1},"w":{9:.1},"h":{10:.1}}},"#,
            r#""margin":{{"x":{11:.1},"y":{12:.1},"w":{13:.1},"h":{14:.1}}},"#,
            r#""display":{15},"position":{16},"#,
            r#""font_size":{17:.1},"text_align":{18},"color":{19},"background":{20},"#,
            r#""margin_trbl":[{21:.1},{22:.1},{23:.1},{24:.1}],"#,
            r#""padding_trbl":[{25:.1},{26:.1},{27:.1},{28:.1}],"#,
            r#""border_trbl":[{29:.1},{30:.1},{31:.1},{32:.1}],"#,
            r#""children":{33},"svg_parsed":{34},"svg_children":{35},"#,
            r#""mask_image":{36},"mask_loaded":{37},"mask_size":[{38},{39}]}}"#
        ),
        dbg_json_escape(&node.tag),
        dbg_json_escape(id),
        dbg_json_escape(cls),
        node.layout.content_rect.x,
        node.layout.content_rect.y,
        node.layout.content_rect.w,
        node.layout.content_rect.h,
        node.layout.padding_rect.x,
        node.layout.padding_rect.y,
        node.layout.padding_rect.w,
        node.layout.padding_rect.h,
        node.layout.margin_rect.x,
        node.layout.margin_rect.y,
        node.layout.margin_rect.w,
        node.layout.margin_rect.h,
        dbg_json_escape(&format!("{:?}", s.display)),
        dbg_json_escape(&format!("{:?}", s.position)),
        s.font_size_px(16.0, 16.0),
        dbg_json_escape(&format!("{:?}", s.text_align)),
        dbg_json_escape(&color_str),
        dbg_json_escape(&bg_str),
        node.layout.resolved_margin_top,
        node.layout.resolved_margin_right,
        node.layout.resolved_margin_bottom,
        node.layout.resolved_margin_left,
        node.layout.resolved_pad_top,
        node.layout.resolved_pad_right,
        node.layout.resolved_pad_bottom,
        node.layout.resolved_pad_left,
        node.layout.resolved_border_top,
        node.layout.resolved_border_right,
        node.layout.resolved_border_bottom,
        node.layout.resolved_border_left,
        node.children.iter().filter(|c| c.tag != "#text").count(),
        node.svg_document.is_some(),
        svg_child_count,
        dbg_json_escape(mask_image),
        node.mask_image_data.is_some(),
        node.mask_image_width,
        node.mask_image_height,
    )
}

fn dbg_svg_metrics_json(root: &webcore::WebCore) -> String {
    let mut summary = webcore::svg::SvgUnsupportedSummary::default();
    Document::walk_all(root, &mut |node| {
        if let Some(doc) = node.svg_document.as_ref() {
            summary.merge(webcore::svg::unsupported_summary(doc));
        }
    });
    format!(
        r#"{{"documents":{},"unsupported_count":{},"clean":{},"unknown_elements":{},"unsupported_elements":{},"unsupported_attributes":{}}}"#,
        summary.documents,
        svg_metric_total(&summary),
        summary.is_empty(),
        dbg_count_map_json(&summary.unknown_elements),
        dbg_count_map_json(&summary.unsupported_elements),
        dbg_count_map_json(&summary.unsupported_attributes),
    )
}

fn svg_metric_total(summary: &webcore::svg::SvgUnsupportedSummary) -> usize {
    summary.unknown_elements.values().sum::<usize>()
        + summary.unsupported_elements.values().sum::<usize>()
        + summary.unsupported_attributes.values().sum::<usize>()
}

fn dbg_count_map_json(map: &std::collections::BTreeMap<String, usize>) -> String {
    let mut parts = Vec::new();
    for (key, count) in map {
        parts.push(format!("{}:{}", dbg_json_escape(key), count));
    }
    format!("{{{}}}", parts.join(","))
}

fn dbg_computed_json(node: &webcore::WebCore) -> String {
    use std::fmt::Write;
    let s = &node.style;
    let id = node.attributes.get("id").map(|v| v.as_str()).unwrap_or("");
    let cls = node
        .attributes
        .get("class")
        .map(|v| v.as_str())
        .unwrap_or("");
    let bg = s.background_color;
    let bg_str = if bg.a > 0 {
        format!("#{:02x}{:02x}{:02x}", bg.r, bg.g, bg.b)
    } else {
        "transparent".to_string()
    };
    let c = s.color;
    let color_hex = format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b);
    let oc = s.outline_color;
    let outline_color_hex = format!("#{:02x}{:02x}{:02x}{:02x}", oc.r, oc.g, oc.b, oc.a);
    let mut buf = String::with_capacity(2048);
    let _ = write!(
        buf,
        r#"{{"node_id":{},"tag":{},"id":{},"class":{}"#,
        node.node_id,
        dbg_json_escape(&node.tag),
        dbg_json_escape(id),
        dbg_json_escape(cls)
    );
    let _ = write!(
        buf,
        r#","box":{{"content":[{:.1},{:.1},{:.1},{:.1}],"padding":[{:.1},{:.1},{:.1},{:.1}],"margin":[{:.1},{:.1},{:.1},{:.1}],"border":[{:.1},{:.1},{:.1},{:.1}]}}"#,
        node.layout.content_rect.x,
        node.layout.content_rect.y,
        node.layout.content_rect.w,
        node.layout.content_rect.h,
        node.layout.padding_rect.x,
        node.layout.padding_rect.y,
        node.layout.padding_rect.w,
        node.layout.padding_rect.h,
        node.layout.margin_rect.x,
        node.layout.margin_rect.y,
        node.layout.margin_rect.w,
        node.layout.margin_rect.h,
        node.layout.border_rect.x,
        node.layout.border_rect.y,
        node.layout.border_rect.w,
        node.layout.border_rect.h
    );
    let _ = write!(
        buf,
        r#","display":{},"position":{},"float":{}"#,
        dbg_json_escape(&format!("{:?}", s.display)),
        dbg_json_escape(&format!("{:?}", s.position)),
        dbg_json_escape(&format!("{:?}", s.float))
    );
    let _ = write!(
        buf,
        r#","visibility":{},"opacity":{:.2}"#,
        dbg_json_escape(&format!("{:?}", s.visibility)),
        s.opacity
    );
    let _ = write!(
        buf,
        r#","overflow":[{},{}],"box_sizing":{}"#,
        dbg_json_escape(&format!("{:?}", s.overflow_x)),
        dbg_json_escape(&format!("{:?}", s.overflow_y)),
        dbg_json_escape(&format!("{:?}", s.box_sizing))
    );
    let _ = write!(
        buf,
        r#","width":{},"height":{}"#,
        dbg_json_escape(&format!("{:?}", s.width)),
        dbg_json_escape(&format!("{:?}", s.height))
    );
    let _ = write!(
        buf,
        r#","font_size":{:.1},"font_weight":{},"font_family":{}"#,
        s.font_size_px(16.0, 16.0),
        dbg_json_escape(&format!("{:?}", s.font_weight)),
        dbg_json_escape(&s.font_family)
    );
    let _ = write!(
        buf,
        r#","text_align":{},"vertical_align":{},"direction":{},"writing_mode":{}"#,
        dbg_json_escape(&format!("{:?}", s.text_align)),
        dbg_json_escape(&format!("{:?}", s.vertical_align)),
        dbg_json_escape(&format!("{:?}", s.direction)),
        dbg_json_escape(&format!("{:?}", s.writing_mode))
    );
    let _ = write!(
        buf,
        r#","color":{},"background":{}"#,
        dbg_json_escape(&color_hex),
        dbg_json_escape(&bg_str)
    );
    let _ = write!(
        buf,
        r#","outline":{{"width":{:.1},"style":{},"offset":{:.1},"color":{}}}"#,
        s.outline_width,
        dbg_json_escape(&format!("{:?}", s.outline_style)),
        s.outline_offset,
        dbg_json_escape(&outline_color_hex)
    );
    let _ = write!(
        buf,
        r#","flex_direction":{},"flex_wrap":{}"#,
        dbg_json_escape(&format!("{:?}", s.flex_direction)),
        dbg_json_escape(&format!("{:?}", s.flex_wrap))
    );
    let _ = write!(
        buf,
        r#","flex_grow":{},"flex_shrink":{},"align_items":{},"justify_content":{}"#,
        s.flex_grow,
        s.flex_shrink,
        dbg_json_escape(&format!("{:?}", s.align_items)),
        dbg_json_escape(&format!("{:?}", s.justify_content))
    );
    let _ = write!(
        buf,
        r#","css_padding":[{},{},{},{}]"#,
        dbg_json_escape(&format!("{:?}", s.padding_top)),
        dbg_json_escape(&format!("{:?}", s.padding_right)),
        dbg_json_escape(&format!("{:?}", s.padding_bottom)),
        dbg_json_escape(&format!("{:?}", s.padding_left))
    );
    let _ = write!(
        buf,
        r#","css_margin":[{},{},{},{}]"#,
        dbg_json_escape(&format!("{:?}", s.margin_top)),
        dbg_json_escape(&format!("{:?}", s.margin_right)),
        dbg_json_escape(&format!("{:?}", s.margin_bottom)),
        dbg_json_escape(&format!("{:?}", s.margin_left))
    );
    let _ = write!(
        buf,
        r#","resolved_padding":[{:.1},{:.1},{:.1},{:.1}]"#,
        node.layout.resolved_pad_top,
        node.layout.resolved_pad_right,
        node.layout.resolved_pad_bottom,
        node.layout.resolved_pad_left
    );
    let _ = write!(
        buf,
        r#","resolved_margin":[{:.1},{:.1},{:.1},{:.1}]"#,
        node.layout.resolved_margin_top,
        node.layout.resolved_margin_right,
        node.layout.resolved_margin_bottom,
        node.layout.resolved_margin_left
    );
    let _ = write!(
        buf,
        r#","checked":{},"selected":{},"dirty_checked":{},"dirty_selected":{}"#,
        node.checkedness, node.selectedness, node.dirty_checked, node.dirty_selectedness
    );
    let _ = write!(
        buf,
        r#","border_collapse":{},"matched_rules":{},"line_count":{}}}"#,
        s.border_collapse,
        node.matched_rules.len(),
        node.layout.line_cache.len()
    );
    buf
}

fn dbg_match_report_json(doc: &Document, node: &webcore::WebCore) -> String {
    let hover_chain = webcore::css::build_hover_chain(&doc.root, doc.hovered_box);
    let Some(report) = webcore::css::debug_match_report_for_node(
        &doc.root,
        &doc.stylesheet,
        node.node_id,
        doc.viewport_w,
        doc.viewport_h,
        doc.focused_box,
        doc.keyboard_focus,
        &hover_chain,
        0,
        &doc.base_url,
    ) else {
        return format!(
            r#"{{"node_id":{},"ok":false,"error":"node not found"}}"#,
            node.node_id
        );
    };
    let rule_names = |indices: &[usize]| -> String {
        indices
            .iter()
            .filter_map(|idx| {
                doc.stylesheet.rules.get(*idx).map(|rule| {
                    format!(
                        r#"{{"idx":{},"selector":{},"pseudo":"{:?}"}}"#,
                        idx,
                        dbg_json_escape(&rule.original_selector),
                        rule.pseudo_element
                    )
                })
            })
            .collect::<Vec<_>>()
            .join(",")
    };
    format!(
        r#"{{"ok":true,"node_id":{},"tag":{},"id":{},"class":{},"candidate_count":{},"matched_count":{},"candidates":[{}],"matched":[{}],"hover":[{}],"active":[{}],"visited":[{}],"before":[{}],"after":[{}]}}"#,
        report.node_id,
        dbg_json_escape(&report.tag),
        dbg_json_escape(&report.id),
        dbg_json_escape(&report.class_attr),
        report.candidate_rule_indices.len(),
        report.matched_rule_indices.len(),
        rule_names(&report.candidate_rule_indices),
        rule_names(&report.matched_rule_indices),
        rule_names(&report.hover_rule_indices),
        rule_names(&report.active_rule_indices),
        rule_names(&report.visited_rule_indices),
        rule_names(&report.before_rule_indices),
        rule_names(&report.after_rule_indices)
    )
}

fn dbg_height_dump_json(root: &webcore::WebCore, limit: usize) -> String {
    let mut rows: Vec<(f32, String)> = Vec::new();
    fn walk(node: &webcore::WebCore, ancestors: &mut Vec<String>, rows: &mut Vec<(f32, String)>) {
        if matches!(node.style.display, webcore::types::Display::None) {
            return;
        }
        let mr = node.layout.margin_rect;
        let cr = node.layout.content_rect;
        let bottom = mr.y + mr.h;
        if bottom <= 0.0 {
            return;
        }
        let id = node.attributes.get("id").map(|v| v.as_str()).unwrap_or("");
        let cls = node
            .attributes
            .get("class")
            .map(|v| v.as_str())
            .unwrap_or("");
        let ancestor_json = ancestors
            .iter()
            .rev()
            .take(8)
            .rev()
            .map(|tag| dbg_json_escape(tag))
            .collect::<Vec<_>>()
            .join(",");
        rows.push((
            bottom,
            format!(
                r#"{{"node_id":{},"tag":{},"id":{},"class":{},"ancestors":[{}],"display":{},"position":{},"overflow_y":{},"visibility":{},"content":[{:.1},{:.1},{:.1},{:.1}],"margin":[{:.1},{:.1},{:.1},{:.1}],"bottom":{:.1}}}"#,
                node.node_id,
                dbg_json_escape(&node.tag),
                dbg_json_escape(id),
                dbg_json_escape(cls),
                ancestor_json,
                dbg_json_escape(&format!("{:?}", node.style.display)),
                dbg_json_escape(&format!("{:?}", node.style.position)),
                dbg_json_escape(&format!("{:?}", node.style.overflow_y)),
                dbg_json_escape(&format!("{:?}", node.style.visibility)),
                cr.x,
                cr.y,
                cr.w,
                cr.h,
                mr.x,
                mr.y,
                mr.w,
                mr.h,
                bottom
            ),
        ));
        ancestors.push(node.tag.clone());
        for child in &node.children {
            walk(child, ancestors, rows);
        }
        if let Some(shadow) = node.shadow_root.as_ref() {
            ancestors.push("#shadow-root".to_string());
            for child in &shadow.children {
                walk(child, ancestors, rows);
            }
            ancestors.pop();
        }
        ancestors.pop();
    }
    walk(root, &mut Vec::new(), &mut rows);
    rows.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    rows.truncate(limit.max(1));
    let elements = rows.into_iter().map(|(_, json)| json).collect::<Vec<_>>();
    format!(
        r#"{{"ok":true,"count":{},"scroll_height":{:.1},"elements":[{}]}}"#,
        elements.len(),
        Document::scroll_height(root),
        elements.join(",")
    )
}

fn dbg_dump_box(depth: usize, node: &webcore::WebCore, buf: &mut String) {
    use std::fmt::Write;
    use webcore::types::Display;
    if matches!(node.style.display, Display::None) {
        return;
    }
    let indent = "  ".repeat(depth);
    let tag = if node.tag.is_empty() {
        "(box)"
    } else {
        &node.tag
    };
    let id = node
        .attributes
        .get("id")
        .map(|v| format!("#{v}"))
        .unwrap_or_default();
    let cls = node
        .attributes
        .get("class")
        .map(|v| {
            format!(
                ".{}",
                v.split_whitespace().take(3).collect::<Vec<_>>().join(".")
            )
        })
        .unwrap_or_default();
    let text_preview = if node.tag == "#text" && !node.text.is_empty() {
        let s: String = node.text.chars().take(40).collect();
        format!(" {:?}", s.trim())
    } else {
        String::new()
    };
    let _ = writeln!(
        buf,
        "{}{}{}{} [{:?}] c=[{:.0},{:.0} {:.0}x{:.0}] m=[{:.0},{:.0} {:.0}x{:.0}]{}",
        indent,
        tag,
        id,
        cls,
        node.style.display,
        node.layout.content_rect.x,
        node.layout.content_rect.y,
        node.layout.content_rect.w,
        node.layout.content_rect.h,
        node.layout.margin_rect.x,
        node.layout.margin_rect.y,
        node.layout.margin_rect.w,
        node.layout.margin_rect.h,
        text_preview
    );
    for child in &node.children {
        dbg_dump_box(depth + 1, child, buf);
    }
}

fn dbg_serialize_html(node: &webcore::WebCore, buf: &mut String, depth: usize) {
    use std::fmt::Write;
    if node.tag == "#text" {
        let t = node.text.trim();
        if !t.is_empty() {
            let _ = write!(buf, "{}", t);
        }
        return;
    }
    let indent = "  ".repeat(depth);
    let _ = write!(buf, "{}<{}", indent, node.tag);
    for (k, v) in &node.attributes {
        let _ = write!(buf, " {}={}", k, dbg_json_escape(v));
    }
    let _ = write!(buf, ">");
    if !node.children.is_empty() {
        let _ = writeln!(buf);
        for child in &node.children {
            dbg_serialize_html(child, buf, depth + 1);
        }
        let _ = write!(buf, "{}</{}>", indent, node.tag);
    } else {
        let _ = write!(buf, "</{}>", node.tag);
    }
    let _ = writeln!(buf);
}

fn dbg_selector_center(doc: &Document, selector: &str) -> Option<(f32, f32)> {
    let mut result = None;
    Document::walk_all(&doc.root, &mut |node| {
        if result.is_some() {
            return;
        }
        if dbg_matches_query(doc, node, selector) {
            let r = &node.layout.content_rect;
            result = Some((r.x + r.w / 2.0, r.y + r.h / 2.0));
        }
    });
    result
}

impl BrowserApp {
    /// Handle a single remote debug command line against the active tab.
    fn handle_debug_command(&mut self, line: &str) -> String {
        let line = line.trim();
        if line.is_empty() {
            return String::new();
        }
        let t0 = std::time::Instant::now();
        let cmd = dbg_json_str(line, "cmd").unwrap_or_default();
        let result = self.dispatch_debug_cmd(&cmd, line);
        if debug_cmd_requests_redraw(&cmd) && result.starts_with(r#"{"ok":true"#) {
            if let Some(w) = &self.window {
                w.request_redraw();
            }
        }
        let cmd_ms = t0.elapsed().as_micros() as f64 / 1000.0;
        if result.starts_with(r#"{"ok":true"#) && cmd != "perf" {
            let insert_pos = result.len() - 1;
            let mut r = result;
            r.insert_str(insert_pos, &format!(r#","cmd_ms":{:.2}"#, cmd_ms));
            r
        } else {
            result
        }
    }

    fn dispatch_debug_cmd(&mut self, cmd: &str, line: &str) -> String {
        match cmd {
            // ── Screenshot ───────────────────────────────────────────────────
            "screenshot" => {
                let path = dbg_json_str(line, "out").unwrap_or_else(|| "snapshot.png".to_string());
                let scale = dbg_json_num(line, "scale")
                    .map(|v| v.clamp(0.25, 4.0) as f32)
                    .unwrap_or(1.0);
                let view_w = self.width;
                let view_h = self.content_h();
                let phys_w = ((view_w as f32) * scale).ceil() as u32;
                let ch = ((view_h as f32) * scale).ceil() as u32;
                if self.tabs[self.active].view.document().is_none() {
                    return r#"{"ok":false,"error":"no document loaded"}"#.to_string();
                }
                let Some(mut pm) = tiny_skia::Pixmap::new(phys_w.max(1), ch.max(1)) else {
                    return r#"{"ok":false,"error":"pixmap alloc failed"}"#.to_string();
                };
                pm.fill(tiny_skia::Color::WHITE);
                self.tabs[self.active].view.resize(view_w, view_h);
                self.tabs[self.active].view.paint_into(&mut pm, 0, 0, scale);
                match pm.save_png(&path) {
                    Ok(_) => format!(
                        r#"{{"ok":true,"path":{},"width":{},"height":{},"scale":{}}}"#,
                        dbg_json_escape(&path),
                        phys_w,
                        ch,
                        scale
                    ),
                    Err(e) => format!(
                        r#"{{"ok":false,"error":{}}}"#,
                        dbg_json_escape(&e.to_string())
                    ),
                }
            }
            // ── Navigation ───────────────────────────────────────────────────
            "navigate" => match dbg_json_str(line, "url") {
                Some(u) => {
                    self.navigate(normalize_url(u));
                    format!(r#"{{"ok":true}}"#)
                }
                None => r#"{"ok":false,"error":"navigate needs url"}"#.to_string(),
            },
            // ── Browse tab list / switch ──────────────────────────────────────
            "tabs" => {
                let list: Vec<String> = self
                    .tabs
                    .iter()
                    .enumerate()
                    .map(|(i, t)| {
                        format!(
                            r#"{{"index":{},"active":{},"url":{},"title":{}}}"#,
                            i,
                            i == self.active,
                            dbg_json_escape(&t.url),
                            dbg_json_escape(&t.title)
                        )
                    })
                    .collect();
                format!(
                    r#"{{"ok":true,"count":{},"tabs":[{}]}}"#,
                    list.len(),
                    list.join(",")
                )
            }
            "switch-tab" => {
                if let Some(idx) = dbg_json_num(line, "index") {
                    self.switch_tab(idx as usize);
                    format!(r#"{{"ok":true,"index":{}}}"#, idx as usize)
                } else {
                    r#"{"ok":false,"error":"switch-tab needs index"}"#.to_string()
                }
            }
            // ── Resize ───────────────────────────────────────────────────────
            "resize" => {
                if let Some(w) = dbg_json_num(line, "width") {
                    self.width = w;
                    self.relayout_active();
                }
                format!(
                    r#"{{"ok":true,"width":{:.0},"height":{:.0}}}"#,
                    self.width,
                    self.content_h()
                )
            }
            // ── Scroll ───────────────────────────────────────────────────────
            "scroll" => {
                if self.tabs[self.active].view.document().is_none() {
                    r#"{"ok":false,"error":"no document"}"#.to_string()
                } else {
                    if let Some(dy) = dbg_json_num(line, "dy") {
                        self.tabs[self.active].view.scroll_by(0.0, dy);
                    } else if let Some(y) = dbg_json_num(line, "y") {
                        self.tabs[self.active].view.scroll_to(0.0, y);
                    }
                    format!(
                        r#"{{"ok":true,"scroll_y":{:.0}}}"#,
                        self.tabs[self.active].view.scroll_y()
                    )
                }
            }
            // ── Click ────────────────────────────────────────────────────────
            "click" => {
                let coords = if let Some(sel) = dbg_json_str(line, "selector") {
                    let doc = self.tabs[self.active].view.document();
                    doc.and_then(|d| dbg_selector_center(d, &sel))
                        .ok_or_else(|| {
                            format!(
                                r#"{{"ok":false,"error":"no element matches {}"}}"#,
                                dbg_json_escape(&sel)
                            )
                        })
                } else if let (Some(x), Some(y)) =
                    (dbg_json_num(line, "x"), dbg_json_num(line, "y"))
                {
                    Ok((x, y))
                } else {
                    return r#"{"ok":false,"error":"click needs x,y or selector"}"#.to_string();
                };
                let (x, y) = match coords {
                    Ok(c) => c,
                    Err(e) => return e,
                };
                self.tabs[self.active].view.handle_mouse_button(
                    webcore::dom::HtmlEventType::MouseDown,
                    x,
                    y,
                    0,
                );
                self.tabs[self.active].view.handle_mouse_button(
                    webcore::dom::HtmlEventType::MouseUp,
                    x,
                    y,
                    0,
                );
                format!(r#"{{"ok":true,"x":{:.0},"y":{:.0}}}"#, x, y)
            }
            // ── Hover ────────────────────────────────────────────────────────
            "hover" => {
                let coords = if let Some(sel) = dbg_json_str(line, "selector") {
                    let doc = self.tabs[self.active].view.document();
                    doc.and_then(|d| dbg_selector_center(d, &sel))
                        .ok_or_else(|| {
                            format!(
                                r#"{{"ok":false,"error":"no element matches {}"}}"#,
                                dbg_json_escape(&sel)
                            )
                        })
                } else if let (Some(x), Some(y)) =
                    (dbg_json_num(line, "x"), dbg_json_num(line, "y"))
                {
                    Ok((x, y))
                } else {
                    return r#"{"ok":false,"error":"hover needs x,y or selector"}"#.to_string();
                };
                let (x, y) = match coords {
                    Ok(c) => c,
                    Err(e) => return e,
                };
                let changed = self.tabs[self.active].view.handle_mouse_move(x, y);
                format!(r#"{{"ok":true,"changed":{}}}"#, changed)
            }
            // ── Type / Key ───────────────────────────────────────────────────
            "type" => match dbg_json_str(line, "text") {
                Some(text) => {
                    let mut any = false;
                    for ch in text.chars() {
                        if self.tabs[self.active].view.handle_key(
                            webcore::dom::HtmlEventType::KeyDown,
                            ch as u32,
                            Some(ch),
                            false,
                            false,
                            false,
                            false,
                        ) {
                            any = true;
                        }
                    }
                    if any {}
                    format!(r#"{{"ok":true,"typed":{}}}"#, any)
                }
                None => r#"{"ok":false,"error":"type needs text"}"#.to_string(),
            },
            "key" => match dbg_json_str(line, "key") {
                Some(k) => {
                    let (code, ch) = match k.as_str() {
                        "Enter" => (13, Some('\r')),
                        "Tab" => (9, Some('\t')),
                        "Backspace" => (8, None),
                        "Delete" => (46, None),
                        "Escape" => (27, None),
                        "ArrowLeft" => (37, None),
                        "ArrowRight" => (39, None),
                        "ArrowUp" => (38, None),
                        "ArrowDown" => (40, None),
                        "Home" => (36, None),
                        "End" => (35, None),
                        "Space" => (32, Some(' ')),
                        s if s.len() == 1 => (s.chars().next().unwrap() as u32, s.chars().next()),
                        _ => {
                            return format!(
                                r#"{{"ok":false,"error":"unknown key: {}"}}"#,
                                dbg_json_escape(&k)
                            );
                        }
                    };
                    let changed = self.tabs[self.active].view.handle_key(
                        webcore::dom::HtmlEventType::KeyDown,
                        code,
                        ch,
                        false,
                        false,
                        false,
                        false,
                    );
                    if changed {}
                    format!(r#"{{"ok":true,"changed":{}}}"#, changed)
                }
                None => r#"{"ok":false,"error":"key needs key name"}"#.to_string(),
            },
            // ── Find / Text / Attr / HTML ────────────────────────────────────
            "find" => match dbg_json_str(line, "selector") {
                Some(sel) => {
                    let mut results = Vec::new();
                    if let Some(doc) = self.tabs[self.active].view.document() {
                        Document::walk_all(&doc.root, &mut |node| {
                            if dbg_matches_query(doc, node, &sel) {
                                let id =
                                    node.attributes.get("id").map(|v| v.as_str()).unwrap_or("");
                                let cls = node
                                    .attributes
                                    .get("class")
                                    .map(|v| v.as_str())
                                    .unwrap_or("");
                                let r = &node.layout.content_rect;
                                results.push(format!(r#"{{"node_id":{},"tag":{},"id":{},"class":{},"x":{:.0},"y":{:.0},"w":{:.0},"h":{:.0}}}"#,
                                        node.node_id, dbg_json_escape(&node.tag), dbg_json_escape(id), dbg_json_escape(cls),
                                        r.x, r.y, r.w, r.h));
                            }
                        });
                    }
                    format!(
                        r#"{{"ok":true,"count":{},"elements":[{}]}}"#,
                        results.len(),
                        results.join(",")
                    )
                }
                None => r#"{"ok":false,"error":"find needs selector"}"#.to_string(),
            },
            "text" => match dbg_json_str(line, "selector") {
                Some(sel) => {
                    let mut texts = Vec::new();
                    if let Some(doc) = self.tabs[self.active].view.document() {
                        Document::walk_all(&doc.root, &mut |node| {
                            if dbg_matches_query(doc, node, &sel) {
                                let mut t = String::new();
                                dbg_collect_text(node, &mut t);
                                texts.push(dbg_json_escape(&t));
                            }
                        });
                    }
                    format!(
                        r#"{{"ok":true,"count":{},"texts":[{}]}}"#,
                        texts.len(),
                        texts.join(",")
                    )
                }
                None => r#"{"ok":false,"error":"text needs selector"}"#.to_string(),
            },
            "attr" => match (dbg_json_str(line, "selector"), dbg_json_str(line, "name")) {
                (Some(sel), Some(name)) => {
                    let mut values = Vec::new();
                    if let Some(doc) = self.tabs[self.active].view.document() {
                        Document::walk_all(&doc.root, &mut |node| {
                            if dbg_matches_query(doc, node, &sel) {
                                if let Some(v) = node.attributes.get(&name) {
                                    values.push(dbg_json_escape(v));
                                }
                            }
                        });
                    }
                    format!(
                        r#"{{"ok":true,"count":{},"values":[{}]}}"#,
                        values.len(),
                        values.join(",")
                    )
                }
                _ => r#"{"ok":false,"error":"attr needs selector and name"}"#.to_string(),
            },
            "html" => match dbg_json_str(line, "selector") {
                Some(sel) => {
                    let mut results = Vec::new();
                    if let Some(doc) = self.tabs[self.active].view.document() {
                        Document::walk_all(&doc.root, &mut |node| {
                            if dbg_matches_query(doc, node, &sel) {
                                let mut buf = String::new();
                                dbg_serialize_html(node, &mut buf, 0);
                                results.push(dbg_json_escape(&buf));
                            }
                        });
                    }
                    format!(
                        r#"{{"ok":true,"count":{},"html":[{}]}}"#,
                        results.len(),
                        results.join(",")
                    )
                }
                None => r#"{"ok":false,"error":"html needs selector"}"#.to_string(),
            },
            // ── Inspect / Computed ───────────────────────────────────────────
            "inspect" => match dbg_json_str(line, "selector") {
                Some(sel) => {
                    let mut parts = Vec::new();
                    if let Some(doc) = self.tabs[self.active].view.document() {
                        Document::walk_all(&doc.root, &mut |node| {
                            if dbg_matches_query(doc, node, &sel) {
                                parts.push(dbg_inspect_json(node));
                            }
                        });
                    }
                    format!(
                        r#"{{"ok":true,"count":{},"elements":[{}]}}"#,
                        parts.len(),
                        parts.join(",")
                    )
                }
                None => r#"{"ok":false,"error":"inspect needs selector"}"#.to_string(),
            },
            "svg-metrics" => {
                if let Some((doc, _renderer)) =
                    self.tabs[self.active].view.document_and_renderer_mut()
                {
                    format!(r#"{{"ok":true,"svg":{}}}"#, dbg_svg_metrics_json(&doc.root))
                } else {
                    r#"{"ok":false,"error":"no document"}"#.to_string()
                }
            }
            "computed" => match dbg_json_str(line, "selector") {
                Some(sel) => {
                    let mut parts = Vec::new();
                    if let Some(doc) = self.tabs[self.active].view.document() {
                        Document::walk_all(&doc.root, &mut |node| {
                            if dbg_matches_query(doc, node, &sel) {
                                parts.push(dbg_computed_json(node));
                            }
                        });
                    }
                    format!(
                        r#"{{"ok":true,"count":{},"elements":[{}]}}"#,
                        parts.len(),
                        parts.join(",")
                    )
                }
                None => r#"{"ok":false,"error":"computed needs selector"}"#.to_string(),
            },
            "height-dump" => {
                let limit = dbg_json_num(line, "limit").unwrap_or(40.0).max(1.0) as usize;
                if let Some(doc) = self.tabs[self.active].view.document() {
                    dbg_height_dump_json(&doc.root, limit)
                } else {
                    r#"{"ok":false,"error":"no document"}"#.to_string()
                }
            }
            "inspect-mode" => {
                let on = dbg_json_str(line, "on")
                    .map(|v| !matches!(v.as_str(), "false" | "0"))
                    .or_else(|| dbg_json_num(line, "on").map(|v| v != 0.0))
                    .unwrap_or(true);
                let pw = self.page_width();
                self.inspect_mode = on;
                self.inspect_panel_pct = if on {
                    self.inspect_panel_pct.max(0.35)
                } else {
                    0.0
                };
                if !on {
                    self.inspect_node = 0;
                }
                let _ = pw;
                self.tabs[self.active].view.set_inspect_mode(on);
                format!(r#"{{"ok":true,"inspect_mode":{}}}"#, on)
            }
            "display-list-stats" => {
                if let Some(doc) = self.tabs[self.active].view.document() {
                    let view_h = self.content_h();
                    let overscan = (view_h * 1.5).max(1200.0);
                    let paint_top = (doc.scroll_y - overscan).max(0.0);
                    let paint_bottom = doc.scroll_y + view_h + overscan;
                    let list = build_display_list_viewport(
                        &doc.root,
                        self.width,
                        view_h,
                        doc.scroll_x,
                        doc.scroll_y,
                        paint_top,
                        paint_bottom,
                        0,
                        0,
                        &std::collections::HashSet::new(),
                        &doc.base_url,
                    );
                    display_list_stats_json(&list, Some((paint_top, paint_bottom)))
                } else {
                    r#"{"ok":false,"error":"no document"}"#.to_string()
                }
            }
            "animated-images" => {
                if let Some(doc) = self.tabs[self.active].view.document() {
                    animated_images_json(doc, self.content_h())
                } else {
                    r#"{"ok":false,"error":"no document"}"#.to_string()
                }
            }
            "image-states" => {
                if let Some(doc) = self.tabs[self.active].view.document() {
                    let limit = dbg_json_num(line, "limit").unwrap_or(80.0).max(1.0) as usize;
                    image_states_json(doc, limit)
                } else {
                    r#"{"ok":false,"error":"no document"}"#.to_string()
                }
            }
            "animations" => {
                if let Some(doc) = self.tabs[self.active].view.document() {
                    animations_json(doc, self.content_h())
                } else {
                    r#"{"ok":false,"error":"no document"}"#.to_string()
                }
            }
            "paint-dump" => {
                let x = dbg_json_num(line, "x").unwrap_or(0.0);
                let y = dbg_json_num(line, "y").unwrap_or(0.0);
                let w = dbg_json_num(line, "w").unwrap_or(self.width);
                let h = dbg_json_num(line, "h").unwrap_or(self.content_h());
                let limit = dbg_json_num(line, "limit").unwrap_or(80.0).max(1.0) as usize;
                let qx2 = x + w.max(0.0);
                let qy2 = y + h.max(0.0);
                let mut out = Vec::new();
                let view_w = self.width;
                let view_h = self.content_h();
                if let Some((doc, renderer)) =
                    self.tabs[self.active].view.document_and_renderer_mut()
                {
                    let font_system = Some(&mut renderer.font_system as *mut _);
                    let list = build_display_list_full_with_font_system(
                        &doc.root,
                        view_w,
                        view_h,
                        doc.scroll_x,
                        doc.scroll_y,
                        0,
                        0,
                        &std::collections::HashSet::new(),
                        &doc.base_url,
                        font_system,
                    );
                    for cmd in &list.commands {
                        if out.len() >= limit {
                            break;
                        }
                        match cmd {
                            PaintCmd::FillRect {
                                rect,
                                color,
                                radius,
                                radius_y,
                            } => {
                                if rect.x <= qx2
                                    && rect.right() >= x
                                    && rect.y <= qy2
                                    && rect.bottom() >= y
                                    && color.a > 0
                                {
                                    out.push(format!(
                                        r##"{{"kind":"fill","x":{:.1},"y":{:.1},"w":{:.1},"h":{:.1},"color":"#{:02x}{:02x}{:02x}{:02x}","radius":[{:.1},{:.1},{:.1},{:.1}],"radius_y":[{:.1},{:.1},{:.1},{:.1}]}}"##,
                                        rect.x, rect.y, rect.w, rect.h,
                                        color.r, color.g, color.b, color.a,
                                        radius[0], radius[1], radius[2], radius[3],
                                        radius_y[0], radius_y[1], radius_y[2], radius_y[3]
                                    ));
                                }
                            }
                            PaintCmd::Border {
                                rect,
                                widths,
                                colors,
                                styles,
                                ..
                            } => {
                                if rect.x <= qx2
                                    && rect.right() >= x
                                    && rect.y <= qy2
                                    && rect.bottom() >= y
                                    && widths.iter().any(|w| *w > 0.0)
                                {
                                    out.push(format!(
                                        r##"{{"kind":"border","x":{:.1},"y":{:.1},"w":{:.1},"h":{:.1},"widths":[{:.1},{:.1},{:.1},{:.1}],"colors":["#{:02x}{:02x}{:02x}{:02x}","#{:02x}{:02x}{:02x}{:02x}","#{:02x}{:02x}{:02x}{:02x}","#{:02x}{:02x}{:02x}{:02x}"],"styles":[{},{},{},{}]}}"##,
                                        rect.x, rect.y, rect.w, rect.h,
                                        widths[0], widths[1], widths[2], widths[3],
                                        colors[0].r, colors[0].g, colors[0].b, colors[0].a,
                                        colors[1].r, colors[1].g, colors[1].b, colors[1].a,
                                        colors[2].r, colors[2].g, colors[2].b, colors[2].a,
                                        colors[3].r, colors[3].g, colors[3].b, colors[3].a,
                                        styles[0], styles[1], styles[2], styles[3]
                                    ));
                                }
                            }
                            PaintCmd::Outline {
                                rect,
                                width,
                                color,
                                style,
                                offset,
                                ..
                            } => {
                                if rect.x <= qx2
                                    && rect.right() >= x
                                    && rect.y <= qy2
                                    && rect.bottom() >= y
                                    && *width > 0.0
                                    && color.a > 0
                                {
                                    out.push(format!(
                                        r##"{{"kind":"outline","x":{:.1},"y":{:.1},"w":{:.1},"h":{:.1},"width":{:.1},"color":"#{:02x}{:02x}{:02x}{:02x}","style":{},"offset":{:.1}}}"##,
                                        rect.x, rect.y, rect.w, rect.h,
                                        width,
                                        color.r, color.g, color.b, color.a,
                                        style,
                                        offset
                                    ));
                                }
                            }
                            PaintCmd::Text {
                                x: tx,
                                y: ty,
                                text,
                                font_family,
                                font_size,
                                font_weight,
                                decoration,
                                ..
                            } => {
                                if *tx <= qx2
                                    && *tx + 1200.0 >= x
                                    && *ty <= qy2
                                    && *ty + *font_size >= y
                                {
                                    out.push(format!(
                                        r#"{{"kind":"text","x":{:.1},"y":{:.1},"font":{},"size":{:.1},"weight":{},"underline":{},"overline":{},"strikethrough":{},"text":{}}}"#,
                                        tx,
                                        ty,
                                        dbg_json_escape(font_family),
                                        font_size,
                                        font_weight,
                                        decoration.underline,
                                        decoration.overline,
                                        decoration.strikethrough,
                                        dbg_json_escape(&text.chars().take(120).collect::<String>())
                                    ));
                                }
                            }
                            PaintCmd::TextShadow {
                                x: tx,
                                y: ty,
                                text,
                                font_family,
                                font_size,
                                ..
                            } => {
                                if *tx <= qx2
                                    && *tx + 1200.0 >= x
                                    && *ty <= qy2
                                    && *ty + *font_size >= y
                                {
                                    out.push(format!(
                                        r#"{{"kind":"text-shadow","x":{:.1},"y":{:.1},"font":{},"size":{:.1},"text":{}}}"#,
                                        tx,
                                        ty,
                                        dbg_json_escape(font_family),
                                        font_size,
                                        dbg_json_escape(&text.chars().take(120).collect::<String>())
                                    ));
                                }
                            }
                            PaintCmd::Image { rect, .. } => {
                                if rect.x <= qx2
                                    && rect.right() >= x
                                    && rect.y <= qy2
                                    && rect.bottom() >= y
                                {
                                    out.push(format!(
                                        r#"{{"kind":"image","x":{:.1},"y":{:.1},"w":{:.1},"h":{:.1}}}"#,
                                        rect.x, rect.y, rect.w, rect.h
                                    ));
                                }
                            }
                            PaintCmd::BackgroundImage {
                                container,
                                clip,
                                draw_w,
                                draw_h,
                                pos_x,
                                pos_y,
                                ..
                            } => {
                                if clip.x <= qx2
                                    && clip.right() >= x
                                    && clip.y <= qy2
                                    && clip.bottom() >= y
                                {
                                    out.push(format!(
                                        r#"{{"kind":"background-image","container":[{:.1},{:.1},{:.1},{:.1}],"clip":[{:.1},{:.1},{:.1},{:.1}],"draw_w":{:.1},"draw_h":{:.1},"pos_x":{:.1},"pos_y":{:.1}}}"#,
                                        container.x, container.y, container.w, container.h,
                                        clip.x, clip.y, clip.w, clip.h,
                                        draw_w, draw_h, pos_x, pos_y
                                    ));
                                }
                            }
                            PaintCmd::Gradient {
                                rect,
                                clip,
                                gradient_type,
                                angle,
                                stops,
                                ..
                            } => {
                                if clip.x <= qx2
                                    && clip.right() >= x
                                    && clip.y <= qy2
                                    && clip.bottom() >= y
                                {
                                    out.push(format!(
                                        r#"{{"kind":"gradient","x":{:.1},"y":{:.1},"w":{:.1},"h":{:.1},"clip":[{:.1},{:.1},{:.1},{:.1}],"gradient_type":{},"angle":{:.1},"stops":{}}}"#,
                                        rect.x, rect.y, rect.w, rect.h,
                                        clip.x, clip.y, clip.w, clip.h,
                                        gradient_type, angle, stops.len()
                                    ));
                                }
                            }
                            PaintCmd::BoxShadow {
                                rect,
                                color,
                                offset_x,
                                offset_y,
                                blur,
                                spread,
                                inset,
                                ..
                            } => {
                                let sx = rect.x + offset_x - spread - blur * 4.0;
                                let sy = rect.y + offset_y - spread - blur * 4.0;
                                let sw = rect.w + spread * 2.0 + blur * 8.0;
                                let sh = rect.h + spread * 2.0 + blur * 8.0;
                                if sx <= qx2 && sx + sw >= x && sy <= qy2 && sy + sh >= y {
                                    out.push(format!(
                                        r##"{{"kind":"box-shadow","x":{:.1},"y":{:.1},"w":{:.1},"h":{:.1},"color":"#{:02x}{:02x}{:02x}{:02x}","offset_x":{:.1},"offset_y":{:.1},"blur":{:.1},"spread":{:.1},"inset":{}}}"##,
                                        rect.x, rect.y, rect.w, rect.h,
                                        color.r, color.g, color.b, color.a,
                                        offset_x, offset_y, blur, spread, inset
                                    ));
                                }
                            }
                            PaintCmd::BeginStackingContext { node_id, z_index } => {
                                out.push(format!(
                                    r#"{{"kind":"begin-stacking-context","node_id":{},"z_index":{}}}"#,
                                    node_id, z_index
                                ));
                            }
                            PaintCmd::EndStackingContext => {
                                out.push(r#"{"kind":"end-stacking-context"}"#.to_string());
                            }
                            PaintCmd::PushClip {
                                rect,
                                radius,
                                radius_y,
                            } => {
                                if rect.x <= qx2
                                    && rect.right() >= x
                                    && rect.y <= qy2
                                    && rect.bottom() >= y
                                {
                                    out.push(format!(
                                        r#"{{"kind":"push-clip","x":{:.1},"y":{:.1},"w":{:.1},"h":{:.1},"radius":[{:.1},{:.1},{:.1},{:.1}],"radius_y":[{:.1},{:.1},{:.1},{:.1}]}}"#,
                                        rect.x, rect.y, rect.w, rect.h,
                                        radius[0], radius[1], radius[2], radius[3],
                                        radius_y[0], radius_y[1], radius_y[2], radius_y[3]
                                    ));
                                }
                            }
                            PaintCmd::PushClipPath { points } => {
                                out.push(format!(
                                    r#"{{"kind":"push-clip-path","points":{}}}"#,
                                    points.len()
                                ));
                            }
                            PaintCmd::PushTransform { node_id, transform } => {
                                out.push(format!(
                                    r#"{{"kind":"push-transform","node_id":{},"matrix":[{:.4},{:.4},{:.4},{:.4},{:.1},{:.1}]}}"#,
                                    node_id,
                                    transform[0], transform[1], transform[2], transform[3],
                                    transform[4], transform[5]
                                ));
                            }
                            PaintCmd::PopTransform => {
                                out.push(r#"{"kind":"pop-transform"}"#.to_string());
                            }
                            PaintCmd::PushOpacity { alpha } => {
                                out.push(format!(
                                    r#"{{"kind":"push-opacity","alpha":{:.3}}}"#,
                                    alpha
                                ));
                            }
                            PaintCmd::PopOpacity => {
                                out.push(r#"{"kind":"pop-opacity"}"#.to_string());
                            }
                            PaintCmd::PushFilter { filters } => {
                                out.push(format!(
                                    r#"{{"kind":"push-filter","filters":{}}}"#,
                                    filters.len()
                                ));
                            }
                            PaintCmd::PopFilter => {
                                out.push(r#"{"kind":"pop-filter"}"#.to_string());
                            }
                            PaintCmd::BackdropFilter { rect, filters } => {
                                if rect.x <= qx2
                                    && rect.right() >= x
                                    && rect.y <= qy2
                                    && rect.bottom() >= y
                                {
                                    out.push(format!(
                                        r#"{{"kind":"backdrop-filter","x":{:.1},"y":{:.1},"w":{:.1},"h":{:.1},"filters":{}}}"#,
                                        rect.x, rect.y, rect.w, rect.h, filters.len()
                                    ));
                                }
                            }
                            PaintCmd::PushMask { rect, data } => {
                                if rect.x <= qx2
                                    && rect.right() >= x
                                    && rect.y <= qy2
                                    && rect.bottom() >= y
                                {
                                    let (mw, mh) = match data {
                                        ImageRef::Owned(_, w, h) | ImageRef::Shared(_, w, h) => {
                                            (*w, *h)
                                        }
                                    };
                                    out.push(format!(
                                        r#"{{"kind":"push-mask","x":{:.1},"y":{:.1},"w":{:.1},"h":{:.1},"mask_w":{},"mask_h":{}}}"#,
                                        rect.x, rect.y, rect.w, rect.h, mw, mh
                                    ));
                                }
                            }
                            PaintCmd::PopMask => {
                                out.push(r#"{"kind":"pop-mask"}"#.to_string());
                            }
                            PaintCmd::PushBlendMode { mode } => {
                                out.push(format!(
                                    r#"{{"kind":"push-blend-mode","mode":{}}}"#,
                                    mode
                                ));
                            }
                            PaintCmd::PopBlendMode => {
                                out.push(r#"{"kind":"pop-blend-mode"}"#.to_string());
                            }
                            PaintCmd::PopClip => {
                                // Standalone pop clips are technically in the global display list,
                                // but in a filtered dump they hide the commands we are trying to
                                // inspect. Keep range dumps focused on drawable/entering commands.
                            }
                            _ => {}
                        }
                    }
                }
                format!(
                    r#"{{"ok":true,"count":{},"commands":[{}]}}"#,
                    out.len(),
                    out.join(",")
                )
            }
            "rule-search" => {
                let query = dbg_json_str(line, "query").unwrap_or_default();
                let limit = dbg_json_num(line, "limit").unwrap_or(20.0).max(1.0) as usize;
                let mut matches = Vec::new();
                if let Some(doc) = self.tabs[self.active].view.document() {
                    for rule in &doc.stylesheet.rules {
                        if matches.len() >= limit {
                            break;
                        }
                        if rule.original_selector.contains(&query) {
                            let decls: Vec<String> = rule
                                .declarations
                                .iter()
                                .filter(|(k, _)| !k.starts_with("--"))
                                .map(|(k, v)| {
                                    format!("{}:{}", dbg_json_escape(k), dbg_json_escape(v))
                                })
                                .collect();
                            let selector_ast = format!("{:?}", rule.selectors);
                            let scope_selector = format!("{:?}", rule.scope_selector);
                            let scope_limit_selector = format!("{:?}", rule.scope_limit_selector);
                            let scopes = format!("{:?}", rule.scopes);
                            matches.push(format!(
                                r#"{{"selector":{},"selector_ast":{},"pseudo":"{:?}","specificity":{},"layer":{},"layer_rank":{},"media":{},"scope":{},"scope_limit":{},"scopes":{},"compiled":{},"compiled_important":{},"declarations":{{{}}}}}"#,
                                dbg_json_escape(&rule.original_selector),
                                dbg_json_escape(&selector_ast),
                                rule.pseudo_element,
                                rule.specificity,
                                dbg_json_escape(&rule.layer),
                                rule.layer_rank,
                                dbg_json_escape(&rule.media_condition),
                                dbg_json_escape(&scope_selector),
                                dbg_json_escape(&scope_limit_selector),
                                dbg_json_escape(&scopes),
                                rule.compiled_decls.len(),
                                rule.compiled_important.len(),
                                decls.join(",")
                            ));
                        }
                    }
                }
                format!(
                    r#"{{"ok":true,"query":{},"count":{},"rules":[{}]}}"#,
                    dbg_json_escape(&query),
                    matches.len(),
                    matches.join(",")
                )
            }
            "style-debug" => match dbg_json_str(line, "selector") {
                Some(sel) => {
                    let limit = dbg_json_num(line, "limit").unwrap_or(20.0).max(1.0) as usize;
                    let query = dbg_json_str(line, "query").unwrap_or_default();
                    let mut parts = Vec::new();
                    if let Some(doc) = self.tabs[self.active].view.document() {
                        Document::walk_all(&doc.root, &mut |node| {
                            if parts.len() >= limit {
                                return;
                            }
                            if dbg_matches_query(doc, node, &sel) {
                                let id = node.attributes.get("id").map(|s| s.as_str());
                                let class_attr = node
                                    .attributes
                                    .get("class")
                                    .map(|s| s.as_str())
                                    .unwrap_or("");
                                let classes: Vec<&str> = class_attr.split_whitespace().collect();
                                let mut candidates = Vec::new();
                                doc.stylesheet.candidate_rules(
                                    &node.tag,
                                    id,
                                    &classes,
                                    &mut candidates,
                                );
                                let candidate_selectors = candidates
                                    .iter()
                                    .take(24)
                                    .filter_map(|idx| doc.stylesheet.rules.get(*idx))
                                    .map(|rule| dbg_json_escape(&rule.original_selector))
                                    .collect::<Vec<_>>();
                                let filtered_candidates = if query.is_empty() {
                                    Vec::new()
                                } else {
                                    candidates
                                        .iter()
                                        .filter_map(|idx| doc.stylesheet.rules.get(*idx))
                                        .filter(|rule| rule.original_selector.contains(&query))
                                        .map(|rule| dbg_json_escape(&rule.original_selector))
                                        .collect::<Vec<_>>()
                                };
                                parts.push(format!(
                                    r#"{{"node_id":{},"tag":{},"id":{},"class":{},"candidate_count":{},"candidates":[{}],"filtered_candidates":[{}]}}"#,
                                    node.node_id,
                                    dbg_json_escape(&node.tag),
                                    dbg_json_escape(id.unwrap_or("")),
                                    dbg_json_escape(class_attr),
                                    candidates.len(),
                                    candidate_selectors.join(","),
                                    filtered_candidates.join(",")
                                ));
                            }
                        });
                    }
                    format!(
                        r#"{{"ok":true,"count":{},"elements":[{}]}}"#,
                        parts.len(),
                        parts.join(",")
                    )
                }
                None => r#"{"ok":false,"error":"style-debug needs selector"}"#.to_string(),
            },
            "match-debug" => match dbg_json_str(line, "selector") {
                Some(sel) => {
                    let mut parts = Vec::new();
                    let limit = dbg_json_num(line, "limit").unwrap_or(10.0).max(1.0) as usize;
                    if let Some(doc) = self.tabs[self.active].view.document() {
                        Document::walk_all(&doc.root, &mut |node| {
                            if parts.len() >= limit {
                                return;
                            }
                            if dbg_matches_query(doc, node, &sel) {
                                parts.push(dbg_match_report_json(doc, node));
                            }
                        });
                    }
                    format!(
                        r#"{{"ok":true,"count":{},"elements":[{}]}}"#,
                        parts.len(),
                        parts.join(",")
                    )
                }
                None => r#"{"ok":false,"error":"match-debug needs selector"}"#.to_string(),
            },
            "node-id-stats" => {
                let mut total = 0usize;
                let mut zero = 0usize;
                let mut ids = std::collections::HashMap::<u32, usize>::new();
                if let Some(doc) = self.tabs[self.active].view.document() {
                    Document::walk_all(&doc.root, &mut |node| {
                        total += 1;
                        if node.node_id == 0 {
                            zero += 1;
                        } else {
                            *ids.entry(node.node_id).or_insert(0) += 1;
                        }
                    });
                }
                let mut duplicates = ids
                    .into_iter()
                    .filter(|(_, count)| *count > 1)
                    .collect::<Vec<_>>();
                duplicates.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
                let dup_json = duplicates
                    .iter()
                    .take(20)
                    .map(|(id, count)| format!(r#"{{"id":{},"count":{}}}"#, id, count))
                    .collect::<Vec<_>>()
                    .join(",");
                format!(
                    r#"{{"ok":true,"total":{},"zero":{},"unique":{},"duplicate_ids":{},"duplicates":[{}]}}"#,
                    total,
                    zero,
                    total.saturating_sub(zero).saturating_sub(
                        duplicates
                            .iter()
                            .map(|(_, count)| count.saturating_sub(1))
                            .sum::<usize>()
                    ),
                    duplicates.len(),
                    dup_json
                )
            }
            "keyframes" => {
                let query = dbg_json_str(line, "query").unwrap_or_default();
                let limit = dbg_json_num(line, "limit").unwrap_or(20.0).max(1.0) as usize;
                let mut matches = Vec::new();
                if let Some(doc) = self.tabs[self.active].view.document() {
                    for (name, stops) in &doc.stylesheet.keyframes {
                        if matches.len() >= limit {
                            break;
                        }
                        if !query.is_empty() && !name.contains(&query) {
                            continue;
                        }
                        let stop_json: Vec<String> = stops
                            .iter()
                            .map(|stop| {
                                let props: Vec<String> = stop
                                    .properties
                                    .iter()
                                    .map(|(k, v)| {
                                        format!("{}:{}", dbg_json_escape(k), dbg_json_escape(v))
                                    })
                                    .collect();
                                format!(
                                    r#"{{"offset":{:.6},"properties":{{{}}}}}"#,
                                    stop.offset,
                                    props.join(",")
                                )
                            })
                            .collect();
                        matches.push(format!(
                            r#"{{"name":{},"stops":[{}]}}"#,
                            dbg_json_escape(name),
                            stop_json.join(",")
                        ));
                    }
                } else {
                    return r#"{"ok":false,"error":"no document"}"#.to_string();
                }
                format!(
                    r#"{{"ok":true,"query":{},"count":{},"keyframes":[{}]}}"#,
                    dbg_json_escape(&query),
                    matches.len(),
                    matches.join(",")
                )
            }
            "rules" | "matched-rules" => match dbg_json_str(line, "selector") {
                Some(sel) => {
                    let mut results = Vec::new();
                    if let Some(doc) = self.tabs[self.active].view.document() {
                        Document::walk_all(&doc.root, &mut |node| {
                            if dbg_matches_query(doc, node, &sel) {
                                let rules: Vec<String> = node.matched_rules.iter().map(|r| {
                                        let decls: Vec<String> = r.declarations.iter()
                                            .filter(|(k, _)| !k.starts_with("--"))
                                            .map(|(k, v)| format!("{}:{}", dbg_json_escape(k), dbg_json_escape(v)))
                                            .collect();
                                        format!(r#"{{"selector":{},"specificity":{},"source":{},"layer":{},"layer_rank":{},"declarations":{{{}}}}}"#,
                                            dbg_json_escape(&r.selector), r.specificity,
                                            dbg_json_escape(&r.source),
                                            dbg_json_escape(&r.layer), r.layer_rank, decls.join(","))
                                    }).collect();
                                let id =
                                    node.attributes.get("id").map(|v| v.as_str()).unwrap_or("");
                                let cls = node
                                    .attributes
                                    .get("class")
                                    .map(|v| v.as_str())
                                    .unwrap_or("");
                                results.push(format!(
                                    r#"{{"tag":{},"id":{},"class":{},"rules":[{}]}}"#,
                                    dbg_json_escape(&node.tag),
                                    dbg_json_escape(id),
                                    dbg_json_escape(cls),
                                    rules.join(",")
                                ));
                            }
                        });
                    }
                    format!(
                        r#"{{"ok":true,"count":{},"elements":[{}]}}"#,
                        results.len(),
                        results.join(",")
                    )
                }
                None => r#"{"ok":false,"error":"rules needs selector"}"#.to_string(),
            },
            // ── Mutation ─────────────────────────────────────────────────────
            "setstyle" => {
                match (
                    dbg_json_str(line, "selector"),
                    dbg_json_str(line, "prop"),
                    dbg_json_str(line, "value"),
                ) {
                    (Some(sel), Some(prop), Some(val)) => {
                        let mut count = 0usize;
                        if let Some(doc) = self.tabs[self.active].view.document_mut() {
                            // Resolved against the PRE-mutation tree: a mutating
                            // walk cannot hold the immutable borrow the engine's
                            // matcher needs, and matching what the selector named
                            // when the command arrived is the right semantics.
                            let hits = dbg_query_ids(doc, &sel);
                            Document::walk_all_mut(&mut doc.root, &mut |node| {
                                if hits.contains(&node.node_id) {
                                    webcore::css::apply_property(
                                        std::sync::Arc::make_mut(&mut node.style),
                                        &prop,
                                        &val,
                                    );
                                    node.layout.layout_dirty = true;
                                    count += 1;
                                }
                            });
                        }
                        if count > 0 {
                            self.relayout_active();
                        }
                        format!(r#"{{"ok":true,"modified":{}}}"#, count)
                    }
                    _ => {
                        r#"{"ok":false,"error":"setstyle needs selector, prop, value"}"#.to_string()
                    }
                }
            }
            "setattr" => {
                match (
                    dbg_json_str(line, "selector"),
                    dbg_json_str(line, "name"),
                    dbg_json_str(line, "value"),
                ) {
                    (Some(sel), Some(name), Some(val)) => {
                        let mut count = 0usize;
                        if let Some(doc) = self.tabs[self.active].view.document_mut() {
                            let hits = dbg_query_ids(doc, &sel);
                            Document::walk_all_mut(&mut doc.root, &mut |node| {
                                if hits.contains(&node.node_id) {
                                    node.attributes.insert(name.clone(), val.clone());
                                    count += 1;
                                }
                            });
                        }
                        if count > 0 {
                            self.relayout_active();
                        }
                        format!(r#"{{"ok":true,"modified":{}}}"#, count)
                    }
                    _ => {
                        r#"{"ok":false,"error":"setattr needs selector, name, value"}"#.to_string()
                    }
                }
            }
            // ── Tree ─────────────────────────────────────────────────────────
            "tree" => {
                let sel = dbg_json_str(line, "selector");
                let mut buf = String::new();
                if let Some(doc) = self.tabs[self.active].view.document() {
                    match sel.as_deref() {
                        Some(sel) => Document::walk_all(&doc.root, &mut |node| {
                            if dbg_matches_query(doc, node, sel) {
                                dbg_dump_box(0, node, &mut buf);
                            }
                        }),
                        None => dbg_dump_box(0, &doc.root, &mut buf),
                    }
                }
                format!(r#"{{"ok":true,"tree":{}}}"#, dbg_json_escape(&buf))
            }
            // ── Highlight ────────────────────────────────────────────────────
            "highlight" => match dbg_json_str(line, "selector") {
                Some(sel) => {
                    let path =
                        dbg_json_str(line, "out").unwrap_or_else(|| "highlight.png".to_string());
                    let scale = dbg_json_num(line, "scale")
                        .map(|v| v.clamp(0.25, 4.0) as f32)
                        .unwrap_or(1.0);
                    let view_w = self.width;
                    let view_h = self.content_h();
                    let phys_w = ((view_w as f32) * scale).ceil() as u32;
                    let ch = ((view_h as f32) * scale).ceil() as u32;
                    let Some(mut pm) = tiny_skia::Pixmap::new(phys_w.max(1), ch.max(1)) else {
                        return r#"{"ok":false,"error":"pixmap alloc failed"}"#.to_string();
                    };
                    pm.fill(tiny_skia::Color::WHITE);
                    let mut count = 0usize;
                    self.tabs[self.active].view.resize(view_w, view_h);
                    self.tabs[self.active].view.paint_into(&mut pm, 0, 0, scale);
                    if let Some(doc) = self.tabs[self.active].view.document() {
                        Document::walk_all(&doc.root, &mut |node| {
                            if dbg_matches_query(doc, node, &sel) {
                                webcore::draw_inspect_overlay(node, &mut pm, 0.0, 0.0, scale);
                                count += 1;
                            }
                        });
                    }
                    match pm.save_png(&path) {
                        Ok(_) => format!(
                            r#"{{"ok":true,"path":{},"highlighted":{}}}"#,
                            dbg_json_escape(&path),
                            count
                        ),
                        Err(e) => format!(
                            r#"{{"ok":false,"error":{}}}"#,
                            dbg_json_escape(&e.to_string())
                        ),
                    }
                }
                None => r#"{"ok":false,"error":"highlight needs selector"}"#.to_string(),
            },
            // ── Misc ─────────────────────────────────────────────────────────
            "perf" => {
                let url = self.tabs[self.active].url.clone();
                let loading = self.tabs[self.active].loading;
                format!(
                    r#"{{"ok":true,"active_tab":{},"loading":{},"tabs":{}}}"#,
                    dbg_json_escape(&url),
                    loading,
                    self.tabs.len()
                )
            }
            "resource-states" => {
                let Some(doc) = self.tabs[self.active].view.document() else {
                    return r#"{"ok":false,"error":"no document"}"#.to_string();
                };
                format!(
                    r#"{{"ok":true,"loading":{},"pending_css":{},"pending_images":{},"images_in_flight":{},"css_rules":{},"linked_stylesheets":{},"loaded_stylesheet_slots":{},"doc_height":{:.0}}}"#,
                    self.tabs[self.active].loading,
                    doc.pending_stylesheets.is_some(),
                    doc.pending_images.is_some(),
                    doc.images_in_flight
                        .load(std::sync::atomic::Ordering::SeqCst),
                    doc.stylesheet.rules.len(),
                    doc.linked_stylesheets.len(),
                    doc.loaded_stylesheet_slots.len(),
                    webcore::Document::scroll_height(&doc.root),
                )
            }
            "memory-stats" => {
                let stats = self.tabs[self.active].view.memory_stats();
                let process = process_memory_stats();
                format!(
                    r#"{{"ok":true,"process_rss_bytes":{},"process_vsz_bytes":{},"viewport_surface_bytes":{},"renderer_cached_content_surface_bytes":{},"renderer_cached_surface_bytes":{},"tile_surface_bytes":{},"tile_count":{},"display_list_commands":{},"display_list_estimated_bytes":{},"display_list_inline_bytes":{},"display_list_heap_bytes":{},"display_list_text_bytes":{},"display_list_image_bytes":{},"display_list_vector_bytes":{},"raw_resource_cache_entries":{},"raw_resource_cache_bytes":{},"parsed_css_cache_entries":{},"parsed_css_cache_bytes":{},"decoded_image_cache_entries":{},"decoded_image_cache_bytes":{},"dom_nodes":{},"image_nodes":{},"unique_styles":{},"dom_estimated_bytes":{},"layout_estimated_bytes":{},"style_estimated_bytes":{},"line_cache_estimated_bytes":{},"matched_rules_estimated_bytes":{},"stylesheet_estimated_bytes":{},"decoded_dom_image_bytes":{}}}"#,
                    process.map(|p| p.rss_bytes).unwrap_or(0),
                    process.map(|p| p.vsz_bytes).unwrap_or(0),
                    stats.viewport_surface_bytes,
                    stats.renderer_cached_content_surface_bytes,
                    stats.renderer_cached_surface_bytes,
                    stats.tile_surface_bytes,
                    stats.tile_count,
                    stats.display_list_commands,
                    stats.display_list_estimated_bytes,
                    stats.display_list_inline_bytes,
                    stats.display_list_heap_bytes,
                    stats.display_list_text_bytes,
                    stats.display_list_image_bytes,
                    stats.display_list_vector_bytes,
                    stats.raw_resource_cache_entries,
                    stats.raw_resource_cache_bytes,
                    stats.parsed_css_cache_entries,
                    stats.parsed_css_cache_bytes,
                    stats.decoded_image_cache_entries,
                    stats.decoded_image_cache_bytes,
                    stats.dom_nodes,
                    stats.image_nodes,
                    stats.unique_styles,
                    stats.dom_estimated_bytes,
                    stats.layout_estimated_bytes,
                    stats.style_estimated_bytes,
                    stats.line_cache_estimated_bytes,
                    stats.matched_rules_estimated_bytes,
                    stats.stylesheet_estimated_bytes,
                    stats.decoded_dom_image_bytes,
                )
            }
            "deep" => {
                let Some(doc) = self.tabs[self.active].view.document() else {
                    return r#"{"ok":false,"error":"no document"}"#.to_string();
                };
                let selector = dbg_json_str(line, "selector").unwrap_or_default();
                let nodes = webcore::dom::query_selector_all(&doc.root, &selector);
                let mut items: Vec<String> = Vec::new();
                for node in nodes {
                    let cr = node.layout.content_rect;
                    let pr = node.layout.padding_rect;
                    let mr = node.layout.margin_rect;
                    let cls = node.attributes.get("class").cloned().unwrap_or_default();
                    let id = node.attributes.get("id").cloned().unwrap_or_default();
                    let children: Vec<String> = node.children.iter()
                        .filter(|c| c.tag != "#text" || !c.text.trim().is_empty())
                        .map(|c| {
                            let cc = c.layout.content_rect;
                            format!(r#"{{"tag":"{}","id":"{}","class":"{}","display":"{:?}","c":[{:.0},{:.0},{:.0},{:.0}]}}"#,
                                c.tag, c.attributes.get("id").unwrap_or(&String::new()),
                                c.attributes.get("class").unwrap_or(&String::new()),
                                c.style.display, cc.x, cc.y, cc.w, cc.h)
                        }).collect();
                    items.push(format!(
                        r#"{{"tag":"{}","id":"{}","class":"{}","content":[{:.0},{:.0},{:.0},{:.0}],"padding":[{:.0},{:.0},{:.0},{:.0}],"margin":[{:.0},{:.0},{:.0},{:.0}],"display":"{:?}","children":[{}],"image_w":{},"image_h":{}}}"#,
                        node.tag, id, cls,
                        cr.x, cr.y, cr.w, cr.h,
                        pr.x, pr.y, pr.w, pr.h,
                        mr.x, mr.y, mr.w, mr.h,
                        node.style.display,
                        children.join(","),
                        node.image_width, node.image_height
                    ));
                }
                format!(
                    r#"{{"ok":true,"count":{},"elements":[{}]}}"#,
                    items.len(),
                    items.join(",")
                )
            }
            "css" => {
                let Some(doc) = self.tabs[self.active].view.document() else {
                    return r#"{"ok":false,"error":"no document"}"#.to_string();
                };
                let selector = dbg_json_str(line, "selector").unwrap_or_default();
                let props_str = dbg_json_str(line, "props").unwrap_or_default();
                // Ids as well as nodes: the fallback resolver needs `&mut doc`,
                // so a node borrow cannot be held across it.
                let ids = webcore::dom::query_selector_all_ids(&doc.root, &selector);
                let nodes = dbg_select_with_pseudo(&doc.root, &selector);
                let mut items: Vec<String> = Vec::new();
                let mut fallbacks: Vec<(usize, String)> = Vec::new();
                for (ni, node) in nodes.into_iter().enumerate() {
                    let mut kv: Vec<String> = Vec::new();
                    kv.push(format!(r#""tag":"{}""#, node.tag));
                    kv.push(format!(
                        r#""id":"{}""#,
                        node.attributes.get("id").unwrap_or(&String::new())
                    ));
                    kv.push(format!(
                        r#""class":"{}""#,
                        node.attributes.get("class").unwrap_or(&String::new())
                    ));
                    for prop in props_str.split(',') {
                        let prop = prop.trim();
                        if prop.is_empty() {
                            continue;
                        }
                        let val = match prop {
                            "display" => format!("{:?}", node.style.display),
                            "position" => format!("{:?}", node.style.position),
                            "width" => format!("{:?}", node.style.width),
                            "height" => format!("{:?}", node.style.height),
                            "content-rect" => {
                                let r = node.layout.content_rect;
                                format!("{:.1},{:.1} {:.1}x{:.1}", r.x, r.y, r.w, r.h)
                            }
                            "padding-rect" => {
                                let r = node.layout.padding_rect;
                                format!("{:.1},{:.1} {:.1}x{:.1}", r.x, r.y, r.w, r.h)
                            }
                            "margin-rect" => {
                                let r = node.layout.margin_rect;
                                format!("{:.1},{:.1} {:.1}x{:.1}", r.x, r.y, r.w, r.h)
                            }
                            "border-rect" => {
                                let r = node.layout.border_rect;
                                format!("{:.1},{:.1} {:.1}x{:.1}", r.x, r.y, r.w, r.h)
                            }
                            "line-count" => format!("{}", node.layout.line_cache.len()),
                            // The raw url() from the cascade, and whether the
                            // bytes for it ever arrived — the two halves of a
                            // background image, separately observable.
                            "background-image-url" => node.style.background_image_url.clone(),
                            "background-loaded" => format!("{}", node.bg_image_data.is_some()),
                            "background-px" => {
                                format!("{}x{}", node.bg_image_width, node.bg_image_height)
                            }
                            _ => format!("(unknown: {})", prop),
                        };
                        if val.is_empty() {
                            fallbacks.push((ni, prop.to_string()));
                        }
                        kv.push(format!(r#""{}":"{}""#, prop, val));
                    }
                    items.push(format!("{{{}}}", kv.join(",")));
                }
                // Second pass for the properties the fast match did not know.
                if !fallbacks.is_empty() {
                    if let Some(doc) = self.tabs[self.active].view.document_mut() {
                        for (ni, prop) in fallbacks {
                            let Some(&nid) = ids.get(ni) else { continue };
                            let val = doc.computed_style_property(nid, &prop);
                            let empty = format!("\"{}\":\"\"", prop);
                            let filled = format!("\"{}\":\"{}\"", prop, dbg_json_escape(&val));
                            items[ni] = items[ni].replace(&empty, &filled);
                        }
                    }
                }
                format!(
                    r#"{{"ok":true,"count":{},"elements":[{}]}}"#,
                    items.len(),
                    items.join(",")
                )
            }
            "bench" => {
                let n = dbg_json_num(line, "n").unwrap_or(1.0) as u32;
                let n = n.max(1).min(100);
                if self.tabs[self.active].view.document().is_none() {
                    return r#"{"ok":false,"error":"no document"}"#.to_string();
                }
                let mut layout_times = Vec::new();
                for _ in 0..n {
                    let t = std::time::Instant::now();
                    self.tabs[self.active].view.relayout();
                    layout_times.push(t.elapsed().as_micros() as f64 / 1000.0);
                }
                let avg = layout_times.iter().sum::<f64>() / layout_times.len() as f64;
                format!(
                    r#"{{"ok":true,"iterations":{},"layout_avg_ms":{:.1}}}"#,
                    n, avg
                )
            }
            // ── DOM mutation commands ─────────────────────────────────────
            "set-text" => {
                let Some(doc) = self.tabs[self.active].view.document_mut() else {
                    return r#"{"ok":false,"error":"no document"}"#.to_string();
                };
                let selector = dbg_json_str(line, "selector").unwrap_or_default();
                let text = dbg_json_str(line, "text").unwrap_or_default();
                if let Some(node) = webcore::dom::query_selector_mut(&mut doc.root, &selector) {
                    webcore::dom::set_text_content(node, &text);
                }
                self.relayout_active();
                r#"{"ok":true}"#.to_string()
            }
            "add-class" => {
                let Some(doc) = self.tabs[self.active].view.document_mut() else {
                    return r#"{"ok":false,"error":"no document"}"#.to_string();
                };
                let selector = dbg_json_str(line, "selector").unwrap_or_default();
                let cls = dbg_json_str(line, "class").unwrap_or_default();
                if let Some(node) = webcore::dom::query_selector_mut(&mut doc.root, &selector) {
                    webcore::dom::add_class(node, &cls);
                }
                doc.style_dirty = true;
                self.relayout_active();
                r#"{"ok":true}"#.to_string()
            }
            "remove-class" => {
                let Some(doc) = self.tabs[self.active].view.document_mut() else {
                    return r#"{"ok":false,"error":"no document"}"#.to_string();
                };
                let selector = dbg_json_str(line, "selector").unwrap_or_default();
                let cls = dbg_json_str(line, "class").unwrap_or_default();
                if let Some(node) = webcore::dom::query_selector_mut(&mut doc.root, &selector) {
                    webcore::dom::remove_class(node, &cls);
                }
                doc.style_dirty = true;
                self.relayout_active();
                r#"{"ok":true}"#.to_string()
            }
            "toggle-class" => {
                let Some(doc) = self.tabs[self.active].view.document_mut() else {
                    return r#"{"ok":false,"error":"no document"}"#.to_string();
                };
                let selector = dbg_json_str(line, "selector").unwrap_or_default();
                let cls = dbg_json_str(line, "class").unwrap_or_default();
                if let Some(node) = webcore::dom::query_selector_mut(&mut doc.root, &selector) {
                    webcore::dom::toggle_class(node, &cls);
                }
                doc.style_dirty = true;
                self.relayout_active();
                r#"{"ok":true}"#.to_string()
            }
            // ── Event listeners query ────────────────────────────────────────
            "event-listeners" => {
                let Some(doc) = self.tabs[self.active].view.document() else {
                    return r#"{"ok":false,"error":"no document"}"#.to_string();
                };
                // There is one listener system now — the WHATWG one.
                let has_listeners = !doc.event_targets.is_empty();
                format!(r#"{{"ok":true,"listeners":{}}}"#, has_listeners)
            }
            // ── Force element state ──────────────────────────────────────────
            "force-state" => {
                let Some(doc) = self.tabs[self.active].view.document_mut() else {
                    return r#"{"ok":false,"error":"no document"}"#.to_string();
                };
                let selector = dbg_json_str(line, "selector").unwrap_or_default();
                let state = dbg_json_str(line, "state").unwrap_or_default();
                if let Some(node) = webcore::dom::query_selector(&doc.root, &selector) {
                    let nid = node.node_id;
                    match state.as_str() {
                        "hover" => {
                            doc.hovered_box = nid;
                            doc.hover_changed = true;
                        }
                        "focus" => {
                            doc.focused_box = nid;
                        }
                        "active" => {
                            doc.active_box = nid;
                        }
                        _ => {}
                    }
                }
                doc.style_dirty = true;
                self.relayout_active();
                r#"{"ok":true}"#.to_string()
            }
            // ── Search DOM by text content ───────────────────────────────────
            "search" => {
                let Some(doc) = self.tabs[self.active].view.document() else {
                    return r#"{"ok":false,"error":"no document"}"#.to_string();
                };
                let query = dbg_json_str(line, "query")
                    .unwrap_or_default()
                    .to_lowercase();
                let mut results: Vec<String> = Vec::new();
                fn search_walk(
                    doc: &webcore::Document,
                    node: &webcore::WebCore,
                    q: &str,
                    results: &mut Vec<String>,
                ) {
                    if node.tag == "#text" && node.text.to_lowercase().contains(q) {
                        // ⛔ Ask the DOM. This read `node.parent`, a render-tree
                        // field the mutation APIs never maintained, so every
                        // script-created node reported `parent_id: 0`.
                        let pid = doc.parent_node(node.node_id);
                        results.push(format!(
                            r#"{{"node_id":{},"parent_id":{},"text":{}}}"#,
                            node.node_id,
                            pid,
                            dbg_json_escape(
                                &node.text.trim().chars().take(100).collect::<String>()
                            )
                        ));
                    }
                    for child in &node.children {
                        search_walk(doc, child, q, results);
                    }
                }
                search_walk(doc, &doc.root, &query, &mut results);
                format!(
                    r#"{{"ok":true,"count":{},"results":[{}]}}"#,
                    results.len(),
                    results.join(",")
                )
            }
            // ── Box model (Chrome-style) ─────────────────────────────────────
            "box-model" => {
                let Some(doc) = self.tabs[self.active].view.document() else {
                    return r#"{"ok":false,"error":"no document"}"#.to_string();
                };
                let selector = dbg_json_str(line, "selector").unwrap_or_default();
                if let Some(node) = webcore::dom::query_selector(&doc.root, &selector) {
                    let l = &node.layout;
                    format!(
                        concat!(
                            r#"{{"ok":true,"tag":"{}","margin":{{"top":{:.1},"right":{:.1},"bottom":{:.1},"left":{:.1}}},"#,
                            r#""border":{{"top":{:.1},"right":{:.1},"bottom":{:.1},"left":{:.1}}},"#,
                            r#""padding":{{"top":{:.1},"right":{:.1},"bottom":{:.1},"left":{:.1}}},"#,
                            r#""content":{{"width":{:.1},"height":{:.1}}}}}"#
                        ),
                        node.tag,
                        l.resolved_margin_top,
                        l.resolved_margin_right,
                        l.resolved_margin_bottom,
                        l.resolved_margin_left,
                        l.resolved_border_top,
                        l.resolved_border_right,
                        l.resolved_border_bottom,
                        l.resolved_border_left,
                        l.resolved_pad_top,
                        l.resolved_pad_right,
                        l.resolved_pad_bottom,
                        l.resolved_pad_left,
                        l.content_rect.w,
                        l.content_rect.h,
                    )
                } else {
                    format!(
                        r#"{{"ok":false,"error":"no match: {}"}}"#,
                        dbg_json_escape(&selector)
                    )
                }
            }
            // ── Network log ──────────────────────────────────────────────────
            "network" => {
                // Return basic info about loaded resources
                let Some(doc) = self.tabs[self.active].view.document() else {
                    return r#"{"ok":false,"error":"no document"}"#.to_string();
                };
                let css_count = doc.linked_stylesheets.len();
                let mut img_count = 0u32;
                webcore::Document::walk_all(&doc.root, &mut |b| {
                    if b.image_data.is_some() {
                        img_count += 1;
                    }
                });
                format!(
                    r#"{{"ok":true,"stylesheets":{},"images_loaded":{}}}"#,
                    css_count, img_count
                )
            }
            // ── HTML media element control/state ────────────────────────────
            "media" => {
                let selector = dbg_json_str(line, "selector").unwrap_or_default();
                let action = dbg_json_str(line, "action").unwrap_or_else(|| "state".to_string());
                let Some(doc) = self.tabs[self.active].view.document_mut() else {
                    return r#"{"ok":false,"error":"no document"}"#.to_string();
                };
                let Some(id) = doc.query_selector(&selector) else {
                    return format!(
                        r#"{{"ok":false,"error":"no match: {}"}}"#,
                        dbg_json_escape(&selector)
                    );
                };
                if !doc.is_media_element(id) {
                    return format!(
                        r#"{{"ok":false,"error":"not media: {}"}}"#,
                        dbg_json_escape(&selector)
                    );
                }
                let changed = match action.as_str() {
                    "play" => doc.media_play(id),
                    "pause" => doc.media_pause(id),
                    "toggle" => doc.media_toggle_playback(id),
                    "load" => doc.media_load(id),
                    "seek" => dbg_json_num(line, "time")
                        .map(|time| doc.media_set_current_time(id, time))
                        .unwrap_or(false),
                    "volume" => dbg_json_num(line, "value")
                        .map(|volume| doc.media_set_volume(id, volume))
                        .unwrap_or(false),
                    "muted" => dbg_json_bool(line, "value")
                        .map(|muted| doc.media_set_muted(id, muted))
                        .unwrap_or(false),
                    "rate" => dbg_json_num(line, "value")
                        .map(|rate| doc.media_set_playback_rate(id, rate))
                        .unwrap_or(false),
                    "state" => true,
                    _ => false,
                };
                let src = doc.media_current_src(id).unwrap_or_default();
                let current_time = doc.media_current_time(id).unwrap_or(0.0);
                let duration = doc
                    .media_duration(id)
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "null".to_string());
                let ready = doc.media_ready_state(id).unwrap_or(0);
                let network = doc.media_network_state(id).unwrap_or(0);
                let paused = doc.media_paused(id).unwrap_or(true);
                let ended = doc.media_ended(id).unwrap_or(false);
                let volume = doc.media_volume(id).unwrap_or(1.0);
                let muted = doc.media_muted(id).unwrap_or(false);
                let tracks = doc.media_text_tracks(id).unwrap_or_default();
                let tracks_json = tracks
                    .iter()
                    .map(|track| {
                        format!(
                            r#"{{"kind":{},"label":{},"language":{},"src":{},"default":{}}}"#,
                            dbg_json_escape(&track.kind),
                            dbg_json_escape(&track.label),
                            dbg_json_escape(&track.language),
                            dbg_json_escape(&track.src),
                            track.default
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                if changed && action != "state" {
                    self.tabs[self.active].view.relayout();
                }
                format!(
                    r#"{{"ok":true,"changed":{},"node_id":{},"src":{},"currentTime":{:.3},"duration":{},"readyState":{},"networkState":{},"paused":{},"ended":{},"volume":{:.3},"muted":{},"textTracks":[{}]}}"#,
                    changed,
                    id,
                    dbg_json_escape(&src),
                    current_time,
                    duration,
                    ready,
                    network,
                    paused,
                    ended,
                    volume,
                    muted,
                    tracks_json
                )
            }
            // ── HTML output ────────────────────────────────────────────────
            "dom-html" => {
                let Some(doc) = self.tabs[self.active].view.document() else {
                    return r#"{"ok":false,"error":"no document"}"#.to_string();
                };
                let selector = dbg_json_str(line, "selector").unwrap_or_default();
                if let Some(node) = webcore::dom::query_selector(&doc.root, &selector) {
                    let html = {
                        let mut s = String::new();
                        webcore::html::serializer::serialize_box(node, &mut s);
                        s
                    };
                    format!(r#"{{"ok":true,"html":{}}}"#, dbg_json_escape(&html))
                } else {
                    format!(r#"{{"ok":false,"error":"no match"}}"#)
                }
            }
            // ── Viewport info ────────────────────────────────────────────────
            "viewport" => {
                let scroll_x = self
                    .tabs
                    .get(self.active)
                    .and_then(|t| t.view.document())
                    .map(|d| d.scroll_x)
                    .unwrap_or(0.0);
                let scroll_y = self
                    .tabs
                    .get(self.active)
                    .and_then(|t| t.view.document())
                    .map(|d| d.scroll_y)
                    .unwrap_or(0.0);
                let doc_h = self
                    .tabs
                    .get(self.active)
                    .and_then(|t| t.view.document())
                    .map(|d| Document::scroll_height(&d.root))
                    .unwrap_or(0.0);
                format!(
                    r#"{{"ok":true,"width":{:.0},"height":{:.0},"scroll_x":{:.1},"scroll_y":{:.1},"doc_height":{:.0},"scale":{:.1}}}"#,
                    self.width,
                    self.content_h(),
                    scroll_x,
                    scroll_y,
                    doc_h,
                    self.tabs[self.active].view.zoom()
                )
            }
            // ── Accessibility tree ───────────────────────────────────────────
            "accessibility" | "a11y" => {
                let Some(doc) = self.tabs[self.active].view.document() else {
                    return r#"{"ok":false,"error":"no document"}"#.to_string();
                };
                fn a11y_walk(node: &webcore::WebCore, depth: usize, out: &mut String) {
                    let role = match node.tag.as_str() {
                        "a" => "link",
                        "button" | "input" => "button",
                        "img" => "image",
                        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => "heading",
                        "nav" => "navigation",
                        "main" => "main",
                        "header" => "banner",
                        "footer" => "contentinfo",
                        "ul" | "ol" => "list",
                        "li" => "listitem",
                        "table" => "table",
                        "tr" => "row",
                        "td" | "th" => "cell",
                        "form" => "form",
                        "section" => "region",
                        "article" => "article",
                        "aside" => "complementary",
                        "#text" => {
                            if !node.text.trim().is_empty() {
                                "text"
                            } else {
                                return;
                            }
                        }
                        _ => {
                            let aria = node
                                .attributes
                                .get("role")
                                .map(|s| s.as_str())
                                .unwrap_or("");
                            if !aria.is_empty() { aria } else { "" }
                        }
                    };
                    if !role.is_empty() {
                        let indent = "  ".repeat(depth);
                        let label = node
                            .attributes
                            .get("aria-label")
                            .or(node.attributes.get("alt"))
                            .or(node.attributes.get("title"))
                            .cloned()
                            .unwrap_or_else(|| {
                                if node.tag == "#text" {
                                    node.text.trim().chars().take(50).collect()
                                } else {
                                    String::new()
                                }
                            });
                        out.push_str(&format!("{}{}: {}\n", indent, role, label));
                    }
                    for child in &node.children {
                        a11y_walk(child, depth + if !role.is_empty() { 1 } else { 0 }, out);
                    }
                }
                let mut tree = String::new();
                a11y_walk(&doc.root, 0, &mut tree);
                format!(r#"{{"ok":true,"tree":{}}}"#, dbg_json_escape(&tree))
            }
            // ── Measure distance between elements ────────────────────────────
            "measure" => {
                let Some(doc) = self.tabs[self.active].view.document() else {
                    return r#"{"ok":false,"error":"no document"}"#.to_string();
                };
                let from = dbg_json_str(line, "from").unwrap_or_default();
                let to = dbg_json_str(line, "to").unwrap_or_default();
                let a =
                    webcore::dom::query_selector(&doc.root, &from).map(|n| n.layout.border_rect);
                let b = webcore::dom::query_selector(&doc.root, &to).map(|n| n.layout.border_rect);
                match (a, b) {
                    (Some(a), Some(b)) => {
                        let dx = b.x - (a.x + a.w); // gap between right of A and left of B
                        let dy = b.y - (a.y + a.h); // gap between bottom of A and top of B
                        let cx = (b.x + b.w / 2.0) - (a.x + a.w / 2.0); // center-to-center
                        let cy = (b.y + b.h / 2.0) - (a.y + a.h / 2.0);
                        format!(
                            r#"{{"ok":true,"gap_x":{:.1},"gap_y":{:.1},"center_dx":{:.1},"center_dy":{:.1}}}"#,
                            dx, dy, cx, cy
                        )
                    }
                    _ => r#"{"ok":false,"error":"one or both selectors not found"}"#.to_string(),
                }
            }
            // ── Web inspector UI ─────────────────────────────────────────────
            "inspector" | "devtools" => {
                format!(
                    r#"{{"ok":true,"message":"Connect browser to http://127.0.0.1:{}/inspector to use the web UI"}}"#,
                    self.debug_cmd_rx
                        .as_ref()
                        .map(|_| "debug-port")
                        .unwrap_or("?")
                )
            }
            // ── Structured DOM tree (JSON) ────────────────────────────────
            // ── Inspect by node_id ────────────────────────────────────────
            "inspect-node" => {
                let Some(doc) = self.tabs[self.active].view.document() else {
                    return r#"{"ok":false,"error":"no document"}"#.to_string();
                };
                let nid = dbg_json_num(line, "nid").unwrap_or(0.0) as u32;
                if let Some(node) = doc.get_box_by_id(nid) {
                    let l = &node.layout;
                    let tag = &node.tag;
                    let id = node.attributes.get("id").map(|s| s.as_str()).unwrap_or("");
                    let cls = node
                        .attributes
                        .get("class")
                        .map(|s| s.as_str())
                        .unwrap_or("");
                    format!(
                        concat!(
                            r#"{{"ok":true,"tag":"{}","id":"{}","class":"{}","nid":{},"#,
                            r#""display":"{:?}","position":"{:?}","#,
                            r#""margin":{{"top":{:.1},"right":{:.1},"bottom":{:.1},"left":{:.1}}},"#,
                            r#""border":{{"top":{:.1},"right":{:.1},"bottom":{:.1},"left":{:.1}}},"#,
                            r#""padding":{{"top":{:.1},"right":{:.1},"bottom":{:.1},"left":{:.1}}},"#,
                            r#""content":{{"x":{:.1},"y":{:.1},"width":{:.1},"height":{:.1}}},"#,
                            r#""font_size":{:.1},"color":"{:02x}{:02x}{:02x}","bg":"{:02x}{:02x}{:02x}{:02x}","#,
                            r#""image":{{"width":{},"height":{},"bytes":{},"src":"{}"}}"#,
                            r#"}}"#
                        ),
                        tag,
                        id,
                        cls,
                        nid,
                        node.style.display,
                        node.style.position,
                        l.resolved_margin_top,
                        l.resolved_margin_right,
                        l.resolved_margin_bottom,
                        l.resolved_margin_left,
                        l.resolved_border_top,
                        l.resolved_border_right,
                        l.resolved_border_bottom,
                        l.resolved_border_left,
                        l.resolved_pad_top,
                        l.resolved_pad_right,
                        l.resolved_pad_bottom,
                        l.resolved_pad_left,
                        l.content_rect.x,
                        l.content_rect.y,
                        l.content_rect.w,
                        l.content_rect.h,
                        node.style.font_size_px(16.0, 16.0),
                        node.style.color.r,
                        node.style.color.g,
                        node.style.color.b,
                        node.style.background_color.r,
                        node.style.background_color.g,
                        node.style.background_color.b,
                        node.style.background_color.a,
                        node.image_width,
                        node.image_height,
                        node.image_data.as_ref().map(|d| d.len()).unwrap_or(0),
                        node.resolved_src.replace('"', "\\\""),
                    )
                } else {
                    format!(r#"{{"ok":false,"error":"node {} not found"}}"#, nid)
                }
            }
            "dom-tree" => {
                let Some(doc) = self.tabs[self.active].view.document() else {
                    return r#"{"ok":false,"error":"no document"}"#.to_string();
                };
                let max_depth = dbg_json_num(line, "depth").unwrap_or(3.0) as usize;
                let nid = dbg_json_num(line, "nid").map(|n| n as u32);
                let root_sel = dbg_json_str(line, "selector");
                // Find root node: by nid, by selector, or document root
                let root_node = if let Some(id) = nid {
                    doc.get_box_by_id(id).unwrap_or(&doc.root)
                } else if let Some(sel) = root_sel {
                    webcore::dom::query_selector(&doc.root, &sel).unwrap_or(&doc.root)
                } else {
                    &doc.root
                };
                fn tree_json(node: &webcore::WebCore, depth: usize, max_depth: usize) -> String {
                    let tag = &node.tag;
                    let id = node.attributes.get("id").map(|s| s.as_str()).unwrap_or("");
                    let cls = node
                        .attributes
                        .get("class")
                        .map(|s| s.as_str())
                        .unwrap_or("");
                    let nid = node.node_id;
                    let cr = node.layout.content_rect;
                    let child_count = node
                        .children
                        .iter()
                        .filter(|c| !(c.tag == "#text" && c.text.trim().is_empty()))
                        .count();
                    let text_preview = if tag == "#text" {
                        let t: String = node.text.trim().chars().take(60).collect();
                        format!(
                            r#","text":"{}""#,
                            t.replace('\\', "\\\\").replace('"', "\\\"")
                        )
                    } else {
                        String::new()
                    };
                    let children_json = if depth < max_depth && child_count > 0 {
                        let kids: Vec<String> = node
                            .children
                            .iter()
                            .filter(|c| !(c.tag == "#text" && c.text.trim().is_empty()))
                            .map(|c| tree_json(c, depth + 1, max_depth))
                            .collect();
                        format!(r#","children":[{}]"#, kids.join(","))
                    } else if child_count > 0 {
                        format!(r#","child_count":{}"#, child_count)
                    } else {
                        String::new()
                    };
                    format!(
                        r#"{{"tag":"{}","id":"{}","class":"{}","nid":{},"rect":[{:.0},{:.0},{:.0},{:.0}]{}{}{}}}"#,
                        tag,
                        id,
                        cls,
                        nid,
                        cr.x,
                        cr.y,
                        cr.w,
                        cr.h,
                        text_preview,
                        if child_count > 0 {
                            format!(r#","count":{}"#, child_count)
                        } else {
                            String::new()
                        },
                        children_json
                    )
                }
                let json = tree_json(root_node, 0, max_depth);
                format!(r#"{{"ok":true,"tree":{}}}"#, json)
            }
            // ── DOM path (CSS selector chain to element) ─────────────────
            "dom-path" | "path" => {
                let Some(doc) = self.tabs[self.active].view.document() else {
                    return r#"{"ok":false,"error":"no document"}"#.to_string();
                };
                let selector = dbg_json_str(line, "selector").unwrap_or_default();
                let nodes = webcore::dom::query_selector_all(&doc.root, &selector);
                let mut paths: Vec<String> = Vec::new();
                for node in nodes {
                    fn build_path(
                        root: &webcore::WebCore,
                        target_id: u32,
                        path: &mut Vec<String>,
                    ) -> bool {
                        let id = root
                            .attributes
                            .get("id")
                            .map(|v| format!("#{v}"))
                            .unwrap_or_default();
                        let cls = root
                            .attributes
                            .get("class")
                            .map(|v| format!(".{}", v.split_whitespace().next().unwrap_or("")))
                            .unwrap_or_default();
                        path.push(format!("{}{}{}", root.tag, id, cls));
                        if root.node_id == target_id {
                            return true;
                        }
                        for child in &root.children {
                            if build_path(child, target_id, path) {
                                return true;
                            }
                        }
                        path.pop();
                        false
                    }
                    let mut p = Vec::new();
                    build_path(&doc.root, node.node_id, &mut p);
                    paths.push(dbg_json_escape(&p.join(" > ")));
                }
                format!(
                    r#"{{"ok":true,"count":{},"paths":[{}]}}"#,
                    paths.len(),
                    paths.join(",")
                )
            }
            // ── Parent / ancestor chain ──────────────────────────────────────
            "parent" => {
                let Some(doc) = self.tabs[self.active].view.document() else {
                    return r#"{"ok":false,"error":"no document"}"#.to_string();
                };
                let selector = dbg_json_str(line, "selector").unwrap_or_default();
                if let Some(node) = webcore::dom::query_selector(&doc.root, &selector) {
                    fn ancestors(
                        root: &webcore::WebCore,
                        target_id: u32,
                        chain: &mut Vec<String>,
                    ) -> bool {
                        if root.node_id == target_id {
                            let id = root.attributes.get("id").cloned().unwrap_or_default();
                            let cls = root.attributes.get("class").cloned().unwrap_or_default();
                            chain.push(format!(
                                r#"{{"tag":"{}","id":"{}","class":"{}","nid":{}}}"#,
                                root.tag, id, cls, root.node_id
                            ));
                            return true;
                        }
                        for child in &root.children {
                            if ancestors(child, target_id, chain) {
                                let id = root.attributes.get("id").cloned().unwrap_or_default();
                                let cls = root.attributes.get("class").cloned().unwrap_or_default();
                                chain.push(format!(
                                    r#"{{"tag":"{}","id":"{}","class":"{}","nid":{}}}"#,
                                    root.tag, id, cls, root.node_id
                                ));
                                return true;
                            }
                        }
                        false
                    }
                    let mut chain = Vec::new();
                    ancestors(&doc.root, node.node_id, &mut chain);
                    format!(r#"{{"ok":true,"chain":[{}]}}"#, chain.join(","))
                } else {
                    r#"{"ok":false,"error":"not found"}"#.to_string()
                }
            }
            // ── Hit test at coordinates ──────────────────────────────────────
            "hit" => {
                let Some(doc) = self.tabs[self.active].view.document() else {
                    return r#"{"ok":false,"error":"no document"}"#.to_string();
                };
                let x = dbg_json_num(line, "x").unwrap_or(0.0) as f32;
                let y = dbg_json_num(line, "y").unwrap_or(0.0) as f32;
                if let Some(hit) = webcore::layout::hit_test::point_to_hit(&doc.root, (x, y), 0) {
                    if let Some(node) = doc.get_box_by_id(hit.node_id) {
                        let id = node.attributes.get("id").cloned().unwrap_or_default();
                        let cls = node.attributes.get("class").cloned().unwrap_or_default();
                        format!(
                            r#"{{"ok":true,"nid":{},"tag":"{}","id":"{}","class":"{}"}}"#,
                            hit.node_id, node.tag, id, cls
                        )
                    } else {
                        format!(r#"{{"ok":true,"nid":{},"tag":"?"}}"#, hit.node_id)
                    }
                } else {
                    r#"{"ok":false,"error":"no hit"}"#.to_string()
                }
            }
            // ── Progressive layout benchmark ─────────────────────────────────
            "bench-progressive" => {
                let Some((full_ms, above_ms)) =
                    self.tabs[self.active].view.benchmark_progressive_layout()
                else {
                    return r#"{"ok":false,"error":"no document"}"#.to_string();
                };
                format!(
                    r#"{{"ok":true,"full_ms":{:.1},"above_fold_ms":{:.1}}}"#,
                    full_ms, above_ms
                )
            }
            "bench-render" => {
                let scale = dbg_json_num(line, "scale")
                    .map(|v| v.clamp(0.25, 4.0) as f32)
                    .unwrap_or(1.0);
                let dy = dbg_json_num(line, "dy").unwrap_or(500.0) as f32;
                let view_w = self.page_width();
                let view_h = self.content_h();
                let phys_w = (view_w * scale).ceil().max(1.0) as u32;
                let phys_h = (view_h * scale).ceil().max(1.0) as u32;
                let Some(mut pm) = tiny_skia::Pixmap::new(phys_w, phys_h) else {
                    return r#"{"ok":false,"error":"pixmap failed"}"#.to_string();
                };
                let view = &mut self.tabs[self.active].view;
                let original_y = view.scroll_y();
                let t0 = std::time::Instant::now();
                view.paint_into(&mut pm, 0, 0, scale);
                let first = t0.elapsed().as_micros() as f64 / 1000.0;
                view.scroll_by(0.0, dy);
                let t1 = std::time::Instant::now();
                view.paint_into(&mut pm, 0, 0, scale);
                let scroll = t1.elapsed().as_micros() as f64 / 1000.0;
                let t2 = std::time::Instant::now();
                view.paint_into(&mut pm, 0, 0, scale);
                let cached = t2.elapsed().as_micros() as f64 / 1000.0;
                let scroll_y = view.scroll_y();
                view.scroll_to(0.0, original_y);
                view.paint_into(&mut pm, 0, 0, scale);
                format!(
                    r#"{{"ok":true,"first_ms":{:.1},"scroll_ms":{:.1},"cached_ms":{:.1},"scroll_y":{:.0},"width":{},"height":{},"scale":{}}}"#,
                    first, scroll, cached, scroll_y, phys_w, phys_h, scale
                )
            }
            "quit" => {
                std::process::exit(0);
            }
            _ => format!(
                r#"{{"ok":false,"error":"unknown command: {}"}}"#,
                dbg_json_escape(cmd)
            ),
        }
    }
}

fn debug_cmd_requests_redraw(cmd: &str) -> bool {
    matches!(
        cmd,
        "navigate"
            | "switch-tab"
            | "resize"
            | "scroll"
            | "click"
            | "hover"
            | "type"
            | "key"
            | "set-text"
            | "add-class"
            | "remove-class"
            | "toggle-class"
            | "force-state"
            | "bench"
            | "bench-progressive"
    )
}

/// TCP listener for the browser debug server.
fn browser_debug_spawn_tcp(
    port: u16,
    cmd_tx: mpsc::Sender<(String, mpsc::Sender<String>)>,
    proxy: EventLoopProxy<()>,
) {
    std::thread::spawn(move || {
        let listener = match TcpListener::bind(format!("127.0.0.1:{port}")) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("[debug] Failed to bind port {port}: {e}");
                return;
            }
        };
        eprintln!("[debug] Listening on 127.0.0.1:{port}");
        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    let peer = stream
                        .peer_addr()
                        .map(|a| a.to_string())
                        .unwrap_or_default();
                    eprintln!("[debug] connect {peer}");
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    let mut writer = stream;
                    let cmd_tx = cmd_tx.clone();
                    let proxy = proxy.clone();

                    // Peek first line to detect HTTP vs JSON
                    let mut first_line = String::new();
                    if reader.read_line(&mut first_line).is_err() {
                        continue;
                    }

                    if first_line.starts_with("GET ") {
                        // HTTP request — serve the inspector web UI
                        // Read remaining headers (discard)
                        loop {
                            let mut h = String::new();
                            if reader.read_line(&mut h).is_err() || h.trim().is_empty() {
                                break;
                            }
                        }
                        let path = first_line.split_whitespace().nth(1).unwrap_or("/");
                        let (content_type, body) = if path == "/api" || path.starts_with("/api?") {
                            // API endpoint for the inspector: extract cmd from query string
                            let query = path.split('?').nth(1).unwrap_or("");
                            let cmd_json = urldecode(query.strip_prefix("cmd=").unwrap_or("{}"));
                            let (reply_tx, reply_rx) = mpsc::channel();
                            let _ = cmd_tx.send((cmd_json, reply_tx));
                            let _ = proxy.send_event(());
                            let resp = reply_rx
                                .recv_timeout(std::time::Duration::from_secs(10))
                                .unwrap_or_else(|_| {
                                    r#"{"ok":false,"error":"timeout"}"#.to_string()
                                });
                            ("application/json", resp)
                        } else {
                            ("text/html", INSPECTOR_HTML.to_string())
                        };
                        let response = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n{}",
                            content_type,
                            body.len(),
                            body
                        );
                        let _ = writer.write_all(response.as_bytes());
                        let _ = writer.flush();
                    } else {
                        // JSON command protocol
                        let line = first_line;
                        if !line.trim().is_empty() {
                            let (reply_tx, reply_rx) = mpsc::channel();
                            let _ = cmd_tx.send((line.trim().to_string(), reply_tx));
                            let _ = proxy.send_event(());
                            if let Ok(resp) =
                                reply_rx.recv_timeout(std::time::Duration::from_secs(30))
                            {
                                if !resp.is_empty() {
                                    let _ = writeln!(writer, "{}", resp);
                                    let _ = writer.flush();
                                }
                            }
                        }
                        // Continue reading more JSON commands on same connection
                        for line in reader.lines() {
                            let line = match line {
                                Ok(l) => l,
                                Err(_) => break,
                            };
                            if line.trim().is_empty() {
                                continue;
                            }
                            let (reply_tx, reply_rx) = mpsc::channel();
                            let _ = cmd_tx.send((line, reply_tx));
                            let _ = proxy.send_event(());
                            if let Ok(resp) =
                                reply_rx.recv_timeout(std::time::Duration::from_secs(30))
                            {
                                if !resp.is_empty() {
                                    let _ = writeln!(writer, "{}", resp);
                                    let _ = writer.flush();
                                }
                            }
                        }
                    }
                    eprintln!("[debug] disconnect {peer}");
                }
                Err(e) => eprintln!("[debug] accept error: {e}"),
            }
        }
    });
}

fn find_chrome() -> Option<String> {
    let mac = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
    if std::path::Path::new(mac).exists() {
        return Some(mac.to_string());
    }
    for name in &[
        "google-chrome",
        "google-chrome-stable",
        "chromium",
        "chromium-browser",
    ] {
        if std::process::Command::new("which")
            .arg(name)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            return Some(name.to_string());
        }
    }
    None
}

/// The websocket URL of Chrome's PAGE target, from a `/json` listing.
///
/// The listing is a flat array of objects; each is scanned for `"type":"page"`
/// and its own `webSocketDebuggerUrl` taken, so a browser-UI target listed
/// first cannot capture the connection.
fn cdp_page_target(body: &str) -> Option<String> {
    let mut fallback = None;
    for chunk in body.split("{\n").chain(body.split("},")) {
        let Some(ws) = chunk
            .split("\"webSocketDebuggerUrl\": \"")
            .nth(1)
            .or_else(|| chunk.split("\"webSocketDebuggerUrl\":\"").nth(1))
            .and_then(|s| s.split('"').next())
        else {
            continue;
        };
        let is_page = chunk.contains("\"type\": \"page\"") || chunk.contains("\"type\":\"page\"");
        if is_page {
            return Some(ws.to_string());
        }
        if fallback.is_none() {
            fallback = Some(ws.to_string());
        }
    }
    fallback
}

fn cdp_send(chrome_port: u16, method: &str, params: &str) -> Result<String, String> {
    let list_url = format!("http://127.0.0.1:{}/json", chrome_port);
    let resp = reqwest::blocking::Client::new()
        .get(&list_url)
        .timeout(std::time::Duration::from_secs(2))
        .send()
        .map_err(|e| format!("CDP: {e}"))?;
    let body = resp.text().map_err(|e| e.to_string())?;
    // ⛔ Pick the PAGE target. The list also carries browser-UI targets — an
    // omnibox popup is routinely first — and taking whichever debugger URL
    // appeared first sent every command to the wrong target, which fails as
    // "No debugger URL" or, worse, silently evaluates against Chrome's own UI.
    let ws_url = cdp_page_target(&body).ok_or("No page target")?;
    // The CDP params travel into the helper as a quoted literal and are parsed
    // there, so their JSON stays JSON. Quoted the same way as everything else
    // that crosses this boundary — JSON string syntax is a subset of Python's,
    // so one quoter serves both.
    let params_py = json_quote(params);
    let script = format!(
        r#"
import socket,json,struct,random,base64
url="{ws_url}"
PARAMS={params_py}
p=url.replace("ws://","").split("/",1);hp=p[0].split(":")
s=socket.socket();s.settimeout(5);s.connect((hp[0],int(hp[1])))
path="/"+p[1] if len(p)>1 else "/"
key=base64.b64encode(random.randbytes(16)).decode()
s.sendall(f"GET {{path}} HTTP/1.1\r\nHost: {{hp[0]}}:{{hp[1]}}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {{key}}\r\nSec-WebSocket-Version: 13\r\n\r\n".encode())
r=b""
while b"\r\n\r\n" not in r: r+=s.recv(4096)
# ⛔ params is JSON, not a Python literal: interpolating it raw made
# `true`/`false`/`null` NameErrors, which broke every command that passed
# one — `returnByValue:true` among them.
msg=json.dumps({{"id":1,"method":"{method}","params":json.loads(PARAMS)}}).encode()
f=bytearray([0x81]);mk=random.randbytes(4);l=len(msg)
if l<126: f.append(0x80|l)
elif l<65536: f.append(0x80|126);f.extend(struct.pack(">H",l))
f.extend(mk);f.extend(bytearray(b^mk[i%4] for i,b in enumerate(msg)));s.sendall(bytes(f))
d=b""
while len(d)<2: d+=s.recv(4096)
pl=d[1]&0x7F;o=2
if pl==126: pl=struct.unpack(">H",d[2:4])[0];o=4
elif pl==127: pl=struct.unpack(">Q",d[2:10])[0];o=10
while len(d)<o+pl: d+=s.recv(65536)
print(d[o:o+pl].decode());s.close()
"#
    );
    let output = std::process::Command::new("python3")
        .arg("-c")
        .arg(&script)
        .output()
        .map_err(|e| format!("python3: {e}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

fn base64_decode_std(s: &str) -> Result<Vec<u8>, String> {
    const T: &[u8; 128] = b"\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\x3e\xff\xff\xff\x3f\x34\x35\x36\x37\x38\x39\x3a\x3b\x3c\x3d\xff\xff\xff\xff\xff\xff\xff\x00\x01\x02\x03\x04\x05\x06\x07\x08\x09\x0a\x0b\x0c\x0d\x0e\x0f\x10\x11\x12\x13\x14\x15\x16\x17\x18\x19\xff\xff\xff\xff\xff\xff\x1a\x1b\x1c\x1d\x1e\x1f\x20\x21\x22\x23\x24\x25\x26\x27\x28\x29\x2a\x2b\x2c\x2d\x2e\x2f\x30\x31\x32\x33\xff\xff\xff\xff\xff";
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut buf = 0u32;
    let mut bits = 0;
    for &b in s.as_bytes() {
        if b == b'=' || b == b'\n' || b == b'\r' || b >= 128 {
            continue;
        }
        let v = T[b as usize];
        if v == 0xff {
            continue;
        }
        buf = (buf << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    Ok(out)
}

fn urldecode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.bytes();
    while let Some(b) = chars.next() {
        if b == b'%' {
            let h1 = chars.next().unwrap_or(b'0');
            let h2 = chars.next().unwrap_or(b'0');
            let hex = [h1, h2];
            if let Ok(s) = std::str::from_utf8(&hex) {
                if let Ok(v) = u8::from_str_radix(s, 16) {
                    out.push(v as char);
                    continue;
                }
            }
            out.push('%');
            out.push(h1 as char);
            out.push(h2 as char);
        } else if b == b'+' {
            out.push(' ');
        } else {
            out.push(b as char);
        }
    }
    out
}

const INSPECTOR_HTML: &str = r##"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<title>webcore Inspector</title>
<style>
*{box-sizing:border-box;margin:0;padding:0}
body{font:12px/1.4 Menlo,Monaco,"Courier New",monospace;background:#1e1e1e;color:#d4d4d4;display:flex;flex-direction:column;height:100vh}
#toolbar{background:#2d2d2d;padding:6px 12px;display:flex;gap:8px;align-items:center;border-bottom:1px solid #3e3e3e}
#toolbar input{flex:1;background:#3c3c3c;border:1px solid #555;color:#eee;padding:4px 8px;border-radius:3px;font:inherit}
#toolbar button{background:#0e639c;color:#fff;border:none;padding:4px 12px;border-radius:3px;cursor:pointer;font:inherit}
#toolbar button:hover{background:#1177bb}
#main{display:flex;flex:1;overflow:hidden}
#tree-panel{width:50%;overflow:auto;padding:8px;border-right:1px solid #3e3e3e}
#detail-panel{width:50%;overflow:auto;padding:8px}
.tn{cursor:pointer;padding:1px 0;white-space:nowrap}
.tn:hover{background:#264f78}
.tn.sel{background:#094771}
.tog{display:inline-block;width:14px;text-align:center;color:#888;cursor:pointer;user-select:none}
.tog:hover{color:#fff}
.tag{color:#569cd6}.an{color:#9cdcfe}.av{color:#ce9178}.tx{color:#6a9955;font-style:italic}
.dim{color:#555;font-size:10px}
h3{color:#ccc;margin:8px 0 4px;font-size:12px;border-bottom:1px solid #3e3e3e;padding-bottom:4px}
.prop{display:flex;padding:1px 0}.pn{color:#9cdcfe;min-width:160px}.pv{color:#ce9178}
.bm{text-align:center;padding:12px}
.bm .mb{border:1px dashed #f90;padding:8px;display:inline-block;position:relative}
.bm .bb{border:1px solid #fd5;padding:8px;background:#333}
.bm .pb{border:1px dashed #6c6;padding:8px;background:#2a3a2a}
.bm .cb{background:#264f78;padding:6px;color:#fff;min-width:50px}
.bm span{font-size:10px;color:#aaa}
.lbl{position:absolute;font-size:9px;color:#f90}
.lbl.t{top:-2px;left:50%;transform:translateX(-50%)}
.lbl.b{bottom:-2px;left:50%;transform:translateX(-50%)}
.lbl.l{left:2px;top:50%;transform:translateY(-50%)}
.lbl.r{right:2px;top:50%;transform:translateY(-50%)}
#console{height:120px;border-top:1px solid #3e3e3e;display:flex;flex-direction:column}
#co{flex:1;overflow:auto;padding:4px 8px;font-size:11px;color:#999}
#ci{background:#2d2d2d;border:none;border-top:1px solid #3e3e3e;color:#eee;padding:4px 8px;font:inherit}
</style>
</head>
<body>
<div id="toolbar">
  <input id="sel" placeholder="CSS selector" value="body" onkeydown="if(event.key==='Enter')doFind()">
  <button onclick="doFind()">Find</button>
  <button onclick="doScreenshot()">Screenshot</button>
  <button onclick="loadTree()">Reload Tree</button>
</div>
<div id="main">
  <div id="tree-panel"><i>Loading DOM tree...</i></div>
  <div id="detail-panel"><i>Click an element to inspect</i></div>
</div>
<div id="console">
  <div id="co"></div>
  <input id="ci" placeholder='{"cmd":"find","selector":"h1"}' onkeydown="if(event.key==='Enter')runCmd()">
</div>
<script>
async function api(c){const r=await fetch('/api?cmd='+encodeURIComponent(JSON.stringify(c)));return r.json()}
function log(m){const e=document.getElementById('co');e.innerHTML+='<div>'+(typeof m==='string'?m:JSON.stringify(m).slice(0,200))+'</div>';e.scrollTop=e.scrollHeight}

// ── DOM tree rendering ──
function renderNode(n, depth) {
  if (n.tag==='#text') {
    if (!n.text||!n.text.trim()) return '';
    const t = n.text.trim().slice(0,50);
    return `<div class="tn" style="padding-left:${depth*16}px" onclick="inspect(${n.nid})"><span class="tx">"${esc(t)}"</span></div>`;
  }
  const has = n.children ? n.children.length > 0 : (n.count||0) > 0;
  const tog = has ? `<span class="tog" onclick="event.stopPropagation();toggle(this,${n.nid},${depth+1})">▶</span>` : '<span class="tog"> </span>';
  let attrs = '';
  if (n.id) attrs += ` <span class="an">id</span>=<span class="av">"${esc(n.id)}"</span>`;
  if (n.class) attrs += ` <span class="an">class</span>=<span class="av">"${esc(n.class)}"</span>`;
  const dim = `<span class="dim"> ${n.rect[2]}x${n.rect[3]}</span>`;
  let html = `<div class="tn" style="padding-left:${depth*16}px" data-nid="${n.nid}" onclick="inspect(${n.nid})">${tog}<span class="tag">&lt;${n.tag}</span>${attrs}<span class="tag">&gt;</span>${dim}</div>`;
  if (n.children) {
    html += `<div class="kids" data-parent="${n.nid}">`;
    for (const c of n.children) html += renderNode(c, depth+1);
    html += '</div>';
  } else if (has) {
    html += `<div class="kids" data-parent="${n.nid}" style="display:none"></div>`;
  }
  return html;
}

async function toggle(el, nid, depth) {
  const kids = el.closest('.tn').nextElementSibling;
  if (!kids) return;
  if (kids.style.display === 'none') {
    if (!kids.innerHTML) {
      kids.innerHTML = '<i style="padding-left:'+depth*16+'px;color:#666">Loading...</i>';
      // Fetch subtree rooted at this node
      const r = await api({cmd:'dom-tree', nid: nid, depth: 2});
      if (r.ok && r.tree && r.tree.children) {
        kids.innerHTML = r.tree.children.map(c => renderNode(c, depth)).join('');
      } else {
        kids.innerHTML = `<i style="padding-left:${depth*16}px;color:#666">(empty)</i>`;
      }
    }
    kids.style.display = '';
    el.textContent = '▼';
  } else {
    kids.style.display = 'none';
    el.textContent = '▶';
  }
}

async function loadTree() {
  const r = await api({cmd:'dom-tree', depth: 2});
  if (!r.ok) { document.getElementById('tree-panel').innerHTML='<i>Error</i>'; return; }
  document.getElementById('tree-panel').innerHTML = renderNode(r.tree, 0);
  log('DOM tree loaded');
}

let curNid = 0;
let curTab = 'box';
let curData = null;

async function inspect(nid) {
  document.querySelectorAll('.tn.sel').forEach(e=>e.classList.remove('sel'));
  const el = document.querySelector(`.tn[data-nid="${nid}"]`);
  if (el) el.classList.add('sel');
  curNid = nid;
  curData = await api({cmd:'inspect-node', nid: nid});
  if (!curData.ok) { document.getElementById('detail-panel').innerHTML = '<i>Not found</i>'; return; }
  renderDetail();
}

function switchTab(t) { curTab = t; renderDetail(); }

function renderDetail() {
  const r = curData;
  if (!r || !r.ok) return;
  const dp = document.getElementById('detail-panel');
  const tabs = ['box','computed','dom','layout','attrs','styles'];
  let html = `<div style="background:#252526;border-bottom:1px solid #3e3e3e;display:flex;font-size:11px">`;
  for (const t of tabs) {
    const active = t===curTab;
    html += `<div onclick="switchTab('${t}')" style="padding:5px 10px;cursor:pointer;${active?'color:#fff;border-bottom:2px solid #4fc3f7':'color:#888;border-bottom:2px solid transparent'}">${t}</div>`;
  }
  html += `</div>`;
  html += `<div style="padding:6px 10px;background:#2d2d30;border-bottom:1px solid #3e3e42;font:12px monospace">&lt;${esc(r.tag)}&gt; ${r.id?'#'+esc(r.id):''} ${r.class?'.'+esc(r.class.split(' ').join('.')):''}</div>`;
  html += `<div style="padding:8px;overflow:auto">`;

  if (curTab==='box') {
    const m=r.margin, b=r.border, p=r.padding, c=r.content;
    html += `<div class="bm"><div class="mb"><span class="lbl t">${m.top}</span><span class="lbl b">${m.bottom}</span><span class="lbl l">${m.left}</span><span class="lbl r">${m.right}</span>`;
    html += `<div class="bb"><span>border ${b.top} ${b.right} ${b.bottom} ${b.left}</span><br><div class="pb"><span>padding ${p.top} ${p.right} ${p.bottom} ${p.left}</span><br>`;
    html += `<div class="cb">${c.width} × ${c.height}</div></div></div></div></div>`;
    html += `<div style="text-align:center;color:#666;font-size:10px">position: (${c.x}, ${c.y})</div>`;
  }
  else if (curTab==='computed') {
    for (const [k,v] of Object.entries(r)) {
      if (['ok','cmd_ms','margin','border','padding','content'].includes(k)) continue;
      html += `<div class="prop"><span class="pn">${k}</span><span class="pv">${typeof v==='object'?JSON.stringify(v):v}</span></div>`;
    }
  }
  else if (curTab==='dom') {
    // Fetch tree rooted at this node
    html += `<div id="dom-sub"><i>Loading...</i></div>`;
    dp.innerHTML = html + '</div>';
    (async()=>{
      const t = await api({cmd:'dom-tree', nid: curNid, depth: 3});
      const sub = document.getElementById('dom-sub');
      if (!sub) return;
      if (t.ok && t.tree) {
        // Ancestor chain
        let chain = `<h3 style="color:#ccc;font-size:11px;border-bottom:1px solid #3e3e3e;padding-bottom:4px">Ancestors</h3>`;
        let cur = t.tree;
        // Show the node's tag path
        chain += `<div style="color:#4fc3f7;font-weight:600">&lt;${esc(t.tree.tag)}&gt; #${t.tree.id} .${t.tree.class}</div>`;
        chain += `<h3 style="color:#ccc;font-size:11px;border-bottom:1px solid #3e3e3e;padding-bottom:4px;margin-top:8px">Children (${t.tree.count||0})</h3>`;
        if (t.tree.children) {
          for (const c of t.tree.children) {
            const cid = c.id ? '#'+c.id : '';
            const ccls = c.class ? '.'+c.class.split(' ').slice(0,3).join('.') : '';
            if (c.tag==='#text' && c.text) {
              chain += `<div style="color:#6a9955;font-style:italic;padding:1px 0">"${esc(c.text.slice(0,50))}"</div>`;
            } else {
              chain += `<div style="padding:1px 0;cursor:pointer" onclick="inspect(${c.nid})"><span class="tag">&lt;${c.tag}&gt;</span>${cid}${ccls} <span style="color:#555">${c.rect[2]}x${c.rect[3]} (${c.count||0} children)</span></div>`;
            }
          }
        }
        sub.innerHTML = chain;
      } else { sub.innerHTML = '<i>Error loading</i>'; }
    })();
    return;
  }
  else if (curTab==='layout') {
    const c=r.content, m=r.margin;
    html += `<h3 style="color:#ccc;font-size:11px">Geometry</h3>`;
    html += `<div class="prop"><span class="pn">content</span><span class="pv">(${c.x}, ${c.y}) ${c.width} × ${c.height}</span></div>`;
    html += `<div class="prop"><span class="pn">display</span><span class="pv">${r.display}</span></div>`;
    html += `<div class="prop"><span class="pn">position</span><span class="pv">${r.position}</span></div>`;
    html += `<div class="prop"><span class="pn">font-size</span><span class="pv">${r.font_size}px</span></div>`;
    html += `<div class="prop"><span class="pn">node_id</span><span class="pv">${r.nid}</span></div>`;
  }
  else if (curTab==='attrs') {
    // Fetch deep info for attributes
    html += `<div id="attrs-sub"><i>Loading...</i></div>`;
    dp.innerHTML = html + '</div>';
    (async()=>{
      const d = await api({cmd:'deep', selector: '*'});
      const sub = document.getElementById('attrs-sub');
      if (!sub) return;
      // Find our node - deep returns all, we need to search
      // Just show what we have from inspect-node
      let h = '';
      h += `<div class="prop"><span class="pn">tag</span><span class="pv">${r.tag}</span></div>`;
      if (r.id) h += `<div class="prop"><span class="pn">id</span><span class="pv">${r.id}</span></div>`;
      if (r.class) h += `<div class="prop"><span class="pn">class</span><span class="pv" style="word-break:break-all">${esc(r.class)}</span></div>`;
      h += `<div class="prop"><span class="pn">color</span><span class="pv"><span style="display:inline-block;width:12px;height:12px;background:${r.color};border:1px solid #555;vertical-align:middle;margin-right:4px"></span>${r.color}</span></div>`;
      h += `<div class="prop"><span class="pn">background</span><span class="pv"><span style="display:inline-block;width:12px;height:12px;background:${r.bg};border:1px solid #555;vertical-align:middle;margin-right:4px"></span>${r.bg}</span></div>`;
      sub.innerHTML = h;
    })();
    return;
  }
  else if (curTab==='styles') {
    html += `<div style="color:#888;padding:8px">Matched CSS rules are shown in the F12 browser inspector (Styles tab).<br><br>Use the console to query: <code>{"cmd":"rules","selector":".class"}</code></div>`;
  }
  html += '</div>';
  dp.innerHTML = html;
}

function esc(s){return String(s).replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/"/g,'&quot;')}

async function doFind() {
  const s = document.getElementById('sel').value;
  const r = await api({cmd:'find',selector:s});
  log(`find "${s}": ${r.count} results`);
  if (r.ok && r.count > 0) {
    const p = document.getElementById('tree-panel');
    p.innerHTML = r.elements.map(el =>
      `<div class="tn" onclick="inspect(0)"><span class="tag">&lt;${el.tag}</span>` +
      (el.id?` <span class="an">id</span>=<span class="av">"${el.id}"</span>`:'')+
      (el.class?` <span class="an">class</span>=<span class="av">"${esc(el.class)}"</span>`:'')+
      `<span class="tag">&gt;</span> <span class="dim">${el.w}x${el.h} @${el.x},${el.y}</span></div>`
    ).join('');
  }
}
async function doScreenshot(){await api({cmd:'screenshot',out:'/tmp/inspector.png'});log('Saved /tmp/inspector.png')}
async function runCmd(){const i=document.getElementById('ci');try{log(await api(JSON.parse(i.value)))}catch(e){log('Error: '+e.message)}i.value=''}

// Auto-load tree on page open
loadTree();
</script>
</body>
</html>"##;

// ─── Entry point ──────────────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut initial_url = None;
    let mut cache_dir = Some(String::from("snapshot_cache"));
    let mut debug_port: Option<u16> = None;
    let mut headless = false;
    let mut width: f32 = 1280.0;
    let mut height: f32 = 900.0;
    let mut no_images = false;
    let mut chrome_port: u16 = 0;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--cached" => {
                cache_dir = Some(String::from("snapshot_cache"));
            }
            "--cache-dir" => {
                i += 1;
                if i < args.len() {
                    cache_dir = Some(args[i].clone());
                }
            }
            "--debug-port" | "--port" => {
                i += 1;
                if i < args.len() {
                    debug_port = args[i].parse().ok();
                }
            }
            "--headless" => {
                headless = true;
                if debug_port.is_none() {
                    debug_port = Some(9222);
                }
            }
            "--width" => {
                i += 1;
                if i < args.len() {
                    width = args[i].parse().unwrap_or(1280.0);
                }
            }
            "--height" => {
                i += 1;
                if i < args.len() {
                    height = args[i].parse().unwrap_or(900.0);
                }
            }
            "--no-images" => {
                no_images = true;
            }
            "--chrome" => {
                chrome_port = 9223;
            }
            "--chrome-port" => {
                i += 1;
                if i < args.len() {
                    chrome_port = args[i].parse().unwrap_or(9223);
                }
            }
            other => {
                if initial_url.is_none() && !other.starts_with("--") {
                    initial_url = Some(other.to_string());
                }
            }
        }
        i += 1;
    }

    if headless {
        run_headless(
            initial_url,
            debug_port.unwrap_or(9222),
            width,
            height,
            cache_dir,
            no_images,
            chrome_port,
        );
    } else {
        let event_loop = EventLoop::<()>::with_user_event().build().unwrap();
        event_loop.set_control_flow(ControlFlow::Wait);
        let proxy = event_loop.create_proxy();
        let mut app = BrowserApp::new(proxy.clone());
        app.initial_url = initial_url;
        app.cache_dir = cache_dir;
        app.no_images = no_images;
        app.initial_width = width;
        app.initial_height = height;
        if let Some(port) = debug_port {
            let (cmd_tx, cmd_rx) = mpsc::channel::<(String, mpsc::Sender<String>)>();
            app.debug_cmd_rx = Some(cmd_rx);
            browser_debug_spawn_tcp(port, cmd_tx, proxy);
        }
        event_loop.run_app(&mut app).unwrap();
    }
}

/// Headless mode: load a URL, serve debug commands on TCP, no window.
fn run_headless(
    url: Option<String>,
    port: u16,
    width: f32,
    height: f32,
    cache_dir: Option<String>,
    no_images: bool,
    chrome_port: u16,
) {
    use std::io::{BufRead, Write};

    let url = normalize_url(url.unwrap_or_else(|| "about:blank".into()));
    eprintln!("[headless] Loading {} ({}x{})", url, width, height);

    // Launch Chrome if requested
    let mut _chrome_process: Option<std::process::Child> = None;
    if chrome_port > 0 {
        if let Some(chrome_path) = find_chrome() {
            eprintln!("[headless] Launching Chrome on port {} ...", chrome_port);
            match std::process::Command::new(&chrome_path)
                .arg(format!("--remote-debugging-port={}", chrome_port))
                .arg(format!("--window-size={},{}", width as u32, height as u32))
                .arg("--disable-extensions")
                .arg("--disable-gpu")
                .arg("--disable-javascript")
                .arg("--no-first-run")
                .arg("--no-default-browser-check")
                .arg(format!(
                    "--user-data-dir=/tmp/browser-chrome-{}",
                    chrome_port
                ))
                .arg(format!("--app={}", url))
                .spawn()
            {
                Ok(child) => {
                    eprintln!("[headless] Chrome launched (pid {})", child.id());
                    _chrome_process = Some(child);
                }
                Err(e) => eprintln!("[headless] Chrome not found: {e}"),
            }
            std::thread::sleep(std::time::Duration::from_secs(2)); // wait for Chrome to start
        } else {
            eprintln!("[headless] Chrome not found");
        }
    }

    // Load document through the same browser view/widget used by the GUI path.
    let fetch_start = std::time::Instant::now();
    let mut view = webcore::BrowserView::new(
        width,
        height,
        webcore::PageLoadOptions {
            cache_dir: cache_dir.clone(),
            load_images: !no_images,
            emit_preview: true,
            ..Default::default()
        },
    );
    view.load_until_ready(url.clone(), std::time::Duration::from_secs(30));

    let load_ms = fetch_start.elapsed().as_millis();
    let (node_count, rule_count) = view
        .document()
        .map(|doc| (doc.root.child_count(), doc.stylesheet.rules.len()))
        .unwrap_or((0, 0));
    eprintln!(
        "[headless] Loaded in {}ms ({} nodes, {} rules)",
        load_ms, node_count, rule_count
    );
    eprintln!("[headless] Debug server on http://127.0.0.1:{}", port);

    // TCP listener — blocks main thread
    let listener = match std::net::TcpListener::bind(format!("127.0.0.1:{}", port)) {
        Ok(listener) => listener,
        Err(err) => {
            eprintln!("[headless] Could not bind debug server on 127.0.0.1:{port}: {err}");
            return;
        }
    };
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
        let mut writer = stream;
        let mut first_line = String::new();
        if reader.read_line(&mut first_line).is_err() {
            continue;
        }

        if first_line.starts_with("GET ") {
            // HTTP — serve inspector or API
            loop {
                let mut h = String::new();
                if reader.read_line(&mut h).is_err() || h.trim().is_empty() {
                    break;
                }
            }
            let path = first_line.split_whitespace().nth(1).unwrap_or("/");
            let (ct, body) = if path.starts_with("/api") {
                let query = path.split('?').nth(1).unwrap_or("");
                let cmd_json = urldecode(query.strip_prefix("cmd=").unwrap_or("{}"));
                let resp = dispatch_headless_cmd(
                    &mut view,
                    width,
                    height,
                    &cmd_json,
                    &cache_dir,
                    chrome_port,
                );
                ("application/json", resp)
            } else {
                ("text/html", INSPECTOR_HTML.to_string())
            };
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {ct}\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = writer.write_all(resp.as_bytes());
        } else {
            // JSON protocol
            let line = first_line.trim().to_string();
            if !line.is_empty() {
                let resp =
                    dispatch_headless_cmd(&mut view, width, height, &line, &cache_dir, chrome_port);
                let _ = writeln!(writer, "{}", resp);
                let _ = writer.flush();
            }
            for line in reader.lines() {
                let Ok(line) = line else { break };
                if line.trim().is_empty() {
                    continue;
                }
                let resp =
                    dispatch_headless_cmd(&mut view, width, height, &line, &cache_dir, chrome_port);
                let _ = writeln!(writer, "{}", resp);
                let _ = writer.flush();
            }
        }
    }
}

/// Quote a string as a JSON scalar.
fn json_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Format a layout coordinate for the debug protocol: the exact value, with
/// the trailing `.0` dropped so whole pixels stay readable.
fn fmt_px(v: f32) -> String {
    if (v - v.round()).abs() < 0.001 {
        format!("{}", v.round() as i64)
    } else {
        format!("{:.3}", v)
    }
}

fn animated_images_json(doc: &Document, viewport_h: f32) -> String {
    fn hidden_by_style(node: &webcore::WebCore) -> bool {
        matches!(node.style.display, Display::None)
            || !node.style.visibility
            || node.style.opacity <= 0.0
    }

    fn interval(node: &webcore::WebCore, scroll_y: f32) -> (f32, f32) {
        let r = node.layout.margin_rect;
        if matches!(node.style.position, Position::Fixed) {
            (scroll_y + r.y, scroll_y + r.y + r.h)
        } else {
            (r.y, r.y + r.h)
        }
    }

    fn child_clip(
        node: &webcore::WebCore,
        scroll_y: f32,
        clip_top: f32,
        clip_bottom: f32,
    ) -> (f32, f32) {
        if matches!(
            node.style.overflow_y,
            Overflow::Hidden | Overflow::Clip | Overflow::Scroll | Overflow::Auto
        ) {
            let r = node.layout.content_rect;
            if r.h <= 0.0 {
                return (1.0, 0.0);
            }
            let (top, bottom) = if matches!(node.style.position, Position::Fixed) {
                (scroll_y + r.y, scroll_y + r.y + r.h)
            } else {
                (r.y, r.y + r.h)
            };
            (clip_top.max(top), clip_bottom.min(bottom))
        } else {
            (clip_top, clip_bottom)
        }
    }

    fn walk(
        node: &webcore::WebCore,
        scroll_y: f32,
        viewport_h: f32,
        inherited_visible: bool,
        clip_top: f32,
        clip_bottom: f32,
        out: &mut Vec<String>,
    ) {
        let visible_branch = inherited_visible && !hidden_by_style(node);
        let (top, bottom) = interval(node, scroll_y);
        let r = node.layout.margin_rect;
        if let Some(animated) = node.animated_image.as_ref() {
            let visible = visible_branch
                && r.w > 0.0
                && r.h > 0.0
                && bottom >= scroll_y
                && top <= scroll_y + viewport_h
                && bottom >= clip_top
                && top <= clip_bottom;
            let src = node
                .attributes
                .get("src")
                .or_else(|| node.attributes.get("data-src"))
                .map(|s| s.as_str())
                .unwrap_or("");
            let class = node
                .attributes
                .get("class")
                .map(|s| s.as_str())
                .unwrap_or("");
            out.push(format!(
                r#"{{"nid":{},"tag":{},"class":{},"src":{},"visible":{},"frames":{},"frame":{},"x":{},"y":{},"w":{},"h":{},"clip_top":{},"clip_bottom":{}}}"#,
                node.node_id,
                dbg_json_escape(&node.tag),
                dbg_json_escape(class),
                dbg_json_escape(src),
                visible,
                animated.frames.len(),
                node.animated_image_frame,
                fmt_px(r.x),
                fmt_px(r.y),
                fmt_px(r.w),
                fmt_px(r.h),
                fmt_px(clip_top),
                fmt_px(clip_bottom)
            ));
        }
        if !visible_branch {
            return;
        }
        let (child_clip_top, child_clip_bottom) = child_clip(node, scroll_y, clip_top, clip_bottom);
        if child_clip_bottom < child_clip_top {
            return;
        }
        for child in &node.children {
            walk(
                child,
                scroll_y,
                viewport_h,
                visible_branch,
                child_clip_top,
                child_clip_bottom,
                out,
            );
        }
    }

    let mut items = Vec::new();
    walk(
        &doc.root,
        doc.scroll_y,
        viewport_h,
        true,
        doc.scroll_y,
        doc.scroll_y + viewport_h,
        &mut items,
    );
    let visible = items
        .iter()
        .filter(|item| item.contains(r#""visible":true"#))
        .count();
    format!(
        r#"{{"ok":true,"count":{},"visible":{},"items":[{}]}}"#,
        items.len(),
        visible,
        items.join(",")
    )
}

fn image_states_json(doc: &Document, limit: usize) -> String {
    fn walk(node: &webcore::WebCore, out: &mut Vec<String>, limit: usize) {
        if out.len() >= limit {
            return;
        }
        let has_element_image = node.image_data.is_some();
        let has_background = node.bg_image_data.is_some();
        let has_mask = node.mask_image_data.is_some();
        let is_image_like = node.is_image_element()
            || node.tag == "video"
            || has_element_image
            || has_background
            || has_mask
            || !node.style.background_image_url.is_empty()
            || !node.style.rare().mask_image_url.is_empty();
        if is_image_like {
            let r = node.layout.border_rect;
            let src = if !node.resolved_src.is_empty() {
                node.resolved_src.as_str()
            } else {
                node.attributes
                    .get("src")
                    .or_else(|| node.attributes.get("poster"))
                    .or_else(|| node.attributes.get("data-src"))
                    .map(|s| s.as_str())
                    .unwrap_or("")
            };
            out.push(format!(
                concat!(
                    r#"{{"nid":{},"tag":{},"class":{},"src":{},"srcset":{},"#,
                    r#""element_loaded":{},"element_size":[{},{}],"element_bytes":{},"#,
                    r#""background_url":{},"background_loaded":{},"background_size":[{},{}],"#,
                    r#""mask_url":{},"mask_loaded":{},"mask_size":[{},{}],"#,
                    r#""rect":[{},{},{},{}]}}"#
                ),
                node.node_id,
                dbg_json_escape(&node.tag),
                dbg_json_escape(
                    node.attributes
                        .get("class")
                        .map(String::as_str)
                        .unwrap_or("")
                ),
                dbg_json_escape(src),
                dbg_json_escape(
                    node.attributes
                        .get("srcset")
                        .map(String::as_str)
                        .unwrap_or("")
                ),
                has_element_image,
                node.image_width,
                node.image_height,
                node.image_data.as_ref().map(|d| d.len()).unwrap_or(0),
                dbg_json_escape(&node.style.background_image_url),
                has_background,
                node.bg_image_width,
                node.bg_image_height,
                dbg_json_escape(&node.style.rare().mask_image_url),
                has_mask,
                node.mask_image_width,
                node.mask_image_height,
                fmt_px(r.x),
                fmt_px(r.y),
                fmt_px(r.w),
                fmt_px(r.h)
            ));
        }
        for child in &node.children {
            walk(child, out, limit);
            if out.len() >= limit {
                break;
            }
        }
    }

    let mut items = Vec::new();
    walk(&doc.root, &mut items, limit);
    let errors: Vec<String> = doc
        .image_load_errors
        .iter()
        .rev()
        .take(limit)
        .map(|(path, target, url, error)| {
            format!(
                r#"{{"path":{},"target":"{:?}","url":{},"error":{}}}"#,
                dbg_json_escape(&format!("{path:?}")),
                target,
                dbg_json_escape(url),
                dbg_json_escape(error)
            )
        })
        .collect();
    format!(
        r#"{{"ok":true,"count":{},"errors":{},"pending_channel":{},"in_flight":{},"images":[{}],"load_errors":[{}]}}"#,
        items.len(),
        doc.image_load_errors.len(),
        doc.pending_images.is_some(),
        doc.images_in_flight
            .load(std::sync::atomic::Ordering::SeqCst),
        items.join(","),
        errors.join(",")
    )
}

fn animations_json(doc: &Document, viewport_h: f32) -> String {
    let mut items = Vec::new();
    for state in &doc.active_animations {
        let props = doc
            .stylesheet
            .keyframes
            .get(&state.animation.name)
            .map(|stops| {
                let mut props = Vec::<String>::new();
                for stop in stops {
                    for (prop, _) in &stop.properties {
                        if !props.iter().any(|p| p == prop) {
                            props.push(prop.clone());
                        }
                    }
                }
                props
            })
            .unwrap_or_default();
        let node = doc.get_box_by_id(state.element_id);
        let tag = node.map(|n| n.tag.as_str()).unwrap_or("");
        let rect = node.map(|n| n.layout.margin_rect).unwrap_or_default();
        let current: Vec<String> = doc
            .animation_overrides_for(state.element_id)
            .map(|props| {
                props
                    .iter()
                    .map(|(name, value)| {
                        format!("{}:{}", dbg_json_escape(name), dbg_json_escape(value))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let visible = node.is_some_and(|n| {
            !matches!(n.style.display, Display::None)
                && n.style.visibility
                && n.style.opacity > 0.0
                && rect.w > 0.0
                && rect.h > 0.0
                && rect.y + rect.h >= doc.scroll_y
                && rect.y <= doc.scroll_y + viewport_h
        });
        items.push(format!(
            r#"{{"nid":{},"tag":{},"name":{},"visible":{},"x":{},"y":{},"w":{},"h":{},"duration_ms":{},"delay_ms":{},"iterations":{},"paused":{},"properties":[{}],"current":{{{}}}}}"#,
            state.element_id,
            dbg_json_escape(tag),
            dbg_json_escape(&state.animation.name),
            visible,
            fmt_px(rect.x),
            fmt_px(rect.y),
            fmt_px(rect.w),
            fmt_px(rect.h),
            state.animation.duration_ms,
            state.animation.delay_ms,
            state.animation.iteration_count,
            state.animation.play_state_paused,
            props
                .iter()
                .map(|p| dbg_json_escape(p))
                .collect::<Vec<_>>()
                .join(","),
            current.join(",")
        ));
    }
    format!(
        r#"{{"ok":true,"needs_frame":{},"active":{},"items":[{}]}}"#,
        doc.needs_animation_frame,
        doc.active_animations.len(),
        items.join(",")
    )
}

fn display_list_stats_json(
    list: &webcore::renderer::display_list::DisplayList,
    paint_band: Option<(f32, f32)>,
) -> String {
    let mut fills = 0usize;
    let mut borders = 0usize;
    let mut text = 0usize;
    let mut images = 0usize;
    let mut clips = 0usize;
    let mut clip_paths = 0usize;
    let mut transforms = 0usize;
    let mut layers = 0usize;
    let mut masks = 0usize;
    for cmd in list.commands.iter().chain(list.fixed_commands.iter()) {
        match cmd {
            PaintCmd::FillRect { .. } => fills += 1,
            PaintCmd::Border { .. } | PaintCmd::BorderImage { .. } => borders += 1,
            PaintCmd::Text { .. } => text += 1,
            PaintCmd::Image { .. }
            | PaintCmd::BackgroundImage { .. }
            | PaintCmd::ListMarker { image: Some(_), .. } => images += 1,
            PaintCmd::PushClip { .. } => clips += 1,
            PaintCmd::PushClipPath { .. } => clip_paths += 1,
            PaintCmd::PushTransform { .. } => transforms += 1,
            PaintCmd::PushOpacity { .. }
            | PaintCmd::PushFilter { .. }
            | PaintCmd::PushBlendMode { .. } => layers += 1,
            PaintCmd::PushMask { .. } => masks += 1,
            _ => {}
        }
    }
    let band_json = paint_band
        .map(|(top, bottom)| format!(r#","paint_top":{top:.1},"paint_bottom":{bottom:.1}"#))
        .unwrap_or_default();
    format!(
        r#"{{"ok":true,"commands":{},"fixed_commands":{},"fills":{},"borders":{},"text":{},"images":{},"clips":{},"clip_paths":{},"transforms":{},"layers":{},"masks":{}{}}}"#,
        list.commands.len(),
        list.fixed_commands.len(),
        fills,
        borders,
        text,
        images,
        clips,
        clip_paths,
        transforms,
        layers,
        masks,
        band_json
    )
}

#[derive(Clone, Copy, Debug)]
struct ProcessMemoryStats {
    rss_bytes: u64,
    vsz_bytes: u64,
}

fn process_memory_stats() -> Option<ProcessMemoryStats> {
    let pid = std::process::id().to_string();
    let output = std::process::Command::new("ps")
        .args(["-o", "rss=,vsz=", "-p", &pid])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let mut parts = text.split_whitespace();
    let rss_kib = parts.next()?.parse::<u64>().ok()?;
    let vsz_kib = parts.next()?.parse::<u64>().ok()?;
    Some(ProcessMemoryStats {
        rss_bytes: rss_kib.saturating_mul(1024),
        vsz_bytes: vsz_kib.saturating_mul(1024),
    })
}

fn append_cmd_ms(result: String, started: std::time::Instant) -> String {
    let ms = started.elapsed().as_micros() as f64 / 1000.0;
    if result.ends_with('}') {
        format!(
            "{}{}\"cmd_ms\":{:.2}}}",
            &result[..result.len() - 1],
            if result.len() > 2 { "," } else { "" },
            ms
        )
    } else {
        result
    }
}

fn dispatch_headless_view_cmd(
    view: &mut webcore::BrowserView,
    cmd: &str,
    line: &str,
    width: f32,
    height: f32,
) -> String {
    match cmd {
        "screenshot" => {
            let path = dbg_json_str(line, "out").unwrap_or_else(|| "snapshot.png".to_string());
            let scale = dbg_json_num(line, "scale")
                .map(|v| v.clamp(0.25, 4.0) as f32)
                .unwrap_or(1.0);
            let phys_w = (width * scale).ceil().max(1.0) as u32;
            let phys_h = (height * scale).ceil().max(1.0) as u32;
            let Some(mut pm) = tiny_skia::Pixmap::new(phys_w, phys_h) else {
                return r#"{"ok":false,"error":"pixmap failed"}"#.to_string();
            };
            pm.fill(tiny_skia::Color::WHITE);
            view.resize(width, height);
            view.paint_into(&mut pm, 0, 0, scale);
            match pm.save_png(&path) {
                Ok(_) => format!(
                    r#"{{"ok":true,"path":"{}","width":{},"height":{},"scale":{}}}"#,
                    path, phys_w, phys_h, scale
                ),
                Err(e) => format!(r#"{{"ok":false,"error":"{}"}}"#, e),
            }
        }
        "scroll" => {
            if let Some(y) = dbg_json_num(line, "y") {
                view.scroll_to(0.0, y as f32);
            } else {
                let dy = dbg_json_num(line, "dy").unwrap_or(0.0) as f32;
                view.scroll_by(0.0, dy);
            }
            format!(r#"{{"ok":true,"scroll_y":{:.0}}}"#, view.scroll_y())
        }
        "click" => {
            let coords = if let Some(sel) = dbg_json_str(line, "selector") {
                view.document()
                    .and_then(|doc| dbg_selector_center(doc, &sel))
                    .ok_or_else(|| {
                        format!(
                            r#"{{"ok":false,"error":"no element matches {}"}}"#,
                            dbg_json_escape(&sel)
                        )
                    })
            } else if let (Some(x), Some(y)) = (dbg_json_num(line, "x"), dbg_json_num(line, "y")) {
                Ok((x, y))
            } else {
                return r#"{"ok":false,"error":"click needs x,y or selector"}"#.to_string();
            };
            let (x, y) = match coords {
                Ok(c) => c,
                Err(e) => return e,
            };
            view.handle_mouse_button(webcore::dom::HtmlEventType::MouseDown, x, y, 0);
            view.handle_mouse_button(webcore::dom::HtmlEventType::MouseUp, x, y, 0);
            format!(r#"{{"ok":true,"x":{:.0},"y":{:.0}}}"#, x, y)
        }
        "hover" => {
            let coords = if let Some(sel) = dbg_json_str(line, "selector") {
                view.document()
                    .and_then(|doc| dbg_selector_center(doc, &sel))
                    .ok_or_else(|| {
                        format!(
                            r#"{{"ok":false,"error":"no element matches {}"}}"#,
                            dbg_json_escape(&sel)
                        )
                    })
            } else if let (Some(x), Some(y)) = (dbg_json_num(line, "x"), dbg_json_num(line, "y")) {
                Ok((x, y))
            } else {
                return r#"{"ok":false,"error":"hover needs x,y or selector"}"#.to_string();
            };
            let (x, y) = match coords {
                Ok(c) => c,
                Err(e) => return e,
            };
            let changed = view.handle_mouse_move(x, y);
            format!(r#"{{"ok":true,"changed":{}}}"#, changed)
        }
        "type" => match dbg_json_str(line, "text") {
            Some(text) => {
                let mut any = false;
                for ch in text.chars() {
                    any |= view.handle_key(
                        webcore::dom::HtmlEventType::KeyDown,
                        ch as u32,
                        Some(ch),
                        false,
                        false,
                        false,
                        false,
                    );
                }
                format!(r#"{{"ok":true,"typed":{}}}"#, any)
            }
            None => r#"{"ok":false,"error":"type needs text"}"#.to_string(),
        },
        "key" => match dbg_json_str(line, "key") {
            Some(k) => {
                let (code, ch) = match k.as_str() {
                    "Enter" => (13, Some('\r')),
                    "Tab" => (9, Some('\t')),
                    "Backspace" => (8, None),
                    "Delete" => (46, None),
                    "Escape" => (27, None),
                    "ArrowLeft" => (37, None),
                    "ArrowRight" => (39, None),
                    "ArrowUp" => (38, None),
                    "ArrowDown" => (40, None),
                    "Home" => (36, None),
                    "End" => (35, None),
                    "Space" => (32, Some(' ')),
                    s if s.len() == 1 => (s.chars().next().unwrap() as u32, s.chars().next()),
                    _ => {
                        return format!(
                            r#"{{"ok":false,"error":"unknown key: {}"}}"#,
                            dbg_json_escape(&k)
                        );
                    }
                };
                let changed = view.handle_key(
                    webcore::dom::HtmlEventType::KeyDown,
                    code,
                    ch,
                    false,
                    false,
                    false,
                    false,
                );
                format!(r#"{{"ok":true,"changed":{}}}"#, changed)
            }
            None => r#"{"ok":false,"error":"key needs key name"}"#.to_string(),
        },
        "inspect-mode" => {
            let on = dbg_json_str(line, "on")
                .map(|v| !matches!(v.as_str(), "false" | "0"))
                .or_else(|| dbg_json_num(line, "on").map(|v| v != 0.0))
                .unwrap_or(true);
            view.set_inspect_mode(on);
            format!(r#"{{"ok":true,"inspect_mode":{}}}"#, on)
        }
        "resize" => {
            view.resize(width, height);
            r#"{"ok":true}"#.to_string()
        }
        "bench-progressive" => match view.benchmark_progressive_layout() {
            Some((full, above)) => format!(
                r#"{{"ok":true,"full_ms":{:.1},"above_fold_ms":{:.1}}}"#,
                full, above
            ),
            None => r#"{"ok":false,"error":"no document"}"#.to_string(),
        },
        "bench-render" => {
            let scale = dbg_json_num(line, "scale")
                .map(|v| v.clamp(0.25, 4.0) as f32)
                .unwrap_or(1.0);
            let dy = dbg_json_num(line, "dy").unwrap_or(500.0) as f32;
            let phys_w = (width * scale).ceil().max(1.0) as u32;
            let phys_h = (height * scale).ceil().max(1.0) as u32;
            let Some(mut pm) = tiny_skia::Pixmap::new(phys_w, phys_h) else {
                return r#"{"ok":false,"error":"pixmap failed"}"#.to_string();
            };
            let original_y = view.scroll_y();
            let t0 = std::time::Instant::now();
            view.paint_into(&mut pm, 0, 0, scale);
            let first = t0.elapsed().as_micros() as f64 / 1000.0;
            view.scroll_by(0.0, dy);
            let t1 = std::time::Instant::now();
            view.paint_into(&mut pm, 0, 0, scale);
            let scrolled = t1.elapsed().as_micros() as f64 / 1000.0;
            let t2 = std::time::Instant::now();
            view.paint_into(&mut pm, 0, 0, scale);
            let cached = t2.elapsed().as_micros() as f64 / 1000.0;
            view.scroll_to(0.0, original_y);
            format!(
                r#"{{"ok":true,"first_ms":{:.1},"scroll_ms":{:.1},"cached_ms":{:.1},"width":{},"height":{},"scale":{}}}"#,
                first, scrolled, cached, phys_w, phys_h, scale
            )
        }
        _ => r#"{"ok":false,"error":"unsupported browser command"}"#.to_string(),
    }
}

fn dispatch_headless_cmd(
    view: &mut webcore::BrowserView,
    width: f32,
    height: f32,
    line: &str,
    _cache_dir: &Option<String>,
    chrome_port: u16,
) -> String {
    let cmd_start = std::time::Instant::now();
    let cmd = dbg_json_str(line, "cmd").unwrap_or_default();
    if cmd == "navigate" {
        let result = if let Some(new_url) = dbg_json_str(line, "url") {
            let new_url = normalize_url(new_url);
            view.load_until_ready(new_url.clone(), std::time::Duration::from_secs(30));
            format!(r#"{{"ok":true,"url":"{}"}}"#, view.url())
        } else {
            r#"{"ok":false,"error":"need url"}"#.to_string()
        };
        return append_cmd_ms(result, cmd_start);
    }
    if matches!(
        cmd.as_str(),
        "screenshot"
            | "scroll"
            | "click"
            | "hover"
            | "type"
            | "key"
            | "inspect-mode"
            | "resize"
            | "bench-progressive"
            | "bench-render"
    ) {
        return append_cmd_ms(
            dispatch_headless_view_cmd(view, &cmd, line, width, height),
            cmd_start,
        );
    }

    let Some((doc, renderer)) = view.document_and_renderer_mut() else {
        return r#"{"ok":false,"error":"no document"}"#.to_string();
    };
    let result = match cmd.as_str() {
        "step" | "tick" => {
            let ms = dbg_json_num(line, "ms").unwrap_or(100.0);
            if ms > 0.0 {
                std::thread::sleep(std::time::Duration::from_millis(ms as u64));
            }
            renderer.layout_engine().viewport_h = height;
            renderer.layout_engine().layout(doc, width);
            renderer.invalidate_display_list();
            format!(r#"{{"ok":true,"ms":{}}}"#, ms)
        }
        "find" => {
            let sel = dbg_json_str(line, "selector").unwrap_or_default();
            let nodes = webcore::dom::query_selector_all(&doc.root, &sel);
            let items: Vec<String> = nodes
                .iter()
                .map(|n| {
                    let b = n.layout.border_rect;
                    format!(
                        r#"{{"node_id":{},"tag":"{}","id":"{}","class":"{}","x":{},"y":{},"w":{},"h":{}}}"#,
                        n.node_id,
                        n.tag,
                        n.attributes.get("id").unwrap_or(&String::new()),
                        n.attributes.get("class").unwrap_or(&String::new()),
                        // ⛔ Not `as i32`. Truncating the rect threw away the
                        // sub-pixel part, so a box at 38.7 reported 38 and every
                        // comparison against a browser — which rounds — was off by
                        // one for no reason. Layout works in fractional pixels;
                        // an inspector that hides them cannot be used to diff.
                        fmt_px(b.x),
                        fmt_px(b.y),
                        fmt_px(b.w),
                        fmt_px(b.h)
                    )
                })
                .collect();
            format!(
                r#"{{"ok":true,"count":{},"elements":[{}]}}"#,
                items.len(),
                items.join(",")
            )
        }
        "height-dump" => {
            let limit = dbg_json_num(line, "limit").unwrap_or(40.0).max(1.0) as usize;
            dbg_height_dump_json(&doc.root, limit)
        }
        "display-list-stats" => {
            let overscan = (height * 1.5).max(1200.0);
            let paint_top = (doc.scroll_y - overscan).max(0.0);
            let paint_bottom = doc.scroll_y + height + overscan;
            let list = build_display_list_viewport(
                &doc.root,
                width,
                height,
                doc.scroll_x,
                doc.scroll_y,
                paint_top,
                paint_bottom,
                0,
                0,
                &std::collections::HashSet::new(),
                &doc.base_url,
            );
            display_list_stats_json(&list, Some((paint_top, paint_bottom)))
        }
        "animated-images" => animated_images_json(doc, height),
        "image-states" => {
            let limit = dbg_json_num(line, "limit").unwrap_or(80.0).max(1.0) as usize;
            image_states_json(doc, limit)
        }
        "animations" => animations_json(doc, height),
        "paint-dump" => {
            let x = dbg_json_num(line, "x").unwrap_or(0.0);
            let y = dbg_json_num(line, "y").unwrap_or(0.0);
            let w = dbg_json_num(line, "w").unwrap_or(width);
            let h = dbg_json_num(line, "h").unwrap_or(height);
            let limit = dbg_json_num(line, "limit").unwrap_or(80.0).max(1.0) as usize;
            let qx2 = x + w.max(0.0);
            let qy2 = y + h.max(0.0);
            let font_system = Some(&mut renderer.font_system as *mut _);
            let list = build_display_list_full_with_font_system(
                &doc.root,
                width,
                height,
                doc.scroll_x,
                doc.scroll_y,
                0,
                0,
                &std::collections::HashSet::new(),
                &doc.base_url,
                font_system,
            );
            let mut out = Vec::new();
            for cmd in &list.commands {
                if out.len() >= limit {
                    break;
                }
                match cmd {
                    PaintCmd::FillRect {
                        rect,
                        color,
                        radius,
                        radius_y,
                    } => {
                        if rect.x <= qx2
                            && rect.right() >= x
                            && rect.y <= qy2
                            && rect.bottom() >= y
                            && color.a > 0
                        {
                            out.push(format!(
                                r##"{{"kind":"fill","x":{:.1},"y":{:.1},"w":{:.1},"h":{:.1},"color":"#{:02x}{:02x}{:02x}{:02x}","radius":[{:.1},{:.1},{:.1},{:.1}],"radius_y":[{:.1},{:.1},{:.1},{:.1}]}}"##,
                                rect.x, rect.y, rect.w, rect.h,
                                color.r, color.g, color.b, color.a,
                                radius[0], radius[1], radius[2], radius[3],
                                radius_y[0], radius_y[1], radius_y[2], radius_y[3]
                            ));
                        }
                    }
                    PaintCmd::Border {
                        rect,
                        widths,
                        colors,
                        styles,
                        ..
                    } => {
                        if rect.x <= qx2
                            && rect.right() >= x
                            && rect.y <= qy2
                            && rect.bottom() >= y
                            && widths.iter().any(|w| *w > 0.0)
                        {
                            out.push(format!(
                                r##"{{"kind":"border","x":{:.1},"y":{:.1},"w":{:.1},"h":{:.1},"widths":[{:.1},{:.1},{:.1},{:.1}],"colors":["#{:02x}{:02x}{:02x}{:02x}","#{:02x}{:02x}{:02x}{:02x}","#{:02x}{:02x}{:02x}{:02x}","#{:02x}{:02x}{:02x}{:02x}"],"styles":[{},{},{},{}]}}"##,
                                rect.x, rect.y, rect.w, rect.h,
                                widths[0], widths[1], widths[2], widths[3],
                                colors[0].r, colors[0].g, colors[0].b, colors[0].a,
                                colors[1].r, colors[1].g, colors[1].b, colors[1].a,
                                colors[2].r, colors[2].g, colors[2].b, colors[2].a,
                                colors[3].r, colors[3].g, colors[3].b, colors[3].a,
                                styles[0], styles[1], styles[2], styles[3]
                            ));
                        }
                    }
                    PaintCmd::Outline {
                        rect,
                        width,
                        color,
                        style,
                        offset,
                        ..
                    } => {
                        if rect.x <= qx2
                            && rect.right() >= x
                            && rect.y <= qy2
                            && rect.bottom() >= y
                            && *width > 0.0
                            && color.a > 0
                        {
                            out.push(format!(
                                r##"{{"kind":"outline","x":{:.1},"y":{:.1},"w":{:.1},"h":{:.1},"width":{:.1},"color":"#{:02x}{:02x}{:02x}{:02x}","style":{},"offset":{:.1}}}"##,
                                rect.x, rect.y, rect.w, rect.h,
                                width,
                                color.r, color.g, color.b, color.a,
                                style,
                                offset
                            ));
                        }
                    }
                    PaintCmd::Text {
                        x: tx,
                        y: ty,
                        text,
                        font_family,
                        font_size,
                        font_weight,
                        decoration,
                        ..
                    } => {
                        if *tx <= qx2 && *tx + 1200.0 >= x && *ty <= qy2 && *ty + *font_size >= y {
                            out.push(format!(
                                r#"{{"kind":"text","x":{:.1},"y":{:.1},"font":{},"size":{:.1},"weight":{},"underline":{},"overline":{},"strikethrough":{},"text":{}}}"#,
                                tx,
                                ty,
                                dbg_json_escape(font_family),
                                font_size,
                                font_weight,
                                decoration.underline,
                                decoration.overline,
                                decoration.strikethrough,
                                dbg_json_escape(&text.chars().take(120).collect::<String>())
                            ));
                        }
                    }
                    PaintCmd::TextShadow {
                        x: tx,
                        y: ty,
                        text,
                        font_family,
                        font_size,
                        ..
                    } => {
                        if *tx <= qx2 && *tx + 1200.0 >= x && *ty <= qy2 && *ty + *font_size >= y {
                            out.push(format!(
                                r#"{{"kind":"text-shadow","x":{:.1},"y":{:.1},"font":{},"size":{:.1},"text":{}}}"#,
                                tx,
                                ty,
                                dbg_json_escape(font_family),
                                font_size,
                                dbg_json_escape(&text.chars().take(120).collect::<String>())
                            ));
                        }
                    }
                    PaintCmd::Image { rect, .. } => {
                        if rect.x <= qx2 && rect.right() >= x && rect.y <= qy2 && rect.bottom() >= y
                        {
                            out.push(format!(
                                r#"{{"kind":"image","x":{:.1},"y":{:.1},"w":{:.1},"h":{:.1}}}"#,
                                rect.x, rect.y, rect.w, rect.h
                            ));
                        }
                    }
                    PaintCmd::BackgroundImage {
                        container,
                        clip,
                        draw_w,
                        draw_h,
                        pos_x,
                        pos_y,
                        ..
                    } => {
                        if clip.x <= qx2 && clip.right() >= x && clip.y <= qy2 && clip.bottom() >= y
                        {
                            out.push(format!(
                                r#"{{"kind":"background-image","container":[{:.1},{:.1},{:.1},{:.1}],"clip":[{:.1},{:.1},{:.1},{:.1}],"draw_w":{:.1},"draw_h":{:.1},"pos_x":{:.1},"pos_y":{:.1}}}"#,
                                container.x, container.y, container.w, container.h,
                                clip.x, clip.y, clip.w, clip.h,
                                draw_w, draw_h, pos_x, pos_y
                            ));
                        }
                    }
                    PaintCmd::Gradient {
                        rect,
                        clip,
                        gradient_type,
                        angle,
                        stops,
                        ..
                    } => {
                        if clip.x <= qx2 && clip.right() >= x && clip.y <= qy2 && clip.bottom() >= y
                        {
                            out.push(format!(
                                r#"{{"kind":"gradient","x":{:.1},"y":{:.1},"w":{:.1},"h":{:.1},"clip":[{:.1},{:.1},{:.1},{:.1}],"gradient_type":{},"angle":{:.1},"stops":{}}}"#,
                                rect.x, rect.y, rect.w, rect.h,
                                clip.x, clip.y, clip.w, clip.h,
                                gradient_type, angle, stops.len()
                            ));
                        }
                    }
                    PaintCmd::BoxShadow {
                        rect,
                        color,
                        offset_x,
                        offset_y,
                        blur,
                        spread,
                        inset,
                        ..
                    } => {
                        let sx = rect.x + offset_x - spread - blur * 4.0;
                        let sy = rect.y + offset_y - spread - blur * 4.0;
                        let sw = rect.w + spread * 2.0 + blur * 8.0;
                        let sh = rect.h + spread * 2.0 + blur * 8.0;
                        if sx <= qx2 && sx + sw >= x && sy <= qy2 && sy + sh >= y {
                            out.push(format!(
                                r##"{{"kind":"box-shadow","x":{:.1},"y":{:.1},"w":{:.1},"h":{:.1},"color":"#{:02x}{:02x}{:02x}{:02x}","offset_x":{:.1},"offset_y":{:.1},"blur":{:.1},"spread":{:.1},"inset":{}}}"##,
                                rect.x, rect.y, rect.w, rect.h,
                                color.r, color.g, color.b, color.a,
                                offset_x, offset_y, blur, spread, inset
                            ));
                        }
                    }
                    PaintCmd::BeginStackingContext { node_id, z_index } => {
                        out.push(format!(
                            r#"{{"kind":"begin-stacking-context","node_id":{},"z_index":{}}}"#,
                            node_id, z_index
                        ));
                    }
                    PaintCmd::EndStackingContext => {
                        out.push(r#"{"kind":"end-stacking-context"}"#.to_string());
                    }
                    PaintCmd::PushClip {
                        rect,
                        radius,
                        radius_y,
                    } => {
                        if rect.x <= qx2 && rect.right() >= x && rect.y <= qy2 && rect.bottom() >= y
                        {
                            out.push(format!(
                                r#"{{"kind":"push-clip","x":{:.1},"y":{:.1},"w":{:.1},"h":{:.1},"radius":[{:.1},{:.1},{:.1},{:.1}],"radius_y":[{:.1},{:.1},{:.1},{:.1}]}}"#,
                                rect.x, rect.y, rect.w, rect.h,
                                radius[0], radius[1], radius[2], radius[3],
                                radius_y[0], radius_y[1], radius_y[2], radius_y[3]
                            ));
                        }
                    }
                    PaintCmd::PushClipPath { points } => {
                        out.push(format!(
                            r#"{{"kind":"push-clip-path","points":{}}}"#,
                            points.len()
                        ));
                    }
                    PaintCmd::PushTransform { node_id, transform } => {
                        out.push(format!(
                            r#"{{"kind":"push-transform","node_id":{},"matrix":[{:.4},{:.4},{:.4},{:.4},{:.1},{:.1}]}}"#,
                            node_id,
                            transform[0], transform[1], transform[2], transform[3],
                            transform[4], transform[5]
                        ));
                    }
                    PaintCmd::PopTransform => {
                        out.push(r#"{"kind":"pop-transform"}"#.to_string());
                    }
                    PaintCmd::PushOpacity { alpha } => {
                        out.push(format!(r#"{{"kind":"push-opacity","alpha":{:.3}}}"#, alpha));
                    }
                    PaintCmd::PopOpacity => {
                        out.push(r#"{"kind":"pop-opacity"}"#.to_string());
                    }
                    PaintCmd::PushFilter { filters } => {
                        out.push(format!(
                            r#"{{"kind":"push-filter","filters":{}}}"#,
                            filters.len()
                        ));
                    }
                    PaintCmd::PopFilter => {
                        out.push(r#"{"kind":"pop-filter"}"#.to_string());
                    }
                    PaintCmd::BackdropFilter { rect, filters } => {
                        if rect.x <= qx2 && rect.right() >= x && rect.y <= qy2 && rect.bottom() >= y
                        {
                            out.push(format!(
                                r#"{{"kind":"backdrop-filter","x":{:.1},"y":{:.1},"w":{:.1},"h":{:.1},"filters":{}}}"#,
                                rect.x, rect.y, rect.w, rect.h, filters.len()
                            ));
                        }
                    }
                    PaintCmd::PushMask { rect, data } => {
                        if rect.x <= qx2 && rect.right() >= x && rect.y <= qy2 && rect.bottom() >= y
                        {
                            let (mw, mh) = match data {
                                ImageRef::Owned(_, w, h) | ImageRef::Shared(_, w, h) => (*w, *h),
                            };
                            out.push(format!(
                                r#"{{"kind":"push-mask","x":{:.1},"y":{:.1},"w":{:.1},"h":{:.1},"mask_w":{},"mask_h":{}}}"#,
                                rect.x, rect.y, rect.w, rect.h, mw, mh
                            ));
                        }
                    }
                    PaintCmd::PopMask => out.push(r#"{"kind":"pop-mask"}"#.to_string()),
                    PaintCmd::PushBlendMode { mode } => {
                        out.push(format!(r#"{{"kind":"push-blend-mode","mode":{}}}"#, mode));
                    }
                    PaintCmd::PopBlendMode => {
                        out.push(r#"{"kind":"pop-blend-mode"}"#.to_string());
                    }
                    PaintCmd::PopClip => out.push(r#"{"kind":"pop-clip"}"#.to_string()),
                    _ => {}
                }
            }
            format!(
                r#"{{"ok":true,"count":{},"commands":[{}]}}"#,
                out.len(),
                out.join(",")
            )
        }
        "tree" => {
            let mut buf = String::new();
            fn dump(n: &webcore::WebCore, buf: &mut String, depth: usize) {
                let indent = "  ".repeat(depth);
                let id = n
                    .attributes
                    .get("id")
                    .map(|v| format!("#{v}"))
                    .unwrap_or_default();
                let cls = n
                    .attributes
                    .get("class")
                    .map(|v| {
                        format!(
                            ".{}",
                            v.split_whitespace().take(2).collect::<Vec<_>>().join(".")
                        )
                    })
                    .unwrap_or_default();
                let c = n.layout.content_rect;
                if n.tag == "#text" {
                    let t: String = n.text.trim().chars().take(40).collect();
                    if !t.is_empty() {
                        buf.push_str(&format!("{indent}#text \"{t}\"\n"));
                    }
                } else {
                    buf.push_str(&format!(
                        "{indent}{}{}{} [{:?}] {:.0}x{:.0}\n",
                        n.tag, id, cls, n.style.display, c.w, c.h
                    ));
                }
                for ch in &n.children {
                    dump(ch, buf, depth + 1);
                }
            }
            dump(&doc.root, &mut buf, 0);
            format!(r#"{{"ok":true,"tree":{}}}"#, dbg_json_escape(&buf))
        }
        "dom-tree" => {
            let nid = dbg_json_num(line, "nid").map(|n| n as u32);
            let depth = dbg_json_num(line, "depth").unwrap_or(3.0) as usize;
            let root_node = if let Some(id) = nid {
                doc.get_box_by_id(id).unwrap_or(&doc.root)
            } else {
                &doc.root
            };
            fn tj(n: &webcore::WebCore, d: usize, mx: usize) -> String {
                let cc = n
                    .children
                    .iter()
                    .filter(|c| !(c.tag == "#text" && c.text.trim().is_empty()))
                    .count();
                let tp = if n.tag == "#text" {
                    let t: String = n.text.trim().chars().take(60).collect();
                    format!(
                        r#","text":"{}""#,
                        t.replace('\\', "\\\\").replace('"', "\\\"")
                    )
                } else {
                    String::new()
                };
                let ch = if d < mx && cc > 0 {
                    let k: Vec<String> = n
                        .children
                        .iter()
                        .filter(|c| !(c.tag == "#text" && c.text.trim().is_empty()))
                        .map(|c| tj(c, d + 1, mx))
                        .collect();
                    format!(r#","children":[{}]"#, k.join(","))
                } else if cc > 0 {
                    format!(r#","child_count":{cc}"#)
                } else {
                    String::new()
                };
                let r = n.layout.content_rect;
                format!(
                    r#"{{"tag":"{}","id":"{}","class":"{}","nid":{},"rect":[{:.0},{:.0},{:.0},{:.0}]{},"count":{}{}}}"#,
                    n.tag,
                    n.attributes.get("id").unwrap_or(&String::new()),
                    n.attributes.get("class").unwrap_or(&String::new()),
                    n.node_id,
                    r.x,
                    r.y,
                    r.w,
                    r.h,
                    tp,
                    cc,
                    ch
                )
            }
            format!(r#"{{"ok":true,"tree":{}}}"#, tj(root_node, 0, depth))
        }
        "inspect-node" => {
            let nid = dbg_json_num(line, "nid").unwrap_or(0.0) as u32;
            if let Some(n) = doc.get_box_by_id(nid) {
                let l = &n.layout;
                format!(
                    r#"{{"ok":true,"tag":"{}","id":"{}","class":"{}","nid":{},"display":"{:?}","position":"{:?}","margin":{{"top":{:.1},"right":{:.1},"bottom":{:.1},"left":{:.1}}},"border":{{"top":{:.1},"right":{:.1},"bottom":{:.1},"left":{:.1}}},"padding":{{"top":{:.1},"right":{:.1},"bottom":{:.1},"left":{:.1}}},"content":{{"x":{:.1},"y":{:.1},"width":{:.1},"height":{:.1}}},"font_size":{:.1},"color":"{:02x}{:02x}{:02x}","bg":"{:02x}{:02x}{:02x}{:02x}","image":{{"width":{},"height":{},"bytes":{},"src":"{}"}}}}"#,
                    n.tag,
                    n.attributes.get("id").unwrap_or(&String::new()),
                    n.attributes.get("class").unwrap_or(&String::new()),
                    nid,
                    n.style.display,
                    n.style.position,
                    l.resolved_margin_top,
                    l.resolved_margin_right,
                    l.resolved_margin_bottom,
                    l.resolved_margin_left,
                    l.resolved_border_top,
                    l.resolved_border_right,
                    l.resolved_border_bottom,
                    l.resolved_border_left,
                    l.resolved_pad_top,
                    l.resolved_pad_right,
                    l.resolved_pad_bottom,
                    l.resolved_pad_left,
                    l.content_rect.x,
                    l.content_rect.y,
                    l.content_rect.w,
                    l.content_rect.h,
                    n.style.font_size_px(16.0, 16.0),
                    n.style.color.r,
                    n.style.color.g,
                    n.style.color.b,
                    n.style.background_color.r,
                    n.style.background_color.g,
                    n.style.background_color.b,
                    n.style.background_color.a,
                    n.image_width,
                    n.image_height,
                    n.image_data.as_ref().map(|d| d.len()).unwrap_or(0),
                    n.resolved_src.replace('"', "\\\"")
                )
            } else {
                format!(r#"{{"ok":false,"error":"node not found"}}"#)
            }
        }
        "chrome-screenshot" => {
            if chrome_port == 0 {
                return r#"{"ok":false,"error":"no --chrome"}"#.to_string();
            }
            let params = r#"{"format":"png"}"#;
            match cdp_send(chrome_port, "Page.captureScreenshot", params) {
                Ok(resp) => {
                    // Extract base64 data from CDP response
                    if let Some(data_start) = resp.find("\"data\":\"") {
                        let data = &resp[data_start + 8..];
                        if let Some(end) = data.find('"') {
                            let b64 = &data[..end];
                            let path = dbg_json_str(line, "out")
                                .unwrap_or_else(|| "chrome_screenshot.png".to_string());
                            if let Ok(bytes) = base64_decode_std(b64) {
                                if std::fs::write(&path, &bytes).is_ok() {
                                    return format!(r#"{{"ok":true,"path":"{}"}}"#, path);
                                }
                            }
                        }
                    }
                    format!(r#"{{"ok":false,"error":"failed to extract screenshot"}}"#)
                }
                Err(e) => format!(r#"{{"ok":false,"error":"{}"}}"#, e),
            }
        }
        // Ask BOTH engines for the same selector's geometry and report only
        // where they disagree.
        //
        // Everything needed for this was already here — a CDP transport and a
        // layout query — but nothing joined them, so comparing against a real
        // browser meant hand-writing a fixture with a `getBoundingClientRect`
        // script in it, dumping the DOM, and diffing the text by hand. One
        // command does it, and it scales to any page rather than only to
        // fixtures written for the purpose.
        "compare" => {
            if chrome_port == 0 {
                return r#"{"ok":false,"error":"no --chrome"}"#.to_string();
            }
            let sel = dbg_json_str(line, "selector").unwrap_or_else(|| "*".to_string());
            let tol: f32 = dbg_json_str(line, "tolerance")
                .and_then(|t| t.parse().ok())
                .unwrap_or(0.5);

            // Optional: put BOTH engines into the same hover state first, so a
            // menu that only exists while hovered can be compared at all. A
            // static diff cannot see a dropdown, which is exactly where the
            // interesting differences live.
            if let Some(hsel) = dbg_json_str(line, "hover") {
                // Chrome: ask the page where the target is, then send a real
                // mouse move there. JS cannot set `:hover`; the input event can.
                let find_js = format!(
                    "(()=>{{const e=document.querySelector({hsel:?});if(!e)return '';\
                     const r=e.getBoundingClientRect();\
                     return (r.x+r.width/2)+','+(r.y+r.height/2)}})()"
                );
                let params = format!(
                    r#"{{"expression":{},"returnByValue":true}}"#,
                    json_quote(&find_js)
                );
                if let Ok(resp) = cdp_send(chrome_port, "Runtime.evaluate", &params) {
                    if let Some(v) = dbg_json_str(&resp, "value") {
                        let mut it = v.split(',');
                        if let (Some(x), Some(y)) = (it.next(), it.next()) {
                            let move_params = format!(
                                r#"{{"type":"mouseMoved","x":{},"y":{},"button":"none","buttons":0,"clickCount":0}}"#,
                                x.trim(),
                                y.trim()
                            );
                            let _ = cdp_send(chrome_port, "Input.dispatchMouseEvent", &move_params);
                        }
                    }
                }
                // Ours: the same move, then a full layout so the hover cascade
                // runs before anything is measured.
                if let Some((hx, hy)) = dbg_selector_center(doc, &hsel) {
                    let pt = (hx, hy + doc.scroll_y);
                    doc.process_mouse_event(webcore::dom::HtmlEventType::MouseMove, pt, 0);
                    renderer.layout_engine().layout(doc, width);
                }
            }

            // Key each match by its id, falling back to document order, so the
            // two lists line up even when a selector matches unnamed elements.
            // Document-relative, not viewport-relative: `getBoundingClientRect`
            // is relative to the viewport, our rects are absolute, and any page
            // tall enough to scroll made every y disagree by the scroll offset.
            let js = format!(
                "[...document.querySelectorAll({sel:?})].map((e,i)=>{{const r=e.getBoundingClientRect();                 return [e.id||('@'+i),r.x+window.scrollX,r.y+window.scrollY,r.width,r.height].join(',')}}).join(';')"
            );
            let params = format!(
                r#"{{"expression":{},"returnByValue":true}}"#,
                json_quote(&js)
            );
            let resp = match cdp_send(chrome_port, "Runtime.evaluate", &params) {
                Ok(r) => r,
                Err(e) => return format!(r#"{{"ok":false,"error":"{}"}}"#, e),
            };
            let Some(payload) = dbg_json_str(&resp, "value") else {
                return format!(
                    r#"{{"ok":false,"error":"no value from Chrome","raw":{}}}"#,
                    json_quote(&resp)
                );
            };
            let mut theirs: Vec<(String, [f32; 4])> = Vec::new();
            for rec in payload.split(';').filter(|r| !r.is_empty()) {
                let f: Vec<&str> = rec.split(',').collect();
                if f.len() == 5 {
                    let n = |i: usize| f[i].parse::<f32>().unwrap_or(f32::NAN);
                    theirs.push((f[0].to_string(), [n(1), n(2), n(3), n(4)]));
                }
            }
            let nodes = webcore::dom::query_selector_all(&doc.root, &sel);
            let mut diffs: Vec<String> = Vec::new();
            for (i, n) in nodes.iter().enumerate() {
                let key = match n.attributes.get("id") {
                    Some(id) if !id.is_empty() => id.clone(),
                    _ => format!("@{i}"),
                };
                let Some((_, t)) = theirs.iter().find(|(k, _)| *k == key) else {
                    continue;
                };
                let b = n.layout.border_rect;
                let ours = [b.x, b.y, b.w, b.h];
                if ours.iter().zip(t.iter()).any(|(a, c)| (a - c).abs() > tol) {
                    diffs.push(format!(
                        r#"{{"key":"{}","tag":"{}","ours":{{"x":{},"y":{},"w":{},"h":{}}},"chrome":{{"x":{},"y":{},"w":{},"h":{}}}}}"#,
                        key, n.tag,
                        fmt_px(ours[0]), fmt_px(ours[1]), fmt_px(ours[2]), fmt_px(ours[3]),
                        fmt_px(t[0]), fmt_px(t[1]), fmt_px(t[2]), fmt_px(t[3])));
                }
            }
            format!(
                r#"{{"ok":true,"selector":"{}","tolerance":{},"ours":{},"chrome":{},"mismatched":{},"diffs":[{}]}}"#,
                sel,
                tol,
                nodes.len(),
                theirs.len(),
                diffs.len(),
                diffs.join(",")
            )
        }
        "chrome-sync" | "sync" => {
            if chrome_port == 0 {
                return r#"{"ok":false,"error":"no --chrome"}"#.to_string();
            }
            let scroll_y = doc.scroll_y;
            let params = format!(
                r#"{{"expression":"window.scrollTo(0,{});[window.scrollX,window.scrollY,document.title]","returnByValue":true}}"#,
                scroll_y as i32
            );
            match cdp_send(chrome_port, "Runtime.evaluate", &params) {
                Ok(resp) => format!(r#"{{"ok":true,"chrome_response":{}}}"#, resp),
                Err(e) => format!(r#"{{"ok":false,"error":"{}"}}"#, e),
            }
        }
        "box-model" => {
            let sel = dbg_json_str(line, "selector").unwrap_or_default();
            if let Some(n) = webcore::dom::query_selector(&doc.root, &sel) {
                let l = &n.layout;
                format!(
                    r#"{{"ok":true,"tag":"{}","margin":{{"top":{:.1},"right":{:.1},"bottom":{:.1},"left":{:.1}}},"border":{{"top":{:.1},"right":{:.1},"bottom":{:.1},"left":{:.1}}},"padding":{{"top":{:.1},"right":{:.1},"bottom":{:.1},"left":{:.1}}},"content":{{"width":{:.1},"height":{:.1}}}}}"#,
                    n.tag,
                    l.resolved_margin_top,
                    l.resolved_margin_right,
                    l.resolved_margin_bottom,
                    l.resolved_margin_left,
                    l.resolved_border_top,
                    l.resolved_border_right,
                    l.resolved_border_bottom,
                    l.resolved_border_left,
                    l.resolved_pad_top,
                    l.resolved_pad_right,
                    l.resolved_pad_bottom,
                    l.resolved_pad_left,
                    l.content_rect.w,
                    l.content_rect.h
                )
            } else {
                r#"{"ok":false,"error":"not found"}"#.to_string()
            }
        }
        "dom-path" | "path" => {
            let sel = dbg_json_str(line, "selector").unwrap_or_default();
            if let Some(n) = webcore::dom::query_selector(&doc.root, &sel) {
                fn bp(root: &webcore::WebCore, tid: u32, p: &mut Vec<String>) -> bool {
                    let id = root
                        .attributes
                        .get("id")
                        .map(|v| format!("#{v}"))
                        .unwrap_or_default();
                    let cls = root
                        .attributes
                        .get("class")
                        .map(|v| format!(".{}", v.split_whitespace().next().unwrap_or("")))
                        .unwrap_or_default();
                    p.push(format!("{}{}{}", root.tag, id, cls));
                    if root.node_id == tid {
                        return true;
                    }
                    for c in &root.children {
                        if bp(c, tid, p) {
                            return true;
                        }
                    }
                    p.pop();
                    false
                }
                let mut p = Vec::new();
                bp(&doc.root, n.node_id, &mut p);
                format!(r#"{{"ok":true,"path":"{}"}}"#, p.join(" > "))
            } else {
                r#"{"ok":false,"error":"not found"}"#.to_string()
            }
        }
        "parent" => {
            let sel = dbg_json_str(line, "selector").unwrap_or_default();
            if let Some(n) = webcore::dom::query_selector(&doc.root, &sel) {
                fn anc(r: &webcore::WebCore, tid: u32, ch: &mut Vec<String>) -> bool {
                    if r.node_id == tid {
                        ch.push(format!(r#"{{"tag":"{}","nid":{}}}"#, r.tag, r.node_id));
                        return true;
                    }
                    for c in &r.children {
                        if anc(c, tid, ch) {
                            ch.push(format!(r#"{{"tag":"{}","nid":{}}}"#, r.tag, r.node_id));
                            return true;
                        }
                    }
                    false
                }
                let mut ch = Vec::new();
                anc(&doc.root, n.node_id, &mut ch);
                format!(r#"{{"ok":true,"chain":[{}]}}"#, ch.join(","))
            } else {
                r#"{"ok":false,"error":"not found"}"#.to_string()
            }
        }
        "hit" => {
            let x = dbg_json_num(line, "x").unwrap_or(0.0) as f32;
            let y = dbg_json_num(line, "y").unwrap_or(0.0) as f32;
            if let Some(hit) = webcore::layout::hit_test::point_to_hit(&doc.root, (x, y), 0) {
                if let Some(n) = doc.get_box_by_id(hit.node_id) {
                    format!(
                        r#"{{"ok":true,"nid":{},"tag":"{}","class":"{}"}}"#,
                        hit.node_id,
                        n.tag,
                        n.attributes.get("class").unwrap_or(&String::new())
                    )
                } else {
                    format!(r#"{{"ok":true,"nid":{}}}"#, hit.node_id)
                }
            } else {
                r#"{"ok":false,"error":"no hit"}"#.to_string()
            }
        }
        "search" => {
            let q = dbg_json_str(line, "query")
                .unwrap_or_default()
                .to_lowercase();
            let mut results = Vec::new();
            fn sw(n: &webcore::WebCore, q: &str, r: &mut Vec<String>) {
                if n.tag == "#text" && n.text.to_lowercase().contains(q) {
                    r.push(format!(
                        r#"{{"nid":{},"text":"{}"}}"#,
                        n.node_id,
                        n.text
                            .trim()
                            .chars()
                            .take(60)
                            .collect::<String>()
                            .replace('"', "\\\"")
                    ));
                }
                for c in &n.children {
                    sw(c, q, r);
                }
            }
            sw(&doc.root, &q, &mut results);
            format!(
                r#"{{"ok":true,"count":{},"results":[{}]}}"#,
                results.len(),
                results.join(",")
            )
        }
        "viewport" => {
            let doc_h = Document::scroll_height(&doc.root);
            format!(
                r#"{{"ok":true,"width":{:.0},"height":{:.0},"doc_height":{:.0}}}"#,
                width, height, doc_h
            )
        }
        "network" => {
            let mut img = 0u32;
            webcore::Document::walk_all(&doc.root, &mut |b| {
                if b.image_data.is_some() {
                    img += 1;
                }
            });
            format!(
                r#"{{"ok":true,"stylesheets":{},"images":{}}}"#,
                doc.linked_stylesheets.len(),
                img
            )
        }
        "a11y" | "accessibility" => {
            fn aw(n: &webcore::WebCore, d: usize, o: &mut String) {
                let role = match n.tag.as_str() {
                    "a" => "link",
                    "button" | "input" => "button",
                    "img" => "image",
                    "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => "heading",
                    "nav" => "navigation",
                    "main" => "main",
                    "ul" | "ol" => "list",
                    "li" => "listitem",
                    "#text" => {
                        if !n.text.trim().is_empty() {
                            "text"
                        } else {
                            return;
                        }
                    }
                    _ => n.attributes.get("role").map(|s| s.as_str()).unwrap_or(""),
                };
                if !role.is_empty() {
                    let label = n
                        .attributes
                        .get("aria-label")
                        .or(n.attributes.get("alt"))
                        .cloned()
                        .unwrap_or_else(|| {
                            if n.tag == "#text" {
                                n.text.trim().chars().take(40).collect()
                            } else {
                                String::new()
                            }
                        });
                    o.push_str(&format!("{}{}: {}\n", "  ".repeat(d), role, label));
                }
                for c in &n.children {
                    aw(c, d + if !role.is_empty() { 1 } else { 0 }, o);
                }
            }
            let mut t = String::new();
            aw(&doc.root, 0, &mut t);
            format!(r#"{{"ok":true,"tree":{}}}"#, dbg_json_escape(&t))
        }
        "text" => {
            let sel = dbg_json_str(line, "selector").unwrap_or_default();
            let nodes = webcore::dom::query_selector_all(&doc.root, &sel);
            let texts: Vec<String> = nodes
                .iter()
                .map(|n| {
                    format!(
                        "\"{}\"",
                        webcore::dom::get_text_content(n).replace('"', "\\\"")
                    )
                })
                .collect();
            format!(
                r#"{{"ok":true,"count":{},"texts":[{}]}}"#,
                texts.len(),
                texts.join(",")
            )
        }
        "attr" => {
            let sel = dbg_json_str(line, "selector").unwrap_or_default();
            let name = dbg_json_str(line, "name").unwrap_or_default();
            let nodes = webcore::dom::query_selector_all(&doc.root, &sel);
            let vals: Vec<String> = nodes
                .iter()
                .map(|n| {
                    format!(
                        "\"{}\"",
                        n.attributes
                            .get(&name)
                            .unwrap_or(&String::new())
                            .replace('"', "\\\"")
                    )
                })
                .collect();
            format!(
                r#"{{"ok":true,"count":{},"values":[{}]}}"#,
                vals.len(),
                vals.join(",")
            )
        }
        "setstyle" => {
            let sel = dbg_json_str(line, "selector").unwrap_or_default();
            let prop = dbg_json_str(line, "prop").unwrap_or_default();
            let val = dbg_json_str(line, "value").unwrap_or_default();
            if let Some(n) = webcore::dom::query_selector_mut(&mut doc.root, &sel) {
                webcore::dom::set_style_property(n, &prop, &val);
            }
            renderer.layout_engine().layout(doc, width);
            r#"{"ok":true}"#.to_string()
        }
        "highlight" => {
            let sel = dbg_json_str(line, "selector").unwrap_or_default();
            let out = dbg_json_str(line, "out").unwrap_or_else(|| "highlight.png".to_string());
            let rh = Document::scroll_height(&doc.root).ceil() as u32;
            let rh = rh.max(1).min(4000);
            if let Some(mut pm) = tiny_skia::Pixmap::new(width as u32, rh) {
                pm.fill(tiny_skia::Color::WHITE);
                renderer.render(doc, &mut pm, 1.0);
                for node in webcore::dom::query_selector_all(&doc.root, &sel) {
                    webcore::draw_inspect_overlay(node, &mut pm, doc.scroll_x, doc.scroll_y, 1.0);
                }
                let _ = pm.save_png(&out);
                format!(r#"{{"ok":true,"path":"{}"}}"#, out)
            } else {
                r#"{"ok":false,"error":"pixmap failed"}"#.to_string()
            }
        }
        // ── Inspect (by selector or nid) ─────────────────────────────────
        "inspect" => {
            let sel = dbg_json_str(line, "selector").unwrap_or_default();
            let mut parts = Vec::new();
            Document::walk_all(&doc.root, &mut |node| {
                if dbg_matches_query(doc, node, &sel) {
                    parts.push(dbg_inspect_json(node));
                }
            });
            format!(
                r#"{{"ok":true,"count":{},"elements":[{}]}}"#,
                parts.len(),
                parts.join(",")
            )
        }
        "svg-metrics" => {
            format!(r#"{{"ok":true,"svg":{}}}"#, dbg_svg_metrics_json(&doc.root))
        }
        // ── Computed styles ──────────────────────────────────────────────
        "computed" => {
            let sel = dbg_json_str(line, "selector").unwrap_or_default();
            let mut parts = Vec::new();
            Document::walk_all(&doc.root, &mut |node| {
                if dbg_matches_query(doc, node, &sel) {
                    parts.push(dbg_computed_json(node));
                }
            });
            format!(
                r#"{{"ok":true,"count":{},"elements":[{}]}}"#,
                parts.len(),
                parts.join(",")
            )
        }
        // ── Matched CSS rules ────────────────────────────────────────────
        "rules" | "matched-rules" => {
            let sel = dbg_json_str(line, "selector").unwrap_or_default();
            let mut results = Vec::new();
            Document::walk_all(&doc.root, &mut |node| {
                if dbg_matches_query(doc, node, &sel) {
                    let rules: Vec<String> = node.matched_rules.iter().map(|r| {
                        let decls: Vec<String> = r.declarations.iter()
                            .filter(|(k, _)| !k.starts_with("--"))
                            .map(|(k, v)| format!("{}:{}", dbg_json_escape(k), dbg_json_escape(v)))
                            .collect();
                        format!(r#"{{"selector":{},"specificity":{},"source":{},"layer":{},"layer_rank":{},"declarations":{{{}}}}}"#,
                            dbg_json_escape(&r.selector), r.specificity,
                            dbg_json_escape(&r.source),
                            dbg_json_escape(&r.layer), r.layer_rank, decls.join(","))
                    }).collect();
                    let id = node.attributes.get("id").map(|v| v.as_str()).unwrap_or("");
                    let cls = node
                        .attributes
                        .get("class")
                        .map(|v| v.as_str())
                        .unwrap_or("");
                    results.push(format!(
                        r#"{{"tag":{},"id":{},"class":{},"rules":[{}]}}"#,
                        dbg_json_escape(&node.tag),
                        dbg_json_escape(id),
                        dbg_json_escape(cls),
                        rules.join(",")
                    ));
                }
            });
            format!(
                r#"{{"ok":true,"count":{},"elements":[{}]}}"#,
                results.len(),
                results.join(",")
            )
        }
        // ── Deep inspect ─────────────────────────────────────────────────
        "deep" => {
            let selector = dbg_json_str(line, "selector").unwrap_or_default();
            let nodes = webcore::dom::query_selector_all(&doc.root, &selector);
            let mut items: Vec<String> = Vec::new();
            for node in nodes {
                let cr = node.layout.content_rect;
                let pr = node.layout.padding_rect;
                let mr = node.layout.margin_rect;
                let cls = node.attributes.get("class").cloned().unwrap_or_default();
                let id = node.attributes.get("id").cloned().unwrap_or_default();
                let before_display = node
                    .style
                    .before_style
                    .as_ref()
                    .map(|s| format!("{:?}", s.display))
                    .unwrap_or_default();
                let after_display = node
                    .style
                    .after_style
                    .as_ref()
                    .map(|s| format!("{:?}", s.display))
                    .unwrap_or_default();
                let children: Vec<String> = node.children.iter()
                    .filter(|c| c.tag != "#text" || !c.text.trim().is_empty())
                    .map(|c| {
                        let cc = c.layout.content_rect;
                        format!(r#"{{"tag":"{}","id":"{}","class":"{}","display":"{:?}","float":"{:?}","c":[{:.0},{:.0},{:.0},{:.0}]}}"#,
                            c.tag, c.attributes.get("id").unwrap_or(&String::new()),
                            c.attributes.get("class").unwrap_or(&String::new()),
                            c.style.display, c.style.float, cc.x, cc.y, cc.w, cc.h)
                    }).collect();
                items.push(format!(
                    r#"{{"tag":"{}","id":"{}","class":"{}","content":[{:.0},{:.0},{:.0},{:.0}],"padding":[{:.0},{:.0},{:.0},{:.0}],"margin":[{:.0},{:.0},{:.0},{:.0}],"display":"{:?}","float":"{:?}","before_content":{},"before_display":{},"after_content":{},"after_display":{},"children":[{}]}}"#,
                    node.tag, id, cls,
                    cr.x, cr.y, cr.w, cr.h,
                    pr.x, pr.y, pr.w, pr.h,
                    mr.x, mr.y, mr.w, mr.h,
                    node.style.display, node.style.float,
                    dbg_json_escape(&node.style.before_content),
                    dbg_json_escape(&before_display),
                    dbg_json_escape(&node.style.after_content),
                    dbg_json_escape(&after_display),
                    children.join(",")
                ));
            }
            format!(
                r#"{{"ok":true,"count":{},"elements":[{}]}}"#,
                items.len(),
                items.join(",")
            )
        }
        // ── CSS property query ───────────────────────────────────────────
        "css" => {
            let selector = dbg_json_str(line, "selector").unwrap_or_default();
            let props_str = dbg_json_str(line, "props").unwrap_or_default();
            // Ids, so the DOM's own resolver can answer every property rather
            // than a hardcoded handful. It knows ~47; the old match knew nine
            // and reported "(unknown)" for the rest, which made the inspector
            // useless for exactly the questions worth asking.
            let ids = webcore::dom::query_selector_all_ids(&doc.root, &selector);
            let mut items: Vec<String> = Vec::new();
            for nid in ids {
                let (tag, id, cls) = match doc.get_box_by_id(nid) {
                    Some(n) => (
                        n.tag.clone(),
                        n.attributes.get("id").cloned().unwrap_or_default(),
                        n.attributes.get("class").cloned().unwrap_or_default(),
                    ),
                    None => continue,
                };
                let mut kv: Vec<String> = vec![
                    format!(r#""tag":"{}""#, tag),
                    format!(r#""id":"{}""#, id),
                    format!(r#""class":"{}""#, dbg_json_escape(&cls)),
                ];
                for prop in props_str.split(',') {
                    let prop = prop.trim();
                    if prop.is_empty() {
                        continue;
                    }
                    // Geometry the resolver does not carry.
                    let val = match prop {
                        "content-rect" | "padding-rect" | "margin-rect" | "border-rect" => {
                            match doc.get_box_by_id(nid) {
                                Some(n) => {
                                    let r = match prop {
                                        "content-rect" => n.layout.content_rect,
                                        "padding-rect" => n.layout.padding_rect,
                                        "margin-rect" => n.layout.margin_rect,
                                        _ => n.layout.border_rect,
                                    };
                                    format!("{:.1},{:.1} {:.1}x{:.1}", r.x, r.y, r.w, r.h)
                                }
                                None => String::new(),
                            }
                        }
                        _ => doc.computed_style_property(nid, prop),
                    };
                    kv.push(format!(r#""{}":"{}""#, prop, dbg_json_escape(&val)));
                }
                items.push(format!("{{{}}}", kv.join(",")));
            }
            format!(
                r#"{{"ok":true,"count":{},"elements":[{}]}}"#,
                items.len(),
                items.join(",")
            )
        }
        // ── Benchmark ────────────────────────────────────────────────────
        "bench" => {
            let n = dbg_json_num(line, "n").unwrap_or(1.0) as u32;
            let n = n.max(1).min(100);
            let mut layout_times = Vec::new();
            for _ in 0..n {
                let t = std::time::Instant::now();
                renderer.layout_engine().layout(doc, width);
                layout_times.push(t.elapsed().as_micros() as f64 / 1000.0);
            }
            let avg = layout_times.iter().sum::<f64>() / layout_times.len() as f64;
            format!(
                r#"{{"ok":true,"iterations":{},"layout_avg_ms":{:.1}}}"#,
                n, avg
            )
        }
        "perf" => {
            let rules = doc.stylesheet.rules.len();
            let mut node_count = 0u32;
            Document::walk_all(&doc.root, &mut |_| {
                node_count += 1;
            });
            format!(
                r#"{{"ok":true,"nodes":{},"css_rules":{},"doc_height":{:.0}}}"#,
                node_count,
                rules,
                Document::scroll_height(&doc.root)
            )
        }
        "rule-search" => {
            let query = dbg_json_str(line, "query").unwrap_or_default();
            let limit = dbg_json_num(line, "limit").unwrap_or(20.0).max(1.0) as usize;
            let mut matches = Vec::new();
            for rule in &doc.stylesheet.rules {
                if matches.len() >= limit {
                    break;
                }
                if rule.original_selector.contains(&query) {
                    let decls: Vec<String> = rule
                        .declarations
                        .iter()
                        .filter(|(k, _)| !k.starts_with("--"))
                        .map(|(k, v)| format!("{}:{}", dbg_json_escape(k), dbg_json_escape(v)))
                        .collect();
                    let selector_ast = format!("{:?}", rule.selectors);
                    let scope_selector = format!("{:?}", rule.scope_selector);
                    let scope_limit_selector = format!("{:?}", rule.scope_limit_selector);
                    let scopes = format!("{:?}", rule.scopes);
                    matches.push(format!(
                        r#"{{"selector":{},"selector_ast":{},"pseudo":"{:?}","specificity":{},"layer":{},"layer_rank":{},"media":{},"scope":{},"scope_limit":{},"scopes":{},"compiled":{},"compiled_important":{},"declarations":{{{}}}}}"#,
                        dbg_json_escape(&rule.original_selector),
                        dbg_json_escape(&selector_ast),
                        rule.pseudo_element,
                        rule.specificity,
                        dbg_json_escape(&rule.layer),
                        rule.layer_rank,
                        dbg_json_escape(&rule.media_condition),
                        dbg_json_escape(&scope_selector),
                        dbg_json_escape(&scope_limit_selector),
                        dbg_json_escape(&scopes),
                        rule.compiled_decls.len(),
                        rule.compiled_important.len(),
                        decls.join(",")
                    ));
                }
            }
            format!(
                r#"{{"ok":true,"query":{},"count":{},"rules":[{}]}}"#,
                dbg_json_escape(&query),
                matches.len(),
                matches.join(",")
            )
        }
        "match-debug" => match dbg_json_str(line, "selector") {
            Some(sel) => {
                let mut parts = Vec::new();
                let limit = dbg_json_num(line, "limit").unwrap_or(10.0).max(1.0) as usize;
                Document::walk_all(&doc.root, &mut |node| {
                    if parts.len() >= limit {
                        return;
                    }
                    if dbg_matches_query(doc, node, &sel) {
                        parts.push(dbg_match_report_json(doc, node));
                    }
                });
                format!(
                    r#"{{"ok":true,"count":{},"elements":[{}]}}"#,
                    parts.len(),
                    parts.join(",")
                )
            }
            None => r#"{"ok":false,"error":"match-debug needs selector"}"#.to_string(),
        },
        "node-id-stats" => {
            let mut total = 0usize;
            let mut zero = 0usize;
            let mut ids = std::collections::HashMap::<u32, usize>::new();
            Document::walk_all(&doc.root, &mut |node| {
                total += 1;
                if node.node_id == 0 {
                    zero += 1;
                } else {
                    *ids.entry(node.node_id).or_insert(0) += 1;
                }
            });
            let mut duplicates = ids
                .into_iter()
                .filter(|(_, count)| *count > 1)
                .collect::<Vec<_>>();
            duplicates.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
            let duplicate_extras = duplicates
                .iter()
                .map(|(_, count)| count.saturating_sub(1))
                .sum::<usize>();
            let dup_json = duplicates
                .iter()
                .take(20)
                .map(|(id, count)| format!(r#"{{"id":{},"count":{}}}"#, id, count))
                .collect::<Vec<_>>()
                .join(",");
            format!(
                r#"{{"ok":true,"total":{},"zero":{},"unique":{},"duplicate_ids":{},"duplicates":[{}]}}"#,
                total,
                zero,
                total.saturating_sub(zero).saturating_sub(duplicate_extras),
                duplicates.len(),
                dup_json
            )
        }
        "keyframes" => {
            let query = dbg_json_str(line, "query").unwrap_or_default();
            let limit = dbg_json_num(line, "limit").unwrap_or(20.0).max(1.0) as usize;
            let mut matches = Vec::new();
            for (name, stops) in &doc.stylesheet.keyframes {
                if matches.len() >= limit {
                    break;
                }
                if !query.is_empty() && !name.contains(&query) {
                    continue;
                }
                let stop_json: Vec<String> = stops
                    .iter()
                    .map(|stop| {
                        let props: Vec<String> = stop
                            .properties
                            .iter()
                            .map(|(k, v)| format!("{}:{}", dbg_json_escape(k), dbg_json_escape(v)))
                            .collect();
                        format!(
                            r#"{{"offset":{:.6},"properties":{{{}}}}}"#,
                            stop.offset,
                            props.join(",")
                        )
                    })
                    .collect();
                matches.push(format!(
                    r#"{{"name":{},"stops":[{}]}}"#,
                    dbg_json_escape(name),
                    stop_json.join(",")
                ));
            }
            format!(
                r#"{{"ok":true,"query":{},"count":{},"keyframes":[{}]}}"#,
                dbg_json_escape(&query),
                matches.len(),
                matches.join(",")
            )
        }
        "resolve-css" => {
            let value = dbg_json_str(line, "value").unwrap_or_default();
            let resolved = webcore::css::resolve_var_references(&value, &doc.stylesheet.variables);
            format!(
                r#"{{"ok":true,"value":{},"resolved":{},"variables":{}}}"#,
                dbg_json_escape(&value),
                dbg_json_escape(&resolved),
                doc.stylesheet.variables.len()
            )
        }
        // ── Force element state ──────────────────────────────────────────
        "force-state" => {
            let selector = dbg_json_str(line, "selector").unwrap_or_default();
            let state = dbg_json_str(line, "state").unwrap_or_default();
            if let Some(node) = webcore::dom::query_selector(&doc.root, &selector) {
                let nid = node.node_id;
                match state.as_str() {
                    "hover" => {
                        doc.hovered_box = nid;
                        doc.hover_changed = true;
                    }
                    "focus" => {
                        doc.focused_box = nid;
                    }
                    "active" => {
                        doc.active_box = nid;
                    }
                    _ => {}
                }
            }
            doc.style_dirty = true;
            renderer.layout_engine().layout(doc, width);
            r#"{"ok":true}"#.to_string()
        }
        // ── Set attribute ────────────────────────────────────────────────
        "setattr" => {
            match (
                dbg_json_str(line, "selector"),
                dbg_json_str(line, "name"),
                dbg_json_str(line, "value"),
            ) {
                (Some(sel), Some(name), Some(val)) => {
                    let mut count = 0usize;
                    let hits = dbg_query_ids(doc, &sel);
                    Document::walk_all_mut(&mut doc.root, &mut |node| {
                        if hits.contains(&node.node_id) {
                            node.attributes.insert(name.clone(), val.clone());
                            count += 1;
                        }
                    });
                    if count > 0 {
                        renderer.layout_engine().layout(doc, width);
                    }
                    format!(r#"{{"ok":true,"modified":{}}}"#, count)
                }
                _ => r#"{"ok":false,"error":"setattr needs selector, name, value"}"#.to_string(),
            }
        }
        // ── Set text content ─────────────────────────────────────────────
        "set-text" => {
            let selector = dbg_json_str(line, "selector").unwrap_or_default();
            let text = dbg_json_str(line, "text").unwrap_or_default();
            if let Some(node) = webcore::dom::query_selector_mut(&mut doc.root, &selector) {
                webcore::dom::set_text_content(node, &text);
            }
            renderer.layout_engine().layout(doc, width);
            r#"{"ok":true}"#.to_string()
        }
        // ── Add/remove/toggle class ──────────────────────────────────────
        "add-class" => {
            let selector = dbg_json_str(line, "selector").unwrap_or_default();
            let cls = dbg_json_str(line, "class").unwrap_or_default();
            if let Some(node) = webcore::dom::query_selector_mut(&mut doc.root, &selector) {
                webcore::dom::add_class(node, &cls);
            }
            doc.style_dirty = true;
            renderer.layout_engine().layout(doc, width);
            r#"{"ok":true}"#.to_string()
        }
        "remove-class" => {
            let selector = dbg_json_str(line, "selector").unwrap_or_default();
            let cls = dbg_json_str(line, "class").unwrap_or_default();
            if let Some(node) = webcore::dom::query_selector_mut(&mut doc.root, &selector) {
                webcore::dom::remove_class(node, &cls);
            }
            doc.style_dirty = true;
            renderer.layout_engine().layout(doc, width);
            r#"{"ok":true}"#.to_string()
        }
        "toggle-class" => {
            let selector = dbg_json_str(line, "selector").unwrap_or_default();
            let cls = dbg_json_str(line, "class").unwrap_or_default();
            if let Some(node) = webcore::dom::query_selector_mut(&mut doc.root, &selector) {
                webcore::dom::toggle_class(node, &cls);
            }
            doc.style_dirty = true;
            renderer.layout_engine().layout(doc, width);
            r#"{"ok":true}"#.to_string()
        }
        // Line boxes for a selector: the only view of what inline layout
        // actually produced — heights, baselines and text extents.
        "lines" => {
            let selector = dbg_json_str(line, "selector").unwrap_or_default();
            let nodes = webcore::dom::query_selector_all(&doc.root, &selector);
            let mut items: Vec<String> = Vec::new();
            for node in nodes.into_iter().take(6) {
                let lines: Vec<String> = node.layout.line_cache.iter().map(|l| {
                    let visual: Vec<String> = l.visual_segments.iter().map(|seg| format!(
                        r#"{{"s":{},"len":{},"level":{},"x":{:.2},"w":{:.2}}}"#,
                        seg.logical_start, seg.length, seg.level, seg.x, seg.width
                    )).collect();
                    format!(
                        r#"{{"x":{:.2},"y":{:.2},"h":{:.2},"asc":{:.2},"desc":{:.2},"w":{:.2},"text_x":{:.2},"len":{},"visual":[{}]}}"#,
                        l.x,
                        l.y,
                        l.height,
                        l.ascent,
                        l.descent,
                        l.width,
                        l.text_x_offset,
                        l.text_length,
                        visual.join(",")
                    )
                }).collect();
                items.push(format!(
                    r#"{{"tag":"{}","id":"{}","font_px":{:.2},"line_height":"{:?}","content_h":{:.2},"lines":[{}]}}"#,
                    node.tag,
                    node.attributes.get("id").unwrap_or(&String::new()),
                    node.style.font_size_px(16.0, 16.0),
                    node.style.line_height,
                    node.layout.content_rect.h,
                    lines.join(",")));
            }
            format!(
                r#"{{"ok":true,"count":{},"elements":[{}]}}"#,
                items.len(),
                items.join(",")
            )
        }
        "quit" => std::process::exit(0),
        _ => format!(r#"{{"ok":false,"error":"unknown: {}"}}"#, cmd),
    };
    let ms = cmd_start.elapsed().as_micros() as f64 / 1000.0;
    if result.ends_with('}') {
        format!(
            "{}{}\"cmd_ms\":{:.2}}}",
            &result[..result.len() - 1],
            if result.len() > 2 { "," } else { "" },
            ms
        )
    } else {
        result
    }
}
