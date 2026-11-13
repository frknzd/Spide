mod common;
use common::golden;
use spide_core::metrics::compute;
use spide_core::*;
use std::path::PathBuf;
use std::process::Command;

fn fixture() -> Design {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/library.json");
    let mut d = Design::from_json(&std::fs::read_to_string(root).unwrap()).unwrap();
    d.use_cases = serde_json::from_str(
        r#"{"system":"Library","actors":[{"name":"Member"},{"name":"Librarian"}],
            "use_cases":[{"name":"Borrow book","actors":["Member"],"includes":["Check availability"],"classes":["LendingService"]},
                         {"name":"Check availability","actors":[]},
                         {"name":"Manage catalog","actors":["Librarian"],"classes":["Catalog"]}]}"#,
    )
    .unwrap();
    d.sequences = vec![serde_json::from_str(
        r#"{"name":"Borrow book","participants":["Member","LoanController","LendingService","LoanRepository"],
            "messages":[{"from":"Member","to":"LoanController","method":"borrow","args":"id, isbn"},
                        {"from":"LoanController","to":"LendingService","method":"lend"},
                        {"from":"LendingService","to":"LoanRepository","method":"save"},
                        {"from":"LoanRepository","to":"LendingService","method":"ok","kind":"return"},
                        {"from":"LendingService","to":"LoanRepository","method":"explode"},
                        {"from":"LendingService","to":"LoanController","method":"loan","kind":"return"}]}"#,
    )
    .unwrap()];
    // a layout so direction hints are emitted
    let mut lay = std::collections::BTreeMap::new();
    for (i, c) in d.classes.iter().enumerate() {
        lay.insert(c.name.clone(), Pos { x: (i % 3) as f64 * 300.0, y: (i / 3) as f64 * 250.0, pinned: false });
    }
    d.layout.insert("class".into(), lay);
    // the full level keeps every member (the design level is tested separately)
    d.settings.detail.insert("class".into(), 4);
    d
}

/// If PlantUML is installed, make sure it accepts the output.
fn plantuml_accepts(name: &str, text: &str) {
    let Ok(out) = Command::new("plantuml").arg("-version").output() else { return };
    if !out.status.success() {
        return;
    }
    let dir = std::env::temp_dir().join("spide-puml");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join(name);
    std::fs::write(&file, text).unwrap();
    let r = Command::new("plantuml").args(["-checkonly", "-failfast2"]).arg(&file).output().unwrap();
    assert!(r.status.success(), "plantuml rejected {name}:\n{}\n{}", String::from_utf8_lossy(&r.stdout), String::from_utf8_lossy(&r.stderr));
}

#[test]
fn plantuml_exports_are_stable_and_valid() {
    let d = fixture();
    let m = compute(&d, false);
    for view in ["class", "domain", "package", "usecase", "sequence"] {
        let text = plantuml::export(view, &d, &m, &d.layout).unwrap();
        let name = format!("library-{view}.puml");
        golden(&name, &text);
        plantuml_accepts(&name, &text);
    }
}

#[test]
fn class_export_follows_the_detail_level() {
    let mut d = fixture();
    // `loans: LoanRepository` restates a drawn association; add an accessor and a private helper
    let lending = d.classes.iter_mut().find(|c| c.name == "LendingService").unwrap();
    lending.methods.push(serde_json::from_value(serde_json::json!({"name": "getLoans", "returns": "LoanRepository", "uses": ["loans"]})).unwrap());
    lending.methods.push(serde_json::from_value(serde_json::json!({"name": "check", "visibility": "private", "params": [{"name": "m", "type": "Member"}]})).unwrap());
    d.settings.detail.insert("class".into(), 3);
    let m = compute(&d, false);
    let t = plantuml::export("class", &d, &m, &d.layout).unwrap();
    golden("library-class-design.puml", &t);
    plantuml_accepts("library-class-design.puml", &t);
    assert!(!t.contains("loans : LoanRepository"), "association attribute folded:\n{t}");
    assert!(!t.contains("getLoans"), "accessor folded");
    assert!(t.contains(".. + 1 accessor, 1 private helper .."), "{t}");
    d.settings.detail.insert("class".into(), 1);
    let t = plantuml::export("class", &d, &compute(&d, false), &d.layout).unwrap();
    assert!(!t.contains("+lend("), "names only");
    d.settings.detail.insert("class".into(), 0);
    let t = plantuml::export("class", &d, &compute(&d, false), &d.layout).unwrap();
    assert!(t.contains("Package diagram"), "packages only");
}

