use crate::config::{AiProviderConfig, AiProviderKind};
use crate::ocr;
use crate::providers::{BoxFuture, SnipAction, SnipError, SnipInput, SnipOutput, SnipProvider};
use base64::Engine;
use serde_json::{json, Value};

const DEFAULT_OPENAI_BASE_URL: &str = "https://api.openai.com/v1";
const GEMINI_API_BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta/models";

pub struct ByokAiProvider {
    http: reqwest::Client,
    config: AiProviderConfig,
}

impl ByokAiProvider {
    pub fn new(http: reqwest::Client, config: AiProviderConfig) -> Self {
        Self { http, config }
    }

    fn label(&self) -> String {
        match self.config.provider {
            AiProviderKind::Gemini => format!("Gemini ({})", self.config.effective_model()),
            AiProviderKind::OpenAi => format!("OpenAI ({})", self.config.effective_model()),
        }
    }

    fn build_prompt(action: SnipAction, target_language: &str) -> String {
        match action {
            SnipAction::Translate => format!(
                "You are a precise, layout-aware OCR and translation assistant. \
                 Read all visible text from the image and translate it cleanly into `{target_language}`. \
                 Preserve paragraphs, lists, and code snippets where appropriate. \
                 If the image contains no readable text at all, respond with exactly `NO_TEXT_DETECTED`."
            ),
            SnipAction::ExplainCode => format!(
                "You are a concise senior software engineer. \
                 Read the code or technical content shown in the image and explain it clearly in `{target_language}` using GitHub-flavored Markdown. \
                 Include: (1) a one-sentence summary, (2) a clean fenced code block of the transcribed code if helpful, \
                 (3) step-by-step explanation of key logic, and (4) notable edge cases or potential bugs. \
                 If there is no readable code or text in the image, respond with exactly `NO_TEXT_DETECTED`."
            ),
        }
    }

    async fn call_gemini(&self, png_b64: &str, prompt: &str) -> Result<String, String> {
        let model = self.config.effective_model();
        let url = format!("{GEMINI_API_BASE_URL}/{model}:generateContent");
        let body = json!({
            "contents": [{
                "parts": [
                    { "text": prompt },
                    {
                        "inline_data": {
                            "mime_type": "image/png",
                            "data": png_b64
                        }
                    }
                ]
            }]
        });

        let response = self
            .http
            .post(&url)
            .header("x-goog-api-key", self.config.api_key.trim())
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("network error: {e}"))?;

        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|e| format!("failed to read Gemini response: {e}"))?;

        if !status.is_success() {
            let detail = extract_api_error(&text).unwrap_or_else(|| format!("HTTP {status}"));
            return Err(format!("Gemini API error ({status}): {detail}"));
        }

        parse_gemini_response(&text)
    }

    async fn call_openai(&self, png_b64: &str, prompt: &str) -> Result<String, String> {
        let base = self
            .config
            .base_url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(DEFAULT_OPENAI_BASE_URL)
            .trim_end_matches('/');
        let url = format!("{base}/chat/completions");
        let body = build_openai_request_body(base, self.config.effective_model(), prompt, png_b64);

        let response = self
            .http
            .post(&url)
            .bearer_auth(self.config.api_key.trim())
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("network error: {e}"))?;

        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|e| format!("failed to read OpenAI response: {e}"))?;

        if !status.is_success() {
            let detail = extract_api_error(&text).unwrap_or_else(|| format!("HTTP {status}"));
            return Err(format!("OpenAI API error ({status}): {detail}"));
        }

        parse_openai_response(&text)
    }
}

impl SnipProvider for ByokAiProvider {
    fn name(&self) -> &'static str {
        match self.config.provider {
            AiProviderKind::Gemini => "Gemini Vision",
            AiProviderKind::OpenAi => "OpenAI Vision",
        }
    }

    fn process<'a>(&'a self, input: SnipInput) -> BoxFuture<'a, Result<SnipOutput, SnipError>> {
        Box::pin(async move {
            let png_b64 = base64::engine::general_purpose::STANDARD.encode(&input.png_bytes);
            let prompt = Self::build_prompt(input.action, &input.target_language);

            // Run local Windows OCR concurrently so we have original text for the collapsible drawer
            // and a clean fallback if the remote AI call fails.
            let ocr_png = input.png_bytes;
            let ocr_fut = tokio::task::spawn_blocking(move || ocr::extract_text_from_png(&ocr_png));
            let ai_fut = async {
                match self.config.provider {
                    AiProviderKind::Gemini => self.call_gemini(&png_b64, &prompt).await,
                    AiProviderKind::OpenAi => self.call_openai(&png_b64, &prompt).await,
                }
            };

            let (ocr_res, ai_res) = tokio::join!(ocr_fut, ai_fut);
            let ocr_text = ocr_res
                .ok()
                .and_then(Result::ok)
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());

            match ai_res {
                Ok(content) => {
                    let trimmed = content.trim();
                    if trimmed.eq_ignore_ascii_case("NO_TEXT_DETECTED") && ocr_text.is_none() {
                        return Err(SnipError::NoTextDetected);
                    }
                    Ok(SnipOutput {
                        primary_text: trimmed.to_string(),
                        original_text: ocr_text,
                        detected_language: None,
                        provider_label: self.label(),
                        warning: None,
                        is_markdown: input.action == SnipAction::ExplainCode,
                    })
                }
                Err(err) => {
                    log::warn!("BYOK AI provider failed: {err}");
                    if let Some(raw_ocr) = ocr_text {
                        Ok(SnipOutput {
                            primary_text: String::new(),
                            original_text: Some(raw_ocr),
                            detected_language: None,
                            provider_label: self.label(),
                            warning: Some(format!("{err}. Showing local OCR text below.")),
                            is_markdown: false,
                        })
                    } else {
                        Err(SnipError::ProviderFailed(err))
                    }
                }
            }
        })
    }
}

