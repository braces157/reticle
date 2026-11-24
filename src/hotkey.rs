use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::GlobalHotKeyManager;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum HotkeyError {
    #[error("hotkey string is empty")]
    Empty,
    #[error("invalid hotkey '{spec}': unknown token '{token}'")]
    UnknownToken { spec: String, token: String },
    #[error("invalid hotkey '{spec}': multiple primary keys specified")]
    MultipleKeys { spec: String },
    #[error("invalid hotkey '{spec}': missing primary key")]
    MissingKey { spec: String },
    #[error("failed to initialize global hotkey manager: {0}")]
    ManagerInit(String),
    #[error("hotkey '{spec}' is already in use or could not be registered ({reason})")]
    RegistrationFailed { spec: String, reason: String },
}

pub fn parse_hotkey(spec: &str) -> Result<HotKey, HotkeyError> {
    let trimmed = spec.trim();
    if trimmed.is_empty() {
        return Err(HotkeyError::Empty);
    }

    let mut modifiers = Modifiers::empty();
    let mut key_code: Option<Code> = None;

    for raw_part in trimmed.split('+') {
        let part = raw_part.trim();
        if part.is_empty() {
            return Err(HotkeyError::UnknownToken {
                spec: spec.to_string(),
                token: String::new(),
            });
        }
        let upper = part.to_ascii_uppercase();
        match upper.as_str() {
            "CTRL" | "CONTROL" => modifiers |= Modifiers::CONTROL,
            "SHIFT" => modifiers |= Modifiers::SHIFT,
            "ALT" => modifiers |= Modifiers::ALT,
            "SUPER" | "WIN" | "META" | "CMD" => modifiers |= Modifiers::META,
            other => {
                let code = parse_key_code(other).ok_or_else(|| HotkeyError::UnknownToken {
                    spec: spec.to_string(),
                    token: part.to_string(),
                })?;
                if key_code.is_some() {
                    return Err(HotkeyError::MultipleKeys {
                        spec: spec.to_string(),
                    });
                }
                key_code = Some(code);
            }
        }
    }

    let code = key_code.ok_or_else(|| HotkeyError::MissingKey {
        spec: spec.to_string(),
    })?;
    let mods = if modifiers.is_empty() {
        None
    } else {
        Some(modifiers)
    };
    Ok(HotKey::new(mods, code))
}

fn parse_key_code(token: &str) -> Option<Code> {
    match token {
        "A" => Some(Code::KeyA),
        "B" => Some(Code::KeyB),
        "C" => Some(Code::KeyC),
        "D" => Some(Code::KeyD),
        "E" => Some(Code::KeyE),
        "F" => Some(Code::KeyF),
        "G" => Some(Code::KeyG),
        "H" => Some(Code::KeyH),
        "I" => Some(Code::KeyI),
        "J" => Some(Code::KeyJ),
        "K" => Some(Code::KeyK),
        "L" => Some(Code::KeyL),
        "M" => Some(Code::KeyM),
        "N" => Some(Code::KeyN),
        "O" => Some(Code::KeyO),
        "P" => Some(Code::KeyP),
        "Q" => Some(Code::KeyQ),
        "R" => Some(Code::KeyR),
        "S" => Some(Code::KeyS),
        "T" => Some(Code::KeyT),
        "U" => Some(Code::KeyU),
        "V" => Some(Code::KeyV),
        "W" => Some(Code::KeyW),
        "X" => Some(Code::KeyX),
        "Y" => Some(Code::KeyY),
        "Z" => Some(Code::KeyZ),
        "0" | "DIGIT0" => Some(Code::Digit0),
        "1" | "DIGIT1" => Some(Code::Digit1),
        "2" | "DIGIT2" => Some(Code::Digit2),
        "3" | "DIGIT3" => Some(Code::Digit3),
        "4" | "DIGIT4" => Some(Code::Digit4),
        "5" | "DIGIT5" => Some(Code::Digit5),
        "6" | "DIGIT6" => Some(Code::Digit6),
        "7" | "DIGIT7" => Some(Code::Digit7),
        "8" | "DIGIT8" => Some(Code::Digit8),
        "9" | "DIGIT9" => Some(Code::Digit9),
        "F1" => Some(Code::F1),
        "F2" => Some(Code::F2),
        "F3" => Some(Code::F3),
        "F4" => Some(Code::F4),
        "F5" => Some(Code::F5),
        "F6" => Some(Code::F6),
        "F7" => Some(Code::F7),
        "F8" => Some(Code::F8),
        "F9" => Some(Code::F9),
        "F10" => Some(Code::F10),
        "F11" => Some(Code::F11),
        "F12" => Some(Code::F12),
        "SPACE" => Some(Code::Space),
        "TAB" => Some(Code::Tab),
        "ENTER" | "RETURN" => Some(Code::Enter),
        "BACKSPACE" => Some(Code::Backspace),
        "INSERT" => Some(Code::Insert),
        "DELETE" | "DEL" => Some(Code::Delete),
        "HOME" => Some(Code::Home),
        "END" => Some(Code::End),
        "PAGEUP" => Some(Code::PageUp),
        "PAGEDOWN" => Some(Code::PageDown),
        "PRINTSCREEN" | "PRTSC" => Some(Code::PrintScreen),
        _ => None,
    }
}

