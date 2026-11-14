//! The brief panel: partner mode, forces (goals, qualities, constraints, expected changes), change
//! scenarios and architecture rules.

use super::theme::{bold, css, hex, mono, sans};
use super::widgets::{self, button, icon_button, link, primary_enabled};
use crate::backend::Backend;
use egui::{Color32, Margin, RichText, Sense, Stroke, Ui, Vec2};
use serde_json::{json, Value};
use spide_core::metrics::Metrics;
use spide_core::model::{Design, Force, ForceKind, Layer, PartnerMode, Status};
use spide_core::rules::{DepRule, Preset, Rules, TypeRule};
use spide_core::Op;
use std::collections::HashMap;

pub fn op(v: Value) -> Op {
    serde_json::from_value(v).expect("valid op")
}

pub const MODES: [(PartnerMode, &str, &str); 4] = [
    (PartnerMode::Examiner, "examiner", "raises decisions; you choose and defend"),
    (PartnerMode::Socratic, "socratic", "only asks questions; you make every change"),
    (PartnerMode::DevilsAdvocate, "devil's advocate", "argues against what you lean towards"),
    (PartnerMode::Sketch, "sketch", "proposes freely; decisions only for big forks"),
];

pub const PRESETS: [(Preset, &str, &str, &str); 5] = [
    (Preset::Clean, "clean", "clean", "ui/infrastructure → application → domain; dependencies point inward"),
    (Preset::Hexagonal, "hexagonal", "hexagonal", "adapters → ports → domain; adapters never depend on each other"),
    (Preset::Layered, "layered", "layered", "ui → application → domain → infrastructure; only downward"),
    (Preset::ModularMonolith, "modular_monolith", "modular monolith", "packages <module>.<layer>; other modules only via <module>.api"),
    (Preset::None, "none", "none", "no layering rules"),
];

const KINDS: [(ForceKind, &str, &str); 4] = [
    (ForceKind::Goal, "Goals", "what the system must achieve"),
    (ForceKind::Quality, "Quality attributes", "e.g. evolvability, latency, testability"),
    (ForceKind::Constraint, "Constraints", "technology, team, budget, regulation"),
    (ForceKind::Change, "Expected changes", "what will change — the design should absorb it"),
];

const PRIO: [&str; 4] = ["", "must", "should", "nice"];

#[derive(Default)]
pub struct BriefUi {
    add_text: HashMap<&'static str, String>,
    add_error: HashMap<&'static str, String>,
    editing: Option<(String, String, String)>,
    edit_error: String,
    change_text: String,
    change_error: String,
    dep: String,
    dep_error: String,
    ty: String,
    ty_layer: Option<Layer>,
    ty_error: String,
    th: HashMap<&'static str, String>,
}

pub enum BriefOut {
    Close,
    Stress(Option<String>),
}

fn mode_buttons<T: PartialEq + Copy>(ui: &mut Ui, value: T, items: &[(T, &str, &str)]) -> Option<T> {
    let mut out = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
        for (v, label, help) in items {
            let on = *v == value;
            let b = egui::Button::new(RichText::new(*label).font(sans(12.0)).color(if on { css::ACCENT } else { css::DIM }))
                .fill(if on { css::ACCENT_SOFT } else { Color32::TRANSPARENT })
                .stroke(Stroke::new(1.0, if on { Color32::from_rgba_unmultiplied(138, 169, 255, 140) } else { css::LINE }))
                .corner_radius(12)
                .min_size(Vec2::new(0.0, 26.0));
            if ui.add(b).on_hover_text(*help).clicked() {
                out = Some(*v);
            }
        }
    });
    out
}

fn inline_add(ui: &mut Ui, text: &mut String, hint: &str, id: &str) -> bool {
    let mut submit = false;
    ui.horizontal(|ui| {
        let w = ui.available_width() - 56.0;
        let r = ui.add(egui::TextEdit::singleline(text).id_salt(id).hint_text(RichText::new(hint).color(css::FAINT)).font(sans(12.5)).desired_width(w).margin(Margin::symmetric(9, 5)).background_color(css::BG));
        if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            submit = true;
        }
        if ui.add_enabled(!text.trim().is_empty(), egui::Button::new(RichText::new("Add").font(sans(12.0))).fill(css::RAISED).stroke(Stroke::new(1.0, css::LINE)).corner_radius(6).min_size(Vec2::new(0.0, 26.0))).clicked() {
            submit = true;
        }
    });
    submit
}

