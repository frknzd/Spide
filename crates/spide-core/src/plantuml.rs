//! PlantUML export. Semantically complete (members, visibility, stereotypes,
//! multiplicities, packages, notes), styled like Spide, with direction hints
//! taken from the canvas layout.

use crate::derive;
use crate::metrics::Metrics;
use crate::model::*;
use crate::store::validate_sequence;
use std::collections::BTreeMap;
use std::fmt::Write;

type Layout = BTreeMap<String, BTreeMap<String, Pos>>;

const BG: &str = "#1D1E22";
const NODE: &str = "#25272D";
const HEADER: &str = "#2B2E35";
const BORDER: &str = "#4B5263";
const TEXT: &str = "#E6E8EE";
const DIM: &str = "#9AA1B1";
const EDGE: &str = "#8A93A6";
const RED: &str = "#FF5C6C";
const AMBER: &str = "#FFB454";

fn layer_color(l: Layer) -> &'static str {
    match l {
        Layer::Ui => "#C084FC",
        Layer::Infrastructure => "#F472B6",
        Layer::Application => "#60A5FA",
        Layer::Domain => "#34D399",
    }
}

fn header(title: &str, kind: &str) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "@startuml");
    let _ = writeln!(s, "' {kind} — exported from Spide");
    let _ = writeln!(s, "title {}", esc(title));
    let _ = writeln!(
        s,
        r#"skinparam backgroundColor {BG}
skinparam defaultFontName "Helvetica Neue"
skinparam defaultFontColor {TEXT}
skinparam titleFontColor {DIM}
skinparam shadowing false
skinparam roundCorner 12
skinparam classAttributeIconSize 0
hide circle
skinparam legend {{
  BackgroundColor {NODE}
  BorderColor {BORDER}
  FontColor {DIM}
}}
skinparam ArrowColor {EDGE}
skinparam ArrowFontColor {DIM}
skinparam ArrowFontSize 11
skinparam class {{
  BackgroundColor {NODE}
  HeaderBackgroundColor {HEADER}
  BorderColor {BORDER}
  FontColor {TEXT}
  FontStyle bold
  AttributeFontColor {TEXT}
  AttributeFontName Menlo
  AttributeFontSize 12
  StereotypeFontColor {DIM}
}}
skinparam package {{
  BackgroundColor transparent
  FontColor {DIM}
  BorderColor {BORDER}
}}
skinparam note {{
  BackgroundColor #2B2E35
  BorderColor {AMBER}
  FontColor {TEXT}
}}"#
    );
    s
}

fn esc(s: &str) -> String {
    s.replace('"', "'").replace('\n', " ")
}

/// Identifier usable in PlantUML (quote when needed).
fn id(name: &str) -> String {
    if name.chars().all(|c| c.is_alphanumeric() || c == '_') && !name.is_empty() {
        name.to_string()
    } else {
        format!("\"{}\"", esc(name))
    }
}

fn vis(v: Visibility) -> &'static str {
    match v {
        Visibility::Public => "+",
        Visibility::Private => "-",
        Visibility::Protected => "#",
        Visibility::Package => "~",
    }
}

fn shown(s: Status) -> bool {
    s.is_live()
}

/// Direction keyword from canvas positions (dominant axis).
fn direction(layout: Option<&BTreeMap<String, Pos>>, from: &str, to: &str) -> &'static str {
    let Some(l) = layout else { return "" };
    match (l.get(from), l.get(to)) {
        (Some(a), Some(b)) => {
            let (dx, dy) = (b.x - a.x, b.y - a.y);
            if dx.abs() < 1.0 && dy.abs() < 1.0 {
                ""
            } else if dy.abs() >= dx.abs() * 0.8 {
                if dy > 0.0 { "down" } else { "up" }
            } else if dx > 0.0 {
                "right"
            } else {
                "left"
            }
        }
        _ => "",
    }
}

