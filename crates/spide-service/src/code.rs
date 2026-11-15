//! Code intelligence for the service: the (multi-language) code index, symbol queries, source
//! reading, navigation targets, evidence (LSP diagnostics + linters) and `doctor`.

use serde_json::{json, Value};
use spide_core::evidence::EvidenceSet;
use spide_core::project::Manifest;
use spide_core::symbols::{Entity, SemanticGraph};
use spide_core::Design;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

#[derive(Clone)]
pub struct CodeSnapshot {
    pub design: Arc<Design>,
    pub graph: Arc<SemanticGraph>,
    pub files: usize,
    pub skipped: Vec<String>,
    pub languages: Vec<String>,
}

#[derive(Default)]
pub struct CodeState {
    snapshot: Option<(CodeSnapshot, (u64, usize))>,
    checked: Option<Instant>,
}

/// The project's manifest, or an inferred one for a plain folder (all detected languages, root ".").
pub fn manifest(root: &Path) -> Manifest {
    if let Some(m) = std::fs::read_to_string(root.join(spide_core::project::MANIFEST)).ok().and_then(|t| Manifest::parse(&t).ok()) {
        return m;
    }
    let langs = spide_import::languages::detect(root);
    let mut m = Manifest { roots: vec![".".into()], ..Default::default() };
    if let Some(first) = langs.first() {
        m.language = first.clone();
        m.languages = langs[1..].to_vec();
    }
    m
}

/// Source files per language, absolute.
pub fn source_files(root: &Path, m: &Manifest) -> std::collections::BTreeMap<String, Vec<PathBuf>> {
    m.all_languages()
        .into_iter()
        .map(|l| {
            let mut f = spide_import::scoped_files_of(root, &m.roots, &m.exclude, &l).unwrap_or_default();
            if l == "cpp" && m.all_languages().iter().any(|x| x == "c") {
                f.retain(|p| p.extension().is_none_or(|e| e != "h"));
            }
            (l, f)
        })
        .collect()
}

fn fingerprint(files: &std::collections::BTreeMap<String, Vec<PathBuf>>) -> (u64, usize) {
    let mut newest = 0u64;
    let mut n = 0;
    for f in files.values().flatten() {
        n += 1;
        if let Ok(t) = std::fs::metadata(f).and_then(|m| m.modified()) {
            newest = newest.max(t.duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0));
        }
    }
    (newest, n)
}

impl CodeState {
    /// Current index, rebuilt when source files changed (checked at most every 2 s).
    pub fn get(&mut self, root: &Path) -> Result<CodeSnapshot, String> {
        if let (Some((snap, _)), Some(at)) = (&self.snapshot, self.checked) {
            if at.elapsed() < Duration::from_secs(2) {
                return Ok(snap.clone());
            }
        }
        let m = manifest(root);
        let files = source_files(root, &m);
        let fp = fingerprint(&files);
        self.checked = Some(Instant::now());
        if let Some((snap, old)) = &self.snapshot {
            if *old == fp {
                return Ok(snap.clone());
            }
        }
        let canon = root.canonicalize().map_err(|e| e.to_string())?;
        let r = spide_import::import_project(&canon, &m.all_languages(), &m.roots, &m.exclude, m.code_root.as_deref())?;
        let snap = CodeSnapshot { design: Arc::new(r.design), graph: Arc::new(r.graph), files: r.files, skipped: r.skipped, languages: m.all_languages() };
        self.snapshot = Some((snap.clone(), fp));
        Ok(snap)
    }

    pub fn invalidate(&mut self) {
        self.checked = None;
    }
}

fn entity_json(e: &Entity) -> Value {
    json!({"id": e.id, "kind": e.kind.map(|k| k.as_str()), "name": e.name, "qualified": e.qualified, "language": e.language,
           "file": e.file, "line": e.start_line, "end_line": e.end_line, "col": e.start_col,
           "signature": e.kind.filter(|k| k.is_callable()).map(|_| e.signature())})
}

/// Graph entities (plus Java classes/methods from the projected design) as navigation targets.
pub fn targets(snap: &CodeSnapshot) -> Vec<Value> {
    let mut out: Vec<Value> = snap.graph.entities.iter().map(entity_json).collect();
    // Java goes through its own importer: expose its classes and methods too
    for c in snap.design.classes.iter().filter(|c| c.source.as_ref().is_some_and(|s| s.lang() == "java")) {
        let s = c.source.as_ref().unwrap();
        out.push(json!({"id": format!("java:{}", s.symbol), "kind": c.stereotype.clone().unwrap_or("class".into()), "name": c.name, "qualified": s.symbol, "language": "java", "file": s.file, "line": s.start_line, "end_line": s.end_line, "col": 0}));
        for m in &c.methods {
            if let Some(ms) = &m.source {
                out.push(json!({"id": format!("java:{}", ms.symbol), "kind": "method", "name": m.name, "qualified": ms.symbol, "language": "java", "file": ms.file, "line": ms.start_line, "end_line": ms.end_line, "col": 0}));
            }
        }
    }
    out
}

