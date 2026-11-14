// A small floating text field for direct edits (rename a class or member).
import { useEffect, useRef, useState } from "react";

export interface InlineEditState { at: { x: number; y: number }; label: string; value: string; onSubmit: (v: string) => void }

export function InlineEdit({ s, onClose }: { s: InlineEditState; onClose: () => void }) {
  const [v, setV] = useState(s.value);
  const ref = useRef<HTMLInputElement>(null);
  useEffect(() => {
    ref.current?.focus();
    ref.current?.select();
  }, []);
  const commit = () => {
    const t = v.trim();
    if (!t) return;
    if (t !== s.value) s.onSubmit(t);
    onClose();
  };
  return (
    <div className="inline-edit" data-ui role="dialog" aria-label={s.label} style={{ left: Math.max(8, Math.min(s.at.x - 20, window.innerWidth - 270)), top: Math.max(38, Math.min(s.at.y - 18, window.innerHeight - 90)) }}>
      <label htmlFor="inline-edit-input">{s.label}</label>
      <input id="inline-edit-input" ref={ref} aria-invalid={!v.trim()} value={v} spellCheck={false} onChange={(e) => setV(e.target.value)} onBlur={onClose}
        onKeyDown={(e) => {
          if (e.key === "Enter") commit();
          if (e.key === "Escape") onClose();
          e.stopPropagation();
        }} />
      <span className="inline-hint">{v.trim() ? "Enter to save · Esc to cancel" : "Name is required"}</span>
    </div>
  );
}
