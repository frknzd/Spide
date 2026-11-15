//! Tree-sitter adapters for Rust, TypeScript, Python, C and C++.

use super::{facts, idents, module_path, text, Builder, Call, FactSpec, Item, ParentRef};
use spide_core::model::{Param, Visibility};
use spide_core::symbols::{EdgeKind, EntityKind};
use tree_sitter::Node;

const RUST: FactSpec = FactSpec {
    branches: &["if_expression", "match_arm", "while_expression", "for_expression", "loop_expression", "try_expression"],
    loops: &["for_expression", "while_expression", "loop_expression"],
    blocks: &["block"],
    comments: &["line_comment", "block_comment"],
    nested: &["function_item", "impl_item", "mod_item", "struct_item", "trait_item", "closure_expression"],
    statements: &["expression_statement", "let_declaration", "return_expression"],
    creates: &["struct_expression"],
    switches: &["match_expression"],
    idioms: &[("unsafe_block", "unsafe")],
    idiom_calls: &[("unwrap", "unwrap"), ("expect", "unwrap"), ("panic", "panic"), ("todo", "panic"), ("unimplemented", "panic"), ("clone", "clone")],
};

const TS: FactSpec = FactSpec {
    branches: &["if_statement", "for_statement", "for_in_statement", "while_statement", "do_statement", "switch_case", "catch_clause", "ternary_expression"],
    loops: &["for_statement", "for_in_statement", "while_statement", "do_statement"],
    blocks: &["statement_block"],
    comments: &["comment"],
    nested: &["class_declaration", "function_declaration", "method_definition"],
    statements: &["expression_statement", "lexical_declaration", "variable_declaration", "return_statement", "if_statement", "for_statement", "for_in_statement", "while_statement", "throw_statement"],
    creates: &["new_expression"],
    switches: &["switch_statement"],
    idioms: &[("non_null_expression", "non_null"), ("as_expression", "cast")],
    idiom_calls: &[("any", "any"), ("eval", "eval")],
};

const PY: FactSpec = FactSpec {
    branches: &["if_statement", "elif_clause", "for_statement", "while_statement", "except_clause", "conditional_expression", "case_clause", "boolean_operator", "for_in_clause", "if_clause"],
    loops: &["for_statement", "while_statement", "list_comprehension", "dictionary_comprehension", "set_comprehension", "generator_expression"],
    blocks: &["block"],
    comments: &["comment"],
    nested: &["function_definition", "class_definition"],
    statements: &["expression_statement", "return_statement", "if_statement", "for_statement", "while_statement", "raise_statement", "with_statement", "try_statement", "assert_statement"],
    creates: &[],
    switches: &["match_statement"],
    idioms: &[("global_statement", "global")],
    idiom_calls: &[("eval", "eval"), ("exec", "eval"), ("Any", "any")],
};

const C: FactSpec = FactSpec {
    branches: &["if_statement", "for_statement", "while_statement", "do_statement", "case_statement", "conditional_expression"],
    loops: &["for_statement", "while_statement", "do_statement"],
    blocks: &["compound_statement"],
    comments: &["comment"],
    nested: &["function_definition"],
    statements: &["expression_statement", "declaration", "return_statement", "if_statement", "for_statement", "while_statement", "do_statement", "goto_statement", "break_statement", "continue_statement"],
    creates: &[],
    switches: &["switch_statement"],
    idioms: &[("goto_statement", "goto"), ("preproc_if", "preprocessor"), ("preproc_ifdef", "preprocessor")],
    idiom_calls: &[("malloc", "manual_memory"), ("calloc", "manual_memory"), ("realloc", "manual_memory"), ("free", "manual_memory")],
};

const CPP: FactSpec = FactSpec {
    branches: &["if_statement", "for_statement", "for_range_loop", "while_statement", "do_statement", "case_statement", "conditional_expression", "catch_clause"],
    loops: &["for_statement", "for_range_loop", "while_statement", "do_statement"],
    blocks: &["compound_statement"],
    comments: &["comment"],
    nested: &["function_definition", "class_specifier", "lambda_expression"],
    statements: &["expression_statement", "declaration", "return_statement", "if_statement", "for_statement", "for_range_loop", "while_statement", "do_statement", "goto_statement", "throw_statement"],
    creates: &["new_expression"],
    switches: &["switch_statement"],
    idioms: &[("new_expression", "raw_new_delete"), ("delete_expression", "raw_new_delete"), ("goto_statement", "goto")],
    idiom_calls: &[("reinterpret_cast", "unsafe_cast"), ("const_cast", "unsafe_cast"), ("malloc", "manual_memory"), ("free", "manual_memory")],
};

pub fn spec(lang: &str) -> &'static FactSpec {
    match lang {
        "rust" => &RUST,
        "typescript" => &TS,
        "python" => &PY,
        "c" => &C,
        _ => &CPP,
    }
}

fn at(it: &mut Item, n: Node, rel: &str) {
    it.e.file = rel.to_string();
    it.e.start_line = n.start_position().row as u32 + 1;
    it.e.end_line = n.end_position().row as u32 + 1;
    it.e.start_col = n.start_position().column as u32;
}

fn field<'a>(n: Node<'a>, f: &str) -> Option<Node<'a>> {
    n.child_by_field_name(f)
}

