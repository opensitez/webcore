//! Demo Browser — a focused launcher for the webcore example pages.
//!
//! This keeps page loading, link activation, scrolling, hover, transitions, and
//! form/input handling on the same `BrowserView` path as the full browser while
//! giving the local demos a small persistent navigation shell.

mod demo_live;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use tiny_skia::{Pixmap, PixmapPaint, Transform};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{Cursor, CursorIcon, Window};

use webcore::dom::{self, HtmlEventType};
use webcore::platform::Platform;
use webcore::{BrowserView, CSSCursor, Document, PageLoadOptions, Renderer, parse_html};

use demo_live::{DemoLive, GraphComponent};

const CHROME_BAR_H: f32 = 42.0;
const CHROME_MENU_COLS: usize = 4;
const CHROME_MENU_ROW_H: f32 = 20.0;
const CHROME_MENU_GAP: f32 = 6.0;
const CHROME_MENU_VERTICAL_CHROME: f32 = 42.0;
const DEFAULT_WIDTH: f32 = 1180.0;
const DEFAULT_HEIGHT: f32 = 860.0;
const A4_PAGE_WIDTH_PX: f32 = 794.0;
const A4_PAGE_HEIGHT_PX: f32 = 1122.0;

#[derive(Clone, Copy)]
struct DemoEntry {
    file: &'static str,
    title: &'static str,
    category: &'static str,
}

const DEMOS: &[DemoEntry] = &[
    DemoEntry {
        file: "demo.html",
        title: "Demo Browser",
        category: "Hub",
    },
    DemoEntry {
        file: "hello.html",
        title: "Hello / Layout",
        category: "Core",
    },
    DemoEntry {
        file: "layout_features.html",
        title: "Layout Features",
        category: "Layout",
    },
    DemoEntry {
        file: "cascade_features.html",
        title: "Cascade Features",
        category: "CSS",
    },
    DemoEntry {
        file: "container.html",
        title: "Container Queries",
        category: "CSS",
    },
    DemoEntry {
        file: "subgrid.html",
        title: "Subgrid",
        category: "Layout",
    },
    DemoEntry {
        file: "overflow.html",
        title: "Overflow",
        category: "Layout",
    },
    DemoEntry {
        file: "transitions_demo.html",
        title: "Transitions",
        category: "CSS",
    },
    DemoEntry {
        file: "animation_demo.html",
        title: "Animations",
        category: "CSS",
    },
    DemoEntry {
        file: "transform_filter_demo.html",
        title: "Transforms / Filters",
        category: "CSS",
    },
    DemoEntry {
        file: "forms_demo.html",
        title: "Forms",
        category: "Controls",
    },
    DemoEntry {
        file: "contenteditable.html",
        title: "Contenteditable",
        category: "Editing",
    },
    DemoEntry {
        file: "edit.html",
        title: "Editor",
        category: "Editing",
    },
    DemoEntry {
        file: "edit_demo.html",
        title: "Edit Layout Test",
        category: "Editing",
    },
    DemoEntry {
        file: "markdown.html",
        title: "Markdown Editor",
        category: "Editing",
    },
    DemoEntry {
        file: "events.html",
        title: "Events",
        category: "Events",
    },
    DemoEntry {
        file: "event_playground.html",
        title: "Event Playground",
        category: "Events",
    },
    DemoEntry {
        file: "dom.html",
        title: "DOM Dashboard",
        category: "DOM",
    },
    DemoEntry {
        file: "graph.html",
        title: "Graph Components",
        category: "Components",
    },
    DemoEntry {
        file: "calculator.html",
        title: "Calculator",
        category: "Apps",
    },
    DemoEntry {
        file: "tictactoe.html",
        title: "Tic-Tac-Toe",
        category: "Apps",
    },
    DemoEntry {
        file: "minesweeper.html",
        title: "Minesweeper",
        category: "Apps",
    },
    DemoEntry {
        file: "email.html",
        title: "Mail",
        category: "Apps",
    },
    DemoEntry {
        file: "eudora.html",
        title: "Eudora",
        category: "Apps",
    },
    DemoEntry {
        file: "print.html",
        title: "Print",
        category: "Output",
    },
];

fn menu_demo_rows() -> usize {
    let entries = DEMOS.iter().filter(|demo| demo.file != "demo.html").count();
    entries.div_ceil(CHROME_MENU_COLS).max(1)
}

#[derive(Clone, Copy, Debug)]
enum ChromeHit {
    None,
    Menu,
    Demo(usize),
    Home,
    Back,
    Forward,
    Reload,
    Print,
}

