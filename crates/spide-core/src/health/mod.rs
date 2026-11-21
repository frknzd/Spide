//! Design and source evidence: findings, check status, and gaps, not a quality grade.

pub mod profiles;

use crate::evidence::{Category, EvidenceSet, Level, ToolRun};
use crate::metrics::{Issue, Metrics};
use crate::model::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const VERSION: &str = "evidence-1";

/// One finding feeding health: a rule, its evidence and provenance.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct Signal {
    /// Rule id (`cycle`, `complex_function`, `lsp:correctness`, …) and its version.
    pub rule: String,
    pub rule_version: u32,
    pub dimension: String,
    /// Language/profile the rule was evaluated under (`design` without code).
    pub profile: String,
    /// Entity (diagram node) it is charged to, and the precise target (`Class.method`, file).
    pub entity: String,
    pub target: String,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub severity: f64,
    pub confidence: f64,
    /// structure | heuristic | code | lsp | linter | editor | agent
    pub provenance: String,
    pub message: String,
    pub fix_hint: Option<String>,
    /// Same key = same root cause (deduplicated).
    pub root_cause: String,
    /// Suppressed by an approved exception (kept for inspection).
    pub suppressed_by: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct HealthReport {
    pub version: String,
    pub scope: String,
    pub signals: Vec<Signal>,
    pub suppressed: Vec<Signal>,
    pub exceptions: Vec<HealthException>,
    pub runs: Vec<ToolRun>,
    pub gaps: Vec<String>,
    pub languages: Vec<String>,
    pub profiles: Vec<String>,
    /// Language idioms observed, for information only.
    pub idioms: BTreeMap<String, u32>,
    pub explain: String,
}

/// Retired rating, preserved only to round-trip historical project data.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct DimensionAssessment {
    pub dimension: String,
    /// 0..100
    pub score: f64,
    pub rationale: String,
    /// Design classes / code symbols it is based on (at least one).
    pub cited: Vec<String>,
    /// Forces/goals from the brief it weighed.
    pub goals: Vec<String>,
    /// Evidence that argues the other way, acknowledged.
    pub counterevidence: Vec<String>,
    pub confidence: f64,
    pub model: String,
    pub run: String,
    pub rubric_version: String,
    /// Hash of the cited elements when assessed (staleness).
    pub basis_hash: String,
    pub at: u64,
}

/// A contextual exception: a rule doesn't apply to an entity, for a reason.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct HealthException {
    pub id: String,
    /// Rule id (signal.rule) it exempts.
    pub rule: String,
    /// Entity name, `*` suffix allowed (`Legacy*`), or `*` for all.
    pub scope: String,
    pub reason: String,
    pub evidence: Vec<String>,
    pub proposed_by: String,
    /// Only the owner approves (never an agent).
    pub approved: bool,
    pub approved_at: Option<u64>,
    /// ms since epoch; expired exceptions stop applying.
    pub expires_at: Option<u64>,
    /// Re-review condition, e.g. "when payments get a second provider".
    pub review_when: Option<String>,
}

impl HealthException {
    pub fn active(&self, now: u64) -> bool {
        self.approved && self.expires_at.is_none_or(|e| e > now)
    }
    pub fn matches(&self, s: &Signal) -> bool {
        if s.rule != self.rule && self.rule != "*" {
            return false;
        }
        match self.scope.strip_suffix('*') {
            Some(prefix) => s.entity.starts_with(prefix) || s.target.starts_with(prefix),
            None => self.scope == s.entity || self.scope == s.target,
        }
    }
}

/// Add an agent-proposed exception (never approved by the proposer).
pub fn propose_exception(d: &mut Design, mut x: HealthException, proposed_by: &str) -> Result<String, String> {
    if x.rule.trim().is_empty() || x.scope.trim().is_empty() {
        return Err("rule and scope are required".into());
    }
    if x.reason.trim().len() < 15 {
        return Err("give the reason the rule doesn't apply here".into());
    }
    x.id = format!("HX{}", d.health_exceptions.len() + 1);
    while d.health_exceptions.iter().any(|e| e.id == x.id) {
        x.id.push('x');
    }
    x.approved = false;
    x.approved_at = None;
    x.proposed_by = proposed_by.to_string();
    let id = x.id.clone();
    d.health_exceptions.push(x);
    Ok(id)
}

