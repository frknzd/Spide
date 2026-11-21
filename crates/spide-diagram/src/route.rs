//! Orthogonal ("PCB") edge router: north/south/east/west segments only, distinct neighbor-ordered ports,
//! no segment through a box, parallel segments nudged into lanes, hops where lines cross.
//! Pure and deterministic; positions are whatever the layout/physics currently says.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

pub type Pt = [f64; 2];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    N,
    S,
    E,
    W,
}

impl Side {
    fn dir(self) -> Pt {
        match self {
            Side::N => [0.0, -1.0],
            Side::S => [0.0, 1.0],
            Side::E => [1.0, 0.0],
            Side::W => [-1.0, 0.0],
        }
    }
    fn opp(self) -> Side {
        match self {
            Side::N => Side::S,
            Side::S => Side::N,
            Side::E => Side::W,
            Side::W => Side::E,
        }
    }
    fn index(self) -> usize {
        match self {
            Side::N => 0,
            Side::E => 1,
            Side::S => 2,
            Side::W => 3,
        }
    }
    fn letter(self) -> &'static str {
        match self {
            Side::N => "N",
            Side::S => "S",
            Side::E => "E",
            Side::W => "W",
        }
    }
    pub fn vertical(self) -> bool {
        matches!(self, Side::N | Side::S)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Deco {
    #[default]
    None,
    Arrow,
    Triangle,
    Diamond,
    ODiamond,
}

impl Deco {
    fn name(self) -> &'static str {
        match self {
            Deco::None => "none",
            Deco::Arrow => "arrow",
            Deco::Triangle => "triangle",
            Deco::Diamond => "diamond",
            Deco::ODiamond => "odiamond",
        }
    }
    fn order(name: &str) -> i32 {
        match name {
            "triangle" => 0,
            "diamond" => 1,
            "odiamond" => 2,
            "arrow" => 3,
            "none" => 4,
            _ => -1,
        }
    }
}

/// A box (centre coordinates). `pad_top` reserves room above it (chips drawn over the top border).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RBox {
    pub id: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub pad_top: f64,
    pub pad_bottom: f64,
}

impl RBox {
    pub fn new(id: impl Into<String>, x: f64, y: f64, w: f64, h: f64) -> Self {
        Self { id: id.into(), x, y, w, h, pad_top: 0.0, pad_bottom: 0.0 }
    }
}

/// A soft zone (package hull): crossing it costs, its own members' edges may run inside freely.
#[derive(Debug, Clone, Default)]
pub struct RZone {
    pub id: String,
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
    pub members: HashSet<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct REdge {
    pub id: String,
    pub from: String,
    pub to: String,
    /// Decoration drawn at each end; each relationship has its own attachment.
    pub from_deco: Deco,
    pub to_deco: Deco,
    /// prefer vertical sides (inheritance: child below its parent)
    pub vertical: bool,
}

impl REdge {
    pub fn new(id: impl Into<String>, from: impl Into<String>, to: impl Into<String>) -> Self {
        Self { id: id.into(), from: from.into(), to: to.into(), ..Default::default() }
    }
    pub fn deco(mut self, from: Deco, to: Deco, vertical: bool) -> Self {
        self.from_deco = from;
        self.to_deco = to;
        self.vertical = vertical;
        self
    }
}

/// Router decorations for a relation kind (where its arrowhead / diamond sits).
pub fn deco_for(kind: &str, directed: bool) -> (Deco, Deco, bool) {
    match kind {
        "inheritance" | "realization" => (Deco::None, Deco::Triangle, true),
        "composition" => (Deco::Diamond, Deco::None, false),
        "aggregation" => (Deco::ODiamond, Deco::None, false),
        "dependency" | "flow" => (Deco::None, Deco::Arrow, false),
        "association" if directed => (Deco::None, Deco::Arrow, false),
        _ => (Deco::None, Deco::None, false),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Route {
    pub points: Vec<Pt>,
    /// crossings drawn as a small hop on this edge (always on its horizontal segments)
    pub hops: Vec<Pt>,
    pub from_side: Side,
    pub to_side: Side,
    /// true when the A* search was skipped (budget, no path): a plain L/Z route
    pub fallback: bool,
    /// points before lane nudging (reused for incremental re-routing)
    pub raw: Vec<Pt>,
}

pub struct RouteOptions<'a> {
    pub margin: f64,
    pub stub: f64,
    pub port_gap: f64,
    pub lane: f64,
    /// cost of one turn (in px of detour)
    pub bend: f64,
    /// cost of crossing another edge, and of running on top of one (per px + per overlap)
    pub cross: f64,
    pub overlap: f64,
    /// total time budget (ms); edges beyond it get L/Z routes
    pub budget_ms: f64,
    /// only re-route these edges; the rest keep `previous`
    pub only: Option<&'a HashSet<String>>,
    pub previous: Option<&'a HashMap<String, Route>>,
    pub now: Option<Box<dyn FnMut() -> f64 + 'a>>,
}

impl Default for RouteOptions<'_> {
    fn default() -> Self {
        Self { margin: 12.0, stub: 24.0, port_gap: 14.0, lane: 12.0, bend: 140.0, cross: 70.0, overlap: 5.0, budget_ms: 80.0, only: None, previous: None, now: None }
    }
}

#[derive(Clone)]
struct Port {
    side: Side,
    at: Pt,
    stub_end: Pt,
    key: u32,
}

#[derive(Clone, Copy)]
struct Obs {
    x1: f64,
    x2: f64,
    y1: f64,
    y2: f64,
}

fn choose_sides(a: &RBox, b: &RBox, vertical: bool) -> (Side, Side) {
    let gap_y = (b.y - b.h / 2.0 - (a.y + a.h / 2.0)).max(a.y - a.h / 2.0 - (b.y + b.h / 2.0));
    let gap_x = (b.x - b.w / 2.0 - (a.x + a.w / 2.0)).max(a.x - a.w / 2.0 - (b.x + b.w / 2.0));
    let use_v = if vertical { gap_y > -a.h.min(b.h) / 2.0 || gap_x < 0.0 } else { gap_y >= gap_x };
    if use_v {
        if b.y >= a.y {
            (Side::S, Side::N)
        } else {
            (Side::N, Side::S)
        }
    } else if b.x >= a.x {
        (Side::E, Side::W)
    } else {
        (Side::W, Side::E)
    }
}

/// Exact-value key for coordinates (JS `Set<number>` semantics).
fn fkey(v: f64) -> u64 {
    if v == 0.0 {
        0
    } else {
        v.to_bits()
    }
}

struct Occ {
    a: f64,
    b: f64,
    groups: [u32; 2],
}

struct Router<'b> {
    boxes: &'b [RBox],
    obs: Vec<Obs>,
    zones: &'b [RZone],
    base_xs: Vec<f64>,
    base_ys: Vec<f64>,
    occ_h: HashMap<i64, Vec<Occ>>,
    occ_v: HashMap<i64, Vec<Occ>>,
    keys_h: Vec<i64>,
    keys_v: Vec<i64>,
    bend: f64,
    cross: f64,
    overlap_px: f64,
}

fn insert_key(arr: &mut Vec<i64>, k: i64) {
    if let Err(i) = arr.binary_search(&k) {
        arr.insert(i, k);
    }
}

fn shares(a: &[u32; 2], b: &[u32; 2]) -> bool {
    a.iter().any(|g| b.contains(g))
}

