// Time travel: scrub through the design's history, restore a past state or fork a branch from it.
import { useEffect, useState } from "react";
import { api } from "../state/api";
import type { Design, HistoryEntry } from "../state/types";

interface Props {
  rev: number;
  onPreview: (d: Design | null) => void;
  onClose: () => void;
  run: (p: Promise<unknown>) => void;
  toast: (kind: string, text: string) => void;
}

export function Timeline({ rev, onPreview, onClose, run, toast }: Props) {
  const [entries, setEntries] = useState<HistoryEntry[]>([]);
  const [i, setI] = useState(-1);
  const [fork, setFork] = useState<string | null>(null);

  // reload when the design changes (new entries)
  useEffect(() => {
    api.historyList().then((e) => {
      setEntries(e);
      setI((cur) => (cur < 0 || cur >= e.length - 1 ? e.length - 1 : cur));
    }).catch((e) => toast("error", String(e)));
  }, [rev]); // eslint-disable-line react-hooks/exhaustive-deps

  const latest = i === entries.length - 1;
  useEffect(() => {
    if (i < 0 || !entries[i]) return;
    if (latest) return onPreview(null);
    let alive = true;
    api.historyAt(entries[i].rev).then((d) => alive && onPreview(d)).catch((e) => toast("error", String(e)));
    return () => {
      alive = false;
    };
  }, [i, entries]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => () => onPreview(null), []); // eslint-disable-line react-hooks/exhaustive-deps

  const e = entries[i];
  return (
    <div className="timeline" data-ui onKeyDown={(ev) => {
      if (ev.key === "ArrowLeft") setI((x) => Math.max(0, x - 1));
      if (ev.key === "ArrowRight") setI((x) => Math.min(entries.length - 1, x + 1));
    }}>
      <div className="tl-row">
        <span className="dp-kicker">timeline</span>
        <input type="range" min={0} max={Math.max(0, entries.length - 1)} value={Math.max(0, i)} autoFocus
          onChange={(ev) => setI(Number(ev.target.value))} />
        <span className="faint small">{i + 1}/{entries.length}</span>
        <button className="link" onClick={onClose}>close</button>
      </div>
      {e && (
        <div className="tl-row tl-info">
          <span className={`tl-actor ${e.actor}`}>{e.actor || "—"}</span>
          <span className="tl-sum">{e.summary}</span>
          {e.at > 0 && <span className="faint small">{new Date(e.at).toLocaleString()}</span>}
          {!latest && (
            <>
              <button className="link" onClick={() => run(api.historyRestore(e.rev).then(onClose))} title="propose the changes that bring the design back to this state">restore here</button>
              {fork === null ? (
                <button className="link" onClick={() => setFork("")}>fork…</button>
              ) : (
                <input className="tl-fork" autoFocus placeholder="branch name ⏎" value={fork} onChange={(ev) => setFork(ev.target.value)}
                  onKeyDown={(ev) => {
                    if (ev.key === "Enter" && fork.trim()) run(api.historyFork(e.rev, fork.trim()).then(() => setFork(null)));
                    if (ev.key === "Escape") setFork(null);
                    ev.stopPropagation();
                  }} />
              )}
            </>
          )}
          {latest && <span className="faint small">now</span>}
        </div>
      )}
    </div>
  );
}
