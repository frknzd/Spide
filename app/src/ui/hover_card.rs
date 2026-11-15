//! Hover cards (port of `HoverCard.tsx`): what an element is, why it is here, its diagnostics.

use super::state::hover::{place_card, Bounds, Point};
use super::theme::{bold, css, hex, italic, mono, sans};
use super::widgets;
use crate::state::Snapshot;
use egui::{Color32, Margin, RichText, Stroke, Ui};
use spide_core::model::{Design, DecisionState, Status};
use spide_core::metrics::Metrics;
use spide_diagram::focus::footprint_of;
use spide_diagram::insights::{concern_color, evidence_label, finding_color, member_diagnostics, method_marks, MemberDiagnosis};
use spide_diagram::visuals::{attr_text, heat_color, method_text, vis_label};
use std::collections::BTreeSet;

pub struct CardCtx<'a> {
    pub snap: &'a Snapshot,
    pub design: &'a Design,
    pub metrics: &'a Metrics,
}

pub enum CardAction {
    Close,
    /// show another method's facts inside the evidence section
    Select(Option<String>),
}

fn c32(c: spide_diagram::visuals::Rgb) -> Color32 {
    super::theme::col(c, 1.0)
}

fn h(ui: &mut Ui, t: &str) {
    widgets::section(ui, t);
}

fn wrap(ui: &mut Ui, t: impl Into<String>, size: f32, color: Color32) {
    widgets::para(ui, t, size, color);
}

fn rat(ui: &mut Ui, t: &str) {
    ui.add_space(3.0);
    ui.add(egui::Label::new(RichText::new(t).font(italic(12.5)).color(css::DIM)).wrap());
    ui.add_space(3.0);
}

fn line_color(st: Status) -> Color32 {
    match st {
        Status::Proposed => css::GHOST,
        Status::Removing | Status::Pruned => css::FAINT,
        _ => css::TEXT,
    }
}

/// "What is it for?": the agent's purpose, the role Spide reads and why, misplaced methods → home.
fn purpose_section(ui: &mut Ui, name: &str, cx: &CardCtx) {
    let cr = cx.metrics.responsibility.classes.get(name);
    let role = cr.and_then(|c| c.role.as_ref());
    let a = cx.design.assessments.get(name);
    let marks = method_marks(Some(cx.metrics));
    let marks = marks.get(name);
    if role.is_none() && a.is_none() && marks.is_none() {
        return;
    }
    ui.add_space(6.0);
    h(ui, "purpose");
    if let Some(p) = a.and_then(|a| a.purpose.as_ref()) {
        wrap(ui, p, 12.5, css::TEXT);
    }
    if let Some(role) = role {
        let heuristic = role.by_name.clone().or(role.by_stereotype.clone()).or(role.by_layer.clone());
        let agent_note = a.and_then(|a| a.role.as_ref()).map(|r| match &heuristic {
            Some(hh) if hh != r => format!("agent says {r}, names say {hh}"),
            _ => "agent agrees".into(),
        });
        let source = match role.source.as_str() {
            "explicit" => "set explicitly",
            "agent" => "from the agent's assessment",
            "name" => "from its name",
            "stereotype" => "from its stereotype",
            "layer" => "from its layer",
            "default" => "no signal, assumed",
            s => s,
        };
        let mut rest = format!(" · {source}");
        if role.source != "agent" {
            rest += &format!(" ({})", role.evidence);
        }
        if let Some(n) = agent_note {
            rest += &format!(" · {n}");
        }
        if let Some(cr) = cr.filter(|c| c.concerns.len() > 1) {
            rest += &format!(" · does {}", cr.concerns.iter().map(|(k, n)| format!("{} {n}", k.replacen('_', " ", 1))).collect::<Vec<_>>().join(", "));
        }
        widgets::rich(ui, &[(&role.role, bold(12.5), css::TEXT), (&rest, sans(12.5), css::FAINT)]);
    }
    if let Some(marks) = marks {
        let mut ms: Vec<_> = marks.iter().collect();
        ms.sort_by(|a, b| a.0.cmp(b.0));
        for (m, x) in ms {
            let kind = format!(" {}", x.kind.replace('_', " "));
            let home = x.home.as_ref().map(|h| format!(" → {h}")).unwrap_or_default();
            widgets::rich(ui, &[(&format!("{m}()"), mono(12.0), css::WARN), (&kind, sans(12.0), css::FAINT), (&home, bold(12.0), css::WARN)]);
        }
    }
}

/// Spide Project: where the class is in the code and whether it is in sync.
fn code_line(ui: &mut Ui, name: &str, cx: &CardCtx) {
    let Some(r) = cx.snap.project.as_ref().and_then(|p| p.report.as_ref()) else { return };
    let Some(cs) = r.classes.get(name) else { return };
    let status = cs.status.unwrap_or(spide_core::sync::SyncStatus::InSync);
    let place = match (&cs.symbol, &cs.file) {
        (Some(s), Some(f)) => format!("code: {s} · {f}:{}", cs.line.unwrap_or(0)),
        _ => "not in the code yet".to_string(),
    };
    let st = if cs.differences > 0 { format!(" · {} ({})", status.label(), cs.differences) } else { format!(" · {}", status.label()) };
    ui.add_space(3.0);
    widgets::rich(ui, &[(&place, mono(11.5), css::DIM), (&st, sans(11.5), super::project::status_color(status))]);
}

