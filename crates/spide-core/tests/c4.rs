use spide_core::diff::diff;
use spide_core::store::Target;
use spide_core::*;

fn ops(json: &str) -> Vec<Op> {
    serde_json::from_str(json).expect("ops parse")
}

fn shop() -> Store {
    let mut s = Store::default();
    s.apply_user(ops(
        r#"[{"op":"add_class","name":"CartController","package":"web"},{"op":"add_class","name":"Order","package":"domain"},
            {"op":"add_relation","from":"CartController","to":"Order","kind":"dependency"},
            {"op":"add_container","name":"WebApp","technology":"React"},{"op":"add_container","name":"API","technology":"Spring","replicas":2},
            {"op":"add_container","name":"DB","technology":"Postgres","datastore":true},
            {"op":"add_system","name":"Stripe","description":"payments"},
            {"op":"assign_container","package":"web","container":"WebApp"},{"op":"assign_container","package":"domain","container":"API"},
            {"op":"add_link","from":"API","to":"Stripe","protocol":"HTTPS"},{"op":"add_link","from":"API","to":"DB","protocol":"JDBC"}]"#,
    ))
    .unwrap();
    s
}

#[test]
fn c4_ops_validate_and_apply() {
    let mut s = shop();
    let d = &s.design;
    assert_eq!(d.containers.len(), 3);
    assert_eq!(d.containers[1].replicas, 2);
    assert_eq!(d.packages.iter().find(|p| p.name == "domain").unwrap().container.as_deref(), Some("API"));
    assert_eq!(d.links.len(), 2);
    assert!(s.apply_user(ops(r#"[{"op":"add_link","from":"API","to":"Nowhere"}]"#)).is_err());
    assert!(s.apply_user(ops(r#"[{"op":"add_system","name":"API"}]"#)).is_err(), "names are shared");
    assert!(s.apply_user(ops(r#"[{"op":"assign_container","package":"web","container":"Nope"}]"#)).is_err());
    // removing a container unassigns its packages and drops its links
    s.apply_user(ops(r#"[{"op":"remove_container","name":"API"}]"#)).unwrap();
    assert!(s.design.packages.iter().all(|p| p.container.as_deref() != Some("API")));
    assert!(s.design.links.is_empty());
    // prune / restore
    s.prune(&[Target::System { name: "Stripe".into() }], None).unwrap();
    assert!(s.view().systems.is_empty());
    s.restore(&Target::System { name: "Stripe".into() }).unwrap();
    assert_eq!(s.view().systems.len(), 1);
}

#[test]
fn c4_ghosts_and_diff_round_trip() {
    let mut s = Store::default();
    s.apply_user(ops(r#"[{"op":"add_class","name":"A","package":"core"}]"#)).unwrap();
    let id = s.propose_ops(ops(r#"[{"op":"add_container","name":"Svc"},{"op":"assign_container","package":"core","container":"Svc"}]"#), None, "agent").unwrap();
    let v = s.view();
    assert_eq!(v.containers[0].status, Status::Proposed);
    assert!(s.design.containers.is_empty());
    s.accept_proposal(&id).unwrap();

    let a = shop().design;
    let mut b = a.clone();
    b.containers.retain(|k| k.name != "DB");
    b.links.retain(|l| l.to != "DB");
    b.containers[0].replicas = 3;
    b.systems.push(ExternalSystem { name: "Mailer".into(), ..Default::default() });
    b.links.push(Link { from: "WebApp".into(), to: "Mailer".into(), ..Default::default() });
    b.packages.iter_mut().find(|p| p.name == "web").unwrap().container = Some("API".into());
    let after = ops::apply_all(&a, &diff(&a, &b), ops::Mode::Commit(Status::Accepted, None)).unwrap();
    assert_eq!(after.containers, b.containers);
    assert_eq!(after.systems, b.systems);
    assert_eq!(after.links, b.links);
    assert_eq!(after.packages.iter().map(|p| &p.container).collect::<Vec<_>>(), b.packages.iter().map(|p| &p.container).collect::<Vec<_>>());
}

#[test]
fn system_view_and_consistency() {
    use spide_core::metrics::compute;
    let mut s = shop();
    s.design.use_cases = serde_json::from_str(r#"{"actors":[{"name":"Shopper"},{"name":"Ghost"}],"use_cases":[{"name":"Checkout","actors":["Shopper"]}]}"#).unwrap();
    s.apply_user(ops(r#"[{"op":"add_link","from":"Shopper","to":"WebApp","protocol":"HTTPS"},
                         {"op":"add_class","name":"Audit","package":"logging"},
                         {"op":"add_container","name":"Worker"},{"op":"add_system","name":"Mailer"}]"#))
        .unwrap();
    let m = compute(&s.design, false);
    let v = spide_core::derive::system_view(&s.design, &m);
    assert_eq!(v.container_deps, vec![spide_core::derive::ContainerDep { from: "WebApp".into(), to: "API".into(), count: 1, sync: true }]);
    let api = v.containers.iter().find(|k| k.name == "API").unwrap();
    assert_eq!((api.packages.clone(), api.classes), (vec!["domain".to_string()], 1));
    let ctx: Vec<(String, String)> = v.context_edges.iter().map(|e| (e.from.clone(), e.to.clone())).collect();
    let sys = s.design.name.clone();
    assert!(ctx.contains(&("Shopper".into(), sys.clone())));
    assert!(ctx.contains(&(sys.clone(), "Stripe".into())));
    assert!(!ctx.iter().any(|(_, t)| t == "DB"), "datastores are containers, not context");
    let msgs: Vec<&str> = m.issues.iter().filter(|i| i.kind == "consistency").map(|i| i.message.as_str()).collect();
    assert!(msgs.contains(&"package logging runs in no container"), "{msgs:?}");
    assert!(msgs.contains(&"container Worker has no packages"));
    assert!(msgs.contains(&"external system Mailer is not connected"));
    assert!(msgs.contains(&"actor Ghost takes part in no use case"));
    assert!(!msgs.iter().any(|m| m.contains("DB")), "datastores need no packages");
}
