//! UML text of class boxes, as written into Lucid compartments and read back.
//!
//! - title: optional `«stereotype»` line, the name, optional `{abstract}` line
//! - attribute: `+ {static} total: Money [0..1]` (type and multiplicity optional)
//! - operation: `+ {abstract} {static} addLine(p: Product, qty: int): OrderLine`
//!
//! `parse_*(format_*(x))` restores every field the text shows; everything else (descriptions, code
//! links, status) is not part of the text and is kept by the mapper from the design.

use crate::model::{Attribute, Class, Method, Param, Visibility};

fn vis_glyph(v: Visibility) -> &'static str {
    v.glyph()
}

fn vis_of(c: char) -> Option<Visibility> {
    Some(match c {
        '+' => Visibility::Public,
        '-' => Visibility::Private,
        '#' => Visibility::Protected,
        '~' => Visibility::Package,
        _ => return None,
    })
}

/// The stereotype Lucid shows for a class (`interface`, `enum` …), or None.
pub fn shown_stereotype(c: &Class) -> Option<&str> {
    c.stereotype.as_deref().filter(|s| !s.trim().is_empty() && *s != "abstract")
}

pub fn format_title(c: &Class) -> String {
    let mut lines = vec![];
    if let Some(s) = shown_stereotype(c) {
        lines.push(format!("«{s}»"));
    }
    lines.push(c.name.clone());
    if c.is_abstract || c.stereotype.as_deref() == Some("abstract") {
        lines.push("{abstract}".to_string());
    }
    lines.join("\n")
}

/// A parsed title compartment.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Title {
    pub name: String,
    pub stereotype: Option<String>,
    pub is_abstract: bool,
}

pub fn parse_title(text: &str) -> Title {
    let mut t = Title::default();
    let mut names = vec![];
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if let Some(s) = line.strip_prefix('«').and_then(|l| l.strip_suffix('»')).or_else(|| line.strip_prefix("<<").and_then(|l| l.strip_suffix(">>"))) {
            t.stereotype = Some(s.trim().to_string());
        } else if line.eq_ignore_ascii_case("{abstract}") {
            t.is_abstract = true;
        } else {
            names.push(line);
        }
    }
    t.name = names.join(" ");
    t
}

pub fn format_attribute(a: &Attribute) -> String {
    let mut s = format!("{} ", vis_glyph(a.visibility));
    if a.is_static {
        s.push_str("{static} ");
    }
    s.push_str(&a.name);
    if !a.ty.trim().is_empty() {
        s.push_str(": ");
        s.push_str(a.ty.trim());
    }
    if let Some(m) = a.multiplicity.as_deref().filter(|m| !m.trim().is_empty()) {
        s.push_str(&format!(" [{}]", m.trim()));
    }
    s
}

/// A domain concept's attribute: no visibility (`isbn: ISBN [1]`).
pub fn format_concept_attribute(a: &crate::model::ConceptAttribute) -> String {
    let mut s = a.name.clone();
    if let Some(t) = a.ty.as_deref().filter(|t| !t.trim().is_empty()) {
        s.push_str(": ");
        s.push_str(t.trim());
    }
    if let Some(m) = a.multiplicity.as_deref().filter(|m| !m.trim().is_empty()) {
        s.push_str(&format!(" [{}]", m.trim()));
    }
    s
}

pub fn parse_concept_attribute(line: &str) -> Option<crate::model::ConceptAttribute> {
    let a = parse_attribute(line)?;
    Some(crate::model::ConceptAttribute { name: a.name, ty: (!a.ty.is_empty()).then_some(a.ty), multiplicity: a.multiplicity })
}

pub fn format_method(m: &Method) -> String {
    let mut s = format!("{} ", vis_glyph(m.visibility));
    if m.is_abstract {
        s.push_str("{abstract} ");
    }
    if m.is_static {
        s.push_str("{static} ");
    }
    s.push_str(&m.name);
    let params: Vec<String> = m.params.iter().map(|p| if p.ty.trim().is_empty() { p.name.clone() } else { format!("{}: {}", p.name, p.ty.trim()) }).collect();
    s.push_str(&format!("({})", params.join(", ")));
    if let Some(r) = m.returns.as_deref().filter(|r| !r.trim().is_empty()) {
        s.push_str(": ");
        s.push_str(r.trim());
    }
    s
}

