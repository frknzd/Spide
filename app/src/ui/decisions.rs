//! Decision inspector (cards, mini previews, defend footer), decision history, impact and the
//! side-by-side comparison.

use super::paint::{AnimClock, EguiMeasure, ScenePainter, View};
use super::state::decision_drafts::Drafts;
use super::theme::{bold, css, hex, mono, sans};
use super::widgets::{self, icon_button, link, primary_enabled, warn_button};
use crate::backend::Backend;
use crate::state::{OptionPreview, Snapshot};
use egui::{Color32, Margin, Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2};
use spide_core::impact::Impact;
use spide_core::model::{Class, Decision, DecisionState, Design, ExaminerMode, Status};
use spide_diagram::class_view::{self, Data, Opts, Routes};
use spide_diagram::layout::{self, Direction, LayoutEdge, LayoutNode};
use spide_diagram::scene::{bbox, Scene};
use std::collections::{HashMap, HashSet};

fn hc(v: f64) -> Color32 {
    super::chrome::hc(v)
}

/// Paint a scene fitted into `rect` (static: previews, comparisons).
pub fn paint_fitted(ui: &mut Ui, rect: Rect, scene: &Scene, m: &EguiMeasure, pad: f64) {
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 8.0, css::BG);
    painter.rect_stroke(rect, 8.0, Stroke::new(1.0, css::LINE), egui::StrokeKind::Inside);
    let Some(b) = bbox(&scene.root, m, 0) else { return };
    let (bw, bh) = (b[2] - b[0] + 2.0 * pad, b[3] - b[1] + 2.0 * pad);
    let k = ((rect.width() as f64 / bw).min(rect.height() as f64 / bh)) as f32;
    let cx = (b[0] + b[2]) / 2.0;
    let cy = (b[1] + b[3]) / 2.0;
    let origin = Pos2::new(rect.center().x - cx as f32 * k, rect.center().y - cy as f32 * k);
    let mut clock = AnimClock::settled();
    let mut sp = ScenePainter { painter: &painter, view: View { origin, k }, now: 0.0, clock: &mut clock, fonts: m };
    sp.paint(&scene.root);
}

/// A static class diagram of `classes`, laid out once.
pub fn static_scene(m: &EguiMeasure, classes: &[Class], design: &Design, highlight: &HashSet<String>, pos: Option<&HashMap<String, [f64; 2]>>, direction: Direction) -> Scene {
    let relations: Vec<_> = design.relations.iter().filter(|r| r.status != Status::Pruned && classes.iter().any(|c| c.name == r.from) && classes.iter().any(|c| c.name == r.to)).cloned().collect();
    let boxes = class_view::boxes_for(m, classes, true, 3, &relations, None, &HashSet::new(), None, None, &HashSet::new());
    let pos = match pos {
        Some(p) => p.clone(),
        None => {
            let nodes: Vec<LayoutNode> = classes.iter().map(|c| LayoutNode { id: c.name.clone(), w: boxes[&c.name].w, h: boxes[&c.name].h, group: None, external: false }).collect();
            let edges: Vec<LayoutEdge> = relations.iter().map(|r| LayoutEdge { from: r.from.clone(), to: r.to.clone(), kind: r.kind.as_str().into() }).collect();
            layout::layered(&nodes, &edges, direction)
        }
    };
    let o = Opts { pos, boxes, show_methods: true, still: true, level: 3, zoom_cap: 4, highlight: Some(highlight), hide_packages: true, ..Default::default() };
    let d = Data { classes, relations: &relations, packages: &design.packages, metrics: None, implicit: &[] };
    class_view::build(m, &d, &o, &mut Routes::default())
}

// ---------------------------------------------------------------- impact

