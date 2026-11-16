//! macOS menu bar, built from the action registry (`ui::actions`): File, Edit, View, Navigate,
//! Window, Help. Item handles are kept so availability and check marks follow the app state
//! (`sync`). Quit and Close Window are guarded (unsaved work is never lost silently).
//! Other platforms dispatch the same registry from the keyboard handler.

use crate::events::Events;

#[cfg(target_os = "macos")]
mod mac {
    use std::cell::RefCell;
    use std::collections::HashMap;

    pub enum Item {
        Plain(muda::MenuItem),
        Check(muda::CheckMenuItem),
    }

    thread_local! {
        /// The live Open Recent submenu (rebuilt when the recent list changes).
        pub static RECENT: RefCell<Option<muda::Submenu>> = const { RefCell::new(None) };
        /// Registry items by stable key, with the last pushed (enabled, checked).
        pub static ITEMS: RefCell<HashMap<&'static str, (Item, Option<(bool, Option<bool>)>)>> = RefCell::new(HashMap::new());
        /// The menu bar itself (kept alive for the app's lifetime).
        pub static MENU: RefCell<Option<muda::Menu>> = const { RefCell::new(None) };
    }
}

/// Replace the Open Recent items: (label, canonical path, available).
#[cfg(target_os = "macos")]
pub fn set_recent(items: &[(String, String, bool)]) {
    use muda::{MenuItem, PredefinedMenuItem};
    mac::RECENT.with(|r| {
        let Some(sub) = r.borrow().clone() else { return };
        while sub.remove_at(0).is_some() {}
        for (label, path, ok) in items {
            let _ = sub.append(&MenuItem::with_id(format!("recent:{path}"), label, *ok, None));
        }
        if items.is_empty() {
            let _ = sub.append(&MenuItem::new("No Recent Projects", false, None));
        } else {
            let _ = sub.append(&PredefinedMenuItem::separator());
            let _ = sub.append(&MenuItem::with_id("recent-clear", "Clear Unpinned", true, None));
        }
    });
}

#[cfg(not(target_os = "macos"))]
pub fn set_recent(_items: &[(String, String, bool)]) {}

