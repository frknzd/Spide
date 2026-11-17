//! Integration status uses real connection/index signals; doctor only discovers configuration.
//! No automatic trust grants, provider setup or hidden subprocess analysis.
use super::{app::SpideApp, theme::css};
use crate::state::PiStatus;

#[derive(Default)]
pub struct IntegrationUi {
    pub open: bool,
    pub checking: bool,
    pub doctor: Option<serde_json::Value>,
    pub error: Option<String>,
    /// `lucid_status` (local side; the agent reads the documents for the remote side)
    pub lucid: Option<serde_json::Value>,
}
impl SpideApp {
    pub fn start_or_restart_agent(&mut self) {
        if self.restarting { return; }
        self.restarting = true;
        self.backend.call(crate::commands::restart_agent, |app, result| {
            app.restarting = false;
            match result {
                Ok(()) => app.toast("info", "pi process started; waiting for its integration/trust prompt. This does not mean a task completed."),
                Err(e) => app.toast("error", e),
            }
        });
    }
    /// Abort the current turn, not the user's terminal session. No claim of completion until pi
    /// reports its status through the authenticated control bridge.
    pub fn stop_agent(&mut self) {
        self.backend.run_then(|_, pi| crate::commands::abort_agent(pi), |app, ()| {
            app.toast("info", "Stop request acknowledged by pi. Check the terminal; work may still be settling.");
        });
    }
    pub fn integration_overlay(&mut self) {
        if !self.integrations.open { return; }
        let Some(s) = self.snap.clone() else { return };
        let mut open = true;
        let mut start = false;
        let mut stop = false;
        let mut reindex = false;
        let mut doctor = false;
        let mut lucid_enable = false;
        let mut lucid_agent: Option<&str> = None;
        let (port, service, editors) = {
            let port = self.backend.st.lock().mcp_port;
            let svc = crate::service::get(&self.backend.st);
            (port, svc.is_some(), svc.map(|s| s.editors.list()).unwrap_or_default())
        };
        egui::Window::new("Integrations").open(&mut open).default_width(540.0).show(&self.ctx, |ui| {
            egui::ScrollArea::vertical().max_height(480.0).show(ui, |ui| {
                ui.heading("pi agent");
                ui.label(format!("Status: {:?}", s.pi_status));
                match s.pi_status {
                    PiStatus::Starting => { ui.spinner(); ui.label("Waiting for pi. If it asks to trust the folder, answer in the terminal. Spide never accepts trust prompts for you."); }
                    PiStatus::Offline => { ui.label("Design navigation and editing work without pi. Start it when needed; check PATH or SPIDE_PI if unavailable."); }
                    PiStatus::Busy => { ui.label("Working. Stop requests interrupt the turn; inspect terminal output before assuming it finished."); }
                    PiStatus::Idle => { ui.label("Connected and idle. This is not evidence that a task or tests succeeded."); }
                }
                if let Some(e) = &s.pi_error { ui.colored_label(css::WARN, e); }
                ui.horizontal(|ui| {
                    start = ui.add_enabled(self.workspace_open() && !self.restarting, egui::Button::new(if s.pi_status == PiStatus::Offline { "Start Agent" } else { "Restart Agent" })).clicked();
                    stop = ui.add_enabled(matches!(s.pi_status, PiStatus::Busy | PiStatus::Idle), egui::Button::new("Stop Agent")).clicked();
                });
                ui.separator();
                ui.heading("MCP and editors");
                ui.label(if port > 0 { format!("Local MCP listener: port {port}") } else { "MCP listener unavailable. Restart Spide to retry binding the listener.".into() });
                ui.label(if service { "Workspace service installed" } else { "No workspace service (open a valid project first)" });
                ui.label(format!("Embedded Neovim: {}", if crate::editor::embedded().running() { "running" } else { "not running" }));
                if editors.is_empty() { ui.label("No Neovim sessions connected. Open source to attach an editor."); }
                for e in editors { ui.label(format!("{} Neovim · pid {} · LSP clients: {}", e.kind, e.pid, if e.lsp.is_empty() { "none reported".into() } else { e.lsp.join(", ") })); }
                ui.separator();
                ui.heading("Lucid");
                ui.label("Each diagram is a Lucid document colleagues can view, comment on and edit. pi keeps them in sync (git-like): Push updates them in place, Pull turns colleagues' edits into proposals and their comments into Spide comments.");
                match &self.integrations.lucid {
                    Some(l) if l["loading"] == true => { ui.spinner(); }
                    Some(l) if l.get("error").is_some() => { ui.colored_label(css::WARN, l["error"].as_str().unwrap_or("Lucid status unavailable")); }
                    Some(l) if l["enabled"] == true => {
                        ui.label(format!("Syncs the {} branch{}", l["frontier_branch"].as_str().unwrap_or("main"), if l["sync_allowed"] == true { String::new() } else { format!(" — {}", l["reason"].as_str().unwrap_or("")) }));
                        for d in l["diagrams"].as_array().into_iter().flatten() {
                            ui.horizontal(|ui| {
                                let status = d["status"].as_str().unwrap_or("");
                                let local = match status {
                                    "unlinked" => "not in Lucid yet",
                                    "local_ahead" | "diverged" => "changed here since the last sync",
                                    _ => "unchanged here since the last sync",
                                };
                                ui.label(format!("{} · {local}", d["title"].as_str().unwrap_or("")));
                                if let Some(url) = d["url"].as_str() {
                                    if ui.small_button("Open in Lucid").clicked() {
                                        ui.ctx().open_url(egui::OpenUrl::new_tab(url));
                                    }
                                }
                                if let Some(p) = d["pending_proposals"].as_array().filter(|p| !p.is_empty()) {
                                    ui.colored_label(css::WARN, format!("{} undecided proposal(s) from Lucid", p.len()));
                                }
                            });
                        }
                        ui.horizontal(|ui| {
                            let agent = !matches!(s.pi_status, PiStatus::Offline);
                            if ui.add_enabled(agent, egui::Button::new("Check status")).clicked() { lucid_agent = Some("status"); }
                            if ui.add_enabled(agent, egui::Button::new("Pull")).clicked() { lucid_agent = Some("pull"); }
                            if ui.add_enabled(agent, egui::Button::new("Push")).clicked() { lucid_agent = Some("push"); }
                        });
                        ui.label("Sign in once in the terminal with /mcp login lucid. Lucid Team/Enterprise accounts need an admin to enable MCP access.");
                    }
                    Some(_) => {
                        lucid_enable = ui.add_enabled(service, egui::Button::new("Enable Lucid sync")).clicked();
                    }
                    None => { ui.label("Lucid status unavailable (open a valid project first)."); }
                }
                ui.separator();
                ui.heading("Code index and language tools");
                match s.project.as_ref() {
                    Some(p) => {
                        ui.label(format!("Index: {:?} · {} files · {} classes", p.index, p.files, p.code_classes));
                        if matches!(p.index, Some(crate::project::IndexStatus::Indexing)) { ui.spinner(); }
                        for warning in &p.warnings { ui.colored_label(css::WARN, warning); }
                        for path in &p.skipped { ui.label(format!("Not indexed: {path}")); }
                        reindex = ui.button("Re-index code").clicked();
                    }
                    None => { ui.label("Design-only project: no source roots to index. Create a Spide Project to connect code."); }
                }
                ui.label("Tool discovery reports configuration/availability only. It does not run LSP/linters or grant project trust.");
                doctor = ui.add_enabled(service && !self.integrations.checking, egui::Button::new("Refresh tool discovery")).clicked();
                if self.integrations.checking { ui.spinner(); }
                if let Some(e) = &self.integrations.error { ui.colored_label(css::WARN, e); }
                if let Some(d) = &self.integrations.doctor {
                    let mut text = serde_json::to_string_pretty(d).unwrap_or_default();
                    ui.add(egui::TextEdit::multiline(&mut text).code_editor().desired_rows(10).desired_width(f32::INFINITY));
                }
            });
        });
        self.integrations.open = open && !self.ctx.input(|i| i.key_pressed(egui::Key::Escape));
        if self.integrations.open && self.integrations.lucid.is_none() || lucid_enable {
            self.refresh_lucid(lucid_enable);
        }
        if !self.integrations.open {
            self.integrations.lucid = None;
        }
        if let Some(action) = lucid_agent {
            let text = format!("Run spide_lucid with action \"{action}\" for all diagrams and report the result briefly.");
            self.backend.run(move |s, pi| crate::commands::send_prompt(s, pi, text, "class".into()));
            self.integrations.lucid = None;
        }
        if start { self.start_or_restart_agent(); }
        if stop { self.stop_agent(); }
        if reindex { self.backend.run(|s, _| crate::commands::reindex(s)); }
        if doctor {
            self.integrations.checking = true;
            self.backend.call(|s, _| {
                let svc = crate::service::get(s).ok_or("No workspace service")?;
                let text = svc.call_tool("doctor", &serde_json::json!({}), "user")?;
                serde_json::from_str::<serde_json::Value>(&text).map_err(|e| e.to_string())
            }, |app, result| {
                app.integrations.checking = false;
                match result {
                    Ok(d) => { app.integrations.doctor = Some(d); app.integrations.error = None; }
                    Err(e) => { app.integrations.error = Some(e.clone()); app.toast("error", e); }
                }
            });
        }
    }
}

