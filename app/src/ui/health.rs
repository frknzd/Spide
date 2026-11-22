//! Dedicated Design Health Inspector panel: overall score & evidence confidence,
//! six-dimension interactive breakdown, brief-force alignment, unified filterable
//! findings with inline focus/fix/exception actions, analysis runs & gaps, and
//! exception lifecycle management.

use super::theme::{bold, css, hex, mono, sans};
use super::widgets::{self, button, icon_button, link_sized, primary_enabled};
use egui::{Color32, Margin, Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2};
use spide_core::health::{DimensionReport, HealthReport, Signal};
use spide_core::metrics::Metrics;
use spide_core::model::Design;
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InspectorTab {
    #[default]
    Findings,
    Challenges,
    Evidence,
    Exceptions,
    Forces,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProvenanceFilter {
    #[default]
    All,
    Rules,
    Structure,
    Code,
    Tools,
    Heuristic,
    Agent,
}

impl ProvenanceFilter {
    const ALL: [ProvenanceFilter; 7] = [
        ProvenanceFilter::All,
        ProvenanceFilter::Rules,
        ProvenanceFilter::Structure,
        ProvenanceFilter::Code,
        ProvenanceFilter::Tools,
        ProvenanceFilter::Heuristic,
        ProvenanceFilter::Agent,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Rules => "Rules",
            Self::Structure => "Structure",
            Self::Code => "Code",
            Self::Tools => "Tools",
            Self::Heuristic => "Heuristic",
            Self::Agent => "Agent",
        }
    }

    fn matches(self, s: &Signal) -> bool {
        match self {
            Self::All => true,
            Self::Rules => s.provenance == "rule" || s.rule_label.is_some(),
            Self::Structure => s.provenance == "structure",
            Self::Code => s.provenance == "code",
            Self::Tools => matches!(s.provenance.as_str(), "lsp" | "linter" | "editor"),
            Self::Heuristic => s.provenance.contains("heuristic"),
            Self::Agent => s.provenance.contains("agent"),
        }
    }
}

#[derive(Default, Clone)]
pub struct ExceptionDraft {
    pub rule: String,
    pub scope: String,
    pub reason: String,
    pub review_when: String,
    pub error: String,
}

#[derive(Default)]
pub struct HealthUi {
    pub open: bool,
    pub tab: InspectorTab,
    pub dimension: Option<String>,
    pub provenance: ProvenanceFilter,
    pub query: String,
    pub expanded: HashSet<String>,
    pub draft: Option<ExceptionDraft>,
}

pub enum HealthInspectorAction {
    Close,
    FocusSignal(Signal),
    FocusTarget(String),
    FixSignal(Signal),
    ChallengeAssumptions,
    CreateException { rule: String, scope: String, reason: String, review_when: Option<String> },
    DecideException(String, bool),
    RunAnalysis,
    OpenBrief,
    TogglePrivateDocs(bool),
}

pub fn score_color(score: Option<u8>) -> Color32 {
    match score {
        None => css::FAINT,
        Some(s) if s >= 80 => css::GOOD,
        Some(s) if s >= 60 => css::WARN,
        Some(_) => css::BAD,
    }
}

fn severity_color(s: &Signal) -> Color32 {
    if s.rule_label.is_some() || s.severity >= 0.65 {
        css::BAD
    } else if s.severity >= 0.30 {
        css::WARN
    } else if s.rule == "disagreement" {
        css::ACCENT
    } else {
        css::DIM
    }
}

fn evidence_badge_color(level: &str) -> Color32 {
    match level {
        "verified" => css::GOOD,
        "partial" => css::ACCENT,
        "structural" => css::WARN,
        _ => css::FAINT,
    }
}

fn force_status_color(status: &str) -> Color32 {
    match status {
        "aligned" => css::GOOD,
        "watch" => css::WARN,
        "at_risk" => css::BAD,
        _ => css::FAINT,
    }
}

