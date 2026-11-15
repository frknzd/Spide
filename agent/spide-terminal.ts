// App-owned pi extension: native TUI + a private structured control channel. No stdout protocol.
import type { ExtensionAPI, ExtensionContext } from "@earendil-works/pi-coding-agent";
import { connect, type Socket } from "node:net";

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
  const commands = ["tidy", "fit", "find", "focus", "detail", "lens", "implicit", "external", "brief", "rules", "mode", "strict", "lenient", "accept", "reject", "undo", "redo", "add", "pruned", "timeline", "branch", "switch", "compare", "merge", "project", "readjust", "implement", "verify", "synced", "reindex", "coderoot", "export", "open", "new", "save", "name", "folder", "agent restart"];
  pi.registerCommand("spide", {
    description: "Spide canvas commands (e.g. /spide tidy, /spide find Order). Pi commands stay native.",
    getArgumentCompletions: (prefix) => commands.filter((c) => c.startsWith(prefix)).map((c) => ({ value: c, label: c })),
    handler: async (args, c) => {
      try { await request("command", { command: "/" + (args.trim() || "brief") }); }
      catch (err) { c.ui.notify(String(err), "error"); }
    },
  });
}
