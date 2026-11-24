use crate::providers::byok_ai::{extract_api_error, parse_gemini_response, parse_openai_response};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SseProviderKind {
    OpenAi,
    Gemini,
}

/// Incremental SSE line decoder safe across arbitrary chunk boundaries (including split UTF-8).
#[derive(Default)]
pub struct SseLineDecoder {
    buf: Vec<u8>,
}

impl SseLineDecoder {
    pub fn push_bytes(&mut self, chunk: &[u8]) -> Vec<String> {
        self.buf.extend_from_slice(chunk);
        let mut lines = Vec::new();
        while let Some(pos) = self.buf.iter().position(|&b| b == b'\n') {
            let mut raw: Vec<u8> = self.buf.drain(..=pos).collect();
            // Strip trailing '\n' and optional '\r'
            if raw.last() == Some(&b'\n') {
                raw.pop();
            }
            if raw.last() == Some(&b'\r') {
                raw.pop();
            }
            lines.push(String::from_utf8_lossy(&raw).into_owned());
        }
        lines
    }

    pub fn finish(&mut self) -> Option<String> {
        if self.buf.is_empty() {
            return None;
        }
        let mut raw = std::mem::take(&mut self.buf);
        if raw.last() == Some(&b'\r') {
            raw.pop();
        }
        Some(String::from_utf8_lossy(&raw).into_owned())
    }
}

/// Parses one `data:` payload line from an OpenAI / OpenRouter SSE stream.
/// Returns `Ok(Some(text))` for token deltas, `Ok(None)` for metadata / `[DONE]`,
/// or `Err(msg)` if the stream carries an inline JSON error object.
pub fn parse_openai_sse_data(data: &str) -> Result<Option<String>, String> {
    let trimmed = data.trim();
    if trimmed.is_empty() || trimmed == "[DONE]" {
        return Ok(None);
    }
    let root: Value =
        serde_json::from_str(trimmed).map_err(|e| format!("invalid OpenAI SSE chunk: {e}"))?;
    if let Some(err) = extract_api_error(trimmed) {
        return Err(err);
    }

    let delta_content = root
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|arr| arr.first())
        .and_then(|choice| choice.get("delta"))
        .and_then(|delta| delta.get("content"));

    let Some(content) = delta_content else {
        return Ok(None);
    };

    if let Some(s) = content.as_str() {
        return Ok((!s.is_empty()).then(|| s.to_string()));
    }
    if let Some(parts) = content.as_array() {
        let mut out = String::new();
        for part in parts {
            if let Some(t) = part.get("text").and_then(Value::as_str) {
                out.push_str(t);
            }
        }
        return Ok((!out.is_empty()).then_some(out));
    }
    Ok(None)
}

/// Parses one `data:` payload line from a Gemini `:streamGenerateContent?alt=sse` stream.
pub fn parse_gemini_sse_data(data: &str) -> Result<Option<String>, String> {
    let trimmed = data.trim();
    if trimmed.is_empty() || trimmed == "[DONE]" {
        return Ok(None);
    }
    let root: Value =
        serde_json::from_str(trimmed).map_err(|e| format!("invalid Gemini SSE chunk: {e}"))?;
    if let Some(err) = extract_api_error(trimmed) {
        return Err(err);
    }

    let mut out = String::new();
    if let Some(parts) = root
        .get("candidates")
        .and_then(Value::as_array)
        .and_then(|arr| arr.first())
        .and_then(|cand| cand.get("content"))
        .and_then(|content| content.get("parts"))
        .and_then(Value::as_array)
    {
        for part in parts {
            if let Some(t) = part.get("text").and_then(Value::as_str) {
                out.push_str(t);
            }
        }
    }
    Ok((!out.is_empty()).then_some(out))
}

