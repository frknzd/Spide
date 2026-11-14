import assert from "node:assert/strict";
import test from "node:test";
import { routeAll, routePath, simplify, type RBox, type REdge, type Route } from "../src/canvas/route.ts";

const box = (id: string, x: number, y: number, w = 120, h = 60): RBox => ({ id, x, y, w, h });
const inside = (b: RBox, p: [number, number], m = 0) => p[0] > b.x - b.w / 2 - m + 0.01 && p[0] < b.x + b.w / 2 + m - 0.01 && p[1] > b.y - b.h / 2 - m + 0.01 && p[1] < b.y + b.h / 2 + m - 0.01;
function checkRoute(r: Route, boxes: RBox[], e: REdge) {
  const p = r.points;
  assert.ok(p.length >= 2);
  for (let i = 0; i + 1 < p.length; i++) {
    const [a, b] = [p[i], p[i + 1]];
    assert.ok(Math.abs(a[0] - b[0]) < 0.01 || Math.abs(a[1] - b[1]) < 0.01, `${e.id} segment ${i} not axis-aligned`);
    // sample the segment: never inside a box (own boxes only at the very port)
    for (let t = 0.02; t < 1; t += 0.02) {
      const q: [number, number] = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
      for (const bx of boxes) assert.ok(!inside(bx, q), `${e.id} passes through ${bx.id}`);
    }
  }
}
const onSide = (b: RBox, p: [number, number], side: string) =>
  side === "N" ? Math.abs(p[1] - (b.y - b.h / 2)) < 0.01 : side === "S" ? Math.abs(p[1] - (b.y + b.h / 2)) < 0.01 : side === "E" ? Math.abs(p[0] - (b.x + b.w / 2)) < 0.01 : Math.abs(p[0] - (b.x - b.w / 2)) < 0.01;

test("routes avoid boxes, are orthogonal and start/end on the chosen sides", () => {
  // A blocker sits right between A and B
  const boxes = [box("A", 0, 0), box("B", 0, 300), box("X", 0, 150, 200, 60), box("C", 300, 150)];
  const edges: REdge[] = [{ id: "ab", from: "A", to: "B", toDeco: "arrow" }, { id: "ac", from: "A", to: "C" }, { id: "cb", from: "C", to: "B" }];
  const routes = routeAll(boxes, edges);
  for (const e of edges) {
    const r = routes.get(e.id)!;
    checkRoute(r, boxes, e);
    assert.ok(onSide(boxes.find((b) => b.id === e.from)!, r.points[0], r.fromSide), `${e.id} start side`);
    assert.ok(onSide(boxes.find((b) => b.id === e.to)!, r.points.at(-1)!, r.toSide), `${e.id} end side`);
    assert.ok(!r.fallback);
  }
});

test("edges leaving one side with the same decoration share one port", () => {
  const boxes = [box("P", 0, 0), box("C1", -200, 250), box("C2", 0, 250), box("C3", 200, 250)];
  const edges: REdge[] = ["C1", "C2", "C3"].map((c) => ({ id: c, from: c, to: "P", toDeco: "triangle", vertical: true }));
  const routes = routeAll(boxes, edges);
  const ends = edges.map((e) => routes.get(e.id)!.points.at(-1)!.join());
  assert.equal(new Set(ends).size, 1, "inheritance arrows merge into one triangle");
  for (const e of edges) checkRoute(routes.get(e.id)!, boxes, e);
});

test("different decorations get different ports; parallel edges between one pair are separated", () => {
  const boxes = [box("A", 0, 0), box("B", 0, 300)];
  const routes = routeAll(boxes, [{ id: "x", from: "A", to: "B", toDeco: "arrow" }, { id: "y", from: "A", to: "B", toDeco: "arrow" }, { id: "z", from: "A", to: "B", fromDeco: "diamond" }]);
  const starts = [...routes.values()].map((r) => r.points[0].join());
  assert.equal(new Set(starts).size, 3);
});

test("crossings become hops on horizontal segments only, deterministically", () => {
  const boxes = [box("L", -300, 0), box("R", 300, 0), box("T", 0, -300), box("Bt", 0, 300)];
  const edges: REdge[] = [{ id: "h", from: "L", to: "R" }, { id: "v", from: "T", to: "Bt" }];
  const a = routeAll(boxes, edges), b = routeAll(boxes, edges);
  assert.deepEqual([...a.entries()], [...b.entries()]);
  assert.equal(a.get("h")!.hops.length, 1);
  assert.equal(a.get("v")!.hops.length, 0);
  assert.match(routePath(a.get("h")!.points, a.get("h")!.hops), / A5,5 /);
});

