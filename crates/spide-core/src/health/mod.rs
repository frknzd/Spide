//! Design and source health: evidence-backed scoring across six dimensions,
//! unified findings with provenance, brief-force alignment, tool-run status,
//! explicit evidence gaps, and owner-approved exceptions.

pub mod profiles;

use crate::evidence::{Category, EvidenceSet, Level, ToolRun};
use crate::metrics::{Issue, Metrics};
use crate::model::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const VERSION: &str = "health-2";

/// Dimension specification: id, display label, overall weight, saturation slope.
pub struct DimensionSpec {
    pub id: &'static str,
    pub label: &'static str,
    pub weight: f64,
    pub slope: f64,
}

pub const DIMENSIONS: &[DimensionSpec] = &[
    DimensionSpec { id: "modularity", label: "Modularity & Coupling", weight: 0.22, slope: 0.38 },
    DimensionSpec { id: "cohesion", label: "Cohesion & Responsibility", weight: 0.22, slope: 0.38 },
    DimensionSpec { id: "architecture", label: "Architecture & Rules", weight: 0.20, slope: 0.45 },
    DimensionSpec { id: "extensibility", label: "Extensibility & Change", weight: 0.14, slope: 0.38 },
    DimensionSpec { id: "simplicity", label: "Simplicity & Clarity", weight: 0.12, slope: 0.35 },
    DimensionSpec { id: "implementation_quality", label: "Implementation & Evidence", weight: 0.10, slope: 0.28 },
];

/// One finding feeding health: a rule, its evidence, explanation, and provenance.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct Signal {
    /// Rule id (`cycle`, `complex_function`, `evidence:correctness`, …) and its version.
    pub rule: String,
    pub rule_version: u32,
    pub dimension: String,
    /// Language/profile the rule was evaluated under (`design` without code).
    pub profile: String,
    /// Entity (diagram node) it is charged to, and the precise target (`Class.method`, file).
    pub entity: String,
    pub target: String,
    /// All targets involved in this finding (e.g. every class in a cycle or both duplicate methods).
    pub targets: Vec<String>,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub severity: f64,
    pub confidence: f64,
    /// structure | rule | heuristic | code | lsp | linter | editor | agent
    pub provenance: String,
    pub message: String,
    /// Why this finding matters.
    pub explain: String,
    pub fix_hint: Option<String>,
    /// Principle or tool citation (e.g. "Low Coupling (Ch. 2)", "clippy", "Architecture rules").
    pub source: String,
    /// Explicit architecture rule name when a project rule is broken.
    pub rule_label: Option<String>,
    /// Effective score weight of this root-cause group (transparent contribution).
    pub penalty: f64,
    /// Action priority tier: root_cause | simplify | refactor | polish.
    pub tier: String,
    /// Number of other active unique findings on the same entity (hotspot indicator).
    pub related_count: usize,
    /// Contextual programming pattern or simplification guidance (including when NOT to add indirection).
    pub pattern_hint: Option<String>,
    /// Same key = same root cause (deduplicated for scoring and display).
    pub root_cause: String,
    /// Suppressed by an approved exception (kept for inspection).
    pub suppressed_by: Option<String>,
}

/// A deterministic prompt to question a self-imposed constraint, preset, or abstraction.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct AssumptionChallenge {
    pub id: String,
    /// preset | rule | abstraction | brief | hotspot
    pub kind: String,
    pub title: String,
    pub question: String,
    pub evidence: String,
    pub targets: Vec<String>,
}

/// Per-dimension health breakdown.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct DimensionReport {
    pub id: String,
    pub label: String,
    /// 0..=100; None when this dimension has no basis to evaluate (e.g. empty design or no code/tool evidence).
    pub score: Option<u8>,
    /// good | fair | attention | unknown
    pub status: String,
    /// Deduplicated active root-cause findings in this dimension.
    pub active_findings: usize,
    /// Deduplicated suppressed root-cause findings in this dimension.
    pub suppressed_findings: usize,
    /// Total points deducted (0.0..=100.0).
    pub penalty: f64,
    /// Short description of what evidence backed this dimension.
    pub basis: String,
}

/// Alignment between a brief force (goal, quality, constraint, expected change) and current evidence.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct ForceHealth {
    pub force: String,
    pub kind: String,
    pub priority: u8,
    /// aligned | watch | at_risk | unmeasured
    pub status: String,
    pub summary: String,
    pub related_dimensions: Vec<String>,
    pub related_findings: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct HealthReport {
    pub version: String,
    pub scope: String,
    /// Overall health score 0..=100, weighted across assessed dimensions. None when empty/unassessed.
    pub score: Option<u8>,
    /// 0.0..=1.0 evidence coverage/confidence backing the score.
    pub confidence: f64,
    /// unassessed | structural | partial | verified
    pub evidence_level: String,
    pub dimensions: Vec<DimensionReport>,
    pub forces: Vec<ForceHealth>,
    /// Deterministic questions challenging potential over-engineering or artificial constraints.
    pub challenges: Vec<AssumptionChallenge>,
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

pub fn tier_rank(tier: &str) -> u8 {
    match tier {
        "root_cause" => 0,
        "simplify" => 1,
        "refactor" => 2,
        _ => 3,
    }
}

pub fn tier_label(tier: &str) -> &'static str {
    match tier {
        "root_cause" => "1 · Root Causes",
        "simplify" => "2 · Simplify",
        "refactor" => "3 · Local Refactors",
        _ => "4 · Hygiene & Polish",
    }
}

pub fn tier_of(rule: &str) -> &'static str {
    match rule {
        "cycle" | "layer" | "module_boundary" | "forbidden_dep" | "forbidden_type" | "god" | "cohesion" | "mixed_concerns" | "volatile_hub" | "scattered_concept" | "change_ripple" | "spof" | "multi_aggregate_write" | "evidence:correctness" | "evidence:safety" => "root_cause",
        "indirection_chain" | "over_fragmented_packages" | "ceremony_overhead" | "middle_man" | "speculative_abstraction" | "deep_inheritance" | "degenerate_visitor" | "orphan" => "simplify",
        "missing_types" | "inherited_repeated" | "private_documentation" | "consistency" | "evidence:style" | "evidence:unknown" => "polish",
        _ => "refactor",
    }
}

