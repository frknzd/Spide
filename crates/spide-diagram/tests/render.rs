//! Renders the example designs headlessly (layout → scene → SVG → PNG) and checks the result:
//! complete text, no overlapping classes, routes clear of boxes, a compact near-rectangular drawing.
use spide_core::metrics::{compute, Metrics};
use spide_core::{Design, Store};
use spide_diagram::class_view::{self, Data, Opts, Routes};
use spide_diagram::insights;
use spide_diagram::scene::{HitShape, Prim, Scene};
use spide_diagram::visuals::{ApproxMeasure, ClassBox};
use std::collections::{HashMap, HashSet};

fn example(name: &str) -> Design {
    let text = std::fs::read_to_string(format!("{}/../../examples/{name}.json", env!("CARGO_MANIFEST_DIR"))).unwrap();
    let mut s = Store::default();
    s.load(Design::from_json(&text).unwrap());
    s.view()
}

struct Rendered {
    scene: Scene,
    pos: HashMap<String, [f64; 2]>,
    boxes: HashMap<String, ClassBox>,
    /// layout footprints (box + chip bands), centre-based
    extents: HashMap<String, (f64, f64)>,
    metrics: Metrics,
    routes: Routes,
}

fn render_design(name: &str, view: &Design, level: u8) -> Rendered {
    let metrics = compute(view, false);
    let m = ApproxMeasure;
    let ext = class_view::external_classes(&view.classes, &view.packages);
    let marks = insights::method_marks(Some(&metrics));
    let members = insights::member_diagnostics(Some(&metrics), &view.classes);
    let boxes = class_view::boxes_for(&m, &view.classes, true, level.max(1), &view.relations, None, &ext, Some(&marks), Some(&members), &HashSet::new());
    let extents: HashMap<String, (f64, f64)> = view.classes.iter().map(|c| (c.name.clone(), class_view::node_extent(&m, c, &boxes[&c.name], Some(&metrics)))).collect();
    let t0 = std::time::Instant::now();
    let pos = class_view::layout_classes(&m, &view.classes, &extents, &view.relations, &view.packages);
    let t_layout = t0.elapsed();
    let d = Data { classes: &view.classes, relations: &view.relations, packages: &view.packages, metrics: Some(&metrics), implicit: &metrics.implicit_edges };
    let o = Opts { pos: pos.clone(), boxes: boxes.clone(), show_methods: true, still: true, level: level.max(1), zoom_cap: if level == 0 { 0 } else { 4 }, marks: Some(&marks), members: Some(&members), ..Default::default() };
    let mut routes = Routes::default();
    let t1 = std::time::Instant::now();
    let scene = class_view::build(&m, &d, &o, &mut routes);
    println!("{name}: layout {:.0} ms, routing+scene {:.0} ms", t_layout.as_secs_f64() * 1000.0, t1.elapsed().as_secs_f64() * 1000.0);
    let svg = spide_diagram::svg::scene_svg(&scene, &m).unwrap();
    let out = format!("{}/../../target/scene-out", env!("CARGO_MANIFEST_DIR"));
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(format!("{out}/{name}-{level}.svg"), &svg).unwrap();
    let png = spide_core::png::render(&svg, 1.0).unwrap();
    std::fs::write(format!("{out}/{name}-{level}.png"), png).unwrap();
    Rendered { scene, pos, boxes, extents, metrics, routes }
}

fn texts(p: &Prim, out: &mut Vec<String>) {
    match p {
        Prim::Text { text, .. } => out.push(text.clone()),
        Prim::Group(g) => g.children.iter().for_each(|c| texts(c, out)),
        _ => {}
    }
}

type R4 = [f64; 4];
fn overlap(a: R4, b: R4) -> bool {
    a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3]
}
fn seg_hits(a: [f64; 2], b: [f64; 2], r: R4) -> bool {
    let (x1, x2) = (a[0].min(b[0]), a[0].max(b[0]));
    let (y1, y2) = (a[1].min(b[1]), a[1].max(b[1]));
    x1 < r[2] && x2 > r[0] && y1 < r[3] && y2 > r[1]
}

