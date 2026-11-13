//! Import the structure of real code into a (read-only) Spide design, so the intended design can be
//! compared with reality. Java first, source-only, via tree-sitter.

use spide_core::model::*;
use spide_core::ops::{apply_all, Mode, Op};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use tree_sitter::{Node, Parser};

#[derive(Debug, Default)]
pub struct ImportResult {
    pub design: Design,
    pub files: usize,
    /// Files that could not be read or parsed cleanly.
    pub skipped: Vec<String>,
}

#[derive(Debug, Default, Clone)]
struct RawMember {
    name: String,
    ty: String,
    visibility: Visibility,
    is_static: bool,
}

#[derive(Debug, Default, Clone)]
struct RawMethod {
    name: String,
    params: Vec<(String, String)>,
    returns: Option<String>,
    visibility: Visibility,
    is_static: bool,
    is_abstract: bool,
    /// (receiver expression kind, receiver name, method) — resolved later
    invocations: Vec<(Option<String>, String)>,
    identifiers: BTreeSet<String>,
}

#[derive(Debug, Default, Clone)]
struct RawType {
    name: String,
    java_package: String,
    kind: &'static str, // class | interface | enum | record
    is_abstract: bool,
    extends: Vec<String>,
    implements: Vec<String>,
    fields: Vec<RawMember>,
    methods: Vec<RawMethod>,
}

const SKIP_DIRS: [&str; 8] = ["target", "build", "out", "node_modules", ".git", ".gradle", ".idea", "bin"];

fn java_files(root: &Path) -> Vec<PathBuf> {
    let mut out = vec![];
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            if p.is_dir() {
                // production code only: skip build output, VCS and test sources
                let is_test = name == "test" && p.parent().is_some_and(|x| x.ends_with("src"));
                if !SKIP_DIRS.contains(&name.as_str()) && !is_test {
                    stack.push(p);
                }
            } else if name.ends_with(".java") && name != "package-info.java" && name != "module-info.java" {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

fn text<'a>(n: Node, src: &'a [u8]) -> &'a str {
    n.utf8_text(src).unwrap_or("")
}

fn modifiers(n: Node, src: &[u8]) -> (Visibility, bool, bool) {
    let mut vis = Visibility::Package;
    let (mut st, mut abs) = (false, false);
    let mut c = n.walk();
    for ch in n.children(&mut c) {
        if ch.kind() == "modifiers" {
            for w in text(ch, src).split_whitespace() {
                match w {
                    "public" => vis = Visibility::Public,
                    "private" => vis = Visibility::Private,
                    "protected" => vis = Visibility::Protected,
                    "static" => st = true,
                    "abstract" => abs = true,
                    _ => {}
                }
            }
        }
    }
    (vis, st, abs)
}

/// Simple type names inside a type node (`Map<String, List<Loan>>` → Map, String, List, Loan).
fn type_names(n: Node, src: &[u8], out: &mut Vec<String>) {
    if n.kind() == "type_identifier" {
        out.push(text(n, src).to_string());
    }
    let mut c = n.walk();
    for ch in n.children(&mut c) {
        type_names(ch, src, out);
    }
}

fn collect_body(n: Node, src: &[u8], m: &mut RawMethod) {
    match n.kind() {
        "method_invocation" => {
            let name = n.child_by_field_name("name").map(|x| text(x, src).to_string()).unwrap_or_default();
            let recv = n.child_by_field_name("object").map(|o| match o.kind() {
                "this" => "this".to_string(),
                "field_access" => {
                    let obj = o.child_by_field_name("object").map(|x| text(x, src)).unwrap_or("");
                    let f = o.child_by_field_name("field").map(|x| text(x, src)).unwrap_or("");
                    if obj == "this" { f.to_string() } else { text(o, src).to_string() }
                }
                _ => text(o, src).to_string(),
            });
            m.invocations.push((recv, name));
        }
        "field_access" => {
            if n.child_by_field_name("object").is_some_and(|o| o.kind() == "this") {
                if let Some(f) = n.child_by_field_name("field") {
                    m.identifiers.insert(text(f, src).to_string());
                }
            }
        }
        "identifier" => {
            m.identifiers.insert(text(n, src).to_string());
        }
        _ => {}
    }
    let mut c = n.walk();
    for ch in n.children(&mut c) {
        collect_body(ch, src, m);
    }
}

