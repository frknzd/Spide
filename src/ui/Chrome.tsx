// Small chrome pieces: health ring, tabs, proposal bar, toasts.
import { useEffect, useRef, useState } from "react";
import { healthColor } from "../canvas/visuals";
import { type ProposalSummary, ghostSpotlight } from "../state/proposal-focus";
import type { Issue, Metrics, Principle, Tab } from "../state/types";

const PRINCIPLES: { id: Principle; label: string; help: string }[] = [
  { id: "coupling", label: "coupling", help: "Minimise coupling: cycles, fan-out, Law of Demeter (coupling to stable libraries is fine)" },
  { id: "cohesion", label: "cohesion", help: "Increase cohesion: god classes, LCOM4, mixed concerns, vague names, long methods" },
  { id: "responsibility", label: "responsibilities", help: "Distribute responsibilities: Information Expert, Controller, Creator, Pure Fabrication, duplication, feature envy, data classes" },
  { id: "architecture", label: "architecture", help: "Your architecture rules: layers, modules, forbidden dependencies and types, orphans" },
  { id: "uml", label: "UML hygiene", help: "Exam notes: argument types, no repeated inherited members, Composite/Visitor sanity, long parameter lists" },
];

function confidenceText(i: Issue): string | null {
  if (i.basis === "structure" || i.confidence === undefined) return null;
  return `${Math.round(i.confidence * 100)}% · ${i.basis}`;
}

/** Health breakdown: the five principle scores and the issues grouped by principle, each with its course reference. */
function HealthPopover({ metrics, ghost, onFix, onFocus }: { metrics: Metrics; ghost: number | null; onFix?: (i: Issue) => void; onFocus?: (i: Issue) => void }) {
  const h = metrics.health;
  const color = healthColor(h);
  const [only, setOnly] = useState<Principle | null>(null);
  const issues = metrics.issues.filter((i) => !only || i.principle === only);
  const groups = PRINCIPLES.map((p) => ({ ...p, issues: issues.filter((i) => (i.principle ?? "uml") === p.id) })).filter((g) => g.issues.length);
  return (
    <div className="health-pop">
      <div className="hp-title">
        design health <b style={{ color }}>{h}/100</b>
        {ghost !== null && ghost !== h && <span style={{ color: healthColor(ghost) }}> · {ghost} with proposals</span>}
      </div>
      {metrics.principles && (
        <div className="hp-principles">
          {PRINCIPLES.map((p) => {
            const sc = metrics.principles![p.id];
            if (!sc) return null;
            return (
              <button key={p.id} className={`hp-p ${only === p.id ? "on" : ""}`} title={`${p.help} · weight ${Math.round(sc.weight * 100)}%`}
                onClick={() => setOnly((o) => (o === p.id ? null : p.id))}>
                <span className="hp-pl">{p.label}</span>
                <span className="hp-bar"><i style={{ width: `${sc.score}%`, background: healthColor(sc.score) }} /></span>
                <span className="hp-ps" style={{ color: healthColor(sc.score) }}>{sc.score}</span>
              </button>
            );
          })}
        </div>
      )}
      <div className="hp-issues">
        {only && issues.length === 0 && <div className="faint">No issues for this principle. <button className="link" onClick={() => setOnly(null)}>Show all</button></div>}
        {metrics.issues.length === 0 && <div className="faint">Low coupling, high cohesion, responsibilities where the data is. Clean.</div>}
        {groups.map((g) => (
          <div key={g.id} className="hp-group">
            <div className="hc-h">{g.label} · {g.issues.length}</div>
            {g.issues.slice(0, only ? undefined : 6).map((i, k) => (
              <div key={k} className={`issue ${i.kind}`} title={[i.explain, i.fix_hint && `fix: ${i.fix_hint}`].filter(Boolean).join("\n")}>
                <span className="ik">{i.kind.replace(/_/g, " ")}</span>
                {onFocus ? <button className="imsg" onClick={() => onFocus(i)}>{i.message}</button> : <span>{i.message}</span>}
                <div className="iref">
                  {i.source && <span>{i.source}</span>}
                  {confidenceText(i) && <span title="how sure Spide is (agent agreement raises it)">{confidenceText(i)}</span>}
                  {onFix && <button className="fixbtn" onClick={() => onFix(i)}>fix with agent</button>}
                </div>
              </div>
            ))}
            {!only && g.issues.length > 6 && <button className="link small" onClick={() => setOnly(g.id)}>… {g.issues.length - 6} more</button>}
          </div>
        ))}
      </div>
      <div className="hp-legend">
        <span><i style={{ background: "#5b8cff" }} />calm</span>
        <span><i style={{ background: "#ffb454" }} />coupled</span>
        <span><i style={{ background: "#ff5c6c" }} />hot</span>
        <span>size ∝ coupling · one number: weighted mean of the principles, scaled to the design's size</span>
      </div>
    </div>
  );
}

