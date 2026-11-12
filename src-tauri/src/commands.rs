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

#[tauri::command]
pub fn get_snapshot(st: State<'_, AppState>) -> Snapshot {
    st.snapshot()
}

#[tauri::command]
pub fn send_prompt(st: State<'_, AppState>, pi: State<'_, PiBridge>, text: String, tab: String) -> R {
    let text = text.trim().to_string();
    if text.is_empty() {
        return Ok(());
    }
    let msg = prompts::user_prompt(&text, &tab, &st.lock().store.design);
    pi.prompt(&st, msg, Some(text))
}

#[tauri::command]
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
#[tauri::command]
pub fn prune_feedback(
    st: State<'_, AppState>,
    pi: State<'_, PiBridge>,
    targets: Vec<Target>,
    text: String,
    tab: String,
) -> R {
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
    pi.prompt(&st, msg, Some(format!("✂ {text}")))
}

#[tauri::command]
pub fn restore(st: State<'_, AppState>, target: Target) -> R {
    st.lock().store.restore(&target).map_err(|e| e.0)?;
    done(&st)
}

#[tauri::command]
pub fn submit_defense(
    st: State<'_, AppState>,
    pi: State<'_, PiBridge>,
    id: String,
    option: Option<String>,
    custom: Option<String>,
    defense: String,
) -> R {
    let msg = {
        let mut s = st.lock();
        s.store.submit_defense(&id, option, custom, defense).map_err(|e| e.0)?;
        let dec = s.store.design.decisions.iter().find(|d| d.id == id).cloned().unwrap();
        prompts::examine(&dec, s.store.design.settings.examiner, &s.store.design)
    };
    st.changed();
    pi.prompt(&st, msg, Some(format!("⚖ defending {id}")))
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
pub fn set_name(st: State<'_, AppState>, name: String) -> R {
    st.lock().store.design.name = name;
    done(&st)
}

/// Ask the agent to (re)generate a derived view.
#[tauri::command]
pub fn regenerate(st: State<'_, AppState>, pi: State<'_, PiBridge>, view: String, scenario: Option<String>) -> R {
    let msg = match view.as_str() {
        "usecase" => prompts::gen_use_cases(),
        "sequence" => prompts::gen_sequence(scenario.as_deref()),
        _ => return Err(format!("{view} is derived automatically")),
    };
    let label = match &scenario {
        Some(s) => format!("↻ sequence \"{s}\""),
        None => format!("↻ {view}"),
    };
    pi.prompt(&st, msg, Some(label))
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
    {
        let mut s = st.lock();
        s.project_dir = dir.clone();
        s.last_written = None;
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
pub fn export_plantuml(st: State<'_, AppState>, view: String, path: String) -> R {
    let snap = st.snapshot();
    let text = spide_core::plantuml::export(&view, &snap.view, &snap.metrics, &snap.view.layout)
        .map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| e.to_string())
}

/// PlantUML for every diagram into `dir`; returns the written file paths.
#[tauri::command(async)]
pub fn export_plantuml_all(st: State<'_, AppState>, dir: String) -> R<Vec<String>> {
    let snap = st.snapshot();
    let dir = PathBuf::from(dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let base = slug(&snap.view.name);
    let mut written = vec![];
    for view in ["class", "domain", "package", "usecase", "sequence"] {
        let empty = match view {
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

#[tauri::command]
pub fn reveal_project(st: State<'_, AppState>) -> R {
    let dir = st.lock().project_dir.clone();
    std::process::Command::new("open").arg(dir).spawn().map_err(|e| e.to_string())?;
    Ok(())
}

/// Dispatch a command by name with JSON args (dev browser harness).
pub fn dev_dispatch(st: &AppState, cmd: &str, a: serde_json::Value) -> R<serde_json::Value> {
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
        "set_brief_skipped" => set_brief_skipped(s, a.get("on").and_then(Value::as_bool).unwrap_or(false)),
        "set_show_pruned" => set_show_pruned(s, a.get("on").and_then(Value::as_bool).unwrap_or(false)),
        "set_name" => set_name(s, req("name")?),
        "regenerate" => regenerate(s, pi, req("view")?, str_("scenario")),
        "new_design" => new_design(s, pi, req("dir")?),
        "open_design" => open_design(s, pi, req("path")?),
        "save_as" => save_as(s, req("path")?),
        "export_png" => export_png(req("svg")?, req("path")?, a.get("scale").and_then(Value::as_f64).unwrap_or(2.0) as f32),
        "export_plantuml" => export_plantuml(s, req("view")?, req("path")?),
        "export_plantuml_all" => return export_plantuml_all(s, req("dir")?).map(|v| serde_json::json!(v)),
        _ => Err(format!("unknown command {cmd}")),
    }?;
    Ok(Value::Null)
}
