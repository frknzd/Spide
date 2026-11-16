//! Diagram-first review rail: a one-click queue of pending plans, the question, alternatives,
//! consequences and readable changes up front; refinement requests to pi about elements picked on
//! the diagram; manual operation editing (structured fields + JSON, local drafts) as an advanced mode.

use super::decisions::impact_line;
use super::state::proposal_review::{field_diffs, next_proposal_id, operation_label, operation_target, patch_operation, same_plan, PlanDraft, PlanDrafts, Refinement};
use super::theme::{bold, css, hex, mono, sans};
use super::widgets::{self, button, link, primary_enabled};
use crate::backend::Backend;
use crate::state::Snapshot;
use egui::{Color32, Margin, RichText, Stroke, Ui, Vec2};
use serde_json::{json, Value};
use spide_core::model::{Design, Proposal};
use spide_core::proposal_review::ProposalReview;
use spide_core::Op;

fn op_json(o: &Op) -> Value {
    serde_json::to_value(o).unwrap_or(Value::Null)
}
fn ops_json(p: &Proposal) -> Vec<Value> {
    p.ops.iter().map(op_json).collect()
}
fn to_ops(v: &[Value]) -> Result<Vec<Op>, String> {
    v.iter().map(|o| serde_json::from_value::<Op>(o.clone()).map_err(|e| format!("{}: {e}", o.get("op").and_then(Value::as_str).unwrap_or("operation")))).collect()
}

fn templates() -> Vec<(&'static str, Value)> {
    let attribute = json!({"name": "", "type": "String", "visibility": "private", "is_static": false, "multiplicity": null});
    let method = json!({"name": "", "params": [], "returns": null, "visibility": "public", "is_static": false, "is_abstract": false, "uses": [], "calls": [], "description": null});
    vec![
        ("add_class", json!({"op": "add_class", "name": "", "package": null})),
        ("update_class", json!({"op": "update_class", "name": ""})),
        ("rename_class", json!({"op": "rename_class", "from": "", "to": ""})),
        ("remove_class", json!({"op": "remove_class", "name": ""})),
        ("add_attribute", json!({"op": "add_attribute", "class": "", "attribute": attribute})),
        ("update_attribute", json!({"op": "update_attribute", "class": "", "name": "", "attribute": attribute})),
        ("remove_attribute", json!({"op": "remove_attribute", "class": "", "name": ""})),
        ("add_method", json!({"op": "add_method", "class": "", "method": method})),
        ("update_method", json!({"op": "update_method", "class": "", "name": "", "method": method})),
        ("remove_method", json!({"op": "remove_method", "class": "", "name": ""})),
        ("add_relation", json!({"op": "add_relation", "from": "", "to": "", "kind": "association"})),
        ("update_relation", json!({"op": "update_relation", "from": "", "to": "", "kind": "association"})),
        ("remove_relation", json!({"op": "remove_relation", "from": "", "to": "", "kind": null})),
        ("add_package", json!({"op": "add_package", "name": "", "layer": null})),
        ("update_package", json!({"op": "update_package", "name": ""})),
        ("remove_package", json!({"op": "remove_package", "name": ""})),
        ("move_to_package", json!({"op": "move_to_package", "class": "", "package": null})),
        ("set_layer", json!({"op": "set_layer", "package": "", "layer": null})),
    ]
}

fn fields(op: &str) -> Option<&'static [&'static str]> {
    Some(match op {
        "add_class" => &["name", "package", "stereotype", "is_abstract", "aggregate_root", "external", "role", "rationale", "responsibilities", "attributes", "methods"],
        "update_class" => &["name", "package", "stereotype", "is_abstract", "aggregate_root", "external", "role", "rationale", "responsibilities"],
        "rename_class" => &["from", "to"],
        "remove_class" => &["name"],
        "add_attribute" | "add_method" => &["class"],
        "update_attribute" | "remove_attribute" | "update_method" | "remove_method" => &["class", "name"],
        "add_relation" => &["from", "to", "kind", "label", "from_mult", "to_mult", "directed", "interaction", "rationale"],
        "update_relation" => &["from", "to", "kind", "label", "from_mult", "to_mult", "directed", "interaction"],
        "remove_relation" => &["from", "to", "kind"],
        "add_package" => &["name", "layer", "description", "external", "system"],
        "update_package" => &["name", "description", "external", "system"],
        "remove_package" => &["name"],
        "move_to_package" => &["class", "package"],
        "set_layer" => &["package", "layer"],
        _ => return None,
    })
}

const ATTR_FIELDS: [&str; 5] = ["name", "type", "visibility", "is_static", "multiplicity"];
const METHOD_FIELDS: [&str; 9] = ["name", "params", "returns", "visibility", "is_static", "is_abstract", "uses", "calls", "description"];
const ARRAYS: [&str; 6] = ["params", "uses", "calls", "responsibilities", "attributes", "methods"];
const BOOLS: [&str; 5] = ["is_static", "is_abstract", "aggregate_root", "external", "directed"];

