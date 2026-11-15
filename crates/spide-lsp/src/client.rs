//! A managed stdio LSP client: spawn, initialize (negotiating the position encoding), open
//! documents, query symbols/definitions/references, collect published diagnostics, answer the
//! server's requests (`workspace/configuration`, progress), shut down. Every request has a
//! timeout; a dead server fails requests instead of hanging.

use crate::position::Encoding;
use crate::transport::{read_message, write_message};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub argv: Vec<String>,
    pub root: PathBuf,
    pub env: BTreeMap<String, String>,
    pub init_options: Option<Value>,
    pub settings: Option<Value>,
    pub timeout: Duration,
}

#[derive(Debug, Clone, Default)]
pub struct Published {
    pub version: Option<i64>,
    pub diagnostics: Vec<Value>,
    pub seq: u64,
}

#[derive(Default)]
struct Shared {
    pending: Mutex<HashMap<i64, Sender<Result<Value, String>>>>,
    diagnostics: Mutex<BTreeMap<String, Published>>,
    diag_wake: Condvar,
    seq: Mutex<u64>,
    dead: AtomicBool,
    stderr_tail: Mutex<String>,
}

pub struct LspClient {
    child: Mutex<Child>,
    stdin: Arc<Mutex<ChildStdin>>,
    next_id: AtomicI64,
    shared: Arc<Shared>,
    caps: OnceLock<Value>,
    enc: OnceLock<Encoding>,
    name: Mutex<String>,
    timeout: Duration,
    root: PathBuf,
    versions: Mutex<BTreeMap<String, i64>>,
}

pub fn file_uri(p: &Path) -> String {
    let s = p.to_string_lossy();
    let mut out = String::from("file://");
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub fn uri_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let bytes = rest.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(b) = u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).ok()?, 16) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    Some(PathBuf::from(String::from_utf8(out).ok()?))
}

/// Look `name` up on PATH (or accept an existing path).
pub fn which(name: &str) -> Option<PathBuf> {
    let p = Path::new(name);
    if p.is_absolute() || name.contains('/') {
        return p.is_file().then(|| p.to_path_buf());
    }
    std::env::var_os("PATH").and_then(|paths| std::env::split_paths(&paths).map(|d| d.join(name)).find(|c| c.is_file()))
}

