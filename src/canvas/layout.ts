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
