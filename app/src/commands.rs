//! Commands: the UI's only way to talk to the core. Every command
//! expresses a user intent; results are broadcast as snapshots.

use crate::pi_bridge::PiBridge;
use crate::prompts;
use crate::state::AppState;
use spide_core::model::{ExaminerMode, PartnerMode, Pos};
use spide_core::{Design, Op, Target};
use std::collections::BTreeMap;
use std::path::PathBuf;

type R<T = ()> = Result<T, String>;

fn done(st: &AppState) -> R {
    st.persist_and_emit()
}

pub fn send_prompt(st: &AppState, pi: &PiBridge, text: String, tab: String) -> R {
    let generation = pi.ticket();
    let text = text.trim().to_string();
    if text.is_empty() {
        return Ok(());
    }
    let msg = prompts::user_prompt(&text, &tab, &st.lock().store.design);
    pi.prompt(generation, &st, msg, Some(text))
}

pub fn abort_agent(pi: &PiBridge) -> R {
    pi.abort()
}

pub fn restart_agent(st: &AppState, pi: &PiBridge) -> R {
    pi.restart(&st)
}

pub fn review_proposal(st: &AppState, id: String, ops: Option<Vec<Op>>) -> R<spide_core::proposal_review::ProposalReview> {
    st.lock().store.review_proposal(&id, ops.as_deref()).map_err(|e| e.0)
}

pub fn update_proposal(st: &AppState, id: String, expected_rev: u64, ops: Vec<Op>, rationale: Option<String>, decision: Option<String>, option: Option<String>) -> R {
    st.lock_writable()?.store.update_proposal(&id, expected_rev, ops, rationale, decision, option).map_err(|e| e.0)?;
    done(&st)
}

pub fn accept_proposal(st: &AppState, id: String, expected_rev: Option<u64>) -> R {
    { let mut state = st.lock_writable()?;
      if expected_rev.is_some_and(|rev| rev != state.store.design.rev) { return Err("Workspace changed. Review the latest plan before accepting.".into()); }
      state.store.accept_proposal(&id).map_err(|e| e.0)?;
    }
    done(&st)
}

pub fn approve_decision(st: &AppState, id: String, rev: u64, notes: String) -> R {
    st.lock_writable()?.store.approve_decision(&id, rev, notes).map_err(|e| e.0)?;
    done(st)
}

pub fn select_decision_option(st: &AppState, id: String, option: String, rev: u64) -> R {
    st.lock_writable()?.store.select_decision_option(&id, &option, rev).map_err(|e| e.0)?;
    done(st)
}

/// The structured revision request for one pending decision (stale revisions are rejected).
pub fn revision_message(s: &crate::state::Shared, id: &str, rev: u64, notes: &str, option: Option<&str>, targets: &[String]) -> R<String> {
    if s.store.design.rev != rev { return Err("Workspace changed; review again.".into()); }
    let dec = s.store.design.decisions.iter().find(|d| d.id == id && d.is_pending()).ok_or("Decision is closed")?;
    let notes = notes.trim();
    if notes.is_empty() { return Err("Describe what should change.".into()); }
    let plan = dec.plan.as_deref().unwrap_or("none");
    let option = option.or(dec.chosen.as_deref()).unwrap_or("none");
    let targets = if targets.is_empty() { "none (the whole plan)".to_string() } else { targets.iter().map(|t| format!("\n- {t}")).collect::<String>() };
    Ok(format!(
        "[spide · request revision · decision {id}]\nRevise this decision's concrete plan using propose_ops with decision/option IDs (or propose a replacement decision). Keep related small changes together in one coherent plan. Do NOT apply anything. No defence is required.\nDecision: {}\nPlan: {plan} · option shown to the user: {option} · design revision: {rev}\nDiagram elements the user selected:{targets}\nUser guidance: {notes}",
        serde_json::to_string(dec).unwrap(),
    ))
}

pub fn revise_decision(st: &AppState, pi: &PiBridge, id: String, rev: u64, notes: String, option: Option<String>, targets: Vec<String>) -> R {
    let generation = pi.ticket();
    let msg = revision_message(&st.lock(), &id, rev, &notes, option.as_deref(), &targets)?;
    pi.prompt(generation, st, msg, Some(format!("Revise {id}")))
}

pub fn accept_all(st: &AppState) -> R {
    st.lock_writable()?.store.accept_all_proposals().map_err(|e| e.0)?;
    done(&st)
}

pub fn reject_proposal(st: &AppState, id: String, expected_rev: Option<u64>) -> R {
    { let mut state = st.lock_writable()?;
      if expected_rev.is_some_and(|rev| rev != state.store.design.rev) { return Err("Workspace changed. Review the latest plan before rejecting.".into()); }
      state.store.reject_proposal(&id, None).map_err(|e| e.0)?;
    }
    done(&st)
}

/// Instant prune (right-click / marquee). Feedback comes separately.
pub fn prune(st: &AppState, targets: Vec<Target>) -> R {
    if targets.is_empty() {
        return Ok(());
    }
    st.lock_writable()?.store.prune(&targets, None).map_err(|e| e.0)?;
    done(&st)
}

/// "What didn't you like / what do you want instead?" → stored + sent to the agent.
pub fn prune_feedback(
    st: &AppState,
    pi: &PiBridge,
    targets: Vec<Target>,
    text: String,
    tab: String,
) -> R {
    let generation = pi.ticket();
    let text = text.trim().to_string();
    if text.is_empty() {
        return Ok(());
    }
    {
        let mut s = st.lock_writable()?;
        let d = &mut s.store.design;
        for t in &targets {
            d.feedback.push(spide_core::model::Feedback {
                target: t.label(),
                text: text.clone(),
                at: spide_core::store::now_ms(),
            });
        }
        // attach to pruned elements too
        for t in &targets {
            match t {
                Target::Class { name } => {
                    if let Some(c) = d.class_mut(name) {
                        c.feedback = Some(text.clone());
                    }
                }
                Target::Relation { id } => {
                    if let Some(r) = d.relations.iter_mut().find(|r| &r.id == id) {
                        r.feedback = Some(text.clone());
                    }
                }
                _ => {}
            }
        }
    }
    st.persist_and_emit()?;
    let msg = prompts::prune_feedback(&targets, &text, &tab);
    pi.prompt(generation, &st, msg, Some(format!("✂ {text}")))
}

