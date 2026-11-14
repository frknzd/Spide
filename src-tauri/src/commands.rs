//! Tauri commands: the UI's only way to talk to the core. Every command
//! expresses a user intent; results are broadcast as snapshots.

use crate::pi_bridge::PiBridge;
use crate::prompts;
use crate::state::{AppState, Snapshot};
use spide_core::model::{ExaminerMode, PartnerMode, Pos};
use spide_core::{Design, Op, Target};
use std::collections::BTreeMap;
use std::path::PathBuf;
use tauri::State;

type R<T = ()> = Result<T, String>;

fn done(st: &AppState) -> R {
    st.changed();
    Ok(())
}

#[tauri::command(async)]
pub fn terminal_read(pi: State<'_, PiBridge>, generation: Option<u64>, after: u64) -> R<crate::terminal::Frame> { pi.read(generation, after) }

#[tauri::command(async)]
pub fn terminal_write(pi: State<'_, PiBridge>, generation: u64, data: Vec<u8>) -> R { pi.terminal(Some(generation))?.write(&data) }

#[tauri::command(async)]
pub fn terminal_resize(pi: State<'_, PiBridge>, generation: u64, cols: u16, rows: u16) -> R { pi.terminal(Some(generation))?.resize(cols, rows) }

#[tauri::command]
pub fn terminal_context(pi: State<'_, PiBridge>, tab: String) { pi.context(tab); }

#[tauri::command]
pub fn get_snapshot(st: State<'_, AppState>) -> Snapshot {
    st.snapshot()
}

#[tauri::command(async)]
pub fn send_prompt(st: State<'_, AppState>, pi: State<'_, PiBridge>, text: String, tab: String) -> R {
    let generation = pi.ticket();
    let text = text.trim().to_string();
    if text.is_empty() {
        return Ok(());
    }
    let msg = prompts::user_prompt(&text, &tab, &st.lock().store.design);
    pi.prompt(generation, &st, msg, Some(text))
}

#[tauri::command(async)]
pub fn abort_agent(pi: State<'_, PiBridge>) -> R {
    pi.abort()
}

#[tauri::command(async)]
pub fn restart_agent(st: State<'_, AppState>, pi: State<'_, PiBridge>) -> R {
    pi.restart(&st)
}

#[tauri::command]
pub fn accept_proposal(st: State<'_, AppState>, id: String) -> R {
    st.lock().store.accept_proposal(&id).map_err(|e| e.0)?;
    done(&st)
}

#[tauri::command]
pub fn accept_all(st: State<'_, AppState>) -> R {
    st.lock().store.accept_all_proposals().map_err(|e| e.0)?;
    done(&st)
}

#[tauri::command]
pub fn reject_proposal(st: State<'_, AppState>, id: String) -> R {
    st.lock().store.reject_proposal(&id, None).map_err(|e| e.0)?;
    done(&st)
}

/// Instant prune (right-click / marquee). Feedback comes separately.
#[tauri::command]
pub fn prune(st: State<'_, AppState>, targets: Vec<Target>) -> R {
    if targets.is_empty() {
        return Ok(());
    }
    st.lock().store.prune(&targets, None).map_err(|e| e.0)?;
    done(&st)
}

/// "What didn't you like / what do you want instead?" → stored + sent to the agent.
#[tauri::command(async)]
pub fn prune_feedback(
    st: State<'_, AppState>,
    pi: State<'_, PiBridge>,
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

#[tauri::command]
pub fn restore(st: State<'_, AppState>, target: Target) -> R {
    st.lock().store.restore(&target).map_err(|e| e.0)?;
    done(&st)
}

#[tauri::command(async)]
pub fn submit_defense(
    st: State<'_, AppState>,
    pi: State<'_, PiBridge>,
    id: String,
    option: Option<String>,
    custom: Option<String>,
    defense: String,
) -> R {
    let generation = pi.ticket();
    let msg = {
        let mut s = st.lock();
        s.store.submit_defense(&id, option, custom, defense).map_err(|e| e.0)?;
        let dec = s.store.design.decisions.iter().find(|d| d.id == id).cloned().unwrap();
        prompts::examine(&dec, s.store.design.settings.examiner, &s.store.design)
    };
    st.changed();
    pi.prompt(generation, &st, msg, Some(format!("⚖ defending {id}")))
}

#[tauri::command]
pub fn overrule(st: State<'_, AppState>, id: String) -> R {
    st.lock().store.overrule(&id).map_err(|e| e.0)?;
    done(&st)
}

#[tauri::command]
pub fn dismiss_decision(st: State<'_, AppState>, id: String) -> R {
    st.lock().store.dismiss_decision(&id).map_err(|e| e.0)?;
    done(&st)
}

#[tauri::command]
pub fn undo(st: State<'_, AppState>) -> R {
    st.lock().store.undo();
    done(&st)
}

#[tauri::command]
pub fn redo(st: State<'_, AppState>) -> R {
    st.lock().store.redo();
    done(&st)
}

#[tauri::command]
pub fn set_layout(st: State<'_, AppState>, view: String, positions: BTreeMap<String, Pos>) -> R {
    let mut s = st.lock();
    s.store.set_layout(&view, positions);
    crate::state::save(&mut s);
    Ok(())
}

#[tauri::command]
pub fn set_examiner(st: State<'_, AppState>, strict: bool) -> R {
    st.lock().store.set_examiner(if strict { ExaminerMode::Strict } else { ExaminerMode::Lenient });
    done(&st)
}

/// Direct user edits (brief panel, inline edits): committed immediately, undoable.
#[tauri::command]
pub fn apply_user_ops(st: State<'_, AppState>, ops: Vec<Op>) -> R {
    st.lock().store.apply_user(ops).map_err(|e| e.0)?;
    done(&st)
}

#[tauri::command]
pub fn set_rules(st: State<'_, AppState>, rules: spide_core::rules::Rules) -> R {
    st.lock().store.set_rules(rules).map_err(|e| e.0)?;
    done(&st)
}

/// Ask the agent which classes an expected change would touch.
#[tauri::command(async)]
pub fn analyse_change(st: State<'_, AppState>, pi: State<'_, PiBridge>, name: String) -> R {
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
#[tauri::command(async)]
pub fn fix_issue(
    st: State<'_, AppState>,
    pi: State<'_, PiBridge>,
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

#[tauri::command]
pub fn set_partner(st: State<'_, AppState>, mode: String) -> R {
    let m = PartnerMode::parse(&mode).ok_or_else(|| format!("unknown mode {mode} (examiner | socratic | devils_advocate | sketch)"))?;
    st.lock().store.set_partner(m);
    done(&st)
}

#[tauri::command]
pub fn set_brief_skipped(st: State<'_, AppState>, on: bool) -> R {
    st.lock().store.set_brief_skipped(on);
    done(&st)
}

#[tauri::command]
pub fn set_show_pruned(st: State<'_, AppState>, on: bool) -> R {
    st.lock().store.set_show_pruned(on);
    done(&st)
}

#[tauri::command]
pub fn set_detail(st: State<'_, AppState>, view: String, level: u8) -> R {
    st.lock().store.set_detail(&view, level);
    done(&st)
}

#[tauri::command]
pub fn set_name(st: State<'_, AppState>, name: String) -> R {
    st.lock().store.design.name = name;
    done(&st)
}

/// Ask the agent to (re)generate a derived view.
#[tauri::command(async)]
pub fn regenerate(st: State<'_, AppState>, pi: State<'_, PiBridge>, view: String, scenario: Option<String>) -> R {
    let generation = pi.ticket();
    let msg = match view.as_str() {
        "usecase" => prompts::gen_use_cases(),
        "sequence" => prompts::gen_sequence(scenario.as_deref()),
        _ => return Err(format!("{view} is derived automatically")),
    };
    let label = match &scenario {
        Some(s) => format!("↻ sequence \"{s}\""),
        None => format!("↻ {view}"),
    };
    pi.prompt(generation, &st, msg, Some(label))
}

// ---------------------------------------------------------------- files

#[tauri::command(async)]
pub fn new_design(st: State<'_, AppState>, pi: State<'_, PiBridge>, dir: String) -> R {
    switch_project(&st, &pi, PathBuf::from(dir), None)
}

#[tauri::command(async)]
pub fn open_design(st: State<'_, AppState>, pi: State<'_, PiBridge>, path: String) -> R {
    let path = PathBuf::from(path);
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let design = Design::from_json(&text).map_err(|e| format!("not a Spide design: {e}"))?;
    let dir = path.parent().map(PathBuf::from).ok_or("bad path")?;
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

#[tauri::command(async)]
pub fn save_as(st: State<'_, AppState>, path: String) -> R {
    let json = st.lock().store.design.to_json();
    crate::state::write_atomic(&PathBuf::from(path), &json).map_err(|e| e.to_string())
}

fn switch_project(st: &AppState, pi: &PiBridge, dir: PathBuf, design: Option<Design>) -> R {
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    // Stop old control requests and reap the old process before replacing project state.
    pi.shutdown();
    {
        let mut s = st.lock();
        s.project_dir = dir.clone();
        s.last_written = None;
        s.last_history = None;
        s.branch = crate::branches::active(&dir);
        let d = match design {
            Some(d) => d,
            None => match std::fs::read_to_string(dir.join("design.json")).ok().and_then(|t| Design::from_json(&t).ok()) {
                Some(d) => d,
                None => Design {
                    name: dir.file_name().and_then(|n| n.to_str()).unwrap_or("Untitled").to_string(),
                    ..Design::default()
                },
            },
        };
        s.store.load(d);
    }
    crate::watcher::watch(st, &dir);
    st.changed();
    pi.restart(st)
}

#[tauri::command(async)]
pub fn export_png(svg: String, path: String, scale: f32) -> R {
    let png = spide_core::png::render(&svg, scale).map_err(|e| e.to_string())?;
    std::fs::write(path, png).map_err(|e| e.to_string())
}

#[tauri::command(async)]
pub fn export_pdf(svg: String, path: String) -> R {
    let pdf = spide_core::pdf::render(&svg).map_err(|e| e.to_string())?;
    std::fs::write(path, pdf).map_err(|e| e.to_string())
}

#[tauri::command(async)]
pub fn export_svg(svg: String, path: String) -> R {
    std::fs::write(path, svg).map_err(|e| e.to_string())
}

#[tauri::command(async)]
pub fn reveal_path(path: String) -> R {
    std::process::Command::new("open").arg("-R").arg(&path).spawn().map(|_| ()).map_err(|e| e.to_string())
}

#[tauri::command(async)]
pub fn export_plantuml(st: State<'_, AppState>, view: String, path: String) -> R {
    let snap = st.snapshot();
    let text = spide_core::plantuml::export(&view, &snap.view, &snap.metrics, &snap.view.layout)
        .map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| e.to_string())
}

#[tauri::command(async)]
pub fn export_archunit(st: State<'_, AppState>, path: String) -> R {
    let java = spide_core::archunit::archunit_test(&st.lock().store.design);
    std::fs::write(path, java).map_err(|e| e.to_string())
}

/// Java root package of the real code (`org.acme.library`).
#[tauri::command]
pub fn set_code_root(st: State<'_, AppState>, root: Option<String>) -> R {
    if let Some(r) = root.as_deref().filter(|r| !r.trim().is_empty()) {
        let ok = r.split('.').all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'));
        if !ok {
            return Err(format!("not a Java package name: {r}"));
        }
    }
    st.lock().store.set_code_root(root);
    done(&st)
}

/// ADRs (one markdown file per accepted/overruled decision) into `dir`.
#[tauri::command(async)]
pub fn export_adr(st: State<'_, AppState>, dir: String) -> R<Vec<String>> {
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
#[tauri::command(async)]
pub fn export_plantuml_all(st: State<'_, AppState>, dir: String) -> R<Vec<String>> {
    let snap = st.snapshot();
    let dir = PathBuf::from(dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let base = slug(&snap.view.name);
    let mut written = vec![];
    for view in ["system", "class", "domain", "package", "usecase", "sequence"] {
        let empty = match view {
            "system" => snap.view.containers.is_empty() && snap.view.systems.is_empty(),
            "usecase" => snap.view.use_cases.use_cases.is_empty(),
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

#[tauri::command(async)]
pub fn history_list(st: State<'_, AppState>) -> Vec<crate::history::Entry> {
    let dir = st.lock().project_dir.clone();
    crate::history::list(&dir)
}

#[tauri::command(async)]
pub fn history_at(st: State<'_, AppState>, rev: u64) -> R<Design> {
    let dir = st.lock().project_dir.clone();
    crate::history::at(&dir, rev).ok_or_else(|| format!("no history at revision {rev}"))
}

/// Bring the design back to how it was at `rev` — as a proposal the user accepts.
#[tauri::command(async)]
pub fn history_restore(st: State<'_, AppState>, rev: u64) -> R {
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
#[tauri::command(async)]
pub fn history_fork(st: State<'_, AppState>, rev: u64, name: String) -> R {
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

#[tauri::command(async)]
pub fn branch_list(st: State<'_, AppState>) -> Vec<BranchInfo> {
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
#[tauri::command(async)]
pub fn branch_create(st: State<'_, AppState>, name: String) -> R {
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
#[tauri::command(async)]
pub fn branch_switch(st: State<'_, AppState>, name: String) -> R {
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

#[tauri::command(async)]
pub fn branch_delete(st: State<'_, AppState>, name: String) -> R {
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

#[tauri::command(async)]
pub fn branch_compare(st: State<'_, AppState>, name: String) -> R<BranchCompare> {
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
#[tauri::command(async)]
pub fn branch_merge(st: State<'_, AppState>, name: String) -> R {
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

// ---------------------------------------------------------------- reality (code import)

fn reality_path(project: &std::path::Path) -> PathBuf {
    project.join(".spide").join("reality.json")
}

pub fn load_reality(project: &std::path::Path) -> Option<Design> {
    std::fs::read_to_string(reality_path(project)).ok().and_then(|t| Design::from_json(&t).ok())
}

#[derive(serde::Serialize)]
pub struct ImportSummary {
    pub files: usize,
    pub classes: usize,
    pub relations: usize,
    pub skipped: Vec<String>,
    pub code_root: Option<String>,
}

/// Import Java sources under `dir` as the read-only "reality" model (`.spide/reality.json`).
#[tauri::command(async)]
pub fn import_code(st: State<'_, AppState>, pi: State<'_, PiBridge>, dir: String) -> R<ImportSummary> {
    let generation = pi.ticket();
    let (project, root) = {
        let s = st.lock();
        (s.project_dir.clone(), s.store.design.settings.code_root.clone())
    };
    let r = spide_import::import_java(std::path::Path::new(&dir), root.as_deref())?;
    crate::state::write_atomic(&reality_path(&project), &r.design.to_json()).map_err(|e| e.to_string())?;
    let summary = ImportSummary {
        files: r.files,
        classes: r.design.classes.len(),
        relations: r.design.relations.len(),
        skipped: r.skipped,
        code_root: r.design.settings.code_root.clone(),
    };
    {
        let mut s = st.lock();
        if s.project_dir != project || pi.ticket() != generation {
            return Err("project/session changed during import; assessment cancelled".into());
        }
        if s.store.design.settings.code_root.is_none() {
            s.store.set_code_root(summary.code_root.clone());
        }
        s.store.design.settings.code_dir = Some(dir);
    }
    st.changed();
    // let the agent judge the real implementation (batched, in the background)
    let msg = prompts::assess_code(summary.classes);
    let _ = pi.prompt(generation, &st, msg, Some("⌕ judging the imported code".into()));
    Ok(summary)
}

#[derive(serde::Serialize)]
pub struct DriftReport {
    pub drift: spide_core::drift::Drift,
    /// The code judged like the design: both health scores, code-only issues, clones, design-vs-code disagreements.
    pub findings: spide_core::code::CodeFindings,
    /// The imported code with the design's rules applied (for drawing code-only classes).
    pub reality: Design,
    pub checklist: String,
    pub code_dir: Option<String>,
}

#[tauri::command(async)]
pub fn drift(st: State<'_, AppState>) -> R<DriftReport> {
    let (project, design) = {
        let s = st.lock();
        (s.project_dir.clone(), s.store.design.clone())
    };
    let reality = load_reality(&project).ok_or("no code imported yet — /import a source folder first")?;
    let drift = spide_core::drift::drift(&design, &reality);
    let checklist = spide_core::drift::checklist(&drift);
    let findings = spide_core::code::analyse(&design, &reality);
    Ok(DriftReport { reality: spide_core::drift::with_design_rules(&design, &reality), drift, findings, checklist, code_dir: design.settings.code_dir.clone() })
}

/// The imported implementation of a class (or one method), for the hover card.
#[tauri::command(async)]
pub fn code_snippet(st: State<'_, AppState>, class: String, method: Option<String>) -> R<String> {
    let (project, dir) = {
        let s = st.lock();
        (s.project_dir.clone(), s.store.design.settings.code_dir.clone())
    };
    let dir = dir.ok_or("no code imported yet — /import a source folder first")?;
    let reality = load_reality(&project).ok_or("no code imported yet")?;
    spide_core::code::snippet(&reality, std::path::Path::new(&dir), &class, method.as_deref())
}

/// What exists only in the code becomes a proposal (never removes anything from the design).
#[tauri::command(async)]
pub fn drift_adopt(st: State<'_, AppState>) -> R {
    let project = st.lock().project_dir.clone();
    let reality = load_reality(&project).ok_or("no code imported yet")?;
    {
        let mut s = st.lock();
        let ops = spide_core::drift::adopt_ops(&s.store.design, &reality);
        if ops.is_empty() {
            return Err("the code has nothing the design lacks".into());
        }
        s.store.propose_ops(ops, Some("adopt what exists only in the code".into()), "code").map_err(|e| e.0)?;
    }
    done(&st)
}

#[tauri::command]
pub fn reveal_project(st: State<'_, AppState>) -> R {
    let dir = st.lock().project_dir.clone();
    std::process::Command::new("open").arg(dir).spawn().map_err(|e| e.to_string())?;
    Ok(())
}

/// Dispatch a command by name with JSON args (dev browser harness).
pub fn dev_dispatch(st: &AppState, cmd: &str, a: serde_json::Value) -> R<serde_json::Value> {
    // Interactive pi has shell/file tools now. Never route prompt/PTY/lifecycle control
    // through the unauthenticated browser-preview HTTP harness, even in debug builds.
    if matches!(cmd, "send_prompt" | "abort_agent" | "restart_agent" | "prune_feedback" | "submit_defense" | "analyse_change" | "fix_issue" | "regenerate" | "import_code" | "new_design" | "open_design") || cmd.starts_with("terminal_") {
        return Err("Agent/terminal control is available only through desktop IPC".into());
    }
    use serde_json::Value;
    use tauri::Manager;
    let app = st.app.clone();
    let s = app.state::<AppState>();
    let pi = app.state::<PiBridge>();
    let str_ = |k: &str| a.get(k).and_then(Value::as_str).map(String::from);
    let req = |k: &str| str_(k).ok_or(format!("missing {k}"));
    let de = |k: &str| a.get(k).cloned().unwrap_or(Value::Null);
    let targets = || serde_json::from_value::<Vec<Target>>(de("targets")).map_err(|e| e.to_string());
    match cmd {
        "get_snapshot" => return serde_json::to_value(get_snapshot(s)).map_err(|e| e.to_string()),
        "send_prompt" => send_prompt(s, pi, req("text")?, req("tab")?),
        "abort_agent" => abort_agent(pi),
        "restart_agent" => restart_agent(s, pi),
        "accept_proposal" => accept_proposal(s, req("id")?),
        "accept_all" => accept_all(s),
        "reject_proposal" => reject_proposal(s, req("id")?),
        "prune" => prune(s, targets()?),
        "prune_feedback" => prune_feedback(s, pi, targets()?, req("text")?, req("tab")?),
        "restore" => restore(s, serde_json::from_value(de("target")).map_err(|e| e.to_string())?),
        "submit_defense" => submit_defense(s, pi, req("id")?, str_("option"), str_("custom"), req("defense")?),
        "overrule" => overrule(s, req("id")?),
        "dismiss_decision" => dismiss_decision(s, req("id")?),
        "undo" => undo(s),
        "redo" => redo(s),
        "set_layout" => set_layout(s, req("view")?, serde_json::from_value(de("positions")).map_err(|e| e.to_string())?),
        "set_examiner" => set_examiner(s, a.get("strict").and_then(Value::as_bool).unwrap_or(false)),
        "apply_user_ops" => apply_user_ops(s, serde_json::from_value(de("ops")).map_err(|e| e.to_string())?),
        "set_partner" => set_partner(s, req("mode")?),
        "branch_list" => return serde_json::to_value(branch_list(s)).map_err(|e| e.to_string()),
        "branch_create" => branch_create(s, req("name")?),
        "branch_switch" => branch_switch(s, req("name")?),
        "branch_delete" => branch_delete(s, req("name")?),
        "branch_compare" => return branch_compare(s, req("name")?).and_then(|c| serde_json::to_value(c).map_err(|e| e.to_string())),
        "branch_merge" => branch_merge(s, req("name")?),
        "history_list" => return serde_json::to_value(history_list(s)).map_err(|e| e.to_string()),
        "history_at" => return history_at(s, a.get("rev").and_then(Value::as_u64).ok_or("missing rev")?).and_then(|d| serde_json::to_value(d).map_err(|e| e.to_string())),
        "history_restore" => history_restore(s, a.get("rev").and_then(Value::as_u64).ok_or("missing rev")?),
        "history_fork" => history_fork(s, a.get("rev").and_then(Value::as_u64).ok_or("missing rev")?, req("name")?),
        "set_rules" => set_rules(s, serde_json::from_value(de("rules")).map_err(|e| e.to_string())?),
        "analyse_change" => analyse_change(s, pi, req("name")?),
        "fix_issue" => fix_issue(
            s,
            pi,
            req("message")?,
            str_("rule"),
            str_("fix_hint"),
            str_("kind"),
            a.get("targets").and_then(|v| serde_json::from_value(v.clone()).ok()),
        ),
        "set_brief_skipped" => set_brief_skipped(s, a.get("on").and_then(Value::as_bool).unwrap_or(false)),
        "set_show_pruned" => set_show_pruned(s, a.get("on").and_then(Value::as_bool).unwrap_or(false)),
        "set_name" => set_name(s, req("name")?),
        "set_detail" => set_detail(s, req("view")?, a.get("level").and_then(Value::as_u64).unwrap_or(3) as u8),
        "regenerate" => regenerate(s, pi, req("view")?, str_("scenario")),
        "new_design" => new_design(s, pi, req("dir")?),
        "open_design" => open_design(s, pi, req("path")?),
        "save_as" => save_as(s, req("path")?),
        "export_png" => export_png(req("svg")?, req("path")?, a.get("scale").and_then(Value::as_f64).unwrap_or(2.0) as f32),
        "export_pdf" => export_pdf(req("svg")?, req("path")?),
        "export_svg" => export_svg(req("svg")?, req("path")?),
        "export_plantuml" => export_plantuml(s, req("view")?, req("path")?),
        "export_archunit" => export_archunit(s, req("path")?),
        "set_code_root" => set_code_root(s, str_("root")),
        "drift" => return drift(s).and_then(|v| serde_json::to_value(v).map_err(|e| e.to_string())),
        "drift_adopt" => drift_adopt(s),
        "code_snippet" => return code_snippet(s, req("class")?, str_("method")).map(|v| serde_json::json!(v)),
        "import_code" => return import_code(s, pi, req("dir")?).and_then(|v| serde_json::to_value(v).map_err(|e| e.to_string())),
        "export_adr" => return export_adr(s, req("dir")?).map(|v| serde_json::json!(v)),
        "export_plantuml_all" => return export_plantuml_all(s, req("dir")?).map(|v| serde_json::json!(v)),
        _ => Err(format!("unknown command {cmd}")),
    }?;
    Ok(Value::Null)
}
