//! Copy / Cut / Paste / Duplicate of design elements (classes with their relations, and members).
//!
//! The clipboard carries a versioned JSON payload (also put on the system clipboard, so it pastes
//! into another Spide window). Pasting never overwrites: colliding names get a `Copy` suffix,
//! relations between copied classes are remapped, relations to other classes are kept only if
//! that class exists here (and reported otherwise). One paste = one undo entry. Code links are
//! not copied: a copy is a new design element.

use super::app::SpideApp;
use serde::{Deserialize, Serialize};
use serde_json::json;
use spide_core::model::{Attribute, Class, Design, Method, Relation, Status};
use spide_core::{Op, Target};
use std::collections::{BTreeMap, HashSet};

pub const VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Payload {
    pub spide_clipboard: u32,
    pub classes: Vec<Class>,
    /// relations of the copied classes (between them, or to other classes by name)
    pub relations: Vec<Relation>,
    pub attributes: Vec<(String, Attribute)>,
    pub methods: Vec<(String, Method)>,
}

impl Payload {
    pub fn is_empty(&self) -> bool {
        self.classes.is_empty() && self.attributes.is_empty() && self.methods.is_empty()
    }
    pub fn to_text(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }
    pub fn parse(text: &str) -> Option<Payload> {
        serde_json::from_str::<Payload>(text).ok().filter(|p| p.spide_clipboard >= 1 && p.spide_clipboard <= VERSION && !p.is_empty())
    }
}

fn live(s: Status) -> bool {
    matches!(s, Status::Accepted | Status::Overruled)
}

/// What the selection would copy.
pub fn payload(d: &Design, sel: &[Target]) -> Result<Payload, &'static str> {
    if sel.is_empty() {
        return Err("Select classes or members to copy");
    }
    let mut p = Payload { spide_clipboard: VERSION, ..Default::default() };
    for t in sel {
        match t {
            Target::Class { name } => {
                let c = d.classes.iter().find(|c| &c.name == name && live(c.status)).ok_or("Only accepted classes can be copied (not pending or pruned ones)")?;
                p.classes.push(c.clone());
            }
            Target::Attribute { class, name } => {
                if let Some(a) = d.class(class).and_then(|c| c.attributes.iter().find(|a| &a.name == name && live(a.status))) {
                    p.attributes.push((class.clone(), a.clone()));
                }
            }
            Target::Method { class, name } => {
                if let Some(m) = d.class(class).and_then(|c| c.methods.iter().find(|m| &m.name == name && live(m.status))) {
                    p.methods.push((class.clone(), m.clone()));
                }
            }
            _ => return Err("Only classes and their members can be copied"),
        }
    }
    let names: HashSet<&str> = p.classes.iter().map(|c| c.name.as_str()).collect();
    p.relations = d.relations.iter().filter(|r| live(r.status) && names.contains(r.from.as_str())).cloned().collect();
    if p.is_empty() {
        return Err("Nothing in the selection can be copied");
    }
    Ok(p)
}

fn fresh(base: &str, taken: &HashSet<String>) -> String {
    if !taken.contains(base) {
        return base.to_string();
    }
    (1..).map(|n| if n == 1 { format!("{base}Copy") } else { format!("{base}Copy{n}") }).find(|n| !taken.contains(n)).unwrap()
}

/// A copy starts life as a plain user element: no ghost/proposal/code-link state.
fn clean_attr(mut a: Attribute) -> Attribute {
    a.status = Status::Accepted;
    a.proposal = None;
    a.origin = None;
    a
}
fn clean_method(mut m: Method) -> Method {
    m.status = Status::Accepted;
    m.proposal = None;
    m.origin = None;
    m.source = None;
    m.code = None;
    m
}

