//! Messages Spide sends to the agent on behalf of the user.

use spide_core::model::{Design, PartnerMode};
use spide_core::tools::{brief_digest, stage, stage_hint};
use spide_core::Target;

/// What the current partner mode means for this turn.
pub fn mode_rules(mode: PartnerMode) -> &'static str {
    match mode {
        PartnerMode::Examiner => "Mode EXAMINER: explain focused design choices and alternatives with propose_decision; propose_ops creates a single-option decision for small fixes. The user approves; notes are optional and no defence or verdict is required.",
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
    out.push_str(&assessment_hint(d));
    out.push_str("\nThe above stage/mode rules govern diagram work. For ordinary coding or shell requests, help directly with the available tools; do not force a design interview. Diagram mutations still require Spide MCP proposals/decisions.\n");
    out.push_str(&format!("\nUser: {text}"));
    out
}

/// Ask the agent (once per turn, batched) to judge classes it hasn't assessed or whose assessment is stale.
pub fn assessment_hint(d: &Design) -> String {
    let (stale, missing) = spide_core::assess::stale_and_missing(d);
    if stale.is_empty() && missing.is_empty() {
        return String::new();
    }
    let list = |v: &[String]| {
        let mut s = v.iter().take(8).cloned().collect::<Vec<_>>().join(", ");
        if v.len() > 8 {
            s.push_str(&format!(" … (+{})", v.len() - 8));
        }
        s
    };
    let mut parts = vec![];
    if !missing.is_empty() {
        parts.push(format!("not assessed yet: {}", list(&missing)));
    }
    if !stale.is_empty() {
        parts.push(format!("changed since your assessment: {}", list(&stale)));
    }
    format!(
        "Assessments — {}. When you're done with the user's request, record your judgement with ONE batched assess_class call \
         (purpose, role, and per method concern + verdict fits|misplaced|duplicate|delegates with a reason).\n",
        parts.join("; ")
    )
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


pub fn fix_issue(message: &str, rule: Option<&str>, fix_hint: Option<&str>, kind: Option<&str>, targets: &[String]) -> String {
    let method_targets: Vec<&String> = targets.iter().filter(|t| t.contains('.')).collect();
    let placement = matches!(kind, Some("misplaced_responsibility" | "duplication" | "concept_elsewhere" | "feature_envy" | "fat_controller" | "data_class"));
    let how = if placement && !method_targets.is_empty() {
        format!(
            "This is about WHERE behaviour lives. Call suggest_home with {} and build the decision from the candidates that illustrate the relevant trade-offs: \
             the best existing home (Information Expert), a new Pure Fabrication class if offered, and — if defensible — keeping it \
             (copy each candidate's ops; explain legality, data ownership, dependency changes, cohesion observations and trade-offs; candidates are not ranked).",
            if method_targets.len() > 1 && kind == Some("duplication") {
                format!("group: [{}]", method_targets.iter().map(|m| format!("\"{m}\"")).collect::<Vec<_>>().join(", "))
            } else {
                format!("method: \"{}\"", method_targets[0])
            }
        )
    } else {
        "Read get_design and get_metrics (get_responsibilities / get_overlap when it's about responsibilities or duplication).".into()
    };
    format!(
        "[spide · fix issue]\nThe user wants to address this health finding:\n\"{message}\"{}{}\n{how}\n\
         Then raise ONE propose_decision with 2–3 concrete ways to resolve it (each with ops and trade-offs against the brief):\n\
         - Prefer SIMPLIFYING first (inlining a pass-through, deleting a speculative abstraction, or merging over-fragmented types) when that resolves the root cause.\n\
         - When a programming pattern fits (Strategy, Polymorphism, Facade, Pure Fabrication, Dependency Inversion, Parameter Object), explain WHY it pays for its extra indirection here and when it would be overkill.\n\
         - If the finding stems from an artificial constraint or rule that doesn't fit the system's scale, say so explicitly.\n\
         Don't pick for the user. Reply with one sentence.",
        rule.map(|r| format!("\nBroken rule: {r}")).unwrap_or_default(),
        fix_hint.map(|f| format!("\nPattern / guidance: {f}")).unwrap_or_default(),
    )
}

pub fn challenge_assumptions(d: &Design, r: &spide_core::health::HealthReport) -> String {
    let challenges = if r.challenges.is_empty() {
        "No automated challenge flags triggered; inspect the brief, architecture preset, and abstractions yourself.".to_string()
    } else {
        r.challenges
            .iter()
            .map(|c| format!("- [{}] {}: {} ({})", c.kind, c.title, c.question, c.evidence))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let top_findings: String = r
        .unique_signals()
        .into_iter()
        .take(8)
        .map(|s| format!("- [{} / {}] {} ({})", s.tier, s.rule, s.message, s.target))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "[spide · challenge assumptions & decomplexify]\n\
         The user wants you to act as a sharp, pragmatic architect and CHALLENGE their current assumptions, artificial requirements/constraints, and unnecessary complexity.\n\
         Current health: {}\n\
         Architecture preset: {} ({} custom forbidden deps, {} forbidden types)\n\
         Brief:\n{}\n\
         Automated assumption challenges:\n{challenges}\n\
         Top findings (leverage-ordered):\n{}\n\n\
         Read get_design and get_health. Then:\n\
         1. Question 2–3 specific assumptions, constraints, layers, or abstractions that may be adding complexity without earning their keep.\n\
         2. If there is a concrete simplification (collapsing a pass-through layer, inlining speculative interfaces, merging fragmented packages, or splitting a root-cause hotspot), raise ONE focused propose_decision comparing the simplified design against keeping the current structure (with honest trade-offs and health impact).\n\
         3. Keep your reply concise, direct, and grounded in the actual classes and brief.",
        r.explain,
        d.rules.preset.as_str(),
        d.rules.forbidden.len(),
        d.rules.forbidden_types.len(),
        brief_digest(d),
        if top_findings.is_empty() { "(none)".to_string() } else { top_findings },
    )
}

pub fn analyse_change(name: &str, description: Option<&str>) -> String {
    format!(
        "[spide · analyse change scenario]\nChange scenario: \"{name}\"{}\nRead get_design. Which classes would have to change to support it? \
         Call set_change_impact once with every class that must change and a short why. Don't propose design changes in this turn; \
         if locality is poor, say so in one sentence and offer to raise a decision.",
        description.map(|d| format!(" — {d}")).unwrap_or_default()
    )
}

pub fn gen_domain_model() -> String {
    "[spide · generate domain model]\nRead get_design. Build a conceptual DOMAIN MODEL (Larman) of the problem domain this class design \
     deals with: concepts of the domain (things, events, roles, descriptions), attributes (simple types, no visibility, no methods), \
     associations with verb-phrase labels and multiplicities, generalization only for a real is-a. Leave out software artefacts: UI, \
     controllers, services, repositories, listeners, adapters, framework/library classes and design patterns. Names may differ from \
     the class design; it is a separate model. Keep pruned items out. Call set_domain_model once. Reply with one sentence."
        .into()
}

/// One request for everything a batch export needs regenerated (domain model, use cases, sequences — in that order).
pub fn gen_for_export(domain: bool, use_cases: bool, sequences: &[String]) -> String {
    let mut steps = vec![];
    if domain {
        steps.push("the DOMAIN MODEL: a conceptual model of the problem domain (concepts with attributes, no methods, no software \
                    artefacts; associations with labels and multiplicities) → call set_domain_model once"
            .to_string());
    }
    if use_cases {
        steps.push("the USE CASE diagram (actors, user-goal use cases with the classes that realise them) → call set_use_cases once".to_string());
    }
    for s in sequences {
        steps.push(format!("the sequence diagram \"{s}\" (existing classes and methods only) → call set_sequence with name \"{s}\""));
    }
    let list: String = steps.iter().enumerate().map(|(i, s)| format!("\n{}. {s}", i + 1)).collect();
    format!(
        "[spide · regenerate diagrams for export]\nThe user is exporting all diagrams; these are out of date with the class model. \
         Read get_design, then regenerate, in this order:{list}\nKeep pruned items out. Don't change the class design. Reply with one sentence."
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
        "[spide · generate sequence diagram]\nCreate a UML sequence diagram {what}. Lifelines are objects `name : Class` of existing \
         classes (or actors from the use case diagram); every sync/async message must call an EXISTING method of the receiver's class. \
         If a needed method is missing, do not invent it in the sequence: raise a propose_decision to add it first. Use combined \
         fragments where the behaviour has them (loop for repetition, alt with guards and else for choices, opt for optional parts, \
         par, break, critical, ref for an interaction drawn elsewhere), create objects with kind=create before using them and end \
         them with kind=destroy when they die. Give returned values in `returns`; reply arrows are drawn automatically. \
         Call set_sequence once, then reply with one sentence."
    )
}

/// Spide Project: code → design.
pub fn readjust(classes: &[String], n: usize, empty_design: bool) -> String {
    let scope = if classes.is_empty() { "all classes".to_string() } else { classes.join(", ") };
    let bootstrap = if empty_design {
        "\nThe design is empty: BOOTSTRAP it from the code — one decision per package with the public API only (no private helpers, \
         accessors or framework plumbing), responsibilities per class, framework/library classes marked external; afterwards generate \
         the domain model (set_domain_model) and use cases (set_use_cases)."
    } else {
        ""
    };
    format!(
        "[spide · readjust the design to the code]\nThe code changed; bring the DESIGN up to date ({n} difference(s), scope: {scope}). \
         Follow the spide-readjust skill: get_sync with status [code_ahead, conflict, unreconciled]{} → read get_code where the intent \
         is unclear → propose_ops for mechanical changes (start from suggested_ops; add responsibilities and descriptions), \
         propose_decision when the code changed structure or responsibilities (offer adopting it vs. keeping the design and \
         implementing it back). Never delete design intent just because the code lacks it. Don't edit code. Reply with one sentence.{bootstrap}",
        if classes.is_empty() { String::new() } else { format!(" for classes {}", serde_json::to_string(classes).unwrap()) }
    )
}

/// Spide Project: design → code, for the approved changes in `brief`.
pub fn implement(brief: &str, m: &spide_core::project::Manifest) -> String {
    let test = m
        .commands
        .get("test")
        .map(|c| format!("\n- When done, run `{}` (in `{}` relative to the project root) and fix what you broke.", c.run, c.cwd.as_deref().unwrap_or(".")))
        .unwrap_or_default();
    format!(
        "[spide · implement the approved design in the code]\nThe user asked you to implement these approved design changes in the Java code \
         (follow the spide-implement skill). Per class: the file, the exact changes, the class's responsibilities and the decisions behind them.\n{brief}\n\
         Rules:\n- Implement only these changes and keep existing behaviour; follow the repository's instructions (AGENTS.md).\n\
         - Don't create framework/library types (ZAP, Swing, JDK); ask the user to mark them external instead.\n- Don't change the design through Spide tools.\n- Follow explicit Git requests from the user, subject to repository instructions.\n- Ask the user before deleting a file.{test}\n\
         - Finish with a two-line summary. Spide re-indexes the code afterwards and shows what is still out of sync."
    )
}

/// Spide Project: run the test command.
pub fn verify(cmd: &str, cwd: &str) -> String {
    format!(
        "[spide · verify]\nRun `{cmd}` in `{cwd}` (relative to the project root) and report pass or fail in one sentence; on failure quote the \
         first relevant error. Don't change code unless the user asks. (Structural sync is checked by Spide; tests check behaviour.)"
    )
}