fn pill(ui: &mut Ui, text: &str, color: Color32) -> egui::Response {
    egui::Frame::new()
        .stroke(Stroke::new(1.0, Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 150)))
        .fill(Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 22))
        .corner_radius(6)
        .inner_margin(Margin::symmetric(6, 1))
        .show(ui, |ui| {
            ui.label(RichText::new(text).font(sans(10.5)).color(color));
        })
        .response
}

/// The dedicated Design Health Inspector panel (docked left over the canvas).
pub fn panel(
    ctx: &egui::Context,
    area: Rect,
    st: &mut HealthUi,
    design: &Design,
    metrics: &Metrics,
    actions: &mut Vec<HealthInspectorAction>,
) {
    let r = &metrics.health_report;
    let top = Pos2::new(area.left() + 10.0, area.top() + 124.0);
    let w = (area.width() - 20.0).min(430.0);
    let h = (area.bottom() - 10.0 - top.y).max(260.0);

    if st.tab == InspectorTab::Forces && r.forces.is_empty() {
        st.tab = InspectorTab::Findings;
    }

    egui::Area::new(egui::Id::unique("health-inspector"))
        .order(egui::Order::Foreground)
        .fixed_pos(top)
        .show(ctx, |ui| {
            widgets::panel_frame().show(ui, |ui| {
                ui.set_width(w);
                ui.set_height(h);

                // ---- header
                egui::Frame::new().inner_margin(Margin { left: 16, right: 12, top: 14, bottom: 10 }).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 6.0;
                                widgets::kicker(ui, "Design health");
                                let lvl = match r.evidence_level.as_str() {
                                    "verified" => "Verified",
                                    "partial" => "Partial evidence",
                                    "structural" => "Structural only",
                                    _ => "Unassessed",
                                };
                                pill(ui, lvl, evidence_badge_color(&r.evidence_level))
                                    .on_hover_text(format!("Evidence confidence: {:.0}% · {}", r.confidence * 100.0, r.explain));
                            });
                            ui.add_space(2.0);
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 8.0;
                                let sc_col = score_color(r.score);
                                let sc_txt = r.score.map(|s| s.to_string()).unwrap_or_else(|| "—".into());
                                ui.label(RichText::new(sc_txt).font(bold(24.0)).color(sc_col));
                                if r.score.is_some() {
                                    ui.label(RichText::new("/ 100").font(sans(12.0)).color(css::FAINT));
                                }
                                let active_n = r.unique_signals().len();
                                let gap_n = r.gaps.len();
                                let summary = format!(
                                    "{active_n} finding{}{}",
                                    if active_n == 1 { "" } else { "s" },
                                    if gap_n > 0 { format!(" · {gap_n} gap{}", if gap_n == 1 { "" } else { "s" }) } else { String::new() }
                                );
                                ui.label(RichText::new(summary).font(sans(12.0)).color(css::DIM));
                            });
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                            if icon_button(ui, "×").on_hover_text("Close · Esc").clicked() {
                                actions.push(HealthInspectorAction::Close);
                            }
                        });
                    });
                });

                widgets::hline(ui);

                // ---- six-dimension interactive bars
                egui::Frame::new().inner_margin(Margin { left: 14, right: 14, top: 8, bottom: 8 }).show(ui, |ui| {
                    ui.set_width(w - 28.0);
                    dimension_grid(ui, st, &r.dimensions);
                });

                widgets::hline(ui);

                // ---- inspector sub-tabs
                let active_count = r.unique_signals().len();
                let challenge_count = r.challenges.len();
                let gap_count = r.gaps.len();
                let exc_count = r.exceptions.len();
                let force_count = r.forces.len();
                egui::Frame::new().inner_margin(Margin { left: 14, right: 14, top: 8, bottom: 8 }).show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        let mut tab_btn = |t: InspectorTab, label: String| {
                            let on = st.tab == t;
                            let b = egui::Button::new(RichText::new(label).font(sans(11.5)).color(if on { css::ACCENT } else { css::DIM }))
                                .fill(if on { css::ACCENT_SOFT } else { Color32::TRANSPARENT })
                                .stroke(Stroke::new(1.0, if on { Color32::from_rgba_unmultiplied(138, 169, 255, 140) } else { css::LINE }))
                                .corner_radius(8)
                                .min_size(Vec2::new(0.0, 24.0));
                            if ui.add(b).clicked() {
                                st.tab = t;
                            }
                        };
                        tab_btn(InspectorTab::Findings, format!("Findings ({active_count})"));
                        tab_btn(
                            InspectorTab::Challenges,
                            format!("Challenges{}", if challenge_count > 0 { format!(" ({challenge_count})") } else { String::new() }),
                        );
                        tab_btn(InspectorTab::Evidence, format!("Evidence{}", if gap_count > 0 { format!(" ({gap_count})") } else { String::new() }));
                        tab_btn(InspectorTab::Exceptions, format!("Exceptions ({exc_count})"));
                        if force_count > 0 {
                            tab_btn(InspectorTab::Forces, format!("Forces ({force_count})"));
                        }
                    });
                });

                widgets::hline(ui);

                // ---- scrollable body
                egui::ScrollArea::vertical().id_salt("health-inspector-body").auto_shrink([false, false]).show(ui, |ui| {
                    egui::Frame::new().inner_margin(Margin { left: 14, right: 14, top: 10, bottom: 16 }).show(ui, |ui| {
                        ui.set_width(w - 28.0);
                        ui.spacing_mut().item_spacing.y = 6.0;
                        match st.tab {
                            InspectorTab::Findings => findings_tab(ui, st, r, actions),
                            InspectorTab::Challenges => challenges_tab(ui, r, actions),
                            InspectorTab::Evidence => evidence_tab(ui, design, r, actions),
                            InspectorTab::Exceptions => exceptions_tab(ui, r, actions),
                            InspectorTab::Forces => forces_tab(ui, st, r),
                        }
                    });
                });
            });
        });
}