/// Geometry every presentable class diagram must satisfy, plus its compactness figures.
fn check(name: &str, r: &Rendered) -> (f64, f64) {
    let ids: Vec<&String> = r.pos.keys().collect();
    let foot = |id: &str| {
        let (w, h) = r.extents[id];
        let p = r.pos[id];
        [p[0] - w / 2.0, p[1] - h / 2.0, p[0] + w / 2.0, p[1] + h / 2.0]
    };
    let body = |id: &str| {
        let s = spide_diagram::visuals::node_scale(r.metrics.classes.get(id));
        let b = &r.boxes[id];
        let p = r.pos[id];
        [p[0] - b.w * s / 2.0, p[1] - b.h * s / 2.0, p[0] + b.w * s / 2.0, p[1] + b.h * s / 2.0]
    };
    // 1. no two classes (with their chips) overlap
    for (i, a) in ids.iter().enumerate() {
        for b in &ids[i + 1..] {
            assert!(!overlap(foot(a), foot(b)), "{name}: {a} overlaps {b}");
        }
    }
    // 2. routes: orthogonal and never through a box other than their own ends' first/last segment
    for (eid, route) in r.routes.rel.routes.iter() {
        let rel = r.metrics.classes.len(); // keep borrowck simple
        let _ = rel;
        let pts = &route.points;
        for (k, w) in pts.windows(2).enumerate() {
            assert!((w[0][0] - w[1][0]).abs() < 0.01 || (w[0][1] - w[1][1]).abs() < 0.01, "{name}: {eid} not orthogonal");
            for id in &ids {
                let mut b = body(id);
                b = [b[0] + 1.0, b[1] + 1.0, b[2] - 1.0, b[3] - 1.0];
                let end = k == 0 || k + 2 == pts.len();
                if end && (seg_hits(w[0], w[0], [b[0] - 3.0, b[1] - 3.0, b[2] + 3.0, b[3] + 3.0])) {
                    continue;
                }
                assert!(!seg_hits(w[0], w[1], b), "{name}: route {eid} passes through {id}");
            }
        }
        assert!(!route.fallback, "{name}: route {eid} is an unvalidated fallback");
    }
    // 3. complete text: nothing cut off with an ellipsis
    let mut ts = vec![];
    texts(&Prim::Group(r.scene.root.clone()), &mut ts);
    assert!(!ts.iter().any(|t| t.ends_with('…')), "{name}: truncated text");
    // 4. compactness: occupied share of the bounding box and aspect ratio
    let all: Vec<R4> = ids.iter().map(|id| foot(id)).collect();
    let bb = all.iter().fold([f64::MAX, f64::MAX, f64::MIN, f64::MIN], |a, r| [a[0].min(r[0]), a[1].min(r[1]), a[2].max(r[2]), a[3].max(r[3])]);
    let area: f64 = all.iter().map(|r| (r[2] - r[0]) * (r[3] - r[1])).sum();
    let (w, h) = (bb[2] - bb[0], bb[3] - bb[1]);
    let density = area / (w * h);
    let aspect = w / h;
    println!("{name}: {w:.0}×{h:.0} aspect {aspect:.2} density {density:.2}");
    (density, aspect)
}

#[test]
fn examples_render_with_a_hit_per_class() {
    for name in ["library", "rest-extraction"] {
        let r = render_design(name, &example(name), 3);
        let class_hits = r.scene.hits.iter().filter(|h| h.hover.as_deref().is_some_and(|k| k.starts_with("class:")) && matches!(h.shape, HitShape::Rect { .. }) && h.node.is_some()).count();
        assert!(class_hits >= r.pos.len(), "{name}: {class_hits} < {}", r.pos.len());
    }
}

#[test]
fn examples_are_presentable_at_every_detail_level() {
    let mut bad = vec![];
    for name in ["library", "rest-extraction"] {
        for level in 1..=4 {
            let r = render_design(name, &example(name), level);
            let (density, aspect) = check(&format!("{name}-{level}"), &r);
            // names-only boxes are small next to the routing channels between them
            let min = if level == 1 { 0.22 } else { 0.3 };
            if density <= min || !(0.5..=2.5).contains(&aspect) {
                bad.push(format!("{name}-{level}: density {density:.2} aspect {aspect:.2}"));
            }
        }
    }
    assert!(bad.is_empty(), "not compact: {bad:?}");
}

fn synthetic(ops: serde_json::Value) -> Design {
    let ops: Vec<spide_core::ops::Op> = serde_json::from_value(ops).unwrap();
    spide_core::ops::apply_all(&Design::default(), &ops, spide_core::ops::Mode::Commit(spide_core::model::Status::Accepted, None)).unwrap()
}

#[test]
fn long_text_wraps_instead_of_truncating() {
    let long = "a".repeat(10) + &" withAVeryLongParameterName: SomeEvenLongerGenericType<Map<String, List<Integer>>>,".repeat(4);
    let d = synthetic(serde_json::json!([
        {"op":"add_class","name":"Wide","responsibilities":["Keeps a very long responsibility sentence that would never fit on a single row of a class box in any diagram at all", "second", "third", "fourth", "fifth", "sixth and more"],
         "methods":[{"name":"run","visibility":"public","params":[{"name":"x","type":long}]}]},
        {"op":"add_class","name":"Other"},
        {"op":"add_relation","from":"Wide","to":"Other","kind":"association"}
    ]));
    for level in [2, 3, 4] {
        let r = render_design(&format!("long-{level}"), &d, level);
        check(&format!("long-{level}"), &r);
        let mut ts = vec![];
        texts(&Prim::Group(r.scene.root.clone()), &mut ts);
        if level == 2 {
            assert!(ts.iter().any(|t| t.contains("sixth")), "all responsibilities shown");
        } else {
            assert!(ts.iter().filter(|t| t.contains("withAVeryLongParameterName")).count() >= 4, "{ts:?}");
        }
    }
}

