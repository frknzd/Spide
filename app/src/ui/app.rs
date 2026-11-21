//! The application: UI state, the frame loop, keyboard shortcuts and the `/spide` command dispatcher
//! (port of `App.tsx`).

use super::brief::{self, BriefOut, BriefUi};
use super::canvas::{CanvasInput, CanvasOut, ClassCanvas};
use super::chrome::{self, HealthAction, HealthState, Tab, Toast};
use super::decisions::{self, CompareCol, CompareUi, DecisionUi};
use super::export::{self, DialogOut, ExportDialog, ExportSettings, ExportState, Format};
use super::export_jobs::{self, BatchPhase, BatchRun, BatchSettings, ExportView, JobResult};
use super::hover_card::{self, CardAction, CardCtx};
use super::overlays::{self, InlineAction, InlineEdit, InlineKind, Palette, PaletteAction, PruneAction, PruneState};
use super::paint::EguiMeasure;
use super::proposal::{self, ProposalOut, ProposalUi};
use super::state::hover::{HoverStore, Point};
use super::state::proposal_review::PlanDrafts;
use super::terminal::{self, PaneAction, TermSession};
use super::theme::{bold, css, sans};
use super::timeline::{self, TimelineOut, TimelineUi};
use super::views::{ViewOut, Views};
use super::widgets;
use crate::backend::Backend;
use crate::events::{MenuAction, UiEvent};
use crate::state::{PiStatus, Snapshot};
use egui::{Color32, Key, Margin, Pos2, Rect, RichText, Sense, Stroke, Vec2};
use serde_json::json;
use spide_core::model::{Class, Design, DecisionState, Package, Relation, Status};
use spide_core::Target;
use spide_diagram::class_view::{Lens, Stress, SyncSets};
use spide_core::sync::SyncStatus;
use spide_diagram::scene::Scene;
use spide_diagram::svg::{build_export_svg, ExportMeta, ExportOptions, LegendKey, LEGEND_ORDER};
use spide_diagram::views::C4Level;
use spide_diagram::visuals::DETAIL_LABELS;
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::sync::mpsc::Receiver;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WorkspaceSettings {
    pub left: f32,
    pub right: f32,
    pub left_open: bool,
    pub right_open: bool,
}

impl Default for WorkspaceSettings {
    fn default() -> Self {
        Self { left: 420.0, right: 360.0, left_open: true, right_open: true }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PreviewSource {
    Option,
    History,
}

pub struct BranchCmp {
    pub name: String,
    pub cols: Vec<CompareCol>,
}

/// The class-diagram data shown right now (live, preview or review).
pub struct ClassData {
    pub key: u64,
    pub classes: Vec<Class>,
    pub relations: Vec<Relation>,
    pub packages: Vec<Package>,
    pub implicit: Vec<spide_core::metrics::ImplicitEdge>,
    pub scoped_metrics: Option<spide_core::metrics::Metrics>,
    pub saved: Option<std::collections::BTreeMap<String, spide_core::model::Pos>>,
}

type Deferred = Box<dyn FnOnce(&mut SpideApp)>;

pub struct SpideApp {
    pub ctx: egui::Context,
    pub backend: Backend,
    rx: Receiver<UiEvent>,
    pub snap: Option<Arc<Snapshot>>,
    pub measure: EguiMeasure,
    pub tab: Tab,
    pub toasts: Vec<Toast>,
    toast_seq: u64,
    pub hover: HoverStore,
    card_selected: Option<String>,
    health: HealthState,
    pub workspace: WorkspaceSettings,
    drawer: bool,
    dragging_sep: Option<(bool, f32, f32)>,
    pub term: Option<TermSession>,
    /// The embedded Neovim's terminal (created on first use) and whether the pane shows it.
    pub nvim_term: Option<TermSession>,
    pub trace_ui: super::trace::TraceUi,
    pub editor_pane: bool,
    term_confirm: bool,
    pub(super) restarting: bool,
    pane_palette: Option<Palette>,
    pub palette: Option<Palette>,
    pub prune: Option<PruneState>,
    pub inline: Option<InlineEdit>,
    pub brief: bool,
    pub brief_ui: BriefUi,
    pub compare: Option<String>,
    compare_ui: CompareUi,
    pub branch_cmp: Option<BranchCmp>,
    pub stress: Option<String>,
    pub review_active: bool,
    pub selected_proposal: Option<String>,
    pub proposal_canvas: Option<Design>,
    pub proposal_ui: ProposalUi,
    pub shown_decision: Option<String>,
    /// the inspector lists resolved decisions (read-only history)
    pub history_open: bool,
    /// the Project rail (Spide Project / sync)
    pub project_open: bool,
    pub project_ui: super::project::ProjectUi,
    /// fit the canvas to this plan's changes once its review has loaded (deliberate navigation)
    pub review_locate: Option<(String, f64)>,
    /// plans whose review layout has been shown: keep positions when toggling before/after or alternatives
    review_seen: HashSet<String>,
    /// last canvas positions during review, so elements that disappear (before view) return in place
    review_pos: std::collections::BTreeMap<String, spide_core::model::Pos>,
    pub timeline: bool,
    pub timeline_ui: TimelineUi,
    pub preview: Option<Arc<Design>>,
    pub preview_source: PreviewSource,
    preview_gen: u64,
    pub scenario: Option<String>,
    pub show_implicit: bool,
    pub lens: Lens,
    pub decision_ui: DecisionUi,
    pub class_canvas: Option<ClassCanvas>,
    pub(super) package_scope: Option<String>,
    pub(super) full_class_canvas: Option<ClassCanvas>,
    pub domain_canvas: Option<ClassCanvas>,
    /// off-screen canvases for exports at other detail levels (same layout pipeline)
    pub export_canvases: HashMap<String, ClassCanvas>,
    pub views: Views,
    class_data: Option<Arc<ClassData>>,
    domain_data: Option<Arc<ClassData>>,
    /// ghost spotlight from the proposal bar: Some(None) = all ghosts
    pub spotlight: Option<Option<String>>,
    pub export: Option<ExportDialog>,
    pub export_settings: HashMap<String, ExportSettings>,
    seen_commands: VecDeque<String>,
    deferred: Vec<(f64, Deferred)>,
    pub(super) canvas_rect: Rect,
    last_tab: Tab,
    last_rev: Option<(u64, String)>,
    pub exit_requested: bool,
    /// tests: scripted answers for leave questions (None = cancel)
    pub leave_answers: Option<VecDeque<Option<usize>>>,
    pub leave_copy_path: Option<String>,
    /// the user confirmed closing the window: let the next close request through
    close_confirmed: bool,
    /// local preferences (recent projects, startup); None path: not persisted (tests)
    pub prefs: crate::preferences::Preferences,
    pub prefs_path: Option<std::path::PathBuf>,
    pub new_project: Option<super::welcome::NewProject>,
    /// why the last open from the welcome screen failed
    pub open_error: Option<String>,
    last_title: String,
    /// Find in Design / Go to Element
    pub find: Option<super::search::FindUi>,
    /// back/forward over deliberate navigation
    pub nav: super::search::NavHistory,
    /// selected diagram elements (the last one is "the" selection)
    pub selection: Vec<Target>,
    pub show_shortcuts: bool,
    pub settings_open: bool,
    /// the branch chooser
    pub branches: Option<super::branches::BranchUi>,
    /// focus to give back when overlays close: 1 pi, 2 Neovim, 0 none
    return_focus: Option<u8>,
    /// right-click menu on diagram elements
    pub context: Option<super::selection::ContextMenu>,
    pub class_editor: Option<super::properties::ClassEditor>,
    pub new_element: Option<super::properties::NewElement>,
    /// the last copied elements (also on the system clipboard)
    pub app_clipboard: Option<super::clipboard::Payload>,
    pub pending_cameras: std::collections::BTreeMap<String, super::workspace_memory::Camera>,
    last_workspace_save: f64,
    pub input_drafts: super::draft_recovery::InputDrafts,
    pub draft_notice: Option<String>,
    pub recovered_feedback: Option<String>,
    last_draft_save: f64,
    pub activity: super::activity::ActivityLog,
    pub activity_open: bool,
    pub activity_errors_only: bool,
    pub last_operation_status: String,
    pub integrations: super::integrations::IntegrationUi,
    pub documentation_open: bool,
    pub documentation_query: String,
    pub about_open: bool,
    pub open_request_pending: bool,
    pub copying_image: bool,
}

fn hash_of(v: impl std::hash::Hash) -> u64 {
    use std::hash::Hasher;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    v.hash(&mut h);
    h.finish()
}

/// `20251114-104700` (batch folder names).
fn chrono_stamp() -> String {
    let t = super::format_time(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0));
    // dd/mm/yyyy, hh:mm:ss
    let d: Vec<&str> = t.split(|c: char| !c.is_ascii_digit()).filter(|p| !p.is_empty()).collect();
    if d.len() >= 6 {
        format!("{}{}{}-{}{}{}", d[2], d[1], d[0], d[3], d[4], d[5])
    } else {
        "export".into()
    }
}

fn slug(s: Option<&str>) -> String {
    let s: String = s.unwrap_or("design").to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    let s = s.split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-");
    if s.is_empty() {
        "design".into()
    } else {
        s
    }
}

/// Best class-name match: exact, then prefix, then substring (case-insensitive).
fn find_class(d: &Design, q: &str) -> Option<String> {
    let names: Vec<&str> = d.classes.iter().filter(|c| c.status != Status::Pruned).map(|c| c.name.as_str()).collect();
    let l = q.to_lowercase();
    names
        .iter()
        .find(|n| n.to_lowercase() == l)
        .or_else(|| names.iter().find(|n| n.to_lowercase().starts_with(&l)))
        .or_else(|| names.iter().find(|n| n.to_lowercase().contains(&l)))
        .map(|s| s.to_string())
}

/// Is the target currently pruned (so a click restores it)?
fn is_pruned(d: &Design, t: &Target) -> bool {
    let cls = |n: &str| d.classes.iter().find(|c| c.name == n);
    let st = match t {
        Target::Class { name } => cls(name).map(|c| c.status),
        Target::Attribute { class, name } => cls(class).and_then(|c| c.attributes.iter().find(|a| &a.name == name)).map(|a| a.status),
        Target::Method { class, name } => cls(class).and_then(|c| c.methods.iter().find(|a| &a.name == name)).map(|a| a.status),
        Target::Relation { id } => d.relations.iter().find(|r| &r.id == id).map(|r| r.status),
        Target::Package { name } => d.packages.iter().find(|p| &p.name == name).map(|p| p.status),
        Target::Actor { name } => d.use_cases.actors.iter().find(|a| &a.name == name).map(|a| a.status),
        Target::UseCase { name } => d.use_cases.use_cases.iter().find(|a| &a.name == name).map(|a| a.status),
        Target::Sequence { name } => d.sequences.iter().find(|a| &a.name == name).map(|a| a.status),
        Target::Message { sequence, index } => d.sequences.iter().find(|s| &s.name == sequence).and_then(|s| s.messages.get(*index)).map(|m| m.status),
        Target::System { name } => d.systems.iter().find(|a| &a.name == name).map(|a| a.status),
        Target::Container { name } => d.containers.iter().find(|a| &a.name == name).map(|a| a.status),
        Target::Link { id } => d.links.iter().find(|l| &format!("{}->{}", l.from, l.to) == id).map(|l| l.status),
        Target::Concept { name } => d.domain.concepts.iter().find(|c| &c.name == name).map(|c| c.status),
        Target::ConceptLink { id } => d.domain.associations.iter().find(|a| &a.id == id).map(|a| a.status),
        _ => None,
    };
    st == Some(Status::Pruned)
}

/// Domain tab: the class canvas draws concepts as classes. Map its targets/hover keys onto the
/// domain model so nothing reaches the design's classes (that may share a name).
fn domain_target(t: Target) -> Option<Target> {
    match t {
        Target::Class { name } => Some(Target::Concept { name }),
        Target::Relation { id } => Some(Target::ConceptLink { id }),
        _ => None,
    }
}

fn domain_hover_key(k: &str) -> Option<String> {
    let (kind, rest) = k.split_once(':')?;
    match kind {
        "class" => Some(format!("concept:{rest}")),
        "relation" => Some(format!("concept_link:{rest}")),
        "attribute" => rest.split_once('.').map(|(c, _)| format!("concept:{c}")),
        _ => None,
    }
}

pub(super) fn domain_canvas_out(o: CanvasOut) -> Option<CanvasOut> {
    match o {
        CanvasOut::Hover(k, p) => Some(CanvasOut::Hover(k.and_then(|k| domain_hover_key(&k)), p)),
        CanvasOut::Prune(ts, p) => {
            let ts: Vec<Target> = ts.into_iter().filter_map(domain_target).collect();
            (!ts.is_empty()).then_some(CanvasOut::Prune(ts, p))
        }
        CanvasOut::Click(t, p) => domain_target(t).map(|t| CanvasOut::Click(t, p)),
        CanvasOut::Context(t, p) => domain_target(t).map(|t| CanvasOut::Context(t, p)),
        CanvasOut::Select(ts, add) => Some(CanvasOut::Select(ts.into_iter().filter_map(domain_target).collect(), add)),
        // no renames, no packages in the domain model
        CanvasOut::DoubleClick(..) | CanvasOut::MoveToPackage(..) => None,
        o @ CanvasOut::SaveLayout(_) => Some(o),
    }
}

/// Origin (decision / proposal / user) of a clicked element.
fn origin_of(d: &Design, t: &Target) -> Option<String> {
    let cls = |n: &str| d.classes.iter().find(|c| c.name == n);
    match t {
        Target::Class { name } => cls(name).and_then(|c| c.origin.clone()),
        Target::Attribute { class, name } => cls(class).and_then(|c| c.attributes.iter().find(|a| &a.name == name)).and_then(|a| a.origin.clone()),
        Target::Method { class, name } => cls(class).and_then(|c| c.methods.iter().find(|a| &a.name == name)).and_then(|a| a.origin.clone()),
        Target::Relation { id } => d.relations.iter().find(|r| &r.id == id).and_then(|r| r.origin.clone()),
        _ => None,
    }
}

impl SpideApp {
    pub fn new(cc: &eframe::CreationContext<'_>, backend: Backend, rx: Receiver<UiEvent>) -> Self {
        let mut app = Self::with_ctx(&cc.egui_ctx, backend, rx, true);
        if let Some(s) = cc.storage {
            if let Some(w) = eframe::get_value::<WorkspaceSettings>(s, "spide.workspace.v1") {
                app.workspace = WorkspaceSettings { left: w.left.clamp(280.0, 900.0), right: w.right.clamp(300.0, 900.0), ..w };
            }
            if let Some(e) = eframe::get_value::<HashMap<String, ExportSettings>>(s, "spide.export.v1") {
                app.export_settings = e;
            }
        }
        app.prefs_path = crate::preferences::default_path();
        app.prefs = app.prefs_path.as_deref().map(crate::preferences::Preferences::load).unwrap_or_default();
        if let Some(p) = app.prefs_path.as_ref().and_then(|p| p.parent()) {
            app.activity = super::activity::ActivityLog::load(p.join("activity.json"));
        }
        app.input_drafts = match app.prefs_path.as_ref().and_then(|p| p.parent()) {
            Some(p) => super::draft_recovery::InputDrafts::load(p.join("input-drafts.json")),
            None => Default::default(),
        };
        app.apply_preferences();
        app.restore_input_drafts();
        app.restore_workspace();
        app.record_recent();
        app
    }

    pub fn with_ctx(ctx: &egui::Context, backend: Backend, rx: Receiver<UiEvent>, terminal: bool) -> Self {
        super::theme::install_fonts(ctx);
        super::theme::install_style(ctx);
        let snap = Some(Arc::new(backend.st.snapshot()));
        let term = terminal.then(|| TermSession::start(backend.pi.clone() as Arc<dyn terminal::PtySource>, backend.events.clone()));
        let drafts = match dirs::data_dir() {
            Some(d) if terminal => PlanDrafts::with_file(d.join("Spide").join("proposal-drafts.json")),
            _ => PlanDrafts::default(),
        };
        Self {
            ctx: ctx.clone(),
            backend,
            rx,
            snap,
            measure: EguiMeasure::new(ctx),
            tab: Tab::Class,
            toasts: vec![],
            toast_seq: 0,
            hover: HoverStore::default(),
            card_selected: None,
            health: HealthState::default(),
            workspace: WorkspaceSettings::default(),
            drawer: false,
            dragging_sep: None,
            term,
            nvim_term: None,
            trace_ui: Default::default(),
            editor_pane: false,
            term_confirm: false,
            restarting: false,
            pane_palette: None,
            palette: None,
            prune: None,
            inline: None,
            brief: false,
            brief_ui: BriefUi::default(),
            compare: None,
            compare_ui: CompareUi::default(),
            branch_cmp: None,
            stress: None,
            review_active: false,
            selected_proposal: None,
            proposal_canvas: None,
            proposal_ui: ProposalUi { drafts, ..Default::default() },
            shown_decision: None,
            history_open: false,
            project_open: false,
            project_ui: Default::default(),
            review_locate: None,
            review_seen: HashSet::new(),
            review_pos: Default::default(),
            timeline: false,
            timeline_ui: TimelineUi::default(),
            preview: None,
            preview_source: PreviewSource::Option,
            preview_gen: 0,
            scenario: None,
            show_implicit: false,
            lens: Lens::None,
            decision_ui: DecisionUi::default(),
            class_canvas: None,
            package_scope: None,
            full_class_canvas: None,
            domain_canvas: None,
            export_canvases: HashMap::new(),
            views: Views::default(),
            class_data: None,
            domain_data: None,
            spotlight: None,
            export: None,
            export_settings: HashMap::new(),
            seen_commands: VecDeque::new(),
            deferred: vec![],
            canvas_rect: Rect::NOTHING,
            last_tab: Tab::Class,
            last_rev: None,
            exit_requested: false,
            leave_answers: None,
            leave_copy_path: None,
            close_confirmed: false,
            prefs: Default::default(),
            prefs_path: None,
            new_project: None,
            open_error: None,
            last_title: String::new(),
            find: None,
            nav: Default::default(),
            selection: vec![],
            show_shortcuts: false,
            settings_open: false,
            branches: None,
            return_focus: None,
            context: None,
            class_editor: None,
            new_element: None,
            app_clipboard: None,
            pending_cameras: Default::default(),
            last_workspace_save: 0.0,
            input_drafts: Default::default(),
            draft_notice: None,
            recovered_feedback: None,
            last_draft_save: 0.0,
            activity: Default::default(),
            activity_open: false,
            activity_errors_only: false,
            last_operation_status: String::new(),
            integrations: Default::default(),
            documentation_open: false,
            documentation_query: String::new(),
            about_open: false,
            open_request_pending: false,
            copying_image: false,
        }
    }

    // ------------------------------------------------------------------ small helpers

    pub fn toast(&mut self, kind: &str, text: impl Into<String>) {
        let text = text.into();
        self.record_activity(kind, text.clone(), None);
        self.toast_seq += 1;
        let until = self.now() + if kind == "error" { 9.0 } else { 5.0 };
        self.toasts.push(Toast { id: self.toast_seq, kind: kind.to_string(), text, until });
        if self.toasts.len() > 4 {
            self.toasts.remove(0);
        }
    }

    pub(super) fn now(&self) -> f64 {
        self.ctx.input(|i| i.time)
    }

    /// Run `f` after `secs` (the old `setTimeout`).
    pub(super) fn later(&mut self, secs: f64, f: impl FnOnce(&mut SpideApp) + 'static) {
        let at = self.now() + secs;
        self.deferred.push((at, Box::new(f)));
    }

    pub fn decisions_done(&mut self, err: Option<String>) {
        self.decision_ui.sending = false;
        if let Some(e) = err {
            self.decision_ui.error = e.clone();
            self.toast("error", e);
        }
    }

    pub fn close_timeline(&mut self) {
        self.timeline = false;
        if self.preview_source == PreviewSource::History {
            self.set_preview(None, PreviewSource::History);
        }
    }

    pub fn refresh_proposal_canvas(&mut self) {
        let plan = self.proposal_ui.session.as_ref().filter(|s| s.scope == self.proposal_ui.scope).map(|s| s.plan.clone());
        self.proposal_canvas = if self.review_active { self.proposal_ui.canvas(plan.as_ref()) } else { None };
        self.class_data = None;
    }

    fn set_preview(&mut self, d: Option<Design>, src: PreviewSource) {
        self.preview_source = src;
        self.preview = d.map(Arc::new);
        self.preview_gen += 1;
        self.class_data = None;
    }

    fn run(&self, f: impl FnOnce(&crate::state::AppState, &crate::pi_bridge::PiBridge) -> Result<(), String> + Send + 'static) {
        self.backend.run(f);
    }

    fn run_transition(&self, f: impl FnOnce(&crate::state::AppState, &crate::pi_bridge::PiBridge) -> Result<(), String> + Send + 'static) {
        self.backend.transition(f, |app, r| match r {
            Ok(()) => app.after_transition(),
            Err(e) => app.transition_failed(e),
        });
    }

    fn transition_failed(&mut self, e: String) {
        if self.backend.st.lock().workspace_status == crate::state::WorkspaceStatus::Closed {
            self.open_error = Some(e.clone());
        }
        self.toast("error", e);
    }

