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
  /** vertical band: lower ranks above higher ones (ui/infrastructure 0 · application 1 · domain 2) */
  rank: number;
  /** external library/system package: packed compactly beside its clients */
  external?: boolean;
}

/** Package hull padding as drawn by DiagramContent (label on top). */
export const HULL = { left: 28, top: 40, right: 28, bottom: 34 };
const PKG_PREFIX = "pkg::";

/**
 * ELK layout with packages as compound nodes: package hulls can't overlap and classes stay inside
 * their package. Each package is laid out on its own (layered, wrapped towards a 1.6 aspect ratio);
 * the packages are then layered by their aggregated dependencies, in bands by rank (ui and
 * infrastructure on top, application, domain below). Returns class centre positions.
 */
export async function elkCompoundLayout(nodes: LayoutNode[], edges: LayoutEdge[], groups: GroupSpec[]): Promise<Map<string, Pos>> {
  const ids = new Set(nodes.map((n) => n.id));
  const spec = new Map(groups.map((g) => [g.id, g]));
  const groupOf = new Map(nodes.map((n) => [n.id, n.group && spec.has(n.group) ? n.group : null]));
  const pad = 12; // breathing room inside the drawn hull
  const byGroup = new Map<string, LayoutNode[]>();
  const loose: LayoutNode[] = [];
  for (const n of nodes) {
    const g = groupOf.get(n.id);
    if (g) byGroup.set(g, [...(byGroup.get(g) ?? []), n]);
    else loose.push(n);
  }
  // edges: inside a package → that package's layout; across packages → one package-level edge
  const inner = new Map<string, { id: string; sources: string[]; targets: string[] }[]>();
  const outer = new Map<string, { id: string; sources: string[]; targets: string[] }>();
  const parentKey = (n: string) => (groupOf.get(n) ? PKG_PREFIX + groupOf.get(n) : n);
  const rankOf = (key: string) => (key.startsWith(PKG_PREFIX) ? spec.get(key.slice(PKG_PREFIX.length))?.rank ?? 1 : 1);
  const isExt = (key: string) => key.startsWith(PKG_PREFIX) && !!spec.get(key.slice(PKG_PREFIX.length))?.external;
  const extClients = new Map<string, number>();
  edges.forEach((e, i) => {
    if (!ids.has(e.from) || !ids.has(e.to) || e.from === e.to) return;
    const up = e.kind === "inheritance" || e.kind === "realization";
    const [a, b] = up ? [e.to, e.from] : [e.from, e.to];
    const ga = groupOf.get(a);
    if (ga && ga === groupOf.get(b)) {
      inner.set(ga, [...(inner.get(ga) ?? []), { id: `e${i}`, sources: [a], targets: [b] }]);
      return;
    }
    const [pa, pb] = [parentKey(a), parentKey(b)];
    // external packages sit beside their clients; packages in the same band don't push each other down
    if (isExt(pa) || isExt(pb)) {
      const [ext, client] = isExt(pa) ? [pa, pb] : [pb, pa];
      if (!isExt(client)) extClients.set(ext, Math.min(extClients.get(ext) ?? 9, rankOf(client)));
      return;
    }
    if (rankOf(pa) === rankOf(pb) && pa.startsWith(PKG_PREFIX) && pb.startsWith(PKG_PREFIX)) return;
    const k = `${pa}>${pb}`;
    if (!outer.has(k)) outer.set(k, { id: `p${i}`, sources: [pa], targets: [pb] });
  });
  const leaf = (n: LayoutNode) => ({ id: n.id, width: n.w, height: n.h });
  const graph = {
    id: "root",
    layoutOptions: {
      "elk.algorithm": "layered",
      "elk.direction": "DOWN",
      "elk.hierarchyHandling": "SEPARATE_CHILDREN",
      "elk.partitioning.activate": "true",
      "elk.spacing.nodeNode": "70",
      "elk.layered.spacing.nodeNodeBetweenLayers": "90",
      "elk.spacing.componentComponent": "90",
      "elk.layered.nodePlacement.strategy": "BRANDES_KOEPF",
      "elk.layered.cycleBreaking.strategy": "GREEDY",
    },
    children: [
      ...[...byGroup.entries()].map(([g, ns]) => {
        const key = PKG_PREFIX + g;
        const ext = isExt(key);
        return {
          id: key,
          layoutOptions: {
            "elk.padding": `[top=${HULL.top + pad},left=${HULL.left + pad},bottom=${HULL.bottom + pad},right=${HULL.right + pad}]`,
            "elk.partitioning.partition": String(ext ? extClients.get(key) ?? 1 : rankOf(key)),
            ...(ext
              ? { "elk.algorithm": "rectpacking", "elk.aspectRatio": "1.4", "elk.spacing.nodeNode": "16" }
              : {
                  "elk.algorithm": "layered",
                  "elk.direction": "DOWN",
                  "elk.spacing.nodeNode": "48",
                  "elk.layered.spacing.nodeNodeBetweenLayers": "64",
                  "elk.layered.wrapping.strategy": "MULTI_EDGE",
                  "elk.aspectRatio": "1.6",
                  "elk.layered.cycleBreaking.strategy": "GREEDY",
                }),
          },
          children: ns.map(leaf),
          edges: inner.get(g) ?? [],
        };
      }),
      ...loose.map((n) => ({ ...leaf(n), layoutOptions: { "elk.partitioning.partition": "1" } })),
    ],
    edges: [...outer.values()],
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
