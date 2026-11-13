import { useEffect, useRef, useState } from "react";
import { useAgentEntries } from "../state/store";
import { perfCount } from "../state/perf";

/** The agent's latest answer, floating above the prompt; fades after a while. */
export function AnswerBubble({ history, onCloseHistory }: { history: boolean; onCloseHistory: () => void }) {
  perfCount("AnswerBubble");
  const entries = useAgentEntries();
  const last = [...entries].reverse().find((e) => e.role !== "user");
  const [visible, setVisible] = useState(false);
  const [hover, setHover] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const histRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!last) return;
    setVisible(true);
    if (timer.current) clearTimeout(timer.current);
    if (last.done && !hover) {
      const ms = Math.min(30000, 9000 + last.text.length * 45);
      timer.current = setTimeout(() => setVisible(false), ms);
    }
  }, [last?.id, last?.text, last?.done, hover]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    if (history) histRef.current?.scrollTo(0, histRef.current.scrollHeight);
  }, [history, entries.length]);

  if (history) {
    return (
      <div className="history" data-ui>
        <div className="history-head">
          <span>conversation</span>
          <button onClick={onCloseHistory}>close ⌘J</button>
        </div>
        <div className="history-body" ref={histRef}>
          {entries.length === 0 && <div className="faint">nothing yet</div>}
          {entries.map((e) => (
            <div key={e.id} className={`h-entry ${e.role}`}>
              {e.text || (e.tools.length ? "" : "…")}
              {e.tools.length > 0 && <div className="tools">{e.tools.map((t, i) => <span key={i}>{t}</span>)}</div>}
            </div>
          ))}
        </div>
      </div>
    );
  }
  if (!last || (!last.text && last.role === "agent")) return null;
  return (
    <div className={`bubble ${last.role} ${visible ? "show" : ""}`} data-ui onMouseEnter={() => setHover(true)} onMouseLeave={() => setHover(false)}>
      {last.text}
      {!last.done && <span className="caret" />}
    </div>
  );
}
