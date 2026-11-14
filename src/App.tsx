import { ask, open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Canvas, type CanvasApi } from "./canvas/Canvas";
import type { DiagramData, Lens } from "./canvas/DiagramContent";
import type { ViewportApi } from "./canvas/Viewport";
import { standaloneSvg } from "./export/svg";
import { api } from "./state/api";
import { on, useSnapshot } from "./state/store";
import type { Design, DriftReport, ProposalReviewData, Tab, Target } from "./state/types";
import { Workspace } from "./ui/Workspace";
import { SpideCommandPalette } from "./ui/SpideCommandPalette";
import { ExportDialog, loadExportSettings, runExport, type ExportContext } from "./ui/ExportDialog";
import { inTauri } from "./state/api";
import type { LegendKey } from "./export/svg";
import { TerminalPane } from "./ui/TerminalPane";
import { BriefPanel, MODES } from "./ui/BriefPanel";
import { HealthRing, ProposalBar, Tabs, Toasts, type Toast } from "./ui/Chrome";
import { ChangeSection } from "./ui/ChangeSection";
import { ComparePanel, decisionCols, type CompareCol } from "./ui/ComparePanel";
import { DecisionPanel } from "./ui/DecisionCard";
import { DecisionHistory } from "./ui/DecisionHistory";
import { ProposalReview } from "./ui/ProposalReview";
import { DriftPanel } from "./ui/DriftPanel";
import { FeedbackPopover, type PruneState } from "./ui/FeedbackPopover";
import { HoverLayer } from "./ui/HoverLayer";
import { useSpideCommandBridge } from "./state/commands";
import { hoverStore } from "./state/hover";
import { reviewFootprint } from "./state/proposal-review";
import { ghostSpotlight, proposalAt } from "./state/proposal-focus";
import { InlineEdit, type InlineEditState } from "./ui/InlineEdit";
import { PerfOverlay } from "./ui/PerfOverlay";
import { DetailSlider } from "./ui/DetailSlider";
import { CodeView } from "./ui/CodeView";
import { CONCERN_COLOR } from "./state/insights";
import { DEFAULT_DETAIL, DETAIL_LABELS } from "./canvas/visuals";
import { perfCount } from "./state/perf";
import { Timeline } from "./ui/Timeline";
import { PRESETS, RulesSection } from "./ui/RulesSection";
import { PackageView } from "./views/PackageView";
import { SequenceView } from "./views/SequenceView";
import { SystemView } from "./views/SystemView";
import { UseCaseView } from "./views/UseCaseView";

const DEV_QUERY = import.meta.env.DEV ? new URLSearchParams(window.location.search) : null;
const TAB_ORDER: Tab[] = ["system", "class", "domain", "package", "usecase", "sequence"];
const TAB_TITLE: Record<Tab, string> = { system: "System (C4)", class: "Class diagram", domain: "Domain model", package: "Package diagram", usecase: "Use case diagram", sequence: "Sequence diagram" };

