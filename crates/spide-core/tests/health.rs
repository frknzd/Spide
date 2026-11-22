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

#[test]
fn overall_score_is_anti_padding_monotonic_and_unassessed_when_empty() {
    let empty = compute(&Design::default(), false).health_report;
    assert_eq!(empty.version, "health-2");
    assert_eq!(empty.score, None);
    assert_eq!(empty.evidence_level, "unassessed");
    assert_eq!(empty.dimensions.len(), 6);

    let mut cyclic = shop();
    let r_cyclic = compute(&cyclic, false).health_report;
    let s_cyclic = r_cyclic.score.expect("non-empty design is scored");
    assert!(s_cyclic < 100, "cycle should reduce score, got {s_cyclic}");
    assert!(r_cyclic.dimension_score("modularity").unwrap() < 100);
    assert_eq!(r_cyclic.dimension_score("implementation_quality"), None, "no code/tool evidence => unknown");

    // Anti-padding: adding 50 unrelated classes must not inflate the score.
    let mut padded = cyclic.clone();
    padded.classes.extend((0..50).map(|i| Class { name: format!("Pad{i}"), ..Default::default() }));
    // Connect them in a chain so they don't add orphan penalties
    for i in 0..50 {
        let to = if i == 0 { "Order".into() } else { format!("Pad{}", i - 1) };
        padded.relations.push(Relation { id: format!("r{i}"), from: format!("Pad{i}"), to, kind: RelationKind::Dependency, ..Default::default() });
    }
    let r_padded = compute(&padded, false).health_report;
    assert_eq!(r_padded.dimension_score("modularity"), r_cyclic.dimension_score("modularity"));

    // Breaking the cycle strictly improves the score.
    cyclic.relations.retain(|r| !(r.from == "Line" && r.to == "Order"));
    let r_clean = compute(&cyclic, false).health_report;
    assert!(r_clean.score.unwrap() > s_cyclic, "breaking cycle must raise score: {} vs {}", r_clean.score.unwrap(), s_cyclic);
}

#[test]
fn brief_forces_align_with_health_and_change_scenarios() {
    let d = Design::from_json(r#"{
        "classes":[{"name":"Order"},{"name":"Line"}],
        "relations":[{"from":"Order","to":"Line","kind":"dependency"},{"from":"Line","to":"Order","kind":"dependency"}],
        "forces":[
            {"name":"Low coupling","kind":"quality","priority":1},
            {"name":"Add new payment provider","kind":"change","priority":1}
        ]
    }"#).unwrap();
    let r = compute(&d, false).health_report;
    assert_eq!(r.forces.len(), 2);
    let coupling = r.forces.iter().find(|f| f.force == "Low coupling").unwrap();
    assert!(coupling.related_dimensions.contains(&"modularity".to_string()));
    assert!(coupling.related_findings >= 1);
    let change = r.forces.iter().find(|f| f.force == "Add new payment provider").unwrap();
    assert_eq!(change.status, "unmeasured");
}

#[test]
fn decomplexification_detectors_and_tackle_ordering_and_challenges() {
    let d = Design::from_json(r#"{
        "rules": {"preset": "hexagonal", "forbidden": [{"from": "GhostPkg*", "to": "Other*"}]},
        "packages": [{"name":"p1"},{"name":"p2"},{"name":"p3"},{"name":"p4"}],
        "classes": [
            {"name":"ApiFacade","package":"p1","methods":[{"name":"run","calls":["AppService.run"]}]},
            {"name":"AppService","package":"p2","methods":[{"name":"run","calls":["RepoPort.save"]}]},
            {"name":"RepoPort","package":"p3","stereotype":"interface","methods":[{"name":"save","is_abstract":true}]},
            {"name":"SqlRepo","package":"p4","methods":[{"name":"save","params":[{"name":"x","type":""}]}]}
        ],
        "relations": [
            {"from":"ApiFacade","to":"AppService","kind":"dependency"},
            {"from":"AppService","to":"RepoPort","kind":"dependency"},
            {"from":"SqlRepo","to":"RepoPort","kind":"realization"}
        ]
    }"#).unwrap();
    let r = compute(&d, false).health_report;
    let unique = r.unique_signals();
    // Detects pass-through chain and package fragmentation
    assert!(unique.iter().any(|s| s.rule == "indirection_chain"), "expected indirection_chain: {unique:?}");
    assert!(unique.iter().any(|s| s.rule == "over_fragmented_packages"), "expected over_fragmented_packages: {unique:?}");
    // Tackle order: simplify comes before polish (missing_types)
    let simplify_pos = unique.iter().position(|s| s.tier == "simplify").unwrap();
    let polish_pos = unique.iter().position(|s| s.tier == "polish").unwrap();
    assert!(simplify_pos < polish_pos, "simplify findings should appear before polish findings");
    // Pattern hint attached
    assert!(unique[simplify_pos].pattern_hint.is_some());
    // Challenges: heavy preset on small design + unmatched custom rule + unjustified abstractions
    assert!(r.challenges.iter().any(|c| c.kind == "preset"), "expected preset challenge: {:?}", r.challenges);
    assert!(r.challenges.iter().any(|c| c.kind == "rule"), "expected unmatched rule challenge: {:?}", r.challenges);
    assert!(r.challenges.iter().any(|c| c.kind == "abstraction"), "expected abstraction challenge: {:?}", r.challenges);
}
