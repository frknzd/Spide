mod common;
use common::golden;
use spide_core::rules::*;
use spide_core::*;

fn ops(json: &str) -> Vec<Op> {
    serde_json::from_str(json).expect("ops parse")
}

fn base() -> Design {
    let mut d = ops::apply_all(
        &Design::default(),
        &ops(r#"[{"op":"add_package","name":"web","layer":"ui"},{"op":"add_package","name":"app","layer":"application"},
                 {"op":"add_package","name":"domain","layer":"domain"},{"op":"add_package","name":"persistence","layer":"infrastructure"}]"#),
        ops::Mode::Commit(Status::Accepted, None),
    )
    .unwrap();
    d.name = "Library".into();
    d
}

#[test]
fn clean_with_custom_rules() {
    let mut d = base();
    d.settings.code_root = Some("org.acme.library".into());
    d.rules.forbidden.push(DepRule { from: "*Controller".into(), to: "*Repository".into(), reason: Some("go through a service".into()) });
    d.rules.forbidden.push(DepRule { from: "domain".into(), to: "persistence".into(), reason: None });
    d.rules.forbidden_types.push(TypeRule { layer: Layer::Domain, pattern: "Http*".into(), reason: None });
    let java = archunit::archunit_test(&d);
    assert!(java.contains(".whereLayer(\"Domain\").mayOnlyBeAccessedByLayers(\"Ui\", \"Application\", \"Infrastructure\")"));
    assert!(java.contains(".whereLayer(\"Application\").mayOnlyBeAccessedByLayers(\"Ui\", \"Infrastructure\")"));
    assert!(java.contains("haveNameMatching(\".*\\\\.[^.]*Controller\")"));
    assert!(java.contains("resideInAPackage(\"org.acme.library.domain..\")"));
    golden("ArchitectureTest-clean.java", &java);
}

#[test]
fn hexagonal_layered_modular() {
    let mut d = base();
    d.rules.preset = Preset::Hexagonal;
    let java = archunit::archunit_test(&d);
    assert!(java.contains(".whereLayer(\"Infrastructure\").mayNotBeAccessedByAnyLayer()"), "adapters are independent:\n{java}");
    assert!(java.contains("@AnalyzeClasses\n"), "no root → analyze default");
    d.rules.preset = Preset::Layered;
    let java = archunit::archunit_test(&d);
    assert!(java.contains(".whereLayer(\"Infrastructure\").mayOnlyBeAccessedByLayers(\"Ui\", \"Application\", \"Domain\")"));
    assert!(java.contains(".whereLayer(\"Ui\").mayNotBeAccessedByAnyLayer()"));

    let mut m = ops::apply_all(
        &Design::default(),
        &ops(r#"[{"op":"add_package","name":"orders.domain","layer":"domain"},{"op":"add_package","name":"orders.api","layer":"application"},
                 {"op":"add_package","name":"billing.domain","layer":"domain"}]"#),
        ops::Mode::Commit(Status::Accepted, None),
    )
    .unwrap();
    m.rules.preset = Preset::ModularMonolith;
    m.settings.code_root = Some("com.shop".into());
    let java = archunit::archunit_test(&m);
    assert!(java.contains("billing_uses_orders_only_via_api"));
    assert!(java.contains("not(resideInAPackage(\"com.shop.orders.api..\"))"));
    golden("ArchitectureTest-modular.java", &java);
}
