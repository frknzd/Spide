// Orthogonal ("PCB") edge router: north/south/east/west segments only, shared ports that fan out,
// no segment through a box, parallel segments nudged into lanes, hops where lines cross.
// Pure and deterministic; positions are whatever the layout/physics currently says.

export type Side = "N" | "S" | "E" | "W";
export type Deco = "none" | "arrow" | "triangle" | "diamond" | "odiamond";
export type Pt = [number, number];

/** A box (centre coordinates). `padTop` reserves room above it (chips drawn over the top border). */
export interface RBox { id: string; x: number; y: number; w: number; h: number; padTop?: number; padBottom?: number }
/** A soft zone (package hull): crossing it costs, its own members' edges may run inside freely. */
export interface RZone { id: string; x1: number; y1: number; x2: number; y2: number; members: Set<string> }
export interface REdge {
  id: string; from: string; to: string;
  /** decoration drawn at each end: ends with different decorations get different ports */
  fromDeco?: Deco; toDeco?: Deco;
  /** prefer vertical sides (inheritance: child below its parent) */
  vertical?: boolean;
}
export interface Route {
  points: Pt[];
  /** crossings drawn as a small hop on this edge (always on its horizontal segments) */
  hops: Pt[];
  fromSide: Side; toSide: Side;
  /** true when the A* search was skipped (budget, no path): a plain L/Z route */
  fallback?: boolean;
  /** points before lane nudging (reused for incremental re-routing) */
  raw?: Pt[];
}
export interface RouteOptions {
  margin?: number; stub?: number; portGap?: number; lane?: number;
  /** cost of one turn (in px of detour) */
  bend?: number;
  /** cost of crossing another edge, and of running on top of one (per px + per overlap) */
  cross?: number; overlap?: number;
  /** total time budget (ms); edges beyond it get L/Z routes */
  budgetMs?: number;
  /** only re-route these edges; the rest keep `previous` */
  only?: Set<string>;
  previous?: Map<string, Route>;
  now?: () => number;
}

const DIR: Record<Side, Pt> = { N: [0, -1], S: [0, 1], E: [1, 0], W: [-1, 0] };
const OPP: Record<Side, Side> = { N: "S", S: "N", E: "W", W: "E" };
const DECO_ORDER: Deco[] = ["triangle", "diamond", "odiamond", "arrow", "none"];

interface Port { box: RBox; side: Side; at: Pt; stubEnd: Pt; key: string }

function chooseSides(a: RBox, b: RBox, vertical: boolean): [Side, Side] {
  const gapY = Math.max(b.y - b.h / 2 - (a.y + a.h / 2), a.y - a.h / 2 - (b.y + b.h / 2));
  const gapX = Math.max(b.x - b.w / 2 - (a.x + a.w / 2), a.x - a.w / 2 - (b.x + b.w / 2));
  const useV = vertical ? gapY > -Math.min(a.h, b.h) / 2 || gapX < 0 : gapY >= gapX;
  if (useV) return b.y >= a.y ? ["S", "N"] : ["N", "S"];
  return b.x >= a.x ? ["E", "W"] : ["W", "E"];
}

