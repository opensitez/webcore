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
    let (x, y, width, height) = if frame.cover {
        let factor = (frame.width / frame.source_width as f32)
            .max(frame.height / frame.source_height as f32);
        let width = frame.source_width as f32 * factor;
        let height = frame.source_height as f32 * factor;
        (
            frame.x + (frame.width - width) * 0.5,
            frame.y + (frame.height - height) * 0.5,
            width,
            height,
        )
    } else {
        (frame.x, frame.y, frame.width, frame.height)
    };
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

pub struct Platform {
    surface: Surface<Arc<Window>, Arc<Window>>,
    window: Arc<Window>,
    width: u32,
    height: u32,
    /// Reused across frames to avoid per-frame allocation (~10 MB on 2× Retina).
    pixmap: Option<Pixmap>,
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
            let mut composite = pixmap.clone();
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

        {
            let _span = crate::profile::span(crate::profile::Phase::PlatformDraw);
            draw(scale, pixmap);
        }

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

    #[cfg(target_os = "macos")]
    #[test]
    fn captured_frame_includes_native_video_layer() {
        let mut pixmap = Pixmap::new(8, 8).unwrap();
        pixmap.fill(tiny_skia::Color::from_rgba8(0, 0, 255, 255));
        let frame = VideoLayerFrame {
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
