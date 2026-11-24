/// Offline, zero-latency smart formatting utilities for raw `Windows.Media.Ocr` output.
/// Handles CJK inter-character space artifacts, wrapped prose paragraphs, hyphenated line breaks,
/// noisy list bullets, and code-preserving cleanup.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmartFormatKind {
    Prose,
    Code,
    Json,
    Table,
    List,
    Cjk,
}

impl SmartFormatKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Prose => "Prose",
            Self::Code => "Code",
            Self::Json => "JSON",
            Self::Table => "Key-Value",
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

/// Automatically detects whether OCR text is JSON, code, key-value pairs, a bullet/numbered list,
/// CJK text, or prose, and applies tailored cleanup without destroying intentional structure.
pub fn smart_format(raw: &str) -> SmartFormatResult {
    let normalized = raw.replace("\r\n", "\n").replace('\r', "\n");
    let trimmed = normalized.trim();
    if trimmed.is_empty() {
        return SmartFormatResult {
            text: String::new(),
            kind: SmartFormatKind::Prose,
        };
    }

    if let Some(pretty_json) = try_format_json(trimmed) {
        return SmartFormatResult {
            text: pretty_json,
            kind: SmartFormatKind::Json,
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

    if looks_like_key_value_table(&lines) {
        return SmartFormatResult {
            text: format_key_value_table(&lines),
            kind: SmartFormatKind::Table,
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

/// Attempts to parse OCR text as JSON (fixing curly OCR quotes and trailing commas)
/// and pretty-print it with crisp 2-space indentation.
fn try_format_json(trimmed: &str) -> Option<String> {
    if !((trimmed.starts_with('{') && trimmed.ends_with('}'))
        || (trimmed.starts_with('[') && trimmed.ends_with(']')))
    {
        return None;
    }
    let cleaned = trimmed
        .replace(['“', '”'], "\"")
        .replace(['‘', '’'], "'");
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&cleaned) {
        return serde_json::to_string_pretty(&val).ok();
    }
    // Try stripping common trailing commas before `}` or `]`
    let no_trailing = strip_trailing_json_commas(&cleaned);
    let val = serde_json::from_str::<serde_json::Value>(&no_trailing).ok()?;
    serde_json::to_string_pretty(&val).ok()
}

fn strip_trailing_json_commas(input: &str) -> String {
    let chars: Vec<char> = input.chars().collect();
    let mut out = String::with_capacity(input.len());
    let mut in_str = false;
    let mut escaped = false;
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        if in_str {
            out.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        if ch == '"' {
            in_str = true;
            out.push(ch);
            i += 1;
            continue;
        }
        if ch == ',' {
            let mut j = i + 1;
            while j < chars.len() && chars[j].is_whitespace() {
                j += 1;
            }
            if matches!(chars.get(j), Some('}' | ']')) {
                i += 1;
                continue;
            }
        }
        out.push(ch);
        i += 1;
    }
    out
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
    // Check if OCR flattened all leading indentation (every non-empty line starts at column 0)
    // while containing brace blocks — if so, auto-reindent cleanly by brace depth.
    let non_empty: Vec<&str> = lines
        .iter()
        .copied()
        .filter(|l| !l.trim().is_empty())
        .collect();
    let has_original_indent = non_empty
        .iter()
        .any(|l| l.starts_with(' ') || l.starts_with('\t'));
    let has_braces = non_empty.iter().any(|l| l.contains('{') && !l.contains('}'));
    let auto_indent_braces = !has_original_indent && has_braces;

    let mut out_lines: Vec<String> = Vec::with_capacity(lines.len());
    let mut prev_blank = false;
    let mut brace_depth: usize = 0;

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

        let indent_len = trimmed_end.len() - trimmed_end.trim_start().len();
        let orig_indent = &trimmed_end[..indent_len];
        let body = trimmed_end[indent_len..]
            .replace(['“', '”'], "\"")
            .replace(['‘', '’'], "'");
        let cleaned_body = clean_code_line_spacing(&clean_inline_spacing(&body));

        if auto_indent_braces {
            let leading_closes = cleaned_body
                .chars()
                .take_while(|&c| c == '}' || c == ')' || c == ']')
                .count();
            brace_depth = brace_depth.saturating_sub(leading_closes);
            let indent = "    ".repeat(brace_depth);
            out_lines.push(format!("{indent}{cleaned_body}"));
            let opens = count_unquoted_char(&cleaned_body, '{');
            let closes = count_unquoted_char(&cleaned_body, '}').saturating_sub(leading_closes);
            brace_depth = brace_depth.saturating_add(opens).saturating_sub(closes);
        } else {
            out_lines.push(format!("{orig_indent}{cleaned_body}"));
        }
    }

    out_lines.join("\n")
}

/// Cleans common OCR spacing artifacts in code outside string literals (`std :: vec` -> `std::vec`,
/// `foo ( x )` -> `foo(x)`, `a ;` -> `a;`).
fn clean_code_line_spacing(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::with_capacity(line.len());
    let mut in_quote: Option<char> = None;
    let mut escaped = false;
    let mut i = 0;

    while i < chars.len() {
        let ch = chars[i];
        if let Some(q) = in_quote {
            out.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == q {
                in_quote = None;
            }
            i += 1;
            continue;
        }
        if ch == '"' || ch == '\'' || ch == '`' {
            in_quote = Some(ch);
            out.push(ch);
            i += 1;
            continue;
        }
        // Collapse ` :: ` -> `::`
        if ch == ' '
            && chars.get(i + 1) == Some(&':')
            && chars.get(i + 2) == Some(&':')
        {
            out.push_str("::");
            i += 3;
            if chars.get(i) == Some(&' ') {
                i += 1;
            }
            continue;
        }
        // Drop space before `;`, `,`, `)`, `]`
        if ch == ' ' && matches!(chars.get(i + 1), Some(';' | ',' | ')' | ']')) {
            i += 1;
            continue;
        }
        // Drop space right after `(` or `[`
        if matches!(ch, '(' | '[') && chars.get(i + 1) == Some(&' ') {
            out.push(ch);
            i += 2;
            continue;
        }
        out.push(ch);
        i += 1;
    }
    out
}

fn count_unquoted_char(line: &str, needle: char) -> usize {
    let mut count = 0;
    let mut in_quote: Option<char> = None;
    let mut escaped = false;
    for ch in line.chars() {
        if let Some(q) = in_quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == q {
                in_quote = None;
            }
            continue;
        }
        if ch == '"' || ch == '\'' {
            in_quote = Some(ch);
            continue;
        }
        if ch == needle {
            count += 1;
        }
    }
    count
}

fn split_key_value(line: &str) -> Option<(&str, &str)> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return None;
    }
    let idx = trimmed.find(": ").or_else(|| {
        trimmed
            .find(':')
            .filter(|&p| p > 0 && p + 1 < trimmed.len() && !trimmed[p..].starts_with("://"))
    })?;
    let key = trimmed[..idx].trim();
    let val = trimmed[idx + 1..].trim();
    if key.is_empty()
        || val.is_empty()
        || key.chars().count() > 28
        || key.contains('.')
        || key.split_whitespace().count() > 4
    {
        return None;
    }
    Some((key, val))
}