const fn kind_text(k: &str) -> &str {
    k
}

fn relation_kind_text(k: spide_core::model::RelationKind) -> &'static str {
    use spide_core::model::RelationKind::*;
    match k {
        Association => "association",
        Aggregation => "aggregation (whole ◇— part)",
        Composition => "composition (whole ◆— part, lifecycle bound)",
        Inheritance => "inheritance (child ▷ parent)",
        Realization => "realization (implements interface)",
        Dependency => "dependency (uses)",
    }
}

fn proposal_note(ui: &mut Ui, design: &Design, id: Option<&str>) {
    let Some(id) = id else { return };
    let p = design.proposals.iter().find(|x| x.id == id);
    let fp = footprint_of(&design.classes, &design.relations, &design.packages, &BTreeSet::from([id.to_string()]));
    let pkg_of = |n: &str| design.classes.iter().find(|c| c.name == n).and_then(|c| c.package.clone());
    let mut where_: Vec<String> = vec![];
    if let Some(f) = &fp {
        for c in &f.changed {
            if let Some(p) = pkg_of(c) {
                if !where_.contains(&p) {
                    where_.push(p);
                }
            }
        }
    }
    let connects: Vec<String> = fp.as_ref().map(|f| f.anchors.iter().map(|n| match pkg_of(n) {
        Some(p) if !where_.contains(&p) => format!("{n} ({p})"),
        _ => n.clone(),
    }).collect()).unwrap_or_default();
    ui.add_space(6.0);
    let dashed = Color32::from_rgba_unmultiplied(94, 234, 212, 115);
    let r = egui::Frame::new().corner_radius(8).inner_margin(Margin::symmetric(8, 6)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let by = if p.is_some_and(|p| p.source == "file") { " via a design.json edit" } else { " by the agent" };
        widgets::rich(ui, &[("Proposed", bold(12.5), hex(0xb8f5ea)), (by, sans(12.5), hex(0xb8f5ea))]);
        if !where_.is_empty() || !connects.is_empty() {
            let mut runs: Vec<(String, egui::FontId, Color32)> = vec![];
            if !where_.is_empty() {
                runs.push(("in ".into(), sans(12.5), css::TEXT));
                runs.push((where_.join(", "), bold(12.5), css::TEXT));
            }
            if !where_.is_empty() && !connects.is_empty() {
                runs.push((" · ".into(), sans(12.5), css::TEXT));
            }
            if !connects.is_empty() {
                runs.push(("connects to ".into(), sans(12.5), css::TEXT));
                runs.push((connects.join(", "), bold(12.5), css::TEXT));
            }
            let rr: Vec<(&str, egui::FontId, Color32)> = runs.iter().map(|(a, b, c)| (a.as_str(), b.clone(), *c)).collect();
            widgets::rich(ui, &rr);
        }
        if let Some(r) = p.and_then(|p| p.rationale.as_ref()) {
            wrap(ui, r, 12.5, hex(0xb8f5ea));
        }
        if let Some(f) = fp.as_ref().filter(|f| f.items.len() > 1) {
            ui.add_space(5.0);
            h(ui, &format!("this proposal · {} changes", f.items.len()));
            for (sign, text) in f.items.iter().take(8) {
                ui.label(RichText::new(format!("{sign} {text}")).font(mono(11.5)).color(if *sign == '+' { css::GHOST } else { hex(0xff7a85) }));
            }
            if f.items.len() > 8 {
                widgets::small_faint(ui, format!("+{} more", f.items.len() - 8));
            }
        }
        widgets::faint(ui, "Click to review it · during review, click to inspect it or point pi at it");
    });
    let pts = super::paint::rounded_rect_points(r.response.rect, 8.0);
    let mut closed = pts.clone();
    closed.push(pts[0]);
    ui.painter().extend(egui::Shape::dashed_line(&closed, Stroke::new(1.0, dashed), 4.0, 3.0));
}

