//! The derived views as scenes: package diagram, C4 system context/containers, use cases and
//! sequence diagrams.

use crate::class_view::{chip, edge_parts, EdgeKind, EdgeSpec, EdgeStyle};
use crate::layout::{self, Direction, LayoutEdge, LayoutNode, Positions};
use crate::route::{Deco, PathCmd, RBox, REdge, RouteCache};
use crate::scene::{paint, solid, stroke, tag, Anchor, Anim, Group, Hit, HitShape, HitTarget, Prim, Scene};
use crate::visuals::{c, f, health_color, layer_color, layer_name, wrap_all, FontSpec, Measure, Rect, Rgb};
use spide_core::metrics::Metrics;
use spide_core::model::{Actor, Interaction, MessageKind, RelationKind, Sequence, Status, UseCase};
use spide_core::Target;
use std::collections::{HashMap, HashSet};

/// A laid-out view: its scene, and the world positions used to collect marquee targets.
pub struct ViewScene {
    pub scene: Scene,
    /// (target, world point) for marquee selection
    pub points: Vec<(Target, [f64; 2])>,
}

fn hit_rect(r: Rect, target: Option<Target>, hover: String) -> Hit {
    Hit {
        shape: HitShape::Rect { x1: r.x - r.w / 2.0, y1: r.y - r.h / 2.0, x2: r.x + r.w / 2.0, y2: r.y + r.h / 2.0 },
        target: target.map(HitTarget::Target).unwrap_or(HitTarget::None),
        hover: Some(hover),
        node: None,
        title: None,
    }
}

fn glow(g: &mut Group, x: f64, y: f64, w: f64, h: f64, rx: f64) {
    g.push(Prim::Glow { x, y, w, h, rx, color: c::CYCLE, opacity: 0.35 });
}

// ------------------------------------------------------------------ package diagram

/// Cache of a layered layout keyed by its inputs.
#[derive(Default)]
pub struct LayoutCache {
    key: String,
    pub pos: Positions,
}

impl LayoutCache {
    pub fn get(&mut self, key: String, f: impl FnOnce() -> Positions) -> &Positions {
        if key != self.key {
            self.pos = f();
            self.key = key;
        }
        &self.pos
    }
}

#[derive(Default)]
pub struct PackageState {
    pub layout: LayoutCache,
    pub routes: RouteCache,
}

