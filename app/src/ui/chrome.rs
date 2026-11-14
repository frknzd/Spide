//! Canvas chrome: identity row, workspace actions, view tabs, toolbar (health ring and popover,
//! detail slider, zoom group), proposal dock, toasts, welcome and startup cards.

use super::theme::{bold, col, css, mono, sans};
use super::widgets::{self, button, toggle_button};
use egui::{Align2, Color32, CornerRadius, Margin, Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2};
use spide_core::metrics::{Issue, Metrics};
use spide_diagram::visuals::health_color;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Tab {
    System,
    Class,
    Domain,
    Package,
    Usecase,
    Sequence,
}

pub const TAB_ORDER: [Tab; 6] = [Tab::System, Tab::Class, Tab::Domain, Tab::Package, Tab::Usecase, Tab::Sequence];

impl Tab {
    pub fn key(self) -> &'static str {
        match self {
            Tab::System => "system",
            Tab::Class => "class",
            Tab::Domain => "domain",
            Tab::Package => "package",
            Tab::Usecase => "usecase",
            Tab::Sequence => "sequence",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Tab::System => "System",
            Tab::Class => "Class",
            Tab::Domain => "Domain",
            Tab::Package => "Package",
            Tab::Usecase => "Use case",
            Tab::Sequence => "Sequence",
        }
    }
    pub fn title(self) -> &'static str {
        match self {
            Tab::System => "System (C4)",
            Tab::Class => "Class diagram",
            Tab::Domain => "Domain model",
            Tab::Package => "Package diagram",
            Tab::Usecase => "Use case diagram",
            Tab::Sequence => "Sequence diagram",
        }
    }
}

pub fn hc(score: f64) -> Color32 {
    col(health_color(score), 1.0)
}

/// The view tabs; returns the clicked tab.
pub fn tabs(ui: &mut Ui, tab: Tab, stale: &dyn Fn(Tab) -> bool) -> Option<Tab> {
    let mut out = None;
    egui::Frame::new().fill(css::SURFACE).stroke(Stroke::new(1.0, css::LINE)).corner_radius(10).inner_margin(Margin::same(3)).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for (i, t) in TAB_ORDER.iter().enumerate() {
                let on = *t == tab;
                let label = RichText::new(stale_label(*t, stale(*t))).font(sans(12.0)).color(if on { css::TEXT } else { css::DIM });
                let b = egui::Button::new(label).fill(if on { css::RAISED2 } else { Color32::TRANSPARENT }).stroke(Stroke::NONE).corner_radius(7).min_size(Vec2::new(0.0, 26.0));
                let r = ui.add(b);
                if stale(*t) {
                    // the old `.stale` dot: amber, after the label
                    let c = Pos2::new(r.rect.right() - 7.0, r.rect.center().y);
                    ui.painter().circle_filled(c, 3.0, css::WARN);
                }
                let r = r.on_hover_text(format!("{} view · ⌘{}{}", t.label(), i + 1, if stale(*t) { " · out of date with the class model" } else { "" }));
                if r.clicked() {
                    out = Some(*t);
                }
            }
        });
    });
    out
}

/// Tabs need room for the stale dot: pad labels.
pub fn stale_label(t: Tab, stale: bool) -> String {
    if stale {
        format!("{}    ", t.label())
    } else {
        t.label().to_string()
    }
}

const PRINCIPLES: [(&str, &str, &str); 5] = [
    ("coupling", "coupling", "Minimise coupling: cycles, fan-out, Law of Demeter (coupling to stable libraries is fine)"),
    ("cohesion", "cohesion", "Increase cohesion: god classes, LCOM4, mixed concerns, vague names, long methods"),
    ("responsibility", "responsibilities", "Distribute responsibilities: Information Expert, Controller, Creator, Pure Fabrication, duplication, feature envy, data classes"),
    ("architecture", "architecture", "Your architecture rules: layers, modules, forbidden dependencies and types, orphans"),
    ("uml", "UML hygiene", "Exam notes: argument types, no repeated inherited members, Composite/Visitor sanity, long parameter lists"),
];

