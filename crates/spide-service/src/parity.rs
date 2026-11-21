//! Domain operations the desktop UI offers, as agent tools (MCP parity): direct design edits,
//! prune/restore, rules and partner mode, history and branches, text exports, sync, and
//! desktop-only view commands (which fail with a structured error when headless).

use crate::{Host, Service, ToolResult};
use serde_json::{json, Value};
use spide_core::{Design, Op, Target};

pub fn tools() -> Vec<Value> {
    let ops = json!({"type":"array","items":{"type":"object"},"description":"Operations as in propose_ops."});
    vec![
        json!({"name":"apply_ops","description":"Apply design operations directly (autonomous only; the user's direct edits). Validated like proposals; traced; undoable.","inputSchema":{"type":"object","properties":{"ops":ops,"expected_rev":{"type":"integer"},"intent":{"type":"string"}},"required":["ops","expected_rev"]}}),
        json!({"name":"prune","description":"Hide elements (kept, the agent is told not to reintroduce them), with optional feedback. Targets: {kind:class|attribute|method|relation|package,…}.","inputSchema":{"type":"object","properties":{"targets":{"type":"array","items":{"type":"object"}},"feedback":{"type":"string"},"expected_rev":{"type":"integer"}},"required":["targets","expected_rev"]}}),
        json!({"name":"restore","description":"Restore a pruned element.","inputSchema":{"type":"object","properties":{"target":{"type":"object"},"expected_rev":{"type":"integer"}},"required":["target","expected_rev"]}}),
        json!({"name":"set_mode","description":"Partner mode: examiner | socratic | devils_advocate | sketch.","inputSchema":{"type":"object","properties":{"mode":{"type":"string"},"expected_rev":{"type":"integer"}},"required":["mode","expected_rev"]}}),
        json!({"name":"set_rules","description":"Update project architecture rules (owner only). Omitted fields are preserved. rules.private_documentation toggles the default-on private-method source documentation health check.","inputSchema":{"type":"object","properties":{"rules":{"type":"object"}},"required":["rules"]}}),
        json!({"name":"list_history","description":"Design timeline: revisions with actor and summary.","inputSchema":{"type":"object","properties":{"limit":{"type":"integer"}}},"annotations":{"readOnlyHint":true}}),
        json!({"name":"restore_revision","description":"Propose the changes that bring the design back to a past revision (the user approves).","inputSchema":{"type":"object","properties":{"rev":{"type":"integer"}},"required":["rev"]}}),
        json!({"name":"list_branches","description":"Design branches with class counts.","inputSchema":{"type":"object","properties":{}},"annotations":{"readOnlyHint":true}}),
        json!({"name":"create_branch","description":"Create a branch from the current design (or a past revision with `rev`) and switch to it unless `switch` is false.","inputSchema":{"type":"object","properties":{"name":{"type":"string"},"rev":{"type":"integer"},"switch":{"type":"boolean"},"expected_rev":{"type":"integer"}},"required":["name","expected_rev"]}}),
        json!({"name":"switch_branch","description":"Park the current design and load another branch.","inputSchema":{"type":"object","properties":{"name":{"type":"string"},"expected_rev":{"type":"integer"}},"required":["name","expected_rev"]}}),
        json!({"name":"compare_branch","description":"Changes, findings, and rule breaks from the current design to a branch.","inputSchema":{"type":"object","properties":{"name":{"type":"string"}},"required":["name"]},"annotations":{"readOnlyHint":true}}),
        json!({"name":"merge_branch","description":"Bring a branch's changes in as proposals (additions/changes and removals separately).","inputSchema":{"type":"object","properties":{"name":{"type":"string"}},"required":["name"]}}),
        json!({"name":"export","description":"Write a text export into the workspace: plantuml (class view), adr (one file per decision), archunit (Java tests), json (the design). `path` is workspace-relative.","inputSchema":{"type":"object","properties":{"format":{"type":"string","enum":["plantuml","adr","archunit","json"]},"path":{"type":"string"},"view":{"type":"string"},"package":{"type":"string","description":"PlantUML class view only: exact package plus direct incoming/outgoing boundary classes."}},"required":["format","path"]}}),
        json!({"name":"ui_command","description":"Run a desktop view command (/lens, /detail, /tidy, /focus …). Requires the desktop; headless returns an error.","inputSchema":{"type":"object","properties":{"command":{"type":"string"}},"required":["command"]}}),
    ]
}

