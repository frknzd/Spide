use spide_core::model::Layer::{self, *};
use spide_core::rules::*;

const LAYERS: [Layer; 4] = [Ui, Infrastructure, Application, Domain];

/// Allowed-dependency matrix row-major over LAYERS (from → to).
fn matrix(p: Preset) -> Vec<Vec<bool>> {
    LAYERS.iter().map(|f| LAYERS.iter().map(|t| allowed(*f, *t, p).is_ok()).collect()).collect()
}

#[test]
fn clean_preset_points_inward() {
    //            ui     infra  app    domain
    let want = vec![
        vec![true, true, true, true],    // ui
        vec![true, true, true, true],    // infrastructure
        vec![false, false, true, true],  // application
        vec![false, false, false, true], // domain
    ];
    assert_eq!(matrix(Preset::Clean), want);
    assert_eq!(matrix(Preset::ModularMonolith), want);
}

#[test]
fn hexagonal_forbids_adapter_to_adapter() {
    let want = vec![
        vec![true, false, true, true],
        vec![false, true, true, true],
        vec![false, false, true, true],
        vec![false, false, false, true],
    ];
    assert_eq!(matrix(Preset::Hexagonal), want);
    let v = allowed(Ui, Infrastructure, Preset::Hexagonal).unwrap_err();
    assert!(v.fix_hint.contains("port"));
}

#[test]
fn layered_is_downward_only() {
    let want = vec![
        vec![true, true, true, true],     // ui → everything below
        vec![false, true, false, false],  // infrastructure is the bottom
        vec![false, true, true, true],    // application
        vec![false, true, false, true],   // domain → infrastructure ok
    ];
    assert_eq!(matrix(Preset::Layered), want);
}

#[test]
fn none_allows_everything() {
    assert!(matrix(Preset::None).iter().flatten().all(|x| *x));
}

#[test]
fn modules_talk_via_api() {
    assert!(module_violation("orders.domain", "billing.domain").is_some());
    assert!(module_violation("orders.application", "billing.api").is_none());
    assert!(module_violation("orders.domain", "orders.application").is_none(), "same module: layer rules apply instead");
    assert!(module_violation("domain", "billing.domain").is_none(), "not modular");
}

#[test]
fn globs_and_custom_rules() {
    assert!(glob("*Controller", "LoanController"));
    assert!(glob("orders.*", "orders.domain"));
    assert!(glob("H?tp*", "HttpRequest"));
    assert!(!glob("*Controller", "ControllerX"));
    assert!(glob("*", ""));
    let rules = Rules {
        forbidden: vec![DepRule { from: "*Controller".into(), to: "*Repository".into(), reason: Some("go via a service".into()) }],
        ..Default::default()
    };
    assert_eq!(forbidden_by(&rules, "LoanController", Some("ui"), "LoanRepository", Some("infrastructure")).count(), 1);
    assert_eq!(forbidden_by(&rules, "LendingService", Some("application"), "LoanRepository", None).count(), 0);
    assert_eq!(type_tokens("Map<String, javax.servlet.HttpRequest>").collect::<Vec<_>>(), vec!["Map", "String", "javax.servlet.HttpRequest"]);
}

#[test]
fn rules_default_from_v1_json() {
    let d = spide_core::Design::from_json(r#"{"name":"x"}"#).unwrap();
    assert_eq!(d.rules.preset, Preset::Clean);
    assert_eq!(d.rules.thresholds.god_members, 16);
    let d = spide_core::Design::from_json(r#"{"rules":{"preset":"hexagonal","thresholds":{"high_fan_out":4}}}"#).unwrap();
    assert_eq!(d.rules.preset, Preset::Hexagonal);
    assert_eq!(d.rules.thresholds.high_fan_out, 4);
    assert_eq!(d.rules.thresholds.god_members, 16, "missing thresholds keep defaults");
}
