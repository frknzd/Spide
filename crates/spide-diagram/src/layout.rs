//! Layered (Sugiyama) layout for initial placement and the "tidy" action, on `rust-sugiyama`.
//! Replaces elkjs: a flat layered layout, and a compound one where packages are laid out on their
//! own and then stacked in bands by rank.

use rust_sugiyama::configure::Config;
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Debug, Clone)]
pub struct LayoutNode {
    pub id: String,
    pub w: f64,
    pub h: f64,
    pub group: Option<String>,
    /// library/framework code: kept at the edge of the drawing
    pub external: bool,
}

#[derive(Debug, Clone)]
pub struct LayoutEdge {
    pub from: String,
    pub to: String,
    pub kind: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Down,
    Right,
}

#[derive(Debug, Clone)]
pub struct GroupSpec {
    pub id: String,
    /// vertical band: lower ranks above higher ones (ui/infrastructure 0 · application 1 · domain 2)
    pub rank: u8,
    /// external library/system package: packed compactly beside its clients
    pub external: bool,
    /// minimum drawn frame width (its title must fit)
    pub min_w: f64,
}

/// Package hull padding as drawn by the class diagram (label on top).
pub struct Hull {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
}
pub const HULL: Hull = Hull { left: 28.0, top: 40.0, right: 28.0, bottom: 16.0 };

pub type Positions = HashMap<String, [f64; 2]>;

/// Top-left placed boxes of one layout, plus its size.
#[derive(Debug, Clone, Default)]
struct Placed {
    /// id → top-left
    at: Vec<(String, [f64; 2])>,
    w: f64,
    h: f64,
}

/// One layered drawing of `nodes` (centre-free sizes) and `edges`, DOWN: top-left positions.
/// `gap` between nodes of a layer, `layer_gap` between layers.
fn sugiyama(nodes: &[(String, f64, f64)], edges: &[(String, String)], gap: f64, layer_gap: f64, component_gap: f64, aspect: f64) -> Placed {
    if nodes.is_empty() {
        return Placed::default();
    }
    let idx: HashMap<&str, u32> = nodes.iter().enumerate().map(|(i, n)| (n.0.as_str(), i as u32)).collect();
    let extra_h = (layer_gap - gap).max(0.0);
    let verts: Vec<(u32, (f64, f64))> = nodes.iter().enumerate().map(|(i, n)| (i as u32, (n.1, n.2 + extra_h))).collect();
    let mut es: Vec<(u32, u32)> = vec![];
    let mut seen = HashSet::new();
    for (a, b) in edges {
        if let (Some(&x), Some(&y)) = (idx.get(a.as_str()), idx.get(b.as_str())) {
            if x != y && seen.insert((x, y)) && !seen.contains(&(y, x)) {
                es.push((x, y));
            }
        }
    }
    let config = Config { vertex_spacing: gap, ..Default::default() };
    let layouts = rust_sugiyama::from_vertices_and_edges(&verts, &es, &config);
    // each component: top-left boxes normalised to (0,0)
    let mut comps: Vec<Placed> = vec![];
    let mut placed: HashSet<usize> = HashSet::new();
    for (layout, _, _) in layouts {
        let mut boxes: Vec<(String, [f64; 2], f64, f64)> = vec![];
        for (id, (cx, cy)) in layout {
            if id >= nodes.len() {
                continue;
            }
            placed.insert(id);
            let n = &nodes[id];
            // y is the centre of the rank row (which includes the layer gap): keep the box at its top
            boxes.push((n.0.clone(), [cx - n.1 / 2.0, cy - (n.2 + extra_h) / 2.0], n.1, n.2));
        }
        let sizes: HashMap<&str, (f64, f64)> = nodes.iter().map(|n| (n.0.as_str(), (n.1, n.2))).collect();
        comps.push(squeeze(normalise(boxes), &sizes, edges, gap, layer_gap));
    }
    // isolated vertices that the library dropped (no edges): one component each
    for (i, n) in nodes.iter().enumerate() {
        if !placed.contains(&i) {
            comps.push(Placed { at: vec![(n.0.clone(), [0.0, 0.0])], w: n.1, h: n.2 });
        }
    }
    pack(comps, component_gap, aspect)
}

