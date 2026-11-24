use crate::capture::{monitor_rects, ScreenRect};
use crate::providers::SnipOutput;
use crate::snip::state::active_monitor_for;
use crate::ui::popup::{POPUP_HEIGHT_MAX_RESULT, POPUP_WIDTH};
use egui::Pos2;
use std::path::Path;
use windows::core::w;
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE,
    DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
};
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, GetSystemMetrics, SM_CXSCREEN, SM_CXVIRTUALSCREEN, SM_CYSCREEN,
    SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
};

const POPUP_HEIGHT_MIN_RESULT: f32 = 136.0;

/// Applies Windows 11 native DWM rounded corners, dark shadow, and clean neutral border.
pub fn apply_popup_dwm_styling() -> bool {
    // SAFETY: Finds our own top-level viewport window and sets standard Windows 11 DWM attributes.
    unsafe {
        if let Ok(hwnd) = FindWindowW(None, w!("Reticle"))
            && !hwnd.is_invalid()
        {
            let corner = DWMWCP_ROUND;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                (&raw const corner).cast(),
                std::mem::size_of_val(&corner) as u32,
            );
            let dark_mode: i32 = 1;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_USE_IMMERSIVE_DARK_MODE,
                (&raw const dark_mode).cast(),
                std::mem::size_of_val(&dark_mode) as u32,
            );
            // Razor-thin matte obsidian rim RGB(42, 42, 42) -> COLORREF 0x002A2A2A
            let border_color: u32 = 0x002A2A2A;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_BORDER_COLOR,
                (&raw const border_color).cast(),
                std::mem::size_of_val(&border_color) as u32,
            );
            return true;
        }
    }
    false
}

pub fn is_short_ocr(raw: &str) -> bool {
    raw.lines().count() <= 2 && raw.chars().count() <= 110
}

pub fn estimate_result_height(output: &SnipOutput, ocr_expanded: bool) -> f32 {
    let mut h = 104.0f32;

    if output.warning.is_some() {
        h += 50.0;
    }

    if !output.primary_text.is_empty() {
        if output.is_markdown {
            let lines = output.primary_text.lines().count().clamp(4, 16);
            h += 48.0 + (lines as f32 * 21.0);
        } else {
            let wrapped: usize = output
                .primary_text
                .lines()
                .map(|l| (l.chars().count() / 42) + 1)
                .sum();
            h += (wrapped.max(1) as f32) * 23.0;
        }
    }

    if let Some(ref raw) = output.original_text {
        if is_short_ocr(raw) {
            let ocr_lines: usize = raw
                .lines()
                .map(|l| (l.chars().count() / 48) + 1)
                .sum();
            h += 18.0 + (ocr_lines.max(1) as f32) * 18.0;
        } else if ocr_expanded {
            let ocr_lines: usize = raw
                .lines()
                .map(|l| (l.chars().count() / 48) + 1)
                .sum::<usize>()
                .min(8);
            h += 34.0 + (ocr_lines.max(1) as f32) * 17.0;
        } else {
            h += 28.0;
        }
    }

    h.clamp(POPUP_HEIGHT_MIN_RESULT, POPUP_HEIGHT_MAX_RESULT)
}

pub fn estimate_ocr_height(text: &str) -> f32 {
    let wrapped: usize = text
        .lines()
        .map(|l| (l.chars().count() / 44) + 1)
        .sum();
    let h = 152.0 + (wrapped.max(2) as f32) * 21.0;
    h.clamp(POPUP_HEIGHT_MIN_RESULT + 24.0, POPUP_HEIGHT_MAX_RESULT)
}

