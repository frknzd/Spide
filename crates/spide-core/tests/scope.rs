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
    assert_eq!(scope.metrics(&m,&d).health_report.score,m.health_report.score);
    assert_eq!(d.classes.len(),8);
    assert!(PackageScope::new(&d,&m,"empty",false).unwrap().classes.is_empty());
    assert!(PackageScope::new(&d,&m,"missing",false).is_err());
    assert_ne!(scope::package_key("a.b"), scope::package_key("a-b"));
    assert_ne!(scope::package_key("a"), scope::package_key("ä"));
}
