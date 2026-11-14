//! The application: UI state, the frame loop, keyboard shortcuts and the `/spide` command dispatcher
//! (port of `App.tsx`).

use super::brief::{self, BriefOut, BriefUi};
use super::canvas::{CanvasInput, CanvasOut, ClassCanvas};
use super::chrome::{self, HealthAction, HealthState, Tab, Toast, TAB_ORDER};
use super::decisions::{self, CompareCol, CompareUi, DecisionUi};
use super::export::{self, DialogOut, ExportDialog, ExportSettings, ExportState, Format};
use super::hover_card::{self, CardAction, CardCtx};
use super::overlays::{self, InlineAction, InlineEdit, InlineKind, Palette, PaletteAction, PruneAction, PruneState};
use super::paint::EguiMeasure;
use super::proposal::{self, ProposalOut, ProposalUi};
use super::state::hover::{HoverStore, Point};
use super::state::proposal_review::PlanDrafts;
use super::terminal::{self, PaneAction, TermSession};
use super::theme::{bold, css, sans};
use super::timeline::{self, DriftOut, TimelineOut, TimelineUi};
use super::views::{ViewOut, Views};
use super::widgets;
use crate::backend::Backend;
use crate::commands::DriftReport;
use crate::events::{MenuAction, UiEvent};
use crate::state::Snapshot;
use egui::{Color32, Key, Margin, Pos2, Rect, RichText, Sense, Stroke, Vec2};
use serde_json::json;
use spide_core::model::{Class, Design, DecisionState, Package, Relation, Status};
use spide_core::Target;
use spide_diagram::class_view::{DriftSets, Lens, Stress};
use spide_diagram::svg::{build_export_svg, ExportMeta, ExportOptions, LegendKey, LEGEND_ORDER};
use spide_diagram::visuals::DETAIL_LABELS;
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::sync::mpsc::Receiver;
use std::sync::Arc;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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

/// The class-diagram data shown right now (live, preview, review or with code-only phantoms).
pub struct ClassData {
    pub key: u64,
    pub classes: Vec<Class>,
    pub relations: Vec<Relation>,
    pub packages: Vec<Package>,
    pub implicit: Vec<spide_core::metrics::ImplicitEdge>,
    pub saved: Option<std::collections::BTreeMap<String, spide_core::model::Pos>>,
}

type Deferred = Box<dyn FnOnce(&mut SpideApp)>;

pub struct SpideApp {
    ctx: egui::Context,
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
    term_confirm: bool,
    restarting: bool,
    pane_palette: Option<Palette>,
    pub palette: Option<Palette>,
    pub prune: Option<PruneState>,
    pub inline: Option<InlineEdit>,
    pub code_view: Option<(String, String)>,
    pub brief: bool,
    pub brief_ui: BriefUi,
    pub compare: Option<String>,
    compare_ui: CompareUi,
    pub branch_cmp: Option<BranchCmp>,
    pub stress: Option<String>,
    pub review_active: bool,
    pub source_open: bool,
    pub source_ui: super::source::SourceUi,
    pub selected_proposal: Option<String>,
    pub proposal_canvas: Option<Design>,
    pub proposal_ui: ProposalUi,
    pub shown_decision: Option<String>,
    pub timeline: bool,
    pub timeline_ui: TimelineUi,
    pub drift: Option<DriftReport>,
    drift_rev: Option<u64>,
    pub preview: Option<Arc<Design>>,
    pub preview_source: PreviewSource,
    preview_gen: u64,
    pub scenario: Option<String>,
    pub show_implicit: bool,
    pub lens: Lens,
    pub decision_ui: DecisionUi,
    pub class_canvas: Option<ClassCanvas>,
    pub domain_canvas: Option<ClassCanvas>,
    pub views: Views,
    class_data: Option<Arc<ClassData>>,
    domain_data: Option<Arc<ClassData>>,
    /// ghost spotlight from the proposal bar: Some(None) = all ghosts
    pub spotlight: Option<Option<String>>,
    pub export: Option<ExportDialog>,
    pub export_settings: HashMap<String, ExportSettings>,
    seen_commands: VecDeque<String>,
    deferred: Vec<(f64, Deferred)>,
    canvas_rect: Rect,
    last_tab: Tab,
    last_rev: Option<(u64, String)>,
    pub exit_requested: bool,
}

