//! Proposal review helpers: stable selection, field diffs, structured patching, footprints and
//! locally persisted plan drafts.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use spide_core::model::Proposal;
use spide_core::proposal_review::ProposalReview;
use spide_diagram::focus::Footprint;
use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;

/// Structured forms patch fields without dropping advanced/unexposed values.
pub fn patch_operation(op: &Value, field: &str, value: Value, nested: Option<&str>) -> Value {
    let mut out = op.clone();
    let obj = out.as_object_mut().expect("operation is an object");
    match nested {
        Some(n) => {
            let mut inner = obj.get(n).and_then(Value::as_object).cloned().unwrap_or_default();
            inner.insert(field.to_string(), value);
            obj.insert(n.to_string(), Value::Object(inner));
        }
        None => {
            obj.insert(field.to_string(), value);
        }
    }
    out
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlanDraft {
    pub scope: String,
    pub plan: Proposal,
    pub original: Proposal,
    pub rev: u64,
}

pub fn same_plan(a: &Proposal, b: &Proposal) -> bool {
    let k = |p: &Proposal| serde_json::to_string(&(&p.ops, &p.rationale, &p.decision, &p.option)).unwrap_or_default();
    k(a) == k(b)
}

/// Drafts in memory, mirrored to a JSON file (the old UI used localStorage).
#[derive(Default)]
pub struct PlanDrafts {
    memory: HashMap<String, PlanDraft>,
    pub file: Option<PathBuf>,
    /// The last failed write of the drafts file (shown once, then cleared by the UI).
    pub error: Option<String>,
    blocked: bool,
}

impl PlanDrafts {
    pub fn with_file(file: PathBuf) -> Self {
        match std::fs::read_to_string(&file) {
            Ok(t) => match serde_json::from_str(&t) {
                Ok(memory) => Self { memory, file: Some(file), error: None, blocked: false },
                Err(e) => Self { file: Some(file), error: Some(format!("Proposal drafts could not be loaded: {e}. Original file retained.")), blocked: true, ..Default::default() },
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self { file: Some(file), ..Default::default() },
            Err(e) => Self { file: Some(file), error: Some(format!("Proposal drafts could not be read: {e}")), blocked: true, ..Default::default() },
        }
    }
    pub fn flush(&mut self) -> Result<(), String> {
        if self.blocked { return Err("Cannot replace the unreadable proposal draft file. Copy your draft somewhere safe before leaving.".into()); }
        if let Some(f) = &self.file {
            let text = serde_json::to_string(&self.memory).map_err(|e| e.to_string())?;
            if let Err(e) = crate::state::write_atomic(f, &text) {
                let e = format!("Proposal drafts could not be saved to {}: {e}. They are kept in memory until Spide quits.", f.display());
                self.error = Some(e.clone());
                return Err(e);
            }
        }
        Ok(())
    }
    pub fn load(&self, scope: &str) -> Option<PlanDraft> {
        self.memory.get(scope).filter(|d| d.scope == scope && !same_plan(&d.plan, &d.original)).cloned()
    }
    pub fn store(&mut self, draft: PlanDraft) {
        if same_plan(&draft.plan, &draft.original) {
            self.memory.remove(&draft.scope);
        } else {
            self.memory.insert(draft.scope.clone(), draft);
        }
        let _ = self.flush();
    }
}

pub fn next_proposal_id(ids: &[String], selected: Option<&str>, previous: &[String]) -> Option<String> {
    if let Some(s) = selected {
        if ids.iter().any(|i| i == s) {
            return Some(s.to_string());
        }
    }
    if ids.is_empty() {
        return None;
    }
    let index = selected.and_then(|s| previous.iter().position(|p| p == s)).unwrap_or(0);
    Some(ids[index.min(ids.len() - 1)].clone())
}

pub fn operation_label(op: &Value) -> String {
    let name = op.get("op").and_then(Value::as_str).unwrap_or("");
    let verb = if name.starts_with("add_") {
        "Add"
    } else if name.starts_with("remove_") {
        "Delete"
    } else {
        "Update"
    };
    format!("{verb} · {}", name.replace('_', " "))
}

pub fn operation_target(op: &Value) -> Option<String> {
    let name = op.get("op").and_then(Value::as_str).unwrap_or("");
    if let Some(c) = op.get("class").and_then(Value::as_str) {
        return Some(c.into());
    }
    if let Some(f) = op.get("from").and_then(Value::as_str) {
        if name.contains("class") || name.contains("relation") {
            return Some(f.into());
        }
    }
    op.get("name").and_then(Value::as_str).filter(|_| name.contains("class")).map(String::from)
}

#[derive(Debug, Clone, PartialEq)]
pub struct FieldDiff {
    pub field: String,
    pub before: Value,
    pub after: Value,
}

pub fn field_diffs(before: &Value, after: &Value, prefix: &str) -> Vec<FieldDiff> {
    if before == after {
        return vec![];
    }
    let empty = Map::new();
    let is_obj = |v: &Value| v.is_object();
    if (is_obj(before) && (is_obj(after) || after.is_null())) || (is_obj(after) && before.is_null()) {
        let a = before.as_object().unwrap_or(&empty);
        let b = after.as_object().unwrap_or(&empty);
        let mut keys: Vec<&String> = a.keys().collect();
        for k in b.keys() {
            if !keys.contains(&k) {
                keys.push(k);
            }
        }
        return keys
            .into_iter()
            .filter(|k| !matches!(k.as_str(), "status" | "proposal" | "origin"))
            .flat_map(|k| {
                let p = if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") };
                field_diffs(a.get(k).unwrap_or(&Value::Null), b.get(k).unwrap_or(&Value::Null), &p)
            })
            .collect();
    }
    vec![FieldDiff { field: if prefix.is_empty() { "value".into() } else { prefix.into() }, before: before.clone(), after: after.clone() }]
}

// ---------------------------------------------------------------- readable review

/// One readable line of a plan's effect, with the diagram element it concerns (hover-key form).
#[derive(Debug, Clone, PartialEq)]
pub struct ChangeLine {
    /// '+' added, '~' changed, '−' removed
    pub sign: char,
    pub text: String,
    /// `class:A`, `relation:id`, `package:p`; None for changes without a class-diagram element
    pub key: Option<String>,
    /// the class to locate on the canvas
    pub class: Option<String>,
}

fn sign_of(before: &Value, after: &Value) -> char {
    match (before.is_null(), after.is_null()) {
        (true, false) => '+',
        (false, true) => '−',
        _ => '~',
    }
}

fn str_of<'a>(v: &'a Value, before: &'a Value, k: &str) -> &'a str {
    v.get(k).or_else(|| before.get(k)).and_then(Value::as_str).unwrap_or("?")
}

/// A short signature for a member object: `name(a: T): R` for methods, `name: T` for attributes.
pub fn member_text(kind: &str, v: &Value) -> String {
    let name = v.get("name").and_then(Value::as_str).unwrap_or("?");
    if kind == "methods" {
        let params: Vec<String> = v.get("params").and_then(Value::as_array).map(|ps| ps.iter().map(|p| format!("{}: {}", p.get("name").and_then(Value::as_str).unwrap_or(""), p.get("type").and_then(Value::as_str).unwrap_or(""))).collect()).unwrap_or_default();
        let ret = v.get("returns").and_then(Value::as_str).map(|r| format!(": {r}")).unwrap_or_default();
        format!("{name}({}){ret}", params.join(", "))
    } else {
        let ty = v.get("type").and_then(Value::as_str).map(|t| format!(": {t}")).unwrap_or_default();
        format!("{name}{ty}")
    }
}

fn members<'a>(class: &'a Value, kind: &str) -> Vec<&'a Value> {
    class.get(kind).and_then(Value::as_array).map(|a| a.iter().collect()).unwrap_or_default()
}

