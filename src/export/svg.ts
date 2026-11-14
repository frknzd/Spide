// Serialise what a diagram shows into a standalone, print-quality SVG (PNG via resvg, PDF via svg2pdf in Rust).
import { C, F, FONT_SANS, healthColor, textWidth } from "../canvas/visuals";
import type { ViewportApi } from "../canvas/Viewport";
import { applyLightTheme } from "./theme";

const NS = "http://www.w3.org/2000/svg";

export type LegendKey = "association" | "directed" | "inheritance" | "realization" | "dependency" | "composition" | "aggregation" | "cycle" | "violation" | "proposal";
export interface ExportMeta { title: string; view: string; health?: number | null; legend: LegendKey[] }
export interface ExportOptions {
  theme: "light" | "dark";
  background: "solid" | "transparent";
  title: boolean;
  date: boolean;
  health: boolean;
  legend: boolean;
  ghosts: boolean;
  diagnostics: boolean;
  packages: boolean;
  padding: "compact" | "comfortable";
}
export const DEFAULT_EXPORT: ExportOptions = {
  theme: "light", background: "solid", title: true, date: true, health: false, legend: true, ghosts: false, diagnostics: false, packages: true, padding: "comfortable",
};

const el = (tag: string, attrs: Record<string, string | number>, text?: string) => {
  const e = document.createElementNS(NS, tag);
  for (const [k, v] of Object.entries(attrs)) e.setAttribute(k, String(v));
  if (text !== undefined) e.textContent = text;
  return e;
};

const LEGEND: Record<LegendKey, string> = {
  association: "association", directed: "navigable association", inheritance: "inheritance", realization: "realization",
  dependency: "dependency", composition: "composition", aggregation: "aggregation", cycle: "dependency cycle", violation: "rule violation", proposal: "proposed",
};

/** A short sample of a relation's notation, 40px wide, centred on y. */
function legendSample(k: LegendKey, x: number, y: number): Element {
  const g = el("g", {});
  const color = k === "cycle" ? C.cycle : k === "violation" ? C.warn : k === "proposal" ? C.ghost : C.edge;
  const dashed = k === "dependency" || k === "realization" || k === "proposal";
  const x1 = x, x2 = x + 40;
  let start = x1, end = x2;
  if (k === "composition" || k === "aggregation") {
    g.appendChild(el("polygon", { points: `${x1},${y} ${x1 + 8},${y - 5} ${x1 + 16},${y} ${x1 + 8},${y + 5}`, fill: k === "composition" ? color : C.bg, stroke: color, "stroke-width": 1.3 }));
    start = x1 + 16;
  }
  if (k === "inheritance" || k === "realization") {
    g.appendChild(el("polygon", { points: `${x2},${y} ${x2 - 11},${y - 6} ${x2 - 11},${y + 6}`, fill: C.bg, stroke: color, "stroke-width": 1.3 }));
    end = x2 - 11;
  }
  if (k === "directed" || k === "dependency" || k === "cycle" || k === "violation") {
    g.appendChild(el("polyline", { points: `${x2 - 8},${y - 5} ${x2},${y} ${x2 - 8},${y + 5}`, fill: "none", stroke: color, "stroke-width": 1.3, "stroke-linejoin": "round" }));
  }
  g.insertBefore(el("line", { x1: start, y1: y, x2: end, y2: y, stroke: color, "stroke-width": k === "cycle" ? 2 : 1.4, ...(dashed ? { "stroke-dasharray": "6 4" } : {}), "stroke-linecap": "round" }), g.firstChild);
  return g;
}

/**
 * The complete diagram (not just the visible part) as a standalone SVG, in the calm state:
 * no hover dimming, hit areas, animations or interaction classes. Returns its pixel size too.
 */
