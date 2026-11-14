// Derived views of the diagnostics for the canvas and hover cards: which methods are in the wrong
// place (and where they belong), what each method does, and where behaviour overlaps.
import type { Class, Metrics } from "./types";

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

export type MemberKind = "attribute" | "method";
export type IndicatorFamily = "complexity" | "duplication" | "responsibility" | "coupling" | "encapsulation" | "architecture" | "inheritance" | "uml";
export interface MemberIssue { kind: string; family: IndicatorFamily; severity: number; confidence: number; basis: string; message: string; explain: string; fix_hint: string | null; source?: string }
export interface MemberDiagnosis { issues: MemberIssue[]; severity: number; family: IndicatorFamily }

export const FAMILY_LABEL: Record<IndicatorFamily, string> = {
  complexity: "complexity", duplication: "duplication", responsibility: "responsibility", coupling: "coupling",
  encapsulation: "encapsulation", architecture: "architecture", inheritance: "inheritance", uml: "UML",
};
export const FAMILY_ICON: Record<IndicatorFamily, string> = {
  complexity: "≋", duplication: "⧉", responsibility: "↗", coupling: "⛓", encapsulation: "◇", architecture: "▦", inheritance: "△", uml: "□",
};

const FAMILY: Record<string, IndicatorFamily> = {
  long_method: "complexity", long_params: "complexity", type_switch: "complexity",
  duplication: "duplication", misplaced_responsibility: "responsibility", concept_elsewhere: "responsibility", feature_envy: "responsibility", creator: "responsibility",
  ui_bypasses_controller: "architecture", layer: "architecture", module_boundary: "architecture", forbidden_dep: "architecture", forbidden_type: "architecture",
  demeter: "coupling", exposed_state: "encapsulation", narrowed_visibility: "inheritance", inherited_repeated: "inheritance", missing_types: "uml",
};
/** Findings that unambiguously name a field or an operation when an attribute and method share a name. */
const ATTRIBUTE_KINDS = new Set(["exposed_state", "inherited_repeated"]);

/** class → `attribute:name` / `method:name` → findings that explicitly target that member. Class-only findings are never spread to rows. */
export function memberDiagnostics(m: Metrics | null | undefined, design: { classes: Class[] }): Map<string, Map<string, MemberDiagnosis>> {
  const out = new Map<string, Map<string, MemberDiagnosis>>();
  if (!m) return out;
  const classes = new Map(design.classes.map((c) => [c.name, c]));
  const add = (cls: string, key: string, issue: MemberIssue) => {
    const per = out.get(cls) ?? new Map<string, MemberDiagnosis>();
    const d = per.get(key) ?? { issues: [], severity: 0, family: issue.family };
    if (d.issues.some((x) => x.kind === issue.kind && x.message === issue.message)) return;
    d.issues.push(issue);
    d.issues.sort((a, b) => b.severity * b.confidence - a.severity * a.confidence);
    d.severity = Math.max(d.severity, issue.severity * issue.confidence);
    d.family = d.issues[0].family;
    per.set(key, d); out.set(cls, per);
  };
  for (const i of m.issues) {
    for (const t of new Set(i.targets)) {
      const dot = t.indexOf(".");
      if (dot <= 0) continue;
      const cls = t.slice(0, dot), name = t.slice(dot + 1).replace(/\(\)$/, "");
      const c = classes.get(cls);
      if (!c) continue;
      const attr = c.attributes.some((a) => a.name === name), meth = c.methods.some((x) => x.name === name);
      let kind: MemberKind | null = attr && !meth ? "attribute" : meth && !attr ? "method" : null;
      if (attr && meth) kind = ATTRIBUTE_KINDS.has(i.kind) ? "attribute" : FAMILY[i.kind] ? "method" : null;
      if (!kind) continue;
      add(cls, `${kind}:${name}`, {
        kind: i.kind, family: FAMILY[i.kind] ?? (i.principle === "coupling" ? "coupling" : i.principle === "architecture" ? "architecture" : i.principle === "uml" ? "uml" : "responsibility"),
        severity: i.severity, confidence: i.confidence ?? 1, basis: i.basis ?? "structure", message: i.message, explain: i.explain, fix_hint: i.fix_hint, source: i.source,
      });
    }
  }
  return out;
}

/** Plain-language strength label: confirmed structure vs potential concern. */
export function evidenceLabel(i: Pick<MemberIssue, "basis" | "confidence">): string {
  if (i.basis === "structure" && i.confidence >= 0.95) return "structural finding";
  if (i.basis === "code") return "measured in code";
  return "potential concern";
}
