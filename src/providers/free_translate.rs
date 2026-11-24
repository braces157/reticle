use crate::ocr;
use crate::providers::{BoxFuture, SnipAction, SnipError, SnipInput, SnipOutput, SnipProvider};
use serde_json::Value;

const GOOGLE_CLIENTS5_ENDPOINT: &str = "https://clients5.google.com/translate_a/t";
const GOOGLE_GTX_ENDPOINT: &str = "https://translate.googleapis.com/translate_a/single";
const MYMEMORY_ENDPOINT: &str = "https://api.mymemory.translated.net/get";

pub struct FreeTranslateProvider {
    http: reqwest::Client,
    clients5_endpoint: String,
    gtx_endpoint: String,
    mymemory_endpoint: String,
}

impl FreeTranslateProvider {
    pub fn new(http: reqwest::Client) -> Self {
        Self {
            http,
            clients5_endpoint: GOOGLE_CLIENTS5_ENDPOINT.to_string(),
            gtx_endpoint: GOOGLE_GTX_ENDPOINT.to_string(),
            mymemory_endpoint: MYMEMORY_ENDPOINT.to_string(),
        }
    }

    #[cfg(test)]
    fn with_endpoints(
        http: reqwest::Client,
        clients5: impl Into<String>,
        gtx: impl Into<String>,
        mymemory: impl Into<String>,
    ) -> Self {
        Self {
            http,
            clients5_endpoint: clients5.into(),
            gtx_endpoint: gtx.into(),
            mymemory_endpoint: mymemory.into(),
        }
    }

    async fn translate_via_clients5(
        &self,
        text: &str,
        target_language: &str,
    ) -> Result<(String, Option<String>), String> {
        let mut url = reqwest::Url::parse(&self.clients5_endpoint)
            .map_err(|e| format!("invalid clients5 URL: {e}"))?;
        url.query_pairs_mut()
            .append_pair("client", "dict-chrome-ex")
            .append_pair("sl", "auto")
            .append_pair("tl", target_language);

        let encoded_body = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("q", text)
            .finish();

        let response = self
            .http
            .post(url)
            .header(
                reqwest::header::CONTENT_TYPE,
                "application/x-www-form-urlencoded; charset=utf-8",
            )
            .body(encoded_body)
            .send()
            .await
            .map_err(|e| format!("network error: {e}"))?;

        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| format!("failed to read response body: {e}"))?;
        if !status.is_success() {
            return Err(format!("HTTP {status}"));
        }
        parse_clients5_response(&body)
    }

    async fn translate_via_gtx(
        &self,
        text: &str,
        target_language: &str,
    ) -> Result<(String, Option<String>), String> {
        let mut url = reqwest::Url::parse(&self.gtx_endpoint)
            .map_err(|e| format!("invalid gtx URL: {e}"))?;
        url.query_pairs_mut()
            .append_pair("client", "gtx")
            .append_pair("sl", "auto")
            .append_pair("tl", target_language)
            .append_pair("dt", "t")
            .append_pair("q", text);

        let response = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| format!("network error: {e}"))?;

        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| format!("failed to read response body: {e}"))?;
        if !status.is_success() {
            return Err(format!("HTTP {status}"));
        }
        parse_google_translate_response(&body)
    }

    async fn translate_via_mymemory(
        &self,
        text: &str,
        target_language: &str,
    ) -> Result<(String, Option<String>), String> {
        // MyMemory free GET endpoint expects <= 500 chars and an explicit source/autodetect pair.
        let truncated: String = text.chars().take(480).collect();
        let mut url = reqwest::Url::parse(&self.mymemory_endpoint)
            .map_err(|e| format!("invalid MyMemory URL: {e}"))?;
        url.query_pairs_mut()
            .append_pair("q", &truncated)
            .append_pair("langpair", &format!("autodetect|{target_language}"));

        let response = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| format!("network error: {e}"))?;

        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| format!("failed to read response body: {e}"))?;
        if !status.is_success() {
            return Err(format!("HTTP {status}"));
        }
        parse_mymemory_response(&body)
    }

    async fn translate_text(
        &self,
        text: &str,
        target_language: &str,
    ) -> Result<(String, Option<String>, &'static str), String> {
        match self.translate_via_clients5(text, target_language).await {
            Ok((translated, lang)) => return Ok((translated, lang, "Google Translate")),
            Err(err) => log::info!("clients5 translate fallback triggered: {err}"),
        }

        match self.translate_via_gtx(text, target_language).await {
            Ok((translated, lang)) => return Ok((translated, lang, "Google Translate")),
            Err(err) => log::info!("gtx translate fallback triggered: {err}"),
        }

        match self.translate_via_mymemory(text, target_language).await {
            Ok((translated, lang)) => Ok((translated, lang, "MyMemory (Free)")),
            Err(err) => Err(err),
        }
    }
}

