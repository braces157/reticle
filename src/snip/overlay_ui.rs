use crate::capture::{monitor_rects, CroppedImage, ScreenRect, VirtualScreenCapture};
use crate::snip::annotate::{
    paint_op, phys_pt_to_logical, rgba_to_c32, AnnotationDoc, AnnotationOp, AnnotationTool,
    DraftAnnotation, StrokeSize, PALETTE,
};
use crate::snip::raster::composite_annotations;
use crate::snip::state::{
    active_monitor_for, apply_drag, compute_toolbar_pos, hit_test_handle, logical_to_phys,
    phys_rect_to_logical, DragMode, OverlayStage, ResizeHandle, ScreenPt, SnipCommand, SnipMode,
};
use crate::snip::toolbar::{
    dim_color, paint_dimension_pill, paint_dimmed_surround, paint_idle_hint, render_snip_toolbar,
    ToolbarEvent, ToolbarViewState, SEL_ACCENT,
};
use egui::{
    Area, Color32, ColorImage, CornerRadius, CursorIcon, FontId, Frame, Id, Key, Margin, Modifiers,
    Order, Pos2, Rect, Sense, Stroke, StrokeKind, TextEdit, TextureHandle, TextureOptions, Vec2,
    ViewportBuilder, ViewportCommand, ViewportId,
};
use windows::core::w;
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_CLOAK, DWMWA_TRANSITIONS_FORCEDISABLED,
};
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, GetWindowLongPtrW, GetWindowRect, SetForegroundWindow, SetWindowLongPtrW,
    SetWindowPos, GWL_EXSTYLE, GWL_STYLE, HWND_TOPMOST, SWP_FRAMECHANGED, SWP_NOACTIVATE,
    SWP_NOSENDCHANGING, WS_BORDER, WS_CAPTION, WS_CLIPCHILDREN, WS_CLIPSIBLINGS, WS_DLGFRAME,
    WS_EX_APPWINDOW, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_POPUP,
    WS_SYSMENU, WS_THICKFRAME,
};

pub const OVERLAY_VIEWPORT_TITLE: &str = "SnipToolOverlayViewport";

const HANDLE_SIZE: f32 = 7.0;
const HANDLE_HIT_RADIUS: f32 = 9.0;

pub fn overlay_viewport_id() -> ViewportId {
    ViewportId::from_hash_of("snip-tool-overlay-viewport")
}

pub enum SnipOverlayResult {
    Completed {
        cropped: CroppedImage,
        command: SnipCommand,
        doc_revision: u64,
    },
    Cancelled,
}

struct PendingText {
    pos: ScreenPt,
    text: String,
    focus_requested: bool,
}

pub struct SnipOverlaySession {
    capture: VirtualScreenCapture,
    monitors: Vec<ScreenRect>,
    texture: TextureHandle,
    stage: OverlayStage,
    mode: SnipMode,
    revealed: bool,
    cloak_frames: u8,
    stable_frames: u8,
    reposition_attempts: u8,
    window_phys_rect: ScreenRect,
    toolbar_id: Id,
    text_area_id: Id,
    text_edit_id: Id,
    last_toolbar_size: Vec2,
    toolbar_rect: Option<Rect>,
    text_box_rect: Option<Rect>,

    doc: AnnotationDoc,
    doc_revision: u64,
    active_tool: Option<AnnotationTool>,
    active_color: [u8; 4],
    stroke_size: StrokeSize,
    in_progress: Option<DraftAnnotation>,
    pending_text: Option<PendingText>,
}

impl SnipOverlaySession {
    pub fn new(ctx: &egui::Context, capture: VirtualScreenCapture, mode: SnipMode) -> Self {
        let rgba = capture.to_rgba();
        let color_image = ColorImage::from_rgba_unmultiplied(
            [capture.width as usize, capture.height as usize],
            &rgba,
        );
        let texture = ctx.load_texture("snip-frozen-desktop", color_image, TextureOptions::NEAREST);
        let monitors = monitor_rects();
        let initial_bounds = capture.bounds();

        Self {
            capture,
            monitors,
            texture,
            stage: OverlayStage::Idle,
            mode,
            revealed: false,
            cloak_frames: 0,
            stable_frames: 0,
            reposition_attempts: 0,
            window_phys_rect: initial_bounds,
            toolbar_id: Id::unique("snip-floating-toolbar"),
            text_area_id: Id::unique("snip-inline-text-area"),
            text_edit_id: Id::unique("snip-inline-text-edit"),
            last_toolbar_size: Vec2::new(560.0, 38.0),
            toolbar_rect: None,
            text_box_rect: None,
            doc: AnnotationDoc::default(),
            doc_revision: 0,
            active_tool: None,
            active_color: PALETTE[0],
            stroke_size: StrokeSize::Medium,
            in_progress: None,
            pending_text: None,
        }
    }

    /// Returns `(rect, doc_revision)` when the selection box is stationary (mouse released,
    /// not dragging handles, not actively drawing or typing text) — safe to speculatively prefetch.
    pub fn settled_selection(&self) -> Option<(ScreenRect, u64)> {
        if let OverlayStage::Adjusting { rect, drag: None } = self.stage
            && self.in_progress.is_none()
            && self.pending_text.is_none()
        {
            Some((rect, self.doc_revision))
        } else {
            None
        }
    }

