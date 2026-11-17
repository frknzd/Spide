//! Lucid writer: every diagram of a design as the items Spide wants in its Lucid document
//! ([`spide_core::lucid::Target`]), placed where the PNG/SVG export draws them.
//!
//! Content comes from the model (structured UML compartments, typed relationships, sequence markup),
//! geometry from the export scene (the same layout, saved `class`/`domain` positions honoured), scaled
//! so Lucid's 10 pt text fits. Ghosts, pruned elements and diagnostics are never written.

use crate::class_view::{self, Data, Opts, Routes};
use crate::export_parts::{scene_nodes, PartNode, Source};
use crate::insights;
use crate::scene::Scene;
use crate::views::{self, C4Level};
use crate::visuals::{ApproxMeasure, Measure};
use spide_core::derive;
use spide_core::lucid::{diagram_keys, diagram_title, seq_markup, uml_text as ut, Bbox, BlockShape, Item, ItemKind, Target};
use spide_core::metrics::{compute, Metrics};
use spide_core::model::*;
use std::collections::{HashMap, HashSet};

// Lucid's class box: 10 pt text, 16 px per member line, title 22 px per line.
const CHAR_W: f64 = 6.6;
const LINE_H: f64 = 16.0;
const TITLE_H: f64 = 22.0;
const PAD: f64 = 16.0;

/// Every diagram of the design, keyed as in [`diagram_keys`].
pub fn targets(d: &Design) -> Vec<Target> {
    let metrics = compute(d, false);
    diagram_keys(d).iter().filter_map(|k| target_with(d, &metrics, k)).collect()
}

/// One diagram (None: unknown key or nothing to draw).
pub fn target(d: &Design, key: &str) -> Option<Target> {
    target_with(d, &compute(d, false), key)
}

/// One diagram, keeping boxes where the Lucid document (`base`, as last synced) has them: a fresh
/// layout never shuffles a shared document. Spide's own saved positions (class and domain views)
/// win; new elements get the layout's position.
pub fn target_anchored(d: &Design, key: &str, base: Option<&spide_core::lucid::snapshot::Snapshot>) -> Option<Target> {
    if let Some(package) = key.strip_prefix("class.package:") {
        let metrics = compute(d, false);
        let scope = spide_core::scope::PackageScope::new(d, &metrics, package, false).ok()?;
        let projected = scope.design(d);
        let metrics = scope.metrics(&metrics, d);
        let target = target_with(&projected, &metrics, "class")?;
        let mut result = anchor_target(&projected, "class", base, target);
        result.key = key.into();
        result.title = diagram_title(&d.name, key);
        return Some(result);
    }
    Some(anchor_target(d, key, base, target(d, key)?))
}

/// Anchor the already-projected target, avoiding dependency recomputation on a partial model.
fn anchor_target(d: &Design, key: &str, base: Option<&spide_core::lucid::snapshot::Snapshot>, mut t: Target) -> Target {
    let Some(base) = base else { return t };
    let saved = match key {
        "class" | "domain" => d.layout.get(key),
        _ => None,
    };
    let frames: Vec<(String, Bbox)> = base.items.iter().filter_map(|i| match &i.kind {
        ItemKind::Package { name, .. } => Some((name.clone(), i.bbox?)),
        _ => None,
    }).collect();
    let frame_at = |b: Bbox| frames.iter().filter(|(_, f)| f.contains_point(b.centre())).min_by(|a, b| (a.1.w * a.1.h).total_cmp(&(b.1.w * b.1.h))).map(|(n, _)| n.clone());
    let renamed = spide_core::lucid::patch::renames(base, &t);
    let old_sid = |sid: &str| renamed.iter().find(|(_, to)| to == sid).map(|(from, _)| from.clone()).unwrap_or_else(|| sid.to_string());
    let mut anchored = false;
    for it in t.items.iter_mut() {
        if it.kind.is_line() || matches!(it.kind, ItemKind::Sequence { .. } | ItemKind::Package { .. } | ItemKind::Boundary { .. }) {
            continue;
        }
        let Some(sid) = it.sid.clone() else { continue };
        let Some(b) = base.by_sid(&old_sid(&sid)).and_then(|i| i.bbox) else { continue };
        let name = sid.split_once(':').map(|x| x.1).unwrap_or_default();
        if saved.is_some_and(|s| s.contains_key(name)) {
            continue;
        }
        // a class that moved to another package goes where the layout puts it (into its new frame)
        if key == "class" {
            let pkg = d.class(name).and_then(|c| c.package.clone());
            if frame_at(b) != pkg && (pkg.is_some() || frame_at(b).is_some()) {
                continue;
            }
        }
        if let Some(tb) = it.bbox.as_mut() {
            if (tb.x - b.x).abs() >= 5.0 || (tb.y - b.y).abs() >= 5.0 {
                tb.x = b.x;
                tb.y = b.y;
                anchored = true;
            }
        }
    }
    if anchored {
        wrap_frames(d, key, &mut t.items);
    }
    t
}

