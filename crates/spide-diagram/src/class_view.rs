//! The class (and domain) diagram as a scene: package hulls, routed relations with UML notation,
//! class boxes with members, diagnostics and status chips.

use crate::focus::Footprint;
use crate::insights::{concern_color, finding_color, Marks, Members, OverlapLink};
use crate::route::{self, deco_for, label_point, route_cmds, Deco, PathCmd, Pt, RBox, REdge, RZone, Route, RouteCache};
use crate::scene::{paint, solid, stroke, tag, Anchor, Anim, Group, Hit, HitShape, HitTarget, Prim, Scene, Xf};
use crate::visuals::{self, badges, border_point, c, class_box, f, heat_bucket, heat_color, layer_color, layer_name, node_scale, status_opacity, vis_label, BoxContext, BoxInput, ClassBox, FontSpec, Measure, Rect, Rgb, RowKind, MARKER_W, PAD_X};
use spide_core::metrics::{ClassMetrics, ImplicitEdge, Metrics};
use spide_core::model::{Class, Interaction, Package, Relation, RelationKind, Status};
use spide_core::Target;
use std::collections::{HashMap, HashSet};

pub struct Data<'a> {
    pub classes: &'a [Class],
    pub relations: &'a [Relation],
    pub packages: &'a [Package],
    pub metrics: Option<&'a Metrics>,
    pub implicit: &'a [ImplicitEdge],
}

const STRUCTURAL: [RelationKind; 3] = [RelationKind::Association, RelationKind::Aggregation, RelationKind::Composition];

fn type_tokens(t: &str) -> Vec<&str> {
    t.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).filter(|s| !s.is_empty()).collect()
}

/// Attributes that only restate a drawn association (exam note: "do not add variables for
/// associations you draw"), keyed by class, plus the role name each such relation gets.
pub fn association_roles(classes: &[Class], relations: &[Relation]) -> (HashMap<String, HashSet<String>>, HashMap<String, String>) {
    let mut attrs: HashMap<String, HashSet<String>> = HashMap::new();
    let mut roles: HashMap<String, String> = HashMap::new();
    let by_name: HashMap<&str, &Class> = classes.iter().map(|c| (c.name.as_str(), c)).collect();
    for r in relations {
        if !STRUCTURAL.contains(&r.kind) || r.status == Status::Pruned {
            continue;
        }
        let mut ends = vec![(&r.from, &r.to)];
        if r.kind == RelationKind::Association && !r.directed {
            ends.push((&r.to, &r.from));
        }
        for (owner, other) in ends {
            let Some(a) = by_name.get(owner.as_str()).and_then(|c| c.attributes.iter().find(|x| type_tokens(&x.ty).contains(&other.as_str()))) else { continue };
            attrs.entry(owner.clone()).or_default().insert(a.name.clone());
            roles.entry(r.id.clone()).or_insert_with(|| a.name.clone());
        }
    }
    (attrs, roles)
}

/// Attribute names each class inherits from its ancestors (not repeated at the design level).
fn inherited_attrs(classes: &[Class], relations: &[Relation]) -> HashMap<String, HashSet<String>> {
    let mut parents: HashMap<&str, Vec<&str>> = HashMap::new();
    for r in relations {
        if r.kind == RelationKind::Inheritance && r.status != Status::Pruned {
            parents.entry(&r.from).or_default().push(&r.to);
        }
    }
    let by_name: HashMap<&str, &Class> = classes.iter().map(|c| (c.name.as_str(), c)).collect();
    let mut out = HashMap::new();
    for c in classes {
        let mut seen = HashSet::new();
        let mut names = HashSet::new();
        let mut stack: Vec<&str> = parents.get(c.name.as_str()).cloned().unwrap_or_default();
        while let Some(p) = stack.pop() {
            if !seen.insert(p) {
                continue;
            }
            if let Some(pc) = by_name.get(p) {
                names.extend(pc.attributes.iter().map(|a| a.name.clone()));
            }
            stack.extend(parents.get(p).cloned().unwrap_or_default());
        }
        if !names.is_empty() {
            out.insert(c.name.clone(), names);
        }
    }
    out
}

/// Names of classes that are external themselves or live in an external package.
pub fn external_classes(classes: &[Class], packages: &[Package]) -> HashSet<String> {
    let ext: HashSet<&str> = packages.iter().filter(|p| p.external).map(|p| p.name.as_str()).collect();
    classes.iter().filter(|c| c.external || c.package.as_deref().is_some_and(|p| ext.contains(p))).map(|c| c.name.clone()).collect()
}

#[allow(clippy::too_many_arguments)]
pub fn boxes_for(
    m: &dyn Measure,
    classes: &[Class],
    show_methods: bool,
    level: u8,
    relations: &[Relation],
    purposes: Option<&HashMap<String, String>>,
    external: &HashSet<String>,
    keep: Option<&Marks>,
    members: Option<&Members>,
    reveal: &HashSet<String>,
) -> HashMap<String, ClassBox> {
    let assoc = association_roles(classes, relations).0;
    let inh = inherited_attrs(classes, relations);
    let mut out = HashMap::new();
    for cl in classes {
        // external classes: a compact header (they aren't designed here), unless everything is shown
        let open = reveal.contains(&cl.name);
        let lvl = if open {
            level.max(3)
        } else if external.contains(&cl.name) && level < 4 {
            1
        } else {
            level
        };
        let found: Vec<&String> = members.and_then(|m| m.get(&cl.name)).map(|m| m.keys().collect()).unwrap_or_default();
        let of = |k: &str| -> HashSet<String> { found.iter().filter_map(|x| x.strip_prefix(&format!("{k}:")).map(String::from)).collect() };
        let mut keep_m: HashSet<String> = keep.and_then(|k| k.get(&cl.name)).map(|k| k.keys().cloned().collect()).unwrap_or_default();
        if open {
            keep_m.extend(of("method"));
        }
        let ctx = BoxContext {
            inherited: if lvl == 3 { inh.get(&cl.name) } else { None },
            associations: if lvl == 3 { assoc.get(&cl.name) } else { None },
            keep: (!keep_m.is_empty()).then_some(keep_m),
            keep_attrs: open.then(|| of("attribute")),
            flagged: (!found.is_empty()).then(|| found.iter().map(|s| s.to_string()).collect()),
            mark_texts: (lvl >= 3).then(|| keep.and_then(|k| k.get(&cl.name)).map(|k| k.iter().map(|(n, mk)| (n.clone(), crate::insights::mark_text(mk))).collect())).flatten(),
        };
        out.insert(cl.name.clone(), class_box(m, &BoxInput::from(cl), show_methods, lvl, &ctx, purposes.and_then(|p| p.get(&cl.name)).map(|s| s.as_str())));
    }
    out
}

/// Room above (status chips) and below (metric badges) every class box. Part of its layout and
/// routing footprint, so chips never overlap a neighbour or a line.
pub const CHIP_BAND: f64 = 24.0;

/// Layout footprint (width, height) of a class: the scaled box plus its chip bands, at least as wide
/// as its badge row and top chips.
pub fn node_extent(m: &dyn Measure, cl: &Class, bx: &ClassBox, metrics: Option<&Metrics>) -> (f64, f64) {
    let cm = metrics.and_then(|x| x.classes.get(&cl.name));
    let s = node_scale(cm);
    let chip_w = |t: &str| m.width(t, f::SMALL) + 16.0;
    let bottom: f64 = badges(cm).iter().map(|b| chip_w(&b.text)).sum();
    let mut top = 0.0;
    if cl.aggregate_root {
        top += chip_w("◆ aggregate");
    }
    if cl.external {
        top += chip_w("«external»");
    }
    match cl.status {
        Status::Proposed => top += chip_w("+ proposed"),
        Status::Removing => top += chip_w("− proposed removal"),
        Status::Overruled => top += chip_w("⚠ overruled"),
        Status::Pruned => top += chip_w("pruned · click to restore"),
        _ => {}
    }
    (bx.w.max(bottom).max(top) * s, (bx.h + 2.0 * CHIP_BAND) * s)
}

