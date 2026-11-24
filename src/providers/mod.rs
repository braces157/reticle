pub mod byok_ai;
pub mod free_translate;
pub mod stream;

use crate::config::{AiProviderKind, AppConfig, ProviderMode};
use std::future::Future;
use std::pin::Pin;
use thiserror::Error;

use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SnipAction {
    Translate,
    ExplainCode,
    Ocr,
}

#[derive(Debug, Clone)]
pub struct SnipInput {
    pub image_bytes: Vec<u8>,
    pub image_mime: &'static str,
    pub precomputed_ocr: Option<String>,
    pub action: SnipAction,
    pub target_language: String,
}

impl SnipInput {
    #[cfg(test)]
    pub fn from_png(png_bytes: Vec<u8>, action: SnipAction, target_language: String) -> Self {
        Self {
            image_bytes: png_bytes,
            image_mime: "image/png",
            precomputed_ocr: None,
            action,
            target_language,
        }
    }

    pub fn from_jpeg(
        jpeg_bytes: Vec<u8>,
        precomputed_ocr: Option<String>,
        action: SnipAction,
        target_language: String,
    ) -> Self {
        Self {
            image_bytes: jpeg_bytes,
            image_mime: "image/jpeg",
            precomputed_ocr,
            action,
            target_language,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnipOutput {
    pub primary_text: String,
    pub original_text: Option<String>,
    pub detected_language: Option<String>,
    pub provider_label: String,
    pub warning: Option<String>,
    pub is_markdown: bool,
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum SnipError {
    #[error("No text detected")]
    NoTextDetected,
    #[error("OCR failed: {0}")]
    OcrFailed(String),
    #[error("{0}")]
    UnsupportedAction(String),
    #[error("Provider error: {0}")]
    ProviderFailed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    Free,
    Byok(AiProviderKind),
}

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub type DeltaCallback = Arc<dyn Fn(&str) + Send + Sync>;

pub trait SnipProvider: Send + Sync {
    fn name(&self) -> &'static str;
    fn process<'a>(&'a self, input: SnipInput) -> BoxFuture<'a, Result<SnipOutput, SnipError>>;
    fn process_streaming<'a>(
        &'a self,
        input: SnipInput,
        on_delta: DeltaCallback,
    ) -> BoxFuture<'a, Result<SnipOutput, SnipError>> {
        let _ = on_delta;
        self.process(input)
    }
}

pub fn resolve_provider_kind(config: &AppConfig, action: SnipAction) -> ProviderKind {
    if !config.has_byok_key() {
        return ProviderKind::Free;
    }
    match (config.mode, action) {
        (ProviderMode::Byok, _)
        | (ProviderMode::Free, SnipAction::ExplainCode | SnipAction::Ocr) => {
            ProviderKind::Byok(config.ai_provider.provider)
        }
        (ProviderMode::Free, SnipAction::Translate) => ProviderKind::Free,
    }
}

pub fn select_provider(
    config: &AppConfig,
    action: SnipAction,
    http: reqwest::Client,
) -> Box<dyn SnipProvider> {
    match resolve_provider_kind(config, action) {
        ProviderKind::Free => Box::new(free_translate::FreeTranslateProvider::new(http)),
        ProviderKind::Byok(_) => Box::new(byok_ai::ByokAiProvider::new(
            http,
            config.ai_provider.clone(),
        )),
    }
}

pub fn build_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(45))
        .tcp_nodelay(true)
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36")
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AiProviderConfig;

    #[test]
    fn defaults_to_free_provider_without_api_key() {
        let cfg = AppConfig::default();
        assert_eq!(
            resolve_provider_kind(&cfg, SnipAction::Translate),
            ProviderKind::Free
        );
        assert_eq!(
            resolve_provider_kind(&cfg, SnipAction::ExplainCode),
            ProviderKind::Free
        );
    }

    #[test]
    fn byok_mode_without_key_falls_back_to_free() {
        let cfg = AppConfig {
            mode: ProviderMode::Byok,
            ..AppConfig::default()
        };
        assert_eq!(
            resolve_provider_kind(&cfg, SnipAction::Translate),
            ProviderKind::Free
        );
    }

    #[test]
    fn free_mode_with_key_uses_free_for_translate_and_byok_for_explain_code() {
        let cfg = AppConfig {
            mode: ProviderMode::Free,
            ai_provider: AiProviderConfig {
                provider: AiProviderKind::Gemini,
                api_key: "test-key".to_string(),
                model: String::new(),
                base_url: None,
            },
            ..AppConfig::default()
        };
        assert_eq!(
            resolve_provider_kind(&cfg, SnipAction::Translate),
            ProviderKind::Free
        );
        assert_eq!(
            resolve_provider_kind(&cfg, SnipAction::ExplainCode),
            ProviderKind::Byok(AiProviderKind::Gemini)
        );
    }

    #[test]
    fn byok_mode_with_key_uses_configured_ai_provider_for_both_actions() {
        let cfg = AppConfig {
            mode: ProviderMode::Byok,
            ai_provider: AiProviderConfig {
                provider: AiProviderKind::OpenAi,
                api_key: "sk-test".to_string(),
                model: "gpt-4o".to_string(),
                base_url: None,
            },
            ..AppConfig::default()
        };
        assert_eq!(
            resolve_provider_kind(&cfg, SnipAction::Translate),
            ProviderKind::Byok(AiProviderKind::OpenAi)
        );
        assert_eq!(
            resolve_provider_kind(&cfg, SnipAction::ExplainCode),
            ProviderKind::Byok(AiProviderKind::OpenAi)
        );
    }
}
