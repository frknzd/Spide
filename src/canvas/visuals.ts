// Shared visual vocabulary: fonts, colours, metric → visual mappings, geometry helpers.

import type { Attribute, Class, ClassMetrics, Layer, Method, Status, Visibility } from "../state/types";

export const FONT_SANS = "'Helvetica Neue', Helvetica, Arial, sans-serif";
export const FONT_MONO = "Menlo, Monaco, monospace";

export const C = {
  bg: "#1d1e22",
  grid: "#26282e",
  node: "#25272d",
  nodeHeader: "#2b2e35",
  border: "#4b5263",
  text: "#e6e8ee",
  dim: "#9aa1b1",
  faint: "#5d6474",
  edge: "#8a93a6",
  cycle: "#ff5c6c",
  warn: "#ffb454",
  ghost: "#5eead4",
  removing: "#ff7a85",
  accent: "#7aa2ff",
  good: "#4ade80",
};

export const LAYER_COLOR: Record<Layer, string> = {
  ui: "#c084fc",
  infrastructure: "#f472b6",
  application: "#60a5fa",
  domain: "#34d399",
};

const canvas = typeof document !== "undefined" ? document.createElement("canvas") : null;
const ctx = canvas?.getContext("2d") ?? null;
const cache = new Map<string, number>();

export function textWidth(text: string, font: string): number {
  const key = font + "|" + text;
  let w = cache.get(key);
  if (w === undefined) {
    if (ctx) {
      ctx.font = font;
      w = ctx.measureText(text).width;
    } else {
      w = text.length * 7;
    }
    cache.set(key, w);
  }
  return w;
}

export const F = {
  name: `600 14px ${FONT_SANS}`,
  nameItalic: `italic 600 14px ${FONT_SANS}`,
  stereo: `11px ${FONT_SANS}`,
  member: `12px ${FONT_MONO}`,
  small: `10.5px ${FONT_SANS}`,
};

export const VIS: Record<Visibility, string> = { public: "+", private: "−", protected: "#", package: "~" };

export function attrText(a: Attribute): string {
  let s = `${VIS[a.visibility] ?? "+"} ${a.name}`;
  if (a.type) s += `: ${a.type}`;
  if (a.multiplicity) s += ` [${a.multiplicity}]`;
  return s;
}

export function methodText(m: Method): string {
  const params = m.params.map((p) => (p.type ? `${p.name}: ${p.type}` : p.name)).join(", ");
  let s = `${VIS[m.visibility] ?? "+"} ${m.name}(${params})`;
  if (m.returns) s += `: ${m.returns}`;
  return s;
}

export const LINE = 17;
export const PAD_X = 12;
const MAX_TEXT = 360;

export interface Row {
  text: string;
  kind: "attribute" | "method";
  name: string;
  status: Status;
  isStatic: boolean;
  isAbstract: boolean;
  y: number;
}

export interface ClassBox {
  w: number;
  h: number;
  headerH: number;
  attrY: number;
  methY: number;
  rows: Row[];
  stereo: string | null;
}

export function stereotypeText(c: { stereotype: string | null; is_abstract?: boolean }): string | null {
  const s = c.stereotype;
  if (!s || s === "abstract") return null;
  return `«${s}»`;
}

export function truncate(text: string, font: string, max = MAX_TEXT): string {
  if (textWidth(text, font) <= max) return text;
  let t = text;
  while (t.length > 4 && textWidth(t + "…", font) > max) t = t.slice(0, -1);
  return t + "…";
}

