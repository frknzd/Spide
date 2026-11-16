//! Local app preferences (never in the design): recent projects and startup behaviour.
//! Stored in `~/Library/Application Support/Spide/preferences.json` (or `SPIDE_PREFERENCES`).

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const VERSION: u32 = 1;
pub const MAX_RECENT: usize = 15;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Recent {
    /// Canonical project root.
    pub path: String,
    pub name: String,
    pub pinned: bool,
    pub opened_at: u64,
}

impl Default for Recent {
    fn default() -> Self {
        Self { path: String::new(), name: String::new(), pinned: false, opened_at: 0 }
    }
}

impl Recent {
    /// The folder still holds a Spide workspace (never created on demand).
    pub fn available(&self) -> bool {
        let p = Path::new(&self.path);
        p.join("design.json").is_file() || p.join(spide_core::project::MANIFEST).is_file()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub version: u32,
    pub recent: Vec<Recent>,
    /// Open the last project at startup instead of the welcome screen.
    pub reopen_last: bool,
    /// The project open when Spide last quit (None after Close Project: honour the manual close).
    pub last_project: Option<String>,
    pub new_project_location: String,
    /// Scales the complete egui interface, including custom canvas chrome.
    pub ui_scale: f32,
    pub terminal_font_size: f32,
    pub reduced_motion: bool,
    pub agent_auto_start: bool,
    /// Prefer embedded Neovim when opening source. Otherwise use external, with embedded fallback.
    pub embedded_editor: bool,
    pub favorite_commands: Vec<String>,
    pub workspaces: std::collections::BTreeMap<String, crate::ui::workspace_memory::WorkspaceView>,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            version: VERSION, recent: vec![], reopen_last: false, last_project: None,
            new_project_location: dirs::document_dir().unwrap_or_else(|| PathBuf::from(".")).join("Spide").display().to_string(),
            ui_scale: 1.0, terminal_font_size: 12.0, reduced_motion: false,
            agent_auto_start: true, embedded_editor: false, favorite_commands: vec![], workspaces: Default::default(),
        }
    }
}

pub fn default_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("SPIDE_PREFERENCES") {
        return Some(PathBuf::from(p));
    }
    dirs::data_dir().map(|d| d.join("Spide").join("preferences.json"))
}

impl Preferences {
    pub fn load(path: &Path) -> Preferences {
        let mut p = std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str::<Preferences>(&t).ok()).filter(|p| p.version <= VERSION).unwrap_or_default();
        p.sanitize();
        p
    }

    pub fn sanitize(&mut self) {
        self.ui_scale = if self.ui_scale.is_finite() { self.ui_scale.clamp(0.75, 1.75) } else { 1.0 };
        self.terminal_font_size = if self.terminal_font_size.is_finite() { self.terminal_font_size.clamp(9.0, 24.0) } else { 12.0 };
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        crate::state::write_atomic(path, &text).map_err(|e| format!("preferences {}: {e}", path.display()))
    }

    /// A project was opened successfully: move it to the top (canonical path, pin kept).
    pub fn opened(&mut self, root: &Path, name: &str, now: u64) {
        let path = root.canonicalize().unwrap_or_else(|_| root.to_path_buf()).display().to_string();
        let pinned = self.recent.iter().any(|r| r.path == path && r.pinned);
        self.recent.retain(|r| r.path != path);
        self.recent.insert(0, Recent { path: path.clone(), name: name.to_string(), pinned, opened_at: now });
        // drop the oldest unpinned beyond the cap
        while self.recent.len() > MAX_RECENT {
            match self.recent.iter().rposition(|r| !r.pinned) {
                Some(i) => {
                    self.recent.remove(i);
                }
                None => break,
            }
        }
        self.last_project = Some(path);
    }

    pub fn remove(&mut self, path: &str) {
        self.recent.retain(|r| r.path != path);
        if self.last_project.as_deref() == Some(path) {
            self.last_project = None;
        }
    }

    pub fn toggle_pin(&mut self, path: &str) {
        if let Some(r) = self.recent.iter_mut().find(|r| r.path == path) {
            r.pinned = !r.pinned;
        }
    }

    pub fn clear_unpinned(&mut self) {
        self.recent.retain(|r| r.pinned);
    }

    /// Pinned first, then most recently opened.
    pub fn sorted(&self) -> Vec<Recent> {
        let mut v = self.recent.clone();
        v.sort_by(|a, b| b.pinned.cmp(&a.pinned).then(b.opened_at.cmp(&a.opened_at)));
        v
    }

    /// The project to open at startup, if the user asked for it and it still exists.
    pub fn startup_project(&self) -> Option<PathBuf> {
        let p = self.last_project.as_ref().filter(|_| self.reopen_last)?;
        let r = Recent { path: p.clone(), ..Default::default() };
        r.available().then(|| PathBuf::from(p))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_settings_get_defaults_and_sizes_are_sanitized() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("prefs.json");
        std::fs::write(&file, r#"{"version":1,"reopen_last":true,"ui_scale":900,"terminal_font_size":-1}"#).unwrap();
        let p = Preferences::load(&file);
        assert!(p.agent_auto_start);
        assert_eq!(p.ui_scale, 1.75);
        assert_eq!(p.terminal_font_size, 9.0);
        assert!(p.reopen_last);
        p.save(&file).unwrap();
        assert_eq!(Preferences::load(&file), p);
    }

    #[test]
    fn recents_are_canonical_capped_pinned_and_never_recreated() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a");
        std::fs::create_dir(&a).unwrap();
        std::fs::write(a.join("design.json"), "{}").unwrap();
        let mut p = Preferences::default();
        p.opened(&a.join("."), "A", 1);
        assert_eq!(p.recent[0].path, a.canonicalize().unwrap().display().to_string());
        p.toggle_pin(&p.recent[0].path.clone());
        for i in 0..40 {
            p.opened(&dir.path().join(format!("p{i}")), "P", 10 + i);
        }
        assert_eq!(p.recent.len(), MAX_RECENT);
        assert!(p.recent.iter().any(|r| r.name == "A" && r.pinned), "pinned survives the cap");
        assert_eq!(p.sorted()[0].name, "A");
        assert!(!p.sorted()[1].available(), "missing folders are reported, not created");
        assert!(!dir.path().join("p39").exists());
        p.clear_unpinned();
        assert_eq!(p.recent.len(), 1);
        // startup: only when asked, and only if it still exists
        p.opened(&a, "A", 99);
        assert_eq!(p.startup_project(), None);
        p.reopen_last = true;
        assert_eq!(p.startup_project(), Some(a.canonicalize().unwrap()));
        p.remove(&a.canonicalize().unwrap().display().to_string());
        assert_eq!(p.startup_project(), None);
        // round trip
        let file = dir.path().join("prefs/preferences.json");
        p.save(&file).unwrap();
        assert_eq!(Preferences::load(&file), p);
    }
}
