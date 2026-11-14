// UI audit regression: startup failure/retry, empty welcome, command palette, forgiving health popover, forms, geometry.
// Needs `npm run dev`. All backend traffic is mocked; nothing reaches a real design.
import assert from 'node:assert/strict';
import { readFile, mkdir } from 'node:fs/promises';
import { chromium } from '@playwright/test';
const base = process.env.SPIDE_SMOKE_SNAPSHOT ? JSON.parse(await readFile(process.env.SPIDE_SMOKE_SNAPSHOT, 'utf8')) : await (await fetch('http://127.0.0.1:47800/dev/snapshot')).json();
const url = (process.env.SPIDE_SMOKE_URL || 'http://127.0.0.1:1420/').replace(/\?.*$/, '');
const browser = await chromium.launch({ channel: process.env.SPIDE_BROWSER_CHANNEL || 'chrome', headless: true });
await mkdir('.local/ui-audit', { recursive: true });
const errors = [];
async function open(snap, { fail = 0, size = { width: 1440, height: 900 }, invoke = () => null } = {}) {
  const page = await browser.newPage({ viewport: size });
  page.on('pageerror', (e) => errors.push(String(e)));
  let failures = fail; const calls = [];
  await page.route('**/dev/snapshot', (r) => failures-- > 0 ? r.fulfill({ status: 503, body: 'backend not running' }) : r.fulfill({ json: snap, headers: { 'access-control-allow-origin': '*' } }));
  await page.route('**/dev/invoke/**', (r) => {
    const cmd = r.request().url().split('/').at(-1); calls.push(cmd);
    const out = invoke(cmd, r.request().postDataJSON());
    return out instanceof Error ? r.fulfill({ status: 500, body: out.message, headers: { 'access-control-allow-origin': '*' } }) : r.fulfill({ json: out, headers: { 'access-control-allow-origin': '*' } });
  });
  await page.goto(url);
  return { page, calls };
}
try {
  // 1. Startup failure is visible and recoverable.
  {
    const { page } = await open(base, { fail: 100 });
    await page.getByRole('heading', { name: 'Unable to open your workspace' }).waitFor();
    await page.screenshot({ path: '.local/ui-audit/startup-error.png' });
    await page.unroute('**/dev/snapshot');
    await page.route('**/dev/snapshot', (r) => r.fulfill({ json: base, headers: { 'access-control-allow-origin': '*' } }));
    await page.getByRole('button', { name: 'Try again' }).click();
    await page.locator('.workspace').waitFor();
    await page.close();
  }
  // 2. Empty design: navigation and welcome remain available.
  {
    const empty = structuredClone(base);
    Object.assign(empty.view, { classes: [], relations: [], packages: [], decisions: [], proposals: [], forces: [] });
    empty.previews = []; empty.metrics.issues = [];
    const { page } = await open(empty);
    await page.getByRole('heading', { name: 'Make the architecture clear.' }).waitFor();
    assert.equal(await page.getByRole('navigation', { name: 'Diagram views' }).getByRole('button').count(), 6);
    await page.getByText('No open decisions').waitFor();
    await page.screenshot({ path: '.local/ui-audit/empty-1440.png' });
    await page.getByRole('button', { name: 'Edit brief' }).click();
    await page.getByRole('heading', { name: 'Brief & rules' }).waitFor();
    await page.close();
  }
  // 3. Populated workspace.
  const { page, calls } = await open(base, { invoke: (cmd, args) => cmd === 'apply_user_ops' && args.ops[0].name === 'fails' ? new Error('disk full') : null });
  await page.locator('.health-trigger').waitFor();
  await page.waitForTimeout(1500);
  await page.screenshot({ path: '.local/ui-audit/workspace-1440.png' });
  // Health: diagonal travel across the gap keeps the popover open; issue actions are buttons.
  const trigger = await page.locator('.health-trigger').boundingBox();
  await page.mouse.move(trigger.x + trigger.width / 2, trigger.y + trigger.height / 2);
  await page.locator('.health-pop').waitFor();
  const pop = await page.locator('.health-pop').boundingBox();
  for (let i = 1; i <= 10; i++) {
    await page.mouse.move(trigger.x + trigger.width / 2 + (pop.x + 120 - trigger.x - trigger.width / 2) * i / 10, trigger.y + trigger.height / 2 + (pop.y + 40 - trigger.y - trigger.height / 2) * i / 10);
    await page.waitForTimeout(25);
  }
  assert.equal(await page.locator('.health-pop').count(), 1, 'health popover closed while travelling into it');
  await page.screenshot({ path: '.local/ui-audit/health.png' });
  await page.mouse.move(5, 600); await page.waitForTimeout(400);
  assert.equal(await page.locator('.health-pop').count(), 0, 'health popover did not close after leaving');
  // Keyboard: click pins, Escape closes and restores focus.
  await page.locator('.health-trigger').click();
  await page.mouse.move(5, 600); await page.waitForTimeout(400);
  assert.equal(await page.locator('.health-pop').count(), 1, 'pinned popover closed on leave');
  await page.keyboard.press('Escape');
  assert.equal(await page.locator('.health-pop').count(), 0);
  assert.ok(await page.locator('.health-trigger').evaluate((e) => e === document.activeElement));
  // Command palette: ⌘K, arguments required, keyboard navigation, Escape restores focus.
  await page.keyboard.press('Meta+k');
  const input = page.getByRole('combobox');
  await input.waitFor();
  await input.fill('find');
  await page.keyboard.press('Enter');
  assert.equal(await input.inputValue(), 'find ', 'required-argument command did not fill its template');
  await page.keyboard.press('Enter');
  await page.getByRole('alert').filter({ hasText: 'needs' }).waitFor();
  await input.fill('zzzz-no-match');
  await page.getByText('No matching action').waitFor();
  await page.screenshot({ path: '.local/ui-audit/palette.png' });
  await page.keyboard.press('Escape');
  assert.equal(await page.locator('.spide-palette').count(), 0);
  // Brief: failed save keeps the draft and explains; success clears.
  await page.getByRole('button', { name: 'Brief', exact: true }).click();
  await page.getByLabel('Goals').fill('fails');
  await page.getByLabel('Goals').press('Enter');
  await page.getByText('Not saved. Your text is kept').waitFor();
  assert.equal(await page.getByLabel('Goals').inputValue(), 'fails');
  await page.getByLabel('Goals').fill('Fast lending');
  await page.getByLabel('Goals').press('Enter');
  await page.waitForFunction(() => document.querySelector('#force-goal').value === '');
  assert.ok(calls.filter((c) => c === 'apply_user_ops').length === 2);
  await page.screenshot({ path: '.local/ui-audit/brief.png' });
  await page.getByRole('button', { name: 'Close brief' }).click();
  // Fit works in non-class views (regression: toolbar used only the class canvas).
  await page.getByRole('navigation', { name: 'Diagram views' }).getByRole('button', { name: 'Package' }).click();
  await page.waitForTimeout(600);
  const before = await page.locator('.stage svg.viewport > g[data-world]').getAttribute('transform');
  await page.getByRole('button', { name: 'Zoom in' }).click();
  await page.waitForTimeout(300);
  assert.notEqual(await page.locator('.stage svg.viewport > g[data-world]').getAttribute('transform'), before, 'zoom did nothing in package view');
  await page.getByRole('button', { name: 'Fit', exact: true }).click();
  // Keyboard inspection of diagram elements.
  await page.getByRole('navigation', { name: 'Diagram views' }).getByRole('button', { name: 'Class', exact: true }).click();
  await page.waitForTimeout(800);
  await page.locator('.stage svg.viewport').focus();
  await page.keyboard.press(']');
  await page.locator('.hovercard').waitFor();
  await page.keyboard.press('Escape');
  assert.ok(!calls.includes('prune'), 'keyboard inspection mutated the design');
  // Geometry at the minimum desktop window.
  for (const [w, h] of [[1280, 800], [1024, 768], [800, 560]]) {
    await page.setViewportSize({ width: w, height: h });
    await page.waitForTimeout(400);
    const overflow = await page.evaluate(() => [...document.querySelectorAll('.workspace-chrome, .canvas-toolbar')].some((e) => e.scrollWidth > e.clientWidth + 1));
    assert.ok(!overflow, `chrome overflows at ${w}x${h}`);
    const tb = await page.locator('.canvas-toolbar').boundingBox(), cv = await page.locator('.canvas-workspace').boundingBox();
    assert.ok(tb.x + tb.width <= cv.x + cv.width + 1, `toolbar escapes canvas at ${w}x${h}`);
    await page.screenshot({ path: `.local/ui-audit/workspace-${w}.png` });
  }
  // Reduced motion: imperative zoom applies immediately, without animation frames.
  await page.emulateMedia({ reducedMotion: 'reduce' });
  const t0 = await page.locator('.stage svg.viewport > g[data-world]').getAttribute('transform');
  await page.getByRole('button', { name: 'Zoom in' }).click();
  await page.waitForTimeout(20);
  assert.notEqual(await page.locator('.stage svg.viewport > g[data-world]').getAttribute('transform'), t0);
  await page.close();
  assert.deepEqual(errors, []);
  console.log('PASS: startup retry, empty welcome, health hover bridge/keyboard, palette, recoverable brief, active-view zoom, keyboard inspection, geometry');
} finally {
  await browser.close();
}
