//! Spide Project sync: exact identity, comparable shapes, three-way status against the baseline.
use spide_core::project::{Compare, Manifest};
use spide_core::sync::{self, Baseline, SyncStatus};
use spide_core::*;

fn ops(json: &str) -> Vec<Op> {
    serde_json::from_str(json).expect("ops parse")
}

fn apply(d: &Design, json: &str) -> Design {
    ops::apply_all(d, &ops(json), ops::Mode::Commit(Status::Accepted, None)).unwrap()
}

fn manifest() -> Manifest {
    Manifest { code_root: Some("org.shop".into()), ..Default::default() }
}

/// The design: Order (domain) with a total() and lines drawn as a composition to Line.
fn design() -> Design {
    apply(
        &Design::default(),
        r#"[{"op":"add_class","name":"Order","package":"domain","attributes":[{"name":"id","type":"String"}],
             "methods":[{"name":"total","returns":"Money"},{"name":"add","params":[{"name":"l","type":"Line"}]}]},
            {"op":"add_class","name":"Line","package":"domain","attributes":[{"name":"qty"}]},
            {"op":"add_relation","from":"Order","to":"Line","kind":"composition","to_mult":"*"}]"#,
    )
}

fn sym(s: &str, file: &str) -> Option<SourceSymbol> {
    Some(SourceSymbol { symbol: s.into(), file: file.into(), start_line: 3, end_line: 30, ..Default::default() })
}

/// The code as spide-import would give it.
fn code() -> Design {
    let mut c = apply(
        &Design::default(),
        r#"[{"op":"add_class","name":"Order","package":"domain",
             "attributes":[{"name":"id","type":"String","visibility":"private"},{"name":"lines","type":"List<Line>","visibility":"private"},
                           {"name":"LOG","type":"Logger","visibility":"private","is_static":true}],
             "methods":[{"name":"total","returns":"Money"},{"name":"add","params":[{"name":"line","type":"Line"}],"returns":"void"},
                        {"name":"Order"},{"name":"recalc","visibility":"private"},{"name":"getId","returns":"String","uses":["id"]}]},
            {"op":"add_class","name":"Line","package":"domain","attributes":[{"name":"qty","type":"int","visibility":"private"}]}]"#,
    );
    c.settings.code_root = Some("org.shop".into());
    for cl in c.classes.iter_mut() {
        let fqn = format!("org.shop.domain.{}", cl.name);
        let file = format!("src/main/java/org/shop/domain/{}.java", cl.name);
        cl.source = sym(&fqn, &file);
        for m in cl.methods.iter_mut() {
            m.code = Some(CodeFacts { statements: 1, loc: 3, file: file.clone(), start_line: 10, ..Default::default() });
            m.source = sym(&format!("{fqn}#{}()", m.name), &file);
        }
    }
    c
}

fn status(a: &sync::Analysis, id: &str) -> SyncStatus {
    a.report.items.iter().find(|i| i.id == id).map(|i| i.status).unwrap_or(SyncStatus::InSync)
}

#[test]
fn identity_wildcards_filters_and_association_fields() {
    let a = sync::analyse(&design(), &code(), &manifest(), None, false);
    // implicit FQN org.shop.domain.Order; untyped `qty` matches `int`; the composition stands for `lines`;
    // the constructor, the private helper, the accessor and the static logger are not compared
    assert!(a.report.items.is_empty(), "{:#?}", a.report.items.iter().map(|i| (&i.id, i.status, &i.design, &i.code)).collect::<Vec<_>>());
    assert_eq!(a.report.count(SyncStatus::InSync), a.design.len());
    assert_eq!(a.code_class["Order"], "Order");
    // turning the filters off brings them in
    let all = Manifest { compare: Compare { private_methods: true, accessors: true, constructors: true }, ..manifest() };
    let b = sync::analyse(&design(), &code(), &all, None, false);
    for id in ["method:Order.Order()", "method:Order.recalc()", "method:Order.getId()"] {
        assert_eq!(status(&b, id), SyncStatus::Unreconciled, "{id}");
    }
}

#[test]
fn same_simple_name_elsewhere_never_matches_but_explicit_links_do() {
    let mut code = code();
    // the code's Order lives in `billing`: another type
    let o = code.class_mut("Order").unwrap();
    o.package = Some("billing".into());
    o.source = sym("org.shop.billing.Order", "src/main/java/org/shop/billing/Order.java");
    let a = sync::analyse(&design(), &code, &manifest(), None, false);
    assert_eq!(status(&a, "class:Order"), SyncStatus::Unreconciled, "design-only");
    assert_eq!(status(&a, "class:org.shop.billing.Order"), SyncStatus::Unreconciled, "code-only, named by its FQN");
    assert_eq!(a.report.classes["Order"].in_code, false);
    // an explicit link says they are the same: only the package differs
    let mut d = design();
    d.class_mut("Order").unwrap().source = Some(SourceSymbol { symbol: "org.shop.billing.Order".into(), ..Default::default() });
    let b = sync::analyse(&d, &code, &manifest(), None, false);
    assert_eq!(status(&b, "class:Order"), SyncStatus::Unreconciled);
    assert!(b.report.items.iter().all(|i| i.class == "Order" && i.kind == "class"), "{:?}", b.report.items.iter().map(|i| &i.id).collect::<Vec<_>>());
    assert!(matches!(&b.report.items[0].suggested_ops[..], [Op::MoveToPackage { .. }]));
}

