//! The derived views on screen: package, C4 system (context/containers), use cases and sequences,
//! with their banners, level toggle and scenario sidebar.

use super::paint::EguiMeasure;
use super::theme::{css, hex, sans};
use super::viewport::{ViewEvent, Viewport};
use super::widgets;
use crate::state::{PiStatus, Snapshot};
use egui::{Color32, Margin, Pos2, Rect, RichText, Sense, Stroke, Vec2};
use spide_core::model::{MessageKind, Status};
use spide_core::Target;
use spide_diagram::scene::{HitTarget, Scene};
use spide_diagram::views::{self, C4Level, PackageState, SystemState, UseCaseState};

#[derive(Default)]
pub struct Views {
    pub package_vp: Viewport,
    pub system_vp: Viewport,
    pub usecase_vp: Viewport,
    pub sequence_vp: Viewport,
    pub package: PackageState,
    pub system: SystemState,
    pub usecase: UseCaseState,
    pub c4_level: Option<C4Level>,
    /// the last built scene per view (export, hits)
    pub scene: Scene,
    pub points: Vec<(Target, [f64; 2])>,
    pub asked_usecases: bool,
    last_seq: String,
    last_level: Option<C4Level>,
}

pub enum ViewOut {
    Prune(Vec<Target>, Pos2),
    Click(Target),
    Hover(Option<String>, Pos2),
    Regenerate(&'static str, Option<String>),
    SelectScenario(Option<String>),
}

fn banner(ui: &mut egui::Ui, at: Pos2, center: bool, text: &str, busy: bool, button: Option<&str>) -> bool {
    let mut clicked = false;
    egui::Area::new(ui.scope_id().with(("banner", text))).order(egui::Order::Middle).pivot(egui::Align2::CENTER_TOP).fixed_pos(at).show(ui.ctx(), |ui| {
        egui::Frame::new()
            .fill(css::PANEL_SOLID)
            .stroke(Stroke::new(1.0, if center { css::LINE } else { Color32::from_rgba_unmultiplied(255, 180, 84, 102) }))
            .corner_radius(9)
            .inner_margin(Margin { left: 14, right: 8, top: 6, bottom: 6 })
            .shadow(egui::Shadow { offset: [0, 10], blur: 30, spread: 0, color: Color32::from_black_alpha(90) })
            .show(ui, |ui| {
                ui.set_max_width(ui.ctx().content_rect().width() * 0.25);
                ui.horizontal(|ui| {
                    ui.add(egui::Label::new(RichText::new(text).font(sans(12.5)).color(if center { css::DIM } else { hex(0xffd9a3) })).wrap());
                    if busy {
                        ui.spinner();
                        ui.label(RichText::new("agent working…").font(sans(12.0)).color(css::FAINT));
                    } else if let Some(b) = button {
                        let (fg, bg) = if center { (css::ACCENT, Color32::from_rgba_unmultiplied(122, 162, 255, 36)) } else { (css::WARN, Color32::from_rgba_unmultiplied(255, 180, 84, 36)) };
                        if ui.add(egui::Button::new(RichText::new(b).font(sans(12.0)).color(fg)).fill(bg).stroke(Stroke::NONE).corner_radius(6).min_size(Vec2::new(0.0, 26.0))).clicked() {
                            clicked = true;
                        }
                    }
                });
            });
    });
    clicked
}

fn handle(vp: &mut Viewport, ui: &mut egui::Ui, rect: Rect, scene: &Scene, points: &[(Target, [f64; 2])], m: &EguiMeasure, out: &mut Vec<ViewOut>) {
    let mut events = vec![];
    vp.show(ui, rect, scene, m, false, &mut events);
    for e in events {
        match e {
            ViewEvent::Hover(k, p) => out.push(ViewOut::Hover(k, p)),
            ViewEvent::Click(HitTarget::Target(t), _) => out.push(ViewOut::Click(t)),
            ViewEvent::PruneOne(HitTarget::Target(t), p) => out.push(ViewOut::Prune(vec![t], p)),
            ViewEvent::Marquee(r, p) => {
                let ts = views::collect(points, r.x1, r.y1, r.x2, r.y2);
                if !ts.is_empty() {
                    out.push(ViewOut::Prune(ts, p));
                }
            }
            _ => {}
        }
    }
}

impl Views {
    pub fn level(&self) -> C4Level {
        self.c4_level.unwrap_or(C4Level::Container)
    }

