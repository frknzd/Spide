// xterm/IPC integration under React StrictMode, using a deterministic fake native PTY.
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { chromium } from '@playwright/test';
const snapshot = process.env.SPIDE_SMOKE_SNAPSHOT ? JSON.parse(await readFile(process.env.SPIDE_SMOKE_SNAPSHOT, 'utf8')) : await (await fetch('http://127.0.0.1:47800/dev/snapshot')).json();
const browser = await chromium.launch({ channel: process.env.SPIDE_BROWSER_CHANNEL || 'chrome', headless: true });
const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
const errors = [];
page.on('pageerror', (e) => errors.push(String(e)));
await page.addInitScript((snapshot) => {
  const encoder = new TextEncoder();
  const state = window.__ptyTest = { generation: 1, bytes: Array.from(encoder.encode('\x1b[>4;2m\x1b[?2004h\x1b[2J\x1b[Hpi fixture €\r\nnative prompt> ')), input: [], sizes: [], reads: 0, concurrent: 0, maxConcurrent: 0, restarts: 0 };
  let callback = 0;
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
  window.__TAURI_INTERNALS__ = {
    transformCallback: () => ++callback,
    unregisterCallback() {},
    invoke: async (cmd, args) => {
      if (cmd === 'get_snapshot') return snapshot;
      if (cmd === 'plugin:event|listen') return ++callback;
      if (cmd === 'plugin:event|unlisten') return;
      if (cmd === 'terminal_context') return;
      if (cmd === 'terminal_resize') { state.sizes.push(args); return; }
      if (cmd === 'terminal_write') { state.input.push(...args.data); return; }
      if (cmd === 'restart_agent') { state.restarts++; state.generation++; state.bytes = Array.from(encoder.encode('restarted\r\n')); return; }
      if (cmd === 'terminal_read') {
        state.reads++; state.concurrent++; state.maxConcurrent = Math.max(state.maxConcurrent, state.concurrent);
        try {
          await new Promise((r) => setTimeout(r, 15));
          if (args.generation !== null && args.generation !== state.generation) throw new Error('stale terminal generation');
          const bytes = state.bytes.slice(args.after, args.after + 3); // deliberately split ANSI and UTF-8
          return { generation: state.generation, start: args.after, next: args.after + bytes.length, bytes, reset: false, exited: false };
        } finally { state.concurrent--; }
      }
      return null; // mock design layout writes; no backend mutations
    },
  };
}, { ...snapshot, pi_status: 'idle', pi_error: null });
const inspect = (f) => page.evaluate(f);
try {
  await page.goto(process.env.SPIDE_SMOKE_URL || 'http://127.0.0.1:1420/?perf=1');
  await page.locator('.terminal-host .xterm').waitFor();
  await page.waitForTimeout(2400);
  const line = await inspect(async () => {
    // Vite may timestamp this dependency after HMR. Import the exact resource URL the
    // app loaded, not a second module instance with an independent singleton/poll loop.
    const url = performance.getEntriesByType('resource').find((e) => new URL(e.name).pathname === '/src/state/terminal.ts').name;
    return (await import(url)).terminalSession().terminal.buffer.active.getLine(0).translateToString(true);
  });
  assert.equal(line, 'pi fixture €', 'split UTF-8/ANSI not rendered intact');
  assert.equal(await inspect(() => window.__ptyTest.maxConcurrent), 1, 'StrictMode duplicated polling');
  assert.ok((await inspect(() => window.__ptyTest.sizes)).length > 0);
  await page.locator('.xterm-helper-textarea').focus();
  for (const key of ['Control+p', 'Control+l', 'Shift+Tab', 'Shift+Enter', 'Control+Shift+p']) await page.keyboard.press(key);
  await page.waitForTimeout(100);
  const input = await inspect(() => new TextDecoder().decode(Uint8Array.from(window.__ptyTest.input)));
  assert.ok(input.includes('\x10') && input.includes('\x0c') && input.includes('\x1b[Z'));
  assert.ok(input.includes('\x1b[27;2;13~') && input.includes('\x1b[27;6;112~'), JSON.stringify(input));
  await page.getByRole('button', { name: 'Collapse pi pane' }).click();
  const reads = await inspect(() => window.__ptyTest.reads);
  await page.waitForTimeout(100);
  assert.ok(await inspect(() => window.__ptyTest.reads) > reads, 'collapse stopped the session');
  await page.getByRole('button', { name: 'Open pi pane' }).click();
  assert.equal(await page.locator('.terminal-host').count(), 1);
  assert.equal(await inspect(() => window.__ptyTest.restarts), 0);
  // Streaming stays outside React and design rendering, including split frame decoding.
  await inspect(async () => (await import('/src/state/perf.ts')).perfTake());
  await inspect(() => window.__ptyTest.bytes.push(...new TextEncoder().encode('\r\nstreaming text €')));
  await page.waitForTimeout(250);
  const renders = await inspect(async () => (await import('/src/state/perf.ts')).perfTake());
  assert.ok(!renders.App && !renders.Canvas && !renders.DiagramContent, JSON.stringify(renders));
  await page.getByRole('button', { name: 'Restart', exact: true }).click();
  await page.waitForTimeout(200);
  assert.equal(await inspect(() => window.__ptyTest.restarts), 0, 'restart without confirmation');
  await page.getByRole('alertdialog', { name: 'Restart pi' }).getByRole('button', { name: 'Restart' }).click();
  await page.waitForTimeout(900);
  assert.equal(await inspect(() => window.__ptyTest.restarts), 1);
  assert.equal(await inspect(() => window.__ptyTest.maxConcurrent), 1);
  assert.equal(await inspect(async () => {
    const url = performance.getEntriesByType('resource').find((e) => new URL(e.name).pathname === '/src/state/terminal.ts').name;
    return (await import(url)).terminalSession().terminal.buffer.active.getLine(0).translateToString(true);
  }), 'restarted');
  assert.deepEqual(errors, []);
  console.log('PASS: StrictMode single poll, binary decoding, negotiated native keys, hidden retention, streaming isolation, generation restart');
} finally { await browser.close(); }