#[test]
fn three_way_status_table() {
    let (d0, c0) = (design(), code());
    let base = sync::mark_synced(&sync::analyse(&d0, &c0, &manifest(), None, false), 1);
    // design ahead: only the design changed
    let d1 = apply(&d0, r#"[{"op":"add_method","class":"Order","method":{"name":"cancel"}}]"#);
    let a = sync::analyse(&d1, &c0, &manifest(), Some(&base), false);
    assert_eq!(status(&a, "method:Order.cancel()"), SyncStatus::DesignAhead);
    // code ahead: only the code changed
    let mut c1 = c0.clone();
    c1.class_mut("Line").unwrap().attributes.push(Attribute { name: "price".into(), ty: "Money".into(), ..Default::default() });
    let a = sync::analyse(&d0, &c1, &manifest(), Some(&base), false);
    assert_eq!(status(&a, "field:Line.price"), SyncStatus::CodeAhead);
    // conflict: both changed the same thing differently
    let d2 = apply(&d0, r#"[{"op":"update_method","class":"Order","name":"total","method":{"name":"total","returns":"BigDecimal"}}]"#);
    let mut c2 = c0.clone();
    c2.class_mut("Order").unwrap().methods.iter_mut().find(|m| m.name == "total").unwrap().returns = Some("long".into());
    let a = sync::analyse(&d2, &c2, &manifest(), Some(&base), false);
    assert_eq!(status(&a, "method:Order.total()"), SyncStatus::Conflict);
    // no baseline: unreconciled
    let a = sync::analyse(&d1, &c0, &manifest(), None, false);
    assert_eq!(status(&a, "method:Order.cancel()"), SyncStatus::Unreconciled);
    // mark in sync: the remaining difference is accepted
    let marked = sync::mark_synced(&sync::analyse(&d1, &c0, &manifest(), Some(&base), false), 2);
    let a = sync::analyse(&d1, &c0, &manifest(), Some(&marked), false);
    assert_eq!(status(&a, "method:Order.cancel()"), SyncStatus::Accepted);
    assert_eq!(a.report.class_status("Order"), Some(SyncStatus::Accepted));
}

#[test]
fn baseline_advances_only_towards_agreement() {
    let (d0, c0) = (design(), code());
    let base = sync::mark_synced(&sync::analyse(&d0, &c0, &manifest(), None, false), 1);
    // both sides gain the same method: in sync again, the baseline catches up
    let d1 = apply(&d0, r#"[{"op":"add_method","class":"Order","method":{"name":"cancel"}}]"#);
    let mut c1 = c0.clone();
    c1.class_mut("Order").unwrap().methods.push(Method { name: "cancel".into(), ..Default::default() });
    let a = sync::analyse(&d1, &c1, &manifest(), Some(&base), false);
    let next = sync::advance(Some(&base), &a, 5).expect("advanced");
    assert!(next.design.contains_key("method:Order.cancel()") && next.code.contains_key("method:Order.cancel()"));
    assert_eq!(next.updated_at, 5);
    assert!(sync::advance(Some(&next), &a, 6).is_none(), "nothing left to advance");
    // a design-only change doesn't move the baseline (it stays design ahead) …
    let d2 = apply(&d1, r#"[{"op":"add_method","class":"Line","method":{"name":"split"}}]"#);
    let a = sync::analyse(&d2, &c1, &manifest(), Some(&next), false);
    assert!(sync::advance(Some(&next), &a, 7).is_none());
    // … and a partial index never does
    let a = sync::analyse(&d1, &c1, &manifest(), Some(&base), true);
    assert!(sync::advance(Some(&base), &a, 8).is_none());
    // without a baseline, the first agreement creates one
    let a = sync::analyse(&d0, &c0, &manifest(), None, false);
    let created = sync::advance(None, &a, 9).unwrap();
    assert_eq!(created.design, a.design);
}

#[test]
fn suggested_ops_bring_the_code_into_the_design() {
    let mut c = code();
    c.class_mut("Line").unwrap().attributes.push(Attribute { name: "price".into(), ty: "Money".into(), ..Default::default() });
    c.class_mut("Order").unwrap().methods.retain(|m| m.name != "total");
    let mut extra = c.class("Line").unwrap().clone();
    extra.name = "Coupon".into();
    extra.source = sym("org.shop.domain.Coupon", "src/main/java/org/shop/domain/Coupon.java");
    c.classes.push(extra);
    let d = design();
    let a = sync::analyse(&d, &c, &manifest(), None, false);
    let ops: Vec<Op> = a.report.items.iter().flat_map(|i| i.suggested_ops.clone()).collect();
    assert!(!ops.is_empty());
    let after = ops::apply_all(&d, &ops, ops::Mode::Commit(Status::Accepted, None)).expect("ops apply");
    let b = sync::analyse(&after, &c, &manifest(), None, false);
    assert!(b.report.items.is_empty(), "{:#?}", b.report.items.iter().map(|i| (&i.id, &i.design, &i.code)).collect::<Vec<_>>());
    // design intent survives: the field update keeps the attribute, the class keeps its package
    assert_eq!(after.class("Line").unwrap().attributes.iter().find(|a| a.name == "price").unwrap().ty, "Money");
}

#[test]
fn implement_brief_names_files_changes_and_decisions() {
    let (d0, c0) = (design(), code());
    let base = sync::mark_synced(&sync::analyse(&d0, &c0, &manifest(), None, false), 1);
    let mut s = Store::new(d0);
    let id = s.propose_decision(
        "Can orders be cancelled?".into(),
        None,
        vec![serde_json::from_value(serde_json::json!({"label": "Order.cancel()", "ops": [
            {"op":"add_method","class":"Order","method":{"name":"cancel","params":[{"name":"reason","type":"String"}],"description":"stops fulfilment"}},
            {"op":"add_class","name":"Refund","package":"domain","responsibilities":["pay the money back"]}]}))
        .unwrap()],
    )
    .unwrap();
    s.approve_decision(&id, s.design.rev, "keep it simple".into()).unwrap();
    let a = sync::analyse(&s.design, &c0, &manifest(), Some(&base), false);
    assert_eq!(status(&a, "class:Refund"), SyncStatus::DesignAhead);
    let brief = sync::implement_brief(&a, &s.design, &c0, &manifest(), &[], false);
    assert!(brief.contains("## Order — src/main/java/org/shop/domain/Order.java"), "{brief}");
    assert!(brief.contains("add method `cancel(reason: String)` — stops fulfilment"), "{brief}");
    assert!(brief.contains("## Refund — src/main/java/org/shop/domain/Refund.java"), "{brief}");
    assert!(brief.contains("Responsibilities: pay the money back"), "{brief}");
    assert!(brief.contains("Can orders be cancelled? → Order.cancel(). Notes: keep it simple"), "{brief}");
    // scoped to one class
    let only = sync::implement_brief(&a, &s.design, &c0, &manifest(), &["Refund".into()], false);
    assert!(!only.contains("## Order"));
}

#[test]
fn renamed_classes_are_hinted_not_merged() {
    let d = apply(&design(), r#"[{"op":"rename_class","from":"Line","to":"OrderLine"}]"#);
    let a = sync::analyse(&d, &code(), &manifest(), None, false);
    let it = |id: &str| a.report.items.iter().find(|i| i.id == id).unwrap();
    assert_eq!(it("class:OrderLine").rename_of.as_deref(), Some("Line"));
    assert_eq!(it("class:Line").rename_of.as_deref(), Some("OrderLine"));
    assert_eq!(it("class:Line").status, SyncStatus::Unreconciled);
}

#[test]
fn baseline_files_round_trip_and_bad_ones_are_reported() {
    let a = sync::analyse(&design(), &code(), &manifest(), None, false);
    let b = sync::mark_synced(&a, 42);
    assert_eq!(Baseline::parse(&b.to_json()).unwrap(), b);
    assert!(Baseline::parse("<<<<<<< HEAD").unwrap_err().contains("no baseline"));
}

#[test]
fn code_facts_follow_the_links() {
    let a = sync::analyse(&design(), &code(), &manifest(), None, false);
    let d = sync::with_code_facts(&design(), &code(), &a);
    assert!(d.class("Order").unwrap().methods.iter().all(|m| m.code.is_some()));
    assert!(design().class("Order").unwrap().methods.iter().all(|m| m.code.is_none()));
}

#[test]
fn same_name_in_another_package_is_hinted_and_linked() {
    let mut c = code();
    let o = c.class_mut("Line").unwrap();
    o.package = None;
    o.source = sym("org.shop.Line", "src/main/java/org/shop/Line.java");
    let d = design();
    let a = sync::analyse(&d, &c, &manifest(), None, false);
    let code_only = a.report.items.iter().find(|i| i.id == "class:org.shop.Line").expect("code-only, named by its FQN");
    assert_eq!(code_only.rename_of.as_deref(), Some("Line"));
    assert!(matches!(&code_only.suggested_ops[..], [Op::LinkCode { class, symbol, .. }] if class == "Line" && symbol == "org.shop.Line"));
    // applying the link makes them one class again; only the package still differs
    let after = ops::apply_all(&d, &code_only.suggested_ops, ops::Mode::Commit(Status::Accepted, None)).unwrap();
    let b = sync::analyse(&after, &c, &manifest(), None, false);
    assert_eq!(b.report.items.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(), vec!["class:Line"]);
    assert!(matches!(&b.report.items[0].suggested_ops[..], [Op::MoveToPackage { package: None, .. }]));
}
