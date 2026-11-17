//! Health v2: four quality dimensions — extensibility, modularity, simplicity and implementation
//! quality — computed from evidence with explicit coverage, language profiles, owner-approved
//! exceptions, and a bounded, visible agent contribution.
//!
//! Formula (version `health-2`), per dimension:
//! * signals are grouped by entity; an entity's penalty combines its signals as independent
//!   risks, `p = 1 − Π(1 − severity·confidence)`, so correlated findings about one root cause
//!   saturate instead of adding up;
//! * `load = Σp / n + 0.15 · max p` over the `n` entities eligible for the dimension: a rate, so
//!   unrelated files don't dilute or inflate it, plus a bounded hotspot term so one severe
//!   problem stays visible in a large codebase;
//! * `measured = 100 · e^(−2.2 · load)`;
//! * with a current agent assessment: `assisted = 0.6 · measured + 0.4 · agent` (agent-only
//!   dimensions are judgement-only and add no coverage). Verified correctness errors cap the
//!   assisted implementation-quality score at its measured value.
//!
//! Overall = weighted geometric mean of the known dimensions (owner priorities, else equal).
//! Unknown dimensions are excluded and make the score provisional; nothing measured = unassessed.

pub mod profiles;

use crate::evidence::{Category, EvidenceSet, Level};
use crate::metrics::{Issue, Metrics};
use crate::model::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const VERSION: &str = "health-2";
pub const RUBRIC_VERSION: &str = "rubric-1";
pub const AGENT_WEIGHT: f64 = 0.4;
const BETA: f64 = 2.2;
const HOTSPOT: f64 = 0.15;

pub const DIMENSIONS: [&str; 4] = ["extensibility", "modularity", "simplicity", "implementation_quality"];

pub fn is_dimension(d: &str) -> bool {
    DIMENSIONS.contains(&d)
}

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
pub struct DimensionScore {
    pub measured: Option<f64>,
    pub agent: Option<f64>,
    pub assisted: Option<f64>,
    /// 0..1: how much of what this dimension needs was observed.
    pub coverage: f64,
    pub signals: usize,
    pub entities: usize,
    /// Only the agent's judgement exists for it.
    pub judgement_only: bool,
    pub weight: f64,
    /// Why coverage is partial / what is unknown.
    pub gaps: Vec<String>,
}

