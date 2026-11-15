//! Clojure adapter: namespaces, `defn`/`defn-`/`def` functions, `defprotocol`, `defrecord`/
//! `deftype` (with inline protocol implementations), `extend-protocol`/`extend-type`,
//! `defmulti`/`defmethod`, `defmacro`. Macros are not expanded.

use super::{facts, text, Builder, Call, FactSpec, Item, ParentRef};
use spide_core::model::{Param, Visibility};
use spide_core::symbols::{EdgeKind, EntityKind};
use std::collections::{BTreeMap, BTreeSet};
use tree_sitter::Node;

const CLJ: FactSpec = FactSpec {
    branches: &[],
    loops: &[],
    blocks: &["list_lit"],
    comments: &["comment", "dis_expr"],
    nested: &[],
    statements: &["list_lit"],
    creates: &[],
    switches: &[],
    idioms: &[],
    idiom_calls: &[("atom", "mutable_state"), ("swap!", "mutable_state"), ("reset!", "mutable_state"), ("set!", "mutable_state"), ("eval", "eval"), ("alter-var-root", "mutable_state")],
};

const BRANCH_FORMS: [&str; 12] = ["if", "if-not", "when", "when-not", "cond", "condp", "case", "if-let", "when-let", "if-some", "when-some", "cond->"];
const LOOP_FORMS: [&str; 9] = ["loop", "doseq", "dotimes", "for", "map", "reduce", "filter", "mapv", "run!"];

fn values<'a>(n: Node<'a>) -> Vec<Node<'a>> {
    let mut c = n.walk();
    n.children(&mut c).filter(|x| x.is_named() && !matches!(x.kind(), "comment" | "dis_expr")).collect()
}

/// `(head …)` → head symbol text.
fn head<'a>(n: Node, src: &'a [u8]) -> Option<&'a str> {
    if n.kind() != "list_lit" {
        return None;
    }
    let v = values(n);
    let h = v.first()?;
    (h.kind() == "sym_lit").then(|| sym_full(*h, src))
}

/// Full symbol text without metadata (`^:private foo` → `foo`, `pricing/tax`).
fn sym_full<'a>(n: Node, src: &'a [u8]) -> &'a str {
    let name = n.child_by_field_name("name").map(|x| text(x, src)).unwrap_or("");
    match n.child_by_field_name("namespace") {
        Some(ns) => {
            let s = ns.start_byte();
            let e = n.child_by_field_name("name").map(|x| x.end_byte()).unwrap_or(ns.end_byte());
            std::str::from_utf8(&src[s..e]).unwrap_or(name)
        }
        None => name,
    }
}

fn is_private(n: Node, src: &[u8]) -> bool {
    text(n, src).contains(":private")
}

/// Branch/loop counts by form head (Clojure has no statement nodes).
fn form_facts(n: Node, src: &[u8], branches: &mut u32, loops: &mut u32, depth: u32, max_depth: &mut u32, calls: &mut Vec<Call>, idents: &mut BTreeSet<String>) {
    if n.kind() == "list_lit" {
        *max_depth = (*max_depth).max(depth);
        if let Some(h) = head(n, src) {
            if BRANCH_FORMS.contains(&h) {
                *branches += if matches!(h, "cond" | "case" | "condp") { (values(n).len().saturating_sub(1) / 2) as u32 } else { 1 };
            }
            if LOOP_FORMS.contains(&h) {
                *loops += 1;
            }
            if h == "and" || h == "or" {
                *branches += 1;
            }
            let (path, name) = match h.rsplit_once('/') {
                Some((ns, name)) if !ns.is_empty() => (Some(ns.to_string()), name.to_string()),
                _ => (None, h.to_string()),
            };
            // constructor fns ->Record / map->Record
            let name = name.trim_start_matches("map->").trim_start_matches("->").to_string();
            calls.push(Call { recv: None, path, name });
        }
    }
    if n.kind() == "sym_lit" {
        idents.insert(sym_full(n, src).to_string());
    }
    if n.kind() == "kwd_lit" {
        if let Some(k) = n.child_by_field_name("name") {
            idents.insert(text(k, src).to_string());
        }
    }
    let mut c = n.walk();
    for ch in n.children(&mut c) {
        form_facts(ch, src, branches, loops, depth + u32::from(n.kind() == "list_lit"), max_depth, calls, idents);
    }
}

fn params_of(v: Node, src: &[u8]) -> Vec<Param> {
    values(v).into_iter().map(|p| Param { name: text(p, src).to_string(), ty: String::new() }).filter(|p| p.name != "&").collect()
}

