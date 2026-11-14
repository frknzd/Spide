import { DETAIL_LABELS } from "../canvas/visuals";

const HELP = [
  "packages only, with their dependencies",
  "class names and stereotypes",
  "names plus responsibilities (RDD view)",
  "public behaviour; accessors, private helpers, inherited and association attributes folded (exam style)",
  "every member",
];

/** Granularity slider: how much of each class the diagram shows (also /detail 0–4). */
export function DetailSlider({ level, onChange, disabled }: { level: number; onChange: (l: number) => void; disabled?: boolean }) {
  return (
    <label className="detail-slider" data-ui title={disabled ? "Exit preview to change detail" : `${DETAIL_LABELS[level]} — ${HELP[level]}`}>
      <span>Detail</span>
      <input type="range" min={0} max={4} step={1} value={level} disabled={disabled} aria-valuetext={`${DETAIL_LABELS[level]}: ${HELP[level]}`} onChange={(e) => onChange(Number(e.target.value))} />
      <span className="detail-label">{DETAIL_LABELS[level]}</span>
    </label>
  );
}
