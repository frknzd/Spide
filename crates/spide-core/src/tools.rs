//! Agent-facing tool API (served over MCP). Pure functions over the store.

use crate::derive;
use crate::metrics::compute;
use crate::model::*;
use crate::ops::Op;
use crate::store::{summarize, Store};
use serde::Deserialize;
use serde_json::{json, Value};

pub const INSTRUCTIONS: &str = "Spide is a live class-diagram design canvas. The user can only see and accept/prune; \
you change the design exclusively through these tools. Everything you add is a PROPOSAL the user must accept \
(propose_ops) or a DECISION the user must choose and defend (propose_decision). Read get_design first.";

const OPS_DOC: &str = r#"Each op is an object with an "op" field:
- {"op":"add_class","name":"Order","stereotype?":"entity|value|enum|interface|abstract|service|controller|repository|…","package?":"domain","is_abstract?":false,"aggregate_root?":false,
   "attributes?":[ATTR…],"methods?":[METHOD…],"responsibilities?":["…"],"rationale?":"why it exists"}
- {"op":"update_class","name":"Order", any of "stereotype","package","is_abstract","responsibilities","rationale","aggregate_root","external","role"}
   aggregate_root = consistency boundary (one transaction changes one aggregate; Spide flags scenarios that change several).
   external = the class belongs to a library/framework (not designed here). role = intended purpose:
   presentation|controller|service|domain|persistence|adapter|output|creator|utility ("" clears it).
- {"op":"rename_class","from":"A","to":"B"}            - {"op":"remove_class","name":"A"}
- {"op":"add_attribute","class":"Order","attribute":ATTR}   - {"op":"update_attribute","class":"Order","name":"total","attribute":ATTR}
- {"op":"remove_attribute","class":"Order","name":"total"}
- {"op":"add_method","class":"Order","method":METHOD}       - {"op":"update_method","class":"Order","name":"pay","method":METHOD}
- {"op":"remove_method","class":"Order","name":"pay"}
- {"op":"add_relation","from":"Order","to":"OrderLine","kind":"association|aggregation|composition|inheritance|realization|dependency",
   "label?":"contains","from_mult?":"1","to_mult?":"1..*","directed?":true,"interaction?":"sync|async|event","rationale?":"…"}
   Direction: composition/aggregation from=WHOLE to=PART; inheritance/realization from=CHILD to=PARENT; association/dependency from=CLIENT to=SUPPLIER.
