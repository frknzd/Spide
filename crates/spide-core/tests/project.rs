//! The Spide Project manifest and local-file hygiene.
use spide_core::project::{self, Command, Manifest};

#[test]
fn manifest_defaults_round_trip_and_keep_unknown_fields() {
    let m = Manifest::parse(r#"{"name":"REST extraction","code_root":"org.zaproxy.addon.restextraction","owner":"group 9"}"#).unwrap();
    assert_eq!(m.roots, vec!["src/main/java".to_string()]);
    assert_eq!(m.language, "java");
    let again = Manifest::parse(&m.to_json()).unwrap();
    assert_eq!(again, m);
    assert!(m.to_json().contains("\"owner\": \"group 9\""));
}

#[test]
fn validation_rejects_escaping_paths_and_allows_repo_commands() {
    let repo = tempfile::tempdir().unwrap();
    let root = repo.path().join("addOns/rx");
    std::fs::create_dir_all(root.join("src/main/java")).unwrap();
    std::fs::create_dir_all(repo.path().join(".git")).unwrap();
    let wt = project::work_tree(&root).unwrap();
    assert_eq!(wt, repo.path().canonicalize().unwrap());
    let mut m = Manifest { code_root: Some("org.x".into()), ..Default::default() };
    m.commands.insert("test".into(), Command { run: "./gradlew :addOns:rx:test".into(), cwd: Some("../..".into()) });
    assert!(m.validate(&root, Some(&wt)).is_empty(), "{:?}", m.validate(&root, Some(&wt)));
    // without git the project root bounds the command
    assert!(!m.validate(&root, None).is_empty());
    let bad = Manifest { roots: vec!["../other".into(), "/abs".into(), "missing".into()], code_root: Some("org..x".into()), ..Default::default() };
    let errs = bad.validate(&root, Some(&wt));
    assert_eq!(errs.len(), 4, "{errs:?}");
    let mut escaping = Manifest::default();
    escaping.commands.insert("test".into(), Command { run: "make".into(), cwd: Some("../../..".into()) });
    assert!(escaping.validate(&root, Some(&wt)).iter().any(|e| e.contains("leaves the repository")));
}

#[test]
fn git_exclusion_is_local_and_idempotent() {
    let repo = tempfile::tempdir().unwrap();
    let root = repo.path().join("addOns/rx");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(repo.path().join(".git/info")).unwrap();
    std::fs::write(repo.path().join(".git/info/exclude"), "# existing\n*.tmp").unwrap();
    let added = project::exclude_local_files(&root).unwrap();
    assert_eq!(added, vec!["/addOns/rx/.spide/".to_string(), "/addOns/rx/.pi/mcp.json".to_string()]);
    assert!(project::exclude_local_files(&root).unwrap().is_empty());
    let text = std::fs::read_to_string(repo.path().join(".git/info/exclude")).unwrap();
    assert!(text.starts_with("# existing\n*.tmp\n# Spide (local files)\n"), "{text}");
    // outside git: nothing happens
    let plain = tempfile::tempdir().unwrap();
    assert!(project::exclude_local_files(plain.path()).unwrap().is_empty());
}

#[test]
fn roots_resolve_from_the_spide_folder() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("spide")).unwrap();
    std::fs::write(dir.path().join(project::MANIFEST), "{}").unwrap();
    assert!(project::is_project(dir.path()));
    assert_eq!(project::resolve_root(&dir.path().join("spide")), dir.path());
    assert_eq!(project::resolve_root(&dir.path().join("spide/project.json")), dir.path());
    assert_eq!(project::resolve_root(dir.path()), dir.path());
}