    /// A project was opened or created: fresh UI, remember it.
    pub fn after_transition(&mut self) {
        self.reset_workspace_ui();
        self.restore_workspace();
        self.restore_input_drafts();
        self.open_error = None;
        self.record_recent();
    }

    /// The open project (if any) goes to the top of Recent Projects.
    fn record_recent(&mut self) {
        let (open, root, name) = {
            let s = self.backend.st.lock();
            (matches!(s.workspace_status, crate::state::WorkspaceStatus::Ready | crate::state::WorkspaceStatus::ReadOnly), s.project_dir.clone(), s.store.design.name.clone())
        };
        if open {
            self.prefs.opened(&root, &name, spide_core::store::now_ms());
        }
        self.save_prefs();
    }

    pub fn toggle_favorite_command(&mut self, command: String) {
        if self.prefs.favorite_commands.contains(&command) {
            self.prefs.favorite_commands.retain(|c| c != &command);
        } else {
            self.prefs.favorite_commands.push(command);
        }
        self.save_prefs();
    }

    pub fn save_prefs(&mut self) {
        let items: Vec<(String, String, bool)> = self.prefs.sorted().into_iter().map(|r| { let ok = r.available(); (format!("{}{} — {}", if r.pinned { "📌 " } else { "" }, r.name, r.path), r.path, ok) }).collect();
        crate::menu::set_recent(&items);
        if let Some(p) = self.prefs_path.clone() {
            if let Err(e) = self.prefs.save(&p) {
                self.toast("error", e);
            }
        }
    }

    pub(super) fn workspace_open(&self) -> bool {
        self.snap.as_ref().is_some_and(|s| matches!(s.workspace_status, crate::state::WorkspaceStatus::Ready | crate::state::WorkspaceStatus::ReadOnly))
    }

    pub fn new_project_dialog(&mut self) {
        let mut form = super::welcome::NewProject::new();
        form.location = self.prefs.new_project_location.clone();
        self.new_project = Some(form);
    }

    pub fn create_new_project(&mut self, f: super::welcome::NewProject) {
        match f.kind {
            super::welcome::NewKind::Design => {
                if !self.confirm_leave(super::leave::LeaveReason::SwitchProject) {
                    return;
                }
                let (dir, name, template) = (f.target().display().to_string(), f.name.trim().to_string(), f.template);
                self.run_transition(move |s, pi| crate::commands::new_project_template(s, pi, dir, name, template));
            }
            super::welcome::NewKind::Code => {
                if self.workspace_open() {
                    self.open_project_rail();
                }
                self.project_out(super::project::ProjectOut::CreateIn);
            }
        }
    }

    pub fn close_project(&mut self) {
        if !self.workspace_open() && !matches!(self.snap.as_ref().map(|s| &s.workspace_status), Some(crate::state::WorkspaceStatus::Failed(_))) {
            return;
        }
        if !self.confirm_leave(super::leave::LeaveReason::CloseProject) {
            return;
        }
        self.backend.transition(crate::commands::close_project, |app, r| match r {
            Ok(()) => {
                app.reset_workspace_ui();
                // a manual close is honoured at the next start
                app.prefs.last_project = None;
                app.save_prefs();
            }
            Err(e) => app.toast("error", e),
        });
    }

    pub fn save_now(&mut self) {
        if !self.workspace_open() {
            return;
        }
        self.run_ok(|s, _| s.persist_and_emit(), |app| app.toast("info", "Saved"));
    }

    pub fn save_copy_dialog(&mut self) {
        if !self.workspace_open() {
            return self.toast("error", "No project is open.");
        }
        let name = self.snap.as_ref().map(|s| s.view.name.clone());
        if let Some(path) = rfd::FileDialog::new().set_title("Save a copy of the design (the project stays where it is)").set_file_name(format!("{}.json", slug(name.as_deref()))).add_filter("Spide design", &["json"]).save_file() {
            let path = path.display().to_string();
            let msg = format!("Design copy saved to {path}");
            self.run_ok(move |s, _| crate::commands::save_as(s, path), move |app| app.toast("info", msg));
        }
    }

    pub fn open_recent(&mut self, path: String) {
        let r = crate::preferences::Recent { path: path.clone(), ..Default::default() };
        if !r.available() {
            return self.toast("error", format!("{path} is missing or moved. Use Locate… on the welcome screen, or remove it."));
        }
        if !self.confirm_leave(super::leave::LeaveReason::SwitchProject) {
            return;
        }
        self.run_transition(move |s, pi| crate::commands::open_folder(s, pi, path));
    }

    /// A recent project moved: open it from its new place and forget the old entry.
    fn locate_recent(&mut self, old: String) {
        let Some(dir) = rfd::FileDialog::new().set_title("Where is the project now?").pick_folder() else { return };
        if !self.confirm_leave(super::leave::LeaveReason::SwitchProject) {
            return;
        }
        let d = dir.display().to_string();
        self.backend.transition(move |s, pi| crate::commands::open_folder(s, pi, d), move |app, r| match r {
            Ok(()) => {
                app.prefs.remove(&old);
                app.after_transition();
            }
            Err(e) => app.transition_failed(e),
        });
    }

    /// Import Design JSON: a workspace's own design file opens that workspace; any other file is
    /// copied into a NEW folder (nothing is replaced).
    pub fn import_dialog(&mut self) {
        let Some(file) = rfd::FileDialog::new().set_title("Import a Spide design (JSON)").add_filter("Spide design", &["json"]).pick_file() else { return };
        self.import_file_dialog(file);
    }

