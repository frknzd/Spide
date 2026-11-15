//! Import the structure of real code into a (read-only) Spide design, so the intended design can be
//! compared with reality. Java first, source-only, via tree-sitter.

use spide_core::model::*;
use spide_core::ops::{apply_all, Mode, Op};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use tree_sitter::{Node, Parser};

mod facts;
pub mod languages;

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
    /// Local variables (incl. for-each variables) and their declared types.
    locals: Vec<(String, String)>,
    facts: CodeFacts,
}

#[derive(Debug, Default, Clone)]
struct RawType {
    name: String,
    java_package: String,
    source: SourceSymbol,
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
            if e.file_type().map(|t| t.is_symlink()).unwrap_or(true) { continue; }
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
    if matches!(n.kind(), "class_declaration" | "interface_declaration" | "enum_declaration" | "record_declaration" | "class_body" | "method_declaration") { return; }
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
        "local_variable_declaration" => {
            let ty = n.child_by_field_name("type").map(|t| text(t, src).to_string()).unwrap_or_default();
            let mut dc = n.walk();
            for dl in n.children_by_field_name("declarator", &mut dc) {
                if let Some(name) = dl.child_by_field_name("name") {
                    m.locals.push((text(name, src).to_string(), ty.clone()));
                }
            }
        }
        "enhanced_for_statement" => {
            if let (Some(t), Some(name)) = (n.child_by_field_name("type"), n.child_by_field_name("name")) {
                m.locals.push((text(name, src).to_string(), text(t, src).to_string()));
            }
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
    m.facts = facts::facts(n, n.child_by_field_name("body"), src);
    m.facts.params = m.params.len() as u32;
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
            "method_declaration" | "constructor_declaration" | "compact_constructor_declaration" => t.methods.push(parse_method(ch, src)),
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
        source: SourceSymbol { symbol: format!("{}.{}", pkg, text(n.child_by_field_name("name")?, src)).trim_start_matches('.').into(), start_line: n.start_position().row as u32 + 1, end_line: n.end_position().row as u32 + 1, ..Default::default() },
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

fn collect_types(n: Node, src: &[u8], pkg: &str, enclosing: &str, out: &mut Vec<RawType>) {
    if let Some(mut t) = parse_type(n, src, pkg) {
        let qualified = if enclosing.is_empty() { t.source.symbol.clone() } else { format!("{enclosing}.{}", t.name) };
        t.source.symbol = qualified.clone();
        if let Some(body) = n.child_by_field_name("body") {
            let mut cursor = body.walk();
            for child in body.children(&mut cursor) { collect_types(child, src, pkg, &qualified, out); }
        }
        out.push(t);
    }
}

fn parse_file(parser: &mut Parser, path: &Path, dir: &Path) -> Result<Vec<RawType>, String> {
    let src = std::fs::read(path).map_err(|e| e.to_string())?;
    let tree = parser.parse(&src, None).ok_or("parse failed")?;
    let root = tree.root_node();
    let mut pkg = String::new();
    let mut out = vec![];
    let mut c = root.walk();
    for ch in root.children(&mut c) {
        if ch.kind() == "package_declaration" {
            pkg = text(ch, &src).trim_start_matches("package").trim().trim_end_matches(';').trim().to_string();
        } else {
            collect_types(ch, &src, &pkg, "", &mut out);
        }
    }
    if root.has_error() {
        return Err("syntax errors".into());
    }
    let rel = path.strip_prefix(dir).unwrap_or(path).display().to_string();
    for t in &mut out {
        t.source.file = rel.clone();
        for m in &mut t.methods {
            m.facts.file = rel.clone();
        }
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
    import_files(dir, code_root, java_files(dir))
}

/// Safe explicit repository scope. Paths are relative and cannot escape via symlinks.
pub fn scoped_files(repo: &Path, roots: &[String], exclusions: &[String]) -> Result<Vec<PathBuf>, String> {
    scoped_files_of(repo, roots, exclusions, "java")
}

/// Like `scoped_files`, for any supported language.
pub fn scoped_files_of(repo: &Path, roots: &[String], exclusions: &[String], lang: &str) -> Result<Vec<PathBuf>, String> {
    let repo = repo.canonicalize().map_err(|e| e.to_string())?;
    if roots.is_empty() { return Err("Select at least one source root".into()); }
    for exclusion in exclusions {
        let p = Path::new(exclusion);
        if p.is_absolute() || p.components().any(|c| matches!(c, std::path::Component::ParentDir)) { return Err("Exclusions must be repository-relative".into()); }
    }
    let mut files = BTreeSet::new();
    for root in roots {
        let rel = Path::new(root);
        if rel.is_absolute() || rel.components().any(|c| matches!(c, std::path::Component::ParentDir)) { return Err("Source roots must be repository-relative".into()); }
        let path = repo.join(rel).canonicalize().map_err(|e| e.to_string())?;
        if !path.starts_with(&repo) { return Err("Source root escapes repository".into()); }
        let found = if lang == "java" { java_files(&path) } else { languages::files_of(&path, lang, false) };
        for file in found {
            let canonical = file.canonicalize().map_err(|e| e.to_string())?;
            if !canonical.starts_with(&repo) { continue; }
            let relative = canonical.strip_prefix(&repo).unwrap();
            if !exclusions.iter().any(|e| relative.starts_with(e)) { files.insert(canonical); }
        }
    }
    Ok(files.into_iter().collect())
}

pub fn import_java_scoped(repo: &Path, roots: &[String], exclusions: &[String], code_root: Option<&str>) -> Result<ImportResult, String> {
    import_files(repo, code_root, scoped_files(repo, roots, exclusions)?)
}

fn import_files(dir: &Path, code_root: Option<&str>, files: Vec<PathBuf>) -> Result<ImportResult, String> {
    let mut parser = Parser::new();
    parser.set_language(&tree_sitter_java::LANGUAGE.into()).map_err(|e| e.to_string())?;
    if files.is_empty() {
        return Err(format!("no .java files under {}", dir.display()));
    }
    let mut types: Vec<RawType> = vec![];
    let mut skipped = vec![];
    for f in &files {
        match parse_file(&mut parser, f, dir) {
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
            t.source.symbol.clone()
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
        let mut seen_methods: BTreeSet<String> = BTreeSet::new();
        let signature = |m: &RawMethod| format!("{}({})", m.name, m.params.iter().map(|(_, t)| t.as_str()).collect::<Vec<_>>().join(","));
        let methods: Vec<Method> = t
            .methods
            .iter()
            .filter(|m| seen_methods.insert(signature(m)))
            .map(|m| {
                let locals: BTreeMap<&str, &str> =
                    m.params.iter().chain(m.locals.iter()).filter(|(_, ty)| ty != "var").map(|(n, ty)| (n.as_str(), ty.as_str())).collect();
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
                    name: if t.methods.iter().filter(|other| other.name == m.name).count() > 1 { signature(m) } else { m.name.clone() },
                    source: Some(SourceSymbol { symbol: format!("{}#{}", t.source.symbol, signature(m)), file: t.source.file.clone(), start_line: m.facts.start_line, end_line: m.facts.end_line, ..Default::default() }),
                    params: m.params.iter().map(|(n, ty)| Param { name: n.clone(), ty: ty.clone() }).collect(),
                    returns: m.returns.clone(),
                    visibility: if t.kind == "interface" && m.visibility == Visibility::Package { Visibility::Public } else { m.visibility },
                    is_static: m.is_static,
                    is_abstract: m.is_abstract && (t.kind != "class" || t.is_abstract),
                    uses,
                    calls: calls.into_iter().collect(),
                    code: Some(m.facts.clone()),
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
    for t in &types { if let Some(c) = design.class_mut(&name_of(t)) { c.source = Some(t.source.clone()); } }
    design.name = format!("code: {}",  dir.file_name().and_then(|n| n.to_str()).unwrap_or("project"));
    design.settings.code_root = (!root.is_empty()).then_some(root);
    Ok(ImportResult { design, files: files.len(), skipped })
}

/// A whole (possibly mixed-language) project: Java through the Java importer, every other
/// language through its tree-sitter adapter into the semantic graph, all projected into one design.
#[derive(Debug, Default)]
pub struct ProjectImport {
    pub design: Design,
    pub graph: spide_core::symbols::SemanticGraph,
    pub files: usize,
    pub skipped: Vec<String>,
    /// language → files indexed
    pub per_language: BTreeMap<String, usize>,
}

pub fn import_project(repo: &Path, languages: &[String], roots: &[String], exclusions: &[String], code_root: Option<&str>) -> Result<ProjectImport, String> {
    let mut out = ProjectImport { graph: spide_core::symbols::SemanticGraph::new(), ..Default::default() };
    let mut java: Option<Design> = None;
    let canon = repo.canonicalize().map_err(|e| e.to_string())?;
    for lang in languages {
        if !spide_core::symbols::is_supported_language(lang) {
            out.skipped.push(format!("language {lang} is not supported"));
            continue;
        }
        let mut files = scoped_files_of(repo, roots, exclusions, lang)?;
        if lang == "cpp" && languages.iter().any(|l| l == "c") {
            files.retain(|f| f.extension().is_none_or(|e| e != "h")); // `.h` belongs to the C side
        }
        out.per_language.insert(lang.clone(), files.len());
        if files.is_empty() {
            continue;
        }
        out.files += files.len();
        if lang == "java" {
            let r = import_files(&canon, code_root, files)?;
            out.skipped.extend(r.skipped);
            java = Some(r.design);
        } else {
            let (g, skipped) = languages::index_files(&canon, lang, &files);
            out.skipped.extend(skipped);
            out.graph.merge(g);
        }
    }
    if out.files == 0 {
        return Err(format!("no {} source files under {}", languages.join("/"), roots.join(", ")));
    }
    out.graph.finish();
    let root = code_root.map(String::from).unwrap_or_else(|| spide_core::symbols::common_root(&out.graph));
    let projected = if out.graph.entities.is_empty() { None } else { Some(spide_core::symbols::project(&out.graph, &root)?) };
    out.design = match (java, projected) {
        (Some(j), None) => j,
        (None, Some(p)) => p,
        (Some(mut j), Some(p)) => {
            // mixed: same-named nodes from another language keep their qualified name
            let taken: BTreeSet<String> = j.classes.iter().map(|c| c.name.clone()).collect();
            let rename: BTreeMap<String, String> = p.classes.iter().filter(|c| taken.contains(&c.name)).map(|c| (c.name.clone(), c.source.as_ref().map(|s| s.symbol.clone()).unwrap_or_else(|| format!("{}~", c.name)))).collect();
            for mut c in p.classes {
                if let Some(n) = rename.get(&c.name) {
                    c.name = n.clone();
                }
                j.classes.push(c);
            }
            for mut r in p.relations {
                r.from = rename.get(&r.from).cloned().unwrap_or(r.from);
                r.to = rename.get(&r.to).cloned().unwrap_or(r.to);
                r.id = format!("{}x", r.id);
                j.relations.push(r);
            }
            for pk in p.packages {
                if !j.packages.iter().any(|x| x.name == pk.name) {
                    j.packages.push(pk);
                }
            }
            j
        }
        (None, None) => Design::default(),
    };
    out.design.name = format!("code: {}", repo.file_name().and_then(|n| n.to_str()).unwrap_or("project"));
    if out.design.settings.code_root.is_none() && !root.is_empty() {
        out.design.settings.code_root = Some(root);
    }
    Ok(out)
}
