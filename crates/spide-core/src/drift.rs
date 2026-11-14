//! Drift between the intended design and the code ("reality", imported read-only).

use crate::metrics::compute;
use crate::model::*;
use crate::ops::Op;
use serde::Serialize;
use std::collections::BTreeSet;

#[derive(Debug, Clone, Serialize, Default, PartialEq)]
pub struct MemberDrift {
    pub class: String,
    /// In the design, not in the code.
    pub missing_in_code: Vec<String>,
    /// In the code, not in the design.
    pub extra_in_code: Vec<String>,
    /// Same member identity but changed type/signature/modifiers.
    pub changed: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Default, PartialEq)]
pub struct RelationDrift {
    pub from: String,
    pub to: String,
    pub kind: RelationKind,
}

#[derive(Debug, Clone, Serialize, Default, PartialEq)]
pub struct PackageDrift {
    pub class: String,
    pub design: Option<String>,
    pub code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default, PartialEq)]
pub struct Drift {
    pub missing_in_code: Vec<String>,
    pub extra_in_code: Vec<String>,
    pub members: Vec<MemberDrift>,
    /// Designed relations the code doesn't have (no dependency at all, or wrong inheritance).
    pub relations_missing_in_code: Vec<RelationDrift>,
    /// Structural relations (inheritance, realization, fields) in the code the design doesn't show.
    pub relations_extra_in_code: Vec<RelationDrift>,
    pub packages: Vec<PackageDrift>,
    /// The design's rules checked against the code.
    pub rule_violations_in_code: Vec<String>,
    /// Share of designed classes (and their members) found in the code, 0..1.
    pub conformance: f64,
}

fn is_structural(k: RelationKind) -> bool {
    matches!(k, RelationKind::Inheritance | RelationKind::Realization)
}

/// Reality with the design's rules and package layers, so violations mean the same thing.
pub fn with_design_rules(intended: &Design, reality: &Design) -> Design {
    let mut r = reality.clone();
    r.rules = intended.rules.clone();
    for p in r.packages.iter_mut() {
        if let Some(dp) = intended.packages.iter().find(|x| x.name == p.name && x.status.is_live()) {
            p.layer = dp.layer;
            p.container = dp.container.clone();
            p.external = p.external || dp.external;
            p.system = dp.system.clone();
        }
    }
    // the intent (role, responsibilities, stereotype) comes from the design: the code is judged against it
    for c in r.classes.iter_mut() {
        if let Some(dc) = intended.live_class(&c.name) {
            c.role = dc.role.clone();
            c.responsibilities = dc.responsibilities.clone();
            c.external = c.external || dc.external;
            if c.stereotype.is_none() || matches!(c.stereotype.as_deref(), Some("abstract")) {
                c.stereotype = dc.stereotype.clone().or(c.stereotype.take());
            }
        }
    }
    r
}

pub fn drift(intended: &Design, reality: &Design) -> Drift {
    let reality = with_design_rules(intended, reality);
    let mut out = Drift::default();
    let ic: Vec<&Class> = intended.live_classes().collect();
    let rc: Vec<&Class> = reality.live_classes().collect();
    let in_code = |n: &str| reality.live_class(n);
    let mut score = 0.0;
    let mut total = 0.0;
    for c in &ic {
        total += 1.0;
        let Some(r) = in_code(&c.name) else {
            out.missing_in_code.push(c.name.clone());
            continue;
        };
        score += 0.5;
        // members: names only (types and signatures vary legitimately while sketching)
        let names = |c: &Class| -> BTreeSet<String> {
            c.attributes
                .iter()
                .filter(|a| a.status.is_live())
                .map(|a| a.name.clone())
                .chain(c.methods.iter().filter(|m| m.status.is_live()).map(|m| format!("{}()", m.name)))
                .collect()
        };
        let (dn, rn) = (names(c), names(r));
        let missing: Vec<String> = dn.difference(&rn).cloned().collect();
        let extra: Vec<String> = rn.difference(&dn).cloned().collect();
        score += if dn.is_empty() { 0.5 } else { 0.5 * (dn.len() - missing.len()) as f64 / dn.len() as f64 };
        let mut changed = vec![];
        for a in &c.attributes {
            if let Some(b) = r.attributes.iter().find(|b| b.name == a.name) {
                if (a.ty.as_str(), a.visibility, a.is_static) != (b.ty.as_str(), b.visibility, b.is_static) { changed.push(a.name.clone()); }
            }
        }
        for a in &c.methods {
            let matches: Vec<_> = r.methods.iter().filter(|b| b.name == a.name).collect();
            if matches.len() == 1 {
                let b = matches[0];
                if crate::source::signature(a) != crate::source::signature(b) || a.returns != b.returns || a.visibility != b.visibility || a.is_static != b.is_static || a.is_abstract != b.is_abstract { changed.push(format!("{}()", a.name)); }
            }
        }
        if !missing.is_empty() || !extra.is_empty() || !changed.is_empty() {
            out.members.push(MemberDrift { class: c.name.clone(), missing_in_code: missing, extra_in_code: extra, changed });
        }
        if c.package != r.package {
            out.packages.push(PackageDrift { class: c.name.clone(), design: c.package.clone(), code: r.package.clone() });
        }
    }
    let designed: BTreeSet<&str> = ic.iter().map(|c| c.name.as_str()).collect();
    out.extra_in_code = rc.iter().filter(|c| !designed.contains(c.name.as_str())).map(|c| c.name.clone()).collect();
    out.conformance = if total == 0.0 { 1.0 } else { score / total };

    // relations
    let rm = compute(&reality, false);
    let mut code_deps: BTreeSet<(String, String)> = reality.live_relations().map(|r| (r.from.clone(), r.to.clone())).collect();
    code_deps.extend(rm.implicit_edges.iter().map(|e| (e.from.clone(), e.to.clone())));
    let both = |n: &str| designed.contains(n) && in_code(n).is_some();
    for r in intended.live_relations().filter(|r| both(&r.from) && both(&r.to)) {
        let ok = if is_structural(r.kind) {
            reality.live_relations().any(|x| x.from == r.from && x.to == r.to && is_structural(x.kind))
        } else {
            code_deps.contains(&(r.from.clone(), r.to.clone()))
                || (!r.directed && code_deps.contains(&(r.to.clone(), r.from.clone())))
        };
        if !ok {
            out.relations_missing_in_code.push(RelationDrift { from: r.from.clone(), to: r.to.clone(), kind: r.kind });
        }
    }
    for x in reality.live_relations().filter(|x| both(&x.from) && both(&x.to)) {
        let shown = intended.live_relations().any(|r| {
            (r.from == x.from && r.to == x.to) || (!r.directed && r.from == x.to && r.to == x.from)
        });
        if !shown {
            out.relations_extra_in_code.push(RelationDrift { from: x.from.clone(), to: x.to.clone(), kind: x.kind });
        }
    }

    out.rule_violations_in_code = rm.issues.iter().filter(|i| i.rule.is_some() || i.kind == "cycle").map(|i| i.message.clone()).collect();
    out
}