/** Route every edge. */
export function routeAll(boxes: RBox[], edges: REdge[], zones: RZone[] = [], o: RouteOptions = {}): Map<string, Route> {
  const margin = o.margin ?? 12, stubLen = o.stub ?? 32, portGap = o.portGap ?? 14, bendCost = o.bend ?? 140, crossCost = o.cross ?? 70, overlapPx = o.overlap ?? 5, lane = o.lane ?? 8;
  const now = o.now ?? (() => (typeof performance !== "undefined" ? performance.now() : Date.now()));
  const t0 = now();
  const budget = o.budgetMs ?? 80;
  const byId = new Map(boxes.map((b) => [b.id, b]));
  const out = new Map<string, Route>();
  const live = edges.filter((e) => byId.has(e.from) && byId.has(e.to));

  // ------------------------------------------------------------- ports (shared per box/side/decoration)
  const pairCount = new Map<string, number>();
  const pairIndex = new Map<string, number>();
  for (const e of live) {
    if (e.from === e.to) continue;
    const k = [e.from, e.to].sort().join("|");
    pairIndex.set(e.id, pairCount.get(k) ?? 0);
    pairCount.set(k, (pairCount.get(k) ?? 0) + 1);
  }
  const ends = new Map<string, { side: Side; key: string }[]>();
  const sides = new Map<string, [Side, Side]>();
  const groupKey = (e: REdge, end: 0 | 1, side: Side) => {
    const box = end ? e.to : e.from;
    const deco = (end ? e.toDeco : e.fromDeco) ?? "none";
    const pair = [e.from, e.to].sort().join("|");
    // parallel edges between one pair get their own ports, so they run side by side
    const par = (pairCount.get(pair) ?? 1) > 1 ? `#${pairIndex.get(e.id)}` : "";
    return `${box}|${side}|${deco}${par}`;
  };
  // distance from a box side to the nearest *other* box straight ahead (Infinity: open)
  const clearance = (b: RBox, side: Side, ignore: string) => {
    const d = DIR[side];
    let best = Infinity;
    for (const o2 of boxes) {
      if (o2.id === b.id || o2.id === ignore) continue;
      const ox1 = o2.x - o2.w / 2, ox2 = o2.x + o2.w / 2, oy1 = o2.y - o2.h / 2 - (o2.padTop ?? 0), oy2 = o2.y + o2.h / 2 + (o2.padBottom ?? 0);
      if (d[0] === 0) {
        if (ox2 < b.x - 14 || ox1 > b.x + 14) continue; // only the strip in front of the side centre
        const edge = d[1] < 0 ? b.y - b.h / 2 - (b.padTop ?? 0) : b.y + b.h / 2 + (b.padBottom ?? 0);
        const dist = d[1] < 0 ? edge - oy2 : oy1 - edge;
        if (dist >= -1) best = Math.min(best, Math.max(0, dist));
      } else {
        if (oy2 < b.y - 14 || oy1 > b.y + 14) continue;
        const edge = d[0] < 0 ? b.x - b.w / 2 : b.x + b.w / 2;
        const dist = d[0] < 0 ? edge - ox2 : ox1 - edge;
        if (dist >= -1) best = Math.min(best, Math.max(0, dist));
      }
    }
    return best;
  };
  const need = stubLen + margin + 2;
  const pickSide = (b: RBox, other: RBox, first: Side): Side => {
    if (clearance(b, first, other.id) >= need) return first;
    const horiz = first === "E" || first === "W";
    const alts: Side[] = horiz ? (other.y >= b.y ? ["S", "N"] : ["N", "S"]) : (other.x >= b.x ? ["E", "W"] : ["W", "E"]);
    for (const a of [...alts, OPP[first]]) if (clearance(b, a, other.id) >= need) return a;
    return first;
  };
  // ------------------------------------------------------------- obstacles and candidate lines
  const obs = boxes.map((b) => ({ x1: b.x - b.w / 2 - margin, x2: b.x + b.w / 2 + margin, y1: b.y - b.h / 2 - margin - (b.padTop ?? 0), y2: b.y + b.h / 2 + margin + (b.padBottom ?? 0) }));
  const xsAll = new Set<number>(), ysAll = new Set<number>();
  for (const r of obs) { xsAll.add(r.x1); xsAll.add(r.x2); ysAll.add(r.y1); ysAll.add(r.y2); }
  const mids = (s: Set<number>) => {
    const v = [...s].sort((a, b) => a - b);
    for (let i = 1; i < v.length; i++) if (v[i] - v[i - 1] > 30) s.add((v[i] + v[i - 1]) / 2);
  };
  mids(xsAll); mids(ysAll);
  const baseXs = [...xsAll], baseYs = [...ysAll];

  // occupancy: horizontal segments by y, vertical by x (rounded), with the port groups that own them
  type Occ = { a: number; b: number; groups: string[] };
  const occH = new Map<number, Occ[]>(), occV = new Map<number, Occ[]>();
  // sorted line coordinates, to find perpendicular segments a candidate would cross
  const keysH: number[] = [], keysV: number[] = [];
  const insertKey = (arr: number[], k: number) => { let a = 0, b = arr.length; while (a < b) { const m = (a + b) >> 1; if (arr[m] < k) a = m + 1; else b = m; } if (arr[a] !== k) arr.splice(a, 0, k); };
  const occupy = (pts: Pt[], groups: string[]) => {
    for (let i = 0; i + 1 < pts.length; i++) {
      const [p, q] = [pts[i], pts[i + 1]];
      if (p[1] === q[1]) { const k = Math.round(p[1]); occH.set(k, [...(occH.get(k) ?? []), { a: Math.min(p[0], q[0]), b: Math.max(p[0], q[0]), groups }]); insertKey(keysH, k); }
      else { const k = Math.round(p[0]); occV.set(k, [...(occV.get(k) ?? []), { a: Math.min(p[1], q[1]), b: Math.max(p[1], q[1]), groups }]); insertKey(keysV, k); }
    }
  };
  /** Perpendicular edges a candidate segment crosses (edges sharing a port with it don't count). */
  const crossCount = (horizontal: boolean, c: number, a: number, b: number, groups: string[]) => {
    const keys = horizontal ? keysV : keysH, occ = horizontal ? occV : occH;
    let lo = 0, hi = keys.length;
    while (lo < hi) { const m = (lo + hi) >> 1; if (keys[m] <= a) lo = m + 1; else hi = m; }
    let n = 0;
    for (let i = lo; i < keys.length && keys[i] < b; i++) {
      for (const s of occ.get(keys[i])!) if (c > s.a + 0.5 && c < s.b - 0.5 && !s.groups.some((g) => groups.includes(g))) n++;
    }
    return n;
  };
  const overlapCost = (horizontal: boolean, c: number, a: number, b: number, groups: string[]) => {
    const occ = horizontal ? occH : occV;
    const list: Occ[] = [];
    for (let k = Math.round(c) - 4; k <= Math.round(c) + 4; k++) { const l = occ.get(k); if (l) list.push(...l); }
    if (!list.length) return 0;
    let cost = 0;
    for (const s of list) {
      if (s.groups.some((g) => groups.includes(g))) continue; // shared trunk: free
      const ov = Math.min(b, s.b) - Math.max(a, s.a);
      if (ov > 0) cost += ov * overlapPx + 2 * crossCost;
    }
    return cost;
  };
  const softCost = (e: REdge, p: Pt, q: Pt) => {
    let c = 0;
    const mx = (p[0] + q[0]) / 2, my = (p[1] + q[1]) / 2;
    const len = Math.abs(p[0] - q[0]) + Math.abs(p[1] - q[1]);
    for (const z of zones) {
      const inP = p[0] > z.x1 && p[0] < z.x2 && p[1] > z.y1 && p[1] < z.y2;
      const inQ = q[0] > z.x1 && q[0] < z.x2 && q[1] > z.y1 && q[1] < z.y2;
      if (inP !== inQ) c += 30;
      if (!z.members.has(e.from) && !z.members.has(e.to) && mx > z.x1 && mx < z.x2 && my > z.y1 && my < z.y2) c += len * 3;
      // don't trace a package frame: a line running along its border reads as part of the frame
      if (p[1] === q[1] ? (Math.abs(p[1] - z.y1) < 8 || Math.abs(p[1] - z.y2) < 8) && mx > z.x1 && mx < z.x2
                        : (Math.abs(p[0] - z.x1) < 8 || Math.abs(p[0] - z.x2) < 8) && my > z.y1 && my < z.y2) c += len * 2;
    }
    return c;
  };

  // ------------------------------------------------------------- A* on a grid of candidate lines (through the given ports' stubs)
  const makeSearch = (portList: Iterable<Port>) => {
  const xsAll = new Set(baseXs), ysAll = new Set(baseYs);
  for (const p of portList) { xsAll.add(p.stubEnd[0]); ysAll.add(p.stubEnd[1]); }
  const xsSorted = [...xsAll].sort((a, b) => a - b), ysSorted = [...ysAll].sort((a, b) => a - b);
  // free grid points / free unit segments between neighbouring candidate lines (computed once)
  const NX = xsSorted.length, NY = ysSorted.length;
  const gxi = new Map(xsSorted.map((x, i) => [x, i])), gyi = new Map(ysSorted.map((y, i) => [y, i]));
  const gFree = new Uint8Array(NX * NY), hFree = new Uint8Array(NX * NY), vFree = new Uint8Array(NX * NY);
  {
    // mark cells inside obstacles via ranges (fast: obstacles × their covered lines)
    gFree.fill(1); hFree.fill(1); vFree.fill(1);
    const lo = (arr: number[], v: number) => { let a = 0, b = arr.length; while (a < b) { const m = (a + b) >> 1; if (arr[m] <= v) a = m + 1; else b = m; } return a; }; // first > v
    for (const r of obs) {
      const i0 = lo(xsSorted, r.x1 + 0.01), i1 = lo(xsSorted, r.x2 - 0.01 - 1e-9); // xs strictly inside: [i0, i1)
      const j0 = lo(ysSorted, r.y1 + 0.01), j1 = lo(ysSorted, r.y2 - 0.01 - 1e-9);
      for (let i = i0; i < i1; i++) for (let j = j0; j < j1; j++) gFree[i * NY + j] = 0;
      // horizontal unit segments (i→i+1) on rows strictly inside, overlapping the obstacle's x-range
      const ih0 = Math.max(0, i0 - 1), ih1 = Math.min(NX - 1, i1);
      for (let j = j0; j < j1; j++) for (let i = ih0; i < ih1; i++) if (xsSorted[i] < r.x2 - 0.01 && xsSorted[i + 1] > r.x1 + 0.01) hFree[i * NY + j] = 0;
      const jv0 = Math.max(0, j0 - 1), jv1 = Math.min(NY - 1, j1);
      for (let i = i0; i < i1; i++) for (let j = jv0; j < jv1; j++) if (ysSorted[j] < r.y2 - 0.01 && ysSorted[j + 1] > r.y1 + 0.01) vFree[i * NY + j] = 0;
    }
  }
  const search = (e: REdge, s: Port, t: Port, pad: number): { pts: Pt[]; cost: number } | null => {
    const minX = Math.min(s.stubEnd[0], t.stubEnd[0]) - pad, maxX = Math.max(s.stubEnd[0], t.stubEnd[0]) + pad;
    const minY = Math.min(s.stubEnd[1], t.stubEnd[1]) - pad, maxY = Math.max(s.stubEnd[1], t.stubEnd[1]) + pad;
    let ox = 0; while (ox < NX && xsSorted[ox] < minX) ox++;
    let ex = ox; while (ex < NX && xsSorted[ex] <= maxX) ex++;
    let oy = 0; while (oy < NY && ysSorted[oy] < minY) oy++;
    let ey = oy; while (ey < NY && ysSorted[ey] <= maxY) ey++;
    const nx = ex - ox, ny = ey - oy;
    const xs = xsSorted.slice(ox, ex), ys = ysSorted.slice(oy, ey);
    const si = (gxi.get(s.stubEnd[0]) ?? -1) - ox, sj = (gyi.get(s.stubEnd[1]) ?? -1) - oy, ti = (gxi.get(t.stubEnd[0]) ?? -1) - ox, tj = (gyi.get(t.stubEnd[1]) ?? -1) - oy;
    if (si < 0 || sj < 0 || ti < 0 || tj < 0 || si >= nx || ti >= nx || sj >= ny || tj >= ny) return null;
    const isFree = (i: number, j: number) => (i === si && j === sj) || (i === ti && j === tj) || gFree[(i + ox) * NY + j + oy] === 1;
    const segFree = (i: number, j: number, ni: number, nj: number) => {
      if (j === nj) { const a = Math.min(i, ni); return hFree[(a + ox) * NY + j + oy] === 1 || ((i === si && j === sj) || (i === ti && j === tj) || (ni === si && nj === sj) || (ni === ti && nj === tj)); }
      const b = Math.min(j, nj); return vFree[(i + ox) * NY + b + oy] === 1 || ((i === si && j === sj) || (i === ti && j === tj) || (ni === si && nj === sj) || (ni === ti && nj === tj));
    };
    const groups = [s.key, t.key];
    const D: Pt[] = [[0, -1], [1, 0], [0, 1], [-1, 0]]; // N E S W
    const dirIndex = (side: Side) => ({ N: 0, E: 1, S: 2, W: 3 })[side];
    const startDir = dirIndex(s.side), endDir = dirIndex(OPP[t.side]);
    const N = nx * ny * 4;
    const hCache = new Float64Array(nx * ny).fill(-1), vCache = new Float64Array(nx * ny).fill(-1);
    const g = new Float64Array(N).fill(Infinity);
    const prev = new Int32Array(N).fill(-1);
    // binary heap on typed arrays (f, state)
    let hf = new Float64Array(1024), hs = new Int32Array(1024), hn = 0;
    const push = (f: number, st: number) => {
      if (hn === hf.length) { const f2 = new Float64Array(hn * 2); f2.set(hf); hf = f2; const s2 = new Int32Array(hn * 2); s2.set(hs); hs = s2; }
      let i = hn++;
      while (i > 0) { const p = (i - 1) >> 1; if (hf[p] <= f) break; hf[i] = hf[p]; hs[i] = hs[p]; i = p; }
      hf[i] = f; hs[i] = st;
    };
    let popF = 0;
    const pop = () => {
      const st = hs[0]; popF = hf[0];
      const f = hf[--hn], x = hs[hn];
      let i = 0;
      for (;;) {
        const l = 2 * i + 1;
        if (l >= hn) break;
        const m = l + 1 < hn && hf[l + 1] < hf[l] ? l + 1 : l;
        if (hf[m] >= f) break;
        hf[i] = hf[m]; hs[i] = hs[m]; i = m;
      }
      hf[i] = f; hs[i] = x;
      return st;
    };
    // slightly greedy (×1.25): much faster, routes stay near-optimal
    const h = (i: number, j: number) => 1.25 * (Math.abs(xs[i] - xs[ti]) + Math.abs(ys[j] - ys[tj]) + (i !== ti && j !== tj ? bendCost : 0));
    const s0 = (si * ny + sj) * 4 + startDir;
    g[s0] = 0; push(h(si, sj), s0);
    let goal = -1;
    while (hn) {
      const st = pop(), f = popF;
      const d = st & 3, cell = st >> 2, i = (cell / ny) | 0, j = cell % ny;
      if (f - h(i, j) > g[st] + 1e-6) continue;
      if (i === ti && j === tj) { goal = st; break; }
      for (let nd = 0; nd < 4; nd++) {
        if (nd === ((d + 2) & 3)) continue;
        const ni = i + D[nd][0], nj = j + D[nd][1];
        if (ni < 0 || nj < 0 || ni >= nx || nj >= ny || !isFree(ni, nj) || !segFree(i, j, ni, nj)) continue;
        const p: Pt = [xs[i], ys[j]], q: Pt = [xs[ni], ys[nj]];
        const horiz = nd === 1 || nd === 3;
        // segment cost (length, zones, overlaps, crossings) is direction-free: cache it per unit segment
        const ci = horiz ? Math.min(i, ni) * ny + j : i * ny + Math.min(j, nj);
        const cache = horiz ? hCache : vCache;
        let segCost = cache[ci];
        if (segCost < 0) {
          const lo = horiz ? Math.min(p[0], q[0]) : Math.min(p[1], q[1]), hi = horiz ? Math.max(p[0], q[0]) : Math.max(p[1], q[1]), at = horiz ? p[1] : p[0];
          segCost = hi - lo + softCost(e, p, q) + overlapCost(horiz, at, lo, hi, groups) + crossCost * crossCount(horiz, at, lo, hi, groups);
          cache[ci] = segCost;
        }
        let c = segCost + (nd !== d ? bendCost : 0);
        if (ni === ti && nj === tj && nd !== endDir) c += bendCost * (nd === ((endDir + 2) & 3) ? 3 : 1);
        const ns = (ni * ny + nj) * 4 + nd;
        if (g[st] + c < g[ns] - 1e-9) { g[ns] = g[st] + c; prev[ns] = st; push(g[ns] + h(ni, nj), ns); }
      }
    }
    if (goal < 0) return null;
    const pts: Pt[] = [];
    for (let st = goal; st >= 0; st = prev[st]) { const cell = st >> 2; pts.push([xs[(cell / ny) | 0], ys[cell % ny]]); }
    return { pts: pts.reverse(), cost: g[goal] };
  };
  return search;
  };

  for (const e of live) {
    if (e.from === e.to) continue;
    const a = byId.get(e.from)!, b = byId.get(e.to)!;
    const c = chooseSides(a, b, !!e.vertical);
    const s: [Side, Side] = [pickSide(a, b, c[0]), pickSide(b, a, c[1])];
    sides.set(e.id, s);
  }
  // Diagonal relations: try the other port pairs (opposite sides the other way, and the two L shapes)
  // and keep whichever routes with the fewest turns / cheapest path through the obstacles.
  const portAt = (box: RBox, side: Side, off: number, key: string): Port => {
    const d = DIR[side];
    const at: Pt = side === "N" || side === "S" ? [box.x + off, box.y + (d[1] * box.h) / 2] : [box.x + (d[0] * box.w) / 2, box.y + off];
    const len = side === "N" ? Math.max(stubLen, (box.padTop ?? 0) + margin + 4) : side === "S" ? Math.max(stubLen, (box.padBottom ?? 0) + margin + 4) : stubLen;
    return { box, side, at, stubEnd: [at[0] + d[0] * len, at[1] + d[1] * len], key };
  };
  {
    const cands = new Map<string, [Side, Side][]>();
    for (const e of live) {
      if (e.from === e.to || e.vertical) continue;
      const a = byId.get(e.from)!, b = byId.get(e.to)!;
      const gapY = Math.max(b.y - b.h / 2 - (a.y + a.h / 2), a.y - a.h / 2 - (b.y + b.h / 2));
      const gapX = Math.max(b.x - b.w / 2 - (a.x + a.w / 2), a.x - a.w / 2 - (b.x + b.w / 2));
      if (gapX <= 2 * stubLen || gapY <= 2 * stubLen) continue; // not diagonal: the facing sides are right
      const v: Side = b.y > a.y ? "S" : "N", h: Side = b.x > a.x ? "E" : "W";
      const list: [Side, Side][] = [sides.get(e.id)!, [v, OPP[v]], [h, OPP[h]], [h, OPP[v]], [v, OPP[h]]];
      cands.set(e.id, list.filter((x, i) => list.findIndex((y) => y[0] === x[0] && y[1] === x[1]) === i && clearance(a, x[0], b.id) >= need && clearance(b, x[1], a.id) >= need));
    }
    if (cands.size) {
      const pre = new Map<string, Port>();
      for (const [id, list] of cands) {
        const e = live.find((x) => x.id === id)!;
        for (const [fs, ts] of list) {
          pre.set(`${e.from}|${fs}`, portAt(byId.get(e.from)!, fs, 0, `pre|${e.from}|${fs}`));
          pre.set(`${e.to}|${ts}`, portAt(byId.get(e.to)!, ts, 0, `pre|${e.to}|${ts}`));
        }
      }
      const preSearch = makeSearch(pre.values());
      for (const [id, list] of cands) {
        if (list.length < 2 || now() - t0 > budget / 3) continue;
        const e = live.find((x) => x.id === id)!;
        let best = Infinity, pick = sides.get(id)!;
        for (const [fs, ts] of list) {
          const r = preSearch(e, pre.get(`${e.from}|${fs}`)!, pre.get(`${e.to}|${ts}`)!, 160);
          if (r && r.cost < best - 1) { best = r.cost; pick = [fs, ts]; }
        }
        sides.set(id, pick);
      }
    }
  }
  for (const e of live) {
    if (e.from === e.to) continue;
    const s = sides.get(e.id)!;
    for (const end of [0, 1] as const) {
      const box = end ? e.to : e.from;
      const key = groupKey(e, end, s[end]);
      const list = ends.get(`${box}|${s[end]}`) ?? [];
      if (!list.some((x) => x.key === key)) list.push({ side: s[end], key });
      ends.set(`${box}|${s[end]}`, list);
    }
  }
  const ports = new Map<string, Port>();
  for (const [bs, list] of ends) {
    const box = byId.get(bs.slice(0, bs.lastIndexOf("|")))!;
    list.sort((a, b) => {
      const da = DECO_ORDER.indexOf(a.key.split("|")[2].split("#")[0] as Deco), db = DECO_ORDER.indexOf(b.key.split("|")[2].split("#")[0] as Deco);
      return da - db || (a.key < b.key ? -1 : a.key > b.key ? 1 : 0);
    });
    list.forEach((g, i) => {
      const side = g.side;
      const along = side === "N" || side === "S" ? box.w : box.h;
      const span = Math.max(0, along / 2 - 12);
      const off = Math.max(-span, Math.min(span, (i - (list.length - 1) / 2) * portGap));
      ports.set(g.key, portAt(box, side, off, g.key));
    });
  }

  const search = makeSearch(ports.values());

  // a segment may pass if it stays clear of every box by half the margin
  const blocked = (p: Pt, q: Pt) => {
    const x1 = Math.min(p[0], q[0]), x2 = Math.max(p[0], q[0]), y1 = Math.min(p[1], q[1]), y2 = Math.max(p[1], q[1]);
    const m = margin / 2;
    for (const r of obs) if (x2 > r.x1 + m && x1 < r.x2 - m && y2 > r.y1 + m && y1 < r.y2 - m) return true;
    return false;
  };
  const fallbackRoute = (s: Port, t: Port): Pt[] => {
    const [a, b] = [s.stubEnd, t.stubEnd];
    if (s.side === "N" || s.side === "S") {
      if (t.side === "N" || t.side === "S") { const my = (a[1] + b[1]) / 2; return [a, [a[0], my], [b[0], my], b]; }
      return [a, [a[0], b[1]], b];
    }
    if (t.side === "E" || t.side === "W") { const mx = (a[0] + b[0]) / 2; return [a, [mx, a[1]], [mx, b[1]], b]; }
    return [a, [b[0], a[1]], b];
  };

  // ------------------------------------------------------------- route (short edges first: they get the clean paths)
  const todo = live.filter((e) => !o.only || o.only.has(e.id));
  for (const e of live) {
    if (o.only && !o.only.has(e.id) && o.previous?.has(e.id)) {
      const r = o.previous.get(e.id)!;
      const base = r.raw ?? r.points;
      out.set(e.id, { ...r, points: base.map((p) => [p[0], p[1]] as Pt), hops: [] });
      occupy(base, [groupKey(e, 0, r.fromSide), groupKey(e, 1, r.toSide)]);
    } else if (o.only && !o.only.has(e.id)) todo.push(e);
  }
  const dist = (e: REdge) => { const a = byId.get(e.from)!, b = byId.get(e.to)!; return Math.abs(a.x - b.x) + Math.abs(a.y - b.y); };
  todo.sort((a, b) => dist(a) - dist(b) || (a.id < b.id ? -1 : a.id > b.id ? 1 : 0));
  for (const e of todo) {
    if (e.from === e.to) {
      const b = byId.get(e.from)!;
      const r = b.x + b.w / 2, t = b.y - b.h / 2, L = 24 + (b.padTop ?? 0);
      out.set(e.id, { points: [[r - 22, t], [r - 22, t - L], [r + 24, t - L], [r + 24, t + 18], [r, t + 18]], hops: [], fromSide: "N", toSide: "E" });
      continue;
    }
    const [fs, ts] = sides.get(e.id)!;
    const s = ports.get(groupKey(e, 0, fs))!, t = ports.get(groupKey(e, 1, ts))!;
    // facing neighbours closer than two stubs: a short Z through the gap between them
    const gapH = s.side === "E" && t.side === "W" ? t.at[0] - s.at[0] : s.side === "W" && t.side === "E" ? s.at[0] - t.at[0] : -1;
    const gapV = s.side === "S" && t.side === "N" ? t.at[1] - s.at[1] : s.side === "N" && t.side === "S" ? s.at[1] - t.at[1] : -1;
    if ((gapH >= 0 && gapH < 2 * stubLen) || (gapV >= 0 && gapV < 2 * stubLen)) {
      const pts = gapH >= 0
        ? simplify([s.at, [(s.at[0] + t.at[0]) / 2, s.at[1]], [(s.at[0] + t.at[0]) / 2, t.at[1]], t.at])
        : simplify([s.at, [s.at[0], (s.at[1] + t.at[1]) / 2], [t.at[0], (s.at[1] + t.at[1]) / 2], t.at]);
      out.set(e.id, { points: pts, hops: [], fromSide: fs, toSide: ts });
      occupy(pts, [s.key, t.key]);
      continue;
    }
    let mid: Pt[] | null = null;
    if (now() - t0 < budget) mid = (search(e, s, t, 160) ?? search(e, s, t, 600) ?? search(e, s, t, 1e7))?.pts ?? null;
    const pts = unjog(simplify([s.at, ...(mid ?? fallbackRoute(s, t)), t.at]), blocked);
    out.set(e.id, { points: pts, hops: [], fromSide: fs, toSide: ts, fallback: !mid || undefined });
    occupy(pts, [s.key, t.key]);
  }

  for (const r of out.values()) r.raw = r.points.map((p) => [p[0], p[1]] as Pt);
  nudge(out, live, (e, end) => { const r = out.get(e.id)!; return groupKey(e, end, end ? r.toSide : r.fromSide); }, lane);
  // lane nudging can leave small steps behind: straighten them where the result stays clear
  for (const [id, r] of out) out.set(id, { ...r, points: unjog(simplify(r.points), blocked), raw: r.raw });
  addHops(out, live);
  // diagnostics for perf checks (?perf=1 tooling and tests)
  if (todo.length > 1) (globalThis as { __spideRoute?: unknown }).__spideRoute = { ms: Math.round(now() - t0), edges: todo.length, fallback: [...out.values()].filter((r) => r.fallback).length };
  return out;
}

