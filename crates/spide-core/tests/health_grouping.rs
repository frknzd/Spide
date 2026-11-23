use spide_core::{health::{report, Context}, metrics::{compute, Issue}, Design};
use serde_json::json;

#[test]
fn structural_responsibility_symptoms_share_one_charge_across_dimensions() {
    let d = Design::from_json(&json!({"classes":[{"name":"Processor"}]}).to_string()).unwrap();
    let mut m = compute(&d, false);
    m.classes.get_mut("Processor").unwrap().lcom = Some(3);
    m.issues = vec![Issue::new("god", 0.8, "oversized and split".into(), vec!["Processor".into()])];
    let baseline = report(&d, &m, Context::default());
    m.issues.push(Issue::new("cohesion", 0.4, "split method groups".into(), vec!["Processor".into()]));
    m.issues.push(Issue::new("large_class", 0.4, "too many members".into(), vec!["Processor".into()]));
    let grouped = report(&d, &m, Context::default());
    assert_eq!(grouped.score, baseline.score);
    assert_eq!(grouped.unique_signals().iter().filter(|s| s.root_cause == "responsibility:Processor").count(), 1);
    assert_eq!(grouped.signals.iter().filter(|s| s.root_cause == "responsibility:Processor").count(), 3);
    m.issues.push(Issue::new("feature_envy", 0.8, "independent foreign data use".into(), vec!["Processor".into()]));
    let independent = report(&d, &m, Context::default());
    assert!(independent.score < grouped.score);
}

#[test]
fn package_cycle_is_a_symptom_but_independent_class_cycles_are_not_merged() {
    let d = Design::from_json(&json!({"packages":[{"name":"left"},{"name":"right"}],"classes":[
        {"name":"A","package":"left"},{"name":"B","package":"right"},
        {"name":"C","package":"left"},{"name":"D","package":"right"}
    ],"relations":[{"from":"A","to":"B","kind":"dependency"},{"from":"B","to":"A","kind":"dependency"}]}).to_string()).unwrap();
    let mut m = compute(&d, false);
    let first = report(&d, &m, Context::default());
    assert_eq!(first.unique_signals().iter().filter(|s| s.rule == "cycle").count(), 1);
    m.cycles.push(vec!["C".into(), "D".into()]);
    m.issues.push(Issue::new("cycle", 0.9, "dependency cycle: C and D".into(), vec!["C".into(), "D".into()]));
    let independent = report(&d, &m, Context::default());
    assert_eq!(independent.unique_signals().iter().filter(|s| s.rule == "cycle" && s.targets.iter().all(|t| d.class(t).is_some())).count(), 2);
}