fn form_error(ui: &mut Ui, e: &str) {
    if !e.is_empty() {
        ui.label(RichText::new(e).font(sans(11.5)).color(hex(0xffb0b8)));
    }
}

/// The brief panel on the left of the canvas.
pub fn panel(ctx: &egui::Context, area: egui::Rect, st: &mut BriefUi, design: &Design, metrics: &Metrics, stress: Option<&str>, backend: &Backend) -> Option<BriefOut> {
    let mut out = None;
    let top = egui::pos2(area.left() + 10.0, area.top() + 124.0);
    let w = (area.width() - 20.0).min(360.0);
    let h = area.bottom() - 10.0 - top.y;
    egui::Area::new(egui::Id::unique("brief-panel")).order(egui::Order::Foreground).fixed_pos(top).show(ctx, |ui| {
        widgets::panel_frame().show(ui, |ui| {
            ui.set_width(w);
            ui.set_height(h);
            egui::Frame::new().inner_margin(Margin { left: 16, right: 12, top: 14, bottom: 8 }).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        widgets::kicker(ui, "Design context");
                        ui.label(RichText::new("Brief & rules").font(bold(15.0)).color(css::TEXT));
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                        if icon_button(ui, "×").on_hover_text("Close · ⌘B").clicked() {
                            out = Some(BriefOut::Close);
                        }
                    });
                });
            });
            widgets::hline(ui);
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                egui::Frame::new().inner_margin(Margin { left: 16, right: 16, top: 12, bottom: 16 }).show(ui, |ui| {
                    ui.set_width(w - 32.0);
                    ui.spacing_mut().item_spacing.y = 5.0;
                    // partner mode
                    widgets::section(ui, "Partner mode");
                    if let Some(mode) = mode_buttons(ui, design.settings.partner, &MODES) {
                        let m = mode.as_str().to_string();
                        backend.run(move |s, _| crate::commands::set_partner(s, m));
                    }
                    if let Some((_, _, help)) = MODES.iter().find(|m| m.0 == design.settings.partner) {
                        widgets::small_faint(ui, *help);
                    }
                    for (kind, title, hint) in KINDS {
                        ui.add_space(13.0);
                        force_group(ui, st, design, kind, title, hint, backend);
                    }
                    if design.forces.is_empty() && !design.settings.brief_skipped {
                        ui.add_space(8.0);
                        widgets::rich(ui, &[("The agent asks for the brief before designing. Answer in pi, add forces here, or type ", sans(11.0), css::FAINT), ("/skip", mono(11.0), css::FAINT), (".", sans(11.0), css::FAINT)]);
                    }
                    ui.add_space(13.0);
                    if let Some(o) = change_section(ui, st, design, metrics, stress, backend) {
                        out = Some(o);
                    }
                    ui.add_space(13.0);
                    rules_section(ui, st, &design.rules, backend);
                });
            });
        });
    });
    out
}

fn force_group(ui: &mut Ui, st: &mut BriefUi, design: &Design, kind: ForceKind, title: &'static str, hint: &str, backend: &Backend) {
    let forces: Vec<&Force> = design.forces.iter().filter(|f| f.kind == kind).collect();
    widgets::section(ui, title);
    if forces.is_empty() {
        widgets::faint(ui, "Nothing yet");
    }
    for f in &forces {
        force_row(ui, st, f, backend);
    }
    let text = st.add_text.entry(title).or_default();
    if inline_add(ui, text, hint, title) {
        let name = text.trim().to_string();
        if name.is_empty() {
            st.add_error.insert(title, "Enter a short description first.".into());
        } else if forces.iter().any(|f| f.name.to_lowercase() == name.to_lowercase()) {
            st.add_error.insert(title, "This item is already in the brief.".into());
        } else {
            st.add_error.remove(title);
            text.clear();
            let k = kind.as_str();
            backend.run(move |s, _| crate::commands::apply_user_ops(s, vec![op(json!({"op": "add_force", "name": name, "kind": k}))]));
        }
    }
    form_error(ui, st.add_error.get(title).map(|s| s.as_str()).unwrap_or(""));
}

