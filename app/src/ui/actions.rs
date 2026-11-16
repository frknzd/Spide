//! The action registry: one table of stable IDs, labels, menus and shortcuts. It drives the native
//! menu bar (macOS), keyboard shortcuts elsewhere, the shortcut reference and availability.
//!
//! Exactly-once rule: on macOS every shortcut in this table belongs to its native menu item (the
//! menu consumes the key), so the egui shortcut handler ignores these combinations there. On other
//! platforms the shortcut handler dispatches the same table. Edit actions are focus-aware: in a
//! text field or terminal they are re-delivered as the matching egui event to the focused widget;
//! only on the canvas do they act on the design.

use super::app::SpideApp;
use super::chrome::Tab;
use egui::Key;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActionId {
    NewProject,
    OpenProject,
    CloseProject,
    Save,
    SaveCopy,
    SaveShortcut,
    BackupDesign,
    DuplicateProject,
    Import,
    Export,
    ExportAgain,
    CopyDiagramImage,
    OpenExportFolder,
    Reveal,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
    Duplicate,
    EditProperties,
    NewClass,
    NewPackage,
    NewSystem,
    NewContainer,
    Find,
    Commands,
    View(Tab),
    ZoomIn,
    ZoomOut,
    ZoomActual,
    ZoomFit,
    ToggleAgentPane,
    ToggleInspector,
    ResetLayout,
    ShowPruned,
    Brief,
    Timeline,
    Trace,
    ProjectRail,
    Back,
    Forward,
    Outline,
    Branches,
    RevealInEditor,
    Settings,
    Activity,
    Integrations,
    StopAgent,
    Shortcuts,
    About,
    OpenLogs,
    Documentation,
    CopyDiagnostics,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Menu {
    App,
    File,
    Edit,
    View,
    Navigate,
    Help,
}

/// A shortcut: ⌘ (Ctrl elsewhere) plus `key`, optionally with ⇧/⌥.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shortcut {
    pub key: Key,
    pub shift: bool,
    pub alt: bool,
}

const fn cmd(key: Key) -> Option<Shortcut> {
    Some(Shortcut { key, shift: false, alt: false })
}
const fn shift(key: Key) -> Option<Shortcut> {
    Some(Shortcut { key, shift: true, alt: false })
}

pub struct ActionInfo {
    pub id: ActionId,
    /// stable ID (menu item IDs, tests, docs)
    pub key: &'static str,
    pub label: &'static str,
    pub menu: Menu,
    pub shortcut: Option<Shortcut>,
    /// a separator goes before this item
    pub group_start: bool,
    /// shown as a check mark when on
    pub checkable: bool,
}

const fn a(id: ActionId, key: &'static str, label: &'static str, menu: Menu, shortcut: Option<Shortcut>) -> ActionInfo {
    ActionInfo { id, key, label, menu, shortcut, group_start: false, checkable: false }
}
const fn sep(mut i: ActionInfo) -> ActionInfo {
    i.group_start = true;
    i
}
const fn check(mut i: ActionInfo) -> ActionInfo {
    i.checkable = true;
    i
}

use ActionId as A;
use Menu as M;

