//! The class / domain canvas (port of `Canvas.tsx`): the living layout, semantic zoom, focus,
//! drag, pin, prune, reveal and the scene it paints.

use super::paint::EguiMeasure;
use super::viewport::{ViewEvent, Viewport, WorldRect};
use egui::{Modifiers, Pos2, Rect};
use spide_core::metrics::Metrics;
use spide_core::model::{Class, Layer, Package, Pos, Status};
use spide_core::Target;
use spide_diagram::class_view::{self, external_classes, rect_of, Data, Lens, Opts, Routes, Stress, SyncSets};
use spide_diagram::focus::Footprint;
use spide_diagram::insights::{self, Marks, Members, OverlapLink};

use spide_diagram::physics::{LinkSpec, LivingLayout, NodeSpec};
use spide_diagram::scene::{HitTarget, Scene};
use spide_diagram::visuals::ClassBox;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::hash::{Hash, Hasher};

pub fn layer_rank(l: Layer) -> u8 {
    l.rank()
}

/// Vertical band of a package: ui and infrastructure (outer ring) on top, application, then domain.
pub fn package_rank(p: Option<&Package>) -> u8 {
    p.and_then(|p| p.layer).map(layer_rank).unwrap_or(1)
}

pub enum CanvasOut {
    /// ⌥-right-click / ⌥-right-drag: the fast prune gesture
    Prune(Vec<Target>, Pos2),
    /// right-click on an element: select it and open its context menu
    Context(Target, Pos2),
    /// right-drag box (⇧ adds to the selection)
    Select(Vec<Target>, bool),
    Click(Target, Modifiers),
    DoubleClick(Target, Pos2),
    Hover(Option<String>, Pos2),
    MoveToPackage(String, String),
    SaveLayout(BTreeMap<String, Pos>),
}

pub struct CanvasInput<'a> {
    pub classes: &'a [Class],
    pub relations: &'a [spide_core::model::Relation],
    pub packages: &'a [Package],
    pub metrics: Option<&'a Metrics>,
    pub implicit: &'a [spide_core::metrics::ImplicitEdge],
    /// changes whenever the data above changes (snapshot rev, preview identity, …)
    pub data_key: u64,
    pub saved: Option<&'a BTreeMap<String, Pos>>,
    pub show_methods: bool,
    pub level: u8,
    pub show_implicit: bool,
    pub lens: Lens,
    pub read_only: bool,
    pub review_focus: Option<&'a Footprint>,
    pub stress: Option<&'a Stress>,
    pub sync: Option<&'a SyncSets>,
    /// hovered element key (focus mode, ghost spotlight)
    pub hovered: Option<&'a str>,
    /// ghost spotlight requested from the proposal bar: `Some(None)` = all ghosts
    pub spotlight: Option<Option<&'a str>>,
    pub can_move_to_package: bool,
    pub can_edit: bool,
    /// review: read-only, but clicks select concrete elements (inspection, refinement targets)
    pub pickable: bool,
    /// classes drawn as selected (review refinement targets)
    pub highlight: Option<&'a HashSet<String>>,
    /// keep node positions: no automatic re-layout when a batch of classes appears (review toggles)
    pub keep_layout: bool,
}

struct Insights {
    marks: Marks,
    concerns: HashMap<String, HashMap<String, String>>,
    overlap: Vec<OverlapLink>,
    scattered: HashMap<String, Vec<String>>,
    members: Members,
}

pub struct ClassCanvas {
    pub key: String,
    pub view_key: String,
    pub vp: Viewport,
    pub layout: LivingLayout,
    routes: Routes,
    boxes: HashMap<String, ClassBox>,
    boxes_key: u64,
    sync_key: u64,
    insights_key: u64,
    insights: Option<Insights>,
    last_level: Option<u8>,
    pub revealed: HashSet<String>,
    pub pinned_focus: Option<String>,
    pub focus_set: Option<HashSet<String>>,
    /// export mode: full detail regardless of zoom, optionally without diagnostics colouring
    pub export_plain: Option<bool>,
    settled_sig: String,
    route_epoch: u64,
    dragging: Option<(String, [f64; 2])>,
    pending_tidy: bool,
    fit_after_tidy: bool,
    pub scene: Scene,
    hovered_row: Option<(String, String)>,
}

