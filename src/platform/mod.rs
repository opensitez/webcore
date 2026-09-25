use softbuffer::{Context, Surface};
use std::sync::Arc;
use tiny_skia::Pixmap;
use winit::window::Window;

pub struct Platform {
    surface: Surface<Arc<Window>, Arc<Window>>,
    window: Arc<Window>,
    width: u32,
    height: u32,
    /// Reused across frames to avoid per-frame allocation (~10 MB on 2× Retina).
    pixmap: Option<Pixmap>,
}

impl Platform {
    /// Save the last frame presented by this browser window without drawing
    /// another frame. Useful when a repaint would hide a stale-surface bug.
    pub fn save_presented_png(&self, path: &str) -> Result<(u32, u32), String> {
        let pixmap = self.pixmap.as_ref().ok_or("no frame has been presented")?;
        pixmap.save_png(path).map_err(|error| error.to_string())?;
        Ok((pixmap.width(), pixmap.height()))
    }

    pub fn new_windowed(window: Arc<Window>) -> Self {
        let context = Context::new(window.clone()).expect("Failed to create softbuffer context");
        let surface = Surface::new(&context, window.clone()).expect("Failed to create surface");
        let size = window.inner_size();
        let mut platform = Self {
            surface,
            window,
            width: size.width,
            height: size.height,
            pixmap: None,
        };
        platform.resize(size.width, size.height);
        platform
    }

    /// HiDPI scale factor (physical pixels per logical pixel).
    pub fn scale_factor(&self) -> f32 {
        self.window.scale_factor() as f32
    }

    /// Viewport width in logical pixels — use this for layout.
    pub fn logical_width(&self) -> f32 {
        self.width as f32 / self.scale_factor()
    }

    /// Viewport height in logical pixels.
    pub fn logical_height(&self) -> f32 {
        self.height as f32 / self.scale_factor()
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.width = width.max(1);
        self.height = height.max(1);
        // Discard cached pixmap so next render reallocates at the new size.
        self.pixmap = None;
        self.surface
            .resize(
                std::num::NonZeroU32::new(self.width).unwrap(),
                std::num::NonZeroU32::new(self.height).unwrap(),
            )
            .expect("Failed to resize surface");
    }

    /// Render a frame using a closure that calls `renderer.render(doc, pixmap, scale)`.
    /// If the renderer signals that hover state changed, a new redraw is automatically
    /// requested so hover transitions appear immediately without host app changes.
    /// The closure receives `(scale, pixmap)`.
    pub fn render<F: FnOnce(f32, &mut Pixmap)>(&mut self, draw: F) {
        let scale = self.scale_factor();

        // Reuse the pixmap across frames; only reallocate when dimensions change.
        // This avoids allocating+zeroing ~10 MB per frame on HiDPI displays.
        let need_new = self
            .pixmap
            .as_ref()
            .map(|p| p.width() != self.width || p.height() != self.height)
            .unwrap_or(true);
        if need_new {
            self.pixmap = Pixmap::new(self.width, self.height);
        }
        let pixmap = match self.pixmap.as_mut() {
            Some(p) => p,
            None => return,
        };

        draw(scale, pixmap);

        // Blit pixmap (premultiplied RGBA bytes) → softbuffer (0xAARRGGBB native-endian).
        // Using raw byte access + chunks_exact so LLVM can auto-vectorise the loop.
        let mut buf = self
            .surface
            .buffer_mut()
            .expect("Failed to get surface buffer");
        let data = pixmap.data(); // [r,g,b,a, r,g,b,a, …]
        for (dst, chunk) in buf.iter_mut().zip(data.chunks_exact(4)) {
            *dst = ((chunk[3] as u32) << 24)   // a
                 | ((chunk[0] as u32) << 16)   // r
                 | ((chunk[1] as u32) <<  8)   // g
                 |  (chunk[2] as u32); // b
        }
        buf.present().expect("Failed to present buffer");
    }
}
