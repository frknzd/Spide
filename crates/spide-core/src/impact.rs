//! Consequences of a change: what is added/changed/removed, health score and dimension deltas,
//! which findings change (respecting exceptions), which dependencies flip direction, which rules
//! break, and which scenarios are affected.

use crate::diff::diff;
use crate::metrics::{compute, Metrics};
use crate::model::*;
use crate::ops::{apply_all, Mode, Op, OpError};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct ClassDelta {
    pub class: String,
    pub fan_in: i64,
    pub fan_out: i64,
    /// Heat change in percentage points.
    pub heat: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct DimensionDelta {
    pub dimension: String,
    pub label: String,
    pub before: Option<u8>,
    pub after: Option<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct Impact {
    pub added: Vec<String>,
    pub changed: Vec<String>,
    pub removed: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health_before: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health_after: Option<u8>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub dimension_deltas: Vec<DimensionDelta>,
    pub new_issues: Vec<String>,
    pub resolved_issues: Vec<String>,
    /// Class dependencies whose direction reversed (`from` now depends on `to`, used to be the other way).
    pub flipped_deps: Vec<(String, String)>,
    /// New issues that break an explicit architecture rule.
    pub rule_breaks: Vec<String>,
    /// Sequences / change scenarios that involve touched classes.
    pub affected_scenarios: Vec<String>,
    pub class_deltas: Vec<ClassDelta>,
}

impl Impact {
    /// One-line summary for prompts and option cards.
    pub fn summary(&self) -> String {
        let mut parts = vec![];
        if let (Some(b), Some(a)) = (self.health_before, self.health_after) {
            let d = a as i16 - b as i16;
            if d != 0 {
                parts.push(format!("health {b} → {a} ({}{d})", if d > 0 { "+" } else { "" }));
            }
        }
        let n = |v: &Vec<String>, what: &str| (!v.is_empty()).then(|| format!("{} {what}", v.len()));
        parts.extend(n(&self.added, "added"));
        parts.extend(n(&self.changed, "changed"));
        parts.extend(n(&self.removed, "removed"));
        parts.extend(n(&self.new_issues, "new finding(s)"));
        parts.extend(n(&self.resolved_issues, "resolved finding(s)"));
        parts.extend(n(&self.rule_breaks, "new rule break(s)"));
        if !self.flipped_deps.is_empty() {
            parts.push(format!("{} flipped dependency(ies)", self.flipped_deps.len()));
        }
        if !self.affected_scenarios.is_empty() {
            parts.push(format!("affects {}", self.affected_scenarios.join(", ")));
        }
        if parts.is_empty() { "No changes detected by preview".into() } else { parts.join(" · ") }
    }
}

/// New and resolved finding messages between two metric sets, using unified `health_report`
/// signals (deduplicated by root cause and respecting approved exceptions).
pub fn issue_diff(before: &Metrics, after: &Metrics) -> (Vec<String>, Vec<String>) {
    let old_sigs = before.health_report.unique_signals();
    let new_sigs = after.health_report.unique_signals();
    let old: BTreeSet<&str> = old_sigs.iter().map(|s| s.message.as_str()).collect();
    let new: BTreeSet<&str> = new_sigs.iter().map(|s| s.message.as_str()).collect();
    let describe = |s: &&crate::health::Signal| {
        if s.provenance.contains("heuristic") {
            format!("[heuristic] {}", s.message)
        } else {
            s.message.clone()
        }
    };
    (
        new_sigs.iter().filter(|s| !old.contains(s.message.as_str())).map(describe).collect(),
        old_sigs.iter().filter(|s| !new.contains(s.message.as_str())).map(describe).collect(),
    )
}

/// Preview findings, not a verdict or verification of the change.
pub fn finding_delta(before: &Metrics, after: &Metrics) -> String {
    let (new, resolved) = issue_diff(before, after);
    let mut parts = Vec::new();
    if let (Some(b), Some(a)) = (before.health_report.score, after.health_report.score) {
        let d = a as i16 - b as i16;
        if d != 0 {
            parts.push(format!("Health: {b} → {a} ({}{d}).", if d > 0 { "+" } else { "" }));
        }
    }
    if !new.is_empty() {
        parts.push(format!("New findings: {}.", new.join("; ")));
    }
    if !resolved.is_empty() {
        parts.push(format!("Resolved findings: {}.", resolved.join("; ")));
    }
    if parts.is_empty() { "No finding changes detected by preview.".into() } else { parts.join(" ") }
}

/// Class-level dependency pairs (explicit relations, both ways for bidirectional associations, + implicit).
fn dep_pairs(d: &Design, m: &Metrics) -> BTreeSet<(String, String)> {
    let mut out = BTreeSet::new();
    for r in d.live_relations().filter(|r| r.from != r.to) {
        out.insert((r.from.clone(), r.to.clone()));
        if r.kind == RelationKind::Association && !r.directed {
            out.insert((r.to.clone(), r.from.clone()));
        }
    }
    for e in &m.implicit_edges {
        out.insert((e.from.clone(), e.to.clone()));
    }
    out
}

fn classify(op: &Op) -> (u8, String) {
    // 0 added, 1 changed, 2 removed
    match op {
        Op::AddClass { name, .. } => (0, format!("class {name}")),
        Op::UpdateClass { name, .. } => (1, format!("class {name}")),
        Op::RenameClass { from, to } => (1, format!("{from} → {to}")),
        Op::RemoveClass { name } => (2, format!("class {name}")),
        Op::AddAttribute { class, attribute } => (0, format!("{class}.{}", attribute.name)),
        Op::UpdateAttribute { class, name, .. } => (1, format!("{class}.{name}")),
        Op::RemoveAttribute { class, name } => (2, format!("{class}.{name}")),
        Op::AddMethod { class, method } => (0, format!("{class}.{}()", method.name)),
        Op::UpdateMethod { class, name, .. } => (1, format!("{class}.{name}()")),
        Op::RemoveMethod { class, name } => (2, format!("{class}.{name}()")),
        Op::AddRelation { from, to, kind, .. } => (0, format!("{from} —{}→ {to}", kind.as_str())),
        Op::UpdateRelation { from, to, kind, .. } => (1, format!("{from} —{}→ {to}", kind.as_str())),
        Op::RemoveRelation { from, to, .. } => (2, format!("{from} → {to}")),
        Op::AddPackage { name, .. } => (0, format!("package {name}")),
        Op::SetLayer { package, .. } => (1, format!("layer of {package}")),
        Op::RemovePackage { name } => (2, format!("package {name}")),
        Op::MoveToPackage { class, package } => (1, format!("{class} → {}", package.as_deref().unwrap_or("(no package)"))),
        other => {
            let s = other.summary();
            let bucket = if s.starts_with("add") { 0 } else if s.starts_with("remove") { 2 } else { 1 };
            (bucket, s)
        }
    }
}

thread_local! {
    /// Metrics of recently seen "before" designs: every decision option and proposal is compared with
    /// the same base, so its metrics are computed once per snapshot instead of once per option.
    static BEFORE: std::cell::RefCell<Vec<(String, Metrics)>> = const { std::cell::RefCell::new(Vec::new()) };
}

fn compute_before(d: &Design) -> Metrics {
    let key = crate::assess::fnv(&serde_json::to_string(d).unwrap_or_default());
    if let Some(m) = BEFORE.with(|c| c.borrow().iter().find(|(k, _)| *k == key).map(|(_, m)| m.clone())) {
        return m;
    }
    let m = compute(d, false);
    BEFORE.with(|c| {
        let mut c = c.borrow_mut();
        c.push((key, m.clone()));
        if c.len() > 4 {
            c.remove(0);
        }
    });
    m
}

/// Impact of moving from `before` to `after` (both committed designs).
pub fn impact(before: &Design, after: &Design) -> Impact {
    let mb = compute_before(before);
    let ma = compute(after, false);
    let ops = diff(before, after);
    let mut out = Impact::default();
    let mut touched: BTreeSet<String> = BTreeSet::new();
    for op in &ops {
        let (bucket, label) = classify(op);
        match bucket {
            0 => out.added.push(label),
            1 => out.changed.push(label),
            _ => out.removed.push(label),
        }
        touched.extend(op.touched_classes());
    }
    out.health_before = mb.health_report.score;
    out.health_after = ma.health_report.score;
    for da in &ma.health_report.dimensions {
        let b = mb.health_report.dimension_score(&da.id);
        let a = da.score;
        if b != a {
            out.dimension_deltas.push(DimensionDelta {
                dimension: da.id.clone(),
                label: da.label.clone(),
                before: b,
                after: a,
            });
        }
    }
    let (new, resolved) = issue_diff(&mb, &ma);
    let old_rules: BTreeSet<_> = mb
        .health_report
        .unique_signals()
        .into_iter()
        .filter_map(|s| s.rule_label.as_deref().map(|r| (r, s.message.as_str())))
        .collect();
    out.rule_breaks = ma
        .health_report
        .unique_signals()
        .into_iter()
        .filter(|s| s.rule_label.as_deref().is_some_and(|r| !old_rules.contains(&(r, s.message.as_str()))))
        .map(|s| s.message.clone())
        .collect();
    out.new_issues = new;
    out.resolved_issues = resolved;

    let (db, da) = (dep_pairs(before, &mb), dep_pairs(after, &ma));
    for (a, b) in &da {
        let flipped = !db.contains(&(a.clone(), b.clone())) && db.contains(&(b.clone(), a.clone())) && !da.contains(&(b.clone(), a.clone()));
        if flipped {
            out.flipped_deps.push((a.clone(), b.clone()));
            touched.insert(a.clone());
            touched.insert(b.clone());
        }
    }

    for s in after.sequences.iter().filter(|s| s.status.is_live()) {
        if s.participants.iter().any(|p| touched.contains(s.class_of(p))) {
            out.affected_scenarios.push(format!("sequence \"{}\"", s.name));
        }
    }

    for c in &touched {
        let (b, a) = (mb.classes.get(c), ma.classes.get(c));
        if b.is_none() && a.is_none() {
            continue;
        }
        let fi = |m: Option<&crate::metrics::ClassMetrics>| m.map_or(0, |m| m.fan_in as i64);
        let fo = |m: Option<&crate::metrics::ClassMetrics>| m.map_or(0, |m| m.fan_out as i64);
        let ht = |m: Option<&crate::metrics::ClassMetrics>| m.map_or(0.0, |m| m.heat);
        let delta = ClassDelta {
            class: c.clone(),
            fan_in: fi(a) - fi(b),
            fan_out: fo(a) - fo(b),
            heat: ((ht(a) - ht(b)) * 100.0).round() as i64,
        };
        if delta.fan_in != 0 || delta.fan_out != 0 || delta.heat != 0 {
            out.class_deltas.push(delta);
        }
    }
    out
}

/// Pruned elements don't count for consequences.
/// The design without proposals' ghosts and pruned elements (what metrics judge).
pub fn committed(d: &Design) -> Design {
    committed_base(d)
}

fn committed_base(d: &Design) -> Design {
    let mut b = d.clone();
    b.classes.retain(|c| c.status != Status::Pruned);
    for c in b.classes.iter_mut() {
        c.attributes.retain(|a| a.status != Status::Pruned);
        c.methods.retain(|m| m.status != Status::Pruned);
    }
    b.relations.retain(|r| r.status != Status::Pruned);
    b.packages.retain(|p| p.status != Status::Pruned);
    b
}

/// Impact of applying `ops` to `base` (as if accepted).
pub fn impact_of_ops(base: &Design, ops: &[Op]) -> Result<Impact, OpError> {
    let before = committed_base(base);
    let after = apply_all(&before, ops, Mode::Commit(Status::Accepted, None))?;
    Ok(impact(&before, &after))
}