pub const ACTIONS: &[ActionInfo] = &[
    a(A::Settings, "settings", "Settings…", M::App, cmd(Key::Comma)),
    a(A::NewProject, "new-project", "New Project…", M::File, cmd(Key::N)),
    a(A::OpenProject, "open-project", "Open Project…", M::File, cmd(Key::O)),
    sep(a(A::CloseProject, "close-project", "Close Project", M::File, shift(Key::W))),
    a(A::Save, "save", "Save", M::File, cmd(Key::S)),
    a(A::SaveCopy, "save-copy", "Save Design Copy…", M::File, shift(Key::S)),
    a(A::SaveShortcut, "save-shortcut", "Save Project Shortcut…", M::File, None),
    a(A::BackupDesign, "backup-design", "Back Up Design State…", M::File, None),
    a(A::DuplicateProject, "duplicate-project", "Duplicate Design Project…", M::File, None),
    sep(a(A::Import, "import", "Import Design JSON…", M::File, None)),
    a(A::Export, "export", "Export…", M::File, cmd(Key::E)),
    a(A::ExportAgain, "export-again", "Export Again", M::File, shift(Key::E)),
    a(A::CopyDiagramImage, "copy-diagram-image", "Copy Diagram as Image", M::File, None),
    a(A::OpenExportFolder, "open-export-folder", "Open Export Folder", M::File, None),
    sep(a(A::Reveal, "reveal", "Reveal Project in Finder", M::File, None)),
    a(A::Undo, "undo", "Undo", M::Edit, cmd(Key::Z)),
    a(A::Redo, "redo", "Redo", M::Edit, shift(Key::Z)),
    sep(a(A::Cut, "cut", "Cut", M::Edit, cmd(Key::X))),
    a(A::Copy, "copy", "Copy", M::Edit, cmd(Key::C)),
    a(A::Paste, "paste", "Paste", M::Edit, cmd(Key::V)),
    a(A::SelectAll, "select-all", "Select All", M::Edit, cmd(Key::A)),
    a(A::Duplicate, "duplicate", "Duplicate", M::Edit, cmd(Key::D)),
    sep(a(A::EditProperties, "edit-properties", "Edit Properties…", M::Edit, cmd(Key::I))),
    a(A::NewClass, "new-class", "New Class…", M::Edit, shift(Key::N)),
    a(A::NewPackage, "new-package", "New Package…", M::Edit, None),
    a(A::NewSystem, "new-system", "New External System…", M::Edit, None),
    a(A::NewContainer, "new-container", "New Container…", M::Edit, None),
    sep(a(A::Find, "find", "Find in Design…", M::Edit, cmd(Key::F))),
    a(A::Commands, "commands", "Commands…", M::Edit, cmd(Key::K)),
    check(a(A::View(Tab::System), "view-system", "System (C4)", M::View, cmd(Key::Num1))),
    check(a(A::View(Tab::Class), "view-class", "Class Diagram", M::View, cmd(Key::Num2))),
    check(a(A::View(Tab::Domain), "view-domain", "Domain Model", M::View, cmd(Key::Num3))),
    check(a(A::View(Tab::Package), "view-package", "Package Diagram", M::View, cmd(Key::Num4))),
    check(a(A::View(Tab::Usecase), "view-usecase", "Use Case Diagram", M::View, cmd(Key::Num5))),
    check(a(A::View(Tab::Sequence), "view-sequence", "Sequence Diagram", M::View, cmd(Key::Num6))),
    sep(a(A::ZoomIn, "zoom-in", "Zoom In", M::View, cmd(Key::Equals))),
    a(A::ZoomOut, "zoom-out", "Zoom Out", M::View, cmd(Key::Minus)),
    a(A::ZoomActual, "zoom-actual", "Actual Size", M::View, cmd(Key::Num0)),
    a(A::ZoomFit, "zoom-fit", "Zoom to Fit", M::View, cmd(Key::Num9)),
    sep(check(a(A::ToggleAgentPane, "toggle-agent", "Agent Pane", M::View, None))),
    check(a(A::ToggleInspector, "toggle-inspector", "Review Pane", M::View, None)),
    a(A::ResetLayout, "reset-layout", "Reset Workspace Layout", M::View, None),
    sep(check(a(A::ShowPruned, "show-pruned", "Show Pruned Elements", M::View, None))),
    check(a(A::Brief, "brief", "Brief and Rules", M::View, cmd(Key::B))),
    check(a(A::Timeline, "timeline", "Timeline", M::View, cmd(Key::T))),
    check(a(A::Trace, "trace", "Agent Trace", M::View, None)),
    check(a(A::ProjectRail, "project-rail", "Spide Project", M::View, None)),
    a(A::Back, "back", "Back", M::Navigate, cmd(Key::OpenBracket)),
    a(A::Forward, "forward", "Forward", M::Navigate, cmd(Key::CloseBracket)),
    sep(a(A::Outline, "outline", "Go to Element…", M::Navigate, shift(Key::O))),
    a(A::Branches, "branches", "Branches…", M::Navigate, None),
    a(A::RevealInEditor, "reveal-in-editor", "Reveal in Editor", M::Navigate, None),
    a(A::Shortcuts, "shortcuts", "Keyboard Shortcuts", M::Help, cmd(Key::Slash)),
    a(A::Documentation, "documentation", "Spide Documentation", M::Help, None),
    a(A::About, "about", "About Spide", M::Help, None),
    a(A::OpenLogs, "open-logs", "Open Logs", M::Help, None),
    a(A::CopyDiagnostics, "copy-diagnostics", "Copy Diagnostics", M::Help, None),
    a(A::Activity, "activity", "Activity and Errors…", M::Help, None),
    a(A::Integrations, "integrations", "Integrations…", M::Help, None),
    a(A::StopAgent, "stop-agent", "Stop Agent", M::View, None),
];

