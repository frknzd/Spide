//! Responsibility analysis: what a class is for (its role), what each method does (its concern),
//! and whether they fit — RDD (knowing/doing/deciding) and GRASP (Controller, Information Expert,
//! Creator, Pure Fabrication), as taught in the DOSS course.
//!
//! Everything here is a deterministic *signal* computed from names, stereotypes, layers, signatures,
//! `uses` and `calls`. The agent's own judgement (assessments) is layered on top elsewhere.

use crate::detail;
use crate::model::*;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

/// Roles a class can play (its intended purpose).
pub const ROLES: [&str; 10] = ["presentation", "controller", "service", "domain", "persistence", "adapter", "output", "creator", "utility", "value"];

/// Kinds of work a method does.
pub const CONCERNS: [&str; 7] = ["presentation", "coordination", "domain_logic", "persistence", "formatting", "creation", "accessor"];

pub fn check_role(r: &str) -> Result<(), String> {
    if r.is_empty() || ROLES.contains(&r) {
        Ok(())
    } else {
        Err(format!("unknown role {r:?} (use one of {})", ROLES.join("|")))
    }
}

pub fn check_concern(c: &str) -> Result<(), String> {
    if c.is_empty() || CONCERNS.contains(&c) {
        Ok(())
    } else {
        Err(format!("unknown concern {c:?} (use one of {})", CONCERNS.join("|")))
    }
}

// ---------------------------------------------------------------- words

/// camelCase / snake_case / digits → lower-case words (`getHTTPBody2` → get, http, body, 2).
pub fn words(s: &str) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    let mut cur = String::new();
    let cs: Vec<char> = s.chars().collect();
    for (i, &c) in cs.iter().enumerate() {
        if !c.is_alphanumeric() {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            continue;
        }
        let boundary = !cur.is_empty()
            && ((c.is_uppercase() && cs[i - 1].is_lowercase())
                || (c.is_uppercase() && cs.get(i + 1).is_some_and(|n| n.is_lowercase()) && cs[i - 1].is_uppercase())
                || (c.is_ascii_digit() != cs[i - 1].is_ascii_digit()));
        if boundary {
            out.push(std::mem::take(&mut cur));
        }
        cur.extend(c.to_lowercase());
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Very light stemming so `matches`/`matching`/`matched` and `calls`/`call` meet.
pub fn stem(w: &str) -> String {
    let w = w.to_lowercase();
    for suf in ["ing", "ies", "es", "ed", "s"] {
        if w.len() > suf.len() + 2 && w.ends_with(suf) {
            let base = &w[..w.len() - suf.len()];
            return if suf == "ies" { format!("{base}y") } else { base.to_string() };
        }
    }
    w
}

// ---------------------------------------------------------------- roles

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct RoleInfo {
    pub role: String,
    /// explicit | stereotype | name | layer | default
    pub source: String,
    pub evidence: String,
    /// Each signal on its own (for consistency checks): stereotype, name, layer.
    pub by_stereotype: Option<String>,
    pub by_name: Option<String>,
    pub by_layer: Option<String>,
}

const NAME_SUFFIXES: &[(&str, &[&str])] = &[
    ("presentation", &["Panel", "View", "Dialog", "Frame", "Window", "Page", "Screen", "Widget", "TableModel", "Renderer", "Presenter", "Form", "Applet", "Gui", "UI", "Ui"]),
    ("controller", &["Controller", "Handler", "Coordinator", "Session", "Facade"]),
    ("service", &["Service", "UseCase", "Interactor"]),
    ("persistence", &["Repository", "Dao", "DAO", "Store", "Storage"]),
    ("adapter", &["Adapter", "Listener", "Client", "Gateway", "Connector", "Proxy"]),
    ("output", &["Writer", "Exporter", "Formatter", "Serializer", "Printer", "Reporter"]),
    ("creator", &["Factory", "Builder"]),
    ("utility", &["Manager", "Helper", "Util", "Utils", "Utilities", "Processor", "Data", "Info", "Misc"]),
];

fn role_by_name(name: &str) -> Option<(&'static str, String)> {
    let mut best: Option<(&'static str, &str)> = None;
    for (role, sufs) in NAME_SUFFIXES {
        for s in *sufs {
            if name.len() > s.len() && name.ends_with(s) && best.is_none_or(|(_, b)| s.len() > b.len()) {
                best = Some((role, s));
            }
        }
    }
    if let Some((r, s)) = best {
        return Some((r, format!("name ends in {s}")));
    }
    if name.starts_with("Extension") && name.len() > 9 {
        return Some(("adapter", "name starts with Extension (a framework plug-in)".into()));
    }
    None
}

fn role_by_stereotype(st: Option<&str>) -> Option<&'static str> {
    Some(match st? {
        "controller" => "controller",
        "service" => "service",
        "repository" | "dao" => "persistence",
        "entity" | "aggregate" => "domain",
        "value" | "enum" | "enumeration" | "record" => "value",
        "boundary" | "view" | "ui" => "presentation",
        "factory" => "creator",
        "adapter" | "gateway" => "adapter",
        _ => return None,
    })
}

fn role_by_layer(l: Option<Layer>) -> Option<&'static str> {
    Some(match l? {
        Layer::Ui => "presentation",
        Layer::Infrastructure => "adapter",
        Layer::Application => "service",
        Layer::Domain => "domain",
    })
}