/// Member-level changes of one class: (sign, kind, before, after) per attribute/method name.
fn member_changes<'a>(before: &'a Value, after: &'a Value) -> Vec<(char, &'static str, &'a Value, &'a Value)> {
    let mut out = vec![];
    for kind in ["attributes", "methods"] {
        let (a, b) = (members(before, kind), members(after, kind));
        let name = |v: &Value| v.get("name").and_then(Value::as_str).map(str::to_owned);
        let mut names: Vec<String> = vec![];
        for v in a.iter().chain(b.iter()) {
            if let Some(n) = name(v) {
                if !names.contains(&n) {
                    names.push(n);
                }
            }
        }
        for n in names {
            let x = a.iter().find(|v| name(v).as_deref() == Some(&n)).copied().unwrap_or(&Value::Null);
            let y = b.iter().find(|v| name(v).as_deref() == Some(&n)).copied().unwrap_or(&Value::Null);
            if field_diffs(x, y, "").is_empty() {
                continue;
            }
            out.push((sign_of(x, y), kind, x, y));
        }
    }
    out
}

/// A readable line for one review change.
pub fn describe_change(path: &str, before: &Value, after: &Value) -> ChangeLine {
    let (kind, name) = path.split_once('.').unwrap_or((path, ""));
    let sign = sign_of(before, after);
    let verb = match sign {
        '+' => "Add",
        '−' => "Remove",
        _ => "Change",
    };
    let cur = if after.is_null() { before } else { after };
    match kind {
        "classes" => {
            let text = if sign == '~' {
                let mut bits: Vec<String> = member_changes(before, after).into_iter().map(|(s, k, x, y)| format!("{s} {}", member_text(k, if y.is_null() { x } else { y }))).collect();
                bits.extend(field_diffs(before, after, "").into_iter().filter(|f| !f.field.starts_with("attributes") && !f.field.starts_with("methods")).map(|f| format!("~ {}", f.field.replace('_', " "))));
                format!("{name}: {}", if bits.is_empty() { "metadata".into() } else { bits.join(", ") })
            } else {
                let n = |k| members(cur, k).len();
                let (a, m) = (n("attributes"), n("methods"));
                let pkg = cur.get("package").and_then(Value::as_str).map(|p| format!(" in {p}")).unwrap_or_default();
                let size = if a + m > 0 { format!(" ({a} attr, {m} methods)") } else { String::new() };
                format!("{verb} class {name}{pkg}{}", if sign == '+' { size } else { String::new() })
            };
            ChangeLine { sign, text, key: Some(format!("class:{name}")), class: Some(name.to_string()) }
        }
        "relations" => {
            let (from, to) = (str_of(after, before, "from"), str_of(after, before, "to"));
            let rk = str_of(after, before, "kind");
            let detail = if sign == '~' { format!(" ({})", field_diffs(before, after, "").iter().map(|f| f.field.replace('_', " ")).collect::<Vec<_>>().join(", ")) } else { String::new() };
            ChangeLine { sign, text: format!("{verb} {rk} {from} → {to}{detail}"), key: Some(format!("relation:{name}")), class: Some(from.to_string()) }
        }
        "packages" => ChangeLine { sign, text: format!("{verb} package {name}"), key: Some(format!("package:{name}")), class: None },
        _ => {
            let what = kind.trim_end_matches('s').replace('_', " ");
            let text = if name.is_empty() { format!("{verb} {}", kind.replace('_', " ")) } else { format!("{verb} {what} {name}") };
            ChangeLine { sign, text, key: None, class: None }
        }
    }
}

