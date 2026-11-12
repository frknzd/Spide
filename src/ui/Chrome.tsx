// Small chrome pieces: health ring, tabs, proposal bar, toasts.
import { useEffect, useState } from "react";
import { healthColor } from "../canvas/visuals";
import type { Issue, Metrics, Tab } from "../state/types";

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
      {open && (
        <div className="health-pop">
          <div className="hp-title">
            architecture health <b style={{ color }}>{h}/100</b>
            {ghost !== null && ghost !== h && <span style={{ color: healthColor(ghost) }}> · {ghost} with proposals</span>}
          </div>
          {metrics.issues.length === 0 ? (
            <div className="faint">No cycles, god classes, layer violations or orphans. Clean.</div>
          ) : (
            metrics.issues.slice(0, 12).map((i, k) => (
              <div key={k} className={`issue ${i.kind}`} title={[i.explain, i.fix_hint && `fix: ${i.fix_hint}`].filter(Boolean).join("\n")}>
                <span className="ik">{i.kind.replace("_", " ")}</span>
                <span className={onFocus ? "imsg" : undefined} onClick={() => onFocus?.(i)}>{i.message}</span>
                {onFix && <button className="fixbtn" onClick={() => onFix(i)}>fix with agent</button>}
              </div>
            ))
          )}
          <div className="hp-legend">
            <span><i style={{ background: "#5b8cff" }} />calm</span>
            <span><i style={{ background: "#ffb454" }} />coupled</span>
            <span><i style={{ background: "#ff5c6c" }} />hot</span>
            <span>size ∝ coupling · trembling = tension</span>
          </div>
        </div>
      )}
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
