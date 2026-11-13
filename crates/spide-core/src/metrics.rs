//! Architecture health metrics, recomputed on every change.

use crate::model::*;
use crate::rules::{self, Preset};
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
    /// The architecture rule that is broken (if any).
    pub rule: Option<String>,
    /// Why this is a problem.
    pub explain: String,
    /// How it could be fixed.
    pub fix_hint: Option<String>,
    /// coupling | cohesion | responsibility | architecture | uml
    pub principle: &'static str,
    /// Course reference, e.g. "GRASP Information Expert (Ch. 3)".
    pub source: &'static str,
    /// 0..1: how sure Spide is (structural facts 1; name heuristics lower; agent agreement raises it).
    pub confidence: f64,
    /// structure | heuristic | agent | heuristic + agent | code
    pub basis: String,
}

impl Issue {
    pub fn new(kind: &'static str, severity: f64, message: String, targets: Vec<String>) -> Self {
        let (explain, fix_hint) = default_explanation(kind);
        let (principle, source) = classify_issue(kind);
        Issue {
            kind,
            severity,
            message,
            targets,
            rule: None,
            explain: explain.into(),
            fix_hint: fix_hint.map(String::from),
            principle,
            source,
            confidence: 1.0,
            basis: "structure".into(),
        }
    }
    fn with_rule(mut self, rule: impl Into<String>, explain: impl Into<String>, fix: impl Into<String>) -> Self {
        self.rule = Some(rule.into());
        self.explain = explain.into();
        self.fix_hint = Some(fix.into());
        self
    }
}

/// Principle (one of the course's basic OO design principles, plus architecture rules and UML
/// hygiene) and course reference per issue kind.
pub fn classify_issue(kind: &str) -> (&'static str, &'static str) {
    match kind {
        "cycle" => ("coupling", "Low Coupling · break cycles (Ch. 2)"),
        "coupling" | "sync_chain" | "spof" => ("coupling", "Low Coupling (Ch. 2)"),
        "demeter" => ("coupling", "Law of Demeter — don't talk to strangers (Ch. 2)"),
        "inheritance_coupling" => ("coupling", "“Inheritance is strong coupling” (Ch. 2)"),
        "god" | "cohesion" | "mixed_concerns" | "vague_role" => ("cohesion", "High Cohesion (Ch. 2)"),
        "large_class" | "long_method" => ("cohesion", "Bad smells: Large Class / Long Method (Ch. 5)"),
        "misplaced_responsibility" | "concept_elsewhere" | "feature_envy" => ("responsibility", "Information Expert · RDD (Ch. 2–3)"),
        "fat_controller" | "ui_bypasses_controller" => ("responsibility", "GRASP Controller (Ch. 3)"),
        "creator" => ("responsibility", "GRASP Creator (Ch. 3)"),
        "data_class" => ("responsibility", "RDD: make objects smart, not just data (Ch. 2)"),
        "duplication" => ("responsibility", "Bad smells: Duplicated Code (Ch. 5)"),
        "scattered_concept" => ("responsibility", "Bad smells: Shotgun Surgery (Ch. 5)"),
        "middle_man" => ("responsibility", "Delegation without responsibility (Ch. 5)"),
        "type_switch" => ("responsibility", "Polymorphism instead of switches (Ch. 2–3)"),
        "role_inconsistency" | "disagreement" => ("responsibility", "RDD roles (Ch. 2)"),
        "change_ripple" | "chatty_scenario" | "multi_aggregate_write" => ("responsibility", "Protected Variations · Information Expert (Ch. 3)"),
        "layer" | "module_boundary" | "forbidden_dep" | "forbidden_type" | "orphan" | "consistency" => ("architecture", "Architecture rules"),
        "long_params" => ("uml", "Bad smells: Long Parameter List (Ch. 5)"),
        _ => ("uml", "Exam notes: UML class diagrams (Ch. 6)"),
    }
}

