import assert from "node:assert/strict";
import test from "node:test";
import { evidenceLabel, memberDiagnostics } from "../src/state/insights.ts";

const issue = (kind: string, targets: string[], severity = 0.3, confidence = 0.8, basis = "heuristic") =>
  ({ kind, severity, confidence, basis, message: `${kind} message`, targets, rule: null, explain: `${kind} explanation`, fix_hint: "review" });
const design = { classes: [{ name: "Order", attributes: [{ name: "total" }, { name: "state" }], methods: [{ name: "total" }, { name: "submit" }, { name: "submit" }] }] } as never;

test("member diagnostics resolve explicit targets and keep multiple findings", () => {
  const out = memberDiagnostics({ issues: [issue("long_method", ["Order", "Order.submit"], 0.25, 0.9, "code"), issue("demeter", ["Order", "Order.submit"], 0.3, 0.7), issue("god", ["Order"])] } as never, design);
  const d = out.get("Order")!.get("method:submit")!;
  assert.deepEqual(d.issues.map((i) => i.kind), ["long_method", "demeter"]);
  assert.equal(d.family, "complexity");
  assert.equal(out.get("Order")!.size, 1, "class-only findings are not spread to rows");
});

test("ambiguous member names are resolved by finding kind or skipped", () => {
  const out = memberDiagnostics({ issues: [issue("exposed_state", ["Order", "Order.total"]), issue("long_method", ["Order", "Order.total"]), issue("unknown", ["Order", "Order.total"]), issue("long_method", ["Missing.run"])] } as never, design);
  const per = out.get("Order")!;
  assert.equal(per.get("attribute:total")!.issues[0].kind, "exposed_state");
  assert.equal(per.get("method:total")!.issues[0].kind, "long_method");
  assert.equal(per.get("method:total")!.issues.length, 1);
  assert.equal(out.has("Missing"), false);
});

test("evidence wording separates structural, measured and potential findings", () => {
  assert.equal(evidenceLabel({ basis: "structure", confidence: 1 }), "structural finding");
  assert.equal(evidenceLabel({ basis: "code", confidence: 0.9 }), "measured in code");
  assert.equal(evidenceLabel({ basis: "heuristic", confidence: 0.55 }), "potential concern");
});
