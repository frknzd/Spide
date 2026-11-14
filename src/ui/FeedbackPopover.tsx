import { useEffect, useRef, useState } from "react";
import { targetLabel, type Target } from "../state/types";

export interface PruneState {
  targets: Target[];
  at: { x: number; y: number };
}

/** After a prune: "what didn't you like / what do you want instead?" */
export function FeedbackPopover({ prune, onSend, onClose, onUndo }: { prune: PruneState; onSend: (text: string) => void; onClose: () => void; onUndo: () => void }) {
  const [text, setText] = useState("");
  const ref = useRef<HTMLTextAreaElement>(null);
  useEffect(() => ref.current?.focus(), [prune]);
  const w = 340;
  const left = Math.min(window.innerWidth - w - 16, Math.max(16, prune.at.x - w / 2));
  const top = Math.min(window.innerHeight - 220, prune.at.y + 14);
  const labels = prune.targets.map(targetLabel);
  return (
    <div className="feedback" data-ui role="dialog" aria-label="Pruned. Tell the agent why" style={{ left, top, width: w }}>
      <div className="fb-head">
        <span className="scissor" aria-hidden>✂</span>
        <span className="fb-what"><span className="sr-only">Pruned </span>{labels.length > 3 ? `${labels.slice(0, 3).join(", ")} +${labels.length - 3}` : labels.join(", ")}</span>
        <button onClick={onUndo}>Undo</button>
      </div>
      <textarea
        ref={ref}
        rows={2}
        aria-label="Feedback for the agent (optional)"
        value={text}
        placeholder="What didn't you like? What do you want instead?"
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && !e.shiftKey) {
            e.preventDefault();
            if (text.trim()) onSend(text.trim());
            else onClose();
          } else if (e.key === "Escape") { e.stopPropagation(); onClose(); }
        }}
      />
      <div className="fb-foot"><span className="fb-hint">Pruned elements stay hidden; Undo restores them.</span><button onClick={onClose}>Done</button><button className="primary" disabled={!text.trim()} onClick={() => onSend(text.trim())}>Tell pi</button></div>
    </div>
  );
}
