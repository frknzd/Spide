//! UML sequence diagrams: nested steps ↔ flat storage, and the timeline (activations, implicit returns,
//! fragments, creation, destruction).
use spide_core::sequence::{self, Event};
use spide_core::*;

fn seq(v: serde_json::Value) -> Sequence {
    sequence::from_input(&v).unwrap()
}

fn actor(n: &str) -> bool {
    n == "Member"
}

fn tl(s: &Sequence) -> Vec<Event> {
    sequence::timeline(s, &actor).events
}

fn returns(ev: &[Event]) -> Vec<(String, String)> {
    ev.iter().filter_map(|e| if let Event::Return { from, to, .. } = e { Some((from.clone(), to.clone())) } else { None }).collect()
}

fn borrow() -> Sequence {
    seq(serde_json::json!({
        "name": "Borrow a book",
        "lifelines": [{"name": "Member"}, {"name": "ctl", "class": "LoanController"}, {"name": "svc", "class": "LendingService"}, {"name": "l", "class": "Loan"}],
        "steps": [
            {"from": "Member", "to": "ctl", "method": "borrow", "args": "isbn"},
            {"from": "ctl", "to": "svc", "method": "lend", "returns": "loan"},
            {"fragment": "loop", "guard": "for each copy", "steps": [
                {"from": "svc", "to": "svc", "method": "check"},
                {"fragment": "alt", "operands": [
                    {"guard": "available", "steps": [{"from": "svc", "to": "l", "method": "Loan", "kind": "create"}]},
                    {"guard": "else", "steps": [{"from": "svc", "to": "ctl", "method": "notify", "kind": "async"}]}]}]},
            {"ref": "Pay fine", "over": ["Member", "ctl"]},
            {"from": "svc", "to": "l", "method": "", "kind": "destroy"}]
    }))
}

#[test]
fn nested_steps_are_stored_flat_and_read_back() {
    let s = borrow();
    assert_eq!(s.messages.len(), 7);
    assert_eq!(s.participants, vec!["Member", "ctl", "svc", "l"]);
    assert_eq!(s.class_of("ctl"), "LoanController");
    assert_eq!(s.lifeline("l").label(false), "l : Loan");
    assert_eq!(s.lifeline("Member").label(true), "Member");
    let loop_ = s.fragments.iter().find(|f| f.kind == FragmentKind::Loop).unwrap();
    assert_eq!((loop_.operands[0].first, loop_.operands[0].last, loop_.operands[0].guard.as_deref()), (2, 4, Some("for each copy")));
    let alt = s.fragments.iter().find(|f| f.kind == FragmentKind::Alt).unwrap();
    assert_eq!(alt.operands.iter().map(|o| (o.first, o.last)).collect::<Vec<_>>(), vec![(3, 3), (4, 4)]);
    assert_eq!(s.messages[5].kind, MessageKind::Ref);
    // to_steps gives the same tree back
    let back = sequence::to_steps(&s);
    let again = seq(serde_json::json!({"name": "x", "lifelines": back["lifelines"], "steps": back["steps"]}));
    assert_eq!((again.messages.len(), again.fragments.len()), (s.messages.len(), s.fragments.len()));
    assert_eq!(sequence::sane_fragments(&again), sequence::sane_fragments(&s));
}