    pub fn import_file_dialog(&mut self, file: std::path::PathBuf) {
        let text = match std::fs::read_to_string(&file) {
            Ok(t) => t,
            Err(e) => return self.toast("error", format!("{}: {e}", file.display())),
        };
        let design = match crate::workspace::parse_design(&text, &file) {
            Ok(d) => d,
            Err(e) => return self.toast("error", e),
        };
        if file.file_name().is_some_and(|n| n == "design.json") {
            let dir = file.parent().map(std::path::Path::to_path_buf).unwrap_or_default();
            let root = spide_core::project::resolve_root(&dir.join("project.json"));
            let root = if spide_core::project::is_project(&root) && root.join(spide_core::project::DESIGN) == file { root } else { dir };
            if crate::workspace::prepare(&root, crate::workspace::LoadIntent::Open).is_ok() {
                if !self.confirm_leave(super::leave::LeaveReason::SwitchProject) {
                    return;
                }
                let d = root.display().to_string();
                return self.run_transition(move |s, pi| crate::commands::open_folder(s, pi, d));
            }
        }
        let Some(dest) = rfd::FileDialog::new().set_title("New, empty folder for the imported project").pick_folder() else { return };
        if let Err(e) = crate::commands::ensure_empty(&dest) {
            return self.toast("error", e);
        }
        let ok = rfd::MessageDialog::new()
            .set_title("Import Design")
            .set_description(format!("Import “{}” ({} classes) from
{}
into the new project
{}?

The original file is not changed.", design.name, design.live_classes().count(), file.display(), dest.display()))
            .set_buttons(rfd::MessageButtons::OkCancelCustom("Import".into(), "Cancel".into()))
            .show();
        if !matches!(ok, rfd::MessageDialogResult::Ok) && ok != rfd::MessageDialogResult::Custom("Import".into()) {
            return;
        }
        if !self.confirm_leave(super::leave::LeaveReason::SwitchProject) {
            return;
        }
        let (f, d) = (file.display().to_string(), dest.display().to_string());
        self.run_transition(move |s, pi| crate::commands::import_design(s, pi, f, d));
    }

    fn reveal_project(&mut self) {
        if self.workspace_open() {
            self.run(|s, _| crate::commands::reveal_project(s));
        }
    }

    fn welcome_out(&mut self, o: super::welcome::WelcomeOut) {
        use super::welcome::WelcomeOut;
        match o {
            WelcomeOut::New => self.new_project_dialog(),
            WelcomeOut::Open => self.open_project_dialog(),
            WelcomeOut::Import => self.import_dialog(),
            WelcomeOut::OpenRecent(p) => self.open_recent(p),
            WelcomeOut::Locate(p) => self.locate_recent(p),
            WelcomeOut::Remove(p) => {
                self.prefs.remove(&p);
                self.save_prefs();
            }
            WelcomeOut::TogglePin(p) => {
                self.prefs.toggle_pin(&p);
                self.save_prefs();
            }
            WelcomeOut::ClearUnpinned => {
                self.prefs.clear_unpinned();
                self.save_prefs();
            }
            WelcomeOut::ReopenLast(on) => {
                self.prefs.reopen_last = on;
                self.save_prefs();
            }
        }
    }

    fn welcome_layout(&mut self, ui: &mut egui::Ui, ws: Rect, snap: &Arc<Snapshot>) {
        ui.painter().rect_filled(ws, 0.0, css::BG);
        self.canvas_rect = ws;
        let mut outs = vec![];
        let mut project_outs = vec![];
        ui.scope_builder(egui::UiBuilder::new().max_rect(ws.shrink(16.0)), |ui| {
            if self.project_ui.form.is_some() {
                // creating a Spide Project from the welcome screen: the manifest form
                ui.vertical_centered(|ui| {
                    ui.set_max_width(560.0);
                    project_outs = super::project::show(ui, &mut self.project_ui, snap, &self.backend);
                });
            } else {
                outs = super::welcome::show(ui, &self.prefs, self.open_error.as_deref());
            }
        });
        for o in project_outs {
            self.project_out(o);
        }
        for o in outs {
            self.welcome_out(o);
        }
    }

    /// The macOS window title: project, branch and save state (shown in Mission Control and Window menu).
    fn update_title(&mut self, snap: &Snapshot) {
        use crate::state::WorkspaceStatus as W;
        let title = match &snap.workspace_status {
            W::Closed | W::Loading => "Spide".to_string(),
            W::Failed(_) => "Spide — project could not be opened".to_string(),
            st => {
                let state = if *st == W::ReadOnly { " — read-only" } else if snap.save_status.error.is_some() { " — not saved" } else { "" };
                format!("{} ({}){state} — Spide", snap.view.name, snap.branch)
            }
        };
        if title != self.last_title {
            self.ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.last_title = title;
        }
    }

    /// After the workspace changed: nothing transient from the previous project survives.
    pub fn reset_workspace_ui(&mut self) {
        self.review_active = false;
        self.selected_proposal = None;
        self.proposal_canvas = None;
        self.proposal_ui.session = None;
        self.proposal_ui.notes.clear();
        self.decision_ui.drafts = Default::default();
        self.brief_ui = Default::default();
        self.draft_notice = None;
        self.recovered_feedback = None;
        self.proposal_ui.editing = None;
        self.proposal_ui.review = None;
        self.shown_decision = None;
        self.history_open = false;
        self.timeline = false;
        self.set_preview(None, PreviewSource::Option);
        self.compare = None;
        self.branch_cmp = None;
        self.stress = None;
        self.scenario = None;
        self.spotlight = None;
        self.prune = None;
        self.inline = None;
        self.palette = None;
        self.hover.clear();
        self.card_selected = None;
        self.brief = false;
        self.export = None;
        self.project_ui.form = None;
        self.full_class_canvas = None;
        self.package_scope = None;
        self.class_canvas = None;
        self.domain_canvas = None;
        self.export_canvases.clear();
        self.class_data = None;
        self.domain_data = None;
        self.views = Views::default();
        self.find = None;
        self.nav.clear();
        self.selection.clear();
        self.context = None;
        self.class_editor = None;
        self.new_element = None;
        self.integrations = Default::default();
        self.copying_image = false;
    }

    pub(super) fn run_ok(&self, f: impl FnOnce(&crate::state::AppState, &crate::pi_bridge::PiBridge) -> Result<(), String> + Send + 'static, ok: impl FnOnce(&mut SpideApp) + Send + 'static) {
        self.backend.run_then(f, move |app, ()| ok(app));
    }

    fn leave_proposal_review(&self) -> bool {
        !self.proposal_ui.unsaved()
            || rfd::MessageDialog::new().set_title("Spide").set_description("Leave proposal review? Your draft will be kept locally; unfinished operation form edits may be lost.").set_buttons(rfd::MessageButtons::OkCancel).show() == rfd::MessageDialogResult::Ok
    }

    pub fn open_proposal_review(&mut self, id: Option<String>) {
        if let Some(id) = &id {
            if Some(id) != self.selected_proposal.as_ref() && !self.leave_proposal_review() {
                return;
            }
            self.selected_proposal = Some(id.clone());
        }
        if !self.review_active || id.is_some() {
            self.review_locate = Some((id.clone().or(self.selected_proposal.clone()).unwrap_or_default(), self.now()));
        }
        self.review_active = true;
        self.history_open = false;
        self.project_open = false;
        self.shown_decision = None;
        self.set_preview(None, PreviewSource::Option);
        self.compare = None;
        self.branch_cmp = None;
        self.timeline = false;
        self.brief = false;
        self.inline = None;
        self.prune = None;
        // the inspector opens
        self.drawer = true;
        self.workspace.right_open = true;
        self.class_data = None;
    }

    /// A plan was approved or dismissed: show the next one (never approve it), or the completion state.
    pub fn review_resolved(&mut self, accepted: bool) {
        self.toast("info", if accepted { "Approved — ⌘Z undoes it" } else { "Dismissed — ⌘Z undoes it" });
        self.review_locate = self.selected_proposal.clone().map(|id| (id, self.now()));
    }

    /// A diagram element clicked during review: switch to the plan that adds it, else inspect it and
    /// toggle it as a target of a change request.
    fn review_pick(&mut self, t: &Target) {
        let key = match t {
            Target::Class { name } => format!("class:{name}"),
            Target::Relation { id } => format!("relation:{id}"),
            Target::Attribute { class, name } => format!("attribute:{class}.{name}"),
            Target::Method { class, name } => format!("method:{class}.{name}"),
            Target::Package { name } => format!("package:{name}"),
            Target::Proposal { id } => {
                if Some(id) != self.selected_proposal.as_ref() {
                    self.open_proposal_review(Some(id.clone()));
                }
                return;
            }
            _ => return,
        };
        // an element of an earlier pending plan: review that plan instead
        if let Some(d) = self.proposal_canvas.as_ref() {
            if let Some(pid) = spide_diagram::focus::proposal_at(Some(&key), &d.classes, &d.relations) {
                if Some(&pid) != self.selected_proposal.as_ref() && d.proposals.iter().any(|p| p.id == pid) {
                    return self.open_proposal_review(Some(pid));
                }
            }
        }
        self.proposal_ui.pick(&key);
        self.workspace.right_open = true;
        self.drawer = true;
    }

    fn calm(&mut self) {
        self.hover.clear();
        self.spotlight = None;
        if let Some(c) = self.class_canvas.as_mut() {
            c.pinned_focus = None;
            c.focus_set = None;
        }
        self.set_preview(None, PreviewSource::Option);
    }

    /// Select/focus `Class` or `Class.method` (Neovim's :SpideReveal).
    fn reveal(&mut self, symbol: &str) {
        let Some(snap) = self.snap.clone() else { return };
        let (class, method) = match symbol.split_once('.') {
            Some((c, m)) if snap.view.class(c).is_some() => (c.to_string(), Some(m.to_string())),
            _ => (symbol.to_string(), None),
        };
        if snap.view.class(&class).is_none() {
            self.toast("info", format!("{class} isn't in the design yet"));
            return;
        }
        self.tab = Tab::Class;
        self.focus_canvas(Some(&class));
        match method.filter(|m| snap.view.class(&class).is_some_and(|c| c.methods.iter().any(|x| &x.name == m))) {
            Some(m) => self.hover.pin(&format!("method:{class}.{m}")),
            None => self.hover.clear(),
        }
        self.ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }

    pub fn show_editor(&mut self) {
        self.editor_pane = true;
        self.workspace.left_open = true;
        if self.nvim_term.is_none() {
            self.nvim_term = Some(TermSession::start(crate::editor::embedded() as Arc<dyn terminal::PtySource>, self.backend.events.clone()));
        }
        if let Some(t) = self.nvim_term.as_mut() {
            t.focus_requested = true;
        }
    }

    /// ⌘-click: open a class/method in Neovim.
    pub(super) fn open_in_editor(&mut self, class: String, method: Option<String>) {
        self.backend.call(move |st, _| crate::editor::open_element(st, &class, method.as_deref()), |app, r| match r {
            Ok(v) => {
                if v["kind"] == "embedded" {
                    app.show_editor();
                }
            }
            Err(e) => app.toast("error", e),
        });
    }

    pub(super) fn focus_canvas(&mut self, name: Option<&str>) -> bool {
        let now = self.now();
        self.class_canvas.as_mut().is_some_and(|c| c.focus(name, now))
    }

    // ------------------------------------------------------------------ events

    fn drain(&mut self, ctx: &egui::Context) {
        while let Ok(e) = self.rx.try_recv() {
            match e {
                UiEvent::OpenPaths(paths) => self.handle_open_paths(paths),
                UiEvent::RaiseWindow => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                UiEvent::Snapshot(s) => {
                    // ignore snapshots of a previous workspace, and stale ones of this workspace
                    let keep_old = self.snap.as_ref().is_some_and(|p| p.epoch > s.epoch || (p.epoch == s.epoch && p.rev > s.rev && p.pi_status == s.pi_status));
                    if !keep_old {
                        let branch_changed = self.snap.as_ref().is_some_and(|p| p.epoch == s.epoch && p.branch != s.branch);
                        if branch_changed {
                            let _ = self.save_input_drafts();
                            self.discard_input_drafts();
                        }
                        self.snap = Some(s);
                        if branch_changed { self.restore_input_drafts(); }
                        self.class_data = None;
                        self.domain_data = None;
                    }
                }
                UiEvent::Toast { kind, text } => self.toast(&kind, text),
                UiEvent::Task(f) => f(self),
                UiEvent::Menu(MenuAction::Run(id)) => self.perform(id),
                UiEvent::Menu(MenuAction::OpenRecent(p)) => self.open_recent(p),
                UiEvent::Menu(MenuAction::ClearRecent) => {
                    self.prefs.clear_unpinned();
                    self.save_prefs();
                }
                UiEvent::Menu(MenuAction::Quit) => {
                    if self.confirm_leave(super::leave::LeaveReason::Quit) {
                        self.close_confirmed = true;
                        self.ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                }
                UiEvent::SpideCommand { id, generation, project, command } => {
                    if self.snap.as_ref().map(|s| s.project_dir.as_str()) != Some(project.as_str()) {
                        continue;
                    }
                    let key = format!("{generation}:{id}");
                    if self.seen_commands.contains(&key) {
                        continue;
                    }
                    self.seen_commands.push_back(key);
                    if self.seen_commands.len() > 256 {
                        self.seen_commands.pop_front();
                    }
                    let text = if command.starts_with('/') { command } else { format!("/{command}") };
                    self.command(&text);
                }
                UiEvent::TerminalOutput => ctx.request_repaint(),
                UiEvent::Reveal { symbol } => self.reveal(&symbol),
                UiEvent::ShowEditor => self.show_editor(),
                UiEvent::Command(c) => self.command(&c),
                UiEvent::ChangeRequest { text, context } => {
                    let place = match (context["file"].as_str(), context["line"].as_u64()) {
                        (Some(f), Some(l)) => format!(" (at {f}:{l})"),
                        (Some(f), None) => format!(" (in {f})"),
                        _ => String::new(),
                    };
                    let msg = format!("Change request from Neovim{place}: {text}\nPropose the design change through Spide's tools; for code, read it with get_source and stage edits with propose_patch.");
                    let label = format!("✎ {text}");
                    self.run(move |st, pi| pi.prompt(pi.ticket(), st, msg, Some(label)));
                    self.toast("info", "Change request sent to the agent");
                }
            }
        }
    }

    // ------------------------------------------------------------------ data

    /// Change only the class view restriction; do not change the design or full-project health.
    pub(super) fn set_package_scope(&mut self, scope: Option<String>) {
        if scope == self.package_scope { self.tab = Tab::Class; return; }
        if self.package_scope.is_none() { self.full_class_canvas = self.class_canvas.take(); }
        else { self.class_canvas = None; }
        self.package_scope = scope;
        self.class_data = None;
        if self.package_scope.is_none() { self.class_canvas = self.full_class_canvas.take(); }
        self.selection.clear();
        self.hover.clear();
        self.spotlight = None;
        self.tab = Tab::Class;
    }

    fn class_data(&mut self) -> Option<Arc<ClassData>> {
        let snap = self.snap.clone()?;
        let phantoms = if self.lens == Lens::Sync { snap.project.as_ref().map(|p| p.phantoms.iter().map(|c| c.name.clone()).collect::<Vec<_>>()) } else { None };
        if self.package_scope.as_ref().is_some_and(|p| !snap.view.packages.iter().any(|pk| &pk.name == p && pk.status.is_live())) {
            self.set_package_scope(None);
        }
        let key = hash_of((snap.rev, &snap.project_dir, self.preview_gen, self.review_active, self.proposal_canvas.as_ref().map(|d| d.rev), phantoms, &self.package_scope));
        if let Some(d) = self.class_data.as_ref().filter(|d| d.key == key) {
            return Some(d.clone());
        }
        let view = &snap.view;
        let d: &Design = if self.review_active { self.proposal_canvas.as_ref().unwrap_or(view) } else { self.preview.as_deref().unwrap_or(view) };
        let mut cd = ClassData { key, classes: d.classes.clone(), relations: d.relations.clone(), packages: d.packages.clone(), implicit: vec![], scoped_metrics: None, saved: self.preview.as_deref().unwrap_or(view).layout.get("class").cloned() };
        // sync lens: code-only classes join the canvas as "only in code" ghosts
        if let (Lens::Sync, None, false, Some(p)) = (self.lens, &self.preview, self.review_active, snap.project.as_ref()) {
            cd.classes.extend(p.phantoms.iter().cloned());
            cd.relations.extend(p.phantom_relations.iter().cloned());
        }
        if self.preview.is_none() && !self.review_active {
            cd.implicit = snap.metrics.implicit_edges.clone();
        }
        if let Some(package) = &self.package_scope {
            // Resolve after sync ghosts join the model, then project both graph and scene data.
            let mut model = d.clone();
            model.classes = cd.classes.clone();
            model.relations = cd.relations.clone();
            let scope = spide_core::scope::PackageScope::new(&model, &snap.metrics, package, true).ok()?;
            let projected = scope.design(&model);
            cd.classes = projected.classes;
            cd.relations = projected.relations;
            cd.packages = projected.packages;
            cd.implicit.retain(|e| scope.includes_edge(&e.from, &e.to));
            cd.scoped_metrics = Some(scope.metrics(&snap.metrics, &model));
            cd.saved = None;
        }
        let a = Arc::new(cd);
        self.class_data = Some(a.clone());
        Some(a)
    }

    fn domain_data(&mut self) -> Option<Arc<ClassData>> {
        let snap = self.snap.clone()?;
        let key = hash_of((snap.rev, &snap.project_dir, "domain"));
        if let Some(d) = self.domain_data.as_ref().filter(|d| d.key == key) {
            return Some(d.clone());
        }
        // the agent-built conceptual model (no link to the design's classes)
        let (classes, relations) = snap.view.domain.as_class_model();
        let saved = snap.view.layout.get("domain").cloned();
        let a = Arc::new(ClassData { key, classes, relations, packages: vec![], implicit: vec![], scoped_metrics: None, saved });
        self.domain_data = Some(a.clone());
        Some(a)
    }

    /// Sync lens: class rings, member dots and code-only ghosts from the project's report.
    fn sync_sets(&self) -> Option<SyncSets> {
        if self.lens != Lens::Sync {
            return None;
        }
        let p = self.snap.as_ref()?.project.as_ref()?;
        let r = p.report.as_ref()?;
        let mut out = SyncSets::default();
        for (name, c) in &r.classes {
            match c.status {
                Some(SyncStatus::Conflict) => out.conflict.insert(name.clone()),
                Some(SyncStatus::DesignAhead) => out.design_ahead.insert(name.clone()),
                Some(SyncStatus::CodeAhead) => out.code_ahead.insert(name.clone()),
                Some(SyncStatus::Unreconciled) => out.unreconciled.insert(name.clone()),
                _ => false,
            };
        }
        out.phantom = p.phantoms.iter().map(|c| c.name.clone()).collect();
        for it in &r.items {
            let (Some(name), "field" | "method") = (&it.design_name, it.kind.as_str()) else { continue };
            let color = match it.status {
                SyncStatus::Conflict => spide_diagram::class_view::SYNC_CONFLICT,
                SyncStatus::DesignAhead => spide_diagram::class_view::SYNC_DESIGN_AHEAD,
                SyncStatus::CodeAhead => spide_diagram::class_view::SYNC_CODE_AHEAD,
                SyncStatus::Unreconciled => spide_diagram::class_view::SYNC_UNRECONCILED,
                _ => continue,
            };
            let row = format!("{}:{name}", if it.kind == "field" { "attribute" } else { "method" });
            out.rows.entry(it.class.clone()).or_default().insert(row, color);
        }
        Some(out)
    }

    fn stress_sets(&self) -> Option<Stress> {
        let snap = self.snap.as_ref()?;
        let c = snap.metrics.changes.iter().find(|x| Some(&x.name) == self.stress.as_ref())?;
        Some(Stress { impacted: c.impacted.iter().cloned().collect(), ripple: c.ripple.iter().cloned().collect() })
    }

    // ------------------------------------------------------------------ actions

    pub(super) fn do_prune(&mut self, targets: Vec<Target>, at: Pos2) {
        if self.review_active {
            return self.toast("info", "Use the proposal editor to plan deletions; canvas editing is disabled during review.");
        }
        if let Some(id) = targets.iter().find_map(|t| if let Target::Proposal { id } = t { Some(id.clone()) } else { None }) {
            return self.open_proposal_review(Some(id));
        }
        // code-only ghosts (sync lens) aren't part of the design
        let phantom: HashSet<String> = self.snap.as_ref().and_then(|s| s.project.as_ref()).map(|p| p.phantoms.iter().map(|c| c.name.clone()).collect()).unwrap_or_default();
        let rest: Vec<Target> = targets.iter().filter(|t| !matches!(t, Target::Proposal { .. }) && !matches!(t, Target::Class { name } if phantom.contains(name))).cloned().collect();
        let all = targets.clone();
        self.hover.clear();
        self.backend.call(
            move |s, _| if rest.is_empty() { Ok(()) } else { crate::commands::prune(s, rest) },
            move |app, r| match r {
                Ok(()) => app.prune = Some(PruneState { targets: all, at, text: String::new(), focused: false }),
                Err(e) => app.toast("error", e),
            },
        );
    }

    pub(super) fn on_click_target(&mut self, t: Target) {
        let Some(snap) = self.snap.clone() else { return };
        if self.ctx.input(|i| i.modifiers.command) && !self.review_active {
            match &t {
                Target::Class { name } => return self.open_in_editor(name.clone(), None),
                Target::Method { class, name } => return self.open_in_editor(class.clone(), Some(name.clone())),
                _ => {}
            }
        }
        let view = &snap.view;
        if self.review_active {
            return self.review_pick(&t);
        }
        if let Target::Proposal { id } = &t {
            return self.open_proposal_review(Some(id.clone()));
        }
        if self.ctx.input(|i| i.modifiers.shift) {
            // ⇧-click adds to / removes from the selection
            if let Some(n) = self.selection.iter().position(|x| x == &t) {
                self.selection.remove(n);
            } else {
                self.selection.push(t.clone());
            }
        } else {
            self.selection = vec![t.clone()];
        }
        let key = match &t {
            Target::Class { name } => Some(format!("class:{name}")),
            Target::Relation { id } => Some(format!("relation:{id}")),
            Target::Attribute { class, name } => Some(format!("attribute:{class}.{name}")),
            Target::Method { class, name } => Some(format!("method:{class}.{name}")),
            _ => None,
        };
        if let Some(plan) = spide_diagram::focus::proposal_at(key.as_deref(), &view.classes, &view.relations) {
            return self.open_proposal_review(Some(plan));
        }
        if self.review_active {
            return;
        }
        if is_pruned(view, &t) {
            return self.run(move |s, _| crate::commands::restore(s, t));
        }
        // clicking a member pins its card: evidence, similar members and findings can then be explored
        if let Target::Attribute { class, name } | Target::Method { class, name } = &t {
            let k = if matches!(t, Target::Attribute { .. }) { "attribute" } else { "method" };
            self.hover.pin(&format!("{k}:{class}.{name}"));
        }
        if let Some(origin) = origin_of(view, &t) {
            if view.decisions.iter().any(|d| d.id == origin && !d.is_pending()) {
                self.shown_decision = Some(origin);
                return;
            }
        }
        if let Target::UseCase { name } = &t {
            self.scenario = Some(name.clone());
            self.tab = Tab::Sequence;
        }
        // a `ref` frame opens the sequence it refers to
        if let Target::Message { sequence, index } = &t {
            if let Some(m) = view.sequences.iter().find(|s| &s.name == sequence).and_then(|s| s.messages.get(*index)).filter(|m| m.kind == spide_core::model::MessageKind::Ref) {
                if view.sequences.iter().any(|s| s.name == m.method && s.status != Status::Pruned) {
                    self.scenario = Some(m.method.clone());
                } else {
                    self.toast("info", format!("No sequence “{}” yet — ask pi for it", m.method));
                }
            }
        }
    }

    pub(super) fn on_double_click(&mut self, t: Target, at: Pos2) {
        if self.review_active {
            return;
        }
        let Some(snap) = self.snap.clone() else { return };
        let cls = |n: &str| snap.view.classes.iter().find(|c| c.name == n && c.status != Status::Pruned && c.status != Status::Proposed);
        let edit = match &t {
            Target::Class { name } if cls(name).is_some() => Some(InlineEdit { at, label: "rename class".into(), original: name.clone(), value: name.clone(), focused: false, kind: InlineKind::RenameClass(name.clone()) }),
            Target::Attribute { class, name } if cls(class).is_some_and(|c| c.attributes.iter().any(|a| &a.name == name)) => {
                Some(InlineEdit { at, label: format!("rename {class}.{name}"), original: name.clone(), value: name.clone(), focused: false, kind: InlineKind::RenameAttribute(class.clone(), name.clone()) })
            }
            Target::Method { class, name } if cls(class).is_some_and(|c| c.methods.iter().any(|a| &a.name == name)) => {
                Some(InlineEdit { at, label: format!("rename {class}.{name}()"), original: name.clone(), value: name.clone(), focused: false, kind: InlineKind::RenameMethod(class.clone(), name.clone()) })
            }
            _ => None,
        };
        if edit.is_some() {
            self.inline = edit;
        }
        self.hover.clear();
    }

    fn submit_inline(&mut self, kind: InlineKind, v: String) {
        let Some(snap) = self.snap.clone() else { return };
        let o = match kind {
            InlineKind::RenameClass(from) => json!({"op": "rename_class", "from": from, "to": v}),
            InlineKind::RenameAttribute(class, name) => {
                let Some(a) = snap.view.classes.iter().find(|c| c.name == class).and_then(|c| c.attributes.iter().find(|a| a.name == name)) else { return };
                let mut a = serde_json::to_value(a).unwrap();
                a["name"] = json!(v);
                json!({"op": "update_attribute", "class": class, "name": name, "attribute": a})
            }
            InlineKind::RenameMethod(class, name) => {
                let Some(m) = snap.view.classes.iter().find(|c| c.name == class).and_then(|c| c.methods.iter().find(|a| a.name == name)) else { return };
                let mut m = serde_json::to_value(m).unwrap();
                m["name"] = json!(v);
                json!({"op": "update_method", "class": class, "name": name, "method": m})
            }
        };
        let op = brief::op(o);
        self.run(move |s, _| crate::commands::apply_user_ops(s, vec![op]));
    }

    fn focus_issue(&mut self, idx: usize) {
        let Some(snap) = self.snap.clone() else { return };
        let Some(i) = snap.metrics.issues.get(idx) else { return };
        let Some(t) = i.targets.first().cloned() else { return };
        let view = &snap.view;
        let cls = view.classes.iter().find(|c| c.name == t).map(|c| c.name.clone()).or_else(|| t.contains("->").then(|| t.split("->").next().unwrap().to_string()));
        let go = move |app: &mut SpideApp| {
            let snap = app.snap.clone().unwrap();
            let view = &snap.view;
            if let Some(c) = cls.as_ref().filter(|c| view.classes.iter().any(|x| &x.name == *c)) {
                app.focus_canvas(Some(c));
            } else if view.packages.iter().any(|p| p.name == t) {
                let now = app.now();
                if let Some(cv) = app.class_canvas.as_mut() {
                    cv.focus_package(&view.classes, Some(&snap.metrics), &t, now);
                }
            } else if let Some(s) = t.strip_prefix("sequence:") {
                app.scenario = Some(s.to_string());
                app.tab = Tab::Sequence;
            } else if view.containers.iter().any(|k| k.name == t) {
                app.tab = Tab::System;
            }
        };
        if self.tab != Tab::Class && !i.targets[0].starts_with("sequence:") {
            self.tab = Tab::Class;
            self.later(0.3, go);
        } else {
            go(self);
        }
    }

    fn fix_issue(&mut self, idx: usize) {
        let Some(snap) = self.snap.clone() else { return };
        let Some(i) = snap.metrics.issues.get(idx) else { return };
        let (message, rule, fix, kind, targets) = (i.message.clone(), i.rule.clone(), i.fix_hint.clone(), i.kind.to_string(), i.targets.clone());
        self.run(move |s, pi| crate::commands::fix_issue(s, pi, message, rule, fix, Some(kind), Some(targets)));
    }

    // ------------------------------------------------------------------ export

    fn legend_for(&self, tab: Tab) -> Vec<LegendKey> {
        let Some(snap) = &self.snap else { return vec![] };
        let (view, metrics) = (&snap.view, &snap.metrics);
        let mut keys: HashSet<LegendKey> = HashSet::new();
        let rk = |r: &Relation| -> LegendKey {
            use spide_core::model::RelationKind::*;
            match r.kind {
                Association => {
                    if r.directed {
                        LegendKey::Directed
                    } else {
                        LegendKey::Association
                    }
                }
                Aggregation => LegendKey::Aggregation,
                Composition => LegendKey::Composition,
                Inheritance => LegendKey::Inheritance,
                Realization => LegendKey::Realization,
                Dependency => LegendKey::Dependency,
            }
        };
        match tab {
            Tab::Class => {
                for r in view.relations.iter().filter(|r| r.status != Status::Pruned) {
                    keys.insert(rk(r));
                }
                if !metrics.cycle_edges.is_empty() {
                    keys.insert(LegendKey::Cycle);
                }
                if !metrics.layer_violations.is_empty() {
                    keys.insert(LegendKey::Violation);
                }
            }
            Tab::Domain => {
                for r in view.domain.as_class_model().1.iter().filter(|r| r.status != Status::Pruned) {
                    keys.insert(rk(r));
                }
            }
            Tab::Package => {
                keys.insert(LegendKey::Dependency);
            }
            Tab::System => {
                keys.insert(LegendKey::Directed);
                keys.insert(LegendKey::Dependency);
            }
            Tab::Usecase => {
                keys.insert(LegendKey::Association);
                keys.insert(LegendKey::Dependency);
            }
            Tab::Sequence => {}
        }
        if !view.proposals.is_empty() {
            keys.insert(LegendKey::Proposal);
        }
        LEGEND_ORDER.iter().copied().filter(|k| keys.contains(k)).collect()
    }

    /// Diagrams of the project that currently have something to draw (batch export).
    pub fn export_views(&self) -> Vec<ExportView> {
        let Some(snap) = &self.snap else { return vec![] };
        let d = &snap.view;
        let mut out = vec![];
        if !spide_diagram::views::system_empty(&snap.derived, C4Level::Context) {
            out.push(ExportView::SystemContext);
        }
        if !spide_diagram::views::system_empty(&snap.derived, C4Level::Container) {
            out.push(ExportView::SystemContainers);
        }
        let has_classes = d.classes.iter().any(|c| c.status != Status::Pruned);
        if has_classes {
            out.push(ExportView::Class);
        }
        out.extend(d.packages.iter().filter(|p| p.status.is_live()).map(|p| ExportView::ClassPackage(p.name.clone())));
        // agent-built views also when still missing: a batch asks the agent to build them first
        if !d.domain.is_empty() || has_classes {
            out.push(ExportView::Domain);
        }
        if !snap.derived.packages.packages.is_empty() {
            out.push(ExportView::Package);
        }
        if d.use_cases.use_cases.iter().any(|u| u.status != Status::Pruned) || has_classes {
            out.push(ExportView::Usecase);
        }
        for s in d.sequences.iter().filter(|s| s.status != Status::Pruned && !s.participants.is_empty()) {
            out.push(ExportView::Sequence(s.name.clone()));
        }
        out
    }

    /// The Lucid diagram key of the diagram on screen (`class`, `sequence:<name>` …).
    pub(super) fn lucid_key(&self) -> Option<String> {
        Some(match self.current_export_view() {
            ExportView::SystemContext => "system.context".into(),
            ExportView::SystemContainers => "system.containers".into(),
            ExportView::Class => "class".into(),
            ExportView::ClassPackage(p) => format!("class.package:{p}"),
            ExportView::Domain => "domain".into(),
            ExportView::Package => "package".into(),
            ExportView::Usecase => "usecase".into(),
            ExportView::Sequence(n) if !n.is_empty() => format!("sequence:{n}"),
            ExportView::Sequence(_) => return None,
        })
    }

    /// The diagram on screen.
    pub(super) fn current_export_view(&self) -> ExportView {
        match self.tab {
            Tab::System if self.views.level() == C4Level::Context => ExportView::SystemContext,
            Tab::System => ExportView::SystemContainers,
            Tab::Class => self.package_scope.as_ref().map(|p| ExportView::ClassPackage(p.clone())).unwrap_or(ExportView::Class),
            Tab::Domain => ExportView::Domain,
            Tab::Package => ExportView::Package,
            Tab::Usecase => ExportView::Usecase,
            Tab::Sequence => {
                let seqs = self.snap.as_ref().map(|s| s.view.sequences.iter().filter(|s| s.status != Status::Pruned).map(|s| s.name.clone()).collect::<Vec<_>>()).unwrap_or_default();
                let name = self.scenario.clone().filter(|n| seqs.contains(n)).or_else(|| seqs.first().cloned()).unwrap_or_default();
                ExportView::Sequence(name)
            }
        }
    }

    /// Build requested export data without changing live scope/selection/caches.
    fn export_class_data(&mut self, v: &ExportView, ghosts: bool) -> Option<Arc<ClassData>> {
        let scope = match v { ExportView::ClassPackage(p) => Some(p.clone()), _ => None };
        if let Some(package) = &scope {
            let snap = self.snap.as_ref()?;
            spide_core::scope::PackageScope::new(&snap.view, &snap.metrics, package, true).ok()?;
        }
        let original = std::mem::replace(&mut self.package_scope, scope);
        let cache = self.class_data.take();
        let result = self.class_data();
        self.package_scope = original;
        self.class_data = cache;
        if let ExportView::ClassPackage(package) = v {
            let snap = self.snap.as_ref()?;
            let result = result?;
            let mut model = snap.view.clone();
            model.classes = result.classes.clone();
            model.relations = result.relations.clone();
            model.packages = result.packages.clone();
            let scope = spide_core::scope::PackageScope::new(&model, &snap.metrics, package, ghosts).ok()?;
            let projected = scope.design(&model);
            return Some(Arc::new(ClassData {
                key: hash_of((result.key, ghosts)), classes: projected.classes, relations: projected.relations,
                packages: projected.packages,
                implicit: result.implicit.iter().filter(|e| scope.includes_edge(&e.from, &e.to)).cloned().collect(),
                scoped_metrics: Some(scope.metrics(&snap.metrics, &model)), saved: None,
            }));
        }
        result
    }

    /// Text and visual exports share scope semantics without recomputing health.
    fn export_plantuml_for(&self, v: &ExportView) -> Result<String, String> {
        let snap = self.snap.as_ref().ok_or("no project")?;
        if let ExportView::ClassPackage(package) = v {
            spide_core::plantuml::export_package(&snap.view, &snap.metrics, package)
        } else {
            spide_core::plantuml::export(v.plantuml(), &snap.view, &snap.metrics, &snap.view.layout)
        }
    }

    /// One diagram's export scene, shared by the preview, single and batch exports. Class and Domain
    /// at the canvas's own detail level come from the live canvas (its layout, incl. what you
    /// dragged); other levels from an off-screen canvas running the same layout pipeline. The layout
    /// is settled first: an export never captures a frame of a running animation.
    fn export_scene(&mut self, v: &ExportView, level: Option<u8>, o: &ExportOptions) -> Option<Scene> {
        let snap = self.snap.clone()?;
        match v {
            ExportView::Class | ExportView::ClassPackage(_) | ExportView::Domain => {
                let class = !matches!(v, ExportView::Domain);
                let data = if class { self.export_class_data(v, o.ghosts)? } else { self.domain_data()? };
                let view_key = if class { "class" } else { "domain" };
                let live_level = snap.view.settings.detail_for(view_key);
                let level = level.unwrap_or(live_level).min(4);
                let metrics = if class { Some(&snap.metrics) } else { None };
                let inp = self.canvas_input(&data, &snap, metrics, level, None);
                let now = self.now();
                let canvas = if level == live_level && *v == self.current_export_view() {
                    let slot = if class { &mut self.class_canvas } else { &mut self.domain_canvas };
                    slot.get_or_insert_with(|| ClassCanvas::new(format!("{}:{view_key}", snap.project_dir), view_key))
                } else {
                    let key = format!("{}:{view_key}:{level}:{}", snap.project_dir, data.key);
                    self.export_canvases.retain(|k, _| k.ends_with(&format!(":{}", data.key)));
                    self.export_canvases.entry(key.clone()).or_insert_with(|| ClassCanvas::new(key, view_key))
                };
                canvas.export_plain = Some(!o.diagnostics);
                let s = canvas.settled_scene(&self.measure, &inp, now);
                canvas.export_plain = None;
                Some(s)
            }
            _ => self.view_scene(&snap, v),
        }
    }

    fn view_scene(&mut self, snap: &Snapshot, v: &ExportView) -> Option<Scene> {
        let measure = &self.measure;
        match v {
            ExportView::Class | ExportView::ClassPackage(_) | ExportView::Domain => None,
            ExportView::Package => Some(spide_diagram::views::package_scene(measure, &snap.derived, &snap.metrics, &mut self.views.package).scene),
            ExportView::SystemContext | ExportView::SystemContainers => {
                let l = if *v == ExportView::SystemContext { C4Level::Context } else { C4Level::Container };
                Some(spide_diagram::views::system_scene(measure, &snap.derived, &snap.metrics, l, &mut self.views.system).scene)
            }
            ExportView::Usecase => Some(spide_diagram::views::usecase_scene(measure, &snap.view, &mut self.views.usecase).scene),
            ExportView::Sequence(name) => {
                let s = snap.view.sequences.iter().find(|s| &s.name == name && s.status != Status::Pruned)?;
                let actors: Vec<&str> = snap.view.use_cases.actors.iter().map(|a| a.name.as_str()).collect();
                Some(spide_diagram::views::sequence_scene(measure, s, &|n| actors.contains(&n)).scene)
            }
        }
    }

    fn export_tab(v: &ExportView) -> Tab {
        match v {
            ExportView::SystemContext | ExportView::SystemContainers => Tab::System,
            ExportView::Class | ExportView::ClassPackage(_) => Tab::Class,
            ExportView::Domain => Tab::Domain,
            ExportView::Package => Tab::Package,
            ExportView::Usecase => Tab::Usecase,
            ExportView::Sequence(_) => Tab::Sequence,
        }
    }

    /// The export SVG of one diagram (None: nothing to draw).
    pub(super) fn export_svg_for(&mut self, v: &ExportView, level: Option<u8>, o: &ExportOptions) -> Option<(String, u32, u32)> {
        let snap = self.snap.clone()?;
        let mut title = v.title();
        let level = level.or_else(|| v.has_detail().then(|| snap.view.settings.detail_for(if matches!(v, ExportView::Class | ExportView::ClassPackage(_)) { "class" } else { "domain" })));
        if let (true, Some(l)) = (v.has_detail(), level) {
            title = format!("{title} · {}", spide_diagram::visuals::DETAIL_LABELS[l.min(4) as usize]);
        }
        let meta = ExportMeta { title: snap.view.name.clone(), view: title, legend: self.legend_for(Self::export_tab(v)), date: super::format_date_today() };
        let scene = self.export_scene(v, level, o)?;
        build_export_svg(&scene, &self.measure, &meta, o)
    }

    /// The export SVG of the active view.
    fn export_svg(&mut self, o: &ExportOptions) -> Option<(String, u32, u32)> {
        let v = self.current_export_view();
        self.export_svg_for(&v, None, o)
    }

    /// One diagram as an Obsidian canvas (JSON Canvas): the same export scene, never diagnostics.
    fn export_canvas_for(&mut self, v: &ExportView, level: Option<u8>, o: &ExportOptions) -> Option<String> {
        use spide_diagram::canvas::{self, CanvasOptions, Source};
        let snap = self.snap.clone()?;
        let o = export::canvas_preview_options(o);
        let level = level.or_else(|| v.has_detail().then(|| snap.view.settings.detail_for(if matches!(v, ExportView::Class | ExportView::ClassPackage(_)) { "class" } else { "domain" })));
        let mut view_title = v.title();
        if let (true, Some(l)) = (v.has_detail(), level) {
            view_title = format!("{view_title} · {}", DETAIL_LABELS[l.min(4) as usize]);
        }
        let header = (o.title || o.date).then(|| {
            let mut h = String::new();
            if o.title {
                h += &format!("## {}\n", snap.view.name.trim());
            }
            let date = super::format_date_today();
            let sub: Vec<&str> = [Some(view_title.as_str()), o.date.then_some(date.as_str())].into_iter().flatten().collect();
            h += &sub.join(" · ");
            h
        });
        let scene = self.export_scene(v, level, &o)?;
        let opts = CanvasOptions { ghosts: o.ghosts, packages: o.packages, header };
        let actors: Vec<&str> = snap.view.use_cases.actors.iter().map(|a| a.name.as_str()).collect();
        let is_actor = |n: &str| actors.contains(&n);
        let data = match v {
            ExportView::Class | ExportView::ClassPackage(_) => Some(self.export_class_data(v, o.ghosts)?),
            ExportView::Domain => Some(self.domain_data()?),
            _ => None,
        };
        let src = match v {
            ExportView::Class | ExportView::ClassPackage(_) | ExportView::Domain => {
                let d = data.as_deref()?;
                Source::Classes { classes: &d.classes, relations: &d.relations, packages: &d.packages, package_deps: &d.scoped_metrics.as_ref().unwrap_or(&snap.metrics).package_deps, conceptual: *v == ExportView::Domain }
            }
            ExportView::Package => Source::Packages { packages: &snap.derived.packages.packages, deps: &snap.metrics.package_deps },
            ExportView::SystemContext => Source::System { view: &snap.derived.system, level: C4Level::Context },
            ExportView::SystemContainers => Source::System { view: &snap.derived.system, level: C4Level::Container },
            ExportView::Usecase => Source::UseCases { design: &snap.view },
            ExportView::Sequence(name) => Source::Sequence { seq: snap.view.sequences.iter().find(|s| &s.name == name && s.status != Status::Pruned)?, is_actor: &is_actor },
        };
        canvas::to_json_canvas(&scene, &self.measure, &src, &opts)
    }

    /// Write the batch's next file (one immutable design revision: a change stops the batch).
    fn batch_step(&mut self, run: &mut BatchRun) {
        let Some(snap) = self.snap.clone() else { return };
        if snap.rev != run.rev {
            run.cancelled = true;
            run.results.push(("—".into(), JobResult::Failed("the design changed during the export; stopped (finished files are kept)".into())));
            return;
        }
        let Some(job) = run.jobs.get(run.next).cloned() else { return };
        run.next += 1;
        let path = run.dir.join(&job.file);
        let missing = match job.view {
            ExportView::Domain => snap.view.domain.is_empty(),
            ExportView::Usecase => !snap.view.use_cases.use_cases.iter().any(|u| u.status != Status::Pruned),
            _ => false,
        };
        if missing {
            run.results.push((job.file.clone(), JobResult::Skipped("not generated yet".into())));
            return;
        }
        let result = match job.format {
            Format::Canvas => {
                let p = &run.profiles[job.profile.min(run.profiles.len() - 1)];
                let o = p.options.clone();
                match self.export_canvas_for(&job.view, job.level, &o) {
                    None => {
                        run.results.push((job.file.clone(), JobResult::Skipped("nothing to draw".into())));
                        return;
                    }
                    Some(json) => export_jobs::write_atomic(&path, json.as_bytes()),
                }
            }
            Format::Puml => self.export_plantuml_for(&job.view).and_then(|t| export_jobs::write_atomic(&path, t.as_bytes())),
            f => {
                let p = &run.profiles[job.profile.min(run.profiles.len() - 1)];
                match self.export_svg_for(&job.view, job.level, &p.options) {
                    None => {
                        run.results.push((job.file.clone(), JobResult::Skipped("nothing to draw".into())));
                        return;
                    }
                    Some((svg, _, _)) => {
                        let bytes = match f {
                            Format::Png => spide_core::png::render(&svg, p.scale.clamp(1, 6) as f32),
                            Format::Pdf => spide_core::pdf::render(&svg),
                            _ => Ok(svg.into_bytes()),
                        };
                        bytes.and_then(|b| export_jobs::write_atomic(&path, &b))
                    }
                }
            }
        };
        run.results.push((job.file.clone(), match result {
            Ok(()) => JobResult::Written(path.display().to_string()),
            Err(e) => JobResult::Failed(e),
        }));
    }

    /// A batch waiting for the agent: start writing once the views are fresh, or let the user decide.
    fn batch_regen_tick(&mut self, run: &mut BatchRun, batch: &BatchSettings) {
        let Some(snap) = self.snap.clone() else { return };
        let now = self.now();
        let BatchPhase::Regenerating { pending, since, saw_busy } = &mut run.phase else { return };
        if snap.pi_status == PiStatus::Busy {
            *saw_busy = true;
        }
        let still = export_jobs::stale_views(pending, &[], &snap.derived);
        if still.is_empty() && snap.pi_status != PiStatus::Busy {
            self.batch_start_writing(run, batch);
        } else if snap.pi_status == PiStatus::Offline {
            run.phase = BatchPhase::Stuck { pending: still, reason: "pi isn't running, so the out-of-date diagrams can't be regenerated.".into() };
        } else if snap.pi_status == PiStatus::Idle && (*saw_busy || now - *since > 8.0) {
            let list = export_jobs::view_list(&still);
            run.phase = BatchPhase::Stuck { pending: still, reason: format!("The agent didn't update: {list}.") };
        }
    }

    /// Plan the files from the current revision and write from now on.
    fn batch_start_writing(&mut self, run: &mut BatchRun, batch: &BatchSettings) {
        let Some(snap) = self.snap.clone() else { return };
        let views = self.export_views();
        run.jobs = export_jobs::jobs(&views, batch, &run.profiles);
        run.next = 0;
        run.rev = snap.rev;
        run.phase = BatchPhase::Writing;
    }

    /// Ask the agent to regenerate out-of-date views before a batch writes them.
    fn regenerate_for_export(&mut self, stale: Vec<ExportView>) -> BatchPhase {
        let since = self.now();
        let views: Vec<String> = stale.iter().filter_map(|v| match v {
            ExportView::Domain => Some("domain".to_string()),
            ExportView::Usecase => Some("usecase".to_string()),
            _ => None,
        }).collect();
        let sequences: Vec<String> = stale.iter().filter_map(|v| if let ExportView::Sequence(n) = v { Some(n.clone()) } else { None }).collect();
        let st = self.backend.st.clone();
        let pi = self.backend.pi.clone();
        match crate::commands::regenerate_for_export(&st, &pi, views, sequences) {
            Ok(()) => BatchPhase::Regenerating { pending: stale, since, saw_busy: false },
            Err(e) => BatchPhase::Stuck { pending: stale, reason: format!("Couldn't ask the agent: {e}") },
        }
    }

    pub(super) fn settings_for_project(&self) -> ExportSettings {
        let p = self.snap.as_ref().map(|s| s.project_dir.clone()).unwrap_or_default();
        self.export_settings.get(&p).cloned().unwrap_or_default()
    }

    pub(super) fn open_export(&mut self) {
        self.calm();
        self.export = Some(ExportDialog::new(self.settings_for_project()));
    }

    /// Run one export; asks for a path unless given. Returns the written path (None: cancelled).
    fn run_export(&mut self, s: &ExportSettings, path: Option<String>) -> Result<Option<String>, String> {
        let snap = self.snap.clone().ok_or("no design")?;
        let base = format!("{}-{}", slug(Some(&snap.view.name)), self.tab.key());
        let Some(target) = path.or_else(|| export::ask_path(s.format, &base, &snap.project_dir)) else { return Ok(None) };
        let st = self.backend.st.clone();
        match s.format {
            Format::Adr => {
                crate::commands::export_adr(&st, target.clone())?;
            }
            Format::Png | Format::Svg | Format::Pdf => {
                let (svg, _, _) = self.export_svg(&s.options).ok_or("This view has nothing to export yet.")?;
                match s.format {
                    Format::Png => crate::commands::export_png(svg, target.clone(), s.scale as f32)?,
                    Format::Svg => crate::commands::export_svg(svg, target.clone())?,
                    _ => crate::commands::export_pdf(svg, target.clone())?,
                }
            }
            Format::Canvas => {
                let v = self.current_export_view();
                let json = self.export_canvas_for(&v, None, &s.options).ok_or("This view has nothing to export yet.")?;
                crate::commands::export_text(json, target.clone())?;
            }
            Format::Puml => crate::commands::export_text(self.export_plantuml_for(&self.current_export_view())?, target.clone())?,
            Format::Json => crate::commands::save_as(&st, target.clone())?,
            Format::Archunit => crate::commands::export_archunit(&st, target.clone())?,
        }
        Ok(Some(target))
    }

    pub(super) fn export_again(&mut self) {
        let s = self.settings_for_project();
        let Some(last) = s.last_path.clone() else { return self.open_export() };
        self.calm();
        match self.run_export(&s, Some(last)) {
            Ok(Some(p)) => self.toast("info", format!("exported {p}")),
            Ok(None) => {}
            Err(e) => self.toast("error", e),
        }
    }

    fn quick_export(&mut self, format: Format) {
        self.calm();
        let s = ExportSettings { format, scale: 2, ..self.settings_for_project() };
        match self.run_export(&s, None) {
            Ok(Some(p)) => self.toast("info", format!("exported {p}")),
            Ok(None) => {}
            Err(e) => self.toast("error", e),
        }
    }

    fn export_folder(&mut self, title: &str, what: &'static str, adr: bool) {
        let mut dlg = rfd::FileDialog::new().set_title(title);
        if adr {
            if let Some(s) = &self.snap {
                dlg = dlg.set_directory(&s.project_dir);
            }
        }
        let Some(dir) = dlg.pick_folder() else { return };
        let dir = dir.display().to_string();
        self.backend.call(
            move |s, _| if adr { crate::commands::export_adr(s, dir.clone()).map(|f| (f, dir)) } else { crate::commands::export_plantuml_all(s, dir.clone()).map(|f| (f, dir)) },
            move |app, r| match r {
                Ok((files, dir)) => {
                    let n = files.len();
                    if adr {
                        app.toast("info", format!("wrote {n} {what}{} to {dir}", if n == 1 { "" } else { "s" }));
                    } else {
                        app.toast("info", format!("exported {n} diagram{} to {dir}", if n == 1 { "" } else { "s" }));
                    }
                }
                Err(e) => app.toast("error", e),
            },
        );
    }

    // ------------------------------------------------------------------ commands

    /// A `/command` from pi's `/spide`, the palette or a shortcut.
    pub fn command(&mut self, text: &str) {
        // Intentional app commands can leave review; drafts stay persisted, never committed implicitly.
        let is_resolve = text.starts_with("/accept") || text.starts_with("/reject");
        if self.review_active && !is_resolve {
            if !self.leave_proposal_review() {
                return;
            }
            self.review_active = false;
            self.refresh_proposal_canvas();
        }
        self.set_preview(None, PreviewSource::Option);
        let mut parts = text.splitn(2, ' ');
        let cmd = parts.next().unwrap_or("").to_string();
        let arg = parts.next().unwrap_or("").trim().to_string();
        let Some(snap) = self.snap.clone() else { return };
        let view = &snap.view;
        let metrics = &snap.metrics;
        match cmd.as_str() {
            "/trace" => self.trace_ui.open = !self.trace_ui.open,
            "/nvim" => {
                let root = std::path::PathBuf::from(&snap.project_dir);
                match crate::editor::embedded().start(&root) {
                    Ok(()) => self.show_editor(),
                    Err(e) => self.toast("error", e),
                }
            }
            "/goto" => {
                let (class, method) = match arg.split_once('.') {
                    Some((c, m)) => (c.to_string(), Some(m.to_string())),
                    None => (arg.clone(), None),
                };
                if class.is_empty() {
                    self.toast("info", "usage: /goto Class[.method]  (or ⌘-click it on the canvas)");
                } else {
                    self.open_in_editor(class, method);
                }
            }
            "/undo" => self.run(|s, _| crate::commands::undo(s)),
            "/redo" => self.run(|s, _| crate::commands::redo(s)),
            "/accept" | "/reject" => {
                if arg.is_empty() || !view.proposals.iter().any(|p| p.id == arg) {
                    return self.toast("error", format!("usage: {cmd} <proposal-id> · review one saved plan at a time"));
                }
                if self.proposal_ui.unsaved() {
                    return self.toast("error", "Save or discard the current draft before resolving proposals.");
                }
                if cmd == "/accept" {
                    self.backend.run(move |s, _| {
                        let review = crate::commands::review_proposal(s, arg.clone(), None)?;
                        if !review.can_accept {
                            let mut errs = review.errors.clone();
                            errs.extend(review.prerequisites.iter().map(|id| format!("Accept {id} first")));
                            return Err(if errs.is_empty() { "This plan cannot be accepted yet.".into() } else { errs.join("; ") });
                        }
                        crate::commands::accept_proposal(s, arg, Some(review.rev))
                    });
                } else {
                    let rev = snap.rev;
                    self.run(move |s, _| crate::commands::reject_proposal(s, arg, Some(rev)));
                }
            }
            "/tidy" => {
                if let Some(c) = self.class_canvas.as_mut() {
                    c.tidy();
                }
            }
            "/focus" => {
                if arg.is_empty() {
                    self.focus_canvas(None);
                    return;
                }
                let a = arg.to_lowercase();
                let pk = view.packages.iter().find(|p| p.name.to_lowercase() == a).or_else(|| view.packages.iter().find(|p| p.name.to_lowercase().contains(&a))).map(|p| p.name.clone());
                let Some(pk) = pk else { return self.toast("error", format!("no package matches “{arg}”")) };
                let go = move |app: &mut SpideApp| {
                    let snap = app.snap.clone().unwrap();
                    let now = app.now();
                    let ok = app.class_canvas.as_mut().is_some_and(|c| c.focus_package(&snap.view.classes, Some(&snap.metrics), &pk, now));
                    if !ok {
                        app.toast("error", format!("package {pk} has no classes on the canvas"));
                    }
                };
                if self.tab == Tab::Class {
                    go(self);
                } else {
                    self.tab = Tab::Class;
                    self.later(0.3, go);
                }
            }
            "/find" => {
                if arg.is_empty() {
                    self.focus_canvas(None);
                    return;
                }
                let Some(name) = find_class(view, &arg) else { return self.toast("error", format!("no class matches “{arg}”")) };
                let go = move |app: &mut SpideApp| {
                    if !app.focus_canvas(Some(&name)) {
                        app.toast("error", format!("{name} is not on the canvas"));
                    }
                };
                if self.tab == Tab::Class {
                    go(self);
                } else {
                    self.tab = Tab::Class;
                    self.later(0.3, go);
                }
            }
            "/detail" => {
                let v = if self.tab == Tab::Domain { "domain" } else { "class" }.to_string();
                let lvl = arg.parse::<u8>().ok().filter(|n| *n <= 4).or_else(|| if arg.is_empty() { None } else { DETAIL_LABELS.iter().position(|l| l.starts_with(&arg.to_lowercase())).map(|i| i as u8) });
                match lvl {
                    Some(l) => self.run(move |s, _| crate::commands::set_detail(s, v, l)),
                    None => self.toast("error", format!("usage: /detail 0–4 ({})", DETAIL_LABELS.iter().enumerate().map(|(i, l)| format!("{i} {l}")).collect::<Vec<_>>().join(" · "))),
                }
            }
            "/external" => {
                if arg.is_empty() {
                    return self.toast("error", "usage: /external <package|Class> — toggles the external tag");
                }
                if let Some(pk) = view.packages.iter().find(|p| p.name.to_lowercase() == arg.to_lowercase()) {
                    let (n, ext) = (pk.name.clone(), pk.external);
                    let msg = format!("{n}: {}", if ext { "ours again" } else { "external" });
                    return self.run_ok(move |s, _| crate::commands::apply_user_ops(s, vec![brief::op(json!({"op": "update_package", "name": n, "external": !ext}))]), move |app| app.toast("info", msg));
                }
                let c = find_class(view, &arg).and_then(|n| view.classes.iter().find(|c| c.name == n));
                let Some(c) = c else { return self.toast("error", format!("no package or class matches “{arg}”")) };
                let (n, ext) = (c.name.clone(), c.external);
                let msg = format!("{n}: {}", if ext { "ours again" } else { "external" });
                self.run_ok(move |s, _| crate::commands::apply_user_ops(s, vec![brief::op(json!({"op": "update_class", "name": n, "external": !ext}))]), move |app| app.toast("info", msg));
            }
            "/lens" => {
                let l = if arg.starts_with('c') {
                    Lens::Concerns
                } else if arg.starts_with('o') {
                    Lens::Overlap
                } else if arg.starts_with('s') {
                    if snap.project.is_none() {
                        return self.toast("error", "the sync lens needs a Spide Project (/project)");
                    }
                    Lens::Sync
                } else {
                    Lens::None
                };
                self.lens = if arg.is_empty() {
                    if self.lens == Lens::None {
                        Lens::Concerns
                    } else {
                        Lens::None
                    }
                } else {
                    l
                };
                if self.tab != Tab::Class {
                    self.tab = Tab::Class;
                }
                self.toast(
                    "info",
                    match l {
                        Lens::Concerns => "lens: method rows coloured by what they do",
                        Lens::Overlap => "lens: duplicated behaviour and scattered concepts",
                        Lens::Sync => "lens: design ↔ code sync (teal design ahead · violet code ahead · red conflict · amber unreconciled)",
                        Lens::None => "lens off",
                    },
                );
            }
            "/implicit" => {
                self.show_implicit = !self.show_implicit;
                self.toast("info", if !self.show_implicit { "implicit dependencies: only for the focused class" } else { "implicit dependencies: all shown" });
            }
            "/fit" => self.fit_active(),
            "/pruned" => {
                let on = !view.settings.show_pruned;
                self.run(move |s, _| crate::commands::set_show_pruned(s, on));
            }
            "/strict" => self.run_ok(|s, _| crate::commands::set_examiner(s, true), |app| app.toast("info", "examiner: strict — no overruling")),
            "/lenient" => self.run_ok(|s, _| crate::commands::set_examiner(s, false), |app| app.toast("info", "examiner: lenient — you may overrule")),
            "/history" => self.toast("info", "Scroll pi's native transcript in the terminal pane."),
            "/brief" => self.brief = !self.brief,
            "/timeline" => {
                self.tab = Tab::Class;
                self.timeline = !self.timeline;
                if !self.timeline {
                    self.close_timeline();
                }
            }
            "/stress" => {
                if arg.is_empty() {
                    self.stress = None;
                    return;
                }
                let c = metrics.changes.iter().find(|x| x.name.to_lowercase().contains(&arg.to_lowercase()));
                let Some(c) = c else { return self.toast("error", format!("no change scenario matches “{arg}”")) };
                if !c.analysed {
                    return self.toast("error", format!("“{}” isn't analysed yet", c.name));
                }
                self.tab = Tab::Class;
                self.stress = Some(c.name.clone());
            }
            "/branch" | "/branches" => {
                let mut w = arg.split_whitespace();
                let (sub, name) = (w.next().unwrap_or(""), w.next().map(String::from));
                if arg.is_empty() || cmd == "/branches" {
                    return self.open_branches();
                }
                if let (true, Some(name)) = (sub == "delete", name) {
                    let ok = rfd::MessageDialog::new().set_title("Delete branch").set_level(rfd::MessageLevel::Warning).set_description(format!("Delete branch “{name}”? This cannot be undone.")).set_buttons(rfd::MessageButtons::OkCancelCustom("Delete".into(), "Cancel".into())).show();
                    if !matches!(ok, rfd::MessageDialogResult::Ok | rfd::MessageDialogResult::Yes) && ok != rfd::MessageDialogResult::Custom("Delete".into()) {
                        return;
                    }
                    let msg = format!("deleted branch {name}");
                    return self.run_ok(move |s, _| crate::commands::branch_delete(s, name), move |app| app.toast("info", msg));
                }
                if !self.confirm_leave(super::leave::LeaveReason::SwitchBranch) {
                    return;
                }
                self.run(move |s, _| crate::commands::branch_create(s, arg));
            }
            "/switch" => {
                if arg.is_empty() {
                    return self.open_branches();
                }
                if !self.confirm_leave(super::leave::LeaveReason::SwitchBranch) {
                    return;
                }
                self.run(move |s, _| crate::commands::branch_switch(s, arg));
            }
            "/merge" => {
                if arg.is_empty() {
                    return self.toast("error", "usage: /merge <branch>");
                }
                let msg = format!("changes from {arg} proposed as ghosts");
                self.run_ok(move |s, _| crate::commands::branch_merge(s, arg), move |app| app.toast("info", msg));
            }
            "/compare" => {
                if !arg.is_empty() {
                    let name = arg.clone();
                    self.backend.run_then(
                        move |s, _| crate::commands::branch_compare(s, name),
                        move |app, c| {
                            let Some(snap) = app.snap.clone() else { return };
                            let focus: Vec<String> = c
                                .impact
                                .added
                                .iter()
                                .chain(c.impact.changed.iter())
                                .chain(c.impact.removed.iter())
                                .map(|x| x.trim_start_matches("class ").split(['.', ' ']).next().unwrap_or("").to_string())
                                .collect::<BTreeSet<_>>()
                                .into_iter()
                                .collect();
                            app.tab = Tab::Class;
                            app.branch_cmp = Some(BranchCmp {
                                name: arg.clone(),
                                cols: vec![
                                    CompareCol { id: "●".into(), label: format!("{} (current)", snap.branch), design: Some(snap.view.clone()), impact: None, focus: focus.clone(), error: None, summary: None },
                                    CompareCol { id: "⎇".into(), label: arg.clone(), design: Some(c.design), impact: Some(c.impact), focus, error: None, summary: None },
                                ],
                            });
                        },
                    );
                    return;
                }
                match view.decisions.iter().find(|x| matches!(x.state, DecisionState::Open | DecisionState::Rejected) && x.options.len() > 1) {
                    Some(d) => self.compare = Some(d.id.clone()),
                    None => self.toast("error", "no open decision with several options"),
                }
            }
            "/scope" => {
                if arg.is_empty() || arg == "all" { self.set_package_scope(None); }
                else if view.packages.iter().any(|p| p.name == arg && p.status.is_live()) { self.set_package_scope(Some(arg.into())); }
                else { self.toast("error", format!("unknown package: {arg}")); }
            }
            "/rules" => {
                if arg.is_empty() {
                    self.brief = true;
                    return;
                }
                let key = arg.replace(['-', ' '], "_");
                let Some(p) = brief::PRESETS.iter().find(|p| p.1 == key || p.2 == arg) else {
                    return self.toast("error", format!("presets: {}", brief::PRESETS.iter().map(|p| p.1).collect::<Vec<_>>().join(" | ")));
                };
                let rules = spide_core::rules::Rules { preset: p.0, ..view.rules.clone() };
                let msg = format!("rules: {} — {}", p.2, p.3);
                self.run_ok(move |s, _| crate::commands::set_rules(s, rules), move |app| app.toast("info", msg));
            }
            "/mode" => {
                if arg.is_empty() {
                    return self.toast("info", format!("mode: {} — {}", view.settings.partner.as_str(), brief::MODES.iter().map(|m| m.0.as_str()).collect::<Vec<_>>().join(" | ")));
                }
                let msg = format!("partner mode: {arg}");
                self.run_ok(move |s, _| crate::commands::set_partner(s, arg), move |app| app.toast("info", msg));
            }
            "/skip" => {
                let tab = self.tab.key().to_string();
                self.run(move |s, pi| {
                    crate::commands::set_brief_skipped(s, true)?;
                    crate::commands::send_prompt(s, pi, "I'd rather skip the brief for now. Let's start designing.".into(), tab)
                });
            }
            "/stop" => self.run(|_, pi| crate::commands::abort_agent(pi)),
            "/agent" => self.start_or_restart_agent(),
            "/folder" => self.run(|s, _| crate::commands::reveal_project(s)),
            "/add" => {
                let mut w = arg.split_whitespace();
                let (name, a2, a3) = (w.next(), w.next(), w.next());
                let pkg = match (a2, a3) {
                    (Some("in"), Some(p)) => Some(p),
                    (Some(p), None) => Some(p),
                    (None, None) => None,
                    _ => {
                        return self.toast("error", "usage: /add <Class> [in <package>]");
                    }
                };
                let Some(name) = name.map(String::from) else { return self.toast("error", "usage: /add <Class> [in <package>]") };
                if w.next().is_some() {
                    return self.toast("error", "usage: /add <Class> [in <package>]");
                }
                let mut o = json!({"op": "add_class", "name": name});
                if let Some(p) = pkg {
                    o["package"] = json!(p);
                }
                let msg = format!("added {name}");
                self.run_ok(move |s, _| crate::commands::apply_user_ops(s, vec![brief::op(o)]), move |app| app.toast("info", msg));
            }
            "/coderoot" => {
                let msg = if arg.is_empty() { "code root cleared".to_string() } else { format!("code root: {arg}") };
                let root = (!arg.is_empty()).then_some(arg);
                self.run_ok(move |s, _| crate::commands::set_code_root(s, root), move |app| app.toast("info", msg));
            }
            "/name" => {
                if arg.is_empty() {
                    return self.toast("error", "usage: /name <title>");
                }
                self.run(move |s, _| crate::commands::set_name(s, arg));
            }
            "/export" => {
                if arg.starts_with("ar") {
                    if view.settings.code_root.is_none() {
                        self.toast("info", "tip: /coderoot org.your.app makes the rules match your Java packages exactly");
                    }
                    if let Some(path) = rfd::FileDialog::new().set_file_name("ArchitectureTest.java").add_filter("Java", &["java"]).save_file() {
                        let path = path.display().to_string();
                        let msg = format!("exported {path} — needs com.tngtech.archunit:archunit-junit5 as a test dependency");
                        self.run_ok(move |s, _| crate::commands::export_archunit(s, path), move |app| app.toast("info", msg));
                    }
                } else if arg.starts_with("ad") {
                    self.export_folder("Folder for the ADRs (e.g. docs/adr)", "ADR", true);
                } else if arg.starts_with('a') {
                    self.export_folder("Folder for the PlantUML files", "diagram", false);
                } else if arg.starts_with('p') {
                    self.quick_export(if arg.starts_with("png") { Format::Png } else { Format::Puml });
                } else if arg.is_empty() {
                    self.open_export();
                } else {
                    self.toast("error", "usage: /export [png | puml | all | adr | archunit] — without an argument opens the export dialog");
                }
            }
            "/save" if arg == "copy" => self.save_copy_dialog(),
            "/save" => self.save_now(),
            "/close" => self.close_project(),
            "/project" => self.open_project_rail(),
            "/readjust" => {
                let classes: Vec<String> = arg.split_whitespace().map(|c| find_class(view, c).unwrap_or_else(|| c.to_string())).collect();
                self.run(move |s, pi| crate::commands::readjust(s, pi, classes));
            }
            "/implement" => {
                let classes: Vec<String> = arg.split_whitespace().map(|c| find_class(view, c).unwrap_or_else(|| c.to_string())).collect();
                self.run(move |s, pi| crate::commands::implement(s, pi, classes, false));
                self.open_project_rail();
            }
            "/verify" => self.run(|s, pi| crate::commands::verify(s, pi)),
            "/synced" => {
                if snap.project.is_none() {
                    return self.toast("error", "not a Spide Project (/project)");
                }
                self.open_project_rail();
                self.project_ui.confirm_mark = true;
            }
            "/reindex" => self.run(|s, _| crate::commands::reindex(s)),
            "/open" if arg.starts_with("project") => self.open_project_dialog(),
            // legacy: /open picked a design JSON. Same safe loader: a workspace's own design opens
            // it; any other file is imported into a new folder (never copied over a design).
            "/open" => self.import_dialog(),
            "/import" => self.import_dialog(),
            "/new" => self.new_project_dialog(),
            _ => self.toast("error", format!("unknown command {cmd}")),
        }
    }

    pub(super) fn fit_active(&mut self) {
        let now = self.now();
        match self.tab {
            Tab::Class | Tab::Domain => {
                let c = if self.tab == Tab::Class { self.class_canvas.as_mut() } else { self.domain_canvas.as_mut() };
                if let Some(c) = c {
                    let scene = std::mem::take(&mut c.scene);
                    c.vp.fit(&scene, &self.measure, true, now);
                    c.scene = scene;
                }
            }
            t => {
                let scene = std::mem::take(&mut self.views.scene);
                self.views.active_vp(t).fit(&scene, &self.measure, true, now);
                self.views.scene = scene;
            }
        }
    }

    pub(super) fn zoom_active(&mut self, f: f32) {
        let now = self.now();
        match self.tab {
            Tab::Class => {
                if let Some(c) = self.class_canvas.as_mut() {
                    c.vp.zoom_by(f, now)
                }
            }
            Tab::Domain => {
                if let Some(c) = self.domain_canvas.as_mut() {
                    c.vp.zoom_by(f, now)
                }
            }
            t => self.views.active_vp(t).zoom_by(f, now),
        }
    }

    // ------------------------------------------------------------------ keyboard

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let term_focus = self.term.as_ref().is_some_and(|t| t.has_focus) || self.nvim_term.as_ref().is_some_and(|t| t.has_focus && self.editor_pane);
        if term_focus {
            return;
        }
        let in_field = ctx.egui_wants_keyboard_input();
        let ev: Vec<(Key, egui::Modifiers)> = ctx.input(|i| i.events.iter().filter_map(|e| if let egui::Event::Key { key, pressed: true, modifiers, .. } = e { Some((*key, *modifiers)) } else { None }).collect());
        for (key, m) in ev {
            if in_field && key != Key::Escape {
                continue;
            }
            // every ⌘-shortcut belongs to the action registry: on macOS its native menu item
            // receives the key (dispatching here too would run it twice)
            if let Some(id) = super::actions::by_shortcut(key, m) {
                if !cfg!(target_os = "macos") {
                    self.perform(id);
                }
                continue;
            }
            if key == Key::Escape {
                self.escape(in_field);
            }
            // canvas keyboard: Delete prunes the selection, ↑/↓ step through classes
            if !in_field && m.is_none() {
                match key {
                    Key::Delete | Key::Backspace if !self.selection.is_empty() => self.delete_selection(),
                    Key::ArrowDown => self.select_step(true),
                    Key::ArrowUp => self.select_step(false),
                    _ => {}
                }
            }
        }
    }

    /// Escape dismisses exactly one layer: the topmost overlay, else the topmost transient state.
    /// Overlays that read Escape themselves (Find, palette, export, dialogs) are left to do so.
    fn escape(&mut self, in_field: bool) {
        if self.export.is_some() || self.palette.is_some() || self.compare.is_some() || self.branch_cmp.is_some() || self.find.is_some() || self.show_shortcuts || self.settings_open || self.activity_open || self.integrations.open || self.documentation_open || self.about_open || self.new_project.is_some() || self.branches.is_some() || self.context.is_some() || self.class_editor.is_some() || self.new_element.is_some() {
            return;
        }
        if self.prune.is_some() {
            self.prune = None;
        } else if self.inline.is_some() {
            // the inline editor cancels itself
        } else if self.hover.snapshot().is_some_and(|h| h.pinned) {
            self.hover.clear();
        } else if self.spotlight.is_some() {
            self.spotlight = None;
        } else if in_field {
            // Escape in a text field only leaves the field
        } else if !self.selection.is_empty() {
            self.selection.clear();
            self.focus_canvas(None);
        } else if self.stress.is_some() || self.shown_decision.is_some() {
            self.stress = None;
            self.shown_decision = None;
        } else if self.brief {
            self.brief = false;
        } else {
            self.focus_canvas(None);
            self.hover.clear();
        }
    }

    /// When the last overlay closes, keyboard focus returns to the terminal it came from.
    fn restore_overlay_focus(&mut self) {
        let open = self.find.is_some() || self.palette.is_some() || self.show_shortcuts || self.new_project.is_some() || self.branches.is_some() || self.class_editor.is_some() || self.new_element.is_some();
        if open {
            if self.return_focus.is_none() {
                let pi = self.term.as_ref().is_some_and(|t| t.has_focus);
                let nvim = self.editor_pane && self.nvim_term.as_ref().is_some_and(|t| t.has_focus);
                self.return_focus = Some(if pi { 1 } else if nvim { 2 } else { 0 });
            }
        } else if let Some(r) = self.return_focus.take() {
            match r {
                1 => if let Some(t) = self.term.as_mut() { t.focus_requested = true },
                2 => if let Some(t) = self.nvim_term.as_mut() { t.focus_requested = true },
                _ => {}
            }
        }
    }

    // ------------------------------------------------------------------ frame

    fn canvas_input<'a>(&self, data: &'a ClassData, snap: &'a Snapshot, metrics: Option<&'a spide_core::metrics::Metrics>, level: u8, hovered: Option<&'a str>) -> CanvasInput<'a> {
        let class = metrics.is_some();
        let metrics = data.scoped_metrics.as_ref().or(metrics);
        CanvasInput {
            classes: &data.classes,
            relations: &data.relations,
            packages: &data.packages,
            metrics,
            implicit: &data.implicit,
            data_key: data.key,
            saved: data.saved.as_ref(),
            show_methods: class,
            level,
            show_implicit: class && self.show_implicit,
            lens: if class && self.preview.is_none() { self.lens } else { Lens::None },
            read_only: snap.workspace_status != crate::state::WorkspaceStatus::Ready || (class && (self.preview.is_some() || self.review_active)),
            review_focus: None,
            stress: None,
            sync: None,
            hovered,
            spotlight: None,
            can_move_to_package: snap.workspace_status == crate::state::WorkspaceStatus::Ready && class && self.preview.is_none() && !self.review_active,
            can_edit: snap.workspace_status == crate::state::WorkspaceStatus::Ready && !(class && (self.preview.is_some() || self.review_active)),
            pickable: class && self.review_active && self.preview.is_none(),
            highlight: None,
            keep_layout: false,
        }
    }

    pub fn frame(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.drain(&ctx);
        super::viewport::set_comment_badges(&ctx, self.snap.as_deref().map(|s| &s.view));
        let dropped: Vec<_> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect());
        if !dropped.is_empty() { self.handle_open_paths(dropped); }
        if let Some(op) = super::trace::show(&ctx, &mut self.trace_ui, &self.backend.st) {
            self.backend.call(
                move |st, _| crate::service::get(st).ok_or("service not ready".to_string())?.call_tool("revert_op", &json!({"op": op}), "user"),
                |app, r| match r {
                    Ok(_) => app.toast("info", "reverted"),
                    Err(e) => app.toast("error", e),
                },
            );
        }
        let now = ctx.input(|i| i.time);
        // deferred actions (old setTimeout)
        let due: Vec<Deferred> = {
            let mut due = vec![];
            let mut i = 0;
            while i < self.deferred.len() {
                if self.deferred[i].0 <= now {
                    due.push(self.deferred.remove(i).1);
                } else {
                    i += 1;
                }
            }
            due
        };
        for f in due {
            f(self);
        }
        if !self.deferred.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
        if let Some(d) = self.hover.tick(now) {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(d));
        }
        self.shortcuts(&ctx);
        if let Some(e) = self.proposal_ui.drafts.error.take() {
            self.toast("error", e);
        }
        let full = ui.max_rect();
        ui.painter().rect_filled(full, 0.0, css::BG);
        let Some(snap) = self.snap.clone() else {
            ui.vertical_centered(|ui| {
                ui.spinner();
                ui.label("Loading workspace…");
                if ui.button("Retry workspace snapshot").clicked() { self.backend.st.emit(); }
            });
            return;
        };
        // revision / project / tab changes close previews and cards
        let rk = (snap.rev, snap.project_dir.clone());
        if self.last_rev.as_ref() != Some(&rk) || self.last_tab != self.tab {
            if self.last_rev.as_ref().map(|r| &r.1) != Some(&rk.1) {
                self.class_canvas = None;
                self.domain_canvas = None;
                self.views = Views::default();
            }
            let tab_changed = self.last_tab != self.tab;
            self.last_rev = Some(rk);
            self.last_tab = self.tab;
            if self.preview_source == PreviewSource::Option || tab_changed {
                if self.preview.is_some() {
                    self.set_preview(None, self.preview_source);
                }
            }
            if !self.hover.snapshot().is_some_and(|h| h.pinned) {
                self.hover.clear();
            }
        }
        // titlebar (drag region)
        let title = Rect::from_min_size(full.min, Vec2::new(full.width(), 30.0));
        let tr = ui.interact(title, ui.scope_id().with("titlebar"), Sense::click_and_drag());
        if tr.drag_started() {
            ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        if tr.double_clicked() {
            let max = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!max));
        }
        let mut workspace_top = full.top() + 30.0;
        let status = &snap.workspace_status;
        let problem = match status {
            crate::state::WorkspaceStatus::ReadOnly => Some("Read-only — another process owns this workspace. Editing and autosave are disabled.".to_string()),
            crate::state::WorkspaceStatus::Failed(e) => Some(e.clone()),
            crate::state::WorkspaceStatus::Loading => Some("Loading project…".to_string()),
            crate::state::WorkspaceStatus::Closed => None,
            crate::state::WorkspaceStatus::Ready => snap.save_status.error.clone()
                .or_else(|| snap.save_status.history_error.as_ref().map(|e| format!("Design saved, but history could not be written: {e}")))
                .or_else(|| snap.save_status.recovery_error.as_ref().map(|e| format!("Design saved, but its recovery copy could not be kept: {e}"))),
        };
        if let Some(problem) = problem {
            let banner = Rect::from_min_size(Pos2::new(full.left() + 12.0, workspace_top), Vec2::new(full.width() - 24.0, 64.0));
            ui.scope_builder(egui::UiBuilder::new().max_rect(banner), |ui| {
                ui.label(RichText::new(&problem).color(css::WARN)).on_hover_text(&snap.design_path);
                ui.horizontal(|ui| {
                    if *status == crate::state::WorkspaceStatus::Ready && ui.button("Retry save").clicked() {
                        self.run(|s, _| s.persist_and_emit());
                    }
                    if !matches!(status, crate::state::WorkspaceStatus::Failed(_) | crate::state::WorkspaceStatus::Closed | crate::state::WorkspaceStatus::Loading)
                        && ui.button("Save Design Copy…").clicked() {
                        self.command("/save");
                    }
                    if matches!(status, crate::state::WorkspaceStatus::Failed(_)) && ui.button("Recover a Copy…").on_hover_text("Create a new workspace from the newest recovery copy or history. The broken file is left untouched.").clicked() {
                        if let Some(dir) = rfd::FileDialog::new().set_title("New, empty folder for the recovered copy").pick_folder() {
                            let dir = dir.display().to_string();
                            self.backend.transition(move |s, pi| crate::commands::recover_copy(s, pi, dir), |app, r| match r {
                                Ok(label) => {
                                    app.reset_workspace_ui();
                                    app.toast("info", format!("Recovered from {label}"));
                                }
                                Err(e) => app.toast("error", e),
                            });
                        }
                    }
                    if ui.button("Open another project…").clicked() {
                        self.open_project_dialog();
                    }
                    if matches!(status, crate::state::WorkspaceStatus::Failed(_)) && ui.button("Close").on_hover_text("Back to the welcome screen").clicked() {
                        self.close_project();
                    }
                });
            });
            workspace_top += 68.0;
        }
        let ws = Rect::from_min_max(Pos2::new(full.left(), workspace_top), full.max);
        self.update_title(&snap);
        if matches!(snap.workspace_status, crate::state::WorkspaceStatus::Closed | crate::state::WorkspaceStatus::Failed(_)) {
            self.welcome_layout(ui, ws, &snap);
        } else {
            self.workspace_layout(ui, ws, &snap);
        }
        self.find_overlay();
        self.context_overlay();
        self.property_overlays();
        self.shortcuts_overlay();
        self.settings_overlay();
        self.draft_recovery_overlay();
        self.observe_operation_status();
        self.activity_overlay();
        self.integration_overlay();
        self.help_overlays();
        if now - self.last_draft_save > 1.0 {
            let _ = self.save_input_drafts(); // retained error is displayed by draft_recovery_overlay
            self.last_draft_save = now;
        }
        self.restore_cameras();
        if now - self.last_workspace_save > 5.0 {
            self.remember_workspace();
            if self.prefs_path.is_some() { self.save_prefs(); }
            self.last_workspace_save = now;
        }
        self.branches_overlay();
        self.restore_overlay_focus();
        self.sync_menu();
        if let Some(mut f) = self.new_project.take() {
            match super::welcome::new_project_dialog(&ctx, &mut f) {
                Some(super::welcome::NewOut::Create) => self.create_new_project(f),
                Some(super::welcome::NewOut::Cancel) => {}
                None => self.new_project = Some(f),
            }
        }
        chrome::toasts(&ctx, self.canvas_rect, &mut self.toasts);
    }

