import { NodeDefs } from "../canvas/ClassNode";
import { C, F, FONT_MONO, FONT_SANS, textWidth } from "../canvas/visuals";
import { Viewport, type WorldRect } from "../canvas/Viewport";
import type { Message, ScenarioScore, Sequence, Target } from "../state/types";
import type { ViewProps } from "./PackageView";
import { Banner } from "./UseCaseView";

const T = (t: Target) => JSON.stringify(t);
const ROW = 46;
const TOP = 70;

/** Calls are numbered 1, 2, 3 … (returns are not), like the PlantUML export. */
export function msgLabel(m: Message, i: number, all?: Message[]) {
  if (m.kind === "return") return m.method || m.returns || "";
  const n = all ? all.slice(0, i + 1).filter((x) => x.kind !== "return").length : i + 1;
  const call = /\(/.test(m.method) ? m.method : `${m.method}(${m.args ?? ""})`;
  return `${n}: ${call}${m.returns ? ` : ${m.returns}` : ""}`;
}

/** How well the scenario fits the design: hops, depth, boundary crossings, aggregates. */
function ScoreBar({ s, max }: { s?: ScenarioScore; max: number }) {
  if (!s) return null;
  const chip = (text: string, bad: boolean, title: string) => <span className={bad ? "sc-bad" : ""} title={title}>{text}</span>;
  return (
    <div className="score-bar">
      <div className="ss-h">fit of “{s.name}”</div>
      {chip(`${s.hops} calls`, s.hops > max, `calls in this scenario (chatty above ${max})`)}
      {chip(`depth ${s.max_depth}`, s.max_depth > 4, "deepest nesting of synchronous calls")}
      {chip(`${s.package_crossings} package crossings`, false, "calls between packages")}
      {chip(`${s.layer_crossings} layer crossings`, false, "calls between layers")}
      {s.container_crossings > 0 && chip(`${s.container_crossings} container crossings`, s.sync_container_calls > 1, `${s.sync_container_calls} of them synchronous`)}
      {chip(`${s.aggregates_touched.length} aggregate${s.aggregates_touched.length === 1 ? "" : "s"}`, s.aggregates_touched.length > 1,
        s.aggregates_touched.length ? `touches ${s.aggregates_touched.join(", ")} — more than one aggregate per transaction weakens consistency` : "no aggregates touched")}
      {s.unknown_calls > 0 && chip(`${s.unknown_calls} unknown`, true, "calls to methods that don't exist")}
    </div>
  );
}

export function sequenceLayout(s: Sequence, isActor: (n: string) => boolean) {
  const xs = new Map<string, number>();
  const widths = new Map<string, number>();
  let x = 0;
  s.participants.forEach((p, i) => {
    const w = Math.max(110, textWidth(p, F.name) + 30);
    widths.set(p, w);
    if (i > 0) {
      // make room for the longest label between neighbours
      const prev = s.participants[i - 1];
      let need = 190;
      s.messages.forEach((m, k) => {
        if ((m.from === prev && m.to === p) || (m.from === p && m.to === prev)) need = Math.max(need, textWidth(msgLabel(m, k, s.messages), F.member) + 40);
      });
      x += Math.max(need, (widths.get(prev)! + w) / 2 + 40);
    }
    xs.set(p, x);
  });
  const height = TOP + s.messages.length * ROW + 40;
  return { xs, widths, height, isActor };
}

export function SequenceView(p: ViewProps & { selected: string | null; onSelect: (s: string | null) => void; onGenerate: (s: string | null) => void }) {
  const { snap } = p;
  const seqs = snap.view.sequences.filter((s) => s.status !== "pruned");
  const sel = seqs.find((s) => s.name === p.selected) ?? seqs[0];
  const suggestions = snap.derived.suggested_scenarios;
  const stale = sel ? snap.derived.stale_sequences.includes(sel.name) : false;
  const actors = new Set(snap.view.use_cases.actors.map((a) => a.name));
  const wantScenario = p.selected && !seqs.some((s) => s.name === p.selected) ? p.selected : null;

  return (
    <>
      <div className="seq-side" data-ui>
        <div className="ss-h">sequences</div>
        {seqs.map((s) => {
          const probs = s.messages.filter((m) => m.problem).length;
          return (
            <div key={s.name} className={`ss-item ${sel?.name === s.name ? "on" : ""}`} onClick={() => p.onSelect(s.name)}>
              {s.name}
              {probs > 0 && <span className="bad"> ✗{probs}</span>}
              {snap.derived.stale_sequences.includes(s.name) && <span className="stale" />}
            </div>
          );
        })}
        {seqs.length === 0 && <div className="faint small">none yet</div>}
        {suggestions.length > 0 && <div className="ss-h">from use cases</div>}
        {suggestions.map((s) => (
          <div key={s} className="ss-item sugg" onClick={() => (p.onSelect(s), p.onGenerate(s))}>＋ {s}</div>
        ))}
        <div className="faint small ss-hint">or ask in the prompt: “sequence for …”</div>
        {!wantScenario && sel && <ScoreBar s={snap.metrics.scenarios.find((x) => x.name === sel.name)} max={snap.view.rules.thresholds.max_scenario_hops} />}
      </div>
      {wantScenario && <Banner show busy={snap.pi_status === "busy"} empty text={`No sequence for “${wantScenario}” yet.`} onRegenerate={() => p.onGenerate(wantScenario)} />}
      {!wantScenario && !sel && <Banner show busy={snap.pi_status === "busy"} empty text="No sequence diagrams yet." onRegenerate={() => p.onGenerate(null)} />}
      {!wantScenario && sel && stale && <Banner show busy={snap.pi_status === "busy"} text="Class model changed since this was drawn." onRegenerate={() => p.onGenerate(sel.name)} />}
      <Viewport ref={(v) => void (p.vpRef.current = v)} defs={<NodeDefs />} empty={!sel || !!wantScenario} key={sel?.name ?? "none"}
        onPrune={p.onPrune} onHover={p.onHover} onClickTarget={(t) => p.onClickTarget(t)}
        collect={(r: WorldRect) => (sel ? collectMsgs(sel, r, actors) : [])}>
        {sel && !wantScenario && <SequenceDiagram s={sel} isActor={(n) => actors.has(n)} />}
      </Viewport>
    </>
  );
}

function collectMsgs(s: Sequence, r: WorldRect, actors: Set<string>): Target[] {
  const L = sequenceLayout(s, (n) => actors.has(n));
  const out: Target[] = [];
  s.messages.forEach((m, i) => {
    const y = TOP + i * ROW + 20;
    const x = ((L.xs.get(m.from) ?? 0) + (L.xs.get(m.to) ?? 0)) / 2;
    if (x >= r.x1 && x <= r.x2 && y >= r.y1 && y <= r.y2) out.push({ kind: "message", sequence: s.name, index: i });
  });
  return out;
}

export function SequenceDiagram({ s, isActor }: { s: Sequence; isActor: (n: string) => boolean }) {
  const L = sequenceLayout(s, isActor);
  const bottom = L.height;
  // activations: from a call to the matching return (or a short bar)
  const acts: { p: string; y1: number; y2: number; depth: number }[] = [];
  const open: { p: string; i: number; from: string }[] = [];
  s.messages.forEach((m, i) => {
    const y = TOP + i * ROW + 20;
    if (m.kind === "return") {
      const k = [...open].reverse().findIndex((o) => o.p === m.from && o.from === m.to);
      if (k >= 0) {
        const idx = open.length - 1 - k;
        const o = open[idx];
        open.splice(idx, 1);
        acts.push({ p: o.p, y1: TOP + o.i * ROW + 20, y2: y, depth: open.filter((x) => x.p === o.p).length });
      }
    } else if (m.kind === "sync" && m.from !== m.to) open.push({ p: m.to, i, from: m.from });
    else if (m.from === m.to) acts.push({ p: m.to, y1: y + 6, y2: y + 30, depth: 1 });
  });
  for (const o of open) acts.push({ p: o.p, y1: TOP + o.i * ROW + 20, y2: TOP + o.i * ROW + 46, depth: 0 });

  return (
    <g>
      {s.participants.map((p) => {
        const x = L.xs.get(p)!;
        const w = L.widths.get(p)!;
        const actor = isActor(p);
        return (
          <g key={p} className="node-enter">
            <line x1={x} x2={x} y1={actor ? 52 : 36} y2={bottom} stroke={C.faint} strokeDasharray="5 5" />
            {actor ? (
              <g transform={`translate(${x},0)`}>
                <circle cy={-14} r={8} fill="none" stroke={C.text} strokeWidth={1.5} />
                <path d="M0,-6 v16 M-11,0 h22 M0,10 l-9,13 M0,10 l9,13" stroke={C.text} strokeWidth={1.5} fill="none" />
                <text y={40} textAnchor="middle" fontFamily={FONT_SANS} fontSize={12.5} fill={C.text}>{p}</text>
              </g>
            ) : (
              <g data-hover={`class:${p}`}>
                <rect x={x - w / 2} y={0} width={w} height={36} rx={6} fill={C.node} stroke={C.border} strokeWidth={1.2} />
                <text x={x} y={23} textAnchor="middle" fontFamily={FONT_SANS} fontSize={13.5} fontWeight={600} fill={C.text}>{p}</text>
              </g>
            )}
          </g>
        );
      })}
      {acts.map((a, i) => (
        <rect key={i} x={L.xs.get(a.p)! - 6 + a.depth * 5} y={a.y1} width={12} height={Math.max(14, a.y2 - a.y1)} fill="#30343d" stroke={C.border} />
      ))}
      {s.messages.map((m, i) => {
        const y = TOP + i * ROW + 20;
        const x1 = L.xs.get(m.from);
        const x2 = L.xs.get(m.to);
        if (x1 === undefined || x2 === undefined) return null;
        const bad = !!m.problem;
        const color = bad ? C.cycle : m.status === "pruned" ? C.faint : m.kind === "return" ? C.dim : C.text;
        const label = msgLabel(m, i, s.messages);
        const tgt = T({ kind: "message", sequence: s.name, index: i });
        const hover = `message:${s.name}#${i}`;
        if (x1 === x2) {
          const d = `M${x1 + 6},${y + 4} h46 v22 h-40`;
          return (
            <g key={i} data-target={tgt} data-hover={hover} style={{ cursor: "context-menu" }}>
              <path d={d} stroke="transparent" strokeWidth={14} fill="none" />
              <path d={d} stroke={color} strokeWidth={1.3} fill="none" />
              <polygon points={`${x1 + 12},${y + 26} ${x1 + 22},${y + 21} ${x1 + 22},${y + 31}`} fill={color} />
              <text x={x1 + 60} y={y + 8} fontFamily={FONT_MONO} fontSize={11.5} fill={color}>{bad ? "✗ " : ""}{label}</text>
            </g>
          );
        }
        const dir = x2 > x1 ? 1 : -1;
        const ex = x2 - dir * 7;
        const sx = x1 + dir * 7;
        const dashed = m.kind === "return" || m.kind === "create";
        const head =
          m.kind === "sync" || m.kind === "create" ? (
            <polygon points={`${ex},${y} ${ex - dir * 11},${y - 5} ${ex - dir * 11},${y + 5}`} fill={color} />
          ) : (
            <polyline points={`${ex - dir * 11},${y - 5} ${ex},${y} ${ex - dir * 11},${y + 5}`} fill="none" stroke={color} strokeWidth={1.3} />
          );
        return (
          <g key={i} data-target={tgt} data-hover={hover} style={{ cursor: "context-menu" }} opacity={m.status === "pruned" ? 0.35 : 1}>
            <line x1={sx} x2={ex} y1={y} y2={y} stroke="transparent" strokeWidth={14} />
            <line x1={sx} x2={ex} y1={y} y2={y} stroke={color} strokeWidth={bad ? 1.8 : 1.3} strokeDasharray={dashed ? "6 4" : undefined} />
            {head}
            <text x={(x1 + x2) / 2} y={y - 7} textAnchor="middle" fontFamily={FONT_MONO} fontSize={11.5} fill={color}>
              {bad ? "✗ " : ""}{m.kind === "create" ? "«create» " : ""}{label}
            </text>
            {m.note && <text x={(x1 + x2) / 2} y={y + 15} textAnchor="middle" fontFamily={FONT_SANS} fontSize={10.5} fill={C.faint} fontStyle="italic">{m.note}</text>}
          </g>
        );
      })}
    </g>
  );
}