/** Remove small jogs (two turns around a short step) by sliding a neighbouring segment, when the result stays clear. */
export function unjog(pts: Pt[], blocked: (p: Pt, q: Pt) => boolean, max = 14): Pt[] {
  let p = pts.map((x) => [x[0], x[1]] as Pt);
  for (let guard = 0; guard < 20; guard++) {
    let changed = false;
    for (let i = 1; i + 2 < p.length - 0; i++) {
      const [a, b] = [p[i], p[i + 1]];
      const len = Math.abs(a[0] - b[0]) + Math.abs(a[1] - b[1]);
      if (len === 0 || len > max || i + 2 >= p.length) continue;
      const k = Math.abs(a[0] - b[0]) > 0.01 ? 0 : 1; // axis along which the jog steps
      // option 1: slide the following segment back onto the preceding one's line (not the last, port-bound segment)
      if (i + 3 < p.length) {
        const c: Pt = [...p[i + 2]] as Pt; c[k] = a[k];
        if (!blocked(a, c) && !blocked(c, p[i + 3])) { const q = [...p]; q[i + 1] = a; q[i + 2] = c; p = simplify(q); changed = true; break; }
      }
      // option 2: slide the preceding segment onto the following one's line (not the first, port-bound segment)
      if (i - 2 >= 0) {
        const c: Pt = [...p[i - 1]] as Pt; c[k] = b[k];
        if (!blocked(p[i - 2], c) && !blocked(c, b)) { const q = [...p]; q[i - 1] = c; q[i] = b; p = simplify(q); changed = true; break; }
      }
    }
    if (!changed) break;
  }
  return p;
}

