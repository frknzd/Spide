//! Spide Project in the app: the module folder, its code index (re-parsed when Java files change),
//! the sync baseline (`spide/sync.json`) and the analysis the UI, the agent and health use.

use crate::state::{write_atomic, AppState, Shared};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use serde::Serialize;
use serde_json::{json, Value};
use spide_core::model::{Class, Design, Relation, Status};
use spide_core::project::{self, Manifest};
use spide_core::sync::{self, Analysis, Baseline, SyncReport, SyncStatus};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "state", content = "detail", rename_all = "snake_case")]
pub enum IndexStatus {
    Indexing,
    Current,
    /// some files couldn't be parsed
    Partial,
    Error(String),
}

#[derive(Debug, Clone, Default)]
pub struct CodeIndex {
    pub generation: u64,
    pub status: Option<IndexStatus>,
    /// the module as spide-import sees it
    pub code: Option<Arc<Design>>,
    pub files: usize,
    pub skipped: Vec<String>,
    pub parsed_at: u64,
    /// The language-neutral semantic graph (non-Java languages; navigation, evidence mapping).
    pub graph: Option<Arc<spide_core::symbols::SemanticGraph>>,
}

/// An implementation request in flight: which items should end up in sync.
#[derive(Debug, Clone, Serialize)]
pub struct ImplementRun {
    pub classes: Vec<String>,
    pub items: Vec<String>,
    pub started_at: u64,
    pub saw_busy: bool,
    /// pi settled: the next current index decides
    pub awaiting_index: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImplementSummary {
    pub done: usize,
    pub total: usize,
    pub remaining: Vec<String>,
    pub classes: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ProjectState {
    pub root: PathBuf,
    pub manifest: Manifest,
    pub index: CodeIndex,
    pub baseline: Option<Baseline>,
    pub analysis: Option<Arc<Analysis>>,
    pub implementing: Option<ImplementRun>,
    pub last_implement: Option<ImplementSummary>,
    pub warnings: Vec<String>,
    /// baseline text we wrote (ignore our own file events)
    pub baseline_written: Option<String>,
    pub baseline_dirty: bool,
}

/// What the UI needs to know about the project.
#[derive(Debug, Clone, Serialize)]
pub struct ProjectSnap {
    pub root: String,
    pub manifest: Manifest,
    pub index: Option<IndexStatus>,
    pub files: usize,
    pub code_classes: usize,
    pub skipped: Vec<String>,
    pub parsed_at: u64,
    pub has_baseline: bool,
    pub baseline_at: Option<u64>,
    pub report: Option<SyncReport>,
    pub implementing: Option<ImplementRun>,
    pub last_implement: Option<ImplementSummary>,
    pub warnings: Vec<String>,
    /// code-only classes, drawn as ghosts by the sync lens
    pub phantoms: Vec<Class>,
    pub phantom_relations: Vec<Relation>,
    pub root_missing: bool,
}

pub fn now_ms() -> u64 {
    spide_core::store::now_ms()
}

/// The design file of a folder: `spide/design.json` in a Spide Project, `design.json` otherwise.
pub fn design_path(root: &Path) -> PathBuf {
    if project::is_project(root) {
        root.join(project::DESIGN)
    } else {
        root.join("design.json")
    }
}

/// Read the manifest and baseline of a project folder (None: a design-only folder).
pub fn load(root: &Path) -> Option<ProjectState> {
    if !project::is_project(root) {
        return None;
    }
    let mut warnings = vec![];
    let manifest = match std::fs::read_to_string(root.join(project::MANIFEST)).map_err(|e| e.to_string()).and_then(|t| Manifest::parse(&t)) {
        Ok(m) => m,
        Err(e) => {
            warnings.push(e);
            Manifest::default()
        }
    };
    warnings.extend(manifest.validate(root, project::work_tree(root).as_deref()));
    let (baseline, written) = match std::fs::read_to_string(root.join(project::BASELINE)) {
        Ok(t) => match Baseline::parse(&t) {
            Ok(b) => (Some(b), Some(t)),
            Err(e) => {
                warnings.push(e);
                (None, None)
            }
        },
        Err(_) => (None, None),
    };
    Some(ProjectState {
        root: root.to_path_buf(),
        manifest,
        index: CodeIndex::default(),
        baseline,
        analysis: None,
        implementing: None,
        last_implement: None,
        warnings,
        baseline_written: written,
        baseline_dirty: false,
    })
}

/// Re-run sync over the current design and index; move the baseline along in-sync items.
/// Returns true when the baseline changed (the caller schedules the write).
pub fn recompute(s: &mut Shared) -> bool {
    let design = s.store.design.clone();
    let Some(p) = s.project.as_mut() else { return false };
    let Some(code) = p.index.code.clone() else {
        p.analysis = None;
        return false;
    };
    let partial = !matches!(p.index.status, Some(IndexStatus::Current));
    let a = sync::analyse(&design, &code, &p.manifest, p.baseline.as_ref(), partial);
    let mut changed = false;
    if let Some(next) = sync::advance(p.baseline.as_ref(), &a, now_ms()) {
        p.baseline = Some(next);
        p.baseline_dirty = true;
        changed = true;
    }
    // statuses change with the baseline: recompute against it
    let a = if changed { sync::analyse(&design, &code, &p.manifest, p.baseline.as_ref(), partial) } else { a };
    p.analysis = Some(Arc::new(a));
    changed
}

static WRITING: Mutex<bool> = Mutex::new(false);

/// Write the baseline about a second after the last change (atomic, only when it changed).
pub fn schedule_baseline_write(st: &AppState) {
    {
        let mut w = WRITING.lock().unwrap();
        if *w {
            return;
        }
        *w = true;
    }
    let st = st.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(1000));
        *WRITING.lock().unwrap() = false;
        let mut s = st.lock();
        let Some(p) = s.project.as_mut() else { return };
        if !p.baseline_dirty {
            return;
        }
        p.baseline_dirty = false;
        let Some(b) = &p.baseline else { return };
        let text = b.to_json();
        if p.baseline_written.as_deref() == Some(text.as_str()) {
            return;
        }
        match write_atomic(&p.root.join(project::BASELINE), &text) {
            Ok(()) => p.baseline_written = Some(text),
            Err(e) => eprintln!("spide: sync baseline: {e}"),
        }
    });
}