fn layer_of(d: &Design, c: &Class) -> Option<Layer> {
    c.package.as_deref().and_then(|p| d.packages.iter().find(|x| x.name == p)).and_then(|p| p.layer)
}

pub fn role_of(d: &Design, c: &Class) -> RoleInfo {
    let by_st = role_by_stereotype(c.stereotype.as_deref());
    let by_name = role_by_name(&c.name);
    let layer = layer_of(d, c);
    let by_layer = role_by_layer(layer);
    let agent = crate::assess::current(d, &c.name);
    let (role, source, evidence) = if let Some(r) = c.role.as_deref().filter(|r| !r.is_empty()) {
        (r.to_string(), "explicit", "set by the agent/user".to_string())
    } else if let Some(r) = agent.and_then(|a| a.role.as_deref()).filter(|r| !r.is_empty()) {
        (r.to_string(), "agent", agent.and_then(|a| a.purpose.clone()).unwrap_or_else(|| "the agent's assessment".into()))
    } else if let Some((r, ev)) = by_name.clone().filter(|(r, _)| *r != "utility" || by_st.is_none()) {
        // the name is the strongest intent signal (except vague names when a stereotype says more)
        (r.to_string(), "name", ev)
    } else if let Some(r) = by_st {
        (r.to_string(), "stereotype", format!("stereotype «{}»", c.stereotype.as_deref().unwrap_or("")))
    } else if let Some(r) = by_layer {
        (r.to_string(), "layer", format!("package {} is in the {} layer", c.package.as_deref().unwrap_or(""), format!("{:?}", layer.unwrap()).to_lowercase()))
    } else {
        ("domain".to_string(), "default", "no name, stereotype or layer signal: assumed a domain concept".to_string())
    };
    RoleInfo {
        role,
        source: source.into(),
        evidence,
        by_stereotype: by_st.map(String::from),
        by_name: by_name.map(|(r, _)| r.to_string()),
        by_layer: by_layer.map(String::from),
    }
}

// ---------------------------------------------------------------- method concerns

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct MethodInfo {
    pub class: String,
    pub method: String,
    pub concern: String,
    /// knowing | doing | deciding (RDD)
    pub rdd: String,
    /// explicit | name | signature | calls | swing_api | default
    pub source: String,
    pub reason: String,
    /// Getter-style calls into other (non-external) classes, per class: what it reads elsewhere.
    pub reads: BTreeMap<String, usize>,
    /// Calls into other classes' non-getter methods, per class.
    pub tells: BTreeMap<String, usize>,
}

