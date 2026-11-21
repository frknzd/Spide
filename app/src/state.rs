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

/// Application lifecycle; a failed workspace is never writable.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "kind", content = "message")]
pub enum WorkspaceStatus {
    Loading,
    Ready,
    ReadOnly,
    Failed(String),
    Closed,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct SaveStatus {
    pub saved_revision: Option<u64>,
    pub pending_revision: Option<u64>,
    pub error: Option<String>,
    pub history_error: Option<String>,
    /// The last-known-good copy could not be kept (the save itself may have succeeded).
    pub recovery_error: Option<String>,
}

pub struct Shared {
    /// Runtime preferences mirrored by Settings; never part of the design.
    pub preferences: crate::preferences::Preferences,
    /// Bumped on every workspace switch/close: work captured under an older epoch is stale.
    pub epoch: u64,
    pub workspace_status: WorkspaceStatus,
    pub save_status: SaveStatus,
    pub store: Store,
    pub pi_generation: u64,
    pub project_dir: PathBuf,
    /// Content we last wrote to design.json (to ignore our own file events).
    pub last_written: Option<String>,
    /// Exact contents last successfully saved as a separate copy. Allows leaving after Save Copy
    /// despite a failed original save; any subsequent edit invalidates the checkpoint by content.
    pub leave_copy_checkpoint: Option<String>,
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
    pub fn require_writable(&self) -> Result<(), String> {
        if self.workspace_status != WorkspaceStatus::Ready || self.lock.is_none() {
            return Err("This workspace is not writable. Open a writable project to make changes.".into());
        }
        Ok(())
    }

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
    /// One workspace transition at a time, including destination preflight.
    pub transition: Arc<Mutex<()>>,
}

#[derive(Serialize)]
pub struct OptionPreview {
    pub decision: String,
    pub option: String,
    pub design: Option<Design>,
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
    pub epoch: u64,
    pub workspace_status: WorkspaceStatus,
    pub save_status: SaveStatus,
    pub design_path: String,
    pub rev: u64,
    /// Design with pending proposals applied as ghosts.
    pub view: Design,
    /// Metrics over accepted elements.
    pub metrics: Metrics,
    /// Health if all pending proposals were accepted.
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
        Self::load(events, project_dir, crate::workspace::LoadIntent::Open)
    }

    /// No project open (welcome screen): no lock, no watchers, nothing to save.
    pub fn closed(events: Events) -> Self {
        AppState {
            inner: Arc::new(Mutex::new(Shared {
                preferences: Default::default(),
                epoch: 1,
                workspace_status: WorkspaceStatus::Closed,
                save_status: SaveStatus::default(),
                store: Store::default(),
                pi_generation: 0,
                project_dir: PathBuf::new(),
                last_written: None,
                leave_copy_checkpoint: None,
                pi_status: PiStatus::Offline,
                pi_error: None,
                mcp_port: 0,
                last_history: None,
                branch: "main".into(),
                project: None,
                lock: None,
                evidence: None,
            })),
            events,
            service: Arc::new(Mutex::new(None)),
            token: spide_service::workspace::token(),
            endpoint: Arc::new(Mutex::new(None)),
            transition: Arc::new(Mutex::new(())),
        }
    }

    pub fn create(events: Events, project_dir: PathBuf) -> Self {
        Self::load(events, project_dir, crate::workspace::LoadIntent::New)
    }

