//! A retained drawing of one diagram: primitives in world coordinates (what the old SVG DOM held),
//! hit regions for interaction, and tags that exports use to leave things out. The GUI paints it;
//! `svg::write` serialises the very same scene.

use crate::route::{PathCmd, Pt};
use crate::visuals::{FontSpec, Measure, Rgb};
use spide_core::Target;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Paint {
    pub color: Rgb,
    pub opacity: f32,
}

pub fn paint(color: Rgb, opacity: f32) -> Paint {
    Paint { color, opacity }
}
pub fn solid(color: Rgb) -> Paint {
    Paint { color, opacity: 1.0 }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stroke {
    pub color: Rgb,
    pub width: f32,
    pub opacity: f32,
    pub dash: Option<[f32; 2]>,
}

pub fn stroke(color: Rgb, width: f32) -> Stroke {
    Stroke { color, width, opacity: 1.0, dash: None }
}

impl Stroke {
    pub fn opacity(mut self, o: f32) -> Self {
        self.opacity = o;
        self
    }
    pub fn dash(mut self, a: f32, b: f32) -> Self {
        self.dash = Some([a, b]);
        self
    }
    pub fn dash_opt(mut self, d: Option<[f32; 2]>) -> Self {
        self.dash = d;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anchor {
    Start,
    Middle,
    End,
}

/// Decorative motion (CSS animations of the old UI). The painter animates; exports ignore it.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Anim {
    #[default]
    None,
    /// new element: scale .82 → 1 with overshoot, fade in (520 ms)
    Enter(String),
    /// new ghost: enter, then breathe 1 → .55 → 1 (2.4 s × 5)
    Ghost(String),
    /// opacity .35 → .8 → .35, `period` seconds × `times` (heat halo, rings, cycle glow)
    Pulse { key: String, period: f32, times: u32 },
    /// marching dashes 14/4 (cycle edges, .9 s × 7, offset −36)
    Strain(String),
    /// flowing dashes on ghost edges (1.2 s × 10, offset −24)
    Flow(String),
}

pub mod tag {
    /// diagnostics colouring (heat halos, issue chips, markers, cycle glow, rule warnings)
    pub const DIAG: u8 = 1;
    /// hover-only affordances (visibility tags, reveal chips)
    pub const TRANSIENT: u8 = 2;
    /// a proposed (ghost) element
    pub const PROPOSED: u8 = 4;
    /// package frames
    pub const PKG: u8 = 8;
}

#[derive(Debug, Clone, PartialEq)]
pub enum Prim {
    /// top-left rectangle with corner radius
    Rect { x: f64, y: f64, w: f64, h: f64, rx: f64, fill: Option<Paint>, stroke: Option<Stroke> },
    Circle { cx: f64, cy: f64, r: f64, fill: Option<Paint>, stroke: Option<Stroke> },
    /// a soft blurred highlight behind a box (SVG gaussian blur)
    Glow { x: f64, y: f64, w: f64, h: f64, rx: f64, color: Rgb, opacity: f32 },
    Ellipse { cx: f64, cy: f64, rx: f64, ry: f64, fill: Option<Paint>, stroke: Option<Stroke> },
    /// straight segments
    Poly { pts: Vec<Pt>, closed: bool, fill: Option<Paint>, stroke: Option<Stroke> },
    /// curves (rounded edge routes, hops)
    Path { cmds: Vec<PathCmd>, stroke: Stroke },
    /// `y` is the baseline
    Text { x: f64, y: f64, text: String, font: FontSpec, color: Rgb, opacity: f32, anchor: Anchor, underline: bool },
    Group(Group),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    pub tx: f64,
    pub ty: f64,
    pub scale: f64,
    pub opacity: f32,
    pub tags: u8,
    pub anim: Anim,
    pub children: Vec<Prim>,
}

impl Default for Group {
    fn default() -> Self {
        Self { tx: 0.0, ty: 0.0, scale: 1.0, opacity: 1.0, tags: 0, anim: Anim::None, children: vec![] }
    }
}

impl Group {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn at(tx: f64, ty: f64) -> Self {
        Self { tx, ty, ..Self::default() }
    }
    pub fn tags(mut self, t: u8) -> Self {
        self.tags |= t;
        self
    }
    pub fn opacity(mut self, o: f32) -> Self {
        self.opacity = o;
        self
    }
    pub fn anim(mut self, a: Anim) -> Self {
        self.anim = a;
        self
    }
    pub fn push(&mut self, p: Prim) {
        self.children.push(p);
    }
    pub fn add(&mut self, g: Group) {
        self.children.push(Prim::Group(g));
    }
    pub fn rect(&mut self, x: f64, y: f64, w: f64, h: f64, rx: f64, fill: Option<Paint>, stroke: Option<Stroke>) {
        self.children.push(Prim::Rect { x, y, w, h, rx, fill, stroke });
    }
    #[allow(clippy::too_many_arguments)]
    pub fn text(&mut self, x: f64, y: f64, text: impl Into<String>, font: FontSpec, color: Rgb, anchor: Anchor) {
        self.children.push(Prim::Text { x, y, text: text.into(), font, color, opacity: 1.0, anchor, underline: false });
    }
    pub fn line(&mut self, a: Pt, b: Pt, s: Stroke) {
        self.children.push(Prim::Poly { pts: vec![a, b], closed: false, fill: None, stroke: Some(s) });
    }
    pub fn poly(&mut self, pts: Vec<Pt>, closed: bool, fill: Option<Paint>, stroke: Option<Stroke>) {
        self.children.push(Prim::Poly { pts, closed, fill, stroke });
    }
}

/// What a pointer can hit.
#[derive(Debug, Clone, PartialEq)]
pub enum HitTarget {
    Target(Target),
    /// toggle a class's revealed member findings
    Reveal(String),
    /// hover only (implicit dependencies)
    None,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HitShape {
    /// world-space box
    Rect { x1: f64, y1: f64, x2: f64, y2: f64 },
    /// world-space polyline with a hit width
    Line { pts: Vec<Pt>, width: f64 },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub shape: HitShape,
    pub target: HitTarget,
    /// hover key (`class:Name`, `method:Class.name`, `relation:id`, `package:x`, …)
    pub hover: Option<String>,
    /// draggable node id
    pub node: Option<String>,
    /// tooltip text (SVG `<title>`)
    pub title: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Scene {
    pub root: Group,
    /// in paint order: later hits are on top
    pub hits: Vec<Hit>,
}

impl Scene {
    /// The topmost hit at a world point.
    pub fn hit_at(&self, p: Pt) -> Option<&Hit> {
        self.hits.iter().rev().find(|h| match &h.shape {
            HitShape::Rect { x1, y1, x2, y2 } => p[0] >= *x1 && p[0] <= *x2 && p[1] >= *y1 && p[1] <= *y2,
            HitShape::Line { pts, width } => pts.windows(2).any(|w| seg_dist(p, w[0], w[1]) <= width / 2.0),
        })
    }
}

fn seg_dist(p: Pt, a: Pt, b: Pt) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let l2 = dx * dx + dy * dy;
    let t = if l2 == 0.0 { 0.0 } else { (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2).clamp(0.0, 1.0) };
    let (x, y) = (a[0] + t * dx, a[1] + t * dy);
    ((p[0] - x).powi(2) + (p[1] - y).powi(2)).sqrt()
}

/// Accumulated transform while walking the scene.
#[derive(Debug, Clone, Copy)]
pub struct Xf {
    pub tx: f64,
    pub ty: f64,
    pub s: f64,
}

impl Xf {
    pub const ID: Xf = Xf { tx: 0.0, ty: 0.0, s: 1.0 };
    pub fn then(self, g: &Group) -> Xf {
        Xf { tx: self.tx + g.tx * self.s, ty: self.ty + g.ty * self.s, s: self.s * g.scale }
    }
    pub fn apply(self, p: Pt) -> Pt {
        [self.tx + p[0] * self.s, self.ty + p[1] * self.s]
    }
}

/// Bounding box of everything drawn, skipping groups with any tag in `skip`.
pub fn bbox(g: &Group, m: &dyn Measure, skip: u8) -> Option<[f64; 4]> {
    let mut b = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    walk_bbox(g, Xf::ID, m, skip, &mut b);
    (b[0].is_finite() && b[2] > b[0] && b[3] > b[1]).then_some(b)
}

fn grow(b: &mut [f64; 4], p: Pt) {
    b[0] = b[0].min(p[0]);
    b[1] = b[1].min(p[1]);
    b[2] = b[2].max(p[0]);
    b[3] = b[3].max(p[1]);
}

fn walk_bbox(g: &Group, parent: Xf, m: &dyn Measure, skip: u8, b: &mut [f64; 4]) {
    if g.tags & skip != 0 {
        return;
    }
    let xf = parent.then(g);
    for p in &g.children {
        match p {
            Prim::Rect { x, y, w, h, stroke, .. } => {
                let sw = stroke.map(|s| s.width as f64 / 2.0).unwrap_or(0.0);
                grow(b, xf.apply([x - sw, y - sw]));
                grow(b, xf.apply([x + w + sw, y + h + sw]));
            }
            Prim::Glow { x, y, w, h, .. } => {
                grow(b, xf.apply([*x, *y]));
                grow(b, xf.apply([x + w, y + h]));
            }
            Prim::Circle { cx, cy, r, .. } => {
                grow(b, xf.apply([cx - r, cy - r]));
                grow(b, xf.apply([cx + r, cy + r]));
            }
            Prim::Ellipse { cx, cy, rx, ry, .. } => {
                grow(b, xf.apply([cx - rx, cy - ry]));
                grow(b, xf.apply([cx + rx, cy + ry]));
            }
            Prim::Poly { pts, .. } => pts.iter().for_each(|q| grow(b, xf.apply(*q))),
            Prim::Path { cmds, .. } => crate::route::flatten(cmds).iter().for_each(|q| grow(b, xf.apply(*q))),
            Prim::Text { x, y, text, font, anchor, .. } => {
                let w = m.width(text, *font);
                let x0 = match anchor {
                    Anchor::Start => *x,
                    Anchor::Middle => x - w / 2.0,
                    Anchor::End => x - w,
                };
                let s = font.size as f64;
                grow(b, xf.apply([x0, y - s * 0.8]));
                grow(b, xf.apply([x0 + w, y + s * 0.25]));
            }
            Prim::Group(c) => walk_bbox(c, xf, m, skip, b),
        }
    }
}