fn class_block(out: &mut String, c: &Class, methods: bool, indent: &str) {
    let kw = if c.is_enum() {
        "enum"
    } else if c.is_interface() {
        "interface"
    } else if c.is_abstract {
        "abstract class"
    } else {
        "class"
    };
    let st = c
        .stereotype
        .as_deref()
        .filter(|s| !matches!(*s, "interface" | "enum" | "abstract"))
        .map(|s| format!(" <<{s}>>"))
        .unwrap_or_default();
    let color = if c.status == Status::Overruled { format!(" #line:{}", &AMBER[1..]) } else { String::new() };
    let _ = writeln!(out, "{indent}{kw} {}{st}{color} {{", id(&c.name));
    for a in c.attributes.iter().filter(|a| shown(a.status)) {
        if c.is_enum() {
            let _ = writeln!(out, "{indent}  {}", a.name);
            continue;
        }
        let mut line = format!("{}{}", vis(a.visibility), a.name);
        if !a.ty.is_empty() {
            line.push_str(&format!(" : {}", a.ty));
        }
        if let Some(m) = &a.multiplicity {
            line.push_str(&format!(" [{m}]"));
        }
        let st = if a.is_static { "{static} " } else { "" };
        let _ = writeln!(out, "{indent}  {st}{line}");
    }
    if methods {
        if !c.attributes.is_empty() || !c.is_enum() {
            let _ = writeln!(out, "{indent}  --");
        }
        for m in c.methods.iter().filter(|m| shown(m.status)) {
            let params: Vec<String> = m.params.iter().map(|p| if p.ty.is_empty() { p.name.clone() } else { format!("{} : {}", p.name, p.ty) }).collect();
            let mut line = format!("{}{}({})", vis(m.visibility), m.name, params.join(", "));
            if let Some(r) = &m.returns {
                line.push_str(&format!(" : {r}"));
            }
            let mut mods = String::new();
            if m.is_static {
                mods.push_str("{static} ");
            }
            if m.is_abstract {
                mods.push_str("{abstract} ");
            }
            let _ = writeln!(out, "{indent}  {mods}{line}");
        }
    }
    let _ = writeln!(out, "{indent}}}");
}

fn relation_line(r: &Relation, m: Option<&Metrics>, layout: Option<&BTreeMap<String, Pos>>) -> String {
    let cyc = m.is_some_and(|m| m.cycle_edges.contains(&r.id));
    let lv = m.is_some_and(|m| m.layer_violations.contains(&r.id));
    let style = if cyc {
        format!("[{RED},bold]")
    } else if lv {
        format!("[{AMBER},bold]")
    } else if r.status == Status::Overruled {
        format!("[{AMBER}]")
    } else {
        String::new()
    };
    let dir = direction(layout, &r.from, &r.to);
    let (l, body, rgt) = match r.kind {
        RelationKind::Association => ("", "-", if r.directed { ">" } else { "" }),
        RelationKind::Aggregation => ("o", "-", ""),
        RelationKind::Composition => ("*", "-", ""),
        RelationKind::Inheritance => ("", "-", "|>"),
        RelationKind::Realization => ("", ".", "|>"),
        RelationKind::Dependency => ("", ".", ">"),
    };
    // PlantUML: <left-head><line>[style]<direction><line><right-head>
    let arrow = format!("{l}{body}{style}{dir}{body}{rgt}");
    let fm = r.from_mult.as_ref().map(|m| format!(" \"{m}\"")).unwrap_or_default();
    let tm = r.to_mult.as_ref().map(|m| format!("\"{m}\" ")).unwrap_or_default();
    let mut label = r.label.clone().map(|l| esc(&l)).unwrap_or_default();
    if r.interaction != Interaction::Sync {
        let tag = if r.interaction == Interaction::Async { "<<async>>" } else { "<<event>>" };
        label = if label.is_empty() { tag.into() } else { format!("{tag} {label}") };
    }
    if cyc {
        label = if label.is_empty() { "<color:#FF5C6C>⟲ cycle</color>".into() } else { format!("{label} <color:#FF5C6C>⟲</color>") };
    } else if lv {
        label = if label.is_empty() { "<color:#FFB454>⚠ rule</color>".into() } else { format!("{label} <color:#FFB454>⚠</color>") };
    }
    let label = if label.is_empty() { String::new() } else { format!(" : {label}") };
    format!("{}{fm} {arrow} {tm}{}{label}", id(&r.from), id(&r.to))
}

