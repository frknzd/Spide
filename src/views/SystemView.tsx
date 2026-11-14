// C4 levels above classes: Context (actors → system → external systems) and Container.
import { useEffect, useMemo, useState } from "react";
import { Chip, NodeDefs } from "../canvas/ClassNode";
import { Edge } from "../canvas/Edge";
import { useRoutes } from "../canvas/useRoutes";
import { elkLayout } from "../canvas/layout";
import { C, F, FONT_SANS, textWidth } from "../canvas/visuals";
import { Viewport, type WorldRect } from "../canvas/Viewport";
import type { Pos, Target } from "../state/types";
import type { ViewProps } from "./PackageView";

const T = (t: Target) => JSON.stringify(t);
type Kind = "person" | "system" | "external" | "container" | "datastore";
interface NodeSpec { id: string; kind: Kind; title: string; sub: string | null; lines: string[]; ghost: boolean; spof?: boolean; target?: Target }
interface EdgeSpec { from: string; to: string; label: string | null; dashed: boolean; warn: string | null }

const COLOR: Record<Kind, string> = { person: "#c084fc", system: "#7aa2ff", external: "#8a93a6", container: "#60a5fa", datastore: "#34d399" };

export function SystemView({ snap, vpRef, onPrune, onHover, onClickTarget, level, onLevel }: ViewProps & { level: "context" | "container"; onLevel: (l: "context" | "container") => void }) {
  const sv = snap.derived.system;
  const issues = snap.metrics.issues;
  const { nodes, edges } = useMemo(() => {
    const nodes: NodeSpec[] = [];
    const edges: EdgeSpec[] = [];
    for (const a of sv.actors) nodes.push({ id: a, kind: "person", title: a, sub: "person", lines: [], ghost: false, target: { kind: "actor", name: a } });
    for (const x of sv.systems)
      nodes.push({ id: x.name, kind: "external", title: x.name, sub: "external system", lines: x.description ? [x.description] : [], ghost: x.status === "proposed", target: { kind: "system", name: x.name } });
    if (level === "context") {
      nodes.push({ id: sv.system, kind: "system", title: sv.system, sub: "the system", lines: [`${sv.containers.length} container${sv.containers.length === 1 ? "" : "s"}`], ghost: false });
      for (const e of sv.context_edges) edges.push({ from: e.from, to: e.to, label: e.label, dashed: false, warn: null });
    } else {
      for (const k of sv.containers) {
        const spof = issues.some((i) => i.kind === "spof" && i.targets.includes(k.name));
        nodes.push({
          id: k.name, kind: k.datastore ? "datastore" : "container", title: k.name,
          sub: [k.technology, k.replicas > 1 ? `×${k.replicas}` : null].filter(Boolean).join(" · ") || (k.datastore ? "datastore" : "container"),
          lines: k.packages.length ? [`${k.packages.join(", ")}`, `${k.classes} class${k.classes === 1 ? "" : "es"}`] : k.description ? [k.description] : [],
          ghost: k.status === "proposed", spof, target: { kind: "container", name: k.name },
        });
      }
      for (const d of sv.container_deps) edges.push({ from: d.from, to: d.to, label: `${d.count} dep${d.count === 1 ? "" : "s"}`, dashed: !d.sync, warn: null });
      for (const l of sv.links) {
        const label = [l.interaction !== "sync" ? `«${l.interaction}»` : null, l.protocol].filter(Boolean).join(" ") || null;
        edges.push({ from: l.from, to: l.to, label, dashed: l.interaction !== "sync", warn: null });
      }
    }
    return { nodes, edges };
  }, [sv, level, issues]);

  const boxes = useMemo(() => {
    const m = new Map<string, { w: number; h: number }>();
    for (const n of nodes) {
      let w = Math.max(textWidth(n.title, F.name), textWidth(n.sub ?? "", F.small)) + 40;
      for (const l of n.lines) w = Math.max(w, textWidth(l, F.small) + 30);
      const w2 = Math.min(320, Math.max(n.kind === "system" ? 240 : 170, Math.ceil(w)));
      m.set(n.id, { w: w2, h: (n.kind === "person" ? 92 : 62) + n.lines.length * 16 });
    }
    return m;
  }, [nodes]);
  const [pos, setPos] = useState<Map<string, Pos>>(new Map());
  const key = level + JSON.stringify([...boxes.entries()]) + JSON.stringify(edges.map((e) => e.from + ">" + e.to));
  useEffect(() => {
    elkLayout(nodes.map((n) => ({ id: n.id, w: boxes.get(n.id)!.w, h: boxes.get(n.id)!.h + 20 })), edges.map((e) => ({ from: e.from, to: e.to, kind: "dependency" })))
      .then(setPos);
  }, [key]); // eslint-disable-line react-hooks/exhaustive-deps

  const rect = (n: string) => {
    const p = pos.get(n);
    const b = boxes.get(n);
    return p && b ? { x: p.x, y: p.y, w: b.w, h: b.h } : null;
  };
  const collect = (r: WorldRect): Target[] =>
    nodes.filter((n) => {
      const q = pos.get(n.id);
      return n.target && q && q.x >= r.x1 && q.x <= r.x2 && q.y >= r.y1 && q.y <= r.y2;
    }).map((n) => n.target!);

  const rBoxes = nodes.flatMap((n) => { const r = rect(n.id); return r ? [{ id: n.id, x: r.x, y: r.y + (n.spof ? 12 : 0), w: r.w, h: r.h + (n.spof ? 24 : 0) }] : []; });
  const routes = useRoutes(rBoxes, edges.filter((e) => e.from !== e.to).map((e, i) => ({ id: String(i), from: e.from, to: e.to, toDeco: "arrow" as const })));
  const empty = level === "container" ? sv.containers.length === 0 : nodes.length <= 1 && sv.actors.length === 0;
  return (
    <>
      <div className="level-toggle" data-ui>
        <button className={level === "context" ? "on" : ""} onClick={() => onLevel("context")}>Context</button>
        <button className={level === "container" ? "on" : ""} onClick={() => onLevel("container")}>Containers</button>
      </div>
      {empty && (
        <div className="banner center" data-ui>
          {level === "container"
            ? "No containers yet — ask the architect, e.g. “which containers (deployable units) do we need?”"
            : "No actors or external systems yet — generate use cases or ask which systems we integrate with."}
        </div>
      )}
      <Viewport ref={(v) => void (vpRef.current = v)} defs={<NodeDefs />} empty={pos.size === 0} key={level} onPrune={onPrune} collect={collect} onHover={onHover}
        onClickTarget={(t) => onClickTarget(t)}>
        {edges.map((e, i) => {
          const a = rect(e.from);
          const b = rect(e.to);
          if (!a || !b) return null;
          return (
            <Edge key={i} route={routes.get(String(i))} from={a} to={b} kind={e.dashed ? "dependency" : "association"} directed label={e.label} warn={e.warn}
              bend={edges.some((x) => x.from === e.to && x.to === e.from) ? 1 : 0}
              target={e.from !== sv.system && e.to !== sv.system && sv.links.some((l) => l.from === e.from && l.to === e.to) ? T({ kind: "link", id: `${e.from}->${e.to}` }) : undefined}
              style={{ color: C.edge, width: 1.5, opacity: 1, dashed: e.dashed }} />
          );
        })}
        {nodes.map((n) => {
          const r = rect(n.id);
          if (!r) return null;
          const color = COLOR[n.kind];
          const x = r.x - r.w / 2;
          const y = r.y - r.h / 2;
          const top = n.kind === "person" ? 34 : 0;
          return (
            <g key={n.id} transform={`translate(${x},${y})`} opacity={n.ghost ? 0.6 : 1}><g className={n.ghost ? "node-enter node-ghost" : "node-enter"}>
              {n.spof && <rect x={-4} y={top - 4} width={r.w + 8} height={r.h - top + 8} rx={14} fill={C.cycle} opacity={0.35} filter="url(#halo3)" />}
              {n.kind === "person" && <circle cx={r.w / 2} cy={16} r={16} fill={C.node} stroke={color} strokeWidth={1.6} />}
              {n.kind === "datastore" ? (
                <path d={`M0,${top + 8} a${r.w / 2},8 0 0,0 ${r.w},0 v${r.h - top - 16} a${r.w / 2},8 0 0,1 ${-r.w},0 z M0,${top + 8} a${r.w / 2},8 0 0,1 ${r.w},0`}
                  fill={C.node} stroke={color} strokeWidth={1.6} strokeDasharray={n.ghost ? "6 4" : undefined}
                  data-target={n.target ? T(n.target) : undefined} data-hover={`c4:${n.id}`} />
              ) : (
                <rect y={top} width={r.w} height={r.h - top} rx={n.kind === "person" ? 18 : 10} fill={n.kind === "system" ? "#24314f" : C.node}
                  stroke={color} strokeWidth={n.kind === "system" ? 2 : 1.6} strokeDasharray={n.ghost || n.kind === "external" ? "6 4" : undefined}
                  data-target={n.target ? T(n.target) : undefined} data-hover={`c4:${n.id}`} />
              )}
              <text x={r.w / 2} y={top + 24} textAnchor="middle" fontFamily={FONT_SANS} fontSize={14} fontWeight={600} fill={C.text} pointerEvents="none">{n.title}</text>
              {n.sub && <text x={r.w / 2} y={top + 40} textAnchor="middle" fontFamily={FONT_SANS} fontSize={11} fill={C.dim} pointerEvents="none">[{n.sub}]</text>}
              {n.lines.map((l, i) => (
                <text key={i} x={r.w / 2} y={top + 58 + i * 16} textAnchor="middle" fontFamily={FONT_SANS} fontSize={11} fill={C.faint} pointerEvents="none">{l}</text>
              ))}
              {n.spof && <Chip x={r.w / 2 - 50} y={r.h + 6} text="single point of failure" color={C.cycle} />}
            </g></g>
          );
        })}
      </Viewport>
    </>
  );
}