#[test]
fn class_export_is_semantically_complete() {
    let d = fixture();
    let m = compute(&d, false);
    let t = plantuml::export("class", &d, &m, &d.layout).unwrap();
    // members with visibility and types
    assert!(t.contains("-isbn : String"));
    assert!(t.contains("+lend(member : Member, copy : BookCopy) : Loan"));
    // stereotypes and packages with layers
    assert!(t.contains("class LendingService <<service>>"));
    assert!(t.contains("package application"));
    // relation notation + multiplicities
    assert!(t.contains("Catalog \"1\" *-"), "{t}");
    assert!(t.contains("\"*\" Book"));
    assert!(t.contains("Member \"1\" -"), "bidirectional association");
    // cycles highlighted
    assert!(t.contains("#FF5C6C"), "cycle edges coloured");
    // direction hints
    assert!(t.contains("down") || t.contains("right"));
}

#[test]
fn sequence_export_flags_invalid_messages() {
    let d = fixture();
    let m = compute(&d, false);
    let t = plantuml::export("sequence", &d, &m, &d.layout).unwrap();
    assert!(t.contains("actor Member"));
    assert!(t.contains("LoanRepository.explode() does not exist"));
    assert!(t.contains("1: borrow(id, isbn)"));
}

#[test]
fn png_render_works() {
    let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100" viewBox="0 0 200 100">
      <rect width="200" height="100" fill="#1d1e22"/>
      <text x="20" y="50" font-family="Helvetica Neue" font-size="14" fill="#e6e8ee">Order</text></svg>"##;
    let png = png::render(svg, 2.0).unwrap();
    assert_eq!(&png[1..4], b"PNG");
    // 2x scale → 400px wide (IHDR width at bytes 16..20)
    let w = u32::from_be_bytes([png[16], png[17], png[18], png[19]]);
    assert_eq!(w, 400);
}

#[test]
fn c4_export_is_valid() {
    let mut d = fixture();
    let ops: Vec<Op> = serde_json::from_str(
        r#"[{"op":"add_container","name":"Web UI","technology":"JavaFX"},{"op":"add_container","name":"Lending API","technology":"Spring Boot"},
            {"op":"add_container","name":"DB","technology":"Postgres","datastore":true},
            {"op":"add_system","name":"Mail","description":"sends reminders"},
            {"op":"assign_container","package":"ui","container":"Web UI"},{"op":"assign_container","package":"application","container":"Lending API"},
            {"op":"assign_container","package":"domain","container":"Lending API"},{"op":"assign_container","package":"infrastructure","container":"Lending API"},
            {"op":"add_link","from":"Lending API","to":"DB","protocol":"JDBC"},
            {"op":"add_link","from":"Lending API","to":"Mail","protocol":"SMTP","interaction":"async","description":"reminders"},
            {"op":"add_link","from":"Member","to":"Web UI","protocol":"desktop"}]"#,
    )
    .unwrap();
    d = ops::apply_all(&d, &ops, ops::Mode::Commit(Status::Accepted, None)).unwrap();
    let m = compute(&d, false);
    let text = plantuml::export("system", &d, &m, &d.layout).unwrap();
    assert!(text.contains("ContainerDb(DB"));
    assert!(text.contains("Rel(Web_UI, Lending_API"), "derived container dependency");
    assert!(text.contains("Rel(Library_system, Mail"), "context collapses containers into the system");
    golden("library-system.puml", &text);
    plantuml_accepts("library-system.puml", &text);
}
