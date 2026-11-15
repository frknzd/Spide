//! Append-only operational trace (`.spide/trace/journal.jsonl`, schema v1).
//!
//! Every mediated mutation writes an `intent` record before acting and an `outcome` after. An
//! intent without an outcome (crash, kill) is reported as `interrupted` on the next start. Records
//! are redacted and size-bounded. This is an operational audit trail, not a tamper-proof log.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

pub const VERSION: u32 = 1;
const MAX_FIELD: usize = 4000;
/// Keep at most this many records when compacting.
pub const RETAIN: usize = 20_000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Record {
    pub v: u32,
    pub at: u64,
    pub run: String,
    pub op: String,
    pub parent: Option<String>,
    /// intent | outcome | interrupted | note
    pub kind: String,
    pub actor: String,
    pub tool: String,
    /// Policy decision, e.g. "autonomous: allowed"
    pub policy: String,
    pub intent: String,
    pub base_rev: Option<u64>,
    pub result_rev: Option<u64>,
    pub files: Vec<String>,
    pub symbols: Vec<String>,
    /// Compact change summary (ops summary or diffstat).
    pub changes: String,
    pub health_before: Option<i64>,
    pub health_after: Option<i64>,
    pub issues_added: Vec<String>,
    pub issues_resolved: Vec<String>,
    /// e.g. ["cargo test: passed"]
    pub checks: Vec<String>,
    pub ok: Option<bool>,
    pub error: Option<String>,
    pub ms: Option<u64>,
    /// Pointer to a stored artifact (patch, proposal id, job output).
    pub artifact: Option<String>,
}

fn touched_empty(symbols: &[String], files: &[String]) -> bool {
    symbols.is_empty() && files.is_empty()
}

/// Mask obvious secrets: `key=…`, `Bearer …`, common token prefixes.
pub fn redact(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let words: Vec<&str> = s.split_inclusive(char::is_whitespace).collect();
    let mut mask_next = false;
    for w in words {
        let core = w.trim_end();
        let tail = &w[core.len()..];
        let lower = core.to_ascii_lowercase();
        let secretish = ["password=", "passwd=", "secret=", "token=", "api_key=", "apikey=", "authorization:"].iter().any(|k| lower.contains(k));
        let prefixed = ["sk-", "ghp_", "gho_", "github_pat_", "xox", "akia"].iter().any(|p| lower.starts_with(p)) && core.len() >= 16;
        if mask_next {
            out.push_str("[redacted]");
            // `Authorization: Bearer <token>`: the scheme is followed by the secret
            mask_next = lower == "bearer";
        } else if secretish {
            match core.find(['=', ':']) {
                Some(i) if i + 1 < core.len() => {
                    out.push_str(&core[..=i]);
                    out.push_str("[redacted]");
                }
                _ => {
                    out.push_str(core);
                    mask_next = true;
                }
            }
        } else if prefixed {
            out.push_str("[redacted]");
        } else {
            out.push_str(core);
            if lower == "bearer" {
                mask_next = true;
            }
        }
        out.push_str(tail);
    }
    out
}

fn bound(s: &str) -> String {
    let r = redact(s);
    if r.len() <= MAX_FIELD {
        return r;
    }
    let cut = r.char_indices().take_while(|(i, _)| *i < MAX_FIELD).last().map(|(i, c)| i + c.len_utf8()).unwrap_or(0);
    format!("{}… [{} bytes truncated]", &r[..cut], r.len() - cut)
}

impl Record {
    fn sanitized(mut self) -> Record {
        self.v = VERSION;
        self.intent = bound(&self.intent);
        self.changes = bound(&self.changes);
        self.error = self.error.map(|e| bound(&e));
        self.checks = self.checks.iter().map(|c| bound(c)).collect();
        self
    }

