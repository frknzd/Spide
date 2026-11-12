use spide_core::metrics::compute;
use spide_core::*;

fn design(ops_json: &str) -> Design {
    let ops: Vec<Op> = serde_json::from_str(ops_json).unwrap();
    ops::apply_all(&Design::default(), &ops, ops::Mode::Commit(Status::Accepted)).unwrap()
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