pub fn restore(st: &AppState, target: Target) -> R {
    st.lock_writable()?.store.restore(&target).map_err(|e| e.0)?;
    done(&st)
}

pub fn submit_defense(
    st: &AppState,
    pi: &PiBridge,
    id: String,
    option: Option<String>,
    custom: Option<String>,
    defense: String,
) -> R {
    let _ = (st, pi, id, option, custom, defense);
    Err("Defence is no longer required. Review and approve the saved design plan.".into())
}

pub fn overrule(st: &AppState, id: String) -> R {
    st.lock_writable()?.store.overrule(&id).map_err(|e| e.0)?;
    done(&st)
}

pub fn dismiss_decision(st: &AppState, id: String) -> R {
    st.lock_writable()?.store.dismiss_decision(&id).map_err(|e| e.0)?;
    done(&st)
}

pub fn undo(st: &AppState) -> R {
    st.lock_writable()?.store.undo();
    done(&st)
}

pub fn redo(st: &AppState) -> R {
    st.lock_writable()?.store.redo();
    done(&st)
}

pub fn set_layout(st: &AppState, view: String, positions: BTreeMap<String, Pos>) -> R {
    {
        let mut s = st.lock_writable()?;
        s.store.set_layout(&view, positions);
    }
    st.persist_and_emit()
}

pub fn set_examiner(st: &AppState, strict: bool) -> R {
    st.lock_writable()?.store.set_examiner(if strict { ExaminerMode::Strict } else { ExaminerMode::Lenient });
    done(&st)
}

/// Direct user edits (brief panel, inline edits): committed immediately, undoable.
pub fn apply_user_ops(st: &AppState, ops: Vec<Op>) -> R {
    st.lock_writable()?.store.apply_user(ops).map_err(|e| e.0)?;
    done(&st)
}

pub fn set_rules(st: &AppState, rules: spide_core::rules::Rules) -> R {
    st.lock_writable()?.store.set_rules(rules).map_err(|e| e.0)?;
    done(&st)
}

/// Ask the agent which classes an expected change would touch.
pub fn analyse_change(st: &AppState, pi: &PiBridge, name: String) -> R {
    let generation = pi.ticket();
    let desc = {
        let s = st.lock();
        let d = &s.store.design;
        d.change_scenarios
            .iter()
            .find(|c| c.name == name)
            .and_then(|c| c.description.clone())
            .or_else(|| d.force(&name).and_then(|f| f.description.clone()))
    };
    let msg = prompts::analyse_change(&name, desc.as_deref());
    pi.prompt(generation, &st, msg, Some(format!("⇄ what if: {name}")))
}

/// "fix with agent" on a health issue.
pub fn fix_issue(
    st: &AppState,
    pi: &PiBridge,
    message: String,
    rule: Option<String>,
    fix_hint: Option<String>,
    kind: Option<String>,
    targets: Option<Vec<String>>,
) -> R {
    let generation = pi.ticket();
    let msg = prompts::fix_issue(&message, rule.as_deref(), fix_hint.as_deref(), kind.as_deref(), &targets.unwrap_or_default());
    pi.prompt(generation, &st, msg, Some(format!("🔧 {message}")))
}

/// "challenge assumptions" from the Design Health Inspector.
pub fn challenge_assumptions(st: &AppState, pi: &PiBridge) -> R {
    let generation = pi.ticket();
    let snap = st.snapshot();
    let msg = prompts::challenge_assumptions(&snap.view, &snap.metrics.health_report);
    pi.prompt(generation, &st, msg, Some("? challenge assumptions & simplify".into()))
}

pub fn set_partner(st: &AppState, mode: String) -> R {
    let m = PartnerMode::parse(&mode).ok_or_else(|| format!("unknown mode {mode} (examiner | socratic | devils_advocate | sketch)"))?;
    st.lock_writable()?.store.set_partner(m);
    done(&st)
}

pub fn set_brief_skipped(st: &AppState, on: bool) -> R {
    st.lock_writable()?.store.set_brief_skipped(on);
    done(&st)
}

pub fn set_show_pruned(st: &AppState, on: bool) -> R {
    st.lock_writable()?.store.set_show_pruned(on);
    done(&st)
}

pub fn set_detail(st: &AppState, view: String, level: u8) -> R {
    st.lock_writable()?.store.set_detail(&view, level);
    done(&st)
}

pub fn set_name(st: &AppState, name: String) -> R {
    st.lock_writable()?.store.design.name = name;
    done(&st)
}

/// Ask the agent to (re)generate a derived view.
pub fn regenerate(st: &AppState, pi: &PiBridge, view: String, scenario: Option<String>) -> R {
    let generation = pi.ticket();
    let msg = match view.as_str() {
        "usecase" => prompts::gen_use_cases(),
        "domain" => prompts::gen_domain_model(),
        "sequence" => prompts::gen_sequence(scenario.as_deref()),
        _ => return Err(format!("{view} is derived automatically")),
    };
    let label = match &scenario {
        Some(s) => format!("↻ sequence \"{s}\""),
        None => format!("↻ {view}"),
    };
    pi.prompt(generation, &st, msg, Some(label))
}

/// Before a batch export: ask the agent to regenerate every out-of-date agent-built view, in order.
pub fn regenerate_for_export(st: &AppState, pi: &PiBridge, views: Vec<String>, sequences: Vec<String>) -> R {
    let generation = pi.ticket();
    let msg = prompts::gen_for_export(views.iter().any(|v| v == "domain"), views.iter().any(|v| v == "usecase"), &sequences);
    pi.prompt(generation, st, msg, Some("↻ diagrams for export".into()))
}

// ---------------------------------------------------------------- files

pub fn new_design(st: &AppState, pi: &PiBridge, dir: String) -> R {
    switch_project(&st, &pi, PathBuf::from(dir), None, crate::workspace::LoadIntent::New)
}

