use crate::capture::{encode_rgba_to_png, CapturedRegion, CroppedImage};
use crate::clipboard;
use crate::config::AppConfig;
use crate::ocr;
use crate::providers::SnipAction;
use crate::snip::save;
use crate::snip::SnipCommand;
use crate::ui::{wake_ui, EguiContextSlot, UiEvent};
use std::sync::mpsc::Sender;
use std::sync::Arc;

pub enum ActionOutcome {
    None,
    Toast {
        title: String,
        message: String,
        is_warning: bool,
    },
    PinImage(CroppedImage),
    ProcessRegion {
        region: CapturedRegion,
        action: SnipAction,
    },
}

pub fn execute_snip_command(
    cropped: CroppedImage,
    command: SnipCommand,
    config: &AppConfig,
    tokio_handle: &tokio::runtime::Handle,
    ocr_request_id: u64,
    ui_tx: &Sender<UiEvent>,
    egui_slot: &EguiContextSlot,
) -> ActionOutcome {
    match command {
        SnipCommand::Cancel => ActionOutcome::None,
        SnipCommand::PinToScreen => ActionOutcome::PinImage(cropped),
        SnipCommand::CopyImage => match clipboard::copy_rgba_image(
            cropped.width as usize,
            cropped.height as usize,
            &cropped.rgba,
        ) {
            Ok(()) => ActionOutcome::Toast {
                title: "Copied to Clipboard".to_string(),
                message: format!(
                    "{} × {} px image ready to paste",
                    cropped.width, cropped.height
                ),
                is_warning: false,
            },
            Err(err) => ActionOutcome::Toast {
                title: "Clipboard Busy".to_string(),
                message: format!("{err}. Try again or use Save (Ctrl+S)."),
                is_warning: true,
            },
        },
        SnipCommand::SavePng => match encode_rgba_to_png(cropped.width, cropped.height, &cropped.rgba)
            .map_err(|e| e.to_string())
            .and_then(|png| {
                save::save_png_bytes(config.save_dir.as_deref(), &png).map_err(|e| e.to_string())
            }) {
            Ok(path) => {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.display().to_string());
                let folder = path
                    .parent()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                ActionOutcome::Toast {
                    title: "Screenshot Saved".to_string(),
                    message: format!("{name}\n{folder}"),
                    is_warning: false,
                }
            }
            Err(err) => ActionOutcome::Toast {
                title: "Save Failed".to_string(),
                message: err,
                is_warning: true,
            },
        },
        SnipCommand::OcrCopy => match cropped.to_captured_region() {
            Ok(region) => {
                let tx = ui_tx.clone();
                let slot = Arc::clone(egui_slot);
                tokio_handle.spawn(async move {
                    let png_for_ocr = region.png_bytes.clone();
                    let res = tokio::task::spawn_blocking(move || {
                        ocr::extract_text_from_png(&png_for_ocr)
                    })
                    .await
                    .map_err(|e| format!("OCR task failed: {e}"))
                    .and_then(|inner| inner.map_err(|e| e.to_string()));
                    let _ = tx.send(UiEvent::OcrCopyFinished {
                        request_id: ocr_request_id,
                        region,
                        result: res,
                    });
                    wake_ui(&slot);
                });
                ActionOutcome::None
            }
            Err(err) => ActionOutcome::Toast {
                title: "OCR Error".to_string(),
                message: err.to_string(),
                is_warning: true,
            },
        },
        SnipCommand::Translate | SnipCommand::ExplainCode => match cropped.to_captured_region() {
            Ok(region) => ActionOutcome::ProcessRegion {
                region,
                action: if command == SnipCommand::ExplainCode {
                    SnipAction::ExplainCode
                } else {
                    SnipAction::Translate
                },
            },
            Err(err) => ActionOutcome::Toast {
                title: "Capture Error".to_string(),
                message: err.to_string(),
                is_warning: true,
            },
        },
    }
}
