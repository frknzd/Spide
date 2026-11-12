import { invoke } from "@tauri-apps/api/core";
import type { Pos, Snapshot, Tab, Target } from "./types";

export const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
const DEV_URL = "http://127.0.0.1:47800/dev/snapshot";

const call = <T = void>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
  if (inTauri) return invoke<T>(cmd, args);
  // browser preview (dev)
  if (cmd === "get_snapshot") return fetch(DEV_URL).then((r) => r.json());
  // dev harness (debug builds): drive the running app over HTTP
  return fetch(`http://127.0.0.1:47800/dev/invoke/${cmd}`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(args ?? {}) })
    .then(async (r) => (r.ok ? ((await r.json()) as T) : Promise.reject(await r.text())));
};

export const api = {
  snapshot: () => call<Snapshot>("get_snapshot"),
  prompt: (text: string, tab: Tab) => call("send_prompt", { text, tab }),
  abort: () => call("abort_agent"),
  restartAgent: () => call("restart_agent"),
  accept: (id: string) => call("accept_proposal", { id }),
  acceptAll: () => call("accept_all"),
  reject: (id: string) => call("reject_proposal", { id }),
  prune: (targets: Target[]) => call("prune", { targets }),
  pruneFeedback: (targets: Target[], text: string, tab: Tab) => call("prune_feedback", { targets, text, tab }),
  restore: (target: Target) => call("restore", { target }),
  defend: (id: string, option: string | null, custom: string | null, defense: string) =>
    call("submit_defense", { id, option, custom, defense }),
  overrule: (id: string) => call("overrule", { id }),
  dismiss: (id: string) => call("dismiss_decision", { id }),
  undo: () => call("undo"),
  redo: () => call("redo"),
  setLayout: (view: string, positions: Record<string, Pos>) => call("set_layout", { view, positions }),
  setExaminer: (strict: boolean) => call("set_examiner", { strict }),
  setShowPruned: (on: boolean) => call("set_show_pruned", { on }),
  setName: (name: string) => call("set_name", { name }),
  regenerate: (view: "usecase" | "sequence", scenario?: string) => call("regenerate", { view, scenario: scenario ?? null }),
  newDesign: (dir: string) => call("new_design", { dir }),
  open: (path: string) => call("open_design", { path }),
  saveAs: (path: string) => call("save_as", { path }),
  exportPng: (svg: string, path: string, scale: number) => call("export_png", { svg, path, scale }),
  exportPlantUml: (view: string, path: string) => call("export_plantuml", { view, path }),
  exportPlantUmlAll: (dir: string) => call<string[]>("export_plantuml_all", { dir }),
  reveal: () => call("reveal_project"),
};
