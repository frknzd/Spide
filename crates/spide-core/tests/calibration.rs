//! Calibration: health must rank designs the way the DOSS course does. Orderings, not numbers.
//! Each pair is a "worse" and a "better" version of the same design, taken from the course's
//! examples (Monopoly, POS/Sale, CigarOperator, the Brewery exam) and from the REST-extraction design.

use spide_core::metrics::compute;
use spide_core::*;

fn design(ops_json: &str) -> Design {
    let ops: Vec<Op> = serde_json::from_str(ops_json).unwrap();
    ops::apply_all(&Design::default(), &ops, ops::Mode::Commit(Status::Accepted, None)).unwrap()
}

fn report(label: &str, d: &Design) -> u8 {
    let m = compute(d, false);
    let principles: Vec<String> = m.principles.iter().map(|(k, v)| format!("{k} {}", v.score)).collect();
    println!("{label}: health {} [{}]", m.health, principles.join(", "));
    for i in m.issues.iter().take(8) {
        println!("    {} {:.2}/{:.2} {}", i.kind, i.severity, i.confidence, i.message);
    }
    m.health
}

fn worse_than(worse: &Design, better: &Design, what: &str) {
    let (w, b) = (report(&format!("{what} (worse)"), worse), report(&format!("{what} (better)"), better));
    assert!(w < b, "{what}: expected {w} < {b}");
}

#[test]
fn monopoly_game_doing_everything_is_worse_than_delegating() {
    // Ch. 4: "MonopolyGame does everything (low cohesion)" vs "MonopolyGame delegates (high cohesion)"
    let worse = design(
        r#"[{"op":"add_package","name":"domain","layer":"domain"},
            {"op":"add_class","name":"MonopolyGame","package":"domain","attributes":[{"name":"players","type":"List<Player>"},{"name":"board","type":"Board"},{"name":"dice","type":"List<Die>"},{"name":"round","type":"int"}],
             "methods":[{"name":"playGame","uses":["round"],"calls":["playRound"]},
                        {"name":"playRound","uses":["players"],"calls":["takeTurn"]},
                        {"name":"takeTurn","params":[{"name":"p","type":"Player"}],"uses":["dice","board"],
                         "calls":["rollDice","Player.getLocation","Board.getSquares","Square.getIndex","Player.setLocation","Player.getName","Square.getName"]},
                        {"name":"rollDice","returns":"int","uses":["dice"],"calls":["Die.roll","Die.getFaceValue"]},
                        {"name":"computeNewLocation","params":[{"name":"old","type":"Square"},{"name":"roll","type":"int"}],"returns":"Square","uses":["board"],"calls":["Board.getSquares","Square.getIndex"]}]},
            {"op":"add_class","name":"Player","package":"domain","attributes":[{"name":"name","type":"String"},{"name":"location","type":"Square"}],
             "methods":[{"name":"getName","returns":"String","uses":["name"]},{"name":"getLocation","returns":"Square","uses":["location"]},{"name":"setLocation","params":[{"name":"s","type":"Square"}],"uses":["location"]}]},
            {"op":"add_class","name":"Board","package":"domain","attributes":[{"name":"squares","type":"List<Square>"}],"methods":[{"name":"getSquares","returns":"List<Square>","uses":["squares"]}]},
            {"op":"add_class","name":"Square","package":"domain","attributes":[{"name":"name","type":"String"},{"name":"index","type":"int"}],
             "methods":[{"name":"getName","returns":"String","uses":["name"]},{"name":"getIndex","returns":"int","uses":["index"]}]},
            {"op":"add_class","name":"Die","package":"domain","attributes":[{"name":"faceValue","type":"int"}],
             "methods":[{"name":"roll","uses":["faceValue"]},{"name":"getFaceValue","returns":"int","uses":["faceValue"]}]},
            {"op":"add_relation","from":"MonopolyGame","to":"Player","kind":"aggregation"},{"op":"add_relation","from":"MonopolyGame","to":"Board","kind":"composition"},
            {"op":"add_relation","from":"MonopolyGame","to":"Die","kind":"composition"},{"op":"add_relation","from":"Board","to":"Square","kind":"composition"},
            {"op":"add_relation","from":"Player","to":"Square","kind":"association"}]"#,
    );
    let better = design(
        r#"[{"op":"add_package","name":"domain","layer":"domain"},
            {"op":"add_class","name":"MonopolyGame","package":"domain","attributes":[{"name":"players","type":"List<Player>"},{"name":"round","type":"int"}],
             "methods":[{"name":"playGame","uses":["round"],"calls":["playRound"]},{"name":"playRound","uses":["players"],"calls":["Player.takeTurn"]}]},
            {"op":"add_class","name":"Player","package":"domain","attributes":[{"name":"name","type":"String"},{"name":"piece","type":"Piece"},{"name":"board","type":"Board"},{"name":"cup","type":"Cup"}],
             "methods":[{"name":"takeTurn","uses":["piece","board","cup"],"calls":["Cup.roll","Cup.getTotal","Piece.getLocation","Board.getSquare","Piece.setLocation"]}]},
            {"op":"add_class","name":"Piece","package":"domain","attributes":[{"name":"location","type":"Square"}],
             "methods":[{"name":"getLocation","returns":"Square","uses":["location"]},{"name":"setLocation","params":[{"name":"s","type":"Square"}],"uses":["location"]}]},
            {"op":"add_class","name":"Board","package":"domain","attributes":[{"name":"squares","type":"List<Square>"}],
             "methods":[{"name":"getSquare","params":[{"name":"old","type":"Square"},{"name":"distance","type":"int"}],"returns":"Square","uses":["squares"],"calls":["Square.getIndex"]}]},
            {"op":"add_class","name":"Square","package":"domain","attributes":[{"name":"name","type":"String"},{"name":"index","type":"int"}],
             "methods":[{"name":"getIndex","returns":"int","uses":["index"]}]},
            {"op":"add_class","name":"Cup","package":"domain","attributes":[{"name":"dice","type":"List<Die>"},{"name":"total","type":"int"}],
             "methods":[{"name":"roll","uses":["dice","total"],"calls":["Die.roll","Die.getFaceValue"]},{"name":"getTotal","returns":"int","uses":["total"]}]},
            {"op":"add_class","name":"Die","package":"domain","attributes":[{"name":"faceValue","type":"int"}],
             "methods":[{"name":"roll","uses":["faceValue"]},{"name":"getFaceValue","returns":"int","uses":["faceValue"]}]},
            {"op":"add_relation","from":"MonopolyGame","to":"Player","kind":"aggregation"},{"op":"add_relation","from":"Player","to":"Piece","kind":"composition"},
            {"op":"add_relation","from":"Player","to":"Board","kind":"association"},{"op":"add_relation","from":"Player","to":"Cup","kind":"association"},
            {"op":"add_relation","from":"Board","to":"Square","kind":"composition"},{"op":"add_relation","from":"Piece","to":"Square","kind":"association"},
            {"op":"add_relation","from":"Cup","to":"Die","kind":"composition"}]"#,
    );
    worse_than(&worse, &better, "Monopoly");
}

