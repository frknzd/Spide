//! Look & feel: the dark palette of the old stylesheet mapped onto egui visuals, plus fonts
//! (Helvetica Neue and Menlo from the system, egui's fonts as fallback).

use egui::{Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Stroke, Visuals};
use spide_diagram::visuals::{Family, FontSpec, Rgb};
use std::sync::Arc;

pub fn app_icon() -> egui::IconData {
    eframe::icon_data::from_png_bytes(include_bytes!("../../icons/128x128@2x.png")).unwrap_or_default()
}

pub fn col(c: Rgb, a: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(c.0, c.1, c.2, (a.clamp(0.0, 1.0) * 255.0).round() as u8)
}

pub const fn hex(v: u32) -> Color32 {
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

/// CSS variables of the old UI (the refined dark design language).
pub mod css {
    use super::hex;
    use egui::Color32;
    pub const BG: Color32 = hex(0x15171b);
    pub const SURFACE: Color32 = hex(0x1b1e24);
    pub const RAISED: Color32 = hex(0x22262e);
    pub const RAISED2: Color32 = hex(0x2a2f39);
    pub const PANEL_SOLID: Color32 = hex(0x1d2026);
    /// rgba(255,255,255,.085) on the dark surfaces
    pub const LINE: Color32 = Color32::from_rgba_premultiplied(22, 22, 22, 22);
    /// rgba(255,255,255,.15)
    pub const LINE_STRONG: Color32 = Color32::from_rgba_premultiplied(38, 38, 38, 38);
    pub const TEXT: Color32 = hex(0xeceef3);
    pub const DIM: Color32 = hex(0xb0b6c3);
    pub const FAINT: Color32 = hex(0x8c94a5);
    pub const ACCENT: Color32 = hex(0x8aa9ff);
    pub const ACCENT_SOFT: Color32 = Color32::from_rgba_premultiplied(18, 22, 33, 33);
    pub const GHOST: Color32 = hex(0x5eead4);
    pub const BAD: Color32 = hex(0xff7482);
    pub const WARN: Color32 = hex(0xffbf69);
    pub const GOOD: Color32 = hex(0x5fe39a);
    pub const PRO: Color32 = hex(0x86efac);
    pub const CON: Color32 = hex(0xfda4af);
    pub const INK: Color32 = hex(0x0b1020);
    pub const SEPARATOR: Color32 = hex(0x292b2f);
}

// Canvas fonts (the diagram's `Helvetica Neue` and `Menlo`).
pub const SANS: &str = "canvas-sans";
pub const SANS_BOLD: &str = "canvas-sans-bold";
pub const SANS_ITALIC: &str = "canvas-sans-italic";
pub const SANS_BOLD_ITALIC: &str = "canvas-sans-bold-italic";
pub const MONO: &str = "canvas-mono";
pub const MONO_BOLD: &str = "canvas-mono-bold";
pub const MONO_ITALIC: &str = "canvas-mono-italic";
pub const MONO_BOLD_ITALIC: &str = "canvas-mono-bold-italic";
// Chrome fonts (the UI's `-apple-system` / SF Pro): Proportional and Monospace plus these.
pub const UI_SEMIBOLD: &str = "ui-semibold";
pub const UI_ITALIC: &str = "ui-italic";

pub fn family(f: &FontSpec) -> FontFamily {
    let n = match (f.family, f.bold, f.italic) {
        (Family::Sans, false, false) => SANS,
        (Family::Sans, true, false) => SANS_BOLD,
        (Family::Sans, false, true) => SANS_ITALIC,
        (Family::Sans, true, true) => SANS_BOLD_ITALIC,
        (Family::Mono, false, false) => MONO,
        (Family::Mono, true, false) => MONO_BOLD,
        (Family::Mono, false, true) => MONO_ITALIC,
        (Family::Mono, true, true) => MONO_BOLD_ITALIC,
    };
    FontFamily::Name(n.into())
}

/// A canvas font.
pub fn font_id(f: FontSpec) -> FontId {
    FontId::new(f.size, family(&f))
}

/// Chrome text (SF Pro).
pub fn sans(size: f32) -> FontId {
    FontId::proportional(size)
}
/// Chrome semibold (CSS `font-weight: 600`).
pub fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(UI_SEMIBOLD.into()))
}
pub fn italic(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(UI_ITALIC.into()))
}
/// Chrome monospace (SF Mono).
pub fn mono(size: f32) -> FontId {
    FontId::monospace(size)
}

fn system_face(path: &str, index: u32) -> Option<FontData> {
    let bytes = std::fs::read(path).ok()?;
    Some(FontData::from_blob(Arc::new(bytes), index))
}