/// Frames wrap their members (class view: a package's classes; use cases: the use cases).
fn wrap_frames(d: &Design, key: &str, items: &mut [Item]) {
    let round = |v: f64| (v / 10.0).round() * 10.0;
    let boxes: Vec<(String, Bbox)> = items.iter().filter_map(|i| Some((i.sid.clone()?, i.bbox?))).collect();
    for it in items.iter_mut() {
        let members: Vec<Bbox> = match (&it.kind, key) {
            (ItemKind::Package { name, .. }, "class") => d.live_classes().filter(|c| c.package.as_deref() == Some(name)).filter_map(|c| boxes.iter().find(|(s, _)| *s == format!("class:{}", c.name)).map(|x| x.1)).collect(),
            (ItemKind::Boundary { .. }, _) => boxes.iter().filter(|(s, _)| s.starts_with("usecase:")).map(|x| x.1).collect(),
            _ => continue,
        };
        if members.is_empty() {
            continue;
        }
        let x1 = members.iter().map(|b| b.x).fold(f64::INFINITY, f64::min);
        let y1 = members.iter().map(|b| b.y).fold(f64::INFINITY, f64::min);
        let x2 = members.iter().map(|b| b.x + b.w).fold(f64::NEG_INFINITY, f64::max);
        let y2 = members.iter().map(|b| b.y + b.h).fold(f64::NEG_INFINITY, f64::max);
        it.bbox = Some(Bbox { x: round(x1 - 20.0), y: round(y1 - 40.0), w: round(x2 - x1 + 40.0), h: round(y2 - y1 + 60.0) });
    }
}

fn target_with(d: &Design, metrics: &Metrics, key: &str) -> Option<Target> {
    if let Some(package) = key.strip_prefix("class.package:") {
        let scope = spide_core::scope::PackageScope::new(d, metrics, package, false).ok()?;
        let projected = scope.design(d);
        let metrics = scope.metrics(metrics, d);
        let items = class_items(&projected, &metrics);
        return (!items.is_empty()).then(|| Target { key: key.into(), title: diagram_title(&d.name, key), items });
    }
    let items = match key {
        "class" => class_items(d, metrics),
        "domain" => domain_items(d, metrics),
        "package" => package_items(d, metrics),
        "usecase" => usecase_items(d),
        "system.context" => system_items(d, metrics, C4Level::Context),
        "system.containers" => system_items(d, metrics, C4Level::Container),
        k => {
            let name = k.strip_prefix("sequence:")?;
            sequence_items(d, name)
        }
    };
    (!items.is_empty()).then(|| Target { key: key.to_string(), title: diagram_title(&d.name, key), items })
}

fn lines_need(lines: &[&str]) -> f64 {
    lines.iter().map(|l| l.chars().count() as f64 * CHAR_W).fold(0.0, f64::max) + 2.0 * PAD
}