    fn workspace_layout(&mut self, ui: &mut egui::Ui, ws: Rect, snap: &Arc<Snapshot>) {
        let width = ws.width();
        let narrow = width < 1120.0;
        let s = &self.workspace;
        let left_open = s.left_open && width >= 570.0;
        let right_open = if narrow { self.drawer } else { s.right_open };
        let left = if left_open { s.left.min(width - 252.0 - if !narrow && right_open { 300.0 } else { 30.0 }).max(280.0) } else { 30.0 };
        let right = if !narrow && right_open { s.right.min(width - left - 252.0).max(300.0) } else { 30.0 };
        let left_r = Rect::from_min_size(ws.min, Vec2::new(left, ws.height()));
        let right_r = Rect::from_min_max(Pos2::new(ws.right() - right, ws.top()), ws.max);
        let canvas_r = Rect::from_min_max(Pos2::new(left_r.right() + 1.0, ws.top()), Pos2::new(right_r.left() - 1.0, ws.bottom()));
        self.canvas_rect = canvas_r;
        let p = ui.painter();
        p.rect_filled(left_r, 0.0, css::SURFACE);
        p.rect_filled(right_r, 0.0, css::SURFACE);
        // separators
        for (is_left, x, active) in [(true, left_r.right() + 0.5, left_open), (false, right_r.left() - 0.5, right_open && !narrow)] {
            let hit = Rect::from_min_max(Pos2::new(x - 4.0, ws.top()), Pos2::new(x + 4.0, ws.bottom()));
            let r = ui.interact(hit, ui.scope_id().with(("sep", is_left)), if active { Sense::drag() } else { Sense::hover() });
            let hot = active && (r.hovered() || r.dragged());
            ui.painter().vline(x, ws.y_range(), Stroke::new(1.0, if hot { css::ACCENT } else { css::SEPARATOR }));
            if active && (r.hovered() || r.dragged()) {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeColumn);
            }
            if r.drag_started() {
                self.hover.clear();
                self.dragging_sep = Some((is_left, if is_left { left } else { right }, r.interact_pointer_pos().map(|p| p.x).unwrap_or(x)));
            }
            if let (true, Some((side, start, x0))) = (r.dragged(), self.dragging_sep) {
                if side == is_left {
                    let px = r.interact_pointer_pos().map(|p| p.x).unwrap_or(x0);
                    let d = (px - x0) * if is_left { 1.0 } else { -1.0 };
                    let min = if is_left { 280.0 } else { 300.0 };
                    let max = (width - if is_left { right } else { left } - 260.0).max(min);
                    let v = (start + d).clamp(min, max);
                    if is_left {
                        self.workspace.left = v;
                    } else {
                        self.workspace.right = v;
                    }
                }
            }
            if r.drag_stopped() {
                self.dragging_sep = None;
            }
        }
        // left: pi terminal
        ui.scope_builder(egui::UiBuilder::new().max_rect(left_r), |ui| {
            ui.set_clip_rect(left_r);
            if left_open {
                self.terminal_pane(ui, left_r, snap);
            } else if rail(ui, left_r, "pi ›").clicked() {
                self.workspace.left_open = true;
            }
        });
        // centre: canvas workspace
        ui.scope_builder(egui::UiBuilder::new().max_rect(canvas_r), |ui| {
            ui.set_clip_rect(canvas_r);
            self.canvas_workspace(ui, canvas_r, snap);
        });
        // right: inspector (a drawer over the canvas in narrow windows)
        let pending: Vec<&spide_core::model::Decision> = snap.view.decisions.iter().filter(|d| d.is_pending()).collect();
        let n_pending = pending.len();
        if narrow && right_open {
            let w = (width - left - 12.0).min(400.0);
            let dr = Rect::from_min_max(Pos2::new(ws.right() - w, ws.top()), ws.max);
            egui::Area::new(egui::Id::unique("inspector-drawer")).order(egui::Order::Foreground).fixed_pos(dr.min).show(ui.ctx(), |ui| {
                let (r, _) = ui.allocate_exact_size(dr.size(), Sense::hover());
                ui.painter().rect_filled(r, 0.0, css::SURFACE);
                ui.painter().vline(r.left(), r.y_range(), Stroke::new(1.0, css::LINE));
                ui.scope_builder(egui::UiBuilder::new().max_rect(r), |ui| self.inspector(ui, r, snap, true));
            });
        }
        ui.scope_builder(egui::UiBuilder::new().max_rect(right_r), |ui| {
            ui.set_clip_rect(right_r);
            if !narrow && right_open {
                self.inspector(ui, right_r, snap, false);
            } else {
                let label = if n_pending > 0 { format!("‹ Review {n_pending}") } else { "‹ Review".into() };
                if rail(ui, right_r, &label).clicked() {
                    if narrow {
                        self.drawer = !self.drawer;
                    } else {
                        self.workspace.right_open = true;
                    }
                }
            }
        });
    }

    fn terminal_pane(&mut self, ui: &mut egui::Ui, r: Rect, snap: &Arc<Snapshot>) {
        let header = Rect::from_min_size(r.min, Vec2::new(r.width(), 40.0));
        ui.painter().hline(header.x_range(), header.bottom(), Stroke::new(1.0, css::LINE));
        let mut action = None;
        ui.scope_builder(egui::UiBuilder::new().max_rect(header), |ui| {
            action = terminal::header(ui, snap.pi_status, self.restarting, &mut self.term_confirm, self.pane_palette.is_some(), self.editor_pane);
        });
        match action {
            Some(PaneAction::Stop) => self.stop_agent(),
            Some(PaneAction::Collapse) => self.workspace.left_open = false,
            Some(PaneAction::ToggleEditor) => {
                if self.editor_pane {
                    self.editor_pane = false;
                } else {
                    let root = std::path::PathBuf::from(&snap.project_dir);
                    match crate::editor::embedded().start(&root) {
                        Ok(()) => self.show_editor(),
                        Err(e) => self.toast("error", e),
                    }
                }
            }
            Some(PaneAction::Commands) => self.pane_palette = if self.pane_palette.is_some() { None } else { Some(Palette::new("")) },
            _ => {}
        }
        let mut body = Rect::from_min_max(Pos2::new(r.left(), header.bottom() + 1.0), r.max);
        if self.term_confirm {
            let cr = Rect::from_min_size(body.min, Vec2::new(r.width(), 64.0));
            let mut res = None;
            let used = ui.scope_builder(egui::UiBuilder::new().max_rect(cr), |ui| res = terminal::confirm_restart(ui)).response.rect;
            body.min.y = used.bottom();
            match res {
                Some(true) => {
                    self.term_confirm = false;
                    self.restarting = true;
                    self.backend.call(|s, pi| crate::commands::restart_agent(s, pi), |app, r| {
                        app.restarting = false;
                        if let Err(e) = r {
                            if let Some(t) = &app.term {
                                t.state.lock().unwrap().error = Some(e);
                            }
                        }
                    });
                }
                Some(false) => self.term_confirm = false,
                None => {}
            }
        }
        let error = if self.editor_pane { self.nvim_term.as_ref().and_then(|t| t.error()) } else { self.term.as_ref().and_then(|t| t.error()).or_else(|| snap.pi_error.clone()) };
        let mut term_r = body;
        if let Some(e) = &error {
            let g = ui.painter().layout(e.clone(), sans(12.0), Color32::WHITE, r.width() - 24.0);
            let h = (g.size().y + 36.0).min(110.0);
            let er = Rect::from_min_max(Pos2::new(r.left(), r.bottom() - h), r.max);
            term_r.max.y = er.top();
            ui.scope_builder(egui::UiBuilder::new().max_rect(er), |ui| terminal::error_banner(ui, e));
        }
        if self.editor_pane {
            if let Some(t) = self.nvim_term.as_mut() {
                ui.scope_builder(egui::UiBuilder::new().max_rect(term_r), |ui| t.show(ui, term_r));
            }
        } else if let Some(t) = self.term.as_mut() {
            t.tab = self.tab.key().into();
            ui.scope_builder(egui::UiBuilder::new().max_rect(term_r), |ui| t.show(ui, term_r));
        } else {
            ui.painter().rect_filled(term_r, 0.0, css::SURFACE);
        }
        if let Some(p) = self.pane_palette.as_mut() {
            let pr = Rect::from_min_size(Pos2::new(r.left() + 4.0, r.top() + 38.0), Vec2::new(r.width() - 8.0, 10.0));
            let mut act = None;
            egui::Area::new(egui::Id::unique("pane-palette")).order(egui::Order::Foreground).fixed_pos(pr.min).show(ui.ctx(), |ui| {
                ui.set_width(pr.width());
                act = overlays::palette(ui, p, &self.prefs.favorite_commands);
            });
            match act {
                Some(PaletteAction::ToggleFavorite(c)) => self.toggle_favorite_command(c),
                Some(PaletteAction::Close) => self.pane_palette = None,
                Some(PaletteAction::Run(c)) => {
                    self.pane_palette = None;
                    self.command(&format!("/{c}"));
                }
                None => {}
            }
        }
    }

    fn inspector(&mut self, ui: &mut egui::Ui, r: Rect, snap: &Arc<Snapshot>, drawer: bool) {
        let narrow = drawer;
        // toggle
        let tr = Rect::from_min_size(Pos2::new(r.right() - 34.0, r.top()), Vec2::new(30.0, 30.0));
        let toggle = ui.put(tr, egui::Button::new(RichText::new("›").font(sans(14.0)).color(css::DIM)).frame(false));
        if toggle.on_hover_text("Collapse design inspector").clicked() {
            if narrow {
                self.drawer = false;
            } else {
                self.workspace.right_open = false;
            }
        }
        let pending = snap.view.proposals.len();
        let resolved = snap.view.decisions.iter().filter(|d| !d.is_pending()).count();
        let nav = Rect::from_min_size(Pos2::new(r.left(), r.top() + 30.0), Vec2::new(r.width(), 50.0));
        ui.painter().rect_filled(nav, 0.0, css::BG);
        let history_mode = self.history_open || self.shown_decision.is_some();
        ui.scope_builder(egui::UiBuilder::new().max_rect(nav.shrink2(Vec2::new(12.0, 12.0))), |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                let mode = |ui: &mut egui::Ui, text: String, on: bool| {
                    ui.add(egui::Button::new(RichText::new(text).font(sans(12.5)).color(if on { css::GHOST } else { css::TEXT })).fill(if on { css::RAISED } else { Color32::TRANSPARENT }).stroke(Stroke::NONE).corner_radius(6).min_size(Vec2::new(0.0, 26.0)))
                };
                let review_label = if pending > 0 { format!("Review ({pending})") } else { "Review".into() };
                if mode(ui, review_label, !history_mode && !self.project_open).on_hover_text("Pending changes, shown on the diagram").clicked() {
                    self.open_proposal_review(None);
                }
                let project_label = match snap.project.as_ref().and_then(|p| p.report.as_ref()) {
                    Some(r) => {
                        let n = r.count(SyncStatus::Conflict) + r.count(SyncStatus::DesignAhead) + r.count(SyncStatus::CodeAhead) + r.count(SyncStatus::Unreconciled);
                        if n > 0 { format!("Project ({n})") } else { "Project ✓".to_string() }
                    }
                    None => "Project".to_string(),
                };
                if mode(ui, project_label, self.project_open).on_hover_text("Spide Project: the design with its code — sync, readjust, implement").clicked() {
                    self.open_project_rail();
                }
                if mode(ui, format!("History ({resolved})"), history_mode && !self.project_open).on_hover_text("Resolved decisions (read-only)").clicked() && self.leave_proposal_review() {
                    self.project_open = false;
                    self.history_open = true;
                    self.shown_decision = None;
                    self.review_active = false;
                    self.refresh_proposal_canvas();
                }
            });
        });
        let body = Rect::from_min_max(Pos2::new(r.left(), nav.bottom()), r.max);
        ui.scope_builder(egui::UiBuilder::new().max_rect(body), |ui| {
            ui.set_clip_rect(body);
            if self.project_open {
                for o in super::project::show(ui, &mut self.project_ui, snap, &self.backend) {
                    self.project_out(o);
                }
            } else if let Some(id) = self.shown_decision.clone() {
                if decisions::history(ui, &snap.view, &id) {
                    self.shown_decision = None;
                    self.history_open = true;
                }
            } else if self.history_open {
                if let Some(id) = history_list(ui, snap) {
                    self.shown_decision = Some(id);
                }
            } else if pending > 0 {
                if !self.review_active {
                    self.review_active = true;
                    self.review_locate.get_or_insert((String::new(), self.now()));
                    self.class_data = None;
                }
                let now = ui.input(|i| i.time);
                let mut out = vec![];
                proposal::show(ui, &mut self.proposal_ui, snap, self.selected_proposal.as_deref(), &self.backend, now, &mut out);
                for o in out {
                    match o {
                        ProposalOut::Select(id) => {
                            if id.is_some() && id != self.selected_proposal {
                                self.review_locate = id.clone().map(|i| (i, now));
                            }
                            self.selected_proposal = id;
                        }
                        ProposalOut::Focus(name) => {
                            self.tab = Tab::Class;
                            self.later(0.15, move |app| {
                                if !app.focus_canvas(Some(&name)) {
                                    app.toast("info", "This change has no element on the class diagram; its details are in the list.");
                                }
                            });
                        }
                        ProposalOut::Compare(id) => self.compare = Some(id),
                        ProposalOut::Canvas => self.refresh_proposal_canvas(),
                        ProposalOut::Next(n) => self.next_step(n),
                    }
                }
            } else {
                if self.review_active {
                    // nothing left to review: the canvas is editable again
                    self.review_active = false;
                    self.proposal_ui.review = None;
                    self.refresh_proposal_canvas();
                }
                if let Some(n) = proposal::completion(ui, snap) {
                    self.next_step(n);
                }
            }
        });
    }

    /// Show the Project rail (create/open a project in design-only mode).
    pub(super) fn open_project_rail(&mut self) {
        if !self.leave_proposal_review() {
            return;
        }
        self.project_open = true;
        self.history_open = false;
        self.shown_decision = None;
        self.review_active = false;
        self.workspace.right_open = true;
        self.drawer = true;
        self.refresh_proposal_canvas();
    }

    fn project_out(&mut self, o: super::project::ProjectOut) {
        use super::project::ProjectOut;
        match o {
            ProjectOut::Focus(c) => {
                self.tab = Tab::Class;
                self.later(0.15, move |app| {
                    if !app.focus_canvas(Some(&c)) {
                        app.lens = Lens::Sync;
                        app.class_data = None;
                        app.later(0.3, move |app| {
                            app.focus_canvas(Some(&c));
                        });
                    }
                });
            }
            ProjectOut::Lens => {
                self.lens = Lens::Sync;
                self.tab = Tab::Class;
                self.class_data = None;
            }
            ProjectOut::CreateIn => {
                let Some(dir) = rfd::FileDialog::new().set_title("Module folder for the Spide Project (where the code lives)").pick_folder() else { return };
                if spide_core::project::is_project(&dir) {
                    if !self.confirm_leave(super::leave::LeaveReason::SwitchProject) {
                        return;
                    }
                    let d = dir.display().to_string();
                    return self.run_transition(move |s, pi| crate::commands::open_project(s, pi, d));
                }
                let name = self.snap.as_ref().map(|s| s.view.name.clone()).filter(|n| n != "Untitled design").unwrap_or_else(|| dir.file_name().and_then(|n| n.to_str()).unwrap_or("project").to_string());
                let m = crate::project::suggest_manifest(&dir, &name);
                self.project_ui.form = Some(super::project::Form::new(dir, &m, false));
            }
            ProjectOut::Open => self.open_project_dialog(),
            ProjectOut::Create(root, m, git) => {
                if !self.confirm_leave(super::leave::LeaveReason::SwitchProject) {
                    return;
                }
                self.backend.transition(
                    move |s, pi| crate::commands::create_project(s, pi, root, m, git),
                    |app, r: Result<Vec<String>, String>| {
                        let added = match r {
                            Ok(a) => a,
                            Err(e) => return app.toast("error", e),
                        };
                        app.reset_workspace_ui();
                        app.toast("info", if added.is_empty() { "Spide Project ready".to_string() } else { format!("Spide Project ready · local files excluded from git ({})", added.join(", ")) });
                    },
                );
            }
        }
    }

    pub(super) fn open_project_dialog(&mut self) {
        if !self.confirm_leave(super::leave::LeaveReason::SwitchProject) {
            return;
        }
        if let Some(dir) = rfd::FileDialog::new().set_title("Open a project folder (with design.json or spide/project.json)").pick_folder() {
            let d = dir.display().to_string();
            self.run_transition(move |s, pi| crate::commands::open_folder(s, pi, d));
        }
    }

    fn next_step(&mut self, n: proposal::NextStep) {
        use proposal::NextStep;
        match n {
            NextStep::Brief => self.brief = true,
            NextStep::Behaviour => {
                self.tab = Tab::Sequence;
                self.review_active = false;
                self.refresh_proposal_canvas();
            }
            NextStep::History => {
                self.history_open = true;
            }
            NextStep::Implement => {
                self.open_project_rail();
                self.run(|s, pi| crate::commands::implement(s, pi, vec![], false));
            }
            NextStep::FocusPi => {
                self.workspace.left_open = true;
                if let Some(t) = self.term.as_mut() {
                    t.focus_requested = true;
                }
            }
        }
    }

    fn canvas_workspace(&mut self, ui: &mut egui::Ui, r: Rect, snap: &Arc<Snapshot>) {
        let ctx = ui.ctx().clone();
        let stage = Rect::from_min_max(Pos2::new(r.left(), r.top() + 124.0), r.max);
        // clicks in the canvas close pinned cards and the ghost spotlight
        if ctx.input(|i| i.pointer.any_pressed()) && ui.rect_contains_pointer(r) {
            let over_card = ctx.layer_id_at(ctx.input(|i| i.pointer.interact_pos().unwrap_or_default())).is_some_and(|l| l.id == egui::Id::unique("hovercard"));
            if !over_card {
                if self.hover.snapshot().is_some_and(|h| h.pinned) {
                    self.hover.clear();
                }
                self.spotlight = None;
            }
        }
        // ---------------------------------------------------------- stage
        let hovered = self.hover.snapshot().map(|h| h.key.clone());
        let mut canvas_out = vec![];
        let mut view_out = vec![];
        match self.tab {
            Tab::Class => {
                if let Some(data) = self.class_data() {
                    let key = format!("{}:{}:{:?}", snap.project_dir, if self.preview.is_some() { "preview" } else { "class" }, self.package_scope);
                    if self.class_canvas.as_ref().is_none_or(|c| c.key != key) {
                        self.class_canvas = Some(ClassCanvas::new(key, "class"));
                    }
                    let review_focus = if self.review_active { super::state::proposal_review::review_footprint(self.proposal_ui.review.as_ref(), self.selected_proposal.as_deref()) } else { None };
                    let stress = self.stress_sets();
                    let sync = self.sync_sets();
                    let level = snap.view.settings.detail_for("class");
                    let mut inp = self.canvas_input(&data, snap, Some(&snap.metrics), level, hovered.as_deref());
                    inp.review_focus = review_focus.as_ref();
                    inp.stress = stress.as_ref();
                    inp.sync = sync.as_ref();
                    inp.spotlight = self.spotlight.as_ref().map(|s| s.as_deref());
                    inp.saved = if self.package_scope.is_some() { None } else if self.preview.is_some() { self.preview.as_ref().and_then(|p| p.layout.get("class")) } else { data.saved.as_ref() };
                    // review: elements keep their place across before/after and alternatives
                    let review_saved;
                    let picked;
                    if self.review_active {
                        let mut m = data.saved.clone().unwrap_or_default();
                        m.extend(self.review_pos.iter().map(|(k, v)| (k.clone(), v.clone())));
                        review_saved = m;
                        inp.saved = Some(&review_saved);
                        inp.keep_layout = self.selected_proposal.as_ref().is_some_and(|p| self.review_seen.contains(p));
                        picked = self.proposal_ui.picked_classes();
                        inp.highlight = Some(&picked);
                    } else {
                        // the selection is drawn like review picks
                        picked = self.selection.iter().filter_map(|t| match t { Target::Class { name } => Some(name.clone()), _ => None }).collect();
                        if !picked.is_empty() {
                            inp.highlight = Some(&picked);
                        }
                    }
                    let c = self.class_canvas.as_mut().unwrap();
                    if c.pinned_focus.as_ref().is_some_and(|p| !data.classes.iter().any(|x| &x.name == p)) {
                        c.pinned_focus = None;
                    }
                    c.show(ui, stage, &self.measure, &inp, &mut canvas_out);
                    if self.review_active && self.proposal_ui.review.is_some() {
                        self.review_pos.extend(c.layout.positions());
                        if let Some(p) = self.selected_proposal.clone() {
                            self.review_seen.insert(p);
                        }
                        // deliberate navigation: fit the plan's changes once its layout is ready
                        if let (Some((id, since)), Some(fp)) = (self.review_locate.clone(), review_focus.as_ref()) {
                            let now = ui.input(|i| i.time);
                            let current = id.is_empty() || Some(&id) == self.selected_proposal.as_ref();
                            if current && !c.vp.fit_pending() && now - since > 0.25 && (!c.layout.active() || now - since > 2.5) {
                                c.locate(fp, Some(&snap.metrics), now);
                                self.review_locate = None;
                            } else if !current {
                                self.review_locate = None;
                            }
                            ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
                        }
                    }
                }
            }
            Tab::Domain => {
                if let Some(data) = self.domain_data() {
                    let key = format!("{}:domain", snap.project_dir);
                    if self.domain_canvas.as_ref().is_none_or(|c| c.key != key) {
                        self.domain_canvas = Some(ClassCanvas::new(key, "domain"));
                    }
                    let level = snap.view.settings.detail_for("domain");
                    let hov = hovered.as_deref().map(|k| if let Some(n) = k.strip_prefix("concept:") { format!("class:{n}") } else if let Some(n) = k.strip_prefix("concept_link:") { format!("relation:{n}") } else { k.to_string() });
                    let inp = self.canvas_input(&data, snap, None, level, hov.as_deref());
                    self.domain_canvas.as_mut().unwrap().show(ui, stage, &self.measure, &inp, &mut canvas_out);
                }
                self.views.domain_banner(ui, stage, snap, &mut view_out);
                // the canvas speaks in class terms: translate to the domain model's own targets
                canvas_out = canvas_out.into_iter().filter_map(domain_canvas_out).collect();
            }
            Tab::Package => self.views.package(ui, stage, snap, &self.measure, &mut view_out),
            Tab::System => self.views.system(ui, stage, snap, &self.measure, &mut view_out),
            Tab::Usecase => self.views.usecase(ui, stage, snap, &self.measure, &mut view_out),
            Tab::Sequence => {
                let sc = self.scenario.clone();
                self.views.sequence(ui, stage, snap, sc.as_deref(), &self.measure, &mut view_out)
            }
        }
        let now = ctx.input(|i| i.time);
        for o in canvas_out {
            match o {
                CanvasOut::Hover(k, p) => {
                    if self.prune.is_none() && self.context.is_none() {
                        self.hover.move_to(k.as_deref(), Point { x: p.x, y: p.y }, now)
                    }
                }
                CanvasOut::Prune(t, p) => self.do_prune(t, p),
                CanvasOut::Context(t, p) => self.open_context_menu(t, p),
                CanvasOut::Select(ts, add) => self.select_targets(ts, add),
                CanvasOut::Click(t, _) => self.on_click_target(t),
                CanvasOut::DoubleClick(t, p) => self.on_double_click(t, p),
                CanvasOut::MoveToPackage(c, pk) => {
                    let msg = format!("moved {c} to {pk} (⌘Z to undo)");
                    self.run_ok(move |s, _| crate::commands::apply_user_ops(s, vec![brief::op(json!({"op": "move_to_package", "class": c, "package": pk}))]), move |app| app.toast("info", msg));
                }
                CanvasOut::SaveLayout(pos) => {
                    if self.tab == Tab::Class && self.package_scope.is_some() { continue; }
                    let view = if self.tab == Tab::Domain { "domain" } else { "class" }.to_string();
                    self.run(move |s, _| crate::commands::set_layout(s, view, pos));
                }
            }
        }
        for o in view_out {
            match o {
                ViewOut::Hover(k, p) => {
                    if self.prune.is_none() && self.context.is_none() {
                        self.hover.move_to(k.as_deref(), Point { x: p.x, y: p.y }, now)
                    }
                }
                ViewOut::Prune(t, p) => self.do_prune(t, p),
                ViewOut::Context(t, p) => self.open_context_menu(t, p),
                ViewOut::Select(ts, add) => self.select_targets(ts, add),
                ViewOut::Click(t) => self.on_click_target(t),
                ViewOut::Regenerate(v, s) => {
                    let v = v.to_string();
                    self.run(move |st, pi| crate::commands::regenerate(st, pi, v, s));
                }
                ViewOut::SelectScenario(s) => self.scenario = s,
            }
        }
        // ---------------------------------------------------------- chrome
        let chrome_r = Rect::from_min_max(Pos2::new(r.left() + 10.0, r.top() + 8.0), Pos2::new(r.right() - 10.0, r.top() + 124.0));
        let view = &snap.view;
        let metrics = &snap.metrics;
        let mut action = None;
        ui.scope_builder(egui::UiBuilder::new().max_rect(chrome_r), |ui| {
            // identity row | actions
            ui.horizontal(|ui| {
                ui.set_height(26.0);
                ui.spacing_mut().item_spacing.x = 8.0;
                let actions_w = 300.0;
                ui.allocate_ui_with_layout(Vec2::new((chrome_r.width() - actions_w).max(80.0), 26.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    let name = ui.add(egui::Label::new(RichText::new(&view.name).font(bold(13.0)).color(css::TEXT)).sense(Sense::click()).truncate());
                    let name = name.on_hover_text(format!("{}\n{}\nRight-click for project actions", view.name, snap.project_dir));
                    name.context_menu(|ui| {
                        if ui.button("Rename Design…").on_hover_text("The design's title. The folder keeps its name.").clicked() {
                            self.palette = Some(Palette::new("name "));
                            ui.close();
                        }
                        if ui.button("Copy Project Path").clicked() {
                            ui.ctx().copy_text(snap.project_dir.clone());
                            ui.close();
                        }
                        if ui.button("Reveal in Finder").clicked() {
                            self.reveal_project();
                            ui.close();
                        }
                        ui.separator();
                        if ui.button("Close Project").clicked() {
                            ui.close();
                            self.close_project();
                        }
                    });
                    let (label, color) = match (&snap.workspace_status, snap.save_status.error.is_some()) {
                        (crate::state::WorkspaceStatus::ReadOnly, _) => ("read-only", css::WARN),
                        (_, true) => ("not saved", css::BAD),
                        _ => ("saved", css::FAINT),
                    };
                    widgets::pill(ui, label, color, css::LINE, Color32::TRANSPARENT, sans(11.0)).on_hover_text(&snap.design_path);
                    widgets::pill(ui, format!("⎇ {}", snap.branch), css::DIM, css::LINE, css::RAISED, super::theme::mono(11.0)).on_hover_text("active branch (/branch, /switch, /compare, /merge)");
                    if view.settings.partner != spide_core::model::PartnerMode::Examiner {
                        let l = brief::MODES.iter().find(|m| m.0 == view.settings.partner).map(|m| m.1).unwrap_or("");
                        widgets::pill(ui, l, css::ACCENT, Color32::from_rgba_unmultiplied(138, 169, 255, 102), css::ACCENT_SOFT, sans(11.0));
                    }
                    if self.preview.is_some() {
                        let t = if self.preview_source == PreviewSource::History { "Past revision · read-only" } else { "Option preview · read-only" };
                        widgets::pill(ui, t, css::GHOST, Color32::from_rgba_unmultiplied(94, 234, 212, 102), Color32::from_rgba_unmultiplied(94, 234, 212, 18), sans(11.0));
                    }
                    if self.lens == Lens::Sync {
                        widgets::pill(ui, "sync lens", css::GHOST, Color32::from_rgba_unmultiplied(94, 234, 212, 102), Color32::from_rgba_unmultiplied(94, 234, 212, 18), sans(11.0))
                            .on_hover_text("teal: design ahead · violet: code ahead · red: conflict · amber dashed: unreconciled · faded: only in the code · dots mark members · /lens off");
                    }
                    if self.lens == Lens::Overlap {
                        widgets::pill(ui, "overlap: dashed = duplicated behaviour · ⁂ = scattered concept", css::ACCENT, Color32::from_rgba_unmultiplied(196, 181, 253, 102), Color32::from_rgba_unmultiplied(196, 181, 253, 18), sans(11.0)).on_hover_text("/lens off");
                    }
                    if let Some(s) = &self.stress {
                        widgets::pill(ui, format!("what if: {s} · impacted · ripple"), css::TEXT, css::LINE, Color32::TRANSPARENT, sans(11.0));
                    }
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // right-to-left: draw in reverse
                    let mut a = None;
                    ui.allocate_ui_with_layout(Vec2::new(actions_w, 26.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            a = chrome::workspace_actions(ui, self.brief, self.timeline, self.palette.is_some(), self.review_active);
                        });
                    });
                    action = a;
                });
            });
        });
        // the view tabs, centred under the identity row
        let tabs_w = ui.ctx().data(|d| d.get_temp::<f32>(egui::Id::unique("tabs-width"))).unwrap_or(400.0);
        let tabs_at = Pos2::new(chrome_r.center().x - tabs_w / 2.0, chrome_r.top() + 34.0);
        let tabs_r = Rect::from_min_size(tabs_at, Vec2::new(tabs_w + 4.0, 34.0));
        let used = ui.scope_builder(egui::UiBuilder::new().max_rect(tabs_r).layout(egui::Layout::left_to_right(egui::Align::Min)), |ui| {
            let stale = |t: Tab| match t {
                Tab::Usecase => snap.derived.use_cases_stale,
                Tab::Sequence => !snap.derived.stale_sequences.is_empty(),
                _ => false,
            };
            if let Some(t) = chrome::tabs(ui, self.tab, &stale) {
                self.go_to_tab(t);
            }
        });
        let w = used.response.rect.width();
        if (w - tabs_w).abs() > 0.5 {
            ui.ctx().data_mut(|d| d.insert_temp(egui::Id::unique("tabs-width"), w));
            ui.ctx().request_repaint();
        }
        match action {
            Some(chrome::Action::Brief) => self.brief = !self.brief,
            Some(chrome::Action::Timeline) => {
                self.tab = Tab::Class;
                self.timeline = !self.timeline;
                if !self.timeline {
                    self.close_timeline();
                }
            }
            Some(chrome::Action::Find) => self.palette = Some(Palette::new("find ")),
            Some(chrome::Action::Commands) => self.palette = if self.palette.is_some() { None } else { Some(Palette::new("")) },
            None => {}
        }
        // toolbar
        let tb = Rect::from_min_size(Pos2::new(r.left() + 10.0, r.top() + 82.0), Vec2::new(r.width() - 20.0, 36.0));
        let mut health_actions = vec![];
        ui.scope_builder(egui::UiBuilder::new().max_rect(tb), |ui| {
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                chrome::evidence_button(ui, &mut self.health, metrics, &mut health_actions);
                if matches!(self.tab, Tab::Class | Tab::Domain) {
                    let level = view.settings.detail_for(self.tab.key());
                    if let Some(l) = chrome::detail_slider(ui, level, self.preview.is_some()) {
                        let v = self.tab.key().to_string();
                        self.run(move |s, _| crate::commands::set_detail(s, v, l));
                    }
                }
                if self.tab == Tab::Class {
                    let mut scope = self.package_scope.clone();
                    egui::ComboBox::from_id_salt("package-scope").width(155.0)
                        .selected_text(scope.as_deref().map(|p| format!("{p} + boundary")).unwrap_or_else(|| "All packages".into()))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut scope, None, "All packages");
                            for p in view.packages.iter().filter(|p| p.status.is_live()) {
                                ui.selectable_value(&mut scope, Some(p.name.clone()), format!("{} + boundary", p.name));
                            }
                        }).response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, "Package scope"));
                    if scope != self.package_scope { self.set_package_scope(scope); }
                }
                match chrome::zoom_group(ui) {
                    Some(chrome::Zoom::Out) => self.zoom_active(0.8),
                    Some(chrome::Zoom::In) => self.zoom_active(1.25),
                    Some(chrome::Zoom::Fit) => self.fit_active(),
                    None => {}
                }
                if let Some(z) = self.active_zoom() {
                    let b = egui::Button::new(RichText::new(format!("{:.0}%", z * 100.0)).font(super::theme::mono(11.5)).color(css::DIM)).frame(false).min_size(Vec2::new(44.0, 30.0));
                    if ui.add(b).on_hover_text("Zoom level · click for actual size (⌘0)").clicked() {
                        self.zoom_reset();
                    }
                }
                if self.preview.is_some() {
                    let b = egui::Button::new(RichText::new("Exit preview").font(sans(12.0)).color(css::GHOST)).fill(Color32::from_rgba_unmultiplied(94, 234, 212, 20)).stroke(Stroke::new(1.0, Color32::from_rgba_unmultiplied(94, 234, 212, 102))).corner_radius(15).min_size(Vec2::new(0.0, 30.0));
                    if ui.add(b).clicked() {
                        let hist = self.preview_source == PreviewSource::History;
                        self.set_preview(None, self.preview_source);
                        self.decision_ui.previewing = None;
                        if hist {
                            self.timeline = false;
                        }
                    }
                }
            });
        });
        for a in health_actions {
            match a {
                HealthAction::Fix(i) => self.fix_issue(i),
                HealthAction::Focus(i) => self.focus_issue(i),
                HealthAction::FocusTarget(target) => self.reveal(&target),
                HealthAction::Exception(id, approve) => {
                    let tool = if approve { "approve_exception" } else { "reject_exception" };
                    self.backend.call(
                        move |st, _| crate::service::get(st).ok_or("service not ready".to_string())?.call_tool(tool, &json!({"id": id}), "user"),
                        |app, r| if let Err(e) = r { app.toast("error", e) },
                    );
                }
            }
        }
        // concerns lens legend
        if self.lens == Lens::Concerns {
            egui::Area::new(egui::Id::unique("lens-legend")).order(egui::Order::Middle).pivot(egui::Align2::LEFT_BOTTOM).fixed_pos(Pos2::new(r.left() + 16.0, r.bottom() - 16.0)).show(&ctx, |ui| {
                egui::Frame::new().fill(Color32::from_rgba_unmultiplied(94, 234, 212, 18)).stroke(Stroke::new(1.0, Color32::from_rgba_unmultiplied(94, 234, 212, 102))).corner_radius(10).inner_margin(Margin::symmetric(8, 4)).show(ui, |ui| {
                    ui.set_max_width(300.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new("what methods do:").font(sans(11.0)).color(css::GHOST));
                        for k in spide_diagram::insights::CONCERNS {
                            let (rr, _) = ui.allocate_exact_size(Vec2::splat(8.0), Sense::hover());
                            ui.painter().rect_filled(rr, 2.0, super::theme::col(spide_diagram::insights::concern_color(k).unwrap(), 1.0));
                            ui.label(RichText::new(k.replacen('_', " ", 1)).font(sans(11.0)).color(css::GHOST));
                        }
                    });
                });
            });
        }
        // welcome
        if view.classes.is_empty() && self.tab == Tab::Class {
            let brief_open = view.forces.is_empty() && !view.settings.brief_skipped;
            match chrome::welcome(ui, r, brief_open) {
                Some(chrome::WelcomeAction::Brief) => self.brief = true,
                Some(chrome::WelcomeAction::Skip) => self.command("/skip"),
                Some(chrome::WelcomeAction::Open) => self.open_project_dialog(),
                Some(chrome::WelcomeAction::FocusPi) => {
                    self.workspace.left_open = true;
                    if let Some(t) = self.term.as_mut() {
                        t.focus_requested = true;
                    }
                }
                None => {}
            }
        }
        // side panels
        if self.brief {
            match brief::panel(&ctx, r, &mut self.brief_ui, view, metrics, self.stress.as_deref(), &self.backend) {
                Some(BriefOut::Close) => self.brief = false,
                Some(BriefOut::Stress(s)) => {
                    let set = s.is_some();
                    self.stress = s;
                    if set {
                        self.tab = Tab::Class;
                    }
                }
                None => {}
            }
        }
        // compare
        if let Some(id) = self.compare.clone() {
            match view.decisions.iter().find(|x| x.id == id && x.is_pending()) {
                Some(d) => {
                    let cols = decisions::decision_cols(d, snap);
                    if decisions::compare(&ctx, r, &mut self.compare_ui, "compare options", &d.question, &cols, snap, &self.measure) {
                        self.compare = None;
                    }
                }
                None => self.compare = None,
            }
        }
        if let Some(b) = self.branch_cmp.take() {
            let title = format!("{} ↔ {}", snap.branch, b.name);
            if !decisions::compare(&ctx, r, &mut self.compare_ui, "compare branches", &title, &b.cols, snap, &self.measure) {
                self.branch_cmp = Some(b);
            }
        }
        // review bar (before/after, fit, legend) or the proposal dock, and the timeline
        if self.review_active && self.tab == Tab::Class && self.preview.is_none() && !view.proposals.is_empty() {
            let bottom = if self.timeline { 104.0 } else { 16.0 };
            let mut act = None;
            egui::Area::new(egui::Id::unique("review-bar")).order(egui::Order::Middle).pivot(egui::Align2::CENTER_BOTTOM).fixed_pos(Pos2::new(r.center().x, r.bottom() - bottom)).show(&ctx, |ui| {
                act = chrome::review_bar(ui, self.proposal_ui.baseline, self.proposal_ui.review.is_some(), r.width() - 40.0);
            });
            match act {
                Some(chrome::ReviewBar::Before(b)) => {
                    self.proposal_ui.baseline = b;
                    self.refresh_proposal_canvas();
                }
                Some(chrome::ReviewBar::Fit) => {
                    let fp = super::state::proposal_review::review_footprint(self.proposal_ui.review.as_ref(), self.selected_proposal.as_deref());
                    let now = self.now();
                    let located = match (fp, self.class_canvas.as_mut()) {
                        (Some(fp), Some(c)) => {
                            c.focus(None, now);
                            c.locate(&fp, Some(metrics), now)
                        }
                        _ => false,
                    };
                    if !located {
                        self.fit_active();
                    }
                }
                None => {}
            }
        } else if !view.proposals.is_empty() {
            let bottom = if self.timeline { 104.0 } else { 16.0 };
            egui::Area::new(egui::Id::unique("proposal-dock")).order(egui::Order::Middle).pivot(egui::Align2::CENTER_BOTTOM).fixed_pos(Pos2::new(r.center().x, r.bottom() - bottom)).show(&ctx, |ui| {
                if chrome::proposal_bar(ui, view.proposals.len()) {
                    self.open_proposal_review(None);
                }
            });
        }
        if self.timeline {
            timeline::reload(&mut self.timeline_ui, &self.backend, snap.rev);
            let tl = Rect::from_min_max(Pos2::new(r.left() + 10.0, r.bottom() - 100.0), Pos2::new(r.right() - 10.0, r.bottom() - 12.0));
            let mut out = None;
            egui::Area::new(egui::Id::unique("timeline")).order(egui::Order::Middle).pivot(egui::Align2::LEFT_BOTTOM).fixed_pos(Pos2::new(tl.left(), r.bottom() - 12.0)).show(&ctx, |ui| {
                ui.set_width(tl.width());
                out = timeline::show(ui, &mut self.timeline_ui, &self.backend);
            });
            match out {
                Some(TimelineOut::Close) => self.close_timeline(),
                Some(TimelineOut::Preview(None)) => {
                    if self.preview_source == PreviewSource::History {
                        self.set_preview(None, PreviewSource::History);
                    }
                }
                Some(TimelineOut::Preview(Some(rev))) => {
                    self.backend.run_then(move |s, _| crate::commands::history_at(s, rev), move |app, d| {
                        if app.timeline && app.timeline_ui.shown == Some(rev) {
                            app.set_preview(Some(d), PreviewSource::History);
                        }
                    });
                }
                None => {}
            }
        } else if self.timeline_ui.shown.is_some() {
            self.timeline_ui.shown = None;
        }
        // workspace palette (⌘K)
        if let Some(p) = self.palette.as_mut() {
            let w = (r.width() - 20.0).min(560.0);
            let mut act = None;
            egui::Area::new(egui::Id::unique("workspace-palette")).order(egui::Order::Foreground).fixed_pos(Pos2::new(r.right() - 10.0 - w, r.top() + 40.0)).show(&ctx, |ui| {
                ui.set_width(w);
                act = overlays::palette(ui, p, &self.prefs.favorite_commands);
            });
            match act {
                Some(PaletteAction::ToggleFavorite(c)) => self.toggle_favorite_command(c),
                Some(PaletteAction::Close) => self.palette = None,
                Some(PaletteAction::Run(c)) => {
                    self.palette = None;
                    self.command(&format!("/{c}"));
                }
                None => {}
            }
        }
        // hover card
        if self.prune.is_none() {
            if let Some(h) = self.hover.snapshot().cloned() {
                let design: &Design = if self.review_active { self.proposal_canvas.as_ref().unwrap_or(view) } else { self.preview.as_deref().unwrap_or(view) };
                let cx = CardCtx { snap, design, metrics };
                let mut actions = vec![];
                let shown = hover_card::show(&ctx, &h.key, h.at, h.pinned, stage, &cx, &self.card_selected, &mut actions);
                if shown.is_none() {
                    self.hover.clear();
                }
                for a in actions {
                    match a {
                        CardAction::Close => self.hover.clear(),
                        CardAction::Select(s) => self.card_selected = s,
                    }
                }
                if h.pinned && ctx.input(|i| i.key_pressed(Key::Escape)) {
                    self.hover.clear();
                }
            } else {
                self.card_selected = None;
            }
        }
        // prune feedback
        if let Some(p) = self.prune.as_mut() {
            match overlays::feedback(&ctx, p) {
                Some(PruneAction::Close) => self.prune = None,
                Some(PruneAction::Undo) => {
                    self.prune = None;
                    self.run(|s, _| crate::commands::undo(s));
                }
                Some(PruneAction::Send(text)) => {
                    let targets = p.targets.clone();
                    self.prune = None;
                    let tab = self.tab.key().to_string();
                    self.run(move |s, pi| crate::commands::prune_feedback(s, pi, targets, text, tab));
                }
                None => {}
            }
        }
        // inline edit
        if let Some(e) = self.inline.as_mut() {
            match overlays::inline_edit(&ctx, e) {
                Some(InlineAction::Submit(v)) => {
                    let k = e.kind.clone();
                    self.inline = None;
                    self.submit_inline(k, v);
                }
                Some(InlineAction::Close) => self.inline = None,
                None => {}
            }
        }
        // export dialog
        if self.export.is_some() {
            let current = self.current_export_view();
            let mut dlg = self.export.take().unwrap();
            let views = self.export_views();
            // a running batch writes one file per frame: progress stays visible and Stop works
            if let Some(run) = dlg.run.as_mut().filter(|r| !r.done()) {
                match run.phase {
                    BatchPhase::Writing => self.batch_step(run),
                    BatchPhase::Regenerating { .. } => self.batch_regen_tick(run, &dlg.settings.batch),
                    BatchPhase::Stuck { .. } => {}
                }
                ctx.request_repaint();
            }
            let out = export::dialog(&ctx, &mut dlg, &current, &views, &mut |o| self.export_svg(o));
            let mut keep = true;
            match out {
                Some(DialogOut::Close) => keep = false,
                Some(DialogOut::PackageScope(scope)) => {
                    self.set_package_scope(scope);
                    dlg.preview_key.clear();
                    dlg.preview = None;
                }
                Some(DialogOut::BatchCancel) => {
                    if let Some(r) = dlg.run.as_mut() {
                        r.cancelled = true;
                    }
                }
                Some(DialogOut::BatchStart) => {
                    let profiles = export::effective_profiles(&dlg.settings);
                    let jobs = export_jobs::jobs(&views, &dlg.settings.batch, &profiles);
                    if let Some(parent) = rfd::FileDialog::new().set_title("Folder for the exported diagrams").set_directory(&snap.project_dir).pick_folder() {
                        let stamp = chrono_stamp();
                        match export_jobs::output_dir(&parent, &snap.view.name, &stamp) {
                            Ok(dir) => {
                                let mut run = BatchRun::new(dir, jobs, profiles, snap.rev);
                                let stale = export_jobs::stale_views(&views, &dlg.settings.batch.excluded, &snap.derived);
                                if !stale.is_empty() {
                                    run.phase = self.regenerate_for_export(stale);
                                }
                                dlg.run = Some(run);
                            }
                            Err(e) => self.toast("error", format!("export folder: {e}")),
                        }
                    }
                    self.export_settings.insert(snap.project_dir.clone(), dlg.settings.clone());
                }
                Some(DialogOut::BatchExportAnyway) => {
                    if let Some(mut run) = dlg.run.take() {
                        self.batch_start_writing(&mut run, &dlg.settings.batch);
                        dlg.run = Some(run);
                    }
                }
                Some(DialogOut::Changed) => {
                    self.export_settings.insert(snap.project_dir.clone(), dlg.settings.clone());
                }
                Some(DialogOut::Reveal(p)) => {
                    let _ = crate::commands::reveal_path(p);
                }
                Some(DialogOut::PushLucid { all }) => {
                    let key = if all { None } else { self.lucid_key() };
                    let text = match key {
                        Some(k) => format!("Run spide_lucid with action \"push\" for the {k} diagram and report the result briefly."),
                        None => "Run spide_lucid with action \"push\" for all diagrams and report the result briefly.".to_string(),
                    };
                    self.backend.run(move |s, pi| crate::commands::send_prompt(s, pi, text, "class".into()));
                    self.toast("info", "Asked the agent to push to Lucid; follow it in the terminal.");
                    keep = false;
                }
                Some(DialogOut::Export) => {
                    dlg.state = ExportState::Busy;
                    let s = dlg.settings.clone();
                    match self.run_export(&s, None) {
                        Ok(Some(p)) => {
                            dlg.settings.last_path = Some(p.clone());
                            self.export_settings.insert(snap.project_dir.clone(), dlg.settings.clone());
                            self.toast("info", format!("exported {p}"));
                            dlg.state = ExportState::Done(p);
                        }
                        Ok(None) => dlg.state = ExportState::Idle,
                        Err(e) => dlg.state = ExportState::Error(e),
                    }
                }
                None => {}
            }
            if keep {
                self.export = Some(dlg);
            }
        }
        if !self.snap.as_ref().is_some_and(|s| Arc::ptr_eq(s, snap)) {
            ctx.request_repaint();
        }
    }
}

