//! Unified user-approved decisions. Proposals are internal editable plan storage only.
use crate::{Design, Store, OpError};
use crate::model::*;
use crate::ops::{apply_all, Mode};

pub fn reconcile(d: &mut Design) {
    let migrating = d.review_version == 0;
    d.review_version = 1;
    if migrating {
        for dec in &mut d.decisions {
            if dec.is_pending() { dec.state = DecisionState::Open; }
        }
    }
    // Preserve proposal order and IDs: downstream drafts may depend on them.
    for p in &mut d.proposals {
        let existing = d.decisions.iter().position(|x| x.is_pending() && x.plan.as_ref() == Some(&p.id))
            .or_else(|| p.decision.as_ref().and_then(|id| d.decisions.iter().position(|x| &x.id == id && x.is_pending() && x.plan.is_none())));
        let at = existing.unwrap_or_else(|| {
            let id = if d.decisions.iter().any(|x| x.id == p.id) { format!("decision-{}", p.id) } else { p.id.clone() }; // stable legacy alias
            d.decisions.push(Decision { id, question: p.rationale.clone().unwrap_or_else(|| crate::store::summarize(&p.ops)), context: Some(format!("Source: {}{}", p.source, p.decision.as_ref().map(|id| format!("; legacy context decision {id}")).unwrap_or_default())), ..Default::default() });
            d.decisions.len() - 1
        });
        let dec = &mut d.decisions[at];
        let option = p.option.clone().filter(|o| dec.options.iter().any(|x| &x.id == o))
            .or_else(|| dec.options.iter().find(|o| o.ops == p.ops).map(|o| o.id.clone()))
            .unwrap_or_else(|| {
                let id = format!("plan-{}", p.id);
                dec.options.push(DecisionOption { id: id.clone(), label: "Apply change".into(), ops: p.ops.clone(), ..Default::default() });
                id
            });
        if let Some(o) = dec.options.iter_mut().find(|o| o.id == option) { o.ops = p.ops.clone(); }
        dec.plan = Some(p.id.clone());
        dec.chosen = Some(option.clone());
        p.decision = Some(dec.id.clone()); p.option = Some(option);
    }
    let missing: Vec<_> = d.decisions.iter().filter(|x| x.is_pending() && x.plan.is_none() && !x.options.is_empty()).map(|x| x.id.clone()).collect();
    for id in missing {
        let at = d.decisions.iter().position(|x| x.id == id).unwrap();
        let option = d.decisions[at].options.iter().find(|o| Some(&o.id) == d.decisions[at].chosen.as_ref()).unwrap_or(&d.decisions[at].options[0]).clone();
        let pid = d.next_id("P");
        d.decisions[at].plan = Some(pid.clone()); d.decisions[at].chosen = Some(option.id.clone());
        d.proposals.push(Proposal { id: pid, ops: option.ops, rationale: d.decisions[at].context.clone(), source: "agent".into(), decision: Some(id), option: Some(option.id) });
    }
}

impl Store {
    pub fn select_decision_option(&mut self, id: &str, option: &str, expected_rev: u64) -> Result<(), OpError> {
        if expected_rev != self.design.rev { return Err(OpError("Workspace changed; review again.".into())); }
        let dec = self.design.decisions.iter().find(|d| d.id == id && d.is_pending()).ok_or_else(|| OpError("Decision is closed or missing".into()))?.clone();
        let o = dec.options.iter().find(|o| o.id == option).ok_or_else(|| OpError("Unknown option".into()))?.clone();
        let pid = dec.plan.ok_or_else(|| OpError("Missing plan".into()))?;
        let p = self.design.proposals.iter().find(|p| p.id == pid).unwrap().clone();
        self.update_proposal(&pid, expected_rev, o.ops, p.rationale, Some(id.into()), Some(option.into()))
    }

    pub fn approve_decision(&mut self, id: &str, expected_rev: u64, notes: String) -> Result<(), OpError> {
        if expected_rev != self.design.rev { return Err(OpError("Workspace changed; review again.".into())); }
        let dec = self.design.decisions.iter().find(|d| d.id == id && d.is_pending()).ok_or_else(|| OpError("Decision is closed or missing".into()))?.clone();
        let pid = dec.plan.ok_or_else(|| OpError("Missing concrete plan".into()))?;
        let p = self.design.proposals.iter().find(|p| p.id == pid).ok_or_else(|| OpError("Missing saved plan".into()))?.clone();
        let review = self.review_proposal(&pid, None)?;
        if !review.can_accept { return Err(OpError(format!("Resolve errors/prerequisites first: {:?} {:?}", review.errors, review.prerequisites))); }
        let impact = crate::impact::impact_of_ops(&self.design, &p.ops).ok();
        self.mutate("user", format!("approved design {id}"), |d| {
            let mut next = apply_all(d, &p.ops, Mode::Commit(Status::Accepted, Some(id)))?;
            next.proposals.retain(|p| p.id != pid);
            let dec = next.decisions.iter_mut().find(|d| d.id == id).unwrap();
            dec.state = DecisionState::Accepted; dec.notes = notes; dec.applied_ops = p.ops; dec.consequences = impact; dec.decided_at = Some(crate::store::now_ms());
            *d = next; Ok(())
        })
    }
}