const V_PRESENT: &[&str] = &["show", "display", "render", "paint", "draw", "refresh", "redraw", "repaint", "select", "toggle", "confirm", "prompt", "highlight", "scroll", "focus", "popup", "alert", "layout"];
const V_PERSIST: &[&str] = &["save", "persist", "store", "load", "insert", "upsert", "commit", "rollback", "query", "flush"];
const V_FORMAT: &[&str] = &["format", "write", "print", "escape", "serialize", "deserialize", "encode", "decode", "export", "pretty", "wrap", "indent", "quote", "stringify"];
const V_CREATE: &[&str] = &["create", "make", "new", "build", "construct", "of", "from", "parse", "copy", "clone", "spawn", "instantiate"];
const V_LOGIC: &[&str] = &[
    "calculate", "compute", "count", "sum", "total", "match", "validate", "verify", "check", "apply", "filter", "sort", "merge", "aggregate", "price", "score",
    "evaluate", "decide", "resolve", "normalize", "split", "record", "register", "add", "remove", "clear", "same", "start", "stop", "can", "should", "accept",
    "reject", "borrow", "lend", "pay", "book", "reserve", "cancel", "move", "take", "play", "roll", "advance", "find", "lookup", "contain", "estimate", "measure",
    "convert", "transform", "reduce", "group", "partition", "rank", "compare", "detect", "classify", "derive", "increment", "decrement",
];
/// Verbs that compute something (a UI class doing these over other classes' data is doing their work).
const V_COMPUTE: &[&str] = &[
    "calculate", "compute", "count", "sum", "total", "match", "filter", "sort", "aggregate", "validate", "verify", "price", "score", "evaluate", "rank",
    "estimate", "measure", "group", "reduce", "derive", "classify", "detect",
];
/// Verbs that manage the objects a class knows (its own collections).
const V_COLLECTION: &[&str] = &["add", "remove", "register", "unregister", "put", "clear", "attach", "detach", "subscribe", "unsubscribe"];
const V_COORD: &[&str] = &["handle", "process", "dispatch", "notify", "execute", "run", "hook", "init", "initialize", "wire", "delegate", "fire", "publish", "emit", "trigger", "invoke", "schedule", "submit"];
/// Methods every Java object has: not judged.
const OBJECT_METHODS: &[&str] = &["toString", "equals", "hashCode", "compareTo", "clone", "finalize"];
/// Swing/AWT callbacks a UI class must implement.
const SWING_API: &[&str] = &[
    "getRowCount", "getColumnCount", "getColumnName", "getColumnClass", "getValueAt", "setValueAt", "isCellEditable", "paintComponent", "actionPerformed",
    "valueChanged", "stateChanged", "mouseClicked", "keyPressed", "getPreferredSize", "getListCellRendererComponent", "getTableCellRendererComponent",
];
const NUMERIC: &[&str] = &["int", "long", "double", "float", "short", "Integer", "Long", "Double", "Float", "BigDecimal", "BigInteger", "Money", "Number", "Duration"];

fn is_numeric_or_bool(ty: Option<&str>) -> bool {
    ty.is_some_and(|t| NUMERIC.contains(&t) || t == "boolean" || t == "Boolean")
}

fn first_verb(name: &str) -> String {
    words(name).first().map(|w| stem(w)).unwrap_or_default()
}

fn in_list(v: &str, list: &[&str]) -> bool {
    list.iter().any(|x| stem(x) == v || *x == v)
}

/// Getter-style method name (`getX`, `isX`, `hasX`, or a noun-like name with no verb).
fn getter_like(m: &str) -> bool {
    let w = words(m);
    matches!(w.first().map(String::as_str), Some("get" | "is" | "has" | "size" | "length" | "count" | "part" | "to"))
}

