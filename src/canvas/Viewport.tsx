import { forwardRef, useCallback, useEffect, useImperativeHandle, useRef, useState, type ReactNode } from "react";
import type { Target } from "../state/types";
import { hoverStore } from "../state/hover";
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
  zoomBy(factor: number): void;
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
  // while panning/zooming, CSS animations pause (see .interacting in styles.css)
  const interactTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const markInteracting = () => {
    hoverStore.clear();
    const s = svgRef.current;
    if (!s) return;
    if (!interactTimer.current) s.classList.add("interacting");
    else clearTimeout(interactTimer.current);
    interactTimer.current = setTimeout(() => {
      interactTimer.current = null;
      svgRef.current?.classList.remove("interacting");
    }, 200);
  };
  const fitted = useRef(false);
  useEffect(() => () => { hoverStore.clear(); if (interactTimer.current) clearTimeout(interactTimer.current); if (anim.current) cancelAnimationFrame(anim.current); }, []);

  useEffect(() => p.onTransform?.(t), [t]); // eslint-disable-line react-hooks/exhaustive-deps

  const toWorld = useCallback((cx: number, cy: number): [number, number] => {
    const r = svgRef.current!.getBoundingClientRect();
    const tr = tRef.current;
    return [(cx - r.left - tr.x) / tr.k, (cy - r.top - tr.y) / tr.k];
  }, []);

  const animateTo = (to: Transform, ms = 450) => {
    if (anim.current) cancelAnimationFrame(anim.current);
    if (window.matchMedia?.("(prefers-reduced-motion: reduce)").matches) { setT(to); return; }
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
    const bottom = 24;
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
    const bottom = 24;
    animateTo({ k: kk, x: (full.width - insetRef.current) / 2 - x * kk, y: (full.height - bottom) / 2 - y * kk });
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  useImperativeHandle(ref, () => ({ toWorld, fit, zoomTo, zoomBy: (f: number) => zoomByRef.current(f), svg: () => svgRef.current, world: () => worldRef.current, transform: () => tRef.current }), [toWorld, fit, zoomTo]);
  const zoomByRef = useRef<(f: number) => void>(() => {});

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

  useEffect(() => {
    let previous: { width: number; height: number } | null = null;
    const observer = new ResizeObserver(([entry]) => {
      const next = entry.contentRect;
      if (previous && next.width > 0 && next.height > 0) {
        hoverStore.clear();
        if (anim.current) cancelAnimationFrame(anim.current);
        const t = tRef.current;
        setT({ ...t, x: t.x + (next.width - previous.width) / 2, y: t.y + (next.height - previous.height) / 2 });
      }
      previous = { width: next.width, height: next.height };
    });
    observer.observe(svgRef.current!);
    return () => observer.disconnect();
  }, []);

  const onWheel = (e: React.WheelEvent) => {
    if (anim.current) cancelAnimationFrame(anim.current);
    markInteracting();
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
    hoverStore.clear();
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
    if (g.kind === "pan") {
      markInteracting();
      setT({ ...tRef.current, x: g.tx + dx, y: g.ty + dy });
    }
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

  // Keyboard: arrows pan, +/- zoom, 0 fits, [ and ] step through elements, Enter activates the inspected element.
  const kbIndex = useRef(-1);
  const zoomBy = (f: number) => {
    const s = svgRef.current; if (!s) return;
    const r = s.getBoundingClientRect(), tr = tRef.current, mx = r.width / 2, my = r.height / 2;
    const k = Math.min(3, Math.max(0.15, tr.k * f));
    animateTo({ k, x: mx - ((mx - tr.x) * k) / tr.k, y: my - ((my - tr.y) * k) / tr.k }, 160);
  };
  zoomByRef.current = zoomBy;
  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    const tr = tRef.current, step = e.shiftKey ? 180 : 60;
    const pan: Record<string, [number, number]> = { ArrowLeft: [step, 0], ArrowRight: [-step, 0], ArrowUp: [0, step], ArrowDown: [0, -step] };
    if (pan[e.key]) { e.preventDefault(); hoverStore.clear(); setT({ ...tr, x: tr.x + pan[e.key][0], y: tr.y + pan[e.key][1] }); return; }
    if (e.key === "+" || e.key === "=") { e.preventDefault(); zoomBy(1.25); return; }
    if (e.key === "-") { e.preventDefault(); zoomBy(0.8); return; }
    if (e.key === "0") { e.preventDefault(); fit(); return; }
    if (e.key === "[" || e.key === "]") {
      const items = [...(worldRef.current?.querySelectorAll("[data-hover][data-target]") ?? [])];
      if (!items.length) return;
      e.preventDefault();
      kbIndex.current = (kbIndex.current + (e.key === "]" ? 1 : -1) + items.length) % items.length;
      const el = items[kbIndex.current], b = el.getBoundingClientRect();
      hoverStore.clear();
      p.onHover?.(el.getAttribute("data-hover"), { x: b.left + Math.min(b.width, 160) / 2, y: b.top + Math.min(b.height, 40) / 2 });
      const label = svgRef.current?.querySelector(".kb-focus");
      label?.classList.remove("kb-focus");
      el.classList.add("kb-focus");
      return;
    }
    if (e.key === "Enter") {
      const el = worldRef.current?.querySelector(".kb-focus");
      const tgt = parseTarget(el ?? null);
      if (tgt) { e.preventDefault(); p.onClickTarget?.(tgt, e as unknown as React.PointerEvent); }
    }
    if (e.key === "Escape") { hoverStore.clear(); worldRef.current?.querySelector(".kb-focus")?.classList.remove("kb-focus"); kbIndex.current = -1; }
  };

  return (
    <svg
      ref={svgRef}
      className="viewport"
      tabIndex={0}
      role="application"
      aria-roledescription="diagram canvas"
      aria-label="Diagram. Arrow keys pan, plus and minus zoom, 0 fits, brackets step through elements, Enter opens the selected element."
      onKeyDown={onKeyDown}
      onBlur={() => worldRef.current?.querySelector(".kb-focus")?.classList.remove("kb-focus")}
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