struct DemoApp {
    window: Option<Arc<Window>>,
    platform: Option<Platform>,
    chrome_renderer: Renderer,
    chrome_doc: Option<Document>,
    chrome_pixmap: Option<Pixmap>,
    chrome_dirty: bool,
    view: BrowserView,
    mouse_pos: (f32, f32),
    pending_hover_pos: Option<(f32, f32)>,
    width: f32,
    height: f32,
    modifiers: ModifiersState,
    html_dir: PathBuf,
    home_url: String,
    current_url: String,
    current_title: String,
    history: Vec<String>,
    history_index: usize,
    menu_open: bool,
    print_preview: bool,
    live: DemoLive,
}

impl DemoApp {
    fn new(
        proxy: EventLoopProxy<()>,
        html_dir: PathBuf,
        home_url: String,
        initial_url: String,
    ) -> Self {
        let mut view = BrowserView::new(
            DEFAULT_WIDTH,
            DEFAULT_HEIGHT - CHROME_BAR_H,
            PageLoadOptions::default(),
        );
        let wake_proxy = proxy.clone();
        view.set_wake_callback(move || {
            let _ = wake_proxy.send_event(());
        });
        view.register_trait_component("graph", GraphComponent);
        Self {
            window: None,
            platform: None,
            chrome_renderer: Renderer::new(),
            chrome_doc: None,
            chrome_pixmap: None,
            chrome_dirty: true,
            view,
            mouse_pos: (0.0, 0.0),
            pending_hover_pos: None,
            width: DEFAULT_WIDTH,
            height: DEFAULT_HEIGHT,
            modifiers: ModifiersState::empty(),
            html_dir,
            home_url,
            current_url: initial_url,
            current_title: "Demo Browser".to_string(),
            history: Vec::new(),
            history_index: 0,
            menu_open: false,
            print_preview: false,
            live: DemoLive::new(),
        }
    }

    fn chrome_height(&self) -> f32 {
        if self.menu_open {
            CHROME_BAR_H + self.menu_panel_height()
        } else {
            CHROME_BAR_H
        }
    }

    fn menu_panel_height(&self) -> f32 {
        let rows = menu_demo_rows();
        CHROME_MENU_VERTICAL_CHROME
            + rows as f32 * CHROME_MENU_ROW_H
            + rows.saturating_sub(1) as f32 * CHROME_MENU_GAP
    }

    fn content_height(&self) -> f32 {
        (self.height - self.chrome_height()).max(1.0)
    }

    fn can_go_back(&self) -> bool {
        self.history_index > 0
    }

    fn can_go_forward(&self) -> bool {
        self.history_index + 1 < self.history.len()
    }

    fn navigate(&mut self, url: String, push_history: bool) {
        let url = resolve_demo_arg(&self.html_dir, &url);
        self.current_url = url.clone();
        self.current_title =
            title_for_url(&self.html_dir, &url).unwrap_or_else(|| "Loading…".to_string());
        if push_history {
            if !self.history.is_empty() {
                self.history.truncate(self.history_index + 1);
            }
            self.history.push(url.clone());
            self.history_index = self.history.len() - 1;
        }
        self.view.resize(self.width, self.content_height());
        self.view.navigate(url);
        self.rebuild_chrome();
    }

    fn sync_view_url(&mut self) {
        let view_url = self.view.url().to_string();
        if view_url.is_empty() || view_url == self.current_url {
            return;
        }
        self.current_url = view_url.clone();
        if self.history.last() != Some(&view_url) {
            self.history.truncate(self.history_index + 1);
            self.history.push(view_url);
            self.history_index = self.history.len() - 1;
        }
        self.rebuild_chrome();
    }

    fn current_demo_file(&self) -> Option<&str> {
        entry_for_url(&self.html_dir, &self.current_url).map(|entry| entry.file)
    }

    fn go_home(&mut self) {
        self.menu_open = false;
        self.navigate(self.home_url.clone(), true);
    }

    fn go_back(&mut self) {
        if !self.can_go_back() {
            return;
        }
        self.menu_open = false;
        self.history_index -= 1;
        let url = self.history[self.history_index].clone();
        self.navigate(url, false);
    }

    fn go_forward(&mut self) {
        if !self.can_go_forward() {
            return;
        }
        self.menu_open = false;
        self.history_index += 1;
        let url = self.history[self.history_index].clone();
        self.navigate(url, false);
    }

