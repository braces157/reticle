use crate::providers::SnipAction;
use crate::ui::popup::{PopupContent, SnipPopupApp, COPY_FEEDBACK_DURATION};
use crate::ui::theme;
use egui::{
    Align, Color32, CornerRadius, CursorIcon, FontId, Frame, Layout, Margin, Panel, Pos2, Rect,
    RichText, ScrollArea, Sense, Stroke, StrokeKind, Vec2, ViewportCommand,
};
use egui_commonmark::CommonMarkViewer;
use std::time::Instant;

const HEADER_HEIGHT: f32 = 40.0;
const FOOTER_HEIGHT: f32 = 44.0;

const QUICK_LANGUAGES: &[(&str, &str)] = &[
    ("en", "English"),
    ("vi", "Vietnamese · Tiếng Việt"),
    ("zh-CN", "Chinese (Simplified) · 中文"),
    ("zh-TW", "Chinese (Traditional) · 繁體"),
    ("ja", "Japanese · 日本語"),
    ("ko", "Korean · 한국어"),
    ("es", "Spanish · Español"),
    ("fr", "French · Français"),
    ("de", "German · Deutsch"),
    ("pt", "Portuguese · Português"),
    ("ru", "Russian · Русский"),
    ("ar", "Arabic · العربية"),
    ("hi", "Hindi · हिन्दी"),
    ("id", "Indonesian · Bahasa"),
    ("th", "Thai · ไทย"),
    ("it", "Italian · Italiano"),
    ("nl", "Dutch · Nederlands"),
    ("pl", "Polish · Polski"),
    ("tr", "Turkish · Türkçe"),
    ("uk", "Ukrainian · Українська"),
    ("cs", "Czech · Čeština"),
    ("sv", "Swedish · Svenska"),
    ("da", "Danish · Dansk"),
    ("fi", "Finnish · Suomi"),
    ("no", "Norwegian · Norsk"),
    ("el", "Greek · Ελληνικά"),
    ("he", "Hebrew · עברית"),
    ("hu", "Hungarian · Magyar"),
    ("ro", "Romanian · Română"),
    ("ms", "Malay · Melayu"),
    ("tl", "Filipino · Tagalog"),
    ("bn", "Bengali · বাংলা"),
];

/// Renders the obsidian toolmaker popover using pinned `Panel::top`, optional `Panel::bottom`,
/// and auto-measured `CentralPanel`.
pub fn render_popup(app: &mut SnipPopupApp, ui: &mut egui::Ui, ctx: &egui::Context) {
    Panel::top("snip_header")
        .exact_size(HEADER_HEIGHT)
        .show_separator_line(false)
        .frame(Frame::NONE.fill(theme::BG_HEADER))
        .show(ui, |ui| {
            render_header(app, ui, ctx);
        });

    let show_footer = matches!(
        app.content,
        PopupContent::Result { .. } | PopupContent::Ocr { .. }
    );
    if show_footer {
        Panel::bottom("snip_footer")
            .exact_size(FOOTER_HEIGHT)
            .show_separator_line(false)
            .frame(Frame::NONE.fill(theme::BG_FOOTER))
            .show(ui, |ui| {
                draw_hairline(ui);
                render_footer(app, ui, ctx);
            });
    }

    let mut measured_body_h: Option<f32> = None;
    egui::CentralPanel::default()
        .frame(Frame::NONE.fill(theme::BG_WINDOW))
        .show(ui, |ui| {
            if matches!(
                app.content,
                PopupContent::Loading { .. }
                    | PopupContent::Toast { .. }
                    | PopupContent::Error { .. }
            ) {
                paint_subtle_dot_grid(ui.painter(), ui.max_rect());
            }
            let is_streaming_bar = matches!(
                app.content,
                PopupContent::Loading { .. }
                    | PopupContent::Result {
                        is_streaming: true,
                        ..
                    }
            );
            if is_streaming_bar {
                render_loading_shimmer(ui);
            } else {
                draw_hairline(ui);
            }
            measured_body_h = render_body(app, ui, ctx);
        });

    if let Some(inner_h) = measured_body_h {
        let chrome_h =
            HEADER_HEIGHT + 2.0 + 30.0 + if show_footer { FOOTER_HEIGHT } else { 0.0 } + 6.0;
        app.sync_measured_height(ctx, chrome_h + inner_h);
    }
}