#[test]
fn sale_saving_itself_is_worse_than_a_pure_fabrication() {
    // Ch. 3 Pure Fabrication: Sale.save() (Expert) → high coupling, low cohesion; PersistentStorage → low/high
    let base = r#"{"op":"add_package","name":"domain","layer":"domain"},{"op":"add_package","name":"persistence","layer":"infrastructure"},
        {"op":"add_class","name":"SalesLineItem","package":"domain","attributes":[{"name":"quantity","type":"int"}],"methods":[{"name":"getSubtotal","returns":"Money","uses":["quantity"]}]},
        {"op":"add_class","name":"Sale","package":"domain"},{"op":"add_relation","from":"Sale","to":"SalesLineItem","kind":"composition","to_mult":"1..*"},{"op":"remove_class","name":"Sale"}"#;
    let worse = design(&format!(
        r#"[{base},{{"op":"add_class","name":"Sale","stereotype":"entity","package":"domain","attributes":[{{"name":"lines","type":"List<SalesLineItem>"}},{{"name":"connection","type":"DbConnection"}}],
             "methods":[{{"name":"getTotal","returns":"Money","uses":["lines"],"calls":["SalesLineItem.getSubtotal"]}},
                        {{"name":"save","uses":["lines","connection"],"calls":["DbConnection.execute"]}},{{"name":"load","params":[{{"name":"id","type":"long"}}],"uses":["connection"],"calls":["DbConnection.query"]}}]}},
            {{"op":"add_class","name":"DbConnection","package":"persistence","methods":[{{"name":"execute","params":[{{"name":"sql","type":"String"}}]}},{{"name":"query","params":[{{"name":"sql","type":"String"}}],"returns":"ResultSet"}}]}},
            {{"op":"add_relation","from":"Sale","to":"DbConnection","kind":"association"}}]"#
    ));
    let better = design(&format!(
        r#"[{base},{{"op":"add_class","name":"Sale","stereotype":"entity","package":"domain","attributes":[{{"name":"lines","type":"List<SalesLineItem>"}}],
             "methods":[{{"name":"getTotal","returns":"Money","uses":["lines"],"calls":["SalesLineItem.getSubtotal"]}}]}},
            {{"op":"add_class","name":"PersistentStorage","stereotype":"repository","package":"persistence","attributes":[{{"name":"connection","type":"DbConnection"}}],
             "methods":[{{"name":"save","params":[{{"name":"s","type":"Sale"}}],"uses":["connection"],"calls":["DbConnection.execute"]}},
                        {{"name":"load","params":[{{"name":"id","type":"long"}}],"returns":"Sale","uses":["connection"],"calls":["DbConnection.query"]}}]}},
            {{"op":"add_class","name":"DbConnection","package":"persistence","methods":[{{"name":"execute","params":[{{"name":"sql","type":"String"}}]}},{{"name":"query","params":[{{"name":"sql","type":"String"}}],"returns":"ResultSet"}}]}},
            {{"op":"add_relation","from":"PersistentStorage","to":"DbConnection","kind":"association"}},{{"op":"add_relation","from":"PersistentStorage","to":"Sale","kind":"dependency"}}]"#
    ));
    worse_than(&worse, &better, "Sale.save vs PersistentStorage");
}

