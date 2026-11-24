/// Offline, zero-latency smart formatting utilities for raw `Windows.Media.Ocr` output.
/// Handles CJK inter-character space artifacts, wrapped prose paragraphs, hyphenated line breaks,
/// noisy list bullets, and code-preserving cleanup.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmartFormatKind {
    Prose,
    Code,
    List,
    Cjk,
}

impl SmartFormatKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Prose => "Prose",
            Self::Code => "Code",
            Self::List => "List",
            Self::Cjk => "CJK",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmartFormatResult {
    pub text: String,
    pub kind: SmartFormatKind,
}

/// Automatically detects whether OCR text is code, a bullet/numbered list, CJK text, or prose,
/// and applies tailored cleanup without destroying intentional structure.
pub fn smart_format(raw: &str) -> SmartFormatResult {
    let normalized = raw.replace("\r\n", "\n").replace('\r', "\n");
    let trimmed = normalized.trim();
    if trimmed.is_empty() {
        return SmartFormatResult {
            text: String::new(),
            kind: SmartFormatKind::Prose,
        };
    }

    let lines: Vec<&str> = trimmed.lines().collect();
    if looks_like_code(&lines) {
        return SmartFormatResult {
            text: format_code(&lines),
            kind: SmartFormatKind::Code,
        };
    }

    if looks_like_list(&lines) {
        return SmartFormatResult {
            text: format_list(&lines),
            kind: SmartFormatKind::List,
        };
    }

    let is_cjk = has_significant_cjk(trimmed);
    let formatted = format_prose(&lines);
    SmartFormatResult {
        text: formatted,
        kind: if is_cjk {
            SmartFormatKind::Cjk
        } else {
            SmartFormatKind::Prose
        },
    }
}

/// Collapses all line breaks and extra whitespace into a single clean line (ideal for URLs,
/// shell commands, or single-line search queries), while fixing CJK spacing and hyphen wraps.
pub fn join_single_line(raw: &str) -> String {
    let normalized = raw.replace("\r\n", "\n").replace('\r', "\n");
    let mut out = String::new();
    for line in normalized.lines() {
        let cleaned = clean_inline_spacing(line.trim());
        if cleaned.is_empty() {
            continue;
        }
        append_joined_line(&mut out, &cleaned);
    }
    clean_punctuation_spacing(&out)
}

fn is_cjk_char(ch: char) -> bool {
    matches!(
        ch as u32,
        0x3040..=0x309F // Hiragana
            | 0x30A0..=0x30FF // Katakana
            | 0x3400..=0x4DBF // CJK Extension A
            | 0x4E00..=0x9FFF // CJK Unified Ideographs
            | 0xAC00..=0xD7AF // Hangul Syllables
            | 0xF900..=0xFAFF // CJK Compatibility Ideographs
    )
}

fn is_cjk_punct(ch: char) -> bool {
    matches!(
        ch,
        '，' | '。' | '！' | '？' | '；' | '：' | '、' | '（' | '）' | '「' | '」' | '『' | '』'
            | '【' | '】' | '《' | '》'
    )
}

fn has_significant_cjk(text: &str) -> bool {
    let mut cjk = 0usize;
    let mut non_ws = 0usize;
    for ch in text.chars() {
        if !ch.is_whitespace() {
            non_ws += 1;
            if is_cjk_char(ch) {
                cjk += 1;
            }
        }
    }
    non_ws > 0 && (cjk * 4 >= non_ws)
}

/// Strips spurious spaces that `Windows.Media.Ocr` inserts between adjacent CJK characters
/// and collapses runs of multiple ASCII spaces into a single space.
fn clean_inline_spacing(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::with_capacity(line.len());
    let mut i = 0;

    while i < chars.len() {
        let ch = chars[i];
        if ch == ' ' || ch == '\t' {
            // Skip entire whitespace run
            let mut j = i + 1;
            while j < chars.len() && (chars[j] == ' ' || chars[j] == '\t') {
                j += 1;
            }
            let prev = out.chars().last();
            let next = chars.get(j).copied();
            let drop_space = match (prev, next) {
                (Some(p), Some(n))
                    if (is_cjk_char(p) && is_cjk_char(n))
                        || (is_cjk_char(p) && is_cjk_punct(n))
                        || (is_cjk_punct(p) && is_cjk_char(n))
                        || (is_cjk_punct(p) && is_cjk_punct(n)) =>
                {
                    true
                }
                (None, _) | (_, None) => true,
                _ => false,
            };
            if !drop_space {
                out.push(' ');
            }
            i = j;
        } else {
            out.push(ch);
            i += 1;
        }
    }

    out
}

