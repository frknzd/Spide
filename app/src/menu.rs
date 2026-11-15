//! macOS menu bar: app menu, File › Open Spide Project… / Export… / Export Again, Window. Other platforms rely on the
//! same keyboard shortcuts handled in the UI.

use crate::events::Events;

#[cfg(target_os = "macos")]
pub fn install(events: &Events) {
    use crate::events::{MenuAction, UiEvent};
    use muda::accelerator::Accelerator;
    use muda::{Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
    use std::str::FromStr;

    let build = || -> Result<Menu, muda::Error> {
        let menu = Menu::new();
        let app = Submenu::with_items(
            "Spide",
            true,
            &[
                &PredefinedMenuItem::about(None, None),
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::services(None),
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::hide(None),
                &PredefinedMenuItem::hide_others(None),
                &PredefinedMenuItem::show_all(None),
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::quit(None),
            ],
        )?;
        let export = MenuItem::with_id("export", "Export…", true, Accelerator::from_str("CmdOrCtrl+E").ok());
        let again = MenuItem::with_id("export-again", "Export Again", true, Accelerator::from_str("CmdOrCtrl+Shift+E").ok());
        let open_project = MenuItem::with_id("open-project", "Open Spide Project…", true, None);
        let file = Submenu::with_items("File", true, &[&open_project, &PredefinedMenuItem::separator(), &export, &again, &PredefinedMenuItem::separator(), &PredefinedMenuItem::close_window(None)])?;
        let window = Submenu::with_items(
            "Window",
            true,
            &[&PredefinedMenuItem::minimize(None), &PredefinedMenuItem::fullscreen(None), &PredefinedMenuItem::bring_all_to_front(None)],
        )?;
        menu.append_items(&[&app, &file, &window])?;
        Ok(menu)
    };
    match build() {
        Ok(menu) => {
            menu.init_for_nsapp();
            // The menu must live as long as the app.
            std::mem::forget(menu);
        }
        Err(e) => eprintln!("spide: menu: {e}"),
    }
    let events = events.clone();
    MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
        let action = match e.id().0.as_str() {
            "export" => MenuAction::Export,
            "export-again" => MenuAction::ExportAgain,
            "open-project" => MenuAction::OpenProject,
            _ => return,
        };
        events.send(UiEvent::Menu(action));
    }));
}

#[cfg(not(target_os = "macos"))]
pub fn install(_events: &Events) {}
