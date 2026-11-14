use spide_import::{import_java_scoped, scoped_files};
#[test]
fn scoped_identities_include_packages_overloads_and_nested_types() {
    let repo = tempfile::tempdir().unwrap();
    for (path, code) in [
        ("one/A.java", "package a.same; class A { void run(int x) {} void run(String x) {} class Inner {} }"),
        ("two/A.java", "package b.same; class A {}"),
        ("outside/Other.java", "class Other {}"),
    ] {
        let path = repo.path().join(path); std::fs::create_dir_all(path.parent().unwrap()).unwrap(); std::fs::write(path, code).unwrap();
    }
    let roots = vec!["one".into(), "two".into()];
    let r = import_java_scoped(repo.path(), &roots, &[], None).unwrap();
    assert_eq!(r.files, 2);
    let a = r.design.class("a.same.A").unwrap();
    assert_eq!(a.source.as_ref().unwrap().symbol, "a.same.A");
    assert_eq!(a.methods.len(), 2);
    assert_ne!(a.methods[0].name, a.methods[1].name);
    assert_eq!(a.methods[0].source.as_ref().unwrap().symbol, "a.same.A#run(int)");
    assert!(r.design.classes.iter().any(|c| c.source.as_ref().unwrap().symbol == "a.same.A.Inner"));
    assert!(scoped_files(repo.path(), &["../escape".into()], &[]).is_err());
    assert_eq!(scoped_files(repo.path(), &roots, &["two".into()]).unwrap().len(), 1);
}
#[test]
fn code_line_count_ignores_multiline_comments_and_nested_bodies() {
    let repo = tempfile::tempdir().unwrap();
    std::fs::write(repo.path().join("A.java"), r#"class A {
        void run() {
            /* comment starts
               still a comment without a star
            */
            // whole line comment

            int x = 1; /* trailing comment */
            String s = "/* not a comment */";
            class Local {
                void nested() {
                    int y = 2;
                    y++;
                }
            }
            x++;
        }
    }"#).unwrap();
    let result = import_java_scoped(repo.path(), &[".".into()], &[], None).unwrap();
    assert_eq!(result.design.class("A").unwrap().methods[0].code.as_ref().unwrap().loc, 3);
}

#[cfg(unix)]
#[test]
fn scoped_import_does_not_follow_symlinks() {
    let repo = tempfile::tempdir().unwrap(); let external = tempfile::tempdir().unwrap();
    std::fs::write(external.path().join("Secret.java"), "class Secret {}").unwrap();
    std::os::unix::fs::symlink(external.path(), repo.path().join("escape")).unwrap();
    assert!(scoped_files(repo.path(), &["escape".into()], &[]).is_err());
    assert!(scoped_files(repo.path(), &[".".into()], &[]).unwrap().is_empty());
}
