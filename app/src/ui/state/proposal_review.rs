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
}

impl PlanDrafts {
    pub fn with_file(file: PathBuf) -> Self {
        let memory = std::fs::read_to_string(&file).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        Self { memory, file: Some(file) }
    }
    fn persist(&self) {
        if let Some(f) = &self.file {
            if let Some(dir) = f.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(f, serde_json::to_string(&self.memory).unwrap_or_default());
        }
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
        self.persist();
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
