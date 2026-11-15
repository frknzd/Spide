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

fn op(v: serde_json::Value) -> spide_core::Op {
    serde_json::from_value(v).unwrap()
}

/// Library + one decision with two alternatives and one small single-option change.
fn review_fixture(h: &mut Harness<'static, SpideApp>) -> (String, String) {
    use spide_core::model::DecisionOption;
    let st = h.state().backend.st.clone();
    let (decision, small) = {
        let mut s = st.lock();
        let decision = s.store.propose_decision("Where should overdue fines be calculated?".into(), Some("Fines depend on the loan period and the member type.".into()), vec![
            DecisionOption { id: "A".into(), label: "Loan calculates its own fine".into(), summary: Some("Information Expert: the loan knows its dates.".into()), pros: vec!["No new class".into()], cons: vec!["Loan grows".into()], ops: vec![op(serde_json::json!({"op":"add_method","class":"Loan","method":{"name":"fine","returns":"Money"}}))], ..Default::default() },
            DecisionOption { id: "B".into(), label: "A FinePolicy strategy".into(), summary: Some("Pure Fabrication for varying fine rules.".into()), pros: vec!["Rules can vary per member type".into()], cons: vec!["One more class".into()], ops: vec![op(serde_json::json!({"op":"add_class","name":"FinePolicy","package":"domain"})), op(serde_json::json!({"op":"add_relation","from":"Loan","to":"FinePolicy","kind":"dependency"}))], ..Default::default() },
        ]).unwrap();
        let p = s.store.propose_ops(vec![op(serde_json::json!({"op":"add_method","class":"Catalog","method":{"name":"search","params":[{"name":"q","type":"String"}],"returns":"List<Book>"}}))], Some("Add catalogue search".into()), "agent").unwrap();
        let small = s.store.design.decisions.iter().find(|d| d.plan.as_deref() == Some(&p)).unwrap().id.clone();
        (decision, small)
    };
    st.changed();
    (decision, small)
}

#[test]
fn diagram_review_is_one_click_and_advances_after_approval() {
    use egui_kittest::kittest::{NodeT, Queryable};
    let dir = project("library");
    let mut h = harness(dir.path(), (1440.0, 900.0));
    steps(&mut h, 60);
    let st = h.state().backend.st.clone();
    let classes_before = st.lock().store.design.classes.len();
    let (decision, small) = review_fixture(&mut h);
    h.state_mut().open_proposal_review(None);
    steps(&mut h, 90);
    // the first plan is on the diagram right away, without committing anything
    assert!(h.state().review_active);
    assert!(h.state().proposal_ui.review.is_some(), "review loads without a Preview click");
    assert!(h.state().proposal_canvas.as_ref().is_some_and(|d| d.classes.iter().any(|c| c.name == "Loan" && c.methods.iter().any(|m| m.name == "fine"))));
    assert_eq!(st.lock().store.design.classes.len(), classes_before);
    // question, alternatives and their tradeoffs are visible without disclosure clicks
    h.get_by_label("Where should overdue fines be calculated?");
    h.get_by_label("+ Rules can vary per member type");
    h.get_by_label("Approve design");
    shot(&mut h, "review-rail");
    // one click on the other alternative replaces the plan on the diagram
    h.get_by_label("A FinePolicy strategy").click();
    steps(&mut h, 60);
    let plan = st.lock().store.design.decisions.iter().find(|d| d.id == decision).unwrap().plan.clone().unwrap();
    assert_eq!(st.lock().store.design.proposals.iter().find(|p| p.id == plan).unwrap().option.as_deref(), Some("B"));
    assert!(h.state().proposal_canvas.as_ref().is_some_and(|d| d.classes.iter().any(|c| c.name == "FinePolicy")));
    assert!(st.lock().store.design.live_class("FinePolicy").is_none(), "selecting never approves");
    // before / proposed
    h.get_by_label("Before").click();
    steps(&mut h, 20);
    assert!(h.state().proposal_canvas.as_ref().is_some_and(|d| !d.classes.iter().any(|c| c.name == "FinePolicy")));
    h.get_by_label("Proposed").click();
    steps(&mut h, 20);
    // a new arrival doesn't steal the selection
    let selected = h.state().selected_proposal.clone();
    st.lock().store.propose_ops(vec![op(serde_json::json!({"op":"add_class","name":"LateArrival"}))], Some("Late arrival".into()), "agent").unwrap();
    st.changed();
    steps(&mut h, 30);
    assert_eq!(h.state().selected_proposal, selected);
    // approval advances to the next plan in queue order (and doesn't approve it)
    h.get_by_label("Approve design").click();
    steps(&mut h, 60);
    assert!(st.lock().store.design.live_class("FinePolicy").is_some());
    let next_plan = st.lock().store.design.decisions.iter().find(|d| d.id == small).unwrap().plan.clone();
    assert_eq!(h.state().selected_proposal, next_plan);
    assert!(!st.lock().store.design.decisions.iter().find(|d| d.id == small).unwrap().state.eq(&spide_core::model::DecisionState::Accepted));
    // the queue navigates in one click
    h.get_by_label("Review: Late arrival").click();
    steps(&mut h, 30);
    assert_eq!(h.state().selected_proposal.as_deref(), st.lock().store.design.proposals.last().map(|p| p.id.as_str()));
    h.get_by_label("Dismiss").click();
    steps(&mut h, 60);
    h.get_by_label("Approve design").click();
    steps(&mut h, 60);
    assert!(st.lock().store.design.proposals.is_empty());
    h.get_by_label("✓ Review complete");
    h.get_by_label("Check behaviour →");
    assert!(!h.state().review_active, "the canvas is editable again");
    assert!(!h.get_by_label("Decision history →").accesskit_node().is_disabled());
    shot(&mut h, "review-complete");
}

