//! Find in Design (⌘F), Go to Element (⇧⌘O) and back/forward navigation.
//! Workspace-wide design, decision and indexed-source search.

use super::chrome::Tab;
use super::theme::{bold, css, mono, sans};
use super::widgets;
use egui::{Key, Margin, RichText, Vec2};
use spide_core::model::{Design, Status};
use spide_core::Target;

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub label: String,
    /// group heading: Classes, Members, Packages, …
    pub kind: &'static str,
    /// where it lives (package, owning class, …)
    pub detail: String,
    pub tab: Tab,
    pub target: Option<Target>,
    pub destination: Option<Destination>,
    /// Full-text content, separate from compact result metadata.
    pub body: String,
    pub status: Status,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Destination {
    Decision(String),
    Code { class: String, method: Option<String> },
    Brief,
}

impl Item {
    pub fn pending(&self) -> bool {
        matches!(self.status, Status::Proposed | Status::Removing)
    }
    pub fn pruned(&self) -> bool {
        self.status == Status::Pruned
    }
}

/// Every element a user can navigate to, in view order.
pub fn index(d: &Design) -> Vec<Item> {
    let mut out = vec![];
    let mut push = |label: &str, kind, detail: String, tab, target, status| out.push(Item { label: label.to_string(), kind, detail, tab, target: Some(target), destination: None, body: String::new(), status });
    for c in &d.classes {
        push(&c.name, "Classes", c.package.clone().unwrap_or_default(), Tab::Class, Target::Class { name: c.name.clone() }, c.status);
    }
    for c in &d.classes {
        for a in &c.attributes {
            push(&a.name, "Members", format!("{} · attribute", c.name), Tab::Class, Target::Attribute { class: c.name.clone(), name: a.name.clone() }, a.status);
        }
        for m in &c.methods {
            push(&format!("{}()", m.name), "Members", format!("{} · method", c.name), Tab::Class, Target::Method { class: c.name.clone(), name: m.name.clone() }, m.status);
        }
    }
    for p in &d.packages {
        push(&p.name, "Packages", p.layer.map(|l| format!("{l:?}").to_lowercase()).unwrap_or_default(), Tab::Package, Target::Package { name: p.name.clone() }, p.status);
    }
    for c in &d.domain.concepts {
        push(&c.name, "Domain concepts", String::new(), Tab::Domain, Target::Concept { name: c.name.clone() }, c.status);
    }
    for s in &d.systems {
        push(&s.name, "System (C4)", "external system".into(), Tab::System, Target::System { name: s.name.clone() }, s.status);
    }
    for c in &d.containers {
        push(&c.name, "System (C4)", "container".into(), Tab::System, Target::Container { name: c.name.clone() }, c.status);
    }
    for a in &d.use_cases.actors {
        push(&a.name, "Use cases", "actor".into(), Tab::Usecase, Target::Actor { name: a.name.clone() }, a.status);
    }
    for u in &d.use_cases.use_cases {
        push(&u.name, "Use cases", "use case".into(), Tab::Usecase, Target::UseCase { name: u.name.clone() }, u.status);
    }
    for s in &d.sequences {
        push(&s.name, "Sequences", format!("{} messages", s.messages.len()), Tab::Sequence, Target::Sequence { name: s.name.clone() }, s.status);
    }
    // Index descriptions, signatures, responsibilities and other model metadata.
    for item in &mut out {
        let value = match item.target.as_ref() {
            Some(Target::Class { name }) => d.class(name).and_then(|v| serde_json::to_string(v).ok()),
            Some(Target::Method { class, name }) => d.class(class).and_then(|c| c.methods.iter().find(|m| &m.name == name)).and_then(|v| serde_json::to_string(v).ok()),
            Some(Target::Attribute { class, name }) => d.class(class).and_then(|c| c.attributes.iter().find(|m| &m.name == name)).and_then(|v| serde_json::to_string(v).ok()),
            Some(Target::Package { name }) => d.packages.iter().find(|v| &v.name == name).and_then(|v| serde_json::to_string(v).ok()),
            Some(Target::Concept { name }) => d.domain.concepts.iter().find(|v| &v.name == name).and_then(|v| serde_json::to_string(v).ok()),
            Some(Target::System { name }) => d.systems.iter().find(|v| &v.name == name).and_then(|v| serde_json::to_string(v).ok()),
            Some(Target::Container { name }) => d.containers.iter().find(|v| &v.name == name).and_then(|v| serde_json::to_string(v).ok()),
            Some(Target::Actor { name }) => d.use_cases.actors.iter().find(|v| &v.name == name).and_then(|v| serde_json::to_string(v).ok()),
            Some(Target::UseCase { name }) => d.use_cases.use_cases.iter().find(|v| &v.name == name).and_then(|v| serde_json::to_string(v).ok()),
            Some(Target::Sequence { name }) => d.sequences.iter().find(|v| &v.name == name).and_then(|v| serde_json::to_string(v).ok()),
            _ => None,
        };
        item.body = value.unwrap_or_default().to_lowercase();
    }
    out
}