impl SpideApp {
    /// Local Lucid sync status (`lucid_status` without reading the documents); `enable` turns sync on first.
    pub fn refresh_lucid(&mut self, enable: bool) {
        self.integrations.lucid = Some(serde_json::json!({"loading": true}));
        self.backend.call(move |s, _| {
            let svc = crate::service::get(s).ok_or("No workspace service")?;
            if enable {
                svc.call_tool("lucid_link", &serde_json::json!({}), "user")?;
            }
            let text = svc.call_tool("lucid_status", &serde_json::json!({}), "user")?;
            serde_json::from_str::<serde_json::Value>(&text).map_err(|e| e.to_string())
        }, |app, result| match result {
            Ok(v) => app.integrations.lucid = Some(v),
            Err(e) => app.integrations.lucid = Some(serde_json::json!({"error": e})),
        });
    }
}

#[cfg(test)]
mod tests {
    use crate::ui::{actions::ActionId, tests::{project, harness, steps}};
    use egui_kittest::kittest::Queryable;
    #[test]
    fn offline_tools_do_not_block_basic_editing_and_status_is_actionable() {
        let dir = project("library");
        let mut h = harness(dir.path(), (1440.0, 900.0));
        let st = h.state().backend.st.clone();
        st.lock().pi_status = crate::state::PiStatus::Offline;
        st.emit();
        steps(&mut h, 3);
        h.state_mut().perform(ActionId::Integrations);
        steps(&mut h, 3);
        h.get_by_label("Start Agent");
        h.get_by_label("No workspace service (open a valid project first)");
        assert!(!h.state().action_state(ActionId::StopAgent).enabled);
        crate::commands::apply_user_ops(&st, serde_json::from_str(r#"[{"op":"add_class","name":"WorksOffline"}]"#).unwrap()).unwrap();
        assert!(st.lock().store.design.classes.iter().any(|c| c.name == "WorksOffline"));
        h.state_mut().start_or_restart_agent();
        steps(&mut h, 3);
        assert!(!h.state().restarting);
        assert!(st.lock().pi_error.is_some());
        assert_eq!(st.lock().pi_status, crate::state::PiStatus::Offline);
    }
}
