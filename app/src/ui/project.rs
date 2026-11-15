//! The Project rail: a design-only folder can become a Spide Project; a project shows design ↔ code
//! sync per class and starts readjust (code → design) and implement (design → code).

use super::theme::{bold, css, mono, sans};
use super::widgets::{self, button, link, primary, primary_enabled};
use crate::backend::Backend;
use crate::project::{IndexStatus, Preview, ProjectSnap};
use crate::state::{PiStatus, Snapshot};
use egui::{Color32, Margin, RichText, Stroke, Ui};
use spide_core::project::{Command, Compare, Manifest};
use spide_core::sync::{SyncItem, SyncStatus};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// The create / edit-manifest form.
#[derive(Debug, Clone, Default)]
pub struct Form {
    pub root: String,
    pub editing: bool,
    pub name: String,
    pub roots: String,
    pub exclude: String,
    pub code_root: String,
    pub compare: Compare,
    pub test_run: String,
    pub test_cwd: String,
    pub git_exclude: bool,
    pub preview: Option<Preview>,
}

impl Form {
    pub fn new(root: PathBuf, m: &Manifest, editing: bool) -> Self {
        let test = m.commands.get("test");
        Form {
            root: root.display().to_string(),
            editing,
            name: m.name.clone(),
            roots: m.roots.join("\n"),
            exclude: m.exclude.join("\n"),
            code_root: m.code_root.clone().unwrap_or_default(),
            compare: m.compare.clone(),
            test_run: test.map(|c| c.run.clone()).unwrap_or_default(),
            test_cwd: test.and_then(|c| c.cwd.clone()).unwrap_or_default(),
            git_exclude: true,
            preview: None,
        }
    }

    pub fn manifest(&self, base: &Manifest) -> Manifest {
        let lines = |s: &str| s.lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect::<Vec<_>>();
        let mut m = base.clone();
        m.name = self.name.trim().to_string();
        m.roots = lines(&self.roots);
        m.exclude = lines(&self.exclude);
        m.code_root = Some(self.code_root.trim().to_string()).filter(|s| !s.is_empty());
        m.compare = self.compare.clone();
        if self.test_run.trim().is_empty() {
            m.commands.remove("test");
        } else {
            m.commands.insert("test".into(), Command { run: self.test_run.trim().into(), cwd: Some(self.test_cwd.trim().to_string()).filter(|s| !s.is_empty()) });
        }
        m
    }
}

#[derive(Default)]
pub struct ProjectUi {
    pub form: Option<Form>,
    pub confirm_mark: bool,
}

pub enum ProjectOut {
    /// zoom to a class on the class diagram
    Focus(String),
    /// show the sync lens
    Lens,
    /// pick a folder for a new project
    CreateIn,
    /// pick a project to open
    Open,
}

pub fn status_color(s: SyncStatus) -> Color32 {
    let c = match s {
        SyncStatus::InSync => return css::GOOD,
        SyncStatus::Accepted => return css::FAINT,
        SyncStatus::Unreconciled => spide_diagram::class_view::SYNC_UNRECONCILED,
        SyncStatus::DesignAhead => spide_diagram::class_view::SYNC_DESIGN_AHEAD,
        SyncStatus::CodeAhead => spide_diagram::class_view::SYNC_CODE_AHEAD,
        SyncStatus::Conflict => spide_diagram::class_view::SYNC_CONFLICT,
    };
    super::theme::col(c, 1.0)
}

/// One line describing a difference.
pub fn item_text(i: &SyncItem) -> String {
    let what = match (i.kind.as_str(), &i.member) {
        ("class", _) => "class".to_string(),
        ("extends" | "implements", Some(t)) => format!("{} {t}", i.kind),
        (_, Some(m)) => format!("{} {m}", i.kind),
        _ => i.kind.clone(),
    };
    let side = match (&i.design, &i.code) {
        (Some(_), None) => "only in the design",
        (None, Some(_)) => "only in the code",
        _ => "differs",
    };
    let detail = match (&i.design, &i.code) {
        (Some(d), Some(c)) => {
            let diff: Vec<String> = d
                .as_object()
                .map(|o| o.iter().filter(|(k, v)| c.get(*k) != Some(*v) && !v.is_null()).map(|(k, v)| format!("{k}: {} → code {}", short(v), c.get(k).map(short).unwrap_or_default())).collect())
                .unwrap_or_default();
            if diff.is_empty() { String::new() } else { format!(" ({})", diff.join(", ")) }
        }
        _ => String::new(),
    };
    format!("{what} — {side}{detail}")
}

