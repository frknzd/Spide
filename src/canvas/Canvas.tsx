import { forwardRef, useEffect, useImperativeHandle, useMemo, useRef, useState } from "react";
import { api } from "../state/api";
import type { Layer, Pos, Target } from "../state/types";
import { NodeDefs } from "./ClassNode";
import { boxesFor, DiagramContent, type DiagramData } from "./DiagramContent";
import { elkLayout } from "./layout";
import { LivingLayout } from "./physics";
import { nodeScale } from "./visuals";
import { Viewport, type ViewportApi, type WorldRect } from "./Viewport";

export interface CanvasApi {
  fit(): void;
  tidy(): void;
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
}

const LAYER_RANK: Record<Layer, number> = { ui: 0, infrastructure: 0, application: 1, domain: 2 };

export const Canvas = forwardRef<CanvasApi, Props>(function Canvas(p, ref) {
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

  useImperativeHandle(ref, () => ({ fit: () => vp.current?.fit(), tidy, viewport: () => vp.current }));

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

  const focusClass = p.hoverKey?.startsWith("class:") ? p.hoverKey.slice(6) : null;

  return (
    <Viewport
      ref={vp}
      defs={<NodeDefs />}
      empty={p.data.classes.length === 0}
      onPrune={p.onPrune}
      collect={collect}
      onClickTarget={(t) => p.onClickTarget(t)}
      onHover={p.onHover}
      onDragStart={(el, w) => {
        const id = el.closest("[data-node]")?.getAttribute("data-node");
        if (!id || !layout.nodes.has(id)) return false;
        const n = layout.nodes.get(id)!;
        dragging.current = id;
        (dragOffset.current = [n.x! - w[0], n.y! - w[1]]);
        return true;
      }}
      onDragMove={(w) => dragging.current && layout.drag(dragging.current, w[0] + dragOffset.current[0], w[1] + dragOffset.current[1])}
      onDragEnd={() => {
        if (dragging.current) layout.release(dragging.current);
        dragging.current = null;
      }}
    >
      <DiagramContent {...p.data} pos={pos} boxes={boxes} showMethods={p.showMethods} focusClass={focusClass} />
    </Viewport>
  );
});

const dragOffset = { current: [0, 0] as [number, number] };
