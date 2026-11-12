import { forwardRef, useCallback, useEffect, useImperativeHandle, useRef, useState, type ReactNode } from "react";
import type { Target } from "../state/types";
import { C, FONT_SANS } from "./visuals";

export interface Transform { x: number; y: number; k: number }
export interface WorldRect { x1: number; y1: number; x2: number; y2: number }

export interface ViewportApi {
  toWorld(cx: number, cy: number): [number, number];
  fit(animate?: boolean): void;
  /** Centre a world point (in the visible area left of insetRight), optionally at zoom k. */
  zoomTo(x: number, y: number, k?: number): void;
  svg(): SVGSVGElement | null;
  world(): SVGGElement | null;
  transform(): Transform;
}

interface Props {
  children: ReactNode;
  defs?: ReactNode;
  /** right-click (single) or right-drag marquee */
  onPrune?: (targets: Target[], at: { x: number; y: number }) => void;
  /** which targets lie inside a marquee (world coords) */
  collect?: (r: WorldRect) => Target[];
  onClickTarget?: (t: Target, e: React.PointerEvent) => void;
  onHover?: (key: string | null, at: { x: number; y: number }) => void;
  onTransform?: (t: Transform) => void;
  /** element under the pointer started a drag (node drag); return true to claim the pointer */
  onDragStart?: (el: Element, world: [number, number], e: React.PointerEvent) => boolean;
  onDragMove?: (world: [number, number]) => void;
  onDragEnd?: (moved: boolean, e: React.PointerEvent) => void;
  /** double-click on an element (inline edit) */
  onDoubleClickTarget?: (t: Target, e: React.MouseEvent) => void;
  initial?: Transform;
  empty?: boolean;
  /** screen space reserved on the right (e.g. decision panel) */
  insetRight?: number;
}

const parseTarget = (el: Element | null): Target | null => {
  const t = el?.closest("[data-target]")?.getAttribute("data-target");
  if (!t) return null;
  try {
    return JSON.parse(t) as Target;
  } catch {
    return null;
  }
};

