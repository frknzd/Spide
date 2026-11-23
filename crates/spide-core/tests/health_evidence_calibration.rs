use spide_core::{metrics::compute, Design};
use serde_json::json;

fn design(value: serde_json::Value) -> Design {
    Design::from_json(&value.to_string()).unwrap()
}

#[test]
fn in_memory_store_name_does_not_establish_infrastructure() {
    let d = design(json!({"classes":[
        {"name":"ApplicationService","methods":[{"name":"run","calls":["CallStore.add"]}]},
        {"name":"CallStore","attributes":[{"name":"calls","type":"List<String>"}],"methods":[{"name":"add","uses":["calls"]}]}
    ]}));
    assert!(!compute(&d, false).health_report.signals.iter().any(|s| s.rule == "concrete_dependency"));
    let mut explicit = d;
    explicit.classes[1].role = Some("persistence".into());
    assert!(compute(&explicit, false).health_report.signals.iter().any(|s| s.rule == "concrete_dependency"));
}

#[test]
fn uncorroborated_heuristics_are_visible_but_do_not_deduct_quality() {
    let d = design(json!({"classes":[
        {"name":"OrderView","methods":[{"name":"render","calls":["Snapshot.read"]}]},
        {"name":"Snapshot","methods":[{"name":"read"}]},
        {"name":"Readable","stereotype":"interface","methods":[{"name":"read"}]}
    ],"relations":[{"from":"Snapshot","to":"Readable","kind":"realization"}]}));
    let report = compute(&d, false).health_report;
    let leads: Vec<_> = report.signals.iter().filter(|s| s.provenance == "heuristic").collect();
    assert!(!leads.is_empty());
    for s in leads {
        assert!(s.confidence <= 0.35);
        assert_eq!(s.penalty, 0.0);
    }
    assert!(report.gaps.iter().any(|g| g.contains("No tool-run")));
    assert_eq!(report.dimensions.iter().find(|d| d.id == "implementation_quality").unwrap().score, None);
}

#[test]
fn stale_change_analysis_is_unknown_not_a_design_failure() {
    let mut d = design(json!({"model_rev":2,"classes":[
        {"name":"A"},{"name":"B"},{"name":"C"},{"name":"D"}
    ],"forces":[{"name":"Add format","kind":"change"}],
    "change_scenarios":[{"name":"Add format","generated_rev":1,"impacted":[
        {"class":"A"},{"class":"B"},{"class":"C"}
    ]}]}));
    let stale = compute(&d, false);
    assert!(!stale.issues.iter().any(|i| i.kind == "change_ripple"));
    assert_eq!(stale.health_report.forces[0].status, "unmeasured");
    assert!(stale.health_report.forces[0].summary.contains("stale"));
    d.change_scenarios[0].generated_rev = Some(d.model_rev);
    let fresh = compute(&d, false);
    assert!(fresh.issues.iter().any(|i| i.kind == "change_ripple"));
    assert_eq!(fresh.health_report.forces[0].status, "at_risk");
}

#[test]
fn equality_type_guard_is_not_variant_dispatch() {
    let mut d = design(json!({"classes":[{"name":"Snapshot","methods":[
        {"name":"equals","returns":"boolean","params":[{"name":"other","type":"Object"}],"code":{"instanceofs":1}}
    ]}]}));
    assert!(!compute(&d, false).health_report.signals.iter().any(|s| s.rule == "instanceof"));
    d.classes[0].methods[0].name = "dispatch".into();
    assert!(compute(&d, false).health_report.signals.iter().any(|s| s.rule == "instanceof"));
}

#[test]
fn factory_may_construct_concrete_infrastructure() {
    let d = design(json!({"classes":[
        {"name":"RepositoryFactory","methods":[{"name":"create","returns":"SqlRepository","calls":["SqlRepository.new"]}]},
        {"name":"SqlRepository","role":"persistence"}
    ]}));
    assert!(!compute(&d, false).health_report.signals.iter().any(|s| s.rule == "concrete_dependency"));
}
