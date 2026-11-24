use crate::capture::{CroppedImage, ScreenRect};
use crate::config::AppConfig;
use crate::ocr;
use crate::providers::{
    resolve_provider_kind, select_provider, ProviderKind, SnipAction, SnipError, SnipInput,
    SnipOutput,
};
use crate::ui::{wake_ui, EguiContextSlot, UiEvent};
use std::collections::HashMap;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::watch;
use tokio::task::JoinHandle;

/// Delay after mouse-up before speculative prefetch starts so micro-adjustments don't waste work.
const SETTLE_DEBOUNCE: Duration = Duration::from_millis(120);
/// Brief stagger before speculative Explain so strict free-tier endpoints avoid same-ms burst 429s.
const EXPLAIN_SPECULATIVE_STAGGER: Duration = Duration::from_millis(120);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PrefetchKey {
    pub rect: ScreenRect,
    pub doc_revision: u64,
    pub target_language: String,
}

#[derive(Debug)]
pub enum ActionAttach {
    /// Cached successful result ready immediately (0ms).
    Ready(SnipOutput),
    /// Already in flight; subscriber attached so future deltas + completion flow to `ui_tx`.
    AttachedStreaming {
        provider_label: String,
        text_so_far: String,
    },
    /// Not started yet or speculative run failed; caller should start a fresh live request.
    Miss,
}

#[derive(Debug)]
pub enum OcrAttach {
    /// Precomputed local OCR ready immediately (0ms).
    Ready(Result<String, String>),
    /// Already running; waiter attached so completion fires `UiEvent::OcrCopyFinished`.
    AttachedRunning,
    /// Not started yet; caller should start a fresh OCR run.
    Miss,
}

struct OcrWaiter {
    request_id: u64,
    screen_rect: ScreenRect,
    ui_tx: Sender<UiEvent>,
    egui_slot: EguiContextSlot,
}

enum OcrSlot {
    Running { waiters: Vec<OcrWaiter> },
    Ready(Result<String, String>),
}

struct ActionSubscriber {
    request_id: u64,
    action: SnipAction,
    ui_tx: Sender<UiEvent>,
    egui_slot: EguiContextSlot,
}

enum ActionSlot {
    Running {
        provider_label: String,
        accumulated: String,
        subscriber: Option<ActionSubscriber>,
    },
    Ready(Result<SnipOutput, SnipError>),
}

struct SessionState {
    ocr: Option<OcrSlot>,
    ocr_watch_rx: watch::Receiver<Option<Result<String, String>>>,
    vision_jpeg: Option<Arc<Vec<u8>>>,
    actions: HashMap<SnipAction, ActionSlot>,
}

struct ActiveSession {
    key: PrefetchKey,
    state: Arc<Mutex<SessionState>>,
    tasks: Vec<JoinHandle<()>>,
}

impl ActiveSession {
    fn abort_all(&mut self) {
        for handle in self.tasks.drain(..) {
            handle.abort();
        }
    }
}

impl Drop for ActiveSession {
    fn drop(&mut self) {
        self.abort_all();
    }
}

/// Manages debounced background prefetching during selection settle + live-stream joining +
/// instant per-region caching for `OCR`, `Translate`, and `Explain Code`.
pub struct PrefetchManager {
    pending_selection: Option<(PrefetchKey, Instant)>,
    active: Option<ActiveSession>,
}

impl Default for PrefetchManager {
    fn default() -> Self {
        Self::new()
    }
}

impl PrefetchManager {
    pub fn new() -> Self {
        Self {
            pending_selection: None,
            active: None,
        }
    }