fn short(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => "–".into(),
        v => v.to_string(),
    }
}

fn chip(ui: &mut Ui, text: String, color: Color32) {
    widgets::pill(ui, text, color, color.gamma_multiply(0.5), Color32::TRANSPARENT, sans(11.5));
}

pub fn show(ui: &mut Ui, st: &mut ProjectUi, snap: &Snapshot, backend: &Backend) -> Vec<ProjectOut> {
    let mut out = vec![];
    egui::ScrollArea::vertical().id_salt("project-rail").auto_shrink([false, false]).show(ui, |ui| {
        egui::Frame::new().inner_margin(Margin::symmetric(16, 14)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 7.0;
            if let Some(form) = st.form.as_mut() {
                if form_ui(ui, form, snap, backend) {
                    st.form = None;
                }
                return;
            }
            match &snap.project {
                None => design_only(ui, &mut out),
                Some(p) => project(ui, st, p, snap, backend, &mut out),
            }
        });
    });
    out
}

fn design_only(ui: &mut Ui, out: &mut Vec<ProjectOut>) {
    ui.label(RichText::new("Spide Project").font(bold(16.0)).color(css::TEXT));
    widgets::para(
        ui,
        "This design lives on its own. Make it a Spide Project to keep it with its Java code: the design is committed next to the code (spide/), Spide compares both element by element, and the agent readjusts the diagrams to the code or implements approved design changes in the code.",
        12.5,
        css::DIM,
    );
    ui.add_space(4.0);
    if primary(ui, "Create Spide Project in a code folder…").on_hover_text("Pick the module folder (e.g. zap-extensions/addOns/restextraction). The current design is copied; this folder stays as it is.").clicked() {
        out.push(ProjectOut::CreateIn);
    }
    if button(ui, "Open Spide Project…").clicked() {
        out.push(ProjectOut::Open);
    }
}