pub fn pattern_hint_for(rule: &str) -> Option<&'static str> {
    Some(match rule {
        "cycle" => "Pattern: Dependency Inversion (extract a minimal interface) or Merge tightly-coupled peers — avoid adding an interface if the two classes belong together.",
        "god" | "cohesion" | "mixed_concerns" => "Pattern: Extract Class along LCOM4 method groups / single responsibility — split only along real cohesion seams.",
        "volatile_hub" => "Pattern: Stable Abstractions / Facade — isolate unstable dependencies behind a small contract.",
        "layer" | "module_boundary" | "forbidden_dep" | "forbidden_type" => "Pattern: Port & Adapter / Dependency Inversion — or question whether this boundary rule fits the project's size.",
        "indirection_chain" | "middle_man" => "Decomplexify: Inline Class / Remove Middle Man — delete forwarding hops unless they form a real API, async, or trust boundary.",
        "speculative_abstraction" => "Decomplexify: Collapse Hierarchy / Inline Interface — wait for a second real implementation or an explicit change scenario.",
        "over_fragmented_packages" | "ceremony_overhead" => "Decomplexify: Co-locate by feature — merge single-class packages and delete empty wrappers before adding new structure.",
        "deep_inheritance" => "Pattern: Replace Inheritance with Composition — flatten hierarchies deeper than 2–3 levels.",
        "degenerate_visitor" => "Decomplexify: Replace Visitor with Strategy (or a plain method) when only one element type exists.",
        "type_switch" | "instanceof" => "Pattern: Polymorphism / Registry Map — skip if there are only 2–3 closed variants that rarely change.",
        "feature_envy" | "misplaced_responsibility" | "concept_elsewhere" | "data_class" => "Pattern: Move Method (GRASP Information Expert) — put behaviour with the data it uses.",
        "duplication" => "Pattern: Extract shared method into Information Expert or Pure Fabrication — avoid premature abstraction for coincidental 2-line overlap.",
        "demeter" => "Pattern: Tell, Don't Ask — expose an intention-revealing method on the direct collaborator instead of chaining getters.",
        "complex_function" | "deep_nesting" | "long_method" | "long_function" => "Pattern: Guard Clauses (early return) + Extract Method / Dispatch Table.",
        "long_params" | "many_params" => "Pattern: Introduce Parameter Object — group cohesive parameters into a value type.",
        "creator" => "Pattern: GRASP Creator / Factory Method — let the aggregate or container construct its own parts.",
        "ui_bypasses_controller" => "Pattern: GRASP Controller / Application Facade — mediate commands/lifecycle, not trivial read-only display values.",
        "change_ripple" | "scattered_concept" => "Pattern: Protected Variations — encapsulate the single thing that varies behind one module or interface.",
        _ => return None,
    })
}

impl HealthReport {
    /// Active signals deduplicated by `root_cause`, ordered by tackle leverage:
    /// (1) action tier (root causes → simplify → local refactors → polish),
    /// (2) entity hotspot count descending, (3) severity × confidence descending.
    pub fn unique_signals(&self) -> Vec<&Signal> {
        let mut by_rc: BTreeMap<&str, &Signal> = BTreeMap::new();
        for s in &self.signals {
            let key = if s.root_cause.is_empty() { s.message.as_str() } else { s.root_cause.as_str() };
            by_rc
                .entry(key)
                .and_modify(|cur| {
                    if (s.severity * s.confidence) > (cur.severity * cur.confidence) {
                        *cur = s;
                    }
                })
                .or_insert(s);
        }
        let mut out: Vec<&Signal> = by_rc.into_values().collect();
        out.sort_by(|a, b| {
            tier_rank(&a.tier)
                .cmp(&tier_rank(&b.tier))
                .then_with(|| b.related_count.cmp(&a.related_count))
                .then_with(|| (b.severity * b.confidence).total_cmp(&(a.severity * a.confidence)))
                .then_with(|| a.dimension.cmp(&b.dimension))
                .then_with(|| a.rule.cmp(&b.rule))
                .then_with(|| a.target.cmp(&b.target))
        });
        out
    }

    /// Suppressed signals deduplicated by `root_cause`.
    pub fn unique_suppressed(&self) -> Vec<&Signal> {
        let mut by_rc: BTreeMap<&str, &Signal> = BTreeMap::new();
        for s in &self.suppressed {
            let key = if s.root_cause.is_empty() { s.message.as_str() } else { s.root_cause.as_str() };
            by_rc.entry(key).or_insert(s);
        }
        by_rc.into_values().collect()
    }

    /// Score of a specific dimension by id.
    pub fn dimension_score(&self, id: &str) -> Option<u8> {
        self.dimensions.iter().find(|d| d.id == id).and_then(|d| d.score)
    }
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

fn next_exception_id(d: &Design) -> String {
    let mut id = format!("HX{}", d.health_exceptions.len() + 1);
    while d.health_exceptions.iter().any(|e| e.id == id) {
        id.push('x');
    }
    id
}

/// Add an agent-proposed exception (never approved by the proposer).
pub fn propose_exception(d: &mut Design, mut x: HealthException, proposed_by: &str) -> Result<String, String> {
    if x.rule.trim().is_empty() || x.scope.trim().is_empty() {
        return Err("rule and scope are required".into());
    }
    if x.reason.trim().len() < 15 {
        return Err("give the reason the rule doesn't apply here".into());
    }
    x.id = next_exception_id(d);
    x.approved = false;
    x.approved_at = None;
    x.proposed_by = proposed_by.to_string();
    let id = x.id.clone();
    d.health_exceptions.push(x);
    Ok(id)
}

/// Owner-created exception (approved immediately).
pub fn add_exception(d: &mut Design, mut x: HealthException, now: u64) -> Result<String, String> {
    if x.rule.trim().is_empty() || x.scope.trim().is_empty() {
        return Err("rule and scope are required".into());
    }
    if x.reason.trim().len() < 5 {
        return Err("give a short reason why this rule does not apply here".into());
    }
    x.id = next_exception_id(d);
    x.approved = true;
    x.approved_at = Some(now);
    x.proposed_by = "user".into();
    let id = x.id.clone();
    d.health_exceptions.push(x);
    Ok(id)
}

/// Owner decision on an exception (approve pending, or reject/revoke any).
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

pub fn dimension_of(kind: &str) -> &'static str {
    match kind {
        "cycle" | "coupling" | "demeter" | "sync_chain" | "spof" | "volatile_hub" | "multi_aggregate_write" | "orphan" => "modularity",
        "god" | "cohesion" | "mixed_concerns" | "vague_role" | "misplaced_responsibility" | "concept_elsewhere" | "feature_envy" | "fat_controller" | "creator" | "data_class" | "duplication" | "role_inconsistency" | "disagreement" | "exposed_state" => "cohesion",
        "layer" | "module_boundary" | "forbidden_dep" | "forbidden_type" | "ui_bypasses_controller" | "consistency" => "architecture",
        "instanceof" | "type_switch" | "change_ripple" | "scattered_concept" | "narrowed_visibility" | "inheritance_coupling" | "composite_incomplete" | "degenerate_visitor" | "deep_inheritance" => "extensibility",
        "large_class" | "long_method" | "middle_man" | "long_params" | "chatty_scenario" | "missing_types" | "inherited_repeated" | "complex_function" | "deep_nesting" | "long_function" | "many_params" | "speculative_abstraction" | "indirection_chain" | "over_fragmented_packages" | "ceremony_overhead" => "simplicity",
        "private_documentation" => "implementation_quality",
        k if k.starts_with("evidence:") => "implementation_quality",
        _ => "modularity",
    }
}