/// Owner decision on an exception.
pub fn decide_exception(d: &mut Design, id: &str, approve: bool, now: u64) -> Result<(), String> {
    let i = d.health_exceptions.iter().position(|e| e.id == id).ok_or_else(|| format!("no exception {id}"))?;
    if approve {
        d.health_exceptions[i].approved = true;
        d.health_exceptions[i].approved_at = Some(now);
    } else {
        d.health_exceptions.remove(i);
    }
    Ok(())
}

/// Inputs beyond the design.
#[derive(Default, Clone, Copy)]
pub struct Context<'a> {
    pub evidence: Option<&'a EvidenceSet>,
    /// Languages of the workspace (for interpreting check status).
    pub languages: &'a [String],
    pub now: u64,
}

fn dimension_of(kind: &str) -> &'static str {
    match kind {
        "instanceof" | "type_switch" | "change_ripple" | "scattered_concept" | "narrowed_visibility" | "inheritance_coupling" | "composite_incomplete" | "degenerate_visitor" | "deep_inheritance" | "volatile_hub" => "extensibility",
        "large_class" | "long_method" | "duplication" | "middle_man" | "long_params" | "chatty_scenario" | "missing_types" | "inherited_repeated" | "orphan" | "complex_function" | "deep_nesting" | "long_function" | "many_params" | "speculative_abstraction" => "simplicity",
        "private_documentation" => "implementation_quality",
        k if k.starts_with("evidence:") => "implementation_quality",
        _ => "modularity",
    }
}

fn entity_of(target: &str, d: &Design) -> String {
    let class = target.split(['.', '#']).next().unwrap_or(target);
    if d.class(class).is_some() {
        class.to_string()
    } else {
        target.to_string()
    }
}

fn from_issue(i: &Issue, d: &Design, dimension: &str) -> Vec<Signal> {
    let targets: Vec<String> = if i.targets.is_empty() { vec!["(design)".into()] } else { i.targets.clone() };
    let profile = targets.first().and_then(|t| d.class(t.split('.').next().unwrap_or(t))).map(|c| profiles::of_class(c).language).unwrap_or("design");
    // a cycle/violation burdens every participant
    targets
        .iter()
        .map(|t| Signal {
            rule: i.kind.to_string(),
            rule_version: if i.kind == "ui_bypasses_controller" { 2 } else { 1 },
            dimension: dimension.into(),
            profile: profile.into(),
            entity: entity_of(t, d),
            target: t.clone(),
            severity: i.severity.clamp(0.0, 1.0),
            confidence: i.confidence.clamp(0.0, 1.0),
            provenance: i.basis.clone(),
            message: i.message.clone(),
            fix_hint: i.fix_hint.clone(),
            root_cause: format!("{}:{}", i.kind, i.targets.join(",")),
            ..Default::default()
        })
        .collect()
}

fn sig(rule: &str, entity: &str, target: &str, profile: &str, severity: f64, confidence: f64, provenance: &str, message: String, fix: &str) -> Signal {
    Signal {
        rule: rule.into(),
        rule_version: 1,
        dimension: dimension_of(rule).into(),
        profile: profile.into(),
        entity: entity.into(),
        target: target.into(),
        severity: severity.clamp(0.0, 1.0),
        confidence,
        provenance: provenance.into(),
        message,
        fix_hint: (!fix.is_empty()).then(|| fix.to_string()),
        root_cause: format!("{rule}:{target}"),
        ..Default::default()
    }
}