/// Every readable change of a reviewed plan, in plan order.
pub fn change_lines(review: &ProposalReview) -> Vec<ChangeLine> {
    review.steps.iter().flat_map(|s| s.changes.iter().map(|c| describe_change(&c.path, &c.before, &c.after))).collect()
}

/// A human label for a diagram element key (`class:A`, `method:A.run`, `relation:r1`).
pub fn element_label(key: &str, review: Option<&ProposalReview>) -> String {
    let (kind, rest) = key.split_once(':').unwrap_or(("", key));
    match kind {
        "method" => format!("{rest}()"),
        "relation" => review
            .and_then(|r| r.design.relations.iter().chain(r.baseline.relations.iter()).find(|x| x.id == rest))
            .map(|x| format!("{} → {}", x.from, x.to))
            .unwrap_or_else(|| format!("relation {rest}")),
        "package" => format!("package {rest}"),
        _ => rest.to_string(),
    }
}

/// The class a diagram element key belongs to.
pub fn element_class(key: &str, review: Option<&ProposalReview>) -> Option<String> {
    let (kind, rest) = key.split_once(':')?;
    match kind {
        "class" => Some(rest.into()),
        "method" | "attribute" => rest.rfind('.').map(|i| rest[..i].to_string()),
        "relation" => review.and_then(|r| r.design.relations.iter().chain(r.baseline.relations.iter()).find(|x| x.id == rest)).map(|x| x.from.clone()),
        _ => None,
    }
}