/// Tighten a layered drawing: each layer as tall as its tallest box (top-aligned) with exactly
/// `gy` between layers, each layer packed with `gx` gaps and aligned under its neighbours.
fn squeeze(p: Placed, sizes: &HashMap<&str, (f64, f64)>, edges: &[(String, String)], gx: f64, gy: f64) -> Placed {
    if p.at.len() < 2 {
        return p;
    }
    let sz = |id: &str| sizes[id];
    let mut at: Vec<[f64; 2]> = p.at.iter().map(|x| x.1).collect();
    // layers: boxes of one rank share their centre line
    let mut rows: BTreeMap<i64, Vec<usize>> = BTreeMap::new();
    for (i, (id, a)) in p.at.iter().enumerate() {
        rows.entry(((a[1] + sz(id).1 / 2.0) / 2.0).round() as i64).or_default().push(i);
    }
    let mut y = 0.0;
    for ids in rows.values() {
        let h = ids.iter().map(|&i| sz(&p.at[i].0).1).fold(0.0, f64::max);
        for &i in ids {
            at[i][1] = y;
        }
        y += h + gy;
    }
    // x: keep each layer's order (crossing minimisation), pack it tightly and centre every box under
    // its neighbours in the layers above / below (alternating sweeps)
    let layers: Vec<Vec<usize>> = rows
        .values()
        .map(|ids| {
            let mut v = ids.clone();
            v.sort_by(|&a, &b| at[a][0].total_cmp(&at[b][0]));
            v
        })
        .collect();
    let index: HashMap<&str, usize> = p.at.iter().enumerate().map(|(i, (id, _))| (id.as_str(), i)).collect();
    let mut adj: Vec<Vec<usize>> = vec![vec![]; p.at.len()];
    for (a, b) in edges {
        if let (Some(&x), Some(&y)) = (index.get(a.as_str()), index.get(b.as_str())) {
            if x != y {
                adj[x].push(y);
                adj[y].push(x);
            }
        }
    }
    let mut layer_of = vec![0usize; p.at.len()];
    for (l, ids) in layers.iter().enumerate() {
        for &i in ids {
            layer_of[i] = l;
        }
    }
    // initial: each layer packed and centred on 0
    let mut cx = vec![0.0; p.at.len()];
    let pack_layer = |ids: &[usize], want: &dyn Fn(usize) -> Option<f64>, cx: &mut Vec<f64>| {
        let mut x = f64::NEG_INFINITY;
        let mut placed: Vec<f64> = vec![];
        for &i in ids {
            let w = sz(&p.at[i].0).0;
            let left = match want(i) {
                Some(c) => (c - w / 2.0).max(x),
                None if x.is_finite() => x,
                None => 0.0,
            };
            placed.push(left + w / 2.0);
            x = left + w + gx;
        }
        // pull the layer back towards its wishes (a uniform shift never creates overlaps)
        let devs: Vec<f64> = ids.iter().zip(&placed).filter_map(|(&i, c)| want(i).map(|w| w - c)).collect();
        let shift = if devs.is_empty() {
            -(placed.first().copied().unwrap_or(0.0) + placed.last().copied().unwrap_or(0.0)) / 2.0
        } else {
            let mut d = devs.clone();
            d.sort_by(|a, b| a.total_cmp(b));
            d[d.len() / 2]
        };
        for (k, &i) in ids.iter().enumerate() {
            cx[i] = placed[k] + shift;
        }
    };
    for ids in &layers {
        pack_layer(ids, &|_| None, &mut cx);
    }
    for sweep in 0..4 {
        let down = sweep % 2 == 0;
        let order: Vec<usize> = if down { (0..layers.len()).collect() } else { (0..layers.len()).rev().collect() };
        for l in order {
            let snapshot = cx.clone();
            let want = |i: usize| -> Option<f64> {
                let ns: Vec<f64> = adj[i].iter().filter(|&&j| if down { layer_of[j] < l } else { layer_of[j] > l }).map(|&j| snapshot[j]).collect();
                (!ns.is_empty()).then(|| ns.iter().sum::<f64>() / ns.len() as f64)
            };
            pack_layer(&layers[l], &want, &mut cx);
        }
    }
    for (i, a) in at.iter_mut().enumerate() {
        a[0] = cx[i] - sz(&p.at[i].0).0 / 2.0;
    }
    normalise(p.at.iter().enumerate().map(|(i, (id, _))| (id.clone(), at[i], sz(id).0, sz(id).1)).collect())
}

fn normalise(boxes: Vec<(String, [f64; 2], f64, f64)>) -> Placed {
    let x0 = boxes.iter().map(|b| b.1[0]).fold(f64::INFINITY, f64::min);
    let y0 = boxes.iter().map(|b| b.1[1]).fold(f64::INFINITY, f64::min);
    let x1 = boxes.iter().map(|b| b.1[0] + b.2).fold(f64::NEG_INFINITY, f64::max);
    let y1 = boxes.iter().map(|b| b.1[1] + b.3).fold(f64::NEG_INFINITY, f64::max);
    Placed { at: boxes.into_iter().map(|b| (b.0, [b.1[0] - x0, b.1[1] - y0])).collect(), w: x1 - x0, h: y1 - y0 }
}

/// Pack components into rows (largest first) towards a width/height `aspect`, like ELK's component packing.
fn pack(mut comps: Vec<Placed>, gap: f64, aspect: f64) -> Placed {
    if comps.len() == 1 {
        return comps.pop().unwrap();
    }
    comps.sort_by(|a, b| (b.w * b.h).total_cmp(&(a.w * a.h)));
    let area: f64 = comps.iter().map(|c| (c.w + gap) * (c.h + gap)).sum();
    let widest = comps.iter().map(|c| c.w).fold(0.0, f64::max);
    let max_w = (area * aspect).sqrt().max(widest);
    let (mut x, mut y, mut row_h, mut w) = (0.0, 0.0, 0.0f64, 0.0f64);
    let mut out = Placed::default();
    for c in comps {
        if x > 0.0 && x + c.w > max_w {
            x = 0.0;
            y += row_h + gap;
            row_h = 0.0;
        }
        for (id, p) in c.at {
            out.at.push((id, [p[0] + x, p[1] + y]));
        }
        w = w.max(x + c.w);
        row_h = row_h.max(c.h);
        x += c.w + gap;
    }
    out.w = w;
    out.h = y + row_h;
    out
}

