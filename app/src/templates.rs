//! Small, local starter designs. No repository scaffolding, generated code or agent calls.
use serde::{Serialize, Deserialize};
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Template { #[default] Blank, Layered }
impl Template {
    pub fn design(self, name: &str) -> Result<spide_core::Design, String> {
        let mut store = spide_core::Store::default();
        store.design.name = name.into();
        if self == Self::Layered {
            let ops = serde_json::from_value(serde_json::json!([
                {"op":"add_package","name":"ui","layer":"ui"},
                {"op":"add_package","name":"application","layer":"application"},
                {"op":"add_package","name":"domain","layer":"domain"},
                {"op":"add_class","name":"Controller","package":"ui","stereotype":"controller"},
                {"op":"add_class","name":"ApplicationService","package":"application","stereotype":"service"},
                {"op":"add_class","name":"DomainModel","package":"domain","stereotype":"entity"},
                {"op":"add_relation","from":"Controller","to":"ApplicationService","kind":"dependency"},
                {"op":"add_relation","from":"ApplicationService","to":"DomainModel","kind":"dependency"}
            ])).map_err(|e| e.to_string())?;
            store.apply_user(ops).map_err(|e| e.0)?;
        }
        Ok(store.design)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn starters_are_valid_local_designs_with_no_code_links() {
        let blank = Template::Blank.design("Blank").unwrap();
        assert_eq!(blank.name, "Blank");
        assert!(blank.classes.is_empty());
        let layered = Template::Layered.design("Sample").unwrap();
        assert_eq!(layered.classes.len(), 3);
        assert_eq!(layered.packages.len(), 3);
        assert_eq!(layered.relations.len(), 2);
        assert!(layered.classes.iter().all(|c| c.source.is_none()));
        assert!(layered.proposals.is_empty());
    }
}