fn force_row(ui: &mut Ui, st: &mut BriefUi, f: &Force, backend: &Backend) {
    let ghost = matches!(f.status, Status::Proposed | Status::Removing);
    if ghost {
        egui::Frame::new().fill(Color32::from_rgba_unmultiplied(94, 234, 212, 13)).stroke(Stroke::new(1.0, Color32::from_rgba_unmultiplied(94, 234, 212, 115))).corner_radius(8).inner_margin(Margin::symmetric(6, 4)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new(PRIO[f.priority.min(3) as usize].to_uppercase()).font(sans(10.0)).color(css::FAINT));
                ui.vertical(|ui| {
                    widgets::kicker_colored(ui, if f.status == Status::Removing { "Proposed removal" } else { "Proposed" }, css::GHOST);
                    let t = RichText::new(&f.name).font(sans(12.5)).color(css::TEXT);
                    ui.add(egui::Label::new(if f.status == Status::Removing { t.strikethrough() } else { t }).wrap());
                });
                if let Some(p) = &f.proposal {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if button(ui, "Reject").clicked() {
                            let id = p.clone();
                            backend.run(move |s, _| crate::commands::reject_proposal(s, id, None));
                        }
                        if ui.add(egui::Button::new(RichText::new("Accept").font(sans(12.0)).color(css::GHOST)).fill(css::RAISED).stroke(Stroke::new(1.0, css::LINE)).corner_radius(6)).clicked() {
                            let id = p.clone();
                            backend.run(move |s, _| crate::commands::accept_proposal(s, id, None));
                        }
                    });
                }
            });
            if let Some(d) = &f.description {
                widgets::para(ui, d, 11.5, css::DIM);
            }
        });
        return;
    }
    if let Some((orig, name, desc)) = st.editing.as_mut().filter(|e| e.0 == f.name) {
        let mut done = None;
        egui::Frame::new().fill(css::RAISED).corner_radius(6).inner_margin(Margin::same(8)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 6.0;
            let r = widgets::text_field(ui, name, "Name", f32::INFINITY);
            widgets::text_field(ui, desc, "Description (optional)", f32::INFINITY);
            form_error(ui, &st.edit_error);
            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if primary_enabled(ui, true, "Save").clicked() || enter {
                    done = Some(true);
                }
                if button(ui, "Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    done = Some(false);
                }
            });
        });
        match done {
            Some(true) => {
                let n = name.trim().to_string();
                if n.is_empty() {
                    st.edit_error = "Name is required.".into();
                    return;
                }
                let mut o = json!({"op": "update_force", "name": orig.clone()});
                if n != *orig {
                    o["rename"] = json!(n);
                }
                if desc.trim() != f.description.clone().unwrap_or_default() {
                    o["description"] = json!(desc.trim());
                }
                st.editing = None;
                st.edit_error.clear();
                if o.as_object().unwrap().len() > 2 {
                    backend.run(move |s, _| crate::commands::apply_user_ops(s, vec![op(o)]));
                }
            }
            Some(false) => {
                st.editing = None;
                st.edit_error.clear();
            }
            None => {}
        }
        return;
    }
    let r = ui.horizontal(|ui| {
        let pc = match f.priority {
            1 => css::WARN,
            2 => css::DIM,
            _ => css::FAINT,
        };
        let pr = ui.add_sized(Vec2::new(46.0, 22.0), egui::Button::new(RichText::new(PRIO[f.priority.min(3) as usize].to_uppercase()).font(sans(10.0)).color(pc)).frame(false));
        if pr.on_hover_text("Priority — click to cycle").clicked() {
            let (name, p) = (f.name.clone(), (f.priority % 3) + 1);
            backend.run(move |s, _| crate::commands::apply_user_ops(s, vec![op(json!({"op": "update_force", "name": name, "priority": p}))]));
        }
        ui.add(egui::Label::new(RichText::new(&f.name).font(sans(12.5)).color(css::TEXT)).wrap());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.add(egui::Button::new(RichText::new("✕").font(sans(11.0)).color(css::DIM)).frame(false)).on_hover_text("Remove (⌘Z undoes)").clicked() {
                let name = f.name.clone();
                backend.run(move |s, _| crate::commands::apply_user_ops(s, vec![op(json!({"op": "remove_force", "name": name}))]));
            }
            if ui.add(egui::Button::new(RichText::new("✎").font(sans(11.0)).color(css::DIM)).frame(false)).on_hover_text("Edit").clicked() {
                st.editing = Some((f.name.clone(), f.name.clone(), f.description.clone().unwrap_or_default()));
            }
        });
    });
    if let Some(d) = &f.description {
        ui.horizontal(|ui| {
            ui.add_space(52.0);
            widgets::para(ui, d, 11.5, css::DIM);
        });
    }
    let _ = r;
}