pub fn dimension_label(id: &str) -> &'static str {
    DIMENSIONS.iter().find(|d| d.id == id).map(|d| d.label).unwrap_or("Modularity & Coupling")
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
    let profile = targets
        .first()
        .and_then(|t| d.class(t.split('.').next().unwrap_or(t)))
        .map(|c| profiles::of_class(c).language)
        .unwrap_or("design");
    let provenance = if i.rule.is_some() {
        "rule".to_string()
    } else if i.basis.is_empty() {
        "structure".to_string()
    } else {
        i.basis.clone()
    };
    let root_cause = format!("{}:{}", i.kind, targets.join(","));
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
            targets: targets.clone(),
            severity: i.severity.clamp(0.0, 1.0),
            confidence: i.confidence.clamp(0.0, 1.0),
            provenance: provenance.clone(),
            message: i.message.clone(),
            explain: i.explain.clone(),
            fix_hint: i.fix_hint.clone(),
            source: i.source.to_string(),
            rule_label: i.rule.clone(),
            root_cause: root_cause.clone(),
            ..Default::default()
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn sig(
    rule: &str,
    entity: &str,
    target: &str,
    profile: &str,
    severity: f64,
    confidence: f64,
    provenance: &str,
    message: String,
    explain: &str,
    fix: &str,
    source: &str,
) -> Signal {
    Signal {
        rule: rule.into(),
        rule_version: 1,
        dimension: dimension_of(rule).into(),
        profile: profile.into(),
        entity: entity.into(),
        target: target.into(),
        targets: vec![target.into()],
        severity: severity.clamp(0.0, 1.0),
        confidence: confidence.clamp(0.0, 1.0),
        provenance: provenance.into(),
        message,
        explain: explain.into(),
        fix_hint: (!fix.is_empty()).then(|| fix.to_string()),
        source: source.into(),
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
                let mut s = sig(
                    "instanceof",
                    &c.name,
                    &target,
                    p.language,
                    0.18 + 0.04 * f.instanceofs.saturating_sub(1).min(4) as f64,
                    0.9,
                    "code",
                    format!("{target}() uses {} instanceof check(s): type-specific dispatch can require edits for every new node type", f.instanceofs),
                    "Type checks scatter behaviour that varies by subtype across callers; adding a new subtype means finding and updating every instanceof branch.",
                    "review whether polymorphism or a parser registry/class map can isolate new types; instanceof is a smell, not a correctness error",
                    "Source health check",
                );
                s.file = Some(loc.clone());
                s.line = Some(f.start_line);
                out.push(s);
            }
            if d.rules.private_documentation
                && meth.visibility == crate::model::Visibility::Private
                && !meth.is_abstract
                && !crate::detail::is_constructor(&c.name, meth)
                && f.source_documented == Some(false)
            {
                let mut s = sig(
                    "private_documentation",
                    &c.name,
                    &target,
                    p.language,
                    0.08,
                    1.0,
                    "code",
                    format!("{target}() has no attached explanatory source comment"),
                    "Project rules check that private helpers carry an explanatory comment or Javadoc describing their intent.",
                    "document the helper's purpose with an explanatory comment or Javadoc; this optional project check measures presence, not comment quality",
                    "Source health check",
                );
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
                push(sig(
                    "complex_function",
                    &c.name,
                    &target,
                    p.language,
                    0.25 + 0.05 * (cc - p.complexity) as f64,
                    1.0,
                    "code",
                    format!("{target}() has cyclomatic complexity {cc} (> {} for {})", p.complexity, p.language),
                    "High branching complexity makes control flow hard to reason about and increases the number of test paths required.",
                    "split the decision logic into smaller functions, or replace conditionals with polymorphism/dispatch tables",
                    "Language profile",
                ));
            }
            if f.max_nesting > p.nesting {
                push(sig(
                    "deep_nesting",
                    &c.name,
                    &target,
                    p.language,
                    0.2 + 0.1 * (f.max_nesting - p.nesting) as f64,
                    1.0,
                    "code",
                    format!("{target}() nests {} levels deep (> {})", f.max_nesting, p.nesting),
                    "Deeply nested blocks force readers to hold many active conditions in working memory.",
                    "return early, extract the inner blocks",
                    "Language profile",
                ));
            }
            let legacy = p.language == "java" || p.language == "design";
            if !legacy && f.loc > p.function_loc {
                push(sig(
                    "long_function",
                    &c.name,
                    &target,
                    p.language,
                    0.15 + 0.2 * ((f.loc - p.function_loc) as f64 / p.function_loc as f64).min(2.0),
                    1.0,
                    "code",
                    format!("{target}() has {} lines (> {} for {})", f.loc, p.function_loc, p.language),
                    "Long functions tend to mix multiple steps or abstraction levels.",
                    "extract named steps",
                    "Language profile",
                ));
            }
            if !legacy && f.params > p.params {
                push(sig(
                    "many_params",
                    &c.name,
                    &target,
                    p.language,
                    0.2 + 0.05 * (f.params - p.params) as f64,
                    1.0,
                    "code",
                    format!("{target}() takes {} parameters (> {})", f.params, p.params),
                    "Long parameter lists make call sites fragile and hard to read.",
                    "group related parameters into a value/options type",
                    "Language profile",
                ));
            }
        }
        // a module of functions has its own size limit
        if c.is_module() && c.methods.len() > p.module_functions {
            out.push(sig(
                "large_class",
                &c.name,
                &c.name,
                p.language,
                0.3 + 0.01 * (c.methods.len() - p.module_functions) as f64,
                1.0,
                "structure",
                format!("module {} has {} functions (> {} for {})", c.name, c.methods.len(), p.module_functions, p.language),
                "Large modules accumulate unrelated functions and become change magnets.",
                "split the module by the data/feature its functions serve",
                "Language profile",
            ));
        }
    }
    // extensibility: deep inheritance and volatile hubs; simplicity: speculative abstractions
    let parents: BTreeMap<&str, Vec<&str>> = d
        .relations
        .iter()
        .filter(|r| r.status.is_live() && r.kind == RelationKind::Inheritance)
        .fold(BTreeMap::new(), |mut acc, r| {
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
    let realizers: BTreeMap<&str, usize> = d
        .relations
        .iter()
        .filter(|r| r.status.is_live() && matches!(r.kind, RelationKind::Realization | RelationKind::Inheritance))
        .fold(BTreeMap::new(), |mut acc, r| {
            *acc.entry(r.to.as_str()).or_default() += 1;
            acc
        });
    let mentioned: BTreeSet<&str> = d.change_scenarios.iter().flat_map(|s| s.impacted.iter().map(|i| i.class.as_str())).collect();
    for c in live {
        let p = profiles::of_class(c).language;
        let dpt = depth(&c.name);
        if dpt > 3 {
            out.push(sig(
                "deep_inheritance",
                &c.name,
                &c.name,
                p,
                0.2 + 0.1 * (dpt - 3) as f64,
                1.0,
                "structure",
                format!("{} sits {dpt} levels deep in an inheritance hierarchy", c.name),
                "Deep inheritance chains couple subclasses tightly to ancestor implementation details.",
                "prefer composition; flatten the hierarchy",
                "Structural analysis",
            ));
        }
        // a contract with behaviour (an interface that only describes a data shape isn't one)
        if c.is_interface() && !c.methods.is_empty() && live.len() >= 4 && realizers.get(c.name.as_str()).copied().unwrap_or(0) <= 1 && !mentioned.contains(c.name.as_str()) {
            let fan_in = m.classes.get(&c.name).map(|x| x.fan_in).unwrap_or(0);
            if fan_in <= 2 {
                out.push(sig(
                    "speculative_abstraction",
                    &c.name,
                    &c.name,
                    p,
                    0.2,
                    0.6,
                    "heuristic",
                    format!("{} is an abstraction with {} implementation(s), few clients and no expected change that needs it", c.name, realizers.get(c.name.as_str()).copied().unwrap_or(0)),
                    "Unused abstractions add indirection without isolating an actual variation point.",
                    "inline it until a second variant or an expected change needs the seam (or record that change scenario)",
                    "Simplicity heuristic",
                ));
            }
        }
        if let Some(cm) = m.classes.get(&c.name) {
            if cm.fan_in >= 4 && cm.instability > 0.6 && !c.is_interface() && !c.is_module() {
                out.push(sig(
                    "volatile_hub",
                    &c.name,
                    &c.name,
                    p,
                    0.3,
                    0.8,
                    "structure",
                    format!("{} is depended on by {} classes but itself depends on much (instability {:.2})", c.name, cm.fan_in, cm.instability),
                    "Stable Dependencies Principle: components with many dependents should be stable so upstream changes do not cascade.",
                    "make what many depend on stable: depend on an abstraction, or move its dependencies out",
                    "Stable Dependencies",
                ));
            }
        }
    }
    out
}

/// Evidence findings → implementation-quality signals charged to the class whose code range
/// contains them (else the file).
fn evidence_signals(d: &Design, ev: &EvidenceSet) -> Vec<Signal> {
    // (file, start, end, class, Option<method>)
    let mut ranges: Vec<(&str, u32, u32, &str, Option<&str>)> = vec![];
    for c in d.classes.iter().filter(|c| c.status.is_live()) {
        if let Some(s) = &c.source {
            ranges.push((s.file.as_str(), s.start_line, s.end_line.max(s.start_line), c.name.as_str(), None));
        }
        for m in &c.methods {
            if let Some(s) = &m.source {
                ranges.push((s.file.as_str(), s.start_line, s.end_line.max(s.start_line), c.name.as_str(), Some(m.name.as_str())));
            }
        }
    }
    ev.findings
        .iter()
        .filter(|f| f.provider != "editor-unsaved")
        .map(|f| {
            let best = ranges
                .iter()
                .filter(|(file, a, b, _, _)| *file == f.file && *a <= f.range.line && f.range.line <= *b)
                .min_by_key(|(_, a, b, _, meth)| (b - a, meth.is_none()));
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
            let entity = best.map(|x| x.3.to_string()).unwrap_or_else(|| f.file.clone());
            let target = match best {
                Some((_, _, _, cls, Some(meth))) => format!("{cls}.{meth}"),
                _ => format!("{}:{}", f.file, f.range.line),
            };
            Signal {
                rule: format!("evidence:{}", f.category.as_str()),
                rule_version: 1,
                dimension: "implementation_quality".into(),
                profile: f.language.clone(),
                entity: entity.clone(),
                target: target.clone(),
                targets: vec![target],
                file: Some(f.file.clone()),
                line: Some(f.range.line),
                severity: w,
                confidence: if f.provider == "linter" || f.provider == "lsp" || f.provider == "editor" { 1.0 } else { 0.8 },
                provenance: f.provider.clone(),
                message: format!("{} [{}{}]: {}", f.tool, f.code, if f.level == Level::Error { ", error" } else { "" }, f.message.lines().next().unwrap_or("")),
                explain: format!("Reported by {} ({}) in {}:{}.", f.tool, f.category.as_str(), f.file, f.range.line),
                fix_hint: None,
                source: format!("{} ({})", f.tool, f.provider),
                rule_label: None,
                penalty: 0.0,
                root_cause: format!("{}:{}:{}", f.file, f.range.line, f.code),
                suppressed_by: None,
                ..Default::default()
            }
        })
        .collect()
}

fn provenance_weight(p: &str) -> f64 {
    match p {
        "rule" | "structure" | "linter" | "lsp" | "editor" => 1.0,
        "code" => 0.9,
        "heuristic + agent" => 0.9,
        "agent" => 0.8,
        "heuristic" => 0.72,
        _ if p.contains("disagrees") => 0.45,
        _ => 0.75,
    }
}

fn status_for_score(score: Option<u8>) -> &'static str {
    match score {
        None => "unknown",
        Some(s) if s >= 80 => "good",
        Some(s) if s >= 60 => "fair",
        Some(_) => "attention",
    }
}