/// The class diagram's layout: compound (package) layered placement of every class footprint
/// (`extents`, see [`node_extent`]). One function for the live canvas, exports and tests.
pub fn layout_classes(m: &dyn Measure, classes: &[Class], extents: &HashMap<String, (f64, f64)>, relations: &[Relation], packages: &[Package]) -> HashMap<String, [f64; 2]> {
    use crate::layout::{compound, GroupSpec, LayoutEdge, LayoutNode};
    let ext_pkgs: HashSet<&str> = packages.iter().filter(|p| p.external).map(|p| p.name.as_str()).collect();
    let nodes: Vec<LayoutNode> = classes
        .iter()
        .filter_map(|c| {
            let (w, h) = *extents.get(&c.name)?;
            Some(LayoutNode { id: c.name.clone(), w, h, group: c.package.clone(), external: c.external || c.package.as_deref().is_some_and(|p| ext_pkgs.contains(p)) })
        })
        .collect();
    let edges: Vec<LayoutEdge> = relations.iter().filter(|r| r.status != Status::Pruned).map(|r| LayoutEdge { from: r.from.clone(), to: r.to.clone(), kind: r.kind.as_str().into() }).collect();
    let groups: Vec<GroupSpec> = packages.iter().map(|p| GroupSpec { id: p.name.clone(), rank: p.layer.map(|l| l.rank()).unwrap_or(1), external: p.external, min_w: package_min_width(m, &p.name, Some(p)) }).collect();
    compound(&nodes, &edges, &groups)
}

pub fn rect_of(name: &str, pos: &HashMap<String, [f64; 2]>, boxes: &HashMap<String, ClassBox>, metrics: Option<&Metrics>) -> Option<Rect> {
    let p = pos.get(name)?;
    let b = boxes.get(name)?;
    let s = node_scale(metrics.and_then(|m| m.classes.get(name)));
    Some(Rect { x: p[0], y: p[1], w: b.w * s, h: b.h * s })
}

#[derive(Debug, Clone, PartialEq)]
pub struct EdgeStyle {
    pub color: Rgb,
    pub width: f64,
    pub opacity: f64,
    pub dashed: bool,
    /// cycle tension animation
    pub strained: bool,
    pub ghost: bool,
    pub removing: bool,
}

impl EdgeStyle {
    pub fn new(color: Rgb, width: f64, opacity: f64) -> Self {
        Self { color, width, opacity, dashed: false, strained: false, ghost: false, removing: false }
    }
}

pub fn relation_style(r: &Relation, metrics: Option<&Metrics>) -> (EdgeStyle, Option<String>) {
    match r.status {
        Status::Proposed => return (EdgeStyle { dashed: true, ghost: true, ..EdgeStyle::new(c::GHOST, 1.6, 0.85) }, None),
        Status::Removing => return (EdgeStyle { dashed: true, removing: true, ..EdgeStyle::new(c::REMOVING, 1.4, 0.6) }, None),
        Status::Pruned => return (EdgeStyle::new(c::FAINT, 1.0, 0.3), None),
        _ => {}
    }
    let cyc = metrics.is_some_and(|m| m.cycle_edges.contains(&r.id));
    let lv = metrics.is_some_and(|m| m.layer_violations.contains(&r.id));
    let rule = metrics.and_then(|m| m.edge_rules.get(&r.id)).cloned().unwrap_or_else(|| "layer ↑".into());
    if cyc {
        return (EdgeStyle { strained: true, ..EdgeStyle::new(c::CYCLE, 2.4, 1.0) }, Some(if lv { format!("cycle · {rule}") } else { "cycle".into() }));
    }
    if lv {
        return (EdgeStyle::new(c::WARN, 2.0, 1.0), Some(rule));
    }
    if r.status == Status::Overruled {
        return (EdgeStyle::new(c::WARN, 1.5, 0.9), None);
    }
    (EdgeStyle::new(c::EDGE, 1.4, 0.95), None)
}

/// Assign bend offsets to parallel edges between the same pair.
pub fn bends<'a>(edges: impl Iterator<Item = (&'a str, &'a str, &'a str)>) -> HashMap<String, f64> {
    let mut groups: Vec<(String, Vec<(&str, &str, &str)>)> = vec![];
    for e in edges {
        let k = if e.1 <= e.2 { format!("{}|{}", e.1, e.2) } else { format!("{}|{}", e.2, e.1) };
        match groups.iter_mut().find(|g| g.0 == k) {
            Some(g) => g.1.push(e),
            None => groups.push((k, vec![e])),
        }
    }
    let mut out = HashMap::new();
    for (_, g) in groups {
        let n = g.len() as f64;
        for (i, e) in g.iter().enumerate() {
            let b = if g.len() == 1 { 0.0 } else { i as f64 - (n - 1.0) / 2.0 };
            // keep orientation consistent for reversed edges
            out.insert(e.0.to_string(), if e.1 < e.2 { b } else { -b });
        }
    }
    out
}

/// The kind of line an edge draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeKind {
    Rel(RelationKind),
    Implicit,
    Flow,
}

pub struct EdgeSpec<'a> {
    pub from: Rect,
    pub to: Rect,
    pub kind: EdgeKind,
    pub directed: bool,
    pub label: Option<String>,
    pub from_mult: Option<&'a str>,
    pub to_mult: Option<&'a str>,
    pub bend: f64,
    pub style: EdgeStyle,
    pub target: Option<Target>,
    pub hover: Option<String>,
    pub warn: Option<String>,
    pub route: Option<&'a Route>,
    pub status: Option<Status>,
    /// animation key (stable per edge)
    pub key: String,
    /// where the label goes (default: the middle of the route's longest segment)
    pub label_at: Option<Pt>,
}

fn add(a: Pt, b: Pt) -> Pt {
    [a[0] + b[0], a[1] + b[1]]
}
fn mul(a: Pt, k: f64) -> Pt {
    [a[0] * k, a[1] * k]
}
fn norm(a: Pt) -> Pt {
    let l = a[0].hypot(a[1]);
    let l = if l == 0.0 { 1.0 } else { l };
    [a[0] / l, a[1] / l]
}
fn perp(a: Pt) -> Pt {
    [-a[1], a[0]]
}
fn unit(a: Pt, b: Pt) -> Pt {
    norm([b[0] - a[0], b[1] - a[1]])
}

struct Geo {
    s: Pt,
    e: Pt,
    ctrl: Pt,
    dir_s: Pt,
    dir_e: Pt,
    lp: Pt,
    bent: bool,
}

fn edge_geometry(from: Rect, to: Rect, bend: f64) -> Geo {
    let (c1, c2) = ([from.x, from.y], [to.x, to.y]);
    let n = perp(norm([c2[0] - c1[0], c2[1] - c1[1]]));
    let mid = [(c1[0] + c2[0]) / 2.0, (c1[1] + c2[1]) / 2.0];
    let ctrl = add(mid, mul(n, bend * 34.0));
    let aim1 = if bend != 0.0 { ctrl } else { c2 };
    let aim2 = if bend != 0.0 { ctrl } else { c1 };
    let s = border_point(from, aim1[0], aim1[1], 2.0);
    let e = border_point(to, aim2[0], aim2[1], 2.0);
    let dir_s = norm([aim1[0] - s[0], aim1[1] - s[1]]);
    let dir_e = norm([e[0] - aim2[0], e[1] - aim2[1]]);
    let lp = if bend != 0.0 { [0.25 * s[0] + 0.5 * ctrl[0] + 0.25 * e[0], 0.25 * s[1] + 0.5 * ctrl[1] + 0.25 * e[1]] } else { [(s[0] + e[0]) / 2.0, (s[1] + e[1]) / 2.0] };
    Geo { s, e, ctrl, dir_s, dir_e, lp, bent: bend != 0.0 }
}

/// One relation line with its UML decoration, multiplicities and label.
pub fn edge(m: &dyn Measure, p: &EdgeSpec, hits: &mut Vec<Hit>) -> Group {
    let (mut g, labels) = edge_parts(m, p, hits);
    g.add(labels);
    g
}

