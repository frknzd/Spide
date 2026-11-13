import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Canvas, type CanvasApi } from "./canvas/Canvas";
import type { DiagramData } from "./canvas/DiagramContent";
import type { ViewportApi } from "./canvas/Viewport";
import { standaloneSvg } from "./export/svg";
import { api } from "./state/api";
import { on, useAgentFeed, useSnapshot } from "./state/store";
import type { Design, DriftReport, Tab, Target } from "./state/types";
import { AnswerBubble } from "./ui/AnswerBubble";
import { BriefPanel, MODES } from "./ui/BriefPanel";
import { HealthRing, ProposalBar, Tabs, Toasts, type Toast } from "./ui/Chrome";
import { ChangeSection } from "./ui/ChangeSection";
import { ComparePanel, decisionCols, type CompareCol } from "./ui/ComparePanel";
import { DecisionPanel } from "./ui/DecisionCard";
import { DecisionHistory } from "./ui/DecisionHistory";
import { DriftPanel } from "./ui/DriftPanel";
import { FeedbackPopover, type PruneState } from "./ui/FeedbackPopover";
import { HoverCard } from "./ui/HoverCard";
import { InlineEdit, type InlineEditState } from "./ui/InlineEdit";
import { PromptBar } from "./ui/PromptBar";
import { PerfOverlay } from "./ui/PerfOverlay";
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
  const snap = useSnapshot();
  const { entries, activity } = useAgentFeed();
  const [tab, setTab] = useState<Tab>(() => (DEV_QUERY?.get("tab") as Tab | null) ?? "class");
  const [hover, setHover] = useState<{ key: string; at: { x: number; y: number } } | null>(null);
  const [prune, setPrune] = useState<PruneState | null>(null);
  const [toasts, setToasts] = useState<Toast[]>([]);
  const [history, setHistory] = useState(false);
  const [brief, setBrief] = useState(false);
  const [compare, setCompare] = useState<string | null>(null);
  const [branchCmp, setBranchCmp] = useState<{ name: string; cols: CompareCol[] } | null>(null);
  const [stress, setStress] = useState<string | null>(null);
  const [shownDecision, setShownDecision] = useState<string | null>(null);
  const [timeline, setTimeline] = useState(false);
  const [drift, setDrift] = useState<DriftReport | null>(null);
  const [inline, setInline] = useState<InlineEditState | null>(null);
  const [c4Level, setC4Level] = useState<"context" | "container">("container");
  const [preview, setPreview] = useState<Design | null>(null);
  const [focusKey, setFocusKey] = useState(0);
  const [scenario, setScenario] = useState<string | null>(null);
  const canvasRef = useRef<CanvasApi>(null);
  const vpRef = useRef<ViewportApi | null>(null);
  const hoverTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const toastSeq = useRef(0);

  const toast = useCallback((kind: string, text: string) => setToasts((t) => [...t.slice(-3), { id: ++toastSeq.current, kind, text }]), []);
  const run = useCallback((p: Promise<unknown>) => p.catch((e) => toast("error", String(e))), [toast]);

  useEffect(() => {
    const un = on<{ kind: string; text: string }>("toast", (e) => toast(e.payload.kind, e.payload.text));
    return () => void un.then((f) => f());
  }, [toast]);

  const view = snap?.view;
  const metrics = snap?.metrics;
  const hasClasses = !!view && view.classes.length > 0;

  // ------------------------------------------------------------ actions
  const doPrune = useCallback(
    (targets: Target[], at: { x: number; y: number }) => {
      const proposals = targets.filter((t) => t.kind === "proposal") as { kind: "proposal"; id: string }[];
      // phantoms (code-only classes in the drift view) can't be pruned
      const rest = targets.filter((t) => t.kind !== "proposal" && !(t.kind === "class" && drift?.drift.extra_in_code.includes(t.name)));
      Promise.all([...proposals.map((p) => api.reject(p.id)), rest.length ? api.prune(rest) : Promise.resolve()])
        .then(() => setPrune({ targets, at }))
        .catch((e) => toast("error", String(e)));
      setHover(null);
    },
    [toast, drift],
  );

  const onClickTarget = useCallback(
    (t: Target) => {
      if (!view) return;
      if (t.kind === "proposal") return void run(api.accept(t.id));
      if (isPruned(view, t)) return void run(api.restore(t));
      const origin = originOf(view, t);
      if (origin && view.decisions.some((d) => d.id === origin && !["open", "evaluating", "rejected"].includes(d.state))) return setShownDecision(origin);
      if (t.kind === "use_case") {
        setScenario(t.name);
        setTab("sequence");
      }
    },
    [view, run],
  );

  // direct edits: double-click to rename, drop a class onto another package
  const onDoubleClickTarget = useCallback(
    (t: Target, at: { x: number; y: number }) => {
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
    [view, run],
  );
  const onMoveToPackage = useCallback(
    (cls: string, pkg: string) => run(api.userOps([{ op: "move_to_package", class: cls, package: pkg }]).then(() => toast("info", `moved ${cls} to ${pkg} (⌘Z to undo)`))),
    [run, toast],
  );

  const onHover = useCallback((key: string | null, at: { x: number; y: number }) => {
    if (hoverTimer.current) clearTimeout(hoverTimer.current);
    if (!key) {
      hoverTimer.current = setTimeout(() => setHover(null), 80);
      return;
    }
    hoverTimer.current = setTimeout(() => setHover({ key, at }), 260);
  }, []);

  const currentSvg = () => {
    const vp = tab === "class" || tab === "domain" ? canvasRef.current?.viewport() : vpRef.current;
    return vp ? standaloneSvg(vp, `${view?.name ?? "Design"} — ${TAB_TITLE[tab]}`) : null;
  };
  if (import.meta.env.DEV) (window as unknown as { spideSvg: () => string | null }).spideSvg = currentSvg;
  const exportPng = async () => {
    const svg = currentSvg();
    if (!svg) return toast("error", "nothing to export");
    const path = await saveDialog({ defaultPath: `${slug(view?.name)}-${tab}.png`, filters: [{ name: "PNG", extensions: ["png"] }] });
    if (!path) return;
    await run(api.exportPng(svg, path, 2).then(() => toast("info", `exported ${path}`)));
  };
  const exportPuml = async () => {
    const path = await saveDialog({ defaultPath: `${slug(view?.name)}-${tab}.puml`, filters: [{ name: "PlantUML", extensions: ["puml"] }] });
    if (!path) return;
    await run(api.exportPlantUml(tab, path).then(() => toast("info", `exported ${path}`)));
  };

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

  const command = async (text: string) => {
    const [cmd, ...args] = text.split(" ");
    const arg = args.join(" ").trim();
    switch (cmd) {
      case "/undo": return run(api.undo());
      case "/redo": return run(api.redo());
      case "/accept": return run(api.acceptAll());
      case "/reject": return run(Promise.all((view?.proposals ?? []).map((p) => api.reject(p.id))));
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
      case "/fit": return tab === "class" || tab === "domain" ? canvasRef.current?.fit() : vpRef.current?.fit();
      case "/pruned": return run(api.setShowPruned(!view?.settings.show_pruned));
      case "/strict": return run(api.setExaminer(true).then(() => toast("info", "examiner: strict — no overruling")));
      case "/lenient": return run(api.setExaminer(false).then(() => toast("info", "examiner: lenient — you may overrule")));
      case "/history": return setHistory((h) => !h);
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
        if (sub === "delete" && name) return run(api.branchDelete(name));
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
        return toast("error", "usage: /export png | puml | all | adr | archunit");
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

  // dev aid for automated UI checks: ?cmd=/brief&cmd=/find%20Loan runs commands once the first snapshot arrived
  const devRan = useRef(false);
  useEffect(() => {
    if (!DEV_QUERY || devRan.current || !snap) return;
    devRan.current = true;
    const cmds = DEV_QUERY.getAll("cmd");
    cmds.forEach((c, i) => setTimeout(() => command(c), 400 + i * 400));
  }); // eslint-disable-line react-hooks/exhaustive-deps

  const submit = (text: string) => {
    if (text.startsWith("/")) return void command(text);
    run(api.prompt(text, tab));
  };

  // ------------------------------------------------------------ keyboard
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const inField = (e.target as HTMLElement)?.closest?.("textarea, input");
      if (e.metaKey && !e.altKey) {
        const n = Number(e.key);
        if (n >= 1 && n <= 6) {
          e.preventDefault();
          setTab(TAB_ORDER[n - 1]);
          return;
        }
        if (e.key === "z" && !inField) {
          e.preventDefault();
          run(e.shiftKey ? api.redo() : api.undo());
          return;
        }
        if (e.key === "t") {
          e.preventDefault();
          setTab("class");
          setTimeline((t) => !t);
          return;
        }
        if (e.key === "b") {
          e.preventDefault();
          setBrief((b) => !b);
          return;
        }
        if (e.key === "j") {
          e.preventDefault();
          setHistory((h) => !h);
          return;
        }
        if (e.key === "s") {
          e.preventDefault();
          command("/save");
          return;
        }
        if (e.key === "o") {
          e.preventDefault();
          command("/open");
          return;
        }
        if (e.key === "e") {
          e.preventDefault();
          e.shiftKey ? exportPuml() : exportPng();
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
        setHistory(false);
        setHover(null);
      }
      if (!inField && !e.metaKey && !e.ctrlKey && e.key.length === 1) setFocusKey((k) => k + 1);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  // ------------------------------------------------------------ data per tab
  const classData: DiagramData | null = useMemo(() => {
    if (!view || !metrics) return null;
    const d = preview ?? view;
    if (drift && !preview) {
      // code-only classes join the canvas as phantoms
      const phantom = new Set(drift.drift.extra_in_code);
      const extra = drift.reality.classes.filter((c) => phantom.has(c.name));
      const names = new Set([...d.classes.map((c) => c.name), ...extra.map((c) => c.name)]);
      const rels = drift.reality.relations.filter((r) => (phantom.has(r.from) || phantom.has(r.to)) && names.has(r.from) && names.has(r.to));
      return { classes: [...d.classes, ...extra], relations: [...d.relations, ...rels], packages: d.packages, metrics, implicit: metrics.implicit_edges };
    }
    return { classes: d.classes, relations: d.relations, packages: d.packages, metrics, implicit: preview ? [] : metrics.implicit_edges };
  }, [view, metrics, preview, drift]);

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

  // the agent recorded forces → show them so the user can accept/reject
  const ghostForces = view?.forces.filter((f) => f.status === "proposed").length ?? 0;
  useEffect(() => {
    if (ghostForces) setBrief(true);
  }, [ghostForces]);

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

  const pendingCount = view?.decisions.filter((d) => d.state === "open" || d.state === "evaluating" || d.state === "rejected").length ?? 0;
  useEffect(() => {
    if (pendingCount) setTimeout(() => canvasRef.current?.fit(), 50);
  }, [pendingCount]);

  if (!snap || !view || !metrics) return <div className="app" />;

  const pending = view.decisions.filter((d) => d.state === "open" || d.state === "evaluating" || d.state === "rejected");
  const stale: Partial<Record<Tab, boolean>> = { usecase: snap.derived.use_cases_stale, sequence: snap.derived.stale_sequences.length > 0 };
  const placeholder = hasClasses ? "Tell the architect what to change…   ( / for commands )" : "What are we designing?  e.g. “let's start a class diagram for a library system”";

  return (
    <div className={`app ${hasClasses ? "" : "empty"}`}>
      <div className="titlebar" data-tauri-drag-region />
      <div className="stage">
        {tab === "class" && classData && (
          <Canvas ref={canvasRef} key="class" viewKey="class" data={classData} saved={view.layout["class"]} tension={preview ? 0 : metrics.tension}
            onPrune={doPrune} onClickTarget={onClickTarget} onHover={onHover} hoverKey={hover?.key ?? null} insetRight={pendingCount ? 450 : 0}
            stress={stressSets} drift={driftSets} onMoveToPackage={preview || drift ? undefined : onMoveToPackage} onDoubleClickTarget={preview ? undefined : onDoubleClickTarget} />
        )}
        {tab === "domain" && domainData && (
          <Canvas ref={canvasRef} key="domain" viewKey="domain" data={domainData} saved={view.layout["domain"] ?? view.layout["class"]} showMethods={false} tension={0}
            onPrune={doPrune} onClickTarget={onClickTarget} onHover={onHover} hoverKey={hover?.key ?? null} />
        )}
        {tab === "package" && <PackageView snap={snap} vpRef={vpRef} onPrune={doPrune} onHover={onHover} onClickTarget={onClickTarget} />}
        {tab === "system" && <SystemView snap={snap} vpRef={vpRef} onPrune={doPrune} onHover={onHover} onClickTarget={onClickTarget} level={c4Level} onLevel={setC4Level} />}
        {tab === "usecase" && <UseCaseView snap={snap} vpRef={vpRef} onPrune={doPrune} onHover={onHover} onClickTarget={onClickTarget} onRegenerate={() => run(api.regenerate("usecase"))} />}
        {tab === "sequence" && (
          <SequenceView snap={snap} vpRef={vpRef} onPrune={doPrune} onHover={onHover} onClickTarget={onClickTarget}
            selected={scenario} onSelect={setScenario} onGenerate={(s) => run(api.regenerate("sequence", s ?? undefined))} />
        )}
      </div>

      {hasClasses && (
        <>
          <div className="top-left" data-ui>
            <span className="design-name">{view.name}</span>
            <span className="branch-tag" title="active branch (/branch, /switch, /compare, /merge)">⎇ {snap.branch}</span>
            {view.settings.partner !== "examiner" && <span className="preview-tag mode-tag">{MODES.find((m) => m.mode === view.settings.partner)?.label}</span>}
            {preview && <span className="preview-tag">{timeline ? "past revision (read-only)" : "previewing option"}</span>}
            {stressSets && <span className="preview-tag stress-tag">what if: {stress} · <span style={{ color: "#ff5c6c" }}>impacted</span> · <span style={{ color: "#ffb454" }}>ripple</span></span>}
          </div>
          <Tabs tab={tab} onTab={setTab} stale={stale} />
        </>
      )}

      {drift && (
        <DriftPanel report={drift} onClose={() => setDrift(null)} onReimport={() => view.settings.code_dir && run(api.importCode(view.settings.code_dir).then(() => api.drift()).then(setDrift))}
          onFind={(c) => canvasRef.current?.focus(c)} run={run} toast={toast} />
      )}
      {brief && !drift && (
        <BriefPanel design={view} onClose={() => setBrief(false)} run={run}>
          <ChangeSection design={view} metrics={metrics} stress={stress} run={run}
            onStress={(n) => {
              setStress(n);
              if (n) setTab("class");
            }} />
          <RulesSection rules={view.rules} run={run} />
        </BriefPanel>
      )}
      {shownDecision && pending.length === 0 && <DecisionHistory design={view} id={shownDecision} onClose={() => setShownDecision(null)} />}
      {pending.length > 0 && <DecisionPanel decisions={pending} snap={snap} onPreview={setPreview} toast={toast} onCompare={setCompare} />}
      {compare && view.decisions.find((x) => x.id === compare && (x.state === "open" || x.state === "rejected" || x.state === "evaluating")) && (() => {
        const d = view.decisions.find((x) => x.id === compare)!;
        return <ComparePanel title={d.question} cols={decisionCols(d, snap)} snap={snap} onClose={() => setCompare(null)} />;
      })()}
      {branchCmp && (
        <ComparePanel kicker="compare branches" title={`${snap.branch} ↔ ${branchCmp.name}`} cols={branchCmp.cols} snap={snap} onClose={() => setBranchCmp(null)} />
      )}

      <div className="bottom">
        <ProposalBar count={view.proposals.length} onAccept={() => run(api.acceptAll())}
          onReject={() => run(Promise.all(view.proposals.map((p) => api.reject(p.id))))} />
        {timeline && <Timeline rev={snap.rev} onPreview={setPreview} onClose={() => setTimeline(false)} run={run} toast={toast} />}
        <AnswerBubble entries={entries} history={history} onCloseHistory={() => setHistory(false)} />
        <div className="prompt-row">
          {hasClasses && <HealthRing metrics={metrics} ghost={snap.ghost_health} onFix={(i) => run(api.fixIssue(i))} onFocus={focusIssue} />}
          <PromptBar onSubmit={submit} status={snap.pi_status} activity={activity} placeholder={placeholder} focusKey={focusKey} />
          {hasClasses && <div className="health-spacer" />}
        </div>
      </div>

      {inline && <InlineEdit s={inline} onClose={() => setInline(null)} />}
      {hover && !prune && <HoverCard hoverKey={hover.key} at={hover.at} snap={snap} design={preview ?? view} metrics={metrics} />}
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
      <Toasts toasts={toasts} onDone={(id) => setToasts((t) => t.filter((x) => x.id !== id))} />
      {snap.pi_status === "offline" && snap.pi_error && <div className="pi-error" data-ui>agent: {snap.pi_error}</div>}
    </div>
  );
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