pub fn classify(d: &Design, c: &Class, m: &Method, role: &str) -> MethodInfo {
    let mut reads: BTreeMap<String, usize> = BTreeMap::new();
    let mut tells: BTreeMap<String, usize> = BTreeMap::new();
    for call in &m.calls {
        if let Some((cls, meth)) = call.split_once('.') {
            if cls == c.name || cls == "this" || cls == "self" || d.is_external(cls) || d.class(cls).is_none() {
                continue;
            }
            if getter_like(meth) {
                *reads.entry(cls.to_string()).or_default() += 1;
            } else {
                *tells.entry(cls.to_string()).or_default() += 1;
            }
        }
    }
    let base = |concern: &str, source: &str, reason: String| MethodInfo {
        class: c.name.clone(),
        method: m.name.clone(),
        concern: concern.to_string(),
        rdd: String::new(),
        source: source.to_string(),
        reason,
        reads: reads.clone(),
        tells: tells.clone(),
    };
    let verb = first_verb(&m.name);
    let returns = m.returns.as_deref();
    let other_reads: usize = reads.values().sum();
    let other_tells: usize = tells.values().sum();
    let own_calls = m.calls.iter().filter(|x| !x.contains('.') || x.starts_with("this.")).count();
    let agent = crate::assess::current(d, &c.name).and_then(|a| a.methods.iter().find(|x| x.name == m.name)).filter(|x| x.concern.is_some());
    let mut info = if let Some(cn) = m.concern.as_deref().filter(|x| !x.is_empty()) {
        base(cn, "explicit", "set by the agent/user".into())
    } else if let Some(a) = agent {
        base(a.concern.as_deref().unwrap_or(""), "agent", a.reason.clone())
    } else if detail::is_constructor(&c.name, m) {
        base("creation", "name", "constructor".into())
    } else if OBJECT_METHODS.contains(&m.name.as_str()) {
        base("accessor", "name", format!("{}() is an Object method", m.name))
    } else if SWING_API.contains(&m.name.as_str()) {
        base("presentation", "swing_api", format!("{}() is a Swing callback", m.name))
    } else if detail::is_accessor(&c.name, m) && other_reads <= 1 {
        base("accessor", "name", "plain getter/setter".into())
    } else if m.uses.is_empty() && m.calls.is_empty() && m.params.is_empty() {
        base("accessor", "signature", "touches no data (a constant or a framework callback)".into())
    } else if m.name.len() > 2 && m.name.starts_with("on") && m.name[2..].starts_with(|ch: char| ch.is_uppercase()) {
        // an event handler: coordination when it hands the work to others, presentation otherwise
        if other_tells > 0 || role != "presentation" {
            base("coordination", "name", "event handler".into())
        } else {
            base("presentation", "name", "event handler".into())
        }
    } else if in_list(&verb, V_COLLECTION) && !m.uses.is_empty() && other_tells <= 1 && other_reads <= 1 {
        base("accessor", "name", format!("manages the objects it knows (“{verb}” on its own data)"))
    } else if role == "presentation" && other_reads == 0 && !in_list(&verb, V_COMPUTE) {
        base("presentation", "default", "works on its own (view) data only".into())
    } else if role == "presentation" && other_reads >= 2 && (is_numeric_or_bool(returns) || in_list(&verb, V_COMPUTE)) {
        // a UI method that derives a value from other classes' data is doing their work
        base("domain_logic", "calls", format!("derives {} from {} getter call(s) on {}", returns.unwrap_or("a result"), other_reads, reads.keys().cloned().collect::<Vec<_>>().join(", ")))
    } else if in_list(&verb, V_PRESENT) {
        base("presentation", "name", format!("verb “{verb}”"))
    } else if in_list(&verb, V_PERSIST) {
        base("persistence", "name", format!("verb “{verb}”"))
    } else if verb == "to" && m.name.len() > 2 {
        let builds = returns.is_some_and(|r| detail::type_names(r).any(|t| d.class(t).is_some()));
        if builds {
            base("creation", "name", format!("converts into {}", returns.unwrap_or("")))
        } else {
            base("formatting", "name", "converts to a representation".into())
        }
    } else if in_list(&verb, V_FORMAT) {
        base("formatting", "name", format!("verb “{verb}”"))
    } else if in_list(&verb, V_CREATE) {
        base("creation", "name", format!("verb “{verb}”"))
    } else if in_list(&verb, V_COORD) {
        base("coordination", "name", format!("verb “{verb}”"))
    } else if in_list(&verb, V_LOGIC) {
        base("domain_logic", "name", format!("verb “{verb}”"))
    } else if other_tells > 0 && m.uses.len() <= 1 && own_calls == 0 && other_reads == 0 {
        base("coordination", "calls", "only calls other classes".into())
    } else if is_numeric_or_bool(returns) && (!m.uses.is_empty() || other_reads > 0) {
        base("domain_logic", "signature", format!("derives a {} from data", returns.unwrap_or("")))
    } else if (role == "domain" || role == "value") && (!m.uses.is_empty() || !m.params.is_empty()) {
        base("domain_logic", "default", "behaviour of a domain class".into())
    } else {
        base("coordination", "default", "no clear signal".into())
    };
    // pure delegation reads better as coordination, whatever the verb (a facade/controller method)
    if info.source == "name" && info.concern == "domain_logic" && other_tells == 1 && other_reads == 0 && own_calls <= 1 && m.uses.len() <= 1 {
        let target = tells.keys().next().cloned().unwrap_or_default();
        let callee = m.calls.iter().find_map(|x| x.split_once('.').filter(|(c, _)| *c == target).map(|(_, n)| n.to_string())).unwrap_or_default();
        info.concern = "coordination".into();
        info.source = "calls".into();
        info.reason = format!("delegates to {target}.{callee}()");
    }
    info.rdd = rdd_kind(m, &info.concern, &verb).into();
    info
}

