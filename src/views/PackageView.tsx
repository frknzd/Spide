import { useEffect, useMemo, useState, type MutableRefObject } from "react";
import { Chip, NodeDefs } from "../canvas/ClassNode";
import { Edge } from "../canvas/Edge";
import { elkLayout } from "../canvas/layout";
import { C, F, FONT_MONO, FONT_SANS, healthColor, LAYER_COLOR, textWidth } from "../canvas/visuals";
import { Viewport, type ViewportApi, type WorldRect } from "../canvas/Viewport";
import type { Pos, Snapshot, Target } from "../state/types";

export interface ViewProps {
  snap: Snapshot;
  vpRef: MutableRefObject<ViewportApi | null>;
  onPrune: (t: Target[], at: { x: number; y: number }) => void;
  onHover: (key: string | null, at: { x: number; y: number }) => void;
  onClickTarget: (t: Target) => void;
}

const T = (t: Target) => JSON.stringify(t);

export function PackageView({ snap, vpRef, onPrune, onHover, onClickTarget }: ViewProps) {
  const pkgs = snap.derived.packages.packages;
  const pm = snap.metrics.packages;
  const deps = snap.metrics.package_deps;
  const boxes = useMemo(() => {
    const m = new Map<string, { w: number; h: number }>();
    for (const p of pkgs) {
      const label = p.layer && p.layer !== p.name ? `${p.name}  ·  ${p.layer}` : p.name;
      let w = textWidth(label, F.name) + 40;
      for (const c of p.classes) w = Math.max(w, textWidth(c, F.member) + 36);
      m.set(p.name, { w: Math.max(170, Math.ceil(w)), h: 52 + p.classes.length * 17 + 14 });
    }
    return m;
  }, [pkgs]);
  const [pos, setPos] = useState<Map<string, Pos>>(new Map());
  const key = JSON.stringify([...boxes.entries()]) + JSON.stringify(deps.map((d) => d.from + d.to));
  useEffect(() => {
    elkLayout(pkgs.map((p) => ({ id: p.name, w: boxes.get(p.name)!.w, h: boxes.get(p.name)!.h + 22 })), deps.map((d) => ({ from: d.from, to: d.to, kind: "dependency" })))
      .then(setPos);
  }, [key]); // eslint-disable-line react-hooks/exhaustive-deps

  const collect = (r: WorldRect): Target[] =>
    pkgs.filter((p) => {
      const q = pos.get(p.name);
      return q && q.x >= r.x1 && q.x <= r.x2 && q.y >= r.y1 && q.y <= r.y2;
    }).map((p) => ({ kind: "package", name: p.name }) as Target);

  const rect = (n: string) => {
    const p = pos.get(n);
    const b = boxes.get(n);
    return p && b ? { x: p.x, y: p.y, w: b.w, h: b.h } : null;
  };

  return (
    <Viewport ref={(v) => void (vpRef.current = v)} defs={<NodeDefs />} empty={pos.size === 0} onPrune={onPrune} collect={collect} onHover={onHover} onClickTarget={(t) => onClickTarget(t)}>
      {deps.map((d) => {
        const a = rect(d.from);
        const b = rect(d.to);
        if (!a || !b) return null;
        const color = d.in_cycle ? C.cycle : d.layer_violation ? C.warn : C.edge;
        return (
          <Edge key={d.from + ">" + d.to} from={a} to={b} kind="dependency" label={`${d.count}`} warn={d.layer_violation ? "layer ↑" : d.in_cycle ? "cycle" : null}
            bend={deps.some((x) => x.from === d.to && x.to === d.from) ? 1 : 0}
            style={{ color, width: d.in_cycle ? 2.4 : 1.5, opacity: 1, strained: d.in_cycle }} />
        );
      })}
      {pkgs.map((p) => {
        const r = rect(p.name);
        if (!r) return null;
        const m = pm.find((x) => x.name === p.name);
        const color = p.layer ? LAYER_COLOR[p.layer] : "#8a93a6";
        const x = r.x - r.w / 2;
        const y = r.y - r.h / 2;
        const tabW = Math.min(r.w * 0.55, textWidth(p.name, F.name) + 28);
        const label = p.layer && p.layer !== p.name ? `${p.name}  ·  ${p.layer}` : p.name;
        const ghost = p.status === "proposed";
        return (
          <g key={p.name} transform={`translate(${x},${y})`} opacity={ghost ? 0.6 : 1}><g className="node-enter">
            {m?.in_cycle && <rect x={-4} y={-18} width={r.w + 8} height={r.h + 22} rx={10} fill={C.cycle} opacity={0.35} filter="url(#halo3)" />}
            <path d={`M0,0 h${tabW} v14 h${r.w - tabW} v${r.h} h${-r.w} z`} transform="translate(0,-14)" fill={C.node} stroke={color} strokeWidth={1.4}
              strokeDasharray={ghost ? "6 4" : undefined} data-target={T({ kind: "package", name: p.name })} data-hover={`package:${p.name}`} />
            <path d={`M0,${-14} h${tabW} v14 h${-tabW} z`} fill={color} opacity={0.18} pointerEvents="none" />
            <text x={14} y={20} fontFamily={FONT_SANS} fontSize={14} fontWeight={600} fill={C.text} pointerEvents="none">{label}</text>
            <line x1={0} x2={r.w} y1={32} y2={32} stroke={color} opacity={0.4} />
            {p.classes.map((c, i) => (
              <text key={c} x={16} y={50 + i * 17} fontFamily={FONT_MONO} fontSize={12} fill={C.dim} pointerEvents="none">▫ {c}</text>
            ))}
            {m && (
              <g transform={`translate(0,${r.h + 6})`}>
                <Chip x={0} y={0} text={`I ${m.instability.toFixed(2)}`} color={C.dim} />
                <Chip x={56} y={0} text={`A ${m.abstractness.toFixed(2)}`} color={C.dim} />
                <Chip x={112} y={0} text={`D ${m.distance.toFixed(2)}`} color={healthColor(100 - m.distance * 100)} />
              </g>
            )}
          </g></g>
        );
      })}
    </Viewport>
  );
}
