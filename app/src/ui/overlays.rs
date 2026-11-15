//! Small floating overlays: prune feedback, inline rename, code view, command palette.

use super::state::commands::{matches, normalize, strip_prefix, COMMANDS};
use super::theme::{bold, css, hex, mono, sans};
use super::widgets::{self, button, icon_button, primary_enabled};
use egui::{Align2, Color32, Key, Margin, Pos2, RichText, Stroke, Vec2};
use spide_core::Target;

pub fn target_label(t: &Target) -> String {
    match t {
        Target::Class { name } => name.clone(),
        Target::Attribute { class, name } => format!("{class}.{name}"),
        Target::Method { class, name } => format!("{class}.{name}()"),
        Target::Relation { id } => id.clone(),
        Target::Package { name } => format!("package {name}"),
        Target::Actor { name } => format!("actor {name}"),
        Target::UseCase { name } => name.clone(),
        Target::Sequence { name } => format!("sequence {name}"),
        Target::Message { sequence, index } => format!("{sequence} #{index}"),
        Target::Proposal { id } => format!("proposal {id}"),
        Target::Force { name } => name.clone(),
        Target::System { name } => name.clone(),
        Target::Container { name } => name.clone(),
        Target::Link { id } => id.clone(),
        Target::Concept { name } => format!("concept {name}"),
        Target::ConceptLink { id } => id.clone(),
    }
}

pub struct PruneState {
    pub targets: Vec<Target>,
    pub at: Pos2,
    pub text: String,
    pub focused: bool,
}

pub enum PruneAction {
    Send(String),
    Close,
    Undo,
}

/// After a prune: "what didn't you like / what do you want instead?"
pub fn feedback(ctx: &egui::Context, p: &mut PruneState) -> Option<PruneAction> {
    let mut out = None;
    let w = 340.0;
    let screen = ctx.content_rect();
    let left = (p.at.x - w / 2.0).max(16.0).min(screen.right() - w - 16.0);
    let top = (p.at.y + 14.0).min(screen.bottom() - 220.0);
    egui::Area::new(egui::Id::unique("prune-feedback")).order(egui::Order::Foreground).fixed_pos(Pos2::new(left, top)).show(ctx, |ui| {
        egui::Frame::new()
            .fill(css::PANEL_SOLID)
            .stroke(Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 116, 130, 102)))
            .corner_radius(13)
            .inner_margin(Margin::same(12))
            .shadow(egui::Shadow { offset: [0, 14], blur: 50, spread: 0, color: Color32::from_black_alpha(128) })
            .show(ui, |ui| {
                ui.set_width(w - 24.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("✂").font(sans(12.0)).color(css::BAD));
                    let labels: Vec<String> = p.targets.iter().map(target_label).collect();
                    let what = if labels.len() > 3 { format!("{} +{}", labels[..3].join(", "), labels.len() - 3) } else { labels.join(", ") };
                    ui.add(egui::Label::new(RichText::new(what).font(sans(12.0)).color(css::DIM).strikethrough()).truncate());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if button(ui, "Undo").clicked() {
                            out = Some(PruneAction::Undo);
                        }
                    });
                });
                ui.add_space(6.0);
                let r = ui.add(
                    egui::TextEdit::multiline(&mut p.text)
                        .hint_text(RichText::new("What didn't you like? What do you want instead?").color(css::FAINT))
                        .font(sans(13.0))
                        .desired_rows(2)
                        .desired_width(f32::INFINITY)
                        .margin(Margin::symmetric(9, 7))
                        .background_color(css::BG)
                        .return_key(egui::KeyboardShortcut::new(egui::Modifiers::SHIFT, Key::Enter)),
                );
                if !p.focused {
                    p.focused = true;
                    r.request_focus();
                }
                if r.has_focus() && ui.input(|i| i.key_pressed(Key::Enter) && !i.modifiers.shift) {
                    let t = p.text.trim().to_string();
                    out = Some(if t.is_empty() { PruneAction::Close } else { PruneAction::Send(t) });
                }
                if ui.input(|i| i.key_pressed(Key::Escape)) {
                    out = Some(PruneAction::Close);
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.add(egui::Label::new(RichText::new("Pruned elements stay hidden; Undo restores them.").font(sans(10.5)).color(css::FAINT)).wrap());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let t = p.text.trim().to_string();
                        if primary_enabled(ui, !t.is_empty(), "Tell pi").clicked() {
                            out = Some(PruneAction::Send(t));
                        }
                        if button(ui, "Done").clicked() {
                            out = Some(PruneAction::Close);
                        }
                    });
                });
            });
    });
    out
}