pub fn class_diagram(d: &Design, m: &Metrics, layout: &Layout) -> String {
    let mut out = header(&format!("{} — Class diagram", d.name), "class diagram");
    let classes: Vec<&Class> = d.classes.iter().filter(|c| shown(c.status)).collect();
    let mut by_pkg: BTreeMap<Option<String>, Vec<&Class>> = BTreeMap::new();
    for c in &classes {
        by_pkg.entry(c.package.clone()).or_default().push(c);
    }
    for (pkg, cs) in &by_pkg {
        match pkg {
            Some(p) => {
                let pk = d.packages.iter().find(|x| &x.name == p);
                let (st, color) = match pk.and_then(|x| x.layer) {
                    Some(l) => (
                        if format!("{l:?}").to_lowercase() == *p { String::new() } else { format!(" <<{}>>", format!("{l:?}").to_lowercase()) },
                        format!(" #line:{}", &layer_color(l)[1..]),
                    ),
                    None => (String::new(), String::new()),
                };
                let _ = writeln!(out, "\npackage {}{st}{color} {{", id(p));
                for c in cs {
                    class_block(&mut out, c, true, "  ");
                }
                let _ = writeln!(out, "}}");
            }
            None => {
                out.push('\n');
                for c in cs {
                    class_block(&mut out, c, true, "");
                }
            }
        }
    }
    out.push('\n');
    let lay = layout.get("class");
    for r in d.live_relations() {
        let _ = writeln!(out, "{}", relation_line(r, Some(m), lay));
    }
    // notes for overruled decisions
    for c in classes.iter().filter(|c| c.status == Status::Overruled) {
        let _ = writeln!(out, "note bottom of {} : ⚠ applied by overruling the examiner", id(&c.name));
    }
    let _ = writeln!(out, "\nlegend right\n  <b>health {}/100</b>", m.health);
    for i in m.issues.iter().take(5) {
        let _ = writeln!(out, "  {}", esc(&i.message));
    }
    let _ = writeln!(out, "endlegend");
    out.push_str("@enduml\n");
    out
}

pub fn domain_model(d: &Design, layout: &Layout) -> String {
    let mut out = header(&format!("{} — Domain model", d.name), "domain model");
    out.push_str("hide methods\nhide empty members\n\n");
    let dm = derive::domain_model(d);
    for c in dm.classes.iter().filter(|c| shown(c.status)) {
        let full = d.class(&c.name).cloned().unwrap_or_default();
        let concept = Class { methods: vec![], package: None, ..full };
        class_block(&mut out, &concept, false, "");
    }
    out.push('\n');
    let lay = layout.get("domain").or_else(|| layout.get("class"));
    for r in dm.relations.iter().filter(|r| shown(r.status)) {
        let _ = writeln!(out, "{}", relation_line(r, None, lay));
    }
    out.push_str("@enduml\n");
    out
}

pub fn package_diagram(d: &Design, m: &Metrics) -> String {
    let mut out = header(&format!("{} — Package diagram", d.name), "package diagram");
    let pv = derive::package_view(d);
    for p in pv.packages.iter().filter(|p| shown(p.status)) {
        let pm = m.packages.iter().find(|x| x.name == p.name);
        let st = p.layer.filter(|l| format!("{l:?}").to_lowercase() != p.name).map(|l| format!(" <<{}>>", format!("{l:?}").to_lowercase())).unwrap_or_default();
        let color = p.layer.map(|l| format!(" #line:{}", &layer_color(l)[1..])).unwrap_or_default();
        let _ = writeln!(out, "package {}{st}{color} {{", id(&p.name));
        for c in &p.classes {
            if d.live_class(c).is_some() {
                let _ = writeln!(out, "  class {}", id(c));
            }
        }
        let _ = writeln!(out, "}}");
        if let Some(pm) = pm {
            let _ = writeln!(out, "note bottom of {} : I={:.2} A={:.2} D={:.2}", id(&p.name), pm.instability, pm.abstractness, pm.distance);
        }
    }
    out.push_str("hide members\nhide circle\n\n");
    for dep in &m.package_deps {
        let style = if dep.in_cycle { format!("[{RED},bold]") } else if dep.layer_violation { format!("[{AMBER},bold]") } else { String::new() };
        let _ = writeln!(out, "{} .{style}.> {} : {}", id(&dep.from), id(&dep.to), dep.count);
    }
    out.push_str("@enduml\n");
    out
}

