//! Concrete findings and dependency observations on DOSS course examples.
//! Each pair is a "worse" and a "better" version of the same design, taken from the course's
//! examples (Monopoly, POS/Sale, CigarOperator, the Brewery exam) and from the REST-extraction design.

use spide_core::metrics::compute;
use spide_core::*;

fn design(ops_json: &str) -> Design {
    let ops: Vec<Op> = serde_json::from_str(ops_json).unwrap();
    ops::apply_all(&Design::default(), &ops, ops::Mode::Commit(Status::Accepted, None)).unwrap()
}

fn has(d: &Design, kind: &str) -> bool { compute(d, false).issues.iter().any(|i| i.kind == kind) }

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
    assert!(compute(&worse, false).classes["MonopolyGame"].fan_out > compute(&better, false).classes["MonopolyGame"].fan_out);
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
    assert!(has(&worse, "layer"));
    assert!(!has(&better, "layer"));
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
    assert!(has(&worse, "type_switch"));
    assert!(!has(&better, "type_switch"));
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
fn god_class_finding_survives_unrelated_additions() {
    for n in [4, 49] {
        assert!(compute(&with_one_god(n), false).issues.iter().any(|i| i.kind == "god" && i.targets.iter().any(|t| t == "God")));
    }
}

#[test]
fn rest_extraction_improves_when_methods_move_home() {
    let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/rest-extraction.json");
    let mut d = Design::from_json(&std::fs::read_to_string(p).unwrap()).unwrap();
    for p in d.packages.iter_mut().filter(|p| p.name == "zap" || p.name == "swing") {
        p.external = true;
    }
    for (m, target) in [("RestExtractionPanel.callsOf", "ApiReport"), ("ApiNamespace.withoutTrailingSlash", "UrlPath"), ("Endpoint.countCalls", "EndpointReport")] {
        let s = home::suggest_home(&d, &[m.to_string()]).unwrap();
        let c = s.candidates.iter().find(|c| c.class == target && c.legal).unwrap();
        d = ops::apply_all(&d, &c.ops, ops::Mode::Commit(Status::Accepted, None)).unwrap();
        assert!(d.class(target).unwrap().methods.iter().any(|method| m.ends_with(&format!(".{}", method.name))));
    }
}

#[test]
fn findings_cite_principle_and_source() {
    let m = compute(&with_one_god(4), false);
    assert!(!m.issues.is_empty());
    for i in &m.issues {
        assert!(["coupling", "cohesion", "responsibility", "architecture", "uml"].contains(&i.principle), "{}", i.kind);
        assert!(!i.source.is_empty());
    }
}