fn class_need(kind: &ItemKind) -> (f64, f64) {
    match kind {
        ItemKind::Class { name, attributes, operations, .. } => {
            let mut all: Vec<&str> = name.lines().collect();
            all.extend(attributes.iter().map(String::as_str));
            all.extend(operations.iter().map(String::as_str));
            let w = lines_need(&all).max(140.0);
            let h = name.lines().count() as f64 * TITLE_H + (attributes.len().max(1) + operations.len().max(1)) as f64 * LINE_H + 2.0 * PAD;
            (w, h)
        }
        ItemKind::Block { text, .. } => {
            let lines: Vec<&str> = text.lines().collect();
            (lines_need(&lines).max(160.0), (lines.len() as f64 * LINE_H + 2.0 * PAD).max(80.0))
        }
        ItemKind::Actor { name } => ((name.chars().count() as f64 * CHAR_W).max(60.0), 110.0),
        ItemKind::UseCase { name } => ((name.chars().count() as f64 * CHAR_W + 60.0).max(140.0), 70.0),
        _ => (0.0, 0.0),
    }
}

pub use spide_core::lucid::LAYOUT_SCALE;

/// Scene boxes → Lucid boxes: one scale for the whole diagram so every box fits its text (or the fixed
/// [`LAYOUT_SCALE`]), then each box at least as large as its text needs. Frames (packages,
/// boundaries) wrap what they contain.
fn place(items: &mut [Item], rects: &HashMap<String, [f64; 4]>, fixed: bool) {
    let mut k: f64 = 1.0;
    for it in items.iter() {
        let (Some(sid), (nw, nh)) = (it.sid.as_deref(), class_need(&it.kind)) else { continue };
        let Some(r) = rects.get(sid) else { continue };
        if nw > 0.0 {
            k = k.max(nw / (r[2] - r[0]).max(1.0)).max(nh / (r[3] - r[1]).max(1.0));
        }
    }
    let k = if fixed { LAYOUT_SCALE } else { k.clamp(1.0, 3.0) };
    let min_x = if fixed { 0.0 } else { rects.values().map(|r| r[0]).fold(f64::INFINITY, f64::min) };
    let min_y = if fixed { 0.0 } else { rects.values().map(|r| r[1]).fold(f64::INFINITY, f64::min) };
    if !min_x.is_finite() {
        return;
    }
    let round = |v: f64| (v / 10.0).round() * 10.0;
    for it in items.iter_mut() {
        let Some(r) = it.sid.as_deref().and_then(|s| rects.get(s)) else { continue };
        let (nw, nh) = class_need(&it.kind);
        let (x, y) = ((r[0] - min_x) * k, (r[1] - min_y) * k);
        let (w, h) = (((r[2] - r[0]) * k).max(nw), ((r[3] - r[1]) * k).max(nh));
        it.bbox = Some(Bbox { x: round(x), y: round(y), w: round(w).max(10.0), h: round(h).max(10.0) });
    }
    // frames grow to wrap their members' (grown) boxes
    let frames: Vec<usize> = items.iter().enumerate().filter(|(_, i)| matches!(i.kind, ItemKind::Package { .. } | ItemKind::Boundary { .. })).map(|(n, _)| n).collect();
    for f in frames {
        let Some(fb) = items[f].bbox else { continue };
        let inside: Vec<Bbox> = items.iter().filter(|i| !matches!(i.kind, ItemKind::Package { .. } | ItemKind::Boundary { .. })).filter_map(|i| i.bbox).filter(|b| fb.contains_point(b.centre())).collect();
        if inside.is_empty() {
            continue;
        }
        let x1 = inside.iter().map(|b| b.x).fold(fb.x, f64::min) - 0.0;
        let y1 = inside.iter().map(|b| b.y).fold(fb.y, f64::min);
        let x2 = inside.iter().map(|b| b.x + b.w).fold(fb.x + fb.w, f64::max);
        let y2 = inside.iter().map(|b| b.y + b.h).fold(fb.y + fb.h, f64::max);
        items[f].bbox = Some(Bbox { x: round(x1 - 20.0), y: round(y1 - 40.0), w: round(x2 - x1 + 40.0), h: round(y2 - y1 + 60.0) });
    }
}

fn rects_of(nodes: &[PartNode]) -> HashMap<String, [f64; 4]> {
    nodes.iter().map(|n| (n.id.clone(), n.r)).collect()
}

