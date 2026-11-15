//! Language adapters: tree-sitter parsers for Rust, TypeScript, Python, C, C++ and Clojure that
//! produce the language-neutral `SemanticGraph` (Java keeps its dedicated importer).
//!
//! Each adapter reports what it extracts (`CAPABILITIES`) and its honest limits (macros, the C
//! preprocessor, dynamic dispatch). Resolution is name-based within the workspace: a call that
//! does not resolve to exactly one workspace entity is recorded as unresolved, never guessed.

mod clojure;
mod ts_family;

use spide_core::model::{CodeFacts, Visibility};
use spide_core::symbols::{EdgeKind, Entity, EntityKind, SemanticGraph};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use tree_sitter::{Node, Parser};

/// What each adapter extracts, for `doctor`/coverage reporting.
pub const CAPABILITIES: &[(&str, &[&str], &[&str])] = &[
    ("rust", &["modules", "structs", "enums", "traits", "impls", "functions", "methods", "fields", "calls", "facts"], &["macro-generated items are not expanded", "calls through trait objects/generics resolve by name only"]),
    ("typescript", &["modules", "classes", "interfaces", "enums", "type aliases", "functions", "arrow functions", "methods", "fields", "calls", "facts"], &["structural typing is not inferred", "calls through untyped values are unresolved"]),
    ("python", &["modules", "classes", "protocols", "functions", "methods", "fields", "calls", "facts"], &["dynamic attributes and monkey-patching are invisible", "calls on untyped receivers resolve by unique name only"]),
    ("java", &["packages", "classes", "interfaces", "enums", "records", "methods", "fields", "calls", "facts"], &["reflection and generated code are invisible"]),
    ("c", &["file modules", "structs", "unions", "enums", "functions", "fields", "calls", "facts"], &["preprocessor conditionals are not evaluated", "function pointers are unresolved"]),
    ("cpp", &["namespaces", "classes", "structs", "interfaces (pure virtual)", "functions", "methods", "fields", "calls", "facts"], &["templates are not instantiated", "preprocessor conditionals are not evaluated", "overload resolution is by name"]),
    ("clojure", &["namespaces", "functions", "protocols", "records/types", "multimethods", "calls", "facts"], &["macros are not expanded", "dynamic vars and higher-order calls resolve by name only"]),
];

pub fn text<'a>(n: Node, src: &'a [u8]) -> &'a str {
    n.utf8_text(src).unwrap_or("")
}

pub fn fnv64(s: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// A call site: optional receiver (`self`, a field, a variable, a namespace alias) and name.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Call {
    pub recv: Option<String>,
    /// Explicit path (`pricing::tax`, `pricing/tax`, `shop::tax`) — the qualifier only.
    pub path: Option<String>,
    pub name: String,
}

#[derive(Debug, Clone)]
pub enum ParentRef {
    None,
    Item(usize),
    /// A type by name, resolved after all files (Rust impls, C++ out-of-class definitions).
    TypeName { module: Option<usize>, name: String },
}

#[derive(Debug, Clone)]
pub struct Item {
    pub e: Entity,
    pub parent: ParentRef,
    pub calls: Vec<Call>,
    pub idents: BTreeSet<String>,
    /// (type name, Implements|Extends)
    pub heritage: Vec<(String, EdgeKind)>,
    pub type_refs: Vec<String>,
    pub has_body: bool,
    /// Local variable/parameter → declared type (receiver typing).
    pub locals: BTreeMap<String, String>,
    /// Clojure: namespace alias → namespace.
    pub aliases: BTreeMap<String, String>,
}

impl Item {
    pub fn new(kind: EntityKind, name: &str, parent: ParentRef) -> Item {
        Item {
            e: Entity { kind: Some(kind), name: name.to_string(), ..Default::default() },
            parent,
            calls: vec![],
            idents: BTreeSet::new(),
            heritage: vec![],
            type_refs: vec![],
            has_body: false,
            locals: BTreeMap::new(),
            aliases: BTreeMap::new(),
        }
    }
}

