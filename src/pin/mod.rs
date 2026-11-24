use crate::capture::CroppedImage;
use crate::snip::SnipCommand;
use crate::ui::theme;
use egui::{
    Color32, ColorImage, CornerRadius, CursorIcon, FontId, Frame, Key, Modifiers, Pos2, Rect,
    Sense, Stroke, StrokeKind, TextureHandle, TextureOptions, Vec2, ViewportBuilder,
    ViewportCommand, ViewportId,
};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use windows::core::HSTRING;
use windows::Win32::Foundation::COLORREF;
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE,
    DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUNDSMALL,
};
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, GetWindowLongW, SetLayeredWindowAttributes, SetWindowLongW, GWL_EXSTYLE,
    LWA_ALPHA, WS_EX_LAYERED,
};

const MIN_ZOOM: f32 = 0.25;
const MAX_ZOOM: f32 = 4.0;
const MIN_OPACITY: f32 = 0.2;
const MAX_OPACITY: f32 = 1.0;
const HUD_DURATION: Duration = Duration::from_millis(1100);

pub struct PinCommandEvent {
    pub cropped: CroppedImage,
    pub command: SnipCommand,
}

struct PinRuntimeState {
    id: u64,
    title: String,
    cropped: CroppedImage,
    texture: TextureHandle,
    base_size: Vec2,
    zoom: f32,
    opacity: f32,
    applied_opacity: f32,
    dwm_styled: bool,
    hud_notice: Option<(String, Instant)>,
    close_requested: bool,
    close_all_requested: bool,
}

struct PinnedSnip {
    id: u64,
    viewport_id: ViewportId,
    builder: ViewportBuilder,
    state: Arc<Mutex<PinRuntimeState>>,
}

impl PinRuntimeState {
    fn current_size(&self) -> Vec2 {
        Vec2::new(
            (self.base_size.x * self.zoom).max(40.0),
            (self.base_size.y * self.zoom).max(40.0),
        )
    }

    fn sync_win32_style(&mut self) {
        let needs_dwm = !self.dwm_styled;
        let needs_alpha = (self.opacity - self.applied_opacity).abs() > 0.005;
        if !needs_dwm && !needs_alpha {
            return;
        }
        let wide_title = HSTRING::from(&self.title);
        // SAFETY: Finds our own uniquely titled pin viewport window and sets standard DWM / layered alpha attributes.
        unsafe {
            if let Ok(hwnd) = FindWindowW(None, &wide_title)
                && !hwnd.is_invalid()
            {
                if needs_dwm {
                    let corner = DWMWCP_ROUNDSMALL;
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
                    // Subtle sky/slate border COLORREF (0x00BBGGRR)
                    let border_color: u32 = 0x00524838;
                    let _ = DwmSetWindowAttribute(
                        hwnd,
                        DWMWA_BORDER_COLOR,
                        (&raw const border_color).cast(),
                        std::mem::size_of_val(&border_color) as u32,
                    );
                    self.dwm_styled = true;
                }
                if needs_alpha {
                    let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE);
                    if (ex_style & WS_EX_LAYERED.0 as i32) == 0 {
                        let _ = SetWindowLongW(hwnd, GWL_EXSTYLE, ex_style | WS_EX_LAYERED.0 as i32);
                    }
                    let alpha = (self.opacity.clamp(MIN_OPACITY, MAX_OPACITY) * 255.0).round() as u8;
                    let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), alpha, LWA_ALPHA);
                    self.applied_opacity = self.opacity;
                }
            }
        }
    }

    fn apply_scroll(&mut self, ctx: &egui::Context, scroll_y: f32, shift: bool) {
        if scroll_y.abs() < 0.1 {
            return;
        }
        let steps = if scroll_y > 0.0 { 1.0 } else { -1.0 };
        if shift {
            self.opacity = clamp_opacity(self.opacity + steps * 0.1);
            let pct = (self.opacity * 100.0).round() as u32;
            self.hud_notice = Some((format!("Opacity {pct}%"), Instant::now() + HUD_DURATION));
        } else {
            self.zoom = clamp_zoom(self.zoom * if steps > 0.0 { 1.1 } else { 1.0 / 1.1 });
            let size = self.current_size();
            ctx.send_viewport_cmd(ViewportCommand::InnerSize(size));
            let pct = (self.zoom * 100.0).round() as u32;
            self.hud_notice = Some((format!("{pct}%"), Instant::now() + HUD_DURATION));
        }
        ctx.request_repaint();
    }

    fn reset_view(&mut self, ctx: &egui::Context) {
        self.zoom = 1.0;
        self.opacity = 1.0;
        ctx.send_viewport_cmd(ViewportCommand::InnerSize(self.current_size()));
        self.hud_notice = Some(("100%".to_string(), Instant::now() + HUD_DURATION));
        ctx.request_repaint();
    }
}

