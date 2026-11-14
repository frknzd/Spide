import { useEffect, useMemo, useState } from "react";
import { NodeDefs } from "../canvas/ClassNode";
import { boxesFor, DiagramContent } from "../canvas/DiagramContent";
import { elkLayout } from "../canvas/layout";
import { healthColor } from "../canvas/visuals";
import { api } from "../state/api";
import { decisionDraft, saveDecisionDraft, type DecisionDraft } from "../state/decision-drafts";
import type { Decision, Design, OptionPreview, Pos, Snapshot } from "../state/types";
import { ImpactLine, ImpactDetails } from "./Impact";

interface Props {
  decisions: Decision[];
  snap: Snapshot;
  onPreview: (d: Design | null) => void;
  toast: (kind: string, text: string) => void;
  onCompare: (id: string) => void;
}

export function DecisionPanel({ decisions, snap, onPreview, toast, onCompare }: Props) {
  const [active, setActive] = useState<string | null>(null);
  const idx = Math.max(0, decisions.findIndex((d) => d.id === active));
  const d = decisions[idx];
  const [previewing, setPreviewing] = useState<string | null>(null);
  useEffect(() => {
    setPreviewing(null); onPreview(null);
    return () => onPreview(null);
  }, [d.id, snap.rev, snap.project_dir, onPreview]);
  const preview = (pv: OptionPreview) => {
    const stop = previewing === pv.option;
    setPreviewing(stop ? null : pv.option);
    onPreview(stop ? null : pv.design);
  };
  return <div className="decision-panel" data-ui>
    <header className="decision-header">
      <div className="dp-head">
        <span className="dp-kicker">Decision {idx + 1} of {decisions.length}</span>
        {d.options.length > 1 && <button className="link" onClick={() => onCompare(d.id)}>Compare</button>}
        {decisions.length > 1 && <span className="dp-nav">
          <button aria-label="Previous decision" onClick={() => setActive(decisions[(idx + decisions.length - 1) % decisions.length].id)}>‹</button>
          <button aria-label="Next decision" onClick={() => setActive(decisions[(idx + 1) % decisions.length].id)}>›</button>
        </span>}
      </div>
      <h2 className="dq">{d.question}</h2>
    </header>
    <DecisionCard key={`${snap.project_dir}:${d.id}`} d={d} snap={snap} previewing={previewing} onPreview={preview} toast={toast} />
  </div>;
}

