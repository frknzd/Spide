//! The branch chooser (Navigate › Branches…): list, switch, create, compare, merge, delete.
//! Everything goes through the existing branch commands; switching/creating is guarded like
//! leaving (plan drafts, failed saves, unsent text, a busy agent).

use super::app::SpideApp;
use super::leave::LeaveReason;
use super::theme::{bold, css, mono, sans};
use super::widgets::{self, button, primary};
use crate::commands::BranchInfo;
use egui::{RichText, Vec2};

#[derive(Default)]
pub struct BranchUi {
    pub list: Option<Vec<BranchInfo>>,
    pub new_name: String,
    pub error: String,
}

enum Out {
    Switch(String),
    Create(String),
    Compare(String),
    Merge(String),
    Delete(String),
    Close,
}

impl SpideApp {
    pub fn open_branches(&mut self) {
        self.branches = Some(BranchUi::default());
        self.refresh_branches();
    }

    fn refresh_branches(&mut self) {
        self.backend.call(
            |s, _| Ok(crate::commands::branch_list(s)),
            |app, r: Result<Vec<BranchInfo>, String>| {
                if let Some(b) = app.branches.as_mut() {
                    match r {
                        Ok(l) => b.list = Some(l),
                        Err(e) => b.error = e,
                    }
                }
            },
        );
    }

    pub fn branches_overlay(&mut self) {
        let Some(mut b) = self.branches.take() else { return };
        let mut out = None;
        let mut open = true;
        egui::Window::new("Branches").open(&mut open).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_TOP, Vec2::new(0.0, 80.0)).default_width(480.0).show(&self.ctx, |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            widgets::para(ui, "Each branch is a separate version of the design. Switching keeps the current branch as it is.", 12.0, css::DIM);
            match &b.list {
                None => {
                    ui.spinner();
                }
                Some(list) => {
                    for br in list {
                        egui::Frame::new().fill(css::RAISED).stroke(egui::Stroke::new(1.0, if br.active { css::ACCENT } else { css::LINE })).corner_radius(7).inner_margin(egui::Margin::symmetric(10, 6)).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(format!("{}{}", if br.active { "● " } else { "" }, br.name)).font(bold(13.0)).color(css::TEXT));
                                ui.label(RichText::new(format!("{} classes", br.classes)).font(mono(10.5)).color(css::FAINT));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.spacing_mut().item_spacing.x = 4.0;
                                    if br.active {
                                        ui.label(RichText::new("current").font(sans(11.5)).color(css::ACCENT));
                                        return;
                                    }
                                    if button(ui, "Delete").clicked() {
                                        out = Some(Out::Delete(br.name.clone()));
                                    }
                                    if button(ui, "Merge").on_hover_text("Propose this branch's changes as ghosts on the current one").clicked() {
                                        out = Some(Out::Merge(br.name.clone()));
                                    }
                                    if button(ui, "Compare").clicked() {
                                        out = Some(Out::Compare(br.name.clone()));
                                    }
                                    if primary(ui, "Switch").clicked() {
                                        out = Some(Out::Switch(br.name.clone()));
                                    }
                                });
                            });
                        });
                    }
                }
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let r = ui.add(egui::TextEdit::singleline(&mut b.new_name).hint_text("new branch name").desired_width(220.0));
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if (button(ui, "Create from current").clicked() || enter) && !b.new_name.trim().is_empty() {
                    out = Some(Out::Create(b.new_name.trim().to_string()));
                }
            });
            if !b.error.is_empty() {
                widgets::para(ui, &b.error, 12.0, css::BAD);
            }
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                out = Some(Out::Close);
            }
        });
        if !open {
            out = Some(Out::Close);
        }
        match out {
            None => self.branches = Some(b),
            Some(Out::Close) => {}
            Some(Out::Switch(name)) => {
                if !self.confirm_leave(LeaveReason::SwitchBranch) {
                    self.branches = Some(b);
                    return;
                }
                self.backend.call(move |s, _| crate::commands::branch_switch(s, name), |app, r| match r {
                    Ok(()) => app.reset_branch_ui(),
                    Err(e) => app.toast("error", e),
                });
            }
            Some(Out::Create(name)) => {
                if let Err(e) = crate::branches::valid_name(&name) {
                    b.error = e;
                    self.branches = Some(b);
                    return;
                }
                if !self.confirm_leave(LeaveReason::SwitchBranch) {
                    self.branches = Some(b);
                    return;
                }
                self.backend.call(move |s, _| crate::commands::branch_create(s, name), |app, r| match r {
                    Ok(()) => app.reset_branch_ui(),
                    Err(e) => app.toast("error", e),
                });
            }
            Some(Out::Compare(name)) => self.command(&format!("/compare {name}")),
            Some(Out::Merge(name)) => self.command(&format!("/merge {name}")),
            Some(Out::Delete(name)) => {
                // the existing confirmation ("cannot be undone") stays
                self.command(&format!("/branch delete {name}"));
                self.branches = Some(b);
                self.refresh_branches();
            }
        }
    }

    /// Another branch's design is loaded: transient review/preview state belongs to the old one.
    fn reset_branch_ui(&mut self) {
        let (find, nav) = (self.find.take(), std::mem::take(&mut self.nav));
        self.reset_workspace_ui();
        self.restore_input_drafts();
        self.find = find;
        self.nav = nav;
    }
}