pub fn impact_line(ui: &mut Ui, impact: &Impact, open: &mut bool, expandable: bool) {
    let d = impact.health_after as i64 - impact.health_before as i64;
    let mut bits: Vec<(String, Color32)> = vec![];
    if !impact.added.is_empty() {
        bits.push((format!("+{}", impact.added.len()), css::PRO));
    }
    if !impact.changed.is_empty() {
        bits.push((format!("~{}", impact.changed.len()), css::WARN));
    }
    if !impact.removed.is_empty() {
        bits.push((format!("−{}", impact.removed.len()), css::CON));
    }
    if !impact.new_issues.is_empty() {
        let n = impact.new_issues.len();
        bits.push((format!("{n} new issue{}", if n > 1 { "s" } else { "" }), css::BAD));
    }
    if !impact.resolved_issues.is_empty() {
        bits.push((format!("{} resolved", impact.resolved_issues.len()), css::GOOD));
    }
    if !impact.flipped_deps.is_empty() {
        bits.push((format!("{} flipped", impact.flipped_deps.len()), css::WARN));
    }
    if !impact.rule_breaks.is_empty() {
        bits.push((format!("{} rule ✗", impact.rule_breaks.len()), css::BAD));
    }
    if !impact.affected_scenarios.is_empty() {
        let n = impact.affected_scenarios.len();
        bits.push((format!("{n} scenario{}", if n > 1 { "s" } else { "" }), css::WARN));
    }
    ui.add_space(4.0);
    let r = ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 9.0;
        for (t, c) in &bits {
            ui.label(RichText::new(t).font(sans(11.5)).color(*c));
        }
        if d != 0 {
            ui.label(RichText::new(if d > 0 { format!("▲{d} health") } else { format!("▼{} health", -d) }).font(sans(11.5)).color(hc(impact.health_after as f64)));
        }
        if bits.is_empty() && d == 0 {
            ui.label(RichText::new("no structural change").font(sans(11.5)).color(css::FAINT));
        }
        if expandable {
            ui.label(RichText::new(if *open { "▴" } else { "▾" }).font(sans(11.5)).color(css::FAINT));
        }
    });
    if expandable {
        let resp = ui.interact(r.response.rect, ui.scope_id().with(("impact", impact.health_after, impact.added.len(), impact.changed.len())), Sense::click()).on_hover_text("consequences if chosen");
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if resp.clicked() {
            *open = !*open;
        }
    }
    if *open || !expandable {
        impact_details(ui, impact);
    }
}

pub fn impact_details(ui: &mut Ui, impact: &Impact) {
    ui.add_space(4.0);
    widgets::hline(ui);
    let list = |ui: &mut Ui, title: &str, items: &[String], c: Color32| {
        if items.is_empty() {
            return;
        }
        ui.add_space(4.0);
        widgets::section(ui, title);
        for x in items {
            ui.add(egui::Label::new(RichText::new(x).font(sans(11.5)).color(c)).wrap());
        }
    };
    list(ui, "added", &impact.added, css::PRO);
    list(ui, "changed", &impact.changed, css::WARN);
    list(ui, "removed", &impact.removed, css::CON);
    list(ui, "new issues", &impact.new_issues, css::BAD);
    list(ui, "resolved", &impact.resolved_issues, css::GOOD);
    list(ui, "rules broken", &impact.rule_breaks, css::BAD);
    let flipped: Vec<String> = impact.flipped_deps.iter().map(|(a, b)| format!("{a} → {b} (was {b} → {a})")).collect();
    list(ui, "dependencies flipped", &flipped, css::WARN);
    list(ui, "scenarios to re-check", &impact.affected_scenarios, css::WARN);
    if !impact.class_deltas.is_empty() {
        ui.add_space(4.0);
        widgets::section(ui, "coupling");
        let sign = |n: i64| if n > 0 { format!("+{n}") } else { n.to_string() };
        for c in &impact.class_deltas {
            let heat = if c.heat != 0 { format!(" · heat {}", sign(c.heat)) } else { String::new() };
            ui.label(RichText::new(format!("{}: in {} · out {}{heat}", c.class, sign(c.fan_in), sign(c.fan_out))).font(mono(11.0)).color(css::TEXT));
        }
    }
}

pub fn tradeoff_chips(ui: &mut Ui, o: &spide_core::model::DecisionOption, symbol_only: bool) {
    if o.tradeoffs.is_empty() {
        return;
    }
    ui.add_space(6.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
        for t in &o.tradeoffs {
            let minus = t.effect.trim().starts_with('-');
            let c = if minus { css::CON } else { css::PRO };
            let text = if symbol_only { format!("{} {}", if minus { "−" } else { "+" }, t.force) } else { format!("{} {}", t.effect, t.force) };
            let r = egui::Frame::new().stroke(Stroke::new(1.0, c)).corner_radius(6).inner_margin(Margin::symmetric(6, 0)).show(ui, |ui| ui.label(RichText::new(text).font(sans(10.5)).color(c)));
            if let Some(n) = &t.note {
                r.response.on_hover_text(n);
            }
        }
    });
}

