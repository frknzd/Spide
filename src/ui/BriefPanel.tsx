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
  run: (p: Promise<unknown>) => void;
  /** extra sections (rules) */
  children?: ReactNode;
}

export function BriefPanel({ design, onClose, run, children }: Props) {
  const forces = design.forces;
  return (
    <div className="brief-panel" data-ui>
      <div className="bp-head">
        <span className="dp-kicker">brief</span>
        <button className="link" onClick={onClose} title="⌘B">close</button>
      </div>
      <div className="bp-body">
        <div className="bp-sec">
          <div className="hc-h">partner mode</div>
          <div className="bp-modes">
            {MODES.map((m) => (
              <button key={m.mode} className={design.settings.partner === m.mode ? "on" : ""} title={m.help}
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
            The agent asks for the brief before designing. Answer in the prompt, add forces here, or type <span className="mono">/skip</span>.
          </div>
        )}
        {children}
      </div>
    </div>
  );
}

function ForceGroup({ kind, title, hint, forces, run }: { kind: ForceKind; title: string; hint: string; forces: Force[]; run: Props["run"] }) {
  const [text, setText] = useState("");
  const add = () => {
    const name = text.trim();
    if (!name) return;
    run(api.userOps([{ op: "add_force", name, kind }]));
    setText("");
  };
  return (
    <div className="bp-sec">
      <div className="hc-h">{title}</div>
      {forces.map((f) => <ForceRow key={f.name} f={f} run={run} />)}
      <input className="bp-add" value={text} placeholder={`+ ${hint}`} onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => e.key === "Enter" && add()} />
    </div>
  );
}

function ForceRow({ f, run }: { f: Force; run: Props["run"] }) {
  const [editing, setEditing] = useState(false);
  const [name, setName] = useState(f.name);
  const [desc, setDesc] = useState(f.description ?? "");
  const saving = useRef(false);
  const ghost = f.status === "proposed" || f.status === "removing";
  if (ghost) {
    return (
      <div className={`force ghost ${f.status}`} title="proposed by the agent — click to accept, right-click to reject"
        onClick={() => f.proposal && run(api.accept(f.proposal))}
        onContextMenu={(e) => { e.preventDefault(); f.proposal && run(api.reject(f.proposal)); }}>
        <span className="prio">{PRIO[f.priority]}</span>
        <span className="fname">{f.status === "removing" ? "− " : "+ "}{f.name}</span>
        {f.description && <div className="fdesc">{f.description}</div>}
      </div>
    );
  }
  const save = () => {
    if (saving.current) return; // Enter + blur both fire
    saving.current = true;
    setTimeout(() => (saving.current = false), 0);
    setEditing(false);
    const n = name.trim();
    const changes: Record<string, unknown> = {};
    if (n && n !== f.name) changes.rename = n;
    if (desc.trim() !== (f.description ?? "")) changes.description = desc.trim();
    if (Object.keys(changes).length) run(api.userOps([{ op: "update_force", name: f.name, ...changes }]));
  };
  if (editing) {
    return (
      <div className="force editing" onKeyDown={(e) => { if (e.key === "Enter") save(); if (e.key === "Escape") setEditing(false); }}>
        <input value={name} autoFocus onChange={(e) => setName(e.target.value)} />
        <input value={desc} placeholder="description (optional)" onChange={(e) => setDesc(e.target.value)} onBlur={save} />
      </div>
    );
  }
  return (
    <div className="force">
      <button className={`prio p${f.priority}`} title="priority — click to cycle"
        onClick={() => run(api.userOps([{ op: "update_force", name: f.name, priority: (f.priority % 3) + 1 }]))}>{PRIO[f.priority]}</button>
      <span className="fname" onDoubleClick={() => setEditing(true)} title="double-click to edit">{f.name}</span>
      <button className="fx" title="remove" onClick={() => run(api.userOps([{ op: "remove_force", name: f.name }]))}>✕</button>
      {f.description && <div className="fdesc" onDoubleClick={() => setEditing(true)}>{f.description}</div>}
    </div>
  );
}
