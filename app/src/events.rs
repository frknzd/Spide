//! Backend → UI events (replaces Tauri's `emit`). Every send wakes the UI.

use crate::state::Snapshot;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};

pub type UiTask = Box<dyn FnOnce(&mut crate::ui::app::SpideApp) + Send>;

pub enum UiEvent {
    OpenPaths(Vec<std::path::PathBuf>),
    RaiseWindow,
    Snapshot(Arc<Snapshot>),
    Toast { kind: String, text: String },
    /// `/spide <command>` typed in pi (routed through the extension's control socket).
    SpideCommand { id: String, generation: u64, project: String, command: String },
    Menu(MenuAction),
    /// A background call finished: apply its result to the UI.
    Task(UiTask),
    /// New terminal output was parsed (repaint only).
    TerminalOutput,
    /// Select/focus a design element (`Class` or `Class.method`), e.g. from Neovim.
    Reveal { symbol: String },
    /// A change request from Neovim (`:SpideAsk`) or an agent.
    ChangeRequest { text: String, context: serde_json::Value },
    /// Show the embedded Neovim pane.
    ShowEditor,
    /// A `/command` for the canvas (from an agent's ui_command).
    Command(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuAction {
    /// an item of the action registry (`ui::actions`)
    Run(crate::ui::actions::ActionId),
    OpenRecent(String),
    ClearRecent,
    /// ⌘Q: guarded like closing the window
    Quit,
}

#[derive(Clone)]
pub struct Events {
    tx: Arc<Mutex<Sender<UiEvent>>>,
    ctx: Arc<OnceLock<egui::Context>>,
}

impl Events {
    pub fn new() -> (Self, Receiver<UiEvent>) {
        let (tx, rx) = channel();
        (Self { tx: Arc::new(Mutex::new(tx)), ctx: Arc::new(OnceLock::new()) }, rx)
    }
    /// Attach the UI context once it exists, so sends repaint.
    pub fn attach(&self, ctx: &egui::Context) {
        let _ = self.ctx.set(ctx.clone());
    }
    pub fn send(&self, e: UiEvent) {
        let _ = self.tx.lock().unwrap_or_else(|e| e.into_inner()).send(e);
        self.repaint();
    }
    pub fn repaint(&self) {
        if let Some(c) = self.ctx.get() {
            c.request_repaint();
        }
    }
    pub fn task(&self, f: impl FnOnce(&mut crate::ui::app::SpideApp) + Send + 'static) {
        self.send(UiEvent::Task(Box::new(f)));
    }
}
