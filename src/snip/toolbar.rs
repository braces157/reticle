use crate::capture::ScreenRect;
use crate::snip::annotate::{rgba_to_c32, AnnotationTool, StrokeSize, PALETTE};
use crate::snip::state::{active_monitor_for, phys_rect_to_logical, SnipCommand, SnipMode};
use crate::ui::theme;
use egui::{
    Align2, Button, Color32, CornerRadius, CursorIcon, FontId, Frame, Margin, Pos2, Rect, RichText,
    Sense, Shadow, Stroke, StrokeKind, Vec2,
};

pub fn dim_color() -> Color32 {
    Color32::from_rgba_unmultiplied(6, 6, 8, 146)
}
pub const SEL_ACCENT: Color32 = theme::ACCENT_PICK;

pub struct ToolbarViewState {
    pub active_tool: Option<AnnotationTool>,
    pub active_color: [u8; 4],
    pub stroke_size: StrokeSize,
    pub can_undo: bool,
    pub can_redo: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolbarEvent {
    ToggleTool(AnnotationTool),
    SelectColor([u8; 4]),
    SelectSize(StrokeSize),
    Undo,
    Redo,
    Command(SnipCommand),
}

pub fn render_snip_toolbar(ui: &mut egui::Ui, state: &ToolbarViewState) -> Option<ToolbarEvent> {
    let mut event = None;
    Frame::new()
        .fill(Color32::from_rgba_unmultiplied(15, 15, 15, 246))
        .stroke(Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 32)))
        .corner_radius(CornerRadius::same(15))
        .inner_margin(Margin::symmetric(9, 6))
        .shadow(Shadow {
            offset: [0, 12],
            blur: 28,
            spread: 0,
            color: Color32::from_black_alpha(200),
        })
        .show(ui, |ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(4.0, 5.0);

                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(3.0, 0.0);

                    for (tool, tip) in [
                        (AnnotationTool::Rect, "Rectangle (1)"),
                        (AnnotationTool::Arrow, "Arrow (2)"),
                        (AnnotationTool::Pen, "Freehand Pen (3)"),
                        (AnnotationTool::Highlight, "Highlighter (4)"),
                        (AnnotationTool::Mosaic, "Mosaic Blur / Redact (5)"),
                        (AnnotationTool::Text, "Text Note (6)"),
                    ] {
                        let active = state.active_tool == Some(tool);
                        if icon_tool_btn(ui, tool, active, tip) {
                            event = Some(ToolbarEvent::ToggleTool(tool));
                        }
                    }

                    toolbar_divider(ui);

                    if undo_redo_btn(ui, true, state.can_undo, "Undo (Ctrl+Z)") {
                        event = Some(ToolbarEvent::Undo);
                    }
                    if undo_redo_btn(ui, false, state.can_redo, "Redo (Ctrl+Y)") {
                        event = Some(ToolbarEvent::Redo);
                    }

                    toolbar_divider(ui);

                    if toolbar_btn(ui, "Pin", None, "Pin floating screenshot on top (P)") {
                        event = Some(ToolbarEvent::Command(SnipCommand::PinToScreen));
                    }
                    if toolbar_btn(ui, "Save", None, "Save PNG to Screenshots folder (Ctrl+S)") {
                        event = Some(ToolbarEvent::Command(SnipCommand::SavePng));
                    }
                    if toolbar_btn(ui, "OCR", None, "Extract text to clipboard (O)") {
                        event = Some(ToolbarEvent::Command(SnipCommand::OcrCopy));
                    }
                    if toolbar_btn(ui, "Translate", None, "Translate selection (T)") {
                        event = Some(ToolbarEvent::Command(SnipCommand::Translate));
                    }
                    if toolbar_btn(ui, "Explain", None, "Explain code or content (E)") {
                        event = Some(ToolbarEvent::Command(SnipCommand::ExplainCode));
                    }
                    if toolbar_btn(
                        ui,
                        "Copy",
                        Some(theme::CTA_FILL),
                        "Copy image to clipboard (Enter / Ctrl+C)",
                    ) {
                        event = Some(ToolbarEvent::Command(SnipCommand::CopyImage));
                    }
                    if close_toolbar_btn(ui, "Cancel (Esc)") {
                        event = Some(ToolbarEvent::Command(SnipCommand::Cancel));
                    }
                });

                if let Some(tool) = state.active_tool {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::new(5.0, 0.0);

                        if tool != AnnotationTool::Mosaic {
                            for &col in &PALETTE {
                                let selected = state.active_color == col;
                                if color_swatch_btn(ui, col, selected) {
                                    event = Some(ToolbarEvent::SelectColor(col));
                                }
                            }
                            toolbar_divider(ui);
                        }

                        for (size, label) in [
                            (StrokeSize::Thin, "S"),
                            (StrokeSize::Medium, "M"),
                            (StrokeSize::Thick, "L"),
                        ] {
                            let selected = state.stroke_size == size;
                            let fill = if selected {
                                Some(theme::BG_PILL_ACTIVE)
                            } else {
                                None
                            };
                            if toolbar_btn(ui, label, fill, "Tool size") {
                                event = Some(ToolbarEvent::SelectSize(size));
                            }
                        }

                        ui.add_space(4.0);
                        let hint = match tool {
                            AnnotationTool::Text => "Click inside snip to type · Enter to commit",
                            AnnotationTool::Mosaic => "Drag to pixelate sensitive info",
                            _ => "Drag to draw · Right-click to exit tool",
                        };
                        ui.label(RichText::new(hint).size(11.0).color(theme::TEXT_MUTED));
                    });
                }
            });
        });
    event
}