/// Build off the UI thread. Only indexed project sources are read, never arbitrary files.
pub fn workspace_index(d: &Design, code: Option<&Design>, root: &std::path::Path) -> Vec<Item> {
    let mut items = index(d);
    for decision in &d.decisions {
        items.push(Item { label: format!("{} · {}", decision.id, decision.question), kind: "Decisions", detail: format!("{:?}", decision.state), tab: Tab::Class, target: None, destination: Some(Destination::Decision(decision.id.clone())), body: serde_json::to_string(decision).unwrap_or_default().to_lowercase(), status: Status::Accepted });
    }
    for force in &d.forces {
        items.push(Item { label: force.name.clone(), kind: "Brief", detail: force.kind.as_str().into(), tab: Tab::Class, target: Some(Target::Force { name: force.name.clone() }), destination: Some(Destination::Brief), body: serde_json::to_string(force).unwrap_or_default().to_lowercase(), status: force.status });
    }
    for scenario in &d.change_scenarios {
        items.push(Item { label: scenario.name.clone(), kind: "Change scenarios", detail: "Brief & rules".into(), tab: Tab::Class, target: None, destination: Some(Destination::Brief), body: serde_json::to_string(scenario).unwrap_or_default().to_lowercase(), status: scenario.status });
    }
    items.push(Item { label: "Architecture rules".into(), kind: "Rules", detail: "Brief & rules".into(), tab: Tab::Class, target: None, destination: Some(Destination::Brief), body: serde_json::to_string(&d.rules).unwrap_or_default().to_lowercase(), status: Status::Accepted });
    for relation in &d.relations {
        items.push(Item { label: format!("{} → {}", relation.from, relation.to), kind: "Relations", detail: relation.label.clone().unwrap_or_default(), tab: Tab::Class, target: Some(Target::Class { name: relation.from.clone() }), destination: None, body: serde_json::to_string(relation).unwrap_or_default().to_lowercase(), status: relation.status });
    }
    let root = root.canonicalize().ok();
    let mut files = std::collections::BTreeMap::new();
    if let Some(code) = code {
        for c in &code.classes {
            let mut add = |label: String, method: Option<String>, source: Option<&spide_core::model::SourceSymbol>, metadata: String| {
                let mut body = metadata;
                let mut detail = c.name.clone();
                if let Some(source) = source {
                    detail = format!("{}:{}", source.file, source.start_line);
                    if let Some(root) = &root {
                        let text = files.entry(source.file.clone()).or_insert_with(|| {
                            let path = root.join(&source.file).canonicalize().ok()?;
                            if !path.starts_with(root) || std::fs::metadata(&path).ok()?.len() > 2_000_000 { return None; }
                            std::fs::read_to_string(path).ok()
                        });
                        if let Some(text) = text {
                            let start = source.start_line.saturating_sub(1) as usize;
                            let length = source.end_line.saturating_sub(source.start_line) as usize + 1;
                            body.push_str(&text.lines().skip(start).take(length).collect::<Vec<_>>().join("\n"));
                        }
                    }
                }
                items.push(Item { label, kind: "Code", detail, tab: Tab::Class, target: None, destination: Some(Destination::Code { class: c.name.clone(), method }), body: body.to_lowercase(), status: Status::Accepted });
            };
            add(c.name.clone(), None, c.source.as_ref(), serde_json::to_string(c).unwrap_or_default());
            for m in &c.methods {
                add(format!("{}.{}", c.name, m.name), Some(m.name.clone()), m.source.as_ref(), serde_json::to_string(m).unwrap_or_default());
            }
        }
    }
    items
}

