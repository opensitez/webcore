use super::{VideoLayerFrame, tinted_video_pixels};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{MainThreadMarker, msg_send};
use objc2_core_foundation::{CFRetained, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{
    CGBitmapInfo, CGColorRenderingIntent, CGColorSpace, CGDataProvider, CGImage, CGImageAlphaInfo,
    CGImageByteOrderInfo, CGImageComponentInfo, CGImagePixelFormatInfo, kCGColorSpaceSRGB,
};
use objc2_foundation::NSObject;
use objc2_quartz_core::{
    CALayer, CATransaction, kCAGravityResize,
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::Arc;
use winit::window::Window;
use std::time::{Duration, Instant};

#[cfg(target_os = "macos")]
fn commit_thread_cpu_time() -> Option<Duration> {
    use std::ffi::{c_int, c_long, c_uint};
    // Local Darwin SDK _time.h and arm/i386 _types.h: enum clockid_t,
    // CLOCK_THREAD_CPUTIME_ID=16; timespec contains two C longs.
    #[repr(C)]
    struct Timespec { seconds: c_long, nanoseconds: c_long }
    unsafe extern "C" {
        fn clock_gettime(clock: c_uint, time: *mut Timespec) -> c_int;
    }
    let mut time = Timespec { seconds: 0, nanoseconds: 0 };
    if unsafe { clock_gettime(16, &mut time) } != 0
        || time.seconds < 0 || !(0..1_000_000_000).contains(&time.nanoseconds)
    { return None; }
    Some(Duration::new(time.seconds as u64, time.nanoseconds as u32))
}

#[cfg(not(target_os = "macos"))]
fn commit_thread_cpu_time() -> Option<Duration> { None }

#[derive(Clone, Copy, Default)]
struct CommitChanges {
    geometry: bool,
    scale: bool,
    opacity: bool,
    visibility: bool,
}

struct CommitSample {
    wall: Duration,
    cpu: Option<Duration>,
    dimensions: (u32, u32),
    changes: CommitChanges,
}

#[derive(Default)]
struct CommitSummary {
    frames: u64,
    cpu_frames: u64,
    wall: Duration,
    wall_max: Duration,
    cpu: Duration,
    cpu_max: Duration,
    paired_wall: Duration,
    dimensions: (u32, u32),
    dimension_changes: u64,
    geometry_changes: u64,
    scale_changes: u64,
    opacity_changes: u64,
    visibility_changes: u64,
}

struct CommitWindow {
    started: Instant,
    summary: CommitSummary,
}

impl CommitWindow {
    fn new(now: Instant) -> Self { Self { started: now, summary: CommitSummary::default() } }

    fn record(&mut self, sample: CommitSample, now: Instant) -> Option<CommitSummary> {
        let summary = &mut self.summary;
        if summary.frames != 0 && summary.dimensions != sample.dimensions { summary.dimension_changes += 1; }
        summary.frames += 1;
        summary.wall += sample.wall;
        summary.wall_max = summary.wall_max.max(sample.wall);
        if let Some(cpu) = sample.cpu {
            summary.cpu_frames += 1;
            summary.cpu += cpu;
            summary.cpu_max = summary.cpu_max.max(cpu);
            summary.paired_wall += sample.wall;
        }
        summary.dimensions = sample.dimensions;
        summary.geometry_changes += u64::from(sample.changes.geometry);
        summary.scale_changes += u64::from(sample.changes.scale);
        summary.opacity_changes += u64::from(sample.changes.opacity);
        summary.visibility_changes += u64::from(sample.changes.visibility);
        if now.saturating_duration_since(self.started) < Duration::from_secs(1) { return None; }
        self.started = now;
        Some(std::mem::take(&mut self.summary))
    }
}

thread_local! {
    static COMMIT_DIAGNOSTICS: std::cell::RefCell<Option<CommitWindow>> = const { std::cell::RefCell::new(None) };
}

fn record_commit_sample(sample: CommitSample) {
    let now = Instant::now();
    let summary = COMMIT_DIAGNOSTICS.with(|slot| {
        slot.borrow_mut().get_or_insert_with(|| CommitWindow::new(now)).record(sample, now)
    });
    let Some(summary) = summary else { return; };
    let cpu_average = if summary.cpu_frames == 0 { f64::NAN }
        else { summary.cpu.as_secs_f64() * 1000.0 / summary.cpu_frames as f64 };
    let unaccounted = if summary.cpu_frames == 0 { f64::NAN }
        else { summary.paired_wall.saturating_sub(summary.cpu).as_secs_f64() * 1000.0 / summary.cpu_frames as f64 };
    eprintln!("media framebuffer commit size={}x{} frames={} wall_avg_ms={:.3} wall_max_ms={:.3} main_cpu_avg_ms={:.3} main_cpu_max_ms={:.3} wall_minus_cpu_avg_ms={:.3} cpu_samples={} cpu_unavailable={} dimension_changes={} geometry_changes={} scale_changes={} opacity_changes={} visibility_changes={}",
        summary.dimensions.0, summary.dimensions.1, summary.frames,
        summary.wall.as_secs_f64() * 1000.0 / summary.frames as f64,
        summary.wall_max.as_secs_f64() * 1000.0, cpu_average,
        if summary.cpu_frames == 0 { f64::NAN } else { summary.cpu_max.as_secs_f64() * 1000.0 },
        unaccounted, summary.cpu_frames, summary.frames - summary.cpu_frames,
        summary.dimension_changes, summary.geometry_changes, summary.scale_changes,
        summary.opacity_changes, summary.visibility_changes);
}

#[cfg(test)]
mod commit_diagnostic_tests {
    use super::*;

    #[test]
    fn commit_window_is_bounded_and_reports_at_most_once_per_second() {
        let now = Instant::now();
        let mut window = CommitWindow::new(now);
        for frame in 0..100 {
            assert!(window.record(CommitSample {
                wall: Duration::from_millis(24), cpu: Some(Duration::from_millis(3)),
                dimensions: (2560, 1800), changes: CommitChanges::default(),
            }, now + Duration::from_millis(frame)).is_none());
        }
        let summary = window.record(CommitSample {
            wall: Duration::from_millis(32), cpu: None, dimensions: (3840, 2346),
            changes: CommitChanges { geometry: true, scale: true, opacity: true, visibility: true },
        }, now + Duration::from_secs(1)).unwrap();
        assert_eq!((summary.frames, summary.cpu_frames), (101, 100));
        assert_eq!(summary.wall, Duration::from_millis(2432));
        assert_eq!(summary.wall_max, Duration::from_millis(32));
        assert_eq!(summary.cpu, Duration::from_millis(300));
        assert_eq!(summary.cpu_max, Duration::from_millis(3));
        assert_eq!(summary.paired_wall.saturating_sub(summary.cpu), Duration::from_millis(2100));
        assert_eq!(summary.dimensions, (3840, 2346));
        assert_eq!(summary.dimension_changes, 1);
        assert_eq!((summary.geometry_changes, summary.scale_changes, summary.opacity_changes, summary.visibility_changes), (1, 1, 1, 1));
        assert_eq!(window.summary.frames, 0);
        assert!(window.record(CommitSample {
            wall: Duration::from_millis(1), cpu: None, dimensions: (3840, 2346),
            changes: CommitChanges::default(),
        }, now + Duration::from_millis(1999)).is_none());
    }

    #[test]
    fn commit_window_does_not_invent_cpu_samples_when_clock_is_unavailable() {
        let now = Instant::now();
        let mut window = CommitWindow::new(now);
        let summary = window.record(CommitSample {
            wall: Duration::from_millis(24), cpu: None, dimensions: (2560, 1800),
            changes: CommitChanges::default(),
        }, now + Duration::from_secs(1)).unwrap();
        assert_eq!(summary.cpu_frames, 0);
        assert_eq!(summary.cpu, Duration::ZERO);
        assert_eq!(summary.paired_wall, Duration::ZERO);
    }

    #[test]
    fn commit_thread_cpu_clock_has_valid_monotonic_samples() {
        #[cfg(target_os = "macos")]
        {
            let before = commit_thread_cpu_time().expect("Darwin thread CPU clock");
            for value in 0..1000 { std::hint::black_box(value); }
            let after = commit_thread_cpu_time().expect("Darwin thread CPU clock");
            assert!(after >= before);
        }
        #[cfg(not(target_os = "macos"))]
        assert!(commit_thread_cpu_time().is_none());
    }
}

fn set_layer_frame(layer: &CALayer, frame: CGRect) -> bool {
    if layer.frame() != frame {
        layer.setFrame(frame);
        return true;
    }
    false
}

fn set_layer_hidden(layer: &CALayer, hidden: bool) -> bool {
    if layer.isHidden() != hidden {
        layer.setHidden(hidden);
        return true;
    }
    false
}

pub(super) struct VideoLayer {
    root: Retained<CALayer>,
    framebuffer_layer: Retained<CALayer>,
    layer: Retained<CALayer>,
    foreground_layer: Retained<CALayer>,
    color_space: CFRetained<CGColorSpace>,
    last_pixels: Option<Arc<Vec<u8>>>,
    last_tint: Option<[u8; 4]>,
    last_foreground: Option<Arc<Vec<u8>>>,
}

impl VideoLayer {
    pub(super) fn new(window: &Window) -> Option<Self> {
        let _main_thread = MainThreadMarker::new()?;
        let handle = window.window_handle().ok()?;
        let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
            return None;
        };
        let view: &NSObject = unsafe { handle.ns_view.cast().as_ref() };
        let root: Retained<CALayer> = unsafe { msg_send![view, layer] };
        let framebuffer_layer = CALayer::new();
        framebuffer_layer.setAnchorPoint(CGPoint::new(0.0, 0.0));
        framebuffer_layer.setGeometryFlipped(true);
        framebuffer_layer.setContentsGravity(unsafe { kCAGravityResize });
        framebuffer_layer.setHidden(true);
        root.addSublayer(&framebuffer_layer);
        let layer = CALayer::new();
        layer.setAnchorPoint(CGPoint::new(0.0, 0.0));
        layer.setMasksToBounds(true);
        layer.setHidden(true);
        root.addSublayer(&layer);
        let foreground_layer = CALayer::new();
        foreground_layer.setAnchorPoint(CGPoint::new(0.0, 0.0));
        foreground_layer.setContentsGravity(unsafe { kCAGravityResize });
        foreground_layer.setHidden(true);
        root.addSublayer(&foreground_layer);
        Some(Self {
            root,
            framebuffer_layer,
            layer,
            foreground_layer,
            color_space: presentation_color_space()?,
            last_pixels: None,
            last_tint: None,
            last_foreground: None,
        })
    }

    pub(super) fn present(&mut self, frame: Option<&VideoLayerFrame>) {
        let Some(frame) = frame.filter(|frame| {
            frame.source_width > 0
                && frame.source_height > 0
                && frame.width > 0.0
                && frame.height > 0.0
                && u64::from(frame.source_width) * u64::from(frame.source_height) * 4
                    == frame.rgba.len() as u64
        }) else {
            set_layer_hidden(&self.layer, true);
            set_layer_hidden(&self.foreground_layer, true);
            self.last_pixels = None;
            self.last_tint = None;
            self.last_foreground = None;
            return;
        };

        CATransaction::begin();
        CATransaction::setDisableActions(true);
        let y = layer_y_for_window_y(
            self.root.bounds().size.height,
            frame.y as f64,
            frame.height as f64,
            self.root.isGeometryFlipped(),
        );
        set_layer_frame(&self.layer, CGRect::new(
            CGPoint::new(frame.x as f64, y),
            CGSize::new(frame.width as f64, frame.height as f64),
        ));
        if self.layer.cornerRadius() != frame.corner_radius as f64 {
            self.layer.setCornerRadius(frame.corner_radius as f64);
        }
        let (x, y, width, height) = frame.contents_rect();
        self.layer.setContentsRect(CGRect::new(CGPoint::new(x, y), CGSize::new(width, height)));
        self.layer.setContentsGravity(unsafe { kCAGravityResize });
        if self.last_tint != frame.tint
            || !self.last_pixels.as_ref().is_some_and(|last| Arc::ptr_eq(last, &frame.rgba))
        {
            let _span = crate::profile::span(crate::profile::Phase::VideoLayerPresent);
            let pixels = tinted_video_pixels(&frame.rgba, frame.tint);
            if let Some(image) = self.image_for_pixels(&pixels, frame.source_width, frame.source_height) {
                let opaque = CGImage::alpha_info(Some(&image)).0 == CGImageAlphaInfo::NoneSkipLast.0;
                if self.layer.isOpaque() != opaque {
                    self.layer.setOpaque(opaque);
                }
                unsafe { self.layer.setContents(Some(image.as_ref() as &AnyObject)) };
                self.last_pixels = Some(frame.rgba.clone());
                self.last_tint = frame.tint;
            }
        }
        set_layer_hidden(&self.layer, false);
        if let Some(foreground) = &frame.foreground {
            let y = layer_y_for_window_y(
                self.root.bounds().size.height,
                foreground.y as f64,
                foreground.height as f64,
                self.root.isGeometryFlipped(),
            );
            set_layer_frame(&self.foreground_layer, CGRect::new(
                CGPoint::new(foreground.x as f64, y),
                CGSize::new(foreground.width as f64, foreground.height as f64),
            ));
            if !self.last_foreground.as_ref().is_some_and(|last| Arc::ptr_eq(last, &foreground.rgba)) {
                if let Some(image) = self.image_for_pixels(&foreground.rgba, foreground.source_width, foreground.source_height) {
                    unsafe { self.foreground_layer.setContents(Some(image.as_ref() as &AnyObject)) };
                    self.last_foreground = Some(foreground.rgba.clone());
                }
            }
            set_layer_hidden(&self.foreground_layer, false);
        } else {
            set_layer_hidden(&self.foreground_layer, true);
            self.last_foreground = None;
        }
        CATransaction::commit();
    }

    pub(super) fn present_framebuffer(&self, pixels: &Arc<tiny_skia::Pixmap>) -> bool {
        let image = {
            let _span = crate::profile::span(crate::profile::Phase::PlatformImageCreate);
            self.image_for_data(pixels, pixels.data(), pixels.width(), pixels.height())
        };
        let Some(image) = image else {
            set_layer_hidden(&self.framebuffer_layer, true);
            return false;
        };
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        let geometry_changed = set_layer_frame(&self.framebuffer_layer, self.root.bounds());
        let scale = self.root.contentsScale();
        let scale_changed = self.framebuffer_layer.contentsScale() != scale;
        if scale_changed {
            self.framebuffer_layer.setContentsScale(scale);
        }
        let opacity_changed;
        {
            let _span = crate::profile::span(crate::profile::Phase::PlatformUpload);
            let opaque = CGImage::alpha_info(Some(&image)).0 == CGImageAlphaInfo::NoneSkipLast.0;
            opacity_changed = self.framebuffer_layer.isOpaque() != opaque;
            if opacity_changed {
                self.framebuffer_layer.setOpaque(opaque);
            }
            unsafe { self.framebuffer_layer.setContents(Some(image.as_ref() as &AnyObject)) };
        }
        let visibility_changed = set_layer_hidden(&self.framebuffer_layer, false);
        let diagnostics = crate::profile::media_enabled() && MainThreadMarker::new().is_some();
        let cpu_start = diagnostics.then(commit_thread_cpu_time).flatten();
        let sample;
        {
            let _span = crate::profile::span(crate::profile::Phase::PlatformCommit);
            let wall_start = diagnostics.then(Instant::now);
            CATransaction::commit();
            let wall = wall_start.map(|start| start.elapsed());
            let cpu = if diagnostics {
                cpu_start.zip(commit_thread_cpu_time()).and_then(|(start, end)| end.checked_sub(start))
            } else { None };
            sample = wall.map(|wall| CommitSample { wall, cpu,
                dimensions: (pixels.width(), pixels.height()), changes: CommitChanges {
                    geometry: geometry_changed, scale: scale_changed,
                    opacity: opacity_changed, visibility: visibility_changed,
                } });
        }
        if let Some(sample) = sample { record_commit_sample(sample); }
        true
    }

    fn image_for_pixels(&self, pixels: &Arc<Vec<u8>>, width: u32, height: u32) -> Option<CFRetained<CGImage>> {
        self.image_for_data(pixels, pixels.as_slice(), width, height)
    }

    fn image_for_data<T: Send + Sync + 'static>(&self, owner: &Arc<T>, pixels: &[u8], width: u32, height: u32) -> Option<CFRetained<CGImage>> {
        unsafe extern "C-unwind" fn release<T>(
            info: *mut c_void,
            _data: NonNull<c_void>,
            _size: usize,
        ) {
            drop(unsafe { Arc::<T>::from_raw(info.cast()) });
        }
        let retained = Arc::into_raw(owner.clone()) as *mut c_void;
        let provider = unsafe {
            CGDataProvider::with_data(
                retained,
                pixels.as_ptr().cast_mut().cast(),
                pixels.len(),
                Some(release::<T>),
            )
        };
        let Some(provider) = provider else {
            drop(unsafe { Arc::<T>::from_raw(retained.cast()) });
            return None;
        };
        let alpha = if crate::renderer::display_list_replay::rgba_is_opaque(pixels) {
            CGImageAlphaInfo::NoneSkipLast
        } else {
            CGImageAlphaInfo::PremultipliedLast
        };
        let bitmap_info = CGBitmapInfo(
            alpha.0
                | CGImageComponentInfo::Integer.0
                | CGImageByteOrderInfo::Order32Big.0
                | CGImagePixelFormatInfo::Packed.0,
        );
        unsafe {
            CGImage::new(
                width as usize,
                height as usize,
                8,
                32,
                width as usize * 4,
                Some(&self.color_space),
                bitmap_info,
                Some(&provider),
                std::ptr::null(),
                false,
                CGColorRenderingIntent::RenderingIntentDefault,
            )
        }
    }
}

