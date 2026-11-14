import assert from "node:assert/strict";
import test from "node:test";
import { lightColor, luminance } from "../src/export/theme.ts";

const contrast = (a: string, b: string) => { const [x, y] = [luminance(a), luminance(b)].sort((p, q) => q - p); return (x + 0.05) / (y + 0.05); };

test("canvas neutrals become paper and ink", () => {
  for (const dark of ["#1d1e22", "#25272d", "#2b2e35", "#26282e", "#191b20", "rgb(30,31,36)"]) assert.ok(luminance(lightColor(dark)) > 0.7, `${dark} → ${lightColor(dark)}`);
  for (const text of ["#e6e8ee", "#9aa1b1"]) assert.ok(contrast(lightColor(text), "#ffffff") >= 4.5, `${text} readable on paper`);
});

test("semantic colours keep their hue but are readable on white", () => {
  for (const c of ["#ff5c6c", "#ffb454", "#5eead4", "#7aa2ff", "#4ade80", "#c084fc", "#f472b6", "#60a5fa", "#34d399", "rgb(255,180,84)"]) {
    const l = lightColor(c);
    assert.ok(contrast(l, "#ffffff") >= 3, `${c} → ${l} too faint`);
    assert.notEqual(l, c);
  }
});

test("non-colours pass through", () => {
  for (const v of ["none", "transparent", "url(#halo3)", "currentColor"]) assert.equal(lightColor(v), v);
});