fn enum_values(f: &str) -> Option<&'static [&'static str]> {
    Some(match f {
        "visibility" => &["public", "private", "protected", "package"],
        "kind" => &["association", "aggregation", "composition", "inheritance", "realization", "dependency"],
        "layer" => &["ui", "infrastructure", "application", "domain"],
        "interaction" => &["sync", "async", "event"],
        _ => return None,
    })
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct OpEditor {
    pub index: Option<usize>,
    pub op: Value,
    pub advanced: bool,
    pub raw: String,
    pub json_fields: std::collections::BTreeMap<String, String>,
    pub error: String,
    pub editing_existing: bool,
}

impl OpEditor {
    pub fn new(initial: Option<(usize, &Value)>) -> Self {
        let op = initial.map(|(_, v)| v.clone()).unwrap_or_else(|| templates()[0].1.clone());
        let name = op.get("op").and_then(Value::as_str).unwrap_or("").to_string();
        Self { index: initial.map(|(i, _)| i), raw: serde_json::to_string_pretty(&op).unwrap_or_default(), advanced: initial.is_some() && fields(&name).is_none(), op, json_fields: Default::default(), error: String::new(), editing_existing: initial.is_some() }
    }
    fn materialize(&self) -> Result<Value, String> {
        let mut result = self.op.clone();
        for (path, text) in &self.json_fields {
            let (nested, name) = path.split_once('.').unwrap_or(("", path));
            let value: Value = serde_json::from_str(text).map_err(|e| format!("{name}: {e}"))?;
            if !value.is_array() {
                return Err(format!("{name} must be a JSON array"));
            }
            result = patch_operation(&result, name, value, if nested.is_empty() { None } else { Some(nested) });
        }
        Ok(result)
    }
}

pub enum EditorOut {
    Use(Value),
    Cancel,
}

fn field_ui(ui: &mut Ui, ed: &mut OpEditor, name: &str, nested: Option<&str>) {
    let data = match nested {
        Some(n) => ed.op.get(n).cloned().unwrap_or(Value::Null),
        None => ed.op.clone(),
    };
    let value = data.get(name).cloned().unwrap_or(Value::Null);
    let key = format!("{}.{name}", nested.unwrap_or(""));
    let label = format!("{}{}", nested.map(|n| format!("{n} · ")).unwrap_or_default(), name.replace('_', " "));
    ui.label(RichText::new(label).font(sans(12.0)).color(css::DIM));
    if ARRAYS.contains(&name) {
        let empty = Value::Array(vec![]);
        let shown = if value.is_null() { &empty } else { &value };
        let text = ed.json_fields.entry(key.clone()).or_insert_with(|| serde_json::to_string_pretty(shown).unwrap_or_default());
        ui.add(egui::TextEdit::multiline(text).font(mono(11.5)).desired_rows(3).desired_width(f32::INFINITY).background_color(hex(0x1c2027)));
    } else if BOOLS.contains(&name) || enum_values(name).is_some() {
        let options: Vec<(Value, String)> = if BOOLS.contains(&name) {
            vec![(Value::Null, "Unspecified".into()), (json!(true), "Yes".into()), (json!(false), "No".into())]
        } else {
            std::iter::once((Value::Null, "Unspecified".into())).chain(enum_values(name).unwrap().iter().map(|x| (json!(x), x.to_string()))).collect()
        };
        let current = options.iter().find(|(v, _)| *v == value).map(|o| o.1.clone()).unwrap_or_else(|| value.to_string());
        let mut chosen = None;
        egui::ComboBox::from_id_salt(("field", &key)).selected_text(current).width(ui.available_width()).show_ui(ui, |ui| {
            for (v, l) in &options {
                if ui.selectable_label(*v == value, l).clicked() {
                    chosen = Some(v.clone());
                }
            }
        });
        if let Some(v) = chosen {
            ed.op = patch_operation(&ed.op, name, v, nested);
        }
    } else {
        let mut s = match &value {
            Value::String(s) => s.clone(),
            Value::Null => String::new(),
            v => v.to_string(),
        };
        if ui.add(egui::TextEdit::singleline(&mut s).font(sans(12.5)).desired_width(f32::INFINITY).background_color(hex(0x1c2027))).changed() {
            ed.op = patch_operation(&ed.op, name, if s.is_empty() { Value::Null } else { json!(s) }, nested);
        }
    }
}

pub fn op_editor(ui: &mut Ui, ed: &mut OpEditor) -> Option<EditorOut> {
    let mut out = None;
    egui::Frame::new().stroke(Stroke::new(1.0, hex(0x343c48))).corner_radius(8).inner_margin(Margin::same(12)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 6.0;
        ui.label(RichText::new(if ed.editing_existing { "Edit planned change" } else { "Add planned change" }).font(bold(14.0)).color(css::TEXT));
        if !ed.editing_existing {
            let name = ed.op.get("op").and_then(Value::as_str).unwrap_or("").to_string();
            let current = if fields(&name).is_some() { name.replace('_', " ") } else { "Other operation (advanced JSON)".into() };
            ui.label(RichText::new("Change type").font(sans(12.0)).color(css::DIM));
            let mut chosen: Option<Option<Value>> = None;
            egui::ComboBox::from_id_salt("change-type").selected_text(current).width(ui.available_width()).show_ui(ui, |ui| {
                for (k, v) in templates() {
                    if ui.selectable_label(k == name, k.replace('_', " ")).clicked() {
                        chosen = Some(Some(v));
                    }
                }
                if ui.selectable_label(fields(&name).is_none(), "Other operation (advanced JSON)").clicked() {
                    chosen = Some(None);
                }
            });
            if let Some(c) = chosen {
                let next = c.clone().unwrap_or(json!({"op": ""}));
                ed.raw = serde_json::to_string_pretty(&next).unwrap_or_default();
                ed.op = next;
                ed.advanced = c.is_none();
                ed.json_fields.clear();
                ed.error.clear();
            }
        }
        if link(ui, if ed.advanced { "Structured fields" } else { "Advanced JSON" }).clicked() {
            let r: Result<(), String> = (|| {
                if !ed.advanced {
                    let next = ed.materialize()?;
                    ed.raw = serde_json::to_string_pretty(&next).unwrap_or_default();
                    ed.op = next;
                } else {
                    let next: Value = serde_json::from_str(&ed.raw).map_err(|e| e.to_string())?;
                    if fields(next.get("op").and_then(Value::as_str).unwrap_or("")).is_none() {
                        return Err("Use advanced JSON for this operation family".into());
                    }
                    ed.op = next;
                    ed.json_fields.clear();
                }
                ed.advanced = !ed.advanced;
                Ok(())
            })();
            ed.error = r.err().unwrap_or_default();
        }
        if ed.advanced {
            ui.label(RichText::new("Operation JSON").font(sans(12.0)).color(css::DIM));
            ui.add(egui::TextEdit::multiline(&mut ed.raw).font(mono(11.5)).desired_rows(14).desired_width(f32::INFINITY).background_color(hex(0x1c2027)));
        } else {
            let name = ed.op.get("op").and_then(Value::as_str).unwrap_or("").to_string();
            for f in fields(&name).unwrap_or(&[]) {
                field_ui(ui, ed, f, None);
            }
            if ed.op.get("attribute").is_some_and(|a| !a.is_null()) {
                for f in ATTR_FIELDS {
                    field_ui(ui, ed, f, Some("attribute"));
                }
            }
            if ed.op.get("method").is_some_and(|a| !a.is_null()) {
                for f in METHOD_FIELDS {
                    field_ui(ui, ed, f, Some("method"));
                }
            }
        }
        widgets::small_faint(ui, "Deletes plan an element deletion. “Remove planned change” only removes an operation. Backend validation runs before saving.");
        if !ed.error.is_empty() {
            ui.label(RichText::new(&ed.error).font(sans(12.0)).color(hex(0xffb2b2)));
        }
        ui.horizontal(|ui| {
            if button(ui, "Use change in draft").clicked() {
                let r: Result<Value, String> = if ed.advanced { serde_json::from_str(&ed.raw).map_err(|e| e.to_string()) } else { ed.materialize() };
                match r {
                    Ok(v) if v.get("op").and_then(Value::as_str).is_some() => out = Some(EditorOut::Use(v)),
                    Ok(_) => ed.error = "An operation object with an op name is required".into(),
                    Err(e) => ed.error = e,
                }
            }
            if button(ui, "Cancel edit").clicked() {
                out = Some(EditorOut::Cancel);
            }
        });
    });
    out
}

