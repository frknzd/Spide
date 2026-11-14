// The brief: forces (goals, quality attributes, constraints, expected changes) + partner mode.
import { useRef, useState, type ReactNode } from "react";
import { api } from "../state/api";
import type { Design, Force, ForceKind, PartnerMode } from "../state/types";

const KINDS: { kind: ForceKind; title: string; hint: string }[] = [
  { kind: "goal", title: "Goals", hint: "what the system must achieve" },
  { kind: "quality", title: "Quality attributes", hint: "e.g. evolvability, latency, testability" },
  { kind: "constraint", title: "Constraints", hint: "technology, team, budget, regulation" },
  { kind: "change", title: "Expected changes", hint: "what will change — the design should absorb it" },
];

export const MODES: { mode: PartnerMode; label: string; help: string }[] = [
  { mode: "examiner", label: "examiner", help: "raises decisions; you choose and defend" },
  { mode: "socratic", label: "socratic", help: "only asks questions; you make every change" },
  { mode: "devils_advocate", label: "devil's advocate", help: "argues against what you lean towards" },
  { mode: "sketch", label: "sketch", help: "proposes freely; decisions only for big forks" },
];

const PRIO = ["", "must", "should", "nice"];

interface Props {
  design: Design;
  onClose: () => void;
  run: (p: Promise<unknown>) => Promise<boolean>;
  /** extra sections (rules) */
  children?: ReactNode;
}

export function BriefPanel({ design, onClose, run, children }: Props) {
  const forces = design.forces;
  return (
    <aside className="brief-panel" data-ui aria-labelledby="brief-title">
      <div className="bp-head">
        <div><span className="dp-kicker">Design context</span><h2 id="brief-title">Brief &amp; rules</h2></div>
        <button className="icon-button" aria-label="Close brief" onClick={onClose} title="Close · ⌘B">×</button>
      </div>
      <div className="bp-body">
        <div className="bp-sec">
          <div className="hc-h">Partner mode</div>
          <div className="bp-modes" role="group" aria-label="Partner mode">
            {MODES.map((m) => (
              <button key={m.mode} aria-pressed={design.settings.partner === m.mode} className={design.settings.partner === m.mode ? "on" : ""} title={m.help}
                onClick={() => run(api.setPartner(m.mode))}>{m.label}</button>
            ))}
          </div>
          <div className="faint small">{MODES.find((m) => m.mode === design.settings.partner)?.help}</div>
        </div>
        {KINDS.map((k) => (
          <ForceGroup key={k.kind} kind={k.kind} title={k.title} hint={k.hint} forces={forces.filter((f) => f.kind === k.kind)} run={run} />
        ))}
        {forces.length === 0 && !design.settings.brief_skipped && (
          <div className="faint small bp-note">
            The agent asks for the brief before designing. Answer in pi, add forces here, or type <span className="mono">/skip</span>.
          </div>
        )}
        {children}
      </div>
    </aside>
  );
}

function ForceGroup({ kind, title, hint, forces, run }: { kind: ForceKind; title: string; hint: string; forces: Force[]; run: Props["run"] }) {
  const [text, setText] = useState("");
  const [pending, setPending] = useState(false);
  const [error, setError] = useState("");
  const add = async () => {
    const name = text.trim();
    if (!name) return setError("Enter a short description first.");
    if (forces.some((f) => f.name.toLowerCase() === name.toLowerCase())) return setError("This item is already in the brief.");
    setPending(true); setError("");
    if (await run(api.userOps([{ op: "add_force", name, kind }]))) setText("");
    else setError("Not saved. Your text is kept so you can try again.");
    setPending(false);
  };
  const id = `force-${kind}`;
  return (
    <div className="bp-sec">
      <label className="hc-h" htmlFor={id}>{title}</label>
      {forces.length === 0 && <div className="empty-line">Nothing yet</div>}
      {forces.map((f) => <ForceRow key={f.name} f={f} run={run} />)}
      <form className="inline-form" onSubmit={(e) => { e.preventDefault(); void add(); }}>
        <input id={id} className="bp-add" value={text} disabled={pending} placeholder={hint} aria-invalid={!!error} onChange={(e) => { setText(e.target.value); setError(""); }} />
        <button type="submit" disabled={pending || !text.trim()}>{pending ? "Adding…" : "Add"}</button>
      </form>
      {error && <div className="form-error" role="alert">{error}</div>}
    </div>
  );
}