// ---------------------------------------------------------------- decisions

#[derive(Default)]
pub struct DecisionUi {
    pub active: Option<String>,
    pub previewing: Option<String>,
    pub sending: bool,
    pub error: String,
    pub drafts: Drafts,
    pub impact_open: HashSet<String>,
    minis: HashMap<String, Scene>,
    /// (decision, rev, project) the preview belongs to
    preview_key: String,
}

pub enum DecisionOut {
    Preview(Option<Design>),
    Compare(String),
}

pub fn panel(ui: &mut Ui, st: &mut DecisionUi, pending: &[&Decision], snap: &Snapshot, backend: &Backend, m: &EguiMeasure, out: &mut Vec<DecisionOut>) {
    let idx = st.active.as_ref().and_then(|a| pending.iter().position(|d| &d.id == a)).unwrap_or(0);
    let d = pending[idx];
    // previews close on decision/revision changes
    let key = format!("{}:{}:{}", d.id, snap.rev, snap.project_dir);
    if key != st.preview_key {
        st.preview_key = key;
        if st.previewing.take().is_some() {
            out.push(DecisionOut::Preview(None));
        }
        st.minis.clear();
    }
    let full = ui.available_rect_before_wrap();
    let footer_h = 250.0f32.min(full.height() * 0.55);
    // header
    egui::Frame::new().inner_margin(Margin { left: 14, right: 14, top: 14, bottom: 4 }).show(ui, |ui| {
        ui.horizontal(|ui| {
            widgets::kicker(ui, &format!("Decision {} of {}", idx + 1, pending.len()));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if pending.len() > 1 {
                    if icon_button(ui, "›").on_hover_text("Next decision").clicked() {
                        st.active = Some(pending[(idx + 1) % pending.len()].id.clone());
                    }
                    if icon_button(ui, "‹").on_hover_text("Previous decision").clicked() {
                        st.active = Some(pending[(idx + pending.len() - 1) % pending.len()].id.clone());
                    }
                }
                if d.options.len() > 1 && link(ui, "Compare").clicked() {
                    out.push(DecisionOut::Compare(d.id.clone()));
                }
            });
        });
    });
    egui::Frame::new().inner_margin(Margin { left: 14, right: 14, top: 6, bottom: 14 }).show(ui, |ui| {
        ui.add(egui::Label::new(RichText::new(&d.question).font(bold(15.0)).color(css::TEXT)).wrap());
    });
    widgets::hline(ui);
    let evaluating = d.state == DecisionState::Evaluating;
    let rejected = d.state == DecisionState::Rejected;
    let disabled = evaluating || st.sending;
    let mut draft = st.drafts.get(&snap.project_dir, d);
    let before = draft.clone();
    let previews: Vec<&OptionPreview> = snap.previews.iter().filter(|p| p.decision == d.id).collect();
    let health = snap.metrics.health as i64;
    let options_h = (ui.available_height() - footer_h).max(50.0);
    egui::ScrollArea::vertical().id_salt(("options", &d.id)).max_height(options_h).auto_shrink([false, false]).show(ui, |ui| {
        egui::Frame::new().inner_margin(Margin::symmetric(12, 10)).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            if let Some(c) = &d.context {
                widgets::para(ui, c, 12.5, css::DIM);
            }
            for o in &d.options {
                let pv = previews.iter().find(|p| p.option == o.id).copied();
                let selected = !draft.own_mode && draft.choice.as_deref() == Some(&o.id);
                let expanded = draft.expanded.contains(&o.id);
                let previewing = st.previewing.as_deref() == Some(&o.id);
                let (fill, border) = if selected { (Color32::from_rgba_unmultiplied(138, 169, 255, 18), Color32::from_rgba_unmultiplied(138, 169, 255, 178)) } else { (css::SURFACE, css::LINE) };
                let r = egui::Frame::new().fill(fill).stroke(Stroke::new(1.0, border)).corner_radius(9).inner_margin(Margin::symmetric(12, 11)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.add_enabled_ui(!disabled, |ui| {
                        ui.horizontal(|ui| {
                            if ui.radio(selected, "").on_hover_text(format!("Choose {}", o.label)).clicked() {
                                draft.choice = Some(o.id.clone());
                                draft.own_mode = false;
                            }
                            ui.label(RichText::new(&o.id).font(bold(13.0)).color(css::DIM));
                            let health_w = if pv.and_then(|p| p.health).is_some() { 52.0 } else { 0.0 };
                            let lw = (ui.available_width() - health_w).max(40.0);
                            let lr = ui.add_sized(Vec2::new(lw, 18.0), egui::Label::new(RichText::new(&o.label).font(bold(13.5)).color(css::TEXT)).sense(Sense::click()).wrap().halign(egui::Align::Min));
                            if lr.clicked() {
                                draft.choice = Some(o.id.clone());
                                draft.own_mode = false;
                            }
                            if let Some(h) = pv.and_then(|p| p.health) {
                                let delta = h as i64 - health;
                                let mut job = egui::text::LayoutJob::default();
                                job.append(&h.to_string(), 0.0, egui::TextFormat { font_id: bold(12.0), color: hc(h as f64), ..Default::default() });
                                job.append(&format!(" {}{delta}", if delta >= 0 { "+" } else { "" }), 0.0, egui::TextFormat { font_id: sans(11.0), color: hc(h as f64).gamma_multiply(0.8), ..Default::default() });
                                ui.label(job).on_hover_text("Health if chosen (delta from current)");
                            }
                        });
                        if let Some(s) = &o.summary {
                            widgets::para(ui, s, 12.0, css::DIM);
                        }
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 16.0;
                            if widgets::link_sized(ui, if expanded { "Hide details" } else { "Details" }, 11.0).clicked() {
                                if expanded {
                                    draft.expanded.retain(|x| x != &o.id);
                                } else {
                                    draft.expanded.push(o.id.clone());
                                }
                            }
                            let can = pv.is_some_and(|p| p.design.is_some() && p.error.is_none());
                            let label = if previewing { "Stop preview" } else { "Preview" };
                            let pr = ui.add_enabled(can, egui::Label::new(RichText::new(label).font(sans(11.0)).color(if previewing { css::GHOST } else { css::ACCENT })).sense(Sense::click()));
                            if pr.clicked() {
                                if previewing {
                                    st.previewing = None;
                                    out.push(DecisionOut::Preview(None));
                                } else {
                                    st.previewing = Some(o.id.clone());
                                    out.push(DecisionOut::Preview(pv.and_then(|p| p.design.clone())));
                                }
                            }
                        });
                        if let Some(e) = pv.and_then(|p| p.error.as_ref()) {
                            widgets::para(ui, e, 11.0, css::CON);
                        }
                        if expanded {
                            ui.add_space(4.0);
                            widgets::hline(ui);
                            ui.add_space(4.0);
                            mini_preview(ui, st, pv, &d.id, &o.id, m);
                            if let Some(imp) = pv.and_then(|p| p.impact.as_ref()) {
                                let k = format!("{}:{}", d.id, o.id);
                                let mut open = st.impact_open.contains(&k);
                                impact_line(ui, imp, &mut open, true);
                                if open {
                                    st.impact_open.insert(k);
                                } else {
                                    st.impact_open.remove(&k);
                                }
                            }
                            tradeoff_chips(ui, o, false);
                            for x in &o.pros {
                                widgets::para(ui, format!("+ {x}"), 11.5, css::PRO);
                            }
                            for x in &o.cons {
                                widgets::para(ui, format!("− {x}"), 11.5, css::CON);
                            }
                        }
                    });
                });
                if previewing {
                    let rr = r.response.rect;
                    ui.painter().vline(rr.left() + 1.5, rr.y_range().shrink(4.0), Stroke::new(3.0, css::GHOST));
                }
            }
        });
    });
    // footer
    let footer = egui::Frame::new().fill(css::SURFACE).inner_margin(Margin::symmetric(14, 12));
    widgets::hline(ui);
    footer.show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 7.0;
        ui.add_enabled_ui(!disabled, |ui| {
            ui.horizontal(|ui| {
                if ui.radio(draft.own_mode, RichText::new("My own alternative").font(sans(12.0)).color(css::DIM)).clicked() {
                    draft.own_mode = true;
                }
            });
            if draft.own_mode {
                widgets::text_area(ui, &mut draft.custom, "Describe your alternative…", 2);
            } else {
                let sel = d.options.iter().find(|o| Some(&o.id) == draft.choice.as_ref()).map(|o| o.label.clone()).unwrap_or("choose an option above".into());
                ui.label(RichText::new(format!("Selected: {sel}")).font(sans(11.0)).color(css::FAINT));
            }
        });
        if let Some(fb) = d.attempts.last().and_then(|a| a.feedback.as_ref()) {
            let (bg, border, fg) = if rejected { (Color32::from_rgba_unmultiplied(255, 92, 108, 20), Color32::from_rgba_unmultiplied(255, 92, 108, 89), hex(0xffc9cf)) } else { (Color32::from_rgba_unmultiplied(74, 222, 128, 18), Color32::from_rgba_unmultiplied(74, 222, 128, 77), hex(0xc6f6d5)) };
            egui::Frame::new().fill(bg).stroke(Stroke::new(1.0, border)).corner_radius(6).inner_margin(Margin::symmetric(8, 6)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                egui::ScrollArea::vertical().max_height(80.0).id_salt("verdict").show(ui, |ui| {
                    widgets::rich(ui, &[("Examiner: ", bold(12.0), fg), (fb, sans(12.0), fg)]);
                });
            });
        }
        if d.attempts.len() > 1 {
            ui.label(RichText::new(format!("attempt {}", d.attempts.len())).font(sans(11.0)).color(css::FAINT));
        }
        ui.label(RichText::new("Your defence").font(bold(12.0)).color(css::TEXT));
        let hint = if rejected { "Answer the examiner's objection…" } else { "Why is this the right design here? (⌘⏎)" };
        let tr = ui.add_enabled(!disabled, egui::TextEdit::multiline(&mut draft.defense).hint_text(RichText::new(hint).color(css::FAINT)).font(sans(12.5)).desired_rows(3).desired_width(f32::INFINITY).margin(Margin::symmetric(9, 7)).background_color(css::BG));
        let valid_choice = if draft.own_mode { !draft.custom.trim().is_empty() } else { d.options.iter().any(|o| Some(&o.id) == draft.choice.as_ref()) };
        let valid = valid_choice && draft.defense.trim().chars().count() >= 3;
        let mut submit = tr.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter) && i.modifiers.command);
        if !st.error.is_empty() {
            ui.label(RichText::new(&st.error).font(sans(11.5)).color(css::CON));
        }
        ui.horizontal_wrapped(|ui| {
            if evaluating {
                ui.spinner();
                ui.label(RichText::new("Examiner is reading…").font(sans(12.5)).color(css::ACCENT));
            } else {
                let label = if st.sending {
                    "Sending…"
                } else if rejected {
                    "Defend again"
                } else {
                    "Defend"
                };
                if primary_enabled(ui, valid && !disabled, label).clicked() {
                    submit = true;
                }
                if rejected && snap.view.settings.examiner == ExaminerMode::Lenient && ui.add_enabled_ui(!disabled, |ui| warn_button(ui, "Overrule ⚠")).inner.on_hover_text("Apply anyway; marked as overruled").clicked() {
                    st.sending = true;
                    st.error.clear();
                    let id = d.id.clone();
                    backend.call(move |s, _| crate::commands::overrule(s, id), |app, r| app.decisions_done(r.err()));
                }
                if ui.add_enabled(!disabled, egui::Label::new(RichText::new("Skip decision").font(sans(12.0)).color(css::ACCENT)).sense(Sense::click())).on_hover_text("Close this decision without applying an option").clicked() {
                    st.sending = true;
                    st.error.clear();
                    let id = d.id.clone();
                    backend.call(move |s, _| crate::commands::dismiss_decision(s, id), |app, r| {
                        if r.is_ok() {
                            app.toast("info", "Decision skipped. ⌘Z restores it.");
                        }
                        app.decisions_done(r.err());
                    });
                }
            }
        });
        if submit && valid && !disabled {
            st.sending = true;
            st.error.clear();
            let (id, option, custom, defense) = (d.id.clone(), if draft.own_mode { None } else { draft.choice.clone() }, if draft.own_mode { Some(draft.custom.trim().to_string()) } else { None }, draft.defense.trim().to_string());
            backend.call(move |s, pi| crate::commands::submit_defense(s, pi, id, option, custom, defense), |app, r| app.decisions_done(r.err()));
        }
    });
    if draft != before {
        st.drafts.save(&snap.project_dir, &d.id, draft);
    }
}

