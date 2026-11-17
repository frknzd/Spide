//! Review calibration: compare behavior, not preferred class names.
use spide_core::{metrics::compute, *};

fn design(value: serde_json::Value) -> Design { Design::from_json(&value.to_string()).unwrap() }
fn measured(d: &Design, dim: &str) -> f64 { compute(d, false).health_report.dimensions[dim].measured.unwrap() }

#[test]
fn private_documentation_is_default_on_optional_and_unknown_safe() {
    let input = serde_json::json!({"classes":[{"name":"Parser","methods":[{"name":"helper","visibility":"private","description":"Not source documentation","code":{"file":"Parser.java","start_line":7,"source_documented":false}}]}]});
    let missing = design(input.clone());
    let report = compute(&missing, false).health_report;
    let signal = report.signals.iter().find(|s| s.rule == "private_documentation").expect("missing documentation finding");
    assert_eq!(signal.dimension, "implementation_quality");
    assert_eq!(signal.provenance, "code");
    assert_eq!(signal.target, "Parser.helper");
    assert_eq!(signal.file.as_deref(), Some("Parser.java"));
    assert_eq!(signal.line, Some(7));
    let mut documented = input.clone();
    documented["classes"][0]["methods"][0]["code"]["source_documented"] = true.into();
    assert!(report.measured < compute(&design(documented), false).health_report.measured);
    let mut disabled = input.clone();
    disabled["rules"] = serde_json::json!({"private_documentation":false});
    assert!(!compute(&design(disabled), false).health_report.signals.iter().any(|s| s.rule == "private_documentation"));
    let mut unknown = input;
    unknown["classes"][0]["methods"][0]["code"].as_object_mut().unwrap().remove("source_documented");
    assert!(!compute(&design(unknown), false).health_report.signals.iter().any(|s| s.rule == "private_documentation"));
}


#[test]
fn documentation_penalty_is_bounded_per_class_and_type_checks_per_method() {
    let mut d = design(serde_json::json!({"classes":[{"name":"Parser","methods":[{"name":"parse","visibility":"private","code":{"source_documented":false,"instanceofs":1}}]}]}));
    let initial = compute(&d, false).health_report;
    for i in 0..100 {
        let mut m = d.classes[0].methods[0].clone();
        m.name = format!("helper{i}");
        m.code.as_mut().unwrap().instanceofs = 0;
        d.classes[0].methods.push(m);
    }
    assert_eq!(initial.dimensions["implementation_quality"].measured, compute(&d, false).health_report.dimensions["implementation_quality"].measured);
    d.classes[0].methods.truncate(1);
    d.classes[0].methods[0].code.as_mut().unwrap().instanceofs = 100;
    let many = compute(&d, false).health_report;
    let s = many.signals.iter().find(|s| s.rule == "instanceof").unwrap();
    assert!(s.severity <= 0.35);
    assert_eq!(many.signals.iter().filter(|s| s.rule == "instanceof").count(), 1);
    assert!(many.dimensions["extensibility"].measured < initial.dimensions["extensibility"].measured);
}

#[test]
fn lifecycle_queries_use_an_existing_application_boundary() {
    let direct = design(serde_json::json!({"classes":[
        {"name":"Panel","role":"presentation","attributes":[{"name":"controller","type":"Controller"}],"methods":[{"name":"refresh","calls":["Namespace.isRunning"]}]},
        {"name":"Namespace","role":"domain","methods":[{"name":"isRunning","returns":"boolean"}]},
        {"name":"Controller","role":"controller","methods":[{"name":"isRunning","returns":"boolean","calls":["Namespace.isRunning"]}]}
    ]}));
    let mut mediated = direct.clone();
    mediated.classes[0].methods[0].calls = vec!["Controller.isRunning".into()];
    assert!(compute(&direct, false).issues.iter().any(|i| i.kind == "ui_bypasses_controller"));
    assert!(measured(&direct, "modularity") < measured(&mediated, "modularity"));
}