fn hash_of(v: impl Hash) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    v.hash(&mut h);
    h.finish()
}

impl ClassCanvas {
    pub fn new(key: String, view_key: &str) -> Self {
        Self {
            key,
            view_key: view_key.into(),
            vp: Viewport::default(),
            layout: LivingLayout::new(),
            routes: Routes::default(),
            boxes: HashMap::new(),
            boxes_key: 0,
            sync_key: 0,
            insights_key: 0,
            insights: None,
            last_level: None,
            revealed: HashSet::new(),
            pinned_focus: None,
            focus_set: None,
            export_plain: None,
            settled_sig: String::new(),
            route_epoch: 0,
            dragging: None,
            pending_tidy: false,
            fit_after_tidy: false,
            scene: Scene::default(),
            hovered_row: None,
        }
    }

    /// The scene with the layout at rest (exports: never a frame of a running animation).
    pub fn settled_scene(&mut self, m: &EguiMeasure, inp: &CanvasInput, now: f64) -> Scene {
        self.sync(m, inp, now);
        if self.layout.active() {
            self.layout.settle();
            self.route_epoch += 1;
        }
        self.build_scene(m, inp, now)
    }

    pub fn tidy(&mut self) {
        self.pending_tidy = true;
        self.fit_after_tidy = true;
    }

    pub fn positions(&self) -> HashMap<String, [f64; 2]> {
        self.layout.nodes.iter().map(|n| (n.id.clone(), [n.x, n.y])).collect()
    }

    /// Zoom to a class and pin focus mode on it; None clears. False if it isn't on the canvas.
    pub fn focus(&mut self, name: Option<&str>, now: f64) -> bool {
        self.focus_set = None;
        let Some(name) = name else {
            self.pinned_focus = None;
            return true;
        };
        let Some(n) = self.layout.get(name) else { return false };
        let (x, y) = (n.x, n.y);
        self.pinned_focus = Some(name.to_string());
        self.vp.zoom_to(x, y, None, now);
        true
    }

    fn rects_bounds(&self, names: &[String], metrics: Option<&Metrics>) -> Option<[f64; 4]> {
        let pos = self.positions();
        let rs: Vec<_> = names.iter().filter_map(|n| rect_of(n, &pos, &self.boxes, metrics)).collect();
        if rs.is_empty() {
            return None;
        }
        Some([
            rs.iter().map(|r| r.x - r.w / 2.0).fold(f64::INFINITY, f64::min),
            rs.iter().map(|r| r.y - r.h / 2.0).fold(f64::INFINITY, f64::min),
            rs.iter().map(|r| r.x + r.w / 2.0).fold(f64::NEG_INFINITY, f64::max),
            rs.iter().map(|r| r.y + r.h / 2.0).fold(f64::NEG_INFINITY, f64::max),
        ])
    }

    /// Zoom to a package and dim everything else; false if it has no classes on the canvas.
    pub fn focus_package(&mut self, classes: &[Class], metrics: Option<&Metrics>, pkg: &str, now: f64) -> bool {
        let members: Vec<String> = classes.iter().filter(|c| c.package.as_deref() == Some(pkg)).map(|c| c.name.clone()).collect();
        let Some([x1, y1, x2, y2]) = self.rects_bounds(&members, metrics) else { return false };
        let r = self.vp.rect;
        let k = ((r.width() - self.vp.inset_right - 80.0) as f64 / (x2 - x1).max(1.0)).min((r.height() - 80.0) as f64 / (y2 - y1).max(1.0)).clamp(0.2, 1.2) as f32;
        self.pinned_focus = None;
        self.focus_set = Some(members.into_iter().collect());
        self.vp.zoom_to((x1 + x2) / 2.0, (y1 + y2) / 2.0, Some(k), now);
        true
    }

