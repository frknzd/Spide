//! Design branches: `design.json` is the active branch; others live in `.spide/branches/<name>.json`.

use spide_core::Design;
use std::path::{Path, PathBuf};

pub fn dir(project: &Path) -> PathBuf {
    project.join(".spide").join("branches")
}

/// Branch names are file names: letters, digits, `-`, `_`, `.` (no leading dot).
pub fn valid_name(name: &str) -> Result<(), String> {
    let ok = !name.is_empty()
        && name.len() <= 60
        && !name.starts_with('.')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if ok {
        Ok(())
    } else {
        Err(format!("invalid branch name {name:?} (use letters, digits, - _ .)"))
    }
}

pub fn path(project: &Path, name: &str) -> Result<PathBuf, String> {
    valid_name(name)?;
    Ok(dir(project).join(format!("{name}.json")))
}

pub fn write(project: &Path, name: &str, d: &Design) -> Result<(), String> {
    let p = path(project, name)?;
    crate::workspace::write_atomic(&p, &d.to_json()).map_err(|e| e.to_string())
}

pub fn read(project: &Path, name: &str) -> Result<Design, String> {
    let p = path(project, name)?;
    let text = std::fs::read_to_string(&p).map_err(|_| format!("no branch {name}"))?;
    Design::from_json(&text).map_err(|e| format!("branch {name} is invalid: {e}"))
}

pub fn exists(project: &Path, name: &str) -> bool {
    path(project, name).is_ok_and(|p| p.exists())
}

/// Remember which branch design.json holds.
pub fn active(project: &Path) -> String {
    std::fs::read_to_string(project.join(".spide").join("branch"))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| valid_name(s).is_ok())
        .unwrap_or_else(|| "main".into())
}

pub fn set_active(project: &Path, name: &str) -> std::io::Result<()> {
    crate::workspace::write_atomic(&project.join(".spide").join("branch"), name)
}

pub fn list(project: &Path) -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(dir(project))
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter_map(|e| e.file_name().to_str().and_then(|n| n.strip_suffix(".json")).map(String::from))
                .filter(|n| valid_name(n).is_ok())
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

pub fn delete(project: &Path, name: &str) -> Result<(), String> {
    std::fs::remove_file(path(project, name)?).map_err(|_| format!("no branch {name}"))
}