#[test]
fn activations_implicit_returns_and_frames() {
    let s = borrow();
    let ev = tl(&s);
    // the actor isn't activated; ctl and svc are, Loan from its creation
    assert!(!ev.iter().any(|e| matches!(e, Event::Activate { lifeline, .. } if lifeline == "Member")));
    let create = ev.iter().position(|e| *e == Event::Create("l".into())).unwrap();
    assert_eq!(ev[create + 1], Event::Message(3));
    // the created Loan's activation ends inside the alt operand, before the separator; a constructor
    // doesn't draw a reply
    let sep = ev.iter().position(|e| matches!(e, Event::OperandStart { operand: 1, .. })).unwrap();
    let end = ev.iter().position(|e| *e == Event::Deactivate { lifeline: "l".into() }).unwrap();
    assert!(create < end && end < sep, "{ev:#?}");
    assert!(!returns(&ev).iter().any(|(f, _)| f == "l"));
    // self-calls nest without a reply arrow
    assert!(!returns(&ev).contains(&("svc".into(), "svc".into())));
    // at the end everything still active replies, inner first, with the call's `returns` as label
    let tail = returns(&ev);
    assert_eq!(&tail[tail.len() - 2..], &[("svc".to_string(), "ctl".to_string()), ("ctl".to_string(), "Member".to_string())]);
    assert!(ev.iter().any(|e| matches!(e, Event::Return { from, label, .. } if from == "svc" && label.as_deref() == Some("loan"))));
    // the loop's inner self-activation ends inside the loop
    let end = ev.iter().position(|e| *e == Event::FragmentEnd(0)).unwrap();
    assert_eq!(ev[end - 1], Event::Deactivate { lifeline: "svc".into() });
    // destroy ends the lifeline
    assert!(ev.contains(&Event::Destroy("l".into())));
    // every activation ends
    let acts = ev.iter().filter(|e| matches!(e, Event::Activate { .. })).count();
    let deacts = ev.iter().filter(|e| matches!(e, Event::Deactivate { .. })).count();
    assert_eq!(acts, deacts);
    // fragments start before their first message and end after their last
    assert!(ev.iter().position(|e| *e == Event::FragmentStart(0)).unwrap() < ev.iter().position(|e| *e == Event::Message(2)).unwrap());
    assert!(ev.iter().position(|e| *e == Event::FragmentEnd(0)).unwrap() > ev.iter().position(|e| *e == Event::Message(4)).unwrap());
    assert_eq!(sequence::fragment_depths(&sequence::timeline(&s, &actor)), vec![0, 1]);
}

#[test]
fn replies_to_earlier_calls_come_before_a_frame_opens() {
    let s = seq(serde_json::json!({"name": "x", "lifelines": [{"name": "a", "class": "A"}, {"name": "b", "class": "B"}, {"name": "c", "class": "C"}],
        "steps": [{"from": "a", "to": "b", "method": "find", "returns": "x"},
                  {"fragment": "loop", "guard": "each", "steps": [{"from": "a", "to": "c", "method": "run"}]}]}));
    let ev = tl(&s);
    let reply = ev.iter().position(|e| matches!(e, Event::Return { from, .. } if from == "b")).unwrap();
    let frame = ev.iter().position(|e| *e == Event::FragmentStart(0)).unwrap();
    assert!(reply < frame, "{ev:#?}");
}

#[test]
fn explicit_returns_are_not_doubled() {
    let s = seq(serde_json::json!({"name": "x", "lifelines": [{"name": "a", "class": "A"}, {"name": "b", "class": "B"}],
        "steps": [{"from": "a", "to": "b", "method": "run"}, {"from": "b", "to": "a", "method": "done", "kind": "return"}]}));
    let ev = tl(&s);
    assert!(returns(&ev).is_empty(), "{ev:?}");
    assert_eq!(ev.iter().filter(|e| matches!(e, Event::Deactivate { lifeline } if lifeline == "b")).count(), 1);
    // without the explicit return the reply is implied, with the call's value
    let s = seq(serde_json::json!({"name": "x", "lifelines": [{"name": "a", "class": "A"}, {"name": "b", "class": "B"}],
        "steps": [{"from": "a", "to": "b", "method": "run", "returns": "r"}]}));
    let ev = tl(&s);
    assert!(ev.iter().any(|e| matches!(e, Event::Return { from, label, .. } if from == "b" && label.as_deref() == Some("r"))));
}

#[test]
fn async_self_destroy_and_malformed_input() {
    let mut s = seq(serde_json::json!({"name": "x", "lifelines": [{"name": "a", "class": "A"}, {"name": "b", "class": "B"}],
        "steps": [{"from": "a", "to": "b", "method": "ping", "kind": "async"}, {"from": "b", "to": "b", "kind": "destroy"}]}));
    let ev = tl(&s);
    assert!(returns(&ev).iter().all(|(f, _)| f != "b"), "async receivers don't reply: {ev:?}");
    assert!(ev.contains(&Event::Destroy("b".into())));
    // crossing / out-of-range fragments and unknown lifelines are ignored, never panic
    s.fragments.push(Fragment { kind: FragmentKind::Loop, operands: vec![Operand { guard: None, first: 0, last: 9 }] });
    s.fragments.push(Fragment { kind: FragmentKind::Alt, operands: vec![Operand { guard: None, first: 1, last: 1 }, Operand { guard: None, first: 0, last: 0 }] });
    s.messages.push(Message { from: "ghost".into(), to: "a".into(), method: "boo".into(), ..Default::default() });
    let t = sequence::timeline(&s, &actor);
    assert!(t.fragments.is_empty());
    assert!(!t.events.contains(&Event::Message(2)));
}

