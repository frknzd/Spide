//! Lightweight forms: a class's properties (name, stereotype, package, flags, attributes, methods,
//! outgoing relations) and New Element (class, package, external system, container).
//!
//! Saving turns the edits into existing design operations, applied together as ONE user change
//! (one undo entry), validated by the store. These edit the design only — code is never changed.

use super::app::SpideApp;
use super::theme::{bold, css, sans};
use super::widgets::{self, button, primary};
use egui::RichText;
use serde_json::{json, Value};
use spide_core::model::{Attribute, Class, Method, Param, RelationKind, Status};
use spide_core::Op;

const KINDS: [RelationKind; 6] = [RelationKind::Association, RelationKind::Aggregation, RelationKind::Composition, RelationKind::Inheritance, RelationKind::Realization, RelationKind::Dependency];

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct AttrRow {
    pub original: Option<Attribute>,
    pub name: String,
    pub ty: String,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct MethodRow {
    pub original: Option<Method>,
    pub name: String,
    /// `a: T, b: U`
    pub params: String,
    pub returns: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RelRow {
    pub original: Option<(String, RelationKind)>,
    pub to: String,
    pub kind: RelationKind,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ClassEditor {
    /// None: a new class
    pub original: Option<Class>,
    pub name: String,
    pub stereotype: String,
    pub package: String,
    pub new_package: String,
    pub is_abstract: bool,
    pub external: bool,
    pub attributes: Vec<AttrRow>,
    pub methods: Vec<MethodRow>,
    pub relations: Vec<RelRow>,
    /// outgoing relations when the editor opened (rows removed with × are removed from the design)
    pub original_relations: Vec<(String, RelationKind)>,
    pub error: String,
    #[serde(skip)]
    pub saving: bool,
}

fn params_text(m: &Method) -> String {
    m.params.iter().map(|p| if p.ty.is_empty() { p.name.clone() } else { format!("{}: {}", p.name, p.ty) }).collect::<Vec<_>>().join(", ")
}

fn parse_params(s: &str) -> Result<Vec<Param>, String> {
    s.split(',').map(str::trim).filter(|p| !p.is_empty()).map(|p| {
        let (n, t) = p.split_once(':').map(|(n, t)| (n.trim(), t.trim())).unwrap_or((p, ""));
        if n.is_empty() || n.contains(' ') { Err(format!("Parameter “{p}”: write name: Type")) } else { Ok(Param { name: n.into(), ty: t.into() }) }
    }).collect()
}

impl ClassEditor {
    pub fn new_class(package: Option<String>) -> Self {
        Self { name: String::new(), package: package.unwrap_or_default(), ..Default::default() }
    }

    pub fn edit(c: &Class, relations: &[spide_core::model::Relation]) -> Self {
        Self {
            original: Some(c.clone()),
            name: c.name.clone(),
            stereotype: c.stereotype.clone().unwrap_or_default(),
            package: c.package.clone().unwrap_or_default(),
            new_package: String::new(),
            is_abstract: c.is_abstract,
            external: c.external,
            attributes: c.attributes.iter().filter(|a| a.status != Status::Pruned).map(|a| AttrRow { original: Some(a.clone()), name: a.name.clone(), ty: a.ty.clone() }).collect(),
            methods: c.methods.iter().filter(|m| m.status != Status::Pruned).map(|m| MethodRow { original: Some(m.clone()), name: m.name.clone(), params: params_text(m), returns: m.returns.clone().unwrap_or_default() }).collect(),
            relations: relations.iter().filter(|r| r.from == c.name && r.status != Status::Pruned).map(|r| RelRow { original: Some((r.to.clone(), r.kind)), to: r.to.clone(), kind: r.kind }).collect(),
            original_relations: relations.iter().filter(|r| r.from == c.name && r.status != Status::Pruned).map(|r| (r.to.clone(), r.kind)).collect(),
            error: String::new(),
            saving: false,
        }
    }

    /// The design operations for these edits, in an order the store can apply.
    pub fn ops(&self) -> Result<Vec<Op>, String> {
        let name = self.name.trim().to_string();
        if name.is_empty() {
            return Err("A class needs a name.".into());
        }
        let opt = |s: &str| Some(s.trim().to_string()).filter(|s| !s.is_empty());
        let mut v: Vec<Value> = vec![];
        let new_pkg = opt(&self.new_package);
        if let Some(p) = &new_pkg {
            v.push(json!({"op": "add_package", "name": p}));
        }
        let package = new_pkg.or_else(|| opt(&self.package));
        let attr = |r: &AttrRow| -> Result<Attribute, String> {
            if r.name.trim().is_empty() {
                return Err("Every attribute needs a name.".into());
            }
            let mut a = r.original.clone().unwrap_or_default();
            a.name = r.name.trim().into();
            a.ty = r.ty.trim().into();
            Ok(a)
        };
        let method = |r: &MethodRow| -> Result<Method, String> {
            if r.name.trim().is_empty() {
                return Err("Every method needs a name.".into());
            }
            let mut m = r.original.clone().unwrap_or_default();
            m.name = r.name.trim().trim_end_matches("()").into();
            m.params = parse_params(&r.params)?;
            m.returns = opt(&r.returns);
            Ok(m)
        };
        match &self.original {
            None => {
                let attributes = self.attributes.iter().map(attr).collect::<Result<Vec<_>, _>>()?;
                let methods = self.methods.iter().map(method).collect::<Result<Vec<_>, _>>()?;
                v.push(json!({"op": "add_class", "name": name, "stereotype": opt(&self.stereotype), "package": package, "is_abstract": self.is_abstract, "external": self.external, "attributes": attributes, "methods": methods}));
            }
            Some(c) => {
                if c.name != name {
                    v.push(json!({"op": "rename_class", "from": c.name, "to": name}));
                }
                if c.stereotype != opt(&self.stereotype) || c.is_abstract != self.is_abstract || c.external != self.external {
                    v.push(json!({"op": "update_class", "name": name, "stereotype": opt(&self.stereotype).unwrap_or_default(), "is_abstract": self.is_abstract, "external": self.external}));
                }
                if c.package != package {
                    v.push(json!({"op": "move_to_package", "class": name, "package": package}));
                }
                // members: removed, changed, added
                for a in c.attributes.iter().filter(|a| a.status != Status::Pruned) {
                    if !self.attributes.iter().any(|r| r.original.as_ref().is_some_and(|o| o.name == a.name)) {
                        v.push(json!({"op": "remove_attribute", "class": name, "name": a.name}));
                    }
                }
                for r in &self.attributes {
                    let a = attr(r)?;
                    match &r.original {
                        Some(o) if *o != a => v.push(json!({"op": "update_attribute", "class": name, "name": o.name, "attribute": a})),
                        Some(_) => {}
                        None => v.push(json!({"op": "add_attribute", "class": name, "attribute": a})),
                    }
                }
                for m in c.methods.iter().filter(|m| m.status != Status::Pruned) {
                    if !self.methods.iter().any(|r| r.original.as_ref().is_some_and(|o| o.name == m.name)) {
                        v.push(json!({"op": "remove_method", "class": name, "name": m.name}));
                    }
                }
                for r in &self.methods {
                    let m = method(r)?;
                    match &r.original {
                        Some(o) if *o != m => v.push(json!({"op": "update_method", "class": name, "name": o.name, "method": m})),
                        Some(_) => {}
                        None => v.push(json!({"op": "add_method", "class": name, "method": m})),
                    }
                }
            }
        }
        // outgoing relations: removed or changed ones are replaced
        let kept: Vec<(String, RelationKind)> = self.relations.iter().map(|r| (r.to.trim().to_string(), r.kind)).collect();
        for (to, kind) in &self.original_relations {
            if !kept.contains(&(to.clone(), *kind)) {
                v.push(json!({"op": "remove_relation", "from": name, "to": to, "kind": kind}));
            }
        }
        for r in &self.relations {
            let to = r.to.trim();
            if to.is_empty() {
                return Err("Choose the target class of every relation.".into());
            }
            if r.original.as_ref() != Some(&(to.to_string(), r.kind)) {
                v.push(json!({"op": "add_relation", "from": name, "to": to, "kind": r.kind}));
            }
        }
        v.into_iter().map(|o| serde_json::from_value(o).map_err(|e| e.to_string())).collect()
    }
}

/// New Element (beyond classes): the C4 and package operations that exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum NewKind {
    Package,
    System,
    Container,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NewElement {
    pub kind: NewKind,
    pub name: String,
    pub description: String,
    pub technology: String,
    pub error: String,
}

impl NewElement {
    pub fn ops(&self) -> Result<Vec<Op>, String> {
        let name = self.name.trim();
        if name.is_empty() {
            return Err("Enter a name.".into());
        }
        let d = Some(self.description.trim()).filter(|s| !s.is_empty());
        let o = match self.kind {
            NewKind::Package => json!({"op": "add_package", "name": name, "description": d}),
            NewKind::System => json!({"op": "add_system", "name": name, "description": d}),
            NewKind::Container => json!({"op": "add_container", "name": name, "description": d, "technology": Some(self.technology.trim()).filter(|s| !s.is_empty())}),
        };
        Ok(vec![serde_json::from_value(o).map_err(|e| e.to_string())?])
    }
}

impl SpideApp {
    pub fn open_class_editor(&mut self, name: &str) {
        let Some(snap) = self.snap.clone() else { return };
        if let Err(e) = self.canvas_editable() {
            return self.toast("info", e);
        }
        if let Some(c) = snap.view.classes.iter().find(|c| c.name == name && c.status == Status::Accepted) {
            self.class_editor = Some(ClassEditor::edit(c, &snap.view.relations));
        } else {
            self.toast("info", "Only accepted classes can be edited (pending ones are reviewed as plans).");
        }
    }

    pub fn new_class(&mut self) {
        if let Err(e) = self.canvas_editable() {
            return self.toast("info", e);
        }
        let pkg = match self.selected_target() {
            Some(spide_core::Target::Package { name }) => Some(name),
            _ => None,
        };
        self.class_editor = Some(ClassEditor::new_class(pkg));
    }

    pub fn new_element(&mut self, kind: NewKind) {
        if let Err(e) = self.canvas_editable() {
            return self.toast("info", e);
        }
        self.new_element = Some(NewElement { kind, name: String::new(), description: String::new(), technology: String::new(), error: String::new() });
    }

    pub fn property_overlays(&mut self) {
        self.class_editor_overlay();
        self.new_element_overlay();
    }

    fn class_editor_overlay(&mut self) {
        let Some(mut ed) = self.class_editor.take() else { return };
        let Some(snap) = self.snap.clone() else { return };
        let classes: Vec<String> = snap.view.classes.iter().filter(|c| c.status != Status::Pruned).map(|c| c.name.clone()).collect();
        let packages: Vec<String> = snap.view.packages.iter().filter(|p| p.status != Status::Pruned).map(|p| p.name.clone()).collect();
        let mut save = false;
        let mut cancel = false;
        let title = if ed.original.is_some() { "Class Properties" } else { "New Class" };
        egui::Window::new(title).collapsible(false).resizable(true).default_width(520.0).anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO).show(&self.ctx, |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            egui::ScrollArea::vertical().max_height(520.0).show(ui, |ui| {
                egui::Grid::new("class-props").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                    ui.label("Name");
                    ui.add(egui::TextEdit::singleline(&mut ed.name).desired_width(300.0));
                    ui.end_row();
                    ui.label("Stereotype");
                    ui.add(egui::TextEdit::singleline(&mut ed.stereotype).hint_text("entity, service, interface…").desired_width(300.0));
                    ui.end_row();
                    ui.label("Package");
                    ui.horizontal(|ui| {
                        egui::ComboBox::from_id_salt("class-package").selected_text(if ed.package.is_empty() { "none".to_string() } else { ed.package.clone() }).show_ui(ui, |ui| {
                            ui.selectable_value(&mut ed.package, String::new(), "none");
                            for p in &packages {
                                ui.selectable_value(&mut ed.package, p.clone(), p);
                            }
                        });
                        ui.add(egui::TextEdit::singleline(&mut ed.new_package).hint_text("or a new package").desired_width(150.0));
                    });
                    ui.end_row();
                    ui.label("");
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut ed.is_abstract, "abstract");
                        ui.checkbox(&mut ed.external, "external (library/system)");
                    });
                    ui.end_row();
                });
                ui.add_space(4.0);
                ui.label(RichText::new("Attributes").font(bold(12.5)).color(css::TEXT));
                let mut drop = None;
                for (i, a) in ed.attributes.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut a.name).hint_text("name").desired_width(170.0));
                        ui.add(egui::TextEdit::singleline(&mut a.ty).hint_text("type").desired_width(170.0));
                        if ui.small_button("×").on_hover_text("Remove").clicked() {
                            drop = Some(i);
                        }
                    });
                }
                if let Some(i) = drop {
                    ed.attributes.remove(i);
                }
                if button(ui, "+ Attribute").clicked() {
                    ed.attributes.push(AttrRow::default());
                }
                ui.label(RichText::new("Methods").font(bold(12.5)).color(css::TEXT));
                let mut drop = None;
                for (i, m) in ed.methods.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut m.name).hint_text("name").desired_width(120.0));
                        ui.add(egui::TextEdit::singleline(&mut m.params).hint_text("a: T, b: U").desired_width(150.0));
                        ui.add(egui::TextEdit::singleline(&mut m.returns).hint_text("returns").desired_width(80.0));
                        if ui.small_button("×").on_hover_text("Remove").clicked() {
                            drop = Some(i);
                        }
                    });
                }
                if let Some(i) = drop {
                    ed.methods.remove(i);
                }
                if button(ui, "+ Method").clicked() {
                    ed.methods.push(MethodRow::default());
                }
                ui.label(RichText::new("Relations (from this class)").font(bold(12.5)).color(css::TEXT));
                let mut drop = None;
                for (i, r) in ed.relations.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        egui::ComboBox::from_id_salt(("rel-kind", i)).selected_text(r.kind.as_str()).show_ui(ui, |ui| {
                            for k in KINDS {
                                ui.selectable_value(&mut r.kind, k, k.as_str());
                            }
                        });
                        egui::ComboBox::from_id_salt(("rel-to", i)).selected_text(if r.to.is_empty() { "target…" } else { &r.to }).show_ui(ui, |ui| {
                            for c in &classes {
                                ui.selectable_value(&mut r.to, c.clone(), c);
                            }
                        });
                        if ui.small_button("×").on_hover_text("Remove").clicked() {
                            drop = Some(i);
                        }
                    });
                }
                if let Some(i) = drop {
                    ed.relations.remove(i);
                }
                if button(ui, "+ Relation").clicked() {
                    ed.relations.push(RelRow { original: None, to: String::new(), kind: RelationKind::Association });
                }
            });
            ui.separator();
            widgets::para(ui, "Saved as one design change (⌘Z undoes it). This edits the design only — use Implement to change code.", 11.5, css::FAINT);
            if !ed.error.is_empty() {
                widgets::para(ui, &ed.error, 12.0, css::BAD);
            }
            ui.horizontal(|ui| {
                if ui.add_enabled(!ed.saving, egui::Button::new(RichText::new("Save").font(sans(12.5)))).clicked() {
                    save = true;
                }
                if button(ui, "Cancel").clicked() {
                    cancel = true;
                }
            });
        });
        if cancel {
            return;
        }
        if save {
            match ed.ops() {
                Err(e) => ed.error = e,
                Ok(ops) if ops.is_empty() => return,
                Ok(ops) => {
                    ed.saving = true;
                    ed.error.clear();
                    let name = ed.name.trim().to_string();
                    self.backend.call(move |s, _| crate::commands::apply_user_ops(s, ops), move |app, r| match r {
                        Ok(()) => {
                            app.class_editor = None;
                            app.selection = vec![spide_core::Target::Class { name: name.clone() }];
                            app.toast("info", format!("Saved {name} (⌘Z to undo)"));
                        }
                        Err(e) => {
                            if let Some(ed) = app.class_editor.as_mut() {
                                ed.saving = false;
                                ed.error = e;
                            }
                        }
                    });
                }
            }
        }
        self.class_editor = Some(ed);
    }

    fn new_element_overlay(&mut self) {
        let Some(mut el) = self.new_element.take() else { return };
        let mut create = false;
        let mut cancel = false;
        let title = match el.kind {
            NewKind::Package => "New Package",
            NewKind::System => "New External System",
            NewKind::Container => "New Container",
        };
        egui::Window::new(title).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO).show(&self.ctx, |ui| {
            ui.add(egui::TextEdit::singleline(&mut el.name).hint_text("name").desired_width(320.0));
            ui.add(egui::TextEdit::singleline(&mut el.description).hint_text("description (optional)").desired_width(320.0));
            if el.kind == NewKind::Container {
                ui.add(egui::TextEdit::singleline(&mut el.technology).hint_text("technology (optional)").desired_width(320.0));
            }
            if !el.error.is_empty() {
                widgets::para(ui, &el.error, 12.0, css::BAD);
            }
            ui.horizontal(|ui| {
                if primary(ui, "Create").clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    create = true;
                }
                if button(ui, "Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    cancel = true;
                }
            });
        });
        if cancel {
            return;
        }
        if create {
            match el.ops() {
                Err(e) => el.error = e,
                Ok(ops) => {
                    let msg = format!("Created {} (⌘Z to undo)", el.name.trim());
                    self.apply_ops(ops, msg);
                    return;
                }
            }
        }
        self.new_element = Some(el);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_edits_become_one_ordered_batch_of_existing_ops() {
        let c = Class { name: "Loan".into(), attributes: vec![Attribute { name: "due".into(), ty: "Date".into(), ..Default::default() }], methods: vec![Method { name: "renew".into(), ..Default::default() }], ..Default::default() };
        let mut ed = ClassEditor::edit(&c, &[]);
        assert!(ed.ops().unwrap().is_empty(), "no edits, no ops");
        ed.name = "Borrowing".into();
        ed.attributes[0].ty = "LocalDate".into();
        ed.methods.clear();
        ed.methods.push(MethodRow { name: "extend".into(), params: "days: int".into(), returns: "Date".into(), ..Default::default() });
        ed.new_package = "lending".into();
        ed.relations.push(RelRow { original: None, to: "Member".into(), kind: RelationKind::Association });
        ed.original_relations.push(("Book".into(), RelationKind::Dependency)); // was there, row removed
        let ops = serde_json::to_value(ed.ops().unwrap()).unwrap();
        let names: Vec<&str> = ops.as_array().unwrap().iter().map(|o| o["op"].as_str().unwrap()).collect();
        assert_eq!(names, ["add_package", "rename_class", "move_to_package", "update_attribute", "remove_method", "add_method", "remove_relation", "add_relation"]);
        assert_eq!(ops[5]["method"]["params"][0]["type"], "int");
        ed.methods[0].params = "two words: int".into();
        assert!(ed.ops().is_err());
        assert!(ClassEditor::new_class(None).ops().is_err(), "a name is required");
    }
}