/// The line (with its decorations) and, separately, its text (label, multiplicities, warnings), so
/// callers can draw all text above every box and line.
pub fn edge_parts(m: &dyn Measure, p: &EdgeSpec, hits: &mut Vec<Hit>) -> (Group, Group) {
    let pts = p.route.map(|r| &r.points).filter(|p| p.len() >= 2);
    let geo = match pts {
        Some(pts) => Geo { s: pts[0], e: pts[pts.len() - 1], ctrl: [0.0, 0.0], dir_s: unit(pts[0], pts[1]), dir_e: unit(pts[pts.len() - 2], pts[pts.len() - 1]), lp: p.label_at.unwrap_or_else(|| label_point(pts)), bent: false },
        None => edge_geometry(p.from, p.to, p.bend),
    };
    let st = &p.style;
    let color = st.color;
    let mut start = geo.s;
    let mut end = geo.e;
    let mut head = Group::new();
    let (n_e, n_s) = (perp(geo.dir_e), perp(geo.dir_s));
    let hs = stroke(color, 1.4);
    match p.kind {
        EdgeKind::Rel(RelationKind::Inheritance) | EdgeKind::Rel(RelationKind::Realization) => {
            let base = add(geo.e, mul(geo.dir_e, -15.0));
            head.poly(vec![geo.e, add(base, mul(n_e, 8.0)), add(base, mul(n_e, -8.0))], true, Some(solid(c::BG)), Some(hs));
            end = base;
        }
        EdgeKind::Rel(k @ (RelationKind::Composition | RelationKind::Aggregation)) => {
            let m1 = add(geo.s, mul(geo.dir_s, 11.0));
            let tip = add(geo.s, mul(geo.dir_s, 22.0));
            let fill = if k == RelationKind::Composition { color } else { c::BG };
            head.poly(vec![geo.s, add(m1, mul(n_s, 6.5)), tip, add(m1, mul(n_s, -6.5))], true, Some(solid(fill)), Some(hs));
            start = tip;
        }
        _ => {}
    }
    let open_arrow = (p.kind == EdgeKind::Rel(RelationKind::Association) && p.directed) || p.kind == EdgeKind::Rel(RelationKind::Dependency) || p.kind == EdgeKind::Flow || (p.kind == EdgeKind::Implicit && st.width > 1.0);
    if open_arrow {
        let b = add(geo.e, mul(geo.dir_e, -11.0));
        head.poly(vec![add(b, mul(n_e, 6.0)), geo.e, add(b, mul(n_e, -6.0))], false, None, Some(hs));
    }
    let cmds: Vec<PathCmd> = match pts {
        Some(pts) => {
            let mut all = vec![start];
            all.extend_from_slice(&pts[1..pts.len() - 1]);
            all.push(end);
            route_cmds(&all, &p.route.unwrap().hops, 10.0, 5.0)
        }
        None if geo.bent => vec![PathCmd::Move(start), PathCmd::Quad(geo.ctrl, end)],
        None => vec![PathCmd::Move(start), PathCmd::Line(end)],
    };
    let dashed = st.dashed || matches!(p.kind, EdgeKind::Rel(RelationKind::Dependency) | EdgeKind::Rel(RelationKind::Realization) | EdgeKind::Implicit);
    let dash = if p.kind == EdgeKind::Implicit {
        Some([2.0, 5.0])
    } else if dashed {
        Some([7.0, 5.0])
    } else {
        None
    };
    let mut g = Group::new().opacity(st.opacity as f32);
    if p.status == Some(Status::Proposed) {
        g = g.tags(tag::PROPOSED);
    }
    if p.target.is_some() || p.hover.is_some() {
        hits.push(Hit {
            shape: HitShape::Line { pts: route::flatten(&cmds), width: 14.0 },
            target: p.target.clone().map(HitTarget::Target).unwrap_or(HitTarget::None),
            hover: p.hover.clone(),
            node: None,
            title: None,
        });
    }
    if st.strained {
        // the glow's own opacity (.18) is what the pulse animation replaces
        let mut glow = Group::new().tags(tag::DIAG).opacity(0.18).anim(Anim::Pulse { key: format!("glow:{}", p.key), period: 1.1, times: 6 });
        glow.push(Prim::Path { cmds: cmds.clone(), stroke: stroke(color, (st.width + 6.0) as f32) });
        g.add(glow);
    }
    let mut line = Group::new();
    if st.strained {
        line = line.anim(Anim::Strain(p.key.clone()));
    } else if st.ghost {
        line = line.anim(Anim::Flow(p.key.clone()));
    }
    line.push(Prim::Path { cmds, stroke: stroke(color, st.width as f32).dash_opt(dash) });
    g.add(line);
    g.add(head);
    let mut lab = Group::new().opacity(st.opacity as f32);
    if p.status == Some(Status::Proposed) {
        lab = lab.tags(tag::PROPOSED);
    }
    let g_line = g;
    let mut g = lab;
    let mult = |g: &mut Group, q: Pt, dir: Pt, text: &str, side: f64| {
        let at = add(add(q, mul(dir, 16.0)), mul(perp(dir), 11.0 * side));
        g.text(at[0], at[1] + 4.0, text, FontSpec::sans(11.0), c::DIM, Anchor::Middle);
    };
    if let Some(t) = p.from_mult.filter(|t| !t.is_empty()) {
        mult(&mut g, geo.s, geo.dir_s, t, 1.0);
    }
    if let Some(t) = p.to_mult.filter(|t| !t.is_empty()) {
        mult(&mut g, geo.e, mul(geo.dir_e, -1.0), t, -1.0);
    }
    if p.label.is_some() || p.warn.is_some() {
        let mut lg = Group::at(geo.lp[0], geo.lp[1]);
        if let Some(l) = &p.label {
            let lw = m.width(l, f::SMALL) + 10.0;
            lg.rect(-lw / 2.0, -9.0, lw, 17.0, 4.0, Some(paint(c::BG, 0.85)), None);
            lg.push(Prim::Text { x: 0.0, y: 4.0, text: l.clone(), font: f::SMALL.italic(), color: c::DIM, opacity: 1.0, anchor: Anchor::Middle, underline: false });
        }
        if let Some(w) = &p.warn {
            let mut wg = Group::new().tags(tag::DIAG);
            wg.text(0.0, if p.label.is_some() { -13.0 } else { 4.0 }, w.clone(), f::SMALL.bold(), color, Anchor::Middle);
            lg.add(wg);
        }
        g.add(lg);
    }
    if st.removing {
        g.text(geo.lp[0], geo.lp[1] + 4.0, "×", FontSpec::sans(13.0), c::REMOVING, Anchor::Middle);
    }
    (g_line, g)
}

/// A small rounded label (status, badges).
pub fn chip(m: &dyn Measure, g: &mut Group, x: f64, y: f64, text: &str, color: Rgb, end: bool) -> f64 {
    let cw = m.width(text, f::SMALL) + 12.0;
    let x0 = if end { x - cw } else { x };
    let mut cg = Group::at(x0, y);
    cg.rect(0.0, 0.0, cw, 17.0, 8.5, Some(paint(c::BG, 0.95)), Some(stroke(color, 1.0).opacity(0.95)));
    cg.text(cw / 2.0, 12.0, text, f::SMALL, color, Anchor::Middle);
    g.add(cg);
    cw
}

fn row_color(s: Status) -> Rgb {
    match s {
        Status::Proposed => c::GHOST,
        Status::Removing => c::REMOVING,
        Status::Pruned => c::FAINT,
        Status::Overruled => c::WARN,
        _ => c::TEXT,
    }
}

pub struct NodeProps<'a> {
    pub cls: &'a Class,
    pub bx: &'a ClassBox,
    pub x: f64,
    pub y: f64,
    pub scale: f64,
    pub m: Option<&'a ClassMetrics>,
    pub dim: bool,
    /// part of the spotlit ghost proposal: draw at full strength
    pub spotlit: bool,
    pub focus: bool,
    /// outline colour (stress test, sync lens)
    pub ring: Option<Rgb>,
    pub ring_dashed: bool,
    /// exists only in the code (sync lens)
    pub phantom: bool,
    pub pinned: bool,
    pub show_methods: bool,
    /// static rendering (previews/export): no animation
    pub still: bool,
    pub layer_color: Option<Rgb>,
    /// zoomed far out: draw the frame and a large name only (same box size, so nothing moves)
    pub compact: bool,
    pub marks: Option<&'a HashMap<String, crate::insights::MethodMark>>,
    pub concerns: Option<&'a HashMap<String, String>>,
    /// sync lens: member rows that differ from the code
    pub sync_rows: Option<&'a HashMap<String, Rgb>>,
    pub chips: Option<&'a Vec<String>>,
    pub members: Option<&'a HashMap<String, crate::insights::MemberDiagnosis>>,
    pub revealed: bool,
    /// hovered member row (`attribute:x` / `method:x`): its visibility tag shows
    pub hovered_row: Option<&'a str>,
    /// review: ghost elements report their concrete class/member target instead of their proposal
    pub concrete_targets: bool,
}