fn dimension_grid(ui: &mut Ui, st: &mut HealthUi, dims: &[DimensionReport]) {
    let col_w = ((ui.available_width() - 8.0) / 2.0).max(120.0);
    egui::Grid::new("health-dim-grid").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
        for (i, d) in dims.iter().enumerate() {
            let selected = st.dimension.as_deref() == Some(d.id.as_str());
            let sc_col = score_color(d.score);
            let fill = if selected { css::ACCENT_SOFT } else { css::SURFACE };
            let stroke = if selected {
                Stroke::new(1.0, Color32::from_rgba_unmultiplied(138, 169, 255, 160))
            } else {
                Stroke::new(1.0, css::LINE)
            };
            let frame = egui::Frame::new().fill(fill).stroke(stroke).corner_radius(7).inner_margin(Margin::symmetric(8, 6)).show(ui, |ui| {
                ui.set_width(col_w - 16.0);
                ui.horizontal(|ui| {
                    let short = d.label.split(" & ").next().unwrap_or(&d.label);
                    ui.label(RichText::new(short).font(sans(11.5)).color(if selected { css::TEXT } else { css::DIM }));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let sc = d.score.map(|s| s.to_string()).unwrap_or_else(|| "—".into());
                        ui.label(RichText::new(sc).font(bold(11.5)).color(sc_col));
                        if d.active_findings > 0 {
                            ui.label(RichText::new(format!("{}·", d.active_findings)).font(sans(10.5)).color(css::FAINT));
                        }
                    });
                });
                ui.add_space(3.0);
                let (bar_rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 4.0), Sense::hover());
                ui.painter().rect_filled(bar_rect, 2.0, css::RAISED2);
                if let Some(sc) = d.score {
                    let fill_w = (bar_rect.width() * (sc as f32 / 100.0)).max(2.0);
                    let filled = Rect::from_min_size(bar_rect.min, Vec2::new(fill_w, bar_rect.height()));
                    ui.painter().rect_filled(filled, 2.0, sc_col);
                }
            });
            let resp = frame.response.interact(Sense::click()).on_hover_text(format!(
                "{}\nBasis: {}\nActive findings: {} · Suppressed: {} · Penalty: −{:.1}",
                d.label, d.basis, d.active_findings, d.suppressed_findings, d.penalty
            ));
            if resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if resp.clicked() {
                if selected {
                    st.dimension = None;
                } else {
                    st.dimension = Some(d.id.clone());
                    st.tab = InspectorTab::Findings;
                }
            }
            if i % 2 == 1 {
                ui.end_row();
            }
        }
    });
}

