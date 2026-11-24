#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod capture;
mod clipboard;
mod config;
mod hotkey;
mod logging;
mod ocr;
mod pin;
mod providers;
mod snip;
mod ui;

use std::sync::mpsc;
use std::sync::{Arc, OnceLock};
use ui::popup::SnipPopupApp;
use ui::tray::spawn_tray_and_hotkey_thread;
use ui::{EguiContextSlot, UiEvent};
use windows::core::w;
use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};

struct SingleInstanceGuard(HANDLE);
impl Drop for SingleInstanceGuard {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            // SAFETY: Releases the named mutex handle acquired at startup.
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
}

fn acquire_single_instance_guard() -> Option<SingleInstanceGuard> {
    // SAFETY: Standard Win32 named mutex check for single-instance enforcement.
    unsafe {
        let handle = CreateMutexW(None, false, w!("Local\\SnipTranslateSingleInstanceMutex"))
            .ok()?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            let _ = CloseHandle(handle);
            return None;
        }
        Some(SingleInstanceGuard(handle))
    }
}

fn enable_dpi_awareness() {
    // SAFETY: Must be called once at process startup before creating any windows.
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
}

fn main() {
    enable_dpi_awareness();

    let cfg_dir = config::config_dir().ok();
    logging::init_logging(cfg_dir);

    let Some(_instance_guard) = acquire_single_instance_guard() else {
        log::info!("Reticle is already running; exiting duplicate launch");
        return;
    };

    log::info!("Starting Reticle v{}", env!("CARGO_PKG_VERSION"));

    let (app_config, cfg_path, startup_warning) = match config::load_or_create() {
        Ok((cfg, path)) => (cfg, path, None),
        Err(err) => {
            log::error!("Failed to load config.json; using defaults: {err}");
            let fallback_path = config::config_path()
                .unwrap_or_else(|_| std::env::temp_dir().join("snip-translate-config.json"));
            (
                config::AppConfig::default(),
                fallback_path,
                Some(format!(
                    "Could not read config.json ({err}). Running with default settings."
                )),
            )
        }
    };
    log::info!("Loaded config: {app_config:?} (path={})", cfg_path.display());

    let tokio_rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("snip-async")
        .build()
        .expect("build tokio runtime");
    let tokio_handle = tokio_rt.handle().clone();

    let (ui_tx, ui_rx) = mpsc::channel::<UiEvent>();
    let egui_slot: EguiContextSlot = Arc::new(OnceLock::new());

    if let Some(msg) = startup_warning {
        let _ = ui_tx.send(UiEvent::ShowToast {
            title: "Config Warning".to_string(),
            message: msg,
            is_warning: true,
        });
    } else {
        let _ = ui_tx.send(UiEvent::ShowToast {
            title: "Reticle Ready".to_string(),
            message: format!(
                "Press {} to snip (Copy · Pin · Annotate · Save · OCR · Translate)",
                app_config.hotkey
            ),
            is_warning: false,
        });
    }

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Reticle")
            .with_inner_size([432.0, 160.0])
            .with_decorations(false)
            .with_transparent(false)
            .with_always_on_top()
            .with_taskbar(false)
            .with_visible(false)
            .with_resizable(false),
        ..Default::default()
    };

    let tray_slot: Arc<OnceLock<ui::tray::TrayController>> = Arc::new(OnceLock::new());
    let tray_slot_for_creator = Arc::clone(&tray_slot);

    let run_result = eframe::run_native(
        "Reticle",
        native_options,
        Box::new(move |cc| {
            let _ = egui_slot.set(cc.egui_ctx.clone());
            let tray_controller = spawn_tray_and_hotkey_thread(
                app_config.clone(),
                cfg_path.clone(),
                ui_tx.clone(),
                Arc::clone(&egui_slot),
            );
            let _ = tray_slot_for_creator.set(tray_controller.clone());
            cc.egui_ctx.request_repaint();

            Ok(Box::new(SnipPopupApp::new(
                cc,
                app_config,
                cfg_path,
                ui_tx,
                ui_rx,
                egui_slot,
                tokio_handle,
                tray_controller,
            )))
        }),
    );

    if let Some(ctrl) = tray_slot.get() {
        ctrl.request_shutdown();
    }
    if let Err(err) = run_result {
        log::error!("eframe exited with error: {err}");
    }
    log::info!("Reticle exited cleanly");
}
