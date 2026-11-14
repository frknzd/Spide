// Time travel: scrub through the design's history, restore a past state or fork a branch from it.
import { useEffect, useState } from "react";
import { api } from "../state/api";
import type { Design, HistoryEntry } from "../state/types";

interface Props {
  rev: number;
  onPreview: (d: Design | null) => void;
  onClose: () => void;
  run: (p: Promise<unknown>) => Promise<boolean>;
  toast: (kind: string, text: string) => void;
}

export function Timeline({ rev, onPreview, onClose, run }: Props) {
  const [entries, setEntries] = useState<HistoryEntry[] | null>(null);
  const [selected, setSelected] = useState<number | null>(null);
  const [error, setError] = useState("");
  const [attempt, setAttempt] = useState(0);
  const [fork, setFork] = useState<string | null>(null);
  const [pending, setPending] = useState(false);

  // Reload when the design changes; keep the selected revision by identity, not position.
  useEffect(() => {
    let alive = true;
    setError("");
    api.historyList().then((e) => {
      if (!alive) return;
      setEntries(e);
      setSelected((cur) => (cur !== null && e.some((x) => x.rev === cur) ? cur : e.at(-1)?.rev ?? null));
    }, (e) => alive && setError(String(e)));
    return () => { alive = false; };
  }, [rev, attempt]);

  const i = entries && selected !== null ? entries.findIndex((x) => x.rev === selected) : -1;
  const latest = !!entries && i === entries.length - 1;
  useEffect(() => {
    if (!entries || i < 0) return;
    if (latest) return onPreview(null);
    let alive = true;
    api.historyAt(entries[i].rev).then((d) => alive && onPreview(d), (e) => alive && setError(String(e)));
    return () => { alive = false; };
  }, [selected, entries]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => () => onPreview(null), []); // eslint-disable-line react-hooks/exhaustive-deps

  const e = entries?.[i];
  const step = (n: number) => entries?.length && setSelected(entries[Math.max(0, Math.min(entries.length - 1, i + n))].rev);
  const doFork = async () => {
    const name = fork?.trim();
    if (!e || !name || pending) return;
    setPending(true);
    if (await run(api.historyFork(e.rev, name))) setFork(null);
    setPending(false);
  };
  return (
    <section className="timeline" data-ui aria-label="Design timeline">
      <div className="tl-row">
        <span className="dp-kicker">Timeline</span>
        {!entries && !error && <span className="tl-status"><span className="spinner" /> Loading history…</span>}
        {entries?.length === 0 && <span className="tl-status">No history yet. Changes appear here after you edit the design.</span>}
        {!!entries?.length && <>
          <button className="icon-button" aria-label="Previous revision" disabled={i <= 0} onClick={() => step(-1)}>‹</button>
          <input type="range" aria-label="Revision" aria-valuetext={e ? `${i + 1} of ${entries.length}: ${e.summary}` : undefined} min={0} max={entries.length - 1} value={Math.max(0, i)} autoFocus
            onChange={(ev) => setSelected(entries[Number(ev.target.value)].rev)} />
          <button className="icon-button" aria-label="Next revision" disabled={latest} onClick={() => step(1)}>›</button>
          <span className="tl-count">{i + 1} / {entries.length}</span>
        </>}
        <button className="icon-button" aria-label="Close timeline" onClick={onClose}>×</button>
      </div>
      {error && <div className="tl-row form-error" role="alert">Couldn't load history: {error} <button className="link" onClick={() => setAttempt((n) => n + 1)}>Retry</button></div>}
      {e && (
        <div className="tl-row tl-info">
          <span className={`tl-actor ${e.actor}`}>{e.actor || "—"}</span>
          <span className="tl-sum">{e.summary}</span>
          {e.at > 0 && <time className="faint small">{new Date(e.at).toLocaleString()}</time>}
          {latest ? <span className="tl-now">Current</span> : (
            <span className="tl-actions">
              <button disabled={pending} onClick={async () => { setPending(true); if (await run(api.historyRestore(e.rev))) onClose(); setPending(false); }} title="Propose the changes that bring the design back to this state">Restore as proposals</button>
              {fork === null ? (
                <button disabled={pending} onClick={() => setFork("")}>Fork…</button>
              ) : (
                <form className="inline-form" onSubmit={(ev) => { ev.preventDefault(); void doFork(); }} onKeyDown={(ev) => { if (ev.key === "Escape") { ev.stopPropagation(); setFork(null); } }}>
                  <input className="tl-fork" aria-label="New branch name" autoFocus disabled={pending} placeholder="Branch name" value={fork} onChange={(ev) => setFork(ev.target.value)} />
                  <button type="submit" className="primary" disabled={pending || !fork.trim()}>Create</button>
                  <button type="button" onClick={() => setFork(null)}>Cancel</button>
                </form>
              )}
            </span>
          )}
        </div>
      )}
    </section>
  );
}