fn findings_tab(ui: &mut Ui, st: &mut HealthUi, r: &HealthReport, actions: &mut Vec<HealthInspectorAction>) {
    // search + active dimension chip
    ui.horizontal(|ui| {
        let w = ui.available_width() - if st.dimension.is_some() { 82.0 } else { 0.0 };
        ui.add(
            egui::TextEdit::singleline(&mut st.query)
                .hint_text(RichText::new("Filter findings, classes, rules…").color(css::FAINT))
                .font(sans(12.0))
                .desired_width(w.max(120.0))
                .margin(Margin::symmetric(8, 5))
                .background_color(css::BG),
        );
        if st.dimension.is_some() && button(ui, "All dims").clicked() {
            st.dimension = None;
        }
    });

    // provenance filter chips
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
        for pf in ProvenanceFilter::ALL {
            let on = st.provenance == pf;
            let b = egui::Button::new(RichText::new(pf.label()).font(sans(10.5)).color(if on { css::ACCENT } else { css::DIM }))
                .fill(if on { css::ACCENT_SOFT } else { Color32::TRANSPARENT })
                .stroke(Stroke::new(1.0, if on { Color32::from_rgba_unmultiplied(138, 169, 255, 140) } else { css::LINE }))
                .corner_radius(10)
                .min_size(Vec2::new(0.0, 22.0));
            if ui.add(b).clicked() {
                st.provenance = pf;
            }
        }
    });

    // inline exception creator if open
    if let Some(mut draft) = st.draft.take() {
        let mut keep = true;
        egui::Frame::new()
            .fill(css::RAISED)
            .stroke(Stroke::new(1.0, Color32::from_rgba_unmultiplied(138, 169, 255, 140)))
            .corner_radius(8)
            .inner_margin(Margin::same(10))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = 5.0;
                widgets::kicker_colored(ui, "Add health exception", css::ACCENT);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Rule").font(sans(11.5)).color(css::DIM));
                    ui.label(RichText::new(&draft.rule).font(mono(11.5)).color(css::TEXT));
                });
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Scope").font(sans(11.5)).color(css::DIM));
                    ui.add(egui::TextEdit::singleline(&mut draft.scope).font(mono(11.5)).desired_width(ui.available_width()).background_color(css::BG));
                });
                ui.add(
                    egui::TextEdit::singleline(&mut draft.reason)
                        .hint_text(RichText::new("Why does this rule not apply here?").color(css::FAINT))
                        .font(sans(12.0))
                        .desired_width(f32::INFINITY)
                        .background_color(css::BG),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut draft.review_when)
                        .hint_text(RichText::new("Re-review when… (optional)").color(css::FAINT))
                        .font(sans(11.5))
                        .desired_width(f32::INFINITY)
                        .background_color(css::BG),
                );
                if !draft.error.is_empty() {
                    ui.label(RichText::new(&draft.error).font(sans(11.0)).color(hex(0xffb0b8)));
                }
                ui.horizontal(|ui| {
                    if primary_enabled(ui, draft.reason.trim().len() >= 5 && !draft.scope.trim().is_empty(), "Save exception").clicked() {
                        actions.push(HealthInspectorAction::CreateException {
                            rule: draft.rule.clone(),
                            scope: draft.scope.trim().to_string(),
                            reason: draft.reason.trim().to_string(),
                            review_when: (!draft.review_when.trim().is_empty()).then(|| draft.review_when.trim().to_string()),
                        });
                        keep = false;
                    }
                    if button(ui, "Cancel").clicked() {
                        keep = false;
                    }
                });
            });
        if keep {
            st.draft = Some(draft);
        }
    }

    let q = st.query.trim().to_lowercase();
    let all = r.unique_signals();
    let filtered: Vec<&Signal> = all
        .into_iter()
        .filter(|s| st.dimension.as_deref().is_none_or(|d| s.dimension == d))
        .filter(|s| st.provenance.matches(s))
        .filter(|s| {
            q.is_empty()
                || s.message.to_lowercase().contains(&q)
                || s.rule.to_lowercase().contains(&q)
                || s.entity.to_lowercase().contains(&q)
                || s.target.to_lowercase().contains(&q)
                || s.dimension.to_lowercase().contains(&q)
        })
        .collect();

    if filtered.is_empty() {
        ui.add_space(6.0);
        if r.signals.is_empty() {
            widgets::para(ui, "No active findings from available checks. Check the Evidence tab for coverage and gaps.", 12.0, css::DIM);
        } else {
            widgets::faint(ui, "No findings match the current filter.");
        }
        return;
    }

    let mut current_tier: Option<&str> = None;
    for s in filtered {
        if current_tier != Some(s.tier.as_str()) {
            current_tier = Some(s.tier.as_str());
            ui.add_space(4.0);
            let tier_help = match s.tier.as_str() {
                "root_cause" => "Tackle first — resolving these removes cascades of downstream issues.",
                "simplify" => "Delete or collapse unnecessary structure before polishing.",
                "refactor" => "Targeted smells and pattern improvements.",
                _ => "Hygiene, documentation, and style.",
            };
            ui.horizontal(|ui| {
                widgets::section(ui, spide_core::health::tier_label(&s.tier));
                ui.label(RichText::new(tier_help).font(sans(10.5)).color(css::FAINT));
            });
        }
        let rc_key = if s.root_cause.is_empty() { s.message.clone() } else { s.root_cause.clone() };
        let expanded = st.expanded.contains(&rc_key);
        let sc = severity_color(s);
        egui::Frame::new()
            .fill(css::SURFACE)
            .stroke(Stroke::new(1.0, css::LINE))
            .corner_radius(8)
            .inner_margin(Margin::symmetric(10, 8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = 4.0;
                // top metadata row
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    pill(ui, &s.rule.replace('_', " "), sc);
                    if s.related_count >= 2 {
                        pill(ui, &format!("Hotspot +{}", s.related_count), css::WARN)
                            .on_hover_text(format!("{} other active findings also touch {}", s.related_count, s.entity));
                    }
                    let dim_short = spide_core::health::dimension_label(&s.dimension).split(" & ").next().unwrap_or(&s.dimension);
                    ui.label(RichText::new(dim_short).font(sans(10.5)).color(css::FAINT));
                    let prov = if s.provenance == "structure" || s.provenance == "rule" {
                        s.provenance.clone()
                    } else {
                        format!("{} · {}%", s.provenance, (s.confidence * 100.0).round())
                    };
                    ui.label(RichText::new(prov).font(sans(10.5)).color(css::FAINT));
                    if s.penalty >= 0.1 {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(RichText::new(format!("−{:.1}", s.penalty)).font(mono(10.5)).color(sc))
                                .on_hover_text("Points deducted from this dimension");
                        });
                    }
                });

                // message (click to expand details)
                let msg = ui.add(egui::Label::new(RichText::new(&s.message).font(sans(12.5)).color(css::TEXT)).sense(Sense::click()).wrap());
                if msg.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if msg.clicked() {
                    if expanded {
                        st.expanded.remove(&rc_key);
                    } else {
                        st.expanded.insert(rc_key.clone());
                    }
                }

                // location / target line
                let loc = match (&s.file, s.line) {
                    (Some(f), Some(l)) => format!("{} · {f}:{l}", s.target),
                    (Some(f), None) => format!("{} · {f}", s.target),
                    _ => s.target.clone(),
                };
                ui.label(RichText::new(loc).font(mono(10.5)).color(css::FAINT));

                if expanded {
                    if !s.explain.is_empty() {
                        widgets::para(ui, &s.explain, 11.5, css::DIM);
                    }
                    if let Some(pat) = &s.pattern_hint {
                        widgets::para(ui, pat, 11.5, css::GHOST);
                    }
                    if let Some(fix) = &s.fix_hint {
                        widgets::para(ui, format!("Consider: {fix}"), 11.5, hex(0xb8d0ff));
                    }
                    if !s.source.is_empty() {
                        widgets::small_faint(ui, format!("Source: {}", s.source));
                    }
                }

                // actions row
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 12.0;
                    if link_sized(ui, "Focus", 11.0).clicked() {
                        actions.push(HealthInspectorAction::FocusSignal(s.clone()));
                    }
                    if link_sized(ui, if s.tier == "simplify" { "Simplify with agent" } else { "Fix with agent" }, 11.0).clicked() {
                        actions.push(HealthInspectorAction::FixSignal(s.clone()));
                    }
                    if link_sized(ui, "Exempt…", 11.0).clicked() {
                        st.draft = Some(ExceptionDraft {
                            rule: s.rule.clone(),
                            scope: if s.entity.is_empty() { s.target.clone() } else { s.entity.clone() },
                            reason: String::new(),
                            review_when: String::new(),
                            error: String::new(),
                        });
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if link_sized(ui, if expanded { "Less" } else { "Details" }, 10.5).clicked() {
                            if expanded {
                                st.expanded.remove(&rc_key);
                            } else {
                                st.expanded.insert(rc_key);
                            }
                        }
                    });
                });
            });
    }
}

