//! Time travel (scrub through the design's history, restore or fork) and the design ↔ code drift
//! report.

use super::theme::{bold, css, hex, mono, sans};
use super::widgets::{self, button, icon_button, link, primary_enabled};
use crate::backend::Backend;
use crate::commands::DriftReport;
use crate::history::Entry;
use egui::{Color32, Margin, RichText, Stroke, Ui, Vec2};

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

#[derive(Default)]
pub struct DriftUi;

pub enum DriftOut {
    Close,
    Reimport,
    Find(String),
}

/// The drift panel (left, like the brief).
pub fn drift_panel(ctx: &egui::Context, area: egui::Rect, report: &DriftReport, backend: &Backend) -> Option<DriftOut> {
    let mut out = None;
    let d = &report.drift;
    let f = &report.findings;
    let pct = (d.conformance * 100.0).round() as i64;
    let top = egui::pos2(area.left() + 10.0, area.top() + 124.0);
    let w = (area.width() - 20.0).min(360.0);
    let h = area.bottom() - 10.0 - top.y;
    egui::Area::new(egui::Id::unique("drift-panel")).order(egui::Order::Foreground).fixed_pos(top).show(ctx, |ui| {
        widgets::panel_frame().show(ui, |ui| {
            ui.set_width(w);
            ui.set_height(h);
            egui::Frame::new().inner_margin(Margin { left: 16, right: 12, top: 14, bottom: 8 }).show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    widgets::kicker(ui, "design ↔ code");
                    let col = if pct < 60 {
                        css::BAD
                    } else if pct < 85 {
                        css::WARN
                    } else {
                        css::GOOD
                    };
                    ui.label(RichText::new(format!("{pct}% conformant")).font(bold(11.5)).color(col)).on_hover_text("share of the design found in the code");
                    ui.label(RichText::new(format!("design {} · code {}", f.design_health, f.code_health)).font(sans(11.0)).color(css::FAINT)).on_hover_text("health of the design and of the code, judged the same way");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if link(ui, "close").clicked() {
                            out = Some(DriftOut::Close);
                        }
                    });
                });
            });
            widgets::hline(ui);
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                egui::Frame::new().inner_margin(Margin { left: 16, right: 16, top: 12, bottom: 16 }).show(ui, |ui| {
                    ui.set_width(w - 32.0);
                    ui.spacing_mut().item_spacing.y = 5.0;
                    let found: std::cell::RefCell<Option<String>> = std::cell::RefCell::new(None);
                    let find = |ui: &mut Ui, n: &str| {
                        if ui.add(egui::Label::new(RichText::new(n).font(mono(12.0)).color(css::ACCENT)).sense(egui::Sense::click())).clicked() {
                            *found.borrow_mut() = Some(n.to_string());
                        }
                    };
                    if let Some(dir) = &report.code_dir {
                        ui.horizontal(|ui| {
                            let short: Vec<&str> = dir.rsplit('/').take(3).collect::<Vec<_>>().into_iter().rev().collect();
                            ui.label(RichText::new(short.join("/")).font(mono(11.0)).color(css::FAINT)).on_hover_text(dir);
                            if link(ui, "re-import").clicked() {
                                out = Some(DriftOut::Reimport);
                            }
                        });
                    }
                    let clean = d.missing_in_code.len() + d.extra_in_code.len() + d.members.len() + d.relations_missing_in_code.len() + d.relations_extra_in_code.len() + d.packages.len() + d.rule_violations_in_code.len() == 0;
                    if clean {
                        widgets::faint(ui, "The code matches the design.");
                    }
                    let sec = |ui: &mut Ui, title: &str, n: usize| {
                        ui.add_space(12.0);
                        widgets::section(ui, &format!("{title} · {n}"));
                    };
                    if !d.missing_in_code.is_empty() {
                        sec(ui, "designed, not in code", d.missing_in_code.len());
                        ui.horizontal_wrapped(|ui| d.missing_in_code.iter().for_each(|n| find(ui, n)));
                    }
                    if !d.extra_in_code.is_empty() {
                        sec(ui, "only in code", d.extra_in_code.len());
                        ui.horizontal_wrapped(|ui| d.extra_in_code.iter().for_each(|n| find(ui, n)));
                    }
                    if !d.members.is_empty() {
                        sec(ui, "members differ", d.members.len());
                        for m in &d.members {
                            find(ui, &m.class);
                            if !m.missing_in_code.is_empty() {
                                widgets::para(ui, format!("missing: {}", m.missing_in_code.join(", ")), 11.5, css::CON);
                            }
                            if !m.extra_in_code.is_empty() {
                                widgets::para(ui, format!("extra: {}", m.extra_in_code.join(", ")), 11.5, css::FAINT);
                            }
                        }
                    }
                    if !d.relations_missing_in_code.is_empty() {
                        sec(ui, "relations missing in code", d.relations_missing_in_code.len());
                        for r in &d.relations_missing_in_code {
                            ui.label(RichText::new(format!("{} → {} ({})", r.from, r.to, r.kind.as_str())).font(mono(11.0)).color(css::TEXT));
                        }
                    }
                    if !d.relations_extra_in_code.is_empty() {
                        sec(ui, "relations only in code", d.relations_extra_in_code.len());
                        for r in &d.relations_extra_in_code {
                            ui.label(RichText::new(format!("{} → {} ({})", r.from, r.to, r.kind.as_str())).font(mono(11.0)).color(css::FAINT));
                        }
                    }
                    if !d.packages.is_empty() {
                        sec(ui, "in another package", d.packages.len());
                        for p in &d.packages {
                            ui.horizontal_wrapped(|ui| {
                                find(ui, &p.class);
                                widgets::rich(ui, &[(" design ", sans(11.5), css::TEXT), (p.design.as_deref().unwrap_or("—"), bold(11.5), css::TEXT), (", code ", sans(11.5), css::TEXT), (p.code.as_deref().unwrap_or("—"), bold(11.5), css::TEXT)]);
                            });
                        }
                    }
                    if !d.rule_violations_in_code.is_empty() {
                        sec(ui, "rules broken in code", d.rule_violations_in_code.len());
                        for v in &d.rule_violations_in_code {
                            widgets::para(ui, v, 11.5, css::BAD);
                        }
                    }
                    if !f.disagreements.is_empty() {
                        sec(ui, "code contradicts the design", f.disagreements.len());
                        for x in &f.disagreements {
                            find(ui, &x.class);
                            widgets::para(ui, format!("design: {}", x.design), 11.5, css::FAINT);
                            widgets::para(ui, format!("code: {}", x.code), 11.5, css::BAD);
                        }
                    }
                    if !f.clones.is_empty() {
                        sec(ui, "same code in several places", f.clones.len());
                        for p in &f.clones {
                            widgets::rich(ui, &[(&format!("{}() ≈ {}() ", p.a, p.b), mono(11.0), css::TEXT), (&format!("{}%", (p.code * 100.0).round()), sans(11.0), css::FAINT)]);
                        }
                    }
                    if !f.issues.is_empty() {
                        sec(ui, "issues only the code has", f.issues.len());
                        for x in f.issues.iter().take(30) {
                            let tip = format!("{}{}", x.explain, x.fix_hint.as_ref().map(|h| format!("\n→ {h}")).unwrap_or_default());
                            widgets::rich(ui, &[(&format!("{} ", x.kind.replace('_', " ")), sans(11.5), css::FAINT), (&x.message, sans(11.5), css::TEXT)]).on_hover_text(tip);
                        }
                    }
                    if let Some(n) = found.borrow_mut().take() {
                        out = Some(DriftOut::Find(n));
                    }
                    ui.add_space(10.0);
                    ui.horizontal_wrapped(|ui| {
                        let can = d.extra_in_code.len() + d.members.len() + d.relations_extra_in_code.len() > 0;
                        if primary_enabled(ui, can, "adopt code into design").clicked() {
                            backend.call(|s, _| crate::commands::drift_adopt(s), |app, r| match r {
                                Ok(()) => app.toast("info", "code-only parts proposed as ghosts"),
                                Err(e) => app.toast("error", e),
                            });
                        }
                        if link(ui, "copy code to-do list").clicked() {
                            ui.ctx().copy_text(report.checklist.clone());
                            backend.events.send(crate::events::UiEvent::Toast { kind: "info".into(), text: "checklist copied (markdown)".into() });
                        }
                    });
                });
            });
        });
    });
    let _ = (Color32::WHITE, Vec2::ZERO);
    out
}