/// Code-fact and structure rules evaluated per language profile.
fn profile_signals(d: &Design, m: &Metrics, live: &[&Class], idioms: &mut BTreeMap<String, u32>) -> Vec<Signal> {
    let mut out = vec![];
    for c in live {
        let p = profiles::of_class(c);
        for meth in c.methods.iter().filter(|x| x.status.is_live()) {
            let Some(f) = &meth.code else { continue };
            for (k, n) in &f.idioms {
                *idioms.entry(format!("{}:{k}", p.language)).or_default() += n;
            }
            let target = format!("{}.{}", c.name, meth.name);
            let loc = f.file.clone();
            let cc = f.branches + 1;
            if matches!(p.language, "java" | "design") && f.instanceofs > 0 {
                let mut s = sig("instanceof", &c.name, &target, p.language,
                    0.18 + 0.04 * f.instanceofs.saturating_sub(1).min(4) as f64, 0.9, "code",
                    format!("{target}() uses {} instanceof check(s): type-specific dispatch can require edits for every new node type", f.instanceofs),
                    "review whether polymorphism or a parser registry/class map can isolate new types; instanceof is a smell, not a correctness error");
                s.file = Some(loc.clone());
                s.line = Some(f.start_line);
                out.push(s);
            }
            if d.rules.private_documentation && meth.visibility == crate::model::Visibility::Private
                && !meth.is_abstract && !crate::detail::is_constructor(&c.name, meth) && f.source_documented == Some(false) {
                let mut s = sig("private_documentation", &c.name, &target, p.language, 0.08, 1.0, "code",
                    format!("{target}() has no attached explanatory source comment"),
                    "document the helper's purpose with an explanatory comment or Javadoc; this optional project check measures presence, not comment quality");
                // Keep individual locations, but bound the scoring contribution to one per class.
                s.root_cause = format!("private_documentation:{}", c.name);
                s.file = Some(loc.clone());
                s.line = Some(f.start_line);
                out.push(s);
            }
            let mut push = |mut s: Signal| {
                s.file = Some(loc.clone());
                s.line = Some(f.start_line);
                out.push(s);
            };
            if cc > p.complexity {
                push(sig("complex_function", &c.name, &target, p.language, 0.25 + 0.05 * (cc - p.complexity) as f64, 1.0, "code", format!("{target}() has cyclomatic complexity {cc} (> {} for {})", p.complexity, p.language), "split the decision logic into smaller functions, or replace conditionals with polymorphism/dispatch tables"));
            }
            if f.max_nesting > p.nesting {
                push(sig("deep_nesting", &c.name, &target, p.language, 0.2 + 0.1 * (f.max_nesting - p.nesting) as f64, 1.0, "code", format!("{target}() nests {} levels deep (> {})", f.max_nesting, p.nesting), "return early, extract the inner blocks"));
            }
            let legacy = p.language == "java" || p.language == "design";
            if !legacy && f.loc > p.function_loc {
                push(sig("long_function", &c.name, &target, p.language, 0.15 + 0.2 * ((f.loc - p.function_loc) as f64 / p.function_loc as f64).min(2.0), 1.0, "code", format!("{target}() has {} lines (> {} for {})", f.loc, p.function_loc, p.language), "extract named steps"));
            }
            if !legacy && f.params > p.params {
                push(sig("many_params", &c.name, &target, p.language, 0.2 + 0.05 * (f.params - p.params) as f64, 1.0, "code", format!("{target}() takes {} parameters (> {})", f.params, p.params), "group related parameters into a value/options type"));
            }
        }
        // a module of functions has its own size limit
        if c.is_module() && c.methods.len() > p.module_functions {
            out.push(sig("large_class", &c.name, &c.name, p.language, 0.3 + 0.01 * (c.methods.len() - p.module_functions) as f64, 1.0, "structure", format!("module {} has {} functions (> {} for {})", c.name, c.methods.len(), p.module_functions, p.language), "split the module by the data/feature its functions serve"));
        }
    }
    // extensibility: deep inheritance and volatile hubs; simplicity: speculative abstractions
    let parents: BTreeMap<&str, Vec<&str>> = d.relations.iter().filter(|r| r.status.is_live() && r.kind == RelationKind::Inheritance).fold(BTreeMap::new(), |mut acc, r| {
        acc.entry(r.from.as_str()).or_default().push(r.to.as_str());
        acc
    });
    let depth = |c: &str| {
        let mut depth = 0;
        let mut cur = vec![c];
        let mut seen = BTreeSet::new();
        while !cur.is_empty() && depth < 12 {
            let next: Vec<&str> = cur.iter().flat_map(|x| parents.get(x).cloned().unwrap_or_default()).filter(|x| seen.insert(*x)).collect();
            if next.is_empty() {
                break;
            }
            depth += 1;
            cur = next;
        }
        depth
    };
    let realizers: BTreeMap<&str, usize> = d.relations.iter().filter(|r| r.status.is_live() && matches!(r.kind, RelationKind::Realization | RelationKind::Inheritance)).fold(BTreeMap::new(), |mut acc, r| {
        *acc.entry(r.to.as_str()).or_default() += 1;
        acc
    });
    let mentioned: BTreeSet<&str> = d.change_scenarios.iter().flat_map(|s| s.impacted.iter().map(|i| i.class.as_str())).collect();
    for c in live {
        let p = profiles::of_class(c).language;
        let dpt = depth(&c.name);
        if dpt > 3 {
            out.push(sig("deep_inheritance", &c.name, &c.name, p, 0.2 + 0.1 * (dpt - 3) as f64, 1.0, "structure", format!("{} sits {dpt} levels deep in an inheritance hierarchy", c.name), "prefer composition; flatten the hierarchy"));
        }
        // a contract with behaviour (an interface that only describes a data shape isn't one)
        if c.is_interface() && !c.methods.is_empty() && live.len() >= 4 && realizers.get(c.name.as_str()).copied().unwrap_or(0) <= 1 && !mentioned.contains(c.name.as_str()) {
            let fan_in = m.classes.get(&c.name).map(|x| x.fan_in).unwrap_or(0);
            if fan_in <= 2 {
                out.push(sig("speculative_abstraction", &c.name, &c.name, p, 0.2, 0.6, "heuristic", format!("{} is an abstraction with {} implementation(s), few clients and no expected change that needs it", c.name, realizers.get(c.name.as_str()).copied().unwrap_or(0)), "inline it until a second variant or an expected change needs the seam (or record that change scenario)"));
            }
        }
        if let Some(cm) = m.classes.get(&c.name) {
            if cm.fan_in >= 4 && cm.instability > 0.6 && !c.is_interface() && !c.is_module() {
                out.push(sig("volatile_hub", &c.name, &c.name, p, 0.3, 0.8, "structure", format!("{} is depended on by {} classes but itself depends on much (instability {:.2})", c.name, cm.fan_in, cm.instability), "make what many depend on stable: depend on an abstraction, or move its dependencies out"));
            }
        }
    }
    out
}

