//! Lucid sync: local state files (`.spide/lucid/`) and the agent tools that drive push and pull.
//!
//! Spide never talks to Lucid itself: the agent (pi with the Lucid MCP server) reads documents with
//! Spide's read script, hands the result to these tools, and runs the edit scripts they return.

use spide_core::lucid::state::{self, Base, Config};
use std::path::{Path, PathBuf};

pub fn dir(root: &Path) -> PathBuf {
    root.join(".spide").join("lucid")
}

pub fn load_config(root: &Path) -> Config {
    std::fs::read_to_string(dir(root).join("config.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

pub fn save_config(root: &Path, cfg: &Config) -> Result<(), String> {
    crate::workspace::write_atomic(&dir(root).join("config.json"), &serde_json::to_string_pretty(cfg).unwrap()).map_err(|e| format!("can't write the Lucid config: {e}"))
}

fn base_path(root: &Path, key: &str) -> PathBuf {
    dir(root).join("base").join(format!("{}.json", state::file_key(key)))
}

pub fn load_base(root: &Path, key: &str) -> Option<Base> {
    std::fs::read_to_string(base_path(root, key)).ok().and_then(|t| serde_json::from_str(&t).ok())
}

pub fn save_base(root: &Path, base: &Base) -> Result<(), String> {
    crate::workspace::write_atomic(&base_path(root, &base.key), &serde_json::to_string(base).unwrap()).map_err(|e| format!("can't write the Lucid sync state: {e}"))
}

pub fn remove_base(root: &Path, key: &str) {
    let _ = std::fs::remove_file(base_path(root, key));
}

/// Sync is only for the frontier branch of an enabled project.
pub fn guard(root: &Path) -> Result<Config, state::Refusal> {
    let cfg = load_config(root);
    state::check_branch(&cfg, &crate::branches::active(root))?;
    Ok(cfg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use spide_core::lucid::snapshot::Snapshot;

    #[test]
    fn state_files_round_trip_and_branch_guard() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        assert_eq!(guard(root).unwrap_err().code, "disabled");
        let mut cfg = load_config(root);
        cfg.enabled = true;
        cfg.documents.insert("class".into(), state::DocLink { document_id: "d1".into(), url: "u".into(), title: "t".into() });
        save_config(root, &cfg).unwrap();
        assert_eq!(load_config(root), cfg);
        assert!(guard(root).is_ok());
        crate::branches::set_active(root, "experiment").unwrap();
        assert_eq!(guard(root).unwrap_err().code, "not_frontier");
        let b = state::after_pull("sequence:Borrow book", "d1", Snapshot::default(), 3, 9, vec![]);
        save_base(root, &b).unwrap();
        assert_eq!(load_base(root, "sequence:Borrow book"), Some(b));
        assert!(dir(root).join("base").read_dir().unwrap().count() == 1);
        remove_base(root, "sequence:Borrow book");
        assert_eq!(load_base(root, "sequence:Borrow book"), None);
    }
}

// ------------------------------------------------------------------ agent tools

use crate::{Service, ToolResult};
use serde_json::{json, Value};
use spide_core::lucid::{diagram_keys, diagram_title, map, patch, snapshot, Target};

pub fn tools() -> Vec<Value> {
    let key = json!({"type":"string","description":"diagram: class | class.package:<exact package name> (one-hop boundary view) | domain | package | usecase | system.context | system.containers | sequence:<name>"});
    let snap = json!({"type":"object","description":"the result of running lucid_read_script on the document (lucid_run_script), passed through unchanged"});
    vec![
        json!({"name":"lucid_read_script","description":"The JavaScript Spide reads a Lucid document with: run it with the Lucid MCP's lucid_run_script (document_id; load the `uml` script API for the document first) and pass the result to lucid_status / lucid_plan_push / lucid_pull / lucid_record_push.",
            "inputSchema":{"type":"object","properties":{}},"annotations":{"readOnlyHint":true}}),
        json!({"name":"lucid_status","description":"Git-like sync status of every diagram's Lucid document: in_sync, local_ahead (push), remote_ahead (pull), diverged (pull first), unlinked (push creates it). Pass `snapshots` {key: read result} for the documents you read, otherwise the remote side is unknown.",
            "inputSchema":{"type":"object","properties":{"snapshots":{"type":"object"}}},"annotations":{"readOnlyHint":true}}),
        json!({"name":"lucid_export","description":"What Spide wants in a diagram's Lucid document: the items (UML classes with compartments, typed relationships, C4 blocks, sequence markup …) and the scripts that create them in an empty document.",
            "inputSchema":{"type":"object","properties":{"key":key},"required":["key"]},"annotations":{"readOnlyHint":true}}),
        json!({"name":"lucid_plan_push","description":"Plan an in-place push of one diagram: checks the branch and that the document still matches the last sync (else refuses: pull first), then returns the edit scripts (chunks) to run in order with lucid_run_script. `create: true` means: create a blank document titled `title` first. Then read the document again and call lucid_record_push.",
            "inputSchema":{"type":"object","properties":{"key":key,"snapshot":snap},"required":["key"]},"annotations":{"readOnlyHint":true}}),
        json!({"name":"lucid_record_push","description":"After running a push's scripts: verify the document now matches the design (read it with lucid_read_script) and record it as the new sync base. Fails, recording nothing, if edits are missing (rerun the plan: scripts are idempotent).",
            "inputSchema":{"type":"object","properties":{"key":key,"document_id":{"type":"string"},"url":{"type":"string"},"snapshot":snap,"version":{"type":"integer"}},"required":["key","document_id","snapshot"]}}),
        json!({"name":"lucid_pull","description":"Bring colleagues' Lucid edits of one diagram in: changes since the last sync become ghost proposals (additions/changes, removals, conflicts, moved boxes, separately); comment threads and sticky notes become comments (get_comments). Nothing is applied until the user accepts.",
            "inputSchema":{"type":"object","properties":{"key":key,"snapshot":snap,"version":{"type":"integer"}},"required":["key","snapshot"]}}),
        json!({"name":"lucid_link","description":"Enable Lucid sync for this project (optionally: frontier_branch, folder_id) and/or link a diagram to an existing Lucid document (key + document_id/url) — then lucid_pull imports it.",
            "inputSchema":{"type":"object","properties":{"key":key,"document_id":{"type":"string"},"url":{"type":"string"},"frontier_branch":{"type":"string"},"folder_id":{"type":"string"},"enabled":{"type":"boolean"}}}}),
        json!({"name":"lucid_unlink","description":"Forget a diagram's Lucid document (the document itself is not touched).",
            "inputSchema":{"type":"object","properties":{"key":key},"required":["key"]}}),
    ]
}

fn ok(v: Value) -> Result<ToolResult, String> {
    Ok(ToolResult::text(v.to_string()))
}

fn refused(r: state::Refusal) -> String {
    json!({"refused": r.code, "message": r.message}).to_string()
}

fn snap_arg(args: &Value) -> Result<Option<snapshot::Snapshot>, String> {
    match args.get("snapshot") {
        None | Some(Value::Null) => Ok(None),
        Some(v) => {
            let mut s = snapshot::from_raw(v)?;
            s.version = args.get("version").and_then(Value::as_u64);
            Ok(Some(s))
        }
    }
}

fn doc_id_from(args: &Value) -> Option<String> {
    let id = args.get("document_id").and_then(Value::as_str).map(str::to_string);
    id.or_else(|| {
        // https://lucid.app/lucidchart/<uuid>/edit
        let url = args.get("url").and_then(Value::as_str)?;
        url.split('/').find(|p| p.len() == 36 && p.chars().filter(|c| *c == '-').count() == 4).map(str::to_string)
    })
}

impl Service {
    fn lucid_target(&self, key: &str, anchor: Option<&snapshot::Snapshot>) -> Result<Target, String> {
        let d = self.with_store(|s| s.design.clone());
        spide_diagram::lucid::target_anchored(&d, key, anchor).ok_or_else(|| format!("nothing to draw for {key} (diagrams: {})", diagram_keys(&d).join(", ")))
    }

    fn pending_of(&self, base: Option<&Base>) -> Vec<String> {
        let open: Vec<String> = self.with_store(|s| s.design.proposals.iter().map(|p| p.id.clone()).collect());
        base.map(|b| b.pending.iter().filter(|p| open.contains(p)).cloned().collect()).unwrap_or_default()
    }

    pub(crate) fn lucid(&self, name: &str, args: &Value) -> Option<Result<ToolResult, String>> {
        let root = self.root();
        let key = args.get("key").and_then(Value::as_str).unwrap_or("").to_string();
        Some(match name {
            "lucid_read_script" => Ok(ToolResult::text(snapshot::READ_SCRIPT)),
            "lucid_status" => (|| {
                let cfg = load_config(&root);
                let branch = crate::branches::active(&root);
                let gate = state::check_branch(&cfg, &branch).err();
                let d = self.with_store(|s| s.design.clone());
                let mut keys = diagram_keys(&d);
                for k in cfg.documents.keys() {
                    if !keys.contains(k) {
                        keys.push(k.clone());
                    }
                }
                let snaps = args.get("snapshots").and_then(Value::as_object);
                let mut rows = vec![];
                for k in keys {
                    let base = load_base(&root, &k);
                    let remote = match snaps.and_then(|m| m.get(&k)) {
                        Some(v) => Some(snapshot::from_raw(v)?),
                        None => None,
                    };
                    // local changes are measured against the base (a colleague's move is a remote change)
                    let anchor = base.as_ref().map(|b| &b.snapshot).or(remote.as_ref());
                    let target = spide_diagram::lucid::target_anchored(&d, &k, anchor);
                    let th = target.as_ref().map(state::target_hash).unwrap_or_default();
                    let st = state::status(base.as_ref(), &th, remote.as_ref().map(|r| r.fingerprint()).as_deref());
                    let link = cfg.documents.get(&k);
                    rows.push(json!({"key": k, "title": diagram_title(&d.name, &k), "status": st.as_str(), "remote_checked": remote.is_some(),
                        "document_id": link.map(|l| &l.document_id), "url": link.map(|l| &l.url), "pending_proposals": self.pending_of(base.as_ref()),
                        "in_design": target.is_some()}));
                }
                ok(json!({"enabled": cfg.enabled, "frontier_branch": cfg.frontier_branch, "branch": branch, "sync_allowed": gate.is_none(),
                    "reason": gate.map(|g| g.message), "folder_id": cfg.folder_id, "diagrams": rows}))
            })(),
            "lucid_export" => (|| {
                let t = self.lucid_target(&key, load_base(&root, &key).as_ref().map(|b| &b.snapshot))?;
                let p = patch::plan(&snapshot::Snapshot::default(), &t);
                let chunks: Vec<String> = patch::chunks(&p, patch::CHUNK).iter().map(patch::script).collect();
                ok(json!({"key": key, "title": t.title, "items": t.items, "create_scripts": chunks}))
            })(),
            "lucid_plan_push" => (|| {
                let cfg = guard(&root).map_err(refused)?;
                let base = load_base(&root, &key);
                let link = cfg.documents.get(&key).cloned();
                let remote = snap_arg(args)?;
                if link.is_some() && remote.is_none() {
                    return Err("this diagram has a Lucid document: read it with lucid_read_script and pass the result as `snapshot`".into());
                }
                let remote = remote.unwrap_or_default();
                let t = self.lucid_target(&key, link.is_some().then_some(&remote))?;
                let local = match &base {
                    Some(b) => self.lucid_target(&key, Some(&b.snapshot))?,
                    None => t.clone(),
                };
                let st = state::status(base.as_ref(), &state::target_hash(&local), link.as_ref().map(|_| remote.fingerprint()).as_deref());
                state::can_push(st, &self.pending_of(base.as_ref())).map_err(refused)?;
                let p = patch::plan(&remote, &t);
                let chunks: Vec<String> = patch::chunks(&p, patch::CHUNK).iter().map(patch::script).collect();
                ok(json!({"key": key, "title": t.title, "status": st.as_str(), "create": link.is_none(), "document_id": link.as_ref().map(|l| &l.document_id),
                    "folder_id": cfg.folder_id, "ops": p.ops.len(), "chunks": chunks,
                    "next": if p.is_empty() { "nothing to change; call lucid_record_push with the snapshot to record the sync" } else { "run each chunk with lucid_run_script in order, read the document again, then lucid_record_push" }}))
            })(),
            "lucid_record_push" => (|| {
                let mut cfg = guard(&root).map_err(refused)?;
                let doc = doc_id_from(args).ok_or("document_id required")?;
                let remote = snap_arg(args)?.ok_or("snapshot required")?;
                let t = self.lucid_target(&key, Some(&remote))?;
                let left = patch::plan(&remote, &t);
                if !left.is_empty() {
                    return Err(format!("the document doesn't match the design yet ({} edits missing, first: {}); nothing recorded — plan the push again (scripts are idempotent)", left.ops.len(), left.ops[0].sid()));
                }
                let rev = self.with_store(|s| s.design.rev);
                let url = args.get("url").and_then(Value::as_str).map(str::to_string).or_else(|| cfg.documents.get(&key).map(|l| l.url.clone())).unwrap_or_else(|| format!("https://lucid.app/lucidchart/{doc}/edit"));
                cfg.documents.insert(key.clone(), state::DocLink { document_id: doc.clone(), url: url.clone(), title: t.title.clone() });
                save_config(&root, &cfg)?;
                save_base(&root, &state::after_push(&key, &doc, &t, remote, rev, spide_core::store::now_ms()))?;
                ok(json!({"key": key, "status": "in_sync", "url": url}))
            })(),
            "lucid_pull" => (|| {
                let mut cfg = guard(&root).map_err(refused)?;
                let doc = doc_id_from(args).or_else(|| cfg.documents.get(&key).map(|l| l.document_id.clone())).ok_or("this diagram has no Lucid document (lucid_link it, or push to create one)")?;
                let remote = snap_arg(args)?.ok_or("snapshot required")?;
                let base = load_base(&root, &key);
                let d = self.with_store(|s| s.design.clone());
                // never synced (linked to an existing document): compare with what Spide would write
                let base_items = match &base {
                    Some(b) => b.snapshot.items.clone(),
                    None => spide_diagram::lucid::target(&d, &key).map(|t| t.items).unwrap_or_default(),
                };
                let p = map::pull(&d, &key, &base_items, &remote);
                let title = cfg.documents.get(&key).map(|l| l.title.clone()).unwrap_or_else(|| diagram_title(&d.name, &key));
                let ids = if p.is_empty() { vec![] } else { self.with_store(|s| map::propose(s, &title, &p)).map_err(|e| e.0)? };
                let comments = map::comments(&key, &remote, &p.notes);
                let new_comments = self.with_store(|s| s.upsert_comments(comments)).map_err(|e| e.0)?;
                if !ids.is_empty() || new_comments > 0 {
                    self.commit("lucid")?;
                }
                if !cfg.documents.contains_key(&key) {
                    cfg.documents.insert(key.clone(), state::DocLink { document_id: doc.clone(), url: format!("https://lucid.app/lucidchart/{doc}/edit"), title: title.clone() });
                    save_config(&root, &cfg)?;
                }
                let rev = self.with_store(|s| s.design.rev);
                let now = spide_core::store::now_ms();
                let nb = if ids.is_empty() && base.is_none() {
                    // nothing to bring in: the document is what Spide would write
                    let t = self.lucid_target(&key, Some(&remote))?;
                    state::after_push(&key, &doc, &t, remote, rev, now)
                } else {
                    state::after_pull(&key, &doc, remote, rev, now, ids.clone())
                };
                save_base(&root, &nb)?;
                ok(json!({"key": key, "proposals": ids, "changes": p.changes.len(), "removals": p.removals.len(), "conflicts": p.conflicts.len(), "moved": p.layout.len(),
                    "comments_new_or_updated": new_comments, "warnings": p.warnings,
                    "next": if ids.is_empty() { "nothing to review" } else { "the user reviews the proposals on the canvas; after deciding them, push to bring the document up to date" }}))
            })(),
            "lucid_link" => (|| {
                let mut cfg = load_config(&root);
                cfg.enabled = args.get("enabled").and_then(Value::as_bool).unwrap_or(true);
                if let Some(b) = args.get("frontier_branch").and_then(Value::as_str) {
                    crate::branches::valid_name(b)?;
                    cfg.frontier_branch = b.to_string();
                }
                if let Some(f) = args.get("folder_id").and_then(Value::as_str) {
                    cfg.folder_id = Some(f.to_string()).filter(|f| !f.is_empty());
                }
                if !key.is_empty() {
                    let doc = doc_id_from(args).ok_or("document_id or url required to link a diagram")?;
                    let title = self.with_store(|s| diagram_title(&s.design.name, &key));
                    let url = args.get("url").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| format!("https://lucid.app/lucidchart/{doc}/edit"));
                    if cfg.documents.get(&key).is_some_and(|l| l.document_id != doc) {
                        remove_base(&root, &key);
                    }
                    cfg.documents.insert(key.clone(), state::DocLink { document_id: doc, url, title });
                }
                save_config(&root, &cfg)?;
                ok(json!({"enabled": cfg.enabled, "frontier_branch": cfg.frontier_branch, "folder_id": cfg.folder_id, "documents": cfg.documents}))
            })(),
            "lucid_unlink" => (|| {
                let mut cfg = load_config(&root);
                cfg.documents.remove(&key).ok_or_else(|| format!("{key} has no Lucid document"))?;
                remove_base(&root, &key);
                save_config(&root, &cfg)?;
                ok(json!({"unlinked": key}))
            })(),
            _ => return None,
        })
    }
}

#[cfg(test)]
mod flow_tests {
    //! The push/pull flow against a fake Lucid that applies Spide's patches the way the scripts do
    //! and answers reads in the read script's format.
    use super::*;
    use crate::{policy, Host};
    use spide_core::lucid::patch::{Patch, PatchOp};
    use spide_core::lucid::{Item, ItemKind};

    #[derive(Default, Clone)]
    struct FakeLucid {
        items: Vec<Item>,
        next: u32,
    }

    impl FakeLucid {
        fn apply(&mut self, p: &Patch) {
            for op in &p.ops {
                match op {
                    PatchOp::Add { item } => {
                        if self.items.iter().any(|i| i.sid == item.sid) {
                            continue;
                        }
                        self.next += 1;
                        let mut it = item.clone();
                        it.lucid_id = Some(format!("L{}", self.next));
                        it.fill = None;
                        if let ItemKind::Class { interface, .. } = &mut it.kind {
                            *interface = false;
                        }
                        self.items.push(it);
                    }
                    PatchOp::Retag { from, to } => self.items.iter_mut().filter(|i| i.sid.as_deref() == Some(from)).for_each(|i| i.sid = Some(to.clone())),
                    PatchOp::Update { sid, kind } => self.items.iter_mut().filter(|i| i.sid.as_deref() == Some(sid)).for_each(|i| {
                        i.kind = kind.clone();
                        if let ItemKind::Class { interface, .. } = &mut i.kind {
                            *interface = false;
                        }
                    }),
                    PatchOp::Move { sid, x, y, size } => self.items.iter_mut().filter(|i| i.sid.as_deref() == Some(sid)).for_each(|i| {
                        let b = i.bbox.get_or_insert_default();
                        b.x = *x;
                        b.y = *y;
                        if let Some((w, h)) = size {
                            b.w = *w;
                            b.h = *h;
                        }
                    }),
                    PatchOp::Delete { sid } => self.items.retain(|i| i.sid.as_deref() != Some(sid)),
                }
            }
        }
        /// The read script's result.
        fn read(&self) -> Value {
            let lid = |sid: &str| self.items.iter().find(|i| i.sid.as_deref() == Some(sid)).and_then(|i| i.lucid_id.clone()).unwrap_or_else(|| sid.trim_start_matches("lucid:").to_string());
            let items: Vec<Value> = self
                .items
                .iter()
                .map(|i| {
                    let mut o = json!({"id": i.lucid_id, "bbox": i.bbox.map(|b| json!({"x": b.x, "y": b.y, "w": b.w, "h": b.h})), "data": i.sid.as_ref().map(|s| json!({"spide.id": s})).unwrap_or(json!({}))});
                    match &i.kind {
                        ItemKind::Class { name, attributes, operations, .. } => {
                            o["kind"] = json!("block");
                            o["className"] = json!("UMLClassBlock");
                            o["umlType"] = json!("class");
                            o["compartments"] = json!({"name": name, "attributes": attributes, "operations": operations});
                        }
                        ItemKind::Package { name, body } => {
                            o["kind"] = json!("block");
                            o["className"] = json!("UMLPackageBlock");
                            o["textAreas"] = json!({"Title": name, "Text": body});
                        }
                        ItemKind::Relationship { from, to, rel, label, from_mult, to_mult } => {
                            o["kind"] = json!("line");
                            o["from"] = json!(lid(from));
                            o["to"] = json!(lid(to));
                            o["rel"] = json!({"type": rel, "label": label, "fromMultiplicity": from_mult, "toMultiplicity": to_mult, "from": lid(from), "to": lid(to)});
                        }
                        ItemKind::Other { class_name, text } => {
                            o["kind"] = json!("block");
                            o["className"] = json!(class_name);
                            o["label"] = json!(text);
                        }
                        k => panic!("fake Lucid doesn't draw {k:?}"),
                    }
                    o
                })
                .collect();
            json!({"pageId": "0_0", "items": items, "threads": []})
        }
    }

    fn call(s: &Service, name: &str, args: Value) -> Result<Value, String> {
        s.call_tool(name, &args, "agent").map(|t| serde_json::from_str(&t).unwrap_or(Value::String(t)))
    }

    fn push(s: &Service, lucid: &mut FakeLucid) -> Result<Value, String> {
        let plan = call(s, "lucid_plan_push", json!({"key": "class", "snapshot": if lucid.items.is_empty() && lucid.next == 0 { Value::Null } else { lucid.read() }}))?;
        // the fake applies the plan the scripts encode
        let base = load_base(&s.root(), "class");
        let remote = snapshot::from_raw(&lucid.read()).unwrap();
        let anchor = if plan["create"] == true { None } else { Some(&remote) };
        let t = spide_diagram::lucid::target_anchored(&s.with_store(|x| x.design.clone()), "class", anchor).unwrap();
        lucid.apply(&patch::plan(&remote, &t));
        let _ = base;
        call(s, "lucid_record_push", json!({"key": "class", "document_id": "doc-1", "snapshot": lucid.read()}))
    }

    fn status(s: &Service, lucid: &FakeLucid) -> String {
        let st = call(s, "lucid_status", json!({"snapshots": {"class": lucid.read()}})).unwrap();
        st["diagrams"].as_array().unwrap().iter().find(|d| d["key"] == "class").unwrap()["status"].as_str().unwrap().to_string()
    }

    fn refusal(r: Result<Value, String>) -> String {
        let e = r.expect_err("refused");
        serde_json::from_str::<Value>(&e).map(|v| v["refused"].as_str().unwrap_or_default().to_string()).unwrap_or(e)
    }

    #[test]
    fn git_like_push_and_pull() {
        let t = tempfile::tempdir().unwrap();
        let s = Service::open(t.path(), Host::Headless, None).unwrap();
        s.set_trust_override(Some(policy::ProjectTrust::default()));
        s.call_tool("propose_ops", &json!({"ops": [{"op":"add_class","name":"Order","package":"domain","attributes":[{"name":"total","type":"Money"}]},{"op":"add_class","name":"Line","package":"domain"},{"op":"add_relation","from":"Order","to":"Line","kind":"composition","to_mult":"1..*"}]}), "agent").unwrap();
        let pid = s.with_store(|x| x.design.proposals[0].id.clone());
        s.with_store(|x| x.accept_proposal(&pid).unwrap());
        // disabled until linked/enabled
        assert_eq!(refusal(call(&s, "lucid_plan_push", json!({"key": "class"}))), "disabled");
        call(&s, "lucid_link", json!({})).unwrap();
        let mut lucid = FakeLucid::default();
        // first push creates the document
        let plan = call(&s, "lucid_plan_push", json!({"key": "class"})).unwrap();
        assert_eq!(plan["create"], true);
        assert!(plan["chunks"][0].as_str().unwrap().contains("skills.uml.addShape"));
        push(&s, &mut lucid).unwrap();
        assert_eq!(status(&s, &lucid), "in_sync");
        // local change → local_ahead → push in place
        s.with_store(|x| x.apply_user(vec![serde_json::from_value(json!({"op":"add_attribute","class":"Order","attribute":{"name":"placed","type":"Instant"}})).unwrap()]).unwrap());
        assert_eq!(status(&s, &lucid), "local_ahead");
        let before = lucid.items.iter().find(|i| i.sid.as_deref() == Some("class:Order")).unwrap().lucid_id.clone();
        push(&s, &mut lucid).unwrap();
        assert_eq!(status(&s, &lucid), "in_sync");
        assert_eq!(lucid.items.iter().find(|i| i.sid.as_deref() == Some("class:Order")).unwrap().lucid_id, before, "updated in place");
        // a colleague edits: remote_ahead, push refused
        if let ItemKind::Class { attributes, .. } = &mut lucid.items.iter_mut().find(|i| i.sid.as_deref() == Some("class:Line")).unwrap().kind {
            attributes.push("- qty: int".into());
        }
        lucid.items.push(Item { sid: None, lucid_id: Some("N1".into()), kind: ItemKind::Other { class_name: "StickyNoteBlock".into(), text: "Line needs a price".into() }, bbox: None, fill: None });
        assert_eq!(status(&s, &lucid), "remote_ahead");
        assert_eq!(refusal(push(&s, &mut lucid)), "remote_ahead");
        // pull: proposals + a comment; push waits for the proposals
        let pulled = call(&s, "lucid_pull", json!({"key": "class", "snapshot": lucid.read()})).unwrap();
        assert_eq!(pulled["proposals"].as_array().unwrap().len(), 1, "{pulled}");
        assert_eq!(pulled["comments_new_or_updated"], 1);
        assert_eq!(s.with_store(|x| x.design.proposals[0].source.clone()), "lucid");
        assert_eq!(refusal(push(&s, &mut lucid)), "pending_proposals");
        let pid = pulled["proposals"][0].as_str().unwrap().to_string();
        s.with_store(|x| x.accept_proposal(&pid).unwrap());
        assert!(s.with_store(|x| x.design.class("Line").unwrap().attributes.iter().any(|a| a.name == "qty")));
        push(&s, &mut lucid).unwrap();
        assert_eq!(status(&s, &lucid), "in_sync");
        // the colleague's note is still there
        assert!(lucid.items.iter().any(|i| i.lucid_id.as_deref() == Some("N1")));
        // both sides change: diverged, refused
        s.with_store(|x| x.apply_user(vec![serde_json::from_value(json!({"op":"add_class","name":"Customer"})).unwrap()]).unwrap());
        if let ItemKind::Class { operations, .. } = &mut lucid.items.iter_mut().find(|i| i.sid.as_deref() == Some("class:Order")).unwrap().kind {
            operations.push("+ cancel()".into());
        }
        assert_eq!(status(&s, &lucid), "diverged");
        assert_eq!(refusal(push(&s, &mut lucid)), "diverged");
        // a partial push records nothing (the base stays, so the status stays diverged)
        let base_before = load_base(&s.root(), "class");
        let mut partial = lucid.clone();
        partial.items.retain(|i| i.sid.as_deref() != Some("class:Line"));
        assert!(call(&s, "lucid_record_push", json!({"key": "class", "document_id": "doc-1", "snapshot": partial.read()})).unwrap_err().contains("missing"));
        assert_eq!(load_base(&s.root(), "class"), base_before);
        // another branch never syncs
        crate::branches::set_active(&s.root(), "experiment").unwrap();
        assert_eq!(refusal(call(&s, "lucid_pull", json!({"key": "class", "snapshot": lucid.read()}))), "not_frontier");
        let st = call(&s, "lucid_status", json!({})).unwrap();
        assert_eq!(st["sync_allowed"], false);
    }

    #[test]
    fn tools_are_listed_and_classified() {
        for t in tools() {
            let n = t["name"].as_str().unwrap();
            assert!(policy::classify(n).is_some(), "{n} has no policy class");
        }
    }
}
