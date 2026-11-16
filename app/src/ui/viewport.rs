//! Pan/zoom over a world-space scene, shared by every diagram view (port of `Viewport.tsx`).

use super::paint::{AnimClock, EguiMeasure, ScenePainter, View};
use super::theme::col;
use egui::{Color32, CursorIcon, Modifiers, PointerButton, Pos2, Rect, Sense, Stroke, Vec2};
use spide_diagram::scene::{bbox, Hit, HitTarget, Scene};
use spide_diagram::visuals::c;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    pub x: f32,
    pub y: f32,
    pub k: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Gesture {
    None,
    Pan,
    Right,
    Drag,
}

/// World-space rectangle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorldRect {
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
}

pub enum ViewEvent {
    Click(HitTarget, Modifiers),
    DoubleClick(HitTarget, Pos2),
    /// right-click (single) or right-drag marquee
    PruneOne(HitTarget, Pos2),
    Marquee(WorldRect, Pos2),
    /// hover key under the pointer (None: nothing), screen position
    Hover(Option<String>, Pos2),
    /// a draggable node was grabbed at a world point
    DragStart(String, [f64; 2]),
    DragMove([f64; 2]),
    DragEnd(bool),
}

pub struct Viewport {
    pub t: Transform,
    pub reduced_motion: bool,
    anim: Option<(Transform, Transform, f64, f64)>,
    pub fitted: bool,
    last_size: Option<Vec2>,
    pub inset_right: f32,
    gesture: Gesture,
    start: Pos2,
    start_t: Transform,
    moved: bool,
    target: Option<Hit>,
    marquee: Option<WorldRect>,
    pub rect: Rect,
    pub clock: AnimClock,
    /// panning/zooming right now (animations pause, hover clears)
    pub interacting_until: f64,
    pending_fit: Option<(f64, bool)>,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            t: Transform { x: 0.0, y: 0.0, k: 1.0 },
            reduced_motion: false,
            anim: None,
            fitted: false,
            last_size: None,
            inset_right: 0.0,
            gesture: Gesture::None,
            start: Pos2::ZERO,
            start_t: Transform { x: 0.0, y: 0.0, k: 1.0 },
            moved: false,
            target: None,
            marquee: None,
            rect: Rect::NOTHING,
            clock: AnimClock::default(),
            interacting_until: 0.0,
            pending_fit: None,
        }
    }
}

impl Viewport {
    pub fn view(&self) -> View {
        View { origin: self.rect.min + egui::vec2(self.t.x, self.t.y), k: self.t.k }
    }
    pub fn to_world(&self, p: Pos2) -> [f64; 2] {
        self.view().world(p)
    }
    pub fn zoom(&self) -> f32 {
        self.t.k
    }

    pub fn restore_camera(&mut self, x: f32, y: f32, zoom: f32) {
        self.t = Transform { x, y, k: zoom };
        self.anim = None;
        self.pending_fit = None;
        self.fitted = true;
    }

    fn animate_to(&mut self, to: Transform, secs: f64, now: f64) {
        if self.reduced_motion {
            self.t = to;
            self.anim = None;
        } else {
            self.anim = Some((self.t, to, now, secs));
        }
    }

    pub fn fit_rect(&mut self, b: [f64; 4], animate: bool, now: f64) {
        let (bw, bh) = ((b[2] - b[0]) as f32, (b[3] - b[1]) as f32);
        let (w, h) = (self.rect.width() - self.inset_right, self.rect.height());
        if bw <= 0.0 || w <= 0.0 {
            return;
        }
        let margin = 90.0;
        let bottom = 24.0;
        let k = ((w - 2.0 * margin) / bw).min((h - margin - bottom) / bh).clamp(0.2, 1.25);
        let to = Transform { k, x: w / 2.0 - (b[0] as f32 + bw / 2.0) * k, y: (h - bottom + margin) / 2.0 - (b[1] as f32 + bh / 2.0) * k + 10.0 };
        if animate {
            self.animate_to(to, 0.45, now);
        } else {
            self.t = to;
            self.anim = None;
        }
    }

