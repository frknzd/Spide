//! Typed design operations. Agents only ever produce these; the store decides
//! whether they become ghosts (proposals) or real changes.

use crate::model::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Op {
    AddClass {
        name: String,
        #[serde(default)]
        stereotype: Option<String>,
        #[serde(default)]
        package: Option<String>,
        #[serde(default)]
        is_abstract: bool,
        #[serde(default)]
        attributes: Vec<Attribute>,
        #[serde(default)]
        methods: Vec<Method>,
        #[serde(default)]
        responsibilities: Vec<String>,
        #[serde(default)]
        rationale: Option<String>,
        #[serde(default)]
        aggregate_root: bool,
    },
    UpdateClass {
        name: String,
        #[serde(default)]
        stereotype: Option<String>,
        #[serde(default)]
        package: Option<String>,
        #[serde(default)]
        is_abstract: Option<bool>,
        #[serde(default)]
        responsibilities: Option<Vec<String>>,
        #[serde(default)]
        rationale: Option<String>,
        #[serde(default)]
        aggregate_root: Option<bool>,
    },
    RenameClass { from: String, to: String },
    RemoveClass { name: String },
    AddAttribute { class: String, attribute: Attribute },
    UpdateAttribute { class: String, name: String, attribute: Attribute },
    RemoveAttribute { class: String, name: String },
    AddMethod { class: String, method: Method },
    UpdateMethod { class: String, name: String, method: Method },
    RemoveMethod { class: String, name: String },
    AddRelation {
        from: String,
        to: String,
        kind: RelationKind,
        #[serde(default)]
        label: Option<String>,
        #[serde(default)]
        from_mult: Option<String>,
        #[serde(default)]
        to_mult: Option<String>,
        #[serde(default)]
        directed: Option<bool>,
        #[serde(default)]
        rationale: Option<String>,
        #[serde(default)]
        interaction: Option<Interaction>,
    },
    UpdateRelation {
        from: String,
        to: String,
        kind: RelationKind,
        #[serde(default)]
        label: Option<String>,
        #[serde(default)]
        from_mult: Option<String>,
        #[serde(default)]
        to_mult: Option<String>,
        #[serde(default)]
        directed: Option<bool>,
        #[serde(default)]
        interaction: Option<Interaction>,
    },
    RemoveRelation {
        from: String,
        to: String,
        #[serde(default)]
        kind: Option<RelationKind>,
    },
    AddPackage {
        name: String,
        #[serde(default)]
        layer: Option<Layer>,
        #[serde(default)]
        description: Option<String>,
    },
    SetLayer { package: String, layer: Option<Layer> },
    RemovePackage { name: String },
    MoveToPackage { class: String, package: Option<String> },
    AddForce {
        name: String,
        #[serde(default)]
        kind: ForceKind,
        #[serde(default)]
        description: Option<String>,
        #[serde(default)]
        priority: Option<u8>,
    },
    UpdateForce {
        name: String,
        #[serde(default)]
        rename: Option<String>,
        #[serde(default)]
        kind: Option<ForceKind>,
        #[serde(default)]
        description: Option<String>,
        #[serde(default)]
        priority: Option<u8>,
    },
    RemoveForce { name: String },
    AddChangeScenario {
        name: String,
        #[serde(default)]
        description: Option<String>,
    },
    RemoveChangeScenario { name: String },
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("{0}")]
pub struct OpError(pub String);

fn err<T>(msg: impl Into<String>) -> Result<T, OpError> {
    Err(OpError(msg.into()))
}