    /// Zoom so a footprint's changed classes and the classes it connects to are all in view.
    pub fn locate(&mut self, fp: &Footprint, metrics: Option<&Metrics>, now: f64) -> bool {
        let names: Vec<String> = fp.changed.iter().chain(fp.anchors.iter()).cloned().collect();
        let Some([x1, y1, x2, y2]) = self.rects_bounds(&names, metrics) else { return false };
        let r = self.vp.rect;
        let k = ((r.width() - self.vp.inset_right - 160.0) as f64 / (x2 - x1).max(1.0)).min((r.height() - 160.0) as f64 / (y2 - y1).max(1.0)).clamp(0.25, 1.1) as f32;
        self.vp.zoom_to((x1 + x2) / 2.0, (y1 + y2) / 2.0, Some(k), now);
        true
    }

    fn sync(&mut self, m: &EguiMeasure, inp: &CanvasInput, now: f64) {
        let level = inp.level.max(1);
        let ins_key = hash_of((inp.data_key, inp.metrics.is_some()));
        if self.insights.is_none() || ins_key != self.insights_key {
            self.insights_key = ins_key;
            let mm = inp.metrics;
            self.insights = Some(Insights {
                marks: insights::method_marks(mm),
                concerns: insights::method_concerns(mm),
                overlap: insights::overlap_links(mm),
                scattered: insights::scattered_by_class(mm),
                members: insights::member_diagnostics(mm, inp.classes),
            });
        }
        let ins = self.insights.as_ref().unwrap();
        // box sizes (and so the layout) are the same whether or not an export hides diagnostics:
        // an export never re-lays out the canvas, it draws what you see
        let mut reveal: Vec<&String> = self.revealed.iter().collect();
        reveal.sort();
        let bkey = hash_of((inp.data_key, level, inp.show_methods, &reveal));
        if bkey != self.boxes_key {
            self.boxes_key = bkey;
            let ext = external_classes(inp.classes, inp.packages);
            self.boxes = class_view::boxes_for(m, inp.classes, inp.show_methods, level, inp.relations, None, &ext, Some(&ins.marks), Some(&ins.members), &self.revealed);
        }
        // sync the simulation with the model
        let skey = bkey;
        if skey != self.sync_key {
            self.sync_key = skey;
            let cm = inp.metrics.map(|m| &m.classes);
            let pk: HashMap<&str, &Package> = inp.packages.iter().map(|p| (p.name.as_str(), p)).collect();
            let specs: Vec<NodeSpec> = inp
                .classes
                .iter()
                .filter_map(|c| {
                    let b = self.boxes.get(&c.name)?;
                    let (w, h) = class_view::node_extent(m, c, b, inp.metrics);
                    let layer = c.package.as_deref().and_then(|p| pk.get(p)).and_then(|p| p.layer);
                    Some(NodeSpec {
                        id: c.name.clone(),
                        w,
                        h,
                        pkg: c.package.clone(),
                        layer: layer.map(layer_rank),
                        heat: cm.and_then(|m| m.get(&c.name)).map(|m| m.heat).unwrap_or(0.0),
                        init: inp.saved.and_then(|s| s.get(&c.name)).copied(),
                    })
                })
                .collect();
            let fresh = specs.iter().filter(|s| !self.layout.has(&s.id) && s.init.is_none()).count();
            let links: Vec<LinkSpec> = inp.relations.iter().map(|r| LinkSpec { from: r.from.clone(), to: r.to.clone(), kind: r.kind.as_str().into() }).collect();
            self.layout.update(&specs, &links);
            // Restore complete saved placements on first load; the new layout policy
            // takes effect on explicit tidy or subsequent structural/detail changes.
            let restoring = self.last_level.is_none() && !specs.is_empty() && specs.iter().all(|s| s.init.is_some());
            // Review keeps positions while alternatives are toggled.
            if !inp.keep_layout && !restoring {
                self.pending_tidy = true;
                // many new classes (first load, big plans): show the whole new drawing
                self.fit_after_tidy |= fresh >= 3;
            }
        }
        // a different granularity changes every box: lay out again
        if self.last_level.is_some_and(|l| l != level) {
            self.pending_tidy = true;
            self.fit_after_tidy = true;
        }
        self.last_level = Some(level);
        if self.pending_tidy {
            self.pending_tidy = false;
            let extents: HashMap<String, (f64, f64)> = self.layout.nodes.iter().map(|n| (n.id.clone(), (n.w, n.h))).collect();
            let pos = class_view::layout_classes(m, inp.classes, &extents, inp.relations, inp.packages);
            self.layout.tidy(&pos);
            if std::mem::take(&mut self.fit_after_tidy) {
                self.vp.request_fit(now + 1.5, true);
            }
        }
    }

