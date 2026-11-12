//! Shared application state and snapshot emission.

use serde::Serialize;
use spide_core::metrics::{compute, Metrics};
use spide_core::store::revalidate_sequences;
use spide_core::{derive, Design, DecisionState, Store};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use tauri::{AppHandle, Emitter};

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PiStatus {
    Starting,
    Idle,
    Busy,
    Offline,
}

pub struct Shared {
    pub store: Store,
    pub project_dir: PathBuf,
    /// Content we last wrote to design.json (to ignore our own file events).
    pub last_written: Option<String>,
    pub pi_status: PiStatus,
    pub pi_error: Option<String>,
    pub mcp_port: u16,
}

impl Shared {
    pub fn design_path(&self) -> PathBuf {
        self.project_dir.join("design.json")
    }
}

#[derive(Clone)]
pub struct AppState {
    pub inner: Arc<Mutex<Shared>>,
    pub app: AppHandle,
}

#[derive(Serialize)]
pub struct OptionPreview {
    pub decision: String,
    pub option: String,
    pub design: Option<Design>,
    pub health: Option<u8>,
    pub error: Option<String>,
    /// Classes touched by the option (new or changed) — the preview focuses on these.
    pub focus: Vec<String>,
}

#[derive(Serialize)]
pub struct Snapshot {
    pub rev: u64,
    /// Design with pending proposals applied as ghosts.
    pub view: Design,
    /// Metrics over accepted elements.
    pub metrics: Metrics,
    /// Health if all pending proposals were accepted.
    pub ghost_health: Option<u8>,
    /// Ghost metrics (heat etc. for proposed classes too).
    pub ghost_metrics: Option<Metrics>,
    pub previews: Vec<OptionPreview>,
    pub derived: derive::Derived,
    pub can_undo: bool,
    pub can_redo: bool,
    pub project_dir: String,
    pub pi_status: PiStatus,
    pub pi_error: Option<String>,
    pub mcp_url: String,
}

impl AppState {
    pub fn new(app: AppHandle, project_dir: PathBuf) -> Self {
        let path = project_dir.join("design.json");
        let mut store = Store::default();
        let mut last_written = None;
        if let Ok(s) = std::fs::read_to_string(&path) {
            match Design::from_json(&s) {
                Ok(d) => {
                    store.load(d);
                    last_written = Some(s);
                }
                Err(e) => eprintln!("spide: could not parse {}: {e}", path.display()),
            }
        } else if let Some(name) = project_dir.file_name().and_then(|n| n.to_str()) {
            store.design.name = name.to_string();
        }
        AppState {
            inner: Arc::new(Mutex::new(Shared {
                store,
                project_dir,
                last_written,
                pi_status: PiStatus::Starting,
                pi_error: None,
                mcp_port: 0,
            })),
            app,
        }
    }

    pub fn lock(&self) -> MutexGuard<'_, Shared> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn snapshot(&self) -> Snapshot {
        let s = self.lock();
        build_snapshot(&s)
    }

    /// Persist and broadcast after any change.
    pub fn changed(&self) {
        let snap = {
            let mut s = self.lock();
            save(&mut s);
            build_snapshot(&s)
        };
        let _ = self.app.emit("snapshot", &snap);
    }

    /// Broadcast without saving (status changes).
    pub fn emit(&self) {
        let snap = self.snapshot();
        let _ = self.app.emit("snapshot", &snap);
    }

    pub fn toast(&self, kind: &str, text: impl Into<String>) {
        let _ = self.app.emit("toast", serde_json::json!({ "kind": kind, "text": text.into() }));
    }
}

pub fn save(s: &mut Shared) {
    let json = s.store.design.to_json();
    if s.last_written.as_deref() == Some(json.as_str()) {
        return;
    }
    let path = s.design_path();
    if let Err(e) = write_atomic(&path, &json) {
        eprintln!("spide: save failed: {e}");
        return;
    }
    s.last_written = Some(json);
}

pub fn write_atomic(path: &Path, content: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, content)?;
    std::fs::rename(&tmp, path)
}

pub fn build_snapshot(s: &Shared) -> Snapshot {
    let store = &s.store;
    let mut view = store.view();
    revalidate_sequences(&mut view);
    let metrics = compute(&view, false);
    let (ghost_health, ghost_metrics) = if store.design.proposals.is_empty() {
        (None, None)
    } else {
        let gm = compute(&view, true);
        (Some(gm.health), Some(gm))
    };
    let mut previews = vec![];
    for d in store.design.decisions.iter().filter(|d| d.is_pending() || d.state == DecisionState::Accepted) {
        if !d.is_pending() {
            continue;
        }
        for o in &d.options {
            let tag = format!("{}:{}", d.id, o.id);
            let focus: Vec<String> = o.ops.iter().flat_map(|op| op.touched_classes()).collect();
            match store.preview(&o.ops, &tag) {
                Ok(p) => {
                    let h = compute(&p, true).health;
                    previews.push(OptionPreview {
                        decision: d.id.clone(),
                        option: o.id.clone(),
                        design: Some(p),
                        health: Some(h),
                        error: None,
                        focus,
                    })
                }
                Err(e) => previews.push(OptionPreview {
                    decision: d.id.clone(),
                    option: o.id.clone(),
                    design: None,
                    health: None,
                    error: Some(e.0),
                    focus,
                }),
            }
        }
    }
    let derived = derive::derive(&view, &metrics);
    Snapshot {
        rev: store.design.rev,
        view,
        metrics,
        ghost_health,
        ghost_metrics,
        previews,
        derived,
        can_undo: store.can_undo(),
        can_redo: store.can_redo(),
        project_dir: s.project_dir.display().to_string(),
        pi_status: s.pi_status.clone(),
        pi_error: s.pi_error.clone(),
        mcp_url: mcp_url(s.mcp_port),
    }
}

pub fn mcp_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}/mcp")
}
