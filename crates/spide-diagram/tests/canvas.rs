//! Obsidian Canvas export: every diagram kind becomes a valid JSON Canvas with the export layout,
//! readable cards, layer colours, no diagnostics and UML meaning spelled out on the edges.
use spide_core::metrics::compute;
use spide_core::model::*;
use spide_core::{Design, Store};
use spide_diagram::canvas::{self, CanvasOptions, JsonCanvas, Source};
use spide_diagram::class_view::{self, Data, Opts, Routes};
use spide_diagram::insights;
use spide_diagram::scene::Scene;
use spide_diagram::views::{self, C4Level};
use spide_diagram::visuals::{layer_color, ApproxMeasure};
use std::collections::{HashMap, HashSet};

fn example(name: &str) -> Design {
    let text = std::fs::read_to_string(format!("{}/../../tests/fixtures/{name}.json", env!("CARGO_MANIFEST_DIR"))).unwrap();
    let mut s = Store::default();
    s.load(Design::from_json(&text).unwrap());
    s.view()
}

fn synthetic(ops: serde_json::Value) -> Design {
    let ops: Vec<spide_core::ops::Op> = serde_json::from_value(ops).unwrap();
    spide_core::ops::apply_all(&Design::default(), &ops, spide_core::ops::Mode::Commit(Status::Accepted, None)).unwrap()
}

/// The class (or domain) scene the way exports build it: settled layout, diagnostics computed but
/// tagged (the canvas must leave them out).
fn class_scene(classes: &[Class], relations: &[Relation], packages: &[Package], design: &Design, level: u8) -> Scene {
    let metrics = compute(design, false);
    let m = ApproxMeasure;
    let ext = class_view::external_classes(classes, packages);
    let marks = insights::method_marks(Some(&metrics));
    let members = insights::member_diagnostics(Some(&metrics), classes);
    let boxes = class_view::boxes_for(&m, classes, true, level.max(1), relations, None, &ext, Some(&marks), Some(&members), &HashSet::new());
    let extents: HashMap<String, (f64, f64)> = classes.iter().map(|c| (c.name.clone(), class_view::node_extent(&m, c, &boxes[&c.name], Some(&metrics)))).collect();
    let pos = class_view::layout_classes(&m, classes, &extents, relations, packages);
    let d = Data { classes, relations, packages, metrics: Some(&metrics), implicit: &metrics.implicit_edges };
    let o = Opts { pos, boxes, show_methods: true, still: true, level: level.max(1), zoom_cap: if level == 0 { 0 } else { 4 }, marks: Some(&marks), members: Some(&members), ..Default::default() };
    class_view::build(&m, &d, &o, &mut Routes::default())
}

fn opts() -> CanvasOptions {
    CanvasOptions { ghosts: false, packages: true, header: None }
}

/// JSON Canvas 1.0 validity: unique ids, edges between existing nodes, valid sides and ends,
/// positive integer sizes; round-trips through JSON.
fn valid(c: &JsonCanvas) {
    let json = c.to_json();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(v["nodes"].is_array() && v["edges"].is_array());
    let mut ids = HashSet::new();
    for n in &c.nodes {
        assert!(ids.insert(n.id.clone()), "duplicate id {}", n.id);
        assert!(["text", "group"].contains(&n.kind));
        assert!(n.width > 0 && n.height > 0, "{} has no size", n.id);
        if n.kind == "text" {
            assert!(n.text.as_ref().is_some_and(|t| !t.trim().is_empty()), "{} is empty", n.id);
        }
        if let Some(col) = &n.color {
            assert!(col.len() == 7 && col.starts_with('#'), "colour {col}");
        }
    }
    for e in &c.edges {
        assert!(ids.insert(e.id.clone()), "duplicate id {}", e.id);
        assert!(c.nodes.iter().any(|n| n.id == e.from_node) && c.nodes.iter().any(|n| n.id == e.to_node), "edge {} dangles", e.id);
        for s in [e.from_side, e.to_side] {
            assert!(["top", "right", "bottom", "left"].contains(&s));
        }
        assert!(["none", "arrow"].contains(&e.to_end) && e.from_end == "none");
    }
}

fn rect(n: &canvas::Node) -> [i64; 4] {
    [n.x, n.y, n.x + n.width, n.y + n.height]
}

fn cards_dont_overlap(c: &JsonCanvas) {
    let cards: Vec<&canvas::Node> = c.nodes.iter().filter(|n| n.kind == "text").collect();
    for (i, a) in cards.iter().enumerate() {
        for b in &cards[i + 1..] {
            let (p, q) = (rect(a), rect(b));
            assert!(!(p[0] < q[2] && q[0] < p[2] && p[1] < q[3] && q[1] < p[3]), "{} overlaps {}", a.id, b.id);
        }
    }
}

