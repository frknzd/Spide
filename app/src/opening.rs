//! A single path classifier for CLI, Finder and drops. All mutations stay in the guarded UI opener.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
#[derive(Debug, PartialEq)]
pub enum OpenTarget { Workspace(PathBuf), Import(PathBuf) }
#[derive(Serialize, Deserialize)]
pub struct ProjectShortcut { pub version: u32, pub project: PathBuf }

pub fn classify(path: &Path) -> Result<OpenTarget, String> {
    let path = path.canonicalize().map_err(|e| format!("Cannot open {}: {e}", path.display()))?;
    if path.is_dir() { return Ok(OpenTarget::Workspace(path)); }
    if path.extension().is_some_and(|s| s == "spideproject") {
        let t = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        let shortcut: ProjectShortcut = serde_json::from_str(&t).map_err(|e| format!("Invalid Spide project shortcut: {e}"))?;
        if shortcut.version != 1 { return Err("Unsupported Spide project shortcut version".into()); }
        let root = if shortcut.project.is_absolute() { shortcut.project } else { path.parent().ok_or("Shortcut has no parent")?.join(shortcut.project) };
        let root = root.canonicalize().map_err(|e| format!("Project shortcut target: {e}"))?;
        if !root.is_dir() { return Err("A project shortcut must point to a project folder".into()); }
        return Ok(OpenTarget::Workspace(root));
    }
    if path.file_name().is_some_and(|n| n == "project.json") && path.parent().is_some_and(|p| p.file_name().is_some_and(|n| n == "spide")) {
        return Ok(OpenTarget::Workspace(spide_core::project::resolve_root(&path)));
    }
    if path.file_name().is_some_and(|n| n == "design.json") {
        let dir = path.parent().ok_or("Design file has no folder")?;
        return Ok(OpenTarget::Workspace(spide_core::project::resolve_root(dir)));
    }
    if path.extension().is_some_and(|e| e == "json") { return Ok(OpenTarget::Import(path)); }
    Err("Open a Spide folder, project shortcut (.spideproject), manifest or design JSON.".into())
}

pub fn cli_path(args: impl IntoIterator<Item = std::ffi::OsString>) -> Result<Option<PathBuf>, String> {
    let mut args = args.into_iter().filter(|s| !s.to_string_lossy().starts_with("-psn_"));
    let Some(first) = args.next() else { return Ok(None) };
    let path = if first == "--project" { args.next().ok_or("--project needs a folder")? } else { first };
    if path.to_string_lossy().starts_with('-') || args.next().is_some() {
        return Err("Usage: spide [--project] <project-folder | design.json | file.spideproject> (one path at a time)".into());
    }
    Ok(Some(path.into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cli_and_paths_share_one_resolver_without_claiming_all_json() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("design.json"), "{}").unwrap();
        let root = dir.path().canonicalize().unwrap();
        assert_eq!(classify(&root.join("design.json")).unwrap(), OpenTarget::Workspace(root.clone()));
        let shortcut = root.join("demo.spideproject");
        std::fs::write(&shortcut, serde_json::to_string(&ProjectShortcut { version: 1, project: root.clone() }).unwrap()).unwrap();
        assert_eq!(classify(&shortcut).unwrap(), OpenTarget::Workspace(root.clone()));
        std::fs::write(root.join("copy.json"), "{}").unwrap();
        assert!(matches!(classify(&root.join("copy.json")).unwrap(), OpenTarget::Import(_)));
        assert_eq!(cli_path(["--project".into(), "hello world".into()]).unwrap(), Some("hello world".into()));
        assert!(cli_path(["--project".into()]).is_err());
        assert!(cli_path(["a".into(), "b".into()]).is_err());
    }
}
