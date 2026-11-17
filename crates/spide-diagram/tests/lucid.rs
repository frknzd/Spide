//! Lucid writer: every diagram becomes a valid target — unique Spide ids, lines between existing
//! items, positive non-overlapping boxes, every live member written, ghosts and pruned left out.
use spide_core::lucid::{diagram_keys, ItemKind, Target};
use spide_core::model::*;
use spide_core::{Design, Store};
use spide_diagram::lucid;
use std::collections::HashSet;

#[test]
fn package_alternatives_match_scope_and_empty_packages_are_drawn() {
    let d = example("rest-extraction");
    let metrics = spide_core::metrics::compute(&d,false);
    for p in d.packages.iter().filter(|p| p.status.is_live()) {
        let scope = spide_core::scope::PackageScope::new(&d,&metrics,&p.name,false).unwrap();
        let target = lucid::target(&d,&format!("class.package:{}",p.name)).unwrap();
        let ids: std::collections::BTreeSet<String> = target.items.iter().filter_map(|i| i.sid.as_deref()?.strip_prefix("class:").map(String::from)).collect();
        assert_eq!(ids,scope.classes,"{}",p.name);
        for item in target.items {
            if let ItemKind::Relationship { from,to,.. } = item.kind {
                if let (Some(from),Some(to)) = (from.strip_prefix("class:"),to.strip_prefix("class:")) { assert!(scope.includes_edge(from,to)); }
            }
        }
    }
    let empty = Design::from_json(r#"{"packages":[{"name":"empty"}]}"#).unwrap();
    let target = lucid::target(&empty,"class.package:empty").unwrap();
    assert!(target.items.iter().any(|i| matches!(&i.kind,ItemKind::Package {name,..} if name == "empty")));
}

pub fn example(name: &str) -> Design {
    let text = std::fs::read_to_string(format!("{}/../../tests/fixtures/{name}.json", env!("CARGO_MANIFEST_DIR"))).unwrap();
    let mut s = Store::default();
    s.load(Design::from_json(&text).unwrap());
    s.view()
}

/// The library design with C4 elements, use cases, a domain model and a sequence diagram.
pub fn rich() -> Design {
    let mut d = example("library");
    let ops: Vec<spide_core::ops::Op> = serde_json::from_value(serde_json::json!([
        {"op":"add_container","name":"Web UI","technology":"JavaFX"},{"op":"add_container","name":"Lending API","technology":"Spring Boot"},
        {"op":"add_container","name":"DB","technology":"Postgres","datastore":true},
        {"op":"add_system","name":"Mail","description":"sends reminders"},
        {"op":"assign_container","package":"ui","container":"Web UI"},{"op":"assign_container","package":"application","container":"Lending API"},
        {"op":"assign_container","package":"domain","container":"Lending API"},{"op":"assign_container","package":"infrastructure","container":"Lending API"},
        {"op":"add_link","from":"Lending API","to":"DB","protocol":"JDBC"},
        {"op":"add_link","from":"Lending API","to":"Mail","protocol":"SMTP","interaction":"async"}
    ]))
    .unwrap();
    d = spide_core::ops::apply_all(&d, &ops, spide_core::ops::Mode::Commit(Status::Accepted, None)).unwrap();
    d.use_cases = UseCaseModel {
        system: Some("Library".into()),
        actors: vec![Actor { name: "Member".into(), ..Default::default() }],
        use_cases: vec![
            UseCase { name: "Borrow book".into(), actors: vec!["Member".into()], includes: vec!["Check membership".into()], ..Default::default() },
            UseCase { name: "Check membership".into(), ..Default::default() },
        ],
        generated_rev: None,
    };
    d.domain = DomainModel {
        concepts: vec![
            Concept { name: "Book".into(), attributes: vec![ConceptAttribute { name: "isbn".into(), ty: Some("ISBN".into()), multiplicity: None }], ..Default::default() },
            Concept { name: "Loan".into(), attributes: vec![ConceptAttribute { name: "due".into(), ty: Some("Date".into()), multiplicity: Some("1".into()) }], ..Default::default() },
        ],
        associations: vec![ConceptLink { id: "a1".into(), from: "Loan".into(), to: "Book".into(), label: Some("lends".into()), from_mult: Some("*".into()), to_mult: Some("1".into()), ..Default::default() }],
        generated_rev: None,
    };
    d.sequences = vec![Sequence {
        name: "borrow".into(),
        participants: vec!["Member".into(), "Loan".into()],
        messages: vec![Message { from: "Member".into(), to: "Loan".into(), method: "Loan".into(), kind: MessageKind::Create, ..Default::default() }],
        ..Default::default()
    }];
    d
}

fn valid(t: &Target) {
    let mut sids = HashSet::new();
    for it in &t.items {
        let sid = it.sid.as_deref().expect("every target item has a spide id");
        assert!(sids.insert(sid.to_string()), "{}: duplicate {sid}", t.key);
    }
    for it in &t.items {
        if let Some((a, b)) = it.kind.ends() {
            assert!(sids.contains(a) && sids.contains(b), "{}: line {:?} ends at missing items", t.key, it.sid);
        } else if !matches!(it.kind, ItemKind::Sequence { .. }) {
            let b = it.bbox.unwrap_or_else(|| panic!("{}: {:?} has no box", t.key, it.sid));
            assert!(b.w > 0.0 && b.h > 0.0, "{}: {:?} {b:?}", t.key, it.sid);
        }
    }
    // leaf boxes don't overlap
    let leaves: Vec<_> = t.items.iter().filter(|i| matches!(i.kind, ItemKind::Class { .. } | ItemKind::Block { .. } | ItemKind::Actor { .. } | ItemKind::UseCase { .. })).filter_map(|i| i.bbox.map(|b| (i.sid.clone(), b))).collect();
    for (i, (a, x)) in leaves.iter().enumerate() {
        for (b, y) in &leaves[i + 1..] {
            let overlap = x.x < y.x + y.w - 1.0 && y.x < x.x + x.w - 1.0 && x.y < y.y + y.h - 1.0 && y.y < x.y + x.h - 1.0;
            assert!(!overlap, "{}: {a:?} {x:?} overlaps {b:?} {y:?}", t.key);
        }
    }
}

#[test]
fn every_diagram_is_a_valid_target() {
    let d = rich();
    let keys = diagram_keys(&d);
    for k in ["system.context", "system.containers", "class", "domain", "package", "usecase", "sequence:borrow"] {
        assert!(keys.contains(&k.to_string()), "{k} in {keys:?}");
    }
    let ts = lucid::targets(&d);
    assert_eq!(ts.len(), keys.len());
    for t in &ts {
        valid(t);
        assert!(t.title.starts_with(&d.name));
    }
}

#[test]
fn class_diagram_writes_every_live_member_and_relation() {
    let d = rich();
    let t = lucid::target(&d, "class").unwrap();
    for c in d.live_classes() {
        let it = t.item(&format!("class:{}", c.name)).unwrap_or_else(|| panic!("class {}", c.name));
        let ItemKind::Class { attributes, operations, .. } = &it.kind else { panic!() };
        assert_eq!(attributes.len(), c.attributes.iter().filter(|a| a.status.is_live()).count());
        assert_eq!(operations.len(), c.methods.iter().filter(|m| m.status.is_live()).count());
        assert!(it.fill.is_some() || c.package.is_none(), "layer colour for {}", c.name);
    }
    assert_eq!(t.items.iter().filter(|i| matches!(i.kind, ItemKind::Relationship { .. })).count(), d.live_relations().count());
    // package frames contain their classes
    for it in t.items.iter().filter(|i| matches!(i.kind, ItemKind::Package { .. })) {
        let pkg = it.sid.as_deref().unwrap().strip_prefix("package:").unwrap();
        let fb = it.bbox.unwrap();
        for c in d.live_classes().filter(|c| c.package.as_deref() == Some(pkg)) {
            let cb = t.item(&format!("class:{}", c.name)).unwrap().bbox.unwrap();
            assert!(fb.contains_point(cb.centre()), "{} inside {pkg}", c.name);
        }
    }
}

#[test]
fn ghosts_and_pruned_are_left_out() {
    let mut d = rich();
    let ops: Vec<spide_core::ops::Op> = serde_json::from_value(serde_json::json!([{"op":"add_class","name":"Ghost"}])).unwrap();
    d = spide_core::ops::apply_all(&d, &ops, spide_core::ops::Mode::Commit(Status::Proposed, Some("P1".into()))).unwrap();
    let t = lucid::target(&d, "class").unwrap();
    assert!(t.item("class:Ghost").is_none());
}

#[test]
fn writer_is_deterministic() {
    let d = rich();
    assert_eq!(lucid::targets(&d), lucid::targets(&d));
}

#[test]
#[ignore]
fn dump() {
    for t in lucid::targets(&rich()) {
        println!("== {} ({})", t.key, t.title);
        for i in &t.items {
            println!("{:?} {:?} {}", i.sid, i.bbox.map(|b| (b.x, b.y, b.w, b.h)), i.kind.canonical().chars().take(140).collect::<String>());
        }
    }
}

// ------------------------------------------------------------------ pull mapping

use spide_core::lucid::map::pull;
use spide_core::lucid::snapshot::Snapshot;
use spide_core::lucid::{Bbox, Item};
use spide_core::ops::Op;

fn snap(items: Vec<Item>) -> Snapshot {
    Snapshot { items, ..Default::default() }
}

fn item_mut<'a>(items: &'a mut [Item], sid: &str) -> &'a mut Item {
    items.iter_mut().find(|i| i.sid.as_deref() == Some(sid)).unwrap_or_else(|| panic!("{sid}"))
}

