import { useEffect, useRef } from "react";
import { NodeDefs } from "../canvas/ClassNode";
import { Edge } from "../canvas/Edge";
import { useRoutes } from "../canvas/useRoutes";
import { C, FONT_SANS, textWidth, type Rect } from "../canvas/visuals";
import { Viewport, type WorldRect } from "../canvas/Viewport";
import type { Actor, Target, UseCase } from "../state/types";
import type { ViewProps } from "./PackageView";

const T = (t: Target) => JSON.stringify(t);
const UC_FONT = `13px ${FONT_SANS}`;

function isSystemActor(a: Actor) {
  return /system|service|gateway|api|server|bank|provider/i.test(a.name) || /external system/i.test(a.description ?? "");
}

export interface UcLayout {
  actors: Map<string, Rect & { side: "left" | "right" }>;
  ucs: Map<string, Rect>;
  boundary: { x: number; y: number; w: number; h: number };
}

export function layoutUseCases(actors: Actor[], ucs: UseCase[]): UcLayout {
  const left = actors.filter((a) => !isSystemActor(a));
  const right = actors.filter(isSystemActor);
  const rowH = 74;
  const order = new Map<string, number>();
  left.forEach((a, i) => order.set(a.name, i));
  right.forEach((a, i) => order.set(a.name, i + 0.5));
  // order use cases by the barycentre of their actors to reduce crossings
  const sorted = [...ucs].sort((a, b) => bary(a) - bary(b));
  function bary(u: UseCase) {
    const v = u.actors.map((a) => order.get(a) ?? 0);
    return v.length ? v.reduce((x, y) => x + y, 0) / v.length : 99;
  }
  // included / extending use cases go into their own column so their arrows don't cross the main ones
  const targets = new Set(ucs.flatMap((u) => [...u.includes, ...u.extends]));
  const extenders = new Set(ucs.filter((u) => u.extends.length > 0).map((u) => u.name));
  const isSecondary = (u: UseCase) => (targets.has(u.name) || extenders.has(u.name)) && u.actors.length === 0;
  const main = sorted.filter((u) => !isSecondary(u));
  const second = sorted.filter(isSecondary);
  const mainCols = main.length > 9 ? 2 : 1;
  const columns: UseCase[][] = [];
  const per = Math.ceil(main.length / mainCols) || 1;
  for (let c = 0; c < mainCols; c++) columns.push(main.slice(c * per, (c + 1) * per));
  if (second.length) columns.push(second);
  const cols = columns.length;
  const widthOf = (u: UseCase) => Math.max(150, textWidth(u.name, UC_FONT) + 56);
  const colW = Math.max(200, ...sorted.map(widthOf)) + 60;
  const perCol = Math.max(...columns.map((c) => c.length), 1);
  const ucMap = new Map<string, Rect>();
  columns.forEach((col, ci) => {
    const offset = ((perCol - col.length) * rowH) / 2;
    col.forEach((u, row) => {
      ucMap.set(u.name, { x: ci * colW + colW / 2 + (row % 2 && cols === 1 ? 24 : 0), y: offset + row * rowH + 70, w: widthOf(u), h: 46 });
    });
  });
  const bw = cols * colW + 40;
  const bh = Math.max(perCol * rowH + 50, 160);
  const boundary = { x: -20, y: 0, w: bw, h: bh };
  const actMap = new Map<string, Rect & { side: "left" | "right" }>();
  const place = (list: Actor[], x: number, side: "left" | "right") => {
    const gap = bh / (list.length + 1);
    list.forEach((a, i) => actMap.set(a.name, { x, y: gap * (i + 1), w: 50, h: 90, side }));
  };
  place(left, -170, "left");
  place(right, bw + 130, "right");
  return { actors: actMap, ucs: ucMap, boundary };
}