fn default_explanation(kind: &str) -> (&'static str, Option<&'static str>) {
    match kind {
        "cycle" => ("Classes in a cycle can only be understood, tested and changed together.", Some("break the cycle: invert one dependency via an interface, or move the shared part into its own class")),
        "god" => ("A class with too many members, responsibilities or dependencies changes for many reasons.", Some("split it along its responsibilities (look at the method groups)")),
        "cohesion" => ("Its methods form unrelated groups (LCOM4 > 1): it really is several classes.", Some("extract each method group with the attributes it uses into its own class")),
        "orphan" => ("An unconnected class contributes nothing to any collaboration.", Some("connect it to the classes that use it, or prune it")),
        "consistency" => ("The views disagree: something on one level has no counterpart on another.", Some("connect it on the other level, or prune it")),
        "sync_chain" => ("Synchronous calls across containers multiply latency and failure: if one is down, the whole scenario fails.", Some("make some of the calls asynchronous (queue/event), or move the data to where it is needed")),
        "multi_aggregate_write" => ("Each aggregate is a consistency boundary; changing several in one transaction couples them and causes contention.", Some("change one aggregate per transaction and propagate to the others with domain events (eventual consistency)")),
        "spof" => ("A single instance that several containers depend on takes all of them down when it fails.", Some("run more replicas, or let dependents degrade gracefully (cache, queue, circuit breaker)")),
        "change_ripple" => ("An expected change touches much of the design: the variation point isn't encapsulated.", Some("hide what varies behind an interface (Protected Variations) so the change stays in one place")),
        "chatty_scenario" => ("Many calls for one scenario mean responsibilities are scattered: the behaviour lives far from the data.", Some("move behaviour to the class that owns the data (Information Expert), or add a facade for the scenario")),
        "coupling" => ("High fan-out makes the class fragile: any of its dependencies can break it.", Some("introduce a facade or move behaviour closer to the data it needs")),
        _ => ("", None),
    }
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
    /// Relation ids that break an architecture rule (layering, module boundary, forbidden dependency).
    pub layer_violations: Vec<String>,
    /// Short label per rule-breaking relation id (`layer ↑`, `module`, `forbidden`).
    pub edge_rules: BTreeMap<String, String>,
    pub implicit_edges: Vec<ImplicitEdge>,
    pub issues: Vec<Issue>,
    /// Behavioural scores per live sequence.
    pub scenarios: Vec<crate::scenarios::ScenarioScore>,
    /// Footprint of each change scenario.
    pub changes: Vec<crate::scenarios::ChangeFootprint>,
    /// Roles, method concerns and responsibility findings (RDD/GRASP).
    pub responsibility: crate::responsibility::Report,
    /// Duplicated behaviour, concepts owned elsewhere, scattered concepts, delegation.
    pub overlap: crate::overlap::Report,
    /// 0..100
    pub health: u8,
    /// 0..1 — drives the canvas jitter.
    pub tension: f64,
}


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
    let th = d.rules.thresholds;
    let (god_members, god_responsibilities, high_fan_out) = (th.god_members, th.god_responsibilities, th.high_fan_out.max(1));
    let preset = d.rules.preset;
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

    // ---- architecture rules on every dependency (explicit + implicit)
    let mut out = Metrics::default();
    let mut issues: Vec<Issue> = vec![];
    let mut layer_violations: BTreeSet<String> = BTreeSet::new();
    let mut viol_count: HashMap<String, usize> = HashMap::new();
    let mut rule_penalty = 0.0;
    let edge_label = |e: &Dep| e.rel.clone().unwrap_or_else(|| format!("{} → {} (implicit)", e.from, e.to));
    let mut seen_rule_edges: BTreeSet<(String, String, String)> = BTreeSet::new();
    for e in &deps {
        let (pa, pb) = (pkg_of(&e.from), pkg_of(&e.to));
        let mut broken: Vec<(&'static str, rules::Violation)> = vec![];
        if let (Some(a), Some(b)) = (layer_of(&e.from), layer_of(&e.to)) {
            if let Err(v) = rules::allowed(a, b, preset) {
                broken.push(("layer", v));
            }
        }
        if preset == Preset::ModularMonolith {
            if let (Some(a), Some(b)) = (&pa, &pb) {
                if let Some(v) = rules::module_violation(a, b) {
                    broken.push(("module_boundary", v));
                }
            }
        }
        for r in rules::forbidden_by(&d.rules, &e.from, pa.as_deref(), &e.to, pb.as_deref()) {
            broken.push((
                "forbidden_dep",
                rules::Violation {
                    rule: r.id(),
                    explain: r.reason.clone().unwrap_or_else(|| format!("the rules forbid {} depending on {}", r.from, r.to)),
                    fix_hint: "remove the dependency or route it through an allowed intermediary".into(),
                },
            ));
        }
        for (kind, v) in broken {
            // one issue per edge and rule (bidirectional associations yield two deps)
            if !seen_rule_edges.insert((e.from.clone(), e.to.clone(), v.rule.clone())) {
                continue;
            }
            if let Some(r) = &e.rel {
                layer_violations.insert(r.clone());
                let label = match kind {
                    "layer" => "layer ↑",
                    "module_boundary" => "module",
                    _ => "forbidden",
                };
                out.edge_rules.entry(r.clone()).or_insert_with(|| label.into());
            }
            *viol_count.entry(e.from.clone()).or_default() += 1;
            rule_penalty += if e.rel.is_some() { 5.0 } else { 3.0 };
            let what = match kind {
                "layer" => "layer violation",
                "module_boundary" => "module boundary",
                _ => "forbidden dependency",
            };
            issues.push(
                Issue::new(kind, 0.6, format!("{what}: {} — {}", edge_label(e), v.explain), vec![e.rel.clone().unwrap_or(e.from.clone())])
                    .with_rule(v.rule, v.explain.clone(), v.fix_hint),
            );
        }
    }
    // forbidden types per layer (e.g. no Http* in domain)
    for c in &classes {
        let Some(layer) = layer_of(&c.name) else { continue };
        let mut tys: Vec<(String, &str)> = vec![];
        for a in c.attributes.iter().filter(|a| counts(a.status, ghosts)) {
            tys.push((format!("attribute {}", a.name), a.ty.as_str()));
        }
        for m in c.methods.iter().filter(|m| counts(m.status, ghosts)) {
            for p in &m.params {
                tys.push((format!("{}() parameter {}", m.name, p.name), p.ty.as_str()));
            }
            if let Some(r) = &m.returns {
                tys.push((format!("{}() return", m.name), r.as_str()));
            }
        }
        for tr in d.rules.forbidden_types.iter().filter(|t| t.layer == layer && !t.pattern.is_empty()) {
            for (where_, ty) in &tys {
                if let Some(tok) = rules::type_tokens(ty).find(|tok| rules::glob(&tr.pattern, tok) || rules::glob(&tr.pattern, tok.rsplit('.').next().unwrap_or(tok))) {
                    let explain = tr.reason.clone().unwrap_or_else(|| format!("{} must stay free of {} types", c.package.as_deref().unwrap_or("this layer"), tr.pattern));
                    issues.push(
                        Issue::new("forbidden_type", 0.5, format!("{}: {where_} uses {tok} — {explain}", c.name), vec![c.name.clone()])
                            .with_rule(tr.id(), explain, format!("wrap {tok} behind an interface owned by this layer, or move this code outward")),
                    );
                    *viol_count.entry(c.name.clone()).or_default() += 1;
                    rule_penalty += 4.0;
                }
            }
        }
    }

    let many = classes.len() >= 3;
    // external (library/system) classes: coupling to them is discounted, and they aren't judged
    let external: BTreeSet<&str> = classes.iter().filter(|c| d.is_external(&c.name)).map(|c| c.name.as_str()).collect();
    for c in &classes {
        let n = c.name.as_str();
        if external.contains(n) {
            let fan_in = in_set.get(n).map_or(0, |s| s.len());
            out.classes.insert(n.to_string(), ClassMetrics { fan_in, members: c.attributes.len() + c.methods.len(), ..Default::default() });
            continue;
        }
        let fan_out = out_set.get(n).map_or(0, |s| s.iter().filter(|t| !external.contains(*t)).count());
        let fan_in = in_set.get(n).map_or(0, |s| s.iter().filter(|t| !external.contains(*t)).count());
        let instability = if fan_in + fan_out == 0 { 0.0 } else { fan_out as f64 / (fan_in + fan_out) as f64 };
        let attrs: Vec<&Attribute> = c.attributes.iter().filter(|a| counts(a.status, ghosts)).collect();
        let methods: Vec<&Method> = c.methods.iter().filter(|m| counts(m.status, ghosts)).collect();
        let members = attrs.len() + methods.len();
        let lcom = lcom4(c, &methods);
        let connected = out_set.get(n).is_some_and(|s| !s.is_empty()) || in_set.get(n).is_some_and(|s| !s.is_empty());
        let orphan = many && !connected;
        let cyc = scc_of.contains_key(n);
        let lv = viol_count.get(n).copied().unwrap_or(0);

        let mut cm_issues = vec![];
        let mut god_signals = 0;
        if members > god_members {
            god_signals += 1;
            cm_issues.push(format!("{members} members (> {god_members})"));
        }
        if c.responsibilities.len() > god_responsibilities {
            god_signals += 1;
            cm_issues.push(format!("{} responsibilities", c.responsibilities.len()));
        }
        if fan_out >= high_fan_out {
            god_signals += 1;
            cm_issues.push(format!("depends on {fan_out} classes"));
        }
        if let Some(l) = lcom.filter(|l| *l >= 2) {
            god_signals += 1;
            cm_issues.push(format!("LCOM4 = {l}: methods split into {l} unrelated groups"));
        }
        let god = god_signals >= 2 || members > god_members + 8;
        if cyc {
            cm_issues.push("part of a dependency cycle".into());
        }
        if lv > 0 {
            cm_issues.push(format!("{lv} dependency(ies) break architecture rules"));
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
            issues.push(Issue::new("god", 0.8, format!("{n} looks like a god class: {}", cm_issues.join(", ")), vec![n.into()]));
        } else if let Some(l) = lcom.filter(|l| *l >= 2) {
            issues.push(Issue::new("cohesion", 0.4, format!("{n} has low cohesion (LCOM4 = {l}); consider splitting it"), vec![n.into()]));
        }
        if orphan {
            issues.push(Issue::new("orphan", 0.3, format!("{n} is not connected to anything"), vec![n.into()]));
        }
        if fan_out >= high_fan_out && !god {
            issues.push(Issue::new("coupling", 0.4, format!("{n} depends on {fan_out} classes"), vec![n.into()]));
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
        issues.push(Issue::new("cycle", 0.9, format!("dependency cycle: {}", cyc.join(" ⇄ ")), cyc.clone()));
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
        let lv = matches!((player(a), player(b)), (Some(x), Some(y)) if rules::allowed(x, y, preset).is_err())
            || (preset == Preset::ModularMonolith && rules::module_violation(a, b).is_some());
        out.package_deps.push(PackageDep {
            from: a.clone(),
            to: b.clone(),
            count: *n,
            in_cycle: matches!((pscc.get(a.as_str()), pscc.get(b.as_str())), (Some(x), Some(y)) if x == y),
            layer_violation: lv,
        });
    }
    for pc in &package_cycles {
        issues.push(Issue::new("cycle", 0.7, format!("package cycle: {}", pc.join(" ⇄ ")), pc.clone()));
    }

    // ---- implicit edges for drawing
    for ((from, to), via) in implicit {
        let cyc = in_cycle(&from, &to);
        out.implicit_edges.push(ImplicitEdge { from, to, via, in_cycle: cyc });
    }

    // ---- scenarios (sequences)
    let has_aggregate_roots = classes.iter().any(|c| c.aggregate_root);
    for s in d.sequences.iter().filter(|s| s.status.is_live()) {
        let sc = crate::scenarios::score(s, d);
        if sc.sync_container_calls >= 2 {
            issues.push(Issue::new(
                "sync_chain",
                0.45,
                format!("scenario \"{}\" makes {} synchronous calls across containers", s.name, sc.sync_container_calls),
                vec![format!("sequence:{}", s.name)],
            ));
            rule_penalty += 3.0;
        }
        if has_aggregate_roots && sc.aggregates_touched.len() > 1 {
            issues.push(Issue::new(
                "multi_aggregate_write",
                0.4,
                format!("scenario \"{}\" changes {} aggregates ({}) in one go", s.name, sc.aggregates_touched.len(), sc.aggregates_touched.join(", ")),
                sc.aggregates_touched.clone(),
            ));
            rule_penalty += 2.0;
        }
        if sc.hops > th.max_scenario_hops {
            issues.push(Issue::new(
                "chatty_scenario",
                0.35,
                format!("scenario \"{}\" needs {} calls (> {})", s.name, sc.hops, th.max_scenario_hops),
                vec![format!("sequence:{}", s.name)],
            ));
            rule_penalty += 2.0;
        }
        out.scenarios.push(sc);
    }

    // ---- single points of failure: a single-instance container that several others depend on
    let mut dependents: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for e in &deps {
        if let (Some(a), Some(b)) = (crate::scenarios::container_of(d, &e.from), crate::scenarios::container_of(d, &e.to)) {
            if a != b {
                dependents.entry(b).or_default().insert(a);
            }
        }
    }
    for k in d.containers.iter().filter(|k| counts(k.status, ghosts)) {
        let users = dependents.get(k.name.as_str()).map_or(0, |s| s.len());
        if k.replicas <= 1 && users >= 2 {
            issues.push(Issue::new(
                "spof",
                0.5,
                format!("container {} runs once but {users} containers depend on it (single point of failure)", k.name),
                vec![k.name.clone()],
            ));
            rule_penalty += 3.0;
        }
    }

    // ---- cross-level consistency (C4 ↔ packages ↔ use cases)
    let live_containers: Vec<&Container> = d.containers.iter().filter(|k| counts(k.status, ghosts)).collect();
    let mut consistency = |msg: String, target: String| {
        issues.push(Issue::new("consistency", 0.25, msg, vec![target]));
    };
    if !live_containers.is_empty() {
        for p in d.packages.iter().filter(|p| counts(p.status, ghosts) && p.container.is_none()) {
            if classes.iter().any(|c| c.package.as_deref() == Some(p.name.as_str())) {
                consistency(format!("package {} runs in no container", p.name), p.name.clone());
            }
        }
        for k in live_containers.iter().filter(|k| !k.datastore) {
            if !d.packages.iter().any(|p| counts(p.status, ghosts) && p.container.as_deref() == Some(k.name.as_str())) {
                consistency(format!("container {} has no packages", k.name), k.name.clone());
            }
        }
    }
    for p in d.packages.iter().filter(|p| counts(p.status, ghosts)) {
        if let Some(sys) = &p.system {
            if !d.systems.iter().any(|x| counts(x.status, ghosts) && &x.name == sys) {
                consistency(format!("package {} belongs to external system {sys}, which isn't in the context diagram", p.name), p.name.clone());
            }
        }
    }
    for x in d.systems.iter().filter(|x| counts(x.status, ghosts)) {
        if !d.links.iter().any(|l| counts(l.status, ghosts) && (l.from == x.name || l.to == x.name)) {
            consistency(format!("external system {} is not connected", x.name), x.name.clone());
        }
    }
    for a in d.use_cases.actors.iter().filter(|a| a.status.is_live()) {
        if !d.use_cases.use_cases.iter().any(|u| u.status.is_live() && u.actors.contains(&a.name)) {
            consistency(format!("actor {} takes part in no use case", a.name), a.name.clone());
        }
    }

    // ---- change scenarios: how local is each expected change?
    let dep_set: BTreeSet<(String, String)> = deps.iter().map(|e| (e.from.clone(), e.to.clone())).collect();
    for cs in d.change_scenarios.iter().filter(|c| c.status.is_live()) {
        let fp = crate::scenarios::footprint(cs, d, &dep_set);
        if fp.analysed && classes.len() >= 4 && fp.locality < 0.5 {
            issues.push(Issue::new(
                "change_ripple",
                0.35,
                format!("change \"{}\" ripples through {:.0}% of the classes", cs.name, (1.0 - fp.locality) * 100.0),
                fp.impacted.clone(),
            ));
            rule_penalty += 2.0;
        }
        out.changes.push(fp);
    }

    // ---- responsibilities: does each class do what its name/role says? (RDD, GRASP)
    let resp = crate::responsibility::analyse_with(d, ghosts);
    for f in &resp.findings {
        let target = match &f.method {
            Some(m) => vec![f.class.clone(), format!("{}.{m}", f.class)],
            None => vec![f.class.clone()],
        };
        let mut i = Issue::new(f.kind, f.severity, f.message.clone(), target);
        i.explain = f.explain.clone();
        i.fix_hint = Some(f.fix_hint.clone());
        i.source = f.source;
        i.confidence = f.confidence;
        i.basis = "heuristic".into();
        issues.push(i);
    }
    out.responsibility = resp;

    // ---- overlap: the same behaviour in several places, or in the wrong place
    let ov = crate::overlap::analyse_with(d, 0.5, ghosts);
    let mut push = |kind: &'static str, sev: f64, conf: f64, msg: String, targets: Vec<String>, explain: &str, fix: String| {
        let mut i = Issue::new(kind, sev, msg, targets);
        i.explain = explain.into();
        i.fix_hint = Some(fix);
        i.confidence = conf;
        i.basis = "heuristic".into();
        issues.push(i);
    };
    for p in ov.pairs.iter().filter(|p| p.score >= crate::overlap::DUPLICATE) {
        let (ca, cb) = (p.a.split('.').next().unwrap_or("").to_string(), p.b.split('.').next().unwrap_or("").to_string());
        push(
            "duplication",
            0.3 + (p.score - 0.6).max(0.0),
            p.score,
            if p.code >= crate::overlap::DUPLICATE {
                format!("{}() and {}() have the same implementation ({:.0}% of the normalised code matches)", p.a, p.b, p.code * 100.0)
            } else {
                format!("{}() and {}() look like the same behaviour ({:.0}% similar: {})", p.a, p.b, p.score * 100.0, p.shared.join(", "))
            },
            vec![ca, cb, p.a.clone(), p.b.clone()],
            "Duplicated code is “number one in the stink parade”: every change has to be made twice, and the copies drift apart.",
            "keep one implementation in the class that owns the data (Information Expert), or in a new shared class (Pure Fabrication), and call it from both".into(),
        );
    }
    for c in ov.claims.iter().filter(|c| c.uses_no_own_data) {
        let cls = c.method.split('.').next().unwrap_or("").to_string();
        push(
            "concept_elsewhere",
            0.4,
            0.7,
            format!("{}() deals with “{}”, which {} owns ({}), and uses none of {}'s data", c.method, c.shared.join(" "), c.owner, c.evidence, cls),
            vec![cls, c.method.clone(), c.owner.clone()],
            "The behaviour lives far from the information it needs; the class that has the information should do the work (Information Expert), or the same rule ends up in two places.",
            format!("move it to {} and call it from there", c.owner),
        );
    }
    for x in &ov.scattered {
        push(
            "scattered_concept",
            0.25,
            0.5,
            format!("behaviour about “{}” is spread over {} classes ({})", x.concept, x.classes.len(), x.classes.join(", ")),
            x.classes.clone(),
            "When one kind of change needs small edits in many classes (shotgun surgery), changes are easy to miss.",
            "bring that behaviour together in one class (Move Method / Pure Fabrication)".into(),
        );
    }
    for m in &ov.middle_men {
        push(
            "middle_man",
            0.3,
            0.6,
            format!("{m} mostly forwards calls to other classes"),
            vec![m.clone()],
            "A class that only passes calls on adds coupling without responsibility (unless it is a deliberate facade/controller).",
            "let clients talk to the real owner, or give the class a real responsibility (or mark it a controller/facade if that is its role)".into(),
        );
    }
    out.overlap = ov;

    // ---- course checks (Demeter, Creator, switches, long methods, UML hygiene, …)
    issues.extend(crate::course::check(d, ghosts));

    // ---- the agent's judgement: confirm, dispute or add (current assessments only)
    crate::assess::merge(d, &mut issues);
    for i in issues.iter().filter(|i| i.basis != "structure") {
        rule_penalty += 6.0 * i.severity * i.confidence;
    }

    // ---- health
    let mut penalty = 0.0;
    for c in &cycles {
        penalty += 6.0 + 2.0 * c.len() as f64;
    }
    penalty += 5.0 * package_cycles.len() as f64;
    penalty += rule_penalty;
    for (_, m) in &out.classes {
        if m.god {
            penalty += 8.0;
        } else if m.lcom.unwrap_or(1) >= 2 {
            penalty += 4.0;
        }
        if m.orphan {
            penalty += 3.0;
        }
        if m.fan_out + 1 > high_fan_out {
            penalty += 2.0 * (m.fan_out + 1 - high_fan_out) as f64;
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
    // static methods (class-level helpers) don't belong to any object's state: not part of LCOM
    let ms: Vec<&&Method> = methods.iter().filter(|m| !m.is_abstract && !m.is_static).collect();
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