fn fill_of(nodes: &[PartNode], id: &str) -> Option<String> {
    nodes.iter().find(|n| n.id == id).and_then(|n| n.color).map(|c| {
        // the layer colours are for dark canvases: a light tint of them reads well on Lucid's white page
        let mix = |v: u8| ((v as f64) * 0.25 + 255.0 * 0.75).round() as u8;
        format!("#{:02x}{:02x}{:02x}", mix(c.0), mix(c.1), mix(c.2))
    })
}

// ------------------------------------------------------------------ class diagram & domain model

/// The class/domain scene as exports build it (full detail: every member is written to Lucid).
fn class_scene(m: &dyn Measure, classes: &[Class], relations: &[Relation], packages: &[Package], metrics: Option<&Metrics>, saved: Option<&std::collections::BTreeMap<String, Pos>>) -> Scene {
    let ext = class_view::external_classes(classes, packages);
    let marks = insights::method_marks(metrics);
    let members = insights::member_diagnostics(metrics, classes);
    let boxes = class_view::boxes_for(m, classes, true, 4, relations, None, &ext, Some(&marks), Some(&members), &HashSet::new());
    let extents: HashMap<String, (f64, f64)> = classes.iter().filter(|c| boxes.contains_key(&c.name)).map(|c| (c.name.clone(), class_view::node_extent(m, c, &boxes[&c.name], metrics))).collect();
    let mut pos = class_view::layout_classes(m, classes, &extents, relations, packages);
    if let Some(saved) = saved {
        for (name, p) in saved {
            if pos.contains_key(name) {
                pos.insert(name.clone(), [p.x, p.y]);
            }
        }
    }
    let implicit = metrics.map(|x| x.implicit_edges.clone()).unwrap_or_default();
    let data = Data { classes, relations, packages, metrics, implicit: &implicit };
    let o = Opts { pos, boxes, show_methods: true, still: true, plain: true, level: 4, zoom_cap: 4, marks: Some(&marks), members: Some(&members), ..Default::default() };
    class_view::build(m, &data, &o, &mut Routes::default())
}

fn live_classes(d: &Design) -> Vec<Class> {
    d.classes
        .iter()
        .filter(|c| c.status.is_live())
        .map(|c| Class { attributes: c.attributes.iter().filter(|a| a.status.is_live()).cloned().collect(), methods: c.methods.iter().filter(|m| m.status.is_live()).cloned().collect(), ..c.clone() })
        .collect()
}

/// A class box's content as Lucid shows it.
pub fn class_kind(c: &Class) -> ItemKind {
    ItemKind::Class {
        name: ut::format_title(c),
        attributes: c.attributes.iter().filter(|a| a.status.is_live()).map(ut::format_attribute).collect(),
        operations: c.methods.iter().filter(|m| m.status.is_live()).map(ut::format_method).collect(),
        interface: c.is_interface(),
    }
}

pub fn relation_kind(r: &Relation) -> ItemKind {
    ItemKind::Relationship {
        from: format!("class:{}", r.from),
        to: format!("class:{}", r.to),
        rel: ut::relation_type(r.kind, r.directed).to_string(),
        label: ut::relation_label(r.interaction, r.label.as_deref()),
        from_mult: r.from_mult.clone().unwrap_or_default(),
        to_mult: r.to_mult.clone().unwrap_or_default(),
    }
}

