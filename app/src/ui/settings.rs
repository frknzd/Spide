//! Local app settings. pi owns provider/model configuration and secrets.
use super::{app::SpideApp, theme::{css, sans}, widgets};

impl SpideApp {
    pub fn apply_preferences(&mut self) {
        self.prefs.sanitize();
        self.backend.st.lock().preferences = self.prefs.clone();
        self.ctx.set_zoom_factor(self.prefs.ui_scale);
        self.ctx.data_mut(|d| {
            d.insert_temp(egui::Id::unique("terminal-font-size"), self.prefs.terminal_font_size);
            d.insert_temp(egui::Id::unique("reduced-motion"), self.prefs.reduced_motion);
        });
        self.ctx.global_style_mut(|s| s.animation_time = if self.prefs.reduced_motion { 0.0 } else { 0.15 });
    }

    pub fn settings_overlay(&mut self) {
        if !self.settings_open { return; }
        let mut open = true;
        let before = self.prefs.clone();
        let mut reset_layout = false;
        let mut focus_pi = false;
        egui::Window::new("Settings").open(&mut open).collapsible(false).default_width(480.0)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO).show(&self.ctx, |ui| {
            ui.spacing_mut().item_spacing.y = 9.0;
            egui::ScrollArea::vertical().max_height(450.0).show(ui, |ui| {
                ui.heading("Projects");
                ui.checkbox(&mut self.prefs.reopen_last, "Reopen the last project at startup");
                ui.label("Default location for new projects");
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.prefs.new_project_location).desired_width(320.0));
                    if ui.button("Choose…").clicked() {
                        if let Some(p) = rfd::FileDialog::new().pick_folder() { self.prefs.new_project_location = p.display().to_string(); }
                    }
                });
                ui.separator();
                ui.heading("Appearance");
                ui.add(egui::Slider::new(&mut self.prefs.ui_scale, 0.75..=1.75).step_by(0.05).text("Interface scale"));
                ui.add(egui::Slider::new(&mut self.prefs.terminal_font_size, 9.0..=24.0).step_by(1.0).text("Terminal text size"));
                ui.checkbox(&mut self.prefs.reduced_motion, "Reduce motion").on_hover_text("Instant pan/zoom and no entrance animations. Automatic layout still settles into place.");
                reset_layout = ui.button("Reset Workspace Layout").clicked();
                ui.separator();
                ui.heading("Agent and editor");
                ui.checkbox(&mut self.prefs.agent_auto_start, "Start pi automatically when opening a project");
                widgets::para(ui, "Changes apply to the next project open. Restart remains available at any time.", 11.5, css::DIM);
                ui.checkbox(&mut self.prefs.embedded_editor, "Prefer embedded Neovim when opening source");
                widgets::para(ui, "Otherwise Spide prefers an attached external Neovim and falls back to the embedded editor. Your Neovim configuration is unchanged.", 11.5, css::DIM);
                ui.separator();
                ui.label(egui::RichText::new("Models and providers belong to pi, not Spide.").font(sans(12.5)));
                widgets::para(ui, "Use /model and /settings in the pi terminal. Spide stores no provider keys or duplicate model settings.", 12.0, css::DIM);
                focus_pi = ui.button("Show pi terminal").clicked();
            });
        });
        if reset_layout { self.workspace = Default::default(); }
        if focus_pi {
            self.workspace.left_open = true;
            if let Some(t) = self.term.as_mut() { t.focus_requested = true; }
            open = false;
        }
        if before != self.prefs {
            self.apply_preferences();
            self.save_prefs();
        }
        self.settings_open = open && !self.ctx.input(|i| i.key_pressed(egui::Key::Escape));
    }
}

#[cfg(test)]
mod tests {
    use crate::ui::{actions::ActionId, tests::{project, harness, steps}};

    #[test]
    fn settings_apply_locally_and_new_project_uses_selected_location() {
        let dir = project("library");
        let mut h = harness(dir.path(), (1440.0, 900.0));
        h.state_mut().perform(ActionId::Settings);
        steps(&mut h, 3);
        assert!(h.state().settings_open);
        let prefs = &mut h.state_mut().prefs;
        prefs.agent_auto_start = false;
        prefs.reduced_motion = true;
        prefs.terminal_font_size = 17.0;
        prefs.new_project_location = "/tmp/designs".into();
        h.state_mut().apply_preferences();
        h.state_mut().new_project_dialog();
        assert_eq!(h.state().new_project.as_ref().unwrap().location, "/tmp/designs");
        let st = h.state().backend.st.clone();
        assert!(!st.lock().preferences.agent_auto_start);
        assert!(st.lock().preferences.reduced_motion);
        assert_eq!(h.state().ctx.data(|d| d.get_temp::<f32>(egui::Id::unique("terminal-font-size"))), Some(17.0));
        let target = tempfile::tempdir().unwrap();
        h.state_mut().new_project = None;
        h.state_mut().settings_open = false;
        crate::commands::new_project(&st, &h.state().backend.pi, target.path().join("Blank").display().to_string(), "Blank".into()).unwrap();
        assert_eq!(st.lock().pi_status, crate::state::PiStatus::Offline);
        assert!(st.lock().pi_error.is_none(), "auto-start disabled: no attempted pi spawn");
    }
}
