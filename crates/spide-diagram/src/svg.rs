//! Serialise a scene into a standalone, print-quality SVG (PNG via resvg, PDF via svg2pdf in core).

use crate::route::cmds_to_svg;
use crate::scene::{bbox, tag, Anchor, Group, Paint, Prim, Scene, Stroke};
use crate::theme::light_color;
use crate::visuals::{c, f, health_color, FontSpec, Measure, Rgb, FONT_MONO, FONT_SANS};

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Colour output (dark canvas or light paper).
pub struct Writer<'a> {
    pub light: bool,
    pub skip: u8,
    pub m: &'a dyn Measure,
    pub out: String,
}

impl Writer<'_> {
    fn col(&self, c: Rgb) -> String {
        if self.light {
            light_color(&c.css())
        } else {
            c.css()
        }
    }
    fn n(v: f64) -> String {
        let s = format!("{v:.2}");
        let s = s.trim_end_matches('0').trim_end_matches('.');
        if s == "-0" {
            "0".into()
        } else {
            s.to_string()
        }
    }
    fn fill(&self, p: Option<Paint>) -> String {
        match p {
            None => " fill=\"none\"".into(),
            Some(p) if p.opacity >= 1.0 => format!(" fill=\"{}\"", self.col(p.color)),
            Some(p) => format!(" fill=\"{}\" fill-opacity=\"{}\"", self.col(p.color), Self::n(p.opacity as f64)),
        }
    }
    fn stroke(&self, s: Option<Stroke>) -> String {
        let Some(s) = s else { return String::new() };
        let mut o = format!(" stroke=\"{}\" stroke-width=\"{}\"", self.col(s.color), Self::n(s.width as f64));
        if s.opacity < 1.0 {
            o += &format!(" stroke-opacity=\"{}\"", Self::n(s.opacity as f64));
        }
        if let Some([a, b]) = s.dash {
            o += &format!(" stroke-dasharray=\"{} {}\"", Self::n(a as f64), Self::n(b as f64));
        }
        o
    }
    fn font(&self, f: FontSpec) -> String {
        let mut o = format!(" font-family=\"{}\" font-size=\"{}\"", esc(if f.family == crate::visuals::Family::Mono { FONT_MONO } else { FONT_SANS }), Self::n(f.size as f64));
        if f.bold {
            o += " font-weight=\"600\"";
        }
        if f.italic {
            o += " font-style=\"italic\"";
        }
        o
    }

    pub fn group(&mut self, g: &Group) {
        if g.tags & self.skip != 0 {
            return;
        }
        let mut attrs = String::new();
        if g.tx != 0.0 || g.ty != 0.0 || g.scale != 1.0 {
            attrs += &format!(" transform=\"translate({},{})", Self::n(g.tx), Self::n(g.ty));
            if g.scale != 1.0 {
                attrs += &format!(" scale({})", Self::n(g.scale));
            }
            attrs.push('"');
        }
        if g.opacity < 1.0 {
            attrs += &format!(" opacity=\"{}\"", Self::n(g.opacity as f64));
        }
        self.out += &format!("<g{attrs}>");
        for p in &g.children {
            self.prim(p);
        }
        self.out += "</g>";
    }

    fn prim(&mut self, p: &Prim) {
        let n = Self::n;
        match p {
            Prim::Rect { x, y, w, h, rx, fill, stroke } => {
                let r = if *rx > 0.0 { format!(" rx=\"{}\"", n(*rx)) } else { String::new() };
                let s = format!("<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{r}{}{}/>", n(*x), n(*y), n(*w), n(*h), self.fill(*fill), self.stroke(*stroke));
                self.out += &s;
            }
            Prim::Glow { x, y, w, h, rx, color, opacity } => {
                let s = format!("<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"{}\" fill=\"{}\" opacity=\"{}\" filter=\"url(#halo3)\"/>", n(*x), n(*y), n(*w), n(*h), n(*rx), self.col(*color), n(*opacity as f64));
                self.out += &s;
            }
            Prim::Circle { cx, cy, r, fill, stroke } => {
                let s = format!("<circle cx=\"{}\" cy=\"{}\" r=\"{}\"{}{}/>", n(*cx), n(*cy), n(*r), self.fill(*fill), self.stroke(*stroke));
                self.out += &s;
            }
            Prim::Ellipse { cx, cy, rx, ry, fill, stroke } => {
                let s = format!("<ellipse cx=\"{}\" cy=\"{}\" rx=\"{}\" ry=\"{}\"{}{}/>", n(*cx), n(*cy), n(*rx), n(*ry), self.fill(*fill), self.stroke(*stroke));
                self.out += &s;
            }
            Prim::Poly { pts, closed, fill, stroke } => {
                let pts: Vec<String> = pts.iter().map(|p| format!("{},{}", n(p[0]), n(p[1]))).collect();
                let tag = if *closed { "polygon" } else { "polyline" };
                let s = format!("<{tag} points=\"{}\"{}{} stroke-linejoin=\"round\" stroke-linecap=\"round\"/>", pts.join(" "), self.fill(*fill), self.stroke(*stroke));
                self.out += &s;
            }
            Prim::Path { cmds, stroke } => {
                let s = format!("<path d=\"{}\" fill=\"none\"{} stroke-linecap=\"round\" stroke-linejoin=\"round\"/>", cmds_to_svg(cmds), self.stroke(Some(*stroke)));
                self.out += &s;
            }
            Prim::Text { x, y, text, font, color, opacity, anchor, underline } => {
                let a = match anchor {
                    Anchor::Start => "",
                    Anchor::Middle => " text-anchor=\"middle\"",
                    Anchor::End => " text-anchor=\"end\"",
                };
                let o = if *opacity < 1.0 { format!(" opacity=\"{}\"", n(*opacity as f64)) } else { String::new() };
                let u = if *underline { " text-decoration=\"underline\"" } else { "" };
                let s = format!("<text x=\"{}\" y=\"{}\"{a}{}{o}{u} fill=\"{}\">{}</text>", n(*x), n(*y), self.font(*font), self.col(*color), esc(text));
                self.out += &s;
            }
            Prim::Group(g) => self.group(g),
        }
    }
}

