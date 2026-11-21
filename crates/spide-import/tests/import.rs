use spide_core::metrics::compute;
use spide_core::*;
use spide_core::ops;
use std::path::PathBuf;

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/library")
}

#[test]
fn imports_structure_from_java() {
    let r = spide_import::import_java(&fixture(), None).unwrap();
    assert!(r.skipped.is_empty(), "{:?}", r.skipped);
    assert_eq!(r.files, 9, "test sources are skipped");
    let d = &r.design;
    let names: Vec<&str> = d.classes.iter().map(|c| c.name.as_str()).collect();
    for n in ["Book", "Member", "Loan", "LoanPolicy", "StandardPolicy", "LoanType", "LendingService", "LoanRepository", "AuditLog"] {
        assert!(names.contains(&n), "missing {n}: {names:?}");
    }
    assert!(!names.contains(&"LoanTest"));
    assert_eq!(d.settings.code_root.as_deref(), Some("org.acme.library"));

    let c = |n: &str| d.class(n).unwrap();
    assert_eq!(c("Loan").package.as_deref(), Some("domain"));
    assert_eq!(c("LoanPolicy").stereotype.as_deref(), Some("interface"));
    assert_eq!(c("LoanType").stereotype.as_deref(), Some("enum"));
    assert_eq!(c("AuditLog").stereotype.as_deref(), Some("value"), "records are values");
    assert!(c("LoanRepository").is_abstract);
    assert!(c("LoanRepository").methods[0].is_abstract);
    assert!(c("LoanPolicy").methods.iter().find(|m| m.name == "fineFor").unwrap().is_abstract);
    assert!(!c("LoanPolicy").methods.iter().find(|m| m.name == "allowsRenewal").unwrap().is_abstract, "default method");
    assert_eq!(c("Loan").origin.as_deref(), Some("import"));

    // fields → attributes, methods with signatures
    let lend = c("LendingService").methods.iter().find(|m| m.name == "lend").unwrap();
    assert_eq!(lend.params.len(), 2);
    assert_eq!(lend.returns.as_deref(), Some("Loan"));
    // calls resolved through field and parameter types
    assert!(lend.calls.contains(&"LoanRepository.save".to_string()), "{:?}", lend.calls);
    assert!(lend.calls.contains(&"Member.canBorrow".to_string()), "{:?}", lend.calls);
    assert!(lend.uses.contains(&"loans".to_string()));
    let fine = c("Loan").methods.iter().find(|m| m.name == "fine").unwrap();
    assert!(fine.calls.contains(&"LoanPolicy.fineFor".to_string()) && fine.calls.contains(&"AuditLog.record".to_string()), "{:?}", fine.calls);

    // relations
    let has = |from: &str, to: &str, kind: RelationKind| d.relations.iter().any(|r| r.from == from && r.to == to && r.kind == kind);
    assert!(has("StandardPolicy", "LoanPolicy", RelationKind::Realization));
    assert!(has("Member", "Loan", RelationKind::Association));
    assert_eq!(d.relations.iter().find(|r| r.from == "Member" && r.to == "Loan").unwrap().to_mult.as_deref(), Some("*"));
    assert!(has("Loan", "Member", RelationKind::Association));
    assert!(has("LendingService", "LoanRepository", RelationKind::Association));

    // the metrics see the cycle and the layer violation (domain → infra)
    let m = compute(d, false);
    assert!(m.cycles.iter().any(|c| c.contains(&"Loan".to_string()) && c.contains(&"Member".to_string())));
    assert!(m.issues.iter().any(|i| i.kind == "layer" && i.message.contains("Loan->AuditLog")), "{:?}", m.issues.iter().map(|i| &i.message).collect::<Vec<_>>());
}

#[test]
fn explicit_code_root_and_errors() {
    let r = spide_import::import_java(&fixture(), Some("org.acme")).unwrap();
    assert_eq!(r.design.class("Loan").unwrap().package.as_deref(), Some("library.domain"));
    assert!(spide_import::import_java(&std::env::temp_dir().join("spide-nothing-here"), None).is_err());
}

