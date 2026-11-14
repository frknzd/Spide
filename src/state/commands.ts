import { useEffect, useLayoutEffect, useRef } from "react";
import { api } from "./api";
import { on } from "./store";

// One dispatcher, shared by pi's /spide command and the offline-capable palette.
let handler: ((text: string) => Promise<unknown>) | undefined;
export function dispatchSpideCommand(text: string) {
  if (!handler) return Promise.reject(new Error("Workspace is not ready"));
  return handler(text.startsWith("/") ? text : "/" + text);
}

export { COMMANDS, SPIDE_COMMANDS, normalizeCommand, type CommandInfo } from "./command-catalog";

export function useSpideCommandBridge(command: (text: string) => Promise<unknown>, tab: string, project?: string) {
  const latest = useRef(command);
  useLayoutEffect(() => { latest.current = command; });
  useEffect(() => {
    const f = (text: string) => latest.current(text);
    handler = f;
    const seen = new Set<string>();
    const un = on<{ id: string; generation: number; project: string; command: string }>("spide-command", ({ payload: p }) => {
      if (p.project !== project) return;
      const key = `${p.generation}:${p.id}`;
      if (seen.has(key)) return;
      seen.add(key);
      if (seen.size > 256) seen.delete(seen.values().next().value!);
      void f(p.command).catch(console.error);
    });
    return () => { if (handler === f) handler = undefined; void un.then((f) => f()); };
  }, [project]);
  useEffect(() => { void api.terminalContext(tab).catch(console.error); }, [tab, project]);
}
