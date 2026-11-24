use crate::ocr::{extract_text_from_png, is_cjk_or_kana_hangul, tests::has_cjk_ocr_pack};
use crate::ocr_format::{smart_format, SmartFormatKind};

fn assert_no_cjk_hallucinations(label: &str, text: &str) {
    let stray_cjk: String = text.chars().filter(|&c| is_cjk_or_kana_hangul(c)).collect();
    assert!(
        stray_cjk.is_empty(),
        "[{label}] expected zero stray CJK hallucinations on English/code screenshot, found {stray_cjk:?} in:\n{text}"
    );
}

fn assert_token_recall(label: &str, text: &str, required: &[&str], min_ratio: f32) {
    let lowered = text.to_ascii_lowercase();
    let mut hits = 0usize;
    let mut missing = Vec::new();
    for &tok in required {
        if lowered.contains(&tok.to_ascii_lowercase()) {
            hits += 1;
        } else {
            missing.push(tok);
        }
    }
    let ratio = hits as f32 / required.len().max(1) as f32;
    assert!(
        ratio >= min_ratio,
        "[{label}] recall {hits}/{} ({:.0}%) below {:.0}% threshold; missing={missing:?}\n--- OCR OUTPUT ---\n{text}",
        required.len(),
        ratio * 100.0,
        min_ratio * 100.0
    );
}

#[test]
fn hard_01_glassmorphic_purple_rust_code_zero_cjk_hallucination_and_high_recall() {
    let png = include_bytes!("../tests/fixtures/hard_glassmorphic_rust_code.png");
    let text = extract_text_from_png(png).expect("glassmorphic rust code OCR");
    assert_no_cjk_hallucinations("glassmorphic_rust_code", &text);
    assert_token_recall(
        "glassmorphic_rust_code",
        &text,
        &[
            "Semantic pill colors",
            "pub fn format_badge_palette(kind: Option<SmartFormatKind>) -> (Color32, Color32)",
            "match kind",
            "SmartFormatKind::Code",
            "SmartFormatKind::Json",
            "Color32::from_rgb(16, 26, 38)",
            "Color32::from_rgb(36, 62, 92)",
            "SYN",
            "KEY",
            "ACCENT",
            "PICK",
            "SmartFormatKind::Prose",
            "Color32::from_rgb(15, 29, 23)",
            "STRING",
            "EMERALD",
            "None =>",
            "PILL",
            "MUTED",
        ],
        0.90,
    );

    let formatted = smart_format(&text);
    assert_eq!(
        formatted.kind,
        SmartFormatKind::Code,
        "formatted glassmorphic rust OCR must be detected as Code, got {:?}:\n{}",
        formatted.kind,
        formatted.text
    );
}

#[test]
fn hard_02_tiny_low_contrast_terminal_logs_extract_paths_levels_and_numbers() {
    let png = include_bytes!("../tests/fixtures/hard_tiny_low_contrast_terminal.png");
    let text = extract_text_from_png(png).expect("terminal logs OCR");
    assert_no_cjk_hallucinations("tiny_terminal_logs", &text);
    assert_token_recall(
        "tiny_terminal_logs",
        &text,
        &[
            "INFO",
            "WARN",
            "ERROR",
            "capture::screen",
            "x=-1080",
            "w=3960",
            "h=1920",
            "118ms",
            "score=482",
            "HTTP 429",
            "rate limit",
            "byok_ai.rs",
            "142",
            "10000ms",
            "Windows.Media.Ocr",
            "clipboard",
            "exit=0",
        ],
        0.90,
    );
    assert!(
        text.lines()
            .any(|l| l.contains("ERROR") && l.to_ascii_lowercase().contains("byok_ai.rs")),
        "expected ERROR and byok_ai.rs reconstructed on the same line, got:\n{text}"
    );
}

#[test]
fn hard_03_dense_dark_json_payload_extracts_keys_values_and_urls() {
    let png = include_bytes!("../tests/fixtures/hard_dense_json_payload.png");
    let text = extract_text_from_png(png).expect("dense json OCR");
    assert_no_cjk_hallucinations("dense_json_payload", &text);
    assert_token_recall(
        "dense_json_payload",
        &text,
        &[
            "service",
            "reticle-ocr",
            "version",
            "0.1.0",
            "speculative_prefetch",
            "true",
            "max_vision_edge",
            "1440",
            "jpeg_quality",
            "86",
            "openrouter.ai",
            "streaming",
        ],
        0.90,
    );
}