pub fn info(id: ActionId) -> &'static ActionInfo {
    ACTIONS.iter().find(|a| a.id == id).expect("every action is registered")
}

pub fn by_key(key: &str) -> Option<ActionId> {
    ACTIONS.iter().find(|a| a.key == key).map(|a| a.id)
}

/// The action owning a ⌘-combination.
pub fn by_shortcut(key: Key, m: egui::Modifiers) -> Option<ActionId> {
    if !m.command {
        return None;
    }
    ACTIONS.iter().find(|a| a.shortcut.is_some_and(|s| s.key == key && s.shift == m.shift && s.alt == m.alt)).map(|a| a.id)
}

impl Shortcut {
    /// Native accelerator syntax (muda / Tauri).
    pub fn accelerator(&self) -> String {
        let k = match self.key {
            Key::Num0 => "Digit0".to_string(),
            Key::Num1 => "Digit1".into(),
            Key::Num2 => "Digit2".into(),
            Key::Num3 => "Digit3".into(),
            Key::Num4 => "Digit4".into(),
            Key::Num5 => "Digit5".into(),
            Key::Num6 => "Digit6".into(),
            Key::Num9 => "Digit9".into(),
            Key::Equals => "Equal".into(),
            Key::Minus => "Minus".into(),
            Key::OpenBracket => "BracketLeft".into(),
            Key::CloseBracket => "BracketRight".into(),
            Key::Slash => "Slash".into(),
            Key::Comma => "Comma".into(),
            k => k.name().to_string(),
        };
        format!("CmdOrCtrl+{}{}{k}", if self.shift { "Shift+" } else { "" }, if self.alt { "Alt+" } else { "" })
    }
    /// `⇧⌘S`
    pub fn symbol(&self) -> String {
        let k = match self.key {
            Key::Equals => "=".to_string(),
            Key::Minus => "-".into(),
            Key::OpenBracket => "[".into(),
            Key::CloseBracket => "]".into(),
            Key::Slash => "/".into(),
            Key::Comma => ",".into(),
            k => k.symbol_or_name().to_string(),
        };
        format!("{}{}⌘{k}", if self.alt { "⌥" } else { "" }, if self.shift { "⇧" } else { "" })
    }
}

/// Availability of an action right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionState {
    pub enabled: bool,
    /// why it is disabled
    pub reason: Option<&'static str>,
    pub checked: Option<bool>,
}

/// Where keyboard editing goes right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditFocus {
    /// an egui text field (palette, forms, rename)
    Text,
    /// the pi terminal or the embedded Neovim
    Terminal,
    /// the diagram: design undo, element clipboard
    Design,
}

