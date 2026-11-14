use spide_core::*;
fn ops(s: &str) -> Vec<Op> { serde_json::from_str(s).unwrap() }
fn plan(s: &mut Store, json: &str) -> String { s.propose_ops(ops(json), None, "agent").unwrap() }

#[test]
fn isolated_ordered_preview_and_atomic_save() {
    let mut s = Store::default();
    s.apply_user(ops(r#"[{"op":"add_class","name":"A","role":"domain"},{"op":"add_class","name":"B"}]"#)).unwrap();
    let id = plan(&mut s, r#"[{"op":"update_class","name":"A","role":"controller"}]"#);
    let before = s.design.clone();
    let draft = ops(r#"[{"op":"add_attribute","class":"A","attribute":{"name":"id","type":"int"}},{"op":"rename_class","from":"A","to":"Renamed"},{"op":"remove_class","name":"B"}]"#);
    let r = s.review_proposal(&id, Some(&draft)).unwrap();
    assert!(r.can_accept); assert_eq!(r.steps.len(), 3);
    assert!(r.steps[0].changes.iter().any(|c| c.path == "classes.A"));
    assert!(r.design.class("Renamed").is_some());
    assert_eq!(r.design.class("B").unwrap().status, Status::Removing);
    assert_eq!(r.baseline.class("A").unwrap().role.as_deref(), Some("domain"));
    assert_eq!(s.design, before);
    s.update_proposal(&id, before.rev, draft.clone(), Some("amended".into()), None, None).unwrap();
    assert_eq!(s.design.classes, before.classes);
    assert_eq!(s.design.proposals[0].ops, draft);
    assert!(s.undo()); assert_eq!(s.design.proposals, before.proposals);
    assert!(s.redo()); assert_eq!(s.design.proposals[0].ops, draft);
    s.accept_proposal(&id).unwrap();
    assert!(s.design.class("Renamed").is_some()); assert!(!s.design.class("B").is_some_and(|c| c.status.is_live()));
}

#[test]
fn prerequisites_and_rejection_errors_are_explicit() {
    let mut s = Store::default();
    let a = plan(&mut s, r#"[{"op":"add_class","name":"A"}]"#);
    let b = plan(&mut s, r#"[{"op":"add_method","class":"A","method":{"name":"run"}}]"#);
    let r = s.review_proposal(&b, None).unwrap();
    assert!(r.errors.is_empty()); assert!(!r.can_accept); assert!(r.prerequisites.contains(&a));
    let before = s.design.clone(); assert!(s.accept_proposal(&b).is_err()); assert_eq!(s.design, before);
    s.reject_proposal(&a, None).unwrap();
    let r = s.review_proposal(&b, None).unwrap(); assert!(!r.errors.is_empty()); assert!(!r.can_accept);
    assert_eq!(s.design.proposals.len(), 1);
    assert!(s.undo()); s.accept_proposal(&a).unwrap(); assert!(s.review_proposal(&b, None).unwrap().can_accept);
}

#[test]
fn save_rejects_stale_empty_invalid_and_downstream_breakage_without_mutation() {
    let mut s = Store::default();
    let a = plan(&mut s, r#"[{"op":"add_class","name":"A"}]"#);
    plan(&mut s, r#"[{"op":"add_method","class":"A","method":{"name":"run"}}]"#);
    let before = s.design.clone();
    assert!(s.update_proposal(&a, before.rev - 1, before.proposals[0].ops.clone(), None, None, None).is_err());
    assert!(s.update_proposal(&a, before.rev, vec![], None, None, None).is_err());
    assert!(s.update_proposal(&a, before.rev, ops(r#"[{"op":"update_class","name":"Missing","role":"x"}]"#), None, None, None).is_err());
    let e = s.update_proposal(&a, before.rev, ops(r#"[{"op":"add_class","name":"Other"}]"#), None, None, None).unwrap_err();
    assert!(e.0.contains("invalidate"), "{e}"); assert_eq!(s.design, before);
}

#[test]
fn amendments_update_the_owned_option_but_never_approve_it() {
    let legacy: Proposal = serde_json::from_str(r#"{"id":"P1","ops":[],"source":"file"}"#).unwrap();
    assert_eq!(legacy.decision, None);
    let mut s = Store::default();
    let d = s.propose_decision("Why?".into(), None, vec![DecisionOption { label: "One".into(), ops: ops(r#"[{"op":"add_class","name":"A"}]"#), ..Default::default() }]).unwrap();
    let id = s.design.decisions.iter().find(|x| x.id == d).unwrap().plan.clone().unwrap();
    let rev = s.design.rev;
    assert!(s.update_proposal(&id, rev, s.design.proposals[0].ops.clone(), None, Some("missing".into()), None).is_err());
    s.update_proposal(&id, rev, ops(r#"[{"op":"add_class","name":"Amended"}]"#), None, Some(d.clone()), Some("A".into())).unwrap();
    assert_eq!(s.design.decisions[0].options[0].ops, s.design.proposals[0].ops);
    assert!(s.design.decisions[0].is_pending());
    assert!(s.design.classes.is_empty());
    s.accept_proposal(&id).unwrap();
    assert_eq!(s.design.decisions[0].state, spide_core::DecisionState::Accepted);
}

#[test]
fn invalid_preview_stops_after_error_and_metadata_updates_have_diffs() {
    let mut s = Store::default();
    s.apply_user(ops(r#"[{"op":"add_class","name":"A"}]"#)).unwrap();
    let id = plan(&mut s, r#"[{"op":"update_class","name":"A","role":"controller"}]"#);
    let r = s.review_proposal(&id, None).unwrap(); assert!(!r.steps[0].changes.is_empty());
    let draft = ops(r#"[{"op":"remove_class","name":"Missing"},{"op":"add_class","name":"B"}]"#);
    let r = s.review_proposal(&id, Some(&draft)).unwrap();
    assert!(!r.errors.is_empty()); assert!(r.steps[1].error.is_some()); assert!(r.design.class("B").is_none());
}

#[test]
fn non_class_plans_are_reviewable_and_tool_associations_are_atomic() {
    let mut s = Store::default();
    let id = plan(&mut s, r#"[{"op":"add_force","name":"Auditability","kind":"quality"}]"#);
    let review = s.review_proposal(&id, None).unwrap();
    assert!(review.can_accept); assert_eq!(review.steps[0].changes[0].path, "forces.Auditability");
    assert!(s.design.forces.is_empty());
    let before = s.design.clone();
    assert!(tools::call(&mut s, "propose_ops", &serde_json::json!({"ops":[{"op":"add_class","name":"A"}],"decision":"missing"})).is_err());
    assert_eq!(s.design, before);
    assert!(serde_json::from_str::<Vec<Op>>(r#"[{"op":"add_class","name":42}]"#).is_err());
    assert!(serde_json::from_str::<Vec<Op>>(r#"[{"op":"not_an_operation"}]"#).is_err());
}