impl DimensionScore {
    pub fn best(&self) -> Option<f64> {
        self.assisted.or(self.measured)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct HealthReport {
    pub version: String,
    /// Final score (assisted when the agent contributed), None = unassessed.
    pub score: Option<u8>,
    /// Reproducible score from evidence only.
    pub measured: Option<u8>,
    pub assisted: Option<u8>,
    pub provisional: bool,
    /// 0..100 overall evidence coverage.
    pub coverage: u8,
    pub dimensions: BTreeMap<String, DimensionScore>,
    /// Highest-impact signals first (correctness errors always on top).
    pub top: Vec<Signal>,
    pub signals: Vec<Signal>,
    pub suppressed: Vec<Signal>,
    /// Verified correctness/safety errors (shown regardless of score).
    pub blocking: Vec<Signal>,
    pub languages: Vec<String>,
    pub profiles: Vec<String>,
    /// Idioms observed (informational: never penalised by presence).
    pub idioms: BTreeMap<String, u32>,
    /// Agent assessments that are out of date (re-assess them).
    pub stale_assessments: Vec<String>,
    /// The previous formula's score, kept during migration.
    pub legacy: u8,
    pub explain: String,
}

/// The agent's judgement of one dimension.
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

/// What the agent is asked to judge, per dimension, with score anchors. Versioned: a new rubric
/// makes existing assessments stale.
pub const RUBRIC: &str = "Score each dimension 0–100 for THIS project's goals (the brief's forces) and its languages' idioms. Cite the classes/modules you based it on (they make the judgement stale when they change), list counterevidence, and say what would raise the score.
- extensibility: can the expected changes (change scenarios, forces) be made by adding code at a seam rather than editing many places? 90+: variation points match the expected changes; 60: some changes ripple; 30: most changes touch many modules. Don't reward abstractions nobody needs.
- modularity: clear boundaries, dependencies point one way, each module/class has one reason to change, data and behaviour live together (or, in functional/procedural code, pure functions separated from effects). 90+: acyclic, cohesive, minimal coupling; 30: tangled.
- simplicity: the least machinery that does the job; small functions, no speculative generality, no duplicated logic, idiomatic for the language. 90+: obvious; 30: hard to follow.
- implementation_quality: correctness risks, error handling, resource/ownership safety, tests where they matter, consistency with the language's conventions (read the code: get_source). Verified compiler/linter errors cap this regardless of your score.";

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

/// Dimensions whose agent judgement is missing or stale.
pub fn assessment_needed(d: &Design) -> Vec<(String, &'static str)> {
    DIMENSIONS
        .iter()
        .filter_map(|dim| match d.health_assessments.get(*dim) {
            None => Some((dim.to_string(), "missing")),
            Some(a) if !assessment_current(d, a) => Some((dim.to_string(), "stale")),
            _ => None,
        })
        .collect()
}

/// Hash of what an assessment cites (classes by content hash; unknown names by name).
pub fn basis_hash(d: &Design, cited: &[String]) -> String {
    let mut parts: Vec<String> = cited
        .iter()
        .map(|c| {
            let class = c.split(['.', '#']).next().unwrap_or(c);
            match d.class(class) {
                Some(cl) => format!("{class}:{}", crate::assess::class_hash(cl)),
                None => format!("{class}:missing"),
            }
        })
        .collect();
    parts.sort();
    crate::assess::fnv(&format!("{RUBRIC_VERSION}|{}", parts.join("|")))
}

pub fn assessment_current(d: &Design, a: &DimensionAssessment) -> bool {
    !a.cited.is_empty() && a.rubric_version == RUBRIC_VERSION && a.basis_hash == basis_hash(d, &a.cited)
}

/// Validate and record dimension assessments (agent). Returns warnings.
pub fn record(d: &mut Design, items: Vec<DimensionAssessment>, now: u64) -> Result<Vec<String>, String> {
    let mut warnings = vec![];
    for mut a in items {
        if !is_dimension(&a.dimension) {
            return Err(format!("unknown dimension {} ({})", a.dimension, DIMENSIONS.join(", ")));
        }
        if !(0.0..=100.0).contains(&a.score) {
            return Err("score must be 0..100".into());
        }
        if a.cited.is_empty() {
            return Err(format!("{}: cite the classes/symbols the judgement is based on", a.dimension));
        }
        if a.rationale.trim().len() < 20 {
            return Err(format!("{}: give a rationale (why this score, what would raise it)", a.dimension));
        }
        for c in &a.cited {
            let class = c.split(['.', '#']).next().unwrap_or(c);
            if d.class(class).is_none() {
                warnings.push(format!("{c} is not a design class: it can't make the assessment stale"));
            }
        }
        a.confidence = if a.confidence <= 0.0 { 0.7 } else { a.confidence.min(1.0) };
        a.rubric_version = RUBRIC_VERSION.into();
        a.basis_hash = basis_hash(d, &a.cited);
        a.at = now;
        d.health_assessments.insert(a.dimension.clone(), a);
    }
    Ok(warnings)
}

/// Inputs beyond the design.
#[derive(Default, Clone, Copy)]
pub struct Context<'a> {
    pub evidence: Option<&'a EvidenceSet>,
    /// Languages of the workspace (for coverage).
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

fn score_of(signals: &[&Signal], eligible: usize) -> (f64, usize) {
    let mut by_entity: BTreeMap<&str, BTreeMap<&str, f64>> = BTreeMap::new();
    for s in signals {
        // one root cause counts once per entity (its strongest report)
        let w = s.severity * s.confidence;
        let e = by_entity.entry(s.entity.as_str()).or_default().entry(s.root_cause.as_str()).or_insert(0.0);
        *e = e.max(w);
    }
    let penalties: Vec<f64> = by_entity.values().map(|rc| 1.0 - rc.values().fold(1.0, |acc, w| acc * (1.0 - w.min(0.99)))).collect();
    let n = eligible.max(3) as f64;
    let density = penalties.iter().sum::<f64>() / n;
    let hotspot = penalties.iter().cloned().fold(0.0, f64::max);
    let load = density + HOTSPOT * hotspot;
    ((100.0 * (-BETA * load).exp()).clamp(0.0, 100.0), by_entity.len())
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
        // a god class is also a simplicity problem
        if i.kind == "god" {
            signals.extend(from_issue(i, d, "simplicity"));
        }
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
    // ---- per dimension
    let has_code = live.iter().any(|c| c.methods.iter().any(|m| m.code.is_some()));
    let code_coverage = if live.is_empty() { 0.0 } else { live.iter().filter(|c| c.methods.iter().any(|m| m.code.is_some()) || c.methods.is_empty()).count() as f64 / live.len() as f64 };
    let langs: Vec<String> = if ctx.languages.is_empty() { live.iter().filter_map(|c| profiles::language_of(c).map(str::to_string)).collect::<BTreeSet<_>>().into_iter().collect() } else { ctx.languages.to_vec() };
    let ran: BTreeSet<&str> = ctx.evidence.map(|e| e.runs.iter().filter(|r| r.ran()).map(|r| r.language.as_str()).collect()).unwrap_or_default();
    let tool_coverage = if langs.is_empty() { 0.0 } else { langs.iter().filter(|l| ran.contains(l.as_str())).count() as f64 / langs.len() as f64 };
    let documented_classes = live.iter().filter(|c| c.methods.iter().any(|meth| meth.status.is_live()
        && meth.visibility == crate::model::Visibility::Private && !meth.is_abstract
        && !crate::detail::is_constructor(&c.name, meth)
        && meth.code.as_ref().is_some_and(|f| f.source_documented.is_some()))).count();
    let documentation_coverage = if d.rules.private_documentation && !live.is_empty() { 0.15 * documented_classes as f64 / live.len() as f64 } else { 0.0 };
    let analysed_changes = m.changes.iter().filter(|c| c.analysed && !c.stale).count();
    let mut dims: BTreeMap<String, DimensionScore> = BTreeMap::new();
    let total_w: f64 = DIMENSIONS.iter().map(|k| d.health_priorities.get(*k).copied().unwrap_or(1.0).max(0.0)).sum::<f64>().max(1e-9);
    for dim in DIMENSIONS {
        let sigs: Vec<&Signal> = signals.iter().filter(|s| s.dimension == dim).collect();
        let mut ds = DimensionScore { weight: d.health_priorities.get(dim).copied().unwrap_or(1.0).max(0.0) / total_w * DIMENSIONS.len() as f64, ..Default::default() };
        let measurable = match dim {
            "implementation_quality" => tool_coverage > 0.0 || documentation_coverage > 0.0,
            _ => !live.is_empty(),
        };
        ds.coverage = match dim {
            "implementation_quality" => tool_coverage.max(documentation_coverage),
            "simplicity" => if has_code { code_coverage } else { 0.7 },
            "extensibility" => if d.change_scenarios.is_empty() { 0.7 } else if analysed_changes > 0 { 1.0 } else { 0.8 },
            _ => if live.is_empty() { 0.0 } else { 1.0 },
        };
        if dim == "implementation_quality" && !measurable {
            ds.gaps.push(if langs.is_empty() { "no code: implementation quality can't be verified from a design".into() } else { format!("no language server/linter has run for {} (run analyze; see doctor)", langs.join(", ")) });
        } else if dim == "implementation_quality" && tool_coverage < 1.0 {
            ds.gaps.push(format!("tools ran for {} of {} languages; source documentation alone does not verify correctness or safety", ran.len(), langs.len()));
        }
        if dim == "simplicity" && !has_code && !live.is_empty() {
            ds.gaps.push("design-only: function bodies are unknown".into());
        }
        if dim == "extensibility" && d.change_scenarios.is_empty() {
            ds.gaps.push("no expected changes recorded: change locality is unmeasured".into());
        }
        if measurable {
            let eligible = match dim {
                "implementation_quality" => live.len().max(ctx.evidence.map(|e| e.findings.iter().map(|f| f.file.as_str()).collect::<BTreeSet<_>>().len()).unwrap_or(0)),
                _ => live.len(),
            };
            let (score, entities) = score_of(&sigs, eligible);
            ds.measured = Some(score);
            ds.entities = entities;
        }
        ds.signals = sigs.len();
        if let Some(a) = d.health_assessments.get(dim).filter(|a| assessment_current(d, a)) {
            ds.agent = Some(a.score);
            ds.assisted = Some(match ds.measured {
                Some(mv) => {
                    let v = (1.0 - AGENT_WEIGHT) * mv + AGENT_WEIGHT * a.score;
                    // verified correctness errors can't be argued away
                    let blocked = dim == "implementation_quality" && signals.iter().any(|s| s.dimension == dim && s.severity >= 0.9);
                    if blocked { v.min(mv) } else { v }
                }
                None => {
                    ds.judgement_only = true;
                    a.score
                }
            });
        }
        dims.insert(dim.to_string(), ds);
    }
    // ---- aggregate
    let geo = |pick: &dyn Fn(&DimensionScore) -> Option<f64>| -> Option<f64> {
        let parts: Vec<(f64, f64)> = dims.values().filter_map(|x| pick(x).map(|v| (v.max(1.0), x.weight))).collect();
        let wsum: f64 = parts.iter().map(|p| p.1).sum();
        (wsum > 0.0).then(|| (parts.iter().map(|(v, w)| w * v.ln()).sum::<f64>() / wsum).exp())
    };
    let measured = geo(&|x| x.measured);
    let any_agent = dims.values().any(|x| x.agent.is_some());
    let assisted = any_agent.then(|| geo(&|x| x.best())).flatten();
    let known = dims.values().filter(|x| x.measured.is_some()).count();
    let coverage = dims.values().map(|x| x.coverage).sum::<f64>() / DIMENSIONS.len() as f64;
    let mut blocking: Vec<Signal> = signals.iter().filter(|s| s.dimension == "implementation_quality" && s.severity >= 0.9).cloned().collect();
    blocking.truncate(20);
    let mut ranked: Vec<Signal> = signals.clone();
    ranked.sort_by(|a, b| (b.severity * b.confidence).partial_cmp(&(a.severity * a.confidence)).unwrap_or(std::cmp::Ordering::Equal));
    let mut top: Vec<Signal> = blocking.clone();
    top.extend(ranked.into_iter().filter(|s| s.severity < 0.9 || s.dimension != "implementation_quality").take(15));
    let stale_assessments = d.health_assessments.values().filter(|a| !assessment_current(d, a)).map(|a| a.dimension.clone()).collect();
    let profiles_used: BTreeSet<String> = live.iter().map(|c| profiles::of_class(c).language.to_string()).collect();
    HealthReport {
        version: VERSION.into(),
        score: assisted.or(measured).map(|v| v.round() as u8),
        measured: measured.map(|v| v.round() as u8),
        assisted: assisted.map(|v| v.round() as u8),
        provisional: known < DIMENSIONS.len() || coverage < 0.8,
        coverage: (coverage * 100.0).round() as u8,
        dimensions: dims,
        top,
        signals,
        suppressed,
        blocking,
        languages: langs,
        profiles: profiles_used.into_iter().collect(),
        idioms,
        stale_assessments,
        legacy: m.health,
        explain: format!("{VERSION}: per dimension, entity penalties combine as independent risks (one root cause counts once); load = rate over eligible entities + {HOTSPOT}·worst hotspot; score = 100·e^(−{BETA}·load). Agent judgement contributes {:.0}% where current; unknown dimensions are excluded and make the score provisional.", AGENT_WEIGHT * 100.0),
    }
}

#[cfg(test)]
mod tests;