/// `get_symbols`: filter by file/kind/query, paginated.
pub fn symbols(snap: &CodeSnapshot, args: &Value) -> Value {
    let file = args.get("file").and_then(Value::as_str);
    let kind = args.get("kind").and_then(Value::as_str);
    let q = args.get("query").and_then(Value::as_str).map(str::to_ascii_lowercase);
    let offset = args.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize;
    let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(100).clamp(1, 1000) as usize;
    let all: Vec<Value> = targets(snap)
        .into_iter()
        .filter(|e| file.is_none_or(|f| e["file"] == f))
        .filter(|e| kind.is_none_or(|k| e["kind"] == k))
        .filter(|e| q.as_ref().is_none_or(|q| e["qualified"].as_str().unwrap_or("").to_ascii_lowercase().contains(q.as_str())))
        .collect();
    let total = all.len();
    json!({"total": total, "symbols": all.into_iter().skip(offset).take(limit).collect::<Vec<_>>(), "languages": snap.languages, "limits": snap.graph.limits, "skipped": snap.skipped})
}

/// Resolve a navigation request to a source location: by symbol id, qualified name, or a design
/// element (`class` [+ `method`]) through its code link (or the indexed code's same-named node).
pub fn locate(snap: &CodeSnapshot, design: &Design, args: &Value) -> Result<Value, String> {
    let all = targets(snap);
    if let Some(id) = args.get("id").and_then(Value::as_str) {
        return all.into_iter().find(|e| e["id"] == id).ok_or_else(|| format!("no symbol {id}"));
    }
    if let Some(q) = args.get("symbol").and_then(Value::as_str) {
        let hits: Vec<Value> = all.iter().filter(|e| e["qualified"] == q || e["qualified"].as_str().is_some_and(|x| x.replace('#', ".").starts_with(&q.replace('#', ".")) && x.len() <= q.len() + 40)).cloned().collect();
        return match hits.len() {
            0 => Err(format!("no symbol {q}")),
            1 => Ok(hits[0].clone()),
            _ => hits.iter().find(|e| e["qualified"] == q).cloned().ok_or_else(|| format!("{q} is ambiguous: {}", hits.iter().take(5).map(|h| h["qualified"].as_str().unwrap_or("").to_string()).collect::<Vec<_>>().join(", "))),
        };
    }
    let class = args.get("class").and_then(Value::as_str).ok_or("give id, symbol or class [+ method]")?;
    let method = args.get("method").and_then(Value::as_str);
    let from = |d: &Design| -> Option<spide_core::SourceSymbol> {
        let c = d.class(class)?;
        match method {
            Some(m) => c.methods.iter().find(|x| x.name == m || x.name.split('(').next() == Some(m)).and_then(|x| x.source.clone()),
            None => c.source.clone(),
        }
    };
    let link = from(design).filter(|s| !s.file.is_empty()).or_else(|| {
        // design link without a file: find the code node with that symbol, else the code's same-named class
        let sym = from(design).map(|s| s.symbol);
        let code_class = snap.design.classes.iter().find(|c| sym.as_ref().is_some_and(|s| c.source.as_ref().is_some_and(|x| &x.symbol == s))).map(|c| c.name.clone()).unwrap_or_else(|| class.to_string());
        let c = snap.design.class(&code_class)?;
        match method {
            Some(m) => c.methods.iter().find(|x| x.name == m || x.name.split('(').next() == Some(m)).and_then(|x| x.source.clone()),
            None => c.source.clone(),
        }
    });
    let s = link.ok_or_else(|| format!("{class}{} has no code location (link it with link_code, or index the project)", method.map(|m| format!(".{m}")).unwrap_or_default()))?;
    Ok(json!({"id": s.id, "kind": s.kind, "qualified": s.symbol, "language": s.lang(), "file": s.file, "line": s.start_line, "end_line": s.end_line, "col": s.start_col}))
}