    /// Crops the currently settled selection with any committed annotations composited.
    pub fn render_settled_crop(&self) -> Option<(CroppedImage, u64)> {
        let (rect, rev) = self.settled_selection()?;
        let mut cropped = self.capture.crop_to_rgba(rect).ok()?;
        composite_annotations(&mut cropped, &self.doc);
        Some((cropped, rev))
    }

    fn base_viewport_builder() -> ViewportBuilder {
        ViewportBuilder::default()
            .with_title(OVERLAY_VIEWPORT_TITLE)
            .with_decorations(false)
            .with_always_on_top()
            .with_taskbar(false)
            .with_resizable(false)
            .with_has_shadow(false)
            .with_clamp_size_to_monitor_size(false)
    }

    pub fn viewport_builder(&self) -> ViewportBuilder {
        Self::active_viewport_builder()
    }

    fn active_viewport_builder() -> ViewportBuilder {
        // Keep all window-creation attributes identical to `hidden_viewport_builder()` so
        // `ViewportBuilder::patch` never requests window recreation (which panics in eframe 0.37
        // when root is hidden). Deliberately omit `with_position` / `with_inner_size` because
        // winit converts logical coords using the wrong monitor's DPI on mixed-DPI multi-monitor
        // setups; exact physical placement is handled exclusively via Win32 `SetWindowPos`.
        Self::base_viewport_builder().with_visible(true)
    }

    pub fn hidden_viewport_builder() -> ViewportBuilder {
        // Deliberately omit `with_inner_size` so hiding the overlay after a snip preserves its
        // exact multi-monitor HWND geometry and DPI for instant, zero-jump subsequent Alt+O triggers.
        Self::base_viewport_builder().with_visible(false)
    }

    fn sync_window_phys_rect(&mut self, expected: ScreenRect) -> Option<windows::Win32::Foundation::HWND> {
        // SAFETY: Reads the physical bounding rect of our own uniquely titled overlay HWND.
        unsafe {
            let hwnd = FindWindowW(None, w!("SnipToolOverlayViewport")).ok()?;
            if hwnd.is_invalid() {
                return None;
            }
            let mut wr = RECT::default();
            if GetWindowRect(hwnd, &mut wr).is_ok() {
                let w = wr.right - wr.left;
                let h = wr.bottom - wr.top;
                if w > 0 && h > 0 {
                    self.window_phys_rect = ScreenRect {
                        x: wr.left,
                        y: wr.top,
                        width: w,
                        height: h,
                    };
                    return Some(hwnd);
                }
            }
            self.window_phys_rect = expected;
            Some(hwnd)
        }
    }

    fn ensure_win32_bounds_and_reveal(&mut self, ctx: &egui::Context, canvas_logical: Rect) {
        let b = self.capture.bounds();
        let hwnd = self.sync_window_phys_rect(b);

        if !self.revealed {
            // Stay invisibly cloaked (`DWMWA_CLOAK = 1`) until Win32 bounds, `WM_DPICHANGED`,
            // AND the OpenGL/egui swapchain have all stabilized at exact `(b.x, b.y, b.w, b.h)`
            // and swapped a full pixel-aligned frame to DWM. This eliminates both white flash
            // and any visible multi-monitor move/snap-back jump on Alt+O.
            set_overlay_cloaked(true);
            self.cloak_frames = self.cloak_frames.saturating_add(1);

            let bounds_match = self.window_phys_rect == b;
            let ppp = ctx.pixels_per_point().max(1.0);
            let gl_matches = is_gl_canvas_synced(canvas_logical, ppp, b);

            if !bounds_match && let Some(hwnd) = hwnd {
                apply_overlay_tool_popup_style(hwnd);
                // SAFETY: Sizes our own cloaked overlay viewport in exact physical pixels.
                // `SWP_NOSENDCHANGING` bypasses `WM_GETMINMAXINFO` single-monitor clamping.
                unsafe {
                    let _ = SetWindowPos(
                        hwnd,
                        Some(HWND_TOPMOST),
                        b.x,
                        b.y,
                        b.width,
                        b.height,
                        SWP_FRAMECHANGED | SWP_NOACTIVATE | SWP_NOSENDCHANGING,
                    );
                }
                let _ = self.sync_window_phys_rect(b);
                self.stable_frames = 0;
            } else if bounds_match && gl_matches {
                self.stable_frames = self.stable_frames.saturating_add(1);
            } else {
                self.stable_frames = 0;
            }

            // Reveal once we have 2 consecutive settled frames (at least 1 full-size GL swap
            // already in DWM) or after a short safety cap (~160ms) so we never stay hidden.
            let ready_to_reveal = self.stable_frames >= 2 || self.cloak_frames >= 10;
            if ready_to_reveal {
                if let Some(hwnd) = hwnd {
                    // SAFETY: Atomically reveals our already-positioned, already-painted overlay.
                    unsafe {
                        set_overlay_cloaked(false);
                        let _ = SetForegroundWindow(hwnd);
                    }
                } else {
                    set_overlay_cloaked(false);
                }
                self.revealed = true;
            } else {
                ctx.request_repaint();
            }
            return;
        }

        // Post-reveal drift guard (should rarely trigger now that stabilization happens cloaked).
        if self.window_phys_rect != b
            && self.reposition_attempts < 4
            && let Some(hwnd) = hwnd
        {
            self.reposition_attempts += 1;
            // SAFETY: Restores exact physical multi-monitor bounds on our own overlay HWND.
            unsafe {
                let _ = SetWindowPos(
                    hwnd,
                    Some(HWND_TOPMOST),
                    b.x,
                    b.y,
                    b.width,
                    b.height,
                    SWP_NOACTIVATE | SWP_NOSENDCHANGING,
                );
            }
            let _ = self.sync_window_phys_rect(b);
            ctx.request_repaint();
        }
    }

