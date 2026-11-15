//! Language adapters against the `poly` fixtures: the same small shop in six languages.

use spide_core::symbols::{EdgeKind, EntityKind, SemanticGraph};
use spide_import::{import_project, languages};
use std::path::PathBuf;

fn fixture(lang_dir: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/poly").join(lang_dir)
}

fn graph(lang_dir: &str, lang: &str) -> SemanticGraph {
    let root = fixture(lang_dir);
    let files = languages::files_of(&root, lang, false);
    assert!(!files.is_empty(), "{lang}: no files");
    let (g, skipped) = languages::index_files(&root, lang, &files);
    assert!(skipped.is_empty(), "{lang}: {skipped:?}");
    g
}

fn find<'a>(g: &'a SemanticGraph, q: &str) -> &'a spide_core::symbols::Entity {
    g.entities.iter().find(|e| e.qualified == q).unwrap_or_else(|| panic!("no {q} in {:#?}", g.entities.iter().map(|e| (&e.qualified, e.kind)).collect::<Vec<_>>()))
}

fn has_edge(g: &SemanticGraph, from: &str, to: &str, k: EdgeKind) -> bool {
    let (a, b) = (find(g, from).id.clone(), find(g, to).id.clone());
    g.edges.iter().any(|e| e.from == a && e.to == b && e.kind == k)
}

/// Expectations shared by every language: Order implements Priced; price() calls subtotal()
/// and tax(); subtotal has a loop and branches; add() uses the lines field.
fn common(g: &SemanticGraph, order: &str, priced: &str, tax: &str, subtotal: &str, price: &str) {
    let o = find(g, order);
    assert!(o.kind.is_some_and(EntityKind::is_type), "{order}: {:?}", o.kind);
    assert!(find(g, priced).kind.is_some_and(EntityKind::is_contract), "{priced}: {:?}", find(g, priced).kind);
    assert!(has_edge(g, order, priced, EdgeKind::Implements), "{order} implements {priced}: {:#?}", g.edges);
    assert!(has_edge(g, price, subtotal, EdgeKind::Calls), "{price} → {subtotal}");
    assert!(has_edge(g, price, tax, EdgeKind::Calls), "{price} → {tax}");
    let f = find(g, subtotal).facts.clone().unwrap();
    assert!(f.loops >= 1 && f.branches >= 1 && f.loc >= 3, "{subtotal}: {f:?}");
    assert!(!f.hash.is_empty() && !f.fingerprint.is_empty());
    assert!(find(g, subtotal).start_line > 1);
}

#[test]
fn rust() {
    let g = graph("rust", "rust");
    common(&g, "shop.orders.Order", "shop.pricing.Priced", "shop.pricing.tax", "shop.orders.Order.subtotal", "shop.orders.Order.price");
    assert_eq!(find(&g, "shop.orders.Order.add").uses, vec!["lines".to_string()]);
    assert!(find(&g, "shop.orders.Order.new").is_static);
    assert_eq!(find(&g, "shop.orders.Status").kind, Some(EntityKind::Enum));
    assert_eq!(find(&g, "shop.pricing.parse_cents").facts.as_ref().unwrap().idioms.get("unwrap"), Some(&1));
    assert!(has_edge(&g, "shop.orders.Order", "shop.orders.Line", EdgeKind::References));
}

#[test]
fn typescript() {
    let g = graph("ts", "typescript");
    common(&g, "src.orders.Order", "src.pricing.Priced", "src.pricing.tax", "src.orders.Order.subtotal", "src.orders.Order.price");
    assert!(find(&g, "src.pricing.parseCents").facts.as_ref().unwrap().idioms.contains_key("any"));
    assert_eq!(find(&g, "src.orders.Line").kind, Some(EntityKind::Interface));
    assert!(g.entities.iter().any(|e| e.qualified == "src.orders.Order.id"), "parameter property is a field");
}