#[derive(Default)]
pub struct ProposalUi {
    pub drafts: PlanDrafts,
    /// the open plan draft (scope + edited plan as JSON ops)
    pub session: Option<PlanDraft>,
    pub editing: Option<OpEditor>,
    pub review: Option<ProposalReview>,
    pub review_key: String,
    pub requested_key: String,
    pub request_at: f64,
    pub loading: bool,
    pub error: String,
    pub busy: bool,
    pub baseline: bool,
    pub previous_ids: Vec<String>,
    pub scope: String,
    pub option_previews: super::decisions::DecisionUi,
    /// manual operation editing shown
    pub advanced: bool,
    /// all readable changes shown (long plans start collapsed)
    pub all_changes: bool,
    /// per plan scope: diagram elements picked for a change request, and the inspected one
    pub refine: std::collections::HashMap<String, Refinement>,
    pub scroll_to_inspect: bool,
    /// clear the notes of this scope (after a sent request)
    pub clear_notes: Option<String>,
    /// Unsent notes keyed by project/branch/decision scope (mirrored to local draft recovery).
    pub notes: std::collections::HashMap<String, String>,
    pub notice: String,
    pub footer_h: f32,
}

pub enum ProposalOut {
    Select(Option<String>),
    Focus(String),
    Compare(String),
    /// the design to show on the canvas while reviewing (None: none)
    Canvas,
    /// a next step after the review is complete
    Next(NextStep),
}

impl ProposalUi {
    pub fn dirty(&self) -> bool {
        self.session.as_ref().is_some_and(|s| s.scope == self.scope && !same_plan(&s.plan, &s.original))
    }
    pub fn unsaved(&self) -> bool {
        self.dirty() || self.editing.is_some()
    }
    /// Explicit discard: drop the local draft of the open plan and any operation form.
    pub fn discard_session(&mut self) {
        if let Some(s) = self.session.take() {
            self.drafts.store(PlanDraft { plan: s.original.clone(), ..s });
        }
        self.editing = None;
    }
    /// A diagram element was clicked during review: inspect it and toggle it as a request target.
    pub fn pick(&mut self, key: &str) {
        self.refine.entry(self.scope.clone()).or_default().toggle(key);
        self.scroll_to_inspect = true;
    }
    /// Classes picked for the current plan's change request (drawn as selected).
    pub fn picked_classes(&self) -> std::collections::HashSet<String> {
        let r = self.refine.get(&self.scope);
        r.map(|r| r.targets.iter().filter_map(|t| super::state::proposal_review::element_class(t, self.review.as_ref())).collect()).unwrap_or_default()
    }
    /// The design shown on the canvas during review.
    pub fn canvas(&self, plan: Option<&Proposal>) -> Option<Design> {
        let r = self.review.as_ref()?;
        let mut d = if self.baseline { r.baseline.clone() } else { r.design.clone() };
        if let Some(p) = plan {
            for x in &mut d.proposals {
                if x.id == p.id {
                    *x = p.clone();
                }
            }
        }
        Some(d)
    }
}

fn confirm(text: &str) -> bool {
    rfd::MessageDialog::new().set_title("Spide").set_description(text).set_buttons(rfd::MessageButtons::OkCancel).show() == rfd::MessageDialogResult::Ok
}

/// Title of a plan in the queue: its decision's question, else its rationale.
pub fn plan_title(p: &Proposal, snap: &Snapshot) -> String {
    p.decision
        .as_ref()
        .and_then(|id| snap.view.decisions.iter().find(|d| &d.id == id))
        .map(|d| d.question.clone())
        .or_else(|| p.rationale.clone())
        .unwrap_or_else(|| format!("Changes from {}", p.source))
}

/// What to do after the last decision is resolved.
pub enum NextStep {
    Brief,
    Behaviour,
    Implement,
    History,
    FocusPi,
}

/// The "nothing to review" state, with the contextual next steps.
pub fn completion(ui: &mut Ui, snap: &Snapshot) -> Option<NextStep> {
    let mut out = None;
    let approved = snap.view.decisions.iter().filter(|d| !d.is_pending()).count();
    egui::Frame::new().inner_margin(Margin::symmetric(18, 22)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 8.0;
        if snap.view.classes.is_empty() && approved == 0 {
            ui.label(RichText::new("Nothing to review yet").font(bold(15.0)).color(css::TEXT));
            widgets::para(ui, "Describe what you want to build to pi. Proposed changes appear on the diagram and here, ready to approve.", 12.5, css::DIM);
            ui.horizontal_wrapped(|ui| {
                if button(ui, "Edit brief").clicked() {
                    out = Some(NextStep::Brief);
                }
                if button(ui, "Talk to pi").clicked() {
                    out = Some(NextStep::FocusPi);
                }
            });
            return;
        }
        ui.label(RichText::new("✓ Review complete").font(bold(15.0)).color(css::GOOD));
        widgets::para(ui, "Every proposed change is resolved. Ask pi for more, or continue with one of these:", 12.5, css::DIM);
        let stale = snap.derived.use_cases_stale || !snap.derived.stale_sequences.is_empty();
        let step = |ui: &mut Ui, title: &str, detail: &str| -> bool {
            let r = egui::Frame::new().fill(css::RAISED).stroke(Stroke::new(1.0, css::LINE)).corner_radius(8).inner_margin(Margin::symmetric(10, 8)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new(title).font(bold(12.5)).color(css::TEXT));
                widgets::para(ui, detail, 11.5, css::DIM);
            });
            let r = ui.interact(r.response.rect, ui.scope_id().with(title), egui::Sense::click());
            if r.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            r.clicked()
        };
        let behaviour = if stale { "Scenarios may be stale after these changes — re-check their fit in the Sequence view." } else { "See how scenarios flow through the design in the Sequence view." };
        if step(ui, "Check behaviour →", behaviour) {
            out = Some(NextStep::Behaviour);
        }
        let ahead = snap.project.as_ref().and_then(|p| p.report.as_ref()).map(|r| r.count(spide_core::sync::SyncStatus::DesignAhead)).unwrap_or(0);
        if ahead > 0 && step(ui, "Implement in code →", &format!("{ahead} approved change{} not in the code yet — the agent implements them.", if ahead == 1 { "" } else { "s" })) {
            out = Some(NextStep::Implement);
        }
        if approved > 0 && step(ui, "Decision history →", &format!("{approved} resolved decision{} with their consequences.", if approved == 1 { "" } else { "s" })) {
            out = Some(NextStep::History);
        }
    });
    out
}

fn health_badge(ui: &mut Ui, after: u8, before: i64) {
    let delta = after as i64 - before;
    let mut job = egui::text::LayoutJob::default();
    let c = super::chrome::hc(after as f64);
    job.append(&after.to_string(), 0.0, egui::TextFormat { font_id: bold(12.0), color: c, ..Default::default() });
    if delta != 0 {
        job.append(&format!(" {}{delta}", if delta > 0 { "+" } else { "" }), 0.0, egui::TextFormat { font_id: sans(11.0), color: c.gamma_multiply(0.85), ..Default::default() });
    }
    ui.label(job).on_hover_text("Health if approved (change from now)");
}