pub fn class_node(ms: &dyn Measure, p: &NodeProps, parent: Xf, hits: &mut Vec<Hit>) -> Group {
    let (cls, bx, mm) = (p.cls, p.bx, p.m);
    let (w, h) = (bx.w, bx.h);
    let st = cls.status;
    let ghost = st == Status::Proposed;
    let removing = st == Status::Removing;
    let pruned = st == Status::Pruned;
    let heat = if ghost { 0.0 } else { mm.map(|m| m.heat).unwrap_or(0.0) };
    let hc = heat_color(heat);
    let border = if ghost {
        c::GHOST
    } else if removing {
        c::REMOVING
    } else if pruned {
        c::FAINT
    } else if heat > 0.18 {
        hc
    } else {
        c::BORDER
    };
    let target = if ghost && cls.proposal.is_some() && !p.concrete_targets { Target::Proposal { id: cls.proposal.clone().unwrap() } } else { Target::Class { name: cls.name.clone() } };
    let italic = cls.is_abstract || cls.stereotype.as_deref() == Some("interface");
    let bs = if ghost || pruned { vec![] } else { badges(mm) };
    let opacity = (if p.spotlit { 1.0 } else { status_opacity(st) }) * if p.dim { 0.28 } else { 1.0 } * if p.phantom { 0.5 } else { 1.0 };
    let hb = heat_bucket(heat);
    // member findings whose rows aren't drawn (detail level, Domain view): a chip says so and reveals them
    let drawn: HashSet<String> = bx.rows.iter().filter(|r| r.kind != RowKind::Note).map(|r| format!("{}:{}", r.kind.key(), r.name)).collect();
    let hidden: Vec<&String> = if p.compact || ghost || pruned { vec![] } else { p.members.map(|m| m.keys().filter(|k| !drawn.contains(*k)).collect()).unwrap_or_default() };
    let revealable: Vec<&&String> = hidden.iter().filter(|k| k.starts_with("attribute:") || p.show_methods).collect();

    let mut outer = Group { tx: p.x - p.scale * w / 2.0, ty: p.y - p.scale * h / 2.0, scale: p.scale, opacity: opacity as f32, ..Group::default() };
    if ghost {
        outer = outer.tags(tag::PROPOSED);
    }
    let xf = parent.then(&outer);
    let anim = if p.still {
        Anim::None
    } else if ghost {
        Anim::Ghost(format!("node:{}", cls.name))
    } else {
        Anim::Enter(format!("node:{}", cls.name))
    };
    let mut g = Group::new().anim(anim);
    let world = |x1: f64, y1: f64, x2: f64, y2: f64| {
        let a = xf.apply([x1, y1]);
        let b = xf.apply([x2, y2]);
        HitShape::Rect { x1: a[0], y1: a[1], x2: b[0], y2: b[1] }
    };
    // heat halo: plain strokes
    if hb > 0 && !pruned {
        let mut halo = Group::new().tags(tag::DIAG);
        if !p.still && heat > 0.55 {
            halo = halo.anim(Anim::Pulse { key: format!("halo:{}", cls.name), period: 1.6, times: 4 });
        }
        halo.rect(-7.0, -7.0, w + 14.0, h + 14.0, 14.0, None, Some(stroke(hc, (8.0 + heat * 6.0) as f32).opacity((0.07 + heat * 0.12) as f32)));
        halo.rect(-3.0, -3.0, w + 6.0, h + 6.0, 10.0, None, Some(stroke(hc, 3.0).opacity((0.15 + heat * 0.3) as f32)));
        g.add(halo);
    }
    if p.focus {
        g.rect(-5.0, -5.0, w + 10.0, h + 10.0, 12.0, None, Some(stroke(c::ACCENT, 1.5).opacity(0.7)));
    }
    if let Some(ring) = p.ring {
        let mut rg = Group::new().opacity(0.9).anim(Anim::Pulse { key: format!("ring:{}", cls.name), period: 1.6, times: 4 });
        let s = stroke(ring, 2.2);
        rg.rect(-7.0, -7.0, w + 14.0, h + 14.0, 13.0, None, Some(if p.ring_dashed { s.dash(7.0, 5.0) } else { s }));
        g.add(rg);
    }
    if p.pinned {
        g.text(w - 8.0, 14.0, "📌", FontSpec::sans(11.0), c::DIM, Anchor::End);
    }
    if p.phantom {
        chip(ms, &mut g, w - 4.0, -23.0, "only in code", c::DIM, true);
    }
    let body = stroke(border, if ghost { 1.5 } else { (1.2 + heat * 1.6) as f32 });
    g.rect(0.0, 0.0, w, h, 8.0, Some(solid(c::NODE)), Some(if ghost || removing { body.dash(6.0, 4.0) } else { body }));
    hits.push(Hit { shape: world(0.0, 0.0, w, h), target: HitTarget::Target(target), hover: Some(format!("class:{}", cls.name)), node: Some(cls.name.clone()), title: None });
    let name_font = if italic { f::NAME_ITALIC } else { f::NAME };
    if p.compact {
        g.rect(0.0, 0.0, w, h, 8.0, Some(paint(p.layer_color.unwrap_or(c::NODE_HEADER), if p.layer_color.is_some() { 0.14 } else { 0.6 })), None);
        let fs = ((w - 16.0) / (cls.name.chars().count().max(4) as f64) * 1.7).clamp(16.0, 30.0);
        let font = FontSpec { size: fs as f32, ..name_font };
        g.text(w / 2.0, h / 2.0 + 8.0, cls.name.clone(), font, if ghost { c::GHOST } else { c::TEXT }, Anchor::Middle);
    } else {
        // header: rounded top corners
        let hh = bx.header_h;
        let mut hdr = vec![[0.0, hh], [0.0, 8.0]];
        for k in 0..=6 {
            let a = std::f64::consts::PI + std::f64::consts::FRAC_PI_2 * k as f64 / 6.0;
            hdr.push([8.0 + 8.0 * a.cos(), 8.0 + 8.0 * a.sin()]);
        }
        for k in 0..=6 {
            let a = -std::f64::consts::FRAC_PI_2 + std::f64::consts::FRAC_PI_2 * k as f64 / 6.0;
            hdr.push([w - 8.0 + 8.0 * a.cos(), 8.0 + 8.0 * a.sin()]);
        }
        hdr.push([w, hh]);
        let hfill = match p.layer_color {
            Some(l) => paint(l, 0.16),
            None => solid(c::NODE_HEADER),
        };
        g.poly(hdr, true, Some(hfill), None);
        if let Some(s) = &bx.stereo {
            g.text(w / 2.0, 15.0, s.clone(), f::STEREO, c::DIM, Anchor::Middle);
        }
        g.text(w / 2.0, if bx.stereo.is_some() { 31.0 } else { 20.0 }, cls.name.clone(), name_font, if ghost { c::GHOST } else { c::TEXT }, Anchor::Middle);
        g.line([0.0, bx.attr_y], [w, bx.attr_y], stroke(border, 1.0).opacity(0.7));
        if p.show_methods {
            g.line([0.0, bx.meth_y], [w, bx.meth_y], stroke(border, 1.0).opacity(0.7));
        }
        for r in &bx.rows {
            if r.kind == RowKind::Note {
                g.push(Prim::Text { x: PAD_X, y: r.y, text: r.text.clone(), font: f::MEMBER.italic(), color: c::DIM, opacity: 1.0, anchor: Anchor::Start, underline: false });
                continue;
            }
            if r.cont {
                // continuation of a wrapped member: same colour and decoration, no markers or hits
                let color = row_color(r.status);
                let font = if r.is_abstract { f::MEMBER.italic() } else { f::MEMBER };
                g.push(Prim::Text { x: PAD_X, y: r.y, text: r.text.clone(), font, color, opacity: 1.0, anchor: Anchor::Start, underline: r.is_static });
                if matches!(r.status, Status::Removing | Status::Pruned) {
                    let tw = ms.width(&r.text, f::MEMBER);
                    g.line([PAD_X, r.y - 4.0], [PAD_X + tw, r.y - 4.0], stroke(row_color(r.status), 1.2));
                }
                continue;
            }
            let (proposal, visibility) = if r.kind == RowKind::Attribute {
                cls.attributes.iter().find(|a| a.name == r.name).map(|a| (a.proposal.clone(), Some(a.visibility))).unwrap_or((None, None))
            } else {
                cls.methods.iter().find(|x| x.name == r.name).map(|a| (a.proposal.clone(), Some(a.visibility))).unwrap_or((None, None))
            };
            let tgt = match (&proposal, r.status) {
                (Some(id), Status::Proposed) if !p.concrete_targets => Target::Proposal { id: id.clone() },
                _ if r.kind == RowKind::Attribute => Target::Attribute { class: cls.name.clone(), name: r.name.clone() },
                _ => Target::Method { class: cls.name.clone(), name: r.name.clone() },
            };
            let row_key = format!("{}:{}", r.kind.key(), r.name);
            let mut rg = Group::new();
            if r.status == Status::Proposed {
                rg = rg.tags(tag::PROPOSED);
            }
            let tw = ms.width(&r.text, f::MEMBER);
            let mark = if r.kind == RowKind::Method { p.marks.and_then(|m| m.get(&r.name)) } else { None };
            let concern = if r.kind == RowKind::Method { p.concerns.and_then(|m| m.get(&r.name)) } else { None };
            let diag = if r.status == Status::Pruned { None } else { p.members.and_then(|m| m.get(&row_key)) };
            let dc = diag.map(|d| finding_color(d.severity)).unwrap_or(c::DIM);
            let mut title = if r.status == Status::Pruned { Some("pruned · click to restore".to_string()) } else { None };
            if let Some(d) = diag {
                let mut dg = Group::new().tags(tag::DIAG);
                dg.rect(2.0, r.y - 12.0, w - 4.0, 16.0, 3.0, Some(paint(dc, 0.1)), None);
                dg.rect(w - 4.0, r.y - 11.0, 2.0, 14.0, 1.0, Some(paint(dc, 0.85)), None);
                let count = if d.issues.len() > 1 { d.issues.len().to_string() } else { String::new() };
                dg.text(w - 9.0, r.y, format!("{}{count}", d.family.icon()), FontSpec::sans(11.0), dc, Anchor::End);
                title = Some(format!("{}: {}", d.family.label(), d.issues.iter().map(|i| i.message.as_str()).collect::<Vec<_>>().join("; ")));
                rg.add(dg);
            }
            if let Some(cn) = concern {
                rg.rect(3.0, r.y - 11.0, 3.0, 14.0, 1.5, Some(solid(concern_color(cn).unwrap_or(c::DIM))), None);
            }
            if let Some(sc) = p.sync_rows.and_then(|m| m.get(&row_key)) {
                rg.rect(3.0, r.y - 7.0, 6.0, 6.0, 3.0, Some(solid(*sc)), None);
            }
            if let Some(mk) = mark {
                let mut mg = Group::new().tags(tag::DIAG);
                // inside the row's reserved right end: never drawn over a neighbour or an edge
                let right = w - 6.0 - if diag.is_some() { MARKER_W } else { 0.0 };
                mg.text(right, r.y, crate::insights::mark_text(mk), FontSpec::sans(11.0), c::WARN, Anchor::End);
                rg.add(mg);
                if title.is_none() {
                    title = Some(mk.message.clone());
                }
            }
            let color = concern.and_then(|c| concern_color(c)).unwrap_or_else(|| row_color(r.status));
            let font = if r.is_abstract { f::MEMBER.italic() } else { f::MEMBER };
            rg.push(Prim::Text { x: PAD_X, y: r.y, text: r.text.clone(), font, color, opacity: 1.0, anchor: Anchor::Start, underline: r.is_static });
            if matches!(r.status, Status::Removing | Status::Pruned) {
                rg.line([PAD_X, r.y - 4.0], [PAD_X + tw, r.y - 4.0], stroke(row_color(r.status), 1.2));
            }
            if let (Some(v), false, true) = (visibility, p.still, p.hovered_row == Some(row_key.as_str())) {
                let vis = vis_label(v);
                let tag_w = ms.width(vis, FontSpec::sans(9.5)) + 10.0;
                let mut vg = Group::at(w - 6.0 - if diag.is_some() { MARKER_W } else { 0.0 } - tag_w, r.y - 11.0).tags(tag::TRANSIENT);
                vg.rect(0.0, 0.0, tag_w, 14.0, 7.0, Some(solid(c::BG)), Some(stroke(c::ACCENT, 0.8)));
                vg.text(tag_w / 2.0, 10.0, vis, FontSpec::sans(9.5), c::ACCENT, Anchor::Middle);
                rg.add(vg);
            }
            g.add(rg);
            hits.push(Hit { shape: world(2.0, r.y - 12.0, w - 2.0, r.y + 4.0), target: HitTarget::Target(tgt), hover: Some(format!("{}:{}.{}", r.kind.key(), cls.name, r.name)), node: Some(cls.name.clone()), title });
        }
    }
    // status chips
    if ghost {
        chip(ms, &mut g, w - 4.0, -23.0, "+ proposed", c::GHOST, true);
    }
    if removing {
        chip(ms, &mut g, w - 4.0, -23.0, "− proposed removal", c::REMOVING, true);
    }
    if st == Status::Overruled {
        chip(ms, &mut g, w - 4.0, -23.0, "⚠ overruled", c::WARN, true);
    }
    if cls.aggregate_root && !pruned {
        chip(ms, &mut g, 4.0, -23.0, "◆ aggregate", c::DIM, false);
    }
    if cls.external && !pruned {
        chip(ms, &mut g, 4.0, -23.0, "«external»", c::FAINT, false);
    }
    if pruned {
        g.line([-6.0, h / 2.0], [w + 6.0, h / 2.0], stroke(c::CYCLE, 2.0));
        chip(ms, &mut g, w - 4.0, -23.0, "pruned · click to restore", c::FAINT, true);
    }
    if removing {
        g.line([-6.0, -6.0], [w + 6.0, h + 6.0], stroke(c::REMOVING, 1.5).opacity(0.7));
    }
    let mut dg = Group::new().tags(tag::DIAG);
    let mut bxp = 0.0;
    let mut all: Vec<(String, Rgb)> = bs.iter().map(|b| (b.text.clone(), b.color)).collect();
    if let Some(ch) = p.chips {
        all.extend(ch.iter().map(|c| (format!("⁂ {c}"), c::LAVENDER)));
    }
    for (text, color) in &all {
        let cw = chip(ms, &mut dg, bxp, h + 6.0, text, *color, false);
        bxp += cw + 4.0;
    }
    let has_members = p.members.is_some_and(|m| !m.is_empty());
    if (!hidden.is_empty() || (p.revealed && has_members)) && !p.still {
        let text = if p.revealed {
            "▴ fold member findings".to_string()
        } else if !revealable.is_empty() {
            format!("⚠ {} hidden member issue{}", revealable.len(), if revealable.len() > 1 { "s" } else { "" })
        } else {
            format!("⚠ {} method issue{} · inspect", hidden.len(), if hidden.len() > 1 { "s" } else { "" })
        };
        let cw = ms.width(&text, f::SMALL) + 12.0;
        let reveal = !revealable.is_empty() || p.revealed;
        let mut rc = Group::at(bxp, h + 6.0).tags(tag::TRANSIENT);
        rc.rect(0.0, 0.0, cw, 17.0, 8.5, Some(solid(c::BG)), Some(stroke(c::WARN, 1.0).dash(3.0, 2.0)));
        rc.text(cw / 2.0, 12.0, text, f::SMALL, c::WARN, Anchor::Middle);
        dg.add(rc);
        let title = if reveal {
            if p.revealed {
                "Hide the revealed members again"
            } else {
                "Show the members these findings are about (this class only)"
            }
        } else {
            "Methods are hidden in this view; hover for the findings or open the Class view"
        };
        hits.push(Hit {
            shape: world(bxp, h + 6.0, bxp + cw, h + 23.0),
            target: if reveal { HitTarget::Reveal(cls.name.clone()) } else { HitTarget::Target(Target::Class { name: cls.name.clone() }) },
            hover: Some(format!("class:{}", cls.name)),
            node: None,
            title: Some(title.into()),
        });
    }
    g.add(dg);
    outer.add(g);
    outer
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Lens {
    #[default]
    None,
    Concerns,
    Overlap,
    /// Spide Project: design ↔ code sync
    Sync,
}

#[derive(Default)]
pub struct Stress {
    pub impacted: HashSet<String>,
    pub ripple: HashSet<String>,
}

/// Design ↔ code sync status per class (Spide Project sync lens).
#[derive(Default)]
pub struct SyncSets {
    /// the design changed: implement in the code
    pub design_ahead: HashSet<String>,
    /// the code changed: readjust the design
    pub code_ahead: HashSet<String>,
    /// both changed
    pub conflict: HashSet<String>,
    /// no baseline yet, or only on one side
    pub unreconciled: HashSet<String>,
    /// classes drawn from the code only (ghosts)
    pub phantom: HashSet<String>,
    /// class → member row (`attribute:x` / `method:x`) → status colour
    pub rows: HashMap<String, HashMap<String, Rgb>>,
}

/// Sync lens colours.
pub const SYNC_DESIGN_AHEAD: Rgb = c::GHOST;
pub const SYNC_CODE_AHEAD: Rgb = Rgb::hex(0xa78bfa);
pub const SYNC_CONFLICT: Rgb = c::CYCLE;
pub const SYNC_UNRECONCILED: Rgb = c::WARN;

#[derive(Default)]
pub struct Opts<'a> {
    pub pos: HashMap<String, [f64; 2]>,
    pub boxes: HashMap<String, ClassBox>,
    pub show_methods: bool,
    pub still: bool,
    /// hovered class → focus mode
    pub focus_class: Option<&'a str>,
    /// classes to highlight (e.g. preview focus)
    pub highlight: Option<&'a HashSet<String>>,
    pub stress: Option<&'a Stress>,
    pub sync: Option<&'a SyncSets>,
    pub pinned: HashSet<String>,
    /// package focus: everything outside dims
    pub focus_set: Option<&'a HashSet<String>>,
    /// export without diagnostics: neutral edges and boxes
    pub plain: bool,
    /// bump to force a full re-route (the layout settled)
    pub route_epoch: u64,
    pub proposal_focus: Option<&'a Footprint>,
    pub hide_packages: bool,
    /// semantic zoom: 0 = packages only, 1 = class names only, ≥ 2 = as the detail level says
    pub zoom_cap: u8,
    pub level: u8,
    pub show_implicit: bool,
    pub marks: Option<&'a Marks>,
    pub lens: Lens,
    pub concerns: Option<&'a HashMap<String, HashMap<String, String>>>,
    pub overlap: Option<&'a Vec<OverlapLink>>,
    pub scattered: Option<&'a HashMap<String, Vec<String>>>,
    pub members: Option<&'a Members>,
    pub revealed: Option<&'a HashSet<String>>,
    pub hovered_row: Option<(&'a str, &'a str)>,
    /// review: ghost elements report concrete class/member/relation targets instead of their proposal
    pub concrete_targets: bool,
}