pub fn parse_gemini_response(raw: &str) -> Result<String, String> {
    let root: Value =
        serde_json::from_str(raw).map_err(|e| format!("invalid Gemini JSON: {e}"))?;

    let parts = root
        .get("candidates")
        .and_then(Value::as_array)
        .and_then(|c| c.first())
        .and_then(|c| c.get("content"))
        .and_then(|c| c.get("parts"))
        .and_then(Value::as_array)
        .ok_or_else(|| "Gemini response contained no candidate text parts".to_string())?;

    let mut out = String::new();
    for part in parts {
        if let Some(text) = part.get("text").and_then(Value::as_str) {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(text);
        }
    }

    let trimmed = out.trim();
    if trimmed.is_empty() {
        return Err("Gemini returned an empty response".to_string());
    }
    Ok(trimmed.to_string())
}

pub fn parse_openai_response(raw: &str) -> Result<String, String> {
    let root: Value =
        serde_json::from_str(raw).map_err(|e| format!("invalid OpenAI JSON: {e}"))?;

    let content = root
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|c| c.first())
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .ok_or_else(|| "OpenAI response contained no choice message content".to_string())?;

    let text = match content {
        Value::String(s) => s.clone(),
        Value::Array(items) => items
            .iter()
            .filter_map(|item| item.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => return Err("unexpected OpenAI message content type".to_string()),
    };

    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err("OpenAI returned an empty response".to_string());
    }
    Ok(trimmed.to_string())
}

pub fn extract_api_error(raw: &str) -> Option<String> {
    let root: Value = serde_json::from_str(raw).ok()?;
    root.get("error")
        .and_then(|e| e.get("message").or(Some(e)))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn is_openrouter_base_url(base_url: &str) -> bool {
    let lowered = base_url.trim().to_ascii_lowercase();
    let after_scheme = lowered
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(lowered.as_str());
    let host = after_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("");
    host == "openrouter.ai" || host.ends_with(".openrouter.ai")
}

fn build_openai_request_body(base_url: &str, model: &str, prompt: &str, png_b64: &str) -> Value {
    let data_uri = format!("data:image/png;base64,{png_b64}");
    let mut body = json!({
        "model": model,
        "messages": [{
            "role": "user",
            "content": [
                { "type": "text", "text": prompt },
                {
                    "type": "image_url",
                    "image_url": { "url": data_uri }
                }
            ]
        }]
    });

    if is_openrouter_base_url(base_url)
        && let Some(obj) = body.as_object_mut()
    {
        obj.insert("reasoning".to_string(), json!({ "enabled": false }));
    }

    body
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_gemini_candidates_text() {
        let raw = r####"{
            "candidates": [{
                "content": {
                    "parts": [
                        { "text": "### Summary\nThis function sorts numbers." },
                        { "text": "```rust\nfn main() {}\n```" }
                    ]
                }
            }]
        }"####;
        let parsed = parse_gemini_response(raw).expect("parse gemini");
        assert!(parsed.contains("This function sorts numbers."));
        assert!(parsed.contains("```rust"));
    }

    #[test]
    fn parses_openai_string_and_array_content() {
        let raw_str = r#"{
            "choices": [{
                "message": { "role": "assistant", "content": "Translated text" }
            }]
        }"#;
        assert_eq!(parse_openai_response(raw_str).expect("str"), "Translated text");

        let raw_arr = r#"{
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": [
                        { "type": "text", "text": "Line 1" },
                        { "type": "text", "text": "Line 2" }
                    ]
                }
            }]
        }"#;
        assert_eq!(parse_openai_response(raw_arr).expect("arr"), "Line 1\nLine 2");
    }

    #[test]
    fn extracts_provider_error_messages() {
        let gemini_err = r#"{ "error": { "code": 400, "message": "API key not valid." } }"#;
        assert_eq!(
            extract_api_error(gemini_err).as_deref(),
            Some("API key not valid.")
        );
        let openai_err = r#"{ "error": { "message": "Incorrect API key provided" } }"#;
        assert_eq!(
            extract_api_error(openai_err).as_deref(),
            Some("Incorrect API key provided")
        );
    }

    #[test]
    fn disables_reasoning_only_for_openrouter_requests() {
        let openrouter_body = build_openai_request_body(
            "https://openrouter.ai/api/v1",
            "nvidia/nemotron-3-nano-omni-30b-a3b-reasoning:free",
            "Translate this",
            "AAAA",
        );
        assert_eq!(openrouter_body["reasoning"]["enabled"], Value::Bool(false));

        let openai_body = build_openai_request_body(
            DEFAULT_OPENAI_BASE_URL,
            "gpt-4o-mini",
            "Translate this",
            "AAAA",
        );
        assert!(openai_body.get("reasoning").is_none());

        assert!(!is_openrouter_base_url("https://example.com/openrouter.ai/v1"));
        assert!(is_openrouter_base_url("https://api.openrouter.ai/v1"));
    }
}
