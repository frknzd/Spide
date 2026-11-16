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
    let src = format!("{}/../tests/fixtures/{example}.json", env!("CARGO_MANIFEST_DIR"));
    std::fs::copy(src, dir.path().join("design.json")).unwrap();
    dir
}

pub fn harness(dir: &std::path::Path, size: (f32, f32)) -> Harness<'static, SpideApp> {
    let (events, rx) = Events::new();
    let st = if crate::project::design_path(dir).exists() {
        AppState::new(events.clone(), dir.to_path_buf())
    } else {
        AppState::create(events.clone(), dir.to_path_buf())
    };
    harness_with(st, events, rx, size)
}

pub fn harness_with(st: AppState, events: Events, rx: std::sync::mpsc::Receiver<crate::events::UiEvent>, size: (f32, f32)) -> Harness<'static, SpideApp> {
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

/// A ⌘-shortcut as the platform delivers it: on macOS the native menu item owns the key (the
/// raw key event reaches egui only when the item is disabled), elsewhere egui's key handler.
pub fn shortcut(h: &mut Harness<'static, SpideApp>, m: egui::Modifiers, key: egui::Key) {
    if cfg!(target_os = "macos") {
        let id = super::actions::by_shortcut(key, m).expect("registered shortcut");
        h.state_mut().perform(id);
    } else {
        h.key_press_modifiers(m, key);
    }
}