#[test]
fn python() {
    let g = graph("py", "python");
    common(&g, "shop.orders.Order", "shop.pricing.Priced", "shop.pricing.tax", "shop.orders.Order.subtotal", "shop.orders.Order.price");
    assert_eq!(find(&g, "shop.orders.Line").kind, Some(EntityKind::Record));
    assert_eq!(find(&g, "shop.orders.Order.add").uses, vec!["lines".to_string()]);
}

#[test]
fn c() {
    let g = graph("c", "c");
    let o = find(&g, "src.orders.order");
    assert_eq!(o.kind, Some(EntityKind::Struct));
    let price = find(&g, "src.orders.order_price");
    assert!(price.file.ends_with(".c"), "definition wins over the prototype: {}", price.file);
    assert!(has_edge(&g, "src.orders.order_price", "src.pricing.tax", EdgeKind::Calls));
    assert!(has_edge(&g, "src.orders.order_price", "src.orders.order_subtotal", EdgeKind::Calls));
    assert!(find(&g, "src.orders.order_add").facts.as_ref().unwrap().idioms.contains_key("manual_memory"));
    assert_eq!(g.entities.iter().filter(|e| e.name == "order_price").count(), 1);
}

#[test]
fn cpp() {
    let g = graph("cpp", "cpp");
    common(&g, "shop.Order", "shop.Priced", "shop.tax", "shop.Order.subtotal", "shop.Order.price");
    assert_eq!(find(&g, "shop.Order.lines_").visibility, spide_core::model::Visibility::Private);
    assert!(find(&g, "shop.Order.subtotal").file.ends_with(".cpp"));
}

#[test]
fn clojure() {
    let g = graph("clj", "clojure");
    common(&g, "shop.orders.Order", "shop.pricing.Priced", "shop.pricing.tax", "shop.orders.subtotal", "shop.orders.Order.price");
    assert_eq!(find(&g, "shop.orders").kind, Some(EntityKind::Namespace));
    assert!(find(&g, "shop.pricing.parse-cents").facts.as_ref().unwrap().idioms.contains_key("mutable_state"));
    assert!(find(&g, "shop.pricing.tax").facts.as_ref().unwrap().branches >= 2);
}

#[test]
fn projection_never_invents_classes_for_functions() {
    let r = import_project(&fixture("clj"), &["clojure".into()], &["src".into()], &[], None).unwrap();
    let pricing = r.design.class("pricing").expect("namespace node");
    assert!(pricing.is_module());
    assert!(pricing.methods.iter().any(|m| m.name == "tax" && m.is_static));
    let order = r.design.class("Order").unwrap();
    assert!(r.design.relations.iter().any(|x| x.from == "Order" && x.to == "Priced" && x.kind == spide_core::model::RelationKind::Realization));
    assert_eq!(order.source.as_ref().unwrap().lang(), "clojure");
}

#[test]
fn mixed_language_workspace_merges_and_keeps_names_apart() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/poly");
    let r = import_project(&root, &["python".into(), "typescript".into()], &["py".into(), "ts".into()], &[], None).unwrap();
    assert_eq!(r.per_language["python"], 3);
    assert_eq!(r.per_language["typescript"], 2);
    let orders: Vec<_> = r.design.classes.iter().filter(|c| c.name.ends_with("Order")).collect();
    assert_eq!(orders.len(), 2, "{:?}", r.design.classes.iter().map(|c| &c.name).collect::<Vec<_>>());
    assert!(r.graph.languages().len() == 2);
}

#[test]
fn every_language_reports_capabilities_and_limits() {
    for l in spide_core::symbols::LANGUAGES {
        let c = languages::CAPABILITIES.iter().find(|(x, _, _)| *x == l).unwrap_or_else(|| panic!("{l}"));
        assert!(!c.1.is_empty() && !c.2.is_empty());
    }
}

#[test]
fn cpp_out_of_class_definitions_keep_declared_visibility() {
    let r = import_project(&fixture("cpp"), &["cpp".into()], &["src".into()], &[], None).unwrap();
    let order = r.design.class("Order").unwrap();
    for m in &order.methods {
        assert_eq!(m.visibility, spide_core::model::Visibility::Public, "{}", m.name);
    }
}
