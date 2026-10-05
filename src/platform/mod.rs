use softbuffer::{Context, Surface};
use std::sync::Arc;
use tiny_skia::Pixmap;
use winit::window::Window;

#[cfg(target_os = "macos")]
mod video_layer_macos;

#[derive(Clone)]
pub struct VideoForeground {
    pub rgba: Arc<Vec<u8>>,
    pub source_width: u32,
    pub source_height: u32,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Clone)]
pub struct VideoLayerFrame {
    pub rgba: Arc<Vec<u8>>,
    pub source_width: u32,
    pub source_height: u32,
    pub presentation_size: Option<(u32, u32)>,
    /// Destination in logical window coordinates.
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub cover: bool,
    pub corner_radius: f32,
    pub tint: Option<[u8; 4]>,
    pub foreground: Option<VideoForeground>,
}

impl VideoLayerFrame {
    /// Normalized centered source crop, shared by native layers and capture.
    pub fn contents_rect(&self) -> (f64, f64, f64, f64) {
        let (width, height) = self.presentation_size
            .filter(|&(width, height)| width > 0 && height > 0)
            .unwrap_or((self.source_width, self.source_height));
        if !self.cover || width == 0 || height == 0 || !self.width.is_finite()
            || !self.height.is_finite() || self.width <= 0.0 || self.height <= 0.0
        { return (0.0, 0.0, 1.0, 1.0); }
        let source_aspect = f64::from(width) / f64::from(height);
        let destination_aspect = f64::from(self.width) / f64::from(self.height);
        if source_aspect > destination_aspect {
            let crop_width = destination_aspect / source_aspect;
            ((1.0 - crop_width) * 0.5, 0.0, crop_width, 1.0)
        } else {
            let crop_height = source_aspect / destination_aspect;
            (0.0, (1.0 - crop_height) * 0.5, 1.0, crop_height)
        }
    }
    /// Use the same pixel check as software composition before selecting a layer.
    pub fn is_opaque(&self) -> bool {
        let expected = u64::from(self.source_width)
            .checked_mul(u64::from(self.source_height))
            .and_then(|pixels| pixels.checked_mul(4));
        self.source_width != 0 && self.source_height != 0
            && expected == Some(self.rgba.len() as u64)
            && crate::renderer::display_list_replay::rgba_is_opaque(&self.rgba)
    }
}

#[cfg(target_os = "macos")]
fn tinted_video_pixels(pixels: &Arc<Vec<u8>>, tint: Option<[u8; 4]>) -> Arc<Vec<u8>> {
    let Some([r, g, b, alpha]) = tint.filter(|color| color[3] != 0) else {
        return pixels.clone();
    };
    let table = [r, g, b].map(|channel| {
        std::array::from_fn::<_, 256, _>(|value| {
            ((value as u32 * ((255 - alpha as u32) * 255 + alpha as u32 * channel as u32)
                + 32512)
                / 65025) as u8
        })
    });
    let mut output = Vec::with_capacity(pixels.len());
    for pixel in pixels.chunks_exact(4) {
        output.extend_from_slice(&[
            table[0][pixel[0] as usize],
            table[1][pixel[1] as usize],
            table[2][pixel[2] as usize],
            pixel[3],
        ]);
    }
    Arc::new(output)
}

