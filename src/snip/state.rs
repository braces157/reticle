use crate::capture::{ScreenRect, MIN_SNIP_SIZE};
use egui::{Pos2, Rect, Vec2};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenPt {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnipMode {
    /// Unified Snipaste workflow: drag -> adjust handles -> floating toolbar.
    Interactive,
    /// Optional 1-step shortcut: drag & release immediately triggers Translate.
    QuickTranslate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnipCommand {
    CopyImage,
    SavePng,
    PinToScreen,
    OcrCopy,
    Translate,
    ExplainCode,
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizeHandle {
    Nw,
    N,
    Ne,
    E,
    Se,
    S,
    Sw,
    W,
}

impl ResizeHandle {
    pub const ALL: [ResizeHandle; 8] = [
        Self::Nw,
        Self::N,
        Self::Ne,
        Self::E,
        Self::Se,
        Self::S,
        Self::Sw,
        Self::W,
    ];

    pub fn anchor_pos(self, rect: Rect) -> Pos2 {
        match self {
            Self::Nw => rect.left_top(),
            Self::N => Pos2::new(rect.center().x, rect.top()),
            Self::Ne => rect.right_top(),
            Self::E => Pos2::new(rect.right(), rect.center().y),
            Self::Se => rect.right_bottom(),
            Self::S => Pos2::new(rect.center().x, rect.bottom()),
            Self::Sw => rect.left_bottom(),
            Self::W => Pos2::new(rect.left(), rect.center().y),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragMode {
    Move {
        grab_x: i32,
        grab_y: i32,
        original: ScreenRect,
    },
    Resize {
        handle: ResizeHandle,
        original: ScreenRect,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayStage {
    Idle,
    Selecting {
        anchor: ScreenPt,
        current: ScreenPt,
    },
    Adjusting {
        rect: ScreenRect,
        drag: Option<DragMode>,
    },
}

impl OverlayStage {
    pub fn selection_rect(&self) -> Option<ScreenRect> {
        match *self {
            Self::Idle => None,
            Self::Selecting { anchor, current } => {
                Some(ScreenRect::from_points(anchor.x, anchor.y, current.x, current.y))
            }
            Self::Adjusting { rect, .. } => Some(rect),
        }
    }

    pub fn is_adjusting(&self) -> bool {
        matches!(self, Self::Adjusting { .. })
    }
}

/// Converts a logical egui canvas point to physical virtual-screen coordinates.
pub fn logical_to_phys(pos: Pos2, canvas: Rect, bounds: ScreenRect) -> ScreenPt {
    if canvas.width() <= 0.0 || canvas.height() <= 0.0 {
        return ScreenPt {
            x: bounds.x,
            y: bounds.y,
        };
    }
    let nx = ((pos.x - canvas.min.x) / canvas.width()).clamp(0.0, 1.0);
    let ny = ((pos.y - canvas.min.y) / canvas.height()).clamp(0.0, 1.0);
    ScreenPt {
        x: bounds.x + (nx * bounds.width as f32).round() as i32,
        y: bounds.y + (ny * bounds.height as f32).round() as i32,
    }
}

/// Converts a physical virtual-screen rectangle into logical egui canvas coordinates.
pub fn phys_rect_to_logical(rect: ScreenRect, canvas: Rect, bounds: ScreenRect) -> Rect {
    if bounds.width <= 0 || bounds.height <= 0 {
        return Rect::NOTHING;
    }
    let sx = canvas.width() / bounds.width as f32;
    let sy = canvas.height() / bounds.height as f32;
    let min_x = canvas.min.x + (rect.x - bounds.x) as f32 * sx;
    let min_y = canvas.min.y + (rect.y - bounds.y) as f32 * sy;
    let max_x = canvas.min.x + (rect.right() - bounds.x) as f32 * sx;
    let max_y = canvas.min.y + (rect.bottom() - bounds.y) as f32 * sy;
    Rect::from_min_max(Pos2::new(min_x, min_y), Pos2::new(max_x, max_y))
}

/// Hit-tests the 8 resize handles of a logical selection rect within `hit_radius` logical px.
pub fn hit_test_handle(pointer: Pos2, sel_logical: Rect, hit_radius: f32) -> Option<ResizeHandle> {
    let mut best: Option<(ResizeHandle, f32)> = None;
    for handle in ResizeHandle::ALL {
        let dist = pointer.distance(handle.anchor_pos(sel_logical));
        if dist <= hit_radius && best.is_none_or(|(_, d)| dist < d) {
            best = Some((handle, dist));
        }
    }
    best.map(|(h, _)| h)
}

/// Applies a move or resize drag in physical coordinates, clamping to `bounds` and preserving `MIN_SNIP_SIZE`.
pub fn apply_drag(drag: DragMode, cursor: ScreenPt, bounds: ScreenRect) -> ScreenRect {
    match drag {
        DragMode::Move {
            grab_x,
            grab_y,
            original,
        } => {
            let max_x = (bounds.right() - original.width).max(bounds.x);
            let max_y = (bounds.bottom() - original.height).max(bounds.y);
            let new_x = (cursor.x - grab_x).clamp(bounds.x, max_x);
            let new_y = (cursor.y - grab_y).clamp(bounds.y, max_y);
            ScreenRect {
                x: new_x,
                y: new_y,
                width: original.width,
                height: original.height,
            }
        }
        DragMode::Resize { handle, original } => {
            let mut left = original.x;
            let mut top = original.y;
            let mut right = original.right();
            let mut bottom = original.bottom();
            let cx = cursor.x.clamp(bounds.x, bounds.right());
            let cy = cursor.y.clamp(bounds.y, bounds.bottom());

            match handle {
                ResizeHandle::Nw => {
                    left = cx.min(right - MIN_SNIP_SIZE);
                    top = cy.min(bottom - MIN_SNIP_SIZE);
                }
                ResizeHandle::N => {
                    top = cy.min(bottom - MIN_SNIP_SIZE);
                }
                ResizeHandle::Ne => {
                    right = cx.max(left + MIN_SNIP_SIZE);
                    top = cy.min(bottom - MIN_SNIP_SIZE);
                }
                ResizeHandle::E => {
                    right = cx.max(left + MIN_SNIP_SIZE);
                }
                ResizeHandle::Se => {
                    right = cx.max(left + MIN_SNIP_SIZE);
                    bottom = cy.max(top + MIN_SNIP_SIZE);
                }
                ResizeHandle::S => {
                    bottom = cy.max(top + MIN_SNIP_SIZE);
                }
                ResizeHandle::Sw => {
                    left = cx.min(right - MIN_SNIP_SIZE);
                    bottom = cy.max(top + MIN_SNIP_SIZE);
                }
                ResizeHandle::W => {
                    left = cx.min(right - MIN_SNIP_SIZE);
                }
            }

            ScreenRect {
                x: left,
                y: top,
                width: (right - left).max(MIN_SNIP_SIZE),
                height: (bottom - top).max(MIN_SNIP_SIZE),
            }
            .clamp_to(bounds)
        }
    }
}

/// Finds the monitor containing the selection's center (or falls back to full virtual bounds).
pub fn active_monitor_for(
    selection: ScreenRect,
    monitors: &[ScreenRect],
    fallback: ScreenRect,
) -> ScreenRect {
    let cx = selection.x + selection.width / 2;
    let cy = selection.y + selection.height / 2;
    monitors
        .iter()
        .copied()
        .find(|m| cx >= m.x && cx < m.right() && cy >= m.y && cy < m.bottom())
        .unwrap_or(fallback)
}

/// Computes floating toolbar top-left in logical canvas coordinates:
/// 1) Below selection right-aligned,
/// 2) Flipped above selection if clipped at monitor bottom,
/// 3) Docked inside bottom-right if selection fills the monitor vertically.
///
/// Always clamped inside `monitor_logical` with `margin` padding.
pub fn compute_toolbar_pos(
    sel_logical: Rect,
    toolbar_size: Vec2,
    monitor_logical: Rect,
) -> Pos2 {
    let gap = 8.0;
    let margin = 10.0;

    let below_y = sel_logical.bottom() + gap;
    let above_y = sel_logical.top() - gap - toolbar_size.y;

    let y = if below_y + toolbar_size.y <= monitor_logical.bottom() - margin {
        below_y
    } else if above_y >= monitor_logical.top() + margin {
        above_y
    } else {
        (sel_logical.bottom() - toolbar_size.y - gap)
            .clamp(monitor_logical.top() + margin, (monitor_logical.bottom() - toolbar_size.y - margin).max(monitor_logical.top() + margin))
    };

    let preferred_x = sel_logical.right() - toolbar_size.x;
    let min_x = monitor_logical.left() + margin;
    let max_x = (monitor_logical.right() - toolbar_size.x - margin).max(min_x);
    let x = preferred_x.clamp(min_x, max_x);

    Pos2::new(x, y)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bounds_1080p() -> ScreenRect {
        ScreenRect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        }
    }

    #[test]
    fn logical_and_physical_roundtrip_handles_negative_multi_monitor_and_hidpi() {
        let bounds = ScreenRect {
            x: -1920,
            y: 0,
            width: 3840,
            height: 1080,
        };
        // Simulate 150% DPI canvas
        let canvas = Rect::from_min_size(Pos2::ZERO, Vec2::new(2560.0, 720.0));
        let pt = logical_to_phys(Pos2::new(640.0, 360.0), canvas, bounds);
        assert_eq!(pt, ScreenPt { x: -960, y: 540 });

        let sel = ScreenRect {
            x: -960,
            y: 270,
            width: 960,
            height: 540,
        };
        let logical = phys_rect_to_logical(sel, canvas, bounds);
        assert!((logical.min.x - 640.0).abs() < 0.01);
        assert!((logical.min.y - 180.0).abs() < 0.01);
        assert!((logical.width() - 640.0).abs() < 0.01);
        assert!((logical.height() - 360.0).abs() < 0.01);
    }

    #[test]
    fn move_drag_clamps_to_virtual_bounds_preserving_size() {
        let bounds = bounds_1080p();
        let orig = ScreenRect {
            x: 100,
            y: 100,
            width: 300,
            height: 200,
        };
        let drag = DragMode::Move {
            grab_x: 50,
            grab_y: 50,
            original: orig,
        };
        let moved = apply_drag(drag, ScreenPt { x: 2500, y: -200 }, bounds);
        assert_eq!(
            moved,
            ScreenRect {
                x: 1620,
                y: 0,
                width: 300,
                height: 200
            }
        );
    }

    #[test]
    fn resize_drag_enforces_minimum_snip_size() {
        let bounds = bounds_1080p();
        let orig = ScreenRect {
            x: 200,
            y: 200,
            width: 100,
            height: 100,
        };
        let drag = DragMode::Resize {
            handle: ResizeHandle::Nw,
            original: orig,
        };
        // Drag past bottom-right corner
        let resized = apply_drag(drag, ScreenPt { x: 500, y: 500 }, bounds);
        assert_eq!(resized.width, MIN_SNIP_SIZE);
        assert_eq!(resized.height, MIN_SNIP_SIZE);
        assert_eq!(resized.right(), 300);
        assert_eq!(resized.bottom(), 300);
    }

    #[test]
    fn toolbar_flips_above_when_near_bottom_and_docks_inside_for_fullscreen() {
        let monitor = Rect::from_min_size(Pos2::ZERO, Vec2::new(1920.0, 1080.0));
        let bar = Vec2::new(340.0, 38.0);

        // Normal middle selection -> sits below
        let mid = Rect::from_min_size(Pos2::new(400.0, 300.0), Vec2::new(400.0, 200.0));
        let pos_mid = compute_toolbar_pos(mid, bar, monitor);
        assert_eq!(pos_mid.y, 508.0);
        assert_eq!(pos_mid.x, 460.0);

        // Bottom edge selection -> flips above
        let bottom = Rect::from_min_size(Pos2::new(10.0, 900.0), Vec2::new(200.0, 175.0));
        let pos_bot = compute_toolbar_pos(bottom, bar, monitor);
        assert_eq!(pos_bot.y, 900.0 - 8.0 - 38.0);
        assert_eq!(pos_bot.x, 10.0); // Clamped to monitor left margin

        // Fullscreen selection -> docks inside bottom-right
        let full = monitor;
        let pos_full = compute_toolbar_pos(full, bar, monitor);
        assert!(pos_full.y >= 10.0 && pos_full.y + bar.y <= 1070.0);
        assert!(pos_full.x >= 10.0 && pos_full.x + bar.x <= 1910.0);
    }
}
