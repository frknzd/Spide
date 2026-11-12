import type { Class, ClassMetrics, ImplicitEdge, Metrics, Package, Pos, Relation, Target } from "../state/types";
import { ClassNode } from "./ClassNode";
import { Edge, type EdgeStyle } from "./Edge";
import { C, classBox, FONT_SANS, LAYER_COLOR, nodeScale, type ClassBox, type Rect } from "./visuals";

export interface DiagramData {
  classes: Class[];
  relations: Relation[];
  packages: Package[];
  metrics?: Metrics | null;
  implicit?: ImplicitEdge[];
}

export function boxesFor(classes: Class[], showMethods = true): Map<string, ClassBox> {
  const m = new Map<string, ClassBox>();
  for (const c of classes) m.set(c.name, classBox(c, { methods: showMethods }));
  return m;
}

export function rectOf(name: string, pos: Map<string, Pos>, boxes: Map<string, ClassBox>, metrics?: Record<string, ClassMetrics>): Rect | null {
  const p = pos.get(name);
  const b = boxes.get(name);
  if (!p || !b) return null;
  const s = nodeScale(metrics?.[name]);
  return { x: p.x, y: p.y, w: b.w * s, h: b.h * s };
}

/** Assign bend offsets to parallel edges between the same pair. */
export function bends<T extends { from: string; to: string; id: string }>(edges: T[]): Map<string, number> {
  const groups = new Map<string, T[]>();
  for (const e of edges) {
    const k = [e.from, e.to].sort().join("|");
    groups.set(k, [...(groups.get(k) ?? []), e]);
  }
  const out = new Map<string, number>();
  for (const g of groups.values()) {
    g.forEach((e, i) => {
      const b = g.length === 1 ? 0 : i - (g.length - 1) / 2;
      // keep orientation consistent for reversed edges
      out.set(e.id, e.from < e.to ? b : -b);
    });
  }
  return out;
}

const T = (t: Target) => JSON.stringify(t);

export function relationStyle(r: Relation, metrics?: Metrics | null): { style: EdgeStyle; warn: string | null } {
  if (r.status === "proposed") return { style: { color: C.ghost, width: 1.6, opacity: 0.85, dashed: true, ghost: true }, warn: null };
  if (r.status === "removing") return { style: { color: C.removing, width: 1.4, opacity: 0.6, dashed: true, removing: true }, warn: null };
  if (r.status === "pruned") return { style: { color: C.faint, width: 1, opacity: 0.3 }, warn: null };
  const cyc = metrics?.cycle_edges.includes(r.id);
  const lv = metrics?.layer_violations.includes(r.id);
  if (cyc) return { style: { color: C.cycle, width: 2.4, opacity: 1, strained: true }, warn: lv ? "cycle · layer" : "cycle" };
  if (lv) return { style: { color: C.warn, width: 2, opacity: 1 }, warn: "layer ↑" };
  if (r.status === "overruled") return { style: { color: C.warn, width: 1.5, opacity: 0.9 }, warn: null };
  return { style: { color: C.edge, width: 1.4, opacity: 0.95 }, warn: null };
}

interface Props extends DiagramData {
  pos: Map<string, Pos>;
  boxes: Map<string, ClassBox>;
  showMethods?: boolean;
  still?: boolean;
  /** hovered class → focus mode */
  focusClass?: string | null;
  /** classes to highlight (e.g. preview focus) */
  highlight?: Set<string>;
  showPackages?: boolean;
}