fn force_health(d: &Design, m: &Metrics, dims: &[DimensionReport]) -> Vec<ForceHealth> {
    let dim_of = |id: &str| dims.iter().find(|x| x.id == id);
    let mut out = Vec::new();
    for f in d.forces.iter().filter(|f| f.status.is_live()) {
        let lower = format!("{} {}", f.name, f.description.as_deref().unwrap_or("")).to_lowercase();
        match f.kind {
            ForceKind::Change => {
                if let Some(cs) = m.changes.iter().find(|c| c.name.eq_ignore_ascii_case(&f.name)) {
                    if cs.analysed {
                        let pct = (cs.locality * 100.0).round() as i64;
                        let status = if pct >= 75 { "aligned" } else if pct >= 50 { "watch" } else { "at_risk" };
                        let stale_note = if cs.stale { " · stale" } else { "" };
                        out.push(ForceHealth {
                            force: f.name.clone(),
                            kind: f.kind.as_str().into(),
                            priority: f.priority,
                            status: status.into(),
                            summary: format!("{pct}% locality ({} impacted, {} ripple{stale_note})", cs.impacted.len(), cs.ripple.len()),
                            related_dimensions: vec!["extensibility".into()],
                            related_findings: usize::from(pct < 50),
                        });
                    } else {
                        out.push(ForceHealth {
                            force: f.name.clone(),
                            kind: f.kind.as_str().into(),
                            priority: f.priority,
                            status: "unmeasured".into(),
                            summary: "Change scenario added but not analysed yet.".into(),
                            related_dimensions: vec!["extensibility".into()],
                            related_findings: 0,
                        });
                    }
                } else {
                    out.push(ForceHealth {
                        force: f.name.clone(),
                        kind: f.kind.as_str().into(),
                        priority: f.priority,
                        status: "unmeasured".into(),
                        summary: "No change scenario linked yet; analyse it in Brief to measure locality.".into(),
                        related_dimensions: vec!["extensibility".into()],
                        related_findings: 0,
                    });
                }
            }
            ForceKind::Quality => {
                let mut rel: Vec<&'static str> = vec![];
                if ["coupl", "modul", "depend", "cycle", "isolat", "decoupl"].iter().any(|k| lower.contains(k)) {
                    rel.push("modularity");
                }
                if ["cohes", "respons", "single", "domain", "encapsul"].iter().any(|k| lower.contains(k)) {
                    rel.push("cohesion");
                }
                if ["layer", "boundar", "clean", "hexag", "arch", "port", "adapter"].iter().any(|k| lower.contains(k)) {
                    rel.push("architecture");
                }
                if ["extens", "evolv", "chang", "flexib", "plugin", "open"].iter().any(|k| lower.contains(k)) {
                    rel.push("extensibility");
                }
                if ["simpl", "readab", "complex", "clarit", "maintain", "concis"].iter().any(|k| lower.contains(k)) {
                    rel.push("simplicity");
                }
                if ["qualit", "bug", "safe", "secur", "perf", "test", "doc", "lint", "correct"].iter().any(|k| lower.contains(k)) {
                    rel.push("implementation_quality");
                }
                if rel.is_empty() {
                    rel.extend(["modularity", "cohesion", "architecture"]);
                }
                let scores: Vec<u8> = rel.iter().filter_map(|id| dim_of(id).and_then(|x| x.score)).collect();
                let findings: usize = rel.iter().filter_map(|id| dim_of(id).map(|x| x.active_findings)).sum();
                let (status, summary) = if let Some(&min_s) = scores.iter().min() {
                    let st = if min_s >= 80 { "aligned" } else if min_s >= 60 { "watch" } else { "at_risk" };
                    (st, format!("Lowest linked dimension: {min_s}/100 ({findings} finding{})", if findings == 1 { "" } else { "s" }))
                } else {
                    ("unmeasured", "No evidence evaluated for linked dimensions yet.".into())
                };
                out.push(ForceHealth {
                    force: f.name.clone(),
                    kind: f.kind.as_str().into(),
                    priority: f.priority,
                    status: status.into(),
                    summary,
                    related_dimensions: rel.into_iter().map(String::from).collect(),
                    related_findings: findings,
                });
            }
            ForceKind::Constraint => {
                let arch = dim_of("architecture");
                let findings = arch.map(|x| x.active_findings).unwrap_or(0);
                let (status, summary) = match arch.and_then(|x| x.score) {
                    Some(s) if s >= 80 && findings == 0 => ("aligned", "No architecture rule breaks observed.".into()),
                    Some(s) if s >= 60 => ("watch", format!("Architecture at {s}/100 ({findings} finding{})", if findings == 1 { "" } else { "s" })),
                    Some(s) => ("at_risk", format!("Architecture at {s}/100 ({findings} finding{})", if findings == 1 { "" } else { "s" })),
                    None => ("unmeasured", "Add classes or rules to verify constraints.".into()),
                };
                out.push(ForceHealth {
                    force: f.name.clone(),
                    kind: f.kind.as_str().into(),
                    priority: f.priority,
                    status: status.into(),
                    summary,
                    related_dimensions: vec!["architecture".into()],
                    related_findings: findings,
                });
            }
            ForceKind::Goal => {
                let rel = ["cohesion", "architecture"];
                let scores: Vec<u8> = rel.iter().filter_map(|id| dim_of(id).and_then(|x| x.score)).collect();
                let findings: usize = rel.iter().filter_map(|id| dim_of(id).map(|x| x.active_findings)).sum();
                let (status, summary) = if let Some(&min_s) = scores.iter().min() {
                    let st = if min_s >= 80 { "aligned" } else if min_s >= 60 { "watch" } else { "at_risk" };
                    (st, format!("Core structure at {min_s}/100 ({findings} finding{})", if findings == 1 { "" } else { "s" }))
                } else {
                    ("unmeasured", "No classes modelled yet.".into())
                };
                out.push(ForceHealth {
                    force: f.name.clone(),
                    kind: f.kind.as_str().into(),
                    priority: f.priority,
                    status: status.into(),
                    summary,
                    related_dimensions: rel.into_iter().map(String::from).collect(),
                    related_findings: findings,
                });
            }
        }
    }
    out
}