fn class_items(d: &Design, metrics: &Metrics) -> Vec<Item> {
    let classes = live_classes(d);
    let names: HashSet<&str> = classes.iter().map(|c| c.name.as_str()).collect();
    let relations: Vec<Relation> = d.relations.iter().filter(|r| r.status.is_live() && names.contains(r.from.as_str()) && names.contains(r.to.as_str())).cloned().collect();
    let packages: Vec<Package> = d.packages.iter().filter(|p| p.status.is_live()).cloned().collect();
    let m = ApproxMeasure;
    let scene = class_scene(&m, &classes, &relations, &packages, Some(metrics), d.layout.get("class"));
    let pdeps = metrics.package_deps.clone();
    let src = Source::Classes { classes: &classes, relations: &relations, packages: &packages, package_deps: &pdeps, conceptual: false };
    let nodes = scene_nodes(&scene, &m, &src, false, true);
    let mut items = vec![];
    for p in &packages {
        let sid = format!("package:{}", p.name);
        if nodes.iter().any(|n| n.id == sid) {
            items.push(Item { fill: None, ..Item::new(sid, ItemKind::Package { name: p.name.clone(), body: String::new() }, None) });
        }
    }
    for c in &classes {
        let sid = format!("class:{}", c.name);
        items.push(Item { fill: fill_of(&nodes, &sid), ..Item::new(sid, class_kind(c), None) });
    }
    for r in &relations {
        items.push(Item::new(format!("rel:{}", r.id), relation_kind(r), None));
    }
    place(&mut items, &rects_of(&nodes), true);
    items
}

pub fn concept_kind(c: &Concept) -> ItemKind {
    ItemKind::Class { name: c.name.clone(), attributes: c.attributes.iter().map(ut::format_concept_attribute).collect(), operations: vec![], interface: false }
}

pub fn concept_link_kind(a: &ConceptLink) -> ItemKind {
    let kind = match a.kind {
        ConceptLinkKind::Association => "association",
        ConceptLinkKind::Generalization => "generalization",
        ConceptLinkKind::Composition => "composition",
        ConceptLinkKind::Aggregation => "aggregation",
    };
    ItemKind::Relationship {
        from: format!("concept:{}", a.from),
        to: format!("concept:{}", a.to),
        rel: kind.to_string(),
        label: a.label.clone().unwrap_or_default(),
        from_mult: a.from_mult.clone().unwrap_or_default(),
        to_mult: a.to_mult.clone().unwrap_or_default(),
    }
}

fn domain_items(d: &Design, _metrics: &Metrics) -> Vec<Item> {
    let live = |s: Status| s != Status::Pruned && s != Status::Proposed;
    let concepts: Vec<&Concept> = d.domain.concepts.iter().filter(|c| live(c.status)).collect();
    if concepts.is_empty() {
        return vec![];
    }
    let (classes, relations) = d.domain.as_class_model();
    let classes: Vec<Class> = classes.into_iter().filter(|c| live(c.status)).collect();
    let relations: Vec<Relation> = relations.into_iter().filter(|r| live(r.status)).collect();
    let m = ApproxMeasure;
    let scene = class_scene(&m, &classes, &relations, &[], None, d.layout.get("domain"));
    let src = Source::Classes { classes: &classes, relations: &relations, packages: &[], package_deps: &[], conceptual: true };
    let nodes = scene_nodes(&scene, &m, &src, false, false);
    // the scene names concepts `class:X`; Lucid ids are `concept:X`
    let rects: HashMap<String, [f64; 4]> = nodes.iter().filter_map(|n| n.id.strip_prefix("class:").map(|x| (format!("concept:{x}"), n.r))).collect();
    let mut items: Vec<Item> = concepts.iter().map(|c| Item::new(format!("concept:{}", c.name), concept_kind(c), None)).collect();
    for a in d.domain.associations.iter().filter(|a| live(a.status)) {
        if concepts.iter().any(|c| c.name == a.from) && concepts.iter().any(|c| c.name == a.to) {
            items.push(Item::new(format!("assoc:{}", a.id), concept_link_kind(a), None));
        }
    }
    place(&mut items, &rects, true);
    items
}

// ------------------------------------------------------------------ packages