impl PinnedSnip {
    fn new(id: u64, ctx: &egui::Context, cropped: CroppedImage) -> Self {
        let ppp = ctx.pixels_per_point().max(1.0);
        let color_image = ColorImage::from_rgba_unmultiplied(
            [cropped.width as usize, cropped.height as usize],
            &cropped.rgba,
        );
        let texture = ctx.load_texture(
            format!("snip-pin-tex-{id}"),
            color_image,
            TextureOptions::LINEAR,
        );
        let initial_pos = Pos2::new(
            cropped.screen_rect.x as f32 / ppp,
            cropped.screen_rect.y as f32 / ppp,
        );
        let base_size = Vec2::new(
            (cropped.width as f32 / ppp).max(32.0),
            (cropped.height as f32 / ppp).max(32.0),
        );
        let title = format!("SnipPinViewport_{id}");
        let builder = ViewportBuilder::default()
            .with_title(&title)
            .with_decorations(false)
            .with_always_on_top()
            .with_taskbar(false)
            .with_resizable(false)
            .with_has_shadow(true)
            .with_active(true)
            .with_visible(true)
            .with_position(initial_pos)
            .with_inner_size(Vec2::new(base_size.x.max(40.0), base_size.y.max(40.0)));

        let state = PinRuntimeState {
            id,
            title,
            cropped,
            texture,
            base_size,
            zoom: 1.0,
            opacity: 1.0,
            applied_opacity: 1.0,
            dwm_styled: false,
            hud_notice: None,
            close_requested: false,
            close_all_requested: false,
        };

        Self {
            id,
            viewport_id: ViewportId::from_hash_of(("snip-pin-viewport", id)),
            builder,
            state: Arc::new(Mutex::new(state)),
        }
    }
}

#[derive(Default)]
pub struct PinManager {
    pins: Vec<PinnedSnip>,
    next_id: u64,
    pending_events: Arc<Mutex<Vec<PinCommandEvent>>>,
}

impl PinManager {
    pub fn add_pin(&mut self, ctx: &egui::Context, cropped: CroppedImage) {
        self.next_id = self.next_id.wrapping_add(1);
        let w = cropped.width;
        let h = cropped.height;
        let pin = PinnedSnip::new(self.next_id, ctx, cropped);
        log::info!("Created pinned screenshot #{} ({}x{})", pin.id, w, h);
        self.pins.push(pin);
        ctx.request_repaint();
    }

    pub fn close_all(&mut self, ctx: &egui::Context) {
        if !self.pins.is_empty() {
            for pin in &self.pins {
                ctx.send_viewport_cmd_to(pin.viewport_id, ViewportCommand::Close);
            }
            self.pins.clear();
            ctx.request_repaint();
        }
    }

    /// Processes pin close requests and returns any queued user commands from deferred pin windows.
    pub fn drain_pending_events(&mut self, ctx: &egui::Context) -> Vec<PinCommandEvent> {
        let mut close_all_requested = false;
        let mut close_ids = Vec::new();
        for pin in &self.pins {
            let st = pin.state.lock().unwrap_or_else(|e| e.into_inner());
            if st.close_all_requested {
                close_all_requested = true;
            }
            if st.close_requested {
                close_ids.push((pin.id, pin.viewport_id));
            }
        }

        if close_all_requested {
            self.close_all(ctx);
        } else if !close_ids.is_empty() {
            for &(_, vp_id) in &close_ids {
                ctx.send_viewport_cmd_to(vp_id, ViewportCommand::Close);
            }
            self.pins
                .retain(|p| !close_ids.iter().any(|(id, _)| *id == p.id));
            ctx.request_repaint();
        }

        let mut queued = self
            .pending_events
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        std::mem::take(&mut *queued)
    }