#[test]
fn unchanged_documents_pull_nothing() {
    let d = rich();
    for t in lucid::targets(&d) {
        let p = pull(&d, &t.key, &t.items, &snap(t.items.clone()));
        assert!(p.is_empty(), "{}: {:?}", t.key, p);
        assert!(p.warnings.is_empty(), "{}: {:?}", t.key, p.warnings);
    }
}

#[test]
fn class_edits_map_to_exact_ops() {
    let d = rich();
    let base = lucid::target(&d, "class").unwrap().items;
    let mut remote = base.clone();
    // rename Loan → Lending, add an attribute, change a method
    if let ItemKind::Class { name, attributes, operations, .. } = &mut item_mut(&mut remote, "class:Loan").kind {
        *name = name.replace("Loan", "Lending");
        attributes.push("- fee: Money [0..1]".into());
        operations[0] = operations[0].replace("today: Date", "today: LocalDate");
    }
    // delete a relation
    remote.retain(|i| i.sid.as_deref() != Some("rel:Member->LoanController:dependency"));
    // a colleague's new class + inheritance line to Book
    let dom = item_mut(&mut remote, "package:domain").bbox.unwrap();
    remote.push(Item { sid: None, lucid_id: Some("new1".into()), kind: ItemKind::Class { name: "EBook".into(), attributes: vec!["+ url: URL".into()], operations: vec![], interface: false }, bbox: Some(Bbox { x: dom.x + dom.w - 200.0, y: dom.y + dom.h - 100.0, w: 160.0, h: 80.0 }), fill: None });
    remote.push(Item { sid: None, lucid_id: Some("new2".into()), kind: ItemKind::Relationship { from: "lucid:new1".into(), to: "class:Book".into(), rel: "generalization".into(), label: String::new(), from_mult: String::new(), to_mult: String::new() }, bbox: None, fill: None });
    // move NotificationLog into the domain frame
    let nl = item_mut(&mut remote, "class:NotificationLog");
    let b = nl.bbox.unwrap();
    nl.bbox = Some(Bbox { x: dom.x + 30.0, y: dom.y + dom.h - b.h - 30.0, ..b });
    let p = pull(&d, "class", &base, &snap(remote));
    assert!(p.warnings.is_empty(), "{:?}", p.warnings);
    assert_eq!(p.changes[0], Op::RenameClass { from: "Loan".into(), to: "Lending".into() });
    let has = |ops: &[Op], f: &dyn Fn(&Op) -> bool| ops.iter().any(f);
    assert!(has(&p.changes, &|o| matches!(o, Op::AddAttribute { class, attribute } if class == "Lending" && attribute.name == "fee" && attribute.ty == "Money" && attribute.multiplicity.as_deref() == Some("0..1"))), "{:#?}", p.changes);
    assert!(has(&p.changes, &|o| matches!(o, Op::UpdateMethod { class, method, .. } if class == "Lending" && method.params[0].ty == "LocalDate")));
    assert!(has(&p.changes, &|o| matches!(o, Op::AddClass { name, package, attributes, .. } if name == "EBook" && package.as_deref() == Some("domain") && attributes.len() == 1)), "{:#?}", p.changes);
    assert!(has(&p.changes, &|o| matches!(o, Op::AddRelation { from, to, kind, .. } if from == "EBook" && to == "Book" && *kind == RelationKind::Inheritance)));
    assert!(has(&p.changes, &|o| matches!(o, Op::UpdateClass { name, package, .. } if name == "NotificationLog" && package.as_deref() == Some("domain"))), "{:#?}", p.changes);
    assert_eq!(p.removals, vec![Op::RemoveRelation { from: "Member".into(), to: "LoanController".into(), kind: Some(RelationKind::Dependency) }]);
    assert!(p.conflicts.is_empty());
    // the moved box is a layout change in the canvas' coordinates
    let Some(Op::SetLayout { view, positions }) = p.layout.first() else { panic!("{:?}", p.layout) };
    assert_eq!(view, "class");
    assert!(positions.contains_key("NotificationLog"));
    // and nothing else of the class (descriptions, code links …) changes: applying gives the same members otherwise
    let all: Vec<Op> = p.changes.iter().chain(&p.removals).cloned().collect();
    let next = spide_core::ops::apply_all(&d, &all, spide_core::ops::Mode::Commit(Status::Accepted, None)).unwrap();
    let a = next.class("Lending").unwrap();
    let b = d.class("Loan").unwrap();
    assert_eq!(a.responsibilities, b.responsibilities);
    assert_eq!(a.methods[0].description, b.methods[0].description);
}