fn package_items(d: &Design, metrics: &Metrics) -> Vec<Item> {
    let derived = derive::derive(d, metrics);
    let m = ApproxMeasure;
    let vs = views::package_scene(&m, &derived, metrics, &mut Default::default());
    let src = Source::Packages { packages: &derived.packages.packages, deps: &metrics.package_deps };
    let nodes = scene_nodes(&vs.scene, &m, &src, false, true);
    let mut items = vec![];
    for p in derived.packages.packages.iter().filter(|p| p.status.is_live()) {
        let sid = format!("package:{}", p.name);
        if !nodes.iter().any(|n| n.id == sid) {
            continue;
        }
        items.push(Item { fill: fill_of(&nodes, &sid), ..Item::new(sid, ItemKind::Package { name: p.name.clone(), body: p.classes.join("\n") }, None) });
    }
    for dep in metrics.package_deps.iter().filter(|x| x.from != x.to) {
        let (f, t) = (format!("package:{}", dep.from), format!("package:{}", dep.to));
        if items.iter().any(|i| i.sid.as_deref() == Some(&f)) && items.iter().any(|i| i.sid.as_deref() == Some(&t)) {
            items.push(Item::new(format!("pkgdep:{}>{}", dep.from, dep.to), ItemKind::Relationship { from: f, to: t, rel: "dependency".into(), label: String::new(), from_mult: String::new(), to_mult: String::new() }, None));
        }
    }
    // package boxes need room for their class list
    let rects = rects_of(&nodes);
    let mut grown = rects.clone();
    for it in &items {
        if let (Some(sid), ItemKind::Package { name, body }) = (it.sid.as_deref(), &it.kind) {
            if let Some(r) = rects.get(sid) {
                let mut lines: Vec<&str> = body.lines().collect();
                lines.push(name);
                let w = lines_need(&lines).max(r[2] - r[0]);
                let h = (TITLE_H + lines.len() as f64 * LINE_H + 2.0 * PAD).max(r[3] - r[1]);
                grown.insert(sid.to_string(), [r[0], r[1], r[0] + w, r[1] + h]);
            }
        }
    }
    place(&mut items, &grown, false);
    items
}

// ------------------------------------------------------------------ use cases

fn usecase_items(d: &Design) -> Vec<Item> {
    let m = ApproxMeasure;
    let vs = views::usecase_scene(&m, d, &mut Default::default());
    let nodes = scene_nodes(&vs.scene, &m, &Source::UseCases { design: d }, false, true);
    let ucm = &d.use_cases;
    let shown = |s: Status| s != Status::Pruned && s != Status::Proposed;
    let ucs: Vec<&UseCase> = ucm.use_cases.iter().filter(|u| shown(u.status)).collect();
    if ucs.is_empty() {
        return vec![];
    }
    let mut items = vec![Item::new("boundary:system", ItemKind::Boundary { name: ucm.system.clone().unwrap_or_else(|| d.name.clone()) }, None)];
    for a in ucm.actors.iter().filter(|a| shown(a.status)) {
        items.push(Item::new(format!("actor:{}", a.name), ItemKind::Actor { name: a.name.clone() }, None));
    }
    for u in &ucs {
        items.push(Item::new(format!("usecase:{}", u.name), ItemKind::UseCase { name: u.name.clone() }, None));
    }
    let has = |items: &Vec<Item>, sid: &str| items.iter().any(|i| i.sid.as_deref() == Some(sid));
    for u in &ucs {
        let uc = format!("usecase:{}", u.name);
        for a in &u.actors {
            let from = format!("actor:{a}");
            if has(&items, &from) {
                items.push(Item::new(format!("ucactor:{a}>{}", u.name), ItemKind::Relationship { from, to: uc.clone(), rel: "association".into(), label: String::new(), from_mult: String::new(), to_mult: String::new() }, None));
            }
        }
        for (rel, list) in [("«include»", &u.includes), ("«extend»", &u.extends)] {
            for t in list {
                let to = format!("usecase:{t}");
                if has(&items, &to) {
                    let sid = format!("uc{}:{}>{t}", rel.trim_matches(['«', '»']), u.name);
                    items.push(Item::new(sid, ItemKind::Relationship { from: uc.clone(), to, rel: "dependency".into(), label: rel.into(), from_mult: String::new(), to_mult: String::new() }, None));
                }
            }
        }
    }
    let mut rects = rects_of(&nodes);
    if let Some(r) = rects.remove("system") {
        rects.insert("boundary:system".into(), r);
    }
    place(&mut items, &rects, false);
    items
}

// ------------------------------------------------------------------ C4