pub(super) fn mini_preview(ui: &mut Ui, st: &mut DecisionUi, pv: Option<&OptionPreview>, decision: &str, option: &str, m: &EguiMeasure) {
    let w = ui.available_width();
    let box_msg = |ui: &mut Ui, text: &str, color: Color32, h: f32| {
        egui::Frame::new().fill(css::BG).stroke(Stroke::new(1.0, css::LINE)).corner_radius(8).inner_margin(Margin::same(8)).show(ui, |ui| {
            ui.set_width(w - 16.0);
            ui.set_min_height(h);
            ui.centered_and_justified(|ui| ui.label(RichText::new(text).font(sans(11.5)).color(color)));
        });
    };
    if let Some(e) = pv.and_then(|p| p.error.as_ref()) {
        box_msg(ui, e, hex(0xffb3ba), 30.0);
        return;
    }
    let Some(d) = pv.and_then(|p| p.design.as_ref()) else {
        box_msg(ui, "Preview unavailable for this option.", css::DIM, 74.0);
        return;
    };
    let key = format!("{decision}:{option}");
    if !st.minis.contains_key(&key) {
        let mut changed: HashSet<String> = HashSet::new();
        for c in &d.classes {
            if c.status != Status::Accepted || c.attributes.iter().any(|a| a.status != Status::Accepted) || c.methods.iter().any(|x| x.status != Status::Accepted) {
                changed.insert(c.name.clone());
            }
        }
        for r in &d.relations {
            if r.status != Status::Accepted {
                changed.insert(r.from.clone());
                changed.insert(r.to.clone());
            }
        }
        if changed.is_empty() {
            changed.extend(pv.unwrap().focus.iter().cloned());
        }
        let mut keep = changed.clone();
        for r in &d.relations {
            if keep.len() >= 6 {
                break;
            }
            if changed.contains(&r.from) {
                keep.insert(r.to.clone());
            }
        }
        let classes: Vec<Class> = d.classes.iter().filter(|c| keep.contains(&c.name)).take(7).cloned().collect();
        let scene = static_scene(m, &classes, d, &changed, None, Direction::Right);
        st.minis.insert(key.clone(), scene);
    }
    let (rect, _) = ui.allocate_exact_size(Vec2::new(w, 170.0), Sense::hover());
    paint_fitted(ui, rect, &st.minis[&key], m, 30.0);
}

