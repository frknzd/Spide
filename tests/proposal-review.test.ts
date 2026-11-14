import assert from "node:assert/strict";
import test from "node:test";
import { fieldDiffs, nextProposalId, operationLabel, patchOperation, reviewFootprint, samePlan, loadPlanDraft, storePlanDraft } from "../src/state/proposal-review.ts";

test("selection stays by ID and resolves to the next surviving plan", () => {
  assert.equal(nextProposalId(["a", "b", "c"], "b"), "b");
  assert.equal(nextProposalId(["a", "c"], "b", ["a", "b", "c"]), "c");
  assert.equal(nextProposalId(["a", "b"], "c", ["a", "b", "c"]), "b");
  assert.equal(nextProposalId([], "a", ["a"]), null);
});
test("diffs expose metadata changes without ghost bookkeeping noise", () => {
  assert.deepEqual(fieldDiffs({ role: "old", status: "accepted", proposal: null }, { role: "new", status: "proposed", proposal: "p" }), [{ field: "role", before: "old", after: "new" }]);
  assert.match(operationLabel({ op: "remove_class" }), /^Delete/);
  assert.match(operationLabel({ op: "update_class" }), /^Update/);
});
test("structured patching preserves advanced fields and nested method properties", () => {
  const op = { op: "update_method", class: "A", name: "run", custom: "keep", method: { name: "run", concern: "audit", calls: ["B.log"], description: "old" } };
  const edited = patchOperation(op, "description", "new", "method");
  assert.equal(edited.custom, "keep");
  assert.deepEqual(edited.method, { ...op.method, description: "new" });
  assert.deepEqual(JSON.parse(JSON.stringify(edited)), edited);
  assert.equal(op.method.description, "old");
});
test("review footprint includes updates even without ghost status", () => {
  const f = reviewFootprint({ steps: [{ changes: [{ path: "classes.A", before: { role: "old" }, after: { role: "new" } }] }], design: { classes: [{ name: "A", package: "core" }] } } as never, "p")!;
  assert.ok(f.changed.has("A")); assert.ok(f.packages.has("core"));
});
test("drafts are scoped, survive workspace revisions and disappear when clean", () => {
  const values = new Map<string, string>();
  Object.defineProperty(globalThis, "localStorage", { configurable: true, value: { getItem: (k: string) => values.get(k) ?? null, setItem: (k: string, v: string) => values.set(k, v), removeItem: (k: string) => values.delete(k) } });
  const original = { id: "p", ops: [{ op: "add_class", name: "A" }], source: "agent", rationale: null };
  const plan = { ...original, rationale: "amended" };
  const draft = { scope: "project:branch:p", plan, original, rev: 7 };
  assert.ok(!samePlan(plan, original)); storePlanDraft(draft);
  assert.deepEqual(loadPlanDraft(draft.scope), draft);
  assert.equal(loadPlanDraft("other:branch:p"), null);
  storePlanDraft({ ...draft, original: plan }); assert.equal(loadPlanDraft(draft.scope), null);
});
test("drafts also survive scope switches when browser storage is unavailable", () => {
  Object.defineProperty(globalThis, "localStorage", { configurable: true, get() { throw Error("Storage disabled"); } });
  const original = { id: "p", ops: [{ op: "add_class", name: "A" }], source: "agent", rationale: null };
  const draft = { scope: "storage-disabled:branch:p", plan: { ...original, rationale: "keep me" }, original, rev: 2 };
  storePlanDraft(draft); assert.deepEqual(loadPlanDraft(draft.scope), draft);
  assert.equal(loadPlanDraft("some-other-scope"), null);
  storePlanDraft({ ...draft, plan: original }); assert.equal(loadPlanDraft(draft.scope), null);
});
