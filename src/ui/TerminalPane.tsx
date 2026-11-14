import { memo, useEffect, useRef, useState } from "react";
import "@xterm/xterm/css/xterm.css";
import { terminalSession } from "../state/terminal";
import { api } from "../state/api";
import { SpideCommandPalette } from "./SpideCommandPalette";

const STATUS_TEXT: Record<string, string> = {
  starting: "Starting…",
  idle: "Ready",
  busy: "Working",
  offline: "Offline",
};

export const TerminalPane = memo(function TerminalPane({ tab, status, integrationError, onCollapse }: { tab: string; status: string; integrationError: string | null; onCollapse: () => void }) {
  const host = useRef<HTMLDivElement>(null);
  const [error, setError] = useState<string | null>(null);
  const [palette, setPalette] = useState(false);
  const [restarting, setRestarting] = useState(false);
  const [confirming, setConfirming] = useState(false);
  useEffect(() => {
    const session = terminalSession();
    const release = session.attach(host.current!, setError);
    const observer = new ResizeObserver(() => session.fit());
    observer.observe(host.current!);
    const focus = () => session.terminal.focus();
    window.addEventListener("spide-focus-terminal", focus);
    return () => { window.removeEventListener("spide-focus-terminal", focus); observer.disconnect(); release(); };
  }, []);
  useEffect(() => { terminalSession().tab = tab; terminalSession().context(tab); }, [tab]);
  const restart = () => {
    if (restarting) return;
    setConfirming(false); setError(null); setRestarting(true);
    void api.restartAgent().catch((e) => setError(String(e))).finally(() => setRestarting(false));
  };
  return <section className="terminal-pane" data-terminal data-ui aria-label="Interactive pi terminal">
    <header className="pane-header">
      <span className="pane-title"><b>pi</b><span className={`terminal-status ${status}`} title={status === "starting" ? "If pi asks “Trust project folder?”, answer it in the terminal below." : undefined}><i aria-hidden />{STATUS_TEXT[status] ?? status}</span></span>
      <button onClick={() => setPalette((p) => !p)} aria-expanded={palette} title="Spide commands, including open, save and import">Commands</button>
      <button disabled={restarting} aria-expanded={confirming} onClick={() => setConfirming((v) => !v)}>{restarting ? "Restarting…" : "Restart"}</button>
      <button className="icon-button" aria-label="Collapse pi pane" title="Collapse pi pane" onClick={onCollapse}>‹</button>
    </header>
    {confirming && <div className="pane-confirm" role="alertdialog" aria-label="Restart pi"><span>Restart pi? This ends the current terminal process. Spide reopens the latest saved session.</span><button className="warnbtn" autoFocus onClick={restart}>Restart</button><button onClick={() => setConfirming(false)}>Cancel</button></div>}
    {palette && <SpideCommandPalette onClose={() => setPalette(false)} />}
    <div className="terminal-mount" ref={host} />
    {(error || integrationError) && <div className="terminal-error" role="status"><b>pi needs attention</b>{error || integrationError}</div>}
  </section>;
});
