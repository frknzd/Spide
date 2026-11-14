import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { useEffect, useMemo, useRef, useState } from "react";
import type { ViewportApi } from "../canvas/Viewport";
import { buildExportSvg, DEFAULT_EXPORT, type ExportMeta, type ExportOptions } from "../export/svg";
import { api } from "../state/api";
import type { Tab } from "../state/types";
import { useOverlay } from "./overlay";

export type ExportFormat = "png" | "svg" | "pdf" | "puml" | "json" | "adr" | "archunit";
const FORMATS: { id: ExportFormat; label: string; help: string; ext: string; image?: boolean }[] = [
  { id: "png", label: "PNG", help: "Pixel image, sharp at any scale you choose", ext: "png", image: true },
  { id: "svg", label: "SVG", help: "Vector image for the web and design tools", ext: "svg", image: true },
  { id: "pdf", label: "PDF", help: "Vector document with selectable text, for print and reports", ext: "pdf", image: true },
  { id: "puml", label: "PlantUML", help: "Text diagram of this view (PlantUML lays it out itself)", ext: "puml" },
  { id: "json", label: "Design JSON", help: "The whole design, to open in Spide again", ext: "json" },
  { id: "adr", label: "ADRs", help: "One Markdown decision record per decided decision", ext: "" },
  { id: "archunit", label: "ArchUnit", help: "A JUnit test that enforces your architecture rules in Java", ext: "java" },
];

export interface ExportSettings { format: ExportFormat; options: ExportOptions; scale: number; lastPath?: string | null }
const key = (project: string) => `spide.export.v1:${project}`;
export function loadExportSettings(project: string): ExportSettings {
  try {
    const s = JSON.parse(localStorage.getItem(key(project)) ?? "null");
    if (s && FORMATS.some((f) => f.id === s.format)) return { format: s.format, options: { ...DEFAULT_EXPORT, ...s.options }, scale: [1, 2, 3, 4].includes(s.scale) ? s.scale : 2, lastPath: s.lastPath ?? null };
  } catch { /* defaults */ }
  return { format: "png", options: DEFAULT_EXPORT, scale: 2, lastPath: null };
}
const saveSettings = (project: string, s: ExportSettings) => { try { localStorage.setItem(key(project), JSON.stringify(s)); } catch { /* private mode */ } };

export interface ExportContext {
  project: string;
  tab: Tab;
  baseName: string;
  viewport: () => ViewportApi | null;
  meta: () => ExportMeta;
  /** put the view in export state (full detail, diagnostics on/off) and wait for it to render */
  prepare: (o: ExportOptions) => Promise<void>;
  /** back to the interactive state */
  finish: () => void;
}

/** Run one export with the given settings; asks for a path unless `path` is given. Returns the written path (null: cancelled). */
export async function runExport(ctx: ExportContext, s: ExportSettings, path?: string | null): Promise<string | null> {
  const f = FORMATS.find((x) => x.id === s.format)!;
  const devPath = import.meta.env.DEV ? (window as unknown as { __spideSavePath?: string }).__spideSavePath : undefined;
  if (f.id === "adr") {
    const dir = path ?? devPath ?? (await openDialog({ directory: true, multiple: false, title: "Folder for the ADRs (e.g. docs/adr)", defaultPath: ctx.project }));
    if (typeof dir !== "string") return null;
    await api.exportAdr(dir);
    return dir;
  }
  const target = path ?? devPath ?? (await saveDialog({ defaultPath: `${ctx.baseName}.${f.ext}`, filters: [{ name: f.label, extensions: [f.ext] }] }));
  if (!target) return null;
  if (f.image) {
    await ctx.prepare(s.options);
    const vp = ctx.viewport();
    const out = vp && buildExportSvg(vp, ctx.meta(), s.options);
    if (!out) throw new Error("This view has nothing to export yet.");
    if (f.id === "png") await api.exportPng(out.svg, target, s.scale);
    else if (f.id === "svg") await api.exportSvg(out.svg, target);
    else await api.exportPdf(out.svg, target);
  } else if (f.id === "puml") await api.exportPlantUml(ctx.tab, target);
  else if (f.id === "json") await api.saveAs(target);
  else await api.exportArchUnit(target);
  return target;
}

