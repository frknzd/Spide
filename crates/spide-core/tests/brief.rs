use spide_core::diff::diff;
use spide_core::store::Target;
use spide_core::*;

fn ops(json: &str) -> Vec<Op> {
    serde_json::from_str(json).expect("ops parse")
}

#[test]
fn v1_design_loads_with_defaults() {
    let text = include_str!("../../../tests/fixtures/library.json");
    let d = Design::from_json(text).expect("v1 parses");
    assert!(d.forces.is_empty());
    assert_eq!(d.settings.partner, PartnerMode::Examiner);
    let mut s = Store::default();
    s.load(d.clone());
    assert_eq!(s.design.version, SCHEMA_VERSION);
    // round-trip keeps the model
    let back = Design::from_json(&s.design.to_json()).unwrap();
    assert_eq!(back.classes, d.classes);
    assert_eq!(back.relations, d.relations);
}

#[test]
fn forces_are_proposed_accepted_updated_and_removed() {
    let mut s = Store::default();
    let id = s
        .propose_ops(
            ops(r#"[{"op":"add_force","name":"Evolvability","kind":"quality","priority":1,"description":"new loan types"},
                    {"op":"add_force","name":"Lend books","kind":"goal"}]"#),
            None,
            "agent",
        )
        .unwrap();
    assert!(s.design.forces.is_empty(), "proposed forces are ghosts");
    assert_eq!(s.view().forces[0].status, Status::Proposed);
    s.accept_proposal(&id).unwrap();
    assert_eq!(s.design.forces.len(), 2);
    assert_eq!(s.design.force("Lend books").unwrap().priority, 2, "default priority");

    s.apply_user(ops(r#"[{"op":"update_force","name":"Evolvability","rename":"Changeability","priority":9}]"#)).unwrap();
    let f = s.design.force("Changeability").unwrap();
    assert_eq!(f.priority, 3, "clamped");
    assert_eq!(f.kind, ForceKind::Quality);

    assert!(s.apply_user(ops(r#"[{"op":"add_force","name":"Lend books"}]"#)).is_err(), "duplicate");
    s.prune(&[Target::Force { name: "Lend books".into() }], None).unwrap();
    assert!(s.view().forces.iter().all(|f| f.name != "Lend books"));
    s.restore(&Target::Force { name: "Lend books".into() }).unwrap();
    s.apply_user(ops(r#"[{"op":"remove_force","name":"Lend books"}]"#)).unwrap();
    assert_eq!(s.design.forces.len(), 1);
}

#[test]
fn rename_force_updates_tradeoffs() {
    let mut s = Store::default();
    s.apply_user(ops(r#"[{"op":"add_force","name":"Speed","kind":"quality"},{"op":"add_class","name":"A"}]"#)).unwrap();
    let opts: Vec<DecisionOption> = serde_json::from_str(
        r#"[{"label":"x","ops":[{"op":"add_class","name":"B"}],"tradeoffs":[{"force":"Speed","effect":"-","note":"extra hop"}]}]"#,
    )
    .unwrap();
    s.propose_decision("q?".into(), None, opts).unwrap();
    s.apply_user(ops(r#"[{"op":"update_force","name":"Speed","rename":"Latency"}]"#)).unwrap();
    let t = &s.design.decisions[0].options[0].tradeoffs[0];
    assert_eq!(t.force, "Latency");
    assert!(!t.helps());
}

#[test]
fn diff_covers_forces() {
    let mut a = Design::default();
    a.forces.push(Force { name: "Keep".into(), ..Default::default() });
    a.forces.push(Force { name: "Drop".into(), ..Default::default() });
    let mut b = a.clone();
    b.forces.retain(|f| f.name != "Drop");
    b.forces[0].priority = 1;
    b.forces.push(Force { name: "New".into(), kind: ForceKind::Constraint, ..Default::default() });
    let d = diff(&a, &b);
    let after = ops::apply_all(&a, &d, ops::Mode::Commit(Status::Accepted, None)).unwrap();
    assert_eq!(after.forces, b.forces);
}

#[test]
fn mcp_exposes_brief_stage_and_guards_socratic() {
    use serde_json::json;
    use spide_core::tools;
    let mut s = Store::default();
    let v: serde_json::Value = serde_json::from_str(&tools::design_digest(&s)).unwrap();
    assert_eq!(v["stage"], "brief");
    s.apply_user(ops(r#"[{"op":"add_force","name":"Fast checkout","kind":"quality","priority":1}]"#)).unwrap();
    let v: serde_json::Value = serde_json::from_str(&tools::design_digest(&s)).unwrap();
    assert_eq!(v["stage"], "boundaries");
    assert_eq!(v["forces"][0]["name"], "Fast checkout");

    s.set_partner(PartnerMode::Socratic);
    let add_class = json!({"ops":[{"op":"add_class","name":"A"}]});
    assert!(tools::call(&mut s, "propose_ops", &add_class).is_err());
    let add_force = json!({"ops":[{"op":"add_force","name":"Cheap"}]});
    assert!(tools::call(&mut s, "propose_ops", &add_force).is_ok(), "recording forces is allowed");
    s.set_partner(PartnerMode::Examiner);
    let dec = json!({"question":"q","options":[{"label":"a","ops":[{"op":"add_class","name":"A"}],
        "tradeoffs":[{"force":"Nope","effect":"+"}]}]});
    let out = tools::call(&mut s, "propose_decision", &dec).unwrap();
    assert!(out.text.contains("not in the brief"));
    assert!(out.text.contains("at least 4"), "should nudge when < 4 options: {}", out.text);
    assert!(out.text.contains("design_notes"), "should nudge when missing depth paragraphs: {}", out.text);

    // 4 rich alternatives with pattern, design_notes, pros_detail, cons_detail produce no depth/count warnings
    let rich = json!({
        "question": "How should shape rendering dispatch?",
        "options": [
            {"label": "Polymorphic method", "pattern": "Polymorphism", "design_notes": "Each Shape subclass implements render().", "pros_detail": "Adding new shapes requires zero edits to existing code.", "cons_detail": "Adding new operations touches every Shape subclass.", "ops": [{"op":"add_class","name":"ShapeA"}]},
            {"label": "Double dispatch Visitor", "pattern": "Double Dispatch (Visitor)", "design_notes": "Shape exposes accept(ShapeVisitor) and visitors implement visitX().", "pros_detail": "Adding new operations is a single new visitor class.", "cons_detail": "Adding a new Shape breaks every visitor interface implementation.", "ops": [{"op":"add_class","name":"ShapeB"}]},
            {"label": "Tagged enum + switch", "pattern": "Tagged Enum", "design_notes": "Closed ShapeKind enum switched in Renderer.", "pros_detail": "Exhaustive pattern matching keeps all rendering logic co-located.", "cons_detail": "Every new shape variant forces edits across all switch sites.", "ops": [{"op":"add_class","name":"ShapeC"}]},
            {"label": "Strategy registry", "pattern": "Strategy + Registry", "design_notes": "Map shape type keys to pluggable RenderStrategy instances.", "pros_detail": "Decouples both shapes and renderers at runtime without inheritance.", "cons_detail": "Loses static exhaustiveness checking and adds registration indirection.", "ops": [{"op":"add_class","name":"ShapeD"}]}
        ]
    });
    let rich_out = tools::call(&mut s, "propose_decision", &rich).unwrap();
    assert!(!rich_out.text.contains("at least 4"), "unexpected count warning: {}", rich_out.text);
    assert!(!rich_out.text.contains("design_notes"), "unexpected depth warning: {}", rich_out.text);
    let stored = s.design.decisions.last().unwrap();
    assert_eq!(stored.options.len(), 4);
    assert_eq!(stored.options[1].pattern.as_deref(), Some("Double Dispatch (Visitor)"));
}