impl LspClient {
    pub fn start(cfg: &ServerConfig) -> Result<Arc<LspClient>, String> {
        let exe = cfg.argv.first().ok_or("empty server command")?;
        let resolved = crate::defaults::resolve(&cfg.root, exe).unwrap_or_else(|| PathBuf::from(exe));
        let mut cmd = Command::new(resolved);
        cmd.args(&cfg.argv[1..]).current_dir(&cfg.root).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        for (k, v) in &cfg.env {
            cmd.env(k, v);
        }
        let mut child = cmd.spawn().map_err(|e| format!("could not start {exe}: {e}"))?;
        let stdin = Arc::new(Mutex::new(child.stdin.take().ok_or("no stdin")?));
        let stdout = child.stdout.take().ok_or("no stdout")?;
        let stderr = child.stderr.take();
        let shared = Arc::new(Shared::default());
        // the server's requests need answers on stdin: the reader posts them back through a channel
        let (reply_tx, reply_rx) = channel::<Value>();
        {
            let shared = shared.clone();
            let settings = cfg.settings.clone().unwrap_or(json!({}));
            std::thread::spawn(move || {
                let mut r = BufReader::new(stdout);
                loop {
                    match read_message(&mut r) {
                        Ok(Some(msg)) => dispatch_incoming(&shared, msg, &settings, &reply_tx),
                        _ => break,
                    }
                }
                shared.dead.store(true, Ordering::SeqCst);
                for (_, tx) in shared.pending.lock().unwrap().drain() {
                    let _ = tx.send(Err("language server exited".into()));
                }
                shared.diag_wake.notify_all();
            });
        }
        if let Some(mut e) = stderr {
            let shared = shared.clone();
            std::thread::spawn(move || {
                use std::io::Read;
                let mut buf = [0u8; 4096];
                while let Ok(n) = e.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    let mut t = shared.stderr_tail.lock().unwrap();
                    t.push_str(&String::from_utf8_lossy(&buf[..n]));
                    if t.len() > 8192 {
                        let cut = t.len() - 8192;
                        let cut = (cut..t.len()).find(|i| t.is_char_boundary(*i)).unwrap_or(t.len());
                        t.drain(..cut);
                    }
                }
            });
        }
        let client = Arc::new(LspClient {
            child: Mutex::new(child),
            stdin: stdin.clone(),
            next_id: AtomicI64::new(1),
            shared,
            caps: OnceLock::new(),
            enc: OnceLock::new(),
            name: Mutex::new(exe.clone()),
            timeout: cfg.timeout,
            root: cfg.root.clone(),
            versions: Mutex::new(BTreeMap::new()),
        });
        {
            let (stdin, shared) = (stdin.clone(), client.shared.clone());
            std::thread::spawn(move || {
                while let Ok(v) = reply_rx.recv() {
                    if shared.dead.load(Ordering::SeqCst) {
                        break;
                    }
                    let _ = write_message(&mut *stdin.lock().unwrap(), &v);
                }
            });
        }
        let init = client.request(
            "initialize",
            json!({
                "processId": std::process::id(),
                "rootUri": file_uri(&cfg.root),
                "workspaceFolders": [{"uri": file_uri(&cfg.root), "name": cfg.root.file_name().and_then(|n| n.to_str()).unwrap_or("root")}],
                "initializationOptions": cfg.init_options.clone().unwrap_or(Value::Null),
                "capabilities": {
                    "general": {"positionEncodings": ["utf-8", "utf-16"]},
                    "workspace": {"configuration": true, "workspaceFolders": true},
                    "textDocument": {
                        "documentSymbol": {"hierarchicalDocumentSymbolSupport": true},
                        "publishDiagnostics": {"versionSupport": true, "relatedInformation": false},
                        "definition": {}, "references": {}, "typeHierarchy": {}, "callHierarchy": {},
                        "synchronization": {"didSave": true}
                    },
                    "window": {"workDoneProgress": true}
                },
                "clientInfo": {"name": "spide", "version": env!("CARGO_PKG_VERSION")}
            }),
        );
        let init = match init {
            Ok(v) => v,
            Err(e) => {
                client.kill();
                return Err(format!("{exe}: initialize failed: {e}{}", client.stderr_hint()));
            }
        };
        let c = client;
        let caps = init.get("capabilities").cloned().unwrap_or(Value::Null);
        let _ = c.enc.set(Encoding::parse(caps.get("positionEncoding").and_then(Value::as_str).unwrap_or("utf-16")));
        let _ = c.caps.set(caps);
        if let Some(n) = init.pointer("/serverInfo/name").and_then(Value::as_str) {
            *c.name.lock().unwrap() = n.to_string();
        }
        c.notify("initialized", json!({}))?;
        if let Some(s) = &cfg.settings {
            c.notify("workspace/didChangeConfiguration", json!({"settings": s}))?;
        }
        Ok(c)
    }

    fn stderr_hint(&self) -> String {
        let t = self.shared.stderr_tail.lock().unwrap();
        let t = t.trim();
        if t.is_empty() {
            String::new()
        } else {
            format!(" (stderr: {})", t.lines().rev().take(3).collect::<Vec<_>>().join(" | "))
        }
    }

    pub fn capabilities(&self) -> &Value {
        static NULL: Value = Value::Null;
        self.caps.get().unwrap_or(&NULL)
    }

    pub fn encoding(&self) -> Encoding {
        self.enc.get().copied().unwrap_or(Encoding::Utf16)
    }

    pub fn server_name(&self) -> String {
        self.name.lock().unwrap().clone()
    }

    pub fn alive(&self) -> bool {
        !self.shared.dead.load(Ordering::SeqCst)
    }

    pub fn has(&self, capability: &str) -> bool {
        self.capabilities().get(capability).is_some_and(|v| !v.is_null() && v != &json!(false))
    }

    pub fn notify(&self, method: &str, params: Value) -> Result<(), String> {
        if !self.alive() {
            return Err("language server exited".into());
        }
        write_message(&mut *self.stdin.lock().unwrap(), &json!({"jsonrpc":"2.0","method":method,"params":params})).map_err(|e| e.to_string())
    }

    pub fn request(&self, method: &str, params: Value) -> Result<Value, String> {
        self.request_timeout(method, params, self.timeout)
    }

    pub fn request_timeout(&self, method: &str, params: Value, timeout: Duration) -> Result<Value, String> {
        if !self.alive() {
            return Err("language server exited".into());
        }
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = channel();
        self.shared.pending.lock().unwrap().insert(id, tx);
        write_message(&mut *self.stdin.lock().unwrap(), &json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})).map_err(|e| e.to_string())?;
        match rx.recv_timeout(timeout) {
            Ok(r) => r,
            Err(_) => {
                self.shared.pending.lock().unwrap().remove(&id);
                let _ = self.notify("$/cancelRequest", json!({"id": id}));
                Err(format!("{method} timed out after {:?}", timeout))
            }
        }
    }

    /// Open (or re-send) a document with its current text; returns the version sent.
    pub fn open(&self, path: &Path, language_id: &str, text: &str) -> Result<i64, String> {
        let uri = file_uri(path);
        let mut v = self.versions.lock().unwrap();
        let version = v.get(&uri).map(|x| x + 1).unwrap_or(1);
        if version == 1 {
            self.notify("textDocument/didOpen", json!({"textDocument":{"uri":uri,"languageId":language_id,"version":version,"text":text}}))?;
        } else {
            self.notify("textDocument/didChange", json!({"textDocument":{"uri":uri,"version":version},"contentChanges":[{"text":text}]}))?;
        }
        v.insert(uri, version);
        Ok(version)
    }

    pub fn close(&self, path: &Path) -> Result<(), String> {
        let uri = file_uri(path);
        self.versions.lock().unwrap().remove(&uri);
        self.notify("textDocument/didClose", json!({"textDocument":{"uri":uri}}))
    }

    /// Diagnostics published for `path` (None: nothing published yet).
    pub fn diagnostics(&self, path: &Path) -> Option<Published> {
        self.shared.diagnostics.lock().unwrap().get(&file_uri(path)).cloned()
    }

    /// Wait until diagnostics for all `paths` are published and no new publication arrives for
    /// `settle`, or `max` elapses. Returns paths that never got diagnostics.
    pub fn wait_diagnostics(&self, paths: &[PathBuf], settle: Duration, max: Duration) -> Vec<PathBuf> {
        let start = Instant::now();
        let uris: Vec<String> = paths.iter().map(|p| file_uri(p)).collect();
        let mut g = self.shared.diagnostics.lock().unwrap();
        let mut last_seq = *self.shared.seq.lock().unwrap();
        let mut quiet_since = Instant::now();
        loop {
            let all = uris.iter().all(|u| g.contains_key(u));
            let seq = *self.shared.seq.lock().unwrap();
            if seq != last_seq {
                last_seq = seq;
                quiet_since = Instant::now();
            }
            if (all && quiet_since.elapsed() >= settle) || start.elapsed() >= max || !self.alive() {
                break;
            }
            g = self.shared.diag_wake.wait_timeout(g, Duration::from_millis(50)).unwrap().0;
        }
        paths.iter().zip(&uris).filter(|(_, u)| !g.contains_key(*u)).map(|(p, _)| p.clone()).collect()
    }

    pub fn document_symbols(&self, path: &Path) -> Result<Value, String> {
        if !self.has("documentSymbolProvider") {
            return Err(format!("{} does not provide document symbols", self.server_name()));
        }
        self.request("textDocument/documentSymbol", json!({"textDocument":{"uri":file_uri(path)}}))
    }

    /// Definition at a 0-based line and LSP column (already in the negotiated encoding).
    pub fn definition(&self, path: &Path, line: u32, col: u32) -> Result<Value, String> {
        if !self.has("definitionProvider") {
            return Err(format!("{} does not provide definitions", self.server_name()));
        }
        self.request("textDocument/definition", json!({"textDocument":{"uri":file_uri(path)},"position":{"line":line,"character":col}}))
    }

    pub fn references(&self, path: &Path, line: u32, col: u32) -> Result<Value, String> {
        if !self.has("referencesProvider") {
            return Err(format!("{} does not provide references", self.server_name()));
        }
        self.request("textDocument/references", json!({"textDocument":{"uri":file_uri(path)},"position":{"line":line,"character":col},"context":{"includeDeclaration":false}}))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn kill(&self) {
        let mut c = self.child.lock().unwrap();
        let _ = c.kill();
        let _ = c.wait();
    }

    /// Polite shutdown, then kill.
    pub fn shutdown(&self) {
        if self.alive() {
            let _ = self.request_timeout("shutdown", Value::Null, Duration::from_secs(2));
            let _ = self.notify("exit", Value::Null);
        }
        let deadline = Instant::now() + Duration::from_millis(500);
        loop {
            if matches!(self.child.lock().unwrap().try_wait(), Ok(Some(_))) {
                return;
            }
            if Instant::now() > deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        self.kill();
    }
}

impl Drop for LspClient {
    fn drop(&mut self) {
        let mut c = self.child.lock().unwrap();
        if matches!(c.try_wait(), Ok(None)) {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

fn dispatch_incoming(shared: &Shared, msg: Value, settings: &Value, reply: &Sender<Value>) {
    let method = msg.get("method").and_then(Value::as_str);
    let id = msg.get("id").cloned();
    match (method, id) {
        (None, Some(id)) => {
            if let Some(id) = id.as_i64() {
                if let Some(tx) = shared.pending.lock().unwrap().remove(&id) {
                    let r = match msg.get("error") {
                        Some(e) => Err(e.get("message").and_then(Value::as_str).unwrap_or("error").to_string()),
                        None => Ok(msg.get("result").cloned().unwrap_or(Value::Null)),
                    };
                    let _ = tx.send(r);
                }
            }
        }
        (Some(m), Some(id)) => {
            // server → client requests
            let result = match m {
                "workspace/configuration" => {
                    let items = msg.pointer("/params/items").and_then(Value::as_array).cloned().unwrap_or_default();
                    Value::Array(
                        items
                            .iter()
                            .map(|it| {
                                let section = it.get("section").and_then(Value::as_str).unwrap_or("");
                                if section.is_empty() {
                                    settings.clone()
                                } else {
                                    section.split('.').fold(Some(settings), |v, k| v.and_then(|v| v.get(k))).cloned().unwrap_or(Value::Null)
                                }
                            })
                            .collect(),
                    )
                }
                "workspace/workspaceFolders" => Value::Null,
                _ => Value::Null, // progress create, registerCapability, showMessageRequest: acknowledge
            };
            let _ = reply.send(json!({"jsonrpc":"2.0","id":id,"result":result}));
        }
        (Some("textDocument/publishDiagnostics"), None) => {
            let uri = msg.pointer("/params/uri").and_then(Value::as_str).unwrap_or("").to_string();
            let diags = msg.pointer("/params/diagnostics").and_then(Value::as_array).cloned().unwrap_or_default();
            let version = msg.pointer("/params/version").and_then(Value::as_i64);
            let mut seq = shared.seq.lock().unwrap();
            *seq += 1;
            shared.diagnostics.lock().unwrap().insert(uri, Published { version, diagnostics: diags, seq: *seq });
            shared.diag_wake.notify_all();
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uris() {
        let p = Path::new("/tmp/a b/ü.rs");
        let u = file_uri(p);
        assert_eq!(u, "file:///tmp/a%20b/%C3%BC.rs");
        assert_eq!(uri_path(&u).unwrap(), p);
    }
}
