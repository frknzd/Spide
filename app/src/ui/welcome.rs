//! The welcome screen (no project open) and the New Project dialog.

use super::theme::{bold, css, mono, sans};
use super::widgets::{self, button, link, primary};
use crate::preferences::Preferences;
use egui::{Margin, RichText, Ui, Vec2};

pub enum WelcomeOut {
    New,
    Open,
    Import,
    OpenRecent(String),
    Locate(String),
    Remove(String),
    TogglePin(String),
    ClearUnpinned,
    ReopenLast(bool),
}

/// `error`: why the requested project couldn't be opened (startup or a failed open).
pub fn show(ui: &mut Ui, prefs: &Preferences, error: Option<&str>) -> Vec<WelcomeOut> {
    let mut out = vec![];
    let w = ui.available_width().min(600.0);
    ui.vertical_centered(|ui| {
        ui.add_space((ui.available_height() * 0.12).clamp(16.0, 90.0));
        ui.set_max_width(w);
        egui::ScrollArea::vertical().id_salt("welcome").auto_shrink([false, true]).show(ui, |ui| {
            widgets::panel_frame().inner_margin(Margin::same(26)).show(ui, |ui| {
                ui.set_width(w - 52.0);
                ui.spacing_mut().item_spacing.y = 10.0;
                ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                    ui.label(RichText::new("Spide").font(bold(24.0)).color(css::TEXT));
                    widgets::para(ui, "Design software systems live, together with pi. Start a new project or pick up where you left off.", 13.0, css::DIM);
                    if let Some(e) = error {
                        widgets::para(ui, e, 12.5, css::BAD);
                    }
                    ui.horizontal_wrapped(|ui| {
                        if primary(ui, "New Project…").on_hover_text("⌘N").clicked() {
                            out.push(WelcomeOut::New);
                        }
                        if button(ui, "Open Project…").on_hover_text("⌘O · a folder with design.json or spide/project.json").clicked() {
                            out.push(WelcomeOut::Open);
                        }
                        if button(ui, "Import Design JSON…").on_hover_text("Copy a design file into a new project folder").clicked() {
                            out.push(WelcomeOut::Import);
                        }
                    });
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Recent Projects").font(bold(14.0)).color(css::TEXT));
                        if prefs.recent.iter().any(|r| !r.pinned) {
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if link(ui, "Clear unpinned").clicked() {
                                    out.push(WelcomeOut::ClearUnpinned);
                                }
                            });
                        }
                    });
                    let recent = prefs.sorted();
                    if recent.is_empty() {
                        widgets::para(ui, "Projects you open appear here.", 12.0, css::FAINT);
                    }
                    for r in recent {
                        recent_row(ui, &r, &mut out);
                    }
                    ui.add_space(4.0);
                    let mut reopen = prefs.reopen_last;
                    if ui.checkbox(&mut reopen, RichText::new("Reopen the last project at startup").font(sans(12.0)).color(css::DIM)).changed() {
                        out.push(WelcomeOut::ReopenLast(reopen));
                    }
                });
            });
        });
    });
    out
}

fn recent_row(ui: &mut Ui, r: &crate::preferences::Recent, out: &mut Vec<WelcomeOut>) {
    let ok = r.available();
    egui::Frame::new().fill(css::RAISED).stroke(egui::Stroke::new(1.0, css::LINE)).corner_radius(8).inner_margin(Margin::symmetric(10, 6)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                let title = format!("{}{}", if r.pinned { "📌 " } else { "" }, if r.name.is_empty() { "Untitled" } else { &r.name });
                let resp = ui.add(egui::Label::new(RichText::new(title).font(bold(13.0)).color(if ok { css::TEXT } else { css::FAINT })).sense(egui::Sense::click()).truncate());
                if ok && resp.clicked() {
                    out.push(WelcomeOut::OpenRecent(r.path.clone()));
                }
                let path = if ok { r.path.clone() } else { format!("missing or moved · {}", r.path) };
                ui.add(egui::Label::new(RichText::new(path).font(mono(10.5)).color(if ok { css::FAINT } else { css::WARN })).truncate()).on_hover_text(&r.path);
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                if button(ui, "Remove").clicked() {
                    out.push(WelcomeOut::Remove(r.path.clone()));
                }
                if button(ui, if r.pinned { "Unpin" } else { "Pin" }).clicked() {
                    out.push(WelcomeOut::TogglePin(r.path.clone()));
                }
                if ok {
                    if primary(ui, "Open").clicked() {
                        out.push(WelcomeOut::OpenRecent(r.path.clone()));
                    }
                } else if button(ui, "Locate…").on_hover_text("Find the moved project folder").clicked() {
                    out.push(WelcomeOut::Locate(r.path.clone()));
                }
            });
        });
    });
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewKind {
    /// A blank design workspace in a new folder.
    Design,
    /// A Spide Project next to existing code (manifest setup).
    Code,
}

