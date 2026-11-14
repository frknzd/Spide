import type { ReactElement } from "react";
import type { RelationKind } from "../state/types";
import { borderPoint, C, F, FONT_SANS, textWidth, type Rect } from "./visuals";
import { labelPoint, routePath, type Route } from "./route";

export interface EdgeStyle {
  color: string;
  width: number;
  opacity: number;
  dashed?: boolean;
  /** cycle tension animation */
  strained?: boolean;
  ghost?: boolean;
  removing?: boolean;
}

export interface EdgeProps {
  from: Rect;
  to: Rect;
  kind: RelationKind | "implicit" | "flow";
  directed?: boolean;
  label?: string | null;
  fromMult?: string | null;
  toMult?: string | null;
  /** parallel-edge offset index (0 = straight) */
  bend?: number;
  style: EdgeStyle;
  target?: string;
  hoverKey?: string;
  warn?: string | null;
  /** orthogonal route (points + hops); without it the edge is a straight/bent line */
  route?: Route | null;
  /** model status (export can leave out proposed elements) */
  status?: string;
}

type P = [number, number];
const add = (a: P, b: P): P => [a[0] + b[0], a[1] + b[1]];
const mul = (a: P, k: number): P => [a[0] * k, a[1] * k];
const norm = (a: P): P => {
  const l = Math.hypot(a[0], a[1]) || 1;
  return [a[0] / l, a[1] / l];
};
const perp = (a: P): P => [-a[1], a[0]];
const pt = (p: P) => `${p[0].toFixed(1)},${p[1].toFixed(1)}`;

export function edgeGeometry(from: Rect, to: Rect, bend = 0) {
  const c1: P = [from.x, from.y];
  const c2: P = [to.x, to.y];
  const n = perp(norm([c2[0] - c1[0], c2[1] - c1[1]]));
  const mid: P = [(c1[0] + c2[0]) / 2, (c1[1] + c2[1]) / 2];
  const ctrl: P = add(mid, mul(n, bend * 34));
  const aim1 = bend ? ctrl : c2;
  const aim2 = bend ? ctrl : c1;
  const s = borderPoint(from, aim1[0], aim1[1], 2) as P;
  const e = borderPoint(to, aim2[0], aim2[1], 2) as P;
  const dirS = norm([aim1[0] - s[0], aim1[1] - s[1]]); // leaving start
  const dirE = norm([e[0] - aim2[0], e[1] - aim2[1]]); // arriving at end
  // label point on the curve (t = .5)
  const lp: P = bend ? [0.25 * s[0] + 0.5 * ctrl[0] + 0.25 * e[0], 0.25 * s[1] + 0.5 * ctrl[1] + 0.25 * e[1]] : [(s[0] + e[0]) / 2, (s[1] + e[1]) / 2];
  return { s, e, ctrl, dirS, dirE, lp, bent: bend !== 0 };
}

const unit = (a: P, b: P): P => norm([b[0] - a[0], b[1] - a[1]]);