fn presentation_color_space() -> Option<CFRetained<CGColorSpace>> {
    // The CPU framebuffer and decoded RGBA samples are sRGB, not monitor RGB.
    CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB }))
}

fn layer_y_for_window_y(root_height: f64, y: f64, height: f64, flipped: bool) -> f64 {
    if flipped {
        y
    } else {
        root_height - y - height
    }
}

#[test]
fn native_video_frame_stays_below_browser_chrome_in_either_coordinate_system() {
    assert_eq!(layer_y_for_window_y(1080.0, 80.0, 1000.0, true), 80.0);
    assert_eq!(layer_y_for_window_y(1080.0, 80.0, 1000.0, false), 0.0);
}

#[test]
fn presentation_images_retain_pixels_and_preserve_transparency() {
    let layer = VideoLayer {
        root: CALayer::new(), framebuffer_layer: CALayer::new(),
        layer: CALayer::new(), foreground_layer: CALayer::new(),
        color_space: CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB })).unwrap(),
        last_pixels: None, last_tint: None, last_foreground: None,
    };
    for alpha in [255, 128] {
        let pixels = Arc::new([30, 60, 90, alpha].repeat(4));
        let image = layer.image_for_pixels(&pixels, 2, 2).unwrap();
        let expected = if alpha == 255 {
            CGImageAlphaInfo::NoneSkipLast
        } else {
            CGImageAlphaInfo::PremultipliedLast
        };
        assert_eq!(CGImage::alpha_info(Some(&image)).0, expected.0);
        let space = CGImage::color_space(Some(&image)).unwrap();
        let name = CGColorSpace::name(Some(&space)).unwrap();
        assert_eq!(&*name, unsafe { kCGColorSpaceSRGB });
        assert_eq!(Arc::strong_count(&pixels), 2);
        drop(image);
        assert_eq!(Arc::strong_count(&pixels), 1);
    }
}