/// Operations that paste `p` into `d`. `into`: the class members go to (else their own class).
/// Returns the ops, the new class names and notes about what was left out.
pub fn paste_ops(d: &Design, p: &Payload, into: Option<&str>) -> Result<(Vec<Op>, Vec<String>, Vec<String>), String> {
    let mut taken: HashSet<String> = d.classes.iter().map(|c| c.name.clone()).collect();
    let packages: HashSet<&str> = d.packages.iter().filter(|p| p.status != Status::Pruned).map(|p| p.name.as_str()).collect();
    let mut ops = vec![];
    let mut notes = vec![];
    let mut renamed: BTreeMap<String, String> = BTreeMap::new();
    for c in &p.classes {
        let name = fresh(&c.name, &taken);
        taken.insert(name.clone());
        renamed.insert(c.name.clone(), name.clone());
        let package = c.package.clone().filter(|pk| packages.contains(pk.as_str()));
        if c.package.is_some() && package.is_none() {
            notes.push(format!("{name}: package {} doesn't exist here", c.package.as_deref().unwrap_or("")));
        }
        ops.push(json!({"op": "add_class", "name": name, "stereotype": c.stereotype, "package": package, "is_abstract": c.is_abstract,
            "attributes": c.attributes.iter().filter(|a| live(a.status)).cloned().map(clean_attr).collect::<Vec<_>>(),
            "methods": c.methods.iter().filter(|m| live(m.status)).cloned().map(clean_method).collect::<Vec<_>>(),
            "responsibilities": c.responsibilities, "aggregate_root": c.aggregate_root, "external": c.external, "role": c.role}));
    }
    for r in &p.relations {
        let from = renamed.get(&r.from).cloned().unwrap_or_else(|| r.from.clone());
        let to = match renamed.get(&r.to) {
            Some(n) => n.clone(),
            // a reference outside the copy stays explicit: only to a class that exists here
            None if d.classes.iter().any(|c| c.name == r.to && c.status != Status::Pruned) => r.to.clone(),
            None => {
                notes.push(format!("{from} → {}: {} isn't in this design", r.to, r.to));
                continue;
            }
        };
        ops.push(json!({"op": "add_relation", "from": from, "to": to, "kind": r.kind, "label": r.label, "from_mult": r.from_mult, "to_mult": r.to_mult, "directed": r.directed, "interaction": r.interaction}));
    }
    let mut members: BTreeMap<String, HashSet<String>> = BTreeMap::new();
    let target = |own: &str| -> Result<String, String> {
        let t = into.unwrap_or(own);
        d.classes.iter().find(|c| c.name == t && live(c.status)).map(|c| c.name.clone()).ok_or_else(|| format!("Select a class to paste members into ({t} isn't here)"))
    };
    for (own, a) in &p.attributes {
        let class = target(own)?;
        let used = members.entry(class.clone()).or_insert_with(|| d.class(&class).map(|c| c.attributes.iter().map(|a| a.name.clone()).collect()).unwrap_or_default());
        let mut a = clean_attr(a.clone());
        a.name = fresh(&a.name, used);
        used.insert(a.name.clone());
        ops.push(json!({"op": "add_attribute", "class": class, "attribute": a}));
    }
    let mut mnames: BTreeMap<String, HashSet<String>> = BTreeMap::new();
    for (own, m) in &p.methods {
        let class = target(own)?;
        let used = mnames.entry(class.clone()).or_insert_with(|| d.class(&class).map(|c| c.methods.iter().map(|m| m.name.clone()).collect()).unwrap_or_default());
        let mut m = clean_method(m.clone());
        m.name = fresh(&m.name, used);
        used.insert(m.name.clone());
        ops.push(json!({"op": "add_method", "class": class, "method": m}));
    }
    let ops = ops.into_iter().map(|o| serde_json::from_value(o).map_err(|e| e.to_string())).collect::<Result<Vec<Op>, _>>()?;
    Ok((ops, renamed.into_values().collect(), notes))
}