/// How an op is applied.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Mode<'a> {
    /// Real change with the given status for new elements (Accepted / Overruled) and their
    /// origin (decision id, proposal id, `user`, `import`).
    Commit(Status, Option<&'a str>),
    /// Ghost preview belonging to a proposal id.
    Ghost(&'a str),
}

impl Mode<'_> {
    fn status(self) -> Status {
        match self {
            Mode::Commit(s, _) => s,
            Mode::Ghost(_) => Status::Proposed,
        }
    }
    fn proposal(self) -> Option<String> {
        match self {
            Mode::Commit(..) => None,
            Mode::Ghost(p) => Some(p.to_string()),
        }
    }
    fn origin(self) -> Option<String> {
        match self {
            Mode::Commit(_, o) => o.map(String::from),
            Mode::Ghost(_) => None,
        }
    }
    fn is_ghost(self) -> bool {
        matches!(self, Mode::Ghost(_))
    }
}

fn valid_ident(name: &str) -> bool {
    !name.trim().is_empty() && !name.contains(['\n', '"', '{', '}'])
}

/// An existing class counts as "present" unless it's pruned.
fn present(d: &Design, name: &str) -> bool {
    d.class(name).is_some_and(|c| c.status != Status::Pruned)
}

impl Op {
    pub fn summary(&self) -> String {
        match self {
            Op::AddClass { name, .. } => format!("add class {name}"),
            Op::UpdateClass { name, .. } => format!("update class {name}"),
            Op::RenameClass { from, to } => format!("rename {from} → {to}"),
            Op::RemoveClass { name } => format!("remove class {name}"),
            Op::AddAttribute { class, attribute } => format!("add {class}.{}", attribute.name),
            Op::UpdateAttribute { class, name, .. } => format!("update {class}.{name}"),
            Op::RemoveAttribute { class, name } => format!("remove {class}.{name}"),
            Op::AddMethod { class, method } => format!("add {class}.{}()", method.name),
            Op::UpdateMethod { class, name, .. } => format!("update {class}.{name}()"),
            Op::RemoveMethod { class, name } => format!("remove {class}.{name}()"),
            Op::AddRelation { from, to, kind, .. } => format!("{from} —{}→ {to}", kind.as_str()),
            Op::UpdateRelation { from, to, kind, .. } => format!("update {from} —{}→ {to}", kind.as_str()),
            Op::RemoveRelation { from, to, .. } => format!("remove {from} → {to}"),
            Op::AddPackage { name, .. } => format!("add package {name}"),
            Op::SetLayer { package, layer } => format!("layer of {package} = {layer:?}"),
            Op::RemovePackage { name } => format!("remove package {name}"),
            Op::MoveToPackage { class, package } => {
                format!("move {class} → {}", package.as_deref().unwrap_or("(none)"))
            }
            Op::AddForce { name, kind, .. } => format!("add {} \"{name}\"", kind.as_str()),
            Op::UpdateForce { name, .. } => format!("update force \"{name}\""),
            Op::RemoveForce { name } => format!("remove force \"{name}\""),
            Op::AddChangeScenario { name, .. } => format!("add change scenario \"{name}\""),
            Op::RemoveChangeScenario { name } => format!("remove change scenario \"{name}\""),
        }
    }

    /// Class names this op touches (for previews / highlighting).
    pub fn touched_classes(&self) -> Vec<String> {
        match self {
            Op::AddClass { name, .. } | Op::UpdateClass { name, .. } | Op::RemoveClass { name } => vec![name.clone()],
            Op::RenameClass { to, .. } => vec![to.clone()],
            Op::AddAttribute { class, .. }
            | Op::UpdateAttribute { class, .. }
            | Op::RemoveAttribute { class, .. }
            | Op::AddMethod { class, .. }
            | Op::UpdateMethod { class, .. }
            | Op::RemoveMethod { class, .. }
            | Op::MoveToPackage { class, .. } => vec![class.clone()],
            Op::AddRelation { from, to, .. } | Op::UpdateRelation { from, to, .. } | Op::RemoveRelation { from, to, .. } => {
                vec![from.clone(), to.clone()]
            }
            Op::AddPackage { .. } | Op::SetLayer { .. } | Op::RemovePackage { .. } => vec![],
            Op::AddForce { .. } | Op::UpdateForce { .. } | Op::RemoveForce { .. } => vec![],
            Op::AddChangeScenario { .. } | Op::RemoveChangeScenario { .. } => vec![],
        }
    }

