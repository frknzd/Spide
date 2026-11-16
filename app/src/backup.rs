//! Design-state backup, not repository copying. Strict allowlist: current design (including plans),
//! history, active branch marker and inactive branch JSON. No sessions, tokens, endpoints, locks,
//! source files, editor state, project tooling environment or local UI drafts.
use crate::state::AppState;
use std::path::{Path, PathBuf};

fn regular_file(path: &Path) -> Result<bool, String> {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() => Err(format!("Refusing to copy symbolic link {}", path.display())),
        Ok(m) => Ok(m.is_file()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}
fn check_local_directory(path: &Path) -> Result<(), String> {
    if let Ok(m) = std::fs::symlink_metadata(path) {
        if m.file_type().is_symlink() { return Err(format!("Refusing to follow directory symlink {}", path.display())); }
    }
    Ok(())
}
fn collect(root: &Path) -> Result<Vec<PathBuf>, String> {
    let local = root.join(".spide");
    check_local_directory(&local)?;
    let mut out = vec![];
    for name in ["history.jsonl", "branch"] {
        let rel = PathBuf::from(".spide").join(name);
        if regular_file(&root.join(&rel))? { out.push(rel); }
    }
    let branches = local.join("branches");
    check_local_directory(&branches)?;
    if branches.is_dir() {
        for entry in std::fs::read_dir(&branches).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            if entry.path().extension().is_some_and(|e| e == "json") && regular_file(&entry.path())? {
                out.push(PathBuf::from(".spide/branches").join(entry.file_name()));
            }
        }
    }
    Ok(out)
}

pub fn design_backup(st: &AppState, destination: &Path) -> Result<(), String> {
    // Serialize with transitions so root/branch can't change during the backup.
    let _transition = st.transition.lock().unwrap_or_else(|e| e.into_inner());
    let (root, json, code_backed) = {
        let s = st.lock();
        if !matches!(s.workspace_status, crate::state::WorkspaceStatus::Ready | crate::state::WorkspaceStatus::ReadOnly) { return Err("No valid design is open to back up".into()); }
        (s.project_dir.clone(), s.store.design.to_json(), s.project.is_some())
    };
    if destination.try_exists().map_err(|e| e.to_string())? { return Err("Backup destination already exists. Choose a new folder; nothing is replaced.".into()); }
    let parent = destination.parent().ok_or("Choose a parent folder")?.canonicalize().map_err(|e| e.to_string())?;
    if parent.starts_with(&root) { return Err("Choose a backup destination outside the current project.".into()); }
    let files = collect(&root)?;
    // Stage outside the project. Only a complete backup is published.
    let stage = tempfile::Builder::new().prefix(".spide-backup-").tempdir_in(&parent).map_err(|e| e.to_string())?;
    crate::state::write_atomic(&stage.path().join("design.json"), &json).map_err(|e| e.to_string())?;
    for rel in files {
        let from = root.join(&rel);
        let to = stage.path().join(&rel);
        std::fs::create_dir_all(to.parent().unwrap()).map_err(|e| e.to_string())?;
        std::fs::copy(&from, &to).map_err(|e| format!("Cannot back up {}: {e}", from.display()))?;
    }
    let readme = format!("# Spide design-state backup\n\nCreated by Spide {}. Open this folder as a design-only project.\n\nIncludes design.json, history and branches only.{}\n\nNo source code, tooling configuration, credentials, sessions, endpoints, locks or local UI drafts were copied. The original project was not relocated or modified.\n", env!("CARGO_PKG_VERSION"), if code_backed { " This was a code-backed project: the repository was NOT copied; source indexing/sync must be configured again if needed." } else { "" });
    std::fs::write(stage.path().join("BACKUP.md"), readme).map_err(|e| e.to_string())?;
    // Reserve the destination without replacing any existing path. On Unix rename can replace an
    // empty directory, so reserve/create_new a marker and copy the staged allowlist into it.
    std::fs::create_dir(destination).map_err(|e| format!("Cannot reserve backup folder: {e}"))?;
    fn publish(from: &Path, to: &Path) -> Result<(), String> {
        for entry in std::fs::read_dir(from).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let target = to.join(entry.file_name());
            if entry.file_type().map_err(|e| e.to_string())?.is_dir() {
                std::fs::create_dir(&target).map_err(|e| e.to_string())?;
                publish(&entry.path(), &target)?;
            } else {
                use std::io::Write;
                let mut out = std::fs::OpenOptions::new().write(true).create_new(true).open(&target).map_err(|e| e.to_string())?;
                out.write_all(&std::fs::read(entry.path()).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
                out.sync_all().map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }
    publish(stage.path(), destination).map_err(|e| format!("Backup could not be published completely: {e}. Original unchanged; incomplete output is in {}.", destination.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn backup_is_openable_includes_history_branches_and_excludes_private_runtime_data() {
        let src = tempfile::tempdir().unwrap();
        let output = tempfile::tempdir().unwrap();
        let st = AppState::create(crate::events::Events::new().0, src.path().into());
        crate::commands::apply_user_ops(&st, serde_json::from_str(r#"[{"op":"add_class","name":"Saved"}]"#).unwrap()).unwrap();
        crate::commands::branch_create(&st, "experiment".into()).unwrap();
        for rel in [".pi/mcp.json", ".spide/sessions/secret.json", ".spide/endpoint.json", "src/private.rs"] {
            let path = src.path().join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "private-token").unwrap();
        }
        let dest = output.path().join("copy");
        design_backup(&st, &dest).unwrap();
        assert!(dest.join("design.json").is_file());
        assert!(dest.join(".spide/history.jsonl").is_file());
        assert!(dest.join(".spide/branches/main.json").is_file());
        assert!(!dest.join(".pi").exists());
        assert!(!dest.join(".spide/sessions").exists());
        assert!(!dest.join("src").exists());
        assert!(crate::workspace::prepare(&dest, crate::workspace::LoadIntent::Open).is_ok());
        assert!(design_backup(&st, &dest).is_err());
        assert!(design_backup(&st, &src.path().join("nested")).is_err());
        #[cfg(unix)] {
            std::os::unix::fs::symlink(src.path().join(".pi/mcp.json"), src.path().join(".spide/branches/secret.json")).unwrap();
            assert!(design_backup(&st, &output.path().join("unsafe-copy")).is_err());
        }
    }
}
