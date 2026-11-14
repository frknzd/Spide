use spide_core::{Design, Store, Op};
use spide_core::model::{DecisionOption, DecisionState};

fn add(name: &str) -> Op { serde_json::from_value(serde_json::json!({"op":"add_class","name":name})).unwrap() }

#[test]
fn small_change_is_a_decision_and_empty_notes_approve_without_examiner() {
    let mut s = Store::default();
    let p = s.propose_ops(vec![add("Order")], Some("Add Order".into()), "agent").unwrap();
    let d = s.design.decisions.iter().find(|d| d.plan.as_deref() == Some(&p)).unwrap().id.clone();
    assert!(s.design.classes.is_empty());
    let rev = s.design.rev;
    s.approve_decision(&d, rev, "".into()).unwrap();
    assert_eq!(s.design.decisions[0].state, DecisionState::Accepted);
    assert_eq!(s.design.classes[0].origin.as_deref(), Some(d.as_str()));
    assert!(s.evaluate_defense(&d, true, "late callback".into(), None).is_err());
    s.undo(); assert!(s.design.classes.is_empty());
    s.redo(); assert_eq!(s.design.classes.len(), 1);
}

#[test]
fn alternatives_and_stale_approval() {
    let mut s = Store::default();
    let id = s.propose_decision("Owner?".into(), None, vec![
        DecisionOption { id: "A".into(), label: "A".into(), ops: vec![add("A")], ..Default::default() },
        DecisionOption { id: "B".into(), label: "B".into(), ops: vec![add("B")], ..Default::default() },
    ]).unwrap();
    let rev = s.design.rev;
    s.select_decision_option(&id, "B", rev).unwrap();
    assert!(s.approve_decision(&id, rev, "".into()).is_err());
    s.approve_decision(&id, s.design.rev, "Use B".into()).unwrap();
    assert!(s.design.live_class("B").is_some());
    assert!(s.design.live_class("A").is_none());
    assert_eq!(s.design.decisions[0].notes, "Use B");
}

#[test]
fn migration_is_idempotent_and_never_approves_pending_work() {
    let mut s = Store::default();
    s.propose_ops(vec![add("A")], None, "file").unwrap();
    s.propose_ops(vec![add("B")], None, "code").unwrap();
    let mut legacy = s.design.clone();
    legacy.review_version = 0; legacy.decisions.clear();
    let migrated = Store::new(legacy).design;
    assert_eq!(migrated.decisions.len(), 2);
    assert!(migrated.classes.is_empty());
    let again = Store::new(Design::from_json(&migrated.to_json()).unwrap()).design;
    assert_eq!(migrated, again);
}