    pub fn apply(&self, d: &mut Design, mode: Mode) -> Result<(), OpError> {
        let st = mode.status();
        let prop = mode.proposal();
        let origin = mode.origin();
        match self {
            Op::AddClass { name, stereotype, package, is_abstract, attributes, methods, responsibilities, rationale, aggregate_root } => {
                if !valid_ident(name) {
                    return err(format!("invalid class name {name:?}"));
                }
                if present(d, name) {
                    return err(format!("class {name} already exists; use update_class / add_attribute / add_method"));
                }
                d.classes.retain(|c| c.name != *name); // replace a pruned one
                if let Some(p) = package {
                    ensure_package(d, p, mode);
                }
                let mark_attr = |a: &Attribute| Attribute { status: st, proposal: prop.clone(), origin: origin.clone(), ..a.clone() };
                let mark_m = |m: &Method| Method { status: st, proposal: prop.clone(), origin: origin.clone(), ..m.clone() };
                check_unique(attributes.iter().map(|a| &a.name), "attribute")?;
                check_unique(methods.iter().map(|m| (&m.name, m.params.len())), "method")?;
                d.classes.push(Class {
                    name: name.clone(),
                    stereotype: stereotype.clone().filter(|s| !s.is_empty()),
                    package: package.clone(),
                    is_abstract: *is_abstract || stereotype.as_deref() == Some("abstract"),
                    attributes: attributes.iter().map(mark_attr).collect(),
                    methods: methods.iter().map(mark_m).collect(),
                    responsibilities: responsibilities.clone(),
                    rationale: rationale.clone(),
                    aggregate_root: *aggregate_root,
                    status: st,
                    proposal: prop,
                    feedback: None,
                    origin,
                });
            }
            Op::UpdateClass { name, stereotype, package, is_abstract, responsibilities, rationale, aggregate_root } => {
                if let Some(p) = package {
                    if !present(d, name) {
                        return err(format!("unknown class {name}"));
                    }
                    ensure_package(d, p, mode);
                }
                let c = live_class_mut(d, name)?;
                if let Some(s) = stereotype {
                    c.stereotype = if s.is_empty() { None } else { Some(s.clone()) };
                }
                if let Some(p) = package {
                    c.package = if p.is_empty() { None } else { Some(p.clone()) };
                }
                if let Some(a) = is_abstract {
                    c.is_abstract = *a;
                }
                if let Some(r) = responsibilities {
                    c.responsibilities = r.clone();
                }
                if let Some(r) = rationale {
                    c.rationale = Some(r.clone());
                }
                if let Some(a) = aggregate_root {
                    c.aggregate_root = *a;
                }
                mark_changed(c, mode);
            }
            Op::RenameClass { from, to } => {
                if !valid_ident(to) {
                    return err(format!("invalid class name {to:?}"));
                }
                if present(d, to) {
                    return err(format!("class {to} already exists"));
                }
                live_class_mut(d, from)?;
                rename_everywhere(d, from, to);
                let c = d.class_mut(to).expect("renamed");
                mark_changed(c, mode);
            }
            Op::RemoveClass { name } => {
                live_class_mut(d, name)?;
                if mode.is_ghost() {
                    let c = d.class_mut(name).unwrap();
                    c.status = Status::Removing;
                    c.proposal = prop.clone();
                    for r in d.relations.iter_mut().filter(|r| r.from == *name || r.to == *name) {
                        r.status = Status::Removing;
                        r.proposal = prop.clone();
                    }
                } else {
                    d.classes.retain(|c| c.name != *name);
                    d.relations.retain(|r| r.from != *name && r.to != *name);
                }
            }
            Op::AddAttribute { class, attribute } => {
                if !valid_ident(&attribute.name) {
                    return err("attribute needs a name");
                }
                let c = live_class_mut(d, class)?;
                if c.attributes.iter().any(|a| a.name == attribute.name && a.status != Status::Pruned) {
                    return err(format!("{class}.{} already exists", attribute.name));
                }
                c.attributes.retain(|a| a.name != attribute.name);
                c.attributes.push(Attribute { status: st, proposal: prop, origin, ..attribute.clone() });
            }
            Op::UpdateAttribute { class, name, attribute } => {
                let c = live_class_mut(d, class)?;
                let a = c
                    .attributes
                    .iter_mut()
                    .find(|a| a.name == *name && a.status != Status::Pruned)
                    .ok_or_else(|| OpError(format!("unknown attribute {class}.{name}")))?;
                let keep = if mode.is_ghost() { st } else { a.status };
                let keep_origin = a.origin.clone();
                *a = Attribute { status: keep, proposal: prop, origin: keep_origin, ..attribute.clone() };
                if a.name.is_empty() {
                    a.name = name.clone();
                }
            }
            Op::RemoveAttribute { class, name } => {
                let c = live_class_mut(d, class)?;
                let idx = c
                    .attributes
                    .iter()
                    .position(|a| a.name == *name && a.status != Status::Pruned)
                    .ok_or_else(|| OpError(format!("unknown attribute {class}.{name}")))?;
                if mode.is_ghost() {
                    c.attributes[idx].status = Status::Removing;
                    c.attributes[idx].proposal = prop;
                } else {
                    c.attributes.remove(idx);
                }
            }
            Op::AddMethod { class, method } => {
                if !valid_ident(&method.name) {
                    return err("method needs a name");
                }
                let c = live_class_mut(d, class)?;
                if c.methods.iter().any(|m| {
                    m.name == method.name && m.params.len() == method.params.len() && m.status != Status::Pruned
                }) {
                    return err(format!("{class}.{}() already exists; use update_method", method.name));
                }
                c.methods.retain(|m| !(m.name == method.name && m.params.len() == method.params.len()));
                c.methods.push(Method { status: st, proposal: prop, origin, ..method.clone() });
            }
            Op::UpdateMethod { class, name, method } => {
                let c = live_class_mut(d, class)?;
                let m = c
                    .methods
                    .iter_mut()
                    .find(|m| m.name == *name && m.status != Status::Pruned)
                    .ok_or_else(|| OpError(format!("unknown method {class}.{name}()")))?;
                let keep = if mode.is_ghost() { st } else { m.status };
                let keep_origin = m.origin.clone();
                *m = Method { status: keep, proposal: prop, origin: keep_origin, ..method.clone() };
                if m.name.is_empty() {
                    m.name = name.clone();
                }
            }
            Op::RemoveMethod { class, name } => {
                let c = live_class_mut(d, class)?;
                let idx = c
                    .methods
                    .iter()
                    .position(|m| m.name == *name && m.status != Status::Pruned)
                    .ok_or_else(|| OpError(format!("unknown method {class}.{name}()")))?;
                if mode.is_ghost() {
                    c.methods[idx].status = Status::Removing;
                    c.methods[idx].proposal = prop;
                } else {
                    c.methods.remove(idx);
                }
            }
            Op::AddRelation { from, to, kind, label, from_mult, to_mult, directed, rationale, interaction } => {
                for n in [from, to] {
                    if !present(d, n) {
                        return err(format!("unknown class {n} (add it first, in the same batch is fine)"));
                    }
                }
                if from == to && matches!(kind, RelationKind::Inheritance | RelationKind::Realization) {
                    return err(format!("{from} cannot {} itself", kind.as_str()));
                }
                let id = relation_id(from, to, *kind);
                if d.relations.iter().any(|r| r.id == id && r.status != Status::Pruned) {
                    return err(format!("relation {id} already exists; use update_relation"));
                }
                d.relations.retain(|r| r.id != id);
                d.relations.push(Relation {
                    id,
                    from: from.clone(),
                    to: to.clone(),
                    kind: *kind,
                    label: label.clone().filter(|s| !s.is_empty()),
                    from_mult: from_mult.clone().filter(|s| !s.is_empty()),
                    to_mult: to_mult.clone().filter(|s| !s.is_empty()),
                    directed: directed.unwrap_or(true),
                    interaction: interaction.unwrap_or_default(),
                    rationale: rationale.clone(),
                    status: st,
                    proposal: prop,
                    feedback: None,
                    origin,
                });
            }
            Op::UpdateRelation { from, to, kind, label, from_mult, to_mult, directed, interaction } => {
                let id = relation_id(from, to, *kind);
                let r = d
                    .relations
                    .iter_mut()
                    .find(|r| r.id == id && r.status != Status::Pruned)
                    .ok_or_else(|| OpError(format!("unknown relation {id}")))?;
                if let Some(l) = label {
                    r.label = Some(l.clone()).filter(|s| !s.is_empty());
                }
                if let Some(m) = from_mult {
                    r.from_mult = Some(m.clone()).filter(|s| !s.is_empty());
                }
                if let Some(m) = to_mult {
                    r.to_mult = Some(m.clone()).filter(|s| !s.is_empty());
                }
                if let Some(dir) = directed {
                    r.directed = *dir;
                }
                if let Some(i) = interaction {
                    r.interaction = *i;
                }
                if mode.is_ghost() {
                    r.status = Status::Proposed;
                    r.proposal = prop;
                }
            }
            Op::RemoveRelation { from, to, kind } => {
                let matches = |r: &Relation| {
                    r.from == *from && r.to == *to && kind.is_none_or(|k| r.kind == k) && r.status != Status::Pruned
                };
                if !d.relations.iter().any(matches) {
                    return err(format!("no relation {from} → {to}"));
                }
                if mode.is_ghost() {
                    for r in d.relations.iter_mut().filter(|r| matches(r)) {
                        r.status = Status::Removing;
                        r.proposal = prop.clone();
                    }
                } else {
                    d.relations.retain(|r| !matches(r));
                }
            }
            Op::AddPackage { name, layer, description } => {
                if !valid_ident(name) {
                    return err("package needs a name");
                }
                if let Some(p) = d.packages.iter_mut().find(|p| p.name == *name) {
                    if p.status != Status::Pruned {
                        return err(format!("package {name} already exists; use set_layer"));
                    }
                }
                d.packages.retain(|p| p.name != *name);
                d.packages.push(Package {
                    name: name.clone(),
                    layer: *layer,
                    description: description.clone(),
                    container: None,
                    status: st,
                    proposal: prop,
                    origin,
                });
            }
            Op::SetLayer { package, layer } => {
                let p = d
                    .packages
                    .iter_mut()
                    .find(|p| p.name == *package && p.status != Status::Pruned)
                    .ok_or_else(|| OpError(format!("unknown package {package}")))?;
                p.layer = *layer;
            }
            Op::RemovePackage { name } => {
                if !d.packages.iter().any(|p| p.name == *name && p.status != Status::Pruned) {
                    return err(format!("unknown package {name}"));
                }
                if mode.is_ghost() {
                    for p in d.packages.iter_mut().filter(|p| p.name == *name) {
                        p.status = Status::Removing;
                        p.proposal = prop.clone();
                    }
                } else {
                    d.packages.retain(|p| p.name != *name);
                    for c in d.classes.iter_mut().filter(|c| c.package.as_deref() == Some(name)) {
                        c.package = None;
                    }
                }
            }
            Op::MoveToPackage { class, package } => {
                if let Some(p) = package {
                    if !present(d, class) {
                        return err(format!("unknown class {class}"));
                    }
                    ensure_package(d, p, mode);
                }
                let c = live_class_mut(d, class)?;
                c.package = package.clone().filter(|s| !s.is_empty());
                mark_changed(c, mode);
            }
            Op::AddForce { name, kind, description, priority } => {
                if !valid_ident(name) {
                    return err("force needs a name");
                }
                if d.forces.iter().any(|f| f.name == *name && f.status != Status::Pruned) {
                    return err(format!("force \"{name}\" already exists; use update_force"));
                }
                d.forces.retain(|f| f.name != *name);
                d.forces.push(Force {
                    name: name.clone(),
                    kind: *kind,
                    description: description.clone().filter(|s| !s.is_empty()),
                    priority: priority.unwrap_or(2).clamp(1, 3),
                    status: st,
                    proposal: prop,
                });
            }
            Op::UpdateForce { name, rename, kind, description, priority } => {
                if let Some(r) = rename.as_ref().filter(|r| *r != name) {
                    if !valid_ident(r) {
                        return err("force needs a name");
                    }
                    if d.forces.iter().any(|f| f.name == *r && f.status != Status::Pruned) {
                        return err(format!("force \"{r}\" already exists"));
                    }
                }
                let f = d
                    .forces
                    .iter_mut()
                    .find(|f| f.name == *name && f.status != Status::Pruned)
                    .ok_or_else(|| OpError(format!("unknown force \"{name}\"")))?;
                if let Some(r) = rename.as_ref().filter(|r| !r.is_empty()) {
                    f.name = r.clone();
                }
                if let Some(k) = kind {
                    f.kind = *k;
                }
                if let Some(desc) = description {
                    f.description = Some(desc.clone()).filter(|s| !s.is_empty());
                }
                if let Some(p) = priority {
                    f.priority = (*p).clamp(1, 3);
                }
                if mode.is_ghost() {
                    f.proposal = prop;
                }
                if let Some(r) = rename.as_ref().filter(|r| !r.is_empty() && *r != name) {
                    // keep decision trade-offs pointing at the renamed force
                    for o in d.decisions.iter_mut().flat_map(|x| x.options.iter_mut()) {
                        for t in o.tradeoffs.iter_mut().filter(|t| t.force == *name) {
                            t.force = r.clone();
                        }
                    }
                }
            }
            Op::AddChangeScenario { name, description } => {
                if !valid_ident(name) {
                    return err("change scenario needs a name");
                }
                if d.change_scenarios.iter().any(|c| c.name == *name && c.status != Status::Pruned) {
                    return err(format!("change scenario \"{name}\" already exists"));
                }
                d.change_scenarios.retain(|c| c.name != *name);
                d.change_scenarios.push(ChangeScenario {
                    name: name.clone(),
                    description: description.clone().filter(|s| !s.is_empty()),
                    status: st,
                    proposal: prop,
                    ..Default::default()
                });
            }
            Op::RemoveChangeScenario { name } => {
                let idx = d
                    .change_scenarios
                    .iter()
                    .position(|c| c.name == *name && c.status != Status::Pruned)
                    .ok_or_else(|| OpError(format!("unknown change scenario \"{name}\"")))?;
                if mode.is_ghost() {
                    d.change_scenarios[idx].status = Status::Removing;
                    d.change_scenarios[idx].proposal = prop;
                } else {
                    d.change_scenarios.remove(idx);
                }
            }
            Op::RemoveForce { name } => {
                let idx = d
                    .forces
                    .iter()
                    .position(|f| f.name == *name && f.status != Status::Pruned)
                    .ok_or_else(|| OpError(format!("unknown force \"{name}\"")))?;
                if mode.is_ghost() {
                    d.forces[idx].status = Status::Removing;
                    d.forces[idx].proposal = prop;
                } else {
                    d.forces.remove(idx);
                }
            }
        }
        Ok(())
    }
}