/// Leading visibility and `{static}`/`{abstract}` modifiers.
fn modifiers(line: &str) -> (Visibility, bool, bool, &str) {
    let mut rest = line.trim();
    let mut vis = Visibility::Public;
    if let Some(c) = rest.chars().next() {
        if let Some(v) = vis_of(c) {
            vis = v;
            rest = rest[c.len_utf8()..].trim_start();
        }
    }
    let (mut is_static, mut is_abstract) = (false, false);
    loop {
        if let Some(r) = strip_ci(rest, "{static}") {
            is_static = true;
            rest = r.trim_start();
        } else if let Some(r) = strip_ci(rest, "{abstract}") {
            is_abstract = true;
            rest = r.trim_start();
        } else {
            break;
        }
    }
    (vis, is_static, is_abstract, rest)
}

fn strip_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    (s.len() >= prefix.len() && s[..prefix.len()].eq_ignore_ascii_case(prefix)).then(|| &s[prefix.len()..])
}

/// Split at the first `sep` outside `<>`, `()` and `[]` (generic types like `Map<K, V>`).
fn split_top(s: &str, sep: char) -> Option<(&str, &str)> {
    let mut depth = 0i32;
    for (i, c) in s.char_indices() {
        match c {
            '<' | '(' | '[' | '{' => depth += 1,
            '>' | ')' | ']' | '}' => depth -= 1,
            c if c == sep && depth == 0 => return Some((&s[..i], &s[i + c.len_utf8()..])),
            _ => {}
        }
    }
    None
}

fn split_all_top(s: &str, sep: char) -> Vec<&str> {
    let mut out = vec![];
    let mut rest = s;
    while let Some((a, b)) = split_top(rest, sep) {
        out.push(a);
        rest = b;
    }
    out.push(rest);
    out
}

/// Parse an attribute line; None for an empty line.
pub fn parse_attribute(line: &str) -> Option<Attribute> {
    if line.trim().is_empty() {
        return None;
    }
    let (visibility, is_static, _, rest) = modifiers(line);
    // trailing multiplicity `[0..1]`
    // trailing multiplicity ` [0..1]`: separated by a space, not empty (`byte[]` is a type)
    let rest = rest.trim_end();
    let (rest, multiplicity) = match rest.strip_suffix(']').and_then(|r| r.rfind('[').map(|i| (r, i))) {
        Some((r, i)) if r[..i].ends_with(char::is_whitespace) && !r[i + 1..].trim().is_empty() && !r[..i].trim().is_empty() && !r[..i].trim_end().ends_with(':') => (r[..i].trim_end().to_string(), Some(r[i + 1..].trim().to_string())),
        _ => (rest.to_string(), None),
    };
    let (name, ty) = match split_top(&rest, ':') {
        Some((n, t)) => (n.trim().to_string(), t.trim().to_string()),
        None => (rest.trim().to_string(), String::new()),
    };
    Some(Attribute { name, ty, visibility, is_static, multiplicity: multiplicity.filter(|m| !m.is_empty()), ..Default::default() })
}

/// Parse an operation line; None when it isn't one (no parentheses).
pub fn parse_method(line: &str) -> Option<Method> {
    if line.trim().is_empty() {
        return None;
    }
    let (visibility, is_static, is_abstract, rest) = modifiers(line);
    let open = rest.find('(')?;
    let name = rest[..open].trim().to_string();
    // matching close paren
    let mut depth = 0i32;
    let mut close = None;
    for (i, c) in rest[open..].char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    close = Some(open + i);
                    break;
                }
            }
            _ => {}
        }
    }
    let close = close?;
    let inner = rest[open + 1..close].trim();
    let params: Vec<Param> = if inner.is_empty() {
        vec![]
    } else {
        split_all_top(inner, ',')
            .into_iter()
            .map(|p| match split_top(p, ':') {
                Some((n, t)) => Param { name: n.trim().to_string(), ty: t.trim().to_string() },
                None => Param { name: p.trim().to_string(), ty: String::new() },
            })
            .collect()
    };
    let after = rest[close + 1..].trim();
    let returns = after.strip_prefix(':').map(|r| r.trim().to_string()).filter(|r| !r.is_empty());
    Some(Method { name, params, returns, visibility, is_static, is_abstract, ..Default::default() })
}

