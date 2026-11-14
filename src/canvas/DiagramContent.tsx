import { memo } from "react";
import { perfCount } from "../state/perf";
import type { MemberDiagnosis, MethodMark, OverlapLink } from "../state/insights";
import type { ProposalFootprint } from "../state/proposal-focus";
import type { Class, ClassMetrics, ImplicitEdge, Metrics, Package, Pos, Relation, Target } from "../state/types";
import { ClassNode } from "./ClassNode";
import { Edge, type EdgeStyle } from "./Edge";
import type { RBox, REdge, RZone } from "./route";
import { decoFor, useRoutes } from "./useRoutes";
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

export function boxesFor(
  classes: Class[], showMethods = true, level = 4, relations: Relation[] = [], purposes?: Record<string, string>, external?: Set<string>,
  keep?: Map<string, Map<string, unknown>>,
  /** member findings per class (`attribute:x` / `method:x`) and the classes whose hidden findings are revealed */
  members?: Map<string, Map<string, unknown>>, reveal?: Set<string>,
): Map<string, ClassBox> {
  const m = new Map<string, ClassBox>();
  // revealed classes may be drawn at the design level while the view shows less
  let assocCache: Map<string, Set<string>> | null = null, inhCache: Map<string, Set<string>> | null = null;
  const assocAll = () => (assocCache ??= associationRoles(classes, relations).attrs);
  const inhAll = () => (inhCache ??= inheritedAttrs(classes, relations));
  for (const c of classes) {
    // external classes: a compact header (they aren't designed here), unless everything is shown
    const open = !!reveal?.has(c.name);
    const lvl = open ? Math.max(3, level) : external?.has(c.name) && level < 4 ? 1 : level;
    const found = [...(members?.get(c.name)?.keys() ?? [])];
    const of = (k: string) => found.filter((x) => x.startsWith(k + ":")).map((x) => x.slice(k.length + 1));
    const keepM = new Set([...(keep?.get(c.name)?.keys() ?? []), ...(open ? of("method") : [])]);
    const ctx = { inherited: lvl === 3 ? inhAll().get(c.name) : undefined, associations: lvl === 3 ? assocAll().get(c.name) : undefined, keep: keepM.size ? keepM : undefined,
      keepAttrs: open ? new Set(of("attribute")) : undefined, flagged: found.length ? new Set(found) : undefined };
    m.set(c.name, classBox(c, { methods: showMethods, level: lvl, ctx, purpose: purposes?.[c.name] }));
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
  /** export without diagnostics: neutral edges and boxes (no heat, cycle or rule colours) */
  plain?: boolean;
  /** bump to force a full re-route (the layout settled) */
  routeEpoch?: number;
  /** hovered ghost proposal: its changes stand out, where it connects is marked, the rest recedes */
  proposalFocus?: ProposalFootprint | null;
  showPackages?: boolean;
  /** semantic zoom: 0 = packages only, 1 = class names only, ≥ 2 = as the detail level says */
  zoomCap?: number;
  /** granularity (slider) */
  level?: number;
  /** draw every implicit (type/call) dependency, not just the focused class's */
  showImplicit?: boolean;
  /** diagnostics on the canvas */
  marks?: Map<string, Map<string, MethodMark>>;
  lens?: Lens;
  concerns?: Map<string, Map<string, string>>;
  overlap?: OverlapLink[];
  scattered?: Map<string, string[]>;
  members?: Map<string, Map<string, MemberDiagnosis>>;
  revealed?: Set<string>;
}

export type Lens = "none" | "concerns" | "overlap";

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
  const pf = p.proposalFocus ?? null;
  const inProposal = (n: string) => !!pf && (pf.changed.has(n) || pf.anchors.has(n));
  const bendMap = bends(p.relations);
  const inStress = (n: string) => !!p.stress && (p.stress.impacted.has(n) || p.stress.ripple.has(n));

  const level = p.level ?? 4;
  const cap = Math.min(p.zoomCap ?? 4, level);
  const roles = level === 3 ? associationRoles(p.classes, p.relations).roles : null;
  // zoomed far out: packages become solid boxes; their classes and class edges are hidden
  const overview = cap <= 0;
  const hullOf = new Map<string, Rect>();
  const hiddenInPkg = (n: string) => overview && !!p.classes.find((c) => c.name === n)?.package;

  // ------------------------------------------------------------- orthogonal routing
  const rBoxes: RBox[] = [];
  const zones: RZone[] = [];
  const hullBoxes: RBox[] = [];
  {
    const byPkg = new Map<string, { names: Set<string>; rs: Rect[] }>();
    for (const c of p.classes) {
      const r = rects.get(c.name);
      if (!r) continue;
      if (!overview || !c.package) rBoxes.push({ id: c.name, x: r.x, y: r.y, w: r.w, h: r.h, padTop: p.still ? 0 : 22, padBottom: p.still ? 0 : 22 });
      if (c.package) {
        const g = byPkg.get(c.package) ?? { names: new Set<string>(), rs: [] };
        g.names.add(c.name); g.rs.push(r); byPkg.set(c.package, g);
      }
    }
    for (const [name, g] of byPkg) {
      const x1 = Math.min(...g.rs.map((r) => r.x - r.w / 2)) - 28, y1 = Math.min(...g.rs.map((r) => r.y - r.h / 2)) - 40;
      const x2 = Math.max(...g.rs.map((r) => r.x + r.w / 2)) + 28, y2 = Math.max(...g.rs.map((r) => r.y + r.h / 2)) + 34;
      zones.push({ id: name, x1, y1, x2, y2, members: g.names });
      hullBoxes.push({ id: name, x: (x1 + x2) / 2, y: (y1 + y2) / 2, w: x2 - x1, h: y2 - y1 });
    }
  }
  const relEdges: REdge[] = overview ? [] : p.relations.filter((r) => rects.has(r.from) && rects.has(r.to)).map((r) => ({ id: r.id, from: r.from, to: r.to, ...decoFor(r.kind, r.directed) }));
  const routes = useRoutes(rBoxes, relEdges, zones, p.routeEpoch ?? 0);
  // transient edges (hover-implicit, lenses) are routed separately so hovering never re-routes the diagram
  const extraEdges: REdge[] = [];
  if (!overview) {
    for (const e of p.implicit ?? []) if ((p.showImplicit || (focus && (e.from === focus || e.to === focus))) && rects.has(e.from) && rects.has(e.to) && e.from !== e.to) extraEdges.push({ id: `imp${e.from}>${e.to}`, from: e.from, to: e.to, toDeco: e.in_cycle ? "arrow" : "none" });
    if (p.lens === "overlap") for (const l of p.overlap ?? []) if (rects.has(l.from) && rects.has(l.to) && l.from !== l.to) extraEdges.push({ id: `ov${l.from}>${l.to}`, from: l.from, to: l.to });
  }
  const extraRoutes = useRoutes(rBoxes, extraEdges, zones, p.routeEpoch ?? 0);
  const pkgEdges: REdge[] = overview ? (p.metrics?.package_deps ?? []).filter((d) => d.from !== d.to).map((d) => ({ id: `pd${d.from}>${d.to}`, from: d.from, to: d.to, toDeco: "arrow" })) : [];
  const pkgRoutes = useRoutes(overview ? hullBoxes : [], pkgEdges, [], p.routeEpoch ?? 0);

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
          <g data-pkg key={"pkg" + name}>
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
        <g data-pkg key={"pkg" + name} opacity={focus ? 0.4 : pf ? (pf.packages.has(name) ? 1 : 0.25) : 1} style={{ transition: "opacity 180ms" }}>
          <rect x={x1} y={y1} width={x2 - x1} height={y2 - y1} rx={18} fill={color} fillOpacity={pf?.packages.has(name) ? 0.09 : 0.045} stroke={color} strokeOpacity={pf?.packages.has(name) ? 0.8 : 0.35} strokeWidth={pf?.packages.has(name) ? 1.8 : 1}
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
        <Edge key={`pd${d.from}>${d.to}`} from={a} to={b} kind="dependency" directed label={`${d.count}`} route={pkgRoutes.get(`pd${d.from}>${d.to}`)}
          style={{ color, width: 2 + Math.min(d.count, 8) * 0.6, opacity: 0.9, strained: d.in_cycle }} warn={d.layer_violation ? "layer ↑" : null} />,
      );
    }
  }
  // overlap lens: classes that duplicate each other, methods dealing with another class's concept
  if (p.lens === "overlap" && !overview) {
    for (const l of p.overlap ?? []) {
      const a = rects.get(l.from);
      const b = rects.get(l.to);
      if (!a || !b) continue;
      edgeEls.push(
        <Edge key={`ov${l.from}>${l.to}`} from={a} to={b} kind="implicit" bend={-0.4} label={l.label} route={extraRoutes.get(`ov${l.from}>${l.to}`)}
          style={{ color: "#c4b5fd", width: 3 + l.strength * 4, opacity: 0.9, dashed: true }} />,
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
      <Edge key={`imp${e.from}>${e.to}`} from={a} to={b} kind="implicit" bend={0.35} route={extraRoutes.get(`imp${e.from}>${e.to}`)}
        hoverKey={`implicit:${e.from}>${e.to}`} target={undefined}
        style={{ color: e.in_cycle ? C.cycle : "#c8cede", width: e.in_cycle ? 1.4 : 1, opacity: dim ? 0.05 : e.in_cycle ? 0.7 : 0.22, strained: e.in_cycle }} />,
    );
  }
  for (const r of p.relations) {
    if (hiddenInPkg(r.from) || hiddenInPkg(r.to)) continue;
    const a = rects.get(r.from);
    const b = rects.get(r.to);
    if (!a || !b) continue;
    const { style, warn } = relationStyle(r, p.plain ? null : p.metrics);
    if (focus && !(r.from === focus || r.to === focus)) style.opacity *= 0.15;
    if (p.stress && !(inStress(r.from) && inStress(r.to))) style.opacity *= 0.15;
    if (p.focusSet && !(p.focusSet.has(r.from) || p.focusSet.has(r.to))) style.opacity *= 0.15;
    if (pf) {
      if (pf.relations.has(r.id)) { style.opacity = 1; style.width += 1; }
      else style.opacity *= inProposal(r.from) && inProposal(r.to) ? 0.35 : 0.08;
    }
    const tgt: Target = r.status === "proposed" && r.proposal ? { kind: "proposal", id: r.proposal } : { kind: "relation", id: r.id };
    edgeEls.push(
      <Edge key={r.id} from={a} to={b} kind={r.kind} directed={r.directed} fromMult={r.from_mult} toMult={r.to_mult}
        label={(() => {
          const l = r.label ?? roles?.get(r.id) ?? null;
          return r.interaction && r.interaction !== "sync" ? `«${r.interaction}»${l ? " " + l : ""}` : l;
        })()}
        bend={bendMap.get(r.id) ?? 0} route={routes.get(r.id)} status={r.status} style={style} warn={warn} target={T(tgt)} hoverKey={`relation:${r.id}`} />,
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
            <ClassNode key={c.name} cls={c} box={box} x={pos.x} y={pos.y} scale={nodeScale(cm?.[c.name])} m={p.plain ? undefined : cm?.[c.name]}
              dim={(!!focus && !neighbours.has(c.name)) || (!!p.stress && !inStress(c.name)) || (!!p.focusSet && !p.focusSet.has(c.name)) || (!!pf && !inProposal(c.name))}
              pinned={!!p.pinned?.has(c.name)} focus={focus === c.name || p.highlight?.has(c.name) || !!pf?.anchors.has(c.name)} showMethods={p.showMethods}
              ring={pf?.changed.has(c.name) ? C.ghost : p.stress?.impacted.has(c.name) ? C.cycle : p.stress?.ripple.has(c.name) ? C.warn
                : p.drift?.missing.has(c.name) ? C.cycle : p.drift?.drifted.has(c.name) ? C.warn : null}
              ringDashed={!!p.drift?.missing.has(c.name) || !!pf?.changed.has(c.name)} spotlit={!!pf?.changed.has(c.name)} phantom={!!p.drift?.phantom.has(c.name)}
              still={p.still} layerColor={pk?.layer ? LAYER_COLOR[pk.layer] : null} compact={cap <= 1 && level >= 2}
              marks={level >= 3 && !p.plain ? p.marks?.get(c.name) : undefined} concerns={p.lens === "concerns" ? p.concerns?.get(c.name) : undefined}
              chips={p.lens === "overlap" ? p.scattered?.get(c.name) : undefined}
              members={p.plain || cap <= 1 ? undefined : p.members?.get(c.name)} revealed={!!p.revealed?.has(c.name)} />
          );
        })}
      </g>
    </>
  );
});