fn check_unique<T: PartialEq + std::fmt::Debug>(items: impl Iterator<Item = T>, what: &str) -> Result<(), OpError> {
    let v: Vec<T> = items.collect();
    for (i, a) in v.iter().enumerate() {
        if v[..i].contains(a) {
            return err(format!("duplicate {what} {a:?}"));
        }
    }
    Ok(())
}

fn live_class_mut<'a>(d: &'a mut Design, name: &str) -> Result<&'a mut Class, OpError> {
    d.classes
        .iter_mut()
        .find(|c| c.name == name && c.status != Status::Pruned)
        .ok_or_else(|| OpError(format!("unknown class {name}")))
}

fn mark_changed(c: &mut Class, mode: Mode) {
    if let Mode::Ghost(p) = mode {
        if c.status.is_live() {
            // keep the class live but remember that a proposal touches it
            c.proposal = Some(p.to_string());
        }
    }
}

fn ensure_package(d: &mut Design, name: &str, mode: Mode) {
    if name.is_empty() || d.packages.iter().any(|p| p.name == name && p.status != Status::Pruned) {
        return;
    }
    d.packages.retain(|p| p.name != name);
    d.packages.push(Package {
        name: name.to_string(),
        layer: guess_layer(name),
        description: None,
        container: None,
        status: mode.status(),
        proposal: mode.proposal(),
        origin: mode.origin(),
    });
}