#[test]
fn diagram_picks_become_refinement_targets_and_requests_need_pi() {
    use egui_kittest::kittest::{NodeT, Queryable};
    let dir = project("library");
    let mut h = harness(dir.path(), (1440.0, 900.0));
    steps(&mut h, 60);
    review_fixture(&mut h);
    h.state_mut().open_proposal_review(None);
    steps(&mut h, 200);
    // clicking an element on the diagram during review inspects it and targets it
    let p = class_pos(&h, "method:Loan.fine");
    click(&mut h, p, egui::PointerButton::Primary);
    steps(&mut h, 10);
    let scope = h.state().proposal_ui.scope.clone();
    let r = h.state().proposal_ui.refine.get(&scope).cloned().unwrap_or_default();
    assert_eq!(r.targets, vec!["method:Loan.fine".to_string()]);
    assert_eq!(r.inspect.as_deref(), Some("method:Loan.fine"));
    assert!(h.state().proposal_ui.picked_classes().contains("Loan"));
    h.get_by_label("Loan.fine()");
    h.get_by_label("+ Money");
    shot(&mut h, "review-pick");
    // pi is offline in tests: the request is unavailable with a reason; notes and picks are kept
    h.get_by_label("Notes or requested changes").focus();
    steps(&mut h, 2);
    h.get_by_label("Notes or requested changes").type_text("Name it overdueFine");
    steps(&mut h, 5);
    assert!(h.get_by_label("Request changes").accesskit_node().is_disabled());
    h.get_by_label_contains("Request changes: pi is");
    // clicking it again removes it
    let p = class_pos(&h, "method:Loan.fine");
    click(&mut h, p, egui::PointerButton::Primary);
    steps(&mut h, 10);
    assert!(h.state().proposal_ui.refine.get(&scope).is_some_and(|r| r.targets.is_empty()));
    // narrow window: the rail is a drawer beside pi, actions stay reachable
    let mut narrow = harness(dir.path(), (960.0, 720.0));
    steps(&mut narrow, 60);
    narrow.state_mut().open_proposal_review(None);
    steps(&mut narrow, 90);
    narrow.get_by_label("Approve design");
    shot(&mut narrow, "review-narrow");
}

#[test]
fn revision_requests_carry_the_selection_and_reject_stale_revisions() {
    let dir = project("library");
    let mut h = harness(dir.path(), (1200.0, 800.0));
    let (decision, _) = review_fixture(&mut h);
    let st = h.state().backend.st.clone();
    let s = st.lock();
    let rev = s.store.design.rev;
    let msg = crate::commands::revision_message(&s, &decision, rev, " rename it ", Some("B"), &["Loan [class:Loan]".into()]).unwrap();
    assert!(msg.contains(&format!("decision {decision}")));
    assert!(msg.contains("option shown to the user: B"));
    assert!(msg.contains("- Loan [class:Loan]"));
    assert!(msg.contains("User guidance: rename it"));
    assert!(msg.contains("Do NOT apply anything"));
    assert!(crate::commands::revision_message(&s, &decision, rev - 1, "x", None, &[]).is_err(), "stale revision");
    assert!(crate::commands::revision_message(&s, &decision, rev, "  ", None, &[]).is_err(), "guidance required");
}

#[test]
fn unsaved_manual_edits_block_approval_and_survive() {
    use egui_kittest::kittest::{NodeT, Queryable};
    let dir = project("library");
    let mut h = harness(dir.path(), (1440.0, 900.0));
    steps(&mut h, 60);
    review_fixture(&mut h);
    h.state_mut().open_proposal_review(None);
    steps(&mut h, 90);
    let st = h.state().backend.st.clone();
    // a manual amendment (advanced mode) in the local draft
    {
        let ui = &mut h.state_mut().proposal_ui;
        let mut sess = ui.session.clone().unwrap();
        sess.plan.rationale = Some("my amendment".into());
        ui.drafts.store(sess.clone());
        ui.session = Some(sess);
    }
    steps(&mut h, 20);
    assert!(h.get_by_label("Approve design").accesskit_node().is_disabled());
    h.get_by_label("Approve: Save or discard your manual edits first.");
    h.get_by_label("Save plan");
    assert!(st.lock().store.design.decisions.iter().all(|d| d.is_pending()));
    let scope = h.state().proposal_ui.scope.clone();
    assert!(h.state().proposal_ui.drafts.load(&scope).is_some(), "draft persisted");
}
