// Light "print" theme for exports: rewrite the dark canvas palette on paper.
// Known canvas tokens map exactly; anything else keeps its hue: neutrals invert, colours darken for contrast on white.

const EXACT: Record<string, string> = {
  "#1d1e22": "#ffffff", // bg (also label plates, chip fills)
  "#25272d": "#ffffff", // node
  "#2b2e35": "#f2f4f8", // node header
  "#26282e": "#eceff4", // grid
  "#4b5263": "#8b93a5", // border
  "#e6e8ee": "#1b2030", // text
  "#9aa1b1": "#4b5365", // dim
  "#5d6474": "#7b8395", // faint
  "#8a93a6": "#5a6378", // edge
  "#24314f": "#e8eefc", // C4 system fill
  "#30343d": "#eef0f4",
  "#ffffff": "#1b2030", // "white" accents on dark become ink
};

type RGB = [number, number, number];
export function parseColor(c: string): RGB | null {
  const s = c.trim().toLowerCase();
  let m = s.match(/^#([0-9a-f]{6})$/);
  if (m) return [0, 2, 4].map((i) => parseInt(m![1].slice(i, i + 2), 16)) as RGB;
  m = s.match(/^#([0-9a-f]{3})$/);
  if (m) return [0, 1, 2].map((i) => parseInt(m![1][i] + m![1][i], 16)) as RGB;
  m = s.match(/^rgba?\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)/);
  if (m) return [Number(m[1]), Number(m[2]), Number(m[3])];
  return null;
}
const hex = (c: RGB) => "#" + c.map((v) => Math.round(Math.max(0, Math.min(255, v))).toString(16).padStart(2, "0")).join("");

function toHsl([r, g, b]: RGB): [number, number, number] {
  r /= 255; g /= 255; b /= 255;
  const max = Math.max(r, g, b), min = Math.min(r, g, b), l = (max + min) / 2;
  if (max === min) return [0, 0, l];
  const d = max - min, s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
  const h = max === r ? (g - b) / d + (g < b ? 6 : 0) : max === g ? (b - r) / d + 2 : (r - g) / d + 4;
  return [h / 6, s, l];
}
function fromHsl([h, s, l]: [number, number, number]): RGB {
  if (s === 0) return [l * 255, l * 255, l * 255];
  const q = l < 0.5 ? l * (1 + s) : l + s - l * s, p = 2 * l - q;
  const f = (t: number) => { t = (t + 1) % 1; return t < 1 / 6 ? p + (q - p) * 6 * t : t < 1 / 2 ? q : t < 2 / 3 ? p + (q - p) * (2 / 3 - t) * 6 : p; };
  return [f(h + 1 / 3) * 255, f(h) * 255, f(h - 1 / 3) * 255];
}

/** The paper equivalent of a canvas colour. */
export function lightColor(c: string): string {
  const k = c.trim().toLowerCase();
  if (EXACT[k]) return EXACT[k];
  const rgb = parseColor(k);
  if (!rgb) return c; // none, transparent, url(#…), currentColor
  const [h, s, l] = toHsl(rgb);
  if (s < 0.18) return hex(fromHsl([h, s, Math.max(0.12, Math.min(0.97, 1.08 - l))])); // neutrals: invert lightness
  // colours: deepen until they read on white (≥ 3.4:1), keeping the hue
  let ll = Math.min(l, 0.5), out = hex(fromHsl([h, s, ll]));
  while (ll > 0.15 && (1.05 / (luminance(out) + 0.05)) < 3.4) { ll -= 0.02; out = hex(fromHsl([h, s, ll])); }
  return out;
}

const ATTRS = ["fill", "stroke", "stop-color", "flood-color", "color"];
/** Rewrite every colour attribute of an SVG subtree in place. */
export function applyLightTheme(root: Element) {
  const all = [root, ...root.querySelectorAll("*")];
  for (const el of all) for (const a of ATTRS) {
    const v = el.getAttribute(a);
    if (v) el.setAttribute(a, lightColor(v));
  }
}

/** Relative luminance (WCAG). */
export function luminance(c: string): number {
  const rgb = parseColor(c);
  if (!rgb) return 1;
  const [r, g, b] = rgb.map((v) => { v /= 255; return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4; });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}
