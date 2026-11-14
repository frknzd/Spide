import { forwardRef, memo, useEffect, useImperativeHandle, useMemo, useRef, useState, useSyncExternalStore } from "react";
import { api } from "../state/api";
import { perfCount } from "../state/perf";
import { hoverStore, useHoveredClass } from "../state/hover";
import { footprintOf, proposalAt, useGhostSpotlight, type ProposalFootprint } from "../state/proposal-focus";
import type { Layer, Pos, Target } from "../state/types";
import { NodeDefs } from "./ClassNode";
import { boxesFor, DiagramContent, externalClasses, rectOf, type DiagramData, type Lens } from "./DiagramContent";
import { memberDiagnostics, methodConcerns, methodMarks, overlapLinks, scatteredByClass } from "../state/insights";
import { elkCompoundLayout, type GroupSpec } from "./layout";
import { LivingLayout } from "./physics";
import { nodeScale } from "./visuals";
import { Viewport, type ViewportApi, type WorldRect } from "./Viewport";

export interface CanvasApi {
  fit(): void;
  tidy(): void;
  /** Zoom to a class and pin focus mode on it; null clears. Returns false if it isn't on the canvas. */
  focus(name: string | null): boolean;
  /** Zoom to a package and dim everything else; false if it has no classes on the canvas. */
  focusPackage(name: string): boolean;
  /** Export mode: full detail regardless of zoom, optionally without diagnostics colouring. null = normal. */
  setExportMode(mode: { plain: boolean } | null): void;
  /** Zoom to everything one ghost proposal changes or connects to. */
  locateProposal(id: string): boolean;
  viewport(): ViewportApi | null;
}

interface Props {
  data: DiagramData;
  viewKey: string;
  saved: Record<string, Pos> | undefined;
  showMethods?: boolean;
  /** granularity 0–4 (slider) */
  level?: number;
  showImplicit?: boolean;
  lens?: Lens;
  tension: number;
  onPrune: (targets: Target[], at: { x: number; y: number }) => void;
  onClickTarget: (t: Target) => void;
  onHover: (key: string | null, at: { x: number; y: number }) => void;
  insetRight?: number;
  readOnly?: boolean;
  stress?: { impacted: Set<string>; ripple: Set<string> } | null;
  drift?: { missing: Set<string>; drifted: Set<string>; phantom: Set<string> } | null;
  /** a class was dropped onto another package */
  onMoveToPackage?: (cls: string, pkg: string) => void;
  onDoubleClickTarget?: (t: Target, at: { x: number; y: number }) => void;
}

const LAYER_RANK: Record<Layer, number> = { ui: 0, infrastructure: 0, application: 1, domain: 2 };

/** Vertical band of a package: ui and infrastructure (outer ring) on top, application, then domain. */
export function packageRank(pk: { layer: Layer | null } | undefined): number {
  return pk?.layer ? LAYER_RANK[pk.layer] : 1;
}

