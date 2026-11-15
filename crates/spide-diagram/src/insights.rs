//! Derived views of the diagnostics for the canvas and hover cards: which methods are in the wrong
//! place (and where they belong), what each method does, and where behaviour overlaps.

use crate::visuals::Rgb;
use spide_core::metrics::Metrics;
use spide_core::model::Class;
use std::collections::{BTreeSet, HashMap};

#[derive(Debug, Clone, PartialEq)]
pub struct MethodMark {
    pub kind: String,
    pub home: Option<String>,
    pub message: String,
}

const PLACEMENT: [&str; 6] = ["misplaced_responsibility", "duplication", "concept_elsewhere", "feature_envy", "ui_bypasses_controller", "demeter"];

/// The label drawn after a flagged method (`↗ Home` or `⚠ kind`).
pub fn mark_text(mk: &MethodMark) -> String {
    match &mk.home {
        Some(h) => format!("↗ {h}"),
        None => format!("⚠ {}", mk.kind.replace('_', " ")),
    }
}

pub type Marks = HashMap<String, HashMap<String, MethodMark>>;

/// class → method → why it's flagged and its suggested home (best guess; suggest_home ranks properly).
pub fn method_marks(m: Option<&Metrics>) -> Marks {
    let mut out: Marks = HashMap::new();
    let Some(m) = m else { return out };
    let mut homes: HashMap<String, String> = HashMap::new();
    for f in &m.responsibility.findings {
        if let (Some(meth), Some(home)) = (&f.method, &f.home) {
            homes.insert(format!("{}.{meth}", f.class), home.clone());
        }
    }
    for c in &m.overlap.claims {
        if c.uses_no_own_data {
            homes.insert(c.method.clone(), c.owner.clone());
        }
    }
    for i in &m.issues {
        if !PLACEMENT.contains(&i.kind) {
            continue;
        }
        let methods: Vec<&String> = i.targets.iter().filter(|t| t.contains('.')).collect();
        for (k, t) in methods.iter().enumerate() {
            let dot = t.find('.').unwrap();
            let (cls, meth) = (&t[..dot], &t[dot + 1..]);
            let mut home = homes.get(t.as_str()).cloned();
            if home.is_none() && i.kind == "feature_envy" {
                home = i.targets.get(2).cloned();
            }
            if home.is_none() && i.kind == "duplication" {
                let other = if k <= 1 { methods.get(1 - k) } else { None };
                home = other.map(|s| s.split('.').next().unwrap_or("").to_string()).filter(|s| !s.is_empty());
            }
            if home.is_none() {
                if let Some(rest) = i.fix_hint.as_deref().and_then(|h| h.strip_prefix("move it to ")) {
                    home = rest.split([' ', ',', '(']).next().map(String::from);
                }
            }
            let per = out.entry(cls.to_string()).or_default();
            per.entry(meth.to_string()).or_insert(MethodMark { kind: i.kind.to_string(), home, message: i.message.clone() });
        }
    }
    out
}

/// class → method → concern (presentation, domain_logic, …).
pub fn method_concerns(m: Option<&Metrics>) -> HashMap<String, HashMap<String, String>> {
    let mut out: HashMap<String, HashMap<String, String>> = HashMap::new();
    if let Some(m) = m {
        for x in &m.responsibility.methods {
            out.entry(x.class.clone()).or_default().insert(x.method.clone(), x.concern.clone());
        }
    }
    out
}

pub fn concern_color(c: &str) -> Option<Rgb> {
    Some(match c {
        "presentation" => Rgb::hex(0xc084fc),
        "coordination" => Rgb::hex(0x60a5fa),
        "domain_logic" => Rgb::hex(0x34d399),
        "persistence" => Rgb::hex(0xf472b6),
        "formatting" => Rgb::hex(0xfbbf24),
        "creation" => Rgb::hex(0xa3e635),
        "accessor" => Rgb::hex(0x7b8294),
        _ => return None,
    })
}

pub const CONCERNS: [&str; 7] = ["presentation", "coordination", "domain_logic", "persistence", "formatting", "creation", "accessor"];

#[derive(Debug, Clone, PartialEq)]
pub struct OverlapLink {
    pub from: String,
    pub to: String,
    pub label: String,
    pub strength: f64,
}

