use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

pub const DEFAULT_HOTKEY: &str = "Ctrl+Shift+O";
pub const DEFAULT_TARGET_LANGUAGE: &str = "en";
pub const DEFAULT_GEMINI_MODEL: &str = "gemini-2.5-flash";
pub const DEFAULT_OPENAI_MODEL: &str = "gpt-4o-mini";

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to locate %APPDATA% directory")]
    MissingAppDataDir,
    #[error("failed to read config file {path}: {source}")]
    ReadFailed {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to write config file {path}: {source}")]
    WriteFailed {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse config JSON in {path}: {source}")]
    ParseFailed {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ProviderMode {
    #[default]
    Free,
    Byok,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AiProviderKind {
    #[default]
    Gemini,
    OpenAi,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AiProviderConfig {
    pub provider: AiProviderKind,
    pub api_key: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
}

impl fmt::Debug for AiProviderConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let masked_key = if self.api_key.trim().is_empty() {
            ""
        } else {
            "[REDACTED]"
        };
        f.debug_struct("AiProviderConfig")
            .field("provider", &self.provider)
            .field("api_key", &masked_key)
            .field("model", &self.model)
            .field("base_url", &self.base_url)
            .finish()
    }
}

impl AiProviderConfig {
    pub fn has_api_key(&self) -> bool {
        !self.api_key.trim().is_empty()
    }

    pub fn effective_model(&self) -> &str {
        let trimmed = self.model.trim();
        if !trimmed.is_empty() {
            return trimmed;
        }
        match self.provider {
            AiProviderKind::Gemini => DEFAULT_GEMINI_MODEL,
            AiProviderKind::OpenAi => DEFAULT_OPENAI_MODEL,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub hotkey: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quick_translate_hotkey: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub save_dir: Option<String>,
    pub target_language: String,
    pub mode: ProviderMode,
    pub ai_provider: AiProviderConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            hotkey: DEFAULT_HOTKEY.to_string(),
            quick_translate_hotkey: None,
            save_dir: None,
            target_language: DEFAULT_TARGET_LANGUAGE.to_string(),
            mode: ProviderMode::Free,
            ai_provider: AiProviderConfig::default(),
        }
    }
}

impl AppConfig {
    pub fn has_byok_key(&self) -> bool {
        self.ai_provider.has_api_key()
    }

    pub fn normalized_target_language(&self) -> String {
        let trimmed = self.target_language.trim();
        if trimmed.is_empty() {
            DEFAULT_TARGET_LANGUAGE.to_string()
        } else {
            trimmed.to_string()
        }
    }

    pub fn from_json_str(raw: &str) -> Result<Self, serde_json::Error> {
        let mut cfg: AppConfig = serde_json::from_str(raw)?;
        if cfg.hotkey.trim().is_empty() {
            cfg.hotkey = DEFAULT_HOTKEY.to_string();
        }
        if cfg
            .quick_translate_hotkey
            .as_deref()
            .is_some_and(|s| s.trim().is_empty())
        {
            cfg.quick_translate_hotkey = None;
        }
        if cfg.save_dir.as_deref().is_some_and(|s| s.trim().is_empty()) {
            cfg.save_dir = None;
        }
        if cfg.target_language.trim().is_empty() {
            cfg.target_language = DEFAULT_TARGET_LANGUAGE.to_string();
        }
        Ok(cfg)
    }

    pub fn to_pretty_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    pub fn save_to(&self, path: &Path) -> Result<(), ConfigError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| ConfigError::WriteFailed {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let json = self.to_pretty_json().map_err(|source| ConfigError::ParseFailed {
            path: path.to_path_buf(),
            source,
        })?;
        fs::write(path, json).map_err(|source| ConfigError::WriteFailed {
            path: path.to_path_buf(),
            source,
        })
    }
}

pub fn config_dir() -> Result<PathBuf, ConfigError> {
    let appdata = std::env::var_os("APPDATA").ok_or(ConfigError::MissingAppDataDir)?;
    let primary = PathBuf::from(&appdata).join("reticle");
    let legacy = PathBuf::from(appdata).join("snip-translate");
    if !primary.join("config.json").exists() && legacy.join("config.json").exists() {
        return Ok(legacy);
    }
    Ok(primary)
}

pub fn config_path() -> Result<PathBuf, ConfigError> {
    Ok(config_dir()?.join("config.json"))
}

pub fn load_or_create() -> Result<(AppConfig, PathBuf), ConfigError> {
    let path = config_path()?;
    let cfg = load_or_create_at(&path)?;
    Ok((cfg, path))
}

pub fn load_or_create_at(path: &Path) -> Result<AppConfig, ConfigError> {
    if !path.exists() {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| ConfigError::WriteFailed {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let default_cfg = AppConfig::default();
        let json = default_cfg
            .to_pretty_json()
            .expect("default AppConfig serialization cannot fail");
        fs::write(path, json).map_err(|source| ConfigError::WriteFailed {
            path: path.to_path_buf(),
            source,
        })?;
        return Ok(default_cfg);
    }

    let raw = fs::read_to_string(path).map_err(|source| ConfigError::ReadFailed {
        path: path.to_path_buf(),
        source,
    })?;
    AppConfig::from_json_str(&raw).map_err(|source| ConfigError::ParseFailed {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_matches_expected_schema() {
        let cfg = AppConfig::default();
        assert_eq!(cfg.hotkey, "Ctrl+Shift+O");
        assert_eq!(cfg.target_language, "en");
        assert_eq!(cfg.mode, ProviderMode::Free);
        assert_eq!(cfg.ai_provider.provider, AiProviderKind::Gemini);
        assert!(cfg.ai_provider.api_key.is_empty());
        assert!(cfg.ai_provider.model.is_empty());
        assert!(!cfg.has_byok_key());

        let json = cfg.to_pretty_json().expect("serialize default");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid json");
        assert_eq!(parsed["hotkey"], "Ctrl+Shift+O");
        assert_eq!(parsed["target_language"], "en");
        assert_eq!(parsed["mode"], "free");
        assert_eq!(parsed["ai_provider"]["provider"], "gemini");
        assert_eq!(parsed["ai_provider"]["api_key"], "");
        assert_eq!(parsed["ai_provider"]["model"], "");
        assert!(parsed["ai_provider"].get("base_url").is_none());
    }

    #[test]
    fn parses_partial_json_with_defaults() {
        let raw = r#"{ "target_language": "vi", "hotkey": "Ctrl+O" }"#;
        let cfg = AppConfig::from_json_str(raw).expect("parse partial");
        assert_eq!(cfg.hotkey, "Ctrl+O");
        assert_eq!(cfg.target_language, "vi");
        assert_eq!(cfg.mode, ProviderMode::Free);
        assert_eq!(cfg.ai_provider.provider, AiProviderKind::Gemini);
    }

    #[test]
    fn parses_byok_openai_config() {
        let raw = r#"{
            "hotkey": "Alt+Shift+T",
            "target_language": "ja",
            "mode": "byok",
            "ai_provider": {
                "provider": "openai",
                "api_key": "sk-test-secret-123",
                "model": "gpt-4o"
            }
        }"#;
        let cfg = AppConfig::from_json_str(raw).expect("parse byok");
        assert_eq!(cfg.mode, ProviderMode::Byok);
        assert_eq!(cfg.ai_provider.provider, AiProviderKind::OpenAi);
        assert!(cfg.has_byok_key());
        assert_eq!(cfg.ai_provider.effective_model(), "gpt-4o");
    }

    #[test]
    fn debug_output_redacts_api_key() {
        let cfg = AppConfig {
            ai_provider: AiProviderConfig {
                provider: AiProviderKind::Gemini,
                api_key: "super-secret-token-value".to_string(),
                model: String::new(),
                base_url: None,
            },
            ..AppConfig::default()
        };
        let rendered = format!("{cfg:?}");
        assert!(!rendered.contains("super-secret-token-value"));
        assert!(rendered.contains("[REDACTED]"));
    }

    #[test]
    fn default_models_resolve_per_provider() {
        let gemini = AiProviderConfig {
            provider: AiProviderKind::Gemini,
            ..Default::default()
        };
        assert_eq!(gemini.effective_model(), DEFAULT_GEMINI_MODEL);

        let openai = AiProviderConfig {
            provider: AiProviderKind::OpenAi,
            ..Default::default()
        };
        assert_eq!(openai.effective_model(), DEFAULT_OPENAI_MODEL);
    }

    #[test]
    fn load_or_create_at_writes_and_reloads_file() {
        let temp_dir = std::env::temp_dir().join(format!(
            "snip-translate-cfg-test-{}",
            std::process::id()
        ));
        let path = temp_dir.join("config.json");
        let _ = fs::remove_dir_all(&temp_dir);

        let created = load_or_create_at(&path).expect("create config");
        assert_eq!(created, AppConfig::default());
        assert!(path.exists());

        let reloaded = load_or_create_at(&path).expect("reload config");
        assert_eq!(reloaded, AppConfig::default());

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn parses_optional_save_dir_and_quick_translate_hotkey() {
        let raw = r#"{
            "hotkey": "Ctrl+Shift+O",
            "quick_translate_hotkey": "Ctrl+Shift+T",
            "save_dir": "D:\\Screenshots"
        }"#;
        let cfg = AppConfig::from_json_str(raw).expect("parse extended config");
        assert_eq!(cfg.quick_translate_hotkey.as_deref(), Some("Ctrl+Shift+T"));
        assert_eq!(cfg.save_dir.as_deref(), Some(r"D:\Screenshots"));

        let blank = AppConfig::from_json_str(r#"{"quick_translate_hotkey":"   ","save_dir":""}"#)
            .expect("normalize blank optionals");
        assert_eq!(blank.quick_translate_hotkey, None);
        assert_eq!(blank.save_dir, None);
    }
}