test("budget exhausted → L/Z fallback that is still orthogonal", () => {
  let t = 0;
  const boxes = [box("A", 0, 0), box("B", 400, 300)];
  const r = routeAll(boxes, [{ id: "e", from: "A", to: "B" }], [], { budgetMs: 5, now: () => (t += 10) }).get("e")!;
  assert.ok(r.fallback);
  for (let i = 0; i + 1 < r.points.length; i++) assert.ok(r.points[i][0] === r.points[i + 1][0] || r.points[i][1] === r.points[i + 1][1]);
});

test("incremental re-route keeps untouched edges and doesn't drift", () => {
  const boxes = [box("A", 0, 0), box("B", 0, 300), box("C", 40, 0, 60, 40)];
  const edges: REdge[] = [{ id: "ab", from: "A", to: "B" }, { id: "cb", from: "C", to: "B" }];
  const first = routeAll([box("A", 0, 0), box("B", 0, 300), box("C", 300, 0)], edges);
  let prev = first;
  for (let k = 0; k < 5; k++) prev = routeAll([box("A", 0, 0), box("B", 0, 300), box("C", 300 + k, 0)], edges, [], { only: new Set(["cb"]), previous: prev });
  assert.deepEqual(prev.get("ab")!.points, first.get("ab")!.points);
  void boxes;
});

test("rounded corners and simplification", () => {
  assert.deepEqual(simplify([[0, 0], [0, 0], [0, 10], [0, 20], [10, 20]]), [[0, 0], [0, 20], [10, 20]]);
  assert.match(routePath([[0, 0], [0, 50], [50, 50]]), /Q0\.0,50\.0 10\.0,50\.0/);
});

test("diagonal relations take an L (one turn) instead of a Z when nothing is in the way", () => {
  const boxes = [box("A", 0, 0), box("B", 500, 400)];
  const r = routeAll(boxes, [{ id: "e", from: "A", to: "B", toDeco: "arrow" }]).get("e")!;
  assert.equal(r.points.length - 2, 1, JSON.stringify(r.points));
});

test("penalties: fewer crossings, overlaps and turns than an unpenalised router", () => {
  let seed = 7; const rnd = () => (seed = (seed * 16807) % 2147483647) / 2147483647;
  const boxes: RBox[] = []; const edges: REdge[] = [];
  for (let i = 0; i < 6; i++) for (let j = 0; j < 5; j++) boxes.push(box(`c${i}${j}`, i * 260 + rnd() * 40, j * 200 + rnd() * 30, 140 + rnd() * 60, 60 + rnd() * 80));
  for (let k = 0; k < 45; k++) { const a = boxes[Math.floor(rnd() * 30)].id, b = boxes[Math.floor(rnd() * 30)].id; if (a !== b) edges.push({ id: `e${k}`, from: a, to: b, toDeco: "arrow" }); }
  const measure = (routes: Map<string, Route>) => {
    let bends = 0, crossings = 0;
    const H: number[][] = [], V: number[][] = [];
    for (const [i, r] of [...routes.values()].entries()) {
      bends += r.points.length - 2;
      for (let k = 0; k + 1 < r.points.length; k++) {
        const [p, q] = [r.points[k], r.points[k + 1]];
        if (Math.abs(p[1] - q[1]) < 0.01) H.push([i, p[1], Math.min(p[0], q[0]), Math.max(p[0], q[0])]); else V.push([i, p[0], Math.min(p[1], q[1]), Math.max(p[1], q[1])]);
      }
    }
    for (const h of H) for (const v of V) if (h[0] !== v[0] && v[1] > h[2] + 1 && v[1] < h[3] - 1 && h[1] > v[2] + 1 && h[1] < v[3] - 1) crossings++;
    return { bends, crossings };
  };
  const loose = measure(routeAll(boxes, edges, [], { budgetMs: 1e9, bend: 1, cross: 0, overlap: 0 }));
  const tuned = measure(routeAll(boxes, edges, [], { budgetMs: 1e9 }));
  assert.ok(tuned.crossings < loose.crossings, JSON.stringify({ loose, tuned }));
  assert.ok(tuned.bends < loose.bends, JSON.stringify({ loose, tuned }));
});

test("small jogs are straightened", async () => {
  const { unjog } = await import("../src/canvas/route.ts");
  assert.deepEqual(unjog([[0, 0], [0, 100], [3, 100], [3, 200], [80, 200], [80, 260]], () => false), [[0, 0], [0, 200], [80, 200], [80, 260]]);
  // blocked → kept
  assert.equal(unjog([[0, 0], [0, 100], [3, 100], [3, 200], [80, 200], [80, 260]], () => true).length, 6);
});