fn rdd_kind(m: &Method, concern: &str, verb: &str) -> &'static str {
    if concern == "accessor" {
        return "knowing";
    }
    let returns = m.returns.as_deref();
    let decides = ["decide", "choose", "select", "resolve", "can", "should", "match", "validate", "check", "verify", "is", "has", "evaluate", "accept"];
    if decides.iter().any(|v| stem(v) == verb) && (returns.is_some_and(|r| r == "boolean" || r == "Boolean") || returns.is_none() && verb == "decide") {
        return "deciding";
    }
    if matches!(returns, Some(r) if r == "boolean" || r == "Boolean") && !m.params.is_empty() {
        return "deciding";
    }
    if returns.is_some() && m.params.is_empty() && concern != "creation" && concern != "coordination" {
        return "knowing";
    }
    "doing"
}

// ---------------------------------------------------------------- fit

/// How well a concern fits a role: None = fits, Some(severity, why) = misplaced.
pub fn misfit(role: &str, concern: &str) -> Option<(f64, &'static str)> {
    match (role, concern) {
        (_, "accessor") | (_, "creation") | (_, "coordination") => None,
        ("presentation", "domain_logic") => Some((0.6, "a UI class should only present data and pass user actions on; domain calculations belong to the domain (GRASP Controller: decouple the GUI from the domain)")),
        ("presentation", "persistence") => Some((0.6, "a UI class should not save or load data itself; that is a persistence responsibility (Pure Fabrication)")),
        ("presentation", _) => None,
        ("controller", "domain_logic") => Some((0.45, "a controller coordinates and delegates; doing the work itself makes it a fat controller (GRASP Controller)")),
        ("controller", "persistence") => Some((0.45, "a controller should delegate storage to a persistence class (Pure Fabrication)")),
        ("controller", "presentation") => Some((0.35, "a controller should not present data: that ties it to one UI")),
        ("controller", "formatting") => Some((0.25, "formatting output is not coordination; give it to an output/presentation class")),
        ("service", "presentation") => Some((0.45, "an application service should not present data: that ties it to one UI")),
        ("service", "formatting") => Some((0.2, "formatting output belongs to an output/presentation class")),
        ("service", _) => None,
        ("domain" | "value", "presentation") => Some((0.6, "a domain class should not know how it is shown (separate the domain from the UI)")),
        ("domain" | "value", "persistence") => Some((0.55, "a domain class saving itself couples it to a database (the Sale.save() example: use a Pure Fabrication like PersistentStorage)")),
        ("domain" | "value", "formatting") => Some((0.35, "formatting for output (HTML, JSON, text) is not domain knowledge; give it to an output class")),
        ("domain" | "value", _) => None,
        ("persistence", "domain_logic") => Some((0.4, "business rules hidden in a repository are hard to find and test; keep them in the domain (Information Expert)")),
        ("persistence", "presentation") => Some((0.6, "a persistence class should not present data")),
        ("persistence", _) => None,
        ("adapter", "domain_logic") => Some((0.4, "an adapter should only translate between the outside world and your design (Indirection); domain logic belongs to the domain")),
        ("adapter", _) => None,
        ("output", "domain_logic") => Some((0.4, "an output class should format what the domain computed, not compute it (Information Expert)")),
        ("output", "presentation") => None,
        ("output", _) => None,
        ("creator", "domain_logic") => Some((0.3, "a factory should create objects, not run domain logic")),
        _ => None,
    }
}

