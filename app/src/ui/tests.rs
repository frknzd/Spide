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