export function buildExportSvg(vp: ViewportApi, meta: ExportMeta, o: ExportOptions): { svg: string; width: number; height: number } | null {
  const svg = vp.svg();
  const world = vp.world();
  if (!svg || !world) return null;
  const clone = world.cloneNode(true) as SVGGElement;
  clone.removeAttribute("transform");
  clone.querySelectorAll('[stroke="transparent"], [fill="transparent"], title').forEach((n) => n.remove());
  // hover-only affordances (visibility tags, reveal chips) never belong in a picture
  clone.querySelectorAll("[data-transient]").forEach((n) => n.remove());
  if (!o.ghosts) clone.querySelectorAll('[data-status="proposed"]').forEach((n) => n.remove());
  if (!o.diagnostics) clone.querySelectorAll("[data-diag]").forEach((n) => n.remove());
  if (!o.packages) clone.querySelectorAll("[data-pkg]").forEach((n) => n.remove());
  for (const n of [clone, ...clone.querySelectorAll("*")]) {
    for (const a of ["class", "style", "data-target", "data-hover", "data-node", "data-status", "data-diag", "data-pkg", "pointer-events", "data-world", "data-finding"]) n.removeAttribute(a);
  }
  const defs = svg.querySelector("defs")?.cloneNode(true) as SVGDefsElement | undefined;
  defs?.querySelector("#dots")?.remove();

  // measure what's left
  const probe = document.createElementNS(NS, "svg");
  probe.setAttribute("style", "position:absolute;left:-99999px;top:0;visibility:hidden");
  if (defs) probe.appendChild(defs.cloneNode(true));
  probe.appendChild(clone);
  document.body.appendChild(probe);
  const b = clone.getBBox();
  probe.remove();
  if (b.width === 0 || b.height === 0) return null;

  const pad = o.padding === "compact" ? 24 : 56;
  const showHeader = o.title || o.date || (o.health && meta.health != null);
  const headerH = showHeader ? (o.title ? 58 : 30) : 0;
  const legendKeys = o.legend ? meta.legend.filter((k) => o.ghosts || k !== "proposal").filter((k) => o.diagnostics || (k !== "cycle" && k !== "violation")) : [];
  // legend rows (wrap to the content width)
  const contentW = Math.max(b.width, 360);
  const items = legendKeys.map((k) => ({ k, w: 40 + 8 + textWidth(LEGEND[k], F.small) + 22 }));
  const rows: { k: LegendKey; w: number }[][] = [];
  for (const it of items) {
    const row = rows[rows.length - 1];
    if (row && row.reduce((t, x) => t + x.w, 0) + it.w <= contentW) row.push(it); else rows.push([it]);
  }
  const legendH = rows.length ? 22 + rows.length * 22 : 0;
  const width = Math.ceil(contentW + 2 * pad);
  const height = Math.ceil(b.height + 2 * pad + headerH + legendH);
  const x0 = b.x - pad - (contentW - b.width) / 2, y0 = b.y - pad - headerH;

  const root = document.createElementNS(NS, "svg");
  root.setAttribute("xmlns", NS);
  root.setAttribute("width", String(width));
  root.setAttribute("height", String(height));
  root.setAttribute("viewBox", `${x0} ${y0} ${width} ${height}`);
  root.setAttribute("font-family", FONT_SANS);
  if (defs) root.appendChild(defs);
  if (o.background === "solid") root.appendChild(el("rect", { x: x0, y: y0, width, height, fill: C.bg }));
  const left = x0 + pad;
  if (showHeader) {
    let y = y0 + pad + 4;
    if (o.title) { root.appendChild(el("text", { x: left, y: y + 16, "font-size": 20, "font-weight": 600, fill: C.text, "font-family": FONT_SANS }, meta.title)); y += 30; }
    const parts = [meta.view, o.date ? new Date().toLocaleDateString(undefined, { year: "numeric", month: "long", day: "numeric" }) : null].filter(Boolean).join("  ·  ");
    const sub = el("text", { x: left, y: y + 10, "font-size": 12, fill: C.dim, "font-family": FONT_SANS }, parts);
    root.appendChild(sub);
    if (o.health && meta.health != null) {
      const tx = left + textWidth(parts, `12px ${FONT_SANS}`) + (parts ? 14 : 0);
      root.appendChild(el("text", { x: tx, y: y + 10, "font-size": 12, fill: C.dim, "font-family": FONT_SANS }, "health "));
      root.appendChild(el("text", { x: tx + textWidth("health ", `12px ${FONT_SANS}`), y: y + 10, "font-size": 12, "font-weight": 700, fill: healthColor(meta.health), "font-family": FONT_SANS }, `${meta.health}/100`));
    }
    root.appendChild(el("line", { x1: left, x2: left + contentW, y1: y0 + pad + headerH - 10, y2: y0 + pad + headerH - 10, stroke: C.border, "stroke-width": 0.8, opacity: 0.6 }));
  }
  root.appendChild(clone);
  if (rows.length) {
    let y = b.y + b.height + pad / 2 + 14;
    root.appendChild(el("line", { x1: left, x2: left + contentW, y1: y - 12, y2: y - 12, stroke: C.border, "stroke-width": 0.8, opacity: 0.6 }));
    for (const row of rows) {
      let x = left;
      for (const it of row) {
        root.appendChild(legendSample(it.k, x, y + 6));
        root.appendChild(el("text", { x: x + 48, y: y + 10, "font-size": 10.5, fill: C.dim, "font-family": FONT_SANS }, LEGEND[it.k]));
        x += it.w;
      }
      y += 22;
    }
  }
  if (o.theme === "light") applyLightTheme(root);
  return { svg: new XMLSerializer().serializeToString(root), width, height };
}

/** Dark, full-detail SVG of what's on screen (dev hook and quick exports). */
export function standaloneSvg(vp: ViewportApi, title?: string): string | null {
  return buildExportSvg(vp, { title: title ?? "", view: "", legend: [] }, { ...DEFAULT_EXPORT, theme: "dark", title: !!title, date: false, legend: false, ghosts: true, diagnostics: true })?.svg ?? null;
}
