use super::app::SpideApp;
impl SpideApp {
    pub fn backup_dialog(&mut self, open_copy: bool) {
        let Some(s) = self.snap.clone() else { return };
        if !self.workspace_open() { return; }
        if open_copy && s.project.is_some() { return self.toast("info", "Code-backed projects support design-state backup, not repository duplication."); }
        let title = if open_copy { "Duplicate Design Project" } else { "Back Up Design State" };
        let accepted = rfd::MessageDialog::new().set_title(title)
            .set_description("Copies the current design, history and branches into a NEW folder. Source code, sessions, credentials, locks, endpoint files and local UI drafts are NOT included. The original stays where it is. Code-backed backups open as design-only projects.")
            .set_buttons(rfd::MessageButtons::OkCancel).show() == rfd::MessageDialogResult::Ok;
        if !accepted { return; }
        let Some(dest) = rfd::FileDialog::new().set_title("Name a NEW backup folder outside this project")
            .set_file_name(format!("{}-{}", s.view.name, if open_copy { "copy" } else { "backup" })).save_file() else { return };
        self.backend.call(move |st, _| { crate::backup::design_backup(st, &dest)?; Ok(dest) }, move |app, result| match result {
            Err(e) => app.toast("error", e),
            Ok(dest) => {
                app.toast("info", format!("Design-state backup written to {}", dest.display()));
                if open_copy { app.handle_open_paths(vec![dest]); }
            }
        });
    }
}