pub fn package_scene(m: &dyn Measure, d: &spide_core::derive::Derived, metrics: &Metrics, st: &mut PackageState) -> ViewScene {
    let pkgs = &d.packages.packages;
    let deps = &metrics.package_deps;
    let mut boxes: HashMap<&str, (f64, f64)> = HashMap::new();
    for p in pkgs {
        let label = match p.layer {
            Some(l) if layer_name(l) != p.name => format!("{}  ·  {}", p.name, layer_name(l)),
            _ => p.name.clone(),
        };
        let mut w = m.width(&label, f::NAME) + 40.0;
        for cl in &p.classes {
            w = w.max(m.width(cl, f::MEMBER) + 36.0);
        }
        boxes.insert(&p.name, (w.ceil().max(170.0), 52.0 + p.classes.len() as f64 * 17.0 + 14.0));
    }
    let key = format!("{:?}|{:?}", pkgs.iter().map(|p| (&p.name, boxes[p.name.as_str()].0 as i64, boxes[p.name.as_str()].1 as i64)).collect::<Vec<_>>(), deps.iter().map(|d| format!("{}{}", d.from, d.to)).collect::<Vec<_>>());
    let pos = st
        .layout
        .get(key, || {
            let nodes: Vec<LayoutNode> = pkgs.iter().map(|p| LayoutNode { id: p.name.clone(), w: boxes[p.name.as_str()].0, h: boxes[p.name.as_str()].1 + 22.0, group: None, external: false }).collect();
            let edges: Vec<LayoutEdge> = deps.iter().map(|d| LayoutEdge { from: d.from.clone(), to: d.to.clone(), kind: "dependency".into() }).collect();
            layout::layered(&nodes, &edges, Direction::Down)
        })
        .clone();
    let rect = |n: &str| -> Option<Rect> {
        let p = pos.get(n)?;
        let b = boxes.get(n)?;
        Some(Rect { x: p[0], y: p[1], w: b.0, h: b.1 })
    };
    // routing boxes include the folder tab above and the metric chips below
    let r_boxes: Vec<RBox> = pkgs.iter().filter_map(|p| rect(&p.name).map(|r| RBox::new(p.name.clone(), r.x, r.y + 6.0, r.w, r.h + 40.0))).collect();
    let r_edges: Vec<REdge> = deps.iter().filter(|d| d.from != d.to).map(|d| REdge::new(format!("{}>{}", d.from, d.to), &d.from, &d.to).deco(Deco::None, Deco::Arrow, false)).collect();
    let routes = st.routes.update(&r_boxes, &r_edges, &[], 0).clone();
    let mut scene = Scene::default();
    let mut hits = vec![];
    let mut points = vec![];
    let mut edges = Group::new();
    let mut labels = Group::new();
    for dep in deps {
        let (Some(a), Some(b)) = (rect(&dep.from), rect(&dep.to)) else { continue };
        let color = if dep.in_cycle {
            c::CYCLE
        } else if dep.layer_violation {
            c::WARN
        } else {
            c::EDGE
        };
        let id = format!("{}>{}", dep.from, dep.to);
        let spec = EdgeSpec {
            from: a,
            to: b,
            kind: EdgeKind::Rel(RelationKind::Dependency),
            directed: false,
            label: Some(dep.count.to_string()),
            from_mult: None,
            to_mult: None,
            bend: if deps.iter().any(|x| x.from == dep.to && x.to == dep.from) { 1.0 } else { 0.0 },
            style: EdgeStyle { strained: dep.in_cycle, ..EdgeStyle::new(color, if dep.in_cycle { 2.4 } else { 1.5 }, 1.0) },
            target: None,
            hover: None,
            warn: if dep.layer_violation {
                Some("layer ↑".into())
            } else if dep.in_cycle {
                Some("cycle".into())
            } else {
                None
            },
            route: routes.get(&id),
            status: None,
            key: id.clone(),
            label_at: None,
        };
        {
            let (l, t) = edge_parts(m, &spec, &mut hits);
            edges.add(l);
            labels.add(t);
        }
    }
    scene.root.add(edges);
    let mut nodes = Group::new();
    for p in pkgs {
        let Some(r) = rect(&p.name) else { continue };
        let pm = metrics.packages.iter().find(|x| x.name == p.name);
        let color = p.layer.map(layer_color).unwrap_or(c::EDGE);
        let (x, y) = (r.x - r.w / 2.0, r.y - r.h / 2.0);
        let tab_w = (r.w * 0.55).min(m.width(&p.name, f::NAME) + 28.0);
        let label = match p.layer {
            Some(l) if layer_name(l) != p.name => format!("{}  ·  {}", p.name, layer_name(l)),
            _ => p.name.clone(),
        };
        let ghost = p.status == Status::Proposed;
        let mut outer = Group::at(x, y).opacity(if ghost { 0.6 } else { 1.0 });
        if ghost {
            outer = outer.tags(tag::PROPOSED);
        }
        let mut g = Group::new().anim(Anim::Enter(format!("pkg:{}", p.name)));
        if pm.is_some_and(|m| m.in_cycle) {
            let mut dg = Group::new().tags(tag::DIAG);
            glow(&mut dg, -4.0, -18.0, r.w + 8.0, r.h + 22.0, 10.0);
            g.add(dg);
        }
        let s = stroke(color, 1.4);
        g.rect(0.0, 0.0, r.w, r.h, 0.0, Some(solid(c::NODE)), None);
        g.rect(0.0, -14.0, tab_w, 14.0, 0.0, Some(solid(c::NODE)), None);
        g.poly(vec![[0.0, -14.0], [tab_w, -14.0], [tab_w, 0.0], [r.w, 0.0], [r.w, r.h], [0.0, r.h]], true, None, Some(if ghost { s.dash(6.0, 4.0) } else { s }));
        g.rect(0.0, -14.0, tab_w, 14.0, 0.0, Some(paint(color, 0.18)), None);
        g.text(14.0, 20.0, label, f::NAME, c::TEXT, Anchor::Start);
        g.line([0.0, 32.0], [r.w, 32.0], stroke(color, 1.0).opacity(0.4));
        for (i, cl) in p.classes.iter().enumerate() {
            g.text(16.0, 50.0 + i as f64 * 17.0, format!("▫ {cl}"), f::MEMBER, c::DIM, Anchor::Start);
        }
        if let Some(pm) = pm {
            let mut cg = Group::at(0.0, r.h + 6.0);
            chip(m, &mut cg, 0.0, 0.0, &format!("I {:.2}", pm.instability), c::DIM, false);
            chip(m, &mut cg, 56.0, 0.0, &format!("A {:.2}", pm.abstractness), c::DIM, false);
            chip(m, &mut cg, 112.0, 0.0, &format!("D {:.2}", pm.distance), health_color(100.0 - pm.distance * 100.0), false);
            g.add(cg);
        }
        outer.add(g);
        nodes.add(outer);
        let t = Target::Package { name: p.name.clone() };
        hits.push(Hit { shape: HitShape::Rect { x1: x, y1: y - 14.0, x2: x + r.w, y2: y + r.h }, target: HitTarget::Target(t.clone()), hover: Some(format!("package:{}", p.name)), node: None, title: None });
        points.push((t, [r.x, r.y]));
    }
    scene.root.add(nodes);
    scene.root.add(labels);
    scene.hits = hits;
    ViewScene { scene, points }
}

// ------------------------------------------------------------------ C4

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum C4Level {
    Context,
    Container,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Person,
    System,
    External,
    Container,
    Datastore,
}

fn kind_color(k: Kind) -> Rgb {
    match k {
        Kind::Person => Rgb::hex(0xc084fc),
        Kind::System => Rgb::hex(0x7aa2ff),
        Kind::External => Rgb::hex(0x8a93a6),
        Kind::Container => Rgb::hex(0x60a5fa),
        Kind::Datastore => Rgb::hex(0x34d399),
    }
}

struct C4Node {
    id: String,
    kind: Kind,
    title: String,
    sub: Option<String>,
    lines: Vec<String>,
    ghost: bool,
    spof: bool,
    target: Option<Target>,
}

struct C4Edge {
    from: String,
    to: String,
    label: Option<String>,
    dashed: bool,
}

#[derive(Default)]
pub struct SystemState {
    pub layout: LayoutCache,
    pub routes: RouteCache,
}

pub fn system_empty(d: &spide_core::derive::Derived, level: C4Level) -> bool {
    let sv = &d.system;
    match level {
        C4Level::Container => sv.containers.is_empty(),
        C4Level::Context => sv.systems.is_empty() && sv.actors.is_empty(),
    }
}

