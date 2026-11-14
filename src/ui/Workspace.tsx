import { useEffect, useRef, useState, type CSSProperties, type ReactNode } from "react";
import { hoverStore } from "../state/hover";
import { ghostSpotlight } from "../state/proposal-focus";

interface Settings { left: number; right: number; leftOpen: boolean; rightOpen: boolean }
const defaults: Settings = { left: 420, right: 360, leftOpen: true, rightOpen: true };
function load(): Settings {
  try { const p = JSON.parse(localStorage.getItem("spide.workspace.v1") ?? "{}");
    return { left: Math.max(280, Math.min(900, Number(p.left) || 420)), right: Math.max(300, Math.min(900, Number(p.right) || 360)), leftOpen: p.leftOpen !== false, rightOpen: p.rightOpen !== false };
  } catch { return defaults; }
}
export function Workspace({ terminal, inspector, pending = 0, children }: { terminal: (collapse: () => void) => ReactNode; inspector: ReactNode; pending?: number; children: ReactNode }) {
  const root = useRef<HTMLDivElement>(null);
  const [settings, setSettings] = useState(load);
  const [width, setWidth] = useState(window.innerWidth);
  const narrow = width < 1120;
  const [drawer, setDrawer] = useState(false);
  useEffect(() => {
    const open = () => { setDrawer(true); setSettings((s) => ({ ...s, rightOpen: true })); };
    window.addEventListener("spide-open-inspector", open);
    return () => window.removeEventListener("spide-open-inspector", open);
  }, []);
  const leftOpen = settings.leftOpen && width >= 570;
  const rightOpen = narrow ? drawer : settings.rightOpen;
  const left = leftOpen ? Math.max(280, Math.min(settings.left, width - 252 - (!narrow && rightOpen ? 300 : 30))) : 30;
  const right = !narrow && rightOpen ? Math.max(300, Math.min(settings.right, width - left - 252)) : 30;
  useEffect(() => { try { localStorage.setItem("spide.workspace.v1", JSON.stringify(settings)); } catch {} }, [settings]);
  useEffect(() => {
    const observer = new ResizeObserver(([e]) => setWidth(e.contentRect.width));
    observer.observe(root.current!);
    return () => observer.disconnect();
  }, []);
  const resize = (side: "left" | "right", e: React.PointerEvent) => {
    e.preventDefault(); hoverStore.clear();
    const el = e.currentTarget as HTMLElement;
    el.setPointerCapture(e.pointerId);
    const initial = e.clientX;
    const start = side === "left" ? left : right;
    let next = start;
    const move = (ev: PointerEvent) => {
      const max = Math.max(side === "left" ? 280 : 300, width - (side === "left" ? right : left) - 260);
      next = Math.max(side === "left" ? 280 : 300, Math.min(max, start + (ev.clientX - initial) * (side === "left" ? 1 : -1)));
      // No React/model updates while dragging, and never call ELK/tidy on pane resize.
      root.current?.style.setProperty(`--${side}-width`, `${next}px`);
    };
    const up = () => {
      el.removeEventListener("pointermove", move); el.removeEventListener("pointerup", up); el.removeEventListener("pointercancel", up);
      setSettings((s) => ({ ...s, [side]: next }));
    };
    el.addEventListener("pointermove", move); el.addEventListener("pointerup", up); el.addEventListener("pointercancel", up);
  };
  const bounds = (side: "left" | "right") => { const min = side === "left" ? 280 : 300; return { min, max: Math.max(min, width - (side === "left" ? right : left) - 260) }; };
  const separator = (side: "left" | "right", visible: boolean) => { const b = bounds(side); const current = side === "left" ? left : right; return <div className={`workspace-separator ${visible ? "" : "inactive"}`} role="separator" aria-label={`Resize ${side === "left" ? "pi" : "decision"} pane`} aria-orientation="vertical" aria-valuemin={b.min} aria-valuemax={b.max} aria-valuenow={Math.round(current)} tabIndex={visible ? 0 : -1} onPointerDown={visible ? (e) => resize(side, e) : undefined} onKeyDown={(e) => {
    if (!["ArrowLeft", "ArrowRight"].includes(e.key)) return;
    e.preventDefault();
    setSettings((s) => ({ ...s, [side]: Math.max(b.min, Math.min(b.max, current + (e.key === "ArrowRight" ? 20 : -20) * (side === "left" ? 1 : -1))) }));
  }} />; };
  return <div ref={root} className={`workspace ${narrow ? "narrow" : ""}`} style={{ "--left-width": `${Math.max(30, left)}px`, "--right-width": `${right}px` } as CSSProperties}>
    <div className={`workspace-left ${leftOpen ? "" : "collapsed"}`}>
      {!leftOpen && <button className="pane-rail" aria-label="Open pi pane" onClick={() => setSettings((s) => ({ ...s, leftOpen: true }))}>pi ›</button>}
      <div className="terminal-container" style={{ display: leftOpen ? undefined : "none" }}>{terminal(() => setSettings((s) => ({ ...s, leftOpen: false })))}</div>
    </div>
    {separator("left", leftOpen)}
    <main className="canvas-workspace" onPointerDown={(e) => { if (!(e.target as Element).closest(".hovercard")) hoverStore.clear(); if (!(e.target as Element).closest(".proposal-wrap")) ghostSpotlight.set(null); }}>{children}</main>
    {separator("right", rightOpen && !narrow)}
    <aside className={`workspace-right ${rightOpen ? "open" : ""}`} aria-label="Design inspector">
      <button className="inspector-toggle pane-rail" onClick={() => narrow ? setDrawer((d) => !d) : setSettings((s) => ({ ...s, rightOpen: !s.rightOpen }))} aria-expanded={rightOpen} aria-label={rightOpen ? "Collapse design inspector" : `Open design inspector${pending ? `, ${pending} pending` : ""}`}>{rightOpen ? "›" : "‹"} {narrow || !rightOpen ? <>Review{pending > 0 && <span className="count-badge">{pending}</span>}</> : ""}</button>
      <div className="inspector-content" style={{ display: rightOpen ? undefined : "none" }}>{inspector || <div className="inspector-empty"><span className="empty-icon" aria-hidden>◇</span><h2>No open decisions</h2><p>When pi finds a design fork, the options appear here. Choose one and defend it to the examiner.</p></div>}</div>
    </aside>
  </div>;
}