/// What a plan does to one element: readable before/after field changes (empty: untouched).
pub fn element_diffs(review: &ProposalReview, key: &str) -> Vec<FieldDiff> {
    let Some((kind, rest)) = key.split_once(':') else { return vec![] };
    let find = |path: String| review.steps.iter().flat_map(|s| s.changes.iter()).filter(move |c| c.path == path);
    let mut out = vec![];
    match kind {
        "class" => {
            for c in find(format!("classes.{rest}")) {
                for (_, k, x, y) in member_changes(&c.before, &c.after) {
                    let kind = if k == "methods" { "method" } else { "attribute" };
                    let text = |v: &Value| if v.is_null() { Value::Null } else { Value::String(member_text(k, v)) };
                    let name = if y.is_null() { x } else { y }.get("name").and_then(Value::as_str).unwrap_or("?");
                    out.push(FieldDiff { field: format!("{kind} {name}"), before: text(x), after: text(y) });
                }
                out.extend(field_diffs(&c.before, &c.after, "").into_iter().filter(|f| !f.field.starts_with("attributes") && !f.field.starts_with("methods")));
            }
        }
        "method" | "attribute" => {
            let Some(i) = rest.rfind('.') else { return vec![] };
            let (class, name) = (&rest[..i], &rest[i + 1..]);
            let plural = if kind == "method" { "methods" } else { "attributes" };
            for c in find(format!("classes.{class}")) {
                let pick = |v: &Value| members(v, plural).into_iter().find(|m| m.get("name").and_then(Value::as_str) == Some(name)).cloned().unwrap_or(Value::Null);
                out.extend(field_diffs(&pick(&c.before), &pick(&c.after), ""));
            }
        }
        "relation" => {
            for c in find(format!("relations.{rest}")) {
                out.extend(field_diffs(&c.before, &c.after, ""));
            }
        }
        "package" => {
            for c in find(format!("packages.{rest}")) {
                out.extend(field_diffs(&c.before, &c.after, ""));
            }
        }
        _ => {}
    }
    // a new or removed element: only its meaningful values (no `false`, empty lists, unset fields)
    let blank = |v: &Value| match v {
        Value::Null | Value::Bool(false) => true,
        Value::Array(a) => a.is_empty(),
        Value::String(s) => s.is_empty(),
        _ => false,
    };
    out.retain(|f| !(blank(&f.before) && blank(&f.after)));
    out
}

/// A compact display of a diff value.
pub fn value_text(v: &Value) -> String {
    match v {
        Value::Null => "—".into(),
        Value::String(s) => s.clone(),
        Value::Array(a) if a.iter().all(Value::is_string) => {
            if a.is_empty() {
                "—".into()
            } else {
                a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")
            }
        }
        v => serde_json::to_string(v).unwrap_or_default(),
    }
}

/// Refinement targets and guidance for one decision, kept until sent or discarded.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Refinement {
    pub targets: Vec<String>,
    pub inspect: Option<String>,
}

impl Refinement {
    /// Click on a diagram element: inspect it and toggle it as a refinement target.
    pub fn toggle(&mut self, key: &str) {
        if let Some(i) = self.targets.iter().position(|t| t == key) {
            self.targets.remove(i);
            if self.inspect.as_deref() == Some(key) {
                self.inspect = None;
            }
        } else {
            self.targets.push(key.to_string());
            self.inspect = Some(key.to_string());
        }
    }
}