#[test]
fn namespace_concerns_not_boolean_or_collection_ownership() {
    let mut mixed = design(serde_json::json!({"classes":[{"name":"Namespace","attributes":[{"name":"enabled","type":"boolean"},{"name":"items","type":"List<String>"}],"methods":[
        {"name":"collect","concern":"domain_logic","uses":["items"]},
        {"name":"start","concern":"coordination","uses":["enabled"]},
        {"name":"save","concern":"persistence","uses":["enabled"]},
        {"name":"load","concern":"persistence","uses":["items"]},
        {"name":"render","concern":"presentation","uses":["items"]}
    ]}]}));
    let mut separated = mixed.clone();
    let methods = std::mem::take(&mut separated.classes[0].methods);
    separated.classes[0].methods = methods[..2].to_vec();
    separated.classes.push(Class { name: "Storage".into(), methods: methods[2..4].to_vec(), ..Default::default() });
    separated.classes.push(Class { name: "View".into(), methods: vec![methods[4].clone()], ..Default::default() });
    assert!(compute(&mixed, false).issues.iter().any(|i| i.kind == "mixed_concerns"));
    assert!(measured(&mixed, "modularity") < measured(&separated, "modularity"));
    let mut padded = mixed.clone();
    padded.classes.extend(separated.classes[1..].iter().map(|c| Class { name: c.name.clone(), ..Default::default() }));
    assert!(measured(&padded, "modularity") < measured(&separated, "modularity"), "empty abstractions must not explain the separation benefit");
    mixed.classes[0].methods.retain(|m| m.concern.as_deref() == Some("coordination"));
    assert!(!compute(&mixed, false).issues.iter().any(|i| i.kind == "mixed_concerns"));
}

#[test]
fn a_display_value_does_not_require_a_controller() {
    let d = design(serde_json::json!({"packages":[{"name":"ui","layer":"ui"},{"name":"model","layer":"domain"}],"classes":[
        {"name":"View","role":"presentation","package":"ui","methods":[{"name":"render","calls":["Value.getLabel"]}]},
        {"name":"Value","stereotype":"value","package":"model","methods":[{"name":"getLabel","returns":"String"}]}
    ]}));
    assert!(!compute(&d, false).issues.iter().any(|i| i.kind == "ui_bypasses_controller"));
}

#[test]
fn tree_type_dispatch_vs_registry_has_the_same_public_contract() {
    let dispatch = design(serde_json::json!({"classes":[{"name":"TreeParser","methods":[{"name":"parse","params":[{"name":"node","type":"Node"}],"returns":"TreeNode","code":{"instanceofs":3,"loc":12}}]}]}));
    let mut registry = dispatch.clone();
    registry.classes[0].methods[0].code.as_mut().unwrap().instanceofs = 0;
    // The registry keeps the same class/API: no benefit from merely adding abstractions.
    assert_eq!(dispatch.classes.len(), registry.classes.len());
    assert_eq!(dispatch.classes[0].methods[0].params, registry.classes[0].methods[0].params);
    // Strict ordering is asserted with the signal implementation below.
    assert!(measured(&dispatch, "extensibility") < measured(&registry, "extensibility"));
    assert!(compute(&dispatch, false).health_report.measured < compute(&registry, false).health_report.measured);
}

#[test]
fn application_commands_belong_behind_a_boundary() {
    let direct = design(serde_json::json!({"packages":[{"name":"ui","layer":"ui"},{"name":"model","layer":"domain"}],"classes":[
        {"name":"Panel","role":"presentation","package":"ui","methods":[{"name":"start","calls":["Session.start"]}]},
        {"name":"Session","role":"domain","package":"model","methods":[{"name":"start"}]}
    ]}));
    let mut mediated = direct.clone();
    mediated.classes[0].methods[0].calls = vec!["Controller.start".into()];
    mediated.classes.push(Class { name:"Controller".into(), stereotype:Some("controller".into()), methods:vec![Method {name:"start".into(),calls:vec!["Session.start".into()],..Default::default()}],..Default::default() });
    assert!(measured(&direct,"modularity") < measured(&mediated,"modularity"));
}
