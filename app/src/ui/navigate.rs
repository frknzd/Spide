//! Navigation and small editing helpers behind the action registry: Find / Go to Element,
//! back/forward, locating elements on any view, zoom reset, selection, help.

use super::app::SpideApp;
use super::chrome::Tab;
use super::search::{self, FindOut, FindUi, Item, Location};
use spide_core::Target;

impl SpideApp {
    pub fn open_find(&mut self) {
        self.find = Some(FindUi::new(false));
    }

    pub fn open_outline(&mut self) {
        self.find = Some(FindUi::new(true));
    }

    pub fn find_overlay(&mut self) {
        let Some(mut f) = self.find.take() else { return };
        let Some(snap) = self.snap.clone() else { return };
        let items = search::index(&snap.view);
        match search::show(&self.ctx, &mut f, &items) {
            Some(FindOut::Go(i)) => self.go_to_item(&i),
            Some(FindOut::Close) => {}
            None => self.find = Some(f),
        }
    }

    pub fn current_location(&self) -> Location {
        Location { tab: self.tab, target: self.selected_target(), scenario: self.scenario.clone() }
    }

    /// A deliberate view switch (menu, shortcut, tab bar): remembered for Back.
    pub fn go_to_tab(&mut self, t: Tab) {
        if t == self.tab {
            return;
        }
        let here = self.current_location();
        self.nav.visit(here);
        self.tab = t;
    }

    pub fn go_to_item(&mut self, i: &Item) {
        let here = self.current_location();
        self.nav.visit(here);
        self.show_location(Location { tab: i.tab, target: Some(i.target.clone()), scenario: None });
    }

    fn show_location(&mut self, loc: Location) {
        self.tab = loc.tab;
        if let Some(s) = loc.scenario {
            self.scenario = Some(s);
        }
        let Some(t) = loc.target else {
            self.selection.clear();
            return;
        };
        // the element may be gone (renamed, removed, other branch)
        let exists = self.snap.as_ref().is_some_and(|s| search::index(&s.view).iter().any(|i| i.target == t));
        if !exists {
            self.selection.clear();
            return self.toast("info", format!("{} no longer exists in this design", t.label()));
        }
        if let Target::Sequence { name } = &t {
            self.scenario = Some(name.clone());
        }
        self.selection = vec![t.clone()];
        // the view's scene is built on the next frame: locate then
        self.later(0.12, move |app| app.locate_target(&t));
    }

    /// Zoom the active view onto an element.
    pub fn locate_target(&mut self, t: &Target) {
        let now = self.now();
        match (self.tab, t) {
            (Tab::Class, Target::Class { name } | Target::Attribute { class: name, .. } | Target::Method { class: name, .. }) => {
                self.focus_canvas(Some(name));
                if let Target::Attribute { class, name } | Target::Method { class, name } = t {
                    let k = if matches!(t, Target::Attribute { .. }) { "attribute" } else { "method" };
                    self.hover.pin(&format!("{k}:{class}.{name}"));
                }
            }
            (Tab::Domain, Target::Concept { name }) => {
                if let Some(c) = self.domain_canvas.as_mut() {
                    c.focus(Some(name), now);
                }
            }
            (Tab::Sequence, _) => {}
            (tab, t) => {
                let scene = &self.views.scene;
                let hit = scene.hits.iter().find(|h| matches!(&h.target, spide_diagram::scene::HitTarget::Target(x) if x == t));
                if let Some(spide_diagram::scene::HitShape::Rect { x1, y1, x2, y2 }) = hit.map(|h| h.shape.clone()) {
                    self.views.active_vp(tab).zoom_to((x1 + x2) / 2.0, (y1 + y2) / 2.0, None, now);
                }
            }
        }
    }

    pub fn nav_back(&mut self) {
        let here = self.current_location();
        if let Some(to) = self.nav.back(here) {
            self.show_location(to);
        }
    }

    pub fn nav_forward(&mut self) {
        let here = self.current_location();
        if let Some(to) = self.nav.forward(here) {
            self.show_location(to);
        }
    }

    /// The selected element (the last one picked).
    pub fn selected_target(&self) -> Option<Target> {
        self.selection.last().cloned()
    }

    /// Zoom of the active view (1.0 = 100 %).
    pub fn active_zoom(&mut self) -> Option<f32> {
        match self.tab {
            Tab::Class => self.class_canvas.as_ref().map(|c| c.vp.zoom()),
            Tab::Domain => self.domain_canvas.as_ref().map(|c| c.vp.zoom()),
            t => Some(self.views.active_vp(t).zoom()),
        }
    }

