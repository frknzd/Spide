//! Normalising LSP diagnostics and linter output into `evidence::Finding`s.

use crate::position::{line, to_byte_col, Encoding};
use serde_json::Value;
use spide_core::evidence::{categorize, Finding, Level, Range};
use std::collections::BTreeMap;
use std::path::Path;

/// Tool that produced an LSP diagnostic (its `source`, e.g. `rustc`, `clippy`, `ts`, `Pyright`).
fn tool_of(source: &str, server: &str) -> String {
    let s = source.to_ascii_lowercase();
    match s.as_str() {
        "" => server.to_ascii_lowercase(),
        "ts" | "typescript" => "tsc".into(),
        "pyright" | "basedpyright" => "pyright".into(),
        _ => s,
    }
}

fn code_str(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

/// LSP `Diagnostic`s for one file → findings (positions converted to UTF-8 byte columns).
pub fn from_lsp(diags: &[Value], rel: &str, language: &str, text: &str, enc: Encoding, server: &str, doc_hash: Option<String>) -> Vec<Finding> {
    diags
        .iter()
        .map(|d| {
            let l0 = d.pointer("/range/start/line").and_then(Value::as_u64).unwrap_or(0) as u32;
            let c0 = d.pointer("/range/start/character").and_then(Value::as_u64).unwrap_or(0) as u32;
            let l1 = d.pointer("/range/end/line").and_then(Value::as_u64).unwrap_or(l0 as u64) as u32;
            let c1 = d.pointer("/range/end/character").and_then(Value::as_u64).unwrap_or(c0 as u64) as u32;
            let level = Level::from_lsp(d.get("severity").and_then(Value::as_u64).unwrap_or(2));
            let tool = tool_of(d.get("source").and_then(Value::as_str).unwrap_or(""), server);
            let code = code_str(d.get("code"));
            let message = d.get("message").and_then(Value::as_str).unwrap_or("").to_string();
            Finding {
                category: categorize(&tool, &code, level, &message),
                code,
                tool,
                provider: "lsp".into(),
                level,
                language: language.into(),
                file: rel.into(),
                range: Range { line: l0 + 1, col: to_byte_col(line(text, l0), c0, enc), end_line: l1 + 1, end_col: to_byte_col(line(text, l1), c1, enc) },
                message,
                entity: None,
                doc_hash: doc_hash.clone(),
            }
        })
        .collect()
}

/// Reads files lazily to convert linter columns (1-based characters) to byte columns.
pub struct Files<'a> {
    root: &'a Path,
    cache: BTreeMap<String, String>,
}

impl<'a> Files<'a> {
    pub fn new(root: &'a Path) -> Self {
        Files { root, cache: BTreeMap::new() }
    }
    /// Workspace-relative path for whatever the tool printed (absolute or relative).
    pub fn rel(&self, p: &str) -> String {
        let path = Path::new(p);
        let canon_root = self.root.canonicalize().unwrap_or_else(|_| self.root.to_path_buf());
        let abs = if path.is_absolute() { path.to_path_buf() } else { self.root.join(path) };
        let abs = abs.canonicalize().unwrap_or(abs);
        abs.strip_prefix(&canon_root).map(|r| r.to_string_lossy().replace('\\', "/")).unwrap_or_else(|_| p.to_string())
    }
    fn col(&mut self, rel: &str, line1: u32, col1: u32, enc: Encoding) -> u32 {
        let text = self.cache.entry(rel.to_string()).or_insert_with(|| std::fs::read_to_string(self.root.join(rel)).unwrap_or_default());
        to_byte_col(line(text, line1.saturating_sub(1)), col1.saturating_sub(1), enc)
    }
    pub fn hash(&mut self, rel: &str) -> Option<String> {
        let text = self.cache.entry(rel.to_string()).or_insert_with(|| std::fs::read_to_string(self.root.join(rel)).unwrap_or_default());
        Some(format!("{:016x}", fnv64(text.as_bytes())))
    }
}