/// Layers a role is expected in.
fn expected_layers(role: &str) -> &'static [Layer] {
    match role {
        "presentation" => &[Layer::Ui],
        "controller" => &[Layer::Ui, Layer::Application, Layer::Domain],
        "service" => &[Layer::Application, Layer::Domain],
        "persistence" => &[Layer::Infrastructure],
        "adapter" => &[Layer::Infrastructure, Layer::Ui],
        "output" => &[Layer::Ui, Layer::Infrastructure, Layer::Application],
        "domain" | "value" => &[Layer::Domain],
        _ => &[Layer::Ui, Layer::Infrastructure, Layer::Application, Layer::Domain],
    }
}

// ---------------------------------------------------------------- report

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Finding {
    pub kind: &'static str,
    pub class: String,
    pub method: Option<String>,
    pub severity: f64,
    /// 0..1: how sure the signal is (a weak name heuristic is low; explicit tags and calls are high)
    pub confidence: f64,
    pub message: String,
    pub explain: String,
    pub fix_hint: String,
    /// Course reference, e.g. "GRASP Controller (Ch. 3)".
    pub source: &'static str,
    /// Quick Information-Expert guess (the class whose data it reads most); `suggest_home` ranks properly.
    pub home: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default, PartialEq)]
pub struct ClassResponsibility {
    pub role: Option<RoleInfo>,
    /// Concerns of its behavioural methods (accessors/creation/coordination left out) with counts.
    pub concerns: BTreeMap<String, usize>,
    /// 0..1: share of behavioural methods in the dominant concern (1 = one functional concern).
    pub purity: f64,
}

#[derive(Debug, Clone, Serialize, Default, PartialEq)]
pub struct Report {
    pub classes: BTreeMap<String, ClassResponsibility>,
    pub methods: Vec<MethodInfo>,
    pub findings: Vec<Finding>,
}

impl Report {
    pub fn method(&self, class: &str, method: &str) -> Option<&MethodInfo> {
        self.methods.iter().find(|m| m.class == class && m.method == method)
    }
}

fn confidence_for(source: &str) -> f64 {
    match source {
        "explicit" | "agent" => 0.95,
        "calls" | "swing_api" => 0.8,
        "signature" => 0.6,
        "name" => 0.65,
        _ => 0.4,
    }
}

/// The class whose data a method reads (or tells) most: a quick Information Expert guess.
pub fn expert_guess(mi: &MethodInfo) -> Option<String> {
    let mut score: BTreeMap<&String, usize> = BTreeMap::new();
    for (c, n) in &mi.reads {
        *score.entry(c).or_default() += 2 * n;
    }
    for (c, n) in &mi.tells {
        *score.entry(c).or_default() += n;
    }
    score.into_iter().max_by_key(|(_, n)| *n).map(|(c, _)| c.clone())
}

/// Analyse every live, non-external class.
pub fn analyse(d: &Design) -> Report {
    analyse_with(d, false)
}