fn change_section(ui: &mut Ui, st: &mut BriefUi, design: &Design, metrics: &Metrics, stress: Option<&str>, backend: &Backend) -> Option<BriefOut> {
    let mut out = None;
    let names: Vec<&str> = design.change_scenarios.iter().map(|c| c.name.as_str()).collect();
    // expected changes from the brief that have no scenario yet
    let seeds: Vec<&Force> = design.forces.iter().filter(|f| f.kind == ForceKind::Change && f.status != Status::Proposed && !names.contains(&f.name.as_str())).collect();
    widgets::section(ui, "Change scenarios · what if…?");
    if metrics.changes.is_empty() && seeds.is_empty() {
        widgets::faint(ui, "Add a likely change to see which classes it touches.");
    }
    for c in &metrics.changes {
        let cs = design.change_scenarios.iter().find(|x| x.name == c.name);
        let pct = (c.locality * 100.0).round() as i64;
        let on = stress == Some(c.name.as_str());
        egui::Frame::new().fill(if on { Color32::from_rgba_unmultiplied(255, 92, 108, 18) } else { Color32::TRANSPARENT }).corner_radius(8).inner_margin(Margin::symmetric(8, 6)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.add(egui::Label::new(RichText::new(&c.name).font(sans(12.5)).color(css::TEXT)).wrap());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if c.stale {
                        let (r, _) = ui.allocate_exact_size(Vec2::splat(6.0), Sense::hover());
                        ui.painter().circle_filled(r.center(), 3.0, css::WARN);
                    }
                    if c.analysed {
                        let col = if pct < 50 {
                            css::BAD
                        } else if pct < 75 {
                            css::WARN
                        } else {
                            css::GOOD
                        };
                        ui.label(RichText::new(format!("{pct}%")).font(bold(11.5)).color(col)).on_hover_text("locality: share of classes untouched");
                    }
                });
            });
            if let Some(d) = cs.and_then(|c| c.description.as_ref()) {
                widgets::para(ui, d, 11.5, css::DIM);
            }
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                if c.analysed {
                    let pk = if c.packages.is_empty() { String::new() } else { format!(" · {} package{}", c.packages.len(), if c.packages.len() > 1 { "s" } else { "" }) };
                    ui.label(RichText::new(format!("{} impacted · {} ripple{pk}", c.impacted.len(), c.ripple.len())).font(sans(11.5)).color(css::DIM));
                    if link(ui, if on { "Stop stress test" } else { "Stress test" }).clicked() {
                        out = Some(BriefOut::Stress(if on { None } else { Some(c.name.clone()) }));
                    }
                } else {
                    ui.label(RichText::new("not analysed yet").font(sans(11.5)).color(css::FAINT));
                }
                if link(ui, if c.analysed { "Re-analyse" } else { "Analyse" }).clicked() {
                    let n = c.name.clone();
                    backend.run(move |s, pi| crate::commands::analyse_change(s, pi, n));
                }
                if ui.add(egui::Button::new(RichText::new("✕").font(sans(11.0)).color(css::DIM)).frame(false)).on_hover_text("Remove (⌘Z undoes)").clicked() {
                    let n = c.name.clone();
                    backend.run(move |s, _| crate::commands::apply_user_ops(s, vec![op(json!({"op": "remove_change_scenario", "name": n}))]));
                }
            });
        });
    }
    for f in &seeds {
        ui.horizontal(|ui| {
            ui.label(RichText::new(&f.name).font(sans(12.5)).color(css::DIM));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if link(ui, "Analyse").clicked() {
                    let (n, d) = (f.name.clone(), f.description.clone());
                    backend.run(move |s, pi| {
                        crate::commands::apply_user_ops(s, vec![op(json!({"op": "add_change_scenario", "name": n, "description": d}))])?;
                        crate::commands::analyse_change(s, pi, n)
                    });
                }
            });
        });
    }
    if inline_add(ui, &mut st.change_text, "What if we must support…?", "change-scenario") {
        let name = st.change_text.trim().to_string();
        if name.is_empty() {
            st.change_error = "Describe a change first.".into();
        } else if names.contains(&name.as_str()) {
            st.change_error = "This scenario already exists. Use Analyse on it below.".into();
        } else {
            st.change_error.clear();
            st.change_text.clear();
            backend.call(
                move |s, pi| {
                    crate::commands::apply_user_ops(s, vec![op(json!({"op": "add_change_scenario", "name": name}))])?;
                    crate::commands::analyse_change(s, pi, name).map_err(|e| format!("analyse: {e}"))
                },
                |app, r| {
                    if let Err(e) = r {
                        app.brief_ui.change_error = if e.starts_with("analyse:") { "Scenario saved, but analysis couldn't start. Use Analyse to retry.".into() } else { "Not saved. Your scenario is kept.".into() };
                    }
                },
            );
        }
    }
    form_error(ui, &st.change_error);
    out
}