pub fn use_case_diagram(d: &Design) -> String {
    let mut out = header(&format!("{} — Use case diagram", d.name), "use case diagram");
    out.push_str(&format!(
        "left to right direction\nskinparam usecase {{\n  BackgroundColor {NODE}\n  BorderColor #7AA2FF\n  FontColor {TEXT}\n}}\nskinparam actor {{\n  BorderColor {TEXT}\n  BackgroundColor {BG}\n  FontColor {TEXT}\n}}\nskinparam rectangle {{\n  BorderColor {BORDER}\n  FontColor {DIM}\n}}\n\n"
    ));
    let ucm = &d.use_cases;
    let alias: BTreeMap<&str, String> = ucm.use_cases.iter().enumerate().map(|(i, u)| (u.name.as_str(), format!("UC{}", i + 1))).collect();
    let aid: BTreeMap<&str, String> = ucm.actors.iter().enumerate().map(|(i, a)| (a.name.as_str(), format!("A{}", i + 1))).collect();
    for a in ucm.actors.iter().filter(|a| shown(a.status)) {
        let _ = writeln!(out, "actor \"{}\" as {}", esc(&a.name), aid[a.name.as_str()]);
    }
    let _ = writeln!(out, "rectangle \"{}\" {{", esc(ucm.system.as_deref().unwrap_or(&d.name)));
    for u in ucm.use_cases.iter().filter(|u| shown(u.status)) {
        let _ = writeln!(out, "  usecase \"{}\" as {}", esc(&u.name), alias[u.name.as_str()]);
    }
    let _ = writeln!(out, "}}");
    for u in ucm.use_cases.iter().filter(|u| shown(u.status)) {
        let me = &alias[u.name.as_str()];
        for a in &u.actors {
            if let Some(x) = aid.get(a.as_str()) {
                let _ = writeln!(out, "{x} -- {me}");
            }
        }
        for i in &u.includes {
            if let Some(x) = alias.get(i.as_str()) {
                let _ = writeln!(out, "{me} ..> {x} : <<include>>");
            }
        }
        for e in &u.extends {
            if let Some(x) = alias.get(e.as_str()) {
                let _ = writeln!(out, "{me} ..> {x} : <<extend>>");
            }
        }
    }
    out.push_str("@enduml\n");
    out
}

pub fn sequence_diagrams(d: &Design) -> String {
    let mut all = String::new();
    let actors: Vec<&str> = d.use_cases.actors.iter().map(|a| a.name.as_str()).collect();
    for s in d.sequences.iter().filter(|s| shown(s.status)) {
        let mut s = s.clone();
        validate_sequence(d, &mut s);
        let mut out = header(&format!("{} — {}", d.name, s.name), "sequence diagram");
        let _ = writeln!(
            out,
            "skinparam sequence {{\n  LifeLineBorderColor #5D6474\n  ParticipantBackgroundColor {NODE}\n  ParticipantBorderColor {BORDER}\n  ParticipantFontColor {TEXT}\n  ActorBorderColor {TEXT}\n  ActorBackgroundColor {BG}\n  ActorFontColor {TEXT}\n  LifeLineBackgroundColor #30343D\n  ArrowColor {TEXT}\n}}\nautoactivate on\n"
        );
        for p in &s.participants {
            let kw = if actors.contains(&p.as_str()) { "actor" } else { "participant" };
            let _ = writeln!(out, "{kw} {}", id(p));
        }
        out.push('\n');
        let mut numbered = 0;
        for m in s.messages.iter().filter(|m| m.status != Status::Pruned) {
            let color = if m.problem.is_some() { format!("[{RED}]") } else { String::new() };
            let (arrow, text) = match m.kind {
                MessageKind::Return => (format!("-{color}->"), m.method.clone()),
                MessageKind::Async => (format!("-{color}>>"), call_text(m)),
                MessageKind::Create => {
                    let _ = writeln!(out, "create {}", id(&m.to));
                    (format!("-{color}>"), format!("<<create>> {}", call_text(m)))
                }
                MessageKind::Sync => (format!("-{color}>"), call_text(m)),
            };
            if m.kind != MessageKind::Return {
                numbered += 1;
            }
            let problem = m.problem.as_ref().map(|p| format!(" <color:{RED}>✗ {}</color>", esc(p))).unwrap_or_default();
            let num = if m.kind == MessageKind::Return { String::new() } else { format!("{numbered}: ") };
            let _ = writeln!(out, "{} {arrow} {} : {num}{}{problem}", id(&m.from), id(&m.to), esc(&text));
            if m.from == m.to && m.kind == MessageKind::Sync {
                let _ = writeln!(out, "return");
            }
            if let Some(n) = &m.note {
                let _ = writeln!(out, "note right : {}", esc(n));
            }
        }
        out.push_str("@enduml\n\n");
        all.push_str(&out);
    }
    if all.is_empty() {
        all = format!("{}note \"no sequence diagrams yet\" as N\n@enduml\n", header(&d.name, "sequence diagram"));
    }
    all
}

fn call_text(m: &Message) -> String {
    let base = if m.method.contains('(') { m.method.clone() } else { format!("{}({})", m.method, m.args.as_deref().unwrap_or("")) };
    match &m.returns {
        Some(r) => format!("{base} : {r}"),
        None => base,
    }
}