impl SnipProvider for FreeTranslateProvider {
    fn name(&self) -> &'static str {
        "Free · Google Translate"
    }

    fn process<'a>(&'a self, input: SnipInput) -> BoxFuture<'a, Result<SnipOutput, SnipError>> {
        Box::pin(async move {
            if matches!(input.action, SnipAction::ExplainCode | SnipAction::Ocr) {
                return Err(SnipError::UnsupportedAction(
                    "This action requires a BYOK API key in config.json".to_string(),
                ));
            }

            let ocr_text = if let Some(pre) = input.precomputed_ocr {
                pre
            } else {
                let img_bytes = input.image_bytes;
                tokio::task::spawn_blocking(move || ocr::extract_text_from_png(&img_bytes))
                    .await
                    .map_err(|e| SnipError::OcrFailed(format!("OCR task panicked: {e}")))?
                    .map_err(|e| SnipError::OcrFailed(e.to_string()))?
            };

            let trimmed = ocr_text.trim().to_string();
            if trimmed.is_empty() {
                return Err(SnipError::NoTextDetected);
            }

            match self.translate_text(&trimmed, &input.target_language).await {
                Ok((translated, detected_lang, engine_label)) => Ok(SnipOutput {
                    primary_text: translated,
                    original_text: Some(trimmed),
                    detected_language: detected_lang,
                    provider_label: format!("Windows OCR · {engine_label}"),
                    warning: None,
                    is_markdown: false,
                }),
                Err(err) => {
                    log::warn!("All free translation endpoints failed; returning raw OCR fallback: {err}");
                    Ok(SnipOutput {
                        primary_text: String::new(),
                        original_text: Some(trimmed),
                        detected_language: None,
                        provider_label: self.name().to_string(),
                        warning: Some(format!(
                            "Translation failed ({err}). Showing raw OCR text below."
                        )),
                        is_markdown: false,
                    })
                }
            }
        })
    }
}

/// Parses `clients5.google.com/translate_a/t?client=dict-chrome-ex` payloads:
/// - `[["Hello world","fr"]]` (when `sl=auto`)
/// - `["Hello world"]` (when source language is explicit)
pub fn parse_clients5_response(raw: &str) -> Result<(String, Option<String>), String> {
    let root: Value =
        serde_json::from_str(raw).map_err(|e| format!("invalid JSON from clients5: {e}"))?;

    let items = root
        .as_array()
        .ok_or_else(|| "unexpected clients5 payload structure".to_string())?;

    let mut translated_parts = Vec::new();
    let mut detected_lang: Option<String> = None;

    for item in items {
        match item {
            Value::Array(pair) => {
                if let Some(chunk) = pair.first().and_then(Value::as_str) {
                    translated_parts.push(chunk);
                }
                if detected_lang.is_none() {
                    detected_lang = pair
                        .get(1)
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_string);
                }
            }
            Value::String(chunk) => translated_parts.push(chunk),
            _ => {}
        }
    }

    let joined = translated_parts.join("");
    let trimmed = joined.trim();
    if trimmed.is_empty() {
        return Err("clients5 returned empty translation".to_string());
    }

    Ok((trimmed.to_string(), detected_lang))
}

/// Parses `translate.googleapis.com/translate_a/single` payloads:
/// `[[["Translated line 1\n", "Source 1", ...]], null, "ja", ...]`
pub fn parse_google_translate_response(raw: &str) -> Result<(String, Option<String>), String> {
    let root: Value =
        serde_json::from_str(raw).map_err(|e| format!("invalid JSON from translator: {e}"))?;

    let segments = root
        .get(0)
        .and_then(Value::as_array)
        .ok_or_else(|| "unexpected translator payload structure".to_string())?;

    let mut translated = String::new();
    for seg in segments {
        if let Some(chunk) = seg.get(0).and_then(Value::as_str) {
            translated.push_str(chunk);
        }
    }

    if translated.trim().is_empty() {
        return Err("translator returned empty translation".to_string());
    }

    let detected_lang = root
        .get(2)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    Ok((translated.trim().to_string(), detected_lang))
}

