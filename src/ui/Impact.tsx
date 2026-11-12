// Consequences of a change (decision option / proposal): compact line + details.
import { useState } from "react";
import { healthColor } from "../canvas/visuals";
import type { Impact } from "../state/types";

export function ImpactLine({ impact, expandable = true }: { impact: Impact | null | undefined; expandable?: boolean }) {
  const [open, setOpen] = useState(false);
  if (!impact) return null;
  const d = impact.health_after - impact.health_before;
  const bits: { text: string; cls: string }[] = [];
  if (impact.added.length) bits.push({ text: `+${impact.added.length}`, cls: "im-add" });
  if (impact.changed.length) bits.push({ text: `~${impact.changed.length}`, cls: "im-chg" });
  if (impact.removed.length) bits.push({ text: `−${impact.removed.length}`, cls: "im-rem" });
  if (impact.new_issues.length) bits.push({ text: `${impact.new_issues.length} new issue${impact.new_issues.length > 1 ? "s" : ""}`, cls: "im-bad" });
  if (impact.resolved_issues.length) bits.push({ text: `${impact.resolved_issues.length} resolved`, cls: "im-good" });
  if (impact.flipped_deps.length) bits.push({ text: `${impact.flipped_deps.length} flipped`, cls: "im-chg" });
  if (impact.rule_breaks.length) bits.push({ text: `${impact.rule_breaks.length} rule ✗`, cls: "im-bad" });
  if (impact.affected_scenarios.length) bits.push({ text: `${impact.affected_scenarios.length} scenario${impact.affected_scenarios.length > 1 ? "s" : ""}`, cls: "im-chg" });
  return (
    <div className="impact" onClick={(e) => e.stopPropagation()}>
      <div className={`im-line ${expandable ? "click" : ""}`} onClick={() => expandable && setOpen((o) => !o)} title="consequences if chosen">
        {bits.map((b, i) => <span key={i} className={b.cls}>{b.text}</span>)}
        {d !== 0 && <span style={{ color: healthColor(impact.health_after) }}>{d > 0 ? `▲${d}` : `▼${-d}`} health</span>}
        {bits.length === 0 && d === 0 && <span className="faint">no structural change</span>}
        {expandable && <span className="faint">{open ? "▴" : "▾"}</span>}
      </div>
      {(open || !expandable) && <ImpactDetails impact={impact} />}
    </div>
  );
}

export function ImpactDetails({ impact }: { impact: Impact }) {
  const list = (title: string, items: string[], cls: string) =>
    items.length > 0 && (
      <div className="im-sec">
        <div className="hc-h">{title}</div>
        {items.map((x, i) => <div key={i} className={cls}>{x}</div>)}
      </div>
    );
  return (
    <div className="im-details">
      {list("added", impact.added, "im-add")}
      {list("changed", impact.changed, "im-chg")}
      {list("removed", impact.removed, "im-rem")}
      {list("new issues", impact.new_issues, "im-bad")}
      {list("resolved", impact.resolved_issues, "im-good")}
      {list("rules broken", impact.rule_breaks, "im-bad")}
      {list("dependencies flipped", impact.flipped_deps.map(([a, b]) => `${a} → ${b} (was ${b} → ${a})`), "im-chg")}
      {list("scenarios to re-check", impact.affected_scenarios, "im-chg")}
      {impact.class_deltas.length > 0 && (
        <div className="im-sec">
          <div className="hc-h">coupling</div>
          {impact.class_deltas.map((c) => (
            <div key={c.class} className="mono small">
              {c.class}: in {sign(c.fan_in)} · out {sign(c.fan_out)}{c.heat ? ` · heat ${sign(c.heat)}` : ""}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

const sign = (n: number) => (n > 0 ? `+${n}` : `${n}`);