/// Decomplexification detectors: pass-through indirection chains, package fragmentation,
/// and high ceremony-to-behaviour ratios.
fn decomplexify_signals(d: &Design, m: &Metrics, live: &[&Class], existing: &[Signal]) -> Vec<Signal> {
    let mut out = vec![];
    if live.len() < 3 {
        return out;
    }

    // 1. Pass-through indirection chains (A → B → C where B is a middle man / stateless forwarder)
    let middle: BTreeSet<&str> = m.overlap.middle_men.iter().map(String::as_str).collect();
    let thin_forwarder = |c: &Class| -> bool {
        if middle.contains(c.name.as_str()) {
            return true;
        }
        let methods: Vec<&Method> = c.methods.iter().filter(|x| x.status.is_live() && !x.is_abstract).collect();
        let attrs = c.attributes.iter().filter(|a| a.status.is_live()).count();
        !c.is_interface()
            && !c.is_enum()
            && !methods.is_empty()
            && methods.len() <= 3
            && attrs <= 1
            && methods.iter().all(|meth| meth.uses.is_empty() && meth.calls.iter().any(|call| call.contains('.')))
    };
    let forwarders: BTreeSet<&str> = live.iter().filter(|c| thin_forwarder(c)).map(|c| c.name.as_str()).collect();
    if forwarders.len() >= 2 {
        // Look for edges F1 → F2 between two forwarding classes
        let mut adj: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for r in d.relations.iter().filter(|r| r.status.is_live()) {
            if forwarders.contains(r.from.as_str()) && forwarders.contains(r.to.as_str()) && r.from != r.to {
                adj.entry(r.from.as_str()).or_default().push(r.to.as_str());
            }
        }
        for e in &m.implicit_edges {
            if forwarders.contains(e.from.as_str()) && forwarders.contains(e.to.as_str()) && e.from != e.to {
                adj.entry(e.from.as_str()).or_default().push(e.to.as_str());
            }
        }
        let mut reported: BTreeSet<(String, String)> = BTreeSet::new();
        for (a, tos) in &adj {
            for b in tos {
                if reported.insert(((*a).to_string(), (*b).to_string())) {
                    let targets = vec![(*a).to_string(), (*b).to_string()];
                    let mut s = sig(
                        "indirection_chain",
                        a,
                        a,
                        "design",
                        0.35,
                        0.75,
                        "heuristic",
                        format!("Pass-through chain: {a} → {b} forwards calls through consecutive thin layers"),
                        "Chained pass-through layers multiply boilerplate and obscure control flow without adding domain logic or real isolation.",
                        "collapse one of the pass-through layers unless it isolates a real deployment, protocol, or trust boundary",
                        "Simplicity analysis",
                    );
                    s.targets = targets;
                    s.root_cause = format!("indirection_chain:{a},{b}");
                    out.push(s);
                }
            }
        }
    }

    // 2. Over-fragmented packages (many 1-class packages)
    let mut pkg_counts: BTreeMap<&str, usize> = BTreeMap::new();
    for c in live {
        if let Some(p) = c.package.as_deref() {
            *pkg_counts.entry(p).or_default() += 1;
        }
    }
    let non_empty = pkg_counts.len();
    if non_empty >= 4 && live.len() >= 4 {
        let singletons: Vec<&str> = pkg_counts.iter().filter(|(_, n)| **n == 1).map(|(p, _)| *p).collect();
        if singletons.len() * 10 >= non_empty * 6 {
            let names = singletons.iter().take(5).copied().collect::<Vec<_>>().join(", ");
            let mut s = sig(
                "over_fragmented_packages",
                "(design)",
                singletons[0],
                "design",
                0.22,
                0.7,
                "structure",
                format!("Package fragmentation: {} of {non_empty} packages hold only 1 class ({names})", singletons.len()),
                "Too many tiny packages create artificial import and boundary ceremony without meaningful modular cohesion.",
                "merge single-class packages by feature or cohesion boundary until each package owns a cohesive group of types",
                "Simplicity analysis",
            );
            s.targets = singletons.iter().map(|p| (*p).to_string()).collect();
            s.root_cause = "over_fragmented_packages:design".into();
            out.push(s);
        }
    }

    // 3. High ceremony-to-behaviour ratio
    if live.len() >= 6 {
        let spec_interfaces: BTreeSet<&str> = existing
            .iter()
            .filter(|s| s.rule == "speculative_abstraction")
            .map(|s| s.entity.as_str())
            .collect();
        let ceremony: Vec<&str> = live
            .iter()
            .filter(|c| {
                spec_interfaces.contains(c.name.as_str())
                    || forwarders.contains(c.name.as_str())
                    || (!c.is_enum() && c.methods.iter().filter(|m| m.status.is_live()).count() == 0 && c.attributes.iter().filter(|a| a.status.is_live()).count() == 0)
            })
            .map(|c| c.name.as_str())
            .collect();
        if ceremony.len() * 100 > live.len() * 55 {
            let sample = ceremony.iter().take(5).copied().collect::<Vec<_>>().join(", ");
            let mut s = sig(
                "ceremony_overhead",
                "(design)",
                "(design)",
                "design",
                0.28,
                0.7,
                "heuristic",
                format!("High ceremony ratio: {} of {} classes are single-impl interfaces, pass-throughs, or empty shells ({sample})", ceremony.len(), live.len()),
                "When most types in a design are indirection or boilerplate rather than domain behaviour, every feature change touches too many files.",
                "inline speculative interfaces and remove thin wrapper classes until concrete variation justifies them",
                "Simplicity analysis",
            );
            s.targets = ceremony.iter().map(|c| (*c).to_string()).collect();
            s.root_cause = "ceremony_overhead:design".into();
            out.push(s);
        }
    }

    out
}