pub fn system_scene(m: &dyn Measure, d: &spide_core::derive::Derived, metrics: &Metrics, level: C4Level, st: &mut SystemState) -> ViewScene {
    let sv = &d.system;
    let mut nodes: Vec<C4Node> = vec![];
    let mut edges: Vec<C4Edge> = vec![];
    for a in &sv.actors {
        nodes.push(C4Node { id: a.clone(), kind: Kind::Person, title: a.clone(), sub: Some("person".into()), lines: vec![], ghost: false, spof: false, target: Some(Target::Actor { name: a.clone() }) });
    }
    for x in &sv.systems {
        nodes.push(C4Node {
            id: x.name.clone(),
            kind: Kind::External,
            title: x.name.clone(),
            sub: Some("external system".into()),
            lines: x.description.clone().into_iter().collect(),
            ghost: x.status == Status::Proposed,
            spof: false,
            target: Some(Target::System { name: x.name.clone() }),
        });
    }
    if level == C4Level::Context {
        let n = sv.containers.len();
        nodes.push(C4Node { id: sv.system.clone(), kind: Kind::System, title: sv.system.clone(), sub: Some("the system".into()), lines: vec![format!("{n} container{}", if n == 1 { "" } else { "s" })], ghost: false, spof: false, target: None });
        for e in &sv.context_edges {
            edges.push(C4Edge { from: e.from.clone(), to: e.to.clone(), label: e.label.clone(), dashed: false });
        }
    } else {
        for k in &sv.containers {
            let spof = metrics.issues.iter().any(|i| i.kind == "spof" && i.targets.contains(&k.name));
            let mut sub: Vec<String> = vec![];
            if let Some(t) = &k.technology {
                sub.push(t.clone());
            }
            if k.replicas > 1 {
                sub.push(format!("×{}", k.replicas));
            }
            let sub = if sub.is_empty() { if k.datastore { "datastore".into() } else { "container".into() } } else { sub.join(" · ") };
            let lines = if !k.packages.is_empty() {
                vec![k.packages.join(", "), format!("{} class{}", k.classes, if k.classes == 1 { "" } else { "es" })]
            } else {
                k.description.clone().into_iter().collect()
            };
            nodes.push(C4Node { id: k.name.clone(), kind: if k.datastore { Kind::Datastore } else { Kind::Container }, title: k.name.clone(), sub: Some(sub), lines, ghost: k.status == Status::Proposed, spof, target: Some(Target::Container { name: k.name.clone() }) });
        }
        for dep in &sv.container_deps {
            edges.push(C4Edge { from: dep.from.clone(), to: dep.to.clone(), label: Some(format!("{} dep{}", dep.count, if dep.count == 1 { "" } else { "s" })), dashed: !dep.sync });
        }
        for l in &sv.links {
            let mut parts = vec![];
            if l.interaction != Interaction::Sync {
                parts.push(format!("«{}»", if l.interaction == Interaction::Async { "async" } else { "event" }));
            }
            if let Some(p) = &l.protocol {
                parts.push(p.clone());
            }
            edges.push(C4Edge { from: l.from.clone(), to: l.to.clone(), label: (!parts.is_empty()).then(|| parts.join(" ")), dashed: l.interaction != Interaction::Sync });
        }
    }
    let mut boxes: HashMap<String, (f64, f64)> = HashMap::new();
    for n in &mut nodes {
        let mut w = m.width(&n.title, f::NAME).max(m.width(n.sub.as_deref().unwrap_or(""), f::SMALL)) + 40.0;
        for l in &n.lines {
            w = w.max(m.width(l, f::SMALL) + 30.0);
        }
        let title_w = m.width(&n.title, f::NAME).max(m.width(&format!("[{}]", n.sub.as_deref().unwrap_or("")), FontSpec::sans(11.0))) + 40.0;
        let w2 = w.ceil().max(if n.kind == Kind::System { 240.0 } else { 170.0 }).min(320.0).max(title_w.ceil());
        // long description lines wrap inside the box instead of running out of it
        n.lines = n.lines.iter().flat_map(|l| wrap_all(m, l, w2 - 30.0, FontSpec::sans(11.0))).map(|l| l.trim().to_string()).collect();
        boxes.insert(n.id.clone(), (w2, if n.kind == Kind::Person { 92.0 } else { 62.0 } + n.lines.len() as f64 * 16.0));
    }
    let key = format!("{level:?}{:?}{:?}", nodes.iter().map(|n| (&n.id, boxes[&n.id].0 as i64, boxes[&n.id].1 as i64)).collect::<Vec<_>>(), edges.iter().map(|e| format!("{}>{}", e.from, e.to)).collect::<Vec<_>>());
    let pos = st
        .layout
        .get(key, || {
            let ln: Vec<LayoutNode> = nodes.iter().map(|n| LayoutNode { id: n.id.clone(), w: boxes[&n.id].0, h: boxes[&n.id].1 + 20.0, group: None, external: false }).collect();
            let le: Vec<LayoutEdge> = edges.iter().map(|e| LayoutEdge { from: e.from.clone(), to: e.to.clone(), kind: "dependency".into() }).collect();
            layout::layered(&ln, &le, Direction::Down)
        })
        .clone();
    let rect = |n: &str| -> Option<Rect> {
        let p = pos.get(n)?;
        let b = boxes.get(n)?;
        Some(Rect { x: p[0], y: p[1], w: b.0, h: b.1 })
    };
    let r_boxes: Vec<RBox> = nodes.iter().filter_map(|n| rect(&n.id).map(|r| RBox::new(n.id.clone(), r.x, r.y + if n.spof { 12.0 } else { 0.0 }, r.w, r.h + if n.spof { 24.0 } else { 0.0 }))).collect();
    let r_edges: Vec<REdge> = edges.iter().enumerate().filter(|(_, e)| e.from != e.to).map(|(i, e)| REdge::new(i.to_string(), &e.from, &e.to).deco(Deco::None, Deco::Arrow, false)).collect();
    let routes = st.routes.update(&r_boxes, &r_edges, &[], 0).clone();
    let mut scene = Scene::default();
    let mut hits = vec![];
    let mut points = vec![];
    let mut eg = Group::new();
    let mut labels = Group::new();
    for (i, e) in edges.iter().enumerate() {
        let (Some(a), Some(b)) = (rect(&e.from), rect(&e.to)) else { continue };
        let is_link = e.from != sv.system && e.to != sv.system && sv.links.iter().any(|l| l.from == e.from && l.to == e.to);
        let spec = EdgeSpec {
            from: a,
            to: b,
            kind: EdgeKind::Rel(if e.dashed { RelationKind::Dependency } else { RelationKind::Association }),
            directed: true,
            label: e.label.clone(),
            from_mult: None,
            to_mult: None,
            bend: if edges.iter().any(|x| x.from == e.to && x.to == e.from) { 1.0 } else { 0.0 },
            style: EdgeStyle { dashed: e.dashed, ..EdgeStyle::new(c::EDGE, 1.5, 1.0) },
            target: is_link.then(|| Target::Link { id: format!("{}->{}", e.from, e.to) }),
            hover: None,
            warn: None,
            route: routes.get(&i.to_string()),
            status: None,
            key: format!("c4e{i}"),
            label_at: None,
        };
        {
            let (l, t) = edge_parts(m, &spec, &mut hits);
            eg.add(l);
            labels.add(t);
        }
    }
    scene.root.add(eg);
    let mut ng = Group::new();
    for n in &nodes {
        let Some(r) = rect(&n.id) else { continue };
        let color = kind_color(n.kind);
        let (x, y) = (r.x - r.w / 2.0, r.y - r.h / 2.0);
        let top = if n.kind == Kind::Person { 34.0 } else { 0.0 };
        let mut outer = Group::at(x, y).opacity(if n.ghost { 0.6 } else { 1.0 });
        if n.ghost {
            outer = outer.tags(tag::PROPOSED);
        }
        let mut g = Group::new().anim(if n.ghost { Anim::Ghost(format!("c4:{}", n.id)) } else { Anim::Enter(format!("c4:{}", n.id)) });
        if n.spof {
            let mut dg = Group::new().tags(tag::DIAG);
            glow(&mut dg, -4.0, top - 4.0, r.w + 8.0, r.h - top + 8.0, 14.0);
            g.add(dg);
        }
        if n.kind == Kind::Person {
            g.push(Prim::Circle { cx: r.w / 2.0, cy: 16.0, r: 16.0, fill: Some(solid(c::NODE)), stroke: Some(stroke(color, 1.6)) });
        }
        if n.kind == Kind::Datastore {
            // cylinder
            let (w, h0, h1) = (r.w, top + 8.0, r.h - 8.0);
            let s = stroke(color, 1.6).dash_opt(if n.ghost { Some([6.0, 4.0]) } else { None });
            let mut body = vec![[0.0, h0]];
            for k in 0..=24 {
                let a = std::f64::consts::PI * k as f64 / 24.0;
                body.push([w / 2.0 - (w / 2.0) * a.cos(), h0 + 8.0 * a.sin()]);
            }
            body.push([w, h1]);
            for k in 0..=24 {
                let a = std::f64::consts::PI * k as f64 / 24.0;
                body.push([w / 2.0 + (w / 2.0) * a.cos(), h1 + 8.0 * a.sin()]);
            }
            g.rect(0.0, h0, w, h1 - h0, 0.0, Some(solid(c::NODE)), None);
            g.push(Prim::Ellipse { cx: w / 2.0, cy: h1, rx: w / 2.0, ry: 8.0, fill: Some(solid(c::NODE)), stroke: None });
            g.line([0.0, h0], [0.0, h1], s);
            g.line([w, h0], [w, h1], s);
            let mut bottom = vec![];
            for k in 0..=24 {
                let a = std::f64::consts::PI * k as f64 / 24.0;
                bottom.push([w / 2.0 + (w / 2.0) * a.cos(), h1 + 8.0 * a.sin()]);
            }
            g.poly(bottom, false, None, Some(s));
            g.push(Prim::Ellipse { cx: w / 2.0, cy: h0, rx: w / 2.0, ry: 8.0, fill: Some(solid(c::NODE)), stroke: Some(s) });
        } else {
            let fill = if n.kind == Kind::System { Rgb::hex(0x24314f) } else { c::NODE };
            let s = stroke(color, if n.kind == Kind::System { 2.0 } else { 1.6 }).dash_opt(if n.ghost || n.kind == Kind::External { Some([6.0, 4.0]) } else { None });
            g.rect(0.0, top, r.w, r.h - top, if n.kind == Kind::Person { 18.0 } else { 10.0 }, Some(solid(fill)), Some(s));
        }
        g.text(r.w / 2.0, top + 24.0, n.title.clone(), f::NAME, c::TEXT, Anchor::Middle);
        if let Some(sub) = &n.sub {
            g.text(r.w / 2.0, top + 40.0, format!("[{sub}]"), FontSpec::sans(11.0), c::DIM, Anchor::Middle);
        }
        for (i, l) in n.lines.iter().enumerate() {
            g.text(r.w / 2.0, top + 58.0 + i as f64 * 16.0, l.clone(), FontSpec::sans(11.0), c::FAINT, Anchor::Middle);
        }
        if n.spof {
            let mut dg = Group::new().tags(tag::DIAG);
            chip(m, &mut dg, r.w / 2.0 - 50.0, r.h + 6.0, "single point of failure", c::CYCLE, false);
            g.add(dg);
        }
        outer.add(g);
        ng.add(outer);
        hits.push(Hit { shape: HitShape::Rect { x1: x, y1: y + top, x2: x + r.w, y2: y + r.h }, target: n.target.clone().map(HitTarget::Target).unwrap_or(HitTarget::None), hover: Some(format!("c4:{}", n.id)), node: None, title: None });
        if let Some(t) = &n.target {
            points.push((t.clone(), [r.x, r.y]));
        }
    }
    scene.root.add(ng);
    scene.root.add(labels);
    scene.hits = hits;
    ViewScene { scene, points }
}

