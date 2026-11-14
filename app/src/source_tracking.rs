//! Optional repository attachment. One atomic local record owns scope, snapshot and baselines.
use crate::state::{AppState, write_atomic};
use serde::{Serialize, Deserialize};
use spide_core::Design;
use std::{collections::BTreeMap, path::{Path, PathBuf}, sync::{Mutex, mpsc}, time::Duration};
use notify::{Watcher, RecursiveMode, RecommendedWatcher};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Attachment {
    pub repository: PathBuf,
    pub roots: Vec<String>,
    pub exclusions: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct SourceState {
    pub attachment: Option<Attachment>,
    pub generation: u64,
    pub status: String,
    pub snapshot: Option<Design>,
    pub files: BTreeMap<String, String>,
    pub warnings: Vec<String>,
    /// Design class/member key -> fully qualified Java symbol. Never infer ambiguous bindings.
    pub bindings: BTreeMap<String, String>,
    pub acknowledged_design: Option<Design>,
    pub acknowledged_source: Option<Design>,
}
fn path(project: &Path) -> PathBuf { project.join(".spide/source.json") }
pub fn load(project: &Path) -> SourceState {
    match std::fs::read_to_string(path(project)) {
        Ok(s) => {
            let mut state: SourceState = serde_json::from_str(&s).unwrap_or_else(|e| SourceState { status: "error".into(), warnings: vec![format!("Invalid source state: {e}")], ..Default::default() });
            if let Some(a) = &state.attachment {
                state.status = if a.repository.is_dir() { "stale (refresh required after opening)" } else { "repository missing — relink" }.into();
            }
            state
        },
        Err(_) => SourceState::default(),
    }
}
pub fn persist(project: &Path, source: &SourceState) -> Result<(), String> {
    write_atomic(&path(project), &serde_json::to_string_pretty(source).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
pub fn fingerprint(bytes: &[u8]) -> String {
    // Deterministic content checksum for change detection, not a security primitive.
    let mut hash = 0xcbf29ce484222325u64;
    for b in bytes { hash = (hash ^ *b as u64).wrapping_mul(0x100000001b3); }
    format!("{hash:016x}")
}
pub fn inventory(a: &Attachment) -> Result<BTreeMap<String, String>, String> {
    spide_import::scoped_files(&a.repository, &a.roots, &a.exclusions)?.into_iter().map(|p| {
        let relative = p.strip_prefix(&a.repository).map_err(|e| e.to_string())?.to_string_lossy().into_owned();
        let bytes = std::fs::read(&p).map_err(|e| e.to_string())?;
        Ok((relative, fingerprint(&bytes)))
    }).collect()
}
pub fn preview(mut a: Attachment) -> Result<(Attachment, usize), String> {
    a.repository = a.repository.canonicalize().map_err(|e| e.to_string())?;
    let n = spide_import::scoped_files(&a.repository, &a.roots, &a.exclusions)?.len();
    if n == 0 { return Err("No production Java files in selected scope".into()); }
    Ok((a, n))
}
pub fn attach(st: &AppState, a: Attachment) -> Result<(), String> {
    let (a, _) = preview(a)?;
    {
        let mut s = st.lock();
        let mut next = SourceState { attachment: Some(a), generation: s.source.generation + 1, status: "refreshing".into(), ..Default::default() };
        if next.attachment == s.source.attachment { next = s.source.clone(); next.generation += 1; }
        persist(&s.project_dir, &next)?; s.source = next;
    }
    watch(st); refresh_async(st); st.emit(); Ok(())
}
pub fn detach(st: &AppState) -> Result<(), String> {
    {
        let mut s = st.lock(); let mut next = s.source.clone();
        next.attachment = None; next.generation += 1; next.status = "detached (snapshot inactive)".into();
        persist(&s.project_dir, &next)?; s.source = next;
    }
    watch(st); st.emit(); Ok(())
}
pub fn refresh_async(st: &AppState) {
    let request = {
        let mut s = st.lock();
        let Some(a) = s.source.attachment.clone() else { return };
        s.source.generation += 1;
        s.source.status = "refreshing (previous evidence stale)".into();
        (s.project_dir.clone(), s.source.generation, a, s.store.design.settings.code_root.clone())
    };
    st.emit();
    let st = st.clone();
    std::thread::spawn(move || {
        let (project, generation, a, root) = request;
        let result = (|| {
            let before = inventory(&a)?;
            let imported = spide_import::import_java_scoped(&a.repository, &a.roots, &a.exclusions, root.as_deref());
            let after = inventory(&a)?;
            if before != after { return Err("Files changed during import; refresh again".into()); }
            // Deleting the last file is a valid empty observation, not an old snapshot.
            match imported {
                Ok(r) => Ok((r.design, after, r.skipped)),
                Err(_) if after.is_empty() => Ok((Design::default(), after, vec![])),
                Err(e) => Err(e),
            }
        })();
        {
            let mut s = st.lock();
            if s.project_dir != project || s.source.generation != generation || s.source.attachment.as_ref() != Some(&a) { return; }
            let mut next = s.source.clone();
            match result {
                Ok((design, files, warnings)) => {
                    next.status = if warnings.is_empty() { "current" } else { "partial (not synchronized)" }.into();
                    next.snapshot = Some(design); next.files = files; next.warnings = warnings;
                }
                Err(e) => { next.status = "stale/error".into(); next.warnings = vec![e]; }
            }
            if let Err(e) = persist(&project, &next) { next.status = "save error".into(); next.warnings.push(e); }
            s.source = next;
        }
        st.emit();
    });
}

pub fn bind(st: &AppState, key: String, symbol: String, expected_generation: u64) -> Result<(), String> {
    let mut s = st.lock();
    if s.source.generation != expected_generation { return Err("Source changed; select again".into()); }
    let snapshot = s.source.snapshot.as_ref().ok_or("Refresh source first")?;
    let valid = snapshot.classes.iter().any(|c| c.source.as_ref().is_some_and(|x| x.symbol == symbol) || c.methods.iter().any(|m| m.source.as_ref().is_some_and(|x| x.symbol == symbol)));
    if !valid { return Err("Unknown source symbol".into()); }
    if s.source.bindings.iter().any(|(k, v)| k != &key && v == &symbol) { return Err("Symbol already bound to another element".into()); }
    let mut next = s.source.clone(); next.bindings.insert(key, symbol); next.generation += 1;
    // Changing identity invalidates the three-way baseline, never silently reuses it.
    next.acknowledged_design = None; next.acknowledged_source = None;
    persist(&s.project_dir, &next)?; s.source = next;
    drop(s); st.emit(); Ok(())
}
pub fn context(s: &crate::state::Shared) -> serde_json::Value {
    let mapped = mapped(&s.store.design, &s.source);
    let reconciliation = mapped.as_ref().map(|code| spide_core::source::reconcile(&s.store.design, code, s.source.acknowledged_design.as_ref(), s.source.acknowledged_source.as_ref(), &s.source.bindings));
    serde_json::json!({
        "attachment":s.source.attachment, "status":s.source.status, "generation":s.source.generation,
        "bindings":s.source.bindings, "warnings":s.source.warnings, "files":s.source.files,
        "symbols":s.source.snapshot.as_ref().map(|d| d.classes.iter().map(|c| serde_json::json!({"type":c.source,"members":c.methods.iter().map(|m| &m.source).collect::<Vec<_>>() })).collect::<Vec<_>>()),
        "drift": mapped.as_ref().map(|c| spide_core::drift::drift(&s.store.design,c)),
        "reconciliation":reconciliation,
        "implementations":s.implementations.iter().map(|t| serde_json::json!({"id":t.id,"decision":t.decision,"status":t.status,"message":t.message,"residual":t.residual,"validation":t.validation})).collect::<Vec<_>>(),
        "rules":"Source/design refresh is observation only. Source implementation requires a separate explicit UI approval. First call begin_implementation with the approved task ID; native permissions and repository instructions still apply. Never silently fix all drift or modify out-of-scope files."
    })
}

pub fn unbind(st: &AppState, key: String, generation: u64) -> Result<(), String> {
    let mut s = st.lock();
    if s.source.generation != generation { return Err("Source changed; select again".into()); }
    let mut next = s.source.clone(); next.bindings.retain(|k, _| k != &key && !k.starts_with(&format!("{key}.")));
    next.generation += 1; next.acknowledged_design = None; next.acknowledged_source = None;
    persist(&s.project_dir, &next)?; s.source = next;
    drop(s); st.emit(); Ok(())
}

pub fn mapped(design: &Design, source: &SourceState) -> Option<Design> {
    if source.attachment.is_none() { return None; }
    source.snapshot.as_ref().map(|code| spide_core::source::mapped_reality(design, code, &source.bindings))
}
pub fn acknowledge(st: &AppState, rev: u64, generation: u64) -> Result<(), String> {
    let mut s = st.lock();
    if s.store.design.rev != rev || s.source.generation != generation || s.source.status != "current" { return Err("Design/source changed or source is not current".into()); }
    let mut next = s.source.clone();
    next.acknowledged_source = mapped(&s.store.design, &next); next.acknowledged_design = Some(s.store.design.clone());
    persist(&s.project_dir, &next)?; s.source = next;
    drop(s); st.emit(); Ok(())
}
pub fn propose_code_class(st: &AppState, symbol: String, rev: u64, generation: u64) -> Result<(), String> {
    let mut s = st.lock();
    if s.store.design.rev != rev || s.source.generation != generation || s.source.status != "current" { return Err("Design/source changed or source is not current".into()); }
    let c = s.source.snapshot.as_ref().and_then(|d| d.classes.iter().find(|c| c.source.as_ref().is_some_and(|x| x.symbol == symbol))).ok_or("Unknown source class")?.clone();
    let name = if s.store.design.class(&c.name).is_none() { c.name.clone() } else { symbol.clone() };
    let methods: Vec<_> = c.methods.iter().map(|m| spide_core::Method { code: None, ..m.clone() }).collect();
    let op = serde_json::from_value(serde_json::json!({"op":"add_class", "name":name, "package":c.package, "stereotype":c.stereotype, "attributes":c.attributes, "methods":methods})).map_err(|e| e.to_string())?;
    let mut store = s.store.clone();
    store.propose_ops(vec![op], Some(format!("Adopt Java class {symbol}")), "code").map_err(|e| e.0)?;
    let mut source = s.source.clone(); source.bindings.insert(name, symbol); source.generation += 1;
    source.acknowledged_design = None; source.acknowledged_source = None;
    persist(&s.project_dir, &source)?; s.source = source; s.store = store;
    drop(s); st.changed(); Ok(())
}

pub fn propose_reconciliation(st: &AppState, class: String, rev: u64, generation: u64) -> Result<(), String> {
    let mut s = st.lock();
    if s.store.design.rev != rev || s.source.generation != generation || s.source.status != "current" { return Err("Design/source changed or source is not current".into()); }
    let code = mapped(&s.store.design, &s.source).ok_or("No source attachment")?;
    let candidates = spide_core::source::reconcile(&s.store.design, &code, s.source.acknowledged_design.as_ref(), s.source.acknowledged_source.as_ref(), &s.source.bindings);
    let candidate = candidates.into_iter().find(|c| c.class == class).ok_or("No changes for this class")?;
    s.store.propose_ops(candidate.ops, Some(format!("Adopt source structure for {class}; preserve design intent ({}).", candidate.direction)), "code").map_err(|e| e.0)?;
    drop(s); st.changed(); Ok(())
}

static WATCHER: Mutex<Option<RecommendedWatcher>> = Mutex::new(None);
pub fn watch(st: &AppState) {
    *WATCHER.lock().unwrap() = None;
    let (project, attachment) = { let s = st.lock(); (s.project_dir.clone(), s.source.attachment.clone()) };
    let Some(a) = attachment else { return };
    let (tx, rx) = mpsc::channel();
    let mut watcher = match notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        if let Ok(e) = event {
            if matches!(e.kind, notify::EventKind::Create(_) | notify::EventKind::Modify(_) | notify::EventKind::Remove(_)) && e.paths.iter().any(|p| p.extension().is_some_and(|x| x == "java") || p.extension().is_none()) { let _ = tx.send(()); }
        }
    }) { Ok(w) => w, Err(e) => { st.toast("error", format!("Source watcher: {e}")); return; } };
    for root in &a.roots {
        if let Err(e) = watcher.watch(&a.repository.join(root), RecursiveMode::Recursive) { st.toast("error", format!("Source watcher: {e}; manual refresh available")); }
    }
    *WATCHER.lock().unwrap() = Some(watcher);
    let st = st.clone();
    std::thread::spawn(move || {
        while rx.recv().is_ok() {
            loop { match rx.recv_timeout(Duration::from_millis(350)) { Ok(()) => (), Err(mpsc::RecvTimeoutError::Timeout) => break, Err(_) => return } }
            let current = { let s = st.lock(); s.project_dir == project && s.source.attachment.as_ref() == Some(&a) };
            if !current { return; }
            // Ignore build/excluded changes and duplicate notifications without a model request.
            let unchanged = inventory(&a).ok().is_some_and(|files| files == st.lock().source.files);
            if !unchanged { refresh_async(&st); }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_current_mapped_length_evidence_affects_design_health() {
        let project = tempfile::tempdir().unwrap(); let (events, _) = crate::events::Events::new();
        let st = AppState::new(events, project.path().into());
        let design = Design { classes: vec![spide_core::Class { name: "A".into(), methods: vec![spide_core::Method { name: "run".into(), ..Default::default() }], ..Default::default() }], ..Default::default() };
        let mut code = design.clone(); code.classes[0].source = Some(spide_core::SourceSymbol { symbol: "pkg.A".into(), ..Default::default() });
        code.classes[0].methods[0].code = Some(spide_core::CodeFacts { loc: 31, ..Default::default() });
        {
            let mut s = st.lock(); s.store.load(design); s.source.attachment = Some(Attachment { repository: project.path().into(), roots: vec![".".into()], exclusions: vec![] }); s.source.snapshot = Some(code); s.source.status = "current".into();
        }
        assert!(!st.snapshot().metrics.issues.iter().any(|i| i.kind == "long_method"));
        st.lock().source.bindings.insert("A".into(), "pkg.A".into());
        assert_eq!(st.snapshot().metrics.issues.iter().filter(|i| i.kind == "long_method").count(), 1);
        let op = serde_json::from_str(r#"[{"op":"remove_method","class":"A","name":"run"}]"#).unwrap();
        st.lock().store.propose_ops(op, None, "agent").unwrap();
        assert_eq!(st.snapshot().metrics.issues.iter().filter(|i| i.kind == "long_method").count(), 1, "pending removals cannot improve accepted health");
        st.lock().source.status = "stale/error".into();
        assert!(!st.snapshot().metrics.issues.iter().any(|i| i.kind == "long_method"));
    }
    #[test]
    fn refresh_handles_edits_deletion_and_detach_without_committing_design() {
        let repo = tempfile::tempdir().unwrap(); let project = tempfile::tempdir().unwrap();
        let file = repo.path().join("A.java"); std::fs::write(&file, "class A {}").unwrap();
        let (events, _) = crate::events::Events::new(); let st = AppState::new(events, project.path().into());
        st.lock().source.attachment = Some(Attachment { repository: repo.path().canonicalize().unwrap(), roots: vec![".".into()], exclusions: vec![] });
        let wait = |st: &AppState| {
            let start = std::time::Instant::now();
            while st.lock().source.status.starts_with("refreshing") {
                assert!(start.elapsed() < Duration::from_secs(10)); std::thread::sleep(Duration::from_millis(10));
            }
        };
        watch(&st);
        refresh_async(&st); wait(&st);
        assert_eq!(st.lock().source.snapshot.as_ref().unwrap().classes.len(), 1);
        std::fs::write(&file, "class A { public void run() {} }").unwrap();
        let start = std::time::Instant::now();
        while st.lock().source.snapshot.as_ref().unwrap().classes[0].methods.is_empty() {
            assert!(start.elapsed() < Duration::from_secs(10), "source watcher did not observe edit"); std::thread::sleep(Duration::from_millis(30));
        }
        std::fs::remove_file(&file).unwrap();
        let start = std::time::Instant::now();
        while !st.lock().source.snapshot.as_ref().unwrap().classes.is_empty() {
            assert!(start.elapsed() < Duration::from_secs(10), "source watcher did not observe deletion"); std::thread::sleep(Duration::from_millis(30));
        }
        assert!(st.lock().source.snapshot.as_ref().unwrap().classes.is_empty());
        assert!(st.lock().store.design.classes.is_empty());
        refresh_async(&st); detach(&st).unwrap(); std::thread::sleep(Duration::from_millis(60));
        assert!(st.lock().source.attachment.is_none()); assert!(st.lock().source.status.starts_with("detached"));
    }
    #[test]
    fn scope_inventory_and_persistence_are_local_and_explicit() {
        let repo = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(repo.path().join("module/src/main/java")).unwrap();
        std::fs::write(repo.path().join("module/src/main/java/A.java"), "class A {}").unwrap();
        std::fs::write(repo.path().join("Outside.java"), "class Outside {}").unwrap();
        let (a, count) = preview(Attachment { repository: repo.path().into(), roots: vec!["module/src/main/java".into()], exclusions: vec![] }).unwrap();
        assert_eq!(count, 1); assert_eq!(inventory(&a).unwrap().len(), 1);
        assert!(preview(Attachment { roots: vec!["../".into()], ..a.clone() }).is_err());
        let project = tempfile::tempdir().unwrap();
        let state = SourceState { attachment: Some(a.clone()), status: "current".into(), ..Default::default() };
        persist(project.path(), &state).unwrap(); assert_eq!(load(project.path()).attachment, Some(a));
        assert!(!project.path().join("design.json").exists());
    }
}
