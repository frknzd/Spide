import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { api, inTauri } from "./api";
import { modifiedKey } from "./terminal-keys";

/** Imperative, single ordered stream. No terminal bytes or keystrokes go through React. */
class TerminalSession {
  readonly terminal = new Terminal({ fontSize: 12, fontFamily: "Menlo, Monaco, monospace", scrollback: 10000, allowProposedApi: true,
    theme: { background: "#1b1e24", foreground: "#e5e7eb", cursor: "#a8c7ff", selectionBackground: "#435576" } });
  readonly fitAddon = new FitAddon();
  readonly host = document.createElement("div");
  private generation: number | null = null;
  private after = 0;
  private active = true;
  private modifyOtherKeys = false;
  private writes = Promise.resolve();
  private listeners = new Set<(error: string | null) => void>();
  private error: string | null = null;
  private refs = 0;
  private disposal?: ReturnType<typeof setTimeout>;
  private lastSize = "";
  private lastContext = "";
  tab = "class";
  private send(data: string | Uint8Array) {
    const generation = this.generation;
    if (generation === null) return;
    const bytes = Array.from(typeof data === "string" ? new TextEncoder().encode(data) : data);
    this.writes = this.writes.then(() => api.terminalWrite(generation, bytes)).catch((e) => this.notify(String(e)));
  }
  constructor() {
    this.host.className = "terminal-host";
    this.terminal.loadAddon(this.fitAddon);
    this.terminal.open(this.host);
    this.terminal.onData((s) => this.send(s));
    this.terminal.onBinary((s) => this.send(Uint8Array.from(s, (c) => c.charCodeAt(0))));
    this.terminal.parser.registerCsiHandler({ prefix: ">", final: "m" }, (p) => {
      if (p[0] !== 4) return false;
      this.modifyOtherKeys = p[1] === 2;
      return true;
    });
    this.terminal.attachCustomKeyEventHandler((e) => {
      // Browser print/model hotkeys must never steal pi input.
      if (e.ctrlKey && ["p", "l"].includes(e.key.toLowerCase())) e.preventDefault();
      const sequence = modifiedKey(e, this.modifyOtherKeys);
      if (sequence) { e.preventDefault(); this.send(sequence); return false; }
      return true;
    });
    this.host.addEventListener("copy", (e) => {
      if (!this.terminal.hasSelection()) return;
      e.clipboardData?.setData("text/plain", this.terminal.getSelection());
      e.preventDefault();
    });
    void this.poll();
  }
  private notify(error: string | null) {
    if (error === this.error) return;
    this.error = error;
    this.listeners.forEach((f) => f(error));
  }
  private async poll() {
    if (!inTauri) { this.notify("Native pi runs in the desktop app. Browser preview has no shell access."); return; }
    while (this.active) {
      try {
        const frame = await api.terminalRead(this.generation, this.after);
        if (!this.active) break;
        if (this.generation !== frame.generation || frame.reset) {
          this.terminal.reset();
          this.modifyOtherKeys = false;
          this.lastSize = "";
        }
        this.generation = frame.generation;
        if (frame.bytes.length) {
          // xterm's UTF-8 decoder holds split codepoints and its parser holds split ANSI sequences.
          await new Promise<void>((resolve) => this.terminal.write(Uint8Array.from(frame.bytes), resolve));
        }
        this.after = frame.next; // acknowledge only after rendering; backpressure stays end-to-end
        this.fit();
        this.context(this.tab);
        if (frame.exited) {
          this.notify("pi exited. Use Restart to start a new interactive session.");
          await new Promise((r) => setTimeout(r, 500));
        } else this.notify(null);
      } catch (e) {
        this.generation = null; this.after = 0;
        this.notify(String(e));
        await new Promise((r) => setTimeout(r, 500));
      }
    }
  }
  fit() {
    if (!this.host.isConnected || this.host.clientWidth < 20 || this.host.clientHeight < 20) return;
    const size = this.fitAddon.proposeDimensions();
    if (!size) return;
    const key = `${this.generation}:${size.cols}:${size.rows}`;
    if (key === this.lastSize || this.generation === null) return;
    this.lastSize = key;
    this.terminal.resize(size.cols, size.rows);
    void api.terminalResize(this.generation, size.cols, size.rows).catch((e) => this.notify(String(e)));
  }
  context(tab: string) {
    const key = `${this.generation}:${tab}`;
    if (this.generation !== null && key !== this.lastContext) {
      this.lastContext = key;
      void api.terminalContext(tab).catch((e) => { this.lastContext = ""; this.notify(String(e)); });
    }
  }
  attach(parent: HTMLElement, listener: (error: string | null) => void) {
    clearTimeout(this.disposal);
    this.refs++;
    parent.append(this.host);
    this.listeners.add(listener);
    listener(this.error);
    this.fit();
    return () => {
      this.listeners.delete(listener);
      this.refs--;
      // React StrictMode's immediate cleanup/re-attach reuses the same xterm/cursor/poll loop.
      this.disposal = setTimeout(() => {
        if (this.refs) return;
        this.active = false;
        this.terminal.dispose();
        singleton = undefined;
      }, 100);
    };
  }
}
let singleton: TerminalSession | undefined;
export const terminalSession = () => singleton ??= new TerminalSession();