/// One-click list of pending plans.
fn queue(ui: &mut Ui, plans: &[Proposal], current: &str, snap: &Snapshot, enabled: bool, select: &mut Option<String>) {
    let at = plans.iter().position(|x| x.id == current).unwrap_or(0);
    ui.horizontal(|ui| {
        widgets::kicker(ui, &format!("To review · {} of {}", at + 1, plans.len()));
    });
    if plans.len() < 2 {
        return;
    }
    let row_h = 26.0;
    egui::ScrollArea::vertical().id_salt("review-queue").max_height(row_h * 5.0 + 4.0).auto_shrink([false, true]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        for (i, x) in plans.iter().enumerate() {
            let on = x.id == current;
            let impact = snap.proposal_impacts.iter().find(|pi| pi.proposal == x.id).map(|pi| &pi.impact);
            let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), row_h), egui::Sense::click());
            let title = plan_title(x, snap);
            resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, enabled, on, format!("Review: {title}")));
            let resp = resp.on_hover_text(title);
            if on {
                ui.painter().rect_filled(rect, 6.0, Color32::from_rgba_unmultiplied(138, 169, 255, 28));
                ui.painter().vline(rect.left() + 1.0, rect.y_range().shrink(5.0), Stroke::new(2.5, css::ACCENT));
            } else if resp.hovered() && enabled {
                ui.painter().rect_filled(rect, 6.0, css::RAISED);
            }
            let mut right = rect.right() - 6.0;
            if let Some(im) = impact {
                let d = im.health_delta();
                if d != 0 {
                    let t = if d > 0 { format!("▲{d}") } else { format!("▼{}", -d) };
                    let g = ui.painter().text(egui::pos2(right, rect.center().y), egui::Align2::RIGHT_CENTER, t, sans(11.0), if d > 0 { css::GOOD } else { css::BAD });
                    right = g.left() - 8.0;
                }
            }
            let n = format!("{}", i + 1);
            ui.painter().text(egui::pos2(rect.left() + 10.0, rect.center().y), egui::Align2::LEFT_CENTER, n, bold(11.0), if on { css::ACCENT } else { css::FAINT });
            let title = plan_title(x, snap);
            let w = (right - rect.left() - 30.0).max(20.0);
            let galley = ui.painter().layout(title, sans(12.0), if on { css::TEXT } else { css::DIM }, f32::INFINITY);
            let text = if galley.size().x > w {
                let chars = ((w / galley.size().x) * galley.job.text.chars().count() as f32) as usize;
                widgets::clip(&galley.job.text, chars.max(4))
            } else {
                galley.job.text.clone()
            };
            ui.painter().text(egui::pos2(rect.left() + 28.0, rect.center().y), egui::Align2::LEFT_CENTER, text, sans(12.0), if on { css::TEXT } else { css::DIM });
            if resp.clicked() && enabled && !on {
                *select = Some(x.id.clone());
            }
            if resp.hovered() && enabled && !on {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
        }
    });
}

/// Alternatives as one-click cards: choosing one replaces the plan shown on the diagram.
fn alternatives(ui: &mut Ui, st: &mut ProposalUi, dec: &spide_core::model::Decision, chosen: Option<&str>, snap: &Snapshot, backend: &Backend, out: &mut Vec<ProposalOut>) {
    let health = snap.metrics.health as i64;
    let locked = st.unsaved();
    ui.horizontal(|ui| {
        widgets::section(ui, &format!("{} alternatives", dec.options.len()));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if widgets::link_sized(ui, "Compare side by side", 11.5).clicked() {
                out.push(ProposalOut::Compare(dec.id.clone()));
            }
        });
    });
    for o in &dec.options {
        let on = chosen == Some(o.id.as_str());
        let pv = snap.previews.iter().find(|v| v.decision == dec.id && v.option == o.id);
        let (fill, border) = if on { (Color32::from_rgba_unmultiplied(94, 234, 212, 16), Color32::from_rgba_unmultiplied(94, 234, 212, 160)) } else { (css::SURFACE, css::LINE_STRONG) };
        let r = egui::Frame::new().fill(fill).stroke(Stroke::new(1.0, border)).corner_radius(8).inner_margin(Margin::symmetric(10, 8)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 4.0;
            ui.horizontal(|ui| {
                ui.label(RichText::new(if on { "◉" } else { "○" }).font(sans(13.0)).color(if on { css::GHOST } else { css::FAINT }));
                let w = ui.available_width() - if pv.and_then(|p| p.health).is_some() { 52.0 } else { 0.0 };
                ui.allocate_ui_with_layout(Vec2::new(w.max(40.0), 18.0), egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(true), |ui| {
                    ui.set_width(w.max(40.0));
                    ui.add(egui::Label::new(RichText::new(&o.label).font(bold(13.0)).color(css::TEXT)).wrap());
                });
                if let Some(h) = pv.and_then(|p| p.health) {
                    health_badge(ui, h, health);
                }
            });
            if let Some(s) = &o.summary {
                widgets::para(ui, s, 12.0, css::DIM);
            }
            super::decisions::tradeoff_chips(ui, o, true);
            for x in o.pros.iter().take(3) {
                widgets::para(ui, format!("+ {x}"), 11.5, css::PRO);
            }
            for x in o.cons.iter().take(3) {
                widgets::para(ui, format!("− {x}"), 11.5, css::CON);
            }
            if let Some(e) = pv.and_then(|p| p.error.as_ref()) {
                widgets::para(ui, format!("Can't apply: {e}"), 11.5, css::BAD);
            }
        });
        if on {
            continue;
        }
        let hint = if locked { "Save or discard your manual edits before switching alternatives" } else { "Show this alternative on the diagram" };
        let resp = ui.interact(r.response.rect, ui.scope_id().with(("alt", &o.id)), egui::Sense::click()).on_hover_text(hint);
        if resp.hovered() && !locked && !st.busy {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if resp.clicked() && !locked && !st.busy {
            st.busy = true;
            st.error.clear();
            let (id, opt, rev) = (dec.id.clone(), o.id.clone(), snap.rev);
            backend.call(move |s, _| crate::commands::select_decision_option(s, id, opt, rev), |app, r| {
                app.proposal_ui.busy = false;
                if let Err(e) = r {
                    app.proposal_ui.error = e;
                }
            });
        }
    }
}

fn diff_rows(ui: &mut Ui, diffs: &[super::state::proposal_review::FieldDiff]) {
    use super::state::proposal_review::value_text;
    for f in diffs {
        ui.label(RichText::new(f.field.replace('_', " ")).font(sans(11.5)).color(hex(0x9aa9bb)));
        let (b, a) = (value_text(&f.before), value_text(&f.after));
        if !f.before.is_null() {
            egui::Frame::new().fill(hex(0x371f27)).corner_radius(4).inner_margin(Margin::symmetric(6, 3)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.add(egui::Label::new(RichText::new(format!("− {b}")).font(mono(11.0)).color(hex(0xffb2b2))).wrap());
            });
        }
        if !f.after.is_null() {
            egui::Frame::new().fill(hex(0x17322f)).corner_radius(4).inner_margin(Margin::symmetric(6, 3)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.add(egui::Label::new(RichText::new(format!("+ {a}")).font(mono(11.0)).color(hex(0xa5efdd))).wrap());
            });
        }
    }
}