fn issue_color(kind: &str) -> Color32 {
    match kind {
        "cycle" | "god" => css::BAD,
        "layer" | "cohesion" | "coupling" | "forbidden_dep" | "module_boundary" | "forbidden_type" | "misplaced_responsibility" | "duplication" | "fat_controller" | "ui_bypasses_controller" | "concept_elsewhere" => css::WARN,
        "orphan" => css::FAINT,
        "disagreement" => css::ACCENT,
        _ => css::DIM,
    }
}

fn confidence_text(i: &Issue) -> Option<String> {
    if i.basis == "structure" || i.basis.is_empty() {
        return None;
    }
    Some(format!("{}% · {}", (i.confidence * 100.0).round(), i.basis))
}

pub enum HealthAction {
    Fix(usize),
    Focus(usize),
}

#[derive(Default)]
pub struct HealthState {
    pub open: bool,
    pub pinned: bool,
    pub only: Option<&'static str>,
    close_at: Option<f64>,
}

/// The health ring (30px), its label, and the breakdown popover on hover/click.
pub fn health_ring(ui: &mut Ui, st: &mut HealthState, m: &Metrics, ghost: Option<u8>, actions: &mut Vec<HealthAction>) {
    let h = m.health as f64;
    let color = hc(h);
    let now = ui.input(|i| i.time);
    let resp = egui::Frame::new().fill(css::SURFACE).stroke(Stroke::new(1.0, css::LINE)).corner_radius(20).inner_margin(Margin { left: 2, right: 10, top: 2, bottom: 2 }).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 7.0;
            let (r, _) = ui.allocate_exact_size(Vec2::splat(30.0), Sense::hover());
            let c = r.center();
            let rad = 11.9;
            let p = ui.painter();
            p.circle_stroke(c, rad, Stroke::new(2.5, Color32::from_rgb(0x2c, 0x2f, 0x36)));
            let arc = |frac: f64, color: Color32| {
                let n = 48;
                let pts: Vec<Pos2> = (0..=((n as f64 * frac).ceil() as usize)).map(|i| {
                    let a = -std::f32::consts::FRAC_PI_2 + (i as f32 / n as f32).min(frac as f32) * std::f32::consts::TAU;
                    c + Vec2::angled(a) * rad
                }).collect();
                if pts.len() > 1 {
                    p.add(egui::Shape::line(pts, Stroke::new(2.5, color)));
                }
            };
            if let Some(g) = ghost.filter(|g| *g as f64 != h) {
                arc(g as f64 / 100.0, hc(g as f64).gamma_multiply(0.35));
            }
            arc(h / 100.0, color);
            p.text(c + Vec2::new(0.0, 0.5), Align2::CENTER_CENTER, format!("{}", m.health), bold(8.5), color);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.label(RichText::new("Health").font(bold(11.5)).color(css::TEXT));
                let n = m.issues.len();
                ui.label(RichText::new(format!("{n} issue{}", if n == 1 { "" } else { "s" })).font(sans(10.5)).color(css::DIM));
            });
        });
    });
    let trigger = ui.interact(resp.response.rect, ui.scope_id().with("health-trigger"), Sense::click());
    if let Some(g) = ghost.filter(|g| *g as f64 != h) {
        let at = resp.response.rect.left_top() + Vec2::new(24.0, -6.0);
        ui.painter().text(at, Align2::LEFT_BOTTOM, format!("→ {g}"), bold(10.5), hc(g as f64));
    }
    if trigger.clicked() {
        if st.pinned {
            st.open = false;
            st.pinned = false;
        } else {
            st.open = true;
            st.pinned = true;
        }
    }
    if trigger.hovered() {
        st.open = true;
        st.close_at = None;
    }
    if !st.open {
        return;
    }
    let anchor = resp.response.rect.left_bottom();
    let max_w = (ui.clip_rect().right() - anchor.x - 10.0).clamp(280.0, 470.0);
    let area = egui::Area::new(ui.scope_id().with("health-pop")).order(egui::Order::Foreground).fixed_pos(anchor + Vec2::new(0.0, 8.0)).show(ui.ctx(), |ui| {
        widgets::panel_frame().inner_margin(Margin::symmetric(16, 14)).show(ui, |ui| {
            ui.set_width(max_w - 32.0);
            ui.set_max_height(ui.ctx().content_rect().height() * 0.7);
            health_popover(ui, st, m, ghost, actions);
        });
    });
    // close: click outside, Escape, or leaving (unless pinned)
    let pop = area.response.rect;
    let over = ui.input(|i| i.pointer.hover_pos()).is_some_and(|p| pop.expand(4.0).contains(p) || trigger.rect.expand(4.0).contains(p) || Rect::from_min_max(trigger.rect.left_bottom(), pop.right_top()).contains(p));
    let clicked_outside = ui.input(|i| i.pointer.any_pressed()) && !over;
    if clicked_outside || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        st.open = false;
        st.pinned = false;
    } else if !over && !st.pinned {
        let at = *st.close_at.get_or_insert(now + 0.28);
        if now >= at {
            st.open = false;
            st.close_at = None;
        } else {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(60));
        }
    } else {
        st.close_at = None;
    }
}

