//! Canvas chrome: identity row, workspace actions, view tabs, toolbar (evidence popover,
//! detail slider, zoom group), proposal dock, toasts, welcome and startup cards.

use super::theme::{bold, css, hex, mono, sans};
use super::widgets::{self, button, toggle_button};
use egui::{Align2, Color32, CornerRadius, Margin, Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2};
use spide_core::metrics::{Issue, Metrics};

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
    FocusTarget(String),
    /// Owner decision on a proposed health exception.
    Exception(String, bool),
}

#[derive(Default)]
pub struct HealthState {
    pub open: bool,
    pub pinned: bool,
    close_at: Option<f64>,
}

/// Neutral entry point for findings and unavailable evidence.
pub fn evidence_button(ui: &mut Ui, st: &mut HealthState, m: &Metrics, actions: &mut Vec<HealthAction>) {
    let now = ui.input(|i| i.time);
    let trigger = ui.button(RichText::new("Evidence").font(bold(11.5)).color(css::TEXT));
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
    let anchor = trigger.rect.left_bottom();
    let max_w = (ui.clip_rect().right() - anchor.x - 10.0).clamp(280.0, 470.0);
    let area = egui::Area::new(ui.scope_id().with("health-pop")).order(egui::Order::Foreground).fixed_pos(anchor + Vec2::new(0.0, 8.0)).show(ui.ctx(), |ui| {
        widgets::panel_frame().inner_margin(Margin::symmetric(16, 14)).show(ui, |ui| {
            ui.set_width(max_w - 32.0);
            ui.set_max_height(ui.ctx().content_rect().height() * 0.7);
            evidence_popover(ui, m, actions);
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

fn evidence_popover(ui: &mut Ui, m: &Metrics, actions: &mut Vec<HealthAction>) {
    let r = &m.health_report;
    widgets::section(ui, "Design evidence");
    egui::ScrollArea::vertical().max_height(ui.ctx().content_rect().height() * 0.55).auto_shrink([false, true]).show(ui, |ui| {
        if !r.gaps.is_empty() {
            widgets::section(ui, "Evidence gaps");
            for gap in &r.gaps { ui.label(gap); }
        }
        if !r.runs.is_empty() {
            widgets::section(ui, "Analysis runs");
            for run in &r.runs {
                ui.label(format!("{} ({}) · {}", run.tool, run.language, run.status))
                    .on_hover_text(run.detail.as_deref().unwrap_or(""));
            }
        }
        widgets::section(ui, "Findings");
        if r.signals.is_empty() { widgets::faint(ui, "No findings from available checks. This does not establish design quality."); }
        for signal in &r.signals {
            if let Some((idx, issue)) = m.issues.iter().enumerate().find(|(_, i)| i.kind == signal.rule && i.message == signal.message && i.targets.contains(&signal.target)) {
                if issue.targets.first() == Some(&signal.target) { issue_row(ui, idx, issue, actions); }
            } else {
                ui.horizontal_wrapped(|ui| {
                    widgets::faint(ui, &signal.provenance);
                    if widgets::link_sized(ui, &signal.message, 12.0).on_hover_text(signal.fix_hint.as_deref().unwrap_or(&signal.target)).clicked() {
                        actions.push(HealthAction::FocusTarget(signal.target.clone()));
                    }
                });
            }
        }
        if !r.exceptions.is_empty() { widgets::section(ui, "Exceptions"); }
        for x in &r.exceptions {
            ui.horizontal_wrapped(|ui| {
                let status = if x.active(spide_core::store::now_ms()) { "Approved" } else if x.approved { "Expired" } else { "Proposed" };
                ui.label(format!("{status}: {} on {} — {}", x.rule, x.scope, x.reason));
                if !x.approved {
                    if widgets::link_sized(ui, "approve", 11.0).clicked() { actions.push(HealthAction::Exception(x.id.clone(), true)); }
                    if widgets::link_sized(ui, "reject", 11.0).clicked() { actions.push(HealthAction::Exception(x.id.clone(), false)); }
                }
            });
        }
        if !r.suppressed.is_empty() { widgets::section(ui, "Suppressed findings"); }
        for signal in &r.suppressed {
            widgets::faint(ui, format!("{} · {}", signal.message, signal.suppressed_by.as_deref().unwrap_or("")));
        }
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
            if b(ui, "−", "Zoom out (⌘−)").clicked() {
                out = Some(Zoom::Out);
            }
            let sep = |ui: &mut Ui| {
                let (r, _) = ui.allocate_exact_size(Vec2::new(1.0, 30.0), Sense::hover());
                ui.painter().vline(r.center().x, r.y_range(), Stroke::new(1.0, css::LINE));
            };
            sep(ui);
            if b(ui, " Fit ", "Zoom to fit (⌘9) · double-click canvas").clicked() {
                out = Some(Zoom::Fit);
            }
            sep(ui);
            if b(ui, "+", "Zoom in (⌘=)").clicked() {
                out = Some(Zoom::In);
            }
        });
    });
    out
}

/// "Review proposals (N)" dock button (bottom centre of the canvas).
pub fn proposal_bar(ui: &mut Ui, count: usize) -> bool {
    let b = egui::Button::new(RichText::new(format!("Review {count} change{} on the diagram", if count == 1 { "" } else { "s" })).font(sans(12.0)).color(css::TEXT)).fill(css::PANEL_SOLID).stroke(Stroke::NONE).corner_radius(18).min_size(Vec2::new(0.0, 36.0));
    let r = ui.add(b);
    // dashed ghost border
    let pts = super::paint::rounded_rect_points(r.rect, 18.0);
    let mut closed = pts.clone();
    closed.push(pts[0]);
    ui.painter().extend(egui::Shape::dashed_line(&closed, Stroke::new(1.0, Color32::from_rgba_unmultiplied(94, 234, 212, 140)), 4.0, 3.0));
    r.clicked()
}

pub enum ReviewBar {
    /// show the design before this plan (true) or with it (false)
    Before(bool),
    Fit,
}

/// Floating review controls on the canvas: before/proposed, fit to the changes and a legend.
pub fn review_bar(ui: &mut Ui, before: bool, ready: bool, max_w: f32) -> Option<ReviewBar> {
    let mut out = None;
    widgets::panel_frame().corner_radius(18).inner_margin(Margin::symmetric(8, 5)).show(ui, |ui| {
        ui.set_max_width(max_w.max(200.0));
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            if let Some(b) = widgets::segmented(ui, &before, &[(true, "Before"), (false, "Proposed")]) {
                if b != before {
                    out = Some(ReviewBar::Before(b));
                }
            }
            if ui.add_enabled(ready, egui::Button::new(RichText::new("Fit changes").font(sans(12.0)).color(css::TEXT)).fill(css::RAISED).stroke(Stroke::new(1.0, css::LINE)).corner_radius(6).min_size(Vec2::new(0.0, 26.0))).on_hover_text("Zoom to the classes this plan touches").clicked() {
                out = Some(ReviewBar::Fit);
            }
            if max_w > 560.0 {
                let key = |ui: &mut Ui, c: Color32, t: &str| {
                    let (r, _) = ui.allocate_exact_size(Vec2::splat(9.0), Sense::hover());
                    ui.painter().rect_stroke(r, 2.0, Stroke::new(1.5, c), egui::StrokeKind::Inside);
                    ui.label(RichText::new(t).font(sans(11.0)).color(css::DIM));
                };
                key(ui, css::GHOST, "new / changed");
                key(ui, hex(0xff7a85), "removed");
                ui.label(RichText::new("· click elements to inspect or point pi at them").font(sans(11.0)).color(css::FAINT));
            }
        });
    });
    out
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
    Skip,
    Open,
    FocusPi,
}

/// The empty-canvas welcome card.
pub fn welcome(ui: &mut Ui, at: Rect, brief_open: bool) -> Option<WelcomeAction> {
    let mut out = None;
    let w = (at.width() - 40.0).min(460.0);
    egui::Area::new(ui.scope_id().with("welcome")).order(egui::Order::Middle).pivot(Align2::CENTER_CENTER).fixed_pos(Pos2::new(at.center().x, at.top() + at.height() * 0.46)).show(ui.ctx(), |ui| {
        widgets::panel_frame().inner_margin(Margin::same(26)).show(ui, |ui| {
            ui.set_width(w - 52.0);
            ui.spacing_mut().item_spacing.y = 10.0;
            widgets::kicker(ui, "Your next design starts here");
            ui.label(RichText::new("Make the architecture clear.").font(bold(21.0)).color(css::TEXT));
            widgets::para(ui, "Tell pi what you're building. Proposed changes appear right on the diagram — review them there and approve what fits.", 13.0, css::DIM);
            ui.horizontal_wrapped(|ui| {
                for (n, t) in [("01", "Set the brief"), ("02", "Review on the diagram"), ("03", "Approve")] {
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
                if brief_open && button(ui, "Skip brief").on_hover_text("Start designing right away (/skip)").clicked() {
                    out = Some(WelcomeAction::Skip);
                }
                if button(ui, "Open project…").clicked() {
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
        if toggle_button(ui, "Brief", brief).clicked() {
            out = Some(Action::Brief);
        }
        if ui.add_enabled_ui(!review, |ui| toggle_button(ui, "Timeline", timeline)).inner.on_disabled_hover_text("The timeline opens after the review (⌘T leaves review)").clicked() {
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
