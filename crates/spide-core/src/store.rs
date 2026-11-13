//! DesignStore: owns the design, the undo/redo history, proposals and decisions.

use crate::model::*;
use crate::ops::{apply_all, Mode, Op, OpError};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

const UNDO_LIMIT: usize = 200;

pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Something the user can point at (prune, restore, feedback).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Target {
    Class { name: String },
    Attribute { class: String, name: String },
    Method { class: String, name: String },
    Relation { id: String },
    Package { name: String },
    Actor { name: String },
    UseCase { name: String },
    Sequence { name: String },
    Message { sequence: String, index: usize },
    Proposal { id: String },
    Force { name: String },
    System { name: String },
    Container { name: String },
    Link { id: String },
}

impl Target {
    pub fn label(&self) -> String {
        match self {
            Target::Class { name } => format!("class {name}"),
            Target::Attribute { class, name } => format!("attribute {class}.{name}"),
            Target::Method { class, name } => format!("method {class}.{name}()"),
            Target::Relation { id } => format!("relation {id}"),
            Target::Package { name } => format!("package {name}"),
            Target::Actor { name } => format!("actor {name}"),
            Target::UseCase { name } => format!("use case \"{name}\""),
            Target::Sequence { name } => format!("sequence \"{name}\""),
            Target::Message { sequence, index } => format!("message #{} in \"{sequence}\"", index + 1),
            Target::Proposal { id } => format!("proposal {id}"),
            Target::Force { name } => format!("force \"{name}\""),
            Target::System { name } => format!("external system {name}"),
            Target::Container { name } => format!("container {name}"),
            Target::Link { id } => format!("link {id}"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Store {
    pub design: Design,
    undo: Vec<Design>,
    redo: Vec<Design>,
}

impl Default for Store {
    fn default() -> Self {
        Store::new(Design::default())
    }
}

impl Store {
    pub fn new(design: Design) -> Self {
        Store { design, undo: vec![], redo: vec![] }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Transactional change: `f` works on a copy; on success it becomes current.
    pub fn mutate<T>(
        &mut self,
        actor: &str,
        summary: impl Into<String>,
        f: impl FnOnce(&mut Design) -> Result<T, OpError>,
    ) -> Result<T, OpError> {
        let mut next = self.design.clone();
        let out = f(&mut next)?;
        let model_changed = next.classes != self.design.classes
            || next.relations != self.design.relations
            || next.packages != self.design.packages;
        next.rev = self.design.rev + 1;
        if model_changed {
            next.model_rev = next.rev;
        }
        next.log.push(LogEntry { rev: next.rev, at: now_ms(), actor: actor.into(), summary: summary.into() });
        let prev = std::mem::replace(&mut self.design, next);
        self.undo.push(prev);
        if self.undo.len() > UNDO_LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
        Ok(out)
    }

    pub fn undo(&mut self) -> bool {
        let Some(prev) = self.undo.pop() else { return false };
        let cur = self.swap_in(prev);
        self.redo.push(cur);
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(next) = self.redo.pop() else { return false };
        let cur = self.swap_in(next);
        self.undo.push(cur);
        true
    }

    /// Swap in a history state, keeping layout + settings, bumping the revision.
    fn swap_in(&mut self, mut d: Design) -> Design {
        d.layout = self.design.layout.clone();
        d.settings = self.design.settings.clone();
        d.rev = self.design.rev + 1;
        d.model_rev = d.rev;
        std::mem::replace(&mut self.design, d)
    }

    /// Replace everything (load). Clears history.
    pub fn load(&mut self, mut design: Design) {
        design.rev = design.rev.max(1);
        design.version = SCHEMA_VERSION;
        self.design = design;
        self.undo.clear();
        self.redo.clear();
    }

    /// Direct user edits (inline edits, brief panel): committed immediately, undoable.
    pub fn apply_user(&mut self, ops: Vec<Op>) -> Result<(), OpError> {
        if ops.is_empty() {
            return Ok(());
        }
        let summary = summarize(&ops);
        self.mutate("user", summary, |d| {
            *d = apply_all(d, &ops, Mode::Commit(Status::Accepted, Some("user")))?;
            Ok(())
        })
    }

    /// Layout changes don't create undo entries or new revisions.
    pub fn set_layout(&mut self, view: &str, positions: std::collections::BTreeMap<String, Pos>) {
        self.design.layout.insert(view.to_string(), positions);
    }

    // ------------------------------------------------------------ view

    /// Design with all pending proposals applied as ghosts. Proposals that no
    /// longer apply are skipped.
    pub fn view(&self) -> Design {
        let mut v = self.design.clone();
        if !v.settings.show_pruned {
            strip_pruned(&mut v);
        }
        for p in &self.design.proposals {
            if let Ok(next) = apply_all(&v, &p.ops, Mode::Ghost(&p.id)) {
                v = next;
            }
        }
        v
    }

    /// Preview: current design + ops applied as ghosts (for decision options).
    pub fn preview(&self, ops: &[Op], tag: &str) -> Result<Design, OpError> {
        let mut base = self.design.clone();
        strip_pruned(&mut base);
        apply_all(&base, ops, Mode::Ghost(tag))
    }

    // ------------------------------------------------------------ proposals

    pub fn propose_ops(&mut self, ops: Vec<Op>, rationale: Option<String>, source: &str) -> Result<String, OpError> {
        if ops.is_empty() {
            return Err(OpError("no ops".into()));
        }
        // validate against design + other pending proposals (they may build on each other)
        let base = self.view();
        apply_all(&base, &ops, Mode::Ghost("check"))?;
        let summary = summarize(&ops);
        self.mutate(source, format!("proposed: {summary}"), |d| {
            let id = d.next_id("P");
            d.proposals.push(Proposal { id: id.clone(), ops, rationale, source: source.to_string() });
            Ok(id)
        })
    }

    pub fn accept_proposal(&mut self, id: &str) -> Result<(), OpError> {
        let p = self
            .design
            .proposals
            .iter()
            .find(|p| p.id == id)
            .cloned()
            .ok_or_else(|| OpError(format!("unknown proposal {id}")))?;
        let summary = summarize(&p.ops);
        self.mutate("user", format!("accepted {id}: {summary}"), |d| {
            let mut next = apply_all(d, &p.ops, Mode::Commit(Status::Accepted, Some(&p.id)))?;
            if let Some(r) = p.rationale.as_ref().filter(|_| p.source == "agent") {
                for name in p.ops.iter().flat_map(|o| match o {
                    Op::AddClass { name, rationale: None, .. } => Some(name.clone()),
                    _ => None,
                }) {
                    if let Some(c) = next.class_mut(&name) {
                        c.rationale.get_or_insert_with(|| r.clone());
                    }
                }
            }
            next.proposals.retain(|x| x.id != p.id);
            *d = next;
            Ok(())
        })
    }

    pub fn accept_all_proposals(&mut self) -> Result<usize, OpError> {
        let ids: Vec<String> = self.design.proposals.iter().map(|p| p.id.clone()).collect();
        let mut n = 0;
        for id in ids {
            if self.accept_proposal(&id).is_ok() {
                n += 1;
            }
        }
        Ok(n)
    }

    pub fn reject_proposal(&mut self, id: &str, feedback: Option<String>) -> Result<(), OpError> {
        if !self.design.proposals.iter().any(|p| p.id == id) {
            return Err(OpError(format!("unknown proposal {id}")));
        }
        self.mutate("user", format!("rejected {id}"), |d| {
            d.proposals.retain(|p| p.id != id);
            if let Some(f) = feedback.filter(|f| !f.trim().is_empty()) {
                d.feedback.push(Feedback { target: format!("proposal {id}"), text: f, at: now_ms() });
            }
            Ok(())
        })
    }

    // ------------------------------------------------------------ decisions

    pub fn propose_decision(
        &mut self,
        question: String,
        context: Option<String>,
        mut options: Vec<DecisionOption>,
    ) -> Result<String, OpError> {
        if options.is_empty() {
            return Err(OpError("a decision needs at least one option".into()));
        }
        if options.len() > 4 {
            return Err(OpError("keep it to 2–4 options".into()));
        }
        for (i, o) in options.iter_mut().enumerate() {
            if o.id.is_empty() {
                o.id = ((b'A' + i as u8) as char).to_string();
            }
            self.preview(&o.ops, "check").map_err(|e| OpError(format!("option {} invalid: {}", o.id, e.0)))?;
        }
        self.mutate("agent", format!("decision: {question}"), |d| {
            let id = d.next_id("D");
            d.decisions.push(Decision { id: id.clone(), question, context, options, ..Default::default() });
            Ok(id)
        })
    }

    /// User picks an option (or writes a custom one) and defends it.
    pub fn submit_defense(
        &mut self,
        id: &str,
        option: Option<String>,
        custom: Option<String>,
        defense: String,
    ) -> Result<(), OpError> {
        let dec = self.decision(id)?;
        if !dec.is_pending() {
            return Err(OpError(format!("decision {id} is closed")));
        }
        if dec.state == DecisionState::Evaluating {
            return Err(OpError("the examiner is still evaluating".into()));
        }
        if let Some(o) = &option {
            if !dec.options.iter().any(|x| &x.id == o) {
                return Err(OpError(format!("unknown option {o}")));
            }
        }
        if option.is_none() && custom.as_deref().is_none_or(|c| c.trim().is_empty()) {
            return Err(OpError("choose an option or describe your own".into()));
        }
        if defense.trim().len() < 3 {
            return Err(OpError("you need to defend your choice".into()));
        }
        self.mutate("user", format!("defended {id}"), |d| {
            let dec = d.decisions.iter_mut().find(|x| x.id == id).unwrap();
            dec.state = DecisionState::Evaluating;
            dec.chosen = option.clone();
            dec.custom = custom.clone();
            dec.attempts.push(Attempt { option, custom, defense, verdict: None, feedback: None });
            Ok(())
        })
    }

    /// Examiner verdict. For custom answers the examiner must supply the ops.
    pub fn evaluate_defense(
        &mut self,
        id: &str,
        accept: bool,
        feedback: String,
        ops: Option<Vec<Op>>,
    ) -> Result<(), OpError> {
        let dec = self.decision(id)?.clone();
        if dec.state != DecisionState::Evaluating {
            return Err(OpError(format!("decision {id} is not awaiting evaluation (state {:?})", dec.state)));
        }
        let ops = match (ops, &dec.chosen) {
            (Some(o), _) if !o.is_empty() => o,
            (_, Some(opt)) => dec.options.iter().find(|o| &o.id == opt).map(|o| o.ops.clone()).unwrap_or_default(),
            _ if accept => {
                return Err(OpError("custom answer accepted: pass the ops that implement it".into()));
            }
            _ => vec![],
        };
        let verdict = if accept { "accept" } else { "reject" };
        let consequences = if accept { crate::impact::impact_of_ops(&self.design, &ops).ok() } else { None };
        self.mutate("examiner", format!("{verdict}ed defence for {id}"), |d| {
            let mut next = if accept { apply_all(d, &ops, Mode::Commit(Status::Accepted, Some(id)))? } else { d.clone() };
            let dec = next.decisions.iter_mut().find(|x| x.id == id).unwrap();
            if let Some(a) = dec.attempts.last_mut() {
                a.verdict = Some(verdict.into());
                a.feedback = Some(feedback.clone());
            }
            if accept {
                dec.state = DecisionState::Accepted;
                dec.applied_ops = ops.clone();
                dec.consequences = consequences.clone();
                dec.decided_at = Some(now_ms());
            } else {
                dec.state = DecisionState::Rejected;
                // remember ops for a possible overrule of a custom answer
                if dec.chosen.is_none() && !ops.is_empty() {
                    dec.applied_ops = ops.clone();
                }
            }
            *d = next;
            Ok(())
        })
    }

    /// Lenient mode: apply despite rejection; elements are marked overruled.
    pub fn overrule(&mut self, id: &str) -> Result<(), OpError> {
        if self.design.settings.examiner == ExaminerMode::Strict {
            return Err(OpError("strict examiner: revise your defence instead".into()));
        }
        let dec = self.decision(id)?.clone();
        if dec.state != DecisionState::Rejected {
            return Err(OpError("only rejected decisions can be overruled".into()));
        }
        let ops = match &dec.chosen {
            Some(o) => dec.options.iter().find(|x| &x.id == o).map(|x| x.ops.clone()).unwrap_or_default(),
            None => dec.applied_ops.clone(),
        };
        if ops.is_empty() {
            return Err(OpError("nothing concrete to apply: the examiner gave no ops for your custom answer".into()));
        }
        let consequences = crate::impact::impact_of_ops(&self.design, &ops).ok();
        self.mutate("user", format!("overruled examiner on {id}"), |d| {
            let mut next = apply_all(d, &ops, Mode::Commit(Status::Overruled, Some(id)))?;
            let dec = next.decisions.iter_mut().find(|x| x.id == id).unwrap();
            dec.state = DecisionState::Overruled;
            dec.applied_ops = ops.clone();
            dec.consequences = consequences.clone();
            dec.decided_at = Some(now_ms());
            *d = next;
            Ok(())
        })
    }

    /// Evaluating decisions without verdict → Rejected with a note. Returns how many.
    pub fn release_stuck_evaluations(&mut self) -> usize {
        let n = self.design.decisions.iter().filter(|d| d.state == DecisionState::Evaluating).count();
        if n == 0 {
            return 0;
        }
        let _ = self.mutate("spide", "examiner gave no verdict", |d| {
            for dec in d.decisions.iter_mut().filter(|x| x.state == DecisionState::Evaluating) {
                dec.state = DecisionState::Rejected;
                if let Some(a) = dec.attempts.last_mut() {
                    a.verdict = Some("none".into());
                    a.feedback = Some("The examiner gave no verdict (agent problem). Defend again.".into());
                }
            }
            Ok(())
        });
        n
    }

    pub fn dismiss_decision(&mut self, id: &str) -> Result<(), OpError> {
        self.decision(id)?;
        self.mutate("user", format!("dismissed {id}"), |d| {
            d.decisions.iter_mut().find(|x| x.id == id).unwrap().state = DecisionState::Dismissed;
            Ok(())
        })
    }

    fn decision(&self, id: &str) -> Result<&Decision, OpError> {
        self.design.decisions.iter().find(|d| d.id == id).ok_or_else(|| OpError(format!("unknown decision {id}")))
    }

    // ------------------------------------------------------------ prune / restore

    pub fn prune(&mut self, targets: &[Target], feedback: Option<String>) -> Result<Vec<String>, OpError> {
        let fb = feedback.filter(|f| !f.trim().is_empty());
        let labels: Vec<String> = targets.iter().map(Target::label).collect();
        self.mutate("user", format!("pruned {}", labels.join(", ")), |d| {
            for t in targets {
                if let Target::Proposal { id } = t {
                    d.proposals.retain(|p| &p.id != id);
                } else {
                    set_status(d, t, Status::Pruned, fb.clone())?;
                }
                if let Some(f) = &fb {
                    d.feedback.push(Feedback { target: t.label(), text: f.clone(), at: now_ms() });
                }
            }
            Ok(labels.clone())
        })
    }

    pub fn restore(&mut self, target: &Target) -> Result<(), OpError> {
        self.mutate("user", format!("restored {}", target.label()), |d| set_status(d, target, Status::Accepted, None))
    }

    // ------------------------------------------------------------ derived (agent-generated)

    pub fn set_use_cases(&mut self, mut m: UseCaseModel) -> Result<Vec<String>, OpError> {
        let mut warnings = vec![];
        for uc in &mut m.use_cases {
            uc.classes.retain(|c| {
                let ok = self.design.live_class(c).is_some();
                if !ok {
                    warnings.push(format!("use case \"{}\": unknown class {c} dropped", uc.name));
                }
                ok
            });
            for a in &uc.actors {
                if !m.actors.iter().any(|x| &x.name == a) {
                    warnings.push(format!("use case \"{}\": actor {a} not declared", uc.name));
                }
            }
        }
        let rev = self.design.model_rev;
        m.generated_rev = Some(rev);
        // keep pruned entries (and their feedback) so the agent doesn't re-add them silently
        self.mutate("agent", "use case diagram updated", move |d| {
            let old = std::mem::take(&mut d.use_cases);
            for a in old.actors.into_iter().filter(|a| a.status == Status::Pruned) {
                if !m.actors.iter().any(|x| x.name == a.name) {
                    m.actors.push(a);
                }
            }
            for u in old.use_cases.into_iter().filter(|u| u.status == Status::Pruned) {
                if !m.use_cases.iter().any(|x| x.name == u.name) {
                    m.use_cases.push(u);
                }
            }
            d.use_cases = m;
            Ok(())
        })?;
        Ok(warnings)
    }

    pub fn set_sequence(&mut self, mut s: Sequence) -> Result<Vec<String>, OpError> {
        if s.name.trim().is_empty() {
            return Err(OpError("sequence needs a name".into()));
        }
        let problems = validate_sequence(&self.design, &mut s);
        s.generated_rev = Some(self.design.model_rev);
        s.status = Status::Accepted;
        self.mutate("agent", format!("sequence \"{}\" updated", s.name), move |d| {
            d.sequences.retain(|x| x.name != s.name);
            d.sequences.push(s);
            Ok(())
        })?;
        Ok(problems)
    }

    /// Agent analysis of a change scenario (upsert). Unknown classes are dropped with a warning.
    pub fn set_change_impact(&mut self, name: String, description: Option<String>, impacted: Vec<ImpactedClass>) -> Result<Vec<String>, OpError> {
        if name.trim().is_empty() {
            return Err(OpError("change scenario needs a name".into()));
        }
        let mut warnings = vec![];
        let impacted: Vec<ImpactedClass> = impacted
            .into_iter()
            .filter(|i| {
                let ok = self.design.live_class(&i.class).is_some();
                if !ok {
                    warnings.push(format!("unknown class {} dropped", i.class));
                }
                ok
            })
            .collect();
        let rev = self.design.model_rev;
        self.mutate("agent", format!("change scenario \"{name}\" analysed"), move |d| {
            match d.change_scenarios.iter_mut().find(|c| c.name == name && c.status != Status::Pruned) {
                Some(c) => {
                    c.impacted = impacted;
                    c.generated_rev = Some(rev);
                    if description.is_some() {
                        c.description = description;
                    }
                }
                None => d.change_scenarios.push(ChangeScenario {
                    name,
                    description,
                    impacted,
                    generated_rev: Some(rev),
                    ..Default::default()
                }),
            }
            Ok(())
        })?;
        Ok(warnings)
    }

    pub fn set_examiner(&mut self, mode: ExaminerMode) {
        self.design.settings.examiner = mode;
    }
    pub fn set_show_pruned(&mut self, on: bool) {
        self.design.settings.show_pruned = on;
    }
    pub fn set_detail(&mut self, view: &str, level: u8) {
        self.design.settings.detail.insert(view.to_string(), level.min(4));
    }
    /// Architecture rules are user-owned: changed directly (undoable), never proposed.
    pub fn set_rules(&mut self, rules: crate::rules::Rules) -> Result<(), OpError> {
        if rules == self.design.rules {
            return Ok(());
        }
        let summary = format!("rules: {} preset, {} custom", rules.preset.as_str(), rules.forbidden.len() + rules.forbidden_types.len());
        self.mutate("user", summary, move |d| {
            d.rules = rules;
            Ok(())
        })
    }

    pub fn set_partner(&mut self, mode: PartnerMode) {
        self.design.settings.partner = mode;
    }
    pub fn set_code_root(&mut self, root: Option<String>) {
        self.design.settings.code_root = root.map(|r| r.trim().to_string()).filter(|r| !r.is_empty());
    }
    pub fn set_brief_skipped(&mut self, on: bool) {
        self.design.settings.brief_skipped = on;
    }
}

pub fn summarize(ops: &[Op]) -> String {
    let parts: Vec<String> = ops.iter().take(4).map(Op::summary).collect();
    let more = if ops.len() > 4 { format!(" (+{} more)", ops.len() - 4) } else { String::new() };
    format!("{}{more}", parts.join("; "))
}

fn strip_pruned(d: &mut Design) {
    d.classes.retain(|c| c.status != Status::Pruned);
    for c in d.classes.iter_mut() {
        c.attributes.retain(|a| a.status != Status::Pruned);
        c.methods.retain(|m| m.status != Status::Pruned);
    }
    let names: Vec<String> = d.classes.iter().map(|c| c.name.clone()).collect();
    d.relations.retain(|r| r.status != Status::Pruned && names.contains(&r.from) && names.contains(&r.to));
    d.packages.retain(|p| p.status != Status::Pruned);
    d.forces.retain(|f| f.status != Status::Pruned);
    d.change_scenarios.retain(|c| c.status != Status::Pruned);
    d.systems.retain(|x| x.status != Status::Pruned);
    d.containers.retain(|x| x.status != Status::Pruned);
    let nodes: Vec<String> = d
        .systems
        .iter()
        .map(|x| x.name.clone())
        .chain(d.containers.iter().map(|x| x.name.clone()))
        .chain(d.use_cases.actors.iter().filter(|a| a.status != Status::Pruned).map(|a| a.name.clone()))
        .collect();
    d.links.retain(|l| l.status != Status::Pruned && nodes.contains(&l.from) && nodes.contains(&l.to));
}

fn set_status(d: &mut Design, t: &Target, st: Status, fb: Option<String>) -> Result<(), OpError> {
    let nf = || OpError(format!("{} not found", t.label()));
    match t {
        Target::Class { name } => {
            let c = d.class_mut(name).ok_or_else(nf)?;
            c.status = st;
            c.feedback = fb;
        }
        Target::Attribute { class, name } => {
            let c = d.class_mut(class).ok_or_else(nf)?;
            c.attributes.iter_mut().find(|a| &a.name == name).ok_or_else(nf)?.status = st;
        }
        Target::Method { class, name } => {
            let c = d.class_mut(class).ok_or_else(nf)?;
            c.methods.iter_mut().find(|m| &m.name == name).ok_or_else(nf)?.status = st;
        }
        Target::Relation { id } => {
            let r = d.relations.iter_mut().find(|r| &r.id == id).ok_or_else(nf)?;
            r.status = st;
            r.feedback = fb;
        }
        Target::Package { name } => {
            d.packages.iter_mut().find(|p| &p.name == name).ok_or_else(nf)?.status = st;
            // pruning a package prunes its classes; restoring it brings them back
            let affected = |c: &Class| match st {
                Status::Pruned => c.status.is_live(),
                _ => c.status == Status::Pruned,
            };
            for c in d.classes.iter_mut().filter(|c| c.package.as_deref() == Some(name.as_str()) && affected(c)) {
                c.status = st;
                c.feedback = fb.clone();
            }
        }
        Target::Actor { name } => {
            let a = d.use_cases.actors.iter_mut().find(|a| &a.name == name).ok_or_else(nf)?;
            a.status = st;
            a.feedback = fb;
        }
        Target::UseCase { name } => {
            let u = d.use_cases.use_cases.iter_mut().find(|u| &u.name == name).ok_or_else(nf)?;
            u.status = st;
            u.feedback = fb;
        }
        Target::Sequence { name } => {
            let s = d.sequences.iter_mut().find(|s| &s.name == name).ok_or_else(nf)?;
            s.status = st;
            s.feedback = fb;
        }
        Target::Message { sequence, index } => {
            let s = d.sequences.iter_mut().find(|s| &s.name == sequence).ok_or_else(nf)?;
            let m = s.messages.get_mut(*index).ok_or_else(nf)?;
            m.status = st;
            m.feedback = fb;
        }
        Target::Force { name } => {
            d.forces.iter_mut().find(|f| &f.name == name).ok_or_else(nf)?.status = st;
        }
        Target::System { name } => {
            d.systems.iter_mut().find(|x| &x.name == name).ok_or_else(nf)?.status = st;
        }
        Target::Container { name } => {
            d.containers.iter_mut().find(|x| &x.name == name).ok_or_else(nf)?.status = st;
        }
        Target::Link { id } => {
            d.links.iter_mut().find(|l| &l.id() == id).ok_or_else(nf)?.status = st;
        }
        Target::Proposal { .. } => {}
    }
    Ok(())
}

/// Ancestors via inheritance/realization (for method lookup).
pub fn supertypes(d: &Design, class: &str) -> Vec<String> {
    let mut out = vec![];
    let mut stack = vec![class.to_string()];
    while let Some(c) = stack.pop() {
        for r in d.live_relations() {
            if r.from == c && matches!(r.kind, RelationKind::Inheritance | RelationKind::Realization) && !out.contains(&r.to) {
                out.push(r.to.clone());
                stack.push(r.to.clone());
            }
        }
    }
    out
}

pub fn has_method(d: &Design, class: &str, method: &str) -> bool {
    std::iter::once(class.to_string()).chain(supertypes(d, class)).any(|c| {
        d.live_class(&c).is_some_and(|c| c.methods.iter().any(|m| m.name == method && m.status.is_live()))
    })
}

/// Validate a sequence against the class model; fills `problem` on messages.
pub fn validate_sequence(d: &Design, s: &mut Sequence) -> Vec<String> {
    let mut problems = vec![];
    let is_actor = |n: &str| d.use_cases.actors.iter().any(|a| a.name == n);
    for m in s.messages.iter() {
        for p in [&m.from, &m.to] {
            if !s.participants.contains(p) {
                s.participants.push(p.clone());
            }
        }
    }
    for p in &s.participants {
        if d.live_class(p).is_none() && !is_actor(p) {
            problems.push(format!("participant {p} is neither a class nor an actor"));
        }
    }
    for (i, m) in s.messages.iter_mut().enumerate() {
        m.problem = None;
        if m.kind == MessageKind::Return || is_actor(&m.to) {
            continue;
        }
        let name = m.method.split('(').next().unwrap_or("").trim().to_string();
        let problem = if d.live_class(&m.to).is_none() {
            Some(format!("{} is not a class", m.to))
        } else if m.kind == MessageKind::Create {
            None
        } else if !has_method(d, &m.to, &name) {
            Some(format!("{}.{}() does not exist", m.to, name))
        } else {
            None
        };
        if let Some(p) = &problem {
            problems.push(format!("message #{}: {p}", i + 1));
        }
        m.problem = problem;
    }
    problems
}

/// Re-validate all sequences (after model changes) – used when building snapshots.
pub fn revalidate_sequences(d: &mut Design) {
    let copy = d.clone();
    for s in d.sequences.iter_mut() {
        validate_sequence(&copy, s);
    }
}
