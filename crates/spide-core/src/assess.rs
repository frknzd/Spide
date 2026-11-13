//! The agent's judgement, layered on Spide's deterministic signals.
//!
//! The agent records per class what it is for (purpose, role) and, per method, what it does and
//! whether it belongs there (`assess_class`). Each assessment remembers a content hash of the class
//! (and of the method source when judged from code); when the class changes it becomes stale and
//! no longer counts until the agent re-assesses it.
//!
//! Merging: agreement raises an issue's confidence; an agent verdict "fits" lowers a heuristic
//! issue's confidence (and, on a high-severity issue, adds a visible disagreement); a "misplaced" or
//! "duplicate" verdict without a heuristic issue becomes an issue of its own.

use crate::metrics::Issue;
use crate::model::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct MethodAssessment {
    pub name: String,
    /// presentation | coordination | domain_logic | persistence | formatting | creation | accessor
    pub concern: Option<String>,
    /// knowing | doing | deciding
    pub rdd: Option<String>,
    /// fits | misplaced | duplicate | delegates
    pub verdict: String,
    /// Better owner (a class, or a new class name) when misplaced/duplicate.
    pub home: Option<String>,
    /// For duplicates: the other `Class.method`.
    pub duplicate_of: Option<String>,
    pub confidence: f64,
    pub reason: String,
    /// Hash of the method's source when judged from code.
    pub code_hash: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Assessment {
    pub class: String,
    /// What the class is for, in one sentence.
    pub purpose: Option<String>,
    pub role: Option<String>,
    pub methods: Vec<MethodAssessment>,
    /// design | code
    pub basis: String,
    /// Content hash of the class when assessed (staleness).
    pub hash: String,
    pub at: u64,
}

/// FNV-1a, hex: a stable content hash.
pub fn fnv(text: &str) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in text.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

/// Hash of what an assessment depends on: the class's name, role-relevant metadata and members.
pub fn class_hash(c: &Class) -> String {
    let mut s = format!("{}|{:?}|{:?}|{:?}|{}", c.name, c.stereotype, c.package, c.role, c.responsibilities.join(";"));
    for a in &c.attributes {
        s.push_str(&format!("|a:{}:{}", a.name, a.ty));
    }
    for m in &c.methods {
        let ps: Vec<&str> = m.params.iter().map(|p| p.ty.as_str()).collect();
        s.push_str(&format!("|m:{}({}):{:?}:{}:{}:{:?}", m.name, ps.join(","), m.returns, m.uses.join(","), m.calls.join(","), m.concern));
    }
    fnv(&s)
}

/// The class's assessment if it still matches the class (not stale).
pub fn current<'a>(d: &'a Design, class: &str) -> Option<&'a Assessment> {
    let a = d.assessments.get(class)?;
    let c = d.class(class)?;
    (a.hash == class_hash(c)).then_some(a)
}

/// Classes whose assessment is outdated, and (non-external, live) classes never assessed.
pub fn stale_and_missing(d: &Design) -> (Vec<String>, Vec<String>) {
    let mut stale = vec![];
    let mut missing = vec![];
    for c in d.classes.iter().filter(|c| c.status.is_live() && !d.is_external(&c.name)) {
        match d.assessments.get(&c.name) {
            Some(a) if a.hash != class_hash(c) => stale.push(c.name.clone()),
            Some(_) => {}
            None => missing.push(c.name.clone()),
        }
    }
    (stale, missing)
}

/// Validate and record assessments (from `assess_class`). Returns warnings.
pub fn record(d: &mut Design, items: Vec<Assessment>, now: u64) -> Result<Vec<String>, String> {
    let mut warnings = vec![];
    for mut a in items {
        let Some(c) = d.class(&a.class).filter(|c| c.status.is_live()).cloned() else {
            return Err(format!("unknown class {}", a.class));
        };
        if let Some(r) = &a.role {
            crate::responsibility::check_role(r)?;
        }
        let names: BTreeSet<&str> = c.methods.iter().map(|m| m.name.as_str()).collect();
        for m in &mut a.methods {
            if !names.contains(m.name.as_str()) {
                warnings.push(format!("{}.{}() doesn't exist; ignored", a.class, m.name));
                continue;
            }
            if let Some(cn) = &m.concern {
                crate::responsibility::check_concern(cn)?;
            }
            if !["fits", "misplaced", "duplicate", "delegates"].contains(&m.verdict.as_str()) {
                return Err(format!("{}.{}(): verdict must be fits|misplaced|duplicate|delegates", a.class, m.name));
            }
            m.confidence = if m.confidence <= 0.0 { 0.7 } else { m.confidence.min(1.0) };
            if a.basis == "code" && m.code_hash.is_none() {
                m.code_hash = c.methods.iter().find(|x| x.name == m.name).and_then(|x| x.code.as_ref()).map(|k| k.hash.clone());
            }
        }
        a.methods.retain(|m| names.contains(m.name.as_str()));
        if a.basis.is_empty() {
            a.basis = "design".into();
        }
        a.hash = class_hash(&c);
        a.at = now;
        d.assessments.insert(a.class.clone(), a);
    }
    Ok(warnings)
}

