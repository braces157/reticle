use crate::config::{AiProviderConfig, AiProviderKind};
use crate::ocr;
use crate::providers::stream::{consume_sse_or_json, SseProviderKind};
use crate::providers::{
    BoxFuture, DeltaCallback, SnipAction, SnipError, SnipInput, SnipOutput, SnipProvider,
};
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
                "Translate all visible text in the image cleanly into `{target_language}`. \
                 Output only the translation, preserving layout/lists/code identifiers. \
                 If there is no readable text, reply exactly `NO_TEXT_DETECTED`."
            ),
            SnipAction::ExplainCode => format!(
                "You are a sharp, practical senior software engineer explaining a screenshot of code or technical output to a developer in `{target_language}` using clean GitHub-flavored Markdown. \
                 Do NOT mechanically restate syntax line-by-line and do NOT retype the whole screenshot. \
                 Instead explain:\n\
                 - **What it does**: 1–2 plain-English sentences on the purpose, inputs, and return value/effect.\n\
                 - **How it works**: concise bullets explaining the *why* behind each meaningful step, state transition, or side effect (reference key symbols in inline `code`).\n\
                 - **Watch out for**: concrete edge cases, hidden assumptions, or subtle bugs (only include this section if there is a real, non-obvious catch).\n\
                 If there is no readable code or technical text in the image, reply exactly `NO_TEXT_DETECTED`."
            ),
            SnipAction::Ocr => {
                "Transcribe all visible text and code from this image verbatim. \
                 Preserve exact characters, capitalization, underscores, punctuation, symbols, indentation, and line breaks. \
                 Output ONLY the raw transcribed text with no markdown code fences (no ```), no commentary, and no explanation. \
                 If there is no readable text at all, reply exactly `NO_TEXT_DETECTED`."
                    .to_string()
            }
        }
    }

    async fn call_gemini_stream(
        &self,
        image_b64: &str,
        image_mime: &str,
        prompt: &str,
        on_delta: Option<&DeltaCallback>,
    ) -> Result<String, String> {
        let model = self.config.effective_model();
        let streaming = on_delta.is_some();
        let url = if streaming {
            format!("{GEMINI_API_BASE_URL}/{model}:streamGenerateContent?alt=sse")
        } else {
            format!("{GEMINI_API_BASE_URL}/{model}:generateContent")
        };
        let body = json!({
            "contents": [{
                "parts": [
                    { "text": prompt },
                    {
                        "inline_data": {
                            "mime_type": image_mime,
                            "data": image_b64
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
        if !status.is_success() {
            let text = response
                .text()
                .await
                .unwrap_or_else(|_| format!("HTTP {status}"));
            let detail = extract_api_error(&text).unwrap_or_else(|| format!("HTTP {status}"));
            return Err(format!("Gemini API error ({status}): {detail}"));
        }

        if let Some(cb) = on_delta {
            consume_sse_or_json(response, SseProviderKind::Gemini, cb.as_ref()).await
        } else {
            let text = response
                .text()
                .await
                .map_err(|e| format!("failed to read Gemini response: {e}"))?;
            parse_gemini_response(&text)
        }
    }

    async fn call_openai_stream(
        &self,
        image_b64: &str,
        image_mime: &str,
        prompt: &str,
        on_delta: Option<&DeltaCallback>,
    ) -> Result<String, String> {
        let base = self
            .config
            .base_url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(DEFAULT_OPENAI_BASE_URL)
            .trim_end_matches('/');
        let url = format!("{base}/chat/completions");
        let streaming = on_delta.is_some();
        let body = build_openai_request_body(
            base,
            self.config.effective_model(),
            prompt,
            image_b64,
            image_mime,
            streaming,
        );

        let response = self
            .http
            .post(&url)
            .bearer_auth(self.config.api_key.trim())
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("network error: {e}"))?;

        let status = response.status();
        if !status.is_success() {
            let text = response
                .text()
                .await
                .unwrap_or_else(|_| format!("HTTP {status}"));
            let detail = extract_api_error(&text).unwrap_or_else(|| format!("HTTP {status}"));
            return Err(format!("OpenAI API error ({status}): {detail}"));
        }

        if let Some(cb) = on_delta {
            consume_sse_or_json(response, SseProviderKind::OpenAi, cb.as_ref()).await
        } else {
            let text = response
                .text()
                .await
                .map_err(|e| format!("failed to read OpenAI response: {e}"))?;
            parse_openai_response(&text)
        }
    }

    async fn run_request(
        &self,
        input: SnipInput,
        on_delta: Option<DeltaCallback>,
    ) -> Result<SnipOutput, SnipError> {
        let image_b64 = base64::engine::general_purpose::STANDARD.encode(&input.image_bytes);
        let prompt = Self::build_prompt(input.action, &input.target_language);

        let precomputed_ocr = input
            .precomputed_ocr
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        // Only spawn background PNG OCR if we weren't already given precomputed OCR text.
        // Never block AI streaming on OCR completion when precomputed text is already available.
        let ocr_handle = if precomputed_ocr.is_none() && input.image_mime == "image/png" {
            let ocr_png = input.image_bytes;
            Some(tokio::task::spawn_blocking(move || {
                ocr::extract_text_from_png(&ocr_png)
            }))
        } else {
            None
        };

        let ai_res = match self.config.provider {
            AiProviderKind::Gemini => {
                self.call_gemini_stream(&image_b64, input.image_mime, &prompt, on_delta.as_ref())
                    .await
            }
            AiProviderKind::OpenAi => {
                self.call_openai_stream(&image_b64, input.image_mime, &prompt, on_delta.as_ref())
                    .await
            }
        };

        let ocr_text = if precomputed_ocr.is_some() {
            precomputed_ocr
        } else if let Some(handle) = ocr_handle {
            handle
                .await
                .ok()
                .and_then(Result::ok)
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        } else {
            None
        };

        match ai_res {
            Ok(content) => {
                let cleaned = if input.action == SnipAction::Ocr {
                    strip_outer_code_fence(&content)
                } else {
                    content.trim().to_string()
                };
                if cleaned.eq_ignore_ascii_case("NO_TEXT_DETECTED") && ocr_text.is_none() {
                    return Err(SnipError::NoTextDetected);
                }
                Ok(SnipOutput {
                    primary_text: cleaned,
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
        Box::pin(async move { self.run_request(input, None).await })
    }

    fn process_streaming<'a>(
        &'a self,
        input: SnipInput,
        on_delta: DeltaCallback,
    ) -> BoxFuture<'a, Result<SnipOutput, SnipError>> {
        Box::pin(async move { self.run_request(input, Some(on_delta)).await })
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

fn build_openai_request_body(
    base_url: &str,
    model: &str,
    prompt: &str,
    image_b64: &str,
    image_mime: &str,
    stream: bool,
) -> Value {
    let data_uri = format!("data:{image_mime};base64,{image_b64}");
    let mut body = json!({
        "model": model,
        "stream": stream,
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

/// Strips an accidental outer Markdown code fence (` ```lang\n...\n``` `) if an LLM
/// wraps verbatim Vision OCR output despite instructions.
pub fn strip_outer_code_fence(raw: &str) -> String {
    let trimmed = raw.trim();
    let Some(rest) = trimmed.strip_prefix("```") else {
        return trimmed.to_string();
    };
    let Some(first_nl) = rest.find('\n') else {
        return trimmed.to_string();
    };
    let after_open = &rest[first_nl + 1..];
    if let Some(body) = after_open.strip_suffix("```") {
        body.trim_end_matches(['\r', '\n']).to_string()
    } else {
        // Streaming partial fence before closing ``` arrives
        after_open.to_string()
    }
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
            "image/jpeg",
            true,
        );
        assert_eq!(openrouter_body["reasoning"]["enabled"], Value::Bool(false));
        assert_eq!(openrouter_body["stream"], Value::Bool(true));
        assert!(openrouter_body["messages"][0]["content"][1]["image_url"]["url"]
            .as_str()
            .unwrap()
            .starts_with("data:image/jpeg;base64,"));

        let openai_body = build_openai_request_body(
            DEFAULT_OPENAI_BASE_URL,
            "gpt-4o-mini",
            "Translate this",
            "AAAA",
            "image/png",
            false,
        );
        assert!(openai_body.get("reasoning").is_none());
        assert_eq!(openai_body["stream"], Value::Bool(false));

        assert!(!is_openrouter_base_url("https://example.com/openrouter.ai/v1"));
        assert!(is_openrouter_base_url("https://api.openrouter.ai/v1"));
    }

    #[test]
    fn strip_outer_code_fence_removes_markdown_wrapper_preserving_indent() {
        let wrapped = "```rust\n    pub fn demo() {\n        println!(\"ok\");\n    }\n```";
        assert_eq!(
            strip_outer_code_fence(wrapped),
            "    pub fn demo() {\n        println!(\"ok\");\n    }"
        );
        assert_eq!(strip_outer_code_fence("plain text"), "plain text");
    }
}
