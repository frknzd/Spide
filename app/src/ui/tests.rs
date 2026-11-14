//! UI tests: the app rendered headlessly with egui_kittest.
use super::app::SpideApp;
use super::chrome::Tab;
use crate::backend::Backend;
use crate::events::Events;
use crate::pi_bridge::PiBridge;
use crate::state::AppState;
use egui_kittest::Harness;
use std::sync::Arc;

pub fn project(example: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let src = format!("{}/../examples/{example}.json", env!("CARGO_MANIFEST_DIR"));
    std::fs::copy(src, dir.path().join("design.json")).unwrap();
    dir
}

pub fn harness(dir: &std::path::Path, size: (f32, f32)) -> Harness<'static, SpideApp> {
    let (events, rx) = Events::new();
    let st = AppState::new(events.clone(), dir.to_path_buf());
    let backend = Backend::new(st, Arc::new(PiBridge::new()), None);
    let mut rx = Some(rx);
    Harness::builder()
        .with_size(size)
        .with_max_steps(400)
        .with_step_dt(1.0 / 60.0)
        .allow_missing_glyphs()
        .build_eframe(move |cc| {
            events.attach(&cc.egui_ctx);
            SpideApp::with_ctx(&cc.egui_ctx, backend.clone(), rx.take().unwrap(), false)
        })
}

pub fn steps(h: &mut Harness<'static, SpideApp>, n: usize) {
    for _ in 0..n {
        h.step();
    }
}

pub fn shot(h: &mut Harness<'static, SpideApp>, name: &str) {
    let img = h.render().expect("render");
    let out = format!("{}/../target/ui-shots", env!("CARGO_MANIFEST_DIR"));
    std::fs::create_dir_all(&out).unwrap();
    img.save(format!("{out}/{name}.png")).unwrap();
}

#[test]
fn unified_review_approves_a_large_collapsed_plan_with_empty_notes() {
    use egui_kittest::kittest::Queryable;
    let dir = tempfile::tempdir().unwrap();
    let mut h = harness(dir.path(), (1440.0, 720.0));
    let st = h.state().backend.st.clone();
    let ops = (0..100).map(|n| serde_json::from_value(serde_json::json!({"op":"add_class","name":format!("Example{n}")})).unwrap()).collect();
    st.lock().store.propose_ops(ops, Some("Large plan".into()), "agent").unwrap(); st.changed();
    steps(&mut h, 90);
    assert!(st.lock().store.design.classes.is_empty());
    h.get_by_label("Approve design").click();
    steps(&mut h, 30);
    assert_eq!(st.lock().store.design.classes.len(), 100);
    assert!(st.lock().store.design.decisions[0].notes.is_empty());
    assert!(!dir.path().join(".spide/source.json").exists(), "pure design must not create a source attachment");
}

#[test]
fn source_panel_is_optional_and_accessible_for_scoped_java() {
    let dir = project("library"); let repo = tempfile::tempdir().unwrap();
    std::fs::write(repo.path().join("Book.java"), "public class Book { public void read() {} }").unwrap();
    let a = crate::source_tracking::Attachment { repository: repo.path().canonicalize().unwrap(), roots: vec![".".into()], exclusions: vec![] };
    let imported = spide_import::import_java(repo.path(), None).unwrap();
    let source = crate::source_tracking::SourceState { attachment: Some(a.clone()), snapshot: Some(imported.design), files: crate::source_tracking::inventory(&a).unwrap(), status: "current".into(), ..Default::default() };
    crate::source_tracking::persist(dir.path(), &source).unwrap();
    let mut h = harness(dir.path(), (1440.0, 900.0));
    h.state_mut().source_open = true;
    steps(&mut h, 60);
    assert!(h.state().source_open);
    shot(&mut h, "source-scoped-java");
}

#[test]
fn renders_every_tab_of_the_examples() {
    for ex in ["library", "rest-extraction"] {
        let dir = project(ex);
        let mut h = harness(dir.path(), (1440.0, 900.0));
        steps(&mut h, 150);
        shot(&mut h, &format!("{ex}-class"));
        for t in [Tab::System, Tab::Domain, Tab::Package, Tab::Usecase, Tab::Sequence] {
            h.state_mut().tab = t;
            steps(&mut h, 60);
            shot(&mut h, &format!("{ex}-{}", t.key()));
        }
    }
}

