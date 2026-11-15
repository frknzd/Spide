//! Health v2 invariants: unknown ≠ healthy, correlated findings count once, size doesn't hide
//! hotspots, renaming doesn't matter, overengineering isn't rewarded, exceptions need approval,
//! agent judgement is bounded, visible and goes stale.

use spide_core::evidence::{Category, EvidenceSet, Finding, Level, Range, ToolRun};
use spide_core::health::{self, Context, DimensionAssessment, HealthException};
use spide_core::metrics::{compute, compute_with};
use spide_core::*;

fn design(ops: &str) -> Design {
    let ops: Vec<Op> = serde_json::from_str(ops).unwrap();
    ops::apply_all(&Design::default(), &ops, ops::Mode::Commit(Status::Accepted, None)).unwrap()
}

fn shop() -> Design {
    design(
        r#"[{"op":"add_package","name":"domain","layer":"domain"},
        {"op":"add_class","name":"Order","package":"domain","attributes":[{"name":"lines","type":"List<Line>"}],"methods":[{"name":"total","uses":["lines"],"calls":["Line.subtotal"]}]},
        {"op":"add_class","name":"Line","package":"domain","attributes":[{"name":"qty","type":"int"}],"methods":[{"name":"subtotal","uses":["qty"]}]},
        {"op":"add_class","name":"Pricing","package":"domain","methods":[{"name":"tax","params":[{"name":"c","type":"int"}]}]},
        {"op":"add_relation","from":"Order","to":"Line","kind":"composition"},
        {"op":"add_relation","from":"Order","to":"Pricing","kind":"dependency"}]"#,
    )
}

