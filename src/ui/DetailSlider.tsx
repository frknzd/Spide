import { DETAIL_LABELS } from "../canvas/visuals";

const HELP = [
  "packages only, with their dependencies",
  "class names and stereotypes",
  "names plus responsibilities (RDD view)",
  "public behaviour; accessors, private helpers, inherited and association attributes folded (exam style)",
  "every member",
];

/** Granularity slider: how much of each class the diagram shows (also /detail 0–4). */
export function DetailSlider({ level, onChange }: { level: number; onChange: (l: number) => void }) {
  return (
    <div className="detail-slider" data-ui title={`detail: ${DETAIL_LABELS[level]} — ${HELP[level]}  (/detail 0–4)`}>
      <span className="faint">detail</span>
      <input type="range" min={0} max={4} step={1} value={level} onChange={(e) => onChange(Number(e.target.value))} />
      <span className="detail-label">{DETAIL_LABELS[level]}</span>
    </div>
  );
}