pub fn snap(s: &Shared) -> Option<ProjectSnap> {
    let p = s.project.as_ref()?;
    let code = p.index.code.as_deref();
    let report = p.analysis.as_ref().map(|a| a.report.clone());
    let mut phantoms = vec![];
    let mut phantom_relations = vec![];
    if let (Some(a), Some(code)) = (p.analysis.as_deref(), code) {
        let names: std::collections::BTreeMap<&String, &String> = a.code_class.iter().map(|(shape, code)| (code, shape)).collect();
        for (shape, cs) in &a.report.classes {
            if cs.in_code && !cs.in_design {
                if let Some(c) = a.code_class.get(shape).and_then(|n| code.class(n)) {
                    let mut c = c.clone();
                    c.name = shape.clone();
                    c.status = Status::Accepted;
                    phantoms.push(c);
                }
            }
        }
        let phantom: std::collections::BTreeSet<&String> = phantoms.iter().map(|c| &c.name).collect();
        for r in code.relations.iter() {
            let (Some(f), Some(t)) = (names.get(&r.from), names.get(&r.to)) else { continue };
            if phantom.contains(f) || phantom.contains(t) {
                phantom_relations.push(Relation { from: (*f).clone(), to: (*t).clone(), id: format!("code:{}->{}:{}", f, t, r.kind.as_str()), ..r.clone() });
            }
        }
    }
    Some(ProjectSnap {
        root: p.root.display().to_string(),
        manifest: p.manifest.clone(),
        index: p.index.status.clone(),
        files: p.index.files,
        code_classes: code.map(|c| c.classes.iter().filter(|c| !c.external).count()).unwrap_or(0),
        skipped: p.index.skipped.clone(),
        parsed_at: p.index.parsed_at,
        has_baseline: p.baseline.is_some(),
        baseline_at: p.baseline.as_ref().map(|b| b.updated_at),
        report,
        implementing: p.implementing.clone(),
        last_implement: p.last_implement.clone(),
        warnings: p.warnings.clone(),
        phantoms,
        phantom_relations,
        root_missing: !p.root.is_dir(),
    })
}