/// Match quality (higher is better): exact, prefix, word prefix, substring, then in-order letters.
pub fn score(label: &str, query: &str) -> Option<u32> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Some(1);
    }
    let l = label.trim_end_matches("()").to_lowercase();
    if l == q {
        return Some(1000);
    }
    if l.starts_with(&q) {
        return Some(800 - l.len().min(200) as u32);
    }
    // CamelCase / word starts: "Repo" in "LoanRepository"
    let words: Vec<usize> = label.char_indices().filter(|(i, c)| *i == 0 || c.is_uppercase() || label[..*i].ends_with(['_', ' ', '.'])).map(|(i, _)| i).collect();
    if words.iter().any(|&i| label[i..].to_lowercase().starts_with(&q)) {
        return Some(600);
    }
    if l.contains(&q) {
        return Some(400);
    }
    // fuzzy: every query letter in order
    let mut it = l.chars();
    if q.chars().filter(|c| !c.is_whitespace()).all(|c| it.any(|x| x == c)) {
        return Some(100);
    }
    None
}

/// Results for a query, best first, filtered by the toggles.
pub fn search<'a>(items: &'a [Item], query: &str, pruned: bool, pending: bool) -> Vec<&'a Item> {
    let mut v: Vec<(u32, usize, &Item)> = items
        .iter()
        .enumerate()
        .filter(|(_, i)| (pruned || !i.pruned()) && (pending || !i.pending()))
        .filter_map(|(n, i)| score(&i.label, query).or_else(|| score(&i.detail, query).map(|s| s / 4)).or_else(|| (!query.trim().is_empty() && i.body.contains(&query.trim().to_lowercase())).then_some(50)).map(|s| (s, n, i)))
        .collect();
    if query.trim().is_empty() {
        // outline: view order
        v.sort_by_key(|(_, n, _)| *n);
    } else {
        v.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    }
    v.into_iter().map(|(_, _, i)| i).collect()
}

#[derive(Debug, Clone, Default)]
pub struct FindUi {
    pub query: String,
    pub loading: bool,
    pub active: usize,
    pub pruned: bool,
    pub pending: bool,
    /// Go to Element: the outline (everything, in view order) before typing
    pub outline: bool,
    focused: bool,
}

impl FindUi {
    pub fn new(outline: bool) -> Self {
        Self { pending: true, outline, ..Default::default() }
    }
}

pub enum FindOut {
    Go(Item),
    Close,
}

