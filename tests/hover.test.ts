import { test } from "node:test";
import assert from "node:assert/strict";
import { HoverStore, placeCard } from "../src/state/hover.ts";

test("motion over one target doesn't postpone the intent timer", (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const s = new HoverStore();
  s.move("class:A", { x: 1, y: 2 });
  t.mock.timers.tick(40);
  s.move("class:A", { x: 9, y: 10 });
  t.mock.timers.tick(20);
  assert.equal(s.snapshot()?.key, "class:A");
  assert.equal(s.snapshot()?.at.x, 9);
  s.move("method:A.f", { x: 2, y: 3 });
  assert.equal(s.snapshot()?.key, "method:A.f");
  assert.equal(s.classKey(), "A");
  s.move(null, { x: 0, y: 0 });
  t.mock.timers.tick(79);
  assert.ok(s.snapshot());
  t.mock.timers.tick(1);
  assert.equal(s.snapshot(), null);
});
test("clear cancels stale opening", (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const s = new HoverStore();
  s.move("class:A", { x: 1, y: 1 });
  s.clear();
  t.mock.timers.tick(100);
  assert.equal(s.snapshot(), null);
});
test("measured card fits the canvas, not side panes", () => {
  const p = placeCard({ x: 850, y: 700 }, { width: 360, height: 1000 }, { left: 420, top: 40, right: 900, bottom: 800 });
  assert.ok(p.left >= 428);
  assert.ok(p.left + 360 <= 892);
  assert.equal(p.maxHeight, 744);
  assert.equal(p.top, 48);
});