impl Router<'_> {
    fn occupy(&mut self, pts: &[Pt], groups: [u32; 2]) {
        for w in pts.windows(2) {
            let (p, q) = (w[0], w[1]);
            if p[1] == q[1] {
                let k = p[1].round() as i64;
                self.occ_h.entry(k).or_default().push(Occ { a: p[0].min(q[0]), b: p[0].max(q[0]), groups });
                insert_key(&mut self.keys_h, k);
            } else {
                let k = p[0].round() as i64;
                self.occ_v.entry(k).or_default().push(Occ { a: p[1].min(q[1]), b: p[1].max(q[1]), groups });
                insert_key(&mut self.keys_v, k);
            }
        }
    }

    /// Perpendicular edges a candidate segment crosses (edges sharing a port with it don't count).
    fn cross_count(&self, horizontal: bool, c: f64, a: f64, b: f64, groups: &[u32; 2]) -> f64 {
        let (keys, occ) = if horizontal { (&self.keys_v, &self.occ_v) } else { (&self.keys_h, &self.occ_h) };
        // first key > a
        let lo = keys.partition_point(|&k| (k as f64) <= a);
        let mut n = 0.0;
        for &k in &keys[lo..] {
            if (k as f64) >= b {
                break;
            }
            for s in &occ[&k] {
                if c > s.a + 0.5 && c < s.b - 0.5 && !shares(&s.groups, groups) {
                    n += 1.0;
                }
            }
        }
        n
    }

    fn overlap_cost(&self, horizontal: bool, c: f64, a: f64, b: f64, groups: &[u32; 2]) -> f64 {
        let occ = if horizontal { &self.occ_h } else { &self.occ_v };
        let r = c.round() as i64;
        let mut cost = 0.0;
        for k in r - 4..=r + 4 {
            if let Some(l) = occ.get(&k) {
                for s in l {
                    if shares(&s.groups, groups) {
                        continue; // shared trunk: free
                    }
                    let ov = b.min(s.b) - a.max(s.a);
                    if ov > 0.0 {
                        cost += ov * self.overlap_px + 2.0 * self.cross;
                    }
                }
            }
        }
        cost
    }

    fn soft_cost(&self, e: &REdge, p: Pt, q: Pt) -> f64 {
        let mut c = 0.0;
        let (mx, my) = ((p[0] + q[0]) / 2.0, (p[1] + q[1]) / 2.0);
        let len = (p[0] - q[0]).abs() + (p[1] - q[1]).abs();
        for z in self.zones {
            let in_p = p[0] > z.x1 && p[0] < z.x2 && p[1] > z.y1 && p[1] < z.y2;
            let in_q = q[0] > z.x1 && q[0] < z.x2 && q[1] > z.y1 && q[1] < z.y2;
            if in_p != in_q {
                c += 30.0;
            }
            if !z.members.contains(&e.from) && !z.members.contains(&e.to) && mx > z.x1 && mx < z.x2 && my > z.y1 && my < z.y2 {
                c += len * 3.0;
            }
            // don't trace a package frame: a line running along its border reads as part of the frame
            let along = if p[1] == q[1] {
                ((p[1] - z.y1).abs() < 8.0 || (p[1] - z.y2).abs() < 8.0) && mx > z.x1 && mx < z.x2
            } else {
                ((p[0] - z.x1).abs() < 8.0 || (p[0] - z.x2).abs() < 8.0) && my > z.y1 && my < z.y2
            };
            if along {
                c += len * 2.0;
            }
        }
        c
    }
}

/// A* on a grid of candidate lines (through the given ports' stubs).
struct Search {
    xs: Vec<f64>,
    ys: Vec<f64>,
    gxi: HashMap<u64, usize>,
    gyi: HashMap<u64, usize>,
    g_free: Vec<u8>,
    h_free: Vec<u8>,
    v_free: Vec<u8>,
}

impl Search {
    fn new(r: &Router, ports: &[&Port]) -> Search {
        let mut xs = r.base_xs.clone();
        let mut ys = r.base_ys.clone();
        for p in ports {
            xs.push(p.stub_end[0]);
            ys.push(p.stub_end[1]);
        }
        xs.sort_by(|a, b| a.total_cmp(b));
        xs.dedup_by(|a, b| fkey(*a) == fkey(*b));
        ys.sort_by(|a, b| a.total_cmp(b));
        ys.dedup_by(|a, b| fkey(*a) == fkey(*b));
        let (nx, ny) = (xs.len(), ys.len());
        let gxi = xs.iter().enumerate().map(|(i, x)| (fkey(*x), i)).collect();
        let gyi = ys.iter().enumerate().map(|(i, y)| (fkey(*y), i)).collect();
        let mut g_free = vec![1u8; nx * ny];
        let mut h_free = vec![1u8; nx * ny];
        let mut v_free = vec![1u8; nx * ny];
        // first index with value > v
        let lo = |arr: &[f64], v: f64| arr.partition_point(|&x| x <= v);
        for o in &r.obs {
            let (i0, i1) = (lo(&xs, o.x1 + 0.01), lo(&xs, o.x2 - 0.01 - 1e-9));
            let (j0, j1) = (lo(&ys, o.y1 + 0.01), lo(&ys, o.y2 - 0.01 - 1e-9));
            for i in i0..i1 {
                for j in j0..j1 {
                    g_free[i * ny + j] = 0;
                }
            }
            let (ih0, ih1) = (i0.saturating_sub(1), i1.min(nx.saturating_sub(1)));
            for j in j0..j1 {
                for i in ih0..ih1 {
                    if xs[i] < o.x2 - 0.01 && xs[i + 1] > o.x1 + 0.01 {
                        h_free[i * ny + j] = 0;
                    }
                }
            }
            let (jv0, jv1) = (j0.saturating_sub(1), j1.min(ny.saturating_sub(1)));
            for i in i0..i1 {
                for j in jv0..jv1 {
                    if ys[j] < o.y2 - 0.01 && ys[j + 1] > o.y1 + 0.01 {
                        v_free[i * ny + j] = 0;
                    }
                }
            }
        }
        Search { xs, ys, gxi, gyi, g_free, h_free, v_free }
    }

    fn run(&self, r: &Router, e: &REdge, s: &Port, t: &Port, pad: f64) -> Option<(Vec<Pt>, f64)> {
        let (xs_all, ys_all) = (&self.xs, &self.ys);
        let (big_nx, big_ny) = (xs_all.len(), ys_all.len());
        let min_x = s.stub_end[0].min(t.stub_end[0]) - pad;
        let max_x = s.stub_end[0].max(t.stub_end[0]) + pad;
        let min_y = s.stub_end[1].min(t.stub_end[1]) - pad;
        let max_y = s.stub_end[1].max(t.stub_end[1]) + pad;
        let mut ox = 0;
        while ox < big_nx && xs_all[ox] < min_x {
            ox += 1;
        }
        let mut ex = ox;
        while ex < big_nx && xs_all[ex] <= max_x {
            ex += 1;
        }
        let mut oy = 0;
        while oy < big_ny && ys_all[oy] < min_y {
            oy += 1;
        }
        let mut ey = oy;
        while ey < big_ny && ys_all[ey] <= max_y {
            ey += 1;
        }
        let (nx, ny) = (ex - ox, ey - oy);
        let xs = &xs_all[ox..ex];
        let ys = &ys_all[oy..ey];
        let idx = |m: &HashMap<u64, usize>, v: f64, o: usize| m.get(&fkey(v)).map(|&i| i as i64 - o as i64).unwrap_or(-1);
        let (si, sj, ti, tj) = (idx(&self.gxi, s.stub_end[0], ox), idx(&self.gyi, s.stub_end[1], oy), idx(&self.gxi, t.stub_end[0], ox), idx(&self.gyi, t.stub_end[1], oy));
        if si < 0 || sj < 0 || ti < 0 || tj < 0 || si >= nx as i64 || ti >= nx as i64 || sj >= ny as i64 || tj >= ny as i64 {
            return None;
        }
        let (si, sj, ti, tj) = (si as usize, sj as usize, ti as usize, tj as usize);
        let is_end = |i: usize, j: usize| (i == si && j == sj) || (i == ti && j == tj);
        let is_free = |i: usize, j: usize| is_end(i, j) || self.g_free[(i + ox) * big_ny + j + oy] == 1;
        let seg_free = |i: usize, j: usize, ni: usize, nj: usize| {
            if j == nj {
                let a = i.min(ni);
                self.h_free[(a + ox) * big_ny + j + oy] == 1 || is_end(i, j) || is_end(ni, nj)
            } else {
                let b = j.min(nj);
                self.v_free[(i + ox) * big_ny + b + oy] == 1 || is_end(i, j) || is_end(ni, nj)
            }
        };
        let groups = [s.key, t.key];
        const D: [(i64, i64); 4] = [(0, -1), (1, 0), (0, 1), (-1, 0)]; // N E S W
        let start_dir = s.side.index();
        let end_dir = t.side.opp().index();
        let n = nx * ny * 4;
        let mut h_cache = vec![-1.0f64; nx * ny];
        let mut v_cache = vec![-1.0f64; nx * ny];
        let mut g = vec![f64::INFINITY; n];
        let mut prev = vec![-1i64; n];
        let bend = r.bend;
        // slightly greedy (×1.25): much faster, routes stay near-optimal
        let h = |i: usize, j: usize| 1.25 * ((xs[i] - xs[ti]).abs() + (ys[j] - ys[tj]).abs() + if i != ti && j != tj { bend } else { 0.0 });
        let mut heap = Heap::default();
        let s0 = (si * ny + sj) * 4 + start_dir;
        g[s0] = 0.0;
        heap.push(h(si, sj), s0);
        let mut goal: i64 = -1;
        while let Some((f, st)) = heap.pop() {
            let d = st & 3;
            let cell = st >> 2;
            let (i, j) = (cell / ny, cell % ny);
            if f - h(i, j) > g[st] + 1e-6 {
                continue;
            }
            if i == ti && j == tj {
                goal = st as i64;
                break;
            }
            for (nd, dd) in D.iter().enumerate() {
                if nd == ((d + 2) & 3) {
                    continue;
                }
                let (ni, nj) = (i as i64 + dd.0, j as i64 + dd.1);
                if ni < 0 || nj < 0 || ni >= nx as i64 || nj >= ny as i64 {
                    continue;
                }
                let (ni, nj) = (ni as usize, nj as usize);
                if !is_free(ni, nj) || !seg_free(i, j, ni, nj) {
                    continue;
                }
                let p = [xs[i], ys[j]];
                let q = [xs[ni], ys[nj]];
                let horiz = nd == 1 || nd == 3;
                // segment cost (length, zones, overlaps, crossings) is direction-free: cache it per unit segment
                let ci = if horiz { i.min(ni) * ny + j } else { i * ny + j.min(nj) };
                let cache = if horiz { &mut h_cache } else { &mut v_cache };
                let mut seg_cost = cache[ci];
                if seg_cost < 0.0 {
                    let (lo, hi, at) = if horiz { (p[0].min(q[0]), p[0].max(q[0]), p[1]) } else { (p[1].min(q[1]), p[1].max(q[1]), p[0]) };
                    seg_cost = hi - lo + r.soft_cost(e, p, q) + r.overlap_cost(horiz, at, lo, hi, &groups) + r.cross * r.cross_count(horiz, at, lo, hi, &groups);
                    cache[ci] = seg_cost;
                }
                let mut c = seg_cost + if nd != d { bend } else { 0.0 };
                if ni == ti && nj == tj && nd != end_dir {
                    c += bend * if nd == ((end_dir + 2) & 3) { 3.0 } else { 1.0 };
                }
                let ns = (ni * ny + nj) * 4 + nd;
                if g[st] + c < g[ns] - 1e-9 {
                    g[ns] = g[st] + c;
                    prev[ns] = st as i64;
                    heap.push(g[ns] + h(ni, nj), ns);
                }
            }
        }
        if goal < 0 {
            return None;
        }
        let mut pts = vec![];
        let mut st = goal;
        while st >= 0 {
            let cell = (st as usize) >> 2;
            pts.push([xs[cell / ny], ys[cell % ny]]);
            st = prev[st as usize];
        }
        pts.reverse();
        Some((pts, g[goal as usize]))
    }
}

