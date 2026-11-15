//! Default analysis tooling per language, overridable in `spide/project.json` (`tools`), and
//! `doctor`: which tools are actually available here.

use crate::client::which;
use serde::Serialize;
use spide_core::project::{LanguageTools, Manifest};
use std::collections::BTreeMap;
use std::path::Path;

/// (language, LSP candidates, linters (name, argv), LSP languageId)
pub struct Defaults {
    pub language: &'static str,
    pub lsp: &'static [&'static [&'static str]],
    pub linters: &'static [(&'static str, &'static [&'static str])],
    pub language_id: &'static str,
    pub linter_hint: &'static str,
}

pub const DEFAULTS: &[Defaults] = &[
    Defaults { language: "rust", lsp: &[&["rust-analyzer"]], linters: &[("clippy", &["cargo", "clippy", "--message-format=json", "--quiet"])], language_id: "rust", linter_hint: "rustup component add clippy" },
    Defaults {
        language: "typescript",
        lsp: &[&["vtsls", "--stdio"], &["typescript-language-server", "--stdio"]],
        linters: &[("eslint", &["eslint", "-f", "json", "{files}"])],
        language_id: "typescript",
        linter_hint: "npm i -D eslint (node_modules/.bin/eslint is preferred)",
    },
    Defaults {
        language: "python",
        lsp: &[&["pyright-langserver", "--stdio"], &["basedpyright-langserver", "--stdio"], &["pylsp"]],
        linters: &[("ruff", &["ruff", "check", "--output-format", "json", "--exit-zero", "{files}"])],
        language_id: "python",
        linter_hint: "pipx install ruff",
    },
    Defaults { language: "java", lsp: &[&["jdtls"]], linters: &[], language_id: "java", linter_hint: "configure a linter under tools.java.linters (e.g. checkstyle) if wanted" },
    Defaults { language: "c", lsp: &[&["clangd", "--background-index=false"]], linters: &[("clang-tidy", &["clang-tidy", "--quiet", "{compile_db}", "{files}"])], language_id: "c", linter_hint: "install LLVM (clang-tidy) and generate compile_commands.json" },
    Defaults { language: "cpp", lsp: &[&["clangd", "--background-index=false"]], linters: &[("clang-tidy", &["clang-tidy", "--quiet", "{compile_db}", "{files}"])], language_id: "cpp", linter_hint: "install LLVM (clang-tidy) and generate compile_commands.json" },
    Defaults { language: "clojure", lsp: &[&["clojure-lsp"]], linters: &[("clj-kondo", &["clj-kondo", "--config", "{:output {:format :json}}", "--lint", "{files}"])], language_id: "clojure", linter_hint: "brew install clj-kondo" },
];

pub fn defaults(lang: &str) -> Option<&'static Defaults> {
    DEFAULTS.iter().find(|d| d.language == lang)
}

