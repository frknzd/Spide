//! Small styled widgets: the old UI's single button vocabulary, labels and panel frames.

use super::theme::{bold, css, sans};
use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, CornerRadius, FontId, Margin, Response, RichText, Sense, Stroke, Ui, Vec2};

/// Standard button: raised surface, thin line, 6px radius.
pub fn button(ui: &mut Ui, text: impl Into<String>) -> Response {
    ui.add(egui::Button::new(RichText::new(text.into()).font(sans(12.0)).color(css::TEXT)).fill(css::RAISED).stroke(Stroke::new(1.0, css::LINE)).corner_radius(6).min_size(Vec2::new(0.0, 26.0)))
}

/// Button that reads as "on" (aria-expanded / pressed).
pub fn toggle_button(ui: &mut Ui, text: impl Into<String>, on: bool) -> Response {
    if !on {
        return button(ui, text);
    }
    ui.add(
        egui::Button::new(RichText::new(text.into()).font(sans(12.0)).color(css::ACCENT))
            .fill(css::ACCENT_SOFT)
            .stroke(Stroke::new(1.0, Color32::from_rgba_unmultiplied(138, 169, 255, 102)))
            .corner_radius(6)
            .min_size(Vec2::new(0.0, 26.0)),
    )
}

pub fn primary(ui: &mut Ui, text: impl Into<String>) -> Response {
    ui.add(egui::Button::new(RichText::new(text.into()).font(bold(12.5)).color(css::INK)).fill(css::ACCENT).stroke(Stroke::NONE).corner_radius(6).min_size(Vec2::new(0.0, 28.0)))
}

pub fn primary_enabled(ui: &mut Ui, enabled: bool, text: impl Into<String>) -> Response {
    ui.add_enabled(enabled, egui::Button::new(RichText::new(text.into()).font(bold(12.5)).color(css::INK)).fill(css::ACCENT).stroke(Stroke::NONE).corner_radius(6).min_size(Vec2::new(0.0, 28.0)))
}

pub fn warn_button(ui: &mut Ui, text: impl Into<String>) -> Response {
    ui.add(egui::Button::new(RichText::new(text.into()).font(sans(12.5)).color(css::WARN)).fill(Color32::from_rgba_unmultiplied(255, 191, 105, 20)).stroke(Stroke::new(1.0, css::WARN)).corner_radius(6).min_size(Vec2::new(0.0, 26.0)))
}

/// Text-only accent link.
pub fn link(ui: &mut Ui, text: impl Into<String>) -> Response {
    link_sized(ui, text, 12.0)
}

pub fn link_sized(ui: &mut Ui, text: impl Into<String>, size: f32) -> Response {
    let r = ui.add(egui::Label::new(RichText::new(text.into()).font(sans(size)).color(css::ACCENT)).sense(Sense::click()));
    if r.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    r
}

pub fn colored_link(ui: &mut Ui, text: impl Into<String>, color: Color32, size: f32) -> Response {
    let r = ui.add(egui::Label::new(RichText::new(text.into()).font(sans(size)).color(color)).sense(Sense::click()));
    if r.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    r
}

/// Small square icon button (×, ‹, ›).
pub fn icon_button(ui: &mut Ui, text: &str) -> Response {
    ui.add(egui::Button::new(RichText::new(text).font(sans(16.0)).color(css::DIM)).frame(false).min_size(Vec2::splat(26.0)))
}

/// Flat header button (pane headers).
pub fn flat_button(ui: &mut Ui, text: impl Into<String>) -> Response {
    ui.add(egui::Button::new(RichText::new(text.into()).font(sans(12.0)).color(css::DIM)).frame(false).min_size(Vec2::new(0.0, 26.0)))
}

fn spaced(text: &str, font: FontId, color: Color32, spacing: f32) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.append(text, 0.0, TextFormat { font_id: font, color, extra_letter_spacing: spacing, ..Default::default() });
    job
}

/// Uppercase accent kicker ("DECISION 1 OF 1").
pub fn kicker(ui: &mut Ui, text: &str) -> Response {
    ui.label(spaced(&text.to_uppercase(), bold(10.5), css::ACCENT, 1.05))
}

pub fn kicker_colored(ui: &mut Ui, text: &str, color: Color32) -> Response {
    ui.label(spaced(&text.to_uppercase(), bold(10.5), color, 1.05))
}

/// Uppercase faint section heading (`.hc-h`).
pub fn section(ui: &mut Ui, text: &str) -> Response {
    ui.label(spaced(&text.to_uppercase(), bold(10.5), css::FAINT, 0.84))
}

