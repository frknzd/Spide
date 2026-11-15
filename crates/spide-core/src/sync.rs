//! Design ↔ code sync for a Spide Project.
//!
//! Both sides are reduced to the same comparable **shape** (classes, fields, methods, extends/implements).
//! Identity is exact: a design class is the Java type with its explicit link (`class.source.symbol`) or the
//! fully qualified name `code_root.package.Name`; same simple names in other packages never match.
//! A committed **baseline** (the shapes when both sides last agreed) tells which side changed: design
//! ahead (implement it), code ahead (readjust the design), both (conflict), or a difference the user
//! accepted.

use crate::model::*;
use crate::ops::Op;
use crate::project::{Compare, Manifest};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

/// Item id → normalised value. Ids: `class:Foo`, `field:Foo.x`, `method:Foo.bar(int)`,
/// `extends:Foo>Bar`, `implements:Foo>Bar`.
pub type Shape = BTreeMap<String, Value>;

/// `spide/sync.json`: both shapes at the last agreement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Baseline {
    pub version: u32,
    /// ms since epoch
    pub updated_at: u64,
    pub design: Shape,
    pub code: Shape,
}

impl Baseline {
    pub fn parse(text: &str) -> Result<Baseline, String> {
        serde_json::from_str(text).map_err(|e| format!("spide/sync.json is invalid ({e}); treated as no baseline"))
    }
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("baseline serialises") + "\n"
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum SyncStatus {
    InSync,
    /// differs, but the user accepted it (Mark in sync)
    Accepted,
    /// differs and there is no baseline yet
    Unreconciled,
    /// the design changed: implement in the code
    DesignAhead,
    /// the code changed: readjust the design
    CodeAhead,
    /// both changed
    Conflict,
}

impl SyncStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            SyncStatus::InSync => "in_sync",
            SyncStatus::Accepted => "accepted",
            SyncStatus::Unreconciled => "unreconciled",
            SyncStatus::DesignAhead => "design_ahead",
            SyncStatus::CodeAhead => "code_ahead",
            SyncStatus::Conflict => "conflict",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            SyncStatus::InSync => "in sync",
            SyncStatus::Accepted => "accepted difference",
            SyncStatus::Unreconciled => "unreconciled",
            SyncStatus::DesignAhead => "design ahead",
            SyncStatus::CodeAhead => "code ahead",
            SyncStatus::Conflict => "conflict",
        }
    }
    pub fn parse(s: &str) -> Option<SyncStatus> {
        [SyncStatus::InSync, SyncStatus::Accepted, SyncStatus::Unreconciled, SyncStatus::DesignAhead, SyncStatus::CodeAhead, SyncStatus::Conflict].into_iter().find(|x| x.as_str() == s)
    }
    /// Worst first: conflict > design/code ahead > unreconciled > accepted > in sync.
    pub fn rank(self) -> u8 {
        match self {
            SyncStatus::InSync => 0,
            SyncStatus::Accepted => 1,
            SyncStatus::Unreconciled => 2,
            SyncStatus::DesignAhead | SyncStatus::CodeAhead => 3,
            SyncStatus::Conflict => 4,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncItem {
    pub id: String,
    /// class name (the design's name, or the code's name for code-only classes)
    pub class: String,
    /// class | field | method | extends | implements
    pub kind: String,
    pub member: Option<String>,
    /// the design's member name (rows on the canvas)
    pub design_name: Option<String>,
    pub status: SyncStatus,
    pub design: Option<Value>,
    pub code: Option<Value>,
    pub baseline_design: Option<Value>,
    pub baseline_code: Option<Value>,
    /// where the code is (project-relative), for code-side items
    pub file: Option<String>,
    pub line: Option<u32>,
    /// ops that bring the code's version into the design (readjust)
    pub suggested_ops: Vec<Op>,
    /// accepted decisions since the baseline that touched this class
    pub decisions: Vec<String>,
    /// a design-only class and a code-only class that look like the same thing renamed
    pub rename_of: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct ClassSync {
    pub status: Option<SyncStatus>,
    pub differences: usize,
    pub symbol: Option<String>,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub in_design: bool,
    pub in_code: bool,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct SyncReport {
    /// every item that is not in sync
    pub items: Vec<SyncItem>,
    pub classes: BTreeMap<String, ClassSync>,
    /// per status (`in_sync`, `design_ahead`, …)
    pub counts: BTreeMap<String, usize>,
    /// computed from a partial or failed index: don't trust absences
    pub partial: bool,
    pub has_baseline: bool,
}

impl SyncReport {
    pub fn count(&self, s: SyncStatus) -> usize {
        self.counts.get(s.as_str()).copied().unwrap_or(0)
    }
    pub fn class_status(&self, class: &str) -> Option<SyncStatus> {
        self.classes.get(class).and_then(|c| c.status)
    }
}

/// Everything sync knows: both shapes, the report and how design elements map onto the code.
#[derive(Debug, Clone, Default)]
pub struct Analysis {
    pub design: Shape,
    pub code: Shape,
    pub report: SyncReport,
    /// shape class name → code class name in the index
    pub code_class: BTreeMap<String, String>,
    /// item id → design member name
    pub design_member: BTreeMap<String, String>,
    /// item id → code member name (in `code_class`)
    pub code_member: BTreeMap<String, String>,
}

// ---------------------------------------------------------------- normalisation

fn norm_type(t: &str) -> Option<String> {
    let t: String = t.chars().filter(|c| !c.is_whitespace()).collect();
    (!t.is_empty() && t != "void").then_some(t)
}

/// `java.util.List<org.x.Book>` → `List`; `String...` → `String[]`.
pub fn simple_type(t: &str) -> String {
    let mut depth = 0;
    let mut out = String::new();
    for ch in t.chars() {
        match ch {
            '<' => depth += 1,
            '>' => depth -= 1,
            c if depth == 0 && !c.is_whitespace() => out.push(c),
            _ => {}
        }
    }
    let out = out.replace("...", "[]");
    let (base, arr) = match out.find('[') {
        Some(i) => (out[..i].to_string(), out[i..].to_string()),
        None => (out.clone(), String::new()),
    };
    format!("{}{arr}", base.rsplit('.').next().unwrap_or(&base))
}

fn base_name(m: &str) -> &str {
    m.split('(').next().unwrap_or(m)
}

/// `name(T1,T2)` with simple parameter types (`None` when a parameter is untyped).
fn method_key(m: &Method) -> Option<String> {
    let mut types = vec![];
    for p in &m.params {
        if p.ty.trim().is_empty() {
            return None;
        }
        types.push(simple_type(&p.ty));
    }
    Some(format!("{}({})", base_name(&m.name), types.join(",")))
}

/// The key inside an explicit method link `org.x.Foo#bar(int, java.lang.String)`.
fn linked_key(symbol: &str) -> Option<String> {
    let sig = symbol.split_once('#')?.1;
    let (name, params) = sig.split_once('(')?;
    let params = params.trim_end_matches(')');
    let types: Vec<String> = if params.trim().is_empty() { vec![] } else { params.split(',').map(simple_type).collect() };
    Some(format!("{name}({})", types.join(",")))
}

fn kind_of(c: &Class) -> &'static str {
    match c.stereotype.as_deref() {
        Some("interface" | "trait" | "protocol") => "interface",
        Some("module" | "namespace") => "module",
        Some("enum") | Some("enumeration") => "enum",
        _ if c.is_abstract || c.stereotype.as_deref() == Some("abstract") => "abstract",
        _ => "class",
    }
}

fn accessor_name(m: &Method) -> bool {
    let n = base_name(&m.name);
    let after = |p: &str| n.strip_prefix(p).and_then(|r| r.chars().next()).is_some_and(|c| c.is_ascii_uppercase());
    ((after("get") || after("is") || after("has")) && m.params.is_empty()) || (after("set") && m.params.len() == 1)
}

/// A getter/setter whose body is at most one statement on a single field.
fn code_accessor(m: &Method) -> bool {
    accessor_name(m) && m.code.as_ref().is_none_or(|f| f.statements <= 1) && m.uses.len() <= 1
}

fn constructor(class: &str, m: &Method) -> bool {
    let simple = class.rsplit('.').next().unwrap_or(class);
    base_name(&m.name) == simple
}

fn join_fqn(parts: &[Option<&str>]) -> String {
    parts.iter().flatten().filter(|p| !p.is_empty()).copied().collect::<Vec<_>>().join(".")
}

/// The Java type a design class stands for: its explicit link, or `code_root.package.Name`.
pub fn design_fqn(c: &Class, code_root: Option<&str>) -> String {
    match c.source.as_ref().map(|s| s.symbol.as_str()).filter(|s| !s.is_empty()) {
        Some(s) => s.to_string(),
        None => join_fqn(&[code_root, c.package.as_deref(), Some(c.name.as_str())]),
    }
}

/// Wildcard-aware equality: an empty design type/return type matches any code type.
fn same(d: Option<&Value>, c: Option<&Value>) -> bool {
    match (d, c) {
        (None, None) => true,
        (Some(Value::Object(d)), Some(Value::Object(c))) => {
            let keys: BTreeSet<&String> = d.keys().chain(c.keys()).collect();
            keys.into_iter().all(|k| {
                let dv = d.get(k).unwrap_or(&Value::Null);
                (matches!(k.as_str(), "type" | "returns") && dv.is_null()) || Some(dv) == c.get(k).or(Some(&Value::Null))
            })
        }
        (Some(d), Some(c)) => d == c,
        _ => false,
    }
}

fn status_of(d: Option<&Value>, c: Option<&Value>, b: Option<&Baseline>, id: &str) -> SyncStatus {
    if same(d, c) {
        return SyncStatus::InSync;
    }
    let Some(b) = b else { return SyncStatus::Unreconciled };
    let (bd, bc) = (b.design.get(id), b.code.get(id));
    // on both sides but never agreed on (the baseline doesn't know it): not a conflict, just unreconciled
    if bd.is_none() && bc.is_none() && d.is_some() && c.is_some() {
        return SyncStatus::Unreconciled;
    }
    let dc = d != bd;
    let cc = c != bc;
    match (dc, cc) {
        (false, false) => SyncStatus::Accepted,
        (true, false) => SyncStatus::DesignAhead,
        (false, true) => SyncStatus::CodeAhead,
        (true, true) => SyncStatus::Conflict,
    }
}

fn type_names(t: &str) -> Vec<&str> {
    t.split(|c: char| !c.is_alphanumeric() && c != '_').filter(|s| !s.is_empty()).collect()
}

// ---------------------------------------------------------------- analysis

/// Compare the accepted design with the indexed code (`code` = spide-import's design of the module).
pub fn analyse(design: &Design, code: &Design, manifest: &Manifest, baseline: Option<&Baseline>, partial: bool) -> Analysis {
    let cmp: &Compare = &manifest.compare;
    let code_root = manifest.code_root.clone().or(code.settings.code_root.clone());
    let code_root = code_root.as_deref();
    let mut a = Analysis::default();

    // ---- identity
    let dclasses: Vec<&Class> = design.classes.iter().filter(|c| c.status.is_live() && !design.is_external(&c.name)).collect();
    let all_design_names: BTreeSet<&str> = design.classes.iter().map(|c| c.name.as_str()).collect();
    let cclasses: Vec<&Class> = code.classes.iter().filter(|c| c.status.is_live() && !c.external).collect();
    let fqn_of_code = |c: &Class| c.source.as_ref().map(|s| s.symbol.clone()).unwrap_or_else(|| join_fqn(&[code_root, c.package.as_deref(), Some(c.name.as_str())]));
    let by_fqn: BTreeMap<String, &Class> = cclasses.iter().map(|c| (fqn_of_code(c), *c)).collect();
    // shape name of every code class
    let mut shape_of_code: BTreeMap<String, String> = BTreeMap::new();
    for d in &dclasses {
        if let Some(c) = by_fqn.get(&design_fqn(d, code_root)) {
            shape_of_code.insert(c.name.clone(), d.name.clone());
        }
    }
    for c in &cclasses {
        if !shape_of_code.contains_key(&c.name) {
            // never take a design class's name by accident: same simple name, other package → the FQN
            let n = if all_design_names.contains(c.name.as_str()) { fqn_of_code(c) } else { c.name.clone() };
            shape_of_code.insert(c.name.clone(), n);
        }
    }
    for (code_name, shape) in &shape_of_code {
        a.code_class.insert(shape.clone(), code_name.clone());
    }

    // ---- code shape
    let mut excluded_accessors: BTreeSet<String> = BTreeSet::new();
    // shape class → (key, base name, arity) of every code method (also the filtered ones)
    let mut code_methods: BTreeMap<String, Vec<(String, String, usize)>> = BTreeMap::new();
    let mut code_static_fields: BTreeSet<String> = BTreeSet::new();
    let mut location: BTreeMap<String, (String, u32)> = BTreeMap::new();
    // item id → (kind, class, member)
    let mut meta: BTreeMap<String, (&'static str, String, Option<String>)> = BTreeMap::new();
    for c in &cclasses {
        let n = &shape_of_code[&c.name];
        a.code.insert(format!("class:{n}"), json!({"package": c.package, "kind": kind_of(c)}));
        meta.insert(format!("class:{n}"), ("class", n.clone(), None));
        if let Some(s) = &c.source {
            location.insert(format!("class:{n}"), (s.file.clone(), s.start_line));
        }
        for f in c.attributes.iter().filter(|f| f.status.is_live()) {
            let id = format!("field:{n}.{}", f.name);
            if f.is_static {
                code_static_fields.insert(id.clone());
            }
            a.code.insert(id.clone(), json!({"type": norm_type(&f.ty), "static": f.is_static}));
            meta.insert(id.clone(), ("field", n.clone(), Some(f.name.clone())));
            a.code_member.insert(id.clone(), f.name.clone());
            if let Some(s) = &c.source {
                location.insert(id, (s.file.clone(), s.start_line));
            }
        }
        for m in c.methods.iter().filter(|m| m.status.is_live()) {
            let key = method_key(m).unwrap_or_else(|| format!("{}(?)", base_name(&m.name)));
            code_methods.entry(n.clone()).or_default().push((key.clone(), base_name(&m.name).to_string(), m.params.len()));
            let id = format!("method:{n}.{key}");
            meta.insert(id.clone(), ("method", n.clone(), Some(key.clone())));
            if !cmp.constructors && constructor(&c.name, m) {
                continue;
            }
            if !cmp.private_methods && m.visibility == Visibility::Private {
                continue;
            }
            if !cmp.accessors && code_accessor(m) {
                excluded_accessors.insert(id);
                continue;
            }
            a.code.insert(id.clone(), json!({"returns": m.returns.as_deref().and_then(norm_type), "static": m.is_static, "abstract": m.is_abstract}));
            a.code_member.insert(id.clone(), m.name.clone());
            if let Some(s) = &m.source {
                location.insert(id, (s.file.clone(), s.start_line));
            }
        }
    }
    for r in code.relations.iter().filter(|r| r.status.is_live()) {
        let (Some(f), Some(t)) = (shape_of_code.get(&r.from), shape_of_code.get(&r.to)) else { continue };
        let kind = match r.kind {
            RelationKind::Inheritance => "extends",
            RelationKind::Realization => "implements",
            _ => continue,
        };
        a.code.insert(format!("{kind}:{f}>{t}"), json!({}));
        meta.insert(format!("{kind}:{f}>{t}"), (kind, f.clone(), Some(t.clone())));
    }

    // ---- design shape
    for c in &dclasses {
        let n = &c.name;
        a.design.insert(format!("class:{n}"), json!({"package": c.package, "kind": kind_of(c)}));
        meta.insert(format!("class:{n}"), ("class", n.clone(), None));
        for f in c.attributes.iter().filter(|f| f.status.is_live()) {
            let id = format!("field:{n}.{}", f.name);
            meta.insert(id.clone(), ("field", n.clone(), Some(f.name.clone())));
            a.design.insert(id.clone(), json!({"type": norm_type(&f.ty), "static": f.is_static}));
            a.design_member.insert(id, f.name.clone());
        }
        let candidates = code_methods.get(n).cloned().unwrap_or_default();
        for m in c.methods.iter().filter(|m| m.status.is_live()) {
            if !cmp.constructors && constructor(n, m) {
                continue;
            }
            if !cmp.private_methods && m.visibility == Visibility::Private {
                continue;
            }
            let explicit = m.source.as_ref().and_then(|s| linked_key(&s.symbol));
            let key = explicit.or_else(|| method_key(m)).or_else(|| {
                let same_name: Vec<&(String, String, usize)> = candidates.iter().filter(|(_, b, ar)| b == base_name(&m.name) && *ar == m.params.len()).collect();
                (same_name.len() == 1).then(|| same_name[0].0.clone())
            });
            let key = key.unwrap_or_else(|| format!("{}({})", base_name(&m.name), vec!["_"; m.params.len()].join(",")));
            let id = format!("method:{n}.{key}");
            let has_code = candidates.iter().any(|(k, ..)| *k == key);
            if !cmp.accessors && (excluded_accessors.contains(&id) || (!has_code && accessor_name(m))) {
                continue;
            }
            a.design.insert(id.clone(), json!({"returns": m.returns.as_deref().and_then(norm_type), "static": m.is_static, "abstract": m.is_abstract}));
            meta.insert(id.clone(), ("method", n.clone(), Some(key.clone())));
            a.design_member.insert(id, m.name.clone());
        }
    }
    let shown: BTreeSet<&str> = dclasses.iter().map(|c| c.name.as_str()).collect();
    for r in design.live_relations() {
        if !shown.contains(r.from.as_str()) || !shown.contains(r.to.as_str()) {
            continue;
        }
        let kind = match r.kind {
            RelationKind::Inheritance => "extends",
            RelationKind::Realization => "implements",
            _ => continue,
        };
        a.design.insert(format!("{kind}:{}>{}", r.from, r.to), json!({}));
        meta.insert(format!("{kind}:{}>{}", r.from, r.to), (kind, r.from.clone(), Some(r.to.clone())));
    }
    // a code field the design draws as an association counts as present in the design;
    // static code fields (constants, loggers) only count when the design has them
    let code_fields: Vec<(String, Value)> = a.code.iter().filter(|(k, _)| k.starts_with("field:")).map(|(k, v)| (k.clone(), v.clone())).collect();
    for (id, v) in code_fields {
        if a.design.contains_key(&id) {
            continue;
        }
        let class = meta[&id].1.clone();
        let ty = v.get("type").and_then(Value::as_str).unwrap_or("");
        let drawn = design.live_relations().any(|r| {
            r.from == class && matches!(r.kind, RelationKind::Association | RelationKind::Aggregation | RelationKind::Composition) && type_names(ty).contains(&r.to.as_str())
        });
        if drawn {
            a.design.insert(id.clone(), v.clone());
        } else if code_static_fields.contains(&id) {
            a.code.remove(&id);
        }
    }

    // ---- statuses
    let ids: BTreeSet<String> = a.design.keys().chain(a.code.keys()).cloned().collect();
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut items = vec![];
    for id in &ids {
        let (d, c) = (a.design.get(id), a.code.get(id));
        let st = status_of(d, c, baseline, id);
        *counts.entry(st.as_str().to_string()).or_default() += 1;
        let (kind, class, member) = meta[id].clone();
        let loc = location.get(id).or_else(|| location.get(&format!("class:{class}")));
        items.push(SyncItem {
            id: id.clone(),
            class,
            kind: kind.to_string(),
            member,
            design_name: a.design_member.get(id).cloned(),
            status: st,
            design: d.cloned(),
            code: c.cloned(),
            baseline_design: baseline.and_then(|b| b.design.get(id).cloned()),
            baseline_code: baseline.and_then(|b| b.code.get(id).cloned()),
            file: c.and(loc.map(|l| l.0.clone())),
            line: c.and(loc.map(|l| l.1)),
            suggested_ops: vec![],
            decisions: vec![],
            rename_of: None,
        });
    }

    // ---- per class
    for it in &items {
        let cs = a.report.classes.entry(it.class.clone()).or_default();
        if it.status != SyncStatus::InSync {
            cs.differences += 1;
        }
        if cs.status.is_none_or(|s| it.status.rank() > s.rank()) {
            cs.status = Some(it.status);
        }
    }
    for (name, cs) in a.report.classes.iter_mut() {
        cs.in_design = a.design.contains_key(&format!("class:{name}"));
        cs.in_code = a.code.contains_key(&format!("class:{name}"));
        if let Some(cc) = a.code_class.get(name).and_then(|n| code.class(n)) {
            cs.symbol = cc.source.as_ref().map(|s| s.symbol.clone());
            cs.file = cc.source.as_ref().map(|s| s.file.clone());
            cs.line = cc.source.as_ref().map(|s| s.start_line);
        }
    }

    // ---- suggestions, decisions, rename hints
    let since = baseline.map(|b| b.updated_at).unwrap_or(0);
    let touched: Vec<(String, BTreeSet<String>)> = design
        .decisions
        .iter()
        .filter(|d| matches!(d.state, DecisionState::Accepted | DecisionState::Overruled) && d.decided_at.unwrap_or(0) > since)
        .map(|d| (d.id.clone(), d.applied_ops.iter().flat_map(Op::touched_classes).collect()))
        .collect();
    let members_of = |shape: &Shape, class: &str| -> BTreeSet<String> {
        shape
            .keys()
            .filter_map(|k| {
                let (kind, rest) = k.split_once(':')?;
                (matches!(kind, "field" | "method") && rest.starts_with(&format!("{class}."))).then(|| rest[class.len() + 1..].to_string())
            })
            .collect()
    };
    let design_only: Vec<String> = a.report.classes.iter().filter(|(_, c)| c.in_design && !c.in_code).map(|(n, _)| n.clone()).collect();
    let code_only: Vec<String> = a.report.classes.iter().filter(|(_, c)| !c.in_design && c.in_code).map(|(n, _)| n.clone()).collect();
    let mut renames: BTreeMap<String, String> = BTreeMap::new();
    let simple = |n: &str| n.rsplit('.').next().unwrap_or(n).to_string();
    for dn in &design_only {
        // the same class name in another package: probably the same class, placed differently
        if let Some(cn) = code_only.iter().find(|cn| simple(cn) == *dn) {
            renames.insert(dn.clone(), cn.clone());
            renames.insert(cn.clone(), dn.clone());
            continue;
        }
        let dm = members_of(&a.design, dn);
        let dp = a.design.get(&format!("class:{dn}")).and_then(|v| v.get("package").cloned());
        for cn in &code_only {
            if a.code.get(&format!("class:{cn}")).and_then(|v| v.get("package").cloned()) != dp {
                continue;
            }
            let cm = members_of(&a.code, cn);
            let common = dm.intersection(&cm).count();
            let size = dm.len().max(cm.len());
            if size > 0 && common as f64 / size as f64 >= 0.6 {
                renames.insert(dn.clone(), cn.clone());
                renames.insert(cn.clone(), dn.clone());
            }
        }
    }
    for it in items.iter_mut() {
        it.decisions = touched.iter().filter(|(_, cls)| cls.contains(&it.class)).map(|(id, _)| id.clone()).collect();
        if it.kind == "class" {
            it.rename_of = renames.get(&it.class).cloned();
        }
        if it.status != SyncStatus::InSync {
            it.suggested_ops = suggest(it, design, code, &a, code_root);
            // a code-only class with a design class of the same name elsewhere: link them (or move the design class)
            if let (true, None, Some(dn)) = (it.kind == "class", &it.design, it.rename_of.as_ref().filter(|dn| simple(&it.class) == **dn)) {
                if let Some(sym) = a.code_class.get(&it.class).and_then(|n| code.class(n)).and_then(|c| c.source.as_ref()).map(|s| s.symbol.clone()) {
                    it.suggested_ops = vec![Op::LinkCode { class: dn.clone(), method: None, symbol: sym, language: None }];
                }
            }
        }
    }
    a.report.counts = counts;
    a.report.partial = partial;
    a.report.has_baseline = baseline.is_some();
    a.report.items = items.into_iter().filter(|i| i.status != SyncStatus::InSync).collect();
    a
}

/// Ops that make the design say what the code says, keeping the design's intent (responsibilities,
/// descriptions, rationale).
fn suggest(it: &SyncItem, design: &Design, code: &Design, a: &Analysis, code_root: Option<&str>) -> Vec<Op> {
    let class = it.class.as_str();
    let cc = a.code_class.get(class).and_then(|n| code.class(n));
    let dc = design.class(class).filter(|c| c.status.is_live());
    let strip_method = |m: &Method| Method { code: None, source: None, status: Status::Accepted, origin: None, proposal: None, ..m.clone() };
    let strip_attr = |x: &Attribute| Attribute { status: Status::Accepted, origin: None, proposal: None, ..x.clone() };
    let stereotype = |c: &Class| -> Option<String> {
        match kind_of(c) {
            "class" => c.stereotype.clone().filter(|s| s == "value"),
            k => Some(k.to_string()),
        }
    };
    match (it.kind.as_str(), &it.design, &it.code) {
        ("class", None, Some(_)) => {
            let Some(cc) = cc else { return vec![] };
            let keep = |id: String| a.code.contains_key(&id);
            let mut ops = vec![Op::AddClass {
                name: class.to_string(),
                stereotype: stereotype(cc),
                package: cc.package.clone(),
                is_abstract: cc.is_abstract,
                attributes: cc.attributes.iter().filter(|f| keep(format!("field:{class}.{}", f.name))).map(strip_attr).collect(),
                methods: cc
                    .methods
                    .iter()
                    .filter(|m| method_key(m).is_some_and(|k| keep(format!("method:{class}.{k}"))))
                    .map(strip_method)
                    .collect(),
                responsibilities: vec![],
                rationale: Some("found in the code".into()),
                aggregate_root: false,
                external: false,
                role: None,
            }];
            let symbol = cc.source.as_ref().map(|s| s.symbol.clone()).unwrap_or_default();
            let implicit = join_fqn(&[code_root, cc.package.as_deref(), Some(class)]);
            if !symbol.is_empty() && symbol != implicit {
                ops.push(Op::LinkCode { class: class.to_string(), method: None, symbol, language: None });
            }
            ops
        }
        ("class", Some(_), None) => vec![Op::RemoveClass { name: class.to_string() }],
        ("class", Some(d), Some(c)) => {
            let (Some(dc), Some(cc)) = (dc, cc) else { return vec![] };
            let mut ops = vec![];
            if d.get("package") != c.get("package") {
                ops.push(Op::MoveToPackage { class: class.to_string(), package: cc.package.clone() });
            }
            if d.get("kind") != c.get("kind") {
                ops.push(Op::UpdateClass {
                    name: class.to_string(),
                    stereotype: Some(stereotype(cc).unwrap_or_default()),
                    package: None,
                    is_abstract: Some(cc.is_abstract),
                    responsibilities: None,
                    rationale: None,
                    aggregate_root: None,
                    external: None,
                    role: None,
                });
            }
            let _ = dc;
            ops
        }
        ("field", _, _) => {
            let dname = a.design_member.get(&it.id);
            let cname = a.code_member.get(&it.id);
            let cf = cc.and_then(|c| cname.and_then(|n| c.attributes.iter().find(|f| &f.name == n)));
            let df = dc.and_then(|c| dname.and_then(|n| c.attributes.iter().find(|f| &f.name == n)));
            match (df, cf) {
                (None, Some(cf)) if dc.is_some() => vec![Op::AddAttribute { class: class.to_string(), attribute: strip_attr(cf) }],
                (Some(df), None) => vec![Op::RemoveAttribute { class: class.to_string(), name: df.name.clone() }],
                (Some(df), Some(cf)) => vec![Op::UpdateAttribute { class: class.to_string(), name: df.name.clone(), attribute: Attribute { ty: cf.ty.clone(), is_static: cf.is_static, ..strip_attr(df) } }],
                _ => vec![],
            }
        }
        ("method", _, _) => {
            let dname = a.design_member.get(&it.id);
            let cname = a.code_member.get(&it.id);
            let cm = cc.and_then(|c| cname.and_then(|n| c.methods.iter().find(|m| &m.name == n)));
            let dm = dc.and_then(|c| dname.and_then(|n| c.methods.iter().find(|m| &m.name == n)));
            match (dm, cm) {
                (None, Some(cm)) if dc.is_some() => vec![Op::AddMethod { class: class.to_string(), method: strip_method(cm) }],
                (Some(dm), None) => vec![Op::RemoveMethod { class: class.to_string(), name: dm.name.clone() }],
                (Some(dm), Some(cm)) => vec![Op::UpdateMethod {
                    class: class.to_string(),
                    name: dm.name.clone(),
                    method: Method {
                        params: cm.params.clone(),
                        returns: cm.returns.clone().filter(|r| r != "void"),
                        is_static: cm.is_static,
                        is_abstract: cm.is_abstract,
                        ..strip_method(dm)
                    },
                }],
                _ => vec![],
            }
        }
        (k @ ("extends" | "implements"), d, c) => {
            let Some(to) = it.member.clone() else { return vec![] };
            let kind = if k == "extends" { RelationKind::Inheritance } else { RelationKind::Realization };
            match (d, c) {
                (None, Some(_)) => vec![Op::AddRelation { from: class.to_string(), to, kind, label: None, from_mult: None, to_mult: None, directed: None, rationale: Some("found in the code".into()), interaction: None }],
                (Some(_), None) => vec![Op::RemoveRelation { from: class.to_string(), to, kind: Some(kind) }],
                _ => vec![],
            }
        }
        _ => vec![],
    }
}

// ---------------------------------------------------------------- baseline

/// In-sync items whose baseline entries lag behind move the baseline along. `None`: nothing to do
/// (also never from a partial index). Without a baseline, the first in-sync items create one.
pub fn advance(baseline: Option<&Baseline>, a: &Analysis, now: u64) -> Option<Baseline> {
    if a.report.partial {
        return None;
    }
    let mut b = baseline.cloned().unwrap_or(Baseline { version: 1, ..Default::default() });
    let mut changed = baseline.is_none();
    let ids: BTreeSet<String> = a.design.keys().chain(a.code.keys()).chain(b.design.keys()).chain(b.code.keys()).cloned().collect();
    let mut any_in_sync = false;
    for id in &ids {
        let (d, c) = (a.design.get(id), a.code.get(id));
        if !same(d, c) {
            continue;
        }
        any_in_sync = true;
        for (side, v) in [(&mut b.design, d), (&mut b.code, c)] {
            if side.get(id) != v {
                changed = true;
                match v {
                    Some(v) => side.insert(id.clone(), v.clone()),
                    None => side.remove(id),
                };
            }
        }
    }
    if baseline.is_none() && !any_in_sync {
        return None;
    }
    changed.then(|| {
        b.updated_at = now;
        b
    })
}

/// The user's "Mark in sync": both current shapes become the baseline; what still differs is accepted.
pub fn mark_synced(a: &Analysis, now: u64) -> Baseline {
    Baseline { version: 1, updated_at: now, design: a.design.clone(), code: a.code.clone() }
}

// ---------------------------------------------------------------- code facts

/// The design with the implementation facts (`CodeFacts`) of its linked methods, so long methods,
/// clones and the code-based course checks show up. Never persisted.
pub fn with_code_facts(design: &Design, code: &Design, a: &Analysis) -> Design {
    let mut d = design.clone();
    for c in d.classes.iter_mut() {
        let Some(cc) = a.code_class.get(&c.name).and_then(|n| code.class(n)) else { continue };
        let ids: Vec<(String, String)> = a.design_member.iter().filter(|(id, _)| id.starts_with(&format!("method:{}.", c.name))).map(|(id, m)| (id.clone(), m.clone())).collect();
        for (id, dname) in ids {
            let Some(cname) = a.code_member.get(&id) else { continue };
            let Some(cm) = cc.methods.iter().find(|m| &m.name == cname) else { continue };
            if let Some(m) = c.methods.iter_mut().find(|m| m.name == dname) {
                m.code = cm.code.clone();
            }
        }
    }
    d
}

// ---------------------------------------------------------------- implement brief

fn method_text(m: &Method) -> String {
    let params: Vec<String> = m.params.iter().map(|p| if p.ty.is_empty() { p.name.clone() } else { format!("{}: {}", p.name, p.ty) }).collect();
    let r = m.returns.as_deref().filter(|r| !r.is_empty()).map(|r| format!(": {r}")).unwrap_or_default();
    format!("{}({}){r}", base_name(&m.name), params.join(", "))
}

/// What to implement for these classes (empty = everything design-ahead, or unreconciled where the
/// design has something; conflicts only with `keep_design`), as instructions for the agent.
pub fn implement_brief(a: &Analysis, design: &Design, code: &Design, manifest: &Manifest, scope: &[String], keep_design: bool) -> String {
    let code_root = manifest.code_root.clone().or(code.settings.code_root.clone());
    let wanted = |it: &SyncItem| {
        (scope.is_empty() || scope.contains(&it.class))
            && match it.status {
                SyncStatus::DesignAhead => true,
                // without a baseline, only what the design has is implemented (code-only stays for readjust)
                SyncStatus::Unreconciled => it.design.is_some(),
                SyncStatus::Conflict => keep_design,
                _ => false,
            }
    };
    let mut by_class: BTreeMap<&str, Vec<&SyncItem>> = BTreeMap::new();
    for it in a.report.items.iter().filter(|i| wanted(i)) {
        by_class.entry(it.class.as_str()).or_default().push(it);
    }
    if by_class.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for (class, items) in &by_class {
        let dc = design.class(class).filter(|c| c.status.is_live());
        let cs = a.report.classes.get(*class);
        let file = cs.and_then(|c| c.file.clone()).unwrap_or_else(|| {
            let fqn = dc.map(|c| design_fqn(c, code_root.as_deref())).unwrap_or_else(|| class.to_string());
            format!("{}/{}.java", manifest.roots.first().map(String::as_str).unwrap_or("src/main/java"), fqn.replace('.', "/"))
        });
        out.push_str(&format!("\n## {class} — {file}\n"));
        for it in items {
            let m_design = a.design_member.get(&it.id).and_then(|n| dc.and_then(|c| c.methods.iter().find(|m| &m.name == n)));
            let a_design = a.design_member.get(&it.id).and_then(|n| dc.and_then(|c| c.attributes.iter().find(|x| &x.name == n)));
            let line = match (it.kind.as_str(), &it.design, &it.code) {
                ("class", Some(d), None) => format!(
                    "create the {} `{class}`{}",
                    d.get("kind").and_then(Value::as_str).unwrap_or("class"),
                    d.get("package").and_then(Value::as_str).map(|p| format!(" in package `{p}`")).unwrap_or_default()
                ),
                ("class", None, Some(_)) => format!("remove the class `{class}` (not in the design) — ask the user before deleting the file"),
                ("class", Some(d), Some(c)) => {
                    let mut v = vec![];
                    if d.get("package") != c.get("package") {
                        v.push(format!("move it to package `{}`", d.get("package").and_then(Value::as_str).unwrap_or("(root)")));
                    }
                    if d.get("kind") != c.get("kind") {
                        v.push(format!("make it a{} {}", if d.get("kind").and_then(Value::as_str) == Some("interface") || d.get("kind").and_then(Value::as_str) == Some("abstract") { "n" } else { "" }, d.get("kind").and_then(Value::as_str).unwrap_or("class")));
                    }
                    v.join("; ")
                }
                ("field", Some(_), None) => format!("add field `{}`", a_design.map(|x| format!("{}: {}", x.name, x.ty)).unwrap_or_else(|| it.member.clone().unwrap_or_default())),
                ("field", None, Some(_)) => format!("remove field `{}`", it.member.clone().unwrap_or_default()),
                ("field", Some(_), Some(_)) => format!("change field `{}`", a_design.map(|x| format!("{}: {}{}", x.name, x.ty, if x.is_static { " (static)" } else { "" })).unwrap_or_default()),
                ("method", Some(_), None) => format!(
                    "add method `{}`{}",
                    m_design.map(method_text).unwrap_or_else(|| it.member.clone().unwrap_or_default()),
                    m_design.and_then(|m| m.description.as_ref()).map(|d| format!(" — {d}")).unwrap_or_default()
                ),
                ("method", None, Some(_)) => format!("remove method `{}` (no longer in the design)", it.member.clone().unwrap_or_default()),
                ("method", Some(_), Some(_)) => format!("change `{}` to `{}`", it.member.clone().unwrap_or_default(), m_design.map(method_text).unwrap_or_default()),
                ("extends", Some(_), None) => format!("`{class}` extends `{}`", it.member.clone().unwrap_or_default()),
                ("extends", None, Some(_)) => format!("`{class}` no longer extends `{}`", it.member.clone().unwrap_or_default()),
                ("implements", Some(_), None) => format!("`{class}` implements `{}`", it.member.clone().unwrap_or_default()),
                ("implements", None, Some(_)) => format!("`{class}` no longer implements `{}`", it.member.clone().unwrap_or_default()),
                _ => continue,
            };
            if !line.is_empty() {
                let tag = if it.status == SyncStatus::Conflict { " (conflict: keep the design's version)" } else { "" };
                out.push_str(&format!("- {line}{tag}\n"));
            }
        }
        if let Some(dc) = dc {
            if !dc.responsibilities.is_empty() {
                out.push_str(&format!("  Responsibilities: {}\n", dc.responsibilities.join("; ")));
            }
            if let Some(r) = &dc.rationale {
                out.push_str(&format!("  Rationale: {r}\n"));
            }
        }
        let decisions: BTreeSet<&String> = items.iter().flat_map(|i| i.decisions.iter()).collect();
        for id in decisions {
            if let Some(d) = design.decisions.iter().find(|d| &d.id == id) {
                let chosen = d.chosen.as_ref().and_then(|c| d.options.iter().find(|o| &o.id == c)).map(|o| o.label.clone()).or(d.custom.clone()).unwrap_or_default();
                let notes = if d.notes.trim().is_empty() { String::new() } else { format!(" Notes: {}", d.notes.trim()) };
                out.push_str(&format!("  Decision {id}: {} → {chosen}.{notes}\n", d.question));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_types_and_keys() {
        assert_eq!(simple_type("java.util.List<org.x.Book>"), "List");
        assert_eq!(simple_type("String..."), "String[]");
        assert_eq!(simple_type("Map<String, List<Loan>>"), "Map");
        assert_eq!(linked_key("org.x.Foo#bar(int, java.lang.String)").as_deref(), Some("bar(int,String)"));
        assert_eq!(linked_key("org.x.Foo#bar()").as_deref(), Some("bar()"));
    }

    #[test]
    fn wildcards_only_for_types() {
        assert!(same(Some(&json!({"type": null, "static": false})), Some(&json!({"type": "int", "static": false}))));
        assert!(!same(Some(&json!({"type": null, "static": true})), Some(&json!({"type": "int", "static": false}))));
        assert!(!same(Some(&json!({"package": null, "kind": "class"})), Some(&json!({"package": "a", "kind": "class"}))));
    }
}