fn icon_tool_btn(ui: &mut egui::Ui, tool: AnnotationTool, active: bool, tooltip: &str) -> bool {
    let size = Vec2::new(26.0, 26.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let hovered = response.hovered();
    let bg = if active {
        theme::BG_PILL_ACTIVE
    } else if hovered {
        theme::BG_PILL_HOVER
    } else {
        Color32::TRANSPARENT
    };
    let fg = if active || hovered {
        theme::TEXT_PRIMARY
    } else {
        theme::TEXT_SECONDARY
    };
    let painter = ui.painter();
    if bg != Color32::TRANSPARENT {
        painter.rect_filled(rect, CornerRadius::same(13), bg);
    }
    if active {
        painter.rect_stroke(
            rect,
            CornerRadius::same(13),
            Stroke::new(1.0, theme::BORDER_HIGHLIGHT),
            StrokeKind::Inside,
        );
    }
    if hovered {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }

    let c = rect.center();
    let stroke = Stroke::new(1.6, fg);
    match tool {
        AnnotationTool::Rect => {
            let r = Rect::from_center_size(c, Vec2::new(12.0, 9.0));
            painter.rect_stroke(r, 1.0, stroke, StrokeKind::Middle);
        }
        AnnotationTool::Arrow => {
            let a = c + Vec2::new(-5.0, 5.0);
            let b = c + Vec2::new(5.0, -5.0);
            painter.line_segment([a, b], stroke);
            painter.line_segment([b, b + Vec2::new(-5.0, 0.5)], stroke);
            painter.line_segment([b, b + Vec2::new(-0.5, 5.0)], stroke);
        }
        AnnotationTool::Pen => {
            painter.line_segment([c + Vec2::new(-5.0, 4.0), c + Vec2::new(-1.0, -2.0)], stroke);
            painter.line_segment([c + Vec2::new(-1.0, -2.0), c + Vec2::new(2.0, 2.0)], stroke);
            painter.line_segment([c + Vec2::new(2.0, 2.0), c + Vec2::new(5.5, -4.0)], stroke);
        }
        AnnotationTool::Highlight => {
            let hl = Color32::from_rgba_unmultiplied(245, 158, 11, 210);
            painter.line_segment(
                [c + Vec2::new(-5.5, 3.0), c + Vec2::new(5.5, -3.0)],
                Stroke::new(4.2, hl),
            );
        }
        AnnotationTool::Mosaic => {
            let s = 3.2;
            for gy in 0..3 {
                for gx in 0..3 {
                    let col = if (gx + gy) % 2 == 0 {
                        fg
                    } else {
                        theme::TEXT_MUTED
                    };
                    let cell = Rect::from_min_size(
                        Pos2::new(c.x - 4.8 + gx as f32 * s, c.y - 4.8 + gy as f32 * s),
                        Vec2::splat(s),
                    );
                    painter.rect_filled(cell, 0.0, col);
                }
            }
        }
        AnnotationTool::Text => {
            painter.text(
                c,
                Align2::CENTER_CENTER,
                "T",
                FontId::proportional(13.5),
                fg,
            );
        }
    }

    response.on_hover_text(tooltip).clicked()
}

fn undo_redo_btn(ui: &mut egui::Ui, is_undo: bool, enabled: bool, tooltip: &str) -> bool {
    let size = Vec2::new(24.0, 26.0);
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    let bg = if enabled && response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        theme::BG_PILL_HOVER
    } else {
        Color32::TRANSPARENT
    };
    let fg = if enabled {
        if response.hovered() {
            theme::TEXT_PRIMARY
        } else {
            theme::TEXT_SECONDARY
        }
    } else {
        Color32::from_rgb(72, 72, 72)
    };
    let painter = ui.painter();
    if bg != Color32::TRANSPARENT {
        painter.rect_filled(rect, CornerRadius::same(12), bg);
    }

    let c = rect.center();
    let s = Stroke::new(1.5, fg);
    if is_undo {
        painter.line_segment([c + Vec2::new(-4.5, -1.5), c + Vec2::new(3.5, -1.5)], s);
        painter.line_segment([c + Vec2::new(3.5, -1.5), c + Vec2::new(3.5, 3.5)], s);
        painter.line_segment([c + Vec2::new(-4.5, -1.5), c + Vec2::new(-1.5, -4.5)], s);
        painter.line_segment([c + Vec2::new(-4.5, -1.5), c + Vec2::new(-1.5, 1.5)], s);
    } else {
        painter.line_segment([c + Vec2::new(4.5, -1.5), c + Vec2::new(-3.5, -1.5)], s);
        painter.line_segment([c + Vec2::new(-3.5, -1.5), c + Vec2::new(-3.5, 3.5)], s);
        painter.line_segment([c + Vec2::new(4.5, -1.5), c + Vec2::new(1.5, -4.5)], s);
        painter.line_segment([c + Vec2::new(4.5, -1.5), c + Vec2::new(1.5, 1.5)], s);
    }

    response.on_hover_text(tooltip).clicked() && enabled
}

