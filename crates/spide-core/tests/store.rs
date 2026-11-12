use spide_core::store::Target;
use spide_core::*;

pub fn ops(json: &str) -> Vec<Op> {
    serde_json::from_str(json).expect("ops parse")
}

fn library() -> Store {
    let mut s = Store::default();
    let id = s
        .propose_ops(
            ops(r#"[
              {"op":"add_class","name":"Library","package":"domain",
               "attributes":[{"name":"name","type":"String"}],
               "methods":[{"name":"lend","params":[{"name":"b","type":"Book"},{"name":"m","type":"Member"}],"returns":"Loan","uses":["name"]}]},
              {"op":"add_class","name":"Book","package":"domain","attributes":[{"name":"isbn","type":"String","visibility":"private"}]},
              {"op":"add_class","name":"Member","package":"domain"},
              {"op":"add_class","name":"Loan","package":"domain"},
              {"op":"add_relation","from":"Library","to":"Book","kind":"composition","from_mult":"1","to_mult":"*"},
              {"op":"add_relation","from":"Loan","to":"Book","kind":"association","to_mult":"1"},
              {"op":"add_relation","from":"Loan","to":"Member","kind":"association","to_mult":"1"}
            ]"#),
            Some("start".into()),
            "agent",
        )
        .unwrap();
    s.accept_proposal(&id).unwrap();
    s
}