/// Rows of boxes towards an aspect ratio (ELK rectpacking for external packages).
fn rect_pack(nodes: &[(String, f64, f64)], gap: f64, aspect: f64) -> Placed {
    pack(nodes.iter().map(|n| Placed { at: vec![(n.0.clone(), [0.0, 0.0])], w: n.1, h: n.2 }).collect(), gap, aspect)
}

/// Wrap a tall layered drawing into side-by-side chunks of layers (ELK's MULTI_EDGE wrapping):
/// the number of chunks is the one whose drawing comes closest to the wanted aspect ratio.
/// Wrapping moves later layers beside earlier ones; an edge between chunks then runs sideways or
/// upwards. Never split a generalisation (parents stay above their children); other split edges
/// cost a little.
fn wrap_layers(p: Placed, sizes: &HashMap<String, (f64, f64)>, gap: f64, aspect: f64, edges: &[(String, String)], hier: &HashSet<(String, String)>) -> Placed {
    if p.h <= 0.0 || p.w / p.h >= aspect {
        return p;
    }
    // rows by top y
    let mut rows: BTreeMap<i64, Vec<(String, [f64; 2])>> = BTreeMap::new();
    for (id, at) in &p.at {
        rows.entry(at[1].round() as i64).or_default().push((id.clone(), *at));
    }
    let rows: Vec<Vec<(String, [f64; 2])>> = rows.into_values().collect();
    if rows.len() < 2 {
        return p;
    }
    let arrange = |chunks: usize| -> Placed {
        let per = rows.len().div_ceil(chunks);
        let mut out = Placed::default();
        let mut x = 0.0;
        for chunk in rows.chunks(per) {
            let boxes: Vec<(String, [f64; 2], f64, f64)> = chunk.iter().flatten().map(|(id, at)| (id.clone(), *at, sizes[id].0, sizes[id].1)).collect();
            let c = normalise(boxes);
            for (id, at) in c.at {
                out.at.push((id, [at[0] + x, at[1]]));
            }
            out.h = out.h.max(c.h);
            x += c.w + gap;
            out.w = x - gap;
        }
        out
    };
    let score = |q: &Placed| {
        let at: HashMap<_, _> = q.at.iter().map(|(id, p)| (id.as_str(), [p[0] + sizes[id].0 / 2.0, p[1] + sizes[id].1 / 2.0])).collect();
        let length: f64 = edges.iter().filter_map(|(a, b)| Some((at.get(a.as_str())?, at.get(b.as_str())?)))
            .map(|(a, b)| (a[0] - b[0]).abs() + (a[1] - b[1]).abs()).sum();
        // Area penalises whitespace; edge travel discourages overly aggressive folding.
        shape_score(q.w, q.h, aspect) * (1.0 + 0.6 * ((q.w / q.h.max(1.0)) / aspect).ln().abs()) + length * gap * 0.15
    };
    let mut best = p.clone();
    let mut best_score = score(&p);
    for k in 2..=rows.len() {
        let per = rows.len().div_ceil(k);
        let chunk_of: HashMap<&str, usize> = rows.iter().enumerate().flat_map(|(i, r)| r.iter().map(move |(id, _)| (id.as_str(), i / per))).collect();
        let split = |e: &(String, String)| chunk_of.get(e.0.as_str()) != chunk_of.get(e.1.as_str());
        if edges.iter().any(|e| split(e) && hier.contains(e)) {
            continue;
        }
        let q = arrange(k);
        let sc = score(&q) * (1.0 + 0.08 * edges.iter().filter(|e| split(e)).count() as f64);
        if sc < best_score {
            best = q;
            best_score = sc;
        }
    }
    best
}

fn centred(p: &Placed, sizes: &HashMap<String, (f64, f64)>) -> Positions {
    let mut res: Positions = p.at.iter().map(|(id, at)| (id.clone(), [at[0] + sizes[id].0 / 2.0, at[1] + sizes[id].1 / 2.0])).collect();
    let n = res.len().max(1) as f64;
    let cx = res.values().map(|p| p[0]).sum::<f64>() / n;
    let cy = res.values().map(|p| p[1]).sum::<f64>() / n;
    for p in res.values_mut() {
        p[0] -= cx;
        p[1] -= cy;
    }
    res
}