    /// Renders the interactive overlay inside its viewport and returns `Some(result)` when finished.
    pub fn ui(&mut self, ui: &mut egui::Ui) -> Option<SnipOverlayResult> {
        let ctx = ui.ctx().clone();
        self.ensure_win32_bounds_and_reveal(&ctx, ui.max_rect());

        if let Some(cmd) = self.handle_keyboard(&ctx) {
            return Some(self.finalize_command(cmd));
        }

        let mut triggered_cmd: Option<SnipCommand> = None;
        let capture_bounds = self.capture.bounds();
        // Always map canvas pixels and texture UVs against the HWND's ACTUAL physical rect so
        // Monitor 2 can never bleed onto Monitor 1 even during mixed-DPI transitions.
        let bounds = self.window_phys_rect;
        let uv_rect = compute_overlay_uv(bounds, capture_bounds);

        egui::CentralPanel::default()
            .frame(Frame::NONE)
            .show(ui, |ui| {
                let canvas = ui.max_rect();
                let response = ui.allocate_rect(canvas, Sense::click_and_drag());
                let painter = ui.painter_at(canvas);

                painter.image(self.texture.id(), canvas, uv_rect, Color32::WHITE);

                let hover_pos = ctx.input(|i| i.pointer.hover_pos());
                let pointer_pos = ctx.input(|i| i.pointer.interact_pos()).or(hover_pos);
                let over_ui = pointer_pos.is_some_and(|p| {
                    self.toolbar_rect.is_some_and(|r| r.expand(3.0).contains(p))
                        || self.text_box_rect.is_some_and(|r| r.expand(3.0).contains(p))
                });

                if !over_ui
                    && let Some(cmd) =
                        self.handle_pointer(&ctx, &response, canvas, bounds, hover_pos)
                {
                    triggered_cmd = Some(cmd);
                }

                if let Some(sel_phys) = self.stage.selection_rect() {
                    let sel_logical = phys_rect_to_logical(sel_phys, canvas, bounds);

                    // Paint committed + in-progress annotations clipped to the selection crop
                    let clip_painter = ui.painter_at(sel_logical);
                    for op in &self.doc.ops {
                        paint_op(&clip_painter, op, &self.capture, canvas, bounds);
                    }
                    if let Some(ref draft) = self.in_progress {
                        paint_op(&clip_painter, &draft.op, &self.capture, canvas, bounds);
                    }

                    paint_dimmed_surround(&painter, canvas, sel_logical);

                    painter.rect_stroke(
                        sel_logical,
                        0.0,
                        Stroke::new(1.5, SEL_ACCENT),
                        StrokeKind::Outside,
                    );

                    if self.stage.is_adjusting() {
                        for handle in ResizeHandle::ALL {
                            let center = handle.anchor_pos(sel_logical);
                            painter.circle_filled(center, HANDLE_SIZE * 0.52, Color32::WHITE);
                            painter.circle_stroke(
                                center,
                                HANDLE_SIZE * 0.52,
                                Stroke::new(1.25, SEL_ACCENT),
                            );
                        }
                    }

                    paint_dimension_pill(&painter, sel_logical, sel_phys, canvas);

                    // Render inline text editor if active
                    self.render_inline_text_editor(&ctx, canvas, bounds);
                } else {
                    self.text_box_rect = None;
                    painter.rect_filled(canvas, 0.0, dim_color());
                    if let Some(pos) = hover_pos {
                        let guide =
                            Stroke::new(1.0, Color32::from_rgba_unmultiplied(226, 232, 240, 110));
                        painter.line_segment(
                            [Pos2::new(canvas.left(), pos.y), Pos2::new(canvas.right(), pos.y)],
                            guide,
                        );
                        painter.line_segment(
                            [Pos2::new(pos.x, canvas.top()), Pos2::new(pos.x, canvas.bottom())],
                            guide,
                        );
                    }
                    let cursor_phys = hover_pos.map(|p| {
                        let pt = logical_to_phys(p, canvas, bounds);
                        (pt.x, pt.y)
                    });
                    paint_idle_hint(&painter, canvas, &self.monitors, bounds, cursor_phys, self.mode);
                }

                // Floating Snipaste toolbar when Adjusting & not actively dragging a handle or drawing
                if let OverlayStage::Adjusting {
                    rect: sel_phys,
                    drag: None,
                } = self.stage
                    && self.in_progress.is_none()
                {
                    let sel_logical = phys_rect_to_logical(sel_phys, canvas, bounds);
                    let active_mon = active_monitor_for(sel_phys, &self.monitors, bounds);
                    let mon_logical = phys_rect_to_logical(active_mon, canvas, bounds);
                    let pos = compute_toolbar_pos(sel_logical, self.last_toolbar_size, mon_logical);

                    let tb_state = ToolbarViewState {
                        active_tool: self.active_tool,
                        active_color: self.active_color,
                        stroke_size: self.stroke_size,
                        can_undo: self.doc.can_undo(),
                        can_redo: self.doc.can_redo(),
                    };
                    let area_resp = Area::new(self.toolbar_id)
                        .order(Order::Foreground)
                        .fixed_pos(pos)
                        .show(&ctx, |ui| render_snip_toolbar(ui, &tb_state));

                    let bar_rect = area_resp.response.rect;
                    self.toolbar_rect = Some(bar_rect);
                    if (bar_rect.size() - self.last_toolbar_size).length_sq() > 1.0 {
                        self.last_toolbar_size = bar_rect.size();
                        ctx.request_repaint();
                    }
                    if let Some(ev) = area_resp.inner
                        && let Some(cmd) = self.apply_toolbar_event(ev)
                    {
                        triggered_cmd = Some(cmd);
                    }
                } else {
                    self.toolbar_rect = None;
                }
            });

        triggered_cmd.map(|cmd| self.finalize_command(cmd))
    }