#[cfg(test)]
mod framebuffer_format_tests {
    use super::*;
    use objc2_core_graphics::{CGBitmapContextCreate, CGContext};

    #[derive(Clone, Copy, Debug)]
    enum Format { Rgba, Bgra }

    fn image(layer: &VideoLayer, rgba: &Arc<Vec<u8>>, width: u32, height: u32,
        format: Format) -> CFRetained<CGImage>
    {
        assert_eq!(rgba.len(), width as usize * height as usize * 4);
        if matches!(format, Format::Rgba) {
            return layer.image_for_pixels(rgba, width, height).unwrap();
        }
        let opaque = crate::renderer::display_list_replay::rgba_is_opaque(rgba);
        let mut bytes = rgba.as_ref().clone();
        for pixel in bytes.chunks_exact_mut(4) { pixel.swap(0, 2); }
        let owner = Arc::new(bytes);
        unsafe extern "C-unwind" fn release(info: *mut c_void, _: NonNull<c_void>, _: usize) {
            drop(unsafe { Arc::<Vec<u8>>::from_raw(info.cast()) });
        }
        let retained = Arc::into_raw(owner.clone()) as *mut c_void;
        let provider = unsafe { CGDataProvider::with_data(retained,
            owner.as_ptr().cast_mut().cast(), owner.len(), Some(release)) };
        let Some(provider) = provider else {
            drop(unsafe { Arc::<Vec<u8>>::from_raw(retained.cast()) });
            panic!("BGRA provider creation failed");
        };
        let alpha = if opaque { CGImageAlphaInfo::NoneSkipFirst }
            else { CGImageAlphaInfo::PremultipliedFirst };
        unsafe { CGImage::new(width as usize, height as usize, 8, 32, width as usize * 4,
            Some(&layer.color_space), CGBitmapInfo(alpha.0
                | CGImageByteOrderInfo::Order32Little.0
                | CGImageComponentInfo::Integer.0 | CGImagePixelFormatInfo::Packed.0),
            Some(&provider), std::ptr::null(), false,
            CGColorRenderingIntent::RenderingIntentDefault) }.unwrap()
    }