/// Collects items from all files of one language, then resolves names into a graph.
#[derive(Default)]
pub struct Builder {
    pub lang: String,
    pub items: Vec<Item>,
    /// module qualified name → item index (dedups C++ namespaces / C header+source pairs)
    pub modules: BTreeMap<String, usize>,
    pub documents: BTreeMap<String, spide_core::symbols::Document>,
    pub unresolved: Vec<String>,
    pub limits: Vec<String>,
    pub skipped: Vec<String>,
    /// (module item, type name, contract name): `impl Trait for Type`, `extend-protocol`.
    pub pending_impls: Vec<(Option<usize>, String, String)>,
}

impl Builder {
    pub fn push(&mut self, it: Item) -> usize {
        self.items.push(it);
        self.items.len() - 1
    }

    /// The module item for a qualified name (created once).
    pub fn module(&mut self, qualified: &str, kind: EntityKind, file: &str, line: u32) -> usize {
        if let Some(&i) = self.modules.get(qualified) {
            return i;
        }
        let mut it = Item::new(kind, qualified.rsplit('.').next().unwrap_or(qualified), ParentRef::None);
        it.e.qualified = qualified.to_string();
        it.e.file = file.to_string();
        it.e.start_line = line;
        it.e.end_line = line;
        it.e.language = self.lang.clone();
        let i = self.push(it);
        self.modules.insert(qualified.to_string(), i);
        i
    }

    fn qualified_of(&self, i: usize, resolved_parent: &BTreeMap<usize, usize>) -> String {
        let it = &self.items[i];
        if !it.e.qualified.is_empty() {
            return it.e.qualified.clone();
        }
        match resolved_parent.get(&i) {
            Some(&p) => format!("{}.{}", self.qualified_of(p, resolved_parent), it.e.name),
            None => it.e.name.clone(),
        }
    }