fn challenges_tab(ui: &mut Ui, r: &HealthReport, actions: &mut Vec<HealthInspectorAction>) {
    widgets::para(
        ui,
        "Question self-imposed constraints, heavy presets, and abstractions that may add complexity without earning their keep.",
        12.0,
        css::DIM,
    );
    ui.horizontal(|ui| {
        if primary_enabled(ui, true, "Challenge assumptions with agent")
            .on_hover_text("Ask pi to critique your brief, rules, and abstractions and propose concrete simplifications")
            .clicked()
        {
            actions.push(HealthInspectorAction::ChallengeAssumptions);
        }
    });
    ui.add_space(6.0);
    if r.challenges.is_empty() {
        widgets::faint(ui, "No automated assumption flags triggered right now. Use the button above for a deeper review with pi.");
        return;
    }
    for ch in &r.challenges {
        egui::Frame::new()
            .fill(css::SURFACE)
            .stroke(Stroke::new(1.0, css::LINE))
            .corner_radius(8)
            .inner_margin(Margin::symmetric(10, 8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = 4.0;
                ui.horizontal_wrapped(|ui| {
                    pill(ui, &ch.kind, css::ACCENT);
                    ui.label(RichText::new(&ch.title).font(bold(12.5)).color(css::TEXT));
                });
                widgets::para(ui, &ch.question, 12.0, css::TEXT);
                widgets::small_faint(ui, &ch.evidence);
                if !ch.targets.is_empty() {
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        for t in ch.targets.iter().take(4) {
                            if link_sized(ui, &format!("Focus {t}"), 10.5).clicked() {
                                actions.push(HealthInspectorAction::FocusTarget(t.clone()));
                            }
                        }
                    });
                }
            });
    }
}