    fn apply_toolbar_event(&mut self, event: ToolbarEvent) -> Option<SnipCommand> {
        match event {
            ToolbarEvent::ToggleTool(tool) => {
                self.commit_pending_text();
                self.active_tool = if self.active_tool == Some(tool) {
                    None
                } else {
                    Some(tool)
                };
                None
            }
            ToolbarEvent::SelectColor(col) => {
                self.active_color = col;
                None
            }
            ToolbarEvent::SelectSize(size) => {
                self.stroke_size = size;
                None
            }
            ToolbarEvent::Undo => {
                self.commit_pending_text();
                self.doc.undo();
                self.doc_revision = self.doc_revision.wrapping_add(1);
                None
            }
            ToolbarEvent::Redo => {
                self.doc.redo();
                self.doc_revision = self.doc_revision.wrapping_add(1);
                None
            }
            ToolbarEvent::Command(cmd) => Some(cmd),
        }
    }

    fn commit_pending_text(&mut self) {
        if let Some(pending) = self.pending_text.take() {
            let trimmed = pending.text.trim();
            if !trimmed.is_empty() {
                self.doc.push(AnnotationOp::Text {
                    pos: pending.pos,
                    content: trimmed.to_string(),
                    color: self.active_color,
                    font_px: self.stroke_size.font_px(),
                });
                self.doc_revision = self.doc_revision.wrapping_add(1);
            }
        }
        self.text_box_rect = None;
    }

    fn render_inline_text_editor(&mut self, ctx: &egui::Context, canvas: Rect, bounds: ScreenRect) {
        let Some(ref mut pending) = self.pending_text else {
            self.text_box_rect = None;
            return;
        };

        let scale = if bounds.width > 0 {
            canvas.width() / bounds.width as f32
        } else {
            1.0
        };
        let pos = phys_pt_to_logical(pending.pos, canvas, bounds);
        let font_size = (self.stroke_size.font_px() * scale).max(12.0);
        let color = rgba_to_c32(self.active_color);

        let mut commit = false;
        let mut cancel = false;

        let area_resp = Area::new(self.text_area_id)
            .order(Order::Foreground)
            .fixed_pos(pos - Vec2::new(4.0, 3.0))
            .show(ctx, |ui| {
                Frame::new()
                    .fill(Color32::from_rgba_unmultiplied(20, 20, 24, 210))
                    .stroke(Stroke::new(1.0, SEL_ACCENT))
                    .corner_radius(CornerRadius::same(4))
                    .inner_margin(Margin::symmetric(5, 3))
                    .show(ui, |ui| {
                        let edit = TextEdit::singleline(&mut pending.text)
                            .id(self.text_edit_id)
                            .font(FontId::proportional(font_size))
                            .text_color(color)
                            .frame(Frame::NONE)
                            .desired_width(180.0)
                            .hint_text("Type note…");
                        let resp = ui.add(edit);
                        if !pending.focus_requested {
                            resp.request_focus();
                            pending.focus_requested = true;
                        }
                        if ui.input(|i| i.key_pressed(Key::Enter)) {
                            commit = true;
                        } else if ui.input(|i| i.key_pressed(Key::Escape)) {
                            cancel = true;
                        }
                    });
            });

        self.text_box_rect = Some(area_resp.response.rect);

        if cancel {
            self.pending_text = None;
            self.text_box_rect = None;
        } else if commit {
            self.commit_pending_text();
        }
    }

    fn handle_keyboard(&mut self, ctx: &egui::Context) -> Option<SnipCommand> {
        // When typing inside inline TextEdit, let it handle Enter/Esc/letters
        if self.pending_text.is_some() {
            return None;
        }

        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            return Some(SnipCommand::Cancel);
        }

        let OverlayStage::Adjusting { mut rect, drag } = self.stage else {
            return None;
        };
        if drag.is_some() || self.in_progress.is_some() {
            return None;
        }

