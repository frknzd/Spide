import { useEffect, useMemo, useRef, useState } from "react";
import { useOverlay } from "./overlay";
import { COMMANDS, dispatchSpideCommand, normalizeCommand, type CommandInfo } from "../state/commands";

export function SpideCommandPalette({ onClose, initialText = "" }: { onClose: () => void; initialText?: string }) {
  const [text, setText] = useState(initialText);
  const [error, setError] = useState("");
  const [pending, setPending] = useState(false);
  const [active, setActive] = useState(0);
  const input = useRef<HTMLInputElement>(null);
  useOverlay(onClose);
  useEffect(() => { input.current?.focus(); input.current?.setSelectionRange(text.length, text.length); }, []); // eslint-disable-line react-hooks/exhaustive-deps
  const query = text.trim().replace(/^\/?spide\s+/, "").replace(/^\//, "").toLowerCase();
  const matches = useMemo(() => COMMANDS.filter((c) => !query || `${c.command} ${c.label} ${c.group} ${c.help} ${c.keywords ?? ""}`.toLowerCase().includes(query.split(/\s+/)[0])), [query]);
  const run = async (value: string) => {
    if (pending) return;
    const parsed = normalizeCommand(value);
    if (parsed.error) { setError(parsed.error); input.current?.focus(); return; }
    setPending(true); setError("");
    try { await dispatchSpideCommand(parsed.command!); onClose(); }
    catch (e) { setError(String(e)); setPending(false); input.current?.focus(); }
  };
  const choose = (c: CommandInfo) => {
    if (c.required && c.template) { setText(c.template); setError(""); input.current?.focus(); return; }
    void run(c.command);
  };
  return <div className="spide-palette" data-ui role="dialog" aria-modal="false" aria-labelledby="palette-title">
    <form onSubmit={(e) => {
      e.preventDefault();
      const exact = COMMANDS.find((c) => c.command === query);
      if (exact?.required && !/\s$/.test(text)) choose(exact);
      else if (!query.includes(" ") && !exact && matches[active]) choose(matches[active]); else void run(text);
    }}>
      <div className="palette-head"><label id="palette-title" htmlFor="spide-command-input">Commands</label><button type="button" className="icon-button" aria-label="Close commands" onClick={onClose}>×</button></div>
      <div className="palette-input"><span aria-hidden>⌘</span><input id="spide-command-input" ref={input} role="combobox" aria-expanded aria-controls="palette-list" aria-activedescendant={matches[active] ? `cmd-${active}` : undefined} disabled={pending} placeholder="Type an action or command…" value={text} onChange={(e) => { setText(e.target.value); setActive(0); setError(""); }} onKeyDown={(e) => {
        if (e.key === "ArrowDown") { e.preventDefault(); setActive((i) => Math.min(matches.length - 1, i + 1)); }
        if (e.key === "ArrowUp") { e.preventDefault(); setActive((i) => Math.max(0, i - 1)); }
      }} />{pending && <span className="spinner" aria-label="Running" />}</div>
    </form>
    <div id="palette-list" className="palette-commands" role="listbox">{matches.map((c, i) => <button id={`cmd-${i}`} role="option" aria-selected={i === active} className={i === active ? "active" : ""} key={c.command} disabled={pending} onMouseEnter={() => setActive(i)} onClick={() => choose(c)}>
      <span><b>{c.label}</b><small>{c.help}</small></span><code>/spide {c.template ?? c.command}{c.required ? "…" : ""}</code>
    </button>)}
      {!matches.length && <p className="palette-empty">No matching action. Press Enter to run “{text.trim()}” as a command.</p>}
    </div>
    {error && <p className="form-error" role="alert">{error}</p>}
  </div>;
}
