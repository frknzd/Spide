// ELK layered layout for initial placement and the "tidy" action.
import ELK from "elkjs/lib/elk.bundled.js";
import type { Pos } from "../state/types";

const elk = new ELK();

export interface LayoutNode { id: string; w: number; h: number; group?: string | null }
export interface LayoutEdge { from: string; to: string; kind: string }

/** Returns centre positions. Parents (inheritance) end up above children; clients above suppliers. */
export async function elkLayout(nodes: LayoutNode[], edges: LayoutEdge[], direction: "DOWN" | "RIGHT" = "DOWN"): Promise<Map<string, Pos>> {
  const ids = new Set(nodes.map((n) => n.id));
  const graph = {
    id: "root",
    layoutOptions: {
      "elk.algorithm": "layered",
      "elk.direction": direction,
      "elk.spacing.nodeNode": "70",
      "elk.layered.spacing.nodeNodeBetweenLayers": "110",
      "elk.spacing.componentComponent": "120",
      "elk.layered.nodePlacement.strategy": "BRANDES_KOEPF",
      "elk.layered.cycleBreaking.strategy": "GREEDY",
      "elk.separateConnectedComponents": "true",
    },
    children: nodes.map((n) => ({ id: n.id, width: n.w, height: n.h })),
    edges: edges
      .filter((e) => ids.has(e.from) && ids.has(e.to) && e.from !== e.to)
      .map((e, i) => {
        const up = e.kind === "inheritance" || e.kind === "realization";
        return { id: `e${i}`, sources: [up ? e.to : e.from], targets: [up ? e.from : e.to] };
      }),
  };
  const out = await elk.layout(graph);
  const res = new Map<string, Pos>();
  let cx = 0, cy = 0;
  const kids = out.children ?? [];
  for (const c of kids) {
    cx += (c.x ?? 0) + (c.width ?? 0) / 2;
    cy += (c.y ?? 0) + (c.height ?? 0) / 2;
  }
  cx /= kids.length || 1;
  cy /= kids.length || 1;
  for (const c of kids) res.set(c.id, { x: (c.x ?? 0) + (c.width ?? 0) / 2 - cx, y: (c.y ?? 0) + (c.height ?? 0) / 2 - cy });
  return res;
}

export interface GroupSpec {
  id: string;
  /** vertical band: lower ranks above higher ones (ui 0 · application/infrastructure 1 · domain 2 · external 3) */
  rank: number;
}

/** Package hull padding as drawn by DiagramContent (label on top). */
export const HULL = { left: 28, top: 40, right: 28, bottom: 34 };
const PKG_PREFIX = "pkg::";

/**
 * Layered ELK layout with packages as compound nodes: package hulls can't overlap and classes stay
 * inside their package. Packages are ordered top → bottom by rank. Returns class centre positions.
 */
export async function elkCompoundLayout(nodes: LayoutNode[], edges: LayoutEdge[], groups: GroupSpec[]): Promise<Map<string, Pos>> {
  const ids = new Set(nodes.map((n) => n.id));
  const rank = new Map(groups.map((g) => [g.id, g.rank]));
  const groupOf = new Map(nodes.map((n) => [n.id, n.group && rank.has(n.group) ? n.group : null]));
  const pad = 12; // breathing room inside the drawn hull
  const byGroup = new Map<string, LayoutNode[]>();
  const loose: LayoutNode[] = [];
  for (const n of nodes) {
    const g = groupOf.get(n.id);
    if (g) byGroup.set(g, [...(byGroup.get(g) ?? []), n]);
    else loose.push(n);
  }
  const leaf = (n: LayoutNode) => ({ id: n.id, width: n.w, height: n.h });
  const elkEdges = edges
    .filter((e) => ids.has(e.from) && ids.has(e.to) && e.from !== e.to)
    .map((e, i) => {
      const up = e.kind === "inheritance" || e.kind === "realization";
      return { id: `e${i}`, sources: [up ? e.to : e.from], targets: [up ? e.from : e.to] };
    });
  const graph = {
    id: "root",
    layoutOptions: {
      "elk.algorithm": "layered",
      "elk.direction": "DOWN",
      "elk.hierarchyHandling": "INCLUDE_CHILDREN",
      "elk.partitioning.activate": "true",
      "elk.spacing.nodeNode": "60",
      "elk.layered.spacing.nodeNodeBetweenLayers": "80",
      "elk.spacing.componentComponent": "90",
      "elk.layered.nodePlacement.strategy": "BRANDES_KOEPF",
      "elk.layered.cycleBreaking.strategy": "GREEDY",
      "elk.layered.considerModelOrder.strategy": "NODES_AND_EDGES",
      "elk.separateConnectedComponents": "false",
    },
    children: [
      ...[...byGroup.entries()].map(([g, ns]) => ({
        id: PKG_PREFIX + g,
        layoutOptions: {
          "elk.padding": `[top=${HULL.top + pad},left=${HULL.left + pad},bottom=${HULL.bottom + pad},right=${HULL.right + pad}]`,
          "elk.partitioning.partition": String(rank.get(g) ?? 1),
          "elk.spacing.nodeNode": "48",
          "elk.layered.spacing.nodeNodeBetweenLayers": "64",
        },
        children: ns.map(leaf),
      })),
      ...loose.map((n) => ({ ...leaf(n), layoutOptions: { "elk.partitioning.partition": "1" } })),
    ],
    edges: elkEdges,
  };
  const out = await elk.layout(graph as Parameters<typeof elk.layout>[0]);
  const res = new Map<string, Pos>();
  const walk = (n: { id: string; x?: number; y?: number; width?: number; height?: number; children?: unknown[] }, ox: number, oy: number) => {
    const x = ox + (n.x ?? 0);
    const y = oy + (n.y ?? 0);
    if (n.id.startsWith(PKG_PREFIX) || n.id === "root") {
      for (const c of (n.children ?? []) as typeof n[]) walk(c, n.id === "root" ? 0 : x, n.id === "root" ? 0 : y);
    } else if (ids.has(n.id)) res.set(n.id, { x: x + (n.width ?? 0) / 2, y: y + (n.height ?? 0) / 2 });
  };
  walk(out as Parameters<typeof walk>[0], 0, 0);
  // centre on the origin
  const ps = [...res.values()];
  const cx = ps.reduce((a, p) => a + p.x, 0) / (ps.length || 1);
  const cy = ps.reduce((a, p) => a + p.y, 0) / (ps.length || 1);
  for (const [k, p] of res) res.set(k, { x: p.x - cx, y: p.y - cy });
  return res;
}