fn draw_hairline(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, theme::BORDER_HAIRLINE);
}

/// Subtle architectural dot matrix on status/loading surfaces (never competes with foreground ink).
fn paint_subtle_dot_grid(painter: &egui::Painter, rect: Rect) {
    let step = 18.0;
    let mut y = rect.top() + 10.0;
    while y < rect.bottom() - 4.0 {
        let mut x = rect.left() + 10.0;
        while x < rect.right() - 4.0 {
            painter.circle_filled(Pos2::new(x, y), 0.75, theme::DOT_GRID);
            x += step;
        }
        y += step;
    }
}

fn render_loading_shimmer(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.5), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, theme::BORDER_HAIRLINE);
    let t = (ui.ctx().input(|i| i.time) * 1.25).fract() as f32;
    let bar_w = rect.width() * 0.26;
    let start_x = rect.left() - bar_w + (rect.width() + bar_w) * t;
    let bar = Rect::from_min_max(
        Pos2::new(start_x.max(rect.left()), rect.top()),
        Pos2::new((start_x + bar_w).min(rect.right()), rect.bottom()),
    );
    if bar.width() > 0.0 {
        ui.painter().rect_filled(bar, 1.0, theme::SHIMMER_BAR);
    }
}

fn paint_reticle_mark(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(13.0), Sense::hover());
    let p = ui.painter();
    let s = Stroke::new(1.25, theme::TEXT_SECONDARY);
    let arm = 3.2;
    let l = rect.left() + 0.8;
    let r = rect.right() - 0.8;
    let t = rect.top() + 0.8;
    let b = rect.bottom() - 0.8;

    p.line_segment([Pos2::new(l, t + arm), Pos2::new(l, t)], s);
    p.line_segment([Pos2::new(l, t), Pos2::new(l + arm, t)], s);
    p.line_segment([Pos2::new(r - arm, t), Pos2::new(r, t)], s);
    p.line_segment([Pos2::new(r, t), Pos2::new(r, t + arm)], s);
    p.line_segment([Pos2::new(l, b - arm), Pos2::new(l, b)], s);
    p.line_segment([Pos2::new(l, b), Pos2::new(l + arm, b)], s);
    p.line_segment([Pos2::new(r - arm, b), Pos2::new(r, b)], s);
    p.line_segment([Pos2::new(r, b), Pos2::new(r, b - arm)], s);
    p.circle_filled(rect.center(), 1.5, theme::ACCENT_PICK);
}

fn close_icon_button(ui: &mut egui::Ui) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::click());
    let hovered = response.hovered();
    let down = response.is_pointer_button_down_on();
    let bg = if down {
        theme::CLOSE_HOVER_BG
    } else if hovered {
        theme::BG_PILL_HOVER
    } else {
        Color32::TRANSPARENT
    };
    let fg = if down || hovered {
        theme::TEXT_PRIMARY
    } else {
        theme::TEXT_MUTED
    };
    let p = ui.painter();
    if bg != Color32::TRANSPARENT {
        p.circle_filled(rect.center(), 10.5, bg);
        if hovered && !down {
            p.circle_stroke(rect.center(), 10.5, Stroke::new(1.0, theme::BORDER_SUBTLE));
        }
    }
    let c = rect.center();
    let r = 3.4;
    let stroke = Stroke::new(1.35, fg);
    p.line_segment([Pos2::new(c.x - r, c.y - r), Pos2::new(c.x + r, c.y + r)], stroke);
    p.line_segment([Pos2::new(c.x + r, c.y - r), Pos2::new(c.x - r, c.y + r)], stroke);
    if hovered {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    response
}

