//! Obsidian Canvas export ([JSON Canvas 1.0](https://jsoncanvas.org/spec/1.0/)): the export scene's
//! layout as editable Markdown cards, groups and labelled edges.
//!
//! Positions come from the very scene the PNG/SVG exports draw (same layout, same detail level), so
//! a canvas matches the image. Cards hold the text the scene draws inside each node; diagnostics are
//! never included. A canvas edge is a plain arrow, so the UML notation (hollow triangles, diamonds,
//! dashes) is spelled out in its label. Class and package (layer) colours are kept.

use crate::route::Pt;
use crate::scene::{tag, Anchor, Group, HitShape, Prim, Scene, Xf};
use crate::views::{self, C4Level};
use crate::visuals::{layer_color, Family, FontSpec, Measure, Rgb};
use serde::Serialize;
use spide_core::derive::{PackageNode, SystemView};
use spide_core::metrics::PackageDep;
use spide_core::model::{Class, Design, Interaction, MessageKind, Package, Relation, RelationKind, Sequence, Status};
use spide_core::sequence::{self, Event};
use std::collections::{HashMap, HashSet};

/// The model behind an export scene (where nodes and edges come from).
pub enum Source<'a> {
    /// Class diagram or domain model (`conceptual`: generalization reads "is a").
    Classes { classes: &'a [Class], relations: &'a [Relation], packages: &'a [Package], package_deps: &'a [PackageDep], conceptual: bool },
    Packages { packages: &'a [PackageNode], deps: &'a [PackageDep] },
    System { view: &'a SystemView, level: C4Level },
    UseCases { design: &'a Design },
    Sequence { seq: &'a Sequence, is_actor: &'a dyn Fn(&str) -> bool },
}

#[derive(Debug, Clone, Default)]
pub struct CanvasOptions {
    /// include proposed (ghost) elements
    pub ghosts: bool,
    /// include package frames (groups)
    pub packages: bool,
    /// Markdown of a title card above the diagram (title, view, date)
    pub header: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Node {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub x: i64,
    pub y: i64,
    pub width: i64,
    pub height: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Edge {
    pub id: String,
    pub from_node: String,
    pub from_side: &'static str,
    pub from_end: &'static str,
    pub to_node: String,
    pub to_side: &'static str,
    pub to_end: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct JsonCanvas {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

impl JsonCanvas {
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }
}

/// The diagram as an Obsidian canvas (`.canvas` JSON). None: nothing to draw.
pub fn to_json_canvas(scene: &Scene, m: &dyn Measure, src: &Source, o: &CanvasOptions) -> Option<String> {
    build(scene, m, src, o).map(|c| c.to_json())
}

/// The canvas as data (tests inspect it).
pub fn build(scene: &Scene, m: &dyn Measure, src: &Source, o: &CanvasOptions) -> Option<JsonCanvas> {
    let (nodes, edges) = match src {
        Source::Sequence { seq, is_actor } => sequence_parts(m, seq, *is_actor),
        _ => scene_parts(scene, m, src, o),
    };
    if !nodes.iter().any(|n| !n.group) {
        return None;
    }
    Some(finish(nodes, edges, o))
}

// ------------------------------------------------------------------ geometry and output

type R4 = [f64; 4];

/// A node in scene coordinates.
struct N {
    id: String,
    r: R4,
    group: bool,
    text: String,
    label: Option<String>,
    color: Option<Rgb>,
}

/// An edge between node ids; `ends`: the drawn route's first and last point, if known.
struct E {
    id: String,
    from: String,
    to: String,
    label: Option<String>,
    arrow: bool,
    ends: Option<(Pt, Pt)>,
    sides: Option<(&'static str, &'static str)>,
}

fn area(r: R4) -> f64 {
    (r[2] - r[0]) * (r[3] - r[1])
}

fn contains(r: R4, p: Pt, tol: f64) -> bool {
    p[0] >= r[0] - tol && p[0] <= r[2] + tol && p[1] >= r[1] - tol && p[1] <= r[3] + tol
}

/// The side of `r` nearest to `p` (where a route leaves or enters the box).
fn side_at(r: R4, p: Pt) -> &'static str {
    let d = [((p[1] - r[1]).abs(), "top"), ((p[0] - r[2]).abs(), "right"), ((p[1] - r[3]).abs(), "bottom"), ((p[0] - r[0]).abs(), "left")];
    d.iter().min_by(|a, b| a.0.total_cmp(&b.0)).map(|x| x.1).unwrap_or("top")
}

/// Facing sides of two boxes (no route known).
fn facing(a: R4, b: R4) -> (&'static str, &'static str) {
    let (ca, cb) = ([(a[0] + a[2]) / 2.0, (a[1] + a[3]) / 2.0], [(b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0]);
    let (dx, dy) = (cb[0] - ca[0], cb[1] - ca[1]);
    let horizontal = dx.abs() / ((a[2] - a[0]) + (b[2] - b[0])).max(1.0) > dy.abs() / ((a[3] - a[1]) + (b[3] - b[1])).max(1.0);
    match (horizontal, dx >= 0.0, dy >= 0.0) {
        (true, true, _) => ("right", "left"),
        (true, false, _) => ("left", "right"),
        (false, _, true) => ("bottom", "top"),
        (false, _, false) => ("top", "bottom"),
    }
}

// Obsidian renders card text at 16 px (code 14 px) with 24 px lines; the scene uses 11–14 px.
const OB_LINE: f64 = 24.0;
const OB_HR: f64 = 40.0;
const OB_PAD_X: f64 = 48.0;
const OB_PAD_Y: f64 = 28.0;
const OB_LABEL_CHAR: f64 = 7.6;

/// The size a Markdown card needs in Obsidian (estimate).
pub fn card_need(md: &str) -> (f64, f64) {
    let mut w: f64 = 0.0;
    let mut h = OB_PAD_Y;
    for line in md.lines() {
        if line == "---" {
            h += OB_HR;
            continue;
        }
        h += OB_LINE;
        let code_spans = line.matches('`').count() as f64 / 2.0;
        let mut lw = 0.0;
        let mut code = false;
        let mut esc = false;
        for ch in line.chars() {
            if esc {
                esc = false;
                lw += 8.4;
                continue;
            }
            match ch {
                '`' => code = !code,
                '\\' if !code => esc = true,
                '*' if !code => {}
                _ => lw += if code { 8.6 } else { 8.4 },
            }
        }
        w = w.max(lw + code_spans * 8.0);
    }
    (w + OB_PAD_X, h)
}

/// Scale the scene layout so every card fits its text at Obsidian's size, then serialise.
fn finish(mut nodes: Vec<N>, edges: Vec<E>, o: &CanvasOptions) -> JsonCanvas {
    let mut k: f64 = 1.0;
    for n in nodes.iter().filter(|n| !n.group) {
        let (nw, nh) = card_need(&n.text);
        k = k.max(nw / (n.r[2] - n.r[0]).max(1.0)).max(nh / (n.r[3] - n.r[1]).max(1.0));
    }
    // sequence messages: their labels sit between the two lifelines, keep room for them
    for e in edges.iter().filter(|e| e.sides.is_some()) {
        let (Some(l), Some((a, b))) = (&e.label, e.ends) else { continue };
        let gap = (b[0] - a[0]).abs().max((b[1] - a[1]).abs());
        if gap > 1.0 {
            k = k.max(l.chars().count() as f64 * OB_LABEL_CHAR / gap);
        }
    }
    let k = k.clamp(1.0, 3.0);
    let min_x = nodes.iter().map(|n| n.r[0]).fold(f64::INFINITY, f64::min);
    let min_y = nodes.iter().map(|n| n.r[1]).fold(f64::INFINITY, f64::min);
    // groups first (largest at the bottom), then cards: JSON Canvas z-order is the array order
    nodes.sort_by(|a, b| b.group.cmp(&a.group).then(area(b.r).total_cmp(&area(a.r))));
    let by_id: HashMap<&str, R4> = nodes.iter().map(|n| (n.id.as_str(), n.r)).collect();
    let out_edges: Vec<Edge> = edges
        .iter()
        .filter_map(|e| {
            let (a, b) = (*by_id.get(e.from.as_str())?, *by_id.get(e.to.as_str())?);
            let (fs, ts) = match (e.sides, e.ends) {
                (Some(s), _) => s,
                (None, Some((p, q))) if e.from != e.to => (side_at(a, p), side_at(b, q)),
                _ if e.from == e.to => ("right", "right"),
                _ => facing(a, b),
            };
            Some(Edge { id: e.id.clone(), from_node: e.from.clone(), from_side: fs, from_end: "none", to_node: e.to.clone(), to_side: ts, to_end: if e.arrow { "arrow" } else { "none" }, label: e.label.clone() })
        })
        .collect();
    let mut out: Vec<Node> = nodes
        .iter()
        .map(|n| {
            let x = ((n.r[0] - min_x) * k).round() as i64;
            let y = ((n.r[1] - min_y) * k).round() as i64;
            Node {
                id: n.id.clone(),
                kind: if n.group { "group" } else { "text" },
                x,
                y,
                width: (((n.r[2] - min_x) * k).round() as i64 - x).max(1),
                height: (((n.r[3] - min_y) * k).round() as i64 - y).max(1),
                text: (!n.group).then(|| n.text.clone()),
                label: n.label.clone(),
                color: n.color.map(|c| c.css()),
            }
        })
        .collect();
    if let Some(h) = o.header.as_ref().filter(|h| !h.trim().is_empty()) {
        let (w, hh) = card_need(h);
        let (w, hh) = (w.ceil().max(320.0) as i64, hh.ceil() as i64);
        out.push(Node { id: "title".into(), kind: "text", x: 0, y: -hh - 48, width: w, height: hh, text: Some(h.clone()), label: None, color: None });
    }
    JsonCanvas { nodes: out, edges: out_edges }
}

// ------------------------------------------------------------------ Markdown from scene text

#[derive(Clone)]
struct T {
    x0: f64,
    base: f64,
    mid: Pt,
    text: String,
    font: FontSpec,
    anchor: Anchor,
}

/// Horizontal rule (compartment separator) in world coordinates.
struct Sep {
    y: f64,
    x0: f64,
    x1: f64,
}

fn collect(g: &Group, parent: Xf, m: &dyn Measure, skip: u8, ts: &mut Vec<T>, seps: &mut Vec<Sep>) {
    if g.tags & skip != 0 || g.opacity <= 0.01 {
        return;
    }
    let xf = parent.then(g);
    for p in &g.children {
        match p {
            Prim::Text { x, y, text, font, anchor, .. } if !text.trim().is_empty() => {
                let w = m.width(text, *font);
                let x0 = match anchor {
                    Anchor::Start => *x,
                    Anchor::Middle => x - w / 2.0,
                    Anchor::End => x - w,
                };
                let a = xf.apply([x0, *y]);
                let mid = xf.apply([x0 + w / 2.0, y - font.size as f64 * 0.3]);
                ts.push(T { x0: a[0], base: a[1], mid, text: text.clone(), font: *font, anchor: *anchor });
            }
            Prim::Poly { pts, closed: false, stroke: Some(_), .. } if pts.len() == 2 && (pts[0][1] - pts[1][1]).abs() < 0.5 => {
                let (a, b) = (xf.apply(pts[0]), xf.apply(pts[1]));
                seps.push(Sep { y: a[1], x0: a[0].min(b[0]), x1: a[0].max(b[0]) });
            }
            Prim::Group(c) => collect(c, xf, m, skip, ts, seps),
            _ => {}
        }
    }
}

/// Escape Markdown syntax in plain card text.
fn esc_md(s: &str) -> String {
    let mut o = String::new();
    for ch in s.chars() {
        if "\\`*_[]<>|~=%$".contains(ch) {
            o.push('\\');
        }
        o.push(ch);
    }
    let starts_block = o.starts_with('#') || o.starts_with("- ") || o.starts_with("+ ") || o.starts_with('>') || {
        let digits: String = o.chars().take_while(|c| c.is_ascii_digit()).collect();
        !digits.is_empty() && o[digits.len()..].starts_with(". ")
    };
    if starts_block {
        o.insert(0, '\\');
    }
    o
}

fn code_md(s: &str) -> String {
    if s.contains('`') {
        format!("`` {s} ``")
    } else {
        format!("`{s}`")
    }
}

fn fmt_text(t: &T) -> String {
    let s = t.text.trim();
    let body = if t.font.family == Family::Mono { code_md(s) } else { esc_md(s) };
    match (t.font.bold, t.font.italic) {
        (true, true) => format!("***{body}***"),
        (true, false) => format!("**{body}**"),
        (false, true) => format!("*{body}*"),
        _ => body,
    }
}

/// The node's text as Markdown: lines top-down, compartments separated by `---`, members as code.
fn card_markdown(ts: &[&T], seps: &[&Sep]) -> String {
    let mut ts: Vec<&T> = ts.to_vec();
    ts.sort_by(|a, b| a.base.total_cmp(&b.base).then(a.x0.total_cmp(&b.x0)));
    // lines: texts sharing a baseline
    let mut lines: Vec<Vec<T>> = vec![];
    for t in ts {
        match lines.last_mut() {
            Some(l) if (l[0].base - t.base).abs() < 2.5 => l.push(t.clone()),
            _ => lines.push(vec![t.clone()]),
        }
    }
    let mut ys: Vec<f64> = seps.iter().map(|s| s.y).collect();
    ys.sort_by(f64::total_cmp);
    let mut out: Vec<String> = vec![];
    let mut prev: Option<(f64, f64, Family)> = None; // (line x0 of the compartment's first line, base, family)
    let mut first_x0: Option<f64> = None;
    let mut si = 0;
    for l in lines {
        let top = l[0].base - l[0].font.size as f64 * 0.8;
        // a separator between the previous line and this one starts a new compartment
        let mut crossed = false;
        while si < ys.len() && ys[si] < top + 1.0 {
            if prev.is_some_and(|p| ys[si] > p.1) {
                crossed = true;
            }
            si += 1;
        }
        if crossed {
            out.push("---".into());
            first_x0 = None;
            prev = None;
        }
        let lead = &l[0];
        // a wrapped member/responsibility continues indented under its first line: join it
        let continues = !crossed && l.len() == 1 && lead.anchor == Anchor::Start && !lead.font.bold && prev.is_some_and(|p| p.2 == lead.font.family) && first_x0.is_some_and(|x| lead.x0 > x + 3.0);
        if continues {
            if let Some(last) = out.last_mut() {
                let joined = if lead.font.family == Family::Mono {
                    // re-open the code span
                    let inner = last.trim_end_matches('`').trim_end_matches('*').to_string();
                    let tail = &last[inner.len()..];
                    format!("{inner} {}{tail}", lead.text.trim())
                } else {
                    format!("{last} {}", fmt_text(lead))
                };
                *last = joined;
                prev = Some((first_x0.unwrap_or(lead.x0), lead.base, lead.font.family));
                continue;
            }
        }
        if first_x0.is_none() {
            first_x0 = Some(lead.x0);
        }
        out.push(l.iter().map(fmt_text).collect::<Vec<_>>().join(" "));
        prev = Some((first_x0.unwrap_or(lead.x0), lead.base, lead.font.family));
    }
    out.join("\n")
}

// ------------------------------------------------------------------ scene-based views

fn relation_label(r: &Relation, conceptual: bool) -> (Option<String>, bool) {
    let (kind, arrow) = match r.kind {
        RelationKind::Association => (None, r.directed),
        RelationKind::Aggregation => (Some("◇ aggregates"), false),
        RelationKind::Composition => (Some("◆ composed of"), false),
        RelationKind::Inheritance => (Some(if conceptual { "is a" } else { "extends" }), true),
        RelationKind::Realization => (Some("implements"), true),
        RelationKind::Dependency => (Some("depends on"), true),
    };
    let mut parts: Vec<String> = kind.map(String::from).into_iter().collect();
    match r.interaction {
        Interaction::Async => parts.push("«async»".into()),
        Interaction::Event => parts.push("«event»".into()),
        Interaction::Sync => {}
    }
    if let Some(l) = r.label.as_ref().filter(|l| !l.trim().is_empty()) {
        parts.push(l.trim().to_string());
    }
    let mults = (r.from_mult.as_deref().filter(|s| !s.is_empty()), r.to_mult.as_deref().filter(|s| !s.is_empty()));
    match mults {
        (Some(a), Some(b)) => parts.push(format!("{a} → {b}")),
        (Some(a), None) => parts.push(format!("{a} →")),
        (None, Some(b)) => parts.push(format!("→ {b}")),
        (None, None) => {}
    }
    ((!parts.is_empty()).then(|| parts.join(" · ")), arrow)
}

/// C4 element colours (as the system view draws them).
fn c4_color(sv: &SystemView, id: &str) -> Rgb {
    if sv.actors.iter().any(|a| a == id) {
        Rgb::hex(0xc084fc)
    } else if sv.systems.iter().any(|x| x.name == id) {
        Rgb::hex(0x8a93a6)
    } else if let Some(k) = sv.containers.iter().find(|k| k.name == id) {
        if k.datastore {
            Rgb::hex(0x34d399)
        } else {
            Rgb::hex(0x60a5fa)
        }
    } else {
        Rgb::hex(0x7aa2ff)
    }
}

fn scene_parts(scene: &Scene, m: &dyn Measure, src: &Source, o: &CanvasOptions) -> (Vec<N>, Vec<E>) {
    // candidate nodes from the scene's hit boxes (hover keys name the model element)
    let mut nodes: Vec<N> = vec![];
    let mut seen = HashSet::new();
    for h in &scene.hits {
        let (HitShape::Rect { x1, y1, x2, y2 }, Some(hover)) = (&h.shape, &h.hover) else { continue };
        let (kind, name) = hover.split_once(':').unwrap_or(("", ""));
        let ok = match kind {
            "class" => h.node.is_some(),
            "package" | "c4" | "actor" | "usecase" => true,
            _ => false,
        };
        if !ok || !seen.insert(hover.clone()) {
            continue;
        }
        let keep = match (src, kind) {
            (Source::Classes { classes, .. }, "class") => classes.iter().find(|c| c.name == name).is_none_or(|c| o.ghosts || c.status != Status::Proposed),
            (Source::Classes { packages, .. }, "package") => o.packages && packages.iter().find(|p| p.name == name).is_none_or(|p| o.ghosts || p.status != Status::Proposed),
            (Source::Packages { packages, .. }, "package") => packages.iter().find(|p| p.name == name).is_none_or(|p| o.ghosts || p.status != Status::Proposed),
            (Source::System { view, .. }, "c4") => {
                let proposed = view.containers.iter().any(|k| k.name == name && k.status == Status::Proposed) || view.systems.iter().any(|x| x.name == name && x.status == Status::Proposed);
                o.ghosts || !proposed
            }
            (Source::UseCases { .. }, "actor" | "usecase") => true,
            _ => false,
        };
        if !keep {
            continue;
        }
        let color = match (src, kind) {
            (Source::Classes { classes, packages, .. }, "class") => classes.iter().find(|c| c.name == name).and_then(|c| c.package.as_ref()).and_then(|p| packages.iter().find(|x| &x.name == p)).and_then(|p| p.layer).map(layer_color),
            (Source::Classes { packages, .. }, "package") => packages.iter().find(|p| p.name == name).and_then(|p| p.layer).map(layer_color),
            (Source::Packages { packages, .. }, "package") => packages.iter().find(|p| p.name == name).and_then(|p| p.layer).map(layer_color),
            (Source::System { view, .. }, "c4") => Some(c4_color(view, name)),
            _ => None,
        };
        nodes.push(N { id: hover.clone(), r: [*x1, *y1, *x2, *y2], group: false, text: String::new(), label: None, color });
    }
    // use case diagram: the system boundary
    if let Source::UseCases { design } = src {
        let ucm = &design.use_cases;
        let actors: Vec<_> = ucm.actors.iter().filter(|a| a.status != Status::Pruned).collect();
        let ucs: Vec<_> = ucm.use_cases.iter().filter(|u| u.status != Status::Pruned).collect();
        if !ucs.is_empty() {
            let l = views::layout_use_cases(m, &actors, &ucs);
            let [bx, by, bw, bh] = l.boundary;
            nodes.push(N { id: "system".into(), r: [bx, by, bx + bw, by + bh], group: true, text: String::new(), label: Some(ucm.system.clone().unwrap_or_else(|| design.name.clone())), color: None });
        }
    }
    // a package frame around classes is a group; at the package level of detail it is the card itself
    let leaf_centres: Vec<(Pt, String)> = nodes.iter().filter(|n| !n.id.starts_with("package:") && !n.group).map(|n| ([(n.r[0] + n.r[2]) / 2.0, (n.r[1] + n.r[3]) / 2.0], n.id.clone())).collect();
    if matches!(src, Source::Classes { .. }) {
        for n in nodes.iter_mut().filter(|n| n.id.starts_with("package:")) {
            if leaf_centres.iter().any(|(c, _)| contains(n.r, *c, 0.0)) {
                n.group = true;
                n.label = Some(n.id["package:".len()..].to_string());
            }
        }
    }
    // card text: what the scene draws inside each card (calm export state, no diagnostics)
    let mut skip = tag::TRANSIENT | tag::DIAG;
    if !o.ghosts {
        skip |= tag::PROPOSED;
    }
    if !o.packages {
        skip |= tag::PKG;
    }
    let (mut ts, mut seps) = (vec![], vec![]);
    collect(&scene.root, Xf::ID, m, skip, &mut ts, &mut seps);
    let mut owned: Vec<Vec<&T>> = vec![vec![]; nodes.len()];
    for t in &ts {
        let best = nodes.iter().enumerate().filter(|(_, n)| !n.group && contains(n.r, t.mid, 1.0)).min_by(|a, b| area(a.1.r).total_cmp(&area(b.1.r))).map(|(i, _)| i);
        if let Some(i) = best {
            owned[i].push(t);
        }
    }
    for (i, n) in nodes.iter_mut().enumerate() {
        if n.group {
            continue;
        }
        let w = n.r[2] - n.r[0];
        let mine: Vec<&Sep> = seps.iter().filter(|s| s.y > n.r[1] + 1.0 && s.y < n.r[3] - 1.0 && s.x0 >= n.r[0] - 1.0 && s.x1 <= n.r[2] + 1.0 && s.x1 - s.x0 >= w * 0.6).collect();
        n.text = card_markdown(&owned[i], &mine);
        if n.text.is_empty() {
            // nothing drawn (e.g. a hidden ghost): name it at least
            n.text = format!("**{}**", esc_md(n.id.split_once(':').map(|x| x.1).unwrap_or(&n.id)));
        }
    }
    let ids: HashSet<String> = nodes.iter().map(|n| n.id.clone()).collect();
    let leaf = |id: &str| nodes.iter().any(|n| n.id == id && !n.group);
    let mut edges: Vec<E> = vec![];
    let mut push = |id: String, from: String, to: String, label: Option<String>, arrow: bool, ends: Option<(Pt, Pt)>| {
        if ids.contains(&from) && ids.contains(&to) {
            edges.push(E { id, from, to, label, arrow, ends, sides: None });
        }
    };
    let route_of = |key: &str| {
        scene.hits.iter().find_map(|h| match (&h.shape, &h.hover) {
            (HitShape::Line { pts, .. }, Some(hv)) if hv == key && pts.len() >= 2 => Some((pts[0], *pts.last().unwrap())),
            _ => None,
        })
    };
    match src {
        Source::Classes { relations, package_deps, conceptual, .. } => {
            for r in relations.iter() {
                if r.status == Status::Pruned || (r.status == Status::Proposed && !o.ghosts) {
                    continue;
                }
                let (from, to) = (format!("class:{}", r.from), format!("class:{}", r.to));
                if !leaf(&from) || !leaf(&to) {
                    continue;
                }
                let (label, arrow) = relation_label(r, *conceptual);
                let ends = route_of(&format!("relation:{}", r.id)).map(|(a, b)| {
                    // the route runs from → to; make sure of it
                    let fr = nodes.iter().find(|n| n.id == from).map(|n| n.r).unwrap_or_default();
                    let d = |p: Pt| ((p[0] - (fr[0] + fr[2]) / 2.0).powi(2) + (p[1] - (fr[1] + fr[3]) / 2.0).powi(2)).sqrt();
                    if d(a) <= d(b) { (a, b) } else { (b, a) }
                });
                push(format!("relation:{}", r.id), from, to, label, arrow, ends);
            }
            // package level of detail: package cards and their dependencies
            for d in package_deps.iter().filter(|d| d.from != d.to) {
                let (from, to) = (format!("package:{}", d.from), format!("package:{}", d.to));
                if leaf(&from) && leaf(&to) {
                    push(format!("dep:{}>{}", d.from, d.to), from, to, Some("depends on".into()), true, None);
                }
            }
        }
        Source::Packages { deps, .. } => {
            for d in deps.iter().filter(|d| d.from != d.to) {
                push(format!("dep:{}>{}", d.from, d.to), format!("package:{}", d.from), format!("package:{}", d.to), Some("depends on".into()), true, None);
            }
        }
        Source::System { view, level } => {
            let c4 = |n: &str| format!("c4:{n}");
            if *level == C4Level::Context {
                for (i, e) in view.context_edges.iter().enumerate() {
                    push(format!("c4e{i}"), c4(&e.from), c4(&e.to), e.label.clone(), true, None);
                }
            } else {
                for (i, d) in view.container_deps.iter().enumerate() {
                    let label = format!("{}{} dep{}", if d.sync { "" } else { "«async» " }, d.count, if d.count == 1 { "" } else { "s" });
                    push(format!("dep{i}"), c4(&d.from), c4(&d.to), Some(label), true, None);
                }
                for l in &view.links {
                    if l.status == Status::Proposed && !o.ghosts {
                        continue;
                    }
                    let mut parts = vec![];
                    if l.interaction != Interaction::Sync {
                        parts.push(format!("«{}»", if l.interaction == Interaction::Async { "async" } else { "event" }));
                    }
                    if let Some(p) = &l.protocol {
                        parts.push(p.clone());
                    }
                    push(format!("link:{}", l.id()), c4(&l.from), c4(&l.to), (!parts.is_empty()).then(|| parts.join(" ")), true, None);
                }
            }
        }
        Source::UseCases { design } => {
            for u in design.use_cases.use_cases.iter().filter(|u| u.status != Status::Pruned) {
                let uc = format!("usecase:{}", u.name);
                for a in &u.actors {
                    push(format!("{a}>{}", u.name), format!("actor:{a}"), uc.clone(), None, false, None);
                }
                for i in &u.includes {
                    push(format!("{}«include»{i}", u.name), uc.clone(), format!("usecase:{i}"), Some("«include»".into()), true, None);
                }
                for i in &u.extends {
                    push(format!("{}«extend»{i}", u.name), uc.clone(), format!("usecase:{i}"), Some("«extend»".into()), true, None);
                }
            }
        }
        Source::Sequence { .. } => {}
    }
    (nodes, edges)
}

// ------------------------------------------------------------------ sequence diagrams

const ANCHOR_W: f64 = 44.0;
const ANCHOR_H: f64 = 28.0;
const HEAD: f64 = 36.0;

/// Lifelines as groups with a header card, messages as edges between small numbered anchor cards.
fn sequence_parts(m: &dyn Measure, s: &Sequence, is_actor: &dyn Fn(&str) -> bool) -> (Vec<N>, Vec<E>) {
    let l = views::sequence_layout(m, s, is_actor);
    let t = &l.timeline;
    let actor = |p: &str| is_actor(s.class_of(p)) || is_actor(p);
    let mut created_at: HashMap<&str, f64> = HashMap::new();
    let mut destroyed_at: HashMap<&str, f64> = HashMap::new();
    for (k, e) in t.events.iter().enumerate() {
        match e {
            Event::Create(p) => {
                created_at.insert(p.as_str(), l.ys[k]);
            }
            Event::Destroy(p) => {
                destroyed_at.insert(p.as_str(), l.ys[k]);
            }
            _ => {}
        }
    }
    let mut nodes: Vec<N> = vec![];
    let mut edges: Vec<E> = vec![];
    for p in &s.participants {
        let (Some(&x), Some(&w)) = (l.xs.get(p), l.widths.get(p)) else { continue };
        let label = l.labels.get(p).cloned().unwrap_or_else(|| p.clone());
        let (top, h, text) = match created_at.get(p.as_str()) {
            Some(cy) => (cy - HEAD / 2.0, HEAD, format!("**{}**", esc_md(&label))),
            None if actor(p) => (-24.0, 70.0, format!("**{}**\n*actor*", esc_md(&label))),
            None => (0.0, HEAD, format!("**{}**", esc_md(&label))),
        };
        let end = destroyed_at.get(p.as_str()).copied().unwrap_or(l.height);
        nodes.push(N { id: format!("lane:{p}"), r: [x - w / 2.0 - 8.0, top - 8.0, x + w / 2.0 + 8.0, end + 16.0], group: true, text: String::new(), label: None, color: None });
        nodes.push(N { id: format!("lifeline:{p}"), r: [x - w / 2.0, top, x + w / 2.0, top + h], group: false, text, label: None, color: None });
    }
    let xl = |p: &str| l.xs.get(p).copied();
    let anchor = |id: String, x: f64, y: f64, text: &str| N { id, r: [x - ANCHOR_W / 2.0, y - ANCHOR_H / 2.0, x + ANCHOR_W / 2.0, y + ANCHOR_H / 2.0], group: false, text: text.to_string(), label: None, color: None };
    let side = |x1: f64, x2: f64| if x2 >= x1 { ("right", "left") } else { ("left", "right") };
    // label room per message (fragments enclose them, as drawn)
    let lab = FontSpec::mono(11.5);
    let mut label_right: HashMap<usize, f64> = HashMap::new();
    for (i, msg) in s.messages.iter().enumerate() {
        let (Some(x1), Some(x2)) = (xl(&msg.from), xl(&msg.to)) else { continue };
        let w = m.width(&format!("✗ {}", views::msg_label(s, i)), lab);
        label_right.insert(i, if x1 == x2 { x1 + 60.0 + w } else { (x1 + x2) / 2.0 + w / 2.0 });
    }
    let depths = sequence::fragment_depths(t);
    for (fk, f) in t.fragments.iter().enumerate() {
        let start = t.events.iter().position(|e| *e == Event::FragmentStart(fk));
        let end = t.events.iter().position(|e| *e == Event::FragmentEnd(fk));
        let (Some(start), Some(end)) = (start, end) else { continue };
        let mut xs: Vec<f64> = vec![];
        let mut right = f64::MIN;
        for i in sequence::fragment_messages(f) {
            let Some(msg) = s.messages.get(i) else { continue };
            xs.extend([&msg.from, &msg.to].iter().filter_map(|p| xl(p)));
            if let Some(r) = label_right.get(&i) {
                right = right.max(*r);
            }
        }
        if xs.is_empty() {
            continue;
        }
        let d = depths.get(fk).copied().unwrap_or(0) as f64;
        let x1 = xs.iter().cloned().fold(f64::MAX, f64::min) - 46.0 + d * 9.0;
        let x2 = (xs.iter().cloned().fold(f64::MIN, f64::max) + 46.0 - d * 9.0).max(right + 14.0 - d * 9.0).max(x1 + 180.0);
        let guards: Vec<String> = f.operands.iter().filter_map(|o| o.guard.as_ref()).map(|g| format!("[{}]", g.trim_matches(['[', ']']))).collect();
        let label = if guards.is_empty() { f.kind.as_str().to_string() } else { format!("{} {}", f.kind.as_str(), guards.join(" / ")) };
        nodes.push(N { id: format!("fragment:{fk}"), r: [x1, l.ys[start], x2, l.ys[end]], group: true, text: String::new(), label: Some(label), color: None });
    }
    for (k, e) in t.events.iter().enumerate() {
        let y = l.ys[k];
        match e {
            Event::Message(i) => {
                let msg = &s.messages[*i];
                if msg.status == Status::Pruned {
                    continue;
                }
                let text = views::msg_label(s, *i);
                let num = if msg.kind.is_call() { text.split(':').next().unwrap_or("").trim().to_string() } else { String::new() };
                if msg.kind == MessageKind::Ref {
                    let cover: Vec<f64> = [&msg.from, &msg.to].iter().filter_map(|p| xl(p)).collect();
                    let cover = if cover.is_empty() { l.xs.values().copied().collect() } else { cover };
                    let x1 = cover.iter().cloned().fold(f64::MAX, f64::min) - 50.0;
                    let x2 = (cover.iter().cloned().fold(f64::MIN, f64::max) + 50.0).max(x1 + m.width(&msg.method, FontSpec::sans(12.5)) + 90.0);
                    nodes.push(N { id: format!("ref:{i}"), r: [x1, y - 14.0, x2, y + 24.0], group: false, text: format!("**ref** {}", esc_md(&msg.method)), label: None, color: None });
                    continue;
                }
                let (Some(x1), Some(x2)) = (xl(&msg.from), xl(&msg.to)) else { continue };
                let mut label = match msg.kind {
                    MessageKind::Return => format!("↩ {text}"),
                    _ => text,
                };
                if let Some(n) = msg.note.as_ref().filter(|n| !n.trim().is_empty()) {
                    label = format!("{label} — {}", n.trim());
                }
                let from = format!("m{i}:from");
                if x1 == x2 {
                    if msg.kind == MessageKind::Destroy {
                        nodes.push(anchor(from, x1, y, "✗"));
                        continue;
                    }
                    // a self-call: one anchor, an edge back to itself
                    nodes.push(anchor(from.clone(), x1, y + 14.0, &num));
                    edges.push(E { id: format!("m{i}"), from: from.clone(), to: from, label: Some(label), arrow: true, ends: None, sides: Some(("right", "right")) });
                    continue;
                }
                let anchor_text = match msg.kind {
                    MessageKind::Return => "↩".to_string(),
                    MessageKind::Destroy => "✗".to_string(),
                    _ => num.clone(),
                };
                nodes.push(anchor(from.clone(), x1, y, &anchor_text));
                // a create message ends on the new object's header
                let to = if msg.kind == MessageKind::Create && created_at.contains_key(msg.to.as_str()) {
                    format!("lifeline:{}", msg.to)
                } else {
                    let id = format!("m{i}:to");
                    nodes.push(anchor(id.clone(), x2, y, &anchor_text));
                    id
                };
                edges.push(E { id: format!("m{i}"), from, to, label: Some(label), arrow: true, ends: Some(([x1, y], [x2, y])), sides: Some(side(x1, x2)) });
            }
            Event::Return { from, to, label, call } => {
                if s.messages.get(*call).is_some_and(|m| m.status == Status::Pruned) {
                    continue;
                }
                let (Some(x1), Some(x2)) = (xl(from), xl(to)) else { continue };
                if x1 == x2 {
                    continue;
                }
                let (a, b) = (format!("r{k}:from"), format!("r{k}:to"));
                nodes.push(anchor(a.clone(), x1, y, "↩"));
                nodes.push(anchor(b.clone(), x2, y, "↩"));
                let text = label.as_ref().filter(|l| !l.is_empty()).map(|l| format!("↩ {l}")).unwrap_or_else(|| "↩".into());
                edges.push(E { id: format!("r{k}"), from: a, to: b, label: Some(text), arrow: true, ends: Some(([x1, y], [x2, y])), sides: Some(side(x1, x2)) });
            }
            _ => {}
        }
    }
    (nodes, edges)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_markdown_but_keeps_code() {
        assert_eq!(esc_md("a*b_[c]"), "a\\*b\\_\\[c\\]");
        assert_eq!(esc_md("1. step"), "\\1. step");
        assert_eq!(esc_md("# not a heading"), "\\# not a heading");
        assert_eq!(code_md("List<Loan>"), "`List<Loan>`");
        assert_eq!(code_md("a`b"), "`` a`b ``");
    }

    #[test]
    fn relation_labels_spell_out_uml() {
        let r = |kind, directed| Relation { id: "x".into(), from: "A".into(), to: "B".into(), kind, directed, ..Default::default() };
        assert_eq!(relation_label(&r(RelationKind::Inheritance, false), false), (Some("extends".into()), true));
        assert_eq!(relation_label(&r(RelationKind::Inheritance, false), true), (Some("is a".into()), true));
        assert_eq!(relation_label(&r(RelationKind::Realization, false), false), (Some("implements".into()), true));
        assert_eq!(relation_label(&r(RelationKind::Dependency, false), false), (Some("depends on".into()), true));
        assert_eq!(relation_label(&r(RelationKind::Association, false), false), (None, false));
        assert_eq!(relation_label(&r(RelationKind::Association, true), false), (None, true));
        let mut c = r(RelationKind::Composition, false);
        c.label = Some("holds".into());
        c.from_mult = Some("1".into());
        c.to_mult = Some("0..*".into());
        assert_eq!(relation_label(&c, false), (Some("◆ composed of · holds · 1 → 0..*".into()), false));
        let mut a = r(RelationKind::Aggregation, false);
        a.interaction = Interaction::Async;
        assert_eq!(relation_label(&a, false).0.unwrap(), "◇ aggregates · «async»");
    }

    #[test]
    fn sides_follow_geometry() {
        let a = [0.0, 0.0, 100.0, 50.0];
        let b = [300.0, 0.0, 400.0, 50.0];
        assert_eq!(facing(a, b), ("right", "left"));
        assert_eq!(facing(b, a), ("left", "right"));
        let c = [0.0, 200.0, 100.0, 250.0];
        assert_eq!(facing(a, c), ("bottom", "top"));
        assert_eq!(side_at(a, [50.0, 0.0]), "top");
        assert_eq!(side_at(a, [100.0, 20.0]), "right");
    }

    #[test]
    fn card_size_grows_with_text() {
        let (w1, h1) = card_need("**A**");
        let (w2, h2) = card_need("**A**\n---\n`+ longer: String`\n`+ more()`");
        assert!(w2 > w1 && h2 > h1 + 2.0 * OB_LINE);
    }
}
