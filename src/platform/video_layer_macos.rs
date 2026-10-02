use super::{VideoLayerFrame, tinted_video_pixels};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{MainThreadMarker, msg_send};
use objc2_core_foundation::{CFRetained, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{
    CGBitmapInfo, CGColorRenderingIntent, CGColorSpace, CGDataProvider, CGImage, CGImageAlphaInfo,
    CGImageByteOrderInfo, CGImageComponentInfo, CGImagePixelFormatInfo,
};
use objc2_foundation::NSObject;
use objc2_quartz_core::{
    CALayer, CATransaction, kCAGravityResize, kCAGravityResizeAspectFill,
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::Arc;
use winit::window::Window;

pub(super) struct VideoLayer {
    root: Retained<CALayer>,
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
            layer,
            foreground_layer,
            color_space: CGColorSpace::new_device_rgb()?,
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
            self.layer.setHidden(true);
            self.foreground_layer.setHidden(true);
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
        self.layer.setFrame(CGRect::new(
            CGPoint::new(frame.x as f64, y),
            CGSize::new(frame.width as f64, frame.height as f64),
        ));
        self.layer.setCornerRadius(frame.corner_radius as f64);
        self.layer.setContentsGravity(unsafe {
            if frame.cover {
                kCAGravityResizeAspectFill
            } else {
                kCAGravityResize
            }
        });
        if self.last_tint != frame.tint
            || !self.last_pixels.as_ref().is_some_and(|last| Arc::ptr_eq(last, &frame.rgba))
        {
            let _span = crate::profile::span(crate::profile::Phase::VideoLayerPresent);
            let pixels = tinted_video_pixels(&frame.rgba, frame.tint);
            if let Some(image) = self.image_for_pixels(&pixels, frame.source_width, frame.source_height) {
                unsafe { self.layer.setContents(Some(image.as_ref() as &AnyObject)) };
                self.last_pixels = Some(frame.rgba.clone());
                self.last_tint = frame.tint;
            }
        }
        self.layer.setHidden(false);
        if let Some(foreground) = &frame.foreground {
            let y = layer_y_for_window_y(
                self.root.bounds().size.height,
                foreground.y as f64,
                foreground.height as f64,
                self.root.isGeometryFlipped(),
            );
            self.foreground_layer.setFrame(CGRect::new(
                CGPoint::new(foreground.x as f64, y),
                CGSize::new(foreground.width as f64, foreground.height as f64),
            ));
            if !self.last_foreground.as_ref().is_some_and(|last| Arc::ptr_eq(last, &foreground.rgba)) {
                if let Some(image) = self.image_for_pixels(&foreground.rgba, foreground.source_width, foreground.source_height) {
                    unsafe { self.foreground_layer.setContents(Some(image.as_ref() as &AnyObject)) };
                    self.last_foreground = Some(foreground.rgba.clone());
                }
            }
            self.foreground_layer.setHidden(false);
        } else {
            self.foreground_layer.setHidden(true);
            self.last_foreground = None;
        }
        CATransaction::commit();
    }

    fn image_for_pixels(&self, pixels: &Arc<Vec<u8>>, width: u32, height: u32) -> Option<CFRetained<CGImage>> {
        unsafe extern "C-unwind" fn release(
            info: *mut c_void,
            _data: NonNull<c_void>,
            _size: usize,
        ) {
            drop(unsafe { Arc::<Vec<u8>>::from_raw(info.cast()) });
        }
        let retained = Arc::into_raw(pixels.clone()) as *mut c_void;
        let provider = unsafe {
            CGDataProvider::with_data(
                retained,
                pixels.as_ptr().cast_mut().cast(),
                pixels.len(),
                Some(release),
            )
        };
        let Some(provider) = provider else {
            drop(unsafe { Arc::<Vec<u8>>::from_raw(retained.cast()) });
            return None;
        };
        let bitmap_info = CGBitmapInfo(
            CGImageAlphaInfo::PremultipliedLast.0
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

impl Drop for VideoLayer {
    fn drop(&mut self) {
        self.layer.removeFromSuperlayer();
        self.foreground_layer.removeFromSuperlayer();
    }
}
