pub mod screen;

pub use screen::{
    capture_virtual_screen, encode_rgba_to_png, monitor_rects, CapturedRegion, CroppedImage,
    ScreenRect, VirtualScreenCapture, MIN_SNIP_SIZE,
};