#[cfg(target_os = "macos")]
fn paint_video_layer_for_capture(pixmap: &mut Pixmap, frame: &VideoLayerFrame, scale: f32) {
    use crate::renderer::display_list::{DisplayList, ImageRef, PaintCmd};
    use crate::types::Rect;

    let rect = Rect::new(frame.x, frame.y, frame.width, frame.height);
    let (crop_x, crop_y, crop_width, crop_height) = frame.contents_rect();
    let width = frame.width / crop_width as f32;
    let height = frame.height / crop_height as f32;
    let x = frame.x - crop_x as f32 * width;
    let y = frame.y - crop_y as f32 * height;
    let mut list = DisplayList::new();
    list.push(PaintCmd::PushClip {
        rect,
        radius: [frame.corner_radius; 4],
        radius_y: [frame.corner_radius; 4],
    });
    list.push(PaintCmd::Image {
        rect: Rect::new(x, y, width, height),
        data: ImageRef::Shared(
            tinted_video_pixels(&frame.rgba, frame.tint),
            frame.source_width,
            frame.source_height,
        ),
    });
    list.push(PaintCmd::PopClip);
    if let Some(foreground) = &frame.foreground {
        list.push(PaintCmd::Image {
            rect: Rect::new(foreground.x, foreground.y, foreground.width, foreground.height),
            data: ImageRef::Shared(
                foreground.rgba.clone(),
                foreground.source_width,
                foreground.source_height,
            ),
        });
    }
    crate::renderer::display_list_replay::replay(&list, pixmap, scale);
}

pub mod dialogs;