#[test]
fn hard_04_mixed_bilingual_latin_and_cjk_dark_ui_preserves_both_scripts() {
    let png = include_bytes!("../tests/fixtures/hard_mixed_latin_cjk_ui.png");
    let text = extract_text_from_png(png).expect("mixed latin+cjk OCR");
    assert_token_recall(
        "mixed_latin_cjk_ui",
        &text,
        &["Latency", "142 ms", "Engine", "Windows", "OCR", "OpenRouter", "Alt+O"],
        0.85,
    );

    if has_cjk_ocr_pack() {
        let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
        let cjk_hits = [
            "离线识别与实时翻译",
            "支持暗色模式截图",
            "代码解释",
            "快捷键",
            "立即框选屏幕区域",
            "无需等待",
            "运行正常",
        ]
        .iter()
        .filter(|&&tok| compact.contains(tok))
        .count();
        assert!(
            cjk_hits >= 6,
            "[mixed_latin_cjk_ui] expected >=6/7 full CJK phrases when CJK pack installed, got {cjk_hits}:\n{text}"
        );
    }
}

#[test]
fn hard_05_light_zebra_striped_metrics_table_extracts_all_rows_and_numbers() {
    let png = include_bytes!("../tests/fixtures/hard_light_striped_table.png");
    let text = extract_text_from_png(png).expect("light striped table OCR");
    assert_no_cjk_hallucinations("light_striped_table", &text);
    assert_token_recall(
        "light_striped_table",
        &text,
        &[
            "Service",
            "Region",
            "Uptime",
            "Status",
            "Auth Gateway",
            "us-east-1",
            "99.98%",
            "1,284.50",
            "ACTIVE",
            "Search Index",
            "eu-west-2",
            "99.94%",
            "842.00",
            "Vision Worker",
            "ap-south-1",
            "99.91%",
            "2,150.75",
            "HEALTHY",
            "Batch Export",
            "us-west-2",
            "98.70%",
            "319.20",
            "DEGRADED",
        ],
        0.90,
    );
}

#[test]
fn hard_06_ultra_low_contrast_muted_gray_credits_on_dark_background() {
    let png = include_bytes!("../tests/fixtures/hard_muted_gray_on_dark_credits.png");
    let text = extract_text_from_png(png).expect("muted gray credits OCR");
    assert_no_cjk_hallucinations("muted_gray_credits", &text);
    assert_token_recall(
        "muted_gray_credits",
        &text,
        &[
            "Credits",
            "Release Notes",
            "Midnight Signals Orchestra",
            "Elena Vance",
            "Marcus Holloway",
            "Obsidian Audio Labs",
            "2026",
            "RET-2026-0941",
        ],
        0.87,
    );
}

#[test]
fn hard_07_edge_hugging_tight_crop_never_clips_border_glyphs() {
    let png = include_bytes!("../tests/fixtures/hard_edge_hugging_tight_crop.png");
    let text = extract_text_from_png(png).expect("edge hugging tight crop OCR");
    assert_no_cjk_hallucinations("edge_hugging_tight_crop", &text);
    assert_token_recall(
        "edge_hugging_tight_crop",
        &text,
        &[
            "Edge-to-edge",
            "tight crop",
            "padding",
            "Verify",
            "never clip",
            "Z9-ALPHA",
            "Bottom line",
            "lower frame edge",
        ],
        0.90,
    );
}

#[test]
fn hard_08_bbox_line_reconstructor_merges_columns_and_preserves_sentence_spacing() {
    use crate::ocr::{fix_slashed_zero_artifacts, reconstruct_lines_from_words, OcrWordBox};

    assert_eq!(fix_slashed_zero_artifacts("w=396Ø"), "w=3960");
    assert_eq!(fix_slashed_zero_artifacts("exit=Ø"), "exit=0");
    assert_eq!(fix_slashed_zero_artifacts("Øslo"), "Øslo");

    let words = vec![
        OcrWordBox { text: "Done.".into(), x: 10.0, y: 40.0, width: 40.0, height: 16.0 },
        OcrWordBox { text: "Next".into(), x: 56.0, y: 40.0, width: 34.0, height: 16.0 },
        OcrWordBox { text: "byok_ai.".into(), x: 140.0, y: 10.0, width: 58.0, height: 16.0 },
        OcrWordBox { text: "rs".into(), x: 200.0, y: 10.0, width: 14.0, height: 16.0 },
        OcrWordBox { text: "ERROR".into(), x: 10.0, y: 11.0, width: 44.0, height: 16.0 },
        OcrWordBox { text: "std::".into(), x: 70.0, y: 10.0, width: 36.0, height: 16.0 },
        OcrWordBox { text: "io".into(), x: 108.0, y: 10.0, width: 16.0, height: 16.0 },
    ];
    let reconstructed = reconstruct_lines_from_words(words);
    assert_eq!(reconstructed, "ERROR std::io byok_ai.rs\nDone. Next");
}
