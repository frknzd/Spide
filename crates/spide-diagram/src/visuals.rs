//! Shared visual vocabulary: fonts, colours, metric → visual mappings, geometry helpers.

use spide_core::metrics::ClassMetrics;
use spide_core::model::{Attribute, Class, Layer, Method, Status, Visibility};
use std::collections::{HashMap, HashSet};

/// An sRGB colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub const fn hex(v: u32) -> Rgb {
        Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
    }
    pub fn css(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.0, self.1, self.2)
    }
    pub fn parse(s: &str) -> Option<Rgb> {
        crate::theme::parse_color(s).map(|c| Rgb(c[0].round() as u8, c[1].round() as u8, c[2].round() as u8))
    }
}

/// Canvas palette (dark).
pub mod c {
    use super::Rgb;
    pub const BG: Rgb = Rgb::hex(0x1d1e22);
    pub const GRID: Rgb = Rgb::hex(0x26282e);
    pub const NODE: Rgb = Rgb::hex(0x25272d);
    pub const NODE_HEADER: Rgb = Rgb::hex(0x2b2e35);
    pub const BORDER: Rgb = Rgb::hex(0x4b5263);
    pub const TEXT: Rgb = Rgb::hex(0xe6e8ee);
    pub const DIM: Rgb = Rgb::hex(0x9aa1b1);
    pub const FAINT: Rgb = Rgb::hex(0x5d6474);
    pub const EDGE: Rgb = Rgb::hex(0x8a93a6);
    pub const CYCLE: Rgb = Rgb::hex(0xff5c6c);
    pub const WARN: Rgb = Rgb::hex(0xffb454);
    pub const GHOST: Rgb = Rgb::hex(0x5eead4);
    pub const REMOVING: Rgb = Rgb::hex(0xff7a85);
    pub const ACCENT: Rgb = Rgb::hex(0x7aa2ff);
    pub const GOOD: Rgb = Rgb::hex(0x4ade80);
    pub const LIME: Rgb = Rgb::hex(0xa3e635);
    pub const LAVENDER: Rgb = Rgb::hex(0xc4b5fd);
}

pub fn layer_color(l: Layer) -> Rgb {
    match l {
        Layer::Ui => Rgb::hex(0xc084fc),
        Layer::Infrastructure => Rgb::hex(0xf472b6),
        Layer::Application => Rgb::hex(0x60a5fa),
        Layer::Domain => Rgb::hex(0x34d399),
    }
}

pub fn layer_name(l: Layer) -> &'static str {
    match l {
        Layer::Ui => "ui",
        Layer::Infrastructure => "infrastructure",
        Layer::Application => "application",
        Layer::Domain => "domain",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Family {
    Sans,
    Mono,
}

/// A font as the old CSS described it (`600 14px Helvetica Neue`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FontSpec {
    pub family: Family,
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
}

impl FontSpec {
    pub const fn sans(size: f32) -> Self {
        Self { family: Family::Sans, size, bold: false, italic: false }
    }
    pub const fn mono(size: f32) -> Self {
        Self { family: Family::Mono, size, bold: false, italic: false }
    }
    pub const fn bold(mut self) -> Self {
        self.bold = true;
        self
    }
    pub const fn italic(mut self) -> Self {
        self.italic = true;
        self
    }
}

pub mod f {
    use super::FontSpec;
    pub const NAME: FontSpec = FontSpec::sans(14.0).bold();
    pub const NAME_ITALIC: FontSpec = FontSpec::sans(14.0).bold().italic();
    pub const STEREO: FontSpec = FontSpec::sans(11.0);
    pub const MEMBER: FontSpec = FontSpec::mono(12.0);
    pub const SMALL: FontSpec = FontSpec::sans(10.5);
}

pub const FONT_SANS: &str = "'Helvetica Neue', Helvetica, Arial, sans-serif";
pub const FONT_MONO: &str = "Menlo, Monaco, monospace";

/// Text measurement (the GUI measures with its real fonts; tests use an approximation).
pub trait Measure {
    fn width(&self, text: &str, font: FontSpec) -> f64;
}

/// Character-count approximation (tests, headless export without fonts).
pub struct ApproxMeasure;

