//! Language-neutral semantic graph of observed code, and its projection onto the class-diagram
//! model.
//!
//! Language adapters (tree-sitter parsers, LSP document symbols) produce a `SemanticGraph` of
//! modules/namespaces, types (class, struct, enum, trait, interface, protocol, record, union),
//! functions, methods, fields and macros, with containment, call, implementation, extension and
//! type-reference edges. The graph never invents classes for procedural or functional code: a
//! module's free functions are projected onto one diagram node stereotyped «module» (or
//! «namespace» for Clojure), which the health profiles judge as a module, not as an object.
//!
//! Identity is `stable_id(language, file, qualified, signature)`; positions are only locations.

use crate::model::*;
use crate::ops::{apply_all, Mode, Op};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const GRAPH_VERSION: u32 = 1;

pub const LANGUAGES: [&str; 7] = ["rust", "typescript", "python", "java", "c", "cpp", "clojure"];

pub fn is_supported_language(l: &str) -> bool {
    LANGUAGES.contains(&l)
}

/// The language of a file, by extension.
pub fn language_of(path: &str) -> Option<&'static str> {
    let ext = path.rsplit_once('.')?.1.to_ascii_lowercase();
    Some(match ext.as_str() {
        "rs" => "rust",
        "ts" | "tsx" | "mts" | "cts" => "typescript",
        "py" | "pyi" => "python",
        "java" => "java",
        "c" | "h" => "c",
        "cc" | "cpp" | "cxx" | "hpp" | "hh" | "hxx" | "ipp" => "cpp",
        "clj" | "cljs" | "cljc" | "edn" | "bb" => "clojure",
        _ => return None,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum EntityKind {
    Module,
    Namespace,
    Class,
    Struct,
    Enum,
    Trait,
    Interface,
    Protocol,
    Record,
    Union,
    /// A type alias (`type X = …`, `typedef`).
    Alias,
    Function,
    Method,
    Field,
    Macro,
}

impl EntityKind {
    pub fn as_str(self) -> &'static str {
        match self {
            EntityKind::Module => "module",
            EntityKind::Namespace => "namespace",
            EntityKind::Class => "class",
            EntityKind::Struct => "struct",
            EntityKind::Enum => "enum",
            EntityKind::Trait => "trait",
            EntityKind::Interface => "interface",
            EntityKind::Protocol => "protocol",
            EntityKind::Record => "record",
            EntityKind::Union => "union",
            EntityKind::Alias => "alias",
            EntityKind::Function => "function",
            EntityKind::Method => "method",
            EntityKind::Field => "field",
            EntityKind::Macro => "macro",
        }
    }
    pub fn is_type(self) -> bool {
        matches!(self, EntityKind::Class | EntityKind::Struct | EntityKind::Enum | EntityKind::Trait | EntityKind::Interface | EntityKind::Protocol | EntityKind::Record | EntityKind::Union)
    }
    pub fn is_container(self) -> bool {
        self.is_type() || matches!(self, EntityKind::Module | EntityKind::Namespace)
    }
    pub fn is_callable(self) -> bool {
        matches!(self, EntityKind::Function | EntityKind::Method | EntityKind::Macro)
    }
    /// Abstraction-only types (contracts).
    pub fn is_contract(self) -> bool {
        matches!(self, EntityKind::Trait | EntityKind::Interface | EntityKind::Protocol)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    /// Function/method → function/method.
    Calls,
    /// Type → contract it implements (`impl Trait for T`, `implements`, protocol extension).
    Implements,
    /// Type → supertype.
    Extends,
    /// Field/parameter/return type reference: container → type.
    References,
    /// Module → module (import/require/include/use).
    Imports,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Entity {
    pub id: String,
    pub kind: Option<EntityKind>,
    pub name: String,
    /// Dotted path of containers + name.
    pub qualified: String,
    /// Containing entity id (module for top-level items, type for members).
    pub parent: Option<String>,
    pub language: String,
    pub file: String,
    pub start_line: u32,
    pub end_line: u32,
    pub start_col: u32,
    /// Callables: parameter types in order (for overload identity).
    pub params: Vec<Param>,
    pub returns: Option<String>,
    /// Fields: declared type.
    pub ty: Option<String>,
    pub visibility: Visibility,
    pub is_static: bool,
    pub is_abstract: bool,
    /// Callables: names of fields of the owning type the body touches (cohesion).
    pub uses: Vec<String>,
    pub facts: Option<CodeFacts>,
    /// Generated/macro-expanded/preprocessor-dependent: identity or edges may be incomplete.
    pub generated: bool,
}

impl Entity {
    pub fn signature(&self) -> String {
        format!("{}({})", self.name, self.params.iter().map(|p| p.ty.as_str()).collect::<Vec<_>>().join(","))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub kind: EdgeKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Document {
    pub language: String,
    /// Content hash (FNV) of the text the entities came from.
    pub hash: String,
    /// Editor/LSP document version when known (live buffers).
    pub version: Option<i64>,
    /// Parsed without syntax errors.
    pub clean: bool,
    /// Where the facts came from: parser | lsp | editor-buffer.
    pub provider: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct SemanticGraph {
    pub version: u32,
    pub entities: Vec<Entity>,
    pub edges: Vec<Edge>,
    /// Workspace-relative file → document facts.
    pub documents: BTreeMap<String, Document>,
    /// References the adapter saw but could not resolve (`Foo.bar` in another library, macros…).
    pub unresolved: Vec<String>,
    /// Honest limits of the analysis (e.g. "C preprocessor conditionals not evaluated").
    pub limits: Vec<String>,
}

/// Stable id: FNV of language | file | qualified | signature.
pub fn stable_id(language: &str, file: &str, qualified: &str, signature: &str) -> String {
    format!("{}:{}", language, crate::assess::fnv(&format!("{language}|{file}|{qualified}|{signature}")))
}

impl SemanticGraph {
    pub fn new() -> Self {
        SemanticGraph { version: GRAPH_VERSION, ..Default::default() }
    }

    pub fn entity(&self, id: &str) -> Option<&Entity> {
        self.entities.iter().find(|e| e.id == id)
    }

    /// Add an entity, assigning its id. Returns the id.
    pub fn add(&mut self, mut e: Entity) -> String {
        let sig = if e.kind.is_some_and(EntityKind::is_callable) { e.signature() } else { String::new() };
        e.id = stable_id(&e.language, &e.file, &e.qualified, &sig);
        // overloads/duplicate declarations (C headers + sources) keep distinct ids
        let mut id = e.id.clone();
        let mut n = 1;
        while self.entities.iter().any(|x| x.id == id) {
            n += 1;
            id = format!("{}~{n}", e.id);
        }
        e.id = id.clone();
        self.entities.push(e);
        id
    }

    pub fn edge(&mut self, from: &str, to: &str, kind: EdgeKind) {
        if from != to {
            self.edges.push(Edge { from: from.into(), to: to.into(), kind });
        }
    }

    /// Merge another graph (mixed-language workspaces). Ids are language-prefixed.
    pub fn merge(&mut self, other: SemanticGraph) {
        let have: BTreeSet<String> = self.entities.iter().map(|e| e.id.clone()).collect();
        self.entities.extend(other.entities.into_iter().filter(|e| !have.contains(&e.id)));
        self.edges.extend(other.edges);
        self.documents.extend(other.documents);
        self.unresolved.extend(other.unresolved);
        for l in other.limits {
            if !self.limits.contains(&l) {
                self.limits.push(l);
            }
        }
    }

    pub fn finish(&mut self) {
        self.edges.sort();
        self.edges.dedup();
        self.unresolved.sort();
        self.unresolved.dedup();
    }

    pub fn children<'a>(&'a self, id: &'a str) -> impl Iterator<Item = &'a Entity> + 'a {
        self.entities.iter().filter(move |e| e.parent.as_deref() == Some(id))
    }

    pub fn languages(&self) -> BTreeSet<String> {
        self.entities.iter().map(|e| e.language.clone()).collect()
    }

    /// Entity whose source range contains `file:line` (innermost).
    pub fn at(&self, file: &str, line: u32) -> Option<&Entity> {
        self.entities.iter().filter(|e| e.file == file && e.start_line <= line && line <= e.end_line).min_by_key(|e| e.end_line - e.start_line)
    }
}

/// Last dotted segment.
fn last(q: &str) -> &str {
    q.rsplit('.').next().unwrap_or(q)
}

fn source_of(e: &Entity) -> SourceSymbol {
    SourceSymbol {
        symbol: e.qualified.clone(),
        file: e.file.clone(),
        start_line: e.start_line,
        end_line: e.end_line,
        language: Some(e.language.clone()),
        kind: e.kind.map(|k| k.as_str().to_string()),
        id: Some(e.id.clone()),
        start_col: e.start_col,
    }
}

fn stereotype_for(e: &Entity) -> Option<String> {
    match e.kind? {
        EntityKind::Module => Some("module".into()),
        EntityKind::Namespace => Some("namespace".into()),
        EntityKind::Interface => Some("interface".into()),
        // contracts project to UML interfaces (realization edges), named by their idiom
        EntityKind::Trait => Some("trait".into()),
        EntityKind::Protocol => Some("protocol".into()),
        EntityKind::Enum => Some("enum".into()),
        EntityKind::Record => Some("value".into()),
        EntityKind::Struct => Some("struct".into()),
        EntityKind::Union => Some("union".into()),
        EntityKind::Class if e.is_abstract => Some("abstract".into()),
        _ => None,
    }
}

/// Is a diagram node standing for a module/namespace (functions, not an object)?
pub fn is_module_node(c: &Class) -> bool {
    matches!(c.stereotype.as_deref(), Some("module" | "namespace"))
}

/// Is a diagram node a contract (interface/trait/protocol)?
pub fn is_contract_node(c: &Class) -> bool {
    matches!(c.stereotype.as_deref(), Some("interface" | "trait" | "protocol"))
}

/// The design package of a container: its parent module's dotted path relative to `root`.
fn package_of(g: &SemanticGraph, e: &Entity, root: &str) -> Option<String> {
    let mut p = e.parent.as_deref().and_then(|id| g.entity(id));
    // nested types: climb to the enclosing module
    while let Some(x) = p {
        if matches!(x.kind, Some(EntityKind::Module | EntityKind::Namespace)) {
            break;
        }
        p = x.parent.as_deref().and_then(|id| g.entity(id));
    }
    let q = match (e.kind, p) {
        (Some(EntityKind::Module | EntityKind::Namespace), _) => e.qualified.rsplit_once('.').map(|(a, _)| a.to_string()).unwrap_or_default(),
        (_, Some(m)) => m.qualified.clone(),
        _ => String::new(),
    };
    let rel = if root.is_empty() { q.as_str() } else if q == root { "" } else { q.strip_prefix(root).and_then(|r| r.strip_prefix('.')).unwrap_or(&q) };
    (!rel.is_empty()).then(|| rel.to_string())
}

/// Project the graph onto the class-diagram model (compatibility projection for metrics, sync,
/// views and exports). Types become classes; a module/namespace with free functions (or
/// constants) becomes a «module»/«namespace» node; members become attributes/methods.
pub fn project(g: &SemanticGraph, root: &str) -> Result<Design, String> {
    // diagram nodes: types and modules with direct callables/fields
    let nodes: Vec<&Entity> = g
        .entities
        .iter()
        .filter(|e| match e.kind {
            Some(k) if k.is_type() => true,
            Some(EntityKind::Module | EntityKind::Namespace) => g.children(&e.id).any(|c| matches!(c.kind, Some(EntityKind::Function | EntityKind::Field | EntityKind::Macro))),
            _ => false,
        })
        .collect();
    let mut count: BTreeMap<&str, usize> = BTreeMap::new();
    for n in &nodes {
        *count.entry(last(&n.qualified)).or_default() += 1;
    }
    let node_name: BTreeMap<&str, String> = nodes.iter().map(|n| (n.id.as_str(), if count[last(&n.qualified)] > 1 { n.qualified.clone() } else { last(&n.qualified).to_string() })).collect();
    // callable/field → owning node
    let owner: BTreeMap<&str, &str> = g.entities.iter().filter_map(|e| {
        let p = e.parent.as_deref()?;
        node_name.contains_key(p).then_some((e.id.as_str(), p))
    }).collect();
    let mut calls_of: BTreeMap<&str, BTreeSet<String>> = BTreeMap::new();
    let mut node_deps: BTreeSet<(String, String, RelationKind)> = BTreeSet::new();
    for ed in &g.edges {
        match ed.kind {
            EdgeKind::Calls => {
                let (Some(from_owner), Some(to)) = (owner.get(ed.from.as_str()), g.entity(&ed.to)) else { continue };
                let target = match owner.get(ed.to.as_str()) {
                    Some(o) if o == from_owner => to.name.clone(),
                    Some(o) => format!("{}.{}", node_name[o], to.name),
                    None => continue,
                };
                calls_of.entry(ed.from.as_str()).or_default().insert(target);
            }
            EdgeKind::Implements | EdgeKind::Extends | EdgeKind::References => {
                let from = node_name.get(ed.from.as_str()).or_else(|| owner.get(ed.from.as_str()).map(|o| &node_name[o]));
                let to = node_name.get(ed.to.as_str());
                if let (Some(f), Some(t)) = (from, to) {
                    if f != t {
                        let kind = match ed.kind {
                            EdgeKind::Implements => RelationKind::Realization,
                            EdgeKind::Extends => RelationKind::Inheritance,
                            _ => RelationKind::Association,
                        };
                        node_deps.insert((f.clone(), t.clone(), kind));
                    }
                }
            }
            EdgeKind::Imports => {}
        }
    }
    let mut ops = vec![];
    for n in &nodes {
        let me = &node_name[n.id.as_str()];
        let mut attributes = vec![];
        let mut methods = vec![];
        let mut seen = BTreeSet::new();
        let members: Vec<&Entity> = g.children(&n.id).collect();
        let overloaded = |name: &str| members.iter().filter(|m| m.kind.is_some_and(EntityKind::is_callable) && m.name == name).count() > 1;
        for m in &members {
            match m.kind {
                Some(EntityKind::Field) => {
                    if seen.insert(format!("f:{}", m.name)) {
                        attributes.push(Attribute { name: m.name.clone(), ty: m.ty.clone().unwrap_or_default(), visibility: m.visibility, is_static: m.is_static, ..Default::default() });
                    }
                }
                Some(k) if k.is_callable() => {
                    let name = if overloaded(&m.name) { m.signature() } else { m.name.clone() };
                    if !seen.insert(format!("m:{name}")) {
                        continue;
                    }
                    let mut src = source_of(m);
                    src.symbol = format!("{}#{}", n.qualified, m.signature());
                    methods.push(Method {
                        name,
                        source: Some(src),
                        params: m.params.clone(),
                        returns: m.returns.clone(),
                        visibility: m.visibility,
                        is_static: m.is_static || matches!(n.kind, Some(EntityKind::Module | EntityKind::Namespace)),
                        is_abstract: m.is_abstract,
                        uses: m.uses.clone(),
                        calls: calls_of.get(m.id.as_str()).map(|c| c.iter().cloned().collect()).unwrap_or_default(),
                        code: m.facts.clone(),
                        ..Default::default()
                    });
                }
                _ => {}
            }
        }
        ops.push(Op::AddClass {
            name: me.clone(),
            stereotype: stereotype_for(n),
            package: package_of(g, n, root),
            is_abstract: n.is_abstract || n.kind.is_some_and(EntityKind::is_contract),
            attributes,
            methods,
            responsibilities: vec![],
            rationale: None,
            aggregate_root: false,
            external: false,
            role: None,
        });
    }
    // call dependencies between nodes without a structural relation
    let structural: BTreeSet<(String, String)> = node_deps.iter().map(|(a, b, _)| (a.clone(), b.clone())).collect();
    for n in &nodes {
        let me = &node_name[n.id.as_str()];
        let targets: BTreeSet<String> = g.children(&n.id).flat_map(|m| calls_of.get(m.id.as_str()).into_iter().flatten()).filter_map(|c| c.rsplit_once('.').map(|(t, _)| t.to_string())).collect();
        for t in targets {
            if !structural.contains(&(me.clone(), t.clone())) && &t != me {
                node_deps.insert((me.clone(), t, RelationKind::Dependency));
            }
        }
    }
    for (from, to, kind) in node_deps {
        ops.push(Op::AddRelation { from, to, kind, label: None, from_mult: None, to_mult: None, directed: (kind == RelationKind::Association).then_some(true), rationale: None, interaction: None });
    }
    let mut d = apply_all(&Design::default(), &ops, Mode::Commit(Status::Accepted, Some("import"))).map_err(|e| e.0)?;
    for n in &nodes {
        if let Some(c) = d.class_mut(&node_name[n.id.as_str()]) {
            c.source = Some(source_of(n));
        }
    }
    d.settings.code_root = (!root.is_empty()).then(|| root.to_string());
    Ok(d)
}

/// Common dotted prefix of all module paths (the "code root" of a non-Java project).
pub fn common_root(g: &SemanticGraph) -> String {
    let mods: Vec<&str> = g.entities.iter().filter(|e| matches!(e.kind, Some(EntityKind::Module | EntityKind::Namespace))).map(|e| e.qualified.as_str()).collect();
    let Some(first) = mods.first() else { return String::new() };
    let mut parts: Vec<&str> = first.split('.').collect();
    for m in &mods[1..] {
        let other: Vec<&str> = m.split('.').collect();
        let n = parts.iter().zip(&other).take_while(|(a, b)| a == b).count();
        parts.truncate(n);
    }
    // keep at least one level of packages when everything shares one module path
    if mods.iter().all(|m| *m == parts.join(".")) {
        parts.pop();
    }
    parts.join(".")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ent(kind: EntityKind, q: &str, parent: Option<&str>) -> Entity {
        Entity { kind: Some(kind), name: last(q).into(), qualified: q.into(), parent: parent.map(String::from), language: "rust".into(), file: "src/lib.rs".into(), start_line: 1, end_line: 2, ..Default::default() }
    }

    #[test]
    fn modules_types_functions_and_impls_project_without_fake_classes() {
        let mut g = SemanticGraph::new();
        let m = g.add(ent(EntityKind::Module, "shop.orders", None));
        let pricing = g.add(ent(EntityKind::Module, "shop.pricing", None));
        let order = g.add(ent(EntityKind::Struct, "shop.orders.Order", Some(&m)));
        let total = g.add(Entity { uses: vec!["lines".into()], ..ent(EntityKind::Method, "shop.orders.Order.total", Some(&order)) });
        g.add(Entity { ty: Some("Vec<Line>".into()), ..ent(EntityKind::Field, "shop.orders.Order.lines", Some(&order)) });
        let priced = g.add(ent(EntityKind::Trait, "shop.pricing.Priced", Some(&pricing)));
        let tax = g.add(ent(EntityKind::Function, "shop.pricing.tax", Some(&pricing)));
        g.edge(&order, &priced, EdgeKind::Implements);
        g.edge(&total, &tax, EdgeKind::Calls);
        g.finish();
        let d = project(&g, &common_root(&g)).unwrap();
        let names: BTreeSet<&str> = d.classes.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["Order", "Priced", "pricing"].into_iter().collect(), "orders has no free functions: no module node");
        let pricing = d.class("pricing").unwrap();
        assert!(is_module_node(pricing));
        assert_eq!(pricing.package, None, "top-level module under root 'shop'");
        let order = d.class("Order").unwrap();
        assert_eq!(order.package.as_deref(), Some("orders"));
        assert_eq!(order.methods[0].calls, vec!["pricing.tax".to_string()]);
        assert_eq!(order.methods[0].source.as_ref().unwrap().symbol, "shop.orders.Order#total()");
        assert_eq!(order.source.as_ref().unwrap().language.as_deref(), Some("rust"));
        let kinds: BTreeSet<(String, String, RelationKind)> = d.relations.iter().map(|r| (r.from.clone(), r.to.clone(), r.kind)).collect();
        assert!(kinds.contains(&("Order".into(), "Priced".into(), RelationKind::Realization)));
        assert!(kinds.contains(&("Order".into(), "pricing".into(), RelationKind::Dependency)));
    }

    #[test]
    fn overloads_and_same_names_keep_distinct_identity() {
        let mut g = SemanticGraph::new();
        let a = g.add(ent(EntityKind::Module, "a", None));
        let b = g.add(ent(EntityKind::Module, "b", None));
        g.add(ent(EntityKind::Struct, "a.Item", Some(&a)));
        g.add(ent(EntityKind::Struct, "b.Item", Some(&b)));
        let f1 = g.add(Entity { params: vec![Param { name: "x".into(), ty: "i32".into() }], ..ent(EntityKind::Function, "a.f", Some(&a)) });
        let f2 = g.add(Entity { params: vec![Param { name: "x".into(), ty: "str".into() }], ..ent(EntityKind::Function, "a.f", Some(&a)) });
        assert_ne!(f1, f2);
        let d = project(&g, "").unwrap();
        assert!(d.class("a.Item").is_some() && d.class("b.Item").is_some());
        let a = d.class("a").unwrap();
        assert_eq!(a.methods.len(), 2);
        assert_eq!(stable_id("rust", "x.rs", "a.f", "f()"), stable_id("rust", "x.rs", "a.f", "f()"));
    }

    #[test]
    fn languages_by_extension() {
        for (f, l) in [("a.rs", "rust"), ("a.tsx", "typescript"), ("a.py", "python"), ("A.java", "java"), ("a.h", "c"), ("a.hpp", "cpp"), ("a.cljc", "clojure")] {
            assert_eq!(language_of(f), Some(l));
        }
        assert_eq!(language_of("README.md"), None);
    }
}

#[cfg(test)]
mod compat_tests {
    use crate::model::*;
    use crate::ops::{apply_all, Mode, Op};

    #[test]
    fn schema_1_links_are_java_and_round_trip_unchanged() {
        let old = r#"{"name":"Order","source":{"symbol":"org.shop.Order","file":"src/Order.java","start_line":3,"end_line":9}}"#;
        let c: Class = serde_json::from_str(old).unwrap();
        let s = c.source.as_ref().unwrap();
        assert_eq!(s.lang(), "java");
        let back = serde_json::to_value(s).unwrap();
        assert_eq!(back.as_object().unwrap().len(), 4, "no new keys for old links: {back}");
    }

    #[test]
    fn link_code_accepts_non_java_symbols() {
        let ops: Vec<Op> = serde_json::from_str(r#"[{"op":"add_class","name":"pricing","stereotype":"namespace"},
            {"op":"link_code","class":"pricing","symbol":"shop.pricing","language":"clojure"},
            {"op":"add_method","class":"pricing","method":{"name":"compute-total!"}},
            {"op":"link_code","class":"pricing","method":"compute-total!","symbol":"shop.pricing#compute-total!(order)","language":"clojure"}]"#).unwrap();
        let d = apply_all(&Design::default(), &ops, Mode::Commit(Status::Accepted, None)).unwrap();
        let c = d.class("pricing").unwrap();
        assert!(c.is_module());
        assert_eq!(c.methods[0].source.as_ref().unwrap().lang(), "clojure");
        let bad: Vec<Op> = serde_json::from_str(r#"[{"op":"add_class","name":"A"},{"op":"link_code","class":"A","symbol":"x","language":"cobol"}]"#).unwrap();
        assert!(apply_all(&Design::default(), &bad, Mode::Commit(Status::Accepted, None)).is_err());
    }

    #[test]
    fn manifest_accepts_all_languages_and_keeps_schema_1() {
        let m = crate::project::Manifest::parse(r#"{"version":1,"name":"x","language":"rust","languages":["typescript","clojure"],"roots":["src"]}"#).unwrap();
        assert_eq!(m.all_languages(), ["rust", "typescript", "clojure"]);
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join("src")).unwrap();
        assert!(m.validate(d.path(), None).is_empty(), "{:?}", m.validate(d.path(), None));
        let bad = crate::project::Manifest::parse(r#"{"language":"cobol","roots":["src"]}"#).unwrap();
        assert!(bad.validate(d.path(), None).iter().any(|w| w.contains("cobol")));
        let old = crate::project::Manifest::parse(r#"{"version":1,"name":"x","language":"java","roots":["src/main/java"],"custom":1}"#).unwrap();
        assert!(!old.to_json().contains("languages") && old.to_json().contains("custom"));
    }
}