/// Binary min-heap on (f, state), same tie behaviour as the JS version.
#[derive(Default)]
struct Heap {
    f: Vec<f64>,
    s: Vec<usize>,
}

impl Heap {
    fn push(&mut self, f: f64, st: usize) {
        self.f.push(f);
        self.s.push(st);
        let mut i = self.f.len() - 1;
        while i > 0 {
            let p = (i - 1) >> 1;
            if self.f[p] <= f {
                break;
            }
            self.f[i] = self.f[p];
            self.s[i] = self.s[p];
            i = p;
        }
        self.f[i] = f;
        self.s[i] = st;
    }
    fn pop(&mut self) -> Option<(f64, usize)> {
        if self.f.is_empty() {
            return None;
        }
        let (top_f, top_s) = (self.f[0], self.s[0]);
        let f = self.f.pop().unwrap();
        let x = self.s.pop().unwrap();
        let hn = self.f.len();
        if hn == 0 {
            return Some((top_f, top_s));
        }
        let mut i = 0;
        loop {
            let l = 2 * i + 1;
            if l >= hn {
                break;
            }
            let m = if l + 1 < hn && self.f[l + 1] < self.f[l] { l + 1 } else { l };
            if self.f[m] >= f {
                break;
            }
            self.f[i] = self.f[m];
            self.s[i] = self.s[m];
            i = m;
        }
        self.f[i] = f;
        self.s[i] = x;
        Some((top_f, top_s))
    }
}

fn port_at(b: &RBox, side: Side, off: f64, key: u32, stub_len: f64, margin: f64) -> Port {
    let d = side.dir();
    let at = if side.vertical() { [b.x + off, b.y + (d[1] * b.h) / 2.0] } else { [b.x + (d[0] * b.w) / 2.0, b.y + off] };
    let len = match side {
        Side::N => stub_len.max(b.pad_top + margin + 4.0),
        Side::S => stub_len.max(b.pad_bottom + margin + 4.0),
        _ => stub_len,
    };
    Port { side, at, stub_end: [at[0] + d[0] * len, at[1] + d[1] * len], key }
}