fn health_popover(ui: &mut Ui, st: &mut HealthState, m: &Metrics, ghost: Option<u8>, actions: &mut Vec<HealthAction>) {
    let h = m.health as f64;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.label(RichText::new("design health").font(sans(12.5)).color(css::DIM));
        ui.label(RichText::new(format!("{}/100", m.health)).font(bold(12.5)).color(hc(h)));
        if let Some(g) = ghost.filter(|g| *g as f64 != h) {
            ui.label(RichText::new(format!(" · {g} with proposals")).font(sans(12.5)).color(hc(g as f64)));
        }
    });
    ui.add_space(6.0);
    for (id, label, help) in PRINCIPLES {
        let Some(sc) = m.principles.get(id) else { continue };
        let on = st.only == Some(id);
        let (r, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 26.0), Sense::click());
        if on || resp.hovered() {
            ui.painter().rect_filled(r, 6.0, Color32::from_rgba_unmultiplied(122, 162, 255, 26));
        }
        let p = ui.painter();
        p.text(Pos2::new(r.left() + 4.0, r.center().y), Align2::LEFT_CENTER, label, sans(12.0), css::DIM);
        let bar = Rect::from_min_size(Pos2::new(r.left() + 122.0, r.center().y - 3.0), Vec2::new(r.width() - 122.0 - 42.0, 6.0));
        p.rect_filled(bar, 3.0, Color32::from_rgb(0x2c, 0x2f, 0x36));
        p.rect_filled(Rect::from_min_size(bar.min, Vec2::new(bar.width() * sc.score as f32 / 100.0, 6.0)), 3.0, hc(sc.score as f64));
        p.text(Pos2::new(r.right() - 4.0, r.center().y), Align2::RIGHT_CENTER, sc.score.to_string(), bold(12.0), hc(sc.score as f64));
        if resp.on_hover_text(format!("{help} · weight {}%", (sc.weight * 100.0).round())).clicked() {
            st.only = if on { None } else { Some(id) };
        }
    }
    ui.add_space(6.0);
    let issues: Vec<(usize, &Issue)> = m.issues.iter().enumerate().filter(|(_, i)| st.only.is_none_or(|o| i.principle == o)).collect();
    egui::ScrollArea::vertical().max_height(ui.ctx().content_rect().height() * 0.45).auto_shrink([false, true]).show(ui, |ui| {
        if st.only.is_some() && issues.is_empty() {
            ui.horizontal(|ui| {
                widgets::faint(ui, "No issues for this principle.");
                if widgets::link(ui, "Show all").clicked() {
                    st.only = None;
                }
            });
        }
        if m.issues.is_empty() {
            widgets::faint(ui, "Low coupling, high cohesion, responsibilities where the data is. Clean.");
        }
        for (id, label, _) in PRINCIPLES {
            let group: Vec<&(usize, &Issue)> = issues.iter().filter(|(_, i)| (if i.principle.is_empty() { "uml" } else { i.principle }) == id).collect();
            if group.is_empty() {
                continue;
            }
            ui.add_space(6.0);
            widgets::section(ui, &format!("{label} · {}", group.len()));
            let limit = if st.only.is_some() { usize::MAX } else { 6 };
            for (idx, i) in group.iter().take(limit) {
                issue_row(ui, *idx, i, actions);
            }
            if st.only.is_none() && group.len() > 6 && widgets::link_sized(ui, format!("… {} more", group.len() - 6), 11.0).clicked() {
                st.only = Some(id);
            }
        }
    });
    ui.add_space(8.0);
    widgets::hline(ui);
    ui.add_space(6.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 12.0;
        for (c, t) in [(0x5b8cff, "calm"), (0xffb454, "coupled"), (0xff5c6c, "hot")] {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let (r, _) = ui.allocate_exact_size(Vec2::splat(8.0), Sense::hover());
                ui.painter().circle_filled(r.center(), 4.0, super::theme::hex(c));
                ui.label(RichText::new(t).font(sans(11.0)).color(css::FAINT));
            });
        }
        ui.label(RichText::new("size ∝ coupling · one number: weighted mean of the principles, scaled to the design's size").font(sans(11.0)).color(css::FAINT));
    });
}

