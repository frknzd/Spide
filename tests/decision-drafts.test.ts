import { test } from "node:test";
import assert from "node:assert/strict";
import { decisionDraft, saveDecisionDraft } from "../src/state/decision-drafts.ts";

test("drafts survive decision navigation/snapshots but are isolated by project", () => {
  const d = { id: "a", chosen: null, custom: null, state: "open", attempts: [] } as any;
  const draft = { choice: "A", custom: "", ownMode: false, defense: "keep this typed defence", expanded: ["A"] };
  saveDecisionDraft("project-one", "a", draft);
  assert.deepEqual(decisionDraft("project-one", { ...d, state: "evaluating" }), draft);
  assert.equal(decisionDraft("project-two", d).defense, "");
  assert.equal(decisionDraft("project-one", { ...d, id: "b" }).choice, null);
  assert.deepEqual(decisionDraft("project-one", { ...d, state: "rejected", attempts: [{ defense: "server text" }] }), draft);
});
