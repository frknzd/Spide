//! Spawns the user's `pi` in RPC mode and streams its output to the UI.

use crate::state::{AppState, PiStatus};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tauri::Emitter;

pub const SKILL: &str = include_str!("../../agent/spide-skill.md");

struct Proc {
    child: Child,
    stdin: Arc<Mutex<ChildStdin>>,
    generation: u64,
}

#[derive(Default)]
pub struct PiBridge {
    proc: Mutex<Option<Proc>>,
    generation: Arc<AtomicU64>,
    req: AtomicU64,
}

fn pi_binary() -> PathBuf {
    if let Ok(p) = std::env::var("SPIDE_PI") {
        return PathBuf::from(p);
    }
    if let Some(home) = dirs::home_dir() {
        let p = home.join(".pi/agent/bin/pi");
        if p.exists() {
            return p;
        }
    }
    PathBuf::from("pi")
}

/// Write `.pi/mcp.json` (project-local) so pi — and any pi you start in a
/// terminal in this folder — connects to Spide's MCP server.
pub fn write_mcp_config(project: &Path, url: &str) -> std::io::Result<()> {
    let dir = project.join(".pi");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("mcp.json");
    let mut cfg: Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| json!({}));
    if !cfg.get("mcpServers").is_some_and(Value::is_object) {
        cfg["mcpServers"] = json!({});
    }
    cfg["mcpServers"]["spide"] = json!({
        "url": url,
        "exposure": "direct",
        "description": "Spide: the live class-diagram design canvas. The only way to change the design."
    });
    std::fs::write(path, serde_json::to_string_pretty(&cfg)?)
}

impl PiBridge {
    pub fn start(&self, st: &AppState) -> Result<(), String> {
        self.stop();
        let (project, url) = {
            let s = st.lock();
            (s.project_dir.clone(), crate::state::mcp_url(s.mcp_port))
        };
        let set_status = |status: PiStatus, err: Option<String>| {
            let mut s = st.lock();
            s.pi_status = status;
            s.pi_error = err;
        };
        set_status(PiStatus::Starting, None);
        st.emit();

        let spide_dir = project.join(".spide");
        std::fs::create_dir_all(spide_dir.join("sessions")).map_err(|e| e.to_string())?;
        let skill_path = spide_dir.join("agent.md");
        std::fs::write(&skill_path, SKILL).map_err(|e| e.to_string())?;
        write_mcp_config(&project, &url).map_err(|e| e.to_string())?;

        let mut cmd = Command::new(pi_binary());
        cmd.current_dir(&project)
            .args(["--mode", "rpc", "--approve", "--continue", "--name", "Spide"])
            .arg("--session-dir")
            .arg(spide_dir.join("sessions"))
            .arg("--append-system-prompt")
            .arg(&skill_path)
            .args(["--tools", "read,ls,grep,find"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Ok(extra) = std::env::var("SPIDE_PI_ARGS") {
            cmd.args(extra.split_whitespace());
        }
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                let msg = format!("could not start pi ({}): {e}", pi_binary().display());
                set_status(PiStatus::Offline, Some(msg.clone()));
                st.emit();
                return Err(msg);
            }
        };
        eprintln!("spide: started pi (pid {}) in {}", child.id(), project.display());
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let stdin = Arc::new(Mutex::new(child.stdin.take().unwrap()));
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();

        // stdout: protocol records
        {
            let st = st.clone();
            let stdin = stdin.clone();
            let current = self.generation.clone();
            std::thread::spawn(move || {
                let reader = BufReader::new(stdout);
                for line in reader.split(b'\n') {
                    let Ok(line) = line else { break };
                    let line = String::from_utf8_lossy(&line);
                    let line = line.trim_end_matches('\r');
                    if line.is_empty() {
                        continue;
                    }
                    match serde_json::from_str::<Value>(line) {
                        Ok(v) => handle_record(&st, &stdin, &v),
                        Err(_) => eprintln!("pi(stdout?): {line}"),
                    }
                }
                eprintln!("spide: pi stdout closed");
                // exited (ignore if a newer pi was started meanwhile)
                if current.load(Ordering::SeqCst) != generation {
                    return;
                }
                let mut s = st.lock();
                if s.pi_status != PiStatus::Offline {
                    s.pi_status = PiStatus::Offline;
                    s.pi_error.get_or_insert_with(|| "pi exited".into());
                }
                drop(s);
                let _ = st.app.emit("agent", json!({"kind": "exit", "generation": generation}));
                st.emit();
            });
        }
        // stderr: diagnostics; keep the tail for error reporting
        {
            let st = st.clone();
            std::thread::spawn(move || {
                let reader = BufReader::new(stderr);
                for line in reader.lines().map_while(Result::ok) {
                    eprintln!("pi: {line}");
                    if line.to_lowercase().contains("error") {
                        st.lock().pi_error = Some(line);
                    }
                }
            });
        }
        *self.proc.lock().unwrap() = Some(Proc { child, stdin, generation });
        set_status(PiStatus::Idle, None);
        st.emit();
        Ok(())
    }