/// Push availability and check marks (only what changed).
#[cfg(target_os = "macos")]
pub fn sync(states: &[(&'static str, bool, Option<bool>)]) {
    mac::ITEMS.with(|items| {
        let mut items = items.borrow_mut();
        for (key, enabled, checked) in states {
            let Some((item, last)) = items.get_mut(key) else { continue };
            if *last == Some((*enabled, *checked)) {
                continue;
            }
            match item {
                mac::Item::Plain(i) => i.set_enabled(*enabled),
                mac::Item::Check(i) => {
                    i.set_enabled(*enabled);
                    i.set_checked(checked.unwrap_or(false));
                }
            }
            *last = Some((*enabled, *checked));
        }
    });
}

#[cfg(not(target_os = "macos"))]
pub fn sync(_states: &[(&'static str, bool, Option<bool>)]) {}

#[cfg(target_os = "macos")]
pub fn install(events: &Events) {
    use crate::events::{MenuAction, UiEvent};
    use crate::ui::actions::{self, Menu as M, ACTIONS};
    use muda::accelerator::Accelerator;
    use muda::{CheckMenuItem, IsMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
    use std::str::FromStr;

    let build = || -> Result<(Menu, Submenu), muda::Error> {
        let menu = Menu::new();
        // not the predefined Quit: that terminates without asking about unsaved work
        let quit = MenuItem::with_id("quit", "Quit Spide", true, Accelerator::from_str("CmdOrCtrl+Q").ok());
        let settings_action = actions::info(actions::ActionId::Settings);
        let settings = MenuItem::with_id(settings_action.key, settings_action.label, true,
            settings_action.shortcut.and_then(|s| Accelerator::from_str(&s.accelerator()).ok()));
        mac::ITEMS.with(|m| m.borrow_mut().insert(settings_action.key, (mac::Item::Plain(settings.clone()), None)));
        let app = Submenu::with_items(
            "Spide",
            true,
            &[
                &PredefinedMenuItem::about(None, Some(muda::AboutMetadata {
                    name: Some("Spide".into()),
                    version: Some(format!("{} ({})", env!("CARGO_PKG_VERSION"), env!("SPIDE_BUILD_REV"))),
                    credits: Some(crate::ui::help::build_details()),
                    ..Default::default()
                })),
                &PredefinedMenuItem::separator(),
                &settings,
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::services(None),
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::hide(None),
                &PredefinedMenuItem::hide_others(None),
                &PredefinedMenuItem::show_all(None),
                &PredefinedMenuItem::separator(),
                &quit,
            ],
        )?;
        let recent = Submenu::new("Open Recent", true);
        let mut subs = vec![];
        for (title, which) in [("File", M::File), ("Edit", M::Edit), ("View", M::View), ("Navigate", M::Navigate)] {
            let sub = Submenu::new(title, true);
            for a in ACTIONS.iter().filter(|a| a.menu == which) {
                if a.group_start {
                    sub.append(&PredefinedMenuItem::separator())?;
                }
                let acc = a.shortcut.and_then(|s| Accelerator::from_str(&s.accelerator()).ok());
                if a.checkable {
                    let i = CheckMenuItem::with_id(a.key, a.label, true, false, acc);
                    sub.append(&i)?;
                    mac::ITEMS.with(|m| m.borrow_mut().insert(a.key, (mac::Item::Check(i), None)));
                } else {
                    let i = MenuItem::with_id(a.key, a.label, true, acc);
                    sub.append(&i)?;
                    mac::ITEMS.with(|m| m.borrow_mut().insert(a.key, (mac::Item::Plain(i), None)));
                }
                if a.id == actions::ActionId::OpenProject {
                    sub.append(&recent)?;
                }
            }
            if which == M::File {
                sub.append_items(&[&PredefinedMenuItem::separator() as &dyn IsMenuItem, &PredefinedMenuItem::close_window(None)])?;
            }
            subs.push(sub);
        }
        let window = Submenu::with_items(
            "Window",
            true,
            &[&PredefinedMenuItem::minimize(None), &PredefinedMenuItem::fullscreen(None), &PredefinedMenuItem::bring_all_to_front(None)],
        )?;
        let help = Submenu::new("Help", true);
        for a in ACTIONS.iter().filter(|a| a.menu == M::Help) {
            let acc = a.shortcut.and_then(|s| Accelerator::from_str(&s.accelerator()).ok());
            let i = MenuItem::with_id(a.key, a.label, true, acc);
            help.append(&i)?;
            mac::ITEMS.with(|m| m.borrow_mut().insert(a.key, (mac::Item::Plain(i), None)));
        }
        menu.append(&app)?;
        for s in &subs {
            menu.append(s)?;
        }
        menu.append_items(&[&window, &help])?;
        help.set_as_help_menu_for_nsapp();
        window.set_as_windows_menu_for_nsapp();
        Ok((menu, recent))
    };
    match build() {
        Ok((menu, recent)) => {
            menu.init_for_nsapp();
            mac::MENU.with(|m| *m.borrow_mut() = Some(menu));
            mac::RECENT.with(|r| *r.borrow_mut() = Some(recent));
            set_recent(&[]);
        }
        Err(e) => eprintln!("spide: menu: {e}"),
    }
    let events = events.clone();
    MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
        let id = e.id().0.as_str();
        let action = match id {
            "recent-clear" => MenuAction::ClearRecent,
            "quit" => MenuAction::Quit,
            _ => match (id.strip_prefix("recent:"), actions::by_key(id)) {
                (Some(path), _) => MenuAction::OpenRecent(path.to_string()),
                (None, Some(a)) => MenuAction::Run(a),
                _ => return,
            },
        };
        events.send(UiEvent::Menu(action));
    }));
}

#[cfg(not(target_os = "macos"))]
pub fn install(_events: &Events) {}