    /// Observes the overlay's current settled selection `(rect, doc_revision)`.
    /// Returns `true` when the selection has been steady long enough (`SETTLE_DEBOUNCE`)
    /// and needs `start_speculative(...)` called with a fresh crop.
    pub fn observe_overlay_selection(
        &mut self,
        settled: Option<(ScreenRect, u64)>,
        target_language: &str,
        now: Instant,
    ) -> Option<Duration> {
        let Some((rect, doc_revision)) = settled else {
            self.pending_selection = None;
            if self.active.is_some() {
                self.cancel_all();
            }
            return None;
        };

        let key = PrefetchKey {
            rect,
            doc_revision,
            target_language: target_language.to_string(),
        };

        if self.active.as_ref().is_some_and(|s| s.key == key) {
            self.pending_selection = None;
            return None;
        }

        if self.active.is_some() {
            self.cancel_all();
        }

        match &self.pending_selection {
            Some((pending_key, since)) if *pending_key == key => {
                let elapsed = now.saturating_duration_since(*since);
                if elapsed >= SETTLE_DEBOUNCE {
                    Some(Duration::ZERO)
                } else {
                    Some(SETTLE_DEBOUNCE - elapsed)
                }
            }
            _ => {
                self.pending_selection = Some((key, now));
                Some(SETTLE_DEBOUNCE)
            }
        }
    }

    pub fn cancel_all(&mut self) {
        self.pending_selection = None;
        if let Some(mut session) = self.active.take() {
            session.abort_all();
        }
    }

    /// Starts speculative background prefetch for a settled crop:
    /// 1) Always starts local direct-RGBA Windows OCR (100% offline, instant OCR).
    /// 2) When `config.speculative_prefetch` is enabled, starts streaming `Translate`
    ///    and (if BYOK key is configured) `ExplainCode` in parallel.
    pub fn start_speculative(
        &mut self,
        crop: CroppedImage,
        doc_revision: u64,
        config: &AppConfig,
        http: &reqwest::Client,
        tokio_handle: &tokio::runtime::Handle,
    ) {
        let target_language = config.normalized_target_language();
        let key = PrefetchKey {
            rect: crop.screen_rect,
            doc_revision,
            target_language: target_language.clone(),
        };
        self.pending_selection = None;

        if self.active.as_ref().is_some_and(|s| s.key == key) {
            return;
        }
        self.cancel_all();

        let (ocr_watch_tx, ocr_watch_rx) = watch::channel(None);
        let state = Arc::new(Mutex::new(SessionState {
            ocr: Some(OcrSlot::Running {
                waiters: Vec::new(),
            }),
            ocr_watch_rx,
            vision_jpeg: None,
            actions: HashMap::new(),
        }));

        let crop_arc = Arc::new(crop);
        let mut tasks = Vec::new();

        tasks.push(spawn_ocr_task(
            Arc::clone(&crop_arc),
            Arc::clone(&state),
            ocr_watch_tx,
            tokio_handle,
        ));

        if config.speculative_prefetch {
            let translate_handle = spawn_action_task(
                Arc::clone(&crop_arc),
                SnipAction::Translate,
                config.clone(),
                http.clone(),
                Arc::clone(&state),
                None,
                Duration::ZERO,
                tokio_handle,
            );
            tasks.push(translate_handle);

            if config.has_byok_key() {
                let explain_handle = spawn_action_task(
                    Arc::clone(&crop_arc),
                    SnipAction::ExplainCode,
                    config.clone(),
                    http.clone(),
                    Arc::clone(&state),
                    None,
                    EXPLAIN_SPECULATIVE_STAGGER,
                    tokio_handle,
                );
                tasks.push(explain_handle);
            }
        }

        self.active = Some(ActiveSession { key, state, tasks });
    }

    /// Tries to satisfy an `OCR` click from the active prefetched session (0ms if ready,
    /// or attaches to the already-running OCR task without starting a second one).
    pub fn attach_or_take_ocr(
        &mut self,
        rect: ScreenRect,
        doc_revision: u64,
        request_id: u64,
        ui_tx: Sender<UiEvent>,
        egui_slot: EguiContextSlot,
    ) -> OcrAttach {
        let Some(session) = self.active.as_ref() else {
            return OcrAttach::Miss;
        };
        if session.key.rect != rect || session.key.doc_revision != doc_revision {
            return OcrAttach::Miss;
        }
        let Ok(mut guard) = session.state.lock() else {
            return OcrAttach::Miss;
        };
        match guard.ocr.as_mut() {
            Some(OcrSlot::Ready(res)) => OcrAttach::Ready(res.clone()),
            Some(OcrSlot::Running { waiters }) => {
                waiters.push(OcrWaiter {
                    request_id,
                    screen_rect: rect,
                    ui_tx,
                    egui_slot,
                });
                OcrAttach::AttachedRunning
            }
            None => OcrAttach::Miss,
        }
    }

