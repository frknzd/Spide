use super::*;

fn svc(dir: &Path) -> Service {
    let s = Service::open(dir, Host::Headless, None).unwrap();
    s.set_trust_override(Some(policy::ProjectTrust::default()));
    s
}

fn with(dir: &Path, permission: policy::Permission, trusted: bool, cmds: &[&str]) -> Service {
    let s = Service::open(dir, Host::Headless, None).unwrap();
    s.set_trust_override(Some(policy::ProjectTrust { permission, trusted, approved_commands: cmds.iter().map(|c| c.to_string()).collect() }));
    s
}

fn rev(s: &Service) -> u64 {
    s.with_store(|st| st.design.rev)
}

fn pending_decision(s: &Service) -> String {
    let r = s.call_tool("propose_decision", &json!({"question":"Add Order?","options":[{"label":"yes","ops":[{"op":"add_class","name":"Order"}]}]}), "agent");
    assert!(r.is_ok(), "{r:?}");
    s.with_store(|st| st.design.decisions.iter().find(|d| d.is_pending()).unwrap().id.clone())
}

fn rpc(s: &Service, id: u64, method: &str, params: Value) -> Value {
    mcp::handle(s, &json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})).unwrap()
}

#[test]
fn project_documentation_toggle_is_a_partial_persistent_rule_update() {
    let t = tempfile::tempdir().unwrap();
    let s = svc(t.path());
    s.with_store(|st| st.design.rules.thresholds.max_method_lines = 77);
    s.call_tool("set_rules", &json!({"rules":{"private_documentation":false}}), "user").unwrap();
    assert!(!s.with_store(|st| st.design.rules.private_documentation));
    assert_eq!(s.with_store(|st| st.design.rules.thresholds.max_method_lines), 77);
    drop(s);
    let s = svc(t.path());
    assert!(!s.with_store(|st| st.design.rules.private_documentation));
    assert_eq!(s.with_store(|st| st.design.rules.thresholds.max_method_lines), 77);
}

#[test]
fn scoped_text_exports_exclude_transitive_neighbors() {
    let t = tempfile::tempdir().unwrap();
    let s = svc(t.path());
    let d = spide_core::Design::from_json(&json!({"packages":[{"name":"ui"},{"name":"model"}],"classes":[
        {"name":"Panel","package":"ui"},{"name":"Boundary","package":"model"},{"name":"Hidden","package":"model"}],
        "relations":[{"id":"one","from":"Panel","to":"Boundary","kind":"dependency"},{"id":"two","from":"Boundary","to":"Hidden","kind":"dependency"}]
    }).to_string()).unwrap();
    s.with_store(|st| st.design = d);
    s.call_tool("export", &json!({"format":"plantuml","package":"ui","path":"ui.puml"}), "user").unwrap();
    let text = std::fs::read_to_string(t.path().join("ui.puml")).unwrap();
    assert!(text.contains("Panel") && text.contains("Boundary"));
    assert!(!text.contains("Hidden"));
    assert!(s.call_tool("export", &json!({"format":"json","package":"ui","path":"bad.json"}), "user").is_err());
}

#[test]
fn second_writer_is_refused_and_lock_released_on_drop() {
    let t = tempfile::tempdir().unwrap();
    let a = svc(t.path());
    let err = Service::open(t.path(), Host::Headless, None).err().unwrap();
    assert!(err.contains("attach"), "{err}");
    drop(a);
    svc(t.path());
}

#[test]
fn stale_lock_is_taken_over() {
    let t = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(t.path().join(".spide")).unwrap();
    // pid 0x7ffffffe is practically never alive
    std::fs::write(t.path().join(workspace::LOCK), "2147483646\nabc\n").unwrap();
    svc(t.path());
}

