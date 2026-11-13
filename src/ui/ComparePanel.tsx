// Side-by-side comparison of a decision's options on a shared layout.
import { useEffect, useMemo, useState } from "react";
import { NodeDefs } from "../canvas/ClassNode";
import { boxesFor, DiagramContent } from "../canvas/DiagramContent";
import { elkLayout, type LayoutEdge } from "../canvas/layout";
import { healthColor } from "../canvas/visuals";
import type { Class, Decision, Design, Impact, Pos, Snapshot } from "../state/types";
import { ImpactDetails } from "./Impact";

export interface CompareCol {
  id: string;
  label: string;
  design: Design | null;
  health: number | null;
  impact: Impact | null;
  focus: string[];
  error?: string | null;
  summary?: string | null;
}

/** Columns for a decision's options. */
export function decisionCols(d: Decision, snap: Snapshot): CompareCol[] {
  return d.options.map((o) => {
    const pv = snap.previews.find((p) => p.decision === d.id && p.option === o.id);
    return { id: o.id, label: o.label, design: pv?.design ?? null, health: pv?.health ?? null, impact: pv?.impact ?? null, focus: pv?.focus ?? [], error: pv?.error, summary: o.summary };
  });
}

export function ComparePanel({ title, kicker = "compare options", cols, snap, onClose }: { title: string; kicker?: string; cols: CompareCol[]; snap: Snapshot; onClose: () => void }) {

  // one layout for the union of all classes → every column shares positions
  const union = useMemo(() => {
    const m = new Map<string, Class>();
    for (const c of snap.view.classes) if (c.status !== "pruned") m.set(c.name, c);
    for (const col of cols) for (const c of col.design?.classes ?? []) if (!m.has(c.name) && c.status !== "pruned") m.set(c.name, c);
    return [...m.values()];
  }, [snap, cols.map((c) => c.id).join()]); // eslint-disable-line react-hooks/exhaustive-deps
  const boxes = useMemo(() => boxesFor(union, true, 3, snap.view.relations), [union]); // eslint-disable-line react-hooks/exhaustive-deps
  const [pos, setPos] = useState<Map<string, Pos> | null>(null);
  useEffect(() => {
    let alive = true;
    const rels = new Map<string, LayoutEdge>();
    for (const r of snap.view.relations) rels.set(r.id, r);
    for (const col of cols) for (const r of col.design?.relations ?? []) rels.set(r.id, r);
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
        <span className="dp-kicker">{kicker}</span>
        <span className="cmp-q">{title}</span>
        <button className="link" onClick={onClose}>close (Esc)</button>
      </div>
      <div className="cmp-cols" style={{ gridTemplateColumns: `repeat(${cols.length}, 1fr)` }}>
        {cols.map((col) => {
          const changed = new Set(col.focus);
          return (
            <div key={col.id} className="cmp-col">
              <div className="o-head">
                <span className="o-id">{col.id}</span>
                <span className="o-label">{col.label}</span>
                {col.health != null && <span className="o-health" style={{ color: healthColor(col.health) }}>{col.health}</span>}
              </div>
              {col.error ? (
                <div className="mini err">✗ {col.error}</div>
              ) : (
                <svg className="cmp-svg" viewBox={vb} preserveAspectRatio="xMidYMid meet">
                  <defs><NodeDefs /></defs>
                  {pos && col.design && (
                    <DiagramContent level={3} classes={col.design.classes.filter((c) => c.status !== "pruned")} relations={col.design.relations.filter((r) => r.status !== "pruned")}
                      packages={col.design.packages} pos={pos} boxes={boxes} still highlight={changed} metrics={null} />
                  )}
                </svg>
              )}
              {col.summary && <div className="o-sum">{col.summary}</div>}
              {col.impact && <ImpactDetails impact={col.impact} />}
            </div>
          );
        })}
      </div>
    </div>
  );
}
