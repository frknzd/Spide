//! The agent-built conceptual domain model: separate from the classes, stale when they change.
use spide_core::derive::derive;
use spide_core::metrics::compute;
use spide_core::store::Target;
use spide_core::*;

fn ops(json: &str) -> Vec<Op> {
    serde_json::from_str(json).expect("ops parse")
}

fn store() -> Store {
    let mut s = Store::default();
    s.apply_user(ops(r#"[{"op":"add_class","name":"LoanService","stereotype":"service"},{"op":"add_class","name":"LoanEntity"}]"#)).unwrap();
    s
}

fn model(json: serde_json::Value) -> DomainModel {
    serde_json::from_value(json).unwrap()
}

fn sample() -> DomainModel {
    model(serde_json::json!({
        "concepts": [
            {"name": "Member", "description": "may borrow", "attributes": [{"name": "name", "type": "String"}]},
            {"name": "Loan", "attributes": [{"name": "due", "type": "Date"}]},
            {"name": "Loan"},
            {"name": "  "}
        ],
        "associations": [
            {"from": "Member", "to": "Loan", "label": "holds", "from_mult": "1", "to_mult": "0..5"},
            {"from": "Member", "to": "Loan", "label": "returned"},
            {"from": "Member", "to": "Ghost"}
        ]
    }))
}

fn stale(s: &Store) -> bool {
    let v = s.view();
    derive(&v, &compute(&v, false)).domain_stale
}

#[test]
fn set_marks_fresh_and_class_changes_make_it_stale() {
    let mut s = store();
    assert!(stale(&s), "classes exist, no domain model yet");
    let w = s.set_domain_model(sample()).unwrap();
    assert!(w.iter().any(|w| w.contains("duplicate concept Loan")), "{w:?}");
    assert!(w.iter().any(|w| w.contains("Ghost")), "{w:?}");
    assert!(w.iter().any(|w| w.contains("without a name")), "{w:?}");
    let d = &s.design.domain;
    assert_eq!(d.concepts.len(), 2);
    let ids: Vec<&str> = d.associations.iter().map(|a| a.id.as_str()).collect();
    assert_eq!(ids, vec!["Member-association-Loan", "Member-association-Loan-2"]);
    assert!(!stale(&s));
    // the concept names are independent of the classes; only the class model drives staleness
    s.apply_user(ops(r#"[{"op":"rename_class","from":"LoanEntity","to":"Loan"}]"#)).unwrap();
    assert!(stale(&s));
}

#[test]
fn pruned_items_and_feedback_survive_a_later_set() {
    let mut s = store();
    s.set_domain_model(sample()).unwrap();
    s.prune(&[Target::Concept { name: "Loan".into() }], Some("not a concept here".into())).unwrap();
    let loan = s.design.domain.concepts.iter().find(|c| c.name == "Loan").unwrap();
    assert_eq!(loan.status, Status::Pruned);
    assert!(s.design.domain.associations.iter().all(|a| a.status == Status::Pruned), "its associations follow");
    // the design's classes are untouched
    assert!(s.design.classes.iter().all(|c| c.status == Status::Accepted));
    // a new model without Loan keeps the pruned one (and its feedback)
    s.set_domain_model(model(serde_json::json!({"concepts": [{"name": "Member"}]}))).unwrap();
    let loan = s.design.domain.concepts.iter().find(|c| c.name == "Loan").unwrap();
    assert_eq!((loan.status, loan.feedback.as_deref()), (Status::Pruned, Some("not a concept here")));
    // the view hides it; restoring brings back the concept
    assert!(!s.view().domain.concepts.iter().any(|c| c.name == "Loan"));
    s.restore(&Target::Concept { name: "Loan".into() }).unwrap();
    assert!(s.view().domain.concepts.iter().any(|c| c.name == "Loan"));
}

#[test]
fn old_designs_without_a_domain_model_load() {
    let d = Design::from_json(r#"{"name":"old","classes":[{"name":"A"}]}"#).unwrap();
    assert!(d.domain.is_empty());
    assert!(d.domain.generated_rev.is_none());
}

#[test]
fn tool_sets_and_reads_the_domain_model() {
    let mut s = store();
    let names: Vec<String> = tools::tool_list().as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect();
    assert!(names.contains(&"set_domain_model".to_string()));
    let out = tools::call(&mut s, "set_domain_model", &serde_json::json!({
        "concepts": [{"name": "Endpoint", "attributes": [{"name": "path", "type": "String"}]}, {"name": "ApiCall"}],
        "associations": [{"from": "ApiCall", "to": "Endpoint", "label": "targets", "to_mult": "1"}]
    }))
    .unwrap();
    assert!(out.changed, "{}", out.text);
    let v: serde_json::Value = serde_json::from_str(&tools::call(&mut s, "get_view", &serde_json::json!({"kind": "domain"})).unwrap().text).unwrap();
    assert_eq!(v["concepts"][0]["name"], "Endpoint");
    assert_eq!(v["concepts"][0]["attributes"][0]["type"], "String");
    let g: serde_json::Value = serde_json::from_str(&tools::call(&mut s, "get_design", &serde_json::json!({})).unwrap().text).unwrap();
    assert_eq!(g["domain_model"]["stale"], false);
    assert_eq!(g["domain_model"]["concepts"].as_array().unwrap().len(), 2);
}

#[test]
fn drawing_model_maps_generalization_to_inheritance() {
    let m = model(serde_json::json!({"concepts": [{"name": "A", "description": "x"}, {"name": "B"}],
        "associations": [{"id": "B-generalization-A", "from": "B", "to": "A", "kind": "generalization"}]}));
    let (classes, rels) = m.as_class_model();
    assert_eq!(classes[0].responsibilities, vec!["x".to_string()]);
    assert_eq!(rels[0].kind, RelationKind::Inheritance);
}