pub fn show(ctx: &egui::Context, f: &mut FindUi, items: &[Item]) -> Option<FindOut> {
    let mut out = None;
    let w = (ctx.content_rect().width() - 40.0).min(560.0);
    egui::Area::new(egui::Id::unique("find-in-design")).order(egui::Order::Foreground).anchor(egui::Align2::CENTER_TOP, Vec2::new(0.0, 70.0)).show(ctx, |ui| {
        widgets::panel_frame().inner_margin(Margin::same(10)).show(ui, |ui| {
            ui.set_width(w);
            ui.horizontal(|ui| {
                widgets::kicker_colored(ui, if f.outline { "Go to element" } else { "Search workspace" }, css::DIM);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.checkbox(&mut f.pruned, RichText::new("pruned").font(sans(11.5)).color(css::DIM)).on_hover_text("Include elements you removed");
                    ui.checkbox(&mut f.pending, RichText::new("proposed").font(sans(11.5)).color(css::DIM)).on_hover_text("Include elements of pending plans (ghosts)");
                });
            });
            ui.label(RichText::new(if f.loading { "Building workspace index…".to_string() } else { format!("{} indexed items · source files up to 2 MB", items.len()) }).font(sans(10.5)).color(css::FAINT));
            let r = ui.add(egui::TextEdit::singleline(&mut f.query).hint_text("Search design, code, decisions, responsibilities…").font(sans(14.0)).desired_width(f32::INFINITY));
            if !f.focused {
                f.focused = true;
                r.request_focus();
            }
            if r.changed() {
                f.active = 0;
            }
            let found = search(items, &f.query, f.pruned, f.pending);
            f.active = f.active.min(found.len().saturating_sub(1));
            let (down, up, enter, esc) = ui.input(|i| (i.key_pressed(Key::ArrowDown), i.key_pressed(Key::ArrowUp), i.key_pressed(Key::Enter), i.key_pressed(Key::Escape)));
            if down {
                f.active = (f.active + 1).min(found.len().saturating_sub(1));
            }
            if up {
                f.active = f.active.saturating_sub(1);
            }
            if esc {
                out = Some(FindOut::Close);
            }
            if enter {
                if let Some(i) = found.get(f.active) {
                    out = Some(FindOut::Go((*i).clone()));
                }
            }
            if found.is_empty() {
                ui.add_space(8.0);
                let hint = if f.query.trim().is_empty() { "This design has no elements yet." } else if !f.pruned || !f.pending { "No matches. Try including pruned or proposed elements." } else { "No matches in this design." };
                widgets::para(ui, hint, 12.5, css::FAINT);
                return;
            }
            egui::ScrollArea::vertical().max_height((ctx.content_rect().height() * 0.55).min(440.0)).auto_shrink([false, true]).show(ui, |ui| {
                // grouped by kind; best match first within the query ordering
                let mut kinds: Vec<&str> = vec![];
                for i in &found {
                    if !kinds.contains(&i.kind) {
                        kinds.push(i.kind);
                    }
                }
                for kind in kinds {
                    ui.add_space(4.0);
                    ui.label(RichText::new(kind).font(bold(11.0)).color(css::FAINT));
                    for (n, i) in found.iter().enumerate().filter(|(_, i)| i.kind == kind) {
                        let on = n == f.active;
                        let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 26.0), egui::Sense::click());
                        resp.clone().on_hover_text(format!("{}\n{}", i.detail, i.body.chars().take(600).collect::<String>()));
                        resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, on, format!("{}: {} {}", i.kind, i.label, i.detail)));
                        if on {
                            ui.painter().rect_filled(rect, 5.0, css::ACCENT_SOFT);
                            if down || up {
                                resp.scroll_to_me(None);
                            }
                        }
                        let color = if i.pruned() { css::FAINT } else if i.pending() { css::GHOST } else { css::TEXT };
                        ui.painter().text(rect.left_center() + Vec2::new(8.0, 0.0), egui::Align2::LEFT_CENTER, &i.label, sans(13.0), color);
                        let tag = if i.pruned() { " · pruned" } else if i.pending() { " · proposed" } else { "" };
                        let meta = [i.detail.as_str(), tag.trim_start_matches(" · "), i.tab.label()].into_iter().filter(|x| !x.is_empty()).collect::<Vec<_>>().join(" · ");
                        ui.painter().text(rect.right_center() - Vec2::new(8.0, 0.0), egui::Align2::RIGHT_CENTER, meta, mono(10.5), css::FAINT);
                        if resp.hovered() {
                            f.active = n;
                        }
                        if resp.clicked() {
                            out = Some(FindOut::Go((*i).clone()));
                        }
                    }
                }
            });
        });
    });
    out
}

/// A place in the design: a view and optionally an element there.
#[derive(Debug, Clone, PartialEq)]
pub struct Location {
    pub tab: Tab,
    pub target: Option<Target>,
    pub scenario: Option<String>,
}

/// Back/forward over deliberate navigation (not undo: the design is never changed).
#[derive(Debug, Clone, Default)]
pub struct NavHistory {
    back: Vec<Location>,
    forward: Vec<Location>,
}