#[test]
fn local_work_is_kept_and_both_sided_edits_conflict() {
    let d0 = rich();
    let base = lucid::target(&d0, "class").unwrap().items;
    // here: Book gains an attribute and Member's attributes change; there: Member's attributes change too, and Catalog changes
    let ops: Vec<Op> = serde_json::from_value(serde_json::json!([
        {"op":"add_attribute","class":"Book","attribute":{"name":"year","type":"int"}},
        {"op":"update_attribute","class":"Member","name":"name","attribute":{"name":"name","type":"FullName","visibility":"private"}}
    ])).unwrap();
    let d = spide_core::ops::apply_all(&d0, &ops, spide_core::ops::Mode::Commit(Status::Accepted, None)).unwrap();
    let mut remote = base.clone();
    if let ItemKind::Class { attributes, .. } = &mut item_mut(&mut remote, "class:Member").kind {
        for a in attributes.iter_mut().filter(|a| a.contains("name:")) {
            *a = "- name: PersonName".into();
        }
    }
    if let ItemKind::Class { operations, .. } = &mut item_mut(&mut remote, "class:Catalog").kind {
        operations.push("+ count(): int".into());
    }
    let p = pull(&d, "class", &base, &snap(remote));
    // Book.year (local) is not removed; Catalog.count (remote) is added
    assert!(!p.removals.iter().any(|o| matches!(o, Op::RemoveAttribute { name, .. } if name == "year")), "{:?}", p.removals);
    assert!(p.changes.iter().any(|o| matches!(o, Op::AddMethod { class, method } if class == "Catalog" && method.name == "count")));
    // Member.name changed on both sides: a conflict carrying the remote version
    assert!(!p.changes.iter().any(|o| matches!(o, Op::UpdateAttribute { class, .. } if class == "Member")));
    assert!(p.conflicts.iter().any(|o| matches!(o, Op::UpdateAttribute { class, attribute, .. } if class == "Member" && attribute.ty == "PersonName")), "{:?}", p.conflicts);
}

