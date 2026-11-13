import { useEffect, useMemo, useRef, useState } from "react";
import { NodeDefs } from "../canvas/ClassNode";
import { boxesFor, DiagramContent } from "../canvas/DiagramContent";
import { elkLayout } from "../canvas/layout";
import { healthColor } from "../canvas/visuals";
import { api } from "../state/api";
import type { Decision, Design, OptionPreview, Pos, Snapshot } from "../state/types";
import { ImpactLine } from "./Impact";

interface Props {
  decisions: Decision[];
  snap: Snapshot;
  onPreview: (d: Design | null) => void;
  toast: (kind: string, text: string) => void;
  onCompare: (id: string) => void;
}

export function DecisionPanel({ decisions, snap, onPreview, toast, onCompare }: Props) {
  const [idx, setIdx] = useState(0);
  const d = decisions[Math.min(idx, decisions.length - 1)];
  return (
    <div className="decision-panel" data-ui onMouseLeave={() => onPreview(null)}>
      <div className="dp-head">
        <span className="dp-kicker">design decision{decisions.length > 1 ? ` ${idx + 1}/${decisions.length}` : ""}</span>
        {d.options.length > 1 && <button className="link" onClick={() => onCompare(d.id)} title="side by side (/compare)">compare</button>}
        {decisions.length > 1 && (
          <span className="dp-nav">
            <button onClick={() => setIdx((i) => (i + decisions.length - 1) % decisions.length)}>‹</button>
            <button onClick={() => setIdx((i) => (i + 1) % decisions.length)}>›</button>
          </span>
        )}
      </div>
      <DecisionCard key={d.id} d={d} snap={snap} onPreview={onPreview} toast={toast} />
    </div>
  );
}

function DecisionCard({ d, snap, onPreview, toast }: { d: Decision; snap: Snapshot; onPreview: (d: Design | null) => void; toast: Props["toast"] }) {
  const last = d.attempts[d.attempts.length - 1];
  const [choice, setChoice] = useState<string | null>(d.chosen ?? null);
  const [custom, setCustom] = useState<string>(d.custom ?? "");
  const [ownMode, setOwnMode] = useState(!!d.custom && !d.chosen);
  const [defense, setDefense] = useState(d.state === "rejected" ? last?.defense ?? "" : "");
  const evaluating = d.state === "evaluating";
  const rejected = d.state === "rejected";
  const lenient = snap.view.settings.examiner === "lenient";
  const previews = snap.previews.filter((p) => p.decision === d.id);
  const health = snap.metrics.health;
  const verdictRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (rejected) verdictRef.current?.scrollIntoView({ behavior: "smooth", block: "center" });
  }, [rejected, d.attempts.length]);

  const submit = () => {
    if (!ownMode && !choice) return toast("error", "pick an option or write your own");
    if (ownMode && !custom.trim()) return toast("error", "describe your alternative");
    if (defense.trim().length < 3) return toast("error", "you have to defend your choice");
    api.defend(d.id, ownMode ? null : choice, ownMode ? custom.trim() : null, defense.trim()).catch((e) => toast("error", String(e)));
  };

  return (
    <div className={`decision ${d.state}`}>
      <div className="dq">{d.question}</div>
      {d.context && <div className="dctx">{d.context}</div>}
      <div className="options">
        {d.options.map((o) => {
          const pv = previews.find((p) => p.option === o.id);
          const sel = !ownMode && choice === o.id;
          return (
            <div key={o.id} className={`option ${sel ? "sel" : ""}`}
              onClick={() => !evaluating && (setChoice(o.id), setOwnMode(false))}
              onMouseEnter={() => pv?.design && onPreview(pv.design)} onMouseLeave={() => onPreview(null)}>
              <div className="o-head">
                <span className="o-id">{o.id}</span>
                <span className="o-label">{o.label}</span>
                {pv?.health != null && (
                  <span className="o-health" style={{ color: healthColor(pv.health) }} title="architecture health if chosen">
                    {pv.health}{pv.health !== health && <em>{pv.health > health ? ` ▲${pv.health - health}` : ` ▼${health - pv.health}`}</em>}
                  </span>
                )}
              </div>
              <MiniPreview pv={pv} />
              <ImpactLine impact={pv?.impact} />
              {o.summary && <div className="o-sum">{o.summary}</div>}
              {o.tradeoffs?.length > 0 && (
                <div className="tradeoffs">
                  {o.tradeoffs.map((t, i) => {
                    const minus = t.effect.trim().startsWith("-");
                    const known = snap.view.forces.some((f) => f.name === t.force);
                    return (
                      <span key={i} className={`tchip ${minus ? "minus" : "plus"} ${known ? "" : "unknown"}`}
                        title={(t.note ?? "") + (known ? "" : " (not in the brief)")}>
                        {minus ? "−" : "+"} {t.force}
                      </span>
                    );
                  })}
                </div>
              )}
              {(o.pros.length > 0 || o.cons.length > 0) && (
                <div className="o-pc">
                  {o.pros.map((x, i) => <div key={"p" + i} className="pro">+ {x}</div>)}
                  {o.cons.map((x, i) => <div key={"c" + i} className="con">− {x}</div>)}
                </div>
              )}
            </div>
          );
        })}
        <div className={`option own ${ownMode ? "sel" : ""}`} onClick={() => !evaluating && setOwnMode(true)}>
          <div className="o-head"><span className="o-id">✎</span><span className="o-label">my own alternative</span></div>
          {ownMode && (
            <textarea rows={2} value={custom} autoFocus placeholder="Describe your design: which classes, who talks to whom…" disabled={evaluating}
              onChange={(e) => setCustom(e.target.value)} onClick={(e) => e.stopPropagation()} />
          )}
        </div>
      </div>

      {rejected && last?.feedback && (
        <div className="verdict reject" ref={verdictRef}>
          <b>Examiner:</b> {last.feedback}
        </div>
      )}
      {d.attempts.length > 1 && <div className="faint small">attempt {d.attempts.length}</div>}

      <div className="defend">
        <textarea rows={3} value={defense} disabled={evaluating}
          placeholder={rejected ? "Revise your defence — answer the examiner's objection…" : "Defend your choice. Why is this the right design here? (⌘⏎)"}
          onChange={(e) => setDefense(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
              e.preventDefault();
              submit();
            }
          }} />
        <div className="d-actions">
          {evaluating ? (
            <span className="eval-note"><span className="spinner" /> the examiner is reading your defence…</span>
          ) : (
            <>
              <button className="primary" onClick={submit}>{rejected ? "Defend again" : "Defend"}</button>
              {rejected && lenient && (
                <button className="warnbtn" title="Apply anyway; marked as overruled" onClick={() => api.overrule(d.id).catch((e) => toast("error", String(e)))}>
                  Overrule ⚠
                </button>
              )}
              <button className="link" onClick={() => api.dismiss(d.id).catch((e) => toast("error", String(e)))}>skip</button>
            </>
          )}
        </div>
      </div>
    </div>
  );
}

