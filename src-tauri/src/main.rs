#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod mcp;
mod pi_bridge;
mod prompts;
mod state;
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

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(PiBridge::default())
        .setup(|app| {
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
            commands::apply_user_ops,
            commands::set_partner,
            commands::set_brief_skipped,
            commands::set_name,
            commands::regenerate,
            commands::new_design,
            commands::open_design,
            commands::save_as,
            commands::export_png,
            commands::export_plantuml,
            commands::export_plantuml_all,
            commands::reveal_project,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Spide")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                app.state::<PiBridge>().stop();
            }
        });
}