fn rgba_to_argb_scalar(dst: &mut [u32], rgba: &[u8]) {
    for (dst, chunk) in dst.iter_mut().zip(rgba.chunks_exact(4)) {
        *dst = ((chunk[3] as u32) << 24)
            | ((chunk[0] as u32) << 16)
            | ((chunk[1] as u32) << 8)
            | (chunk[2] as u32);
    }
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
#[target_feature(enable = "neon")]
unsafe fn rgba_to_argb_neon(dst: &mut [u32], rgba: &[u8]) {
    use std::arch::aarch64::*;

    let count = dst.len().min(rgba.len() / 4);
    let mut index = 0;
    while index + 16 <= count {
        let channels = unsafe { vld4q_u8(rgba.as_ptr().add(index * 4)) };
        let swapped = uint8x16x4_t(channels.2, channels.1, channels.0, channels.3);
        unsafe { vst4q_u8(dst.as_mut_ptr().cast::<u8>().add(index * 4), swapped) };
        index += 16;
    }
    rgba_to_argb_scalar(&mut dst[index..count], &rgba[index * 4..count * 4]);
}

fn rgba_to_argb(dst: &mut [u32], rgba: &[u8]) {
    #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
    {
        unsafe { rgba_to_argb_neon(dst, rgba) };
        return;
    }
    #[cfg(not(all(target_arch = "aarch64", target_endian = "little")))]
    rgba_to_argb_scalar(dst, rgba);
}

fn recycle_framebuffer(previous: &Pixmap, pool: &mut Vec<Arc<Pixmap>>, preserve: bool) -> Option<Arc<Pixmap>> {
    let mut next = pool.iter().position(|p| Arc::strong_count(p) == 1)
        .map(|index| pool.swap_remove(index))
        .or_else(|| Pixmap::new(previous.width(), previous.height()).map(Arc::new))?;
    // Partial repaints need the latest frame, not the recycled buffer's contents.
    if preserve {
        Arc::get_mut(&mut next)?.data_mut().copy_from_slice(previous.data());
    }
    Some(next)
}

pub struct Platform {
    surface: Surface<Arc<Window>, Arc<Window>>,
    window: Arc<Window>,
    width: u32,
    height: u32,
    /// Reused across frames to avoid per-frame allocation (~10 MB on 2× Retina).
    pixmap: Option<Arc<Pixmap>>,
    framebuffer_pool: Vec<Arc<Pixmap>>,
    #[cfg(target_os = "macos")]
    video_layer: Option<video_layer_macos::VideoLayer>,
    #[cfg(target_os = "macos")]
    last_video_frame: Option<VideoLayerFrame>,
}

impl Platform {
    /// Save the last frame presented by this browser window without drawing
    /// another frame. Useful when a repaint would hide a stale-surface bug.
    pub fn save_presented_png(&self, path: &str) -> Result<(u32, u32), String> {
        let pixmap = self.pixmap.as_ref().ok_or("no frame has been presented")?;
        #[cfg(target_os = "macos")]
        if let Some(frame) = &self.last_video_frame {
            if std::env::var_os("WEBCORE_TRACE_VIDEO_CAPTURE").is_some() {
                eprintln!(
                    "[video-capture] source={}x{} dest=({}, {}, {}, {}) scale={}",
                    frame.source_width, frame.source_height, frame.x, frame.y,
                    frame.width, frame.height, self.scale_factor(),
                );
            }
            let mut composite = pixmap.as_ref().clone();
            paint_video_layer_for_capture(&mut composite, frame, self.scale_factor());
            composite.save_png(path).map_err(|error| error.to_string())?;
            return Ok((composite.width(), composite.height()));
        }
        pixmap.save_png(path).map_err(|error| error.to_string())?;
        Ok((pixmap.width(), pixmap.height()))
    }

    pub fn new_windowed(window: Arc<Window>) -> Self {
        let context = Context::new(window.clone()).expect("Failed to create softbuffer context");
        let surface = Surface::new(&context, window.clone()).expect("Failed to create surface");
        let size = window.inner_size();
        #[cfg(target_os = "macos")]
        let video_layer = video_layer_macos::VideoLayer::new(&window);
        let mut platform = Self {
            surface,
            window,
            width: size.width,
            height: size.height,
            pixmap: None,
            framebuffer_pool: Vec::new(),
            #[cfg(target_os = "macos")]
            video_layer,
            #[cfg(target_os = "macos")]
            last_video_frame: None,
        };
        platform.resize(size.width, size.height);
        platform
    }

    /// HiDPI scale factor (physical pixels per logical pixel).
    pub fn scale_factor(&self) -> f32 {
        self.window.scale_factor() as f32
    }

    pub fn supports_video_layer(&self) -> bool {
        #[cfg(target_os = "macos")]
        {
            self.video_layer.is_some()
                && std::env::var_os("WEBCORE_DISABLE_NATIVE_VIDEO").is_none()
        }
        #[cfg(not(target_os = "macos"))]
        {
            false
        }
    }

    pub fn present_video_layer(&mut self, frame: Option<&VideoLayerFrame>) {
        #[cfg(target_os = "macos")]
        if let Some(layer) = self.video_layer.as_mut() {
            layer.present(frame);
            self.last_video_frame = frame.cloned();
        }
        #[cfg(not(target_os = "macos"))]
        let _ = frame;
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
        self.framebuffer_pool.clear();
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
        self.render_frame(true, draw);
    }

    /// Draw every pixel of the frame, without preserving the previous buffer.
    pub fn render_full<F: FnOnce(f32, &mut Pixmap)>(&mut self, draw: F) {
        self.render_frame(false, draw);
    }

    fn render_frame<F: FnOnce(f32, &mut Pixmap)>(&mut self, preserve: bool, draw: F) {
        let scale = self.scale_factor();

        // Reuse the pixmap across frames; only reallocate when dimensions change.
        // This avoids allocating+zeroing ~10 MB per frame on HiDPI displays.
        let need_new = self
            .pixmap
            .as_ref()
            .map(|p| p.width() != self.width || p.height() != self.height)
            .unwrap_or(true);
        if need_new {
            self.pixmap = Pixmap::new(self.width, self.height).map(Arc::new);
        } else if self.pixmap.as_ref().is_some_and(|p| Arc::strong_count(p) != 1) {
            // Core Animation may still be reading a previous frame. Recycle only
            // buffers whose presentation owner has released them.
            let next = recycle_framebuffer(self.pixmap.as_ref().unwrap(), &mut self.framebuffer_pool, preserve);
            if let Some(previous) = self.pixmap.take() {
                if self.framebuffer_pool.len() >= 3 {
                    self.framebuffer_pool.remove(0);
                }
                self.framebuffer_pool.push(previous);
            }
            self.pixmap = next;
        }
        let pixmap = match self.pixmap.as_mut().and_then(Arc::get_mut) {
            Some(p) => p,
            None => return,
        };

        {
            let _span = crate::profile::span(crate::profile::Phase::PlatformDraw);
            draw(scale, pixmap);
        }

        #[cfg(target_os = "macos")]
        if let Some(layer) = self.video_layer.as_ref() {
            let _span = crate::profile::span(crate::profile::Phase::PlatformPresent);
            if layer.present_framebuffer(self.pixmap.as_ref().unwrap()) {
                return;
            }
        }

        let pixmap = self.pixmap.as_ref().unwrap();

        // Blit premultiplied RGBA bytes into softbuffer's 0xAARRGGBB pixels.
        let mut buf = self
            .surface
            .buffer_mut()
            .expect("Failed to get surface buffer");
        {
            let _span = crate::profile::span(crate::profile::Phase::PlatformConvert);
            rgba_to_argb(&mut buf, pixmap.data());
        }
        {
            let _span = crate::profile::span(crate::profile::Phase::PlatformPresent);
            buf.present().expect("Failed to present buffer");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn video_cover_crop_uses_presentation_aspect_not_coded_aspect() {
        let mut frame = VideoLayerFrame {
            rgba: Arc::new(vec![255; 4 * 4 * 4]), source_width: 4, source_height: 4,
            presentation_size: Some((8, 4)),
            x: 0.0, y: 0.0, width: 100.0, height: 100.0, cover: true,
            corner_radius: 0.0, tint: None, foreground: None,
        };
        assert!(frame.is_opaque());
        assert_eq!(frame.contents_rect(), (0.25, 0.0, 0.5, 1.0));
        frame.presentation_size = Some((4, 8));
        assert_eq!(frame.contents_rect(), (0.0, 0.25, 1.0, 0.5));
        frame.cover = false;
        assert_eq!(frame.contents_rect(), (0.0, 0.0, 1.0, 1.0));
        frame.cover = true;
        frame.presentation_size = None;
        assert_eq!(frame.contents_rect(), (0.0, 0.0, 1.0, 1.0));
    }

    #[test]
    fn video_layer_opacity_requires_complete_nonempty_frames() {
        let mut frame = VideoLayerFrame {
            presentation_size: None,
            rgba: Arc::new(vec![100, 50, 25, 255]),
            source_width: 1, source_height: 1,
            x: 0.0, y: 0.0, width: 1.0, height: 1.0,
            cover: false, corner_radius: 0.0, tint: None, foreground: None,
        };
        assert!(frame.is_opaque());
        for (width, height, pixels) in [
            (0, 0, vec![]), (2, 1, vec![255; 4]),
            (1, 1, vec![255; 5]), (1, 1, vec![100, 50, 25, 254]),
            (u32::MAX, u32::MAX, vec![]),
        ] {
            frame.source_width = width;
            frame.source_height = height;
            frame.rgba = Arc::new(pixels);
            assert!(!frame.is_opaque());
        }
    }

    #[test]
    fn framebuffer_recycling_preserves_partial_paints_and_live_presentations() {
        let mut previous = Pixmap::new(4, 3).unwrap();
        previous.fill(tiny_skia::Color::from_rgba8(30, 60, 90, 255));
        let held = Arc::new(Pixmap::new(4, 3).unwrap());
        let presentation = held.clone();
        let reusable = Arc::new(Pixmap::new(4, 3).unwrap());
        let address = reusable.data().as_ptr();
        let mut pool = vec![held, reusable];
        let mut next = recycle_framebuffer(&previous, &mut pool, true).unwrap();
        assert_eq!(next.data().as_ptr(), address);
        assert_eq!(next.data(), previous.data());
        Arc::get_mut(&mut next).unwrap().data_mut()[0] = 1;
        assert!(presentation.data().iter().all(|byte| *byte == 0));
        assert_eq!(previous.data()[0], 30);
        drop(presentation);
        let address = pool[0].data().as_ptr();
        let next = recycle_framebuffer(&previous, &mut pool, true).unwrap();
        assert_eq!(next.data().as_ptr(), address);
        assert_eq!(next.data(), previous.data());
        let reusable = Arc::new(Pixmap::new(4, 3).unwrap());
        let address = reusable.data().as_ptr();
        pool.push(reusable);
        let next = recycle_framebuffer(&previous, &mut pool, false).unwrap();
        assert_eq!(next.data().as_ptr(), address);
        assert!(next.data().iter().all(|byte| *byte == 0));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn captured_frame_includes_native_video_layer() {
        let mut pixmap = Pixmap::new(8, 8).unwrap();
        pixmap.fill(tiny_skia::Color::from_rgba8(0, 0, 255, 255));
        let frame = VideoLayerFrame {
            presentation_size: None,
            rgba: Arc::new(vec![255, 0, 0, 255]),
            source_width: 1,
            source_height: 1,
            x: 2.0,
            y: 2.0,
            width: 4.0,
            height: 4.0,
            cover: false,
            corner_radius: 0.0,
            tint: None,
            foreground: Some(VideoForeground {
                rgba: Arc::new(vec![0, 255, 0, 255]),
                source_width: 1,
                source_height: 1,
                x: 3.0,
                y: 3.0,
                width: 1.0,
                height: 1.0,
            }),
        };
        assert!(frame.is_opaque());
        let mut invalid = frame.clone();
        invalid.source_width = 2;
        assert!(!invalid.is_opaque());
        invalid.source_width = 1;
        invalid.rgba = Arc::new(vec![255, 0, 0, 254]);
        assert!(!invalid.is_opaque());
        invalid.rgba = Arc::new(vec![255, 0, 0, 255, 0]);
        assert!(!invalid.is_opaque());
        paint_video_layer_for_capture(&mut pixmap, &frame, 1.0);
        assert_eq!(pixmap.pixel(3, 3).unwrap().green(), 255);
        assert_eq!(pixmap.pixel(4, 4).unwrap().red(), 255);
        assert_eq!(pixmap.pixel(0, 0).unwrap().red(), 0);
        assert_eq!(pixmap.pixel(0, 0).unwrap().blue(), 255);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn video_tint_multiplies_opaque_source_channels() {
        let source = Arc::new(vec![200, 100, 50, 255]);
        assert!(Arc::ptr_eq(&source, &tinted_video_pixels(&source, None)));
        assert_eq!(&*tinted_video_pixels(&source, Some([0, 255, 0, 128])), &[100, 100, 25, 255]);
    }

    #[test]
    fn argb_conversion_matches_scalar_for_vector_and_tail_pixels() {
        for count in [1, 15, 16, 17, 31, 32, 33] {
            let rgba: Vec<u8> = (0..count)
                .flat_map(|index| {
                    [
                        (index * 13) as u8,
                        (index * 7) as u8,
                        (index * 3) as u8,
                        (index * 11) as u8,
                    ]
                })
                .collect();
            let mut expected = vec![0u32; count];
            let mut actual = vec![0u32; count];
            rgba_to_argb_scalar(&mut expected, &rgba);
            rgba_to_argb(&mut actual, &rgba);
            assert_eq!(actual, expected);
        }
    }

    #[test]
    #[ignore = "run explicitly when measuring fullscreen presentation"]
    fn benchmark_fullscreen_argb_conversion() {
        let count = 2560 * 1440;
        let rgba: Vec<u8> = (0..count)
            .flat_map(|index| {
                [
                    (index * 13) as u8,
                    (index * 7) as u8,
                    (index * 3) as u8,
                    255,
                ]
            })
            .collect();
        let mut dst = vec![0u32; count as usize];
        for vector in [false, true, true, false] {
            let start = std::time::Instant::now();
            for _ in 0..40 {
                if vector {
                    rgba_to_argb(&mut dst, &rgba);
                } else {
                    rgba_to_argb_scalar(&mut dst, &rgba);
                }
                std::hint::black_box(&dst);
            }
            eprintln!(
                "fullscreen RGBA to ARGB x40 vector={vector}: {:?}",
                start.elapsed()
            );
        }
    }
}
