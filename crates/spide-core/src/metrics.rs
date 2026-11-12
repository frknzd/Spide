//! Architecture health metrics, recomputed on every change.

use crate::model::*;
use petgraph::algo::tarjan_scc;
use petgraph::graph::{DiGraph, NodeIndex};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap};

#[derive(Debug, Clone, Serialize, Default)]
pub struct ClassMetrics {
    pub fan_in: usize,
    pub fan_out: usize,
    pub instability: f64,
    /// LCOM4: number of connected method groups (1 = cohesive). None if unknown.
    pub lcom: Option<usize>,
    pub members: usize,
    pub in_cycle: bool,
    pub god: bool,
    pub orphan: bool,
    pub layer_violations: usize,
    /// 0..1: how "hot" (problematic/coupled) the class is.
    pub heat: f64,
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct PackageMetrics {
    pub name: String,
    pub layer: Option<Layer>,
    pub classes: usize,
    pub fan_in: usize,
    pub fan_out: usize,
    pub instability: f64,
    pub abstractness: f64,
    /// Distance from the main sequence |A + I − 1|.
    pub distance: f64,
    pub in_cycle: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct PackageDep {
    pub from: String,
    pub to: String,
    pub count: usize,
    pub in_cycle: bool,
    pub layer_violation: bool,
}

/// A dependency that exists through types/calls but has no drawn relation.
#[derive(Debug, Clone, Serialize)]
pub struct ImplicitEdge {
    pub from: String,
    pub to: String,
    pub via: String,
    pub in_cycle: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Issue {
    pub kind: &'static str,
    pub severity: f64,
    pub message: String,
    pub targets: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct Metrics {
    pub classes: BTreeMap<String, ClassMetrics>,
    pub packages: Vec<PackageMetrics>,
    pub package_deps: Vec<PackageDep>,
    pub cycles: Vec<Vec<String>>,
    /// Relation ids that are part of a dependency cycle.
    pub cycle_edges: Vec<String>,
    pub package_cycles: Vec<Vec<String>>,
    /// Relation ids that break the layering rule.
    pub layer_violations: Vec<String>,
    pub implicit_edges: Vec<ImplicitEdge>,
    pub issues: Vec<Issue>,
    /// 0..100
    pub health: u8,
    /// 0..1 — drives the canvas jitter.
    pub tension: f64,
}

pub const GOD_MEMBERS: usize = 16;
pub const GOD_RESPONSIBILITIES: usize = 4;
pub const HIGH_FAN_OUT: usize = 6;

struct Dep {
    from: String,
    to: String,
    /// relation id if explicit
    rel: Option<String>,
}

fn counts(s: Status, ghosts: bool) -> bool {
    s.is_live() || (ghosts && s == Status::Proposed)
}

/// Extract class names referenced in a type expression (`List<Book>`, `Map<String, Member[]>`).
fn type_refs<'a>(ty: &'a str, names: &'a BTreeSet<&str>) -> impl Iterator<Item = &'a str> + 'a {
    ty.split(|c: char| !(c.is_alphanumeric() || c == '_')).filter(move |t| names.contains(t))
}

/// Compute metrics. `ghosts` = also count proposed elements (for previews).
pub fn compute(d: &Design, ghosts: bool) -> Metrics {
    let classes: Vec<&Class> = d.classes.iter().filter(|c| counts(c.status, ghosts)).collect();
    let names: BTreeSet<&str> = classes.iter().map(|c| c.name.as_str()).collect();
    let rels: Vec<&Relation> = d
        .relations
        .iter()
        .filter(|r| counts(r.status, ghosts) && names.contains(r.from.as_str()) && names.contains(r.to.as_str()))
        .collect();

    // ---- dependency edges
    let mut deps: Vec<Dep> = vec![];
    for r in &rels {
        if r.from != r.to {
            deps.push(Dep { from: r.from.clone(), to: r.to.clone(), rel: Some(r.id.clone()) });
            if r.kind == RelationKind::Association && !r.directed {
                deps.push(Dep { from: r.to.clone(), to: r.from.clone(), rel: Some(r.id.clone()) });
            }
        }
    }
    let explicit: BTreeSet<(String, String)> = deps.iter().map(|e| (e.from.clone(), e.to.clone())).collect();
    let mut implicit: BTreeMap<(String, String), String> = BTreeMap::new();
    for c in &classes {
        let mut add = |to: &str, via: String| {
            if to != c.name && !explicit.contains(&(c.name.clone(), to.to_string())) {
                implicit.entry((c.name.clone(), to.to_string())).or_insert(via);
            }
        };
        for a in c.attributes.iter().filter(|a| counts(a.status, ghosts)) {
            for t in type_refs(&a.ty, &names) {
                add(t, format!("attribute {}", a.name));
            }
        }
        for m in c.methods.iter().filter(|m| counts(m.status, ghosts)) {
            for p in &m.params {
                for t in type_refs(&p.ty, &names) {
                    add(t, format!("{}() parameter", m.name));
                }
            }
            if let Some(ret) = &m.returns {
                for t in type_refs(ret, &names) {
                    add(t, format!("{}() return", m.name));
                }
            }
            for call in &m.calls {
                if let Some((cls, _)) = call.split_once('.') {
                    if names.contains(cls) {
                        add(cls, format!("{}() calls {call}", m.name));
                    }
                }
            }
        }
    }
    for ((from, to), via) in &implicit {
        let _ = via;
        deps.push(Dep { from: from.clone(), to: to.clone(), rel: None });
    }

    // ---- graph + SCC
    let mut g: DiGraph<&str, usize> = DiGraph::new();
    let idx: HashMap<&str, NodeIndex> = names.iter().map(|n| (*n, g.add_node(*n))).collect();
    for (i, e) in deps.iter().enumerate() {
        g.add_edge(idx[e.from.as_str()], idx[e.to.as_str()], i);
    }
    let mut cycles: Vec<Vec<String>> = tarjan_scc(&g)
        .into_iter()
        .filter(|c| c.len() > 1)
        .map(|c| {
            let mut v: Vec<String> = c.iter().map(|n| g[*n].to_string()).collect();
            v.sort();
            v
        })
        .collect();
    cycles.sort();
    let scc_of: HashMap<&str, usize> =
        cycles.iter().enumerate().flat_map(|(i, c)| c.iter().map(move |n| (n.as_str(), i))).collect();
    let in_cycle = |a: &str, b: &str| matches!((scc_of.get(a), scc_of.get(b)), (Some(x), Some(y)) if x == y);
    let mut cycle_edges: BTreeSet<String> = BTreeSet::new();
    for e in &deps {
        if in_cycle(&e.from, &e.to) {
            if let Some(r) = &e.rel {
                cycle_edges.insert(r.clone());
            }
        }
    }

    // ---- per-class coupling
    let mut out_set: HashMap<&str, BTreeSet<&str>> = HashMap::new();
    let mut in_set: HashMap<&str, BTreeSet<&str>> = HashMap::new();
    for e in &deps {
        out_set.entry(e.from.as_str()).or_default().insert(e.to.as_str());
        in_set.entry(e.to.as_str()).or_default().insert(e.from.as_str());
    }
    let pkg_of = |n: &str| d.class(n).and_then(|c| c.package.clone());
    let layer_of = |n: &str| {
        pkg_of(n).and_then(|p| d.packages.iter().find(|x| x.name == p && counts(x.status, ghosts)).and_then(|x| x.layer))
    };

    // ---- layering
    let mut layer_violations: BTreeSet<String> = BTreeSet::new();
    let mut viol_count: HashMap<String, usize> = HashMap::new();
    for e in &deps {
        if let (Some(a), Some(b)) = (layer_of(&e.from), layer_of(&e.to)) {
            if a.rank() > b.rank() {
                if let Some(r) = &e.rel {
                    layer_violations.insert(r.clone());
                }
                *viol_count.entry(e.from.clone()).or_default() += 1;
            }
        }
    }

    let mut out = Metrics::default();
    let mut issues: Vec<Issue> = vec![];
    let many = classes.len() >= 3;
    for c in &classes {
        let n = c.name.as_str();
        let fan_out = out_set.get(n).map_or(0, |s| s.len());
        let fan_in = in_set.get(n).map_or(0, |s| s.len());
        let instability = if fan_in + fan_out == 0 { 0.0 } else { fan_out as f64 / (fan_in + fan_out) as f64 };
        let attrs: Vec<&Attribute> = c.attributes.iter().filter(|a| counts(a.status, ghosts)).collect();
        let methods: Vec<&Method> = c.methods.iter().filter(|m| counts(m.status, ghosts)).collect();
        let members = attrs.len() + methods.len();
        let lcom = lcom4(c, &methods);
        let orphan = many && fan_in == 0 && fan_out == 0;
        let cyc = scc_of.contains_key(n);
        let lv = viol_count.get(n).copied().unwrap_or(0);

        let mut cm_issues = vec![];
        let mut god_signals = 0;
        if members > GOD_MEMBERS {
            god_signals += 1;
            cm_issues.push(format!("{members} members (> {GOD_MEMBERS})"));
        }
        if c.responsibilities.len() > GOD_RESPONSIBILITIES {
            god_signals += 1;
            cm_issues.push(format!("{} responsibilities", c.responsibilities.len()));
        }
        if fan_out >= HIGH_FAN_OUT {
            god_signals += 1;
            cm_issues.push(format!("depends on {fan_out} classes"));
        }
        if let Some(l) = lcom.filter(|l| *l >= 2) {
            god_signals += 1;
            cm_issues.push(format!("LCOM4 = {l}: methods split into {l} unrelated groups"));
        }
        let god = god_signals >= 2 || members > GOD_MEMBERS + 8;
        if cyc {
            cm_issues.push("part of a dependency cycle".into());
        }
        if lv > 0 {
            cm_issues.push(format!("{lv} dependency(ies) point outward across layers"));
        }
        if orphan {
            cm_issues.push("not connected to anything".into());
        }

        let coupling = ((fan_in + fan_out) as f64 / 10.0).min(1.0);
        let heat = (0.45 * coupling
            + if cyc { 0.35 } else { 0.0 }
            + if god { 0.4 } else { 0.0 }
            + 0.15 * (lv as f64).min(2.0)
            + if lcom.unwrap_or(1) >= 2 { 0.15 } else { 0.0 })
        .min(1.0);

        if god {
            issues.push(Issue {
                kind: "god",
                severity: 0.8,
                message: format!("{n} looks like a god class: {}", cm_issues.join(", ")),
                targets: vec![n.into()],
            });
        } else if let Some(l) = lcom.filter(|l| *l >= 2) {
            issues.push(Issue {
                kind: "cohesion",
                severity: 0.4,
                message: format!("{n} has low cohesion (LCOM4 = {l}); consider splitting it"),
                targets: vec![n.into()],
            });
        }
        if orphan {
            issues.push(Issue {
                kind: "orphan",
                severity: 0.3,
                message: format!("{n} is not connected to anything"),
                targets: vec![n.into()],
            });
        }
        if fan_out >= HIGH_FAN_OUT && !god {
            issues.push(Issue {
                kind: "coupling",
                severity: 0.4,
                message: format!("{n} depends on {fan_out} classes"),
                targets: vec![n.into()],
            });
        }
        out.classes.insert(
            n.to_string(),
            ClassMetrics {
                fan_in,
                fan_out,
                instability,
                lcom,
                members,
                in_cycle: cyc,
                god,
                orphan,
                layer_violations: lv,
                heat,
                issues: cm_issues,
            },
        );
    }
    for cyc in &cycles {
        issues.push(Issue {
            kind: "cycle",
            severity: 0.9,
            message: format!("dependency cycle: {}", cyc.join(" ⇄ ")),
            targets: cyc.clone(),
        });
    }
    for r in &layer_violations {
        issues.push(Issue {
            kind: "layer",
            severity: 0.6,
            message: format!("layer violation: {r} points from an inner layer outward"),
            targets: vec![r.clone()],
        });
    }

    // ---- packages
    let pkg_name = |n: &str| pkg_of(n).unwrap_or_else(|| "(none)".into());
    let mut pkgs: BTreeMap<String, Vec<&Class>> = BTreeMap::new();
    for c in &classes {
        pkgs.entry(pkg_name(&c.name)).or_default().push(c);
    }
    for p in d.packages.iter().filter(|p| counts(p.status, ghosts)) {
        pkgs.entry(p.name.clone()).or_default();
    }
    let mut pdeps: BTreeMap<(String, String), usize> = BTreeMap::new();
    for e in &deps {
        let (a, b) = (pkg_name(&e.from), pkg_name(&e.to));
        if a != b {
            *pdeps.entry((a, b)).or_default() += 1;
        }
    }
    let mut pg: DiGraph<&str, ()> = DiGraph::new();
    let pidx: HashMap<&str, NodeIndex> = pkgs.keys().map(|k| (k.as_str(), pg.add_node(k.as_str()))).collect();
    for (a, b) in pdeps.keys() {
        pg.add_edge(pidx[a.as_str()], pidx[b.as_str()], ());
    }
    let package_cycles: Vec<Vec<String>> = tarjan_scc(&pg)
        .into_iter()
        .filter(|c| c.len() > 1)
        .map(|c| {
            let mut v: Vec<String> = c.iter().map(|n| pg[*n].to_string()).collect();
            v.sort();
            v
        })
        .collect();
    let pscc: HashMap<&str, usize> =
        package_cycles.iter().enumerate().flat_map(|(i, c)| c.iter().map(move |n| (n.as_str(), i))).collect();
    let player = |p: &str| d.packages.iter().find(|x| x.name == p).and_then(|x| x.layer);
    for (name, cs) in &pkgs {
        let fan_out = pdeps.keys().filter(|(a, _)| a == name).count();
        let fan_in = pdeps.keys().filter(|(_, b)| b == name).count();
        let instability = if fan_in + fan_out == 0 { 0.0 } else { fan_out as f64 / (fan_in + fan_out) as f64 };
        let abs = cs.iter().filter(|c| c.is_abstract || c.is_interface()).count();
        let abstractness = if cs.is_empty() { 0.0 } else { abs as f64 / cs.len() as f64 };
        out.packages.push(PackageMetrics {
            name: name.clone(),
            layer: player(name),
            classes: cs.len(),
            fan_in,
            fan_out,
            instability,
            abstractness,
            distance: (abstractness + instability - 1.0).abs(),
            in_cycle: pscc.contains_key(name.as_str()),
        });
    }
    for ((a, b), n) in &pdeps {
        let lv = matches!((player(a), player(b)), (Some(x), Some(y)) if x.rank() > y.rank());
        out.package_deps.push(PackageDep {
            from: a.clone(),
            to: b.clone(),
            count: *n,
            in_cycle: matches!((pscc.get(a.as_str()), pscc.get(b.as_str())), (Some(x), Some(y)) if x == y),
            layer_violation: lv,
        });
    }
    for pc in &package_cycles {
        issues.push(Issue {
            kind: "cycle",
            severity: 0.7,
            message: format!("package cycle: {}", pc.join(" ⇄ ")),
            targets: pc.clone(),
        });
    }

    // ---- implicit edges for drawing
    for ((from, to), via) in implicit {
        let cyc = in_cycle(&from, &to);
        out.implicit_edges.push(ImplicitEdge { from, to, via, in_cycle: cyc });
    }

    // ---- health
    let mut penalty = 0.0;
    for c in &cycles {
        penalty += 6.0 + 2.0 * c.len() as f64;
    }
    penalty += 5.0 * package_cycles.len() as f64;
    penalty += 5.0 * layer_violations.len() as f64;
    for (_, m) in &out.classes {
        if m.god {
            penalty += 8.0;
        } else if m.lcom.unwrap_or(1) >= 2 {
            penalty += 4.0;
        }
        if m.orphan {
            penalty += 3.0;
        }
        if m.fan_out > HIGH_FAN_OUT - 1 {
            penalty += 2.0 * (m.fan_out + 1 - HIGH_FAN_OUT) as f64;
        }
    }
    let health = (100.0 - penalty).clamp(0.0, 100.0);
    issues.sort_by(|a, b| b.severity.partial_cmp(&a.severity).unwrap());
    out.cycles = cycles;
    out.cycle_edges = cycle_edges.into_iter().collect();
    out.package_cycles = package_cycles;
    out.layer_violations = layer_violations.into_iter().collect();
    out.issues = issues;
    out.health = health.round() as u8;
    out.tension = ((100.0 - health) / 100.0).clamp(0.0, 1.0);
    out
}

/// LCOM4 over methods that declare attribute usage or calls.
fn lcom4(c: &Class, methods: &[&Method]) -> Option<usize> {
    let ms: Vec<&&Method> = methods.iter().filter(|m| !m.is_abstract).collect();
    if ms.len() < 3 || !ms.iter().any(|m| !m.uses.is_empty() || !m.calls.is_empty()) {
        return None;
    }
    let n = ms.len();
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(p: &mut Vec<usize>, x: usize) -> usize {
        if p[x] != x {
            let r = find(p, p[x]);
            p[x] = r;
        }
        p[x]
    }
    let local_call = |call: &str| -> Option<String> {
        match call.split_once('.') {
            Some((cls, m)) if cls == c.name || cls == "this" || cls == "self" => Some(m.to_string()),
            Some(_) => None,
            None => Some(call.to_string()),
        }
    };
    for i in 0..n {
        for j in (i + 1)..n {
            let (a, b) = (ms[i], ms[j]);
            let share = a.uses.iter().any(|u| b.uses.contains(u));
            let calls = a.calls.iter().filter_map(|x| local_call(x)).any(|x| x == b.name)
                || b.calls.iter().filter_map(|x| local_call(x)).any(|x| x == a.name);
            if share || calls {
                let (ra, rb) = (find(&mut parent, i), find(&mut parent, j));
                parent[ra] = rb;
            }
        }
    }
    let roots: BTreeSet<usize> = (0..n).map(|i| find(&mut parent, i)).collect();
    Some(roots.len())
}
