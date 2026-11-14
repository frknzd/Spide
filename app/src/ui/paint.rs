//! Paints a `spide_diagram::scene::Scene` with egui, including the decorative animations.

use super::theme::{col, font_id};
use egui::{epaint, Color32, CornerRadius, Pos2, Rect, Shape, Stroke as EStroke, StrokeKind, Vec2};
use spide_diagram::route::{flatten, Pt};
use spide_diagram::scene::{Anchor, Anim, Group, Prim, Stroke, Xf};
use spide_diagram::visuals::{FontSpec, Measure};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

/// Start times of running animations, keyed by element; forgotten when the element disappears.
#[derive(Default)]
pub struct AnimClock {
    started: HashMap<String, f64>,
    seen: HashSet<String>,
    /// an animation is still running: keep repainting
    pub busy: bool,
    /// skip entrance animations (first paint of a view)
    pub settle_all: bool,
}

impl AnimClock {
    pub fn settled() -> Self {
        Self { settle_all: true, ..Default::default() }
    }
    fn start(&mut self, key: &str, now: f64) -> f64 {
        self.seen.insert(key.to_string());
        if self.settle_all {
            return *self.started.entry(key.to_string()).or_insert(now - 1e6);
        }
        *self.started.entry(key.to_string()).or_insert(now)
    }
    /// Call after painting a frame.
    pub fn end_frame(&mut self) {
        let seen = std::mem::take(&mut self.seen);
        self.started.retain(|k, _| seen.contains(k));
        self.settle_all = false;
    }
    pub fn begin_frame(&mut self) {
        self.busy = false;
    }
}

fn ease_in_out(t: f64) -> f64 {
    // CSS `ease-in-out` ≈ cubic-bezier(.42,0,.58,1)
    if t < 0.5 {
        2.0 * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(2) / 2.0
    }
}

