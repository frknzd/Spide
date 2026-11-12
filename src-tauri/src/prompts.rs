//! Messages Spide sends to the agent on behalf of the user.

use spide_core::model::{Decision, ExaminerMode};
use spide_core::Target;

pub fn user_prompt(text: &str, tab: &str) -> String {
    format!("[spide · user prompt · tab={tab}]\n{text}")
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

pub fn examine(d: &Decision, mode: ExaminerMode) -> String {
    let attempt = d.attempts.last().cloned().unwrap_or_default();
    let choice = match (&attempt.option, &attempt.custom) {
        (Some(o), _) => {
            let opt = d.options.iter().find(|x| &x.id == o);
            format!(
                "option {o}: \"{}\"{}",
                opt.map(|x| x.label.as_str()).unwrap_or("?"),
                opt.and_then(|x| x.summary.as_ref()).map(|s| format!(" — {s}")).unwrap_or_default()
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
    let custom_note = if attempt.custom.is_some() && attempt.option.is_none() {
        "\nBecause this is a custom answer, you MUST pass `ops` to evaluate_defense that implement it in the model \
         (also when rejecting, so the user can overrule)."
    } else {
        ""
    };
    format!(
        "[spide · examine defence · decision {id}]\nQuestion: {q}\nThe user chose {choice}.\nTheir defence (attempt {n}):\n\"{def}\"\n\n\
         Act as a demanding but fair software-design examiner. {strict} Judge whether the defence is sound for THIS design \
         (cohesion, coupling, responsibilities/GRASP, SOLID, layering, the stated requirements). {prior_note}\n\
         Call `evaluate_defense` exactly once with decision_id=\"{id}\", verdict accept|reject, and short feedback addressed to the \
         user (what was convincing, or which question they must still answer).{custom_note}",
        id = d.id,
        q = d.question,
        n = d.attempts.len(),
        def = attempt.defense,
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