    pub fn fit(&mut self, scene: &Scene, m: &EguiMeasure, animate: bool, now: f64) {
        if let Some(b) = bbox(&scene.root, m, 0) {
            self.fit_rect(b, animate, now);
        }
    }

    /// Fit once the scene exists (after a short delay, like the old 60 ms timer).
    pub fn request_fit(&mut self, at: f64, animate: bool) {
        self.pending_fit = Some((at, animate));
    }

    pub fn fit_pending(&self) -> bool {
        self.pending_fit.is_some()
    }

    /// Centre a world point (in the visible area left of the inset), optionally at zoom k.
    pub fn zoom_to(&mut self, x: f64, y: f64, k: Option<f32>, now: f64) {
        let kk = k.unwrap_or(self.t.k.max(0.9));
        let bottom = 24.0;
        let to = Transform { k: kk, x: (self.rect.width() - self.inset_right) / 2.0 - x as f32 * kk, y: (self.rect.height() - bottom) / 2.0 - y as f32 * kk };
        self.animate_to(to, 0.45, now);
    }

    pub fn zoom_by(&mut self, f: f32, now: f64) {
        let (mx, my) = (self.rect.width() / 2.0, self.rect.height() / 2.0);
        let t = self.t;
        let k = (t.k * f).clamp(0.15, 3.0);
        self.animate_to(Transform { k, x: mx - (mx - t.x) * k / t.k, y: my - (my - t.y) * k / t.k }, 0.16, now);
    }

    fn step(&mut self, now: f64) -> bool {
        let Some((from, to, t0, dur)) = self.anim else { return false };
        let u = ((now - t0) / dur).clamp(0.0, 1.0) as f32;
        let e = 1.0 - (1.0 - u).powi(3);
        self.t = Transform { x: from.x + (to.x - from.x) * e, y: from.y + (to.y - from.y) * e, k: from.k + (to.k - from.k) * e };
        if u >= 1.0 {
            self.anim = None;
        }
        true
    }

