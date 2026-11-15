//! Spide Project: a module folder where the design lives with its code.
//!
//! ```text
//! <root>/spide/project.json   manifest (committed)
//! <root>/spide/design.json    the design (committed)
//! <root>/spide/sync.json      sync baseline (committed)
//! <root>/.spide/              local: sessions, history, branches, skills
//! ```

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

pub const DIR: &str = "spide";
pub const MANIFEST: &str = "spide/project.json";
pub const DESIGN: &str = "spide/design.json";
pub const BASELINE: &str = "spide/sync.json";

/// What is compared beyond the defaults (fields and public/protected/package methods).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Compare {
    pub private_methods: bool,
    pub accessors: bool,
    pub constructors: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Command {
    pub run: String,
    /// Relative to the project root; may point up into the repository (e.g. `../..`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Manifest {
    pub version: u32,
    pub name: String,
    pub language: String,
    /// Production source roots, relative to the project root.
    pub roots: Vec<String>,
    pub exclude: Vec<String>,
    /// Java root package (design packages are relative to it).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code_root: Option<String>,
    pub compare: Compare,
    /// Suggested validation, run by the agent (`test`, `build`).
    pub commands: BTreeMap<String, Command>,
    /// Unknown fields survive a rewrite.
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

impl Default for Manifest {
    fn default() -> Self {
        Manifest {
            version: 1,
            name: String::new(),
            language: "java".into(),
            roots: vec!["src/main/java".into()],
            exclude: vec![],
            code_root: None,
            compare: Compare::default(),
            commands: BTreeMap::new(),
            extra: BTreeMap::new(),
        }
    }
}

/// A relative path without `..`, `.` only as a whole, nothing absolute.
pub fn is_plain_relative(p: &str) -> bool {
    let path = Path::new(p);
    !p.trim().is_empty() && !path.is_absolute() && path.components().all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}

pub fn is_java_package(s: &str) -> bool {
    !s.is_empty() && s.split('.').all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && !p.starts_with(|c: char| c.is_ascii_digit()))
}

/// `a/b/../c` → `a/c`, lexically (no filesystem access).
pub fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            c => out.push(c),
        }
    }
    out
}

impl Manifest {
    pub fn parse(text: &str) -> Result<Manifest, String> {
        serde_json::from_str(text).map_err(|e| format!("spide/project.json: {e}"))
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("manifest serialises") + "\n"
    }

    /// Problems with the manifest for a project at `root` (inside the git work tree `work_tree`, if any).
    pub fn validate(&self, root: &Path, work_tree: Option<&Path>) -> Vec<String> {
        let mut out = vec![];
        if self.language != "java" {
            out.push(format!("language {} is not supported (java only)", self.language));
        }
        if self.roots.is_empty() {
            out.push("at least one source root is needed".into());
        }
        let canon_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        for r in &self.roots {
            if !is_plain_relative(r) {
                out.push(format!("source root {r}: must be relative to the project, without `..`"));
                continue;
            }
            match root.join(r).canonicalize() {
                Ok(p) if p.starts_with(&canon_root) => {}
                Ok(_) => out.push(format!("source root {r} leaves the project (symlink)")),
                Err(_) => out.push(format!("source root {r} doesn't exist")),
            }
        }
        for e in &self.exclude {
            if !is_plain_relative(e) {
                out.push(format!("exclusion {e}: must be relative to the project, without `..`"));
            }
        }
        if let Some(c) = &self.code_root {
            if !is_java_package(c) {
                out.push(format!("code root {c} is not a Java package name"));
            }
        }
        let bound = normalize(&work_tree.map(Path::to_path_buf).unwrap_or_else(|| canon_root.clone()));
        for (name, cmd) in &self.commands {
            if cmd.run.trim().is_empty() {
                out.push(format!("command {name} is empty"));
            }
            if let Some(cwd) = &cmd.cwd {
                if Path::new(cwd).is_absolute() {
                    out.push(format!("command {name}: cwd must be relative"));
                } else if !normalize(&canon_root.join(cwd)).starts_with(&bound) {
                    out.push(format!("command {name}: cwd {cwd} leaves the {}", if work_tree.is_some() { "repository" } else { "project" }));
                }
            }
        }
        out
    }
}

/// The git work tree containing `dir` (walks up to a `.git` directory or file).
pub fn work_tree(dir: &Path) -> Option<PathBuf> {
    let mut d = dir.canonicalize().ok()?;
    loop {
        if d.join(".git").exists() {
            return Some(d);
        }
        if !d.pop() {
            return None;
        }
    }
}

/// The git directory of a work tree (`.git`, or the `gitdir:` a worktree's `.git` file points to).
pub fn git_dir(work_tree: &Path) -> Option<PathBuf> {
    let dot = work_tree.join(".git");
    if dot.is_dir() {
        return Some(dot);
    }
    let text = std::fs::read_to_string(&dot).ok()?;
    let p = text.lines().find_map(|l| l.strip_prefix("gitdir:"))?.trim();
    let p = PathBuf::from(p);
    Some(if p.is_absolute() { p } else { normalize(&work_tree.join(p)) })
}

/// Keep Spide's local files out of git without touching committed files: append them to
/// `info/exclude` (idempotent). Returns the lines added; nothing outside git.
pub fn exclude_local_files(root: &Path) -> Result<Vec<String>, String> {
    let Some(wt) = work_tree(root) else { return Ok(vec![]) };
    let Some(git) = git_dir(&wt) else { return Ok(vec![]) };
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let rel = root.strip_prefix(&wt).map_err(|e| e.to_string())?;
    let prefix = if rel.as_os_str().is_empty() { String::from("/") } else { format!("/{}/", rel.to_string_lossy().replace('\\', "/")) };
    let wanted = [format!("{prefix}.spide/"), format!("{prefix}.pi/mcp.json")];
    let path = git.join("info").join("exclude");
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let have: Vec<&str> = existing.lines().map(str::trim).collect();
    let add: Vec<String> = wanted.into_iter().filter(|w| !have.contains(&w.as_str())).collect();
    if add.is_empty() {
        return Ok(vec![]);
    }
    let mut text = existing.clone();
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    if !existing.contains("# Spide (local files)") {
        text.push_str("# Spide (local files)\n");
    }
    for a in &add {
        text.push_str(a);
        text.push('\n');
    }
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| e.to_string())?;
    Ok(add)
}

/// `dir` itself, or its parent when `dir` is the `spide/` folder or its manifest.
pub fn resolve_root(dir: &Path) -> PathBuf {
    if dir.file_name().is_some_and(|n| n == "project.json") {
        if let Some(p) = dir.parent().and_then(Path::parent) {
            return p.to_path_buf();
        }
    }
    if dir.file_name().is_some_and(|n| n == DIR) && dir.join("project.json").exists() {
        if let Some(p) = dir.parent() {
            return p.to_path_buf();
        }
    }
    dir.to_path_buf()
}

pub fn is_project(root: &Path) -> bool {
    root.join(MANIFEST).is_file()
}
