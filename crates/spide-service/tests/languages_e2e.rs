//! The agent workflow end to end, once per language: index → symbols → navigate both ways →
//! health of the code → read source → guarded edit (applied, traced, re-indexed) → stale edit
//! refused → revert → staged patch applied by the user. Real LSP/linters are exercised by
//! `spide-lsp`'s `real_tools` and Neovim by `spide-headless`'s `nvim` tests.

use serde_json::{json, Value};
use spide_service::{policy, Host, Service};
use std::path::{Path, PathBuf};

fn copy_dir(from: &Path, to: &Path) {
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let p = e.path();
        let t = to.join(e.file_name());
        if p.is_dir() {
            if e.file_name() == "test" {
                continue;
            }
            std::fs::create_dir_all(&t).unwrap();
            copy_dir(&p, &t);
        } else {
            std::fs::copy(&p, &t).unwrap();
        }
    }
}

fn call(s: &Service, tool: &str, args: Value, actor: &str) -> Value {
    let t = s.call_tool(tool, &args, actor).unwrap_or_else(|e| panic!("{tool}: {e}"));
    serde_json::from_str(&t).unwrap_or(Value::String(t))
}

/// (language, fixture dir, function to work on (name), file it lives in)
const CASES: [(&str, &str, &str, &str); 7] = [
    ("rust", "poly/rust", "subtotal", "src/orders.rs"),
    ("typescript", "poly/ts", "subtotal", "src/orders.ts"),
    ("python", "poly/py", "subtotal", "shop/orders.py"),
    ("java", "library", "fine", "src/main/java/org/acme/library/domain/Loan.java"),
    ("c", "poly/c", "order_subtotal", "src/orders.c"),
    ("cpp", "poly/cpp", "subtotal", "src/orders.cpp"),
    ("clojure", "poly/clj", "subtotal", "src/shop/orders.clj"),
];

#[test]
fn agent_workflow_in_every_language() {
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../spide-import/tests/fixtures");
    for (lang, dir, func, file) in CASES {
        let t = tempfile::tempdir().unwrap();
        copy_dir(&fixtures.join(dir), t.path());
        let s = Service::open(t.path(), Host::Headless, None).unwrap();
        s.set_trust_override(Some(policy::ProjectTrust { permission: policy::Permission::Autonomous, trusted: false, approved_commands: vec![] }));

        // 1. index + symbols
        let syms = call(&s, "get_symbols", json!({"query": func, "limit": 50}), "agent");
        let hit = syms["symbols"].as_array().unwrap().iter().find(|x| x["file"] == file && x["name"].as_str().is_some_and(|n| n == func || n.starts_with(&format!("{func}(")))).unwrap_or_else(|| panic!("{lang}: {func} not in {syms}")).clone();
        assert_eq!(syms["languages"][0], lang, "{lang}");

        // 2. navigation both ways
        let loc = call(&s, "find_symbol", json!({"symbol": hit["qualified"]}), "agent");
        assert_eq!(loc["file"], file, "{lang}");
        let line = loc["line"].as_u64().unwrap();
        let at = call(&s, "element_at", json!({"file": file, "line": line}), "agent");
        assert!(at["method"].as_str().is_some_and(|m| m == func || m.starts_with(&format!("{func}("))), "{lang}: {at}");

        // 3. health of the code (no design yet): measured, honest about unknown implementation quality
        let h = call(&s, "get_health", json!({}), "agent");
        assert_eq!(h["scope"], "code", "{lang}: {h}");
        assert_eq!(h["version"], "evidence-1");
        assert!(!h["gaps"].as_array().unwrap().is_empty(), "{lang}");

        // 4. read + guarded edit: a comment line after the function's first line
        let src = call(&s, "get_source", json!({"file": file, "start_line": 1, "end_line": 400}), "agent");
        let hash = src["hash"].as_str().unwrap().to_string();
        let comment = match lang {
            "python" => "        # reviewed by the agent",
            "clojure" => "  ;; reviewed by the agent",
            _ => "    // reviewed by the agent",
        };
        let edit = json!([{"start_line": line + 1, "end_line": line, "text": comment}]);
        let applied = call(&s, "apply_patch", json!({"file": file, "edits": edit, "expected_hash": hash, "intent": format!("annotate {func}")}), "agent");
        assert_eq!(applied["via"], "disk");
        assert!(std::fs::read_to_string(t.path().join(file)).unwrap().contains("reviewed by the agent"));
        // the index follows the edit (the function moved by one line or grew)
        let loc2 = call(&s, "find_symbol", json!({"symbol": hit["qualified"]}), "agent");
        assert_eq!(loc2["file"], file);
        assert!(loc2["end_line"].as_u64() >= loc["end_line"].as_u64(), "{lang}: {loc} → {loc2}");
        // a stale hash is a conflict, never a blind write
        let e = s.call_tool("apply_patch", &json!({"file": file, "edits": edit, "expected_hash": hash}), "agent").unwrap_err();
        assert!(e.contains("conflict"), "{lang}: {e}");
        let tr = call(&s, "get_trace", json!({"limit": 5}), "agent");
        assert!(tr["rows"][1]["summary"].as_str().unwrap().contains(&format!("annotate {func}")) || tr["rows"][0]["summary"].as_str().unwrap().contains(&format!("annotate {func}")), "{lang}: {tr}");

        // 5. revert as a new guarded change
        call(&s, "revert_op", json!({"op": applied["revert"]}), "agent");
        assert!(!std::fs::read_to_string(t.path().join(file)).unwrap().contains("reviewed by the agent"));

        // 6. propose-only agents stage; the user applies
        s.set_trust_override(Some(policy::ProjectTrust::default()));
        let hash = call(&s, "get_source", json!({"file": file}), "agent")["hash"].as_str().unwrap().to_string();
        let p = call(&s, "propose_patch", json!({"file": file, "edits": edit, "expected_hash": hash, "intent": "staged"}), "agent");
        assert!(s.call_tool("apply_patch", &json!({"id": p["patch"]}), "agent").unwrap_err().contains("permission denied"), "{lang}");
        call(&s, "apply_patch", json!({"id": p["patch"]}), "user");
        assert!(std::fs::read_to_string(t.path().join(file)).unwrap().contains("reviewed by the agent"), "{lang}");
        println!("{lang:<11} ok: {} → {}:{line}", hit["qualified"], file);
    }
}