/// Resolve an executable: project-local `node_modules/.bin` first (eslint), then PATH, then the
/// tools Neovim's mason installed (`$XDG_DATA_HOME/nvim/mason/bin`), so Spide reuses the editor's
/// language servers instead of asking for a second installation.
pub fn resolve(root: &Path, exe: &str) -> Option<std::path::PathBuf> {
    let local = root.join("node_modules/.bin").join(exe);
    if local.is_file() {
        return Some(local);
    }
    which(exe).or_else(|| {
        if exe.contains('/') {
            return None;
        }
        let data = std::env::var_os("XDG_DATA_HOME").map(std::path::PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/share")))?;
        let p = data.join("nvim/mason/bin").join(exe);
        p.is_file().then_some(p)
    })
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ToolStatus {
    pub name: String,
    pub argv: Vec<String>,
    /// available | missing | disabled
    pub status: String,
    pub path: Option<String>,
    pub hint: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct LanguageStatus {
    pub language: String,
    pub lsp: ToolStatus,
    pub linters: Vec<ToolStatus>,
    pub parser: Vec<String>,
    pub limits: Vec<String>,
    /// Build configs that matter for accuracy (compile_commands.json, tsconfig.json, …) and whether present.
    pub configs: BTreeMap<String, bool>,
}

/// The LSP argv for a language: manifest override, else the first available default.
pub fn lsp_argv(root: &Path, lang: &str, t: Option<&LanguageTools>) -> Option<Vec<String>> {
    if let Some(argv) = t.and_then(|t| t.lsp.clone()).filter(|a| !a.is_empty()) {
        return Some(argv);
    }
    let d = defaults(lang)?;
    let pick = d.lsp.iter().find(|c| resolve(root, c[0]).is_some()).or(d.lsp.first())?;
    Some(pick.iter().map(|s| s.to_string()).collect())
}

/// Linters for a language: manifest's (if any are configured) else defaults.
pub fn linters(lang: &str, t: Option<&LanguageTools>) -> Vec<(String, Vec<String>)> {
    if let Some(t) = t.filter(|t| !t.linters.is_empty()) {
        return t.linters.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    }
    defaults(lang).map(|d| d.linters.iter().map(|(n, a)| (n.to_string(), a.iter().map(|s| s.to_string()).collect())).collect()).unwrap_or_default()
}

pub fn compile_db(root: &Path) -> Option<std::path::PathBuf> {
    ["compile_commands.json", "build/compile_commands.json", "out/compile_commands.json", "cmake-build-debug/compile_commands.json"].iter().map(|p| root.join(p)).find(|p| p.is_file())
}

fn status(root: &Path, name: &str, argv: Vec<String>, enabled: bool, hint: &str) -> ToolStatus {
    let path = argv.first().and_then(|e| resolve(root, e));
    ToolStatus {
        name: name.to_string(),
        status: if !enabled { "disabled" } else if path.is_some() { "available" } else { "missing" }.into(),
        path: path.map(|p| p.display().to_string()),
        hint: (enabled && argv.first().and_then(|e| resolve(root, e)).is_none()).then(|| hint.to_string()),
        argv,
    }
}

/// What can run for each project language.
pub fn doctor(root: &Path, m: &Manifest) -> Vec<LanguageStatus> {
    m.all_languages()
        .iter()
        .map(|lang| {
            let t = m.tools.get(lang);
            let enabled = t.and_then(|t| t.enabled).unwrap_or(true);
            let d = defaults(lang);
            let argv = lsp_argv(root, lang, t).unwrap_or_default();
            let lsp_name = argv.first().cloned().unwrap_or_default();
            let mut configs = BTreeMap::new();
            match lang.as_str() {
                "c" | "cpp" => {
                    configs.insert("compile_commands.json".into(), compile_db(root).is_some());
                }
                "typescript" => {
                    configs.insert("tsconfig.json".into(), root.join("tsconfig.json").is_file());
                }
                "rust" => {
                    configs.insert("Cargo.toml".into(), root.join("Cargo.toml").is_file());
                }
                "java" => {
                    configs.insert("pom.xml|build.gradle".into(), ["pom.xml", "build.gradle", "build.gradle.kts", ".classpath"].iter().any(|f| root.join(f).is_file()));
                }
                "python" => {
                    configs.insert("pyproject.toml".into(), root.join("pyproject.toml").is_file());
                }
                "clojure" => {
                    configs.insert("deps.edn|project.clj".into(), root.join("deps.edn").is_file() || root.join("project.clj").is_file());
                    configs.insert(".lsp/config.edn".into(), root.join(".lsp/config.edn").is_file());
                }
                _ => {}
            }
            let caps = spide_core::symbols::LANGUAGES.contains(&lang.as_str());
            LanguageStatus {
                language: lang.clone(),
                lsp: status(root, &lsp_name, argv, enabled, &format!("install {lsp_name} or set tools.{lang}.lsp in spide/project.json")),
                linters: linters(lang, t).into_iter().map(|(n, a)| status(root, &n, a, enabled, d.map(|d| d.linter_hint).unwrap_or(""))).collect(),
                parser: if caps { vec!["tree-sitter".into()] } else { vec![] },
                limits: vec![],
                configs,
            }
        })
        .collect()
}