fn ok(t: impl Into<String>) -> Result<ToolResult, String> {
    Ok(ToolResult::text(t))
}

impl Service {
    pub(crate) fn parity(&self, name: &str, args: &Value) -> Option<Result<ToolResult, String>> {
        let s = |k: &str| args.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        let root = self.root();
        Some(match name {
            "apply_ops" => (|| {
                let ops: Vec<Op> = serde_json::from_value(args.get("ops").cloned().unwrap_or(json!([]))).map_err(|e| format!("ops: {e}"))?;
                self.with_store(|st| st.apply_user(ops)).map_err(|e| e.0)?;
                self.commit("agent")?;
                ok("applied")
            })(),
            "prune" => (|| {
                let targets: Vec<Target> = serde_json::from_value(args.get("targets").cloned().unwrap_or(json!([]))).map_err(|e| format!("targets: {e}"))?;
                let fb = args.get("feedback").and_then(Value::as_str).map(str::to_string);
                let pruned = self.with_store(|st| st.prune(&targets, fb)).map_err(|e| e.0)?;
                self.commit("agent")?;
                ok(json!({"pruned": pruned}).to_string())
            })(),
            "restore" => (|| {
                let t: Target = serde_json::from_value(args.get("target").cloned().unwrap_or(Value::Null)).map_err(|e| format!("target: {e}"))?;
                self.with_store(|st| st.restore(&t)).map_err(|e| e.0)?;
                self.commit("agent")?;
                ok("restored")
            })(),
            "set_mode" => (|| {
                let mode: spide_core::model::PartnerMode = serde_json::from_value(json!(s("mode"))).map_err(|_| "mode: examiner | socratic | devils_advocate | sketch".to_string())?;
                self.with_store(|st| st.set_partner(mode));
                self.commit("agent")?;
                ok("ok")
            })(),
            "set_rules" => (|| {
                let patch = args.get("rules").and_then(Value::as_object).ok_or("rules must be an object")?;
                let mut value = self.with_store(|st| serde_json::to_value(&st.design.rules).unwrap());
                for (key, field) in patch { value[key] = field.clone(); }
                let rules: spide_core::rules::Rules = serde_json::from_value(value).map_err(|e| format!("rules: {e}"))?;
                self.with_store(|st| st.set_rules(rules)).map_err(|e| e.0)?;
                self.commit("user")?;
                ok("ok")
            })(),
            "list_history" => {
                let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(50) as usize;
                let all = crate::history::list(&root);
                let n = all.len();
                ok(json!({"total": n, "entries": all.into_iter().skip(n.saturating_sub(limit)).collect::<Vec<_>>()}).to_string())
            }
            "restore_revision" => (|| {
                let rev = args.get("rev").and_then(Value::as_u64).ok_or("rev required")?;
                let past = crate::history::at(&root, rev).ok_or_else(|| format!("no history at revision {rev}"))?;
                let pid = self.with_store(|st| {
                    let ops = spide_core::diff::diff(&st.design, &past);
                    if ops.is_empty() {
                        return Err("the design already looks like that".to_string());
                    }
                    st.propose_ops(ops, Some(format!("restore the design as of revision {rev}")), "history").map_err(|e| e.0)
                })?;
                self.commit("agent")?;
                ok(json!({"proposal": pid}).to_string())
            })(),
            "list_branches" => {
                let active = crate::branches::active(&root);
                let cur = self.with_store(|st| st.design.clone());
                let info = |n: &str, d: &Design, a: bool| json!({"name": n, "active": a, "classes": d.live_classes().count()});
                let mut out = vec![info(&active, &cur, true)];
                for n in crate::branches::list(&root).into_iter().filter(|n| *n != active) {
                    if let Ok(d) = crate::branches::read(&root, &n) {
                        out.push(info(&n, &d, false));
                    }
                }
                ok(Value::Array(out).to_string())
            }
            "create_branch" => (|| {
                let n = s("name");
                crate::branches::valid_name(&n)?;
                let active = crate::branches::active(&root);
                if n == active || crate::branches::exists(&root, &n) {
                    return Err(format!("branch {n} already exists"));
                }
                let d = match args.get("rev").and_then(Value::as_u64) {
                    Some(rev) => crate::history::at(&root, rev).ok_or_else(|| format!("no history at revision {rev}"))?,
                    None => self.with_store(|st| st.design.clone()),
                };
                crate::branches::write(&root, &n, &d)?;
                if args.get("switch").and_then(Value::as_bool).unwrap_or(true) {
                    self.switch_branch(&n)?;
                }
                ok(json!({"branch": n}).to_string())
            })(),
            "switch_branch" => self.switch_branch(&s("name")).map(|_| ToolResult::text("switched")),
            "compare_branch" => (|| {
                let other = crate::branches::read(&root, &s("name"))?;
                let cur = self.with_store(|st| st.design.clone());
                ok(serde_json::to_string(&spide_core::impact::impact(&cur, &other)).unwrap())
            })(),
            "merge_branch" => (|| {
                let n = s("name");
                let other = crate::branches::read(&root, &n)?;
                let ids = self.with_store(|st| {
                    let ops = spide_core::diff::diff(&st.design, &other);
                    if ops.is_empty() {
                        return Err(format!("{n} has nothing this design doesn't"));
                    }
                    let (removes, rest): (Vec<Op>, Vec<Op>) = ops.into_iter().partition(|o| matches!(o, Op::RemoveClass { .. } | Op::RemoveAttribute { .. } | Op::RemoveMethod { .. } | Op::RemoveRelation { .. } | Op::RemovePackage { .. } | Op::RemoveForce { .. } | Op::RemoveChangeScenario { .. }));
                    let mut ids = vec![];
                    if !rest.is_empty() {
                        ids.push(st.propose_ops(rest, Some(format!("merge from branch {n}: additions and changes")), "branch").map_err(|e| e.0)?);
                    }
                    if !removes.is_empty() {
                        ids.push(st.propose_ops(removes, Some(format!("merge from branch {n}: removals")), "branch").map_err(|e| e.0)?);
                    }
                    Ok(ids)
                })?;
                self.commit("agent")?;
                ok(json!({"proposals": ids}).to_string())
            })(),
            "export" => (|| {
                let path = s("path");
                if !spide_core::project::is_plain_relative(&path) {
                    return Err("path must be workspace-relative without `..`".into());
                }
                let d = self.with_store(|st| st.design.clone());
                let target = root.join(&path);
                if args.get("package").is_some() && (s("format") != "plantuml" || args.get("view").and_then(Value::as_str).unwrap_or("class") != "class") {
                    return Err("package scope is supported only for PlantUML class diagrams".into());
                }
                let written: Vec<String> = match s("format").as_str() {
                    "json" => {
                        crate::workspace::write_atomic(&target, &d.to_json()).map_err(|e| e.to_string())?;
                        vec![path.clone()]
                    }
                    "archunit" => {
                        crate::workspace::write_atomic(&target, &spide_core::archunit::archunit_test(&d)).map_err(|e| e.to_string())?;
                        vec![path.clone()]
                    }
                    "plantuml" => {
                        let m = spide_core::metrics::compute(&d, false);
                        let view = args.get("view").and_then(Value::as_str).unwrap_or("class");
                        let text = if let Some(package) = args.get("package").and_then(Value::as_str) {
                            spide_core::plantuml::export_package(&d, &m, package)?
                        } else { spide_core::plantuml::export(view, &d, &m, &d.layout)? };
                        crate::workspace::write_atomic(&target, &text).map_err(|e| e.to_string())?;
                        vec![path.clone()]
                    }
                    "adr" => {
                        let mut out = vec![];
                        for (name, text) in spide_core::adr::adrs(&d) {
                            crate::workspace::write_atomic(&target.join(&name), &text).map_err(|e| e.to_string())?;
                            out.push(format!("{path}/{name}"));
                        }
                        out
                    }
                    f => return Err(format!("unknown format {f}")),
                };
                Ok(ToolResult { text: json!({"written": written}).to_string(), files: written, ..Default::default() })
            })(),
            "ui_command" if self.host == Host::Headless => Err("ui_command needs the desktop (it changes what the canvas shows); start Spide and it will attach".into()),
            _ => return None,
        })
    }

    pub(crate) fn switch_branch(&self, name: &str) -> Result<(), String> {
        let root = self.root();
        let active = crate::branches::active(&root);
        if name == active {
            return Err(format!("already on {name}"));
        }
        let next = crate::branches::read(&root, name)?;
        let cur = self.with_store(|st| st.design.clone());
        crate::branches::write(&root, &active, &cur)?;
        self.with_store(|st| st.load(next));
        crate::branches::delete(&root, name)?;
        crate::branches::set_active(&root, name).map_err(|e| e.to_string())?;
        self.store_host().on_branch(name);
        self.commit("agent")
    }
}
