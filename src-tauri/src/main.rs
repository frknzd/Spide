#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod branches;
mod commands;
mod history;
mod mcp;
mod pi_bridge;
mod pi_control;
mod prompts;
mod state;
mod terminal;
mod watcher;

use pi_bridge::PiBridge;
use state::AppState;
use std::path::PathBuf;
use tauri::Manager;

fn default_project() -> PathBuf {
    if let Ok(p) = std::env::var("SPIDE_PROJECT") {
        return PathBuf::from(p);
    }
    dirs::document_dir().unwrap_or_else(|| PathBuf::from(".")).join("Spide").join("untitled")
}

/// macOS menu bar: the default menus (Edit/Window keep copy & paste working) plus File › Export… / Export Again.
fn install_menu(app: &tauri::AppHandle) -> tauri::Result<()> {
    use tauri::menu::{Menu, MenuItem, MenuItemKind, PredefinedMenuItem};
    use tauri::Emitter;
    let menu = Menu::default(app)?;
    let export = MenuItem::with_id(app, "export", "Export…", true, Some("CmdOrCtrl+E"))?;
    let again = MenuItem::with_id(app, "export-again", "Export Again", true, Some("CmdOrCtrl+Shift+E"))?;
    let mut placed = false;
    for item in menu.items()? {
        if let MenuItemKind::Submenu(sm) = item {
            if sm.text()? == "File" {
                sm.insert(&export, 0)?;
                sm.insert(&again, 1)?;
                sm.insert(&PredefinedMenuItem::separator(app)?, 2)?;
                placed = true;
            }
        }
    }
    if !placed {
        let file = tauri::menu::Submenu::with_items(app, "File", true, &[&export, &again])?;
        menu.insert(&file, 1)?;
    }
    app.set_menu(menu)?;
    app.on_menu_event(|app, ev| {
        let name = match ev.id().as_ref() {
            "export" => "menu-export",
            "export-again" => "menu-export-again",
            _ => return,
        };
        let _ = app.emit(name, ());
    });
    Ok(())
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(PiBridge::new())
        .setup(|app| {
            install_menu(app.handle())?;
            let project = default_project();
            std::fs::create_dir_all(&project)?;
            let st = AppState::new(app.handle().clone(), project.clone());
            let port = tauri::async_runtime::block_on(mcp::serve(st.clone()))?;
            st.lock().mcp_port = port;
            app.manage(st.clone());
            watcher::watch(&st, &project);
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                let st = handle.state::<AppState>();
                let pi = handle.state::<PiBridge>();
                if let Err(e) = pi.start(&st) {
                    eprintln!("spide: {e}");
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_snapshot,
            commands::terminal_read,
            commands::terminal_write,
            commands::terminal_resize,
            commands::terminal_context,
            commands::send_prompt,
            commands::abort_agent,
            commands::restart_agent,
            commands::accept_proposal,
            commands::accept_all,
            commands::reject_proposal,
            commands::prune,
            commands::prune_feedback,
            commands::restore,
            commands::submit_defense,
            commands::overrule,
            commands::dismiss_decision,
            commands::undo,
            commands::redo,
            commands::set_layout,
            commands::set_examiner,
            commands::set_show_pruned,
            commands::set_detail,
            commands::apply_user_ops,
            commands::set_partner,
            commands::set_rules,
            commands::fix_issue,
            commands::analyse_change,
            commands::history_list,
            commands::history_at,
            commands::history_restore,
            commands::history_fork,
            commands::branch_list,
            commands::branch_create,
            commands::branch_switch,
            commands::branch_delete,
            commands::branch_compare,
            commands::branch_merge,
            commands::set_brief_skipped,
            commands::set_name,
            commands::regenerate,
            commands::new_design,
            commands::open_design,
            commands::save_as,
            commands::export_png,
            commands::export_pdf,
            commands::export_svg,
            commands::reveal_path,
            commands::export_plantuml,
            commands::export_plantuml_all,
            commands::export_adr,
            commands::export_archunit,
            commands::set_code_root,
            commands::import_code,
            commands::code_snippet,
            commands::drift,
            commands::drift_adopt,
            commands::reveal_project,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Spide")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                app.state::<PiBridge>().shutdown();
            }
        });
}