/// Issue kinds the agent's verdicts can confirm or dispute.
const JUDGED: &[&str] = &["misplaced_responsibility", "fat_controller", "duplication", "concept_elsewhere", "mixed_concerns", "role_inconsistency", "feature_envy", "demeter", "ui_bypasses_controller", "data_class"];

fn method_of(i: &Issue) -> Option<(String, String)> {
    i.targets.iter().find_map(|t| t.split_once('.').map(|(c, m)| (c.to_string(), m.to_string())))
}

/// Merge current (non-stale) assessments into the issues.
pub fn merge(d: &Design, issues: &mut Vec<Issue>) {
    let mut covered: BTreeSet<(String, String)> = BTreeSet::new();
    let mut extra: Vec<Issue> = vec![];
    for i in issues.iter_mut().filter(|i| JUDGED.contains(&i.kind)) {
        let Some((c, m)) = method_of(i) else { continue };
        let Some(a) = current(d, &c) else { continue };
        let Some(v) = a.methods.iter().find(|x| x.name == m) else { continue };
        covered.insert((c.clone(), m.clone()));
        match v.verdict.as_str() {
            "misplaced" | "duplicate" => {
                i.confidence = 1.0 - (1.0 - i.confidence) * (1.0 - v.confidence);
                i.basis = "heuristic + agent".into();
                i.message = format!("{} — agent agrees: {}", i.message, v.reason);
                if let Some(h) = &v.home {
                    i.fix_hint = Some(format!("move it to {h} (agent) · {}", i.fix_hint.clone().unwrap_or_default()));
                }
            }
            "fits" | "delegates" => {
                let high = i.severity >= 0.5;
                i.confidence *= 1.0 - v.confidence;
                i.basis = "heuristic (agent disagrees)".into();
                i.message = format!("{} — agent disagrees: {}", i.message, v.reason);
                if high && v.confidence >= 0.5 {
                    let mut n = Issue::new("disagreement", 0.2, format!("Spide and the agent disagree about {c}.{m}(): {}", v.reason), vec![c.clone(), format!("{c}.{m}")]);
                    n.explain = "Spide's signals flag this method, the agent says it fits. Either can be wrong: the examiner should make you justify the choice.".into();
                    n.fix_hint = Some("defend where it lives (forces, coupling, cohesion), or move it".into());
                    n.confidence = 1.0;
                    n.basis = "agent".into();
                    extra.push(n);
                }
            }
            _ => {}
        }
    }
    // agent findings Spide's signals missed
    for (c, a) in &d.assessments {
        if current(d, c).is_none() {
            continue;
        }
        for v in a.methods.iter().filter(|v| matches!(v.verdict.as_str(), "misplaced" | "duplicate")) {
            if covered.contains(&(c.clone(), v.name.clone())) {
                continue;
            }
            let kind = if v.verdict == "duplicate" { "duplication" } else { "misplaced_responsibility" };
            let mut n = Issue::new(
                kind,
                if v.verdict == "duplicate" { 0.4 } else { 0.45 },
                format!(
                    "{c}.{}() {} (agent{}): {}",
                    v.name,
                    if v.verdict == "duplicate" { "duplicates behaviour" } else { "is in the wrong class" },
                    v.duplicate_of.as_ref().map(|x| format!(", same as {x}")).unwrap_or_default(),
                    v.reason
                ),
                vec![c.clone(), format!("{c}.{}", v.name)],
            );
            n.confidence = v.confidence;
            n.basis = "agent".into();
            if let Some(h) = &v.home {
                n.fix_hint = Some(format!("move it to {h}"));
            }
            extra.push(n);
        }
    }
    issues.extend(extra);
}