    /// Starts or attaches to an action (`Translate` / `ExplainCode`) for the given crop.
    /// - If cached `Ok(output)` exists for `(rect, doc_revision, target_language)` -> `Ready` (0ms).
    /// - If already streaming in flight -> `AttachedStreaming` (live takeover with accumulated text).
    /// - Otherwise -> launches a fresh streaming task (reusing cached OCR + vision JPEG if same crop).
    #[allow(clippy::too_many_arguments)]
    pub fn dispatch_or_attach_action(
        &mut self,
        crop: CroppedImage,
        doc_revision: u64,
        action: SnipAction,
        config: &AppConfig,
        http: &reqwest::Client,
        tokio_handle: &tokio::runtime::Handle,
        request_id: u64,
        ui_tx: Sender<UiEvent>,
        egui_slot: EguiContextSlot,
    ) -> ActionAttach {
        let target_language = config.normalized_target_language();
        let key = PrefetchKey {
            rect: crop.screen_rect,
            doc_revision,
            target_language: target_language.clone(),
        };

        let matches_active = self.active.as_ref().is_some_and(|s| s.key == key);
        if matches_active {
            let session = self.active.as_ref().expect("checked");
            if let Ok(mut guard) = session.state.lock() {
                match guard.actions.get_mut(&action) {
                    Some(ActionSlot::Ready(Ok(output))) => {
                        let mut out = output.clone();
                        if out.original_text.is_none()
                            && let Some(OcrSlot::Ready(Ok(ref ocr_text))) = guard.ocr
                        {
                            let trimmed = ocr_text.trim();
                            if !trimmed.is_empty() {
                                out.original_text = Some(trimmed.to_string());
                            }
                        }
                        return ActionAttach::Ready(out);
                    }
                    Some(ActionSlot::Running {
                        provider_label,
                        accumulated,
                        subscriber,
                    }) => {
                        *subscriber = Some(ActionSubscriber {
                            request_id,
                            action,
                            ui_tx,
                            egui_slot,
                        });
                        return ActionAttach::AttachedStreaming {
                            provider_label: provider_label.clone(),
                            text_so_far: accumulated.clone(),
                        };
                    }
                    // Either never started or previous speculative attempt failed (e.g. 429) -> retry fresh!
                    Some(ActionSlot::Ready(Err(_))) | None => {}
                }
            }

            let session = self.active.as_mut().expect("checked");
            let state = Arc::clone(&session.state);
            let subscriber = ActionSubscriber {
                request_id,
                action,
                ui_tx,
                egui_slot,
            };
            let handle = spawn_action_task(
                Arc::new(crop),
                action,
                config.clone(),
                http.clone(),
                state,
                Some(subscriber),
                Duration::ZERO,
                tokio_handle,
            );
            session.tasks.push(handle);
            return ActionAttach::Miss;
        }

        // Brand-new crop or changed target_language: preserve OCR + vision JPEG if same crop!
        let preserved = self.active.as_ref().and_then(|s| {
            if s.key.rect == rect_of(&crop) && s.key.doc_revision == doc_revision {
                s.state.lock().ok().map(|g| {
                    let ocr_ready = match &g.ocr {
                        Some(OcrSlot::Ready(res)) => Some(res.clone()),
                        _ => None,
                    };
                    (ocr_ready, g.vision_jpeg.clone())
                })
            } else {
                None
            }
        });

        self.cancel_all();

        let (ocr_watch_tx, ocr_watch_rx) = watch::channel(
            preserved
                .as_ref()
                .and_then(|(ocr_ready, _)| ocr_ready.clone()),
        );
        let initial_ocr_slot = preserved
            .as_ref()
            .and_then(|(ocr_ready, _)| ocr_ready.clone())
            .map(OcrSlot::Ready);
        let initial_jpeg = preserved.and_then(|(_, jpeg)| jpeg);

        let needs_ocr_spawn = initial_ocr_slot.is_none();
        let state = Arc::new(Mutex::new(SessionState {
            ocr: Some(initial_ocr_slot.unwrap_or(OcrSlot::Running {
                waiters: Vec::new(),
            })),
            ocr_watch_rx,
            vision_jpeg: initial_jpeg,
            actions: HashMap::new(),
        }));

        let crop_arc = Arc::new(crop);
        let mut tasks = Vec::new();
        if needs_ocr_spawn {
            tasks.push(spawn_ocr_task(
                Arc::clone(&crop_arc),
                Arc::clone(&state),
                ocr_watch_tx,
                tokio_handle,
            ));
        }

        let subscriber = ActionSubscriber {
            request_id,
            action,
            ui_tx,
            egui_slot,
        };
        tasks.push(spawn_action_task(
            Arc::clone(&crop_arc),
            action,
            config.clone(),
            http.clone(),
            Arc::clone(&state),
            Some(subscriber),
            Duration::ZERO,
            tokio_handle,
        ));

        self.active = Some(ActiveSession { key, state, tasks });
        ActionAttach::Miss
    }
}