fn form_ui(ui: &mut Ui, f: &mut Form, snap: &Snapshot, backend: &Backend) -> bool {
    let mut close = false;
    ui.label(RichText::new(if f.editing { "Edit project manifest" } else { "New Spide Project" }).font(bold(16.0)).color(css::TEXT));
    widgets::para(ui, &f.root, 11.5, css::FAINT);
    let field = |ui: &mut Ui, label: &str, v: &mut String, hint: &str| -> bool {
        ui.label(RichText::new(label).font(sans(12.0)).color(css::DIM));
        ui.add(egui::TextEdit::singleline(v).hint_text(hint).desired_width(f32::INFINITY)).changed()
    };
    let area = |ui: &mut Ui, label: &str, v: &mut String, hint: &str| -> bool {
        ui.label(RichText::new(label).font(sans(12.0)).color(css::DIM));
        ui.add(egui::TextEdit::multiline(v).hint_text(hint).desired_rows(2).desired_width(f32::INFINITY)).changed()
    };
    let mut changed = field(ui, "Name", &mut f.name, "REST extraction");
    changed |= area(ui, "Source roots (relative, one per line)", &mut f.roots, "src/main/java");
    changed |= area(ui, "Excluded paths (relative, one per line)", &mut f.exclude, "src/main/java/org/x/generated");
    changed |= field(ui, "Java root package", &mut f.code_root, "org.zaproxy.addon.restextraction");
    ui.label(RichText::new("Also compare").font(sans(12.0)).color(css::DIM));
    ui.horizontal_wrapped(|ui| {
        changed |= ui.checkbox(&mut f.compare.private_methods, "private methods").changed();
        changed |= ui.checkbox(&mut f.compare.accessors, "getters/setters").changed();
        changed |= ui.checkbox(&mut f.compare.constructors, "constructors").changed();
    });
    field(ui, "Test command (the agent runs it)", &mut f.test_run, "./gradlew :addOns:restextraction:test");
    field(ui, "…in folder (relative to the project)", &mut f.test_cwd, "../..");
    if !f.editing {
        ui.checkbox(&mut f.git_exclude, "Keep Spide's local files out of git (.git/info/exclude)");
    }
    if changed {
        f.preview = None;
    }
    let base = snap.project.as_ref().map(|p| p.manifest.clone()).unwrap_or_default();
    let manifest = f.manifest(&base);
    if let Some(p) = &f.preview {
        if p.errors.is_empty() {
            widgets::para(ui, format!("{} Java files · {} classes{}", p.files, p.classes, p.code_root.as_ref().map(|r| format!(" · package root {r}")).unwrap_or_default()), 12.0, css::GOOD);
        }
        for e in &p.errors {
            widgets::para(ui, e, 12.0, css::BAD);
        }
    }
    ui.horizontal_wrapped(|ui| {
        if button(ui, "Preview").clicked() {
            let (root, m) = (PathBuf::from(&f.root), manifest.clone());
            backend.call(
                move |_, _| Ok(crate::project::preview(&root, &m)),
                |app, r: Result<Preview, String>| {
                    if let (Ok(p), Some(form)) = (r, app.project_ui.form.as_mut()) {
                        if form.code_root.is_empty() {
                            form.code_root = p.code_root.clone().unwrap_or_default();
                        }
                        form.preview = Some(p);
                    }
                },
            );
        }
        let ok = f.preview.as_ref().is_some_and(|p| p.errors.is_empty());
        if primary_enabled(ui, ok, if f.editing { "Save" } else { "Create" }).on_hover_text("Preview first").clicked() {
            let (root, m, git) = (f.root.clone(), manifest.clone(), f.git_exclude);
            if f.editing {
                backend.run_then(move |s, _| crate::commands::save_manifest(s, m), |app, ()| app.project_ui.form = None);
            } else {
                backend.run_then(
                    move |s, pi| crate::commands::create_project(s, pi, root, m, git),
                    |app, added: Vec<String>| {
                        app.project_ui.form = None;
                        app.toast("info", if added.is_empty() { "Spide Project ready".to_string() } else { format!("Spide Project ready · local files excluded from git ({})", added.join(", ")) });
                    },
                );
            }
        }
        if button(ui, "Cancel").clicked() {
            close = true;
        }
    });
    close
}

