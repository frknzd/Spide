import type { ReactNode } from "react";
import { attrText, healthColor, heatColor, methodText } from "../canvas/visuals";
import type { Design, DriftReport, Metrics, Snapshot } from "../state/types";
import { CONCERN_COLOR, methodMarks } from "../state/insights";

interface Props {
  hoverKey: string;
  at: { x: number; y: number };
  snap: Snapshot;
  design: Design;
  metrics: Metrics;
  drift?: DriftReport | null;
}

/** "What is it for?": the agent's purpose, the role Spide reads and why, misplaced methods → home, what the code says. */
function purposeSection(name: string, design: Design, metrics: Metrics, drift?: DriftReport | null): ReactNode {
  const cr = metrics.responsibility?.classes[name];
  const role = cr?.role;
  const a = design.assessments?.[name];
  const marks = methodMarks(metrics).get(name);
  const code = drift?.findings?.disagreements.filter((d) => d.class === name) ?? [];
  if (!role && !a && !marks && !code.length) return null;
  const heuristic = role?.by_name ?? role?.by_stereotype ?? role?.by_layer ?? null;
  const agentNote = a?.role ? (heuristic && a.role !== heuristic ? `agent says ${a.role}, names say ${heuristic}` : "agent agrees") : null;
  const sourceText: Record<string, string> = { explicit: "set explicitly", agent: "from the agent's assessment", name: "from its name", stereotype: "from its stereotype", layer: "from its layer", default: "no signal, assumed" };
  return (
    <div className="hc-sec hc-purpose">
      <div className="hc-h">purpose</div>
      {a?.purpose && <div>{a.purpose}</div>}
      {role && (
        <div className="faint">
          <b style={{ color: "var(--text)" }}>{role.role}</b> · {sourceText[role.source] ?? role.source}{role.source !== "agent" ? ` (${role.evidence})` : ""}{agentNote ? ` · ${agentNote}` : ""}
          {cr && Object.keys(cr.concerns).length > 1 && <> · does {Object.entries(cr.concerns).map(([k, n]) => `${k.replace("_", " ")} ${n}`).join(", ")}</>}
        </div>
      )}
      {marks && [...marks.entries()].map(([m, x]) => (
        <div key={m} className="hc-home">
          <span className="mono">{m}()</span> <span className="faint">{x.kind.replace(/_/g, " ")}</span>{x.home && <> → <b>{x.home}</b></>}
        </div>
      ))}
      {code.map((d, i) => <div key={i} className="hc-code">code says: {d.code}</div>)}
      {drift && <div className="faint small">/code {name} shows the implementation</div>}
    </div>
  );
}

const KIND_TEXT: Record<string, string> = {
  association: "association",
  aggregation: "aggregation (whole ◇— part)",
  composition: "composition (whole ◆— part, lifecycle bound)",
  inheritance: "inheritance (child ▷ parent)",
  realization: "realization (implements interface)",
  dependency: "dependency (uses)",
};

function proposalNote(design: Design, id: string | null): ReactNode {
  if (!id) return null;
  const p = design.proposals.find((x) => x.id === id);
  return (
    <div className="hc-ghost">
      <b>proposed</b> {p?.source === "file" ? "via design.json edit" : "by the agent"}
      {p?.rationale && <div>{p.rationale}</div>}
      <div className="faint">click to accept · right-click to reject</div>
    </div>
  );
}

/** "Why is this here?" — the decision, proposal or person that introduced an element. */
export function originNote(design: Design, origin: string | null | undefined): ReactNode {
  if (!origin) return null;
  if (origin === "user") return <div className="hc-origin">added by you</div>;
  if (origin === "import") return <div className="hc-origin">imported from code</div>;
  const d = design.decisions.find((x) => x.id === origin);
  if (d) {
    const opt = d.options.find((o) => o.id === d.chosen);
    const last = d.attempts[d.attempts.length - 1];
    return (
      <div className="hc-origin">
        <div className="hc-h">why it's here · decision {d.id}{d.state === "overruled" ? " (overruled)" : ""}</div>
        <div>{d.question}</div>
        <div>→ <b>{opt?.label ?? d.custom ?? "?"}</b></div>
        {last?.defense && <div className="faint">“{last.defense.length > 160 ? last.defense.slice(0, 157) + "…" : last.defense}”</div>}
        <div className="faint small">click for the full decision</div>
      </div>
    );
  }
  const entry = design.log.find((l) => l.summary.startsWith(`accepted ${origin}:`));
  return <div className="hc-origin">accepted proposal {origin}{entry ? <span className="faint"> · {new Date(entry.at).toLocaleString()}</span> : null}</div>;
}

