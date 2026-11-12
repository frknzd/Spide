//! Deterministic derived views (domain model, packages) + staleness info for
//! agent-generated views (use cases, sequences).

use crate::metrics::Metrics;
use crate::model::*;
use serde::Serialize;

/// Stereotypes / name suffixes that mark technical (non-conceptual) classes.
const TECHNICAL: &[&str] = &[
    "controller", "service", "repository", "dao", "gateway", "adapter", "factory", "manager", "handler",
    "boundary", "view", "presenter", "client", "provider", "mapper", "dto", "util", "helper", "config",
    "interface", "facade", "proxy", "strategy", "listener", "observer",
];

pub fn is_conceptual(c: &Class) -> bool {
    if let Some(s) = &c.stereotype {
        let s = s.to_lowercase();
        if s == "entity" || s == "value" || s == "enum" || s == "abstract" {
            return true;
        }
        if TECHNICAL.iter().any(|t| s.contains(t)) {
            return false;
        }
    }
    let n = c.name.to_lowercase();
    !TECHNICAL.iter().filter(|t| **t != "interface").any(|t| n.ends_with(t))
}

#[derive(Debug, Clone, Serialize)]
pub struct DomainClass {
    pub name: String,
    pub stereotype: Option<String>,
    pub attributes: Vec<Attribute>,
    pub status: Status,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct DomainModel {
    pub classes: Vec<DomainClass>,
    pub relations: Vec<Relation>,
    /// Technical classes left out.
    pub excluded: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PackageNode {
    pub name: String,
    pub layer: Option<Layer>,
    pub classes: Vec<String>,
    pub status: Status,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct PackageView {
    pub packages: Vec<PackageNode>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct Derived {
    pub domain: DomainModel,
    pub packages: PackageView,
    /// Use cases were generated from an older class model (or never).
    pub use_cases_stale: bool,
    pub stale_sequences: Vec<String>,
    /// Use cases without a sequence diagram yet.
    pub suggested_scenarios: Vec<String>,
}

fn shown(s: Status) -> bool {
    !matches!(s, Status::Pruned)
}

pub fn domain_model(d: &Design) -> DomainModel {
    let mut out = DomainModel::default();
    for c in d.classes.iter().filter(|c| shown(c.status)) {
        if is_conceptual(c) {
            out.classes.push(DomainClass {
                name: c.name.clone(),
                stereotype: c.stereotype.clone().filter(|s| s != "entity"),
                attributes: c.attributes.iter().filter(|a| shown(a.status)).cloned().collect(),
                status: c.status,
            });
        } else {
            out.excluded.push(c.name.clone());
        }
    }
    let keep = |n: &str| out.classes.iter().any(|c| c.name == n);
    let rels: Vec<Relation> = d
        .relations
        .iter()
        .filter(|r| shown(r.status) && r.kind != RelationKind::Dependency && r.kind != RelationKind::Realization)
        .filter(|r| keep(&r.from) && keep(&r.to))
        .cloned()
        .collect();
    out.relations = rels;
    out
}

pub fn package_view(d: &Design) -> PackageView {
    let mut out = PackageView::default();
    for p in d.packages.iter().filter(|p| shown(p.status)) {
        out.packages.push(PackageNode {
            name: p.name.clone(),
            layer: p.layer,
            classes: vec![],
            status: p.status,
        });
    }
    for c in d.classes.iter().filter(|c| shown(c.status)) {
        let name = c.package.clone().unwrap_or_else(|| "(none)".into());
        match out.packages.iter_mut().find(|p| p.name == name) {
            Some(p) => p.classes.push(c.name.clone()),
            None => out.packages.push(PackageNode {
                name,
                layer: None,
                classes: vec![c.name.clone()],
                status: Status::Accepted,
            }),
        }
    }
    out
}

pub fn derive(d: &Design, _m: &Metrics) -> Derived {
    let has_classes = d.classes.iter().any(|c| c.status.is_live());
    let uc = &d.use_cases;
    let use_cases_stale =
        has_classes && (uc.use_cases.is_empty() || uc.generated_rev.is_none_or(|r| r < d.model_rev));
    let stale_sequences = d
        .sequences
        .iter()
        .filter(|s| s.generated_rev.is_none_or(|r| r < d.model_rev))
        .map(|s| s.name.clone())
        .collect();
    let suggested_scenarios = uc
        .use_cases
        .iter()
        .filter(|u| u.status.is_live())
        .filter(|u| !d.sequences.iter().any(|s| s.use_case.as_deref() == Some(&u.name) || s.name == u.name))
        .map(|u| u.name.clone())
        .collect();
    Derived {
        domain: domain_model(d),
        packages: package_view(d),
        use_cases_stale,
        stale_sequences,
        suggested_scenarios,
    }
}
