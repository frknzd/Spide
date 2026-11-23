use spide_import::import_java_scoped;

#[test]
fn explicit_external_import_does_not_bind_an_unrelated_internal_namesake() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("Helper.java"), "package internal; class Helper { void run() {} }").unwrap();
    std::fs::write(dir.path().join("Main.java"), "package app; import external.Helper; class Main { Helper h; void go() { h.run(); } }").unwrap();
    let result = import_java_scoped(dir.path(), &[".".into()], &[], None).unwrap();
    let main = result.design.class("Main").unwrap();
    assert!(main.methods[0].calls.is_empty());
    assert!(!result.design.relations.iter().any(|r| r.from == "Main" && r.to == "Helper"));
}