    /// One compact line: intent → changes → health → checks → outcome.
    pub fn summary(&self) -> String {
        let mut parts = vec![if self.intent.is_empty() { self.tool.clone() } else { self.intent.clone() }];
        let mut touched: Vec<String> = self.symbols.iter().take(4).cloned().collect();
        touched.extend(self.files.iter().take(3).cloned());
        if !touched.is_empty() {
            let more = self.symbols.len().saturating_sub(4) + self.files.len().saturating_sub(3);
            parts.push(format!("{}{}", touched.join(", "), if more > 0 { format!(" +{more}") } else { String::new() }));
        } else if !self.changes.is_empty() {
            parts.push(self.changes.lines().next().unwrap_or("").to_string());
        }
        if let (Some(a), Some(b)) = (self.health_before, self.health_after) {
            if a != b {
                parts.push(format!("health {a}→{b} ({:+})", b - a));
            }
        }
        if !touched_empty(&self.symbols, &self.files) && !self.changes.is_empty() && self.changes.contains('→') {
            parts.push(self.changes.clone());
        }
        if !self.issues_added.is_empty() || !self.issues_resolved.is_empty() {
            parts.push(format!("issues +{} −{}", self.issues_added.len(), self.issues_resolved.len()));
        }
        if !self.checks.is_empty() {
            parts.push(self.checks.join("; "));
        }
        parts.push(match (self.kind.as_str(), self.ok) {
            ("interrupted", _) => "INTERRUPTED".into(),
            (_, Some(true)) => "ok".into(),
            (_, Some(false)) => format!("FAILED: {}", self.error.clone().unwrap_or_default()),
            _ => "…".into(),
        });
        parts.join(" → ")
    }
}

pub struct Journal {
    path: PathBuf,
}

impl Journal {
    pub fn new(root: &Path) -> Journal {
        Journal { path: root.join(".spide").join("trace").join("journal.jsonl") }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn append(&self, r: Record) -> Result<(), String> {
        let r = r.sanitized();
        if let Some(d) = self.path.parent() {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        }
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&self.path).map_err(|e| e.to_string())?;
        crate::workspace::restrict(&self.path);
        let line = serde_json::to_string(&r).map_err(|e| e.to_string())?;
        // a torn previous write must not swallow this record
        let torn = {
            use std::io::{Read, Seek, SeekFrom};
            let mut rf = std::fs::File::open(&self.path).map_err(|e| e.to_string())?;
            let len = rf.metadata().map(|m| m.len()).unwrap_or(0);
            let mut last = [b'\n'];
            if len > 0 {
                let _ = rf.seek(SeekFrom::End(-1)).and_then(|_| rf.read_exact(&mut last));
            }
            last[0] != b'\n'
        };
        let line = if torn { format!("\n{line}") } else { line };
        // one write call per line: concurrent appenders don't interleave records
        f.write_all(format!("{line}\n").as_bytes()).map_err(|e| e.to_string())?;
        f.sync_data().map_err(|e| e.to_string())
    }

    /// All readable records (corrupt lines, e.g. a torn final write, are skipped).
    pub fn read(&self) -> Vec<Record> {
        let Ok(f) = std::fs::File::open(&self.path) else { return vec![] };
        std::io::BufReader::new(f).lines().map_while(Result::ok).filter_map(|l| serde_json::from_str(&l).ok()).collect()
    }

    /// Mark intents without an outcome as interrupted (call once at startup). Returns their op ids.
    pub fn recover(&self) -> Vec<String> {
        let recs = self.read();
        let closed: BTreeSet<&str> = recs.iter().filter(|r| r.kind != "intent").map(|r| r.op.as_str()).collect();
        let open: Vec<Record> = recs.iter().filter(|r| r.kind == "intent" && !closed.contains(r.op.as_str())).cloned().collect();
        let mut ids = vec![];
        for r in open {
            ids.push(r.op.clone());
            let _ = self.append(Record {
                kind: "interrupted".into(),
                ok: Some(false),
                error: Some("process ended before the outcome was recorded; verify the workspace".into()),
                at: spide_core::store::now_ms(),
                ..r
            });
        }
        ids
    }