/// The element clicked on the diagram: what this plan does to it.
fn inspected(ui: &mut Ui, st: &mut ProposalUi, scope: &str, out: &mut Vec<ProposalOut>) {
    use super::state::proposal_review::{element_class, element_diffs, element_label};
    let Some(key) = st.refine.get(scope).and_then(|r| r.inspect.clone()) else { return };
    let review = st.review.as_ref();
    let label = element_label(&key, review);
    let diffs = review.map(|r| element_diffs(r, &key)).unwrap_or_default();
    let targeted = st.refine.get(scope).is_some_and(|r| r.targets.contains(&key));
    let r = egui::Frame::new().fill(css::RAISED).stroke(Stroke::new(1.0, css::LINE_STRONG)).corner_radius(8).inner_margin(Margin::symmetric(10, 8)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 4.0;
        ui.horizontal(|ui| {
            ui.label(RichText::new(&label).font(bold(12.5)).color(css::TEXT));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::icon_button(ui, "×").on_hover_text("Close").clicked() {
                    if let Some(r) = st.refine.get_mut(scope) {
                        r.inspect = None;
                    }
                }
                if let Some(c) = element_class(&key, review) {
                    if widgets::link_sized(ui, "Locate", 11.0).clicked() {
                        out.push(ProposalOut::Focus(c));
                    }
                }
            });
        });
        if diffs.is_empty() {
            widgets::para(ui, "This plan doesn't change it.", 11.5, css::FAINT);
        } else {
            diff_rows(ui, &diffs);
        }
        let t = if targeted { "Included in your change request · click it again on the diagram to remove" } else { "Click it on the diagram to include it in a change request" };
        widgets::small_faint(ui, t);
    });
    if st.scroll_to_inspect {
        st.scroll_to_inspect = false;
        r.response.scroll_to_me(Some(egui::Align::Center));
    }
}

fn changes(ui: &mut Ui, st: &mut ProposalUi, scope: &str, p: &Proposal, out: &mut Vec<ProposalOut>) {
    use super::state::proposal_review::change_lines;
    let Some(review) = st.review.as_ref() else {
        widgets::section(ui, &format!("Changes ({})", p.ops.len()));
        widgets::small_faint(ui, "Checking the plan…");
        return;
    };
    let lines = change_lines(review);
    let errors: Vec<(usize, String)> = review.steps.iter().filter_map(|s| s.error.clone().map(|e| (s.index, e))).collect();
    widgets::section(ui, &format!("Changes ({})", lines.len()));
    let limit = if st.all_changes { usize::MAX } else { 12 };
    let inspect = st.refine.get(scope).and_then(|r| r.inspect.clone());
    let mut clicked = None;
    for l in lines.iter().take(limit) {
        let c = match l.sign {
            '+' => css::GHOST,
            '−' => css::CON,
            _ => css::WARN,
        };
        let on = l.key.is_some() && l.key == inspect;
        let r = ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            ui.label(RichText::new(l.sign.to_string()).font(bold(12.0)).color(c));
            ui.add(egui::Label::new(RichText::new(&l.text).font(sans(12.0)).color(if on { css::TEXT } else { css::DIM })).wrap());
        });
        if l.key.is_some() {
            let resp = ui.interact(r.response.rect, ui.scope_id().with(("change", &l.text)), egui::Sense::click()).on_hover_text("Show on the diagram");
            if resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if resp.clicked() {
                clicked = Some(l.clone());
            }
        }
    }
    if lines.len() > 12 && widgets::link_sized(ui, if st.all_changes { "Show fewer".into() } else { format!("Show all {}", lines.len()) }, 11.5).clicked() {
        st.all_changes = !st.all_changes;
    }
    for (i, e) in errors {
        ui.label(RichText::new(format!("Change {}: {e}", i + 1)).font(sans(11.5)).color(css::BAD));
    }
    if let Some(l) = clicked {
        st.refine.entry(scope.to_string()).or_default().inspect = l.key.clone();
        if let Some(c) = l.class {
            out.push(ProposalOut::Focus(c));
        }
    }
}

