//! The application: UI state and the frame loop.
use crate::backend::Backend;
use crate::events::UiEvent;
use crate::state::Snapshot;
use std::sync::mpsc::Receiver;
use std::sync::Arc;

pub struct SpideApp {
    pub backend: Backend,
    rx: Receiver<UiEvent>,
    pub snap: Option<Arc<Snapshot>>,
    pub toasts: Vec<(String, String)>,
}

impl SpideApp {
    pub fn new(_cc: &eframe::CreationContext<'_>, backend: Backend, rx: Receiver<UiEvent>) -> Self {
        let snap = Some(Arc::new(backend.st.snapshot()));
        Self { backend, rx, snap, toasts: vec![] }
    }
    pub fn toast(&mut self, kind: &str, text: impl Into<String>) {
        self.toasts.push((kind.to_string(), text.into()));
    }
    fn drain(&mut self) {
        while let Ok(e) = self.rx.try_recv() {
            match e {
                UiEvent::Snapshot(s) => self.snap = Some(s),
                UiEvent::Toast { kind, text } => self.toast(&kind, text),
                UiEvent::Task(f) => f(self),
                _ => {}
            }
        }
    }
}

impl eframe::App for SpideApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.drain();
        if let Some(s) = &self.snap {
            ui.label(format!("rev {} · health {}", s.rev, s.metrics.health));
        }
    }
}
