// Small chrome pieces: health ring, tabs, proposal bar, toasts.
import { useEffect, useState } from "react";
import { healthColor } from "../canvas/visuals";
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
        {metrics.issues.length === 0 && <div className="faint">Low coupling, high cohesion, responsibilities where the data is. Clean.</div>}
        {groups.map((g) => (
          <div key={g.id} className="hp-group">
            <div className="hc-h">{g.label} · {g.issues.length}</div>
            {g.issues.slice(0, only ? 40 : 6).map((i, k) => (
              <div key={k} className={`issue ${i.kind}`} title={[i.explain, i.fix_hint && `fix: ${i.fix_hint}`].filter(Boolean).join("\n")}>
                <span className="ik">{i.kind.replace(/_/g, " ")}</span>
                <span className={onFocus ? "imsg" : undefined} onClick={() => onFocus?.(i)}>{i.message}</span>
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
  const h = metrics.health;
  const r = 19;
  const circ = 2 * Math.PI * r;
  const color = healthColor(h);
  return (
    <div className="health" data-ui onMouseEnter={() => setOpen(true)} onMouseLeave={() => setOpen(false)}>
      <svg width={48} height={48} viewBox="0 0 48 48">
        <circle cx={24} cy={24} r={r} fill="none" stroke="#2c2f36" strokeWidth={4} />
        {ghost !== null && ghost !== h && (
          <circle cx={24} cy={24} r={r} fill="none" stroke={healthColor(ghost)} strokeOpacity={0.35} strokeWidth={4} strokeDasharray={`${(circ * ghost) / 100} ${circ}`}
            transform="rotate(-90 24 24)" strokeLinecap="round" />
        )}
        <circle cx={24} cy={24} r={r} fill="none" stroke={color} strokeWidth={4} strokeDasharray={`${(circ * h) / 100} ${circ}`}
          transform="rotate(-90 24 24)" strokeLinecap="round" style={{ transition: "stroke-dasharray 600ms, stroke 600ms" }} />
        <text x={24} y={28.5} textAnchor="middle" fontSize={13} fontWeight={700} fill={color} fontFamily="Helvetica Neue">{h}</text>
      </svg>
      {ghost !== null && ghost !== h && <div className="ghost-health" style={{ color: healthColor(ghost) }}>→ {ghost}</div>}
      {open && <HealthPopover metrics={metrics} ghost={ghost} onFix={onFix} onFocus={onFocus} />}
    </div>
  );
}

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
    <div className="tabs" data-ui>
      {TABS.map((t, i) => (
        <button key={t.id} className={tab === t.id ? "on" : ""} onClick={() => onTab(t.id)} title={`⌘${i + 1}`}>
          {t.label}
          {stale[t.id] && <span className="stale" title="out of date with the class model" />}
        </button>
      ))}
    </div>
  );
}

export function ProposalBar({ count, onAccept, onReject }: { count: number; onAccept: () => void; onReject: () => void }) {
  if (!count) return null;
  return (
    <div className="proposal-bar" data-ui>
      <span className="pg">{count} ghost proposal{count > 1 ? "s" : ""}</span>
      <span className="faint">click a ghost to accept · right-click to reject</span>
      <button onClick={onAccept}>accept all</button>
      <button className="ghostbtn" onClick={onReject}>reject all</button>
    </div>
  );
}

export interface Toast { id: number; kind: string; text: string }

export function Toasts({ toasts, onDone }: { toasts: Toast[]; onDone: (id: number) => void }) {
  return (
    <div className="toasts" data-ui>
      {toasts.map((t) => <ToastItem key={t.id} t={t} onDone={onDone} />)}
    </div>
  );
}

function ToastItem({ t, onDone }: { t: Toast; onDone: (id: number) => void }) {
  useEffect(() => {
    const id = setTimeout(() => onDone(t.id), t.kind === "error" ? 9000 : 5000);
    return () => clearTimeout(id);
  }, [t.id]); // eslint-disable-line react-hooks/exhaustive-deps
  return <div className={`toast ${t.kind}`} onClick={() => onDone(t.id)}>{t.text}</div>;
}