/// Class pairs that duplicate each other's behaviour, and methods dealing with another class's concept.
pub fn overlap_links(m: Option<&Metrics>) -> Vec<OverlapLink> {
    let mut out: Vec<OverlapLink> = vec![];
    let Some(m) = m else { return out };
    for c in &m.overlap.classes {
        let share = c.share_a.max(c.share_b);
        if c.pairs > 0 {
            out.push(OverlapLink { from: c.a.clone(), to: c.b.clone(), label: format!("duplicate {}%", (share * 100.0).round()), strength: share });
        } else if c.responsibility >= 0.25 {
            out.push(OverlapLink { from: c.a.clone(), to: c.b.clone(), label: format!("same responsibilities {}%", (c.responsibility * 100.0).round()), strength: c.responsibility });
        }
    }
    for c in &m.overlap.claims {
        let cls = c.method.split('.').next().unwrap_or("").to_string();
        if cls != c.owner && !out.iter().any(|l| (l.from == cls && l.to == c.owner) || (l.to == cls && l.from == c.owner)) {
            let meth = c.method.split('.').nth(1).unwrap_or("");
            out.push(OverlapLink { from: cls, to: c.owner.clone(), label: format!("{meth}() → {}'s “{}”", c.owner, c.shared.join(" ")), strength: 0.5 });
        }
    }
    out
}

/// class → scattered concepts it takes part in.
pub fn scattered_by_class(m: Option<&Metrics>) -> HashMap<String, Vec<String>> {
    let mut out: HashMap<String, Vec<String>> = HashMap::new();
    if let Some(m) = m {
        for s in &m.overlap.scattered {
            for c in &s.classes {
                out.entry(c.clone()).or_default().push(s.concept.clone());
            }
        }
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Family {
    Complexity,
    Duplication,
    Responsibility,
    Coupling,
    Encapsulation,
    Architecture,
    Inheritance,
    Uml,
}

impl Family {
    pub fn label(self) -> &'static str {
        match self {
            Family::Complexity => "complexity",
            Family::Duplication => "duplication",
            Family::Responsibility => "responsibility",
            Family::Coupling => "coupling",
            Family::Encapsulation => "encapsulation",
            Family::Architecture => "architecture",
            Family::Inheritance => "inheritance",
            Family::Uml => "UML",
        }
    }
    pub fn icon(self) -> &'static str {
        match self {
            Family::Complexity => "≋",
            Family::Duplication => "⧉",
            Family::Responsibility => "↗",
            Family::Coupling => "⛓",
            Family::Encapsulation => "◇",
            Family::Architecture => "▦",
            Family::Inheritance => "△",
            Family::Uml => "□",
        }
    }
}

fn family_of(kind: &str) -> Option<Family> {
    Some(match kind {
        "long_method" | "long_params" | "type_switch" => Family::Complexity,
        "duplication" => Family::Duplication,
        "misplaced_responsibility" | "concept_elsewhere" | "feature_envy" | "creator" => Family::Responsibility,
        "ui_bypasses_controller" | "layer" | "module_boundary" | "forbidden_dep" | "forbidden_type" => Family::Architecture,
        "demeter" => Family::Coupling,
        "exposed_state" => Family::Encapsulation,
        "narrowed_visibility" | "inherited_repeated" => Family::Inheritance,
        "missing_types" => Family::Uml,
        _ => return None,
    })
}

#[derive(Debug, Clone, PartialEq)]
pub struct MemberIssue {
    pub kind: String,
    pub family: Family,
    pub severity: f64,
    pub confidence: f64,
    pub basis: String,
    pub message: String,
    pub explain: String,
    pub fix_hint: Option<String>,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MemberDiagnosis {
    pub issues: Vec<MemberIssue>,
    pub severity: f64,
    pub family: Family,
}

pub type Members = HashMap<String, HashMap<String, MemberDiagnosis>>;

/// class → `attribute:name` / `method:name` → findings that explicitly target that member.
/// Class-only findings are never spread to rows.
pub fn member_diagnostics(m: Option<&Metrics>, classes: &[Class]) -> Members {
    let mut out: Members = HashMap::new();
    let Some(m) = m else { return out };
    let by_name: HashMap<&str, &Class> = classes.iter().map(|c| (c.name.as_str(), c)).collect();
    for i in &m.issues {
        let targets: BTreeSet<&String> = i.targets.iter().collect();
        for t in targets {
            let Some((cls, c)) = by_name.iter().filter(|(name, _)| t.starts_with(&format!("{name}."))).max_by_key(|(name, _)| name.len()) else { continue };
            let cls = *cls;
            let raw = &t[cls.len() + 1..];
            let name = if c.methods.iter().any(|m| m.name == raw) || c.attributes.iter().any(|a| a.name == raw) { raw } else { raw.strip_suffix("()").unwrap_or(raw) };
            let attr = c.attributes.iter().any(|a| a.name == name);
            let meth = c.methods.iter().any(|x| x.name == name);
            let kind = match (attr, meth) {
                (true, false) => "attribute",
                (false, true) => "method",
                (true, true) => {
                    if matches!(i.kind, "exposed_state" | "inherited_repeated") {
                        "attribute"
                    } else if family_of(i.kind).is_some() {
                        "method"
                    } else {
                        continue;
                    }
                }
                _ => continue,
            };
            let family = family_of(i.kind).unwrap_or(match i.principle {
                "coupling" => Family::Coupling,
                "architecture" => Family::Architecture,
                "uml" => Family::Uml,
                _ => Family::Responsibility,
            });
            let issue = MemberIssue {
                kind: i.kind.to_string(),
                family,
                severity: i.severity,
                confidence: i.confidence,
                basis: if i.basis.is_empty() { "structure".into() } else { i.basis.clone() },
                message: i.message.clone(),
                explain: i.explain.clone(),
                fix_hint: i.fix_hint.clone(),
                source: i.source.to_string(),
            };
            let per = out.entry(cls.to_string()).or_default();
            let d = per.entry(format!("{kind}:{name}")).or_insert(MemberDiagnosis { issues: vec![], severity: 0.0, family });
            if d.issues.iter().any(|x| x.kind == issue.kind && x.message == issue.message) {
                continue;
            }
            d.severity = d.severity.max(issue.severity * issue.confidence);
            d.issues.push(issue);
            d.issues.sort_by(|a, b| (b.severity * b.confidence).total_cmp(&(a.severity * a.confidence)));
            d.family = d.issues[0].family;
        }
    }
    out
}

/// Plain-language strength label: confirmed structure vs potential concern.
pub fn evidence_label(basis: &str, confidence: f64) -> &'static str {
    if basis == "structure" && confidence >= 0.95 {
        "structural finding"
    } else if basis == "code" {
        "measured in code"
    } else {
        "potential concern"
    }
}

