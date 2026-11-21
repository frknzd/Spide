//! Evidence is not a verdict: check availability, finding provenance, and exceptions.
use spide_core::{evidence::{Category, EvidenceSet, Finding, Level, Range, ToolRun}, health::{Context, HealthException}, metrics::{compute, compute_with}, *};

fn shop() -> Design {
    Design::from_json(r#"{"classes":[{"name":"Order"},{"name":"Line"}],"relations":[{"from":"Order","to":"Line","kind":"dependency"},{"from":"Line","to":"Order","kind":"dependency"}]}"#).unwrap()
}

#[test]
fn check_status_and_diagnostics_are_separate() {
    let d = shop();
    let languages = ["java".into()];
    let mut ev = EvidenceSet::default();
    ev.record_run(ToolRun { tool:"javac".into(), language:"java".into(), status:"missing".into(), ..Default::default() });
    let report = |ev: &EvidenceSet| compute_with(&d, false, Context { evidence: Some(ev), languages: &languages, now: 0 }).health_report;
    let missing = report(&ev);
    assert_eq!(missing.runs[0].status, "missing");
    assert!(missing.gaps.iter().any(|g| g.contains("No successful analysis")));
    ev.runs[0].status = "ran".into();
    assert!(!report(&ev).gaps.iter().any(|g| g.contains("No successful analysis")));
    let f = Finding { file:"Order.java".into(), range:Range { line:3, ..Default::default() }, tool:"javac".into(), code:"E1".into(), category:Category::Correctness, level:Level::Error, doc_hash:Some("old".into()), provider:"linter".into(), ..Default::default() };
    ev.add([f.clone(), Finding { provider:"lsp".into(), ..f }]);
    let diagnostic = report(&ev).signals.into_iter().filter(|s| s.rule == "evidence:correctness").collect::<Vec<_>>();
    assert_eq!(diagnostic.len(), 1);
    assert_eq!(diagnostic[0].provenance, "linter");
    assert_eq!(diagnostic[0].line, Some(3));
    ev.drop_stale(&[("Order.java".into(), "new".into())].into());
    assert!(report(&ev).gaps.iter().any(|g| g.contains("stale diagnostic")));
    assert!(!report(&ev).signals.iter().any(|s| s.rule == "evidence:correctness"));
}

#[test]
fn cycle_finding_survives_padding_and_is_traceable_after_rename() {
    let mut d = shop();
    let cycles = |d: &Design| compute(d, false).health_report.signals.into_iter().filter(|s| s.rule == "cycle").collect::<Vec<_>>();
    let before = cycles(&d);
    assert!(!before.is_empty());
    d.classes.extend((0..50).map(|i| Class { name:format!("Extra{i}"), ..Default::default() }));
    assert_eq!(cycles(&d), before);
    let ops = serde_json::from_str::<Vec<Op>>(r#"[{"op":"rename_class","from":"Order","to":"Purchase"}]"#).unwrap();
    let renamed = ops::apply_all(&d, &ops, ops::Mode::Commit(Status::Accepted, None)).unwrap();
    assert!(cycles(&renamed).iter().any(|s| s.target == "Purchase"));
}

#[test]
fn speculative_abstraction_is_an_inspectable_lead() {
    let d = shop();
    let ops = serde_json::from_str::<Vec<Op>>(r#"[{"op":"add_class","name":"Pricing"},{"op":"add_class","name":"IOrder","stereotype":"interface","methods":[{"name":"run","is_abstract":true}]},{"op":"add_relation","from":"Order","to":"IOrder","kind":"realization"}]"#).unwrap();
    let over = ops::apply_all(&d, &ops, ops::Mode::Commit(Status::Accepted, None)).unwrap();
    assert!(compute(&over, false).health_report.signals.iter().any(|s| s.rule == "speculative_abstraction"));
}

#[test]
fn exceptions_require_approval_and_expire_without_erasing_the_finding() {
    let mut d = shop();
    let report = |d: &Design, now| compute_with(d, false, Context { now, ..Default::default() }).health_report;
    let original = report(&d, 0).signals;
    d.health_exceptions.push(HealthException { id:"X1".into(), rule:"cycle".into(), scope:"*".into(), reason:"legacy bridge".into(), expires_at:Some(10), ..Default::default() });
    assert_eq!(report(&d, 1).signals, original);
    d.health_exceptions[0].approved = true;
    let approved = report(&d, 1);
    assert!(approved.signals.iter().all(|s| s.rule != "cycle"));
    assert!(!approved.suppressed.is_empty());
    assert_eq!(approved.exceptions[0].reason, "legacy bridge");
    assert_eq!(report(&d, 10).signals, original);
}