- {"op":"update_relation","from","to","kind", any of "label","from_mult","to_mult","directed","interaction"}
- {"op":"remove_relation","from":"A","to":"B","kind?":"…"}
- {"op":"add_package","name":"domain","layer?":"ui|application|domain|infrastructure","external?":false,"system?":"C4 system name"}  - {"op":"set_layer","package":"…","layer":"…"|null}
- {"op":"update_package","name":"zap", any of "description","external","system"}  (external = a library/framework's package, e.g. one you extend)
- {"op":"move_to_package","class":"Order","package":"domain"}   - {"op":"remove_package","name":"…"}
- {"op":"add_force","name":"Evolvability","kind":"goal|quality|constraint|change","priority?":1|2|3,"description?":"…"}  (the brief)
- {"op":"update_force","name":"…", any of "rename","kind","priority","description"}   - {"op":"remove_force","name":"…"}
- {"op":"add_change_scenario","name":"Support e-books","description?":"…"}   - {"op":"remove_change_scenario","name":"…"}
C4 levels (above classes):
- {"op":"add_system","name":"PaymentProvider","description?":"…"}  (external system)   - {"op":"remove_system","name":"…"}
- {"op":"add_container","name":"API","technology?":"Spring Boot","description?":"…","replicas?":2,"datastore?":false}
- {"op":"update_container","name":"API", any of "technology","description","replicas","datastore"}   - {"op":"remove_container","name":"…"}
- {"op":"assign_container","package":"domain","container":"API"}   (which container a package runs in; null unassigns)
- {"op":"add_link","from":"API","to":"PaymentProvider","interaction?":"sync|async|event","protocol?":"HTTPS/JSON"}  (container/system/actor ends)
- {"op":"remove_link","from":"…","to":"…"}
Container→container dependencies are DERIVED from class dependencies; only add links for external systems, actors or protocols.
ATTR   = {"name":"total","type":"Money","visibility?":"public|private|protected|package","is_static?":false,"multiplicity?":"0..1"}
METHOD = {"name":"addLine","params":[{"name":"p","type":"Product"},{"name":"qty","type":"int"}],"returns?":"OrderLine","visibility?":"public",
          "is_abstract?":false,"uses?":["lines"] (attributes of the same class it touches — drives the cohesion metric),
          "calls?":["Product.price","recalculate"] (methods it calls — drives coupling), "description?":"…",
          "concern?":"presentation|coordination|domain_logic|persistence|formatting|creation|accessor" (set it when the name doesn't say what it does)}
Batches are atomic and may reference classes added earlier in the same batch."#;

pub fn tool_list() -> Value {
    let ops_schema = json!({"type":"array","description": OPS_DOC,"items":{"type":"object","properties":{"op":{"type":"string"}},"required":["op"]}});
    json!([
        {"name":"get_design","description":"Read the whole design: partner mode, design stage, the brief (forces: goals, qualities, constraints, expected changes), classes with members, relations, packages, pending proposals and decisions, \
           pruned elements WITH the user's feedback (respect it!), use cases and sequences. Compact JSON.",
         "inputSchema":{"type":"object","properties":{}}, "annotations":{"readOnlyHint":true}},
        {"name":"get_metrics","description":"Architecture health: score 0–100, issues (cycles, god classes, low cohesion (LCOM4), layer violations, orphans, high coupling), per-class fan-in/out & instability, package metrics.",
         "inputSchema":{"type":"object","properties":{}}, "annotations":{"readOnlyHint":true}},
        {"name":"get_view","description":"Read a derived view: domain (conceptual classes), package (packages + dependencies), use_case, sequence.",
         "inputSchema":{"type":"object","properties":{"kind":{"type":"string","enum":["domain","package","use_case","sequence"]}},"required":["kind"]},
         "annotations":{"readOnlyHint":true}},
        {"name":"propose_decision","description":"Raise a design DECISION for the user, quiz-style. Use this for every non-trivial choice \
           (which class owns a responsibility, who talks to whom, inheritance vs composition, where a class lives…). Give 2–3 genuinely \
           different, viable options, each with the concrete ops that implement it; the user sees a live preview of each, picks one or \
           writes their own, and must DEFEND it. You will then be asked to examine the defence. Do not apply the ops yourself. \
           Keep each decision focused (one question).",
         "inputSchema":{"type":"object","properties":{
            "question":{"type":"string"},
            "context":{"type":"string","description":"Why this decision matters now (1–2 sentences)."},
            "options":{"type":"array","minItems":1,"maxItems":4,"items":{"type":"object","properties":{
                "label":{"type":"string","description":"Short name, e.g. 'Order owns OrderLines'"},
                "summary":{"type":"string"},
                "pros":{"type":"array","items":{"type":"string"}},
                "cons":{"type":"array","items":{"type":"string"}},
                "tradeoffs":{"type":"array","description":"Forces from the brief this option helps (+) or hurts (-).",
                    "items":{"type":"object","properties":{"force":{"type":"string"},"effect":{"type":"string","enum":["+","-"]},"note":{"type":"string"}},
                    "required":["force","effect"]}},
                "ops": ops_schema},"required":["label","ops"]}}},
          "required":["question","options"]}},
        {"name":"propose_ops","description":"Propose small, unambiguous changes (e.g. adding obvious attributes, fixing a multiplicity). They appear as \
           ghosts the user reviews and can amend in the Proposals inspector before accepting. Optional decision/option fields associate context only, not approval. Use propose_decision instead for anything the user should think about.",
         "inputSchema":{"type":"object","properties":{"ops": ops_schema,"rationale":{"type":"string"},"decision":{"type":"string"},"option":{"type":"string"}},"required":["ops"]}},
        {"name":"evaluate_defense","description":"Examiner verdict on the user's defence of a decision (only when Spide asks you to examine). \
           accept applies the chosen option; reject sends your feedback back so the user can try again. For a CUSTOM answer pass the ops that implement it.",
         "inputSchema":{"type":"object","properties":{
            "decision_id":{"type":"string"},
            "verdict":{"type":"string","enum":["accept","reject"]},
            "feedback":{"type":"string","description":"Addressed to the user, 1–3 sentences."},
            "ops": ops_schema},
          "required":["decision_id","verdict","feedback"]}},
        {"name":"set_use_cases","description":"Replace the use case diagram (derived from the class model).",
         "inputSchema":{"type":"object","properties":{
            "system":{"type":"string","description":"System boundary name"},
            "actors":{"type":"array","items":{"type":"object","properties":{"name":{"type":"string"},"description":{"type":"string"}},"required":["name"]}},
            "use_cases":{"type":"array","items":{"type":"object","properties":{
                "name":{"type":"string"},"actors":{"type":"array","items":{"type":"string"}},
                "includes":{"type":"array","items":{"type":"string"}},"extends":{"type":"array","items":{"type":"string"}},
                "classes":{"type":"array","items":{"type":"string"},"description":"Classes that realise it"},
                "description":{"type":"string"}},"required":["name","actors"]}}},
          "required":["actors","use_cases"]}},
        {"name":"set_change_impact","description":"Record your analysis of a CHANGE SCENARIO (\"what if we must support X?\"): the classes that \
           would have to change, each with a short why. Spide computes the ripple (classes depending on them) and a locality score, \
           and lets the user stress-test the design. Creates the scenario if it doesn't exist. Be honest: list every class that must change.",
         "inputSchema":{"type":"object","properties":{
            "name":{"type":"string"},"description":{"type":"string"},
            "impacted":{"type":"array","items":{"type":"object","properties":{"class":{"type":"string"},"why":{"type":"string"}},"required":["class"]}}},
          "required":["name","impacted"]}},
        {"name":"assess_class","description":"Record YOUR judgement of classes (batch several in one call): what each class is for \
           (purpose, role) and, per method, what it does and whether it belongs there. Spide layers this on its own signals (names, \
           stereotypes, verbs, calls): agreement raises an issue's confidence, a 'fits' verdict lowers it (and on serious issues shows a \
           disagreement the user must resolve), and your 'misplaced'/'duplicate' verdicts become issues. An assessment goes stale when the \
           class changes; get_design lists stale and missing ones. Judge by responsibilities (RDD knowing/doing/deciding, GRASP Expert, \
           Controller, Creator, Pure Fabrication), not by taste. basis=code when you read the implementation (get_code).",
         "inputSchema":{"type":"object","properties":{
            "assessments":{"type":"array","items":{"type":"object","properties":{
                "class":{"type":"string"},"purpose":{"type":"string","description":"one sentence"},
                "role":{"type":"string","enum":["presentation","controller","service","domain","persistence","adapter","output","creator","utility","value"]},
                "basis":{"type":"string","enum":["design","code"]},
                "methods":{"type":"array","items":{"type":"object","properties":{
                    "name":{"type":"string"},
                    "concern":{"type":"string","enum":["presentation","coordination","domain_logic","persistence","formatting","creation","accessor"]},
                    "rdd":{"type":"string","enum":["knowing","doing","deciding"]},
                    "verdict":{"type":"string","enum":["fits","misplaced","duplicate","delegates"]},
                    "home":{"type":"string","description":"better owner (class or new class name) if misplaced/duplicate"},
                    "duplicate_of":{"type":"string","description":"Class.method it duplicates"},
                    "confidence":{"type":"number","minimum":0,"maximum":1},
                    "reason":{"type":"string"}},"required":["name","verdict","reason"]}}},
              "required":["class","purpose","methods"]}}},
          "required":["assessments"]}},
        {"name":"get_responsibilities","description":"What each class is FOR and whether its methods fit: the role Spide reads \
           (explicit role > your assessment > name > stereotype > layer, with the evidence), every method's concern \
           (presentation|coordination|domain_logic|persistence|formatting|creation|accessor) and RDD kind (knowing|doing|deciding) \
           with the reason, what it reads/tells elsewhere, and the findings (misplaced work, fat controllers, vague names, role \
           inconsistencies, mixed concerns) with course references. on=code judges the imported implementation instead.",
         "inputSchema":{"type":"object","properties":{"class":{"type":"string","description":"only this class"},
            "on":{"type":"string","enum":["design","code"]}}}},
        {"name":"get_overlap","description":"How much behaviour is duplicated or misplaced: method pairs with a similarity score and \
           its parts (name, signature, touches = what the bodies call/use, text, code = normalised implementation), the class \
           overlap matrix (share of A's behaviour duplicated in B), concepts another class owns (its data or stated \
           responsibility), concepts scattered over ≥3 classes (shotgun surgery), pass-through delegation (not duplication) and \
           middle men. Run it before proposing new members. on=code compares the imported implementation (real clones).",
         "inputSchema":{"type":"object","properties":{"class":{"type":"string"},"min_score":{"type":"number","minimum":0,"maximum":1},
            "on":{"type":"string","enum":["design","code"]}}}},
        {"name":"suggest_home","description":"Where should a misplaced method (method: \"Class.method\") or a group of duplicates \
           (group: [\"A.m\", \"B.n\"]) live? Ranked candidates: keep, move to an existing class (Information Expert share of \
           the data it uses; wholes take over their parts), or a NEW Pure Fabrication class in the innermost legal layer. Each \
           has legality (layer/forbidden rules for every caller), the simulated health before→after, the source's fan-out \
           change, the receiver's LCOM4, role fit, a score, a why, and ready-to-use OPS. Turn the top 2–3 into propose_decision \
           options (copy their ops) — the user chooses and defends.",
         "inputSchema":{"type":"object","properties":{"method":{"type":"string"},"group":{"type":"array","items":{"type":"string"}}}}},
        {"name":"get_code","description":"Read the IMPLEMENTATION of a class (or one method) from the imported code (/import), \
           with the facts Spide extracted (lines, call chains, switches, loops, creations, UI calls). Use it to judge what the code \
           really does before assess_class with basis=code. Read-only; long classes are capped — ask per method.",
         "inputSchema":{"type":"object","properties":{"class":{"type":"string"},"method":{"type":"string"}},"required":["class"]}},
        {"name":"set_sequence","description":"Create/replace one sequence diagram. Participants: classes or actors. Each sync/async/create message's \
           `method` must be an existing method name on the `to` class (validated; problems are reported back and highlighted). \
           Use kind=return for replies.",
         "inputSchema":{"type":"object","properties":{
            "name":{"type":"string"},"use_case":{"type":"string"},
            "participants":{"type":"array","items":{"type":"string"},"description":"Ordered lifelines (left→right)"},
            "messages":{"type":"array","items":{"type":"object","properties":{
                "from":{"type":"string"},"to":{"type":"string"},"method":{"type":"string"},"args":{"type":"string"},
                "returns":{"type":"string"},"kind":{"type":"string","enum":["sync","async","return","create"]},"note":{"type":"string"}},
                "required":["from","to","method"]}}},
          "required":["name","messages"]}}
    ])
}

pub struct ToolOutcome {
    pub text: String,
    pub changed: bool,
}

fn ok(text: impl Into<String>, changed: bool) -> Result<ToolOutcome, String> {
    Ok(ToolOutcome { text: text.into(), changed })
}

fn parse<T: for<'de> Deserialize<'de>>(v: &Value) -> Result<T, String> {
    serde_json::from_value(v.clone()).map_err(|e| format!("invalid arguments: {e}"))
}

use crate::impact::health_delta;

/// Where the design is in the boundaries → responsibilities → interactions → details progression.
pub fn stage(d: &Design) -> &'static str {
    let classes: Vec<&Class> = d.live_classes().collect();
    if classes.is_empty() {
        return if d.live_forces().next().is_none() && !d.settings.brief_skipped { "brief" } else { "boundaries" };
    }
    let packaged = classes.iter().filter(|c| c.package.is_some()).count();
    if classes.len() >= 4 && packaged * 2 < classes.len() {
        return "boundaries";
    }
    let described = classes
        .iter()
        .filter(|c| !c.responsibilities.is_empty() || c.methods.iter().any(|m| m.status.is_live()))
        .count();
    if described * 2 < classes.len() {
        return "responsibilities";
    }
    let rels = d.live_relations().count();
    if rels + 1 < classes.len() {
        return "interactions";
    }
    "details"
}

pub fn stage_hint(stage: &str) -> &'static str {
    match stage {
        "brief" => "The brief is empty: elicit goals, quality attributes, constraints and expected changes first (one question at a time).",
        "boundaries" => "Settle boundaries: packages/layers and the core concepts before details.",
        "responsibilities" => "Assign responsibilities: what each class knows and does (GRASP).",
        "interactions" => "Decide collaborations: who talks to whom, ownership, multiplicities.",
        _ => "Refine details: signatures, types, cohesion; validate with scenarios.",
    }
}

/// Compact one-line-per-force brief for prompts.
pub fn brief_digest(d: &Design) -> String {
    let mut forces: Vec<&Force> = d.live_forces().collect();
    forces.sort_by_key(|f| (f.priority, f.kind as u8));
    if forces.is_empty() {
        return "(no brief yet)".into();
    }
    forces
        .iter()
        .map(|f| {
            let prio = ["must", "should", "nice"][(f.priority.clamp(1, 3) - 1) as usize];
            match &f.description {
                Some(desc) => format!("- [{} · {prio}] {}: {desc}", f.kind.as_str(), f.name),
                None => format!("- [{} · {prio}] {}", f.kind.as_str(), f.name),
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn call(store: &mut Store, name: &str, args: &Value) -> Result<ToolOutcome, String> {
    if store.design.settings.partner == PartnerMode::Socratic && matches!(name, "propose_ops" | "propose_decision") {
        // recording the user's own answers in the brief is still fine
        let only_forces = name == "propose_ops"
            && args.get("ops").and_then(Value::as_array).is_some_and(|ops| {
                ops.iter().all(|o| o.get("op").and_then(Value::as_str).is_some_and(|n| n.ends_with("_force")))
            });
        if !only_forces {
            return Err("Socratic mode: you may not propose design changes. Ask the user a guiding question instead; \
                        they will decide (or switch modes with /mode). Recording their answers as forces is allowed."
                .into());
        }
    }
    match name {
        "get_design" => ok(design_digest(store), false),
        "get_metrics" => {
            let v = store.view();
            let m = compute(&v, false);
            ok(serde_json::to_string(&json!({
                "health": m.health,
                "issues": m.issues,
                "classes": m.classes.iter().map(|(k, c)| json!({"class":k,"fan_in":c.fan_in,"fan_out":c.fan_out,
                    "instability":(c.instability*100.0).round()/100.0,"lcom4":c.lcom,"members":c.members,"god":c.god})).collect::<Vec<_>>(),
                "packages": m.packages,
                "package_dependencies": m.package_deps,
                "principles": m.principles,
                "health_explained": "health = weighted geometric mean of the principle scores (coupling .25, cohesion .2, responsibility .3, architecture .15, uml .1); each issue carries principle, course source, confidence (0..1) and basis (structure|heuristic|agent|code)",
            }))
            .unwrap(), false)
        }
        "get_view" => {
            let v = store.view();
            let kind = args.get("kind").and_then(Value::as_str).unwrap_or("");
            let out = match kind {
                "domain" => serde_json::to_value(derive::domain_model(&v)).unwrap(),
                "package" => {
                    let m = compute(&v, false);
                    json!({"packages": derive::package_view(&v).packages, "dependencies": m.package_deps})
                }
                "use_case" => serde_json::to_value(&v.use_cases).unwrap(),
                "sequence" => serde_json::to_value(&v.sequences).unwrap(),
                _ => return Err("kind must be domain|package|use_case|sequence".into()),
            };
            ok(out.to_string(), false)
        }
        "propose_decision" => {
            #[derive(Deserialize)]
            struct A {
                question: String,
                context: Option<String>,
                options: Vec<DecisionOption>,
            }
            let a: A = parse(args)?;
            let before = compute(&store.view(), false);
            let previews: Vec<String> = a
                .options
                .iter()
                .enumerate()
                .filter_map(|(i, o)| {
                    let id = if o.id.is_empty() { ((b'A' + i as u8) as char).to_string() } else { o.id.clone() };
                    crate::impact::impact_of_ops(&store.design, &o.ops).ok().map(|im| format!("{id}: {}", im.summary()))
                })
                .collect();
            // pending (ghost) forces count as known
            let view = store.view();
            let mut unknown: Vec<String> = a
                .options
                .iter()
                .flat_map(|o| o.tradeoffs.iter())
                .filter(|t| view.force(&t.force).is_none_or(|f| matches!(f.status, Status::Pruned | Status::Removing)))
                .map(|t| format!("\"{}\"", t.force))
                .collect();
            unknown.sort();
            unknown.dedup();
            let id = store.propose_decision(a.question, a.context, a.options).map_err(|e| e.0)?;
            let warn = if unknown.is_empty() {
                String::new()
            } else {
                format!(" Note: trade-offs mention forces that are not in the brief: {}.", unknown.join(", "))
            };
            ok(
                format!(
                    "Decision {id} is now shown to the user with live previews (current health {}). Consequences per option: {}.{warn} \
                     Wait: the user will choose and defend; Spide will then ask you to examine the defence. \
                     End your turn now with at most one short sentence.",
                    before.health,
                    previews.join(", ")
                ),
                true,
            )
        }
        "propose_ops" => {
            #[derive(Deserialize)]
            struct A {
                ops: Vec<Op>,
                rationale: Option<String>,
                decision: Option<String>,
                option: Option<String>,
            }
            let a: A = parse(args)?;
            let before = compute(&store.view(), true);
            let summary = summarize(&a.ops);
            let standalone = crate::impact::impact_of_ops(&store.design, &a.ops).ok();
            let id = store.propose_linked_ops(a.ops, a.rationale, "agent", a.decision, a.option).map_err(|e| e.0)?;
            let after = compute(&store.view(), true);
            let mut text = format!("Proposal {id} shown as ghosts ({summary}). {}", health_delta(&before, &after));
            if let Some(im) = standalone.filter(|im| !im.flipped_deps.is_empty() || !im.affected_scenarios.is_empty()) {
                if !im.flipped_deps.is_empty() {
                    let f: Vec<String> = im.flipped_deps.iter().map(|(a, b)| format!("{a} → {b}")).collect();
                    text.push_str(&format!(" Flipped dependencies: {}.", f.join(", ")));
                }
                if !im.affected_scenarios.is_empty() {
                    text.push_str(&format!(" Affects {} — check they still work.", im.affected_scenarios.join(", ")));
                }
            }
            ok(text, true)
        }
        "evaluate_defense" => {
            #[derive(Deserialize)]
            struct A {
                decision_id: String,
                verdict: String,
                feedback: String,
                ops: Option<Vec<Op>>,
            }
            let a: A = parse(args)?;
            let accept = match a.verdict.as_str() {
                "accept" => true,
                "reject" => false,
                _ => return Err("verdict must be accept or reject".into()),
            };
            store.evaluate_defense(&a.decision_id, accept, a.feedback, a.ops).map_err(|e| e.0)?;
            let m = compute(&store.view(), false);
            ok(
                if accept {
                    format!("Accepted and applied. Health now {}. Reply with at most one sentence.", m.health)
                } else {
                    "Rejected; your feedback is shown to the user who may revise their defence. End your turn.".into()
                },
                true,
            )
        }
        "assess_class" => {
            #[derive(Deserialize)]
            struct A {
                assessments: Vec<crate::assess::Assessment>,
            }
            let a: A = parse(args)?;
            let n = a.assessments.len();
            let warnings = crate::assess::record(&mut store.design, a.assessments, crate::store::now_ms())?;
            let m = compute(&store.view(), false);
            let disputes = m.issues.iter().filter(|i| i.kind == "disagreement").count();
            ok(
                format!(
                    "Recorded {n} assessment(s). Health now {}.{}{}",
                    m.health,
                    if disputes > 0 { format!(" {disputes} disagreement(s) with Spide's signals: raise them with the user.") } else { String::new() },
                    if warnings.is_empty() { String::new() } else { format!(" Warnings: {}", warnings.join("; ")) }
                ),
                true,
            )
        }
        "get_code" => Err("get_code is answered by the Spide app (it needs the imported code)".into()),
        "get_responsibilities" => ok(responsibilities_json(&store.view(), args), false),
        "get_overlap" => ok(overlap_json(&store.view(), args), false),
        "suggest_home" => {
            let v = store.view();
            let methods: Vec<String> = match (args.get("method").and_then(Value::as_str), args.get("group").and_then(Value::as_array)) {
                (_, Some(g)) if !g.is_empty() => g.iter().filter_map(Value::as_str).map(String::from).collect(),
                (Some(m), _) => vec![m.to_string()],
                _ => return Err("give method: \"Class.method\" or group: [\"A.m\", \"B.n\"]".into()),
            };
            let s = crate::home::suggest_home(&v, &methods)?;
            ok(serde_json::to_string(&s).unwrap(), false)
        }
        "set_use_cases" => {
            let m: UseCaseModel = parse(args)?;
            let warnings = store.set_use_cases(m).map_err(|e| e.0)?;
            ok(
                if warnings.is_empty() { "Use case diagram updated.".into() } else { format!("Updated with warnings: {}", warnings.join("; ")) },
                true,
            )
        }
        "set_change_impact" => {
            #[derive(Deserialize)]
            struct A {
                name: String,
                description: Option<String>,
                impacted: Vec<ImpactedClass>,
            }
            let a: A = parse(args)?;
            let name = a.name.clone();
            let warnings = store.set_change_impact(a.name, a.description, a.impacted).map_err(|e| e.0)?;
            let m = compute(&store.view(), false);
            let fp = m.changes.iter().find(|c| c.name == name);
            let mut text = match fp {
                Some(fp) => format!(
                    "Change scenario \"{name}\" stored: {} impacted, {} more depend on them, locality {:.0}%.",
                    fp.impacted.len(),
                    fp.ripple.len(),
                    fp.locality * 100.0
                ),
                None => format!("Change scenario \"{name}\" stored."),
            };
            if !warnings.is_empty() {
                text.push_str(&format!(" Warnings: {}.", warnings.join("; ")));
            }
            if fp.is_some_and(|fp| fp.locality < 0.5) {
                text.push_str(" Poor locality: consider raising a decision that encapsulates this variation point.");
            }
            ok(text, true)
        }
        "set_sequence" => {
            let s: Sequence = parse(args)?;
            let problems = store.set_sequence(s).map_err(|e| e.0)?;
            ok(
                if problems.is_empty() {
                    "Sequence diagram stored; all messages match the class model.".into()
                } else {
                    format!(
                        "Stored, but these messages don't match the class model and are highlighted: {}. \
                         Fix the sequence, or raise a propose_decision to add the missing methods.",
                        problems.join("; ")
                    )
                },
                true,
            )
        }
        _ => Err(format!("unknown tool {name}")),
    }
}

fn fmt_attr(a: &Attribute) -> String {
    let mut s = format!("{}{}: {}", a.visibility.glyph(), a.name, a.ty);
    if let Some(m) = &a.multiplicity {
        s.push_str(&format!(" [{m}]"));
    }
    if a.is_static {
        s.push_str(" {static}");
    }
    s
}

/// `get_responsibilities` over any design (the intended one, or the judged code).
pub fn responsibilities_json(d: &Design, args: &Value) -> String {
    let m = compute(d, false);
    let only = args.get("class").and_then(Value::as_str);
    let r = &m.responsibility;
    let classes: Vec<Value> = r
        .classes
        .iter()
        .filter(|(c, _)| only.is_none_or(|o| o == c.as_str()))
        .map(|(c, cr)| {
            json!({
                "class": c,
                "role": cr.role,
                "purpose": crate::assess::current(d, c).and_then(|a| a.purpose.clone()),
                "concerns": cr.concerns,
                "purity": (cr.purity * 100.0).round() / 100.0,
                "methods": r.methods.iter().filter(|mi| &mi.class == c).map(|mi| json!({
                    "method": mi.method, "concern": mi.concern, "rdd": mi.rdd, "source": mi.source, "reason": mi.reason,
                    "reads": mi.reads, "tells": mi.tells})).collect::<Vec<_>>(),
            })
        })
        .collect();
    let findings: Vec<&crate::metrics::Issue> = m
        .issues
        .iter()
        .filter(|i| i.principle == "responsibility" || i.principle == "cohesion")
        .filter(|i| only.is_none_or(|o| i.targets.iter().any(|t| t == o || t.starts_with(&format!("{o}.")))))
        .collect();
    serde_json::to_string(&json!({"classes": classes, "findings": findings})).unwrap()
}

/// `get_overlap` over any design.
pub fn overlap_json(d: &Design, args: &Value) -> String {
    let min = args.get("min_score").and_then(Value::as_f64).unwrap_or(0.5).clamp(0.3, 1.0);
    let only = args.get("class").and_then(Value::as_str);
    let r = crate::overlap::analyse(d, min);
    let mine = |s: &str| only.is_none_or(|o| s == o || s.starts_with(&format!("{o}.")));
    serde_json::to_string(&json!({
        "duplicate_threshold": crate::overlap::DUPLICATE,
        "pairs": r.pairs.iter().filter(|p| mine(&p.a) || mine(&p.b)).take(40).collect::<Vec<_>>(),
        "class_overlap": r.classes.iter().filter(|c| mine(&c.a) || mine(&c.b)).collect::<Vec<_>>(),
        "owned_elsewhere": r.claims.iter().filter(|c| mine(&c.method) || mine(&c.owner)).collect::<Vec<_>>(),
        "scattered_concepts": r.scattered.iter().filter(|s| only.is_none_or(|o| s.classes.iter().any(|c| c == o))).collect::<Vec<_>>(),
        "delegations": r.delegations.iter().filter(|x| mine(&x.from) || mine(&x.to)).collect::<Vec<_>>(),
        "middle_men": r.middle_men,
        "how_to_read": "score ≥ 0.6 = near-duplicate. Delegation (A.m only calls B.n) is not duplication. For each duplicate or owned_elsewhere entry, call suggest_home to find the one right place.",
    }))
    .unwrap()
}

pub fn fmt_method(m: &Method) -> String {
    let params: Vec<String> = m.params.iter().map(|p| format!("{}: {}", p.name, p.ty)).collect();
    let mut s = format!("{}{}({})", m.visibility.glyph(), m.name, params.join(", "));
    if let Some(r) = &m.returns {
        s.push_str(&format!(": {r}"));
    }
    if m.is_abstract {
        s.push_str(" {abstract}");
    }
    s
}

/// Compact, agent-friendly dump of the design.
pub fn design_digest(store: &Store) -> String {
    let d = &store.design;
    let live = |s: Status| s.is_live();
    let classes: Vec<Value> = d
        .classes
        .iter()
        .filter(|c| live(c.status))
        .map(|c| {
            let mut o = json!({
                "name": c.name,
                "attributes": c.attributes.iter().filter(|a| live(a.status)).map(fmt_attr).collect::<Vec<_>>(),
                "methods": c.methods.iter().filter(|m| live(m.status)).map(|m| {
                    let mut s = fmt_method(m);
                    if !m.uses.is_empty() { s.push_str(&format!(" uses[{}]", m.uses.join(","))); }
                    if !m.calls.is_empty() { s.push_str(&format!(" calls[{}]", m.calls.join(","))); }
                    s
                }).collect::<Vec<_>>(),
            });
            if let Some(s) = &c.stereotype {
                o["stereotype"] = json!(s);
            }
            if let Some(p) = &c.package {
                o["package"] = json!(p);
            }
            if c.is_abstract {
                o["abstract"] = json!(true);
            }
            if !c.responsibilities.is_empty() {
                o["responsibilities"] = json!(c.responsibilities);
            }
            if c.status == Status::Overruled {
                o["overruled"] = json!(true);
            }
            if c.external {
                o["external"] = json!(true);
            }
            if let Some(r) = &c.role {
                o["role"] = json!(r);
            }
            if let Some(a) = crate::assess::current(d, &c.name) {
                o["purpose"] = json!(a.purpose);
            }
            o
        })
        .collect();
    let relations: Vec<String> = d
        .live_relations()
        .map(|r| {
            let mut s = format!("{} -{}-> {}", r.from, r.kind.as_str(), r.to);
            if r.from_mult.is_some() || r.to_mult.is_some() {
                s.push_str(&format!(" [{}..{}]", r.from_mult.as_deref().unwrap_or(""), r.to_mult.as_deref().unwrap_or("")));
            }
            if let Some(l) = &r.label {
                s.push_str(&format!(" \"{l}\""));
            }
            if r.kind == RelationKind::Association && !r.directed {
                s.push_str(" (bidirectional)");
            }
            s
        })
        .collect();
    let mut pruned: Vec<Value> = vec![];
    for c in &d.classes {
        if c.status == Status::Pruned {
            pruned.push(json!({"class": c.name, "feedback": c.feedback}));
        } else {
            for a in c.attributes.iter().filter(|a| a.status == Status::Pruned) {
                pruned.push(json!({"attribute": format!("{}.{}", c.name, a.name)}));
            }
            for m in c.methods.iter().filter(|m| m.status == Status::Pruned) {
                pruned.push(json!({"method": format!("{}.{}()", c.name, m.name)}));
            }
        }
    }
    for r in d.relations.iter().filter(|r| r.status == Status::Pruned) {
        pruned.push(json!({"relation": r.id, "feedback": r.feedback}));
    }
    let feedback: Vec<Value> =
        d.feedback.iter().rev().take(12).map(|f| json!({"on": f.target, "user_said": f.text})).collect();
    let pending: Vec<Value> = d
        .decisions
        .iter()
        .filter(|x| x.is_pending())
        .map(|x| json!({"id": x.id, "question": x.question, "state": x.state, "options": x.options.iter().map(|o| format!("{}: {}", o.id, o.label)).collect::<Vec<_>>(),
                          "last_attempt": x.attempts.last()}))
        .collect();
    let decided: Vec<Value> = d
        .decisions
        .iter()
        .filter(|x| matches!(x.state, DecisionState::Accepted | DecisionState::Overruled))
        .map(|x| {
            let chosen = x
                .chosen
                .as_ref()
                .and_then(|c| x.options.iter().find(|o| &o.id == c))
                .map(|o| o.label.clone())
                .or_else(|| x.custom.clone());
            json!({"question": x.question, "chosen": chosen, "state": x.state})
        })
        .collect();
    let proposals: Vec<Value> =
        d.proposals.iter().map(|p| json!({"id": p.id, "ops": summarize(&p.ops), "rationale": p.rationale})).collect();
    let packages: Vec<Value> = d
        .packages
        .iter()
        .filter(|p| live(p.status))
        .map(|p| json!({"name": p.name, "layer": p.layer, "external": p.external}))
        .collect();
    let m = compute(&store.view(), false);
    let (stale, missing) = crate::assess::stale_and_missing(d);
    let forces: Vec<Value> = d
        .live_forces()
        .map(|f| {
            let mut o = json!({"name": f.name, "kind": f.kind, "priority": f.priority});
            if let Some(desc) = &f.description {
                o["description"] = json!(desc);
            }
            o
        })
        .collect();
    let st = stage(d);
    serde_json::to_string(&json!({
        "name": d.name,
        "mode": d.settings.partner,
        "stage": st,
        "stage_hint": stage_hint(st),
        "forces": forces,
        "rules": {
            "preset": d.rules.preset.as_str(),
            "meaning": d.rules.preset.describe(),
            "forbidden": d.rules.forbidden.iter().map(|r| json!({"from": r.from, "to": r.to, "reason": r.reason})).collect::<Vec<_>>(),
            "forbidden_types": d.rules.forbidden_types.iter().map(|r| json!({"layer": r.layer, "pattern": r.pattern})).collect::<Vec<_>>(),
        },
        "health": m.health,
        "top_issues": m.issues.iter().take(6).map(|i| &i.message).collect::<Vec<_>>(),
        "assessments": {"stale": stale, "missing": missing,
            "hint": "assess_class (batched) for stale/missing classes when you next touch them"},
        "packages": packages,
        "classes": classes,
        "relations": relations,
        "pending_proposals": proposals,
        "pending_decisions": pending,
        "decided": decided,
        "pruned_by_user": pruned,
        "recent_user_feedback": feedback,
        "use_cases": d.use_cases.use_cases.iter().filter(|u| live(u.status)).map(|u| &u.name).collect::<Vec<_>>(),
        "actors": d.use_cases.actors.iter().filter(|a| live(a.status)).map(|a| &a.name).collect::<Vec<_>>(),
        "sequences": d.sequences.iter().filter(|s| live(s.status)).map(|s| &s.name).collect::<Vec<_>>(),
        "containers": d.containers.iter().filter(|k| live(k.status)).map(|k| json!({"name": k.name, "technology": k.technology, "replicas": k.replicas,
            "datastore": k.datastore, "packages": d.packages.iter().filter(|p| live(p.status) && p.container.as_deref() == Some(&k.name)).map(|p| &p.name).collect::<Vec<_>>()})).collect::<Vec<_>>(),
        "external_systems": d.systems.iter().filter(|x| live(x.status)).map(|x| &x.name).collect::<Vec<_>>(),
        "links": d.links.iter().filter(|l| live(l.status)).map(|l| format!("{} -{}-> {}{}", l.from, format!("{:?}", l.interaction).to_lowercase(), l.to,
            l.protocol.as_ref().map(|p| format!(" [{p}]")).unwrap_or_default())).collect::<Vec<_>>(),
        "change_scenarios": m.changes.iter().map(|c| json!({"name": c.name, "analysed": c.analysed, "stale": c.stale,
            "impacted": c.impacted, "locality": (c.locality * 100.0).round()})).collect::<Vec<_>>(),
        "settings": {"examiner": d.settings.examiner},
    }))
    .unwrap()
}
