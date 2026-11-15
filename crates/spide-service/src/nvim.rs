//! Minimal Neovim msgpack-RPC client over a Unix socket (`nvim --listen`, `v:servername`).
//! Spide only ever calls fixed functions of its own Lua plugin (`require('spide').…`) — no
//! arbitrary Lua or Ex commands reach Neovim through it.

use rmpv::Value as Mp;
use serde_json::Value;
use std::collections::HashMap;
use std::io::{BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub struct Nvim {
    w: Mutex<UnixStream>,
    next: AtomicU32,
    pending: Arc<Mutex<HashMap<u32, Sender<Result<Mp, String>>>>>,
    dead: Arc<AtomicBool>,
    timeout: Duration,
}

pub fn to_mp(v: &Value) -> Mp {
    match v {
        Value::Null => Mp::Nil,
        Value::Bool(b) => Mp::Boolean(*b),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Mp::from(i)
            } else if let Some(u) = n.as_u64() {
                Mp::from(u)
            } else {
                Mp::F64(n.as_f64().unwrap_or(0.0))
            }
        }
        Value::String(s) => Mp::from(s.as_str()),
        Value::Array(a) => Mp::Array(a.iter().map(to_mp).collect()),
        Value::Object(o) => Mp::Map(o.iter().map(|(k, v)| (Mp::from(k.as_str()), to_mp(v))).collect()),
    }
}

pub fn to_json(v: &Mp) -> Value {
    match v {
        Mp::Nil => Value::Null,
        Mp::Boolean(b) => Value::Bool(*b),
        Mp::Integer(i) => i.as_i64().map(Value::from).or_else(|| i.as_u64().map(Value::from)).unwrap_or(Value::Null),
        Mp::F32(f) => serde_json::json!(*f),
        Mp::F64(f) => serde_json::json!(*f),
        Mp::String(s) => Value::String(s.as_str().map(str::to_string).unwrap_or_else(|| String::from_utf8_lossy(s.as_bytes()).into_owned())),
        Mp::Binary(b) => Value::String(String::from_utf8_lossy(b).into_owned()),
        Mp::Array(a) => Value::Array(a.iter().map(to_json).collect()),
        Mp::Map(m) => Value::Object(m.iter().map(|(k, v)| (match k { Mp::String(s) => s.as_str().unwrap_or("").to_string(), other => other.to_string() }, to_json(v))).collect()),
        Mp::Ext(_, _) => Value::Null,
    }
}

impl Nvim {
    pub fn connect(socket: &Path, timeout: Duration) -> Result<Arc<Nvim>, String> {
        let s = UnixStream::connect(socket).map_err(|e| format!("cannot reach Neovim at {}: {e}", socket.display()))?;
        let r = s.try_clone().map_err(|e| e.to_string())?;
        let pending: Arc<Mutex<HashMap<u32, Sender<Result<Mp, String>>>>> = Default::default();
        let dead = Arc::new(AtomicBool::new(false));
        {
            let (pending, dead) = (pending.clone(), dead.clone());
            std::thread::spawn(move || {
                let mut r = BufReader::new(r);
                while let Ok(msg) = rmpv::decode::read_value(&mut r) {
                    let Mp::Array(a) = msg else { continue };
                    // [1, id, error, result]
                    if a.first().and_then(Mp::as_u64) == Some(1) {
                        let id = a.get(1).and_then(Mp::as_u64).unwrap_or(0) as u32;
                        if let Some(tx) = pending.lock().unwrap().remove(&id) {
                            let err = a.get(2).cloned().unwrap_or(Mp::Nil);
                            let _ = tx.send(if err.is_nil() { Ok(a.get(3).cloned().unwrap_or(Mp::Nil)) } else { Err(match &err { Mp::Array(e) => e.get(1).map(|m| m.to_string()).unwrap_or_default(), e => e.to_string() }) });
                        }
                    }
                }
                dead.store(true, Ordering::SeqCst);
                for (_, tx) in pending.lock().unwrap().drain() {
                    let _ = tx.send(Err("Neovim disconnected".into()));
                }
            });
        }
        Ok(Arc::new(Nvim { w: Mutex::new(s), next: AtomicU32::new(1), pending, dead, timeout }))
    }

    pub fn alive(&self) -> bool {
        !self.dead.load(Ordering::SeqCst)
    }

    pub fn call(&self, method: &str, args: Vec<Mp>) -> Result<Mp, String> {
        if !self.alive() {
            return Err("Neovim disconnected".into());
        }
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = channel();
        self.pending.lock().unwrap().insert(id, tx);
        let msg = Mp::Array(vec![Mp::from(0), Mp::from(id), Mp::from(method), Mp::Array(args)]);
        let mut buf = vec![];
        rmpv::encode::write_value(&mut buf, &msg).map_err(|e| e.to_string())?;
        self.w.lock().unwrap().write_all(&buf).map_err(|e| e.to_string())?;
        rx.recv_timeout(self.timeout).map_err(|_| {
            self.pending.lock().unwrap().remove(&id);
            format!("Neovim did not answer {method} in {:?}", self.timeout)
        })?
    }

    /// Call `require('spide').<func>(args…)` and return its JSON-encoded result.
    pub fn plugin(&self, func: &str, args: Vec<Value>) -> Result<Value, String> {
        if !func.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err("invalid plugin function".into());
        }
        let code = format!("local ok, spide = pcall(require, 'spide') if not ok then error('spide.nvim is not loaded') end return vim.json.encode(spide.{func}(...))");
        let r = self.call("nvim_exec_lua", vec![Mp::from(code.as_str()), Mp::Array(args.iter().map(to_mp).collect())])?;
        let s = r.as_str().ok_or("plugin returned no JSON")?;
        serde_json::from_str(s).map_err(|e| e.to_string())
    }
}
