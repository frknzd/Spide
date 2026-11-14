mod backend;
mod branches;
mod commands;
mod events;
mod history;
mod mcp;
mod menu;
mod pi_bridge;
mod pi_control;
mod prompts;
mod state;
mod source_tracking;
mod source_implementation;
mod terminal;
mod ui;
mod watcher;

use backend::Backend;
use pi_bridge::PiBridge;
use state::AppState;
use std::path::PathBuf;
use std::sync::Arc;

fn default_project() -> PathBuf {
    if let Ok(p) = std::env::var("SPIDE_PROJECT") {
        return PathBuf::from(p);
    }
    dirs::document_dir().unwrap_or_else(|| PathBuf::from(".")).join("Spide").join("untitled")
}

fn main() -> eframe::Result {
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("tokio runtime");
    let project = default_project();
    if let Err(e) = std::fs::create_dir_all(&project) {
        eprintln!("spide: cannot create {}: {e}", project.display());
    }
    let (events, rx) = events::Events::new();
    let st = AppState::new(events.clone(), project.clone());
    match rt.block_on(mcp::serve(st.clone())) {
        Ok(port) => st.lock().mcp_port = port,
        Err(e) => eprintln!("spide: MCP server: {e}"),
    }
    watcher::watch(&st, &project);
    source_tracking::watch(&st);
    source_tracking::refresh_async(&st);
    let pi = Arc::new(PiBridge::new());
    {
        let (st, pi) = (st.clone(), pi.clone());
        std::thread::spawn(move || {
            if let Err(e) = pi.start(&st) {
                eprintln!("spide: {e}");
            }
        });
    }
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
            Ok(Box::new(ui::app::SpideApp::new(cc, backend, rx)))
        }),
    );
    pi.shutdown();
    drop(rt);
    result
}