/// Route caches per edge family (relations, transient edges, package deps).
#[derive(Default)]
pub struct Routes {
    pub rel: RouteCache,
    pub extra: RouteCache,
    pub pkg: RouteCache,
}

/// A package frame's title (name · layer · «external» · system).
pub fn package_label(name: &str, pk: Option<&Package>) -> String {
    let mut label_parts = vec![name.to_string()];
    if let Some(l) = pk.and_then(|p| p.layer) {
        if layer_name(l) != name {
            label_parts.push(layer_name(l).into());
        }
    }
    if pk.is_some_and(|p| p.external) {
        label_parts.push("«external»".into());
    }
    if let Some(s) = pk.and_then(|p| p.system.clone()) {
        label_parts.push(s);
    }
    label_parts.join("  ·  ")
}

/// Minimum frame width so the package title fits inside it.
pub fn package_min_width(m: &dyn Measure, name: &str, pk: Option<&Package>) -> f64 {
    m.width(&package_label(name, pk), FontSpec::sans(12.0).bold()) + 40.0
}

/// The drawn package frame around its classes, widened (symmetrically) to fit its title.
pub fn package_hull(rs: &[Rect], min_w: f64) -> [f64; 4] {
    let [mut x1, y1, mut x2, y2] = hull_of(rs);
    if x2 - x1 < min_w {
        let d = (min_w - (x2 - x1)) / 2.0;
        x1 -= d;
        x2 += d;
    }
    [x1, y1, x2, y2]
}

