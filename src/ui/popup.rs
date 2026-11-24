use crate::capture::{capture_virtual_screen, CroppedImage, ScreenRect};
use crate::clipboard;
use crate::config::{self, AppConfig};
use crate::pin::PinManager;
use crate::prefetch::{ActionAttach, OcrAttach, PrefetchManager};
use crate::providers::{build_http_client, SnipAction, SnipError, SnipOutput};
use crate::snip::save;
use crate::snip::{
    execute_snip_command, hide_overlay_viewport, overlay_viewport_id,
    prepare_overlay_window_cloaked, ActionOutcome, SnipCommand, SnipMode, SnipOverlayResult,
    SnipOverlaySession,
};
use crate::ui::popup_view;
use crate::ui::theme;
use crate::ui::tray::TrayController;
use crate::ui::window;
use crate::ui::{EguiContextSlot, UiEvent};
use egui::{Key, Modifiers, Pos2, Vec2, ViewportCommand};
use egui_commonmark::CommonMarkCache;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub(crate) const POPUP_WIDTH: f32 = 432.0;
pub(crate) const POPUP_HEIGHT_MAX_RESULT: f32 = 460.0;
const POPUP_HEIGHT_LOADING: f32 = 96.0;
const POPUP_HEIGHT_TOAST: f32 = 114.0;
const FOCUS_GRACE_PERIOD: Duration = Duration::from_millis(380);
const TOAST_AUTO_DISMISS: Duration = Duration::from_secs(4);
const POPUP_HIDE_CAPTURE_DELAY: Duration = Duration::from_millis(55);
pub(crate) const COPY_FEEDBACK_DURATION: Duration = Duration::from_millis(1600);

pub(crate) enum PopupContent {
    Hidden,
    Loading {
        action: SnipAction,
    },
    Result {
        output: SnipOutput,
        is_streaming: bool,
    },
    Ocr {
        text: String,
    },
    Error {
        title: String,
        message: String,
    },
    Toast {
        title: String,
        message: String,
        is_warning: bool,
        expires_at: Instant,
    },
}

pub struct SnipPopupApp {
    pub(crate) config: AppConfig,
    config_path: PathBuf,
    ui_tx: Sender<UiEvent>,
    ui_rx: Receiver<UiEvent>,
    egui_slot: EguiContextSlot,
    tokio_handle: tokio::runtime::Handle,
    http: reqwest::Client,
    tray_controller: TrayController,

    snip_overlay: Option<SnipOverlaySession>,
    pins: PinManager,
    prefetch: PrefetchManager,
    overlay_vp_registered: bool,
    pending_capture: Option<(SnipMode, Instant)>,
    visible: bool,
    dwm_styled: bool,
    pub(crate) current_height: f32,
    pub(crate) pinned: bool,
    pub(crate) ocr_expanded: bool,
    was_focused: bool,
    opened_at: Instant,
    pub(crate) copied_at: Option<Instant>,
    pub(crate) ocr_copied_at: Option<Instant>,

    pub(crate) request_id: u64,
    ocr_copy_request_id: u64,
    pub(crate) active_action: SnipAction,
    pub(crate) last_crop: Option<(CroppedImage, u64)>,
    pub(crate) content: PopupContent,
    pub(crate) md_cache: CommonMarkCache,
}