fn pin_toggle_button(ui: &mut egui::Ui, pinned: bool) -> egui::Response {
    let label = if pinned { "Pinned" } else { "Pin" };
    let font = FontId::proportional(11.2);
    let text_color = if pinned {
        theme::TEXT_PRIMARY
    } else {
        theme::TEXT_MUTED
    };
    let galley = ui.painter().layout_no_wrap(label.to_string(), font, text_color);
    let size = Vec2::new(galley.size().x + 24.0, 22.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let hovered = response.hovered();

    let (fill, stroke, final_ink) = if pinned {
        (
            if hovered {
                theme::BG_PILL_ACTIVE
            } else {
                theme::BG_PILL
            },
            Stroke::new(1.0, theme::BORDER_SUBTLE),
            theme::TEXT_PRIMARY,
        )
    } else if hovered {
        (
            theme::BG_PILL,
            Stroke::new(1.0, theme::BORDER_HAIRLINE),
            theme::TEXT_PRIMARY,
        )
    } else {
        (Color32::TRANSPARENT, Stroke::NONE, theme::TEXT_MUTED)
    };

    let p = ui.painter();
    if fill != Color32::TRANSPARENT {
        p.rect_filled(rect, CornerRadius::same(11), fill);
        p.rect_stroke(rect, CornerRadius::same(11), stroke, StrokeKind::Inside);
    }

    let dot_center = Pos2::new(rect.left() + 9.5, rect.center().y);
    let dot_col = if pinned {
        theme::ACCENT_PICK
    } else if hovered {
        theme::TEXT_SECONDARY
    } else {
        theme::TEXT_MUTED
    };
    p.circle_filled(dot_center, 2.5, dot_col);
    p.galley(
        Pos2::new(rect.left() + 16.5, rect.center().y - galley.size().y * 0.5),
        galley,
        final_ink,
    );

    if hovered {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    response
}

/// Full-capsule button: inverted pure white (`#FFF` / `#0A0A0A`) for primary CTAs,
/// quiet hairline graphite capsule for secondary actions.
fn action_pill_button(ui: &mut egui::Ui, label: &str, primary: bool) -> egui::Response {
    let font = FontId::proportional(11.8);
    let fg = if primary {
        theme::CTA_INK
    } else {
        theme::TEXT_SECONDARY
    };
    let galley = ui.painter().layout_no_wrap(label.to_string(), font, fg);
    let height = 28.0;
    let size = Vec2::new(galley.size().x + 26.0, height);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let hovered = response.hovered();
    let down = response.is_pointer_button_down_on();

    let (fill, stroke, ink) = if primary {
        let bg = if down {
            theme::CTA_FILL_ACTIVE
        } else if hovered {
            theme::CTA_FILL_HOVER
        } else {
            theme::CTA_FILL
        };
        (bg, Stroke::NONE, theme::CTA_INK)
    } else {
        let bg = if down {
            theme::BG_PILL_ACTIVE
        } else if hovered {
            theme::BG_PILL_HOVER
        } else {
            theme::BG_PILL
        };
        let border = if hovered {
            theme::BORDER_SUBTLE
        } else {
            theme::BORDER_HAIRLINE
        };
        let text = if hovered {
            theme::TEXT_PRIMARY
        } else {
            theme::TEXT_SECONDARY
        };
        (bg, Stroke::new(1.0, border), text)
    };

    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(14), fill);
    if stroke != Stroke::NONE {
        p.rect_stroke(rect, CornerRadius::same(14), stroke, StrokeKind::Inside);
    }
    p.galley(
        Pos2::new(
            rect.center().x - galley.size().x * 0.5,
            rect.center().y - galley.size().y * 0.5,
        ),
        galley,
        ink,
    );
    if hovered {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    response
}

fn copied_status_pill(ui: &mut egui::Ui) {
    let font = FontId::proportional(11.8);
    let galley = ui
        .painter()
        .layout_no_wrap("Copied".to_string(), font, theme::OK_EMERALD);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(galley.size().x + 34.0, 28.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(14), theme::OK_EMERALD_BG);
    p.rect_stroke(
        rect,
        CornerRadius::same(14),
        Stroke::new(1.0, theme::OK_EMERALD_BORDER),
        StrokeKind::Inside,
    );
    let c = Pos2::new(rect.left() + 14.0, rect.center().y);
    let s = Stroke::new(1.6, theme::OK_EMERALD);
    p.line_segment([Pos2::new(c.x - 3.6, c.y + 0.2), Pos2::new(c.x - 1.0, c.y + 2.8)], s);
    p.line_segment([Pos2::new(c.x - 1.0, c.y + 2.8), Pos2::new(c.x + 3.8, c.y - 2.8)], s);
    p.galley(
        Pos2::new(rect.left() + 23.0, rect.center().y - galley.size().y * 0.5),
        galley,
        theme::OK_EMERALD,
    );
}

fn paint_status_badge(ui: &mut egui::Ui, is_warning: bool) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(26.0), Sense::hover());
    let p = ui.painter();
    let c = rect.center();
    if is_warning {
        p.circle_filled(c, 12.5, theme::WARN_ICON_BG);
        p.circle_stroke(
            c,
            12.5,
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(224, 154, 18, 72)),
        );
        p.line_segment(
            [Pos2::new(c.x, c.y - 4.6), Pos2::new(c.x, c.y + 1.0)],
            Stroke::new(1.8, theme::WARN_TEXT),
        );
        p.circle_filled(Pos2::new(c.x, c.y + 4.6), 1.2, theme::WARN_TEXT);
    } else {
        p.circle_filled(c, 12.5, theme::OK_EMERALD_BG);
        p.circle_stroke(c, 12.5, Stroke::new(1.0, theme::OK_EMERALD_BORDER));
        let s = Stroke::new(1.7, theme::OK_EMERALD);
        p.line_segment([Pos2::new(c.x - 4.2, c.y + 0.4), Pos2::new(c.x - 1.0, c.y + 3.5)], s);
        p.line_segment([Pos2::new(c.x - 1.0, c.y + 3.5), Pos2::new(c.x + 4.8, c.y - 3.2)], s);
    }
}