/// The whole scene as one dark SVG (debugging, quick checks).
const DEFS: &str = "<defs><filter id=\"halo3\" x=\"-50%\" y=\"-50%\" width=\"200%\" height=\"200%\"><feGaussianBlur stdDeviation=\"13.6\"/></filter></defs>";

pub fn scene_svg(scene: &Scene, m: &dyn Measure) -> Option<String> {
    let b = bbox(&scene.root, m, 0)?;
    let pad = 24.0;
    let (x0, y0, w, h) = (b[0] - pad, b[1] - pad, b[2] - b[0] + 2.0 * pad, b[3] - b[1] + 2.0 * pad);
    let mut wr = Writer { light: false, skip: tag::TRANSIENT, m, out: String::new() };
    wr.out += &format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\" viewBox=\"{} {} {} {}\" font-family=\"{}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{}\"/>",
        w.ceil(),
        h.ceil(),
        x0,
        y0,
        w.ceil(),
        h.ceil(),
        esc(FONT_SANS),
        x0,
        y0,
        w.ceil(),
        h.ceil(),
        c::BG.css()
    );
    wr.out += DEFS;
    wr.group(&scene.root);
    wr.out += "</svg>";
    Some(wr.out)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LegendKey {
    Association,
    Directed,
    Inheritance,
    Realization,
    Dependency,
    Composition,
    Aggregation,
    Cycle,
    Violation,
    Proposal,
}

pub const LEGEND_ORDER: [LegendKey; 10] = [
    LegendKey::Association,
    LegendKey::Directed,
    LegendKey::Inheritance,
    LegendKey::Realization,
    LegendKey::Dependency,
    LegendKey::Composition,
    LegendKey::Aggregation,
    LegendKey::Cycle,
    LegendKey::Violation,
    LegendKey::Proposal,
];

