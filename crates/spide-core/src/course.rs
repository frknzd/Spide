//! Course checks (DOSS, Roel Wuyts): Law of Demeter, Creator, Polymorphism instead of switches,
//! long methods — from the implementation when code facts are known (imported code), and from the
//! design otherwise.

use crate::metrics::Issue;
use crate::model::*;
use std::collections::{BTreeMap, BTreeSet};

fn issue(kind: &'static str, sev: f64, conf: f64, basis: &str, msg: String, targets: Vec<String>, explain: &str, fix: &str) -> Issue {
    let mut i = Issue::new(kind, sev, msg, targets);
    i.explain = explain.into();
    i.fix_hint = Some(fix.into());
    i.confidence = conf;
    i.basis = basis.into();
    i
}

/// Checks that need method bodies (imported code).
pub fn code_checks(d: &Design, ghosts: bool) -> Vec<Issue> {
    let live = |s: Status| s.is_live() || (ghosts && s == Status::Proposed);
    let classes: Vec<&Class> = d.classes.iter().filter(|c| live(c.status) && !d.is_external(&c.name)).collect();
    let mut out = vec![];
    let mut switches: BTreeMap<String, BTreeSet<String>> = BTreeMap::new(); // case constant → Class.method
    for c in &classes {
        for m in c.methods.iter().filter(|m| live(m.status)) {
            let Some(f) = &m.code else { continue };
            let key = format!("{}.{}", c.name, m.name);
            if f.max_chain >= 3 {
                out.push(issue(
                    "demeter",
                    0.3,
                    0.8,
                    "code",
                    format!("{key}() talks to strangers: {}", f.chains.first().cloned().unwrap_or_else(|| format!("a chain of {} calls", f.max_chain))),
                    vec![c.name.clone(), key.clone()],
                    "Only talk to your friends (yourself, your arguments, objects you create, your fields). A chain through getters ties this class to the inner structure of others: changing either breaks it.",
                    "ask the first object to do the work (Tell, don't ask), or add a method on it that hides the chain",
                ));
            }
            if f.loc > 30 {
                out.push(issue(
                    "long_method",
                    0.25,
                    0.9,
                    "code",
                    format!("{key}() is {} lines long", f.loc),
                    vec![c.name.clone(), key.clone()],
                    "The longer a method is, the harder it is to understand; long methods usually do several things.",
                    "Extract Method for each part (and move parts to the classes whose data they use)",
                ));
            }
            for k in &f.switch_on {
                switches.entry(k.clone()).or_default().insert(key.clone());
            }
            // Creator: B should create A if B contains/aggregates/records A, or has the data to initialise it
            // (a static main is the composition root: it creates everything by design)
            let root = m.is_static && m.name == "main";
            for x in f.creates.iter().filter(|_| !root) {
                let Some(xc) = d.class(x).filter(|xc| live(xc.status) && !d.is_external(&xc.name)) else { continue };
                let holds = |owner: &str| {
                    d.relations.iter().any(|r| {
                        live(r.status) && r.from == owner && r.to == xc.name && matches!(r.kind, RelationKind::Composition | RelationKind::Aggregation | RelationKind::Association)
                    }) || d.class(owner).is_some_and(|o| o.attributes.iter().any(|a| crate::detail::type_names(&a.ty).any(|t| t == xc.name)))
                };
                if holds(&c.name) || xc.name == c.name || m.returns.as_deref().is_some_and(|r| crate::detail::type_names(r).any(|t| t == xc.name)) {
                    continue;
                }
                // who contains/records X: a composition/aggregation, else a field of X's type (collections first)
                let by_relation = d
                    .relations
                    .iter()
                    .find(|r| live(r.status) && r.to == xc.name && r.from != c.name && matches!(r.kind, RelationKind::Composition | RelationKind::Aggregation))
                    .map(|r| r.from.clone());
                let by_field = || {
                    let holders: Vec<(&Class, bool)> = classes
                        .iter()
                        .filter(|o| o.name != c.name && o.name != xc.name)
                        .filter_map(|o| {
                            o.attributes.iter().find(|a| crate::detail::type_names(&a.ty).any(|t| t == xc.name)).map(|a| (*o, a.ty.contains('<') || a.ty.contains('[')))
                        })
                        .collect();
                    holders.iter().find(|(_, many)| *many).or(holders.first()).map(|(o, _)| o.name.clone())
                };
                if let Some(w) = by_relation.or_else(by_field) {
                    out.push(issue(
                        "creator",
                        0.3,
                        0.75,
                        "code",
                        format!("{key}() creates {} but {w} contains/records it", xc.name),
                        vec![c.name.clone(), key.clone(), w.clone()],
                        "Creator: B should create A when B contains, aggregates or records A, or has the data to initialise it. Creating parts elsewhere couples this class to them for no reason.",
                        "let the whole create its parts (e.g. a make…/add… method on it)",
                    ));
                }
            }
        }
    }
    // the same type code switched on in several places (the GATK example: 11 places)
    let mut by_methods: BTreeMap<Vec<String>, BTreeSet<String>> = BTreeMap::new();
    for (k, ms) in &switches {
        by_methods.entry(ms.iter().cloned().collect()).or_default().insert(k.clone());
    }
    for (ms, consts) in by_methods {
        let classes: BTreeSet<&str> = ms.iter().filter_map(|m| m.split('.').next()).collect();
        if consts.len() >= 2 && (ms.len() >= 3 || classes.len() >= 2) {
            let mut t: Vec<String> = classes.iter().map(|c| c.to_string()).collect();
            t.extend(ms.iter().cloned());
            out.push(issue(
                "type_switch",
                0.4,
                0.8,
                "code",
                format!("the same cases ({}) are switched on in {} places: {}", consts.iter().take(5).cloned().collect::<Vec<_>>().join(", "), ms.len(), ms.join(", ")),
                t,
                "Switches on a type code, repeated in several places, are a procedural way to choose behaviour: every new case means finding and changing all of them.",
                "Replace Conditional with Polymorphism: give each case its own class (or enum constant) with the behaviour",
            ));
        }
    }
    out
}

/// All course checks for a design (code-based ones only where code facts exist).
pub fn check(d: &Design, ghosts: bool) -> Vec<Issue> {
    code_checks(d, ghosts)
}
