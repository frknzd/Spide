//! Granularity levels shared by the canvas (mirrored in `src/canvas/visuals.ts`) and the exporters.
//!
//! 0 packages · 1 names · 2 responsibilities · 3 design (exam style, default) · 4 full.
//! The design level follows the course's exam notes: inherited attributes are not repeated,
//! attributes that restate a drawn association become the association's role name, and plain
//! accessors, constructors and private helpers are folded.

use crate::model::*;
use std::collections::{BTreeMap, BTreeSet};

/// Simple type names in a type expression (`Map<String, List<Loan>>` → Map, String, List, Loan).
pub fn type_names(ty: &str) -> impl Iterator<Item = &str> {
    ty.split(|c: char| !(c.is_alphanumeric() || c == '_')).filter(|t| !t.is_empty())
}

/// A plain getter/setter/predicate, or a constructor.
pub fn is_accessor(class: &str, m: &Method) -> bool {
    if m.name == class {
        return true;
    }
    let n = &m.name;
    let prefixed = ["get", "is", "has", "set"].iter().find(|p| {
        n.len() > p.len() && n.starts_with(*p) && n[p.len()..].chars().next().is_some_and(|c| c.is_uppercase() || c == '_')
    });
    let Some(p) = prefixed else { return false };
    if *p == "set" {
        return m.params.len() == 1 && m.calls.is_empty();
    }
    m.params.is_empty() && m.calls.iter().filter(|c| !c.starts_with("this.")).count() <= 1 && m.uses.len() <= 1
}

pub fn is_constructor(class: &str, m: &Method) -> bool {
    m.name == class
}

/// Attributes per class that only restate a drawn association, and the role name per relation id.
pub fn association_roles(d: &Design) -> (BTreeMap<String, BTreeSet<String>>, BTreeMap<String, String>) {
    let mut attrs: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut roles: BTreeMap<String, String> = BTreeMap::new();
    for r in d.relations.iter().filter(|r| r.status != Status::Pruned) {
        if !matches!(r.kind, RelationKind::Association | RelationKind::Aggregation | RelationKind::Composition) {
            continue;
        }
        let mut ends = vec![(&r.from, &r.to)];
        if r.kind == RelationKind::Association && !r.directed {
            ends.push((&r.to, &r.from));
        }
        for (owner, other) in ends {
            let Some(a) = d.class(owner).and_then(|c| c.attributes.iter().find(|a| type_names(&a.ty).any(|t| t == other))) else { continue };
            attrs.entry(owner.clone()).or_default().insert(a.name.clone());
            roles.entry(r.id.clone()).or_insert_with(|| a.name.clone());
        }
    }
    (attrs, roles)
}

/// Ancestors of a class through inheritance relations (transitive).
pub fn ancestors(d: &Design, class: &str) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    let mut stack = vec![class.to_string()];
    while let Some(c) = stack.pop() {
        for r in d.relations.iter().filter(|r| r.status != Status::Pruned && r.from == c && matches!(r.kind, RelationKind::Inheritance | RelationKind::Realization)) {
            if !out.contains(&r.to) && r.to != class {
                out.push(r.to.clone());
                stack.push(r.to.clone());
            }
        }
    }
    out
}

/// Attribute names a class inherits from its ancestors.
pub fn inherited_attributes(d: &Design, class: &str) -> BTreeSet<String> {
    ancestors(d, class).iter().filter_map(|a| d.class(a)).flat_map(|c| c.attributes.iter().map(|a| a.name.clone())).collect()
}