/// Deterministic assumption & constraint challenges: surface potential self-imposed complexity.
fn assumption_challenges(d: &Design, live: &[&Class], unique: &[&Signal]) -> Vec<AssumptionChallenge> {
    let mut out = vec![];
    if live.is_empty() {
        return out;
    }

    // 1. Heavy architecture preset on a small design
    let heavy_preset = matches!(d.rules.preset, crate::rules::Preset::Hexagonal | crate::rules::Preset::ModularMonolith);
    let spec_or_mid = unique
        .iter()
        .filter(|s| matches!(s.rule.as_str(), "speculative_abstraction" | "middle_man" | "indirection_chain"))
        .count();
    if heavy_preset && live.len() <= 6 {
        out.push(AssumptionChallenge {
            id: "CH-preset".into(),
            kind: "preset".into(),
            title: format!("Heavy style ({}) for {} classes", d.rules.preset.as_str(), live.len()),
            question: format!(
                "Is {} architecture paying for itself at {} classes, or is it forcing ports/adapters before you have multiple implementations?",
                d.rules.preset.as_str(),
                live.len()
            ),
            evidence: format!(
                "Active preset is {} with {} internal class(es) and {} speculative/forwarding finding(s).",
                d.rules.preset.as_str(),
                live.len(),
                spec_or_mid
            ),
            targets: vec![],
        });
    }

    // 2. Unused custom forbidden dependency rules
    for (idx, r) in d.rules.forbidden.iter().enumerate() {
        let from_match = live.iter().any(|c| crate::rules::matches_class(&r.from, &c.name, c.package.as_deref()));
        let to_match = live.iter().any(|c| crate::rules::matches_class(&r.to, &c.name, c.package.as_deref()));
        if !from_match || !to_match {
            out.push(AssumptionChallenge {
                id: format!("CH-rule-{}", idx + 1),
                kind: "rule".into(),
                title: format!("Unmatched rule: {}", r.id()),
                question: format!("Rule “{}” matches no current classes or packages — is it still a real constraint or leftover ceremony?", r.id()),
                evidence: format!("from={} (matched: {from_match}), to={} (matched: {to_match}).", r.from, r.to),
                targets: vec![],
            });
        }
    }

    // 3. Unjustified abstractions with no change scenarios
    if spec_or_mid >= 2 && d.change_scenarios.is_empty() {
        let targets: Vec<String> = unique
            .iter()
            .filter(|s| matches!(s.rule.as_str(), "speculative_abstraction" | "middle_man" | "indirection_chain"))
            .map(|s| s.entity.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        out.push(AssumptionChallenge {
            id: "CH-abstractions".into(),
            kind: "abstraction".into(),
            title: format!("{spec_or_mid} abstractions/wrappers without expected changes"),
            question: "What concrete future change are these interfaces and pass-throughs protecting against? If none, can you inline them?".into(),
            evidence: format!("Found {spec_or_mid} speculative abstraction/forwarding findings ({}) with 0 recorded change scenarios.", targets.join(", ")),
            targets,
        });
    }

    // 4. Brief priority inflation ("everything is MUST")
    let musts: Vec<&str> = d
        .forces
        .iter()
        .filter(|f| f.status.is_live() && f.priority == 1)
        .map(|f| f.name.as_str())
        .collect();
    if musts.len() >= 5 {
        out.push(AssumptionChallenge {
            id: "CH-priorities".into(),
            kind: "brief".into(),
            title: format!("{} forces marked MUST", musts.len()),
            question: "When almost every requirement is marked MUST, which one would you relax first to keep the architecture simple?".into(),
            evidence: format!("{} brief forces share top priority: {}.", musts.len(), musts.iter().take(4).copied().collect::<Vec<_>>().join(", ")),
            targets: vec![],
        });
    }

    // 5. Entity complexity hotspots (≥3 unique active findings on one class)
    let mut per_entity: BTreeMap<&str, usize> = BTreeMap::new();
    for s in unique {
        if !s.entity.is_empty() && s.entity != "(design)" && d.class(&s.entity).is_some() {
            *per_entity.entry(s.entity.as_str()).or_default() += 1;
        }
    }
    let mut hotspots: Vec<(&str, usize)> = per_entity.into_iter().filter(|(_, n)| *n >= 3).collect();
    hotspots.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    for (ent, n) in hotspots.into_iter().take(2) {
        out.push(AssumptionChallenge {
            id: format!("CH-hotspot-{ent}"),
            kind: "hotspot".into(),
            title: format!("Complexity hotspot: {ent} ({n} findings)"),
            question: format!("{ent} concentrates {n} separate findings — is it trying to solve two different problems at once?"),
            evidence: format!("Splitting or simplifying {ent} has the highest leverage on the overall health score."),
            targets: vec![ent.to_string()],
        });
    }

    out
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
    let decomp = decomplexify_signals(d, m, &live, &signals);
    signals.extend(decomp);
    if let Some(ev) = ctx.evidence {
        signals.extend(evidence_signals(d, ev));
    }
    // populate action tier & contextual pattern guidance on every signal
    for s in &mut signals {
        s.tier = tier_of(&s.rule).into();
        if s.pattern_hint.is_none() {
            s.pattern_hint = pattern_hint_for(&s.rule).map(String::from);
        }
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

    let languages: Vec<_> = ctx
        .languages
        .iter()
        .cloned()
        .chain(live.iter().filter_map(|c| profiles::language_of(c).map(str::to_string)))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let mut runs = ctx.evidence.map(|e| e.runs.clone()).unwrap_or_default();
    runs.sort_by(|a, b| (&a.language, &a.tool).cmp(&(&b.language, &b.tool)));

    let has_code_facts = live.iter().any(|c| c.methods.iter().any(|m| m.code.is_some()));
    let has_tool_runs = runs.iter().any(|r| r.ran());
    let has_ev_signals = signals.iter().chain(suppressed.iter()).any(|s| s.dimension == "implementation_quality");

    // ---- gaps
    let mut gaps = Vec::new();
    if live.is_empty() {
        gaps.push("No internal design or indexed code units to inspect.".into());
    } else if !has_code_facts {
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

    // ---- per-dimension scoring (root-cause deduplicated, saturating, anti-padding)
    let mut dimensions: Vec<DimensionReport> = Vec::with_capacity(DIMENSIONS.len());
    let mut rc_penalties: BTreeMap<String, f64> = BTreeMap::new();

    for spec in DIMENSIONS {
        let mut active_rc: BTreeMap<&str, f64> = BTreeMap::new();
        for s in signals.iter().filter(|s| s.dimension == spec.id) {
            let key = if s.root_cause.is_empty() { s.message.as_str() } else { s.root_cause.as_str() };
            let w = (s.severity * s.confidence * provenance_weight(&s.provenance)).clamp(0.0, 1.0);
            active_rc.entry(key).and_modify(|cur| *cur = cur.max(w)).or_insert(w);
        }
        let suppressed_rc: BTreeSet<&str> = suppressed
            .iter()
            .filter(|s| s.dimension == spec.id)
            .map(|s| if s.root_cause.is_empty() { s.message.as_str() } else { s.root_cause.as_str() })
            .collect();

        let assessed = if live.is_empty() {
            false
        } else if spec.id == "implementation_quality" {
            has_code_facts || has_tool_runs || has_ev_signals
        } else {
            true
        };

        let total_w: f64 = active_rc.values().sum();
        let penalty = if assessed {
            (100.0 * (1.0 - (-spec.slope * total_w).exp())).clamp(0.0, 100.0)
        } else {
            0.0
        };
        if total_w > 0.0 {
            for (rc, w) in &active_rc {
                let standalone = 100.0 * (1.0 - (-spec.slope * *w).exp());
                rc_penalties.insert((*rc).to_string(), (standalone * 10.0).round() / 10.0);
            }
        }
        let score = assessed.then(|| (100.0 - penalty).round().clamp(0.0, 100.0) as u8);
        let basis = match spec.id {
            _ if !assessed => if live.is_empty() { "no classes yet" } else { "no code or tool evidence" },
            "architecture" => "architecture rules & boundaries",
            "implementation_quality" if has_tool_runs && has_code_facts => "code facts + tool diagnostics",
            "implementation_quality" if has_tool_runs => "LSP / linter diagnostics",
            "implementation_quality" => "code facts",
            _ if has_code_facts => "structure + code facts + heuristics",
            _ => "structure + design heuristics",
        };
        dimensions.push(DimensionReport {
            id: spec.id.into(),
            label: spec.label.into(),
            score,
            status: status_for_score(score).into(),
            active_findings: active_rc.len(),
            suppressed_findings: suppressed_rc.len(),
            penalty: (penalty * 10.0).round() / 10.0,
            basis: basis.into(),
        });
    }

    for s in &mut signals {
        let key = if s.root_cause.is_empty() { s.message.as_str() } else { s.root_cause.as_str() };
        if let Some(&p) = rc_penalties.get(key) {
            s.penalty = p;
        }
    }
    // compute per-entity hotspot count across unique active root causes
    let mut entity_rcs: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for s in &signals {
        if !s.entity.is_empty() && s.entity != "(design)" {
            let rc = if s.root_cause.is_empty() { s.message.clone() } else { s.root_cause.clone() };
            entity_rcs.entry(s.entity.clone()).or_default().insert(rc);
        }
    }
    for s in &mut signals {
        if let Some(rcs) = entity_rcs.get(&s.entity) {
            s.related_count = rcs.len().saturating_sub(1);
        }
    }

    // ---- overall score (weighted average of assessed dimensions)
    let (mut w_sum, mut val_sum) = (0.0, 0.0);
    for (spec, dim) in DIMENSIONS.iter().zip(dimensions.iter()) {
        if let Some(sc) = dim.score {
            w_sum += spec.weight;
            val_sum += spec.weight * sc as f64;
        }
    }
    let score = (w_sum > 0.0).then(|| (val_sum / w_sum).round().clamp(0.0, 100.0) as u8);

    // ---- evidence confidence
    let confidence = if live.is_empty() {
        0.0
    } else {
        let mut c = 0.40; // structural design graph
        if has_code_facts {
            c += 0.25;
        }
        if !languages.is_empty() {
            let ran_langs = languages.iter().filter(|l| runs.iter().any(|r| &r.language == *l && r.ran())).count();
            c += 0.25 * (ran_langs as f64 / languages.len() as f64);
        } else if has_tool_runs {
            c += 0.20;
        }
        if !d.change_scenarios.is_empty() && m.changes.iter().any(|ch| ch.analysed && !ch.stale) {
            c += 0.10;
        }
        if ctx.evidence.is_some_and(|e| e.stale_findings > 0) {
            c -= 0.10;
        }
        ((c.clamp(0.0, 1.0)) * 100.0).round() / 100.0
    };
    let evidence_level = if live.is_empty() {
        "unassessed"
    } else if confidence >= 0.80 {
        "verified"
    } else if confidence >= 0.60 {
        "partial"
    } else {
        "structural"
    }
    .to_string();

    let forces = force_health(d, m, &dimensions);

    let compare = |a: &Signal, b: &Signal| {
        (&a.dimension, &a.rule, &a.target, &a.provenance, &a.message)
            .cmp(&(&b.dimension, &b.rule, &b.target, &b.provenance, &b.message))
    };
    signals.sort_by(compare);
    suppressed.sort_by(compare);

    let mut report = HealthReport {
        version: VERSION.into(),
        scope: "design".into(),
        score,
        confidence,
        evidence_level,
        dimensions,
        forces,
        challenges: vec![],
        signals,
        suppressed,
        exceptions: d.health_exceptions.clone(),
        runs,
        gaps,
        languages,
        profiles: live.iter().map(|c| profiles::of_class(c).language.to_string()).collect::<BTreeSet<_>>().into_iter().collect(),
        idioms,
        explain: String::new(),
    };
    let unique = report.unique_signals();
    let challenges = assumption_challenges(d, &live, &unique);
    let active_count = unique.len();
    let assessed_dims = report.dimensions.iter().filter(|d| d.score.is_some()).count();
    let explain = match score {
        None => "No internal classes or code units to evaluate yet.".into(),
        Some(sc) => {
            let gap_note = if report.gaps.is_empty() {
                String::new()
            } else {
                format!(" {} evidence gap(s) remain.", report.gaps.len())
            };
            format!(
                "Score {sc}/100 ({} evidence, {:.0}% confidence) from {active_count} active finding(s) across {assessed_dims} assessed dimension(s).{gap_note}",
                report.evidence_level,
                confidence * 100.0,
            )
        }
    };
    report.challenges = challenges;
    report.explain = explain;
    report
}

#[cfg(test)]
mod tests;