impl SpideApp {
    pub fn edit_focus(&self) -> EditFocus {
        if self.term.as_ref().is_some_and(|t| t.has_focus) || (self.editor_pane && self.nvim_term.as_ref().is_some_and(|t| t.has_focus)) {
            EditFocus::Terminal
        } else if self.ctx.egui_wants_keyboard_input() {
            EditFocus::Text
        } else {
            EditFocus::Design
        }
    }

    pub fn action_state(&self, id: ActionId) -> ActionState {
        let open = self.workspace_open();
        let writable = self.snap.as_ref().is_some_and(|s| s.workspace_status == crate::state::WorkspaceStatus::Ready);
        let focus = self.edit_focus();
        let on = |b: bool| ActionState { enabled: true, reason: None, checked: Some(b) };
        let ok = || ActionState { enabled: true, reason: None, checked: None };
        let off = |r: &'static str| ActionState { enabled: false, reason: Some(r), checked: None };
        let needs_open = |s: ActionState| if open { s } else { ActionState { enabled: false, reason: Some("No project is open"), checked: s.checked.map(|_| false) } };
        match id {
            A::NewProject | A::OpenProject | A::Import | A::About | A::OpenLogs | A::Integrations | A::Activity | A::Settings | A::Shortcuts | A::Documentation | A::CopyDiagnostics | A::Commands => ok(),
            A::CloseProject => {
                if open || matches!(self.snap.as_ref().map(|s| &s.workspace_status), Some(crate::state::WorkspaceStatus::Failed(_))) { ok() } else { off("No project is open") }
            }
            A::BackupDesign => needs_open(ok()),
            A::DuplicateProject => needs_open(if self.snap.as_ref().is_some_and(|s| s.project.is_some()) { off("Use design-state backup; repository duplication is not supported") } else { ok() }),
            A::CopyDiagramImage => needs_open(if self.copying_image { off("Preparing clipboard image…") } else { ok() }),
            A::OpenExportFolder => if self.last_export_folder().is_some() { ok() } else { off("No previous export folder") },
            A::StopAgent => if self.snap.as_ref().is_some_and(|s| matches!(s.pi_status, crate::state::PiStatus::Busy | crate::state::PiStatus::Idle)) { ok() } else { off("No connected agent turn to interrupt") },
            A::SaveShortcut | A::Save | A::SaveCopy | A::Export | A::ExportAgain | A::Reveal | A::Outline | A::Branches => needs_open(ok()),
            // text and terminals own their own undo
            A::Undo | A::Redo if focus != EditFocus::Design => ok(),
            A::Undo => match self.snap.as_ref() {
                _ if !writable => off("The design can't be edited"),
                Some(s) if s.can_undo => ok(),
                _ => off("Nothing to undo"),
            },
            A::Redo => match self.snap.as_ref() {
                _ if !writable => off("The design can't be edited"),
                Some(s) if s.can_redo => ok(),
                _ => off("Nothing to redo"),
            },
            A::Cut | A::Copy | A::Paste | A::SelectAll if focus != EditFocus::Design => ok(),
            A::Copy => match self.clipboard_source() {
                Ok(_) => ok(),
                Err(e) => off(e),
            },
            A::Cut => self.design_clipboard_state(true),
            A::Paste => self.design_clipboard_state(false),
            A::SelectAll => needs_open(if matches!(self.tab, Tab::Class | Tab::Domain) { ok() } else { off("Select All works on the class and domain diagrams") }),
            A::Find => needs_open(ok()),
            A::Duplicate => match self.clipboard_source().map(|_| ()).and(self.canvas_editable()) {
                Ok(()) => ok(),
                Err(e) => off(e),
            },
            A::EditProperties => match (self.selected_target(), self.canvas_editable()) {
                (_, Err(e)) => off(e),
                (Some(spide_core::Target::Class { .. }), Ok(())) => ok(),
                _ => off("Select one class"),
            },
            A::NewClass | A::NewPackage | A::NewSystem | A::NewContainer => match self.canvas_editable() {
                Ok(()) => ok(),
                Err(e) => off(e),
            },
            A::View(t) => needs_open(on(self.tab == t)),
            A::ZoomIn | A::ZoomOut | A::ZoomActual | A::ZoomFit => needs_open(ok()),
            A::ToggleAgentPane => on(self.workspace.left_open),
            A::ToggleInspector => on(self.workspace.right_open),
            A::ResetLayout => ok(),
            A::ShowPruned => needs_open(on(self.snap.as_ref().is_some_and(|s| s.view.settings.show_pruned))),
            A::Brief => needs_open(on(self.brief)),
            A::Timeline => needs_open(if self.review_active { ActionState { enabled: false, reason: Some("Leave the review first"), checked: Some(false) } } else { on(self.timeline) }),
            A::Trace => needs_open(on(self.trace_ui.open)),
            A::ProjectRail => needs_open(on(self.project_open)),
            A::Back => if self.nav.can_back() { ok() } else { off("Nothing to go back to") },
            A::Forward => if self.nav.can_forward() { ok() } else { off("Nothing to go forward to") },
            A::RevealInEditor => match self.selected_target() {
                Some(spide_core::Target::Class { .. } | spide_core::Target::Method { .. }) => ok(),
                _ => off("Select a class or method first"),
            },
        }
    }

