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
        #[serde(default)]
        external: bool,
        #[serde(default)]
        role: Option<String>,
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
        #[serde(default)]
        external: Option<bool>,
        /// "" clears it
        #[serde(default)]
        role: Option<String>,
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
        #[serde(default)]
        external: bool,
        #[serde(default)]
        system: Option<String>,
    },
    UpdatePackage {
        name: String,
        #[serde(default)]
        description: Option<String>,
        #[serde(default)]
        external: Option<bool>,
        /// C4 external system ("" clears it)
        #[serde(default)]
        system: Option<String>,
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
    AddSystem {
        name: String,
        #[serde(default)]
        description: Option<String>,
    },
    RemoveSystem { name: String },
    AddContainer {
        name: String,
        #[serde(default)]
        technology: Option<String>,
        #[serde(default)]
        description: Option<String>,
        #[serde(default)]
        replicas: Option<u32>,
        #[serde(default)]
        datastore: bool,
    },
    UpdateContainer {
        name: String,
        #[serde(default)]
        technology: Option<String>,
        #[serde(default)]
        description: Option<String>,
        #[serde(default)]
        replicas: Option<u32>,
        #[serde(default)]
        datastore: Option<bool>,
    },
    RemoveContainer { name: String },
    /// Run a package in a container (`container: null` unassigns).
    AssignContainer { package: String, container: Option<String> },
    AddLink {
        from: String,
        to: String,
        #[serde(default)]
        interaction: Option<Interaction>,
        #[serde(default)]
        protocol: Option<String>,
        #[serde(default)]
        description: Option<String>,
    },
    RemoveLink { from: String, to: String },
    /// Spide Project: trace a class (or one of its methods) to a code symbol: the qualified name
    /// (`org.x.Foo`, `shop.orders.Order`, `shop.orders` for a module), or `Owner#name(param types)`
    /// for a method/function. `language` defaults to the project's (Java for schema-1 designs).
    LinkCode {
        class: String,
        #[serde(default)]
        method: Option<String>,
        symbol: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        language: Option<String>,
    },
    UnlinkCode {
        class: String,
        #[serde(default)]
        method: Option<String>,
    },
    /// Replace the domain model (e.g. edited by colleagues in Lucid). Pruned concepts survive.
    SetDomainModel { model: DomainModel },
    /// Replace the use case diagram. Pruned actors/use cases survive.
    SetUseCases { model: UseCaseModel },
    /// Create or replace one sequence diagram (by name).
    SetSequence { sequence: Sequence },
    /// Pin canvas positions in a view (`class`, `domain`): element → position.
    SetLayout { view: String, positions: std::collections::BTreeMap<String, Pos> },
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
            Op::UpdatePackage { name, external, .. } => match external {
                Some(true) => format!("mark {name} external"),
                Some(false) => format!("mark {name} as ours"),
                None => format!("update package {name}"),
            },
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
            Op::AddSystem { name, .. } => format!("add external system {name}"),
            Op::RemoveSystem { name } => format!("remove external system {name}"),
            Op::AddContainer { name, .. } => format!("add container {name}"),
            Op::UpdateContainer { name, .. } => format!("update container {name}"),
            Op::RemoveContainer { name } => format!("remove container {name}"),
            Op::AssignContainer { package, container } => format!("run {package} in {}", container.as_deref().unwrap_or("(no container)")),
            Op::AddLink { from, to, .. } => format!("link {from} → {to}"),
            Op::RemoveLink { from, to } => format!("remove link {from} → {to}"),
            Op::LinkCode { class, method, symbol, .. } => match method {
                Some(m) => format!("link {class}.{m}() → {symbol}"),
                None => format!("link {class} → {symbol}"),
            },
            Op::UnlinkCode { class, method } => match method {
                Some(m) => format!("unlink {class}.{m}()"),
                None => format!("unlink {class}"),
            },
            Op::SetDomainModel { model } => format!("update the domain model ({} concepts)", model.concepts.len()),
            Op::SetUseCases { model } => format!("update the use cases ({} use cases)", model.use_cases.len()),
            Op::SetSequence { sequence } => format!("update sequence \"{}\"", sequence.name),
            Op::SetLayout { view, positions } => format!("move {} element{} in the {view} view", positions.len(), if positions.len() == 1 { "" } else { "s" }),
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
            | Op::MoveToPackage { class, .. }
            | Op::LinkCode { class, .. }
            | Op::UnlinkCode { class, .. } => vec![class.clone()],
            Op::AddRelation { from, to, .. } | Op::UpdateRelation { from, to, .. } | Op::RemoveRelation { from, to, .. } => {
                vec![from.clone(), to.clone()]
            }
            Op::AddPackage { .. } | Op::UpdatePackage { .. } | Op::SetLayer { .. } | Op::RemovePackage { .. } => vec![],
            Op::AddForce { .. } | Op::UpdateForce { .. } | Op::RemoveForce { .. } => vec![],
            Op::AddChangeScenario { .. } | Op::RemoveChangeScenario { .. } => vec![],
            Op::AddSystem { .. }
            | Op::RemoveSystem { .. }
            | Op::AddContainer { .. }
            | Op::UpdateContainer { .. }
            | Op::RemoveContainer { .. }
            | Op::AssignContainer { .. }
            | Op::AddLink { .. }
            | Op::RemoveLink { .. } => vec![],
            Op::SetLayout { view, positions } if view == "class" => positions.keys().cloned().collect(),
            Op::SetDomainModel { .. } | Op::SetUseCases { .. } | Op::SetSequence { .. } | Op::SetLayout { .. } => vec![],
        }
    }

    pub fn apply(&self, d: &mut Design, mode: Mode) -> Result<(), OpError> {
        let st = mode.status();
        let prop = mode.proposal();
        let origin = mode.origin();
        match self {
            Op::AddClass { name, stereotype, package, is_abstract, attributes, methods, responsibilities, rationale, aggregate_root, external, role } => {
                if !valid_ident(name) {
                    return err(format!("invalid class name {name:?}"));
                }
                if present(d, name) {
                    return err(format!("class {name} already exists; use update_class / add_attribute / add_method"));
                }
                if let Some(r) = role {
                    crate::responsibility::check_role(r).map_err(OpError)?;
                }
                for m in methods {
                    if let Some(c) = &m.concern {
                        crate::responsibility::check_concern(c).map_err(OpError)?;
                    }
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
                    source: None,
                    stereotype: stereotype.clone().filter(|s| !s.is_empty()),
                    package: package.clone(),
                    is_abstract: *is_abstract || stereotype.as_deref() == Some("abstract"),
                    attributes: attributes.iter().map(mark_attr).collect(),
                    methods: methods.iter().map(mark_m).collect(),
                    responsibilities: responsibilities.clone(),
                    rationale: rationale.clone(),
                    aggregate_root: *aggregate_root,
                    external: *external,
                    role: role.clone().filter(|r| !r.is_empty()),
                    status: st,
                    proposal: prop,
                    feedback: None,
                    origin,
                });
            }
            Op::UpdateClass { name, stereotype, package, is_abstract, responsibilities, rationale, aggregate_root, external, role } => {
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
                if let Some(x) = external {
                    c.external = *x;
                }
                if let Some(r) = role {
                    crate::responsibility::check_role(r).map_err(OpError)?;
                }
                if let Some(r) = role {
                    c.role = if r.is_empty() { None } else { Some(r.clone()) };
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
                if let Some(c) = &method.concern {
                    crate::responsibility::check_concern(c).map_err(OpError)?;
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
                if let Some(c) = &method.concern {
                    crate::responsibility::check_concern(c).map_err(OpError)?;
                }
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
            Op::AddPackage { name, layer, description, external, system } => {
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
                    external: *external,
                    system: system.clone().filter(|s| !s.is_empty()),
                    status: st,
                    proposal: prop,
                    origin,
                });
            }
            Op::UpdatePackage { name, description, external, system } => {
                let p = d
                    .packages
                    .iter_mut()
                    .find(|p| p.name == *name && p.status != Status::Pruned)
                    .ok_or_else(|| OpError(format!("unknown package {name}")))?;
                if let Some(x) = description {
                    p.description = Some(x.clone());
                }
                if let Some(x) = external {
                    p.external = *x;
                }
                if let Some(x) = system {
                    p.system = if x.is_empty() { None } else { Some(x.clone()) };
                }
                if mode.is_ghost() {
                    p.proposal = prop;
                }
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
            Op::AddSystem { name, description } => {
                if !valid_ident(name) {
                    return err("system needs a name");
                }
                if node_exists(d, name) {
                    return err(format!("{name} already exists (systems, containers and actors share names)"));
                }
                d.systems.retain(|x| x.name != *name);
                d.systems.push(ExternalSystem { name: name.clone(), description: description.clone(), status: st, proposal: prop });
            }
            Op::RemoveSystem { name } => {
                let idx = d
                    .systems
                    .iter()
                    .position(|x| x.name == *name && x.status != Status::Pruned)
                    .ok_or_else(|| OpError(format!("unknown system {name}")))?;
                if mode.is_ghost() {
                    d.systems[idx].status = Status::Removing;
                    d.systems[idx].proposal = prop;
                } else {
                    d.systems.remove(idx);
                    d.links.retain(|l| l.from != *name && l.to != *name);
                }
            }
            Op::AddContainer { name, technology, description, replicas, datastore } => {
                if !valid_ident(name) {
                    return err("container needs a name");
                }
                if node_exists(d, name) {
                    return err(format!("{name} already exists (systems, containers and actors share names)"));
                }
                d.containers.retain(|x| x.name != *name);
                d.containers.push(Container {
                    name: name.clone(),
                    technology: technology.clone(),
                    description: description.clone(),
                    replicas: replicas.unwrap_or(1).max(1),
                    datastore: *datastore,
                    status: st,
                    proposal: prop,
                });
            }
            Op::UpdateContainer { name, technology, description, replicas, datastore } => {
                let k = d
                    .containers
                    .iter_mut()
                    .find(|x| x.name == *name && x.status != Status::Pruned)
                    .ok_or_else(|| OpError(format!("unknown container {name}")))?;
                if let Some(t) = technology {
                    k.technology = Some(t.clone()).filter(|s| !s.is_empty());
                }
                if let Some(t) = description {
                    k.description = Some(t.clone()).filter(|s| !s.is_empty());
                }
                if let Some(r) = replicas {
                    k.replicas = (*r).max(1);
                }
                if let Some(x) = datastore {
                    k.datastore = *x;
                }
                if mode.is_ghost() {
                    k.proposal = prop;
                }
            }
            Op::RemoveContainer { name } => {
                let idx = d
                    .containers
                    .iter()
                    .position(|x| x.name == *name && x.status != Status::Pruned)
                    .ok_or_else(|| OpError(format!("unknown container {name}")))?;
                if mode.is_ghost() {
                    d.containers[idx].status = Status::Removing;
                    d.containers[idx].proposal = prop;
                } else {
                    d.containers.remove(idx);
                    d.links.retain(|l| l.from != *name && l.to != *name);
                    for p in d.packages.iter_mut().filter(|p| p.container.as_deref() == Some(name.as_str())) {
                        p.container = None;
                    }
                }
            }
            Op::AssignContainer { package, container } => {
                if let Some(c) = container {
                    if !d.containers.iter().any(|x| x.name == *c && x.status != Status::Pruned) {
                        return err(format!("unknown container {c} (add_container first, in the same batch is fine)"));
                    }
                }
                let p = d
                    .packages
                    .iter_mut()
                    .find(|p| p.name == *package && p.status != Status::Pruned)
                    .ok_or_else(|| OpError(format!("unknown package {package}")))?;
                p.container = container.clone().filter(|c| !c.is_empty());
                if mode.is_ghost() {
                    p.proposal = prop;
                }
            }
            Op::AddLink { from, to, interaction, protocol, description } => {
                for n in [from, to] {
                    if !node_exists(d, n) {
                        return err(format!("unknown endpoint {n} (a container, external system or actor)"));
                    }
                }
                if from == to {
                    return err("a link needs two different ends");
                }
                if d.links.iter().any(|l| l.from == *from && l.to == *to && l.status != Status::Pruned) {
                    return err(format!("link {from} → {to} already exists"));
                }
                d.links.retain(|l| !(l.from == *from && l.to == *to));
                d.links.push(Link {
                    from: from.clone(),
                    to: to.clone(),
                    interaction: interaction.unwrap_or_default(),
                    protocol: protocol.clone().filter(|s| !s.is_empty()),
                    description: description.clone(),
                    status: st,
                    proposal: prop,
                });
            }
            Op::LinkCode { class, method, symbol, language } => {
                let sym = symbol.trim();
                // language-neutral: Clojure `ns/fn-name!`, C++ `ns::T`, generics, operators
                let ok = !sym.is_empty() && sym.len() <= 400 && !sym.chars().any(|c| c.is_control() || matches!(c, '"' | '\'' | '`' | '\\'));
                if !ok {
                    return err(format!("not a code symbol: {symbol}"));
                }
                if method.is_some() != sym.contains('#') {
                    return err("a method link needs Owner#name(params); a class/module link a qualified name");
                }
                if let Some(l) = language.as_deref().filter(|l| !crate::symbols::is_supported_language(l)) {
                    return err(format!("unsupported language {l}"));
                }
                let c = live_class_mut(d, class)?;
                let link = Some(SourceSymbol { symbol: sym.to_string(), language: language.clone().filter(|l| l != "java"), ..Default::default() });
                match method {
                    Some(m) => c.methods.iter_mut().find(|x| x.name == *m && x.status != Status::Pruned).ok_or_else(|| OpError(format!("unknown method {class}.{m}")))?.source = link,
                    None => c.source = link,
                }
                mark_changed(c, mode);
            }
            Op::UnlinkCode { class, method } => {
                let c = live_class_mut(d, class)?;
                match method {
                    Some(m) => c.methods.iter_mut().find(|x| x.name == *m && x.status != Status::Pruned).ok_or_else(|| OpError(format!("unknown method {class}.{m}")))?.source = None,
                    None => c.source = None,
                }
                mark_changed(c, mode);
            }
            Op::RemoveLink { from, to } => {
                let idx = d
                    .links
                    .iter()
                    .position(|l| l.from == *from && l.to == *to && l.status != Status::Pruned)
                    .ok_or_else(|| OpError(format!("no link {from} → {to}")))?;
                if mode.is_ghost() {
                    d.links[idx].status = Status::Removing;
                    d.links[idx].proposal = prop;
                } else {
                    d.links.remove(idx);
                }
            }
            Op::SetDomainModel { model } => set_domain_model(d, model, mode)?,
            Op::SetUseCases { model } => set_use_cases(d, model, mode)?,
            Op::SetSequence { sequence } => set_sequence(d, sequence, mode)?,
            Op::SetLayout { view, positions } => {
                if !matches!(view.as_str(), "class" | "domain") {
                    return err(format!("no saved layout for the {view} view"));
                }
                let lay = d.layout.entry(view.clone()).or_default();
                for (k, p) in positions {
                    lay.insert(k.clone(), Pos { pinned: true, ..*p });
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

// ------------------------------------------------------------------ derived views as operations

/// Ghost marking: what is new or changed shows as proposed, what is gone as removing.
fn ghost_merge<T: Clone + PartialEq>(old: &[T], new: &mut Vec<T>, key: impl Fn(&T) -> String, status: impl Fn(&mut T) -> &mut Status, ghost: bool) {
    // pruned entries the new version leaves out survive (with their feedback)
    for o in old {
        let mut o2 = o.clone();
        let st = *status(&mut o2);
        if new.iter().any(|n| key(n) == key(o)) {
            continue;
        }
        if st == Status::Pruned {
            new.push(o2);
        } else if ghost {
            *status(&mut o2) = Status::Removing;
            new.push(o2);
        }
    }
    if ghost {
        for n in new.iter_mut() {
            let same = old.iter().any(|o| o == n);
            let st = status(n);
            if !same && *st != Status::Removing && *st != Status::Pruned {
                *st = Status::Proposed;
            }
        }
    }
}

fn set_domain_model(d: &mut Design, model: &DomainModel, mode: Mode) -> Result<(), OpError> {
    let mut m = model.clone();
    check_unique(m.concepts.iter().map(|c| c.name.as_str()), "concept")?;
    if m.concepts.iter().any(|c| c.name.trim().is_empty()) {
        return err("concept without a name");
    }
    for a in &m.associations {
        if !m.concepts.iter().any(|c| c.name == a.from) || !m.concepts.iter().any(|c| c.name == a.to) {
            return err(format!("association {} → {}: unknown concept", a.from, a.to));
        }
    }
    let ghost = mode.is_ghost();
    ghost_merge(&d.domain.concepts, &mut m.concepts, |c| c.name.clone(), |c| &mut c.status, ghost);
    ghost_merge(&d.domain.associations, &mut m.associations, |a| a.id.clone(), |a| &mut a.status, ghost);
    m.generated_rev = d.domain.generated_rev.max(m.generated_rev);
    d.domain = m;
    Ok(())
}

fn set_use_cases(d: &mut Design, model: &UseCaseModel, mode: Mode) -> Result<(), OpError> {
    let mut m = model.clone();
    check_unique(m.use_cases.iter().map(|u| u.name.as_str()), "use case")?;
    check_unique(m.actors.iter().map(|a| a.name.as_str()), "actor")?;
    let ghost = mode.is_ghost();
    ghost_merge(&d.use_cases.actors, &mut m.actors, |a| a.name.clone(), |a| &mut a.status, ghost);
    ghost_merge(&d.use_cases.use_cases, &mut m.use_cases, |u| u.name.clone(), |u| &mut u.status, ghost);
    m.generated_rev = d.use_cases.generated_rev.max(m.generated_rev);
    d.use_cases = m;
    Ok(())
}

fn set_sequence(d: &mut Design, s: &Sequence, mode: Mode) -> Result<(), OpError> {
    if s.name.trim().is_empty() {
        return err("sequence needs a name");
    }
    let mut s = s.clone();
    let old = d.sequences.iter().find(|x| x.name == s.name).cloned();
    if mode.is_ghost() {
        let old_msgs = old.as_ref().map(|o| o.messages.clone()).unwrap_or_default();
        for m in s.messages.iter_mut() {
            if m.status != Status::Pruned && !old_msgs.contains(m) {
                m.status = Status::Proposed;
            }
        }
        if old.is_none() {
            s.status = Status::Proposed;
        }
    }
    crate::store::validate_sequence(d, &mut s);
    match d.sequences.iter_mut().find(|x| x.name == s.name) {
        Some(x) => *x = s,
        None => d.sequences.push(s),
    }
    Ok(())
}

/// Containers, external systems and actors share one name space (they are the C4 nodes).
fn node_exists(d: &Design, name: &str) -> bool {
    d.containers.iter().any(|x| x.name == name && x.status != Status::Pruned)
        || d.systems.iter().any(|x| x.name == name && x.status != Status::Pruned)
        || d.use_cases.actors.iter().any(|a| a.name == name && a.status != Status::Pruned)
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
        ..Default::default()
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
        // named lifelines keep their name; only the class changes
        for l in s.lifelines.iter_mut() {
            if l.class == from {
                l.class = to.to_string();
            }
            if l.name == from {
                l.name = to.to_string();
            }
        }
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
