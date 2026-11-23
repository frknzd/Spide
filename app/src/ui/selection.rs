//! Selection and the element context menu (right-click), keyboard selection and Delete.
//! Removing elements prunes them: they stay in the model hidden, ⌘Z restores them, and code is
//! never touched.

use super::app::SpideApp;
use super::chrome::Tab;
use super::theme::{bold, css, sans};
use super::widgets;
use egui::{Pos2, RichText};
use serde_json::json;
use spide_core::model::Status;
use spide_core::Target;

pub struct ContextMenu {
    pub targets: Vec<Target>,
    pub at: Pos2,
}

enum Item {
    Inspect,
    PackageScope(String),
    Rename,
    Properties,
    Reveal,
    TogglePin(String),
    MoveTo(Option<String>),
    CopyName,
    Copy,
    Cut,
    Duplicate,
    Paste,
    Remove,
    Restore,
}

impl SpideApp {
    /// May the design be edited from the canvas right now?
    pub fn canvas_editable(&self) -> Result<(), &'static str> {
        let s = self.snap.as_ref().ok_or("No project is open")?;
        if s.workspace_status != crate::state::WorkspaceStatus::Ready {
            return Err("The design can't be edited (read-only or not open)");
        }
        if self.review_active {
            return Err("Leave the review first; canvas editing is paused while reviewing");
        }
        if self.preview.is_some() {
            return Err("Exit the preview first");
        }
        Ok(())
    }

    pub fn open_context_menu(&mut self, t: Target, at: Pos2) {
        if !self.selection.contains(&t) {
            self.selection = vec![t];
        }
        self.hover.clear();
        self.context = Some(ContextMenu { targets: self.selection.clone(), at });
    }

    pub fn select_targets(&mut self, ts: Vec<Target>, add: bool) {
        if !add {
            self.selection.clear();
        }
        for t in ts {
            if !self.selection.contains(&t) {
                self.selection.push(t);
            }
        }
    }

    fn is_pruned(&self, t: &Target) -> bool {
        self.snap.as_ref().is_some_and(|s| super::search::index(&s.view).iter().any(|i| i.target.as_ref() == Some(t) && i.pruned()))
    }

    fn pinned(&self, class: &str) -> bool {
        self.class_canvas.as_ref().is_some_and(|c| c.layout.positions().get(class).is_some_and(|p| p.pinned))
    }

    pub fn context_overlay(&mut self) {
        let Some(menu) = self.context.take() else { return };
        let Some(snap) = self.snap.clone() else { return };
        let editable = self.canvas_editable();
        let single = (menu.targets.len() == 1).then(|| menu.targets[0].clone());
        let classes: Vec<String> = menu.targets.iter().filter_map(|t| if let Target::Class { name } = t { Some(name.clone()) } else { None }).collect();
        let all_pruned = menu.targets.iter().all(|t| self.is_pruned(t));
        let mut chosen = None;
        let mut close = false;
        let r = egui::Area::new(egui::Id::unique("element-context-menu")).order(egui::Order::Foreground).fixed_pos(menu.at).constrain(true).show(&self.ctx, |ui| {
            widgets::panel_frame().inner_margin(egui::Margin::same(6)).show(ui, |ui| {
                ui.set_min_width(220.0);
                ui.spacing_mut().button_padding = egui::vec2(8.0, 4.0);
                let title = match &single {
                    Some(t) => t.label(),
                    None => format!("{} elements", menu.targets.len()),
                };
                ui.label(RichText::new(title).font(bold(12.0)).color(css::DIM));
                ui.separator();
                let one = |ok: bool, why: &'static str| if ok { Ok(()) } else { Err(why) };
                let renamable = matches!(single, Some(Target::Class { .. } | Target::Attribute { .. } | Target::Method { .. }));
                if let Some(Target::Package { name }) = &single {
                    if let Some(x) = item(ui, Ok(()), "Show Package + Boundary", "", Item::PackageScope(name.clone())) { chosen = Some(x); }
                }
                if let Some(x) = item(ui, one(single.is_some(), "Select one element"), "Inspect", "", Item::Inspect) {
                    chosen = Some(x);
                }
                if let Some(x) = item(ui, one(renamable, "Only classes and members can be renamed here").and(editable), "Rename…", "double-click", Item::Rename) {
                    chosen = Some(x);
                }
                if let Some(x) = item(ui, one(matches!(single, Some(Target::Class { .. })), "Select one class").and(editable), "Edit Properties…", "", Item::Properties) {
                    chosen = Some(x);
                }
                if let Some(x) = item(ui, one(matches!(single, Some(Target::Class { .. } | Target::Method { .. })), "Classes and methods only"), "Reveal in Editor", "⌘-click", Item::Reveal) {
                    chosen = Some(x);
                }
                if let (Some(Target::Class { name }), Tab::Class) = (&single, self.tab) {
                    let label = if self.pinned(name) { "Unpin" } else { "Pin in Place" };
                    if let Some(x) = item(ui, editable, label, "⌥-click", Item::TogglePin(name.clone())) {
                        chosen = Some(x);
                    }
                }
                if !classes.is_empty() && self.tab == Tab::Class {
                    let packages: Vec<String> = snap.view.packages.iter().filter(|p| p.status != Status::Pruned).map(|p| p.name.clone()).collect();
                    ui.add_enabled_ui(editable.is_ok(), |ui| {
                        ui.menu_button(RichText::new("Move to Package").font(sans(12.5)), |ui| {
                            for p in &packages {
                                if ui.button(p).clicked() {
                                    chosen = Some(Item::MoveTo(Some(p.clone())));
                                }
                            }
                            ui.separator();
                            if ui.button("No package").clicked() {
                                chosen = Some(Item::MoveTo(None));
                            }
                        });
                    });
                }
                ui.separator();
                if let Some(x) = item(ui, Ok(()), "Copy Name", "", Item::CopyName) {
                    chosen = Some(x);
                }
                if let Some(x) = item(ui, self.clipboard_source().map(|_| ()), "Copy", "⌘C", Item::Copy) {
                    chosen = Some(x);
                }
                if let Some(x) = item(ui, self.clipboard_source().map(|_| ()).and(editable), "Cut", "⌘X", Item::Cut) {
                    chosen = Some(x);
                }
                if let Some(x) = item(ui, self.clipboard_source().map(|_| ()).and(editable), "Duplicate", "⌘D", Item::Duplicate) {
                    chosen = Some(x);
                }
                if let Some(x) = item(ui, self.paste_state().and(editable), "Paste", "⌘V", Item::Paste) {
                    chosen = Some(x);
                }
                ui.separator();
                if all_pruned {
                    if let Some(x) = item(ui, editable, "Restore", "", Item::Restore) {
                        chosen = Some(x);
                    }
                } else {
                    if let Some(x) = item(ui, editable, "Remove from Design", "⌫", Item::Remove) {
                        chosen = Some(x);
                    }
                    ui.label(RichText::new("Hidden, not deleted: ⌘Z or click to restore. Code is not changed.").font(sans(10.5)).color(css::FAINT));
                }
            });
        });
        let (esc, clicked_elsewhere) = self.ctx.input(|i| (i.key_pressed(egui::Key::Escape), i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| !r.response.rect.contains(p))));
        if esc || clicked_elsewhere {
            close = true;
        }
        match chosen {
            Some(it) => self.context_item(it, &menu),
            None if !close => self.context = Some(menu),
            None => {}
        }
    }

    fn context_item(&mut self, it: Item, menu: &ContextMenu) {
        let single = (menu.targets.len() == 1).then(|| menu.targets[0].clone());
        match it {
            Item::Inspect => {
                if let Some(t) = single {
                    let key = match &t {
                        Target::Class { name } => format!("class:{name}"),
                        Target::Attribute { class, name } => format!("attribute:{class}.{name}"),
                        Target::Method { class, name } => format!("method:{class}.{name}"),
                        Target::Relation { id } => format!("relation:{id}"),
                        _ => return self.locate_target(&t),
                    };
                    self.hover.pin(&key);
                }
            }
            Item::PackageScope(name) => self.set_package_scope(Some(name)),
            Item::Rename => {
                if let Some(t) = single {
                    self.on_double_click(t, menu.at);
                }
            }
            Item::Properties => {
                if let Some(Target::Class { name }) = single {
                    self.open_class_editor(&name);
                }
            }
            Item::Reveal => self.perform(super::actions::ActionId::RevealInEditor),
            Item::TogglePin(name) => {
                if let Some(c) = self.class_canvas.as_mut() {
                    if c.layout.has(&name) {
                        c.layout.toggle_pin(&name);
                        let pos = c.layout.positions();
                        self.run_action(move |s, _| crate::commands::set_layout(s, "class".into(), pos));
                    }
                }
            }
            Item::MoveTo(pkg) => {
                let ops: Vec<spide_core::Op> = menu.targets.iter().filter_map(|t| if let Target::Class { name } = t { Some(super::brief::op(json!({"op": "move_to_package", "class": name, "package": pkg}))) } else { None }).collect();
                let n = ops.len();
                let msg = format!("Moved {n} class{} to {} (⌘Z to undo)", if n == 1 { "" } else { "es" }, pkg.as_deref().unwrap_or("no package"));
                self.apply_ops(ops, msg);
            }
            Item::CopyName => {
                let names: Vec<String> = menu.targets.iter().map(target_name).collect();
                self.ctx.copy_text(names.join("\n"));
            }
            Item::Copy => {
                self.design_copy();
            }
            Item::Cut => self.design_cut(),
            Item::Duplicate => self.duplicate(),
            Item::Paste => self.design_paste(),
            Item::Remove => self.delete_selection(),
            Item::Restore => {
                let ts = menu.targets.clone();
                self.run_action(move |s, _| ts.into_iter().try_for_each(|t| crate::commands::restore(s, t)));
            }
        }
    }

    /// One user gesture = one undo entry.
    pub fn apply_ops(&mut self, ops: Vec<spide_core::Op>, done: String) {
        if ops.is_empty() {
            return;
        }
        self.backend.run_then(move |s, _| crate::commands::apply_user_ops(s, ops), move |app, ()| app.toast("info", done));
    }

    /// Delete / Backspace / Remove from Design: prune the selection (reversible).
    pub fn delete_selection(&mut self) {
        if let Err(e) = self.canvas_editable() {
            return self.toast("info", e);
        }
        let ts: Vec<Target> = self.selection.iter().filter(|t| !self.is_pruned(t)).cloned().collect();
        if ts.is_empty() {
            return;
        }
        let at = self.canvas_rect.center();
        let n = ts.len();
        self.selection.clear();
        self.do_prune(ts, at);
        self.toast("info", format!("Removed {n} element{} from the design — hidden, not deleted (⌘Z restores). Code is not changed.", if n == 1 { "" } else { "s" }));
    }

    /// ↑/↓: previous/next class (or concept) in name order, zooming to it.
    pub fn select_step(&mut self, forward: bool) {
        let Some(snap) = self.snap.clone() else { return };
        let mut names: Vec<Target> = match self.tab {
            Tab::Class => snap.view.classes.iter().filter(|c| c.status != Status::Pruned).map(|c| Target::Class { name: c.name.clone() }).collect(),
            Tab::Domain => snap.view.domain.concepts.iter().filter(|c| c.status != Status::Pruned).map(|c| Target::Concept { name: c.name.clone() }).collect(),
            _ => return,
        };
        names.sort_by_key(target_name);
        if names.is_empty() {
            return;
        }
        let cur = self.selected_target().and_then(|t| names.iter().position(|x| *x == t));
        let next = match (cur, forward) {
            (None, _) => 0,
            (Some(i), true) => (i + 1) % names.len(),
            (Some(i), false) => (i + names.len() - 1) % names.len(),
        };
        let t = names[next].clone();
        self.selection = vec![t.clone()];
        self.locate_target(&t);
    }
}

fn item(ui: &mut egui::Ui, enabled: Result<(), &str>, label: &str, hint: &str, it: Item) -> Option<Item> {
    let r = ui.add_enabled(enabled.is_ok(), egui::Button::new(RichText::new(label).font(sans(12.5))).frame(false).shortcut_text(hint).min_size(egui::vec2(208.0, 0.0)));
    let r = match enabled {
        Err(why) => r.on_disabled_hover_text(why),
        Ok(()) => r,
    };
    r.clicked().then_some(it)
}

pub fn target_name(t: &Target) -> String {
    match t {
        Target::Class { name } | Target::Package { name } | Target::Concept { name } | Target::System { name } | Target::Container { name } | Target::Actor { name } | Target::UseCase { name } | Target::Sequence { name } => name.clone(),
        Target::Attribute { class, name } | Target::Method { class, name } => format!("{class}.{name}"),
        t => t.label(),
    }
}