export function ExportDialog({ ctx, onClose, onDone }: { ctx: ExportContext; onClose: () => void; onDone: (path: string) => void }) {
  useOverlay(onClose);
  useEffect(() => ctx.finish, []); // eslint-disable-line react-hooks/exhaustive-deps
  const [s, setS] = useState<ExportSettings>(() => loadExportSettings(ctx.project));
  const [preview, setPreview] = useState<{ url: string; width: number; height: number } | null>(null);
  const [state, setState] = useState<{ kind: "idle" | "busy" | "done" | "error"; text?: string; path?: string }>({ kind: "idle" });
  const f = FORMATS.find((x) => x.id === s.format)!;
  const set = (patch: Partial<ExportSettings>) => setS((old) => { const next = { ...old, ...patch }; saveSettings(ctx.project, next); return next; });
  const opt = (patch: Partial<ExportOptions>) => set({ options: { ...s.options, ...patch } });
  const first = useRef<HTMLButtonElement>(null);
  useEffect(() => first.current?.focus(), []);

  // live preview of exactly what will be written (image formats)
  const optsKey = JSON.stringify(s.options);
  useEffect(() => {
    if (!f.image) return setPreview(null);
    let alive = true;
    const id = setTimeout(async () => {
      await ctx.prepare(s.options);
      if (!alive) return;
      const vp = ctx.viewport();
      const out = vp && buildExportSvg(vp, ctx.meta(), s.options);
      setPreview(out ? { url: "data:image/svg+xml;charset=utf-8," + encodeURIComponent(out.svg), width: out.width, height: out.height } : null);
    }, 120);
    return () => { alive = false; clearTimeout(id); };
  }, [optsKey, f.image]); // eslint-disable-line react-hooks/exhaustive-deps

  const size = useMemo(() => {
    if (!preview) return null;
    if (s.format === "png") return `${Math.round(preview.width * s.scale)} × ${Math.round(preview.height * s.scale)} px`;
    if (s.format === "pdf") return `${Math.round((preview.width / 96) * 25.4)} × ${Math.round((preview.height / 96) * 25.4)} mm page`;
    return `${preview.width} × ${preview.height} px canvas`;
  }, [preview, s.format, s.scale]);

  const go = async () => {
    if (state.kind === "busy") return;
    setState({ kind: "busy" });
    try {
      const path = await runExport(ctx, s);
      if (!path) return setState({ kind: "idle" });
      set({ lastPath: path });
      setState({ kind: "done", path });
      onDone(path);
    } catch (e) {
      setState({ kind: "error", text: String(e) });
    }
  };

  const toggle = (k: keyof ExportOptions, label: string, help?: string) => (
    <label className="ex-check" title={help}>
      <input type="checkbox" checked={!!s.options[k]} onChange={(e) => opt({ [k]: e.target.checked } as Partial<ExportOptions>)} /> {label}
    </label>
  );
  const seg = <T extends string | number>(label: string, value: T, options: { v: T; label: string }[], onChange: (v: T) => void) => (
    <div className="ex-row">
      <span className="ex-label">{label}</span>
      <div className="ex-seg" role="radiogroup" aria-label={label}>
        {options.map((o) => <button type="button" key={String(o.v)} role="radio" aria-checked={value === o.v} className={value === o.v ? "on" : ""} onClick={() => onChange(o.v)}>{o.label}</button>)}
      </div>
    </div>
  );

  return (
    <div className="ex-backdrop" data-ui onPointerDown={(e) => { if (e.target === e.currentTarget) onClose(); }}>
      <section className="ex-sheet" role="dialog" aria-modal="true" aria-labelledby="ex-title">
        <header className="ex-head">
          <div><span className="dp-kicker">Export</span><h2 id="ex-title">{ctx.meta().view}</h2></div>
          <button className="icon-button" aria-label="Close export" onClick={onClose}>×</button>
        </header>
        <div className="ex-body">
          <div className="ex-preview" aria-live="polite">
            {f.image ? (preview ? <img src={preview.url} alt={`Preview of the ${f.label} export`} className={s.options.theme === "light" ? "paper" : ""} /> : <span className="faint">Nothing to export in this view yet.</span>)
              : <div className="ex-noimg"><b>{f.label}</b><p>{f.help}</p></div>}
            {size && f.image && <span className="ex-size">{size}</span>}
          </div>
          <div className="ex-form">
            <div className="ex-formats" role="radiogroup" aria-label="Format">
              {FORMATS.map((x, i) => (
                <button key={x.id} ref={i === 0 ? first : undefined} type="button" role="radio" aria-checked={s.format === x.id} className={s.format === x.id ? "on" : ""} title={x.help} onClick={() => set({ format: x.id })}>
                  {x.label}
                </button>
              ))}
            </div>
            <p className="ex-help">{f.help}</p>
            {f.image && <>
              {seg("Theme", s.options.theme, [{ v: "light", label: "Light (print)" }, { v: "dark", label: "Dark" }], (v) => opt({ theme: v }))}
              {s.format !== "pdf" && seg("Background", s.options.background, [{ v: "solid", label: "Solid" }, { v: "transparent", label: "Transparent" }], (v) => opt({ background: v }))}
              {s.format === "png" && seg("Scale", s.scale, [1, 2, 3, 4].map((v) => ({ v, label: `${v}×` })), (v) => set({ scale: v }))}
              {seg("Margins", s.options.padding, [{ v: "compact", label: "Compact" }, { v: "comfortable", label: "Comfortable" }], (v) => opt({ padding: v }))}
              <div className="ex-row ex-toggles">
                <span className="ex-label">Include</span>
                <div className="ex-checks">
                  {toggle("title", "Title")}
                  {toggle("date", "Date")}
                  {toggle("health", "Health score")}
                  {toggle("legend", "Legend", "Notation and colours used in this view")}
                  {toggle("packages", "Package frames")}
                  {toggle("ghosts", "Ghost proposals", "Proposed but not yet accepted changes")}
                  {toggle("diagnostics", "Diagnostics", "Heat halos, issue chips, member finding markers, cycle glow and rule warnings. Off: clean UML shapes")}
                </div>
              </div>
            </>}
          </div>
        </div>
        <footer className="ex-foot">
          {state.kind === "error" && <span className="form-error" role="alert">{state.text}</span>}
          {state.kind === "done" && <span className="ex-ok" role="status">Saved {state.path!.split("/").pop()} <button className="link" onClick={() => void api.revealPath(state.path!)}>Reveal in Finder</button></span>}
          <span className="ex-spacer" />
          <button onClick={onClose}>{state.kind === "done" ? "Done" : "Cancel"}</button>
          <button className="primary" disabled={state.kind === "busy" || (f.image && !preview)} onClick={() => void go()}>{state.kind === "busy" ? "Exporting…" : f.id === "adr" ? "Choose folder…" : "Export…"}</button>
        </footer>
      </section>
    </div>
  );
}