    /// Lay out, handle input and paint `scene` in `rect`. `draggable` decides whether a node hit
    /// starts a node drag (otherwise the canvas pans).
    pub fn show(&mut self, ui: &mut egui::Ui, rect: Rect, scene: &Scene, m: &EguiMeasure, draggable: bool, events: &mut Vec<ViewEvent>) -> egui::Response {
        let now = ui.input(|i| i.time);
        self.reduced_motion = ui.ctx().data(|d| d.get_temp::<bool>(egui::Id::unique("reduced-motion"))).unwrap_or(false);
        if self.reduced_motion {
            if let Some((_, to, _, _)) = self.anim.take() { self.t = to; }
            self.clock.settle_all = true;
        }
        // keep the content centred when the canvas resizes (panes dragged, window resized)
        if let Some(prev) = self.last_size {
            let d = rect.size() - prev;
            if d.length() > 0.5 && prev.x > 0.0 {
                self.t.x += d.x / 2.0;
                self.t.y += d.y / 2.0;
                if let Some((f, to, t0, dur)) = self.anim {
                    self.anim = Some((f, Transform { x: to.x + d.x / 2.0, y: to.y + d.y / 2.0, ..to }, t0, dur));
                }
            }
        }
        self.last_size = Some(rect.size());
        self.rect = rect;
        if let Some((at, animate)) = self.pending_fit {
            if now >= at {
                self.pending_fit = None;
                self.fitted = true;
                self.fit(scene, m, animate, now);
            } else {
                ui.ctx().request_repaint();
            }
        }
        if self.step(now) {
            ui.ctx().request_repaint();
        }
        let id = ui.scope_id().with("viewport");
        let resp = ui.interact(rect, id, Sense::click_and_drag());
        let pointer = ui.input(|i| i.pointer.interact_pos().or(i.pointer.hover_pos()));
        let mods = ui.input(|i| i.modifiers);

        // wheel: pan; pinch / ⌘-scroll: zoom
        if resp.hovered() {
            let (scroll, zoom) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta()));
            if zoom != 1.0 {
                if let Some(p) = pointer {
                    self.anim = None;
                    let (mx, my) = (p.x - rect.min.x, p.y - rect.min.y);
                    let t = self.t;
                    let k = (t.k * zoom).clamp(0.15, 3.0);
                    self.t = Transform { k, x: mx - (mx - t.x) * k / t.k, y: my - (my - t.y) * k / t.k };
                    self.interacting_until = now + 0.2;
                    events.push(ViewEvent::Hover(None, p));
                }
            } else if scroll != Vec2::ZERO {
                self.anim = None;
                self.t.x += scroll.x;
                self.t.y += scroll.y;
                self.interacting_until = now + 0.2;
                events.push(ViewEvent::Hover(None, pointer.unwrap_or_default()));
            }
        }

        let hit_at = |vp: &Viewport, p: Pos2| scene.hit_at(vp.to_world(p)).cloned();
        // gestures
        let (pressed_primary, pressed_secondary, released_primary, released_secondary) =
            ui.input(|i| (i.pointer.button_pressed(PointerButton::Primary), i.pointer.button_pressed(PointerButton::Secondary), i.pointer.button_released(PointerButton::Primary), i.pointer.button_released(PointerButton::Secondary)));
        if let Some(p) = pointer {
            if resp.hovered() && (pressed_primary || pressed_secondary) && self.gesture == Gesture::None {
                events.push(ViewEvent::Hover(None, p));
                self.start = p;
                self.start_t = self.t;
                self.moved = false;
                self.target = hit_at(self, p);
                if pressed_secondary {
                    self.gesture = Gesture::Right;
                } else {
                    let node = self.target.as_ref().and_then(|h| h.node.clone());
                    match node.filter(|_| draggable) {
                        Some(n) => {
                            self.gesture = Gesture::Drag;
                            events.push(ViewEvent::DragStart(n, self.to_world(p)));
                        }
                        None => self.gesture = Gesture::Pan,
                    }
                }
            }
            if self.gesture != Gesture::None {
                let d = p - self.start;
                if d.length() > 4.0 {
                    self.moved = true;
                }
                if self.moved {
                    match self.gesture {
                        Gesture::Pan => {
                            self.anim = None;
                            self.t = Transform { x: self.start_t.x + d.x, y: self.start_t.y + d.y, ..self.t };
                            self.interacting_until = now + 0.2;
                        }
                        Gesture::Drag => events.push(ViewEvent::DragMove(self.to_world(p))),
                        Gesture::Right => {
                            let [x1, y1] = self.to_world(self.start);
                            let [x2, y2] = self.to_world(p);
                            self.marquee = Some(WorldRect { x1: x1.min(x2), y1: y1.min(y2), x2: x1.max(x2), y2: y1.max(y2) });
                        }
                        Gesture::None => {}
                    }
                }
            }
            let released = (released_primary && matches!(self.gesture, Gesture::Pan | Gesture::Drag)) || (released_secondary && self.gesture == Gesture::Right);
            if released {
                let kind = self.gesture;
                self.gesture = Gesture::None;
                if kind == Gesture::Drag {
                    events.push(ViewEvent::DragEnd(self.moved));
                }
                if kind == Gesture::Right {
                    match (self.moved, self.marquee.take()) {
                        (true, Some(m)) => events.push(ViewEvent::Marquee(m, p)),
                        _ => {
                            if let Some(h) = self.target.take().filter(|h| !matches!(h.target, HitTarget::None)) {
                                events.push(ViewEvent::PruneOne(h.target, p));
                            }
                        }
                    }
                }
                if matches!(kind, Gesture::Pan | Gesture::Drag) && !self.moved {
                    if let Some(h) = self.target.take() {
                        if !matches!(h.target, HitTarget::None) {
                            events.push(ViewEvent::Click(h.target, mods));
                        }
                    }
                }
                self.target = None;
            }
        }
        if resp.double_clicked() {
            if let Some(p) = pointer {
                match hit_at(self, p).map(|h| h.target).filter(|t| !matches!(t, HitTarget::None)) {
                    Some(t) => events.push(ViewEvent::DoubleClick(t, p)),
                    None => self.fit(scene, m, true, now),
                }
            }
        }
        // hover
        let mut hovered_hit: Option<Hit> = None;
        if self.gesture == Gesture::None {
            if let Some(p) = ui.input(|i| i.pointer.hover_pos()).filter(|p| rect.contains(*p) && resp.hovered()) {
                hovered_hit = hit_at(self, p);
                events.push(ViewEvent::Hover(hovered_hit.as_ref().and_then(|h| h.hover.clone()), p));
            } else if resp.hovered() || !ui.rect_contains_pointer(rect) {
                if let Some(p) = pointer {
                    if !rect.contains(p) {
                        events.push(ViewEvent::Hover(None, p));
                    }
                }
            }
        }
        if self.gesture == Gesture::Pan && self.moved || self.gesture == Gesture::Drag {
            ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
        } else if let Some(h) = &hovered_hit {
            if h.node.is_some() && draggable && h.hover.as_deref().is_some_and(|k| k.starts_with("class:")) {
                ui.ctx().set_cursor_icon(CursorIcon::Grab);
            } else if !matches!(h.target, HitTarget::None) {
                ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            }
        }

        // paint
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, super::theme::css::BG);
        self.dots(&painter, rect);
        self.clock.begin_frame();
        let interacting = now < self.interacting_until;
        {
            let mut sp = ScenePainter { painter: &painter, view: self.view(), now, clock: &mut self.clock, fonts: m };
            sp.paint(&scene.root);
        }
        self.clock.end_frame();
        if self.clock.busy && !interacting {
            ui.ctx().request_repaint();
        } else if interacting {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(220));
        }
        if let Some(mq) = self.marquee {
            let v = self.view();
            let a = v.to_screen(spide_diagram::scene::Xf::ID, [mq.x1, mq.y1]);
            let b = v.to_screen(spide_diagram::scene::Xf::ID, [mq.x2, mq.y2]);
            let r = Rect::from_min_max(a, b);
            painter.rect_filled(r, 6.0, Color32::from_rgba_unmultiplied(255, 92, 108, 18));
            let pts = super::paint::rounded_rect_points(r, 6.0);
            let mut closed = pts.clone();
            closed.push(pts[0]);
            painter.extend(egui::Shape::dashed_line(&closed, Stroke::new(1.2, col(c::CYCLE, 1.0)), 6.0, 4.0));
            painter.text(Pos2::new(r.min.x + 8.0, r.min.y - 8.0), egui::Align2::LEFT_BOTTOM, "✂ prune", super::theme::sans(11.0), col(c::CYCLE, 1.0));
        }
        // SVG <title> tooltips
        if let Some(t) = hovered_hit.as_ref().and_then(|h| h.title.clone()) {
            resp.clone().on_hover_text_at_pointer(t);
        }
        resp
    }

    fn dots(&self, painter: &egui::Painter, rect: Rect) {
        let step = 28.0 * self.t.k;
        if step < 7.0 {
            return;
        }
        let color = col(c::GRID, 1.0);
        let r = (self.t.k).clamp(0.5, 2.0);
        let ox = (rect.min.x + self.t.x + self.t.k).rem_euclid(step);
        let oy = (rect.min.y + self.t.y + self.t.k).rem_euclid(step);
        let mut y = rect.min.y - (rect.min.y - oy).rem_euclid(step);
        while y < rect.max.y {
            let mut x = rect.min.x - (rect.min.x - ox).rem_euclid(step);
            while x < rect.max.x {
                painter.circle_filled(Pos2::new(x, y), r, color);
                x += step;
            }
            y += step;
        }
    }
}