/// The design with the linked code's facts (health judges the real methods too).
pub fn with_code_facts(s: &Shared, d: &Design) -> Option<Design> {
    let p = s.project.as_ref()?;
    let (a, code) = (p.analysis.as_deref()?, p.index.code.as_deref()?);
    Some(sync::with_code_facts(d, code, a))
}

// ---------------------------------------------------------------- index

/// Parse the module again in the background (a newer request wins).
pub fn refresh_async(st: &AppState) {
    let job = {
        let mut s = st.lock();
        let Some(p) = s.project.as_mut() else { return };
        p.index.generation += 1;
        p.index.status = Some(IndexStatus::Indexing);
        (p.index.generation, p.root.clone(), p.manifest.clone())
    };
    st.emit();
    let st = st.clone();
    std::thread::spawn(move || {
        let (generation, root, manifest) = job;
        // canonical root: file paths in the index come out relative to it
        let canon = root.canonicalize().unwrap_or_else(|_| root.clone());
        let r = spide_import::import_project(&canon, &manifest.all_languages(), &manifest.roots, &manifest.exclude, manifest.code_root.as_deref());
        let dirty = {
            let mut s = st.lock();
            let Some(p) = s.project.as_mut().filter(|p| p.root == root && p.index.generation == generation) else { return };
            match r {
                Ok(r) => {
                    p.index.status = Some(if r.skipped.is_empty() { IndexStatus::Current } else { IndexStatus::Partial });
                    p.index.files = r.files;
                    p.index.skipped = r.skipped;
                    p.index.code = Some(Arc::new(r.design));
                    p.index.graph = Some(Arc::new(r.graph));
                }
                Err(e) => {
                    p.index.status = Some(IndexStatus::Error(e));
                    p.index.files = 0;
                }
            }
            p.index.parsed_at = now_ms();
            let dirty = recompute(&mut s);
            finish_implement(&mut s);
            dirty
        };
        if dirty {
            schedule_baseline_write(&st);
        }
        st.emit();
    });
}

static WATCHER: Mutex<Option<RecommendedWatcher>> = Mutex::new(None);