    fn collect(&self, classes: &[Class], relations: &[spide_core::model::Relation], r: WorldRect) -> Vec<Target> {
        let pos = self.positions();
        let inside = |q: &[f64; 2]| q[0] >= r.x1 && q[0] <= r.x2 && q[1] >= r.y1 && q[1] <= r.y2;
        let mut out: Vec<Target> = vec![];
        for c in classes {
            if let Some(q) = pos.get(&c.name) {
                if inside(q) {
                    out.push(match (&c.proposal, c.status) {
                        (Some(id), Status::Proposed) => Target::Proposal { id: id.clone() },
                        _ => Target::Class { name: c.name.clone() },
                    });
                }
            }
        }
        // relations whose midpoint is inside and whose ends aren't both selected
        for rel in relations {
            let (Some(a), Some(b)) = (pos.get(&rel.from), pos.get(&rel.to)) else { continue };
            let mid = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
            if inside(&mid) && !(inside(a) || inside(b)) {
                out.push(Target::Relation { id: rel.id.clone() });
            }
        }
        let mut seen = Vec::new();
        out.retain(|t| {
            if seen.contains(t) {
                false
            } else {
                seen.push(t.clone());
                true
            }
        });
        out
    }

    /// Package box (as drawn) under a dragged class's centre, other than its own.
    fn package_at(&self, inp: &CanvasInput, id: &str) -> Option<String> {
        let me = inp.classes.iter().find(|c| c.name == id)?;
        let pos = self.positions();
        let at = pos.get(id)?;
        for pk in inp.packages {
            if Some(&pk.name) == me.package.as_ref() || pk.status == Status::Pruned {
                continue;
            }
            let rs: Vec<_> = inp.classes.iter().filter(|c| c.package.as_ref() == Some(&pk.name) && c.name != id).filter_map(|c| rect_of(&c.name, &pos, &self.boxes, inp.metrics)).collect();
            if rs.is_empty() {
                continue;
            }
            let [x1, y1, x2, y2] = class_view::hull_of(&rs);
            if at[0] >= x1 && at[0] <= x2 && at[1] >= y1 && at[1] <= y2 {
                return Some(pk.name.clone());
            }
        }
        None
    }

    fn zoom_cap(&self) -> u8 {
        let k = self.vp.zoom();
        if k < 0.22 {
            0
        } else if k < 0.42 {
            1
        } else {
            4
        }
    }