/// "Why is this here?" — the decision, proposal or person that introduced an element.
pub fn origin_note(ui: &mut Ui, design: &Design, origin: Option<&str>) {
    let Some(origin) = origin else { return };
    ui.add_space(7.0);
    let frame = egui::Frame::new().fill(Color32::from_rgba_unmultiplied(122, 162, 255, 13)).inner_margin(Margin::symmetric(8, 6)).corner_radius(egui::CornerRadius { nw: 0, sw: 0, ne: 8, se: 8 });
    let r = frame.show(ui, |ui| {
        ui.set_width(ui.available_width());
        match origin {
            "user" => {
                ui.label(RichText::new("added by you").font(sans(12.0)).color(hex(0xc3c8d4)));
            }
            "import" => {
                ui.label(RichText::new("imported from code").font(sans(12.0)).color(hex(0xc3c8d4)));
            }
            _ => {
                if let Some(d) = design.decisions.iter().find(|x| x.id == origin) {
                    let opt = d.options.iter().find(|o| Some(&o.id) == d.chosen.as_ref());
                    h(ui, &format!("why it's here · decision {}{}", d.id, if d.state == DecisionState::Overruled { " (overruled)" } else { "" }));
                    wrap(ui, &d.question, 12.0, hex(0xc3c8d4));
                    let chosen = opt.map(|o| o.label.clone()).or(d.custom.clone()).unwrap_or("?".into());
                    widgets::rich(ui, &[("→ ", sans(12.0), hex(0xc3c8d4)), (&chosen, bold(12.0), hex(0xc3c8d4))]);
                    if let Some(def) = d.attempts.last().map(|a| a.defense.as_str()).filter(|s| !s.is_empty()) {
                        let t = if def.chars().count() > 160 { def.chars().take(157).collect::<String>() + "…" } else { def.to_string() };
                        wrap(ui, format!("“{t}”"), 12.0, css::FAINT);
                    }
                    widgets::small_faint(ui, "click for the full decision");
                } else {
                    let entry = design.log.iter().find(|l| l.summary.starts_with(&format!("accepted {origin}:")));
                    let when = entry.map(|e| format!(" · {}", super::format_time(e.at))).unwrap_or_default();
                    widgets::rich(ui, &[(&format!("accepted proposal {origin}"), sans(12.0), hex(0xc3c8d4)), (&when, sans(12.0), css::FAINT)]);
                }
            }
        }
    });
    let rr = r.response.rect;
    ui.painter().vline(rr.left() + 1.0, rr.y_range(), Stroke::new(2.0, css::ACCENT));
}

fn vis_pill(ui: &mut Ui, v: spide_core::model::Visibility) {
    egui::Frame::new().stroke(Stroke::new(1.0, css::ACCENT)).corner_radius(8).inner_margin(Margin::symmetric(6, 0)).show(ui, |ui| {
        ui.label(RichText::new(vis_label(v)).font(sans(10.5)).color(css::ACCENT));
    });
}

/// Every finding on one member: what, how sure, why it matters, what to consider.
fn findings(ui: &mut Ui, d: Option<&MemberDiagnosis>) {
    let Some(d) = d else { return };
    for i in &d.issues {
        let c = c32(finding_color(i.severity * i.confidence));
        ui.add_space(6.0);
        let r = egui::Frame::new().fill(Color32::from_white_alpha(8)).inner_margin(Margin { left: 9, right: 7, top: 5, bottom: 5 }).corner_radius(3).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(format!("{} {}", i.family.icon(), i.family.label())).font(bold(12.5)).color(c));
                ui.label(RichText::new(format!("{} · confidence {}%", evidence_label(&i.basis, i.confidence), (i.confidence * 100.0).round())).font(sans(11.0)).color(css::FAINT));
            });
            wrap(ui, &i.message, 12.5, c);
            if !i.explain.is_empty() {
                wrap(ui, &i.explain, 12.5, css::DIM);
            }
            if let Some(f) = &i.fix_hint {
                wrap(ui, format!("consider: {f}"), 11.5, css::DIM);
            }
            if !i.source.is_empty() {
                widgets::small_faint(ui, &i.source);
            }
        });
        let rr = r.response.rect;
        ui.painter().vline(rr.left() + 1.0, rr.y_range(), Stroke::new(2.0, c));
    }
}

fn metric(ui: &mut Ui, label: &str, value: String, tip: &str) {
    let r = widgets::rich(ui, &[(label, sans(11.5), css::DIM), (" ", sans(11.5), css::DIM), (&value, bold(11.5), css::TEXT)]);
    if !tip.is_empty() {
        r.on_hover_text(tip);
    }
}