fn issue_row(ui: &mut Ui, idx: usize, i: &Issue, actions: &mut Vec<HealthAction>) {
    let tip = [Some(i.explain.clone()), i.fix_hint.as_ref().map(|f| format!("fix: {f}"))].into_iter().flatten().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\n");
    ui.add_space(5.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let kc = issue_color(i.kind);
        egui::Frame::new().stroke(Stroke::new(1.0, kc)).corner_radius(4).inner_margin(Margin::symmetric(5, 0)).show(ui, |ui| {
            ui.label(RichText::new(i.kind.replace('_', " ").to_uppercase()).font(sans(10.0)).color(kc));
        });
        let r = ui.add(egui::Label::new(RichText::new(&i.message).font(sans(12.5)).color(css::TEXT)).sense(Sense::click()).wrap());
        if r.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        let r = if tip.is_empty() { r } else { r.on_hover_text(tip) };
        if r.clicked() {
            actions.push(HealthAction::Focus(idx));
        }
    });
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        if !i.source.is_empty() {
            ui.label(RichText::new(i.source).font(sans(10.5)).color(css::FAINT));
        }
        if let Some(c) = confidence_text(i) {
            ui.label(RichText::new(c).font(sans(10.5)).color(css::FAINT)).on_hover_text("how sure Spide is (agent agreement raises it)");
        }
        if widgets::link_sized(ui, "fix with agent", 11.0).clicked() {
            actions.push(HealthAction::Fix(idx));
        }
    });
    ui.add_space(3.0);
    widgets::hline(ui);
}

/// Detail slider pill (`/detail 0–4`); returns the new level.
pub fn detail_slider(ui: &mut Ui, level: u8, disabled: bool) -> Option<u8> {
    const HELP: [&str; 5] = [
        "packages only, with their dependencies",
        "class names and stereotypes",
        "names plus responsibilities (RDD view)",
        "public behaviour; accessors, private helpers, inherited and association attributes folded (exam style)",
        "every member",
    ];
    let mut out = None;
    let l = level.min(4) as usize;
    egui::Frame::new().fill(css::SURFACE).stroke(Stroke::new(1.0, css::LINE)).corner_radius(15).inner_margin(Margin::symmetric(10, 0)).show(ui, |ui| {
        ui.set_height(30.0);
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.label(RichText::new("Detail").font(sans(11.0)).color(css::DIM));
            let mut v = level as f32;
            ui.spacing_mut().slider_width = 65.0;
            let r = ui.add_enabled(!disabled, egui::Slider::new(&mut v, 0.0..=4.0).step_by(1.0).show_value(false).trailing_fill(true));
            let tip = if disabled { "Exit preview to change detail".to_string() } else { format!("{} — {}", spide_diagram::visuals::DETAIL_LABELS[l], HELP[l]) };
            if r.on_hover_text(tip).changed() {
                out = Some(v.round() as u8);
            }
            ui.label(RichText::new(spide_diagram::visuals::DETAIL_LABELS[l]).font(sans(11.0)).color(css::TEXT));
        });
    });
    out
}

