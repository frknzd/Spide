// Run with the disposable-fixture debug backend and `npm run dev` running.
// All browser mutations are intercepted. No design or shell commands reach the backend.
import assert from 'node:assert/strict';
import { readFile, mkdir } from 'node:fs/promises';
import { chromium } from '@playwright/test';
const source = process.env.SPIDE_SMOKE_SNAPSHOT;
const snap = source ? JSON.parse(await readFile(source, 'utf8')) : await (await fetch('http://127.0.0.1:47800/dev/snapshot')).json();
const makeDecision = (id) => ({ id, question: `Where should ${id} responsibility live?`, context: 'Choose deliberately, then defend your reasoning.', state: 'open', chosen: null, custom: null, attempts: [], options: ['A', 'B'].map((id) => ({ id, label: `Alternative ${id}`, summary: 'A concise alternative summary', pros: ['Keeps behaviour cohesive'], cons: ['Adds indirection'], tradeoffs: [], ops: [] })) });
snap.view.decisions = [makeDecision('one'), makeDecision('two')];
snap.derived.system.containers = [{ name: 'Web', technology: 'Java', replicas: 1, datastore: false, packages: ['ui'], classes: 1, status: 'accepted', description: 'Test container' }];
snap.view.use_cases = { system: 'Library', actors: [{ name: 'Reader', description: 'Borrows books', status: 'accepted', feedback: null }], use_cases: [{ name: 'Borrow', actors: ['Reader'], includes: [], extends: [], classes: ['LoanController'], description: 'Borrow a book', status: 'accepted', feedback: null }], generated_rev: snap.rev };
snap.view.sequences = [{ name: 'Borrow', use_case: 'Borrow', participants: ['LoanController', 'LendingService'], messages: [{ from: 'LoanController', to: 'LendingService', method: 'lend', args: null, returns: null, kind: 'sync', note: null, problem: null, status: 'accepted', feedback: null }], generated_rev: snap.rev, status: 'accepted', feedback: null }];
snap.previews = snap.view.decisions.flatMap((d) => d.options.map((o) => {
  const design = structuredClone(snap.view);
  design.classes.push({ ...structuredClone(design.classes[0]), name: 'PreviewOnly', status: 'accepted' });
  return { decision: d.id, option: o.id, design, health: snap.metrics.health + 1, focus: ['PreviewOnly'], impact: null, error: null };
}));
const browser = await chromium.launch({ channel: process.env.SPIDE_BROWSER_CHANNEL || 'chrome', headless: true });
const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
const errors = [], calls = [];
page.on('pageerror', (e) => errors.push(String(e)));
await page.route('**/dev/snapshot', (route) => route.fulfill({ json: snap, headers: { 'access-control-allow-origin': '*' } }));
await page.route('**/dev/invoke/**', (route) => {
  calls.push(route.request().url().split('/').at(-1));
  return route.fulfill({ json: null, headers: { 'access-control-allow-origin': '*' } });
});
const box = (selector) => page.locator(selector).boundingBox();
try {
  await page.goto(process.env.SPIDE_SMOKE_URL || 'http://127.0.0.1:1420/?perf=1');
  await page.locator('.decision-panel').waitFor();
  await page.waitForTimeout(2100);
  let [terminal, canvas, inspector] = await Promise.all(['.workspace-left', '.canvas-workspace', '.workspace-right'].map(box));
  assert.ok(terminal.x + terminal.width <= canvas.x && canvas.x + canvas.width <= inspector.x);
  assert.ok(canvas.width >= 220);
  assert.equal(await page.locator('.option svg.mini').count(), 0);
  await page.getByLabel('Choose Alternative A').check();
  await page.getByLabel('Your defence').fill('This keeps the changing behaviour in one place.');
  await page.getByRole('button', { name: 'Details', exact: true }).first().click();
  await page.locator('.option svg.mini').waitFor();
  assert.equal(await page.locator('.stage [data-hover="class:PreviewOnly"]').count(), 0);
  const beforePreview = calls.length;
  await page.getByRole('button', { name: 'Preview', exact: true }).first().click();
  await page.locator('.stage [data-hover="class:PreviewOnly"]').waitFor();
  await page.waitForTimeout(1700);
  assert.ok(!calls.slice(beforePreview).includes('set_layout'), 'preview saved layout');
  const node = page.locator('.stage [data-hover="class:PreviewOnly"]');
  await node.dispatchEvent('pointerdown', { button: 2, pointerId: 1, clientX: 500, clientY: 300 });
  await node.dispatchEvent('pointerup', { button: 2, pointerId: 1, clientX: 500, clientY: 300 });
  assert.ok(!calls.slice(beforePreview).includes('prune'));
  await page.getByRole('button', { name: 'Stop preview' }).click();
  await page.getByRole('button', { name: 'Next decision' }).click();
  await page.getByLabel('Your defence').fill('A separate draft for the second decision.');
  await page.getByRole('button', { name: 'Previous decision' }).click();
  assert.equal(await page.getByLabel('Your defence').inputValue(), 'This keeps the changing behaviour in one place.');
  assert.ok(await page.getByLabel('Choose Alternative A').isChecked());
  // Underlying revision closes preview without resetting the draft.
  await page.getByRole('button', { name: 'Preview', exact: true }).first().click();
  snap.rev++;
  await page.waitForTimeout(1800);
  assert.equal(await page.getByRole('button', { name: 'Stop preview' }).count(), 0);
  assert.equal(await page.getByLabel('Your defence').inputValue(), 'This keeps the changing behaviour in one place.');
  // Continuous movement must not reset the hover's initial intent timer.
  await page.waitForTimeout(1700);
  const target = page.locator('.stage [data-hover^="class:"]').first();
  const b = await target.boundingBox();
  await page.evaluate(() => {
    window.__hoverStart = performance.now();
    window.__hoverMs = null;
    const observer = new MutationObserver(() => {
      if (document.querySelector('.hovercard')) { window.__hoverMs = performance.now() - window.__hoverStart; observer.disconnect(); }
    });
    observer.observe(document.body, { childList: true, subtree: true });
  });
  for (let i = 0; i < 12; i++) { await page.mouse.move(b.x + 8 + (i % 3), b.y + 8); await page.waitForTimeout(8); }
  await page.locator('.hovercard').waitFor({ timeout: 1000 });
  console.log('hover first visible ms:', Math.round(await page.evaluate(() => window.__hoverMs)));
  await page.evaluate(async () => (await import('/src/state/perf.ts')).perfTake());
  for (let i = 0; i < 12; i++) { await page.mouse.move(b.x + 8 + (i % 3), b.y + 8); }
  const renders = await page.evaluate(async () => (await import('/src/state/perf.ts')).perfTake());
  assert.ok(!renders.App && !renders.Canvas && !renders.DiagramContent, `same-target render storm: ${JSON.stringify(renders)}`);
  console.log('same-target motion renders:', renders);
  const hover = await box('.hovercard');
  canvas = await box('.canvas-workspace');
  assert.ok(hover.x >= canvas.x && hover.x + hover.width <= canvas.x + canvas.width + 1);
  await page.mouse.wheel(0, 80);
  await page.waitForTimeout(50);
  assert.equal(await page.locator('.hovercard').count(), 0);
  await mkdir('.local', { recursive: true });
  await page.screenshot({ path: '.local/workspace-1440.png' });
  for (const [tab, hoverKey] of [['Domain', 'class:'], ['Package', 'package:'], ['System', 'c4:Web'], ['Use case', 'usecase:Borrow'], ['Sequence', 'message:']]) {
    await page.locator('.tabs').getByRole('button', { name: tab, exact: true }).click();
    await page.waitForTimeout(300);
    const candidate = page.locator(`.stage [data-hover^="${hoverKey}"]`).first();
    await candidate.waitFor();
    const rect = await candidate.boundingBox();
    await candidate.dispatchEvent('pointermove', { clientX: rect.x + rect.width / 2, clientY: rect.y + rect.height / 2 });
    await page.locator('.hovercard').waitFor();
    assert.ok((await page.locator('.hovercard').innerText()).length > 0, `${tab} card missing`);
  }
  await page.locator('.tabs').getByRole('button', { name: 'Class', exact: true }).click();
  assert.equal(await page.locator('.hovercard').count(), 0, 'card survived tab change');
  await page.setViewportSize({ width: 800, height: 560 });
  await page.waitForTimeout(200);
  assert.equal(await page.locator('.decision-panel').isVisible(), false);
  await page.getByRole('button', { name: 'Open decision inspector' }).click();
  const footer = await box('.decision-footer');
  assert.ok(footer.y >= 30 && footer.y + footer.height <= 561);
  terminal = await box('.workspace-left'); inspector = await box('.workspace-right');
  assert.ok(inspector.x >= terminal.x + terminal.width, 'drawer covers pi');
  assert.equal(await page.getByLabel('Your defence').inputValue(), 'This keeps the changing behaviour in one place.');
  await page.screenshot({ path: '.local/workspace-800.png' });
  await page.getByRole('button', { name: 'Collapse decision inspector' }).click();
  await page.getByRole('button', { name: 'Collapse pi pane' }).click();
  await page.getByRole('button', { name: 'Open pi pane' }).click();
  assert.equal(await page.locator('.terminal-host').count(), 1);
  assert.ok(!calls.includes('restart_agent'));
  assert.deepEqual(errors, []);
  console.log('PASS: panes, lazy details, explicit read-only preview, revision reset, drafts, hover, narrow drawer, terminal retention');
} catch (error) {
  console.error('browser errors:', errors);
  await page.screenshot({ path: '.local/workspace-failure.png' }).catch(() => {});
  throw error;
} finally { await browser.close(); }
