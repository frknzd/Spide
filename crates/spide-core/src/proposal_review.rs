//! Isolated, non-mutating proposal review. Never replay onto the ghost view.
use crate::{Design, Op, OpError, Store};
use crate::ops::{apply_all, Mode};
use crate::model::Status;
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Serialize)]
pub struct ReviewChange { pub path: String, pub before: Value, pub after: Value }
#[derive(Debug, Serialize)]
pub struct ReviewStep { pub index: usize, pub changes: Vec<ReviewChange>, pub error: Option<String> }
#[derive(Debug, Serialize)]
pub struct ProposalReview {
    pub rev: u64,
    pub baseline: Design,
    pub design: Design,
    pub steps: Vec<ReviewStep>,
    pub errors: Vec<String>,
    pub prerequisites: Vec<String>,
    pub notices: Vec<String>,
    pub can_accept: bool,
    pub impact: Option<crate::impact::Impact>,
}

fn changes(before: &Design, after: &Design) -> Vec<ReviewChange> {
    let a = serde_json::to_value(before).unwrap();
    let b = serde_json::to_value(after).unwrap();
    let mut out = vec![];
    for (key, av) in a.as_object().unwrap() {
        if matches!(key.as_str(), "proposals" | "decisions" | "log" | "layout" | "assessments") { continue; }
        let bv = &b[key];
        if av == bv { continue; }
        if let (Some(aa), Some(bb)) = (av.as_array(), bv.as_array()) {
            let identity = |v: &Value| v.get("name").or_else(|| v.get("id")).and_then(Value::as_str).map(str::to_owned);
            if aa.iter().chain(bb).all(|v| identity(v).is_some()) {
                let mut names = std::collections::BTreeSet::new();
                for v in aa.iter().chain(bb) { names.insert(identity(v).unwrap()); }
                for name in names {
                    let x = aa.iter().find(|v| identity(v).as_deref() == Some(&name)).cloned().unwrap_or(Value::Null);
                    let y = bb.iter().find(|v| identity(v).as_deref() == Some(&name)).cloned().unwrap_or(Value::Null);
                    if x != y { out.push(ReviewChange { path: format!("{key}.{name}"), before: x, after: y }); }
                }
                continue;
            }
        }
        out.push(ReviewChange { path: key.clone(), before: av.clone(), after: bv.clone() });
    }
    out
}

impl Store {
    /// Replace one plan, never accepted elements. Validate before creating an undo entry.
    pub fn update_proposal(&mut self, id: &str, expected_rev: u64, ops: Vec<Op>, rationale: Option<String>, decision: Option<String>, option: Option<String>) -> Result<(), OpError> {
        if self.design.rev != expected_rev { return Err(OpError("Workspace changed. Reload the saved plan or reconcile your draft before saving.".into())); }
        if ops.is_empty() { return Err(OpError("Plan is empty. Reject the proposal instead.".into())); }
        self.validate_proposal_association(decision.as_deref(), option.as_deref())?;
        let review = self.review_proposal(id, Some(&ops))?;
        if !review.errors.is_empty() { return Err(OpError(review.errors.join("; "))); }
        let at = self.design.proposals.iter().position(|p| p.id == id).unwrap();
        let mut candidate = self.clone();
        candidate.design.proposals[at].ops = ops.clone();
        for p in &self.design.proposals[at + 1..] {
            let before = self.review_proposal(&p.id, None)?;
            let after = candidate.review_proposal(&p.id, None)?;
            if before.errors.is_empty() && !after.errors.is_empty() { return Err(OpError(format!("Would invalidate {}: {}. Repair dependent plans first.", p.id, after.errors.join("; ")))); }
        }
        self.mutate("user", format!("updated plan {id} (user amendment)"), |d| {
            let p = d.proposals.iter_mut().find(|p| p.id == id).unwrap();
            p.ops = ops; p.rationale = rationale; p.decision = decision; p.option = option;
            Ok(())
        })
    }

    pub fn validate_proposal_association(&self, decision: Option<&str>, option: Option<&str>) -> Result<(), OpError> {
        match decision {
            Some(id) => {
                let d = self.design.decisions.iter().find(|d| d.id == id).ok_or_else(|| OpError(format!("unknown decision {id}")))?;
                if let Some(o) = option { if !d.options.iter().any(|x| x.id == o) { return Err(OpError(format!("unknown option {o} for {id}"))); } }
            }
            None if option.is_some() => return Err(OpError("An option requires an associated decision.".into())),
            None => (),
        }
        Ok(())
    }

    pub fn review_proposal(&self, id: &str, draft: Option<&[Op]>) -> Result<ProposalReview, OpError> {
        let at = self.design.proposals.iter().position(|p| p.id == id).ok_or_else(|| OpError(format!("unknown proposal {id}")))?;
        let ops = draft.unwrap_or(&self.design.proposals[at].ops);
        let mut baseline = self.design.clone();
        let mut notices = vec![];
        for p in &self.design.proposals[..at] {
            match apply_all(&baseline, &p.ops, Mode::Ghost(&p.id)) {
                Ok(next) => baseline = next,
                Err(e) => notices.push(format!("{} is invalid: {}", p.id, e.0)),
            }
        }
        let mut design = baseline.clone();
        let mut steps = vec![];
        let mut errors = vec![];
        if ops.is_empty() { errors.push("Plan is empty. Add a change or reject the proposal.".into()); }
        for (index, op) in ops.iter().enumerate() {
            if !errors.is_empty() {
                steps.push(ReviewStep { index, changes: vec![], error: Some("Not evaluated: fix the earlier error first.".into()) });
                continue;
            }
            match apply_all(&design, std::slice::from_ref(op), Mode::Ghost(id)) {
                Ok(next) => { steps.push(ReviewStep { index, changes: changes(&design, &next), error: None }); design = next; }
                Err(e) => { errors.push(format!("Change {}: {}", index + 1, e.0)); steps.push(ReviewStep { index, changes: vec![], error: Some(e.0) }); }
            }
        }
        let standalone = apply_all(&self.design, ops, Mode::Commit(Status::Accepted, Some(id)));
        let mut prerequisites = vec![];
        if standalone.is_err() && errors.is_empty() {
            // Omit each earlier plan and replay, so transitive dependencies are visible too.
            for omitted in &self.design.proposals[..at] {
                let mut base = self.design.clone();
                for p in &self.design.proposals[..at] {
                    if p.id == omitted.id { continue; }
                    if let Ok(next) = apply_all(&base, &p.ops, Mode::Ghost(&p.id)) { base = next; }
                }
                if apply_all(&base, ops, Mode::Ghost(id)).is_err() { prerequisites.push(omitted.id.clone()); }
            }
            if prerequisites.is_empty() { notices.push("This plan requires earlier proposals to be accepted first.".into()); }
        }
        let impact = if errors.is_empty() { crate::impact::impact_of_ops(&baseline, ops).ok() } else { None };
        Ok(ProposalReview { rev: self.design.rev, baseline, design, steps, can_accept: errors.is_empty() && standalone.is_ok(), errors, prerequisites, notices, impact })
    }
}
