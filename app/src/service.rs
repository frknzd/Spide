//! The desktop's handle on `spide-service`: the app's shared state is the service's store, and
//! the desktop contributes its Spide Project tools. All MCP traffic (pi and external agents) goes
//! through `Service::call_tool`, so policy, revision guards and the trace apply identically to
//! the headless runtime.

use crate::state::AppState;
use serde_json::Value;
use spide_core::Store;
use spide_service::events::{DomainEvent, EventSink};
use spide_service::{Extension, Host, Service, StoreHost, ToolResult};
use std::path::PathBuf;
use std::sync::Arc;

/// The desktop store as seen by one service instance. A service built for an earlier workspace
/// epoch keeps its handle after a switch: it must never read from or write to the new store.
pub struct DesktopStore(pub AppState, pub u64);

impl DesktopStore {
    pub fn new(st: &AppState) -> Self {
        Self(st.clone(), st.epoch())
    }
    fn current(&self) -> bool {
        self.0.epoch() == self.1
    }
}

impl StoreHost for DesktopStore {
    fn root(&self) -> PathBuf {
        self.0.lock().project_dir.clone()
    }
    fn with(&self, f: &mut dyn FnMut(&mut Store)) {
        let mut s = self.0.lock();
        if s.epoch == self.1 && s.require_writable().is_ok() {
            f(&mut s.store);
        } else {
            // Read tools may normalize data internally; never let that modify a
            // failed or read-only workspace, even through a retained service handle.
            f(&mut s.store.clone());
        }
    }
    fn commit(&self) -> Result<(), String> {
        if !self.current() {
            return Err("The project changed; this request was cancelled.".into());
        }
        self.0.persist_and_emit()
    }
    fn writer(&self) -> bool {
        let s = self.0.lock();
        s.epoch == self.1 && s.require_writable().is_ok()
    }
    fn on_branch(&self, name: &str) {
        self.0.lock().branch = name.to_string();
    }
    fn code_snapshot(&self) -> Option<spide_service::code::CodeSnapshot> {
        let s = self.0.lock();
        let p = s.project.as_ref()?;
        Some(spide_service::code::CodeSnapshot {
            design: p.index.code.clone()?,
            graph: p.index.graph.clone().unwrap_or_default(),
            files: p.index.files,
            skipped: p.index.skipped.clone(),
            languages: p.manifest.all_languages(),
        })
    }
}

/// Domain events → desktop (notices become toasts; reveal/open are handled by the editor bridge).
pub struct DesktopSink(pub AppState, pub u64);

impl EventSink for DesktopSink {
    fn emit(&self, e: DomainEvent) {
        if self.0.epoch() != self.1 {
            return; // an event from a previous workspace's service
        }
        match e {
            DomainEvent::Notice { kind, text } => self.0.toast(&kind, text),
            DomainEvent::Reveal { symbol, origin } => crate::editor::on_reveal(&self.0, &symbol, &origin),
            DomainEvent::EvidenceChanged => self.0.emit(),
            DomainEvent::AssessmentRequested { dimensions, reason, .. } => self.0.events.send(crate::events::UiEvent::AssessmentRequested { dimensions, reason }),
            DomainEvent::ChangeRequest { text, context } => self.0.events.send(crate::events::UiEvent::ChangeRequest { text, context }),
            _ => {}
        }
    }
}

/// Spide Project tools (code index, sync) that only the desktop computes today.
pub struct ProjectTools(pub AppState, pub u64);

impl Extension for ProjectTools {
    fn tools(&self) -> Vec<Value> {
        if self.0.lock().project.is_some() {
            crate::project::tool_list()
        } else {
            vec![]
        }
    }

