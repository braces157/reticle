use crate::capture::{ScreenRect, VirtualScreenCapture};
use crate::snip::state::{phys_rect_to_logical, ScreenPt};
use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, Vec2};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnnotationTool {
    Rect,
    Arrow,
    Pen,
    Highlight,
    Mosaic,
    Text,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrokeSize {
    Thin,
    Medium,
    Thick,
}

impl StrokeSize {
    pub fn stroke_px(self) -> f32 {
        match self {
            Self::Thin => 2.5,
            Self::Medium => 4.5,
            Self::Thick => 8.0,
        }
    }

    pub fn highlight_px(self) -> f32 {
        match self {
            Self::Thin => 14.0,
            Self::Medium => 22.0,
            Self::Thick => 32.0,
        }
    }

    pub fn mosaic_block(self) -> u32 {
        match self {
            Self::Thin => 8,
            Self::Medium => 12,
            Self::Thick => 18,
        }
    }

    pub fn font_px(self) -> f32 {
        match self {
            Self::Thin => 16.0,
            Self::Medium => 22.0,
            Self::Thick => 30.0,
        }
    }
}

pub const PALETTE: [[u8; 4]; 5] = [
    [239, 68, 68, 255],  // Red
    [245, 158, 11, 255], // Amber
    [34, 197, 94, 255],  // Emerald
    [56, 189, 248, 255], // Sky
    [250, 250, 250, 255], // White
];

#[derive(Debug, Clone, PartialEq)]
pub enum AnnotationOp {
    Rect {
        from: ScreenPt,
        to: ScreenPt,
        color: [u8; 4],
        stroke_px: f32,
    },
    Arrow {
        from: ScreenPt,
        to: ScreenPt,
        color: [u8; 4],
        stroke_px: f32,
    },
    Freehand {
        points: Vec<ScreenPt>,
        color: [u8; 4],
        stroke_px: f32,
        highlighter: bool,
    },
    Mosaic {
        rect: ScreenRect,
        block_size: u32,
    },
    Text {
        pos: ScreenPt,
        content: String,
        color: [u8; 4],
        font_px: f32,
    },
}

#[derive(Debug, Clone)]
pub struct DraftAnnotation {
    pub op: AnnotationOp,
    anchor: ScreenPt,
}

impl DraftAnnotation {
    pub fn begin(
        tool: AnnotationTool,
        pt: ScreenPt,
        color: [u8; 4],
        size: StrokeSize,
    ) -> Option<Self> {
        let op = match tool {
            AnnotationTool::Rect => AnnotationOp::Rect {
                from: pt,
                to: pt,
                color,
                stroke_px: size.stroke_px(),
            },
            AnnotationTool::Arrow => AnnotationOp::Arrow {
                from: pt,
                to: pt,
                color,
                stroke_px: size.stroke_px(),
            },
            AnnotationTool::Pen => AnnotationOp::Freehand {
                points: vec![pt],
                color,
                stroke_px: size.stroke_px(),
                highlighter: false,
            },
            AnnotationTool::Highlight => AnnotationOp::Freehand {
                points: vec![pt],
                color,
                stroke_px: size.highlight_px(),
                highlighter: true,
            },
            AnnotationTool::Mosaic => AnnotationOp::Mosaic {
                rect: ScreenRect::from_points(pt.x, pt.y, pt.x, pt.y),
                block_size: size.mosaic_block(),
            },
            AnnotationTool::Text => return None,
        };
        Some(Self { op, anchor: pt })
    }

    pub fn update(&mut self, pt: ScreenPt) {
        match &mut self.op {
            AnnotationOp::Rect { to, .. } | AnnotationOp::Arrow { to, .. } => *to = pt,
            AnnotationOp::Freehand { points, .. } => {
                if points
                    .last()
                    .is_none_or(|last| (last.x - pt.x).abs() + (last.y - pt.y).abs() >= 2)
                {
                    points.push(pt);
                }
            }
            AnnotationOp::Mosaic { rect, .. } => {
                *rect = ScreenRect::from_points(self.anchor.x, self.anchor.y, pt.x, pt.y);
            }
            AnnotationOp::Text { .. } => {}
        }
    }

    pub fn is_meaningful(&self) -> bool {
        match &self.op {
            AnnotationOp::Rect { from, to, .. } | AnnotationOp::Arrow { from, to, .. } => {
                (from.x - to.x).abs() >= 3 || (from.y - to.y).abs() >= 3
            }
            AnnotationOp::Freehand { points, .. } => !points.is_empty(),
            AnnotationOp::Mosaic { rect, .. } => rect.width >= 4 && rect.height >= 4,
            AnnotationOp::Text { content, .. } => !content.trim().is_empty(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct AnnotationDoc {
    pub ops: Vec<AnnotationOp>,
    undone: Vec<AnnotationOp>,
}

impl AnnotationDoc {
    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    pub fn can_undo(&self) -> bool {
        !self.ops.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.undone.is_empty()
    }

    pub fn push(&mut self, op: AnnotationOp) {
        self.ops.push(op);
        self.undone.clear();
    }

    pub fn undo(&mut self) -> bool {
        if let Some(op) = self.ops.pop() {
            self.undone.push(op);
            true
        } else {
            false
        }
    }

    pub fn redo(&mut self) -> bool {
        if let Some(op) = self.undone.pop() {
            self.ops.push(op);
            true
        } else {
            false
        }
    }
}

/// Converts a single physical virtual-screen point to logical egui canvas coordinates.
pub fn phys_pt_to_logical(pt: ScreenPt, canvas: Rect, bounds: ScreenRect) -> Pos2 {
    if bounds.width <= 0 || bounds.height <= 0 {
        return canvas.min;
    }
    let sx = canvas.width() / bounds.width as f32;
    let sy = canvas.height() / bounds.height as f32;
    Pos2::new(
        canvas.min.x + (pt.x - bounds.x) as f32 * sx,
        canvas.min.y + (pt.y - bounds.y) as f32 * sy,
    )
}

/// Renders an annotation op live onto the frozen overlay canvas (clipped to `sel_logical`).
pub fn paint_op(
    painter: &Painter,
    op: &AnnotationOp,
    capture: &VirtualScreenCapture,
    canvas: Rect,
    bounds: ScreenRect,
) {
    let scale = if bounds.width > 0 {
        canvas.width() / bounds.width as f32
    } else {
        1.0
    };

    match op {
        AnnotationOp::Rect {
            from,
            to,
            color,
            stroke_px,
        } => {
            let p1 = phys_pt_to_logical(*from, canvas, bounds);
            let p2 = phys_pt_to_logical(*to, canvas, bounds);
            let rect = Rect::from_two_pos(p1, p2);
            painter.rect_stroke(
                rect,
                0.0,
                Stroke::new((stroke_px * scale).max(1.0), rgba_to_c32(*color)),
                StrokeKind::Middle,
            );
        }
        AnnotationOp::Arrow {
            from,
            to,
            color,
            stroke_px,
        } => {
            let p1 = phys_pt_to_logical(*from, canvas, bounds);
            let p2 = phys_pt_to_logical(*to, canvas, bounds);
            paint_arrow(painter, p1, p2, rgba_to_c32(*color), (stroke_px * scale).max(1.5));
        }
        AnnotationOp::Freehand {
            points,
            color,
            stroke_px,
            highlighter,
        } => {
            if points.is_empty() {
                return;
            }
            let c = if *highlighter {
                Color32::from_rgba_unmultiplied(color[0], color[1], color[2], 96)
            } else {
                rgba_to_c32(*color)
            };
            let width = (stroke_px * scale).max(1.5);
            let pts: Vec<Pos2> = points
                .iter()
                .map(|p| phys_pt_to_logical(*p, canvas, bounds))
                .collect();
            if pts.len() == 1 {
                painter.circle_filled(pts[0], width * 0.5, c);
            } else {
                painter.add(Shape::line(pts, Stroke::new(width, c)));
            }
        }
        AnnotationOp::Mosaic { rect, block_size } => {
            paint_live_mosaic(painter, capture, *rect, *block_size, canvas, bounds);
        }
        AnnotationOp::Text {
            pos,
            content,
            color,
            font_px,
        } => {
            if content.is_empty() {
                return;
            }
            let p = phys_pt_to_logical(*pos, canvas, bounds);
            let font = FontId::proportional((font_px * scale).max(10.0));
            // Subtle dark shadow for readability on bright backgrounds
            painter.text(
                p + Vec2::new(1.0, 1.0),
                Align2::LEFT_TOP,
                content,
                font.clone(),
                Color32::from_black_alpha(150),
            );
            painter.text(p, Align2::LEFT_TOP, content, font, rgba_to_c32(*color));
        }
    }
}

pub fn rgba_to_c32(c: [u8; 4]) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3])
}

fn paint_arrow(painter: &Painter, from: Pos2, to: Pos2, color: Color32, width: f32) {
    let dir = to - from;
    let len = dir.length();
    if len < 2.0 {
        return;
    }
    let u = dir / len;
    let perp = Vec2::new(-u.y, u.x);
    let head_len = (width * 3.6).clamp(11.0, 32.0).min(len * 0.65);
    let head_half_w = head_len * 0.58;

    let shaft_end = to - u * (head_len * 0.72);
    painter.line_segment([from, shaft_end], Stroke::new(width, color));

    let base = to - u * head_len;
    let left = base + perp * head_half_w;
    let right = base - perp * head_half_w;
    painter.add(Shape::convex_polygon(
        vec![to, left, right],
        color,
        Stroke::NONE,
    ));
}

fn paint_live_mosaic(
    painter: &Painter,
    capture: &VirtualScreenCapture,
    rect: ScreenRect,
    block_size: u32,
    canvas: Rect,
    bounds: ScreenRect,
) {
    let clamped = rect.clamp_to(bounds);
    if clamped.width <= 0 || clamped.height <= 0 {
        return;
    }
    let step = block_size.max(4) as i32;
    let full_w = capture.width as usize;

    let mut y = clamped.y;
    while y < clamped.bottom() {
        let bh = (clamped.bottom() - y).min(step);
        let mut x = clamped.x;
        while x < clamped.right() {
            let bw = (clamped.right() - x).min(step);
            let sample_x = (x + bw / 2 - bounds.x).clamp(0, capture.width - 1) as usize;
            let sample_y = (y + bh / 2 - bounds.y).clamp(0, capture.height - 1) as usize;
            let idx = (sample_y * full_w + sample_x) * 4;
            let color = if idx + 3 < capture.bgra.len() {
                Color32::from_rgb(
                    capture.bgra[idx + 2],
                    capture.bgra[idx + 1],
                    capture.bgra[idx],
                )
            } else {
                Color32::GRAY
            };
            let cell = ScreenRect {
                x,
                y,
                width: bw,
                height: bh,
            };
            let cell_log = phys_rect_to_logical(cell, canvas, bounds);
            painter.rect_filled(cell_log, 0.0, color);
            x += step;
        }
        y += step;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undo_redo_stack_clears_redo_on_new_push() {
        let mut doc = AnnotationDoc::default();
        assert!(!doc.can_undo());
        assert!(!doc.can_redo());

        doc.push(AnnotationOp::Rect {
            from: ScreenPt { x: 0, y: 0 },
            to: ScreenPt { x: 10, y: 10 },
            color: PALETTE[0],
            stroke_px: 3.0,
        });
        doc.push(AnnotationOp::Arrow {
            from: ScreenPt { x: 2, y: 2 },
            to: ScreenPt { x: 20, y: 20 },
            color: PALETTE[1],
            stroke_px: 4.0,
        });
        assert_eq!(doc.ops.len(), 2);

        assert!(doc.undo());
        assert_eq!(doc.ops.len(), 1);
        assert!(doc.can_redo());

        assert!(doc.redo());
        assert_eq!(doc.ops.len(), 2);
        assert!(!doc.can_redo());

        assert!(doc.undo());
        doc.push(AnnotationOp::Mosaic {
            rect: ScreenRect {
                x: 0,
                y: 0,
                width: 12,
                height: 12,
            },
            block_size: 8,
        });
        assert!(!doc.can_redo());
        assert_eq!(doc.ops.len(), 2);
    }

    #[test]
    fn mosaic_draft_resizes_reversibly_around_anchor() {
        let anchor = ScreenPt { x: 100, y: 100 };
        let mut draft =
            DraftAnnotation::begin(AnnotationTool::Mosaic, anchor, PALETTE[0], StrokeSize::Medium)
                .expect("mosaic draft");

        draft.update(ScreenPt { x: 220, y: 180 });
        assert!(matches!(
            draft.op,
            AnnotationOp::Mosaic {
                rect: ScreenRect { x: 100, y: 100, width: 120, height: 80 },
                ..
            }
        ));

        // Drag back past anchor (up-left) -> must shrink & flip cleanly instead of staying 120x80
        draft.update(ScreenPt { x: 60, y: 70 });
        assert!(matches!(
            draft.op,
            AnnotationOp::Mosaic {
                rect: ScreenRect { x: 60, y: 70, width: 40, height: 30 },
                ..
            }
        ));
    }
}
