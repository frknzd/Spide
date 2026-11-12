import { listen } from "@tauri-apps/api/event";
import { useEffect, useRef, useState } from "react";
import { api, inTauri } from "./api";

/** Tauri event listener; a no-op in a plain browser. */
export function on<T>(event: string, f: (e: { payload: T }) => void): Promise<() => void> {
  if (!inTauri) return Promise.resolve(() => {});
  return listen<T>(event, f);
}
import type { AgentEvent, Snapshot } from "./types";

export function useSnapshot(): Snapshot | null {
  const [snap, setSnap] = useState<Snapshot | null>(null);
  useEffect(() => {
    let alive = true;
    api.snapshot().then((s) => alive && setSnap(s));
    const poll = inTauri ? null : setInterval(() => api.snapshot().then((s) => alive && setSnap(s)), 1500);
    const un = on<Snapshot>("snapshot", (e) => {
      setSnap((prev) => (prev && prev.rev > e.payload.rev && prev.pi_status === e.payload.pi_status ? prev : e.payload));
    });
    return () => {
      alive = false;
      if (poll) clearInterval(poll);
      un.then((f) => f());
    };
  }, []);
  return snap;
}

export interface ChatEntry {
  id: number;
  role: "user" | "agent" | "error" | "status";
  text: string;
  tools: string[];
  done: boolean;
  at: number;
}

/** Conversation feed reconstructed from agent events. */
export function useAgentFeed() {
  const [entries, setEntries] = useState<ChatEntry[]>([]);
  const [activity, setActivity] = useState<string | null>(null);
  const seq = useRef(0);
  useEffect(() => {
    const push = (e: Omit<ChatEntry, "id" | "at">) =>
      setEntries((xs) => [...xs.slice(-200), { ...e, id: ++seq.current, at: Date.now() }]);
    const updateLast = (f: (e: ChatEntry) => ChatEntry) =>
      setEntries((xs) => {
        const i = xs.map((x) => x.role).lastIndexOf("agent");
        if (i < 0 || xs[i].done) return xs;
        const copy = xs.slice();
        copy[i] = f(copy[i]);
        return copy;
      });
    const un = on<AgentEvent>("agent", ({ payload: ev }) => {
      switch (ev.kind) {
        case "user":
          push({ role: "user", text: ev.text, tools: [], done: true });
          setActivity("thinking…");
          break;
        case "assistant_start":
          setEntries((xs) => {
            const last = xs[xs.length - 1];
            if (last && last.role === "agent" && !last.done) return xs;
            return [...xs, { id: ++seq.current, role: "agent", text: "", tools: [], done: false, at: Date.now() }];
          });
          setActivity("thinking…");
          break;
        case "thinking":
          setActivity("thinking…");
          break;
        case "text":
          setActivity(null);
          updateLast((e) => ({ ...e, text: e.text + ev.delta }));
          break;
        case "tool":
          setActivity(toolLabel(ev.name));
          updateLast((e) => ({ ...e, tools: [...e.tools, ev.name] }));
          break;
        case "tool_error":
          push({ role: "error", text: `${ev.name}: ${ev.text}`, tools: [], done: true });
          break;
        case "status":
          setActivity(ev.text);
          break;
        case "error":
          push({ role: "error", text: ev.text, tools: [], done: true });
          setActivity(null);
          break;
        case "settled":
        case "exit":
          setActivity(null);
          setEntries((xs) => xs.map((x) => (x.done ? x : { ...x, done: true })));
          break;
      }
    });
    return () => {
      un.then((f) => f());
    };
  }, []);
  return { entries, activity };
}

function toolLabel(name: string): string {
  const map: Record<string, string> = {
    get_design: "reading the design…",
    get_metrics: "checking architecture health…",
    get_view: "reading a view…",
    propose_decision: "preparing a decision…",
    propose_ops: "sketching a proposal…",
    evaluate_defense: "examining your defence…",
    set_use_cases: "drawing use cases…",
    set_sequence: "drawing a sequence…",
  };
  return map[name] ?? `${name}…`;
}