    fn reload(&mut self) {
        if self.current_url.is_empty() {
            return;
        }
        self.menu_open = false;
        self.view.navigate(self.current_url.clone());
        self.rebuild_chrome();
    }

    fn request_redraw(&self) {
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }

    fn resize_view(&mut self) {
        self.view.resize(self.width, self.content_height());
        self.rebuild_chrome();
    }

    fn rebuild_chrome(&mut self) {
        let html = self.chrome_html();
        let chrome_height = self.chrome_height();
        let mut doc = parse_html(&html);
        let engine = self.chrome_renderer.layout_engine();
        engine.viewport_h = chrome_height;
        engine.layout(&mut doc, self.width);
        self.chrome_doc = Some(doc);
        self.chrome_dirty = true;
    }

    fn chrome_html(&self) -> String {
        let height = self.chrome_height();
        let menu_h = self.menu_panel_height();
        let grid_h = (menu_h - CHROME_MENU_VERTICAL_CHROME).max(CHROME_MENU_ROW_H);
        let demos_label = if self.menu_open {
            "Demos ▴"
        } else {
            "Demos ▾"
        };
        let mut menu = String::new();
        if self.menu_open {
            menu.push_str(r#"<div class="menu"><div class="menu-head">Jump directly to any local demo</div><div class="demo-grid">"#);
            for (index, demo) in DEMOS
                .iter()
                .enumerate()
                .filter(|(_, demo)| demo.file != "demo.html")
            {
                let current = entry_for_url(&self.html_dir, &self.current_url)
                    .is_some_and(|entry| entry.file == demo.file);
                let class = if current {
                    "demo-row active"
                } else {
                    "demo-row"
                };
                menu.push_str(&format!(
                    r#"<div id="demo-{index}" class="{class}"><b>{}</b><span>{}</span><em>{}</em></div>"#,
                    escape_html(demo.title),
                    escape_html(demo.category),
                    escape_html(demo.file)
                ));
            }
            menu.push_str("</div></div>");
        }
        let raw_title = self
            .view
            .title()
            .trim()
            .is_empty()
            .then_some(self.current_title.as_str())
            .unwrap_or_else(|| self.view.title().trim());
        let title = escape_html(raw_title);
        let location = escape_html(&pretty_demo_url(&self.html_dir, &self.current_url));
        let back_class = if self.can_go_back() {
            "btn"
        } else {
            "btn disabled"
        };
        let forward_class = if self.can_go_forward() {
            "btn"
        } else {
            "btn disabled"
        };
        let print_class = if self.print_preview {
            "btn active"
        } else {
            "btn"
        };
        let print_label = if self.print_preview {
            "Print ✓"
        } else {
            "Print"
        };
        let loading = if self.view.is_loading() {
            " • loading"
        } else {
            ""
        };
        format!(
            r#"<!doctype html><html><head><style>
*{{box-sizing:border-box;margin:0;padding:0}}
body{{height:{height}px;overflow:hidden;background:#0f172a;color:#e5e7eb;
font-family:-apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,sans-serif}}
.bar{{height:42px;display:flex;align-items:center;gap:8px;padding:6px 10px;
background:#0f172a;border-bottom:1px solid #1f2937}}
.brand{{font-weight:800;font-size:13px;color:#f8fafc;margin-right:4px;white-space:nowrap}}
.btn{{height:28px;min-width:30px;padding:0 9px;border:1px solid #334155;
border-radius:8px;background:#1e293b;color:#dbeafe;font-size:12px;display:flex;
align-items:center;justify-content:center;cursor:pointer}}
.btn:hover{{background:#263449;border-color:#3b82f6}}
.active{{background:#7c2d12;border-color:#fb923c;color:#fff}}
.active:hover{{background:#9a3412;border-color:#fed7aa}}
.primary{{background:#2563eb;border-color:#60a5fa;color:#fff;font-weight:700;min-width:82px}}
.primary:hover{{background:#1d4ed8}}
.disabled{{color:#64748b;background:#111827;border-color:#1f2937;cursor:default}}
.loc{{flex:1;min-width:0;height:28px;border:1px solid #334155;border-radius:14px;
background:#020617;color:#cbd5e1;display:flex;align-items:center;padding:0 12px;
font-size:12px;overflow:hidden;white-space:nowrap}}
.title{{color:#f8fafc;font-weight:700;margin-right:8px}}
.sub{{color:#94a3b8;overflow:hidden;text-overflow:ellipsis}}
.menu{{height:{menu_h}px;background:#111827;border-bottom:1px solid #334155;padding:8px 10px 10px}}
.menu-head{{height:18px;color:#93c5fd;font-size:12px;font-weight:700;margin-bottom:6px}}
.demo-grid{{display:grid;grid-template-columns:repeat({CHROME_MENU_COLS},1fr);gap:{CHROME_MENU_GAP}px;height:{grid_h}px;overflow:hidden}}
.demo-row{{height:20px;border:1px solid #334155;border-radius:7px;background:#0f172a;color:#e5e7eb;
display:flex;align-items:center;gap:6px;padding:0 7px;font-size:12px;cursor:pointer;overflow:hidden;white-space:nowrap}}
.demo-row:hover{{background:#1e3a8a;border-color:#60a5fa}}
.demo-row.active{{background:#172554;border-color:#93c5fd}}
.demo-row b{{font-size:12px;font-weight:700;overflow:hidden;text-overflow:ellipsis}}
.demo-row span{{color:#bfdbfe;font-size:10px;text-transform:uppercase;letter-spacing:.04em}}
.demo-row em{{margin-left:auto;color:#94a3b8;font-style:normal;font-size:10px;overflow:hidden;text-overflow:ellipsis;max-width:86px}}
</style></head><body>
<div class="bar">
  <div id="menu-btn" class="btn primary">{demos_label}</div>
  <div id="back" class="{back_class}">←</div>
  <div id="forward" class="{forward_class}">→</div>
  <div id="reload" class="btn">↻</div>
  <div id="home" class="btn">Home</div>
  <div id="print" class="{print_class}">{print_label}</div>
  <div class="loc"><span class="title">{title}</span><span class="sub">{location}{loading}</span></div>
</div>
{menu}
</body></html>"#
        )
    }

    fn chrome_hit(&self, x: f32, y: f32) -> ChromeHit {
        let Some(doc) = self.chrome_doc.as_ref() else {
            return ChromeHit::None;
        };
        let hit_id = |id: &str| -> bool {
            dom::query_selector(&doc.root, &format!("#{id}")).is_some_and(|node| {
                let rect = node.layout.border_rect;
                x >= rect.x && x < rect.x + rect.w && y >= rect.y && y < rect.y + rect.h
            })
        };
        if hit_id("menu-btn") {
            ChromeHit::Menu
        } else if self.menu_open {
            DEMOS
                .iter()
                .enumerate()
                .filter(|(_, demo)| demo.file != "demo.html")
                .find_map(|(index, _)| {
                    hit_id(&format!("demo-{index}")).then_some(ChromeHit::Demo(index))
                })
                .unwrap_or_else(|| {
                    if hit_id("back") {
                        ChromeHit::Back
                    } else if hit_id("forward") {
                        ChromeHit::Forward
                    } else if hit_id("reload") {
                        ChromeHit::Reload
                    } else if hit_id("home") {
                        ChromeHit::Home
                    } else if hit_id("print") {
                        ChromeHit::Print
                    } else {
                        ChromeHit::None
                    }
                })
        } else if hit_id("back") {
            ChromeHit::Back
        } else if hit_id("forward") {
            ChromeHit::Forward
        } else if hit_id("reload") {
            ChromeHit::Reload
        } else if hit_id("home") {
            ChromeHit::Home
        } else if hit_id("print") {
            ChromeHit::Print
        } else {
            ChromeHit::None
        }
    }

    fn draw(&mut self) {
        let chrome_height = self.chrome_height();
        let print_preview = self.print_preview;
        let scroll_y = self.view.scroll_y();
        let Some(platform) = self.platform.as_mut() else {
            return;
        };
        platform.render(|scale, pixmap| {
            pixmap.fill(tiny_skia::Color::from_rgba8(15, 23, 42, 255));
            let chrome_px = ((chrome_height * scale).ceil() as u32).min(pixmap.height());
            self.view.paint_into(pixmap, 0, chrome_px as i32, scale);
            if print_preview {
                draw_page_breaks(pixmap, scale, chrome_px, scroll_y, A4_PAGE_HEIGHT_PX);
            }
            if let Some(doc) = self.chrome_doc.as_mut() {
                let needs_pm = self.chrome_pixmap.as_ref().is_none_or(|pm| {
                    pm.width() != pixmap.width() || pm.height() != chrome_px.max(1)
                });
                if needs_pm {
                    self.chrome_pixmap = Pixmap::new(pixmap.width(), chrome_px.max(1));
                    self.chrome_dirty = true;
                }
                if let Some(chrome) = self.chrome_pixmap.as_mut() {
                    if self.chrome_dirty {
                        chrome.fill(tiny_skia::Color::from_rgba8(15, 23, 42, 255));
                        self.chrome_renderer.render(doc, chrome, scale);
                        self.chrome_dirty = false;
                    }
                    pixmap.draw_pixmap(
                        0,
                        0,
                        chrome.as_ref(),
                        &PixmapPaint::default(),
                        Transform::identity(),
                        None,
                    );
                }
            }
        });
    }
}

impl ApplicationHandler<()> for DemoApp {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title("webcore Demo Browser")
                        .with_inner_size(winit::dpi::LogicalSize::new(
                            DEFAULT_WIDTH,
                            DEFAULT_HEIGHT,
                        )),
                )
                .expect("failed to create demo browser window"),
        );
        let platform = Platform::new_windowed(window.clone());
        self.width = platform.logical_width();
        self.height = platform.logical_height();
        self.window = Some(window);
        self.platform = Some(platform);
        self.rebuild_chrome();
        self.navigate(self.current_url.clone(), true);
    }

    fn user_event(&mut self, _event_loop: &winit::event_loop::ActiveEventLoop, _: ()) {
        self.request_redraw();
    }

    fn about_to_wait(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        self.view.resize(self.width, self.content_height());
        let hover_redraw = self
            .pending_hover_pos
            .take()
            .is_some_and(|(x, y)| self.view.handle_mouse_move(x, y));
        let needs_redraw = self.view.drive_idle(event_loop);
        self.sync_view_url();
        let file = self.current_demo_file().map(str::to_string);
        let live_redraw = { self.live.update(file.as_deref(), &mut self.view) };
        let live_layout_redraw = live_redraw && self.view.drive_idle(event_loop);
        if !self.view.title().is_empty() && self.current_title != self.view.title() {
            self.current_title = self.view.title().to_string();
            self.rebuild_chrome();
        }
        if let Some(deadline) = self.live.next_wake_deadline(file.as_deref(), &self.view) {
            event_loop.set_control_flow(ControlFlow::WaitUntil(deadline.max(Instant::now())));
        }
        if hover_redraw || needs_redraw || live_redraw || live_layout_redraw {
            self.request_redraw();
        }
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        if matches!(event, WindowEvent::ModifiersChanged(_)) {
            self.view.handle_window_event(&event);
        }
        let mut redraw = false;
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
            }
            WindowEvent::Resized(size) => {
                if let Some(platform) = self.platform.as_mut() {
                    platform.resize(size.width, size.height);
                    self.width = platform.logical_width();
                    self.height = platform.logical_height();
                }
                self.resize_view();
                redraw = true;
            }
            WindowEvent::CursorMoved { position, .. } => {
                let scale = self
                    .platform
                    .as_ref()
                    .map(|platform| platform.scale_factor())
                    .unwrap_or(1.0);
                let x = position.x as f32 / scale;
                let y = position.y as f32 / scale;
                self.mouse_pos = (x, y);
                let chrome_h = self.chrome_height();
                if y >= chrome_h {
                    let content_y = y - chrome_h;
                    self.pending_hover_pos = Some((x, content_y));
                    if let Some(window) = self.window.as_ref() {
                        window.set_cursor(Cursor::Icon(cursor_icon(
                            self.view.cursor_at(x, content_y),
                        )));
                    }
                } else if let Some(window) = self.window.as_ref() {
                    self.pending_hover_pos = None;
                    window.set_cursor(Cursor::Icon(CursorIcon::Default));
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let button_num = match button {
                    MouseButton::Left => 0,
                    MouseButton::Middle => 1,
                    MouseButton::Right => 2,
                    _ => 0,
                };
                let (x, y) = self.mouse_pos;
                let chrome_h = self.chrome_height();
                if y < chrome_h && state == ElementState::Pressed {
                    match self.chrome_hit(x, y) {
                        ChromeHit::Menu => {
                            self.menu_open = !self.menu_open;
                            self.resize_view();
                        }
                        ChromeHit::Demo(index) => {
                            if let Some(demo) = DEMOS.get(index) {
                                self.menu_open = false;
                                self.navigate(file_url(&self.html_dir.join(demo.file)), true);
                            }
                        }
                        ChromeHit::Home => self.go_home(),
                        ChromeHit::Back => self.go_back(),
                        ChromeHit::Forward => self.go_forward(),
                        ChromeHit::Reload => self.reload(),
                        ChromeHit::Print => {
                            self.menu_open = false;
                            self.print_preview = !self.print_preview;
                            self.rebuild_chrome();
                        }
                        ChromeHit::None => {}
                    }
                    redraw = true;
                } else if y >= chrome_h {
                    if self.menu_open && state == ElementState::Pressed {
                        self.menu_open = false;
                        self.resize_view();
                    }
                    let event_type = if state == ElementState::Pressed {
                        HtmlEventType::MouseDown
                    } else {
                        HtmlEventType::MouseUp
                    };
                    redraw |=
                        self.view
                            .handle_mouse_button(event_type, x, y - chrome_h, button_num);
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let (_, y) = self.mouse_pos;
                let chrome_h = self.chrome_height();
                if y >= chrome_h {
                    let scale = self
                        .platform
                        .as_ref()
                        .map(|platform| platform.scale_factor())
                        .unwrap_or(1.0);
                    let (dx, dy) = match delta {
                        winit::event::MouseScrollDelta::LineDelta(x, y) => (-x * 40.0, -y * 40.0),
                        winit::event::MouseScrollDelta::PixelDelta(pos) => {
                            (-(pos.x as f32) / scale, -(pos.y as f32) / scale)
                        }
                    };
                    let _ = self.view.handle_wheel(dx, dy);
                    redraw = true;
                }
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                if self.handle_shortcut(&event.logical_key) {
                    redraw = true;
                } else {
                    let key_code = key_code(&event.logical_key);
                    let ch = key_char(&event.logical_key);
                    if key_code != 0 || ch.is_some() {
                        let key_redraw = self.view.handle_key(
                            HtmlEventType::KeyDown,
                            if key_code != 0 {
                                key_code
                            } else {
                                ch.unwrap_or(' ') as u32
                            },
                            ch,
                            self.modifiers.control_key(),
                            self.modifiers.shift_key(),
                            self.modifiers.alt_key(),
                            self.modifiers.super_key(),
                        );
                        redraw |= key_redraw;
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                self.draw();
            }
            _ => {}
        }
        if redraw {
            self.request_redraw();
        }
    }
}

impl DemoApp {
    fn handle_shortcut(&mut self, key: &Key) -> bool {
        let command = self.modifiers.super_key() || self.modifiers.control_key();
        match key {
            Key::Named(NamedKey::BrowserBack) => {
                self.go_back();
                true
            }
            Key::Named(NamedKey::BrowserForward) => {
                self.go_forward();
                true
            }
            Key::Named(NamedKey::BrowserRefresh) => {
                self.reload();
                true
            }
            Key::Named(NamedKey::Home) if command => {
                self.go_home();
                true
            }
            Key::Character(s) if command && s.eq_ignore_ascii_case("r") => {
                self.reload();
                true
            }
            Key::Character(s) if command && s.eq_ignore_ascii_case("[") => {
                self.go_back();
                true
            }
            Key::Character(s) if command && s.eq_ignore_ascii_case("]") => {
                self.go_forward();
                true
            }
            _ => false,
        }
    }
}

fn cursor_icon(cursor: CSSCursor) -> CursorIcon {
    match cursor {
        CSSCursor::Pointer => CursorIcon::Pointer,
        CSSCursor::Text => CursorIcon::Text,
        CSSCursor::Move => CursorIcon::Move,
        CSSCursor::NotAllowed => CursorIcon::NotAllowed,
        CSSCursor::Grab => CursorIcon::Grab,
        CSSCursor::Grabbing => CursorIcon::Grabbing,
        CSSCursor::ColResize => CursorIcon::ColResize,
        CSSCursor::RowResize => CursorIcon::RowResize,
        CSSCursor::Crosshair => CursorIcon::Crosshair,
        CSSCursor::Help => CursorIcon::Help,
        CSSCursor::Wait => CursorIcon::Wait,
        _ => CursorIcon::Default,
    }
}

fn key_code(key: &Key) -> u32 {
    match key {
        Key::Named(NamedKey::Backspace) => 8,
        Key::Named(NamedKey::Tab) => 9,
        Key::Named(NamedKey::Enter) => 13,
        Key::Named(NamedKey::Escape) => 27,
        Key::Named(NamedKey::Delete) => 46,
        Key::Named(NamedKey::ArrowLeft) => 37,
        Key::Named(NamedKey::ArrowUp) => 38,
        Key::Named(NamedKey::ArrowRight) => 39,
        Key::Named(NamedKey::ArrowDown) => 40,
        Key::Named(NamedKey::Home) => 36,
        Key::Named(NamedKey::End) => 35,
        Key::Named(NamedKey::Space) => 32,
        _ => 0,
    }
}

fn key_char(key: &Key) -> Option<char> {
    match key {
        Key::Character(s) => s.chars().next(),
        Key::Named(NamedKey::Space) => Some(' '),
        Key::Named(NamedKey::Tab) => Some('\t'),
        _ => None,
    }
}

fn resolve_demo_arg(html_dir: &Path, raw: &str) -> String {
    if raw.starts_with("http://") || raw.starts_with("https://") || raw.starts_with("file://") {
        return raw.to_string();
    }
    let candidate = if raw.ends_with(".html") {
        html_dir.join(raw)
    } else {
        html_dir.join(format!("{raw}.html"))
    };
    if candidate.exists() {
        return file_url(&candidate);
    }
    raw.to_string()
}

fn file_url(path: &Path) -> String {
    let absolute = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    format!("file://{}", absolute.display())
}

fn pretty_demo_url(html_dir: &Path, url: &str) -> String {
    let prefix = file_url(html_dir);
    if let Some(rest) = url.strip_prefix(&(prefix + "/")) {
        return rest.to_string();
    }
    url.trim_start_matches("file://").to_string()
}

fn title_for_url(html_dir: &Path, url: &str) -> Option<String> {
    entry_for_url(html_dir, url).map(|demo| demo.title.to_string())
}

fn entry_for_url<'a>(html_dir: &Path, url: &str) -> Option<&'a DemoEntry> {
    let pretty = pretty_demo_url(html_dir, url);
    DEMOS.iter().find(|demo| demo.file == pretty)
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn draw_page_breaks(
    pixmap: &mut Pixmap,
    scale: f32,
    chrome_px: u32,
    scroll_y: f32,
    page_height: f32,
) {
    let pix_w = pixmap.width();
    let pix_h = pixmap.height();
    if pix_w == 0 || pix_h == 0 || chrome_px >= pix_h || scale <= 0.0 || page_height <= 0.0 {
        return;
    }

    let viewport_w = pix_w as f32 / scale;
    let viewport_h = (pix_h - chrome_px) as f32 / scale;
    let page_w = A4_PAGE_WIDTH_PX.min((viewport_w - 32.0).max(320.0));
    let page_x = ((viewport_w - page_w) * 0.5).max(0.0);
    let page_x_px = (page_x * scale).round().max(0.0);
    let page_w_px = (page_w * scale).round().min(pix_w as f32);
    let content_top = chrome_px as f32;
    let content_h = (pix_h - chrome_px) as f32;

    let mut shade = tiny_skia::Paint::default();
    shade.set_color_rgba8(15, 23, 42, 150);
    let identity = Transform::identity();
    if page_x_px > 0.0 {
        if let Some(rect) = tiny_skia::Rect::from_xywh(0.0, content_top, page_x_px, content_h) {
            pixmap.fill_rect(rect, &shade, identity, None);
        }
    }
    let right_x = page_x_px + page_w_px;
    if right_x < pix_w as f32 {
        if let Some(rect) =
            tiny_skia::Rect::from_xywh(right_x, content_top, pix_w as f32 - right_x, content_h)
        {
            pixmap.fill_rect(rect, &shade, identity, None);
        }
    }

    let mut border = tiny_skia::Paint::default();
    border.set_color_rgba8(245, 158, 11, 235);
    for edge_x in [page_x_px, (page_x_px + page_w_px - 2.0).max(page_x_px)] {
        if let Some(rect) = tiny_skia::Rect::from_xywh(edge_x, content_top, 2.0, content_h) {
            pixmap.fill_rect(rect, &border, identity, None);
        }
    }

    let mut boundary = (scroll_y / page_height).floor() * page_height;
    let red = tiny_skia::ColorU8::from_rgba(220, 50, 50, 190).premultiply();
    let shadow = tiny_skia::ColorU8::from_rgba(255, 255, 255, 120).premultiply();
    let pixels = pixmap.pixels_mut();
    while boundary <= scroll_y + viewport_h {
        let y_physical = chrome_px as f32 + (boundary - scroll_y) * scale;
        if y_physical >= chrome_px as f32 && y_physical < pix_h as f32 {
            let row = y_physical.floor() as u32;
            for marker_row in [row.saturating_sub(1), row] {
                if marker_row >= chrome_px && marker_row < pix_h {
                    let color = if marker_row == row { red } else { shadow };
                    let base = (marker_row * pix_w) as usize;
                    let start = page_x_px.max(0.0).floor() as usize;
                    let end = (page_x_px + page_w_px).min(pix_w as f32).ceil() as usize;
                    for col in start..end {
                        pixels[base + col] = color;
                    }
                }
            }
        }
        boundary += page_height;
    }
}

fn initial_url_from_args_or_home(html_dir: &Path, home_url: &str) -> String {
    let Some(arg) = std::env::args().nth(1) else {
        return home_url.to_string();
    };
    resolve_demo_arg(html_dir, &arg)
}

fn write_home_file(html_dir: &Path) -> String {
    let path =
        std::env::temp_dir().join(format!("webcore-demo-browser-{}.html", std::process::id()));
    match std::fs::write(&path, build_home_html(html_dir)) {
        Ok(()) => file_url(&path),
        Err(_) => file_url(&html_dir.join("demo.html")),
    }
}

fn build_home_html(html_dir: &Path) -> String {
    let mut sections = String::new();
    let mut current_category = "";
    for demo in DEMOS.iter().filter(|demo| demo.file != "demo.html") {
        if demo.category != current_category {
            if !current_category.is_empty() {
                sections.push_str("</div></section>");
            }
            current_category = demo.category;
            sections.push_str(&format!(
                r#"<section class="section"><h2>{}</h2><div class="links">"#,
                escape_html(current_category)
            ));
        }
        let url = escape_html(&file_url(&html_dir.join(demo.file)));
        sections.push_str(&format!(
            r#"<a href="{url}"><b>{}</b><span>{}</span></a>"#,
            escape_html(demo.title),
            escape_html(demo.file)
        ));
    }
    if !current_category.is_empty() {
        sections.push_str("</div></section>");
    }
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>webcore Demo Browser</title>
<style>
*{{box-sizing:border-box}}
body{{margin:0;padding:14px;font-family:-apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,sans-serif;background:#f8fafc;color:#111827;line-height:1.25}}
header{{padding:10px 12px;margin-bottom:10px;border:1px solid #cbd5e1;border-radius:8px;background:#fff}}
h1{{margin:0;font-size:22px}}
p{{margin:3px 0 0;color:#475569;font-size:13px}}
.catalog{{display:grid;grid-template-columns:repeat(4,1fr);gap:10px;align-items:start}}
.section{{border:1px solid #cbd5e1;border-radius:8px;background:#fff;overflow:hidden}}
h2{{margin:0;padding:7px 9px;background:#f1f5f9;border-bottom:1px solid #cbd5e1;font-size:12px;text-transform:uppercase;letter-spacing:.07em;color:#475569}}
.links{{display:flex;flex-direction:column}}
a{{display:flex;justify-content:space-between;gap:10px;padding:7px 9px;border-bottom:1px solid #e2e8f0;color:#0f172a;text-decoration:none;font-size:13px}}
a:last-child{{border-bottom:none}}
a:hover{{background:#eff6ff;color:#1d4ed8}}
a b{{font-weight:650}}
a span{{color:#64748b;font-size:11px;white-space:nowrap}}
@media(max-width:900px){{.catalog{{grid-template-columns:repeat(2,1fr)}}}}
</style>
</head>
<body>
<header>
<h1>webcore Demo Browser</h1>
<p>Pick a local demo. Use the toolbar to jump demos, navigate, reload, or preview print page breaks for the current page.</p>
</header>
<main class="catalog">{sections}</main>
</body>
</html>"#
    )
}

fn main() {
    let html_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/html");
    let home_url = write_home_file(&html_dir);
    let initial_url = initial_url_from_args_or_home(&html_dir, &home_url);
    let event_loop = EventLoop::<()>::with_user_event().build().unwrap();
    event_loop.set_control_flow(ControlFlow::Wait);
    let proxy = event_loop.create_proxy();
    let mut app = DemoApp::new(proxy, html_dir, home_url, initial_url);
    event_loop.run_app(&mut app).unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_dropdown_height_fits_all_entries() {
        let entries = DEMOS.iter().filter(|demo| demo.file != "demo.html").count();
        let rows = menu_demo_rows();
        assert!(rows * CHROME_MENU_COLS >= entries);
        let grid_height =
            rows as f32 * CHROME_MENU_ROW_H + rows.saturating_sub(1) as f32 * CHROME_MENU_GAP;
        let panel_height = CHROME_MENU_VERTICAL_CHROME + grid_height;
        assert!(panel_height > 184.0);
    }
}