export function HealthRing({ metrics, ghost, onFix, onFocus }: { metrics: Metrics; ghost: number | null; onFix?: (i: Issue) => void; onFocus?: (i: Issue) => void }) {
  const [open, setOpen] = useState(false);
  const [pinned, setPinned] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const closeTimer = useRef<ReturnType<typeof setTimeout>>(undefined);
  const h = metrics.health;
  const r = 19;
  const circ = 2 * Math.PI * r;
  const color = healthColor(h);
  const cancelClose = () => clearTimeout(closeTimer.current);
  const close = (restore = false) => { cancelClose(); setOpen(false); setPinned(false); if (restore) trigger.current?.focus(); };
  const scheduleClose = () => { if (pinned) return; cancelClose(); closeTimer.current = setTimeout(() => setOpen(false), HEALTH_CLOSE_MS); };
  useEffect(() => {
    if (!open) return;
    const outside = (e: PointerEvent) => { if (!root.current?.contains(e.target as Node)) close(); };
    document.addEventListener("pointerdown", outside);
    return () => document.removeEventListener("pointerdown", outside);
  }, [open]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => cancelClose, []);
  return (
    <div ref={root} className="health" data-ui onPointerEnter={() => { cancelClose(); setOpen(true); }} onPointerLeave={scheduleClose}
      onFocus={cancelClose} onBlur={(e) => { if (!pinned && !root.current?.contains(e.relatedTarget as Node)) close(); }}
      onKeyDown={(e) => { if (e.key === "Escape" && open) { e.preventDefault(); e.stopPropagation(); close(true); } }}>
      <button ref={trigger} className="health-trigger" aria-expanded={open} aria-haspopup="dialog" aria-label={`Design health ${h} out of 100. ${metrics.issues.length} issues. Show breakdown`}
        onClick={() => { cancelClose(); if (pinned) close(); else { setOpen(true); setPinned(true); } }}>
      <svg width={48} height={48} viewBox="0 0 48 48" aria-hidden>
        <circle cx={24} cy={24} r={r} fill="none" stroke="#2c2f36" strokeWidth={4} />
        {ghost !== null && ghost !== h && (
          <circle cx={24} cy={24} r={r} fill="none" stroke={healthColor(ghost)} strokeOpacity={0.35} strokeWidth={4} strokeDasharray={`${(circ * ghost) / 100} ${circ}`}
            transform="rotate(-90 24 24)" strokeLinecap="round" />
        )}
        <circle cx={24} cy={24} r={r} fill="none" stroke={color} strokeWidth={4} strokeDasharray={`${(circ * h) / 100} ${circ}`}
          transform="rotate(-90 24 24)" strokeLinecap="round" style={{ transition: "stroke-dasharray 600ms, stroke 600ms" }} />
        <text x={24} y={28.5} textAnchor="middle" fontSize={13} fontWeight={700} fill={color} fontFamily="Helvetica Neue">{h}</text>
      </svg>
      <span className="health-copy"><b>Health</b><small>{metrics.issues.length} issue{metrics.issues.length === 1 ? "" : "s"}</small></span>
      </button>
      {ghost !== null && ghost !== h && <div className="ghost-health" style={{ color: healthColor(ghost) }}>→ {ghost}</div>}
      {open && <div className="health-pop-wrap" role="dialog" aria-label="Design health breakdown"><HealthPopover metrics={metrics} ghost={ghost} onFix={onFix} onFocus={onFocus} /></div>}
    </div>
  );
}

/** Hover dismissal grace for moving diagonally from the ring into the breakdown. */
export const HEALTH_CLOSE_MS = 280;

const TABS: { id: Tab; label: string }[] = [
  { id: "system", label: "System" },
  { id: "class", label: "Class" },
  { id: "domain", label: "Domain" },
  { id: "package", label: "Package" },
  { id: "usecase", label: "Use case" },
  { id: "sequence", label: "Sequence" },
];

export function Tabs({ tab, onTab, stale }: { tab: Tab; onTab: (t: Tab) => void; stale: Partial<Record<Tab, boolean>> }) {
  return (
    <nav className="tabs" data-ui aria-label="Diagram views">
      {TABS.map((t, i) => (
        <button key={t.id} aria-label={t.label} aria-description={stale[t.id] ? "Out of date with the class model" : undefined} aria-current={tab === t.id ? "page" : undefined} className={tab === t.id ? "on" : ""} onClick={() => onTab(t.id)} title={`${t.label} view · ⌘${i + 1}`}>
          {t.label}
          {stale[t.id] && <span className="stale" title="Out of date with the class model" />}
        </button>
      ))}
    </nav>
  );
}