export function Edge(p: EdgeProps) {
  const pts = p.route?.points;
  const routed = !!pts && pts.length >= 2;
  const geo = routed
    ? { s: pts[0] as P, e: pts[pts.length - 1] as P, ctrl: [0, 0] as P, dirS: unit(pts[0], pts[1]), dirE: unit(pts[pts.length - 2], pts[pts.length - 1]), lp: labelPoint(pts) as P, bent: false }
    : edgeGeometry(p.from, p.to, p.bend ?? 0);
  const { s, e, ctrl, dirS, dirE, lp, bent } = geo;
  const st = p.style;
  const kind = p.kind;
  const color = st.color;
  // shorten the line where a closed head sits
  let start = s;
  let end = e;
  const head: ReactElement[] = [];
  const nE = perp(dirE);
  const nS = perp(dirS);
  if (kind === "inheritance" || kind === "realization") {
    const base = add(e, mul(dirE, -15));
    head.push(<polygon key="h" points={`${pt(e)} ${pt(add(base, mul(nE, 8)))} ${pt(add(base, mul(nE, -8)))}`} fill={C.bg} stroke={color} strokeWidth={1.4} />);
    end = base;
  } else if (kind === "composition" || kind === "aggregation") {
    const m1 = add(s, mul(dirS, 11));
    const tip = add(s, mul(dirS, 22));
    head.push(
      <polygon key="d" points={`${pt(s)} ${pt(add(m1, mul(nS, 6.5)))} ${pt(tip)} ${pt(add(m1, mul(nS, -6.5)))}`}
        fill={kind === "composition" ? color : C.bg} stroke={color} strokeWidth={1.4} />,
    );
    start = tip;
  }
  const openArrow = (kind === "association" && p.directed) || kind === "dependency" || kind === "flow" || (kind === "implicit" && st.width > 1);
  if (openArrow) {
    const b = add(e, mul(dirE, -11));
    head.push(<polyline key="a" points={`${pt(add(b, mul(nE, 6)))} ${pt(e)} ${pt(add(b, mul(nE, -6)))}`} fill="none" stroke={color} strokeWidth={1.4} strokeLinejoin="round" />);
  }
  const d = routed
    ? routePath([start, ...(pts!.slice(1, -1) as P[]), end], p.route!.hops)
    : bent ? `M${pt(start)} Q${pt(ctrl)} ${pt(end)}` : `M${pt(start)} L${pt(end)}`;
  const dashed = st.dashed || kind === "dependency" || kind === "realization" || kind === "implicit";
  const dash = kind === "implicit" ? "2 5" : dashed ? "7 5" : undefined;
  const multLabel = (q: P, dir: P, text: string, side: number) => {
    const at = add(add(q, mul(dir, 16)), mul(perp(dir), 11 * side));
    return (
      <text x={at[0]} y={at[1] + 4} fontFamily={FONT_SANS} fontSize={11} fill={C.dim} textAnchor="middle">{text}</text>
    );
  };
  const lw = p.label ? textWidth(p.label, F.small) + 10 : 0;
  return (
    <g data-status={p.status} opacity={st.opacity} className={st.strained ? "edge-strained" : st.ghost ? "edge-ghost" : undefined}>
      {p.target && (
        <path d={d} stroke="transparent" strokeWidth={14} fill="none" data-target={p.target} data-hover={p.hoverKey} style={{ cursor: "context-menu" }} />
      )}
      {st.strained && <path data-diag d={d} stroke={color} strokeWidth={st.width + 6} fill="none" opacity={0.18} className="edge-glow" pointerEvents="none" />}
      <path d={d} stroke={color} strokeWidth={st.width} fill="none" strokeDasharray={dash} pointerEvents="none" strokeLinecap="round" strokeLinejoin="round" className={st.strained ? "edge-line-strained" : undefined} />
      <g pointerEvents="none">{head}</g>
      {p.fromMult && multLabel(s, dirS, p.fromMult, 1)}
      {p.toMult && multLabel(e, mul(dirE, -1), p.toMult, -1)}
      {(p.label || p.warn) && (
        <g transform={`translate(${lp[0].toFixed(1)},${lp[1].toFixed(1)})`} pointerEvents="none">
          {p.label && (
            <>
              <rect x={-lw / 2} y={-9} width={lw} height={17} rx={4} fill={C.bg} opacity={0.85} />
              <text y={4} textAnchor="middle" fontFamily={FONT_SANS} fontSize={10.5} fill={C.dim} fontStyle="italic">{p.label}</text>
            </>
          )}
          {p.warn && <text data-diag y={p.label ? -13 : 4} textAnchor="middle" fontFamily={FONT_SANS} fontSize={10.5} fontWeight={600} fill={color}>{p.warn}</text>}
        </g>
      )}
      {st.removing && <text x={lp[0]} y={lp[1] + 4} textAnchor="middle" fontFamily={FONT_SANS} fontSize={13} fill={C.removing} pointerEvents="none">×</text>}
    </g>
  );
}