#[test]
fn mcp_initialize_list_and_call() {
    let t = tempfile::tempdir().unwrap();
    let s = svc(t.path());
    let init = rpc(&s, 1, "initialize", json!({"protocolVersion": mcp::PROTOCOL}));
    assert_eq!(init["result"]["serverInfo"]["name"], "spide");
    let list = rpc(&s, 2, "tools/list", json!({}));
    let names: Vec<&str> = list["result"]["tools"].as_array().unwrap().iter().filter_map(|t| t["name"].as_str()).collect();
    assert!(names.contains(&"get_design") && names.contains(&"workspace_info"));
    let r = rpc(&s, 3, "tools/call", json!({"name":"workspace_info","arguments":{}}));
    let info: Value = serde_json::from_str(r["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(info["host"], "headless");
    assert!(mcp::handle(&s, &json!({"jsonrpc":"2.0","method":"notifications/initialized"})).is_none());
    assert_eq!(rpc(&s, 4, "nope", json!({}))["error"]["code"], -32601);
}

#[test]
fn proposals_persist_and_emit_events() {
    let t = tempfile::tempdir().unwrap();
    let s = svc(t.path());
    let r = s.call_tool("propose_ops", &json!({"ops":[{"op":"add_class","name":"Order"}],"rationale":"start"}), "agent");
    assert!(r.is_ok(), "{r:?}");
    let saved = std::fs::read_to_string(t.path().join("design.json")).unwrap();
    assert!(saved.contains("Order"));
    let ev: Value = serde_json::from_str(&s.call_tool("get_events", &json!({}), "agent").unwrap()).unwrap();
    assert_eq!(ev["events"][0]["event"]["event"], "design_changed");
}

#[test]
fn existing_designs_open_unchanged() {
    let t = tempfile::tempdir().unwrap();
    let fixture = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures/library.json")).unwrap();
    std::fs::write(t.path().join("design.json"), &fixture).unwrap();
    let s = svc(t.path());
    assert!(!s.with_store(|st| st.design.classes.is_empty()));
}

#[test]
fn read_only_denies_and_traces() {
    let t = tempfile::tempdir().unwrap();
    let s = with(t.path(), policy::Permission::ReadOnly, false, &[]);
    assert!(s.call_tool("get_design", &json!({}), "agent").is_ok());
    let e = s.call_tool("propose_ops", &json!({"ops":[{"op":"add_class","name":"X"}]}), "agent").unwrap_err();
    assert!(e.contains("permission denied"));
    assert!(s.call_tool("set_permission", &json!({}), "agent").is_err());
    let tr: Value = serde_json::from_str(&s.call_tool("get_trace", &json!({"only":"failures"}), "agent").unwrap()).unwrap();
    assert_eq!(tr["total"], 2);
    assert!(!t.path().join("design.json").exists());
}

#[test]
fn propose_only_cannot_apply_autonomous_can_with_guard_and_trace() {
    let t = tempfile::tempdir().unwrap();
    {
        let s = svc(t.path());
        let id = pending_decision(&s);
        let e = s.call_tool("approve_decision", &json!({"id":id,"expected_rev":rev(&s)}), "agent").unwrap_err();
        assert!(e.contains("autonomous"), "{e}");
    }
    let s = with(t.path(), policy::Permission::Autonomous, false, &[]);
    let id = s.with_store(|st| st.design.decisions[0].id.clone());
    assert!(s.call_tool("approve_decision", &json!({"id":id}), "agent").unwrap_err().contains("expected_rev"));
    let stale = s.call_tool("approve_decision", &json!({"id":id,"expected_rev":rev(&s) + 7}), "agent").unwrap_err();
    assert!(stale.contains("conflict"));
    let r = rev(&s);
    s.call_tool("approve_decision", &json!({"id":id,"expected_rev":r,"idempotency_key":"k1","intent":"add the order aggregate"}), "agent").unwrap();
    assert!(s.with_store(|st| st.design.class("Order").is_some_and(|c| c.status.is_live())));
    // retry with the same key: not applied twice, no conflict
    let again = s.call_tool("approve_decision", &json!({"id":id,"expected_rev":r,"idempotency_key":"k1"}), "agent").unwrap();
    assert!(again.contains("already applied"));
    let tr: Value = serde_json::from_str(&s.call_tool("get_trace", &json!({"limit":5}), "agent").unwrap()).unwrap();
    let top = tr["rows"][0]["summary"].as_str().unwrap();
    assert!(top.starts_with("add the order aggregate → Order") && top.ends_with("ok"), "{top}");
    let conflicts: Value = serde_json::from_str(&s.call_tool("get_trace", &json!({"only":"conflicts"}), "agent").unwrap()).unwrap();
    assert_eq!(conflicts["total"], 1);
}

#[test]
fn crash_between_intent_and_outcome_is_reported() {
    let t = tempfile::tempdir().unwrap();
    {
        let s = svc(t.path());
        s.journal.append(trace::Record { op: "x".into(), kind: "intent".into(), tool: "approve_decision".into(), intent: "half done".into(), ..Default::default() }).unwrap();
    }
    let s = svc(t.path());
    let tr: Value = serde_json::from_str(&s.call_tool("get_trace", &json!({"only":"failures"}), "agent").unwrap()).unwrap();
    assert!(tr["rows"][0]["summary"].as_str().unwrap().contains("INTERRUPTED"));
}

#[test]
fn run_check_requires_trust_and_owner_approved_command() {
    let t = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(t.path().join("spide")).unwrap();
    std::fs::write(t.path().join("spide/project.json"), r#"{"version":1,"name":"x","language":"java","roots":["src"],"commands":{"test":{"run":"echo tests-pass"}}}"#).unwrap();
    {
        let s = with(t.path(), policy::Permission::Autonomous, false, &[]);
        assert!(s.call_tool("run_check", &json!({"name":"test"}), "agent").unwrap_err().contains("trusted"));
    }
    {
        let s = with(t.path(), policy::Permission::Autonomous, true, &[]);
        assert!(s.call_tool("run_check", &json!({"name":"test"}), "agent").unwrap_err().contains("not owner-approved"));
    }
    let s = with(t.path(), policy::Permission::Autonomous, true, &["echo tests-pass"]);
    let out: Value = serde_json::from_str(&s.call_tool("run_check", &json!({"name":"test"}), "agent").unwrap()).unwrap();
    let j = s.jobs.wait(out["job"].as_str().unwrap(), std::time::Duration::from_secs(5)).unwrap();
    assert_eq!(j.state, jobs::JobState::Succeeded);
    assert!(j.output.contains("tests-pass"));
}

#[test]
fn symbols_navigation_both_ways_and_safe_source() {
    let t = tempfile::tempdir().unwrap();
    let src = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../spide-import/tests/fixtures/poly/py");
    for f in ["shop/__init__.py", "shop/orders.py", "shop/pricing.py", "pyproject.toml"] {
        std::fs::create_dir_all(t.path().join(f).parent().unwrap()).unwrap();
        std::fs::copy(src.join(f), t.path().join(f)).unwrap();
    }
    let s = svc(t.path());
    let syms: Value = serde_json::from_str(&s.call_tool("get_symbols", &json!({"query":"order.subtotal"}), "agent").unwrap()).unwrap();
    assert_eq!(syms["total"], 1, "{syms}");
    // design element → source
    s.call_tool("propose_ops", &json!({"ops":[{"op":"add_class","name":"Order","methods":[{"name":"subtotal"}]}]}), "agent").unwrap();
    let id = s.with_store(|st| st.design.decisions.last().unwrap().id.clone());
    s.with_store(|st| { let r = st.design.rev; st.approve_decision(&id, r, String::new()).unwrap(); });
    let loc: Value = serde_json::from_str(&s.call_tool("find_symbol", &json!({"class":"Order","method":"subtotal"}), "agent").unwrap()).unwrap();
    assert_eq!(loc["file"], "shop/orders.py");
    let line = loc["line"].as_u64().unwrap();
    // source → design element
    let at: Value = serde_json::from_str(&s.call_tool("element_at", &json!({"file":"shop/orders.py","line":line + 1}), "agent").unwrap()).unwrap();
    assert_eq!((at["design_class"].as_str(), at["method"].as_str()), (Some("Order"), Some("subtotal")));
    let src: Value = serde_json::from_str(&s.call_tool("get_source", &json!({"class":"Order","method":"subtotal"}), "agent").unwrap()).unwrap();
    assert!(src["text"].as_str().unwrap().contains("def subtotal"));
    assert!(s.call_tool("get_source", &json!({"file":"../../etc/passwd"}), "agent").is_err());
    std::os::unix::fs::symlink("/etc", t.path().join("escape")).unwrap();
    assert!(s.call_tool("get_source", &json!({"file":"escape/hosts"}), "agent").unwrap_err().contains("outside"));
    let doc: Value = serde_json::from_str(&s.call_tool("doctor", &json!({}), "agent").unwrap()).unwrap();
    assert_eq!(doc["languages"][0], "python");
    assert_eq!(doc["can_run_tools"], false);
    assert!(s.call_tool("analyze", &json!({}), "agent").unwrap_err().contains("permission denied"));
}

#[test]
fn score_free_evidence_and_exception_flow_respects_policy() {
    let t = tempfile::tempdir().unwrap();
    let legacy_queue = t.path().join(".spide/assessments.json");
    std::fs::create_dir_all(legacy_queue.parent().unwrap()).unwrap();
    let old_request = r#"[{"id":"A1","dimensions":["modularity"],"reason":"old request"}]"#;
    std::fs::write(&legacy_queue, old_request).unwrap();
    let s = with(t.path(), policy::Permission::Autonomous, false, &[]);
    assert_eq!(std::fs::read_to_string(&legacy_queue).unwrap(), old_request);
    s.call_tool("propose_decision", &json!({"question":"model?","options":[{"label":"a","ops":[{"op":"add_class","name":"Order"},{"op":"add_class","name":"Line"},{"op":"add_relation","from":"Order","to":"Line","kind":"composition"},{"op":"add_relation","from":"Line","to":"Order","kind":"dependency"}]}]}), "agent").unwrap();
    let id = s.with_store(|st| st.design.decisions[0].id.clone());
    s.call_tool("approve_decision", &json!({"id":id,"expected_rev":rev(&s)}), "agent").unwrap();
    for name in ["get_assessment_requests", "request_assessment", "assess_health", "cancel_assessment", "set_priorities"] {
        assert!(s.call_tool(name, &json!({}), "agent").unwrap_err().contains("retired"));
    }
    let read = |name| serde_json::from_str::<Value>(&s.call_tool(name, &json!({}), "agent").unwrap()).unwrap();
    let before = read("get_health");
    assert_eq!(before["version"], "evidence-1");
    assert!(!before["gaps"].as_array().unwrap().is_empty());
    for key in ["score", "measured", "assisted", "dimensions", "coverage"] { assert!(before.get(key).is_none()); }
    let metrics = read("get_metrics");
    for key in ["health", "legacy_health", "principles"] { assert!(metrics.get(key).is_none()); }
    // exceptions: the agent proposes, only the owner approves
    let x: Value = serde_json::from_str(&s.call_tool("propose_exception", &json!({"rule":"cycle","scope":"Order","reason":"aggregate back-reference by design"}), "agent").unwrap()).unwrap();
    let xid = x["exception"].as_str().unwrap();
    assert!(s.call_tool("approve_exception", &json!({"id":xid}), "agent").unwrap_err().contains("owner-only"));
    assert!(s.call_tool("approve_exception", &json!({"id":xid}), "editor-user").is_err(), "not from the editor either");
    s.call_tool("approve_exception", &json!({"id":xid}), "user").unwrap();
    let ex: Value = serde_json::from_str(&s.call_tool("list_exceptions", &json!({}), "agent").unwrap()).unwrap();
    assert_eq!(ex[0]["approved"], true);
    assert!(read("get_health")["suppressed"].as_array().unwrap().iter().any(|f| f["suppressed_by"] == xid));
    assert!(s.journal.read().iter().all(|r| r.health_before.is_none() && r.health_after.is_none()));
}