// ------------------------------------------------------------------ use cases

fn is_system_actor(a: &Actor) -> bool {
    let n = a.name.to_lowercase();
    ["system", "service", "gateway", "api", "server", "bank", "provider"].iter().any(|k| n.contains(k)) || a.description.as_deref().is_some_and(|d| d.to_lowercase().contains("external system"))
}

pub struct UcLayout {
    pub actors: HashMap<String, (Rect, bool)>,
    pub ucs: HashMap<String, Rect>,
    pub boundary: [f64; 4],
}

pub fn layout_use_cases(m: &dyn Measure, actors: &[&Actor], ucs: &[&UseCase]) -> UcLayout {
    let left: Vec<&&Actor> = actors.iter().filter(|a| !is_system_actor(a)).collect();
    let right: Vec<&&Actor> = actors.iter().filter(|a| is_system_actor(a)).collect();
    let row_h = 74.0;
    let mut order: HashMap<&str, f64> = HashMap::new();
    for (i, a) in left.iter().enumerate() {
        order.insert(&a.name, i as f64);
    }
    for (i, a) in right.iter().enumerate() {
        order.insert(&a.name, i as f64 + 0.5);
    }
    let bary = |u: &UseCase| -> f64 {
        if u.actors.is_empty() {
            99.0
        } else {
            u.actors.iter().map(|a| order.get(a.as_str()).copied().unwrap_or(0.0)).sum::<f64>() / u.actors.len() as f64
        }
    };
    let mut sorted: Vec<&UseCase> = ucs.to_vec();
    sorted.sort_by(|a, b| bary(a).total_cmp(&bary(b)));
    // included / extending use cases go into their own column so their arrows don't cross the main ones
    let targets: HashSet<&str> = ucs.iter().flat_map(|u| u.includes.iter().chain(u.extends.iter()).map(|s| s.as_str())).collect();
    let extenders: HashSet<&str> = ucs.iter().filter(|u| !u.extends.is_empty()).map(|u| u.name.as_str()).collect();
    let secondary = |u: &UseCase| (targets.contains(u.name.as_str()) || extenders.contains(u.name.as_str())) && u.actors.is_empty();
    let main: Vec<&UseCase> = sorted.iter().copied().filter(|u| !secondary(u)).collect();
    let second: Vec<&UseCase> = sorted.iter().copied().filter(|u| secondary(u)).collect();
    let main_cols = if main.len() > 9 { 2 } else { 1 };
    let per = main.len().div_ceil(main_cols).max(1);
    let mut columns: Vec<Vec<&UseCase>> = (0..main_cols).map(|c| main.iter().skip(c * per).take(per).copied().collect()).collect();
    if !second.is_empty() {
        columns.push(second);
    }
    let cols = columns.len();
    let uc_font = FontSpec::sans(13.0);
    let width_of = |u: &UseCase| (m.width(&u.name, uc_font) + 56.0).max(150.0);
    let col_w = sorted.iter().map(|u| width_of(u)).fold(200.0, f64::max) + 60.0;
    let per_col = columns.iter().map(|c| c.len()).max().unwrap_or(1).max(1);
    let mut uc_map = HashMap::new();
    for (ci, col) in columns.iter().enumerate() {
        let offset = (per_col - col.len()) as f64 * row_h / 2.0;
        for (row, u) in col.iter().enumerate() {
            let jog = if row % 2 == 1 && cols == 1 { 24.0 } else { 0.0 };
            uc_map.insert(u.name.clone(), Rect { x: ci as f64 * col_w + col_w / 2.0 + jog, y: offset + row as f64 * row_h + 70.0, w: width_of(u), h: 46.0 });
        }
    }
    let bw = cols as f64 * col_w + 40.0;
    let bh = (per_col as f64 * row_h + 50.0).max(160.0);
    let mut act = HashMap::new();
    let mut place = |list: &[&&Actor], x: f64, right: bool| {
        let gap = bh / (list.len() + 1) as f64;
        for (i, a) in list.iter().enumerate() {
            act.insert(a.name.clone(), (Rect { x, y: gap * (i + 1) as f64, w: 50.0, h: 90.0 }, right));
        }
    };
    place(&left, -170.0, false);
    place(&right, bw + 130.0, true);
    UcLayout { actors: act, ucs: uc_map, boundary: [-20.0, 0.0, bw, bh] }
}