pub enum Zoom {
    Out,
    Fit,
    In,
}

pub fn zoom_group(ui: &mut Ui) -> Option<Zoom> {
    let mut out = None;
    egui::Frame::new().fill(css::SURFACE).stroke(Stroke::new(1.0, css::LINE)).corner_radius(15).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            let b = |ui: &mut Ui, t: &str, tip: &str| {
                ui.add(egui::Button::new(RichText::new(t).font(sans(12.0)).color(css::TEXT)).frame(false).min_size(Vec2::new(30.0, 30.0))).on_hover_text(tip)
            };
            if b(ui, "−", "Zoom out").clicked() {
                out = Some(Zoom::Out);
            }
            let sep = |ui: &mut Ui| {
                let (r, _) = ui.allocate_exact_size(Vec2::new(1.0, 30.0), Sense::hover());
                ui.painter().vline(r.center().x, r.y_range(), Stroke::new(1.0, css::LINE));
            };
            sep(ui);
            if b(ui, " Fit ", "Fit diagram · double-click canvas").clicked() {
                out = Some(Zoom::Fit);
            }
            sep(ui);
            if b(ui, "+", "Zoom in").clicked() {
                out = Some(Zoom::In);
            }
        });
    });
    out
}

/// "Review proposals (N)" dock button (bottom centre of the canvas).
pub fn proposal_bar(ui: &mut Ui, count: usize) -> bool {
    let b = egui::Button::new(RichText::new(format!("Review proposals ({count})")).font(sans(12.0)).color(css::TEXT)).fill(css::PANEL_SOLID).stroke(Stroke::NONE).corner_radius(18).min_size(Vec2::new(0.0, 36.0));
    let r = ui.add(b);
    // dashed ghost border
    let pts = super::paint::rounded_rect_points(r.rect, 18.0);
    let mut closed = pts.clone();
    closed.push(pts[0]);
    ui.painter().extend(egui::Shape::dashed_line(&closed, Stroke::new(1.0, Color32::from_rgba_unmultiplied(94, 234, 212, 140)), 4.0, 3.0));
    r.clicked()
}

#[derive(Debug, Clone)]
pub struct Toast {
    pub id: u64,
    pub kind: String,
    pub text: String,
    pub until: f64,
}

/// Toasts, top-right of the canvas. Returns ids to dismiss.
pub fn toasts(ctx: &egui::Context, area: Rect, toasts: &mut Vec<Toast>) {
    let now = ctx.input(|i| i.time);
    if toasts.is_empty() {
        return;
    }
    egui::Area::new(egui::Id::unique("toasts")).order(egui::Order::Tooltip).pivot(Align2::RIGHT_TOP).fixed_pos(Pos2::new(area.right() - 16.0, area.top() + 44.0)).show(ctx, |ui| {
        ui.with_layout(egui::Layout::top_down(egui::Align::Max), |ui| {
            let mut dismiss = vec![];
            for t in toasts.iter_mut() {
                let err = t.kind == "error";
                let r = egui::Frame::new()
                    .fill(css::PANEL_SOLID)
                    .stroke(Stroke::new(1.0, if err { Color32::from_rgba_unmultiplied(255, 116, 130, 128) } else { css::LINE_STRONG }))
                    .corner_radius(9)
                    .inner_margin(Margin { left: 14, right: 6, top: 8, bottom: 8 })
                    .shadow(egui::Shadow { offset: [0, 10], blur: 30, spread: 0, color: Color32::from_black_alpha(90) })
                    .show(ui, |ui| {
                        ui.set_max_width(560.0);
                        ui.horizontal(|ui| {
                            ui.add(egui::Label::new(RichText::new(&t.text).font(sans(12.5)).color(if err { super::theme::hex(0xffc5cb) } else { css::TEXT })).wrap());
                            if widgets::icon_button(ui, "×").clicked() {
                                dismiss.push(t.id);
                            }
                        });
                    });
                // hovering pauses the timer
                if r.response.hovered() {
                    t.until = t.until.max(now + 1.0);
                }
            }
            toasts.retain(|t| !dismiss.contains(&t.id) && t.until > now);
        });
    });
    if let Some(next) = toasts.iter().map(|t| t.until).reduce(f64::min) {
        ctx.request_repaint_after(std::time::Duration::from_secs_f64((next - now).max(0.05)));
    }
}

