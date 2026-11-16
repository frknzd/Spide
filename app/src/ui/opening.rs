//! All external requests use the same guarded opener. Multiple simultaneous projects are rejected
//! rather than racing transitions in this single-workspace application.
use super::app::SpideApp;
use crate::opening::OpenTarget;
use std::path::PathBuf;
impl SpideApp {
    pub fn handle_open_paths(&mut self, paths: Vec<PathBuf>) {
        if paths.is_empty() { return; }
        if paths.len() != 1 { return self.toast("error", "Open one project at a time. No projects were switched."); }
        if self.open_request_pending { return self.toast("info", "A project open request is already in progress; try again after it finishes."); }
        match crate::opening::classify(&paths[0]) {
            Err(e) => { self.open_error = Some(e.clone()); self.toast("error", e); }
            Ok(OpenTarget::Import(file)) => self.import_file_dialog(file),
            Ok(OpenTarget::Workspace(root)) => {
                if !self.confirm_leave(super::leave::LeaveReason::SwitchProject) { return; }
                self.open_request_pending = true;
                self.backend.transition(move |s, pi| crate::commands::open_folder(s, pi, root.display().to_string()), |app, result| {
                    app.open_request_pending = false;
                    match result {
                        Ok(()) => app.after_transition(),
                        Err(e) => { app.open_error = Some(e.clone()); app.toast("error", e); }
                    }
                });
            }
        }
    }
    pub fn save_project_shortcut(&mut self) {
        let snapshot = self.backend.st.snapshot();
        if !self.workspace_open() { return; }
        let Some(mut file) = rfd::FileDialog::new().set_title("Save Finder project shortcut (references this project folder)")
            .set_file_name(format!("{}.spideproject", snapshot.view.name)).add_filter("Spide Project Shortcut", &["spideproject"]).save_file() else { return };
        file.set_extension("spideproject");
        let shortcut = crate::opening::ProjectShortcut { version: 1, project: PathBuf::from(&snapshot.project_dir) };
        match serde_json::to_string_pretty(&shortcut).map_err(|e| e.to_string()).and_then(|text| crate::state::write_atomic(&file, &text).map_err(|e| e.to_string())) {
            Ok(()) => self.toast("info", format!("Project shortcut saved to {}. The project stays where it is.", file.display())),
            Err(e) => self.toast("error", e),
        }
    }
}
#[cfg(test)]
mod tests {
    use crate::ui::tests::{project, harness, steps};
    #[test]
    fn external_open_and_drop_requests_use_the_guarded_loader() {
        let a = project("library");
        let b = project("rest-extraction");
        let mut h = harness(a.path(), (1440.0, 900.0));
        h.state_mut().prefs.agent_auto_start = false;
        h.state_mut().apply_preferences();
        h.state_mut().handle_open_paths(vec![a.path().into(), b.path().into()]);
        assert_eq!(h.state().backend.st.lock().project_dir, a.path().canonicalize().unwrap());
        h.state_mut().handle_open_paths(vec![b.path().join("design.json")]);
        steps(&mut h, 3);
        assert_eq!(h.state().backend.st.lock().project_dir, b.path().canonicalize().unwrap());
        let invalid = tempfile::tempdir().unwrap();
        std::fs::write(invalid.path().join("design.json"), "bad").unwrap();
        h.state_mut().handle_open_paths(vec![invalid.path().join("design.json")]);
        steps(&mut h, 3);
        assert_eq!(h.state().backend.st.lock().project_dir, b.path().canonicalize().unwrap());
        assert!(!h.state().open_request_pending);
    }
}
