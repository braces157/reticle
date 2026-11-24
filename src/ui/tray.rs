use crate::config::{self, AppConfig};
use crate::hotkey::HotkeyRegistration;
use crate::snip::SnipMode;
use crate::ui::{wake_ui, EguiContextSlot, UiEvent};
use global_hotkey::{GlobalHotKeyEvent, HotKeyState};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, PostQuitMessage, PostThreadMessageW, TranslateMessage, MSG,
    WM_APP,
};

const WM_RELOAD_CONFIG: u32 = WM_APP + 1;
const WM_SHUTDOWN_TRAY: u32 = WM_APP + 2;

#[derive(Clone)]
pub struct TrayController {
    thread_id: Arc<AtomicU32>,
}

impl TrayController {
    pub fn request_shutdown(&self) {
        let tid = self.thread_id.load(Ordering::SeqCst);
        if tid != 0 {
            // SAFETY: Posting an application-defined message to our own tray worker thread.
            unsafe {
                let _ = PostThreadMessageW(tid, WM_SHUTDOWN_TRAY, Default::default(), Default::default());
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn noop() -> Self {
        Self {
            thread_id: Arc::new(AtomicU32::new(0)),
        }
    }
}

/// Generates a crisp 32x32 RGBA Reticle tray icon (matte obsidian squircle + white brackets + optical blue focal dot).
pub fn build_tray_icon() -> Icon {
    let size = 32usize;
    let mut rgba = vec![0u8; size * size * 4];

    let set_px = |buf: &mut [u8], x: usize, y: usize, color: [u8; 4]| {
        if x < size && y < size {
            let i = (y * size + x) * 4;
            buf[i..i + 4].copy_from_slice(&color);
        }
    };

    let bg = [12, 12, 14, 255];
    let border = [56, 56, 62, 255];
    let white = [255, 255, 255, 255];
    let focal_blue = [58, 169, 255, 255];

    // Rounded squircle background
    for y in 1..31 {
        for x in 1..31 {
            let corner = !(6..26).contains(&x) && !(6..26).contains(&y);
            if corner {
                let dx = if x < 6 { 6 - x } else { x - 25 };
                let dy = if y < 6 { 6 - y } else { y - 25 };
                if dx * dx + dy * dy > 25 {
                    continue;
                }
            }
            let is_edge = x == 1 || x == 30 || y == 1 || y == 30;
            set_px(&mut rgba, x, y, if is_edge { border } else { bg });
        }
    }

    // Crisp white viewfinder corner brackets
    for offset in 0..6 {
        // Top-left
        set_px(&mut rgba, 7 + offset, 7, white);
        set_px(&mut rgba, 7 + offset, 8, white);
        set_px(&mut rgba, 7, 7 + offset, white);
        set_px(&mut rgba, 8, 7 + offset, white);
        // Top-right
        set_px(&mut rgba, 24 - offset, 7, white);
        set_px(&mut rgba, 24 - offset, 8, white);
        set_px(&mut rgba, 24, 7 + offset, white);
        set_px(&mut rgba, 23, 7 + offset, white);
        // Bottom-left
        set_px(&mut rgba, 7 + offset, 24, white);
        set_px(&mut rgba, 7 + offset, 23, white);
        set_px(&mut rgba, 7, 24 - offset, white);
        set_px(&mut rgba, 8, 24 - offset, white);
        // Bottom-right
        set_px(&mut rgba, 24 - offset, 24, white);
        set_px(&mut rgba, 24 - offset, 23, white);
        set_px(&mut rgba, 24, 24 - offset, white);
        set_px(&mut rgba, 23, 24 - offset, white);
    }

    // Optical blue center focal dot (4x4 rounded)
    for y in 14..18 {
        for x in 14..18 {
            set_px(&mut rgba, x, y, focal_blue);
        }
    }

    Icon::from_rgba(rgba, size as u32, size as u32).expect("32x32 RGBA tray icon is valid")
}

pub fn spawn_tray_and_hotkey_thread(
    initial_config: AppConfig,
    config_path: PathBuf,
    ui_tx: Sender<UiEvent>,
    egui_slot: EguiContextSlot,
) -> TrayController {
    let thread_id = Arc::new(AtomicU32::new(0));
    let tid_writer = Arc::clone(&thread_id);

    std::thread::Builder::new()
        .name("tray-hotkey-loop".to_string())
        .spawn(move || {
            // SAFETY: GetCurrentThreadId has no preconditions.
            let current_tid = unsafe { GetCurrentThreadId() };
            tid_writer.store(current_tid, Ordering::SeqCst);

            let trigger_item =
                MenuItem::new(format!("New Snip ({})", initial_config.hotkey), true, None);
            let close_pins_item = MenuItem::new("Close All Pinned Screenshots", true, None);
            let open_snips_item = MenuItem::new("Open Screenshots Folder", true, None);
            let open_cfg_item = MenuItem::new("Open config.json", true, None);
            let reload_cfg_item = MenuItem::new("Reload config", true, None);
            let quit_item = MenuItem::new("Quit", true, None);

            let trigger_id = trigger_item.id().clone();
            let close_pins_id = close_pins_item.id().clone();
            let open_snips_id = open_snips_item.id().clone();
            let open_cfg_id = open_cfg_item.id().clone();
            let reload_cfg_id = reload_cfg_item.id().clone();
            let quit_id = quit_item.id().clone();

            let menu = Menu::new();
            let _ = menu.append(&trigger_item);
            let _ = menu.append(&close_pins_item);
            let _ = menu.append(&open_snips_item);
            let _ = menu.append(&PredefinedMenuItem::separator());
            let _ = menu.append(&open_cfg_item);
            let _ = menu.append(&reload_cfg_item);
            let _ = menu.append(&PredefinedMenuItem::separator());
            let _ = menu.append(&quit_item);

            let icon = build_tray_icon();
            let tray = match TrayIconBuilder::new()
                .with_menu(Box::new(menu))
                .with_menu_on_left_click(false)
                .with_tooltip(format!("Reticle ({})", initial_config.hotkey))
                .with_icon(icon)
                .build()
            {
                Ok(t) => t,
                Err(e) => {
                    log::error!("Failed to build system tray icon: {e}");
                    let _ = ui_tx.send(UiEvent::ShowToast {
                        title: "Tray Icon Error".to_string(),
                        message: format!("Could not create Windows tray icon: {e}"),
                        is_warning: true,
                    });
                    wake_ui(&egui_slot);
                    return;
                }
            };

            let mut hotkey_reg = match HotkeyRegistration::new() {
                Ok(reg) => Some(reg),
                Err(e) => {
                    log::warn!("GlobalHotkeyManager init failed: {e}");
                    let _ = ui_tx.send(UiEvent::ShowToast {
                        title: "Hotkey Unavailable".to_string(),
                        message: format!(
                            "{e}. Use the system tray menu to trigger snips."
                        ),
                        is_warning: true,
                    });
                    wake_ui(&egui_slot);
                    None
                }
            };

            let active_hotkey_id = Arc::new(AtomicU32::new(0));
            let quick_translate_hotkey_id = Arc::new(AtomicU32::new(0));
            let register_and_report = |reg: &mut Option<HotkeyRegistration>,
                                       cfg: &AppConfig,
                                       tray_icon: &tray_icon::TrayIcon,
                                       trigger_menu_item: &MenuItem,
                                       id_slot: &Arc<AtomicU32>,
                                       quick_id_slot: &Arc<AtomicU32>| {
                let mut warnings = Vec::new();
                if let Some(manager) = reg.as_mut() {
                    let spec = &cfg.hotkey;
                    match manager.register_from_str(spec) {
                        Ok(hk) => {
                            id_slot.store(hk.id(), Ordering::SeqCst);
                            trigger_menu_item.set_text(format!("New Snip ({spec})"));
                            let _ = tray_icon.set_tooltip(Some(format!("Reticle ({spec})")));
                            log::info!("Registered global hotkey: {spec} (id={})", hk.id());
                        }
                        Err(err) => {
                            id_slot.store(0, Ordering::SeqCst);
                            trigger_menu_item.set_text("New Snip (hotkey conflict)");
                            let _ = tray_icon.set_tooltip(Some(format!(
                                "Reticle — Hotkey conflict: {spec}"
                            )));
                            log::warn!("Hotkey conflict for '{spec}': {err}");
                            warnings.push(format!(
                                "{err}.\nOpen config.json from the tray menu to choose another hotkey (e.g. \"Ctrl+Alt+O\")."
                            ));
                        }
                    }

                    match manager.register_secondary_from_str(cfg.quick_translate_hotkey.as_deref()) {
                        Ok(Some(hk)) => {
                            quick_id_slot.store(hk.id(), Ordering::SeqCst);
                            log::info!(
                                "Registered quick-translate hotkey: {:?} (id={})",
                                cfg.quick_translate_hotkey,
                                hk.id()
                            );
                        }
                        Ok(None) => {
                            quick_id_slot.store(0, Ordering::SeqCst);
                        }
                        Err(err) => {
                            quick_id_slot.store(0, Ordering::SeqCst);
                            log::warn!("Quick-translate hotkey error: {err}");
                            warnings.push(format!("Quick-translate hotkey failed: {err}"));
                        }
                    }
                }
                if warnings.is_empty() {
                    None
                } else {
                    Some(warnings.join("\n"))
                }
            };

            if let Some(warning) = register_and_report(
                &mut hotkey_reg,
                &initial_config,
                &tray,
                &trigger_item,
                &active_hotkey_id,
                &quick_translate_hotkey_id,
            ) {
                let _ = ui_tx.send(UiEvent::ShowToast {
                    title: "Hotkey Conflict".to_string(),
                    message: warning,
                    is_warning: true,
                });
                wake_ui(&egui_slot);
            }

            // Menu event callback (runs synchronously inside Win32 message dispatch)
            let menu_ui_tx = ui_tx.clone();
            let menu_egui_slot = Arc::clone(&egui_slot);
            let menu_cfg_path = config_path.clone();
            MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
                if event.id == trigger_id {
                    let _ = menu_ui_tx.send(UiEvent::TriggerSnip(SnipMode::Interactive));
                    wake_ui(&menu_egui_slot);
                } else if event.id == close_pins_id {
                    let _ = menu_ui_tx.send(UiEvent::CloseAllPins);
                    wake_ui(&menu_egui_slot);
                } else if event.id == open_snips_id {
                    let _ = menu_ui_tx.send(UiEvent::OpenScreenshotsFolder);
                    wake_ui(&menu_egui_slot);
                } else if event.id == open_cfg_id {
                    let _ = menu_ui_tx.send(UiEvent::OpenConfigRequested(menu_cfg_path.clone()));
                    wake_ui(&menu_egui_slot);
                } else if event.id == reload_cfg_id {
                    // SAFETY: Posts reload message to this tray thread's own message queue.
                    unsafe {
                        let _ = PostThreadMessageW(
                            current_tid,
                            WM_RELOAD_CONFIG,
                            Default::default(),
                            Default::default(),
                        );
                    }
                } else if event.id == quit_id {
                    let _ = menu_ui_tx.send(UiEvent::Quit);
                    wake_ui(&menu_egui_slot);
                    // SAFETY: Ends the tray Win32 message loop.
                    unsafe {
                        PostQuitMessage(0);
                    }
                }
            }));

            // Left-click on tray icon directly triggers interactive snip (right-click opens menu).
            let click_ui_tx = ui_tx.clone();
            let click_egui_slot = Arc::clone(&egui_slot);
            TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
                if let TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } = event
                {
                    let _ = click_ui_tx.send(UiEvent::TriggerSnip(SnipMode::Interactive));
                    wake_ui(&click_egui_slot);
                }
            }));

            // Global hotkey callback
            let hk_ui_tx = ui_tx.clone();
            let hk_egui_slot = Arc::clone(&egui_slot);
            let hk_id_slot = Arc::clone(&active_hotkey_id);
            let hk_quick_id_slot = Arc::clone(&quick_translate_hotkey_id);
            GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
                if event.state == HotKeyState::Pressed {
                    let primary = hk_id_slot.load(Ordering::SeqCst);
                    let quick = hk_quick_id_slot.load(Ordering::SeqCst);
                    log::info!(
                        "Global hotkey pressed (event_id={}, primary={}, quick={})",
                        event.id,
                        primary,
                        quick
                    );
                    if primary != 0 && event.id == primary {
                        let _ = hk_ui_tx.send(UiEvent::TriggerSnip(SnipMode::Interactive));
                        wake_ui(&hk_egui_slot);
                    } else if quick != 0 && event.id == quick {
                        let _ = hk_ui_tx.send(UiEvent::TriggerSnip(SnipMode::QuickTranslate));
                        wake_ui(&hk_egui_slot);
                    }
                }
            }));

            // Standard Win32 message pump for tray-icon + global-hotkey
            let mut msg = MSG::default();
            // SAFETY: Standard Win32 message loop for the tray thread.
            unsafe {
                while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                    if msg.message == WM_RELOAD_CONFIG {
                        match config::load_or_create_at(&config_path) {
                            Ok(new_cfg) => {
                                let warn = register_and_report(
                                    &mut hotkey_reg,
                                    &new_cfg,
                                    &tray,
                                    &trigger_item,
                                    &active_hotkey_id,
                                    &quick_translate_hotkey_id,
                                );
                                let _ = ui_tx.send(UiEvent::ConfigUpdated {
                                    config: new_cfg,
                                    toast: warn.or_else(|| {
                                        Some("Configuration reloaded successfully.".to_string())
                                    }),
                                });
                                wake_ui(&egui_slot);
                            }
                            Err(err) => {
                                log::warn!("Failed to reload config: {err}");
                                let _ = ui_tx.send(UiEvent::ShowToast {
                                    title: "Config Error".to_string(),
                                    message: format!("Could not parse config.json: {err}"),
                                    is_warning: true,
                                });
                                wake_ui(&egui_slot);
                            }
                        }
                        continue;
                    }
                    if msg.message == WM_SHUTDOWN_TRAY {
                        PostQuitMessage(0);
                        continue;
                    }
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }

            // Cleanly unregister handlers before thread exits.
            TrayIconEvent::set_event_handler::<fn(TrayIconEvent)>(None);
            MenuEvent::set_event_handler::<fn(MenuEvent)>(None);
            GlobalHotKeyEvent::set_event_handler::<fn(GlobalHotKeyEvent)>(None);
            drop(hotkey_reg);
            drop(tray);
        })
        .expect("spawn tray-hotkey-loop thread");

    TrayController { thread_id }
}