fn project(ui: &mut Ui, st: &mut ProjectUi, p: &ProjectSnap, snap: &Snapshot, backend: &Backend, out: &mut Vec<ProjectOut>) {
    let busy = snap.pi_status == PiStatus::Busy;
    ui.horizontal(|ui| {
        ui.label(RichText::new(if p.manifest.name.is_empty() { "Spide Project" } else { &p.manifest.name }).font(bold(16.0)).color(css::TEXT));
        if link(ui, "Edit manifest").clicked() {
            st.form = Some(Form::new(PathBuf::from(&p.root), &p.manifest, true));
        }
    });
    widgets::para(ui, &p.root, 11.5, css::FAINT);
    if p.root_missing {
        widgets::para(ui, "The project folder is missing (moved or deleted).", 12.5, css::BAD);
        if button(ui, "Open Spide Project…").clicked() {
            out.push(ProjectOut::Open);
        }
        return;
    }
    widgets::rich(ui, &[("roots ", sans(12.0), css::FAINT), (&p.manifest.roots.join(", "), mono(12.0), css::DIM), ("  ·  package ", sans(12.0), css::FAINT), (p.manifest.code_root.as_deref().unwrap_or("(auto)"), mono(12.0), css::DIM)]);
    ui.horizontal_wrapped(|ui| {
        let (text, color) = match &p.index {
            Some(IndexStatus::Current) => (format!("code indexed · {} files · {} classes", p.files, p.code_classes), css::DIM),
            Some(IndexStatus::Partial) => (format!("code indexed · {} file(s) couldn't be parsed", p.skipped.len()), css::WARN),
            Some(IndexStatus::Indexing) | None => ("indexing the code…".to_string(), css::DIM),
            Some(IndexStatus::Error(e)) => (format!("index failed: {e}"), css::BAD),
        };
        ui.label(RichText::new(text).font(sans(12.0)).color(color));
        if link(ui, "Re-index").clicked() {
            backend.run(|s, _| crate::commands::reindex(s));
        }
    });
    for s in &p.skipped {
        widgets::para(ui, s, 11.5, css::WARN);
    }
    for w in &p.warnings {
        widgets::para(ui, w, 12.0, css::WARN);
    }
    widgets::para(ui, if p.has_baseline { "Baseline: the last time design and code agreed (spide/sync.json)." } else { "No baseline yet: differences are unreconciled until design and code agree once, or you mark them in sync." }, 11.5, css::FAINT);
    if let Some(r) = &p.implementing {
        ui.horizontal(|ui| {
            ui.spinner();
            widgets::para(ui, format!("Implementing {} change(s) in the code with the agent…", r.items.len()), 12.5, css::DIM);
        });
    }
    if let Some(sum) = &p.last_implement {
        egui::Frame::new().fill(css::RAISED).stroke(Stroke::new(1.0, css::LINE)).corner_radius(8).inner_margin(Margin::symmetric(10, 8)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let color = if sum.done == sum.total { css::GOOD } else { css::WARN };
            ui.label(RichText::new(format!("Implemented {}/{}", sum.done, sum.total)).font(bold(13.0)).color(color));
            if !sum.remaining.is_empty() {
                widgets::para(ui, format!("Remaining: {}", sum.remaining.join(", ")), 11.5, css::DIM);
                ui.horizontal_wrapped(|ui| {
                    if button(ui, "Ask agent to finish").clicked() {
                        let classes = sum.classes.clone();
                        backend.run(move |s, pi| crate::commands::implement(s, pi, classes, false));
                    }
                    if button(ui, "Readjust these instead").clicked() {
                        let classes = sum.classes.clone();
                        backend.run(move |s, pi| crate::commands::readjust(s, pi, classes));
                    }
                });
            }
        });
    }
    let Some(r) = &p.report else { return };
    ui.horizontal_wrapped(|ui| {
        for s in [SyncStatus::InSync, SyncStatus::Conflict, SyncStatus::DesignAhead, SyncStatus::CodeAhead, SyncStatus::Unreconciled, SyncStatus::Accepted] {
            let n = r.count(s);
            if n > 0 || s == SyncStatus::InSync {
                chip(ui, format!("{n} {}", s.label()), status_color(s));
            }
        }
    });
    if r.partial {
        widgets::para(ui, "Partial index: absences may not be real. Fix or exclude the skipped files.", 12.0, css::WARN);
    }
    if link(ui, "Show on the class diagram (sync lens)").clicked() {
        out.push(ProjectOut::Lens);
    }
    // group the differences per section and class
    let mut sections: BTreeMap<u8, BTreeMap<&str, Vec<&SyncItem>>> = BTreeMap::new();
    for it in &r.items {
        let order = match it.status {
            SyncStatus::Conflict => 0,
            SyncStatus::DesignAhead => 1,
            SyncStatus::CodeAhead => 2,
            SyncStatus::Unreconciled => 3,
            _ => 4,
        };
        sections.entry(order).or_default().entry(it.class.as_str()).or_default().push(it);
    }
    let mut shown_any = false;
    for (order, classes) in &sections {
        shown_any = true;
        let n: usize = classes.values().map(Vec::len).sum();
        let (title, status) = match order {
            0 => ("Conflicts — both sides changed", SyncStatus::Conflict),
            1 => ("Design ahead — implement in the code", SyncStatus::DesignAhead),
            2 => ("Code ahead — readjust the design", SyncStatus::CodeAhead),
            3 => ("Unreconciled", SyncStatus::Unreconciled),
            _ => ("Accepted differences", SyncStatus::Accepted),
        };
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(format!("{title} ({n})")).font(bold(13.0)).color(status_color(status)));
            match status {
                SyncStatus::DesignAhead if primary_enabled(ui, !busy, format!("Implement in code ({n})")).clicked() => {
                    backend.run(|s, pi| crate::commands::implement(s, pi, vec![], false));
                }
                SyncStatus::CodeAhead if primary_enabled(ui, !busy, format!("Readjust design ({n})")).clicked() => {
                    backend.run(|s, pi| crate::commands::readjust(s, pi, vec![]));
                }
                SyncStatus::Unreconciled => {
                    if button(ui, "Readjust design").on_hover_text("Bring what the code has into the design (agent proposals)").clicked() {
                        backend.run(|s, pi| crate::commands::readjust(s, pi, vec![]));
                    }
                    if button(ui, "Implement in code").on_hover_text("Implement what only the design has").clicked() {
                        backend.run(|s, pi| crate::commands::implement(s, pi, vec![], false));
                    }
                }
                _ => {}
            }
        });
        for (class, items) in classes {
            let header = RichText::new(format!("{class} · {}", items.len())).font(sans(12.5)).color(css::TEXT);
            egui::CollapsingHeader::new(header).id_salt(("sync", order, class)).default_open(*order == 0).show(ui, |ui| {
                for it in items {
                    let loc = it.file.as_ref().map(|f| format!("  {}:{}", f.rsplit('/').next().unwrap_or(f), it.line.unwrap_or(0))).unwrap_or_default();
                    widgets::rich(ui, &[(&item_text(it), sans(12.0), css::DIM), (&loc, mono(11.0), css::FAINT)]).on_hover_text(it.file.clone().unwrap_or_default());
                    if let Some(r) = &it.rename_of {
                        widgets::para(ui, format!("possibly renamed: {r}"), 11.5, css::WARN);
                    }
                }
                ui.horizontal_wrapped(|ui| {
                    let c = class.to_string();
                    if link(ui, "Show").clicked() {
                        out.push(ProjectOut::Focus(c.clone()));
                    }
                    match status {
                        SyncStatus::Conflict => {
                            if button(ui, "Keep design").on_hover_text("Implement the design's version in the code").clicked() {
                                let c = c.clone();
                                backend.run(move |s, pi| crate::commands::implement(s, pi, vec![c], true));
                            }
                            if button(ui, "Keep code").on_hover_text("Readjust the design to the code").clicked() {
                                let c = c.clone();
                                backend.run(move |s, pi| crate::commands::readjust(s, pi, vec![c]));
                            }
                        }
                        SyncStatus::DesignAhead => {
                            if button(ui, "Implement").clicked() {
                                backend.run(move |s, pi| crate::commands::implement(s, pi, vec![c], false));
                            }
                        }
                        SyncStatus::CodeAhead | SyncStatus::Unreconciled => {
                            if button(ui, "Readjust").clicked() {
                                let c = c.clone();
                                backend.run(move |s, pi| crate::commands::readjust(s, pi, vec![c]));
                            }
                            if status == SyncStatus::Unreconciled && items.iter().any(|i| i.design.is_some()) && button(ui, "Implement").clicked() {
                                backend.run(move |s, pi| crate::commands::implement(s, pi, vec![c], false));
                            }
                        }
                        _ => {}
                    }
                });
            });
        }
    }
    if !shown_any {
        widgets::para(ui, "Design and code agree.", 12.5, css::GOOD);
    }
    ui.add_space(8.0);
    widgets::hline(ui);
    ui.add_space(6.0);
    if st.confirm_mark {
        let open = r.items.iter().filter(|i| i.status != SyncStatus::Accepted).count();
        widgets::para(ui, format!("{open} remaining difference(s) become accepted: Spide stops reporting them until either side changes again. This records the current pair in spide/sync.json; it doesn't change design or code."), 12.0, css::DIM);
        ui.horizontal(|ui| {
            if primary(ui, "Mark in sync").clicked() {
                st.confirm_mark = false;
                backend.run_then(|s, _| crate::commands::mark_synced(s), |app, n: usize| app.toast("info", format!("Marked in sync · {n} difference(s) accepted")));
            }
            if button(ui, "Cancel").clicked() {
                st.confirm_mark = false;
            }
        });
    } else {
        ui.horizontal_wrapped(|ui| {
            if button(ui, "Mark in sync…").on_hover_text("Accept the remaining differences as they are").clicked() {
                st.confirm_mark = true;
            }
            let verify = if p.manifest.commands.contains_key("test") { "Verify (re-index + tests)" } else { "Verify (re-index)" };
            if button(ui, verify).clicked() {
                backend.run(|s, pi| crate::commands::verify(s, pi));
            }
        });
    }
    widgets::para(ui, "Structural agreement doesn't prove the behaviour is right — that's what the tests are for.", 11.5, css::FAINT);
}