/// Watch the module: source files (any indexed language) under the roots re-index, `spide/sync.json` reloads the baseline.
pub fn watch(st: &AppState) {
    let Some((root, roots, langs)) = st.lock().project.as_ref().map(|p| (p.root.clone(), p.manifest.roots.clone(), p.manifest.all_languages())) else {
        *WATCHER.lock().unwrap() = None;
        return;
    };
    let (tx, rx) = channel::<bool>();
    // events arrive with canonical paths (/private/var/… on macOS)
    let canon = root.canonicalize().unwrap_or_else(|_| root.clone());
    let (r2, roots2) = (canon.clone(), roots.clone());
    let baseline = canon.join(project::BASELINE);
    let watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        let Ok(ev) = res else { return };
        for p in &ev.paths {
            if p == &baseline {
                let _ = tx.send(false);
            } else if p.to_str().and_then(spide_core::symbols::language_of).is_some_and(|l| langs.iter().any(|x| x == l)) && roots2.iter().any(|r| p.starts_with(r2.join(r))) {
                let _ = tx.send(true);
            }
        }
    });
    let mut watcher = match watcher {
        Ok(w) => w,
        Err(e) => return eprintln!("spide: project watcher: {e}"),
    };
    if let Err(e) = watcher.watch(&root, RecursiveMode::Recursive) {
        return eprintln!("spide: watch {}: {e}", root.display());
    }
    *WATCHER.lock().unwrap() = Some(watcher);
    let st = st.clone();
    std::thread::spawn(move || loop {
        let Ok(first) = rx.recv() else { return };
        let (mut java, mut base) = (first, !first);
        loop {
            match rx.recv_timeout(Duration::from_millis(400)) {
                Ok(j) => {
                    java |= j;
                    base |= !j;
                }
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
        if st.lock().project.as_ref().is_none_or(|p| p.root != root) {
            return;
        }
        if base {
            reload_baseline(&st);
        }
        if java {
            refresh_async(&st);
        }
    });
}

/// `spide/sync.json` changed on disk (e.g. a git pull): take it.
pub fn reload_baseline(st: &AppState) {
    {
        let mut s = st.lock();
        let Some(p) = s.project.as_mut() else { return };
        let text = std::fs::read_to_string(p.root.join(project::BASELINE)).ok();
        if text == p.baseline_written {
            return;
        }
        p.warnings.retain(|w| !w.contains("sync.json"));
        p.baseline = match text.as_deref().map(Baseline::parse) {
            Some(Ok(b)) => Some(b),
            Some(Err(e)) => {
                p.warnings.push(e);
                None
            }
            None => None,
        };
        p.baseline_written = text;
        recompute(&mut s);
    }
    st.emit();
}

// ---------------------------------------------------------------- implement runs

pub fn on_agent_busy(s: &mut Shared) {
    if let Some(r) = s.project.as_mut().and_then(|p| p.implementing.as_mut()) {
        r.saw_busy = true;
    }
}

/// pi settled after an implementation request: re-index, then summarise.
pub fn on_agent_settled(st: &AppState) {
    let go = {
        let mut s = st.lock();
        match s.project.as_mut().and_then(|p| p.implementing.as_mut()) {
            Some(r) if r.saw_busy => {
                r.awaiting_index = true;
                true
            }
            _ => false,
        }
    };
    if go {
        refresh_async(st);
    }
}

/// After a current index: how much of the requested scope is now in sync.
fn finish_implement(s: &mut Shared) {
    let Some(p) = s.project.as_mut() else { return };
    if !matches!(p.index.status, Some(IndexStatus::Current)) || !p.implementing.as_ref().is_some_and(|r| r.awaiting_index) {
        return;
    }
    let run = p.implementing.take().unwrap();
    let Some(a) = p.analysis.as_deref() else { return };
    let open: Vec<String> = run.items.iter().filter(|id| a.report.items.iter().any(|i| &i.id == *id && !matches!(i.status, SyncStatus::InSync | SyncStatus::Accepted))).cloned().collect();
    p.last_implement = Some(ImplementSummary { done: run.items.len() - open.len(), total: run.items.len(), remaining: open, classes: run.classes });
}

/// Start tracking an implementation: the design-side items in scope.
pub fn begin_implement(s: &mut Shared, classes: &[String], keep_design: bool) -> Result<Vec<String>, String> {
    let p = s.project.as_mut().ok_or("not a Spide Project")?;
    match &p.index.status {
        Some(IndexStatus::Current) => {}
        Some(IndexStatus::Indexing) | None => return Err("the code is still being indexed — try again in a moment".into()),
        Some(IndexStatus::Partial) => return Err(format!("{} Java file(s) couldn't be parsed; fix them or exclude them first", p.index.skipped.len())),
        Some(IndexStatus::Error(e)) => return Err(format!("the code index failed: {e}")),
    }
    let a = p.analysis.as_deref().ok_or("no sync analysis yet")?;
    let items: Vec<String> = a
        .report
        .items
        .iter()
        .filter(|i| classes.is_empty() || classes.contains(&i.class))
        .filter(|i| matches!(i.status, SyncStatus::DesignAhead | SyncStatus::Unreconciled) || (keep_design && i.status == SyncStatus::Conflict))
        // unreconciled code-only elements are readjust candidates, never deleted by default
        .filter(|i| i.status != SyncStatus::Unreconciled || i.design.is_some())
        .map(|i| i.id.clone())
        .collect();
    if items.is_empty() {
        return Err("nothing to implement in this scope".into());
    }
    p.implementing = Some(ImplementRun { classes: classes.to_vec(), items: items.clone(), started_at: now_ms(), saw_busy: false, awaiting_index: false });
    p.last_implement = None;
    Ok(items)
}

// ---------------------------------------------------------------- create / convert

/// What a folder would index with this manifest.
#[derive(Debug, Clone, Serialize, Default)]
pub struct Preview {
    pub files: usize,
    pub classes: usize,
    /// common package prefix the importer found (a good code root)
    pub code_root: Option<String>,
    pub errors: Vec<String>,
}

pub fn preview(root: &Path, manifest: &Manifest) -> Preview {
    let mut out = Preview { errors: manifest.validate(root, project::work_tree(root).as_deref()), ..Default::default() };
    if !out.errors.is_empty() {
        return out;
    }
    let canon = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    match spide_import::import_project(&canon, &manifest.all_languages(), &manifest.roots, &manifest.exclude, manifest.code_root.as_deref()) {
        Ok(r) => {
            out.files = r.files;
            out.classes = r.design.classes.iter().filter(|c| !c.external).count();
            out.code_root = r.design.settings.code_root.clone();
            out.errors.extend(r.skipped.iter().map(|s| format!("couldn't parse {s}")));
        }
        Err(e) => out.errors.push(e),
    }
    out
}

/// A sensible starting manifest for a module folder.
pub fn suggest_manifest(root: &Path, name: &str) -> Manifest {
    let mut m = Manifest { name: name.to_string(), ..Default::default() };
    if root.join("src/main/java").is_dir() {
        return m;
    }
    m.roots = vec![".".into()];
    let langs = detect_languages(root);
    if let Some(first) = langs.first() {
        m.language = first.clone();
        m.languages = langs[1..].to_vec();
    }
    if m.language != "java" && root.join("src").is_dir() {
        m.roots = vec!["src".into()];
    }
    m
}

/// Languages present in a folder, most files first.
pub fn detect_languages(root: &Path) -> Vec<String> {
    spide_import::languages::detect(root)
}

/// Make `root` a Spide Project carrying `design` (copied, with its local history). Never touches an
/// existing project's design; the old folder stays as it was.
pub fn create(root: &Path, manifest: &Manifest, design: &Design, from: Option<&Path>, git_exclude: bool) -> Result<Vec<String>, String> {
    if project::is_project(root) {
        return Ok(vec![]);
    }
    let errors = manifest.validate(root, project::work_tree(root).as_deref());
    if !errors.is_empty() {
        return Err(errors.join("; "));
    }
    let mut d = design.clone();
    if d.name.trim().is_empty() || d.name == "Untitled design" {
        d.name = manifest.name.clone();
    }
    if root.join(project::DESIGN).exists() {
        return Err(format!("{} already exists", root.join(project::DESIGN).display()));
    }
    write_atomic(&root.join(project::DESIGN), &d.to_json()).map_err(|e| e.to_string())?;
    if let Some(from) = from {
        let history = crate::history::path(from);
        if history.is_file() {
            let target = crate::history::path(root);
            if let Some(p) = target.parent() {
                let _ = std::fs::create_dir_all(p);
            }
            let _ = std::fs::copy(history, target);
        }
    }
    write_atomic(&root.join(project::MANIFEST), &manifest.to_json()).map_err(|e| e.to_string())?;
    if git_exclude {
        project::exclude_local_files(root)
    } else {
        Ok(vec![])
    }
}

// ---------------------------------------------------------------- agent tools

pub fn tool_list() -> Vec<Value> {
    vec![
        json!({"name":"get_project","description":"Spide Project (design and code in one module): the manifest (roots, code_root, compare filters, validation commands), the code index (files, classes, skipped, freshness), the sync baseline and how many elements are in sync / design ahead / code ahead / conflict / unreconciled / accepted.",
            "inputSchema":{"type":"object","properties":{}},"annotations":{"readOnlyHint":true}}),
        json!({"name":"get_sync","description":"Design ↔ code differences per element (class, field, method, extends, implements): status (design_ahead = implement it in the code; code_ahead = readjust the design; conflict; unreconciled = no baseline yet; accepted), the design's and the code's value, the baseline, file:line, the accepted decisions since the baseline, a rename hint, and suggested_ops that bring the code's version into the design (start readjust proposals from them). Identity is exact (fully qualified names or explicit link_code links).",
            "inputSchema":{"type":"object","properties":{
                "status":{"type":"array","items":{"type":"string","enum":["design_ahead","code_ahead","conflict","unreconciled","accepted"]}},
                "classes":{"type":"array","items":{"type":"string"}},
                "limit":{"type":"integer","minimum":1,"maximum":1000}}},"annotations":{"readOnlyHint":true}}),
        json!({"name":"get_code","description":"Read the implementation of a class (design or code name, or a fully qualified symbol) or one of its methods from the indexed code: file, line range, numbered source (up to 400 lines; ask per method for long classes) and the facts Spide extracted. Read-only.",
            "inputSchema":{"type":"object","properties":{"class":{"type":"string"},"symbol":{"type":"string"},"method":{"type":"string"}}},"annotations":{"readOnlyHint":true}}),
    ]
}

pub fn summary(s: &Shared) -> Value {
    let Some(p) = s.project.as_ref() else { return Value::Null };
    let counts = p.analysis.as_ref().map(|a| json!(a.report.counts)).unwrap_or(Value::Null);
    json!({
        "root": p.root.display().to_string(),
        "manifest": p.manifest,
        "design_file": project::DESIGN,
        "index": {"status": p.index.status, "files": p.index.files, "classes": p.index.code.as_ref().map(|c| c.classes.len()), "skipped": p.index.skipped, "parsed_at": p.index.parsed_at},
        "baseline": {"exists": p.baseline.is_some(), "updated_at": p.baseline.as_ref().map(|b| b.updated_at)},
        "counts": counts,
        "warnings": p.warnings,
        "hint": "readjust = propose design changes for code_ahead items (get_sync suggested_ops); implement = edit code for design_ahead items, only when the user asks. Marking in sync is the user's.",
    })
}

pub fn get_sync(s: &Shared, args: &Value) -> Result<String, String> {
    let p = s.project.as_ref().ok_or("This folder is not a Spide Project")?;
    let a = p.analysis.as_deref().ok_or("The code isn't indexed yet")?;
    let statuses: Vec<SyncStatus> = args.get("status").and_then(Value::as_array).map(|v| v.iter().filter_map(Value::as_str).filter_map(SyncStatus::parse).collect()).unwrap_or_default();
    let classes: Vec<&str> = args.get("classes").and_then(Value::as_array).map(|v| v.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
    let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(200) as usize;
    let items: Vec<&sync::SyncItem> = a
        .report
        .items
        .iter()
        .filter(|i| statuses.is_empty() || statuses.contains(&i.status))
        .filter(|i| classes.is_empty() || classes.contains(&i.class.as_str()))
        .collect();
    let total = items.len();
    Ok(json!({"partial": a.report.partial, "has_baseline": a.report.has_baseline, "counts": a.report.counts, "total": total,
        "items": items.into_iter().take(limit).collect::<Vec<_>>()})
    .to_string())
}

/// The numbered source of a class or method, read from the module (never outside it).
pub fn get_code(s: &Shared, args: &Value) -> Result<String, String> {
    let p = s.project.as_ref().ok_or("This folder is not a Spide Project")?;
    let code = p.index.code.as_deref().ok_or("The code isn't indexed yet")?;
    let wanted = args.get("class").and_then(Value::as_str).or(args.get("symbol").and_then(Value::as_str)).ok_or("give class or symbol")?;
    let method = args.get("method").and_then(Value::as_str).filter(|m| !m.is_empty());
    let by_shape = p.analysis.as_deref().and_then(|a| a.code_class.get(wanted)).and_then(|n| code.class(n));
    let c = by_shape
        .or_else(|| code.class(wanted))
        .or_else(|| code.classes.iter().find(|c| c.source.as_ref().is_some_and(|s| s.symbol == wanted)))
        .ok_or(format!("{wanted} is not in the indexed code"))?;
    let src = c.source.as_ref().ok_or("no source location")?;
    let (start, end, facts) = match method {
        Some(m) => {
            let ms: Vec<_> = c.methods.iter().filter(|x| x.name == m || x.name.split('(').next() == Some(m)).collect();
            let x = ms.first().ok_or(format!("{}.{m}() is not in the code", c.name))?;
            let f = x.code.clone().unwrap_or_default();
            (f.start_line.max(1), f.end_line, Some(json!({"loc": f.loc, "statements": f.statements, "max_chain": f.max_chain, "switches": f.switches, "loops": f.loops, "creates": f.creates, "ui_api": f.ui_api})))
        }
        None => (src.start_line.max(1), src.end_line, None),
    };
    let root = p.root.canonicalize().map_err(|e| e.to_string())?;
    let file = root.join(&src.file).canonicalize().map_err(|e| format!("{}: {e}", src.file))?;
    if !file.starts_with(&root) {
        return Err("refusing to read outside the project".into());
    }
    let text = std::fs::read_to_string(&file).map_err(|e| e.to_string())?;
    let lines: Vec<&str> = text.lines().collect();
    let end = if end == 0 { lines.len() as u32 } else { end };
    let (a, b) = ((start - 1) as usize, (end as usize).min(lines.len()));
    let cap = 400;
    let mut body: String = lines[a.min(b)..b].iter().take(cap).enumerate().map(|(i, l)| format!("{:>5} {l}\n", a + i + 1)).collect();
    if b - a.min(b) > cap {
        body.push_str(&format!("… {} more lines; ask for one method at a time\n", b - a - cap));
    }
    Ok(json!({"symbol": src.symbol, "file": src.file, "lines": [start, end], "facts": facts, "source": body}).to_string())
}

/// Mark in sync (the user's): the current shapes become the baseline, written now.
pub fn mark_synced(st: &AppState) -> Result<usize, String> {
    let accepted = {
        let mut s = st.lock();
        let p = s.project.as_mut().ok_or("not a Spide Project")?;
        if !matches!(p.index.status, Some(IndexStatus::Current)) {
            return Err("the code index isn't current — wait for indexing or fix the skipped files".into());
        }
        let a = p.analysis.as_deref().ok_or("no sync analysis yet")?;
        let accepted = a.report.items.iter().filter(|i| i.status != SyncStatus::Accepted).count();
        let b = sync::mark_synced(a, now_ms());
        let text = b.to_json();
        write_atomic(&p.root.join(project::BASELINE), &text).map_err(|e| e.to_string())?;
        p.baseline = Some(b);
        p.baseline_written = Some(text);
        p.baseline_dirty = false;
        recompute(&mut s);
        accepted
    };
    st.emit();
    Ok(accepted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::Events;
    use std::time::Instant;

    const FOO: &str = "package org.x.domain;\npublic class Foo {\n    private int qty;\n    public int total(int n) {\n        return qty * n;\n    }\n}\n";

    fn module() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src/main/java/org/x/domain");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("Foo.java"), FOO).unwrap();
        dir
    }

    fn make_project(dir: &Path) {
        let m = Manifest { name: "x".into(), code_root: Some("org.x".into()), ..Default::default() };
        create(dir, &m, &Design::default(), None, true).unwrap();
    }

    fn state(dir: &Path) -> AppState {
        AppState::new(Events::new().0, dir.to_path_buf())
    }

    fn wait(st: &AppState, what: &str, f: impl Fn(&Shared) -> bool) {
        let t = Instant::now();
        while t.elapsed() < Duration::from_secs(10) {
            if f(&st.lock()) {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("timed out waiting for {what}");
    }

    fn indexed(st: &AppState) {
        refresh_async(st);
        wait(st, "the index", |s| s.project.as_ref().is_some_and(|p| p.index.status == Some(IndexStatus::Current) && p.analysis.is_some()));
    }

    #[test]
    fn detection_and_design_path() {
        let dir = module();
        assert!(state(dir.path()).lock().project.is_none(), "a plain folder is design-only");
        assert_eq!(state(dir.path()).lock().design_path(), dir.path().join("design.json"));
        make_project(dir.path());
        for open in [dir.path().to_path_buf(), dir.path().join("spide"), dir.path().join("spide/project.json")] {
            let st = state(&open);
            let s = st.lock();
            assert!(s.project.is_some(), "{}", open.display());
            assert_eq!(s.design_path(), dir.path().join("spide/design.json"));
        }
    }

    #[test]
    fn create_copies_the_design_and_leaves_the_old_folder() {
        let old = tempfile::tempdir().unwrap();
        let mut d = Design { name: "Shop".into(), ..Default::default() };
        d.classes.push(Class { name: "Order".into(), ..Default::default() });
        std::fs::write(old.path().join("design.json"), d.to_json()).unwrap();
        std::fs::create_dir_all(old.path().join(".spide")).unwrap();
        std::fs::write(old.path().join(".spide/history.jsonl"), "{}\n").unwrap();
        let dir = module();
        std::fs::create_dir_all(dir.path().join(".git/info")).unwrap();
        let added = create(dir.path(), &Manifest { name: "Shop".into(), ..Default::default() }, &d, Some(old.path()), true).unwrap();
        assert_eq!(added.len(), 2, "{added:?}");
        let copied = Design::from_json(&std::fs::read_to_string(dir.path().join("spide/design.json")).unwrap()).unwrap();
        assert_eq!(copied.classes.len(), 1);
        assert!(dir.path().join(".spide/history.jsonl").exists());
        assert_eq!(std::fs::read_to_string(old.path().join("design.json")).unwrap(), d.to_json(), "the old folder is untouched");
        // an existing project is opened, never overwritten
        assert!(create(dir.path(), &Manifest::default(), &Design::default(), None, true).unwrap().is_empty());
        assert_eq!(Design::from_json(&std::fs::read_to_string(dir.path().join("spide/design.json")).unwrap()).unwrap().classes.len(), 1);
    }

    #[test]
    fn index_sync_tools_and_implementation_runs() {
        let dir = module();
        make_project(dir.path());
        let st = state(dir.path());
        indexed(&st);
        {
            let s = st.lock();
            let a = s.project.as_ref().unwrap().analysis.clone().unwrap();
            assert_eq!(a.report.class_status("Foo"), Some(SyncStatus::Unreconciled), "code-only, no baseline");
            let v: Value = serde_json::from_str(&get_sync(&s, &json!({"classes": ["Foo"]})).unwrap()).unwrap();
            assert!(v["items"].as_array().unwrap().iter().any(|i| i["id"] == "class:Foo" && !i["suggested_ops"].as_array().unwrap().is_empty()), "{v}");
            let code: Value = serde_json::from_str(&get_code(&s, &json!({"class": "Foo", "method": "total"})).unwrap()).unwrap();
            assert!(code["source"].as_str().unwrap().contains("return qty * n;"), "{code}");
            assert_eq!(code["file"], "src/main/java/org/x/domain/Foo.java");
            assert!(summary(&s)["counts"]["unreconciled"].as_u64().unwrap() >= 1);
        }
        // the design adopts Foo: in sync, and the first agreement creates the baseline
        let ops: Vec<spide_core::Op> = st.lock().project.as_ref().unwrap().analysis.as_ref().unwrap().report.items.iter().filter(|i| i.id == "class:Foo").flat_map(|i| i.suggested_ops.clone()).collect();
        st.lock().store.apply_user(ops).unwrap();
        st.changed();
        assert!(st.lock().project.as_ref().unwrap().analysis.as_ref().unwrap().report.items.is_empty());
        wait(&st, "the baseline file", |s| s.project.as_ref().unwrap().root.join(project::BASELINE).exists());
        // a design-only method is design ahead; implementing it is tracked to the next index
        st.lock().store.apply_user(serde_json::from_str(r#"[{"op":"add_method","class":"Foo","method":{"name":"reset","returns":"void"}}]"#).unwrap()).unwrap();
        st.changed();
        let items = begin_implement(&mut st.lock(), &[], false).unwrap();
        assert_eq!(items, vec!["method:Foo.reset()".to_string()]);
        on_agent_busy(&mut st.lock());
        // "the agent" edits the code; the watcher re-indexes, settling summarises
        std::fs::write(dir.path().join("src/main/java/org/x/domain/Foo.java"), FOO.replace("    public int total", "    public void reset() {\n        qty = 0;\n    }\n    public int total")).unwrap();
        watch(&st);
        on_agent_settled(&st);
        wait(&st, "the implementation summary", |s| s.project.as_ref().unwrap().last_implement.is_some());
        let sum = st.lock().project.as_ref().unwrap().last_implement.clone().unwrap();
        assert_eq!((sum.done, sum.total), (1, 1), "{sum:?}");
    }

    #[test]
    fn the_watcher_reindexes_when_java_changes() {
        let dir = module();
        make_project(dir.path());
        let st = state(dir.path());
        indexed(&st);
        watch(&st);
        std::thread::sleep(Duration::from_millis(200));
        std::fs::write(dir.path().join("src/main/java/org/x/domain/Bar.java"), "package org.x.domain;\npublic class Bar {}\n").unwrap();
        wait(&st, "Bar in the index", |s| s.project.as_ref().unwrap().index.code.as_ref().is_some_and(|c| c.class("Bar").is_some()));
    }

    #[test]
    fn mark_synced_accepts_the_rest() {
        let dir = module();
        make_project(dir.path());
        let st = state(dir.path());
        indexed(&st);
        let n = mark_synced(&st).unwrap();
        assert!(n >= 1);
        let s = st.lock();
        let a = s.project.as_ref().unwrap().analysis.clone().unwrap();
        assert!(a.report.items.iter().all(|i| i.status == SyncStatus::Accepted));
        assert!(s.project.as_ref().unwrap().root.join(project::BASELINE).exists());
    }
}
