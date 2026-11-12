// Serialise what the canvas shows into a standalone SVG (rendered to PNG by resvg in Rust).
import { C, FONT_SANS } from "../canvas/visuals";
import type { ViewportApi } from "../canvas/Viewport";

export function standaloneSvg(vp: ViewportApi, title?: string): string | null {
  const svg = vp.svg();
  const world = vp.world();
  if (!svg || !world) return null;
  const b = world.getBBox();
  if (b.width === 0 || b.height === 0) return null;
  const pad = 40;
  const titleH = title ? 34 : 0;
  const x = b.x - pad;
  const y = b.y - pad - titleH;
  const w = b.width + 2 * pad;
  const h = b.height + 2 * pad + titleH;

  const clone = world.cloneNode(true) as SVGGElement;
  clone.removeAttribute("transform");
  // drop invisible hit areas & interaction-only bits
  clone.querySelectorAll('[stroke="transparent"], [fill="transparent"]').forEach((n) => n.remove());
  clone.querySelectorAll("[class]").forEach((n) => n.removeAttribute("class"));
  clone.querySelectorAll("[style]").forEach((n) => n.removeAttribute("style"));
  clone.querySelectorAll("[data-target],[data-hover],[data-node]").forEach((n) => {
    n.removeAttribute("data-target");
    n.removeAttribute("data-hover");
    n.removeAttribute("data-node");
  });
  const defs = svg.querySelector("defs")?.cloneNode(true) as SVGDefsElement | undefined;
  defs?.querySelector("#dots")?.remove();

  const ser = new XMLSerializer();
  const body = ser.serializeToString(clone);
  const defsStr = defs ? ser.serializeToString(defs) : "";
  const titleStr = title
    ? `<text x="${x + pad}" y="${y + pad + 6}" font-family="${FONT_SANS.replace(/'/g, "")}" font-size="16" font-weight="600" fill="${C.dim}">${escapeXml(title)}</text>`
    : "";
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${Math.ceil(w)}" height="${Math.ceil(h)}" viewBox="${x} ${y} ${w} ${h}">${defsStr}<rect x="${x}" y="${y}" width="${w}" height="${h}" fill="${C.bg}"/>${titleStr}${body}</svg>`;
}

function escapeXml(s: string) {
  return s.replace(/[<>&"]/g, (c) => ({ "<": "&lt;", ">": "&gt;", "&": "&amp;", '"': "&quot;" })[c]!);
}