impl SnipPopupApp {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        config: AppConfig,
        config_path: PathBuf,
        ui_tx: Sender<UiEvent>,
        ui_rx: Receiver<UiEvent>,
        egui_slot: EguiContextSlot,
        tokio_handle: tokio::runtime::Handle,
        tray_controller: TrayController,
    ) -> Self {
        Self::with_context(
            &cc.egui_ctx,
            config,
            config_path,
            ui_tx,
            ui_rx,
            egui_slot,
            tokio_handle,
            tray_controller,
        )
    }

    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new_for_test(
        ctx: &egui::Context,
        config: AppConfig,
        config_path: PathBuf,
        ui_tx: Sender<UiEvent>,
        ui_rx: Receiver<UiEvent>,
        egui_slot: EguiContextSlot,
        tokio_handle: tokio::runtime::Handle,
        tray_controller: TrayController,
    ) -> Self {
        Self::with_context(
            ctx,
            config,
            config_path,
            ui_tx,
            ui_rx,
            egui_slot,
            tokio_handle,
            tray_controller,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn with_context(
        ctx: &egui::Context,
        config: AppConfig,
        config_path: PathBuf,
        ui_tx: Sender<UiEvent>,
        ui_rx: Receiver<UiEvent>,
        egui_slot: EguiContextSlot,
        tokio_handle: tokio::runtime::Handle,
        tray_controller: TrayController,
    ) -> Self {
        theme::apply_theme(ctx);
        let _ = egui_slot.set(ctx.clone());

        Self {
            config,
            config_path,
            ui_tx,
            ui_rx,
            egui_slot,
            tokio_handle,
            http: build_http_client(),
            tray_controller,
            snip_overlay: None,
            pins: PinManager::default(),
            prefetch: PrefetchManager::new(),
            overlay_vp_registered: false,
            pending_capture: None,
            visible: false,
            dwm_styled: false,
            current_height: 160.0,
            pinned: false,
            ocr_expanded: false,
            was_focused: false,
            opened_at: Instant::now(),
            copied_at: None,
            ocr_copied_at: None,
            request_id: 0,
            ocr_copy_request_id: 0,
            active_action: SnipAction::Translate,
            last_crop: None,
            content: PopupContent::Hidden,
            md_cache: CommonMarkCache::default(),
        }
    }

    fn apply_dwm_styling_once(&mut self) {
        if !self.dwm_styled && window::apply_popup_dwm_styling() {
            self.dwm_styled = true;
        }
    }

    pub(crate) fn hide_popup(&mut self, ctx: &egui::Context) {
        self.visible = false;
        self.was_focused = false;
        self.content = PopupContent::Hidden;
        self.prefetch.cancel_all();
        ctx.send_viewport_cmd(ViewportCommand::Visible(false));
    }

    fn show_popup_at(&mut self, ctx: &egui::Context, pos_logical: Pos2, size_logical: Vec2) {
        self.visible = true;
        self.was_focused = false;
        self.opened_at = Instant::now();
        self.current_height = size_logical.y;
        ctx.send_viewport_cmd(ViewportCommand::InnerSize(size_logical));
        ctx.send_viewport_cmd(ViewportCommand::OuterPosition(pos_logical));
        ctx.send_viewport_cmd(ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(ViewportCommand::Focus);
        let _ = window::apply_popup_dwm_styling();
        ctx.request_repaint();
        ctx.request_repaint_after(Duration::from_millis(28));
    }

    /// Snaps popup window height to the exact measured height of rendered content (zero clipping, zero dead void).
    pub(crate) fn sync_measured_height(&mut self, ctx: &egui::Context, measured_total_h: f32) {
        let target = measured_total_h.clamp(84.0, POPUP_HEIGHT_MAX_RESULT);
        if (target - self.current_height).abs() > 3.0 {
            self.current_height = target;
            ctx.send_viewport_cmd(ViewportCommand::InnerSize(Vec2::new(POPUP_WIDTH, target)));
            ctx.request_repaint();
        }
    }

    pub(crate) fn is_short_ocr(raw: &str) -> bool {
        window::is_short_ocr(raw)
    }

    pub(crate) fn toggle_ocr_expanded(&mut self, ctx: &egui::Context) {
        self.ocr_expanded = !self.ocr_expanded;
        if let PopupContent::Result { ref output, .. } = self.content {
            let new_h = window::estimate_result_height(output, self.ocr_expanded);
            ctx.send_viewport_cmd(ViewportCommand::InnerSize(Vec2::new(POPUP_WIDTH, new_h)));
            ctx.request_repaint();
        }
    }

    fn compute_popup_pos(
        &self,
        ctx: &egui::Context,
        anchor: Option<ScreenRect>,
        height: f32,
    ) -> Pos2 {
        window::compute_popup_pos(ctx, anchor, height)
    }

    pub(crate) fn trigger_snip_overlay(&mut self, ctx: &egui::Context) {
        self.request_snip(ctx, SnipMode::Interactive);
    }

    pub(crate) fn open_config(&self) {
        window::open_path_in_shell(&self.config_path);
    }

    fn request_snip(&mut self, ctx: &egui::Context, mode: SnipMode) {
        if self.snip_overlay.is_some() || self.pending_capture.is_some() {
            log::info!("Snip overlay already active or pending; ignoring duplicate trigger");
            return;
        }
        log::info!("Requesting snip overlay (mode={mode:?}, visible={})", self.visible);
        if self.visible {
            self.hide_popup(ctx);
            self.pending_capture = Some((mode, Instant::now() + POPUP_HIDE_CAPTURE_DELAY));
            ctx.request_repaint_after(POPUP_HIDE_CAPTURE_DELAY);
        } else if !self.overlay_vp_registered {
            self.pending_capture = Some((mode, Instant::now() + Duration::from_millis(30)));
            ctx.request_repaint_after(Duration::from_millis(30));
        } else {
            self.start_snip_overlay_now(ctx, mode);
        }
    }

    fn poll_pending_capture(&mut self, ctx: &egui::Context) {
        let Some((mode, ready_at)) = self.pending_capture else {
            return;
        };
        let now = Instant::now();
        if now < ready_at {
            ctx.request_repaint_after(ready_at.saturating_duration_since(now));
            return;
        }
        if !self.overlay_vp_registered {
            ctx.request_repaint_after(Duration::from_millis(20));
            return;
        }
        self.pending_capture = None;
        self.start_snip_overlay_now(ctx, mode);
    }

    fn start_snip_overlay_now(&mut self, ctx: &egui::Context, mode: SnipMode) {
        self.prefetch.cancel_all();
        if let Ok(latest) = config::load_or_create_at(&self.config_path) {
            self.config = latest;
        }
        match capture_virtual_screen() {
            Ok(capture) => {
                let b = capture.bounds();
                // Cloak + pre-size overlay HWND in exact physical pixels before showing so Windows
                // never flashes white and mixed-DPI winit logical commands never shift Monitor 2
                // onto Monitor 1.
                prepare_overlay_window_cloaked(b);
                let session = SnipOverlaySession::new(ctx, capture, mode);
                self.snip_overlay = Some(session);
                let vp = overlay_viewport_id();
                ctx.send_viewport_cmd_to(vp, ViewportCommand::Visible(true));
                ctx.request_repaint();
            }
            Err(err) => {
                log::error!("Snip capture failed: {err}");
                self.show_toast(ctx, "Capture Error", err.to_string(), true, None);
            }
        }
    }

    fn show_toast(
        &mut self,
        ctx: &egui::Context,
        title: impl Into<String>,
        message: impl Into<String>,
        is_warning: bool,
        anchor: Option<ScreenRect>,
    ) {
        let duration = Duration::from_secs(if is_warning { 6 } else { 3 });
        self.content = PopupContent::Toast {
            title: title.into(),
            message: message.into(),
            is_warning,
            expires_at: Instant::now() + duration,
        };
        let h = if is_warning { 132.0 } else { POPUP_HEIGHT_TOAST };
        let pos = self.compute_popup_pos(ctx, anchor, h);
        self.show_popup_at(ctx, pos, Vec2::new(POPUP_WIDTH, h));
    }

    fn handle_overlay_result(&mut self, ctx: &egui::Context, result: SnipOverlayResult) {
        match result {
            SnipOverlayResult::Cancelled => {
                self.prefetch.cancel_all();
            }
            SnipOverlayResult::Completed {
                cropped,
                command,
                doc_revision,
            } => {
                self.execute_cropped_command(ctx, cropped, command, doc_revision);
            }
        }
    }

    fn present_ocr_result(
        &mut self,
        ctx: &egui::Context,
        screen_rect: ScreenRect,
        result: Result<String, String>,
    ) {
        let anchor = Some(screen_rect);
        match result {
            Ok(text) => {
                let trimmed = text.trim();
                if trimmed.is_empty() {
                    self.show_toast(
                        ctx,
                        "No Text Detected",
                        "Try selecting a larger or higher-contrast region.",
                        false,
                        anchor,
                    );
                } else {
                    Self::copy_to_clipboard(ctx, trimmed);
                    self.copied_at = Some(Instant::now());
                    let height = window::estimate_ocr_height(trimmed);
                    self.content = PopupContent::Ocr {
                        text: trimmed.to_string(),
                    };
                    let pos = self.compute_popup_pos(ctx, anchor, height);
                    self.show_popup_at(ctx, pos, Vec2::new(POPUP_WIDTH, height));
                }
            }
            Err(err) => self.show_toast(ctx, "OCR Failed", err, true, anchor),
        }
    }

    fn execute_cropped_command(
        &mut self,
        ctx: &egui::Context,
        cropped: CroppedImage,
        command: SnipCommand,
        doc_revision: u64,
    ) {
        if command == SnipCommand::OcrCopy {
            self.ocr_copy_request_id = self.ocr_copy_request_id.wrapping_add(1);
            let screen_rect = cropped.screen_rect;
            self.last_crop = Some((cropped.clone(), doc_revision));
            match self.prefetch.attach_or_take_ocr(
                screen_rect,
                doc_revision,
                self.ocr_copy_request_id,
                self.ui_tx.clone(),
                Arc::clone(&self.egui_slot),
            ) {
                OcrAttach::Ready(result) => {
                    self.present_ocr_result(ctx, screen_rect, result);
                    return;
                }
                OcrAttach::AttachedRunning => {
                    return;
                }
                OcrAttach::Miss => {}
            }
        } else if !matches!(command, SnipCommand::Translate | SnipCommand::ExplainCode) {
            self.prefetch.cancel_all();
        }

        match execute_snip_command(
            cropped,
            command,
            &self.config,
            &self.tokio_handle,
            self.ocr_copy_request_id,
            &self.ui_tx,
            &self.egui_slot,
        ) {
            ActionOutcome::None => {}
            ActionOutcome::PinImage(img) => {
                self.pins.add_pin(ctx, img);
            }
            ActionOutcome::Toast {
                title,
                message,
                is_warning,
            } => self.show_toast(ctx, title, message, is_warning, None),
            ActionOutcome::ProcessCrop { crop, action } => {
                self.last_crop = Some((crop, doc_revision));
                self.copied_at = None;
                self.ocr_copied_at = None;
                self.ocr_expanded = false;
                self.dispatch_processing(ctx, action, true);
            }
        }
    }

    fn render_overlay_viewport(&mut self, ctx: &egui::Context) {
        self.overlay_vp_registered = true;
        if self.snip_overlay.is_some() {
            let builder = self
                .snip_overlay
                .as_ref()
                .expect("checked is_some")
                .viewport_builder();
            let outcome = ctx.show_viewport_immediate(overlay_viewport_id(), builder, |ui, _| {
                self.snip_overlay.as_mut().and_then(|s| s.ui(ui))
            });
            if let Some(result) = outcome {
                self.snip_overlay = None;
                hide_overlay_viewport(ctx);
                self.handle_overlay_result(ctx, result);
            } else if let Some(ref overlay) = self.snip_overlay {
                // Speculatively pre-run local OCR (+ AI Translate/Explain when enabled) once
                // the selection settles so clicking any toolbar action is instant or live-streaming.
                let settled = overlay.settled_selection();
                let target_lang = self.config.normalized_target_language();
                if let Some(wait) = self.prefetch.observe_overlay_selection(
                    settled,
                    &target_lang,
                    Instant::now(),
                ) {
                    if wait.is_zero() {
                        if let Some((crop, rev)) = overlay.render_settled_crop() {
                            self.prefetch.start_speculative(
                                crop,
                                rev,
                                &self.config,
                                &self.http,
                                &self.tokio_handle,
                            );
                        }
                    } else {
                        ctx.request_repaint_after(wait);
                    }
                }
            }
        } else {
            ctx.show_viewport_immediate(
                overlay_viewport_id(),
                SnipOverlaySession::hidden_viewport_builder(),
                |_ui, _| {},
            );
        }
    }

    pub(crate) fn change_target_language(&mut self, ctx: &egui::Context, new_lang: &str) {
        if self.config.target_language.eq_ignore_ascii_case(new_lang) {
            return;
        }
        self.config.target_language = new_lang.to_string();
        if let Err(err) = self.config.save_to(&self.config_path) {
            log::warn!("Failed to persist updated target_language: {err}");
        }
        if self.last_crop.is_some() {
            self.dispatch_processing(ctx, self.active_action, false);
        }
    }

    pub(crate) fn dispatch_processing(
        &mut self,
        ctx: &egui::Context,
        action: SnipAction,
        reposition: bool,
    ) {
        let Some((crop, doc_revision)) = self.last_crop.clone() else {
            return;
        };

        self.request_id = self.request_id.wrapping_add(1);
        let req_id = self.request_id;
        self.active_action = action;

        let anchor = Some(crop.screen_rect);

        match self.prefetch.dispatch_or_attach_action(
            crop,
            doc_revision,
            action,
            &self.config,
            &self.http,
            &self.tokio_handle,
            req_id,
            self.ui_tx.clone(),
            Arc::clone(&self.egui_slot),
        ) {
            ActionAttach::Ready(output) => {
                // 0ms instant hit from speculative prefetch or per-region cache!
                self.ocr_expanded = output.primary_text.is_empty();
                let height = window::estimate_result_height(&output, self.ocr_expanded);
                self.content = PopupContent::Result {
                    output,
                    is_streaming: false,
                };
                if reposition || !self.visible {
                    let pos = self.compute_popup_pos(ctx, anchor, height);
                    self.show_popup_at(ctx, pos, Vec2::new(POPUP_WIDTH, height));
                } else {
                    self.current_height = height;
                    ctx.send_viewport_cmd(ViewportCommand::InnerSize(Vec2::new(
                        POPUP_WIDTH,
                        height,
                    )));
                    ctx.request_repaint();
                }
            }
            ActionAttach::AttachedStreaming {
                provider_label,
                text_so_far,
            } if !text_so_far.is_empty() => {
                let output = SnipOutput {
                    primary_text: text_so_far,
                    original_text: None,
                    detected_language: None,
                    provider_label,
                    warning: None,
                    is_markdown: action == SnipAction::ExplainCode,
                };
                let height = window::estimate_result_height(&output, false);
                self.content = PopupContent::Result {
                    output,
                    is_streaming: true,
                };
                if reposition || !self.visible {
                    let pos = self.compute_popup_pos(ctx, anchor, height);
                    self.show_popup_at(ctx, pos, Vec2::new(POPUP_WIDTH, height));
                } else {
                    self.current_height = height;
                    ctx.send_viewport_cmd(ViewportCommand::InnerSize(Vec2::new(
                        POPUP_WIDTH,
                        height,
                    )));
                    ctx.request_repaint();
                }
            }
            ActionAttach::AttachedStreaming { .. } | ActionAttach::Miss => {
                self.content = PopupContent::Loading { action };
                if reposition || !self.visible {
                    let pos = self.compute_popup_pos(ctx, anchor, POPUP_HEIGHT_LOADING);
                    self.show_popup_at(ctx, pos, Vec2::new(POPUP_WIDTH, POPUP_HEIGHT_LOADING));
                } else {
                    ctx.send_viewport_cmd(ViewportCommand::InnerSize(Vec2::new(
                        POPUP_WIDTH,
                        POPUP_HEIGHT_LOADING,
                    )));
                    ctx.request_repaint();
                }
            }
        }
    }

    fn process_events(&mut self, ctx: &egui::Context) {
        while let Ok(event) = self.ui_rx.try_recv() {
            match event {
                UiEvent::TriggerSnip(mode) => self.request_snip(ctx, mode),
                UiEvent::OcrCopyFinished {
                    request_id,
                    screen_rect,
                    result,
                } => {
                    if request_id != self.ocr_copy_request_id {
                        continue;
                    }
                    self.present_ocr_result(ctx, screen_rect, result);
                }
                UiEvent::ProcessingDelta {
                    request_id,
                    action,
                    provider_label,
                    text,
                } => {
                    if request_id != self.request_id {
                        continue;
                    }
                    self.active_action = action;
                    let anchor = self.last_crop.as_ref().map(|(c, _)| c.screen_rect);
                    let output = SnipOutput {
                        primary_text: text,
                        original_text: None,
                        detected_language: None,
                        provider_label,
                        warning: None,
                        is_markdown: action == SnipAction::ExplainCode,
                    };
                    let was_loading = matches!(self.content, PopupContent::Loading { .. });
                    let est_h = window::estimate_result_height(&output, false);
                    self.content = PopupContent::Result {
                        output,
                        is_streaming: true,
                    };
                    if !self.visible {
                        let pos = self.compute_popup_pos(ctx, anchor, est_h);
                        self.show_popup_at(ctx, pos, Vec2::new(POPUP_WIDTH, est_h));
                    } else if was_loading {
                        self.sync_measured_height(ctx, est_h);
                    }
                    ctx.request_repaint();
                }
                UiEvent::ProcessingFinished {
                    request_id,
                    action,
                    result,
                } => {
                    if request_id != self.request_id {
                        continue;
                    }
                    self.active_action = action;
                    let anchor = self.last_crop.as_ref().map(|(c, _)| c.screen_rect);
                    match result {
                        Ok(output) => {
                            self.ocr_expanded = output.primary_text.is_empty();
                            let height = window::estimate_result_height(&output, self.ocr_expanded);
                            let already_visible = self.visible;
                            self.content = PopupContent::Result {
                                output,
                                is_streaming: false,
                            };
                            if already_visible {
                                self.sync_measured_height(ctx, height);
                                ctx.request_repaint();
                            } else {
                                let pos = self.compute_popup_pos(ctx, anchor, height);
                                self.show_popup_at(ctx, pos, Vec2::new(POPUP_WIDTH, height));
                            }
                        }
                        Err(SnipError::NoTextDetected) => {
                            self.content = PopupContent::Toast {
                                title: "No text detected".to_string(),
                                message: "Try selecting a slightly larger or clearer region."
                                    .to_string(),
                                is_warning: false,
                                expires_at: Instant::now() + TOAST_AUTO_DISMISS,
                            };
                            let pos = self.compute_popup_pos(ctx, anchor, POPUP_HEIGHT_TOAST);
                            self.show_popup_at(ctx, pos, Vec2::new(POPUP_WIDTH, POPUP_HEIGHT_TOAST));
                        }
                        Err(SnipError::UnsupportedAction(msg)) => {
                            self.content = PopupContent::Error {
                                title: "API Key Required".to_string(),
                                message: msg,
                            };
                            let h = 176.0;
                            let pos = self.compute_popup_pos(ctx, anchor, h);
                            self.show_popup_at(ctx, pos, Vec2::new(POPUP_WIDTH, h));
                        }
                        Err(err) => {
                            self.content = PopupContent::Error {
                                title: "Couldn't Process Snip".to_string(),
                                message: err.to_string(),
                            };
                            let h = 176.0;
                            let pos = self.compute_popup_pos(ctx, anchor, h);
                            self.show_popup_at(ctx, pos, Vec2::new(POPUP_WIDTH, h));
                        }
                    }
                }
                UiEvent::ConfigUpdated { config, toast } => {
                    self.config = config;
                    self.prefetch.cancel_all();
                    if let Some(msg) = toast {
                        let is_warning = msg.to_ascii_lowercase().contains("hotkey");
                        self.content = PopupContent::Toast {
                            title: if is_warning {
                                "Hotkey Conflict".to_string()
                            } else {
                                "Config Reloaded".to_string()
                            },
                            message: msg,
                            is_warning,
                            expires_at: Instant::now() + Duration::from_secs(6),
                        };
                        let h = if is_warning { 138.0 } else { POPUP_HEIGHT_TOAST };
                        let pos = self.compute_popup_pos(ctx, None, h);
                        self.show_popup_at(ctx, pos, Vec2::new(POPUP_WIDTH, h));
                    }
                }
                UiEvent::ShowToast {
                    title,
                    message,
                    is_warning,
                } => {
                    self.content = PopupContent::Toast {
                        title,
                        message,
                        is_warning,
                        expires_at: Instant::now()
                            + Duration::from_secs(if is_warning { 6 } else { 4 }),
                    };
                    let h = if is_warning { 138.0 } else { POPUP_HEIGHT_TOAST };
                    let pos = self.compute_popup_pos(ctx, None, h);
                    self.show_popup_at(ctx, pos, Vec2::new(POPUP_WIDTH, h));
                }
                UiEvent::OpenConfigRequested(path) => {
                    window::open_path_in_shell(&path);
                }
                UiEvent::OpenScreenshotsFolder => {
                    match save::resolve_save_dir(self.config.save_dir.as_deref()) {
                        Ok(dir) => {
                            let _ = std::fs::create_dir_all(&dir);
                            window::open_path_in_shell(&dir);
                        }
                        Err(err) => self.show_toast(
                            ctx,
                            "Screenshots Folder",
                            err.to_string(),
                            true,
                            None,
                        ),
                    }
                }
                UiEvent::CloseAllPins => {
                    self.pins.close_all(ctx);
                }
                UiEvent::Quit => {
                    self.pins.close_all(ctx);
                    self.tray_controller.request_shutdown();
                    ctx.send_viewport_cmd(ViewportCommand::Close);
                }
            }
        }
    }

    pub(crate) fn copy_primary_result(&mut self, ctx: &egui::Context) {
        let payload = match &self.content {
            PopupContent::Result { output, .. } => {
                if !output.primary_text.is_empty() {
                    Some(output.primary_text.clone())
                } else {
                    output.original_text.clone()
                }
            }
            PopupContent::Ocr { text } if !text.is_empty() => Some(text.clone()),
            _ => None,
        };
        if let Some(text) = payload {
            Self::copy_to_clipboard(ctx, &text);
            self.copied_at = Some(Instant::now());
        }
    }

    pub(crate) fn copy_to_clipboard(ctx: &egui::Context, text: &str) {
        if clipboard::copy_text(text).is_ok() {
            return;
        }
        ctx.copy_text(text.to_string());
    }
}

impl eframe::App for SnipPopupApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        theme::CLEAR_COLOR_RGBA
    }

    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.process_events(ctx);
        self.poll_pending_capture(ctx);
        let pin_events = self.pins.drain_pending_events(ctx);
        for ev in pin_events {
            self.execute_cropped_command(ctx, ev.cropped, ev.command, 0);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.process_events(&ctx);
        self.poll_pending_capture(&ctx);
        self.render_overlay_viewport(&ctx);

        let pin_events = self.pins.render_all(&ctx);
        for ev in pin_events {
            self.execute_cropped_command(&ctx, ev.cropped, ev.command, 0);
        }

        if !self.visible {
            return;
        }
        self.apply_dwm_styling_once();

        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.hide_popup(&ctx);
            return;
        }

        if ctx.input(|i| i.modifiers.matches_logically(Modifiers::CTRL) && i.key_pressed(Key::C)) {
            self.copy_primary_result(&ctx);
        }

        if matches!(self.content, PopupContent::Loading { .. }) {
            ctx.request_repaint_after(Duration::from_millis(24));
        }

        if let PopupContent::Toast { expires_at, .. } = self.content {
            let hovered = ctx.input(|i| i.pointer.has_pointer());
            if !self.pinned && !hovered {
                if Instant::now() >= expires_at {
                    self.hide_popup(&ctx);
                    return;
                }
                let remaining = expires_at.saturating_duration_since(Instant::now());
                ctx.request_repaint_after(remaining.min(Duration::from_millis(200)));
            }
        }

        if self
            .copied_at
            .map(|t| t.elapsed() < COPY_FEEDBACK_DURATION)
            .unwrap_or(false)
            || self
                .ocr_copied_at
                .map(|t| t.elapsed() < COPY_FEEDBACK_DURATION)
                .unwrap_or(false)
        {
            ctx.request_repaint_after(Duration::from_millis(160));
        }

        let is_focused = ctx.input(|i| i.viewport().focused).unwrap_or(false);
        if is_focused {
            self.was_focused = true;
        } else if !self.pinned
            && self.was_focused
            && self.opened_at.elapsed() > FOCUS_GRACE_PERIOD
        {
            self.hide_popup(&ctx);
            return;
        }

        popup_view::render_popup(self, ui, &ctx);
    }
}
