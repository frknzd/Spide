// Side-by-side comparison of a decision's options on a shared layout.
import { useEffect, useMemo, useState } from "react";
import { NodeDefs } from "../canvas/ClassNode";
import { boxesFor, DiagramContent } from "../canvas/DiagramContent";
import { elkLayout, type LayoutEdge } from "../canvas/layout";
import { healthColor } from "../canvas/visuals";
import type { Class, Decision, Pos, Snapshot } from "../state/types";
import { ImpactDetails } from "./Impact";

export function ComparePanel({ d, snap, onClose }: { d: Decision; snap: Snapshot; onClose: () => void }) {
  const cols = d.options.map((o) => ({ o, pv: snap.previews.find((p) => p.decision === d.id && p.option === o.id) }));

  // one layout for the union of all classes → every column shares positions
  const union = useMemo(() => {
    const m = new Map<string, Class>();
    for (const c of snap.view.classes) if (c.status !== "pruned") m.set(c.name, c);
    for (const { pv } of cols) for (const c of pv?.design?.classes ?? []) if (!m.has(c.name)) m.set(c.name, c);
    return [...m.values()];
  }, [snap, d.id]); // eslint-disable-line react-hooks/exhaustive-deps
  const boxes = useMemo(() => boxesFor(union, true), [union]);
  const [pos, setPos] = useState<Map<string, Pos> | null>(null);
  useEffect(() => {
    let alive = true;
    const rels = new Map<string, LayoutEdge>();
    for (const r of snap.view.relations) rels.set(r.id, r);
    for (const { pv } of cols) for (const r of pv?.design?.relations ?? []) rels.set(r.id, r);
    elkLayout(union.map((c) => ({ id: c.name, w: boxes.get(c.name)!.w, h: boxes.get(c.name)!.h })), [...rels.values()])
      .then((p) => alive && setPos(p));
    return () => {
      alive = false;
    };
  }, [union, boxes]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  let vb = "0 0 100 100";
  if (pos) {
    let x1 = Infinity, y1 = Infinity, x2 = -Infinity, y2 = -Infinity;
    for (const c of union) {
      const p = pos.get(c.name);
      const b = boxes.get(c.name);
      if (!p || !b) continue;
      x1 = Math.min(x1, p.x - b.w / 2);
      y1 = Math.min(y1, p.y - b.h / 2);
      x2 = Math.max(x2, p.x + b.w / 2);
      y2 = Math.max(y2, p.y + b.h / 2);
    }
    const pad = 40;
    if (isFinite(x1)) vb = `${x1 - pad} ${y1 - pad} ${x2 - x1 + 2 * pad} ${y2 - y1 + 2 * pad}`;
  }

  return (
    <div className="compare" data-ui>
      <div className="cmp-head">
        <span className="dp-kicker">compare options</span>
        <span className="cmp-q">{d.question}</span>
        <button className="link" onClick={onClose}>close (Esc)</button>
      </div>
      <div className="cmp-cols" style={{ gridTemplateColumns: `repeat(${cols.length}, 1fr)` }}>
        {cols.map(({ o, pv }) => {
          const changed = new Set(pv?.focus ?? []);
          return (
            <div key={o.id} className="cmp-col">
              <div className="o-head">
                <span className="o-id">{o.id}</span>
                <span className="o-label">{o.label}</span>
                {pv?.health != null && <span className="o-health" style={{ color: healthColor(pv.health) }}>{pv.health}</span>}
              </div>
              {pv?.error ? (
                <div className="mini err">✗ {pv.error}</div>
              ) : (
                <svg className="cmp-svg" viewBox={vb} preserveAspectRatio="xMidYMid meet">
                  <defs><NodeDefs /></defs>
                  {pos && pv?.design && (
                    <DiagramContent classes={pv.design.classes.filter((c) => c.status !== "pruned")} relations={pv.design.relations}
                      packages={pv.design.packages} pos={pos} boxes={boxes} still highlight={changed} metrics={null} />
                  )}
                </svg>
              )}
              {o.summary && <div className="o-sum">{o.summary}</div>}
              {pv?.impact && <ImpactDetails impact={pv.impact} />}
            </div>
          );
        })}
      </div>
    </div>
  );
}
