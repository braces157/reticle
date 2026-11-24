use crate::ocr_format::SmartFormatKind;
use crate::ui::theme;
use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, FontId};

// Matte-obsidian editorial syntax palette (tuned for `#121212` card surface)
const SYN_KEYWORD: Color32 = Color32::from_rgb(196, 181, 253); // Soft violet-lavender
const SYN_STRING: Color32 = Color32::from_rgb(110, 231, 183); // Emerald mint
const SYN_KEY: Color32 = Color32::from_rgb(125, 211, 252); // Optical sky blue
const SYN_TYPE: Color32 = Color32::from_rgb(103, 232, 249); // Crisp cyan
const SYN_NUMBER: Color32 = Color32::from_rgb(252, 211, 77); // Warm amber
const SYN_COMMENT: Color32 = Color32::from_rgb(110, 115, 130); // Quiet slate
const SYN_PUNCT: Color32 = Color32::from_rgb(142, 149, 166); // Cool silver
const SYN_BODY: Color32 = Color32::from_rgb(234, 236, 239); // Crisp readable off-white
const SYN_HEADING: Color32 = Color32::from_rgb(255, 255, 255); // Pure white emphasis

/// Semantic pill colors `(fill, stroke, text, dot)` for each smart-format mode.
pub fn format_badge_palette(kind: Option<SmartFormatKind>) -> (Color32, Color32, Color32, Color32) {
    match kind {
        Some(SmartFormatKind::Code | SmartFormatKind::Json) => (
            Color32::from_rgb(16, 26, 38),
            Color32::from_rgb(36, 62, 92),
            SYN_KEY,
            theme::ACCENT_PICK,
        ),
        Some(SmartFormatKind::Prose | SmartFormatKind::Cjk) => (
            Color32::from_rgb(15, 29, 23),
            Color32::from_rgb(31, 64, 48),
            SYN_STRING,
            theme::OK_EMERALD,
        ),
        Some(SmartFormatKind::List | SmartFormatKind::Table) => (
            Color32::from_rgb(24, 20, 38),
            Color32::from_rgb(52, 42, 82),
            SYN_KEYWORD,
            SYN_KEYWORD,
        ),
        None => (
            theme::BG_PILL,
            theme::BORDER_HAIRLINE,
            theme::TEXT_MUTED,
            theme::TEXT_MUTED,
        ),
    }
}

/// Builds a rich, live syntax-colored `LayoutJob` for `egui::TextEdit::multiline`
/// tailored to the detected `SmartFormatKind`.
pub fn build_ocr_layout_job(
    text: &str,
    wrap_width: f32,
    kind: Option<SmartFormatKind>,
) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.wrap.max_width = wrap_width;

    match kind {
        Some(SmartFormatKind::Code | SmartFormatKind::Json) => {
            highlight_code(&mut job, text, FontId::monospace(12.6));
        }
        Some(SmartFormatKind::Table) => {
            highlight_table(&mut job, text, FontId::proportional(13.4));
        }
        Some(SmartFormatKind::List) => {
            highlight_list(&mut job, text, FontId::proportional(13.5));
        }
        Some(SmartFormatKind::Prose | SmartFormatKind::Cjk) => {
            highlight_prose(&mut job, text, FontId::proportional(13.6));
        }
        None => {
            highlight_prose(&mut job, text, FontId::proportional(13.6));
        }
    }

    job
}

fn push_span(job: &mut LayoutJob, slice: &str, font: &FontId, color: Color32) {
    if slice.is_empty() {
        return;
    }
    job.append(
        slice,
        0.0,
        TextFormat {
            font_id: font.clone(),
            color,
            ..Default::default()
        },
    );
}

fn is_keyword(word: &str) -> bool {
    matches!(
        word,
        "fn" | "pub"
            | "let"
            | "mut"
            | "const"
            | "static"
            | "struct"
            | "enum"
            | "impl"
            | "trait"
            | "type"
            | "use"
            | "mod"
            | "match"
            | "if"
            | "else"
            | "for"
            | "while"
            | "loop"
            | "return"
            | "break"
            | "continue"
            | "async"
            | "await"
            | "self"
            | "Self"
            | "true"
            | "false"
            | "null"
            | "None"
            | "Some"
            | "Ok"
            | "Err"
            | "def"
            | "class"
            | "import"
            | "from"
            | "export"
            | "default"
            | "function"
            | "var"
            | "interface"
            | "extends"
            | "new"
            | "try"
            | "catch"
            | "throw"
            | "SELECT"
            | "FROM"
            | "WHERE"
    )
}