export const Canvas = memo(forwardRef<CanvasApi, Props>(function Canvas(p, ref) {
  perfCount("Canvas");
  const vp = useRef<ViewportApi>(null);
  const readOnly = useRef(!!p.readOnly);
  readOnly.current = !!p.readOnly;
  const alive = useRef(true);
  const layoutRequest = useRef(0);
  const fitTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const [, setFrame] = useState(0);
  // full orthogonal re-route once the layout has settled (while moving, only touched edges re-route)
  const [routeEpoch, setRouteEpoch] = useState(0);
  const settledSig = useRef("");
  const savedRef = useRef(p.saved);
  savedRef.current = p.saved;
  const layout = useMemo(
    () =>
      new LivingLayout(
        () => setFrame((f) => (f + 1) % 1e9),
        () => {
          const pos = layout.positions();
          // re-route fully only if something moved since the last full route (an idle settle costs no render)
          const sig = JSON.stringify(pos);
          if (sig !== settledSig.current) { settledSig.current = sig; setRouteEpoch((n) => n + 1); }
          if (!readOnly.current && alive.current && Object.keys(pos).length) api.setLayout(p.viewKey, pos).catch(() => {});
        },
      ),
    [p.viewKey], // eslint-disable-line react-hooks/exhaustive-deps
  );
  useEffect(() => {
    alive.current = true;
    return () => { alive.current = false; layoutRequest.current++; clearTimeout(fitTimer.current); layout.stop(); };
  }, [layout]);
  const dragging = useRef<string | null>(null);

  const level = p.level ?? 4;
  const insights = useMemo(() => {
    const m = p.data.metrics;
    return { marks: methodMarks(m), concerns: methodConcerns(m), overlap: overlapLinks(m), scattered: scatteredByClass(m), members: memberDiagnostics(m, p.data) };
  }, [p.data.metrics, p.data.classes]); // eslint-disable-line react-hooks/exhaustive-deps
  // classes whose hidden member findings are shown (local to this canvas; the detail level is unchanged)
  const [exportMode, setExportMode] = useState<{ plain: boolean } | null>(null);
  const [revealed, setRevealed] = useState<Set<string>>(() => new Set());
  const shownReveal = exportMode ? EMPTY : revealed;
  const boxMembers = exportMode?.plain ? undefined : insights.members;
  const external = useMemo(() => externalClasses(p.data.classes, p.data.packages), [p.data.classes, p.data.packages]);
  const boxes = useMemo(
    () => boxesFor(p.data.classes, p.showMethods !== false, Math.max(1, level), p.data.relations, undefined, external, exportMode?.plain ? undefined : insights.marks, boxMembers, shownReveal),
    [p.data.classes, p.data.relations, p.showMethods, level, external, insights.marks, boxMembers, shownReveal, exportMode?.plain],
  );
  const cm = p.data.metrics?.classes;

  // sync the simulation with the model
  useEffect(() => {
    layoutRequest.current++;
    const pk = new Map(p.data.packages.map((x) => [x.name, x]));
    const specs = p.data.classes.map((c) => {
      const b = boxes.get(c.name)!;
      const s = nodeScale(cm?.[c.name]);
      const layer = c.package ? pk.get(c.package)?.layer ?? null : null;
      return {
        id: c.name,
        w: b.w * s,
        h: b.h * s + 22, // room for badges
        pkg: c.package,
        layer: layer ? LAYER_RANK[layer] : null,
        heat: cm?.[c.name]?.heat ?? 0,
        init: savedRef.current?.[c.name],
      };
    });
    const fresh = specs.filter((s) => !layout.nodes.has(s.id) && !s.init);
    const links = p.data.relations.map((r) => ({ from: r.from, to: r.to, kind: r.kind }));
    layout.update(specs, links, p.tension);
    if (fresh.length >= 3) {
      // a batch of new classes: lay everything out again (packages as compound nodes)
      relayout(specs);
    }
  }, [p.data, boxes, p.tension, layout]); // eslint-disable-line react-hooks/exhaustive-deps

  const relayout = (specs?: { id: string; w: number; h: number; pkg: string | null }[]) => {
    const ss = specs ?? [...layout.nodes.values()].map((n) => ({ id: n.id, w: n.w, h: n.h, pkg: n.pkg }));
    const groups: GroupSpec[] = p.data.packages.map((x) => ({ id: x.name, rank: packageRank(x), external: !!x.external }));
    const links = p.data.relations.map((r) => ({ from: r.from, to: r.to, kind: r.kind }));
    const request = ++layoutRequest.current;
    elkCompoundLayout(ss.map((s) => ({ id: s.id, w: s.w, h: s.h, group: s.pkg })), links, groups)
      .then((pos) => {
        if (!alive.current || request !== layoutRequest.current) return;
        layout.tidy(pos);
        clearTimeout(fitTimer.current);
        fitTimer.current = setTimeout(() => vp.current?.fit(), 1500);
      })
      .catch((e) => console.error("layout failed", e));
  };
  const tidy = () => relayout();
  // a different granularity changes every box: lay out again
  const lastLevel = useRef(level);
  useEffect(() => {
    if (lastLevel.current === level) return;
    lastLevel.current = level;
    relayout();
  }, [level]); // eslint-disable-line react-hooks/exhaustive-deps

  // semantic zoom: far out, draw less (positions and box sizes stay the same)
  const [zoomCap, setZoomCap] = useState(4);
  const onTransform = (t: { k: number }) => {
    const cap = t.k < 0.22 ? 0 : t.k < 0.42 ? 1 : 4;
    if (cap !== zoomCap) setZoomCap(cap);
  };

  const [pinned, setPinned] = useState<string | null>(null);
  const [focusSet, setFocusSet] = useState<Set<string> | null>(null);
  const focus = (name: string | null) => {
    setFocusSet(null);
    if (!name) {
      setPinned(null);
      return true;
    }
    const n = layout.nodes.get(name);
    if (!n || n.x == null || n.y == null) return false;
    setPinned(name);
    vp.current?.zoomTo(n.x, n.y);
    return true;
  };
  // a pinned class that disappears (pruned, renamed) unpins itself
  useEffect(() => {
    if (pinned && !p.data.classes.some((c) => c.name === pinned)) setPinned(null);
  }, [p.data.classes, pinned]);

  /** Zoom so a proposal's changed classes and the classes it connects to are all in view. */
  const locate = (fp: ProposalFootprint) => {
    const names = [...fp.changed, ...fp.anchors];
    const rs = names.map((n) => rectOf(n, pos, boxes, cm)).filter((r) => r !== null);
    const svg = vp.current?.svg();
    if (!rs.length || !svg) return false;
    const x1 = Math.min(...rs.map((r) => r.x - r.w / 2)), y1 = Math.min(...rs.map((r) => r.y - r.h / 2));
    const x2 = Math.max(...rs.map((r) => r.x + r.w / 2)), y2 = Math.max(...rs.map((r) => r.y + r.h / 2));
    const b = svg.getBoundingClientRect();
    const k = Math.max(0.25, Math.min(1.1, (b.width - (p.insetRight ?? 0) - 160) / (x2 - x1 || 1), (b.height - 160) / (y2 - y1 || 1)));
    vp.current?.zoomTo((x1 + x2) / 2, (y1 + y2) / 2, k);
    return true;
  };
  const locateProposal = (id: string) => {
    const fp = footprintOf(p.data, new Set([id]));
    return !!fp && locate(fp);
  };

  const focusPackage = (pkg: string) => {
    const members = p.data.classes.filter((c) => c.package === pkg).map((c) => c.name);
    const rs = members.map((n) => rectOf(n, pos, boxes, cm)).filter((r) => r !== null);
    const svg = vp.current?.svg();
    if (!rs.length || !svg) return false;
    const x1 = Math.min(...rs.map((r) => r.x - r.w / 2));
    const y1 = Math.min(...rs.map((r) => r.y - r.h / 2));
    const x2 = Math.max(...rs.map((r) => r.x + r.w / 2));
    const y2 = Math.max(...rs.map((r) => r.y + r.h / 2));
    const b = svg.getBoundingClientRect();
    const k = Math.max(0.2, Math.min(1.2, (b.width - (p.insetRight ?? 0) - 80) / (x2 - x1 || 1), (b.height - 80) / (y2 - y1 || 1)));
    setPinned(null);
    setFocusSet(new Set(members));
    vp.current?.zoomTo((x1 + x2) / 2, (y1 + y2) / 2, k);
    return true;
  };

  useImperativeHandle(ref, () => ({ fit: () => vp.current?.fit(), tidy, focus, focusPackage, locateProposal, setExportMode, viewport: () => vp.current }));

  const pos = new Map<string, Pos>();
  for (const n of layout.nodes.values()) pos.set(n.id, { x: n.x!, y: n.y! });

  const collect = (r: WorldRect): Target[] => {
    const out: Target[] = [];
    for (const c of p.data.classes) {
      const q = pos.get(c.name);
      if (q && q.x >= r.x1 && q.x <= r.x2 && q.y >= r.y1 && q.y <= r.y2) {
        out.push(c.status === "proposed" && c.proposal ? { kind: "proposal", id: c.proposal } : { kind: "class", name: c.name });
      }
    }
    // relations whose midpoint is inside and whose ends aren't both selected
    for (const rel of p.data.relations) {
      const a = pos.get(rel.from);
      const b = pos.get(rel.to);
      if (!a || !b) continue;
      const mx = (a.x + b.x) / 2;
      const my = (a.y + b.y) / 2;
      const inside = (q: Pos) => q.x >= r.x1 && q.x <= r.x2 && q.y >= r.y1 && q.y <= r.y2;
      if (mx >= r.x1 && mx <= r.x2 && my >= r.y1 && my <= r.y2 && !(inside(a) || inside(b))) out.push({ kind: "relation", id: rel.id });
    }
    // dedupe proposals
    const seen = new Set<string>();
    return out.filter((t) => {
      const k = JSON.stringify(t);
      if (seen.has(k)) return false;
      seen.add(k);
      return true;
    });
  };

  /** Package box (as drawn) under a dragged class's centre, other than its own. */
  const packageAt = (id: string): string | null => {
    const me = p.data.classes.find((c) => c.name === id);
    const at = pos.get(id);
    if (!me || !at) return null;
    for (const pk of p.data.packages) {
      if (pk.name === me.package || pk.status === "pruned") continue;
      const rs = p.data.classes.filter((c) => c.package === pk.name && c.name !== id).map((c) => rectOf(c.name, pos, boxes, cm)).filter((r) => r !== null);
      if (!rs.length) continue;
      const x1 = Math.min(...rs.map((r) => r.x - r.w / 2)) - 28;
      const y1 = Math.min(...rs.map((r) => r.y - r.h / 2)) - 40;
      const x2 = Math.max(...rs.map((r) => r.x + r.w / 2)) + 28;
      const y2 = Math.max(...rs.map((r) => r.y + r.h / 2)) + 34;
      if (at.x >= x1 && at.x <= x2 && at.y >= y1 && at.y <= y2) return pk.name;
    }
    return null;
  };

  const hoveredClass = useHoveredClass();
  // Hovering a ghost spotlights its whole proposal: what changes, what it plugs into, which package it lives in.
  // Derived primitive: re-renders only when the hovered *proposal* changes, not on every hovered element.
  const hoveredProposal = useSyncExternalStore(hoverStore.subscribe, () => proposalAt(hoverStore.snapshot()?.key ?? null, p.data));
  const spotlight = useGhostSpotlight();
  const proposalFocus = useMemo(() => {
    if (p.readOnly) return null;
    const ids = hoveredProposal ? [hoveredProposal]
      : spotlight === "all" ? [...new Set([...p.data.classes.flatMap((c) => [c.proposal, ...c.attributes.map((a) => a.proposal), ...c.methods.map((m) => m.proposal)]), ...p.data.relations.map((r) => r.proposal)])].filter((x): x is string => !!x)
      : spotlight ? [spotlight] : [];
    return ids.length ? footprintOf(p.data, new Set(ids)) : null;
  }, [hoveredProposal, spotlight, p.data, p.readOnly]);
  const focusClass = proposalFocus ? null : hoveredClass ?? pinned;

  return (
    <Viewport
      ref={vp}
      defs={<NodeDefs />}
      empty={p.data.classes.length === 0}
      insetRight={p.insetRight}
      onPrune={p.readOnly || !p.onPrune ? undefined : (ts, at) => { const real = ts.filter((t) => (t as { kind: string }).kind !== "reveal"); if (real.length) p.onPrune!(real, at); }}
      collect={collect}
      onClickTarget={(t, e) => {
        const rv = t as unknown as { kind: string; class?: string };
        if (rv.kind === "reveal" && rv.class) {
          const name = rv.class;
          setRevealed((s) => { const n = new Set(s); if (n.has(name)) n.delete(name); else n.add(name); return n; });
          return;
        }
        if (p.readOnly) return;
        // ⌥-click pins / unpins a class where it is
        if (e.altKey && t.kind === "class" && layout.nodes.has(t.name)) {
          layout.togglePin(t.name);
          api.setLayout(p.viewKey, layout.positions()).catch(() => {});
          setFrame((f) => (f + 1) % 1e9);
          return;
        }
        p.onClickTarget(t);
      }}
      onHover={p.onHover}
      onTransform={onTransform}
      onDragStart={(el, w) => {
        if (p.readOnly) return false;
        const id = el.closest("[data-node]")?.getAttribute("data-node");
        if (!id || !layout.nodes.has(id)) return false;
        const n = layout.nodes.get(id)!;
        dragging.current = id;
        (dragOffset.current = [n.x! - w[0], n.y! - w[1]]);
        return true;
      }}
      onDragMove={(w) => dragging.current && layout.drag(dragging.current, w[0] + dragOffset.current[0], w[1] + dragOffset.current[1])}
      onDragEnd={(moved) => {
        const id = dragging.current;
        if (id) layout.release(id);
        dragging.current = null;
        if (id && moved && p.onMoveToPackage) {
          const target = packageAt(id);
          if (target) p.onMoveToPackage(id, target);
        }
      }}
      onDoubleClickTarget={p.readOnly ? undefined : (t, e) => p.onDoubleClickTarget?.(t, { x: e.clientX, y: e.clientY })}
    >
      <DiagramContent {...p.data} pos={pos} boxes={boxes} showMethods={p.showMethods} focusClass={focusClass} proposalFocus={exportMode ? null : proposalFocus} routeEpoch={routeEpoch} plain={!!exportMode?.plain} stress={p.stress} drift={p.drift}
        pinned={exportMode ? undefined : layout.pinnedIds()} focusSet={exportMode ? null : focusSet} zoomCap={exportMode ? 4 : zoomCap} level={level} showImplicit={p.showImplicit}
        lens={p.lens ?? "none"} marks={insights.marks} concerns={insights.concerns} overlap={insights.overlap} scattered={insights.scattered}
        members={insights.members} revealed={shownReveal} />
    </Viewport>
  );
}));

const EMPTY = new Set<string>();
const dragOffset = { current: [0, 0] as [number, number] };
