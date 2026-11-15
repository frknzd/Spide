//! Authenticated discovery: a running service publishes its endpoint and a bearer token in
//! owner-only files, both in the project (`.spide/endpoint.json`) and in a per-user registry
//! (`$XDG_STATE_HOME/spide/endpoints/<hash>.json`) so Neovim/agents can find it by root.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Endpoint {
    pub version: u32,
    pub root: String,
    pub pid: u32,
    /// desktop | headless
    pub host: String,
    pub mcp_url: Option<String>,
    /// Unix socket for editor RPC (Neovim bridge), if any.
    pub editor_socket: Option<String>,
    pub token: String,
    pub started_at: u64,
}

pub fn project_file(root: &Path) -> PathBuf {
    root.join(".spide").join("endpoint.json")
}

pub fn registry_dir() -> PathBuf {
    let base = std::env::var_os("XDG_STATE_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local").join("state"))).unwrap_or_else(std::env::temp_dir);
    base.join("spide").join("endpoints")
}

fn registry_file(root: &Path) -> PathBuf {
    registry_dir().join(format!("{}.json", spide_core::assess::fnv(&crate::policy::key(root))))
}

/// Constant-time comparison of a presented token.
pub fn token_ok(expected: &str, presented: &str) -> bool {
    let (a, b) = (expected.as_bytes(), presented.as_bytes());
    if a.len() != b.len() || a.is_empty() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// `Authorization: Bearer <token>` header value check.
pub fn bearer_ok(expected: &str, header: Option<&str>) -> bool {
    header.and_then(|h| h.strip_prefix("Bearer ")).is_some_and(|t| token_ok(expected, t.trim()))
}

impl Endpoint {
    pub fn publish(&self) -> Result<(), String> {
        for p in [project_file(Path::new(&self.root)), registry_file(Path::new(&self.root))] {
            let dir = p.parent().unwrap();
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
            // create private before writing the secret
            let tmp = p.with_extension("json.tmp");
            {
                use std::io::Write;
                use std::os::unix::fs::OpenOptionsExt;
                let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp).map_err(|e| e.to_string())?;
                f.write_all(serde_json::to_string_pretty(self).unwrap().as_bytes()).map_err(|e| e.to_string())?;
            }
            std::fs::rename(&tmp, &p).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// Remove our published files (only if they are still ours).
    pub fn retract(&self) {
        for p in [project_file(Path::new(&self.root)), registry_file(Path::new(&self.root))] {
            if Self::read(&p).is_some_and(|e| e.token == self.token) {
                let _ = std::fs::remove_file(p);
            }
        }
    }

    pub fn read(p: &Path) -> Option<Endpoint> {
        serde_json::from_str(&std::fs::read_to_string(p).ok()?).ok()
    }

    /// The live endpoint serving `root`, if any.
    pub fn find(root: &Path) -> Option<Endpoint> {
        let e = Self::read(&registry_file(root)).or_else(|| Self::read(&project_file(root)))?;
        let alive = unsafe { libc::kill(e.pid as libc::pid_t, 0) == 0 };
        alive.then_some(e)
    }
}

/// Minimal blocking MCP `tools/call` to a published endpoint (loopback HTTP + bearer).
pub fn call(ep: &Endpoint, tool: &str, args: serde_json::Value) -> Result<serde_json::Value, String> {
    use std::io::{Read, Write};
    let url = ep.mcp_url.as_deref().ok_or("no MCP url")?;
    let rest = url.strip_prefix("http://").ok_or("bad url")?;
    let (hostport, path) = rest.split_once('/').ok_or("bad url")?;
    let mut s = std::net::TcpStream::connect(hostport).map_err(|e| e.to_string())?;
    s.set_read_timeout(Some(std::time::Duration::from_secs(10))).ok();
    let body = serde_json::json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":tool,"arguments":args}}).to_string();
    write!(s, "POST /{path} HTTP/1.1\r\nHost: {hostport}\r\nContent-Type: application/json\r\nAuthorization: Bearer {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", ep.token, body.len()).map_err(|e| e.to_string())?;
    let mut r = String::new();
    s.read_to_string(&mut r).map_err(|e| e.to_string())?;
    let v: serde_json::Value = serde_json::from_str(r.split("\r\n\r\n").nth(1).unwrap_or("")).map_err(|e| e.to_string())?;
    let text = v.pointer("/result/content/0/text").and_then(serde_json::Value::as_str).unwrap_or("").to_string();
    if v.pointer("/result/isError") == Some(&serde_json::Value::Bool(true)) {
        return Err(text);
    }
    Ok(serde_json::from_str(&text).unwrap_or(serde_json::Value::String(text)))
}

/// Ask a headless service owning `root` to hand the workspace over; wait for its lock to go.
pub fn take_over_from_headless(root: &Path) -> Option<crate::workspace::WorkspaceLock> {
    let ep = Endpoint::find(root).filter(|e| e.host == "headless")?;
    call(&ep, "yield_workspace", serde_json::json!({})).ok()?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while std::time::Instant::now() < deadline {
        if let Ok(l) = crate::workspace::WorkspaceLock::acquire(root) {
            return Some(l);
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens() {
        assert!(token_ok("abc", "abc"));
        assert!(!token_ok("abc", "abd") && !token_ok("abc", "ab") && !token_ok("", ""));
        assert!(bearer_ok("abc", Some("Bearer abc")));
        assert!(!bearer_ok("abc", Some("abc")) && !bearer_ok("abc", None));
    }

    #[test]
    fn publish_find_retract() {
        let d = tempfile::tempdir().unwrap();
        std::env::set_var("XDG_STATE_HOME", d.path().join("state"));
        let e = Endpoint { version: 1, root: d.path().display().to_string(), pid: std::process::id(), host: "headless".into(), mcp_url: None, editor_socket: None, token: "t0k".into(), started_at: 0 };
        e.publish().unwrap();
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(project_file(d.path())).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(Endpoint::find(d.path()).unwrap().token, "t0k");
        e.retract();
        assert!(Endpoint::find(d.path()).is_none());
    }
}