#[derive(Default)]
pub struct UseCaseState {
    pub routes: RouteCache,
}

pub fn usecase_scene(m: &dyn Measure, design: &spide_core::Design, st: &mut UseCaseState) -> ViewScene {
    let ucm = &design.use_cases;
    let actors: Vec<&Actor> = ucm.actors.iter().filter(|a| a.status != Status::Pruned).collect();
    let ucs: Vec<&UseCase> = ucm.use_cases.iter().filter(|u| u.status != Status::Pruned).collect();
    let l = layout_use_cases(m, &actors, &ucs);
    let mut r_boxes: Vec<RBox> = l.actors.iter().map(|(n, (r, _))| RBox::new(format!("a:{n}"), r.x, r.y, r.w, 60.0)).collect();
    r_boxes.extend(l.ucs.iter().map(|(n, r)| RBox::new(format!("u:{n}"), r.x, r.y, r.w, r.h)));
    r_boxes.sort_by(|a, b| a.id.cmp(&b.id));
    let mut r_edges: Vec<REdge> = vec![];
    for u in &ucs {
        for a in &u.actors {
            r_edges.push(REdge::new(format!("{a}>{}", u.name), format!("a:{a}"), format!("u:{}", u.name)));
        }
        for i in &u.includes {
            r_edges.push(REdge::new(format!("{}«include»{i}", u.name), format!("u:{}", u.name), format!("u:{i}")).deco(Deco::None, Deco::Arrow, false));
        }
        for i in &u.extends {
            r_edges.push(REdge::new(format!("{}«extend»{i}", u.name), format!("u:{}", u.name), format!("u:{i}")).deco(Deco::None, Deco::Arrow, false));
        }
    }
    r_edges.retain(|e| e.from != e.to);
    let routes = st.routes.update(&r_boxes, &r_edges, &[], 0).clone();
    let mut scene = Scene::default();
    let mut hits = vec![];
    let mut points = vec![];
    if !ucs.is_empty() {
        let [bx, by, bw, bh] = l.boundary;
        let mut g = Group::new();
        g.rect(bx, by, bw, bh, 14.0, Some(paint(Rgb::hex(0xffffff), 0.02)), Some(stroke(c::BORDER, 1.2)));
        g.text(bx + 16.0, by + 24.0, ucm.system.clone().unwrap_or_else(|| design.name.clone()), FontSpec::sans(13.0).bold(), c::DIM, Anchor::Start);
        scene.root.add(g);
    }
    let mut eg = Group::new();
    let mut labels = Group::new();
    for u in &ucs {
        for a in &u.actors {
            let (Some((ar, _)), Some(ur)) = (l.actors.get(a), l.ucs.get(&u.name)) else { continue };
            let id = format!("{a}>{}", u.name);
            let spec = EdgeSpec {
                from: Rect { h: 60.0, ..*ar },
                to: Rect { w: ur.w * 0.95, ..*ur },
                kind: EdgeKind::Rel(RelationKind::Association),
                directed: false,
                label: None,
                from_mult: None,
                to_mult: None,
                bend: 0.0,
                style: EdgeStyle::new(c::EDGE, 1.3, 0.9),
                target: None,
                hover: None,
                warn: None,
                route: routes.get(&id),
                status: None,
                key: id.clone(),
                label_at: None,
            };
            {
            let (l, t) = edge_parts(m, &spec, &mut hits);
            eg.add(l);
            labels.add(t);
        }
        }
    }
    for u in &ucs {
        let rel: Vec<(&String, &str)> = u.includes.iter().map(|i| (i, "«include»")).chain(u.extends.iter().map(|i| (i, "«extend»"))).collect();
        for (other, label) in rel {
            let (Some(a), Some(b)) = (l.ucs.get(&u.name), l.ucs.get(other)) else { continue };
            let id = format!("{}{label}{other}", u.name);
            let spec = EdgeSpec {
                from: *a,
                to: *b,
                kind: EdgeKind::Rel(RelationKind::Dependency),
                directed: false,
                label: Some(label.into()),
                from_mult: None,
                to_mult: None,
                bend: 0.6,
                style: EdgeStyle::new(c::DIM, 1.2, 0.9),
                target: None,
                hover: None,
                warn: None,
                route: routes.get(&id),
                status: None,
                key: id.clone(),
                label_at: None,
            };
            {
            let (l, t) = edge_parts(m, &spec, &mut hits);
            eg.add(l);
            labels.add(t);
        }
        }
    }
    scene.root.add(eg);
    let mut ng = Group::new();
    for a in &actors {
        let Some((r, _)) = l.actors.get(&a.name) else { continue };
        let mut outer = Group::at(r.x, r.y - 30.0);
        let mut g = Group::new().anim(Anim::Enter(format!("actor:{}", a.name)));
        let s = stroke(c::TEXT, 1.6);
        g.push(Prim::Circle { cx: 0.0, cy: 0.0, r: 10.0, fill: None, stroke: Some(s) });
        g.line([0.0, 10.0], [0.0, 36.0], s);
        g.line([-16.0, 20.0], [16.0, 20.0], s);
        g.poly(vec![[-13.0, 56.0], [0.0, 36.0], [13.0, 56.0]], false, None, Some(s));
        g.text(0.0, 76.0, a.name.clone(), FontSpec::sans(12.5), c::TEXT, Anchor::Middle);
        outer.add(g);
        ng.add(outer);
        let t = Target::Actor { name: a.name.clone() };
        hits.push(Hit { shape: HitShape::Rect { x1: r.x - 40.0, y1: r.y - 46.0, x2: r.x + 40.0, y2: r.y + 54.0 }, target: HitTarget::Target(t.clone()), hover: Some(format!("actor:{}", a.name)), node: None, title: None });
        points.push((t, [r.x, r.y]));
    }
    for u in &ucs {
        let Some(r) = l.ucs.get(&u.name) else { continue };
        let realised = !u.classes.is_empty();
        let mut outer = Group::at(r.x, r.y);
        let mut g = Group::new().anim(Anim::Enter(format!("uc:{}", u.name)));
        g.push(Prim::Ellipse { cx: 0.0, cy: 0.0, rx: r.w / 2.0, ry: r.h / 2.0, fill: Some(solid(c::NODE)), stroke: Some(stroke(if realised { c::ACCENT } else { c::WARN }, 1.4)) });
        g.text(0.0, 4.5, u.name.clone(), FontSpec::sans(13.0), c::TEXT, Anchor::Middle);
        if !realised {
            g.text(0.0, r.h / 2.0 + 14.0, "no realising class", FontSpec::sans(10.0), c::WARN, Anchor::Middle);
        }
        outer.add(g);
        ng.add(outer);
        let t = Target::UseCase { name: u.name.clone() };
        hits.push(hit_rect(*r, Some(t.clone()), format!("usecase:{}", u.name)));
        points.push((t, [r.x, r.y]));
    }
    scene.root.add(ng);
    scene.root.add(labels);
    scene.hits = hits;
    ViewScene { scene, points }
}