#[test]
fn derived_views_pull_as_view_ops() {
    let d = rich();
    // domain: an attribute added in Lucid
    let base = lucid::target(&d, "domain").unwrap().items;
    let mut remote = base.clone();
    if let ItemKind::Class { attributes, .. } = &mut item_mut(&mut remote, "concept:Book").kind {
        attributes.push("title: Text".into());
    }
    let p = pull(&d, "domain", &base, &snap(remote));
    let Some(Op::SetDomainModel { model }) = p.changes.first() else { panic!("{p:?}") };
    assert_eq!(model.concepts.iter().find(|c| c.name == "Book").unwrap().attributes.len(), 2);
    assert_eq!(model.associations.len(), 1);
    // use cases: a new use case linked to the actor
    let base = lucid::target(&d, "usecase").unwrap().items;
    let mut remote = base.clone();
    remote.push(Item { sid: None, lucid_id: Some("u1".into()), kind: ItemKind::UseCase { name: "Return book".into() }, bbox: Some(Bbox { x: 400.0, y: 300.0, w: 200.0, h: 70.0 }), fill: None });
    remote.push(Item { sid: None, lucid_id: Some("l1".into()), kind: ItemKind::Relationship { from: "actor:Member".into(), to: "lucid:u1".into(), rel: "association".into(), label: String::new(), from_mult: String::new(), to_mult: String::new() }, bbox: None, fill: None });
    let p = pull(&d, "usecase", &base, &snap(remote));
    let Some(Op::SetUseCases { model }) = p.changes.first() else { panic!("{p:?}") };
    assert_eq!(model.use_cases.iter().find(|u| u.name == "Return book").unwrap().actors, vec!["Member".to_string()]);
    assert_eq!(model.use_cases.iter().find(|u| u.name == "Borrow book").unwrap().includes, vec!["Check membership".to_string()]);
    // sequence: a message added to the markup
    let base = lucid::target(&d, "sequence:borrow").unwrap().items;
    let mut remote = base.clone();
    if let ItemKind::Sequence { markup } = &mut remote[0].kind {
        markup.push_str("\nMember -> Loan: renew()");
    }
    let p = pull(&d, "sequence:borrow", &base, &snap(remote));
    let Some(Op::SetSequence { sequence }) = p.changes.first() else { panic!("{p:?}") };
    assert_eq!(sequence.messages.len(), 2);
    assert_eq!(sequence.messages[1].method, "renew");
    // C4: a container's technology changed and a new external system
    let base = lucid::target(&d, "system.containers").unwrap().items;
    let mut remote = base.clone();
    if let ItemKind::Block { text, .. } = &mut item_mut(&mut remote, "c4:DB").kind {
        *text = text.replace("Postgres", "MySQL");
    }
    remote.push(Item { sid: None, lucid_id: Some("s1".into()), kind: ItemKind::Block { shape: spide_core::lucid::BlockShape::System, text: "Payments\n[Software System]".into() }, bbox: Some(Bbox { x: 0.0, y: 900.0, w: 200.0, h: 80.0 }), fill: None });
    let p = pull(&d, "system.containers", &base, &snap(remote));
    assert!(p.changes.iter().any(|o| matches!(o, Op::UpdateContainer { name, technology, .. } if name == "DB" && technology.as_deref() == Some("MySQL"))), "{:?}", p.changes);
    assert!(p.changes.iter().any(|o| matches!(o, Op::AddSystem { name, .. } if name == "Payments")));
}

