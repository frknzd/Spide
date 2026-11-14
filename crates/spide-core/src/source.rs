//! Explicit source bindings and three-way structural reconciliation.
use crate::{Design, Class, Method, Op};
use std::collections::{BTreeMap, BTreeSet};
use serde::{Serialize, Deserialize};

pub fn signature(m: &Method) -> String {
    let name = m.name.split('(').next().unwrap_or(&m.name);
    format!("{name}({})", m.params.iter().map(|p| p.ty.as_str()).collect::<Vec<_>>().join(","))
}

/// Only explicitly mapped classes participate as intended elements. Unbound classes retain
/// separate code-only identities even when their simple names happen to match the design.
pub fn mapped_reality(intended: &Design, code: &Design, bindings: &BTreeMap<String, String>) -> Design {
    let mut reality = code.clone();
    let mut names = BTreeMap::new();
    for c in &code.classes {
        let symbol = c.source.as_ref().map(|s| s.symbol.as_str()).unwrap_or(&c.name);
        let owners: Vec<_> = intended.live_classes().filter(|d| bindings.get(&d.name).map(String::as_str).or_else(|| d.source.as_ref().map(|s| s.symbol.as_str())) == Some(symbol)).collect();
        names.insert(c.name.clone(), if owners.len() == 1 { owners[0].name.clone() } else { format!("code::{symbol}") });
    }
    for c in &mut reality.classes {
        c.name = names[&c.name].clone();
        if let Some(design) = intended.live_class(&c.name) {
            for m in &mut c.methods {
                let owners: Vec<_> = design.methods.iter().filter(|d| {
                    let key = format!("{}.{}", c.name, d.name);
                    match bindings.get(&key).map(String::as_str).or_else(|| d.source.as_ref().map(|s| s.symbol.as_str())) {
                        Some(symbol) => m.source.as_ref().is_some_and(|s| s.symbol == symbol),
                        None => signature(d) == signature(m),
                    }
                }).collect();
                if owners.len() == 1 { m.name = owners[0].name.clone(); }
            }
        }
        for m in &mut c.methods {
            m.calls = m.calls.iter().map(|call| {
                names.iter().find_map(|(from, to)| call.strip_prefix(&format!("{from}.")).map(|rest| format!("{to}.{rest}"))).unwrap_or_else(|| call.clone())
            }).collect();
        }
    }
    for r in &mut reality.relations { r.from = names[&r.from].clone(); r.to = names[&r.to].clone(); r.id = format!("{}->{}:{}", r.from, r.to, r.kind.as_str()); }
    reality
}

