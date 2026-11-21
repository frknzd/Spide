//! Layout physics: a port of the d3-force subset Spide used, with its custom forces. It only runs
//! after a model change, a drag or a tidy, and stops completely once it has settled (no perpetual
//! motion: idle = no work).

use spide_core::model::Pos;
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Debug, Clone)]
pub struct SimNode {
    pub id: String,
    pub x: f64,
    pub y: f64,
    pub vx: f64,
    pub vy: f64,
    pub w: f64,
    pub h: f64,
    pub pkg: Option<String>,
    pub layer: Option<u8>,
    pub heat: f64,
    /// tidy target
    pub tx: Option<f64>,
    pub ty: Option<f64>,
    /// pinned by the user: fx/fy stay set
    pub pinned: bool,
    pub fx: Option<f64>,
    pub fy: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct NodeSpec {
    pub id: String,
    pub w: f64,
    pub h: f64,
    pub pkg: Option<String>,
    pub layer: Option<u8>,
    pub heat: f64,
    pub init: Option<Pos>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LinkSpec {
    pub from: String,
    pub to: String,
    pub kind: String,
}

const LAYER_GAP: f64 = 300.0;
const ALPHA_MIN: f64 = 0.005;
const ALPHA_DECAY: f64 = 0.035;
const VELOCITY_DECAY: f64 = 1.0 - 0.42;
/// d3 ticks once per animation frame
const TICK: f64 = 1.0 / 60.0;

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Step {
    /// positions changed this frame
    pub moved: bool,
    /// the layout came to rest (or went idle): persist positions, re-route fully
    pub settled: bool,
}

pub struct LivingLayout {
    pub nodes: Vec<SimNode>,
    index: HashMap<String, usize>,
    links: Vec<LinkSpec>,
    alpha: f64,
    alpha_target: f64,
    running: bool,
    tidy_until: Option<f64>,
    settle_due: Option<f64>,
    clock: f64,
    last: Option<f64>,
    seed: u64,
}

impl Default for LivingLayout {
    fn default() -> Self {
        Self::new()
    }
}

impl LivingLayout {
    pub fn new() -> Self {
        Self { nodes: vec![], index: HashMap::new(), links: vec![], alpha: 1.0, alpha_target: 0.0, running: false, tidy_until: None, settle_due: None, clock: 0.0, last: None, seed: 0x5eed }
    }

    fn random(&mut self) -> f64 {
        // xorshift64*: deterministic stand-in for Math.random
        let mut x = self.seed;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.seed = x;
        (x.wrapping_mul(0x2545F4914F6CDD1D) >> 11) as f64 / (1u64 << 53) as f64
    }

    pub fn get(&self, id: &str) -> Option<&SimNode> {
        self.index.get(id).map(|&i| &self.nodes[i])
    }
    fn get_mut(&mut self, id: &str) -> Option<&mut SimNode> {
        self.index.get(id).copied().map(move |i| &mut self.nodes[i])
    }
    pub fn has(&self, id: &str) -> bool {
        self.index.contains_key(id)
    }
    /// Still moving (or a settle notification is pending): the UI keeps repainting.
    pub fn active(&self) -> bool {
        self.running || self.settle_due.is_some()
    }

    fn restart(&mut self) {
        self.running = true;
    }

    pub fn update(&mut self, specs: &[NodeSpec], links: &[LinkSpec]) {
        let before: HashSet<String> = self.index.keys().cloned().collect();
        let mut next: Vec<SimNode> = vec![];
        let mut next_index: HashMap<String, usize> = HashMap::new();
        let mut added = 0usize;
        let mut changed = false;
        // neighbour lookup for placing new nodes
        let mut nb: HashMap<&str, Vec<&str>> = HashMap::new();
        for l in links {
            nb.entry(&l.from).or_default().push(&l.to);
            nb.entry(&l.to).or_default().push(&l.from);
        }
        for s in specs {
            let n = if let Some(old) = self.get(&s.id) {
                let mut n = old.clone();
                if n.w != s.w || n.h != s.h || n.pkg != s.pkg || n.layer != s.layer {
                    changed = true;
                }
                n.w = s.w;
                n.h = s.h;
                n.pkg = s.pkg.clone();
                n.layer = s.layer;
                n.heat = s.heat;
                n
            } else {
                added += 1;
                let (x, y) = if let Some(p) = s.init {
                    (p.x, p.y)
                } else {
                    let ns: Vec<SimNode> = nb.get(s.id.as_str()).map(|v| v.iter().filter_map(|id| self.get(id).cloned().or_else(|| next_index.get(*id).map(|&i| next[i].clone()))).collect()).unwrap_or_default();
                    if !ns.is_empty() {
                        let len = ns.len() as f64;
                        let x = ns.iter().map(|n| n.x).sum::<f64>() / len + (self.random() - 0.5) * 260.0;
                        let mut y = ns.iter().map(|n| n.y).sum::<f64>() / len + (self.random() - 0.5) * 200.0 + 120.0;
                        if let Some(layer) = s.layer {
                            if let Some(n0) = ns.iter().find(|n| n.layer.is_some()) {
                                y = n0.y + (layer as f64 - n0.layer.unwrap() as f64) * LAYER_GAP * 0.8;
                            }
                        }
                        (x, y)
                    } else {
                        let i = (next.len() + added) as f64;
                        let ang = i * 2.4;
                        let r = 140.0 + 60.0 * i.sqrt();
                        (ang.cos() * r, ang.sin() * r * 0.6)
                    }
                };
                let pinned = s.init.is_some_and(|p| p.pinned);
                SimNode { id: s.id.clone(), x, y, vx: 0.0, vy: 0.0, w: s.w, h: s.h, pkg: s.pkg.clone(), layer: s.layer, heat: s.heat, tx: None, ty: None, pinned, fx: pinned.then_some(x), fy: pinned.then_some(y) }
            };
            next_index.insert(s.id.clone(), next.len());
            next.push(n);
        }
        let removed = before.iter().filter(|id| !next_index.contains_key(*id)).count();
        self.nodes = next;
        self.index = next_index;
        let old: HashSet<LinkSpec> = self.links.iter().cloned().collect();
        let new_links: Vec<LinkSpec> = links.iter().filter(|l| self.index.contains_key(&l.from) && self.index.contains_key(&l.to) && l.from != l.to).cloned().collect();
        let links_changed = new_links.len() != old.len() || new_links.iter().any(|l| !old.contains(l));
        self.links = new_links;
        let kick = if added > 0 || removed > 0 {
            0.6
        } else if links_changed {
            0.35
        } else if changed {
            0.15
        } else {
            0.0
        };
        // Loading a complete saved drawing is not a request to rearrange it.
        let restoring = before.is_empty() && !specs.is_empty() && specs.iter().all(|s| s.init.is_some());
        if kick > 0.0 && !restoring {
            self.alpha = self.alpha.max(kick);
            self.restart();
        }
    }

    pub fn drag(&mut self, id: &str, x: f64, y: f64) {
        let Some(n) = self.get_mut(id) else { return };
        n.fx = Some(x);
        n.fy = Some(y);
        self.alpha_target = self.alpha_target.max(0.15);
        self.restart();
    }

    pub fn release(&mut self, id: &str) {
        let Some(n) = self.get_mut(id) else { return };
        // a pinned node stays where it was dropped
        if !n.pinned {
            n.fx = None;
            n.fy = None;
        }
        self.alpha_target = 0.0;
    }

    /// Pin / unpin a node where it is. Returns the new state.
    pub fn toggle_pin(&mut self, id: &str) -> bool {
        let Some(n) = self.get_mut(id) else { return false };
        n.pinned = !n.pinned;
        n.fx = n.pinned.then_some(n.x);
        n.fy = n.pinned.then_some(n.y);
        let p = n.pinned;
        self.alpha = self.alpha.max(0.1);
        self.restart();
        p
    }

    pub fn pinned_ids(&self) -> HashSet<String> {
        self.nodes.iter().filter(|n| n.pinned).map(|n| n.id.clone()).collect()
    }

    /// Animate towards target positions (e.g. the layered layout), then let physics take over. Pinned nodes stay.
    pub fn tidy(&mut self, pos: &HashMap<String, [f64; 2]>) {
        for (id, p) in pos {
            if let Some(n) = self.get_mut(id) {
                if !n.pinned {
                    n.tx = Some(p[0]);
                    n.ty = Some(p[1]);
                }
            }
        }
        self.tidy_until = Some(self.clock + 1.4);
        self.alpha = 0.8;
        self.restart();
    }

    pub fn positions(&self) -> BTreeMap<String, Pos> {
        self.nodes.iter().map(|n| (n.id.clone(), Pos { x: n.x.round(), y: n.y.round(), pinned: n.pinned })).collect()
    }

    /// Advance to wall-clock time `now` (seconds).
    pub fn step(&mut self, now: f64) -> Step {
        let dt = match self.last {
            Some(l) => (now - l).clamp(0.0, 0.25),
            None => TICK,
        };
        self.last = Some(now);
        self.clock = now;
        let mut out = Step::default();
        if self.running {
            let ticks = ((dt / TICK).round() as usize).clamp(1, 3);
            for _ in 0..ticks {
                self.tick();
                out.moved = true;
                self.settle_due = Some(now + 1.2);
                if self.alpha < ALPHA_MIN {
                    self.running = false;
                    out.settled = true;
                    break;
                }
            }
        } else if self.settle_due.is_some_and(|t| now >= t) {
            self.settle_due = None;
            out.settled = true;
        }
        out
    }

    /// Run until settled without a clock (initial placement, tests).
    pub fn settle(&mut self) {
        let mut t = self.clock;
        for _ in 0..2000 {
            t += TICK;
            self.step(t);
            if !self.running {
                break;
            }
        }
        self.tidy_until = None;
        self.settle_due = None;
    }

    fn tick(&mut self) {
        self.alpha += (self.alpha_target - self.alpha) * ALPHA_DECAY;
        let alpha = self.alpha;
        self.rect_collide(36.0, alpha);
        self.package_separation(80.0, alpha);
        self.tidy_pull();
        for n in &mut self.nodes {
            match n.fx {
                None => {
                    n.vx *= VELOCITY_DECAY;
                    n.x += n.vx;
                }
                Some(fx) => {
                    n.x = fx;
                    n.vx = 0.0;
                }
            }
            match n.fy {
                None => {
                    n.vy *= VELOCITY_DECAY;
                    n.y += n.vy;
                }
                Some(fy) => {
                    n.y = fy;
                    n.vy = 0.0;
                }
            }
        }
        if self.tidy_until.is_some_and(|t| self.clock > t) {
            self.tidy_until = None;
            for n in &mut self.nodes {
                // land exactly on the target (the layered layout guarantees no overlaps there)
                if let (Some(tx), Some(ty), false) = (n.tx, n.ty, n.pinned) {
                    n.x = tx;
                    n.y = ty;
                    n.vx = 0.0;
                    n.vy = 0.0;
                }
                n.tx = None;
                n.ty = None;
            }
            // the computed layout is final: no collision forces drift it afterwards
            self.alpha = 0.0;
            self.alpha_target = 0.0;
            self.running = false;
            self.settle_due = Some(self.clock);
        }
    }

    fn rect_collide(&mut self, margin: f64, alpha: f64) {
        let k = 0.6 * alpha.max(0.05);
        let n = self.nodes.len();
        for i in 0..n {
            for j in i + 1..n {
                let (a, b) = (&self.nodes[i], &self.nodes[j]);
                let dx = (b.x + b.vx) - (a.x + a.vx);
                let dy = (b.y + b.vy) - (a.y + a.vy);
                let ox = (a.w + b.w) / 2.0 + margin - dx.abs();
                let oy = (a.h + b.h) / 2.0 + margin - dy.abs();
                if ox > 0.0 && oy > 0.0 {
                    if ox / (a.w + b.w) < oy / (a.h + b.h) {
                        let s = if dx < 0.0 { -1.0 } else { 1.0 } * ox * k * 0.5;
                        self.nodes[i].vx -= s;
                        self.nodes[j].vx += s;
                    } else {
                        let s = if dy < 0.0 { -1.0 } else { 1.0 } * oy * k * 0.5;
                        self.nodes[i].vy -= s;
                        self.nodes[j].vy += s;
                    }
                }
            }
        }
    }

    /// Keep package hulls from overlapping: push whole packages apart.
    fn package_separation(&mut self, margin: f64, alpha: f64) {
        struct B {
            x1: f64,
            y1: f64,
            x2: f64,
            y2: f64,
            ns: Vec<usize>,
            layer: Option<u8>,
        }
        let mut order: Vec<String> = vec![];
        let mut boxes: HashMap<String, B> = HashMap::new();
        for (i, n) in self.nodes.iter().enumerate() {
            let Some(p) = &n.pkg else { continue };
            let b = boxes.entry(p.clone()).or_insert_with(|| {
                order.push(p.clone());
                B { x1: f64::INFINITY, y1: f64::INFINITY, x2: f64::NEG_INFINITY, y2: f64::NEG_INFINITY, ns: vec![], layer: n.layer }
            });
            b.x1 = b.x1.min(n.x - n.w / 2.0);
            b.y1 = b.y1.min(n.y - n.h / 2.0);
            b.x2 = b.x2.max(n.x + n.w / 2.0);
            b.y2 = b.y2.max(n.y + n.h / 2.0);
            b.ns.push(i);
        }
        let list: Vec<&B> = order.iter().map(|k| &boxes[k]).collect();
        let k = 0.25 * alpha.max(0.03);
        let mut dv: Vec<(usize, bool, f64)> = vec![];
        for i in 0..list.len() {
            for j in i + 1..list.len() {
                let (a, b) = (list[i], list[j]);
                let ox = a.x2.min(b.x2) - a.x1.max(b.x1) + margin;
                let oy = a.y2.min(b.y2) - a.y1.max(b.y1) + margin;
                if ox <= 0.0 || oy <= 0.0 {
                    continue;
                }
                let (acx, bcx, acy, bcy) = ((a.x1 + a.x2) / 2.0, (b.x1 + b.x2) / 2.0, (a.y1 + a.y2) / 2.0, (b.y1 + b.y2) / 2.0);
                // different layers separate vertically in layer order (outer layers on top)
                let layered = a.layer.is_some() && b.layer.is_some() && a.layer != b.layer;
                let horizontal = !layered && ox < oy;
                let s = if horizontal { ox } else { oy } * k;
                let dir = if layered {
                    if b.layer > a.layer {
                        1.0
                    } else {
                        -1.0
                    }
                } else if horizontal {
                    if bcx >= acx {
                        1.0
                    } else {
                        -1.0
                    }
                } else if bcy >= acy {
                    1.0
                } else {
                    -1.0
                };
                for &n in &a.ns {
                    dv.push((n, horizontal, -(dir * s) / a.ns.len() as f64 * 2.0));
                }
                for &n in &b.ns {
                    dv.push((n, horizontal, (dir * s) / b.ns.len() as f64 * 2.0));
                }
            }
        }
        for (n, h, d) in dv {
            if h {
                self.nodes[n].vx += d;
            } else {
                self.nodes[n].vy += d;
            }
        }
    }

    fn tidy_pull(&mut self) {
        for n in &mut self.nodes {
            if let (Some(tx), Some(ty)) = (n.tx, n.ty) {
                n.vx += (tx - n.x) * 0.18;
                n.vy += (ty - n.y) * 0.18;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(id: &str, x: f64, y: f64) -> NodeSpec {
        NodeSpec { id: id.into(), w: 120.0, h: 60.0, pkg: None, layer: None, heat: 0.0, init: Some(Pos { x, y, pinned: false }) }
    }

    #[test]
    fn overlapping_nodes_separate_and_settle() {
        let mut l = LivingLayout::new();
        l.update(&[spec("A", 0.0, 0.0), spec("B", 10.0, 5.0)], &[]);
        // A subsequent size change restarts collision resolution, unlike restoration.
        let mut resized = spec("A", 0.0, 0.0);
        resized.w = 130.0;
        l.update(&[resized, spec("B", 10.0, 5.0)], &[]);
        assert!(l.active());
        l.settle();
        assert!(!l.active());
        let (a, b) = (l.get("A").unwrap(), l.get("B").unwrap());
        assert!((a.x - b.x).abs() >= 120.0 || (a.y - b.y).abs() >= 60.0, "{a:?} {b:?}");
    }

    #[test]
    fn saved_positions_restore_without_a_simulation_tick() {
        let mut l = LivingLayout::new();
        l.update(&[spec("A", 17.0, 23.0), spec("B", 400.0, 60.0)], &[]);
        assert!(!l.active());
        for i in 0..120 { assert!(!l.step(i as f64 / 60.0).moved); }
        assert_eq!((l.get("A").unwrap().x, l.get("A").unwrap().y), (17.0, 23.0));
        assert_eq!((l.get("B").unwrap().x, l.get("B").unwrap().y), (400.0, 60.0));
    }

    #[test]
    fn pinned_nodes_stay() {
        let mut l = LivingLayout::new();
        let mut s = spec("A", 0.0, 0.0);
        s.init = Some(Pos { x: 0.0, y: 0.0, pinned: true });
        l.update(&[s, spec("B", 10.0, 5.0)], &[]);
        l.settle();
        let a = l.get("A").unwrap();
        assert_eq!((a.x, a.y), (0.0, 0.0));
        assert!(l.positions()["A"].pinned);
    }

    #[test]
    fn tidy_lands_on_targets() {
        let mut l = LivingLayout::new();
        l.update(&[spec("A", 0.0, 0.0), spec("B", 400.0, 0.0)], &[]);
        l.settle();
        let pos: HashMap<String, [f64; 2]> = [("A".to_string(), [-300.0, 50.0]), ("B".to_string(), [300.0, 50.0])].into();
        l.tidy(&pos);
        let mut t = 100.0;
        while l.active() {
            t += 1.0 / 60.0;
            l.step(t);
        }
        assert_eq!(l.get("A").map(|n| (n.x, n.y)), Some((-300.0, 50.0)));
    }
}
