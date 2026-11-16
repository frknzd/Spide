//! Unsent decision drafts (choice, own alternative, defence) survive navigation and snapshots,
//! isolated per project.

use spide_core::model::{Decision, DecisionState};
use std::collections::VecDeque;

#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct DecisionDraft {
    pub choice: Option<String>,
    pub custom: String,
    pub own_mode: bool,
    pub defense: String,
    pub expanded: Vec<String>,
}

#[derive(Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct Drafts {
    items: VecDeque<(String, DecisionDraft)>,
}

fn key(project: &str, id: &str) -> String {
    format!("{project}\0{id}")
}

impl Drafts {
    pub fn has_input(&self) -> bool {
        self.items.iter().any(|(_, d)| !d.custom.trim().is_empty() || !d.defense.trim().is_empty())
    }
    pub fn get(&self, project: &str, d: &Decision) -> DecisionDraft {
        let k = key(project, &d.id);
        if let Some((_, v)) = self.items.iter().find(|(x, _)| *x == k) {
            return v.clone();
        }
        DecisionDraft {
            choice: d.chosen.clone(),
            custom: d.custom.clone().unwrap_or_default(),
            own_mode: d.custom.is_some() && d.chosen.is_none(),
            defense: if d.state == DecisionState::Rejected { d.attempts.last().map(|a| a.defense.clone()).unwrap_or_default() } else { String::new() },
            expanded: vec![],
        }
    }
    pub fn save(&mut self, project: &str, id: &str, draft: DecisionDraft) {
        let k = key(project, id);
        self.items.retain(|(x, _)| *x != k);
        self.items.push_back((k, draft));
        if self.items.len() > 200 {
            self.items.pop_front();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spide_core::model::Attempt;

    #[test]
    fn drafts_survive_navigation_but_are_isolated_by_project() {
        let d = Decision { id: "a".into(), ..Default::default() };
        let draft = DecisionDraft { choice: Some("A".into()), custom: String::new(), own_mode: false, defense: "keep this typed defence".into(), expanded: vec!["A".into()] };
        let mut s = Drafts::default();
        s.save("project-one", "a", draft.clone());
        assert_eq!(s.get("project-one", &Decision { state: DecisionState::Evaluating, ..d.clone() }), draft);
        assert_eq!(s.get("project-two", &d).defense, "");
        assert_eq!(s.get("project-one", &Decision { id: "b".into(), ..d.clone() }).choice, None);
        let rejected = Decision { state: DecisionState::Rejected, attempts: vec![Attempt { defense: "server text".into(), ..Default::default() }], ..d };
        assert_eq!(s.get("project-one", &rejected), draft);
    }
}
