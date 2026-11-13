//! How long the snapshot's metric work takes on the 26-class fixture (printed; asserts a loose bound).
use spide_core::*;

#[test]
fn snapshot_metrics_timing() {
    let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/rest-extraction.json");
    let d = Design::from_json(&std::fs::read_to_string(p).unwrap()).unwrap();
    let t = std::time::Instant::now();
    let _ = metrics::compute(&d, false);
    let one = t.elapsed();
    let t = std::time::Instant::now();
    for _ in 0..5 {
        let _ = metrics::compute(&d, false);
    }
    println!("compute: first {one:?}, then {:?} each", t.elapsed() / 5);
    let t = std::time::Instant::now();
    let _ = responsibility::analyse(&d);
    println!("responsibility {:?}", t.elapsed());
    let t = std::time::Instant::now();
    let _ = overlap::analyse(&d, 0.5);
    println!("overlap {:?}", t.elapsed());
    let t = std::time::Instant::now();
    let _ = course::check(&d, false, &responsibility::analyse(&d));
    println!("course {:?}", t.elapsed());
    let t = std::time::Instant::now();
    let _ = serde_json::to_string(&d).unwrap();
    println!("serialize {:?}", t.elapsed());
    let limit = if cfg!(debug_assertions) { 60 } else { 10 };
    assert!(one.as_millis() < limit, "compute took {one:?}");
}

#[test]
fn snapshot_like_work_timing() {
    let p = std::env::var("SPIDE_TIMING_DESIGN").unwrap_or_else(|_| format!("{}/../../examples/rest-extraction.json", env!("CARGO_MANIFEST_DIR")));
    let d = Design::from_json(&std::fs::read_to_string(p).unwrap()).unwrap();
    let store = Store::new(d);
    let t = std::time::Instant::now();
    let view = store.view();
    println!("view {:?}", t.elapsed());
    let t = std::time::Instant::now();
    let m = metrics::compute(&view, false);
    println!("compute view {:?}", t.elapsed());
    for dec in store.design.decisions.iter().filter(|x| x.is_pending()) {
        for o in &dec.options {
            let t = std::time::Instant::now();
            let _ = impact::impact_of_ops(&store.design, &o.ops);
            println!("impact {:?}", t.elapsed());
            let t = std::time::Instant::now();
            if let Ok(p) = store.preview(&o.ops, "x") {
                let _ = metrics::compute(&p, true);
            }
            println!("preview {:?}", t.elapsed());
        }
    }
    let t = std::time::Instant::now();
    let _ = derive::derive(&view, &m);
    println!("derive {:?}", t.elapsed());
}