pub struct HotkeyRegistration {
    manager: GlobalHotKeyManager,
    active: Option<HotKey>,
    secondary: Option<HotKey>,
}

impl HotkeyRegistration {
    pub fn new() -> Result<Self, HotkeyError> {
        let manager =
            GlobalHotKeyManager::new().map_err(|e| HotkeyError::ManagerInit(e.to_string()))?;
        Ok(Self {
            manager,
            active: None,
            secondary: None,
        })
    }

    pub fn register_from_str(&mut self, spec: &str) -> Result<HotKey, HotkeyError> {
        let hotkey = parse_hotkey(spec)?;
        if let Some(prev) = self.active.take() {
            let _ = self.manager.unregister(prev);
        }
        self.manager
            .register(hotkey)
            .map_err(|e| HotkeyError::RegistrationFailed {
                spec: spec.to_string(),
                reason: e.to_string(),
            })?;
        self.active = Some(hotkey);
        Ok(hotkey)
    }

    pub fn register_secondary_from_str(
        &mut self,
        spec: Option<&str>,
    ) -> Result<Option<HotKey>, HotkeyError> {
        if let Some(prev) = self.secondary.take() {
            let _ = self.manager.unregister(prev);
        }
        let Some(raw) = spec.map(str::trim).filter(|s| !s.is_empty()) else {
            return Ok(None);
        };
        let hotkey = parse_hotkey(raw)?;
        if self.active.is_some_and(|primary| primary == hotkey) {
            return Err(HotkeyError::RegistrationFailed {
                spec: raw.to_string(),
                reason: "matches the primary snip hotkey".to_string(),
            });
        }
        self.manager
            .register(hotkey)
            .map_err(|e| HotkeyError::RegistrationFailed {
                spec: raw.to_string(),
                reason: e.to_string(),
            })?;
        self.secondary = Some(hotkey);
        Ok(Some(hotkey))
    }
}

impl Drop for HotkeyRegistration {
    fn drop(&mut self) {
        if let Some(prev) = self.active.take() {
            let _ = self.manager.unregister(prev);
        }
        if let Some(prev) = self.secondary.take() {
            let _ = self.manager.unregister(prev);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_default_hotkey() {
        let hk = parse_hotkey("Ctrl+Shift+O").expect("parse Ctrl+Shift+O");
        assert_eq!(
            hk,
            HotKey::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyO)
        );
    }

    #[test]
    fn parses_case_insensitive_with_spaces() {
        let hk = parse_hotkey("  ctrl + o ").expect("parse ctrl + o");
        assert_eq!(hk, HotKey::new(Some(Modifiers::CONTROL), Code::KeyO));

        let hk2 = parse_hotkey("Alt+Win+F9").expect("parse Alt+Win+F9");
        assert_eq!(
            hk2,
            HotKey::new(Some(Modifiers::ALT | Modifiers::META), Code::F9)
        );
    }

    #[test]
    fn rejects_invalid_hotkey_strings() {
        assert_eq!(parse_hotkey(""), Err(HotkeyError::Empty));
        assert!(matches!(
            parse_hotkey("Ctrl+Shift"),
            Err(HotkeyError::MissingKey { .. })
        ));
        assert!(matches!(
            parse_hotkey("Ctrl+A+B"),
            Err(HotkeyError::MultipleKeys { .. })
        ));
        assert!(matches!(
            parse_hotkey("Ctrl+NotAKey"),
            Err(HotkeyError::UnknownToken { .. })
        ));
    }
}
