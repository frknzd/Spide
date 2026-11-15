//! The queue of agent assessment requests: scoped (dimensions), deduplicated (one pending request
//! per dimension), bounded, cancellable, persisted in `.spide/assessments.json`. Nothing calls an
//! LLM per keystroke: requests are explicit (user, agent) and the agent claims them via MCP.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const MAX_PENDING: usize = 8;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Request {
    pub id: String,
    pub dimensions: Vec<String>,
    pub reason: String,
    pub created_at: u64,
    /// pending | done | cancelled
    pub state: String,
}

pub struct Queue {
    path: PathBuf,
    items: Mutex<Vec<Request>>,
}

impl Queue {
    pub fn new(root: &Path) -> Queue {
        let path = root.join(".spide").join("assessments.json");
        let items = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        Queue { path, items: Mutex::new(items) }
    }

    fn save(&self, items: &[Request]) {
        let keep: Vec<&Request> = items.iter().rev().take(100).collect::<Vec<_>>().into_iter().rev().collect();
        let _ = crate::workspace::write_atomic(&self.path, &serde_json::to_string_pretty(&keep).unwrap_or_default());
    }

    pub fn pending(&self) -> Vec<Request> {
        self.items.lock().unwrap().iter().filter(|r| r.state == "pending").cloned().collect()
    }

    pub fn request(&self, dimensions: Vec<String>, reason: &str) -> Result<String, String> {
        for d in &dimensions {
            if !spide_core::health::is_dimension(d) {
                return Err(format!("unknown dimension {d}"));
            }
        }
        let mut g = self.items.lock().unwrap();
        let pending: Vec<&Request> = g.iter().filter(|r| r.state == "pending").collect();
        let fresh: Vec<String> = dimensions.into_iter().filter(|d| !pending.iter().any(|r| r.dimensions.contains(d))).collect();
        if fresh.is_empty() {
            let id = pending.first().map(|r| r.id.clone()).unwrap_or_default();
            return Ok(id);
        }
        if pending.len() >= MAX_PENDING {
            return Err(format!("{MAX_PENDING} assessment requests are already pending; cancel or complete some"));
        }
        let id = format!("A{}", spide_core::store::now_ms());
        g.push(Request { id: id.clone(), dimensions: fresh, reason: reason.to_string(), created_at: spide_core::store::now_ms(), state: "pending".into() });
        self.save(&g);
        Ok(id)
    }

    /// Mark requests covering these dimensions done.
    pub fn complete(&self, dims: &[String]) {
        let mut g = self.items.lock().unwrap();
        for r in g.iter_mut().filter(|r| r.state == "pending") {
            r.dimensions.retain(|d| !dims.contains(d));
            if r.dimensions.is_empty() {
                r.state = "done".into();
            }
        }
        self.save(&g);
    }

    pub fn cancel(&self, id: &str) -> Result<(), String> {
        let mut g = self.items.lock().unwrap();
        let r = g.iter_mut().find(|r| r.id == id && r.state == "pending").ok_or_else(|| format!("no pending request {id}"))?;
        r.state = "cancelled".into();
        self.save(&g);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedup_bound_cancel_complete() {
        let d = tempfile::tempdir().unwrap();
        let q = Queue::new(d.path());
        let a = q.request(vec!["modularity".into(), "simplicity".into()], "changed").unwrap();
        assert_eq!(q.request(vec!["modularity".into()], "again").unwrap(), a, "already pending");
        let b = q.request(vec!["modularity".into(), "extensibility".into()], "x").unwrap();
        assert_ne!(a, b);
        assert_eq!(q.pending()[1].dimensions, vec!["extensibility".to_string()]);
        q.complete(&["modularity".into(), "simplicity".into()]);
        assert_eq!(q.pending().len(), 1);
        q.cancel(&b).unwrap();
        assert!(q.pending().is_empty());
        assert!(q.request(vec!["vibes".into()], "").is_err());
        assert!(Queue::new(d.path()).pending().is_empty(), "persisted");
    }
}
