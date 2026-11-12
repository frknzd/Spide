//! Agent-facing tool API (served over MCP). Pure functions over the store.

use crate::derive;
use crate::metrics::{compute, Metrics};
use crate::model::*;
use crate::ops::Op;
use crate::store::{summarize, Store};
use serde::Deserialize;
use serde_json::{json, Value};

pub const INSTRUCTIONS: &str = "Spide is a live class-diagram design canvas. The user can only see and accept/prune; \
you change the design exclusively through these tools. Everything you add is a PROPOSAL the user must accept \
(propose_ops) or a DECISION the user must choose and defend (propose_decision). Read get_design first.";

const OPS_DOC: &str = r#"Each op is an object with an "op" field:
- {"op":"add_class","name":"Order","stereotype?":"entity|value|enum|interface|abstract|service|controller|repository|…","package?":"domain","is_abstract?":false,
   "attributes?":[ATTR…],"methods?":[METHOD…],"responsibilities?":["…"],"rationale?":"why it exists"}
- {"op":"update_class","name":"Order", any of "stereotype","package","is_abstract","responsibilities","rationale"}
- {"op":"rename_class","from":"A","to":"B"}            - {"op":"remove_class","name":"A"}
- {"op":"add_attribute","class":"Order","attribute":ATTR}   - {"op":"update_attribute","class":"Order","name":"total","attribute":ATTR}
- {"op":"remove_attribute","class":"Order","name":"total"}
- {"op":"add_method","class":"Order","method":METHOD}       - {"op":"update_method","class":"Order","name":"pay","method":METHOD}
- {"op":"remove_method","class":"Order","name":"pay"}
- {"op":"add_relation","from":"Order","to":"OrderLine","kind":"association|aggregation|composition|inheritance|realization|dependency",
   "label?":"contains","from_mult?":"1","to_mult?":"1..*","directed?":true,"rationale?":"…"}
   Direction: composition/aggregation from=WHOLE to=PART; inheritance/realization from=CHILD to=PARENT; association/dependency from=CLIENT to=SUPPLIER.
- {"op":"update_relation","from","to","kind", any of "label","from_mult","to_mult","directed"}
- {"op":"remove_relation","from":"A","to":"B","kind?":"…"}
- {"op":"add_package","name":"domain","layer?":"ui|application|domain|infrastructure"}  - {"op":"set_layer","package":"…","layer":"…"|null}
- {"op":"move_to_package","class":"Order","package":"domain"}   - {"op":"remove_package","name":"…"}
ATTR   = {"name":"total","type":"Money","visibility?":"public|private|protected|package","is_static?":false,"multiplicity?":"0..1"}
METHOD = {"name":"addLine","params":[{"name":"p","type":"Product"},{"name":"qty","type":"int"}],"returns?":"OrderLine","visibility?":"public",
          "is_abstract?":false,"uses?":["lines"] (attributes of the same class it touches — drives the cohesion metric),
          "calls?":["Product.price","recalculate"] (methods it calls — drives coupling), "description?":"…"}
Batches are atomic and may reference classes added earlier in the same batch."#;

pub fn tool_list() -> Value {
    let ops_schema = json!({"type":"array","description": OPS_DOC,"items":{"type":"object","properties":{"op":{"type":"string"}},"required":["op"]}});
    json!([
        {"name":"get_design","description":"Read the whole design: classes with members, relations, packages, pending proposals and decisions, \
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
                "ops": ops_schema},"required":["label","ops"]}}},
          "required":["question","options"]}},
        {"name":"propose_ops","description":"Propose small, unambiguous changes (e.g. adding obvious attributes, fixing a multiplicity). They appear as \
           ghosts the user accepts with one click or prunes. Use propose_decision instead for anything the user should think about.",
         "inputSchema":{"type":"object","properties":{"ops": ops_schema,"rationale":{"type":"string"}},"required":["ops"]}},
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

fn health_delta(before: &Metrics, after: &Metrics) -> String {
    let mut s = format!("Health if accepted: {} → {}.", before.health, after.health);
    let old: Vec<&String> = before.issues.iter().map(|i| &i.message).collect();
    let new: Vec<&str> = after.issues.iter().filter(|i| !old.contains(&&i.message)).map(|i| i.message.as_str()).collect();
    if !new.is_empty() {
        s.push_str(&format!(" New issues: {}.", new.join("; ")));
    }
    s
}

pub fn call(store: &mut Store, name: &str, args: &Value) -> Result<ToolOutcome, String> {
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
                .map(|(i, o)| match store.preview(&o.ops, "x") {
                    Ok(p) => format!("{}: health {}", (b'A' + i as u8) as char, compute(&p, true).health),
                    Err(_) => String::new(),
                })
                .collect();
            let id = store.propose_decision(a.question, a.context, a.options).map_err(|e| e.0)?;
            ok(
                format!(
                    "Decision {id} is now shown to the user with live previews (current health {}; {}). \
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
            }
            let a: A = parse(args)?;
            let before = compute(&store.view(), true);
            let summary = summarize(&a.ops);
            let id = store.propose_ops(a.ops, a.rationale, "agent").map_err(|e| e.0)?;
            let after = compute(&store.view(), true);
            ok(format!("Proposal {id} shown as ghosts ({summary}). {}", health_delta(&before, &after)), true)
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
        "set_use_cases" => {
            let m: UseCaseModel = parse(args)?;
            let warnings = store.set_use_cases(m).map_err(|e| e.0)?;
            ok(
                if warnings.is_empty() { "Use case diagram updated.".into() } else { format!("Updated with warnings: {}", warnings.join("; ")) },
                true,
            )
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
        .map(|p| json!({"name": p.name, "layer": p.layer}))
        .collect();
    let m = compute(&store.view(), false);
    serde_json::to_string(&json!({
        "name": d.name,
        "health": m.health,
        "top_issues": m.issues.iter().take(6).map(|i| &i.message).collect::<Vec<_>>(),
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
        "settings": {"examiner": d.settings.examiner},
    }))
    .unwrap()
}