fn highlight_code(job: &mut LayoutJob, text: &str, font: FontId) {
    for (line_idx, line) in text.split('\n').enumerate() {
        if line_idx > 0 {
            push_span(job, "\n", &font, SYN_BODY);
        }
        highlight_code_line(job, line, &font);
    }
}

fn highlight_code_line(job: &mut LayoutJob, line: &str, font: &FontId) {
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        // Line comment (`//` or leading `#` that isn't Rust `#[`)
        if (bytes[i] == b'/' && i + 1 < len && bytes[i + 1] == b'/')
            || (bytes[i] == b'#' && bytes.get(i + 1) != Some(&b'['))
        {
            push_span(job, &line[i..], font, SYN_COMMENT);
            return;
        }

        // Quoted string literal (or JSON key if followed by `:`)
        if matches!(bytes[i], b'"' | b'\'' | b'`') {
            let quote = bytes[i];
            let start = i;
            i += 1;
            while i < len {
                if bytes[i] == b'\\' && i + 1 < len {
                    i += 2;
                    continue;
                }
                if bytes[i] == quote {
                    i += 1;
                    break;
                }
                i += 1;
            }
            let after = line[i..].trim_start();
            let color = if quote == b'"' && after.starts_with(':') {
                SYN_KEY
            } else {
                SYN_STRING
            };
            push_span(job, &line[start..i], font, color);
            continue;
        }

        // Number literal
        if bytes[i].is_ascii_digit() {
            let start = i;
            i += 1;
            while i < len
                && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'.' || bytes[i] == b'_')
            {
                i += 1;
            }
            push_span(job, &line[start..i], font, SYN_NUMBER);
            continue;
        }

        // Identifier / keyword / PascalCase type
        if bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' {
            let start = i;
            i += 1;
            while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let word = &line[start..i];
            let color = if is_keyword(word) {
                SYN_KEYWORD
            } else if word.chars().next().is_some_and(|c| c.is_ascii_uppercase())
                && word.chars().any(|c| c.is_ascii_lowercase())
            {
                SYN_TYPE
            } else {
                SYN_BODY
            };
            push_span(job, word, font, color);
            continue;
        }

        // Punctuation & operators
        if bytes[i].is_ascii_punctuation() {
            let start = i;
            i += 1;
            while i < len
                && bytes[i].is_ascii_punctuation()
                && !matches!(bytes[i], b'"' | b'\'' | b'`' | b'_' | b'/' | b'#')
            {
                i += 1;
            }
            push_span(job, &line[start..i], font, SYN_PUNCT);
            continue;
        }

        // Whitespace or UTF-8 characters
        let ch = line[i..].chars().next().unwrap();
        let next = i + ch.len_utf8();
        push_span(job, &line[i..next], font, SYN_BODY);
        i = next;
    }
}

fn highlight_table(job: &mut LayoutJob, text: &str, font: FontId) {
    for (idx, line) in text.split('\n').enumerate() {
        if idx > 0 {
            push_span(job, "\n", &font, SYN_BODY);
        }
        if let Some(colon) = line.find(':')
            && colon > 0
            && !line[colon..].starts_with("://")
        {
            push_span(job, &line[..colon], &font, SYN_KEY);
            push_span(job, ":", &font, SYN_PUNCT);
            highlight_inline_accents(job, &line[colon + 1..], &font, SYN_HEADING);
        } else {
            push_span(job, line, &font, SYN_BODY);
        }
    }
}