pub enum WelcomeAction {
    Brief,
    Open,
    FocusPi,
}

/// The empty-canvas welcome card.
pub fn welcome(ui: &mut Ui, at: Rect) -> Option<WelcomeAction> {
    let mut out = None;
    let w = (at.width() - 40.0).min(460.0);
    egui::Area::new(ui.scope_id().with("welcome")).order(egui::Order::Middle).pivot(Align2::CENTER_CENTER).fixed_pos(Pos2::new(at.center().x, at.top() + at.height() * 0.46)).show(ui.ctx(), |ui| {
        widgets::panel_frame().inner_margin(Margin::same(26)).show(ui, |ui| {
            ui.set_width(w - 52.0);
            ui.spacing_mut().item_spacing.y = 10.0;
            widgets::kicker(ui, "Your next design starts here");
            ui.label(RichText::new("Make the architecture clear.").font(bold(21.0)).color(css::TEXT));
            widgets::para(ui, "Describe your goals to pi. Explore its proposals on the canvas, then choose and defend the design that fits.", 13.0, css::DIM);
            ui.horizontal_wrapped(|ui| {
                for (n, t) in [("01", "Set the brief"), ("02", "Explore options"), ("03", "Defend your design")] {
                    egui::Frame::new().fill(css::RAISED).stroke(Stroke::new(1.0, css::LINE)).corner_radius(12).inner_margin(Margin::symmetric(9, 3)).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            ui.label(RichText::new(n).font(bold(11.5)).color(css::ACCENT));
                            ui.label(RichText::new(t).font(sans(11.5)).color(css::DIM));
                        });
                    });
                }
            });
            ui.horizontal(|ui| {
                if widgets::primary(ui, "Edit brief").clicked() {
                    out = Some(WelcomeAction::Brief);
                }
                if button(ui, "Open design…").clicked() {
                    out = Some(WelcomeAction::Open);
                }
                if button(ui, "Focus pi →").clicked() {
                    out = Some(WelcomeAction::FocusPi);
                }
            });
        });
    });
    out
}

/// Workspace actions (Brief, Timeline, Find, Commands ⌘K).
pub enum Action {
    Brief,
    Timeline,
    Find,
    Commands,
}

pub fn workspace_actions(ui: &mut Ui, brief: bool, timeline: bool, commands: bool, review: bool) -> Option<Action> {
    let mut out = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        if ui.add_enabled_ui(!review, |ui| toggle_button(ui, "Brief", brief)).inner.clicked() {
            out = Some(Action::Brief);
        }
        if ui.add_enabled_ui(!review, |ui| toggle_button(ui, "Timeline", timeline)).inner.clicked() {
            out = Some(Action::Timeline);
        }
        if button(ui, "Find").clicked() {
            out = Some(Action::Find);
        }
        let r = ui.add(
            egui::Button::new({
                let mut job = egui::text::LayoutJob::default();
                job.append("Commands  ", 0.0, egui::TextFormat { font_id: sans(12.0), color: if commands { css::ACCENT } else { css::TEXT }, ..Default::default() });
                job.append("⌘K", 0.0, egui::TextFormat { font_id: mono(10.5), color: css::FAINT, ..Default::default() });
                job
            })
            .fill(if commands { css::ACCENT_SOFT } else { css::RAISED })
            .stroke(Stroke::new(1.0, css::LINE))
            .corner_radius(6)
            .min_size(Vec2::new(0.0, 26.0)),
        );
        if r.clicked() {
            out = Some(Action::Commands);
        }
    });
    out
}

pub fn radius(r: u8) -> CornerRadius {
    CornerRadius::same(r)
}
