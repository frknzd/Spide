use spide_core::*;

fn rest_extraction() -> Design {
    let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/rest-extraction.json");
    let mut d = Design::from_json(&std::fs::read_to_string(p).unwrap()).unwrap();
    for p in d.packages.iter_mut().filter(|p| p.name == "zap" || p.name == "swing") {
        p.external = true;
    }
    d
}

#[test]
#[ignore = "debug output"]
fn dump() {
    let r = overlap::analyse(&rest_extraction(), 0.4);
    for p in &r.pairs {
        println!("{:.2} {} ~ {}  name {:.2} sig {:.2} touch {:.2} text {:.2} {:?}", p.score, p.a, p.b, p.name, p.signature, p.touches, p.text, p.shared);
    }
    for x in &r.delegations { println!("deleg {} → {}", x.from, x.to); }
    for x in &r.classes { println!("class {} / {} {:.2} {:.2} resp {:.2} n{}", x.a, x.b, x.share_a, x.share_b, x.responsibility, x.pairs); }
    for x in &r.claims { println!("claim {} → {} via {} ({}) {:?} noown={}", x.method, x.owner, x.via, x.evidence, x.shared, x.uses_no_own_data); }
    for x in &r.scattered { println!("scattered {} {:?}", x.concept, x.methods); }
    println!("middle men {:?}", r.middle_men);
}

fn apply(ops_json: &str) -> Design {
    let ops: Vec<Op> = serde_json::from_str(ops_json).unwrap();
    ops::apply_all(&Design::default(), &ops, ops::Mode::Commit(Status::Accepted, None)).unwrap()
}

#[test]
fn rest_extraction_overlap() {
    let r = overlap::analyse(&rest_extraction(), 0.5);
    // the controller's pass-throughs are delegation, never duplication
    assert!(r.delegations.iter().any(|x| x.from == "RestExtractor.addEndpoint" && x.to == "ApiNamespace.addEndpoint"));
    assert!(!r.pairs.iter().any(|p| p.score >= overlap::DUPLICATE && (p.a.starts_with("RestExtractor.") || p.b.starts_with("RestExtractor."))), "{:#?}", r.pairs);
    assert!(r.middle_men.is_empty(), "a controller is no middle man");
    // trailing-slash handling belongs to the class that claims all path rules
    assert!(r.claims.iter().any(|c| c.method == "ApiNamespace.withoutTrailingSlash" && c.owner == "UrlPath" && c.uses_no_own_data));
    // counting calls duplicates what EndpointReport keeps
    assert!(r.claims.iter().any(|c| c.method == "Endpoint.countCalls" && c.owner == "EndpointReport"));
    // polymorphic matches() in the PathSegment hierarchy is not a scattered concept
    assert!(!r.scattered.iter().any(|s| s.concept == "match"));
}

#[test]
fn detects_real_duplication() {
    let d = apply(
        r#"[{"op":"add_class","name":"OrderLine","methods":[{"name":"getPrice","returns":"Money"},{"name":"getQuantity","returns":"int"}]},
            {"op":"add_class","name":"Order","attributes":[{"name":"lines","type":"List<OrderLine>"}],
             "methods":[{"name":"computeTotal","returns":"Money","uses":["lines"],"calls":["OrderLine.getPrice","OrderLine.getQuantity"]}]},
            {"op":"add_class","name":"InvoicePrinter","attributes":[{"name":"lines","type":"List<OrderLine>"}],
             "methods":[{"name":"calculateTotal","returns":"Money","uses":["lines"],"calls":["OrderLine.getPrice","OrderLine.getQuantity"]},
                        {"name":"print","uses":["lines"]}]}]"#,
    );
    let r = overlap::analyse(&d, 0.5);
    let p = r.pairs.first().expect("a pair");
    assert!(p.score >= overlap::DUPLICATE, "{p:?}");
    assert_eq!((p.a.as_str(), p.b.as_str()), ("Order.computeTotal", "InvoicePrinter.calculateTotal"));
    assert!(r.classes.iter().any(|c| c.a == "Order" && c.b == "InvoicePrinter" && c.share_a > 0.9));
    let m = metrics::compute(&d, false);
    assert!(m.issues.iter().any(|i| i.kind == "duplication"));
}