pub fn open_design(st: &AppState, pi: &PiBridge, path: String) -> R {
    let path = PathBuf::from(path);
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let design = crate::workspace::parse_design(&text, &path)?;
    let dir = path.parent().map(PathBuf::from).ok_or("bad path")?;
    // `<root>/spide/design.json` of a Spide Project: open the project itself
    let root = spide_core::project::resolve_root(&dir);
    if root != dir && path.file_name().is_some_and(|n| n == "design.json") {
        return switch_project(st, pi, root, None, crate::workspace::LoadIntent::Open);
    }
    // Compatibility for legacy /open: never replace a different workspace's design.
    let target = dir.join("design.json");
    let intent = if target.try_exists().map_err(|e| e.to_string())? {
        let existing = std::fs::read_to_string(&target).map_err(|e| format!("{}: {e}", target.display()))?;
        if crate::workspace::parse_design(&existing, &target)? != design {
            return Err("This folder already has a different design.json. Import into a new workspace instead; no files were changed.".into());
        }
        crate::workspace::LoadIntent::Open
    } else {
        crate::workspace::LoadIntent::New
    };
    switch_project(&st, &pi, dir, Some(design), intent)
}

/// Open any Spide workspace folder: design-only (`design.json`) or a Spide Project (`spide/project.json`).
pub fn open_folder(st: &AppState, pi: &PiBridge, dir: String) -> R {
    switch_project(st, pi, PathBuf::from(dir), None, crate::workspace::LoadIntent::Open)
}

/// A new, blank design workspace in a new or empty folder (never an occupied one).
pub fn new_project(st: &AppState, pi: &PiBridge, dir: String, name: String) -> R {
    new_project_template(st, pi, dir, name, crate::templates::Template::Blank)
}

pub fn new_project_template(st: &AppState, pi: &PiBridge, dir: String, name: String, template: crate::templates::Template) -> R {
    let dir = PathBuf::from(dir);
    ensure_empty(&dir)?;
    let name = name.trim();
    let name = if name.is_empty() { dir.file_name().and_then(|n| n.to_str()).unwrap_or("Untitled") } else { name };
    let design = template.design(name)?;
    switch_project(st, pi, dir, Some(design), crate::workspace::LoadIntent::New)
}

/// Import a design JSON file into a new workspace folder; the source file is only read.
pub fn import_design(st: &AppState, pi: &PiBridge, file: String, dest: String) -> R {
    let file = PathBuf::from(file);
    let text = std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
    let design = crate::workspace::parse_design(&text, &file)?;
    let dest = PathBuf::from(dest);
    ensure_empty(&dest)?;
    switch_project(st, pi, dest, Some(design), crate::workspace::LoadIntent::New)
}

/// New projects and imports go into a new or empty folder only.
pub fn ensure_empty(dir: &std::path::Path) -> R {
    match std::fs::read_dir(dir) {
        Ok(mut it) => {
            if it.any(|e| e.is_ok_and(|e| e.file_name() != ".DS_Store")) {
                return Err(format!("{} is not empty. Choose a new or empty folder (use Open Project for existing projects).", dir.display()));
            }
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("{}: {e}", dir.display())),
    }
}

/// Close the project: guarded like any other leave, then nothing is open (welcome screen).
pub fn close_project(st: &AppState, pi: &PiBridge) -> R {
    let _transition = st.transition.lock().unwrap_or_else(|e| e.into_inner());
    if st.lock().workspace_status == crate::state::WorkspaceStatus::Closed {
        return Ok(());
    }
    guard_workspace_leave(st)?;
    pi.shutdown();
    crate::editor::embedded().stop();
    crate::service::retract(st);
    *st.service.lock().unwrap_or_else(|e| e.into_inner()) = None;
    crate::watcher::stop();
    {
        let mut s = st.lock();
        s.epoch += 1;
        s.workspace_status = crate::state::WorkspaceStatus::Closed;
        s.save_status = Default::default();
        s.lock = None; // releases the writer lock last
        s.project = None;
        s.project_dir = PathBuf::new();
        s.last_written = None;
        s.leave_copy_checkpoint = None;
        s.last_history = None;
        s.store = spide_core::Store::default();
        s.branch = "main".into();
        s.pi_status = crate::state::PiStatus::Offline;
        s.pi_error = None;
        s.evidence = None;
    }
    crate::project::watch(st); // no project: stops the project watcher
    st.emit();
    Ok(())
}

pub fn save_as(st: &AppState, path: String) -> R {
    let target = PathBuf::from(path);
    let json = {
        let s = st.lock();
        if matches!(s.workspace_status, crate::state::WorkspaceStatus::Failed(_) | crate::state::WorkspaceStatus::Loading | crate::state::WorkspaceStatus::Closed) {
            return Err("No valid design is open to copy.".into());
        }
        let original = s.design_path();
        let same_file = target == original || std::fs::canonicalize(&target).ok().zip(std::fs::canonicalize(&original).ok()).is_some_and(|(a, b)| a == b);
        if same_file {
            return Err("Choose a different file for the copy; use Save to retry saving this workspace.".into());
        }
        s.store.design.to_json()
    };
    crate::state::write_atomic(&target, &json).map_err(|e| e.to_string())?;
    let mut s = st.lock();
    if s.store.design.to_json() == json { s.leave_copy_checkpoint = Some(json); }
    Ok(())
}

fn switch_project(st: &AppState, pi: &PiBridge, dir: PathBuf, design: Option<Design>, intent: crate::workspace::LoadIntent) -> R {
    switch_project_with_agent(st, pi, dir, design, intent, |st, pi| pi.restart(st))
}

/// Common backend leave guard; interactive draft/job choices are added by the UI.
pub fn guard_workspace_leave(st: &AppState) -> R {
    match crate::editor::embedded_status(st) {
        crate::editor::EditorStatus::Dirty(d) => return Err(format!("Neovim has {} unsaved buffers. Save or discard them in Neovim before leaving this project.", d.len())),
        crate::editor::EditorStatus::Unknown(e) => return Err(format!("Can't verify Neovim's unsaved buffers ({e}). Save and quit Neovim (:wqa) before leaving this project.")),
        _ => {}
    }
    let writable = st.lock().require_writable().is_ok();
    if writable {
        if let Err(e) = st.persist_and_emit() {
            let s = st.lock();
            if s.leave_copy_checkpoint.as_deref() != Some(s.store.design.to_json().as_str()) { return Err(e); }
        }
    }
    Ok(())
}

