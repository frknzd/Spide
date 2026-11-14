//! Private app↔pi extension control socket. Not part of the browser/MCP HTTP API.
use crate::state::{AppState, PiStatus};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::unix::{fs::PermissionsExt, net::{UnixListener, UnixStream}};
use std::sync::{atomic::{AtomicBool, AtomicU64, Ordering}, mpsc, Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::Emitter;

const LIMIT: usize = 1024 * 1024;
fn valid_hello(v: &Value, token: &str, generation: u64) -> bool {
    v["type"] == "hello" && v["token"].as_str() == Some(token) && v["generation"].as_u64() == Some(generation)
}
pub struct Control {
    dir: tempfile::TempDir,
    pub path: String,
    pub token: String,
    pub generation: u64,
    stopped: AtomicBool,
    writer: Mutex<Option<UnixStream>>,
    pending: Mutex<HashMap<String, mpsc::SyncSender<Value>>>,
    pub tab: Mutex<String>,
    seq: AtomicU64,
    worker: Mutex<Option<std::thread::JoinHandle<()>>>,
}
impl Control {
    pub fn start(st: AppState, generation: u64) -> Result<Arc<Self>, String> {
        // /tmp avoids macOS's long per-user TMPDIR exceeding sockaddr_un's path limit.
        let dir = tempfile::Builder::new().prefix("spide-").tempdir_in("/tmp").map_err(|e| e.to_string())?;
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
        let path = dir.path().join("pi.sock");
        let listener = UnixListener::bind(&path).map_err(|e| e.to_string())?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).map_err(|e| e.to_string())?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let mut random = [0u8; 32];
        std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut random)).map_err(|e| e.to_string())?;
        let c = Arc::new(Self { path: path.display().to_string(), token: random.iter().map(|b| format!("{b:02x}")).collect(), generation,
            dir, stopped: AtomicBool::new(false), writer: Mutex::new(None), pending: Mutex::new(HashMap::new()), tab: Mutex::new("class".into()), seq: AtomicU64::new(0), worker: Mutex::new(None) });
        let ctrl = c.clone();
        let worker = std::thread::spawn(move || {
            while !ctrl.stopped.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((s, _)) => ctrl.connection(s, &st),
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(30)),
                    Err(_) => break,
                }
            }
        });
        *c.worker.lock().unwrap() = Some(worker);
        Ok(c)
    }
    pub fn ready(&self) -> bool { self.writer.lock().unwrap().is_some() }
    fn send(&self, v: &Value) -> Result<(), String> {
        let text = serde_json::to_vec(v).map_err(|e| e.to_string())?;
        if text.len() > LIMIT { return Err("control message too large".into()); }
        let mut w = self.writer.lock().unwrap();
        let s = w.as_mut().ok_or("pi's integration is not ready (restart pi if it doesn't reconnect)")?;
        s.write_all(&text).and_then(|_| s.write_all(b"\n")).map_err(|e| e.to_string())
    }
    pub fn request(&self, method: &str, payload: Value) -> Result<Value, String> {
        self.request_timeout(method, payload, Duration::from_secs(8))
    }
    fn request_timeout(&self, method: &str, payload: Value, timeout: Duration) -> Result<Value, String> {
        let id = format!("app-{}-{}", self.generation, self.seq.fetch_add(1, Ordering::SeqCst));
        let (tx, rx) = mpsc::sync_channel(1);
        self.pending.lock().unwrap().insert(id.clone(), tx);
        let sent = self.send(&json!({"type":"request","id":id,"generation":self.generation,"method":method,"payload":payload}));
        if let Err(e) = sent { self.pending.lock().unwrap().remove(&id); return Err(e); }
        let result = rx.recv_timeout(timeout).map_err(|_| "pi didn't acknowledge the request (not retried, to avoid duplicate turns)".to_string());
        self.pending.lock().unwrap().remove(&id);
        let v = result?;
        if v["ok"] == true { Ok(v["result"].clone()) } else { Err(v["error"].as_str().unwrap_or("pi request failed").into()) }
    }
    fn response(&self, v: Value) {
        if v["generation"] != self.generation { return; }
        if let Some(id) = v["id"].as_str() {
            if let Some(tx) = self.pending.lock().unwrap().remove(id) { let _ = tx.send(v); }
        }
    }
    fn connection(&self, mut stream: UnixStream, st: &AppState) {
        let _ = stream.set_read_timeout(Some(Duration::from_millis(200)));
        let mut authed = false;
        let mut pending = vec![];
        let mut buf = [0u8; 8192];
        let connected = Instant::now();
        let mut seen: HashMap<String, Value> = HashMap::new();
        'connection: while !self.stopped.load(Ordering::SeqCst) {
            if !authed && connected.elapsed() > Duration::from_secs(2) { break; }
            match stream.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => pending.extend_from_slice(&buf[..n]),
                Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => continue,
                Err(_) => break,
            }
            if pending.len() > LIMIT { break; }
            while let Some(n) = pending.iter().position(|b| *b == b'\n') {
                let line: Vec<u8> = pending.drain(..=n).collect();
                let Ok(v) = serde_json::from_slice::<Value>(&line) else { break 'connection };
                if !authed {
                    if !valid_hello(&v, &self.token, self.generation) { break 'connection; }
                    let Ok(writer) = stream.try_clone() else { break 'connection };
                    let _ = writer.set_write_timeout(Some(Duration::from_secs(2)));
                    *self.writer.lock().unwrap() = Some(writer);
                    authed = true;
                    let _ = self.send(&json!({"type":"ready","generation":self.generation}));
                    { let mut s = st.lock(); s.pi_status = PiStatus::Idle; s.pi_error = None; }
                    st.emit();
                    continue;
                }
                if v["generation"] != self.generation { continue; }
                match v["type"].as_str() {
                    Some("response") => self.response(v.clone()),
                    Some("request") => {
                        let id = v["id"].as_str().unwrap_or("").to_string();
                        if id.is_empty() || id.len() > 128 { break 'connection; }
                        if let Some(r) = seen.get(&id) { let _ = self.send(r); continue; }
                        let payload = &v["payload"];
                        let result = match v["method"].as_str() {
                            Some("context") => {
                                let s = st.lock();
                                let tab = self.tab.lock().unwrap();
                                Ok(json!({"text": crate::prompts::user_prompt(payload["text"].as_str().unwrap_or(""), &tab, &s.store.design)}))
                            }
                            Some("command") => st.app.emit("spide-command", json!({"id":id,"generation":self.generation,"project":st.lock().project_dir.display().to_string(),"command":payload["command"]})).map(|_| json!({"queued":true})).map_err(|e| e.to_string()),
                            _ => Err("unknown integration request".into()),
                        };
                        let r = match result {
                            Ok(r) => json!({"type":"response","id":id,"generation":self.generation,"ok":true,"result":r}),
                            Err(e) => json!({"type":"response","id":id,"generation":self.generation,"ok":false,"error":e}),
                        };
                        if seen.len() > 128 { seen.clear(); }
                        seen.insert(id, r.clone());
                        let _ = self.send(&r);
                    }
                    Some("event") => match v["event"].as_str() {
                        Some("busy") => { st.lock().pi_status = PiStatus::Busy; st.emit(); }
                        Some("settled") => {
                            st.lock().pi_status = PiStatus::Idle;
                            let stuck = st.lock().store.release_stuck_evaluations();
                            if stuck > 0 { st.changed(); st.toast("error", "The examiner didn't give a verdict — defend again."); }
                            else { st.emit(); }
                        }
                        Some("shutdown") => { st.lock().pi_status = PiStatus::Starting; st.emit(); }
                        _ => {}
                    },
                    _ => {}
                }
            }
        }
        *self.writer.lock().unwrap() = None;
        if authed && !self.stopped.load(Ordering::SeqCst) {
            { let mut s = st.lock(); s.pi_status = PiStatus::Offline; s.pi_error = Some("pi integration disconnected; reconnecting (or use Restart)".into()); }
            let stuck = st.lock().store.release_stuck_evaluations();
            if stuck > 0 { st.changed(); } else { st.emit(); }
        }
        for (_, tx) in self.pending.lock().unwrap().drain() {
            let _ = tx.send(json!({"ok":false,"error":"pi integration disconnected"}));
        }
    }
    pub fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
        if let Some(s) = self.writer.lock().unwrap().take() { let _ = s.shutdown(std::net::Shutdown::Both); }
        if let Some(worker) = self.worker.lock().unwrap().take() {
            if worker.thread().id() != std::thread::current().id() { let _ = worker.join(); }
        }
        let _ = self.dir.path();
    }
}
impl Drop for Control { fn drop(&mut self) { self.stop(); } }

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (Arc<Control>, UnixStream) {
        let dir = tempfile::tempdir().unwrap();
        let (writer, peer) = UnixStream::pair().unwrap();
        (Arc::new(Control { dir, path: String::new(), token: "secret".into(), generation: 3, stopped: AtomicBool::new(false), writer: Mutex::new(Some(writer)), pending: Mutex::new(HashMap::new()), tab: Mutex::new("class".into()), seq: AtomicU64::new(0), worker: Mutex::new(None) }), peer)
    }
    #[test]
    fn request_ack_stale_duplicate_caps_and_timeout() {
        use std::io::{BufRead, BufReader};
        let (c, peer) = fixture();
        let controller = c.clone();
        let fake = std::thread::spawn(move || {
            let mut reader = BufReader::new(peer);
            let mut line = String::new(); reader.read_line(&mut line).unwrap();
            let v: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(v["method"], "prompt");
            controller.response(json!({"id":v["id"],"generation":2,"ok":true}));
            assert_eq!(controller.pending.lock().unwrap().len(), 1);
            controller.response(json!({"id":v["id"],"generation":3,"ok":true,"result":"once"}));
            controller.response(json!({"id":v["id"],"generation":3,"ok":true,"result":"duplicate"}));
            std::thread::sleep(Duration::from_millis(100));
        });
        assert_eq!(c.request("prompt", json!({"message":"examine"})).unwrap(), "once");
        assert!(c.pending.lock().unwrap().is_empty());
        assert!(c.send(&json!({"big":"x".repeat(LIMIT)})).is_err());
        assert!(c.request_timeout("prompt", json!({}), Duration::from_millis(10)).unwrap_err().contains("not retried"));
        assert!(c.pending.lock().unwrap().is_empty());
        fake.join().unwrap();
        c.stop();
        assert!(c.request("prompt", json!({})).is_err());
    }
    #[test]
    fn auth_requires_token_and_generation() {
        assert!(valid_hello(&json!({"type":"hello","token":"secret","generation":3}), "secret", 3));
        for v in [json!({"type":"hello","token":"wrong","generation":3}), json!({"type":"hello","token":"secret","generation":2}), json!({"token":"secret","generation":3}), json!({"type":"hello","token":"secret","generation":"3"})] {
            assert!(!valid_hello(&v, "secret", 3));
        }
    }
}
