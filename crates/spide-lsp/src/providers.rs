//! Running the evidence providers for a workspace: language servers (diagnostics, symbol
//! capability check) and linters, with timeouts and cancellation. Only for trusted workspaces —
//! these tools execute project code (build scripts, macros, plugins).

use crate::client::{LspClient, ServerConfig};
use crate::defaults::{compile_db, defaults, linters, lsp_argv, resolve};
use crate::diagnostics::{from_lsp, parse_linter};
use serde::Serialize;
use spide_core::evidence::{EvidenceSet, ToolRun};
use spide_core::project::Manifest;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct Options {
    pub timeout: Duration,
    /// Quiet period after the last diagnostics publication.
    pub settle: Duration,
    pub max_files_per_server: usize,
    pub run_lsp: bool,
    pub run_linters: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options { timeout: Duration::from_secs(120), settle: Duration::from_millis(1500), max_files_per_server: 300, run_lsp: true, run_linters: true }
    }
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct LspReport {
    pub language: String,
    pub server: String,
    pub encoding: String,
    /// Advertised features we use: documentSymbol, definition, references, typeHierarchy, callHierarchy, rename.
    pub features: Vec<String>,
    pub files_without_diagnostics: usize,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct Analysis {
    pub evidence: EvidenceSet,
    pub servers: Vec<LspReport>,
    pub cancelled: bool,
}

fn now() -> u64 {
    spide_core::store::now_ms()
}

/// Run a linter (argv with `{files}`/`{compile_db}` placeholders) and parse its output.
pub fn run_linter(root: &Path, lang: &str, name: &str, argv: &[String], files: &[String], env: &std::collections::BTreeMap<String, String>, timeout: Duration, cancel: &AtomicBool) -> (ToolRun, Vec<spide_core::evidence::Finding>) {
    let mut run = ToolRun { tool: name.into(), language: lang.into(), at: now(), ..Default::default() };
    let mut args: Vec<String> = vec![];
    for a in argv {
        match a.as_str() {
            "{files}" => args.extend(files.iter().cloned()),
            "{compile_db}" => match compile_db(root) {
                Some(db) => args.push(format!("-p={}", db.parent().unwrap().display())),
                None => {
                    run.status = "not_configured".into();
                    run.detail = Some("no compile_commands.json: clang-tidy can't know the flags".into());
                    return (run, vec![]);
                }
            },
            _ => args.push(a.clone()),
        }
    }
    let Some(exe) = args.first().and_then(|e| resolve(root, e)) else {
        run.status = "missing".into();
        run.detail = defaults(lang).map(|d| d.linter_hint.to_string());
        return (run, vec![]);
    };
    if name == "clippy" && !root.join("Cargo.toml").is_file() {
        run.status = "not_configured".into();
        run.detail = Some("no Cargo.toml at the project root".into());
        return (run, vec![]);
    }
    let mut cmd = Command::new(exe);
    cmd.args(&args[1..]).current_dir(root).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    for (k, v) in env {
        cmd.env(k, v);
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            run.status = "failed".into();
            run.detail = Some(e.to_string());
            return (run, vec![]);
        }
    };
    let out = child.stdout.take();
    let reader = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(mut o) = out {
            use std::io::Read;
            let _ = o.read_to_string(&mut s);
        }
        s
    });
    let start = Instant::now();
    let status = loop {
        if let Ok(Some(s)) = child.try_wait() {
            break Some(s);
        }
        if start.elapsed() > timeout || cancel.load(Ordering::SeqCst) {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let stdout = reader.join().unwrap_or_default();
    match status {
        None => {
            run.status = if cancel.load(Ordering::SeqCst) { "cancelled" } else { "timed_out" }.into();
            (run, vec![])
        }
        Some(s) => {
            run.exit_code = s.code();
            match parse_linter(name, lang, &stdout, root) {
                Ok(f) => {
                    // linters exit non-zero when they find something; that is still a run
                    run.status = "ran".into();
                    run.findings = f.len();
                    (run, f)
                }
                Err(e) => {
                    run.status = "failed".into();
                    run.detail = Some(e);
                    (run, vec![])
                }
            }
        }
    }
}

/// Start a server, open `files`, collect diagnostics.
pub fn run_lsp(root: &Path, lang: &str, argv: Vec<String>, m: &Manifest, files: &[PathBuf], opts: &Options) -> (ToolRun, Option<LspReport>, Vec<spide_core::evidence::Finding>) {
    let t = m.tools.get(lang);
    let mut run = ToolRun { tool: argv.first().cloned().unwrap_or_default(), language: lang.into(), at: now(), ..Default::default() };
    if lang == "java" && !["pom.xml", "build.gradle", "build.gradle.kts", ".classpath", "settings.gradle", "settings.gradle.kts"].iter().any(|f| root.join(f).is_file()) {
        // without a build, jdtls can't see packages/classpath: its diagnostics would be noise
        run.status = "not_configured".into();
        run.detail = Some("no pom.xml/build.gradle/.classpath at the project root".into());
        return (run, None, vec![]);
    }
    if argv.first().and_then(|e| resolve(root, e)).is_none() {
        run.status = "missing".into();
        run.detail = Some(format!("{} not found on PATH", run.tool));
        return (run, None, vec![]);
    }
    let cfg = ServerConfig { argv, root: root.to_path_buf(), env: t.map(|t| t.env.clone()).unwrap_or_default(), init_options: t.and_then(|t| t.init_options.clone()), settings: t.and_then(|t| t.settings.clone()), timeout: opts.timeout.min(Duration::from_secs(60)) };
    let client = match LspClient::start(&cfg) {
        Ok(c) => c,
        Err(e) => {
            run.status = "failed".into();
            run.detail = Some(e);
            return (run, None, vec![]);
        }
    };
    let lang_id = defaults(lang).map(|d| d.language_id).unwrap_or(lang);
    let files: Vec<PathBuf> = files.iter().take(opts.max_files_per_server).cloned().collect();
    let mut texts = vec![];
    for f in &files {
        let text = std::fs::read_to_string(f).unwrap_or_default();
        let id = if f.extension().is_some_and(|e| e == "tsx") { "typescriptreact" } else { lang_id };
        if client.open(f, id, &text).is_err() {
            break;
        }
        texts.push(text);
    }
    let missing = client.wait_diagnostics(&files, opts.settle, opts.timeout);
    let mut findings = vec![];
    let canon = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    for (f, text) in files.iter().zip(&texts) {
        if let Some(p) = client.diagnostics(f) {
            let rel = f.canonicalize().ok().and_then(|a| a.strip_prefix(&canon).ok().map(|r| r.to_string_lossy().replace('\\', "/"))).unwrap_or_else(|| f.display().to_string());
            let hash = format!("{:016x}", crate::diagnostics::fnv64(text.as_bytes()));
            findings.extend(from_lsp(&p.diagnostics, &rel, lang, text, client.encoding(), &client.server_name(), Some(hash)));
        }
    }
    let features: Vec<String> = ["documentSymbolProvider", "definitionProvider", "referencesProvider", "typeHierarchyProvider", "callHierarchyProvider", "renameProvider"].iter().filter(|c| client.has(c)).map(|c| c.trim_end_matches("Provider").to_string()).collect();
    let report = LspReport { language: lang.into(), server: client.server_name(), encoding: client.encoding().as_str().into(), features, files_without_diagnostics: missing.len() };
    run.status = if client.alive() { "ran" } else { "failed" }.into();
    if !client.alive() {
        run.detail = Some("the server exited during analysis".into());
    }
    run.findings = findings.len();
    client.shutdown();
    (run, Some(report), findings)
}

/// Analyse a workspace: every language's LSP diagnostics and linters. `files`: language →
/// absolute paths.
pub fn analyze(root: &Path, m: &Manifest, files: &std::collections::BTreeMap<String, Vec<PathBuf>>, opts: &Options, cancel: Arc<AtomicBool>) -> Analysis {
    let mut a = Analysis::default();
    for lang in m.all_languages() {
        if cancel.load(Ordering::SeqCst) {
            a.cancelled = true;
            break;
        }
        let t = m.tools.get(&lang);
        if t.and_then(|t| t.enabled) == Some(false) {
            a.evidence.record_run(ToolRun { tool: "all".into(), language: lang.clone(), status: "disabled".into(), at: now(), ..Default::default() });
            continue;
        }
        let fs = files.get(&lang).cloned().unwrap_or_default();
        if fs.is_empty() {
            continue;
        }
        let canon = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        let rels: Vec<String> = fs.iter().map(|f| f.strip_prefix(&canon).or_else(|_| f.strip_prefix(root)).map(|r| r.display().to_string()).unwrap_or_else(|_| f.display().to_string())).collect();
        if opts.run_linters {
            for (name, argv) in linters(&lang, t) {
                let (run, findings) = run_linter(root, &lang, &name, &argv, &rels, &t.map(|t| t.env.clone()).unwrap_or_default(), opts.timeout, &cancel);
                a.evidence.record_run(run);
                a.evidence.add(findings);
            }
        }
        if opts.run_lsp {
            if let Some(argv) = lsp_argv(root, &lang, t) {
                let (run, report, findings) = run_lsp(root, &lang, argv, m, &fs, opts);
                a.evidence.record_run(run);
                a.evidence.add(findings);
                a.servers.extend(report);
            }
        }
    }
    a
}