/// Read-only smoke test on a real project when available (zap-extensions next to this repo).
#[test]
fn smoke_on_zap_addon_if_present() {
    let addon = std::env::var("SPIDE_SMOKE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../zap-extensions/addOns/callhome"));
    if !addon.exists() {
        return;
    }
    let t = std::time::Instant::now();
    let r = spide_import::import_java(&addon, None).unwrap();
    let ms = t.elapsed().as_millis();
    eprintln!("{}: {} files, {} classes, {} relations, {} skipped, {ms} ms", addon.display(), r.files, r.design.classes.len(), r.design.relations.len(), r.skipped.len());
    assert!(r.design.classes.len() > 3);
    assert!(ms < 2000, "import too slow: {ms} ms");
    // the code judged on its own (no intended design): what stands out
    let t = std::time::Instant::now();
    let f = compute(&r.design, false);
    eprintln!("{} findings ({} ms)", f.issues.len(), t.elapsed().as_millis());
    for i in f.issues.iter().take(if std::env::var("SPIDE_SMOKE_VERBOSE").is_ok() { 60 } else { 0 }) {
        eprintln!("  [{} {:.2}/{:.2} {}] {}", i.kind, i.severity, i.confidence, i.basis, i.message);
    }
    for c in f.overlap.pairs.iter().filter(|p| p.code >= spide_core::overlap::DUPLICATE).take(10) {
        eprintln!("  clone {} ≈ {} ({:.2})", c.a, c.b, c.code);
    }
}

#[test]
fn sync_between_design_and_code() {
    use spide_core::sync::{self, SyncStatus};
    let manifest = project::Manifest { code_root: Some("org.acme.library".into()), ..Default::default() };
    let reality = spide_import::import_java(&fixture(), Some("org.acme.library")).unwrap().design;
    // the design as the code is: everything in sync
    let a = sync::analyse(&reality, &reality, &manifest, None, false);
    assert!(a.report.items.is_empty(), "{:#?}", a.report.items.iter().map(|i| (&i.id, i.status)).collect::<Vec<_>>());
    // the intended design: planned differently
    let ops: Vec<Op> = serde_json::from_str(
        r#"[{"op":"remove_class","name":"AuditLog"},
            {"op":"add_class","name":"Reservation","package":"domain"},
            {"op":"add_method","class":"Member","method":{"name":"reserve","params":[{"name":"b","type":"Book"}]}},
            {"op":"move_to_package","class":"LoanType","package":"app"}]"#,
    )
    .unwrap();
    let intended = ops::apply_all(&reality, &ops, ops::Mode::Commit(Status::Accepted, None)).unwrap();
    let a = sync::analyse(&intended, &reality, &manifest, None, false);
    let st = |id: &str| a.report.items.iter().find(|i| i.id == id).map(|i| i.status);
    // no baseline yet: every difference is unreconciled
    assert_eq!(st("class:Reservation"), Some(SyncStatus::Unreconciled));
    assert_eq!(st("method:Member.reserve(Book)"), Some(SyncStatus::Unreconciled));
    assert_eq!(st("class:AuditLog"), Some(SyncStatus::Unreconciled));
    // LoanType keeps its explicit link (the imported design carries it): only its package differs
    assert_eq!(st("class:LoanType"), Some(SyncStatus::Unreconciled));
    let lt = a.report.items.iter().find(|i| i.id == "class:LoanType").unwrap();
    assert!(matches!(&lt.suggested_ops[..], [Op::MoveToPackage { package: Some(p), .. }] if p == "domain"), "{:?}", lt.suggested_ops);
    // without a link, the moved class is another type: the code's LoanType keeps its own identity
    let mut unlinked = intended.clone();
    unlinked.class_mut("LoanType").unwrap().source = None;
    let u = sync::analyse(&unlinked, &reality, &manifest, None, false);
    assert!(u.report.items.iter().any(|i| i.id == "class:org.acme.library.domain.LoanType"), "{:?}", u.report.items.iter().map(|i| &i.id).collect::<Vec<_>>());
    // readjusting AuditLog back into the design applies cleanly and puts it in sync
    let audit = a.report.items.iter().find(|i| i.id == "class:AuditLog").unwrap();
    let after = ops::apply_all(&intended, &audit.suggested_ops, ops::Mode::Commit(Status::Accepted, None)).expect("suggested ops apply");
    let a2 = sync::analyse(&after, &reality, &manifest, None, false);
    assert!(!a2.report.items.iter().any(|i| i.class == "AuditLog"), "{:?}", a2.report.items.iter().filter(|i| i.class == "AuditLog").map(|i| &i.id).collect::<Vec<_>>());
    // the brief names files and changes
    let brief = sync::implement_brief(&a, &intended, &reality, &manifest, &["Member".into(), "Reservation".into()], false);
    assert!(brief.contains("add method `reserve(b: Book)`"), "{brief}");
    assert!(brief.contains("src/main/java/org/acme/library/domain/Member.java"), "{brief}");
    assert!(brief.contains("src/main/java/org/acme/library/domain/Reservation.java"), "{brief}");
}

