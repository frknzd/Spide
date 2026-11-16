//! Offline documentation, build information and access to the local desktop activity log.
use super::app::SpideApp;

pub fn build_details() -> String {
    format!("{} · {} · {}\nBuilt {} · {} {}", env!("CARGO_PKG_VERSION"), env!("SPIDE_BUILD_REV"),
        if cfg!(debug_assertions) { "debug" } else { "release" },
        super::format_time(env!("SPIDE_BUILD_EPOCH").parse::<u64>().unwrap_or(0) * 1000), std::env::consts::OS, std::env::consts::ARCH)
}
impl SpideApp {
    pub fn open_logs(&mut self) {
        self.activity_open = true;
        if let Some(file) = self.activity.file.as_ref() {
            let path = file.parent().map(std::path::Path::to_path_buf);
            if let Some(p) = path.filter(|p| p.is_dir()) {
                if let Err(e) = std::process::Command::new("open").arg(&p).spawn() { self.toast("error", format!("Cannot open logs folder: {e}")); }
            }
        }
    }
    pub fn help_overlays(&mut self) {
        if self.documentation_open {
            let mut open = true;
            egui::Window::new("Spide Documentation").open(&mut open).default_width(740.0).default_height(540.0).show(&self.ctx, |ui| {
                ui.label("Bundled documentation — available offline. Select text to copy it.");
                ui.horizontal(|ui| {
                    ui.label("Find section:");
                    ui.add(egui::TextEdit::singleline(&mut self.documentation_query).hint_text("e.g. Neovim, branches, exports"));
                });
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let text = include_str!("../../../README.md");
                    let q = self.documentation_query.to_lowercase();
                    for section in text.split("\n## ") {
                        if !q.is_empty() && !section.to_lowercase().contains(&q) { continue; }
                        ui.add(egui::Label::new(section).selectable(true).wrap());
                        ui.separator();
                    }
                });
            });
            self.documentation_open = open && !self.ctx.input(|i| i.key_pressed(egui::Key::Escape));
        }
        if self.about_open {
            let mut open = true;
            let mut docs = false;
            let mut diagnostics = false;
            egui::Window::new("About Spide").open(&mut open).collapsible(false).resizable(false).show(&self.ctx, |ui| {
                ui.heading("Spide");
                ui.label("Design software systems live, together with an AI agent.");
                ui.label(build_details());
                ui.label("Native Rust / egui · pi terminal · Neovim");
                ui.horizontal(|ui| {
                    docs = ui.button("Documentation").clicked();
                    diagnostics = ui.button("Copy Diagnostics").clicked();
                });
            });
            self.about_open = open && !self.ctx.input(|i| i.key_pressed(egui::Key::Escape));
            if docs { self.documentation_open = true; }
            if diagnostics { self.copy_diagnostics(); }
        }
    }
}
#[cfg(test)]
mod tests {
    use crate::ui::{actions::ActionId, tests::{project, harness, steps}};
    use std::sync::Arc;
    #[test]
    fn diagnostics_are_sanitized_and_help_works_without_external_documents() {
        let dir = project("library");
        let mut h = harness(dir.path(), (1440.0, 900.0));
        let st = h.state().backend.st.clone();
        st.lock().pi_error = Some("secret-token private-prompt /sensitive/path".into());
        st.lock().store.design.name = "private design contents".into();
        h.state_mut().snap = Some(Arc::new(st.snapshot()));
        let text = h.state().diagnostics_text();
        for forbidden in ["secret-token", "private-prompt", "/sensitive/path", "private design contents", &dir.path().display().to_string()] {
            assert!(!text.contains(forbidden), "leaked {forbidden}");
        }
        assert!(text.contains(env!("SPIDE_BUILD_REV")));
        h.state_mut().perform(ActionId::Documentation);
        steps(&mut h, 3);
        assert!(h.state().documentation_open);
        h.state_mut().perform(ActionId::About);
        steps(&mut h, 3);
        assert!(h.state().about_open);
    }
}