    /// Actual size (100 %), keeping the centre of the view.
    pub fn zoom_reset(&mut self) {
        if let Some(z) = self.active_zoom().filter(|z| *z > 0.0) {
            self.zoom_active(1.0 / z);
        }
    }

    pub fn open_documentation(&mut self) {
        self.documentation_open = true;
    }

    /// Version, platform and workspace state for bug reports. No tokens, prompts, paths or source.
    pub fn diagnostics_text(&self) -> String {
        let s = self.snap.as_ref();
        let status = s.map(|s| format!("{:?}", s.workspace_status).split('(').next().unwrap_or("").to_string()).unwrap_or_default();
        let (classes, rev, project) = s.map(|s| (s.view.classes.len(), s.rev, s.project.is_some())).unwrap_or_default();
        format!(
            "Spide {} ({})\nOS: {} {}\nWorkspace: {status}{}\nDesign: revision {rev}, {classes} classes\nSave: {}\nAgent (pi): {:?}{}\nEmbedded Neovim: {}\n",
            format!("{} · {}", env!("CARGO_PKG_VERSION"), env!("SPIDE_BUILD_REV")),
            if cfg!(debug_assertions) { "debug" } else { "release" },
            std::env::consts::OS,
            std::env::consts::ARCH,
            if project { " · Spide Project" } else { "" },
            s.map(|s| if s.save_status.error.is_some() { "failing" } else if s.save_status.history_error.is_some() { "ok, history failing" } else { "ok" }).unwrap_or("-"),
            s.map(|s| s.pi_status).unwrap_or(crate::state::PiStatus::Offline),
            s.and_then(|s| s.pi_error.as_ref()).map(|_| " (error)").unwrap_or(""),
            if crate::editor::embedded().running() { "running" } else { "off" },
        )
    }

    pub fn copy_diagnostics(&mut self) {
        let t = self.diagnostics_text();
        self.ctx.copy_text(t);
        self.toast("info", "Diagnostics copied (no paths, prompts, tokens or source)");
    }

    pub fn run_action(&self, f: impl FnOnce(&crate::state::AppState, &crate::pi_bridge::PiBridge) -> Result<(), String> + Send + 'static) {
        self.backend.run(f);
    }

    /// The keyboard shortcut reference (Help › Keyboard Shortcuts, ⌘/).
    pub fn shortcuts_overlay(&mut self) {
        if !self.show_shortcuts {
            return;
        }
        use super::actions::{Menu, ACTIONS};
        use super::theme::{bold, css, mono, sans};
        let mut open = true;
        egui::Window::new("Keyboard Shortcuts").open(&mut open).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO).show(&self.ctx, |ui| {
            egui::ScrollArea::vertical().max_height(520.0).show(ui, |ui| {
                ui.columns(2, |cols| {
                    for (n, (title, which)) in [("Spide", Menu::App), ("File", Menu::File), ("Edit", Menu::Edit), ("View", Menu::View), ("Navigate", Menu::Navigate), ("Help", Menu::Help)].into_iter().enumerate() {
                        let ui = &mut cols[if n < 2 { 0 } else { 1 }];
                        ui.label(egui::RichText::new(title).font(bold(13.0)).color(css::TEXT));
                        for a in ACTIONS.iter().filter(|a| a.menu == which && a.shortcut.is_some()) {
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new(a.label).font(sans(12.0)).color(css::DIM));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.label(egui::RichText::new(a.shortcut.unwrap().symbol()).font(mono(11.5)).color(css::TEXT));
                                });
                            });
                        }
                        ui.add_space(8.0);
                    }
                });
                ui.label(egui::RichText::new("Canvas: right-click opens actions · right-drag selects · ⇧-click adds · Delete prunes (⌘Z restores) · ⌥-right-click prunes instantly · ⌥-click pins · double-click renames · ⌘-click opens Neovim · Esc closes the topmost panel").font(sans(11.5)).color(css::FAINT));
            });
        });
        if !open || self.ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.show_shortcuts = false;
        }
    }
}

// ---------------------------------------------------------------- design clipboard (refined later)

impl SpideApp {
    pub fn select_all(&mut self) {
        let Some(snap) = self.snap.clone() else { return };
        self.selection = match self.tab {
            Tab::Domain => snap.view.domain.concepts.iter().filter(|c| c.status != spide_core::model::Status::Pruned).map(|c| Target::Concept { name: c.name.clone() }).collect(),
            _ => snap.view.classes.iter().filter(|c| c.status != spide_core::model::Status::Pruned).map(|c| Target::Class { name: c.name.clone() }).collect(),
        };
    }
}
