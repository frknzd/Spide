use spide_core::metrics::compute;
use spide_core::*;

fn design(ops_json: &str) -> Design {
    let ops: Vec<Op> = serde_json::from_str(ops_json).unwrap();
    ops::apply_all(&Design::default(), &ops, ops::Mode::Commit(Status::Accepted, None)).unwrap()
}

#[test]
fn clean_design_is_healthy() {
    let d = design(
        r#"[{"op":"add_class","name":"A"},{"op":"add_class","name":"B"},{"op":"add_class","name":"C"},
            {"op":"add_relation","from":"A","to":"B","kind":"association"},
            {"op":"add_relation","from":"B","to":"C","kind":"association"}]"#,
    );
    let m = compute(&d, false);
    assert_eq!(m.health, 100);
    assert!(m.cycles.is_empty());
    assert_eq!(m.classes["B"].fan_in, 1);
    assert_eq!(m.classes["B"].fan_out, 1);
    assert!((m.classes["A"].instability - 1.0).abs() < 1e-9);
}

#[test]
fn detects_cycles_including_implicit() {
    let d = design(
        r#"[{"op":"add_class","name":"A"},
            {"op":"add_class","name":"B","methods":[{"name":"f","params":[{"name":"a","type":"List<A>"}]}]},
            {"op":"add_class","name":"C"},
            {"op":"add_relation","from":"A","to":"B","kind":"association"},
            {"op":"add_relation","from":"B","to":"C","kind":"dependency"},
            {"op":"add_relation","from":"C","to":"A","kind":"dependency"}]"#,
    );
    let m = compute(&d, false);
    assert_eq!(m.cycles, vec![vec!["A".to_string(), "B".into(), "C".into()]]);
    assert_eq!(m.cycle_edges.len(), 3);
    assert!(m.classes["A"].in_cycle);
    assert!(m.implicit_edges.iter().any(|e| e.from == "B" && e.to == "A" && e.in_cycle));
    assert!(m.health < 100);
    assert!(m.issues.iter().any(|i| i.kind == "cycle"));
}

#[test]
fn detects_god_class_and_lcom() {
    let mut methods = vec![];
    for i in 0..10 {
        methods.push(format!(r#"{{"name":"m{i}","uses":["a{}"]}}"#, i % 3));
    }
    let attrs: Vec<String> = (0..10).map(|i| format!(r#"{{"name":"a{i}","type":"int"}}"#)).collect();
    let d = design(&format!(
        r#"[{{"op":"add_class","name":"Manager","attributes":[{}],"methods":[{}],
              "responsibilities":["a","b","c","d","e"]}}]"#,
        attrs.join(","),
        methods.join(",")
    ));
    let m = compute(&d, false);
    let c = &m.classes["Manager"];
    assert_eq!(c.lcom, Some(3));
    assert!(c.god, "{c:?}");
    assert!(c.heat > 0.3);
    assert!(m.issues.iter().any(|i| i.kind == "god"));
}

#[test]
fn detects_layer_violation_and_package_cycle() {
    let d = design(
        r#"[{"op":"add_package","name":"domain","layer":"domain"},
            {"op":"add_package","name":"ui","layer":"ui"},
            {"op":"add_class","name":"Order","package":"domain"},
            {"op":"add_class","name":"OrderView","package":"ui"},
            {"op":"add_relation","from":"OrderView","to":"Order","kind":"dependency"},
            {"op":"add_relation","from":"Order","to":"OrderView","kind":"dependency"}]"#,
    );
    let m = compute(&d, false);
    assert_eq!(m.layer_violations, vec!["Order->OrderView:dependency".to_string()]);
    assert_eq!(m.package_cycles.len(), 1);
    assert_eq!(m.classes["Order"].layer_violations, 1);
    assert!(m.package_deps.iter().any(|p| p.layer_violation));
}

#[test]
fn orphans_and_ghost_counting() {
    let mut s = Store::default();
    let ops: Vec<Op> = serde_json::from_str(
        r#"[{"op":"add_class","name":"A"},{"op":"add_class","name":"B"},{"op":"add_class","name":"C"},
            {"op":"add_relation","from":"A","to":"B","kind":"association"}]"#,
    )
    .unwrap();
    let id = s.propose_ops(ops, None, "agent").unwrap();
    assert!(compute(&s.view(), false).classes.is_empty());
    let m = compute(&s.view(), true);
    assert!(m.classes["C"].orphan);
    s.accept_proposal(&id).unwrap();
    assert!(compute(&s.design, false).classes["C"].orphan);
}