/// cubic-bezier(.2,1.4,.4,1): overshooting scale-in
fn overshoot(t: f64) -> f64 {
    let (x1, y1, x2, y2) = (0.2, 1.4, 0.4, 1.0);
    // solve x(u) = t by bisection
    let bx = |u: f64| 3.0 * (1.0 - u).powi(2) * u * x1 + 3.0 * (1.0 - u) * u * u * x2 + u * u * u;
    let by = |u: f64| 3.0 * (1.0 - u).powi(2) * u * y1 + 3.0 * (1.0 - u) * u * u * y2 + u * u * u;
    let (mut lo, mut hi) = (0.0, 1.0);
    for _ in 0..24 {
        let mid = (lo + hi) / 2.0;
        if bx(mid) < t {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    by((lo + hi) / 2.0)
}

/// Screen mapping of the world: `screen = origin + world * k`.
#[derive(Clone, Copy, Debug)]
pub struct View {
    pub origin: Pos2,
    pub k: f32,
}

impl View {
    pub fn to_screen(&self, xf: Xf, p: Pt) -> Pos2 {
        let w = xf.apply(p);
        Pos2::new(self.origin.x + w[0] as f32 * self.k, self.origin.y + w[1] as f32 * self.k)
    }
    pub fn world(&self, p: Pos2) -> Pt {
        [((p.x - self.origin.x) / self.k) as f64, ((p.y - self.origin.y) / self.k) as f64]
    }
}

pub struct ScenePainter<'a> {
    pub painter: &'a egui::Painter,
    pub view: View,
    pub now: f64,
    pub clock: &'a mut AnimClock,
    pub fonts: &'a EguiMeasure,
}

/// Per-group animation state handed down to children.
#[derive(Clone, Copy, Default)]
struct Motion {
    /// dash offset in world px (marching dashes) and its dash override
    dash_offset: Option<(f32, [f32; 2])>,
}

impl ScenePainter<'_> {
    pub fn paint(&mut self, g: &Group) {
        self.group(g, Xf::ID, 1.0, Motion::default());
    }

    fn group(&mut self, g: &Group, parent: Xf, parent_opacity: f32, motion: Motion) {
        let mut xf = parent.then(g);
        let mut opacity = parent_opacity * g.opacity;
        let mut motion = motion;
        match &g.anim {
            Anim::None => {}
            Anim::Enter(key) | Anim::Ghost(key) => {
                let t0 = self.clock.start(key, self.now);
                let t = self.now - t0;
                let enter = (t / 0.52).clamp(0.0, 1.0);
                if enter < 1.0 {
                    self.clock.busy = true;
                    let s = 0.82 + 0.18 * overshoot(enter);
                    opacity *= enter as f32;
                    // scale about the centre of the group's own box
                    let (cx, cy) = local_centre(g);
                    xf = Xf { tx: xf.tx + xf.s * cx * (1.0 - s), ty: xf.ty + xf.s * cy * (1.0 - s), s: xf.s * s };
                }
                if matches!(g.anim, Anim::Ghost(_)) {
                    let bt = t - 0.52;
                    if bt > 0.0 && bt < 2.4 * 5.0 {
                        self.clock.busy = true;
                        let ph = (bt % 2.4) / 2.4;
                        let tri = if ph < 0.5 { ph * 2.0 } else { 2.0 - ph * 2.0 };
                        opacity *= (1.0 - 0.45 * ease_in_out(tri)) as f32;
                    }
                }
            }
            Anim::Pulse { key, period, times } => {
                let t0 = self.clock.start(key, self.now);
                let t = self.now - t0;
                let total = *period as f64 * *times as f64;
                if t < total {
                    self.clock.busy = true;
                    let ph = (t % *period as f64) / *period as f64;
                    let tri = if ph < 0.5 { ph * 2.0 } else { 2.0 - ph * 2.0 };
                    // keyframes replace the element's opacity: .35 → .8 → .35
                    opacity = parent_opacity * (0.35 + 0.45 * ease_in_out(tri)) as f32;
                }
            }
            Anim::Strain(key) => {
                let t0 = self.clock.start(key, self.now);
                let t = self.now - t0;
                let off = if t < 0.9 * 7.0 {
                    self.clock.busy = true;
                    -36.0 * ((t % 0.9) / 0.9)
                } else {
                    0.0
                };
                motion.dash_offset = Some((off as f32, [14.0, 4.0]));
            }
            Anim::Flow(key) => {
                let t0 = self.clock.start(key, self.now);
                let t = self.now - t0;
                if t < 1.2 * 10.0 {
                    self.clock.busy = true;
                    motion.dash_offset = Some((-24.0 * ((t % 1.2) / 1.2) as f32, [7.0, 5.0]));
                }
            }
        }
        if opacity <= 0.003 {
            return;
        }
        for p in &g.children {
            self.prim(p, xf, opacity, motion);
        }
    }

    fn color(&self, c: spide_diagram::visuals::Rgb, a: f32) -> Color32 {
        col(c, a)
    }

    fn stroke_path(&self, pts: &[Pos2], s: &Stroke, opacity: f32, scale: f32, closed: bool, motion: Motion) {
        if pts.len() < 2 {
            return;
        }
        let width = (s.width * scale).max(0.35);
        let color = self.color(s.color, s.opacity * opacity);
        let es = EStroke::new(width, color);
        let dash = match (motion.dash_offset, s.dash) {
            // a strained edge always marches with its 14/4 pattern; flowing ghosts keep their own
            (Some((_, d)), None) => Some(d),
            (_, d) => d,
        };
        let mut path: Vec<Pos2> = pts.to_vec();
        if closed {
            path.push(pts[0]);
        }
        match dash {
            Some([a, b]) => {
                let off = motion.dash_offset.map(|o| o.0).unwrap_or(0.0);
                let total = (a + b) * scale;
                let offset = (-off * scale).rem_euclid(total.max(0.001));
                self.painter.extend(Shape::dashed_line_with_offset(&path, es, &[a * scale], &[b * scale], offset));
            }
            None => {
                if closed {
                    self.painter.add(Shape::closed_line(pts.to_vec(), es));
                } else {
                    self.painter.add(Shape::line(path, es));
                }
            }
        }
    }

    fn prim(&mut self, p: &Prim, xf: Xf, opacity: f32, motion: Motion) {
        let k = self.view.k * xf.s as f32;
        let clip = self.painter.clip_rect().expand(40.0);
        match p {
            Prim::Rect { x, y, w, h, rx, fill, stroke } => {
                let a = self.view.to_screen(xf, [*x, *y]);
                let b = self.view.to_screen(xf, [x + w, y + h]);
                let r = Rect::from_min_max(a, b);
                if !clip.intersects(r.expand(30.0)) {
                    return;
                }
                let rad = (*rx as f32 * k).min(r.width() / 2.0).min(r.height() / 2.0).clamp(0.0, 255.0);
                if let Some(f) = fill {
                    self.painter.add(egui::epaint::RectShape::filled(r, CornerRadius::same(rad.round() as u8), self.color(f.color, f.opacity * opacity)));
                }
                if let Some(s) = stroke {
                    if s.dash.is_some() || motion.dash_offset.is_some() {
                        let pts = rounded_rect_points(r, rad);
                        self.stroke_path(&pts, s, opacity, k, true, motion);
                    } else {
                        let es = EStroke::new((s.width * k).max(0.35), self.color(s.color, s.opacity * opacity));
                        self.painter.add(egui::epaint::RectShape::stroke(r, CornerRadius::same(rad.round() as u8), es, StrokeKind::Middle));
                    }
                }
            }
            Prim::Glow { x, y, w, h, rx, color, opacity: o } => {
                let a = self.view.to_screen(xf, [*x, *y]);
                let b = self.view.to_screen(xf, [x + w, y + h]);
                let r = Rect::from_min_max(a, b).shrink(13.6 * k);
                let shadow = egui::Shadow { offset: [0, 0], blur: (27.0 * k).clamp(1.0, 255.0) as u8, spread: 0, color: self.color(*color, o * opacity) };
                self.painter.add(shadow.as_shape(r, CornerRadius::same((*rx as f32 * k).clamp(0.0, 255.0) as u8)));
            }
            Prim::Circle { cx, cy, r, fill, stroke } => {
                let c = self.view.to_screen(xf, [*cx, *cy]);
                let rr = *r as f32 * k;
                if let Some(f) = fill {
                    self.painter.circle_filled(c, rr, self.color(f.color, f.opacity * opacity));
                }
                if let Some(s) = stroke {
                    if s.dash.is_some() {
                        let pts: Vec<Pos2> = (0..48).map(|i| c + Vec2::angled(i as f32 / 48.0 * std::f32::consts::TAU) * rr).collect();
                        self.stroke_path(&pts, s, opacity, k, true, motion);
                    } else {
                        self.painter.circle_stroke(c, rr, EStroke::new((s.width * k).max(0.35), self.color(s.color, s.opacity * opacity)));
                    }
                }
            }
            Prim::Ellipse { cx, cy, rx, ry, fill, stroke } => {
                let c = self.view.to_screen(xf, [*cx, *cy]);
                let r = Vec2::new(*rx as f32 * k, *ry as f32 * k);
                if let Some(f) = fill {
                    self.painter.add(Shape::ellipse_filled(c, r, self.color(f.color, f.opacity * opacity)));
                }
                if let Some(s) = stroke {
                    if s.dash.is_some() {
                        let pts: Vec<Pos2> = (0..64).map(|i| {
                            let a = i as f32 / 64.0 * std::f32::consts::TAU;
                            c + Vec2::new(a.cos() * r.x, a.sin() * r.y)
                        }).collect();
                        self.stroke_path(&pts, s, opacity, k, true, motion);
                    } else {
                        self.painter.add(Shape::ellipse_stroke(c, r, EStroke::new((s.width * k).max(0.35), self.color(s.color, s.opacity * opacity))));
                    }
                }
            }
            Prim::Poly { pts, closed, fill, stroke } => {
                let sp: Vec<Pos2> = pts.iter().map(|q| self.view.to_screen(xf, *q)).collect();
                if let Some(f) = fill {
                    if sp.len() >= 3 {
                        self.painter.add(Shape::convex_polygon(sp.clone(), self.color(f.color, f.opacity * opacity), EStroke::NONE));
                    }
                }
                if let Some(s) = stroke {
                    self.stroke_path(&sp, s, opacity, k, *closed, motion);
                }
            }
            Prim::Path { cmds, stroke } => {
                let sp: Vec<Pos2> = flatten(cmds).into_iter().map(|q| self.view.to_screen(xf, q)).collect();
                self.stroke_path(&sp, stroke, opacity, k, false, motion);
            }
            Prim::Text { x, y, text, font, color, opacity: o, anchor, underline } => {
                let size = font.size * k;
                if size < 2.5 {
                    return;
                }
                let at = self.view.to_screen(xf, [*x, *y]);
                if !clip.contains(at) && !clip.expand(400.0 * k).contains(at) {
                    return;
                }
                let c = self.color(*color, o * opacity);
                let fid = font_id(FontSpec { size, ..*font });
                let galley = self.painter.layout_no_wrap(text.clone(), fid, c);
                let w = galley.size().x;
                let baseline = galley_baseline(&galley);
                let x0 = match anchor {
                    Anchor::Start => at.x,
                    Anchor::Middle => at.x - w / 2.0,
                    Anchor::End => at.x - w,
                };
                let pos = Pos2::new(x0, at.y - baseline);
                if *underline {
                    let uy = at.y + size * 0.12;
                    self.painter.line_segment([Pos2::new(x0, uy), Pos2::new(x0 + w, uy)], EStroke::new((k * 0.9).max(0.5), c));
                }
                self.painter.galley(pos, galley, c);
            }
            Prim::Group(g) => self.group(g, xf, opacity, motion),
        }
    }
}

