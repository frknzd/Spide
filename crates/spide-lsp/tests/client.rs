//! The LSP client against the scripted mock server: encodings, server→client requests,
//! diagnostics, timeouts and crashes.

use serde_json::json;
use spide_core::evidence::Level;
use spide_lsp::client::{LspClient, ServerConfig};
use spide_lsp::diagnostics::from_lsp;
use std::time::Duration;

const MOCK: &str = env!("CARGO_BIN_EXE_spide-mock-lsp");

fn cfg(dir: &std::path::Path, extra: &[&str]) -> ServerConfig {
    let mut argv = vec![MOCK.to_string()];
    argv.extend(extra.iter().map(|s| s.to_string()));
    ServerConfig { argv, root: dir.to_path_buf(), env: Default::default(), init_options: None, settings: Some(json!({"mock":{"level":"strict"}})), timeout: Duration::from_secs(5) }
}

#[test]
fn diagnostics_with_utf16_and_utf8_encodings() {
    for (flag, enc) in [(&[][..], "utf-16"), (&["--utf8"][..], "utf-8")] {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("a.rs");
        let text = "fn main() {}\nlet 🦀é = 1; // TODO\n";
        std::fs::write(&f, text).unwrap();
        let c = LspClient::start(&cfg(d.path(), flag)).unwrap();
        assert_eq!(c.encoding().as_str(), enc);
        assert_eq!(c.server_name(), "mock-ls");
        // let the configuration round trip happen before opening
        std::thread::sleep(Duration::from_millis(100));
        c.open(&f, "rust", text).unwrap();
        assert!(c.wait_diagnostics(&[f.clone()], Duration::from_millis(100), Duration::from_secs(5)).is_empty());
        let p = c.diagnostics(&f).unwrap();
        let found = from_lsp(&p.diagnostics, "a.rs", "rust", text, c.encoding(), &c.server_name(), None);
        let todo = found.iter().find(|x| x.code == "todo").unwrap();
        assert_eq!((todo.range.line, todo.range.col), (2, "let 🦀é = 1; // ".len() as u32), "{enc}");
        assert_eq!(todo.level, Level::Warning);
        let config = found.iter().find(|x| x.code == "config").expect("server asked for workspace/configuration");
        assert!(config.message.contains("strict"), "{}", config.message);
        assert!(c.document_symbols(&f).unwrap().as_array().unwrap().len() == 1);
        assert!(c.references(&f, 0, 0).unwrap_err().contains("does not provide references"));
        c.shutdown();
        assert!(!c.alive() || c.request_timeout("x", json!({}), Duration::from_millis(200)).is_err());
    }
}

#[test]
fn initialize_timeout_and_crash_fail_fast() {
    let d = tempfile::tempdir().unwrap();
    let mut c = cfg(d.path(), &["--hang"]);
    c.timeout = Duration::from_millis(300);
    let start = std::time::Instant::now();
    assert!(LspClient::start(&c).err().unwrap().contains("timed out"));
    assert!(start.elapsed() < Duration::from_secs(3));

    let f = d.path().join("a.rs");
    std::fs::write(&f, "x").unwrap();
    let client = LspClient::start(&cfg(d.path(), &["--crash-after-open"])).unwrap();
    client.open(&f, "rust", "x").unwrap();
    let missing = client.wait_diagnostics(&[f.clone()], Duration::from_millis(50), Duration::from_secs(5));
    assert_eq!(missing.len(), 1);
    assert!(!client.alive());
    assert!(client.document_symbols(&f).is_err());
}

#[test]
fn missing_server_is_reported_not_clean() {
    let d = tempfile::tempdir().unwrap();
    let f = d.path().join("a.rs");
    std::fs::write(&f, "x").unwrap();
    let m = spide_core::project::Manifest { language: "rust".into(), ..Default::default() };
    let (run, report, findings) = spide_lsp::providers::run_lsp(d.path(), "rust", vec!["definitely-not-a-server-xyz".into()], &m, &[f], &Default::default());
    assert_eq!(run.status, "missing");
    assert!(report.is_none() && findings.is_empty());
}

#[test]
fn provider_run_with_mock_server_collects_findings() {
    let d = tempfile::tempdir().unwrap();
    let f = d.path().join("a.rs");
    std::fs::write(&f, "// TODO one\n").unwrap();
    let mut m = spide_core::project::Manifest { language: "rust".into(), ..Default::default() };
    m.tools.insert("rust".into(), spide_core::project::LanguageTools { lsp: Some(vec![MOCK.into()]), linters: [("nothing".to_string(), vec!["definitely-not-a-linter-xyz".to_string()])].into(), ..Default::default() });
    let files = [("rust".to_string(), vec![f])].into();
    let a = spide_lsp::providers::analyze(d.path(), &m, &files, &spide_lsp::providers::Options { settle: Duration::from_millis(100), timeout: Duration::from_secs(5), ..Default::default() }, Default::default());
    assert!(a.evidence.findings.iter().any(|x| x.code == "todo" && x.file == "a.rs"), "{:?}", a.evidence.findings);
    let runs: Vec<(&str, &str)> = a.evidence.runs.iter().map(|r| (r.tool.as_str(), r.status.as_str())).collect();
    assert!(runs.contains(&("nothing", "missing")), "{runs:?}");
    assert!(a.servers[0].features.contains(&"documentSymbol".to_string()));
}

#[test]
fn doctor_reports_every_language() {
    let d = tempfile::tempdir().unwrap();
    let m = spide_core::project::Manifest { language: "rust".into(), languages: vec!["typescript".into(), "python".into(), "java".into(), "c".into(), "cpp".into(), "clojure".into()], ..Default::default() };
    let r = spide_lsp::defaults::doctor(d.path(), &m);
    assert_eq!(r.len(), 7);
    for l in &r {
        assert!(matches!(l.lsp.status.as_str(), "available" | "missing"), "{l:?}");
    }
    assert_eq!(r.iter().find(|l| l.language == "cpp").unwrap().configs["compile_commands.json"], false);
}
