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

export function RulesSection({ rules, run }: { rules: Rules; run: (p: Promise<unknown>) => void }) {
  const set = (r: Partial<Rules>) => run(api.setRules({ ...rules, ...r }));
  const [dep, setDep] = useState("");
  const [ty, setTy] = useState("");
  const [tyLayer, setTyLayer] = useState<Layer>("domain");

  const addDep = () => {
    // "from -> to : reason"
    const m = dep.match(/^\s*(\S+)\s*(?:->|→)\s*(\S+)\s*(?::\s*(.*))?$/);
    if (!m) return;
    set({ forbidden: [...rules.forbidden, { from: m[1], to: m[2], reason: m[3]?.trim() || null }] });
    setDep("");
  };
  const addType = () => {
    const p = ty.trim();
    if (!p) return;
    set({ forbidden_types: [...rules.forbidden_types, { layer: tyLayer, pattern: p, reason: null }] });
    setTy("");
  };

  return (
    <>
      <div className="bp-sec">
        <div className="hc-h">architecture style</div>
        <div className="bp-modes">
          {PRESETS.map((p) => (
            <button key={p.preset} className={rules.preset === p.preset ? "on" : ""} title={p.help} onClick={() => set({ preset: p.preset })}>{p.label}</button>
          ))}
        </div>
        <div className="faint small">{PRESETS.find((p) => p.preset === rules.preset)?.help}</div>
      </div>
      <div className="bp-sec">
        <div className="hc-h">forbidden dependencies</div>
        {rules.forbidden.map((r, i) => (
          <div key={i} className="rule-row">
            <span className="mono">{r.from} ↛ {r.to}</span>
            {r.reason && <span className="faint"> — {r.reason}</span>}
            <button className="fx" title="remove" onClick={() => set({ forbidden: rules.forbidden.filter((_, k) => k !== i) })}>✕</button>
          </div>
        ))}
        <input className="bp-add" value={dep} placeholder="+ *Controller -> *Repository : go via a service" onChange={(e) => setDep(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && addDep()} />
      </div>
      <div className="bp-sec">
        <div className="hc-h">forbidden types per layer</div>
        {rules.forbidden_types.map((r, i) => (
          <div key={i} className="rule-row">
            <span className="mono">no {r.pattern} in {r.layer}</span>
            <button className="fx" title="remove" onClick={() => set({ forbidden_types: rules.forbidden_types.filter((_, k) => k !== i) })}>✕</button>
          </div>
        ))}
        <div className="rule-add">
          <select value={tyLayer} onChange={(e) => setTyLayer(e.target.value as Layer)}>
            {LAYERS.map((l) => <option key={l} value={l}>{l}</option>)}
          </select>
          <input className="bp-add" value={ty} placeholder="+ Http*  (glob)" onChange={(e) => setTy(e.target.value)} onKeyDown={(e) => e.key === "Enter" && addType()} />
        </div>
      </div>
      <div className="bp-sec">
        <div className="hc-h">thresholds</div>
        {TH.map((t) => (
          <label key={t.key} className="th-row">
            <span>{t.label}</span>
            <input type="number" min={1} max={99} value={rules.thresholds[t.key]}
              onChange={(e) => {
                const v = Math.max(1, Math.min(99, Number(e.target.value) || 1));
                set({ thresholds: { ...rules.thresholds, [t.key]: v } });
              }} />
          </label>
        ))}
      </div>
    </>
  );
}
