//! Unsent UI input, isolated by canonical project + branch. Never submitted automatically.
//! Manual plan edits continue using PlanDrafts; this store covers notes and unfinished forms.
use super::{app::SpideApp, brief::BriefUi, overlays::InlineEdit, properties::{ClassEditor, NewElement}, project::Form, proposal::OpEditor, state::decision_drafts::Drafts};
use serde::{Deserialize, Serialize};
use std::{collections::{BTreeMap, HashMap}, path::PathBuf};

#[derive(Default, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct InputDraft {
    pub revision: u64,
    pub notes: HashMap<String, String>,
    pub decisions: Drafts,
    pub brief: BriefUi,
    pub inline: Option<InlineEdit>,
    pub class: Option<ClassEditor>,
    pub new_element: Option<NewElement>,
    pub project: Option<Form>,
    pub operation: Option<OpEditor>,
    pub prune_feedback: String,
}
impl InputDraft {
    pub fn has_input(&self) -> bool {
        self.notes.values().any(|n| !n.trim().is_empty()) || self.decisions.has_input() || self.brief.has_input()
            || self.inline.is_some() || self.class.is_some() || self.new_element.is_some() || self.project.is_some()
            || self.operation.is_some() || !self.prune_feedback.trim().is_empty()
    }
}

