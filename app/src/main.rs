mod backend;
use spide_service::{branches, history};
mod commands;
mod events;
mod mcp;
mod menu;
mod pi_bridge;
mod pi_control;
mod project;
mod service;
mod editor;
mod prompts;
mod state;
mod terminal;
mod ui;
mod watcher;
mod workspace;
mod recovery;
mod preferences;
mod opening;
mod macos_open;
mod backup;
mod templates;

use backend::Backend;
use pi_bridge::PiBridge;
use state::AppState;
use std::path::PathBuf;
use std::sync::Arc;

fn main() -> eframe::Result {
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("tokio runtime");
    let (events, rx) = events::Events::new();
    // the welcome screen by default; SPIDE_PROJECT or "reopen last project" open one (strictly)
    let prefs = preferences::default_path().map(|p| preferences::Preferences::load(&p)).unwrap_or_default();
    let cli = opening::cli_path(std::env::args_os().skip(1));
    let initial_path = cli.as_ref().ok().and_then(|p| p.clone());
    let startup = if initial_path.is_some() || cli.is_err() { None } else { std::env::var_os("SPIDE_PROJECT").map(PathBuf::from).or_else(|| prefs.startup_project()) };
    let st = match startup {
        Some(project) => AppState::new(events.clone(), project),
        None => AppState::closed(events.clone()),
    };
    if let Some(path) = initial_path { events.send(events::UiEvent::OpenPaths(vec![path])); }
    if let Err(error) = cli { events.send(events::UiEvent::Toast { kind: "error".into(), text: error }); }
    st.lock().preferences = prefs.clone();
    let open = matches!(st.lock().workspace_status, state::WorkspaceStatus::Ready | state::WorkspaceStatus::ReadOnly);
    match rt.block_on(mcp::serve(st.clone())) {
        Ok(port) => st.lock().mcp_port = port,
        Err(e) => eprintln!("spide: MCP server: {e}"),
    }
    if open {
        service::install(&st);
    }
    if let Err(e) = editor::install_plugin() {
        eprintln!("spide: neovim plugin: {e}");
    }
    if open {
        let design = st.lock().design_path();
        watcher::watch(&st, &design);
        project::watch(&st);
        project::refresh_async(&st);
    }
    let pi = Arc::new(PiBridge::new());
    if open && prefs.agent_auto_start {
        let (st, pi) = (st.clone(), pi.clone());
        std::thread::spawn(move || {
            if let Err(e) = pi.start(&st) {
                eprintln!("spide: {e}");
            }
        });
    }
    let backend_state = st.clone();
    let backend = Backend::new(st, pi.clone(), Some(rt.handle().clone()));
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Spide")
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([800.0, 560.0])
            .with_icon(ui::theme::app_icon())
            .with_fullsize_content_view(true)
            .with_titlebar_shown(false)
            .with_title_shown(false),
        persist_window: true,
        ..Default::default()
    };
    let result = eframe::run_native(
        "Spide",
        options,
        Box::new(move |cc| {
            events.attach(&cc.egui_ctx);
            menu::install(&events);
            macos_open::install(&events);
            Ok(Box::new(ui::app::SpideApp::new(cc, backend, rx)))
        }),
    );
    pi.shutdown();
    editor::embedded().stop();
    service::retract(&backend_state);
    drop(rt);
    result
}