pub fn fnv64(s: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

#[allow(clippy::too_many_arguments)]
fn finding(files: &mut Files, tool: &str, language: &str, file: &str, line1: u32, col1: u32, level: Level, code: &str, message: &str, enc: Encoding) -> Finding {
    let rel = files.rel(file);
    let col = files.col(&rel, line1, col1, enc);
    Finding {
        category: categorize(tool, code, level, message),
        code: code.into(),
        tool: tool.into(),
        provider: "linter".into(),
        level,
        language: language.into(),
        range: Range { line: line1, col, end_line: line1, end_col: col },
        message: message.into(),
        entity: None,
        doc_hash: files.hash(&rel),
        file: rel,
    }
}

fn level_word(s: &str) -> Level {
    match s.to_ascii_lowercase().as_str() {
        "error" | "fatal" => Level::Error,
        "warning" | "warn" => Level::Warning,
        "info" | "information" | "note" => Level::Info,
        _ => Level::Hint,
    }
}

/// Parse a linter's output. Unknown linters yield no findings (their run status still counts).
pub fn parse_linter(name: &str, language: &str, stdout: &str, root: &Path) -> Result<Vec<Finding>, String> {
    let mut files = Files::new(root);
    let mut out = vec![];
    match name {
        "clippy" => {
            for l in stdout.lines() {
                let Ok(v) = serde_json::from_str::<Value>(l) else { continue };
                if v.get("reason").and_then(Value::as_str) != Some("compiler-message") {
                    continue;
                }
                let m = &v["message"];
                let level = level_word(m["level"].as_str().unwrap_or("warning"));
                let code = m.pointer("/code/code").and_then(Value::as_str).unwrap_or("");
                let Some(span) = m["spans"].as_array().and_then(|s| s.iter().find(|x| x["is_primary"] == true)) else { continue };
                let tool = if code.starts_with("clippy::") { "clippy" } else { "rustc" };
                out.push(finding(&mut files, tool, language, span["file_name"].as_str().unwrap_or(""), span["line_start"].as_u64().unwrap_or(1) as u32, span["column_start"].as_u64().unwrap_or(1) as u32, level, code, m["message"].as_str().unwrap_or(""), Encoding::Utf32));
            }
        }
        "eslint" => {
            let v: Value = serde_json::from_str(stdout.trim()).map_err(|e| format!("eslint output is not JSON: {e}"))?;
            for f in v.as_array().into_iter().flatten() {
                let path = f["filePath"].as_str().unwrap_or("");
                for m in f["messages"].as_array().into_iter().flatten() {
                    let level = if m["severity"].as_u64() == Some(2) { Level::Error } else { Level::Warning };
                    let code = m["ruleId"].as_str().unwrap_or("");
                    out.push(finding(&mut files, "eslint", language, path, m["line"].as_u64().unwrap_or(1) as u32, m["column"].as_u64().unwrap_or(1) as u32, level, code, m["message"].as_str().unwrap_or(""), Encoding::Utf16));
                }
            }
        }
        "ruff" => {
            let v: Value = serde_json::from_str(stdout.trim()).map_err(|e| format!("ruff output is not JSON: {e}"))?;
            for m in v.as_array().into_iter().flatten() {
                let code = m["code"].as_str().unwrap_or("");
                let level = if code.starts_with("F8") || code.starts_with("E9") { Level::Error } else { Level::Warning };
                out.push(finding(&mut files, "ruff", language, m["filename"].as_str().unwrap_or(""), m.pointer("/location/row").and_then(Value::as_u64).unwrap_or(1) as u32, m.pointer("/location/column").and_then(Value::as_u64).unwrap_or(1) as u32, level, code, m["message"].as_str().unwrap_or(""), Encoding::Utf32));
            }
        }
        "clj-kondo" => {
            let v: Value = serde_json::from_str(stdout.trim()).map_err(|e| format!("clj-kondo output is not JSON: {e}"))?;
            for m in v["findings"].as_array().into_iter().flatten() {
                out.push(finding(&mut files, "clj-kondo", language, m["filename"].as_str().unwrap_or(""), m["row"].as_u64().unwrap_or(1) as u32, m["col"].as_u64().unwrap_or(1) as u32, level_word(m["level"].as_str().unwrap_or("warning")), m["type"].as_str().unwrap_or(""), m["message"].as_str().unwrap_or(""), Encoding::Utf32));
            }
        }
        // `file:line:col: level: message [check]` (clang-tidy, gcc-style tools)
        _ => {
            for l in stdout.lines() {
                let mut parts = l.splitn(5, ':');
                let (Some(file), Some(line_s), Some(col_s), Some(level_s), Some(rest)) = (parts.next(), parts.next(), parts.next(), parts.next(), parts.next()) else { continue };
                let (Ok(line1), Ok(col1)) = (line_s.trim().parse::<u32>(), col_s.trim().parse::<u32>()) else { continue };
                let level = level_word(level_s.trim());
                if level == Level::Hint || level_s.trim() == "note" {
                    continue;
                }
                let rest = rest.trim();
                let (msg, code) = match rest.rsplit_once(" [") {
                    Some((m, c)) if c.ends_with(']') => (m, c.trim_end_matches(']')),
                    _ => (rest, ""),
                };
                out.push(finding(&mut files, name, language, file, line1, col1, level, code, msg, Encoding::Utf8));
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use spide_core::evidence::Category;

    #[test]
    fn lsp_diagnostics_convert_utf16_columns() {
        let text = "let 🦀 = x;\n";
        let d = json!([{"range":{"start":{"line":0,"character":7},"end":{"line":0,"character":8}},"severity":1,"source":"rustc","code":"E0425","message":"cannot find value `x`"}]);
        let f = from_lsp(d.as_array().unwrap(), "a.rs", "rust", text, Encoding::Utf16, "rust-analyzer", None);
        assert_eq!(f[0].range.col, 9, "🦀 is 2 UTF-16 units but 4 bytes");
        assert_eq!((f[0].level, f[0].category, f[0].tool.as_str()), (Level::Error, Category::Correctness, "rustc"));
    }

    #[test]
    fn linters() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a.py"), "import os\né = 1\n").unwrap();
        let ruff = r#"[{"code":"F401","message":"`os` imported but unused","filename":"a.py","location":{"row":1,"column":8}}]"#;
        let f = parse_linter("ruff", "python", ruff, d.path()).unwrap();
        assert_eq!((f[0].category, f[0].range.col, f[0].file.as_str()), (Category::Maintainability, 7, "a.py"));
        let clippy = r#"{"reason":"compiler-message","message":{"level":"warning","message":"this function has too many arguments","code":{"code":"clippy::too_many_arguments"},"spans":[{"is_primary":true,"file_name":"a.py","line_start":2,"column_start":2}]}}"#;
        let f = parse_linter("clippy", "rust", clippy, d.path()).unwrap();
        assert_eq!((f[0].category, f[0].range.col), (Category::Complexity, 2), "column 2 = after the 2-byte é");
        let tidy = format!("{}/a.py:1:1: warning: use after move [bugprone-use-after-move]\nnote: whatever\n", d.path().display());
        let f = parse_linter("clang-tidy", "cpp", &tidy, d.path()).unwrap();
        assert_eq!((f.len(), f[0].file.as_str(), f[0].category), (1, "a.py", Category::Correctness));
        let kondo = r#"{"findings":[{"filename":"a.py","row":1,"col":1,"level":"error","type":"unresolved-symbol","message":"Unresolved symbol: x"}]}"#;
        assert_eq!(parse_linter("clj-kondo", "clojure", kondo, d.path()).unwrap()[0].category, Category::Correctness);
        let eslint = r#"[{"filePath":"a.py","messages":[{"ruleId":"@typescript-eslint/no-explicit-any","severity":1,"message":"Unexpected any","line":1,"column":1}]}]"#;
        assert_eq!(parse_linter("eslint", "typescript", eslint, d.path()).unwrap()[0].category, Category::Correctness);
        assert!(parse_linter("ruff", "python", "not json", d.path()).is_err());
    }
}
