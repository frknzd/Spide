// Derived views of the diagnostics for the canvas and hover cards: which methods are in the wrong
// place (and where they belong), what each method does, and where behaviour overlaps.
import type { Metrics } from "./types";

export interface MethodMark { kind: string; home: string | null; message: string }

const PLACEMENT = new Set(["misplaced_responsibility", "duplication", "concept_elsewhere", "feature_envy", "ui_bypasses_controller", "demeter"]);

/** class → method → why it's flagged and its suggested home (best guess; suggest_home ranks properly). */
export function methodMarks(m: Metrics | null | undefined): Map<string, Map<string, MethodMark>> {
  const out = new Map<string, Map<string, MethodMark>>();
  if (!m) return out;
  const homes = new Map<string, string>();
  for (const f of m.responsibility?.findings ?? []) if (f.method && f.home) homes.set(`${f.class}.${f.method}`, f.home);
  for (const c of m.overlap?.claims ?? []) if (c.uses_no_own_data) homes.set(c.method, c.owner);
  for (const i of m.issues) {
    if (!PLACEMENT.has(i.kind)) continue;
    const methods = i.targets.filter((t) => t.includes("."));
    methods.forEach((t, k) => {
      const [cls, meth] = [t.slice(0, t.indexOf(".")), t.slice(t.indexOf(".") + 1)];
      let home = homes.get(t) ?? null;
      if (!home && i.kind === "feature_envy") home = i.targets[2] ?? null;
      if (!home && i.kind === "duplication") home = (methods[1 - k] ?? "").split(".")[0] || null;
      if (!home && i.fix_hint?.startsWith("move it to ")) home = i.fix_hint.slice(11).split(/[ ,(]/)[0];
      const per = out.get(cls) ?? new Map<string, MethodMark>();
      if (!per.has(meth)) per.set(meth, { kind: i.kind, home, message: i.message });
      out.set(cls, per);
    });
  }
  return out;
}

/** class → method → concern (presentation, domain_logic, …). */
export function methodConcerns(m: Metrics | null | undefined): Map<string, Map<string, string>> {
  const out = new Map<string, Map<string, string>>();
  for (const x of m?.responsibility?.methods ?? []) {
    const per = out.get(x.class) ?? new Map<string, string>();
    per.set(x.method, x.concern);
    out.set(x.class, per);
  }
  return out;
}

export const CONCERN_COLOR: Record<string, string> = {
  presentation: "#c084fc",
  coordination: "#60a5fa",
  domain_logic: "#34d399",
  persistence: "#f472b6",
  formatting: "#fbbf24",
  creation: "#a3e635",
  accessor: "#7b8294",
};

export interface OverlapLink { from: string; to: string; label: string; strength: number }

/** Class pairs that duplicate each other's behaviour, and methods dealing with another class's concept. */
export function overlapLinks(m: Metrics | null | undefined): OverlapLink[] {
  const out: OverlapLink[] = [];
  for (const c of m?.overlap?.classes ?? []) {
    const share = Math.max(c.share_a, c.share_b);
    if (c.pairs > 0) out.push({ from: c.a, to: c.b, label: `duplicate ${Math.round(share * 100)}%`, strength: share });
    else if (c.responsibility >= 0.25) out.push({ from: c.a, to: c.b, label: `same responsibilities ${Math.round(c.responsibility * 100)}%`, strength: c.responsibility });
  }
  for (const c of m?.overlap?.claims ?? []) {
    const cls = c.method.split(".")[0];
    if (cls !== c.owner && !out.some((l) => (l.from === cls && l.to === c.owner) || (l.to === cls && l.from === c.owner))) {
      out.push({ from: cls, to: c.owner, label: `${c.method.split(".")[1]}() → ${c.owner}'s “${c.shared.join(" ")}”`, strength: 0.5 });
    }
  }
  return out;
}

/** class → scattered concepts it takes part in. */
export function scatteredByClass(m: Metrics | null | undefined): Map<string, string[]> {
  const out = new Map<string, string[]>();
  for (const s of m?.overlap?.scattered ?? []) for (const c of s.classes) out.set(c, [...(out.get(c) ?? []), s.concept]);
  return out;
}