impl SpideApp {
    pub fn clipboard_source(&self) -> Result<Payload, &'static str> {
        let snap = self.snap.as_ref().ok_or("No project is open")?;
        payload(&snap.view, &self.selection)
    }

    pub fn paste_state(&self) -> Result<(), &'static str> {
        if self.app_clipboard.is_some() { Ok(()) } else { Err("Copy classes or members first") }
    }

    pub fn design_clipboard_state(&self, cut: bool) -> super::actions::ActionState {
        let r = if cut { self.clipboard_source().map(|_| ()).and(self.canvas_editable()) } else { self.paste_state().and(self.canvas_editable()) };
        super::actions::ActionState { enabled: r.is_ok(), reason: r.err(), checked: None }
    }

    /// Copy the selection: the payload goes on the system clipboard and stays in the app.
    pub fn design_copy(&mut self) -> bool {
        match self.clipboard_source() {
            Ok(p) => {
                self.ctx.copy_text(p.to_text());
                let n = p.classes.len() + p.attributes.len() + p.methods.len();
                self.app_clipboard = Some(p);
                self.toast("info", format!("Copied {n} element{}", if n == 1 { "" } else { "s" }));
                true
            }
            Err(e) => {
                self.toast("info", e);
                false
            }
        }
    }

    /// Cut = copy, then remove (only if the copy succeeded). ⌘Z restores.
    pub fn design_cut(&mut self) {
        if let Err(e) = self.canvas_editable() {
            return self.toast("info", e);
        }
        if !self.design_copy() {
            return;
        }
        let p = self.app_clipboard.clone().unwrap_or_default();
        let mut ops: Vec<serde_json::Value> = vec![];
        for (c, a) in &p.attributes {
            ops.push(json!({"op": "remove_attribute", "class": c, "name": a.name}));
        }
        for (c, m) in &p.methods {
            ops.push(json!({"op": "remove_method", "class": c, "name": m.name}));
        }
        for c in &p.classes {
            ops.push(json!({"op": "remove_class", "name": c.name}));
        }
        let ops: Vec<Op> = ops.into_iter().filter_map(|o| serde_json::from_value(o).ok()).collect();
        self.selection.clear();
        self.apply_ops(ops, "Cut — paste with ⌘V, or ⌘Z to put it back".into());
    }

    pub fn design_paste(&mut self) {
        if let Err(e) = self.canvas_editable() {
            return self.toast("info", e);
        }
        // another Spide window may have put a payload on the system clipboard
        let p = super::actions::clipboard_text().and_then(|t| Payload::parse(&t)).or_else(|| self.app_clipboard.clone());
        let Some(p) = p else { return self.toast("info", "Copy classes or members first") };
        self.paste(p, "Pasted");
    }

    /// Duplicate the selection in place (the clipboard is left alone).
    pub fn duplicate(&mut self) {
        if let Err(e) = self.canvas_editable() {
            return self.toast("info", e);
        }
        match self.clipboard_source() {
            Ok(p) => self.paste(p, "Duplicated"),
            Err(e) => self.toast("info", e),
        }
    }

    fn paste(&mut self, p: Payload, verb: &str) {
        let Some(snap) = self.snap.clone() else { return };
        let into = match self.selected_target() {
            Some(Target::Class { name }) if p.classes.is_empty() => Some(name),
            _ => None,
        };
        match paste_ops(&snap.view, &p, into.as_deref()) {
            Err(e) => self.toast("error", e),
            Ok((ops, new_classes, notes)) => {
                let mut msg = format!("{verb} (⌘Z to undo)");
                if !notes.is_empty() {
                    msg = format!("{msg}. Left out: {}", notes.join("; "));
                }
                self.selection = new_classes.into_iter().map(|name| Target::Class { name }).collect();
                self.apply_ops(ops, msg);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spide_core::Store;

    fn design() -> Design {
        Design::from_json(&std::fs::read_to_string(format!("{}/../tests/fixtures/library.json", env!("CARGO_MANIFEST_DIR"))).unwrap()).unwrap()
    }

    #[test]
    fn pasting_renames_remaps_relations_and_is_one_undo_entry() {
        let d = design();
        let names: Vec<String> = d.relations.iter().filter(|r| r.status == Status::Accepted).take(1).flat_map(|r| [r.from.clone(), r.to.clone()]).collect();
        let sel: Vec<Target> = names.iter().map(|n| Target::Class { name: n.clone() }).collect();
        let p = payload(&d, &sel).unwrap();
        // survives the system clipboard
        let p = Payload::parse(&p.to_text()).unwrap();
        assert!(Payload::parse("{\"spide_clipboard\": 99, \"classes\": []}").is_none());
        let (ops, new, notes) = paste_ops(&d, &p, None).unwrap();
        assert!(new.iter().all(|n| n.ends_with("Copy")), "{new:?}");
        let mut s = Store::default();
        s.load(d.clone());
        let before = s.design.classes.len();
        s.apply_user(ops).unwrap();
        assert_eq!(s.design.classes.len(), before + new.len());
        // the copied relation connects the copies, not the originals
        let (a, b) = (format!("{}Copy", names[0]), format!("{}Copy", names[1]));
        assert!(s.design.relations.iter().any(|r| r.from == a && r.to == b), "{notes:?}");
        // no code links are copied
        assert!(s.design.class(&a).unwrap().methods.iter().all(|m| m.source.is_none()));
        s.undo();
        assert_eq!(s.design.classes.len(), before, "one paste = one undo entry");
    }

    #[test]
    fn members_paste_into_the_selected_class_and_external_references_must_exist() {
        let d = design();
        let c = d.classes.iter().find(|c| c.status == Status::Accepted && !c.attributes.is_empty()).unwrap();
        let a = &c.attributes[0];
        let p = payload(&d, &[Target::Attribute { class: c.name.clone(), name: a.name.clone() }]).unwrap();
        let (ops, _, _) = paste_ops(&d, &p, Some(&c.name)).unwrap();
        let v = serde_json::to_value(&ops).unwrap();
        assert_eq!(v[0]["attribute"]["name"], format!("{}Copy", a.name), "same class: a fresh name");
        // into a design without the referenced class: the relation is reported, not left dangling
        let mut empty = Design::default();
        empty.classes.push(Class { name: "Other".into(), ..Default::default() });
        let rel = d.relations.iter().find(|r| r.status == Status::Accepted).unwrap();
        let p = payload(&d, &[Target::Class { name: rel.from.clone() }]).unwrap();
        let (ops, _, notes) = paste_ops(&empty, &p, None).unwrap();
        assert!(!notes.is_empty());
        assert!(serde_json::to_value(&ops).unwrap().as_array().unwrap().iter().all(|o| o["op"] != "add_relation" || o["to"] != rel.to));
        assert!(payload(&d, &[Target::Package { name: "x".into() }]).is_err());
    }
}
