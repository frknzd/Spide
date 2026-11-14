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
}

/// Package hull padding as drawn by the class diagram (label on top).
pub struct Hull {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
}
pub const HULL: Hull = Hull { left: 28.0, top: 40.0, right: 28.0, bottom: 34.0 };

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
        comps.push(normalise(boxes));
    }
    // isolated vertices that the library dropped (no edges): one component each
    for (i, n) in nodes.iter().enumerate() {
        if !placed.contains(&i) {
            comps.push(Placed { at: vec![(n.0.clone(), [0.0, 0.0])], w: n.1, h: n.2 });
        }
    }
    pack(comps, component_gap, aspect)
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

/// Wrap a tall layered drawing into side-by-side chunks of layers (ELK's MULTI_EDGE wrapping).
fn wrap_layers(p: Placed, sizes: &HashMap<String, (f64, f64)>, gap: f64, aspect: f64) -> Placed {
    if p.h <= 0.0 || p.w / p.h >= aspect / 2.5 {
        return p;
    }
    // rows by top y
    let mut rows: BTreeMap<i64, Vec<(String, [f64; 2])>> = BTreeMap::new();
    for (id, at) in &p.at {
        rows.entry(at[1].round() as i64).or_default().push((id.clone(), *at));
    }
    let rows: Vec<Vec<(String, [f64; 2])>> = rows.into_values().collect();
    if rows.len() < 4 {
        return p;
    }
    let chunks = ((aspect * p.h / p.w.max(1.0)).sqrt().round() as usize).clamp(2, rows.len());
    let per = rows.len().div_ceil(chunks);
    let mut comps = vec![];
    for chunk in rows.chunks(per) {
        let boxes: Vec<(String, [f64; 2], f64, f64)> = chunk.iter().flatten().map(|(id, at)| (id.clone(), *at, sizes[id].0, sizes[id].1)).collect();
        comps.push(normalise(boxes));
    }
    // side by side, in layer order
    let mut out = Placed::default();
    let mut x = 0.0;
    for c in comps {
        for (id, at) in c.at {
            out.at.push((id, [at[0] + x, at[1]]));
        }
        out.h = out.h.max(c.h);
        x += c.w + gap;
        out.w = x - gap;
    }
    out
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
    let (gap, layer_gap) = if flip { (70.0, 110.0) } else { (70.0, 110.0) };
    let p = sugiyama(&ns, &es, gap, layer_gap, 120.0, 1.6);
    let sizes: HashMap<String, (f64, f64)> = ns.iter().map(|n| (n.0.clone(), (n.1, n.2))).collect();
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

/// Compound layout: package hulls can't overlap and classes stay inside their package. Each package
/// is laid out on its own (layered, wrapped towards a 1.6 aspect ratio); the packages are then
/// placed in bands by rank (ui and infrastructure on top, application, domain below), each band laid
/// out by the packages' aggregated dependencies. Returns class centre positions.
pub fn compound(nodes: &[LayoutNode], edges: &[LayoutEdge], groups: &[GroupSpec]) -> Positions {
    let ids: HashSet<&str> = nodes.iter().map(|n| n.id.as_str()).collect();
    let spec: HashMap<&str, &GroupSpec> = groups.iter().map(|g| (g.id.as_str(), g)).collect();
    let group_of: HashMap<&str, Option<&str>> = nodes.iter().map(|n| (n.id.as_str(), n.group.as_deref().filter(|g| spec.contains_key(g)))).collect();
    let pad = 12.0; // breathing room inside the drawn hull
    let mut by_group: Vec<(String, Vec<&LayoutNode>)> = vec![];
    let mut loose: Vec<&LayoutNode> = vec![];
    for n in nodes {
        match group_of[n.id.as_str()] {
            Some(g) => match by_group.iter_mut().find(|(k, _)| k == g) {
                Some((_, v)) => v.push(n),
                None => by_group.push((g.to_string(), vec![n])),
            },
            None => loose.push(n),
        }
    }
    let pkey = |g: &str| format!("pkg::{g}");
    let parent_key = |n: &str| match group_of.get(n).copied().flatten() {
        Some(g) => pkey(g),
        None => n.to_string(),
    };
    let rank_of = |key: &str| key.strip_prefix("pkg::").and_then(|g| spec.get(g)).map(|g| g.rank).unwrap_or(1);
    let is_ext = |key: &str| key.strip_prefix("pkg::").and_then(|g| spec.get(g)).is_some_and(|g| g.external);
    // edges inside a package → that package's layout; across packages → one package-level edge
    let mut inner: HashMap<String, Vec<(String, String)>> = HashMap::new();
    let mut outer: Vec<(String, String)> = vec![];
    let mut ext_clients: HashMap<String, u8> = HashMap::new();
    for e in edges {
        if !ids.contains(e.from.as_str()) || !ids.contains(e.to.as_str()) || e.from == e.to {
            continue;
        }
        let (a, b) = orient(e);
        let ga = group_of[a.as_str()];
        if ga.is_some() && ga == group_of[b.as_str()] {
            inner.entry(ga.unwrap().to_string()).or_default().push((a, b));
            continue;
        }
        let (pa, pb) = (parent_key(&a), parent_key(&b));
        // external packages sit beside their clients; packages in the same band don't push each other down
        if is_ext(&pa) || is_ext(&pb) {
            let (ext, client) = if is_ext(&pa) { (pa, pb) } else { (pb, pa) };
            if !is_ext(&client) {
                let r = ext_clients.get(&ext).copied().unwrap_or(9).min(rank_of(&client));
                ext_clients.insert(ext, r);
            }
            continue;
        }
        if rank_of(&pa) == rank_of(&pb) && pa.starts_with("pkg::") && pb.starts_with("pkg::") {
            continue;
        }
        if !outer.contains(&(pa.clone(), pb.clone())) {
            outer.push((pa, pb));
        }
    }
    // 1. each package on its own: inner positions (top-left inside the package box) and the box size
    let mut inner_pos: HashMap<String, Vec<(String, [f64; 2])>> = HashMap::new();
    let mut top: Vec<(String, f64, f64, u8)> = vec![]; // key, w, h, band
    for (g, ns) in &by_group {
        let key = pkey(g);
        let ext = is_ext(&key);
        let items: Vec<(String, f64, f64)> = ns.iter().map(|n| (n.id.clone(), n.w, n.h)).collect();
        let sizes: HashMap<String, (f64, f64)> = items.iter().map(|n| (n.0.clone(), (n.1, n.2))).collect();
        let p = if ext {
            rect_pack(&items, 16.0, 1.4)
        } else {
            let p = sugiyama(&items, inner.get(g).map(|v| v.as_slice()).unwrap_or(&[]), 48.0, 64.0, 48.0, 1.6);
            wrap_layers(p, &sizes, 48.0, 1.6)
        };
        let (ox, oy) = (HULL.left + pad, HULL.top + pad);
        inner_pos.insert(key.clone(), p.at.iter().map(|(id, at)| (id.clone(), [at[0] + ox + sizes[id].0 / 2.0, at[1] + oy + sizes[id].1 / 2.0])).collect());
        let band = if ext { ext_clients.get(&key).copied().unwrap_or(1) } else { rank_of(&key) };
        top.push((key, p.w + HULL.left + HULL.right + 2.0 * pad, p.h + HULL.top + HULL.bottom + 2.0 * pad, band));
    }
    for n in &loose {
        top.push((n.id.clone(), n.w, n.h, 1));
    }
    // 2. bands, top to bottom; inside a band, a layered drawing of the band's own dependencies,
    //    components ordered by where their neighbours sit in the bands above
    let mut bands: BTreeMap<u8, Vec<(String, f64, f64)>> = BTreeMap::new();
    for (k, w, h, b) in &top {
        bands.entry(*b).or_default().push((k.clone(), *w, *h));
    }
    let band_of: HashMap<&str, u8> = top.iter().map(|t| (t.0.as_str(), t.3)).collect();
    let mut placed: HashMap<String, [f64; 2]> = HashMap::new(); // top-left of each top-level box
    let mut sizes_top: HashMap<String, (f64, f64)> = top.iter().map(|t| (t.0.clone(), (t.1, t.2))).collect();
    let mut y = 0.0;
    for (band, members) in &bands {
        let es: Vec<(String, String)> = outer.iter().filter(|(a, b)| band_of.get(a.as_str()) == Some(band) && band_of.get(b.as_str()) == Some(band)).cloned().collect();
        let p = sugiyama(members, &es, 70.0, 90.0, 90.0, 4.0);
        // order the band's members by the barycentre of their neighbours above (crossing reduction)
        let mut bary: Vec<(String, [f64; 2], f64)> = p
            .at
            .iter()
            .map(|(id, at)| {
                let xs: Vec<f64> = outer
                    .iter()
                    .filter_map(|(a, b)| {
                        let other = if a == id { b } else if b == id { a } else { return None };
                        placed.get(other).map(|q| q[0] + sizes_top[other].0 / 2.0)
                    })
                    .collect();
                let bx = if xs.is_empty() { at[0] } else { xs.iter().sum::<f64>() / xs.len() as f64 };
                (id.clone(), *at, bx)
            })
            .collect();
        let has_above = bary.iter().any(|b| b.2 != b.1[0]);
        if has_above && es.is_empty() {
            // a single row of independent boxes: reorder by barycentre and re-flow
            bary.sort_by(|a, b| a.2.total_cmp(&b.2));
            let mut x = 0.0;
            for (id, at, _) in &mut bary {
                at[0] = x;
                at[1] = 0.0;
                x += sizes_top[id.as_str()].0 + 70.0;
            }
        }
        let w = bary.iter().map(|b| b.1[0] + sizes_top[&b.0].0).fold(0.0, f64::max);
        let h = bary.iter().map(|b| b.1[1] + sizes_top[&b.0].1).fold(0.0, f64::max);
        // centre the band under the barycentre of what it connects to (or at 0)
        let target: Vec<f64> = bary.iter().filter(|b| b.2 != b.1[0]).map(|b| b.2).collect();
        let cx = if target.is_empty() { 0.0 } else { target.iter().sum::<f64>() / target.len() as f64 };
        let x0 = cx - w / 2.0;
        for (id, at, _) in bary {
            placed.insert(id, [at[0] + x0, at[1] + y]);
        }
        y += h + 90.0;
    }
    // 3. class centres
    let mut res: Positions = HashMap::new();
    for (key, at) in &placed {
        if let Some(inner) = inner_pos.get(key) {
            for (id, p) in inner {
                res.insert(id.clone(), [at[0] + p[0], at[1] + p[1]]);
            }
        } else if let Some(n) = loose.iter().find(|n| &n.id == key) {
            res.insert(key.clone(), [at[0] + n.w / 2.0, at[1] + n.h / 2.0]);
        }
    }
    sizes_top.clear();
    // centre on the origin
    let n = res.len().max(1) as f64;
    let cx = res.values().map(|p| p[0]).sum::<f64>() / n;
    let cy = res.values().map(|p| p[1]).sum::<f64>() / n;
    for p in res.values_mut() {
        p[0] -= cx;
        p[1] -= cy;
    }
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: &str, g: Option<&str>) -> LayoutNode {
        LayoutNode { id: id.into(), w: 140.0, h: 80.0, group: g.map(Into::into) }
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
    fn compound_keeps_packages_apart_and_in_bands() {
        let ns = vec![node("Panel", Some("ui")), node("Ctl", Some("ui")), node("Svc", Some("app")), node("Entity", Some("domain")), node("Value", Some("domain")), node("Loose", None)];
        let es = vec![edge("Panel", "Ctl", "dependency"), edge("Ctl", "Svc", "dependency"), edge("Svc", "Entity", "association"), edge("Entity", "Value", "composition")];
        let groups = vec![
            GroupSpec { id: "ui".into(), rank: 0, external: false },
            GroupSpec { id: "app".into(), rank: 1, external: false },
            GroupSpec { id: "domain".into(), rank: 2, external: false },
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
