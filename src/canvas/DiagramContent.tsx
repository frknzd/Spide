import { memo } from "react";
import { perfCount } from "../state/perf";
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

const STRUCTURAL = new Set(["association", "aggregation", "composition"]);
const typeTokens = (t: string) => t.split(/[^A-Za-z0-9_]+/).filter(Boolean);

/** Attributes that only restate a drawn association (exam note: "do not add variables for associations you draw"),
 *  keyed by class, plus the role name each such relation gets on the canvas. */
export function associationRoles(classes: Class[], relations: Relation[]): { attrs: Map<string, Set<string>>; roles: Map<string, string> } {
  const attrs = new Map<string, Set<string>>();
  const roles = new Map<string, string>();
  const byName = new Map(classes.map((c) => [c.name, c]));
  for (const r of relations) {
    if (!STRUCTURAL.has(r.kind) || r.status === "pruned") continue;
    const ends: [string, string][] = [[r.from, r.to]];
    if (r.kind === "association" && !r.directed) ends.push([r.to, r.from]);
    for (const [owner, other] of ends) {
      const a = byName.get(owner)?.attributes.find((x) => typeTokens(x.type).includes(other));
      if (!a) continue;
      attrs.set(owner, (attrs.get(owner) ?? new Set()).add(a.name));
      if (!roles.has(r.id)) roles.set(r.id, a.name);
    }
  }
  return { attrs, roles };
}

/** Attribute names each class inherits from its ancestors (not repeated at the design level). */
function inheritedAttrs(classes: Class[], relations: Relation[]): Map<string, Set<string>> {
  const parents = new Map<string, string[]>();
  for (const r of relations) if (r.kind === "inheritance" && r.status !== "pruned") parents.set(r.from, [...(parents.get(r.from) ?? []), r.to]);
  const byName = new Map(classes.map((c) => [c.name, c]));
  const out = new Map<string, Set<string>>();
  for (const c of classes) {
    const seen = new Set<string>();
    const names = new Set<string>();
    const stack = [...(parents.get(c.name) ?? [])];
    while (stack.length) {
      const p = stack.pop()!;
      if (seen.has(p)) continue;
      seen.add(p);
      for (const a of byName.get(p)?.attributes ?? []) names.add(a.name);
      stack.push(...(parents.get(p) ?? []));
    }
    if (names.size) out.set(c.name, names);
  }
  return out;
}

/** Names of classes that are external themselves or live in an external package. */
export function externalClasses(classes: Class[], packages: Package[]): Set<string> {
  const ext = new Set(packages.filter((p) => p.external).map((p) => p.name));
  return new Set(classes.filter((c) => c.external || (c.package && ext.has(c.package))).map((c) => c.name));
}

export function boxesFor(classes: Class[], showMethods = true, level = 4, relations: Relation[] = [], purposes?: Record<string, string>, external?: Set<string>): Map<string, ClassBox> {
  const m = new Map<string, ClassBox>();
  const assoc = level === 3 ? associationRoles(classes, relations).attrs : new Map<string, Set<string>>();
  const inh = level === 3 ? inheritedAttrs(classes, relations) : new Map<string, Set<string>>();
  for (const c of classes) {
    // external classes: a compact header (they aren't designed here), unless everything is shown
    const lvl = external?.has(c.name) && level < 4 ? 1 : level;
    m.set(c.name, classBox(c, { methods: showMethods, level: lvl, ctx: { inherited: inh.get(c.name), associations: assoc.get(c.name) }, purpose: purposes?.[c.name] }));
  }
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
  const rule = metrics?.edge_rules?.[r.id] ?? "layer ↑";
  if (cyc) return { style: { color: C.cycle, width: 2.4, opacity: 1, strained: true }, warn: lv ? `cycle · ${rule}` : "cycle" };
  if (lv) return { style: { color: C.warn, width: 2, opacity: 1 }, warn: rule };
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
  /** change-scenario stress test: impacted (red) + ripple (amber); everything else dims */
  stress?: { impacted: Set<string>; ripple: Set<string> } | null;
  /** design vs code: missing in code (red dashed), drifted (amber), only in code (phantom) */
  drift?: { missing: Set<string>; drifted: Set<string>; phantom: Set<string> } | null;
  /** pinned classes (badge) */
  pinned?: Set<string>;
  /** package focus: everything outside dims */
  focusSet?: Set<string> | null;
  showPackages?: boolean;
  /** semantic zoom: 0 = packages only, 1 = class names only, ≥ 2 = as the detail level says */
  zoomCap?: number;
  /** granularity (slider) */
  level?: number;
  /** draw every implicit (type/call) dependency, not just the focused class's */
  showImplicit?: boolean;
}

