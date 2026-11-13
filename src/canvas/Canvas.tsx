import { forwardRef, memo, useEffect, useImperativeHandle, useMemo, useRef, useState } from "react";
import { api } from "../state/api";
import { perfCount } from "../state/perf";
import type { Layer, Pos, Target } from "../state/types";
import { NodeDefs } from "./ClassNode";
import { boxesFor, DiagramContent, rectOf, type DiagramData } from "./DiagramContent";
import { elkLayout } from "./layout";
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
  viewport(): ViewportApi | null;
}

interface Props {
  data: DiagramData;
  viewKey: string;
  saved: Record<string, Pos> | undefined;
  showMethods?: boolean;
  tension: number;
  onPrune: (targets: Target[], at: { x: number; y: number }) => void;
  onClickTarget: (t: Target) => void;
  onHover: (key: string | null, at: { x: number; y: number }) => void;
  hoverKey: string | null;
  insetRight?: number;
  stress?: { impacted: Set<string>; ripple: Set<string> } | null;
  drift?: { missing: Set<string>; drifted: Set<string>; phantom: Set<string> } | null;
  /** a class was dropped onto another package */
  onMoveToPackage?: (cls: string, pkg: string) => void;
  onDoubleClickTarget?: (t: Target, at: { x: number; y: number }) => void;
}

const LAYER_RANK: Record<Layer, number> = { ui: 0, infrastructure: 0, application: 1, domain: 2 };

export const Canvas = memo(forwardRef<CanvasApi, Props>(function Canvas(p, ref) {
  perfCount("Canvas");
  const vp = useRef<ViewportApi>(null);
  const [, setFrame] = useState(0);
  const savedRef = useRef(p.saved);
  savedRef.current = p.saved;
  const layout = useMemo(
    () =>
      new LivingLayout(
        () => setFrame((f) => (f + 1) % 1e9),
        () => {
          const pos = layout.positions();
          if (Object.keys(pos).length) api.setLayout(p.viewKey, pos).catch(() => {});
        },
      ),
    [p.viewKey], // eslint-disable-line react-hooks/exhaustive-deps
  );
  useEffect(() => () => layout.stop(), [layout]);
  const dragging = useRef<string | null>(null);

  const boxes = useMemo(() => boxesFor(p.data.classes, p.showMethods !== false), [p.data.classes, p.showMethods]);
  const cm = p.data.metrics?.classes;

  // sync the simulation with the model
  useEffect(() => {
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
      // a batch of new classes: give them a clean layered start
      elkLayout(specs.map((s) => ({ id: s.id, w: s.w, h: s.h })), links).then((pos) => {
        const known = specs.filter((s) => !fresh.includes(s));
        if (known.length) {
          // keep existing classes where they are: shift the elk layout next to them
          const fx = Math.max(...known.map((s) => layout.nodes.get(s.id)!.x! + s.w / 2));
          const minx = Math.min(...fresh.map((s) => pos.get(s.id)!.x - s.w / 2));
          const only = new Map(fresh.map((s) => [s.id, { x: pos.get(s.id)!.x - minx + fx + 120, y: pos.get(s.id)!.y }]));
          layout.tidy(only);
        } else layout.tidy(pos);
        setTimeout(() => vp.current?.fit(), 1500);
      });
    }
  }, [p.data, boxes, p.tension, layout]); // eslint-disable-line react-hooks/exhaustive-deps

  const tidy = () => {
    const specs = [...layout.nodes.values()].map((n) => ({ id: n.id, w: n.w, h: n.h }));
    elkLayout(specs, p.data.relations.map((r) => ({ from: r.from, to: r.to, kind: r.kind }))).then((pos) => {
      layout.tidy(pos);
      setTimeout(() => vp.current?.fit(), 1500);
    });
  };

  // semantic zoom: far out, draw less (positions and box sizes stay the same)
  const [zoomCap, setZoomCap] = useState(4);
  const onTransform = (t: { k: number }) => {
    const cap = t.k < 0.3 ? 0 : t.k < 0.6 ? 1 : 4;
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
    const k = Math.max(0.2, Math.min(1.2, (b.width - (p.insetRight ?? 0) - 240) / (x2 - x1 || 1), (b.height - 320) / (y2 - y1 || 1)));
    setPinned(null);
    setFocusSet(new Set(members));
    vp.current?.zoomTo((x1 + x2) / 2, (y1 + y2) / 2, k);
    return true;
  };

  useImperativeHandle(ref, () => ({ fit: () => vp.current?.fit(), tidy, focus, focusPackage, viewport: () => vp.current }));

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

  const focusClass = p.hoverKey?.startsWith("class:") ? p.hoverKey.slice(6) : pinned;

  return (
    <Viewport
      ref={vp}
      defs={<NodeDefs />}
      empty={p.data.classes.length === 0}
      insetRight={p.insetRight}
      onPrune={p.onPrune}
      collect={collect}
      onClickTarget={(t, e) => {
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
      onDoubleClickTarget={(t, e) => p.onDoubleClickTarget?.(t, { x: e.clientX, y: e.clientY })}
    >
      <DiagramContent {...p.data} pos={pos} boxes={boxes} showMethods={p.showMethods} focusClass={focusClass} stress={p.stress} drift={p.drift}
        pinned={layout.pinnedIds()} focusSet={focusSet} zoomCap={zoomCap} />
    </Viewport>
  );
}));

const dragOffset = { current: [0, 0] as [number, number] };