impl LegendKey {
    pub fn label(self) -> &'static str {
        match self {
            LegendKey::Association => "association",
            LegendKey::Directed => "navigable association",
            LegendKey::Inheritance => "inheritance",
            LegendKey::Realization => "realization",
            LegendKey::Dependency => "dependency",
            LegendKey::Composition => "composition",
            LegendKey::Aggregation => "aggregation",
            LegendKey::Cycle => "dependency cycle",
            LegendKey::Violation => "rule violation",
            LegendKey::Proposal => "proposed",
        }
    }
}

/// A short sample of a relation's notation, 40px wide, centred on y.
fn legend_sample(k: LegendKey, x: f64, y: f64) -> Group {
    use crate::scene::{solid, stroke as st};
    let mut g = Group::new();
    let color = match k {
        LegendKey::Cycle => c::CYCLE,
        LegendKey::Violation => c::WARN,
        LegendKey::Proposal => c::GHOST,
        _ => c::EDGE,
    };
    let dashed = matches!(k, LegendKey::Dependency | LegendKey::Realization | LegendKey::Proposal);
    let (x1, x2) = (x, x + 40.0);
    let (mut start, mut end) = (x1, x2);
    let mut deco = Group::new();
    if matches!(k, LegendKey::Composition | LegendKey::Aggregation) {
        deco.poly(vec![[x1, y], [x1 + 8.0, y - 5.0], [x1 + 16.0, y], [x1 + 8.0, y + 5.0]], true, Some(solid(if k == LegendKey::Composition { color } else { c::BG })), Some(st(color, 1.3)));
        start = x1 + 16.0;
    }
    if matches!(k, LegendKey::Inheritance | LegendKey::Realization) {
        deco.poly(vec![[x2, y], [x2 - 11.0, y - 6.0], [x2 - 11.0, y + 6.0]], true, Some(solid(c::BG)), Some(st(color, 1.3)));
        end = x2 - 11.0;
    }
    if matches!(k, LegendKey::Directed | LegendKey::Dependency | LegendKey::Cycle | LegendKey::Violation) {
        deco.poly(vec![[x2 - 8.0, y - 5.0], [x2, y], [x2 - 8.0, y + 5.0]], false, None, Some(st(color, 1.3)));
    }
    let s = st(color, if k == LegendKey::Cycle { 2.0 } else { 1.4 });
    g.line([start, y], [end, y], if dashed { s.dash(6.0, 4.0) } else { s });
    g.add(deco);
    g
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ExportOptions {
    /// "light" | "dark"
    pub theme: String,
    /// "solid" | "transparent"
    pub background: String,
    pub title: bool,
    pub date: bool,
    pub health: bool,
    pub legend: bool,
    pub ghosts: bool,
    pub diagnostics: bool,
    pub packages: bool,
    /// "compact" | "comfortable"
    pub padding: String,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self { theme: "light".into(), background: "solid".into(), title: true, date: true, health: false, legend: true, ghosts: false, diagnostics: false, packages: true, padding: "comfortable".into() }
    }
}

pub struct ExportMeta {
    pub title: String,
    pub view: String,
    pub health: Option<u8>,
    pub legend: Vec<LegendKey>,
    /// pre-formatted date ("14 November 2025")
    pub date: String,
}

