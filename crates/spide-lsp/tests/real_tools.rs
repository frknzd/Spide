//! Real language servers and linters against the `poly` fixtures. Ignored by default (needs the
//! tools installed); run with `cargo test -p spide-lsp --test real_tools -- --ignored --nocapture`.
//! A missing tool is reported and skipped, never counted as a clean run.

use spide_lsp::defaults::{lsp_argv, resolve};
use spide_lsp::providers::{run_linter, run_lsp, Options};
use std::path::PathBuf;
use std::time::Duration;

fn poly(dir: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../spide-import/tests/fixtures/poly").join(dir).canonicalize().unwrap()
}

fn files(root: &PathBuf, exts: &[&str]) -> Vec<PathBuf> {
    let mut out = vec![];
    let mut stack = vec![root.clone()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() && !p.ends_with("target") {
                stack.push(p);
            } else if p.extension().and_then(|e| e.to_str()).is_some_and(|e| exts.contains(&e)) {
                out.push(p);
            }
        }
    }
    out
}

#[test]
#[ignore]
fn every_language_server_on_its_fixture() {
    let cases = [("java", "../library", &["java"][..]), ("rust", "rust", &["rs"]), ("typescript", "ts", &["ts"]), ("python", "py", &["py"]), ("c", "c", &["c", "h"]), ("cpp", "cpp", &["cpp", "hpp"]), ("clojure", "clj", &["clj"])];
    let mut ran = 0;
    for (lang, dir, exts) in cases {
        let root = poly(dir);
        let m = spide_core::project::Manifest { language: lang.into(), ..Default::default() };
        let argv = lsp_argv(&root, lang, None).unwrap();
        let opts = Options { timeout: Duration::from_secs(90), settle: Duration::from_secs(2), ..Default::default() };
        let (run, report, findings) = run_lsp(&root, lang, argv.clone(), &m, &files(&root, exts), &opts);
        println!("{lang:<11} {:<24} {:<10} {:?} findings={} {:?}", argv[0], run.status, report.as_ref().map(|r| (&r.server, &r.encoding, &r.features)), findings.len(), run.detail);
        for f in findings.iter().take(5) {
            println!("    {}:{}:{} {:?} {} [{}] {}", f.file, f.range.line, f.range.col, f.category, f.tool, f.code, f.message.lines().next().unwrap_or(""));
        }
        if run.status == "ran" {
            ran += 1;
            assert!(report.unwrap().features.contains(&"documentSymbol".to_string()));
        } else {
            assert!(matches!(run.status.as_str(), "missing" | "failed" | "not_configured"), "{lang}: {}", run.status);
        }
    }
    println!("{ran} language servers ran");
}

#[test]
#[ignore]
fn linters_on_fixtures() {
    for (lang, dir, name, argv) in [
        ("python", "py", "ruff", vec!["ruff", "check", "--output-format", "json", "--exit-zero", "{files}"]),
        ("clojure", "clj", "clj-kondo", vec!["clj-kondo", "--config", "{:output {:format :json}}", "--lint", "{files}"]),
    ] {
        let root = poly(dir);
        if resolve(&root, argv[0]).is_none() {
            println!("{name}: missing (skipped)");
            continue;
        }
        let argv: Vec<String> = argv.into_iter().map(String::from).collect();
        let fs: Vec<String> = files(&root, &[if lang == "python" { "py" } else { "clj" }]).iter().map(|f| f.strip_prefix(&root).unwrap().display().to_string()).collect();
        let (run, findings) = run_linter(&root, lang, name, &argv, &fs, &Default::default(), Duration::from_secs(60), &Default::default());
        println!("{name}: {} exit={:?} findings={} {:?}", run.status, run.exit_code, findings.len(), run.detail);
        for f in &findings {
            println!("    {}:{}:{} {:?} [{}] {}", f.file, f.range.line, f.range.col, f.category, f.code, f.message);
        }
        assert_eq!(run.status, "ran");
    }
}