#[test]
fn ui_calling_the_domain_directly_is_worse_than_a_controller() {
    // Ch. 3 Controller: the POS applet calling makeLineItem on Sale is "incorrect"
    let base = r#"{"op":"add_package","name":"ui","layer":"ui"},{"op":"add_package","name":"domain","layer":"domain"},
        {"op":"add_class","name":"Sale","stereotype":"entity","package":"domain","attributes":[{"name":"lines","type":"List<SalesLineItem>"}],
         "methods":[{"name":"makeLineItem","params":[{"name":"spec","type":"ProductSpecification"},{"name":"qty","type":"int"}],"uses":["lines"]},
                    {"name":"getTotal","returns":"Money","uses":["lines"],"calls":["SalesLineItem.getSubtotal"]}]},
        {"op":"add_class","name":"SalesLineItem","package":"domain","attributes":[{"name":"quantity","type":"int"}],"methods":[{"name":"getSubtotal","returns":"Money","uses":["quantity"]}]},
        {"op":"add_class","name":"ProductSpecification","stereotype":"value","package":"domain","attributes":[{"name":"price","type":"Money"}],"methods":[{"name":"getPrice","returns":"Money","uses":["price"]}]},
        {"op":"add_relation","from":"Sale","to":"SalesLineItem","kind":"composition"},{"op":"add_relation","from":"SalesLineItem","to":"ProductSpecification","kind":"association"}"#;
    let worse = design(&format!(
        r#"[{base},{{"op":"add_class","name":"SaleFrame","package":"ui","attributes":[{{"name":"sale","type":"Sale"}}],
             "methods":[{{"name":"onEnterItem","uses":["sale"],"calls":["Sale.makeLineItem","refresh"]}},{{"name":"refresh","uses":["sale"],"calls":["Sale.getTotal"]}}]}},
            {{"op":"add_relation","from":"SaleFrame","to":"Sale","kind":"association"}}]"#
    ));
    let better = design(&format!(
        r#"[{base},{{"op":"add_class","name":"SaleFrame","package":"ui","attributes":[{{"name":"register","type":"Register"}}],
             "methods":[{{"name":"onEnterItem","uses":["register"],"calls":["Register.enterItem","refresh"]}},{{"name":"refresh","uses":["register"],"calls":["Register.getTotal"]}}]}},
            {{"op":"add_class","name":"Register","stereotype":"controller","package":"domain","attributes":[{{"name":"sale","type":"Sale"}}],
             "methods":[{{"name":"enterItem","params":[{{"name":"id","type":"ItemID"}},{{"name":"qty","type":"int"}}],"uses":["sale"],"calls":["Sale.makeLineItem"]}},
                        {{"name":"getTotal","returns":"Money","uses":["sale"],"calls":["Sale.getTotal"]}}]}},
            {{"op":"add_relation","from":"SaleFrame","to":"Register","kind":"association"}},{{"op":"add_relation","from":"Register","to":"Sale","kind":"association"}}]"#
    ));
    assert!(compute(&worse, false).issues.iter().any(|i| i.kind == "ui_bypasses_controller"));
    assert!(has(&worse, "ui_bypasses_controller"));
    assert!(!has(&better, "ui_bypasses_controller"));
}