fn card<'a>(c: &'a JsonCanvas, id: &str) -> &'a canvas::Node {
    c.nodes.iter().find(|n| n.id == id).unwrap_or_else(|| panic!("no node {id}"))
}

#[test]
fn class_diagram_becomes_cards_groups_and_labelled_edges() {
    let d = example("library");
    let metrics = compute(&d, false);
    let scene = class_scene(&d.classes, &d.relations, &d.packages, &d, 3);
    let src = Source::Classes { classes: &d.classes, relations: &d.relations, packages: &d.packages, package_deps: &metrics.package_deps, conceptual: false };
    let c = canvas::build(&scene, &ApproxMeasure, &src, &opts()).unwrap();
    valid(&c);
    cards_dont_overlap(&c);
    // one card per class, packages as coloured groups (behind the cards)
    for cl in &d.classes {
        let n = card(&c, &format!("class:{}", cl.name));
        assert_eq!(n.kind, "text");
        assert!(n.text.as_ref().unwrap().contains(&format!("**{}**", cl.name)), "{:?}", n.text);
    }
    for p in &d.packages {
        let g = card(&c, &format!("package:{}", p.name));
        assert_eq!(g.kind, "group");
        assert_eq!(g.label.as_deref(), Some(p.name.as_str()));
        assert_eq!(g.color, p.layer.map(|l| layer_color(l).css()));
    }
    let first_card = c.nodes.iter().position(|n| n.kind == "text").unwrap();
    assert!(c.nodes[first_card..].iter().all(|n| n.kind == "text"), "groups come first (z-order)");
    // class colours follow their package's layer
    let lending = card(&c, "class:LendingService");
    assert_eq!(lending.color, Some(layer_color(Layer::Application).css()));
    // members are code, compartments separated, stereotype kept
    let t = lending.text.as_ref().unwrap();
    assert!(t.contains("«service»"), "{t}");
    assert!(t.contains("---"), "{t}");
    assert!(t.contains("`+ lend(member: Member, copy: BookCopy): Loan`"), "{t}");
    // no diagnostics anywhere
    for n in &c.nodes {
        let t = n.text.clone().unwrap_or_default();
        for bad in ["cycle", "rule ✗", "orphan", "layer ↑"] {
            assert!(!t.contains(bad), "{} shows diagnostic {bad}: {t}", n.id);
        }
    }
    // every drawn relation is an edge, with its UML meaning in the label
    for r in &d.relations {
        let e = c.edges.iter().find(|e| e.id == format!("relation:{}", r.id)).unwrap_or_else(|| panic!("no edge {}", r.id));
        assert_eq!(e.from_node, format!("class:{}", r.from));
        assert_eq!(e.to_node, format!("class:{}", r.to));
        match r.kind {
            RelationKind::Composition => assert!(e.label.as_ref().unwrap().starts_with("◆ composed of")),
            RelationKind::Inheritance => assert_eq!(e.to_end, "arrow"),
            _ => {}
        }
    }
    let holds = c.edges.iter().find(|e| e.label.as_deref().is_some_and(|l| l.contains("holds"))).unwrap();
    assert!(holds.label.as_ref().unwrap().contains("→"), "multiplicities: {:?}", holds.label);
    std::fs::create_dir_all(format!("{}/../../target/scene-out", env!("CARGO_MANIFEST_DIR"))).unwrap();
    std::fs::write(format!("{}/../../target/scene-out/library-3.canvas", env!("CARGO_MANIFEST_DIR")), c.to_json()).unwrap();
}

#[test]
fn detail_levels_change_the_cards() {
    let d = example("rest-extraction");
    let metrics = compute(&d, false);
    let src = Source::Classes { classes: &d.classes, relations: &d.relations, packages: &d.packages, package_deps: &metrics.package_deps, conceptual: false };
    let at = |level: u8| {
        let c = canvas::build(&class_scene(&d.classes, &d.relations, &d.packages, &d, level), &ApproxMeasure, &src, &opts()).unwrap();
        valid(&c);
        cards_dont_overlap(&c);
        c
    };
    let (names, full) = (at(1), at(4));
    let len = |c: &JsonCanvas| c.nodes.iter().filter_map(|n| n.text.as_ref()).map(|t| t.len()).sum::<usize>();
    assert!(len(&full) > len(&names) * 2, "full detail shows members: {} vs {}", len(&full), len(&names));
    assert!(full.nodes.iter().filter_map(|n| n.text.as_ref()).any(|t| t.contains('`')));
    // the package level: packages are the cards, joined by their dependencies
    let pk = at(0);
    assert!(pk.nodes.iter().all(|n| n.id.starts_with("package:")), "{:?}", pk.nodes.iter().map(|n| &n.id).collect::<Vec<_>>());
    assert!(pk.nodes.iter().all(|n| n.kind == "text"));
    assert!(!pk.edges.is_empty() && pk.edges.iter().all(|e| e.label.as_deref() == Some("depends on") && e.to_end == "arrow"));
}