#[test]
fn workspace_shortcuts_and_narrow_drawer() {
    let dir = project("rest-extraction");
    let mut h = harness(dir.path(), (1440.0, 900.0));
    steps(&mut h, 150);
    let cmd = egui::Modifiers::COMMAND;
    for (key, tab) in [(egui::Key::Num1, Tab::System), (egui::Key::Num2, Tab::Class), (egui::Key::Num3, Tab::Domain), (egui::Key::Num4, Tab::Package), (egui::Key::Num5, Tab::Usecase), (egui::Key::Num6, Tab::Sequence)] {
        shortcut(&mut h, cmd, key);
        steps(&mut h, 5);
        assert_eq!(h.state().tab, tab);
    }
    if cfg!(target_os = "macos") {
        // exactly once: the raw key alone (menu already handled it) does nothing here
        h.key_press_modifiers(cmd, egui::Key::Num1);
        steps(&mut h, 5);
        assert_eq!(h.state().tab, Tab::Sequence);
    }
    // a view switch is navigation: Back returns
    shortcut(&mut h, cmd, egui::Key::OpenBracket);
    steps(&mut h, 5);
    assert_eq!(h.state().tab, Tab::Usecase);
    shortcut(&mut h, cmd, egui::Key::CloseBracket);
    steps(&mut h, 5);
    assert_eq!(h.state().tab, Tab::Sequence);
    shortcut(&mut h, cmd, egui::Key::B);
    steps(&mut h, 5);
    assert!(h.state().brief);
    shot(&mut h, "workspace-brief");
    shortcut(&mut h, cmd, egui::Key::B);
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
fn panels_compare_and_timeline() {
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
    // right-click → selects and opens the context menu; nothing is removed
    use egui_kittest::kittest::Queryable;
    let status = |h: &Harness<'static, SpideApp>, n: &str| h.state().backend.st.lock().store.design.classes.iter().find(|c| c.name == n).map(|c| c.status);
    let p = class_pos(&h, "class:NotificationLog");
    click(&mut h, p, egui::PointerButton::Secondary);
    steps(&mut h, 10);
    assert!(h.state().context.is_some(), "context menu opens");
    assert_eq!(h.state().selected_target(), Some(spide_core::Target::Class { name: "NotificationLog".into() }));
    assert_eq!(status(&h, "NotificationLog"), Some(spide_core::model::Status::Accepted));
    shot(&mut h, "interaction-context-menu");
    assert!(h.query_by_label_contains("Remove from Design").is_some());
    assert!(h.query_by_label_contains("Edit Properties").is_some());
    h.key_press(egui::Key::Escape);
    steps(&mut h, 5);
    assert!(h.state().context.is_none());
    // Delete prunes the selection (reversible) and asks what you didn't like
    h.key_press(egui::Key::Delete);
    steps(&mut h, 10);
    assert_eq!(status(&h, "NotificationLog"), Some(spide_core::model::Status::Pruned));
    assert!(h.state().prune.is_some(), "feedback popover opens");
    shot(&mut h, "interaction-prune");
    h.key_press(egui::Key::Escape);
    steps(&mut h, 5);
    assert!(h.state().prune.is_none());
    // ⌥-right-click: the fast prune gesture
    steps(&mut h, 30);
    let p = class_pos(&h, "class:BookCopy");
    let alt = egui::Modifiers { alt: true, ..Default::default() };
    h.event(egui::Event::ModifiersChanged(alt));
    h.event(egui::Event::PointerMoved(p));
    h.event(egui::Event::PointerButton { pos: p, button: egui::PointerButton::Secondary, pressed: true, modifiers: alt });
    h.step();
    h.event(egui::Event::PointerButton { pos: p, button: egui::PointerButton::Secondary, pressed: false, modifiers: alt });
    h.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
    steps(&mut h, 10);
    assert_eq!(status(&h, "BookCopy"), Some(spide_core::model::Status::Pruned));
    h.key_press(egui::Key::Escape);
    steps(&mut h, 5);
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

#[test]
fn domain_tab_draws_the_stored_model_and_prunes_only_concepts() {
    use super::canvas::CanvasOut;
    use spide_core::{Status, Target};
    let dir = project("library");
    let mut h = harness(dir.path(), (1440.0, 900.0));
    let st = h.state().backend.st.clone();
    steps(&mut h, 60);
    h.state_mut().tab = Tab::Domain;
    steps(&mut h, 200);
    shot(&mut h, "library-domain-stored");
    // the canvas reports classes; on the Domain tab they become concepts, and renames/moves are dropped
    let Some(CanvasOut::Prune(ts, _)) = super::app::domain_canvas_out(CanvasOut::Prune(vec![Target::Class { name: "Loan".into() }, Target::Attribute { class: "Loan".into(), name: "due".into() }], egui::Pos2::ZERO)) else { panic!("prune kept") };
    assert_eq!(ts, vec![Target::Concept { name: "Loan".into() }]);
    assert!(super::app::domain_canvas_out(CanvasOut::DoubleClick(Target::Class { name: "Loan".into() }, egui::Pos2::ZERO)).is_none());
    crate::commands::prune(&st, ts).unwrap();
    let s = st.lock();
    assert_eq!(s.store.design.class("Loan").unwrap().status, Status::Accepted, "the design class with the same name is untouched");
    assert_eq!(s.store.design.domain.concepts.iter().find(|c| c.name == "Loan").unwrap().status, Status::Pruned);
}

#[test]
fn domain_tab_without_a_model_offers_to_generate_it() {
    use egui_kittest::kittest::Queryable;
    let dir = project("rest-extraction");
    let mut h = harness(dir.path(), (1440.0, 900.0));
    steps(&mut h, 60);
    h.state_mut().tab = Tab::Domain;
    steps(&mut h, 30);
    assert!(h.query_by_label("No domain model yet.").is_some());
    assert!(h.query_by_label("↻ generate").is_some());
    shot(&mut h, "rest-extraction-domain-empty");
}


fn copy_dir(from: &std::path::Path, to: &std::path::Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            copy_dir(&p, &to.join(e.file_name()));
        } else {
            std::fs::copy(&p, to.join(e.file_name())).unwrap();
        }
    }
}

/// A Spide Project: the library fixture's Java code with the library example design.
fn library_project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    copy_dir(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../crates/spide-import/tests/fixtures/library/src"), &dir.path().join("src"));
    let design: spide_core::Design = serde_json::from_str(&std::fs::read_to_string(format!("{}/../tests/fixtures/library.json", env!("CARGO_MANIFEST_DIR"))).unwrap()).unwrap();
    let m = spide_core::project::Manifest { name: "Library".into(), code_root: Some("org.acme.library".into()), ..Default::default() };
    crate::project::create(dir.path(), &m, &design, None, false).unwrap();
    dir
}

