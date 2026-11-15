//! Editor sessions (Neovim) and source edits.
//!
//! Neovim owns unsaved buffers. An edit to a file open in Neovim goes through the plugin as an
//! undoable buffer change guarded by the buffer's `changedtick`; a closed file is changed with a
//! content-hash guard and an atomic write. Spide never overwrites a dirty buffer, and every
//! applied patch keeps its before/after text so a revert is a new, guarded change.

use crate::nvim::Nvim;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Session {
    pub id: String,
    /// external | embedded
    pub kind: String,
    pub socket: String,
    pub pid: u32,
    pub cwd: String,
    pub registered_at: u64,
    pub last_active: u64,
    /// LSP clients attached in Neovim, by name (evidence comes from them for live buffers).
    pub lsp: Vec<String>,
}

#[derive(Default)]
pub struct Editors {
    sessions: Mutex<BTreeMap<String, (Session, Option<Arc<Nvim>>)>>,
    /// The session the user chose; otherwise the most recently active external one.
    preferred: Mutex<Option<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Edit {
    /// 1-based, inclusive. Insert before `start_line` with `end_line = start_line - 1`.
    pub start_line: u32,
    pub end_line: u32,
    pub text: String,
}

pub fn hash(text: &str) -> String {
    format!("{:016x}", spide_lsp::diagnostics::fnv64(text.as_bytes()))
}

/// Apply line edits (bottom-up) to `text`.
pub fn apply_edits(text: &str, edits: &[Edit]) -> Result<String, String> {
    let trailing = text.ends_with('\n');
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let mut sorted = edits.to_vec();
    sorted.sort_by(|a, b| b.start_line.cmp(&a.start_line));
    let mut last_start = u32::MAX;
    for e in &sorted {
        if e.start_line == 0 || e.end_line + 1 < e.start_line || e.end_line as usize > lines.len() || e.end_line >= last_start {
            return Err(format!("edit {}..{} is out of range or overlaps (file has {} lines)", e.start_line, e.end_line, lines.len()));
        }
        last_start = e.start_line;
        let new: Vec<String> = if e.text.is_empty() { vec![] } else { e.text.trim_end_matches('\n').split('\n').map(str::to_string).collect() };
        lines.splice((e.start_line - 1) as usize..e.end_line as usize, new);
    }
    let mut out = lines.join("\n");
    if trailing || text.is_empty() {
        out.push('\n');
    }
    Ok(out)
}

/// A compact unified-style diff (changed lines with a little context).
pub fn diff(before: &str, after: &str) -> String {
    let a: Vec<&str> = before.lines().collect();
    let b: Vec<&str> = after.lines().collect();
    let pre = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let suf = a[pre..].iter().rev().zip(b[pre..].iter().rev()).take_while(|(x, y)| x == y).count();
    let mut out = format!("@@ -{},{} +{},{} @@\n", pre + 1, a.len() - pre - suf, pre + 1, b.len() - pre - suf);
    for l in &a[pre.saturating_sub(2)..pre] {
        out.push_str(&format!(" {l}\n"));
    }
    for l in &a[pre..a.len() - suf] {
        out.push_str(&format!("-{l}\n"));
    }
    for l in &b[pre..b.len() - suf] {
        out.push_str(&format!("+{l}\n"));
    }
    for l in a[a.len() - suf..].iter().take(2) {
        out.push_str(&format!(" {l}\n"));
    }
    out
}

impl Editors {
    pub fn register(&self, mut s: Session) -> Result<Session, String> {
        let nvim = Nvim::connect(Path::new(&s.socket), Duration::from_secs(5)).ok();
        if s.id.is_empty() {
            s.id = format!("{}:{}", s.kind, s.pid);
        }
        let now = spide_core::store::now_ms();
        s.registered_at = now;
        s.last_active = now;
        self.sessions.lock().unwrap().insert(s.id.clone(), (s.clone(), nvim));
        Ok(s)
    }

    pub fn unregister(&self, id: &str) {
        self.sessions.lock().unwrap().remove(id);
        let mut p = self.preferred.lock().unwrap();
        if p.as_deref() == Some(id) {
            *p = None;
        }
    }

    pub fn touch(&self, id: &str) {
        if let Some((s, _)) = self.sessions.lock().unwrap().get_mut(id) {
            s.last_active = spide_core::store::now_ms();
        }
    }

    pub fn prefer(&self, id: Option<String>) -> Result<(), String> {
        if let Some(i) = &id {
            if !self.sessions.lock().unwrap().contains_key(i) {
                return Err(format!("no editor session {i}"));
            }
        }
        *self.preferred.lock().unwrap() = id;
        Ok(())
    }

    pub fn list(&self) -> Vec<Session> {
        self.prune();
        self.sessions.lock().unwrap().values().map(|(s, _)| s.clone()).collect()
    }

    /// Drop sessions whose Neovim is gone.
    fn prune(&self) {
        self.sessions.lock().unwrap().retain(|_, (s, n)| n.as_ref().is_some_and(|n| n.alive()) && unsafe { libc::kill(s.pid as libc::pid_t, 0) == 0 });
    }

    /// The session to use: explicit, preferred, else the most recently active external, else embedded.
    pub fn pick(&self, id: Option<&str>) -> Result<(Session, Arc<Nvim>), String> {
        self.prune();
        let g = self.sessions.lock().unwrap();
        let chosen = id.map(str::to_string).or_else(|| self.preferred.lock().unwrap().clone()).filter(|i| g.contains_key(i)).or_else(|| {
            let mut v: Vec<&Session> = g.values().map(|(s, _)| s).collect();
            v.sort_by_key(|s| (s.kind == "external", s.last_active));
            v.last().map(|s| s.id.clone())
        });
        let Some(c) = chosen else {
            return Err("no Neovim attached: run :SpideConnect in Neovim (or open the embedded editor in Spide)".into());
        };
        let (s, n) = g.get(&c).cloned().ok_or("editor session vanished")?;
        Ok((s, n.ok_or("editor session has no RPC connection")?))
    }

