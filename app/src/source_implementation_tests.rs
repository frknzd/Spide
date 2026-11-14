use super::*;
fn fixture() -> (tempfile::TempDir, tempfile::TempDir, AppState, String) {
    let repo = tempfile::tempdir().unwrap(); let project = tempfile::tempdir().unwrap();
    std::fs::write(repo.path().join("A.java"), "public class A { } // existing dirty work\n").unwrap();
    let (events, _) = crate::events::Events::new(); let st = AppState::new(events, project.path().into());
    let a = Attachment { repository: repo.path().canonicalize().unwrap(), roots: vec![".".into()], exclusions: vec![] };
    let imported = spide_import::import_java_scoped(&a.repository, &a.roots, &a.exclusions, None).unwrap();
    let id = {
        let mut s = st.lock(); s.source.attachment = Some(a.clone()); s.source.status = "current".into(); s.source.files = source_tracking::inventory(&a).unwrap(); s.source.snapshot = Some(imported.design.clone()); s.source.bindings.insert("A".into(), "A".into());
        s.store.load(imported.design);
        let ops: Vec<Op> = serde_json::from_str(r#"[{"op":"add_method","class":"A","method":{"name":"run","visibility":"public"}}]"#).unwrap();
        let p = s.store.propose_ops(ops, None, "agent").unwrap(); s.store.accept_proposal(&p).unwrap();
        s.store.design.decisions.last().unwrap().id.clone()
    };
    (repo, project, st, id)
}
#[test]
fn design_approval_does_not_edit_source_and_authorization_dispatches_once() {
    let (repo, _project, st, decision) = fixture();
    let original = std::fs::read_to_string(repo.path().join("A.java")).unwrap();
    let id = prepare(&st, decision, vec![], vec![], BTreeMap::new()).unwrap();
    assert_eq!(std::fs::read_to_string(repo.path().join("A.java")).unwrap(), original);
    let mut count = 0;
    authorize_with(&st, &id, 0, |message| { count += 1; assert!(message.contains("AGENTS.md")); assert!(message.contains("begin_implementation")); assert!(message.contains(&repo.path().display().to_string())); Ok(()) }).unwrap();
    assert_eq!(count, 1);
    assert!(authorize_with(&st, &id, 0, |_| panic!("duplicate dispatch")).is_err());
    begin(&st, &id).unwrap(); assert!(begin(&st, &id).is_err());
    assert_eq!(std::fs::read_to_string(repo.path().join("A.java")).unwrap(), original);
}
#[test]
fn source_changes_invalidate_approval_and_diagram_undo_never_reverts_source() {
    let (repo, _project, st, decision) = fixture();
    let id = prepare(&st, decision, vec![], vec![], BTreeMap::new()).unwrap();
    std::fs::write(repo.path().join("A.java"), "public class A { public void changed() {} }").unwrap();
    assert!(authorize_with(&st, &id, 0, |_| panic!("stale dispatch")).is_err());
    let before = std::fs::read(repo.path().join("A.java")).unwrap();
    st.lock().store.undo(); st.changed();
    assert_eq!(st.lock().implementations[0].status, "stale");
    assert_eq!(std::fs::read(repo.path().join("A.java")).unwrap(), before);
}
#[test]
fn verification_checks_structure_and_retains_preexisting_dirty_content() {
    let (repo, _project, st, decision) = fixture();
    let id = prepare(&st, decision, vec![], vec!["printf 'fixture validation'".into()], BTreeMap::new()).unwrap();
    authorize_with(&st, &id, 0, |_| Ok(())).unwrap(); begin(&st, &id).unwrap();
    let code = "public class A { public void run() {} } // existing dirty work\n";
    std::fs::write(repo.path().join("A.java"), code).unwrap();
    verify(&st, &id).unwrap();
    let start = Instant::now();
    loop {
        let status = st.lock().implementations[0].status.clone();
        if status != "verifying" { assert_eq!(status, "completed", "{:?}", st.lock().implementations[0]); break; }
        assert!(start.elapsed() < Duration::from_secs(10)); std::thread::sleep(Duration::from_millis(20));
    }
    let s = st.lock(); let t = &s.implementations[0];
    assert!(t.changed_files["A.java"].contains("existing dirty work")); assert_eq!(t.validation.len(), 1);
    assert_eq!(std::fs::read_to_string(repo.path().join("A.java")).unwrap(), code);
}
#[test]
fn cancellation_stops_later_validation_and_keeps_written_files() {
    let (repo, _project, st, decision) = fixture();
    let id = prepare(&st, decision, vec!["sentinel".into()], vec!["sleep 2".into(), "printf unexpected > sentinel".into()], BTreeMap::new()).unwrap();
    authorize_with(&st, &id, 0, |_| Ok(())).unwrap(); begin(&st, &id).unwrap();
    let code = "public class A { public void run() {} } // existing dirty work";
    std::fs::write(repo.path().join("A.java"), code).unwrap();
    verify(&st, &id).unwrap(); std::thread::sleep(Duration::from_millis(100));
    cancel(&st, &PiBridge::new(), &id).unwrap();
    std::thread::sleep(Duration::from_millis(250));
    assert_eq!(st.lock().implementations[0].status, "cancelled");
    assert!(!repo.path().join("sentinel").exists());
    assert_eq!(std::fs::read_to_string(repo.path().join("A.java")).unwrap(), code);
    assert!(st.lock().implementations[0].changed_files.contains_key("A.java"));
}

#[test]
fn validation_failure_is_persisted_without_reverting_source() {
    let (repo, _project, st, decision) = fixture();
    let id = prepare(&st, decision, vec![], vec!["printf fixture_failure; exit 2".into()], BTreeMap::new()).unwrap();
    authorize_with(&st, &id, 0, |_| Ok(())).unwrap(); begin(&st, &id).unwrap();
    std::fs::write(repo.path().join("A.java"), "public class A { public void run() {} }").unwrap();
    verify(&st, &id).unwrap();
    let start = Instant::now();
    while st.lock().implementations[0].status == "verifying" { assert!(start.elapsed() < Duration::from_secs(10)); std::thread::sleep(Duration::from_millis(10)); }
    let s = st.lock(); assert_eq!(s.implementations[0].status, "failed");
    assert!(s.implementations[0].validation[0].contains("fixture_failure"));
    assert!(std::fs::read_to_string(repo.path().join("A.java")).unwrap().contains("run"));
}

#[test]
fn agent_success_cannot_replace_structural_verification() {
    let (repo, _project, st, decision) = fixture();
    let id = prepare(&st, decision, vec![], vec![], BTreeMap::new()).unwrap();
    authorize_with(&st, &id, 0, |_| Ok(())).unwrap(); begin(&st, &id).unwrap();
    let task = st.lock().implementations[0].clone();
    let code = spide_import::import_java(repo.path(), None).unwrap().design;
    assert!(!implementation_mismatches(&task, &code).is_empty());
    update(&st, &id, |t| t.status = "running".into()).unwrap();
    let loaded = load(&st.lock().project_dir);
    assert_eq!(loaded[0].status, "interrupted");
}
