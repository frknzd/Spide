//! Deterministic derived views (packages, C4) + staleness info for agent-generated views
//! (domain model, use cases, sequences).

use crate::metrics::Metrics;
use crate::model::*;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct PackageNode {
    pub name: String,
    pub layer: Option<Layer>,
    pub classes: Vec<String>,
    /// Abstract classes and interfaces among `classes` (drawn in italics).
    pub abstract_classes: Vec<String>,
    pub status: Status,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct PackageView {
    pub packages: Vec<PackageNode>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ContainerNode {
    pub name: String,
    pub technology: Option<String>,
    pub description: Option<String>,
    pub replicas: u32,
    pub datastore: bool,
    pub packages: Vec<String>,
    pub classes: usize,
    pub status: Status,
    pub proposal: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ContainerDep {
    pub from: String,
    pub to: String,
    /// Class-level dependencies behind it.
    pub count: usize,
    /// At least one of them is synchronous.
    pub sync: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ContextEdge {
    pub from: String,
    pub to: String,
    pub label: Option<String>,
}

/// C4 context + container levels.
#[derive(Debug, Clone, Serialize, Default)]
pub struct SystemView {
    pub system: String,
    pub actors: Vec<String>,
    pub systems: Vec<ExternalSystem>,
    pub containers: Vec<ContainerNode>,
    /// Derived from class dependencies through package → container.
    pub container_deps: Vec<ContainerDep>,
    pub links: Vec<Link>,
    /// Context level: actors → our system → external systems.
    pub context_edges: Vec<ContextEdge>,
}

pub fn system_view(d: &Design, m: &Metrics) -> SystemView {
    let live = |s: Status| shown(s);
    let container_of = |class: &str| -> Option<String> {
        let pkg = d.class(class)?.package.as_deref()?;
        d.packages.iter().find(|p| p.name == pkg && live(p.status))?.container.clone()
    };
    let mut v = SystemView { system: d.name.clone(), ..Default::default() };
    v.actors = d.use_cases.actors.iter().filter(|a| a.status.is_live()).map(|a| a.name.clone()).collect();
    v.systems = d.systems.iter().filter(|x| live(x.status)).cloned().collect();
    v.links = d.links.iter().filter(|l| live(l.status)).cloned().collect();
    for k in d.containers.iter().filter(|k| live(k.status)) {
        let packages: Vec<String> = d.packages.iter().filter(|p| live(p.status) && p.container.as_deref() == Some(&k.name)).map(|p| p.name.clone()).collect();
        let classes = d.live_classes().filter(|c| c.package.as_ref().is_some_and(|p| packages.contains(p))).count();
        v.containers.push(ContainerNode {
            name: k.name.clone(),
            technology: k.technology.clone(),
            description: k.description.clone(),
            replicas: k.replicas,
            datastore: k.datastore,
            packages,
            classes,
            status: k.status,
            proposal: k.proposal.clone(),
        });
    }
    // container dependencies from class dependencies
    let mut deps: std::collections::BTreeMap<(String, String), (usize, bool)> = Default::default();
    let mut add = |a: &str, b: &str, sync: bool| {
        if let (Some(x), Some(y)) = (container_of(a), container_of(b)) {
            if x != y {
                let e = deps.entry((x, y)).or_default();
                e.0 += 1;
                e.1 |= sync;
            }
        }
    };
    for r in d.live_relations() {
        add(&r.from, &r.to, r.interaction == Interaction::Sync);
    }
    for e in &m.implicit_edges {
        add(&e.from, &e.to, true);
    }
    v.container_deps = deps.into_iter().map(|((from, to), (count, sync))| ContainerDep { from, to, count, sync }).collect();
    // context: actors use the system; the system uses external systems (collapsed from links)
    let ours = |n: &str| d.containers.iter().any(|k| k.name == n);
    let is_ext = |n: &str| d.systems.iter().any(|x| x.name == n);
    let mut ctx: Vec<ContextEdge> = vec![];
    let mut push = |from: String, to: String, label: Option<String>| {
        if !ctx.iter().any(|e| e.from == from && e.to == to) {
            ctx.push(ContextEdge { from, to, label });
        }
    };
    for a in &v.actors {
        let uses: Vec<String> = d
            .use_cases
            .use_cases
            .iter()
            .filter(|u| u.status.is_live() && u.actors.contains(a))
            .map(|u| u.name.clone())
            .take(2)
            .collect();
        push(a.clone(), v.system.clone(), (!uses.is_empty()).then(|| uses.join(", ")));
    }
    for l in &v.links {
        let side = |n: &str| if ours(n) { v.system.clone() } else { n.to_string() };
        if is_ext(&l.from) || is_ext(&l.to) || !ours(&l.from) {
            let (f, t) = (side(&l.from), side(&l.to));
            if f != t {
                push(f, t, l.protocol.clone());
            }
        }
    }
    v.context_edges = ctx;
    v
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct Derived {
    pub packages: PackageView,
    pub system: SystemView,
    /// The domain model was generated from an older class model (or never).
    pub domain_stale: bool,
    /// Use cases were generated from an older class model (or never).
    pub use_cases_stale: bool,
    pub stale_sequences: Vec<String>,
    /// Use cases without a sequence diagram yet.
    pub suggested_scenarios: Vec<String>,
}

fn shown(s: Status) -> bool {
    !matches!(s, Status::Pruned)
}

pub fn package_view(d: &Design) -> PackageView {
    let mut out = PackageView::default();
    for p in d.packages.iter().filter(|p| shown(p.status)) {
        out.packages.push(PackageNode {
            name: p.name.clone(),
            layer: p.layer,
            classes: vec![],
            abstract_classes: vec![],
            status: p.status,
        });
    }
    for c in d.classes.iter().filter(|c| shown(c.status)) {
        let name = c.package.clone().unwrap_or_else(|| "(none)".into());
        let abs = c.is_abstract || c.is_interface();
        let i = match out.packages.iter().position(|p| p.name == name) {
            Some(i) => i,
            None => {
                out.packages.push(PackageNode { name, layer: None, classes: vec![], abstract_classes: vec![], status: Status::Accepted });
                out.packages.len() - 1
            }
        };
        out.packages[i].classes.push(c.name.clone());
        if abs {
            out.packages[i].abstract_classes.push(c.name.clone());
        }
    }
    out
}

pub fn derive(d: &Design, m: &Metrics) -> Derived {
    let has_classes = d.classes.iter().any(|c| c.status.is_live());
    let uc = &d.use_cases;
    let use_cases_stale =
        has_classes && (uc.use_cases.is_empty() || uc.generated_rev.is_none_or(|r| r < d.model_rev));
    let domain_stale = has_classes && (d.domain.is_empty() || d.domain.generated_rev.is_none_or(|r| r < d.model_rev));
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
        packages: package_view(d),
        system: system_view(d, m),
        domain_stale,
        use_cases_stale,
        stale_sequences,
        suggested_scenarios,
    }
}