/// Package frame around class footprints (boxes with their chip bands), as the layout reserves it.
pub fn hull_of(rs: &[Rect]) -> [f64; 4] {
    use crate::layout::HULL;
    let x1 = rs.iter().map(|r| r.x - r.w / 2.0).fold(f64::INFINITY, f64::min) - HULL.left;
    let y1 = rs.iter().map(|r| r.y - r.h / 2.0).fold(f64::INFINITY, f64::min) - HULL.top;
    let x2 = rs.iter().map(|r| r.x + r.w / 2.0).fold(f64::NEG_INFINITY, f64::max) + HULL.right;
    let y2 = rs.iter().map(|r| r.y + r.h / 2.0).fold(f64::NEG_INFINITY, f64::max) + HULL.bottom;
    [x1, y1, x2, y2]
}

/// A label position along a route whose box (`w` wide, `up` above and `down` below the point) is
/// clear of every class footprint in `avoid` and of the labels already `taken`. Tries the longest
/// segments first, their middle first. None: no free spot (the caller keeps the default).
pub fn place_label(pts: &[Pt], w: f64, up: f64, down: f64, avoid: &[[f64; 4]], taken: &mut Vec<[f64; 4]>) -> Option<Pt> {
    let mut segs: Vec<(Pt, Pt, f64)> = pts.windows(2).map(|s| (s[0], s[1], (s[1][0] - s[0][0]).abs() + (s[1][1] - s[0][1]).abs())).collect();
    segs.sort_by(|a, b| b.2.total_cmp(&a.2));
    let free = |r: [f64; 4], taken: &Vec<[f64; 4]>| !avoid.iter().chain(taken.iter()).any(|o| r[0] < o[2] && o[0] < r[2] && r[1] < o[3] && o[1] < r[3]);
    for (a, b, _) in &segs {
        for t in [0.5, 0.32, 0.68, 0.18, 0.82] {
            let q = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
            let r = [q[0] - w / 2.0 - 2.0, q[1] - up - 2.0, q[0] + w / 2.0 + 2.0, q[1] + down + 2.0];
            if free(r, taken) {
                taken.push(r);
                return Some(q);
            }
        }
    }
    // crowded: beside the line instead of on it (left/right of vertical, above/below horizontal runs)
    for (a, b, _) in &segs {
        let vertical = (a[0] - b[0]).abs() < 0.5;
        for t in [0.5, 0.3, 0.7] {
            let q = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
            let offs: [Pt; 2] = if vertical { [[w / 2.0 + 6.0, 0.0], [-w / 2.0 - 6.0, 0.0]] } else { [[0.0, -(down + 8.0)], [0.0, up + 8.0]] };
            for o in offs {
                let q = [q[0] + o[0], q[1] + o[1]];
                let r = [q[0] - w / 2.0 - 2.0, q[1] - up - 2.0, q[0] + w / 2.0 + 2.0, q[1] + down + 2.0];
                if free(r, taken) {
                    taken.push(r);
                    return Some(q);
                }
            }
        }
    }
    None
}

/// A class's footprint: its box plus the chip bands above and below.
pub fn footprint(r: Rect, scale: f64) -> Rect {
    Rect { h: r.h + 2.0 * CHIP_BAND * scale, ..r }
}

