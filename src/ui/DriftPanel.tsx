// Design ↔ code drift report.
import { api } from "../state/api";
import type { DriftReport } from "../state/types";

interface Props {
  report: DriftReport;
  onClose: () => void;
  onReimport: () => void;
  onFind: (cls: string) => void;
  run: (p: Promise<unknown>) => void;
  toast: (kind: string, text: string) => void;
}

export function DriftPanel({ report, onClose, onReimport, onFind, run, toast }: Props) {
  const d = report.drift;
  const pct = Math.round(d.conformance * 100);
  const cls = (n: string) => <button key={n} className="link mono" onClick={() => onFind(n)}>{n}</button>;
  const sec = (title: string, n: number, body: React.ReactNode) =>
    n > 0 && (
      <div className="bp-sec">
        <div className="hc-h">{title} · {n}</div>
        {body}
      </div>
    );
  const clean = d.missing_in_code.length + d.extra_in_code.length + d.members.length + d.relations_missing_in_code.length +
    d.relations_extra_in_code.length + d.packages.length + d.rule_violations_in_code.length === 0;
  return (
    <div className="brief-panel drift-panel" data-ui>
      <div className="bp-head">
        <span className="dp-kicker">design ↔ code</span>
        <span className={`loc ${pct < 60 ? "bad" : pct < 85 ? "mid" : "good"}`} title="share of the design found in the code">{pct}% conformant</span>
        <button className="link" onClick={onClose}>close</button>
      </div>
      <div className="bp-body">
        {report.code_dir && <div className="faint small mono" title={report.code_dir}>{report.code_dir.split("/").slice(-3).join("/")} · <button className="link" onClick={onReimport}>re-import</button></div>}
        {clean && <div className="faint">The code matches the design.</div>}
        {sec("designed, not in code", d.missing_in_code.length, <div className="drift-list">{d.missing_in_code.map(cls)}</div>)}
        {sec("only in code", d.extra_in_code.length, <div className="drift-list">{d.extra_in_code.map(cls)}</div>)}
        {sec("members differ", d.members.length, d.members.map((m) => (
          <div key={m.class} className="drift-row">
            {cls(m.class)}
            {m.missing_in_code.length > 0 && <div className="im-rem small">missing: {m.missing_in_code.join(", ")}</div>}
            {m.extra_in_code.length > 0 && <div className="faint small">extra: {m.extra_in_code.join(", ")}</div>}
          </div>
        )))}
        {sec("relations missing in code", d.relations_missing_in_code.length, d.relations_missing_in_code.map((r, i) => <div key={i} className="mono small">{r.from} → {r.to} ({r.kind})</div>))}
        {sec("relations only in code", d.relations_extra_in_code.length, d.relations_extra_in_code.map((r, i) => <div key={i} className="mono small faint">{r.from} → {r.to} ({r.kind})</div>))}
        {sec("in another package", d.packages.length, d.packages.map((p) => <div key={p.class} className="small">{cls(p.class)} design <b>{p.design ?? "—"}</b>, code <b>{p.code ?? "—"}</b></div>))}
        {sec("rules broken in code", d.rule_violations_in_code.length, d.rule_violations_in_code.map((v, i) => <div key={i} className="im-bad small">{v}</div>))}
        <div className="d-actions">
          <button className="primary" disabled={d.extra_in_code.length + d.members.length + d.relations_extra_in_code.length === 0}
            onClick={() => run(api.driftAdopt().then(() => toast("info", "code-only parts proposed as ghosts")))}>adopt code into design</button>
          <button className="link" onClick={() => navigator.clipboard.writeText(report.checklist).then(() => toast("info", "checklist copied (markdown)"), (e) => toast("error", String(e)))}>
            copy code to-do list
          </button>
        </div>
      </div>
    </div>
  );
}