#[test]
fn ghosts_and_package_frames_follow_the_options() {
    let mut d = example("library");
    let mut ghost = d.classes[0].clone();
    ghost.name = "GhostThing".into();
    ghost.status = Status::Proposed;
    d.classes.push(ghost);
    let metrics = compute(&d, false);
    let scene = class_scene(&d.classes, &d.relations, &d.packages, &d, 2);
    let src = Source::Classes { classes: &d.classes, relations: &d.relations, packages: &d.packages, package_deps: &metrics.package_deps, conceptual: false };
    let plain = canvas::build(&scene, &ApproxMeasure, &src, &CanvasOptions { ghosts: false, packages: false, header: Some("## Library\nClass diagram · 1 January 2026".into()) }).unwrap();
    valid(&plain);
    assert!(!plain.nodes.iter().any(|n| n.id == "class:GhostThing"));
    assert!(!plain.nodes.iter().any(|n| n.kind == "group"));
    let title = card(&plain, "title");
    assert!(title.y + title.height <= 0, "the title sits above the diagram");
    let with = canvas::build(&scene, &ApproxMeasure, &src, &CanvasOptions { ghosts: true, packages: true, header: None }).unwrap();
    valid(&with);
    assert!(with.nodes.iter().any(|n| n.id == "class:GhostThing"));
    assert!(!with.nodes.iter().any(|n| n.id == "title"));
}

#[test]
fn every_relation_kind_reads_correctly() {
    let d = synthetic(serde_json::json!([
        {"op":"add_class","name":"Shape","stereotype":"abstract"},
        {"op":"add_class","name":"Drawable","stereotype":"interface"},
        {"op":"add_class","name":"Circle"},
        {"op":"add_class","name":"Canvas"},
        {"op":"add_class","name":"Layer"},
        {"op":"add_class","name":"Palette"},
        {"op":"add_class","name":"Renderer"},
        {"op":"add_relation","from":"Circle","to":"Shape","kind":"inheritance"},
        {"op":"add_relation","from":"Shape","to":"Drawable","kind":"realization"},
        {"op":"add_relation","from":"Canvas","to":"Layer","kind":"composition","from_mult":"1","to_mult":"1..*","label":"layers"},
        {"op":"add_relation","from":"Layer","to":"Shape","kind":"aggregation"},
        {"op":"add_relation","from":"Canvas","to":"Palette","kind":"association","directed":true,"label":"palette"},
        {"op":"add_relation","from":"Palette","to":"Layer","kind":"association","directed":false},
        {"op":"add_relation","from":"Renderer","to":"Canvas","kind":"dependency"}
    ]));
    let scene = class_scene(&d.classes, &d.relations, &d.packages, &d, 3);
    let src = Source::Classes { classes: &d.classes, relations: &d.relations, packages: &d.packages, package_deps: &[], conceptual: false };
    let c = canvas::build(&scene, &ApproxMeasure, &src, &opts()).unwrap();
    valid(&c);
    cards_dont_overlap(&c);
    let e = |from: &str, to: &str| c.edges.iter().find(|e| e.from_node == format!("class:{from}") && e.to_node == format!("class:{to}")).unwrap_or_else(|| panic!("{from}→{to}"));
    assert_eq!((e("Circle", "Shape").label.as_deref(), e("Circle", "Shape").to_end), (Some("extends"), "arrow"));
    assert_eq!((e("Shape", "Drawable").label.as_deref(), e("Shape", "Drawable").to_end), (Some("implements"), "arrow"));
    assert_eq!((e("Canvas", "Layer").label.as_deref(), e("Canvas", "Layer").to_end), (Some("◆ composed of · layers · 1 → 1..*"), "none"));
    assert_eq!((e("Layer", "Shape").label.as_deref(), e("Layer", "Shape").to_end), (Some("◇ aggregates"), "none"));
    assert_eq!((e("Canvas", "Palette").label.as_deref(), e("Canvas", "Palette").to_end), (Some("palette"), "arrow"));
    assert_eq!((e("Palette", "Layer").label.as_deref(), e("Palette", "Layer").to_end), (None, "none"));
    assert_eq!((e("Renderer", "Canvas").label.as_deref(), e("Renderer", "Canvas").to_end), (Some("depends on"), "arrow"));
    // abstract / interface names keep their emphasis
    assert!(card(&c, "class:Shape").text.as_ref().unwrap().contains("***Shape***") || card(&c, "class:Shape").text.as_ref().unwrap().contains("*Shape*"));
}

