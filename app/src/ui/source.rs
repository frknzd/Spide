//! Source attachment controls; repository scope stays explicit and local.
use crate::{backend::Backend, state::Snapshot, source_tracking::{self, Attachment}};
use super::{theme::{bold, css}, widgets};
use egui::{Ui, RichText};

#[derive(Default)]
pub struct SourceUi {
    pub project: String,
    pub repository: String,
    pub roots: String,
    pub exclusions: String,
    pub preview: Option<(Attachment, usize)>,
    pub error: String,
    pub extra_paths: String,
    pub commands: String,
    pub planned_bindings: String,
}
impl SourceUi {
    fn attachment(&self) -> Attachment {
        let lines = |s: &str| s.lines().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect();
        Attachment { repository: self.repository.clone().into(), roots: lines(&self.roots), exclusions: lines(&self.exclusions) }
    }
}
pub fn show(ui: &mut Ui, state: &mut SourceUi, snap: &Snapshot, backend: &Backend) {
    if state.project != snap.project_dir {
        state.project = snap.project_dir.clone();
        if let Some(a) = &snap.source.attachment {
            state.repository = a.repository.display().to_string(); state.roots = a.roots.join("\n"); state.exclusions = a.exclusions.join("\n");
        } else { state.repository.clear(); state.roots.clear(); state.exclusions.clear(); }
        state.preview = None;
    }
    egui::ScrollArea::vertical().id_salt("source-controls").show(ui, |ui| {
        ui.label(RichText::new("Source · Java").font(bold(18.0)));
        widgets::para(ui, "Optional: attach a repository and selected source folders. Observation never edits source or design. Use the same pi agent to discuss drift and implementation.", 12.5, css::DIM);
        ui.label(format!("Status: {}", if snap.source.status.is_empty() { "design only" } else { &snap.source.status }));
        for warning in &snap.source.warnings { widgets::para(ui, warning, 12.0, css::WARN); }
        if let Some(a) = &snap.source.attachment {
            widgets::para(ui, a.repository.display().to_string(), 12.0, css::DIM);
            ui.label(format!("{} tracked files · {} explicit symbol bindings", snap.source.files.len(), snap.source.bindings.len()));
            ui.horizontal_wrapped(|ui| {
                if ui.button("Refresh source").clicked() { source_tracking::refresh_async(&backend.st); }
                if ui.button("Detach").clicked() { backend.call(|s, _| source_tracking::detach(s), |app, r| { if let Err(e) = r { app.source_ui.error = e; } }); }
            });
        }
        if let Some(code) = snap.source.snapshot.as_ref().filter(|_| snap.source.attachment.is_some()) {
            ui.separator();
            egui::CollapsingHeader::new("Design ↔ Java symbol bindings").show(ui, |ui| {
                for class in snap.view.live_classes() {
                    let key = class.name.clone();
                    let current = snap.source.bindings.get(&key).cloned().unwrap_or("Unmapped — choose explicitly".into());
                    let class_label = ui.label(&class.name);
                    egui::ComboBox::from_id_salt(("class-binding", &key)).selected_text(current).width(ui.available_width()).show_ui(ui, |ui| {
                        for c in &code.classes {
                            if let Some(symbol) = &c.source {
                                if ui.selectable_label(false, format!("{} · {}:{}", symbol.symbol, symbol.file, symbol.start_line)).clicked() {
                                    let (key, symbol, generation) = (key.clone(), symbol.symbol.clone(), snap.source.generation);
                                    backend.call(move |s, _| source_tracking::bind(s, key, symbol, generation), |app, r| { if let Err(e) = r { app.source_ui.error = e; } });
                                }
                            }
                        }
                    }).response.labelled_by(class_label.id);
                    if snap.source.bindings.contains_key(&key) && ui.small_button("Unmap class and member bindings").clicked() {
                        let (key, generation) = (key.clone(), snap.source.generation);
                        backend.call(move |s, _| source_tracking::unbind(s, key, generation), |app, r| { if let Err(e) = r { app.source_ui.error = e; } });
                    }
                    if let Some(target) = snap.source.bindings.get(&class.name).and_then(|symbol| code.classes.iter().find(|c| c.source.as_ref().is_some_and(|s| &s.symbol == symbol))) {
                        egui::CollapsingHeader::new("Member bindings (for changed signatures / overloads)").id_salt((&key, "members")).show(ui, |ui| {
                            for method in &class.methods {
                                let member_key = format!("{}.{}", key, method.name);
                                let label = snap.source.bindings.get(&member_key).cloned().unwrap_or_else(|| format!("{} — exact signature matching", method.name));
                                let method_label = ui.label(&method.name);
                                egui::ComboBox::from_id_salt((&member_key, "member-binding")).selected_text(label).width(ui.available_width()).show_ui(ui, |ui| {
                                    for m in &target.methods {
                                        if let Some(symbol) = &m.source {
                                            if ui.selectable_label(false, &symbol.symbol).clicked() {
                                                let (key, symbol, generation) = (member_key.clone(), symbol.symbol.clone(), snap.source.generation);
                                                backend.call(move |s, _| source_tracking::bind(s, key, symbol, generation), |app, r| { if let Err(e) = r { app.source_ui.error = e; } });
                                            }
                                        }
                                    }
                                }).response.labelled_by(method_label.id);
                            }
                        });
                    }
                }
            });
            egui::CollapsingHeader::new("Unbound source classes · adopt individually").show(ui, |ui| {
                for class in &code.classes {
                    let Some(symbol) = &class.source else { continue };
                    if snap.source.bindings.values().any(|s| s == &symbol.symbol) { continue; }
                    ui.label(&symbol.symbol);
                    if ui.add_enabled(snap.source.status == "current", egui::Button::new("Propose adding this class to design")).clicked() {
                        let (symbol, rev, generation) = (symbol.symbol.clone(), snap.rev, snap.source.generation);
                        backend.call(move |s, _| source_tracking::propose_code_class(s, symbol, rev, generation), |app, r| { if let Err(e) = r { app.source_ui.error = e; } });
                    }
                }
            });
            if let Some(mapped) = source_tracking::mapped(&snap.view, &snap.source) {
                let changes = spide_core::source::reconcile(&snap.view, &mapped, snap.source.acknowledged_design.as_ref(), snap.source.acknowledged_source.as_ref(), &snap.source.bindings);
                ui.label(format!("{} mapped differences · unmapped classes require explicit binding", changes.len()));
                for change in changes {
                    ui.label(format!("{} · {}", change.class, change.direction));
                    if ui.add_enabled(snap.source.status == "current", egui::Button::new(if change.conflict { "Review source version (conflict)" } else { "Review source → design changes" })).clicked() {
                        let (class, rev, generation) = (change.class, snap.rev, snap.source.generation);
                        backend.call(move |s, _| source_tracking::propose_reconciliation(s, class, rev, generation), |app, r| { if let Err(e) = r { app.source_ui.error = e; } });
                    }
                }
                if ui.add_enabled(snap.source.status == "current", egui::Button::new("Acknowledge current pair as comparison baseline")).on_hover_text("Records the observed pair, not that all differences are resolved. Does not approve changes.").clicked() {
                    let (rev, generation) = (snap.rev, snap.source.generation);
                    backend.call(move |s, _| source_tracking::acknowledge(s, rev, generation), |app, r| { if let Err(e) = r { app.source_ui.error = e; } });
                }
            }
        }
        if snap.source.attachment.is_some() {
            ui.separator();
            ui.label(RichText::new("Implement approved design in source").font(bold(14.0)));
            widgets::para(ui, "Diagram approval does not edit code. Prepare a request, inspect scope and drift, then separately authorize the agent. Test/build files outside production roots need explicit additional scope.", 12.0, css::DIM);
            ui.label("Additional permitted files (repository-relative, one per line)");
            ui.add(egui::TextEdit::multiline(&mut state.extra_paths).desired_rows(2).hint_text("Additional permitted files, one per line"));
            ui.label("Validation commands (one per line, run in attached repository)");
            ui.add(egui::TextEdit::multiline(&mut state.commands).desired_rows(2).hint_text("./gradlew :addOns:yourAddon:test"));
            ui.label("Planned Java identities for new classes (DesignClass=fully.qualified.Type)");
            ui.add(egui::TextEdit::multiline(&mut state.planned_bindings).desired_rows(2).hint_text("Planned Java identities: DesignClass=fully.qualified.Type"));
            for dec in snap.view.decisions.iter().filter(|d| matches!(d.state, spide_core::DecisionState::Accepted | spide_core::DecisionState::Overruled)) {
                if ui.button(format!("Implement in source · {} · {}", dec.id, dec.question)).clicked() {
                    let lines = |s: &str| s.lines().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect::<Vec<_>>();
                    let mut mappings = std::collections::BTreeMap::new();
                    let mut valid = true;
                    for line in lines(&state.planned_bindings) {
                        if let Some((key, symbol)) = line.split_once('=') { mappings.insert(key.trim().into(), symbol.trim().into()); }
                        else { state.error = "Use DesignClass=fully.qualified.Type for planned bindings".into(); valid = false; }
                    }
                    if valid {
                        let (id, extra, commands) = (dec.id.clone(), lines(&state.extra_paths), lines(&state.commands));
                        backend.call(move |s, _| crate::source_implementation::prepare(s, id, extra, commands, mappings).map(|_| ()), |app, r| { if let Err(e) = r { app.source_ui.error = e; } });
                    }
                }
            }
        }
        for task in snap.implementations.iter().rev() {
            egui::CollapsingHeader::new(format!("{} · {} · {}", task.id, task.decision, task.status)).id_salt((&snap.project_dir, &task.id)).default_open(matches!(task.status.as_str(), "awaiting approval" | "authorized" | "running" | "verifying")).show(ui, |ui| {
                widgets::para(ui, &task.message, 12.0, css::DIM);
                ui.label(format!("Repository: {}", task.attachment.repository.display()));
                ui.label(format!("Production roots: {:?}", task.attachment.roots));
                ui.label(format!("Additional files: {:?}", task.extra_paths));
                ui.label(format!("Validation: {:?}", task.commands));
                ui.label(format!("Design revision {} · branch {}", task.revision, task.branch));
                egui::CollapsingHeader::new("Approved operations, symbols and file baseline").id_salt((&task.id, "scope")).show(ui, |ui| {
                    ui.label(serde_json::to_string_pretty(&task.approved_ops).unwrap());
                    ui.label(format!("Bindings: {:?}", task.bindings));
                    for (path, contents) in &task.baseline { ui.label(format!("{path} · {}", source_tracking::fingerprint(contents.as_bytes()))); }
                });
                egui::CollapsingHeader::new("Drift and conflicts captured for approval").id_salt((&task.id, "drift")).show(ui, |ui| { ui.label(serde_json::to_string_pretty(&task.drift_at_approval).unwrap()); });
                for line in &task.residual { widgets::para(ui, line, 12.0, css::WARN); }
                for result in &task.validation { egui::CollapsingHeader::new("Validation result").id_salt((&task.id, result)).show(ui, |ui| { ui.label(result); }); }
                for (file, diff) in &task.changed_files { egui::CollapsingHeader::new(format!("Task diff · {file}")).id_salt((&task.id, file)).show(ui, |ui| { ui.label(diff); }); }
                if task.status == "awaiting approval" && ui.button("Approve implementation — allow source edits").clicked() {
                    let approved = rfd::MessageDialog::new().set_title("Authorize source implementation").set_description("Allow the same Spide agent to edit the displayed source scope? The displayed validation commands may run when you choose Verify. Existing work must be preserved. No automatic commits or rollback.").set_buttons(rfd::MessageButtons::OkCancel).show() == rfd::MessageDialogResult::Ok;
                    if approved { let id = task.id.clone(); backend.call(move |s, pi| crate::source_implementation::authorize(s, pi, &id), |app, r| { if let Err(e) = r { app.source_ui.error = e; } }); }
                }
                if task.status == "running" && ui.button("Verify implementation (run approved commands)").clicked() {
                    let id = task.id.clone(); backend.call(move |s, _| crate::source_implementation::verify(s, &id), |app, r| { if let Err(e) = r { app.source_ui.error = e; } });
                }
                if matches!(task.status.as_str(), "awaiting approval" | "authorized" | "running" | "verifying") && ui.button("Cancel task (keep written edits)").clicked() {
                    let id = task.id.clone(); backend.call(move |s, pi| crate::source_implementation::cancel(s, pi, &id), |app, r| { if let Err(e) = r { app.source_ui.error = e; } });
                }
            });
        }
        ui.separator();
        ui.label("Repository (existing folder)");
        let mut changed = ui.add(egui::TextEdit::singleline(&mut state.repository).hint_text("Attached repository path")).changed();
        if ui.button("Choose repository…").clicked() {
            if let Some(p) = rfd::FileDialog::new().pick_folder() { state.repository = p.display().to_string(); changed = true; }
        }
        ui.label("Source roots, one repository-relative path per line");
        changed |= ui.add(egui::TextEdit::multiline(&mut state.roots).desired_rows(3).hint_text("addOns/restextraction/src/main/java")).changed();
        ui.label("Excluded repository-relative paths (one per line)");
        changed |= ui.add(egui::TextEdit::multiline(&mut state.exclusions).desired_rows(2).hint_text("Excluded repository-relative paths")).changed();
        if changed { state.preview = None; }
        if ui.button("Preview selected scope").clicked() {
            let a = state.attachment(); let expected = a.clone();
            backend.call(move |_, _| source_tracking::preview(a), move |app, r| {
                if app.source_ui.attachment() != expected { return; }
                match r { Ok(preview) => { app.source_ui.preview = Some(preview); app.source_ui.error.clear(); }, Err(e) => app.source_ui.error = e }
            });
        }
        if let Some((a, count)) = &state.preview {
            ui.label(format!("{count} production Java files in {} selected root(s)", a.roots.len()));
            if ui.button("Attach / apply this scope").clicked() {
                let a = a.clone();
                backend.call(move |s, _| source_tracking::attach(s, a), |app, r| { if let Err(e) = r { app.source_ui.error = e; } });
            }
        }
        if !state.error.is_empty() { widgets::para(ui, &state.error, 12.0, css::BAD); }
    });
}