#[test]
fn brewery_god_class_without_behaviour_fails_against_the_passing_design() {
    // Ch. 6 exam feedback: "Fail — God class & no real behaviour" vs "Pass — Recipe builds list of steps"
    let fail = design(
        r#"[{"op":"add_class","name":"Brewery","attributes":[{"name":"recipes","type":"List<Recipe>"},{"name":"steps","type":"List<Step>"},{"name":"ingredients","type":"List<Ingredient>"}],
             "methods":[{"name":"brew","params":[{"name":"r","type":"Recipe"}],"uses":["steps"],"calls":["Step.getKind","Step.getMinutes","Ingredient.getName","Ingredient.getAmount"]},
                        {"name":"totalDuration","params":[{"name":"r","type":"Recipe"}],"returns":"int","uses":["steps"],"calls":["Step.getMinutes","Step.getKind"]},
                        {"name":"listIngredients","params":[{"name":"r","type":"Recipe"}],"returns":"List<String>","uses":["ingredients"],"calls":["Ingredient.getName","Ingredient.getAmount"]},
                        {"name":"validate","params":[{"name":"r","type":"Recipe"}],"returns":"boolean","uses":["steps"],"calls":["Step.getKind"]}]},
            {"op":"add_class","name":"Recipe","attributes":[{"name":"name","type":"String"},{"name":"stepIds","type":"List<int>"}]},
            {"op":"add_class","name":"Step","attributes":[{"name":"kind","type":"String"},{"name":"minutes","type":"int"}],
             "methods":[{"name":"getKind","returns":"String","uses":["kind"]},{"name":"getMinutes","returns":"int","uses":["minutes"]}]},
            {"op":"add_class","name":"Ingredient","attributes":[{"name":"name","type":"String"},{"name":"amount","type":"int"}],
             "methods":[{"name":"getName","returns":"String","uses":["name"]},{"name":"getAmount","returns":"int","uses":["amount"]}]},
            {"op":"add_relation","from":"Brewery","to":"Recipe","kind":"aggregation"},{"op":"add_relation","from":"Brewery","to":"Step","kind":"aggregation"},
            {"op":"add_relation","from":"Brewery","to":"Ingredient","kind":"aggregation"}]"#,
    );
    let pass = design(
        r#"[{"op":"add_class","name":"Recipe","attributes":[{"name":"name","type":"String"},{"name":"steps","type":"List<Step>"}],
             "methods":[{"name":"addStep","params":[{"name":"s","type":"Step"}],"uses":["steps"]},{"name":"duration","returns":"int","uses":["steps"],"calls":["Step.duration"]},
                        {"name":"accept","params":[{"name":"v","type":"RecipeVisitor"}],"uses":["steps"],"calls":["Step.accept"]}]},
            {"op":"add_class","name":"Step","stereotype":"abstract","attributes":[{"name":"ingredients","type":"List<Ingredient>"}],
             "methods":[{"name":"duration","returns":"int","is_abstract":true},{"name":"accept","params":[{"name":"v","type":"RecipeVisitor"}],"is_abstract":true}]},
            {"op":"add_class","name":"MaltingStep","attributes":[{"name":"minutes","type":"int"}],"methods":[{"name":"duration","returns":"int","uses":["minutes"]},{"name":"accept","params":[{"name":"v","type":"RecipeVisitor"}],"calls":["RecipeVisitor.visitMalting"]}]},
            {"op":"add_class","name":"BoilingStep","attributes":[{"name":"minutes","type":"int"}],"methods":[{"name":"duration","returns":"int","uses":["minutes"]},{"name":"accept","params":[{"name":"v","type":"RecipeVisitor"}],"calls":["RecipeVisitor.visitBoiling"]}]},
            {"op":"add_class","name":"Ingredient","attributes":[{"name":"name","type":"String"},{"name":"amount","type":"int"}],
             "methods":[{"name":"scale","params":[{"name":"factor","type":"double"}],"uses":["amount"]}]},
            {"op":"add_class","name":"RecipeVisitor","stereotype":"interface","methods":[{"name":"visitMalting","params":[{"name":"s","type":"MaltingStep"}],"is_abstract":true},{"name":"visitBoiling","params":[{"name":"s","type":"BoilingStep"}],"is_abstract":true}]},
            {"op":"add_relation","from":"Recipe","to":"Step","kind":"composition"},{"op":"add_relation","from":"MaltingStep","to":"Step","kind":"inheritance"},
            {"op":"add_relation","from":"BoilingStep","to":"Step","kind":"inheritance"},{"op":"add_relation","from":"Step","to":"Ingredient","kind":"aggregation"},
            {"op":"add_relation","from":"Recipe","to":"RecipeVisitor","kind":"dependency"}]"#,
    );
    let (_, resolved) = spide_core::impact::issue_diff(&compute(&fail, false), &compute(&pass, false));
    assert!(!resolved.is_empty(), "the alternative should resolve concrete findings");
}

#[test]
fn composite_and_visitor_sanity() {
    let d = design(
        r#"[{"op":"add_class","name":"Node","stereotype":"abstract","methods":[{"name":"size","returns":"int","is_abstract":true}]},
            {"op":"add_class","name":"Folder","methods":[{"name":"size","returns":"int"}]},
            {"op":"add_relation","from":"Folder","to":"Node","kind":"inheritance"},
            {"op":"add_relation","from":"Folder","to":"Node","kind":"aggregation","to_mult":"*"},
            {"op":"add_class","name":"SizeVisitor","stereotype":"interface","methods":[{"name":"visitFile","params":[{"name":"f","type":"Node"}],"is_abstract":true}]}]"#,
    );
    let m = compute(&d, false);
    assert!(m.issues.iter().any(|i| i.kind == "composite_incomplete" && i.message.contains("add/remove/getChildren")), "{:#?}", m.issues);
    assert!(m.issues.iter().any(|i| i.kind == "degenerate_visitor"));
}