/// Resolved decisions, newest first; returns the one clicked.
fn history_list(ui: &mut egui::Ui, snap: &Snapshot) -> Option<String> {
    let mut out = None;
    egui::ScrollArea::vertical().id_salt("history-list").auto_shrink([false, false]).show(ui, |ui| {
        egui::Frame::new().inner_margin(Margin::symmetric(12, 6)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let resolved: Vec<_> = snap.view.decisions.iter().filter(|d| !d.is_pending()).rev().collect();
            if resolved.is_empty() {
                widgets::para(ui, "No resolved decisions yet. Approved and dismissed changes appear here with their consequences.", 12.5, css::DIM);
            }
            for d in resolved {
                let (state, c) = match d.state {
                    spide_core::model::DecisionState::Accepted => ("approved", css::GOOD),
                    spide_core::model::DecisionState::Overruled => ("overruled", css::WARN),
                    _ => ("dismissed", css::FAINT),
                };
                let r = egui::Frame::new().inner_margin(Margin::symmetric(8, 6)).corner_radius(6).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.add(egui::Label::new(RichText::new(&d.question).font(sans(12.5)).color(css::TEXT)).wrap());
                    ui.label(RichText::new(state).font(sans(11.0)).color(c));
                });
                let resp = ui.interact(r.response.rect, ui.scope_id().with(("history", &d.id)), Sense::click());
                if resp.hovered() {
                    ui.painter().rect_stroke(r.response.rect, 6.0, Stroke::new(1.0, css::LINE_STRONG), egui::StrokeKind::Inside);
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if resp.clicked() {
                    out = Some(d.id.clone());
                }
            }
        });
    });
    out
}