        let (ctrl, shift, enter, c, s, z, y, o, t, e, p, num, nudge) = ctx.input(|i| {
            let ctrl = i.modifiers.matches_logically(Modifiers::CTRL);
            let shift = i.modifiers.shift;
            let plain = !ctrl && !i.modifiers.alt;
            let step = if shift { 10 } else { 1 };
            let dx = if i.key_pressed(Key::ArrowRight) {
                step
            } else if i.key_pressed(Key::ArrowLeft) {
                -step
            } else {
                0
            };
            let dy = if i.key_pressed(Key::ArrowDown) {
                step
            } else if i.key_pressed(Key::ArrowUp) {
                -step
            } else {
                0
            };
            let num = if plain && i.key_pressed(Key::Num1) {
                Some(AnnotationTool::Rect)
            } else if plain && i.key_pressed(Key::Num2) {
                Some(AnnotationTool::Arrow)
            } else if plain && i.key_pressed(Key::Num3) {
                Some(AnnotationTool::Pen)
            } else if plain && i.key_pressed(Key::Num4) {
                Some(AnnotationTool::Highlight)
            } else if plain && i.key_pressed(Key::Num5) {
                Some(AnnotationTool::Mosaic)
            } else if plain && i.key_pressed(Key::Num6) {
                Some(AnnotationTool::Text)
            } else {
                None
            };
            (
                ctrl,
                shift,
                i.key_pressed(Key::Enter),
                i.key_pressed(Key::C),
                i.key_pressed(Key::S),
                i.key_pressed(Key::Z),
                i.key_pressed(Key::Y),
                plain && i.key_pressed(Key::O),
                plain && i.key_pressed(Key::T),
                plain && i.key_pressed(Key::E),
                plain && i.key_pressed(Key::P),
                num,
                (dx, dy),
            )
        });

        if ctrl && z && !shift {
            self.doc.undo();
            self.doc_revision = self.doc_revision.wrapping_add(1);
            return None;
        }
        if ctrl && (y || (z && shift)) {
            self.doc.redo();
            self.doc_revision = self.doc_revision.wrapping_add(1);
            return None;
        }
        if let Some(tool) = num {
            self.active_tool = if self.active_tool == Some(tool) {
                None
            } else {
                Some(tool)
            };
            return None;
        }

        if enter || (ctrl && c) {
            return Some(SnipCommand::CopyImage);
        }
        if ctrl && s {
            return Some(SnipCommand::SavePng);
        }
        if p {
            return Some(SnipCommand::PinToScreen);
        }
        if o {
            return Some(SnipCommand::OcrCopy);
        }
        if t {
            return Some(SnipCommand::Translate);
        }
        if e {
            return Some(SnipCommand::ExplainCode);
        }

        if nudge != (0, 0) {
            let bounds = self.capture.bounds();
            let max_x = (bounds.right() - rect.width).max(bounds.x);
            let max_y = (bounds.bottom() - rect.height).max(bounds.y);
            rect.x = (rect.x + nudge.0).clamp(bounds.x, max_x);
            rect.y = (rect.y + nudge.1).clamp(bounds.y, max_y);
            self.stage = OverlayStage::Adjusting { rect, drag: None };
        }