    /// Outcomes (and interruptions) newest first, merged with their intents, filtered and paginated.
    pub fn timeline(&self, filter: &Filter) -> (Vec<Value>, usize) {
        let recs = self.read();
        let mut rows: Vec<&Record> = recs.iter().filter(|r| r.kind != "intent").collect();
        rows.reverse();
        let rows: Vec<&Record> = rows.into_iter().filter(|r| filter.matches(r)).collect();
        let total = rows.len();
        let page = rows
            .into_iter()
            .skip(filter.offset)
            .take(filter.limit.clamp(1, 500))
            .map(|r| {
                let mut v = serde_json::json!({"op": r.op, "run": r.run, "at": r.at, "actor": r.actor, "tool": r.tool, "summary": r.summary()});
                if filter.detail {
                    v["record"] = serde_json::to_value(r).unwrap();
                }
                v
            })
            .collect();
        (page, total)
    }

    /// Drop the oldest records beyond `RETAIN` (atomic rewrite).
    pub fn compact(&self) -> Result<usize, String> {
        let recs = self.read();
        if recs.len() <= RETAIN {
            return Ok(0);
        }
        let dropped = recs.len() - RETAIN;
        let text: String = recs[dropped..].iter().map(|r| serde_json::to_string(r).unwrap() + "\n").collect();
        crate::workspace::write_atomic(&self.path, &text).map_err(|e| e.to_string())?;
        Ok(dropped)
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Filter {
    /// failures | conflicts | unverified | all
    pub only: Option<String>,
    pub run: Option<String>,
    pub actor: Option<String>,
    pub offset: usize,
    pub limit: usize,
    pub detail: bool,
}

impl Filter {
    fn matches(&self, r: &Record) -> bool {
        let only = match self.only.as_deref() {
            Some("failures") => r.ok == Some(false),
            Some("conflicts") => r.error.as_deref().is_some_and(|e| e.contains("conflict") || e.contains("changed")),
            Some("unverified") => r.ok == Some(true) && r.result_rev.is_some() && r.checks.is_empty(),
            _ => true,
        };
        only && self.run.as_ref().is_none_or(|x| &r.run == x) && self.actor.as_ref().is_none_or(|x| &r.actor == x)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_secrets() {
        let s = redact("curl -H Authorization: Bearer abcdef password=hunter2 sk-0123456789abcdef0123 ok");
        assert!(!s.contains("abcdef ") && !s.contains("hunter2") && !s.contains("sk-0123"), "{s}");
        assert!(s.ends_with(" ok"));
    }

    #[test]
    fn interrupted_intents_are_recovered_once() {
        let d = tempfile::tempdir().unwrap();
        let j = Journal::new(d.path());
        j.append(Record { op: "a".into(), kind: "intent".into(), tool: "approve_decision".into(), ..Default::default() }).unwrap();
        j.append(Record { op: "b".into(), kind: "intent".into(), ..Default::default() }).unwrap();
        j.append(Record { op: "b".into(), kind: "outcome".into(), ok: Some(true), ..Default::default() }).unwrap();
        // torn final line
        std::fs::OpenOptions::new().append(true).open(j.path()).unwrap().write_all(b"{\"v\":1,\"op\":").unwrap();
        assert_eq!(j.recover(), vec!["a".to_string()]);
        assert!(j.recover().is_empty());
        let (rows, total) = j.timeline(&Filter { only: Some("failures".into()), limit: 10, ..Default::default() });
        assert_eq!(total, 1);
        assert!(rows[0]["summary"].as_str().unwrap().contains("INTERRUPTED"));
    }

    #[test]
    fn compact_summary() {
        let r = Record {
            intent: "move pricing to Order".into(),
            symbols: vec!["Order.total".into()],
            health_before: Some(70),
            health_after: Some(76),
            checks: vec!["cargo test: passed".into()],
            ok: Some(true),
            kind: "outcome".into(),
            ..Default::default()
        };
        assert_eq!(r.summary(), "move pricing to Order → Order.total → health 70→76 (+6) → cargo test: passed → ok");
    }
}