    pub fn active_vp(&mut self, tab: super::chrome::Tab) -> &mut Viewport {
        use super::chrome::Tab;
        match tab {
            Tab::Package => &mut self.package_vp,
            Tab::System => &mut self.system_vp,
            Tab::Usecase => &mut self.usecase_vp,
            _ => &mut self.sequence_vp,
        }
    }

    pub fn package(&mut self, ui: &mut egui::Ui, rect: Rect, snap: &Snapshot, m: &EguiMeasure, out: &mut Vec<ViewOut>) {
        let vs = views::package_scene(m, &snap.derived, &snap.metrics, &mut self.package);
        self.fit_once(0, &vs.scene, ui);
        handle(&mut self.package_vp, ui, rect, &vs.scene, &vs.points, m, out);
        self.scene = vs.scene;
        self.points = vs.points;
    }

    fn fit_once(&mut self, which: u8, scene: &Scene, ui: &egui::Ui) {
        let vp = match which {
            0 => &mut self.package_vp,
            1 => &mut self.system_vp,
            2 => &mut self.usecase_vp,
            _ => &mut self.sequence_vp,
        };
        let empty = scene.root.children.iter().all(|c| matches!(c, spide_diagram::scene::Prim::Group(g) if g.children.is_empty()));
        if !vp.fitted && !empty {
            vp.fitted = true;
            vp.request_fit(ui.input(|i| i.time) + 0.06, false);
        }
        if empty {
            vp.fitted = false;
        }
    }