impl Measure for ApproxMeasure {
    fn width(&self, text: &str, font: FontSpec) -> f64 {
        let per = match font.family {
            Family::Mono => 0.602,
            Family::Sans => {
                if font.bold {
                    0.58
                } else {
                    0.53
                }
            }
        };
        text.chars().count() as f64 * per * font.size as f64
    }
}

pub fn vis_glyph(v: Visibility) -> &'static str {
    match v {
        Visibility::Public => "+",
        Visibility::Private => "−",
        Visibility::Protected => "#",
        Visibility::Package => "~",
    }
}

pub fn vis_label(v: Visibility) -> &'static str {
    match v {
        Visibility::Public => "public",
        Visibility::Private => "private",
        Visibility::Protected => "protected",
        Visibility::Package => "package-private",
    }
}

pub fn attr_text(a: &Attribute) -> String {
    let mut s = format!("{} {}", vis_glyph(a.visibility), a.name);
    if !a.ty.is_empty() {
        s += &format!(": {}", a.ty);
    }
    if let Some(m) = a.multiplicity.as_deref().filter(|m| !m.is_empty()) {
        s += &format!(" [{m}]");
    }
    s
}

pub fn method_text(m: &Method) -> String {
    let params: Vec<String> = m.params.iter().map(|p| if p.ty.is_empty() { p.name.clone() } else { format!("{}: {}", p.name, p.ty) }).collect();
    let mut s = format!("{} {}({})", vis_glyph(m.visibility), m.name, params.join(", "));
    if let Some(r) = m.returns.as_deref().filter(|r| !r.is_empty()) {
        s += &format!(": {r}");
    }
    s
}

pub const LINE: f64 = 17.0;
pub const PAD_X: f64 = 12.0;
const MAX_TEXT: f64 = 360.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    Attribute,
    Method,
    /// a non-member line (folded accessors, responsibilities)
    Note,
}