    pub fn finish(mut self) -> (SemanticGraph, Vec<String>) {
        let lang = self.lang.clone();
        // ---- resolve parents
        let types_by_name: BTreeMap<String, Vec<usize>> = self.items.iter().enumerate().filter(|(_, it)| it.e.kind.is_some_and(EntityKind::is_type)).fold(BTreeMap::new(), |mut m, (i, it)| {
            m.entry(it.e.name.clone()).or_default().push(i);
            m
        });
        let mut parent: BTreeMap<usize, usize> = BTreeMap::new();
        let mut dropped: BTreeSet<usize> = BTreeSet::new();
        for i in 0..self.items.len() {
            match self.items[i].parent.clone() {
                ParentRef::None => {}
                ParentRef::Item(p) => {
                    parent.insert(i, p);
                }
                ParentRef::TypeName { module, name } => {
                    let cands = types_by_name.get(&name).cloned().unwrap_or_default();
                    let pick = cands.iter().copied().find(|c| module.is_some() && self.items[*c].parent_item() == module).or_else(|| (cands.len() == 1).then(|| cands[0]));
                    match pick {
                        Some(t) => {
                            parent.insert(i, t);
                        }
                        None => {
                            self.unresolved.push(format!("{}: owner type {name} of {} not found", self.items[i].e.file, self.items[i].e.name));
                            dropped.insert(i);
                        }
                    }
                }
            }
        }
        for (module, ty, contract) in std::mem::take(&mut self.pending_impls) {
            let cands = types_by_name.get(&ty).cloned().unwrap_or_default();
            match cands.iter().copied().find(|c| module.is_some() && self.items[*c].parent_item() == module).or_else(|| (cands.len() == 1).then(|| cands[0])) {
                Some(t) => self.items[t].heritage.push((contract, EdgeKind::Implements)),
                None => self.unresolved.push(format!("impl {contract} for {ty}: type outside the workspace")),
            }
        }
        // ---- merge declarations with definitions (C/C++ prototypes, in-class declarations)
        let mut key_of: BTreeMap<(Option<usize>, String, usize), usize> = BTreeMap::new();
        for i in 0..self.items.len() {
            if dropped.contains(&i) || !self.items[i].e.kind.is_some_and(EntityKind::is_callable) {
                continue;
            }
            let k = (parent.get(&i).copied(), self.items[i].e.name.clone(), self.items[i].e.params.len());
            match key_of.get(&k).copied() {
                Some(j) => {
                    // keep the definition (body) as the entity, inherit the declaration's visibility
                    let (keep, drop) = if self.items[i].has_body && !self.items[j].has_body { (i, j) } else { (j, i) };
                    // the declaration knows the access level and staticness; the definition the body
                    let (vis, is_static, is_abs) = (self.items[drop].e.visibility, self.items[drop].e.is_static, self.items[drop].e.is_abstract);
                    if self.items[keep].e.visibility == Visibility::Package {
                        self.items[keep].e.visibility = vis;
                    }
                    self.items[keep].e.is_static |= is_static;
                    if !self.items[keep].has_body {
                        self.items[keep].e.is_abstract |= is_abs;
                    }
                    key_of.insert(k, keep);
                    if self.items[drop].has_body && self.items[keep].has_body {
                        // two bodies: distinct entities (e.g. same name in two C files) — keep both
                        continue;
                    }
                    dropped.insert(drop);
                }
                None => {
                    key_of.insert(k, i);
                }
            }
        }
        // ---- qualified names, ids
        let mut g = SemanticGraph::new();
        let mut id_of: BTreeMap<usize, String> = BTreeMap::new();
        let order: Vec<usize> = {
            // parents before children
            let mut v: Vec<usize> = (0..self.items.len()).filter(|i| !dropped.contains(i)).collect();
            let depth = |mut i: usize| {
                let mut d = 0;
                while let Some(&p) = parent.get(&i) {
                    d += 1;
                    i = p;
                    if d > 64 {
                        break;
                    }
                }
                d
            };
            v.sort_by_key(|i| (depth(*i), *i));
            v
        };
        for &i in &order {
            let q = self.qualified_of(i, &parent);
            let mut e = self.items[i].e.clone();
            e.qualified = q;
            e.language = lang.clone();
            e.parent = parent.get(&i).and_then(|p| id_of.get(p).cloned());
            if e.parent.is_none() && parent.contains_key(&i) {
                continue; // parent dropped
            }
            let id = g.add(e);
            id_of.insert(i, id);
        }
        // ---- uses: idents ∩ owner's fields
        let fields_of: BTreeMap<usize, BTreeSet<String>> = order.iter().filter(|i| self.items[**i].e.kind == Some(EntityKind::Field)).filter_map(|i| parent.get(i).map(|p| (*p, self.items[*i].e.name.clone()))).fold(BTreeMap::new(), |mut m, (p, n)| {
            m.entry(p).or_default().insert(n);
            m
        });
        let field_type: BTreeMap<(usize, String), String> = order.iter().filter(|i| self.items[**i].e.kind == Some(EntityKind::Field)).filter_map(|i| Some(((*parent.get(i)?, self.items[*i].e.name.clone()), self.items[*i].e.ty.clone()?))).collect();
        for &i in &order {
            let Some(id) = id_of.get(&i) else { continue };
            if let (Some(p), true) = (parent.get(&i), self.items[i].e.kind.is_some_and(EntityKind::is_callable)) {
                if let Some(fs) = fields_of.get(p) {
                    let uses: Vec<String> = fs.iter().filter(|f| self.items[i].idents.contains(*f)).cloned().collect();
                    if let Some(e) = g.entities.iter_mut().find(|e| &e.id == id) {
                        e.uses = uses;
                    }
                }
            }
        }
        // ---- name tables for resolution
        let alive = |i: &usize| id_of.contains_key(i);
        let callables: Vec<usize> = order.iter().copied().filter(|i| alive(i) && self.items[*i].e.kind.is_some_and(EntityKind::is_callable)).collect();
        let mut by_name: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
        for &c in &callables {
            by_name.entry(self.items[c].e.name.as_str()).or_default().push(c);
        }
        let type_named = |n: &str| -> Option<usize> {
            let simple = n.rsplit(['.', ':', '/']).next().unwrap_or(n);
            let v: Vec<usize> = types_by_name.get(simple).map(|v| v.iter().copied().filter(|i| alive(i)).collect()).unwrap_or_default();
            (v.len() == 1).then(|| v[0])
        };
        let module_of = |mut i: usize| -> Option<usize> {
            loop {
                if matches!(self.items[i].e.kind, Some(EntityKind::Module | EntityKind::Namespace)) {
                    return Some(i);
                }
                i = *parent.get(&i)?;
            }
        };
        let mut edges: Vec<(usize, usize, EdgeKind)> = vec![];
        for &c in &callables {
            let it = &self.items[c];
            let owner = parent.get(&c).copied();
            let my_module = module_of(c);
            for call in &it.calls {
                let cands = by_name.get(call.name.as_str()).cloned().unwrap_or_default();
                let in_owner = |o: usize| cands.iter().copied().filter(|x| parent.get(x) == Some(&o)).collect::<Vec<_>>();
                let target: Option<usize> = match (&call.recv, &call.path) {
                    (Some(r), _) if matches!(r.as_str(), "self" | "this" | "Self" | "cls") => owner.and_then(|o| in_owner(o).first().copied()),
                    (Some(r), _) => {
                        // receiver typed by a field or a local/parameter
                        let ty = owner.and_then(|o| field_type.get(&(o, r.clone()))).or_else(|| it.locals.get(r));
                        let typed = ty.and_then(|t| t.split(|ch: char| !ch.is_alphanumeric() && ch != '_').filter(|s| !s.is_empty()).find_map(type_named));
                        match typed {
                            Some(t) => in_owner(t).first().copied(),
                            None => type_named(r).and_then(|t| in_owner(t).first().copied()).or_else(|| (cands.len() == 1).then(|| cands[0])),
                        }
                    }
                    (None, Some(path)) => {
                        let ns = it.aliases.get(path).cloned().unwrap_or_else(|| path.clone());
                        let last = ns.rsplit(['.', ':', '/']).next().unwrap_or(&ns).to_string();
                        cands
                            .iter()
                            .copied()
                            .find(|x| parent.get(x).is_some_and(|p| {
                                let pq = self.qualified_of(*p, &parent);
                                pq == ns.replace("::", ".").replace('/', ".") || pq.ends_with(&format!(".{last}")) || self.items[*p].e.name == last
                            }))
                            .or_else(|| (cands.len() == 1).then(|| cands[0]))
                    }
                    (None, None) => owner
                        .and_then(|o| in_owner(o).first().copied())
                        .or_else(|| cands.iter().copied().find(|x| module_of(*x) == my_module && my_module.is_some()))
                        .or_else(|| (cands.len() == 1).then(|| cands[0])),
                };
                match target {
                    Some(t) if t != c => edges.push((c, t, EdgeKind::Calls)),
                    Some(_) => {}
                    None => {
                        if !cands.is_empty() {
                            self.unresolved.push(format!("{}: {}{} is ambiguous", it.e.file, call.recv.as_ref().map(|r| format!("{r}.")).unwrap_or_default(), call.name));
                        }
                    }
                }
            }
            for t in &it.type_refs {
                if let Some(x) = type_named(t) {
                    if let Some(o) = owner {
                        edges.push((o, x, EdgeKind::References));
                    }
                }
            }
        }
        for &i in &order {
            let it = &self.items[i];
            for (name, kind) in &it.heritage {
                match type_named(name) {
                    Some(t) => {
                        // a supertype that is a contract is implemented, not extended
                        let k = if *kind == EdgeKind::Extends && self.items[t].e.kind.is_some_and(EntityKind::is_contract) && !self.items[i].e.kind.is_some_and(EntityKind::is_contract) { EdgeKind::Implements } else { *kind };
                        edges.push((i, t, k));
                    }
                    None => self.unresolved.push(format!("{}: supertype {name} of {} is outside the workspace", it.e.file, it.e.name)),
                }
            }
            if it.e.kind == Some(EntityKind::Field) {
                if let (Some(p), Some(ty)) = (parent.get(&i), &it.e.ty) {
                    for part in ty.split(|ch: char| !ch.is_alphanumeric() && ch != '_').filter(|s| !s.is_empty()) {
                        if let Some(x) = type_named(part) {
                            edges.push((*p, x, EdgeKind::References));
                        }
                    }
                }
            }
        }
        for (a, b, k) in edges {
            if let (Some(a), Some(b)) = (id_of.get(&a), id_of.get(&b)) {
                g.edge(a, b, k);
            }
        }
        g.documents = std::mem::take(&mut self.documents);
        g.unresolved = std::mem::take(&mut self.unresolved);
        g.limits = std::mem::take(&mut self.limits);
        g.finish();
        (g, self.skipped)
    }
}