#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct InputDrafts {
    pub version: u32,
    pub entries: BTreeMap<String, InputDraft>,
    #[serde(skip)]
    pub file: Option<PathBuf>,
    #[serde(skip)]
    pub error: Option<String>,
    #[serde(skip)]
    last_text: String,
}
impl Default for InputDrafts {
    fn default() -> Self { Self { version: 1, entries: Default::default(), file: None, error: None, last_text: String::new() } }
}
impl InputDrafts {
    pub fn load(file: PathBuf) -> Self {
        let mut out = match std::fs::read_to_string(&file) {
            Ok(t) => match serde_json::from_str::<Self>(&t) {
                Ok(s) if s.version == 1 => s,
                _ => Self { error: Some(format!("Cannot read drafts from {}. Original file retained; drafts will not overwrite it.", file.display())), ..Default::default() },
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(e) => Self { error: Some(format!("Draft recovery {}: {e}", file.display())), ..Default::default() },
        };
        // Do not replace a malformed/unreadable recovery file with an empty store.
        if out.error.is_none() { out.file = Some(file); }
        out
    }
    pub fn persist(&mut self) -> Result<(), String> {
        if let Some(e) = &self.error { if self.file.is_none() { return Err(e.clone()); } }
        let Some(file) = self.file.as_ref() else { return Ok(()) };
        let text = serde_json::to_string(self).map_err(|e| e.to_string())?;
        if text == self.last_text { return Ok(()) }
        match crate::state::write_atomic(file, &text) {
            Ok(()) => { self.last_text = text; self.error = None; Ok(()) }
            Err(e) => {
                let e = format!("Unsent input could not be saved to {}: {e}. Keep Spide open or copy your input elsewhere.", file.display());
                self.error = Some(e.clone()); Err(e)
            }
        }
    }
}

impl SpideApp {
    fn draft_key(&self) -> Option<String> {
        let s = self.snap.as_ref()?;
        matches!(s.workspace_status, crate::state::WorkspaceStatus::Ready | crate::state::WorkspaceStatus::ReadOnly)
            .then(|| format!("{}\0{}", s.project_dir, s.branch))
    }
    /// Capture the displayed project's inputs, even while a switch is pending.
    pub fn save_input_drafts(&mut self) -> Result<(), String> {
        let Some(key) = self.draft_key() else { return Ok(()) };
        let d = InputDraft {
            revision: self.snap.as_ref().unwrap().rev,
            notes: self.proposal_ui.notes.clone(), decisions: self.decision_ui.drafts.clone(), brief: self.brief_ui.clone(),
            inline: self.inline.clone(), class: self.class_editor.clone(), new_element: self.new_element.clone(),
            project: self.project_ui.form.clone(), operation: self.proposal_ui.editing.clone(),
            prune_feedback: self.prune.as_ref().map(|p| p.text.clone()).or_else(|| self.recovered_feedback.clone()).unwrap_or_default(),
        };
        if d.has_input() { self.input_drafts.entries.insert(key, d); } else { self.input_drafts.entries.remove(&key); }
        self.input_drafts.persist()
    }
    pub fn restore_input_drafts(&mut self) {
        let s = self.backend.st.snapshot();
        let key = format!("{}\0{}", s.project_dir, s.branch);
        let Some(d) = self.input_drafts.entries.get(&key).cloned() else { return };
        self.proposal_ui.notes = d.notes;
        self.decision_ui.drafts = d.decisions;
        self.brief_ui = d.brief;
        self.inline = d.inline.map(|mut i| { i.focused = false; i });
        self.class_editor = d.class;
        self.new_element = d.new_element;
        self.project_ui.form = d.project;
        self.proposal_ui.editing = d.operation;
        if !d.prune_feedback.trim().is_empty() {
            // Never re-prune/re-send just because feedback was recovered.
            self.recovered_feedback = Some(d.prune_feedback);
        }
        self.draft_notice = Some(if s.rev != d.revision {
            format!("Recovered unsent input from revision {}. This design is now revision {}: review the draft against the current design before saving or sending it.", d.revision, s.rev)
        } else { "Recovered unsent input. Nothing has been saved to the design or sent to pi.".into() });
    }
    pub fn discard_input_drafts(&mut self) {
        self.inline = None;
        self.class_editor = None;
        self.new_element = None;
        self.project_ui.form = None;
        self.proposal_ui.editing = None;
        self.proposal_ui.notes.clear();
        self.decision_ui.drafts = Default::default();
        self.brief_ui = Default::default();
        self.prune = None;
        self.recovered_feedback = None;
        self.draft_notice = None;
    }
    pub fn draft_recovery_overlay(&mut self) {
        let message = self.input_drafts.error.clone().or_else(|| self.draft_notice.clone());
        let Some(message) = message else { return };
        let mut done = false;
        let mut discard = false;
        egui::Window::new("Unsent input recovery").collapsible(false).default_width(450.0).show(&self.ctx, |ui| {
            ui.colored_label(super::theme::css::WARN, &message);
            if let Some(text) = self.recovered_feedback.as_ref() {
                ui.label("Recovered feedback (copy it before closing):");
                let mut text = text.clone();
                ui.add(egui::TextEdit::multiline(&mut text).desired_rows(3));
                if ui.button("Copy feedback").clicked() { ui.ctx().copy_text(text); }
            }
            ui.horizontal(|ui| {
                if self.input_drafts.error.is_some() {
                    if ui.button("Retry draft save").clicked() { let _ = self.input_drafts.persist(); }
                } else {
                    done = ui.button("Keep editing").clicked();
                    discard = ui.button("Discard recovered input").clicked();
                }
            });
        });
        if discard { self.discard_input_drafts(); let _ = self.save_input_drafts(); }
        if done { self.draft_notice = None; self.recovered_feedback = None; }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::tests::{project, harness, steps};
    #[test]
    fn notes_and_forms_recover_per_project_with_stale_revision_notice() {
        let a = project("library");
        let b = project("rest-extraction");
        let file = a.path().join("local-drafts.json");
        let mut h = harness(a.path(), (1440.0, 900.0));
        steps(&mut h, 3);
        h.state_mut().input_drafts = InputDrafts::load(file.clone());
        h.state_mut().proposal_ui.notes.insert("a decision".into(), "Keep this revision request".into());
        h.state_mut().class_editor = Some(ClassEditor::new_class(None));
        h.state_mut().class_editor.as_mut().unwrap().name = "Unfinished".into();
        h.state_mut().save_input_drafts().unwrap();
        let mut other = harness(b.path(), (1440.0, 900.0));
        other.state_mut().input_drafts = InputDrafts::load(file.clone());
        other.state_mut().restore_input_drafts();
        assert!(other.state().class_editor.is_none());
        h.state_mut().discard_input_drafts();
        h.state().backend.st.lock().store.design.rev += 2;
        h.state_mut().input_drafts = InputDrafts::load(file);
        h.state_mut().restore_input_drafts();
        assert_eq!(h.state().class_editor.as_ref().unwrap().name, "Unfinished");
        assert_eq!(h.state().proposal_ui.notes["a decision"], "Keep this revision request");
        assert!(h.state().draft_notice.as_ref().unwrap().contains("review the draft"));
    }
    #[test]
    fn corrupt_drafts_are_preserved_and_write_failures_are_visible() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("drafts.json");
        std::fs::write(&file, "invalid original").unwrap();
        let mut drafts = InputDrafts::load(file.clone());
        assert!(drafts.persist().is_err());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "invalid original");
        std::fs::remove_file(&file).unwrap();
        let mut drafts = InputDrafts::load(file.clone());
        std::fs::create_dir(file.with_extension("json.tmp")).unwrap();
        assert!(drafts.persist().is_err());
        assert!(drafts.error.is_some());
    }
}
