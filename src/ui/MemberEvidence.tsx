import { useState } from "react";
import type { Design, Method, Metrics } from "../state/types";

/** Facts are deliberately separate from issue judgements. No source facts means unknown, not zero. */
export function MemberEvidence({ member, qualified, design, metrics }: { member: Method; qualified: string; design: Design; metrics: Metrics }) {
  const [selected, setSelected] = useState<string | null>(null);
  const pairs = (metrics.overlap?.pairs ?? []).filter((p) => p.a === qualified || p.b === qualified).sort((a, b) => b.score - a.score).slice(0, 5);
  const find = (key: string) => design.classes.flatMap((c) => c.methods.map((m) => ({ key: `${c.name}.${m.name}`, m }))).filter((x) => x.key === key);
  const matches = selected ? find(selected) : [];
  const other = matches.length === 1 ? matches[0].m : null;
  return <details className="hc-sec">
    <summary>Code facts & similarity</summary>
    <Facts member={member} />
    <div className="faint small">Similarity covers the loaded/imported model; it is not the probability of a defect.</div>
    {pairs.length === 0 && <div className="faint">No reported similarity matches.</div>}
    {pairs.map((p) => {
      const key = p.a === qualified ? p.b : p.a;
      const candidates = find(key);
      const peer = candidates.length === 1 ? candidates[0].m : null;
      const codeAvailable = (member.code?.fingerprint.length ?? 0) >= 6 && (peer?.code?.fingerprint.length ?? 0) >= 6;
      const percent = (v: number) => `${Math.round(v * 100)}%`;
      return <div className="hc-sec" key={key}>
        <button className="ghostbtn" onClick={() => setSelected(key)} disabled={!peer} aria-label={`Inspect ${key}`}>{key}()</button>
        <div>Implementation fingerprints: {codeAvailable ? percent(p.code) : "unavailable"}</div>
        <div className="faint small">Name {percent(p.name)} · signature {percent(p.signature)} · data/calls {percent(p.touches)} · description {percent(p.text)}</div>
      </div>;
    })}
    {other && <section aria-label={`Related member ${selected}`}><b>{selected}()</b><Facts member={other} /><button className="ghostbtn" onClick={() => setSelected(null)}>Back to matches</button></section>}
  </details>;
}

function Facts({ member }: { member: Method }) {
  const c = member.code;
  if (!c) return <div className="faint">Imported code facts unavailable.</div>;
  return <div className="hc-sec">
    <div>{c.loc} code lines (excluding blanks/comments) · {c.statements} statements</div>
    <div>{plural(c.loops, "loop")} · {plural(c.switches, "switch", "switches")} · longest receiver chain {c.max_chain}</div>
    <div className="mono small">{c.file}:{c.start_line}–{c.end_line}</div>
  </div>;
}

const plural = (n: number, one: string, many = one + "s") => `${n} ${n === 1 ? one : many}`;
