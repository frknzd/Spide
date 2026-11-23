use spide_core::{metrics::compute, scope::PackageScope, *};

#[test]
fn visibility_and_inheritance_do_not_expand_other_package_siblings() {
    let d = Design::from_json(&serde_json::json!({"packages":[{"name":"ui"},{"name":"library","external":true}],"classes":[
        {"name":"Panel","package":"ui"},{"name":"Base","package":"library","external":true},
        {"name":"Other","package":"library"},{"name":"Proposal","package":"ui","status":"proposed"},
        {"name":"ProposalNeighbor"}],"relations":[
        {"id":"inherit","from":"Panel","to":"Base","kind":"inheritance"},
        {"id":"proposal","from":"Proposal","to":"ProposalNeighbor","kind":"dependency","status":"proposed"}
    ]}).to_string()).unwrap();
    let m = compute(&d,true);
    let accepted = PackageScope::new(&d,&m,"ui",false).unwrap();
    assert_eq!(accepted.classes.len(),2);
    assert!(accepted.classes.contains("Base"));
    let ghosts = PackageScope::new(&d,&m,"ui",true).unwrap();
    assert_eq!(ghosts.classes.len(),4);
    assert!(!ghosts.classes.contains("Other"));
}

#[test]
fn package_scope_is_bidirectional_one_hop_and_preserves_health() {
    let d = Design::from_json(&serde_json::json!({"packages":[{"name":"ui"},{"name":"model"},{"name":"empty"},{"name":"ui.sub"}],"classes":[
        {"name":"Panel","package":"ui","methods":[{"name":"render","calls":["Implicit.read"]}]},
        {"name":"Sibling","package":"ui"},{"name":"Caller","package":"model"},{"name":"Model","package":"model"},
        {"name":"Implicit","package":"model","methods":[{"name":"read","returns":"String"}]},
        {"name":"Distant","package":"model"},{"name":"Child","package":"ui.sub"},{"name":"Hidden","status":"pruned"}],
        "relations":[{"id":"in","from":"Caller","to":"Panel","kind":"dependency"},{"id":"out","from":"Panel","to":"Model","kind":"dependency"},
        {"id":"context","from":"Caller","to":"Model","kind":"dependency"},{"id":"hop2","from":"Model","to":"Distant","kind":"dependency"},
        {"id":"hidden","from":"Panel","to":"Hidden","kind":"dependency"}]
    }).to_string()).unwrap();
    let m = compute(&d,false);
    let scope = PackageScope::new(&d,&m,"ui",false).unwrap();
    assert_eq!(scope.classes.iter().map(String::as_str).collect::<Vec<_>>(), vec!["Caller","Implicit","Model","Panel","Sibling"]);
    assert!(!scope.includes_edge("Caller","Model"));
    let projected = scope.design(&d);
    assert_eq!(projected.relations.len(),2);
    assert_eq!(scope.metrics(&m,&d).health_report,m.health_report);
    assert_eq!(d.classes.len(),8);
    assert!(PackageScope::new(&d,&m,"empty",false).unwrap().classes.is_empty());
    assert!(PackageScope::new(&d,&m,"missing",false).is_err());
    assert_ne!(scope::package_key("a.b"), scope::package_key("a-b"));
    assert_ne!(scope::package_key("a"), scope::package_key("ä"));
}

#[test]
fn package_scope_includes_code_creates_and_flags_implicit_layer_violations() {
    let d = Design::from_json(&serde_json::json!({
        "packages": [
            {"name": "ui", "layer": "ui"},
            {"name": "domain", "layer": "domain"},
            {"name": "other", "layer": "application"}
        ],
        "classes": [
            {"name": "OrderView", "package": "ui", "methods": [
                {"name": "submit", "calls": ["Order.checkout"], "code": {"file": "OrderView.java", "start_line": 1, "end_line": 10, "creates": ["Invoice"]}}
            ]},
            {"name": "Order", "package": "domain", "methods": [{"name": "checkout"}]},
            {"name": "Invoice", "package": "domain"},
            {"name": "Unrelated", "package": "other"}
        ]
    }).to_string()).unwrap();

    let m = compute(&d, false);
    // Both call-based and code.creates-based implicit edges are captured with rule_violation
    let ui_to_order = m.implicit_edges.iter().find(|e| e.from == "OrderView" && e.to == "Order").expect("UI->Order implicit edge");
    assert_eq!(ui_to_order.rule_violation.as_deref(), Some("layer ↑"));
    let ui_to_invoice = m.implicit_edges.iter().find(|e| e.from == "OrderView" && e.to == "Invoice").expect("UI->Invoice creates edge");
    assert_eq!(ui_to_invoice.rule_violation.as_deref(), Some("layer ↑"));

    // PackageScope for `ui` includes exact minimal boundary: OrderView + Order + Invoice (never Unrelated)
    let scope = PackageScope::new(&d, &m, "ui", false).unwrap();
    assert_eq!(scope.classes.iter().map(String::as_str).collect::<Vec<_>>(), vec!["Invoice", "Order", "OrderView"]);
    assert!(!scope.classes.contains("Unrelated"));
    // Unrelated is truly disconnected -> root_cause orphan
    let orphan_sig = m.health_report.unique_signals().into_iter().find(|s| s.rule == "orphan" && s.entity == "Unrelated").expect("Unrelated should be flagged as orphan");
    assert_eq!(orphan_sig.tier, "root_cause");
    assert!(orphan_sig.severity >= 0.6);
}