/// The complete diagram as a standalone SVG, in the calm state: no hover dimming, hit areas,
/// animations or interaction affordances. Returns its pixel size too.
pub fn build_export_svg(scene: &Scene, m: &dyn Measure, meta: &ExportMeta, o: &ExportOptions) -> Option<(String, u32, u32)> {
    let mut skip = tag::TRANSIENT;
    if !o.ghosts {
        skip |= tag::PROPOSED;
    }
    if !o.diagnostics {
        skip |= tag::DIAG;
    }
    if !o.packages {
        skip |= tag::PKG;
    }
    let b = bbox(&scene.root, m, skip)?;
    let (bx, by, bw, bh) = (b[0], b[1], b[2] - b[0], b[3] - b[1]);
    let pad = if o.padding == "compact" { 24.0 } else { 56.0 };
    let show_header = o.title || o.date || (o.health && meta.health.is_some());
    let header_h = if show_header {
        if o.title {
            58.0
        } else {
            30.0
        }
    } else {
        0.0
    };
    let legend_keys: Vec<LegendKey> = if o.legend {
        meta.legend.iter().copied().filter(|k| o.ghosts || *k != LegendKey::Proposal).filter(|k| o.diagnostics || !matches!(k, LegendKey::Cycle | LegendKey::Violation)).collect()
    } else {
        vec![]
    };
    // legend rows (wrap to the content width)
    let content_w = bw.max(360.0);
    let items: Vec<(LegendKey, f64)> = legend_keys.iter().map(|k| (*k, 40.0 + 8.0 + m.width(k.label(), f::SMALL) + 22.0)).collect();
    let mut rows: Vec<Vec<(LegendKey, f64)>> = vec![];
    for it in items {
        match rows.last_mut() {
            Some(row) if row.iter().map(|x| x.1).sum::<f64>() + it.1 <= content_w => row.push(it),
            _ => rows.push(vec![it]),
        }
    }
    let legend_h = if rows.is_empty() { 0.0 } else { 22.0 + rows.len() as f64 * 22.0 };
    let width = (content_w + 2.0 * pad).ceil();
    let height = (bh + 2.0 * pad + header_h + legend_h).ceil();
    let (x0, y0) = (bx - pad - (content_w - bw) / 2.0, by - pad - header_h);
    let light = o.theme == "light";
    let mut wr = Writer { light, skip, m, out: String::new() };
    let n = Writer::n;
    wr.out += &format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"{} {} {width} {height}\" font-family=\"{}\">",
        n(x0),
        n(y0),
        esc(FONT_SANS)
    );
    wr.out += DEFS;
    if o.background == "solid" {
        let s = format!("<rect x=\"{}\" y=\"{}\" width=\"{width}\" height=\"{height}\" fill=\"{}\"/>", n(x0), n(y0), wr.col(c::BG));
        wr.out += &s;
    }
    let left = x0 + pad;
    let mut extra = Group::new();
    if show_header {
        let mut y = y0 + pad + 4.0;
        if o.title {
            extra.text(left, y + 16.0, meta.title.clone(), FontSpec::sans(20.0).bold(), c::TEXT, Anchor::Start);
            y += 30.0;
        }
        let parts: Vec<&str> = [Some(meta.view.as_str()), o.date.then_some(meta.date.as_str())].into_iter().flatten().filter(|s| !s.is_empty()).collect();
        let parts = parts.join("  ·  ");
        extra.text(left, y + 10.0, parts.clone(), FontSpec::sans(12.0), c::DIM, Anchor::Start);
        if let (true, Some(h)) = (o.health, meta.health) {
            let tx = left + m.width(&parts, FontSpec::sans(12.0)) + if parts.is_empty() { 0.0 } else { 14.0 };
            extra.text(tx, y + 10.0, "health ", FontSpec::sans(12.0), c::DIM, Anchor::Start);
            extra.text(tx + m.width("health ", FontSpec::sans(12.0)), y + 10.0, format!("{h}/100"), FontSpec::sans(12.0).bold(), health_color(h as f64), Anchor::Start);
        }
        let ly = y0 + pad + header_h - 10.0;
        extra.line([left, ly], [left + content_w, ly], crate::scene::stroke(c::BORDER, 0.8).opacity(0.6));
    }
    let mut legend = Group::new();
    if !rows.is_empty() {
        let mut y = by + bh + pad / 2.0 + 14.0;
        legend.line([left, y - 12.0], [left + content_w, y - 12.0], crate::scene::stroke(c::BORDER, 0.8).opacity(0.6));
        for row in &rows {
            let mut x = left;
            for (k, w) in row {
                legend.add(legend_sample(*k, x, y + 6.0));
                legend.text(x + 48.0, y + 10.0, k.label(), f::SMALL, c::DIM, Anchor::Start);
                x += w;
            }
            y += 22.0;
        }
    }
    wr.group(&extra);
    wr.group(&scene.root);
    wr.group(&legend);
    wr.out += "</svg>";
    Some((wr.out, width as u32, height as u32))
}