fn rect_of(crop: &CroppedImage) -> ScreenRect {
    crop.screen_rect
}

fn spawn_ocr_task(
    crop: Arc<CroppedImage>,
    state: Arc<Mutex<SessionState>>,
    ocr_watch_tx: watch::Sender<Option<Result<String, String>>>,
    tokio_handle: &tokio::runtime::Handle,
) -> JoinHandle<()> {
    tokio_handle.spawn(async move {
        let crop_for_ocr = Arc::clone(&crop);
        let res = tokio::task::spawn_blocking(move || {
            ocr::extract_text_from_rgba(crop_for_ocr.width, crop_for_ocr.height, &crop_for_ocr.rgba)
        })
        .await
        .map_err(|e| format!("OCR task failed: {e}"))
        .and_then(|inner| inner.map_err(|e| e.to_string()));

        let _ = ocr_watch_tx.send(Some(res.clone()));

        let waiters = {
            if let Ok(mut guard) = state.lock() {
                let prev = guard.ocr.replace(OcrSlot::Ready(res.clone()));
                match prev {
                    Some(OcrSlot::Running { waiters }) => waiters,
                    _ => Vec::new(),
                }
            } else {
                Vec::new()
            }
        };

        for waiter in waiters {
            let _ = waiter.ui_tx.send(UiEvent::OcrCopyFinished {
                request_id: waiter.request_id,
                screen_rect: waiter.screen_rect,
                result: res.clone(),
            });
            wake_ui(&waiter.egui_slot);
        }
    })
}