/** Static mini-diagram of an option: the touched classes and their neighbours. */
function MiniPreview({ pv }: { pv?: OptionPreview }) {
  const [pos, setPos] = useState<Map<string, Pos> | null>(null);
  const sub = useMemo(() => {
    if (!pv?.design) return null;
    const d = pv.design;
    // what the option adds/changes …
    const changed = new Set<string>();
    for (const c of d.classes) if (c.status !== "accepted" || c.attributes.some((a) => a.status !== "accepted") || c.methods.some((m) => m.status !== "accepted")) changed.add(c.name);
    for (const r of d.relations) if (r.status !== "accepted") (changed.add(r.from), changed.add(r.to));
    if (changed.size === 0) pv.focus.forEach((f) => changed.add(f));
    // … plus a little context
    const keep = new Set(changed);
    for (const r of d.relations) {
      if (keep.size >= 6) break;
      if (r.status !== "accepted" && changed.has(r.from)) keep.add(r.to);
    }
    const focus = changed;
    const classes = d.classes.filter((c) => keep.has(c.name)).slice(0, 7);
    const names = new Set(classes.map((c) => c.name));
    const relations = d.relations.filter((r) => names.has(r.from) && names.has(r.to));
    return { classes, relations, packages: d.packages, focus };
  }, [pv]);
  const boxes = useMemo(() => (sub ? boxesFor(sub.classes, true, 3, sub.relations) : new Map()), [sub]);

  useEffect(() => {
    if (!sub) return;
    let alive = true;
    elkLayout(sub.classes.map((c) => ({ id: c.name, w: boxes.get(c.name)!.w, h: boxes.get(c.name)!.h })), sub.relations.map((r) => ({ from: r.from, to: r.to, kind: r.kind })), "RIGHT")
      .then((p) => alive && setPos(p));
    return () => {
      alive = false;
    };
  }, [sub, boxes]);

  if (pv?.error) return <div className="mini err">✗ {pv.error}</div>;
  if (!sub || !pos) return <div className="mini" />;
  let x1 = Infinity, y1 = Infinity, x2 = -Infinity, y2 = -Infinity;
  for (const c of sub.classes) {
    const p = pos.get(c.name);
    const b = boxes.get(c.name);
    if (!p || !b) continue;
    x1 = Math.min(x1, p.x - b.w / 2);
    y1 = Math.min(y1, p.y - b.h / 2);
    x2 = Math.max(x2, p.x + b.w / 2);
    y2 = Math.max(y2, p.y + b.h / 2);
  }
  if (!isFinite(x1)) return <div className="mini" />;
  const pad = 30;
  return (
    <svg className="mini" viewBox={`${x1 - pad} ${y1 - pad} ${x2 - x1 + 2 * pad} ${y2 - y1 + 2 * pad}`} preserveAspectRatio="xMidYMid meet">
      <defs><NodeDefs /></defs>
      <DiagramContent level={3} classes={sub.classes} relations={sub.relations} packages={sub.packages} pos={pos} boxes={boxes} still showPackages={false} highlight={sub.focus} />
    </svg>
  );
}
