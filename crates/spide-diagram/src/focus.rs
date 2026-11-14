//! What a ghost proposal changes, and where it sits in the design (canvas spotlight).

use spide_core::model::{Class, Package, Relation, Status};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Footprint {
    /// proposal ids in focus
    pub ids: BTreeSet<String>,
    /// classes the proposal adds, removes or changes (members)
    pub changed: BTreeSet<String>,
    /// existing classes a proposed relation connects to (context: where it plugs in)
    pub anchors: BTreeSet<String>,
    pub relations: BTreeSet<String>,
    pub packages: BTreeSet<String>,
    /// human-readable change list: (sign, text)
    pub items: Vec<(char, String)>,
}

fn ghostly(s: Status) -> bool {
    matches!(s, Status::Proposed | Status::Removing)
}

pub fn footprint_of(classes: &[Class], relations: &[Relation], packages: &[Package], ids: &BTreeSet<String>) -> Option<Footprint> {
    let mut f = Footprint { ids: ids.clone(), ..Default::default() };
    let sign = |s: Status| if s == Status::Removing { '−' } else { '+' };
    let in_focus = |s: Status, id: &Option<String>| ghostly(s) && id.as_ref().is_some_and(|i| ids.contains(i));
    for c in classes {
        if in_focus(c.status, &c.proposal) {
            f.changed.insert(c.name.clone());
            f.items.push((sign(c.status), format!("class {}{}", c.name, c.package.as_ref().map(|p| format!(" in {p}")).unwrap_or_default())));
        }
        for a in &c.attributes {
            if in_focus(a.status, &a.proposal) {
                f.changed.insert(c.name.clone());
                f.items.push((sign(a.status), format!("{}.{}", c.name, a.name)));
            }
        }
        for m in &c.methods {
            if in_focus(m.status, &m.proposal) {
                f.changed.insert(c.name.clone());
                f.items.push((sign(m.status), format!("{}.{}()", c.name, m.name)));
            }
        }
    }
    for r in relations {
        if !in_focus(r.status, &r.proposal) {
            continue;
        }
        f.relations.insert(r.id.clone());
        f.anchors.insert(r.from.clone());
        f.anchors.insert(r.to.clone());
        f.items.push((sign(r.status), format!("{} → {} ({})", r.from, r.to, r.kind.as_str())));
    }
    for p in packages {
        if in_focus(p.status, &p.proposal) {
            f.packages.insert(p.name.clone());
            f.items.push((sign(p.status), format!("package {}", p.name)));
        }
    }
    let changed = f.changed.clone();
    f.anchors.retain(|a| !changed.contains(a));
    for c in classes {
        if f.changed.contains(&c.name) || f.anchors.contains(&c.name) {
            if let Some(p) = &c.package {
                f.packages.insert(p.clone());
            }
        }
    }
    (!f.items.is_empty()).then_some(f)
}

/// The proposal behind a hovered canvas element, if it is a ghost.
pub fn proposal_at(key: Option<&str>, classes: &[Class], relations: &[Relation]) -> Option<String> {
    let key = key?;
    let i = key.find(':')?;
    let (kind, rest) = (&key[..i], &key[i + 1..]);
    match kind {
        "class" => classes.iter().find(|c| c.name == rest).filter(|c| ghostly(c.status)).and_then(|c| c.proposal.clone()),
        "attribute" | "method" => {
            let dot = rest.find('.')?;
            let c = classes.iter().find(|x| x.name == rest[..dot])?;
            let name = &rest[dot + 1..];
            if kind == "method" {
                c.methods.iter().find(|m| m.name == name).filter(|m| ghostly(m.status)).and_then(|m| m.proposal.clone())
            } else {
                c.attributes.iter().find(|m| m.name == name).filter(|m| ghostly(m.status)).and_then(|m| m.proposal.clone())
            }
        }
        "relation" => relations.iter().find(|r| r.id == rest).filter(|r| ghostly(r.status)).and_then(|r| r.proposal.clone()),
        _ => None,
    }
}

/// What the dock says about one proposal: its own rationale, and a factual change list (never invented).
#[derive(Debug, Clone, PartialEq)]
pub struct ProposalSummary {
    pub id: String,
    pub source: String,
    pub rationale: Option<String>,
    pub items: Vec<(char, String)>,
    pub ops: String,
    pub changes: usize,
}

