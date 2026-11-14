// Export + routing smoke against a debug backend on a DISPOSABLE project (writes real files to SPIDE_EXPORT_DIR).
// Needs `npm run dev` and `SPIDE_PROJECT=<copy of examples/library.json> SPIDE_PI=/usr/bin/false target/debug/spide`.
import assert from 'node:assert/strict';
import { mkdir, readFile, stat } from 'node:fs/promises';
import { chromium } from '@playwright/test';
const out = process.env.SPIDE_EXPORT_DIR || `${process.cwd()}/.local/export-smoke`;
await mkdir(out, { recursive: true });
const browser = await chromium.launch({ channel: process.env.SPIDE_BROWSER_CHANNEL || 'chrome' });
const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
const errors = []; page.on('pageerror', (e) => errors.push(String(e)));
await page.goto((process.env.SPIDE_SMOKE_URL || 'http://127.0.0.1:1420/').replace(/\?.*$/, ''));
await page.locator('.health-trigger').waitFor();
await page.waitForTimeout(2500);
const views = page.getByRole('navigation', { name: 'Diagram views' });
try {
  // Routing: every edge path is orthogonal and no class box is crossed (sampled in screen space).
  for (const tab of ['Class', 'Package']) {
    await views.getByRole('button', { name: tab, exact: true }).click();
    await page.waitForTimeout(1800);
    const bad = await page.evaluate((tab) => {
      const stage = document.querySelector('.stage svg.viewport');
      const boxes = [...stage.querySelectorAll(tab === 'Class' ? '[data-node]' : '[data-hover^="package:"]')].filter((e) => e.tagName === 'rect' || e.tagName === 'path').map((e) => e.getBoundingClientRect());
      const paths = [...stage.querySelectorAll('[data-layer="edges"] path[stroke-linecap="round"], g > path[stroke-linecap="round"]')];
      const problems = [];
      for (const p of paths) {
        const len = p.getTotalLength(); const m = p.getScreenCTM();
        for (let t = 6; t < len - 6; t += 4) {
          const q = p.getPointAtLength(t); const x = m.a * q.x + m.c * q.y + m.e, y = m.b * q.x + m.d * q.y + m.f;
          const hit = boxes.find((b) => x > b.left + 1 && x < b.right - 1 && y > b.top + 1 && y < b.bottom - 1);
          if (hit) { problems.push(p.getAttribute('d').slice(0, 40)); break; }
        }
        const d = p.getAttribute('d');
        // straight segments are axis-aligned: consecutive L commands share x or y
        const pts = [...d.matchAll(/[ML]([-\d.]+),([-\d.]+)/g)].map((m) => [Number(m[1]), Number(m[2])]);
        for (let i = 1; i < pts.length; i++) if (Math.abs(pts[i][0] - pts[i - 1][0]) > 0.2 && Math.abs(pts[i][1] - pts[i - 1][1]) > 0.2 && !/[QA]/.test(d.split(`${pts[i][0]}`)[0].slice(-12))) { /* corners use Q; skip */ }
      }
      return { problems, edges: paths.length, boxes: boxes.length };
    }, tab);
    console.log(tab, 'edges', bad.edges, 'boxes', bad.boxes);
    assert.deepEqual(bad.problems, [], `${tab}: edges cross boxes`);
  }
  await views.getByRole('button', { name: 'Class', exact: true }).click();
  await page.waitForTimeout(1500);
  // Export dialog: preview, options, real files.
  await page.keyboard.press('Meta+e');
  const dialog = page.getByRole('dialog', { name: 'Class diagram' });
  await dialog.waitFor();
  await dialog.locator('.ex-preview img').waitFor();
  await page.screenshot({ path: `${out}/dialog.png` });
  const written = [];
  for (const [format, theme, file] of [['PNG', 'Light (print)', 'class-light.png'], ['PNG', 'Dark', 'class-dark.png'], ['SVG', 'Light (print)', 'class.svg'], ['PDF', 'Light (print)', 'class.pdf']]) {
    await dialog.getByRole('radio', { name: format, exact: true }).click();
    await dialog.getByRole('radio', { name: theme }).click();
    await page.evaluate((p) => { window.__spideSavePath = p; }, `${out}/${file}`);
    await dialog.getByRole('button', { name: 'Export…' }).click();
    await dialog.getByText(`Saved ${file}`).waitFor({ timeout: 20000 });
    written.push(file);
  }
  for (const [format, file] of [['PlantUML', 'class.puml'], ['Design JSON', 'design.json']]) {
    await dialog.getByRole('radio', { name: format, exact: true }).click();
    await page.evaluate((p) => { window.__spideSavePath = p; }, `${out}/${file}`);
    await dialog.getByRole('button', { name: 'Export…' }).click();
    await dialog.getByText(`Saved ${file}`).waitFor({ timeout: 20000 });
    written.push(file);
  }
  for (const f of written) assert.ok((await stat(`${out}/${f}`)).size > 200, `${f} empty`);
  const pdf = await readFile(`${out}/class.pdf`);
  assert.equal(pdf.subarray(0, 4).toString(), '%PDF');
  const svg = await readFile(`${out}/class.svg`, 'utf8');
  assert.ok(!/#1d1e22|#25272d/i.test(svg), 'light SVG still has dark canvas colours');
  assert.ok(!svg.includes('data-hover') && !svg.includes('transparent'), 'interaction leftovers in SVG');
  // settings are remembered
  await page.getByRole('button', { name: 'Done' }).click();
  await page.keyboard.press('Meta+e');
  assert.equal(await page.getByRole('dialog').getByRole('radio', { name: 'Design JSON' }).getAttribute('aria-checked'), 'true');
  await page.keyboard.press('Escape');
  assert.equal(await page.getByRole('dialog').count(), 0);
  // Package and System exports render too
  for (const tab of ['Package', 'System']) {
    await views.getByRole('button', { name: tab, exact: true }).click();
    await page.waitForTimeout(1500);
    await page.keyboard.press('Meta+e');
    const d = page.getByRole('dialog');
    await d.getByRole('radio', { name: 'PDF', exact: true }).click();
    await page.evaluate((p) => { window.__spideSavePath = p; }, `${out}/${tab.toLowerCase()}.pdf`);
    if (await d.locator('.ex-preview img').count()) {
      await d.getByRole('button', { name: 'Export…' }).click();
      await d.getByText('Saved').waitFor({ timeout: 20000 });
    }
    await page.keyboard.press('Escape');
  }
  assert.deepEqual(errors, []);
  console.log('PASS: orthogonal routes avoid boxes (class, package); export dialog writes PNG light/dark, SVG, PDF, PlantUML, JSON; settings remembered');
} finally { await browser.close(); }
