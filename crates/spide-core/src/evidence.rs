//! Evidence from outside the design: compiler/LSP diagnostics and linter findings, normalised
//! into tool-independent categories, deduplicated across providers, with the runs that produced
//! them. A tool that did not run (missing, untrusted, crashed) is recorded as such: missing
//! evidence is *unknown*, never a clean bill of health.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord, Hash, Default)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    /// Type errors, undefined names, compile errors, likely bugs.
    Correctness,
    /// Memory/undefined-behaviour/security hazards.
    Safety,
    /// Unused code, deprecated APIs, error-prone patterns that aren't bugs yet.
    Maintainability,
    /// Too complex/long/deeply nested.
    Complexity,
    Performance,
    /// Formatting, naming, idiom style.
    Style,
    #[default]
    Unknown,
}

impl Category {
    pub fn as_str(self) -> &'static str {
        match self {
            Category::Correctness => "correctness",
            Category::Safety => "safety",
            Category::Maintainability => "maintainability",
            Category::Complexity => "complexity",
            Category::Performance => "performance",
            Category::Style => "style",
            Category::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord, Hash, Default)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Error,
    #[default]
    Warning,
    Info,
    Hint,
}

impl Level {
    /// LSP DiagnosticSeverity (1 = error … 4 = hint).
    pub fn from_lsp(n: u64) -> Level {
        match n {
            1 => Level::Error,
            2 => Level::Warning,
            3 => Level::Info,
            _ => Level::Hint,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default, PartialOrd, Ord, Hash)]
#[serde(default)]
pub struct Range {
    /// 1-based line, 0-based UTF-8 byte column.
    pub line: u32,
    pub col: u32,
    pub end_line: u32,
    pub end_col: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Finding {
    /// Tool-specific code (`clippy::too_many_arguments`, `E0308`, `F401`, `no-explicit-any`).
    pub code: String,
    /// Tool that reported it: rustc, clippy, tsc, eslint, pyright, ruff, javac, clang, clang-tidy, clj-kondo, …
    pub tool: String,
    /// lsp | linter | parser
    pub provider: String,
    pub category: Category,
    pub level: Level,
    pub language: String,
    pub file: String,
    pub range: Range,
    pub message: String,
    /// Graph entity (symbols::Entity id) containing the range, when resolved.
    pub entity: Option<String>,
    /// Content hash of the document it was computed on (stale when the file changed).
    pub doc_hash: Option<String>,
}

impl Finding {
    /// Root-cause key used for deduplication across providers (LSP diagnostics often repeat the
    /// linter's findings): same file, line, and code (or message when codes differ by tool).
    pub fn key(&self) -> (String, u32, String) {
        let code = self.code.rsplit("::").next().unwrap_or(&self.code).to_ascii_lowercase();
        (self.file.clone(), self.range.line, if code.is_empty() { self.message.to_ascii_lowercase() } else { code })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ToolRun {
    pub tool: String,
    pub language: String,
    /// ran | missing | untrusted | failed | timed_out | not_configured
    pub status: String,
    pub exit_code: Option<i32>,
    pub findings: usize,
    pub at: u64,
    pub detail: Option<String>,
}

impl ToolRun {
    pub fn ran(&self) -> bool {
        self.status == "ran"
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct EvidenceSet {
    pub findings: Vec<Finding>,
    pub runs: Vec<ToolRun>,
}

impl EvidenceSet {
    /// Add findings, keeping one per root cause (the more severe; ties: the linter's, which has
    /// the more specific code).
    pub fn add(&mut self, items: impl IntoIterator<Item = Finding>) {
        let mut by: BTreeMap<(String, u32, String), usize> = self.findings.iter().enumerate().map(|(i, f)| (f.key(), i)).collect();
        for f in items {
            match by.get(&f.key()) {
                Some(&i) => {
                    let cur = &self.findings[i];
                    if f.level < cur.level || (f.level == cur.level && f.provider == "linter" && cur.provider != "linter") {
                        self.findings[i] = f;
                    }
                }
                None => {
                    by.insert(f.key(), self.findings.len());
                    self.findings.push(f);
                }
            }
        }
    }

    /// Replace everything one tool reported for one file set (a fresh run supersedes the old).
    pub fn replace_tool(&mut self, tool: &str, files: &BTreeSet<String>, items: Vec<Finding>) {
        self.findings.retain(|f| !(f.tool == tool && (files.is_empty() || files.contains(&f.file))));
        self.add(items);
    }

    pub fn record_run(&mut self, r: ToolRun) {
        self.runs.retain(|x| !(x.tool == r.tool && x.language == r.language));
        self.runs.push(r);
    }

    /// Drop findings computed on an older version of a file.
    pub fn drop_stale(&mut self, current: &BTreeMap<String, String>) -> usize {
        let before = self.findings.len();
        self.findings.retain(|f| match (&f.doc_hash, current.get(&f.file)) {
            (Some(h), Some(c)) => h == c,
            _ => true,
        });
        before - self.findings.len()
    }
}

/// Map a tool's code/message to a category. Conservative: unknown codes stay `Unknown`/by level.
pub fn categorize(tool: &str, code: &str, level: Level, message: &str) -> Category {
    let c = code.to_ascii_lowercase();
    let m = message.to_ascii_lowercase();
    let has = |xs: &[&str]| xs.iter().any(|x| c.contains(x) || m.contains(x));
    match tool {
        "clippy" => {
            if has(&["correctness", "suspicious", "unsound", "panic", "unwrap_used", "expect_used", "indexing_slicing"]) {
                Category::Correctness
            } else if has(&["complexity", "cognitive_complexity", "too_many_arguments", "too_many_lines", "type_complexity"]) {
                Category::Complexity
            } else if has(&["perf", "large_enum_variant", "box_collection"]) {
                Category::Performance
            } else if has(&["undocumented_unsafe", "mem_forget", "transmute"]) {
                Category::Safety
            } else if has(&["style", "pedantic", "needless", "redundant"]) {
                Category::Style
            } else {
                Category::Maintainability
            }
        }
        "ruff" | "pyflakes" | "flake8" => match c.chars().next() {
            Some('f') if c.starts_with("f8") => Category::Correctness, // undefined names
            Some('f') => Category::Maintainability,                   // unused imports/vars
            Some('e') if c.starts_with("e9") => Category::Correctness,
            Some('e' | 'w' | 'n' | 'd' | 'i' | 'q') => Category::Style,
            Some('b') => Category::Correctness, // bugbear
            Some('c') if c.starts_with("c90") => Category::Complexity,
            Some('s') => Category::Safety, // bandit
            Some('p') if c.starts_with("plr") => Category::Complexity,
            Some('p') if c.starts_with("perf") => Category::Performance,
            _ => Category::Maintainability,
        },
        "eslint" => {
            if has(&["complexity", "max-depth", "max-lines", "max-params", "max-statements", "cognitive"]) {
                Category::Complexity
            } else if has(&["no-explicit-any", "no-unsafe", "no-floating-promises", "no-misused-promises", "no-undef", "no-unreachable", "no-dupe", "eqeqeq", "use-isnan", "valid-typeof"]) {
                Category::Correctness
            } else if has(&["no-eval", "no-implied-eval", "security"]) {
                Category::Safety
            } else if has(&["unused", "deprecat"]) {
                Category::Maintainability
            } else {
                Category::Style
            }
        }
        "clang-tidy" => {
            if has(&["bugprone", "clang-analyzer", "cppcoreguidelines-pro-bounds", "cert-"]) {
                Category::Correctness
            } else if has(&["cppcoreguidelines-owning-memory", "cppcoreguidelines-no-malloc", "pro-type", "security", "insecure"]) {
                Category::Safety
            } else if has(&["readability-function-cognitive-complexity", "readability-function-size"]) {
                Category::Complexity
            } else if has(&["performance"]) {
                Category::Performance
            } else if has(&["modernize", "readability", "llvm-", "google-"]) {
                Category::Style
            } else {
                Category::Maintainability
            }
        }
        "clj-kondo" => {
            if has(&["unresolved", "invalid-arity", "type-mismatch", "syntax", "redefined-var", "loop-without-recur"]) {
                Category::Correctness
            } else if has(&["unused", "deprecated", "redundant"]) {
                Category::Maintainability
            } else {
                Category::Style
            }
        }
        // compilers / type checkers: errors are correctness; warnings mostly maintainability
        _ => {
            if has(&["deprecat", "unused", "never used", "dead code", "unreachable"]) {
                Category::Maintainability
            } else if level == Level::Error {
                Category::Correctness
            } else if has(&["unsafe", "overflow", "uninitialized", "null pointer", "use after"]) {
                Category::Safety
            } else if level == Level::Warning {
                Category::Maintainability
            } else {
                Category::Style
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(tool: &str, provider: &str, code: &str, line: u32, level: Level) -> Finding {
        Finding { tool: tool.into(), provider: provider.into(), code: code.into(), file: "a.rs".into(), range: Range { line, ..Default::default() }, level, ..Default::default() }
    }

    #[test]
    fn dedup_keeps_one_per_root_cause() {
        let mut e = EvidenceSet::default();
        e.add([f("rust-analyzer", "lsp", "clippy::needless_return", 3, Level::Warning), f("clippy", "linter", "needless_return", 3, Level::Warning), f("clippy", "linter", "needless_return", 4, Level::Warning)]);
        assert_eq!(e.findings.len(), 2);
        assert_eq!(e.findings[0].provider, "linter");
        e.replace_tool("clippy", &BTreeSet::new(), vec![]);
        assert!(e.findings.is_empty(), "the linter's replacement owned both");
    }

    #[test]
    fn categories() {
        assert_eq!(categorize("ruff", "F821", Level::Error, ""), Category::Correctness);
        assert_eq!(categorize("ruff", "F401", Level::Warning, ""), Category::Maintainability);
        assert_eq!(categorize("ruff", "E501", Level::Warning, ""), Category::Style);
        assert_eq!(categorize("ruff", "C901", Level::Warning, ""), Category::Complexity);
        assert_eq!(categorize("clippy", "clippy::too_many_arguments", Level::Warning, ""), Category::Complexity);
        assert_eq!(categorize("eslint", "@typescript-eslint/no-explicit-any", Level::Warning, ""), Category::Correctness);
        assert_eq!(categorize("clang-tidy", "bugprone-use-after-move", Level::Warning, ""), Category::Correctness);
        assert_eq!(categorize("clj-kondo", "unresolved-symbol", Level::Error, ""), Category::Correctness);
        assert_eq!(categorize("rustc", "E0308", Level::Error, "mismatched types"), Category::Correctness);
        assert_eq!(categorize("tsc", "6133", Level::Hint, "'x' is declared but never used"), Category::Maintainability);
    }

    #[test]
    fn stale_findings_are_dropped() {
        let mut e = EvidenceSet::default();
        e.add([Finding { doc_hash: Some("old".into()), ..f("clippy", "linter", "x", 1, Level::Warning) }]);
        let cur: BTreeMap<String, String> = [("a.rs".to_string(), "new".to_string())].into();
        assert_eq!(e.drop_stale(&cur), 1);
    }
}