#[allow(clippy::too_many_arguments)]
fn spawn_action_task(
    crop: Arc<CroppedImage>,
    action: SnipAction,
    config: AppConfig,
    http: reqwest::Client,
    state: Arc<Mutex<SessionState>>,
    initial_subscriber: Option<ActionSubscriber>,
    stagger: Duration,
    tokio_handle: &tokio::runtime::Handle,
) -> JoinHandle<()> {
    let provider = select_provider(&config, action, http);
    let provider_kind = resolve_provider_kind(&config, action);
    let provider_label = provider.name().to_string();
    let target_language = config.normalized_target_language();

    if let Ok(mut guard) = state.lock() {
        guard.actions.insert(
            action,
            ActionSlot::Running {
                provider_label: provider_label.clone(),
                accumulated: String::new(),
                subscriber: initial_subscriber,
            },
        );
    }

    tokio_handle.spawn(async move {
        if !stagger.is_zero() {
            tokio::time::sleep(stagger).await;
        }

        let (input_res, ocr_rx) = match provider_kind {
            ProviderKind::Free => {
                // Free Google Translate only needs local OCR text (zero PNG/JPEG encoding!).
                let mut rx = {
                    let Ok(guard) = state.lock() else {
                        return;
                    };
                    guard.ocr_watch_rx.clone()
                };
                let ocr_result = loop {
                    if let Some(val) = rx.borrow().clone() {
                        break val;
                    }
                    if rx.changed().await.is_err() {
                        break Err("OCR cancelled".to_string());
                    }
                };
                let inp = match ocr_result {
                    Ok(text) => Ok(SnipInput::from_jpeg(
                        Vec::new(),
                        Some(text),
                        action,
                        target_language,
                    )),
                    Err(err) => Err(SnipError::OcrFailed(err)),
                };
                (inp, None)
            }
            ProviderKind::Byok(_) => {
                let cached_jpeg = state
                    .lock()
                    .ok()
                    .and_then(|g| g.vision_jpeg.clone());

                let jpeg_res = if let Some(existing) = cached_jpeg {
                    Ok(existing)
                } else {
                    let crop_enc = Arc::clone(&crop);
                    let encoded = tokio::task::spawn_blocking(move || crop_enc.encode_vision_jpeg())
                        .await
                        .map_err(|e| SnipError::ProviderFailed(format!("JPEG task panicked: {e}")))
                        .and_then(|r| {
                            r.map(Arc::new)
                                .map_err(|e| SnipError::ProviderFailed(e.to_string()))
                        });
                    if let Ok(ref bytes) = encoded
                        && let Ok(mut guard) = state.lock()
                    {
                        guard.vision_jpeg = Some(Arc::clone(bytes));
                    }
                    encoded
                };

                let precomputed_ocr = state.lock().ok().and_then(|g| match &g.ocr {
                    Some(OcrSlot::Ready(Ok(text))) if !text.trim().is_empty() => {
                        Some(text.trim().to_string())
                    }
                    _ => None,
                });
                let rx = state.lock().ok().map(|g| g.ocr_watch_rx.clone());

                let inp = jpeg_res.map(|bytes| {
                    SnipInput::from_jpeg(
                        bytes.as_ref().clone(),
                        precomputed_ocr,
                        action,
                        target_language,
                    )
                });
                (inp, rx)
            }
        };

        let result = match input_res {
            Ok(input) => {
                let state_delta = Arc::clone(&state);
                let label_delta = provider_label.clone();
                let on_delta = Arc::new(move |chunk: &str| {
                    if chunk.is_empty() {
                        return;
                    }
                    let notify = if let Ok(mut guard) = state_delta.lock() {
                        if let Some(ActionSlot::Running {
                            accumulated,
                            subscriber,
                            ..
                        }) = guard.actions.get_mut(&action)
                        {
                            accumulated.push_str(chunk);
                            subscriber.as_ref().map(|sub| {
                                (
                                    sub.request_id,
                                    sub.action,
                                    sub.ui_tx.clone(),
                                    Arc::clone(&sub.egui_slot),
                                    accumulated.clone(),
                                )
                            })
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                    if let Some((request_id, act, tx, slot, full_so_far)) = notify {
                        let _ = tx.send(UiEvent::ProcessingDelta {
                            request_id,
                            action: act,
                            provider_label: label_delta.clone(),
                            text: full_so_far,
                        });
                        wake_ui(&slot);
                    }
                });

                let mut res = provider.process_streaming(input, on_delta).await;

                // If local OCR finished while AI was streaming, attach it without blocking!
                if let Ok(ref mut out) = res
                    && out.original_text.is_none()
                    && let Some(rx) = ocr_rx
                    && let Some(Ok(text)) = rx.borrow().as_ref()
                {
                    let trimmed = text.trim();
                    if !trimmed.is_empty() {
                        out.original_text = Some(trimmed.to_string());
                    }
                }
                res
            }
            Err(err) => Err(err),
        };

        let subscriber = if let Ok(mut guard) = state.lock() {
            let prev = guard.actions.insert(action, ActionSlot::Ready(result.clone()));
            match prev {
                Some(ActionSlot::Running { subscriber, .. }) => subscriber,
                _ => None,
            }
        } else {
            None
        };

        if let Some(sub) = subscriber {
            let _ = sub.ui_tx.send(UiEvent::ProcessingFinished {
                request_id: sub.request_id,
                action: sub.action,
                result,
            });
            wake_ui(&sub.egui_slot);
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observe_overlay_selection_debounces_and_cancels_on_change() {
        let mut mgr = PrefetchManager::new();
        let t0 = Instant::now();
        let r1 = ScreenRect {
            x: 10,
            y: 20,
            width: 300,
            height: 120,
        };

        let wait1 = mgr.observe_overlay_selection(Some((r1, 0)), "en", t0);
        assert_eq!(wait1, Some(SETTLE_DEBOUNCE));

        let wait2 = mgr.observe_overlay_selection(
            Some((r1, 0)),
            "en",
            t0 + Duration::from_millis(50),
        );
        assert_eq!(wait2, Some(Duration::from_millis(70)));

        let wait3 = mgr.observe_overlay_selection(
            Some((r1, 0)),
            "en",
            t0 + SETTLE_DEBOUNCE,
        );
        assert_eq!(wait3, Some(Duration::ZERO));

        let wait4 = mgr.observe_overlay_selection(
            Some((r1, 1)),
            "en",
            t0 + SETTLE_DEBOUNCE + Duration::from_millis(10),
        );
        assert_eq!(wait4, Some(SETTLE_DEBOUNCE));

        assert_eq!(
            mgr.observe_overlay_selection(None, "en", t0 + Duration::from_secs(1)),
            None
        );
    }

    #[test]
    fn speculative_ocr_prefetch_populates_instant_ready_result() {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("tokio rt");

        let png = crate::ocr::tests::render_text_png("Instant Prefetch", 460, 120);
        let decoded = image::load_from_memory(&png).expect("decode").to_rgba8();
        let rect = ScreenRect {
            x: 0,
            y: 0,
            width: decoded.width() as i32,
            height: decoded.height() as i32,
        };
        let crop = CroppedImage {
            width: decoded.width(),
            height: decoded.height(),
            rgba: decoded.into_raw(),
            screen_rect: rect,
        };

        let cfg = AppConfig {
            speculative_prefetch: false, // test offline local OCR prefetch only
            ..Default::default()
        };
        let http = reqwest::Client::new();

        let mut mgr = PrefetchManager::new();
        mgr.start_speculative(crop, 0, &cfg, &http, rt.handle());

        let (tx, rx) = std::sync::mpsc::channel();
        let slot = Arc::new(std::sync::OnceLock::new());

        let text = match mgr.attach_or_take_ocr(rect, 0, 1, tx, slot) {
            OcrAttach::Ready(res) => res.expect("ocr ok"),
            OcrAttach::AttachedRunning => match rx
                .recv_timeout(Duration::from_secs(10))
                .expect("waiter event")
            {
                UiEvent::OcrCopyFinished { result, .. } => result.expect("ocr ok"),
                other => panic!("unexpected event: {other:?}"),
            },
            OcrAttach::Miss => panic!("expected hit on active speculative session"),
        };

        let lowered = text.to_ascii_lowercase();
        assert!(
            lowered.contains("instant") && lowered.contains("prefetch"),
            "expected OCR text, got: {text:?}"
        );

        let (tx2, _rx2) = std::sync::mpsc::channel();
        let slot2 = Arc::new(std::sync::OnceLock::new());
        assert!(matches!(
            mgr.attach_or_take_ocr(rect, 0, 2, tx2, slot2),
            OcrAttach::Ready(Ok(_))
        ));
    }
}