pub fn show(ui: &mut Ui, st: &mut ProposalUi, snap: &Snapshot, selected: Option<&str>, backend: &Backend, now: f64, out: &mut Vec<ProposalOut>) {
    let plans = &snap.view.proposals;
    let ids: Vec<String> = plans.iter().map(|p| p.id.clone()).collect();
    let id = next_proposal_id(&ids, selected, &st.previous_ids);
    st.previous_ids = ids.clone();
    if selected != id.as_deref() {
        out.push(ProposalOut::Select(id.clone()));
    }
    let saved = id.as_ref().and_then(|i| plans.iter().find(|p| &p.id == i));
    let scope = format!("{}:{}:{}", snap.project_dir, snap.branch, id.clone().unwrap_or_default());
    if scope != st.scope {
        st.scope = scope.clone();
        st.session = saved.map(|s| st.drafts.load(&scope).unwrap_or(PlanDraft { scope: scope.clone(), plan: s.clone(), original: s.clone(), rev: snap.rev }));
        st.editing = None;
        st.error.clear();
        st.review = None;
        st.baseline = false;
        st.all_changes = false;
        st.advanced = st.session.as_ref().is_some_and(|s| !same_plan(&s.plan, &s.original));
    }
    // a newer saved plan replaces a clean session
    if let (Some(s), Some(sess)) = (saved, st.session.as_ref()) {
        if sess.scope == scope && !st.unsaved() && snap.rev > sess.rev {
            st.session = Some(PlanDraft { scope: scope.clone(), plan: s.clone(), original: s.clone(), rev: snap.rev });
        }
    }
    let Some(p) = st.session.as_ref().filter(|s| s.scope == scope).map(|s| s.plan.clone()).or_else(|| saved.cloned()) else {
        if let Some(n) = completion(ui, snap) {
            out.push(ProposalOut::Next(n));
        }
        if st.review.take().is_some() {
            out.push(ProposalOut::Canvas);
        }
        return;
    };
    let dirty = st.dirty();
    let conflict = st.session.as_ref().is_some_and(|s| (dirty || st.editing.is_some()) && s.rev != snap.rev);
    let request_key = format!("{scope}:{}:{}", snap.rev, serde_json::to_string(&p.ops).unwrap_or_default());
    // debounced validation (200 ms); the previous review stays on the canvas until the new one arrives
    if request_key != st.requested_key {
        st.requested_key = request_key.clone();
        st.option_previews = Default::default();
        st.request_at = now + 0.2;
        st.loading = true;
    }
    if st.loading && now >= st.request_at && st.review_key != request_key && !st.busy {
        st.review_key = request_key.clone();
        let (pid, ops, key) = (p.id.clone(), p.ops.clone(), request_key.clone());
        backend.call(move |s, _| crate::commands::review_proposal(s, pid, Some(ops)), move |app, r| {
            let pu = &mut app.proposal_ui;
            if pu.requested_key != key {
                return;
            }
            pu.loading = false;
            match r {
                Ok(r) => {
                    pu.review = Some(r);
                    app.refresh_proposal_canvas();
                }
                Err(e) => pu.error = e,
            }
        });
    } else if st.loading {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
    }
    let valid_preview = !st.loading && st.review.as_ref().is_some_and(|r| r.rev == snap.rev && r.errors.is_empty());

    let mut change: Option<Proposal> = None;
    let mut select: Option<String> = None;
    let mut notes = st.notes.get(&scope).cloned().unwrap_or_default();
    let owner = p.decision.as_ref().and_then(|id| snap.view.decisions.iter().find(|d| &d.id == id));
    let footer_h = st.footer_h.max(150.0);
    egui::ScrollArea::vertical().id_salt(("review-body", &scope)).max_height((ui.available_height() - footer_h).max(60.0)).auto_shrink([false, false]).show(ui, |ui| {
        egui::Frame::new().inner_margin(Margin { left: 14, right: 14, top: 2, bottom: 16 }).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 7.0;
            queue(ui, plans, &p.id, snap, !st.busy, &mut select);
            ui.add_space(2.0);
            ui.add(egui::Label::new(RichText::new(plan_title(&p, snap)).font(bold(15.5)).color(css::TEXT)).wrap());
            if let Some(c) = owner.and_then(|d| d.context.as_ref()) {
                widgets::para(ui, c, 12.0, css::DIM);
            }
            // blocking problems first: they decide whether approval is possible
            if let Some(r) = st.review.as_ref() {
                for x in &r.errors {
                    widgets::para(ui, format!("⚠ {x}"), 12.0, css::BAD);
                }
                if !r.prerequisites.is_empty() {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new("⚠ Needs earlier changes approved first:").font(sans(12.0)).color(css::WARN));
                        for pre in &r.prerequisites {
                            let t = plans.iter().find(|x| &x.id == pre).map(|x| widgets::clip(&plan_title(x, snap), 40)).unwrap_or(pre.clone());
                            if widgets::link_sized(ui, format!("{t} →"), 12.0).on_hover_text("Review it now").clicked() {
                                select = Some(pre.clone());
                            }
                        }
                    });
                }
                for x in &r.notices {
                    widgets::para(ui, x, 11.5, css::WARN);
                }
            }
            match owner {
                Some(dec) if dec.options.len() > 1 => alternatives(ui, st, dec, p.option.as_deref(), snap, backend, out),
                Some(dec) => {
                    if let Some(o) = dec.options.first() {
                        if let Some(s) = &o.summary {
                            widgets::para(ui, s, 12.5, css::DIM);
                        }
                        super::decisions::tradeoff_chips(ui, o, false);
                        for x in &o.pros {
                            widgets::para(ui, format!("+ {x}"), 11.5, css::PRO);
                        }
                        for x in &o.cons {
                            widgets::para(ui, format!("− {x}"), 11.5, css::CON);
                        }
                    }
                }
                None => {
                    if let Some(r) = p.rationale.as_ref().filter(|r| Some(r.as_str()) != Some(plan_title(&p, snap).as_str())) {
                        widgets::para(ui, r, 12.5, css::DIM);
                    }
                }
            }
            if let Some(imp) = st.review.as_ref().and_then(|r| r.impact.as_ref()) {
                ui.add_space(2.0);
                widgets::section(ui, "Consequences");
                let k = format!("{scope}:impact");
                let mut open = st.option_previews.impact_open.contains(&k);
                impact_line(ui, imp, &mut open, true);
                if open {
                    st.option_previews.impact_open.insert(k);
                } else {
                    st.option_previews.impact_open.remove(&k);
                }
            }
            ui.add_space(2.0);
            inspected(ui, st, &scope, out);
            changes(ui, st, &scope, &p, out);
            ui.add_space(6.0);
            widgets::hline(ui);
            let label = if st.advanced { "▾ Edit plan manually (advanced)" } else { "▸ Edit plan manually (advanced)" };
            if widgets::link_sized(ui, label, 11.5).on_hover_text("Edit the exact operations yourself. Usually it's easier to ask pi for a change below.").clicked() {
                st.advanced = !st.advanced || st.unsaved();
            }
            if st.advanced || st.unsaved() {
                advanced(ui, st, &p, saved, &scope, snap, dirty, conflict, valid_preview, backend, out, &mut change);
            }
        });
    });
    // footer: refinement + decision, always visible
    let top = ui.cursor().top();
    egui::Frame::new().fill(css::SURFACE).inner_margin(Margin::symmetric(12, 10)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 6.0;
        footer(ui, st, &p, &scope, snap, owner, &ids, &mut notes, dirty, conflict, valid_preview, backend);
    });
    st.footer_h = ui.cursor().top() - top + 4.0;
    if notes.is_empty() { st.notes.remove(&scope); } else { st.notes.insert(scope.clone(), notes); }
    if let Some(next) = change {
        if !st.busy {
            let original = st.session.as_ref().filter(|s| s.scope == scope).map(|s| s.original.clone()).or_else(|| saved.cloned()).unwrap_or(next.clone());
            let rev = st.session.as_ref().filter(|s| s.scope == scope).map(|s| s.rev).unwrap_or(snap.rev);
            let draft = PlanDraft { scope: scope.clone(), plan: next, original, rev };
            st.drafts.store(draft.clone());
            st.session = Some(draft);
            st.error.clear();
        }
    }
    if let Some(next) = select {
        if !st.busy && (!st.unsaved() || confirm("Leave this plan? Saved draft changes will be kept locally; unfinished operation form edits will be lost.")) {
            out.push(ProposalOut::Select(Some(next)));
        }
    }
}

/// Why approval is unavailable right now (None: it is available).
fn approve_blocker(st: &ProposalUi, snap: &Snapshot, dirty: bool, conflict: bool, valid_preview: bool) -> Option<String> {
    if st.busy {
        return Some("Working…".into());
    }
    if st.editing.is_some() {
        return Some("Finish or cancel the open change form first.".into());
    }
    if conflict {
        return Some("The workspace changed. Revalidate your manual edits (advanced) first.".into());
    }
    if dirty {
        return Some("Save or discard your manual edits first.".into());
    }
    if st.loading || st.review.is_none() {
        return Some("Checking the plan…".into());
    }
    let r = st.review.as_ref()?;
    if r.rev != snap.rev || st.session.as_ref().is_some_and(|s| s.rev != snap.rev) {
        return Some("Checking the latest workspace…".into());
    }
    if !r.errors.is_empty() || !valid_preview {
        return Some("This plan has errors. Ask pi for a revision or edit it manually.".into());
    }
    if !r.prerequisites.is_empty() {
        return Some("Approve the earlier changes it depends on first.".into());
    }
    if !r.can_accept {
        return Some("This plan depends on earlier changes. Approve those first.".into());
    }
    None
}