/// `ghosts`: also judge proposed elements (previews).
pub fn analyse_with(d: &Design, ghosts: bool) -> Report {
    let live = |s: Status| s.is_live() || (ghosts && s == Status::Proposed);
    let mut r = Report::default();
    for c in d.classes.iter().filter(|c| live(c.status) && !d.is_external(&c.name)) {
        let ri = role_of(d, c);
        let role = ri.role.clone();
        let mut concerns: BTreeMap<String, usize> = BTreeMap::new();
        let mut misplaced_logic = 0;
        for m in c.methods.iter().filter(|m| live(m.status) && !m.is_abstract) {
            let mi = classify(d, c, m, &role);
            if !matches!(mi.concern.as_str(), "accessor" | "creation" | "coordination") {
                *concerns.entry(mi.concern.clone()).or_default() += 1;
            }
            if let Some((sev, why)) = misfit(&role, &mi.concern) {
                if role == "controller" && mi.concern == "domain_logic" {
                    misplaced_logic += 1;
                } else {
                    let home = expert_guess(&mi);
                    let conf = (confidence_for(&mi.source) + confidence_for(&ri.source)) / 2.0;
                    r.findings.push(Finding {
                        kind: "misplaced_responsibility",
                        class: c.name.clone(),
                        method: Some(m.name.clone()),
                        severity: sev,
                        confidence: conf,
                        message: format!(
                            "{}.{}() does {} ({}) but {} is {} ({})",
                            c.name,
                            m.name,
                            mi.concern.replace('_', " "),
                            mi.reason,
                            c.name,
                            role,
                            ri.evidence
                        ),
                        explain: why.to_string(),
                        fix_hint: match &home {
                            Some(h) => format!("move it to {h}, which owns the data it uses (Information Expert), or to a new class if no class fits (Pure Fabrication)"),
                            None => "move it to the class that owns the data it uses (Information Expert), or to a new class (Pure Fabrication)".into(),
                        },
                        source: if role == "presentation" { "GRASP Controller · separate UI from domain (Ch. 3)" } else { "RDD responsibilities · GRASP Information Expert (Ch. 2–3)" },
                        home,
                    });
                }
            }
            r.methods.push(mi);
        }
        // fat controller: does (domain) work instead of delegating, or hoards state
        let attrs = c.attributes.iter().filter(|a| live(a.status) && !a.is_static).count();
        if role == "controller" && (misplaced_logic >= 2 || attrs > 6) {
            r.findings.push(Finding {
                kind: "fat_controller",
                class: c.name.clone(),
                method: None,
                severity: 0.5,
                confidence: confidence_for(&ri.source),
                message: format!(
                    "{} is a controller but {}",
                    c.name,
                    [
                        (misplaced_logic >= 2).then(|| format!("does domain work in {misplaced_logic} methods")),
                        (attrs > 6).then(|| format!("keeps {attrs} attributes")),
                    ]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join(" and ")
                ),
                explain: "Controllers should limit themselves to control and coordination and delegate the real work; fat controllers easily become god classes.".into(),
                fix_hint: "delegate the work to the domain objects that own the data, or split it into use-case controllers".into(),
                source: "GRASP Controller (Ch. 3)",
                home: None,
            });
        } else if role == "controller" && misplaced_logic == 1 {
            let mi = r.methods.iter().rev().find(|m| m.class == c.name && m.concern == "domain_logic").cloned();
            if let Some(mi) = mi {
                let home = expert_guess(&mi);
                r.findings.push(Finding {
                    kind: "misplaced_responsibility",
                    class: c.name.clone(),
                    method: Some(mi.method.clone()),
                    severity: 0.35,
                    confidence: confidence_for(&mi.source),
                    message: format!("{}.{}() does domain work ({}) in a controller", c.name, mi.method, mi.reason),
                    explain: "A controller coordinates; the work itself belongs to the object with the information (Information Expert).".into(),
                    fix_hint: "move it to the domain object that owns the data and let the controller delegate".into(),
                    source: "GRASP Controller · Information Expert (Ch. 3)",
                    home,
                });
            }
        }
        // vague names hide the responsibility
        if ri.by_name.as_deref() == Some("utility") && c.role.is_none() {
            r.findings.push(Finding {
                kind: "vague_role",
                class: c.name.clone(),
                method: None,
                severity: 0.25,
                confidence: 0.6,
                message: format!("{}'s name says little about what it is responsible for", c.name),
                explain: "Names like Manager/Helper/Util/Data attract unrelated responsibilities (low cohesion); a class should have a limited set of responsibilities in one functional area.".into(),
                fix_hint: "rename it after its one responsibility, or split it; set its role explicitly if the name has to stay".into(),
                source: "High Cohesion · RDD (Ch. 2)",
                home: None,
            });
        }
        // name, stereotype and layer should tell the same story
        if c.role.is_none() {
            let name_role = ri.by_name.as_deref().filter(|r| *r != "utility");
            if let (Some(n), Some(s)) = (name_role, ri.by_stereotype.as_deref()) {
                let compatible = n == s || matches!((n, s), ("domain", "value") | ("value", "domain") | ("controller", "service") | ("adapter", "service"));
                if !compatible {
                    r.findings.push(role_inconsistency(c, format!("its name says {n} but its stereotype «{}» says {s}", c.stereotype.as_deref().unwrap_or("")), &ri, d));
                    continue_after_inconsistency(&mut r, c, &ri, d, &role, &concerns);
                    continue;
                }
            }
            let shown = name_role.or(ri.by_stereotype.as_deref());
            if let (Some(n), Some(l)) = (shown, layer_of(d, c)) {
                if !expected_layers(n).contains(&l) {
                    r.findings.push(role_inconsistency(c, format!("it is a {n} class in the {} layer", format!("{l:?}").to_lowercase()), &ri, d));
                }
            }
        }
        continue_after_inconsistency(&mut r, c, &ri, d, &role, &concerns);
    }
    r.findings.sort_by(|a, b| b.severity.partial_cmp(&a.severity).unwrap().then(a.class.cmp(&b.class)));
    r
}

