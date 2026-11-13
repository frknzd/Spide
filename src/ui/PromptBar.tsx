import { useEffect, useRef, useState } from "react";
import type { PiStatus } from "../state/types";
import { useAgentActivity } from "../state/store";

export const COMMANDS: { cmd: string; help: string }[] = [
  { cmd: "/undo", help: "undo (⌘Z)" },
  { cmd: "/redo", help: "redo (⇧⌘Z)" },
  { cmd: "/accept", help: "accept all ghost proposals" },
  { cmd: "/reject", help: "reject all ghost proposals" },
  { cmd: "/tidy", help: "re-layout the diagram" },
  { cmd: "/detail", help: "/detail 0–4 — granularity: packages · names · responsibilities · design (default) · full" },
  { cmd: "/external", help: "/external <package|Class> — tag (or untag) as an external library/system: compact, coupling to it counts less" },
  { cmd: "/implicit", help: "show all implicit (type/call) dependencies, not only the hovered class's" },
  { cmd: "/fit", help: "fit diagram to screen (or double-click)" },
  { cmd: "/add", help: "/add <Class> [in <package>] — add a class yourself (double-click names to rename, drag onto a package to move)" },
  { cmd: "/focus", help: "/focus <package> — zoom to a package and dim the rest (Esc clears)" },
  { cmd: "/find", help: "/find <class> — zoom to a class and focus it (Esc clears)" },
  { cmd: "/pruned", help: "show / hide pruned elements" },
  { cmd: "/strict", help: "strict examiner: no overruling" },
  { cmd: "/lenient", help: "lenient examiner: you may overrule" },
  { cmd: "/history", help: "conversation history (⌘J)" },
  { cmd: "/branch", help: "/branch <name> — branch from here and switch · /branch delete <name> · /branches" },
  { cmd: "/switch", help: "/switch <branch> — work on another branch" },
  { cmd: "/merge", help: "/merge <branch> — bring a branch's changes in as ghost proposals" },
  { cmd: "/compare", help: "/compare [branch] — open decision's options, or this design vs a branch" },
  { cmd: "/timeline", help: "scrub through the design's history; restore or fork (⌘T)" },
  { cmd: "/brief", help: "goals, qualities, constraints, expected changes + partner mode (⌘B)" },
  { cmd: "/mode", help: "/mode examiner | socratic | devils_advocate | sketch" },
  { cmd: "/stress", help: "/stress <change scenario> — highlight what an expected change touches (Esc clears)" },
  { cmd: "/rules", help: "/rules [clean | hexagonal | layered | modular_monolith | none] — architecture rules" },
  { cmd: "/skip", help: "skip the brief and start designing" },
  { cmd: "/export png", help: "export current diagram as PNG" },
  { cmd: "/export puml", help: "export current diagram as PlantUML" },
  { cmd: "/export archunit", help: "export the rules as an ArchUnit test (Java)" },
  { cmd: "/import", help: "import a Java source folder to compare the design with the code" },
  { cmd: "/drift", help: "design ↔ code: what's missing, extra, or breaking rules" },
  { cmd: "/coderoot", help: "/coderoot org.acme.app — Java root package (ArchUnit, code import)" },
  { cmd: "/export adr", help: "export decided decisions as ADRs (markdown)" },
  { cmd: "/export all", help: "export every diagram as PlantUML into a folder" },
  { cmd: "/save", help: "save a copy of the design (JSON)" },
  { cmd: "/open", help: "open a design (JSON)" },
  { cmd: "/new", help: "new design in a folder" },
  { cmd: "/name", help: "/name <title> — rename the design" },
  { cmd: "/stop", help: "stop the agent" },
  { cmd: "/agent restart", help: "restart the agent (pi)" },
  { cmd: "/folder", help: "reveal the project folder" },
];

interface Props {
  onSubmit: (text: string) => void;
  status: PiStatus;
  placeholder: string;
  focusKey: number;
}

export function PromptBar(p: Props) {
  const activity = useAgentActivity();
  const [text, setText] = useState("");
  const [hist, setHist] = useState<string[]>([]);
  const [hi, setHi] = useState(-1);
  const ta = useRef<HTMLTextAreaElement>(null);
  useEffect(() => ta.current?.focus(), [p.focusKey]);
  useEffect(() => {
    const el = ta.current;
    if (!el) return;
    el.style.height = "0px";
    el.style.height = Math.min(160, el.scrollHeight) + "px";
  }, [text]);

  const suggestions = text.startsWith("/") && !text.includes("\n") ? COMMANDS.filter((c) => c.cmd.startsWith(text.split(" ")[0]) || c.cmd.startsWith(text)) : [];

  const submit = () => {
    const t = text.trim();
    if (!t) return;
    p.onSubmit(t);
    setHist((h) => [...h.filter((x) => x !== t), t].slice(-50));
    setHi(-1);
    setText("");
  };

  return (
    <div className="prompt-wrap" data-ui>
      {suggestions.length > 0 && (
        <div className="cmd-list">
          {suggestions.slice(0, 8).map((c) => (
            <div key={c.cmd} className="cmd" onMouseDown={(e) => { e.preventDefault(); setText(c.cmd + (["/add", "/focus", "/name", "/find", "/mode", "/branch", "/switch", "/merge", "/stress", "/rules"].includes(c.cmd) ? " " : "")); ta.current?.focus(); }}>
              <span>{c.cmd}</span>
              <em>{c.help}</em>
            </div>
          ))}
        </div>
      )}
      <div className={`prompt ${p.status === "busy" ? "busy" : ""}`}>
        <span className={`dot ${p.status}`} title={`agent: ${p.status}`} />
        <textarea
          ref={ta}
          rows={1}
          value={text}
          spellCheck={false}
          placeholder={p.placeholder}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              if (suggestions.length === 1 && text !== suggestions[0].cmd && !text.includes(" ")) setText(suggestions[0].cmd);
              else submit();
            } else if (e.key === "Tab" && suggestions.length) {
              e.preventDefault();
              setText(suggestions[0].cmd + " ");
            } else if (e.key === "ArrowUp" && !text.includes("\n") && hist.length && (text === "" || hi >= 0)) {
              e.preventDefault();
              const i = hi < 0 ? hist.length - 1 : Math.max(0, hi - 1);
              setHi(i);
              setText(hist[i]);
            } else if (e.key === "ArrowDown" && hi >= 0) {
              e.preventDefault();
              const i = hi + 1;
              if (i >= hist.length) {
                setHi(-1);
                setText("");
              } else {
                setHi(i);
                setText(hist[i]);
              }
            } else if (e.key === "Escape") {
              (e.target as HTMLTextAreaElement).blur();
            }
          }}
        />
      </div>
      <div className="prompt-activity">{activity ?? (p.status === "offline" ? "agent offline — /agent restart" : "")}</div>
    </div>
  );
}