fn looks_like_key_value_table(lines: &[&str]) -> bool {
    let non_empty: Vec<&str> = lines
        .iter()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();
    if non_empty.len() < 2 {
        return false;
    }
    let kv_count = non_empty
        .iter()
        .filter(|l| split_key_value(l).is_some())
        .count();
    kv_count >= 2 && kv_count * 2 >= non_empty.len()
}

fn format_key_value_table(lines: &[&str]) -> String {
    let mut out = Vec::new();
    for raw in lines {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some((k, v)) = split_key_value(trimmed) {
            let ck = clean_inline_spacing(k);
            let cv = clean_punctuation_spacing(&clean_inline_spacing(v));
            out.push(format!("{ck}: {cv}"));
        } else {
            out.push(clean_punctuation_spacing(&clean_inline_spacing(trimmed)));
        }
    }
    out.join("\n")
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

    #[test]
    fn smart_format_pretty_prints_json_with_curly_quotes_and_trailing_commas() {
        let raw = r#"{“mode”: “byok”, “port”: 8080, “ok”: true,}"#;
        let res = smart_format(raw);
        assert_eq!(res.kind, SmartFormatKind::Json);
        assert!(res.text.contains("  \"mode\": \"byok\","));
        assert!(res.text.contains("  \"port\": 8080"));
        assert!(res.text.contains("  \"ok\": true"));
    }

    #[test]
    fn smart_format_auto_indents_flattened_code_and_fixes_operator_spacing() {
        let raw = "fn main() {\nlet x = std :: env :: args( );\nprintln!( “hi” , x ) ;\n}";
        let res = smart_format(raw);
        assert_eq!(res.kind, SmartFormatKind::Code);
        assert_eq!(
            res.text,
            "fn main() {\n    let x = std::env::args();\n    println!(\"hi\", x);\n}"
        );
    }

    #[test]
    fn smart_format_detects_key_value_metadata_blocks() {
        let raw = "Host : api.openrouter.ai\nStatus : 200 OK\nLatency : 142 ms";
        let res = smart_format(raw);
        assert_eq!(res.kind, SmartFormatKind::Table);
        assert_eq!(
            res.text,
            "Host: api.openrouter.ai\nStatus: 200 OK\nLatency: 142 ms"
        );
    }
}
