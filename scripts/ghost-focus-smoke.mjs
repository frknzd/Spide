import assert from 'node:assert/strict';
import { readFile, mkdir } from 'node:fs/promises';
import { chromium } from '@playwright/test';
// UI contract smoke: HTTP is mocked; real operation/transaction semantics are tested in Rust.
const source = process.env.SPIDE_SMOKE_SNAPSHOT;
const snap = source ? JSON.parse(await readFile(source, 'utf8')) : await (await fetch('http://127.0.0.1:47800/dev/snapshot')).json();
const clone = structuredClone;
let base = clone(snap.view);
base.proposals = [];
base.settings.brief_skipped = true;
base.decisions = [{ id: 'D-review', question: 'How should loan changes be audited?', context: 'Keep a reliable audit trail.', options: [{ id: 'A', label: 'Audit trail', summary: 'Record changes in a dedicated class.', ops: [], pros: [], cons: [], tradeoffs: [] }], state: 'accepted', chosen: 'A', custom: null, attempts: [{ option: 'A', custom: null, defense: 'Separate audit responsibility.', verdict: 'accept', feedback: 'Reasonable separation.' }], applied_ops: [] }];
const infra = base.packages.find((p) => p.name.includes('infra'))?.name ?? base.packages[0].name;
let plans = [
  { id: 'p1', source: 'agent', rationale: 'Record every loan change for auditing.', decision: 'D-review', option: 'A', ops: [
    { op: 'add_class', name: 'AuditTrail', package: infra },
    { op: 'add_relation', from: 'LendingService', to: 'AuditTrail', kind: 'dependency', label: 'audit' },
    { op: 'update_class', name: 'LendingService', rationale: 'Coordinate lending only.' },
  ] },
  { id: 'p2', source: 'agent', rationale: 'Keep audit history.', ops: [{ op: 'add_force', name: 'Traceability', kind: 'quality' }] },
];
let failSave = false;
const undo = [], redo = [];
const remember = () => { undo.push(clone({ base, plans })); redo.length = 0; };
function apply(design, ops, id, commit = false) {
  const d = clone(design), changes = [];
  const tag = { status: commit ? 'accepted' : 'proposed', proposal: commit ? null : id };
  for (const op of ops) {
    const before = clone(d);
    if (op.op === 'add_class') {
      if (!op.name || d.classes.some((c) => c.name === op.name)) throw Error('Invalid or duplicate class');
      d.classes.push({ ...clone(base.classes[0]), name: op.name, package: op.package, attributes: [], methods: [], responsibilities: [], ...tag });
    } else if (op.op === 'add_relation') {
      if (!d.classes.some((c) => c.name === op.from) || !d.classes.some((c) => c.name === op.to)) throw Error('Missing relation endpoint');
      d.relations.push({ ...clone(base.relations[0]), ...op, id: `${op.from}->${op.to}:${op.kind}`, ...tag });
    } else if (op.op === 'update_class') {
      const c = d.classes.find((x) => x.name === op.name); if (!c) throw Error('Missing class'); Object.assign(c, Object.fromEntries(Object.entries(op).filter(([k]) => !['op', 'name'].includes(k))));
    } else if (op.op === 'add_method') {
      const c = d.classes.find((x) => x.name === op.class); if (!c || !op.method.name) throw Error('Missing class or method name'); c.methods.push({ ...op.method, ...tag });
    } else if (op.op === 'remove_class') {
      const c = d.classes.find((x) => x.name === op.name); if (!c) throw Error('Missing class');
      if (commit) d.classes = d.classes.filter((x) => x !== c); else Object.assign(c, { status: 'removing', proposal: id });
    } else if (op.op === 'add_force') d.forces.push({ ...op, description: null, priority: 3, ...tag });
    else throw Error(`Unsupported smoke operation: ${op.op}`);
    const diff = [];
    for (const collection of ['classes', 'relations', 'forces']) {
      const identity = (v) => v.name ?? v.id;
      for (const key of new Set([...before[collection], ...d[collection]].map(identity))) {
        const a = before[collection].find((v) => identity(v) === key) ?? null, b = d[collection].find((v) => identity(v) === key) ?? null;
        if (JSON.stringify(a) !== JSON.stringify(b)) diff.push({ path: `${collection}.${key}`, before: a, after: b });
      }
    }
    changes.push({ index: changes.length, changes: diff, error: null });
  }
  return { design: d, changes };
}
function snapshot() {
  let view = clone(base);
  for (const p of plans) { try { view = apply(view, p.ops, p.id).design; } catch {} }
  view.proposals = clone(plans);
  return { ...snap, view, can_undo: !!undo.length, can_redo: !!redo.length };
}
const browser = await chromium.launch({ channel: process.env.SPIDE_BROWSER_CHANNEL || 'chrome' });
const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
const calls = [], errors = [];
page.on('pageerror', (e) => errors.push(String(e)));
page.on('dialog', (d) => d.accept());
await page.route('**/dev/snapshot', (r) => r.fulfill({ json: snapshot(), headers: { 'access-control-allow-origin': '*' } }));
await page.route('**/dev/invoke/**', async (route) => {
  const cmd = route.request().url().split('/').at(-1), args = route.request().postDataJSON(); calls.push({ cmd, args });
  try {
    let result = null;
    if (cmd === 'review_proposal') {
      const at = plans.findIndex((p) => p.id === args.id); if (at < 0) throw Error('Unknown plan');
      let baseline = clone(base); for (const p of plans.slice(0, at)) baseline = apply(baseline, p.ops, p.id).design;
      try {
        const r = apply(baseline, args.ops ?? plans[at].ops, args.id);
        result = { rev: snap.rev, baseline, design: r.design, steps: r.changes, errors: [], prerequisites: [], notices: [], can_accept: true, impact: null };
      } catch (e) { result = { rev: snap.rev, baseline, design: baseline, steps: [], errors: [String(e)], prerequisites: [], notices: [], can_accept: false, impact: null }; }
    } else if (cmd === 'update_proposal') {
      if (failSave) throw Error('Simulated save failure');
      assert.equal(args.expectedRev, snap.rev); remember(); const p = plans.find((p) => p.id === args.id); Object.assign(p, { ops: args.ops, rationale: args.rationale, decision: args.decision, option: args.option }); snap.rev++;
    } else if (cmd === 'accept_proposal' || cmd === 'reject_proposal') {
      assert.equal(args.expectedRev, snap.rev); remember(); const p = plans.find((p) => p.id === args.id);
      if (cmd === 'accept_proposal') base = apply(base, p.ops, p.id, true).design;
      plans = plans.filter((p) => p.id !== args.id); snap.rev++;
    } else if (cmd === 'undo' || cmd === 'redo') {
      const from = cmd === 'undo' ? undo : redo, to = cmd === 'undo' ? redo : undo;
      if (from.length) { to.push(clone({ base, plans })); ({ base, plans } = from.pop()); snap.rev++; }
    } else assert.ok(!['apply_user_ops', 'prune', 'accept_all'].includes(cmd), `Unsafe review mutation ${cmd}`);
    if (cmd === 'review_proposal' && args.ops?.some((op) => op.method?.name === 'obsolete')) await new Promise((r) => setTimeout(r, 900));
    await route.fulfill({ json: result, headers: { 'access-control-allow-origin': '*' } });
  } catch (e) { await route.fulfill({ status: 400, body: String(e), headers: { 'access-control-allow-origin': '*' } }); }
});
const ui = page.getByRole('region', { name: 'Ghost proposal review' });
const waitPreview = () => page.getByRole('status').filter({ hasText: 'Validating draft' }).waitFor({ state: 'hidden' });
try {
  await page.goto((process.env.SPIDE_SMOKE_URL || 'http://127.0.0.1:1420/').replace(/\?.*$/, ''));
  await page.locator('.stage [data-hover="class:AuditTrail"]').waitFor(); await page.waitForTimeout(1200);
  const opacity = () => page.locator('[data-node="Catalog"]').evaluate((e) => Number(e.closest('g[opacity]').getAttribute('opacity')));
  const calm = await opacity();
  await page.locator('.stage [data-hover="class:AuditTrail"]').hover({ position: { x: 30, y: 12 } });
  await page.locator('.hovercard').waitFor(); assert.ok(await opacity() < calm, 'ghost hover did not spotlight');
  await page.locator('.stage [data-hover="class:AuditTrail"]').click({ position: { x: 30, y: 12 } });
  await ui.waitFor(); await page.waitForTimeout(500);
  assert.ok(!calls.some((c) => /^(accept|reject)/.test(c.cmd)), 'click accepted a ghost');
  assert.match(await ui.innerText(), /How should loan changes be audited/);
  assert.match(await ui.innerText(), /Coordinate lending only/);
  await ui.getByLabel('Show before this plan on canvas').check(); await page.waitForTimeout(250);
  assert.equal(await page.locator('.stage [data-node="AuditTrail"]').count(), 0);
  await ui.getByLabel('Show before this plan on canvas').uncheck(); await page.waitForTimeout(350);
  assert.ok(await opacity() < calm, 'focused review lost its spotlight');
  assert.equal(await page.getByRole('button', { name: /Accept all|Reject all/ }).count(), 0);
  // Add a method using structured fields.
  await ui.getByRole('button', { name: 'Add planned change', exact: true }).click();
  const editor = ui.getByRole('region', { name: 'Edit planned operation' });
  await editor.getByLabel('Change type').selectOption('add_method');
  await editor.getByLabel('class', { exact: true }).fill('AuditTrail');
  await editor.getByLabel('method · name', { exact: true }).fill('record');
  await editor.getByRole('button', { name: 'Use change in draft' }).click();
  await page.waitForTimeout(500);
  assert.equal(await ui.getByRole('button', { name: 'Accept proposal', exact: true }).isDisabled(), true);
  // Switching plans retains a draft without committing it.
  await ui.getByLabel('Plan', { exact: true }).selectOption('p2'); await page.waitForTimeout(350);
  await ui.getByLabel('Plan', { exact: true }).selectOption('p1'); await page.waitForTimeout(400);
  assert.match(await ui.innerText(), /record/);
  // A late response for an earlier draft must not overwrite the current preview.
  const methodOperation = ui.locator('.plan-operation').filter({ has: page.getByRole('heading', { name: /add method/ }) });
  await methodOperation.getByRole('button', { name: 'Edit change' }).click();
  await editor.getByLabel('method · name', { exact: true }).fill('obsolete'); await editor.getByRole('button', { name: 'Use change in draft' }).click();
  await page.waitForTimeout(280);
  await methodOperation.getByRole('button', { name: 'Edit change' }).click();
  await editor.getByLabel('method · name', { exact: true }).fill('record'); await editor.getByRole('button', { name: 'Use change in draft' }).click();
  await page.waitForTimeout(1200); assert.ok(!/obsolete/.test(await ui.innerText()), 'late draft response replaced the current preview');
  // Edit a relation, preserving all unexposed fields.
  await ui.locator('.plan-operation').filter({ has: page.getByRole('heading', { name: /add relation/ }) }).getByRole('button', { name: 'Edit change' }).click();
  await editor.getByLabel('label', { exact: true }).fill('records changes');
  await editor.getByRole('button', { name: 'Use change in draft' }).click();
  // Removing an operation is not deleting an accepted element.
  await ui.locator('.plan-operation').filter({ has: page.getByRole('heading', { name: /update class/ }) }).getByRole('button', { name: 'Remove planned change' }).click();
  await ui.getByRole('button', { name: 'Add planned change', exact: true }).click();
  await editor.getByLabel('Change type').selectOption('remove_class'); await editor.getByLabel('name', { exact: true }).fill('NotificationLog');
  await editor.getByRole('button', { name: 'Use change in draft' }).click();
  await page.waitForTimeout(500); await waitPreview();
  assert.ok(base.classes.some((c) => c.name === 'NotificationLog'), 'draft deletion committed');
  // Conflicts preserve drafts and require an explicit revalidation.
  snap.rev++; await page.waitForTimeout(1800);
  await ui.getByRole('button', { name: 'Revalidate draft against latest workspace' }).click();
  failSave = true;
  await ui.getByRole('button', { name: 'Save plan', exact: true }).click();
  await ui.getByRole('alert').filter({ hasText: 'Simulated save failure' }).waitFor();
  assert.match(await ui.innerText(), /record/); failSave = false;
  await ui.getByRole('button', { name: 'Save plan', exact: true }).click(); await page.waitForTimeout(1900);
  await ui.getByRole('button', { name: 'Accept proposal', exact: true }).click(); await page.waitForTimeout(1900);
  assert.equal(plans.length, 1); assert.equal(plans[0].id, 'p2');
  assert.ok(base.classes.find((c) => c.name === 'AuditTrail').methods.some((m) => m.name === 'record'));
  assert.match(await ui.innerText(), /No associated decision/);
  assert.match(await ui.innerText(), /may not have an element on the class diagram/);
  // Less-common families stay editable via validated advanced JSON.
  await ui.getByRole('button', { name: 'Edit change', exact: true }).click();
  await editor.getByLabel('Operation JSON').fill('{'); await editor.getByRole('button', { name: 'Use change in draft' }).click();
  await editor.getByRole('alert').waitFor();
  await editor.getByLabel('Operation JSON').fill(JSON.stringify({ op: 'add_force', name: 'Traceability', kind: 'quality', priority: 5 }));
  await editor.getByRole('button', { name: 'Use change in draft' }).click();
  await page.waitForTimeout(350);
  assert.equal(await ui.getByRole('button', { name: 'Accept proposal', exact: true }).isDisabled(), true);
  await ui.getByRole('button', { name: 'Reject proposal', exact: true }).click(); await page.waitForTimeout(1800);
  await page.getByRole('heading', { name: 'No ghost proposals' }).waitFor();
  await page.locator('.stage').click({ position: { x: 20, y: 20 } }); await page.keyboard.press('Meta+z'); await page.waitForTimeout(1800);
  await page.getByRole('button', { name: 'Review proposals (1)', exact: true }).click(); await page.waitForTimeout(500);
  assert.equal(plans.length, 1);
  // Redo resolution, then undo it again without touching an accepted element.
  await page.locator('.stage').click({ position: { x: 20, y: 20 } }); await page.keyboard.press('Meta+Shift+z'); await page.waitForTimeout(1700);
  assert.equal(plans.length, 0);
  await page.keyboard.press('Meta+z'); await page.waitForTimeout(1700);
  await page.getByRole('button', { name: 'Review proposals (1)', exact: true }).click();
  // Narrow-screen drawer and keyboard-accessible controls.
  await page.setViewportSize({ width: 800, height: 900 });
  await page.getByRole('button', { name: 'Collapse design inspector' }).click();
  await page.getByRole('button', { name: 'Review proposals (1)', exact: true }).click();
  assert.equal(await page.locator('.workspace-right').evaluate((e) => e.classList.contains('open')), true);
  await mkdir('.local/ui-audit', { recursive: true }); await page.screenshot({ path: '.local/ui-audit/ghost-review.png' });
  assert.ok(!calls.some((c) => ['accept_all', 'apply_user_ops', 'prune'].includes(c.cmd)));
  assert.deepEqual(errors, []);
  console.log('PASS: focused ghost review, structured plan edits, isolated preview, conflict/save errors, individual resolution, undo and responsive drawer');
} finally { await browser.close(); }