pub fn proposal_summary(id: &str, op_names: &[String], rationale: Option<&str>, source: &str, classes: &[Class], relations: &[Relation], packages: &[Package]) -> ProposalSummary {
    let f = footprint_of(classes, relations, packages, &[id.to_string()].into());
    let mut counts: Vec<(String, usize)> = vec![];
    for o in op_names {
        match counts.iter_mut().find(|(k, _)| k == o) {
            Some((_, n)) => *n += 1,
            None => counts.push((o.clone(), 1)),
        }
    }
    let ops = counts.iter().map(|(k, n)| format!("{}{}", k.replace('_', " "), if *n > 1 { format!(" ×{n}") } else { String::new() })).collect::<Vec<_>>().join(", ");
    let items = f.map(|f| f.items).unwrap_or_default();
    let changes = if items.is_empty() { op_names.len() } else { items.len() };
    ProposalSummary { id: id.to_string(), source: source.to_string(), rationale: rationale.map(str::trim).filter(|s| !s.is_empty()).map(String::from), items, ops, changes }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spide_core::model::{Method, RelationKind};

    fn cls(name: &str, pkg: &str) -> Class {
        Class { name: name.into(), package: Some(pkg.into()), ..Default::default() }
    }
    fn data() -> (Vec<Class>, Vec<Relation>, Vec<Package>) {
        let mut loan = cls("Loan", "domain");
        loan.methods = vec![Method { name: "renew".into(), status: Status::Proposed, proposal: Some("p2".into()), ..Default::default() }, Method { name: "close".into(), ..Default::default() }];
        let mut audit = cls("AuditLog", "infrastructure");
        audit.status = Status::Proposed;
        audit.proposal = Some("p1".into());
        let classes = vec![loan, cls("LendingService", "application"), audit];
        let relations = vec![
            Relation { id: "r1".into(), from: "LendingService".into(), to: "AuditLog".into(), kind: RelationKind::Dependency, status: Status::Proposed, proposal: Some("p1".into()), ..Default::default() },
            Relation { id: "r0".into(), from: "LendingService".into(), to: "Loan".into(), kind: RelationKind::Association, ..Default::default() },
        ];
        (classes, relations, vec![])
    }
    fn set(v: &[&str]) -> BTreeSet<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn hovered_ghost_resolves_to_its_proposal() {
        let (c, r, _) = data();
        assert_eq!(proposal_at(Some("class:AuditLog"), &c, &r).as_deref(), Some("p1"));
        assert_eq!(proposal_at(Some("relation:r1"), &c, &r).as_deref(), Some("p1"));
        assert_eq!(proposal_at(Some("method:Loan.renew"), &c, &r).as_deref(), Some("p2"));
        assert_eq!(proposal_at(Some("method:Loan.close"), &c, &r), None);
        assert_eq!(proposal_at(Some("class:Loan"), &c, &r), None);
        assert_eq!(proposal_at(None, &c, &r), None);
    }

    #[test]
    fn footprint_changes_anchors_and_packages() {
        let (c, r, p) = data();
        let f = footprint_of(&c, &r, &p, &set(&["p1"])).unwrap();
        assert_eq!(f.changed, set(&["AuditLog"]));
        assert_eq!(f.anchors, set(&["LendingService"]));
        assert_eq!(f.relations, set(&["r1"]));
        assert_eq!(f.packages, set(&["application", "infrastructure"]));
        assert_eq!(f.items.len(), 2);
        let m = footprint_of(&c, &r, &p, &set(&["p2"])).unwrap();
        assert_eq!(m.changed, set(&["Loan"]));
        assert_eq!(m.items, vec![('+', "Loan.renew()".to_string())]);
        assert!(footprint_of(&c, &r, &p, &set(&["nope"])).is_none());
    }

    #[test]
    fn proposal_summary_is_factual() {
        let (c, r, p) = data();
        let s = proposal_summary("p1", &["add_class".into(), "add_relation".into()], Some("  Audit every loan.  "), "agent", &c, &r, &p);
        assert_eq!(s.rationale.as_deref(), Some("Audit every loan."));
        assert_eq!(s.items.len(), 2);
        assert_eq!(s.ops, "add class, add relation");
        let none = proposal_summary("brief", &["set_brief".into(), "set_brief".into()], Some(" "), "agent", &c, &r, &p);
        assert_eq!(none.rationale, None, "no invented rationale");
        assert!(none.items.is_empty());
        assert_eq!(none.ops, "set brief ×2");
    }
}
