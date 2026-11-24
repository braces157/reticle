#[cfg(test)]
#[path = "ocr_hard_tests.rs"]
mod hard_tests;

use image::imageops::FilterType;
use image::{ImageFormat, Rgba, RgbaImage};
use thiserror::Error;
use windows::Globalization::Language;
use windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine;
use windows::Storage::Streams::DataWriter;
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};

const MAX_OCR_DIM: u32 = 3600;
const OCR_BORDER_PAD: u32 = 40;

#[derive(Debug, Error)]
pub enum OcrError {
    #[error("failed to decode cropped PNG for OCR: {0}")]
    ImageDecode(String),
    #[error("no Windows OCR language packs are installed on this machine")]
    NoRecognizerAvailable,
    #[error("Windows.Media.Ocr failed: {0}")]
    WinRt(String),
}

struct ComGuard {
    should_uninit: bool,
}

impl ComGuard {
    fn init_mta() -> Self {
        // SAFETY: Standard WinRT/COM thread initialization; paired with CoUninitialize only if S_OK/S_FALSE.
        let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        Self {
            should_uninit: hr.is_ok(),
        }
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        if self.should_uninit {
            // SAFETY: Matches a successful CoInitializeEx on the same thread.
            unsafe {
                CoUninitialize();
            }
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct OcrWordBox {
    pub(super) text: String,
    pub(super) x: f32,
    pub(super) y: f32,
    pub(super) width: f32,
    pub(super) height: f32,
}

/// Reconstructs true visual lines from `OcrWord` bounding rectangles so multi-column UIs,
/// terminal logs (`timestamp | LEVEL | message`), tables, and indented code stay on their
/// exact horizontal rows instead of being split vertically into columns by `OcrResult::Lines()`.
pub(super) fn reconstruct_lines_from_words(mut words: Vec<OcrWordBox>) -> String {
    if words.is_empty() {
        return String::new();
    }
    words.sort_by(|a, b| {
        let ay = a.y + a.height * 0.5;
        let by = b.y + b.height * 0.5;
        ay.total_cmp(&by).then_with(|| a.x.total_cmp(&b.x))
    });

    struct Row {
        cy_sum: f32,
        h_sum: f32,
        words: Vec<OcrWordBox>,
    }

    let mut rows: Vec<Row> = Vec::new();
    for w in words {
        let w_cy = w.y + w.height * 0.5;
        let mut best_row: Option<usize> = None;
        let mut best_dist = f32::MAX;
        for (idx, row) in rows.iter().enumerate() {
            let count = row.words.len() as f32;
            let row_cy = row.cy_sum / count;
            let row_h = (row.h_sum / count).max(10.0);
            let tol = (row_h * 0.52).max(6.0);
            let dist = (w_cy - row_cy).abs();
            if dist <= tol && dist < best_dist {
                best_dist = dist;
                best_row = Some(idx);
            }
        }
        if let Some(idx) = best_row {
            let row = &mut rows[idx];
            row.cy_sum += w_cy;
            row.h_sum += w.height;
            row.words.push(w);
        } else {
            rows.push(Row {
                cy_sum: w_cy,
                h_sum: w.height,
                words: vec![w],
            });
        }
    }

    rows.sort_by(|a, b| {
        let ay = a.cy_sum / a.words.len() as f32;
        let by = b.cy_sum / b.words.len() as f32;
        ay.total_cmp(&by)
    });

    let mut lines_out = Vec::with_capacity(rows.len());
    for mut row in rows {
        row.words.sort_by(|a, b| a.x.total_cmp(&b.x));
        let mut line = String::new();
        let mut prev_right: Option<f32> = None;
        let mut prev_h: f32 = 12.0;

        for word in row.words {
            let tok = fix_slashed_zero_artifacts(word.text.trim());
            if tok.is_empty() {
                continue;
            }
            if !line.is_empty() {
                let prev_ch = line.chars().last();
                let next_ch = tok.chars().next();
                let prev_cjk = prev_ch.is_some_and(is_cjk_or_kana_hangul);
                let next_cjk = next_ch.is_some_and(is_cjk_or_kana_hangul);
                let gap = prev_right.map(|r| word.x - r).unwrap_or(f32::MAX);
                let ref_h = prev_h.max(word.height).max(10.0);

                // Omit artificial spaces when Windows OCR splits a single token across tight
                // punctuation (`.`, `:`, `_`, `/`, `(`), Rust `::` paths, dotted identifiers, or
                // split uppercase letters (`OC` + `R`).
                let tight_punct = gap <= ref_h * 0.24
                    && (prev_ch.is_some_and(|c| matches!(c, '.' | ':' | '_' | '/' | '(' | '['))
                        || next_ch.is_some_and(|c| {
                            matches!(c, '.' | ':' | '_' | ')' | ']' | ',' | ';')
                        }));
                let tight_split_caps = gap <= ref_h * 0.14
                    && prev_ch.is_some_and(|c| c.is_ascii_uppercase())
                    && next_ch.is_some_and(|c| c.is_ascii_uppercase());
                let double_colon = prev_ch == Some(':') && next_ch == Some(':');
                let after_double_colon = line.ends_with("::")
                    && gap <= ref_h * 0.55
                    && next_ch.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_');
                let dotted_ident = gap <= ref_h * 0.50
                    && ((prev_ch == Some('.') && next_ch.is_some_and(|c| c.is_ascii_lowercase()))
                        || (next_ch == Some('.')
                            && prev_ch.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')));

                let omit_space = (prev_cjk && next_cjk)
                    || tight_punct
                    || tight_split_caps
                    || double_colon
                    || after_double_colon
                    || dotted_ident;
                if !omit_space {
                    line.push(' ');
                }
            }
            line.push_str(&tok);
            prev_right = Some(word.x + word.width);
            prev_h = word.height;
        }
        if !line.is_empty() {
            lines_out.push(line);
        }
    }
    lines_out.join("\n")
}

/// Repairs Consolas/monospace slashed-zero `0` misrecognized by Windows OCR as `Ø` (`U+00D8`)
/// when adjacent to ASCII digits or `=`/`-` (e.g., `396Ø` -> `3960`, `exit=Ø` -> `exit=0`),
/// while preserving real Nordic words like `Øslo`.
pub(super) fn fix_slashed_zero_artifacts(tok: &str) -> String {
    if !tok.contains('Ø') && !tok.contains('ø') {
        return tok.to_string();
    }
    let chars: Vec<char> = tok.chars().collect();
    let mut out = String::with_capacity(tok.len());
    for (i, &ch) in chars.iter().enumerate() {
        if matches!(ch, 'Ø' | 'ø') {
            let prev = i.checked_sub(1).and_then(|p| chars.get(p)).copied();
            let next = chars.get(i + 1).copied();
            let numeric_ctx = prev
                .is_some_and(|c| c.is_ascii_digit() || matches!(c, '=' | '-' | '(' | ':' | ','))
                || next.is_some_and(|c| c.is_ascii_digit() || matches!(c, ')' | ',' | ';' | '%'));
            if numeric_ctx {
                out.push('0');
                continue;
            }
        }
        out.push(ch);
    }
    out
}

fn recognize_with_engine(engine: &OcrEngine, bitmap: &SoftwareBitmap) -> Result<String, OcrError> {
    let async_op = engine
        .RecognizeAsync(bitmap)
        .map_err(|e| OcrError::WinRt(format!("OcrEngine::RecognizeAsync: {e}")))?;
    let result = async_op
        .join()
        .map_err(|e| OcrError::WinRt(format!("OcrResult::join: {e}")))?;

    let lines = result
        .Lines()
        .map_err(|e| OcrError::WinRt(format!("OcrResult::Lines: {e}")))?;

    let mut word_boxes = Vec::new();
    let mut fallback_lines = Vec::new();
    for line in lines {
        if let Ok(words) = line.Words() {
            for word in words {
                if let (Ok(text), Ok(rect)) = (word.Text(), word.BoundingRect()) {
                    let s = text.to_string_lossy().trim().to_string();
                    if !s.is_empty() && rect.Height > 0.0 {
                        word_boxes.push(OcrWordBox {
                            text: s,
                            x: rect.X,
                            y: rect.Y,
                            width: rect.Width.max(0.0),
                            height: rect.Height,
                        });
                    }
                }
            }
        }
        if let Ok(text) = line.Text() {
            let s = text.to_string_lossy().trim().to_string();
            if !s.is_empty() {
                fallback_lines.push(s);
            }
        }
    }

    if !word_boxes.is_empty() {
        Ok(reconstruct_lines_from_words(word_boxes))
    } else {
        Ok(fallback_lines.join("\n"))
    }
}

fn is_cjk_or_kana_hangul(ch: char) -> bool {
    matches!(
        ch as u32,
        0x3040..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF
    )
}

fn is_fullwidth_cjk_punct(ch: char) -> bool {
    matches!(
        ch,
        '：' | '，' | '。' | '！' | '？' | '；' | '、' | '（' | '）' | '「' | '」' | '『' | '』'
            | '【' | '】' | '．' | '—' | '〔' | '〕'
    )
}

/// Scores OCR output quality so multi-language Windows OCR picks the cleanest script match.
/// Rewards genuine multi-character CJK/non-ASCII text while heavily penalizing CJK recognizer
/// hallucinations (stray CJK chars, fullwidth punctuation `：）（．`, and spaced-out single-letter
/// fragments like `e a c h` or `m 0 d e`) on English/code screenshots.
pub fn score_ocr_text(text: &str) -> usize {
    let mut ascii_alpha = 0usize;
    let mut non_ascii_alpha = 0usize;
    let mut cjk_alpha = 0usize;
    let mut cjk_adjacent_pairs = 0usize;
    let mut fullwidth_punct = 0usize;
    let mut digits = 0usize;
    let mut prev_was_cjk = false;

    for ch in text.chars() {
        if is_cjk_or_kana_hangul(ch) {
            cjk_alpha += 1;
            non_ascii_alpha += 1;
            if prev_was_cjk {
                cjk_adjacent_pairs += 1;
            }
            prev_was_cjk = true;
        } else {
            if ch.is_alphabetic() {
                if ch.is_ascii() {
                    ascii_alpha += 1;
                } else {
                    non_ascii_alpha += 1;
                }
            } else if ch.is_ascii_digit() {
                digits += 1;
            } else if is_fullwidth_cjk_punct(ch) {
                fullwidth_punct += 1;
            }
            if !ch.is_whitespace() {
                prev_was_cjk = false;
            }
        }
    }

    let total_alpha = ascii_alpha + non_ascii_alpha;
    if total_alpha == 0 && digits == 0 {
        return 0;
    }

    // Genuine CJK/multilingual content has either adjacent CJK clusters (even across Windows OCR
    // single spaces) or meaningful CJK density, rather than 1-3 stray glyphs in 98% ASCII code.
    let genuine_cjk = cjk_alpha >= 2 && (cjk_adjacent_pairs >= 1 || cjk_alpha * 10 >= total_alpha);
    let latin_dominant = ascii_alpha >= 12 && ascii_alpha * 10 >= total_alpha * 8;

    let mut score: isize = (ascii_alpha as isize) * 2 + (digits as isize);

    let non_cjk_non_ascii = non_ascii_alpha.saturating_sub(cjk_alpha);
    score += (non_cjk_non_ascii as isize) * 5;

    if genuine_cjk && !latin_dominant {
        score += (cjk_alpha as isize) * 6;
    } else if genuine_cjk {
        // Mixed Latin + real CJK (e.g. bilingual media card or docs)
        score += (cjk_alpha as isize) * 5;
    } else if latin_dominant {
        // Isolated stray CJK glyphs (like `还`, `的`) inside English/code are hallucinations.
        score -= (cjk_alpha as isize) * 10;
    } else {
        score += (cjk_alpha as isize) * 4;
    }

    // Fullwidth CJK punctuation (`：`, `）`, `．`, `「`) inside Latin-dominant text is strong noise.
    if latin_dominant && !genuine_cjk {
        score -= (fullwidth_punct as isize) * 12;
    }

    // Reward clean multi-character ASCII words and penalize single-letter fragmentation (`e a c h`).
    let mut single_ascii_run = 0usize;
    for tok in text.split_whitespace() {
        let len = tok.chars().count();
        let all_ascii_alnum = tok.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        if all_ascii_alnum && len >= 3 {
            score += 2;
            single_ascii_run = 0;
        } else if len == 1 && tok.chars().next().is_some_and(|c| c.is_ascii_alphanumeric() || c == '/') {
            single_ascii_run += 1;
            if single_ascii_run >= 2 {
                score -= 5;
            }
        } else {
            single_ascii_run = 0;
        }
    }

    score.max(0) as usize
}

fn average_luma(img: &RgbaImage) -> f32 {
    let raw = img.as_raw();
    if raw.is_empty() {
        return 255.0;
    }
    let step = (raw.len() / 4 / 2048).max(1);
    let mut sum = 0.0f64;
    let mut count = 0u64;
    for px in raw.as_chunks::<4>().0.iter().step_by(step) {
        let luma = 0.299 * px[0] as f64 + 0.587 * px[1] as f64 + 0.114 * px[2] as f64;
        sum += luma;
        count += 1;
    }
    if count == 0 {
        255.0
    } else {
        (sum / count as f64) as f32
    }
}

#[derive(Clone, Copy)]
enum ContrastMode {
    None,
    InvertPercentileStretch,
    BinarizeOtsu { invert: bool },
}

fn rgb_luma(r: u8, g: u8, b: u8) -> u8 {
    (0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32).clamp(0.0, 255.0) as u8
}

/// Chroma-aware luminance for dark backgrounds: averages standard luma with `max(R, G, B)`
/// so syntax-colored blue/violet/red/orange tokens stay bright and crisp after inversion.
fn dark_syntax_luma(r: u8, g: u8, b: u8) -> u8 {
    let l = rgb_luma(r, g, b) as u16;
    let m = r.max(g).max(b) as u16;
    ((l + m) / 2) as u8
}

/// Computes Otsu's optimal binary threshold for grayscale luminance values.
fn otsu_threshold(hist: &[u32; 256], total: u64) -> u8 {
    if total == 0 {
        return 128;
    }
    let mut sum_all = 0.0f64;
    for (i, &count) in hist.iter().enumerate() {
        sum_all += (i as f64) * (count as f64);
    }
    let mut sum_bg = 0.0f64;
    let mut weight_bg = 0u64;
    let mut max_var = -1.0f64;
    let mut best_t = 128u8;

    for (t, &count) in hist.iter().enumerate() {
        weight_bg += count as u64;
        if weight_bg == 0 {
            continue;
        }
        let weight_fg = total - weight_bg;
        if weight_fg == 0 {
            break;
        }
        sum_bg += (t as f64) * (count as f64);
        let mean_bg = sum_bg / (weight_bg as f64);
        let mean_fg = (sum_all - sum_bg) / (weight_fg as f64);
        let diff = mean_bg - mean_fg;
        let between_var = (weight_bg as f64) * (weight_fg as f64) * diff * diff;
        if between_var > max_var {
            max_var = between_var;
            best_t = t as u8;
        }
    }
    best_t
}

/// Prepares a screenshot crop for high-accuracy `Windows.Media.Ocr`:
/// 1. Upscales small UI snips (up to 4x Catmull-Rom) so tiny 11-14px CJK strokes resolve cleanly.
/// 2. Handles dark-mode / low-contrast gray-on-black text via percentile contrast stretch or Otsu binarization.
/// 3. Adds generous 40px quiet-zone padding around all edges so edge-touching glyphs aren't dropped.
fn preprocess_for_ocr(src: &RgbaImage, mode: ContrastMode, max_dim: u32) -> RgbaImage {
    let (w, h) = src.dimensions();
    let desired_scale = if h < 48 || w < 140 {
        4u32
    } else if h < 90 || w < 240 {
        3u32
    } else if h < 1400 && w < 1800 {
        2u32
    } else {
        1u32
    };

    let pad = OCR_BORDER_PAD;
    let usable_max = max_dim.saturating_sub(pad * 2).max(256);
    let mut scale = desired_scale;
    while scale > 1 && (w * scale > usable_max || h * scale > usable_max) {
        scale -= 1;
    }

    let resized = if scale > 1 {
        image::imageops::resize(src, w * scale, h * scale, FilterType::CatmullRom)
    } else {
        src.clone()
    };

    let (rw, rh) = resized.dimensions();
    let fill = match mode {
        ContrastMode::None => *resized.get_pixel(0, 0),
        ContrastMode::InvertPercentileStretch | ContrastMode::BinarizeOtsu { .. } => {
            Rgba([255, 255, 255, 255])
        }
    };

    let mut out = RgbaImage::from_pixel(rw + pad * 2, rh + pad * 2, fill);

    match mode {
        ContrastMode::None => {
            image::imageops::overlay(&mut out, &resized, pad as i64, pad as i64);
        }
        ContrastMode::InvertPercentileStretch => {
            let mut hist = [0u32; 256];
            let total = (rw as u64) * (rh as u64);
            for px in resized.pixels() {
                let inv_luma = 255 - dark_syntax_luma(px[0], px[1], px[2]);
                hist[inv_luma as usize] += 1;
            }
            // Use 1st..98th percentile so stray 1px window borders don't defeat contrast stretching
            let lo_target = (total / 100).max(1);
            let hi_target = ((total * 98) / 100).max(lo_target);
            let mut acc = 0u64;
            let mut lo = 0u8;
            let mut hi = 255u8;
            for (v, &c) in hist.iter().enumerate() {
                acc += c as u64;
                if acc >= lo_target && lo == 0 {
                    lo = v as u8;
                }
                if acc >= hi_target {
                    hi = v as u8;
                    break;
                }
            }
            let span = (hi.saturating_sub(lo)).max(1) as f32;
            let stretch = span > 16.0;

            for (x, y, px) in resized.enumerate_pixels() {
                let inv = 255 - dark_syntax_luma(px[0], px[1], px[2]);
                let mapped = if stretch {
                    (((inv.saturating_sub(lo)) as f32 / span) * 255.0).clamp(0.0, 255.0) as u8
                } else {
                    inv
                };
                out.put_pixel(x + pad, y + pad, Rgba([mapped, mapped, mapped, 255]));
            }
        }
        ContrastMode::BinarizeOtsu { invert } => {
            let mut hist = [0u32; 256];
            let total = (rw as u64) * (rh as u64);
            for px in resized.pixels() {
                let l = if invert {
                    dark_syntax_luma(px[0], px[1], px[2])
                } else {
                    rgb_luma(px[0], px[1], px[2])
                };
                let v = if invert { 255 - l } else { l };
                hist[v as usize] += 1;
            }
            let threshold = otsu_threshold(&hist, total);
            for (x, y, px) in resized.enumerate_pixels() {
                let l = if invert {
                    dark_syntax_luma(px[0], px[1], px[2])
                } else {
                    rgb_luma(px[0], px[1], px[2])
                };
                let v = if invert { 255 - l } else { l };
                // Soft binarization keeps anti-aliased edge smoothness while crushing low-contrast gray
                let out_v = if v < threshold { 0 } else { 255 };
                out.put_pixel(x + pad, y + pad, Rgba([out_v, out_v, out_v, 255]));
            }
        }
    }

    out
}

fn rgba_to_software_bitmap(img: &RgbaImage) -> Result<SoftwareBitmap, OcrError> {
    let width = img.width() as i32;
    let height = img.height() as i32;
    let mut bgra = Vec::with_capacity((width as usize) * (height as usize) * 4);
    for px in img.as_raw().as_chunks::<4>().0 {
        bgra.push(px[2]); // B
        bgra.push(px[1]); // G
        bgra.push(px[0]); // R
        bgra.push(255); // A
    }

    let writer =
        DataWriter::new().map_err(|e| OcrError::WinRt(format!("DataWriter::new: {e}")))?;
    writer
        .WriteBytes(&bgra)
        .map_err(|e| OcrError::WinRt(format!("DataWriter::WriteBytes: {e}")))?;
    let buffer = writer
        .DetachBuffer()
        .map_err(|e| OcrError::WinRt(format!("DataWriter::DetachBuffer: {e}")))?;

    SoftwareBitmap::CreateCopyFromBuffer(&buffer, BitmapPixelFormat::Bgra8, width, height)
        .map_err(|e| OcrError::WinRt(format!("SoftwareBitmap::CreateCopyFromBuffer: {e}")))
}

fn build_candidate_engines() -> Result<Vec<(String, OcrEngine)>, OcrError> {
    let mut engines = Vec::new();
    let mut seen_tags = std::collections::HashSet::new();

    if let Ok(engine) = OcrEngine::TryCreateFromUserProfileLanguages() {
        let tag = engine
            .RecognizerLanguage()
            .and_then(|l| l.LanguageTag())
            .map(|s| s.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_else(|_| "profile".to_string());
        seen_tags.insert(tag.clone());
        engines.push((tag, engine));
    }

    if let Ok(langs) = OcrEngine::AvailableRecognizerLanguages() {
        let mut fallback_langs: Vec<(String, Language)> = Vec::new();
        for lang in langs {
            let tag = lang
                .LanguageTag()
                .map(|s| s.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default();
            if !tag.is_empty() && !seen_tags.contains(&tag) {
                fallback_langs.push((tag, lang));
            }
        }
        fallback_langs.sort_by_key(|(tag, _)| if tag.starts_with("en") { 0 } else { 1 });
        for (tag, lang) in fallback_langs.into_iter().take(6) {
            if let Ok(engine) = OcrEngine::TryCreateFromLanguage(&lang) {
                seen_tags.insert(tag.clone());
                engines.push((tag, engine));
            }
        }
    }

    if engines.is_empty() {
        return Err(OcrError::NoRecognizerAvailable);
    }
    Ok(engines)
}

fn run_engines_best(engines: &[(String, OcrEngine)], bitmap: &SoftwareBitmap) -> (String, usize) {
    let mut best_text = String::new();
    let mut best_score = 0usize;

    for (tag, engine) in engines {
        match recognize_with_engine(engine, bitmap) {
            Ok(text) => {
                let score = score_ocr_text(&text);
                if score > best_score || (best_text.is_empty() && !text.is_empty()) {
                    best_score = score;
                    best_text = text;
                }
            }
            Err(err) => {
                log::warn!("Windows OCR recognizer '{tag}' failed: {err}");
            }
        }
    }
    (best_text, best_score)
}

fn run_single_ocr_pass(
    decoded: &RgbaImage,
    mode: ContrastMode,
    engine_max_dim: u32,
) -> Result<(String, usize), OcrError> {
    let _com = ComGuard::init_mta();
    let engines = build_candidate_engines()?;
    let img = preprocess_for_ocr(decoded, mode, engine_max_dim);
    let bmp = rgba_to_software_bitmap(&img)?;
    Ok(run_engines_best(&engines, &bmp))
}

/// Extract text directly from raw RGBA pixels without any PNG encode/decode overhead.
/// Automatically upscales small UI text, runs dark-mode inverted & standard passes in parallel,
/// and evaluates installed Windows OCR language packs (Chinese, English, Japanese, etc.).
pub fn extract_text_from_rgba(width: u32, height: u32, rgba: &[u8]) -> Result<String, OcrError> {
    if width == 0 || height == 0 {
        return Ok(String::new());
    }
    let img = RgbaImage::from_raw(width, height, rgba.to_vec())
        .ok_or_else(|| OcrError::ImageDecode("invalid RGBA buffer dimensions".to_string()))?;
    extract_text_from_rgba_image(&img)
}

/// Extract text from cropped PNG bytes using Windows built-in offline `Windows.Media.Ocr`.
/// Automatically upscales small UI text, handles dark-mode backgrounds (Spotify/VS Code),
/// and evaluates installed Windows OCR language packs (Chinese, English, Japanese, etc.).
pub fn extract_text_from_png(png_bytes: &[u8]) -> Result<String, OcrError> {
    let decoded = image::load_from_memory_with_format(png_bytes, ImageFormat::Png)
        .map_err(|e| OcrError::ImageDecode(e.to_string()))?
        .to_rgba8();
    extract_text_from_rgba_image(&decoded)
}

fn extract_text_from_rgba_image(decoded: &RgbaImage) -> Result<String, OcrError> {
    let _com = ComGuard::init_mta();

    if decoded.width() == 0 || decoded.height() == 0 {
        return Ok(String::new());
    }

    let engine_max_dim = OcrEngine::MaxImageDimension()
        .ok()
        .filter(|&d| d >= 512)
        .unwrap_or(MAX_OCR_DIM)
        .min(MAX_OCR_DIM);
    let is_dark_bg = average_luma(decoded) < 126.0;

    let (mut best_text, mut best_score) = if is_dark_bg {
        // Run dark-mode inverted pass and non-inverted pass in parallel to nearly halve latency
        // while keeping full accuracy across mixed dark/bright UI regions.
        std::thread::scope(|s| {
            let h1 = s.spawn(|| {
                run_single_ocr_pass(decoded, ContrastMode::InvertPercentileStretch, engine_max_dim)
            });
            let h2 = s.spawn(|| run_single_ocr_pass(decoded, ContrastMode::None, engine_max_dim));
            let r1 = h1
                .join()
                .map_err(|_| OcrError::WinRt("OCR pass 1 thread panicked".to_string()))?;
            let r2 = h2
                .join()
                .map_err(|_| OcrError::WinRt("OCR pass 2 thread panicked".to_string()))?;
            match (r1, r2) {
                (Ok((t1, s1)), Ok((t2, s2))) => {
                    if s2 > s1 {
                        Ok((t2, s2))
                    } else {
                        Ok((t1, s1))
                    }
                }
                (Ok(ok), Err(_)) | (Err(_), Ok(ok)) => Ok(ok),
                (Err(err), Err(_)) => Err(err),
            }
        })?
    } else {
        run_single_ocr_pass(decoded, ContrastMode::None, engine_max_dim)?
    };

    // Pass 3: If low-contrast tiny text (e.g. muted #757575 gray Spotify credits on #181818)
    // still yielded little/no text, run high-contrast Otsu binarized fallback.
    if best_score < 12 {
        let (otsu_text, otsu_score) = run_single_ocr_pass(
            decoded,
            ContrastMode::BinarizeOtsu { invert: is_dark_bg },
            engine_max_dim,
        )?;
        if otsu_score > best_score {
            best_text = otsu_text;
            best_score = otsu_score;
        }
    }

    log::info!(
        "OCR finished (dark_bg={is_dark_bg}, score={best_score}, chars={})",
        best_text.chars().count()
    );
    Ok(best_text)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use image::codecs::png::PngEncoder;
    use image::{ExtendedColorType, ImageEncoder};
    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::{COLORREF, RECT};
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleBitmap, CreateCompatibleDC, CreateFontW, CreateSolidBrush, DeleteDC,
        DeleteObject, DrawTextW, FillRect, GetDC, GetDIBits, ReleaseDC, SelectObject, SetBkMode,
        SetTextColor, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET,
        DEFAULT_QUALITY, DIB_RGB_COLORS, DT_CENTER, DT_SINGLELINE, DT_VCENTER, FF_SWISS, FW_BOLD,
        FW_NORMAL, HGDIOBJ, OUT_DEFAULT_PRECIS, TRANSPARENT,
    };

    #[allow(clippy::too_many_arguments)]
    fn render_styled_png(
        text: &str,
        width: i32,
        height: i32,
        font_height: i32,
        font_name: PCWSTR,
        bg_bgr: u32,
        fg_bgr: u32,
        bold: bool,
    ) -> Vec<u8> {
        // SAFETY: Renders text onto an offscreen DIB for testing Windows.Media.Ocr.
        unsafe {
            let screen_dc = GetDC(None);
            let mem_dc = CreateCompatibleDC(Some(screen_dc));
            let bmp = CreateCompatibleBitmap(screen_dc, width, height);
            let prev_bmp = SelectObject(mem_dc, HGDIOBJ(bmp.0));

            let bg = CreateSolidBrush(COLORREF(bg_bgr));
            let mut rect = RECT {
                left: 0,
                top: 0,
                right: width,
                bottom: height,
            };
            let _ = FillRect(mem_dc, &rect, bg);

            let font = CreateFontW(
                -font_height,
                0,
                0,
                0,
                if bold { FW_BOLD.0 } else { FW_NORMAL.0 } as i32,
                0,
                0,
                0,
                DEFAULT_CHARSET,
                OUT_DEFAULT_PRECIS,
                CLIP_DEFAULT_PRECIS,
                DEFAULT_QUALITY,
                FF_SWISS.0 as u32,
                font_name,
            );
            let prev_font = SelectObject(mem_dc, HGDIOBJ(font.0));
            let _ = SetBkMode(mem_dc, TRANSPARENT);
            let _ = SetTextColor(mem_dc, COLORREF(fg_bgr));

            if !text.is_empty() {
                let mut wide: Vec<u16> = text.encode_utf16().collect();
                let _ = DrawTextW(
                    mem_dc,
                    &mut wide,
                    &mut rect,
                    DT_CENTER | DT_VCENTER | DT_SINGLELINE,
                );
            }

            let mut bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width,
                    biHeight: -height,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bgra = vec![0u8; (width as usize) * (height as usize) * 4];
            let _ = GetDIBits(
                mem_dc,
                bmp,
                0,
                height as u32,
                Some(bgra.as_mut_ptr().cast()),
                &mut bmi,
                DIB_RGB_COLORS,
            );

            let _ = SelectObject(mem_dc, prev_font);
            let _ = SelectObject(mem_dc, prev_bmp);
            let _ = DeleteObject(HGDIOBJ(font.0));
            let _ = DeleteObject(HGDIOBJ(bg.0));
            let _ = DeleteObject(HGDIOBJ(bmp.0));
            let _ = DeleteDC(mem_dc);
            ReleaseDC(None, screen_dc);

            let mut rgba = Vec::with_capacity(bgra.len());
            for px in bgra.as_chunks::<4>().0 {
                rgba.push(px[2]);
                rgba.push(px[1]);
                rgba.push(px[0]);
                rgba.push(255);
            }
            let mut png = Vec::new();
            PngEncoder::new(&mut png)
                .write_image(&rgba, width as u32, height as u32, ExtendedColorType::Rgba8)
                .expect("encode test png");
            png
        }
    }

    pub(crate) fn render_text_png(text: &str, width: i32, height: i32) -> Vec<u8> {
        render_styled_png(
            text,
            width,
            height,
            36,
            w!("Segoe UI"),
            0x00FFFFFF,
            0x00000000,
            true,
        )
    }

    #[test]
    fn score_ocr_text_prefers_clean_multilingual_text_over_noise() {
        assert!(score_ocr_text("我叫长安") > score_ocr_text("... ---"));
        assert!(score_ocr_text("我叫长安 Chinese") > score_ocr_text("Chinese"));
        assert!(score_ocr_text("Xin chào thế giới") > score_ocr_text("|||"));
        assert_eq!(score_ocr_text("   ...   "), 0);
        let garbled_cjk_on_rust_code =
            "/ / / Semantic p ill- colors 、 for e a c h smart-format m 0 d e ．\n\
             PUb fn format—badge—palette(kind ： Option<SmartFormatKind>) 一 > （ COIO 「 52\n\
             match kind 还\n\
             Some(SmartFormatKind ： :C0de | SmartFormatKind · :Json\n\
             C0t0r32 ： :from-rgb(24, 2 的 38 ） ，";
        let clean_en_rust_code =
            "/// Semantic pill colors (fill, stroke, text, dot) for each smart-format mode.\n\
             pub fn format_badge_palette(kind: Option<SmartFormatKind>) -> (Color32, Color32)\n\
             match kind {\n\
             Some(SmartFormatKind::Code | SmartFormatKind::Json) => (\n\
             Color32::from_rgb(24, 20, 38),";
        assert!(
            score_ocr_text(clean_en_rust_code) > score_ocr_text(garbled_cjk_on_rust_code),
            "clean en-US code OCR must beat zh-Hans hallucinated output"
        );
    }

    #[test]
    fn windows_ocr_extracts_rendered_text_and_handles_blank_image() {
        let blank_png = render_text_png("", 240, 80);
        let blank_text = extract_text_from_png(&blank_png).expect("ocr on blank png");
        assert!(
            blank_text.trim().is_empty(),
            "expected empty OCR result on blank image, got: {blank_text:?}"
        );

        let text_png = render_text_png("Hello World 2026", 480, 120);
        let extracted = extract_text_from_png(&text_png).expect("ocr on rendered text");
        let lower = extracted.to_ascii_lowercase();
        assert!(
            lower.contains("hello") && lower.contains("world"),
            "expected Windows.Media.Ocr to extract 'Hello World', got: {extracted:?}"
        );
    }

    pub(super) fn has_cjk_ocr_pack() -> bool {
        let _com = ComGuard::init_mta();
        OcrEngine::AvailableRecognizerLanguages()
            .ok()
            .into_iter()
            .flatten()
            .filter_map(|lang| lang.LanguageTag().ok())
            .any(|tag| {
                let t = tag.to_string_lossy().to_ascii_lowercase();
                t.starts_with("zh") || t.starts_with("ja") || t.starts_with("ko")
            })
    }

    #[test]
    fn windows_ocr_extracts_small_dark_mode_chinese_spotify_style() {
        if !has_cjk_ocr_pack() {
            // Skip CJK-specific glyph assertions on en-US-only CI runners without Chinese OCR packs.
            return;
        }
        // Reproduces Spotify dark mode (#121212 background, #B8B8B8 14px Microsoft YaHei Chinese text)
        let dark_zh_png = render_styled_png(
            "我叫长安，你叫故里",
            220,
            34,
            14,
            w!("Microsoft YaHei"),
            0x00121212,
            0x00B8B8B8,
            false,
        );
        let extracted = extract_text_from_png(&dark_zh_png).expect("ocr on dark-mode chinese");
        let has_chinese = extracted.chars().any(|c| ('\u{4E00}'..='\u{9FFF}').contains(&c));
        assert!(
            has_chinese,
            "expected Windows.Media.Ocr (zh-Hans-CN) to detect small dark-mode Chinese text, got: {extracted:?}"
        );

        // Also reproduces faint 12px dim gray (#787878) 4-char Chinese credits ("欧阳尚尚") on #181818
        let faint_zh_png = render_styled_png(
            "欧阳尚尚",
            92,
            24,
            13,
            w!("Microsoft YaHei"),
            0x00181818,
            0x00787878,
            false,
        );
        let extracted2 = extract_text_from_png(&faint_zh_png).expect("ocr on faint 4-char chinese");
        let has_chinese2 = extracted2.chars().any(|c| ('\u{4E00}'..='\u{9FFF}').contains(&c));
        assert!(
            has_chinese2,
            "expected Windows.Media.Ocr to detect faint 4-char Spotify Chinese '欧阳尚尚', got: {extracted2:?}"
        );
    }

    #[test]
    fn real_png_fixture_dark_ui_code_card_extracts_full_text() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("dark_ui_code_card.png");
        let bytes = std::fs::read(&path).expect("read tests/fixtures/dark_ui_code_card.png");
        let text = extract_text_from_png(&bytes).expect("OCR dark_ui_code_card.png");
        let lower = text.to_ascii_lowercase();
        assert!(
            lower.contains("reticle") && lower.contains("windows") && lower.contains("cargo"),
            "expected full dark-mode UI + code pill text from fixture, got: {text:?}"
        );
    }

    #[test]
    fn real_png_fixture_light_doc_checklist_extracts_all_lines() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("light_doc_checklist.png");
        let bytes = std::fs::read(&path).expect("read tests/fixtures/light_doc_checklist.png");
        let text = extract_text_from_png(&bytes).expect("OCR light_doc_checklist.png");
        let lower = text.to_ascii_lowercase();
        assert!(
            lower.contains("release checklist")
                && lower.contains("dwm")
                && lower.contains("side-docked")
                && lower.contains("upscaling"),
            "expected all checklist lines from light_doc_checklist.png, got: {text:?}"
        );
    }

    #[test]
    fn real_png_fixture_dark_ui_cjk_card_extracts_multilingual_text() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("dark_ui_cjk_card.png");
        let bytes = std::fs::read(&path).expect("read tests/fixtures/dark_ui_cjk_card.png");
        let text = extract_text_from_png(&bytes).expect("OCR dark_ui_cjk_card.png");
        let lower = text.to_ascii_lowercase();
        assert!(
            lower.contains("midnight") && lower.contains("2026"),
            "expected Latin subtitle from dark_ui_cjk_card.png, got: {text:?}"
        );
        if has_cjk_ocr_pack() {
            assert!(
                text.chars().any(|c| ('\u{4E00}'..='\u{9FFF}').contains(&c)),
                "expected Chinese glyphs from dark_ui_cjk_card.png when CJK OCR pack is installed, got: {text:?}"
            );
        }
    }

    #[test]
    fn real_png_fixture_annotation_composite_and_reencode_roundtrip() {
        use crate::capture::{CroppedImage, ScreenRect};
        use crate::snip::annotate::{AnnotationDoc, AnnotationOp, StrokeSize, PALETTE};
        use crate::snip::raster::composite_annotations;
        use crate::snip::state::ScreenPt;

        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("dark_ui_code_card.png");
        let bytes = std::fs::read(&path).expect("read fixture png");
        let decoded = image::load_from_memory_with_format(&bytes, ImageFormat::Png)
            .expect("decode fixture png")
            .to_rgba8();
        let (w, h) = decoded.dimensions();

        let mut cropped = CroppedImage {
            width: w,
            height: h,
            rgba: decoded.into_raw(),
            screen_rect: ScreenRect {
                x: 0,
                y: 0,
                width: w as i32,
                height: h as i32,
            },
        };

        let mut doc = AnnotationDoc::default();
        doc.push(AnnotationOp::Rect {
            from: ScreenPt { x: 40, y: 40 },
            to: ScreenPt { x: 360, y: 120 },
            color: PALETTE[0],
            stroke_px: StrokeSize::Medium.stroke_px(),
        });
        doc.push(AnnotationOp::Mosaic {
            rect: ScreenRect {
                x: 52,
                y: 194,
                width: 188,
                height: 44,
            },
            block_size: 10,
        });

        let before = cropped.rgba.clone();
        composite_annotations(&mut cropped, &doc);
        assert_ne!(
            cropped.rgba, before,
            "compositing annotations onto real PNG fixture must mutate pixels"
        );

        let reencoded = cropped.to_captured_region().expect("re-encode annotated png");
        let verified = image::load_from_memory_with_format(&reencoded.png_bytes, ImageFormat::Png)
            .expect("re-decode annotated png")
            .to_rgba8();
        assert_eq!(verified.dimensions(), (w, h));
    }
}