#[test]
fn domain_model_reads_as_concepts() {
    let d = example("library");
    let (classes, relations) = d.domain.as_class_model();
    let scene = class_scene(&classes, &relations, &[], &d, 3);
    let src = Source::Classes { classes: &classes, relations: &relations, packages: &[], package_deps: &[], conceptual: true };
    let c = canvas::build(&scene, &ApproxMeasure, &src, &opts()).unwrap();
    valid(&c);
    cards_dont_overlap(&c);
    assert_eq!(c.nodes.iter().filter(|n| n.kind == "text").count(), classes.len());
    assert!(c.nodes.iter().all(|n| n.color.is_none()));
}

#[test]
fn package_and_c4_views() {
    let mut d = example("library");
    let ops: Vec<spide_core::ops::Op> = serde_json::from_value(serde_json::json!([
        {"op":"add_container","name":"Web UI","technology":"JavaFX"},{"op":"add_container","name":"Lending API","technology":"Spring Boot"},
        {"op":"add_container","name":"DB","technology":"Postgres","datastore":true},
        {"op":"add_system","name":"Mail","description":"sends reminders"},
        {"op":"assign_container","package":"ui","container":"Web UI"},{"op":"assign_container","package":"application","container":"Lending API"},
        {"op":"assign_container","package":"domain","container":"Lending API"},{"op":"assign_container","package":"infrastructure","container":"Lending API"},
        {"op":"add_link","from":"Lending API","to":"DB","protocol":"JDBC"},
        {"op":"add_link","from":"Lending API","to":"Mail","protocol":"SMTP","interaction":"async"}
    ]))
    .unwrap();
    d = spide_core::ops::apply_all(&d, &ops, spide_core::ops::Mode::Commit(Status::Accepted, None)).unwrap();
    let metrics = compute(&d, false);
    let derived = spide_core::derive::derive(&d, &metrics);
    let m = ApproxMeasure;
    // package diagram: packages are cards with their classes, coloured by layer
    let ps = views::package_scene(&m, &derived, &metrics, &mut Default::default()).scene;
    let c = canvas::build(&ps, &m, &Source::Packages { packages: &derived.packages.packages, deps: &metrics.package_deps }, &opts()).unwrap();
    valid(&c);
    cards_dont_overlap(&c);
    let dom = card(&c, "package:domain");
    assert!(dom.text.as_ref().unwrap().contains("**domain**") && dom.text.as_ref().unwrap().contains("`Loan`"), "{:?}", dom.text);
    assert_eq!(dom.color, Some(layer_color(Layer::Domain).css()));
    assert!(!c.edges.is_empty());
    // C4 containers and context
    for level in [C4Level::Container, C4Level::Context] {
        let sc = views::system_scene(&m, &derived, &metrics, level, &mut Default::default()).scene;
        let c = canvas::build(&sc, &m, &Source::System { view: &derived.system, level }, &opts()).unwrap();
        valid(&c);
        cards_dont_overlap(&c);
        assert!(!c.edges.is_empty(), "{level:?}");
        assert!(c.nodes.iter().all(|n| n.color.is_some()));
        if level == C4Level::Container {
            assert!(card(&c, "c4:DB").text.as_ref().unwrap().contains("**DB**"));
            assert!(c.edges.iter().any(|e| e.label.as_deref() == Some("«async» SMTP")));
            assert!(c.edges.iter().any(|e| e.label.as_deref() == Some("JDBC")));
        }
    }
}

