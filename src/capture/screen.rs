use image::codecs::png::PngEncoder;
use image::{ExtendedColorType, ImageEncoder};
use thiserror::Error;
use windows::core::BOOL;
use windows::Win32::Foundation::{LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject,
    EnumDisplayMonitors, GetDC, GetDIBits, GetMonitorInfoW, ReleaseDC, SelectObject, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, CAPTUREBLT, DIB_RGB_COLORS, HBITMAP, HDC, HGDIOBJ, HMONITOR,
    MONITORINFO, SRCCOPY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
};

/// Minimum valid snip dimension in physical pixels (ignores accidental tiny drags).
pub const MIN_SNIP_SIZE: i32 = 8;

#[derive(Debug, Error)]
pub enum CaptureError {
    #[error("virtual screen has invalid dimensions ({width}x{height})")]
    InvalidScreenBounds { width: i32, height: i32 },
    #[error("Win32 GDI call failed ({call}): {detail}")]
    GdiFailed { call: &'static str, detail: String },
    #[error("selection too small ({width}x{height} px; minimum {MIN_SNIP_SIZE}x{MIN_SNIP_SIZE} px)")]
    SelectionTooSmall { width: i32, height: i32 },
    #[error("failed to encode PNG: {0}")]
    PngEncode(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl ScreenRect {
    pub fn from_points(x1: i32, y1: i32, x2: i32, y2: i32) -> Self {
        let left = x1.min(x2);
        let top = y1.min(y2);
        let right = x1.max(x2);
        let bottom = y1.max(y2);
        Self {
            x: left,
            y: top,
            width: right - left,
            height: bottom - top,
        }
    }

    pub fn is_valid_snip(&self) -> bool {
        self.width >= MIN_SNIP_SIZE && self.height >= MIN_SNIP_SIZE
    }

    pub fn right(&self) -> i32 {
        self.x + self.width
    }

    pub fn bottom(&self) -> i32 {
        self.y + self.height
    }

    pub fn clamp_to(&self, bounds: ScreenRect) -> Self {
        let left = self.x.max(bounds.x).min(bounds.right());
        let top = self.y.max(bounds.y).min(bounds.bottom());
        let right = self.right().max(bounds.x).min(bounds.right());
        let bottom = self.bottom().max(bounds.y).min(bounds.bottom());
        Self {
            x: left,
            y: top,
            width: (right - left).max(0),
            height: (bottom - top).max(0),
        }
    }
}

#[derive(Debug, Clone)]
pub struct VirtualScreenCapture {
    pub origin_x: i32,
    pub origin_y: i32,
    pub width: i32,
    pub height: i32,
    /// Top-down 32-bit BGRA pixels (4 bytes per pixel).
    pub bgra: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct CroppedImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub screen_rect: ScreenRect,
}

impl CroppedImage {
    pub fn to_captured_region(&self) -> Result<CapturedRegion, CaptureError> {
        let png_bytes = encode_rgba_to_png(self.width, self.height, &self.rgba)?;
        Ok(CapturedRegion {
            png_bytes,
            screen_rect: self.screen_rect,
        })
    }
}

pub fn encode_rgba_to_png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, CaptureError> {
    let mut png_bytes = Vec::with_capacity((width as usize) * (height as usize));
    let encoder = PngEncoder::new(&mut png_bytes);
    encoder
        .write_image(rgba, width, height, ExtendedColorType::Rgba8)
        .map_err(|e| CaptureError::PngEncode(e.to_string()))?;
    Ok(png_bytes)
}

#[derive(Debug, Clone)]
pub struct CapturedRegion {
    pub png_bytes: Vec<u8>,
    pub screen_rect: ScreenRect,
}

impl VirtualScreenCapture {
    pub fn bounds(&self) -> ScreenRect {
        ScreenRect {
            x: self.origin_x,
            y: self.origin_y,
            width: self.width,
            height: self.height,
        }
    }

    /// Converts the full virtual desktop capture from BGRA to opaque RGBA for GPU upload.
    pub fn to_rgba(&self) -> Vec<u8> {
        let mut rgba = vec![0u8; self.bgra.len()];
        for (src, dst) in self.bgra.chunks_exact(4).zip(rgba.chunks_exact_mut(4)) {
            dst[0] = src[2];
            dst[1] = src[1];
            dst[2] = src[0];
            dst[3] = 255;
        }
        rgba
    }

    /// Crops a rectangle in virtual-screen coordinates into opaque RGBA pixels.
    /// Rejects selections smaller than `MIN_SNIP_SIZE`x`MIN_SNIP_SIZE`.
    pub fn crop_to_rgba(&self, screen_rect: ScreenRect) -> Result<CroppedImage, CaptureError> {
        let clamped = screen_rect.clamp_to(self.bounds());
        if !clamped.is_valid_snip() {
            return Err(CaptureError::SelectionTooSmall {
                width: clamped.width,
                height: clamped.height,
            });
        }

        let rel_x = (clamped.x - self.origin_x) as usize;
        let rel_y = (clamped.y - self.origin_y) as usize;
        let crop_w = clamped.width as usize;
        let crop_h = clamped.height as usize;
        let full_w = self.width as usize;

        let mut rgba = Vec::with_capacity(crop_w * crop_h * 4);
        for row in 0..crop_h {
            let src_row_start = ((rel_y + row) * full_w + rel_x) * 4;
            let src_slice = &self.bgra[src_row_start..src_row_start + crop_w * 4];
            for px in src_slice.chunks_exact(4) {
                rgba.push(px[2]); // R
                rgba.push(px[1]); // G
                rgba.push(px[0]); // B
                rgba.push(255); // Opaque alpha for screenshots
            }
        }

        Ok(CroppedImage {
            width: crop_w as u32,
            height: crop_h as u32,
            rgba,
            screen_rect: clamped,
        })
    }

    /// Crop a rectangle in virtual-screen coordinates into PNG bytes.
    #[cfg(test)]
    pub fn crop_to_png(&self, screen_rect: ScreenRect) -> Result<CapturedRegion, CaptureError> {
        self.crop_to_rgba(screen_rect)?.to_captured_region()
    }
}

/// Enumerates all active display monitors in physical virtual-screen coordinates.
pub fn monitor_rects() -> Vec<ScreenRect> {
    unsafe extern "system" fn enum_proc(
        monitor: HMONITOR,
        _dc: HDC,
        _rect: *mut RECT,
        data: LPARAM,
    ) -> BOOL {
        // SAFETY: `data` points to a valid `Vec<ScreenRect>` alive for the duration of EnumDisplayMonitors.
        let out = unsafe { &mut *(data.0 as *mut Vec<ScreenRect>) };
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        // SAFETY: `info.cbSize` is initialized and `monitor` is supplied by Windows.
        if unsafe { GetMonitorInfoW(monitor, &mut info).as_bool() } {
            let r = info.rcMonitor;
            let w = r.right - r.left;
            let h = r.bottom - r.top;
            if w > 0 && h > 0 {
                out.push(ScreenRect {
                    x: r.left,
                    y: r.top,
                    width: w,
                    height: h,
                });
            }
        }
        BOOL(1)
    }

    let mut rects = Vec::new();
    // SAFETY: Standard read-only Win32 monitor enumeration with a stack-owned Vec pointer.
    unsafe {
        let _ = EnumDisplayMonitors(
            None,
            None,
            Some(enum_proc),
            LPARAM((&raw mut rects) as isize),
        );
    }
    rects
}

struct ScreenDcGuard(HDC);
impl Drop for ScreenDcGuard {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            // SAFETY: self.0 was acquired from GetDC(None) and has not been released yet.
            unsafe {
                ReleaseDC(None, self.0);
            }
        }
    }
}

struct MemDcGuard(HDC);
impl Drop for MemDcGuard {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            // SAFETY: self.0 was created by CreateCompatibleDC and is owned exclusively by this guard.
            unsafe {
                let _ = DeleteDC(self.0);
            }
        }
    }
}