#[test]
fn notes_become_comments_near_their_shape() {
    let d = rich();
    let base = lucid::target(&d, "class").unwrap().items;
    let mut remote = base.clone();
    let b = item_mut(&mut remote, "class:Catalog").bbox.unwrap();
    remote.push(Item { sid: None, lucid_id: Some("n1".into()), kind: ItemKind::Other { class_name: "StickyNoteBlock".into(), text: "Should Catalog cache?".into() }, bbox: Some(Bbox { x: b.x, y: b.y - 70.0, w: 120.0, h: 60.0 }), fill: None });
    let p = pull(&d, "class", &base, &snap(remote));
    assert!(p.is_empty());
    assert_eq!(p.notes.len(), 1);
    assert_eq!(p.notes[0].near.as_deref(), Some("class:Catalog"));
}

#[test]
fn pulls_become_separate_lucid_proposals() {
    let d = rich();
    let base = lucid::target(&d, "class").unwrap().items;
    let mut remote = base.clone();
    if let ItemKind::Class { attributes, .. } = &mut item_mut(&mut remote, "class:Book").kind {
        attributes.pop();
        attributes.push("- pages: int".into());
    }
    let p = pull(&d, "class", &base, &snap(remote));
    let mut store = Store::default();
    store.load(d);
    let ids = spide_core::lucid::map::propose(&mut store, "Library · Class diagram", &p).unwrap();
    assert_eq!(ids.len(), 2, "changes and removals");
    assert!(store.design.proposals.iter().all(|x| x.source == "lucid"));
    for id in ids {
        store.accept_proposal(&id).unwrap();
    }
    let b = store.design.class("Book").unwrap();
    assert!(b.attributes.iter().any(|a| a.name == "pages") && !b.attributes.iter().any(|a| a.name == "author"));
}