    pub fn system(&mut self, ui: &mut egui::Ui, rect: Rect, snap: &Snapshot, m: &EguiMeasure, out: &mut Vec<ViewOut>) {
        let level = self.level();
        if self.last_level != Some(level) {
            // a different level is a different diagram: fit again
            self.last_level = Some(level);
            self.system_vp = Viewport::default();
        }
        let vs = views::system_scene(m, &snap.derived, &snap.metrics, level, &mut self.system);
        self.fit_once(1, &vs.scene, ui);
        handle(&mut self.system_vp, ui, rect, &vs.scene, &vs.points, m, out);
        // level toggle
        egui::Area::new(ui.scope_id().with("level-toggle")).order(egui::Order::Middle).pivot(egui::Align2::CENTER_TOP).fixed_pos(Pos2::new(rect.center().x, rect.top() + 8.0)).show(ui.ctx(), |ui| {
            egui::Frame::new().fill(css::SURFACE).stroke(Stroke::new(1.0, css::LINE)).corner_radius(10).inner_margin(Margin::same(3)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    for (l, label) in [(C4Level::Context, "Context"), (C4Level::Container, "Containers")] {
                        let on = l == level;
                        let b = egui::Button::new(RichText::new(label).font(sans(12.0)).color(if on { css::TEXT } else { css::DIM })).fill(if on { css::RAISED2 } else { Color32::TRANSPARENT }).stroke(Stroke::NONE).corner_radius(6).min_size(Vec2::new(0.0, 24.0));
                        if ui.add(b).clicked() {
                            self.c4_level = Some(l);
                        }
                    }
                });
            });
        });
        if views::system_empty(&snap.derived, level) {
            let text = match level {
                C4Level::Container => "No containers yet — ask the architect, e.g. “which containers (deployable units) do we need?”",
                C4Level::Context => "No actors or external systems yet — generate use cases or ask which systems we integrate with.",
            };
            banner(ui, Pos2::new(rect.center().x, rect.top() + 44.0), true, text, false, None);
        }
        self.scene = vs.scene;
        self.points = vs.points;
    }

    pub fn usecase(&mut self, ui: &mut egui::Ui, rect: Rect, snap: &Snapshot, m: &EguiMeasure, out: &mut Vec<ViewOut>) {
        let ucs = snap.view.use_cases.use_cases.iter().filter(|u| u.status != Status::Pruned).count();
        if !self.asked_usecases && ucs == 0 && snap.pi_status == PiStatus::Idle && !snap.view.classes.is_empty() {
            self.asked_usecases = true;
            out.push(ViewOut::Regenerate("usecase", None));
        }
        let vs = views::usecase_scene(m, &snap.view, &mut self.usecase);
        self.fit_once(2, &vs.scene, ui);
        handle(&mut self.usecase_vp, ui, rect, &vs.scene, &vs.points, m, out);
        let stale = snap.derived.use_cases_stale;
        if stale || ucs == 0 {
            let busy = snap.pi_status == PiStatus::Busy;
            let empty = ucs == 0;
            let at = Pos2::new(rect.center().x, rect.top() + 8.0);
            if banner(ui, at, empty, if empty { "No use case diagram yet." } else { "Out of date with the class model." }, busy, Some(if empty { "↻ generate" } else { "↻ regenerate" })) {
                out.push(ViewOut::Regenerate("usecase", None));
            }
        }
        self.scene = vs.scene;
        self.points = vs.points;
    }

    pub fn sequence(&mut self, ui: &mut egui::Ui, rect: Rect, snap: &Snapshot, selected: Option<&str>, m: &EguiMeasure, out: &mut Vec<ViewOut>) {
        let seqs: Vec<_> = snap.view.sequences.iter().filter(|s| s.status != Status::Pruned).collect();
        let sel = seqs.iter().find(|s| Some(s.name.as_str()) == selected).or(seqs.first()).copied();
        let want = selected.filter(|n| !seqs.iter().any(|s| s.name == *n));
        let actors: Vec<&str> = snap.view.use_cases.actors.iter().map(|a| a.name.as_str()).collect();
        let busy = snap.pi_status == PiStatus::Busy;
        // sidebar
        let side_w = (rect.width() * 0.35).min(190.0);
        let side = Rect::from_min_size(Pos2::new(rect.left(), rect.top() + 8.0), Vec2::new(side_w, rect.height() - 16.0));
        let canvas = Rect::from_min_max(Pos2::new(side.right(), rect.top()), rect.max);
        let key = format!("{}", sel.map(|s| s.name.as_str()).unwrap_or("none"));
        if key != self.last_seq {
            self.last_seq = key;
            self.sequence_vp = Viewport::default();
        }
        let vs = match (sel, want) {
            (Some(s), None) => views::sequence_scene(m, s, &|n| actors.contains(&n)),
            _ => views::ViewScene { scene: Scene::default(), points: vec![] },
        };
        self.fit_once(3, &vs.scene, ui);
        handle(&mut self.sequence_vp, ui, canvas, &vs.scene, &vs.points, m, out);
        ui.scope_builder(egui::UiBuilder::new().max_rect(side), |ui| {
            egui::Frame::new().fill(css::SURFACE).inner_margin(Margin::same(8)).show(ui, |ui| {
                ui.set_width(side_w - 16.0);
                ui.set_min_height(side.height() - 16.0);
                let r = ui.max_rect();
                ui.painter().vline(side.right(), side.y_range(), Stroke::new(1.0, css::LINE));
                let _ = r;
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    widgets::section(ui, "sequences");
                    for s in &seqs {
                        let on = sel.is_some_and(|x| x.name == s.name) && want.is_none();
                        let probs = s.messages.iter().filter(|m| m.problem.is_some()).count();
                        let stale = snap.derived.stale_sequences.contains(&s.name);
                        if item(ui, &s.name, on, false, if probs > 0 { Some(format!(" ✗{probs}")) } else { None }, stale).clicked() {
                            out.push(ViewOut::SelectScenario(Some(s.name.clone())));
                        }
                    }
                    if seqs.is_empty() {
                        widgets::small_faint(ui, "none yet");
                    }
                    if !snap.derived.suggested_scenarios.is_empty() {
                        ui.add_space(6.0);
                        widgets::section(ui, "from use cases");
                    }
                    for s in &snap.derived.suggested_scenarios {
                        if item(ui, &format!("＋ {s}"), false, true, None, false).clicked() {
                            out.push(ViewOut::SelectScenario(Some(s.clone())));
                            out.push(ViewOut::Regenerate("sequence", Some(s.clone())));
                        }
                    }
                    ui.add_space(6.0);
                    widgets::small_faint(ui, "or ask in the prompt: “sequence for …”");
                    if let (None, Some(s)) = (want, sel) {
                        if let Some(sc) = snap.metrics.scenarios.iter().find(|x| x.name == s.name) {
                            ui.add_space(8.0);
                            widgets::hline(ui);
                            ui.add_space(4.0);
                            widgets::section(ui, &format!("fit of “{}”", sc.name));
                            let max = snap.view.rules.thresholds.max_scenario_hops;
                            let chip = |ui: &mut egui::Ui, t: String, bad: bool, tip: String| {
                                ui.label(RichText::new(t).font(sans(11.5)).color(if bad { css::WARN } else { css::DIM })).on_hover_text(tip);
                            };
                            chip(ui, format!("{} calls", sc.hops), sc.hops > max, format!("calls in this scenario (chatty above {max})"));
                            chip(ui, format!("depth {}", sc.max_depth), sc.max_depth > 4, "deepest nesting of synchronous calls".into());
                            chip(ui, format!("{} package crossings", sc.package_crossings), false, "calls between packages".into());
                            chip(ui, format!("{} layer crossings", sc.layer_crossings), false, "calls between layers".into());
                            if sc.container_crossings > 0 {
                                chip(ui, format!("{} container crossings", sc.container_crossings), sc.sync_container_calls > 1, format!("{} of them synchronous", sc.sync_container_calls));
                            }
                            let n = sc.aggregates_touched.len();
                            chip(ui, format!("{n} aggregate{}", if n == 1 { "" } else { "s" }), n > 1, if n > 0 { format!("touches {} — more than one aggregate per transaction weakens consistency", sc.aggregates_touched.join(", ")) } else { "no aggregates touched".into() });
                            if sc.unknown_calls > 0 {
                                chip(ui, format!("{} unknown", sc.unknown_calls), true, "calls to methods that don't exist".into());
                            }
                        }
                    }
                });
            });
        });
        let top = Pos2::new(canvas.center().x, canvas.top() + 8.0);
        let center = top;
        if let Some(w) = want {
            if banner(ui, center, true, &format!("No sequence for “{w}” yet."), busy, Some("↻ generate")) {
                out.push(ViewOut::Regenerate("sequence", Some(w.to_string())));
            }
        } else if sel.is_none() {
            if banner(ui, center, true, "No sequence diagrams yet.", busy, Some("↻ generate")) {
                out.push(ViewOut::Regenerate("sequence", None));
            }
        } else if let Some(s) = sel.filter(|s| snap.derived.stale_sequences.contains(&s.name)) {
            if banner(ui, top, false, "Class model changed since this was drawn.", busy, Some("↻ regenerate")) {
                out.push(ViewOut::Regenerate("sequence", Some(s.name.clone())));
            }
        }
        let _ = MessageKind::Sync;
        self.scene = vs.scene;
        self.points = vs.points;
    }
}

fn item(ui: &mut egui::Ui, text: &str, on: bool, sugg: bool, bad: Option<String>, stale: bool) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 26.0), Sense::click());
    if on {
        ui.painter().rect_filled(r, 6.0, super::theme::hex(0x353a45));
    } else if resp.hovered() {
        ui.painter().rect_filled(r, 6.0, super::theme::hex(0x30343d));
    }
    let color = if sugg {
        css::ACCENT
    } else if on || resp.hovered() {
        css::TEXT
    } else {
        css::DIM
    };
    let g = ui.painter().text(Pos2::new(r.left() + 8.0, r.center().y), egui::Align2::LEFT_CENTER, widgets::clip(text, 26), sans(12.5), color);
    let mut x = g.right() + 2.0;
    if let Some(b) = bad {
        let g2 = ui.painter().text(Pos2::new(x, r.center().y), egui::Align2::LEFT_CENTER, b, sans(11.0), css::BAD);
        x = g2.right() + 4.0;
    }
    if stale {
        ui.painter().circle_filled(Pos2::new(x + 4.0, r.center().y), 3.0, css::WARN);
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}