fn children<'a>(n: Node<'a>) -> Vec<Node<'a>> {
    let mut c = n.walk();
    n.children(&mut c).collect()
}

/// Simple type name of a (possibly generic/qualified/reference) type node.
fn simple_type(t: &str) -> String {
    let t = t.trim().trim_start_matches(':').trim().trim_start_matches('&').trim_start_matches("mut ").trim_start_matches("dyn ").trim_start_matches("impl ");
    let base = t.split('<').next().unwrap_or(t);
    base.rsplit(['.', ':']).next().unwrap_or(base).trim().to_string()
}

struct Ctx<'a> {
    lang: &'a str,
    src: &'a [u8],
    rel: &'a str,
    crate_name: Option<&'a str>,
}

pub fn index(b: &mut Builder, lang: &str, root: Node, src: &[u8], rel: &str, crate_name: Option<&str>) {
    let cx = Ctx { lang, src, rel, crate_name };
    let q = module_path(lang, rel, crate_name);
    let module = b.module(&q, EntityKind::Module, rel, 1);
    match lang {
        "rust" => rust_items(b, &cx, root, module, &q),
        "typescript" => ts_items(b, &cx, root, module),
        "python" => py_items(b, &cx, root, module),
        "c" => c_items(b, &cx, root, module, None),
        _ => cpp_items(b, &cx, root, module, "", Visibility::Public),
    }
}

/// A callable item with params, body facts, calls and identifiers.
#[allow(clippy::too_many_arguments)]
fn callable(b: &mut Builder, cx: &Ctx, n: Node, name: &str, kind: EntityKind, parent: ParentRef, params: Vec<Param>, returns: Option<String>, body: Option<Node>, vis: Visibility) -> usize {
    let mut it = Item::new(kind, name, parent);
    at(&mut it, n, cx.rel);
    it.e.params = params.clone();
    it.e.returns = returns.filter(|r| !r.is_empty() && r != "void" && r != "None" && r != "()");
    it.e.visibility = vis;
    it.has_body = body.is_some();
    it.e.is_abstract = body.is_none();
    for p in &params {
        it.locals.insert(p.name.clone(), p.ty.clone());
    }
    let sp = spec(cx.lang);
    if let Some(bd) = body {
        it.e.facts = Some(facts(sp, n, Some(bd), params.len(), cx.rel, cx.src));
        idents(bd, cx.src, sp.nested, &mut it.idents);
        collect_calls(cx, bd, sp.nested, &mut it);
    }
    b.push(it)
}

fn collect_calls(cx: &Ctx, n: Node, nested: &[&str], it: &mut Item) {
    if nested.contains(&n.kind()) {
        return;
    }
    let src = cx.src;
    match (cx.lang, n.kind()) {
        ("rust", "call_expression") | ("typescript", "call_expression") | ("c", "call_expression") | ("cpp", "call_expression") | ("python", "call") => {
            if let Some(f) = field(n, "function") {
                let call = match f.kind() {
                    "identifier" => Some(Call { recv: None, path: None, name: text(f, src).into() }),
                    "field_expression" if cx.lang == "rust" => field(f, "field").map(|x| Call { recv: field(f, "value").map(|v| recv_name(v, src)), path: None, name: text(x, src).into() }),
                    "field_expression" => field(f, "field").map(|x| Call { recv: field(f, "argument").map(|v| recv_name(v, src)), path: None, name: text(x, src).into() }),
                    "member_expression" => field(f, "property").map(|x| Call { recv: field(f, "object").map(|v| recv_name(v, src)), path: None, name: text(x, src).into() }),
                    "attribute" => field(f, "attribute").map(|x| Call { recv: field(f, "object").map(|v| recv_name(v, src)), path: None, name: text(x, src).into() }),
                    "scoped_identifier" | "qualified_identifier" => {
                        let name = field(f, "name").map(|x| text(x, src).to_string()).unwrap_or_default();
                        let path = field(f, "path").or_else(|| field(f, "scope")).map(|x| text(x, src).to_string());
                        Some(Call { recv: None, path, name })
                    }
                    "template_function" => field(f, "name").map(|x| Call { recv: None, path: None, name: text(x, src).into() }),
                    _ => None,
                };
                if let Some(c) = call.filter(|c| !c.name.is_empty()) {
                    it.calls.push(c);
                }
            }
        }
        ("rust", "let_declaration") => {
            if let (Some(p), Some(t)) = (field(n, "pattern"), field(n, "type")) {
                it.locals.insert(text(p, src).trim_start_matches("mut ").into(), text(t, src).into());
            }
        }
        ("typescript", "variable_declarator") => {
            if let (Some(p), Some(t)) = (field(n, "name"), field(n, "type")) {
                it.locals.insert(text(p, src).into(), text(t, src).trim_start_matches(':').trim().into());
            }
        }
        ("python", "assignment") => {
            if let (Some(l), Some(t)) = (field(n, "left"), field(n, "type")) {
                it.locals.insert(text(l, src).into(), text(t, src).into());
            }
        }
        ("c" | "cpp", "declaration") => {
            if let Some(t) = field(n, "type") {
                for d in children(n).into_iter().filter(|c| c.kind().contains("declarator") || c.kind() == "identifier") {
                    if let Some(id) = innermost_ident(d, src) {
                        it.locals.insert(id, text(t, src).into());
                    }
                }
            }
        }
        _ => {}
    }
    for ch in children(n) {
        collect_calls(cx, ch, nested, it);
    }
}