pub fn review_footprint(review: Option<&ProposalReview>, id: Option<&str>) -> Option<Footprint> {
    let (review, id) = (review?, id?);
    let mut f = Footprint { ids: BTreeSet::from([id.to_string()]), ..Default::default() };
    for step in &review.steps {
        for c in &step.changes {
            let mut parts = c.path.splitn(2, '.');
            let kind = parts.next().unwrap_or("");
            let name = parts.next().unwrap_or("").to_string();
            match kind {
                "classes" => {
                    f.changed.insert(name);
                }
                "packages" => {
                    f.packages.insert(name);
                }
                "relations" => {
                    f.relations.insert(name);
                    for v in [&c.before, &c.after] {
                        if let Some(from) = v.get("from").and_then(Value::as_str) {
                            f.anchors.insert(from.into());
                        }
                        if let Some(to) = v.get("to").and_then(Value::as_str) {
                            f.anchors.insert(to.into());
                        }
                    }
                }
                _ => {}
            }
        }
    }
    let changed = f.changed.clone();
    f.anchors.retain(|a| !changed.contains(a));
    for c in &review.design.classes {
        if f.changed.contains(&c.name) || f.anchors.contains(&c.name) {
            if let Some(p) = &c.package {
                f.packages.insert(p.clone());
            }
        }
    }
    (!f.changed.is_empty() || !f.relations.is_empty() || !f.packages.is_empty()).then_some(f)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use spide_core::proposal_review::{ReviewChange, ReviewStep};

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn selection_stays_by_id_and_resolves_to_the_next_surviving_plan() {
        assert_eq!(next_proposal_id(&ids(&["a", "b", "c"]), Some("b"), &[]).as_deref(), Some("b"));
        assert_eq!(next_proposal_id(&ids(&["a", "c"]), Some("b"), &ids(&["a", "b", "c"])).as_deref(), Some("c"));
        assert_eq!(next_proposal_id(&ids(&["a", "b"]), Some("c"), &ids(&["a", "b", "c"])).as_deref(), Some("b"));
        assert_eq!(next_proposal_id(&[], Some("a"), &ids(&["a"])), None);
    }

    #[test]
    fn diffs_expose_metadata_changes_without_ghost_noise() {
        let d = field_diffs(&json!({"role": "old", "status": "accepted", "proposal": null}), &json!({"role": "new", "status": "proposed", "proposal": "p"}), "");
        assert_eq!(d, vec![FieldDiff { field: "role".into(), before: json!("old"), after: json!("new") }]);
        assert!(operation_label(&json!({"op": "remove_class"})).starts_with("Delete"));
        assert!(operation_label(&json!({"op": "update_class"})).starts_with("Update"));
    }

    #[test]
    fn structured_patching_preserves_advanced_and_nested_fields() {
        let op = json!({"op": "update_method", "class": "A", "name": "run", "custom": "keep", "method": {"name": "run", "concern": "audit", "calls": ["B.log"], "description": "old"}});
        let edited = patch_operation(&op, "description", json!("new"), Some("method"));
        assert_eq!(edited["custom"], "keep");
        let mut expected = op["method"].clone();
        expected["description"] = json!("new");
        assert_eq!(edited["method"], expected);
        assert_eq!(op["method"]["description"], "old");
    }

    #[test]
    fn review_footprint_includes_updates_without_ghost_status() {
        let mut design = spide_core::Design::default();
        design.classes.push(spide_core::model::Class { name: "A".into(), package: Some("core".into()), ..Default::default() });
        let review = ProposalReview {
            rev: 1,
            baseline: design.clone(),
            design,
            steps: vec![ReviewStep { index: 0, changes: vec![ReviewChange { path: "classes.A".into(), before: json!({"role": "old"}), after: json!({"role": "new"}) }], error: None }],
            errors: vec![],
            prerequisites: vec![],
            notices: vec![],
            can_accept: true,
            impact: None,
        };
        let f = review_footprint(Some(&review), Some("p")).unwrap();
        assert!(f.changed.contains("A"));
        assert!(f.packages.contains("core"));
    }

    #[test]
    fn changes_read_as_sentences_and_elements_show_their_own_diffs() {
        let before = json!({"name": "Loan", "attributes": [{"name": "due", "type": "Date"}], "methods": [], "role": null});
        let after = json!({"name": "Loan", "attributes": [{"name": "due", "type": "Date"}], "methods": [{"name": "fine", "params": [{"name": "today", "type": "Date"}], "returns": "Money"}], "role": "domain"});
        let l = describe_change("classes.Loan", &before, &after);
        assert_eq!((l.sign, l.key.as_deref(), l.class.as_deref()), ('~', Some("class:Loan"), Some("Loan")));
        assert_eq!(l.text, "Loan: + fine(today: Date): Money, ~ role");
        let added = describe_change("classes.Fine", &Value::Null, &json!({"name": "Fine", "package": "domain"}));
        assert_eq!((added.sign, added.text.as_str()), ('+', "Add class Fine in domain"));
        let rel = describe_change("relations.r1", &json!({"from": "A", "to": "B", "kind": "dependency"}), &Value::Null);
        assert_eq!((rel.sign, rel.text.as_str(), rel.key.as_deref()), ('−', "Remove dependency A → B", Some("relation:r1")));
        let force = describe_change("forces.Speed", &Value::Null, &json!({"name": "Speed"}));
        assert_eq!((force.text.as_str(), force.key), ("Add force Speed", None));

        let review = ProposalReview {
            rev: 1,
            baseline: Default::default(),
            design: Default::default(),
            steps: vec![ReviewStep { index: 0, changes: vec![ReviewChange { path: "classes.Loan".into(), before, after }], error: None }],
            errors: vec![],
            prerequisites: vec![],
            notices: vec![],
            can_accept: true,
            impact: None,
        };
        let d = element_diffs(&review, "class:Loan");
        assert_eq!(d[0], FieldDiff { field: "method fine".into(), before: Value::Null, after: json!("fine(today: Date): Money") });
        assert!(d.iter().any(|f| f.field == "role"));
        let m = element_diffs(&review, "method:Loan.fine");
        assert!(m.iter().any(|f| f.field == "returns" && f.after == json!("Money")));
        assert!(element_diffs(&review, "attribute:Loan.due").is_empty(), "untouched member");
        assert_eq!(element_label("method:Loan.fine", None), "Loan.fine()");
        assert_eq!(element_class("attribute:Loan.due", None).as_deref(), Some("Loan"));
        let mut r = Refinement::default();
        r.toggle("class:Loan");
        assert_eq!((r.targets.len(), r.inspect.as_deref()), (1, Some("class:Loan")));
        r.toggle("class:Loan");
        assert_eq!(r, Refinement::default());
    }

    #[test]
    fn drafts_are_scoped_and_disappear_when_clean() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("drafts.json");
        let original = Proposal { id: "p".into(), ops: vec![], source: "agent".into(), rationale: None, ..Default::default() };
        let plan = Proposal { rationale: Some("amended".into()), ..original.clone() };
        let draft = PlanDraft { scope: "project:branch:p".into(), plan: plan.clone(), original: original.clone(), rev: 7 };
        assert!(!same_plan(&plan, &original));
        let mut s = PlanDrafts::with_file(file.clone());
        s.store(draft.clone());
        assert_eq!(s.load(&draft.scope), Some(draft.clone()));
        assert_eq!(s.load("other:branch:p"), None);
        // survives a restart (the file)
        assert_eq!(PlanDrafts::with_file(file.clone()).load(&draft.scope), Some(draft.clone()));
        s.store(PlanDraft { original: plan, ..draft.clone() });
        assert_eq!(s.load(&draft.scope), None);
        // without a file, in memory only
        let mut m = PlanDrafts::default();
        m.store(draft.clone());
        assert_eq!(m.load(&draft.scope), Some(draft));
    }
}