/// Ops that bring what exists only in the code into the design (never removes anything).
pub fn adopt_ops(intended: &Design, reality: &Design) -> Vec<Op> {
    let dr = drift(intended, reality);
    let mut ops = vec![];
    for name in &dr.extra_in_code {
        let c = reality.class(name).unwrap();
        ops.push(Op::AddClass {
            name: c.name.clone(),
            stereotype: c.stereotype.clone(),
            package: c.package.clone(),
            is_abstract: c.is_abstract,
            attributes: c.attributes.iter().map(|a| Attribute { status: Status::Accepted, proposal: None, origin: None, ..a.clone() }).collect(),
            methods: c.methods.iter().map(|m| Method { status: Status::Accepted, proposal: None, origin: None, code: None, ..m.clone() }).collect(),
            responsibilities: vec![],
            rationale: Some("found in the code".into()),
            aggregate_root: false,
            external: c.external,
            role: None,
        });
    }
    for md in &dr.members {
        let rc = reality.class(&md.class).unwrap();
        for e in &md.extra_in_code {
            if let Some(m) = e.strip_suffix("()") {
                if let Some(m) = rc.methods.iter().find(|x| x.name == m) {
                    ops.push(Op::AddMethod { class: md.class.clone(), method: Method { status: Status::Accepted, proposal: None, origin: None, code: None, ..m.clone() } });
                }
            } else if let Some(a) = rc.attributes.iter().find(|x| &x.name == e) {
                ops.push(Op::AddAttribute { class: md.class.clone(), attribute: Attribute { status: Status::Accepted, proposal: None, origin: None, ..a.clone() } });
            }
        }
    }
    let added: BTreeSet<&str> = dr.extra_in_code.iter().map(String::as_str).collect();
    let known = |n: &str| intended.live_class(n).is_some() || added.contains(n);
    let mut rels: Vec<&Relation> = dr
        .relations_extra_in_code
        .iter()
        .filter_map(|x| reality.live_relations().find(|r| r.from == x.from && r.to == x.to && r.kind == x.kind))
        .collect();
    // relations of adopted classes
    rels.extend(reality.live_relations().filter(|r| (added.contains(r.from.as_str()) || added.contains(r.to.as_str())) && known(&r.from) && known(&r.to)));
    let mut seen = BTreeSet::new();
    for r in rels {
        if seen.insert(r.id.clone()) {
            ops.push(Op::AddRelation {
                from: r.from.clone(),
                to: r.to.clone(),
                kind: r.kind,
                label: r.label.clone(),
                from_mult: r.from_mult.clone(),
                to_mult: r.to_mult.clone(),
                directed: Some(r.directed),
                rationale: Some("found in the code".into()),
                interaction: None,
            });
        }
    }
    ops
}

/// A to-do list for the code (markdown), for what the design has and the code lacks.
pub fn checklist(dr: &Drift) -> String {
    let mut o = String::from("## Bring the code in line with the design\n\n");
    for c in &dr.missing_in_code {
        o.push_str(&format!("- [ ] create class `{c}`\n"));
    }
    for m in &dr.members {
        for x in &m.missing_in_code {
            o.push_str(&format!("- [ ] add `{}.{x}`\n", m.class));
        }
        for x in &m.changed { o.push_str(&format!("- [ ] reconcile signature/type of `{}.{x}`\n", m.class)); }
    }
    for r in &dr.relations_missing_in_code {
        o.push_str(&format!("- [ ] `{}` should {} `{}`\n", r.from, match r.kind {
            RelationKind::Inheritance => "extend",
            RelationKind::Realization => "implement",
            _ => "depend on",
        }, r.to));
    }
    for p in &dr.packages {
        o.push_str(&format!("- [ ] move `{}` to package `{}`\n", p.class, p.design.as_deref().unwrap_or("(none)")));
    }
    for v in &dr.rule_violations_in_code {
        o.push_str(&format!("- [ ] fix: {v}\n"));
    }
    o
}
