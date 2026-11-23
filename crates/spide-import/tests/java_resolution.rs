use spide_import::import_java_scoped;

#[test]
fn imports_overloads_shadowing_and_collection_receivers() {
    let repo = tempfile::tempdir().unwrap();
    for (file, code) in [
        ("a/Helper.java", "package a; public class Helper { public void run() {} public void run(int x) {} public void same(int x) {} public void same(String x) {} public void visit() {} }"),
        ("b/Helper.java", "package b; public class Helper { public void run() {} }"),
        ("Main.java", "package app; import a.Helper; import java.util.List; class Main { Helper helper; List<Helper> items; void test() { helper.run(); helper.run(1); helper.same(1); items.run(); Runnable callback = helper::visit; } void shadow(b.Helper helper) { helper.run(); this.helper.run(); } }"),
    ] {
        let path = repo.path().join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, code).unwrap();
    }
    let result = import_java_scoped(repo.path(), &[".".into()], &[], None).unwrap();
    let main = result.design.class("Main").unwrap();
    let test = main.methods.iter().find(|m| m.name == "test").unwrap();
    assert_eq!(test.calls, vec!["a.Helper.run()", "a.Helper.run(int)", "a.Helper.visit"]);
    assert_eq!(main.methods.iter().find(|m| m.name == "shadow").unwrap().calls, vec!["a.Helper.run()", "b.Helper.run"]);
    assert!(result.design.relations.iter().any(|r| r.from == "Main" && r.to == "a.Helper"));
}

#[test]
fn same_package_collision_and_local_overload_identity() {
    let repo = tempfile::tempdir().unwrap();
    for (file, code) in [
        ("A.java", "package p; class Helper { void run() {} } class A { Helper helper; void go() { helper.run(); local(); local(1); } void local() {} void local(int x) {} }"),
        ("Other.java", "package q; class Helper { void run() {} }"),
    ] { std::fs::write(repo.path().join(file), code).unwrap(); }
    let result = import_java_scoped(repo.path(), &[".".into()], &[], None).unwrap();
    assert_eq!(result.design.class("A").unwrap().methods.iter().find(|m| m.name == "go").unwrap().calls,
        vec!["local()", "local(int)", "p.Helper.run"]);
}
