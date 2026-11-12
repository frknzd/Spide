//! Messages Spide sends to the agent on behalf of the user.

use spide_core::model::{Decision, Design, ExaminerMode, PartnerMode};
use spide_core::tools::{brief_digest, stage, stage_hint};
use spide_core::Target;

/// What the current partner mode means for this turn.
pub fn mode_rules(mode: PartnerMode) -> &'static str {
    match mode {
        PartnerMode::Examiner => "Mode EXAMINER: raise focused decisions (propose_decision) and make the user defend them; propose_ops only for small obvious fixes.",
        PartnerMode::Socratic => "Mode SOCRATIC: do NOT propose design changes. Ask one probing question at a time that leads the user to the answer (forces, responsibilities, coupling, change scenarios). You may record the user's own answers as forces with propose_ops.",
        PartnerMode::DevilsAdvocate => "Mode DEVIL'S ADVOCATE: raise decisions as usual, but challenge whatever the user leans towards with the strongest counter-argument.",
        PartnerMode::Sketch => "Mode SKETCH: move fast. Propose coherent structures directly with propose_ops; use propose_decision only for big forks that are hard to reverse.",
    }
}

pub fn user_prompt(text: &str, tab: &str, d: &Design) -> String {
    let st = stage(d);
    let mut out = format!(
        "[spide · user prompt · tab={tab} · mode={} · stage={st}]\n{}\nStage: {}\n",
        d.settings.partner.as_str(),
        mode_rules(d.settings.partner),
        stage_hint(st)
    );
    if st == "brief" {
        out.push_str(
            "The brief is empty. Before proposing classes, elicit it: ask ONE short question at a time (goals → quality attributes \
             → constraints → expected changes). Record each answer with propose_ops add_force (kind goal|quality|constraint|change, \
             priority 1 must, 2 should, 3 nice). Stop after 3–5 forces or when the user says to go ahead / types /skip.\n",
        );
    } else {
        out.push_str(&format!("Brief:\n{}\n", brief_digest(d)));
    }
    out.push_str(&format!("\nUser: {text}"));
    out
}

pub fn prune_feedback(targets: &[Target], text: &str, tab: &str) -> String {
    let what: Vec<String> = targets.iter().map(Target::label).collect();
    format!(
        "[spide · prune feedback · tab={tab}]\nThe user pruned: {}.\nWhat they didn't like / what they want instead:\n\"{text}\"\n\n\
         Respond briefly. If a replacement is warranted, raise it with propose_decision (or propose_ops for small, \
         unambiguous fixes). Never re-introduce pruned elements unless the user asks.",
        what.join(", ")
    )
}

pub fn examine(d: &Decision, mode: ExaminerMode, design: &Design) -> String {
    let attempt = d.attempts.last().cloned().unwrap_or_default();
    let choice = match (&attempt.option, &attempt.custom) {
        (Some(o), _) => {
            let opt = d.options.iter().find(|x| &x.id == o);
            let tradeoffs = opt
                .map(|x| {
                    x.tradeoffs
                        .iter()
                        .map(|t| format!("{}{}{}", t.effect.trim(), t.force, t.note.as_ref().map(|n| format!(" ({n})")).unwrap_or_default()))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            format!(
                "option {o}: \"{}\"{}{}",
                opt.map(|x| x.label.as_str()).unwrap_or("?"),
                opt.and_then(|x| x.summary.as_ref()).map(|s| format!(" — {s}")).unwrap_or_default(),
                if tradeoffs.is_empty() { String::new() } else { format!(" [trade-offs: {}]", tradeoffs.join(", ")) }
            )
        }
        (None, Some(c)) => format!("their OWN alternative: \"{c}\""),
        _ => "nothing".into(),
    };
    let prior = d.attempts.len().saturating_sub(1);
    let strict = match mode {
        ExaminerMode::Strict => "Examiner mode is STRICT: only accept a defence that names concrete design principles and trade-offs that genuinely apply.",
        ExaminerMode::Lenient => "Examiner mode is LENIENT: accept a defence that is reasonable and shows understanding, even if not perfect.",
    };
    let devil = if design.settings.partner == PartnerMode::DevilsAdvocate && d.attempts.len() == 1 {
        "\nDEVIL'S ADVOCATE: unless the defence already refutes the strongest argument against this choice, reject this first \
         attempt and state that strongest counter-argument as your feedback (the user must answer it)."
    } else {
        ""
    };
    let custom_note = if attempt.custom.is_some() && attempt.option.is_none() {
        "\nBecause this is a custom answer, you MUST pass `ops` to evaluate_defense that implement it in the model \
         (also when rejecting, so the user can overrule)."
    } else {
        ""
    };
    format!(
        "[spide · examine defence · decision {id}]\nQuestion: {q}\nThe user chose {choice}.\nTheir defence (attempt {n}):\n\"{def}\"\n\n\
         Act as a demanding but fair software-design examiner. {strict} Judge whether the defence is sound for THIS design \
         (cohesion, coupling, responsibilities/GRASP, SOLID, layering) and above all against the BRIEF below: a good defence \
         explains which forces the choice serves and which it sacrifices. {prior_note}{devil}\nBrief:\n{brief}\n\
         Call `evaluate_defense` exactly once with decision_id=\"{id}\", verdict accept|reject, and short feedback addressed to the \
         user (what was convincing, or which question they must still answer).{custom_note}",
        id = d.id,
        q = d.question,
        n = d.attempts.len(),
        def = attempt.defense,
        brief = brief_digest(design),
        prior_note = if prior > 0 { format!("They already had {prior} rejected attempt(s); consider whether they addressed your earlier feedback.") } else { String::new() },
    )
}

pub fn gen_use_cases() -> String {
    "[spide · generate use case diagram]\nRead the current class model with get_design and derive a use case diagram for it: \
     actors (human roles and external systems), use cases (verb phrases, user goals) with the classes that realise them, \
     and include/extend only where they add clarity. Then call set_use_cases once. Keep pruned items out. Reply with one sentence."
        .into()
}

pub fn gen_sequence(scenario: Option<&str>) -> String {
    let what = match scenario {
        Some(s) => format!("for the scenario \"{s}\""),
        None => "for the most important use case that has no sequence diagram yet".into(),
    };
    format!(
        "[spide · generate sequence diagram]\nCreate a sequence diagram {what}. Lifelines must be existing classes \
         (or actors from the use case diagram); every sync/async message must call an EXISTING method of the receiving class. \
         If a needed method is missing, do not invent it in the sequence: raise a propose_decision to add it first. \
         Call set_sequence once, then reply with one sentence."
    )
}