fn evidence_tab(ui: &mut Ui, design: &Design, r: &HealthReport, actions: &mut Vec<HealthInspectorAction>) {
    ui.horizontal(|ui| {
        if button(ui, "Run analysis").on_hover_text("Run configured language servers and linters").clicked() {
            actions.push(HealthInspectorAction::RunAnalysis);
        }
        if button(ui, "Brief & rules").on_hover_text("Edit architecture style, forbidden rules, thresholds, and change scenarios").clicked() {
            actions.push(HealthInspectorAction::OpenBrief);
        }
    });

    ui.add_space(4.0);
    let mut priv_doc = design.rules.private_documentation;
    if ui.checkbox(&mut priv_doc, "Check private-method documentation").changed() {
        actions.push(HealthInspectorAction::TogglePrivateDocs(priv_doc));
    }
    widgets::small_faint(ui, format!("Architecture preset: {} · {}", design.rules.preset.as_str(), design.rules.preset.describe()));

    ui.add_space(6.0);
    widgets::section(ui, "Evidence gaps");
    if r.gaps.is_empty() {
        widgets::faint(ui, "No evidence gaps detected.");
    } else {
        for gap in &r.gaps {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("⚠").font(sans(11.5)).color(css::WARN));
                ui.add(egui::Label::new(RichText::new(gap).font(sans(12.0)).color(css::TEXT)).wrap());
            });
        }
    }

    ui.add_space(6.0);
    widgets::section(ui, "Analysis runs");
    if r.runs.is_empty() {
        widgets::faint(ui, "No language server or linter runs recorded yet.");
    } else {
        for run in &r.runs {
            let col = match run.status.as_str() {
                "ran" => css::GOOD,
                "missing" | "not_configured" => css::WARN,
                _ => css::BAD,
            };
            ui.horizontal(|ui| {
                ui.label(RichText::new(&run.tool).font(mono(11.5)).color(css::TEXT));
                ui.label(RichText::new(format!("({})", run.language)).font(sans(11.0)).color(css::FAINT));
                pill(ui, &run.status, col).on_hover_text(run.detail.as_deref().unwrap_or(""));
                if run.ran() {
                    ui.label(RichText::new(format!("{} findings", run.findings)).font(sans(11.0)).color(css::DIM));
                }
            });
        }
    }

    if !r.languages.is_empty() || !r.profiles.is_empty() {
        ui.add_space(6.0);
        widgets::section(ui, "Languages & profiles");
        if !r.languages.is_empty() {
            widgets::para(ui, format!("Languages: {}", r.languages.join(", ")), 11.5, css::DIM);
        }
        if !r.profiles.is_empty() {
            widgets::para(ui, format!("Active profiles: {}", r.profiles.join(", ")), 11.5, css::DIM);
        }
    }
}

