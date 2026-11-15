use super::*;
use crate::ops::{apply_all, Mode, Op};

pub(crate) fn design(ops: &str) -> Design {
    let ops: Vec<Op> = serde_json::from_str(ops).unwrap();
    apply_all(&Design::default(), &ops, Mode::Commit(Status::Accepted, None)).unwrap()
}

#[test]
fn empty_design_is_unassessed_not_perfect() {
    let m = crate::metrics::compute(&Design::default(), false);
    assert_eq!(m.health_report.score, None);
    assert_eq!(m.health_report.measured, None);
}

#[test]
fn design_only_is_provisional_and_implementation_quality_unknown() {
    let d = design(r#"[{"op":"add_class","name":"A","methods":[{"name":"run","calls":["B.go"]}]},{"op":"add_class","name":"B","methods":[{"name":"go"}]},{"op":"add_relation","from":"A","to":"B","kind":"dependency"}]"#);
    let r = crate::metrics::compute(&d, false).health_report;
    assert!(r.score.is_some() && r.provisional);
    assert_eq!(r.dimensions["implementation_quality"].measured, None);
    assert!(!r.dimensions["implementation_quality"].gaps.is_empty());
}
