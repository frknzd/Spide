mod common;
use common::golden;
use spide_core::*;

fn ops(json: &str) -> Vec<Op> {
    serde_json::from_str(json).expect("ops parse")
}

#[test]
fn adrs_for_decided_decisions() {
    let mut s = Store::default();
    s.design.name = "Library".into();
    s.apply_user(ops(r#"[{"op":"add_force","name":"Evolvability","kind":"quality","priority":1,"description":"new loan types each term"},
                         {"op":"add_force","name":"Simplicity","kind":"constraint"},
                         {"op":"add_class","name":"Loan","package":"domain"},{"op":"add_class","name":"Member","package":"domain"}]"#))
        .unwrap();
    let opts: Vec<DecisionOption> = serde_json::from_str(
        r#"[{"label":"Loan policy strategy","summary":"LoanPolicy interface with one class per loan type",
             "ops":[{"op":"add_class","name":"LoanPolicy","stereotype":"interface","package":"domain"},
                    {"op":"add_relation","from":"Loan","to":"LoanPolicy","kind":"association"}],
             "pros":["new loan types without touching Loan"],"cons":["one more abstraction"],
             "tradeoffs":[{"force":"Evolvability","effect":"+"},{"force":"Simplicity","effect":"-","note":"extra interface"}]},
            {"label":"Switch on loan type","ops":[{"op":"add_attribute","class":"Loan","attribute":{"name":"type","type":"String"}}],
             "tradeoffs":[{"force":"Simplicity","effect":"+"}]}]"#,
    )
    .unwrap();
    let d1 = s.propose_decision("How do we vary loan rules?".into(), Some("Each term brings a new loan type.".into()), opts).unwrap();
    s.approve_decision(&d1, s.design.rev, "New loan types only add a LoanPolicy class, serving Evolvability.".into()).unwrap();

    let opts: Vec<DecisionOption> = serde_json::from_str(r#"[{"label":"Member holds loans","ops":[{"op":"add_relation","from":"Member","to":"Loan","kind":"aggregation"}]}]"#).unwrap();
    let d2 = s.propose_decision("Who knows a member's loans?".into(), None, opts).unwrap();
    s.approve_decision(&d2, s.design.rev, String::new()).unwrap();
    // an open decision produces no ADR
    let opts: Vec<DecisionOption> = serde_json::from_str(r#"[{"label":"x","ops":[]}]"#).unwrap();
    s.propose_decision("Still open?".into(), None, opts).unwrap();

    for x in s.design.decisions.iter_mut() {
        if x.decided_at.is_some() {
            x.decided_at = Some(1_767_225_600_000); // 2026-01-01
        }
    }
    let out = adr::adrs(&s.design);
    assert_eq!(out.iter().map(|(f, _)| f.as_str()).collect::<Vec<_>>(), vec!["0001-how-do-we-vary-loan-rules.md", "0002-who-knows-a-member-s-loans.md"]);
    assert!(out[0].1.contains("Design preview"));
    golden("adr-0001.md", &out[0].1);
    golden("adr-0002.md", &out[1].1);
}
