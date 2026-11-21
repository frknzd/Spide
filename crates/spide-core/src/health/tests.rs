use crate::{metrics::compute, Design};

#[test]
fn empty_design_has_no_findings_and_explicit_gaps() {
    let report = compute(&Design::default(), false).health_report;
    assert!(report.signals.is_empty());
    assert!(report.gaps.iter().any(|g| g.contains("No internal design")));
    assert!(report.runs.is_empty());
}