fn recv_name(v: Node, src: &[u8]) -> String {
    match v.kind() {
        "self" | "this" => text(v, src).to_string(),
        "identifier" | "field_identifier" | "property_identifier" => text(v, src).to_string(),
        // this.x.y() / self.x.y(): the field is the receiver
        "member_expression" | "field_expression" | "attribute" => {
            let obj = field(v, "object").or_else(|| field(v, "value")).or_else(|| field(v, "argument"));
            let prop = field(v, "property").or_else(|| field(v, "field")).or_else(|| field(v, "attribute"));
            match (obj.map(|o| text(o, src)), prop) {
                (Some("this" | "self"), Some(p)) => text(p, src).to_string(),
                _ => "?".into(),
            }
        }
        _ => "?".into(),
    }
}

fn innermost_ident(n: Node, src: &[u8]) -> Option<String> {
    if matches!(n.kind(), "identifier" | "field_identifier" | "type_identifier") {
        return Some(text(n, src).to_string());
    }
    if let Some(d) = field(n, "declarator") {
        return innermost_ident(d, src);
    }
    children(n).into_iter().find_map(|c| if c.kind().contains("declarator") || c.kind() == "identifier" { innermost_ident(c, src) } else { None })
}

fn add_field(b: &mut Builder, cx: &Ctx, n: Node, name: &str, ty: Option<String>, parent: usize, vis: Visibility, is_static: bool) {
    let mut f = Item::new(EntityKind::Field, name, ParentRef::Item(parent));
    at(&mut f, n, cx.rel);
    f.e.ty = ty.map(|t| t.trim().trim_start_matches(':').trim().to_string()).filter(|t| !t.is_empty());
    f.e.visibility = vis;
    f.e.is_static = is_static;
    b.push(f);
}

fn type_item(b: &mut Builder, cx: &Ctx, n: Node, name: &str, kind: EntityKind, parent: usize) -> usize {
    let mut it = Item::new(kind, name, ParentRef::Item(parent));
    at(&mut it, n, cx.rel);
    it.e.visibility = Visibility::Public;
    b.push(it)
}

// ------------------------------------------------------------------ Rust

fn rust_vis(n: Node) -> Visibility {
    if children(n).iter().any(|c| c.kind() == "visibility_modifier") {
        Visibility::Public
    } else {
        Visibility::Private
    }
}

fn rust_params(n: Node, src: &[u8]) -> (Vec<Param>, bool) {
    let mut out = vec![];
    let mut has_self = false;
    if let Some(ps) = field(n, "parameters") {
        for p in children(ps) {
            match p.kind() {
                "self_parameter" => has_self = true,
                "parameter" => out.push(Param { name: field(p, "pattern").map(|x| text(x, src).trim_start_matches("mut ").to_string()).unwrap_or_default(), ty: field(p, "type").map(|x| text(x, src).to_string()).unwrap_or_default() }),
                _ => {}
            }
        }
    }
    (out, has_self)
}

fn rust_items(b: &mut Builder, cx: &Ctx, n: Node, module: usize, mq: &str) {
    let src = cx.src;
    for ch in children(n) {
        match ch.kind() {
            "mod_item" => {
                if let (Some(name), Some(body)) = (field(ch, "name"), field(ch, "body")) {
                    let q = format!("{mq}.{}", text(name, src));
                    let m = b.module(&q, EntityKind::Module, cx.rel, ch.start_position().row as u32 + 1);
                    rust_items(b, cx, body, m, &q);
                }
            }
            "struct_item" | "enum_item" | "union_item" | "trait_item" => {
                let Some(name) = field(ch, "name").map(|x| text(x, src).to_string()) else { continue };
                let kind = match ch.kind() {
                    "struct_item" => EntityKind::Struct,
                    "enum_item" => EntityKind::Enum,
                    "union_item" => EntityKind::Union,
                    _ => EntityKind::Trait,
                };
                let t = type_item(b, cx, ch, &name, kind, module);
                b.items[t].e.visibility = rust_vis(ch);
                if let Some(body) = field(ch, "body") {
                    for m in children(body) {
                        match m.kind() {
                            "field_declaration" if kind != EntityKind::Enum => {
                                if let Some(fname) = field(m, "name") {
                                    add_field(b, cx, m, text(fname, src), field(m, "type").map(|x| text(x, src).to_string()), t, rust_vis(m), false);
                                }
                            }
                            "function_signature_item" | "function_item" if kind == EntityKind::Trait => {
                                let Some(mname) = field(m, "name").map(|x| text(x, src).to_string()) else { continue };
                                let (params, has_self) = rust_params(m, src);
                                let i = callable(b, cx, m, &mname, EntityKind::Method, ParentRef::Item(t), params, field(m, "return_type").map(|x| text(x, src).to_string()), field(m, "body"), Visibility::Public);
                                b.items[i].e.is_static = !has_self;
                            }
                            _ => {}
                        }
                    }
                }
                if kind == EntityKind::Trait {
                    if let Some(bounds) = field(ch, "bounds") {
                        for s in children(bounds).into_iter().filter(|x| x.kind().contains("type")) {
                            b.items[t].heritage.push((simple_type(text(s, src)), EdgeKind::Extends));
                        }
                    }
                }
            }
            "impl_item" => {
                let Some(ty) = field(ch, "type").map(|x| simple_type(text(x, src))) else { continue };
                if let Some(tr) = field(ch, "trait") {
                    b.pending_impls.push((Some(module), ty.clone(), simple_type(text(tr, src))));
                }
                let trait_impl = field(ch, "trait").is_some();
                if let Some(body) = field(ch, "body") {
                    for m in children(body).into_iter().filter(|m| m.kind() == "function_item") {
                        let Some(mname) = field(m, "name").map(|x| text(x, src).to_string()) else { continue };
                        let (params, has_self) = rust_params(m, src);
                        let vis = if trait_impl { Visibility::Public } else { rust_vis(m) };
                        let i = callable(b, cx, m, &mname, EntityKind::Method, ParentRef::TypeName { module: Some(module), name: ty.clone() }, params, field(m, "return_type").map(|x| text(x, src).to_string()), field(m, "body"), vis);
                        b.items[i].e.is_static = !has_self;
                    }
                }
            }
            "function_item" => {
                let Some(name) = field(ch, "name").map(|x| text(x, src).to_string()) else { continue };
                let (params, _) = rust_params(ch, src);
                callable(b, cx, ch, &name, EntityKind::Function, ParentRef::Item(module), params, field(ch, "return_type").map(|x| text(x, src).to_string()), field(ch, "body"), rust_vis(ch));
            }
            "macro_definition" => {
                if let Some(name) = field(ch, "name").map(|x| text(x, src).to_string()) {
                    let mut it = Item::new(EntityKind::Macro, &name, ParentRef::Item(module));
                    at(&mut it, ch, cx.rel);
                    it.e.generated = true;
                    b.push(it);
                }
            }
            _ => {}
        }
    }
    let _ = cx.crate_name;
}