/// Computes the popup top-left in logical points relative to `anchor` and the active monitor:
/// 1) Directly to the RIGHT of the snipped image (top-aligned),
/// 2) To the LEFT of the snipped image (top-aligned) if near the right monitor edge,
/// 3) Docked along the right edge of the active monitor at the snip's vertical position
///    if the snip spans nearly the full width (never dumped at the bottom of the screen).
pub fn compute_popup_pos_in_monitor(
    anchor: Option<ScreenRect>,
    monitor_phys: ScreenRect,
    ppp: f32,
    height: f32,
) -> Pos2 {
    let ppp = ppp.max(1.0);
    let margin = 12.0;
    let gap = 12.0;

    let mon_l = monitor_phys.x as f32 / ppp;
    let mon_t = monitor_phys.y as f32 / ppp;
    let mon_r = monitor_phys.right() as f32 / ppp;
    let mon_b = monitor_phys.bottom() as f32 / ppp;

    let min_x = mon_l + margin;
    let max_x = (mon_r - POPUP_WIDTH - margin).max(min_x);
    let min_y = mon_t + margin;
    let max_y = (mon_b - height - margin).max(min_y);

    let (raw_x, raw_y) = if let Some(rect) = anchor {
        let rx = rect.x as f32 / ppp;
        let ry = rect.y as f32 / ppp;
        let r_right = rect.right() as f32 / ppp;

        let right_x = r_right + gap;
        let left_x = rx - POPUP_WIDTH - gap;

        if right_x + POPUP_WIDTH <= mon_r - margin {
            (right_x, ry)
        } else if left_x >= mon_l + margin {
            (left_x, ry)
        } else {
            // Very wide snip: keep beside the right edge at the snip's vertical level.
            (max_x, ry)
        }
    } else {
        // Standalone system toast (no snip anchor): top-right/bottom-right primary corner.
        (mon_r - POPUP_WIDTH - 20.0, mon_b - height - 62.0)
    };

    Pos2::new(raw_x.clamp(min_x, max_x), raw_y.clamp(min_y, max_y))
}

pub fn compute_popup_pos(ctx: &egui::Context, anchor: Option<ScreenRect>, height: f32) -> Pos2 {
    let ppp = ctx.pixels_per_point().max(1.0);
    // SAFETY: Read-only Win32 virtual screen geometry queries.
    let virtual_bounds = unsafe {
        ScreenRect {
            x: GetSystemMetrics(SM_XVIRTUALSCREEN),
            y: GetSystemMetrics(SM_YVIRTUALSCREEN),
            width: GetSystemMetrics(SM_CXVIRTUALSCREEN),
            height: GetSystemMetrics(SM_CYVIRTUALSCREEN),
        }
    };
    let primary_bounds = unsafe {
        ScreenRect {
            x: 0,
            y: 0,
            width: GetSystemMetrics(SM_CXSCREEN),
            height: GetSystemMetrics(SM_CYSCREEN),
        }
    };

    let monitors = monitor_rects();
    let active_mon = match anchor {
        Some(rect) => active_monitor_for(rect, &monitors, virtual_bounds),
        None => primary_bounds,
    };

    compute_popup_pos_in_monitor(anchor, active_mon, ppp, height)
}

pub fn open_path_in_shell(path: &Path) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;

    if let Err(err) = std::process::Command::new("cmd")
        .args(["/C", "start", "", &path.to_string_lossy()])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
    {
        log::warn!("Failed to open path {}: {err}", path.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn places_popup_to_right_of_snip_when_space_available() {
        let mon = ScreenRect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        };
        let snip = ScreenRect {
            x: 200,
            y: 250,
            width: 600,
            height: 400,
        };
        let pos = compute_popup_pos_in_monitor(Some(snip), mon, 1.0, 260.0);
        assert_eq!(pos, Pos2::new(812.0, 250.0));
    }

    #[test]
    fn flips_popup_to_left_of_snip_when_right_edge_is_tight() {
        let mon = ScreenRect {
            x: 0,
            y: 0,
            width: 1646,
            height: 1029,
        };
        // Matches user's screenshot: center-right snip (x=560..1350) leaving only ~296px on right
        let snip = ScreenRect {
            x: 560,
            y: 280,
            width: 790,
            height: 550,
        };
        let pos = compute_popup_pos_in_monitor(Some(snip), mon, 1.0, 320.0);
        assert_eq!(pos, Pos2::new(560.0 - POPUP_WIDTH - 12.0, 280.0));
    }

    #[test]
    fn keeps_popup_inside_negative_origin_secondary_monitor() {
        let mon = ScreenRect {
            x: -1080,
            y: 0,
            width: 1080,
            height: 1920,
        };
        let snip = ScreenRect {
            x: -1000,
            y: 300,
            width: 400,
            height: 300,
        };
        let pos = compute_popup_pos_in_monitor(Some(snip), mon, 1.0, 260.0);
        assert_eq!(pos, Pos2::new(-588.0, 300.0));
        assert!(pos.x + POPUP_WIDTH <= 0.0);
    }
}
