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
    if (t && t !== s.value) s.onSubmit(t);
    onClose();
  };
  return (
    <div className="inline-edit" data-ui style={{ left: s.at.x - 20, top: s.at.y - 18 }}>
      <span className="faint small">{s.label}</span>
      <input ref={ref} value={v} spellCheck={false} onChange={(e) => setV(e.target.value)} onBlur={onClose}
        onKeyDown={(e) => {
          if (e.key === "Enter") commit();
          if (e.key === "Escape") onClose();
          e.stopPropagation();
        }} />
    </div>
  );
}
