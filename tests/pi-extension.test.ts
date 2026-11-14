import { test } from "node:test";
import assert from "node:assert/strict";
import { createServer } from "node:net";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import bridge from "../agent/spide-terminal.ts";

test("one native session: authenticated handshake, context, follow-up, duplicates, shutdown", async () => {
  const dir = await mkdtemp(join(tmpdir(), "spide-test-"));
  const path = join(dir, "pi.sock");
  process.env.SPIDE_CONTROL_SOCKET = path;
  process.env.SPIDE_CONTROL_TOKEN = "test-only";
  process.env.SPIDE_CONTROL_GENERATION = "7";
  const hooks = new Map<string, Function>();
  const messages: unknown[] = [];
  let idle = false;
  let failContext = false;
  const restored: string[] = [], notices: string[] = [];
  let peer: any;
  const received: any[] = [];
  const server = createServer((s) => {
    peer = s;
    let text = "";
    s.setEncoding("utf8");
    s.on("data", (chunk) => {
      text += chunk;
      let n;
      while ((n = text.indexOf("\n")) >= 0) {
        const v = JSON.parse(text.slice(0, n)); text = text.slice(n + 1);
        received.push(v);
        if (v.type === "hello") {
          assert.equal(v.token, "test-only");
          s.write(JSON.stringify({ type: "ready", generation: 7 }) + "\n");
        }
        if (v.method === "context") s.write(JSON.stringify({ type: "response", id: v.id, generation: 7, ok: !failContext, error: "context unavailable", result: { text: "decorated:" + v.payload.text } }) + "\n");
      }
    });
  });
  await new Promise<void>((r) => server.listen(path, r));
  const ctx = { isIdle: () => idle, ui: { notify: (s: string) => notices.push(s), setEditorText: (s: string) => restored.push(s) }, abort() {} };
  bridge({ on: (name: string, f: Function) => hooks.set(name, f), registerCommand() {}, getActiveTools: () => ["read", "bash", "edit", "write"], sendUserMessage: (...args: unknown[]) => messages.push(args) } as any);
  const wait = async (f: () => boolean) => { for (let i = 0; i < 100 && !f(); i++) await new Promise((r) => setTimeout(r, 10)); assert.ok(f()); };
  try {
    await hooks.get("session_start")!({}, ctx);
    await wait(() => received.some((v) => v.type === "hello"));
    await new Promise((r) => setTimeout(r, 30));
    const input = await hooks.get("input")!({ source: "interactive", text: "hello", images: ["image"] }, ctx);
    assert.deepEqual(input, { action: "transform", text: "decorated:hello", images: ["image"] });
    assert.deepEqual(await hooks.get("input")!({ source: "extension", text: "already" }, ctx), { action: "continue" });
    const request = JSON.stringify({ type: "request", id: "one", generation: 7, method: "prompt", payload: { message: "examine" } }) + "\n";
    peer.write(request + request);
    await wait(() => received.filter((v) => v.id === "one").length === 2);
    assert.deepEqual(messages, [["examine", { deliverAs: "followUp" }]]);
    peer.write(JSON.stringify({ type: "request", id: "stale", generation: 6, method: "prompt", payload: { message: "wrong project" } }) + "\n");
    await new Promise((r) => setTimeout(r, 30));
    assert.equal(messages.length, 1);
    await hooks.get("agent_settled")!({}, ctx);
    await wait(() => received.some((v) => v.event === "settled"));
    // /new, /resume and /reload use shutdown/start (there is no session_switch hook).
    // The extension reconnects to the same generation without starting another pi.
    idle = true;
    await hooks.get("session_shutdown")!();
    await new Promise((r) => setTimeout(r, 30));
    await hooks.get("session_start")!({}, ctx);
    await wait(() => received.filter((v) => v.type === "hello").length === 2);
    await new Promise((r) => setTimeout(r, 30));
    peer.write(JSON.stringify({ type: "request", id: "after-reload", generation: 7, method: "prompt", payload: { message: "resume" } }) + "\n");
    await wait(() => messages.length === 2);
    assert.deepEqual(messages[1], ["resume", undefined]);
    failContext = true;
    assert.deepEqual(await hooks.get("input")!({ source: "interactive", text: "keep my draft" }, ctx), { action: "handled" });
    assert.deepEqual(restored, ["keep my draft"]);
    assert.ok(notices[0].includes("context unavailable"));
    assert.equal(messages.length, 2);
  } finally {
    await hooks.get("session_shutdown")!();
    peer?.destroy();
    await new Promise<void>((r) => server.close(() => r()));
    await rm(dir, { recursive: true, force: true });
    delete process.env.SPIDE_CONTROL_TOKEN;
    delete process.env.SPIDE_CONTROL_SOCKET;
    delete process.env.SPIDE_CONTROL_GENERATION;
  }
});