fn highlight_list(job: &mut LayoutJob, text: &str, font: FontId) {
    for (idx, line) in text.split('\n').enumerate() {
        if idx > 0 {
            push_span(job, "\n", &font, SYN_BODY);
        }
        let rest = if let Some(after) = line.strip_prefix("- ") {
            push_span(job, "- ", &font, theme::ACCENT_PICK);
            after
        } else if let Some(dot_sp) = line.find(". ").filter(|&p| p <= 3 && line[..p].chars().all(|c| c.is_ascii_digit())) {
            push_span(job, &line[..dot_sp + 2], &font, theme::ACCENT_PICK);
            &line[dot_sp + 2..]
        } else {
            line
        };

        if let Some(colon) = rest.find(": ").filter(|&p| p <= 30) {
            push_span(job, &rest[..colon + 1], &font, SYN_HEADING);
            highlight_inline_accents(job, &rest[colon + 1..], &font, SYN_BODY);
        } else {
            highlight_inline_accents(job, rest, &font, SYN_BODY);
        }
    }
}

fn highlight_prose(job: &mut LayoutJob, text: &str, font: FontId) {
    let lines: Vec<&str> = text.split('\n').collect();
    let has_heading = lines.len() >= 3
        && !lines[0].trim().is_empty()
        && lines[0].chars().count() <= 48
        && lines[1].trim().is_empty();
    let heading_font = FontId::proportional(14.5);

    for (idx, line) in lines.iter().enumerate() {
        if idx > 0 {
            push_span(job, "\n", &font, SYN_BODY);
        }
        if idx == 0 && has_heading {
            push_span(job, line, &heading_font, SYN_HEADING);
        } else {
            highlight_inline_accents(job, line, &font, SYN_BODY);
        }
    }
}

/// Subtly highlights inline URLs and backtick spans inside prose/list/table values.
fn highlight_inline_accents(job: &mut LayoutJob, text: &str, font: &FontId, base: Color32) {
    let mut rest = text;
    while !rest.is_empty() {
        if let Some(tick) = rest.find('`')
            && let Some(end_rel) = rest[tick + 1..].find('`')
        {
            push_span(job, &rest[..tick], font, base);
            let span_end = tick + 1 + end_rel + 1;
            push_span(job, &rest[tick..span_end], &FontId::monospace(12.6), SYN_KEY);
            rest = &rest[span_end..];
            continue;
        }
        if let Some(http_idx) = rest.find("https://").or_else(|| rest.find("http://")) {
            push_span(job, &rest[..http_idx], font, base);
            let after = &rest[http_idx..];
            let url_end = after
                .find(|c: char| c.is_whitespace() || matches!(c, ')' | ']' | ','))
                .unwrap_or(after.len());
            push_span(job, &after[..url_end], font, SYN_KEY);
            rest = &after[url_end..];
            continue;
        }
        push_span(job, rest, font, base);
        break;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_highlighter_preserves_exact_text_and_colors_keywords_and_strings() {
        let src = "fn main() {\n    let s = \"hi\"; // note\n}";
        let job = build_ocr_layout_job(src, 360.0, Some(SmartFormatKind::Code));
        assert_eq!(job.text, src);
        assert!(job.sections.iter().any(|s| s.format.color == SYN_KEYWORD));
        assert!(job.sections.iter().any(|s| s.format.color == SYN_STRING));
        assert!(job.sections.iter().any(|s| s.format.color == SYN_COMMENT));
    }

    #[test]
    fn json_and_table_highlighters_color_keys_in_optical_blue() {
        let json = "{\n  \"model\": \"reticle\",\n  \"fast\": true\n}";
        let job = build_ocr_layout_job(json, 360.0, Some(SmartFormatKind::Json));
        assert_eq!(job.text, json);
        assert!(job.sections.iter().any(|s| s.format.color == SYN_KEY));
        assert!(job.sections.iter().any(|s| s.format.color == SYN_STRING));
        assert!(job.sections.iter().any(|s| s.format.color == SYN_KEYWORD));

        let kv = "Host: https://openrouter.ai\nStatus: 200 OK";
        let kv_job = build_ocr_layout_job(kv, 360.0, Some(SmartFormatKind::Table));
        assert_eq!(kv_job.text, kv);
        assert!(kv_job.sections.iter().any(|s| s.format.color == SYN_KEY));
    }
}