/// Read-only record of a closed decision: question, options, defences, examiner verdicts.
pub fn history(ui: &mut Ui, design: &Design, id: &str) -> bool {
    let mut close = false;
    let Some(d) = design.decisions.iter().find(|x| x.id == id) else { return true };
    egui::Frame::new().inner_margin(Margin { left: 14, right: 14, top: 14, bottom: 4 }).show(ui, |ui| {
        ui.horizontal(|ui| {
            widgets::kicker(ui, &format!("decision {} · {}", d.id, format!("{:?}", d.state).to_lowercase()));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if link(ui, "close").clicked() {
                    close = true;
                }
            });
        });
    });
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        egui::Frame::new().inner_margin(Margin::symmetric(16, 8)).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 10.0;
            widgets::para(ui, &d.question, 16.5, css::TEXT);
            if let Some(c) = &d.context {
                widgets::para(ui, c, 12.5, css::DIM);
            }
            for o in &d.options {
                let sel = d.chosen.as_deref() == Some(&o.id);
                let (fill, border) = if sel { (Color32::from_rgba_unmultiplied(138, 169, 255, 18), Color32::from_rgba_unmultiplied(138, 169, 255, 178)) } else { (css::SURFACE, css::LINE) };
                egui::Frame::new().fill(fill).stroke(Stroke::new(1.0, border)).corner_radius(9).inner_margin(Margin::symmetric(11, 9)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&o.id).font(bold(13.0)).color(css::DIM));
                        ui.add(egui::Label::new(RichText::new(&o.label).font(bold(13.5)).color(css::TEXT)).wrap());
                        if sel {
                            ui.label(RichText::new("chosen").font(sans(11.0)).color(css::FAINT));
                        }
                    });
                    if let Some(s) = &o.summary {
                        widgets::para(ui, s, 12.0, css::DIM);
                    }
                    tradeoff_chips(ui, o, true);
                });
            }
            if let Some(cu) = &d.custom {
                egui::Frame::new().fill(Color32::from_rgba_unmultiplied(138, 169, 255, 18)).stroke(Stroke::new(1.0, css::ACCENT)).corner_radius(9).inner_margin(Margin::symmetric(11, 9)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    widgets::rich(ui, &[("✎  ", bold(13.0), css::DIM), (cu, bold(13.5), css::TEXT)]);
                });
            }
            for (i, a) in d.attempts.iter().enumerate() {
                widgets::section(ui, &format!("defence {}", i + 1));
                widgets::para(ui, format!("“{}”", a.defense), 13.0, css::TEXT);
                if let Some(fb) = &a.feedback {
                    let ok = a.verdict.as_deref() == Some("accept");
                    let (bg, border, fg) = if !ok { (Color32::from_rgba_unmultiplied(255, 92, 108, 20), Color32::from_rgba_unmultiplied(255, 92, 108, 89), hex(0xffc9cf)) } else { (Color32::from_rgba_unmultiplied(74, 222, 128, 18), Color32::from_rgba_unmultiplied(74, 222, 128, 77), hex(0xc6f6d5)) };
                    egui::Frame::new().fill(bg).stroke(Stroke::new(1.0, border)).corner_radius(6).inner_margin(Margin::symmetric(11, 9)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        widgets::rich(ui, &[(&format!("Examiner ({}): ", a.verdict.clone().unwrap_or("—".into())), bold(13.0), fg), (fb, sans(13.0), fg)]);
                    });
                }
            }
            let mut elements: Vec<String> = design.classes.iter().filter(|c| c.origin.as_deref() == Some(id)).map(|c| c.name.clone()).collect();
            for c in &design.classes {
                if c.origin.as_deref() == Some(id) {
                    continue;
                }
                for a in &c.attributes {
                    if a.origin.as_deref() == Some(id) {
                        elements.push(format!("{}.{}", c.name, a.name));
                    }
                }
                for mm in &c.methods {
                    if mm.origin.as_deref() == Some(id) {
                        elements.push(format!("{}.{}", c.name, mm.name));
                    }
                }
            }
            elements.extend(design.relations.iter().filter(|r| r.origin.as_deref() == Some(id)).map(|r| format!("{} → {}", r.from, r.to)));
            if !elements.is_empty() {
                widgets::section(ui, "introduced");
                ui.add(egui::Label::new(RichText::new(elements.join(", ")).font(mono(11.0)).color(css::TEXT)).wrap());
            }
        });
    });
    close
}