/// One function/method body: `(name [args] body…)` or multi-arity `(name ([a] …) ([a b] …))`.
#[allow(clippy::too_many_arguments)]
fn fn_item(b: &mut Builder, n: Node, src: &[u8], rel: &str, name: &str, parent: ParentRef, args_and_body: &[Node], kind: EntityKind, vis: Visibility, aliases: &BTreeMap<String, String>) -> usize {
    let mut it = Item::new(kind, name, parent);
    it.e.file = rel.to_string();
    it.e.start_line = n.start_position().row as u32 + 1;
    it.e.end_line = n.end_position().row as u32 + 1;
    it.e.start_col = n.start_position().column as u32;
    it.e.visibility = vis;
    it.aliases = aliases.clone();
    let params = args_and_body.iter().find(|x| x.kind() == "vec_lit").map(|v| params_of(*v, src)).or_else(|| args_and_body.iter().find(|x| x.kind() == "list_lit").and_then(|l| values(*l).into_iter().find(|x| x.kind() == "vec_lit")).map(|v| params_of(v, src))).unwrap_or_default();
    it.e.params = params.clone();
    it.has_body = true;
    let mut f = facts(&CLJ, n, Some(n), params.len(), rel, src);
    let (mut br, mut lo, mut md) = (0, 0, 0);
    let mut calls = vec![];
    for x in args_and_body {
        form_facts(*x, src, &mut br, &mut lo, 0, &mut md, &mut calls, &mut it.idents);
    }
    f.branches = br;
    f.loops = lo;
    f.max_nesting = md;
    f.statements = f.statements.saturating_sub(1);
    it.e.facts = Some(f);
    // drop calls to special forms/core that never resolve in the workspace (resolution ignores them anyway)
    it.calls = calls;
    b.push(it)
}