impl RowKind {
    pub fn key(self) -> &'static str {
        match self {
            RowKind::Attribute => "attribute",
            RowKind::Method => "method",
            RowKind::Note => "note",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Row {
    pub text: String,
    pub kind: RowKind,
    pub name: String,
    pub status: Status,
    pub is_static: bool,
    pub is_abstract: bool,
    pub y: f64,
    /// continuation line of a wrapped member/note (same element as the row above)
    pub cont: bool,
}

#[derive(Debug, Clone)]
pub struct ClassBox {
    pub w: f64,
    pub h: f64,
    pub header_h: f64,
    pub attr_y: f64,
    pub meth_y: f64,
    pub rows: Vec<Row>,
    pub stereo: Option<String>,
}

pub fn stereotype_text(stereotype: Option<&str>) -> Option<String> {
    match stereotype {
        None | Some("") | Some("abstract") => None,
        Some(s) => Some(format!("«{s}»")),
    }
}

/// Greedy word wrap without losing text: every word is kept, a word longer than `width` stays on its
/// own line. Continuation lines are indented.
pub fn wrap_all(m: &dyn Measure, text: &str, width: f64, font: FontSpec) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    let mut cur = String::new();
    for word in text.split_whitespace() {
        let next = if cur.is_empty() { word.to_string() } else { format!("{cur} {word}") };
        if m.width(&next, font) <= width || cur.trim().is_empty() {
            cur = next;
        } else {
            out.push(cur);
            cur = format!("\u{a0}\u{a0}\u{a0}\u{a0}{word}");
        }
    }
    if !cur.is_empty() || out.is_empty() {
        out.push(cur);
    }
    out
}

/// Greedy word wrap to at most `max` lines of `width` px (the last one truncated).
pub fn wrap(m: &dyn Measure, text: &str, width: f64, max: usize, font: FontSpec) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    let mut cur = String::new();
    for word in text.split_whitespace() {
        let next = if cur.is_empty() { word.to_string() } else { format!("{cur} {word}") };
        if m.width(&next, font) <= width || cur.is_empty() {
            cur = next;
        } else {
            out.push(cur);
            cur = format!("  {word}");
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    if out.len() > max {
        let mut kept: Vec<String> = out[..max].to_vec();
        let rest = out[max..].join(" ");
        kept[max - 1] = truncate(m, &format!("{} {rest}", kept[max - 1]), font, width);
        return kept;
    }
    out
}

pub fn truncate(m: &dyn Measure, text: &str, font: FontSpec, max: f64) -> String {
    if m.width(text, font) <= max {
        return text.to_string();
    }
    let mut t: Vec<char> = text.chars().collect();
    while t.len() > 4 && m.width(&(t.iter().collect::<String>() + "…"), font) > max {
        t.pop();
    }
    t.into_iter().collect::<String>() + "…"
}

pub fn truncate_default(m: &dyn Measure, text: &str, font: FontSpec) -> String {
    truncate(m, text, font, MAX_TEXT)
}

/// Granularity levels (slider): 0 packages · 1 names · 2 responsibilities · 3 design · 4 full.
pub const DETAIL_LABELS: [&str; 5] = ["packages", "names", "responsibilities", "design", "full"];
pub const DEFAULT_DETAIL: u8 = 3;

/// room at the end of a member row for its diagnostic marker
pub const MARKER_W: f64 = 26.0;

#[derive(Default, Clone)]
pub struct BoxContext<'a> {
    /// attribute names this class inherits (an ancestor declares them): not repeated
    pub inherited: Option<&'a HashSet<String>>,
    /// attributes that duplicate a drawn association: shown as the edge's role name instead
    pub associations: Option<&'a HashSet<String>>,
    /// methods that must stay visible (flagged by the diagnostics)
    pub keep: Option<HashSet<String>>,
    /// attributes that must stay visible (revealed member findings)
    pub keep_attrs: Option<HashSet<String>>,
    /// `attribute:name` / `method:name` rows that carry a diagnostic marker (reserve room for it)
    pub flagged: Option<HashSet<String>>,
    /// method → misplacement mark (`↗ Home`) drawn inside the row's right end (reserve room for it)
    pub mark_texts: Option<HashMap<String, String>>,
}

fn accessor_prefix(name: &str) -> bool {
    for p in ["get", "is", "has", "set"] {
        if let Some(rest) = name.strip_prefix(p) {
            if let Some(c) = rest.chars().next() {
                if c.is_ascii_uppercase() || c == '_' {
                    return true;
                }
            }
        }
    }
    false
}

/// A plain getter/setter/predicate or a constructor: folded at the design level.
pub fn is_accessor(class_name: &str, m: &Method) -> bool {
    if m.name == class_name {
        return true;
    }
    if !accessor_prefix(&m.name) {
        return false;
    }
    if m.name.starts_with("set") {
        return m.params.len() == 1 && m.calls.is_empty();
    }
    m.params.is_empty() && m.calls.iter().filter(|x| !x.starts_with("this.")).count() <= 1 && m.uses.len() <= 1
}

/// What `class_box` needs from a class (also satisfied by derived domain classes).
pub struct BoxInput<'a> {
    pub name: &'a str,
    pub stereotype: Option<&'a str>,
    pub is_abstract: bool,
    pub attributes: &'a [Attribute],
    pub methods: &'a [Method],
    pub responsibilities: &'a [String],
}

impl<'a> From<&'a Class> for BoxInput<'a> {
    fn from(c: &'a Class) -> Self {
        Self { name: &c.name, stereotype: c.stereotype.as_deref(), is_abstract: c.is_abstract, attributes: &c.attributes, methods: &c.methods, responsibilities: &c.responsibilities }
    }
}

