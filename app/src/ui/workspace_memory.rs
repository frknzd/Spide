//! Local, versioned per-project navigation state, stored with preferences, never in design.json.
use super::{app::{SpideApp, WorkspaceSettings}, chrome::Tab, viewport::Viewport};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use spide_core::Target;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Camera {
    pub x: f32,
    pub y: f32,
    pub zoom: f32,
    pub width: f32,
    pub height: f32,
}
impl Camera {
    pub fn capture(v: &Viewport) -> Option<Self> {
        (v.rect.is_positive() && v.fitted).then(|| Self { x: v.t.x, y: v.t.y, zoom: v.t.k, width: v.rect.width(), height: v.rect.height() })
    }
    pub fn valid(&self) -> bool {
        [self.x, self.y, self.zoom, self.width, self.height].iter().all(|x| x.is_finite()) && (0.15..=3.0).contains(&self.zoom) && self.width > 0.0 && self.height > 0.0
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkspaceView {
    pub version: u32,
    pub tab: Tab,
    pub scenario: Option<String>,
    pub panes: WorkspaceSettings,
    pub cameras: BTreeMap<String, Camera>,
    pub selection: Vec<Target>,
    pub implicit: bool,
    pub lens: String,
}
impl Default for WorkspaceView {
    fn default() -> Self {
        Self { version: 1, tab: Tab::Class, scenario: None, panes: Default::default(), cameras: Default::default(), selection: vec![], implicit: false, lens: "none".into() }
    }
}

impl SpideApp {
    /// Capture only the workspace displayed on screen, not an in-flight project's snapshot.
    pub fn remember_workspace(&mut self) {
        let Some(s) = self.snap.as_ref() else { return };
        if !matches!(s.workspace_status, crate::state::WorkspaceStatus::Ready | crate::state::WorkspaceStatus::ReadOnly) { return; }
        // Merge cameras: a view not visited this session keeps its previous camera.
        let old = self.prefs.workspaces.get(&s.project_dir).cloned().unwrap_or_default();
        let mut cameras = old.cameras;
        for (key, camera) in [
            ("class", (if self.package_scope.is_none() { self.class_canvas.as_ref() } else { self.full_class_canvas.as_ref() }).and_then(|c| Camera::capture(&c.vp))),
            ("domain", self.domain_canvas.as_ref().and_then(|c| Camera::capture(&c.vp))),
            ("package", Camera::capture(&self.views.package_vp)),
            ("system", Camera::capture(&self.views.system_vp)),
            ("usecase", Camera::capture(&self.views.usecase_vp)),
            ("sequence", Camera::capture(&self.views.sequence_vp)),
        ] { if let Some(c) = camera { cameras.insert(key.into(), c); } }
        self.prefs.workspaces.insert(s.project_dir.clone(), WorkspaceView {
            tab: self.tab, scenario: self.scenario.clone(), panes: self.workspace.clone(), cameras,
            selection: self.selection.clone(), implicit: self.show_implicit,
            lens: match self.lens { spide_diagram::class_view::Lens::Concerns => "concerns", spide_diagram::class_view::Lens::Overlap => "overlap", spide_diagram::class_view::Lens::Sync => "sync", _ => "none" }.into(),
            ..Default::default()
        });
    }

    pub fn restore_workspace(&mut self) {
        let s = self.backend.st.snapshot();
        let Some(view) = self.prefs.workspaces.get(&s.project_dir).filter(|v| v.version == 1).cloned() else { return };
        self.tab = view.tab;
        self.workspace = WorkspaceSettings {
            left: view.panes.left.clamp(280.0, 900.0), right: view.panes.right.clamp(300.0, 900.0), ..view.panes
        };
        self.scenario = view.scenario.filter(|n| s.view.sequences.iter().any(|seq| &seq.name == n && seq.status != spide_core::model::Status::Pruned));
        let items = super::search::index(&s.view);
        self.selection = view.selection.into_iter().filter(|t| items.iter().any(|i| &i.target == t && !i.pruned())).collect();
        self.show_implicit = view.implicit;
        self.lens = match view.lens.as_str() {
            "concerns" => spide_diagram::class_view::Lens::Concerns,
            "overlap" => spide_diagram::class_view::Lens::Overlap,
            "sync" if s.project.is_some() => spide_diagram::class_view::Lens::Sync,
            _ => spide_diagram::class_view::Lens::None,
        };
        self.pending_cameras = view.cameras.into_iter().filter(|(_, c)| c.valid()).collect();
    }

    /// Apply after a view's scene has been built. Cancel its initial auto-fit; unvisited cameras
    /// stay pending until their view is first shown. Account for a changed pane/window size.
    pub fn restore_cameras(&mut self) {
        fn apply(v: &mut Viewport, key: &str, cameras: &mut BTreeMap<String, Camera>) {
            if !v.rect.is_positive() || !v.fitted { return; }
            if let Some(c) = cameras.remove(key) {
                v.restore_camera(c.x + (v.rect.width() - c.width) / 2.0, c.y + (v.rect.height() - c.height) / 2.0, c.zoom);
            }
        }
        let full = if self.package_scope.is_none() { self.class_canvas.as_mut() } else { self.full_class_canvas.as_mut() };
        if let Some(c) = full { apply(&mut c.vp, "class", &mut self.pending_cameras); }
        if let Some(c) = self.domain_canvas.as_mut() { apply(&mut c.vp, "domain", &mut self.pending_cameras); }
        apply(&mut self.views.package_vp, "package", &mut self.pending_cameras);
        apply(&mut self.views.system_vp, "system", &mut self.pending_cameras);
        apply(&mut self.views.usecase_vp, "usecase", &mut self.pending_cameras);
        apply(&mut self.views.sequence_vp, "sequence", &mut self.pending_cameras);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::tests::{project, harness, steps};
    #[test]
    fn local_workspace_restores_valid_targets_and_camera_without_changing_design() {
        let dir = project("library");
        let mut h = harness(dir.path(), (1440.0, 900.0));
        steps(&mut h, 120);
        let original = std::fs::read(dir.path().join("design.json")).unwrap();
        h.state_mut().workspace.left = 333.0;
        h.state_mut().class_canvas.as_mut().unwrap().vp.restore_camera(44.0, 55.0, 0.8);
        h.state_mut().selection = vec![Target::Class { name: "RemovedClass".into() }];
        h.state_mut().scenario = Some("Missing scenario".into());
        h.state_mut().remember_workspace();
        let prefs_file = dir.path().join("prefs.json");
        h.state().prefs.save(&prefs_file).unwrap();
        h.state_mut().prefs = crate::preferences::Preferences::load(&prefs_file);
        h.state_mut().workspace.left = 700.0;
        h.state_mut().restore_workspace();
        h.state_mut().restore_cameras();
        assert_eq!(h.state().workspace.left, 333.0);
        assert!(h.state().selection.is_empty());
        assert!(h.state().scenario.is_none());
        assert_eq!(h.state().class_canvas.as_ref().unwrap().vp.t.k, 0.8);
        assert_eq!(h.state().class_canvas.as_ref().unwrap().vp.t.x, 44.0);
        assert_eq!(std::fs::read(dir.path().join("design.json")).unwrap(), original);
        assert!(!Camera { x: f32::NAN, y: 0.0, zoom: 1.0, width: 1.0, height: 1.0 }.valid());
    }
}