    fn call(&self, _svc: &Service, name: &str, args: &Value) -> Option<Result<ToolResult, String>> {
        let st = &self.0;
        if st.epoch() != self.1 {
            return Some(Err("The project changed; this request was cancelled.".into()));
        }
        Some(match name {
            "get_metrics" => Ok(ToolResult::text(serde_json::to_string(&st.snapshot().metrics).unwrap())),
            "get_project" => {
                let s = st.lock();
                if s.project.is_none() {
                    Err("This folder is not a Spide Project (design only).".to_string())
                } else {
                    Ok(ToolResult::text(crate::project::summary(&s).to_string()))
                }
            }
            "get_sync" => crate::project::get_sync(&st.lock(), args).map(ToolResult::text),
            "ui_command" => {
                let c = args.get("command").and_then(Value::as_str).unwrap_or("").trim().to_string();
                // view commands only: anything that changes the design or project goes through its own (policy-checked) tool
                const VIEW: [&str; 9] = ["/lens", "/detail", "/tidy", "/focus", "/implicit", "/pruned", "/stress", "/fit", "/zoom"];
                if c.contains('\n') || !VIEW.iter().any(|v| c == *v || c.starts_with(&format!("{v} "))) {
                    return Some(Err(format!("ui_command accepts view commands only: {}", VIEW.join(" "))));
                }
                st.events.send(crate::events::UiEvent::Command(c));
                Ok(ToolResult::text("sent to the canvas"))
            }
            "mark_in_sync" => crate::commands::mark_synced(st).map(|n| ToolResult::text(format!("{n} items marked in sync"))),
            "get_code" => crate::project::get_code(&st.lock(), args).map(ToolResult::text),
            "get_design" => {
                let s = st.lock();
                let mut store = s.store.clone();
                spide_core::tools::call(&mut store, name, args).map(|mut outcome| {
                    if s.project.is_some() {
                        if let Ok(mut value) = serde_json::from_str::<Value>(&outcome.text) {
                            value["project"] = crate::project::summary(&s);
                            outcome.text = value.to_string();
                        }
                    }
                    ToolResult::text(outcome.text)
                })
            }
            _ => return None,
        })
    }
}

/// (Re)build the service for the current project (startup and project switches).
pub fn install(st: &AppState) -> Arc<Service> {
    let writer = st.lock().require_writable().is_ok();
    let svc = Arc::new(Service::new(Arc::new(DesktopStore::new(st)), writer, Host::Desktop, Some(Arc::new(DesktopSink(st.clone(), st.epoch())))));
    svc.add_extension(Arc::new(ProjectTools(st.clone(), st.epoch())));
    st.lock().evidence = Some(svc.evidence.clone());
    crate::editor::install_extension(st, &svc);
    *st.service.lock().unwrap_or_else(|e| e.into_inner()) = Some(svc.clone());
    publish(st);
    svc
}

/// Publish the authenticated endpoint for Neovim and external agents.
pub fn publish(st: &AppState) {
    let (root, port, writer) = {
        let s = st.lock();
        (s.project_dir.clone(), s.mcp_port, s.require_writable().is_ok())
    };
    if !writer || port == 0 {
        return;
    }
    let ep = spide_service::discovery::Endpoint {
        version: 1,
        root: root.display().to_string(),
        pid: std::process::id(),
        host: "desktop".into(),
        mcp_url: Some(crate::state::mcp_url(port)),
        editor_socket: crate::editor::socket_path(st).map(|p| p.display().to_string()),
        token: st.token.clone(),
        started_at: spide_core::store::now_ms(),
    };
    if let Err(e) = ep.publish() {
        eprintln!("spide: discovery: {e}");
    }
    *st.endpoint.lock().unwrap_or_else(|e| e.into_inner()) = Some(ep);
}

pub fn retract(st: &AppState) {
    if let Some(ep) = st.endpoint.lock().unwrap_or_else(|e| e.into_inner()).take() {
        ep.retract();
    }
}

pub fn get(st: &AppState) -> Option<Arc<Service>> {
    st.service.lock().unwrap_or_else(|e| e.into_inner()).clone()
}