fn color_swatch_btn(ui: &mut egui::Ui, rgba: [u8; 4], selected: bool) -> bool {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::click());
    let painter = ui.painter();
    let r = if selected { 7.0 } else { 6.0 };
    painter.circle_filled(rect.center(), r, rgba_to_c32(rgba));
    if selected {
        painter.circle_stroke(rect.center(), 8.8, Stroke::new(1.6, Color32::WHITE));
    } else if response.hovered() {
        painter.circle_stroke(rect.center(), 8.0, Stroke::new(1.0, theme::TEXT_SECONDARY));
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    response.clicked()
}

fn close_toolbar_btn(ui: &mut egui::Ui, tooltip: &str) -> bool {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(26.0), Sense::click());
    let hovered = response.hovered();
    let down = response.is_pointer_button_down_on();
    let bg = if down {
        theme::CLOSE_HOVER_BG
    } else if hovered {
        theme::BG_PILL_HOVER
    } else {
        Color32::TRANSPARENT
    };
    let fg = if hovered || down {
        theme::TEXT_PRIMARY
    } else {
        theme::TEXT_MUTED
    };
    let p = ui.painter();
    if bg != Color32::TRANSPARENT {
        p.circle_filled(rect.center(), 12.0, bg);
    }
    let c = rect.center();
    let r = 3.6;
    let s = Stroke::new(1.4, fg);
    p.line_segment([Pos2::new(c.x - r, c.y - r), Pos2::new(c.x + r, c.y + r)], s);
    p.line_segment([Pos2::new(c.x + r, c.y - r), Pos2::new(c.x - r, c.y + r)], s);
    if hovered {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    response.on_hover_text(tooltip).clicked()
}

fn toolbar_btn(ui: &mut egui::Ui, label: &str, fill: Option<Color32>, tooltip: &str) -> bool {
    let is_primary = fill == Some(theme::CTA_FILL);
    let mut btn = Button::new(
        RichText::new(label)
            .size(11.8)
            .color(if is_primary {
                theme::CTA_INK
            } else {
                theme::TEXT_SECONDARY
            }),
    )
    .corner_radius(CornerRadius::same(13))
    .stroke(Stroke::NONE)
    .min_size(Vec2::new(0.0, 26.0));
    if let Some(bg) = fill {
        btn = btn.fill(bg);
    } else {
        btn = btn.fill(Color32::TRANSPARENT);
    }
    let resp = ui.add(btn);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    resp.on_hover_text(tooltip).clicked()
}