/// strength of a member finding (severity × confidence) → colour
pub fn finding_color(score: f64) -> Rgb {
    use crate::visuals::c;
    if score >= 0.3 {
        c::CYCLE
    } else if score >= 0.15 {
        c::WARN
    } else {
        c::DIM
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spide_core::metrics::Issue;
    use spide_core::model::{Attribute, Method};

    fn issue(kind: &'static str, targets: &[&str], severity: f64, confidence: f64, basis: &str) -> Issue {
        Issue {
            kind,
            severity,
            message: format!("{kind} message"),
            targets: targets.iter().map(|s| s.to_string()).collect(),
            rule: None,
            explain: format!("{kind} explanation"),
            fix_hint: Some("review".into()),
            principle: "responsibilities",
            source: "spide",
            confidence,
            basis: basis.into(),
        }
    }

    fn design() -> Vec<Class> {
        let a = |n: &str| Attribute { name: n.into(), ..Default::default() };
        let m = |n: &str| Method { name: n.into(), ..Default::default() };
        vec![Class { name: "Order".into(), attributes: vec![a("total"), a("state")], methods: vec![m("total"), m("submit"), m("submit")], ..Default::default() }]
    }

    #[test]
    fn member_diagnostics_resolve_explicit_targets_and_keep_multiple_findings() {
        let m = Metrics { issues: vec![issue("long_method", &["Order", "Order.submit"], 0.25, 0.9, "code"), issue("demeter", &["Order", "Order.submit"], 0.3, 0.7, "heuristic"), issue("god", &["Order"], 0.3, 0.8, "heuristic")], ..Default::default() };
        let out = member_diagnostics(Some(&m), &design());
        let d = &out["Order"]["method:submit"];
        assert_eq!(d.issues.iter().map(|i| i.kind.as_str()).collect::<Vec<_>>(), ["long_method", "demeter"]);
        assert_eq!(d.family, Family::Complexity);
        assert_eq!(out["Order"].len(), 1, "class-only findings are not spread to rows");
    }

    #[test]
    fn ambiguous_member_names_are_resolved_by_finding_kind_or_skipped() {
        let m = Metrics {
            issues: vec![
                issue("exposed_state", &["Order", "Order.total"], 0.3, 0.8, "heuristic"),
                issue("long_method", &["Order", "Order.total"], 0.3, 0.8, "heuristic"),
                issue("unknown", &["Order", "Order.total"], 0.3, 0.8, "heuristic"),
                issue("long_method", &["Missing.run"], 0.3, 0.8, "heuristic"),
            ],
            ..Default::default()
        };
        let out = member_diagnostics(Some(&m), &design());
        let per = &out["Order"];
        assert_eq!(per["attribute:total"].issues[0].kind, "exposed_state");
        assert_eq!(per["method:total"].issues[0].kind, "long_method");
        assert_eq!(per["method:total"].issues.len(), 1);
        assert!(!out.contains_key("Missing"));
    }

    #[test]
    fn evidence_wording_separates_structural_measured_and_potential_findings() {
        assert_eq!(evidence_label("structure", 1.0), "structural finding");
        assert_eq!(evidence_label("code", 0.9), "measured in code");
        assert_eq!(evidence_label("heuristic", 0.55), "potential concern");
    }
}
