use spide_core::{CodeFacts, Design};

#[test]
fn java_attached_documentation_is_measured_not_guessed() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("Parser.java"), r#"
class Parser {
    /** Explain parsing. */
    @Deprecated
    private void documented() {}
    // Explain the helper.
    private void ordinary() {}
    /* ** */
    private void empty() {}
    private void previous() {} // About previous, not trailing.
    private void trailing() {}
    // Not attached.

    private void distant() {}
    private void nested() { class Inner { /* inside */ void f() {} } }
    private void missing() {}
}
"#).unwrap();
    let result = spide_import::import_java(dir.path(), None).unwrap();
    let c = result.design.class("Parser").unwrap();
    for m in &c.methods {
        let expected = matches!(m.name.as_str(), "documented" | "ordinary");
        assert_eq!(m.code.as_ref().unwrap().source_documented, Some(expected), "{}", m.name);
    }
    let restored = Design::from_json(&result.design.to_json()).unwrap();
    assert_eq!(restored.class("Parser").unwrap().methods, c.methods);
    let legacy: CodeFacts = serde_json::from_str("{}").unwrap();
    assert_eq!(legacy.source_documented, None);
}