impl Item {
    fn parent_item(&self) -> Option<usize> {
        match self.parent {
            ParentRef::Item(i) => Some(i),
            _ => None,
        }
    }
}

/// Node kinds that drive the generic body facts of one language.
pub struct FactSpec {
    pub branches: &'static [&'static str],
    pub loops: &'static [&'static str],
    pub blocks: &'static [&'static str],
    pub comments: &'static [&'static str],
    /// Nested definitions: not part of this body.
    pub nested: &'static [&'static str],
    pub statements: &'static [&'static str],
    pub creates: &'static [&'static str],
    pub switches: &'static [&'static str],
    /// node kind → idiom name
    pub idioms: &'static [(&'static str, &'static str)],
    /// called name → idiom name
    pub idiom_calls: &'static [(&'static str, &'static str)],
}

const K: usize = 5;
const W: usize = 4;

pub fn winnow(tokens: &[String]) -> Vec<u64> {
    if tokens.len() < K {
        return vec![];
    }
    let hashes: Vec<u64> = tokens.windows(K).map(|w| fnv64(w.join(" ").as_bytes())).collect();
    let mut out: BTreeSet<u64> = BTreeSet::new();
    if hashes.len() <= W {
        out.extend(hashes.iter().min());
    } else {
        for w in hashes.windows(W) {
            out.insert(*w.iter().min().unwrap());
        }
    }
    out.into_iter().collect()
}