#[test]
fn disconnected_and_external_classes_stay_compact() {
    let mut ops = vec![
        serde_json::json!({"op":"add_package","name":"ui","layer":"ui"}),
        serde_json::json!({"op":"add_package","name":"domain","layer":"domain"}),
        serde_json::json!({"op":"add_package","name":"javax.swing","external":true}),
    ];
    for i in 0..6 {
        ops.push(serde_json::json!({"op":"add_class","name":format!("View{i}"),"package":"ui"}));
        ops.push(serde_json::json!({"op":"add_class","name":format!("Thing{i}"),"package":"domain"}));
        ops.push(serde_json::json!({"op":"add_relation","from":format!("View{i}"),"to":format!("Thing{i}"),"kind":"dependency"}));
    }
    for i in 0..3 {
        ops.push(serde_json::json!({"op":"add_class","name":format!("JPanel{i}"),"package":"javax.swing"}));
        ops.push(serde_json::json!({"op":"add_relation","from":format!("View{i}"),"to":format!("JPanel{i}"),"kind":"inheritance"}));
    }
    for i in 0..5 {
        ops.push(serde_json::json!({"op":"add_class","name":format!("Loose{i}")}));
    }
    let d = synthetic(serde_json::Value::Array(ops));
    let r = render_design("mixed", &d, 3);
    let (density, aspect) = check("mixed", &r);
    assert!(density > 0.25, "{density}");
    assert!((0.45..=2.6).contains(&aspect), "{aspect}");
    // external classes sit at an edge of the drawing, not in the middle
    let xs: Vec<f64> = r.pos.values().map(|p| p[0]).collect();
    let ys: Vec<f64> = r.pos.values().map(|p| p[1]).collect();
    let (x1, x2) = (xs.iter().cloned().fold(f64::MAX, f64::min), xs.iter().cloned().fold(f64::MIN, f64::max));
    let (y1, y2) = (ys.iter().cloned().fold(f64::MAX, f64::min), ys.iter().cloned().fold(f64::MIN, f64::max));
    for i in 0..3 {
        let p = r.pos[&format!("JPanel{i}")];
        let edge = (p[0] - x1).min(x2 - p[0]) / (x2 - x1) < 0.2 || (p[1] - y1).min(y2 - p[1]) / (y2 - y1) < 0.2;
        assert!(edge, "JPanel{i} not at an edge: {p:?}");
    }
}

#[test]
fn every_uml_relation_kind_routes_cleanly() {
    let d = synthetic(serde_json::json!([
        {"op":"add_class","name":"Shape","is_abstract":true},
        {"op":"add_class","name":"Drawable","stereotype":"interface"},
        {"op":"add_class","name":"Circle"},
        {"op":"add_class","name":"Square"},
        {"op":"add_class","name":"Canvas"},
        {"op":"add_class","name":"Layer"},
        {"op":"add_class","name":"Palette"},
        {"op":"add_class","name":"Renderer"},
        {"op":"add_relation","from":"Circle","to":"Shape","kind":"inheritance"},
        {"op":"add_relation","from":"Square","to":"Shape","kind":"inheritance"},
        {"op":"add_relation","from":"Shape","to":"Drawable","kind":"realization"},
        {"op":"add_relation","from":"Canvas","to":"Layer","kind":"composition","from_mult":"1","to_mult":"1..*","label":"layers"},
        {"op":"add_relation","from":"Layer","to":"Shape","kind":"aggregation","to_mult":"*","label":"shapes"},
        {"op":"add_relation","from":"Canvas","to":"Palette","kind":"association","directed":true,"label":"palette"},
        {"op":"add_relation","from":"Palette","to":"Canvas","kind":"association","label":"owner"},
        {"op":"add_relation","from":"Renderer","to":"Canvas","kind":"dependency"},
        {"op":"add_relation","from":"Renderer","to":"Canvas","kind":"association","label":"target"},
        {"op":"add_relation","from":"Layer","to":"Layer","kind":"association","label":"parent"}
    ]));
    for level in [1, 3] {
        let r = render_design(&format!("uml-{level}"), &d, level);
        check(&format!("uml-{level}"), &r);
        assert!(r.routes.rel.routes.len() >= 10, "every relation is routed");
    }
}