fn clean_punctuation_spacing(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    for (idx, &ch) in chars.iter().enumerate() {
        if ch == ' '
            && let Some(&next) = chars.get(idx + 1)
            && matches!(next, ',' | '.' | '!' | '?' | ';' | ':' | ')' | ']')
        {
            // Keep space before `.` if it's a relative path/ellipsis like ` .env` or ` ...`
            let is_dot_token = next == '.'
                && chars
                    .get(idx + 2)
                    .is_some_and(|c| c.is_alphanumeric() || *c == '.');
            if !is_dot_token {
                continue;
            }
        }
        out.push(ch);
    }
    out
}

fn append_joined_line(dst: &mut String, next: &str) {
    if dst.is_empty() {
        dst.push_str(next);
        return;
    }

    // De-hyphenate wrapped English/Latin word breaks (`imple-` + `mentation` -> `implementation`)
    if dst.ends_with('-')
        && !dst.ends_with("--")
        && let Some(before_hyphen) = dst.chars().rev().nth(1)
        && before_hyphen.is_ascii_alphabetic()
        && next.chars().next().is_some_and(|c| c.is_ascii_lowercase())
    {
        dst.pop();
        dst.push_str(next);
        return;
    }

    let prev_last = dst.chars().last();
    let next_first = next.chars().next();
    let join_without_space = matches!(
        (prev_last, next_first),
        (Some(p), Some(n))
            if (is_cjk_char(p) || is_cjk_punct(p)) && (is_cjk_char(n) || is_cjk_punct(n))
    );

    if !join_without_space {
        dst.push(' ');
    }
    dst.push_str(next);
}

fn looks_like_code(lines: &[&str]) -> bool {
    let non_empty: Vec<&str> = lines
        .iter()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();
    if non_empty.is_empty() {
        return false;
    }

    let mut score = 0i32;
    for line in &non_empty {
        if line.ends_with('{') || line.ends_with('}') || line.ends_with(';') {
            score += 2;
        }
        if line.contains("=>")
            || line.contains("::")
            || line.contains("->")
            || line.contains("==")
            || line.contains("!=")
            || line.contains("&&")
            || line.contains("||")
        {
            score += 2;
        }
        if line.starts_with("fn ")
            || line.starts_with("pub ")
            || line.starts_with("let ")
            || line.starts_with("const ")
            || line.starts_with("use ")
            || line.starts_with("import ")
            || line.starts_with("export ")
            || line.starts_with("def ")
            || line.starts_with("class ")
            || line.starts_with("return ")
            || line.starts_with("if ")
            || line.starts_with("for ")
            || line.starts_with("while ")
            || line.starts_with("match ")
            || line.starts_with("//")
            || line.starts_with("#[")
            || line.starts_with("SELECT ")
            || line.starts_with("cargo ")
            || line.starts_with("npm ")
            || line.starts_with("git ")
        {
            score += 2;
        }
        // JSON / YAML key-value line
        if line.starts_with('"') && line.contains("\":") {
            score += 2;
        }
    }

    score >= 3 || (non_empty.len() == 1 && score >= 2)
}

fn format_code(lines: &[&str]) -> String {
    let mut out_lines: Vec<String> = Vec::with_capacity(lines.len());
    let mut prev_blank = false;

    for raw_line in lines {
        let trimmed_end = raw_line.trim_end();
        if trimmed_end.trim().is_empty() {
            if !prev_blank && !out_lines.is_empty() {
                out_lines.push(String::new());
                prev_blank = true;
            }
            continue;
        }
        prev_blank = false;

        // Preserve leading indentation, normalize smart quotes OCR often introduces in code
        let indent_len = trimmed_end.len() - trimmed_end.trim_start().len();
        let indent = &trimmed_end[..indent_len];
        let body = trimmed_end[indent_len..]
            .replace(['“', '”'], "\"")
            .replace(['‘', '’'], "'");
        let cleaned_body = clean_inline_spacing(&body);
        out_lines.push(format!("{indent}{cleaned_body}"));
    }

    out_lines.join("\n")
}

fn parse_list_prefix(trimmed: &str) -> Option<(String, &str)> {
    // Bullet symbols commonly emitted by OCR
    for bullet in ["• ", "· ", "◦ ", "▪ ", "▸ ", "- ", "* ", "•", "▪", "◦"] {
        if let Some(rest) = trimmed.strip_prefix(bullet) {
            let rest = rest.trim_start();
            if !rest.is_empty() {
                return Some(("- ".to_string(), rest));
            }
        }
    }
    // Numbered list (`1. Foo` or `1) Foo`)
    let bytes = trimmed.as_bytes();
    let mut digits = 0;
    while digits < bytes.len() && digits < 3 && bytes[digits].is_ascii_digit() {
        digits += 1;
    }
    if digits > 0
        && digits + 1 < bytes.len()
        && (bytes[digits] == b'.' || bytes[digits] == b')')
        && bytes[digits + 1] == b' '
    {
        let marker = format!("{}. ", &trimmed[..digits]);
        let rest = trimmed[digits + 2..].trim_start();
        if !rest.is_empty() {
            return Some((marker, rest));
        }
    }
    None
}