export function HoverCard({ hoverKey, at, snap, design, metrics, drift }: Props) {
  const [kind, ...rest] = hoverKey.split(":");
  const key = rest.join(":");
  let body: ReactNode = null;

  if (kind === "class") {
    const c = design.classes.find((x) => x.name === key);
    if (!c) return null;
    const m = metrics.classes[key] ?? snap.ghost_metrics?.classes[key];
    const pkg = c.package ? design.packages.find((p) => p.name === c.package) : undefined;
    body = (
      <>
        <div className="hc-title">
          {c.stereotype && <span className="hc-st">«{c.stereotype}»</span>}
          <b className={c.is_abstract ? "it" : ""}>{c.name}</b>
          {c.package && <span className="hc-pkg">{c.package}{pkg?.layer && pkg.layer !== c.package ? ` · ${pkg.layer}` : ""}</span>}
        </div>
        {c.status === "proposed" && proposalNote(design, c.proposal)}
        {c.status === "overruled" && <div className="hc-warn">⚠ applied by overruling the examiner</div>}
        {c.status === "pruned" && <div className="hc-warn">pruned{c.feedback ? `: “${c.feedback}”` : ""} · click to restore</div>}
        {c.rationale && <div className="hc-rat">{c.rationale}</div>}
        {c.status !== "proposed" && !c.external && purposeSection(c.name, design, metrics, drift)}
        {c.external && <div className="faint">«external» — a library/system class: drawn compact, coupling to it counts less, not judged</div>}
        {c.status !== "proposed" && originNote(design, c.origin)}
        {c.responsibilities.length > 0 && (
          <div className="hc-sec">
            <div className="hc-h">responsibilities</div>
            <ul>{c.responsibilities.map((r, i) => <li key={i}>{r}</li>)}</ul>
          </div>
        )}
        {c.attributes.length > 0 && (
          <div className="hc-sec mono">
            <div className="hc-h">attributes</div>
            {c.attributes.map((a) => <div key={a.name} className={a.status}>{attrText(a)}{a.is_static ? "  {static}" : ""}</div>)}
          </div>
        )}
        {c.methods.length > 0 && (
          <div className="hc-sec mono">
            <div className="hc-h">methods</div>
            {c.methods.map((x) => (
              <div key={x.name} className={x.status}>
                {methodText(x)}
                {x.description && <div className="hc-desc">{x.description}</div>}
              </div>
            ))}
          </div>
        )}
        {m && (
          <div className="hc-metrics">
            <span title="classes depending on this one">fan-in <b>{m.fan_in}</b></span>
            <span title="classes this one depends on">fan-out <b>{m.fan_out}</b></span>
            <span title="Ce/(Ca+Ce): 0 = stable, 1 = unstable">I <b>{m.instability.toFixed(2)}</b></span>
            <span title="LCOM4: 1 = cohesive">LCOM4 <b>{m.lcom ?? "–"}</b></span>
            <span>members <b>{m.members}</b></span>
            <span className="heat" style={{ background: heatColor(m.heat) }} title="heat" />
          </div>
        )}
        {m && m.issues.length > 0 && <div className="hc-issues">{m.issues.map((i, k) => <div key={k}>• {i}</div>)}</div>}
      </>
    );
  } else if (kind === "attribute" || kind === "method") {
    const [cls, name] = [key.slice(0, key.indexOf(".")), key.slice(key.indexOf(".") + 1)];
    const c = design.classes.find((x) => x.name === cls);
    if (kind === "attribute") {
      const a = c?.attributes.find((x) => x.name === name);
      if (!a) return null;
      const users = c!.methods.filter((m) => m.uses.includes(a.name)).map((m) => m.name + "()");
      body = (
        <>
          <div className="hc-title mono"><b>{attrText(a)}</b></div>
          <div className="faint">attribute of {cls}{a.is_static ? " · static" : ""}</div>
          {a.status === "proposed" && proposalNote(design, a.proposal)}
          {a.status !== "proposed" && a.origin !== c?.origin && originNote(design, a.origin)}
          {users.length > 0 && <div className="hc-sec">used by {users.join(", ")}</div>}
          <div className="faint small">right-click to prune</div>
        </>
      );
    } else {
      const m = c?.methods.find((x) => x.name === name);
      if (!m) return null;
      body = (
        <>
          <div className="hc-title mono"><b>{methodText(m)}</b></div>
          <div className="faint">method of {cls}{m.is_abstract ? " · abstract" : ""}{m.is_static ? " · static" : ""}</div>
          {m.status === "proposed" && proposalNote(design, m.proposal)}
          {m.status !== "proposed" && m.origin !== c?.origin && originNote(design, m.origin)}
          {m.description && <div className="hc-rat">{m.description}</div>}
          {(() => {
            const mi = metrics.responsibility?.methods.find((x) => x.class === cls && x.method === name);
            const mark = methodMarks(metrics).get(cls)?.get(name);
            const av = design.assessments?.[cls]?.methods.find((x) => x.name === name);
            return (
              <>
                {mi && (
                  <div className="hc-sec">
                    <span style={{ color: CONCERN_COLOR[mi.concern] }}>{mi.concern.replace("_", " ")}</span> · {mi.rdd}
                    <span className="faint"> — {mi.reason}</span>
                  </div>
                )}
                {av && <div className="hc-sec">agent: <b>{av.verdict}</b>{av.home ? ` → ${av.home}` : ""} <span className="faint">— {av.reason}</span></div>}
                {mark && <div className="hc-warn">{mark.message}{mark.home && <div>suggested home: <b>{mark.home}</b></div>}</div>}
              </>
            );
          })()}
          {m.uses.length > 0 && <div className="hc-sec">uses <span className="mono">{m.uses.join(", ")}</span></div>}
          {m.calls.length > 0 && <div className="hc-sec">calls <span className="mono">{m.calls.join(", ")}</span></div>}
          <div className="faint small">right-click to prune</div>
        </>
      );
    }
  } else if (kind === "relation") {
    const r = design.relations.find((x) => x.id === key);
    if (!r) return null;
    const cyc = metrics.cycle_edges.includes(r.id);
    const lv = metrics.layer_violations.includes(r.id);
    body = (
      <>
        <div className="hc-title"><b>{r.from}</b> → <b>{r.to}</b></div>
        <div className="faint">{KIND_TEXT[r.kind]}{r.kind === "association" ? (r.directed ? ", navigable →" : ", bidirectional") : ""}</div>
        {(r.from_mult || r.to_mult) && <div className="hc-sec mono">{r.from} [{r.from_mult ?? " "}] — [{r.to_mult ?? " "}] {r.to}</div>}
        {r.label && <div className="hc-sec">“{r.label}”</div>}
        {r.status === "proposed" && proposalNote(design, r.proposal)}
        {r.rationale && <div className="hc-rat">{r.rationale}</div>}
        {r.status !== "proposed" && originNote(design, r.origin)}
        {cyc && <div className="hc-bad">⟲ part of a dependency cycle — break it with an interface, an event, or by moving responsibility</div>}
        {lv && metrics.issues.filter((i) => i.rule && i.targets.includes(r.id)).map((i, k) => (
          <div key={k} className="hc-warn">⚠ {i.explain}{i.fix_hint && <div className="faint small">fix: {i.fix_hint}</div>}</div>
        ))}
        <div className="faint small">right-click to prune</div>
      </>
    );
  } else if (kind === "implicit") {
    const [from, to] = key.split(">");
    const e = metrics.implicit_edges.find((x) => x.from === from && x.to === to);
    if (!e) return null;
    body = (
      <>
        <div className="hc-title"><b>{from}</b> ⋯▸ <b>{to}</b></div>
        <div className="faint">hidden dependency via {e.via} — not drawn as a relation</div>
        {e.in_cycle && <div className="hc-bad">⟲ closes a dependency cycle</div>}
      </>
    );
  } else if (kind === "package") {
    const pm = metrics.packages.find((p) => p.name === key);
    const pk = design.packages.find((p) => p.name === key);
    body = (
      <>
        <div className="hc-title"><b>{key}</b> {pk?.layer && <span className="hc-pkg">{pk.layer} layer</span>}</div>
        {pk?.description && <div className="hc-rat">{pk.description}</div>}
        {pm && (
          <div className="hc-metrics">
            <span>classes <b>{pm.classes}</b></span>
            <span>Ca <b>{pm.fan_in}</b></span>
            <span>Ce <b>{pm.fan_out}</b></span>
            <span title="instability">I <b>{pm.instability.toFixed(2)}</b></span>
            <span title="abstractness">A <b>{pm.abstractness.toFixed(2)}</b></span>
            <span title="distance from the main sequence">D <b style={{ color: healthColor(100 - pm.distance * 100) }}>{pm.distance.toFixed(2)}</b></span>
          </div>
        )}
        {pm?.in_cycle && <div className="hc-bad">⟲ part of a package cycle</div>}
      </>
    );
  } else if (kind === "actor") {
    const a = design.use_cases.actors.find((x) => x.name === key);
    if (!a) return null;
    const ucs = design.use_cases.use_cases.filter((u) => u.actors.includes(a.name)).map((u) => u.name);
    body = (
      <>
        <div className="hc-title"><b>{a.name}</b> <span className="hc-pkg">actor</span></div>
        {a.description && <div className="hc-rat">{a.description}</div>}
        {ucs.length > 0 && <div className="hc-sec">{ucs.join(" · ")}</div>}
      </>
    );
  } else if (kind === "usecase") {
    const u = design.use_cases.use_cases.find((x) => x.name === key);
    if (!u) return null;
    body = (
      <>
        <div className="hc-title"><b>{u.name}</b></div>
        {u.description && <div className="hc-rat">{u.description}</div>}
        <div className="hc-sec">actors: {u.actors.join(", ") || "–"}</div>
        {u.classes.length > 0 && <div className="hc-sec">realised by <span className="mono">{u.classes.join(", ")}</span></div>}
        {u.includes.length > 0 && <div className="hc-sec">«include» {u.includes.join(", ")}</div>}
        {u.extends.length > 0 && <div className="hc-sec">«extend» {u.extends.join(", ")}</div>}
        <div className="faint small">click for its sequence diagram · right-click to prune</div>
      </>
    );
  } else if (kind === "message") {
    const [seq, idx] = key.split("#");
    const s = design.sequences.find((x) => x.name === seq);
    const m = s?.messages[Number(idx)];
    if (!m) return null;
    body = (
      <>
        <div className="hc-title mono"><b>{m.from} → {m.to}: {m.method}{m.args ? `(${m.args})` : m.kind !== "return" ? "()" : ""}</b></div>
        <div className="faint">{m.kind} message{m.returns ? ` · returns ${m.returns}` : ""}</div>
        {m.note && <div className="hc-rat">{m.note}</div>}
        {m.problem && <div className="hc-bad">✗ {m.problem}</div>}
      </>
    );
  } else return null;

  const w = 360;
  const left = at.x + 18 + w > window.innerWidth ? at.x - w - 18 : at.x + 18;
  const top = Math.max(12, Math.min(at.y - 20, window.innerHeight - 420));
  return (
    <div className="hovercard" style={{ left, top, width: w }}>
      {body}
    </div>
  );
}
