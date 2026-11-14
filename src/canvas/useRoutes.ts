import { useRef } from "react";
import { routeAll, type RBox, type REdge, type Route, type RZone } from "./route";

/**
 * Orthogonal routes for a diagram, recomputed only when needed:
 * - structure changed (edges, box sizes) or `epoch` bumped (layout settled) → full routing;
 * - only positions changed (drag, physics) → re-route edges touching moved boxes, keep the rest;
 * - nothing changed → the previous routes.
 */
export function useRoutes(boxes: RBox[], edges: REdge[], zones: RZone[] = [], epoch = 0): Map<string, Route> {
  const prev = useRef<{ struct: string; epoch: number; pos: Map<string, string>; routes: Map<string, Route> } | null>(null);
  const struct = edges.map((e) => `${e.id}:${e.from}>${e.to}:${e.fromDeco ?? ""}${e.toDeco ?? ""}${e.vertical ? "v" : ""}`).join(",") + "|" +
    boxes.map((b) => `${b.id}:${b.w.toFixed(1)}x${b.h.toFixed(1)}:${b.padTop ?? 0}:${b.padBottom ?? 0}`).join(",");
  const pos = new Map(boxes.map((b) => [b.id, `${b.x.toFixed(1)},${b.y.toFixed(1)}`]));
  const p = prev.current;
  if (p && p.struct === struct && p.epoch === epoch) {
    const moved = new Set<string>();
    for (const [id, k] of pos) if (p.pos.get(id) !== k) moved.add(id);
    if (!moved.size) return p.routes;
    const only = new Set(edges.filter((e) => moved.has(e.from) || moved.has(e.to)).map((e) => e.id));
    const routes = routeAll(boxes, edges, zones, { only, previous: p.routes, budgetMs: 10 });
    prev.current = { struct, epoch, pos, routes };
    return routes;
  }
  const routes = routeAll(boxes, edges, zones, { budgetMs: 90 });
  prev.current = { struct, epoch, pos, routes };
  return routes;
}

/** Router decorations for a relation kind (where its arrowhead / diamond sits). */
export function decoFor(kind: string, directed = false): Pick<REdge, "fromDeco" | "toDeco" | "vertical"> {
  switch (kind) {
    case "inheritance": case "realization": return { toDeco: "triangle", vertical: true };
    case "composition": return { fromDeco: "diamond" };
    case "aggregation": return { fromDeco: "odiamond" };
    case "dependency": case "flow": return { toDeco: "arrow" };
    case "association": return directed ? { toDeco: "arrow" } : {};
    default: return {};
  }
}
