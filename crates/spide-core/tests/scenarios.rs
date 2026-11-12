use spide_core::metrics::compute;
use spide_core::scenarios::score;
use spide_core::*;

fn ops(json: &str) -> Vec<Op> {
    serde_json::from_str(json).expect("ops parse")
}

pub fn shop() -> Design {
    ops::apply_all(
        &Design::default(),
        &ops(r#"[{"op":"add_package","name":"web","layer":"ui"},{"op":"add_package","name":"app","layer":"application"},
                 {"op":"add_package","name":"domain","layer":"domain"},
                 {"op":"add_class","name":"CartController","package":"web","methods":[{"name":"checkout"}]},
                 {"op":"add_class","name":"CheckoutService","package":"app","methods":[{"name":"checkout"}]},
                 {"op":"add_class","name":"Order","package":"domain","stereotype":"entity","aggregate_root":true,"methods":[{"name":"place"}]},
                 {"op":"add_class","name":"Stock","package":"domain","stereotype":"entity","aggregate_root":true,"methods":[{"name":"reserve"}]},
                 {"op":"add_class","name":"Line","package":"domain","stereotype":"entity","methods":[{"name":"price"}]}]"#),
        ops::Mode::Commit(Status::Accepted),
    )
    .unwrap()
}

fn msg(from: &str, to: &str, method: &str, kind: MessageKind) -> Message {
    Message { from: from.into(), to: to.into(), method: method.into(), kind, ..Default::default() }
}

pub fn checkout() -> Sequence {
    use MessageKind::*;
    Sequence {
        name: "checkout".into(),
        participants: vec!["CartController".into(), "CheckoutService".into(), "Order".into(), "Stock".into(), "Line".into()],
        messages: vec![
            msg("CartController", "CheckoutService", "checkout", Sync),
            msg("CheckoutService", "Order", "place", Sync),
            msg("Order", "Line", "price", Sync),
            msg("Line", "Order", "price", Return),
            msg("Order", "CheckoutService", "", Return),
            msg("CheckoutService", "Stock", "reserve", Sync),
            msg("CheckoutService", "Stock", "nope", Sync),
        ],
        ..Default::default()
    }
}

#[test]
fn scores_hops_depth_crossings_and_aggregates() {
    let mut d = shop();
    let mut s = checkout();
    store::validate_sequence(&d, &mut s);
    d.sequences.push(s.clone());
    let sc = score(&s, &d);
    assert_eq!(sc.hops, 5);
    assert_eq!(sc.max_depth, 3, "Controller → Service → Order → Line");
    assert_eq!(sc.package_crossings, 4); // web→app, app→domain ×3
    assert_eq!(sc.layer_crossings, 4);
    assert_eq!(sc.aggregates_touched, vec!["Order".to_string(), "Stock".to_string()], "aggregate roots only");
    assert_eq!(sc.unknown_calls, 1);
    assert_eq!(sc.container_crossings, 0);
}

#[test]
fn chatty_scenarios_become_issues() {
    let mut d = shop();
    d.sequences.push(checkout());
    assert!(!compute(&d, false).issues.iter().any(|i| i.kind == "chatty_scenario"));
    d.rules.thresholds.max_scenario_hops = 3;
    let m = compute(&d, false);
    assert!(m.issues.iter().any(|i| i.kind == "chatty_scenario" && i.message.contains("5 calls")));
    assert_eq!(m.scenarios.len(), 1);
}

#[test]
fn change_scenarios_footprint_staleness_and_ripple_issue() {
    let mut s = Store::new(shop());
    s.apply_user(ops(r#"[{"op":"add_relation","from":"CheckoutService","to":"Order","kind":"dependency"},
                         {"op":"add_relation","from":"CartController","to":"CheckoutService","kind":"dependency"},
                         {"op":"add_relation","from":"Order","to":"Line","kind":"composition"},
                         {"op":"add_change_scenario","name":"Gift cards"}]"#))
        .unwrap();
    let m = compute(&s.view(), false);
    assert!(!m.changes[0].analysed);
    let impacted: Vec<ImpactedClass> = serde_json::from_str(r#"[{"class":"Order","why":"new payment"},{"class":"Line"},{"class":"Ghost"}]"#).unwrap();
    let warnings = s.set_change_impact("Gift cards".into(), None, impacted).unwrap();
    assert_eq!(warnings, vec!["unknown class Ghost dropped".to_string()]);
    let m = compute(&s.view(), false);
    let fp = &m.changes[0];
    assert_eq!(fp.impacted, vec!["Line".to_string(), "Order".into()]);
    assert_eq!(fp.ripple, vec!["CheckoutService".to_string()]);
    assert!((fp.locality - 0.4).abs() < 1e-9, "3 of 5 classes involved");
    assert!(fp.analysed && !fp.stale);
    assert!(m.issues.iter().any(|i| i.kind == "change_ripple"));
    // a later model change makes the analysis stale
    s.apply_user(ops(r#"[{"op":"add_class","name":"Coupon"}]"#)).unwrap();
    assert!(compute(&s.view(), false).changes[0].stale);
    // agent tool upserts
    let out = tools::call(&mut s, "set_change_impact", &serde_json::json!({"name":"Multi-currency","impacted":[{"class":"Line"}]})).unwrap();
    assert!(out.text.contains("locality"));
    assert_eq!(s.design.change_scenarios.len(), 2);
}

#[test]
fn quality_attribute_checks() {
    let mut d = shop();
    d.containers = vec![
        Container { name: "Web".into(), ..Default::default() },
        Container { name: "Orders".into(), ..Default::default() },
        Container { name: "Inventory".into(), replicas: 1, ..Default::default() },
    ];
    for (pkg, k) in [("web", "Web"), ("app", "Orders"), ("domain", "Inventory")] {
        d.packages.iter_mut().find(|p| p.name == pkg).unwrap().container = Some(k.into());
    }
    d = ops::apply_all(
        &d,
        &ops(r#"[{"op":"add_relation","from":"CartController","to":"Order","kind":"dependency"},
                 {"op":"add_relation","from":"CheckoutService","to":"Stock","kind":"dependency","interaction":"async"}]"#),
        ops::Mode::Commit(Status::Accepted),
    )
    .unwrap();
    assert_eq!(d.relations[1].interaction, Interaction::Async);
    let mut s = checkout();
    store::validate_sequence(&d, &mut s);
    d.sequences.push(s);
    let m = compute(&d, false);
    let kinds: Vec<&str> = m.issues.iter().map(|i| i.kind).collect();
    assert!(kinds.contains(&"spof"), "Inventory is used by Web and Orders: {kinds:?}");
    assert!(kinds.contains(&"sync_chain"), "{kinds:?}");
    assert!(kinds.contains(&"multi_aggregate_write"), "Order + Stock: {kinds:?}");
    assert_eq!(m.scenarios[0].container_crossings, 4, "Web→Orders, Orders→Inventory ×3");
    d.containers[2].replicas = 3;
    assert!(!compute(&d, false).issues.iter().any(|i| i.kind == "spof"));
}