fn switch_project_with_agent(
    st: &AppState,
    pi: &PiBridge,
    dir: PathBuf,
    design: Option<Design>,
    intent: crate::workspace::LoadIntent,
    start_agent: impl FnOnce(&AppState, &PiBridge) -> R,
) -> R {
    let _transition = st.transition.lock().unwrap_or_else(|e| e.into_inner());
    let resolved = spide_core::project::resolve_root(&dir);
    if intent == crate::workspace::LoadIntent::Open {
        if let Ok(root) = resolved.canonicalize() {
            let s = st.lock();
            if s.project_dir == root && matches!(s.workspace_status, crate::state::WorkspaceStatus::Ready | crate::state::WorkspaceStatus::ReadOnly) {
                return Ok(());
            }
        }
    }
    // Preflight and acquire the destination lock before touching the old session.
    let prepared = crate::workspace::prepare(&dir, intent)?;
    let dir = prepared.root;
    let new_lock = crate::state::acquire_lock(&dir).ok_or("Cannot switch: destination workspace is locked by another process.")?;
    guard_workspace_leave(st)?;
    let mut store = spide_core::Store::default();
    store.load(design.unwrap_or(prepared.design));
    let json = store.design.to_json();
    let design_path = if prepared.project.is_some() { dir.join(spide_core::project::DESIGN) } else { dir.join("design.json") };
    // Prove the first write will succeed before stopping pi/replacing the old store.
    // In particular a failed new/import/migration write leaves the current project intact.
    if prepared.last_written.as_deref() != Some(json.as_str()) {
        crate::state::write_atomic(&design_path, &json).map_err(|e| format!("Cannot open {}: {e}", design_path.display()))?;
    }
    pi.shutdown();
    // The guard proved the embedded editor clean; it belongs to the old root.
    crate::editor::embedded().stop();
    crate::service::retract(st);
    {
        let mut s = st.lock();
        s.epoch += 1;
        s.lock = Some(new_lock);
        s.workspace_status = crate::state::WorkspaceStatus::Ready;
        s.save_status = crate::state::SaveStatus { saved_revision: Some(store.design.rev), ..Default::default() };
        s.project_dir = dir.clone();
        s.last_written = Some(json);
        s.leave_copy_checkpoint = None;
        s.last_history = None;
        s.branch = crate::branches::active(&dir);
        s.project = prepared.project;
        s.store = store;
        s.pi_status = crate::state::PiStatus::Offline;
        s.pi_error = None;
    }
    crate::service::install(st);
    crate::watcher::watch(st, &design_path);
    crate::project::watch(st);
    crate::project::refresh_async(st);
    st.persist_and_emit()?;
    // Opening succeeded. An optional integration failing must not masquerade as
    // a failed project open or roll the user back to the previous workspace.
    let auto_start = st.lock().preferences.agent_auto_start;
    if let Err(e) = if auto_start { start_agent(st, pi) } else { Ok(()) } {
        {
            let mut s = st.lock();
            s.pi_status = crate::state::PiStatus::Offline;
            s.pi_error = Some(e.clone());
        }
        st.emit();
    }
    Ok(())
}

/// Recover a failed/corrupt workspace as a NEW workspace in `dest` from the newest valid recovery
/// copy or history snapshot. The broken original is never written.
pub fn recover_copy(st: &AppState, pi: &PiBridge, dest: String) -> R<String> {
    let source = st.lock().project_dir.clone();
    let (label, mut design) = crate::recovery::candidates(&source).into_iter().next().ok_or("No recovery copy or history was found for this project.")?;
    design.name = format!("{} (recovered)", design.name);
    switch_project(st, pi, PathBuf::from(dest), Some(design), crate::workspace::LoadIntent::New)?;
    Ok(label)
}

pub fn export_png(svg: String, path: String, scale: f32) -> R {
    let png = spide_core::png::render(&svg, scale).map_err(|e| e.to_string())?;
    std::fs::write(path, png).map_err(|e| e.to_string())
}

pub fn export_pdf(svg: String, path: String) -> R {
    let pdf = spide_core::pdf::render(&svg).map_err(|e| e.to_string())?;
    std::fs::write(path, pdf).map_err(|e| e.to_string())
}

pub fn export_svg(svg: String, path: String) -> R {
    export_text(svg, path)
}

/// A text export (SVG, Obsidian canvas JSON).
pub fn export_text(text: String, path: String) -> R {
    std::fs::write(path, text).map_err(|e| e.to_string())
}

pub fn reveal_path(path: String) -> R {
    std::process::Command::new("open").arg("-R").arg(&path).spawn().map(|_| ()).map_err(|e| e.to_string())
}

pub fn export_plantuml(st: &AppState, view: String, path: String) -> R {
    let snap = st.snapshot();
    let text = spide_core::plantuml::export(&view, &snap.view, &snap.metrics, &snap.view.layout)
        .map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| e.to_string())
}

pub fn export_archunit(st: &AppState, path: String) -> R {
    let java = {
        let s = st.lock();
        let mut d = s.store.design.clone();
        // a Spide Project's manifest names the code root
        if let Some(root) = s.project.as_ref().and_then(|p| p.manifest.code_root.clone()) {
            d.settings.code_root = Some(root);
        }
        spide_core::archunit::archunit_test(&d)
    };
    std::fs::write(path, java).map_err(|e| e.to_string())
}

/// Root module path of the real code (`org.acme.library`, `shop`, `my_crate`).
pub fn set_code_root(st: &AppState, root: Option<String>) -> R {
    if let Some(r) = root.as_deref().filter(|r| !r.trim().is_empty()) {
        let ok = r.split('.').all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'));
        if !ok {
            return Err(format!("not a dotted module path: {r}"));
        }
    }
    let manifest = st.lock().project.as_ref().map(|p| p.manifest.clone());
    if let Some(mut m) = manifest {
        m.code_root = root.map(|r| r.trim().to_string()).filter(|r| !r.is_empty());
        return save_manifest(st, m);
    }
    st.lock_writable()?.store.set_code_root(root);
    done(&st)
}

