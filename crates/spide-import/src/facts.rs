//! What a method body actually does: size, creations, call chains, switches, arithmetic, loops,
//! UI toolkit use, and a normalised fingerprint for clone detection.

use spide_core::model::CodeFacts;
use std::collections::BTreeSet;
use tree_sitter::Node;

fn text<'a>(n: Node, src: &'a [u8]) -> &'a str {
    std::str::from_utf8(&src[n.byte_range()]).unwrap_or("")
}

const STREAM_LOOPS: [&str; 9] = ["stream", "forEach", "filter", "map", "mapToInt", "sum", "count", "reduce", "collect"];
const UI_PREFIXES: [&str; 6] = ["JOptionPane", "JTable", "JLabel", "JPanel", "SwingUtilities", "JFrame"];
const UI_CALLS: [&str; 7] = ["repaint", "revalidate", "setText", "fireTableDataChanged", "setVisible", "showMessageDialog", "setEnabled"];
const K: usize = 5;
const W: usize = 4;

fn fnv64(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// The calls of a receiver chain, outermost first: `a.getB().getC().x()` → [x, getC, getB].
fn chain_names<'a>(n: Node, src: &'a [u8], out: &mut Vec<&'a str>) {
    match n.kind() {
        "method_invocation" => {
            out.push(n.child_by_field_name("name").map(|x| text(x, src)).unwrap_or(""));
            if let Some(o) = n.child_by_field_name("object") {
                chain_names(o, src, out);
            }
        }
        "field_access" => {
            out.push("."); // a field of a stranger counts like a getter
            if let Some(o) = n.child_by_field_name("object") {
                chain_names(o, src, out);
            }
        }
        _ => {}
    }
}

/// Law of Demeter depth: how many strangers are reached through getters (`p.getPiece().getLocation()
/// .getName()` → 3). Fluent chains (`sb.append(a).append(b)`) and plain `get(i)` collection access don't count.
fn demeter_depth(n: Node, src: &[u8]) -> u32 {
    let mut names = vec![];
    chain_names(n, src, &mut names);
    let distinct: BTreeSet<&&str> = names.iter().collect();
    if distinct.len() < names.len() {
        return 0; // fluent/builder style
    }
    names
        .iter()
        .filter(|m| **m == "." || ((m.starts_with("get") || m.starts_with("is") || m.starts_with("has")) && m.len() > 3))
        .count() as u32
}

struct Walk<'a> {
    src: &'a [u8],
    f: CodeFacts,
    creates: BTreeSet<String>,
    switch_on: BTreeSet<String>,
    ui: BTreeSet<String>,
    tokens: Vec<String>,
}

