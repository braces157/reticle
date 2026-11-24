use egui::{
    Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, Style, TextStyle,
    Visuals,
};
use std::path::Path;
use std::sync::Arc;

// Precision toolmaker obsidian palette (achromatic tonal steps + inverted-ink CTAs)
pub const BG_WINDOW: Color32 = Color32::from_rgb(10, 10, 10); // #0A0A0A matte obsidian
pub const BG_HEADER: Color32 = Color32::from_rgb(14, 14, 14); // #0E0E0E quiet top rail
pub const BG_CARD: Color32 = Color32::from_rgb(18, 18, 18); // #121212 elevated surface
pub const BG_FOOTER: Color32 = Color32::from_rgb(12, 12, 12); // #0C0C0C recessed bottom dock
pub const BG_SEGMENT_TRACK: Color32 = Color32::from_rgb(20, 20, 20); // #141414 pill track
pub const BG_PILL: Color32 = Color32::from_rgb(26, 26, 26); // #1A1A1A resting well
pub const BG_PILL_HOVER: Color32 = Color32::from_rgb(36, 36, 36); // #242424 hover lift
pub const BG_PILL_ACTIVE: Color32 = Color32::from_rgb(44, 44, 44); // #2C2C2C active thumb

// Ink-alpha hairlines (whisper-quiet at rest, crisp on hover)
pub const BORDER_HAIRLINE: Color32 = Color32::from_rgb(30, 30, 30); // ~8% white on #0A0A0A
pub const BORDER_SUBTLE: Color32 = Color32::from_rgb(46, 46, 46); // ~14% white on #0A0A0A
pub const BORDER_HIGHLIGHT: Color32 = Color32::from_rgb(72, 72, 72); // ~24% white rim
pub const DOT_GRID: Color32 = Color32::from_rgba_premultiplied(14, 14, 14, 14);

// Editorial ink contrast ladder (hierarchy by brightness, not heavy bold)
pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(255, 255, 255); // #FFFFFF
pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(180, 180, 180); // #B4B4B4
pub const TEXT_TERTIARY: Color32 = Color32::from_rgb(142, 141, 136); // #8E8D88 warm stone muted
pub const TEXT_MUTED: Color32 = Color32::from_rgb(112, 112, 112); // #707070 metadata / kbd

// Inverted-ink primary CTA (#FFF fill / #000 ink)
pub const CTA_FILL: Color32 = Color32::from_rgb(255, 255, 255);
pub const CTA_FILL_HOVER: Color32 = Color32::from_rgb(232, 232, 232);
pub const CTA_FILL_ACTIVE: Color32 = Color32::from_rgb(206, 206, 206);
pub const CTA_INK: Color32 = Color32::from_rgb(10, 10, 10);

// Selection / optical pick accent (reserved strictly for reticle, selection, and active indicator)
pub const ACCENT_PICK: Color32 = Color32::from_rgb(58, 169, 255); // #3AA9FF
pub const SHIMMER_BAR: Color32 = Color32::from_rgb(255, 255, 255);

// Quiet semantic status signals
pub const OK_EMERALD: Color32 = Color32::from_rgb(52, 209, 122); // #34D17A
pub const OK_EMERALD_BG: Color32 = Color32::from_rgb(16, 36, 24);
pub const OK_EMERALD_BORDER: Color32 = Color32::from_rgb(32, 82, 52);

pub const WARN_ICON_BG: Color32 = Color32::from_rgb(36, 28, 14);
pub const WARN_TEXT: Color32 = Color32::from_rgb(224, 154, 18); // #E09A12
pub const CLOSE_HOVER_BG: Color32 = Color32::from_rgb(229, 72, 77); // #E5484D

pub const CLEAR_COLOR_RGBA: [f32; 4] = [10.0 / 255.0, 10.0 / 255.0, 10.0 / 255.0, 1.0];

fn load_font_if_exists(fonts: &mut FontDefinitions, name: &str, path: &str) -> bool {
    if Path::new(path).exists()
        && let Ok(bytes) = std::fs::read(path)
    {
        fonts
            .font_data
            .insert(name.to_owned(), Arc::new(FontData::from_owned(bytes)));
        return true;
    }
    false
}