#[allow(clippy::too_many_arguments)]
fn footer(ui: &mut Ui, st: &mut ProposalUi, p: &Proposal, scope: &str, snap: &Snapshot, owner: Option<&spide_core::model::Decision>, ids: &[String], notes: &mut String, dirty: bool, conflict: bool, valid_preview: bool, backend: &Backend) {
    use super::state::proposal_review::element_label;
    use crate::state::PiStatus;
    if st.clear_notes.as_deref() == Some(scope) {
        st.clear_notes = None;
        notes.clear();
    }
    let targets = st.refine.get(scope).map(|r| r.targets.clone()).unwrap_or_default();
    if targets.is_empty() {
        widgets::small_faint(ui, "Click elements on the diagram to point pi at them.");
    } else {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
            ui.label(RichText::new("About").font(sans(11.5)).color(css::FAINT));
            let mut remove = None;
            for t in &targets {
                let label = widgets::clip(&element_label(t, st.review.as_ref()), 32);
                let b = egui::Button::new(RichText::new(format!("{label}  ×")).font(sans(11.5)).color(css::TEXT)).fill(css::RAISED2).stroke(Stroke::new(1.0, css::LINE_STRONG)).corner_radius(10).min_size(Vec2::new(0.0, 20.0));
                if ui.add(b).on_hover_text("Remove from the request").clicked() {
                    remove = Some(t.clone());
                }
            }
            if let Some(t) = remove {
                if let Some(r) = st.refine.get_mut(scope) {
                    r.toggle(&t);
                }
            }
        });
    }
    let notes_label = ui.label(RichText::new("Notes or requested changes").font(sans(11.5)).color(css::DIM));
    ui.add_enabled(!st.busy, egui::TextEdit::multiline(notes).hint_text(RichText::new("Optional notes for the record — or tell pi what to change…").color(css::FAINT)).font(sans(12.5)).desired_rows(2).desired_width(f32::INFINITY).margin(Margin::symmetric(8, 6)).background_color(css::BG)).labelled_by(notes_label.id);
    let blocker = approve_blocker(st, snap, dirty, conflict, valid_preview);
    let pi_ready = matches!(snap.pi_status, PiStatus::Idle | PiStatus::Busy);
    let request_blocker = if owner.is_none() {
        Some("Only decisions can be revised by pi.")
    } else if !pi_ready {
        Some(if snap.pi_status == PiStatus::Starting { "pi is starting…" } else { "pi is offline — use Restart in the terminal header." })
    } else if st.unsaved() {
        Some("Save or discard your manual edits first.")
    } else if notes.trim().is_empty() {
        Some("Describe what pi should change.")
    } else {
        None
    };
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let approve = primary_enabled(ui, blocker.is_none(), "Approve design").on_hover_text("Apply the plan shown on the diagram. Your notes are recorded with it.");
        if approve.clicked() {
            resolve(st, backend, p, scope, snap, ids, true, notes.clone());
        }
        let b = egui::Button::new(RichText::new("Request changes").font(sans(12.0)).color(css::TEXT)).fill(css::RAISED).stroke(Stroke::new(1.0, css::LINE)).corner_radius(6);
        let r = ui.add_enabled(request_blocker.is_none() && !st.busy, b).on_hover_text("Send your notes and selected elements to pi. Nothing is applied.");
        if r.clicked() {
            if let Some(dec) = owner {
                st.busy = true;
                st.error.clear();
                st.notice.clear();
                let labels: Vec<String> = targets.iter().map(|t| format!("{} [{t}]", element_label(t, st.review.as_ref()))).collect();
                let (id, rev, text, option, scope) = (dec.id.clone(), snap.rev, notes.trim().to_string(), p.option.clone(), scope.to_string());
                backend.call(move |s, pi| crate::commands::revise_decision(s, pi, id, rev, text, option, labels), move |app, r| {
                    let pu = &mut app.proposal_ui;
                    pu.busy = false;
                    match r {
                        Ok(()) => {
                            pu.refine.remove(&scope);
                            pu.clear_notes = Some(scope);
                            pu.notice = "Sent to pi. The revised plan will replace this one here — nothing was applied.".into();
                        }
                        Err(e) => pu.error = format!("Couldn't send the request: {e}. Your notes and selection are kept."),
                    }
                });
            }
        }
        let d = egui::Button::new(RichText::new("Dismiss").font(sans(12.0)).color(css::DIM)).fill(Color32::TRANSPARENT).stroke(Stroke::new(1.0, css::LINE)).corner_radius(6);
        if ui.add_enabled(!st.busy, d).on_hover_text("Reject this plan without applying anything").clicked() && (!st.unsaved() || confirm("Reject this proposal and discard its unsaved draft and operation form?")) {
            resolve(st, backend, p, scope, snap, ids, false, String::new());
        }
    });
    if let Some(b) = &blocker {
        widgets::small_faint(ui, format!("Approve: {b}"));
    }
    if let Some(b) = request_blocker.filter(|_| !notes.trim().is_empty() || !targets.is_empty()) {
        widgets::small_faint(ui, format!("Request changes: {b}"));
    } else if request_blocker.is_none() && snap.pi_status == PiStatus::Busy {
        widgets::small_faint(ui, "pi is busy — your request queues as a follow-up.");
    }
    if !st.error.is_empty() {
        widgets::para(ui, &st.error, 12.0, hex(0xffb2b2));
    }
    if !st.notice.is_empty() {
        widgets::para(ui, &st.notice, 12.0, css::GHOST);
    }
}