fn toolbar_divider(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(5.0, 20.0), Sense::hover());
    ui.painter().line_segment(
        [
            Pos2::new(rect.center().x, rect.top() + 2.0),
            Pos2::new(rect.center().x, rect.bottom() - 2.0),
        ],
        Stroke::new(1.0, theme::BORDER_SUBTLE),
    );
}

pub fn paint_dimmed_surround(painter: &egui::Painter, canvas: Rect, sel: Rect) {
    let top = Rect::from_min_max(canvas.min, Pos2::new(canvas.max.x, sel.top()));
    let bottom = Rect::from_min_max(Pos2::new(canvas.min.x, sel.bottom()), canvas.max);
    let left = Rect::from_min_max(
        Pos2::new(canvas.min.x, sel.top()),
        Pos2::new(sel.left(), sel.bottom()),
    );
    let right = Rect::from_min_max(
        Pos2::new(sel.right(), sel.top()),
        Pos2::new(canvas.max.x, sel.bottom()),
    );

    for r in [top, bottom, left, right] {
        if r.width() > 0.0 && r.height() > 0.0 {
            painter.rect_filled(r, 0.0, dim_color());
        }
    }
}

pub fn paint_dimension_pill(
    painter: &egui::Painter,
    sel_logical: Rect,
    sel_phys: ScreenRect,
    canvas: Rect,
) {
    let text = format!("{} × {}", sel_phys.width, sel_phys.height);
    let font = FontId::monospace(11.5);
    let galley = painter.layout_no_wrap(text, font, theme::TEXT_PRIMARY);
    let pad = Vec2::new(7.0, 3.0);
    let pill_size = galley.size() + pad * 2.0;

    let y = if sel_logical.top() - pill_size.y - 6.0 >= canvas.top() + 4.0 {
        sel_logical.top() - pill_size.y - 6.0
    } else {
        sel_logical.top() + 6.0
    };
    let x = sel_logical.left().clamp(
        canvas.left() + 6.0,
        (canvas.right() - pill_size.x - 6.0).max(canvas.left() + 6.0),
    );
    let pill = Rect::from_min_size(Pos2::new(x, y), pill_size);

    painter.rect_filled(pill, 10.0, Color32::from_rgba_unmultiplied(15, 15, 15, 240));
    painter.rect_stroke(
        pill,
        10.0,
        Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 30)),
        StrokeKind::Inside,
    );
    painter.galley(pill.min + pad, galley, theme::TEXT_PRIMARY);
}

pub fn paint_idle_hint(
    painter: &egui::Painter,
    canvas: Rect,
    monitors: &[ScreenRect],
    bounds: ScreenRect,
    cursor_phys: Option<(i32, i32)>,
    mode: SnipMode,
) {
    let hint = match mode {
        SnipMode::Interactive => "DRAG TO SNIP  ·  ESC OR RIGHT-CLICK TO CANCEL",
        SnipMode::QuickTranslate => "QUICK TRANSLATE  ·  DRAG A REGION  ·  ESC TO CANCEL",
    };

    let target_mon = if let Some((x, y)) = cursor_phys {
        let probe = ScreenRect {
            x,
            y,
            width: 1,
            height: 1,
        };
        active_monitor_for(probe, monitors, bounds)
    } else {
        monitors.first().copied().unwrap_or(bounds)
    };
    let mon_log = phys_rect_to_logical(target_mon, canvas, bounds);

    let font = FontId::proportional(11.2);
    let galley = painter.layout_no_wrap(hint.to_string(), font, theme::TEXT_SECONDARY);
    let pad = Vec2::new(16.0, 7.0);
    let size = galley.size() + pad * 2.0;
    let center = Pos2::new(mon_log.center().x, mon_log.top() + 34.0);
    let pill = Rect::from_center_size(center, size);

    painter.rect_filled(pill, 14.0, Color32::from_rgba_unmultiplied(15, 15, 15, 242));
    painter.rect_stroke(
        pill,
        14.0,
        Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 30)),
        StrokeKind::Inside,
    );
    painter.text(
        pill.center(),
        Align2::CENTER_CENTER,
        hint,
        FontId::proportional(11.2),
        theme::TEXT_SECONDARY,
    );
}