    /// Run an action (menu, shortcut, palette). Unavailable actions explain why.
    pub fn perform(&mut self, id: ActionId) {
        let st = self.action_state(id);
        if !st.enabled {
            if let Some(r) = st.reason {
                self.toast("info", format!("{}: {r}", info(id).label));
            }
            return;
        }
        match id {
            A::NewProject => self.new_project_dialog(),
            A::OpenProject => self.open_project_dialog(),
            A::CloseProject => self.close_project(),
            A::Save => self.save_now(),
            A::SaveCopy => self.save_copy_dialog(),
            A::SaveShortcut => self.save_project_shortcut(),
            A::BackupDesign => self.backup_dialog(false),
            A::DuplicateProject => self.backup_dialog(true),
            A::Import => self.import_dialog(),
            A::Export => self.open_export(),
            A::ExportAgain => self.export_again(),
            A::CopyDiagramImage => self.copy_diagram_image(),
            A::OpenExportFolder => self.open_export_folder(),
            A::Reveal => self.run_action(|s, _| crate::commands::reveal_project(s)),
            A::Undo | A::Redo | A::Cut | A::Copy | A::Paste | A::SelectAll => self.edit_action(id),
            A::Find => self.open_find(),
            A::Duplicate => self.duplicate(),
            A::EditProperties => {
                if let Some(spide_core::Target::Class { name }) = self.selected_target() {
                    self.open_class_editor(&name);
                }
            }
            A::NewClass => self.new_class(),
            A::NewPackage => self.new_element(super::properties::NewKind::Package),
            A::NewSystem => self.new_element(super::properties::NewKind::System),
            A::NewContainer => self.new_element(super::properties::NewKind::Container),
            A::Commands => self.palette = if self.palette.is_some() { None } else { Some(super::overlays::Palette::new("")) },
            A::View(t) => self.go_to_tab(t),
            A::ZoomIn => self.zoom_active(1.25),
            A::ZoomOut => self.zoom_active(0.8),
            A::ZoomActual => self.zoom_reset(),
            A::ZoomFit => self.fit_active(),
            A::ToggleAgentPane => self.workspace.left_open = !self.workspace.left_open,
            A::ToggleInspector => self.workspace.right_open = !self.workspace.right_open,
            A::ResetLayout => {
                self.workspace = super::app::WorkspaceSettings::default();
                self.toast("info", "Workspace layout reset");
            }
            A::ShowPruned => self.command("/pruned"),
            A::Brief => self.command("/brief"),
            A::Timeline => self.command("/timeline"),
            A::Trace => self.command("/trace"),
            A::ProjectRail => {
                if self.project_open {
                    self.project_open = false;
                } else {
                    self.command("/project");
                }
            }
            A::Back => self.nav_back(),
            A::Forward => self.nav_forward(),
            A::Outline => self.open_outline(),
            A::Branches => self.open_branches(),
            A::RevealInEditor => {
                let (class, method) = match self.selected_target() {
                    Some(spide_core::Target::Class { name }) => (name, None),
                    Some(spide_core::Target::Method { class, name }) => (class, Some(name)),
                    _ => return,
                };
                self.open_in_editor(class, method);
            }
            A::Integrations => self.integrations.open = !self.integrations.open,
            A::StopAgent => self.stop_agent(),
            A::Activity => self.activity_open = !self.activity_open,
            A::Settings => self.settings_open = !self.settings_open,
            A::Shortcuts => self.show_shortcuts = !self.show_shortcuts,
            A::Documentation => self.open_documentation(),
            A::About => self.about_open = !self.about_open,
            A::OpenLogs => self.open_logs(),
            A::CopyDiagnostics => self.copy_diagnostics(),
        }
    }

