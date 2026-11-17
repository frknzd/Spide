// App-owned pi extension: native TUI + a private structured control channel. No stdout protocol.
import type { ExtensionAPI, ExtensionContext } from "@earendil-works/pi-coding-agent";
import { connect, type Socket } from "node:net";
import { readFileSync } from "node:fs";
import { join } from "node:path";

export default function (pi: ExtensionAPI) {
  const path = process.env.SPIDE_CONTROL_SOCKET;
  const token = process.env.SPIDE_CONTROL_TOKEN;
  const generation = Number(process.env.SPIDE_CONTROL_GENERATION);
  if (!path || !token) return;
  let socket: Socket | undefined;
  let ready = false;
  let stopping = false;
  let retry: ReturnType<typeof setTimeout> | undefined;
  let ctx: ExtensionContext;
  let seq = 0;
  const pending = new Map<string, { resolve: (v: any) => void; reject: (e: Error) => void; timer: ReturnType<typeof setTimeout> }>();
  const responses = new Map<string, object>();
  const send = (v: object) => {
    if (!socket || socket.destroyed) throw new Error("Spide connection is unavailable");
    const line = JSON.stringify({ ...v, generation });
    if (line.length > 1024 * 1024) throw new Error("Spide message too large");
    socket.write(line + "\n");
  };
  const request = (method: string, payload: object) => new Promise<any>((resolve, reject) => {
    if (!ready) return reject(new Error("Spide integration is reconnecting; try again shortly"));
    const id = `pi-${++seq}`;
    const timer = setTimeout(() => { pending.delete(id); reject(new Error("Spide request timed out")); }, 8000);
    pending.set(id, { resolve, reject, timer });
    try { send({ type: "request", id, method, payload }); }
    catch (e) { clearTimeout(timer); pending.delete(id); reject(e); }
  });
  const event = (name: string) => { if (ready) send({ type: "event", event: name }); };
  function open() {
    stopping = false;
    let text = "";
    socket = connect(path!);
    socket.setEncoding("utf8");
    socket.on("connect", () => send({ type: "hello", token, tools: pi.getActiveTools() }));
    socket.on("error", () => {}); // close handles reconnect; don't print socket path/token
    socket.on("close", () => {
      ready = false;
      for (const p of pending.values()) { clearTimeout(p.timer); p.reject(new Error("Spide disconnected")); }
      pending.clear();
      if (!stopping) retry = setTimeout(open, 400);
    });
    socket.on("data", async (chunk: string) => {
      text += chunk;
      if (text.length > 1024 * 1024) { socket?.destroy(); return; }
      let n: number;
      while ((n = text.indexOf("\n")) >= 0) {
        const line = text.slice(0, n); text = text.slice(n + 1);
        let v: any;
        try { v = JSON.parse(line); } catch { socket?.destroy(); return; }
        if (v.generation !== generation) continue;
        if (v.type === "ready") { ready = true; continue; }
        if (v.type === "response") {
          const p = pending.get(v.id);
          if (p) { pending.delete(v.id); clearTimeout(p.timer); v.ok ? p.resolve(v.result) : p.reject(new Error(v.error)); }
          continue;
        }
        if (v.type !== "request") continue;
        if (responses.has(v.id)) { send(responses.get(v.id)!); continue; }
        let reply: object;
        try {
          if (v.method === "prompt") {
            // No synthetic typing: do not touch ctx.ui's editor (it may hold a draft).
            pi.sendUserMessage(v.payload.message, ctx.isIdle() ? undefined : { deliverAs: "followUp" });
          } else if (v.method === "abort") {
            ctx.abort();
          } else throw new Error("Unknown Spide request");
          reply = { type: "response", id: v.id, ok: true, result: { accepted: true } };
        } catch (e) { reply = { type: "response", id: v.id, ok: false, error: String(e) }; }
        if (responses.size > 128) responses.clear();
        responses.set(v.id, reply);
        send(reply);
      }
    });
  }
  pi.on("session_start", (_e, c) => { ctx = c; open(); });
  pi.on("session_shutdown", () => {
    event("shutdown"); stopping = true; ready = false; clearTimeout(retry); socket?.destroy();
  });
  pi.on("agent_start", (_e, c) => { ctx = c; event("busy"); });
  pi.on("agent_settled", (_e, c) => { ctx = c; event("settled"); });
  pi.on("model_select", (_e, c) => { ctx = c; event("model"); });
  pi.on("input", async (e, c) => {
    ctx = c;
    if (e.source === "extension") return { action: "continue" };
    try {
      const result = await request("context", { text: e.text });
      return { action: "transform", text: result.text, images: e.images };
    } catch (err) {
      c.ui.notify(String(err), "error");
      // Preserve the prompt for retry; never send an undecorated turn silently.
      c.ui.setEditorText(e.text);
      return { action: "handled" };
    }
  });
  // ---------------------------------------------------------------- Lucid sync
  // pi talks to Lucid's hosted MCP server (OAuth via `/mcp login lucid`); Spide's MCP tools plan
  // and verify. spide_lucid pipes JSON tool → tool, so diagram scripts never pass through the model.
  const LUCID = { url: "https://mcp.lucid.app/mcp", exposure: "codemode", description: "Lucid (Lucidchart): documents Spide keeps in sync with the design" };
  let lucidRegistered = false;
  const registerLucid = () => {
    if (lucidRegistered || typeof (pi as any).registerMcpServer !== "function") return;
    try { (pi as any).registerMcpServer("lucid", LUCID); lucidRegistered = true; } catch { /* a user-level `lucid` server wins */ lucidRegistered = true; }
  };
  const lucidEnabled = () => {
    try { return JSON.parse(readFileSync(join(process.cwd(), ".spide", "lucid", "config.json"), "utf8")).enabled === true; } catch { return false; }
  };
  pi.registerTool({
    name: "spide_lucid",
    label: "Spide ↔ Lucid",
    description: "Sync Spide's diagrams with Lucid documents (one per diagram, frontier branch only), git-like. " +
      "status: per diagram in_sync | local_ahead | remote_ahead | diverged | unlinked. push: update the Lucid documents in place " +
      "(creates missing ones; refused when Lucid changed since the last sync — pull first). pull: colleagues' Lucid edits become " +
      "ghost proposals and their comments/notes Spide comments. enable: turn sync on. link: attach a diagram to an existing document (key + url).",
    promptSnippet: "Push/pull Spide diagrams to/from Lucid documents (git-like sync)",
    promptGuidelines: ["Use spide_lucid for anything Lucid-related in Spide; never hand-write Lucid scripts or diagram JSON for Spide's diagrams."],
    parameters: { type: "object", properties: {
      action: { type: "string", enum: ["status", "push", "pull", "enable", "link"] },
      key: { type: "string", description: "one diagram (class, domain, package, usecase, system.context, system.containers, sequence:<name>); omitted: all" },
      url: { type: "string", description: "link: the Lucid document URL" },
    }, required: ["action"] },
    reachesTools: () => ["mcp__lucid", "mcp__spide"],
    async execute(_id: string, params: any, signal: AbortSignal, onUpdate: any, tctx: any) {
      const log: string[] = [];
      const say = (s: string) => { log.push(s); onUpdate?.({ content: [{ type: "text", text: log.join("\n") }] }); };
      const text = (r: any) => (r?.content ?? []).filter((c: any) => c.type === "text").map((c: any) => c.text).join("");
      const call = async (name: string, args: object) => {
        const r = await tctx.executeTool(name, args, { signal });
        const t = text(r);
        // Lucid reports script failures as text (the script's canvas changes were reverted)
        if (r?.isError || (name === "mcp__lucid__lucid_run_script" && /^Error running script/.test(t))) {
          if (name.startsWith("mcp__lucid")) throw new Error(`${t}\nIs Lucid connected? Sign in with /mcp login lucid (a Team/Enterprise admin may have to enable Lucid's MCP access).`);
          throw new Error(t);
        }
        try { return JSON.parse(t); } catch { return t; }
      };
      const spide = (tool: string, args: object = {}) => call(`mcp__spide__${tool}`, args);
      const lucid = (tool: string, args: object) => call(`mcp__lucid__${tool}`, args);
      let script = "";
      const read = async (doc: string) => {
        script ||= await spide("lucid_read_script");
        await lucid("lucid_load_script_documentation", { name: "uml", document_id: doc });
        const r = await lucid("lucid_run_script", { source: script, document_id: doc });
        return r?.result ?? r;
      };
      try {
        if (params.action === "enable" || params.action === "link") {
          const r = await spide("lucid_link", params.action === "link" ? { key: params.key, url: params.url } : {});
          registerLucid();
          return { content: [{ type: "text", text: `Lucid sync enabled (${JSON.stringify(r)}). If Lucid isn't signed in yet, ask the user to run /mcp login lucid.` }] };
        }
        registerLucid();
        const status = await spide("lucid_status");
        if (!status.enabled) return { content: [{ type: "text", text: "Lucid sync is off for this project: spide_lucid action=enable turns it on." }] };
        if (!status.sync_allowed && params.action !== "status") return { content: [{ type: "text", text: status.reason }], isError: true };
        const rows: any[] = status.diagrams.filter((d: any) => !params.key || d.key === params.key);
        if (!rows.length) throw new Error(`no diagram ${params.key}`);
        if (params.action === "status") {
          const snapshots: Record<string, unknown> = {};
          for (const d of rows.filter((d) => d.document_id)) { say(`reading ${d.title}…`); snapshots[d.key] = await read(d.document_id); }
          const s = await spide("lucid_status", { snapshots });
          const lines = s.diagrams.filter((d: any) => !params.key || d.key === params.key).map((d: any) => `${d.key}: ${d.status}${d.url ? ` — ${d.url}` : ""}${d.pending_proposals?.length ? ` (undecided: ${d.pending_proposals.join(", ")})` : ""}`);
          return { content: [{ type: "text", text: lines.join("\n") }] };
        }
        if (params.action === "pull") {
          for (const d of rows.filter((d) => d.document_id)) {
            say(`pulling ${d.title}…`);
            const r = await spide("lucid_pull", { key: d.key, snapshot: await read(d.document_id) });
            say(`${d.key}: ${r.proposals.length} proposal(s), ${r.comments_new_or_updated} comment(s)${r.warnings.length ? `; ${r.warnings.join("; ")}` : ""}`);
          }
          return { content: [{ type: "text", text: log.join("\n") || "no linked documents to pull" }] };
        }
        // push
        let folder = status.folder_id as string | null;
        for (const d of rows.filter((d) => d.in_design)) {
          say(`pushing ${d.title}…`);
          let doc = d.document_id as string | null;
          const plan = await spide("lucid_plan_push", { key: d.key, snapshot: doc ? await read(doc) : null });
          let url = d.url;
          if (plan.create) {
            if (!folder) {
              const f = await lucid("lucid_create_folder", { name: plan.title.split(" · ")[0] });
              folder = String(f.id ?? f.folderId ?? "") || null;
              if (folder) await spide("lucid_link", { folder_id: folder });
            }
            const created = await lucid("lucid_create_blank_document", { name: plan.title, product: "lucidchart" });
            doc = created.documentId; url = created.editUrl;
            if (folder) await lucid("lucid_update_document_metadata", { document_id: doc, parent: Number(folder) || folder });
            await lucid("lucid_load_script_documentation", { name: "uml", document_id: doc });
          }
          for (const [i, chunk] of plan.chunks.entries()) {
            say(`  edit ${i + 1}/${plan.chunks.length}`);
            await lucid("lucid_run_script", { source: chunk, document_id: doc });
          }
          await spide("lucid_record_push", { key: d.key, document_id: doc, url, snapshot: await read(doc!) });
          say(`${d.key}: in sync — ${url}`);
        }
        return { content: [{ type: "text", text: log.join("\n") }] };
      } catch (e) {
        const m = String((e as Error)?.message ?? e);
        let refusal: any; try { refusal = JSON.parse(m); } catch { /* plain error */ }
        return { content: [{ type: "text", text: [...log, refusal?.message ?? m].join("\n") }], isError: true };
      }
    },
  } as any);
  if (lucidEnabled()) registerLucid();
  const commands = ["tidy", "fit", "find", "focus", "detail", "lens", "implicit", "external", "brief", "rules", "mode", "strict", "lenient", "accept", "reject", "undo", "redo", "add", "pruned", "timeline", "trace", "goto", "nvim", "branch", "switch", "compare", "merge", "project", "readjust", "implement", "verify", "synced", "reindex", "coderoot", "export", "open", "new", "save", "name", "folder", "agent restart", "lucid status", "lucid push", "lucid pull", "lucid enable", "lucid link"];
  pi.registerCommand("spide", {
    description: "Spide canvas commands (e.g. /spide tidy, /spide find Order). Pi commands stay native.",
    getArgumentCompletions: (prefix) => commands.filter((c) => c.startsWith(prefix)).map((c) => ({ value: c, label: c })),
    handler: async (args, c) => {
      const lucid = args.trim().match(/^lucid\s*(status|push|pull|enable|link)?\s*(.*)$/);
      if (lucid) {
        // the agent runs spide_lucid (tool context: it can reach the Lucid and Spide MCP servers)
        const [, action = "status", rest] = lucid;
        const extra = action === "link" ? ` Link ${rest.split(/\s+/)[0] || "the diagram"} to ${rest.split(/\s+/)[1] || "the document the user names"}.` : rest ? ` Only the ${rest} diagram.` : "";
        pi.sendUserMessage(`Run spide_lucid with action "${action}".${extra} Report the result briefly.`, ctx?.isIdle?.() === false ? { deliverAs: "followUp" } : undefined);
        return;
      }
      try { await request("command", { command: "/" + (args.trim() || "brief") }); }
      catch (err) { c.ui.notify(String(err), "error"); }
    },
  });
}