fn paint_disclosure_chevron(painter: &egui::Painter, center: Pos2, expanded: bool, color: Color32) {
    let pts = if expanded {
        vec![
            Pos2::new(center.x - 3.3, center.y - 1.8),
            Pos2::new(center.x + 3.3, center.y - 1.8),
            Pos2::new(center.x, center.y + 2.6),
        ]
    } else {
        vec![
            Pos2::new(center.x - 1.8, center.y - 3.3),
            Pos2::new(center.x + 2.6, center.y),
            Pos2::new(center.x - 1.8, center.y + 3.3),
        ]
    };
    painter.add(egui::Shape::convex_polygon(pts, color, Stroke::NONE));
}

fn render_header(app: &mut SnipPopupApp, ui: &mut egui::Ui, ctx: &egui::Context) {
    let detected_lang = match &app.content {
        PopupContent::Result { output, .. } => output.detected_language.clone(),
        _ => None,
    };

    // IMPORTANT: Register window-drag background interaction BEFORE child buttons so
    // interactive controls registered afterwards sit strictly on top in egui hit-testing.
    let drag_rect = ui.max_rect();
    let drag_sense = ui.interact(drag_rect, ui.scope_id().with("header_drag"), Sense::drag());

    let mut chosen_lang: Option<String> = None;
    let mut requested_action: Option<SnipAction> = None;

    Frame::new()
        .inner_margin(Margin::symmetric(14, 7))
        .show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                paint_reticle_mark(ui);
                ui.add_space(4.0);

                if matches!(
                    app.content,
                    PopupContent::Result { .. } | PopupContent::Loading { .. }
                ) {
                    let src = detected_lang
                        .as_deref()
                        .map(str::to_ascii_uppercase)
                        .unwrap_or_else(|| "AUTO".to_string());
                    ui.label(
                        RichText::new(src)
                            .size(11.0)
                            .color(theme::TEXT_TERTIARY),
                    );
                    ui.label(RichText::new("→").size(11.0).color(theme::TEXT_MUTED));

                    let current_tl = app.config.normalized_target_language();
                    egui::ComboBox::from_id_salt("target_lang_picker")
                        .selected_text(
                            RichText::new(current_tl.to_ascii_uppercase())
                                .size(11.0)
                                .color(theme::TEXT_PRIMARY),
                        )
                        .width(54.0)
                        .height(220.0)
                        .show_ui(ui, |ui| {
                            for &(code, name) in QUICK_LANGUAGES {
                                let selected = current_tl.eq_ignore_ascii_case(code);
                                let label = format!("{}  ·  {name}", code.to_ascii_uppercase());
                                if ui.selectable_label(selected, label).clicked() {
                                    chosen_lang = Some(code.to_string());
                                }
                            }
                        });
                } else if let PopupContent::Ocr { ref text } = app.content {
                    let chars = text.chars().count();
                    let lines = text.lines().count();
                    ui.label(
                        RichText::new("OCR")
                            .size(11.5)
                            .strong()
                            .color(theme::TEXT_PRIMARY),
                    );
                    ui.label(
                        RichText::new(format!("·  {lines} LINES  ·  {chars} CHARS"))
                            .size(10.5)
                            .color(theme::TEXT_MUTED),
                    );
                } else {
                    ui.label(
                        RichText::new("Reticle")
                            .size(12.0)
                            .color(theme::TEXT_SECONDARY),
                    );
                }

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;

                    if close_icon_button(ui).on_hover_text("Close (Esc)").clicked() {
                        app.hide_popup(ctx);
                    }

                    if pin_toggle_button(ui, app.pinned)
                        .on_hover_text(if app.pinned {
                            "Unpin popup"
                        } else {
                            "Keep floating on top"
                        })
                        .clicked()
                    {
                        app.pinned = !app.pinned;
                    }

                    if app.config.has_byok_key()
                        && app.last_crop.is_some()
                        && matches!(
                            app.content,
                            PopupContent::Result { .. } | PopupContent::Loading { .. }
                        )
                    {
                        Frame::new()
                            .fill(theme::BG_SEGMENT_TRACK)
                            .stroke(Stroke::new(1.0, theme::BORDER_HAIRLINE))
                            .corner_radius(CornerRadius::same(12))
                            .inner_margin(Margin::same(2))
                            .show(ui, |ui| {
                                ui.spacing_mut().item_spacing.x = 2.0;
                                for (action, label) in [
                                    (SnipAction::ExplainCode, "Explain"),
                                    (SnipAction::Translate, "Translate"),
                                ] {
                                    let selected = app.active_action == action;
                                    let btn = egui::Button::new(
                                        RichText::new(label).size(10.8).color(if selected {
                                            theme::TEXT_PRIMARY
                                        } else {
                                            theme::TEXT_MUTED
                                        }),
                                    )
                                    .fill(if selected {
                                        theme::BG_PILL_ACTIVE
                                    } else {
                                        Color32::TRANSPARENT
                                    })
                                    .stroke(if selected {
                                        Stroke::new(1.0, theme::BORDER_SUBTLE)
                                    } else {
                                        Stroke::NONE
                                    })
                                    .corner_radius(CornerRadius::same(10))
                                    .min_size(Vec2::new(0.0, 18.0));
                                    let resp = ui.add(btn);
                                    if resp.hovered() {
                                        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                                    }
                                    if resp.clicked() && !selected {
                                        requested_action = Some(action);
                                    }
                                }
                            });
                    }
                });
            });
        });

    if drag_sense.drag_started_by(egui::PointerButton::Primary) {
        ctx.send_viewport_cmd(ViewportCommand::StartDrag);
    }

    if let Some(lang) = chosen_lang {
        app.change_target_language(ctx, &lang);
    }
    if let Some(action) = requested_action {
        app.dispatch_processing(ctx, action, false);
    }
}