/// The reverse: which design element is at `file:line` (Neovim → Spide).
pub fn element_at(snap: &CodeSnapshot, design: &Design, file: &str, line: u32) -> Option<Value> {
    // innermost code node whose range contains the line
    let mut best: Option<(String, Option<String>, u32)> = None;
    for c in &snap.design.classes {
        // a class declared in one file can have methods defined in another (C++, Rust impls)
        if let Some(s) = c.source.as_ref().filter(|s| s.file == file) {
            if s.start_line <= line && line <= s.end_line.max(s.start_line) && best.as_ref().is_none_or(|b| s.end_line - s.start_line < b.2) {
                best = Some((c.name.clone(), None, s.end_line - s.start_line));
            }
        }
        for m in &c.methods {
            if let Some(ms) = m.source.as_ref().filter(|x| x.file == file && x.start_line <= line && line <= x.end_line.max(x.start_line)) {
                let span = ms.end_line - ms.start_line;
                if best.as_ref().is_none_or(|b| span <= b.2) {
                    best = Some((c.name.clone(), Some(m.name.clone()), span));
                }
            }
        }
    }
    let (code_class, method, _) = best?;
    let symbol = snap.design.class(&code_class).and_then(|c| c.source.as_ref()).map(|s| s.symbol.clone());
    // the design element: linked to the same symbol, else the same name
    let design_class = design
        .classes
        .iter()
        .find(|c| symbol.is_some() && c.source.as_ref().map(|s| &s.symbol) == symbol.as_ref())
        .or_else(|| design.class(&code_class))
        .map(|c| c.name.clone());
    Some(json!({"code_class": code_class, "method": method, "symbol": symbol, "design_class": design_class, "in_design": design_class.is_some()}))
}

/// A workspace path that cannot escape the root (symlinks resolved).
pub fn safe_path(root: &Path, rel: &str) -> Result<PathBuf, String> {
    let canon_root = root.canonicalize().map_err(|e| e.to_string())?;
    let p = Path::new(rel);
    let joined = if p.is_absolute() { p.to_path_buf() } else { canon_root.join(p) };
    let c = joined.canonicalize().map_err(|e| format!("{rel}: {e}"))?;
    if !c.starts_with(&canon_root) {
        return Err(format!("{rel} is outside the workspace"));
    }
    Ok(c)
}

/// Numbered source lines (bounded).
pub fn source(root: &Path, file: &str, start: u32, end: u32) -> Result<Value, String> {
    let p = safe_path(root, file)?;
    let text = std::fs::read_to_string(&p).map_err(|e| e.to_string())?;
    let start = start.max(1);
    let end = end.max(start).min(start + 400);
    let lines: Vec<String> = text.lines().enumerate().skip(start as usize - 1).take((end - start + 1) as usize).map(|(i, l)| format!("{:>5}  {l}", i + 1)).collect();
    Ok(json!({"file": file, "start_line": start, "end_line": start + lines.len().saturating_sub(1) as u32, "hash": format!("{:016x}", spide_lsp::diagnostics::fnv64(text.as_bytes())), "text": lines.join("\n")}))
}

/// Evidence persisted in `.spide/evidence.json`.
pub struct EvidenceStore {
    path: PathBuf,
    pub set: Mutex<EvidenceSet>,
}

impl EvidenceStore {
    pub fn new(root: &Path) -> EvidenceStore {
        let path = root.join(".spide").join("evidence.json");
        let set = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        EvidenceStore { path, set: Mutex::new(set) }
    }
    pub fn save(&self) {
        let text = serde_json::to_string(&*self.set.lock().unwrap()).unwrap_or_default();
        let _ = crate::workspace::write_atomic(&self.path, &text);
    }
    /// Attach graph entity ids to findings and drop findings computed on older file versions.
    pub fn refresh(&self, snap: &CodeSnapshot) {
        let mut s = self.set.lock().unwrap();
        let hashes = snap.graph.documents.iter().map(|(k, d)| (k.clone(), d.hash.clone())).collect();
        s.drop_stale(&hashes);
        for f in &mut s.findings {
            if f.entity.is_none() {
                f.entity = snap.graph.at(&f.file, f.range.line).map(|e| e.id.clone());
            }
        }
    }
    pub fn query(&self, args: &Value) -> Value {
        let s = self.set.lock().unwrap();
        let file = args.get("file").and_then(Value::as_str);
        let cat = args.get("category").and_then(Value::as_str);
        let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(100).clamp(1, 1000) as usize;
        let hits: Vec<&spide_core::evidence::Finding> = s.findings.iter().filter(|f| file.is_none_or(|x| f.file == x) && cat.is_none_or(|c| f.category.as_str() == c)).collect();
        let mut by_cat = std::collections::BTreeMap::<&str, usize>::new();
        for f in &s.findings {
            *by_cat.entry(f.category.as_str()).or_default() += 1;
        }
        json!({"total": hits.len(), "by_category": by_cat, "findings": hits.into_iter().take(limit).collect::<Vec<_>>(), "runs": s.runs})
    }
}