impl NavHistory {
    pub const MAX: usize = 100;
    /// Leaving `from` for somewhere else.
    pub fn visit(&mut self, from: Location) {
        if self.back.last() != Some(&from) {
            self.back.push(from);
            if self.back.len() > Self::MAX {
                self.back.remove(0);
            }
        }
        self.forward.clear();
    }
    pub fn back(&mut self, current: Location) -> Option<Location> {
        let to = self.back.pop()?;
        self.forward.push(current);
        Some(to)
    }
    pub fn forward(&mut self, current: Location) -> Option<Location> {
        let to = self.forward.pop()?;
        self.back.push(current);
        Some(to)
    }
    pub fn can_back(&self) -> bool {
        !self.back.is_empty()
    }
    pub fn can_forward(&self) -> bool {
        !self.forward.is_empty()
    }
    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scoring_prefers_exact_prefix_word_substring_then_fuzzy() {
        assert!(score("Loan", "loan") > score("LoanRepository", "loan"));
        assert!(score("LoanRepository", "loan") > score("BookLoan", "loan"));
        assert!(score("LoanRepository", "repo") > score("Prepositions", "repo"));
        assert_eq!(score("LoanRepository", "lnrp"), Some(100));
        assert_eq!(score("Loan", "xyz"), None);
    }

    #[test]
    fn index_covers_every_view_and_filters_pruned_and_pending() {
        let d: Design = Design::from_json(&std::fs::read_to_string(format!("{}/../tests/fixtures/library.json", env!("CARGO_MANIFEST_DIR"))).unwrap()).unwrap();
        let mut d = d;
        d.classes[0].status = Status::Pruned;
        let pruned = d.classes[0].name.clone();
        let items = index(&d);
        assert!(items.iter().any(|i| i.kind == "Members"));
        assert!(search(&items, &pruned, false, true).iter().all(|i| i.label != pruned));
        assert!(search(&items, &pruned, true, true).iter().any(|i| i.label == pruned && i.pruned()));
        // outline: everything (live), in view order
        let all = search(&items, "", false, true);
        assert_eq!(all.first().map(|i| i.kind), Some("Classes"));
    }

    #[test]
    fn workspace_search_finds_decisions_metadata_and_source_bodies() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("Helper.java"), "class Helper { void run() { String x = \"needle_body\"; } }").unwrap();
        let code = spide_import::import_java_scoped(root.path(), &[".".into()], &[], None).unwrap().design;
        let design = Design::from_json(r#"{"classes":[{"name":"Order","responsibilities":["safeguard invariant"]}],"decisions":[{"id":"D1","question":"Choose storage","context":"durability tradeoff"}],"forces":[{"name":"Reliable","description":"survive power loss"}]}"#).unwrap();
        let items = workspace_index(&design, Some(&code), root.path());
        assert!(search(&items, "needle_body", false, true).iter().any(|i| matches!(i.destination, Some(Destination::Code { .. }))));
        assert!(search(&items, "durability tradeoff", false, true).iter().any(|i| i.destination == Some(Destination::Decision("D1".into()))));
        assert!(search(&items, "safeguard invariant", false, true).iter().any(|i| i.label == "Order"));
        assert!(search(&items, "survive power loss", false, true).iter().any(|i| i.kind == "Brief"));
        let fresh = workspace_index(&Design::default(), None, root.path());
        assert!(search(&fresh, "needle_body", false, true).is_empty());
    }

    #[test]
    fn source_search_does_not_read_outside_workspace() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let file = outside.path().join("secret.java");
        std::fs::write(&file, "secret_body_token").unwrap();
        let mut code = Design::from_json(r#"{"classes":[{"name":"Helper"}]}"#).unwrap();
        code.classes[0].source = Some(spide_core::model::SourceSymbol { file: file.to_string_lossy().into_owned(), start_line: 1, end_line: 1, ..Default::default() });
        let items = workspace_index(&Design::default(), Some(&code), root.path());
        assert!(search(&items, "secret_body_token", false, true).is_empty());
    }

    #[test]
    fn navigation_history_is_a_stack_with_forward_cleared_by_new_visits() {
        let at = |t: Tab| Location { tab: t, target: None, scenario: None };
        let mut h = NavHistory::default();
        assert!(!h.can_back());
        h.visit(at(Tab::Class));
        h.visit(at(Tab::Package));
        assert_eq!(h.back(at(Tab::System)), Some(at(Tab::Package)));
        assert!(h.can_forward());
        assert_eq!(h.forward(at(Tab::Package)), Some(at(Tab::System)));
        h.back(at(Tab::System));
        h.visit(at(Tab::Usecase));
        assert!(!h.can_forward(), "a new visit drops forward history");
    }
}