/// Consumes an HTTP 2xx streaming SSE response, emitting every token delta immediately via
/// `on_delta`. Transparently falls back to single-shot JSON parsing if a proxy ignores `stream: true`.
pub async fn consume_sse_or_json(
    mut response: reqwest::Response,
    kind: SseProviderKind,
    on_delta: &(dyn Fn(&str) + Send + Sync),
) -> Result<String, String> {
    let is_json = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|ct| {
            let lower = ct.to_ascii_lowercase();
            lower.contains("application/json") && !lower.contains("text/event-stream")
        });

    if is_json {
        let text = response
            .text()
            .await
            .map_err(|e| format!("failed to read JSON response: {e}"))?;
        let parsed = match kind {
            SseProviderKind::OpenAi => parse_openai_response(&text)?,
            SseProviderKind::Gemini => parse_gemini_response(&text)?,
        };
        if !parsed.is_empty() {
            on_delta(&parsed);
        }
        return Ok(parsed);
    }

    let mut decoder = SseLineDecoder::default();
    let mut accumulated = String::new();
    let mut raw_fallback = String::new();
    let mut saw_sse_data = false;

    let mut emit = |piece: &str| on_delta(piece);
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| format!("stream read error: {e}"))?
    {
        for line in decoder.push_bytes(&chunk) {
            process_sse_line(
                &line,
                kind,
                &mut emit,
                &mut accumulated,
                &mut raw_fallback,
                &mut saw_sse_data,
            )?;
        }
    }

    if let Some(tail) = decoder.finish() {
        process_sse_line(
            &tail,
            kind,
            &mut emit,
            &mut accumulated,
            &mut raw_fallback,
            &mut saw_sse_data,
        )?;
    }

    if !accumulated.is_empty() {
        return Ok(accumulated);
    }

    // Some proxies omit Content-Type or send non-SSE JSON despite stream=true.
    if !saw_sse_data && !raw_fallback.trim().is_empty() {
        let parsed = match kind {
            SseProviderKind::OpenAi => parse_openai_response(&raw_fallback)?,
            SseProviderKind::Gemini => parse_gemini_response(&raw_fallback)?,
        };
        if !parsed.is_empty() {
            on_delta(&parsed);
        }
        return Ok(parsed);
    }

    Err("AI stream ended without text content".to_string())
}

fn process_sse_line(
    line: &str,
    kind: SseProviderKind,
    on_delta: &mut dyn FnMut(&str),
    accumulated: &mut String,
    raw_fallback: &mut String,
    saw_sse_data: &mut bool,
) -> Result<(), String> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with(':') {
        // Blank separator or SSE keepalive comment (e.g. `: OPENROUTER PROCESSING`)
        return Ok(());
    }

    if let Some(data) = trimmed.strip_prefix("data:") {
        *saw_sse_data = true;
        let parsed = match kind {
            SseProviderKind::OpenAi => parse_openai_sse_data(data)?,
            SseProviderKind::Gemini => parse_gemini_sse_data(data)?,
        };
        if let Some(piece) = parsed {
            accumulated.push_str(&piece);
            on_delta(&piece);
        }
    } else if !*saw_sse_data {
        raw_fallback.push_str(line);
        raw_fallback.push('\n');
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decoder_handles_split_chunks_crlf_and_utf8() {
        let mut dec = SseLineDecoder::default();
        // Split a multi-byte Vietnamese UTF-8 character across chunks
        let payload = "data: {\"choices\":[{\"delta\":{\"content\":\"Xin chào\"}}]}\r\n\r\n";
        let bytes = payload.as_bytes();
        let mid = bytes.len() / 2;

        let l1 = dec.push_bytes(&bytes[..mid]);
        let l2 = dec.push_bytes(&bytes[mid..]);
        let all: Vec<String> = l1.into_iter().chain(l2).collect();
        assert_eq!(all.len(), 2);
        let delta = parse_openai_sse_data(all[0].strip_prefix("data:").unwrap())
            .expect("valid sse")
            .expect("has delta");
        assert_eq!(delta, "Xin chào");
    }

    #[test]
    fn openai_sse_ignores_keepalive_comments_and_done_marker() {
        let mut acc = String::new();
        let mut fallback = String::new();
        let mut saw_sse = false;
        let mut chunks = Vec::new();
        let mut push_line = |line: &str| {
            let mut collect = |d: &str| chunks.push(d.to_string());
            process_sse_line(
                line,
                SseProviderKind::OpenAi,
                &mut collect,
                &mut acc,
                &mut fallback,
                &mut saw_sse,
            )
        };

        push_line(": OPENROUTER PROCESSING").unwrap();
        push_line(r#"data: {"choices":[{"delta":{"role":"assistant"}}]}"#).unwrap();
        push_line(r#"data: {"choices":[{"delta":{"content":"Hello "}}]}"#).unwrap();
        push_line(r#"data: {"choices":[{"delta":{"content":"world"}}]}"#).unwrap();
        push_line("data: [DONE]").unwrap();

        assert!(saw_sse);
        assert_eq!(acc, "Hello world");
        assert_eq!(chunks, vec!["Hello ", "world"]);
    }

    #[test]
    fn openai_sse_surfaces_inline_error_object() {
        let err = parse_openai_sse_data(r#"{"error":{"message":"Rate limit exceeded"}}"#)
            .expect_err("must surface inline SSE error");
        assert!(err.contains("Rate limit exceeded"));
    }

    #[test]
    fn gemini_sse_extracts_candidate_parts() {
        let payload = r#"{"candidates":[{"content":{"parts":[{"text":"Step 1: "},{"text":"parse input"}]}}]}"#;
        let delta = parse_gemini_sse_data(payload)
            .expect("valid gemini sse")
            .expect("has text");
        assert_eq!(delta, "Step 1: parse input");
    }
}