pub fn dim(ui: &mut Ui, text: impl Into<String>) -> Response {
    ui.label(RichText::new(text.into()).font(sans(12.5)).color(css::DIM))
}

pub fn faint(ui: &mut Ui, text: impl Into<String>) -> Response {
    ui.label(RichText::new(text.into()).font(sans(12.0)).color(css::FAINT))
}

pub fn small_faint(ui: &mut Ui, text: impl Into<String>) -> Response {
    ui.label(RichText::new(text.into()).font(sans(11.0)).color(css::FAINT))
}

pub fn text(ui: &mut Ui, text: impl Into<String>, size: f32, color: Color32) -> Response {
    ui.label(RichText::new(text.into()).font(sans(size)).color(color))
}

/// Wrapping label.
pub fn para(ui: &mut Ui, text: impl Into<String>, size: f32, color: Color32) -> Response {
    ui.add(egui::Label::new(RichText::new(text.into()).font(sans(size)).color(color)).wrap())
}

/// Inline rich text: (text, font, colour) runs, wrapping.
pub fn rich(ui: &mut Ui, runs: &[(&str, FontId, Color32)]) -> Response {
    let mut job = LayoutJob::default();
    for (t, f, c) in runs {
        job.append(t, 0.0, TextFormat { font_id: f.clone(), color: *c, ..Default::default() });
    }
    job.wrap.max_width = ui.available_width();
    ui.add(egui::Label::new(job).wrap())
}

/// A raised floating panel (`--panel-solid`, `--line-strong`, `--shadow`).
pub fn panel_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(css::PANEL_SOLID)
        .stroke(Stroke::new(1.0, css::LINE_STRONG))
        .corner_radius(13)
        .inner_margin(Margin::same(0))
        .shadow(egui::Shadow { offset: [0, 18], blur: 48, spread: 0, color: Color32::from_black_alpha(107) })
}

pub fn card_frame(fill: Color32, stroke: Stroke, radius: u8, margin: Margin) -> egui::Frame {
    egui::Frame::new().fill(fill).stroke(stroke).corner_radius(CornerRadius::same(radius)).inner_margin(margin)
}

/// A pill chip (`.preview-tag`, `.branch-tag`).
pub fn pill(ui: &mut Ui, text: impl Into<String>, color: Color32, border: Color32, fill: Color32, font: FontId) -> Response {
    egui::Frame::new()
        .fill(fill)
        .stroke(Stroke::new(1.0, border))
        .corner_radius(10)
        .inner_margin(Margin::symmetric(7, 1))
        .show(ui, |ui| ui.label(RichText::new(text.into()).font(font).color(color)))
        .response
}

/// Horizontal rule in the line colour.
pub fn hline(ui: &mut Ui) {
    let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
    ui.painter().hline(r.x_range(), r.center().y, Stroke::new(1.0, css::LINE));
}

/// A single-line text field styled like the old inputs.
pub fn text_field(ui: &mut Ui, value: &mut String, hint: &str, width: f32) -> Response {
    ui.add(egui::TextEdit::singleline(value).hint_text(RichText::new(hint).color(css::FAINT)).font(sans(12.5)).desired_width(width).margin(Margin::symmetric(9, 5)).background_color(css::BG))
}

pub fn text_area(ui: &mut Ui, value: &mut String, hint: &str, rows: usize) -> Response {
    ui.add(egui::TextEdit::multiline(value).hint_text(RichText::new(hint).color(css::FAINT)).font(sans(12.5)).desired_width(f32::INFINITY).desired_rows(rows).margin(Margin::symmetric(9, 7)).background_color(css::BG))
}

/// A segmented control (`.ex-seg`): returns the clicked value.
pub fn segmented<T: PartialEq + Clone>(ui: &mut Ui, value: &T, options: &[(T, &str)]) -> Option<T> {
    let mut out = None;
    egui::Frame::new().fill(css::BG).stroke(Stroke::new(1.0, css::LINE)).corner_radius(8).inner_margin(Margin::same(2)).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for (v, label) in options {
                let on = v == value;
                let b = egui::Button::new(RichText::new(*label).font(sans(12.0)).color(if on { css::TEXT } else { css::DIM }))
                    .fill(if on { css::RAISED2 } else { Color32::TRANSPARENT })
                    .stroke(Stroke::NONE)
                    .corner_radius(6)
                    .min_size(Vec2::new(0.0, 26.0));
                if ui.add(b).clicked() {
                    out = Some(v.clone());
                }
            }
        });
    });
    out
}

/// Ellipsis-truncate to `max` chars.
pub fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        s.chars().take(max.saturating_sub(1)).collect::<String>() + "…"
    }
}