/// Evidence findings → implementation-quality signals charged to the class whose code range
/// contains them (else the file).
fn evidence_signals(d: &Design, ev: &EvidenceSet) -> Vec<Signal> {
    // (file, start, end) → class
    let mut ranges: Vec<(&str, u32, u32, &str)> = vec![];
    for c in d.classes.iter().filter(|c| c.status.is_live()) {
        if let Some(s) = &c.source {
            ranges.push((s.file.as_str(), s.start_line, s.end_line.max(s.start_line), c.name.as_str()));
        }
        for m in &c.methods {
            if let Some(s) = &m.source {
                ranges.push((s.file.as_str(), s.start_line, s.end_line.max(s.start_line), c.name.as_str()));
            }
        }
    }
    ev.findings
        .iter()
        .filter(|f| f.provider != "editor-unsaved")
        .map(|f| {
            let owner = ranges.iter().filter(|(file, a, b, _)| *file == f.file && *a <= f.range.line && f.range.line <= *b).min_by_key(|(_, a, b, _)| b - a).map(|x| x.3.to_string());
            let w = match (f.category, f.level) {
                (Category::Correctness | Category::Safety, Level::Error) => 0.9,
                (Category::Correctness | Category::Safety, _) => 0.5,
                (Category::Maintainability | Category::Complexity | Category::Performance, Level::Error) => 0.5,
                (Category::Maintainability | Category::Complexity | Category::Performance, Level::Warning) => 0.25,
                (Category::Style, _) => 0.05,
                (Category::Unknown, Level::Error) => 0.5,
                (_, Level::Warning) => 0.15,
                _ => 0.05,
            };
            let entity = owner.unwrap_or_else(|| f.file.clone());
            Signal {
                rule: format!("evidence:{}", f.category.as_str()),
                rule_version: 1,
                dimension: "implementation_quality".into(),
                profile: f.language.clone(),
                entity: entity.clone(),
                target: format!("{}:{}", f.file, f.range.line),
                file: Some(f.file.clone()),
                line: Some(f.range.line),
                severity: w,
                confidence: if f.provider == "linter" || f.provider == "lsp" || f.provider == "editor" { 1.0 } else { 0.8 },
                provenance: f.provider.clone(),
                message: format!("{} [{}{}]: {}", f.tool, f.code, if f.level == Level::Error { ", error" } else { "" }, f.message.lines().next().unwrap_or("")),
                fix_hint: None,
                root_cause: format!("{}:{}:{}", f.file, f.range.line, f.code),
                suppressed_by: None,
            }
        })
        .collect()
}