    pub fn stop(&self) {
        if let Some(mut p) = self.proc.lock().unwrap().take() {
            let _ = p.generation;
            drop(p.stdin); // EOF → orderly shutdown
            let child = &mut p.child;
            for _ in 0..20 {
                if let Ok(Some(_)) = child.try_wait() {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    pub fn restart(&self, st: &AppState) -> Result<(), String> {
        self.start(st)
    }

    fn send(&self, v: Value) -> Result<(), String> {
        let guard = self.proc.lock().unwrap();
        let p = guard.as_ref().ok_or("the agent is not running (type /agent restart)")?;
        let mut stdin = p.stdin.lock().unwrap();
        let mut line = serde_json::to_string(&v).map_err(|e| e.to_string())?;
        line.push('\n');
        stdin.write_all(line.as_bytes()).and_then(|_| stdin.flush()).map_err(|e| format!("agent pipe: {e}"))
    }

    /// Send a prompt; queued as follow-up if the agent is busy.
    pub fn prompt(&self, st: &AppState, message: String, echo: Option<String>) -> Result<(), String> {
        if let Some(e) = echo {
            let _ = st.app.emit("agent", json!({"kind": "user", "text": e}));
        }
        let id = format!("spide-{}", self.req.fetch_add(1, Ordering::SeqCst));
        let r = self.send(json!({"id": id, "type": "prompt", "message": message, "streamingBehavior": "followUp"}));
        if let Err(e) = &r {
            let _ = st.app.emit("agent", json!({"kind": "error", "text": e}));
        }
        r
    }

    pub fn abort(&self) -> Result<(), String> {
        self.send(json!({"type": "abort"}))
    }
}

impl Drop for PiBridge {
    fn drop(&mut self) {
        self.stop();
    }
}

fn short_tool(name: &str) -> String {
    name.strip_prefix("mcp__spide__").unwrap_or(name).to_string()
}

fn handle_record(st: &AppState, stdin: &Arc<Mutex<ChildStdin>>, v: &Value) {
    let ty = v.get("type").and_then(Value::as_str).unwrap_or("");
    let emit = |payload: Value| {
        let _ = st.app.emit("agent", payload);
    };
    let set = |status: PiStatus| {
        let changed = {
            let mut s = st.lock();
            let c = s.pi_status != status;
            s.pi_status = status;
            c
        };
        if changed {
            st.emit();
        }
    };
    match ty {
        "response" => {
            if v.get("success") == Some(&Value::Bool(false)) {
                let err = v.get("error").and_then(Value::as_str).unwrap_or("command failed");
                emit(json!({"kind": "error", "text": err}));
            }
        }
        "agent_start" => set(PiStatus::Busy),
        "agent_settled" => {
            set(PiStatus::Idle);
            emit(json!({"kind": "settled"}));
        }
        "message_start" => {
            if v.pointer("/message/role").and_then(Value::as_str) == Some("assistant") {
                emit(json!({"kind": "assistant_start"}));
            }
        }
        "message_update" => {
            let ev = &v["assistantMessageEvent"];
            match ev.get("type").and_then(Value::as_str) {
                Some("text_delta") => emit(json!({"kind": "text", "delta": ev["delta"]})),
                Some("thinking_start") => emit(json!({"kind": "thinking"})),
                _ => {}
            }
        }
        "message_end" => {
            let m = &v["message"];
            if m.get("role").and_then(Value::as_str) == Some("assistant") {
                if m.get("stopReason").and_then(Value::as_str) == Some("error") {
                    let e = m.get("errorMessage").and_then(Value::as_str).unwrap_or("model error");
                    emit(json!({"kind": "error", "text": e}));
                }
                emit(json!({"kind": "assistant_end"}));
            }
        }
        "tool_execution_start" => {
            let name = v["toolName"].as_str().map(short_tool).unwrap_or_default();
            emit(json!({"kind": "tool", "name": name}));
        }
        "tool_execution_end" => {
            if v.get("isError") == Some(&Value::Bool(true)) {
                let name = v["toolName"].as_str().map(short_tool).unwrap_or_default();
                let text = v.pointer("/result/content/0/text").and_then(Value::as_str).unwrap_or("").to_string();
                emit(json!({"kind": "tool_error", "name": name, "text": text}));
            }
        }
        "auto_retry_start" => emit(json!({"kind": "status", "text": "retrying…"})),
        "compaction_start" => emit(json!({"kind": "status", "text": "compacting context…"})),
        "extension_ui_request" => {
            let id = v.get("id").cloned().unwrap_or(Value::Null);
            let method = v.get("method").and_then(Value::as_str).unwrap_or("");
            let reply = match method {
                // Spide's agent only has read-only + Spide tools; confirmations are safe to grant.
                "confirm" => Some(json!({"type": "extension_ui_response", "id": id, "confirmed": true})),
                "select" | "input" | "editor" => Some(json!({"type": "extension_ui_response", "id": id, "cancelled": true})),
                "notify" => {
                    let msg = v.get("message").and_then(Value::as_str).unwrap_or("");
                    if !msg.is_empty() {
                        emit(json!({"kind": "status", "text": msg}));
                    }
                    None
                }
                _ => None,
            };
            if let Some(r) = reply {
                if let Ok(mut w) = stdin.lock() {
                    let _ = writeln!(w, "{r}");
                    let _ = w.flush();
                }
            }
        }
        _ => {}
    }
}
