//! Domain events emitted by the service. GUI repaint/toast plumbing lives in the desktop app,
//! which implements `EventSink`; headless clients receive the same events over MCP/trace.

use serde::Serialize;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum DomainEvent {
    /// The design changed (and was saved) at this revision.
    DesignChanged { rev: u64, actor: String },
    /// A trace event was recorded.
    Trace { run: String, summary: String },
    /// A client asked to reveal/select an entity (Neovim → Spide, agent → Spide).
    Reveal { symbol: String, origin: String },
    /// A client asked to open source at a location (Spide → Neovim).
    OpenSource { file: String, line: u32, column: u32, symbol: Option<String> },
    Notice { kind: String, text: String },
    /// A human (Neovim) or agent asks for a change, with the code context it points at.
    ChangeRequest { text: String, context: serde_json::Value },
    /// Editor sessions changed (attached, detached, preferred).
    Editors,
    /// LSP/linter/editor evidence changed (health must be recomputed).
    EvidenceChanged,
}

pub trait EventSink: Send + Sync {
    fn emit(&self, e: DomainEvent);
}

/// Buffers events for polling clients (headless MCP `get_events`) and tests.
#[derive(Default, Clone)]
pub struct EventLog {
    inner: Arc<Mutex<(u64, Vec<(u64, DomainEvent)>)>>,
}

impl EventLog {
    pub const CAP: usize = 1000;
    /// Events after sequence `after`, and the latest sequence number.
    pub fn since(&self, after: u64) -> (Vec<(u64, DomainEvent)>, u64) {
        let g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        (g.1.iter().filter(|(n, _)| *n > after).cloned().collect(), g.0)
    }
}

impl EventSink for EventLog {
    fn emit(&self, e: DomainEvent) {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        g.0 += 1;
        let n = g.0;
        g.1.push((n, e));
        if g.1.len() > Self::CAP {
            let drop = g.1.len() - Self::CAP;
            g.1.drain(..drop);
        }
    }
}

/// Fan-out to several sinks.
pub struct Fanout(pub Vec<Arc<dyn EventSink>>);
impl EventSink for Fanout {
    fn emit(&self, e: DomainEvent) {
        for s in &self.0 {
            s.emit(e.clone());
        }
    }
}