/** Merge collinear points, drop zero-length segments. */
export function simplify(pts: Pt[]): Pt[] {
  const a: Pt[] = [];
  for (const p of pts) {
    const q = a[a.length - 1];
    if (q && Math.abs(q[0] - p[0]) < 0.01 && Math.abs(q[1] - p[1]) < 0.01) continue;
    a.push([p[0], p[1]]);
  }
  for (let i = 1; i + 1 < a.length; ) {
    const [p, q, r] = [a[i - 1], a[i], a[i + 1]];
    const col = (Math.abs(p[0] - q[0]) < 0.01 && Math.abs(q[0] - r[0]) < 0.01) || (Math.abs(p[1] - q[1]) < 0.01 && Math.abs(q[1] - r[1]) < 0.01);
    if (col) a.splice(i, 1); else i++;
  }
  return a;
}

/** Spread collinear overlapping segments of different bundles into lanes (end segments stay on their ports). */
function nudge(routes: Map<string, Route>, edges: REdge[], groupOf: (e: REdge, end: 0 | 1) => string, lane: number) {
  type Seg = { edge: string; i: number; horiz: boolean; c: number; a: number; b: number; bundle: string; order: number };
  const segs: Seg[] = [];
  const groupsOf = new Map<string, string[]>();
  for (const e of edges) if (routes.has(e.id) && e.from !== e.to) groupsOf.set(e.id, [groupOf(e, 0), groupOf(e, 1)]);
  for (const e of edges) {
    const r = routes.get(e.id);
    if (!r || e.from === e.to) continue;
    const p = r.points;
    const ends = [p[0], p[p.length - 1]];
    for (let i = 1; i + 2 < p.length; i++) {
      const [u, v] = [p[i], p[i + 1]];
      const horiz = Math.abs(u[1] - v[1]) < 0.01;
      // order lanes by where the edge goes next, so nudging doesn't create crossings
      const order = horiz ? (ends[0][1] + ends[1][1]) / 2 : (ends[0][0] + ends[1][0]) / 2;
      segs.push({ edge: e.id, i, horiz, c: horiz ? u[1] : u[0], a: Math.min(horiz ? u[0] : u[1], horiz ? v[0] : v[1]), b: Math.max(horiz ? u[0] : u[1], horiz ? v[0] : v[1]), bundle: e.id, order });
    }
  }
  // channels: segments on (nearly) the same line, within 6px, are one channel to split into lanes
  const byLine = new Map<string, Seg[]>();
  for (const horiz of [true, false]) {
    const list = segs.filter((x) => x.horiz === horiz).sort((x, y) => x.c - y.c);
    let ch = 0, last = -Infinity;
    for (const sg of list) { if (sg.c - last > 6) ch++; last = sg.c; const k = `${horiz ? "h" : "v"}${ch}`; byLine.set(k, [...(byLine.get(k) ?? []), sg]); }
  }
  const shifts: { edge: string; i: number; horiz: boolean; d: number }[] = [];
  for (const list of byLine.values()) {
    list.sort((x, y) => x.a - y.a);
    // clusters of overlapping intervals
    let cluster: Seg[] = [];
    let end = -Infinity;
    const flush = () => {
      // within this stretch, edges that share a port travel together (one lane); others get their own lane
      const lead = new Map<string, string>();
      for (const sg of cluster) {
        if (lead.has(sg.edge)) continue;
        const mine = groupsOf.get(sg.edge) ?? [];
        const mate = cluster.find((o2) => lead.has(o2.edge) && (groupsOf.get(o2.edge) ?? []).some((g) => mine.includes(g)));
        lead.set(sg.edge, mate ? lead.get(mate.edge)! : sg.edge);
      }
      for (const sg of cluster) sg.bundle = lead.get(sg.edge)!;
      const bundles = [...new Set(cluster.map((s) => s.bundle))];
      if (bundles.length > 1) {
        const ord = new Map(bundles.map((b) => [b, cluster.filter((s) => s.bundle === b).reduce((t, s) => t + s.order, 0) / cluster.filter((s) => s.bundle === b).length]));
        bundles.sort((x, y) => ord.get(x)! - ord.get(y)! || (x < y ? -1 : 1));
        const step = Math.min(lane, 22 / (bundles.length - 1));
        const centre = cluster.reduce((t, s) => t + s.c, 0) / cluster.length;
        bundles.forEach((b, k) => {
          const target = centre + (k - (bundles.length - 1) / 2) * step;
          for (const s of cluster) if (s.bundle === b) shifts.push({ edge: s.edge, i: s.i, horiz: s.horiz, d: Math.max(-11, Math.min(11, target - s.c)) });
        });
      }
      cluster = [];
    };
    for (const s of list) {
      if (cluster.length && s.a >= end - 0.5) { flush(); end = -Infinity; }
      cluster.push(s); end = Math.max(end, s.b);
    }
    flush();
  }
  for (const sh of shifts) {
    const p = routes.get(sh.edge)!.points;
    const k = sh.horiz ? 1 : 0;
    p[sh.i][k] += sh.d; p[sh.i + 1][k] += sh.d;
  }
}