/// Route every edge.
pub fn route_all(boxes: &[RBox], edges: &[REdge], zones: &[RZone], mut o: RouteOptions) -> HashMap<String, Route> {
    let (margin, stub_len, port_gap, lane) = (o.margin, o.stub, o.port_gap, o.lane);
    let started = Instant::now();
    let mut now: Box<dyn FnMut() -> f64> = match o.now.take() {
        Some(f) => f,
        None => Box::new(move || started.elapsed().as_secs_f64() * 1000.0),
    };
    let t0 = now();
    let budget = o.budget_ms;
    let by_id: HashMap<&str, &RBox> = boxes.iter().map(|b| (b.id.as_str(), b)).collect();
    let mut out: HashMap<String, Route> = HashMap::new();
    let live: Vec<&REdge> = edges.iter().filter(|e| by_id.contains_key(e.from.as_str()) && by_id.contains_key(e.to.as_str())).collect();

    // group-key interning
    let mut interned: HashMap<String, u32> = HashMap::new();
    let mut names: Vec<String> = vec![];
    let mut intern = |k: String| -> u32 {
        if let Some(&i) = interned.get(&k) {
            return i;
        }
        let i = names.len() as u32;
        names.push(k.clone());
        interned.insert(k, i);
        i
    };

    // Each relationship has its own attachment, including opposite/parallel edges.
    // Decoration equality does not imply a shared UML relationship.
    let group_key = |e: &REdge, end: usize, side: Side| -> String {
        let b = if end == 1 { &e.to } else { &e.from };
        let deco = if end == 1 { e.to_deco } else { e.from_deco };
        format!("{b}|{}|{}#{}:{end}", side.letter(), deco.name(), e.id)
    };
    // distance from a box side to the nearest *other* box straight ahead (Infinity: open)
    let clearance = |b: &RBox, side: Side, ignore: &str| -> f64 {
        let d = side.dir();
        let mut best = f64::INFINITY;
        for o2 in boxes {
            if o2.id == b.id || o2.id == ignore {
                continue;
            }
            let (ox1, ox2) = (o2.x - o2.w / 2.0, o2.x + o2.w / 2.0);
            let (oy1, oy2) = (o2.y - o2.h / 2.0 - o2.pad_top, o2.y + o2.h / 2.0 + o2.pad_bottom);
            if d[0] == 0.0 {
                if ox2 < b.x - 14.0 || ox1 > b.x + 14.0 {
                    continue;
                }
                let edge = if d[1] < 0.0 { b.y - b.h / 2.0 - b.pad_top } else { b.y + b.h / 2.0 + b.pad_bottom };
                let dist = if d[1] < 0.0 { edge - oy2 } else { oy1 - edge };
                if dist >= -1.0 {
                    best = best.min(dist.max(0.0));
                }
            } else {
                if oy2 < b.y - 14.0 || oy1 > b.y + 14.0 {
                    continue;
                }
                let edge = if d[0] < 0.0 { b.x - b.w / 2.0 } else { b.x + b.w / 2.0 };
                let dist = if d[0] < 0.0 { edge - ox2 } else { ox1 - edge };
                if dist >= -1.0 {
                    best = best.min(dist.max(0.0));
                }
            }
        }
        best
    };
    let need = stub_len + margin + 2.0;
    let pick_side = |b: &RBox, other: &RBox, first: Side| -> Side {
        if clearance(b, first, &other.id) >= need {
            return first;
        }
        let horiz = matches!(first, Side::E | Side::W);
        let alts = if horiz {
            if other.y >= b.y {
                [Side::S, Side::N]
            } else {
                [Side::N, Side::S]
            }
        } else if other.x >= b.x {
            [Side::E, Side::W]
        } else {
            [Side::W, Side::E]
        };
        for a in [alts[0], alts[1], first.opp()] {
            if clearance(b, a, &other.id) >= need {
                return a;
            }
        }
        first
    };
    // ------------------------------------------------------------- obstacles and candidate lines
    let obs: Vec<Obs> = boxes
        .iter()
        .map(|b| Obs { x1: b.x - b.w / 2.0 - margin, x2: b.x + b.w / 2.0 + margin, y1: b.y - b.h / 2.0 - margin - b.pad_top, y2: b.y + b.h / 2.0 + margin + b.pad_bottom })
        .collect();
    let mids = |vals: Vec<f64>| -> Vec<f64> {
        let mut v = vals;
        v.sort_by(|a, b| a.total_cmp(b));
        v.dedup_by(|a, b| fkey(*a) == fkey(*b));
        let mut extra = vec![];
        for i in 1..v.len() {
            if v[i] - v[i - 1] > 30.0 {
                extra.push((v[i] + v[i - 1]) / 2.0);
            }
        }
        v.extend(extra);
        v
    };
    let base_xs = mids(obs.iter().flat_map(|r| [r.x1, r.x2]).collect());
    let base_ys = mids(obs.iter().flat_map(|r| [r.y1, r.y2]).collect());
    let mut router = Router { boxes, obs: obs.clone(), zones, base_xs, base_ys, occ_h: HashMap::new(), occ_v: HashMap::new(), keys_h: vec![], keys_v: vec![], bend: o.bend, cross: o.cross, overlap_px: o.overlap };
    let _ = router.boxes;

    let mut sides: HashMap<String, (Side, Side)> = HashMap::new();
    for e in &live {
        if e.from == e.to {
            continue;
        }
        let (a, b) = (by_id[e.from.as_str()], by_id[e.to.as_str()]);
        let c = choose_sides(a, b, e.vertical);
        sides.insert(e.id.clone(), (pick_side(a, b, c.0), pick_side(b, a, c.1)));
    }
    // Diagonal relations: try the other port pairs (opposite sides the other way, and the two L shapes)
    // and keep whichever routes with the fewest turns / cheapest path through the obstacles.
    {
        let mut cands: Vec<(&REdge, Vec<(Side, Side)>)> = vec![];
        for e in &live {
            if e.from == e.to || e.vertical {
                continue;
            }
            let (a, b) = (by_id[e.from.as_str()], by_id[e.to.as_str()]);
            let gap_y = (b.y - b.h / 2.0 - (a.y + a.h / 2.0)).max(a.y - a.h / 2.0 - (b.y + b.h / 2.0));
            let gap_x = (b.x - b.w / 2.0 - (a.x + a.w / 2.0)).max(a.x - a.w / 2.0 - (b.x + b.w / 2.0));
            if gap_x <= 2.0 * stub_len || gap_y <= 2.0 * stub_len {
                continue; // not diagonal: the facing sides are right
            }
            let v = if b.y > a.y { Side::S } else { Side::N };
            let h = if b.x > a.x { Side::E } else { Side::W };
            let list = [sides[&e.id], (v, v.opp()), (h, h.opp()), (h, v.opp()), (v, h.opp())];
            let mut uniq: Vec<(Side, Side)> = vec![];
            for x in list {
                if !uniq.contains(&x) {
                    uniq.push(x);
                }
            }
            uniq.retain(|x| clearance(a, x.0, &b.id) >= need && clearance(b, x.1, &a.id) >= need);
            cands.push((e, uniq));
        }
        if !cands.is_empty() {
            let mut pre: HashMap<String, Port> = HashMap::new();
            let mut pre_order: Vec<String> = vec![];
            for (e, list) in &cands {
                for (fs, ts) in list {
                    for (bx, sd) in [(&e.from, *fs), (&e.to, *ts)] {
                        let k = format!("{bx}|{}", sd.letter());
                        if !pre.contains_key(&k) {
                            pre_order.push(k.clone());
                        }
                        let key = intern(format!("pre|{k}"));
                        pre.insert(k, port_at(by_id[bx.as_str()], sd, 0.0, key, stub_len, margin));
                    }
                }
            }
            let plist: Vec<&Port> = pre_order.iter().map(|k| &pre[k]).collect();
            let pre_search = Search::new(&router, &plist);
            for (e, list) in &cands {
                if list.len() < 2 || now() - t0 > budget / 3.0 {
                    continue;
                }
                let mut best = f64::INFINITY;
                let mut pick = sides[&e.id];
                for (fs, ts) in list {
                    let s = &pre[&format!("{}|{}", e.from, fs.letter())];
                    let t = &pre[&format!("{}|{}", e.to, ts.letter())];
                    if let Some((_, cost)) = pre_search.run(&router, e, s, t, 160.0) {
                        if cost < best - 1.0 {
                            best = cost;
                            pick = (*fs, *ts);
                        }
                    }
                }
                sides.insert(e.id.clone(), pick);
            }
        }
    }
    let mut ends: Vec<(String, Vec<(Side, String)>)> = vec![];
    let mut destination: HashMap<String, f64> = HashMap::new();
    for e in &live {
        if e.from == e.to {
            continue;
        }
        let s = sides[&e.id];
        for end in [0usize, 1] {
            let (bx, sd) = if end == 1 { (&e.to, s.1) } else { (&e.from, s.0) };
            let key = group_key(e, end, sd);
            let other = by_id[if end == 1 { e.from.as_str() } else { e.to.as_str() }];
            destination.insert(key.clone(), if sd.vertical() { other.x } else { other.y });
            let bs = format!("{bx}|{}", sd.letter());
            let slot = match ends.iter().position(|(k, _)| *k == bs) {
                Some(i) => i,
                None => {
                    ends.push((bs, vec![]));
                    ends.len() - 1
                }
            };
            if !ends[slot].1.iter().any(|x| x.1 == key) {
                ends[slot].1.push((sd, key));
            }
        }
    }
    let mut ports: HashMap<String, Port> = HashMap::new();
    let mut port_order: Vec<String> = vec![];
    for (bs, list) in &mut ends {
        let bx = by_id[&bs[..bs.rfind('|').unwrap()]];
        let deco_of = |k: &str| Deco::order(k.split('|').nth(2).unwrap_or("").split('#').next().unwrap_or(""));
        list.sort_by(|a, b| destination[&a.1].total_cmp(&destination[&b.1])
            .then_with(|| deco_of(&a.1).cmp(&deco_of(&b.1))).then_with(|| a.1.cmp(&b.1)));
        let n = list.len() as f64;
        for (i, (side, key)) in list.iter().enumerate() {
            let along = if side.vertical() { bx.w } else { bx.h };
            let span = (along / 2.0 - 18.0).max(along * 0.2);
            // Spread over the side, not a fixed cluster at its centre. For very dense
            // sides compress all slots evenly instead of clamping many to one point.
            let step = if n > 1.0 { (along / (n + 1.0)).max(port_gap).min(2.0 * span / (n - 1.0)) } else { 0.0 };
            let off = (i as f64 - (n - 1.0) / 2.0) * step;
            let id = intern(key.clone());
            port_order.push(key.clone());
            ports.insert(key.clone(), port_at(bx, *side, off, id, stub_len, margin));
        }
    }
    let plist: Vec<&Port> = port_order.iter().map(|k| &ports[k]).collect();
    let search = Search::new(&router, &plist);

    // a segment may pass if it stays clear of every box by half the margin
    let blocked = |p: Pt, q: Pt| -> bool {
        let (x1, x2, y1, y2) = (p[0].min(q[0]), p[0].max(q[0]), p[1].min(q[1]), p[1].max(q[1]));
        let m = margin / 2.0;
        obs.iter().any(|r| x2 > r.x1 + m && x1 < r.x2 - m && y2 > r.y1 + m && y1 < r.y2 - m)
    };
    let fallback_route = |s: &Port, t: &Port| -> Vec<Pt> {
        let (a, b) = (s.stub_end, t.stub_end);
        if s.side.vertical() {
            if t.side.vertical() {
                let my = (a[1] + b[1]) / 2.0;
                return vec![a, [a[0], my], [b[0], my], b];
            }
            return vec![a, [a[0], b[1]], b];
        }
        if !t.side.vertical() {
            let mx = (a[0] + b[0]) / 2.0;
            return vec![a, [mx, a[1]], [mx, b[1]], b];
        }
        vec![a, [b[0], a[1]], b]
    };

    // ------------------------------------------------------------- route (short edges first: they get the clean paths)
    let mut todo: Vec<&REdge> = live.iter().copied().filter(|e| o.only.is_none_or(|only| only.contains(&e.id))).collect();
    for e in &live {
        let only = o.only;
        if let Some(only) = only {
            if !only.contains(&e.id) {
                if let Some(r) = o.previous.and_then(|p| p.get(&e.id)) {
                    let base = if r.raw.is_empty() { r.points.clone() } else { r.raw.clone() };
                    out.insert(e.id.clone(), Route { points: base.clone(), hops: vec![], ..r.clone() });
                    let g = [intern(group_key(e, 0, r.from_side)), intern(group_key(e, 1, r.to_side))];
                    router.occupy(&base, g);
                } else {
                    todo.push(e);
                }
            }
        }
    }
    let dist = |e: &REdge| {
        let (a, b) = (by_id[e.from.as_str()], by_id[e.to.as_str()]);
        (a.x - b.x).abs() + (a.y - b.y).abs()
    };
    todo.sort_by(|a, b| dist(a).total_cmp(&dist(b)).then_with(|| a.id.cmp(&b.id)));
    for e in todo {
        if e.from == e.to {
            let b = by_id[e.from.as_str()];
            let (r, t, l) = (b.x + b.w / 2.0, b.y - b.h / 2.0, 24.0 + b.pad_top);
            let points = vec![[r - 22.0, t], [r - 22.0, t - l], [r + 24.0, t - l], [r + 24.0, t + 18.0], [r, t + 18.0]];
            out.insert(e.id.clone(), Route { points, hops: vec![], from_side: Side::N, to_side: Side::E, fallback: false, raw: vec![] });
            continue;
        }
        let (fs, ts) = sides[&e.id];
        let s = &ports[&group_key(e, 0, fs)];
        let t = &ports[&group_key(e, 1, ts)];
        // facing neighbours closer than two stubs: a short Z through the gap between them
        let gap_h = match (s.side, t.side) {
            (Side::E, Side::W) => t.at[0] - s.at[0],
            (Side::W, Side::E) => s.at[0] - t.at[0],
            _ => -1.0,
        };
        let gap_v = match (s.side, t.side) {
            (Side::S, Side::N) => t.at[1] - s.at[1],
            (Side::N, Side::S) => s.at[1] - t.at[1],
            _ => -1.0,
        };
        if (gap_h >= 0.0 && gap_h < 2.0 * stub_len) || (gap_v >= 0.0 && gap_v < 2.0 * stub_len) {
            let pts = if gap_h >= 0.0 {
                let mx = (s.at[0] + t.at[0]) / 2.0;
                simplify(&[s.at, [mx, s.at[1]], [mx, t.at[1]], t.at])
            } else {
                let my = (s.at[1] + t.at[1]) / 2.0;
                simplify(&[s.at, [s.at[0], my], [t.at[0], my], t.at])
            };
            router.occupy(&pts, [s.key, t.key]);
            out.insert(e.id.clone(), Route { points: pts, hops: vec![], from_side: fs, to_side: ts, fallback: false, raw: vec![] });
            continue;
        }
        let mut mid: Option<Vec<Pt>> = None;
        if now() - t0 < budget {
            mid = search
                .run(&router, e, s, t, 160.0)
                .or_else(|| search.run(&router, e, s, t, 600.0))
                .or_else(|| search.run(&router, e, s, t, 1e7))
                .map(|r| r.0);
        }
        let fallback = mid.is_none();
        let mut all = vec![s.at];
        all.extend(mid.unwrap_or_else(|| fallback_route(s, t)));
        all.push(t.at);
        let pts = unjog(&simplify(&all), &blocked, 14.0);
        router.occupy(&pts, [s.key, t.key]);
        out.insert(e.id.clone(), Route { points: pts, hops: vec![], from_side: fs, to_side: ts, fallback, raw: vec![] });
    }

    for r in out.values_mut() {
        r.raw = r.points.clone();
    }
    let group_of = |e: &REdge, end: usize, out: &HashMap<String, Route>| {
        let r = &out[&e.id];
        group_key(e, end, if end == 1 { r.to_side } else { r.from_side })
    };
    let groups_of: HashMap<String, [String; 2]> = live.iter().filter(|e| out.contains_key(&e.id) && e.from != e.to).map(|e| (e.id.clone(), [group_of(e, 0, &out), group_of(e, 1, &out)])).collect();
    nudge(&mut out, &live, &groups_of, lane);
    // Use full lane spacing where clear; reduce shifts only when obstacles require it.
    // Endpoints remain fixed, and the raw validated route is always available.
    for (id, r) in &mut out {
        let edge = live.iter().find(|e| &e.id == id).unwrap();
        let shifted = r.points.clone();
        for factor in [1.0, 0.75, 0.5, 0.25, 0.0] {
            let candidate: Vec<Pt> = r.raw.iter().zip(&shifted).map(|(a, b)| [a[0] + (b[0]-a[0])*factor, a[1] + (b[1]-a[1])*factor]).collect();
            let clear = candidate.windows(2).enumerate().all(|(i, w)| {
                let first = i == 0;
                let last = i + 2 == candidate.len();
                if !first && !last { return !blocked(w[0], w[1]); }
                let side = if first { r.from_side } else { r.to_side }.dir();
                let delta = if first { [w[1][0]-w[0][0], w[1][1]-w[0][1]] } else { [w[0][0]-w[1][0], w[0][1]-w[1][1]] };
                if delta[0]*side[0] + delta[1]*side[1] < 0.0 { return false; }
                boxes.iter().filter(|b| !(first && b.id == edge.from || last && b.id == edge.to)).all(|b| {
                    !(w[0][0].max(w[1][0]) > b.x-b.w/2.0 && w[0][0].min(w[1][0]) < b.x+b.w/2.0
                        && w[0][1].max(w[1][1]) > b.y-b.h/2.0-b.pad_top && w[0][1].min(w[1][1]) < b.y+b.h/2.0+b.pad_bottom)
                })
            });
            if clear || factor == 0.0 {
                r.points = unjog(&simplify(&candidate), &blocked, 4.0);
                break;
            }
        }
    }
    add_hops(&mut out, &live);
    out
}