/// The baseline of the first row, relative to the galley's top.
pub fn galley_baseline(g: &epaint::Galley) -> f32 {
    if let Some(row) = g.rows.first() {
        if let Some(gl) = row.row.glyphs.first() {
            return row.pos.y + gl.pos.y;
        }
        return row.pos.y + row.row.size.y * 0.78;
    }
    0.0
}

fn local_centre(g: &Group) -> (f64, f64) {
    // the first rectangle of a node group is its body (halos come first but are concentric)
    for p in &g.children {
        if let Prim::Rect { x, y, w, h, .. } = p {
            return (x + w / 2.0, y + h / 2.0);
        }
        if let Prim::Group(c) = p {
            for q in &c.children {
                if let Prim::Rect { x, y, w, h, .. } = q {
                    return (c.tx + x + w / 2.0, c.ty + y + h / 2.0);
                }
            }
        }
    }
    (0.0, 0.0)
}

pub fn rounded_rect_points(r: Rect, rad: f32) -> Vec<Pos2> {
    let mut pts = vec![];
    let corners = [(r.right() - rad, r.top() + rad, -90.0f32), (r.right() - rad, r.bottom() - rad, 0.0), (r.left() + rad, r.bottom() - rad, 90.0), (r.left() + rad, r.top() + rad, 180.0)];
    for (cx, cy, start) in corners {
        for i in 0..=6 {
            let a = (start + 90.0 * i as f32 / 6.0).to_radians();
            pts.push(Pos2::new(cx + rad * a.cos(), cy + rad * a.sin()));
        }
    }
    pts
}

/// Text measurement with the real egui fonts (so boxes fit what is painted), cached.
pub struct EguiMeasure {
    ctx: egui::Context,
    cache: RefCell<HashMap<(String, u32, u8), f64>>,
}

impl EguiMeasure {
    pub fn new(ctx: &egui::Context) -> Self {
        Self { ctx: ctx.clone(), cache: RefCell::new(HashMap::new()) }
    }
}

impl Measure for EguiMeasure {
    fn width(&self, text: &str, font: FontSpec) -> f64 {
        let key = (text.to_string(), font.size.to_bits(), (font.bold as u8) | ((font.italic as u8) << 1) | (((font.family == spide_diagram::visuals::Family::Mono) as u8) << 2));
        if let Some(w) = self.cache.borrow().get(&key) {
            return *w;
        }
        // measure at 10× and scale down: avoids pixel rounding at small sizes
        let big = FontSpec { size: font.size * 10.0, ..font };
        let w = self.ctx.fonts_mut(|f| f.layout_no_wrap(text.to_string(), font_id(big), Color32::WHITE).size().x) as f64 / 10.0;
        let mut c = self.cache.borrow_mut();
        if c.len() > 50_000 {
            c.clear();
        }
        c.insert(key, w);
        w
    }
}