fn parse_method(n: Node, src: &[u8]) -> RawMethod {
    let (visibility, is_static, is_abstract) = modifiers(n, src);
    let mut m = RawMethod {
        name: n.child_by_field_name("name").map(|x| text(x, src).to_string()).unwrap_or_default(),
        returns: n.child_by_field_name("type").map(|t| text(t, src).to_string()).filter(|t| t != "void"),
        visibility,
        is_static,
        is_abstract: is_abstract || n.child_by_field_name("body").is_none(),
        ..Default::default()
    };
    if let Some(ps) = n.child_by_field_name("parameters") {
        let mut c = ps.walk();
        for p in ps.children(&mut c).filter(|p| matches!(p.kind(), "formal_parameter" | "spread_parameter")) {
            let ty = p.child_by_field_name("type").map(|t| text(t, src).to_string()).unwrap_or_else(|| {
                // spread_parameter: type is the first named child
                p.named_child(0).map(|t| text(t, src).to_string()).unwrap_or_default()
            });
            let name = p
                .child_by_field_name("name")
                .or_else(|| p.named_child(1).and_then(|d| d.child_by_field_name("name").or(Some(d))))
                .map(|x| text(x, src).to_string())
                .unwrap_or_default();
            m.params.push((name, ty));
        }
    }
    if let Some(b) = n.child_by_field_name("body") {
        collect_body(b, src, &mut m);
    }
    m
}

fn parse_body(body: Node, src: &[u8], t: &mut RawType) {
    let mut c = body.walk();
    for ch in body.children(&mut c) {
        match ch.kind() {
            "field_declaration" | "constant_declaration" => {
                let (visibility, is_static, _) = modifiers(ch, src);
                let ty = ch.child_by_field_name("type").map(|x| text(x, src).to_string()).unwrap_or_default();
                let mut dc = ch.walk();
                for d in ch.children_by_field_name("declarator", &mut dc) {
                    if let Some(name) = d.child_by_field_name("name") {
                        t.fields.push(RawMember { name: text(name, src).to_string(), ty: ty.clone(), visibility, is_static });
                    }
                }
            }
            "method_declaration" => t.methods.push(parse_method(ch, src)),
            "enum_body_declarations" => parse_body(ch, src, t),
            _ => {}
        }
    }
}

fn parse_type(n: Node, src: &[u8], pkg: &str) -> Option<RawType> {
    let kind = match n.kind() {
        "class_declaration" => "class",
        "interface_declaration" => "interface",
        "enum_declaration" => "enum",
        "record_declaration" => "record",
        _ => return None,
    };
    let (_, _, is_abstract) = modifiers(n, src);
    let mut t = RawType {
        name: text(n.child_by_field_name("name")?, src).to_string(),
        java_package: pkg.to_string(),
        kind,
        is_abstract,
        ..Default::default()
    };
    let mut c = n.walk();
    for ch in n.children(&mut c) {
        match ch.kind() {
            "superclass" => type_names(ch, src, &mut t.extends),
            "super_interfaces" => type_names(ch, src, &mut t.implements),
            // interface X extends A, B
            "extends_interfaces" => type_names(ch, src, &mut t.extends),
            _ => {}
        }
    }
    if kind == "record" {
        if let Some(ps) = n.child_by_field_name("parameters") {
            let mut pc = ps.walk();
            for p in ps.children(&mut pc).filter(|p| p.kind() == "formal_parameter") {
                let ty = p.child_by_field_name("type").map(|x| text(x, src).to_string()).unwrap_or_default();
                let name = p.child_by_field_name("name").map(|x| text(x, src).to_string()).unwrap_or_default();
                t.fields.push(RawMember { name, ty, visibility: Visibility::Private, is_static: false });
            }
        }
    }
    if let Some(b) = n.child_by_field_name("body") {
        parse_body(b, src, &mut t);
    }
    Some(t)
}

fn parse_file(parser: &mut Parser, path: &Path) -> Result<Vec<RawType>, String> {
    let src = std::fs::read(path).map_err(|e| e.to_string())?;
    let tree = parser.parse(&src, None).ok_or("parse failed")?;
    let root = tree.root_node();
    let mut pkg = String::new();
    let mut out = vec![];
    let mut c = root.walk();
    for ch in root.children(&mut c) {
        if ch.kind() == "package_declaration" {
            pkg = text(ch, &src).trim_start_matches("package").trim().trim_end_matches(';').trim().to_string();
        } else if let Some(t) = parse_type(ch, &src, &pkg) {
            out.push(t);
        }
    }
    if root.has_error() && out.is_empty() {
        return Err("syntax errors".into());
    }
    Ok(out)
}