#[test]
fn switching_on_a_type_code_everywhere_is_worse_than_polymorphism() {
    // Ch. 2 coupling & cohesion in practice: the CigarOperator switch in 11 GATK classes
    let mut worse = design(
        r#"[{"op":"add_class","name":"CigarOperator","stereotype":"enum","attributes":[{"name":"M"},{"name":"I"},{"name":"D"},{"name":"S"}]},
            {"op":"add_class","name":"AlignmentStateMachine","methods":[{"name":"stepForwardOnGenome","params":[{"name":"op","type":"CigarOperator"}],"returns":"int"}]},
            {"op":"add_class","name":"SWPairwiseAlignment","methods":[{"name":"printAlignment","params":[{"name":"op","type":"CigarOperator"}]}]},
            {"op":"add_class","name":"ReadUtils","methods":[{"name":"coverage","params":[{"name":"op","type":"CigarOperator"}],"returns":"int"}]},
            {"op":"add_relation","from":"AlignmentStateMachine","to":"CigarOperator","kind":"dependency"},
            {"op":"add_relation","from":"SWPairwiseAlignment","to":"CigarOperator","kind":"dependency"},
            {"op":"add_relation","from":"ReadUtils","to":"CigarOperator","kind":"dependency"}]"#,
    );
    // the implementation (as imported): the same switch in every class
    for c in worse.classes.iter_mut().filter(|c| c.name != "CigarOperator") {
        for m in &mut c.methods {
            m.code = Some(CodeFacts { switches: 1, switch_on: vec!["M".into(), "I".into(), "D".into(), "S".into()], loc: 14, ..Default::default() });
        }
    }
    let better = design(
        r#"[{"op":"add_class","name":"CigarOperator","stereotype":"abstract","methods":[{"name":"stepForward","returns":"int","is_abstract":true},{"name":"print","is_abstract":true},{"name":"coverage","returns":"int","is_abstract":true}]},
            {"op":"add_class","name":"MatchOp","methods":[{"name":"stepForward","returns":"int"},{"name":"print"},{"name":"coverage","returns":"int"}]},
            {"op":"add_class","name":"InsertionOp","methods":[{"name":"stepForward","returns":"int"},{"name":"print"},{"name":"coverage","returns":"int"}]},
            {"op":"add_class","name":"DeletionOp","methods":[{"name":"stepForward","returns":"int"},{"name":"print"},{"name":"coverage","returns":"int"}]},
            {"op":"add_class","name":"AlignmentStateMachine","methods":[{"name":"stepForwardOnGenome","params":[{"name":"op","type":"CigarOperator"}],"returns":"int","calls":["CigarOperator.stepForward"]}]},
            {"op":"add_class","name":"SWPairwiseAlignment","methods":[{"name":"printAlignment","params":[{"name":"op","type":"CigarOperator"}],"calls":["CigarOperator.print"]}]},
            {"op":"add_class","name":"ReadUtils","methods":[{"name":"coverage","params":[{"name":"op","type":"CigarOperator"}],"returns":"int","calls":["CigarOperator.coverage"]}]},
            {"op":"add_relation","from":"MatchOp","to":"CigarOperator","kind":"inheritance"},{"op":"add_relation","from":"InsertionOp","to":"CigarOperator","kind":"inheritance"},
            {"op":"add_relation","from":"DeletionOp","to":"CigarOperator","kind":"inheritance"},
            {"op":"add_relation","from":"AlignmentStateMachine","to":"CigarOperator","kind":"dependency"},
            {"op":"add_relation","from":"SWPairwiseAlignment","to":"CigarOperator","kind":"dependency"},
            {"op":"add_relation","from":"ReadUtils","to":"CigarOperator","kind":"dependency"}]"#,
    );
    worse_than(&worse, &better, "CigarOperator switch vs polymorphism");
}