/// Manual plan editing: rationale, structured operation forms and raw JSON.
#[allow(clippy::too_many_arguments)]
fn advanced(ui: &mut Ui, st: &mut ProposalUi, p: &Proposal, saved: Option<&Proposal>, scope: &str, snap: &Snapshot, dirty: bool, conflict: bool, valid_preview: bool, backend: &Backend, out: &mut Vec<ProposalOut>, change: &mut Option<Proposal>) {
    let scope = scope.to_string();
    ui.add_enabled_ui(!st.busy, |ui| {
        widgets::small_faint(ui, format!("{} · by {} · {}", p.id, p.source, if dirty { "unsaved manual edits" } else { "saved plan" }));
        ui.horizontal(|ui| {
            let can_save = dirty && !conflict && valid_preview && st.editing.is_none();
            if ui.add_enabled(can_save, egui::Button::new(RichText::new("Save plan").font(sans(12.0))).fill(css::RAISED).stroke(Stroke::new(1.0, css::LINE)).corner_radius(6)).clicked() {
                if let Some(sess) = st.session.clone() {
                    st.busy = true;
                    st.error.clear();
                    let plan = p.clone();
                    backend.call(
                        move |s, _| crate::commands::update_proposal(s, plan.id.clone(), sess.rev, plan.ops.clone(), plan.rationale.clone(), plan.decision.clone(), plan.option.clone()).map(|_| (sess, plan)),
                        |app, r| {
                            let pu = &mut app.proposal_ui;
                            pu.busy = false;
                            match r {
                                Ok((sess, plan)) => {
                                    let next = PlanDraft { original: plan, rev: sess.rev + 1, ..sess };
                                    pu.drafts.store(next.clone());
                                    pu.session = Some(next);
                                }
                                Err(e) => pu.error = e,
                            }
                        },
                    );
                }
            }
            if ui.add_enabled(dirty || st.editing.is_some(), egui::Button::new(RichText::new("Discard edits").font(sans(12.0))).fill(css::RAISED).stroke(Stroke::new(1.0, css::LINE)).corner_radius(6)).clicked() && confirm("Discard draft changes and reload the saved plan?") {
                if let Some(s) = saved {
                    let next = PlanDraft { scope: scope.clone(), plan: s.clone(), original: s.clone(), rev: snap.rev };
                    st.drafts.store(next.clone());
                    st.session = Some(next);
                    st.editing = None;
                    st.error.clear();
                }
            }
        });
        if conflict {
            ui.label(RichText::new("The workspace changed. Your draft was preserved. Saving is blocked until you explicitly reconcile it.").font(sans(12.0)).color(hex(0xffb2b2)));
            if button(ui, "Revalidate draft against latest workspace").clicked() && confirm("Revalidate your draft against the latest workspace? Saving will replace this proposal's saved operations with your draft, not merge them.") {
                let next = PlanDraft { scope: scope.clone(), plan: p.clone(), original: saved.cloned().unwrap_or(p.clone()), rev: snap.rev };
                st.drafts.store(next.clone());
                st.session = Some(next);
            }
        }
        let rationale_label = ui.label(RichText::new("Plan rationale").font(sans(12.0)).color(css::DIM));
        let mut rat = p.rationale.clone().unwrap_or_default();
        if ui.add(egui::TextEdit::multiline(&mut rat).font(sans(12.5)).desired_rows(2).desired_width(f32::INFINITY).background_color(hex(0x1c2027))).labelled_by(rationale_label.id).changed() {
            *change = Some(Proposal { rationale: if rat.is_empty() { None } else { Some(rat) }, ..p.clone() });
        }
        ui.label(RichText::new(format!("Operations ({})", p.ops.len())).font(bold(13.0)).color(css::TEXT));
        let ops = ops_json(p);
        for (i, op) in ops.iter().enumerate() {
            egui::CollapsingHeader::new(format!("{}. {} · {}", i + 1, operation_label(op), operation_target(op).unwrap_or_default())).id_salt((&scope, "operation", i)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    if button(ui, "Edit change").clicked() && (st.editing.is_none() || confirm("Discard the unfinished operation form and edit another change?")) {
                        st.editing = Some(OpEditor::new(Some((i, op))));
                    }
                    if button(ui, "Remove planned change").clicked() {
                        let mut next = ops.clone();
                        next.remove(i);
                        if let Ok(o) = to_ops(&next) {
                            *change = Some(Proposal { ops: o, ..p.clone() });
                        }
                        st.editing = None;
                    }
                    if let Some(t) = operation_target(op) {
                        if link(ui, format!("Locate {t}")).clicked() {
                            out.push(ProposalOut::Focus(t));
                        }
                    }
                });
                if let Some(step) = st.review.as_ref().and_then(|r| r.steps.get(i)) {
                    if let Some(e) = &step.error {
                        ui.label(RichText::new(e).font(sans(12.0)).color(hex(0xffb2b2)));
                    }
                    for c in &step.changes {
                        ui.label(RichText::new(&c.path).font(bold(12.0)).color(css::TEXT));
                        diff_rows(ui, &field_diffs(&c.before, &c.after, ""));
                    }
                }
                egui::CollapsingHeader::new(RichText::new("Operation JSON").font(sans(12.0)).color(css::DIM)).id_salt((&scope, "full", i)).show(ui, |ui| {
                    egui::Frame::new().fill(hex(0x11151b)).inner_margin(Margin::same(8)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.add(egui::Label::new(RichText::new(serde_json::to_string_pretty(op).unwrap_or_default()).font(mono(11.0)).color(css::TEXT)).wrap());
                    });
                });
            });
        }
        if button(ui, "Add planned change").clicked() && (st.editing.is_none() || confirm("Discard the unfinished operation form and edit another change?")) {
            st.editing = Some(OpEditor::new(None));
        }
        if let Some(ed) = st.editing.as_mut() {
            match op_editor(ui, ed) {
                Some(EditorOut::Use(v)) => {
                    let mut next = ops.clone();
                    match ed.index {
                        Some(i) if i < next.len() => next[i] = v,
                        _ => next.push(v),
                    }
                    match to_ops(&next) {
                        Ok(o) => {
                            *change = Some(Proposal { ops: o, ..p.clone() });
                            st.editing = None;
                        }
                        Err(e) => ed.error = e,
                    }
                }
                Some(EditorOut::Cancel) => st.editing = None,
                None => {}
            }
        }
    });
}

fn resolve(st: &mut ProposalUi, backend: &Backend, p: &Proposal, scope: &str, snap: &Snapshot, ids: &[String], accept: bool, notes: String) {
    st.busy = true;
    st.error.clear();
    let (pid, rev, plan, scope, all) = (p.id.clone(), snap.rev, p.clone(), scope.to_string(), ids.to_vec());
    let decision = plan.decision.clone();
    backend.call(
        move |s, _| match (accept, decision) {
            (true, Some(id)) => crate::commands::approve_decision(s, id, rev, notes),
            (true, None) => crate::commands::accept_proposal(s, pid, Some(rev)),
            (false, _) => crate::commands::reject_proposal(s, pid, Some(rev)),
        },
        move |app, r| {
            app.proposal_ui.busy = false;
            match r {
                Ok(()) => {
                    app.proposal_ui.drafts.store(PlanDraft { scope: scope.clone(), plan: plan.clone(), original: plan.clone(), rev });
                    app.proposal_ui.editing = None;
                    app.proposal_ui.session = None;
                    app.proposal_ui.refine.remove(&scope);
                    app.proposal_ui.notice.clear();
                    let rest: Vec<String> = all.iter().filter(|x| **x != plan.id).cloned().collect();
                    app.selected_proposal = next_proposal_id(&rest, Some(&plan.id), &all);
                    app.review_resolved(accept);
                }
                Err(e) => app.proposal_ui.error = e,
            }
        },
    );
}