// ------------------------------------------------------------------ sequences

const ROW: f64 = 46.0;
const TOP: f64 = 70.0;

/// Calls are numbered 1, 2, 3 … (returns are not), like the PlantUML export.
pub fn msg_label(s: &Sequence, i: usize) -> String {
    let m = &s.messages[i];
    if m.kind == MessageKind::Return {
        return if !m.method.is_empty() { m.method.clone() } else { m.returns.clone().unwrap_or_default() };
    }
    let n = s.messages[..=i].iter().filter(|x| x.kind != MessageKind::Return).count();
    let call = if m.method.contains('(') { m.method.clone() } else { format!("{}({})", m.method, m.args.clone().unwrap_or_default()) };
    format!("{n}: {call}{}", m.returns.as_ref().map(|r| format!(" : {r}")).unwrap_or_default())
}

pub struct SeqLayout {
    pub xs: HashMap<String, f64>,
    pub widths: HashMap<String, f64>,
    pub height: f64,
}

pub fn sequence_layout(m: &dyn Measure, s: &Sequence) -> SeqLayout {
    let mut xs = HashMap::new();
    let mut widths = HashMap::new();
    let mut x = 0.0;
    for (i, p) in s.participants.iter().enumerate() {
        let w = (m.width(p, f::NAME) + 30.0).max(110.0);
        widths.insert(p.clone(), w);
        if i > 0 {
            // make room for the longest label between neighbours
            let prev = &s.participants[i - 1];
            let mut need: f64 = 190.0;
            for (k, msg) in s.messages.iter().enumerate() {
                if (&msg.from == prev && &msg.to == p) || (&msg.from == p && &msg.to == prev) {
                    need = need.max(m.width(&format!("✗ «create» {}", msg_label(s, k)), FontSpec::mono(11.5)) + 40.0);
                    if let Some(n) = &msg.note {
                        need = need.max(m.width(n, FontSpec::sans(10.5).italic()) + 40.0);
                    }
                }
                // a self-call's label runs to the right of its loop: keep it clear of the next lifeline
                if &msg.from == prev && &msg.to == prev {
                    need = need.max(m.width(&format!("✗ {}", msg_label(s, k)), FontSpec::mono(11.5)) + 80.0);
                }
            }
            x += need.max((widths[prev] + w) / 2.0 + 40.0);
        }
        xs.insert(p.clone(), x);
    }
    SeqLayout { xs, widths, height: TOP + s.messages.len() as f64 * ROW + 40.0 }
}