// ------------------------------------------------------------------ relationships

use crate::model::{Interaction, RelationKind};

/// Lucid relationship type for a Spide relation (`from` is the whole / the child / the client in both).
pub fn relation_type(kind: RelationKind, directed: bool) -> &'static str {
    match kind {
        RelationKind::Association if directed => "directedAssociation",
        RelationKind::Association => "association",
        RelationKind::Aggregation => "aggregation",
        RelationKind::Composition => "composition",
        RelationKind::Inheritance => "generalization",
        RelationKind::Realization => "realization",
        RelationKind::Dependency => "dependency",
    }
}

/// Inverse of [`relation_type`]; unknown/custom lines read as associations.
pub fn relation_kind(rel: &str) -> (RelationKind, bool) {
    match rel {
        "directedAssociation" => (RelationKind::Association, true),
        "aggregation" => (RelationKind::Aggregation, false),
        "composition" => (RelationKind::Composition, false),
        "generalization" | "inheritance" => (RelationKind::Inheritance, false),
        "realization" | "implementation" => (RelationKind::Realization, false),
        "dependency" => (RelationKind::Dependency, false),
        _ => (RelationKind::Association, false),
    }
}

/// The mid-line label: `«async»`/`«event»` then the relation's own label.
pub fn relation_label(interaction: Interaction, label: Option<&str>) -> String {
    let mut parts = vec![];
    match interaction {
        Interaction::Async => parts.push("«async»".to_string()),
        Interaction::Event => parts.push("«event»".to_string()),
        Interaction::Sync => {}
    }
    if let Some(l) = label.map(str::trim).filter(|l| !l.is_empty()) {
        parts.push(l.to_string());
    }
    parts.join(" ")
}

pub fn parse_relation_label(text: &str) -> (Interaction, Option<String>) {
    let t = text.trim();
    let (i, rest) = if let Some(r) = t.strip_prefix("«async»").or_else(|| t.strip_prefix("<<async>>")) {
        (Interaction::Async, r)
    } else if let Some(r) = t.strip_prefix("«event»").or_else(|| t.strip_prefix("<<event>>")) {
        (Interaction::Event, r)
    } else {
        (Interaction::Sync, t)
    };
    let rest = rest.trim();
    (i, (!rest.is_empty()).then(|| rest.to_string()))
}

// ------------------------------------------------------------------ C4 blocks

/// `Name`, `[Container: technology]` (or `[Software System]`), description.
pub fn c4_text(name: &str, kind: &str, technology: Option<&str>, description: Option<&str>) -> String {
    let mut lines = vec![name.to_string()];
    match technology.map(str::trim).filter(|t| !t.is_empty()) {
        Some(t) => lines.push(format!("[{kind}: {t}]")),
        None => lines.push(format!("[{kind}]")),
    }
    if let Some(d) = description.map(str::trim).filter(|d| !d.is_empty()) {
        lines.push(d.to_string());
    }
    lines.join("\n")
}