/// Remove small jogs (two turns around a short step) by sliding a neighbouring segment, when the result stays clear.
pub fn unjog(pts: &[Pt], blocked: &dyn Fn(Pt, Pt) -> bool, max: f64) -> Vec<Pt> {
    let mut p: Vec<Pt> = pts.to_vec();
    for _ in 0..20 {
        let mut changed = false;
        let mut i = 1;
        while i + 2 < p.len() {
            let (a, b) = (p[i], p[i + 1]);
            let len = (a[0] - b[0]).abs() + (a[1] - b[1]).abs();
            if len == 0.0 || len > max {
                i += 1;
                continue;
            }
            let k = if (a[0] - b[0]).abs() > 0.01 { 0 } else { 1 }; // axis along which the jog steps
            // option 1: slide the following segment back onto the preceding one's line (not the last, port-bound segment)
            if i + 3 < p.len() {
                let mut c = p[i + 2];
                c[k] = a[k];
                if !blocked(a, c) && !blocked(c, p[i + 3]) {
                    let mut q = p.clone();
                    q[i + 1] = a;
                    q[i + 2] = c;
                    p = simplify(&q);
                    changed = true;
                    break;
                }
            }
            // option 2: slide the preceding segment onto the following one's line (not the first, port-bound segment)
            if i >= 2 {
                let mut c = p[i - 1];
                c[k] = b[k];
                if !blocked(p[i - 2], c) && !blocked(c, b) {
                    let mut q = p.clone();
                    q[i - 1] = c;
                    q[i] = b;
                    p = simplify(&q);
                    changed = true;
                    break;
                }
            }
            i += 1;
        }
        if !changed {
            break;
        }
    }
    p
}

/// Merge collinear points, drop zero-length segments.
pub fn simplify(pts: &[Pt]) -> Vec<Pt> {
    let mut a: Vec<Pt> = vec![];
    for p in pts {
        if let Some(q) = a.last() {
            if (q[0] - p[0]).abs() < 0.01 && (q[1] - p[1]).abs() < 0.01 {
                continue;
            }
        }
        a.push(*p);
    }
    let mut i = 1;
    while i + 1 < a.len() {
        let (p, q, r) = (a[i - 1], a[i], a[i + 1]);
        let col = ((p[0] - q[0]).abs() < 0.01 && (q[0] - r[0]).abs() < 0.01) || ((p[1] - q[1]).abs() < 0.01 && (q[1] - r[1]).abs() < 0.01);
        if col {
            a.remove(i);
        } else {
            i += 1;
        }
    }
    a
}