/// ADRs (one markdown file per accepted/overruled decision) into `dir`.
pub fn export_adr(st: &AppState, dir: String) -> R<Vec<String>> {
    let files = spide_core::adr::adrs(&st.lock().store.design);
    if files.is_empty() {
        return Err("no decided decisions yet".into());
    }
    let dir = PathBuf::from(dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut out = vec![];
    for (name, text) in files {
        let p = dir.join(name);
        std::fs::write(&p, text).map_err(|e| format!("{}: {e}", p.display()))?;
        out.push(p.display().to_string());
    }
    Ok(out)
}

/// PlantUML for every diagram into `dir`; returns the written file paths.
pub fn export_plantuml_all(st: &AppState, dir: String) -> R<Vec<String>> {
    let snap = st.snapshot();
    let dir = PathBuf::from(dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let base = slug(&snap.view.name);
    let mut written = vec![];
    for view in ["system", "class", "domain", "package", "usecase", "sequence"] {
        let empty = match view {
            "system" => snap.view.containers.is_empty() && snap.view.systems.is_empty(),
            "usecase" => snap.view.use_cases.use_cases.is_empty(),
            "domain" => snap.view.domain.is_empty(),
            "sequence" => snap.view.sequences.is_empty(),
            _ => snap.view.classes.is_empty(),
        };
        if empty {
            continue;
        }
        let text = spide_core::plantuml::export(view, &snap.view, &snap.metrics, &snap.view.layout)?;
        let path = dir.join(format!("{base}-{view}.puml"));
        std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
        written.push(path.display().to_string());
    }
    for package in snap.view.packages.iter().filter(|p| p.status.is_live()) {
        let text = spide_core::plantuml::export_package(&snap.view, &snap.metrics, &package.name)?;
        let path = dir.join(format!("{base}-class-package-{}.puml", spide_core::scope::package_key(&package.name)));
        std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
        written.push(path.display().to_string());
    }
    if written.is_empty() {
        return Err("nothing to export yet".into());
    }
    Ok(written)
}

fn slug(s: &str) -> String {
    let s: String = s.to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    let s = s.split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-");
    if s.is_empty() { "design".into() } else { s }
}

// ---------------------------------------------------------------- history

pub fn history_list(st: &AppState) -> Vec<crate::history::Entry> {
    let dir = st.lock().project_dir.clone();
    crate::history::list(&dir)
}

pub fn history_at(st: &AppState, rev: u64) -> R<Design> {
    let dir = st.lock().project_dir.clone();
    crate::history::at(&dir, rev).ok_or_else(|| format!("no history at revision {rev}"))
}

/// Bring the design back to how it was at `rev` — as a proposal the user accepts.
pub fn history_restore(st: &AppState, rev: u64) -> R {
    let dir = st.lock().project_dir.clone();
    let past = crate::history::at(&dir, rev).ok_or_else(|| format!("no history at revision {rev}"))?;
    {
        let mut s = st.lock_writable()?;
        let ops = spide_core::diff::diff(&s.store.design, &past);
        if ops.is_empty() {
            return Err("the design already looks like that".into());
        }
        s.store.propose_ops(ops, Some(format!("restore the design as of revision {rev}")), "history").map_err(|e| e.0)?;
    }
    done(&st)
}

/// Start a branch from a past revision.
pub fn history_fork(st: &AppState, rev: u64, name: String) -> R {
    let dir = st.lock_writable()?.project_dir.clone();
    if crate::branches::exists(&dir, &name) || name == "main" {
        return Err(format!("branch {name} already exists"));
    }
    let mut past = crate::history::at(&dir, rev).ok_or_else(|| format!("no history at revision {rev}"))?;
    past.name = format!("{} ({name})", st.lock().store.design.name);
    crate::branches::write(&dir, &name, &past)?;
    st.toast("info", format!("branch {name} created from revision {rev} — /switch {name}"));
    Ok(())
}

// ---------------------------------------------------------------- branches

#[derive(serde::Serialize)]
pub struct BranchInfo {
    pub name: String,
    pub active: bool,
    pub classes: usize,
}

pub fn branch_list(st: &AppState) -> Vec<BranchInfo> {
    let (dir, active, cur) = {
        let s = st.lock();
        (s.project_dir.clone(), s.branch.clone(), s.store.design.clone())
    };
    let info = |name: String, d: &Design, active: bool| BranchInfo {
        name,
        active,
        classes: d.live_classes().count(),
    };
    let mut out = vec![info(active.clone(), &cur, true)];
    for name in crate::branches::list(&dir).into_iter().filter(|n| *n != active) {
        if let Ok(d) = crate::branches::read(&dir, &name) {
            out.push(info(name, &d, false));
        }
    }
    out
}

/// Create a branch from the current design and switch to it.
pub fn branch_create(st: &AppState, name: String) -> R {
    drop(st.lock_writable()?);
    crate::branches::valid_name(&name)?;
    let (dir, active, cur) = {
        let s = st.lock();
        (s.project_dir.clone(), s.branch.clone(), s.store.design.clone())
    };
    if name == active || crate::branches::exists(&dir, &name) {
        return Err(format!("branch {name} already exists"));
    }
    crate::branches::write(&dir, &name, &cur)?;
    switch_branch(&st, &name)
}

/// Park the current design as its branch and load another one into design.json.
pub fn branch_switch(st: &AppState, name: String) -> R {
    switch_branch(&st, &name)
}

fn switch_branch(st: &AppState, name: &str) -> R {
    let (dir, active, cur) = {
        let s = st.lock_writable()?;
        (s.project_dir.clone(), s.branch.clone(), s.store.design.clone())
    };
    if name == active {
        return Err(format!("already on {name}"));
    }
    let next = crate::branches::read(&dir, name)?;
    crate::branches::write(&dir, &active, &cur)?;
    {
        let mut s = st.lock_writable()?;
        s.store.load(next);
        s.branch = name.to_string();
    }
    crate::branches::delete(&dir, name)?;
    crate::branches::set_active(&dir, name).map_err(|e| e.to_string())?;
    st.toast("info", format!("on branch {name} (was {active})"));
    done(st)
}

pub fn branch_delete(st: &AppState, name: String) -> R {
    let (dir, active) = {
        let s = st.lock_writable()?;
        (s.project_dir.clone(), s.branch.clone())
    };
    if name == active {
        return Err("can't delete the active branch; switch first".into());
    }
    crate::branches::delete(&dir, &name)?;
    st.toast("info", format!("branch {name} deleted"));
    st.emit();
    Ok(())
}

#[derive(serde::Serialize)]
pub struct BranchCompare {
    pub name: String,
    pub design: Design,
    /// From the active design to the branch.
    pub impact: spide_core::impact::Impact,
}

pub fn branch_compare(st: &AppState, name: String) -> R<BranchCompare> {
    let (dir, cur) = {
        let s = st.lock();
        (s.project_dir.clone(), s.store.design.clone())
    };
    let other = crate::branches::read(&dir, &name)?;
    let impact = spide_core::impact::impact(&cur, &other);
    Ok(BranchCompare { name, design: other, impact })
}

/// Bring another branch's changes in as proposals: additions/changes and removals separately,
/// so the user can take one without the other.
pub fn branch_merge(st: &AppState, name: String) -> R {
    let dir = st.lock().project_dir.clone();
    let other = crate::branches::read(&dir, &name)?;
    {
        let mut s = st.lock_writable()?;
        let ops = spide_core::diff::diff(&s.store.design, &other);
        if ops.is_empty() {
            return Err(format!("{name} has nothing this design doesn't"));
        }
        let (removes, rest): (Vec<Op>, Vec<Op>) = ops.into_iter().partition(|o| {
            matches!(
                o,
                Op::RemoveClass { .. }
                    | Op::RemoveAttribute { .. }
                    | Op::RemoveMethod { .. }
                    | Op::RemoveRelation { .. }
                    | Op::RemovePackage { .. }
                    | Op::RemoveForce { .. }
                    | Op::RemoveChangeScenario { .. }
            )
        });
        if !rest.is_empty() {
            s.store.propose_ops(rest, Some(format!("merge from branch {name}: additions and changes")), "branch").map_err(|e| e.0)?;
        }
        if !removes.is_empty() {
            s.store.propose_ops(removes, Some(format!("merge from branch {name}: removals")), "branch").map_err(|e| e.0)?;
        }
    }
    done(&st)
}


pub fn reveal_project(st: &AppState) -> R {
    let dir = st.lock().project_dir.clone();
    std::process::Command::new("open").arg(dir).spawn().map_err(|e| e.to_string())?;
    Ok(())
}

// ---------------------------------------------------------------- Spide Project

/// Make `root` a Spide Project with the current design (copied; the old folder stays) and open it.
pub fn create_project(st: &AppState, pi: &PiBridge, root: String, manifest: spide_core::project::Manifest, git_exclude: bool) -> R<Vec<String>> {
    let root = PathBuf::from(root);
    let (design, from) = {
        let s = st.lock();
        // from the welcome screen nothing is open: start blank, copy no history
        let open = matches!(s.workspace_status, crate::state::WorkspaceStatus::Ready | crate::state::WorkspaceStatus::ReadOnly);
        (if open { s.store.design.clone() } else { Design::default() }, open.then(|| s.project_dir.clone()))
    };
    let added = crate::project::create(&root, &manifest, &design, from.as_deref(), git_exclude)?;
    switch_project(st, pi, root, None, crate::workspace::LoadIntent::Open)?;
    Ok(added)
}

pub fn open_project(st: &AppState, pi: &PiBridge, root: String) -> R {
    let root = spide_core::project::resolve_root(&PathBuf::from(root));
    if !spide_core::project::is_project(&root) {
        return Err(format!("{} has no spide/project.json — create a Spide Project there first", root.display()));
    }
    switch_project(st, pi, root, None, crate::workspace::LoadIntent::Open)
}

/// Save an edited manifest and index again.
pub fn save_manifest(st: &AppState, manifest: spide_core::project::Manifest) -> R {
    {
        let mut s = st.lock_writable()?;
        let p = s.project.as_mut().ok_or("not a Spide Project")?;
        let errors = manifest.validate(&p.root, spide_core::project::work_tree(&p.root).as_deref());
        if !errors.is_empty() {
            return Err(errors.join("; "));
        }
        crate::state::write_atomic(&p.root.join(spide_core::project::MANIFEST), &manifest.to_json()).map_err(|e| e.to_string())?;
        p.manifest = manifest;
        p.warnings.clear();
    }
    crate::project::watch(st);
    crate::project::refresh_async(st);
    Ok(())
}

pub fn reindex(st: &AppState) -> R {
    if st.lock().project.is_none() {
        return Err("not a Spide Project".into());
    }
    crate::project::refresh_async(st);
    Ok(())
}

/// Code → design: the agent proposes design changes for what changed in the code.
pub fn readjust(st: &AppState, pi: &PiBridge, classes: Vec<String>) -> R {
    let generation = pi.ticket();
    let msg = {
        let s = st.lock();
        let p = s.project.as_ref().ok_or("not a Spide Project")?;
        let a = p.analysis.as_deref().ok_or("the code isn't indexed yet")?;
        let n = a
            .report
            .items
            .iter()
            .filter(|i| classes.is_empty() || classes.contains(&i.class))
            .filter(|i| matches!(i.status, spide_core::sync::SyncStatus::CodeAhead | spide_core::sync::SyncStatus::Conflict | spide_core::sync::SyncStatus::Unreconciled))
            .count();
        if n == 0 {
            return Err("nothing to readjust in this scope".into());
        }
        let empty = s.store.design.live_classes().next().is_none();
        prompts::readjust(&classes, n, empty)
    };
    pi.prompt(generation, st, msg, Some("↻ readjust design".into()))
}

/// Design → code: the agent implements the approved design (pi's own permissions guard the edits).
pub fn implement(st: &AppState, pi: &PiBridge, classes: Vec<String>, keep_design: bool) -> R {
    let generation = pi.ticket();
    let (msg, pending) = {
        let mut s = st.lock_writable()?;
        let items = crate::project::begin_implement(&mut s, &classes, keep_design)?;
        let p = s.project.as_ref().unwrap();
        let a = p.analysis.clone().unwrap();
        let code = p.index.code.clone().unwrap();
        let brief = spide_core::sync::implement_brief(&a, &s.store.design, &code, &p.manifest, &classes, keep_design);
        let touched: std::collections::BTreeSet<String> = items.iter().filter_map(|id| a.report.items.iter().find(|i| &i.id == id)).map(|i| i.class.clone()).collect();
        let pending: Vec<String> = s.store.design.proposals.iter().filter(|p| p.ops.iter().flat_map(Op::touched_classes).any(|c| touched.contains(&c))).map(|p| p.id.clone()).collect();
        (prompts::implement(&brief, &p.manifest), pending)
    };
    if !pending.is_empty() {
        st.toast("info", format!("Pending plans touch these classes ({}); only the approved design is implemented.", pending.join(", ")));
    }
    st.emit();
    let r = pi.prompt(generation, st, msg, Some("⇢ implement in code".into()));
    if r.is_err() {
        if let Some(p) = st.lock().project.as_mut() {
            p.implementing = None;
        }
        st.emit();
    }
    r
}

/// Re-index; with a test command, the agent runs it and reports.
pub fn verify(st: &AppState, pi: &PiBridge) -> R {
    let generation = pi.ticket();
    let cmd = {
        let s = st.lock();
        let p = s.project.as_ref().ok_or("not a Spide Project")?;
        p.manifest.commands.get("test").cloned()
    };
    crate::project::refresh_async(st);
    match cmd {
        Some(c) => pi.prompt(generation, st, prompts::verify(&c.run, c.cwd.as_deref().unwrap_or(".")), Some("✓ verify".into())),
        None => Ok(()),
    }
}

pub fn mark_synced(st: &AppState) -> R<usize> {
    crate::project::mark_synced(st)
}

#[cfg(test)]
mod loader_tests {
    use super::*;
    use crate::{events::Events, workspace::LoadIntent};

    #[test]
    fn successful_save_copy_allows_leaving_a_failed_save_but_not_newer_unsaved_work() {
        let current = tempfile::tempdir().unwrap();
        let copies = tempfile::tempdir().unwrap();
        let st = AppState::create(Events::new().0, current.path().into());
        st.persist_and_emit().unwrap();
        std::fs::create_dir(current.path().join("design.json.tmp")).unwrap();
        assert!(set_name(&st, "Unsaved current".into()).is_err());
        assert!(guard_workspace_leave(&st).is_err());
        save_as(&st, copies.path().join("copy.json").display().to_string()).unwrap();
        assert!(guard_workspace_leave(&st).is_ok());
        assert!(set_name(&st, "Newer work not in copy".into()).is_err());
        assert!(guard_workspace_leave(&st).is_err());
        save_as(&st, copies.path().join("newer-copy.json").display().to_string()).unwrap();
        close_project(&st, &PiBridge::new()).unwrap();
        assert_eq!(st.lock().workspace_status, crate::state::WorkspaceStatus::Closed);
        assert!(std::fs::read_to_string(copies.path().join("newer-copy.json")).unwrap().contains("Newer work not in copy"));
    }

    #[test]
    fn same_project_alias_is_a_noop_and_locked_destination_preserves_current() {
        let current = tempfile::tempdir().unwrap();
        let destination = tempfile::tempdir().unwrap();
        let st = AppState::create(Events::new().0, current.path().into());
        st.persist_and_emit().unwrap();
        let other = AppState::create(Events::new().0, destination.path().into());
        other.persist_and_emit().unwrap();
        let pi = PiBridge::new();
        let ticket = pi.ticket();
        switch_project_with_agent(&st, &pi, current.path().join("."), None, LoadIntent::Open, |_, _| panic!("same project must not restart pi")).unwrap();
        assert_eq!(pi.ticket(), ticket);
        assert!(switch_project_with_agent(&st, &pi, destination.path().into(), None, LoadIntent::Open, |_, _| panic!("locked destination must not restart pi")).is_err());
        assert_eq!(st.lock().project_dir, current.path().canonicalize().unwrap());
        assert_eq!(pi.ticket(), ticket);
    }

    #[test]
    fn failed_destination_write_or_current_save_keeps_current_session() {
        let current = tempfile::tempdir().unwrap();
        let destination = tempfile::tempdir().unwrap();
        let st = AppState::create(Events::new().0, current.path().into());
        set_name(&st, "Current".into()).unwrap();
        let pi = PiBridge::new();
        let ticket = pi.ticket();
        std::fs::create_dir(destination.path().join("design.json.tmp")).unwrap();
        assert!(switch_project_with_agent(&st, &pi, destination.path().into(), None, LoadIntent::New, |_, _| panic!("failed first write must not restart pi")).is_err());
        assert_eq!(st.lock().store.design.name, "Current");
        assert_eq!(pi.ticket(), ticket);
        std::fs::remove_dir(destination.path().join("design.json.tmp")).unwrap();
        std::fs::create_dir(current.path().join("design.json.tmp")).unwrap();
        assert!(set_name(&st, "Unsaved current".into()).is_err());
        assert!(switch_project_with_agent(&st, &pi, destination.path().into(), None, LoadIntent::New, |_, _| panic!("failed current save must not restart pi")).is_err());
        assert_eq!(st.lock().store.design.name, "Unsaved current");
        assert!(!destination.path().join("design.json").exists());
        assert_eq!(pi.ticket(), ticket);
    }

    #[test]
    fn agent_failure_leaves_the_successfully_opened_project_editable() {
        let current = tempfile::tempdir().unwrap();
        let destination = tempfile::tempdir().unwrap();
        let st = AppState::create(Events::new().0, current.path().into());
        let pi = PiBridge::new();
        switch_project_with_agent(&st, &pi, destination.path().into(), None, LoadIntent::New, |_, _| Err("pi missing".into())).unwrap();
        assert_eq!(st.lock().project_dir, destination.path().canonicalize().unwrap());
        assert_eq!(st.lock().pi_status, crate::state::PiStatus::Offline);
        assert!(st.lock().pi_error.as_deref().unwrap().contains("pi missing"));
        set_name(&st, "Usable offline".into()).unwrap();
        assert!(crate::state::acquire_lock(current.path()).is_some(), "old writer lock is released");
        crate::service::retract(&st);
    }

    #[test]
    fn stale_service_and_queued_results_cannot_touch_a_reopened_workspace() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let (events, rx) = Events::new();
        let st = AppState::create(events, a.path().into());
        st.persist_and_emit().unwrap();
        let pi = PiBridge::new();
        let old = crate::service::install(&st);
        let epoch = st.epoch();
        // A → B → A: same root, same path, new epoch
        switch_project_with_agent(&st, &pi, b.path().into(), None, LoadIntent::New, |_, _| Ok(())).unwrap();
        switch_project_with_agent(&st, &pi, a.path().into(), None, LoadIntent::Open, |_, _| Ok(())).unwrap();
        assert_eq!(st.epoch(), epoch + 2);
        let before = st.lock().store.design.to_json();
        let rev = st.lock().store.design.rev;
        let op = serde_json::json!({"op":"add_class","name":"Ghost"});
        assert!(old.call_tool("apply_ops", &serde_json::json!({"expected_rev": rev, "ops": [op]}), "user").is_err());
        assert!(old.call_tool("get_project", &serde_json::json!({}), "user").is_err());
        assert_eq!(st.lock().store.design.to_json(), before, "the old service never reaches the new store");
        // the current service works
        let svc = crate::service::get(&st).unwrap();
        svc.call_tool("apply_ops", &serde_json::json!({"expected_rev": rev, "ops": [op]}), "user").unwrap();
        assert!(st.lock().store.design.classes.iter().any(|c| c.name == "Ghost"));
        // a queued UI request whose workspace changes while it runs: the result is dropped
        while rx.try_recv().is_ok() {}
        let backend = crate::backend::Backend::new(st.clone(), std::sync::Arc::new(PiBridge::new()), None);
        backend.call(|s, _| { s.lock().epoch += 1; Ok(()) }, |_, _: Result<(), String>| panic!("stale result delivered"));
        assert!(!std::iter::from_fn(|| rx.try_recv().ok()).any(|e| matches!(e, crate::events::UiEvent::Task(_))));
        // a transition's own result is always delivered
        backend.transition(|s, _| { s.lock().epoch += 1; Ok(()) }, |_, _: Result<(), String>| {});
        assert!(std::iter::from_fn(|| rx.try_recv().ok()).any(|e| matches!(e, crate::events::UiEvent::Task(_))));
        crate::service::retract(&st);
    }

    #[test]
    fn corrupt_workspace_recovers_as_a_new_copy_without_touching_the_original() {
        let broken = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        {
            let st = AppState::create(Events::new().0, broken.path().into());
            set_name(&st, "Precious".into()).unwrap();
            apply_user_ops(&st, serde_json::from_str(r#"[{"op":"add_class","name":"Order"}]"#).unwrap()).unwrap();
        }
        std::fs::write(broken.path().join("design.json"), "{ truncated").unwrap();
        let st = AppState::new(Events::new().0, broken.path().into());
        assert!(matches!(st.lock().workspace_status, crate::state::WorkspaceStatus::Failed(_)));
        let pi = PiBridge::new();
        // the agent can't start in tests; recovery still succeeds
        let label = recover_copy(&st, &pi, dest.path().join("recovered").display().to_string()).unwrap();
        assert!(label.contains("history"), "{label}");
        assert_eq!(std::fs::read_to_string(broken.path().join("design.json")).unwrap(), "{ truncated");
        let s = st.lock();
        assert_eq!(s.workspace_status, crate::state::WorkspaceStatus::Ready);
        assert!(s.store.design.name.starts_with("Precious"));
        assert!(s.store.design.classes.iter().any(|c| c.name == "Order"));
        drop(s);
        crate::service::retract(&st);
    }

    #[test]
    fn workspace_transitions_do_not_overlap() {
        use std::sync::{atomic::{AtomicUsize, Ordering}, Arc};
        let current = tempfile::tempdir().unwrap();
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let st = AppState::create(Events::new().0, current.path().into());
        let pi = Arc::new(PiBridge::new());
        let active = Arc::new(AtomicUsize::new(0));
        std::thread::scope(|scope| {
            for destination in [first.path(), second.path()] {
                let st = st.clone();
                let pi = pi.clone();
                let active = active.clone();
                scope.spawn(move || {
                    switch_project_with_agent(&st, &pi, destination.into(), None, LoadIntent::New, |_, _| {
                        assert_eq!(active.fetch_add(1, Ordering::SeqCst), 0);
                        std::thread::sleep(std::time::Duration::from_millis(20));
                        assert_eq!(active.fetch_sub(1, Ordering::SeqCst), 1);
                        Ok(())
                    }).unwrap();
                });
            }
        });
        assert_eq!(active.load(Ordering::SeqCst), 0);
        assert!(first.path().join("design.json").is_file());
        assert!(second.path().join("design.json").is_file());
        crate::service::retract(&st);
    }

    #[test]
    fn failed_open_preserves_active_workspace_and_both_files() {
        let current = tempfile::tempdir().unwrap();
        let destination = tempfile::tempdir().unwrap();
        let st = AppState::create(Events::new().0, current.path().into());
        set_name(&st, "Keep current".into()).unwrap();
        let before = std::fs::read(current.path().join("design.json")).unwrap();
        let destination_file = destination.path().join("design.json");
        let pi = PiBridge::new();
        let ticket = pi.ticket();
        for text in ["broken", r#"{"version":999}"#] {
            std::fs::write(&destination_file, text).unwrap();
            assert!(switch_project(&st, &pi, destination.path().into(), None, LoadIntent::Open).is_err());
            assert_eq!(st.lock().project_dir, current.path().canonicalize().unwrap());
            assert_eq!(st.lock().store.design.name, "Keep current");
            assert_eq!(std::fs::read(current.path().join("design.json")).unwrap(), before);
            assert_eq!(std::fs::read_to_string(&destination_file).unwrap(), text);
            assert_eq!(pi.ticket(), ticket, "failed validation must not stop pi");
        }
        assert!(new_design(&st, &pi, current.path().display().to_string()).is_err());
        assert_eq!(std::fs::read(current.path().join("design.json")).unwrap(), before);
    }

    #[test]
    fn opening_a_copy_never_replaces_a_different_design_json() {
        let current = tempfile::tempdir().unwrap();
        let destination = tempfile::tempdir().unwrap();
        let st = AppState::create(Events::new().0, current.path().into());
        let original = Design { name: "Original".into(), ..Default::default() }.to_json();
        let copy = Design { name: "Incoming".into(), ..Default::default() }.to_json();
        std::fs::write(destination.path().join("design.json"), &original).unwrap();
        let source = destination.path().join("copy.json");
        std::fs::write(&source, &copy).unwrap();
        assert!(open_design(&st, &PiBridge::new(), source.display().to_string()).is_err());
        assert_eq!(std::fs::read_to_string(destination.path().join("design.json")).unwrap(), original);
        assert_eq!(std::fs::read_to_string(source).unwrap(), copy);
    }
}