pub fn index(b: &mut Builder, root: Node, src: &[u8], rel: &str) {
    let mut module: Option<usize> = None;
    let mut aliases: BTreeMap<String, String> = BTreeMap::new();
    for form in values(root) {
        let Some(h) = head(form, src) else { continue };
        let v = values(form);
        let name_node = v.get(1).copied().filter(|x| x.kind() == "sym_lit");
        let name = name_node.map(|x| sym_full(x, src).to_string()).unwrap_or_default();
        if h == "ns" || h == "in-ns" {
            let q = name.trim_start_matches('\'').to_string();
            module = Some(b.module(&q, EntityKind::Namespace, rel, form.start_position().row as u32 + 1));
            // (:require [x.y :as z])
            for clause in v.iter().skip(2).filter(|c| c.kind() == "list_lit") {
                for spec in values(*clause).into_iter().filter(|s| s.kind() == "vec_lit") {
                    let parts = values(spec);
                    if let Some(target) = parts.first().filter(|p| p.kind() == "sym_lit") {
                        let target = sym_full(*target, src).to_string();
                        for w in parts.windows(2) {
                            if text(w[0], src) == ":as" {
                                aliases.insert(text(w[1], src).to_string(), target.clone());
                            }
                        }
                        aliases.insert(target.clone(), target);
                    }
                }
            }
            continue;
        }
        let m = match module {
            Some(m) => m,
            None => {
                let q = super::module_path("clojure", rel, None);
                let m = b.module(&q, EntityKind::Namespace, rel, 1);
                module = Some(m);
                m
            }
        };
        if name.is_empty() && !matches!(h, "extend-protocol" | "extend-type") {
            continue;
        }
        let vis = if h.ends_with('-') || name_node.is_some_and(|x| is_private(x, src)) { Visibility::Private } else { Visibility::Public };
        match h {
            "defn" | "defn-" | "defmacro" | "defmulti" => {
                let kind = if h == "defmacro" { EntityKind::Macro } else { EntityKind::Function };
                fn_item(b, form, src, rel, &name, ParentRef::Item(m), &v[2..], kind, vis, &aliases);
            }
            "defmethod" => {
                // (defmethod area :circle [c] …): an implementation of the multimethod
                let dispatch = v.get(2).map(|x| text(*x, src)).unwrap_or("");
                fn_item(b, form, src, rel, &format!("{name} {dispatch}"), ParentRef::Item(m), &v[2..], EntityKind::Function, vis, &aliases);
            }
            "def" | "defonce" => {
                let value = v.get(2).copied();
                if value.is_some_and(|x| head(x, src) == Some("fn")) {
                    let fv = values(value.unwrap());
                    fn_item(b, form, src, rel, &name, ParentRef::Item(m), &fv[1..], EntityKind::Function, vis, &aliases);
                } else {
                    let mut f = Item::new(EntityKind::Field, &name, ParentRef::Item(m));
                    f.e.file = rel.into();
                    f.e.start_line = form.start_position().row as u32 + 1;
                    f.e.end_line = form.end_position().row as u32 + 1;
                    f.e.visibility = vis;
                    f.e.is_static = true;
                    f.e.ty = value.and_then(|x| head(x, src)).map(|h| if h == "atom" { "atom".to_string() } else { h.to_string() });
                    b.push(f);
                }
            }
            "defprotocol" | "definterface" => {
                let mut t = Item::new(EntityKind::Protocol, &name, ParentRef::Item(m));
                t.e.file = rel.into();
                t.e.start_line = form.start_position().row as u32 + 1;
                t.e.end_line = form.end_position().row as u32 + 1;
                t.e.visibility = Visibility::Public;
                t.e.is_abstract = true;
                let ti = b.push(t);
                for sig in v.iter().skip(2).filter(|x| x.kind() == "list_lit") {
                    let sv = values(*sig);
                    if let Some(n) = sv.first().filter(|x| x.kind() == "sym_lit") {
                        let mut it = Item::new(EntityKind::Method, sym_full(*n, src), ParentRef::Item(ti));
                        it.e.file = rel.into();
                        it.e.start_line = sig.start_position().row as u32 + 1;
                        it.e.end_line = sig.end_position().row as u32 + 1;
                        it.e.is_abstract = true;
                        it.e.visibility = Visibility::Public;
                        it.e.params = sv.iter().find(|x| x.kind() == "vec_lit").map(|p| params_of(*p, src)).unwrap_or_default();
                        b.push(it);
                    }
                }
            }
            "defrecord" | "deftype" => {
                let mut t = Item::new(if h == "defrecord" { EntityKind::Record } else { EntityKind::Class }, &name, ParentRef::Item(m));
                t.e.file = rel.into();
                t.e.start_line = form.start_position().row as u32 + 1;
                t.e.end_line = form.end_position().row as u32 + 1;
                t.e.visibility = Visibility::Public;
                let ti = b.push(t);
                if let Some(fields) = v.get(2).filter(|x| x.kind() == "vec_lit") {
                    for f in values(*fields) {
                        let mut it = Item::new(EntityKind::Field, text(f, src), ParentRef::Item(ti));
                        it.e.file = rel.into();
                        it.e.start_line = f.start_position().row as u32 + 1;
                        it.e.end_line = it.e.start_line;
                        it.e.visibility = Visibility::Public;
                        b.push(it);
                    }
                }
                inline_impls(b, &v[3.min(v.len())..], src, rel, ti, &aliases);
            }
            "extend-protocol" => {
                // (extend-protocol P TypeA (m [x] …) TypeB (m …))
                let proto = v.get(1).map(|x| sym_full(*x, src).rsplit('/').next().unwrap_or("").to_string()).unwrap_or_default();
                for x in v.iter().skip(2).filter(|x| x.kind() == "sym_lit") {
                    let ty = sym_full(*x, src).rsplit(['.', '/']).next().unwrap_or("").to_string();
                    b.pending_impls.push((Some(m), ty, proto.clone()));
                }
            }
            "extend-type" => {
                let ty = name.rsplit(['.', '/']).next().unwrap_or("").to_string();
                for x in v.iter().skip(2).filter(|x| x.kind() == "sym_lit") {
                    b.pending_impls.push((Some(m), ty.clone(), sym_full(*x, src).rsplit('/').next().unwrap_or("").to_string()));
                }
            }
            _ => {}
        }
    }
}

/// `Proto (m [this] …) (n [this] …) OtherProto …` inside defrecord/deftype/reify.
fn inline_impls(b: &mut Builder, forms: &[Node], src: &[u8], rel: &str, ty: usize, aliases: &BTreeMap<String, String>) {
    for f in forms {
        match f.kind() {
            "sym_lit" => {
                let p = sym_full(*f, src).rsplit('/').next().unwrap_or("").to_string();
                b.items[ty].heritage.push((p, EdgeKind::Implements));
            }
            "list_lit" => {
                let v = values(*f);
                if let Some(n) = v.first().filter(|x| x.kind() == "sym_lit") {
                    let name = sym_full(*n, src).to_string();
                    fn_item(b, *f, src, rel, &name, ParentRef::Item(ty), &v[1..], EntityKind::Method, Visibility::Public, aliases);
                }
            }
            _ => {}
        }
    }
}