struct BitmapGuard(HBITMAP);
impl Drop for BitmapGuard {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            // SAFETY: self.0 is an owned HBITMAP that has been deselected from any DC.
            unsafe {
                let _ = DeleteObject(HGDIOBJ(self.0.0));
            }
        }
    }
}

/// Capture the full multi-monitor virtual desktop via Win32 `BitBlt`.
pub fn capture_virtual_screen() -> Result<VirtualScreenCapture, CaptureError> {
    // SAFETY: GetSystemMetrics is thread-safe and reads OS virtual desktop geometry.
    let (origin_x, origin_y, width, height) = unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN),
            GetSystemMetrics(SM_CYVIRTUALSCREEN),
        )
    };

    if width <= 0 || height <= 0 {
        return Err(CaptureError::InvalidScreenBounds { width, height });
    }

    // SAFETY: All GDI handles are checked for validity and released by RAII guards on every exit path.
    unsafe {
        let screen_dc = GetDC(None);
        if screen_dc.is_invalid() {
            return Err(CaptureError::GdiFailed {
                call: "GetDC",
                detail: std::io::Error::last_os_error().to_string(),
            });
        }
        let screen_dc_guard = ScreenDcGuard(screen_dc);

        let mem_dc = CreateCompatibleDC(Some(screen_dc_guard.0));
        if mem_dc.is_invalid() {
            return Err(CaptureError::GdiFailed {
                call: "CreateCompatibleDC",
                detail: std::io::Error::last_os_error().to_string(),
            });
        }
        let mem_dc_guard = MemDcGuard(mem_dc);

        let bitmap = CreateCompatibleBitmap(screen_dc_guard.0, width, height);
        if bitmap.is_invalid() {
            return Err(CaptureError::GdiFailed {
                call: "CreateCompatibleBitmap",
                detail: std::io::Error::last_os_error().to_string(),
            });
        }
        let bitmap_guard = BitmapGuard(bitmap);

        let prev_obj = SelectObject(mem_dc_guard.0, HGDIOBJ(bitmap_guard.0.0));
        let blt_res = BitBlt(
            mem_dc_guard.0,
            0,
            0,
            width,
            height,
            Some(screen_dc_guard.0),
            origin_x,
            origin_y,
            SRCCOPY | CAPTUREBLT,
        )
        .or_else(|_| {
            // Fallback without CAPTUREBLT for drivers/sessions that disallow layered window capture.
            BitBlt(
                mem_dc_guard.0,
                0,
                0,
                width,
                height,
                Some(screen_dc_guard.0),
                origin_x,
                origin_y,
                SRCCOPY,
            )
        });
        if let Err(err) = blt_res {
            let _ = SelectObject(mem_dc_guard.0, prev_obj);
            return Err(CaptureError::GdiFailed {
                call: "BitBlt",
                detail: err.to_string(),
            });
        }

        let mut bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height, // Top-down DIB
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };

        let mut bgra = vec![0u8; (width as usize) * (height as usize) * 4];
        let lines = GetDIBits(
            mem_dc_guard.0,
            bitmap_guard.0,
            0,
            height as u32,
            Some(bgra.as_mut_ptr().cast()),
            &mut bmi,
            DIB_RGB_COLORS,
        );
        let _ = SelectObject(mem_dc_guard.0, prev_obj);

        if lines == 0 {
            return Err(CaptureError::GdiFailed {
                call: "GetDIBits",
                detail: std::io::Error::last_os_error().to_string(),
            });
        }

        Ok(VirtualScreenCapture {
            origin_x,
            origin_y,
            width,
            height,
            bgra,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_from_points_normalizes_all_drag_directions() {
        assert_eq!(
            ScreenRect::from_points(100, 200, 40, 50),
            ScreenRect {
                x: 40,
                y: 50,
                width: 60,
                height: 150
            }
        );
        assert_eq!(
            ScreenRect::from_points(-300, 10, -100, 90),
            ScreenRect {
                x: -300,
                y: 10,
                width: 200,
                height: 80
            }
        );
    }

    #[test]
    fn rejects_tiny_accidental_drags() {
        let capture = VirtualScreenCapture {
            origin_x: 0,
            origin_y: 0,
            width: 32,
            height: 32,
            bgra: vec![255; 32 * 32 * 4],
        };
        let err = capture
            .crop_to_png(ScreenRect {
                x: 5,
                y: 5,
                width: 7,
                height: 12,
            })
            .expect_err("7x12 must be rejected");
        assert!(matches!(err, CaptureError::SelectionTooSmall { .. }));
    }

    #[test]
    fn crops_negative_monitor_origin_and_encodes_valid_png() {
        let w = 20;
        let h = 20;
        let mut bgra = vec![0u8; w * h * 4];
        // Fill with BGR = (10, 20, 30) -> RGB = (30, 20, 10)
        for px in bgra.chunks_exact_mut(4) {
            px[0] = 10;
            px[1] = 20;
            px[2] = 30;
            px[3] = 255;
        }
        let capture = VirtualScreenCapture {
            origin_x: -100,
            origin_y: -50,
            width: w as i32,
            height: h as i32,
            bgra,
        };
        let cropped = capture
            .crop_to_png(ScreenRect {
                x: -95,
                y: -45,
                width: 10,
                height: 12,
            })
            .expect("valid 10x12 crop");
        assert_eq!(cropped.screen_rect.width, 10);
        assert_eq!(cropped.screen_rect.height, 12);

        let decoded = image::load_from_memory(&cropped.png_bytes)
            .expect("valid png")
            .to_rgba8();
        assert_eq!(decoded.dimensions(), (10, 12));
        assert_eq!(decoded.get_pixel(0, 0).0, [30, 20, 10, 255]);
    }
}