fn hash_of(v: impl std::hash::Hash) -> u64 {
    use std::hash::Hasher;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    v.hash(&mut h);
    h.finish()
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
        _ => None,
    };
    st == Some(Status::Pruned)
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
        app
    }

    pub fn with_ctx(ctx: &egui::Context, backend: Backend, rx: Receiver<UiEvent>, terminal: bool) -> Self {
        super::theme::install_fonts(ctx);
        super::theme::install_style(ctx);
        let snap = Some(Arc::new(backend.st.snapshot()));
        let term = terminal.then(|| TermSession::start(backend.pi.clone(), backend.events.clone()));
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
            term_confirm: false,
            restarting: false,
            pane_palette: None,
            palette: None,
            prune: None,
            inline: None,
            code_view: None,
            brief: false,
            brief_ui: BriefUi::default(),
            compare: None,
            compare_ui: CompareUi::default(),
            branch_cmp: None,
            stress: None,
            review_active: false,
            source_open: false,
            source_ui: Default::default(),
            selected_proposal: None,
            proposal_canvas: None,
            proposal_ui: ProposalUi { drafts, ..Default::default() },
            shown_decision: None,
            timeline: false,
            timeline_ui: TimelineUi::default(),
            drift: None,
            drift_rev: None,
            preview: None,
            preview_source: PreviewSource::Option,
            preview_gen: 0,
            scenario: None,
            show_implicit: false,
            lens: Lens::None,
            decision_ui: DecisionUi::default(),
            class_canvas: None,
            domain_canvas: None,
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
        }
    }

    // ------------------------------------------------------------------ small helpers

    pub fn toast(&mut self, kind: &str, text: impl Into<String>) {
        self.toast_seq += 1;
        let until = self.now() + if kind == "error" { 9.0 } else { 5.0 };
        self.toasts.push(Toast { id: self.toast_seq, kind: kind.to_string(), text: text.into(), until });
        if self.toasts.len() > 4 {
            self.toasts.remove(0);
        }
    }

    fn now(&self) -> f64 {
        self.ctx.input(|i| i.time)
    }

    /// Run `f` after `secs` (the old `setTimeout`).
    fn later(&mut self, secs: f64, f: impl FnOnce(&mut SpideApp) + 'static) {
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

    fn run_ok(&self, f: impl FnOnce(&crate::state::AppState, &crate::pi_bridge::PiBridge) -> Result<(), String> + Send + 'static, ok: impl FnOnce(&mut SpideApp) + Send + 'static) {
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
        self.review_active = true;
        self.set_preview(None, PreviewSource::Option);
        self.compare = None;
        self.branch_cmp = None;
        self.drift = None;
        self.timeline = false;
        self.brief = false;
        self.inline = None;
        self.prune = None;
        // the inspector opens
        self.drawer = true;
        self.workspace.right_open = true;
        self.class_data = None;
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

    fn focus_canvas(&mut self, name: Option<&str>) -> bool {
        let now = self.now();
        self.class_canvas.as_mut().is_some_and(|c| c.focus(name, now))
    }

    // ------------------------------------------------------------------ events

    fn drain(&mut self, ctx: &egui::Context) {
        while let Ok(e) = self.rx.try_recv() {
            match e {
                UiEvent::Snapshot(s) => {
                    // ignore stale snapshots of the same project
                    let keep_old = self.snap.as_ref().is_some_and(|p| p.project_dir == s.project_dir && p.rev > s.rev && p.pi_status == s.pi_status);
                    if !keep_old {
                        let source_changed = self.snap.as_ref().is_some_and(|p| p.rev != s.rev || p.source.generation != s.source.generation || p.source.status != s.source.status);
                        if self.drift.is_some() && source_changed {
                            if self.snap.as_ref().is_some_and(|p| p.source.attachment.is_some()) && s.source.attachment.is_none() {
                                self.drift = None;
                            } else {
                                let (project, generation, rev) = (s.project_dir.clone(), s.source.generation, s.rev);
                                self.backend.call(|st, _| crate::commands::drift(st), move |app, result| {
                                    if app.snap.as_ref().is_some_and(|snap| snap.project_dir == project && snap.source.generation == generation && snap.rev == rev) { app.drift = result.ok(); app.class_data = None; }
                                });
                            }
                        }
                        self.snap = Some(s);
                        self.class_data = None;
                        self.domain_data = None;
                    }
                }
                UiEvent::Toast { kind, text } => self.toast(&kind, text),
                UiEvent::Task(f) => f(self),
                UiEvent::Menu(MenuAction::Export) => self.open_export(),
                UiEvent::Menu(MenuAction::ExportAgain) => self.export_again(),
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
            }
        }
    }

    // ------------------------------------------------------------------ data

    fn class_data(&mut self) -> Option<Arc<ClassData>> {
        let snap = self.snap.clone()?;
        let key = hash_of((snap.rev, snap.source.generation, &snap.source.status, &snap.project_dir, self.preview_gen, self.review_active, self.drift.is_some(), self.proposal_canvas.as_ref().map(|d| d.rev)));
        if let Some(d) = self.class_data.as_ref().filter(|d| d.key == key) {
            return Some(d.clone());
        }
        let view = &snap.view;
        let d: &Design = if self.review_active { self.proposal_canvas.as_ref().unwrap_or(view) } else { self.preview.as_deref().unwrap_or(view) };
        let mut cd = ClassData { key, classes: d.classes.clone(), relations: d.relations.clone(), packages: d.packages.clone(), implicit: vec![], saved: self.preview.as_deref().unwrap_or(view).layout.get("class").cloned() };
        if let (Some(dr), None, false) = (&self.drift, &self.preview, self.review_active) {
            // code-only classes join the canvas as phantoms
            let phantom: HashSet<&String> = dr.drift.extra_in_code.iter().collect();
            let extra: Vec<Class> = dr.reality.classes.iter().filter(|c| phantom.contains(&c.name)).cloned().collect();
            let names: HashSet<String> = cd.classes.iter().map(|c| c.name.clone()).chain(extra.iter().map(|c| c.name.clone())).collect();
            let rels: Vec<Relation> = dr.reality.relations.iter().filter(|r| (phantom.contains(&r.from) || phantom.contains(&r.to)) && names.contains(&r.from) && names.contains(&r.to)).cloned().collect();
            cd.classes.extend(extra);
            cd.relations.extend(rels);
            cd.implicit = snap.metrics.implicit_edges.clone();
        } else if self.preview.is_none() && !self.review_active {
            cd.implicit = snap.metrics.implicit_edges.clone();
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
        let dm = &snap.derived.domain;
        let classes = dm.classes.iter().map(|c| Class { name: c.name.clone(), stereotype: c.stereotype.clone(), attributes: c.attributes.clone(), status: c.status, ..Default::default() }).collect();
        let saved = snap.view.layout.get("domain").or(snap.view.layout.get("class")).cloned();
        let a = Arc::new(ClassData { key, classes, relations: dm.relations.clone(), packages: vec![], implicit: vec![], saved });
        self.domain_data = Some(a.clone());
        Some(a)
    }

    fn stress_sets(&self) -> Option<Stress> {
        let snap = self.snap.as_ref()?;
        let c = snap.metrics.changes.iter().find(|x| Some(&x.name) == self.stress.as_ref())?;
        Some(Stress { impacted: c.impacted.iter().cloned().collect(), ripple: c.ripple.iter().cloned().collect() })
    }

    fn drift_sets(&self) -> Option<DriftSets> {
        let d = &self.drift.as_ref()?.drift;
        Some(DriftSets {
            missing: d.missing_in_code.iter().cloned().collect(),
            drifted: d.members.iter().map(|m| m.class.clone()).chain(d.packages.iter().map(|p| p.class.clone())).collect(),
            phantom: d.extra_in_code.iter().cloned().collect(),
        })
    }

    // ------------------------------------------------------------------ actions

    fn do_prune(&mut self, targets: Vec<Target>, at: Pos2) {
        if self.review_active {
            return self.toast("info", "Use the proposal editor to plan deletions; canvas editing is disabled during review.");
        }
        if let Some(id) = targets.iter().find_map(|t| if let Target::Proposal { id } = t { Some(id.clone()) } else { None }) {
            return self.open_proposal_review(Some(id));
        }
        // phantoms (code-only classes in the drift view) can't be pruned
        let phantom: HashSet<String> = self.drift.as_ref().map(|d| d.drift.extra_in_code.iter().cloned().collect()).unwrap_or_default();
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

    fn on_click_target(&mut self, t: Target) {
        let Some(snap) = self.snap.clone() else { return };
        let view = &snap.view;
        if let Target::Proposal { id } = &t {
            return self.open_proposal_review(Some(id.clone()));
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
    }

    fn on_double_click(&mut self, t: Target, at: Pos2) {
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

    fn import_code(&mut self) {
        let mut dlg = rfd::FileDialog::new().set_title("Java source folder to compare with the design");
        if let Some(d) = self.snap.as_ref().and_then(|s| s.view.settings.code_dir.clone()) {
            dlg = dlg.set_directory(d);
        }
        let Some(dir) = dlg.pick_folder() else { return };
        let dir = dir.display().to_string();
        self.backend.call(
            move |s, pi| {
                let sum = crate::commands::import_code(s, pi, dir)?;
                let d = crate::commands::drift(s)?;
                Ok((sum, d))
            },
            |app, r| match r {
                Ok((s, d)) => {
                    app.toast("info", format!("imported {} classes, {} relations from {} files{}", s.classes, s.relations, s.files, if s.skipped.is_empty() { String::new() } else { format!(" ({} skipped)", s.skipped.len()) }));
                    app.drift = Some(d);
                    app.tab = Tab::Class;
                    app.brief = false;
                    app.class_data = None;
                }
                Err(e) => app.toast("error", e),
            },
        );
    }

    // ------------------------------------------------------------------ export

    fn legend_for(&self) -> Vec<LegendKey> {
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
        match self.tab {
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
                for r in &snap.derived.domain.relations {
                    keys.insert(rk(r));
                }
            }
            Tab::Package => {
                keys.insert(LegendKey::Dependency);
                if metrics.package_deps.iter().any(|d| d.in_cycle) {
                    keys.insert(LegendKey::Cycle);
                }
                if metrics.package_deps.iter().any(|d| d.layer_violation) {
                    keys.insert(LegendKey::Violation);
                }
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

    /// The export SVG of the active view.
    fn export_svg(&mut self, o: &ExportOptions) -> Option<(String, u32, u32)> {
        let snap = self.snap.clone()?;
        let meta = ExportMeta { title: snap.view.name.clone(), view: self.tab.title().into(), health: Some(snap.metrics.health), legend: self.legend_for(), date: super::format_date_today() };
        let scene = match self.tab {
            Tab::Class | Tab::Domain => {
                let data = if self.tab == Tab::Class { self.class_data()? } else { self.domain_data()? };
                let level = snap.view.settings.detail_for(self.tab.key());
                let metrics = if self.tab == Tab::Class { Some(&snap.metrics) } else { None };
                let inp = self.canvas_input(&data, &snap, metrics, level, None);
                let now = self.now();
                let canvas = if self.tab == Tab::Class { self.class_canvas.as_mut()? } else { self.domain_canvas.as_mut()? };
                canvas.export_plain = Some(!o.diagnostics);
                let s = canvas.build_scene(&self.measure, &inp, now);
                canvas.export_plain = None;
                s
            }
            _ => std::mem::take(&mut self.views.scene),
        };
        let r = build_export_svg(&scene, &self.measure, &meta, o);
        if !matches!(self.tab, Tab::Class | Tab::Domain) {
            self.views.scene = scene;
        }
        r
    }

    fn settings_for_project(&self) -> ExportSettings {
        let p = self.snap.as_ref().map(|s| s.project_dir.clone()).unwrap_or_default();
        self.export_settings.get(&p).cloned().unwrap_or_default()
    }

    fn open_export(&mut self) {
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
            Format::Puml => crate::commands::export_plantuml(&st, self.tab.key().into(), target.clone())?,
            Format::Json => crate::commands::save_as(&st, target.clone())?,
            Format::Archunit => crate::commands::export_archunit(&st, target.clone())?,
        }
        Ok(Some(target))
    }

    fn export_again(&mut self) {
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
            "/code" => {
                if arg.is_empty() {
                    return self.toast("error", "usage: /code <Class>[.method] — the imported implementation");
                }
                let (c, m) = match arg.split_once('.') {
                    Some((c, m)) => (c.to_string(), Some(m.to_string()).filter(|m| !m.is_empty())),
                    None => (arg.clone(), None),
                };
                let name = find_class(view, &c).unwrap_or(c);
                let title = match &m {
                    Some(m) => format!("{name}.{m}()"),
                    None => name.clone(),
                };
                self.backend.run_then(move |s, _| crate::commands::code_snippet(s, name, m), move |app, text| app.code_view = Some((title, text)));
            }
            "/lens" => {
                let l = if arg.starts_with('c') {
                    Lens::Concerns
                } else if arg.starts_with('o') {
                    Lens::Overlap
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
                    self.backend.call(
                        |s, _| Ok(crate::commands::branch_list(s)),
                        |app, r: Result<Vec<crate::commands::BranchInfo>, String>| match r {
                            Ok(bs) => app.toast("info", format!("branches: {}", bs.iter().map(|b| format!("{}{} ({} classes, health {})", if b.active { "● " } else { "" }, b.name, b.classes, b.health)).collect::<Vec<_>>().join(" · "))),
                            Err(e) => app.toast("error", e),
                        },
                    );
                    return;
                }
                if let (true, Some(name)) = (sub == "delete", name) {
                    let ok = rfd::MessageDialog::new().set_title("Delete branch").set_level(rfd::MessageLevel::Warning).set_description(format!("Delete branch “{name}”? This cannot be undone.")).set_buttons(rfd::MessageButtons::OkCancelCustom("Delete".into(), "Cancel".into())).show();
                    if !matches!(ok, rfd::MessageDialogResult::Ok | rfd::MessageDialogResult::Yes) && ok != rfd::MessageDialogResult::Custom("Delete".into()) {
                        return;
                    }
                    let msg = format!("deleted branch {name}");
                    return self.run_ok(move |s, _| crate::commands::branch_delete(s, name), move |app| app.toast("info", msg));
                }
                self.run(move |s, _| crate::commands::branch_create(s, arg));
            }
            "/switch" => {
                if arg.is_empty() {
                    return self.toast("error", "usage: /switch <branch>");
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
                                    CompareCol { id: "●".into(), label: format!("{} (current)", snap.branch), design: Some(snap.view.clone()), health: Some(snap.metrics.health), impact: None, focus: focus.clone(), error: None, summary: None },
                                    CompareCol { id: "⎇".into(), label: arg.clone(), design: Some(c.design), health: Some(c.health), impact: Some(c.impact), focus, error: None, summary: None },
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
            "/agent" => self.run_ok(|s, pi| crate::commands::restart_agent(s, pi), |app| app.toast("info", "agent restarted")),
            "/folder" => self.run(|s, _| crate::commands::reveal_project(s)),
            "/import" => self.import_code(),
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
            "/drift" => {
                if self.drift.take().is_some() {
                    self.class_data = None;
                    return;
                }
                self.backend.run_then(
                    |s, _| crate::commands::drift(s),
                    |app, r| {
                        app.tab = Tab::Class;
                        app.brief = false;
                        app.drift = Some(r);
                        app.class_data = None;
                    },
                );
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
            "/save" => {
                if let Some(path) = rfd::FileDialog::new().set_file_name(format!("{}.json", slug(Some(&view.name)))).add_filter("Spide design", &["json"]).save_file() {
                    let path = path.display().to_string();
                    let msg = format!("saved {path}");
                    self.run_ok(move |s, _| crate::commands::save_as(s, path), move |app| app.toast("info", msg));
                }
            }
            "/open" => {
                if let Some(path) = rfd::FileDialog::new().add_filter("Spide design", &["json"]).pick_file() {
                    let path = path.display().to_string();
                    self.run(move |s, pi| crate::commands::open_design(s, pi, path));
                }
            }
            "/new" => {
                if let Some(dir) = rfd::FileDialog::new().set_title("Folder for the new design").pick_folder() {
                    let dir = dir.display().to_string();
                    self.run(move |s, pi| crate::commands::new_design(s, pi, dir));
                }
            }
            _ => self.toast("error", format!("unknown command {cmd}")),
        }
    }

    fn fit_active(&mut self) {
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

    fn zoom_active(&mut self, f: f32) {
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
        let term_focus = self.term.as_ref().is_some_and(|t| t.has_focus);
        if term_focus {
            return;
        }
        let in_field = ctx.egui_wants_keyboard_input();
        let ev: Vec<(Key, egui::Modifiers)> = ctx.input(|i| i.events.iter().filter_map(|e| if let egui::Event::Key { key, pressed: true, modifiers, .. } = e { Some((*key, *modifiers)) } else { None }).collect());
        for (key, m) in ev {
            if in_field && key != Key::Escape {
                continue;
            }
            if m.command && !m.alt {
                match key {
                    Key::K => {
                        self.palette = Some(Palette::new(""));
                        continue;
                    }
                    Key::Num1 | Key::Num2 | Key::Num3 | Key::Num4 | Key::Num5 | Key::Num6 => {
                        let n = [Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6].iter().position(|k| *k == key).unwrap();
                        self.tab = TAB_ORDER[n];
                        continue;
                    }
                    Key::Z => {
                        self.command(if m.shift { "/redo" } else { "/undo" });
                        continue;
                    }
                    Key::T => {
                        if self.review_active {
                            if !self.leave_proposal_review() {
                                continue;
                            }
                            self.review_active = false;
                            self.refresh_proposal_canvas();
                        }
                        self.tab = Tab::Class;
                        self.timeline = !self.timeline;
                        if !self.timeline {
                            self.close_timeline();
                        }
                        continue;
                    }
                    Key::B => {
                        self.brief = !self.brief;
                        continue;
                    }
                    Key::S => {
                        self.command("/save");
                        continue;
                    }
                    Key::O => {
                        self.command("/open");
                        continue;
                    }
                    // in the macOS app File › Export… (⌘E) and Export Again (⇧⌘E) own these shortcuts
                    Key::E if !cfg!(target_os = "macos") => {
                        if m.shift {
                            self.export_again();
                        } else {
                            self.open_export();
                        }
                        continue;
                    }
                    _ => {}
                }
            }
            if key == Key::Escape && self.export.is_none() && self.palette.is_none() && self.code_view.is_none() && self.compare.is_none() && self.branch_cmp.is_none() {
                if !in_field {
                    self.focus_canvas(None);
                    self.brief = false;
                    self.stress = None;
                    self.shown_decision = None;
                }
                self.prune = None;
                self.hover.clear();
                self.spotlight = None;
            }
        }
    }

    // ------------------------------------------------------------------ frame

    fn canvas_input<'a>(&self, data: &'a ClassData, snap: &'a Snapshot, metrics: Option<&'a spide_core::metrics::Metrics>, level: u8, hovered: Option<&'a str>) -> CanvasInput<'a> {
        let class = metrics.is_some();
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
            tension: if class && self.preview.is_none() { snap.metrics.tension } else { 0.0 },
            read_only: class && (self.preview.is_some() || self.review_active),
            review_focus: None,
            stress: None,
            drift: None,
            hovered,
            spotlight: None,
            can_move_to_package: class && self.preview.is_none() && self.drift.is_none() && !self.review_active,
            can_edit: !(class && (self.preview.is_some() || self.review_active)),
        }
    }

    pub fn frame(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.drain(&ctx);
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
        let full = ui.max_rect();
        ui.painter().rect_filled(full, 0.0, css::BG);
        let Some(snap) = self.snap.clone() else {
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
            // keep the drift report current while it's open
            if self.drift.is_some() && self.drift_rev != Some(snap.rev) {
                self.drift_rev = Some(snap.rev);
                self.backend.call(|s, _| crate::commands::drift(s), |app, r| {
                    if let (Ok(d), true) = (r, app.drift.is_some()) {
                        app.drift = Some(d);
                        app.class_data = None;
                    }
                });
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
        let ws = Rect::from_min_max(Pos2::new(full.left(), full.top() + 30.0), full.max);
        self.workspace_layout(ui, ws, &snap);
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
            action = terminal::header(ui, snap.pi_status, self.restarting, &mut self.term_confirm, self.pane_palette.is_some());
        });
        match action {
            Some(PaneAction::Collapse) => self.workspace.left_open = false,
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
        let error = self.term.as_ref().and_then(|t| t.error()).or_else(|| snap.pi_error.clone());
        let mut term_r = body;
        if let Some(e) = &error {
            let g = ui.painter().layout(e.clone(), sans(12.0), Color32::WHITE, r.width() - 24.0);
            let h = (g.size().y + 36.0).min(110.0);
            let er = Rect::from_min_max(Pos2::new(r.left(), r.bottom() - h), r.max);
            term_r.max.y = er.top();
            ui.scope_builder(egui::UiBuilder::new().max_rect(er), |ui| terminal::error_banner(ui, e));
        }
        if let Some(t) = self.term.as_mut() {
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
                act = overlays::palette(ui, p);
            });
            match act {
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
        let pending: Vec<&spide_core::model::Decision> = snap.view.decisions.iter().filter(|d| d.is_pending()).collect();
        let nav = Rect::from_min_size(Pos2::new(r.left(), r.top() + 30.0), Vec2::new(r.width(), 50.0));
        ui.painter().rect_filled(nav, 0.0, css::BG);
        ui.scope_builder(egui::UiBuilder::new().max_rect(nav.shrink2(Vec2::new(12.0, 12.0))), |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                let mode = |ui: &mut egui::Ui, text: String, on: bool| {
                    ui.add(egui::Button::new(RichText::new(text).font(sans(12.5)).color(if on { css::GHOST } else { css::TEXT })).fill(Color32::TRANSPARENT).stroke(Stroke::NONE).corner_radius(6).min_size(Vec2::new(0.0, 26.0)))
                };
                if mode(ui, format!("Decisions ({})", pending.len()), !self.source_open).clicked() {
                    self.source_open = false;
                    self.shown_decision = None;
                    self.open_proposal_review(None);
                }
                if mode(ui, "Source".into(), self.source_open).clicked() && self.leave_proposal_review() {
                    self.source_open = true;
                    self.review_active = false;
                    self.refresh_proposal_canvas();
                }
                let history_label = ui.label("History");
                egui::ComboBox::from_id_salt("decision-history").selected_text("History").width(70.0).show_ui(ui, |ui| {
                    for dec in snap.view.decisions.iter().filter(|d| !d.is_pending()).rev() {
                        if ui.selectable_label(self.shown_decision.as_ref() == Some(&dec.id), format!("{} · {}", dec.id, dec.question)).clicked() && self.leave_proposal_review() {
                            self.source_open = false;
                            self.review_active = false;
                            self.shown_decision = Some(dec.id.clone());
                        }
                    }
                }).response.labelled_by(history_label.id);
            });
        });
        let body = Rect::from_min_max(Pos2::new(r.left(), nav.bottom()), r.max);
        ui.scope_builder(egui::UiBuilder::new().max_rect(body), |ui| {
            ui.set_clip_rect(body);
            if self.source_open {
                super::source::show(ui, &mut self.source_ui, snap, &self.backend);
            } else if !pending.is_empty() && self.shown_decision.is_none() {
                self.review_active = true;
                let now = ui.input(|i| i.time);
                let mut out = vec![];
                proposal::show(ui, &mut self.proposal_ui, snap, self.selected_proposal.as_deref(), &self.backend, now, &mut out);
                for o in out {
                    match o {
                        ProposalOut::Select(id) => {
                            self.selected_proposal = id;
                        }
                        ProposalOut::Focus(name) => {
                            self.tab = Tab::Class;
                            self.later(0.15, move |app| {
                                if !app.focus_canvas(Some(&name)) {
                                    app.toast("info", "This target is not drawn on the class diagram. See the change details.");
                                }
                            });
                        }
                        ProposalOut::Compare(id) => self.compare = Some(id),
                        ProposalOut::Canvas => self.refresh_proposal_canvas(),
                    }
                }
            } else if let Some(id) = self.shown_decision.clone() {
                if decisions::history(ui, &snap.view, &id) {
                    self.shown_decision = None;
                }
            } else {
                egui::Frame::new().inner_margin(Margin::symmetric(22, 22)).show(ui, |ui| {
                    ui.label(RichText::new("No open decisions.").font(sans(12.5)).color(css::DIM));
                });
            }
        });
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
                    let key = format!("{}:{}", snap.project_dir, if self.preview.is_some() { "preview" } else { "class" });
                    if self.class_canvas.as_ref().is_none_or(|c| c.key != key) {
                        self.class_canvas = Some(ClassCanvas::new(key, "class"));
                    }
                    let review_focus = if self.review_active { super::state::proposal_review::review_footprint(self.proposal_ui.review.as_ref(), self.selected_proposal.as_deref()) } else { None };
                    let stress = self.stress_sets();
                    let drift = self.drift_sets();
                    let level = snap.view.settings.detail_for("class");
                    let mut inp = self.canvas_input(&data, snap, Some(&snap.metrics), level, hovered.as_deref());
                    inp.review_focus = review_focus.as_ref();
                    inp.stress = stress.as_ref();
                    inp.drift = drift.as_ref();
                    inp.spotlight = self.spotlight.as_ref().map(|s| s.as_deref());
                    inp.saved = if self.preview.is_some() { self.preview.as_ref().and_then(|p| p.layout.get("class")) } else { data.saved.as_ref() };
                    let c = self.class_canvas.as_mut().unwrap();
                    if c.pinned_focus.as_ref().is_some_and(|p| !data.classes.iter().any(|x| &x.name == p)) {
                        c.pinned_focus = None;
                    }
                    c.show(ui, stage, &self.measure, &inp, &mut canvas_out);
                }
            }
            Tab::Domain => {
                if let Some(data) = self.domain_data() {
                    let key = format!("{}:domain", snap.project_dir);
                    if self.domain_canvas.as_ref().is_none_or(|c| c.key != key) {
                        self.domain_canvas = Some(ClassCanvas::new(key, "domain"));
                    }
                    let level = snap.view.settings.detail_for("domain");
                    let inp = self.canvas_input(&data, snap, None, level, hovered.as_deref());
                    self.domain_canvas.as_mut().unwrap().show(ui, stage, &self.measure, &inp, &mut canvas_out);
                }
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
                    if self.prune.is_none() {
                        self.hover.move_to(k.as_deref(), Point { x: p.x, y: p.y }, now)
                    }
                }
                CanvasOut::Prune(t, p) => self.do_prune(t, p),
                CanvasOut::Click(t, _) => self.on_click_target(t),
                CanvasOut::DoubleClick(t, p) => self.on_double_click(t, p),
                CanvasOut::MoveToPackage(c, pk) => {
                    let msg = format!("moved {c} to {pk} (⌘Z to undo)");
                    self.run_ok(move |s, _| crate::commands::apply_user_ops(s, vec![brief::op(json!({"op": "move_to_package", "class": c, "package": pk}))]), move |app| app.toast("info", msg));
                }
                CanvasOut::SaveLayout(pos) => {
                    let view = if self.tab == Tab::Domain { "domain" } else { "class" }.to_string();
                    self.run(move |s, _| crate::commands::set_layout(s, view, pos));
                }
            }
        }
        for o in view_out {
            match o {
                ViewOut::Hover(k, p) => {
                    if self.prune.is_none() {
                        self.hover.move_to(k.as_deref(), Point { x: p.x, y: p.y }, now)
                    }
                }
                ViewOut::Prune(t, p) => self.do_prune(t, p),
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
                    ui.add(egui::Label::new(RichText::new(&view.name).font(bold(13.0)).color(css::TEXT)).truncate());
                    widgets::pill(ui, format!("⎇ {}", snap.branch), css::DIM, css::LINE, css::RAISED, super::theme::mono(11.0)).on_hover_text("active branch (/branch, /switch, /compare, /merge)");
                    if view.settings.partner != spide_core::model::PartnerMode::Examiner {
                        let l = brief::MODES.iter().find(|m| m.0 == view.settings.partner).map(|m| m.1).unwrap_or("");
                        widgets::pill(ui, l, css::ACCENT, Color32::from_rgba_unmultiplied(138, 169, 255, 102), css::ACCENT_SOFT, sans(11.0));
                    }
                    if self.preview.is_some() {
                        let t = if self.preview_source == PreviewSource::History { "Past revision · read-only" } else { "Option preview · read-only" };
                        widgets::pill(ui, t, css::GHOST, Color32::from_rgba_unmultiplied(94, 234, 212, 102), Color32::from_rgba_unmultiplied(94, 234, 212, 18), sans(11.0));
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
                self.tab = t;
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
                chrome::health_ring(ui, &mut self.health, metrics, snap.ghost_health, &mut health_actions);
                if matches!(self.tab, Tab::Class | Tab::Domain) {
                    let level = view.settings.detail_for(self.tab.key());
                    if let Some(l) = chrome::detail_slider(ui, level, self.preview.is_some()) {
                        let v = self.tab.key().to_string();
                        self.run(move |s, _| crate::commands::set_detail(s, v, l));
                    }
                }
                match chrome::zoom_group(ui) {
                    Some(chrome::Zoom::Out) => self.zoom_active(0.8),
                    Some(chrome::Zoom::In) => self.zoom_active(1.25),
                    Some(chrome::Zoom::Fit) => self.fit_active(),
                    None => {}
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
            match chrome::welcome(ui, r) {
                Some(chrome::WelcomeAction::Brief) => self.brief = true,
                Some(chrome::WelcomeAction::Open) => self.command("/open"),
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
        if let Some(report) = self.drift.clone() {
            match timeline::drift_panel(&ctx, r, &report, &self.backend) {
                Some(DriftOut::Close) => {
                    self.drift = None;
                    self.class_data = None;
                }
                Some(DriftOut::Reimport) => {
                    if let Some(dir) = view.settings.code_dir.clone() {
                        self.backend.run_then(
                            move |s, pi| {
                                crate::commands::import_code(s, pi, dir)?;
                                crate::commands::drift(s)
                            },
                            |app, d| {
                                app.drift = Some(d);
                                app.class_data = None;
                            },
                        );
                    }
                }
                Some(DriftOut::Find(c)) => {
                    self.focus_canvas(Some(&c));
                }
                None => {}
            }
        } else if self.brief && !self.review_active {
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
        // proposal dock and timeline
        if !view.proposals.is_empty() {
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
                act = overlays::palette(ui, p);
            });
            match act {
                Some(PaletteAction::Close) => self.palette = None,
                Some(PaletteAction::Run(c)) => {
                    self.palette = None;
                    self.command(&format!("/{c}"));
                }
                None => {}
            }
        }
        // code view
        if let Some((t, text)) = self.code_view.clone() {
            if overlays::code_view(&ctx, r, &t, &text) {
                self.code_view = None;
            }
        }
        // hover card
        if self.prune.is_none() {
            if let Some(h) = self.hover.snapshot().cloned() {
                let design: &Design = if self.review_active { self.proposal_canvas.as_ref().unwrap_or(view) } else { self.preview.as_deref().unwrap_or(view) };
                let cx = CardCtx { snap, design, metrics, drift: self.drift.as_ref() };
                let mut actions = vec![];
                let shown = hover_card::show(&ctx, &h.key, h.at, h.pinned, stage, &cx, &self.card_selected, &mut actions);
                if shown.is_none() {
                    self.hover.clear();
                }
                for a in actions {
                    match a {
                        CardAction::Close => self.hover.clear(),
                        CardAction::Select(s) => self.card_selected = s,
                        CardAction::Code { class, method } => {
                            let title = format!("{class}.{method}");
                            self.backend.run_then(move |s, _| crate::commands::code_snippet(s, class, Some(method)), move |app, text| app.code_view = Some((title, text)));
                        }
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
            let title = self.tab.title();
            let mut dlg = self.export.take().unwrap();
            let out = export::dialog(&ctx, &mut dlg, title, &mut |o| self.export_svg(o));
            let mut keep = true;
            match out {
                Some(DialogOut::Close) => keep = false,
                Some(DialogOut::Changed) => {
                    self.export_settings.insert(snap.project_dir.clone(), dlg.settings.clone());
                }
                Some(DialogOut::Reveal(p)) => {
                    let _ = crate::commands::reveal_path(p);
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
    fn export_dialog_and_real_files() {
        let dir = project("library");
        let mut h = harness(dir.path(), (1440.0, 900.0));
        steps(&mut h, 150);
        h.state_mut().open_export();
        steps(&mut h, 10);
        assert!(h.state().export.is_some());
        shot(&mut h, "export-dialog");
        h.state_mut().export = None;
        for (format, ext, magic) in [
            (Format::Svg, "svg", b"<svg".as_slice()),
            (Format::Png, "png", b"\x89PNG".as_slice()),
            (Format::Pdf, "pdf", b"%PDF".as_slice()),
            (Format::Puml, "puml", b"@startuml".as_slice()),
        ] {
            let path = dir.path().join(format!("test.{ext}"));
            let settings = ExportSettings { format, ..Default::default() };
            h.state_mut().run_export(&settings, Some(path.to_string_lossy().into())).unwrap();
            let data = std::fs::read(&path).unwrap();
            assert!(data.len() > 100);
            assert!(data.windows(magic.len()).any(|w| w == magic), "{ext} signature");
        }
    }
}

impl eframe::App for SpideApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.frame(ui);
    }
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, "spide.workspace.v1", &self.workspace);
        eframe::set_value(storage, "spide.export.v1", &self.export_settings);
    }
}