export function UseCaseView({ snap, vpRef, onPrune, onHover, onClickTarget, onRegenerate }: ViewProps & { onRegenerate: () => void }) {
  const ucm = snap.view.use_cases;
  const actors = ucm.actors.filter((a) => a.status !== "pruned");
  const ucs = ucm.use_cases.filter((u) => u.status !== "pruned");
  const stale = snap.derived.use_cases_stale;
  const asked = useRef(false);
  useEffect(() => {
    if (!asked.current && ucs.length === 0 && snap.pi_status === "idle" && snap.view.classes.length > 0) {
      asked.current = true;
      onRegenerate();
    }
  }, [ucs.length, snap.pi_status]); // eslint-disable-line react-hooks/exhaustive-deps

  const L = layoutUseCases(actors, ucs);
  const collect = (r: WorldRect): Target[] => {
    const inside = (q: Rect) => q.x >= r.x1 && q.x <= r.x2 && q.y >= r.y1 && q.y <= r.y2;
    return [
      ...actors.filter((a) => L.actors.get(a.name) && inside(L.actors.get(a.name)!)).map((a) => ({ kind: "actor", name: a.name }) as Target),
      ...ucs.filter((u) => L.ucs.get(u.name) && inside(L.ucs.get(u.name)!)).map((u) => ({ kind: "use_case", name: u.name }) as Target),
    ];
  };

  const rBoxes = [
    ...[...L.actors].map(([n, r]) => ({ id: "a:" + n, x: r.x, y: r.y, w: r.w, h: 60 })),
    ...[...L.ucs].map(([n, r]) => ({ id: "u:" + n, x: r.x, y: r.y, w: r.w, h: r.h })),
  ];
  const rEdges = ucs.flatMap((u) => [
    ...u.actors.map((a) => ({ id: `${a}>${u.name}`, from: "a:" + a, to: "u:" + u.name })),
    ...u.includes.map((i) => ({ id: `${u.name}«include»${i}`, from: "u:" + u.name, to: "u:" + i, toDeco: "arrow" as const })),
    ...u.extends.map((i) => ({ id: `${u.name}«extend»${i}`, from: "u:" + u.name, to: "u:" + i, toDeco: "arrow" as const })),
  ]).filter((e) => e.from !== e.to);
  const routes = useRoutes(rBoxes, rEdges);

  return (
    <>
      <Banner show={stale || ucs.length === 0} busy={snap.pi_status === "busy"} empty={ucs.length === 0} onRegenerate={onRegenerate}
        text={ucs.length === 0 ? "No use case diagram yet." : "Out of date with the class model."} />
      <Viewport ref={(v) => void (vpRef.current = v)} defs={<NodeDefs />} empty={ucs.length === 0} onPrune={onPrune} collect={collect} onHover={onHover} onClickTarget={(t) => onClickTarget(t)}>
        {ucs.length > 0 && (
          <g>
            <rect x={L.boundary.x} y={L.boundary.y} width={L.boundary.w} height={L.boundary.h} rx={14} fill="#ffffff" fillOpacity={0.02} stroke={C.border} strokeWidth={1.2} />
            <text x={L.boundary.x + 16} y={L.boundary.y + 24} fontFamily={FONT_SANS} fontSize={13} fontWeight={600} fill={C.dim}>{ucm.system ?? snap.view.name}</text>
          </g>
        )}
        {ucs.flatMap((u) =>
          u.actors.map((a) => {
            const ar = L.actors.get(a);
            const ur = L.ucs.get(u.name);
            if (!ar || !ur) return null;
            return <Edge key={a + ">" + u.name} route={routes.get(`${a}>${u.name}`)} from={{ ...ar, h: 60 }} to={{ ...ur, w: ur.w * 0.95 }} kind="association" style={{ color: C.edge, width: 1.3, opacity: 0.9 }} />;
          }),
        )}
        {ucs.flatMap((u) => [
          ...u.includes.map((i) => [i, "«include»"] as const),
          ...u.extends.map((i) => [i, "«extend»"] as const),
        ].map(([other, label]) => {
          const a = L.ucs.get(u.name);
          const b = L.ucs.get(other);
          if (!a || !b) return null;
          // extend points from the extension to the base
          return <Edge key={u.name + label + other} route={routes.get(u.name + label + other)} from={a} to={b} kind="dependency" label={label} bend={0.6} style={{ color: C.dim, width: 1.2, opacity: 0.9 }} />;
        }))}
        {actors.map((a) => {
          const r = L.actors.get(a.name);
          if (!r) return null;
          return (
            <g key={a.name} transform={`translate(${r.x},${r.y - 30})`}><g className="node-enter" data-target={T({ kind: "actor", name: a.name })} data-hover={`actor:${a.name}`} style={{ cursor: "context-menu" }}>
              <rect x={-40} y={-16} width={80} height={100} fill="transparent" />
              <circle cx={0} cy={0} r={10} fill="none" stroke={C.text} strokeWidth={1.6} />
              <path d="M0,10 v26 M-16,20 h32 M0,36 l-13,20 M0,36 l13,20" stroke={C.text} strokeWidth={1.6} fill="none" />
              <text y={76} textAnchor="middle" fontFamily={FONT_SANS} fontSize={12.5} fill={C.text}>{a.name}</text>
            </g></g>
          );
        })}
        {ucs.map((u) => {
          const r = L.ucs.get(u.name);
          if (!r) return null;
          const realised = u.classes.length > 0;
          return (
            <g key={u.name} transform={`translate(${r.x},${r.y})`}><g className="node-enter" data-target={T({ kind: "use_case", name: u.name })} data-hover={`usecase:${u.name}`} style={{ cursor: "pointer" }}>
              <ellipse rx={r.w / 2} ry={r.h / 2} fill={C.node} stroke={realised ? C.accent : C.warn} strokeWidth={1.4} />
              <text y={4.5} textAnchor="middle" fontFamily={FONT_SANS} fontSize={13} fill={C.text}>{u.name}</text>
              {!realised && <text y={r.h / 2 + 14} textAnchor="middle" fontFamily={FONT_SANS} fontSize={10} fill={C.warn}>no realising class</text>}
            </g></g>
          );
        })}
      </Viewport>
    </>
  );
}

export function Banner({ show, busy, text, onRegenerate, empty }: { show: boolean; busy: boolean; text: string; onRegenerate: () => void; empty?: boolean }) {
  if (!show) return null;
  return (
    <div className={`banner ${empty ? "center" : ""}`} data-ui>
      <span>{text}</span>
      {busy ? <span className="faint"><span className="spinner" /> agent working…</span> : <button onClick={onRegenerate}>↻ {empty ? "generate" : "regenerate"}</button>}
    </div>
  );
}