export default function App() {
  perfCount("App");
  const { snap, error: snapshotError, retry: retrySnapshot } = useSnapshot();
  const [commandsOpen, setCommandsOpen] = useState(false);
  const [commandText, setCommandText] = useState("");
  const openCommands = (text = "") => { setCommandText(text); setCommandsOpen(true); };
  const [tab, setTab] = useState<Tab>(() => (DEV_QUERY?.get("tab") as Tab | null) ?? "class");
  const [prune, setPrune] = useState<PruneState | null>(null);
  const [toasts, setToasts] = useState<Toast[]>([]);
  const [brief, setBrief] = useState(false);
  const [compare, setCompare] = useState<string | null>(null);
  const [branchCmp, setBranchCmp] = useState<{ name: string; cols: CompareCol[] } | null>(null);
  const [stress, setStress] = useState<string | null>(null);
  const [reviewActive, setReviewActive] = useState(false);
  const [selectedProposal, setSelectedProposal] = useState<string | null>(null);
  const [proposalCanvas, setProposalCanvas] = useState<Design | null>(null);
  const [proposalReviewData, setProposalReviewData] = useState<ProposalReviewData | null>(null);
  const proposalFocus = useMemo(() => reviewActive ? reviewFootprint(proposalReviewData, selectedProposal) : null, [reviewActive, proposalReviewData, selectedProposal]);
  const focusProposalTarget = (name: string) => { setTab("class"); window.setTimeout(() => { if (!canvasRef.current?.focus(name)) toast("info", "This target is not drawn on the class diagram. See the change details."); }, 150); };
  const [proposalDirty, setProposalDirty] = useState(false);
  const leaveProposalReview = () => !proposalDirty || window.confirm("Leave proposal review? Your draft will be kept locally; unfinished operation form edits may be lost.");
  const openProposalReview = (id?: string) => { if (id && id !== selectedProposal && !leaveProposalReview()) return; if (id) setSelectedProposal(id); setReviewActive(true); setPreview(null); setCompare(null); setBranchCmp(null); setDrift(null); setTimeline(false); setBrief(false); setInline(null); setPrune(null); window.dispatchEvent(new Event("spide-open-inspector")); };
  const [shownDecision, setShownDecision] = useState<string | null>(null);
  const [timeline, setTimeline] = useState(false);
  const [drift, setDrift] = useState<DriftReport | null>(null);
  const [inline, setInline] = useState<InlineEditState | null>(null);
  const [c4Level, setC4Level] = useState<"context" | "container">("container");
  const [preview, setPreview] = useState<Design | null>(null);
  const [previewSource, setPreviewSource] = useState<"option" | "history">("option");
  const optionPreview = useCallback((d: Design | null) => { setPreviewSource("option"); setPreview(d); }, []);
  const historyPreview = useCallback((d: Design | null) => { setPreviewSource("history"); setPreview(d); }, []);
  const [scenario, setScenario] = useState<string | null>(null);
  const [showImplicit, setShowImplicit] = useState(false);
  const [lens, setLens] = useState<Lens>("none");
  const [codeView, setCodeView] = useState<{ title: string; text: string } | null>(null);
  const canvasRef = useRef<CanvasApi>(null);
  const vpRef = useRef<ViewportApi | null>(null);
  const setHover = (_: null) => hoverStore.clear();
  const toastSeq = useRef(0);

  const toast = useCallback((kind: string, text: string) => setToasts((t) => [...t.slice(-3), { id: ++toastSeq.current, kind, text }]), []);
  const run = useCallback((p: Promise<unknown>) => p.then(() => true, (e) => { toast("error", String(e)); return false; }), [toast]);

  useEffect(() => {
    const un = on<{ kind: string; text: string }>("toast", (e) => toast(e.payload.kind, e.payload.text));
    return () => void un.then((f) => f());
  }, [toast]);

  const view = snap?.view;
  const metrics = snap?.metrics;
  const hasClasses = !!view && view.classes.length > 0;

  // ------------------------------------------------------------ actions
  const doPrune = useStableCallback(
    (targets: Target[], at: { x: number; y: number }) => {
      if (reviewActive) return toast("info", "Use the proposal editor to plan deletions; canvas editing is disabled during review.");
      const proposals = targets.filter((t) => t.kind === "proposal") as { kind: "proposal"; id: string }[];
      if (proposals.length) { openProposalReview(proposals[0].id); return; }
      // phantoms (code-only classes in the drift view) can't be pruned
      const rest = targets.filter((t) => t.kind !== "proposal" && !(t.kind === "class" && drift?.drift.extra_in_code.includes(t.name)));
      (rest.length ? api.prune(rest) : Promise.resolve())
        .then(() => setPrune({ targets, at }))
        .catch((e) => toast("error", String(e)));
      setHover(null);
    },
  );

  const onClickTarget = useStableCallback(
    (t: Target) => {
      if (!view) return;
      if (t.kind === "proposal") return openProposalReview(t.id);
      const key = t.kind === "class" ? `class:${t.name}` : t.kind === "relation" ? `relation:${t.id}` : t.kind === "attribute" || t.kind === "method" ? `${t.kind}:${t.class}.${t.name}` : null;
      const ghostPlan = proposalAt(key, view);
      if (ghostPlan) return openProposalReview(ghostPlan);
      if (reviewActive) return;
      if (isPruned(view, t)) return void run(api.restore(t));
      // clicking a member pins its card: evidence, similar members and findings can then be explored
      if (t.kind === "attribute" || t.kind === "method") hoverStore.pin(`${t.kind}:${t.class}.${t.name}`);
      const origin = originOf(view, t);
      if (origin && view.decisions.some((d) => d.id === origin && !["open", "evaluating", "rejected"].includes(d.state))) return setShownDecision(origin);
      if (t.kind === "use_case") {
        setScenario(t.name);
        setTab("sequence");
      }
    },
  );

  // direct edits: double-click to rename, drop a class onto another package
  const onDoubleClickTarget = useStableCallback(
    (t: Target, at: { x: number; y: number }) => {
      if (reviewActive) return;
      if (!view) return;
      const cls = (n: string) => view.classes.find((c) => c.name === n && c.status !== "pruned" && c.status !== "proposed");
      if (t.kind === "class" && cls(t.name)) {
        setInline({ at, label: "rename class", value: t.name, onSubmit: (v) => run(api.userOps([{ op: "rename_class", from: t.name, to: v }])) });
      } else if (t.kind === "attribute") {
        const a = cls(t.class)?.attributes.find((x) => x.name === t.name);
        if (a) setInline({ at, label: `rename ${t.class}.${t.name}`, value: a.name,
          onSubmit: (v) => run(api.userOps([{ op: "update_attribute", class: t.class, name: t.name, attribute: { ...a, name: v } }])) });
      } else if (t.kind === "method") {
        const m = cls(t.class)?.methods.find((x) => x.name === t.name);
        if (m) setInline({ at, label: `rename ${t.class}.${t.name}()`, value: m.name,
          onSubmit: (v) => run(api.userOps([{ op: "update_method", class: t.class, name: t.name, method: { ...m, name: v } }])) });
      }
      setHover(null);
    },
  );
  const onMoveToPackage = useCallback(
    (cls: string, pkg: string) => run(api.userOps([{ op: "move_to_package", class: cls, package: pkg }]).then(() => toast("info", `moved ${cls} to ${pkg} (⌘Z to undo)`))),
    [run, toast],
  );

  const onHover = hoverStore.move;

  const currentSvg = () => {
    const vp = tab === "class" || tab === "domain" ? canvasRef.current?.viewport() : vpRef.current;
    return vp ? standaloneSvg(vp, `${view?.name ?? "Design"} — ${TAB_TITLE[tab]}`) : null;
  };
  if (import.meta.env.DEV) (window as unknown as { spideSvg: () => string | null }).spideSvg = currentSvg;
  // ------------------------------------------------------------ export (File › Export…)
  const [exportOpen, setExportOpen] = useState(false);
  const legendFor = (): LegendKey[] => {
    if (!view || !metrics) return [];
    const keys = new Set<LegendKey>();
    if (tab === "class" || tab === "domain") {
      const rels = tab === "domain" ? snap!.derived.domain.relations : view.relations.filter((r) => r.status !== "pruned");
      for (const r of rels) keys.add(r.kind === "association" ? (r.directed ? "directed" : "association") : (r.kind as LegendKey));
      if (tab === "class" && metrics.cycle_edges.length) keys.add("cycle");
      if (tab === "class" && metrics.layer_violations.length) keys.add("violation");
    } else if (tab === "package") { keys.add("dependency"); if (metrics.package_deps.some((d) => d.in_cycle)) keys.add("cycle"); if (metrics.package_deps.some((d) => d.layer_violation)) keys.add("violation"); }
    else if (tab === "system") { keys.add("directed"); keys.add("dependency"); }
    else if (tab === "usecase") { keys.add("association"); keys.add("dependency"); }
    if (view.proposals.length) keys.add("proposal");
    const order: LegendKey[] = ["association", "directed", "inheritance", "realization", "dependency", "composition", "aggregation", "cycle", "violation", "proposal"];
    return order.filter((k) => keys.has(k));
  };
  const exportCtx: ExportContext = {
    project: snap?.project_dir ?? "",
    tab,
    baseName: `${slug(view?.name)}-${tab}`,
    viewport: () => (tab === "class" || tab === "domain" ? canvasRef.current?.viewport() ?? null : vpRef.current),
    meta: () => ({ title: view?.name ?? "Design", view: TAB_TITLE[tab], health: metrics?.health ?? null, legend: legendFor() }),
    prepare: async (o) => {
      if (tab === "class" || tab === "domain") canvasRef.current?.setExportMode({ plain: !o.diagnostics });
      // two frames: React commits the export state, then the routes/boxes are laid out
      await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
    },
    finish: () => canvasRef.current?.setExportMode(null),
  };
  /** Exports show the calm diagram: no hover, spotlight, focus or preview. */
  const calm = () => { hoverStore.clear(); ghostSpotlight.set(null); canvasRef.current?.focus(null); setPreview(null); };
  const openExport = () => { calm(); setExportOpen(true); };
  const exportAgain = async () => {
    const s = loadExportSettings(exportCtx.project);
    if (!s.lastPath) return openExport();
    calm();
    await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
    try { const p = await runExport(exportCtx, s, s.lastPath); if (p) toast("info", `exported ${p}`); }
    catch (e) { toast("error", String(e)); }
    finally { exportCtx.finish(); }
  };
  const quickExport = async (format: "png" | "puml") => {
    calm();
    await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
    const s = { ...loadExportSettings(exportCtx.project), format, scale: 2 } as const;
    try { const p = await runExport(exportCtx, s); if (p) toast("info", `exported ${p}`); }
    catch (e) { toast("error", String(e)); }
    finally { exportCtx.finish(); }
  };
  const exportPng = () => quickExport("png");
  const exportPuml = () => quickExport("puml");
  const menuActions = useRef({ openExport, exportAgain });
  menuActions.current = { openExport, exportAgain };
  useEffect(() => {
    const a = on("menu-export", () => menuActions.current.openExport());
    const b = on("menu-export-again", () => void menuActions.current.exportAgain());
    return () => { void a.then((f) => f()); void b.then((f) => f()); };
  }, []);

  const importCode = async () => {
    const dir = await openDialog({ directory: true, multiple: false, title: "Java source folder to compare with the design", defaultPath: view?.settings.code_dir ?? undefined });
    if (typeof dir !== "string") return;
    try {
      const s = await api.importCode(dir);
      toast("info", `imported ${s.classes} classes, ${s.relations} relations from ${s.files} files${s.skipped.length ? ` (${s.skipped.length} skipped)` : ""}`);
      setDrift(await api.drift());
      setTab("class");
      setBrief(false);
    } catch (e) {
      toast("error", String(e));
    }
  };
  // health popover: jump to what an issue is about
  const focusIssue = (i: { targets: string[] }) => {
    const t = i.targets[0];
    if (!t || !view) return;
    const cls = view.classes.find((c) => c.name === t)?.name ?? (t.includes("->") ? t.split("->")[0] : null);
    const go = () => {
      if (cls && view.classes.some((c) => c.name === cls)) canvasRef.current?.focus(cls);
      else if (view.packages.some((p) => p.name === t)) canvasRef.current?.focusPackage(t);
      else if (t.startsWith("sequence:")) {
        setScenario(t.slice(9));
        setTab("sequence");
      } else if (view.containers.some((k) => k.name === t)) setTab("system");
    };
    if (tab !== "class" && !t.startsWith("sequence:")) {
      setTab("class");
      setTimeout(go, 300);
    } else go();
  };
  const exportFolder = async (title: string, f: (dir: string) => Promise<string[]>, what: string) => {
    const dir = await openDialog({ directory: true, multiple: false, title, defaultPath: snap?.project_dir });
    if (typeof dir !== "string") return;
    await run(f(dir).then((files) => toast("info", `wrote ${files.length} ${what}${files.length === 1 ? "" : "s"} to ${dir}`)));
  };
  const exportAll = async () => {
    const dir = await openDialog({ directory: true, multiple: false, title: "Folder for the PlantUML files" });
    if (typeof dir !== "string") return;
    await run(api.exportPlantUmlAll(dir).then((files) => toast("info", `exported ${files.length} diagram${files.length === 1 ? "" : "s"} to ${dir}`)));
  };

  const activeViewport = () => (tab === "class" || tab === "domain" ? canvasRef.current?.viewport() : vpRef.current);
  const fitActive = () => activeViewport()?.fit();
  const command = async (text: string) => {
    try { return await executeCommand(text); }
    catch (e) { toast("error", String(e)); throw e; }
  };
  const executeCommand = async (text: string) => {
    // Intentional app commands can leave review; drafts stay persisted, never committed implicitly.
    if (reviewActive && !/^\/(accept|reject)( |$)/.test(text)) {
      if (!leaveProposalReview()) return;
      setReviewActive(false);
    }
    setPreview(null);
    const [cmd, ...args] = text.split(" ");
    const arg = args.join(" ").trim();
    switch (cmd) {
      case "/undo": return run(api.undo());
      case "/redo": return run(api.redo());
      case "/accept":
      case "/reject": {
        if (!arg || !view?.proposals.some((p) => p.id === arg)) return toast("error", `usage: ${cmd} <proposal-id> · review one saved plan at a time`);
        if (proposalDirty) return toast("error", "Save or discard the current draft before resolving proposals.");
        if (cmd === "/accept") {
          const review = await api.reviewProposal(arg);
          if (!review.can_accept) return toast("error", [...review.errors, ...review.prerequisites.map((id) => `Accept ${id} first`)].join("; ") || "This plan cannot be accepted yet.");
          return run(api.accept(arg, review.rev));
        }
        return run(api.reject(arg, snap?.rev));
      }
      case "/tidy": return canvasRef.current?.tidy();
      case "/focus": {
        if (!arg) return canvasRef.current?.focus(null);
        const pk = view?.packages.find((p) => p.name.toLowerCase() === arg.toLowerCase()) ?? view?.packages.find((p) => p.name.toLowerCase().includes(arg.toLowerCase()));
        if (!pk) return toast("error", `no package matches “${arg}”`);
        const go = () => canvasRef.current?.focusPackage(pk.name) || toast("error", `package ${pk.name} has no classes on the canvas`);
        if (tab === "class") return void go();
        setTab("class");
        return void setTimeout(go, 300);
      }
      case "/find": {
        if (!arg) return canvasRef.current?.focus(null);
        const name = findClass(view, arg);
        if (!name) return toast("error", `no class matches “${arg}”`);
        const go = () => canvasRef.current?.focus(name) || toast("error", `${name} is not on the canvas`);
        if (tab === "class") return void go();
        setTab("class");
        return void setTimeout(go, 300);
      }
      case "/detail": {
        const v = tab === "domain" ? "domain" : "class";
        const n = arg === "" ? NaN : Number(arg);
        const byName = DETAIL_LABELS.findIndex((l) => l.startsWith(arg.toLowerCase()));
        const lvl = Number.isInteger(n) && n >= 0 && n <= 4 ? n : arg && byName >= 0 ? byName : NaN;
        if (Number.isNaN(lvl)) return toast("error", `usage: /detail 0–4 (${DETAIL_LABELS.map((l, i) => `${i} ${l}`).join(" · ")})`);
        return run(api.setDetail(v, lvl));
      }
      case "/external": {
        if (!view || !arg) return toast("error", "usage: /external <package|Class> — toggles the external tag");
        const pk = view.packages.find((p) => p.name.toLowerCase() === arg.toLowerCase());
        if (pk) return run(api.userOps([{ op: "update_package", name: pk.name, external: !pk.external }]).then(() => toast("info", `${pk.name}: ${pk.external ? "ours again" : "external"}`)));
        const name = findClass(view, arg);
        const c = name ? view.classes.find((x) => x.name === name) : null;
        if (!c) return toast("error", `no package or class matches “${arg}”`);
        return run(api.userOps([{ op: "update_class", name: c.name, external: !c.external }]).then(() => toast("info", `${c.name}: ${c.external ? "ours again" : "external"}`)));
      }
      case "/code": {
        if (!arg) return toast("error", "usage: /code <Class>[.method] — the imported implementation");
        const [c, m] = arg.split(".");
        const name = findClass(view, c) ?? c;
        return api.codeSnippet(name, m || undefined).then((text) => setCodeView({ title: m ? `${name}.${m}()` : name, text }), (e) => toast("error", String(e)));
      }
      case "/lens": {
        const l = arg.startsWith("c") ? "concerns" : arg.startsWith("o") ? "overlap" : "none";
        setLens((cur) => (arg ? l : cur === "none" ? "concerns" : "none"));
        if (tab !== "class") setTab("class");
        return toast("info", l === "concerns" ? "lens: method rows coloured by what they do" : l === "overlap" ? "lens: duplicated behaviour and scattered concepts" : "lens off");
      }
      case "/implicit":
        setShowImplicit((x) => !x);
        return toast("info", showImplicit ? "implicit dependencies: only for the focused class" : "implicit dependencies: all shown");
      case "/fit": return fitActive();
      case "/pruned": return run(api.setShowPruned(!view?.settings.show_pruned));
      case "/strict": return run(api.setExaminer(true).then(() => toast("info", "examiner: strict — no overruling")));
      case "/lenient": return run(api.setExaminer(false).then(() => toast("info", "examiner: lenient — you may overrule")));
      case "/history": return toast("info", "Scroll pi's native transcript in the terminal pane.");
      case "/brief": return setBrief((b) => !b);
      case "/timeline":
        setTab("class");
        return setTimeline((t) => !t);
      case "/stress": {
        if (!arg) return setStress(null);
        const c = metrics?.changes.find((x) => x.name.toLowerCase().includes(arg.toLowerCase()));
        if (!c) return toast("error", `no change scenario matches “${arg}”`);
        if (!c.analysed) return toast("error", `“${c.name}” isn't analysed yet`);
        setTab("class");
        return setStress(c.name);
      }
      case "/branch":
      case "/branches": {
        const [sub, name] = arg.split(/\s+/);
        if (!arg || cmd === "/branches") {
          const bs = await api.branches().catch((e) => (toast("error", String(e)), []));
          return toast("info", "branches: " + bs.map((b) => `${b.active ? "● " : ""}${b.name} (${b.classes} classes, health ${b.health})`).join(" · "));
        }
        if (sub === "delete" && name) {
          if (!(await ask(`Delete branch “${name}”? This cannot be undone.`, { title: "Delete branch", kind: "warning", okLabel: "Delete", cancelLabel: "Cancel" }))) return;
          return run(api.branchDelete(name).then(() => toast("info", `deleted branch ${name}`)));
        }
        return run(api.branchCreate(arg));
      }
      case "/switch": return arg ? run(api.branchSwitch(arg)) : toast("error", "usage: /switch <branch>");
      case "/merge": return arg ? run(api.branchMerge(arg).then(() => toast("info", `changes from ${arg} proposed as ghosts`))) : toast("error", "usage: /merge <branch>");
      case "/compare": {
        if (arg) {
          const c = await api.branchCompare(arg).catch((e) => (toast("error", String(e)), null));
          if (!c || !view || !metrics) return;
          const focus = [...new Set(c.impact.added.concat(c.impact.changed, c.impact.removed).map((x) => x.replace(/^class /, "").split(/[.\s]/)[0]))];
          setTab("class");
          return setBranchCmp({
            name: arg,
            cols: [
              { id: "●", label: `${snap?.branch ?? "current"} (current)`, design: view, health: metrics.health, impact: null, focus },
              { id: "⎇", label: arg, design: c.design, health: c.health, impact: c.impact, focus },
            ],
          });
        }
        const d = view?.decisions.find((x) => (x.state === "open" || x.state === "rejected") && x.options.length > 1);
        return d ? setCompare(d.id) : toast("error", "no open decision with several options");
      }
      case "/rules": {
        if (!arg) return setBrief(true);
        const p = PRESETS.find((x) => x.preset === arg.replace(/[- ]/g, "_") || x.label === arg);
        if (!p) return toast("error", `presets: ${PRESETS.map((x) => x.preset).join(" | ")}`);
        return run(api.setRules({ ...view!.rules, preset: p.preset }).then(() => toast("info", `rules: ${p.label} — ${p.help}`)));
      }
      case "/mode": {
        if (!arg) return toast("info", `mode: ${view?.settings.partner} — ${MODES.map((m) => m.mode).join(" | ")}`);
        return run(api.setPartner(arg).then(() => toast("info", `partner mode: ${arg}`)));
      }
      case "/skip":
        return run(api.setBriefSkipped(true).then(() => api.prompt("I'd rather skip the brief for now. Let's start designing.", tab)));
      case "/stop": return run(api.abort());
      case "/agent": return run(api.restartAgent().then(() => toast("info", "agent restarted")));
      case "/folder": return run(api.reveal());
      case "/import": return importCode();
      case "/add": {
        const m = arg.match(/^(\S+)(?:\s+(?:in\s+)?(\S+))?$/);
        if (!m) return toast("error", "usage: /add <Class> [in <package>]");
        return run(api.userOps([{ op: "add_class", name: m[1], ...(m[2] ? { package: m[2] } : {}) }]).then(() => toast("info", `added ${m[1]}`)));
      }
      case "/drift": {
        if (drift) return setDrift(null);
        const r = await api.drift().catch((e) => (toast("error", String(e)), null));
        if (r) {
          setTab("class");
          setBrief(false);
          setDrift(r);
        }
        return;
      }
      case "/coderoot": return run(api.setCodeRoot(arg || null).then(() => toast("info", arg ? `code root: ${arg}` : "code root cleared")));
      case "/name": return arg ? run(api.setName(arg)) : toast("error", "usage: /name <title>");
      case "/export":
        if (arg.startsWith("ar")) {
          if (!view?.settings.code_root) toast("info", "tip: /coderoot org.your.app makes the rules match your Java packages exactly");
          const path = await saveDialog({ defaultPath: "ArchitectureTest.java", filters: [{ name: "Java", extensions: ["java"] }] });
          if (path) await run(api.exportArchUnit(path).then(() => toast("info", `exported ${path} — needs com.tngtech.archunit:archunit-junit5 as a test dependency`)));
          return;
        }
        if (arg.startsWith("ad")) return exportFolder("Folder for the ADRs (e.g. docs/adr)", api.exportAdr, "ADR");
        if (arg.startsWith("a")) return exportAll();
        if (arg.startsWith("p")) return arg.startsWith("png") ? exportPng() : exportPuml();
        if (!arg) return openExport();
        return toast("error", "usage: /export [png | puml | all | adr | archunit] — without an argument opens the export dialog");
      case "/save": {
        const path = await saveDialog({ defaultPath: `${slug(view?.name)}.json`, filters: [{ name: "Spide design", extensions: ["json"] }] });
        if (path) await run(api.saveAs(path).then(() => toast("info", `saved ${path}`)));
        return;
      }
      case "/open": {
        const path = await openDialog({ multiple: false, filters: [{ name: "Spide design", extensions: ["json"] }] });
        if (typeof path === "string") await run(api.open(path));
        return;
      }
      case "/new": {
        const dir = await openDialog({ directory: true, multiple: false, title: "Folder for the new design" });
        if (typeof dir === "string") await run(api.newDesign(dir));
        return;
      }
      default:
        toast("error", `unknown command ${cmd}`);
    }
  };

  useSpideCommandBridge(command, tab, snap?.project_dir);

  // dev aid for automated UI checks: ?cmd=/brief&cmd=/find%20Loan runs commands once the first snapshot arrived
  const devRan = useRef(false);
  useEffect(() => {
    if (!DEV_QUERY || devRan.current || !snap) return;
    devRan.current = true;
    const cmds = DEV_QUERY.getAll("cmd");
    cmds.forEach((c, i) => setTimeout(() => command(c), 400 + i * 400));
  }); // eslint-disable-line react-hooks/exhaustive-deps

  // ------------------------------------------------------------ keyboard
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.defaultPrevented || (e.target as HTMLElement)?.closest?.("[data-terminal]")) return;
      const inField = (e.target as HTMLElement)?.closest?.("textarea, input, select, [contenteditable=true]");
      if (inField) return;
      if (e.metaKey && e.key.toLowerCase() === "k") { e.preventDefault(); openCommands(); return; }
      if (e.metaKey && !e.altKey) {
        const n = Number(e.key);
        if (n >= 1 && n <= 6) {
          e.preventDefault();
          setTab(TAB_ORDER[n - 1]);
          return;
        }
        if (e.key === "z" && !inField) {
          e.preventDefault();
          void command(e.shiftKey ? "/redo" : "/undo");
          return;
        }
        if (e.key === "t") {
          e.preventDefault();
          if (reviewActive) { if (!leaveProposalReview()) return; setReviewActive(false); }
          setTab("class");
          setTimeline((t) => !t);
          return;
        }
        if (e.key === "b") {
          e.preventDefault();
          if (!reviewActive) setBrief((b) => !b);
          return;
        }
        if (e.key === "s") {
          e.preventDefault();
          void command("/save").catch(() => {});
          return;
        }
        if (e.key === "o") {
          e.preventDefault();
          void command("/open").catch(() => {});
          return;
        }
        if (e.key === "e" && !inTauri) {
          // in the desktop app File › Export… (⌘E) and Export Again (⇧⌘E) own these shortcuts
          e.preventDefault();
          if (e.shiftKey) void exportAgain(); else openExport();
          return;
        }
      }
      if (e.key === "Escape") {
        if (!inField) {
          canvasRef.current?.focus(null);
          setBrief(false);
          setStress(null);
          setShownDecision(null);
        }
        setPrune(null);
        setHover(null);
        ghostSpotlight.set(null);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  // ------------------------------------------------------------ data per tab
  const classData: DiagramData | null = useMemo(() => {
    if (!view || !metrics) return null;
    const d = (reviewActive ? proposalCanvas : null) ?? preview ?? view;
    if (drift && !preview && !reviewActive) {
      // code-only classes join the canvas as phantoms
      const phantom = new Set(drift.drift.extra_in_code);
      const extra = drift.reality.classes.filter((c) => phantom.has(c.name));
      const names = new Set([...d.classes.map((c) => c.name), ...extra.map((c) => c.name)]);
      const rels = drift.reality.relations.filter((r) => (phantom.has(r.from) || phantom.has(r.to)) && names.has(r.from) && names.has(r.to));
      return { classes: [...d.classes, ...extra], relations: [...d.relations, ...rels], packages: d.packages, metrics, implicit: metrics.implicit_edges };
    }
    return { classes: d.classes, relations: d.relations, packages: d.packages, metrics, implicit: preview || reviewActive ? [] : metrics.implicit_edges };
  }, [view, metrics, preview, drift, reviewActive, proposalCanvas]);

  const domainData: DiagramData | null = useMemo(() => {
    if (!snap) return null;
    const dm = snap.derived.domain;
    return {
      classes: dm.classes.map((c) => ({ ...c, package: null, is_abstract: false, methods: [], responsibilities: [], rationale: null, aggregate_root: false, proposal: null, feedback: null, origin: null })),
      relations: dm.relations,
      packages: [],
      metrics: null,
    };
  }, [snap]);

  // keep the drift report current while it's open
  useEffect(() => {
    if (drift) api.drift().then(setDrift).catch(() => {});
  }, [snap?.rev]); // eslint-disable-line react-hooks/exhaustive-deps
  const driftSets = useMemo(() => {
    if (!drift) return null;
    const d = drift.drift;
    return {
      missing: new Set(d.missing_in_code),
      drifted: new Set([...d.members.map((m) => m.class), ...d.packages.map((p) => p.class)]),
      phantom: new Set(d.extra_in_code),
    };
  }, [drift]);

  const stressSets = useMemo(() => {
    const c = stress ? metrics?.changes.find((x) => x.name === stress) : null;
    return c ? { impacted: new Set(c.impacted), ripple: new Set(c.ripple) } : null;
  }, [stress, metrics]);


  useEffect(() => { setPreview(null); hoverStore.clear(); }, [snap?.rev, snap?.project_dir, tab]);

  if (!snap || !view || !metrics) return <div className="app startup">
    <div className="titlebar" data-tauri-drag-region />
    <section className="startup-card" aria-live="polite"><span className="brand-mark">S</span><p className="eyebrow">SPIDE · DESIGN WORKSPACE</p>
      <h1>{snapshotError ? "Unable to open your workspace" : "Opening your workspace…"}</h1>
      <p>{snapshotError ?? "Connecting to your design. Your project stays on this Mac."}</p>
      {snapshotError ? <button className="primary" onClick={retrySnapshot}>Try again</button> : <span className="spinner" />}
    </section>
  </div>;
  const detailOf = (v: string) => view.settings.detail?.[v] ?? DEFAULT_DETAIL;

  const pending = view.decisions.filter((d) => d.state === "open" || d.state === "evaluating" || d.state === "rejected");
  const stale: Partial<Record<Tab, boolean>> = { usecase: snap.derived.use_cases_stale, sequence: snap.derived.stale_sequences.length > 0 };

  return (
    <div className={`app ${hasClasses ? "" : "empty"}`}>
      <div className="titlebar" data-tauri-drag-region />
      <Workspace pending={pending.length} terminal={(collapse) => <TerminalPane tab={tab} status={snap.pi_status} integrationError={snap.pi_error} onCollapse={collapse} />} inspector={
        <>
          <nav className="inspector-modes" aria-label="Inspector mode"><button aria-pressed={!reviewActive} onClick={() => { if (leaveProposalReview()) setReviewActive(false); }}>Decisions ({pending.length})</button><button aria-pressed={reviewActive} onClick={() => openProposalReview()}>Proposals ({view.proposals.length})</button></nav>
          <div hidden={!reviewActive}><ProposalReview snap={snap} selected={selectedProposal} onSelect={setSelectedProposal} onPreview={setProposalCanvas} onReviewData={setProposalReviewData} onFocus={focusProposalTarget} active={reviewActive} onDirty={setProposalDirty} /></div>
          {!reviewActive && (pending.length > 0 ? <DecisionPanel decisions={pending} snap={snap} onPreview={optionPreview} toast={toast} onCompare={setCompare} /> : shownDecision ? <DecisionHistory design={view} id={shownDecision} onClose={() => setShownDecision(null)} /> : <p className="inspector-empty">No open decisions.</p>)}
        </>
      }>
      <div className="stage">
        {tab === "class" && classData && (
          <Canvas ref={canvasRef} key={`${snap.project_dir}:${preview ? "preview" : "class"}`} readOnly={!!preview || reviewActive} reviewFocus={proposalFocus} viewKey="class" data={classData} saved={(preview ?? view).layout["class"]} tension={preview ? 0 : metrics.tension} level={detailOf("class")} showImplicit={showImplicit} lens={preview ? "none" : lens}
            onPrune={doPrune} onClickTarget={onClickTarget} onHover={onHover}
            stress={stressSets} drift={driftSets} onMoveToPackage={preview || drift || reviewActive ? undefined : onMoveToPackage} onDoubleClickTarget={preview || reviewActive ? undefined : onDoubleClickTarget} />
        )}
        {tab === "domain" && domainData && (
          <Canvas ref={canvasRef} key={`${snap.project_dir}:domain`} viewKey="domain" data={domainData} saved={view.layout["domain"] ?? view.layout["class"]} showMethods={false} tension={0} level={detailOf("domain")}
            onPrune={doPrune} onClickTarget={onClickTarget} onHover={onHover} />
        )}
        {tab === "package" && <PackageView snap={snap} vpRef={vpRef} onPrune={doPrune} onHover={onHover} onClickTarget={onClickTarget} />}
        {tab === "system" && <SystemView snap={snap} vpRef={vpRef} onPrune={doPrune} onHover={onHover} onClickTarget={onClickTarget} level={c4Level} onLevel={setC4Level} />}
        {tab === "usecase" && <UseCaseView snap={snap} vpRef={vpRef} onPrune={doPrune} onHover={onHover} onClickTarget={onClickTarget} onRegenerate={() => run(api.regenerate("usecase"))} />}
        {tab === "sequence" && (
          <SequenceView snap={snap} vpRef={vpRef} onPrune={doPrune} onHover={onHover} onClickTarget={onClickTarget}
            selected={scenario} onSelect={setScenario} onGenerate={(s) => run(api.regenerate("sequence", s ?? undefined))} />
        )}
      </div>

      <header className="workspace-chrome" data-ui>
        <>
          <div className="top-left" data-ui>
            <span className="design-name">{view.name}</span>
            <span className="branch-tag" title="active branch (/branch, /switch, /compare, /merge)">⎇ {snap.branch}</span>
            {view.settings.partner !== "examiner" && <span className="preview-tag mode-tag">{MODES.find((m) => m.mode === view.settings.partner)?.label}</span>}
            {preview && <span className="preview-tag">{previewSource === "history" ? "Past revision · read-only" : "Option preview · read-only"}</span>}
            {lens === "concerns" && (
              <span className="preview-tag lens-legend" title="/lens off">
                what methods do:{Object.entries(CONCERN_COLOR).map(([k, c]) => <span key={k}><i style={{ background: c }} />{k.replace("_", " ")}</span>)}
              </span>
            )}
            {lens === "overlap" && <span className="preview-tag" title="/lens off" style={{ color: "#c4b5fd" }}>overlap: dashed = duplicated behaviour · ⁂ = scattered concept</span>}
            {stressSets && <span className="preview-tag stress-tag">what if: {stress} · <span style={{ color: "#ff5c6c" }}>impacted</span> · <span style={{ color: "#ffb454" }}>ripple</span></span>}
          </div>
          <div className="workspace-actions">
            <button disabled={reviewActive} onClick={() => setBrief((v) => !v)} aria-expanded={brief}>Brief</button>
            <button disabled={reviewActive} onClick={() => { setTab("class"); setTimeline((v) => !v); }} aria-expanded={timeline}>Timeline</button>
            <button onClick={() => openCommands("find ")}>Find</button>
            <button className="commands-trigger" onClick={() => openCommands()} aria-expanded={commandsOpen}>Commands <kbd>⌘K</kbd></button>
          </div>
          <Tabs tab={tab} onTab={setTab} stale={stale} />
        </>
      </header>
      {commandsOpen && <div className="workspace-palette"><SpideCommandPalette key={commandText} initialText={commandText} onClose={() => setCommandsOpen(false)} /></div>}
      {snapshotError && <div className="connection-notice" role="alert">Updates unavailable · showing the last design <button onClick={retrySnapshot}>Retry</button></div>}
      {!hasClasses && tab === "class" && <section className="welcome" data-ui>
        <span className="eyebrow">YOUR NEXT DESIGN STARTS HERE</span><h1>Make the architecture clear.</h1>
        <p>Describe your goals to pi. Explore its proposals on the canvas, then choose and defend the design that fits.</p>
        <div className="welcome-steps"><span><b>01</b> Set the brief</span><span><b>02</b> Explore options</span><span><b>03</b> Defend your design</span></div>
        <div className="d-actions"><button className="primary" onClick={() => setBrief(true)}>Edit brief</button><button onClick={() => void command("/open").catch(() => {})}>Open design…</button><button onClick={() => window.dispatchEvent(new Event("spide-focus-terminal"))}>Focus pi →</button></div>
      </section>}

      {drift && (
        <DriftPanel report={drift} onClose={() => setDrift(null)} onReimport={() => view.settings.code_dir && run(api.importCode(view.settings.code_dir).then(() => api.drift()).then(setDrift))}
          onFind={(c) => canvasRef.current?.focus(c)} run={run} toast={toast} />
      )}
      {brief && !drift && !reviewActive && (
        <BriefPanel design={view} onClose={() => setBrief(false)} run={run}>
          <ChangeSection design={view} metrics={metrics} stress={stress} run={run}
            onStress={(n) => {
              setStress(n);
              if (n) setTab("class");
            }} />
          <RulesSection rules={view.rules} run={run} />
        </BriefPanel>
      )}
      {compare && view.decisions.find((x) => x.id === compare && (x.state === "open" || x.state === "rejected" || x.state === "evaluating")) && (() => {
        const d = view.decisions.find((x) => x.id === compare)!;
        return <ComparePanel title={d.question} cols={decisionCols(d, snap)} snap={snap} onClose={() => setCompare(null)} />;
      })()}
      {branchCmp && (
        <ComparePanel kicker="compare branches" title={`${snap.branch} ↔ ${branchCmp.name}`} cols={branchCmp.cols} snap={snap} onClose={() => setBranchCmp(null)} />
      )}

      <div className="canvas-toolbar" data-ui>
        <HealthRing metrics={metrics} ghost={snap.ghost_health} onFix={(i) => run(api.fixIssue(i))} onFocus={focusIssue} />
        {(tab === "class" || tab === "domain") && <DetailSlider level={detailOf(tab)} disabled={!!preview} onChange={(l) => run(api.setDetail(tab, l))} />}
        <div className="zoom-group" role="group" aria-label="Zoom"><button aria-label="Zoom out" title="Zoom out" onClick={() => activeViewport()?.zoomBy(0.8)}>−</button><button onClick={fitActive} title="Fit diagram · double-click canvas">Fit</button><button aria-label="Zoom in" title="Zoom in" onClick={() => activeViewport()?.zoomBy(1.25)}>+</button></div>
        {preview && <button className="preview-exit" onClick={() => { setPreview(null); if (previewSource === "history") setTimeline(false); }}>Exit preview</button>}

      </div>
      {view.proposals.length > 0 && <div className={`proposal-dock ${timeline ? "above-timeline" : ""}`}>
        <ProposalBar count={view.proposals.length}
          onReview={() => openProposalReview()} />
      </div>}
      {timeline && <div className="canvas-timeline"><Timeline key={`${snap.project_dir}:${snap.branch}`} rev={snap.rev} onPreview={historyPreview} onClose={() => setTimeline(false)} run={run} toast={toast} /></div>}
      {exportOpen && <ExportDialog ctx={exportCtx} onClose={() => setExportOpen(false)} onDone={(p) => toast("info", `exported ${p}`)} />}
      {codeView && <CodeView title={codeView.title} text={codeView.text} onClose={() => setCodeView(null)} />}
      {inline && <InlineEdit s={inline} onClose={() => setInline(null)} />}
      <HoverLayer scope={`${snap.project_dir}:${tab}`} disabled={!!prune} snap={snap} design={(reviewActive ? proposalCanvas : null) ?? preview ?? view} metrics={metrics} drift={drift} />
      {prune && (
        <FeedbackPopover
          prune={prune}
          onClose={() => setPrune(null)}
          onUndo={() => {
            setPrune(null);
            run(api.undo());
          }}
          onSend={(text) => {
            const targets = prune.targets;
            setPrune(null);
            run(api.pruneFeedback(targets, text, tab));
          }}
        />
      )}
      <PerfOverlay buildMs={snap.build_ms} />
      </Workspace>
      <Toasts toasts={toasts} onDone={(id) => setToasts((t) => t.filter((x) => x.id !== id))} />
    </div>
  );
}