fn structural(c: Option<&Class>) -> serde_json::Value {
    match c {
        None => serde_json::Value::Null,
        Some(c) => serde_json::json!({"package": c.package, "abstract": c.is_abstract,
            "attributes": c.attributes.iter().filter(|a| a.status.is_live()).map(|a| serde_json::json!([a.name,a.ty,a.visibility,a.is_static,a.multiplicity])).collect::<Vec<_>>(),
            "methods": c.methods.iter().filter(|m| m.status.is_live()).map(|m| serde_json::json!([signature(m),m.returns,m.visibility,m.is_static,m.is_abstract])).collect::<Vec<_>>() }),
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reconciliation {
    pub class: String,
    pub direction: String,
    pub conflict: bool,
    pub ops: Vec<Op>,
}

/// Suggestions only. Deletes/updates require selecting and approving a concrete decision.
pub fn reconcile(design: &Design, source: &Design, baseline_design: Option<&Design>, baseline_source: Option<&Design>, bindings: &BTreeMap<String, String>) -> Vec<Reconciliation> {
    let mut out = vec![];
    let names: BTreeSet<_> = bindings.keys().filter(|k| design.class(k).is_some() || baseline_design.is_some_and(|d| d.class(k).is_some())).cloned().collect();
    for name in names {
        let current = design.live_class(&name); let actual = source.live_class(&name);
        let target = match (current, actual) {
            (Some(d), Some(s)) => {
                let mut c = d.clone();
                c.package = s.package.clone(); c.is_abstract = s.is_abstract;
                c.attributes = s.attributes.iter().map(|a| {
                    let old = d.attributes.iter().find(|x| x.name == a.name);
                    crate::Attribute { origin: old.and_then(|x| x.origin.clone()), proposal: None, ..a.clone() }
                }).collect();
                c.methods = s.methods.iter().map(|m| {
                    let old = d.methods.iter().find(|x| x.name == m.name);
                    Method { code: None, description: old.and_then(|x| x.description.clone()), concern: old.and_then(|x| x.concern.clone()), origin: old.and_then(|x| x.origin.clone()), proposal: None, ..m.clone() }
                }).collect();
                Some(c)
            }
            (_, Some(s)) => Some(s.clone()),
            (_, None) => None,
        };
        let relations = |d: &Design| -> BTreeSet<_> { d.live_relations().filter(|r| r.from == name && matches!(r.kind, crate::RelationKind::Inheritance | crate::RelationKind::Realization)).map(|r| (r.to.clone(), r.kind)).collect() };
        if structural(current) == structural(target.as_ref()) && relations(design) == relations(source) { continue; }
        let acknowledged = baseline_design.is_some() && baseline_source.is_some();
        let dc = baseline_design.is_some_and(|d| structural(d.live_class(&name)) != structural(current) || relations(d) != relations(design));
        let sc = baseline_source.is_some_and(|d| structural(d.live_class(&name)) != structural(actual) || relations(d) != relations(source));
        let conflict = !acknowledged || (dc && sc);
        let mut next = design.clone();
        match target { Some(c) => { next.classes.retain(|c| c.name != name); next.classes.push(c); }, None => { next.classes.retain(|c| c.name != name); next.relations.retain(|r| r.from != name && r.to != name); } }
        next.relations.retain(|r| !(r.from == name && matches!(r.kind, crate::RelationKind::Inheritance | crate::RelationKind::Realization)));
        for r in source.live_relations().filter(|r| r.from == name && next.live_class(&r.to).is_some() && matches!(r.kind, crate::RelationKind::Inheritance | crate::RelationKind::Realization)).cloned().collect::<Vec<_>>() { next.relations.push(r); }
        let ops = crate::diff::diff(design, &next);
        out.push(Reconciliation { class: name, direction: if conflict { "conflict / establish baseline" } else if sc { "source → design" } else { "design → source" }.into(), conflict, ops });
    }
    out
}

/// Measured code facts for mapped methods, without replacing intended responsibilities/signatures.
pub fn with_length_evidence(design: &Design, mapped: &Design) -> Design {
    with_bound_length_evidence(design, mapped, &BTreeMap::new())
}

pub fn with_bound_length_evidence(design: &Design, mapped: &Design, bindings: &BTreeMap<String, String>) -> Design {
    let mut d = design.clone();
    for c in &mut d.classes {
        let Some(source) = mapped.live_class(&c.name) else { continue };
        for m in &mut c.methods {
            let bound = bindings.get(&format!("{}.{}", c.name, m.name)).map(String::as_str).or_else(|| m.source.as_ref().map(|s| s.symbol.as_str()));
            let matches: Vec<_> = source.methods.iter().filter(|s| s.name == m.name && (signature(s) == signature(m) || bound.is_some_and(|symbol| s.source.as_ref().is_some_and(|source| source.symbol == symbol)))).collect();
            if matches.len() == 1 { m.code = matches[0].code.clone(); m.source = matches[0].source.clone(); }
        }
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SourceSymbol, Attribute};
    fn model(ty: &str) -> Design { Design { classes: vec![Class { name: "A".into(), attributes: vec![Attribute { name: "value".into(), ty: ty.into(), ..Default::default() }], ..Default::default() }], ..Default::default() } }
    #[test]
    fn selected_source_reconciliation_preserves_intent_and_can_remove_members() {
        let mut design = model("String");
        design.classes[0].responsibilities = vec!["Own the value".into()];
        design.classes[0].rationale = Some("Domain intent".into());
        let mut actual = model("int");
        let bindings = BTreeMap::from([("A".into(), "a.A".into())]);
        let plans = reconcile(&design, &actual, Some(&design), Some(&design), &bindings);
        let mut store = crate::Store::new(design.clone());
        let p = store.propose_ops(plans[0].ops.clone(), None, "code").unwrap();
        assert_eq!(store.design.classes[0].attributes[0].ty, "String");
        store.accept_proposal(&p).unwrap();
        assert_eq!(store.design.classes[0].attributes[0].ty, "int");
        assert_eq!(store.design.classes[0].responsibilities, design.classes[0].responsibilities);
        assert_eq!(store.design.classes[0].rationale, design.classes[0].rationale);
        actual.classes[0].attributes.clear();
        let plans = reconcile(&store.design, &actual, Some(&store.design), Some(&store.design), &bindings);
        assert!(plans[0].ops.iter().any(|o| matches!(o, Op::RemoveAttribute { .. })));
    }
    #[test]
    fn explicit_bindings_and_three_way_conflicts() {
        let design = model("String"); let mut code = model("int");
        code.classes[0].source = Some(SourceSymbol { symbol: "a.A".into(), ..Default::default() });
        assert!(mapped_reality(&design, &code, &BTreeMap::new()).live_class("A").is_none());
        let bindings = BTreeMap::from([("A".into(), "a.A".into())]);
        let mapped = mapped_reality(&design, &code, &bindings);
        let r = reconcile(&design, &mapped, Some(&design), Some(&design), &bindings);
        assert!(!r[0].conflict); assert_eq!(r[0].direction, "source → design"); assert!(!r[0].ops.is_empty());
        assert!(reconcile(&model("long"), &mapped, Some(&design), Some(&design), &bindings)[0].conflict);
        assert!(reconcile(&design, &mapped, None, None, &bindings)[0].conflict);
        assert!(reconcile(&design, &design, Some(&design), Some(&design), &bindings).is_empty());
    }
}