fn role_inconsistency(c: &Class, what: String, ri: &RoleInfo, d: &Design) -> Finding {
    let _ = d;
    Finding {
        kind: "role_inconsistency",
        class: c.name.clone(),
        method: None,
        severity: 0.25,
        confidence: 0.7,
        message: format!("{}: {what}", c.name),
        explain: "When a class's name, stereotype and package disagree, readers (and the examiner) can't tell what it is responsible for.".into(),
        fix_hint: format!("make them agree: rename it, change the stereotype or move it to the right layer (or set role explicitly; now read as {})", ri.role),
        source: "RDD roles · layering (Ch. 2)",
        home: None,
    }
}

/// Per-class summary (purity, mixed concerns) — shared by both branches of the consistency check.
fn continue_after_inconsistency(r: &mut Report, c: &Class, ri: &RoleInfo, _d: &Design, role: &str, concerns: &BTreeMap<String, usize>) {
    if r.classes.contains_key(&c.name) {
        return;
    }
    let total: usize = concerns.values().sum();
    let top = concerns.values().copied().max().unwrap_or(0);
    let purity = if total == 0 { 1.0 } else { top as f64 / total as f64 };
    let distinct: BTreeSet<&String> = concerns.keys().collect();
    if distinct.len() >= 3 && total >= 4 && role != "utility" {
        r.findings.push(Finding {
            kind: "mixed_concerns",
            class: c.name.clone(),
            method: None,
            severity: 0.35,
            confidence: 0.6,
            message: format!(
                "{} mixes {} kinds of work ({})",
                c.name,
                distinct.len(),
                concerns.iter().map(|(k, n)| format!("{} {n}", k.replace('_', " "))).collect::<Vec<_>>().join(", ")
            ),
            explain: "A class with responsibilities in widely varying functional areas has very low cohesion: it changes for many reasons.".into(),
            fix_hint: "split it along those concerns, e.g. move formatting to an output class and storage to a persistence class (Pure Fabrication)".into(),
            source: "High Cohesion (Ch. 2)",
            home: None,
        });
    }
    r.classes.insert(c.name.clone(), ClassResponsibility { role: Some(ri.clone()), concerns: concerns.clone(), purity });
}