    fn test_layer() -> VideoLayer {
        VideoLayer {
            root: CALayer::new(), framebuffer_layer: CALayer::new(),
            layer: CALayer::new(), foreground_layer: CALayer::new(),
            color_space: presentation_color_space().unwrap(),
            last_pixels: None, last_tint: None, last_foreground: None,
        }
    }

    // Readback is rendered output, not the CGDataProvider's unchanged source bytes.
    // CALayer readback is still offscreen and is not an onscreen parity claim.
    fn render(layer: &VideoLayer, image: &CGImage, via_layer: bool) -> Vec<u8> {
        let mut output = vec![0; 4 * 4];
        let context = unsafe { CGBitmapContextCreate(output.as_mut_ptr().cast(),
            4, 1, 8, 16, Some(&layer.color_space),
            CGImageAlphaInfo::PremultipliedLast.0 | CGImageByteOrderInfo::Order32Big.0) }.unwrap();
        let rect = CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(4.0, 1.0));
        CGContext::set_rgb_fill_color(Some(&context), 0.0, 1.0, 0.0, 1.0);
        CGContext::fill_rect(Some(&context), rect);
        if via_layer {
            CATransaction::begin();
            CATransaction::setDisableActions(true);
            layer.framebuffer_layer.setFrame(rect);
            layer.framebuffer_layer.setContentsScale(1.0);
            layer.framebuffer_layer.setContentsGravity(unsafe { kCAGravityResize });
            unsafe { layer.framebuffer_layer.setContents(Some(image.as_ref() as &AnyObject)) };
            CATransaction::commit();
            layer.framebuffer_layer.renderInContext(&context);
        } else {
            CGContext::draw_image(Some(&context), rect, Some(image));
        }
        drop(context);
        output
    }

    #[test]
    fn framebuffer_rgba_bgra_rendered_color_and_alpha_parity() {
        let layer = test_layer();
        for rgba in [
            vec![255, 0, 0, 255, 0, 0, 255, 255, 0, 255, 0, 255, 255, 255, 255, 255],
            vec![255, 0, 0, 255, 0, 0, 255, 255, 128, 0, 0, 128, 0, 0, 0, 0],
        ] {
            let source = Arc::new(rgba);
            let a = image(&layer, &source, 4, 1, Format::Rgba);
            let b = image(&layer, &source, 4, 1, Format::Bgra);
            let reference = render(&layer, &a, false);
            assert_eq!(reference, render(&layer, &b, false));
            assert_eq!(&reference[..8], &[255, 0, 0, 255, 0, 0, 255, 255]);
            assert!(reference.chunks_exact(4).all(|pixel| pixel[3] == 255));
            if source[15] == 0 {
                assert_eq!(&reference[12..], &[0, 255, 0, 255]);
                assert!((127..=129).contains(&reference[8]));
                assert!((126..=128).contains(&reference[9]));
            }
        }
    }

    #[test]
    #[ignore = "coordinated offscreen CoreAnimation gate; not onscreen pixel evidence"]
    fn framebuffer_rgba_bgra_layer_render_parity() {
        let layer = test_layer();
        let source = Arc::new(vec![255, 0, 0, 255, 0, 0, 255, 255,
            128, 0, 0, 128, 0, 0, 0, 0]);
        let a = image(&layer, &source, 4, 1, Format::Rgba);
        let b = image(&layer, &source, 4, 1, Format::Bgra);
        assert_eq!(render(&layer, &a, true), render(&layer, &b, true));
    }

    // A coordinated attached-window harness must capture actual presented pixels
    // outside this timing interval. Do not substitute renderInContext for that gate.
    #[allow(dead_code)]
    fn present_sample(layer: &VideoLayer, rgba: &Arc<Vec<u8>>, width: u32, height: u32,
        format: Format) -> (Duration, Duration, Option<Duration>)
    {
        let total_start = Instant::now();
        let image = image(layer, rgba, width, height, format);
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        set_layer_frame(&layer.framebuffer_layer, layer.root.bounds());
        let scale = layer.root.contentsScale();
        if layer.framebuffer_layer.contentsScale() != scale {
            layer.framebuffer_layer.setContentsScale(scale);
        }
        let alpha = CGImage::alpha_info(Some(&image));
        let opaque = alpha == CGImageAlphaInfo::NoneSkipLast || alpha == CGImageAlphaInfo::NoneSkipFirst;
        if layer.framebuffer_layer.isOpaque() != opaque { layer.framebuffer_layer.setOpaque(opaque); }
        unsafe { layer.framebuffer_layer.setContents(Some(image.as_ref() as &AnyObject)) };
        set_layer_hidden(&layer.framebuffer_layer, false);
        let cpu_start = commit_thread_cpu_time();
        let commit_start = Instant::now();
        CATransaction::commit();
        let commit_wall = commit_start.elapsed();
        let cpu = cpu_start.zip(commit_thread_cpu_time()).and_then(|(a, b)| b.checked_sub(a));
        (total_start.elapsed(), commit_wall, cpu)
    }
}

impl Drop for VideoLayer {
    fn drop(&mut self) {
        self.framebuffer_layer.removeFromSuperlayer();
        self.layer.removeFromSuperlayer();
        self.foreground_layer.removeFromSuperlayer();
    }
}