/// Design package for a Java package, relative to `root` (`org.acme.lib.domain` → `domain`).
fn design_package(java: &str, root: &str) -> Option<String> {
    let rel = if root.is_empty() {
        java
    } else if java == root {
        ""
    } else {
        java.strip_prefix(root).and_then(|r| r.strip_prefix('.')).unwrap_or(java)
    };
    (!rel.is_empty()).then(|| rel.to_string())
}

fn common_prefix(pkgs: &BTreeSet<String>) -> String {
    let mut it = pkgs.iter();
    let Some(first) = it.next() else { return String::new() };
    let mut parts: Vec<&str> = first.split('.').collect();
    for p in it {
        let other: Vec<&str> = p.split('.').collect();
        let n = parts.iter().zip(other.iter()).take_while(|(a, b)| a == b).count();
        parts.truncate(n);
    }
    parts.join(".")
}

const COLLECTIONS: [&str; 10] = ["List", "Set", "Collection", "Iterable", "Map", "Queue", "Deque", "Stream", "Optional", "SortedSet"];

/// Import all production `.java` files under `dir`. `code_root` (e.g. `org.acme.library`) maps Java
/// packages to design packages; without it the common prefix of all packages is used.
pub fn import_java(dir: &Path, code_root: Option<&str>) -> Result<ImportResult, String> {
    let mut parser = Parser::new();
    parser.set_language(&tree_sitter_java::LANGUAGE.into()).map_err(|e| e.to_string())?;
    let files = java_files(dir);
    if files.is_empty() {
        return Err(format!("no .java files under {}", dir.display()));
    }
    let mut types: Vec<RawType> = vec![];
    let mut skipped = vec![];
    for f in &files {
        match parse_file(&mut parser, f) {
            Ok(ts) => types.extend(ts),
            Err(e) => skipped.push(format!("{}: {e}", f.display())),
        }
    }

    // unique design names: simple name, or Name_pkg on collisions
    let mut count: BTreeMap<String, usize> = BTreeMap::new();
    for t in &types {
        *count.entry(t.name.clone()).or_default() += 1;
    }
    let name_of = |t: &RawType| {
        if count[&t.name] > 1 {
            format!("{}_{}", t.name, t.java_package.rsplit('.').next().unwrap_or(""))
        } else {
            t.name.clone()
        }
    };
    let by_simple: BTreeMap<String, String> =
        types.iter().filter(|t| count[&t.name] == 1).map(|t| (t.name.clone(), t.name.clone())).collect();
    let resolve = |simple: &str| by_simple.get(simple).cloned();

    let pkgs: BTreeSet<String> = types.iter().map(|t| t.java_package.clone()).collect();
    let root = code_root.map(String::from).unwrap_or_else(|| {
        let p = common_prefix(&pkgs);
        // a single package: keep its last segment as the design package
        if pkgs.len() == 1 { p.rsplit_once('.').map(|(a, _)| a.to_string()).unwrap_or_default() } else { p }
    });

    let mut ops: Vec<Op> = vec![];
    let mut rels: Vec<Op> = vec![];
    for t in &types {
        let me = name_of(t);
        let fields: BTreeMap<&str, &str> = t.fields.iter().map(|f| (f.name.as_str(), f.ty.as_str())).collect();
        let attributes: Vec<Attribute> = t
            .fields
            .iter()
            .filter(|f| !(t.kind == "enum" && f.is_static && f.ty.is_empty()))
            .map(|f| Attribute { name: f.name.clone(), ty: f.ty.clone(), visibility: f.visibility, is_static: f.is_static, ..Default::default() })
            .collect();
        let mut seen_methods: BTreeSet<(String, usize)> = BTreeSet::new();
        let methods: Vec<Method> = t
            .methods
            .iter()
            .filter(|m| seen_methods.insert((m.name.clone(), m.params.len())))
            .map(|m| {
                let locals: BTreeMap<&str, &str> = m.params.iter().map(|(n, ty)| (n.as_str(), ty.as_str())).collect();
                let mut calls: BTreeSet<String> = BTreeSet::new();
                for (recv, name) in &m.invocations {
                    match recv.as_deref() {
                        None | Some("this") => {
                            calls.insert(name.clone());
                        }
                        Some(r) => {
                            let ty = fields.get(r).or_else(|| locals.get(r)).copied();
                            let mut names = vec![];
                            if let Some(ty) = ty {
                                names.extend(ty.split(|c: char| !c.is_alphanumeric() && c != '_').map(String::from));
                            } else {
                                // static call on a type: Foo.bar()
                                names.push(r.to_string());
                            }
                            if let Some(target) = names.iter().filter(|n| !COLLECTIONS.contains(&n.as_str())).find_map(|n| resolve(n)) {
                                if target != me {
                                    calls.insert(format!("{target}.{name}"));
                                } else {
                                    calls.insert(name.clone());
                                }
                            }
                        }
                    }
                }
                let uses: Vec<String> = t.fields.iter().filter(|f| m.identifiers.contains(&f.name)).map(|f| f.name.clone()).collect();
                Method {
                    name: m.name.clone(),
                    params: m.params.iter().map(|(n, ty)| Param { name: n.clone(), ty: ty.clone() }).collect(),
                    returns: m.returns.clone(),
                    visibility: if t.kind == "interface" && m.visibility == Visibility::Package { Visibility::Public } else { m.visibility },
                    is_static: m.is_static,
                    is_abstract: m.is_abstract && (t.kind != "class" || t.is_abstract),
                    uses,
                    calls: calls.into_iter().collect(),
                    ..Default::default()
                }
            })
            .collect();
        let stereotype = match t.kind {
            "interface" => Some("interface".to_string()),
            "enum" => Some("enum".to_string()),
            "record" => Some("value".to_string()),
            _ if t.is_abstract => Some("abstract".to_string()),
            _ => None,
        };
        ops.push(Op::AddClass {
            name: me.clone(),
            stereotype,
            package: design_package(&t.java_package, &root),
            is_abstract: t.is_abstract,
            attributes,
            methods,
            responsibilities: vec![],
            rationale: None,
            aggregate_root: false,
            // a type outside the code root is someone else's code
            external: !root.is_empty() && t.java_package != root && !t.java_package.starts_with(&format!("{root}.")),
            role: None,
        });
        // structure: inheritance, realization, associations through fields
        let mut targets: BTreeSet<(String, RelationKind)> = BTreeSet::new();
        for e in &t.extends {
            if let Some(x) = resolve(e).filter(|x| *x != me) {
                targets.insert((x, RelationKind::Inheritance));
            }
        }
        for i in &t.implements {
            if let Some(x) = resolve(i).filter(|x| *x != me) {
                targets.insert((x, RelationKind::Realization));
            }
        }
        for f in t.fields.iter().filter(|f| !f.is_static) {
            let names: Vec<&str> = f.ty.split(|c: char| !c.is_alphanumeric() && c != '_').filter(|s| !s.is_empty()).collect();
            let many = f.ty.contains('[') || names.iter().any(|n| COLLECTIONS.contains(n));
            for n in names {
                if let Some(x) = resolve(n).filter(|x| *x != me) {
                    if targets.insert((x.clone(), RelationKind::Association)) {
                        rels.push(Op::AddRelation {
                            from: me.clone(),
                            to: x,
                            kind: RelationKind::Association,
                            label: None,
                            from_mult: None,
                            to_mult: Some(if many { "*".into() } else { "1".into() }),
                            directed: Some(true),
                            rationale: None,
                            interaction: None,
                        });
                    }
                }
            }
        }
        for (x, kind) in targets.into_iter().filter(|(_, k)| *k != RelationKind::Association) {
            rels.push(Op::AddRelation {
                from: me.clone(),
                to: x,
                kind,
                label: None,
                from_mult: None,
                to_mult: None,
                directed: None,
                rationale: None,
                interaction: None,
            });
        }
    }
    ops.extend(rels);
    let mut design = apply_all(&Design::default(), &ops, Mode::Commit(Status::Accepted, Some("import"))).map_err(|e| e.0)?;
    design.name = format!("code: {}", dir.file_name().and_then(|n| n.to_str()).unwrap_or("project"));
    design.settings.code_root = (!root.is_empty()).then_some(root);
    Ok(ImportResult { design, files: files.len(), skipped })
}
