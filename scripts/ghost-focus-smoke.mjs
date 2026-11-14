import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { chromium } from '@playwright/test';
// Hovering a ghost spotlights its proposal; the proposal dock locates proposals. Needs `npm run dev`; all traffic mocked.
const snap = process.env.SPIDE_SMOKE_SNAPSHOT ? JSON.parse(await readFile(process.env.SPIDE_SMOKE_SNAPSHOT, 'utf8')) : await (await fetch('http://127.0.0.1:47800/dev/snapshot')).json();
{
  const v = snap.view, base = v.classes.find((c) => c.name === 'LendingService');
  const infra = v.packages.find((p) => p.name.includes('infra'))?.name ?? base.package;
  v.classes.push({ ...structuredClone(base), name: 'AuditTrail', status: 'proposed', proposal: 'p1', origin: null, package: infra, attributes: [], methods: [] });
  v.relations.push({ id: 'rp1', from: 'LendingService', to: 'AuditTrail', kind: 'dependency', label: null, from_mult: null, to_mult: null, directed: true, interaction: 'sync', rationale: null, status: 'proposed', proposal: 'p1', feedback: null, origin: null });
  v.proposals = [{ id: 'p1', ops: [], rationale: 'Record every loan change for auditing.', source: 'agent' }, { id: 'p2', ops: [], rationale: null, source: 'agent' }];
}
const b = await chromium.launch({ channel: process.env.SPIDE_BROWSER_CHANNEL || 'chrome' }); const page = await b.newPage({ viewport: { width: 1440, height: 900 } });
const calls = []; const errors = []; page.on('pageerror', (e) => errors.push(String(e)));
await page.route('**/dev/snapshot', (r) => r.fulfill({ json: snap, headers: { 'access-control-allow-origin': '*' } }));
await page.route('**/dev/invoke/**', (r) => { calls.push(r.request().url().split('/').at(-1)); r.fulfill({ json: null, headers: { 'access-control-allow-origin': '*' } }); });
await page.goto((process.env.SPIDE_SMOKE_URL || 'http://127.0.0.1:1420/').replace(/\?.*$/, '')); await page.waitForTimeout(2500);
const opacity = (n) => page.locator(`[data-node="${n}"]`).evaluate((e) => Number(e.closest('g[opacity]').getAttribute('opacity')));
const o0 = await opacity('Catalog');
const ghost = page.locator('.stage [data-hover="class:AuditTrail"]'); const bb = await ghost.boundingBox();
await page.mouse.move(bb.x + bb.width / 2, bb.y + 10); await page.locator('.hovercard').waitFor();
await page.waitForTimeout(300);
const card = await page.locator('.hovercard').innerText();
assert.match(card, /in infrastructure · connects to LendingService \(application\)/); assert.match(card, /2 changes/i);
assert.ok(await opacity('Catalog') < o0 * 0.5, 'unrelated class not dimmed');
assert.ok(await opacity('LendingService') >= o0, 'anchor dimmed');
await page.screenshot({ path: '.local/ui-audit/ghost-hover.png' });
await page.mouse.move(1040, 200); await page.waitForTimeout(400);
assert.equal(await opacity('Catalog'), o0, 'dim persisted after leaving');
// proposal bar: hover spotlights all, Locate steps & zooms
await page.locator('.proposal-bar .pg').hover(); await page.waitForTimeout(250);
assert.ok(await opacity('Catalog') < o0 * 0.5, 'bar hover did not spotlight');
const t0 = await page.locator('.stage svg.viewport > g[data-world]').getAttribute('transform');
await page.getByRole('button', { name: 'Next proposal' }).click(); await page.waitForTimeout(700);
assert.notEqual(await page.locator('.stage svg.viewport > g[data-world]').getAttribute('transform'), t0, 'locate did not move');
await page.screenshot({ path: '.local/ui-audit/ghost-locate.png' });
assert.ok(!calls.includes('accept_proposal') && !calls.includes('reject_proposal'), 'hover/locate mutated proposals');
assert.deepEqual(errors, []); console.log('PASS: ghost hover spotlight, location in card, dock spotlight and locate');
await b.close();