    fn load(events: Events, project_dir: PathBuf, intent: crate::workspace::LoadIntent) -> Self {
        let resolved = spide_core::project::resolve_root(&project_dir);
        let mut project_dir = resolved.canonicalize().unwrap_or(resolved);
        let mut project = None;
        let mut store = Store::default();
        let mut last_written = None;
        let mut load_error = None;
        match crate::workspace::prepare(&project_dir, intent) {
            Ok(p) => {
                project_dir = p.root;
                project = p.project;
                store.load(p.design);
                store.release_stuck_evaluations();
                store.suggest_externals();
                last_written = p.last_written;
            }
            Err(e) => load_error = Some(e),
        }
        let branch = crate::branches::active(&project_dir);
        let lock = if load_error.is_none() { acquire_lock(&project_dir) } else { None };
        if lock.is_none() && load_error.is_none() {
            events.send(UiEvent::Toast { kind: "error".into(), text: "Another Spide process owns this workspace; changes will not be saved here.".into() });
        }
        let workspace_status = match load_error {
            Some(e) => WorkspaceStatus::Failed(e),
            None if lock.is_none() => WorkspaceStatus::ReadOnly,
            None => WorkspaceStatus::Ready,
        };
        let mut save_status = SaveStatus {
            saved_revision: last_written.as_ref().map(|_| store.design.rev),
            ..Default::default()
        };
        let mut last_history = None;
        if workspace_status == WorkspaceStatus::Ready {
            save_status.history_error = crate::history::append(&project_dir, &store.design, &mut last_history).err().map(|e| e.to_string());
        }
        AppState {
            inner: Arc::new(Mutex::new(Shared {
                preferences: Default::default(),
                epoch: 1,
                workspace_status,
                save_status,
                store,
                pi_generation: 0,
                project_dir,
                last_written,
                leave_copy_checkpoint: None,
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
            transition: Arc::new(Mutex::new(())),
        }
    }

    pub fn lock(&self) -> MutexGuard<'_, Shared> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Check and mutate under the same mutex so UI commands cannot edit read-only stores.
    pub fn lock_writable(&self) -> Result<MutexGuard<'_, Shared>, String> {
        let s = self.lock();
        s.require_writable()?;
        Ok(s)
    }

    pub fn epoch(&self) -> u64 {
        self.lock().epoch
    }

    pub fn snapshot(&self) -> Snapshot {
        let s = self.lock();
        build_snapshot(&s)
    }

    /// Persist and broadcast after any change.
    pub fn changed(&self) {
        if let Err(e) = self.persist_and_emit() {
            self.toast("error", e);
        }
    }

    /// Persist without swallowing errors; always publish status, including failed saves.
    pub fn persist_and_emit(&self) -> Result<(), String> {
        let (snap, dirty, result) = {
            let mut s = self.lock();
            if s.require_writable().is_ok() {
                s.store.suggest_externals();
            }
            let result = save(&mut s);
            let dirty = result.is_ok() && crate::project::recompute(&mut s);
            (build_snapshot(&s), dirty, result)
        };
        if dirty {
            crate::project::schedule_baseline_write(self);
        }
        self.events.send(UiEvent::Snapshot(Arc::new(snap)));
        result
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

pub fn save(s: &mut Shared) -> Result<(), String> {
    s.require_writable()?;
    let json = s.store.design.to_json();
    let path = s.design_path();
    if s.last_written.as_deref() != Some(json.as_str()) {
        s.save_status.pending_revision = Some(s.store.design.rev);
        let dir = s.project_dir.clone();
        s.save_status.recovery_error = crate::recovery::keep_previous(&dir, &path, spide_core::store::now_ms()).err().map(|e| e.to_string());
        if let Err(e) = write_atomic(&path, &json) {
            let error = format!("Not saved — {}: {e}", path.display());
            s.save_status.error = Some(error.clone());
            return Err(error);
        }
        s.last_written = Some(json);
    }
    s.save_status.saved_revision = Some(s.store.design.rev);
    s.save_status.pending_revision = None;
    s.save_status.error = None;
    let dir = s.project_dir.clone();
    s.save_status.history_error = crate::history::append(&dir, &s.store.design, &mut s.last_history).err().map(|e| e.to_string());
    Ok(())
}

pub fn write_atomic(path: &Path, content: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    {
        use std::io::Write;
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(content.as_bytes())?;
        // the bytes are on disk before the rename makes them the design
        f.sync_all()?;
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    // and the rename itself survives a crash
    if let Some(dir) = path.parent() {
        std::fs::File::open(dir)?.sync_all()?;
    }
    Ok(())
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
    let ghost_metrics = (!store.design.proposals.is_empty()).then(|| compute(&view, true));
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
                    previews.push(OptionPreview {
                        decision: d.id.clone(),
                        option: o.id.clone(),
                        design: Some(p),
                        error: None,
                        focus,
                        impact,
                    })
                }
                Err(e) => previews.push(OptionPreview {
                    decision: d.id.clone(),
                    option: o.id.clone(),
                    design: None,
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
        epoch: s.epoch,
        workspace_status: s.workspace_status.clone(),
        save_status: s.save_status.clone(),
        design_path: s.design_path().display().to_string(),
        rev: store.design.rev,
        view,
        metrics,
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

#[cfg(test)]
mod persistence_tests {
    use super::*;
    use crate::{commands, service::DesktopStore};
    use spide_service::StoreHost;

    fn state(dir: &Path) -> AppState {
        if crate::project::design_path(dir).exists() {
            AppState::new(Events::new().0, dir.to_path_buf())
        } else {
            AppState::create(Events::new().0, dir.to_path_buf())
        }
    }

    #[test]
    fn failed_save_retains_work_and_retry_publishes_saved_revision() {
        let dir = tempfile::tempdir().unwrap();
        let st = state(dir.path());
        st.persist_and_emit().unwrap();
        let original = std::fs::read(dir.path().join("design.json")).unwrap();
        // A directory at the temp-file path reliably injects a failure, even as root.
        std::fs::create_dir(dir.path().join("design.json.tmp")).unwrap();
        let err = commands::set_name(&st, "Unsaved work".into()).unwrap_err();
        assert!(err.contains("Not saved"));
        let snap = st.snapshot();
        assert_eq!(snap.view.name, "Unsaved work");
        assert!(snap.save_status.error.is_some());
        assert_eq!(snap.save_status.pending_revision, Some(snap.rev));
        assert_eq!(std::fs::read(dir.path().join("design.json")).unwrap(), original);
        assert!(DesktopStore::new(&st).commit().is_err());
        std::fs::remove_dir(dir.path().join("design.json.tmp")).unwrap();
        st.persist_and_emit().unwrap();
        let snap = st.snapshot();
        assert!(snap.save_status.error.is_none());
        assert!(snap.save_status.pending_revision.is_none());
        assert_eq!(snap.save_status.saved_revision, Some(snap.rev));
        assert_eq!(Design::from_json(&std::fs::read_to_string(dir.path().join("design.json")).unwrap()).unwrap().name, "Unsaved work");
    }

    #[test]
    fn read_only_commands_and_service_cannot_mutate_or_persist() {
        let dir = tempfile::tempdir().unwrap();
        let owner = state(dir.path());
        owner.persist_and_emit().unwrap();
        let st = state(dir.path());
        assert_eq!(st.snapshot().workspace_status, WorkspaceStatus::ReadOnly);
        let before = st.lock().store.design.to_json();
        assert!(commands::set_name(&st, "Forbidden".into()).is_err());
        assert!(commands::undo(&st).is_err());
        assert!(commands::set_layout(&st, "class".into(), Default::default()).is_err());
        assert!(commands::apply_user_ops(&st, vec![]).is_err());
        assert!(commands::branch_create(&st, "forbidden".into()).is_err());
        assert!(commands::save_as(&st, dir.path().join("design.json").display().to_string()).is_err());
        let svc = crate::service::install(&st);
        assert!(svc.call_tool("apply_ops", &serde_json::json!({"expected_rev": st.snapshot().rev, "ops": []}), "user").unwrap_err().contains("read-only"));
        DesktopStore::new(&st).with(&mut |store| store.design.name = "Not retained".into());
        assert_eq!(st.lock().store.design.to_json(), before);
        assert!(st.persist_and_emit().is_err());
        assert!(!dir.path().join(".spide/branches/forbidden.json").exists());
    }

    #[test]
    fn corrupt_startup_is_failed_and_cannot_overwrite_original() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("design.json");
        std::fs::write(&path, "invalid original").unwrap();
        let st = state(dir.path());
        assert!(matches!(st.snapshot().workspace_status, WorkspaceStatus::Failed(_)));
        assert!(commands::set_name(&st, "Blank".into()).is_err());
        assert!(st.persist_and_emit().is_err());
        assert_eq!(std::fs::read_to_string(path).unwrap(), "invalid original");
        assert!(!dir.path().join(".spide/history.jsonl").exists());
    }

    #[test]
    fn history_failure_does_not_misreport_a_successful_design_save() {
        let dir = tempfile::tempdir().unwrap();
        let st = state(dir.path());
        let history = dir.path().join(".spide/history.jsonl");
        if history.is_file() { std::fs::remove_file(&history).unwrap(); }
        std::fs::create_dir_all(&history).unwrap();
        commands::set_name(&st, "Saved without history".into()).unwrap();
        let snap = st.snapshot();
        assert!(snap.save_status.error.is_none());
        assert!(snap.save_status.history_error.is_some());
        assert_eq!(snap.save_status.saved_revision, Some(snap.rev));
    }
}
