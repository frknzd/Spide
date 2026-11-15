use super::*;

#[test]
fn hover_cards_reserve_height_even_for_short_content_and_fit_small_canvases() {
    let project = tempfile::tempdir().unwrap();
    let (events, _) = crate::events::Events::new();
    let state = crate::state::AppState::new(events, project.path().into());
    let ops = serde_json::from_str(r#"[{"op":"add_class","name":"Example","attributes":[{"name":"value","type":"int"}],"methods":[{"name":"run"}]}]"#).unwrap();
    state.lock().store.apply_user(ops).unwrap();
    let snap = state.snapshot();
    let cx = CardCtx { snap: &snap, design: &snap.view, metrics: &snap.metrics };
    let ctx = egui::Context::default();
    crate::ui::theme::install_fonts(&ctx);
    crate::ui::theme::install_style(&ctx);
    for height in [800.0, 300.0] {
        let bounds = egui::Rect::from_min_size(egui::pos2(200.0, 50.0), egui::vec2(600.0, height));
        for key in ["class:Example", "method:Example.run", "attribute:Example.value"] {
            let mut card = None;
            for _ in 0..4 {
                let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 1000.0))), ..Default::default() };
                let mut output = ctx.run_pass(input, |ctx| {
                    card = show(ctx, key, Point { x: bounds.right() - 10.0, y: bounds.bottom() - 10.0 }, false, bounds, &cx, &None, &mut vec![]);
                });
                // This geometry-only test has no renderer to consume texture updates.
                output.textures_delta.clear();
            }
            let rect = card.unwrap();
            let expected = 250.0f32.min(height - 16.0);
            assert!((rect.height() - expected).abs() < 2.0, "{key}: expected {expected}, got {}", rect.height());
            assert!(rect.top() >= bounds.top() && rect.bottom() <= bounds.bottom(), "{key}: {rect:?}");
        }
    }
}
