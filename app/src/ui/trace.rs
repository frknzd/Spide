//! The compact operation trace: one row per mediated change — intent → changed symbols/files →
//! health delta → checks → outcome — newest first, filterable, each expandable into the full
//! record (policy decision, revisions, issues, errors) and, for source edits, the exact diff.

use super::theme::{bold, css, sans};
use super::widgets;
use egui::{RichText, Ui};
use serde_json::Value;
use std::collections::HashSet;

#[derive(Default)]
pub struct TraceUi {
    pub open: bool,
    pub only: String,
    expanded: HashSet<String>,
    rows: Vec<Value>,
    total: usize,
    loaded_at: f64,
}

impl TraceUi {
    fn load(&mut self, st: &crate::state::AppState, now: f64) {
        let Some(svc) = crate::service::get(st) else { return };
        let f = spide_service::trace::Filter { only: (!self.only.is_empty()).then(|| self.only.clone()), limit: 200, detail: true, ..Default::default() };
        let (rows, total) = svc.journal.timeline(&f);
        self.rows = rows;
        self.total = total;
        self.loaded_at = now;
    }
}

fn diff_of(st: &crate::state::AppState, artifact: &str) -> Option<String> {
    let op = artifact.split_whitespace().find_map(|w| w.strip_prefix("source:"))?;
    let root = st.lock().project_dir.clone();
    let a: spide_service::editor::Applied = serde_json::from_str(&std::fs::read_to_string(spide_service::editor::applied_path(&root, op)).ok()?).ok()?;
    Some(format!("{} (via {})\n{}", a.file, a.via, spide_service::editor::diff(&a.before, &a.after)))
}

/// Show the trace window. Returns an op to revert, if asked.
pub fn show(ctx: &egui::Context, t: &mut TraceUi, st: &crate::state::AppState) -> Option<String> {
    if !t.open {
        return None;
    }
    let now = ctx.input(|i| i.time);
    if now - t.loaded_at > 2.0 || t.rows.is_empty() && t.loaded_at == 0.0 {
        t.load(st, now);
    }
    ctx.request_repaint_after(std::time::Duration::from_secs(2));
    let mut revert = None;
    let mut open = t.open;
    egui::Window::new("Trace").open(&mut open).default_width(640.0).default_height(460.0).show(ctx, |ui: &mut Ui| {
        ui.horizontal(|ui| {
            for (key, label) in [("", "all"), ("failures", "failures"), ("conflicts", "conflicts"), ("unverified", "unverified")] {
                if ui.selectable_label(t.only == key, label).clicked() {
                    t.only = key.to_string();
                    t.load(st, now);
                }
            }
            ui.label(RichText::new(format!("{} of {}", t.rows.len(), t.total)).font(sans(11.0)).color(css::FAINT));
        });
        ui.separator();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            if t.rows.is_empty() {
                widgets::faint(ui, "No traced operations yet. Every change an agent (or you, from Neovim) makes through Spide shows up here.");
            }
            for r in &t.rows {
                let op = r["op"].as_str().unwrap_or("").to_string();
                let rec = &r["record"];
                let ok = rec["ok"].as_bool();
                let color = match (rec["kind"].as_str(), ok) {
                    (Some("interrupted"), _) | (_, Some(false)) => css::BAD,
                    _ => css::TEXT,
                };
                let open = t.expanded.contains(&op);
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(r["actor"].as_str().unwrap_or("")).font(bold(11.0)).color(css::DIM));
                    let resp = ui.add(egui::Label::new(RichText::new(r["summary"].as_str().unwrap_or("")).font(sans(12.0)).color(color)).sense(egui::Sense::click()));
                    if resp.on_hover_text("click for the full record").clicked() {
                        if open {
                            t.expanded.remove(&op);
                        } else {
                            t.expanded.insert(op.clone());
                        }
                    }
                });
                if open {
                    egui::Frame::new().fill(css::SURFACE).inner_margin(8).corner_radius(6).show(ui, |ui| {
                        let mut lines = vec![
                            format!("op {op} · run {} · tool {}", rec["run"].as_str().unwrap_or(""), rec["tool"].as_str().unwrap_or("")),
                            format!("policy: {}", rec["policy"].as_str().unwrap_or("")),
                            format!("revision {} → {}", rec["base_rev"], rec["result_rev"]),
                        ];
                        for (k, label) in [("issues_added", "new issues"), ("issues_resolved", "resolved"), ("checks", "checks")] {
                            if let Some(a) = rec[k].as_array().filter(|a| !a.is_empty()) {
                                lines.push(format!("{label}: {}", a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("; ")));
                            }
                        }
                        if let Some(e) = rec["error"].as_str() {
                            lines.push(format!("error: {e}"));
                        }
                        ui.label(RichText::new(lines.join("\n")).font(sans(11.5)).color(css::DIM));
                        if let Some(d) = rec["artifact"].as_str().and_then(|a| diff_of(st, a)) {
                            ui.label(RichText::new(d).monospace().size(11.0).color(css::TEXT));
                            if ui.button("Revert this change").on_hover_text("A new guarded change: fails if the file changed since").clicked() {
                                revert = rec["artifact"].as_str().and_then(|a| a.split_whitespace().find_map(|w| w.strip_prefix("source:"))).map(str::to_string);
                            }
                        }
                    });
                }
                ui.add_space(2.0);
            }
        });
    });
    t.open = open;
    revert
}