/// Export the given view (`class`, `domain`, `package`, `usecase`, `sequence`).
fn c4_id(name: &str) -> String {
    let s: String = name.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
    if s.chars().next().is_none_or(|c| c.is_ascii_digit()) { format!("n_{s}") } else { s }
}

fn c4_str(s: &str) -> String {
    s.replace('"', "'")
}

/// C4-PlantUML (stdlib): a context diagram and a container diagram.
pub fn c4_diagrams(d: &Design, m: &Metrics) -> String {
    let v = crate::derive::system_view(d, m);
    let mut o = String::new();
    let sys = c4_id(&v.system);
    // ---- context
    let _ = writeln!(o, "@startuml\n' C4 context — exported from Spide\n!include <C4/C4_Context>\nLAYOUT_WITH_LEGEND()\ntitle {} — System context\n", esc(&v.system));
    for a in &v.actors {
        let _ = writeln!(o, "Person({}, \"{}\")", c4_id(a), c4_str(a));
    }
    let _ = writeln!(o, "System({sys}, \"{}\", \"{} containers\")", c4_str(&v.system), v.containers.len());
    for x in &v.systems {
        let _ = writeln!(o, "System_Ext({}, \"{}\", \"{}\")", c4_id(&x.name), c4_str(&x.name), c4_str(x.description.as_deref().unwrap_or("")));
    }
    let _ = writeln!(o);
    for e in &v.context_edges {
        let _ = writeln!(o, "Rel({}, {}, \"{}\")", c4_id(&e.from), c4_id(&e.to), c4_str(e.label.as_deref().unwrap_or("uses")));
    }
    let _ = writeln!(o, "@enduml\n");
    // ---- containers
    let _ = writeln!(o, "@startuml\n' C4 containers — exported from Spide\n!include <C4/C4_Container>\nLAYOUT_WITH_LEGEND()\ntitle {} — Containers\n", esc(&v.system));
    for a in &v.actors {
        let _ = writeln!(o, "Person({}, \"{}\")", c4_id(a), c4_str(a));
    }
    let _ = writeln!(o, "System_Boundary(b_{sys}, \"{}\") {{", c4_str(&v.system));
    for k in &v.containers {
        let macro_ = if k.datastore { "ContainerDb" } else { "Container" };
        let mut desc = k.packages.join(", ");
        if k.replicas > 1 {
            desc = format!("{desc} (×{})", k.replicas);
        }
        let spof = m.issues.iter().any(|i| i.kind == "spof" && i.targets.contains(&k.name));
        if spof {
            desc = format!("{desc} — single point of failure");
        }
        let _ = writeln!(o, "  {macro_}({}, \"{}\", \"{}\", \"{}\")", c4_id(&k.name), c4_str(&k.name), c4_str(k.technology.as_deref().unwrap_or("")), c4_str(&desc));
    }
    let _ = writeln!(o, "}}");
    for x in &v.systems {
        let _ = writeln!(o, "System_Ext({}, \"{}\", \"{}\")", c4_id(&x.name), c4_str(&x.name), c4_str(x.description.as_deref().unwrap_or("")));
    }
    let _ = writeln!(o);
    for dep in &v.container_deps {
        let label = format!("{} dependenc{}{}", dep.count, if dep.count == 1 { "y" } else { "ies" }, if dep.sync { "" } else { " (async)" });
        let _ = writeln!(o, "Rel({}, {}, \"{label}\")", c4_id(&dep.from), c4_id(&dep.to));
    }
    for l in &v.links {
        let mut label = l.description.clone().unwrap_or_else(|| "uses".into());
        if l.interaction != Interaction::Sync {
            label = format!("{label} ({})", format!("{:?}", l.interaction).to_lowercase());
        }
        let tech = l.protocol.as_deref().map(|p| format!(", \"{}\"", c4_str(p))).unwrap_or_default();
        let _ = writeln!(o, "Rel({}, {}, \"{}\"{tech})", c4_id(&l.from), c4_id(&l.to), c4_str(&label));
    }
    let _ = writeln!(o, "@enduml");
    o
}

pub fn export(view: &str, d: &Design, m: &Metrics, layout: &Layout) -> Result<String, String> {
    Ok(match view {
        "class" => class_diagram(d, m, layout),
        "domain" => domain_model(d, layout),
        "package" => package_diagram(d, m),
        "usecase" => use_case_diagram(d),
        "sequence" => sequence_diagrams(d),
        "system" => c4_diagrams(d, m),
        _ => return Err(format!("unknown view {view}")),
    })
}