pub fn parse_mymemory_response(raw: &str) -> Result<(String, Option<String>), String> {
    let root: Value =
        serde_json::from_str(raw).map_err(|e| format!("invalid JSON from MyMemory: {e}"))?;

    if root
        .get("quotaFinished")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Err("MyMemory daily quota reached".to_string());
    }

    let translated = root
        .get("responseData")
        .and_then(|d| d.get("translatedText"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "MyMemory returned empty translation".to_string())?;

    let detected = root
        .get("responseData")
        .and_then(|d| d.get("detectedLanguage"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    Ok((translated.to_string(), detected))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_clients5_nested_and_flat_responses() {
        let nested = r#"[["Hello world!\nThis is a test.","fr"]]"#;
        let (text, lang) = parse_clients5_response(nested).expect("parse clients5 nested");
        assert_eq!(text, "Hello world!\nThis is a test.");
        assert_eq!(lang.as_deref(), Some("fr"));

        let flat = r#"["Xin chào thế giới"]"#;
        let (text2, lang2) = parse_clients5_response(flat).expect("parse clients5 flat");
        assert_eq!(text2, "Xin chào thế giới");
        assert_eq!(lang2, None);
    }

    #[test]
    fn parses_multi_segment_google_translate_response() {
        let payload = r#"[
            [
                ["Hello world!\n", "Xin chào thế giới!\n", null, null, 10],
                ["How are you?", "Bạn khỏe không?", null, null, 10]
            ],
            null,
            "vi",
            null,
            null,
            null,
            1.0
        ]"#;
        let (translated, detected) =
            parse_google_translate_response(payload).expect("parse valid payload");
        assert_eq!(translated, "Hello world!\nHow are you?");
        assert_eq!(detected.as_deref(), Some("vi"));
    }

    #[test]
    fn parses_mymemory_response() {
        let payload = r#"{
            "responseData": {
                "translatedText": "Hello world",
                "detectedLanguage": "fr"
            },
            "quotaFinished": false
        }"#;
        let (text, lang) = parse_mymemory_response(payload).expect("parse mymemory");
        assert_eq!(text, "Hello world");
        assert_eq!(lang.as_deref(), Some("fr"));
    }

    #[test]
    fn rejects_malformed_or_empty_responses() {
        assert!(parse_clients5_response("{}").is_err());
        assert!(parse_clients5_response("[]").is_err());
        assert!(parse_google_translate_response("not json").is_err());
        assert!(parse_google_translate_response("{}").is_err());
        assert!(parse_google_translate_response("[[]]").is_err());
        assert!(parse_mymemory_response(r#"{"quotaFinished": true}"#).is_err());
    }

    #[tokio::test]
    async fn blank_image_returns_no_text_detected_without_calling_network() {
        let provider = FreeTranslateProvider::with_endpoints(
            reqwest::Client::new(),
            "http://127.0.0.1:1/c5",
            "http://127.0.0.1:1/gtx",
            "http://127.0.0.1:1/mm",
        );
        let blank_png = crate::ocr::tests::render_text_png("", 220, 80);
        let res = provider
            .process(SnipInput::from_png(
                blank_png,
                SnipAction::Translate,
                "en".to_string(),
            ))
            .await;
        assert_eq!(res, Err(SnipError::NoTextDetected));
    }

    #[tokio::test]
    async fn network_failure_preserves_raw_ocr_text_with_warning() {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(2))
            .build()
            .expect("http client");
        let provider = FreeTranslateProvider::with_endpoints(
            http,
            "http://127.0.0.1:1/c5",
            "http://127.0.0.1:1/gtx",
            "http://127.0.0.1:1/mm",
        );
        let text_png = crate::ocr::tests::render_text_png("Hello Rust OCR", 460, 120);
        let output = provider
            .process(SnipInput::from_png(
                text_png,
                SnipAction::Translate,
                "vi".to_string(),
            ))
            .await
            .expect("must gracefully return OCR fallback on network error");

        assert!(output.primary_text.is_empty());
        assert!(output
            .warning
            .as_deref()
            .unwrap_or("")
            .contains("Translation failed"));
        let raw_ocr = output.original_text.unwrap_or_default().to_ascii_lowercase();
        assert!(
            raw_ocr.contains("hello") && raw_ocr.contains("rust"),
            "expected preserved OCR text, got: {raw_ocr:?}"
        );
    }
}