export function DiagramContent(p: Props) {
  const cm = p.metrics?.classes;
  const rects = new Map<string, Rect>();
  for (const c of p.classes) {
    const r = rectOf(c.name, p.pos, p.boxes, cm);
    if (r) rects.set(c.name, r);
  }
  const focus = p.focusClass && rects.has(p.focusClass) ? p.focusClass : null;
  const neighbours = new Set<string>();
  if (focus) {
    neighbours.add(focus);
    for (const r of p.relations) {
      if (r.from === focus) neighbours.add(r.to);
      if (r.to === focus) neighbours.add(r.from);
    }
    for (const e of p.implicit ?? []) {
      if (e.from === focus) neighbours.add(e.to);
      if (e.to === focus) neighbours.add(e.from);
    }
  }
  const bendMap = bends(p.relations);

  // packages
  const pkgEls = [];
  if (p.showPackages !== false) {
    const byPkg = new Map<string, Rect[]>();
    for (const c of p.classes) {
      const r = rects.get(c.name);
      if (r && c.package) byPkg.set(c.package, [...(byPkg.get(c.package) ?? []), r]);
    }
    for (const [name, rs] of byPkg) {
      const pk = p.packages.find((x) => x.name === name);
      const color = pk?.layer ? LAYER_COLOR[pk.layer] : "#8a93a6";
      const x1 = Math.min(...rs.map((r) => r.x - r.w / 2)) - 28;
      const y1 = Math.min(...rs.map((r) => r.y - r.h / 2)) - 40;
      const x2 = Math.max(...rs.map((r) => r.x + r.w / 2)) + 28;
      const y2 = Math.max(...rs.map((r) => r.y + r.h / 2)) + 34;
      const label = pk?.layer && pk.layer !== name ? `${name}  ·  ${pk.layer}` : name;
      pkgEls.push(
        <g key={"pkg" + name} opacity={focus ? 0.4 : 1}>
          <rect x={x1} y={y1} width={x2 - x1} height={y2 - y1} rx={18} fill={color} fillOpacity={0.045} stroke={color} strokeOpacity={0.35}
            strokeDasharray={pk?.status === "proposed" ? "6 5" : undefined} data-target={T({ kind: "package", name })} data-hover={`package:${name}`} />
          <text x={x1 + 16} y={y1 + 20} fontFamily={FONT_SANS} fontSize={12} fontWeight={600} fill={color} opacity={0.85} pointerEvents="none">{label}</text>
        </g>,
      );
    }
  }

  const edgeEls = [];
  for (const e of p.implicit ?? []) {
    const a = rects.get(e.from);
    const b = rects.get(e.to);
    if (!a || !b) continue;
    const dim = focus && !(e.from === focus || e.to === focus);
    edgeEls.push(
      <Edge key={`imp${e.from}>${e.to}`} from={a} to={b} kind="implicit" bend={0.35}
        hoverKey={`implicit:${e.from}>${e.to}`} target={undefined}
        style={{ color: e.in_cycle ? C.cycle : "#c8cede", width: e.in_cycle ? 1.4 : 1, opacity: dim ? 0.05 : e.in_cycle ? 0.7 : 0.22, strained: e.in_cycle }} />,
    );
  }
  for (const r of p.relations) {
    const a = rects.get(r.from);
    const b = rects.get(r.to);
    if (!a || !b) continue;
    const { style, warn } = relationStyle(r, p.metrics);
    if (focus && !(r.from === focus || r.to === focus)) style.opacity *= 0.15;
    const tgt: Target = r.status === "proposed" && r.proposal ? { kind: "proposal", id: r.proposal } : { kind: "relation", id: r.id };
    if (r.from === r.to) {
      const x = a.x + a.w / 2;
      const y = a.y - a.h / 2;
      edgeEls.push(
        <g key={r.id} opacity={style.opacity}>
          <path d={`M${x - 30},${y} C${x - 30},${y - 50} ${x + 40},${y - 50} ${x + 40},${y + 0.1} L${x},${y + 30}`} fill="none" stroke={style.color} strokeWidth={style.width}
            data-target={T(tgt)} data-hover={`relation:${r.id}`} />
        </g>,
      );
      continue;
    }
    edgeEls.push(
      <Edge key={r.id} from={a} to={b} kind={r.kind} directed={r.directed} label={r.label} fromMult={r.from_mult} toMult={r.to_mult}
        bend={bendMap.get(r.id) ?? 0} style={style} warn={warn} target={T(tgt)} hoverKey={`relation:${r.id}`} />,
    );
  }

  return (
    <>
      <g data-layer="packages">{pkgEls}</g>
      <g data-layer="edges">{edgeEls}</g>
      <g data-layer="nodes">
        {p.classes.map((c) => {
          const pos = p.pos.get(c.name);
          const box = p.boxes.get(c.name);
          if (!pos || !box) return null;
          const pk = c.package ? p.packages.find((x) => x.name === c.package) : undefined;
          return (
            <ClassNode key={c.name} cls={c} box={box} x={pos.x} y={pos.y} scale={nodeScale(cm?.[c.name])} m={cm?.[c.name]}
              dim={!!focus && !neighbours.has(c.name)} focus={focus === c.name || p.highlight?.has(c.name)} showMethods={p.showMethods}
              still={p.still} layerColor={pk?.layer ? LAYER_COLOR[pk.layer] : null} />
          );
        })}
      </g>
    </>
  );
}