fn render_body(app: &mut SnipPopupApp, ui: &mut egui::Ui, ctx: &egui::Context) -> Option<f32> {
    let mut measured: Option<f32> = None;
    let mut open_config_clicked = false;
    let mut fallback_translate = false;
    let mut retry_action: Option<SnipAction> = None;
    let mut dismiss_clicked = false;
    let has_last_region = app.last_crop.is_some();
    let active_action = app.active_action;

    Frame::new()
        .inner_margin(Margin::symmetric(16, 15))
        .show(ui, |ui| match &app.content {
            PopupContent::Hidden => {}
            PopupContent::Loading { action, .. } => {
                let r = ui.horizontal(|ui| {
                    let verb = match action {
                        SnipAction::Translate => "Translating…",
                        SnipAction::ExplainCode => "Explaining code…",
                    };
                    ui.label(
                        RichText::new(verb)
                            .size(14.0)
                            .color(theme::TEXT_PRIMARY),
                    );
                });
                measured = Some(r.response.rect.height());
            }
            PopupContent::Toast {
                title,
                message,
                is_warning,
                ..
            } => {
                let r = ui.horizontal_top(|ui| {
                    paint_status_badge(ui, *is_warning);
                    ui.add_space(8.0);
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new(title)
                                .size(13.5)
                                .color(theme::TEXT_PRIMARY),
                        );
                        ui.add_space(3.0);
                        ui.add(
                            egui::Label::new(
                                RichText::new(message)
                                    .size(12.2)
                                    .color(theme::TEXT_TERTIARY),
                            )
                            .wrap(),
                        );
                    });
                });
                measured = Some(r.response.rect.height());
            }
            PopupContent::Error { title, message } => {
                let needs_config = message.to_ascii_lowercase().contains("config.json")
                    || message.to_ascii_lowercase().contains("api key");
                let r = ui.vertical(|ui| {
                    Frame::new()
                        .fill(theme::BG_CARD)
                        .stroke(Stroke::new(1.0, theme::BORDER_HAIRLINE))
                        .corner_radius(CornerRadius::same(12))
                        .inner_margin(Margin::symmetric(14, 12))
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.horizontal_top(|ui| {
                                paint_status_badge(ui, true);
                                ui.add_space(8.0);
                                ui.vertical(|ui| {
                                    ui.label(
                                        RichText::new(title)
                                            .size(13.5)
                                            .color(theme::TEXT_PRIMARY),
                                    );
                                    ui.add_space(3.0);
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(message)
                                                .size(12.2)
                                                .color(theme::TEXT_TERTIARY),
                                        )
                                        .wrap(),
                                    );
                                });
                            });
                        });

                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        if needs_config {
                            if action_pill_button(ui, "Open config.json", true).clicked() {
                                open_config_clicked = true;
                            }
                            if has_last_region
                                && action_pill_button(ui, "Translate Instead", false).clicked()
                            {
                                fallback_translate = true;
                            }
                        } else if has_last_region
                            && action_pill_button(ui, "Retry", true).clicked()
                        {
                            retry_action = Some(active_action);
                        }

                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if action_pill_button(ui, "Dismiss", false).clicked() {
                                dismiss_clicked = true;
                            }
                        });
                    });
                });
                measured = Some(r.response.rect.height());
            }
            PopupContent::Ocr { text } => {
                let text = text.clone();
                let scroll_out = ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let inner = ui.vertical(|ui| {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&text)
                                        .size(13.8)
                                        .color(theme::TEXT_PRIMARY),
                                )
                                .wrap()
                                .selectable(true),
                            );
                        });
                        inner.response.rect.height()
                    });
                measured = Some(scroll_out.inner);
            }
            PopupContent::Result { output, .. } => {
                let output = output.clone();
                let mut toggle_ocr = false;

                let scroll_out = ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let inner = ui.vertical(|ui| {
                            if let Some(ref warning) = output.warning {
                                Frame::new()
                                    .fill(theme::BG_CARD)
                                    .stroke(Stroke::new(1.0, theme::BORDER_HAIRLINE))
                                    .corner_radius(CornerRadius::same(10))
                                    .inner_margin(Margin::symmetric(12, 8))
                                    .show(ui, |ui| {
                                        ui.set_min_width(ui.available_width());
                                        ui.horizontal_top(|ui| {
                                            let (bar, _) = ui.allocate_exact_size(
                                                Vec2::new(2.5, 16.0),
                                                Sense::hover(),
                                            );
                                            ui.painter().rect_filled(bar, 1.5, theme::WARN_TEXT);
                                            ui.add_space(6.0);
                                            ui.add(
                                                egui::Label::new(
                                                    RichText::new(warning)
                                                        .size(11.8)
                                                        .color(theme::TEXT_TERTIARY),
                                                )
                                                .wrap(),
                                            );
                                        });
                                    });
                                ui.add_space(10.0);
                            }

                            if !output.primary_text.is_empty() {
                                if output.is_markdown {
                                    CommonMarkViewer::new().show(
                                        ui,
                                        &mut app.md_cache,
                                        &output.primary_text,
                                    );
                                } else {
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(&output.primary_text)
                                                .size(15.0)
                                                .color(theme::TEXT_PRIMARY),
                                        )
                                        .wrap()
                                        .selectable(true),
                                    );
                                }
                            }

                            if let Some(ref raw_ocr) = output.original_text {
                                ui.add_space(12.0);
                                draw_hairline(ui);
                                ui.add_space(8.0);

                                let short = SnipPopupApp::is_short_ocr(raw_ocr);
                                if short && !output.primary_text.is_empty() {
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(raw_ocr)
                                                .size(12.5)
                                                .color(theme::TEXT_TERTIARY),
                                        )
                                        .wrap()
                                        .selectable(true),
                                    );
                                } else {
                                    let line_count = raw_ocr.lines().count();
                                    ui.horizontal(|ui| {
                                        let label = if line_count > 1 {
                                            format!("ORIGINAL  ·  {line_count} LINES")
                                        } else {
                                            "ORIGINAL TEXT".to_string()
                                        };
                                        let font = FontId::proportional(10.5);
                                        let galley = ui.painter().layout_no_wrap(
                                            label,
                                            font,
                                            theme::TEXT_MUTED,
                                        );
                                        let (rect, resp) = ui.allocate_exact_size(
                                            Vec2::new(galley.size().x + 18.0, 20.0),
                                            Sense::click(),
                                        );
                                        let col = if resp.hovered() {
                                            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                                            theme::TEXT_SECONDARY
                                        } else {
                                            theme::TEXT_MUTED
                                        };
                                        paint_disclosure_chevron(
                                            ui.painter(),
                                            Pos2::new(rect.left() + 6.0, rect.center().y),
                                            app.ocr_expanded,
                                            col,
                                        );
                                        ui.painter().galley(
                                            Pos2::new(
                                                rect.left() + 16.0,
                                                rect.center().y - galley.size().y * 0.5,
                                            ),
                                            galley,
                                            col,
                                        );
                                        if resp.clicked() {
                                            toggle_ocr = true;
                                        }

                                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                            let copied = app
                                                .ocr_copied_at
                                                .map(|t| t.elapsed() < COPY_FEEDBACK_DURATION)
                                                .unwrap_or(false);
                                            let copy_ocr = egui::Button::new(
                                                RichText::new(if copied {
                                                    "Copied"
                                                } else {
                                                    "Copy OCR"
                                                })
                                                .size(10.8)
                                                .color(if copied {
                                                    theme::OK_EMERALD
                                                } else {
                                                    theme::TEXT_MUTED
                                                }),
                                            )
                                            .fill(Color32::TRANSPARENT)
                                            .stroke(Stroke::NONE);
                                            let r = ui.add(copy_ocr);
                                            if r.hovered() {
                                                ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                                            }
                                            if r.clicked() {
                                                SnipPopupApp::copy_to_clipboard(ctx, raw_ocr);
                                                app.ocr_copied_at = Some(Instant::now());
                                            }
                                        });
                                    });

                                    if app.ocr_expanded {
                                        ui.add_space(6.0);
                                        Frame::new()
                                            .fill(theme::BG_CARD)
                                            .stroke(Stroke::new(1.0, theme::BORDER_HAIRLINE))
                                            .corner_radius(CornerRadius::same(10))
                                            .inner_margin(Margin::symmetric(12, 9))
                                            .show(ui, |ui| {
                                                ui.set_min_width(ui.available_width());
                                                ui.add(
                                                    egui::Label::new(
                                                        RichText::new(raw_ocr)
                                                            .size(12.2)
                                                            .color(theme::TEXT_SECONDARY),
                                                    )
                                                    .wrap()
                                                    .selectable(true),
                                                );
                                            });
                                    }
                                }
                            }
                        });
                        inner.response.rect.height()
                    });
                measured = Some(scroll_out.inner);

                if toggle_ocr {
                    app.toggle_ocr_expanded(ctx);
                }
            }
        });

    if open_config_clicked {
        app.open_config();
    }
    if fallback_translate {
        app.dispatch_processing(ctx, SnipAction::Translate, false);
    } else if let Some(act) = retry_action {
        app.dispatch_processing(ctx, act, false);
    }
    if dismiss_clicked {
        app.hide_popup(ctx);
    }

    measured
}

