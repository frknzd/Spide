//! Validating workspace preparation shared by startup and project switches.
//! Never interpret a failed read as permission to create a blank design.
use crate::project::{self, ProjectState};
use spide_core::{model::SCHEMA_VERSION, Design};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadIntent {
    Open,
    New,
}

pub struct PreparedWorkspace {
    pub root: PathBuf,
    pub project: Option<ProjectState>,
    pub design: Design,
    pub last_written: Option<String>,
}

pub fn parse_design(text: &str, path: &Path) -> Result<Design, String> {
    let design = Design::from_json(text).map_err(|e| format!("Could not open {}: {e}", path.display()))?;
    if design.version == 0 || design.version > SCHEMA_VERSION {
        return Err(format!("Unsupported design version {} in {} (supported: 1–{SCHEMA_VERSION})", design.version, path.display()));
    }
    Ok(design)
}

/// Resolve aliases without silently creating an Open destination. New may create
/// a folder only after configuration and destination permissions are checked.
pub fn prepare(path: &Path, intent: LoadIntent) -> Result<PreparedWorkspace, String> {
    let resolved = spide_core::project::resolve_root(path);
    let root = match resolved.canonicalize() {
        Ok(p) => p,
        Err(e) if intent == LoadIntent::New && e.kind() == std::io::ErrorKind::NotFound => {
            let absolute = if resolved.is_absolute() { resolved } else {
                std::env::current_dir().map_err(|e| e.to_string())?.join(resolved)
            };
            spide_core::project::normalize(&absolute)
        }
        Err(e) => return Err(format!("Could not open {}: {e}", resolved.display())),
    };
    if root.try_exists().map_err(|e| e.to_string())? && !root.is_dir() {
        return Err(format!("Not a project folder: {}", root.display()));
    }
    let manifest = root.join(spide_core::project::MANIFEST);
    let design_only = root.join("design.json");
    let code_design = root.join(spide_core::project::DESIGN);
    if intent == LoadIntent::New {
        for p in [&manifest, &design_only, &code_design] {
            if p.try_exists().map_err(|e| format!("{}: {e}", p.display()))? {
                return Err(format!("{} already contains Spide data. Open it instead or choose a new folder.", root.display()));
            }
        }
        check_destination(&root)?;
        std::fs::create_dir_all(&root).map_err(|e| format!("Cannot create {}: {e}", root.display()))?;
        let root = root.canonicalize().map_err(|e| e.to_string())?;
        return Ok(PreparedWorkspace {
            design: Design { name: root.file_name().and_then(|s| s.to_str()).unwrap_or("Untitled").into(), ..Default::default() },
            root, project: None, last_written: None,
        });
    }
    if !manifest.try_exists().map_err(|e| e.to_string())? && !design_only.try_exists().map_err(|e| e.to_string())? {
        return Err(format!("{} is not a Spide project (no design.json or spide/project.json). Use New Project to create one.", root.display()));
    }
    let project = project::load(&root)?;
    let path = if project.is_some() { code_design } else { design_only };
    let text = std::fs::read_to_string(&path).map_err(|e| format!("Could not read {}: {e}. Choose another project or recover a copy; the original was not changed.", path.display()))?;
    let design = parse_design(&text, &path)?;
    check_destination(path.parent().ok_or("Invalid design path")?)?;
    Ok(PreparedWorkspace { root, project, design, last_written: Some(text) })
}

/// Check directory write/rename permission with an unrelated unique temporary
/// file. Do not touch design.json, its .tmp sibling, or any project data.
fn check_destination(dir: &Path) -> Result<(), String> {
    let mut existing = dir;
    while !existing.try_exists().map_err(|e| format!("{}: {e}", existing.display()))? {
        existing = existing.parent().ok_or("No existing parent for new project")?;
    }
    if !existing.is_dir() || std::fs::metadata(existing).map_err(|e| e.to_string())?.permissions().readonly() {
        return Err(format!("Project destination is not writable: {}", existing.display()));
    }
    tempfile::Builder::new().prefix(".spide-write-check-").tempfile_in(existing)
        .map_err(|e| format!("Project destination is not writable — {}: {e}", existing.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_open_never_creates_a_folder_or_blank_design() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing");
        assert!(prepare(&missing, LoadIntent::Open).is_err());
        assert!(!missing.exists());
        assert!(prepare(dir.path(), LoadIntent::Open).is_err());
        assert!(!dir.path().join("design.json").exists());
        assert!(prepare(&missing, LoadIntent::New).is_ok());
    }

    #[test]
    fn invalid_and_unsupported_designs_are_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("design.json");
        for text in ["corrupt", r#"{"version":999,"name":"Future"}"#] {
            std::fs::write(&path, text).unwrap();
            assert!(prepare(dir.path(), LoadIntent::Open).is_err());
            assert!(prepare(dir.path(), LoadIntent::New).is_err());
            assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        }
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert!(prepare(dir.path(), LoadIntent::Open).is_err());
    }

    #[test]
    fn invalid_manifest_is_not_replaced_with_defaults() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("spide")).unwrap();
        let manifest = dir.path().join(spide_core::project::MANIFEST);
        for text in ["corrupt", r#"{"version":9}"#, r#"{"roots":["../outside"]}"#] {
            std::fs::write(&manifest, text).unwrap();
            assert!(prepare(dir.path(), LoadIntent::Open).is_err());
            assert_eq!(std::fs::read_to_string(&manifest).unwrap(), text);
        }
    }

    #[test]
    fn legacy_designs_and_canonical_aliases_open() {
        let dir = tempfile::tempdir().unwrap();
        let text = r#"{"version":1,"name":"Legacy"}"#;
        std::fs::write(dir.path().join("design.json"), text).unwrap();
        let p = prepare(&dir.path().join("."), LoadIntent::Open).unwrap();
        assert_eq!(p.root, dir.path().canonicalize().unwrap());
        assert_eq!(p.design.name, "Legacy");
        assert_eq!(p.last_written.as_deref(), Some(text));
        #[cfg(unix)] {
            let alias = dir.path().join("alias");
            std::os::unix::fs::symlink(dir.path(), &alias).unwrap();
            assert_eq!(prepare(&alias, LoadIntent::Open).unwrap().root, p.root);
        }
    }

    #[test]
    fn permission_failure_does_not_change_design_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("design.json");
        let text = Design::default().to_json();
        std::fs::write(&path, &text).unwrap();
        let original = std::fs::metadata(dir.path()).unwrap().permissions();
        let mut readonly = original.clone();
        readonly.set_readonly(true);
        std::fs::set_permissions(dir.path(), readonly).unwrap();
        let result = prepare(dir.path(), LoadIntent::Open);
        std::fs::set_permissions(dir.path(), original).unwrap();
        assert!(result.is_err());
        assert_eq!(std::fs::read_to_string(path).unwrap(), text);
    }
}
