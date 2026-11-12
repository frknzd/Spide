// Change scenarios ("what if …?"): footprint, locality, stress test.
import { useState } from "react";
import { api } from "../state/api";
import type { Design, Metrics } from "../state/types";

interface Props {
  design: Design;
  metrics: Metrics;
  stress: string | null;
  onStress: (name: string | null) => void;
  run: (p: Promise<unknown>) => void;
}

export function ChangeSection({ design, metrics, stress, onStress, run }: Props) {
  const [text, setText] = useState("");
  const names = new Set(design.change_scenarios.map((c) => c.name));
  // expected changes from the brief that have no scenario yet
  const seeds = design.forces.filter((f) => f.kind === "change" && f.status !== "proposed" && !names.has(f.name));
  const add = () => {
    const name = text.trim();
    if (!name) return;
    run(api.userOps([{ op: "add_change_scenario", name }]).then(() => api.analyseChange(name)));
    setText("");
  };
  return (
    <div className="bp-sec">
      <div className="hc-h">change scenarios — what if…?</div>
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
                  <button className="link" onClick={() => onStress(stress === c.name ? null : c.name)}>{stress === c.name ? "stop" : "stress test"}</button>
                </>
              ) : (
                <span className="faint">not analysed yet</span>
              )}
              <button className="link" onClick={() => run(api.analyseChange(c.name))}>{c.analysed ? "re-analyse" : "analyse"}</button>
              <button className="fx" title="remove" onClick={() => run(api.userOps([{ op: "remove_change_scenario", name: c.name }]))}>✕</button>
            </div>
          </div>
        );
      })}
      {seeds.map((f) => (
        <div key={f.name} className="change seed">
          <span className="fname">{f.name}</span>
          <button className="link" onClick={() => run(api.userOps([{ op: "add_change_scenario", name: f.name, description: f.description }]).then(() => api.analyseChange(f.name)))}>analyse</button>
        </div>
      ))}
      <input className="bp-add" value={text} placeholder="+ what if we must support …?" onChange={(e) => setText(e.target.value)} onKeyDown={(e) => e.key === "Enter" && add()} />
    </div>
  );
}
