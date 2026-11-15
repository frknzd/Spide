//! Shared application state and snapshot emission.

use serde::Serialize;
use spide_core::metrics::Metrics;
use spide_core::store::revalidate_sequences;
use spide_core::impact::{impact_of_ops, Impact};
use spide_core::{derive, Design, DecisionState, Store};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use crate::events::{Events, UiEvent};

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PiStatus {
    Starting,
    Idle,
    Busy,
    Offline,
}

pub struct Shared {
    pub store: Store,
    pub pi_generation: u64,
    pub project_dir: PathBuf,
    /// Content we last wrote to design.json (to ignore our own file events).
    pub last_written: Option<String>,
    pub pi_status: PiStatus,
    pub pi_error: Option<String>,
    pub mcp_port: u16,
    /// Fingerprint of the last history snapshot (dedup).
    pub last_history: Option<String>,
    /// Name of the active branch (design.json).
    pub branch: String,
    /// Spide Project: the code folder this design lives in (None: design-only).
    pub project: Option<crate::project::ProjectState>,
    /// Single-writer lock on the workspace (None: another process owns it; we stay read-only).
    pub lock: Option<spide_service::workspace::WorkspaceLock>,
    /// LSP/linter/editor evidence (the service's store) for health.
    pub evidence: Option<Arc<spide_service::code::EvidenceStore>>,
}

impl Shared {
    /// `spide/design.json` in a Spide Project, `design.json` otherwise.
    pub fn design_path(&self) -> PathBuf {
        if self.project.is_some() {
            self.project_dir.join(spide_core::project::DESIGN)
        } else {
            self.project_dir.join("design.json")
        }
    }
}

#[derive(Clone)]
pub struct AppState {
    pub inner: Arc<Mutex<Shared>>,
    pub events: Events,
    /// The application service all agent/editor commands go through (rebuilt on project switch).
    pub service: Arc<Mutex<Option<Arc<spide_service::Service>>>>,
    /// Bearer token for this process's MCP endpoint (given to pi via SPIDE_MCP_TOKEN, published for Neovim/agents).
    pub token: String,
    pub endpoint: Arc<Mutex<Option<spide_service::discovery::Endpoint>>>,
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
    /// Consequences if this option were chosen.
    pub impact: Option<Impact>,
}

#[derive(Serialize)]
pub struct ProposalImpact {
    pub proposal: String,
    pub impact: Impact,
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
    pub proposal_impacts: Vec<ProposalImpact>,
    pub derived: derive::Derived,
    pub can_undo: bool,
    pub can_redo: bool,
    pub project_dir: String,
    pub pi_status: PiStatus,
    pub pi_error: Option<String>,
    pub mcp_url: String,
    pub branch: String,
    pub project: Option<crate::project::ProjectSnap>,
}

impl AppState {
    pub fn new(events: Events, project_dir: PathBuf) -> Self {
        let project_dir = spide_core::project::resolve_root(&project_dir);
        let project = crate::project::load(&project_dir);
        let path = crate::project::design_path(&project_dir);
        let mut store = Store::default();
        let mut last_written = None;
        if let Ok(s) = std::fs::read_to_string(&path) {
            match Design::from_json(&s) {
                Ok(d) => {
                    store.load(d);
                    store.release_stuck_evaluations();
                    store.suggest_externals();
                    last_written = Some(s);
                }
                Err(e) => eprintln!("spide: could not parse {}: {e}", path.display()),
            }
        } else if let Some(name) = project_dir.file_name().and_then(|n| n.to_str()) {
            store.design.name = name.to_string();
        }
        let branch = crate::branches::active(&project_dir);
        let lock = acquire_lock(&project_dir);
        if lock.is_none() {
            events.send(UiEvent::Toast { kind: "error".into(), text: "Another Spide process owns this workspace; changes will not be saved here.".into() });
        }
        let mut last_history = None;
        if let Err(e) = crate::history::append(&project_dir, &store.design, &mut last_history) {
            eprintln!("spide: history: {e}");
        }
        AppState {
            inner: Arc::new(Mutex::new(Shared {
                store,
                pi_generation: 0,
                project_dir,
                last_written,
                pi_status: PiStatus::Starting,
                pi_error: None,
                mcp_port: 0,
                last_history,
                branch,
                project,
                lock,
                evidence: None,
            })),
            events,
            service: Arc::new(Mutex::new(None)),
            token: spide_service::workspace::token(),
            endpoint: Arc::new(Mutex::new(None)),
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
            s.store.suggest_externals();
            save(&mut s);
            let dirty = crate::project::recompute(&mut s);
            (build_snapshot(&s), dirty)
        };
        let (snap, dirty) = snap;
        if dirty {
            crate::project::schedule_baseline_write(self);
        }
        self.events.send(UiEvent::Snapshot(Arc::new(snap)));
    }

