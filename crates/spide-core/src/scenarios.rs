//! Behavioural validation: how well do scenarios (sequences) and expected changes fit the design?

use crate::model::*;
use serde::Serialize;
use std::collections::BTreeSet;

#[derive(Debug, Clone, Serialize, Default, PartialEq)]
pub struct ScenarioScore {
    pub name: String,
    /// Calls (sync, async, create) — returns don't count.
    pub hops: usize,
    /// Deepest nesting of synchronous calls.
    pub max_depth: usize,
    pub package_crossings: usize,
    pub layer_crossings: usize,
    pub container_crossings: usize,
    /// Distinct aggregates (aggregate roots, else entities) receiving calls.
    pub aggregates_touched: Vec<String>,
    pub unknown_calls: usize,
    /// Synchronous calls that cross a container boundary.
    pub sync_container_calls: usize,
}

fn is_aggregate(c: &Class) -> bool {
    c.aggregate_root
}

fn is_entity(c: &Class) -> bool {
    matches!(c.stereotype.as_deref(), Some("entity") | Some("aggregate") | Some("aggregate_root"))
}

/// Container a class runs in (via its package).
pub fn container_of<'a>(d: &'a Design, class: &str) -> Option<&'a str> {
    let pkg = d.live_class(class)?.package.as_deref()?;
    d.packages.iter().find(|p| p.name == pkg && p.status.is_live())?.container.as_deref()
}

pub fn score(s: &Sequence, d: &Design) -> ScenarioScore {
    let mut out = ScenarioScore { name: s.name.clone(), ..Default::default() };
    let pkg = |n: &str| d.live_class(n).and_then(|c| c.package.clone());
    let layer = |n: &str| pkg(n).and_then(|p| d.packages.iter().find(|x| x.name == p).and_then(|x| x.layer));
    let any_aggregate_roots = d.live_classes().any(is_aggregate);
    let mut aggs: BTreeSet<String> = BTreeSet::new();
    // activation stack for depth: a sync call pushes the callee, a return pops back to the caller
    let mut stack: Vec<String> = vec![];
    for m in s.messages.iter().filter(|m| m.status.is_live()) {
        if m.problem.is_some() {
            out.unknown_calls += 1;
        }
        if m.kind == MessageKind::Return {
            if let Some(pos) = stack.iter().rposition(|x| *x == m.from) {
                stack.truncate(pos);
            }
            continue;
        }
        out.hops += 1;
        let (pf, pt) = (pkg(&m.from), pkg(&m.to));
        if pf.is_some() && pt.is_some() && pf != pt {
            out.package_crossings += 1;
        }
        let (lf, lt) = (layer(&m.from), layer(&m.to));
        if lf.is_some() && lt.is_some() && lf != lt {
            out.layer_crossings += 1;
        }
        let (cf, ct) = (container_of(d, &m.from), container_of(d, &m.to));
        if cf.is_some() && ct.is_some() && cf != ct {
            out.container_crossings += 1;
            if m.kind == MessageKind::Sync {
                out.sync_container_calls += 1;
            }
        }
        if let Some(c) = d.live_class(&m.to) {
            if (any_aggregate_roots && is_aggregate(c)) || (!any_aggregate_roots && is_entity(c)) {
                aggs.insert(c.name.clone());
            }
        }
        if m.kind == MessageKind::Sync {
            if let Some(pos) = stack.iter().rposition(|x| *x == m.from) {
                stack.truncate(pos + 1);
            } else {
                stack.clear();
                stack.push(m.from.clone());
            }
            stack.push(m.to.clone());
            out.max_depth = out.max_depth.max(stack.len().saturating_sub(1));
        }
    }
    out.aggregates_touched = aggs.into_iter().collect();
    out
}

/// What an expected change would touch in the current design.
#[derive(Debug, Clone, Serialize, Default, PartialEq)]
pub struct ChangeFootprint {
    pub name: String,
    /// Classes the change must touch (from the analysis).
    pub impacted: Vec<String>,
    /// Classes that depend on an impacted class (may need to change too).
    pub ripple: Vec<String>,
    /// 1 − (impacted + ripple) / classes: 1 = perfectly local.
    pub locality: f64,
    pub packages: Vec<String>,
    pub containers: Vec<String>,
    /// The analysis predates the latest class-model change.
    pub stale: bool,
    /// Has the agent analysed it yet?
    pub analysed: bool,
}

/// `deps` = class-level dependency pairs (from → to).
pub fn footprint(cs: &ChangeScenario, d: &Design, deps: &BTreeSet<(String, String)>) -> ChangeFootprint {
    let live: BTreeSet<&str> = d.live_classes().map(|c| c.name.as_str()).collect();
    let impacted: BTreeSet<String> = cs.impacted.iter().filter(|i| live.contains(i.class.as_str())).map(|i| i.class.clone()).collect();
    let ripple: BTreeSet<String> = deps
        .iter()
        .filter(|(from, to)| impacted.contains(to) && !impacted.contains(from) && live.contains(from.as_str()))
        .map(|(from, _)| from.clone())
        .collect();
    let n = live.len().max(1) as f64;
    let all: Vec<&String> = impacted.iter().chain(ripple.iter()).collect();
    let mut packages: BTreeSet<String> = BTreeSet::new();
    let mut containers: BTreeSet<String> = BTreeSet::new();
    for c in impacted.iter() {
        if let Some(p) = d.live_class(c).and_then(|c| c.package.clone()) {
            packages.insert(p);
        }
        if let Some(k) = container_of(d, c) {
            containers.insert(k.to_string());
        }
    }
    ChangeFootprint {
        name: cs.name.clone(),
        locality: if impacted.is_empty() { 1.0 } else { 1.0 - all.len() as f64 / n },
        impacted: impacted.into_iter().collect(),
        ripple: ripple.into_iter().collect(),
        packages: packages.into_iter().collect(),
        containers: containers.into_iter().collect(),
        stale: cs.generated_rev.is_some_and(|r| r < d.model_rev),
        analysed: cs.generated_rev.is_some(),
    }
}