/// Returns centre positions. Parents (inheritance) end up above children; clients above suppliers.
pub fn layered(nodes: &[LayoutNode], edges: &[LayoutEdge], direction: Direction) -> Positions {
    let ids: HashSet<&str> = nodes.iter().map(|n| n.id.as_str()).collect();
    let flip = direction == Direction::Right;
    let ns: Vec<(String, f64, f64)> = nodes.iter().map(|n| if flip { (n.id.clone(), n.h, n.w) } else { (n.id.clone(), n.w, n.h) }).collect();
    let es: Vec<(String, String)> = edges.iter().filter(|e| ids.contains(e.from.as_str()) && ids.contains(e.to.as_str()) && e.from != e.to).map(|e| orient(e)).collect();
    let target = if flip { 1.0 / ASPECT } else { ASPECT };
    let sizes: HashMap<String, (f64, f64)> = ns.iter().map(|n| (n.0.clone(), (n.1, n.2))).collect();
    let hier: HashSet<_> = edges.iter().filter(|e| e.kind == "inheritance" || e.kind == "realization").map(orient).collect();
    let p = sugiyama(&ns, &es, 70.0, 110.0, 120.0, target);
    // Small semantic chains already read clearly; folding is useful for larger views.
    let p = if ns.len() > 4 { wrap_layers(p, &sizes, 80.0, target, &es, &hier) } else { p };
    let mut res = centred(&p, &sizes);
    if flip {
        for p in res.values_mut() {
            *p = [p[1], p[0]];
        }
    }
    res
}

fn orient(e: &LayoutEdge) -> (String, String) {
    let up = e.kind == "inheritance" || e.kind == "realization";
    if up {
        (e.to.clone(), e.from.clone())
    } else {
        (e.from.clone(), e.to.clone())
    }
}

/// Target width/height ratio of a whole drawing (screens and landscape pages).
pub const ASPECT: f64 = 16.0 / 9.0;

/// Smaller is better: the bounding area, penalised the further its aspect is from `target`.
fn shape_score(w: f64, h: f64, target: f64) -> f64 {
    let a = w.max(1.0) / h.max(1.0);
    let deviation = (a / target).ln().abs();
    // Prefer compactness near the target, but strongly discourage very long strips.
    w.max(1.0) * h.max(1.0) * (1.0 + 0.4 * deviation + (deviation - 0.7).max(0.0).powi(2))
}

/// One group (package, or the classes outside any package) as a compact layered drawing:
/// tight layers, no empty columns, wrapped towards the target aspect ratio.
fn arrange_group(items: &[(String, f64, f64)], edges: &[(String, String)], aspect: f64, hier: &HashSet<(String, String)>) -> Placed {
    let sizes: HashMap<String, (f64, f64)> = items.iter().map(|n| (n.0.clone(), (n.1, n.2))).collect();
    let p = sugiyama(items, edges, 40.0, 40.0, 48.0, aspect);
    wrap_layers(p, &sizes, 48.0, aspect, edges, hier)
}

#[derive(Clone, Copy, PartialEq)]
enum ExtSide {
    Right,
    Bottom,
}