#[test]
fn derived_view_ops_ghost_and_accept() {
    let d = rich();
    let base = lucid::target(&d, "domain").unwrap().items;
    let mut remote = base.clone();
    remote.retain(|i| i.sid.as_deref() != Some("assoc:a1"));
    if let ItemKind::Class { name, .. } = &mut item_mut(&mut remote, "concept:Loan").kind {
        *name = "Lending".into();
    }
    let p = pull(&d, "domain", &base, &snap(remote));
    let mut store = Store::default();
    store.load(d);
    let ids = spide_core::lucid::map::propose(&mut store, "domain", &p).unwrap();
    // the ghost view shows the new concept as proposed and the old one as being removed
    let v = store.view();
    assert_eq!(v.domain.concepts.iter().find(|c| c.name == "Lending").unwrap().status, Status::Proposed);
    assert_eq!(v.domain.concepts.iter().find(|c| c.name == "Loan").unwrap().status, Status::Removing);
    store.accept_proposal(&ids[0]).unwrap();
    let names: Vec<&str> = store.design.domain.concepts.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, vec!["Book", "Lending"]);
    assert!(store.design.domain.associations.is_empty());
}

/// Live check helpers (run by hand against a real Lucid document):
/// `LUCID_OUT=dir cargo test --test lucid live_scripts -- --ignored` writes each diagram's create
/// script; `LUCID_READ=file LUCID_KEY=class` checks a read-back maps to no changes.
#[test]
#[ignore]
fn live_scripts() {
    let mut d = rich();
    if std::env::var("LUCID_MUTATED").is_ok() {
        let ops: Vec<Op> = serde_json::from_value(serde_json::json!([
            {"op":"rename_class","from":"Loan","to":"Lending"},
            {"op":"add_attribute","class":"Lending","attribute":{"name":"fee","type":"Money","multiplicity":"0..1"}},
            {"op":"remove_relation","from":"Member","to":"LoanController","kind":"dependency"},
            {"op":"add_class","name":"EBook","package":"domain","attributes":[{"name":"url","type":"URL"}]},
            {"op":"add_relation","from":"EBook","to":"Book","kind":"inheritance"},
            {"op":"update_relation","from":"Catalog","to":"Book","kind":"composition","label":"holds","to_mult":"0..*"}
        ])).unwrap();
        d = spide_core::ops::apply_all(&d, &ops, spide_core::ops::Mode::Commit(Status::Accepted, None)).unwrap();
        d.layout.entry("class".into()).or_default().insert("NotificationLog".into(), Pos { x: 600.0, y: -400.0, pinned: true });
    }
    if let (Ok(dir), Err(_)) = (std::env::var("LUCID_OUT"), std::env::var("LUCID_READ")) {
        for t in lucid::targets(&d) {
            let p = spide_core::lucid::patch::plan(&Snapshot::default(), &t);
            for (i, c) in spide_core::lucid::patch::chunks(&p, spide_core::lucid::patch::CHUNK).iter().enumerate() {
                std::fs::write(format!("{dir}/{}-{i}.js", spide_core::lucid::state::file_key(&t.key)), spide_core::lucid::patch::script(c)).unwrap();
            }
        }
    }
    if let (Ok(file), Ok(key)) = (std::env::var("LUCID_READ"), std::env::var("LUCID_KEY")) {
        let raw: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap();
        let s = spide_core::lucid::snapshot::from_raw(&raw).unwrap();
        let t = lucid::target_anchored(&d, &key, Some(&s)).unwrap();
        let p = spide_core::lucid::patch::plan(&s, &t);
        println!("PATCH {}", serde_json::to_string_pretty(&p).unwrap());
        if let Ok(dir) = std::env::var("LUCID_OUT") {
            std::fs::write(format!("{dir}/patch.js"), spide_core::lucid::patch::script(&p)).unwrap();
        }
        let pl = pull(&d, &key, &t.items, &s);
        println!("PULL changes={:?}\nremovals={:?}\nlayout={:?}\nwarnings={:?}", pl.changes, pl.removals, pl.layout, pl.warnings);
    }
}

