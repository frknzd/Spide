import { useSyncExternalStore } from "react";
import type { Class, Package, Relation } from "./types";

/** What a ghost proposal changes, and where it sits in the design. */
export interface ProposalFootprint {
  /** proposal ids in focus */
  ids: Set<string>;
  /** classes the proposal adds, removes or changes (members) */
  changed: Set<string>;
  /** existing classes a proposed relation connects to (context: where it plugs in) */
  anchors: Set<string>;
  relations: Set<string>;
  packages: Set<string>;
  /** human-readable change list */
  items: { sign: "+" | "−"; text: string }[];
}

const ghostly = (s: string) => s === "proposed" || s === "removing";

export function footprintOf(data: { classes: Class[]; relations: Relation[]; packages: Package[] }, ids: Set<string>): ProposalFootprint | null {
  const f: ProposalFootprint = { ids, changed: new Set(), anchors: new Set(), relations: new Set(), packages: new Set(), items: [] };
  const sign = (s: string) => (s === "removing" ? "−" : "+");
  const inFocus = (s: string, id: string | null) => ghostly(s) && !!id && ids.has(id);
  for (const c of data.classes) {
    if (inFocus(c.status, c.proposal)) {
      f.changed.add(c.name);
      f.items.push({ sign: sign(c.status), text: `class ${c.name}${c.package ? ` in ${c.package}` : ""}` });
    }
    for (const a of c.attributes) if (inFocus(a.status, a.proposal)) { f.changed.add(c.name); f.items.push({ sign: sign(a.status), text: `${c.name}.${a.name}` }); }
    for (const m of c.methods) if (inFocus(m.status, m.proposal)) { f.changed.add(c.name); f.items.push({ sign: sign(m.status), text: `${c.name}.${m.name}()` }); }
  }
  for (const r of data.relations) {
    if (!inFocus(r.status, r.proposal)) continue;
    f.relations.add(r.id);
    f.anchors.add(r.from); f.anchors.add(r.to);
    f.items.push({ sign: sign(r.status), text: `${r.from} → ${r.to} (${r.kind})` });
  }
  for (const p of data.packages) if (inFocus(p.status, p.proposal)) { f.packages.add(p.name); f.items.push({ sign: sign(p.status), text: `package ${p.name}` }); }
  for (const n of f.changed) f.anchors.delete(n);
  for (const c of data.classes) if ((f.changed.has(c.name) || f.anchors.has(c.name)) && c.package) f.packages.add(c.package);
  return f.items.length ? f : null;
}

/** The proposal behind a hovered canvas element, if it is a ghost. */
export function proposalAt(key: string | null, data: { classes: Class[]; relations: Relation[] }): string | null {
  if (!key) return null;
  const i = key.indexOf(":");
  const kind = key.slice(0, i), rest = key.slice(i + 1);
  if (kind === "class") { const c = data.classes.find((x) => x.name === rest); return c && ghostly(c.status) ? c.proposal : null; }
  if (kind === "attribute" || kind === "method") {
    const dot = rest.indexOf(".");
    const c = data.classes.find((x) => x.name === rest.slice(0, dot));
    const m = kind === "method" ? c?.methods.find((x) => x.name === rest.slice(dot + 1)) : c?.attributes.find((x) => x.name === rest.slice(dot + 1));
    return m && ghostly(m.status) ? m.proposal : null;
  }
  if (kind === "relation") { const r = data.relations.find((x) => x.id === rest); return r && ghostly(r.status) ? r.proposal : null; }
  return null;
}

/** Spotlight requested from outside the canvas (the proposal bar): "all" ghosts or one proposal id. */
let spotlight: string | null = null;
const subs = new Set<() => void>();
export const ghostSpotlight = {
  set(v: string | null) { if (v !== spotlight) { spotlight = v; subs.forEach((f) => f()); } },
  get: () => spotlight,
  subscribe(f: () => void) { subs.add(f); return () => { subs.delete(f); }; },
};
export const useGhostSpotlight = () => useSyncExternalStore(ghostSpotlight.subscribe, ghostSpotlight.get);

/** What the dock says about one proposal: its own rationale, and a factual change list (never invented). */
export interface ProposalSummary { id: string; source: string; rationale: string | null; items: { sign: "+" | "−"; text: string }[]; ops: string; changes: number }
export function proposalSummary(p: { id: string; ops: { op: string }[]; rationale: string | null; source: string }, data: { classes: Class[]; relations: Relation[]; packages: Package[] }): ProposalSummary {
  const f = footprintOf(data, new Set([p.id]));
  const counts = new Map<string, number>();
  for (const o of p.ops) counts.set(o.op, (counts.get(o.op) ?? 0) + 1);
  const ops = [...counts].map(([k, n]) => `${k.replace(/_/g, " ")}${n > 1 ? ` ×${n}` : ""}`).join(", ");
  return { id: p.id, source: p.source, rationale: p.rationale?.trim() || null, items: f?.items ?? [], ops, changes: f?.items.length || p.ops.length };
}