/// Spread collinear overlapping segments of different bundles into lanes (end segments stay on their ports).
fn nudge(routes: &mut HashMap<String, Route>, edges: &[&REdge], groups_of: &HashMap<String, [String; 2]>, lane: f64) {
    #[derive(Clone)]
    struct Seg {
        edge: String,
        i: usize,
        horiz: bool,
        c: f64,
        a: f64,
        b: f64,
        bundle: String,
        order: f64,
    }
    let mut segs: Vec<Seg> = vec![];
    for e in edges {
        let Some(r) = routes.get(&e.id) else { continue };
        if e.from == e.to {
            continue;
        }
        let p = &r.points;
        let ends = [p[0], p[p.len() - 1]];
        let mut i = 1;
        while i + 2 < p.len() {
            let (u, v) = (p[i], p[i + 1]);
            let horiz = (u[1] - v[1]).abs() < 0.01;
            // order lanes by where the edge goes next, so nudging doesn't create crossings
            let order = if horiz { (ends[0][1] + ends[1][1]) / 2.0 } else { (ends[0][0] + ends[1][0]) / 2.0 };
            let (ua, va) = if horiz { (u[0], v[0]) } else { (u[1], v[1]) };
            segs.push(Seg { edge: e.id.clone(), i, horiz, c: if horiz { u[1] } else { u[0] }, a: ua.min(va), b: ua.max(va), bundle: e.id.clone(), order });
            i += 1;
        }
    }
    // channels: segments on (nearly) the same line, within 6px, are one channel to split into lanes
    let mut by_line: Vec<Vec<Seg>> = vec![];
    for horiz in [true, false] {
        let mut list: Vec<Seg> = segs.iter().filter(|x| x.horiz == horiz).cloned().collect();
        list.sort_by(|x, y| x.c.total_cmp(&y.c));
        let mut last = f64::NEG_INFINITY;
        for sg in list {
            if sg.c - last > 6.0 {
                by_line.push(vec![]);
            }
            last = sg.c;
            by_line.last_mut().unwrap().push(sg);
        }
    }
    let empty: [String; 2] = [String::new(), String::new()];
    let mut shifts: Vec<(String, usize, bool, f64)> = vec![];
    for mut list in by_line {
        list.sort_by(|x, y| x.a.total_cmp(&y.a));
        // clusters of overlapping intervals
        let mut cluster: Vec<Seg> = vec![];
        let mut end = f64::NEG_INFINITY;
        let mut flush = |cluster: &mut Vec<Seg>| {
            // within this stretch, edges that share a port travel together (one lane); others get their own lane
            let mut lead: Vec<(String, String)> = vec![];
            let lead_of = |lead: &Vec<(String, String)>, e: &str| lead.iter().find(|(k, _)| k == e).map(|(_, v)| v.clone());
            for k in 0..cluster.len() {
                let sg = &cluster[k];
                if lead_of(&lead, &sg.edge).is_some() {
                    continue;
                }
                let mine = groups_of.get(&sg.edge).unwrap_or(&empty);
                let mate = cluster.iter().find(|o2| {
                    lead_of(&lead, &o2.edge).is_some() && groups_of.get(&o2.edge).unwrap_or(&empty).iter().any(|g| !g.is_empty() && mine.contains(g))
                });
                let v = match mate {
                    Some(m) => lead_of(&lead, &m.edge).unwrap(),
                    None => sg.edge.clone(),
                };
                lead.push((sg.edge.clone(), v));
            }
            for sg in cluster.iter_mut() {
                sg.bundle = lead_of(&lead, &sg.edge).unwrap();
            }
            let mut bundles: Vec<String> = vec![];
            for s in cluster.iter() {
                if !bundles.contains(&s.bundle) {
                    bundles.push(s.bundle.clone());
                }
            }
            if bundles.len() > 1 {
                let ord: HashMap<String, f64> = bundles
                    .iter()
                    .map(|b| {
                        let m: Vec<&Seg> = cluster.iter().filter(|s| &s.bundle == b).collect();
                        (b.clone(), m.iter().map(|s| s.order).sum::<f64>() / m.len() as f64)
                    })
                    .collect();
                bundles.sort_by(|x, y| ord[x].total_cmp(&ord[y]).then_with(|| if x < y { std::cmp::Ordering::Less } else { std::cmp::Ordering::Greater }));
                let n = bundles.len() as f64;
                let step = lane;
                let centre = cluster.iter().map(|s| s.c).sum::<f64>() / cluster.len() as f64;
                for (k, b) in bundles.iter().enumerate() {
                    let target = centre + (k as f64 - (n - 1.0) / 2.0) * step;
                    for s in cluster.iter() {
                        if &s.bundle == b {
                            shifts.push((s.edge.clone(), s.i, s.horiz, target - s.c));
                        }
                    }
                }
            }
            cluster.clear();
        };
        for s in list {
            if !cluster.is_empty() && s.a >= end - 0.5 {
                flush(&mut cluster);
                end = f64::NEG_INFINITY;
            }
            end = end.max(s.b);
            cluster.push(s);
        }
        flush(&mut cluster);
    }
    for (edge, i, horiz, d) in shifts {
        let p = &mut routes.get_mut(&edge).unwrap().points;
        let k = if horiz { 1 } else { 0 };
        p[i][k] += d;
        p[i + 1][k] += d;
    }
}

/// Hops: where a horizontal segment crosses another edge's vertical segment (away from corners).
fn add_hops(routes: &mut HashMap<String, Route>, edges: &[&REdge]) {
    let mut hs: Vec<(&str, f64, f64, f64)> = vec![];
    let mut vs: Vec<(&str, f64, f64, f64)> = vec![];
    for e in edges {
        let Some(r) = routes.get(&e.id) else { continue };
        for w in r.points.windows(2) {
            let (u, v) = (w[0], w[1]);
            if (u[1] - v[1]).abs() < 0.01 {
                hs.push((&e.id, u[1], u[0].min(v[0]), u[0].max(v[0])));
            } else {
                vs.push((&e.id, u[0], u[1].min(v[1]), u[1].max(v[1])));
            }
        }
    }
    let clear = 16.0;
    let mut add: Vec<(String, Pt)> = vec![];
    for h in &hs {
        for v in &vs {
            if v.0 == h.0 {
                continue;
            }
            if v.1 > h.2 + clear && v.1 < h.3 - clear && h.1 > v.2 + 2.0 && h.1 < v.3 - 2.0 {
                add.push((h.0.to_string(), [v.1, h.1]));
            }
        }
    }
    for (e, p) in add {
        routes.get_mut(&e).unwrap().hops.push(p);
    }
    for r in routes.values_mut() {
        r.hops.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
        let mut kept: Vec<Pt> = vec![];
        for (i, q) in r.hops.iter().enumerate() {
            if i == 0 || (q[0] - r.hops[i - 1][0]).abs() > 11.0 || q[1] != r.hops[i - 1][1] {
                kept.push(*q);
            }
        }
        r.hops = kept;
    }
}

/// One drawing command of an edge path.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PathCmd {
    Move(Pt),
    Line(Pt),
    /// quadratic curve: control, end
    Quad(Pt, Pt),
    /// semicircular hop of radius r to `end`; `sweep` as in SVG
    Hop { r: f64, end: Pt, sweep: bool },
}

/// Path: rounded corners (radius r) and semicircular hops (radius hr) on horizontal segments.
pub fn route_cmds(points: &[Pt], hops: &[Pt], r: f64, hr: f64) -> Vec<PathCmd> {
    let mut d = vec![];
    if points.len() < 2 {
        return d;
    }
    let len = |a: Pt, b: Pt| (a[0] - b[0]).abs() + (a[1] - b[1]).abs();
    let unit = |a: Pt, b: Pt| {
        let l = len(a, b);
        let l = if l == 0.0 { 1.0 } else { l };
        [(b[0] - a[0]) / l, (b[1] - a[1]) / l]
    };
    let n = points.len();
    let rad: Vec<f64> = (0..n).map(|i| if i == 0 || i == n - 1 { 0.0 } else { r.min(len(points[i - 1], points[i]) / 2.0).min(len(points[i], points[i + 1]) / 2.0) }).collect();
    d.push(PathCmd::Move(points[0]));
    for i in 0..n - 1 {
        let (a, b) = (points[i], points[i + 1]);
        let u = unit(a, b);
        let start = [a[0] + u[0] * rad[i], a[1] + u[1] * rad[i]];
        let end = [b[0] - u[0] * rad[i + 1], b[1] - u[1] * rad[i + 1]];
        if (a[1] - b[1]).abs() < 0.01 && !hops.is_empty() {
            let dir = (b[0] - a[0]).signum();
            let mut on: Vec<Pt> = hops.iter().copied().filter(|h| (h[1] - a[1]).abs() < 0.5 && (h[0] - start[0]) * dir > hr && (end[0] - h[0]) * dir > hr).collect();
            on.sort_by(|p, q| ((p[0] - q[0]) * dir).total_cmp(&0.0));
            for h in on {
                d.push(PathCmd::Line([h[0] - dir * hr, a[1]]));
                d.push(PathCmd::Hop { r: hr, end: [h[0] + dir * hr, a[1]], sweep: dir > 0.0 });
            }
        }
        d.push(PathCmd::Line(end));
        if i + 2 < n && rad[i + 1] > 0.0 {
            let c = points[i + 1];
            let u2 = unit(c, points[i + 2]);
            d.push(PathCmd::Quad(c, [c[0] + u2[0] * rad[i + 1], c[1] + u2[1] * rad[i + 1]]));
        }
    }
    d
}