/// A collapsed pane's vertical rail button.
fn rail(ui: &mut egui::Ui, r: Rect, label: &str) -> egui::Response {
    let br = Rect::from_min_size(r.min, Vec2::new(30.0, r.height().min(160.0)));
    let resp = ui.interact(br, ui.scope_id().with(("rail", label)), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(br, 0.0, css::RAISED);
    }
    let g = ui.painter().layout_no_wrap(label.to_string(), sans(11.5), if resp.hovered() { css::TEXT } else { css::DIM });
    let pos = Pos2::new(br.center().x + g.size().y / 2.0, br.top() + 8.0);
    ui.painter().add(egui::epaint::TextShape::new(pos, g, css::DIM).with_angle(std::f32::consts::FRAC_PI_2));
    resp
}

#[cfg(test)]
mod export_tests {
    use super::*;
    use crate::ui::tests::{harness, project, steps, shot};

    #[test]
    fn package_scope_keeps_full_layout_and_evidence() {
        let dir = project("rest-extraction");
        let mut h = harness(dir.path(), (1440.0, 900.0));
        steps(&mut h, 20);
        let snap = h.state().snap.clone().unwrap();
        let package = snap.view.packages.iter().find(|p| p.status.is_live() && snap.view.classes.iter().any(|c| c.package.as_deref() == Some(&p.name))).unwrap().name.clone();
        let expected = spide_core::scope::PackageScope::new(&snap.view, &snap.metrics, &package, true).unwrap();
        let full_positions = h.state().class_canvas.as_ref().map(|c| c.layout.positions());
        h.state_mut().set_package_scope(Some(package.clone()));
        steps(&mut h, 10);
        let data = h.state_mut().class_data().unwrap();
        assert_eq!(data.classes.iter().map(|c| c.name.clone()).collect::<std::collections::BTreeSet<_>>(), expected.classes);
        assert!(data.saved.is_none());
        shot(&mut h, "package-boundary");
        assert_eq!(h.state().snap.as_ref().unwrap().metrics.health_report, snap.metrics.health_report);
        assert_eq!(h.state().snap.as_ref().unwrap().view.layout, snap.view.layout);
        let v = ExportView::ClassPackage(package.clone());
        let options = ExportOptions { ghosts: true, ..Default::default() };
        let canvas = h.state_mut().export_canvas_for(&v, Some(1), &options).unwrap();
        let canvas: serde_json::Value = serde_json::from_str(&canvas).unwrap();
        let ids: std::collections::BTreeSet<String> = canvas["nodes"].as_array().unwrap().iter().filter_map(|n| n["id"].as_str()?.strip_prefix("class:").map(String::from)).collect();
        assert_eq!(ids, expected.classes);
        let puml = h.state().export_plantuml_for(&v).unwrap();
        for c in snap.view.classes.iter().filter(|c| !expected.classes.contains(&c.name)) {
            assert!(!puml.contains(&format!("class {} {{", c.name)), "hidden class {}", c.name);
        }
        let full = h.state_mut().export_class_data(&ExportView::Class, true).unwrap();
        assert_eq!(full.classes.len(), snap.view.classes.len());
        assert_eq!(h.state().package_scope.as_deref(), Some(package.as_str()));
        assert!(h.state_mut().export_svg_for(&v, Some(1), &options).is_some());
        h.state_mut().set_package_scope(None);
        assert_eq!(h.state().class_canvas.as_ref().map(|c| c.layout.positions()), full_positions);
        assert_eq!(h.state_mut().class_data().unwrap().classes.len(), snap.view.classes.len());
    }

