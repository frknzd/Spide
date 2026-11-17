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
  bridge({ on: (name: string, f: Function) => hooks.set(name, f), registerCommand() {}, registerTool() {}, getActiveTools: () => ["read", "bash", "edit", "write"], sendUserMessage: (...args: unknown[]) => messages.push(args) } as any);
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

test("/spide completions offer the Spide Project commands and not the removed code import", () => {
  process.env.SPIDE_CONTROL_SOCKET = join(tmpdir(), "spide-unused.sock");
  process.env.SPIDE_CONTROL_TOKEN = "test-only";
  process.env.SPIDE_CONTROL_GENERATION = "1";
  const registered = new Map<string, any>();
  bridge({ on() {}, registerCommand: (name: string, spec: any) => registered.set(name, spec), registerTool() {}, getActiveTools: () => [], sendUserMessage() {} } as any);
  const spide = registered.get("spide");
  assert.ok(spide);
  const all = spide.getArgumentCompletions("").map((c: any) => c.value);
  for (const c of ["project", "readjust", "implement", "verify", "synced", "reindex"]) assert.ok(all.includes(c), c);
  for (const c of ["import", "drift", "code"]) assert.ok(!all.includes(c), c);
  assert.deepEqual(spide.getArgumentCompletions("re").map((c: any) => c.value), ["reject", "redo", "readjust", "reindex"]);
});

test("spide_lucid pushes in place through Spide's plan and Lucid's scripts, and reports refusals", async () => {
  process.env.SPIDE_CONTROL_SOCKET = join(tmpdir(), "spide-unused.sock");
  process.env.SPIDE_CONTROL_TOKEN = "test-only";
  process.env.SPIDE_CONTROL_GENERATION = "1";
  const tools = new Map<string, any>();
  const servers: string[] = [];
  const commands = new Map<string, any>();
  const sent: unknown[] = [];
  bridge({ on() {}, registerCommand: (n: string, s: any) => commands.set(n, s), registerTool: (t: any) => tools.set(t.name, t), registerMcpServer: (n: string) => servers.push(n), getActiveTools: () => [], sendUserMessage: (...a: unknown[]) => sent.push(a) } as any);
  const tool = tools.get("spide_lucid");
  assert.ok(tool);
  assert.deepEqual(tool.reachesTools({}), ["mcp__lucid", "mcp__spide"]);
  const calls: string[] = [];
  let remoteAhead = false;
  const json = (v: unknown) => ({ content: [{ type: "text", text: typeof v === "string" ? v : JSON.stringify(v) }] });
  const executeTool = async (name: string, args: any) => {
    calls.push(name);
    switch (name) {
      case "mcp__spide__lucid_status": return json({ enabled: true, sync_allowed: true, folder_id: "42", diagrams: [
        { key: "class", title: "Shop · Class diagram", status: "local_ahead", document_id: "doc-1", url: "https://lucid.app/lucidchart/doc-1/edit", in_design: true },
        { key: "domain", title: "Shop · Domain model", status: "unlinked", document_id: null, in_design: true }] });
      case "mcp__spide__lucid_read_script": return json("return {items: []}");
      case "mcp__lucid__lucid_load_script_documentation": return json({ documentation: "" });
      case "mcp__lucid__lucid_run_script": return json({ result: args.source.startsWith("return") ? { items: [], doc: args.document_id } : { applied: 1 } });
      case "mcp__spide__lucid_plan_push":
        if (remoteAhead) return { ...json(JSON.stringify({ refused: "remote_ahead", message: "Pull first." })), isError: true };
        return json(args.key === "class" ? { create: false, chunks: ["edit-1", "edit-2"], title: "Shop · Class diagram" } : { create: true, chunks: ["create-1"], title: "Shop · Domain model" });
      case "mcp__lucid__lucid_create_blank_document": return json({ documentId: "doc-2", editUrl: "https://lucid.app/lucidchart/doc-2/edit" });
      case "mcp__lucid__lucid_update_document_metadata": assert.equal(args.parent, 42); return json({});
      case "mcp__spide__lucid_record_push": assert.ok(args.snapshot.items); return json({ status: "in_sync" });
      default: return { ...json(`unknown tool ${name}`), isError: true };
    }
  };
  const r = await tool.execute("t1", { action: "push" }, new AbortController().signal, undefined, { executeTool });
  assert.ok(!r.isError, r.content[0].text);
  assert.ok(servers.includes("lucid"));
  // class: read → plan → 2 edits → read → record; domain: plan → create in the folder → 1 edit → read → record
  const runs = calls.filter((c) => c === "mcp__lucid__lucid_run_script").length;
  assert.equal(runs, 2 + 2 + 1 + 1, calls.join(" "));
  assert.equal(calls.filter((c) => c === "mcp__spide__lucid_record_push").length, 2);
  assert.ok(calls.indexOf("mcp__lucid__lucid_create_blank_document") < calls.lastIndexOf("mcp__spide__lucid_record_push"));
  assert.match(r.content[0].text, /domain: in sync — https:\/\/lucid.app\/lucidchart\/doc-2\/edit/);
  remoteAhead = true;
  const refused = await tool.execute("t2", { action: "push", key: "class" }, new AbortController().signal, undefined, { executeTool });
  assert.ok(refused.isError);
  assert.match(refused.content.at(-1).text, /Pull first/);
  // /spide lucid … asks the agent to run the tool
  await commands.get("spide").handler("lucid pull class", { ui: { notify() {} } });
  assert.match(String((sent[0] as unknown[])[0]), /spide_lucid with action "pull".*class/);
  delete process.env.SPIDE_CONTROL_TOKEN;
  delete process.env.SPIDE_CONTROL_SOCKET;
  delete process.env.SPIDE_CONTROL_GENERATION;
});