        None
    }

    fn handle_pointer(
        &mut self,
        ctx: &egui::Context,
        response: &egui::Response,
        canvas: Rect,
        bounds: ScreenRect,
        hover_pos: Option<Pos2>,
    ) -> Option<SnipCommand> {
        let primary_down = ctx.input(|i| i.pointer.primary_down());
        let primary_pressed = ctx.input(|i| i.pointer.primary_pressed());
        let primary_released = ctx.input(|i| i.pointer.primary_released());
        let secondary_clicked = ctx.input(|i| i.pointer.secondary_clicked());
        let interact_pos = ctx.input(|i| i.pointer.interact_pos()).or(hover_pos);

        let cursor_icon = match self.stage {
            OverlayStage::Idle | OverlayStage::Selecting { .. } => CursorIcon::Crosshair,
            OverlayStage::Adjusting { rect, drag } => {
                if self.in_progress.is_some() {
                    CursorIcon::Crosshair
                } else if let Some(d) = drag {
                    match d {
                        DragMode::Move { .. } => CursorIcon::Grabbing,
                        DragMode::Resize { handle, .. } => handle_cursor(handle),
                    }
                } else if let Some(p) = hover_pos {
                    let sel_log = phys_rect_to_logical(rect, canvas, bounds);
                    if let Some(h) = hit_test_handle(p, sel_log, HANDLE_HIT_RADIUS) {
                        handle_cursor(h)
                    } else if sel_log.contains(p) {
                        if self.active_tool == Some(AnnotationTool::Text) {
                            CursorIcon::Text
                        } else if self.active_tool.is_some() {
                            CursorIcon::Crosshair
                        } else {
                            CursorIcon::Grab
                        }
                    } else {
                        CursorIcon::Crosshair
                    }
                } else {
                    CursorIcon::Crosshair
                }
            }
        };
        ctx.set_cursor_icon(cursor_icon);

        if secondary_clicked {
            if self.pending_text.is_some() {
                self.pending_text = None;
                self.text_box_rect = None;
                return None;
            }
            if self.in_progress.is_some() {
                self.in_progress = None;
                return None;
            }
            if self.active_tool.is_some() {
                self.active_tool = None;
                return None;
            }
            return match self.stage {
                OverlayStage::Idle => Some(SnipCommand::Cancel),
                OverlayStage::Selecting { .. } | OverlayStage::Adjusting { .. } => {
                    self.doc = AnnotationDoc::default();
                    self.stage = OverlayStage::Idle;
                    None
                }
            };
        }

        // Handle active in-progress annotation stroke
        if let Some(mut draft) = self.in_progress.take() {
            if let Some(pos) = interact_pos {
                let pt = logical_to_phys(pos, canvas, bounds);
                draft.update(pt);
            }
            if primary_released || !primary_down {
                if draft.is_meaningful() {
                    self.doc.push(draft.op);
                    self.doc_revision = self.doc_revision.wrapping_add(1);
                }
            } else {
                self.in_progress = Some(draft);
            }
            return None;
        }

        match self.stage {
            OverlayStage::Idle => {
                if primary_pressed && let Some(pos) = interact_pos {
                    let pt = logical_to_phys(pos, canvas, bounds);
                    self.stage = OverlayStage::Selecting {
                        anchor: pt,
                        current: pt,
                    };
                }
            }
            OverlayStage::Selecting {
                anchor,
                mut current,
            } => {
                if let Some(pos) = interact_pos {
                    current = logical_to_phys(pos, canvas, bounds);
                }
                if primary_released || !primary_down {
                    let rect = ScreenRect::from_points(anchor.x, anchor.y, current.x, current.y)
                        .clamp_to(bounds);
                    if rect.is_valid_snip() {
                        self.stage = OverlayStage::Adjusting { rect, drag: None };
                        if self.mode == SnipMode::QuickTranslate {
                            return Some(SnipCommand::Translate);
                        }
                    } else {
                        self.stage = OverlayStage::Idle;
                    }
                } else {
                    self.stage = OverlayStage::Selecting { anchor, current };
                }
            }
            OverlayStage::Adjusting { rect, drag } => {
                let sel_log = phys_rect_to_logical(rect, canvas, bounds);
                if drag.is_none()
                    && self.active_tool.is_none()
                    && response.double_clicked()
                    && interact_pos.is_some_and(|p| sel_log.contains(p))
                {
                    return Some(SnipCommand::CopyImage);
                }

                if let Some(active_drag) = drag {
                    if let Some(pos) = interact_pos {
                        let cursor = logical_to_phys(pos, canvas, bounds);
                        let next_rect = apply_drag(active_drag, cursor, bounds);
                        self.stage = OverlayStage::Adjusting {
                            rect: next_rect,
                            drag: if primary_released || !primary_down {
                                None
                            } else {
                                Some(active_drag)
                            },
                        };
                    } else if primary_released || !primary_down {
                        self.stage = OverlayStage::Adjusting { rect, drag: None };
                    }
                } else if primary_pressed && let Some(pos) = interact_pos {
                    self.commit_pending_text();
                    let pt: ScreenPt = logical_to_phys(pos, canvas, bounds);
                    if let Some(handle) = hit_test_handle(pos, sel_log, HANDLE_HIT_RADIUS) {
                        self.stage = OverlayStage::Adjusting {
                            rect,
                            drag: Some(DragMode::Resize {
                                handle,
                                original: rect,
                            }),
                        };
                    } else if sel_log.contains(pos) {
                        if let Some(tool) = self.active_tool {
                            self.begin_tool_at(tool, pt);
                        } else {
                            self.stage = OverlayStage::Adjusting {
                                rect,
                                drag: Some(DragMode::Move {
                                    grab_x: pt.x - rect.x,
                                    grab_y: pt.y - rect.y,
                                    original: rect,
                                }),
                            };
                        }
                    } else if self.active_tool.is_none() {
                        self.doc = AnnotationDoc::default();
                        self.stage = OverlayStage::Selecting {
                            anchor: pt,
                            current: pt,
                        };
                    }
                }
            }
        }

        None
    }

    fn begin_tool_at(&mut self, tool: AnnotationTool, pt: ScreenPt) {
        if tool == AnnotationTool::Text {
            self.pending_text = Some(PendingText {
                pos: pt,
                text: String::new(),
                focus_requested: false,
            });
        } else {
            self.in_progress = DraftAnnotation::begin(tool, pt, self.active_color, self.stroke_size);
        }
    }

    fn finalize_command(&mut self, command: SnipCommand) -> SnipOverlayResult {
        if command == SnipCommand::Cancel {
            return SnipOverlayResult::Cancelled;
        }
        self.commit_pending_text();
        let Some(rect) = self.stage.selection_rect() else {
            return SnipOverlayResult::Cancelled;
        };
        match self.capture.crop_to_rgba(rect) {
            Ok(mut cropped) => {
                composite_annotations(&mut cropped, &self.doc);
                SnipOverlayResult::Completed {
                    cropped,
                    command,
                    doc_revision: self.doc_revision,
                }
            }
            Err(err) => {
                log::warn!("Ignoring invalid snip crop: {err}");
                SnipOverlayResult::Cancelled
            }
        }
    }
}