#[derive(Debug, Clone)]
pub struct NewProject {
    pub name: String,
    pub location: String,
    pub kind: NewKind,
    pub template: crate::templates::Template,
    pub error: String,
}

impl NewProject {
    pub fn new() -> Self {
        let location = dirs::document_dir().unwrap_or_else(|| std::path::PathBuf::from(".")).join("Spide").display().to_string();
        Self { name: "Untitled".into(), location, kind: NewKind::Design, template: Default::default(), error: String::new() }
    }
    /// The folder a blank design workspace will be created in.
    pub fn target(&self) -> std::path::PathBuf {
        std::path::Path::new(&self.location).join(self.name.trim())
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.kind == NewKind::Code {
            return Ok(());
        }
        let n = self.name.trim();
        if n.is_empty() {
            return Err("Enter a project name.".into());
        }
        if n.contains('/') || n == "." || n == ".." {
            return Err("The name is used as the folder name: no slashes.".into());
        }
        if self.location.trim().is_empty() {
            return Err("Choose a location.".into());
        }
        crate::commands::ensure_empty(&self.target())
    }
}

pub enum NewOut {
    Create,
    Cancel,
}

pub fn new_project_dialog(ctx: &egui::Context, f: &mut NewProject) -> Option<NewOut> {
    let mut out = None;
    egui::Window::new("New Project").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO).default_width(460.0).show(ctx, |ui| {
        ui.spacing_mut().item_spacing.y = 8.0;
        ui.radio_value(&mut f.kind, NewKind::Design, RichText::new("Design workspace").font(bold(13.0)));
        widgets::para(ui, "A blank design in its own new folder.", 12.0, css::DIM);
        ui.radio_value(&mut f.kind, NewKind::Code, RichText::new("Spide Project in a code folder").font(bold(13.0)));
        widgets::para(ui, "Keep the design next to existing code (spide/ in the module). You pick the code folder and review its settings next.", 12.0, css::DIM);
        if f.kind == NewKind::Design {
            egui::ComboBox::from_id_salt("new-project-template").selected_text(match f.template { crate::templates::Template::Blank => "Blank design", crate::templates::Template::Layered => "Layered starter example" }).show_ui(ui, |ui| {
                ui.selectable_value(&mut f.template, crate::templates::Template::Blank, "Blank design");
                ui.selectable_value(&mut f.template, crate::templates::Template::Layered, "Layered starter example");
            });
            if f.template == crate::templates::Template::Layered { ui.label("Three packages, three classes and two dependencies. Design only: no code is created."); }
            ui.label(RichText::new("Name").font(sans(12.0)).color(css::DIM));
            let r = ui.add(egui::TextEdit::singleline(&mut f.name).desired_width(f32::INFINITY));
            if r.changed() {
                f.error.clear();
            }
            ui.label(RichText::new("Location").font(sans(12.0)).color(css::DIM));
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut f.location).desired_width(ui.available_width() - 90.0));
                if button(ui, "Choose…").clicked() {
                    if let Some(d) = rfd::FileDialog::new().set_title("Where to create the project folder").set_directory(&f.location).pick_folder() {
                        f.location = d.display().to_string();
                        f.error.clear();
                    }
                }
            });
            widgets::para(ui, format!("Creates {}", f.target().display()), 11.5, css::FAINT);
        }
        if !f.error.is_empty() {
            widgets::para(ui, &f.error, 12.0, css::BAD);
        }
        ui.horizontal(|ui| {
            if primary(ui, if f.kind == NewKind::Design { "Create" } else { "Choose Code Folder…" }).clicked() || (f.kind == NewKind::Design && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                match f.validate() {
                    Ok(()) => out = Some(NewOut::Create),
                    Err(e) => f.error = e,
                }
            }
            if button(ui, "Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                out = Some(NewOut::Cancel);
            }
        });
    });
    out
}
