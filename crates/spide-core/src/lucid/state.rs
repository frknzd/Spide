//! Git-like sync state of the Lucid documents (local only, in `.spide/lucid/`).
//!
//! Per diagram Spide remembers the *base*: what the Lucid document held right after the last push or
//! pull (snapshot + fingerprint) and what Spide wanted it to hold then (the target's hash). Comparing
//! the current target and the current remote with the base gives the status:
//!
//! | local target | remote | status |
//! |---|---|---|
//! | = base | = base | in sync |
//! | ≠ base | = base | local ahead → push |
//! | = base | ≠ base | remote ahead → pull |
//! | ≠ base | ≠ base | diverged → pull first |

use super::snapshot::Snapshot;
use super::Target;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const DEFAULT_FRONTIER: &str = "main";

/// Never equals a target hash: set after a pull, so the next push writes Spide's version back
/// (what the user rejected is overwritten, like a merge commit).
pub const PULLED: &str = "pulled";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocLink {
    pub document_id: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub title: String,
}

/// `.spide/lucid/config.json`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub enabled: bool,
    /// The only branch that syncs with Lucid.
    pub frontier_branch: String,
    /// Lucid folder holding the documents.
    pub folder_id: Option<String>,
    /// Diagram key → its Lucid document.
    pub documents: BTreeMap<String, DocLink>,
}

impl Default for Config {
    fn default() -> Self {
        Config { enabled: false, frontier_branch: DEFAULT_FRONTIER.into(), folder_id: None, documents: BTreeMap::new() }
    }
}

/// `.spide/lucid/base/<key>.json`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Base {
    pub key: String,
    pub document_id: String,
    /// What Lucid held after the last sync.
    pub snapshot: Snapshot,
    pub fingerprint: String,
    /// Hash of the target Spide wanted then ([`PULLED`] after a pull).
    pub target_hash: String,
    /// Design revision at the sync (informational).
    #[serde(default)]
    pub rev: u64,
    /// ms since the epoch
    #[serde(default)]
    pub synced_at: u64,
    /// Proposals the last pull created (push waits until they are decided).
    #[serde(default)]
    pub pending: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncStatus {
    Unlinked,
    InSync,
    LocalAhead,
    RemoteAhead,
    Diverged,
}

impl SyncStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            SyncStatus::Unlinked => "unlinked",
            SyncStatus::InSync => "in_sync",
            SyncStatus::LocalAhead => "local_ahead",
            SyncStatus::RemoteAhead => "remote_ahead",
            SyncStatus::Diverged => "diverged",
        }
    }
}

/// Hash of what Spide wants in the document (content and geometry, not colours).
pub fn target_hash(t: &Target) -> String {
    let mut parts: Vec<String> = t
        .items
        .iter()
        .map(|i| format!("{}|{}|{}", i.sid.clone().unwrap_or_default(), i.kind.canonical(), i.bbox.map(|b| format!("{:?}", b.grid())).unwrap_or_default()))
        .collect();
    parts.sort();
    crate::assess::fnv(&parts.join("\n"))
}

/// The status of one diagram. `remote`: the fingerprint of the document now (None: not read).
pub fn status(base: Option<&Base>, target: &str, remote: Option<&str>) -> SyncStatus {
    let Some(b) = base else { return SyncStatus::Unlinked };
    let local = target != b.target_hash;
    let theirs = remote.is_some_and(|r| r != b.fingerprint);
    match (local, theirs) {
        (false, false) => SyncStatus::InSync,
        (true, false) => SyncStatus::LocalAhead,
        (false, true) => SyncStatus::RemoteAhead,
        (true, true) => SyncStatus::Diverged,
    }
}

/// Why a sync action is refused (structured for agents).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Refusal {
    /// `not_frontier` · `disabled` · `remote_ahead` · `diverged` · `pending_proposals` · `unlinked`
    pub code: String,
    pub message: String,
}

fn refuse(code: &str, message: impl Into<String>) -> Refusal {
    Refusal { code: code.into(), message: message.into() }
}

/// Sync is allowed on the frontier branch of an enabled project only.
pub fn check_branch(cfg: &Config, active_branch: &str) -> Result<(), Refusal> {
    if !cfg.enabled {
        return Err(refuse("disabled", "Lucid sync is not enabled for this project (lucid_link enables it)."));
    }
    if active_branch != cfg.frontier_branch {
        return Err(refuse("not_frontier", format!("Lucid syncs only the {} branch; the active branch is {active_branch}. Switch to {} to push or pull.", cfg.frontier_branch, cfg.frontier_branch)));
    }
    Ok(())
}

