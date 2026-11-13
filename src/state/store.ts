import { listen } from "@tauri-apps/api/event";
import { useEffect, useState, useSyncExternalStore } from "react";
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
    // browser dev mode polls; keep the old object when nothing changed so the canvas doesn't re-render
    const same = (a: Snapshot | null, b: Snapshot) => !!a && a.rev === b.rev && a.pi_status === b.pi_status && a.branch === b.branch;
    const poll = inTauri ? null : setInterval(() => api.snapshot().then((s) => alive && setSnap((prev) => (same(prev, s) ? prev : s))), 1500);
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

/** Conversation feed reconstructed from agent events. A module-level store, so only the
 *  components that show the chat re-render while the agent streams (not the whole app). */
interface Feed { entries: ChatEntry[]; activity: string | null }
let feed: Feed = { entries: [], activity: null };
const feedSubs = new Set<() => void>();
let feedSeq = 0;
let feedStarted = false;

function setFeed(f: (x: Feed) => Feed) {
  const next = f(feed);
  if (next === feed) return;
  feed = next;
  feedSubs.forEach((l) => l());
}

function startFeed() {
  if (feedStarted) return;
  feedStarted = true;
  const setEntries = (f: (xs: ChatEntry[]) => ChatEntry[]) => setFeed((x) => {
    const e = f(x.entries);
    return e === x.entries ? x : { ...x, entries: e };
  });
  const setActivity = (a: string | null) => setFeed((x) => (x.activity === a ? x : { ...x, activity: a }));
  const push = (e: Omit<ChatEntry, "id" | "at">) => setEntries((xs) => [...xs.slice(-200), { ...e, id: ++feedSeq, at: Date.now() }]);
  const updateLast = (f: (e: ChatEntry) => ChatEntry) =>
    setEntries((xs) => {
      const i = xs.map((x) => x.role).lastIndexOf("agent");
      if (i < 0 || xs[i].done) return xs;
      const copy = xs.slice();
      copy[i] = f(copy[i]);
      return copy;
    });
  on<AgentEvent>("agent", ({ payload: ev }) => {
    switch (ev.kind) {
      case "user":
        push({ role: "user", text: ev.text, tools: [], done: true });
        setActivity("thinking…");
        break;
      case "assistant_start":
        setEntries((xs) => {
          const last = xs[xs.length - 1];
          if (last && last.role === "agent" && !last.done) return xs;
          return [...xs, { id: ++feedSeq, role: "agent", text: "", tools: [], done: false, at: Date.now() }];
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
        setEntries((xs) => (xs.some((x) => !x.done) ? xs.map((x) => (x.done ? x : { ...x, done: true })) : xs));
        break;
    }
  });
}

function subscribeFeed(l: () => void) {
  startFeed();
  feedSubs.add(l);
  return () => void feedSubs.delete(l);
}

export function useAgentEntries(): ChatEntry[] {
  return useSyncExternalStore(subscribeFeed, () => feed.entries);
}

export function useAgentActivity(): string | null {
  return useSyncExternalStore(subscribeFeed, () => feed.activity);
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