#[test]
fn use_case_diagram_has_a_system_boundary() {
    let mut d = example("library");
    d.use_cases = UseCaseModel {
        system: Some("Library".into()),
        actors: vec![Actor { name: "Member".into(), ..Default::default() }, Actor { name: "Payment provider".into(), description: Some("external system".into()), ..Default::default() }],
        use_cases: vec![
            UseCase { name: "Borrow book".into(), actors: vec!["Member".into()], includes: vec!["Check membership".into()], classes: vec!["Loan".into()], ..Default::default() },
            UseCase { name: "Check membership".into(), classes: vec!["Member".into()], ..Default::default() },
            UseCase { name: "Pay fine".into(), actors: vec!["Member".into(), "Payment provider".into()], extends: vec!["Borrow book".into()], ..Default::default() },
        ],
        generated_rev: None,
    };
    let m = ApproxMeasure;
    let sc = views::usecase_scene(&m, &d, &mut Default::default()).scene;
    let c = canvas::build(&sc, &m, &Source::UseCases { design: &d }, &opts()).unwrap();
    valid(&c);
    cards_dont_overlap(&c);
    let sys = card(&c, "system");
    assert_eq!((sys.kind, sys.label.as_deref()), ("group", Some("Library")));
    for uc in ["Borrow book", "Check membership", "Pay fine"] {
        assert!(card(&c, &format!("usecase:{uc}")).text.as_ref().unwrap().contains(uc));
    }
    // "no realising class" is a diagnostic hint of the canvas, but drawn inside the scene: it's text
    // below the ellipse, outside the card, so it isn't part of the card
    assert!(c.edges.iter().any(|e| e.label.as_deref() == Some("«include»")));
    assert!(c.edges.iter().any(|e| e.label.as_deref() == Some("«extend»")));
    assert_eq!(c.edges.iter().filter(|e| e.from_node.starts_with("actor:")).count(), 3);
}

#[test]
fn sequence_diagram_maps_lifelines_messages_and_fragments() {
    let s = Sequence {
        name: "borrow".into(),
        participants: vec!["Member".into(), "ui".into(), "svc".into(), "loan".into()],
        lifelines: vec![
            Lifeline { name: "ui".into(), class: "LoanController".into(), multi: false },
            Lifeline { name: "svc".into(), class: "LendingService".into(), multi: false },
            Lifeline { name: "loan".into(), class: "Loan".into(), multi: false },
        ],
        messages: vec![
            Message { from: "Member".into(), to: "ui".into(), method: "borrow".into(), args: Some("isbn".into()), ..Default::default() },
            Message { from: "ui".into(), to: "svc".into(), method: "lend".into(), args: Some("member, copy".into()), returns: Some("loan".into()), ..Default::default() },
            Message { from: "svc".into(), to: "loan".into(), method: "Loan".into(), kind: MessageKind::Create, ..Default::default() },
            Message { from: "svc".into(), to: "svc".into(), method: "audit".into(), ..Default::default() },
        ],
        fragments: vec![Fragment { kind: FragmentKind::Opt, operands: vec![Operand { guard: Some("available".into()), first: 2, last: 3 }] }],
        ..Default::default()
    };
    let m = ApproxMeasure;
    let actor = |n: &str| n == "Member";
    let sc = views::sequence_scene(&m, &s, &actor).scene;
    let c = canvas::build(&sc, &m, &Source::Sequence { seq: &s, is_actor: &actor }, &opts()).unwrap();
    valid(&c);
    cards_dont_overlap(&c);
    assert!(card(&c, "lifeline:ui").text.as_ref().unwrap().contains("ui : LoanController"));
    assert!(card(&c, "lifeline:Member").text.as_ref().unwrap().contains("*actor*"));
    assert_eq!(card(&c, "lane:svc").kind, "group");
    let e = |id: &str| c.edges.iter().find(|e| e.id == id).unwrap_or_else(|| panic!("no edge {id}"));
    assert_eq!(e("m0").label.as_deref(), Some("1: borrow(isbn)"));
    assert_eq!((e("m0").from_side, e("m0").to_side), ("right", "left"));
    assert_eq!(e("m1").label.as_deref(), Some("2: lend(member, copy)"));
    // the create message ends on the new object's header
    assert_eq!(e("m2").to_node, "lifeline:loan");
    // self-call loops on its anchor
    assert_eq!(e("m3").from_node, e("m3").to_node);
    // replies run back, labelled
    assert!(c.edges.iter().any(|e| e.label.as_deref() == Some("↩ loan") && e.from_side == "left"));
    let frag = c.nodes.iter().find(|n| n.id.starts_with("fragment:")).unwrap();
    assert_eq!((frag.kind, frag.label.as_deref()), ("group", Some("opt [available]")));
    // anchors sit on their lifelines, in message order
    let a0 = card(&c, "m0:from");
    let a1 = card(&c, "m1:from");
    assert!(a1.y > a0.y);
    let lane = card(&c, "lane:Member");
    assert!(a0.x >= lane.x && a0.x + a0.width <= lane.x + lane.width);
    std::fs::write(format!("{}/../../target/scene-out/sequence.canvas", env!("CARGO_MANIFEST_DIR")), c.to_json()).ok();
}