export function classBox(
  c: Pick<Class, "name" | "stereotype" | "is_abstract"> & { attributes: Attribute[]; methods?: Method[] },
  opts: { methods?: boolean } = {},
): ClassBox {
  const stereo = stereotypeText(c);
  const nameFont = c.is_abstract || c.stereotype === "interface" ? F.nameItalic : F.name;
  let w = textWidth(c.name, nameFont) + 2 * PAD_X + 8;
  if (stereo) w = Math.max(w, textWidth(stereo, F.stereo) + 2 * PAD_X);
  const headerH = stereo ? 40 : 30;
  const rows: Row[] = [];
  let y = headerH + 6;
  const attrY = headerH;
  for (const a of c.attributes) {
    const text = truncate(attrText(a), F.member);
    w = Math.max(w, textWidth(text, F.member) + 2 * PAD_X);
    rows.push({ text, kind: "attribute", name: a.name, status: a.status, isStatic: a.is_static, isAbstract: false, y: y + 12 });
    y += LINE;
  }
  if (c.attributes.length === 0) y += 6;
  y += 6;
  const methY = y;
  const showMethods = opts.methods !== false;
  if (showMethods) {
    y += 6;
    for (const m of c.methods ?? []) {
      const text = truncate(methodText(m), F.member);
      w = Math.max(w, textWidth(text, F.member) + 2 * PAD_X);
      rows.push({ text, kind: "method", name: m.name, status: m.status, isStatic: m.is_static, isAbstract: m.is_abstract, y: y + 12 });
      y += LINE;
    }
    if ((c.methods ?? []).length === 0) y += 6;
    y += 6;
  }
  return { w: Math.ceil(Math.max(w, 120)), h: Math.ceil(showMethods ? y : methY), headerH, attrY, methY, rows, stereo };
}

// ---------------------------------------------------------------- metric → visuals

function hex(c: string): [number, number, number] {
  const n = parseInt(c.slice(1), 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}
function mix(a: string, b: string, t: number): string {
  const [r1, g1, b1] = hex(a);
  const [r2, g2, b2] = hex(b);
  const f = (x: number, y: number) => Math.round(x + (y - x) * t);
  return `rgb(${f(r1, r2)},${f(g1, g2)},${f(b1, b2)})`;
}

/** blue → amber → red */
export function heatColor(h: number): string {
  const t = Math.max(0, Math.min(1, h));
  return t < 0.5 ? mix("#5b8cff", "#ffb454", t / 0.5) : mix("#ffb454", "#ff5c6c", (t - 0.5) / 0.5);
}

export function healthColor(score: number): string {
  if (score >= 85) return C.good;
  if (score >= 65) return "#a3e635";
  if (score >= 45) return C.warn;
  return C.cycle;
}

export function heatBucket(h: number): number {
  return Math.max(0, Math.min(5, Math.round(h * 5)));
}

export function nodeScale(m: ClassMetrics | undefined): number {
  if (!m) return 1;
  return 1 + Math.min(m.fan_in + m.fan_out, 10) * 0.018 + (m.god ? 0.05 : 0);
}

export function badges(m: ClassMetrics | undefined): { text: string; color: string; title: string }[] {
  if (!m) return [];
  const out: { text: string; color: string; title: string }[] = [];
  if (m.in_cycle) out.push({ text: "cycle", color: C.cycle, title: "Part of a dependency cycle" });
  if (m.god) out.push({ text: "god class", color: C.cycle, title: m.issues.join("; ") });
  else if (m.lcom !== null && m.lcom >= 2) out.push({ text: `LCOM ${m.lcom}`, color: C.warn, title: "Low cohesion: unrelated method groups" });
  if (m.layer_violations > 0) out.push({ text: "layer ↑", color: C.warn, title: "Depends outward across layers" });
  if (m.orphan) out.push({ text: "orphan", color: C.faint, title: "Not connected" });
  return out;
}

// ---------------------------------------------------------------- geometry

export interface Rect { x: number; y: number; w: number; h: number } // centre x,y

/** Point where the ray from the rect centre towards (tx,ty) leaves the rect. */
export function borderPoint(r: Rect, tx: number, ty: number, pad = 0): [number, number] {
  const dx = tx - r.x;
  const dy = ty - r.y;
  if (dx === 0 && dy === 0) return [r.x, r.y];
  const hw = r.w / 2 + pad;
  const hh = r.h / 2 + pad;
  const s = Math.min(hw / Math.abs(dx || 1e-9), hh / Math.abs(dy || 1e-9));
  return [r.x + dx * s, r.y + dy * s];
}

export function statusOpacity(s: Status): number {
  switch (s) {
    case "proposed": return 0.62;
    case "removing": return 0.5;
    case "pruned": return 0.22;
    default: return 1;
  }
}