/// Guess a layer from common package names.
pub fn guess_layer(name: &str) -> Option<Layer> {
    let n = name.to_lowercase();
    let last = n.rsplit(['.', '/', ':']).next().unwrap_or(&n);
    match last {
        "ui" | "view" | "views" | "presentation" | "web" | "controller" | "controllers" | "api" | "cli" | "gui" => {
            Some(Layer::Ui)
        }
        "application" | "app" | "service" | "services" | "usecase" | "usecases" => Some(Layer::Application),
        "domain" | "model" | "models" | "core" | "entities" => Some(Layer::Domain),
        "infrastructure" | "infra" | "persistence" | "db" | "data" | "adapters" | "repository" | "repositories" => {
            Some(Layer::Infrastructure)
        }
        _ => None,
    }
}

fn rename_everywhere(d: &mut Design, from: &str, to: &str) {
    for c in d.classes.iter_mut() {
        if c.name == from {
            c.name = to.to_string();
        }
        for m in c.methods.iter_mut() {
            for call in m.calls.iter_mut() {
                if let Some(rest) = call.strip_prefix(&format!("{from}.")) {
                    *call = format!("{to}.{rest}");
                }
            }
        }
    }
    for r in d.relations.iter_mut() {
        if r.from == from {
            r.from = to.to_string();
        }
        if r.to == from {
            r.to = to.to_string();
        }
        r.id = relation_id(&r.from, &r.to, r.kind);
    }
    for uc in d.use_cases.use_cases.iter_mut() {
        for c in uc.classes.iter_mut() {
            if c == from {
                *c = to.to_string();
            }
        }
    }
    for s in d.sequences.iter_mut() {
        for p in s.participants.iter_mut() {
            if p == from {
                *p = to.to_string();
            }
        }
        for m in s.messages.iter_mut() {
            if m.from == from {
                m.from = to.to_string();
            }
            if m.to == from {
                m.to = to.to_string();
            }
        }
    }
    for view in d.layout.values_mut() {
        if let Some(p) = view.remove(from) {
            view.insert(to.to_string(), p);
        }
    }
}

/// Apply a batch atomically on a copy; returns the new design.
pub fn apply_all(d: &Design, ops: &[Op], mode: Mode) -> Result<Design, OpError> {
    let mut next = d.clone();
    for (i, op) in ops.iter().enumerate() {
        op.apply(&mut next, mode).map_err(|e| OpError(format!("op #{} ({}): {}", i + 1, op.summary(), e.0)))?;
    }
    Ok(next)
}