fn set_overlay_cloaked(cloaked: bool) {
    let flag: i32 = i32::from(cloaked);
    // SAFETY: Standard DWM cloak attribute on our own uniquely titled overlay window.
    unsafe {
        if let Ok(hwnd) = FindWindowW(None, w!("SnipToolOverlayViewport"))
            && !hwnd.is_invalid()
        {
            let disable_transitions: i32 = 1;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_TRANSITIONS_FORCEDISABLED,
                (&raw const disable_transitions).cast(),
                std::mem::size_of_val(&disable_transitions) as u32,
            );
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_CLOAK,
                (&raw const flag).cast(),
                std::mem::size_of_val(&flag) as u32,
            );
        }
    }
}

/// Checks whether the egui/OpenGL canvas physical dimensions match the target virtual screen
/// within a tiny DPI rounding tolerance (4 physical px).
fn is_gl_canvas_synced(canvas_logical: Rect, ppp: f32, expected: ScreenRect) -> bool {
    let phys_w = (canvas_logical.width() * ppp).round() as i32;
    let phys_h = (canvas_logical.height() * ppp).round() as i32;
    (phys_w - expected.width).abs() <= 4 && (phys_h - expected.height).abs() <= 4
}

/// Computes the normalized texture UV rectangle for the sub-region of `capture_bounds`
/// physically covered by `window_rect`, guaranteeing 1:1 pixel alignment on mixed-DPI monitors.
fn compute_overlay_uv(window_rect: ScreenRect, capture_bounds: ScreenRect) -> Rect {
    if capture_bounds.width <= 0 || capture_bounds.height <= 0 {
        return Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
    }
    let w = capture_bounds.width as f32;
    let h = capture_bounds.height as f32;
    let u0 = ((window_rect.x - capture_bounds.x) as f32 / w).clamp(0.0, 1.0);
    let v0 = ((window_rect.y - capture_bounds.y) as f32 / h).clamp(0.0, 1.0);
    let u1 = ((window_rect.right() - capture_bounds.x) as f32 / w).clamp(0.0, 1.0);
    let v1 = ((window_rect.bottom() - capture_bounds.y) as f32 / h).clamp(0.0, 1.0);
    Rect::from_min_max(Pos2::new(u0, v0), Pos2::new(u1, v1))
}

/// Computes Win32 `GWL_STYLE` and `GWL_EXSTYLE` for a true borderless topmost tool overlay
/// (`WS_POPUP` + `WS_EX_TOOLWINDOW`, no `WS_CAPTION`/`WS_THICKFRAME`/`WS_EX_APPWINDOW`).
/// This prevents Windows tiling window managers (GlazeWM, komorebi, FancyZones) and Aero Snap
/// from hijacking, tiling, or shifting the multi-monitor overlay onto Monitor 1 on Alt+O.
fn compute_overlay_window_styles(cur_style: u32, cur_ex_style: u32) -> (u32, u32) {
    let strip_style = WS_CAPTION.0
        | WS_THICKFRAME.0
        | WS_MINIMIZEBOX.0
        | WS_MAXIMIZEBOX.0
        | WS_SYSMENU.0
        | WS_BORDER.0
        | WS_DLGFRAME.0;
    let new_style = (cur_style & !strip_style) | WS_POPUP.0 | WS_CLIPSIBLINGS.0 | WS_CLIPCHILDREN.0;
    let new_ex_style =
        (cur_ex_style & !WS_EX_APPWINDOW.0) | WS_EX_TOOLWINDOW.0 | WS_EX_TOPMOST.0;
    (new_style, new_ex_style)
}

fn apply_overlay_tool_popup_style(hwnd: windows::Win32::Foundation::HWND) {
    // SAFETY: Updates window styles on our own uniquely titled overlay HWND so tiling WMs
    // treat it as an unmanaged fullscreen overlay rather than a tileable application window.
    unsafe {
        let cur_style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
        let cur_ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        let (new_style, new_ex_style) = compute_overlay_window_styles(cur_style, cur_ex_style);
        if new_style != cur_style {
            let _ = SetWindowLongPtrW(hwnd, GWL_STYLE, new_style as isize);
        }
        if new_ex_style != cur_ex_style {
            let _ = SetWindowLongPtrW(hwnd, GWL_EXSTYLE, new_ex_style as isize);
        }
    }
}

/// Cloaks, applies unmanaged tool-popup styles, and pre-sizes the overlay HWND before making it
/// visible so neither DWM nor tiling WMs (GlazeWM/komorebi) flash or shift it on Alt+O.
pub fn prepare_overlay_window_cloaked(bounds: ScreenRect) {
    set_overlay_cloaked(true);
    // SAFETY: Configures styles and physical bounds on our own overlay viewport while cloaked.
    unsafe {
        if let Ok(hwnd) = FindWindowW(None, w!("SnipToolOverlayViewport"))
            && !hwnd.is_invalid()
        {
            apply_overlay_tool_popup_style(hwnd);
            let _ = SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                bounds.x,
                bounds.y,
                bounds.width,
                bounds.height,
                SWP_FRAMECHANGED | SWP_NOACTIVATE | SWP_NOSENDCHANGING,
            );
        }
    }
}