fn plural(n: u32, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

fn facts(ui: &mut Ui, m: &spide_core::model::Method) {
    let Some(c) = &m.code else {
        widgets::faint(ui, "Imported code facts unavailable.");
        return;
    };
    wrap(ui, format!("{} code lines (excluding blanks/comments) · {} statements", c.loc, c.statements), 12.5, css::TEXT);
    wrap(ui, format!("{} · {} · longest receiver chain {}", plural(c.loops, "loop", "loops"), plural(c.switches, "switch", "switches"), c.max_chain), 12.5, css::TEXT);
    ui.label(RichText::new(format!("{}:{}–{}", c.file, c.start_line, c.end_line)).font(mono(11.0)).color(css::DIM));
}

/// Facts are deliberately separate from issue judgements. No source facts means unknown, not zero.
fn member_evidence(ui: &mut Ui, member: &spide_core::model::Method, qualified: &str, cx: &CardCtx, selected: &Option<String>, actions: &mut Vec<CardAction>) {
    let mut pairs: Vec<_> = cx.metrics.overlap.pairs.iter().filter(|p| p.a == qualified || p.b == qualified).collect();
    pairs.sort_by(|a, b| b.score.total_cmp(&a.score));
    pairs.truncate(5);
    let find = |key: &str| -> Vec<&spide_core::model::Method> { cx.design.classes.iter().flat_map(|c| c.methods.iter().filter(move |m| format!("{}.{}", c.name, m.name) == key)).collect() };
    egui::CollapsingHeader::new(RichText::new("Code facts & similarity").font(sans(12.5)).color(css::DIM)).id_salt(("evidence", qualified)).show(ui, |ui| {
        facts(ui, member);
        widgets::small_faint(ui, "Similarity covers the loaded/imported model; it is not the probability of a defect.");
        if pairs.is_empty() {
            widgets::faint(ui, "No reported similarity matches.");
        }
        let pct = |v: f64| format!("{}%", (v * 100.0).round());
        for p in &pairs {
            let key = if p.a == qualified { &p.b } else { &p.a };
            let cands = find(key);
            let peer = if cands.len() == 1 { Some(cands[0]) } else { None };
            let code_ok = member.code.as_ref().is_some_and(|c| c.fingerprint.len() >= 6) && peer.and_then(|m| m.code.as_ref()).is_some_and(|c| c.fingerprint.len() >= 6);
            ui.add_space(6.0);
            if ui.add_enabled(peer.is_some(), egui::Button::new(RichText::new(format!("{key}()")).font(sans(12.5)).color(css::ACCENT)).frame(false)).clicked() {
                actions.push(CardAction::Select(Some(key.clone())));
            }
            wrap(ui, format!("Implementation fingerprints: {}", if code_ok { pct(p.code) } else { "unavailable".into() }), 12.5, css::TEXT);
            widgets::small_faint(ui, format!("Name {} · signature {} · data/calls {} · description {}", pct(p.name), pct(p.signature), pct(p.touches), pct(p.text)));
        }
        if let Some(sel) = selected {
            let m = find(sel);
            if m.len() == 1 {
                ui.add_space(6.0);
                ui.label(RichText::new(format!("{sel}()")).font(bold(12.5)).color(css::TEXT));
                facts(ui, m[0]);
                if ui.add(egui::Button::new(RichText::new("Back to matches").font(sans(12.0)).color(css::ACCENT)).frame(false)).clicked() {
                    actions.push(CardAction::Select(None));
                }
            }
        }
    });
}

/// Draw the card body for a hover key. Returns false when there's nothing to show.
pub fn body(ui: &mut Ui, key: &str, cx: &CardCtx, pinned: bool, selected: &Option<String>, actions: &mut Vec<CardAction>) -> bool {
    let Some((kind, rest)) = key.split_once(':') else { return false };
    let design = cx.design;
    let metrics = cx.metrics;
    match kind {
        "class" => {
            // a code-only class (sync lens ghost)
            if !design.classes.iter().any(|x| x.name == rest) {
                let Some(p) = cx.snap.project.as_ref() else { return false };
                let Some(c) = p.phantoms.iter().find(|x| x.name == rest) else { return false };
                widgets::rich(ui, &[(&c.name, bold(14.0), css::TEXT), ("  only in the code", sans(11.0), css::FAINT)]);
                code_line(ui, rest, cx);
                wrap(ui, format!("{} field(s) · {} method(s). Readjust (Project panel) proposes it for the design.", c.attributes.len(), c.methods.len()), 12.0, css::DIM);
                return true;
            }
            let Some(c) = design.classes.iter().find(|x| x.name == rest) else { return false };
            let m = metrics.classes.get(rest).or(cx.snap.ghost_metrics.as_ref().and_then(|g| g.classes.get(rest)));
            let pkg = c.package.as_ref().and_then(|p| design.packages.iter().find(|x| &x.name == p));
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                if let Some(s) = &c.stereotype {
                    ui.label(RichText::new(format!("«{s}»")).font(sans(11.5)).color(css::DIM));
                }
                ui.label(RichText::new(&c.name).font(if c.is_abstract { super::theme::font_id(spide_diagram::visuals::FontSpec::sans(14.0).bold().italic()) } else { bold(14.0) }).color(css::TEXT));
                if let Some(p) = &c.package {
                    let layer = pkg.and_then(|p| p.layer).map(spide_diagram::visuals::layer_name).filter(|l| l != p).map(|l| format!(" · {l}")).unwrap_or_default();
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(format!("{p}{layer}")).font(sans(11.0)).color(css::FAINT));
                    });
                }
            });
            if c.status == Status::Proposed {
                proposal_note(ui, design, c.proposal.as_deref());
            }
            if c.status == Status::Overruled {
                wrap(ui, "⚠ applied by overruling the examiner", 12.5, css::WARN);
            }
            if c.status == Status::Pruned {
                wrap(ui, format!("pruned{} · click to restore", c.feedback.as_ref().map(|f| format!(": “{f}”")).unwrap_or_default()), 12.5, css::WARN);
            }
            if let Some(r) = &c.rationale {
                rat(ui, r);
            }
            code_line(ui, &c.name, cx);
            if c.status != Status::Proposed && !c.external {
                purpose_section(ui, &c.name, cx);
            }
            if c.external {
                widgets::faint(ui, "«external» — a library/system class: drawn compact, coupling to it counts less, not judged");
            }
            if c.status != Status::Proposed {
                origin_note(ui, design, c.origin.as_deref());
            }
            if !c.responsibilities.is_empty() {
                ui.add_space(7.0);
                h(ui, "responsibilities");
                for r in &c.responsibilities {
                    wrap(ui, format!("•  {r}"), 12.5, css::TEXT);
                }
            }
            if !c.attributes.is_empty() {
                ui.add_space(7.0);
                h(ui, "attributes");
                for a in &c.attributes {
                    let t = RichText::new(format!("{}{}", attr_text(a), if a.is_static { "  {static}" } else { "" })).font(mono(11.5)).color(line_color(a.status));
                    ui.label(if matches!(a.status, Status::Removing | Status::Pruned) { t.strikethrough() } else { t });
                }
            }
            if !c.methods.is_empty() {
                ui.add_space(7.0);
                h(ui, "methods");
                for x in &c.methods {
                    let t = RichText::new(method_text(x)).font(mono(11.5)).color(line_color(x.status));
                    ui.label(if matches!(x.status, Status::Removing | Status::Pruned) { t.strikethrough() } else { t });
                    if let Some(d) = &x.description {
                        ui.horizontal(|ui| {
                            ui.add_space(14.0);
                            ui.add(egui::Label::new(RichText::new(d).font(sans(11.0)).color(css::FAINT)).wrap());
                        });
                    }
                }
            }
            if let Some(m) = m {
                ui.add_space(9.0);
                widgets::hline(ui);
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(12.0, 4.0);
                    metric(ui, "fan-in", m.fan_in.to_string(), "classes depending on this one");
                    metric(ui, "fan-out", m.fan_out.to_string(), "classes this one depends on");
                    metric(ui, "I", format!("{:.2}", m.instability), "Ce/(Ca+Ce): 0 = stable, 1 = unstable");
                    metric(ui, "LCOM4", m.lcom.map(|l| l.to_string()).unwrap_or("–".into()), "LCOM4: 1 = cohesive");
                    metric(ui, "members", m.members.to_string(), "");
                    let (r, _) = ui.allocate_exact_size(egui::vec2(26.0, 6.0), egui::Sense::hover());
                    ui.painter().rect_filled(r, 3.0, c32(heat_color(m.heat)));
                });
                if !m.issues.is_empty() {
                    ui.add_space(6.0);
                    for i in &m.issues {
                        wrap(ui, format!("• {i}"), 11.5, css::WARN);
                    }
                }
            }
            let per = member_diagnostics(Some(metrics), &design.classes);
            if let Some(per) = per.get(&c.name).filter(|p| !p.is_empty()) {
                ui.add_space(7.0);
                h(ui, "member findings");
                let mut list: Vec<_> = per.iter().collect();
                list.sort_by(|a, b| a.0.cmp(b.0));
                for (k, d) in list {
                    let name = &k[k.find(':').map(|i| i + 1).unwrap_or(0)..];
                    let cc = c32(finding_color(d.severity));
                    let msgs = d.issues.iter().map(|i| i.message.as_str()).collect::<Vec<_>>().join("; ");
                    widgets::rich(ui, &[(&format!("{} ", d.family.icon()), sans(12.5), cc), (&format!("{name}{}", if k.starts_with("method:") { "()" } else { "" }), mono(12.0), cc), (&format!(" — {msgs}"), sans(12.5), css::FAINT)]);
                }
            }
        }
        "attribute" | "method" => {
            let c = design.classes.iter().filter(|c| rest.starts_with(&format!("{}.", c.name))).max_by_key(|c| c.name.len());
            let Some(owner) = c else { return false };
            let (cls, name) = (owner.name.as_str(), &rest[owner.name.len() + 1..]);
            let diags = member_diagnostics(Some(metrics), &design.classes);
            if kind == "attribute" {
                let Some(a) = c.and_then(|c| c.attributes.iter().find(|x| x.name == name)) else { return false };
                let c = c.unwrap();
                let users: Vec<String> = c.methods.iter().filter(|m| m.uses.contains(&a.name)).map(|m| format!("{}()", m.name)).collect();
                ui.label(RichText::new(attr_text(a)).font(mono(14.0)).strong().color(css::TEXT));
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    widgets::faint(ui, format!("attribute of {cls}"));
                    vis_pill(ui, a.visibility);
                    if a.is_static {
                        widgets::faint(ui, " · static");
                    }
                });
                findings(ui, diags.get(cls).and_then(|p| p.get(&format!("attribute:{name}"))));
                if a.status == Status::Proposed {
                    proposal_note(ui, design, a.proposal.as_deref());
                }
                if a.status != Status::Proposed && a.origin != c.origin {
                    origin_note(ui, design, a.origin.as_deref());
                }
                if !users.is_empty() {
                    ui.add_space(7.0);
                    wrap(ui, format!("used by {}", users.join(", ")), 12.5, css::TEXT);
                }
                widgets::small_faint(ui, "right-click to prune");
            } else {
                let Some(m) = c.and_then(|c| c.methods.iter().find(|x| x.name == name)) else { return false };
                let c = c.unwrap();
                ui.label(RichText::new(method_text(m)).font(mono(14.0)).strong().color(css::TEXT));
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    widgets::faint(ui, format!("method of {cls}"));
                    vis_pill(ui, m.visibility);
                    let mut extra = String::new();
                    if m.is_abstract {
                        extra += " · abstract";
                    }
                    if m.is_static {
                        extra += " · static";
                    }
                    if !extra.is_empty() {
                        widgets::faint(ui, extra);
                    }
                });
                findings(ui, diags.get(cls).and_then(|p| p.get(&format!("method:{name}"))));
                if m.status == Status::Proposed {
                    proposal_note(ui, design, m.proposal.as_deref());
                }
                if m.status != Status::Proposed && m.origin != c.origin {
                    origin_note(ui, design, m.origin.as_deref());
                }
                if let Some(d) = &m.description {
                    rat(ui, d);
                }
                ui.add_space(7.0);
                member_evidence(ui, m, &format!("{cls}.{name}"), cx, selected, actions);
                let mi = metrics.responsibility.methods.iter().find(|x| x.class == cls && x.method == name);
                let marks = method_marks(Some(metrics));
                let mark = marks.get(cls).and_then(|p| p.get(name));
                let av = design.assessments.get(cls).and_then(|a| a.methods.iter().find(|x| x.name == name));
                if let Some(mi) = mi {
                    ui.add_space(7.0);
                    let cc = concern_color(&mi.concern).map(c32).unwrap_or(css::DIM);
                    widgets::rich(ui, &[(&mi.concern.replacen('_', " ", 1), sans(12.5), cc), (&format!(" · {}", mi.rdd), sans(12.5), css::TEXT), (&format!(" — {}", mi.reason), sans(12.5), css::FAINT)]);
                }
                if let Some(av) = av {
                    ui.add_space(7.0);
                    let home = av.home.as_ref().map(|h| format!(" → {h}")).unwrap_or_default();
                    widgets::rich(ui, &[("agent: ", sans(12.5), css::TEXT), (&av.verdict, bold(12.5), css::TEXT), (&home, sans(12.5), css::TEXT), (&format!(" — {}", av.reason), sans(12.5), css::FAINT)]);
                }
                if let Some(home) = mark.and_then(|m| m.home.as_ref()) {
                    ui.add_space(6.0);
                    widgets::rich(ui, &[("suggested home: ", sans(12.5), css::WARN), (home, bold(12.5), css::WARN)]);
                }
                if !m.uses.is_empty() {
                    ui.add_space(7.0);
                    widgets::rich(ui, &[("uses ", sans(12.5), css::TEXT), (&m.uses.join(", "), mono(12.0), css::TEXT)]);
                }
                if !m.calls.is_empty() {
                    ui.add_space(7.0);
                    widgets::rich(ui, &[("calls ", sans(12.5), css::TEXT), (&m.calls.join(", "), mono(12.0), css::TEXT)]);
                }
                widgets::small_faint(ui, if pinned { "Esc or click outside to close" } else { "click to pin · right-click to prune" });
            }
        }
        "relation" => {
            let Some(r) = design.relations.iter().find(|x| x.id == rest) else { return false };
            let cyc = metrics.cycle_edges.contains(&r.id);
            let lv = metrics.layer_violations.contains(&r.id);
            widgets::rich(ui, &[(&r.from, bold(14.0), css::TEXT), (" → ", sans(14.0), css::TEXT), (&r.to, bold(14.0), css::TEXT)]);
            let nav = if r.kind == spide_core::model::RelationKind::Association { if r.directed { ", navigable →" } else { ", bidirectional" } } else { "" };
            widgets::faint(ui, format!("{}{nav}", relation_kind_text(r.kind)));
            if r.from_mult.is_some() || r.to_mult.is_some() {
                ui.add_space(7.0);
                ui.label(RichText::new(format!("{} [{}] — [{}] {}", r.from, r.from_mult.clone().unwrap_or(" ".into()), r.to_mult.clone().unwrap_or(" ".into()), r.to)).font(mono(11.5)).color(css::TEXT));
            }
            if let Some(l) = &r.label {
                ui.add_space(7.0);
                wrap(ui, format!("“{l}”"), 12.5, css::TEXT);
            }
            if r.status == Status::Proposed {
                proposal_note(ui, design, r.proposal.as_deref());
            }
            if let Some(x) = &r.rationale {
                rat(ui, x);
            }
            if r.status != Status::Proposed {
                origin_note(ui, design, r.origin.as_deref());
            }
            if cyc {
                ui.add_space(6.0);
                wrap(ui, "⟲ part of a dependency cycle — break it with an interface, an event, or by moving responsibility", 12.5, css::BAD);
            }
            if lv {
                for i in metrics.issues.iter().filter(|i| i.rule.is_some() && i.targets.contains(&r.id)) {
                    ui.add_space(6.0);
                    wrap(ui, format!("⚠ {}", i.explain), 12.5, css::WARN);
                    if let Some(f) = &i.fix_hint {
                        widgets::small_faint(ui, format!("fix: {f}"));
                    }
                }
            }
            widgets::small_faint(ui, "right-click to prune");
        }
        "implicit" => {
            let Some((from, to)) = rest.split_once('>') else { return false };
            let Some(e) = metrics.implicit_edges.iter().find(|x| x.from == from && x.to == to) else { return false };
            widgets::rich(ui, &[(from, bold(14.0), css::TEXT), (" ⋯▸ ", sans(14.0), css::TEXT), (to, bold(14.0), css::TEXT)]);
            widgets::faint(ui, format!("hidden dependency via {} — not drawn as a relation", e.via));
            if e.in_cycle {
                wrap(ui, "⟲ closes a dependency cycle", 12.5, css::BAD);
            }
        }
        "c4" => {
            let system = &cx.snap.derived.system;
            let container = system.containers.iter().find(|c| c.name == rest);
            let external = system.systems.iter().find(|s| s.name == rest);
            let actor = system.actors.iter().any(|a| a == rest);
            ui.label(RichText::new(rest).font(sans(14.0)).color(css::TEXT));
            let what = match (container, external, actor) {
                (Some(c), _, _) => {
                    if c.datastore {
                        "datastore"
                    } else {
                        "container"
                    }
                }
                (None, Some(_), _) => "external system",
                (None, None, true) => "person",
                _ => "system context",
            };
            widgets::faint(ui, what);
            if let Some(c) = container {
                wrap(ui, format!("{} · {} replica(s) · {} classes", c.technology.clone().unwrap_or_default(), c.replicas, c.classes), 12.5, css::TEXT);
                if let Some(d) = &c.description {
                    wrap(ui, d, 12.5, css::TEXT);
                }
                if !c.packages.is_empty() {
                    h(ui, "packages");
                    wrap(ui, c.packages.join(", "), 12.5, css::TEXT);
                }
            }
            if let Some(d) = external.and_then(|e| e.description.as_ref()) {
                wrap(ui, d, 12.5, css::TEXT);
            }
            if container.is_none() && external.is_none() && !actor {
                wrap(ui, format!("{} containers · {} classes", system.containers.len(), design.classes.len()), 12.5, css::TEXT);
            }
        }
        "package" => {
            let pm = metrics.packages.iter().find(|p| p.name == rest);
            let pk = design.packages.iter().find(|p| p.name == rest);
            ui.horizontal(|ui| {
                ui.label(RichText::new(rest).font(bold(14.0)).color(css::TEXT));
                if let Some(l) = pk.and_then(|p| p.layer) {
                    ui.label(RichText::new(format!("{} layer", spide_diagram::visuals::layer_name(l))).font(sans(11.0)).color(css::FAINT));
                }
            });
            if let Some(d) = pk.and_then(|p| p.description.as_ref()) {
                rat(ui, d);
            }
            if let Some(pm) = pm {
                let deps = |out: bool| -> Vec<&str> {
                    metrics.package_deps.iter().filter(|d| if out { d.from == rest } else { d.to == rest }).map(|d| if out { d.to.as_str() } else { d.from.as_str() }).collect()
                };
                let (uses, used_by) = (deps(true), deps(false));
                wrap(ui, format!("{} class{}", pm.classes, if pm.classes == 1 { "" } else { "es" }), 12.5, css::TEXT);
                if !uses.is_empty() {
                    wrap(ui, format!("depends on {}", uses.join(", ")), 12.0, css::DIM);
                }
                if !used_by.is_empty() {
                    wrap(ui, format!("used by {}", used_by.join(", ")), 12.0, css::DIM);
                }
            }
        }
        "concept" => {
            let Some(c) = design.domain.concepts.iter().find(|x| x.name == rest) else { return false };
            widgets::rich(ui, &[(&c.name, bold(14.0), css::TEXT), ("  domain concept", sans(11.0), css::FAINT)]);
            if let Some(d) = &c.description {
                rat(ui, d);
            }
            for a in &c.attributes {
                let mut t = a.name.clone();
                if let Some(ty) = a.ty.as_deref().filter(|t| !t.is_empty()) {
                    t += &format!(": {ty}");
                }
                if let Some(m) = a.multiplicity.as_deref().filter(|m| !m.is_empty()) {
                    t += &format!(" [{m}]");
                }
                ui.label(RichText::new(t).font(mono(12.0)).color(css::TEXT));
            }
            let links: Vec<String> = design
                .domain
                .associations
                .iter()
                .filter(|a| a.status != Status::Pruned && (a.from == c.name || a.to == c.name))
                .map(|a| format!("{} {} {}", a.from, a.label.as_deref().unwrap_or(a.kind.as_str()), a.to))
                .collect();
            if !links.is_empty() {
                ui.add_space(4.0);
                wrap(ui, links.join(" · "), 12.0, css::DIM);
            }
            if c.status == Status::Pruned {
                wrap(ui, "pruned — click to restore", 12.0, css::FAINT);
            }
        }
        "concept_link" => {
            let Some(a) = design.domain.associations.iter().find(|x| x.id == rest) else { return false };
            let mult = |m: &Option<String>| m.as_deref().map(|m| format!(" [{m}]")).unwrap_or_default();
            widgets::rich(ui, &[(&format!("{}{} — {}{}", a.from, mult(&a.from_mult), a.to, mult(&a.to_mult)), bold(13.0), css::TEXT)]);
            wrap(ui, format!("{}{}", a.kind.as_str(), a.label.as_deref().map(|l| format!(": {l}")).unwrap_or_default()), 12.5, css::DIM);
        }
        "actor" => {
            let Some(a) = design.use_cases.actors.iter().find(|x| x.name == rest) else { return false };
            let ucs: Vec<&str> = design.use_cases.use_cases.iter().filter(|u| u.actors.contains(&a.name)).map(|u| u.name.as_str()).collect();
            widgets::rich(ui, &[(&a.name, bold(14.0), css::TEXT), ("  actor", sans(11.0), css::FAINT)]);
            if let Some(d) = &a.description {
                rat(ui, d);
            }
            if !ucs.is_empty() {
                wrap(ui, ucs.join(" · "), 12.5, css::TEXT);
            }
        }
        "usecase" => {
            let Some(u) = design.use_cases.use_cases.iter().find(|x| x.name == rest) else { return false };
            ui.label(RichText::new(&u.name).font(bold(14.0)).color(css::TEXT));
            if let Some(d) = &u.description {
                rat(ui, d);
            }
            wrap(ui, format!("actors: {}", if u.actors.is_empty() { "–".into() } else { u.actors.join(", ") }), 12.5, css::TEXT);
            if !u.classes.is_empty() {
                widgets::rich(ui, &[("realised by ", sans(12.5), css::TEXT), (&u.classes.join(", "), mono(12.0), css::TEXT)]);
            }
            if !u.includes.is_empty() {
                wrap(ui, format!("«include» {}", u.includes.join(", ")), 12.5, css::TEXT);
            }
            if !u.extends.is_empty() {
                wrap(ui, format!("«extend» {}", u.extends.join(", ")), 12.5, css::TEXT);
            }
            widgets::small_faint(ui, "click for its sequence diagram · right-click to prune");
        }
        "message" => {
            let Some((seq, idx)) = rest.split_once('#') else { return false };
            let s = design.sequences.iter().find(|x| x.name == seq);
            let Some(m) = s.and_then(|s| idx.parse::<usize>().ok().and_then(|i| s.messages.get(i))) else { return false };
            let args = match (&m.args, m.kind) {
                (Some(a), _) => format!("({a})"),
                (None, spide_core::model::MessageKind::Return) => String::new(),
                _ => "()".into(),
            };
            ui.label(RichText::new(format!("{} → {}: {}{args}", m.from, m.to, m.method)).font(mono(14.0)).strong().color(css::TEXT));
            let k = format!("{:?}", m.kind).to_lowercase();
            widgets::faint(ui, format!("{} message{}", kind_text(&k), m.returns.as_ref().map(|r| format!(" · returns {r}")).unwrap_or_default()));
            if let Some(n) = &m.note {
                rat(ui, n);
            }
            if let Some(p) = &m.problem {
                wrap(ui, format!("✗ {p}"), 12.5, css::BAD);
            }
        }
        _ => return false,
    }
    true
}

