import assert from "node:assert/strict";
import test from "node:test";
import { footprintOf, proposalAt } from "../src/state/proposal-focus.ts";

const cls = (name: string, pkg: string | null, extra: Record<string, unknown> = {}) => ({ name, stereotype: null, package: pkg, is_abstract: false, attributes: [], methods: [], responsibilities: [], rationale: null, aggregate_root: false, status: "accepted", proposal: null, feedback: null, origin: null, ...extra }) as never;
const data = {
  classes: [
    cls("Loan", "domain", { methods: [{ name: "renew", status: "proposed", proposal: "p2" }, { name: "close", status: "accepted", proposal: null }] }),
    cls("LendingService", "application"),
    cls("AuditLog", "infrastructure", { status: "proposed", proposal: "p1" }),
  ],
  relations: [{ id: "r1", from: "LendingService", to: "AuditLog", kind: "dependency", status: "proposed", proposal: "p1" }, { id: "r0", from: "LendingService", to: "Loan", kind: "association", status: "accepted", proposal: null }] as never[],
  packages: [],
};

test("a hovered ghost resolves to its proposal; accepted elements don't", () => {
  assert.equal(proposalAt("class:AuditLog", data), "p1");
  assert.equal(proposalAt("relation:r1", data), "p1");
  assert.equal(proposalAt("method:Loan.renew", data), "p2");
  assert.equal(proposalAt("method:Loan.close", data), null);
  assert.equal(proposalAt("class:Loan", data), null);
  assert.equal(proposalAt(null, data), null);
});

test("footprint: what changes, what it connects to, and where it lives", () => {
  const f = footprintOf(data, new Set(["p1"]))!;
  assert.deepEqual([...f.changed], ["AuditLog"]);
  assert.deepEqual([...f.anchors], ["LendingService"]);
  assert.deepEqual([...f.relations], ["r1"]);
  assert.deepEqual([...f.packages].sort(), ["application", "infrastructure"]);
  assert.equal(f.items.length, 2);
  const m = footprintOf(data, new Set(["p2"]))!;
  assert.deepEqual([...m.changed], ["Loan"]);
  assert.deepEqual(m.items, [{ sign: "+", text: "Loan.renew()" }]);
  assert.equal(footprintOf(data, new Set(["nope"])), null);
});

test("proposal summary: rationale as given, factual change list and operation counts", async () => {
  const { proposalSummary } = await import("../src/state/proposal-focus.ts");
  const s = proposalSummary({ id: "p1", ops: [{ op: "add_class" }, { op: "add_relation" }], rationale: "  Audit every loan.  ", source: "agent" }, data);
  assert.equal(s.rationale, "Audit every loan.");
  assert.equal(s.items.length, 2);
  assert.equal(s.ops, "add class, add relation");
  const none = proposalSummary({ id: "brief", ops: [{ op: "set_brief" }, { op: "set_brief" }], rationale: " ", source: "agent" }, data);
  assert.equal(none.rationale, null, "no invented rationale");
  assert.deepEqual(none.items, []);
  assert.equal(none.ops, "set brief ×2");
});
