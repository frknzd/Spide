use spide_core::impact::*;
use spide_core::*;

fn ops(json: &str) -> Vec<Op> {
    serde_json::from_str(json).expect("ops parse")
}
fn base() -> Design {
    ops::apply_all(
        &Design::default(),
        &ops(r#"[{"op":"add_package","name":"ui","layer":"ui"},{"op":"add_package","name":"domain","layer":"domain"},
                 {"op":"add_class","name":"OrderView","package":"ui"},
                 {"op":"add_class","name":"Order","package":"domain"},
                 {"op":"add_class","name":"Line","package":"domain"},
                 {"op":"add_relation","from":"OrderView","to":"Order","kind":"dependency"},
                 {"op":"add_relation","from":"Order","to":"Line","kind":"composition"}]"#),
        ops::Mode::Commit(Status::Accepted, None),
    )
    .unwrap()
}

#[test]
fn cycle_introduced_and_rule_broken() {
    let b = base();
    let i = impact_of_ops(&b, &ops(r#"[{"op":"add_relation","from":"Line","to":"Order","kind":"association"}]"#)).unwrap();
    assert_eq!(i.added, vec!["Line —association→ Order".to_string()]);
    assert!(i.new_issues.iter().any(|m| m.contains("cycle")), "{:?}", i.new_issues);
    assert!(i.rule_breaks.is_empty());

    let i = impact_of_ops(&b, &ops(r#"[{"op":"add_relation","from":"Order","to":"OrderView","kind":"dependency"}]"#)).unwrap();
    assert_eq!(i.rule_breaks.len(), 1, "domain → ui breaks the clean preset");
    assert!(i.summary().contains("new rule break"));
}

#[test]
fn cycle_resolved() {
    let mut b = base();
    b = ops::apply_all(&b, &ops(r#"[{"op":"add_relation","from":"Line","to":"Order","kind":"association"}]"#), ops::Mode::Commit(Status::Accepted, None)).unwrap();
    let i = impact_of_ops(&b, &ops(r#"[{"op":"remove_relation","from":"Line","to":"Order"}]"#)).unwrap();
    assert!(i.resolved_issues.iter().any(|m| m.contains("cycle")));
    assert!(i.summary().contains("resolved finding"));
    assert_eq!(i.removed, vec!["Line → Order".to_string()]);
}

#[test]
fn flipped_dependency_and_deltas() {
    let b = base();
    let i = impact_of_ops(
        &b,
        &ops(r#"[{"op":"remove_relation","from":"Order","to":"Line"},{"op":"add_relation","from":"Line","to":"Order","kind":"association"}]"#),
    )
    .unwrap();
    assert_eq!(i.flipped_deps, vec![("Line".to_string(), "Order".to_string())]);
    let order = i.class_deltas.iter().find(|d| d.class == "Order").unwrap();
    assert_eq!((order.fan_in, order.fan_out), (1, -1));
}

#[test]
fn affected_sequences() {
    let mut b = base();
    b.sequences.push(Sequence { name: "checkout".into(), participants: vec!["OrderView".into(), "Order".into()], ..Default::default() });
    let i = impact_of_ops(&b, &ops(r#"[{"op":"add_attribute","class":"Order","attribute":{"name":"total","type":"int"}}]"#)).unwrap();
    assert_eq!(i.affected_scenarios, vec!["sequence \"checkout\"".to_string()]);
    assert_eq!(i.added, vec!["Order.total".to_string()]);
}