fn system_items(d: &Design, metrics: &Metrics, level: C4Level) -> Vec<Item> {
    let derived = derive::derive(d, metrics);
    let sv = &derived.system;
    let m = ApproxMeasure;
    let vs = views::system_scene(&m, &derived, metrics, level, &mut Default::default());
    let nodes = scene_nodes(&vs.scene, &m, &Source::System { view: sv, level }, false, true);
    let shown = |s: Status| s != Status::Pruned && s != Status::Proposed;
    let mut items = vec![];
    let add = |items: &mut Vec<Item>, name: &str, shape: BlockShape, text: String| {
        let sid = format!("c4:{name}");
        if nodes.iter().any(|n| n.id == sid) && !items.iter().any(|i: &Item| i.sid.as_deref() == Some(&sid)) {
            items.push(Item { fill: Some(shape.fill().to_string()), ..Item::new(sid, ItemKind::Block { shape, text }, None) });
        }
    };
    for a in &sv.actors {
        let desc = d.use_cases.actors.iter().find(|x| &x.name == a).and_then(|x| x.description.clone());
        add(&mut items, a, BlockShape::Person, ut::c4_text(a, "Person", None, desc.as_deref()));
    }
    for x in sv.systems.iter().filter(|x| shown(x.status)) {
        add(&mut items, &x.name, BlockShape::ExternalSystem, ut::c4_text(&x.name, "Software System", None, x.description.as_deref()));
    }
    match level {
        C4Level::Context => add(&mut items, &sv.system, BlockShape::System, ut::c4_text(&sv.system, "Software System", None, None)),
        C4Level::Container => {
            for k in sv.containers.iter().filter(|k| shown(k.status)) {
                let (shape, kind) = if k.datastore { (BlockShape::Datastore, "Container") } else { (BlockShape::Container, "Container") };
                add(&mut items, &k.name, shape, ut::c4_text(&k.name, kind, k.technology.as_deref(), k.description.as_deref()));
            }
        }
    }
    let has = |items: &Vec<Item>, sid: &str| items.iter().any(|i| i.sid.as_deref() == Some(sid));
    let c4 = |n: &str| format!("c4:{n}");
    match level {
        C4Level::Context => {
            for e in &sv.context_edges {
                if has(&items, &c4(&e.from)) && has(&items, &c4(&e.to)) {
                    items.push(Item::new(format!("ctx:{}>{}", e.from, e.to), ItemKind::Line { from: c4(&e.from), to: c4(&e.to), label: e.label.clone().unwrap_or_default() }, None));
                }
            }
        }
        C4Level::Container => {
            for dep in &sv.container_deps {
                if has(&items, &c4(&dep.from)) && has(&items, &c4(&dep.to)) {
                    let label = format!("{}{} dep{}", if dep.sync { "" } else { "«async» " }, dep.count, if dep.count == 1 { "" } else { "s" });
                    items.push(Item::new(format!("cdep:{}>{}", dep.from, dep.to), ItemKind::Line { from: c4(&dep.from), to: c4(&dep.to), label }, None));
                }
            }
            for l in sv.links.iter().filter(|l| shown(l.status)) {
                if has(&items, &c4(&l.from)) && has(&items, &c4(&l.to)) {
                    let label = ut::relation_label(l.interaction, l.protocol.as_deref());
                    items.push(Item::new(format!("link:{}", l.id()), ItemKind::Line { from: c4(&l.from), to: c4(&l.to), label }, None));
                }
            }
        }
    }
    place(&mut items, &rects_of(&nodes), false);
    items
}

// ------------------------------------------------------------------ sequences

fn sequence_items(d: &Design, name: &str) -> Vec<Item> {
    let Some(s) = d.sequences.iter().find(|s| s.name == name && s.status != Status::Pruned) else { return vec![] };
    let actors: Vec<&str> = d.use_cases.actors.iter().map(|a| a.name.as_str()).collect();
    let is_actor = |n: &str| actors.contains(&n);
    vec![Item::new(format!("seq:{name}"), ItemKind::Sequence { markup: seq_markup::to_markup(s, &is_actor) }, None)]
}