    /// Registers all active pinned screenshots as deferred viewports and returns any user commands.
    /// Deferred viewports create their native windows via `handle_viewport_output(event_loop, ...)`
    /// rather than `render_immediate_viewport`, avoiding the eframe 0.37 hidden-root panic.
    pub fn render_all(&mut self, ctx: &egui::Context) -> Vec<PinCommandEvent> {
        let events = self.drain_pending_events(ctx);
        let total_pins = self.pins.len();

        for pin in &self.pins {
            let vp_id = pin.viewport_id;
            let builder = pin.builder.clone();
            let state = Arc::clone(&pin.state);
            let pending_events = Arc::clone(&self.pending_events);

            ctx.show_viewport_deferred(vp_id, builder, move |ui, _| {
                let mut pin = state.lock().unwrap_or_else(|e| e.into_inner());
                let _pin_id = pin.id;
                pin.sync_win32_style();
                let vp_ctx = ui.ctx().clone();

                let push_event = |cropped: &CroppedImage, command: SnipCommand| {
                    pending_events
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .push(PinCommandEvent {
                            cropped: cropped.clone(),
                            command,
                        });
                    vp_ctx.request_repaint_of(ViewportId::ROOT);
                };

                if vp_ctx.input(|i| i.viewport().close_requested()) {
                    pin.close_requested = true;
                    vp_ctx.request_repaint_of(ViewportId::ROOT);
                    return;
                }

                // Keyboard shortcuts when this pin viewport has focus
                let (esc, ctrl_c, ctrl_s, key_o, key_t, key_zero, scroll_y, shift) =
                    vp_ctx.input(|i| {
                        let ctrl = i.modifiers.matches_logically(Modifiers::CTRL);
                        let plain = !ctrl && !i.modifiers.alt;
                        (
                            i.key_pressed(Key::Escape),
                            ctrl && i.key_pressed(Key::C),
                            ctrl && i.key_pressed(Key::S),
                            plain && i.key_pressed(Key::O),
                            plain && i.key_pressed(Key::T),
                            i.key_pressed(Key::Num0),
                            i.smooth_scroll_delta.y,
                            i.modifiers.shift,
                        )
                    });

                if esc {
                    pin.close_requested = true;
                    vp_ctx.request_repaint_of(ViewportId::ROOT);
                    return;
                }
                if ctrl_c {
                    push_event(&pin.cropped, SnipCommand::CopyImage);
                } else if ctrl_s {
                    push_event(&pin.cropped, SnipCommand::SavePng);
                } else if key_o {
                    push_event(&pin.cropped, SnipCommand::OcrCopy);
                } else if key_t {
                    push_event(&pin.cropped, SnipCommand::Translate);
                } else if key_zero {
                    pin.reset_view(&vp_ctx);
                }

                if scroll_y.abs() > 0.1 {
                    pin.apply_scroll(&vp_ctx, scroll_y, shift);
                }

                egui::CentralPanel::default()
                    .frame(Frame::NONE)
                    .show(ui, |ui| {
                        let rect = ui.max_rect();
                        let response = ui.allocate_rect(rect, Sense::click_and_drag());
                        let painter = ui.painter_at(rect);

                        painter.image(
                            pin.texture.id(),
                            rect,
                            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                            Color32::WHITE,
                        );

                        // Subtle 1px frame border so pinned shots stay legible over matching backgrounds
                        let hovered = vp_ctx.input(|i| i.pointer.has_pointer());
                        let border_col = if hovered {
                            theme::ACCENT_PICK
                        } else {
                            Color32::from_rgba_unmultiplied(255, 255, 255, 42)
                        };
                        painter.rect_stroke(
                            rect,
                            0.0,
                            Stroke::new(if hovered { 1.5 } else { 1.0 }, border_col),
                            StrokeKind::Inside,
                        );

                        // Hover close button in top-right corner (crisp vector cross)
                        let close_btn_rect = Rect::from_min_size(
                            Pos2::new(rect.right() - 26.0, rect.top() + 6.0),
                            Vec2::splat(20.0),
                        );
                        let pointer_pos = vp_ctx.input(|i| i.pointer.hover_pos());
                        let over_close =
                            hovered && pointer_pos.is_some_and(|p| close_btn_rect.contains(p));

                        if hovered && rect.width() >= 56.0 && rect.height() >= 36.0 {
                            let bg = if over_close {
                                theme::CLOSE_HOVER_BG
                            } else {
                                Color32::from_rgba_unmultiplied(15, 15, 15, 224)
                            };
                            let c = close_btn_rect.center();
                            painter.circle_filled(c, 10.0, bg);
                            if !over_close {
                                painter.circle_stroke(
                                    c,
                                    10.0,
                                    Stroke::new(
                                        1.0,
                                        Color32::from_rgba_unmultiplied(255, 255, 255, 36),
                                    ),
                                );
                            }
                            let r = 3.2;
                            let s = Stroke::new(1.35, theme::TEXT_PRIMARY);
                            painter.line_segment(
                                [Pos2::new(c.x - r, c.y - r), Pos2::new(c.x + r, c.y + r)],
                                s,
                            );
                            painter.line_segment(
                                [Pos2::new(c.x + r, c.y - r), Pos2::new(c.x - r, c.y + r)],
                                s,
                            );
                        }

                        if over_close {
                            vp_ctx.set_cursor_icon(CursorIcon::PointingHand);
                            if vp_ctx.input(|i| i.pointer.primary_clicked()) {
                                pin.close_requested = true;
                                vp_ctx.request_repaint_of(ViewportId::ROOT);
                            }
                        } else {
                            if response.hovered() {
                                vp_ctx.set_cursor_icon(CursorIcon::Grab);
                            }
                            if response.double_clicked() {
                                pin.close_requested = true;
                                vp_ctx.request_repaint_of(ViewportId::ROOT);
                            } else if response.drag_started_by(egui::PointerButton::Primary) {
                                vp_ctx.send_viewport_cmd(ViewportCommand::StartDrag);
                            }
                        }

                        // Zoom / opacity HUD badge
                        if let Some((ref label, expires)) = pin.hud_notice {
                            if Instant::now() < expires {
                                let font = FontId::monospace(11.5);
                                let galley =
                                    painter.layout_no_wrap(label.clone(), font, theme::TEXT_PRIMARY);
                                let pad = Vec2::new(7.0, 3.0);
                                let pill = Rect::from_min_size(
                                    Pos2::new(rect.left() + 6.0, rect.top() + 6.0),
                                    galley.size() + pad * 2.0,
                                );
                                painter.rect_filled(
                                    pill,
                                    CornerRadius::same(10),
                                    Color32::from_rgba_unmultiplied(15, 15, 15, 236),
                                );
                                painter.rect_stroke(
                                    pill,
                                    CornerRadius::same(10),
                                    Stroke::new(
                                        1.0,
                                        Color32::from_rgba_unmultiplied(255, 255, 255, 30),
                                    ),
                                    StrokeKind::Inside,
                                );
                                painter.galley(pill.min + pad, galley, theme::TEXT_PRIMARY);
                                vp_ctx.request_repaint_after(Duration::from_millis(80));
                            } else {
                                pin.hud_notice = None;
                            }
                        }

                        response.context_menu(|ui| {
                            if ui.button("Copy Image  (Ctrl+C)").clicked() {
                                push_event(&pin.cropped, SnipCommand::CopyImage);
                                ui.close();
                            }
                            if ui.button("Save PNG  (Ctrl+S)").clicked() {
                                push_event(&pin.cropped, SnipCommand::SavePng);
                                ui.close();
                            }
                            ui.separator();
                            if ui.button("OCR Copy Text  (O)").clicked() {
                                push_event(&pin.cropped, SnipCommand::OcrCopy);
                                ui.close();
                            }
                            if ui.button("Translate  (T)").clicked() {
                                push_event(&pin.cropped, SnipCommand::Translate);
                                ui.close();
                            }
                            if ui.button("Explain Code").clicked() {
                                push_event(&pin.cropped, SnipCommand::ExplainCode);
                                ui.close();
                            }
                            ui.separator();
                            if ui.button("Reset Zoom & Opacity  (0)").clicked() {
                                pin.reset_view(&vp_ctx);
                                ui.close();
                            }
                            if ui.button("Close Pin  (Esc)").clicked() {
                                pin.close_requested = true;
                                vp_ctx.request_repaint_of(ViewportId::ROOT);
                                ui.close();
                            }
                            if total_pins > 1 && ui.button("Close All Pins").clicked() {
                                pin.close_all_requested = true;
                                vp_ctx.request_repaint_of(ViewportId::ROOT);
                                ui.close();
                            }
                        });
                    });
            });
        }

        events
    }
}

pub fn clamp_zoom(z: f32) -> f32 {
    z.clamp(MIN_ZOOM, MAX_ZOOM)
}

pub fn clamp_opacity(o: f32) -> f32 {
    o.clamp(MIN_OPACITY, MAX_OPACITY)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamps_zoom_and_opacity_to_safe_bounds() {
        assert_eq!(clamp_zoom(0.05), MIN_ZOOM);
        assert_eq!(clamp_zoom(10.0), MAX_ZOOM);
        assert_eq!(clamp_zoom(1.5), 1.5);

        assert_eq!(clamp_opacity(0.0), MIN_OPACITY);
        assert_eq!(clamp_opacity(2.0), MAX_OPACITY);
        assert_eq!(clamp_opacity(0.7), 0.7);
    }
}