/// Can Spide write to the document now? `pending`: proposals of the last pull still undecided.
pub fn can_push(status: SyncStatus, pending: &[String]) -> Result<(), Refusal> {
    match status {
        SyncStatus::RemoteAhead => Err(refuse("remote_ahead", "Colleagues changed the Lucid document since the last sync. Pull first (their edits become proposals), then push.")),
        SyncStatus::Diverged => Err(refuse("diverged", "Both the design and the Lucid document changed since the last sync. Pull first, decide the proposals, then push.")),
        _ if !pending.is_empty() => Err(refuse("pending_proposals", format!("Decide the proposals from the last pull first ({}).", pending.join(", ")))),
        _ => Ok(()),
    }
}

/// A base after a push: the document now holds `snapshot`, which is what `target` asked for.
pub fn after_push(key: &str, document_id: &str, target: &Target, snapshot: Snapshot, rev: u64, now: u64) -> Base {
    Base { key: key.into(), document_id: document_id.into(), fingerprint: snapshot.fingerprint(), snapshot, target_hash: target_hash(target), rev, synced_at: now, pending: vec![] }
}

/// A base after a pull: the remote is the new base; Spide's version is pushed back after the
/// proposals are decided.
pub fn after_pull(key: &str, document_id: &str, snapshot: Snapshot, rev: u64, now: u64, pending: Vec<String>) -> Base {
    Base { key: key.into(), document_id: document_id.into(), fingerprint: snapshot.fingerprint(), snapshot, target_hash: PULLED.into(), rev, synced_at: now, pending }
}

/// File-safe name of a diagram key (`sequence:Borrow book` → `sequence-Borrow_book`).
pub fn file_key(key: &str) -> String {
    let safe: String = key.chars().map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' { c } else if c == ':' { '-' } else { '_' }).collect();
    let h = crate::assess::fnv(key);
    format!("{safe}-{}", &h[..6.min(h.len())])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lucid::{Item, ItemKind};

    fn target(n: usize) -> Target {
        Target { key: "class".into(), title: "x".into(), items: (0..n).map(|i| Item::new(format!("class:C{i}"), ItemKind::Package { name: format!("C{i}"), body: String::new() }, None)).collect() }
    }

    #[test]
    fn five_states() {
        let t = target(2);
        let snap = Snapshot { items: t.items.clone(), ..Default::default() };
        let base = after_push("class", "doc", &t, snap.clone(), 1, 0);
        let fp = snap.fingerprint();
        let mut other = snap.clone();
        other.items.pop();
        let changed_fp = other.fingerprint();
        let th = target_hash(&t);
        let th2 = target_hash(&target(3));
        assert_eq!(status(None, &th, Some(&fp)), SyncStatus::Unlinked);
        assert_eq!(status(Some(&base), &th, Some(&fp)), SyncStatus::InSync);
        assert_eq!(status(Some(&base), &th2, Some(&fp)), SyncStatus::LocalAhead);
        assert_eq!(status(Some(&base), &th, Some(&changed_fp)), SyncStatus::RemoteAhead);
        assert_eq!(status(Some(&base), &th2, Some(&changed_fp)), SyncStatus::Diverged);
        // after a pull the next push writes Spide's version back
        let pulled = after_pull("class", "doc", other.clone(), 2, 0, vec!["P1".into()]);
        assert_eq!(status(Some(&pulled), &th, Some(&changed_fp)), SyncStatus::LocalAhead);
    }

    #[test]
    fn push_refusals() {
        assert_eq!(can_push(SyncStatus::RemoteAhead, &[]).unwrap_err().code, "remote_ahead");
        assert_eq!(can_push(SyncStatus::Diverged, &[]).unwrap_err().code, "diverged");
        assert_eq!(can_push(SyncStatus::LocalAhead, &["P3".into()]).unwrap_err().code, "pending_proposals");
        assert!(can_push(SyncStatus::LocalAhead, &[]).is_ok());
        assert!(can_push(SyncStatus::Unlinked, &[]).is_ok(), "the first push creates the document");
        let cfg = Config { enabled: true, ..Default::default() };
        assert!(check_branch(&cfg, "main").is_ok());
        assert_eq!(check_branch(&cfg, "experiment").unwrap_err().code, "not_frontier");
        assert_eq!(check_branch(&Config::default(), "main").unwrap_err().code, "disabled");
    }

    #[test]
    fn file_keys_are_safe_and_distinct() {
        let a = file_key("sequence:Borrow book");
        assert!(a.starts_with("sequence-Borrow_book-") && !a.contains(' '));
        assert_ne!(file_key("sequence:a b"), file_key("sequence:a_b"));
    }
}