fn course_fixture() -> Design {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/course");
    let r = spide_import::import_java(&dir, None).unwrap();
    assert!(r.skipped.is_empty(), "{:?}", r.skipped);
    r.design
}

#[test]
fn imports_body_facts() {
    let d = course_fixture();
    let m = |c: &str, n: &str| d.class(c).unwrap().methods.iter().find(|x| x.name == n).unwrap().code.clone().unwrap();
    let land = m("Player", "landOn");
    assert_eq!(land.switches, 1);
    assert!(land.switch_on.contains(&"TAX".to_string()), "{:?}", land.switch_on);
    assert!(land.ui_api.iter().any(|u| u.contains("JOptionPane")), "{:?}", land.ui_api);
    assert!(land.file.ends_with("domain/Player.java"), "{}", land.file);
    assert!(land.start_line > 1 && land.end_line > land.start_line);
    let loc = m("GamePanel", "locationOf");
    assert_eq!(loc.max_chain, 3, "{loc:?}");
    assert!(loc.chains[0].contains("getPiece().getLocation().getName()"));
    let worth = m("Game", "netWorth");
    assert!(worth.loops >= 1 && worth.arithmetic >= 1 && worth.comparisons >= 1, "{worth:?}");
    assert!(m("Game", "addPlayer").creates.contains(&"Piece".to_string()));
    assert!(!worth.fingerprint.is_empty());
}

#[test]
fn code_is_judged_like_the_design() {
    let reality = course_fixture();
    // the intended design: Player is a domain class, the panel only presents
    let intended: Design = {
        let ops: Vec<Op> = serde_json::from_str(
            r#"[{"op":"add_package","name":"domain","layer":"domain"},{"op":"add_package","name":"ui","layer":"ui"},
                {"op":"add_class","name":"Player","package":"domain","stereotype":"entity","methods":[{"name":"landOn","params":[{"name":"s","type":"Square"}],"uses":["cash"]}]},
                {"op":"add_class","name":"Square","package":"domain","stereotype":"entity"},
                {"op":"add_class","name":"GamePanel","package":"ui","methods":[{"name":"onNextRound","calls":["Game.playRound"]},{"name":"refresh"}]},
                {"op":"add_class","name":"Game","package":"domain","methods":[{"name":"playRound"}]}]"#,
        )
        .unwrap();
        ops::apply_all(&Design::default(), &ops, ops::Mode::Commit(Status::Accepted, None)).unwrap()
    };
    let _ = intended;
    // the code judged like a design (the same checks run on the implementation's facts)
    let m = compute(&reality, false);
    let kinds = |k: &str| m.issues.iter().filter(|i| i.kind == k).map(|i| i.message.clone()).collect::<Vec<_>>();
    // the same switch on the square kind in two classes → polymorphism
    assert!(!kinds("type_switch").is_empty(), "{:#?}", m.issues.iter().map(|i| (&i.kind, &i.message)).collect::<Vec<_>>());
    // a chain through getters → Law of Demeter
    assert!(kinds("demeter").iter().any(|m| m.contains("GamePanel.locationOf")));
    // Game creates the Piece that Player keeps → Creator
    assert!(kinds("creator").iter().any(|m| m.contains("Piece")), "{:?}", kinds("creator"));
    // the UI computes standings
    assert!(m.issues.iter().any(|i| i.kind == "misplaced_responsibility" && i.targets.contains(&"GamePanel.richestCash".to_string())));
    // two loops summing positive values are the same code
    assert!(m.overlap.pairs.iter().filter(|p| p.code >= overlap::DUPLICATE).any(|p| (p.a.contains("totalPrice") && p.b.contains("netWorth")) || (p.b.contains("totalPrice") && p.a.contains("netWorth"))));
}
