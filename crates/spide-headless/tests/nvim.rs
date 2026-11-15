//! End to end with a real headless Neovim running the bundled plugin (`integrations/nvim`) and a
//! real `spide-headless serve`: attachment, Spide → Neovim navigation, Neovim → Spide reveal,
//! guarded agent edits against dirty buffers, closed-file edits and revert, diagnostics
//! forwarding, and the user token. Skipped when `nvim` is not installed.

use serde_json::{json, Value};
use spide_service::nvim::Nvim;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_spide-headless");

fn which(n: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|p| std::env::split_paths(&p).map(|d| d.join(n)).find(|c| c.is_file()))
}

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn wait_for(what: &str, mut f: impl FnMut() -> bool) {
    let t = Instant::now();
    while !f() {
        assert!(t.elapsed() < Duration::from_secs(20), "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn post(url: &str, token: &str, body: &Value) -> Value {
    let rest = url.strip_prefix("http://").unwrap();
    let (hostport, path) = rest.split_once('/').unwrap();
    let mut s = std::net::TcpStream::connect(hostport).unwrap();
    let b = body.to_string();
    write!(s, "POST /{path} HTTP/1.1\r\nHost: {hostport}\r\nContent-Type: application/json\r\nAuthorization: Bearer {token}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{b}", b.len()).unwrap();
    let mut r = String::new();
    s.read_to_string(&mut r).unwrap();
    serde_json::from_str(r.split("\r\n\r\n").nth(1).unwrap()).unwrap()
}

struct Spide {
    url: String,
    token: String,
}

impl Spide {
    fn call(&self, tool: &str, args: Value) -> Result<Value, String> {
        let r = post(&self.url, &self.token, &json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":tool,"arguments":args}}));
        let text = r["result"]["content"][0]["text"].as_str().unwrap_or("").to_string();
        if r["result"]["isError"] == true {
            Err(text)
        } else {
            Ok(serde_json::from_str(&text).unwrap_or(Value::String(text)))
        }
    }
}

fn copy_fixture(to: &Path) {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../spide-import/tests/fixtures/poly/py");
    for f in ["shop/__init__.py", "shop/orders.py", "shop/pricing.py", "pyproject.toml"] {
        std::fs::create_dir_all(to.join(f).parent().unwrap()).unwrap();
        std::fs::copy(src.join(f), to.join(f)).unwrap();
    }
}

fn lua(n: &Nvim, code: &str) -> Value {
    spide_service::nvim::to_json(&n.call("nvim_exec_lua", vec![rmpv::Value::from(code), rmpv::Value::Array(vec![])]).unwrap())
}

#[test]
fn neovim_bridge_end_to_end() {
    let Some(nvim) = which("nvim") else {
        eprintln!("nvim not installed: skipped");
        return;
    };
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("proj");
    std::fs::create_dir_all(&root).unwrap();
    copy_fixture(&root);
    let root = root.canonicalize().unwrap();
    let state = t.path().join("state");
    let trust = t.path().join("trust.json");
    std::fs::write(&trust, serde_json::to_string(&json!({"version":1,"projects":{root.display().to_string():{"permission":"read_only","trusted":false,"approved_commands":[]}}})).unwrap()).unwrap();
    let env = |c: &mut Command| {
        c.env("XDG_STATE_HOME", &state).env("SPIDE_TRUST_FILE", &trust).env_remove("SPIDE_PROJECT");
    };
    let mut c = Command::new(BIN);
    c.args(["--project", root.to_str().unwrap(), "serve"]).stderr(Stdio::null());
    env(&mut c);
    let _server = Kill(c.spawn().unwrap());
    let ep_path = root.join(".spide/endpoint.json");
    wait_for("endpoint", || ep_path.exists());
    let ep: Value = serde_json::from_str(&std::fs::read_to_string(&ep_path).unwrap()).unwrap();
    let sp = Spide { url: ep["mcp_url"].as_str().unwrap().into(), token: ep["token"].as_str().unwrap().into() };

    // Neovim with the bundled plugin, the user's config ignored
    let sock = t.path().join("nvim.sock");
    let plugin = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../integrations/nvim").canonicalize().unwrap();
    let mut n = Command::new(nvim);
    n.current_dir(&root)
        .args(["--headless", "--clean", "--listen", sock.to_str().unwrap(), "--cmd", &format!("set rtp^={}", plugin.display()), "-c", "runtime plugin/spide.lua", "-c", "lua require('spide').setup({})"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    env(&mut n);
    let _nvim = Kill(n.spawn().unwrap());
    wait_for("nvim socket", || sock.exists());
    let rpc = Nvim::connect(&sock, Duration::from_secs(10)).unwrap();

    // 1. attachment: VimEnter auto-connect finds the endpoint and registers
    wait_for("editor registration", || sp.call("get_editor", json!({})).unwrap()["sessions"].as_array().is_some_and(|s| !s.is_empty()));
    let sessions = sp.call("get_editor", json!({})).unwrap();
    assert_eq!(sessions["sessions"][0]["kind"], "external");
    assert_eq!(lua(&rpc, "return require('spide').state.user_token ~= nil"), json!(true), "Spide pushed the user token over RPC");

    // 2. Spide → Neovim: open a method at its exact range
    let loc = sp.call("find_symbol", json!({"symbol":"shop.orders.Order.subtotal"})).unwrap();
    sp.call("open_source", json!({"symbol":"shop.orders.Order.subtotal"})).unwrap();
    let here = lua(&rpc, "return {vim.api.nvim_buf_get_name(0), vim.api.nvim_win_get_cursor(0)[1]}");
    assert!(here[0].as_str().unwrap().ends_with("shop/orders.py"), "{here}");
    assert_eq!(here[1], loc["line"]);

    // 3. Neovim → Spide: reveal the element under the cursor
    lua(&rpc, "vim.api.nvim_win_set_cursor(0, {vim.api.nvim_win_get_cursor(0)[1] + 2, 0}) require('spide').reveal() return true");
    let ev = sp.call("get_events", json!({})).unwrap();
    let reveals: Vec<&Value> = ev["events"].as_array().unwrap().iter().filter(|e| e["event"]["event"] == "reveal").collect();
    assert_eq!(reveals.last().unwrap()["event"]["symbol"], "Order.subtotal", "{ev}");

    // 4. an agent edit never overwrites a dirty buffer
    let file = "shop/orders.py";
    let disk_hash = sp.call("get_source", json!({"file": file})).unwrap()["hash"].as_str().unwrap().to_string();
    lua(&rpc, "vim.api.nvim_buf_set_lines(0, 0, 1, false, {'# edited in neovim'}) return true");
    let mut trust_v: Value = serde_json::from_str(&std::fs::read_to_string(&trust).unwrap()).unwrap();
    trust_v["projects"][root.display().to_string()]["permission"] = json!("autonomous");
    std::fs::write(&trust, trust_v.to_string()).unwrap();
    let e = sp.call("apply_patch", json!({"file": file, "edits":[{"start_line":2,"end_line":2,"text":"# agent"}], "expected_hash": disk_hash})).unwrap_err();
    assert!(e.contains("conflict") && e.contains("unsaved"), "{e}");
    // against the buffer's text it applies inside Neovim (undoable), not on disk
    let buf_text = lua(&rpc, &format!("return require('spide').buffer_text('{}').text", root.join(file).display()));
    let buf_hash = spide_service::editor::hash(buf_text.as_str().unwrap());
    let ok = sp.call("apply_patch", json!({"file": file, "edits":[{"start_line":2,"end_line":2,"text":"# agent"}], "expected_hash": buf_hash})).unwrap();
    assert!(ok["via"].as_str().unwrap().starts_with("neovim"), "{ok}");
    assert_eq!(lua(&rpc, "return vim.api.nvim_buf_get_lines(0, 0, 2, false)"), json!(["# edited in neovim", "# agent"]));
    assert!(!std::fs::read_to_string(root.join(file)).unwrap().contains("# agent"), "disk untouched: Neovim owns the buffer");
    lua(&rpc, "vim.cmd('silent undo') return true");
    assert_eq!(lua(&rpc, "return vim.api.nvim_buf_get_lines(0, 0, 2, false)"), json!(["# edited in neovim", ""]), "the agent's edit is its own undo step");

    // 5. closed files: atomic write with a hash guard, revert as a guarded change
    let pf = "shop/pricing.py";
    let h = sp.call("get_source", json!({"file": pf})).unwrap()["hash"].as_str().unwrap().to_string();
    let applied = sp.call("apply_patch", json!({"file": pf, "edits":[{"start_line":1,"end_line":0,"text":"# header"}], "expected_hash": h, "intent":"add header"})).unwrap();
    assert_eq!(applied["via"], "disk");
    assert!(std::fs::read_to_string(root.join(pf)).unwrap().starts_with("# header\n"));
    sp.call("revert_op", json!({"op": applied["revert"]})).unwrap();
    assert!(!std::fs::read_to_string(root.join(pf)).unwrap().contains("# header"));
    assert!(sp.call("revert_op", json!({"op": applied["revert"]})).unwrap_err().contains("conflict"), "reverting twice is a conflict");
    let trace = sp.call("get_trace", json!({"limit": 10})).unwrap();
    assert!(trace["rows"].as_array().unwrap().iter().any(|r| r["summary"].as_str().unwrap().contains("add header")), "{trace}");

    // 6. live diagnostics from Neovim's LSP become evidence (unsaved buffers are labelled)
    lua(&rpc, "local ns = vim.api.nvim_create_namespace('t') vim.diagnostic.set(ns, 0, {{lnum=3, col=4, severity=1, source='Pyright', code='reportUndefinedVariable', message='x is undefined'}}) return true");
    wait_for("forwarded diagnostics", || sp.call("get_evidence", json!({"file": file})).unwrap()["total"].as_u64() == Some(1));
    let f = &sp.call("get_evidence", json!({"file": file})).unwrap()["findings"][0];
    assert_eq!((f["provider"].as_str(), f["category"].as_str(), f["range"]["line"].as_u64()), (Some("editor-unsaved"), Some("correctness"), Some(4)));

    // 7. human actions from Neovim carry the user token: allowed even when agents are read-only
    trust_v["projects"][root.display().to_string()]["permission"] = json!("read_only");
    std::fs::write(&trust, trust_v.to_string()).unwrap();
    let h = sp.call("get_source", json!({"file": pf})).unwrap()["hash"].as_str().unwrap().to_string();
    assert!(sp.call("apply_patch", json!({"file": pf, "edits":[{"start_line":1,"end_line":0,"text":"# x"}], "expected_hash": h})).unwrap_err().contains("permission denied"));
    let r = lua(&rpc, &format!("local r, e = require('spide').call('apply_patch', {{file='{pf}', edits={{{{start_line=1,end_line=0,text='# by user'}}}}, expected_hash='{h}'}}, {{user=true}}) return {{r ~= nil, e}}"));
    assert_eq!(r[0], json!(true), "{r}");
    assert!(std::fs::read_to_string(root.join(pf)).unwrap().starts_with("# by user"));

    // 8. handoff to another session refuses to strand unsaved buffers unless told what to do
    lua(&rpc, "vim.api.nvim_buf_set_lines(0, 0, 1, false, {'# dirty again'}) return true");
    let sid = sp.call("get_editor", json!({})).unwrap()["sessions"][0]["id"].as_str().unwrap().to_string();
    let other = Command::new(which("nvim").unwrap()).args(["--headless", "--clean", "--listen", t.path().join("n2.sock").to_str().unwrap()]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
    let other = Kill(other);
    wait_for("second nvim", || t.path().join("n2.sock").exists());
    let r2 = post(&sp.url, &sp.token, &json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"editor_register","arguments":{"socket": t.path().join("n2.sock").display().to_string(), "pid": other.0.id(), "kind":"external"}}}));
    let sid2 = serde_json::from_str::<Value>(r2["result"]["content"][0]["text"].as_str().unwrap()).unwrap()["session"]["id"].as_str().unwrap().to_string();
    let e = lua(&rpc, &format!("local r, e = require('spide').call('editor_handoff', {{to='{sid2}'}}, {{user=true}}) return e"));
    assert!(e.as_str().unwrap().contains("unsaved"), "{e}");
    let ok = lua(&rpc, &format!("local r, e = require('spide').call('editor_handoff', {{to='{sid2}', dirty='save'}}, {{user=true}}) return r and r.preferred or e"));
    assert_eq!(ok.as_str(), Some(sid2.as_str()), "{ok}");
    assert!(std::fs::read_to_string(root.join(file)).unwrap().starts_with("# dirty again"), "saved by the handoff");
    let _ = sid;
}

#[test]
fn plugin_unit_suite_in_isolated_neovim() {
    let Some(nvim) = which("nvim") else { return };
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../integrations/nvim/tests/run.lua");
    let out = Command::new(nvim).args(["--headless", "--clean", "-l"]).arg(&script).output().unwrap();
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success() && !text.contains("FAIL"), "{text}");
}
