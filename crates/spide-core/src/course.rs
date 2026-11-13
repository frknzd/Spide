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

fn getter_like(m: &str) -> bool {
    let w = crate::responsibility::words(m);
    matches!(w.first().map(String::as_str), Some("get" | "is" | "has" | "size" | "count" | "to" | "find"))
}

fn returns_of(d: &Design, call: &str) -> Option<String> {
    let (cls, meth) = call.split_once('.')?;
    let r = d.class(cls)?.methods.iter().find(|m| m.name == meth)?.returns.clone()?;
    let t = crate::detail::type_names(&r).find(|t| d.class(t).is_some() && *t != cls).map(String::from);
    t
}

/// Checks on the design itself (signatures, uses, calls, relations).
pub fn design_checks(d: &Design, ghosts: bool, resp: &crate::responsibility::Report) -> Vec<Issue> {
    let live = |s: Status| s.is_live() || (ghosts && s == Status::Proposed);
    let classes: Vec<&Class> = d.classes.iter().filter(|c| live(c.status) && !d.is_external(&c.name)).collect();
    let role = |c: &str| resp.classes.get(c).and_then(|x| x.role.as_ref()).map(|r| r.role.clone()).unwrap_or_default();
    let misplaced: std::collections::BTreeSet<(String, String)> =
        resp.findings.iter().filter_map(|f| f.method.clone().map(|m| (f.class.clone(), m))).collect();
    let mut out = vec![];
    // who reads whose getters (for data classes)
    let mut read_by_others: BTreeMap<&str, usize> = BTreeMap::new();
    for c in &classes {
        for m in c.methods.iter().filter(|m| live(m.status)) {
            for call in &m.calls {
                if let Some((x, n)) = call.split_once('.') {
                    if x != c.name && getter_like(n) {
                        *read_by_others.entry(x).or_default() += 1;
                    }
                }
            }
        }
    }
    for c in &classes {
        let r = role(&c.name);
        let mut untyped = 0;
        for m in c.methods.iter().filter(|m| live(m.status)) {
            let key = format!("{}.{}", c.name, m.name);
            untyped += m.params.iter().filter(|p| p.ty.trim().is_empty()).count();
            if m.params.len() > 4 {
                out.push(issue(
                    "long_params",
                    0.2,
                    0.9,
                    "heuristic",
                    format!("{key}() takes {} parameters", m.params.len()),
                    vec![c.name.clone(), key.clone()],
                    "In OO you pass enough for the method to reach what it needs, not everything it needs: long parameter lists are hard to use and change.",
                    "pass an object that knows these values (or ask an object you already have) — Replace Parameter with Method / Introduce Parameter Object",
                ));
            }
            // Law of Demeter: calls on objects obtained from another call (strangers)
            let friends: std::collections::BTreeSet<String> = c
                .attributes
                .iter()
                .flat_map(|a| crate::detail::type_names(&a.ty).map(String::from).collect::<Vec<_>>())
                .chain(m.params.iter().flat_map(|p| crate::detail::type_names(&p.ty).map(String::from).collect::<Vec<_>>()))
                .chain(std::iter::once(c.name.clone()))
                .collect();
            let obtained: std::collections::BTreeSet<String> = m.calls.iter().filter_map(|x| returns_of(d, x)).collect();
            let strangers: Vec<&String> = m
                .calls
                .iter()
                .filter(|x| x.split_once('.').is_some_and(|(cls, _)| !friends.contains(cls) && obtained.contains(cls) && !d.is_external(cls) && !matches!(role(cls).as_str(), "value")))
                .collect();
            if m.code.is_none() && !strangers.is_empty() {
                out.push(issue(
                    "demeter",
                    0.3,
                    0.7,
                    "heuristic",
                    format!("{key}() talks to strangers: {} (objects it got from other calls)", strangers.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")),
                    vec![c.name.clone(), key.clone()],
                    "Only talk to your friends (yourself, your arguments, objects you create, your fields). Reaching through one object into another ties this class to both.",
                    "ask the object you have to do the work (Tell, don't ask), or give it a method that hides the chain",
                ));
            }
            // Feature Envy: more interested in another class's data than in its own
            if !matches!(r.as_str(), "presentation" | "output" | "adapter") && !misplaced.contains(&(c.name.clone(), m.name.clone())) && !crate::detail::is_accessor(&c.name, m) {
                let mut reads: BTreeMap<&str, usize> = BTreeMap::new();
                for call in &m.calls {
                    if let Some((x, n)) = call.split_once('.') {
                        if x != c.name && getter_like(n) && !d.is_external(x) {
                            *reads.entry(x).or_default() += 1;
                        }
                    }
                }
                let total: usize = reads.values().sum();
                let own = m.uses.len() + m.calls.iter().filter(|x| !x.contains('.')).count();
                if let Some((x, n)) = reads.iter().max_by_key(|(_, n)| **n) {
                    if total >= 3 && total > own && *n >= 2 {
                        out.push(issue(
                            "feature_envy",
                            0.35,
                            0.65,
                            "heuristic",
                            format!("{key}() reads {total} values from other classes ({n} from {x}) and uses little of {}'s own data", c.name),
                            vec![c.name.clone(), key.clone(), x.to_string()],
                            "A method more interested in another class than in its own: the work lives far from the data (Information Expert).",
                            &format!("move it (or the part that uses {x}'s data) to {x} — Move Method"),
                        ));
                    }
                }
            }
            // UI calling the domain's commands directly (the incorrect POS example)
            if r == "presentation" {
                for call in &m.calls {
                    let Some((x, n)) = call.split_once('.') else { continue };
                    let xr = role(x);
                    // a *command* (changes domain state, returns nothing); reading values to show them is fine
                    let command = d.class(x).is_some_and(|xc| xc.methods.iter().any(|mm| mm.name == n && mm.returns.is_none() && !crate::detail::is_accessor(x, mm) && !crate::detail::is_constructor(x, mm)));
                    if matches!(xr.as_str(), "domain" | "value") && !getter_like(n) && command {
                        out.push(issue(
                            "ui_bypasses_controller",
                            0.45,
                            0.7,
                            "heuristic",
                            format!("{key}() calls {x}.{n}() directly: the UI drives the domain without a controller"),
                            vec![c.name.clone(), key.clone(), x.to_string()],
                            "System events from the UI should go to a controller (facade or use-case controller) that delegates to the domain; otherwise the UI is coupled to domain internals and the domain can't be reused with another UI.",
                            "send the system event to a controller (e.g. Register.enterItem) that calls the domain",
                        ));
                        break;
                    }
                }
            }
        }
        if untyped > 0 {
            out.push(issue(
                "missing_types",
                0.15,
                1.0,
                "structure",
                format!("{}: {untyped} parameter(s) without a type", c.name),
                vec![c.name.clone()],
                "Always give argument types: most of the time they are needed to understand the design.",
                "add the parameter types",
            ));
        }
        // inherited attributes repeated
        let inh = crate::detail::inherited_attributes(d, &c.name);
        let repeated: Vec<&str> = c.attributes.iter().filter(|a| live(a.status) && inh.contains(&a.name)).map(|a| a.name.as_str()).collect();
        if !repeated.is_empty() {
            out.push(issue(
                "inherited_repeated",
                0.15,
                1.0,
                "structure",
                format!("{} repeats inherited attribute(s) {}", c.name, repeated.join(", ")),
                vec![c.name.clone()],
                "Don't repeat inherited variables/operations: it clutters the diagram and suggests two separate fields.",
                "remove them from the subclass",
            ));
        }
        // a class without behaviour (data class / "no behaviour" exam fail)
        let kind_ok = c.is_interface() || c.is_enum() || matches!(c.stereotype.as_deref(), Some("value" | "record" | "dto" | "enum" | "interface" | "exception"))
            || matches!(r.as_str(), "value");
        let behaviour = c.methods.iter().filter(|m| live(m.status) && !crate::detail::is_accessor(&c.name, m)).count();
        let attrs = c.attributes.iter().filter(|a| live(a.status)).count();
        if !kind_ok && behaviour == 0 && attrs >= 2 {
            let reads = read_by_others.get(c.name.as_str()).copied().unwrap_or(0);
            out.push(issue(
                "data_class",
                if reads >= 2 { 0.4 } else { 0.25 },
                0.7,
                "heuristic",
                format!(
                    "{} only holds data ({} attribute(s), no behaviour){}",
                    c.name,
                    attrs,
                    if reads >= 2 { format!(" while other classes compute with it ({reads} getter calls)") } else { String::new() }
                ),
                vec![c.name.clone()],
                "Make objects smart, not just data with getters/setters: the behaviour that uses this data belongs here (RDD, Information Expert). Designs without behaviour in their classes don't show how they work.",
                "move the behaviour that uses these attributes into the class",
            ));
        }
        // Visitor with one element → a Strategy
        if c.name.ends_with("Visitor") && (c.is_interface() || c.is_abstract) {
            let visits = c.methods.iter().filter(|m| live(m.status) && m.name.starts_with("visit")).count();
            if visits == 1 {
                out.push(issue(
                    "degenerate_visitor",
                    0.2,
                    0.8,
                    "structure",
                    format!("{} visits only one kind of element", c.name),
                    vec![c.name.clone()],
                    "A Visitor decouples several operations from several element types. With one element type there's no double dispatch needed: it's a Strategy.",
                    "model it as a Strategy (no accept() on the element), or add the other element types",
                ));
            }
        }
    }
    // Composite: a composite (child of S that holds many S) needs add/remove/getChildren
    for r in d.relations.iter().filter(|r| live(r.status) && matches!(r.kind, RelationKind::Aggregation | RelationKind::Composition | RelationKind::Association)) {
        let (comp, comp_t) = (&r.from, &r.to);
        let is_child = crate::detail::ancestors(d, comp).contains(comp_t);
        if !is_child {
            continue;
        }
        let names: Vec<String> = [comp.as_str(), comp_t.as_str()]
            .iter()
            .filter_map(|n| d.class(n))
            .flat_map(|c| c.methods.iter().filter(|m| live(m.status)).map(|m| m.name.to_lowercase()))
            .collect();
        let has = |p: &[&str]| names.iter().any(|n| p.iter().any(|x| n.starts_with(x)));
        let missing: Vec<&str> = [("add", &["add"][..]), ("remove", &["remove", "delete"][..]), ("getChildren", &["get", "children", "elements", "iterator"][..])]
            .iter()
            .filter(|(_, p)| !has(p))
            .map(|(n, _)| *n)
            .collect();
        if !missing.is_empty() {
            out.push(issue(
                "composite_incomplete",
                0.3,
                0.8,
                "structure",
                format!("{comp} is a Composite of {comp_t} but has no {}", missing.join("/")),
                vec![comp.clone(), comp_t.clone()],
                "A Composite always needs add(..), remove(..) and getElements(..) — and you should say whether they sit on the Component (transparency) or on the Composite (safety).",
                "add the child-management operations on the Composite (or the Component, for transparency)",
            ));
        }
    }
    // the design-level hint of a type-code switch: one enum handed to many classes' methods
    for e in classes.iter().filter(|c| c.is_enum()) {
        let users: std::collections::BTreeSet<&str> = classes
            .iter()
            .filter(|c| c.name != e.name)
            .filter(|c| {
                c.methods.iter().any(|m| {
                    live(m.status) && !crate::detail::is_constructor(&c.name, m) && m.params.iter().any(|p| crate::detail::type_names(&p.ty).next() == Some(e.name.as_str()))
                })
            })
            .map(|c| c.name.as_str())
            .collect();
        let has_code = classes.iter().any(|c| c.methods.iter().any(|m| m.code.is_some()));
        if users.len() >= 3 && !has_code {
            let mut t: Vec<String> = users.iter().map(|u| u.to_string()).collect();
            t.push(e.name.clone());
            out.push(issue(
                "type_switch",
                0.2,
                0.45,
                "heuristic",
                format!("{} is passed to methods of {} classes ({}): each probably switches on it", e.name, users.len(), users.iter().cloned().collect::<Vec<_>>().join(", ")),
                t,
                "A type code that several classes decide on means a switch in each of them; a new value then needs changes everywhere.",
                "move the varying behaviour onto the type itself (Polymorphism: an enum with behaviour or a class hierarchy)",
            ));
        }
    }
    out
}

/// Factory-style creation in the design: `C.createX(): X` while another class contains X.
pub fn creator_checks(d: &Design, ghosts: bool) -> Vec<Issue> {
    let live = |s: Status| s.is_live() || (ghosts && s == Status::Proposed);
    let mut out = vec![];
    for c in d.classes.iter().filter(|c| live(c.status) && !d.is_external(&c.name)) {
        if c.stereotype.as_deref() == Some("factory") || c.name.ends_with("Factory") || c.name.ends_with("Builder") {
            continue;
        }
        for m in c.methods.iter().filter(|m| live(m.status) && m.code.is_none()) {
            let w = crate::responsibility::words(&m.name);
            if !matches!(w.first().map(String::as_str), Some("create" | "make" | "new" | "build")) {
                continue;
            }
            let Some(x) = m.returns.as_deref().and_then(|r| crate::detail::type_names(r).find(|t| d.class(t).is_some() && *t != c.name).map(String::from)) else { continue };
            let holds = d.relations.iter().any(|r| live(r.status) && r.from == c.name && r.to == x)
                || c.attributes.iter().any(|a| crate::detail::type_names(&a.ty).any(|t| t == x));
            if holds {
                continue;
            }
            if let Some(w) = d.relations.iter().find(|r| live(r.status) && r.to == x && r.from != c.name && matches!(r.kind, RelationKind::Composition | RelationKind::Aggregation)) {
                out.push(issue(
                    "creator",
                    0.3,
                    0.6,
                    "heuristic",
                    format!("{}.{}() creates {x} but {} contains them", c.name, m.name, w.from),
                    vec![c.name.clone(), format!("{}.{}", c.name, m.name), w.from.clone()],
                    "Creator: B should create A when B contains, aggregates or records A, or has the data to initialise it.",
                    "let the whole create its parts",
                ));
            }
        }
    }
    out
}

/// All course checks (code-based ones only where code facts exist).
pub fn check(d: &Design, ghosts: bool, resp: &crate::responsibility::Report) -> Vec<Issue> {
    let mut out = code_checks(d, ghosts);
    out.extend(design_checks(d, ghosts, resp));
    out.extend(creator_checks(d, ghosts));
    out
}
