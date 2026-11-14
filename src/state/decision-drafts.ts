import type { Decision } from "./types";
export interface DecisionDraft { choice: string | null; custom: string; ownMode: boolean; defense: string; expanded: string[] }
const drafts = new Map<string, DecisionDraft>();
const key = (project: string, id: string) => `${project}\0${id}`;
export function decisionDraft(project: string, d: Decision): DecisionDraft {
  return drafts.get(key(project, d.id)) ?? { choice: d.chosen ?? null, custom: d.custom ?? "", ownMode: !!d.custom && !d.chosen, defense: d.state === "rejected" ? d.attempts.at(-1)?.defense ?? "" : "", expanded: [] };
}
export function saveDecisionDraft(project: string, id: string, draft: DecisionDraft) {
  const k = key(project, id);
  drafts.delete(k); drafts.set(k, draft);
  if (drafts.size > 200) drafts.delete(drafts.keys().next().value!);
}
