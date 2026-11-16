//! Desktop activity and errors, separate from the agent's design-change trace.
//! Bounded local log. Retries are explicit, safe operations and tied to their originating workspace.
use super::app::SpideApp;
use serde::{Deserialize, Serialize};
use std::{collections::VecDeque, path::PathBuf};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Retry { Save, RestartAgent, Reindex }
#[derive(Clone, Serialize, Deserialize)]
pub struct Entry {
    pub at: u64,
    pub kind: String,
    pub text: String,
    pub project: String,
    pub epoch: u64,
    pub session: String,
    pub retry: Option<Retry>,
}
#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct ActivityLog {
    pub version: u32,
    pub entries: VecDeque<Entry>,
    #[serde(skip)]
    pub file: Option<PathBuf>,
    #[serde(skip)]
    pub error: Option<String>,
    #[serde(skip)]
    pub session: String,
}
impl Default for ActivityLog {
    fn default() -> Self {
        Self { version: 1, entries: Default::default(), file: None, error: None,
            session: format!("{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos()) }
    }
}
impl ActivityLog {
    pub const CAP: usize = 250;
    pub fn load(file: PathBuf) -> Self {
        let mut out = Self::default();
        match std::fs::read_to_string(&file) {
            Ok(text) => match serde_json::from_str::<Self>(&text) {
                Ok(log) if log.version == 1 => out.entries = log.entries.into_iter().rev().take(Self::CAP).collect::<Vec<_>>().into_iter().rev().collect(),
                _ => { out.error = Some(format!("Could not read {}. Original retained.", file.display())); return out; }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {},
            Err(e) => { out.error = Some(format!("Activity log {}: {e}", file.display())); return out; }
        }
        out.file = Some(file);
        out
    }
    pub fn persist(&mut self) {
        if let Some(file) = self.file.as_ref() {
            self.error = serde_json::to_string(self).map_err(|e| e.to_string())
                .and_then(|text| crate::state::write_atomic(file, &text).map_err(|e| format!("Activity log {}: {e}", file.display()))).err();
        }
    }
    pub fn push(&mut self, entry: Entry) {
        self.entries.push_back(entry);
        while self.entries.len() > Self::CAP { self.entries.pop_front(); }
        self.persist();
    }
}
impl SpideApp {
    pub fn record_activity(&mut self, kind: &str, text: String, retry: Option<Retry>) {
        let (project, epoch) = self.snap.as_ref().map(|s| (s.project_dir.clone(), s.epoch)).unwrap_or_default();
        self.activity.push(Entry { at: spide_core::store::now_ms(), kind: kind.into(), text, project, epoch, retry, session: self.activity.session.clone() });
    }
    pub fn can_retry(&self, e: &Entry) -> bool {
        e.session == self.activity.session && self.snap.as_ref().is_some_and(|s| s.project_dir == e.project && s.epoch == e.epoch && s.workspace_status == crate::state::WorkspaceStatus::Ready)
    }
    pub fn retry_activity(&mut self, e: &Entry) {
        if !self.can_retry(e) { return; }
        match e.retry {
            Some(Retry::Save) => self.save_now(),
            Some(Retry::RestartAgent) => self.backend.run(|s, pi| crate::commands::restart_agent(s, pi)),
            Some(Retry::Reindex) => self.backend.run(|s, _| crate::commands::reindex(s)),
            None => {},
        }
    }
    pub fn observe_operation_status(&mut self) {
        let Some(s) = self.snap.clone() else { return };
        let state = format!("{}:{:?}:{}:{:?}", s.epoch, s.pi_status, s.save_status.error.is_some(), s.project.as_ref().map(|p| &p.index));
        if state == self.last_operation_status { return; }
        self.last_operation_status = state;
        if let Some(e) = s.save_status.error.as_ref() {
            self.record_activity("error", e.clone(), Some(Retry::Save));
        }
        let agent = match s.pi_status {
            crate::state::PiStatus::Busy => "Agent working; output and changes are still in progress",
            crate::state::PiStatus::Starting => "Agent starting; waiting for the terminal integration",
            crate::state::PiStatus::Idle => "Agent idle; inspect its output and review any proposals (idle is not a verification result)",
            crate::state::PiStatus::Offline => "Agent offline; design editing remains available",
        };
        self.record_activity("status", agent.into(), (s.pi_status == crate::state::PiStatus::Offline).then_some(Retry::RestartAgent));
        if let Some(p) = s.project.as_ref() {
            self.record_activity("status", format!("Code index: {:?} · {} files", p.index, p.files), Some(Retry::Reindex));
        }
    }
    pub fn activity_overlay(&mut self) {
        if !self.activity_open { return; }
        let mut open = true;
        let mut retry = None;
        let mut clear = false;
        let entries: Vec<Entry> = self.activity.entries.iter().rev().cloned().collect();
        let ctx = self.ctx.clone();
        egui::Window::new("Activity and Errors").open(&mut open).default_width(650.0).default_height(440.0).show(&ctx, |ui| {
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.activity_errors_only, "Errors only");
                if ui.button("Copy log").clicked() {
                    ui.ctx().copy_text(entries.iter().map(|e| format!("{} [{}] {}\n{}", super::format_time(e.at), e.kind, e.text, e.project)).collect::<Vec<_>>().join("\n\n"));
                }
                clear = ui.button("Clear history").clicked();
            });
            if let Some(e) = self.activity.error.as_ref() { ui.colored_label(super::theme::css::WARN, e); }
            if ui.button("Retry writing log").clicked() { self.activity.persist(); }
            egui::ScrollArea::vertical().show(ui, |ui| {
                let visible: Vec<_> = entries.iter().filter(|e| !self.activity_errors_only || e.kind == "error").collect();
                if visible.is_empty() { ui.label("No activity to show."); }
                for (i, e) in visible.into_iter().enumerate() {
                    ui.push_id(i, |ui| {
                        egui::CollapsingHeader::new(format!("{} · {}", super::format_time(e.at), e.text.lines().next().unwrap_or(""))).show(ui, |ui| {
                            let details = format!("{}\n{}", e.text, e.project);
                            ui.add(egui::Label::new(&details).selectable(true));
                            ui.horizontal(|ui| {
                                if ui.button("Copy Details").clicked() { ui.ctx().copy_text(details); }
                                if e.retry.is_some() && ui.add_enabled(self.can_retry(e), egui::Button::new("Retry"))
                                    .on_disabled_hover_text("This operation belongs to another workspace or application session").clicked() { retry = Some(e.clone()); }
                            });
                        });
                    });
                }
            });
        });
        self.activity_open = open && !self.ctx.input(|i| i.key_pressed(egui::Key::Escape));
        if clear { self.activity.entries.clear(); self.activity.persist(); }
        if let Some(e) = retry { self.retry_activity(&e); }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn log_is_retained_bounded_and_retries_do_not_cross_workspace_epochs() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("activity.json");
        let mut log = ActivityLog::load(file.clone());
        for i in 0..260 { log.push(Entry { at: i, kind: "error".into(), text: format!("error {i}"), project: "A".into(), epoch: 1, session: log.session.clone(), retry: Some(Retry::Save) }); }
        assert_eq!(log.entries.len(), ActivityLog::CAP);
        let restored = ActivityLog::load(file);
        assert_eq!(restored.entries.len(), ActivityLog::CAP);
        assert_ne!(restored.session, log.session);
        let fixture = crate::ui::tests::project("library");
        let mut h = crate::ui::tests::harness(fixture.path(), (1440.0, 900.0));
        h.state_mut().record_activity("error", "save failed".into(), Some(Retry::Save));
        let entry = h.state().activity.entries.back().unwrap().clone();
        assert!(h.state().can_retry(&entry));
        h.state().backend.st.lock().epoch += 1;
        h.state_mut().snap = Some(std::sync::Arc::new(h.state().backend.st.snapshot()));
        assert!(!h.state().can_retry(&entry));
    }
}