    pub fn open(&self, abs: &Path, line: u32, col: u32, session: Option<&str>) -> Result<Value, String> {
        let (s, n) = self.pick(session)?;
        let r = n.plugin("open", vec![json!(abs.display().to_string()), json!(line), json!(col)])?;
        self.touch(&s.id);
        Ok(json!({"session": s.id, "kind": s.kind, "result": r}))
    }

    /// Buffer state of a file in any session: {open, dirty, changedtick, session}.
    pub fn buffer(&self, abs: &Path) -> Option<(String, Value)> {
        let all: Vec<(String, Arc<Nvim>)> = self.sessions.lock().unwrap().iter().filter_map(|(id, (_, n))| n.clone().map(|n| (id.clone(), n))).collect();
        for (id, n) in all {
            if let Ok(v) = n.plugin("buffer_state", vec![json!(abs.display().to_string())]) {
                if v["open"] == true {
                    return Some((id, v));
                }
            }
        }
        None
    }

    pub fn buffer_text(&self, abs: &Path) -> Option<(String, Value)> {
        let (id, _) = self.buffer(abs)?;
        let n = self.sessions.lock().unwrap().get(&id).and_then(|(_, n)| n.clone())?;
        n.plugin("buffer_text", vec![json!(abs.display().to_string())]).ok().map(|v| (id, v))
    }

    pub fn apply_in_buffer(&self, session: &str, abs: &Path, changedtick: i64, edits: &[Edit]) -> Result<Value, String> {
        let n = self.sessions.lock().unwrap().get(session).and_then(|(_, n)| n.clone()).ok_or("editor session vanished")?;
        let r = n.plugin("apply_edits", vec![json!(abs.display().to_string()), json!(changedtick), serde_json::to_value(edits).unwrap()])?;
        if r["ok"] != true {
            return Err(format!("conflict: {}", r["error"].as_str().unwrap_or("the buffer changed")));
        }
        Ok(r)
    }

    pub fn dirty(&self, session: &str) -> Result<Vec<String>, String> {
        let n = self.sessions.lock().unwrap().get(session).and_then(|(_, n)| n.clone()).ok_or("no such session")?;
        let v = n.plugin("dirty_buffers", vec![])?;
        Ok(v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default())
    }

    pub fn save_all(&self, session: &str) -> Result<Value, String> {
        let n = self.sessions.lock().unwrap().get(session).and_then(|(_, n)| n.clone()).ok_or("no such session")?;
        n.plugin("save_all", vec![])
    }
}

/// A staged patch (`propose_patch`), applied later by the user (UI/Neovim) or an autonomous agent.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Patch {
    pub id: String,
    pub file: String,
    pub edits: Vec<Edit>,
    /// Content hash of the file the edits were written against.
    pub base_hash: String,
    pub intent: String,
    pub diff: String,
    pub created_at: u64,
    /// pending | applied | rejected | conflict
    pub state: String,
}

pub fn patches_dir(root: &Path) -> PathBuf {
    root.join(".spide").join("patches")
}

pub fn save_patch(root: &Path, p: &Patch) -> Result<(), String> {
    crate::workspace::write_atomic(&patches_dir(root).join(format!("{}.json", p.id)), &serde_json::to_string_pretty(p).unwrap()).map_err(|e| e.to_string())
}

pub fn load_patch(root: &Path, id: &str) -> Result<Patch, String> {
    if !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err("invalid patch id".into());
    }
    serde_json::from_str(&std::fs::read_to_string(patches_dir(root).join(format!("{id}.json"))).map_err(|_| format!("no patch {id}"))?).map_err(|e| e.to_string())
}

pub fn list_patches(root: &Path) -> Vec<Patch> {
    let mut v: Vec<Patch> = std::fs::read_dir(patches_dir(root)).into_iter().flatten().flatten().filter_map(|e| serde_json::from_str(&std::fs::read_to_string(e.path()).ok()?).ok()).collect();
    v.sort_by_key(|p| std::cmp::Reverse(p.created_at));
    v
}

/// Before/after text of an applied change (for revert and the trace's detail view).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Applied {
    pub op: String,
    pub file: String,
    pub before: String,
    pub after: String,
    pub via: String,
}

pub fn applied_path(root: &Path, op: &str) -> PathBuf {
    root.join(".spide").join("trace").join("patches").join(format!("{op}.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_apply_bottom_up_and_reject_overlaps() {
        let t = "a\nb\nc\nd\n";
        let out = apply_edits(t, &[Edit { start_line: 2, end_line: 2, text: "B".into() }, Edit { start_line: 4, end_line: 3, text: "x\ny".into() }]).unwrap();
        assert_eq!(out, "a\nB\nc\nx\ny\nd\n");
        assert_eq!(apply_edits(t, &[Edit { start_line: 1, end_line: 4, text: String::new() }]).unwrap(), "\n");
        assert!(apply_edits(t, &[Edit { start_line: 1, end_line: 2, text: "x".into() }, Edit { start_line: 2, end_line: 3, text: "y".into() }]).is_err());
        assert!(apply_edits(t, &[Edit { start_line: 3, end_line: 9, text: "x".into() }]).is_err());
        let df = diff(t, &out);
        assert!(df.contains("-b\n") && df.contains("+B\n") && df.starts_with("@@ -2,"), "{df}");
    }
}