struct FactWalk<'a> {
    spec: &'a FactSpec,
    src: &'a [u8],
    f: CodeFacts,
    rows: BTreeSet<usize>,
    tokens: Vec<String>,
    creates: BTreeSet<String>,
    chains: Vec<String>,
}

impl FactWalk<'_> {
    fn visit(&mut self, n: Node, depth: u32, top: bool) {
        let k = n.kind();
        if !top && self.spec.nested.contains(&k) {
            return;
        }
        if self.spec.comments.contains(&k) {
            return;
        }
        let mut depth = depth;
        if self.spec.blocks.contains(&k) {
            depth += 1;
            self.f.max_nesting = self.f.max_nesting.max(depth.saturating_sub(1));
        }
        if self.spec.branches.contains(&k) {
            self.f.branches += 1;
        }
        if k == "binary_expression" || k == "boolean_operator" {
            let op = n.child_by_field_name("operator").map(|o| text(o, self.src)).unwrap_or("");
            match op {
                "&&" | "||" | "and" | "or" => self.f.branches += 1,
                "+" | "-" | "*" | "/" | "%" | "//" => self.f.arithmetic += 1,
                "<" | ">" | "<=" | ">=" | "==" | "!=" | "===" | "!==" => self.f.comparisons += 1,
                _ => {}
            }
        }
        if self.spec.loops.contains(&k) {
            self.f.loops += 1;
        }
        if self.spec.switches.contains(&k) {
            self.f.switches += 1;
        }
        if self.spec.statements.contains(&k) {
            self.f.statements += 1;
        }
        if self.spec.creates.contains(&k) {
            if let Some(t) = n.child_by_field_name("type").or_else(|| n.child_by_field_name("constructor")).or_else(|| n.child_by_field_name("name")) {
                self.creates.insert(text(t, self.src).split('<').next().unwrap_or("").trim().to_string());
            }
        }
        for (kind, idiom) in self.spec.idioms {
            if *kind == k {
                *self.f.idioms.entry(idiom.to_string()).or_default() += 1;
            }
        }
        if n.child_count() == 0 {
            let t = text(n, self.src);
            for (i, line) in t.lines().enumerate() {
                if !line.trim().is_empty() && !matches!(t, "{" | "}" | "(" | ")") {
                    self.rows.insert(n.start_position().row + i);
                }
            }
            for (call, idiom) in self.spec.idiom_calls {
                if *call == t {
                    *self.f.idioms.entry(idiom.to_string()).or_default() += 1;
                }
            }
            let tok = if k.contains("identifier") || k == "sym_name" || k == "name" {
                "ID".to_string()
            } else if k.contains("literal") || k.contains("string") || k == "integer" || k == "float" || k == "number" || k == "num_lit" || k == "str_lit" || k == "kwd_name" {
                "LIT".to_string()
            } else {
                t.to_string()
            };
            self.tokens.push(tok);
        }
        // receiver chains (Law of Demeter): a.b().c().d()
        if matches!(k, "call_expression" | "call" | "method_invocation") && n.parent().is_none_or(|p| !matches!(p.kind(), "field_expression" | "member_expression" | "attribute")) {
            let t = text(n, self.src);
            let dots = t.split(['(', '[']).next().unwrap_or("").matches(['.']).count() as u32 + t.matches(").").count() as u32;
            if dots > self.f.max_chain {
                self.f.max_chain = dots;
            }
            if dots >= 3 && self.chains.len() < 8 {
                let s: String = t.split_whitespace().collect::<Vec<_>>().join(" ");
                self.chains.push(s.chars().take(90).collect());
            }
        }
        let mut c = n.walk();
        for ch in n.children(&mut c) {
            self.visit(ch, depth, false);
        }
    }
}

