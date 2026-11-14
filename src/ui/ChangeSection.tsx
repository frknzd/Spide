// Change scenarios ("what if …?"): footprint, locality, stress test.
import { useState } from "react";
import { api } from "../state/api";
import type { Design, Metrics } from "../state/types";

interface Props {
  design: Design;
  metrics: Metrics;
  stress: string | null;
  onStress: (name: string | null) => void;
  run: (p: Promise<unknown>) => Promise<boolean>;
}

export function ChangeSection({ design, metrics, stress, onStress, run }: Props) {
  const [text, setText] = useState("");
  const [pending, setPending] = useState(false);
  const [error, setError] = useState("");
  const names = new Set(design.change_scenarios.map((c) => c.name));
  // expected changes from the brief that have no scenario yet
  const seeds = design.forces.filter((f) => f.kind === "change" && f.status !== "proposed" && !names.has(f.name));
  const add = async () => {
    const name = text.trim();
    if (!name) return setError("Describe a change first.");
    if (names.has(name)) return setError("This scenario already exists. Use Analyse on it below.");
    setPending(true); setError("");
    if (await run(api.userOps([{ op: "add_change_scenario", name }]))) {
      setText("");
      if (!(await run(api.analyseChange(name)))) setError("Scenario saved, but analysis couldn't start. Use Analyse to retry.");
    } else setError("Not saved. Your scenario is kept.");
    setPending(false);
  };
  return (
    <div className="bp-sec">
      <label className="hc-h" htmlFor="change-scenario">Change scenarios · what if…?</label>
      {metrics.changes.length === 0 && seeds.length === 0 && <div className="empty-line">Add a likely change to see which classes it touches.</div>}
      {metrics.changes.map((c) => {
        const cs = design.change_scenarios.find((x) => x.name === c.name);
        const pct = Math.round(c.locality * 100);
        return (
          <div key={c.name} className={`change ${stress === c.name ? "on" : ""}`}>
            <div className="ch-head">
              <span className="fname">{c.name}</span>
              {c.analysed && <span className={`loc ${pct < 50 ? "bad" : pct < 75 ? "mid" : "good"}`} title="locality: share of classes untouched">{pct}%</span>}
              {c.stale && <span className="stale" title="class model changed since the analysis" />}
            </div>
            {cs?.description && <div className="fdesc">{cs.description}</div>}
            <div className="ch-meta">
              {c.analysed ? (
                <>
                  <span>{c.impacted.length} impacted · {c.ripple.length} ripple{c.packages.length > 0 ? ` · ${c.packages.length} package${c.packages.length > 1 ? "s" : ""}` : ""}</span>
                  <button className="link" onClick={() => onStress(stress === c.name ? null : c.name)}>{stress === c.name ? "Stop stress test" : "Stress test"}</button>
                </>
              ) : (
                <span className="faint">not analysed yet</span>
              )}
              <button className="link" onClick={() => run(api.analyseChange(c.name))}>{c.analysed ? "Re-analyse" : "Analyse"}</button>
              <button className="fx" aria-label={`Remove ${c.name}`} title="Remove (⌘Z undoes)" onClick={() => run(api.userOps([{ op: "remove_change_scenario", name: c.name }]))}>✕</button>
            </div>
          </div>
        );
      })}
      {seeds.map((f) => (
        <div key={f.name} className="change seed">
          <span className="fname">{f.name}</span>
          <button className="link" onClick={async () => { if (await run(api.userOps([{ op: "add_change_scenario", name: f.name, description: f.description }]))) void run(api.analyseChange(f.name)); }}>Analyse</button>
        </div>
      ))}
      <form className="inline-form" onSubmit={(e) => { e.preventDefault(); void add(); }}>
        <input id="change-scenario" className="bp-add" value={text} disabled={pending} aria-invalid={!!error} placeholder="What if we must support…?" onChange={(e) => { setText(e.target.value); setError(""); }} />
        <button type="submit" disabled={pending || !text.trim()}>{pending ? "Adding…" : "Add"}</button>
      </form>
      {error && <div className="form-error" role="alert">{error}</div>}
    </div>
  );
}