fn wait_indexed(h: &mut Harness<'static, SpideApp>) {
    crate::project::refresh_async(&h.state().backend.st);
    for _ in 0..300 {
        h.step();
        let ok = h.state().snap.as_ref().and_then(|s| s.project.as_ref()).is_some_and(|p| p.report.is_some() && p.index == Some(crate::project::IndexStatus::Current));
        if ok {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("project never indexed");
}

#[test]
fn project_rail_design_only_and_form() {
    use egui_kittest::kittest::Queryable;
    let dir = project("library");
    let mut h = harness(dir.path(), (1440.0, 900.0));
    steps(&mut h, 30);
    h.state_mut().command("/project");
    steps(&mut h, 20);
    assert!(h.state().project_open);
    assert!(h.query_by_label("Create Spide Project in a code folder…").is_some());
    shot(&mut h, "project-design-only");
    let module = library_project();
    let m = crate::project::suggest_manifest(module.path(), "Library");
    h.state_mut().project_ui.form = Some(super::project::Form::new(module.path().to_path_buf(), &m, false));
    steps(&mut h, 10);
    assert!(h.query_by_label("New Spide Project").is_some());
    shot(&mut h, "project-form");
}

#[test]
fn project_rail_sync_and_lens() {
    use egui_kittest::kittest::Queryable;
    let module = library_project();
    let mut h = harness(module.path(), (1440.0, 900.0));
    wait_indexed(&mut h);
    h.state_mut().command("/project");
    steps(&mut h, 30);
    let snap = h.state().snap.clone().unwrap();
    let r = snap.project.as_ref().unwrap().report.as_ref().unwrap();
    // the first agreement created the baseline: what only the design has is ahead of the code and vice versa
    assert!(r.count(spide_core::sync::SyncStatus::DesignAhead) > 0 && r.count(spide_core::sync::SyncStatus::CodeAhead) > 0, "{:?}", r.counts);
    assert!(h.query_by_label_contains("Implement in code").is_some());
    shot(&mut h, "project-sync");
    // the sync lens draws statuses and code-only ghosts on the class diagram
    h.state_mut().command("/lens sync");
    steps(&mut h, 200);
    assert_eq!(h.state().lens, spide_diagram::class_view::Lens::Sync);
    let phantoms = snap.project.as_ref().unwrap().phantoms.len();
    assert!(phantoms > 0);
    shot(&mut h, "project-sync-lens");
}

#[test]
fn uml_sequence_with_fragments_creation_destruction_and_refs() {
    use spide_core::model::*;
    let dir = project("library");
    let path = dir.path().join("design.json");
    let mut d: Design = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    d.use_cases = UseCaseModel { actors: vec![Actor { name: "Member".into(), ..Default::default() }], ..Default::default() };
    let s = spide_core::sequence::from_input(&serde_json::json!({
        "name": "Borrow a book",
        "lifelines": [{"name": "Member"}, {"name": "ctl", "class": "LoanController"}, {"name": "svc", "class": "LendingService"},
                      {"name": "cat", "class": "Catalog"}, {"name": "loan", "class": "Loan"}, {"name": "repo", "class": "LoanRepository"}],
        "steps": [
            {"from": "Member", "to": "ctl", "method": "borrow", "args": "id, isbn", "returns": "loan"},
            {"from": "ctl", "to": "svc", "method": "lend", "args": "m, copy", "returns": "loan"},
            {"from": "svc", "to": "cat", "method": "find", "args": "isbn", "returns": "book"},
            {"fragment": "loop", "guard": "for each open loan of m", "steps": [
                {"fragment": "alt", "operands": [
                    {"guard": "overdue", "steps": [{"from": "svc", "to": "repo", "method": "findByMember", "args": "m", "returns": "loans"}]},
                    {"guard": "else", "steps": [{"from": "svc", "to": "repo", "method": "save", "args": "l", "kind": "async"}]}]}]},
            {"fragment": "opt", "guard": "copy available", "steps": [
                {"from": "svc", "to": "loan", "method": "Loan", "args": "m, copy", "kind": "create"},
                {"from": "svc", "to": "repo", "method": "save", "args": "loan"}]},
            {"fragment": "par", "operands": [
                {"guard": "notify", "steps": [{"from": "svc", "to": "repo", "method": "save", "args": "loan", "kind": "async"}]},
                {"guard": "audit", "steps": [{"from": "svc", "to": "svc", "method": "giveBack", "args": "loan"}]}]},
            {"ref": "Pay a fine", "over": ["Member", "ctl"]},
            {"from": "svc", "to": "loan", "kind": "destroy"}]
    }))
    .unwrap();
    d.sequences.push(s);
    d.sequences.push(Sequence { name: "Pay a fine".into(), participants: vec!["Member".into()], ..Default::default() });
    std::fs::write(path, serde_json::to_vec_pretty(&d).unwrap()).unwrap();
    let mut h = harness(dir.path(), (1440.0, 900.0));
    h.state_mut().tab = Tab::Sequence;
    h.state_mut().scenario = Some("Borrow a book".into());
    steps(&mut h, 150);
    let snap = h.state().snap.clone().unwrap();
    let s = &snap.view.sequences[0];
    assert!(s.messages.iter().all(|m| m.problem.is_none()), "{:?}", s.messages.iter().filter_map(|m| m.problem.clone()).collect::<Vec<_>>());
    // frames, the created header, the ✗ and implied replies are in the scene
    let scene = spide_diagram::views::sequence_scene(&h.state().measure, s, &|n| n == "Member").scene;
    let texts = format!("{:?}", scene.root);
    for t in ["\"loop\"", "\"alt\"", "\"opt\"", "\"par\"", "\"ref\"", "[for each open loan of m]", "[else]", "loan : Loan", ": Catalog", "Pay a fine", "«create»"] {
        assert!(texts.contains(t), "{t} drawn");
    }
    shot(&mut h, "sequence-uml");
    // the ref frame opens the referenced sequence
    let ref_index = s.messages.iter().position(|m| m.kind == MessageKind::Ref).unwrap();
    h.state_mut().on_click_target(spide_core::Target::Message { sequence: s.name.clone(), index: ref_index });
    assert_eq!(h.state().scenario.as_deref(), Some("Pay a fine"));
    // the same scene as an export image (readable size) for visual review
    let meta = spide_diagram::svg::ExportMeta { title: "Library".into(), view: "Sequence".into(), health: None, legend: vec![], date: String::new() };
    let o = spide_diagram::svg::ExportOptions { theme: "dark".into(), ..Default::default() };
    let (svg, _, _) = spide_diagram::svg::build_export_svg(&scene, &h.state().measure, &meta, &o).unwrap();
    let png = spide_core::png::render(&svg, 1.5).unwrap();
    std::fs::write(format!("{}/../target/ui-shots/sequence-uml-export.png", env!("CARGO_MANIFEST_DIR")), png).unwrap();
}

#[test]
fn failed_save_banner_survives_toast_expiry_and_retry_clears_it() {
    use egui_kittest::kittest::Queryable;
    let dir = project("library");
    let mut h = harness(dir.path(), (1440.0, 900.0));
    steps(&mut h, 2);
    let st = h.state().backend.st.clone();
    std::fs::create_dir(dir.path().join("design.json.tmp")).unwrap();
    assert!(crate::commands::set_name(&st, "Work to keep".into()).is_err());
    steps(&mut h, 500);
    assert!(h.state().snap.as_ref().unwrap().save_status.error.is_some());
    h.get_by_label("Retry save");
    h.get_by_label("Save Design Copy…");
    std::fs::remove_dir(dir.path().join("design.json.tmp")).unwrap();
    h.get_by_label("Retry save").click();
    steps(&mut h, 2);
    assert!(h.state().snap.as_ref().unwrap().save_status.error.is_none());
}

#[test]
fn leaving_requires_resolving_failed_saves_unsent_text_and_busy_agent() {
    use super::leave::{LeaveIssue, LeaveReason};
    let dir = project("library");
    let mut h = harness(dir.path(), (1440.0, 900.0));
    steps(&mut h, 2);
    let st = h.state().backend.st.clone();
    assert!(h.state().leave_issues().is_empty());
    assert!(h.state_mut().confirm_leave(LeaveReason::CloseProject));

    // failed save: cancel keeps us here; retry after the problem is fixed lets us leave
    std::fs::create_dir(dir.path().join("design.json.tmp")).unwrap();
    assert!(crate::commands::set_name(&st, "Keep me".into()).is_err());
    assert!(matches!(h.state().leave_issues()[0], LeaveIssue::SaveFailed(_)));
    h.state_mut().leave_answers = Some([None].into());
    assert!(!h.state_mut().confirm_leave(LeaveReason::Quit));
    h.state_mut().leave_answers = Some([Some(0)].into());
    assert!(!h.state_mut().confirm_leave(LeaveReason::Quit), "retry still fails");
    // save a copy instead: allowed to leave, the copy holds the unsaved work
    let copy = dir.path().join("copy.json");
    h.state_mut().leave_copy_path = Some(copy.display().to_string());
    h.state_mut().leave_answers = Some([Some(1)].into());
    assert!(h.state_mut().confirm_leave(LeaveReason::Quit));
    assert!(std::fs::read_to_string(&copy).unwrap().contains("Keep me"));
    std::fs::remove_dir(dir.path().join("design.json.tmp")).unwrap();
    h.state_mut().leave_answers = Some([Some(0)].into());
    assert!(h.state_mut().confirm_leave(LeaveReason::SwitchProject));
    assert!(h.state().leave_issues().is_empty());

    // unsent rename text is only discarded explicitly
    h.state_mut().inline = Some(super::overlays::InlineEdit { at: egui::Pos2::ZERO, label: "Rename".into(), original: "Book".into(), value: "Volume".into(), focused: true, kind: super::overlays::InlineKind::RenameClass("Book".into()) });
    h.state_mut().leave_answers = Some([None].into());
    assert!(!h.state_mut().confirm_leave(LeaveReason::CloseWindow));
    assert!(h.state().inline.is_some());
    h.state_mut().leave_answers = Some([Some(1)].into()); // explicitly discard rather than keep locally
    assert!(h.state_mut().confirm_leave(LeaveReason::CloseWindow));
    assert!(h.state().inline.is_none());

    // a busy agent needs an explicit stop
    st.lock().pi_status = crate::state::PiStatus::Busy;
    assert_eq!(h.state().leave_issues(), vec![LeaveIssue::AgentBusy]);
    h.state_mut().leave_answers = Some([None].into());
    assert!(!h.state_mut().confirm_leave(LeaveReason::SwitchProject));
    h.state_mut().leave_answers = Some([Some(0)].into());
    assert!(h.state_mut().confirm_leave(LeaveReason::SwitchProject));
}

#[test]
fn welcome_new_close_recent_and_reopen_without_terminal_commands() {
    use egui_kittest::kittest::Queryable;
    let (events, rx) = Events::new();
    let st = AppState::closed(events.clone());
    let mut h = harness_with(st.clone(), events, rx, (1440.0, 900.0));
    steps(&mut h, 3);
    // nothing open: the welcome screen, no lock, no service
    h.get_by_label("New Project…");
    h.get_by_label("Recent Projects");
    assert!(crate::service::get(&st).is_none());
    shot(&mut h, "welcome-empty");

    // New Project → a blank design workspace in a new folder
    let root = tempfile::tempdir().unwrap();
    let mut f = super::welcome::NewProject::new();
    f.name = "Shop".into();
    f.location = root.path().display().to_string();
    f.validate().unwrap();
    h.state_mut().create_new_project(f.clone());
    steps(&mut h, 3);
    let target = root.path().join("Shop").canonicalize().unwrap();
    assert_eq!(st.lock().workspace_status, crate::state::WorkspaceStatus::Ready);
    assert_eq!(st.lock().project_dir, target);
    assert_eq!(st.lock().store.design.name, "Shop");
    crate::commands::apply_user_ops(&st, serde_json::from_str(r#"[{"op":"add_class","name":"Order"}]"#).unwrap()).unwrap();
    // the folder is now occupied: New refuses it rather than opening/overwriting
    assert!(f.validate().is_err());

    // Close Project → welcome; the lock is released; the project is in Recent Projects
    h.state_mut().close_project();
    steps(&mut h, 3);
    assert_eq!(st.lock().workspace_status, crate::state::WorkspaceStatus::Closed);
    h.get_by_label("New Project…");
    assert!(crate::state::acquire_lock(&target).is_some(), "writer lock released on close");
    let recent = h.state().prefs.recent.clone();
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].path, target.display().to_string());
    assert_eq!(h.state().prefs.last_project, None, "a manual close is honoured at startup");
    h.get_by_label("Shop");
    h.state_mut().new_project_dialog();
    steps(&mut h, 3);
    shot(&mut h, "welcome-recent-new-dialog");
    h.state_mut().new_project = None;

    // reopen it from Recent Projects: the edit is there
    h.state_mut().open_recent(recent[0].path.clone());
    steps(&mut h, 3);
    assert_eq!(st.lock().project_dir, target);
    assert!(st.lock().store.design.classes.iter().any(|c| c.name == "Order"));

    // a failed open from the welcome screen keeps the welcome screen and says why
    h.state_mut().close_project();
    steps(&mut h, 2);
    let broken = tempfile::tempdir().unwrap();
    std::fs::write(broken.path().join("design.json"), "nope").unwrap();
    let d = broken.path().display().to_string();
    h.state_mut().backend.transition(move |s, pi| crate::commands::open_folder(s, pi, d), |app, r| if let Err(e) = r { app.open_error = Some(e) });
    steps(&mut h, 3);
    assert_eq!(st.lock().workspace_status, crate::state::WorkspaceStatus::Closed);
    assert!(h.state().open_error.is_some());
    assert_eq!(std::fs::read_to_string(broken.path().join("design.json")).unwrap(), "nope");
}

#[test]
fn edit_actions_follow_focus_and_unavailable_actions_explain_why() {
    use super::actions::{ActionId, EditFocus};
    let dir = project("library");
    let mut h = harness(dir.path(), (1440.0, 900.0));
    steps(&mut h, 60);
    let st = h.state().backend.st.clone();
    assert_eq!(h.state().action_state(ActionId::Undo).reason, Some("Nothing to undo"));
    let has = |st: &AppState| st.lock().store.design.classes.iter().any(|c| c.name == "Undoable");
    crate::commands::apply_user_ops(&st, serde_json::from_str(r#"[{"op":"add_class","name":"Undoable"}]"#).unwrap()).unwrap();
    steps(&mut h, 3);
    assert!(h.state().action_state(ActionId::Undo).enabled);

    // a text field has focus: Undo belongs to it, the design is untouched
    h.state_mut().palette = Some(super::overlays::Palette::new("abc"));
    steps(&mut h, 3);
    assert_eq!(h.state().edit_focus(), EditFocus::Text);
    h.state_mut().perform(ActionId::Undo);
    steps(&mut h, 3);
    assert!(has(&st));

    // the canvas has focus: Undo is design undo
    h.state_mut().palette = None;
    steps(&mut h, 3);
    assert_eq!(h.state().edit_focus(), EditFocus::Design);
    h.state_mut().perform(ActionId::Undo);
    steps(&mut h, 3);
    assert!(!has(&st));

    // Find in Design jumps to a member on the class diagram and Back returns
    h.state_mut().go_to_tab(Tab::Package);
    let item = super::search::index(&st.lock().store.design).into_iter().find(|i| i.kind == "Members").unwrap();
    h.state_mut().go_to_item(&item);
    steps(&mut h, 30);
    assert_eq!(h.state().tab, Tab::Class);
    assert_eq!(h.state().selected_target(), Some(item.target.clone()));
    h.state_mut().perform(ActionId::Back);
    steps(&mut h, 5);
    assert_eq!(h.state().tab, Tab::Package);

    // nothing open: project actions say why
    h.state_mut().close_project();
    steps(&mut h, 3);
    assert_eq!(h.state().action_state(ActionId::Save).reason, Some("No project is open"));
    assert!(h.state().action_state(ActionId::NewProject).enabled);
    assert_eq!(h.state().action_state(ActionId::View(Tab::Class)).checked, Some(false));
}

#[test]
fn find_in_design_lists_grouped_results_and_goes_to_them_on_any_view() {
    use egui_kittest::kittest::Queryable;
    let dir = project("library");
    let mut h = harness(dir.path(), (1440.0, 900.0));
    steps(&mut h, 60);
    h.state_mut().perform(super::actions::ActionId::Find);
    steps(&mut h, 3);
    h.state_mut().find.as_mut().unwrap().query = "loan".into();
    steps(&mut h, 3);
    h.get_by_label("Classes");
    shot(&mut h, "find-in-design");
    // no results
    h.state_mut().find.as_mut().unwrap().query = "zzzzqqq".into();
    steps(&mut h, 3);
    h.get_by_label("No matches. Try including pruned or proposed elements.");
    // Escape closes the topmost overlay only
    h.state_mut().brief = true;
    h.key_press(egui::Key::Escape);
    steps(&mut h, 3);
    assert!(h.state().find.is_none());
    assert!(h.state().brief, "Escape closed Find, not the brief behind it");
    // a package on the package diagram
    let st = h.state().backend.st.clone();
    let pkg = super::search::index(&st.lock().store.design).into_iter().find(|i| i.kind == "Packages").unwrap();
    h.state_mut().go_to_item(&pkg);
    steps(&mut h, 40);
    assert_eq!(h.state().tab, Tab::Package);
    // Go to Element: the outline lists everything before typing
    h.state_mut().perform(super::actions::ActionId::Outline);
    steps(&mut h, 3);
    assert!(h.state().find.as_ref().unwrap().outline);
    shot(&mut h, "go-to-element");
}

#[test]
fn branch_chooser_creates_switches_and_guards_unsaved_plan_drafts() {
    use egui_kittest::kittest::Queryable;
    let dir = project("library");
    let mut h = harness(dir.path(), (1440.0, 900.0));
    steps(&mut h, 30);
    let st = h.state().backend.st.clone();
    h.state_mut().perform(super::actions::ActionId::Branches);
    steps(&mut h, 5);
    h.get_by_label("Create from current");
    shot(&mut h, "branches");
    // create "experiment" through the chooser's own path (it switches to the new branch)
    crate::commands::branch_create(&st, "experiment".into()).unwrap();
    assert_eq!(st.lock().branch, "experiment");
    // a busy agent: switching back asks first; cancelling keeps the branch
    st.lock().pi_status = crate::state::PiStatus::Busy;
    h.state_mut().leave_answers = Some([None].into());
    h.state_mut().command("/switch main");
    steps(&mut h, 5);
    assert_eq!(st.lock().branch, "experiment");
    st.lock().pi_status = crate::state::PiStatus::Idle;
    h.state_mut().leave_answers = None;
    h.state_mut().command("/switch main");
    steps(&mut h, 5);
    assert_eq!(st.lock().branch, "main");
}

#[test]
fn copy_paste_duplicate_and_properties_without_an_agent() {
    use super::actions::ActionId;
    let dir = project("library");
    let mut h = harness(dir.path(), (1440.0, 900.0));
    steps(&mut h, 60);
    let st = h.state().backend.st.clone();
    let has = |n: &str| st.lock().store.design.classes.iter().any(|c| c.name == n && c.status == spide_core::model::Status::Accepted);
    // New Class… through the form: one change, undoable
    h.state_mut().perform(ActionId::NewClass);
    steps(&mut h, 3);
    {
        let ed = h.state_mut().class_editor.as_mut().unwrap();
        ed.name = "Invoice".into();
        ed.attributes.push(super::properties::AttrRow { name: "total".into(), ty: "Money".into(), ..Default::default() });
    }
    let ops = h.state().class_editor.as_ref().unwrap().ops().unwrap();
    crate::commands::apply_user_ops(&st, ops).unwrap();
    h.state_mut().class_editor = None;
    steps(&mut h, 3);
    assert!(has("Invoice"));
    // Duplicate (⌘D) keeps the clipboard alone; Copy + Paste pastes again with a fresh name
    h.state_mut().selection = vec![spide_core::Target::Class { name: "Invoice".into() }];
    h.state_mut().perform(ActionId::Duplicate);
    steps(&mut h, 3);
    assert!(has("InvoiceCopy"));
    assert!(h.state().app_clipboard.is_none());
    h.state_mut().selection = vec![spide_core::Target::Class { name: "Invoice".into() }];
    h.state_mut().perform(ActionId::Copy);
    h.state_mut().perform(ActionId::Paste);
    steps(&mut h, 3);
    assert!(has("InvoiceCopy2"));
    // Edit Properties… of the selected class
    h.state_mut().selection = vec![spide_core::Target::Class { name: "InvoiceCopy2".into() }];
    h.state_mut().perform(ActionId::EditProperties);
    steps(&mut h, 3);
    assert_eq!(h.state().class_editor.as_ref().unwrap().name, "InvoiceCopy2");
    shot(&mut h, "class-properties");
    h.state_mut().class_editor = None;
    // Cut removes only after the copy, and ⌘Z puts it back
    h.state_mut().selection = vec![spide_core::Target::Class { name: "InvoiceCopy".into() }];
    h.state_mut().perform(ActionId::Cut);
    steps(&mut h, 3);
    assert!(!has("InvoiceCopy"));
    h.state_mut().perform(ActionId::Undo);
    steps(&mut h, 3);
    assert!(has("InvoiceCopy"));
    // a package can't be copied: the action says why
    h.state_mut().selection = vec![spide_core::Target::Package { name: "domain".into() }];
    assert_eq!(h.state().action_state(ActionId::Copy).reason, Some("Only classes and their members can be copied"));
}
