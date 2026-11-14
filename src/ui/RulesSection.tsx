// Architecture rules: style preset, custom forbidden dependencies / types, thresholds.
import { useState } from "react";
import { api } from "../state/api";
import type { Layer, Preset, Rules, Thresholds } from "../state/types";

export const PRESETS: { preset: Preset; label: string; help: string }[] = [
  { preset: "clean", label: "clean", help: "ui/infrastructure → application → domain; dependencies point inward" },
  { preset: "hexagonal", label: "hexagonal", help: "adapters → ports → domain; adapters never depend on each other" },
  { preset: "layered", label: "layered", help: "ui → application → domain → infrastructure; only downward" },
  { preset: "modular_monolith", label: "modular monolith", help: "packages <module>.<layer>; other modules only via <module>.api" },
  { preset: "none", label: "none", help: "no layering rules" },
];

const LAYERS: Layer[] = ["ui", "application", "domain", "infrastructure"];
const TH: { key: keyof Thresholds; label: string }[] = [
  { key: "god_members", label: "god class: members >" },
  { key: "god_responsibilities", label: "god class: responsibilities >" },
  { key: "high_fan_out", label: "high coupling: fan-out ≥" },
  { key: "max_scenario_hops", label: "chatty scenario: hops >" },
];

export function RulesSection({ rules, run }: { rules: Rules; run: (p: Promise<unknown>) => Promise<boolean> }) {
  const set = (r: Partial<Rules>) => run(api.setRules({ ...rules, ...r }));
  const [dep, setDep] = useState("");
  const [ty, setTy] = useState("");
  const [tyLayer, setTyLayer] = useState<Layer>("domain");
  const [depError, setDepError] = useState("");
  const [tyError, setTyError] = useState("");
  const [pending, setPending] = useState<string | null>(null);
  const [th, setTh] = useState<Record<string, string>>({});

  const addDep = async () => {
    // "from -> to : reason"
    const m = dep.match(/^\s*(\S+)\s*(?:->|→)\s*(\S+)\s*(?::\s*(.*))?$/);
    if (!m) return setDepError("Use the form From -> To : reason.");
    setPending("dep");
    if (await set({ forbidden: [...rules.forbidden, { from: m[1], to: m[2], reason: m[3]?.trim() || null }] })) setDep("");
    else setDepError("Not saved. Your rule is kept.");
    setPending(null);
  };
  const addType = async () => {
    const p = ty.trim();
    if (!p) return setTyError("Enter a type pattern, for example Http*.");
    setPending("type");
    if (await set({ forbidden_types: [...rules.forbidden_types, { layer: tyLayer, pattern: p, reason: null }] })) setTy("");
    else setTyError("Not saved. Your pattern is kept.");
    setPending(null);
  };
  const commitThreshold = (key: keyof Thresholds) => {
    const raw = th[key];
    if (raw === undefined) return;
    const v = Number(raw);
    if (!Number.isInteger(v) || v < 1 || v > 99) return;
    setTh(({ [key]: _, ...rest }) => rest);
    if (v !== rules.thresholds[key]) void set({ thresholds: { ...rules.thresholds, [key]: v } });
  };

  return (
    <>
      <div className="bp-sec">
        <div className="hc-h">Architecture style</div>
        <div className="bp-modes" role="group" aria-label="Architecture style">
          {PRESETS.map((p) => (
            <button key={p.preset} aria-pressed={rules.preset === p.preset} className={rules.preset === p.preset ? "on" : ""} title={p.help} onClick={() => set({ preset: p.preset })}>{p.label}</button>
          ))}
        </div>
        <div className="faint small">{PRESETS.find((p) => p.preset === rules.preset)?.help}</div>
      </div>
      <div className="bp-sec">
        <label className="hc-h" htmlFor="rule-dependency">Forbidden dependencies</label>
        {rules.forbidden.map((r, i) => (
          <div key={i} className="rule-row">
            <span className="mono">{r.from} ↛ {r.to}</span>
            {r.reason && <span className="faint"> — {r.reason}</span>}
            <button className="fx" aria-label={`Remove rule ${r.from} to ${r.to}`} title="Remove" onClick={() => set({ forbidden: rules.forbidden.filter((_, k) => k !== i) })}>✕</button>
          </div>
        ))}
        <form className="inline-form" onSubmit={(e) => { e.preventDefault(); void addDep(); }}>
          <input id="rule-dependency" className="bp-add" value={dep} disabled={pending === "dep"} aria-invalid={!!depError} placeholder="*Controller -> *Repository : use a service" onChange={(e) => { setDep(e.target.value); setDepError(""); }} />
          <button type="submit" disabled={pending === "dep" || !dep.trim()}>Add</button>
        </form>
        {depError && <div className="form-error" role="alert">{depError}</div>}
      </div>
      <div className="bp-sec">
        <label className="hc-h" htmlFor="rule-type">Forbidden types per layer</label>
        {rules.forbidden_types.map((r, i) => (
          <div key={i} className="rule-row">
            <span className="mono">no {r.pattern} in {r.layer}</span>
            <button className="fx" aria-label={`Remove ${r.pattern} rule`} title="Remove" onClick={() => set({ forbidden_types: rules.forbidden_types.filter((_, k) => k !== i) })}>✕</button>
          </div>
        ))}
        <form className="rule-add inline-form" onSubmit={(e) => { e.preventDefault(); void addType(); }}>
          <select aria-label="Layer" value={tyLayer} onChange={(e) => setTyLayer(e.target.value as Layer)}>
            {LAYERS.map((l) => <option key={l} value={l}>{l}</option>)}
          </select>
          <input id="rule-type" className="bp-add" value={ty} disabled={pending === "type"} aria-invalid={!!tyError} placeholder="Http*" onChange={(e) => { setTy(e.target.value); setTyError(""); }} />
          <button type="submit" disabled={pending === "type" || !ty.trim()}>Add</button>
        </form>
        {tyError && <div className="form-error" role="alert">{tyError}</div>}
      </div>
      <div className="bp-sec">
        <div className="hc-h">Thresholds</div>
        {TH.map((t) => (
          <label key={t.key} className="th-row">
            <span>{t.label}</span>
            <input type="number" min={1} max={99} step={1} value={th[t.key] ?? rules.thresholds[t.key]} aria-invalid={th[t.key] !== undefined && !/^([1-9]|[1-9][0-9])$/.test(th[t.key])}
              onChange={(e) => setTh((x) => ({ ...x, [t.key]: e.target.value }))} onBlur={() => commitThreshold(t.key)} onKeyDown={(e) => { if (e.key === "Enter") commitThreshold(t.key); if (e.key === "Escape") setTh(({ [t.key]: _, ...rest }) => rest); }} />
          </label>
        ))}
      </div>
    </>
  );
}