/// The health report for a design (and, when present, its code facts and evidence).
pub fn report(d: &Design, m: &Metrics, ctx: Context) -> HealthReport {
    let live: Vec<&Class> = d.classes.iter().filter(|c| c.status.is_live() && !d.is_external(&c.name)).collect();
    let mut idioms = BTreeMap::new();
    // ---- signals
    let mut signals: Vec<Signal> = vec![];
    for i in &m.issues {
        // profile applicability of the course's OO heuristics
        let applicable = i.targets.iter().filter_map(|t| d.class(t.split('.').next().unwrap_or(t))).all(|c| profiles::applies(i.kind, c));
        if !applicable {
            continue;
        }
        signals.extend(from_issue(i, d, dimension_of(i.kind)));
    }
    signals.extend(profile_signals(d, m, &live, &mut idioms));
    if let Some(ev) = ctx.evidence {
        signals.extend(evidence_signals(d, ev));
    }
    // ---- exceptions
    let mut suppressed = vec![];
    signals.retain_mut(|s| match d.health_exceptions.iter().find(|x| x.active(ctx.now) && x.matches(s)) {
        Some(x) => {
            let mut c = s.clone();
            c.suppressed_by = Some(x.id.clone());
            suppressed.push(c);
            false
        }
        None => true,
    });
    let languages: Vec<_> = ctx.languages.iter().cloned()
        .chain(live.iter().filter_map(|c| profiles::language_of(c).map(str::to_string)))
        .collect::<BTreeSet<_>>().into_iter().collect();
    let mut runs = ctx.evidence.map(|e| e.runs.clone()).unwrap_or_default();
    runs.sort_by(|a, b| (&a.language, &a.tool).cmp(&(&b.language, &b.tool)));
    let mut gaps = Vec::new();
    if live.is_empty() {
        gaps.push("No internal design or indexed code units to inspect.".into());
    } else if !live.iter().any(|c| c.methods.iter().any(|m| m.code.is_some())) {
        gaps.push("No function body facts available.".into());
    }
    if runs.is_empty() {
        gaps.push("No tool-run status available.".into());
    }
    for lang in &languages {
        if !runs.iter().any(|r| &r.language == lang && r.ran()) {
            gaps.push(format!("No successful analysis run recorded for {lang}."));
        }
    }
    if let Some(ev) = ctx.evidence {
        if ev.stale_findings > 0 {
            gaps.push(format!("{} stale diagnostic(s) excluded; rerun analysis for current evidence.", ev.stale_findings));
        }
    }
    if d.change_scenarios.is_empty() {
        gaps.push("No expected change scenarios recorded.".into());
    } else if m.changes.iter().any(|c| !c.analysed || c.stale) {
        gaps.push("Some change scenarios have no current analysis.".into());
    }
    let compare = |a: &Signal, b: &Signal| {
        (&a.dimension, &a.rule, &a.target, &a.provenance, &a.message)
            .cmp(&(&b.dimension, &b.rule, &b.target, &b.provenance, &b.message))
    };
    signals.sort_by(compare);
    suppressed.sort_by(compare);
    HealthReport {
        version: VERSION.into(),
        scope: "design".into(),
        signals,
        suppressed,
        exceptions: d.health_exceptions.clone(),
        runs,
        gaps,
        languages,
        profiles: live.iter().map(|c| profiles::of_class(c).language.to_string()).collect::<BTreeSet<_>>().into_iter().collect(),
        idioms,
        explain: "Findings describe available checks only. Heuristics are leads, not verdicts; no findings does not establish design quality.".into(),
    }
}

#[cfg(test)]
mod tests;