fn looks_like_list(lines: &[&str]) -> bool {
    let bullet_count = lines
        .iter()
        .filter(|l| parse_list_prefix(l.trim()).is_some())
        .count();
    bullet_count >= 2
}

fn format_list(lines: &[&str]) -> String {
    let mut items: Vec<String> = Vec::new();

    for raw in lines {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some((prefix, rest)) = parse_list_prefix(trimmed) {
            let cleaned = clean_punctuation_spacing(&clean_inline_spacing(rest));
            items.push(format!("{prefix}{cleaned}"));
        } else {
            let cleaned = clean_punctuation_spacing(&clean_inline_spacing(trimmed));
            if let Some(last) = items.last_mut() {
                append_joined_line(last, &cleaned);
            } else {
                items.push(cleaned);
            }
        }
    }

    items.join("\n")
}

fn ends_hard_sentence(line: &str) -> bool {
    line.ends_with('.')
        || line.ends_with('!')
        || line.ends_with('?')
        || line.ends_with(':')
        || line.ends_with('。')
        || line.ends_with('！')
        || line.ends_with('？')
        || line.ends_with('：')
}

fn format_prose(lines: &[&str]) -> String {
    let mut paragraphs: Vec<String> = Vec::new();
    let mut current = String::new();

    for raw in lines {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            if !current.is_empty() {
                paragraphs.push(clean_punctuation_spacing(&current));
                current.clear();
            }
            continue;
        }

        let cleaned = clean_inline_spacing(trimmed);
        if cleaned.is_empty() {
            continue;
        }

        // Preserve explicit bullet/numbered item inside mixed prose
        if let Some((prefix, rest)) = parse_list_prefix(&cleaned) {
            if !current.is_empty() {
                paragraphs.push(clean_punctuation_spacing(&current));
                current.clear();
            }
            current.push_str(&prefix);
            current.push_str(rest);
            continue;
        }

        // Keep short standalone title/heading lines separate from body paragraph
        let prev_looks_like_heading = !current.is_empty()
            && current.chars().count() <= 42
            && !ends_hard_sentence(&current)
            && !current.contains(',')
            && !current.contains('，')
            && cleaned.chars().count() > 45;

        if prev_looks_like_heading {
            paragraphs.push(clean_punctuation_spacing(&current));
            current.clear();
            current.push_str(&cleaned);
            continue;
        }

        append_joined_line(&mut current, &cleaned);
    }

    if !current.is_empty() {
        paragraphs.push(clean_punctuation_spacing(&current));
    }

    paragraphs.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smart_format_unwraps_prose_and_dehyphenates() {
        let raw = "Reticle captures your entire desk-\ntop with zero white flash , and\nlets you edit OCR text cleanly .\n\nSecond paragraph starts here.";
        let res = smart_format(raw);
        assert_eq!(res.kind, SmartFormatKind::Prose);
        assert_eq!(
            res.text,
            "Reticle captures your entire desktop with zero white flash, and lets you edit OCR text cleanly.\n\nSecond paragraph starts here."
        );
    }

    #[test]
    fn smart_format_removes_windows_ocr_cjk_spaces() {
        let raw = "你 好 世 界 ， 今 天 天 气\n真 不 错 ！";
        let res = smart_format(raw);
        assert_eq!(res.kind, SmartFormatKind::Cjk);
        assert_eq!(res.text, "你好世界，今天天气真不错！");
    }

    #[test]
    fn smart_format_preserves_code_lines_and_normalizes_smart_quotes() {
        let raw = "fn greet() {\n    let msg = “hello”;\n    println!(“{}”, msg);\n}";
        let res = smart_format(raw);
        assert_eq!(res.kind, SmartFormatKind::Code);
        assert_eq!(
            res.text,
            "fn greet() {\n    let msg = \"hello\";\n    println!(\"{}\", msg);\n}"
        );
    }

    #[test]
    fn smart_format_normalizes_bullets_and_joins_wrapped_list_items() {
        let raw = "• First item that wraps\nonto the second line\n• Second item\n• Third item";
        let res = smart_format(raw);
        assert_eq!(res.kind, SmartFormatKind::List);
        assert_eq!(
            res.text,
            "- First item that wraps onto the second line\n- Second item\n- Third item"
        );
    }

    #[test]
    fn join_single_line_collapses_wrapped_command_or_cjk() {
        let raw = "cargo clippy --all-targets\n  -- -D warnings";
        assert_eq!(
            join_single_line(raw),
            "cargo clippy --all-targets -- -D warnings"
        );
        assert_eq!(join_single_line("你 好\n世 界"), "你好世界");
    }
}