function DecisionCard({ d, snap, previewing, onPreview, toast }: { d: Decision; snap: Snapshot; previewing: string | null; onPreview: (pv: OptionPreview) => void; toast: Props["toast"] }) {
  const last = d.attempts.at(-1);
  const [draft, setDraft] = useState(() => decisionDraft(snap.project_dir, d));
  const change = (patch: Partial<DecisionDraft>) => setDraft((old) => {
    const next = { ...old, ...patch };
    saveDecisionDraft(snap.project_dir, d.id, next);
    return next;
  });
  const [sending, setSending] = useState(false);
  const [error, setError] = useState("");
  const evaluating = d.state === "evaluating";
  const rejected = d.state === "rejected";
  const disabled = evaluating || sending;
  const validChoice = draft.ownMode ? !!draft.custom.trim() : d.options.some((o) => o.id === draft.choice);
  const valid = validChoice && draft.defense.trim().length >= 3;
  const previews = snap.previews.filter((p) => p.decision === d.id);
  const health = snap.metrics.health;
  const request = async (f: () => Promise<unknown>) => {
    if (disabled) return;
    setSending(true); setError("");
    try { await f(); }
    catch (e) { setError(String(e)); toast("error", String(e)); }
    finally { setSending(false); }
  };
  const submit = () => {
    if (!valid || disabled) return;
    void request(() => api.defend(d.id, draft.ownMode ? null : draft.choice, draft.ownMode ? draft.custom.trim() : null, draft.defense.trim()));
  };
  return <div className={`decision ${d.state}`}>
    <div className="decision-options">
      {d.context && <div className="dctx">{d.context}</div>}
      <fieldset className="options" disabled={disabled}>
        <legend className="sr-only">Choose an alternative</legend>
        {d.options.map((o) => {
          const pv = previews.find((p) => p.option === o.id);
          const selected = !draft.ownMode && draft.choice === o.id;
          const expanded = draft.expanded.includes(o.id);
          return <article key={o.id} className={`option ${selected ? "sel" : ""} ${previewing === o.id ? "previewing" : ""}`}>
            <label className="o-head">
              <input type="radio" name={`decision-${d.id}`} checked={selected} onChange={() => change({ choice: o.id, ownMode: false })} aria-label={`Choose ${o.label}`} />
              <span className="o-id">{o.id}</span><span className="o-label">{o.label}</span>
              {pv?.health != null && <span className="o-health" style={{ color: healthColor(pv.health) }} title="Health if chosen (delta from current)">{pv.health}<em>{pv.health >= health ? " +" : " "}{pv.health - health}</em></span>}
            </label>
            {o.summary && <div className="o-sum">{o.summary}</div>}
            <div className="option-controls">
              <button type="button" className="link" aria-expanded={expanded} onClick={() => change({ expanded: expanded ? draft.expanded.filter((x) => x !== o.id) : [...draft.expanded, o.id] })}>{expanded ? "Hide details" : "Details"}</button>
              <button type="button" className={`link ${previewing === o.id ? "active" : ""}`} disabled={!pv?.design || !!pv.error} aria-pressed={previewing === o.id} onClick={() => pv && onPreview(pv)}>{previewing === o.id ? "Stop preview" : "Preview"}</button>
            </div>
            {pv?.error && <div className="small con">{pv.error}</div>}
            {expanded && <div className="option-details">
              <MiniPreview pv={pv} />
              <ImpactLine impact={pv?.impact} />
              {pv?.impact && <ImpactDetails impact={pv.impact} />}
              {!!o.tradeoffs?.length && <div className="tradeoffs">{o.tradeoffs.map((t, i) => <span key={i} className={`tchip ${t.effect.trim().startsWith("-") ? "minus" : "plus"}`} title={t.note ?? ""}>{t.effect} {t.force}</span>)}</div>}
              <div className="o-pc">{o.pros.map((x, i) => <div key={`p${i}`} className="pro">+ {x}</div>)}{o.cons.map((x, i) => <div key={`c${i}`} className="con">− {x}</div>)}</div>
            </div>}
          </article>;
        })}
      </fieldset>
    </div>
    <footer className="decision-footer">
      <label className={`own-choice ${draft.ownMode ? "sel" : ""}`}>
        <input type="radio" name={`decision-${d.id}`} checked={draft.ownMode} disabled={disabled} onChange={() => change({ ownMode: true })} /> My own alternative
      </label>
      {draft.ownMode ? <textarea aria-label="Custom alternative" rows={2} value={draft.custom} disabled={disabled} placeholder="Describe your alternative…" onChange={(e) => change({ custom: e.target.value })} /> :
        <div className="small faint">Selected: {d.options.find((o) => o.id === draft.choice)?.label ?? "choose an option above"}</div>}
      {last?.feedback && <div className={`verdict ${rejected ? "reject" : "accept"}`}><b>Examiner:</b> {last.feedback}</div>}
      {d.attempts.length > 1 && <div className="faint small">attempt {d.attempts.length}</div>}
      <label className="defense-label" htmlFor={`defense-${d.id}`}>Your defence</label>
      <textarea id={`defense-${d.id}`} rows={3} value={draft.defense} disabled={disabled} placeholder={rejected ? "Answer the examiner's objection…" : "Why is this the right design here? (⌘⏎)"} onChange={(e) => change({ defense: e.target.value })} onKeyDown={(e) => {
        if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) { e.preventDefault(); submit(); }
      }} />
      {error && <div role="alert" className="con small">{error}</div>}
      <div className="d-actions">
        {evaluating ? <span className="eval-note" role="status"><span className="spinner" /> Examiner is reading…</span> : <>
          <button className="primary" disabled={!valid || disabled} onClick={submit}>{sending ? "Sending…" : rejected ? "Defend again" : "Defend"}</button>
          {rejected && snap.view.settings.examiner === "lenient" && <button className="warnbtn" disabled={disabled} title="Apply anyway; marked as overruled" onClick={() => void request(() => api.overrule(d.id))}>Overrule ⚠</button>}
          <button className="link" disabled={disabled} onClick={() => void request(() => api.dismiss(d.id).then(() => toast("info", "Decision skipped. ⌘Z restores it.")))} title="Close this decision without applying an option">Skip decision</button>
        </>}
      </div>
    </footer>
  </div>;
}