#[test]
fn old_sequences_load_unchanged() {
    let s: Sequence = serde_json::from_str(r#"{"name":"old","participants":["Reader","Catalog"],"messages":[{"from":"Reader","to":"Catalog","method":"find"}]}"#).unwrap();
    assert!(s.lifelines.is_empty() && s.fragments.is_empty());
    assert_eq!(s.class_of("Catalog"), "Catalog");
    assert_eq!(s.lifeline("Catalog").label(false), ": Catalog");
    let json = serde_json::to_string(&s).unwrap();
    assert!(!json.contains("lifelines") && !json.contains("fragments"), "nothing new is written for old sequences");
}

#[test]
fn validation_and_rename_use_the_lifeline_class() {
    let mut store = Store::default();
    store.apply_user(serde_json::from_str(r#"[{"op":"add_class","name":"Loan","methods":[{"name":"close"}]},{"op":"add_class","name":"Svc"}]"#).unwrap()).unwrap();
    let s = seq(serde_json::json!({"name": "x", "lifelines": [{"name": "s", "class": "Svc"}, {"name": "l", "class": "Loan"}],
        "steps": [{"from": "s", "to": "l", "method": "close"}, {"from": "s", "to": "l", "method": "open"}, {"from": "s", "to": "l", "kind": "destroy"}]}));
    let problems = store.set_sequence(s).unwrap();
    assert_eq!(problems, vec!["message #2: Loan.open() does not exist".to_string()], "destroy isn't a method call");
    store.apply_user(serde_json::from_str(r#"[{"op":"rename_class","from":"Loan","to":"Lease"}]"#).unwrap()).unwrap();
    let s = &store.design.sequences[0];
    assert_eq!(s.class_of("l"), "Lease");
    assert_eq!(s.lifeline("l").label(false), "l : Lease");
}

#[test]
fn validation_matches_overloaded_methods() {
    let mut store = Store::default();
    store
        .apply_user(
            serde_json::from_str(
                r#"[
                    {"op":"add_class","name":"Caller"},
                    {"op":"add_class","name":"HeaderMasker","methods":[
                        {"name":"mask(Map<String,List<String>>)"},
                        {"name":"mask(String,String)"},
                        {"name":"maskValue"}
                    ]}
                ]"#,
            )
            .unwrap(),
        )
        .unwrap();
    let s = seq(serde_json::json!({
        "name": "SD3",
        "lifelines": [
            {"name": "c", "class": "Caller"},
            {"name": "hm", "class": "HeaderMasker"}
        ],
        "steps": [
            {"from": "c", "to": "hm", "method": "mask", "args": "headers"},
            {"from": "c", "to": "hm", "method": "mask(headers)"},
            {"from": "c", "to": "hm", "method": "mask(Map<String,List<String>>)"},
            {"from": "c", "to": "hm", "method": "mas"}
        ]
    }));
    let problems = store.set_sequence(s).unwrap();
    assert_eq!(problems, vec!["message #4: HeaderMasker.mas() does not exist".to_string()]);
    let stored = &store.design.sequences[0];
    assert!(stored.messages[0].problem.is_none());
    assert!(stored.messages[1].problem.is_none());
    assert!(stored.messages[2].problem.is_none());
    assert!(stored.messages[3].problem.is_some());

    // Pruned overloads must not match when no live overload remains
    store
        .prune(
            &[
                Target::Method { class: "HeaderMasker".into(), name: "mask(Map<String,List<String>>)".into() },
                Target::Method { class: "HeaderMasker".into(), name: "mask(String,String)".into() },
            ],
            None,
        )
        .unwrap();
    assert!(!store::has_method(&store.design, "HeaderMasker", "mask"));
}