fn render_footer(app: &mut SnipPopupApp, ui: &mut egui::Ui, ctx: &egui::Context) {
    let is_ocr_view = matches!(app.content, PopupContent::Ocr { .. });
    let is_streaming = matches!(
        app.content,
        PopupContent::Result {
            is_streaming: true,
            ..
        }
    );
    let mut translate_ocr = false;

    Frame::new()
        .inner_margin(Margin::symmetric(14, 7))
        .show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let just_copied = app
                    .copied_at
                    .map(|t| t.elapsed() < COPY_FEEDBACK_DURATION)
                    .unwrap_or(false);

                if just_copied {
                    copied_status_pill(ui);
                } else if action_pill_button(ui, "Copy", true)
                    .on_hover_text("Copy to clipboard (Ctrl+C)")
                    .clicked()
                {
                    app.copy_primary_result(ctx);
                }

                if is_ocr_view
                    && app.last_crop.is_some()
                    && action_pill_button(ui, "Translate", false)
                        .on_hover_text("Translate this snipped region")
                        .clicked()
                {
                    translate_ocr = true;
                }

                if action_pill_button(ui, "New Snip", false)
                    .on_hover_text("Capture a new region")
                    .clicked()
                {
                    app.trigger_snip_overlay(ctx);
                }

                if is_streaming {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new("STREAMING…")
                                .size(10.0)
                                .color(theme::TEXT_MUTED),
                        );
                    });
                }
            });
        });

    if translate_ocr {
        app.dispatch_processing(ctx, SnipAction::Translate, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;
    use crate::ui::tray::TrayController;
    use crate::ui::UiEvent;
    use std::sync::{mpsc, Arc, OnceLock};

    fn test_popup_with_error(ctx: &egui::Context) -> (SnipPopupApp, tokio::runtime::Runtime) {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("rt");
        let (tx, rx) = mpsc::channel::<UiEvent>();
        let slot = Arc::new(OnceLock::new());
        let _ = slot.set(ctx.clone());
        let mut app = SnipPopupApp::new_for_test(
            ctx,
            AppConfig::default(),
            std::env::temp_dir().join("snip-ui-test-config.json"),
            tx,
            rx,
            slot,
            rt.handle().clone(),
            TrayController::noop(),
        );
        app.content = PopupContent::Error {
            title: "API Key Required".to_string(),
            message: "Explain Code requires a BYOK API key in config.json".to_string(),
        };
        (app, rt)
    }

    fn run_frame(ctx: &egui::Context, app: &mut SnipPopupApp, input: egui::RawInput) {
        let mut out = ctx.run_ui(input, |ui| render_popup(app, ui, &ctx.clone()));
        out.textures_delta.clear();
    }

    fn simulate_click(ctx: &egui::Context, app: &mut SnipPopupApp, pos: Pos2) {
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(432.0, 176.0));
        let mut down = egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };
        down.events.push(egui::Event::PointerMoved(pos));
        down.events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        });
        run_frame(ctx, app, down);

        let mut up = egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };
        up.events.push(egui::Event::PointerMoved(pos));
        up.events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        });
        run_frame(ctx, app, up);
    }

    #[test]
    fn header_pin_and_close_buttons_are_clickable_and_not_blocked_by_header_drag() {
        let ctx = egui::Context::default();
        let (mut app, _rt) = test_popup_with_error(&ctx);
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(432.0, 176.0));

        // Initial layout pass
        let init = egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };
        run_frame(&ctx, &mut app, init);
        assert!(!app.pinned);

        // Pin button sits immediately left of Close button in right-to-left header (width 432, margin 14, close 22, gap 6)
        let pin_pos = Pos2::new(432.0 - 14.0 - 22.0 - 6.0 - 20.0, HEADER_HEIGHT * 0.5);
        simulate_click(&ctx, &mut app, pin_pos);
        assert!(app.pinned, "Pin button click must toggle pinned=true");

        // Click Close (×) button at top-right (center ~ 432 - 14 - 11 = 407)
        let close_pos = Pos2::new(432.0 - 14.0 - 11.0, HEADER_HEIGHT * 0.5);
        simulate_click(&ctx, &mut app, close_pos);
        assert!(
            matches!(app.content, PopupContent::Hidden),
            "Close (×) button click must hide popup"
        );
    }
}
