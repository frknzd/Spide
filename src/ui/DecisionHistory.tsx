// Read-only record of a closed decision: question, options, defences, examiner verdicts.
import type { Design } from "../state/types";

export function DecisionHistory({ design, id, onClose }: { design: Design; id: string; onClose: () => void }) {
  const d = design.decisions.find((x) => x.id === id);
  if (!d) return null;
  const elements = [
    ...design.classes.filter((c) => c.origin === id).map((c) => c.name),
    ...design.classes.flatMap((c) => [...c.attributes, ...c.methods].filter((m) => m.origin === id && c.origin !== id).map((m) => `${c.name}.${m.name}`)),
    ...design.relations.filter((r) => r.origin === id).map((r) => `${r.from} → ${r.to}`),
  ];
  return (
    <div className="decision-panel history-panel" data-ui>
      <div className="dp-head">
        <span className="dp-kicker">decision {d.id} · {d.state}</span>
        <button className="link" onClick={onClose}>close</button>
      </div>
      <div className="decision">
        <div className="dq">{d.question}</div>
        {d.context && <div className="dctx">{d.context}</div>}
        <div className="options">
          {d.options.map((o) => (
            <div key={o.id} className={`option ${d.chosen === o.id ? "sel" : ""}`}>
              <div className="o-head"><span className="o-id">{o.id}</span><span className="o-label">{o.label}</span>{d.chosen === o.id && <span className="faint small">chosen</span>}</div>
              {o.summary && <div className="o-sum">{o.summary}</div>}
              {o.tradeoffs.length > 0 && (
                <div className="tradeoffs">
                  {o.tradeoffs.map((t, i) => <span key={i} className={`tchip ${t.effect.trim().startsWith("-") ? "minus" : "plus"}`} title={t.note ?? ""}>{t.effect.trim().startsWith("-") ? "−" : "+"} {t.force}</span>)}
                </div>
              )}
            </div>
          ))}
          {d.custom && <div className="option sel"><div className="o-head"><span className="o-id">✎</span><span className="o-label">{d.custom}</span></div></div>}
        </div>
        {d.attempts.map((a, i) => (
          <div key={i} className="attempt">
            <div className="hc-h">defence {i + 1}</div>
            <div>“{a.defense}”</div>
            {a.feedback && <div className={`verdict ${a.verdict === "accept" ? "accept" : "reject"}`}><b>Examiner ({a.verdict ?? "—"}):</b> {a.feedback}</div>}
          </div>
        ))}
        {elements.length > 0 && (
          <div className="attempt">
            <div className="hc-h">introduced</div>
            <div className="mono small">{elements.join(", ")}</div>
          </div>
        )}
      </div>
    </div>
  );
}
