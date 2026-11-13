import { invoke } from "@tauri-apps/api/core";
import type { BranchCompare, BranchInfo, Design, DriftReport, ImportSummary, HistoryEntry, Issue, Op, Pos, Rules, Snapshot, Tab, Target } from "./types";

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
  setDetail: (view: string, level: number) => call("set_detail", { view, level }),
  setName: (name: string) => call("set_name", { name }),
  userOps: (ops: Op[]) => call("apply_user_ops", { ops }),
  setPartner: (mode: string) => call("set_partner", { mode }),
  setRules: (rules: Rules) => call("set_rules", { rules }),
  branches: () => call<BranchInfo[]>("branch_list"),
  branchCreate: (name: string) => call("branch_create", { name }),
  branchSwitch: (name: string) => call("branch_switch", { name }),
  branchDelete: (name: string) => call("branch_delete", { name }),
  branchCompare: (name: string) => call<BranchCompare>("branch_compare", { name }),
  branchMerge: (name: string) => call("branch_merge", { name }),
  historyList: () => call<HistoryEntry[]>("history_list"),
  historyAt: (rev: number) => call<Design>("history_at", { rev }),
  historyRestore: (rev: number) => call("history_restore", { rev }),
  historyFork: (rev: number, name: string) => call("history_fork", { rev, name }),
  analyseChange: (name: string) => call("analyse_change", { name }),
  fixIssue: (i: Issue) => call("fix_issue", { message: i.message, rule: i.rule, fix_hint: i.fix_hint }),
  setBriefSkipped: (on: boolean) => call("set_brief_skipped", { on }),
  regenerate: (view: "usecase" | "sequence", scenario?: string) => call("regenerate", { view, scenario: scenario ?? null }),
  newDesign: (dir: string) => call("new_design", { dir }),
  open: (path: string) => call("open_design", { path }),
  saveAs: (path: string) => call("save_as", { path }),
  exportPng: (svg: string, path: string, scale: number) => call("export_png", { svg, path, scale }),
  exportPlantUml: (view: string, path: string) => call("export_plantuml", { view, path }),
  exportArchUnit: (path: string) => call("export_archunit", { path }),
  setCodeRoot: (root: string | null) => call("set_code_root", { root }),
  importCode: (dir: string) => call<ImportSummary>("import_code", { dir }),
  drift: () => call<DriftReport>("drift"),
  driftAdopt: () => call("drift_adopt"),
  exportAdr: (dir: string) => call<string[]>("export_adr", { dir }),
  exportPlantUmlAll: (dir: string) => call<string[]>("export_plantuml_all", { dir }),
  reveal: () => call("reveal_project"),
};