pub fn class_box(m: &dyn Measure, c: &BoxInput, methods: bool, level: u8, ctx: &BoxContext, purpose: Option<&str>) -> ClassBox {
    let level = level.clamp(1, 4);
    let stereo = stereotype_text(c.stereotype);
    let name_font = if c.is_abstract || matches!(c.stereotype, Some("interface" | "trait" | "protocol")) { f::NAME_ITALIC } else { f::NAME };
    let mut w = m.width(c.name, name_font) + 2.0 * PAD_X + 8.0;
    if let Some(s) = &stereo {
        w = w.max(m.width(s, f::STEREO) + 2.0 * PAD_X);
    }
    let header_h = if stereo.is_some() { 40.0 } else { 30.0 };
    let mut rows: Vec<Row> = vec![];
    let mut y = header_h + 6.0;
    let attr_y = header_h;
    let is_enum = matches!(c.stereotype, Some("enum") | Some("enumeration"));
    let push = |rows: &mut Vec<Row>, y: &mut f64, w: &mut f64, r: Row, font: FontSpec| {
        let marker = if r.kind != RowKind::Note && ctx.flagged.as_ref().is_some_and(|f| f.contains(&format!("{}:{}", r.kind.key(), r.name))) { MARKER_W } else { 0.0 };
        let mark = if r.kind == RowKind::Method { ctx.mark_texts.as_ref().and_then(|t| t.get(&r.name)).map(|t| m.width(t, FontSpec::sans(11.0)) + 12.0).unwrap_or(0.0) } else { 0.0 };
        // long text wraps onto continuation lines instead of being cut off
        let lines = wrap_all(m, &r.text, MAX_TEXT, font);
        for (i, line) in lines.into_iter().enumerate() {
            let extra = if i == 0 { marker + mark } else { 0.0 };
            *w = w.max(m.width(&line, font) + 2.0 * PAD_X + extra);
            rows.push(Row { y: *y + 12.0, text: line, cont: i > 0, ..r.clone() });
            *y += LINE;
        }
    };
    let note = |text: &str| Row { text: text.to_string(), kind: RowKind::Note, name: text.to_string(), status: Status::Accepted, is_static: false, is_abstract: false, y: 0.0, cont: false };

    if level == 1 {
        return ClassBox { w: w.max(120.0).ceil(), h: header_h + 4.0, header_h, attr_y, meth_y: header_h + 4.0, rows, stereo };
    }
    if level == 2 {
        let mut lines: Vec<&str> = vec![];
        if let Some(p) = purpose {
            lines.push(p);
        }
        lines.extend(c.responsibilities.iter().map(|s| s.as_str()));
        for r in &lines {
            push(&mut rows, &mut y, &mut w, note(&format!("• {r}")), f::MEMBER);
        }
        if lines.is_empty() {
            push(&mut rows, &mut y, &mut w, note("(no responsibilities yet)"), f::MEMBER);
        }
        y += 6.0;
        return ClassBox { w: w.max(140.0).ceil(), h: y.ceil(), header_h, attr_y, meth_y: y, rows, stereo };
    }

    let design = level == 3;
    for a in c.attributes {
        let kept = ctx.keep_attrs.as_ref().is_some_and(|k| k.contains(&a.name));
        if design && !is_enum && !kept && (ctx.inherited.is_some_and(|s| s.contains(&a.name)) || ctx.associations.is_some_and(|s| s.contains(&a.name))) {
            continue;
        }
        // without methods (conceptual domain model) attributes carry no visibility
        let text = if is_enum { a.name.clone() } else if methods { attr_text(a) } else { attr_text(a).split_once(' ').map(|x| x.1.to_string()).unwrap_or_default() };
        push(&mut rows, &mut y, &mut w, Row { text, kind: RowKind::Attribute, name: a.name.clone(), status: a.status, is_static: a.is_static && !is_enum, is_abstract: false, y: 0.0, cont: false }, f::MEMBER);
    }
    if rows.is_empty() {
        y += 6.0;
    }
    y += 6.0;
    let meth_y = y;
    if methods {
        y += 6.0;
        let (mut accessors, mut helpers, mut shown) = (0, 0, 0);
        for me in c.methods {
            if design && me.status != Status::Proposed && me.status != Status::Removing && !ctx.keep.as_ref().is_some_and(|k| k.contains(&me.name)) {
                if is_accessor(c.name, me) {
                    accessors += 1;
                    continue;
                }
                if me.visibility == Visibility::Private && !me.is_abstract {
                    helpers += 1;
                    continue;
                }
            }
            shown += 1;
            let text = method_text(me);
            push(&mut rows, &mut y, &mut w, Row { text, kind: RowKind::Method, name: me.name.clone(), status: me.status, is_static: me.is_static, is_abstract: me.is_abstract, y: 0.0, cont: false }, f::MEMBER);
        }
        if accessors > 0 || helpers > 0 {
            let mut parts = vec![];
            if accessors > 0 {
                parts.push(format!("{accessors} accessor{}", if accessors == 1 { "" } else { "s" }));
            }
            if helpers > 0 {
                parts.push(format!("{helpers} private helper{}", if helpers == 1 { "" } else { "s" }));
            }
            push(&mut rows, &mut y, &mut w, note(&format!("+ {}", parts.join(", "))), f::MEMBER);
        }
        if shown == 0 && accessors == 0 && helpers == 0 {
            y += 6.0;
        }
        y += 6.0;
    }
    ClassBox { w: w.max(120.0).ceil(), h: if methods { y.ceil() } else { meth_y.ceil() }, header_h, attr_y, meth_y, rows, stereo }
}