pub struct InlineEdit {
    pub at: Pos2,
    pub label: String,
    pub original: String,
    pub value: String,
    pub focused: bool,
    /// what to do with the new name
    pub kind: InlineKind,
}

#[derive(Clone)]
pub enum InlineKind {
    RenameClass(String),
    RenameAttribute(String, String),
    RenameMethod(String, String),
}

pub enum InlineAction {
    Submit(String),
    Close,
}

/// A small floating text field for direct edits (rename a class or member).
pub fn inline_edit(ctx: &egui::Context, s: &mut InlineEdit) -> Option<InlineAction> {
    let mut out = None;
    let screen = ctx.content_rect();
    let pos = Pos2::new((s.at.x - 20.0).clamp(8.0, screen.right() - 270.0), (s.at.y - 18.0).clamp(38.0, screen.bottom() - 90.0));
    egui::Area::new(egui::Id::unique("inline-edit")).order(egui::Order::Foreground).fixed_pos(pos).show(ctx, |ui| {
        egui::Frame::new()
            .fill(css::PANEL_SOLID)
            .stroke(Stroke::new(1.0, Color32::from_rgba_unmultiplied(138, 169, 255, 153)))
            .corner_radius(9)
            .inner_margin(Margin::symmetric(10, 8))
            .shadow(egui::Shadow { offset: [0, 10], blur: 40, spread: 0, color: Color32::from_black_alpha(128) })
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 5.0;
                ui.label(RichText::new(&s.label).font(sans(11.0)).color(css::DIM));
                let r = ui.add(egui::TextEdit::singleline(&mut s.value).font(mono(13.0)).desired_width(220.0).margin(Margin::symmetric(6, 4)).background_color(css::BG));
                if !s.focused {
                    s.focused = true;
                    r.request_focus();
                    if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), r.id) {
                        let ccursor = egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(s.value.chars().count()));
                        state.cursor.set_char_range(Some(ccursor));
                        state.store(ui.ctx(), r.id);
                    }
                }
                let t = s.value.trim().to_string();
                ui.label(RichText::new(if t.is_empty() { "Name is required" } else { "Enter to save · Esc to cancel" }).font(sans(10.5)).color(css::FAINT));
                if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    if !t.is_empty() {
                        out = Some(if t != s.original { InlineAction::Submit(t) } else { InlineAction::Close });
                    }
                } else if ui.input(|i| i.key_pressed(Key::Escape)) || (r.lost_focus() && s.focused) {
                    out = Some(InlineAction::Close);
                }
            });
    });
    out
}


pub struct Palette {
    pub text: String,
    pub error: String,
    pub active: usize,
    pub pending: bool,
    pub focused: bool,
}

impl Palette {
    pub fn new(text: &str) -> Self {
        Self { text: text.into(), error: String::new(), active: 0, pending: false, focused: false }
    }
}

pub enum PaletteAction {
    Run(String),
    Close,
}