#[test]
fn metrics_are_fast() {
    let mut ops = vec![];
    for i in 0..120 {
        ops.push(format!(r#"{{"op":"add_class","name":"C{i}","methods":[{{"name":"a","uses":["x"]}},{{"name":"b","uses":["x"]}},{{"name":"c"}}]}}"#));
    }
    for i in 0..120 {
        ops.push(format!(r#"{{"op":"add_relation","from":"C{i}","to":"C{}","kind":"dependency"}}"#, (i * 7 + 3) % 120));
    }
    let d = design(&format!("[{}]", ops.join(",")));
    let t = std::time::Instant::now();
    let m = compute(&d, false);
    assert_eq!(m.classes.len(), 120);
    assert!(t.elapsed().as_millis() < 50, "took {:?}", t.elapsed());
}

mod rules_in_metrics {
    use super::design;
    use spide_core::metrics::compute;
    use spide_core::rules::*;
    use spide_core::Layer;

    fn layered() -> spide_core::Design {
        design(
            r#"[{"op":"add_package","name":"ui","layer":"ui"},{"op":"add_package","name":"domain","layer":"domain"},
                {"op":"add_package","name":"db","layer":"infrastructure"},
                {"op":"add_class","name":"OrderController","package":"ui"},
                {"op":"add_class","name":"Order","package":"domain","attributes":[{"name":"req","type":"HttpRequest"}]},
                {"op":"add_class","name":"OrderRepository","package":"db"},
                {"op":"add_relation","from":"OrderController","to":"Order","kind":"dependency"},
                {"op":"add_relation","from":"Order","to":"OrderRepository","kind":"dependency"},
                {"op":"add_relation","from":"OrderController","to":"OrderRepository","kind":"dependency"}]"#,
        )
    }

    #[test]
    fn preset_changes_what_counts_as_a_violation() {
        let mut d = layered();
        // clean: domain → infrastructure points outward
        let m = compute(&d, false);
        assert_eq!(m.layer_violations, vec!["Order->OrderRepository:dependency".to_string()]);
        let i = m.issues.iter().find(|i| i.kind == "layer").unwrap();
        assert_eq!(i.rule.as_deref(), Some("clean: inward only"));
        assert!(i.fix_hint.as_deref().unwrap().contains("interface"));
        // layered: domain → infrastructure is downward = fine
        d.rules.preset = Preset::Layered;
        assert!(compute(&d, false).layer_violations.is_empty());
        // hexagonal: ui → infrastructure adapter-to-adapter
        d.rules.preset = Preset::Hexagonal;
        let m = compute(&d, false);
        assert!(m.layer_violations.contains(&"OrderController->OrderRepository:dependency".to_string()));
        d.rules.preset = Preset::None;
        assert_eq!(compute(&d, false).health, 100);
    }

    #[test]
    fn custom_rules_types_and_thresholds() {
        let mut d = layered();
        d.rules.preset = Preset::None;
        d.rules.forbidden.push(DepRule { from: "*Controller".into(), to: "*Repository".into(), reason: Some("go through a service".into()) });
        d.rules.forbidden_types.push(TypeRule { layer: Layer::Domain, pattern: "Http*".into(), reason: None });
        let m = compute(&d, false);
        let fd = m.issues.iter().find(|i| i.kind == "forbidden_dep").expect("forbidden dep");
        assert!(fd.message.contains("go through a service"));
        assert!(m.issues.iter().any(|i| i.kind == "forbidden_type" && i.message.contains("HttpRequest")));
        assert!(m.health < 100);
        // thresholds
        d.rules.forbidden.clear();
        d.rules.forbidden_types.clear();
        d.rules.thresholds.high_fan_out = 2;
        assert!(compute(&d, false).issues.iter().any(|i| i.kind == "coupling" && i.targets == vec!["OrderController".to_string()]));
    }

    #[test]
    fn modular_monolith_boundaries() {
        let mut d = design(
            r#"[{"op":"add_class","name":"Order","package":"orders.domain"},
                {"op":"add_class","name":"Invoice","package":"billing.domain"},
                {"op":"add_class","name":"BillingApi","package":"billing.api"},
                {"op":"add_relation","from":"Order","to":"Invoice","kind":"dependency"},
                {"op":"add_relation","from":"Order","to":"BillingApi","kind":"dependency"}]"#,
        );
        d.rules.preset = Preset::ModularMonolith;
        let m = compute(&d, false);
        let mb: Vec<_> = m.issues.iter().filter(|i| i.kind == "module_boundary").collect();
        assert_eq!(mb.len(), 1);
        assert!(mb[0].message.contains("Order->Invoice"));
    }
}

#[test]
fn external_packages_are_suggested_once_and_discounted() {
    let d = design(
        r#"[{"op":"add_package","name":"ui","layer":"ui"},{"op":"add_package","name":"lib"},
            {"op":"add_class","name":"Panel","package":"ui"},
            {"op":"add_class","name":"A","package":"lib"},{"op":"add_class","name":"B","package":"lib"},
            {"op":"add_class","name":"C","package":"lib"},{"op":"add_class","name":"D","package":"lib"},
            {"op":"add_class","name":"E","package":"lib"},{"op":"add_class","name":"F","package":"lib"},
            {"op":"add_relation","from":"Panel","to":"A","kind":"inheritance"},
            {"op":"add_relation","from":"Panel","to":"B","kind":"dependency"},{"op":"add_relation","from":"Panel","to":"C","kind":"dependency"},
            {"op":"add_relation","from":"Panel","to":"D","kind":"dependency"},{"op":"add_relation","from":"Panel","to":"E","kind":"dependency"},
            {"op":"add_relation","from":"Panel","to":"F","kind":"dependency"}]"#,
    );
    let before = compute(&d, false);
    assert_eq!(before.classes["Panel"].fan_out, 6);
    let mut s = Store::new(d);
    assert_eq!(s.external_candidates(), vec!["lib".to_string()]);
    let id = s.suggest_externals().expect("proposal");
    assert!(s.suggest_externals().is_none(), "suggested only once");
    s.accept_proposal(&id).unwrap();
    assert!(s.design.is_external("A"));
    let after = compute(&s.design, false);
    assert_eq!(after.classes["Panel"].fan_out, 0, "coupling to a stable library is discounted");
    assert!(after.health >= before.health);
    assert!(!after.issues.iter().any(|i| i.targets.contains(&"A".to_string())), "external classes aren't judged");
}

/// 200 classes with realistic, overlapping method names (the overlap engine compares names pairwise).
#[test]
fn metrics_scale_to_200_classes() {
    let verbs = ["add", "remove", "compute", "find", "validate", "show", "save", "count"];
    let nouns = ["Order", "Line", "Customer", "Invoice", "Payment", "Item", "Report", "Path"];
    let mut ops = vec![];
    for i in 0..200 {
        let ms: Vec<String> = (0..6)
            .map(|k| {
                let (v, n) = (verbs[(i + k) % 8], nouns[(i * 3 + k) % 8]);
                format!(r#"{{"name":"{v}{n}{k}","params":[{{"name":"x","type":"C{}"}}],"returns":"int","uses":["f{}"],"calls":["C{}.{}{}{}"]}}"#, (i + 1) % 200, k % 2, (i + 2) % 200, verbs[(i + 2 + k) % 8], nouns[((i + 2) * 3 + k) % 8], k)
            })
            .collect();
        ops.push(format!(r#"{{"op":"add_class","name":"C{i}","attributes":[{{"name":"f0","type":"int"}},{{"name":"f1","type":"C{}"}}],"methods":[{}]}}"#, (i + 5) % 200, ms.join(",")));
    }
    for i in 0..200 {
        ops.push(format!(r#"{{"op":"add_relation","from":"C{i}","to":"C{}","kind":"dependency"}}"#, (i * 7 + 3) % 200));
    }
    let d = design(&format!("[{}]", ops.join(",")));
    let t = std::time::Instant::now();
    let m = compute(&d, false);
    let ms = t.elapsed().as_millis();
    println!("200 classes: {ms} ms, {} issues", m.issues.len());
    let limit = if cfg!(debug_assertions) { 400 } else { 20 };
    assert!(ms < limit, "took {ms} ms");
}
