//! One guard for every way of leaving a workspace (New/Open/Recent/Close Project, Close Window,
//! Quit). Each issue is resolved explicitly by the user; nothing is discarded silently.

use super::app::SpideApp;
use crate::editor::EditorStatus;
use crate::state::PiStatus;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaveReason {
    SwitchProject,
    /// another branch's design replaces this one (code and editors are unaffected)
    SwitchBranch,
    CloseProject,
    CloseWindow,
    Quit,
}

impl LeaveReason {
    fn verb(self) -> &'static str {
        match self {
            LeaveReason::SwitchProject => "switching projects",
            LeaveReason::SwitchBranch => "switching branches",
            LeaveReason::CloseProject => "closing this project",
            LeaveReason::CloseWindow => "closing the window",
            LeaveReason::Quit => "quitting Spide",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeaveIssue {
    SaveFailed(String),
    PlanDraft,
    UnsentText(&'static str),
    EditorDirty(Vec<String>),
    EditorUnknown(String),
    AgentBusy,
    ExportRunning,
    TerminalSession,
}

/// A question shown to the user: index of the chosen button, `None` = cancel.
pub struct Ask {
    pub title: String,
    pub text: String,
    pub buttons: Vec<&'static str>,
}

impl SpideApp {
    /// Everything that would be lost or interrupted by leaving now.
    pub fn leave_issues(&self) -> Vec<LeaveIssue> {
        let mut out = vec![];
        let snap = self.backend.st.snapshot();
        if let Some(e) = snap.save_status.error.clone() {
            out.push(LeaveIssue::SaveFailed(e));
        }
        if self.proposal_ui.unsaved() {
            out.push(LeaveIssue::PlanDraft);
        }
        if self.inline.as_ref().is_some_and(|i| i.value != i.original) {
            out.push(LeaveIssue::UnsentText("an unfinished rename"));
        }
        if self.class_editor.is_some() || self.new_element.is_some() || self.project_ui.form.is_some() || self.brief_ui.has_input()
            || self.proposal_ui.notes.values().any(|n| !n.trim().is_empty()) || self.decision_ui.drafts.has_input() {
            out.push(LeaveIssue::UnsentText("unsent notes or form input"));
        }
        if self.prune.as_ref().is_some_and(|p| !p.text.trim().is_empty()) {
            out.push(LeaveIssue::UnsentText("unsent prune feedback"));
        }
        match crate::editor::embedded_status(&self.backend.st) {
            EditorStatus::Dirty(d) => out.push(LeaveIssue::EditorDirty(d)),
            EditorStatus::Unknown(e) => out.push(LeaveIssue::EditorUnknown(e)),
            _ => {}
        }
        if snap.pi_status == PiStatus::Busy {
            out.push(LeaveIssue::AgentBusy);
        }
        if self.export.as_ref().is_some_and(|d| d.run.as_ref().is_some_and(|r| !r.done())) {
            out.push(LeaveIssue::ExportRunning);
        }
        if self.backend.pi.terminal(None).is_ok() { out.push(LeaveIssue::TerminalSession); }
        out
    }

    fn ask(&mut self, q: Ask) -> Option<usize> {
        if let Some(answers) = self.leave_answers.as_mut() {
            return answers.pop_front().flatten();
        }
        let mut buttons = q.buttons.clone();
        buttons.push("Cancel");
        let b = match buttons.len() {
            2 => rfd::MessageButtons::OkCancelCustom(buttons[0].into(), buttons[1].into()),
            _ => rfd::MessageButtons::YesNoCancelCustom(buttons[0].into(), buttons[1].into(), buttons[2].into()),
        };
        let r = rfd::MessageDialog::new().set_title(&q.title).set_level(rfd::MessageLevel::Warning).set_description(&q.text).set_buttons(b).show();
        let label = match r {
            rfd::MessageDialogResult::Custom(s) => s,
            rfd::MessageDialogResult::Ok | rfd::MessageDialogResult::Yes => buttons[0].to_string(),
            rfd::MessageDialogResult::No => buttons[1].to_string(),
            _ => return None,
        };
        q.buttons.iter().position(|b| *b == label)
    }

    /// Resolve every leave issue with the user. `true`: safe to leave now.
    pub fn confirm_leave(&mut self, reason: LeaveReason) -> bool {
        let mut input_resolved = false;
        for issue in self.leave_issues() {
            if reason == LeaveReason::SwitchBranch && matches!(issue, LeaveIssue::EditorDirty(_) | LeaveIssue::EditorUnknown(_) | LeaveIssue::ExportRunning | LeaveIssue::TerminalSession) {
                continue; // branches change the design, not code buffers; an export keeps its snapshot
            }
            let title = "Spide".to_string();
            match issue {
                LeaveIssue::SaveFailed(e) => {
                    let text = format!("The design could not be saved:\n{e}\n\nResolve this before {}.", reason.verb());
                    match self.ask(Ask { title, text, buttons: vec!["Retry Save", "Save Design Copy…"] }) {
                        Some(0) => {
                            if let Err(e) = self.backend.st.persist_and_emit() {
                                self.toast("error", e);
                                return false;
                            }
                        }
                        Some(1) => {
                            let Some(path) = self.pick_copy_path() else { return false };
                            if let Err(e) = crate::commands::save_as(&self.backend.st, path) {
                                self.toast("error", e);
                                return false;
                            }
                        }
                        _ => return false,
                    }
                }
                LeaveIssue::PlanDraft => {
                    let text = "You are editing a proposal. Keep the plan and unfinished operation form locally for later, or discard the draft.".to_string();
                    match self.ask(Ask { title, text, buttons: vec!["Keep Draft and Leave", "Discard Draft"] }) {
                        Some(0) => {
                            if let Err(e) = self.proposal_ui.drafts.flush().and_then(|_| self.save_input_drafts()) { self.toast("error", e); return false; }
                        }
                        Some(1) => self.proposal_ui.discard_session(),
                        _ => return false,
                    }
                }
                LeaveIssue::UnsentText(what) => {
                    if input_resolved { continue; }
                    let text = format!("You have {what}. Save this input locally for this project, or explicitly discard it. Nothing is submitted to the design or agent.");
                    match self.ask(Ask { title, text, buttons: vec!["Save Input and Leave", "Discard Input"] }) {
                        Some(0) => {
                            if let Err(e) = self.save_input_drafts() { self.toast("error", e); return false; }
                        }
                        Some(1) => self.discard_input_drafts(),
                        _ => return false,
                    }
                    input_resolved = true;
                }
                LeaveIssue::EditorDirty(d) => {
                    self.show_editor();
                    let text = format!("Neovim has {} unsaved buffer(s):\n{}\n\nSave or discard them in Neovim (:wa or :qa!) before {}.", d.len(), d.join("\n"), reason.verb());
                    let _ = self.ask(Ask { title, text, buttons: vec!["Show Neovim"] });
                    return false;
                }
                LeaveIssue::EditorUnknown(e) => {
                    self.show_editor();
                    let text = format!("Spide can't verify whether Neovim has unsaved buffers ({e}).\n\nSave and quit Neovim yourself (:wqa) before {}.", reason.verb());
                    let _ = self.ask(Ask { title, text, buttons: vec!["Show Neovim"] });
                    return false;
                }
                LeaveIssue::AgentBusy => {
                    let text = format!("pi is still working. Wait for it to finish, or stop it before {}.", reason.verb());
                    match self.ask(Ask { title, text, buttons: vec!["Stop Agent and Leave"] }) {
                        Some(0) => {
                            let _ = self.backend.pi.abort();
                        }
                        _ => return false,
                    }
                }
                LeaveIssue::TerminalSession => {
                    let text = "The pi session will stop. Spide cannot inspect or preserve an unsent draft in the terminal. Send it or copy it somewhere safe before leaving.".into();
                    if self.ask(Ask { title, text, buttons: vec!["Stop Session and Leave"] }) != Some(0) { return false; }
                }
                LeaveIssue::ExportRunning => {
                    let text = "A batch export is still running. Files written so far are kept.".to_string();
                    match self.ask(Ask { title, text, buttons: vec!["Stop Export and Leave"] }) {
                        Some(0) => {
                            if let Some(r) = self.export.as_mut().and_then(|d| d.run.as_mut()) {
                                r.cancelled = true;
                            }
                        }
                        _ => return false,
                    }
                }
            }
        }
        if let Err(e) = self.save_input_drafts() { self.toast("error", e); return false; }
        self.remember_workspace();
        self.save_prefs();
        true
    }

    fn pick_copy_path(&mut self) -> Option<String> {
        if self.leave_answers.is_some() {
            return self.leave_copy_path.take();
        }
        let name = self.snap.as_ref().map(|s| s.view.name.clone()).unwrap_or_else(|| "design".into());
        rfd::FileDialog::new().set_file_name(format!("{name}.json")).add_filter("Spide design", &["json"]).save_file().map(|p| p.display().to_string())
    }
}