    /// Edit actions go to whatever has keyboard focus.
    fn edit_action(&mut self, id: ActionId) {
        match self.edit_focus() {
            EditFocus::Text | EditFocus::Terminal => {
                let ev = match id {
                    A::Copy => Some(egui::Event::Copy),
                    A::Cut => Some(egui::Event::Cut),
                    A::Paste => clipboard_text().map(egui::Event::Paste),
                    A::Undo | A::Redo | A::SelectAll => {
                        let key = if id == A::SelectAll { Key::A } else { Key::Z };
                        let modifiers = egui::Modifiers { command: true, mac_cmd: cfg!(target_os = "macos"), ctrl: !cfg!(target_os = "macos"), shift: id == A::Redo, ..Default::default() };
                        Some(egui::Event::Key { key, physical_key: Some(key), pressed: true, repeat: false, modifiers })
                    }
                    _ => None,
                };
                if let Some(ev) = ev {
                    self.ctx.input_mut(|i| i.events.push(ev));
                }
            }
            EditFocus::Design => match id {
                A::Undo => self.command("/undo"),
                A::Redo => self.command("/redo"),
                A::Copy => {
                    self.design_copy();
                }
                A::Cut => self.design_cut(),
                A::Paste => self.design_paste(),
                A::SelectAll => self.select_all(),
                _ => {}
            },
        }
    }

    /// Native menu: enabled/checked state of every item, pushed when it changes.
    pub fn sync_menu(&self) {
        let states: Vec<(&'static str, bool, Option<bool>)> = ACTIONS.iter().map(|a| { let s = self.action_state(a.id); (a.key, s.enabled, s.checked) }).collect();
        crate::menu::sync(&states);
    }
}

pub fn clipboard_text() -> Option<String> {
    arboard::Clipboard::new().ok()?.get_text().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_is_unique_and_every_shortcut_has_one_owner() {
        let keys: std::collections::HashSet<_> = ACTIONS.iter().map(|a| a.key).collect();
        assert_eq!(keys.len(), ACTIONS.len());
        let shortcuts: Vec<_> = ACTIONS.iter().filter_map(|a| a.shortcut).collect();
        for (i, s) in shortcuts.iter().enumerate() {
            assert!(!shortcuts[i + 1..].contains(s), "{} is bound twice", s.symbol());
            assert!(s.accelerator().starts_with("CmdOrCtrl+"));
        }
        let m = egui::Modifiers { command: true, shift: true, ..Default::default() };
        assert_eq!(by_shortcut(Key::S, m), Some(ActionId::SaveCopy));
        assert_eq!(by_shortcut(Key::S, egui::Modifiers::NONE), None);
        assert_eq!(by_key("close-project"), Some(ActionId::CloseProject));
        assert_eq!(info(ActionId::ZoomIn).shortcut.unwrap().accelerator(), "CmdOrCtrl+Equal");
        assert_eq!(info(ActionId::Redo).shortcut.unwrap().symbol(), "⇧⌘Z");
    }
}