/// Compound layout: package hulls can't overlap and classes stay inside their package. Each package
/// is laid out on its own (compact layered drawing). Packages are then placed in bands by rank
/// (ui and infrastructure on top, application, domain below), each band ordered by where its
/// neighbours sit and flowed into rows, at the row width that gives the most compact, near-
/// rectangular drawing. External (library) packages and classes go into a band along the right
/// or bottom edge, next to their clients. Returns class centre positions.
pub fn compound(nodes: &[LayoutNode], edges: &[LayoutEdge], groups: &[GroupSpec]) -> Positions {
    let ids: HashSet<&str> = nodes.iter().map(|n| n.id.as_str()).collect();
    let spec: HashMap<&str, &GroupSpec> = groups.iter().map(|g| (g.id.as_str(), g)).collect();
    let group_of: HashMap<&str, Option<&str>> = nodes.iter().map(|n| (n.id.as_str(), n.group.as_deref().filter(|g| spec.contains_key(g)))).collect();
    let pad = 0.0; // class footprints already include their chip bands
    const LOOSE: &str = "loose::";
    let mut by_group: Vec<(String, Vec<&LayoutNode>)> = vec![];
    let mut loose: Vec<&LayoutNode> = vec![];
    let mut loose_ext: Vec<&LayoutNode> = vec![];
    for n in nodes {
        match group_of[n.id.as_str()] {
            Some(g) => match by_group.iter_mut().find(|(k, _)| k == g) {
                Some((_, v)) => v.push(n),
                None => by_group.push((g.to_string(), vec![n])),
            },
            None if n.external => loose_ext.push(n),
            None => loose.push(n),
        }
    }
    let pkey = |g: &str| format!("pkg::{g}");
    let ext_loose: HashSet<&str> = loose_ext.iter().map(|n| n.id.as_str()).collect();
    // top-level item of a class: its package, the block of loose classes, or itself (external)
    let parent_key = |n: &str| match group_of.get(n).copied().flatten() {
        Some(g) => pkey(g),
        None if ext_loose.contains(n) => n.to_string(),
        None => LOOSE.to_string(),
    };
    let rank_of = |key: &str| key.strip_prefix("pkg::").and_then(|g| spec.get(g)).map(|g| g.rank).unwrap_or(1);
    let is_ext = |key: &str| key.strip_prefix("pkg::").and_then(|g| spec.get(g)).is_some_and(|g| g.external) || ext_loose.contains(key);
    // edges inside a group → that group's layout; across groups → neighbours of the top-level items
    let mut inner: HashMap<String, Vec<(String, String)>> = HashMap::new();
    let mut nb: HashMap<String, Vec<String>> = HashMap::new();
    let hier: HashSet<(String, String)> = edges.iter().filter(|e| e.kind == "inheritance" || e.kind == "realization").map(orient).collect();
    for e in edges {
        if !ids.contains(e.from.as_str()) || !ids.contains(e.to.as_str()) || e.from == e.to {
            continue;
        }
        let (a, b) = orient(e);
        let (pa, pb) = (parent_key(&a), parent_key(&b));
        if pa == pb {
            inner.entry(pa).or_default().push((a, b));
            continue;
        }
        nb.entry(pa.clone()).or_default().push(pb.clone());
        nb.entry(pb).or_default().push(pa);
    }
    // 1. each group on its own: inner centre positions (relative to the item's top-left) and size
    let mut inner_pos: HashMap<String, Vec<(String, [f64; 2])>> = HashMap::new();
    let mut main: Vec<(String, f64, f64, u8)> = vec![]; // key, w, h, band
    let mut ext_items: Vec<(String, Vec<(String, f64, f64)>, bool, f64)> = vec![]; // key, members, framed, min_w
    let mut variants: HashMap<String, Vec<(Vec<(String, [f64; 2])>, f64, f64)>> = HashMap::new();
    let place_group = |p: &Placed, items: &[(String, f64, f64)], framed: bool, min_w: f64| -> (Vec<(String, [f64; 2])>, f64, f64) {
        let sizes: HashMap<&str, (f64, f64)> = items.iter().map(|n| (n.0.as_str(), (n.1, n.2))).collect();
        let (l, t, r, b) = if framed { (HULL.left + pad, HULL.top + pad, HULL.right + pad, HULL.bottom + pad) } else { (0.0, 0.0, 0.0, 0.0) };
        let w = (p.w + l + r).max(min_w);
        let ox = l + (w - (p.w + l + r)) / 2.0;
        let at = p.at.iter().map(|(id, at)| (id.clone(), [at[0] + ox + sizes[id.as_str()].0 / 2.0, at[1] + t + sizes[id.as_str()].1 / 2.0])).collect();
        (at, w, p.h + t + b)
    };
    for (g, ns) in &by_group {
        let key = pkey(g);
        let items: Vec<(String, f64, f64)> = ns.iter().map(|n| (n.id.clone(), n.w, n.h)).collect();
        let min_w = spec.get(g.as_str()).map(|s| s.min_w).unwrap_or(0.0);
        if is_ext(&key) {
            ext_items.push((key, items, true, min_w));
            continue;
        }
        // a few shapes of the same package (narrow … wide); the whole drawing picks among them
        let mut vs: Vec<(Vec<(String, [f64; 2])>, f64, f64)> = vec![];
        for aspect in [ASPECT, 1.6, 0.7, 3.2, 6.0] {
            let p = arrange_group(&items, inner.get(&key).map(|v| v.as_slice()).unwrap_or(&[]), aspect, &hier);
            let v = place_group(&p, &items, true, min_w);
            if !vs.iter().any(|x| (x.1 - v.1).abs() < 1.0 && (x.2 - v.2).abs() < 1.0) {
                vs.push(v);
            }
        }
        main.push((key.clone(), vs[0].1, vs[0].2, rank_of(&key)));
        variants.insert(key.clone(), vs);
    }
    if !loose.is_empty() {
        let items: Vec<(String, f64, f64)> = loose.iter().map(|n| (n.id.clone(), n.w, n.h)).collect();
        let p = arrange_group(&items, inner.get(LOOSE).map(|v| v.as_slice()).unwrap_or(&[]), if by_group.is_empty() { ASPECT } else { 1.6 }, &hier);
        let (at, w, h) = place_group(&p, &items, false, 0.0);
        inner_pos.insert(LOOSE.into(), at);
        main.push((LOOSE.into(), w, h, 1));
    }
    for n in &loose_ext {
        ext_items.push((n.id.clone(), vec![(n.id.clone(), n.w, n.h)], false, 0.0));
    }
    if main.is_empty() && !ext_items.is_empty() {
        // only library code: lay it out like ordinary classes
        for (key, items, framed, min_w) in std::mem::take(&mut ext_items) {
            let p = arrange_group(&items, inner.get(&key).map(|v| v.as_slice()).unwrap_or(&[]), 1.6, &hier);
            let (at, w, h) = place_group(&p, &items, framed, min_w);
            inner_pos.insert(key.clone(), at);
            main.push((key, w, h, 1));
        }
    }
    // 2. bands, top to bottom, flowed into rows; the best row width and external side win
    type ExtPlaced = (HashMap<String, [f64; 2]>, HashMap<String, (Vec<(String, [f64; 2])>, f64, f64)>);
    let solve = |main: &Vec<(String, f64, f64, u8)>| -> Option<(f64, HashMap<String, [f64; 2]>, ExtPlaced)> {
        let mut bands: BTreeMap<u8, Vec<(String, f64, f64)>> = BTreeMap::new();
        for (k, w, h, b) in main.iter() {
            bands.entry(*b).or_default().push((k.clone(), *w, *h));
        }
        let (item_gap, row_gap, band_gap, ext_gap) = (60.0, 60.0, 80.0, 80.0);
        let widest = main.iter().map(|m| m.1).fold(0.0, f64::max);
        let widest_band = bands.values().map(|b| b.iter().map(|x| x.1).sum::<f64>() + item_gap * (b.len().saturating_sub(1)) as f64).fold(0.0, f64::max);
        let mut candidates: Vec<f64> = (0..=16).map(|i| widest * (widest_band.max(widest) / widest.max(1.0)).powf(i as f64 / 16.0)).collect();
        candidates.push(widest_band.max(widest));
        let total_area: f64 = main.iter().map(|m| m.1 * m.2).sum::<f64>() + ext_items.iter().flat_map(|e| e.1.iter()).map(|n| n.1 * n.2).sum::<f64>();
        candidates.push((total_area * ASPECT).sqrt().max(widest));

        let layout_main = |limit: f64| -> HashMap<String, [f64; 2]> {
            let mut placed: HashMap<String, [f64; 2]> = HashMap::new();
            let mut size: HashMap<String, (f64, f64)> = HashMap::new();
            let mut y = 0.0;
            for members in bands.values() {
                // order by the barycentre of neighbours already placed (fewer crossings); unconnected keep order
                let mut order: Vec<(usize, &(String, f64, f64), f64)> = members
                    .iter()
                    .enumerate()
                    .map(|(i, m)| {
                        let xs: Vec<f64> = nb.get(&m.0).map(|v| v.iter().filter_map(|o| placed.get(o).map(|p| p[0] + size[o].0 / 2.0)).collect()).unwrap_or_default();
                        let bx = if xs.is_empty() { f64::NAN } else { xs.iter().sum::<f64>() / xs.len() as f64 };
                        (i, m, bx)
                    })
                    .collect();
                // unconnected items keep their relative order, slotted between the connected ones
                let mut last = f64::NEG_INFINITY;
                for o in &mut order {
                    if o.2.is_nan() {
                        o.2 = last + 1e-3;
                    }
                    last = o.2;
                }
                order.sort_by(|a, b| a.2.total_cmp(&b.2).then(a.0.cmp(&b.0)));
                // flow into rows of at most `limit` width, each row centred
                let mut rows: Vec<Vec<&(String, f64, f64)>> = vec![vec![]];
                let mut x = 0.0;
                for (_, m, _) in &order {
                    if x > 0.0 && x + m.1 > limit + 0.5 {
                        rows.push(vec![]);
                        x = 0.0;
                    }
                    rows.last_mut().unwrap().push(m);
                    x += m.1 + item_gap;
                }
                for row in rows {
                    let rw: f64 = row.iter().map(|m| m.1).sum::<f64>() + item_gap * (row.len().saturating_sub(1)) as f64;
                    let rh = row.iter().map(|m| m.2).fold(0.0, f64::max);
                    let mut x = -rw / 2.0;
                    for m in row {
                        placed.insert(m.0.clone(), [x, y]);
                        size.insert(m.0.clone(), (m.1, m.2));
                        x += m.1 + item_gap;
                    }
                    y += rh + row_gap;
                }
                y += band_gap - row_gap;
            }
            placed
        };
        let main_size: HashMap<&str, (f64, f64)> = main.iter().map(|m| (m.0.as_str(), (m.1, m.2))).collect();
        let bbox = |placed: &HashMap<String, [f64; 2]>, sizes: &dyn Fn(&str) -> (f64, f64)| -> [f64; 4] {
            let mut b = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
            for (k, p) in placed {
                let (w, h) = sizes(k);
                b = [b[0].min(p[0]), b[1].min(p[1]), b[2].max(p[0] + w), b[3].max(p[1] + h)];
            }
            b
        };
        // external items beside their clients along one edge
        let layout_ext = |placed: &HashMap<String, [f64; 2]>, mb: [f64; 4], side: ExtSide| -> ExtPlaced {
            let mut at = HashMap::new();
            let mut inner_ext = HashMap::new();
            if ext_items.is_empty() {
                return (at, inner_ext);
            }
            let mut items: Vec<(String, f64, f64, f64)> = vec![]; // key, w, h, desired coordinate
            for (key, members, framed, min_w) in &ext_items {
                let p = rect_pack(members, 16.0, if side == ExtSide::Right { 0.8 } else { 3.0 });
                let (pos, w, h) = place_group(&p, members, *framed, *min_w);
                let cs: Vec<f64> = nb
                    .get(key)
                    .map(|v| {
                        v.iter()
                            .filter_map(|o| {
                                let p = placed.get(o)?;
                                let (w, h) = main_size[o.as_str()];
                                Some(if side == ExtSide::Right { p[1] + h / 2.0 } else { p[0] + w / 2.0 })
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let want = if cs.is_empty() { f64::INFINITY } else { cs.iter().sum::<f64>() / cs.len() as f64 };
                inner_ext.insert(key.clone(), (pos, w, h));
                items.push((key.clone(), w, h, want));
            }
            items.sort_by(|a, b| a.3.total_cmp(&b.3).then(a.0.cmp(&b.0)));
            match side {
                ExtSide::Right => {
                    let max_h = (mb[3] - mb[1]).max(items.iter().map(|i| i.2).fold(0.0, f64::max));
                    let (mut x, mut y, mut col_w) = (mb[2] + ext_gap, mb[1], 0.0f64);
                    for (key, w, h, want) in items {
                        let mut top = if want.is_finite() { y.max(want - h / 2.0) } else { y };
                        if top + h > mb[1] + max_h + 0.5 && y > mb[1] {
                            x += col_w + item_gap;
                            y = mb[1];
                            col_w = 0.0;
                            top = if want.is_finite() { y.max(want - h / 2.0) } else { y };
                        }
                        top = top.min((mb[1] + max_h - h).max(y));
                        at.insert(key, [x, top]);
                        y = top + h + 40.0;
                        col_w = col_w.max(w);
                    }
                }
                ExtSide::Bottom => {
                    let max_w = (mb[2] - mb[0]).max(items.iter().map(|i| i.1).fold(0.0, f64::max));
                    let (mut x, mut y, mut row_h) = (mb[0], mb[3] + ext_gap, 0.0f64);
                    for (key, w, h, want) in items {
                        let mut left = if want.is_finite() { x.max(want - w / 2.0) } else { x };
                        if left + w > mb[0] + max_w + 0.5 && x > mb[0] {
                            y += row_h + row_gap;
                            x = mb[0];
                            row_h = 0.0;
                            left = if want.is_finite() { x.max(want - w / 2.0) } else { x };
                        }
                        left = left.min((mb[0] + max_w - w).max(x));
                        at.insert(key, [left, y]);
                        x = left + w + 40.0;
                        row_h = row_h.max(h);
                    }
                }
            }
            (at, inner_ext)
        };
        let mut best: Option<(f64, HashMap<String, [f64; 2]>, ExtPlaced)> = None;
        for limit in candidates {
            let placed = layout_main(limit);
            let mb = bbox(&placed, &|k| main_size[k]);
            for side in [ExtSide::Right, ExtSide::Bottom] {
                let ext = layout_ext(&placed, mb, side);
                let eb = bbox(&ext.0, &|k| (ext.1[k].1, ext.1[k].2));
                let all = [mb[0].min(eb[0]), mb[1].min(eb[1]), mb[2].max(eb[2]), mb[3].max(eb[3])];
                let score = shape_score(all[2] - all[0], all[3] - all[1], ASPECT);
                if best.as_ref().is_none_or(|b| score < b.0 - 1e-6) {
                    best = Some((score, placed.clone(), ext));
                }
                if ext_items.is_empty() {
                    break;
                }
            }
        }
        best
    };
    // coordinate descent over the packages' shapes
    let mut choice: HashMap<String, usize> = HashMap::new();
    let with_choice = |choice: &HashMap<String, usize>| -> Vec<(String, f64, f64, u8)> {
        main.iter()
            .map(|m| match (variants.get(&m.0), choice.get(&m.0)) {
                (Some(v), Some(&i)) => (m.0.clone(), v[i].1, v[i].2, m.3),
                _ => m.clone(),
            })
            .collect()
    };
    let mut best = solve(&main);
    if variants.values().any(|v| v.len() > 1) {
        for _ in 0..2 {
            let mut improved = false;
            for m in &main {
                let Some(vs) = variants.get(&m.0) else { continue };
                for i in 0..vs.len() {
                    if choice.get(&m.0).copied().unwrap_or(0) == i {
                        continue;
                    }
                    let mut c = choice.clone();
                    c.insert(m.0.clone(), i);
                    let r = solve(&with_choice(&c));
                    if r.as_ref().map(|x| x.0).unwrap_or(f64::INFINITY) < best.as_ref().map(|x| x.0).unwrap_or(f64::INFINITY) - 1e-6 {
                        best = r;
                        choice = c;
                        improved = true;
                    }
                }
            }
            if !improved {
                break;
            }
        }
    }
    for (k, vs) in &variants {
        inner_pos.insert(k.clone(), vs[choice.get(k).copied().unwrap_or(0)].0.clone());
    }
    let Some((_, placed, (ext_at, ext_inner))) = best else { return HashMap::new() };
    // 3. class centres
    let mut res: Positions = HashMap::new();
    for (key, at) in &placed {
        for (id, p) in inner_pos.get(key).into_iter().flatten() {
            res.insert(id.clone(), [at[0] + p[0], at[1] + p[1]]);
        }
    }
    for (key, at) in &ext_at {
        for (id, p) in &ext_inner[key].0 {
            res.insert(id.clone(), [at[0] + p[0], at[1] + p[1]]);
        }
    }
    // centre the drawing's bounding box on the origin
    let sizes: HashMap<&str, (f64, f64)> = nodes.iter().map(|n| (n.id.as_str(), (n.w, n.h))).collect();
    let mut b = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    for (id, p) in &res {
        let (w, h) = sizes[id.as_str()];
        b = [b[0].min(p[0] - w / 2.0), b[1].min(p[1] - h / 2.0), b[2].max(p[0] + w / 2.0), b[3].max(p[1] + h / 2.0)];
    }
    let (cx, cy) = ((b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0);
    for p in res.values_mut() {
        p[0] = (p[0] - cx).round();
        p[1] = (p[1] - cy).round();
    }
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: &str, g: Option<&str>) -> LayoutNode {
        LayoutNode { id: id.into(), w: 140.0, h: 80.0, group: g.map(Into::into), external: false }
    }
    fn edge(a: &str, b: &str, kind: &str) -> LayoutEdge {
        LayoutEdge { from: a.into(), to: b.into(), kind: kind.into() }
    }
    fn overlap(p: &Positions, a: &str, b: &str) -> bool {
        let (x, y) = (p[a], p[b]);
        (x[0] - y[0]).abs() < 140.0 && (x[1] - y[1]).abs() < 80.0
    }

    #[test]
    fn layered_puts_parents_above_and_clients_above_suppliers() {
        let ns = vec![node("Shape", None), node("Circle", None), node("Square", None), node("Canvas", None)];
        let es = vec![edge("Circle", "Shape", "inheritance"), edge("Square", "Shape", "inheritance"), edge("Canvas", "Shape", "association")];
        let p = layered(&ns, &es, Direction::Down);
        assert!(p["Shape"][1] > p["Canvas"][1]);
        assert!(p["Shape"][1] < p["Circle"][1]);
        for (a, b) in [("Circle", "Square"), ("Shape", "Canvas"), ("Circle", "Shape")] {
            assert!(!overlap(&p, a, b), "{a} overlaps {b}: {p:?}");
        }
    }

    #[test]
    fn tall_dependency_chain_folds_toward_landscape_without_overlap() {
        let ns: Vec<_> = (0..12).map(|i| node(&format!("p{i}"), None)).collect();
        let es: Vec<_> = (0..11).map(|i| edge(&format!("p{i}"), &format!("p{}", i + 1), "dependency")).collect();
        for direction in [Direction::Down, Direction::Right] {
            let p = layered(&ns, &es, direction);
            assert_eq!(p, layered(&ns, &es, direction));
            let bounds = p.values().fold([f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY], |b, p| [b[0].min(p[0]-70.0), b[1].min(p[1]-40.0), b[2].max(p[0]+70.0), b[3].max(p[1]+40.0)]);
            let aspect = (bounds[2]-bounds[0]) / (bounds[3]-bounds[1]);
            println!("dependency chain aspect {aspect:.2}");
            assert!((0.8..3.0).contains(&aspect));
            for (i, a) in ns.iter().enumerate() {
                for b in &ns[i+1..] { assert!(!overlap(&p, &a.id, &b.id)); }
            }
        }
        // A hierarchy is never folded across a parent/child edge merely to fit a page.
        let es: Vec<_> = es.iter().map(|e| edge(&e.from, &e.to, "inheritance")).collect();
        let p = layered(&ns, &es, Direction::Down);
        for e in &es { assert!(p[&e.from][1] > p[&e.to][1]); }
    }

    #[test]
    fn compound_keeps_packages_apart_and_in_bands() {
        let ns = vec![node("Panel", Some("ui")), node("Ctl", Some("ui")), node("Svc", Some("app")), node("Entity", Some("domain")), node("Value", Some("domain")), node("Loose", None)];
        let es = vec![edge("Panel", "Ctl", "dependency"), edge("Ctl", "Svc", "dependency"), edge("Svc", "Entity", "association"), edge("Entity", "Value", "composition")];
        let groups = vec![
            GroupSpec { id: "ui".into(), rank: 0, external: false, min_w: 0.0 },
            GroupSpec { id: "app".into(), rank: 1, external: false, min_w: 0.0 },
            GroupSpec { id: "domain".into(), rank: 2, external: false, min_w: 0.0 },
        ];
        let p = compound(&ns, &es, &groups);
        assert_eq!(p.len(), 6);
        assert!(p["Panel"][1].max(p["Ctl"][1]) < p["Svc"][1], "{p:?}");
        assert!(p["Svc"][1] < p["Entity"][1].min(p["Value"][1]), "{p:?}");
        let ids: Vec<&String> = p.keys().collect();
        for a in &ids {
            for b in &ids {
                if a != b {
                    assert!(!overlap(&p, a, b), "{a} overlaps {b}");
                }
            }
        }
    }
}
