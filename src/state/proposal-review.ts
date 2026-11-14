import type { Op, Proposal, ProposalReviewData } from "./types";
import type { ProposalFootprint } from "./proposal-focus";

/** Structured forms patch fields without dropping advanced/unexposed values. */
export function patchOperation(op: Op, field: string, value: unknown, nested?: string): Op {
  return nested ? { ...op, [nested]: { ...(op[nested] as Record<string, unknown> ?? {}), [field]: value } } : { ...op, [field]: value };
}
export interface PlanDraft { scope: string; plan: Proposal; original: Proposal; rev: number }
const memoryDrafts = new Map<string, PlanDraft>();
const storageKey = (scope: string) => `spide.proposal-draft.v1:${scope}`;
export function loadPlanDraft(scope: string): PlanDraft | null {
  if (memoryDrafts.has(scope)) return structuredClone(memoryDrafts.get(scope)!);
  try { const d = JSON.parse(localStorage.getItem(storageKey(scope)) ?? "null"); return d?.scope === scope && Array.isArray(d.plan?.ops) && Array.isArray(d.original?.ops) && typeof d.rev === "number" && !samePlan(d.plan, d.original) ? d : null; } catch { return null; }
}
export function storePlanDraft(draft: PlanDraft) {
  if (samePlan(draft.plan, draft.original)) memoryDrafts.delete(draft.scope); else memoryDrafts.set(draft.scope, structuredClone(draft));
  try { if (samePlan(draft.plan, draft.original)) localStorage.removeItem(storageKey(draft.scope)); else localStorage.setItem(storageKey(draft.scope), JSON.stringify(draft)); } catch { /* Keep the in-memory draft if storage is unavailable. */ }
}

export function nextProposalId(ids: string[], selected: string | null, previous: string[] = []): string | null {
  if (selected && ids.includes(selected)) return selected;
  const index = selected ? previous.indexOf(selected) : 0;
  return ids[Math.min(Math.max(index, 0), ids.length - 1)] ?? null;
}
export const samePlan = (a: Proposal, b: Proposal) => JSON.stringify([a.ops, a.rationale, a.decision ?? null, a.option ?? null]) === JSON.stringify([b.ops, b.rationale, b.decision ?? null, b.option ?? null]);
export const operationLabel = (op: Op) => `${op.op.startsWith("add_") ? "Add" : op.op.startsWith("remove_") ? "Delete" : "Update"} · ${op.op.replaceAll("_", " ")}`;
export function operationTarget(op: Op): string | null {
  return typeof op.class === "string" ? op.class : typeof op.from === "string" && /class|relation/.test(op.op) ? op.from : typeof op.name === "string" && /class/.test(op.op) ? op.name : null;
}
export interface FieldDiff { field: string; before: unknown; after: unknown }
export function fieldDiffs(before: unknown, after: unknown, prefix = ""): FieldDiff[] {
  if (JSON.stringify(before) === JSON.stringify(after)) return [];
  const object = (v: unknown): v is Record<string, unknown> => v !== null && typeof v === "object" && !Array.isArray(v);
  if ((object(before) && (object(after) || after == null)) || (object(after) && before == null)) {
    const a = (before ?? {}) as Record<string, unknown>, b = (after ?? {}) as Record<string, unknown>;
    return [...new Set([...Object.keys(a), ...Object.keys(b)])].filter((key) => !["status", "proposal", "origin"].includes(key)).flatMap((key) => fieldDiffs(a[key], b[key], prefix ? `${prefix}.${key}` : key));
  }
  return [{ field: prefix || "value", before, after }];
}
export function reviewFootprint(review: ProposalReviewData | null, id: string | null): ProposalFootprint | null {
  if (!review || !id) return null;
  const f: ProposalFootprint = { ids: new Set([id]), changed: new Set(), anchors: new Set(), relations: new Set(), packages: new Set(), items: [] };
  for (const step of review.steps) for (const c of step.changes) {
    const [kind, ...rest] = c.path.split("."), name = rest.join(".");
    if (kind === "classes") f.changed.add(name);
    if (kind === "packages") f.packages.add(name);
    if (kind === "relations") {
      f.relations.add(name);
      for (const value of [c.before, c.after]) {
        const r = value as { from?: string; to?: string } | null;
        if (r?.from) f.anchors.add(r.from); if (r?.to) f.anchors.add(r.to);
      }
    }
  }
  for (const name of f.changed) f.anchors.delete(name);
  for (const c of review.design.classes) if ((f.changed.has(c.name) || f.anchors.has(c.name)) && c.package) f.packages.add(c.package);
  return f.changed.size || f.relations.size || f.packages.size ? f : null;
}