    #[test]
    fn export_dialog_and_real_files() {
        let dir = project("library");
        let mut h = harness(dir.path(), (1440.0, 900.0));
        steps(&mut h, 150);
        h.state_mut().open_export();
        steps(&mut h, 10);
        assert!(h.state().export.is_some());
        shot(&mut h, "export-dialog");
        h.state_mut().export.as_mut().unwrap().batch_mode = true;
        steps(&mut h, 10);
        shot(&mut h, "export-batch");
        h.state_mut().export = None;
        for (format, ext, magic) in [
            (Format::Svg, "svg", b"<svg".as_slice()),
            (Format::Png, "png", b"\x89PNG".as_slice()),
            (Format::Pdf, "pdf", b"%PDF".as_slice()),
            (Format::Puml, "puml", b"@startuml".as_slice()),
            (Format::Canvas, "canvas", b"\"nodes\"".as_slice()),
        ] {
            let path = dir.path().join(format!("test.{ext}"));
            let settings = ExportSettings { format, ..Default::default() };
            h.state_mut().run_export(&settings, Some(path.to_string_lossy().into())).unwrap();
            let data = std::fs::read(&path).unwrap();
            assert!(data.len() > 100);
            assert!(data.windows(magic.len()).any(|w| w == magic), "{ext} signature");
        }
        // PNGs declare their resolution: 2× is 144 dpi, so viewers show it at 1× size, sharp
        let png = std::fs::read(dir.path().join("test.png")).unwrap();
        let at = png.windows(4).position(|w| w == b"pHYs").expect("pHYs chunk") + 4;
        assert_eq!(&png[at..at + 4], 5669u32.to_be_bytes().as_slice());
        // the canvas: valid JSON Canvas, one card per class, no diagnostics, edges with UML labels
        let canvas: serde_json::Value = serde_json::from_slice(&std::fs::read(dir.path().join("test.canvas")).unwrap()).unwrap();
        let nodes = canvas["nodes"].as_array().unwrap();
        let classes = h.state().snap.as_ref().unwrap().view.classes.len();
        assert_eq!(nodes.iter().filter(|n| n["id"].as_str().unwrap().starts_with("class:")).count(), classes);
        assert!(nodes.iter().any(|n| n["type"] == "group" && n["color"].is_string()));
        assert!(!canvas.to_string().contains("rule ✗") && !canvas.to_string().contains("\"cycle"));
        assert!(canvas["edges"].as_array().unwrap().iter().any(|e| e["label"].as_str().is_some_and(|l| l.starts_with("◆ composed of"))));
        // the dialog previews a canvas (same layout)
        h.state_mut().open_export();
        h.state_mut().export.as_mut().unwrap().settings.format = Format::Canvas;
        steps(&mut h, 20);
        assert!(h.state().export.as_ref().unwrap().preview.is_some());
        shot(&mut h, "export-canvas");
        h.state_mut().export = None;
    }