#[test]
fn proposals_are_ghosts_until_accepted() {
    let mut s = Store::default();
    let id = s.propose_ops(ops(r#"[{"op":"add_class","name":"A"}]"#), None, "agent").unwrap();
    assert!(s.design.classes.is_empty(), "proposal must not touch the design");
    let v = s.view();
    assert_eq!(v.classes[0].status, Status::Proposed);
    assert_eq!(v.classes[0].proposal.as_deref(), Some(id.as_str()));
    s.accept_proposal(&id).unwrap();
    assert_eq!(s.design.classes[0].status, Status::Accepted);
    assert!(s.design.proposals.is_empty());
}

#[test]
fn invalid_ops_are_rejected_atomically() {
    let mut s = library();
    let before = s.design.clone();
    let e = s
        .propose_ops(ops(r#"[{"op":"add_class","name":"X"},{"op":"add_relation","from":"X","to":"Nope","kind":"dependency"}]"#), None, "agent")
        .unwrap_err();
    assert!(e.0.contains("Nope"), "{e}");
    assert_eq!(s.design, before);
    assert!(s.propose_ops(ops(r#"[{"op":"add_class","name":"Book"}]"#), None, "agent").is_err());
}

#[test]
fn ghost_removal_marks_removing() {
    let mut s = library();
    s.propose_ops(ops(r#"[{"op":"remove_class","name":"Loan"}]"#), None, "agent").unwrap();
    let v = s.view();
    assert_eq!(v.class("Loan").unwrap().status, Status::Removing);
    assert!(v.relations.iter().filter(|r| r.from == "Loan").all(|r| r.status == Status::Removing));
    assert_eq!(s.design.class("Loan").unwrap().status, Status::Accepted);
}

#[test]
fn undo_redo_roundtrip() {
    let mut s = library();
    let n = s.design.classes.len();
    s.prune(&[Target::Class { name: "Loan".into() }], Some("loans belong to members".into())).unwrap();
    assert_eq!(s.design.class("Loan").unwrap().status, Status::Pruned);
    assert_eq!(s.design.feedback.last().unwrap().text, "loans belong to members");
    assert_eq!(s.view().classes.len(), n - 1);
    assert!(s.undo());
    assert_eq!(s.design.class("Loan").unwrap().status, Status::Accepted);
    assert!(s.redo());
    assert_eq!(s.design.class("Loan").unwrap().status, Status::Pruned);
}

#[test]
fn rename_updates_references() {
    let mut s = library();
    let id = s.propose_ops(ops(r#"[{"op":"rename_class","from":"Member","to":"Patron"}]"#), None, "agent").unwrap();
    s.accept_proposal(&id).unwrap();
    assert!(s.design.relations.iter().any(|r| r.to == "Patron" && r.id == "Loan->Patron:association"));
}

#[test]
fn decision_flow_accept_reject_overrule() {
    let mut s = library();
    let opts: Vec<DecisionOption> = serde_json::from_str(
        r#"[{"label":"Library creates loans","ops":[{"op":"add_relation","from":"Library","to":"Loan","kind":"dependency"}]},
            {"label":"Member creates loans","ops":[{"op":"add_relation","from":"Member","to":"Loan","kind":"association"}]}]"#,
    )
    .unwrap();
    let d = s.propose_decision("Who creates loans?".into(), None, opts).unwrap();
    assert!(s.submit_defense(&d, Some("A".into()), None, "".into()).is_err(), "defence required");
    s.submit_defense(&d, Some("A".into()), None, "Creator pattern: library aggregates books".into()).unwrap();
    s.evaluate_defense(&d, false, "Library doesn't record loans".into(), None).unwrap();
    assert!(!s.design.relations.iter().any(|r| r.from == "Library" && r.to == "Loan"));
    s.overrule(&d).unwrap();
    let r = s.design.relations.iter().find(|r| r.from == "Library" && r.to == "Loan").unwrap();
    assert_eq!(r.status, Status::Overruled);

    // strict mode forbids overrule
    let opts2: Vec<DecisionOption> =
        serde_json::from_str(r#"[{"label":"x","ops":[{"op":"add_class","name":"Fine"}]}]"#).unwrap();
    s.set_examiner(ExaminerMode::Strict);
    let d2 = s.propose_decision("Fines?".into(), None, opts2).unwrap();
    s.submit_defense(&d2, None, Some("no fines at all".into()), "keep it simple".into()).unwrap();
    s.evaluate_defense(&d2, false, "fines are a requirement".into(), None).unwrap();
    assert!(s.overrule(&d2).is_err());
    s.submit_defense(&d2, Some("A".into()), None, "ok fines are needed for late returns".into()).unwrap();
    s.evaluate_defense(&d2, true, "good".into(), None).unwrap();
    assert!(s.design.class("Fine").is_some());
}

#[test]
fn json_roundtrip() {
    let s = library();
    let json = s.design.to_json();
    let back = Design::from_json(&json).unwrap();
    assert_eq!(back, s.design);
    // tolerant to missing fields
    let minimal = Design::from_json(r#"{"classes":[{"name":"A"}]}"#).unwrap();
    assert_eq!(minimal.classes[0].status, Status::Accepted);
}

#[test]
fn sequence_validation() {
    let mut s = library();
    let seq: Sequence = serde_json::from_str(
        r#"{"name":"Lend","participants":["Library","Book"],
            "messages":[{"from":"Library","to":"Book","method":"isAvailable"},{"from":"Book","to":"Library","method":"lend"}]}"#,
    )
    .unwrap();
    let problems = s.set_sequence(seq).unwrap();
    assert_eq!(problems.len(), 1, "{problems:?}");
    let stored = &s.design.sequences[0];
    assert!(stored.messages[0].problem.is_some());
    assert!(stored.messages[1].problem.is_none());
}

#[test]
fn restore_package_brings_back_its_classes() {
    let mut s = library();
    s.prune(&[Target::Package { name: "domain".into() }], None).unwrap();
    assert!(s.design.classes.iter().all(|c| c.status == Status::Pruned));
    s.restore(&Target::Package { name: "domain".into() }).unwrap();
    assert!(s.design.classes.iter().all(|c| c.status == Status::Accepted));
    assert!(s.design.classes.iter().all(|c| c.feedback.is_none()));
}

#[test]
fn restore_members() {
    let mut s = library();
    let attr = Target::Attribute { class: "Book".into(), name: "isbn".into() };
    let method = Target::Method { class: "Library".into(), name: "lend".into() };
    s.prune(&[attr.clone(), method.clone()], None).unwrap();
    assert!(s.view().class("Book").unwrap().attributes.is_empty(), "pruned members are hidden");
    s.restore(&attr).unwrap();
    s.restore(&method).unwrap();
    assert_eq!(s.view().class("Book").unwrap().attributes.len(), 1);
    assert_eq!(s.view().class("Library").unwrap().methods.len(), 1);
}