/// SVG path data for `route_cmds`.
pub fn route_path(points: &[Pt], hops: &[Pt]) -> String {
    cmds_to_svg(&route_cmds(points, hops, 10.0, 5.0))
}

pub fn cmds_to_svg(cmds: &[PathCmd]) -> String {
    let f = |n: f64| format!("{n:.1}");
    let mut s = String::new();
    for c in cmds {
        match c {
            PathCmd::Move(p) => s += &format!("M{},{}", f(p[0]), f(p[1])),
            PathCmd::Line(p) => s += &format!(" L{},{}", f(p[0]), f(p[1])),
            PathCmd::Quad(c, p) => s += &format!(" Q{},{} {},{}", f(c[0]), f(c[1]), f(p[0]), f(p[1])),
            PathCmd::Hop { r, end, sweep } => s += &format!(" A{r},{r} 0 0 {} {},{}", if *sweep { 1 } else { 0 }, f(end[0]), f(end[1])),
        }
    }
    s
}

/// Flatten path commands to a polyline (for painting).
pub fn flatten(cmds: &[PathCmd]) -> Vec<Pt> {
    let mut out: Vec<Pt> = vec![];
    let mut cur = [0.0, 0.0];
    for c in cmds {
        match *c {
            PathCmd::Move(p) | PathCmd::Line(p) => {
                out.push(p);
                cur = p;
            }
            PathCmd::Quad(ctrl, end) => {
                for k in 1..=6 {
                    let t = k as f64 / 6.0;
                    let mt = 1.0 - t;
                    out.push([mt * mt * cur[0] + 2.0 * mt * t * ctrl[0] + t * t * end[0], mt * mt * cur[1] + 2.0 * mt * t * ctrl[1] + t * t * end[1]]);
                }
                cur = end;
            }
            PathCmd::Hop { r, end, sweep } => {
                // semicircle above the line (screen y down): centre midway
                let cx = (cur[0] + end[0]) / 2.0;
                let cy = cur[1];
                let start_ang = if end[0] > cur[0] { std::f64::consts::PI } else { 0.0 };
                // SVG sweep=1 is clockwise on screen; going left→right clockwise passes above (y−)
                let dir = if sweep { 1.0 } else { -1.0 };
                for k in 1..=8 {
                    let a = start_ang + dir * std::f64::consts::PI * k as f64 / 8.0;
                    out.push([cx + r * a.cos(), cy + r * a.sin()]);
                }
                cur = end;
            }
        }
    }
    out
}

/// Midpoint of the longest inner segment: where a label sits.
pub fn label_point(points: &[Pt]) -> Pt {
    let mut best = 0.0;
    let mut at = points[0];
    let inner = if points.len() > 3 { (1, points.len() - 2) } else { (0, points.len() - 1) };
    for i in inner.0..inner.1 {
        let l = (points[i][0] - points[i + 1][0]).abs() + (points[i][1] - points[i + 1][1]).abs();
        if l > best {
            best = l;
            at = [(points[i][0] + points[i + 1][0]) / 2.0, (points[i][1] + points[i + 1][1]) / 2.0];
        }
    }
    at
}

/// Orthogonal routes for a diagram, recomputed only when needed (port of `useRoutes`):
/// structure changed (edges, box sizes) or `epoch` bumped → full routing; only positions changed →
/// re-route edges touching moved boxes, keep the rest; nothing changed → the previous routes.
#[derive(Default)]
pub struct RouteCache {
    structure: String,
    epoch: u64,
    pos: HashMap<String, String>,
    pub routes: HashMap<String, Route>,
    valid: bool,
}