/// Generic facts for a callable: `decl` spans the declaration, `body` its body.
pub fn facts(spec: &FactSpec, decl: Node, body: Option<Node>, params: usize, file: &str, src: &[u8]) -> CodeFacts {
    let mut w = FactWalk { spec, src, f: CodeFacts::default(), rows: BTreeSet::new(), tokens: vec![], creates: BTreeSet::new(), chains: vec![] };
    w.f.file = file.to_string();
    w.f.start_line = decl.start_position().row as u32 + 1;
    w.f.end_line = decl.end_position().row as u32 + 1;
    w.f.params = params as u32;
    if let Some(b) = body {
        w.f.hash = format!("{:016x}", fnv64(text(b, src).as_bytes()));
        w.visit(b, 0, true);
    }
    let mut f = w.f;
    f.loc = w.rows.len() as u32;
    f.fingerprint = winnow(&w.tokens);
    f.creates = w.creates.into_iter().filter(|c| !c.is_empty()).collect();
    f.chains = w.chains;
    f
}

/// Identifiers in a subtree (for field-use cohesion), skipping nested definitions.
pub fn idents(n: Node, src: &[u8], nested: &[&str], out: &mut BTreeSet<String>) {
    if nested.contains(&n.kind()) {
        return;
    }
    let k = n.kind();
    if n.child_count() == 0 && (k.contains("identifier") || k == "sym_name") {
        out.insert(text(n, src).to_string());
    }
    let mut c = n.walk();
    for ch in n.children(&mut c) {
        idents(ch, src, nested, out);
    }
}

pub const SKIP_DIRS: [&str; 16] = ["target", "build", "out", "dist", "node_modules", ".git", ".gradle", ".idea", "bin", "obj", "__pycache__", ".venv", "venv", ".mypy_cache", ".cpcache", ".shadow-cljs"];

/// Source files of `lang` under `dir` (C++ also takes `.h` headers) (no symlinks; build output, VCS and test dirs skipped).
pub fn files_of(dir: &Path, lang: &str, include_tests: bool) -> Vec<PathBuf> {
    let mut out = vec![];
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            if e.file_type().map(|t| t.is_symlink()).unwrap_or(true) {
                continue;
            }
            if p.is_dir() {
                let testy = matches!(name.as_str(), "test" | "tests" | "__tests__" | "spec") && !include_tests;
                if !SKIP_DIRS.contains(&name.as_str()) && !name.starts_with('.') && !testy {
                    stack.push(p);
                }
            } else if (spide_core::symbols::language_of(&name) == Some(lang) || (lang == "cpp" && name.ends_with(".h"))) && !name.ends_with(".d.ts") && !name.ends_with(".edn") {
                let testy = !include_tests && (name.contains(".test.") || name.contains(".spec.") || name.starts_with("test_") || name.ends_with("_test.py") || name.ends_with("_test.clj"));
                if !testy {
                    out.push(p);
                }
            }
        }
    }
    out.sort();
    out
}

pub fn parser_for(lang: &str, file: &str) -> Option<Parser> {
    let l: tree_sitter::Language = match lang {
        "rust" => tree_sitter_rust::LANGUAGE.into(),
        "typescript" if file.ends_with(".tsx") => tree_sitter_typescript::LANGUAGE_TSX.into(),
        "typescript" => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        "python" => tree_sitter_python::LANGUAGE.into(),
        "c" => tree_sitter_c::LANGUAGE.into(),
        "cpp" => tree_sitter_cpp::LANGUAGE.into(),
        "clojure" => tree_sitter_clojure_orchard::LANGUAGE.into(),
        _ => return None,
    };
    let mut p = Parser::new();
    p.set_language(&l).ok()?;
    Some(p)
}