    #[test]
    fn batch_regenerates_out_of_date_views_or_lets_the_user_decide() {
        let dir = project("rest-extraction");
        let mut h = harness(dir.path(), (1440.0, 900.0));
        steps(&mut h, 30);
        let snap = h.state().snap.clone().unwrap();
        let views = h.state().export_views();
        assert!(views.contains(&ExportView::Domain), "missing domain model is still offered: {views:?}");
        let stale = export_jobs::stale_views(&views, &[], &snap.derived);
        assert!(stale.contains(&ExportView::Domain), "{stale:?}");
        let batch = BatchSettings { levels: vec![1], formats: vec![Format::Svg], ..Default::default() };
        let profiles = vec![export_jobs::Profile::default()];
        let out = tempfile::tempdir().unwrap();
        let mut run = BatchRun::new(out.path().to_path_buf(), export_jobs::jobs(&views, &batch, &profiles), profiles, snap.rev);
        // pi isn't running in tests: the batch can't regenerate and waits for the user's choice
        run.phase = h.state_mut().regenerate_for_export(stale);
        assert!(matches!(run.phase, BatchPhase::Stuck { .. }), "{:?}", run.phase);
        assert!(!run.done(), "nothing is written before the user decides");
        // a regenerating batch whose agent went offline gets stuck too
        run.phase = BatchPhase::Regenerating { pending: vec![ExportView::Domain], since: 0.0, saw_busy: false };
        h.state().backend.st.lock().pi_status = PiStatus::Offline;
        h.state().backend.st.emit();
        steps(&mut h, 2);
        h.state_mut().batch_regen_tick(&mut run, &batch);
        assert!(matches!(&run.phase, BatchPhase::Stuck { reason, .. } if reason.contains("pi isn't running")), "{:?}", run.phase);
        // Export anyway writes what exists; the missing domain model is skipped, not drawn empty
        h.state_mut().batch_start_writing(&mut run, &batch);
        while !run.done() {
            h.state_mut().batch_step(&mut run);
        }
        assert!(run.results.iter().any(|(f, r)| f.starts_with("domain") && matches!(r, JobResult::Skipped(_))), "{:?}", run.results);
        assert!(run.results.iter().any(|(f, r)| f.starts_with("class") && matches!(r, JobResult::Written(_))));
        // once the agent has delivered and pi is idle, the batch starts writing by itself
        {
            let st = h.state().backend.st.clone();
            st.lock().store.set_domain_model(serde_json::from_value(json!({"concepts": [{"name": "Endpoint"}]})).unwrap()).unwrap();
            st.lock().pi_status = PiStatus::Idle;
            st.changed();
        }
        steps(&mut h, 2);
        let mut run = BatchRun::new(out.path().to_path_buf(), vec![], vec![export_jobs::Profile::default()], 0);
        run.phase = BatchPhase::Regenerating { pending: vec![ExportView::Domain], since: 0.0, saw_busy: true };
        h.state_mut().batch_regen_tick(&mut run, &batch);
        assert_eq!(run.phase, BatchPhase::Writing);
        assert!(!run.jobs.is_empty() && run.rev == h.state().snap.as_ref().unwrap().rev);
    }

    #[test]
    fn batch_exports_every_diagram_level_and_format_without_touching_the_canvas() {
        let dir = project("library");
        let mut h = harness(dir.path(), (1440.0, 900.0));
        steps(&mut h, 150);
        let tab = h.state().tab;
        let detail = h.state().snap.as_ref().unwrap().view.settings.detail_for("class");
        let before = h.state().class_canvas.as_ref().map(|c| c.layout.positions());
        let views = h.state().export_views();
        assert!(views.contains(&ExportView::Class) && views.contains(&ExportView::Domain), "{views:?}");
        let batch = BatchSettings { levels: vec![0, 1, 2, 3, 4], formats: vec![Format::Svg, Format::Png, Format::Pdf, Format::Canvas], plantuml: true, ..Default::default() };
        let profiles = vec![export_jobs::Profile::default(), export_jobs::Profile { name: "dark".into(), options: ExportOptions { theme: "dark".into(), diagnostics: true, ..Default::default() }, scale: 1 }];
        let jobs = export_jobs::jobs(&views, &batch, &profiles);
        let out = tempfile::tempdir().unwrap();
        let rev = h.state().snap.as_ref().unwrap().rev;
        let mut run = BatchRun::new(out.path().to_path_buf(), jobs.clone(), profiles, rev);
        while !run.done() {
            h.state_mut().batch_step(&mut run);
        }
        let (written, skipped, failed) = run.counts();
        assert_eq!(failed, 0, "{:?}", run.results);
        assert_eq!(written + skipped, jobs.len());
        assert!(written >= views.len() * 3, "{:?}", run.results);
        for j in &jobs {
            if let Some((_, export_jobs::JobResult::Written(p))) = run.results.iter().find(|r| r.0 == j.file) {
                let data = std::fs::read(p).unwrap();
                assert!(data.len() > 50, "{}", j.file);
            }
        }
        // the class diagram at every detail level differs (different boxes, same pipeline)
        let a = std::fs::read_to_string(out.path().join("class-1-names-current.svg")).unwrap();
        let b = std::fs::read_to_string(out.path().join("class-4-full-current.svg")).unwrap();
        assert_ne!(a, b);
        // canvases too, valid JSON at every level
        for l in ["0-packages", "1-names", "4-full"] {
            let t = std::fs::read_to_string(out.path().join(format!("class-{l}-current.canvas"))).unwrap();
            let v: serde_json::Value = serde_json::from_str(&t).unwrap();
            assert!(!v["nodes"].as_array().unwrap().is_empty(), "{l}");
        }
        // nothing on screen changed: tab, saved detail level, canvas layout
        assert_eq!(h.state().tab, tab);
        assert_eq!(h.state().snap.as_ref().unwrap().view.settings.detail_for("class"), detail);
        if let (Some(b), Some(c)) = (before, h.state().class_canvas.as_ref()) {
            assert_eq!(b, c.layout.positions());
        }
        // the live level's export equals the single export (same scene)
        let o = ExportOptions::default();
        let single = h.state_mut().export_svg_for(&ExportView::Class, None, &o).unwrap().0;
        let batch_one = h.state_mut().export_svg_for(&ExportView::Class, Some(detail), &o).unwrap().0;
        assert_eq!(single, batch_one);
        // cancelling stops between jobs and keeps finished files
        let mut run2 = BatchRun::new(tempfile::tempdir().unwrap().keep(), jobs, vec![export_jobs::Profile::default()], rev);
        h.state_mut().batch_step(&mut run2);
        run2.cancelled = true;
        assert!(run2.done());
        assert_eq!(run2.results.len(), 1);
    }
}

impl eframe::App for SpideApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // closing the window (red button, ⌘W, Quit) goes through the common leave guard
        if ui.ctx().input(|i| i.viewport().close_requested()) {
            if self.close_confirmed {
                self.close_confirmed = false;
            } else {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::CancelClose);
                if self.confirm_leave(super::leave::LeaveReason::CloseWindow) {
                    self.close_confirmed = true;
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
        self.frame(ui);
    }
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.remember_workspace();
        let _ = self.save_input_drafts();
        self.save_prefs();
        eframe::set_value(storage, "spide.workspace.v1", &self.workspace);
        eframe::set_value(storage, "spide.export.v1", &self.export_settings);
    }
}
