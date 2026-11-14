//! Renders the example designs headlessly (layout → scene → SVG → PNG) and checks the result.
use spide_core::metrics::compute;
use spide_core::{Design, Store};
use spide_diagram::class_view::{self, Data, Opts, Routes};
use spide_diagram::insights;
use spide_diagram::layout::{self, GroupSpec, LayoutEdge, LayoutNode};
use spide_diagram::scene::HitShape;
use spide_diagram::visuals::{node_scale, ApproxMeasure};
use std::collections::{HashMap, HashSet};

fn example(name: &str) -> Design {
    let text = std::fs::read_to_string(format!("{}/../../examples/{name}.json", env!("CARGO_MANIFEST_DIR"))).unwrap();
    let mut s = Store::default();
    s.load(Design::from_json(&text).unwrap());
    s.view()
}

fn render(name: &str) -> (spide_diagram::scene::Scene, usize) {
    let view = example(name);
    let metrics = compute(&view, false);
    let m = ApproxMeasure;
    let ext = class_view::external_classes(&view.classes, &view.packages);
    let marks = insights::method_marks(Some(&metrics));
    let members = insights::member_diagnostics(Some(&metrics), &view.classes);
    let boxes = class_view::boxes_for(&m, &view.classes, true, 3, &view.relations, None, &ext, Some(&marks), Some(&members), &HashSet::new());
    let nodes: Vec<LayoutNode> = view
        .classes
        .iter()
        .map(|c| {
            let s = node_scale(metrics.classes.get(&c.name));
            LayoutNode { id: c.name.clone(), w: boxes[&c.name].w * s, h: boxes[&c.name].h * s + 22.0, group: c.package.clone() }
        })
        .collect();
    let edges: Vec<LayoutEdge> = view.relations.iter().map(|r| LayoutEdge { from: r.from.clone(), to: r.to.clone(), kind: r.kind.as_str().into() }).collect();
    let groups: Vec<GroupSpec> = view.packages.iter().map(|p| GroupSpec { id: p.name.clone(), rank: p.layer.map(|l| l.rank()).unwrap_or(1), external: p.external }).collect();
    let pos: HashMap<String, [f64; 2]> = layout::compound(&nodes, &edges, &groups);
    let d = Data { classes: &view.classes, relations: &view.relations, packages: &view.packages, metrics: Some(&metrics), implicit: &metrics.implicit_edges };
    let o = Opts { pos, boxes, show_methods: true, still: true, level: 3, zoom_cap: 4, marks: Some(&marks), members: Some(&members), ..Default::default() };
    let scene = class_view::build(&m, &d, &o, &mut Routes::default());
    let svg = spide_diagram::svg::scene_svg(&scene, &m).unwrap();
    let out = format!("{}/../../target/scene-out", env!("CARGO_MANIFEST_DIR"));
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(format!("{out}/{name}.svg"), &svg).unwrap();
    let png = spide_core::png::render(&svg, 1.0).unwrap();
    std::fs::write(format!("{out}/{name}.png"), png).unwrap();
    (scene, view.classes.len())
}

#[test]
fn examples_render_with_a_hit_per_class() {
    for name in ["library", "rest-extraction"] {
        let (scene, classes) = render(name);
        let class_hits = scene.hits.iter().filter(|h| h.hover.as_deref().is_some_and(|k| k.starts_with("class:")) && matches!(h.shape, HitShape::Rect { .. }) && h.node.is_some()).count();
        assert!(class_hits >= classes, "{name}: {class_hits} < {classes}");
    }
}