/** Hops: where a horizontal segment crosses another edge's vertical segment (away from corners). */
function addHops(routes: Map<string, Route>, edges: REdge[]) {
  const H: { edge: string; y: number; a: number; b: number }[] = [];
  const V: { edge: string; x: number; a: number; b: number }[] = [];
  for (const e of edges) {
    const r = routes.get(e.id);
    if (!r) continue;
    const p = r.points;
    for (let i = 0; i + 1 < p.length; i++) {
      const [u, v] = [p[i], p[i + 1]];
      if (Math.abs(u[1] - v[1]) < 0.01) H.push({ edge: e.id, y: u[1], a: Math.min(u[0], v[0]), b: Math.max(u[0], v[0]) });
      else V.push({ edge: e.id, x: u[0], a: Math.min(u[1], v[1]), b: Math.max(u[1], v[1]) });
    }
  }
  const clear = 16;
  for (const h of H) {
    for (const v of V) {
      if (v.edge === h.edge) continue;
      if (v.x > h.a + clear && v.x < h.b - clear && h.y > v.a + 2 && h.y < v.b - 2) routes.get(h.edge)!.hops.push([v.x, h.y]);
    }
  }
  for (const r of routes.values()) {
    r.hops.sort((a, b) => a[0] - b[0] || a[1] - b[1]);
    r.hops = r.hops.filter((q, i, arr) => i === 0 || Math.abs(q[0] - arr[i - 1][0]) > 11 || q[1] !== arr[i - 1][1]);
  }
}