// ---------------------------------------------------------------- metric → visuals

fn mix(a: Rgb, b: Rgb, t: f64) -> Rgb {
    let f = |x: u8, y: u8| (x as f64 + (y as f64 - x as f64) * t).round() as u8;
    Rgb(f(a.0, b.0), f(a.1, b.1), f(a.2, b.2))
}

/// blue → amber → red
pub fn heat_color(h: f64) -> Rgb {
    let t = h.clamp(0.0, 1.0);
    if t < 0.5 {
        mix(Rgb::hex(0x5b8cff), Rgb::hex(0xffb454), t / 0.5)
    } else {
        mix(Rgb::hex(0xffb454), Rgb::hex(0xff5c6c), (t - 0.5) / 0.5)
    }
}

pub fn heat_bucket(h: f64) -> u8 {
    (h * 5.0).round().clamp(0.0, 5.0) as u8
}

pub fn node_scale(m: Option<&ClassMetrics>) -> f64 {
    match m {
        None => 1.0,
        Some(m) => 1.0 + ((m.fan_in + m.fan_out).min(10) as f64) * 0.018 + if m.god { 0.05 } else { 0.0 },
    }
}

pub struct Badge {
    pub text: String,
    pub color: Rgb,
    pub title: String,
}

pub fn badges(m: Option<&ClassMetrics>) -> Vec<Badge> {
    let Some(m) = m else { return vec![] };
    let mut out = vec![];
    let b = |t: &str, color: Rgb, title: String| Badge { text: t.to_string(), color, title };
    if m.in_cycle {
        out.push(b("cycle", c::CYCLE, "Part of a dependency cycle".into()));
    }
    if m.god {
        out.push(b("god class", c::CYCLE, m.issues.join("; ")));
    } else if let Some(l) = m.lcom.filter(|l| *l >= 2) {
        out.push(b(&format!("LCOM {l}"), c::WARN, "Low cohesion: unrelated method groups".into()));
    }
    if m.layer_violations > 0 {
        out.push(b("rule ✗", c::WARN, "Breaks an architecture rule (layering, module boundary, forbidden dependency or type)".into()));
    }
    if m.orphan {
        out.push(b("orphan", c::FAINT, "Not connected".into()));
    }
    out
}

// ---------------------------------------------------------------- geometry

/// Centre-based rectangle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// Point where the ray from the rect centre towards (tx,ty) leaves the rect.
pub fn border_point(r: Rect, tx: f64, ty: f64, pad: f64) -> [f64; 2] {
    let (dx, dy) = (tx - r.x, ty - r.y);
    if dx == 0.0 && dy == 0.0 {
        return [r.x, r.y];
    }
    let (hw, hh) = (r.w / 2.0 + pad, r.h / 2.0 + pad);
    let nz = |v: f64| if v == 0.0 { 1e-9 } else { v.abs() };
    let s = (hw / nz(dx)).min(hh / nz(dy));
    [r.x + dx * s, r.y + dy * s]
}

pub fn status_opacity(s: Status) -> f64 {
    match s {
        Status::Proposed => 0.62,
        Status::Removing => 0.5,
        Status::Pruned => 0.22,
        _ => 1.0,
    }
}

pub fn status_name(s: Status) -> &'static str {
    match s {
        Status::Accepted => "accepted",
        Status::Proposed => "proposed",
        Status::Removing => "removing",
        Status::Pruned => "pruned",
        Status::Overruled => "overruled",
    }
}