export const DiagramContent = memo(function DiagramContent(p: Props) {
  perfCount("DiagramContent");
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
  const inStress = (n: string) => !!p.stress && (p.stress.impacted.has(n) || p.stress.ripple.has(n));

  const level = p.level ?? 4;
  const cap = Math.min(p.zoomCap ?? 4, level);
  const roles = level === 3 ? associationRoles(p.classes, p.relations).roles : null;
  // zoomed far out: packages become solid boxes; their classes and class edges are hidden
  const overview = cap <= 0;
  const hullOf = new Map<string, Rect>();
  const hiddenInPkg = (n: string) => overview && !!p.classes.find((c) => c.name === n)?.package;

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
      const color = pk?.layer ? LAYER_COLOR[pk.layer] : pk?.external ? "#6b7280" : "#8a93a6";
      const x1 = Math.min(...rs.map((r) => r.x - r.w / 2)) - 28;
      const y1 = Math.min(...rs.map((r) => r.y - r.h / 2)) - 40;
      const x2 = Math.max(...rs.map((r) => r.x + r.w / 2)) + 28;
      const y2 = Math.max(...rs.map((r) => r.y + r.h / 2)) + 34;
      const label = [name, pk?.layer && pk.layer !== name ? pk.layer : null, pk?.external ? "«external»" : null, pk?.system ?? null].filter(Boolean).join("  ·  ");
      hullOf.set(name, { x: (x1 + x2) / 2, y: (y1 + y2) / 2, w: x2 - x1, h: y2 - y1 });
      if (overview) {
        const fs = Math.max(48, Math.min(140, (x2 - x1) / Math.max(5, name.length) * 1.3));
        pkgEls.push(
          <g key={"pkg" + name}>
            <rect x={x1} y={y1} width={x2 - x1} height={y2 - y1} rx={22} fill={color} fillOpacity={0.12} stroke={color} strokeOpacity={0.6} strokeWidth={3}
              strokeDasharray={pk?.status === "proposed" ? "10 8" : undefined} data-target={T({ kind: "package", name })} data-hover={`package:${name}`} />
            <text x={(x1 + x2) / 2} y={(y1 + y2) / 2} textAnchor="middle" fontFamily={FONT_SANS} fontSize={fs} fontWeight={700} fill={color} pointerEvents="none">{name}</text>
            <text x={(x1 + x2) / 2} y={(y1 + y2) / 2 + fs * 0.9} textAnchor="middle" fontFamily={FONT_SANS} fontSize={fs * 0.5} fill={C.dim} pointerEvents="none">
              {rs.length} class{rs.length === 1 ? "" : "es"}{pk?.layer ? ` · ${pk.layer}` : ""}
            </text>
          </g>,
        );
        continue;
      }
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
  if (overview) {
    for (const d of p.metrics?.package_deps ?? []) {
      const a = hullOf.get(d.from);
      const b = hullOf.get(d.to);
      if (!a || !b) continue;
      const color = d.in_cycle ? C.cycle : d.layer_violation ? C.warn : C.edge;
      edgeEls.push(
        <Edge key={`pd${d.from}>${d.to}`} from={a} to={b} kind="dependency" directed label={`${d.count}`}
          style={{ color, width: 2 + Math.min(d.count, 8) * 0.6, opacity: 0.9, strained: d.in_cycle }} warn={d.layer_violation ? "layer ↑" : null} />,
      );
    }
  }
  for (const e of overview ? [] : p.implicit ?? []) {
    // implicit dependencies are clutter unless asked for (/implicit) or the class is in focus
    if (!p.showImplicit && !(focus && (e.from === focus || e.to === focus))) continue;
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
    if (hiddenInPkg(r.from) || hiddenInPkg(r.to)) continue;
    const a = rects.get(r.from);
    const b = rects.get(r.to);
    if (!a || !b) continue;
    const { style, warn } = relationStyle(r, p.metrics);
    if (focus && !(r.from === focus || r.to === focus)) style.opacity *= 0.15;
    if (p.stress && !(inStress(r.from) && inStress(r.to))) style.opacity *= 0.15;
    if (p.focusSet && !(p.focusSet.has(r.from) || p.focusSet.has(r.to))) style.opacity *= 0.15;
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
      <Edge key={r.id} from={a} to={b} kind={r.kind} directed={r.directed} fromMult={r.from_mult} toMult={r.to_mult}
        label={(() => {
          const l = r.label ?? roles?.get(r.id) ?? null;
          return r.interaction && r.interaction !== "sync" ? `«${r.interaction}»${l ? " " + l : ""}` : l;
        })()}
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
          if (!pos || !box || hiddenInPkg(c.name)) return null;
          const pk = c.package ? p.packages.find((x) => x.name === c.package) : undefined;
          return (
            <ClassNode key={c.name} cls={c} box={box} x={pos.x} y={pos.y} scale={nodeScale(cm?.[c.name])} m={cm?.[c.name]}
              dim={(!!focus && !neighbours.has(c.name)) || (!!p.stress && !inStress(c.name)) || (!!p.focusSet && !p.focusSet.has(c.name))}
              pinned={!!p.pinned?.has(c.name)} focus={focus === c.name || p.highlight?.has(c.name)} showMethods={p.showMethods}
              ring={p.stress?.impacted.has(c.name) ? C.cycle : p.stress?.ripple.has(c.name) ? C.warn
                : p.drift?.missing.has(c.name) ? C.cycle : p.drift?.drifted.has(c.name) ? C.warn : null}
              ringDashed={!!p.drift?.missing.has(c.name)} phantom={!!p.drift?.phantom.has(c.name)}
              still={p.still} layerColor={pk?.layer ? LAYER_COLOR[pk.layer] : null} compact={cap <= 1 && level >= 2} />
          );
        })}
      </g>
    </>
  );
});