/// Font rasterization is platform-specific; run explicitly on macOS when reviewing visuals.
#[test]
#[ignore = "macOS visual baseline; run with --ignored macos_view_snapshots"]
fn macos_view_snapshots() {
    assert!(cfg!(target_os = "macos"));
    let dir = project("library");
    let mut h = harness(dir.path(), (1440.0, 900.0));
    steps(&mut h, 400);
    h.snapshot("library-class");
    for t in [Tab::System, Tab::Domain, Tab::Package, Tab::Usecase, Tab::Sequence] {
        h.state_mut().tab = t;
        steps(&mut h, 400);
        h.snapshot(format!("library-{}", t.key()));
    }
}

#[test]
fn workspace_shortcuts_and_narrow_drawer() {
    let dir = project("rest-extraction");
    let mut h = harness(dir.path(), (1440.0, 900.0));
    steps(&mut h, 150);
    let cmd = egui::Modifiers::COMMAND;
    for (key, tab) in [(egui::Key::Num1, Tab::System), (egui::Key::Num2, Tab::Class), (egui::Key::Num3, Tab::Domain), (egui::Key::Num4, Tab::Package), (egui::Key::Num5, Tab::Usecase), (egui::Key::Num6, Tab::Sequence)] {
        h.key_press_modifiers(cmd, key);
        steps(&mut h, 5);
        assert_eq!(h.state().tab, tab);
    }
    h.key_press_modifiers(cmd, egui::Key::B);
    steps(&mut h, 5);
    assert!(h.state().brief);
    shot(&mut h, "workspace-brief");
    h.key_press_modifiers(cmd, egui::Key::B);
    steps(&mut h, 5);
    assert!(!h.state().brief);
    h.state_mut().open_proposal_review(None);
    steps(&mut h, 60);
    assert!(h.state().review_active);
    shot(&mut h, "workspace-review");
    let mut narrow = harness(dir.path(), (960.0, 720.0));
    steps(&mut narrow, 150);
    narrow.state_mut().open_proposal_review(None);
    steps(&mut narrow, 60);
    assert!(narrow.state().review_active);
    shot(&mut narrow, "workspace-drawer");
}

#[test]
fn panels_compare_timeline_and_code() {
    let dir = project("rest-extraction");
    let mut h = harness(dir.path(), (1440.0, 900.0));
    steps(&mut h, 150);
    h.state_mut().command("/compare");
    steps(&mut h, 20);
    assert!(h.state().compare.is_some());
    shot(&mut h, "panel-compare");
    h.state_mut().compare = None;
    h.state_mut().command("/timeline");
    steps(&mut h, 20);
    assert!(h.state().timeline);
    shot(&mut h, "panel-timeline");
    h.state_mut().command("/timeline");
    // Code modal renders the backend's source text without rich-text interpretation.
    h.state_mut().code_view = Some(("RestExtractor".into(), "public class RestExtractor {\n    void extract() {}\n}".into()));
    steps(&mut h, 20);
    assert!(h.state().code_view.is_some());
    shot(&mut h, "panel-code");
}

#[test]
fn populated_usecase_sequence_and_system_views() {
    use spide_core::model::*;
    let dir = project("library");
    let path = dir.path().join("design.json");
    let mut d: Design = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    d.use_cases = UseCaseModel {
        system: Some("Library".into()),
        actors: vec![Actor { name: "Reader".into(), ..Default::default() }],
        use_cases: vec![UseCase { name: "Borrow book".into(), actors: vec!["Reader".into()], classes: vec!["Catalog".into(), "LendingService".into()], ..Default::default() }],
        ..Default::default()
    };
    d.sequences.push(Sequence {
        name: "Borrow book".into(), use_case: Some("Borrow book".into()),
        participants: vec!["Reader".into(), "Catalog".into(), "LendingService".into()],
        messages: vec![
            Message { from: "Reader".into(), to: "Catalog".into(), method: "findBook".into(), ..Default::default() },
            Message { from: "Catalog".into(), to: "Reader".into(), method: "book".into(), kind: MessageKind::Return, ..Default::default() },
        ], ..Default::default()
    });
    std::fs::write(path, serde_json::to_vec_pretty(&d).unwrap()).unwrap();
    let mut h = harness(dir.path(), (1440.0, 900.0));
    for t in [Tab::Usecase, Tab::Sequence, Tab::System] {
        h.state_mut().tab = t;
        if t == Tab::System { h.state_mut().views.c4_level = Some(spide_diagram::views::C4Level::Context); }
        steps(&mut h, 150);
        assert!(!h.state().views.scene.hits.is_empty(), "{} has interactive diagram elements", t.key());
        shot(&mut h, &format!("populated-{}", t.key()));
    }
}