/// `n` small, clean, connected classes plus one god class.
fn with_one_god(n: usize) -> Design {
    let mut ops = vec![r#"{"op":"add_class","name":"God","attributes":[{"name":"a","type":"int"},{"name":"b","type":"int"},{"name":"c","type":"int"},{"name":"d","type":"int"},{"name":"e","type":"int"},{"name":"f","type":"int"},{"name":"g","type":"int"},{"name":"h","type":"int"},{"name":"i","type":"int"}],
        "methods":[{"name":"m1","uses":["a"]},{"name":"m2","uses":["b"]},{"name":"m3","uses":["c"]},{"name":"m4","uses":["d"]},{"name":"m5","uses":["e"]},{"name":"m6","uses":["f"]},{"name":"m7","uses":["g"]},{"name":"m8","uses":["h"]},{"name":"m9","uses":["i"]}],
        "responsibilities":["billing","shipping","reporting","login","printing"]}"#
        .to_string()];
    for i in 0..n {
        ops.push(format!(r#"{{"op":"add_class","name":"C{i}","attributes":[{{"name":"x","type":"int"}}],"methods":[{{"name":"work{i}","uses":["x"]}}]}}"#));
        ops.push(format!(r#"{{"op":"add_relation","from":"C{i}","to":"{}","kind":"association"}}"#, if i == 0 { "God".to_string() } else { format!("C{}", i - 1) }));
    }
    design(&format!("[{}]", ops.join(",")))
}

#[test]
fn one_bad_class_hurts_a_small_design_more() {
    worse_than(&with_one_god(4), &with_one_god(49), "one god class in 5 vs in 50");
    let m = compute(&with_one_god(4), false);
    assert!(m.health < 85, "a god class in a 5-class design is serious: {}", m.health);
}

#[test]
fn rest_extraction_improves_when_methods_move_home() {
    let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/rest-extraction.json");
    let mut d = Design::from_json(&std::fs::read_to_string(p).unwrap()).unwrap();
    for p in d.packages.iter_mut().filter(|p| p.name == "zap" || p.name == "swing") {
        p.external = true;
    }
    let before = d.clone();
    for m in ["RestExtractionPanel.callsOf", "ApiNamespace.withoutTrailingSlash", "Endpoint.countCalls"] {
        let s = home::suggest_home(&d, &[m.to_string()]).unwrap();
        let c = s.candidates.iter().find(|c| c.class == s.recommended).unwrap();
        d = ops::apply_all(&d, &c.ops, ops::Mode::Commit(Status::Accepted, None)).unwrap();
    }
    worse_than(&before, &d, "REST extraction before/after suggested homes");
}

#[test]
fn principle_scores_explain_the_number() {
    let m = compute(&with_one_god(4), false);
    assert_eq!(m.principles.len(), 5);
    assert!(m.principles["cohesion"].score < m.principles["architecture"].score);
    for i in &m.issues {
        assert!(["coupling", "cohesion", "responsibility", "architecture", "uml"].contains(&i.principle), "{}", i.kind);
        assert!(!i.source.is_empty());
    }
}
