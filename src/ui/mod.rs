pub mod popup;
mod popup_view;
pub mod theme;
pub mod tray;
mod window;

use crate::capture::CapturedRegion;
use crate::config::AppConfig;
use crate::providers::{SnipAction, SnipError, SnipOutput};
use crate::snip::SnipMode;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

pub type EguiContextSlot = Arc<OnceLock<egui::Context>>;

pub fn wake_ui(slot: &EguiContextSlot) {
    if let Some(ctx) = slot.get() {
        ctx.request_repaint();
    }
}

#[derive(Debug)]
pub enum UiEvent {
    TriggerSnip(SnipMode),
    OcrCopyFinished {
        request_id: u64,
        region: CapturedRegion,
        result: Result<String, String>,
    },
    ProcessingFinished {
        request_id: u64,
        action: SnipAction,
        result: Result<SnipOutput, SnipError>,
    },
    ConfigUpdated {
        config: AppConfig,
        toast: Option<String>,
    },
    ShowToast {
        title: String,
        message: String,
        is_warning: bool,
    },
    OpenConfigRequested(PathBuf),
    OpenScreenshotsFolder,
    CloseAllPins,
    Quit,
}