/// Inverse of [`c4_text`]: (name, technology, description).
pub fn parse_c4_text(text: &str) -> (String, Option<String>, Option<String>) {
    let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty());
    let name = lines.next().unwrap_or_default().to_string();
    let mut tech = None;
    let mut desc = vec![];
    for l in lines {
        if tech.is_none() && desc.is_empty() && l.starts_with('[') && l.ends_with(']') {
            let inner = &l[1..l.len() - 1];
            tech = Some(inner.split_once(':').map(|(_, t)| t.trim().to_string()).unwrap_or_default());
        } else {
            desc.push(l);
        }
    }
    (name, tech.filter(|t| !t.is_empty()), (!desc.is_empty()).then(|| desc.join(" ")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relations_and_c4_round_trip() {
        for k in [RelationKind::Association, RelationKind::Aggregation, RelationKind::Composition, RelationKind::Inheritance, RelationKind::Realization, RelationKind::Dependency] {
            assert_eq!(relation_kind(relation_type(k, false)), (k, false));
        }
        assert_eq!(relation_kind(relation_type(RelationKind::Association, true)), (RelationKind::Association, true));
        assert_eq!(parse_relation_label(&relation_label(Interaction::Async, Some("pays"))), (Interaction::Async, Some("pays".into())));
        assert_eq!(parse_relation_label(""), (Interaction::Sync, None));
        let t = c4_text("API", "Container", Some("Spring Boot"), Some("serves loans"));
        assert_eq!(parse_c4_text(&t), ("API".into(), Some("Spring Boot".into()), Some("serves loans".into())));
        assert_eq!(parse_c4_text(&c4_text("Mail", "Software System", None, None)), ("Mail".into(), None, None));
    }

    #[test]
    fn titles_round_trip() {
        let c = Class { name: "Payment".into(), stereotype: Some("interface".into()), ..Default::default() };
        assert_eq!(format_title(&c), "«interface»\nPayment");
        assert_eq!(parse_title(&format_title(&c)), Title { name: "Payment".into(), stereotype: Some("interface".into()), is_abstract: false });
        let a = Class { name: "Shape".into(), is_abstract: true, ..Default::default() };
        assert_eq!(parse_title(&format_title(&a)), Title { name: "Shape".into(), stereotype: None, is_abstract: true });
        assert_eq!(parse_title("<<entity>>\nBook").stereotype.as_deref(), Some("entity"));
    }

    /// Exhaustive over the shapes members take: `parse(format(x))` restores every shown field.
    #[test]
    fn format_parse_round_trips() {
        let vis = [Visibility::Public, Visibility::Private, Visibility::Protected, Visibility::Package];
        let types = ["", "int", "List<Loan>", "Map<String, List<Item>>", "byte[]", "Option<(A, B)>", "fn(i32) -> bool"];
        let mults = [None, Some("0..1"), Some("*"), Some("1..*")];
        let names = ["total", "_x", "camelCase", "äöü"];
        let mut n = 0;
        for v in vis {
            for st in [false, true] {
                for t in types {
                    for m in mults {
                        for name in names {
                            let a = Attribute { name: name.into(), ty: t.into(), visibility: v, is_static: st, multiplicity: m.map(str::to_string), ..Default::default() };
                            assert_eq!(parse_attribute(&format_attribute(&a)).as_ref(), Some(&a), "{}", format_attribute(&a));
                            n += 1;
                        }
                    }
                }
            }
        }
        let params: [&[(&str, &str)]; 4] = [&[], &[("p", "Product")], &[("m", "Map<K, V>"), ("n", "int")], &[("cb", "fn(a, b)"), ("x", "")]];
        for v in vis {
            for (st, ab) in [(false, false), (true, false), (false, true), (true, true)] {
                for ps in params {
                    for r in [None, Some("Order"), Some("Result<Vec<Loan>, Error>")] {
                        let m = Method {
                            name: "addLine".into(),
                            params: ps.iter().map(|(a, b)| Param { name: (*a).into(), ty: (*b).into() }).collect(),
                            returns: r.map(str::to_string),
                            visibility: v,
                            is_static: st,
                            is_abstract: ab,
                            ..Default::default()
                        };
                        assert_eq!(parse_method(&format_method(&m)).as_ref(), Some(&m), "{}", format_method(&m));
                        n += 1;
                    }
                }
            }
        }
        for t in ["", "ISBN", "List<Date>"] {
            for m in mults {
                let a = crate::model::ConceptAttribute { name: "isbn".into(), ty: (!t.is_empty()).then(|| t.to_string()), multiplicity: m.map(str::to_string) };
                assert_eq!(parse_concept_attribute(&format_concept_attribute(&a)), Some(a));
            }
        }
        assert!(n > 1000);
    }

    #[test]
    fn members_parse_tolerantly() {
        let a = parse_attribute("- items : Map<String, Line> [0..*]").unwrap();
        assert_eq!((a.name.as_str(), a.ty.as_str(), a.multiplicity.as_deref(), a.visibility), ("items", "Map<String, Line>", Some("0..*"), Visibility::Private));
        let a = parse_attribute("count").unwrap();
        assert_eq!((a.name.as_str(), a.ty.as_str(), a.visibility), ("count", "", Visibility::Public));
        let m = parse_method("# {static} of(map: Map<K, V>, n: int) : Order").unwrap();
        assert_eq!(m.name, "of");
        assert_eq!(m.params.len(), 2);
        assert_eq!(m.params[0].ty, "Map<K, V>");
        assert_eq!(m.returns.as_deref(), Some("Order"));
        assert!(m.is_static && m.visibility == Visibility::Protected);
        assert!(parse_method("not an operation").is_none());
    }
}