    /// Build the scene (interactive state or export state).
    pub fn build_scene(&mut self, m: &EguiMeasure, inp: &CanvasInput, now: f64) -> Scene {
        self.sync(m, inp, now);
        let ins = self.insights.as_ref().unwrap();
        let exporting = self.export_plain.is_some();
        let plain = self.export_plain == Some(true);
        // Hovering a ghost spotlights its whole proposal; the proposal bar can spotlight one or all.
        let hovered_proposal = spide_diagram::focus::proposal_at(inp.hovered, inp.classes, inp.relations);
        let spot_fp;
        let proposal_focus: Option<&Footprint> = if let Some(f) = inp.review_focus {
            Some(f)
        } else if inp.read_only {
            None
        } else {
            let ids: std::collections::BTreeSet<String> = match (&hovered_proposal, inp.spotlight) {
                (Some(p), _) => [p.clone()].into(),
                (None, Some(None)) => inp
                    .classes
                    .iter()
                    .flat_map(|c| std::iter::once(c.proposal.clone()).chain(c.attributes.iter().map(|a| a.proposal.clone())).chain(c.methods.iter().map(|m| m.proposal.clone())))
                    .chain(inp.relations.iter().map(|r| r.proposal.clone()))
                    .flatten()
                    .collect(),
                (None, Some(Some(id))) => [id.to_string()].into(),
                _ => Default::default(),
            };
            spot_fp = if ids.is_empty() { None } else { spide_diagram::focus::footprint_of(inp.classes, inp.relations, inp.packages, &ids) };
            spot_fp.as_ref()
        };
        let hovered_class = inp.hovered.and_then(|k| {
            if let Some(c) = k.strip_prefix("class:") {
                Some(c.to_string())
            } else if let Some(rest) = k.strip_prefix("method:").or_else(|| k.strip_prefix("attribute:")) {
                rest.rfind('.').map(|i| rest[..i].to_string())
            } else {
                None
            }
        });
        let focus_class = if proposal_focus.is_some() { None } else { hovered_class.or_else(|| self.pinned_focus.clone()) };
        let pinned = if exporting { HashSet::new() } else { self.layout.pinned_ids() };
        let empty = HashSet::new();
        let o = Opts {
            pos: self.positions(),
            boxes: self.boxes.clone(),
            show_methods: inp.show_methods,
            still: exporting,
            focus_class: if exporting { None } else { focus_class.as_deref() },
            highlight: if exporting { None } else { inp.highlight },
            stress: if exporting { None } else { inp.stress },
            sync: if exporting { None } else { inp.sync },
            pinned,
            focus_set: if exporting { None } else { self.focus_set.as_ref() },
            plain,
            route_epoch: self.route_epoch,
            proposal_focus: if exporting { None } else { proposal_focus },
            hide_packages: false,
            // detail 0 is the package overview, on screen and in exports
            zoom_cap: if inp.level == 0 { 0 } else if exporting { 4 } else { self.zoom_cap() },
            level: inp.level.max(1),
            show_implicit: inp.show_implicit,
            marks: Some(&ins.marks),
            lens: inp.lens,
            concerns: Some(&ins.concerns),
            overlap: Some(&ins.overlap),
            scattered: Some(&ins.scattered),
            members: Some(&ins.members),
            revealed: Some(if exporting { &empty } else { &self.revealed }),
            hovered_row: self.hovered_row.as_ref().map(|(a, b)| (a.as_str(), b.as_str())),
            concrete_targets: inp.pickable && !exporting,
        };
        let data = Data { classes: inp.classes, relations: inp.relations, packages: inp.packages, metrics: inp.metrics, implicit: inp.implicit };
        class_view::build(m, &data, &o, &mut self.routes)
    }