/// Loads native Windows 11 Segoe UI + Cascadia Mono + multi-script fallbacks for crisp Unicode.
fn install_windows_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let mut proportional_chain = Vec::new();
    let mut mono_chain = Vec::new();

    if load_font_if_exists(&mut fonts, "segoe_ui", r"C:\Windows\Fonts\segoeui.ttf") {
        proportional_chain.push("segoe_ui".to_owned());
    }
    if load_font_if_exists(&mut fonts, "cascadia_mono", r"C:\Windows\Fonts\CascadiaMono.ttf") {
        mono_chain.push("cascadia_mono".to_owned());
    } else if load_font_if_exists(&mut fonts, "consolas", r"C:\Windows\Fonts\consola.ttf") {
        mono_chain.push("consolas".to_owned());
    }

    for (name, path) in [
        ("leelawadee", r"C:\Windows\Fonts\LeelawUI.ttf"),
        ("nirmala", r"C:\Windows\Fonts\Nirmala.ttc"),
        ("msyh", r"C:\Windows\Fonts\msyh.ttc"),
        ("yugothic", r"C:\Windows\Fonts\YuGothR.ttc"),
        ("malgun", r"C:\Windows\Fonts\malgun.ttf"),
    ] {
        if load_font_if_exists(&mut fonts, name, path) {
            proportional_chain.push(name.to_owned());
            mono_chain.push(name.to_owned());
        }
    }

    if !proportional_chain.is_empty()
        && let Some(family) = fonts.families.get_mut(&FontFamily::Proportional)
    {
        for (idx, name) in proportional_chain.into_iter().enumerate() {
            family.insert(idx, name);
        }
    }
    if !mono_chain.is_empty()
        && let Some(family) = fonts.families.get_mut(&FontFamily::Monospace)
    {
        for (idx, name) in mono_chain.into_iter().enumerate() {
            family.insert(idx, name);
        }
    }

    ctx.set_fonts(fonts);
}

pub fn apply_theme(ctx: &egui::Context) {
    install_windows_fonts(ctx);

    let mut visuals = Visuals::dark();
    visuals.window_fill = BG_WINDOW;
    visuals.panel_fill = BG_WINDOW;
    visuals.extreme_bg_color = BG_FOOTER;
    visuals.faint_bg_color = BG_PILL;
    visuals.override_text_color = Some(TEXT_PRIMARY);
    visuals.selection.bg_fill = Color32::from_rgb(32, 62, 92);
    visuals.selection.stroke = Stroke::NONE;
    visuals.window_stroke = Stroke::new(1.0, BORDER_HAIRLINE);
    visuals.popup_shadow = egui::Shadow {
        offset: [0, 10],
        blur: 28,
        spread: 0,
        color: Color32::from_black_alpha(190),
    };

    visuals.widgets.noninteractive.bg_fill = BG_WINDOW;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER_HAIRLINE);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);
    visuals.widgets.noninteractive.corner_radius = CornerRadius::same(8);

    visuals.widgets.inactive.bg_fill = BG_PILL;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER_HAIRLINE);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);
    visuals.widgets.inactive.corner_radius = CornerRadius::same(8);

    visuals.widgets.hovered.bg_fill = BG_PILL_HOVER;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, BORDER_SUBTLE);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);
    visuals.widgets.hovered.corner_radius = CornerRadius::same(8);

    visuals.widgets.active.bg_fill = BG_PILL_ACTIVE;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, BORDER_HIGHLIGHT);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);
    visuals.widgets.active.corner_radius = CornerRadius::same(8);

    visuals.widgets.open.bg_fill = BG_PILL_HOVER;
    visuals.widgets.open.bg_stroke = Stroke::new(1.0, BORDER_SUBTLE);
    visuals.widgets.open.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);
    visuals.widgets.open.corner_radius = CornerRadius::same(8);

    let mut style = Style {
        visuals,
        ..Style::default()
    };
    style.text_styles.insert(
        TextStyle::Body,
        FontId::new(14.0, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Button,
        FontId::new(12.0, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Small,
        FontId::new(11.0, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Monospace,
        FontId::new(12.0, FontFamily::Monospace),
    );
    style.spacing.item_spacing = egui::vec2(6.0, 6.0);
    style.spacing.button_padding = egui::vec2(10.0, 5.0);
    style.spacing.scroll = egui::style::ScrollStyle::thin();

    ctx.set_theme(egui::Theme::Dark);
    ctx.set_style_of(egui::Theme::Dark, style);
}