pub fn build(ms: &dyn Measure, d: &Data, o: &Opts, routes: &mut Routes) -> Scene {
    let cm = d.metrics.map(|m| &m.classes);
    let mut rects: HashMap<&str, Rect> = HashMap::new();
    for cl in d.classes {
        if let Some(r) = rect_of(&cl.name, &o.pos, &o.boxes, d.metrics) {
            rects.insert(&cl.name, r);
        }
    }
    // what edge text must not cover: every box and the chips actually drawn around it
    let mut avoid: Vec<[f64; 4]> = vec![];
    for cl in d.classes {
        let Some(r) = rects.get(cl.name.as_str()) else { continue };
        let s = node_scale(cm.and_then(|m| m.get(&cl.name)));
        let (x1, y1, x2, y2) = (r.x - r.w / 2.0, r.y - r.h / 2.0, r.x + r.w / 2.0, r.y + r.h / 2.0);
        avoid.push([x1 - 2.0, y1 - 2.0, x2 + 2.0, y2 + 2.0]);
        let chip_w = |t: &str| ms.width(t, f::SMALL) + 16.0;
        let bottom: f64 = if o.plain { 0.0 } else { badges(cm.and_then(|m| m.get(&cl.name))).iter().map(|b| chip_w(&b.text)).sum() };
        if bottom > 0.0 {
            avoid.push([x1, y2, x1 + bottom * s, y2 + CHIP_BAND * s]);
        }
        let top = cl.status != Status::Accepted || cl.external || cl.aggregate_root;
        if top {
            avoid.push([x1, y1 - CHIP_BAND * s, x2, y1]);
        }
    }
    let mut taken: Vec<[f64; 4]> = vec![];
    let focus = o.focus_class.filter(|f| rects.contains_key(f));
    let mut neighbours: HashSet<&str> = HashSet::new();
    if let Some(fc) = focus {
        neighbours.insert(fc);
        for r in d.relations {
            if r.from == fc {
                neighbours.insert(&r.to);
            }
            if r.to == fc {
                neighbours.insert(&r.from);
            }
        }
        for e in d.implicit {
            if e.from == fc {
                neighbours.insert(&e.to);
            }
            if e.to == fc {
                neighbours.insert(&e.from);
            }
        }
    }
    let pf = o.proposal_focus;
    let in_proposal = |n: &str| pf.is_some_and(|f| f.changed.contains(n) || f.anchors.contains(n));
    let bend_map = bends(d.relations.iter().map(|r| (r.id.as_str(), r.from.as_str(), r.to.as_str())));
    let in_stress = |n: &str| o.stress.is_some_and(|s| s.impacted.contains(n) || s.ripple.contains(n));
    let level = o.level;
    let cap = o.zoom_cap.min(level);
    let roles = if level == 3 { Some(association_roles(d.classes, d.relations).1) } else { None };
    // zoomed far out: packages become solid boxes; their classes and class edges are hidden
    let overview = cap == 0;
    let pkg_of: HashMap<&str, Option<&str>> = d.classes.iter().map(|c| (c.name.as_str(), c.package.as_deref())).collect();
    let hidden_in_pkg = |n: &str| overview && pkg_of.get(n).copied().flatten().is_some();

    // ------------------------------------------------------------- orthogonal routing
    let mut r_boxes: Vec<RBox> = vec![];
    let mut zones: Vec<RZone> = vec![];
    let mut hull_boxes: Vec<RBox> = vec![];
    let mut by_pkg: Vec<(&str, Vec<&str>, Vec<Rect>)> = vec![];
    for cl in d.classes {
        let Some(r) = rects.get(cl.name.as_str()) else { continue };
        if !overview || cl.package.is_none() {
            // chips above / badges below are part of the footprint: live canvas and export route alike
            let pad = CHIP_BAND * node_scale(d.metrics.and_then(|m| m.classes.get(&cl.name)));
            r_boxes.push(RBox { id: cl.name.clone(), x: r.x, y: r.y, w: r.w, h: r.h, pad_top: pad, pad_bottom: pad });
        }
        if let Some(p) = &cl.package {
            let fr = footprint(*r, node_scale(d.metrics.and_then(|m| m.classes.get(&cl.name))));
            match by_pkg.iter_mut().find(|g| g.0 == p) {
                Some(g) => {
                    g.1.push(&cl.name);
                    g.2.push(fr);
                }
                None => by_pkg.push((p, vec![&cl.name], vec![fr])),
            }
        }
    }
    let pkg_min_w = |name: &str| package_min_width(ms, name, d.packages.iter().find(|x| x.name == name));
    for (name, names, rs) in &by_pkg {
        let [x1, y1, x2, y2] = package_hull(rs, pkg_min_w(name));
        zones.push(RZone { id: name.to_string(), x1, y1, x2, y2, members: names.iter().map(|s| s.to_string()).collect() });
        hull_boxes.push(RBox::new(*name, (x1 + x2) / 2.0, (y1 + y2) / 2.0, x2 - x1, y2 - y1));
    }
    let rel_edges: Vec<REdge> = if overview {
        vec![]
    } else {
        d.relations
            .iter()
            .filter(|r| rects.contains_key(r.from.as_str()) && rects.contains_key(r.to.as_str()))
            .map(|r| {
                let (fd, td, v) = deco_for(r.kind.as_str(), r.directed);
                REdge { id: r.id.clone(), from: r.from.clone(), to: r.to.clone(), from_deco: fd, to_deco: td, vertical: v }
            })
            .collect()
    };
    let rel_routes = routes.rel.update(&r_boxes, &rel_edges, &zones, o.route_epoch).clone();
    // transient edges (hover-implicit, lenses) are routed separately so hovering never re-routes the diagram
    let mut extra_edges: Vec<REdge> = vec![];
    if !overview {
        for e in d.implicit {
            if (o.show_implicit || focus.is_some_and(|f| e.from == f || e.to == f)) && rects.contains_key(e.from.as_str()) && rects.contains_key(e.to.as_str()) && e.from != e.to {
                extra_edges.push(REdge::new(format!("imp{}>{}", e.from, e.to), &e.from, &e.to).deco(Deco::None, if e.in_cycle { Deco::Arrow } else { Deco::None }, false));
            }
        }
        if o.lens == Lens::Overlap {
            for l in o.overlap.into_iter().flatten() {
                if rects.contains_key(l.from.as_str()) && rects.contains_key(l.to.as_str()) && l.from != l.to {
                    extra_edges.push(REdge::new(format!("ov{}>{}", l.from, l.to), &l.from, &l.to));
                }
            }
        }
    }
    let extra_routes = routes.extra.update(&r_boxes, &extra_edges, &zones, o.route_epoch).clone();
    let pkg_deps = d.metrics.map(|m| m.package_deps.as_slice()).unwrap_or(&[]);
    let pkg_edges: Vec<REdge> = if overview { pkg_deps.iter().filter(|d| d.from != d.to).map(|d| REdge::new(format!("pd{}>{}", d.from, d.to), &d.from, &d.to).deco(Deco::None, Deco::Arrow, false)).collect() } else { vec![] };
    let pkg_routes = routes.pkg.update(if overview { &hull_boxes } else { &[] }, &pkg_edges, &[], o.route_epoch).clone();

    let mut scene = Scene::default();
    let mut hits: Vec<Hit> = vec![];
    // packages
    let mut pkg_layer = Group::new();
    let mut hull_of_pkg: HashMap<&str, Rect> = HashMap::new();
    if !o.hide_packages {
        for (name, _, rs) in &by_pkg {
            let pk = d.packages.iter().find(|x| x.name == *name);
            let color = match pk {
                Some(Package { layer: Some(l), .. }) => layer_color(*l),
                Some(Package { external: true, .. }) => Rgb::hex(0x6b7280),
                _ => c::EDGE,
            };
            let [x1, y1, x2, y2] = package_hull(rs, pkg_min_w(name));
            let label = package_label(name, pk);
            hull_of_pkg.insert(name, Rect { x: (x1 + x2) / 2.0, y: (y1 + y2) / 2.0, w: x2 - x1, h: y2 - y1 });
            let proposed = pk.is_some_and(|p| p.status == Status::Proposed);
            let hit = Hit { shape: HitShape::Rect { x1, y1, x2, y2 }, target: HitTarget::Target(Target::Package { name: name.to_string() }), hover: Some(format!("package:{name}")), node: None, title: None };
            if overview {
                let fs = ((x2 - x1) / (name.chars().count().max(5) as f64) * 1.3).clamp(48.0, 140.0);
                let mut g = Group::new().tags(tag::PKG);
                let s = stroke(color, 3.0).opacity(0.6);
                g.rect(x1, y1, x2 - x1, y2 - y1, 22.0, Some(paint(color, 0.12)), Some(if proposed { s.dash(10.0, 8.0) } else { s }));
                g.text((x1 + x2) / 2.0, (y1 + y2) / 2.0, name.to_string(), FontSpec::sans(fs as f32).bold(), color, Anchor::Middle);
                let sub = format!("{} class{}{}", rs.len(), if rs.len() == 1 { "" } else { "es" }, pk.and_then(|p| p.layer).map(|l| format!(" · {}", layer_name(l))).unwrap_or_default());
                g.text((x1 + x2) / 2.0, (y1 + y2) / 2.0 + fs * 0.9, sub, FontSpec::sans((fs * 0.5) as f32), c::DIM, Anchor::Middle);
                pkg_layer.add(g);
                hits.push(hit);
                continue;
            }
            let lit = pf.is_some_and(|f| f.packages.contains(*name));
            let op = if focus.is_some() {
                0.4
            } else if let Some(f) = pf {
                if f.packages.contains(*name) {
                    1.0
                } else {
                    0.25
                }
            } else {
                1.0
            };
            let mut g = Group::new().tags(tag::PKG).opacity(op);
            let s = stroke(color, if lit { 1.8 } else { 1.0 }).opacity(if lit { 0.8 } else { 0.35 });
            g.rect(x1, y1, x2 - x1, y2 - y1, 18.0, Some(paint(color, if lit { 0.09 } else { 0.045 })), Some(if proposed { s.dash(6.0, 5.0) } else { s }));
            g.push(Prim::Text { x: x1 + 16.0, y: y1 + 20.0, text: label, font: FontSpec::sans(12.0).bold(), color, opacity: 0.85, anchor: Anchor::Start, underline: false });
            pkg_layer.add(g);
            hits.push(hit);
        }
    }
    scene.root.add(pkg_layer);

    let mut edges = Group::new();
    // edge text is drawn above every box and line
    let mut labels = Group::new();
    if overview {
        for dep in pkg_deps {
            let (Some(a), Some(b)) = (hull_of_pkg.get(dep.from.as_str()), hull_of_pkg.get(dep.to.as_str())) else { continue };
            let color = if dep.in_cycle {
                c::CYCLE
            } else if dep.layer_violation {
                c::WARN
            } else {
                c::EDGE
            };
            let id = format!("pd{}>{}", dep.from, dep.to);
            let spec = EdgeSpec {
                from: *a,
                to: *b,
                kind: EdgeKind::Rel(RelationKind::Dependency),
                directed: true,
                label: Some(dep.count.to_string()),
                from_mult: None,
                to_mult: None,
                bend: 0.0,
                style: EdgeStyle { strained: dep.in_cycle, ..EdgeStyle::new(color, 2.0 + (dep.count.min(8) as f64) * 0.6, 0.9) },
                target: None,
                hover: None,
                warn: dep.layer_violation.then(|| "layer ↑".into()),
                route: pkg_routes.get(&id),
                status: None,
                key: id.clone(),
                label_at: None,
            };
            {
            let (l, t) = edge_parts(ms, &spec, &mut hits);
            edges.add(l);
            labels.add(t);
        }
        }
    }
    // overlap lens: classes that duplicate each other, methods dealing with another class's concept
    if o.lens == Lens::Overlap && !overview {
        for l in o.overlap.into_iter().flatten() {
            let (Some(a), Some(b)) = (rects.get(l.from.as_str()), rects.get(l.to.as_str())) else { continue };
            let id = format!("ov{}>{}", l.from, l.to);
            let spec = EdgeSpec {
                from: *a,
                to: *b,
                kind: EdgeKind::Implicit,
                directed: false,
                label: Some(l.label.clone()),
                from_mult: None,
                to_mult: None,
                bend: -0.4,
                style: EdgeStyle { dashed: true, ..EdgeStyle::new(c::LAVENDER, 3.0 + l.strength * 4.0, 0.9) },
                target: None,
                hover: None,
                warn: None,
                route: extra_routes.get(&id),
                status: None,
                key: id.clone(),
                label_at: None,
            };
            {
            let (l, t) = edge_parts(ms, &spec, &mut hits);
            edges.add(l);
            labels.add(t);
        }
        }
    }
    if !overview {
        for e in d.implicit {
            // implicit dependencies are clutter unless asked for (/implicit) or the class is in focus
            let touches = focus.is_some_and(|f| e.from == f || e.to == f);
            if !o.show_implicit && !touches {
                continue;
            }
            let (Some(a), Some(b)) = (rects.get(e.from.as_str()), rects.get(e.to.as_str())) else { continue };
            let dim = focus.is_some() && !touches;
            let id = format!("imp{}>{}", e.from, e.to);
            let spec = EdgeSpec {
                from: *a,
                to: *b,
                kind: EdgeKind::Implicit,
                directed: false,
                label: None,
                from_mult: None,
                to_mult: None,
                bend: 0.35,
                style: EdgeStyle {
                    strained: e.in_cycle,
                    ..EdgeStyle::new(
                        if e.in_cycle { c::CYCLE } else { Rgb::hex(0xc8cede) },
                        if e.in_cycle { 1.4 } else { 1.0 },
                        if dim {
                            0.05
                        } else if e.in_cycle {
                            0.7
                        } else {
                            0.22
                        },
                    )
                },
                target: None,
                hover: Some(format!("implicit:{}>{}", e.from, e.to)),
                warn: None,
                route: extra_routes.get(&id),
                status: None,
                key: id.clone(),
                label_at: None,
            };
            {
            let (l, t) = edge_parts(ms, &spec, &mut hits);
            edges.add(l);
            labels.add(t);
        }
        }
    }
    for r in d.relations {
        if hidden_in_pkg(&r.from) || hidden_in_pkg(&r.to) {
            continue;
        }
        let (Some(a), Some(b)) = (rects.get(r.from.as_str()), rects.get(r.to.as_str())) else { continue };
        let (mut style, warn) = relation_style(r, if o.plain { None } else { d.metrics });
        if focus.is_some_and(|f| r.from != f && r.to != f) {
            style.opacity *= 0.15;
        }
        if o.stress.is_some() && !(in_stress(&r.from) && in_stress(&r.to)) {
            style.opacity *= 0.15;
        }
        if o.focus_set.is_some_and(|s| !(s.contains(&r.from) || s.contains(&r.to))) {
            style.opacity *= 0.15;
        }
        if let Some(f) = pf {
            if f.relations.contains(&r.id) {
                style.opacity = 1.0;
                style.width += 1.0;
            } else {
                style.opacity *= if in_proposal(&r.from) && in_proposal(&r.to) { 0.35 } else { 0.08 };
            }
        }
        let tgt = match (&r.proposal, r.status) {
            (Some(id), Status::Proposed) if !o.concrete_targets => Target::Proposal { id: id.clone() },
            _ => Target::Relation { id: r.id.clone() },
        };
        let l = r.label.clone().or_else(|| roles.as_ref().and_then(|ro| ro.get(&r.id).cloned()));
        let label = if r.interaction != Interaction::Sync {
            let i = match r.interaction {
                Interaction::Async => "async",
                Interaction::Event => "event",
                Interaction::Sync => "sync",
            };
            Some(format!("«{i}»{}", l.map(|l| format!(" {l}")).unwrap_or_default()))
        } else {
            l
        };
        let label_at = rel_routes.get(&r.id).filter(|rt| rt.points.len() >= 2 && (label.is_some() || warn.is_some())).and_then(|rt| {
            let lw = label.as_ref().map(|l| ms.width(l, f::SMALL) + 10.0).unwrap_or(0.0).max(warn.as_ref().map(|w| ms.width(w, f::SMALL.bold()) + 4.0).unwrap_or(0.0));
            let up = if label.is_some() && warn.is_some() { 24.0 } else { 10.0 };
            place_label(&rt.points, lw, up, 9.0, &avoid, &mut taken)
        });
        let spec = EdgeSpec {
            from: *a,
            to: *b,
            kind: EdgeKind::Rel(r.kind),
            directed: r.directed,
            label,
            from_mult: r.from_mult.as_deref(),
            to_mult: r.to_mult.as_deref(),
            bend: bend_map.get(&r.id).copied().unwrap_or(0.0),
            style,
            target: Some(tgt),
            hover: Some(format!("relation:{}", r.id)),
            warn,
            route: rel_routes.get(&r.id),
            status: Some(r.status),
            key: r.id.clone(),
            label_at,
        };
        {
            let (l, t) = edge_parts(ms, &spec, &mut hits);
            edges.add(l);
            labels.add(t);
        }
    }
    scene.root.add(edges);

    let mut nodes = Group::new();
    for cl in d.classes {
        let (Some(pos), Some(bx)) = (o.pos.get(&cl.name), o.boxes.get(&cl.name)) else { continue };
        if hidden_in_pkg(&cl.name) {
            continue;
        }
        let pk = cl.package.as_ref().and_then(|p| d.packages.iter().find(|x| &x.name == p));
        let n = cl.name.as_str();
        let m = cm.and_then(|c| c.get(n));
        let ring = if pf.is_some_and(|f| f.changed.contains(n)) {
            Some(c::GHOST)
        } else if o.stress.is_some_and(|s| s.impacted.contains(n)) {
            Some(c::CYCLE)
        } else if o.stress.is_some_and(|s| s.ripple.contains(n)) {
            Some(c::WARN)
        } else if o.sync.is_some_and(|s| s.conflict.contains(n)) {
            Some(SYNC_CONFLICT)
        } else if o.sync.is_some_and(|s| s.design_ahead.contains(n)) {
            Some(SYNC_DESIGN_AHEAD)
        } else if o.sync.is_some_and(|s| s.code_ahead.contains(n)) {
            Some(SYNC_CODE_AHEAD)
        } else if o.sync.is_some_and(|s| s.unreconciled.contains(n)) {
            Some(SYNC_UNRECONCILED)
        } else {
            None
        };
        let props = NodeProps {
            cls: cl,
            bx,
            x: pos[0],
            y: pos[1],
            scale: node_scale(m),
            m: if o.plain { None } else { m },
            dim: (focus.is_some() && !neighbours.contains(n)) || (o.stress.is_some() && !in_stress(n)) || o.focus_set.is_some_and(|s| !s.contains(n)) || (pf.is_some() && !in_proposal(n)),
            spotlit: pf.is_some_and(|f| f.changed.contains(n)),
            focus: focus == Some(n) || o.highlight.is_some_and(|h| h.contains(n)) || pf.is_some_and(|f| f.anchors.contains(n)),
            ring,
            ring_dashed: o.sync.is_some_and(|s| s.unreconciled.contains(n)) || pf.is_some_and(|f| f.changed.contains(n)),
            phantom: o.sync.is_some_and(|s| s.phantom.contains(n)),
            pinned: o.pinned.contains(n),
            show_methods: o.show_methods,
            still: o.still,
            layer_color: pk.and_then(|p| p.layer).map(layer_color),
            compact: cap <= 1 && level >= 2,
            marks: if level >= 3 && !o.plain { o.marks.and_then(|m| m.get(n)) } else { None },
            concerns: if o.lens == Lens::Concerns { o.concerns.and_then(|m| m.get(n)) } else { None },
            sync_rows: o.sync.and_then(|s| s.rows.get(n)),
            chips: if o.lens == Lens::Overlap { o.scattered.and_then(|m| m.get(n)) } else { None },
            members: if o.plain || cap <= 1 { None } else { o.members.and_then(|m| m.get(n)) },
            revealed: o.revealed.is_some_and(|r| r.contains(n)),
            hovered_row: o.hovered_row.filter(|h| h.0 == n).map(|h| h.1),
            concrete_targets: o.concrete_targets,
        };
        nodes.add(class_node(ms, &props, Xf::ID, &mut hits));
    }
    scene.root.add(nodes);
    scene.root.add(labels);
    scene.hits = hits;
    scene
}

/// `visuals` re-export for callers building legends etc.
pub use visuals::health_color;