    pub fn show(&mut self, ui: &mut egui::Ui, rect: Rect, m: &EguiMeasure, inp: &CanvasInput, out: &mut Vec<CanvasOut>) {
        let now = ui.input(|i| i.time);
        // physics
        let step = self.layout.step(now);
        if step.moved || self.layout.active() {
            ui.ctx().request_repaint();
        }
        if step.settled {
            let pos = self.layout.positions();
            // re-route fully only if something moved since the last full route
            let sig = format!("{pos:?}");
            if sig != self.settled_sig {
                self.settled_sig = sig;
                self.route_epoch += 1;
            }
            if !inp.read_only && !pos.is_empty() {
                out.push(CanvasOut::SaveLayout(pos));
            }
        }
        self.scene = self.build_scene(m, inp, now);
        if !self.vp.fitted && !inp.classes.is_empty() {
            self.vp.fitted = true;
            if !self.vp.fit_pending() {
                self.vp.request_fit(now + 0.06, false);
            }
        }
        if inp.classes.is_empty() {
            self.vp.fitted = false;
        }
        let mut events = vec![];
        let scene = std::mem::take(&mut self.scene);
        self.vp.show(ui, rect, &scene, m, !inp.read_only, &mut events);
        self.scene = scene;
        for e in events {
            match e {
                ViewEvent::Hover(k, p) => {
                    self.hovered_row = k.as_deref().and_then(|k| {
                        let (kind, rest) = k.split_once(':')?;
                        if kind != "method" && kind != "attribute" {
                            return None;
                        }
                        let dot = rest.find('.')?;
                        Some((rest[..dot].to_string(), format!("{kind}:{}", &rest[dot + 1..])))
                    });
                    out.push(CanvasOut::Hover(k, p));
                }
                ViewEvent::Click(HitTarget::Reveal(name), _) => {
                    if !self.revealed.remove(&name) {
                        self.revealed.insert(name);
                    }
                }
                ViewEvent::Click(HitTarget::Target(t), mods) => {
                    if inp.read_only && !inp.pickable {
                        continue;
                    }
                    // ⌥-click pins / unpins a class where it is
                    if mods.alt && !inp.read_only {
                        if let Target::Class { name } = &t {
                            if self.layout.has(name) {
                                self.layout.toggle_pin(name);
                                out.push(CanvasOut::SaveLayout(self.layout.positions()));
                                continue;
                            }
                        }
                    }
                    out.push(CanvasOut::Click(t, mods));
                }
                ViewEvent::Click(HitTarget::None, _) => {}
                ViewEvent::DoubleClick(HitTarget::Target(t), p) => {
                    if !inp.read_only && inp.can_edit {
                        out.push(CanvasOut::DoubleClick(t, p));
                    }
                }
                ViewEvent::DoubleClick(..) => {}
                ViewEvent::PruneOne(HitTarget::Target(t), p) => {
                    let alt = ui.input(|i| i.modifiers.alt);
                    if alt && !inp.read_only {
                        out.push(CanvasOut::Prune(vec![t], p));
                    } else if !inp.pickable {
                        out.push(CanvasOut::Context(t, p));
                    }
                }
                ViewEvent::PruneOne(..) => {}
                ViewEvent::Marquee(r, p) => {
                    let (alt, shift) = ui.input(|i| (i.modifiers.alt, i.modifiers.shift));
                    let ts = self.collect(inp.classes, inp.relations, r);
                    if alt && !inp.read_only {
                        if !ts.is_empty() {
                            out.push(CanvasOut::Prune(ts, p));
                        }
                    } else if !inp.pickable {
                        out.push(CanvasOut::Select(ts, shift));
                    }
                }
                ViewEvent::DragStart(id, w) => {
                    if let Some(n) = self.layout.get(&id) {
                        self.dragging = Some((id.clone(), [n.x - w[0], n.y - w[1]]));
                    }
                }
                ViewEvent::DragMove(w) => {
                    if let Some((id, off)) = self.dragging.clone() {
                        self.layout.drag(&id, w[0] + off[0], w[1] + off[1]);
                        ui.ctx().request_repaint();
                    }
                }
                ViewEvent::DragEnd(moved) => {
                    if let Some((id, _)) = self.dragging.take() {
                        self.layout.release(&id);
                        if moved && inp.can_move_to_package {
                            if let Some(target) = self.package_at(inp, &id) {
                                out.push(CanvasOut::MoveToPackage(id, target));
                            }
                        }
                    }
                }
            }
        }
    }
}