// Cache bounded static layouts across collapses and equivalent incoming snapshots.
const miniLayouts = new Map<string, Promise<Map<string, Pos>>>();
function MiniPreview({ pv }: { pv?: OptionPreview }) {
  const [pos, setPos] = useState<Map<string, Pos> | null>(null);
  const [failed, setFailed] = useState(false);
  const sub = useMemo(() => {
    if (!pv?.design) return null;
    const d = pv.design;
    const changed = new Set<string>();
    for (const c of d.classes) if (c.status !== "accepted" || c.attributes.some((a) => a.status !== "accepted") || c.methods.some((m) => m.status !== "accepted")) changed.add(c.name);
    for (const r of d.relations) if (r.status !== "accepted") { changed.add(r.from); changed.add(r.to); }
    if (!changed.size) pv.focus.forEach((f) => changed.add(f));
    const keep = new Set(changed);
    for (const r of d.relations) { if (keep.size >= 6) break; if (changed.has(r.from)) keep.add(r.to); }
    const classes = d.classes.filter((c) => keep.has(c.name)).slice(0, 7);
    const names = new Set(classes.map((c) => c.name));
    return { classes, relations: d.relations.filter((r) => names.has(r.from) && names.has(r.to)), packages: d.packages, focus: changed };
  }, [pv]);
  const boxes = useMemo(() => sub ? boxesFor(sub.classes, true, 3, sub.relations) : new Map(), [sub]);
  useEffect(() => {
    setPos(null); setFailed(false);
    if (!sub) return;
    let alive = true;
    const nodes = sub.classes.map((c) => ({ id: c.name, w: boxes.get(c.name)!.w, h: boxes.get(c.name)!.h }));
    const links = sub.relations.map((r) => ({ from: r.from, to: r.to, kind: r.kind }));
    const key = JSON.stringify([nodes, links]);
    if (!miniLayouts.has(key)) {
      if (miniLayouts.size >= 64) miniLayouts.delete(miniLayouts.keys().next().value!);
      miniLayouts.set(key, elkLayout(nodes, links, "RIGHT"));
    }
    miniLayouts.get(key)!.then((p) => { if (alive) setPos(p); }).catch(() => { miniLayouts.delete(key); if (alive) setFailed(true); });
    return () => { alive = false; };
  }, [sub, boxes]);
  if (pv?.error) return <div className="mini err">{pv.error}</div>;
  if (!sub) return <div className="mini loading">Preview unavailable for this option.</div>;
  if (failed) return <div className="mini err">Couldn't draw this preview. Use Preview to inspect it on the canvas.</div>;
  if (!pos) return <div className="mini loading"><span className="spinner" /> Drawing preview…</div>;
  let x1 = Infinity, y1 = Infinity, x2 = -Infinity, y2 = -Infinity;
  for (const c of sub.classes) {
    const p = pos.get(c.name), b = boxes.get(c.name);
    if (!p || !b) continue;
    x1 = Math.min(x1, p.x - b.w / 2); y1 = Math.min(y1, p.y - b.h / 2);
    x2 = Math.max(x2, p.x + b.w / 2); y2 = Math.max(y2, p.y + b.h / 2);
  }
  if (!isFinite(x1)) return <div className="mini" />;
  return <svg className="mini" viewBox={`${x1 - 30} ${y1 - 30} ${x2 - x1 + 60} ${y2 - y1 + 60}`} preserveAspectRatio="xMidYMid meet" aria-label="Alternative diagram">
    <defs><NodeDefs /></defs>
    <DiagramContent level={3} classes={sub.classes} relations={sub.relations} packages={sub.packages} pos={pos} boxes={boxes} still showPackages={false} highlight={sub.focus} />
  </svg>;
}