    /// Broadcast without saving (status changes).
    pub fn emit(&self) {
        let snap = {
            let s = self.lock();
            build_snapshot(&s)
        };
        self.events.send(UiEvent::Snapshot(Arc::new(snap)));
    }

    pub fn toast(&self, kind: &str, text: impl Into<String>) {
        self.events.send(UiEvent::Toast { kind: kind.to_string(), text: text.into() });
    }
}

/// Take the workspace's writer lock; on conflict the desktop opens read-only (and says so).
pub fn acquire_lock(dir: &Path) -> Option<spide_service::workspace::WorkspaceLock> {
    match spide_service::workspace::WorkspaceLock::acquire(dir) {
        Ok(l) => Some(l),
        Err(e) => {
            // a headless Spide serving agents hands over and forwards to us
            if let Some(l) = spide_service::discovery::take_over_from_headless(dir) {
                return Some(l);
            }
            eprintln!("spide: {e}");
            None
        }
    }
}

pub fn save(s: &mut Shared) {
    if s.lock.is_none() {
        return; // another process (e.g. spide-headless) owns this workspace
    }
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
    let dir = s.project_dir.clone();
    if let Err(e) = crate::history::append(&dir, &s.store.design, &mut s.last_history) {
        eprintln!("spide: history: {e}");
    }
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
    let started = std::time::Instant::now();
    let store = &s.store;
    let mut view = store.view();
    let mut accepted = store.design.clone();
    // Spide Project: linked methods carry the real code's facts (long methods, clones, course checks)
    if let Some(v) = crate::project::with_code_facts(s, &view) {
        view = v;
    }
    if let Some(a) = crate::project::with_code_facts(s, &accepted) {
        accepted = a;
    }
    revalidate_sequences(&mut view);
    let ev_guard = s.evidence.as_ref().map(|e| e.set.lock().unwrap_or_else(|x| x.into_inner()));
    let languages: Vec<String> = s.project.as_ref().map(|p| p.manifest.all_languages()).unwrap_or_default();
    let hctx = spide_core::health::Context { evidence: ev_guard.as_deref(), languages: &languages, now: spide_core::store::now_ms() };
    let compute = |d: &Design, ghosts: bool| spide_core::metrics::compute_with(d, ghosts, hctx);
    let metrics = compute(&accepted, false);
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
            let impact = impact_of_ops(&store.design, &o.ops).ok();
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
                        impact,
                    })
                }
                Err(e) => previews.push(OptionPreview {
                    decision: d.id.clone(),
                    option: o.id.clone(),
                    design: None,
                    health: None,
                    error: Some(e.0),
                    focus,
                    impact: None,
                }),
            }
        }
    }
    // proposals on their own (a proposal that builds on another one has no standalone impact)
    let proposal_impacts = store
        .design
        .proposals
        .iter()
        .filter_map(|p| impact_of_ops(&store.design, &p.ops).ok().map(|impact| ProposalImpact { proposal: p.id.clone(), impact }))
        .collect();
    let derived = derive::derive(&view, &metrics);
    let build_ms = started.elapsed().as_secs_f64() * 1000.0;
    #[cfg(debug_assertions)]
    if build_ms > 30.0 {
        eprintln!("spide: snapshot rev {} took {build_ms:.1} ms", store.design.rev);
    }
    Snapshot {
        rev: store.design.rev,
        view,
        metrics,
        ghost_health,
        ghost_metrics,
        previews,
        proposal_impacts,
        derived,
        can_undo: store.can_undo(),
        can_redo: store.can_redo(),
        project_dir: s.project_dir.display().to_string(),
        pi_status: s.pi_status.clone(),
        pi_error: s.pi_error.clone(),
        mcp_url: mcp_url(s.mcp_port),
        branch: s.branch.clone(),
        project: crate::project::snap(s),
    }
}

pub fn mcp_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}/mcp")
}
