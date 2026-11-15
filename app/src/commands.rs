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
    st.changed();
    Ok(())
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
    st.lock().store.update_proposal(&id, expected_rev, ops, rationale, decision, option).map_err(|e| e.0)?;
    done(&st)
}

pub fn accept_proposal(st: &AppState, id: String, expected_rev: Option<u64>) -> R {
    { let mut state = st.lock();
      if expected_rev.is_some_and(|rev| rev != state.store.design.rev) { return Err("Workspace changed. Review the latest plan before accepting.".into()); }
      state.store.accept_proposal(&id).map_err(|e| e.0)?;
    }
    done(&st)
}

pub fn approve_decision(st: &AppState, id: String, rev: u64, notes: String) -> R {
    st.lock().store.approve_decision(&id, rev, notes).map_err(|e| e.0)?;
    done(st)
}

pub fn select_decision_option(st: &AppState, id: String, option: String, rev: u64) -> R {
    st.lock().store.select_decision_option(&id, &option, rev).map_err(|e| e.0)?;
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
    st.lock().store.accept_all_proposals().map_err(|e| e.0)?;
    done(&st)
}

pub fn reject_proposal(st: &AppState, id: String, expected_rev: Option<u64>) -> R {
    { let mut state = st.lock();
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
    st.lock().store.prune(&targets, None).map_err(|e| e.0)?;
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
        let mut s = st.lock();
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
    st.changed();
    let msg = prompts::prune_feedback(&targets, &text, &tab);
    pi.prompt(generation, &st, msg, Some(format!("✂ {text}")))
}

pub fn restore(st: &AppState, target: Target) -> R {
    st.lock().store.restore(&target).map_err(|e| e.0)?;
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
    st.lock().store.overrule(&id).map_err(|e| e.0)?;
    done(&st)
}

pub fn dismiss_decision(st: &AppState, id: String) -> R {
    st.lock().store.dismiss_decision(&id).map_err(|e| e.0)?;
    done(&st)
}

pub fn undo(st: &AppState) -> R {
    st.lock().store.undo();
    done(&st)
}

pub fn redo(st: &AppState) -> R {
    st.lock().store.redo();
    done(&st)
}

pub fn set_layout(st: &AppState, view: String, positions: BTreeMap<String, Pos>) -> R {
    let mut s = st.lock();
    s.store.set_layout(&view, positions);
    crate::state::save(&mut s);
    Ok(())
}

pub fn set_examiner(st: &AppState, strict: bool) -> R {
    st.lock().store.set_examiner(if strict { ExaminerMode::Strict } else { ExaminerMode::Lenient });
    done(&st)
}

/// Direct user edits (brief panel, inline edits): committed immediately, undoable.
pub fn apply_user_ops(st: &AppState, ops: Vec<Op>) -> R {
    st.lock().store.apply_user(ops).map_err(|e| e.0)?;
    done(&st)
}

pub fn set_rules(st: &AppState, rules: spide_core::rules::Rules) -> R {
    st.lock().store.set_rules(rules).map_err(|e| e.0)?;
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

pub fn set_partner(st: &AppState, mode: String) -> R {
    let m = PartnerMode::parse(&mode).ok_or_else(|| format!("unknown mode {mode} (examiner | socratic | devils_advocate | sketch)"))?;
    st.lock().store.set_partner(m);
    done(&st)
}

pub fn set_brief_skipped(st: &AppState, on: bool) -> R {
    st.lock().store.set_brief_skipped(on);
    done(&st)
}

pub fn set_show_pruned(st: &AppState, on: bool) -> R {
    st.lock().store.set_show_pruned(on);
    done(&st)
}

pub fn set_detail(st: &AppState, view: String, level: u8) -> R {
    st.lock().store.set_detail(&view, level);
    done(&st)
}

pub fn set_name(st: &AppState, name: String) -> R {
    st.lock().store.design.name = name;
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
    switch_project(&st, &pi, PathBuf::from(dir), None)
}

pub fn open_design(st: &AppState, pi: &PiBridge, path: String) -> R {
    let path = PathBuf::from(path);
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let design = Design::from_json(&text).map_err(|e| format!("not a Spide design: {e}"))?;
    let dir = path.parent().map(PathBuf::from).ok_or("bad path")?;
    // `<root>/spide/design.json` of a Spide Project: open the project itself
    let root = spide_core::project::resolve_root(&dir);
    if root != dir && path.file_name().is_some_and(|n| n == "design.json") {
        return switch_project(st, pi, root, None);
    }
    // a design.json elsewhere is copied into its folder's design.json; never clobber a different one
    let target = dir.join("design.json");
    if path.file_name() != target.file_name() {
        if let Ok(existing) = std::fs::read_to_string(&target) {
            if existing != text {
                let backup = dir.join(format!("design.{}.bak.json", spide_core::store::now_ms()));
                std::fs::copy(&target, &backup).map_err(|e| format!("could not back up {}: {e}", target.display()))?;
                st.toast("info", format!("existing design.json backed up to {}", backup.display()));
            }
        }
    }
    switch_project(&st, &pi, dir, Some(design))
}

pub fn save_as(st: &AppState, path: String) -> R {
    let json = st.lock().store.design.to_json();
    crate::state::write_atomic(&PathBuf::from(path), &json).map_err(|e| e.to_string())
}

fn switch_project(st: &AppState, pi: &PiBridge, dir: PathBuf, design: Option<Design>) -> R {
    let dir = spide_core::project::resolve_root(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    // Stop old control requests and reap the old process before replacing project state.
    pi.shutdown();
    {
        let mut s = st.lock();
        s.project_dir = dir.clone();
        s.last_written = None;
        s.last_history = None;
        s.branch = crate::branches::active(&dir);
        s.project = crate::project::load(&dir);
        let d = match design {
            Some(d) => d,
            None => match std::fs::read_to_string(s.design_path()).ok().and_then(|t| Design::from_json(&t).ok()) {
                Some(d) => d,
                None => Design {
                    name: dir.file_name().and_then(|n| n.to_str()).unwrap_or("Untitled").to_string(),
                    ..Design::default()
                },
            },
        };
        s.store.load(d);
    }
    let design_path = st.lock().design_path();
    crate::watcher::watch(st, &design_path);
    crate::project::watch(st);
    crate::project::refresh_async(st);
    st.changed();
    pi.restart(st)
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
    std::fs::write(path, svg).map_err(|e| e.to_string())
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

/// Java root package of the real code (`org.acme.library`).
pub fn set_code_root(st: &AppState, root: Option<String>) -> R {
    if let Some(r) = root.as_deref().filter(|r| !r.trim().is_empty()) {
        let ok = r.split('.').all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'));
        if !ok {
            return Err(format!("not a Java package name: {r}"));
        }
    }
    let manifest = st.lock().project.as_ref().map(|p| p.manifest.clone());
    if let Some(mut m) = manifest {
        m.code_root = root.map(|r| r.trim().to_string()).filter(|r| !r.is_empty());
        return save_manifest(st, m);
    }
    st.lock().store.set_code_root(root);
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
        let mut s = st.lock();
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
    let dir = st.lock().project_dir.clone();
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
    pub health: u8,
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
        health: spide_core::metrics::compute(d, false).health,
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
        let s = st.lock();
        (s.project_dir.clone(), s.branch.clone(), s.store.design.clone())
    };
    if name == active {
        return Err(format!("already on {name}"));
    }
    let next = crate::branches::read(&dir, name)?;
    crate::branches::write(&dir, &active, &cur)?;
    {
        let mut s = st.lock();
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
        let s = st.lock();
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
    pub health: u8,
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
    Ok(BranchCompare { name, health: impact.health_after, design: other, impact })
}

/// Bring another branch's changes in as proposals: additions/changes and removals separately,
/// so the user can take one without the other.
pub fn branch_merge(st: &AppState, name: String) -> R {
    let dir = st.lock().project_dir.clone();
    let other = crate::branches::read(&dir, &name)?;
    {
        let mut s = st.lock();
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
        (s.store.design.clone(), s.project_dir.clone())
    };
    let added = crate::project::create(&root, &manifest, &design, Some(&from), git_exclude)?;
    switch_project(st, pi, root, None)?;
    Ok(added)
}

pub fn open_project(st: &AppState, pi: &PiBridge, root: String) -> R {
    let root = spide_core::project::resolve_root(&PathBuf::from(root));
    if !spide_core::project::is_project(&root) {
        return Err(format!("{} has no spide/project.json — create a Spide Project there first", root.display()));
    }
    switch_project(st, pi, root, None)
}

/// Save an edited manifest and index again.
pub fn save_manifest(st: &AppState, manifest: spide_core::project::Manifest) -> R {
    {
        let mut s = st.lock();
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
        let mut s = st.lock();
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