export function ProposalBar({ count, ids = [], summaries = [], onAccept, onReject, onLocate, disabled }: { count: number; ids?: string[]; summaries?: ProposalSummary[]; onAccept: () => void; onReject: () => void; onLocate?: (id: string) => void; disabled?: boolean }) {
  const [at, setAt] = useState(-1);
  const [open, setOpen] = useState(false);
  const key = ids.join();
  useEffect(() => { setAt(-1); ghostSpotlight.set(null); }, [key]);
  useEffect(() => () => ghostSpotlight.set(null), []);
  if (!count) return null;
  const rest = () => ghostSpotlight.set(at >= 0 ? ids[at] ?? null : null);
  const go = (next: number) => {
    setAt(next);
    ghostSpotlight.set(ids[next]);
    onLocate?.(ids[next]);
  };
  const step = (d: number) => {
    if (!ids.length || !onLocate) return;
    go((at + d + ids.length) % ids.length);
  };
  const enter = () => { setOpen(true); if (!disabled) ghostSpotlight.set(at >= 0 ? ids[at] ?? "all" : "all"); };
  const one = at >= 0 ? summaries.find((x) => x.id === ids[at]) : summaries.length === 1 ? summaries[0] : null;
  return (
    <div className="proposal-wrap" data-ui onPointerEnter={enter} onPointerLeave={() => { setOpen(false); rest(); }}
      onFocus={enter} onBlur={(e) => { if (!e.currentTarget.contains(e.relatedTarget as Node)) { setOpen(false); rest(); } }}
      onKeyDown={(e) => { if (e.key === "Escape" && open) { e.stopPropagation(); setOpen(false); } }}>
      {open && summaries.length > 0 && (
        <div className="proposal-panel" role="region" aria-label="Ghost proposal details">
          <div className="proposal-panel-inner">
            {one ? <ProposalDetail s={one} index={at >= 0 ? at : 0} total={ids.length} /> : (
              <>
                <div className="pp-head">{summaries.length} ghost proposals <span className="faint">· ‹ › or pick one to locate it</span></div>
                <ul className="pp-list">
                  {summaries.map((s, i) => (
                    <li key={s.id}><button className="pp-pick" disabled={disabled || !onLocate} onClick={() => go(i)}>
                      <b>{s.rationale ?? s.items[0]?.text ?? s.ops}</b>
                      <span className="faint"> · {s.changes} change{s.changes === 1 ? "" : "s"} · {s.source}</span>
                    </button></li>
                  ))}
                </ul>
              </>
            )}
          </div>
        </div>
      )}
      <div className="proposal-bar">
        <span className="pg">{count} ghost proposal{count > 1 ? "s" : ""}</span>
        {onLocate && ids.length > 0 && <span className="locate" role="group" aria-label="Locate proposals">
          <button className="icon-button" disabled={disabled} aria-label="Previous proposal" title="Show previous proposal" onClick={() => step(-1)}>‹</button>
          <span className="faint" aria-live="polite">{at >= 0 ? `${at + 1}/${ids.length}` : "Locate"}</span>
          <button className="icon-button" disabled={disabled} aria-label="Next proposal" title="Show next proposal" onClick={() => step(1)}>›</button>
        </span>}
        <button disabled={disabled} onClick={onAccept} aria-label={`Accept all ${count} proposals`} title="Accept every ghost proposal">Accept all</button>
        <button disabled={disabled} className="ghostbtn" onClick={onReject} aria-label={`Reject all ${count} proposals`} title="Reject every ghost proposal">Reject all</button>
      </div>
    </div>
  );
}

const MAX_ITEMS = 8;
function ProposalDetail({ s, index, total }: { s: ProposalSummary; index: number; total: number }) {
  return (
    <>
      <div className="pp-head">{total > 1 ? `Proposal ${index + 1} of ${total}` : "Ghost proposal"} <span className="faint">· by {s.source}</span></div>
      {s.rationale ? <p className="pp-why">{s.rationale}</p> : <p className="pp-why faint">No rationale was given. What it changes:</p>}
      {s.items.length > 0 ? (
        <ul className="pp-items">
          {s.items.slice(0, MAX_ITEMS).map((it, k) => <li key={k} className={it.sign === "−" ? "minus" : "plus"}><span>{it.sign}</span> {it.text}</li>)}
          {s.items.length > MAX_ITEMS && <li className="faint">… {s.items.length - MAX_ITEMS} more</li>}
        </ul>
      ) : <div className="faint small">{s.ops || "no operations"} (not drawn on the class diagram)</div>}
    </>
  );
}

export interface Toast { id: number; kind: string; text: string }

export function Toasts({ toasts, onDone }: { toasts: Toast[]; onDone: (id: number) => void }) {
  return (
    <div className="toasts" data-ui aria-live="polite" aria-relevant="additions">
      {toasts.map((t) => <ToastItem key={t.id} t={t} onDone={onDone} />)}
    </div>
  );
}

function ToastItem({ t, onDone }: { t: Toast; onDone: (id: number) => void }) {
  const [paused, setPaused] = useState(false);
  useEffect(() => {
    if (paused) return;
    const id = setTimeout(() => onDone(t.id), t.kind === "error" ? 9000 : 5000);
    return () => clearTimeout(id);
  }, [t.id, paused]); // eslint-disable-line react-hooks/exhaustive-deps
  return <div className={`toast ${t.kind}`} role={t.kind === "error" ? "alert" : "status"} onPointerEnter={() => setPaused(true)} onPointerLeave={() => setPaused(false)} onFocus={() => setPaused(true)} onBlur={() => setPaused(false)}>
    <span>{t.text}</span><button className="icon-button" aria-label="Dismiss notification" onClick={() => onDone(t.id)}>×</button>
  </div>;
}
