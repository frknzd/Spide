//! Evidence boundaries: smells are review signals, not proof of design defects.
use spide_core::{course, metrics, ops, *};

fn sample() -> Design {
    let ops: Vec<Op> = serde_json::from_str(r#"[{"op":"add_class","name":"Worker","attributes":[{"name":"value","type":"int"}],"methods":[{"name":"process","uses":["value"]}]}]"#).unwrap();
    ops::apply_all(&Design::default(), &ops, ops::Mode::Commit(Status::Accepted, None)).unwrap()
}

#[test]
fn long_method_boundary_and_evidence() {
    let mut d = sample();
    d.classes[0].methods[0].code = Some(CodeFacts { loc: 30, ..Default::default() });
    assert!(!course::code_checks(&d, false).iter().any(|i| i.kind == "long_method"));
    let before = metrics::compute(&d, false).health;
    d.classes[0].methods[0].code.as_mut().unwrap().loc = 31;
    let issues = course::code_checks(&d, false);
    let i = issues.iter().find(|i| i.kind == "long_method").unwrap();
    assert!(i.targets.contains(&"Worker.process".into()));
    assert_eq!(i.basis, "code");
    assert!(i.confidence < 1.0 && i.severity < 0.5);
    assert!(!i.explain.is_empty() && i.fix_hint.is_some());
    assert!(metrics::compute(&d, false).health <= before);
    d.classes[0].methods[0].code = None;
    assert!(!course::code_checks(&d, false).iter().any(|i| i.kind == "long_method"));
}

#[test]
fn exposed_state_is_qualified_and_data_carriers_are_exempt() {
    let mut d = sample();
    d.classes[0].attributes[0].visibility = Visibility::Public;
    // a sketch: public is the unspecified default, not a decision
    assert!(!metrics::compute(&d, false).issues.iter().any(|i| i.kind == "exposed_state"));
    d.classes[0].methods[0].code = Some(CodeFacts { loc: 3, ..Default::default() });
    let m = metrics::compute(&d, false);
    let i = m.issues.iter().find(|i| i.kind == "exposed_state").unwrap();
    assert_eq!(i.basis, "heuristic");
    assert!(i.confidence < 0.6 && i.explain.contains("intentional"));
    assert!(i.targets.contains(&"Worker.value".into()));
    for stereotype in ["value", "record", "dto"] {
        d.classes[0].stereotype = Some(stereotype.into());
        assert!(!metrics::compute(&d, false).issues.iter().any(|i| i.kind == "exposed_state"));
    }
}

#[test]
fn narrowed_public_contract_is_not_a_behavioural_lsp_claim() {
    let mut d = sample();
    d.classes[0].methods[0].visibility = Visibility::Public;
    let mut child = d.classes[0].clone();
    child.name = "Child".into();
    child.methods[0].visibility = Visibility::Protected;
    d.classes.push(child);
    let ops: Vec<Op> = serde_json::from_str(r#"[{"op":"add_relation","from":"Child","to":"Worker","kind":"inheritance"}]"#).unwrap();
    let mut d = ops::apply_all(&d, &ops, ops::Mode::Commit(Status::Accepted, None)).unwrap();
    let m = metrics::compute(&d, false);
    let i = m.issues.iter().find(|i| i.kind == "narrowed_visibility").unwrap();
    assert!(i.explain.contains("not behavioural"));
    assert!(i.targets.contains(&"Child.process".into()));
    d.classes[1].methods[0].visibility = Visibility::Public;
    assert!(!metrics::compute(&d, false).issues.iter().any(|i| i.kind == "narrowed_visibility"));
}

#[test]
fn parameter_boundary_and_legitimate_data_types() {
    let mut d = sample();
    for n in 0..5 {
        d.classes[0].methods[0].params.push(Param { name: format!("p{n}"), ty: "int".into() });
        let m = metrics::compute(&d, false);
        assert_eq!(m.issues.iter().any(|i| i.kind == "long_params"), n == 4);
    }
    d.classes[0].methods.clear();
    let a = d.classes[0].attributes[0].clone();
    d.classes[0].attributes.push(Attribute { name: "other".into(), ..a });
    for stereotype in ["value", "record", "dto", "enum", "exception"] {
        d.classes[0].stereotype = Some(stereotype.into());
        assert!(!metrics::compute(&d, false).issues.iter().any(|i| i.kind == "data_class"), "{stereotype}");
    }
}