fn exceptions_tab(ui: &mut Ui, r: &HealthReport, actions: &mut Vec<HealthInspectorAction>) {
    let now = spide_core::store::now_ms();
    if r.exceptions.is_empty() {
        widgets::para(ui, "No exceptions yet. Click “Exempt…” on any finding to record why a rule does not apply.", 12.0, css::DIM);
    } else {
        widgets::section(ui, "Exceptions");
        for x in &r.exceptions {
            let (status, col) = if x.active(now) {
                ("Active", css::GOOD)
            } else if x.approved {
                ("Expired", css::FAINT)
            } else {
                ("Pending", css::WARN)
            };
            egui::Frame::new()
                .fill(css::SURFACE)
                .stroke(Stroke::new(1.0, css::LINE))
                .corner_radius(8)
                .inner_margin(Margin::symmetric(10, 8))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        pill(ui, status, col);
                        ui.label(RichText::new(format!("{} on {}", x.rule, x.scope)).font(mono(11.5)).color(css::TEXT));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(RichText::new(&x.id).font(mono(10.5)).color(css::FAINT));
                        });
                    });
                    widgets::para(ui, &x.reason, 12.0, css::DIM);
                    if let Some(rw) = &x.review_when {
                        widgets::small_faint(ui, format!("Re-review when: {rw}"));
                    }
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 10.0;
                        if !x.approved {
                            if link_sized(ui, "Approve", 11.0).clicked() {
                                actions.push(HealthInspectorAction::DecideException(x.id.clone(), true));
                            }
                            if link_sized(ui, "Reject", 11.0).clicked() {
                                actions.push(HealthInspectorAction::DecideException(x.id.clone(), false));
                            }
                        } else if link_sized(ui, "Revoke", 11.0).clicked() {
                            actions.push(HealthInspectorAction::DecideException(x.id.clone(), false));
                        }
                    });
                });
        }
    }

    let suppressed = r.unique_suppressed();
    if !suppressed.is_empty() {
        ui.add_space(8.0);
        widgets::section(ui, &format!("Suppressed findings ({})", suppressed.len()));
        for s in suppressed {
            widgets::faint(ui, format!("{} · {} ({})", s.rule, s.message, s.suppressed_by.as_deref().unwrap_or("")));
        }
    }
}

