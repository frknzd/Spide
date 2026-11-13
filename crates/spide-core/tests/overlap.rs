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

fn show(s: &home::HomeSuggestion) {
    println!("== {:?} → {}", s.methods, s.recommended);
    for c in &s.candidates {
        println!("  {:5} {:24} pkg {:?} expert {:.2} legal {} health {}→{} fo {:+} lcom {:?} fit {} score {:.2} | {} | {:?}", c.kind, c.class, c.package, c.expert, c.legal, c.health_before, c.health_after, c.source_fan_out_delta, c.lcom_after, c.role_fit, c.score, c.why, c.illegal_because);
    }
}

#[test]
#[ignore = "debug output"]
fn dump_homes() {
    let d = rest_extraction();
    for m in ["RestExtractionPanel.callsOf", "ApiNamespace.withoutTrailingSlash", "Endpoint.countCalls", "CallTableModel.matches"] {
        show(&home::suggest_home(&d, &[m.to_string()]).unwrap());
    }
}

#[test]
#[ignore = "debug output"]
fn dump_move_impact() {
    let d = rest_extraction();
    let s = home::suggest_home(&d, &["RestExtractionPanel.callsOf".to_string()]).unwrap();
    let c = s.candidates.iter().find(|c| c.class == "ApiReport").unwrap();
    println!("{}", serde_json::to_string_pretty(&c.ops).unwrap());
    let im = impact::impact_of_ops(&d, &c.ops).unwrap();
    println!("new: {:#?}\nresolved: {:#?}", im.new_issues, im.resolved_issues);
}

#[test]
fn suggested_homes() {
    let d = rest_extraction();
    let best = |m: &str| home::suggest_home(&d, &[m.to_string()]).unwrap();
    // Demeter chain + counting in the UI → the report that holds the endpoints
    let s = best("RestExtractionPanel.callsOf");
    assert_eq!(s.recommended, "ApiReport", "{:#?}", s.candidates.iter().map(|c| (&c.class, c.score)).collect::<Vec<_>>());
    let top = &s.candidates[0];
    assert!(top.health_after > top.health_before && top.legal);
    // the moved method no longer needs the extractor: the panel obtains the report itself
    let added = top.ops.iter().find_map(|o| match o { Op::AddMethod { class, method } if class == "ApiReport" => Some(method), _ => None }).unwrap();
    assert!(added.params.iter().all(|p| p.ty != "RestExtractor" && p.ty != "ApiNamespace"), "{added:?}");
    // trailing-slash rule → the class that claims all path rules
    assert_eq!(best("ApiNamespace.withoutTrailingSlash").recommended, "UrlPath");
    // counting calls per method → the report that keeps callCounts
    assert_eq!(best("Endpoint.countCalls").recommended, "EndpointReport");
    // every candidate is applicable as is
    for c in &best("Endpoint.countCalls").candidates {
        ops::apply_all(&d, &c.ops, ops::Mode::Commit(Status::Accepted, None)).unwrap();
    }
}

#[test]
fn duplicates_get_one_home_and_pure_fabrication_when_no_expert_fits() {
    let d = apply(
        r#"[{"op":"add_class","name":"OrderLine","methods":[{"name":"getPrice","returns":"Money"},{"name":"getQuantity","returns":"int"}]},
            {"op":"add_class","name":"Order","attributes":[{"name":"lines","type":"List<OrderLine>"}],
             "methods":[{"name":"computeTotal","returns":"Money","uses":["lines"],"calls":["OrderLine.getPrice","OrderLine.getQuantity"]},
                        {"name":"checkout","calls":["computeTotal"]}]},
            {"op":"add_class","name":"InvoicePrinter","attributes":[{"name":"lines","type":"List<OrderLine>"}],
             "methods":[{"name":"calculateTotal","returns":"Money","uses":["lines"],"calls":["OrderLine.getPrice","OrderLine.getQuantity"]},
                        {"name":"print","uses":["lines"],"calls":["calculateTotal"]}]},
            {"op":"add_relation","from":"Order","to":"OrderLine","kind":"composition"},
            {"op":"add_relation","from":"InvoicePrinter","to":"OrderLine","kind":"association"}]"#,
    );
    let s = home::suggest_home(&d, &["Order.computeTotal".into(), "InvoicePrinter.calculateTotal".into()]).unwrap();
    assert_eq!(s.recommended, "Order", "{:#?}", s.candidates.iter().map(|c| (&c.class, c.score, &c.why)).collect::<Vec<_>>());
    let c = &s.candidates[0];
    let after = ops::apply_all(&d, &c.ops, ops::Mode::Commit(Status::Accepted, None)).unwrap();
    assert!(after.class("InvoicePrinter").unwrap().methods.iter().all(|m| m.name != "calculateTotal"));
    assert!(after.class("InvoicePrinter").unwrap().methods.iter().find(|m| m.name == "print").unwrap().calls.contains(&"Order.computeTotal".to_string()));
    assert!(s.candidates.iter().any(|c| c.kind == "new"), "a Pure Fabrication option is always offered");

    // a helper used by two unrelated classes, owning no data anywhere → a new shared class
    let d = apply(
        r#"[{"op":"add_class","name":"Invoice","methods":[{"name":"formatAmount","params":[{"name":"cents","type":"long"}],"returns":"String"}]},
            {"op":"add_class","name":"Receipt","methods":[{"name":"formatAmount","params":[{"name":"cents","type":"long"}],"returns":"String"}]}]"#,
    );
    let s = home::suggest_home(&d, &["Invoice.formatAmount".into(), "Receipt.formatAmount".into()]).unwrap();
    assert_eq!(s.candidates[0].kind, "new", "{:#?}", s.candidates);
    assert!(s.recommended.ends_with("Formatter"), "{}", s.recommended);
}