// ---------------------------------------------------------------- compare

pub struct CompareCol {
    pub id: String,
    pub label: String,
    pub design: Option<Design>,
    pub health: Option<u8>,
    pub impact: Option<Impact>,
    pub focus: Vec<String>,
    pub error: Option<String>,
    pub summary: Option<String>,
}

/// Columns for a decision's options.
pub fn decision_cols(d: &Decision, snap: &Snapshot) -> Vec<CompareCol> {
    d.options
        .iter()
        .map(|o| {
            let pv = snap.previews.iter().find(|p| p.decision == d.id && p.option == o.id);
            CompareCol {
                id: o.id.clone(),
                label: o.label.clone(),
                design: pv.and_then(|p| p.design.clone()),
                health: pv.and_then(|p| p.health),
                impact: pv.and_then(|p| p.impact.clone()),
                focus: pv.map(|p| p.focus.clone()).unwrap_or_default(),
                error: pv.and_then(|p| p.error.clone()),
                summary: o.summary.clone(),
            }
        })
        .collect()
}

#[derive(Default)]
pub struct CompareUi {
    key: String,
    scenes: Vec<Option<Scene>>,
}

/// Side-by-side comparison on one shared layout. Returns true to close.
pub fn compare(ctx: &egui::Context, area: Rect, st: &mut CompareUi, kicker: &str, title: &str, cols: &[CompareCol], snap: &Snapshot, m: &EguiMeasure) -> bool {
    let mut close = false;
    let key = format!("{title}:{}:{}", snap.rev, cols.iter().map(|c| c.id.as_str()).collect::<Vec<_>>().join(","));
    if key != st.key {
        st.key = key;
        // one layout for the union of all classes → every column shares positions
        let mut union: Vec<Class> = snap.view.classes.iter().filter(|c| c.status != Status::Pruned).cloned().collect();
        for col in cols {
            for c in col.design.iter().flat_map(|d| d.classes.iter()) {
                if c.status != Status::Pruned && !union.iter().any(|u| u.name == c.name) {
                    union.push(c.clone());
                }
            }
        }
        let mut rels = snap.view.relations.clone();
        for col in cols {
            for r in col.design.iter().flat_map(|d| d.relations.iter()) {
                if !rels.iter().any(|x| x.id == r.id) {
                    rels.push(r.clone());
                }
            }
        }
        let boxes = class_view::boxes_for(m, &union, true, 3, &rels, None, &HashSet::new(), None, None, &HashSet::new());
        let nodes: Vec<LayoutNode> = union.iter().map(|c| LayoutNode { id: c.name.clone(), w: boxes[&c.name].w, h: boxes[&c.name].h, group: None, external: false }).collect();
        let edges: Vec<LayoutEdge> = rels.iter().map(|r| LayoutEdge { from: r.from.clone(), to: r.to.clone(), kind: r.kind.as_str().into() }).collect();
        let pos = layout::layered(&nodes, &edges, Direction::Down);
        st.scenes = cols
            .iter()
            .map(|col| {
                let d = col.design.as_ref()?;
                let classes: Vec<Class> = d.classes.iter().filter(|c| c.status != Status::Pruned).cloned().collect();
                let focus: HashSet<String> = col.focus.iter().cloned().collect();
                Some(static_scene(m, &classes, d, &focus, Some(&pos), Direction::Down))
            })
            .collect();
    }
    let rect = area.shrink(8.0);
    egui::Area::new(egui::Id::unique("compare")).order(egui::Order::Foreground).fixed_pos(rect.min).show(ctx, |ui| {
        widgets::panel_frame().fill(css::SURFACE).show(ui, |ui| {
            ui.set_width(rect.width());
            ui.set_height(rect.height());
            egui::Frame::new().inner_margin(Margin { left: 18, right: 14, top: 12, bottom: 12 }).show(ui, |ui| {
                ui.horizontal(|ui| {
                    widgets::kicker(ui, kicker);
                    ui.add_space(8.0);
                    ui.add(egui::Label::new(RichText::new(title).font(bold(15.0)).color(css::TEXT)).truncate());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if icon_button(ui, "×").on_hover_text("Close · Esc").clicked() {
                            close = true;
                        }
                    });
                });
            });
            widgets::hline(ui);
            let n = cols.len().max(1) as f32;
            let avail = ui.available_rect_before_wrap();
            let col_w = ((avail.width() - (n - 1.0)) / n).max(260.0);
            egui::ScrollArea::horizontal().auto_shrink([false, false]).show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 1.0;
                    for (i, col) in cols.iter().enumerate() {
                        ui.allocate_ui_with_layout(Vec2::new(col_w, avail.height()), egui::Layout::top_down(egui::Align::Min), |ui| {
                            egui::ScrollArea::vertical().id_salt(("cmp", i)).auto_shrink([false, false]).show(ui, |ui| {
                                egui::Frame::new().inner_margin(Margin::symmetric(14, 12)).show(ui, |ui| {
                                    ui.set_width(col_w - 28.0);
                                    ui.spacing_mut().item_spacing.y = 8.0;
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new(&col.id).font(bold(13.0)).color(css::DIM));
                                        ui.add(egui::Label::new(RichText::new(&col.label).font(bold(13.5)).color(css::TEXT)).wrap());
                                        if let Some(h) = col.health {
                                            ui.label(RichText::new(h.to_string()).font(bold(12.0)).color(hc(h as f64)));
                                        }
                                    });
                                    let h = ui.ctx().content_rect().height() * 0.52;
                                    if let Some(e) = &col.error {
                                        widgets::para(ui, format!("✗ {e}"), 11.5, hex(0xffb3ba));
                                    } else if let Some(Some(scene)) = st.scenes.get(i) {
                                        let (r, _) = ui.allocate_exact_size(Vec2::new(col_w - 28.0, h), Sense::hover());
                                        paint_fitted(ui, r, scene, m, 40.0);
                                    }
                                    if let Some(s) = &col.summary {
                                        widgets::para(ui, s, 12.0, css::DIM);
                                    }
                                    if let Some(imp) = &col.impact {
                                        impact_details(ui, imp);
                                    }
                                });
                            });
                        });
                        if i + 1 < cols.len() {
                            let (r, _) = ui.allocate_exact_size(Vec2::new(1.0, avail.height()), Sense::hover());
                            ui.painter().vline(r.center().x, r.y_range(), Stroke::new(1.0, css::LINE));
                        }
                    }
                });
            });
        });
    });
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        close = true;
    }
    close
}