fn forces_tab(ui: &mut Ui, st: &mut HealthUi, r: &HealthReport) {
    widgets::small_faint(ui, "How current structural & scenario evidence aligns with the brief's forces.");
    for f in &r.forces {
        let col = force_status_color(&f.status);
        egui::Frame::new()
            .fill(css::SURFACE)
            .stroke(Stroke::new(1.0, css::LINE))
            .corner_radius(8)
            .inner_margin(Margin::symmetric(10, 8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let prio = match f.priority {
                        1 => "MUST",
                        2 => "SHOULD",
                        _ => "NICE",
                    };
                    ui.label(RichText::new(prio).font(sans(10.0)).color(if f.priority == 1 { css::WARN } else { css::FAINT }));
                    ui.add(egui::Label::new(RichText::new(&f.force).font(bold(12.5)).color(css::TEXT)).wrap());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        pill(ui, &f.status.replace('_', " "), col);
                    });
                });
                widgets::para(ui, &f.summary, 11.5, css::DIM);
                if !f.related_dimensions.is_empty() {
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        for dim in &f.related_dimensions {
                            let label = spide_core::health::dimension_label(dim).split(" & ").next().unwrap_or(dim);
                            if link_sized(ui, &format!("Inspect {label}"), 10.5).clicked() {
                                st.dimension = Some(dim.clone());
                                st.tab = InspectorTab::Findings;
                            }
                        }
                    });
                }
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::tests::{harness, project, steps};

    #[test]
    fn health_inspector_opens_via_command_and_closes_brief() {
        let dir = project("library");
        let mut h = harness(dir.path(), (1440.0, 900.0));
        steps(&mut h, 5);
        h.state_mut().brief = true;
        steps(&mut h, 2);
        h.state_mut().command("/health");
        steps(&mut h, 4);
        assert!(h.state().health.open);
        assert!(!h.state().brief, "opening health inspector closes brief");
        let report = &h.state().snap.as_ref().unwrap().metrics.health_report;
        assert_eq!(report.version, "health-2");
        assert!(report.score.is_some());
        assert_eq!(report.dimensions.len(), 6);
        // Switch through inspector tabs without panicking
        for tab in [InspectorTab::Findings, InspectorTab::Challenges, InspectorTab::Evidence, InspectorTab::Exceptions, InspectorTab::Forces] {
            h.state_mut().health.tab = tab;
            steps(&mut h, 2);
        }
        h.state_mut().health.open = false;
        steps(&mut h, 2);
        assert!(!h.state().health.open);
    }
}