export const Viewport = forwardRef<ViewportApi, Props>(function Viewport(p, ref) {
  const svgRef = useRef<SVGSVGElement>(null);
  const worldRef = useRef<SVGGElement>(null);
  const [t, setT] = useState<Transform>(p.initial ?? { x: 0, y: 0, k: 1 });
  const tRef = useRef(t);
  tRef.current = t;
  const [marquee, setMarquee] = useState<WorldRect | null>(null);
  const gesture = useRef<{ kind: "pan" | "right" | "drag" | "none"; sx: number; sy: number; tx: number; ty: number; moved: boolean; target: Element | null }>({
    kind: "none", sx: 0, sy: 0, tx: 0, ty: 0, moved: false, target: null,
  });
  const anim = useRef<number | null>(null);
  const fitted = useRef(false);

  useEffect(() => p.onTransform?.(t), [t]); // eslint-disable-line react-hooks/exhaustive-deps

  const toWorld = useCallback((cx: number, cy: number): [number, number] => {
    const r = svgRef.current!.getBoundingClientRect();
    const tr = tRef.current;
    return [(cx - r.left - tr.x) / tr.k, (cy - r.top - tr.y) / tr.k];
  }, []);

  const animateTo = (to: Transform, ms = 450) => {
    if (anim.current) cancelAnimationFrame(anim.current);
    const from = tRef.current;
    const t0 = performance.now();
    const step = (now: number) => {
      // rAF timestamps can precede t0 (frame start) — clamp, or the easing extrapolates (even to k < 0)
      const u = Math.max(0, Math.min(1, (now - t0) / ms));
      const e = 1 - Math.pow(1 - u, 3);
      setT({ x: from.x + (to.x - from.x) * e, y: from.y + (to.y - from.y) * e, k: from.k + (to.k - from.k) * e });
      if (u < 1) anim.current = requestAnimationFrame(step);
    };
    anim.current = requestAnimationFrame(step);
  };

  const insetRef = useRef(0);
  insetRef.current = p.insetRight ?? 0;
  const fit = useCallback((animate = true) => {
    const w = worldRef.current;
    const s = svgRef.current;
    if (!w || !s) return;
    const b = w.getBBox();
    const full = s.getBoundingClientRect();
    const r = { width: full.width - insetRef.current, height: full.height };
    if (b.width === 0 || r.width === 0) return;
    const margin = 90;
    const bottom = 150; // keep clear of the prompt
    const k = Math.min(1.25, Math.max(0.2, Math.min((r.width - 2 * margin) / b.width, (r.height - margin - bottom) / b.height)));
    const to = { k, x: r.width / 2 - (b.x + b.width / 2) * k, y: (r.height - bottom + margin) / 2 - (b.y + b.height / 2) * k + 10 };
    if (animate) animateTo(to);
    else setT(to);
  }, []);

  const zoomTo = useCallback((x: number, y: number, k?: number) => {
    const s = svgRef.current;
    if (!s) return;
    const full = s.getBoundingClientRect();
    const kk = k ?? Math.max(tRef.current.k, 0.9);
    const bottom = 150; // keep clear of the prompt
    animateTo({ k: kk, x: (full.width - insetRef.current) / 2 - x * kk, y: (full.height - bottom) / 2 - y * kk });
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  useImperativeHandle(ref, () => ({ toWorld, fit, zoomTo, svg: () => svgRef.current, world: () => worldRef.current, transform: () => tRef.current }), [toWorld, fit, zoomTo]);

  // first content → fit
  useEffect(() => {
    if (p.empty) {
      fitted.current = false;
      return;
    }
    if (!fitted.current) {
      const id = setTimeout(() => {
        fitted.current = true;
        fit(false);
      }, 60);
      return () => clearTimeout(id);
    }
  }, [p.empty, fit]);

  const onWheel = (e: React.WheelEvent) => {
    if (anim.current) cancelAnimationFrame(anim.current);
    const tr = tRef.current;
    if (e.ctrlKey || e.metaKey) {
      const r = svgRef.current!.getBoundingClientRect();
      const mx = e.clientX - r.left;
      const my = e.clientY - r.top;
      const k = Math.min(3, Math.max(0.15, tr.k * Math.exp(-e.deltaY * 0.01)));
      setT({ k, x: mx - ((mx - tr.x) * k) / tr.k, y: my - ((my - tr.y) * k) / tr.k });
    } else {
      setT({ ...tr, x: tr.x - e.deltaX, y: tr.y - e.deltaY });
    }
  };

  const onPointerDown = (e: React.PointerEvent) => {
    const el = e.target as Element;
    if (el.closest("[data-ui]")) return;
    const g = gesture.current;
    g.sx = e.clientX;
    g.sy = e.clientY;
    g.tx = tRef.current.x;
    g.ty = tRef.current.y;
    g.moved = false;
    g.target = el;
    if (e.button === 2) {
      g.kind = "right";
    } else if (e.button === 0) {
      if (p.onDragStart && p.onDragStart(el, toWorld(e.clientX, e.clientY), e)) g.kind = "drag";
      else g.kind = "pan";
    } else return;
    (e.currentTarget as Element).setPointerCapture(e.pointerId);
  };

  const onPointerMove = (e: React.PointerEvent) => {
    const g = gesture.current;
    if (g.kind === "none") {
      const key = (e.target as Element).closest("[data-hover]")?.getAttribute("data-hover") ?? null;
      p.onHover?.(key, { x: e.clientX, y: e.clientY });
      return;
    }
    const dx = e.clientX - g.sx;
    const dy = e.clientY - g.sy;
    if (Math.hypot(dx, dy) > 4) g.moved = true;
    if (!g.moved) return;
    p.onHover?.(null, { x: e.clientX, y: e.clientY });
    if (g.kind === "pan") setT({ ...tRef.current, x: g.tx + dx, y: g.ty + dy });
    else if (g.kind === "drag") p.onDragMove?.(toWorld(e.clientX, e.clientY));
    else if (g.kind === "right") {
      const [x1, y1] = toWorld(g.sx, g.sy);
      const [x2, y2] = toWorld(e.clientX, e.clientY);
      setMarquee({ x1: Math.min(x1, x2), y1: Math.min(y1, y2), x2: Math.max(x1, x2), y2: Math.max(y1, y2) });
    }
  };

  const onPointerUp = (e: React.PointerEvent) => {
    const g = gesture.current;
    const kind = g.kind;
    g.kind = "none";
    if (kind === "drag") p.onDragEnd?.(g.moved, e);
    if (kind === "right") {
      if (g.moved && marquee) {
        const targets = p.collect?.(marquee) ?? [];
        setMarquee(null);
        if (targets.length) p.onPrune?.(targets, { x: e.clientX, y: e.clientY });
      } else {
        setMarquee(null);
        const tgt = parseTarget(g.target);
        if (tgt) p.onPrune?.([tgt], { x: e.clientX, y: e.clientY });
      }
    }
    if ((kind === "pan" || kind === "drag") && !g.moved && e.button === 0) {
      const tgt = parseTarget(g.target);
      if (tgt) p.onClickTarget?.(tgt, e);
    }
  };

  return (
    <svg
      ref={svgRef}
      className="viewport"
      onWheel={onWheel}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerLeave={() => gesture.current.kind === "none" && p.onHover?.(null, { x: 0, y: 0 })}
      onContextMenu={(e) => e.preventDefault()}
      onDoubleClick={(e) => {
        const tgt = parseTarget(e.target as Element);
        if (!tgt) return fit();
        p.onDoubleClickTarget?.(tgt, e);
      }}
    >
      <defs>
        <pattern id="dots" width={28} height={28} patternUnits="userSpaceOnUse" patternTransform={`translate(${t.x},${t.y}) scale(${t.k})`}>
          <circle cx={1} cy={1} r={1} fill={C.grid} />
        </pattern>
        {p.defs}
      </defs>
      <rect width="100%" height="100%" fill="url(#dots)" />
      <g ref={worldRef} transform={`translate(${t.x},${t.y}) scale(${t.k})`} data-world>
        {p.children}
      </g>
      {marquee && (
        <g transform={`translate(${t.x},${t.y}) scale(${t.k})`} pointerEvents="none">
          <rect x={marquee.x1} y={marquee.y1} width={marquee.x2 - marquee.x1} height={marquee.y2 - marquee.y1}
            fill="rgba(255,92,108,0.07)" stroke={C.cycle} strokeDasharray="6 4" strokeWidth={1.2 / t.k} rx={6 / t.k} />
          <text x={marquee.x1 + 8 / t.k} y={marquee.y1 - 8 / t.k} fill={C.cycle} fontSize={11 / t.k} fontFamily={FONT_SANS}>✂ prune</text>
        </g>
      )}
    </svg>
  );
});