/// Screen position of a hit (by hover key) on the class canvas.
pub fn class_pos(h: &Harness<'static, SpideApp>, key: &str) -> egui::Pos2 {
    let c = h.state().class_canvas.as_ref().expect("class canvas");
    let hit = c.scene.hits.iter().rev().find(|x| x.hover.as_deref() == Some(key)).unwrap_or_else(|| panic!("no hit for {key}"));
    let p = match &hit.shape {
        spide_diagram::scene::HitShape::Rect { x1, y1, x2, y2 } => [(x1 + x2) / 2.0, (y1 + y2) / 2.0],
        spide_diagram::scene::HitShape::Line { pts, .. } => pts[pts.len() / 2],
    };
    c.vp.view().to_screen(spide_diagram::scene::Xf::ID, p)
}

pub fn click(h: &mut Harness<'static, SpideApp>, pos: egui::Pos2, button: egui::PointerButton) {
    h.event(egui::Event::PointerMoved(pos));
    h.event(egui::Event::PointerButton { pos, button, pressed: true, modifiers: egui::Modifiers::NONE });
    h.step();
    h.event(egui::Event::PointerButton { pos, button, pressed: false, modifiers: egui::Modifiers::NONE });
    h.step();
}

#[test]
fn canvas_hover_prune_and_rename() {
    let dir = project("library");
    let mut h = harness(dir.path(), (1440.0, 900.0));
    steps(&mut h, 150);
    // hover → card
    let p = class_pos(&h, "class:LendingService");
    h.hover_at(p);
    steps(&mut h, 10);
    assert_eq!(h.state().hover.snapshot().map(|x| x.key.as_str()), Some("class:LendingService"));
    shot(&mut h, "interaction-hover");
    // right-click → pruned, feedback popover
    let p = class_pos(&h, "class:NotificationLog");
    click(&mut h, p, egui::PointerButton::Secondary);
    steps(&mut h, 10);
    assert!(h.state().prune.is_some(), "feedback popover opens");
    let pruned = h.state().backend.st.lock().store.design.classes.iter().find(|c| c.name == "NotificationLog").map(|c| c.status);
    assert_eq!(pruned, Some(spide_core::model::Status::Pruned));
    shot(&mut h, "interaction-prune");
    h.key_press(egui::Key::Escape);
    steps(&mut h, 5);
    assert!(h.state().prune.is_none());
    // Simulate a real double-click, not the harness's default 250 ms frames.
    steps(&mut h, 30);
    let p = class_pos(&h, "class:Catalog");
    click(&mut h, p, egui::PointerButton::Primary);
    h.step();
    click(&mut h, p, egui::PointerButton::Primary);
    h.step();
    steps(&mut h, 5);
    assert!(h.state().inline.is_some(), "inline rename opens");
    shot(&mut h, "interaction-rename");
    h.key_press(egui::Key::Escape);
    steps(&mut h, 5);
    assert!(h.state().inline.is_none());
}

#[test]
fn canvas_pin_drag_and_layout_persistence() {
    let dir = project("library");
    let mut h = harness(dir.path(), (1440.0, 900.0));
    steps(&mut h, 150);
    let p = class_pos(&h, "class:Catalog");
    let modifiers = egui::Modifiers { alt: true, ..Default::default() };
    h.event(egui::Event::ModifiersChanged(modifiers));
    h.event(egui::Event::PointerMoved(p));
    h.event(egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: true, modifiers });
    h.step();
    h.event(egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: false, modifiers });
    steps(&mut h, 10);
    let c = h.state().class_canvas.as_ref().unwrap();
    assert!(c.layout.pinned_ids().contains("Catalog"));
    let before = c.layout.positions()["Catalog"].clone();
    let view_key = c.view_key.clone();
    h.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
    let p = class_pos(&h, "class:Catalog");
    h.event(egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE });
    h.step();
    h.event(egui::Event::PointerMoved(p + egui::vec2(30.0, 25.0)));
    h.step();
    h.event(egui::Event::PointerButton { pos: p + egui::vec2(30.0, 25.0), button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::NONE });
    steps(&mut h, 240);
    let after = h.state().class_canvas.as_ref().unwrap().layout.positions()["Catalog"].clone();
    assert!(after.pinned);
    assert!((after.x - before.x).abs() > 5.0, "drag moves the pinned node");
    let saved = h.state().backend.st.lock().store.design.layout[&view_key]["Catalog"].clone();
    assert_eq!(saved.x, after.x);
    assert_eq!(saved.y, after.y);
    assert!(saved.pinned);
}