impl Walk<'_> {
    fn visit(&mut self, n: Node) {
        let k = n.kind();
        if matches!(k, "class_declaration" | "interface_declaration" | "enum_declaration" | "record_declaration" | "class_body" | "method_declaration") { return; }
        if (k.ends_with("_statement") && k != "labeled_statement") || k == "local_variable_declaration" {
            self.f.statements += 1;
        }
        if matches!(k, "if_statement" | "for_statement" | "enhanced_for_statement" | "while_statement" | "do_statement" | "catch_clause" | "switch_label" | "ternary_expression") {
            self.f.branches += 1;
        }
        if k == "binary_expression" && matches!(n.child_by_field_name("operator").map(|o| text(o, self.src)), Some("&&" | "||")) {
            self.f.branches += 1;
        }
        match k {
            "object_creation_expression" => {
                if let Some(t) = n.child_by_field_name("type") {
                    let name = text(t, self.src).split('<').next().unwrap_or("").trim().to_string();
                    if !name.is_empty() {
                        self.creates.insert(name);
                    }
                }
            }
            "method_invocation" => {
                let d = demeter_depth(n, self.src);
                // only the outermost call of a chain records it
                let outer = n.parent().is_none_or(|p| !(p.kind() == "method_invocation" && p.child_by_field_name("object") == Some(n)));
                if d > self.f.max_chain {
                    self.f.max_chain = d;
                }
                if outer && d >= 3 {
                    let t: String = text(n, self.src).split_whitespace().collect::<Vec<_>>().join(" ");
                    self.f.chains.push(if t.len() > 90 { format!("{}…", &t[..t.char_indices().nth(88).map(|(i, _)| i).unwrap_or(t.len())]) } else { t });
                }
                let name = n.child_by_field_name("name").map(|x| text(x, self.src)).unwrap_or("");
                if STREAM_LOOPS.contains(&name) {
                    self.f.loops += 1;
                }
                if UI_CALLS.contains(&name) {
                    self.ui.insert(format!("{name}()"));
                }
            }
            "switch_expression" | "switch_statement" => self.f.switches += 1,
            "switch_label" => {
                let mut c = n.walk();
                for ch in n.named_children(&mut c) {
                    let t = text(ch, self.src).trim();
                    if t.chars().next().is_some_and(|c| c.is_uppercase()) && t.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '.') {
                        self.switch_on.insert(t.rsplit('.').next().unwrap_or(t).to_string());
                    }
                }
            }
            "instanceof_expression" => self.f.instanceofs += 1,
            "binary_expression" => {
                let op = n.child_by_field_name("operator").map(|o| text(o, self.src)).unwrap_or("");
                match op {
                    "+" | "-" | "*" | "/" | "%" => self.f.arithmetic += 1,
                    "<" | ">" | "<=" | ">=" | "==" | "!=" => self.f.comparisons += 1,
                    _ => {}
                }
            }
            "update_expression" | "assignment_expression" => {
                let t = text(n, self.src);
                if t.contains("++") || t.contains("--") || t.contains("+=") || t.contains("-=") || t.contains("*=") {
                    self.f.arithmetic += 1;
                }
            }
            "for_statement" | "enhanced_for_statement" | "while_statement" | "do_statement" => self.f.loops += 1,
            "type_identifier" | "identifier" => {
                let t = text(n, self.src);
                if UI_PREFIXES.iter().any(|p| t.starts_with(p)) {
                    self.ui.insert(t.to_string());
                }
            }
            _ => {}
        }
        // normalised tokens for the fingerprint
        if n.child_count() == 0 {
            let t = text(n, self.src);
            let tok = match k {
                "identifier" | "type_identifier" | "field_identifier" => "ID".to_string(),
                _ if k.ends_with("_literal") || k == "string_fragment" || k == "true" || k == "false" || k == "null_literal" => "LIT".to_string(),
                "line_comment" | "block_comment" => String::new(),
                _ => t.to_string(),
            };
            if !tok.is_empty() {
                self.tokens.push(tok);
            }
        } else if k.ends_with("_literal") {
            self.tokens.push("LIT".into());
            return;
        }
        let mut c = n.walk();
        for ch in n.children(&mut c) {
            self.visit(ch);
        }
    }
}

/// Winnowing (Schleimer et al.): k-gram hashes, keep the minimum of every window.
pub fn winnow(tokens: &[String]) -> Vec<u64> {
    if tokens.len() < K {
        return vec![];
    }
    let hashes: Vec<u64> = tokens.windows(K).map(|w| fnv64(&w.join(" "))).collect();
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

/// Count syntax-token-bearing body lines, not comment text or nested type/method bodies.
fn code_rows(n: Node, src: &[u8], rows: &mut BTreeSet<usize>) {
    if matches!(n.kind(), "line_comment" | "block_comment" | "class_declaration" | "interface_declaration" | "enum_declaration" | "record_declaration" | "method_declaration" | "class_body") { return; }
    if n.child_count() == 0 {
        if !matches!(n.kind(), "{" | "}") {
            for (line, value) in text(n, src).lines().enumerate() {
                if !value.trim().is_empty() { rows.insert(n.start_position().row + line); }
            }
        }
    } else {
        let mut cursor = n.walk();
        for child in n.children(&mut cursor) { code_rows(child, src, rows); }
    }
}

/// Deepest block nesting inside a body (the body's own block is depth 0).
fn nesting(n: Node, depth: u32) -> u32 {
    if matches!(n.kind(), "class_body" | "lambda_expression") {
        return depth;
    }
    let mut max = depth;
    let mut c = n.walk();
    for ch in n.children(&mut c) {
        let d = if ch.kind() == "block" { depth + 1 } else { depth };
        max = max.max(nesting(ch, d));
    }
    max
}

/// Facts for a method declaration node (`body` may be missing for abstract methods).
pub fn facts(decl: Node, body: Option<Node>, src: &[u8]) -> CodeFacts {
    let mut w = Walk { src, f: CodeFacts::default(), creates: BTreeSet::new(), switch_on: BTreeSet::new(), ui: BTreeSet::new(), tokens: vec![] };
    w.f.start_line = decl.start_position().row as u32 + 1;
    w.f.end_line = decl.end_position().row as u32 + 1;
    if let Some(b) = body {
        let t = text(b, src);
        let mut rows = BTreeSet::new();
        code_rows(b, src, &mut rows);
        w.f.loc = rows.len() as u32;
        w.f.hash = format!("{:016x}", fnv64(t));
        w.f.max_nesting = nesting(b, 0);
        w.visit(b);
    }
    let mut f = w.f;
    f.creates = w.creates.into_iter().collect();
    f.switch_on = w.switch_on.into_iter().collect();
    f.ui_api = w.ui.into_iter().collect();
    f.fingerprint = winnow(&w.tokens);
    f
}