fn with_cycle(mut d: Design) -> Design {
    let ops: Vec<Op> = serde_json::from_str(r#"[{"op":"add_relation","from":"Line","to":"Order","kind":"dependency"},{"op":"add_relation","from":"Pricing","to":"Order","kind":"dependency"}]"#).unwrap();
    d = ops::apply_all(&d, &ops, ops::Mode::Commit(Status::Accepted, None)).unwrap();
    d
}

fn run(lang: &str) -> ToolRun {
    ToolRun { tool: "t".into(), language: lang.into(), status: "ran".into(), ..Default::default() }
}

fn finding(file: &str, line: u32, code: &str, provider: &str, cat: Category, level: Level) -> Finding {
    Finding { code: code.into(), tool: "t".into(), provider: provider.into(), category: cat, level, language: "java".into(), file: file.into(), range: Range { line, ..Default::default() }, message: code.into(), ..Default::default() }
}

#[test]
fn missing_evidence_is_unknown_not_healthy() {
    let d = shop();
    let langs = vec!["java".to_string()];
    let none = compute_with(&d, false, Context { languages: &langs, ..Default::default() }).health_report;
    assert_eq!(none.dimensions["implementation_quality"].measured, None);
    assert!(none.provisional);
    let mut ev = EvidenceSet::default();
    ev.record_run(ToolRun { status: "missing".into(), ..run("java") });
    let missing = compute_with(&d, false, Context { evidence: Some(&ev), languages: &langs, ..Default::default() }).health_report;
    assert_eq!(missing.dimensions["implementation_quality"].measured, None, "a missing tool is not a clean run");
    ev.record_run(run("java"));
    let clean = compute_with(&d, false, Context { evidence: Some(&ev), languages: &langs, ..Default::default() }).health_report;
    assert_eq!(clean.dimensions["implementation_quality"].measured.map(|x| x.round()), Some(100.0));
    assert!(clean.coverage > none.coverage);
}

#[test]
fn correctness_errors_hurt_and_stay_on_top() {
    let d = shop();
    let langs = vec!["java".to_string()];
    let mut ev = EvidenceSet::default();
    ev.record_run(run("java"));
    let base = compute_with(&d, false, Context { evidence: Some(&ev), languages: &langs, ..Default::default() }).health_report;
    ev.add([finding("Order.java", 3, "E1", "lsp", Category::Correctness, Level::Error)]);
    let bad = compute_with(&d, false, Context { evidence: Some(&ev), languages: &langs, ..Default::default() }).health_report;
    assert!(bad.score < base.score);
    assert_eq!(bad.blocking.len(), 1);
    assert!(bad.top[0].rule.starts_with("evidence:correctness"));
    // style noise barely moves it
    let mut ev2 = EvidenceSet::default();
    ev2.record_run(run("java"));
    ev2.add((1..=5).map(|i| finding("Order.java", i, &format!("S{i}"), "linter", Category::Style, Level::Warning)));
    let style = compute_with(&d, false, Context { evidence: Some(&ev2), languages: &langs, ..Default::default() }).health_report;
    assert!(style.dimensions["implementation_quality"].measured.unwrap() > bad.dimensions["implementation_quality"].measured.unwrap());
}

#[test]
fn correlated_findings_count_once() {
    let d = shop();
    let langs = vec!["java".to_string()];
    let mut once = EvidenceSet::default();
    once.record_run(run("java"));
    once.add([finding("Order.java", 3, "unused", "linter", Category::Maintainability, Level::Warning)]);
    let mut twice = once.clone();
    // the same root cause from the language server
    twice.add([finding("Order.java", 3, "unused", "lsp", Category::Maintainability, Level::Warning)]);
    let a = compute_with(&d, false, Context { evidence: Some(&once), languages: &langs, ..Default::default() }).health_report;
    let b = compute_with(&d, false, Context { evidence: Some(&twice), languages: &langs, ..Default::default() }).health_report;
    assert_eq!(a.dimensions["implementation_quality"].measured, b.dimensions["implementation_quality"].measured);
}

#[test]
fn unrelated_size_dilutes_but_never_hides_a_hotspot() {
    let bad = with_cycle(shop());
    let mut big = bad.clone();
    let mut big_huge = bad.clone();
    for (d, n) in [(&mut big, 45), (&mut big_huge, 600)] {
        let ops: Vec<Op> = (0..n).map(|i| serde_json::from_value(serde_json::json!({"op":"add_class","name":format!("Util{i}"),"package":"domain"})).unwrap()).collect();
        *d = ops::apply_all(d, &ops, ops::Mode::Commit(Status::Accepted, None)).unwrap();
        // connected, so they aren't orphans
        let rels: Vec<Op> = (0..n).map(|i| serde_json::from_value(serde_json::json!({"op":"add_relation","from":format!("Util{i}"),"to":"Line","kind":"dependency"})).unwrap()).collect();
        *d = ops::apply_all(d, &rels, ops::Mode::Commit(Status::Accepted, None)).unwrap();
    }
    let m = |d: &Design| compute(d, false).health_report.dimensions["modularity"].measured.unwrap();
    assert!(m(&bad) < m(&big), "{} < {}", m(&bad), m(&big));
    assert!(m(&big_huge) < 92.0, "a cycle stays visible in a huge design: {}", m(&big_huge));
}

#[test]
fn renaming_does_not_change_health() {
    let d = with_cycle(shop());
    let ops: Vec<Op> = serde_json::from_str(r#"[{"op":"rename_class","from":"Order","to":"Purchase"},{"op":"rename_class","from":"Line","to":"Item"}]"#).unwrap();
    let r = ops::apply_all(&d, &ops, ops::Mode::Commit(Status::Accepted, None)).unwrap();
    assert_eq!(compute(&d, false).health_report.measured, compute(&r, false).health_report.measured);
}

#[test]
fn overengineering_is_not_rewarded() {
    let plain = shop();
    // an interface in front of every class, one implementation each, no expected change needs them
    let mut over = shop();
    for c in ["Order", "Line", "Pricing"] {
        let ops: Vec<Op> = serde_json::from_value(serde_json::json!([
            {"op":"add_class","name":format!("I{c}"),"stereotype":"interface","package":"domain","methods":[{"name":"run","is_abstract":true}]},
            {"op":"add_relation","from":c,"to":format!("I{c}"),"kind":"realization"}])).unwrap();
        over = ops::apply_all(&over, &ops, ops::Mode::Commit(Status::Accepted, None)).unwrap();
    }
    let s = |d: &Design| compute(d, false).health_report.dimensions["simplicity"].measured.unwrap();
    assert!(s(&over) < s(&plain), "{} < {}", s(&over), s(&plain));
    assert!(compute(&over, false).health_report.signals.iter().any(|x| x.rule == "speculative_abstraction"));
}

#[test]
fn exceptions_need_owner_approval_and_expire() {
    let mut d = with_cycle(shop());
    let before = compute(&d, false).health_report;
    let mut x = HealthException { id: "X1".into(), rule: "cycle".into(), scope: "*".into(), reason: "legacy bridge".into(), proposed_by: "agent".into(), ..Default::default() };
    d.health_exceptions.push(x.clone());
    assert_eq!(compute(&d, false).health_report.measured, before.measured, "proposed, not approved");
    x.approved = true;
    d.health_exceptions = vec![x.clone()];
    let after = compute(&d, false).health_report;
    assert!(after.measured > before.measured);
    assert!(!after.suppressed.is_empty(), "suppressed findings stay inspectable");
    x.expires_at = Some(1);
    d.health_exceptions = vec![x];
    assert_eq!(compute(&d, false).health_report.measured, before.measured, "expired");
}

#[test]
fn agent_judgement_is_bounded_visible_and_goes_stale() {
    let mut d = with_cycle(shop());
    let base = compute(&d, false).health_report;
    let m = base.dimensions["modularity"].measured.unwrap();
    let a = DimensionAssessment { dimension: "modularity".into(), score: 100.0, rationale: "the cycle is a deliberate event loop; fine here".into(), cited: vec!["Order".into(), "Line".into()], confidence: 0.9, ..Default::default() };
    assert!(health::record(&mut d, vec![DimensionAssessment { cited: vec![], ..a.clone() }], 0).is_err(), "citations required");
    health::record(&mut d, vec![a], 0).unwrap();
    let r = compute(&d, false).health_report;
    let dm = &r.dimensions["modularity"];
    assert_eq!(dm.agent, Some(100.0));
    assert!((dm.assisted.unwrap() - (0.6 * m + 40.0)).abs() < 1e-6);
    assert_eq!(r.measured, base.measured, "the reproducible score is untouched");
    assert!(r.assisted > r.measured && r.score == r.assisted);
    // the cited class changes: the judgement no longer counts
    let ops: Vec<Op> = serde_json::from_str(r#"[{"op":"add_method","class":"Order","method":{"name":"cancel"}}]"#).unwrap();
    d = ops::apply_all(&d, &ops, ops::Mode::Commit(Status::Accepted, None)).unwrap();
    let r = compute(&d, false).health_report;
    assert_eq!(r.dimensions["modularity"].agent, None);
    assert_eq!(r.stale_assessments, vec!["modularity".to_string()]);
}

#[test]
fn verified_errors_cap_the_assisted_implementation_quality() {
    let mut d = shop();
    let langs = vec!["java".to_string()];
    let mut ev = EvidenceSet::default();
    ev.record_run(run("java"));
    ev.add([finding("Order.java", 3, "E1", "lsp", Category::Correctness, Level::Error)]);
    health::record(&mut d, vec![DimensionAssessment { dimension: "implementation_quality".into(), score: 100.0, rationale: "looks fine to me, small and readable code".into(), cited: vec!["Order".into()], ..Default::default() }], 0).unwrap();
    let r = compute_with(&d, false, Context { evidence: Some(&ev), languages: &langs, ..Default::default() }).health_report;
    let iq = &r.dimensions["implementation_quality"];
    assert_eq!(iq.assisted, iq.measured);
}

#[test]
fn agent_only_dimension_is_judgement_only() {
    let mut d = shop();
    health::record(&mut d, vec![DimensionAssessment { dimension: "implementation_quality".into(), score: 80.0, rationale: "read the code: small functions, clear names".into(), cited: vec!["Order".into()], ..Default::default() }], 0).unwrap();
    let r = compute(&d, false).health_report;
    let iq = &r.dimensions["implementation_quality"];
    assert!(iq.judgement_only && iq.measured.is_none() && iq.coverage == 0.0);
}

#[test]
fn priorities_weight_dimensions() {
    let mut d = with_cycle(shop());
    let even = compute(&d, false).health_report.measured.unwrap();
    d.health_priorities.insert("modularity".into(), 3.0);
    let modular = compute(&d, false).health_report.measured.unwrap();
    assert!(modular < even, "weighting the broken dimension lowers the score");
}
