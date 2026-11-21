use spide_core::*;

fn apply(ops_json: &str) -> Result<Design, OpError> {
    let ops: Vec<Op> = serde_json::from_str(ops_json).unwrap();
    ops::apply_all(&Design::default(), &ops, ops::Mode::Commit(Status::Accepted, None))
}

#[test]
fn role_and_concern_are_validated_and_stored() {
    let d = apply(
        r#"[{"op":"add_class","name":"Panel","role":"presentation","methods":[{"name":"refresh","concern":"presentation"}]},
            {"op":"update_class","name":"Panel","role":"controller"},
            {"op":"update_method","class":"Panel","name":"refresh","method":{"name":"refresh","concern":"coordination"}}]"#,
    )
    .unwrap();
    let c = d.class("Panel").unwrap();
    assert_eq!(c.role.as_deref(), Some("controller"));
    assert_eq!(c.methods[0].concern.as_deref(), Some("coordination"));
    assert!(apply(r#"[{"op":"add_class","name":"X","role":"wizard"}]"#).is_err());
    assert!(apply(r#"[{"op":"add_class","name":"X","methods":[{"name":"f","concern":"magic"}]}]"#).is_err());
    let cleared = apply(r#"[{"op":"add_class","name":"X","role":"domain"},{"op":"update_class","name":"X","role":""}]"#).unwrap();
    assert_eq!(cleared.class("X").unwrap().role, None);
}

pub fn rest_extraction() -> Design {
    let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/rest-extraction.json");
    let mut d = Design::from_json(&std::fs::read_to_string(p).unwrap()).unwrap();
    for p in d.packages.iter_mut().filter(|p| p.name == "zap" || p.name == "swing") {
        p.external = true;
    }
    d
}

#[test]
fn rest_extraction_findings() {
    let d = rest_extraction();
    let r = responsibility::analyse(&d);
    let has = |kind: &str, class: &str, method: Option<&str>| r.findings.iter().any(|f| f.kind == kind && f.class == class && f.method.as_deref() == method);
    // a UI class counting calls through a chain of getters does domain work
    assert!(has("misplaced_responsibility", "RestExtractionPanel", Some("callsOf")), "{:#?}", r.findings);
    // the controller only delegates: no fat controller, no misplaced work
    assert!(!r.findings.iter().any(|f| f.class == "RestExtractor"), "{:#?}", r.findings);
    assert_eq!(r.method("RestExtractor", "startExtraction").unwrap().concern, "coordination");
    // name (Writer → output) and stereotype («service») disagree
    assert!(has("role_inconsistency", "ApiDocumentationWriter", None));
    // Swing callbacks are presentation, not misplaced
    assert_eq!(r.method("CallTableModel", "getValueAt").unwrap().concern, "presentation");
    // externals aren't judged
    assert!(!r.classes.contains_key("ExtensionAdaptor"));
    assert_eq!(r.classes["RestExtractionPanel"].role.as_ref().unwrap().role, "presentation");
}

#[test]
fn course_examples() {
    // Pure Fabrication slide: Sale saving itself couples the domain to the database
    let d = apply(
        r#"[{"op":"add_package","name":"domain","layer":"domain"},
            {"op":"add_class","name":"Sale","stereotype":"entity","package":"domain","attributes":[{"name":"lines","type":"List<SalesLineItem>"}],
             "methods":[{"name":"getTotal","returns":"Money","uses":["lines"],"calls":["SalesLineItem.getSubtotal"]},
                        {"name":"save","uses":["lines"]},{"name":"toHtml","returns":"String","uses":["lines"]}]},
            {"op":"add_class","name":"SalesLineItem","package":"domain","methods":[{"name":"getSubtotal","returns":"Money"}]}]"#,
    )
    .unwrap();
    let r = responsibility::analyse(&d);
    let f = r.findings.iter().find(|f| f.method.as_deref() == Some("save")).expect("Sale.save flagged");
    assert!(f.explain.contains("Pure Fabrication"), "{}", f.explain);
    assert!(r.findings.iter().any(|f| f.method.as_deref() == Some("toHtml")));
    assert!(!r.findings.iter().any(|f| f.method.as_deref() == Some("getTotal")), "Information Expert: Sale knows its total");
    assert_eq!(r.method("Sale", "getTotal").unwrap().rdd, "knowing");

    // a controller that does the work instead of delegating (Monopoly: MonopolyGame does everything)
    let d = apply(
        r#"[{"op":"add_class","name":"GameController","stereotype":"controller","methods":[
              {"name":"calculateRent","params":[{"name":"s","type":"Square"}],"returns":"int","calls":["Square.getPrice"]},
              {"name":"computeScore","params":[{"name":"p","type":"Player"}],"returns":"int","calls":["Player.getCash"]},
              {"name":"validateMove","params":[{"name":"p","type":"Player"}],"returns":"boolean","calls":["Player.getLocation"]}]},
            {"op":"add_class","name":"Square","methods":[{"name":"getPrice","returns":"int"}]},
            {"op":"add_class","name":"Player","methods":[{"name":"getCash","returns":"int"},{"name":"getLocation","returns":"Square"}]}]"#,
    )
    .unwrap();
    let r = responsibility::analyse(&d);
    assert!(r.findings.iter().any(|f| f.kind == "fat_controller" && f.class == "GameController"), "{:#?}", r.findings);

    // vague names
    let d = apply(r#"[{"op":"add_class","name":"OrderManager"}]"#).unwrap();
    assert!(responsibility::analyse(&d).findings.iter().any(|f| f.kind == "vague_role"));
}

#[test]
#[ignore = "debug output: cargo test -p spide-core --test responsibility dump -- --ignored --nocapture"]
fn dump_rest_extraction() {
    let d = rest_extraction();
    let r = responsibility::analyse(&d);
    for (c, cr) in &r.classes {
        let ri = cr.role.as_ref().unwrap();
        println!("{c}: {} ({}: {}) purity {:.2} {:?}", ri.role, ri.source, ri.evidence, cr.purity, cr.concerns);
    }
    for m in &r.methods {
        println!("  {}.{} → {} / {} [{}: {}]", m.class, m.method, m.concern, m.rdd, m.source, m.reason);
    }
    for f in &r.findings {
        println!("! {} {:.2}/{:.2} {} → {:?}", f.kind, f.severity, f.confidence, f.message, f.home);
    }
}

#[test]
fn agent_assessments_merge_with_signals_and_go_stale() {
    let mut d = rest_extraction();
    let before = metrics::compute(&d, false);
    let flagged = |m: &metrics::Metrics, method: &str| m.issues.iter().find(|i| i.kind == "misplaced_responsibility" && i.targets.contains(&method.to_string())).cloned();
    let base = flagged(&before, "RestExtractionPanel.callsOf").expect("heuristic finding");

    // the agent agrees on callsOf and finds something the heuristics missed
    let a: Vec<assess::Assessment> = serde_json::from_value(serde_json::json!([
        {"class": "RestExtractionPanel", "purpose": "Shows the extraction status and passes user actions to the controller", "role": "presentation",
         "methods": [{"name": "callsOf", "verdict": "misplaced", "home": "ApiReport", "confidence": 0.8, "reason": "counts calls of a report"}]},
        {"class": "Endpoint", "purpose": "An endpoint with its recorded calls",
         "methods": [{"name": "clear", "verdict": "misplaced", "home": "ApiNamespace", "confidence": 0.6, "reason": "for the test"}]}
    ])).unwrap();
    assess::record(&mut d, a, 1).unwrap();
    let agreed = metrics::compute(&d, false);
    let f = flagged(&agreed, "RestExtractionPanel.callsOf").unwrap();
    assert!(f.confidence > base.confidence, "agreement raises confidence");
    assert_eq!(f.basis, "heuristic + agent");
    assert!(agreed.issues.iter().any(|i| i.basis == "agent" && i.targets.contains(&"Endpoint.clear".to_string())));
    // LLM-first: the agent's role beats the name/stereotype heuristics (an explicit `role` beats both)
    assert_eq!(agreed.responsibility.classes["RestExtractionPanel"].role.as_ref().unwrap().source, "agent");

    // the agent disputes it → lower confidence and a visible disagreement
    let a: Vec<assess::Assessment> = serde_json::from_value(serde_json::json!([
        {"class": "RestExtractionPanel", "purpose": "…", "methods": [{"name": "callsOf", "verdict": "fits", "confidence": 0.9, "reason": "only formats a count for a confirm dialog"}]}
    ])).unwrap();
    assess::record(&mut d, a, 2).unwrap();
    let disputed = metrics::compute(&d, false);
    assert!(flagged(&disputed, "RestExtractionPanel.callsOf").unwrap().confidence < base.confidence);
    assert!(disputed.issues.iter().any(|i| i.kind == "disagreement"));

    // the class changes → the assessment is stale and no longer counts
    d.class_mut("RestExtractionPanel").unwrap().methods.retain(|m| m.name != "confirm");
    let (stale, missing) = assess::stale_and_missing(&d);
    assert!(stale.contains(&"RestExtractionPanel".to_string()));
    assert!(missing.contains(&"ApiNamespace".to_string()));
    let after = metrics::compute(&d, false);
    assert!(!after.issues.iter().any(|i| i.kind == "disagreement"));
    // unknown roles/verdicts are rejected
    let bad: Vec<assess::Assessment> = serde_json::from_value(serde_json::json!([{"class": "Endpoint", "methods": [{"name": "clear", "verdict": "meh", "reason": ""}]}])).unwrap();
    assert!(assess::record(&mut d, bad, 3).is_err());
}