/// Show the card for `key` near `at`, fitted inside `bounds` (the canvas).
#[allow(clippy::too_many_arguments)]
pub fn show(ctx: &egui::Context, key: &str, at: Point, pinned: bool, bounds: egui::Rect, cx: &CardCtx, selected: &Option<String>, actions: &mut Vec<CardAction>) -> Option<egui::Rect> {
    let id = egui::Id::unique("hovercard");
    // Reserve real vertical space, rather than a larger maximum that short cards shrink out of.
    // 250 px keeps hover cards compact; smaller canvases remain the hard limit.
    let b = Bounds { left: bounds.left(), top: bounds.top(), right: bounds.right(), bottom: bounds.bottom() };
    let pl = place_card(at, (360.0, 250.0), &b);
    let mut shown = true;
    let resp = egui::Area::new(id)
        .order(egui::Order::Tooltip)
        .fixed_pos(egui::pos2(pl.left, pl.top))
        .interactable(pinned)
        .default_size(egui::vec2(360.0, 250.0f32.min(pl.max_height)))
        .show(ctx, |ui| {
            ui.set_height(250.0f32.min(pl.max_height));
            egui::Frame::new()
                .fill(Color32::from_rgba_unmultiplied(29, 32, 38, 250))
                .stroke(Stroke::new(1.0, css::LINE_STRONG))
                .corner_radius(13)
                .inner_margin(Margin::symmetric(14, 12))
                .shadow(egui::Shadow { offset: [0, 14], blur: 50, spread: 0, color: Color32::from_black_alpha(128) })
                .show(ui, |ui| {
                    ui.set_width((360.0f32.min(pl.max_width) - 28.0).max(1.0));
                    ui.spacing_mut().item_spacing.y = 2.0;
                    let max_h = (250.0f32.min(pl.max_height) - 24.0).max(1.0);
                    egui::ScrollArea::vertical().max_height(max_h).auto_shrink([false, false]).show(ui, |ui| {
                        if pinned {
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                                if widgets::icon_button(ui, "×").clicked() {
                                    actions.push(CardAction::Close);
                                }
                            });
                        }
                        shown = body(ui, key, cx, pinned, selected, actions);
                    });
                });
        });
    shown.then_some(resp.response.rect)
}

#[cfg(test)]
#[path = "hover_card_tests.rs"]
mod tests;
