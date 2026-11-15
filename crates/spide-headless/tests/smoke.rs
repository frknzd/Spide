//! Protocol smoke tests against the real binary: stdio MCP and authenticated HTTP.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_spide-headless");

fn env<'a>(cmd: &'a mut Command, home: &std::path::Path) -> &'a mut Command {
    cmd.env("XDG_STATE_HOME", home.join("state")).env("SPIDE_TRUST_FILE", home.join("trust.json")).env_remove("SPIDE_PROJECT")
}

#[test]
fn stdio_initialize_list_call_and_second_writer() {
    let t = tempfile::tempdir().unwrap();
    let mut child = env(Command::new(BIN).args(["--project", t.path().to_str().unwrap(), "mcp"]), t.path()).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut out = BufReader::new(child.stdout.take().unwrap());
    let mut ask = |m: Value| {
        writeln!(stdin, "{m}").unwrap();
        let mut l = String::new();
        out.read_line(&mut l).unwrap();
        serde_json::from_str::<Value>(&l).unwrap()
    };
    assert_eq!(ask(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}))["result"]["serverInfo"]["name"], "spide");
    let tools = ask(json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}));
    assert!(tools["result"]["tools"].as_array().unwrap().len() > 15);
    let r = ask(json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"propose_ops","arguments":{"ops":[{"op":"add_class","name":"Order"}]}}}));
    assert_eq!(r["result"]["isError"], false, "{r}");
    // a second writer is refused while the first runs
    let second = env(Command::new(BIN).args(["--project", t.path().to_str().unwrap(), "call", "get_design"]), t.path()).output().unwrap();
    assert!(!second.status.success());
    assert!(String::from_utf8_lossy(&second.stderr).contains("attach"));
    drop(stdin);
    child.wait().unwrap();
    // released: the trace CLI shows the traced proposal
    let tr = env(Command::new(BIN).args(["--project", t.path().to_str().unwrap(), "trace"]), t.path()).output().unwrap();
    assert!(String::from_utf8_lossy(&tr.stdout).contains("propose_ops"), "{}", String::from_utf8_lossy(&tr.stdout));
}

fn post(port: u16, token: Option<&str>, body: &str) -> (u16, String) {
    let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    let auth = token.map(|t| format!("Authorization: Bearer {t}\r\n")).unwrap_or_default();
    write!(s, "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\n{auth}Content-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
    let mut r = String::new();
    s.read_to_string(&mut r).unwrap();
    let code = r.split_whitespace().nth(1).unwrap().parse().unwrap();
    (code, r.split("\r\n\r\n").nth(1).unwrap_or("").to_string())
}

#[test]
fn http_requires_bearer_token_from_discovery() {
    let t = tempfile::tempdir().unwrap();
    let mut child = env(Command::new(BIN).args(["--project", t.path().to_str().unwrap(), "serve"]), t.path()).stderr(Stdio::null()).spawn().unwrap();
    let ep_path = t.path().join(".spide/endpoint.json");
    let start = Instant::now();
    while !ep_path.exists() && start.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(20));
    }
    let ep: Value = serde_json::from_str(&std::fs::read_to_string(&ep_path).unwrap()).unwrap();
    let port: u16 = ep["mcp_url"].as_str().unwrap().rsplit(':').next().unwrap().trim_end_matches("/mcp").parse().unwrap();
    let body = r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#;
    assert_eq!(post(port, None, body).0, 401);
    assert_eq!(post(port, Some("wrong"), body).0, 401);
    let (code, text) = post(port, ep["token"].as_str(), body);
    assert_eq!(code, 200, "{text}");
    child.kill().unwrap();
    child.wait().unwrap();
}

#[test]
fn headless_hands_the_workspace_over_and_takes_it_back() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().canonicalize().unwrap();
    let spawn = || env(Command::new(BIN).args(["--project", root.to_str().unwrap(), "serve"]), t.path()).stderr(Stdio::null()).spawn().unwrap();
    let mut a = spawn();
    let ep_path = root.join(".spide/endpoint.json");
    let read_ep = || serde_json::from_str::<Value>(&std::fs::read_to_string(&ep_path).unwrap_or_default()).ok();
    let start = Instant::now();
    while read_ep().is_none() && start.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(20));
    }
    let ep_a = read_ep().unwrap();
    let port = |ep: &Value| -> u16 { ep["mcp_url"].as_str().unwrap().rsplit(':').next().unwrap().trim_end_matches("/mcp").parse().unwrap() };
    let call = |ep: &Value, tool: &str, args: Value| -> Value {
        let body = json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":tool,"arguments":args}}).to_string();
        let (_, text) = post(port(ep), ep["token"].as_str(), &body);
        serde_json::from_str(&text).unwrap()
    };
    // a "desktop" takes over: A yields and its lock is gone
    std::env::set_var("XDG_STATE_HOME", t.path().join("state"));
    let lock = spide_service::discovery::take_over_from_headless(&root).expect("A yields");
    drop(lock);
    let mut b = spawn(); // the new owner (stands in for the desktop)
    let start = Instant::now();
    while read_ep().is_none_or(|e| e["pid"] == ep_a["pid"]) && start.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(20));
    }
    let ep_b = read_ep().unwrap();
    assert_ne!(ep_b["pid"], ep_a["pid"]);
    // agents still talking to A are forwarded to B
    let info = call(&ep_a, "workspace_info", json!({}));
    let text: Value = serde_json::from_str(info["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(text["writer"], true, "answered by the writer (B)");
    // B leaves: A takes the workspace back
    b.kill().unwrap();
    b.wait().unwrap();
    let info = call(&ep_a, "workspace_info", json!({}));
    let text: Value = serde_json::from_str(info["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(text["writer"], true, "A reacquired");
    a.kill().unwrap();
    a.wait().unwrap();
}
