use serde_json::json;
use spide_core::{impact::Impact, metrics::compute, Design};

#[test]
fn legacy_ratings_round_trip_but_do_not_change_evidence() {
    let assessments = json!({"modularity": {"dimension":"modularity", "score":99, "rationale":"Legacy rationale", "cited":["Order"]}});
    let priorities = json!({"modularity":3});
    let design: Design = serde_json::from_value(json!({
        "health_assessments": assessments, "health_priorities": priorities
    })).unwrap();
    let saved = serde_json::to_value(&design).unwrap();
    assert_eq!(saved["health_assessments"]["modularity"]["rationale"], assessments["modularity"]["rationale"]);
    assert_eq!(saved["health_assessments"]["modularity"]["cited"], assessments["modularity"]["cited"]);
    assert_eq!(saved["health_priorities"]["modularity"].as_f64(), Some(3.0));
    let baseline = compute(&Design::default(), false);
    let metrics = compute(&design, false);
    assert_eq!(metrics.health_report, baseline.health_report);
    let report = serde_json::to_value(metrics.health_report).unwrap();
    for key in ["score", "measured", "assisted", "legacy", "dimensions", "coverage"] {
        assert!(report.get(key).is_none(), "retired field {key}");
    }
}

#[test]
fn legacy_impact_loads_without_reemitting_scores() {
    let impact: Impact = serde_json::from_value(json!({
        "health_before": 70, "health_after": 90, "added": ["Order"]
    })).unwrap();
    let saved = serde_json::to_value(impact).unwrap();
    assert_eq!(saved["added"], json!(["Order"]));
    assert!(saved.get("health_before").is_none());
    assert!(saved.get("health_after").is_none());
}