/// Documents Spide created and patched in a real Lucid account, read back with the read script:
/// each maps to no patch and no pull (fixtures in spide-core/tests/fixtures/lucid).
#[test]
fn live_documents_round_trip() {
    let fx = |n: &str| -> Snapshot {
        let raw: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(format!("{}/../spide-core/tests/fixtures/lucid/{n}.json", env!("CARGO_MANIFEST_DIR"))).unwrap()).unwrap();
        spide_core::lucid::snapshot::from_raw(&raw).unwrap()
    };
    let d = rich();
    for (file, key) in [("live-domain", "domain"), ("live-package", "package"), ("live-sequence-borrow", "sequence:borrow"), ("live-system.containers", "system.containers"), ("live-system.context", "system.context"), ("live-usecase", "usecase")] {
        let s = fx(file);
        let t = lucid::target_anchored(&d, key, Some(&s)).unwrap();
        assert!(spide_core::lucid::patch::plan(&s, &t).is_empty(), "{key}: {:?}", spide_core::lucid::patch::plan(&s, &t));
        let p = pull(&d, key, &t.items, &s);
        assert!(p.is_empty() && p.warnings.is_empty(), "{key}: {p:?}");
    }
    // the class diagram after an in-place patch (rename, new class, moved box, changed relation)
    let ops: Vec<Op> = serde_json::from_value(serde_json::json!([
        {"op":"rename_class","from":"Loan","to":"Lending"},
        {"op":"add_attribute","class":"Lending","attribute":{"name":"fee","type":"Money","multiplicity":"0..1"}},
        {"op":"remove_relation","from":"Member","to":"LoanController","kind":"dependency"},
        {"op":"add_class","name":"EBook","package":"domain","attributes":[{"name":"url","type":"URL"}]},
        {"op":"add_relation","from":"EBook","to":"Book","kind":"inheritance"},
        {"op":"update_relation","from":"Catalog","to":"Book","kind":"composition","label":"holds","to_mult":"0..*"}
    ])).unwrap();
    let mut m = spide_core::ops::apply_all(&d, &ops, spide_core::ops::Mode::Commit(Status::Accepted, None)).unwrap();
    m.layout.entry("class".into()).or_default().insert("NotificationLog".into(), Pos { x: 600.0, y: -400.0, pinned: true });
    let s = fx("live-class-mutated");
    let t = lucid::target_anchored(&m, "class", Some(&s)).unwrap();
    assert!(spide_core::lucid::patch::plan(&s, &t).is_empty());
    assert!(s.by_sid("class:Lending").is_some() && s.by_sid("class:Loan").is_none(), "renamed in place");
    let p = pull(&m, "class", &t.items, &s);
    assert!(p.is_empty() && p.warnings.is_empty(), "{p:?}");
}