pub fn hide_overlay_viewport(ctx: &egui::Context) {
    set_overlay_cloaked(true);
    ctx.send_viewport_cmd_to(overlay_viewport_id(), ViewportCommand::Visible(false));
}

fn handle_cursor(handle: ResizeHandle) -> CursorIcon {
    match handle {
        ResizeHandle::Nw | ResizeHandle::Se => CursorIcon::ResizeNwSe,
        ResizeHandle::Ne | ResizeHandle::Sw => CursorIcon::ResizeNeSw,
        ResizeHandle::N | ResizeHandle::S => CursorIcon::ResizeVertical,
        ResizeHandle::E | ResizeHandle::W => CursorIcon::ResizeHorizontal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_and_active_overlay_builders_never_trigger_window_recreation() {
        let mut hidden = SnipOverlaySession::hidden_viewport_builder();
        let active = SnipOverlaySession::active_viewport_builder();

        let (cmds, recreate_to_active) = hidden.patch(active.clone());
        assert!(
            !recreate_to_active,
            "switching overlay viewport from hidden to active must never recreate the native window"
        );
        assert!(
            !cmds.iter().any(|c| matches!(
                c,
                ViewportCommand::OuterPosition(_) | ViewportCommand::InnerSize(_)
            )),
            "active overlay builder must not emit DPI-skewed winit OuterPosition/InnerSize commands"
        );

        let mut active_builder = active;
        let (hide_cmds, recreate_to_hidden) =
            active_builder.patch(SnipOverlaySession::hidden_viewport_builder());
        assert!(
            !recreate_to_hidden,
            "switching overlay viewport from active to hidden must never recreate the native window"
        );
        assert!(
            !hide_cmds
                .iter()
                .any(|c| matches!(c, ViewportCommand::InnerSize(_))),
            "hiding overlay must preserve HWND size so subsequent Alt+O triggers never jump"
        );
    }

    #[test]
    fn gl_canvas_sync_check_tolerates_dpi_rounding_and_rejects_stale_sizes() {
        let expected = ScreenRect {
            x: -1080,
            y: 0,
            width: 3960,
            height: 1920,
        };
        // Exact match at 1.25 DPI (Monitor 2)
        let exact = Rect::from_min_size(Pos2::ZERO, Vec2::new(3960.0 / 1.25, 1920.0 / 1.25));
        assert!(is_gl_canvas_synced(exact, 1.25, expected));

        // Stale 1x1 or primary-only size must be rejected so overlay stays cloaked
        let stale_primary = Rect::from_min_size(Pos2::ZERO, Vec2::new(2880.0 / 1.75, 1800.0 / 1.75));
        assert!(!is_gl_canvas_synced(stale_primary, 1.25, expected));
        assert!(!is_gl_canvas_synced(stale_primary, 1.75, expected));
    }

    #[test]
    fn overlay_styles_mark_tool_popup_and_strip_tileable_overlapped_flags() {
        let winit_default_style =
            WS_CAPTION.0 | WS_THICKFRAME.0 | WS_SYSMENU.0 | WS_MINIMIZEBOX.0 | WS_MAXIMIZEBOX.0;
        let winit_default_ex = WS_EX_APPWINDOW.0;

        let (style, ex) = compute_overlay_window_styles(winit_default_style, winit_default_ex);
        assert_ne!(style & WS_POPUP.0, 0, "must set WS_POPUP");
        assert_eq!(style & WS_CAPTION.0, 0, "must clear WS_CAPTION");
        assert_eq!(style & WS_THICKFRAME.0, 0, "must clear WS_THICKFRAME");
        assert_eq!(style & WS_SYSMENU.0, 0, "must clear WS_SYSMENU");
        assert_ne!(ex & WS_EX_TOOLWINDOW.0, 0, "must set WS_EX_TOOLWINDOW");
        assert_ne!(ex & WS_EX_TOPMOST.0, 0, "must set WS_EX_TOPMOST");
        assert_eq!(ex & WS_EX_APPWINDOW.0, 0, "must clear WS_EX_APPWINDOW");
    }

    #[test]
    fn overlay_uv_maps_exact_physical_subregion_on_mixed_dpi_dual_monitors() {
        let virtual_desktop = ScreenRect {
            x: -1080,
            y: 0,
            width: 3960,
            height: 1920,
        };
        // Full virtual desktop match -> exact 0..1 UVs
        let full = compute_overlay_uv(virtual_desktop, virtual_desktop);
        assert_eq!(full, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)));

        // If OS temporarily clamped window to primary monitor (0, 0, 2880, 1800),
        // UV must start at 1080/3960 so Monitor 2 never bleeds onto Monitor 1.
        let primary_only = ScreenRect {
            x: 0,
            y: 0,
            width: 2880,
            height: 1800,
        };
        let sub = compute_overlay_uv(primary_only, virtual_desktop);
        assert!((sub.min.x - (1080.0 / 3960.0)).abs() < 1e-5);
        assert!((sub.max.x - 1.0).abs() < 1e-5);
        assert!((sub.min.y - 0.0).abs() < 1e-5);
        assert!((sub.max.y - (1800.0 / 1920.0)).abs() < 1e-5);
    }
}
