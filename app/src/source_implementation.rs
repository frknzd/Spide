//! Separately authorized agent work. Diagram undo never rolls back source files.
#[cfg(test)]
#[path = "source_implementation_tests.rs"]
mod tests;
use crate::{state::{AppState, Shared, write_atomic}, source_tracking::{self, Attachment}, pi_bridge::PiBridge};
use serde::{Serialize, Deserialize};
use spide_core::{Design, Op, DecisionState};
use std::{collections::BTreeMap, path::{Path, PathBuf}, time::{Duration, Instant}};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Task {
    pub id: String,
    pub decision: String,
    pub status: String,
    pub revision: u64,
    pub branch: String,
    pub session: u64,
    pub attachment: Attachment,
    pub approved_ops: Vec<Op>,
    pub expected_design: Design,
    pub bindings: BTreeMap<String, String>,
    pub drift_at_approval: serde_json::Value,
    pub extra_paths: Vec<String>,
    pub commands: Vec<String>,
    /// Exact pre-run bytes for approved text files, not repository-wide dirty diffs.
    pub baseline: BTreeMap<String, String>,
    pub changed_files: BTreeMap<String, String>,
    pub validation: Vec<String>,
    pub residual: Vec<String>,
    pub message: String,
}
fn path(project: &Path) -> PathBuf { project.join(".spide/implementations.json") }
pub fn load(project: &Path) -> Vec<Task> {
    let mut tasks: Vec<Task> = std::fs::read_to_string(path(project)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    let mut changed = false;
    for task in &mut tasks { if matches!(task.status.as_str(), "authorized" | "running" | "verifying") {
        task.status = "interrupted".into(); task.message = "Session ended; review partial edits before creating a new task.".into(); changed = true;
        if let Ok(now) = contents(&task.attachment, &task.extra_paths) { task.changed_files = diff(&task.baseline, &now); }
    } }
    if changed { let _ = persist(project, &tasks); }
    tasks
}
pub fn persist(project: &Path, tasks: &[Task]) -> Result<(), String> {
    write_atomic(&path(project), &serde_json::to_string_pretty(tasks).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn active(status: &str) -> bool { matches!(status, "awaiting approval" | "authorized" | "running" | "verifying") }
pub fn invalidate(s: &mut Shared) {
    let mut changed = false;
    for task in &mut s.implementations {
        if active(&task.status) && (task.revision != s.store.design.rev || task.branch != s.branch || s.source.attachment.as_ref() != Some(&task.attachment) || (task.session != 0 && task.session != s.pi_generation)) {
            task.status = "stale".into(); task.message = "Design, branch, scope or agent session changed; approval must be renewed. Any written source remains untouched.".into();
            if let Ok(now) = contents(&task.attachment, &task.extra_paths) { task.changed_files = diff(&task.baseline, &now); }
            changed = true;
        }
    }
    if changed { let _ = persist(&s.project_dir, &s.implementations); }
}
fn safe_path(repo: &Path, relative: &str) -> Result<PathBuf, String> {
    let p = Path::new(relative);
    if p.is_absolute() || p.components().any(|c| matches!(c, std::path::Component::ParentDir)) { return Err("Approved paths must be repository-relative".into()); }
    let joined = repo.join(p);
    let mut ancestor = joined.as_path();
    while !ancestor.exists() { ancestor = ancestor.parent().ok_or("Invalid path")?; }
    if !ancestor.canonicalize().map_err(|e| e.to_string())?.starts_with(repo) { return Err("Approved path escapes repository".into()); }
    Ok(joined)
}
fn contents(a: &Attachment, extras: &[String]) -> Result<BTreeMap<String, String>, String> {
    let mut paths = spide_import::scoped_files(&a.repository, &a.roots, &a.exclusions)?;
    for relative in extras { let p = safe_path(&a.repository, relative)?; if p.exists() { if !p.is_file() { return Err("Additional scope must list files, not directories".into()); } paths.push(p); } }
    let mut out = BTreeMap::new();
    for p in paths {
        let p = safe_path(&a.repository, p.strip_prefix(&a.repository).map_err(|e| e.to_string())?.to_str().ok_or("Non-UTF8 source path")?)?;
        out.insert(p.strip_prefix(&a.repository).unwrap().to_string_lossy().into_owned(), std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?);
    }
    Ok(out)
}
pub fn prepare(st: &AppState, decision: String, extra_paths: Vec<String>, commands: Vec<String>, planned_bindings: BTreeMap<String, String>) -> Result<String, String> {
    let mut s = st.lock();
    if s.source.status != "current" { return Err("Refresh source successfully before preparing implementation".into()); }
    if s.implementations.iter().any(|t| matches!(t.status.as_str(), "authorized" | "running" | "verifying")) { return Err("Finish or cancel the active implementation first".into()); }
    let dec = s.store.design.decisions.iter().find(|d| d.id == decision && matches!(d.state, DecisionState::Accepted | DecisionState::Overruled)).ok_or("Approve the design first")?.clone();
    if !dec.applied_ops.iter().any(|o| !o.touched_classes().is_empty()) { return Err("This decision has no source-mappable class changes".into()); }
    let attachment = s.source.attachment.clone().ok_or("Attach a repository first")?;
    for p in &extra_paths { safe_path(&attachment.repository, p)?; }
    if source_tracking::inventory(&attachment)? != s.source.files { return Err("Source changed since refresh; refresh before preparing approval".into()); }
    let baseline = contents(&attachment, &extra_paths)?;
    let drift_at_approval = source_tracking::context(&s);
    let mut bindings = s.source.bindings.clone(); bindings.extend(planned_bindings);
    let task = Task { id: format!("I{}-{}", spide_core::store::now_ms(), s.implementations.len()), decision, status: "awaiting approval".into(), revision: s.store.design.rev, branch: s.branch.clone(), attachment, approved_ops: dec.applied_ops, expected_design: s.store.design.clone(), bindings, drift_at_approval, extra_paths, commands, baseline, message: "Review scope, bindings, existing drift and commands. Authorizing this task allows source edits, not commits.".into(), ..Default::default() };
    let id = task.id.clone();
    s.implementations.push(task); persist(&s.project_dir, &s.implementations)?;
    drop(s); st.emit(); Ok(id)
}
fn task_is_current(s: &Shared, task: &Task) -> bool {
    s.store.design.rev == task.revision && s.branch == task.branch && s.source.attachment.as_ref() == Some(&task.attachment) && (task.session == 0 || task.session == s.pi_generation)
}
fn prompt(task: &Task) -> String {
    format!("[spide · separately approved source implementation · {}]\nBefore any edit, call begin_implementation with task_id=\"{}\". If it fails, STOP. Use get_source_context for current drift and symbols.\nRead repository and ancestor AGENTS.md instructions first. Repository: {}\nPermitted production roots: {:?}\nPermitted additional files: {:?}\nBindings: {:?}\nApproved design operations: {}\nPre-existing differences are not permission to fix unrelated work. Preserve existing edits, do not reset/stash/commit/switch branches. Request new approval for additional file scope. Native permissions remain active; this is workflow guidance, not a sandbox.\nImplement the visible approved operations. Do not run other validation commands than these user-approved commands: {:?}. When done, report edits and ask the user to Verify in Spide; Spide independently refreshes source and runs the approved commands. Do not claim behavioural equivalence. No examiner verdict or further design approval is needed for these already-approved operations.", task.id, task.id, task.attachment.repository.display(), task.attachment.roots, task.extra_paths, task.bindings, serde_json::to_string(&task.approved_ops).unwrap(), task.commands)
}
pub fn authorize(st: &AppState, pi: &PiBridge, id: &str) -> Result<(), String> {
    if !pi.ready() { return Err("pi integration is not ready".into()); }
    let generation = pi.ticket();
    authorize_with(st, id, generation, |message| pi.prompt(generation, st, message, Some(format!("Implement {id}"))))
}
fn authorize_with(st: &AppState, id: &str, session: u64, dispatch: impl FnOnce(String) -> Result<(), String>) -> Result<(), String> {
    let message = {
        let mut s = st.lock();
        let at = s.implementations.iter().position(|t| t.id == id).ok_or("Unknown task")?;
        let t = &s.implementations[at];
        if t.status != "awaiting approval" || !task_is_current(&s, t) { return Err("Task changed or already dispatched; prepare a new task".into()); }
        if contents(&t.attachment, &t.extra_paths)? != t.baseline { return Err("Source changed since review; prepare a new implementation approval".into()); }
        let t = &mut s.implementations[at]; t.status = "authorized".into(); t.session = session;
        let message = prompt(t);
        persist(&s.project_dir, &s.implementations)?;
        message
    };
    let result = dispatch(message);
    if let Err(e) = &result { update(st, id, |t| { t.status = "interrupted".into(); t.message = format!("Dispatch not retried (may have been received): {e}"); })?; }
    st.emit(); result
}
/// Called by the agent immediately before editing; stale queued follow-ups cannot authorize themselves.
pub fn begin(st: &AppState, id: &str) -> Result<String, String> {
    let mut s = st.lock();
    let at = s.implementations.iter().position(|t| t.id == id).ok_or("Unknown task")?;
    let task = &s.implementations[at];
    if task.status != "authorized" || !task_is_current(&s, task) || contents(&task.attachment, &task.extra_paths)? != task.baseline { return Err("Approval is stale or already consumed. Stop and request new approval.".into()); }
    let task = &mut s.implementations[at]; task.status = "running".into(); task.message = "Agent implementing approved scope. Source edits are not diagram undoable.".into();
    let result = prompt(task);
    persist(&s.project_dir, &s.implementations)?;
    drop(s); st.emit(); Ok(result)
}
fn update(st: &AppState, id: &str, f: impl FnOnce(&mut Task)) -> Result<(), String> {
    let mut s = st.lock(); let t = s.implementations.iter_mut().find(|t| t.id == id).ok_or("Unknown task")?; f(t);
    persist(&s.project_dir, &s.implementations)
}
pub fn cancel(st: &AppState, pi: &PiBridge, id: &str) -> Result<(), String> {
    let task = st.lock().implementations.iter().find(|t| t.id == id).cloned().ok_or("Unknown task")?;
    update(st, id, |t| { t.status = "cancelled".into(); t.message = "Cancelled. Already-written edits are retained; review the diff. No rollback performed.".into(); })?;
    let result = if task.session == pi.ticket() && matches!(task.status.as_str(), "authorized" | "running") { pi.abort() } else { Ok(()) };
    collect_diff(st, id)?; st.emit(); result
}
fn diff(before: &BTreeMap<String, String>, after: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    before.keys().chain(after.keys()).filter(|p| before.get(*p) != after.get(*p)).map(|p| {
        let a = before.get(p).map(String::as_str).unwrap_or(""); let b = after.get(p).map(String::as_str).unwrap_or("");
        // A lossless before/after record relative to the task baseline, including already-dirty files.
        (p.clone(), format!("--- Before task\n{a}\n+++ After task\n{b}"))
    }).collect()
}
fn collect_diff(st: &AppState, id: &str) -> Result<(), String> {
    let task = st.lock().implementations.iter().find(|t| t.id == id).cloned().ok_or("Unknown task")?;
    let now = contents(&task.attachment, &task.extra_paths)?;
    update(st, id, |t| t.changed_files = diff(&task.baseline, &now))
}
fn still_verifying(st: &AppState, project: &Path, id: &str) -> bool {
    let s = st.lock(); s.project_dir == project && s.implementations.iter().any(|t| t.id == id && t.status == "verifying" && task_is_current(&s, t))
}
fn validate_command(st: &AppState, project: &Path, id: &str, repo: &Path, command: &str) -> Result<String, String> {
    use std::process::{Command, Stdio};
    use std::os::unix::process::CommandExt;
    let stdout = tempfile::tempfile().map_err(|e| e.to_string())?; let stderr = tempfile::tempfile().map_err(|e| e.to_string())?;
    let mut child = Command::new("/bin/sh").arg("-c").arg(command).current_dir(repo).process_group(0).stdout(Stdio::from(stdout.try_clone().unwrap())).stderr(Stdio::from(stderr.try_clone().unwrap())).spawn().map_err(|e| e.to_string())?;
    let start = Instant::now();
    let status = loop {
        if !still_verifying(st, project, id) || start.elapsed() > Duration::from_secs(300) { unsafe { libc::kill(-(child.id() as i32), libc::SIGTERM); } let _ = child.kill(); let _ = child.wait(); return Err("Validation cancelled/stale or exceeded 5 minutes; partial edits remain".into()); }
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? { break status; }
        std::thread::sleep(Duration::from_millis(50));
    };
    use std::io::{Read, Seek};
    let read = |mut file: std::fs::File| { let _ = file.rewind(); let mut s = String::new(); let _ = file.take(16000).read_to_string(&mut s); s };
    let result = format!("$ {command}\n{status}\n{}\n{}", read(stdout), read(stderr));
    if status.success() { Ok(result) } else { Err(result) }
}
pub fn verify(st: &AppState, id: &str) -> Result<(), String> {
    let (project, task) = {
        let mut s = st.lock();
        let at = s.implementations.iter().position(|t| t.id == id).ok_or("Unknown task")?;
        if s.implementations[at].status != "running" || !task_is_current(&s, &s.implementations[at]) { return Err("Task is not running/current".into()); }
        s.implementations[at].status = "verifying".into();
        persist(&s.project_dir, &s.implementations)?;
        (s.project_dir.clone(), s.implementations[at].clone())
    };
    st.emit(); let st = st.clone();
    std::thread::spawn(move || {
        let result = (|| {
            let mut validation = vec![];
            for command in &task.commands {
                if !still_verifying(&st, &project, &task.id) { return Err("Task cancelled or stale".into()); }
                let outcome = validate_command(&st, &project, &task.id, &task.attachment.repository, command);
                validation.push(outcome.clone().unwrap_or_else(|e| e));
                if still_verifying(&st, &project, &task.id) { let _ = update(&st, &task.id, |t| t.validation = validation.clone()); }
                outcome?;
            }
            let files = source_tracking::inventory(&task.attachment)?;
            let imported = spide_import::import_java_scoped(&task.attachment.repository, &task.attachment.roots, &task.attachment.exclusions, task.expected_design.settings.code_root.as_deref());
            let code = match imported { Ok(r) if r.skipped.is_empty() => r.design, Ok(r) => return Err(format!("Partial parse: {:?}", r.skipped)), Err(_) if files.is_empty() => Design::default(), Err(e) => return Err(e) };
            if source_tracking::inventory(&task.attachment)? != files { return Err("Source changed during verification; review and verify again".into()); }
            let residual = implementation_mismatches(&task, &code);
            Ok((validation, residual, code, files))
        })();
        if !still_verifying(&st, &project, &task.id) { return; }
        let _ = collect_diff(&st, &task.id);
        {
            let mut s = st.lock();
            if s.project_dir != project || !task_is_current(&s, &task) || !s.implementations.iter().any(|t| t.id == task.id && t.status == "verifying") { return; }
            match result {
                Ok((validation, residual, code, files)) => {
                    s.source.snapshot = Some(code); s.source.files = files; s.source.status = "current".into(); s.source.warnings.clear(); s.source.generation += 1;
                    s.source.bindings = task.bindings.clone();
                    let t = s.implementations.iter_mut().find(|t| t.id == task.id).unwrap();
                    t.status = if residual.is_empty() { "completed" } else { "failed" }.into();
                    t.validation = validation; t.residual = residual;
                    t.message = if task.commands.is_empty() { "Structural delta checked; no validation commands were approved. Behaviour remains unverified." } else { "Approved validation commands ran; structural delta checked. Behavioural equivalence is not guaranteed." }.into();
                    let _ = source_tracking::persist(&project, &s.source);
                }
                Err(e) => { let t = s.implementations.iter_mut().find(|t| t.id == task.id).unwrap(); t.status = "failed".into(); t.message = e; }
            }
            let _ = persist(&project, &s.implementations);
        }
        source_tracking::refresh_async(&st);
        st.emit();
    });
    Ok(())
}

fn implementation_mismatches(task: &Task, code: &Design) -> Vec<String> {
    let mapped = spide_core::source::mapped_reality(&task.expected_design, code, &task.bindings);
    let mut residual = vec![];
    for op in &task.approved_ops {
        let ok = match op {
            Op::AddClass { name, .. } => mapped.live_class(name).is_some_and(|c| task.expected_design.live_class(name).is_some_and(|d| d.methods.iter().all(|m| c.methods.iter().any(|s| spide_core::source::signature(m) == spide_core::source::signature(s) && m.returns == s.returns)) && d.attributes.iter().all(|a| c.attributes.iter().any(|s| a.name == s.name && a.ty == s.ty)))),
            Op::RemoveClass { name } => task.bindings.get(name).is_some_and(|symbol| !code.classes.iter().any(|c| c.source.as_ref().is_some_and(|s| &s.symbol == symbol))),
            Op::AddMethod { class, method } | Op::UpdateMethod { class, method, .. } => mapped.live_class(class).is_some_and(|c| c.methods.iter().any(|m| spide_core::source::signature(m) == spide_core::source::signature(method) && m.returns == method.returns && m.visibility == method.visibility && m.is_static == method.is_static)),
            Op::RemoveMethod { class, name } => mapped.live_class(class).is_some_and(|c| !c.methods.iter().any(|m| &m.name == name || task.bindings.get(&format!("{class}.{name}")).is_some_and(|symbol| m.source.as_ref().is_some_and(|s| &s.symbol == symbol)))),
            Op::AddAttribute { class, attribute } | Op::UpdateAttribute { class, attribute, .. } => mapped.live_class(class).is_some_and(|c| c.attributes.iter().any(|a| a.name == attribute.name && a.ty == attribute.ty && a.visibility == attribute.visibility && a.is_static == attribute.is_static)),
            Op::RemoveAttribute { class, name } => mapped.live_class(class).is_some_and(|c| !c.attributes.iter().any(|a| &a.name == name)),
            Op::MoveToPackage { class, package } => mapped.live_class(class).is_some_and(|c| &c.package == package),
            Op::AddRelation { from, to, kind, .. } | Op::UpdateRelation { from, to, kind, .. } => mapped.live_relations().any(|r| &r.from == from && &r.to == to && (&r.kind == kind || matches!(kind, spide_core::RelationKind::Composition | spide_core::RelationKind::Aggregation) && r.kind == spide_core::RelationKind::Association)),
            Op::RemoveRelation { from, to, .. } => !mapped.live_relations().any(|r| &r.from == from && &r.to == to),
            // Design-only metadata has no implementation analogue; unsupported structural ops
            // stay explicit instead of falsely declaring synchronization.
            Op::UpdateClass { stereotype, package, is_abstract, .. } if stereotype.is_none() && package.is_none() && is_abstract.is_none() => true,
            _ => { residual.push(format!("Manual verification required: {}", serde_json::to_string(op).unwrap())); continue; }
        };
        if !ok { residual.push(format!("Not implemented / unmapped: {}", serde_json::to_string(op).unwrap())); }
    }
    residual
}