/// Parse and index `files` (relative to `root`) of one language into a graph.
pub fn index_files(root: &Path, lang: &str, files: &[PathBuf]) -> (SemanticGraph, Vec<String>) {
    let mut b = Builder { lang: lang.to_string(), ..Default::default() };
    if let Some((_, _, limits)) = CAPABILITIES.iter().find(|(l, _, _)| *l == lang) {
        b.limits = limits.iter().map(|s| format!("{lang}: {s}")).collect();
    }
    let crate_name = if lang == "rust" { rust_crate_name(root) } else { None };
    for f in files {
        let rel = f.strip_prefix(root).unwrap_or(f).to_string_lossy().replace('\\', "/");
        let Ok(src) = std::fs::read(f) else {
            b.skipped.push(format!("{rel}: unreadable"));
            continue;
        };
        let Some(mut parser) = parser_for(lang, &rel) else { continue };
        let Some(tree) = parser.parse(&src, None) else {
            b.skipped.push(format!("{rel}: parse failed"));
            continue;
        };
        let clean = !tree.root_node().has_error();
        if !clean {
            b.skipped.push(format!("{rel}: syntax errors (partially indexed)"));
        }
        b.documents.insert(rel.clone(), spide_core::symbols::Document { language: lang.into(), hash: format!("{:016x}", fnv64(&src)), version: None, clean, provider: "parser".into() });
        match lang {
            "clojure" => clojure::index(&mut b, tree.root_node(), &src, &rel),
            _ => ts_family::index(&mut b, lang, tree.root_node(), &src, &rel, crate_name.as_deref()),
        }
    }
    b.finish()
}

fn rust_crate_name(root: &Path) -> Option<String> {
    let t = std::fs::read_to_string(root.join("Cargo.toml")).ok()?;
    let mut in_pkg = false;
    for l in t.lines() {
        let l = l.trim();
        if l.starts_with('[') {
            in_pkg = l == "[package]";
        } else if in_pkg {
            if let Some(v) = l.strip_prefix("name").map(str::trim_start).and_then(|r| r.strip_prefix('=')) {
                return Some(v.trim().trim_matches('"').replace('-', "_"));
            }
        }
    }
    None
}

/// Module path of a file: `src/a/b.rs` → `crate.a.b`, `shop/orders.py` → `shop.orders`.
pub fn module_path(lang: &str, rel: &str, crate_name: Option<&str>) -> String {
    let no_ext = rel.rsplit_once('.').map(|(a, _)| a).unwrap_or(rel);
    let mut parts: Vec<&str> = no_ext.split('/').filter(|p| !p.is_empty()).collect();
    match lang {
        "rust" => {
            if let Some(i) = parts.iter().position(|p| *p == "src") {
                parts.drain(..=i);
            }
            if matches!(parts.last(), Some(&"mod" | &"lib" | &"main")) {
                parts.pop();
            }
            let mut q = vec![crate_name.unwrap_or("crate")];
            q.extend(parts);
            q.join(".")
        }
        "python" => {
            if parts.last() == Some(&"__init__") {
                parts.pop();
            }
            if parts.first() == Some(&"src") && parts.len() > 1 {
                parts.remove(0);
            }
            parts.join(".")
        }
        "typescript" => {
            if parts.last() == Some(&"index") && parts.len() > 1 {
                parts.pop();
            }
            parts.join(".").replace('-', "_")
        }
        _ => parts.join("."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_paths() {
        assert_eq!(module_path("rust", "src/pricing/mod.rs", Some("shop")), "shop.pricing");
        assert_eq!(module_path("rust", "src/lib.rs", Some("shop")), "shop");
        assert_eq!(module_path("python", "src/shop/__init__.py", None), "shop");
        assert_eq!(module_path("typescript", "src/ui/index.ts", None), "src.ui");
        assert_eq!(module_path("c", "src/orders.h", None), "src.orders");
    }
}

/// Languages present under `root`, most files first (build output and dependencies skipped).
pub fn detect(root: &Path) -> Vec<String> {
    let mut counts: Vec<(usize, String)> = spide_core::symbols::LANGUAGES
        .iter()
        .map(|l| (if *l == "java" { crate::scoped_files(root, &[".".into()], &[]).map(|f| f.len()).unwrap_or(0) } else { files_of(root, l, false).iter().filter(|f| *l != "cpp" || f.extension().is_none_or(|e| e != "h")).count() }, l.to_string()))
        .filter(|(n, _)| *n > 0)
        .collect();
    counts.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    counts.into_iter().map(|(_, l)| l).collect()
}