/** A callback with a stable identity that always runs the latest closure (keeps memoised children from re-rendering). */
function useStableCallback<A extends unknown[], R>(f: (...a: A) => R): (...a: A) => R {
  const ref = useRef(f);
  ref.current = f;
  return useCallback((...a: A) => ref.current(...a), []);
}

/** Is the target currently pruned (so a click restores it)? */
function isPruned(d: Design, t: Target): boolean {
  const cls = (name: string) => d.classes.find((c) => c.name === name);
  const st = (() => {
    switch (t.kind) {
      case "class": return cls(t.name)?.status;
      case "attribute": return cls(t.class)?.attributes.find((a) => a.name === t.name)?.status;
      case "method": return cls(t.class)?.methods.find((m) => m.name === t.name)?.status;
      case "relation": return d.relations.find((r) => r.id === t.id)?.status;
      case "package": return d.packages.find((p) => p.name === t.name)?.status;
      case "actor": return d.use_cases.actors.find((a) => a.name === t.name)?.status;
      case "use_case": return d.use_cases.use_cases.find((u) => u.name === t.name)?.status;
      case "sequence": return d.sequences.find((s) => s.name === t.name)?.status;
      case "message": return d.sequences.find((s) => s.name === t.sequence)?.messages[t.index]?.status;
      case "system": return d.systems.find((x) => x.name === t.name)?.status;
      case "container": return d.containers.find((x) => x.name === t.name)?.status;
      case "link": return d.links.find((l) => `${l.from}->${l.to}` === t.id)?.status;
      default: return undefined;
    }
  })();
  return st === "pruned";
}

/** Origin (decision / proposal / user) of a clicked element. */
function originOf(d: Design, t: Target): string | null {
  const cls = (name: string) => d.classes.find((c) => c.name === name);
  switch (t.kind) {
    case "class": return cls(t.name)?.origin ?? null;
    case "attribute": return cls(t.class)?.attributes.find((a) => a.name === t.name)?.origin ?? null;
    case "method": return cls(t.class)?.methods.find((m) => m.name === t.name)?.origin ?? null;
    case "relation": return d.relations.find((r) => r.id === t.id)?.origin ?? null;
    default: return null;
  }
}

/** Best class-name match: exact, then prefix, then substring (case-insensitive). */
function findClass(d: Design | undefined, q: string): string | null {
  const names = (d?.classes ?? []).filter((c) => c.status !== "pruned").map((c) => c.name);
  const l = q.toLowerCase();
  return (
    names.find((n) => n.toLowerCase() === l) ??
    names.find((n) => n.toLowerCase().startsWith(l)) ??
    names.find((n) => n.toLowerCase().includes(l)) ??
    null
  );
}

function slug(s?: string) {
  return (s ?? "design").toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "") || "design";
}