pub fn sequence_scene(m: &dyn Measure, s: &Sequence, is_actor: &dyn Fn(&str) -> bool) -> ViewScene {
    let l = sequence_layout(m, s);
    let bottom = l.height;
    // activations: from a call to the matching return (or a short bar)
    struct Act {
        p: String,
        y1: f64,
        y2: f64,
        depth: usize,
    }
    let mut acts: Vec<Act> = vec![];
    let mut open: Vec<(String, usize, String)> = vec![];
    for (i, msg) in s.messages.iter().enumerate() {
        let y = TOP + i as f64 * ROW + 20.0;
        if msg.kind == MessageKind::Return {
            if let Some(k) = open.iter().rposition(|o| o.0 == msg.from && o.2 == msg.to) {
                let o = open.remove(k);
                let depth = open.iter().filter(|x| x.0 == o.0).count();
                acts.push(Act { p: o.0, y1: TOP + o.1 as f64 * ROW + 20.0, y2: y, depth });
            }
        } else if msg.kind == MessageKind::Sync && msg.from != msg.to {
            open.push((msg.to.clone(), i, msg.from.clone()));
        } else if msg.from == msg.to {
            acts.push(Act { p: msg.to.clone(), y1: y + 6.0, y2: y + 30.0, depth: 1 });
        }
    }
    for o in open {
        acts.push(Act { p: o.0, y1: TOP + o.1 as f64 * ROW + 20.0, y2: TOP + o.1 as f64 * ROW + 46.0, depth: 0 });
    }
    let mut scene = Scene::default();
    let mut hits = vec![];
    let mut points = vec![];
    let mut g = Group::new();
    for p in &s.participants {
        let x = l.xs[p];
        let w = l.widths[p];
        let actor = is_actor(p);
        let mut pg = Group::new().anim(Anim::Enter(format!("part:{p}")));
        pg.line([x, if actor { 52.0 } else { 36.0 }], [x, bottom], stroke(c::FAINT, 1.0).dash(5.0, 5.0));
        if actor {
            let mut ag = Group::at(x, 0.0);
            let st = stroke(c::TEXT, 1.5);
            ag.push(Prim::Circle { cx: 0.0, cy: -14.0, r: 8.0, fill: None, stroke: Some(st) });
            ag.line([0.0, -6.0], [0.0, 10.0], st);
            ag.line([-11.0, 0.0], [11.0, 0.0], st);
            ag.poly(vec![[-9.0, 23.0], [0.0, 10.0], [9.0, 23.0]], false, None, Some(st));
            ag.text(0.0, 40.0, p.clone(), FontSpec::sans(12.5), c::TEXT, Anchor::Middle);
            pg.add(ag);
        } else {
            pg.rect(x - w / 2.0, 0.0, w, 36.0, 6.0, Some(solid(c::NODE)), Some(stroke(c::BORDER, 1.2)));
            pg.text(x, 23.0, p.clone(), FontSpec::sans(13.5).bold(), c::TEXT, Anchor::Middle);
            hits.push(Hit { shape: HitShape::Rect { x1: x - w / 2.0, y1: 0.0, x2: x + w / 2.0, y2: 36.0 }, target: HitTarget::None, hover: Some(format!("class:{p}")), node: None, title: None });
        }
        g.add(pg);
    }
    for a in &acts {
        let x = l.xs.get(&a.p).copied().unwrap_or(0.0);
        g.rect(x - 6.0 + a.depth as f64 * 5.0, a.y1, 12.0, (a.y2 - a.y1).max(14.0), 0.0, Some(solid(Rgb::hex(0x30343d))), Some(stroke(c::BORDER, 1.0)));
    }
    for (i, msg) in s.messages.iter().enumerate() {
        let y = TOP + i as f64 * ROW + 20.0;
        let (Some(&x1), Some(&x2)) = (l.xs.get(&msg.from), l.xs.get(&msg.to)) else { continue };
        let bad = msg.problem.is_some();
        let color = if bad {
            c::CYCLE
        } else if msg.status == Status::Pruned {
            c::FAINT
        } else if msg.kind == MessageKind::Return {
            c::DIM
        } else {
            c::TEXT
        };
        let label = msg_label(s, i);
        let tgt = Target::Message { sequence: s.name.clone(), index: i };
        let hover = format!("message:{}#{i}", s.name);
        let mut mg = Group::new().opacity(if msg.status == Status::Pruned { 0.35 } else { 1.0 });
        if x1 == x2 {
            let pts = vec![[x1 + 6.0, y + 4.0], [x1 + 52.0, y + 4.0], [x1 + 52.0, y + 26.0], [x1 + 12.0, y + 26.0]];
            mg.poly(pts.clone(), false, None, Some(stroke(color, 1.3)));
            mg.poly(vec![[x1 + 12.0, y + 26.0], [x1 + 22.0, y + 21.0], [x1 + 22.0, y + 31.0]], true, Some(solid(color)), None);
            mg.text(x1 + 60.0, y + 8.0, format!("{}{label}", if bad { "✗ " } else { "" }), FontSpec::mono(11.5), color, Anchor::Start);
            hits.push(Hit { shape: HitShape::Line { pts, width: 14.0 }, target: HitTarget::Target(tgt.clone()), hover: Some(hover), node: None, title: None });
        } else {
            let dir = if x2 > x1 { 1.0 } else { -1.0 };
            let ex = x2 - dir * 7.0;
            let sx = x1 + dir * 7.0;
            let dashed = matches!(msg.kind, MessageKind::Return | MessageKind::Create);
            let s = stroke(color, if bad { 1.8 } else { 1.3 });
            mg.push(Prim::Path { cmds: vec![PathCmd::Move([sx, y]), PathCmd::Line([ex, y])], stroke: if dashed { s.dash(6.0, 4.0) } else { s } });
            if matches!(msg.kind, MessageKind::Sync | MessageKind::Create) {
                mg.poly(vec![[ex, y], [ex - dir * 11.0, y - 5.0], [ex - dir * 11.0, y + 5.0]], true, Some(solid(color)), None);
            } else {
                mg.poly(vec![[ex - dir * 11.0, y - 5.0], [ex, y], [ex - dir * 11.0, y + 5.0]], false, None, Some(stroke(color, 1.3)));
            }
            mg.text((x1 + x2) / 2.0, y - 7.0, format!("{}{}{label}", if bad { "✗ " } else { "" }, if msg.kind == MessageKind::Create { "«create» " } else { "" }), FontSpec::mono(11.5), color, Anchor::Middle);
            if let Some(n) = &msg.note {
                mg.text((x1 + x2) / 2.0, y + 15.0, n.clone(), FontSpec::sans(10.5).italic(), c::FAINT, Anchor::Middle);
            }
            hits.push(Hit { shape: HitShape::Line { pts: vec![[sx, y], [ex, y]], width: 14.0 }, target: HitTarget::Target(tgt.clone()), hover: Some(hover), node: None, title: None });
        }
        g.add(mg);
        points.push((tgt, [(x1 + x2) / 2.0, y]));
    }
    scene.root.add(g);
    scene.hits = hits;
    ViewScene { scene, points }
}

/// Collect targets whose point lies inside a world rectangle.
pub fn collect(points: &[(Target, [f64; 2])], x1: f64, y1: f64, x2: f64, y2: f64) -> Vec<Target> {
    points.iter().filter(|(_, p)| p[0] >= x1 && p[0] <= x2 && p[1] >= y1 && p[1] <= y2).map(|(t, _)| t.clone()).collect()
}