fn rules_section(ui: &mut Ui, st: &mut BriefUi, rules: &Rules, backend: &Backend) {
    let set = |r: Rules| {
        backend.run(move |s, _| crate::commands::set_rules(s, r));
    };
    widgets::section(ui, "Architecture style");
    let presets: Vec<(Preset, &str, &str)> = PRESETS.iter().map(|p| (p.0, p.2, p.3)).collect();
    if let Some(p) = mode_buttons(ui, rules.preset, &presets) {
        set(Rules { preset: p, ..rules.clone() });
    }
    if let Some(p) = PRESETS.iter().find(|p| p.0 == rules.preset) {
        widgets::small_faint(ui, p.3);
    }
    ui.add_space(13.0);
    widgets::section(ui, "Forbidden dependencies");
    for (i, r) in rules.forbidden.iter().enumerate() {
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("{} ↛ {}", r.from, r.to)).font(mono(12.0)).color(css::TEXT));
            if let Some(reason) = &r.reason {
                ui.label(RichText::new(format!(" — {reason}")).font(sans(12.0)).color(css::FAINT));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(egui::Button::new(RichText::new("✕").font(sans(11.0)).color(css::DIM)).frame(false)).on_hover_text("Remove").clicked() {
                    let mut f = rules.forbidden.clone();
                    f.remove(i);
                    set(Rules { forbidden: f, ..rules.clone() });
                }
            });
        });
    }
    if inline_add(ui, &mut st.dep, "*Controller -> *Repository : use a service", "rule-dependency") {
        // "from -> to : reason"
        let t = st.dep.trim().replace('→', "->");
        let (lhs, reason) = match t.split_once(':') {
            Some((a, b)) => (a.trim().to_string(), Some(b.trim().to_string()).filter(|s| !s.is_empty())),
            None => (t.clone(), None),
        };
        match lhs.split_once("->").map(|(a, b)| (a.trim(), b.trim())) {
            Some((a, b)) if !a.is_empty() && !b.is_empty() && !a.contains(' ') && !b.contains(' ') => {
                let mut f = rules.forbidden.clone();
                f.push(DepRule { from: a.into(), to: b.into(), reason });
                st.dep.clear();
                st.dep_error.clear();
                set(Rules { forbidden: f, ..rules.clone() });
            }
            _ => st.dep_error = "Use the form From -> To : reason.".into(),
        }
    }
    form_error(ui, &st.dep_error);
    ui.add_space(13.0);
    widgets::section(ui, "Forbidden types per layer");
    for (i, r) in rules.forbidden_types.iter().enumerate() {
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("no {} in {}", r.pattern, spide_diagram::visuals::layer_name(r.layer))).font(mono(12.0)).color(css::TEXT));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(egui::Button::new(RichText::new("✕").font(sans(11.0)).color(css::DIM)).frame(false)).on_hover_text("Remove").clicked() {
                    let mut f = rules.forbidden_types.clone();
                    f.remove(i);
                    set(Rules { forbidden_types: f, ..rules.clone() });
                }
            });
        });
    }
    ui.horizontal(|ui| {
        let layer = st.ty_layer.unwrap_or(Layer::Domain);
        egui::ComboBox::from_id_salt("type-layer").selected_text(spide_diagram::visuals::layer_name(layer)).width(110.0).show_ui(ui, |ui| {
            for l in [Layer::Ui, Layer::Application, Layer::Domain, Layer::Infrastructure] {
                ui.selectable_value(&mut st.ty_layer, Some(l), spide_diagram::visuals::layer_name(l));
            }
        });
        let r = ui.add(egui::TextEdit::singleline(&mut st.ty).hint_text(RichText::new("Http*").color(css::FAINT)).font(sans(12.5)).desired_width(ui.available_width() - 56.0).margin(Margin::symmetric(9, 5)).background_color(css::BG));
        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if ui.add_enabled(!st.ty.trim().is_empty(), egui::Button::new(RichText::new("Add").font(sans(12.0))).fill(css::RAISED).stroke(Stroke::new(1.0, css::LINE)).corner_radius(6).min_size(Vec2::new(0.0, 26.0))).clicked() || enter {
            let p = st.ty.trim().to_string();
            if p.is_empty() {
                st.ty_error = "Enter a type pattern, for example Http*.".into();
            } else {
                let mut f = rules.forbidden_types.clone();
                f.push(TypeRule { layer, pattern: p, reason: None });
                st.ty.clear();
                st.ty_error.clear();
                set(Rules { forbidden_types: f, ..rules.clone() });
            }
        }
    });
    form_error(ui, &st.ty_error);
    ui.add_space(13.0);
    widgets::section(ui, "Thresholds");
    let th = &rules.thresholds;
    let rows: [(&'static str, &str, usize); 4] = [
        ("god_members", "god class: members >", th.god_members),
        ("god_responsibilities", "god class: responsibilities >", th.god_responsibilities),
        ("high_fan_out", "high coupling: fan-out ≥", th.high_fan_out),
        ("max_scenario_hops", "chatty scenario: hops >", th.max_scenario_hops),
    ];
    for (key, label, value) in rows {
        ui.horizontal(|ui| {
            ui.label(RichText::new(label).font(sans(12.5)).color(css::DIM));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let text = st.th.entry(key).or_insert_with(|| value.to_string());
                let valid = text.parse::<usize>().is_ok_and(|v| (1..=99).contains(&v));
                let r = ui.add(egui::TextEdit::singleline(text).font(sans(12.0)).desired_width(52.0).align(egui::Align2::RIGHT_CENTER).margin(Margin::symmetric(6, 3)).background_color(css::BG).text_color(if valid { css::TEXT } else { css::BAD }));
                if r.lost_focus() {
                    if let Ok(v) = text.parse::<usize>() {
                        if (1..=99).contains(&v) && v != value {
                            let mut t = rules.thresholds.clone();
                            match key {
                                "god_members" => t.god_members = v,
                                "god_responsibilities" => t.god_responsibilities = v,
                                "high_fan_out" => t.high_fan_out = v,
                                _ => t.max_scenario_hops = v,
                            }
                            set(Rules { thresholds: t, ..rules.clone() });
                        }
                    }
                    st.th.remove(key);
                }
                if !r.has_focus() && !r.lost_focus() {
                    *st.th.get_mut(key).unwrap() = value.to_string();
                }
            });
        });
    }
}
