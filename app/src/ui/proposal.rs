//! Ghost proposal review: one plan at a time, before/after, impact, an operation editor (structured
//! fields + advanced JSON), local drafts, save / accept / reject.

use super::decisions::impact_line;
use super::state::proposal_review::{field_diffs, next_proposal_id, operation_label, operation_target, patch_operation, same_plan, PlanDraft, PlanDrafts};
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
}

pub enum ProposalOut {
    Select(Option<String>),
    Focus(String),
    /// the design to show on the canvas while reviewing (None: none)
    Canvas,
}

impl ProposalUi {
    pub fn dirty(&self) -> bool {
        self.session.as_ref().is_some_and(|s| s.scope == self.scope && !same_plan(&s.plan, &s.original))
    }
    pub fn unsaved(&self) -> bool {
        self.dirty() || self.editing.is_some()
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
    }
    // a newer saved plan replaces a clean session
    if let (Some(s), Some(sess)) = (saved, st.session.as_ref()) {
        if sess.scope == scope && !st.unsaved() && snap.rev > sess.rev {
            st.session = Some(PlanDraft { scope: scope.clone(), plan: s.clone(), original: s.clone(), rev: snap.rev });
        }
    }
    let Some(p) = st.session.as_ref().filter(|s| s.scope == scope).map(|s| s.plan.clone()).or_else(|| saved.cloned()) else {
        egui::Frame::new().inner_margin(Margin::symmetric(22, 32)).show(ui, |ui| {
            ui.label(RichText::new("No ghost proposals").font(bold(14.0)).color(css::TEXT));
            widgets::para(ui, "All plans reviewed. Undo restores the last change. Any unfinished drafts remain stored locally.", 12.5, css::DIM);
        });
        if st.review.take().is_some() {
            out.push(ProposalOut::Canvas);
        }
        return;
    };
    let dirty = st.dirty();
    let conflict = st.session.as_ref().is_some_and(|s| (dirty || st.editing.is_some()) && s.rev != snap.rev);
    let request_key = format!("{scope}:{}:{}", snap.rev, serde_json::to_string(&p.ops).unwrap_or_default());
    // debounced validation (200 ms)
    if request_key != st.requested_key {
        st.requested_key = request_key.clone();
        st.request_at = now + 0.2;
        st.loading = true;
        if st.review.take().is_some() {
            out.push(ProposalOut::Canvas);
        }
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
    let at = plans.iter().position(|x| x.id == p.id).unwrap_or(0);

    let mut change: Option<Proposal> = None;
    let mut select: Option<String> = None;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        egui::Frame::new().inner_margin(Margin { left: 16, right: 16, top: 4, bottom: 24 }).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 8.0;
            ui.label(RichText::new("Ghost proposals").font(bold(19.0)).color(css::TEXT));
            ui.add_enabled_ui(!st.busy, |ui| {
                ui.label(RichText::new("Plan").font(sans(12.0)).color(css::DIM));
                egui::ComboBox::from_id_salt("plan").selected_text(format!("{} · {}", p.id, p.rationale.clone().unwrap_or(p.source.clone()))).width(ui.available_width()).truncate().show_ui(ui, |ui| {
                    for x in plans {
                        if ui.selectable_label(x.id == p.id, format!("{} · {}", x.id, x.rationale.clone().unwrap_or(x.source.clone()))).clicked() {
                            select = Some(x.id.clone());
                        }
                    }
                });
                ui.horizontal(|ui| {
                    if button(ui, "‹").on_hover_text("Previous proposal").clicked() {
                        select = Some(plans[(at + plans.len() - 1) % plans.len()].id.clone());
                    }
                    ui.label(RichText::new(format!("{} of {}", at + 1, plans.len())).font(sans(12.5)));
                    if button(ui, "›").on_hover_text("Next proposal").clicked() {
                        select = Some(plans[(at + 1) % plans.len()].id.clone());
                    }
                });
            });
            ui.add_enabled_ui(!st.busy, |ui| {
                ui.label(RichText::new("Plan rationale").font(sans(12.0)).color(css::DIM));
                let mut rat = p.rationale.clone().unwrap_or_default();
                if ui.add(egui::TextEdit::multiline(&mut rat).font(sans(12.5)).desired_rows(2).desired_width(f32::INFINITY).background_color(hex(0x1c2027))).changed() {
                    change = Some(Proposal { rationale: if rat.is_empty() { None } else { Some(rat) }, ..p.clone() });
                }
                widgets::small_faint(ui, format!("{} · by {} · {}", p.id, p.source, if dirty { "Unsaved user amendment" } else { "Saved plan" }));
                if !st.error.is_empty() {
                    ui.label(RichText::new(&st.error).font(sans(12.0)).color(hex(0xffb2b2)));
                }
                if st.loading {
                    ui.label(RichText::new("Validating draft preview…").font(sans(12.0)).color(css::DIM));
                }
                if conflict {
                    ui.label(RichText::new("The workspace changed. Your draft was preserved. Saving is blocked until you explicitly reconcile it.").font(sans(12.0)).color(hex(0xffb2b2)));
                    if button(ui, "Revalidate draft against latest workspace").clicked() && confirm("Revalidate your draft against the latest workspace? Saving will replace this proposal's saved operations with your draft, not merge them.") {
                        let next = PlanDraft { scope: scope.clone(), plan: p.clone(), original: saved.cloned().unwrap_or(p.clone()), rev: snap.rev };
                        st.drafts.store(next.clone());
                        st.session = Some(next);
                    }
                }
                if let Some(r) = &st.review {
                    for x in &r.errors {
                        ui.label(RichText::new(x).font(sans(12.0)).color(hex(0xffb2b2)));
                    }
                    if !r.prerequisites.is_empty() {
                        widgets::para(ui, format!("Accept prerequisites first: {}", r.prerequisites.join(", ")), 12.5, css::TEXT);
                    }
                    for x in &r.notices {
                        widgets::para(ui, x, 12.5, css::TEXT);
                    }
                }
                let mut baseline = st.baseline;
                if ui.checkbox(&mut baseline, RichText::new("Show before this plan on canvas").font(sans(12.0))).changed() {
                    st.baseline = baseline;
                    out.push(ProposalOut::Canvas);
                }
                widgets::small_faint(ui, "Earlier pending plans are included as context. Updates and deletions stay highlighted in plan view. Canvas editing is disabled during review.");
                if let Some(imp) = st.review.as_ref().and_then(|r| r.impact.as_ref()) {
                    let mut open = false;
                    impact_line(ui, imp, &mut open, false);
                }
                ui.add_space(10.0);
                ui.label(RichText::new(format!("Planned changes ({})", p.ops.len())).font(bold(14.0)).color(css::TEXT));
                let ops = ops_json(&p);
                for (i, op) in ops.iter().enumerate() {
                    egui::Frame::new().stroke(Stroke::new(1.0, hex(0x343c48))).corner_radius(8).inner_margin(Margin::same(12)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(RichText::new(format!("{}. {}", i + 1, operation_label(op))).font(bold(13.0)).color(css::TEXT));
                        ui.horizontal_wrapped(|ui| {
                            if button(ui, "Edit change").clicked() && (st.editing.is_none() || confirm("Discard the unfinished operation form and edit another change?")) {
                                st.editing = Some(OpEditor::new(Some((i, op))));
                            }
                            if button(ui, "Remove planned change").clicked() {
                                let mut next = ops.clone();
                                next.remove(i);
                                if let Ok(o) = to_ops(&next) {
                                    change = Some(Proposal { ops: o, ..p.clone() });
                                }
                                st.editing = None;
                            }
                        });
                        match operation_target(op) {
                            Some(t) => {
                                if link(ui, format!("Focus {t} on canvas")).clicked() {
                                    out.push(ProposalOut::Focus(t));
                                }
                            }
                            None => {
                                widgets::small_faint(ui, "This change may not have an element on the class diagram.");
                            }
                        }
                        if let Some(step) = st.review.as_ref().and_then(|r| r.steps.get(i)) {
                            if let Some(e) = &step.error {
                                ui.label(RichText::new(e).font(sans(12.0)).color(hex(0xffb2b2)));
                            }
                            for c in &step.changes {
                                ui.label(RichText::new(&c.path).font(bold(12.0)).color(css::TEXT));
                                for f in field_diffs(&c.before, &c.after, "") {
                                    ui.label(RichText::new(&f.field).font(sans(12.0)).color(hex(0x9aa9bb)));
                                    let fmt = |v: &Value| match v {
                                        Value::Null => "—".to_string(),
                                        Value::String(s) => s.clone(),
                                        v => serde_json::to_string_pretty(v).unwrap_or_default(),
                                    };
                                    egui::Frame::new().fill(hex(0x371f27)).inner_margin(Margin::same(4)).show(ui, |ui| {
                                        ui.set_width(ui.available_width());
                                        ui.add(egui::Label::new(RichText::new(fmt(&f.before)).font(mono(11.0)).color(hex(0xffb2b2))).wrap());
                                    });
                                    egui::Frame::new().fill(hex(0x17322f)).inner_margin(Margin::same(4)).show(ui, |ui| {
                                        ui.set_width(ui.available_width());
                                        ui.add(egui::Label::new(RichText::new(fmt(&f.after)).font(mono(11.0)).color(hex(0xa5efdd))).wrap());
                                    });
                                }
                            }
                        }
                        egui::CollapsingHeader::new(RichText::new("Full operation and values").font(sans(12.0)).color(css::DIM)).id_salt(("full", i)).show(ui, |ui| {
                            let pre = |ui: &mut Ui, v: &Value| {
                                egui::Frame::new().fill(hex(0x11151b)).inner_margin(Margin::same(8)).show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.add(egui::Label::new(RichText::new(serde_json::to_string_pretty(v).unwrap_or_default()).font(mono(11.0)).color(css::TEXT)).wrap());
                                });
                            };
                            pre(ui, op);
                            if let Some(step) = st.review.as_ref().and_then(|r| r.steps.get(i)) {
                                for c in &step.changes {
                                    ui.label(RichText::new(format!("{} · Before → After", c.path)).font(bold(12.0)));
                                    pre(ui, &c.before);
                                    pre(ui, &c.after);
                                }
                            }
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
                                    change = Some(Proposal { ops: o, ..p.clone() });
                                    st.editing = None;
                                }
                                Err(e) => ed.error = e,
                            }
                        }
                        Some(EditorOut::Cancel) => st.editing = None,
                        None => {}
                    }
                }
                // associated decision (context only)
                egui::Frame::new().stroke(Stroke::new(1.0, hex(0x343c48))).corner_radius(8).inner_margin(Margin::same(12)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new("Associated decision").font(bold(14.0)).color(css::TEXT));
                    let d = p.decision.as_ref().and_then(|id| snap.view.decisions.iter().find(|x| &x.id == id));
                    let cur = d.map(|d| format!("{} · {}", d.id, d.question)).unwrap_or("No associated decision".into());
                    egui::ComboBox::from_id_salt("assoc-decision").selected_text(cur).width(ui.available_width()).truncate().show_ui(ui, |ui| {
                        if ui.selectable_label(p.decision.is_none(), "No associated decision").clicked() {
                            change = Some(Proposal { decision: None, option: None, ..p.clone() });
                        }
                        for x in &snap.view.decisions {
                            if ui.selectable_label(p.decision.as_ref() == Some(&x.id), format!("{} · {}", x.id, x.question)).clicked() {
                                change = Some(Proposal { decision: Some(x.id.clone()), option: None, ..p.clone() });
                            }
                        }
                    });
                    if let Some(d) = d {
                        widgets::rich(ui, &[(&d.question, bold(12.5), css::TEXT), (&format!(" · {:?}", d.state).to_lowercase(), sans(12.5), css::TEXT)]);
                        if let Some(c) = &d.context {
                            widgets::para(ui, c, 12.5, css::TEXT);
                        }
                        let cur = d.options.iter().find(|o| Some(&o.id) == p.option.as_ref()).map(|o| o.label.clone()).unwrap_or("No specific alternative".into());
                        egui::ComboBox::from_id_salt("assoc-option").selected_text(cur).width(ui.available_width()).show_ui(ui, |ui| {
                            if ui.selectable_label(p.option.is_none(), "No specific alternative").clicked() {
                                change = Some(Proposal { option: None, ..p.clone() });
                            }
                            for o in &d.options {
                                if ui.selectable_label(p.option.as_ref() == Some(&o.id), &o.label).clicked() {
                                    change = Some(Proposal { option: Some(o.id.clone()), ..p.clone() });
                                }
                            }
                        });
                        if let Some(s) = d.options.iter().find(|o| Some(&o.id) == p.option.as_ref()).and_then(|o| o.summary.as_ref()) {
                            widgets::para(ui, s, 12.5, css::TEXT);
                        }
                        for (i, a) in d.attempts.iter().enumerate() {
                            egui::CollapsingHeader::new(format!("Defence {} · {}", i + 1, a.verdict.clone().unwrap_or("pending".into()))).id_salt(("def", i)).show(ui, |ui| {
                                widgets::para(ui, &a.defense, 12.5, css::TEXT);
                                widgets::para(ui, a.feedback.clone().unwrap_or_default(), 12.5, css::TEXT);
                            });
                        }
                        widgets::small_faint(ui, "Context only. Plan edits are user amendments, not a new examiner approval.");
                    }
                });
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
                ui.horizontal(|ui| {
                    let can_accept = !st.busy && !dirty && st.session.as_ref().is_some_and(|s| s.rev == snap.rev) && st.editing.is_none() && valid_preview && st.review.as_ref().is_some_and(|r| r.can_accept);
                    if primary_enabled(ui, can_accept, "Accept proposal").clicked() {
                        resolve(st, backend, &p, &scope, snap, &ids, true);
                    }
                    if ui.add_enabled(!st.busy, egui::Button::new(RichText::new("Reject proposal").font(sans(12.0))).fill(css::RAISED).stroke(Stroke::new(1.0, css::LINE)).corner_radius(6)).clicked()
                        && (!st.unsaved() || confirm("Reject this proposal and discard its unsaved draft and operation form?"))
                    {
                        resolve(st, backend, &p, &scope, snap, &ids, false);
                    }
                });
                if dirty {
                    widgets::small_faint(ui, "Save or discard your changes before accepting.");
                }
            });
        });
    });
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
    let _ = (Color32::WHITE, Vec2::ZERO);
}

fn resolve(st: &mut ProposalUi, backend: &Backend, p: &Proposal, scope: &str, snap: &Snapshot, ids: &[String], accept: bool) {
    st.busy = true;
    st.error.clear();
    let (pid, rev, plan, scope, all) = (p.id.clone(), snap.rev, p.clone(), scope.to_string(), ids.to_vec());
    backend.call(
        move |s, _| if accept { crate::commands::accept_proposal(s, pid, Some(rev)) } else { crate::commands::reject_proposal(s, pid, Some(rev)) },
        move |app, r| {
            app.proposal_ui.busy = false;
            match r {
                Ok(()) => {
                    app.proposal_ui.drafts.store(PlanDraft { scope, plan: plan.clone(), original: plan.clone(), rev });
                    app.proposal_ui.editing = None;
                    app.proposal_ui.session = None;
                    let rest: Vec<String> = all.iter().filter(|x| **x != plan.id).cloned().collect();
                    app.selected_proposal = next_proposal_id(&rest, Some(&plan.id), &all);
                }
                Err(e) => app.proposal_ui.error = e,
            }
        },
    );
}