impl RouteCache {
    pub fn update(&mut self, boxes: &[RBox], edges: &[REdge], zones: &[RZone], epoch: u64) -> &HashMap<String, Route> {
        let mut st = String::new();
        for e in edges {
            st += &format!("{}:{}>{}:{:?}{:?}{},", e.id, e.from, e.to, e.from_deco, e.to_deco, if e.vertical { "v" } else { "" });
        }
        st.push('|');
        for b in boxes {
            st += &format!("{}:{:.1}x{:.1}:{}:{},", b.id, b.w, b.h, b.pad_top, b.pad_bottom);
        }
        let pos: HashMap<String, String> = boxes.iter().map(|b| (b.id.clone(), format!("{:.1},{:.1}", b.x, b.y))).collect();
        if self.valid && self.structure == st && self.epoch == epoch {
            let moved: HashSet<&String> = pos.iter().filter(|(id, k)| self.pos.get(*id) != Some(*k)).map(|(id, _)| id).collect();
            if moved.is_empty() {
                return &self.routes;
            }
            let only: HashSet<String> = edges.iter().filter(|e| moved.contains(&e.from) || moved.contains(&e.to)).map(|e| e.id.clone()).collect();
            let routes = route_all(boxes, edges, zones, RouteOptions { only: Some(&only), previous: Some(&self.routes), budget_ms: 10.0, ..Default::default() });
            self.routes = routes;
            self.pos = pos;
            return &self.routes;
        }
        // a full route is final (exports, settled canvas): give it room to find validated paths
        self.routes = route_all(boxes, edges, zones, RouteOptions { budget_ms: 400.0, ..Default::default() });
        self.structure = st;
        self.epoch = epoch;
        self.pos = pos;
        self.valid = true;
        &self.routes
    }
    pub fn invalidate(&mut self) {
        self.valid = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bx(id: &str, x: f64, y: f64) -> RBox {
        RBox::new(id, x, y, 120.0, 60.0)
    }
    fn inside(b: &RBox, p: Pt) -> bool {
        p[0] > b.x - b.w / 2.0 + 0.01 && p[0] < b.x + b.w / 2.0 - 0.01 && p[1] > b.y - b.h / 2.0 + 0.01 && p[1] < b.y + b.h / 2.0 - 0.01
    }
    fn check_route(r: &Route, boxes: &[RBox], e: &REdge) {
        let p = &r.points;
        assert!(p.len() >= 2);
        for i in 0..p.len() - 1 {
            let (a, b) = (p[i], p[i + 1]);
            assert!((a[0] - b[0]).abs() < 0.01 || (a[1] - b[1]).abs() < 0.01, "{} segment {i} not axis-aligned", e.id);
            let mut t = 0.02;
            while t < 1.0 {
                let q = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
                for b in boxes {
                    assert!(!inside(b, q), "{} passes through {}", e.id, b.id);
                }
                t += 0.02;
            }
        }
    }
    fn on_side(b: &RBox, p: Pt, side: Side) -> bool {
        match side {
            Side::N => (p[1] - (b.y - b.h / 2.0)).abs() < 0.01,
            Side::S => (p[1] - (b.y + b.h / 2.0)).abs() < 0.01,
            Side::E => (p[0] - (b.x + b.w / 2.0)).abs() < 0.01,
            Side::W => (p[0] - (b.x - b.w / 2.0)).abs() < 0.01,
        }
    }
    fn arrow(id: &str, f: &str, t: &str) -> REdge {
        REdge::new(id, f, t).deco(Deco::None, Deco::Arrow, false)
    }

    #[test]
    fn routes_avoid_boxes_orthogonal_and_on_sides() {
        let boxes = vec![bx("A", 0.0, 0.0), bx("B", 0.0, 300.0), RBox::new("X", 0.0, 150.0, 200.0, 60.0), bx("C", 300.0, 150.0)];
        let edges = vec![arrow("ab", "A", "B"), REdge::new("ac", "A", "C"), REdge::new("cb", "C", "B")];
        let routes = route_all(&boxes, &edges, &[], RouteOptions::default());
        for e in &edges {
            let r = &routes[&e.id];
            check_route(r, &boxes, e);
            let fb = boxes.iter().find(|b| b.id == e.from).unwrap();
            let tb = boxes.iter().find(|b| b.id == e.to).unwrap();
            assert!(on_side(fb, r.points[0], r.from_side), "{} start side", e.id);
            assert!(on_side(tb, *r.points.last().unwrap(), r.to_side), "{} end side", e.id);
            assert!(!r.fallback);
        }
    }

    #[test]
    fn same_deco_has_distinct_neighbor_ordered_ports() {
        let boxes = vec![bx("P", 0.0, 0.0), bx("C1", -200.0, 250.0), bx("C2", 0.0, 250.0), bx("C3", 200.0, 250.0)];
        let edges: Vec<REdge> = ["C1", "C2", "C3"].iter().map(|c| REdge::new(*c, *c, "P").deco(Deco::None, Deco::Triangle, true)).collect();
        let routes = route_all(&boxes, &edges, &[], RouteOptions::default());
        let ends: HashSet<String> = edges.iter().map(|e| format!("{:?}", routes[&e.id].points.last().unwrap())).collect();
        assert_eq!(ends.len(), 3, "unrelated inheritance arrows remain distinguishable");
        let xs: Vec<f64> = edges.iter().map(|e| routes[&e.id].points.last().unwrap()[0]).collect();
        assert!(xs.windows(2).all(|w| w[1] - w[0] >= 14.0));
        for e in &edges {
            check_route(&routes[&e.id], &boxes, e);
        }
    }

    #[test]
    fn open_channels_keep_full_lane_spacing() {
        let edges: Vec<_> = (0..5).map(|i| arrow(&format!("e{i}"), "A", "B")).collect();
        let mut routes = HashMap::new();
        for (i, e) in edges.iter().enumerate() {
            let x = i as f64 * 30.0;
            let points = vec![[x, 0.0], [x, 100.0], [300.0+x, 100.0], [300.0+x, 200.0]];
            routes.insert(e.id.clone(), Route { raw: points.clone(), points, hops: vec![], from_side: Side::S, to_side: Side::N, fallback: false });
        }
        nudge(&mut routes, &edges.iter().collect::<Vec<_>>(), &HashMap::new(), 12.0);
        let mut lanes: Vec<_> = routes.values().map(|r| r.points[1][1]).collect();
        lanes.sort_by(f64::total_cmp);
        assert!(lanes.windows(2).all(|w| w[1]-w[0] >= 12.0));
        for r in routes.values() {
            assert_eq!(r.points.first(), r.raw.first());
            assert_eq!(r.points.last(), r.raw.last());
        }
    }

    #[test]
    fn crowded_ports_never_collapse_at_corners() {
        let boxes = vec![bx("A", 0.0, 0.0), bx("B", 0.0, 300.0)];
        let edges: Vec<_> = (0..16).map(|i| arrow(&format!("e{i:02}"), "A", "B")).collect();
        let a = route_all(&boxes, &edges, &[], RouteOptions::default());
        let b = route_all(&boxes, &edges, &[], RouteOptions::default());
        assert_eq!(a, b);
        let starts: HashSet<_> = a.values().map(|r| format!("{:?}", r.points[0])).collect();
        assert_eq!(starts.len(), edges.len());
        for e in &edges { check_route(&a[&e.id], &boxes, e); }
    }

    #[test]
    fn different_decos_and_parallel_edges_separate() {
        let boxes = vec![bx("A", 0.0, 0.0), bx("B", 0.0, 300.0)];
        let edges = vec![arrow("x", "A", "B"), arrow("y", "A", "B"), REdge::new("z", "A", "B").deco(Deco::Diamond, Deco::None, false)];
        let routes = route_all(&boxes, &edges, &[], RouteOptions::default());
        let starts: HashSet<String> = routes.values().map(|r| format!("{:?}", r.points[0])).collect();
        assert_eq!(starts.len(), 3);
    }

    #[test]
    fn crossings_become_hops_deterministically() {
        let boxes = vec![bx("L", -300.0, 0.0), bx("R", 300.0, 0.0), bx("T", 0.0, -300.0), bx("Bt", 0.0, 300.0)];
        let edges = vec![REdge::new("h", "L", "R"), REdge::new("v", "T", "Bt")];
        let a = route_all(&boxes, &edges, &[], RouteOptions::default());
        let b = route_all(&boxes, &edges, &[], RouteOptions::default());
        assert_eq!(a, b);
        assert_eq!(a["h"].hops.len(), 1);
        assert_eq!(a["v"].hops.len(), 0);
        assert!(route_path(&a["h"].points, &a["h"].hops).contains(" A5,5 "));
    }

    #[test]
    fn budget_exhausted_fallback_still_orthogonal() {
        let mut t = 0.0;
        let boxes = vec![bx("A", 0.0, 0.0), bx("B", 400.0, 300.0)];
        let r = route_all(
            &boxes,
            &[REdge::new("e", "A", "B")],
            &[],
            RouteOptions {
                budget_ms: 5.0,
                now: Some(Box::new(move || {
                    t += 10.0;
                    t
                })),
                ..Default::default()
            },
        );
        let r = &r["e"];
        assert!(r.fallback);
        for w in r.points.windows(2) {
            assert!(w[0][0] == w[1][0] || w[0][1] == w[1][1]);
        }
    }

    #[test]
    fn incremental_reroute_keeps_untouched_edges() {
        let edges = vec![REdge::new("ab", "A", "B"), REdge::new("cb", "C", "B")];
        let first = route_all(&[bx("A", 0.0, 0.0), bx("B", 0.0, 300.0), bx("C", 300.0, 0.0)], &edges, &[], RouteOptions::default());
        let only: HashSet<String> = ["cb".to_string()].into();
        let mut prev = first.clone();
        for k in 0..5 {
            prev = route_all(&[bx("A", 0.0, 0.0), bx("B", 0.0, 300.0), bx("C", 300.0 + k as f64, 0.0)], &edges, &[], RouteOptions { only: Some(&only), previous: Some(&prev), ..Default::default() });
        }
        assert_eq!(prev["ab"].points, first["ab"].points);
    }

    #[test]
    fn rounded_corners_and_simplification() {
        assert_eq!(simplify(&[[0.0, 0.0], [0.0, 0.0], [0.0, 10.0], [0.0, 20.0], [10.0, 20.0]]), vec![[0.0, 0.0], [0.0, 20.0], [10.0, 20.0]]);
        assert!(route_path(&[[0.0, 0.0], [0.0, 50.0], [50.0, 50.0]], &[]).contains("Q0.0,50.0 10.0,50.0"));
    }

    #[test]
    fn diagonal_takes_an_l() {
        let boxes = vec![bx("A", 0.0, 0.0), bx("B", 500.0, 400.0)];
        let r = route_all(&boxes, &[arrow("e", "A", "B")], &[], RouteOptions::default());
        assert_eq!(r["e"].points.len() - 2, 1, "{:?}", r["e"].points);
    }

    #[test]
    fn penalties_reduce_crossings_and_bends() {
        let mut seed: i64 = 7;
        let mut rnd = || {
            seed = (seed * 16807) % 2147483647;
            seed as f64 / 2147483647.0
        };
        let mut boxes = vec![];
        for i in 0..6 {
            for j in 0..5 {
                let (x, y, w, h) = (i as f64 * 260.0 + rnd() * 40.0, j as f64 * 200.0 + rnd() * 30.0, 140.0 + rnd() * 60.0, 60.0 + rnd() * 80.0);
                boxes.push(RBox::new(format!("c{i}{j}"), x, y, w, h));
            }
        }
        let mut edges = vec![];
        for k in 0..45 {
            let a = boxes[(rnd() * 30.0) as usize].id.clone();
            let b = boxes[(rnd() * 30.0) as usize].id.clone();
            if a != b {
                edges.push(arrow(&format!("e{k}"), &a, &b));
            }
        }
        let measure = |routes: &HashMap<String, Route>| {
            let (mut bends, mut crossings) = (0, 0);
            let (mut hh, mut vv) = (vec![], vec![]);
            for (i, e) in edges.iter().enumerate() {
                let r = &routes[&e.id];
                bends += r.points.len() - 2;
                for w in r.points.windows(2) {
                    let (p, q) = (w[0], w[1]);
                    if (p[1] - q[1]).abs() < 0.01 {
                        hh.push((i, p[1], p[0].min(q[0]), p[0].max(q[0])));
                    } else {
                        vv.push((i, p[0], p[1].min(q[1]), p[1].max(q[1])));
                    }
                }
            }
            for h in &hh {
                for v in &vv {
                    if h.0 != v.0 && v.1 > h.2 + 1.0 && v.1 < h.3 - 1.0 && h.1 > v.2 + 1.0 && h.1 < v.3 - 1.0 {
                        crossings += 1;
                    }
                }
            }
            (bends, crossings)
        };
        let loose = measure(&route_all(&boxes, &edges, &[], RouteOptions { budget_ms: 1e9, bend: 1.0, cross: 0.0, overlap: 0.0, ..Default::default() }));
        let tuned = measure(&route_all(&boxes, &edges, &[], RouteOptions { budget_ms: 1e9, ..Default::default() }));
        assert!(tuned.1 < loose.1, "{loose:?} {tuned:?}");
        assert!(tuned.0 < loose.0, "{loose:?} {tuned:?}");
    }

    #[test]
    fn small_jogs_are_straightened() {
        let pts = [[0.0, 0.0], [0.0, 100.0], [3.0, 100.0], [3.0, 200.0], [80.0, 200.0], [80.0, 260.0]];
        assert_eq!(unjog(&pts, &|_, _| false, 14.0), vec![[0.0, 0.0], [0.0, 200.0], [80.0, 200.0], [80.0, 260.0]]);
        assert_eq!(unjog(&pts, &|_, _| true, 14.0).len(), 6);
    }
}