/// The `/spide` command palette (pi pane header and ⌘K).
pub fn palette(ui: &mut egui::Ui, p: &mut Palette) -> Option<PaletteAction> {
    let mut out = None;
    widgets::panel_frame().inner_margin(Margin::same(8)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.add_space(6.0);
            widgets::kicker_colored(ui, "Commands", css::DIM);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icon_button(ui, "×").clicked() {
                    out = Some(PaletteAction::Close);
                }
            });
        });
        ui.add_space(4.0);
        let query = strip_prefix(&p.text).to_lowercase();
        let found = matches(&p.text);
        let mut submit = false;
        egui::Frame::new().fill(css::BG).stroke(Stroke::new(1.0, css::LINE)).corner_radius(9).inner_margin(Margin::symmetric(10, 0)).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("⌘").color(css::FAINT));
                let r = ui.add_enabled(
                    !p.pending,
                    egui::TextEdit::singleline(&mut p.text).hint_text(RichText::new("Type an action or command…").color(css::FAINT)).font(sans(13.5)).frame(egui::Frame::NONE).desired_width(f32::INFINITY).margin(Margin::symmetric(0, 9)),
                );
                if !p.focused {
                    p.focused = true;
                    r.request_focus();
                    if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), r.id) {
                        let n = p.text.chars().count();
                        state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(n))));
                        state.store(ui.ctx(), r.id);
                    }
                }
                if r.changed() {
                    p.active = 0;
                    p.error.clear();
                }
                if r.has_focus() {
                    if ui.input(|i| i.key_pressed(Key::ArrowDown)) {
                        p.active = (p.active + 1).min(found.len().saturating_sub(1));
                    }
                    if ui.input(|i| i.key_pressed(Key::ArrowUp)) {
                        p.active = p.active.saturating_sub(1);
                    }
                }
                if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    submit = true;
                    r.request_focus();
                }
                if p.pending {
                    ui.spinner();
                }
            });
        });
        let choose = |p: &mut Palette, c: &super::state::commands::CommandInfo| -> Option<PaletteAction> {
            if c.required {
                if let Some(t) = c.template {
                    p.text = t.into();
                    p.error.clear();
                    p.focused = false;
                    return None;
                }
            }
            match normalize(c.command) {
                Ok(cmd) => Some(PaletteAction::Run(cmd)),
                Err(e) => {
                    p.error = e;
                    None
                }
            }
        };
        if submit {
            let exact = COMMANDS.iter().find(|c| c.command == query);
            if let Some(ex) = exact.filter(|c| c.required && !p.text.ends_with(' ')) {
                if let Some(a) = choose(p, ex) {
                    out = Some(a);
                }
            } else if !query.contains(' ') && exact.is_none() && found.get(p.active).is_some() {
                let c = found[p.active];
                if let Some(a) = choose(p, c) {
                    out = Some(a);
                }
            } else {
                match normalize(&p.text) {
                    Ok(cmd) => out = Some(PaletteAction::Run(cmd)),
                    Err(e) => p.error = e,
                }
            }
        }
        ui.add_space(6.0);
        egui::ScrollArea::vertical().max_height((ui.ctx().content_rect().height() * 0.52).min(420.0)).auto_shrink([false, true]).show(ui, |ui| {
            for (i, c) in found.iter().enumerate() {
                let on = i == p.active;
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 40.0), egui::Sense::click());
                if resp.hovered() {
                    p.active = i;
                }
                if on {
                    ui.painter().rect_filled(rect, 6.0, css::ACCENT_SOFT);
                }
                let pnt = ui.painter();
                pnt.text(Pos2::new(rect.left() + 10.0, rect.top() + 13.0), Align2::LEFT_CENTER, c.label, sans(12.5), css::TEXT);
                let code = format!("/spide {}{}", c.template.unwrap_or(c.command), if c.required { "…" } else { "" });
                let code_w = pnt.text(Pos2::new(rect.right() - 10.0, rect.center().y), Align2::RIGHT_CENTER, &code, mono(11.0), if on { css::ACCENT } else { css::FAINT }).width();
                let help = widgets::clip(c.help, ((rect.width() - code_w - 40.0) / 6.2).max(10.0) as usize);
                pnt.text(Pos2::new(rect.left() + 10.0, rect.top() + 29.0), Align2::LEFT_CENTER, help, sans(11.5), css::FAINT);
                if resp.clicked() && !p.pending {
                    if let Some(a) = choose(p, c) {
                    out = Some(a);
                }
                }
            }
            if found.is_empty() {
                ui.label(RichText::new(format!("No matching action. Press Enter to run “{}” as a command.", p.text.trim())).font(sans(12.0)).color(css::DIM));
            }
        });
        if !p.error.is_empty() {
            ui.label(RichText::new(&p.error).font(sans(11.5)).color(hex(0xffb0b8)));
        }
    });
    if ui.input(|i| i.key_pressed(Key::Escape)) {
        out = Some(PaletteAction::Close);
    }
    let _ = bold;
    out
}