/** SVG path: rounded corners (radius r) and semicircular hops (radius hr) on horizontal segments. */
export function routePath(points: Pt[], hops: Pt[] = [], r = 10, hr = 5): string {
  if (points.length < 2) return "";
  const f = (n: number) => n.toFixed(1);
  const len = (a: Pt, b: Pt) => Math.abs(a[0] - b[0]) + Math.abs(a[1] - b[1]);
  const unit = (a: Pt, b: Pt): Pt => { const l = len(a, b) || 1; return [(b[0] - a[0]) / l, (b[1] - a[1]) / l]; };
  const rad = points.map((_, i) => (i === 0 || i === points.length - 1 ? 0 : Math.min(r, len(points[i - 1], points[i]) / 2, len(points[i], points[i + 1]) / 2)));
  let d = `M${f(points[0][0])},${f(points[0][1])}`;
  for (let i = 0; i + 1 < points.length; i++) {
    const [a, b] = [points[i], points[i + 1]];
    const u = unit(a, b);
    const start: Pt = [a[0] + u[0] * rad[i], a[1] + u[1] * rad[i]];
    const end: Pt = [b[0] - u[0] * rad[i + 1], b[1] - u[1] * rad[i + 1]];
    if (Math.abs(a[1] - b[1]) < 0.01 && hops.length) {
      const dir = Math.sign(b[0] - a[0]);
      const on = hops.filter((h) => Math.abs(h[1] - a[1]) < 0.5 && (h[0] - start[0]) * dir > hr && (end[0] - h[0]) * dir > hr).sort((p, q) => (p[0] - q[0]) * dir);
      for (const h of on) {
        d += ` L${f(h[0] - dir * hr)},${f(a[1])} A${hr},${hr} 0 0 ${dir > 0 ? 1 : 0} ${f(h[0] + dir * hr)},${f(a[1])}`;
      }
    }
    d += ` L${f(end[0])},${f(end[1])}`;
    if (i + 2 < points.length && rad[i + 1] > 0) {
      const c = points[i + 1];
      const u2 = unit(c, points[i + 2]);
      d += ` Q${f(c[0])},${f(c[1])} ${f(c[0] + u2[0] * rad[i + 1])},${f(c[1] + u2[1] * rad[i + 1])}`;
    }
  }
  return d;
}

/** Midpoint of the longest inner segment: where a label sits. */
export function labelPoint(points: Pt[]): Pt {
  let best = 0, at: Pt = points[0];
  const inner = points.length > 3 ? [1, points.length - 2] : [0, points.length - 1];
  for (let i = inner[0]; i < inner[1]; i++) {
    const l = Math.abs(points[i][0] - points[i + 1][0]) + Math.abs(points[i][1] - points[i + 1][1]);
    if (l > best) { best = l; at = [(points[i][0] + points[i + 1][0]) / 2, (points[i][1] + points[i + 1][1]) / 2]; }
  }
  return at;
}
