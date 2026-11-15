//! Time travel: scrub through the design's history, restore or fork.

use super::theme::{css, hex, sans};
use super::widgets::{self, button, icon_button, link, primary_enabled};
use crate::backend::Backend;
use crate::history::Entry;
use egui::{Margin, RichText, Stroke, Ui};

#[derive(Default)]
pub struct TimelineUi {
    pub entries: Option<Vec<Entry>>,
    pub selected: Option<u64>,
    pub error: String,
    pub fork: Option<String>,
    pub pending: bool,
    /// snapshot rev the list was loaded for
    pub loaded_rev: Option<u64>,
    /// revision currently shown as preview
    pub shown: Option<u64>,
    pub loading: bool,
}

pub enum TimelineOut {
    Close,
    /// show a past revision (None: back to the live design)
    Preview(Option<u64>),
}

pub fn reload(st: &mut TimelineUi, backend: &Backend, rev: u64) {
    if st.loaded_rev == Some(rev) || st.loading {
        return;
    }
    st.loading = true;
    st.error.clear();
    backend.call(
        |s, _| Ok(crate::commands::history_list(s)),
        move |app, r: Result<Vec<Entry>, String>| {
            let t = &mut app.timeline_ui;
            t.loading = false;
            t.loaded_rev = Some(rev);
            match r {
                Ok(e) => {
                    t.selected = match t.selected {
                        Some(cur) if e.iter().any(|x| x.rev == cur) => Some(cur),
                        _ => e.last().map(|x| x.rev),
                    };
                    t.entries = Some(e);
                }
                Err(e) => t.error = e,
            }
        },
    );
}

pub fn show(ui: &mut Ui, st: &mut TimelineUi, backend: &Backend) -> Option<TimelineOut> {
    let mut out = None;
    widgets::panel_frame().inner_margin(Margin::symmetric(12, 10)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 6.0;
        let i = match (&st.entries, st.selected) {
            (Some(e), Some(s)) => e.iter().position(|x| x.rev == s),
            _ => None,
        };
        let n = st.entries.as_ref().map(|e| e.len()).unwrap_or(0);
        let latest = i.is_some_and(|i| i + 1 == n);
        ui.horizontal(|ui| {
            widgets::kicker(ui, "Timeline");
            ui.add_space(8.0);
            if st.entries.is_none() && st.error.is_empty() {
                ui.spinner();
                ui.label(RichText::new("Loading history…").font(sans(12.0)).color(css::DIM));
            }
            if n == 0 && st.entries.is_some() {
                ui.label(RichText::new("No history yet. Changes appear here after you edit the design.").font(sans(12.0)).color(css::DIM));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icon_button(ui, "×").on_hover_text("Close timeline").clicked() {
                    out = Some(TimelineOut::Close);
                }
                if n > 0 {
                    let idx = i.unwrap_or(0);
                    ui.label(RichText::new(format!("{} / {n}", idx + 1)).font(sans(12.0)).color(css::DIM));
                    if ui.add_enabled(!latest, egui::Button::new(RichText::new("›").font(sans(16.0)).color(css::DIM)).frame(false)).on_hover_text("Next revision").clicked() {
                        st.selected = st.entries.as_ref().map(|e| e[(idx + 1).min(n - 1)].rev);
                    }
                    let mut v = idx as f32;
                    ui.spacing_mut().slider_width = (ui.available_width() - 40.0).max(80.0);
                    if ui.add(egui::Slider::new(&mut v, 0.0..=(n - 1).max(1) as f32).step_by(1.0).show_value(false)).changed() {
                        st.selected = st.entries.as_ref().map(|e| e[(v.round() as usize).min(n - 1)].rev);
                    }
                    if ui.add_enabled(idx > 0, egui::Button::new(RichText::new("‹").font(sans(16.0)).color(css::DIM)).frame(false)).on_hover_text("Previous revision").clicked() {
                        st.selected = st.entries.as_ref().map(|e| e[idx.saturating_sub(1)].rev);
                    }
                }
            });
        });
        if !st.error.is_empty() {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("Couldn't load history: {}", st.error)).font(sans(11.5)).color(hex(0xffb0b8)));
                if link(ui, "Retry").clicked() {
                    st.loaded_rev = None;
                    st.error.clear();
                }
            });
        }
        if let (Some(entries), Some(i)) = (&st.entries, i) {
            let e = &entries[i];
            ui.horizontal(|ui| {
                let ac = match e.actor.as_str() {
                    "agent" | "examiner" => css::GHOST,
                    "user" => css::ACCENT,
                    _ => css::DIM,
                };
                egui::Frame::new().stroke(Stroke::new(1.0, ac)).corner_radius(5).inner_margin(Margin::symmetric(6, 0)).show(ui, |ui| {
                    ui.label(RichText::new(if e.actor.is_empty() { "—".to_uppercase() } else { e.actor.to_uppercase() }).font(sans(10.0)).color(ac));
                });
                ui.add(egui::Label::new(RichText::new(&e.summary).font(sans(12.5)).color(css::TEXT)).truncate());
                if e.at > 0 {
                    ui.label(RichText::new(super::format_time(e.at)).font(sans(11.0)).color(css::FAINT));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if latest {
                        ui.label(RichText::new("Current").font(sans(11.5)).color(css::GOOD));
                        return;
                    }
                    let rev = e.rev;
                    match st.fork.as_mut() {
                        None => {
                            if ui.add_enabled(!st.pending, egui::Button::new(RichText::new("Fork…").font(sans(12.0))).fill(css::RAISED).stroke(Stroke::new(1.0, css::LINE)).corner_radius(6)).clicked() {
                                st.fork = Some(String::new());
                            }
                        }
                        Some(name) => {
                            let mut cancel = false;
                            if button(ui, "Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                                cancel = true;
                            }
                            let can = !name.trim().is_empty() && !st.pending;
                            let mut go = primary_enabled(ui, can, "Create").clicked();
                            let r = ui.add(egui::TextEdit::singleline(name).hint_text(RichText::new("Branch name").color(css::FAINT)).font(sans(12.0)).desired_width(140.0).margin(Margin::symmetric(6, 3)).background_color(css::BG));
                            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && can {
                                go = true;
                            }
                            if go {
                                let n = name.trim().to_string();
                                st.pending = true;
                                backend.call(move |s, _| crate::commands::history_fork(s, rev, n), |app, r| {
                                    app.timeline_ui.pending = false;
                                    match r {
                                        Ok(()) => app.timeline_ui.fork = None,
                                        Err(e) => app.toast("error", e),
                                    }
                                });
                            }
                            if cancel {
                                st.fork = None;
                            }
                        }
                    }
                    let restore = ui.add_enabled(!st.pending, egui::Button::new(RichText::new("Restore as proposals").font(sans(12.0))).fill(css::RAISED).stroke(Stroke::new(1.0, css::LINE)).corner_radius(6));
                    if restore.on_hover_text("Propose the changes that bring the design back to this state").clicked() {
                        st.pending = true;
                        backend.call(move |s, _| crate::commands::history_restore(s, rev), |app, r| {
                            app.timeline_ui.pending = false;
                            match r {
                                Ok(()) => app.close_timeline(),
                                Err(e) => app.toast("error", e),
                            }
                        });
                    }
                });
            });
        }
        // preview follows the selection
        let want = match (i, latest) {
            (Some(_), true) | (None, _) => None,
            (Some(_), false) => st.selected,
        };
        if want != st.shown {
            st.shown = want;
            out = out.take().or(Some(TimelineOut::Preview(want)));
        }
    });
    out
}