// ------------------------------------------------------------------ TypeScript

fn ts_params(n: Node, src: &[u8]) -> Vec<Param> {
    let Some(ps) = field(n, "parameters") else { return vec![] };
    children(ps)
        .into_iter()
        .filter(|p| matches!(p.kind(), "required_parameter" | "optional_parameter"))
        .map(|p| Param { name: field(p, "pattern").map(|x| text(x, src).to_string()).unwrap_or_default(), ty: field(p, "type").map(|x| text(x, src).trim_start_matches(':').trim().to_string()).unwrap_or_default() })
        .collect()
}

fn ts_vis(n: Node, src: &[u8], name: &str) -> Visibility {
    if name.starts_with('#') {
        return Visibility::Private;
    }
    match children(n).iter().find(|c| c.kind() == "accessibility_modifier").map(|c| text(*c, src)) {
        Some("private") => Visibility::Private,
        Some("protected") => Visibility::Protected,
        _ => Visibility::Public,
    }
}

fn ts_returns(n: Node, src: &[u8]) -> Option<String> {
    field(n, "return_type").map(|x| text(x, src).trim_start_matches(':').trim().to_string())
}

fn ts_items(b: &mut Builder, cx: &Ctx, n: Node, module: usize) {
    let src = cx.src;
    for ch in children(n) {
        match ch.kind() {
            "export_statement" => ts_items(b, cx, ch, module),
            "class_declaration" | "abstract_class_declaration" => {
                let Some(name) = field(ch, "name").map(|x| text(x, src).to_string()) else { continue };
                let t = type_item(b, cx, ch, &name, EntityKind::Class, module);
                b.items[t].e.is_abstract = ch.kind() == "abstract_class_declaration";
                for h in children(ch).into_iter().filter(|c| c.kind() == "class_heritage") {
                    for clause in children(h) {
                        let kind = if clause.kind() == "implements_clause" { EdgeKind::Implements } else { EdgeKind::Extends };
                        for ty in children(clause).into_iter().filter(|x| x.is_named()) {
                            b.items[t].heritage.push((simple_type(text(ty, src)), kind));
                        }
                    }
                }
                if let Some(body) = field(ch, "body") {
                    for m in children(body) {
                        match m.kind() {
                            "method_definition" | "method_signature" | "abstract_method_signature" => {
                                let Some(mname) = field(m, "name").map(|x| text(x, src).to_string()) else { continue };
                                let params = ts_params(m, src);
                                if mname == "constructor" {
                                    // parameter properties are fields
                                    if let Some(ps) = field(m, "parameters") {
                                        for p in children(ps).into_iter().filter(|p| children(*p).iter().any(|c| c.kind() == "accessibility_modifier")) {
                                            let pn = field(p, "pattern").map(|x| text(x, src).to_string()).unwrap_or_default();
                                            add_field(b, cx, p, &pn, field(p, "type").map(|x| text(x, src).to_string()), t, ts_vis(p, src, &pn), false);
                                        }
                                    }
                                }
                                let vis = ts_vis(m, src, &mname);
                                let i = callable(b, cx, m, &mname, EntityKind::Method, ParentRef::Item(t), params, ts_returns(m, src), field(m, "body"), vis);
                                b.items[i].e.is_static = children(m).iter().any(|c| c.kind() == "static");
                            }
                            "public_field_definition" => {
                                let Some(fname) = field(m, "name").map(|x| text(x, src).to_string()) else { continue };
                                let vis = ts_vis(m, src, &fname);
                                // arrow-function fields are methods
                                if let Some(v) = field(m, "value").filter(|v| v.kind() == "arrow_function") {
                                    callable(b, cx, m, &fname, EntityKind::Method, ParentRef::Item(t), ts_params(v, src), ts_returns(v, src), field(v, "body"), vis);
                                } else {
                                    let st = children(m).iter().any(|c| c.kind() == "static");
                                    add_field(b, cx, m, &fname, field(m, "type").map(|x| text(x, src).to_string()), t, vis, st);
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            "interface_declaration" => {
                let Some(name) = field(ch, "name").map(|x| text(x, src).to_string()) else { continue };
                let t = type_item(b, cx, ch, &name, EntityKind::Interface, module);
                for h in children(ch).into_iter().filter(|c| c.kind() == "extends_type_clause") {
                    for ty in children(h).into_iter().filter(|x| x.is_named()) {
                        b.items[t].heritage.push((simple_type(text(ty, src)), EdgeKind::Extends));
                    }
                }
                if let Some(body) = field(ch, "body") {
                    for m in children(body) {
                        match m.kind() {
                            "method_signature" => {
                                if let Some(mname) = field(m, "name").map(|x| text(x, src).to_string()) {
                                    callable(b, cx, m, &mname, EntityKind::Method, ParentRef::Item(t), ts_params(m, src), ts_returns(m, src), None, Visibility::Public);
                                }
                            }
                            "property_signature" => {
                                if let Some(fname) = field(m, "name").map(|x| text(x, src).to_string()) {
                                    add_field(b, cx, m, &fname, field(m, "type").map(|x| text(x, src).to_string()), t, Visibility::Public, false);
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            "enum_declaration" => {
                if let Some(name) = field(ch, "name").map(|x| text(x, src).to_string()) {
                    type_item(b, cx, ch, &name, EntityKind::Enum, module);
                }
            }
            "type_alias_declaration" => {
                if let Some(name) = field(ch, "name").map(|x| text(x, src).to_string()) {
                    let mut it = Item::new(EntityKind::Alias, &name, ParentRef::Item(module));
                    at(&mut it, ch, cx.rel);
                    b.push(it);
                }
            }
            "function_declaration" | "generator_function_declaration" => {
                if let Some(name) = field(ch, "name").map(|x| text(x, src).to_string()) {
                    callable(b, cx, ch, &name, EntityKind::Function, ParentRef::Item(module), ts_params(ch, src), ts_returns(ch, src), field(ch, "body"), Visibility::Public);
                }
            }
            "lexical_declaration" | "variable_declaration" => {
                for d in children(ch).into_iter().filter(|d| d.kind() == "variable_declarator") {
                    if let (Some(name), Some(v)) = (field(d, "name"), field(d, "value")) {
                        if matches!(v.kind(), "arrow_function" | "function_expression" | "function") {
                            callable(b, cx, d, text(name, src), EntityKind::Function, ParentRef::Item(module), ts_params(v, src), ts_returns(v, src), field(v, "body"), Visibility::Public);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

// ------------------------------------------------------------------ Python

fn py_params(n: Node, src: &[u8]) -> (Vec<Param>, bool) {
    let mut out = vec![];
    let mut has_self = false;
    let Some(ps) = field(n, "parameters") else { return (out, false) };
    for (i, p) in children(ps).into_iter().filter(|p| p.is_named()).enumerate() {
        let (name, ty) = match p.kind() {
            "identifier" => (text(p, src).to_string(), String::new()),
            "typed_parameter" | "typed_default_parameter" => (children(p).first().map(|x| text(*x, src).to_string()).or_else(|| field(p, "name").map(|x| text(x, src).to_string())).unwrap_or_default(), field(p, "type").map(|x| text(x, src).to_string()).unwrap_or_default()),
            "default_parameter" => (field(p, "name").map(|x| text(x, src).to_string()).unwrap_or_default(), String::new()),
            _ => continue,
        };
        if i == 0 && (name == "self" || name == "cls") {
            has_self = true;
            continue;
        }
        out.push(Param { name, ty });
    }
    (out, has_self)
}

fn py_vis(name: &str) -> Visibility {
    if name.starts_with("__") && name.ends_with("__") {
        Visibility::Public
    } else if name.starts_with('_') {
        Visibility::Private
    } else {
        Visibility::Public
    }
}

/// Unwrap `decorated_definition`, returning the definition and decorator names.
fn py_def<'a>(n: Node<'a>, src: &[u8]) -> (Node<'a>, Vec<String>) {
    if n.kind() == "decorated_definition" {
        let decos: Vec<String> = children(n).into_iter().filter(|c| c.kind() == "decorator").map(|d| text(d, src).trim_start_matches('@').split('(').next().unwrap_or("").rsplit('.').next().unwrap_or("").to_string()).collect();
        if let Some(d) = field(n, "definition") {
            return (d, decos);
        }
    }
    (n, vec![])
}

fn py_items(b: &mut Builder, cx: &Ctx, n: Node, module: usize) {
    let src = cx.src;
    for ch in children(n) {
        let (def, decos) = py_def(ch, src);
        match def.kind() {
            "class_definition" => {
                let Some(name) = field(def, "name").map(|x| text(x, src).to_string()) else { continue };
                let bases: Vec<String> = field(def, "superclasses").map(|s| children(s).into_iter().filter(|x| matches!(x.kind(), "identifier" | "attribute" | "subscript")).map(|x| simple_type(text(x, src).split('[').next().unwrap_or(""))).collect()).unwrap_or_default();
                let kind = if bases.iter().any(|b| b == "Protocol") { EntityKind::Protocol } else if decos.iter().any(|d| d == "dataclass") { EntityKind::Record } else if bases.iter().any(|b| b == "Enum" || b == "IntEnum" || b == "StrEnum") { EntityKind::Enum } else { EntityKind::Class };
                let t = type_item(b, cx, def, &name, kind, module);
                b.items[t].e.visibility = py_vis(&name);
                b.items[t].e.is_abstract = bases.iter().any(|b| b == "ABC");
                for base in bases.iter().filter(|b| !matches!(b.as_str(), "Protocol" | "ABC" | "object" | "Generic" | "Enum" | "IntEnum" | "StrEnum")) {
                    b.items[t].heritage.push((base.clone(), EdgeKind::Extends));
                }
                let Some(body) = field(def, "body") else { continue };
                for m in children(body) {
                    let (md, mdecos) = py_def(m, src);
                    match md.kind() {
                        "function_definition" => {
                            let Some(mname) = field(md, "name").map(|x| text(x, src).to_string()) else { continue };
                            let (params, has_self) = py_params(md, src);
                            let mbody = field(md, "body");
                            let stub = mbody.is_some_and(|bd| text(bd, src).trim() == "..." || text(bd, src).trim() == "pass");
                            let abstract_ = mdecos.iter().any(|d| d == "abstractmethod") || (kind == EntityKind::Protocol && stub);
                            let i = callable(b, cx, md, &mname, EntityKind::Method, ParentRef::Item(t), params, field(md, "return_type").map(|x| text(x, src).to_string()), if abstract_ { None } else { mbody }, py_vis(&mname));
                            b.items[i].e.is_static = !has_self || mdecos.iter().any(|d| d == "staticmethod");
                            // self.x = … in any method declares a field
                            if let Some(bd) = mbody {
                                py_self_fields(b, cx, bd, t);
                            }
                        }
                        "expression_statement" => {
                            for a in children(md).into_iter().filter(|a| a.kind() == "assignment") {
                                if let Some(l) = field(a, "left").filter(|l| l.kind() == "identifier") {
                                    let n = text(l, src).to_string();
                                    add_field(b, cx, a, &n, field(a, "type").map(|x| text(x, src).to_string()), t, py_vis(&n), kind != EntityKind::Record && field(a, "type").is_none());
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            "function_definition" => {
                let Some(name) = field(def, "name").map(|x| text(x, src).to_string()) else { continue };
                let (params, _) = py_params(def, src);
                callable(b, cx, def, &name, EntityKind::Function, ParentRef::Item(module), params, field(def, "return_type").map(|x| text(x, src).to_string()), field(def, "body"), py_vis(&name));
            }
            _ => {}
        }
    }
}

fn py_self_fields(b: &mut Builder, cx: &Ctx, n: Node, t: usize) {
    if matches!(n.kind(), "function_definition" | "class_definition") {
        return;
    }
    if n.kind() == "assignment" {
        if let Some(l) = field(n, "left").filter(|l| l.kind() == "attribute") {
            if field(l, "object").is_some_and(|o| text(o, cx.src) == "self") {
                if let Some(a) = field(l, "attribute") {
                    let name = text(a, cx.src).to_string();
                    let exists = b.items.iter().any(|it| matches!(it.parent, ParentRef::Item(p) if p == t) && it.e.kind == Some(EntityKind::Field) && it.e.name == name);
                    if !exists {
                        add_field(b, cx, n, &name, field(n, "type").map(|x| text(x, cx.src).to_string()), t, py_vis(&name), false);
                    }
                }
            }
        }
    }
    for ch in children(n) {
        py_self_fields(b, cx, ch, t);
    }
}

// ------------------------------------------------------------------ C

fn c_params(decl: Node, src: &[u8]) -> Vec<Param> {
    let Some(ps) = field(decl, "parameters") else { return vec![] };
    children(ps)
        .into_iter()
        .filter(|p| p.kind() == "parameter_declaration")
        .map(|p| {
            let ty = field(p, "type").map(|x| text(x, src).to_string()).unwrap_or_default();
            let ptr = field(p, "declarator").is_some_and(|d| text(d, src).starts_with('*') || text(d, src).starts_with('&'));
            Param { name: field(p, "declarator").and_then(|d| innermost_ident(d, src)).unwrap_or_default(), ty: if ptr { format!("{ty}*") } else { ty } }
        })
        .filter(|p| p.ty != "void")
        .collect()
}

/// The function_declarator inside a (pointer) declarator.
fn fn_declarator(n: Node<'_>) -> Option<Node<'_>> {
    if n.kind() == "function_declarator" {
        return Some(n);
    }
    field(n, "declarator").and_then(fn_declarator)
}

fn record_kind(k: &str) -> Option<EntityKind> {
    match k {
        "struct_specifier" => Some(EntityKind::Struct),
        "union_specifier" => Some(EntityKind::Union),
        "enum_specifier" => Some(EntityKind::Enum),
        "class_specifier" => Some(EntityKind::Class),
        _ => None,
    }
}

fn c_record(b: &mut Builder, cx: &Ctx, n: Node, name: &str, module: usize) {
    let kind = record_kind(n.kind()).unwrap_or(EntityKind::Struct);
    let exists = b.items.iter().any(|it| matches!(it.parent, ParentRef::Item(p) if p == module) && it.e.name == name && it.e.kind == Some(kind));
    if exists {
        return;
    }
    let t = type_item(b, cx, n, name, kind, module);
    if let Some(body) = field(n, "body") {
        for f in children(body).into_iter().filter(|f| f.kind() == "field_declaration") {
            let ty = field(f, "type").map(|x| text(x, cx.src).to_string());
            for d in children(f).into_iter().filter(|d| d.kind().contains("declarator") || d.kind() == "field_identifier") {
                if let Some(id) = innermost_ident(d, cx.src) {
                    let ptr = text(d, cx.src).starts_with('*');
                    add_field(b, cx, f, &id, ty.clone().map(|t| if ptr { format!("{t}*") } else { t }), t, Visibility::Public, false);
                }
            }
        }
    }
}

fn c_items(b: &mut Builder, cx: &Ctx, n: Node, module: usize, _ns: Option<&str>) {
    let src = cx.src;
    for ch in children(n) {
        match ch.kind() {
            "preproc_ifdef" | "preproc_if" | "preproc_else" | "preproc_elif" | "linkage_specification" | "declaration_list" => c_items(b, cx, ch, module, None),
            "struct_specifier" | "union_specifier" | "enum_specifier" if field(ch, "body").is_some() => {
                if let Some(name) = field(ch, "name") {
                    c_record(b, cx, ch, text(name, src), module);
                }
            }
            "type_definition" => {
                let alias = children(ch).into_iter().filter(|d| d.kind() == "type_identifier").last().map(|x| text(x, src).to_string());
                if let (Some(t), Some(alias)) = (field(ch, "type"), alias) {
                    if record_kind(t.kind()).is_some() && field(t, "body").is_some() {
                        let name = field(t, "name").map(|x| text(x, src).to_string()).unwrap_or(alias);
                        c_record(b, cx, t, &name, module);
                    }
                }
            }
            "declaration" | "field_declaration" => {
                // struct definitions inside declarations, and prototypes
                if let Some(t) = field(ch, "type").filter(|t| record_kind(t.kind()).is_some() && field(*t, "body").is_some()) {
                    if let Some(name) = field(t, "name") {
                        c_record(b, cx, t, text(name, src), module);
                    }
                }
                if let Some(fd) = field(ch, "declarator").and_then(fn_declarator) {
                    if let Some(name) = field(fd, "declarator").and_then(|d| innermost_ident(d, src)) {
                        let vis = if text(ch, src).trim_start().starts_with("static") { Visibility::Private } else { Visibility::Public };
                        callable(b, cx, ch, &name, EntityKind::Function, ParentRef::Item(module), c_params(fd, src), field(ch, "type").map(|x| text(x, src).to_string()), None, vis);
                    }
                }
            }
            "function_definition" => {
                let Some(fd) = field(ch, "declarator").and_then(fn_declarator) else { continue };
                let Some(name) = field(fd, "declarator").and_then(|d| innermost_ident(d, src)) else { continue };
                let vis = if children(ch).iter().any(|c| c.kind() == "storage_class_specifier" && text(*c, src) == "static") { Visibility::Private } else { Visibility::Public };
                callable(b, cx, ch, &name, EntityKind::Function, ParentRef::Item(module), c_params(fd, src), field(ch, "type").map(|x| text(x, src).to_string()), field(ch, "body"), vis);
            }
            _ => {}
        }
    }
}

// ------------------------------------------------------------------ C++

fn cpp_items(b: &mut Builder, cx: &Ctx, n: Node, module: usize, ns: &str, _vis: Visibility) {
    let src = cx.src;
    for ch in children(n) {
        match ch.kind() {
            "namespace_definition" => {
                let name = field(ch, "name").map(|x| text(x, src).replace("::", ".")).unwrap_or_default();
                let q = if ns.is_empty() { name.clone() } else if name.is_empty() { ns.to_string() } else { format!("{ns}.{name}") };
                let m = if q.is_empty() { module } else { b.module(&q, EntityKind::Namespace, cx.rel, ch.start_position().row as u32 + 1) };
                if let Some(body) = field(ch, "body") {
                    cpp_items(b, cx, body, m, &q, Visibility::Public);
                }
            }
            "preproc_ifdef" | "preproc_if" | "preproc_else" | "linkage_specification" | "declaration_list" => cpp_items(b, cx, ch, module, ns, Visibility::Public),
            "template_declaration" => cpp_items(b, cx, ch, module, ns, Visibility::Public),
            "class_specifier" | "struct_specifier" | "union_specifier" | "enum_specifier" if field(ch, "body").is_some() => cpp_class(b, cx, ch, module),
            "declaration" | "type_definition" | "alias_declaration" => {
                if let Some(t) = field(ch, "type").filter(|t| record_kind(t.kind()).is_some() && field(*t, "body").is_some()) {
                    cpp_class(b, cx, t, module);
                }
                if let Some(fd) = field(ch, "declarator").and_then(fn_declarator) {
                    cpp_function(b, cx, ch, fd, None, module);
                }
            }
            "function_definition" => {
                if let Some(fd) = field(ch, "declarator").and_then(fn_declarator) {
                    cpp_function(b, cx, ch, fd, field(ch, "body"), module);
                }
            }
            _ => {}
        }
    }
}

fn cpp_function(b: &mut Builder, cx: &Ctx, n: Node, fd: Node, body: Option<Node>, module: usize) {
    let src = cx.src;
    let Some(d) = field(fd, "declarator") else { return };
    let params = c_params(fd, src);
    let returns = field(n, "type").map(|x| text(x, src).to_string());
    if d.kind() == "qualified_identifier" {
        // Type::method defined out of class
        let scope = field(d, "scope").map(|x| simple_type(text(x, src))).unwrap_or_default();
        let name = field(d, "name").map(|x| text(x, src).to_string()).unwrap_or_default();
        let is_type = b.items.iter().any(|it| it.e.kind.is_some_and(EntityKind::is_type) && it.e.name == scope);
        if is_type || scope.chars().next().is_some_and(char::is_uppercase) {
            let i = callable(b, cx, n, &name, EntityKind::Method, ParentRef::TypeName { module: Some(module), name: scope }, params, returns, body, Visibility::Package);
            b.items[i].calls.retain(|_| true);
            return;
        }
        callable(b, cx, n, &name, EntityKind::Function, ParentRef::Item(module), params, returns, body, Visibility::Public);
        return;
    }
    if let Some(name) = innermost_ident(d, src) {
        callable(b, cx, n, &name, EntityKind::Function, ParentRef::Item(module), params, returns, body, Visibility::Public);
    }
}

fn cpp_class(b: &mut Builder, cx: &Ctx, n: Node, module: usize) {
    let src = cx.src;
    let Some(name) = field(n, "name").map(|x| simple_type(text(x, src))) else { return };
    let kind = record_kind(n.kind()).unwrap_or(EntityKind::Class);
    let t = type_item(b, cx, n, &name, kind, module);
    for bc in children(n).into_iter().filter(|c| c.kind() == "base_class_clause") {
        for ty in children(bc).into_iter().filter(|x| matches!(x.kind(), "type_identifier" | "qualified_identifier" | "template_type")) {
            b.items[t].heritage.push((simple_type(text(ty, src)), EdgeKind::Extends));
        }
    }
    let mut vis = if kind == EntityKind::Class { Visibility::Private } else { Visibility::Public };
    let (mut methods, mut abstract_methods, mut fields) = (0, 0, 0);
    let Some(body) = field(n, "body") else { return };
    for m in children(body) {
        match m.kind() {
            "access_specifier" => {
                vis = match text(m, src).trim() {
                    "private" => Visibility::Private,
                    "protected" => Visibility::Protected,
                    _ => Visibility::Public,
                }
            }
            "function_definition" => {
                if let Some(fd) = field(m, "declarator").and_then(fn_declarator) {
                    if let Some(mname) = field(fd, "declarator").and_then(|d| innermost_ident(d, src)) {
                        methods += 1;
                        let i = callable(b, cx, m, &mname, EntityKind::Method, ParentRef::Item(t), c_params(fd, src), field(m, "type").map(|x| text(x, src).to_string()), field(m, "body"), vis);
                        b.items[i].e.is_static = text(m, src).trim_start().starts_with("static");
                    }
                }
            }
            "field_declaration" | "declaration" => {
                if let Some(fd) = field(m, "declarator").and_then(fn_declarator) {
                    if let Some(mname) = field(fd, "declarator").and_then(|d| innermost_ident(d, src)) {
                        methods += 1;
                        let pure = text(m, src).replace(' ', "").contains(")=0") || text(m, src).replace(' ', "").contains("const=0");
                        if pure {
                            abstract_methods += 1;
                        }
                        let i = callable(b, cx, m, &mname, EntityKind::Method, ParentRef::Item(t), c_params(fd, src), field(m, "type").map(|x| text(x, src).to_string()), None, vis);
                        b.items[i].e.is_static = text(m, src).trim_start().starts_with("static");
                        b.items[i].e.is_abstract = pure;
                    }
                } else {
                    let ty = field(m, "type").map(|x| text(x, src).to_string());
                    for d in children(m).into_iter().filter(|d| d.kind().contains("declarator") || d.kind() == "field_identifier") {
                        if let Some(id) = innermost_ident(d, src) {
                            fields += 1;
                            add_field(b, cx, m, &id, ty.clone(), t, vis, text(m, src).trim_start().starts_with("static"));
                        }
                    }
                }
            }
            _ => {}
        }
    }
    // a class of only pure virtual functions (destructor aside) is an interface
    let virtual_dtor = text(body, src).contains("~");
    if kind == EntityKind::Class && fields == 0 && abstract_methods > 0 && abstract_methods + usize::from(virtual_dtor) >= methods {
        b.items[t].e.kind = Some(EntityKind::Interface);
    }
}