/// Install fonts: SF Pro for the chrome, Helvetica Neue and Menlo for the canvas (as the old CSS
/// did), egui's fonts as fallback everywhere.
pub fn install_fonts(ctx: &egui::Context) {
    use egui::epaint::text::{FontTweak, VariationCoords};
    let mut defs = FontDefinitions::default();
    let fallbacks: Vec<String> = defs.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let mono_fallbacks: Vec<String> = defs.families.get(&FontFamily::Monospace).cloned().unwrap_or_default();
    let mut add = |name: &str, path: &str, index: u32, weight: Option<f32>| -> Option<String> {
        let mut data = system_face(path, index)?;
        if let Some(w) = weight {
            // optical size of SF Pro *Text* (the browser picks it from the font size)
            data = data.tweak(FontTweak { coords: VariationCoords::new([(b"wght", w), (b"opsz", 13.0)]), ..Default::default() });
        }
        defs.font_data.insert(name.to_string(), Arc::new(data));
        Some(name.to_string())
    };
    let neue = "/System/Library/Fonts/HelveticaNeue.ttc";
    let menlo = "/System/Library/Fonts/Menlo.ttc";
    let mut symbols: Vec<String> = add("apple-symbols", "/System/Library/Fonts/Apple Symbols.ttf", 0, None).into_iter().collect();
    symbols.extend(add("apple-emoji", "/System/Library/Fonts/Apple Color Emoji.ttc", 0, None));
    let menlo_regular = add("menlo-regular", menlo, 0, None);
    let faces: Vec<(&str, Option<String>, bool)> = vec![
        (SANS, add("hn-regular", neue, 0, None), false),
        (SANS_BOLD, add("hn-bold", neue, 1, None), false),
        (SANS_ITALIC, add("hn-italic", neue, 2, None), false),
        (SANS_BOLD_ITALIC, add("hn-bold-italic", neue, 3, None), false),
        (MONO, menlo_regular.clone(), true),
        (MONO_BOLD, add("menlo-bold", menlo, 1, None), true),
        (MONO_ITALIC, add("menlo-italic", menlo, 2, None), true),
        (MONO_BOLD_ITALIC, add("menlo-bold-italic", menlo, 3, None), true),
        ("@ui", add("sf-regular", "/System/Library/Fonts/SFNS.ttf", 0, Some(400.0)), false),
        (UI_SEMIBOLD, add("sf-semibold", "/System/Library/Fonts/SFNS.ttf", 0, Some(600.0)), false),
        (UI_ITALIC, add("sf-italic", "/System/Library/Fonts/SFNSItalic.ttf", 0, Some(400.0)), false),
        ("@ui-mono", add("sf-mono", "/System/Library/Fonts/SFNSMono.ttf", 0, Some(400.0)), true),
    ];
    for (fam, face, is_mono) in faces {
        let mut list: Vec<String> = face.into_iter().collect();
        if !is_mono {
            list.extend(menlo_regular.clone());
        }
        list.extend(symbols.clone());
        list.extend(if is_mono { mono_fallbacks.iter().cloned() } else { fallbacks.iter().cloned() });
        let key = match fam {
            "@ui" => FontFamily::Proportional,
            "@ui-mono" => FontFamily::Monospace,
            n => FontFamily::Name(n.into()),
        };
        defs.families.insert(key, list);
    }
    ctx.set_fonts(defs);
}

/// egui visuals matching the old CSS (dark panels, thin lines, accent blue).
pub fn install_style(ctx: &egui::Context) {
    let mut v = Visuals::dark();
    v.panel_fill = css::BG;
    v.window_fill = css::PANEL_SOLID;
    v.extreme_bg_color = css::BG;
    v.faint_bg_color = css::RAISED;
    v.override_text_color = Some(css::TEXT);
    v.hyperlink_color = css::ACCENT;
    v.selection.bg_fill = Color32::from_rgba_unmultiplied(138, 169, 255, 77);
    v.selection.stroke = Stroke::new(1.0, css::ACCENT);
    v.window_stroke = Stroke::new(1.0, css::LINE_STRONG);
    v.window_corner_radius = CornerRadius::same(12);
    v.menu_corner_radius = CornerRadius::same(10);
    v.window_shadow = egui::Shadow { offset: [0, 10], blur: 40, spread: 0, color: Color32::from_black_alpha(90) };
    v.popup_shadow = egui::Shadow { offset: [0, 8], blur: 28, spread: 0, color: Color32::from_black_alpha(80) };
    let w = &mut v.widgets;
    let r = CornerRadius::same(7);
    w.noninteractive.bg_stroke = Stroke::new(1.0, css::LINE);
    w.noninteractive.fg_stroke = Stroke::new(1.0, css::TEXT);
    w.noninteractive.corner_radius = r;
    w.inactive.bg_fill = css::RAISED;
    w.inactive.weak_bg_fill = css::RAISED;
    w.inactive.bg_stroke = Stroke::new(1.0, css::LINE);
    w.inactive.fg_stroke = Stroke::new(1.0, css::TEXT);
    w.inactive.corner_radius = r;
    w.hovered.bg_fill = css::RAISED2;
    w.hovered.weak_bg_fill = css::RAISED2;
    w.hovered.bg_stroke = Stroke::new(1.0, css::LINE_STRONG);
    w.hovered.fg_stroke = Stroke::new(1.0, css::TEXT);
    w.hovered.corner_radius = r;
    w.hovered.expansion = 0.0;
    w.active.bg_fill = css::RAISED2;
    w.active.weak_bg_fill = css::RAISED2;
    w.active.bg_stroke = Stroke::new(1.0, css::ACCENT);
    w.active.fg_stroke = Stroke::new(1.0, css::TEXT);
    w.active.corner_radius = r;
    w.active.expansion = 0.0;
    w.open = w.active;
    ctx.set_visuals(v);
    ctx.global_style_mut(|s| {
        s.spacing.item_spacing = egui::vec2(8.0, 6.0);
        s.spacing.button_padding = egui::vec2(10.0, 4.0);
        s.spacing.interact_size.y = 24.0;
        s.spacing.window_margin = Margin::same(14);
        s.spacing.menu_margin = Margin::same(8);
        s.text_styles.insert(egui::TextStyle::Body, sans(13.0));
        s.text_styles.insert(egui::TextStyle::Button, sans(12.5));
        s.text_styles.insert(egui::TextStyle::Small, sans(11.0));
        s.text_styles.insert(egui::TextStyle::Monospace, mono(12.0));
        s.text_styles.insert(egui::TextStyle::Heading, bold(17.0));
        s.interaction.tooltip_delay = 0.35;
    });
}