function ForceRow({ f, run }: { f: Force; run: Props["run"] }) {
  const [editing, setEditing] = useState(false);
  const [name, setName] = useState(f.name);
  const [desc, setDesc] = useState(f.description ?? "");
  const saving = useRef(false);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState("");
  const ghost = f.status === "proposed" || f.status === "removing";
  if (ghost) {
    return (
      <div className={`force ghost ${f.status}`} onContextMenu={(e) => { e.preventDefault(); f.proposal && run(api.reject(f.proposal)); }}>
        <span className="prio">{PRIO[f.priority]}</span>
        <span className="fname"><span className="proposal-label">{f.status === "removing" ? "Proposed removal" : "Proposed"}</span>{f.name}</span>
        {f.proposal && <span className="ghost-actions"><button onClick={() => run(api.accept(f.proposal!))}>Accept</button><button onClick={() => run(api.reject(f.proposal!))}>Reject</button></span>}
        {f.description && <div className="fdesc">{f.description}</div>}
      </div>
    );
  }
  const save = async () => {
    if (saving.current) return;
    const n = name.trim();
    if (!n) return setError("Name is required.");
    const changes: Record<string, unknown> = {};
    if (n !== f.name) changes.rename = n;
    if (desc.trim() !== (f.description ?? "")) changes.description = desc.trim();
    if (!Object.keys(changes).length) return setEditing(false);
    saving.current = true; setPending(true); setError("");
    if (await run(api.userOps([{ op: "update_force", name: f.name, ...changes }]))) setEditing(false);
    else setError("Not saved. Your changes are kept.");
    saving.current = false; setPending(false);
  };
  const cancel = () => { setName(f.name); setDesc(f.description ?? ""); setError(""); setEditing(false); };
  if (editing) {
    return (
      <form className="force editing" onSubmit={(e) => { e.preventDefault(); void save(); }} onKeyDown={(e) => { if (e.key === "Escape") { e.stopPropagation(); cancel(); } }}>
        <label className="sr-only" htmlFor={`fn-${f.name}`}>Name</label>
        <input id={`fn-${f.name}`} value={name} autoFocus disabled={pending} onChange={(e) => setName(e.target.value)} />
        <label className="sr-only" htmlFor={`fd-${f.name}`}>Description</label>
        <input id={`fd-${f.name}`} value={desc} disabled={pending} placeholder="Description (optional)" onChange={(e) => setDesc(e.target.value)} />
        {error && <div className="form-error" role="alert">{error}</div>}
        <div className="form-actions"><button type="button" onClick={cancel} disabled={pending}>Cancel</button><button className="primary" type="submit" disabled={pending}>{pending ? "Saving…" : "Save"}</button></div>
      </form>
    );
  }
  return (
    <div className="force">
      <button className={`prio p${f.priority}`} title="Priority — click to cycle" aria-label={`Priority ${PRIO[f.priority]}. Change priority`}
        onClick={() => run(api.userOps([{ op: "update_force", name: f.name, priority: (f.priority % 3) + 1 }]))}>{PRIO[f.priority]}</button>
      <span className="fname">{f.name}</span>
      <button className="fx" aria-label={`Edit ${f.name}`} title="Edit" onClick={() => setEditing(true)}>✎</button>
      <button className="fx" aria-label={`Remove ${f.name}`} title="Remove (⌘Z undoes)" onClick={() => run(api.userOps([{ op: "remove_force", name: f.name }]))}>✕</button>
      {f.description && <div className="fdesc">{f.description}</div>}
    </div>
  );
}
