// Mirrors crates/spide-core (serde) types.

export type Status = "accepted" | "proposed" | "removing" | "pruned" | "overruled";
export type Visibility = "public" | "private" | "protected" | "package";
export type RelationKind = "association" | "aggregation" | "composition" | "inheritance" | "realization" | "dependency";
export type Layer = "ui" | "infrastructure" | "application" | "domain";

export interface Attribute {
  name: string;
  type: string;
  visibility: Visibility;
  is_static: boolean;
  multiplicity: string | null;
  status: Status;
  proposal: string | null;
}
export interface Param { name: string; type: string }
export interface Method {
  name: string;
  params: Param[];
  returns: string | null;
  visibility: Visibility;
  is_static: boolean;
  is_abstract: boolean;
  uses: string[];
  calls: string[];
  description: string | null;
  status: Status;
  proposal: string | null;
}
export interface Class {
  name: string;
  stereotype: string | null;
  package: string | null;
  is_abstract: boolean;
  attributes: Attribute[];
  methods: Method[];
  responsibilities: string[];
  rationale: string | null;
  status: Status;
  proposal: string | null;
  feedback: string | null;
}
export interface Relation {
  id: string;
  from: string;
  to: string;
  kind: RelationKind;
  label: string | null;
  from_mult: string | null;
  to_mult: string | null;
  directed: boolean;
  rationale: string | null;
  status: Status;
  proposal: string | null;
  feedback: string | null;
}
export interface Package { name: string; layer: Layer | null; description: string | null; status: Status; proposal: string | null }
export type Op = { op: string; [k: string]: unknown };
export interface Proposal { id: string; ops: Op[]; rationale: string | null; source: string }
export interface Tradeoff { force: string; effect: string; note: string | null }
export interface DecisionOption { id: string; label: string; summary: string | null; ops: Op[]; pros: string[]; cons: string[]; tradeoffs: Tradeoff[] }
export type ForceKind = "goal" | "quality" | "constraint" | "change";
export interface Force { name: string; kind: ForceKind; description: string | null; priority: number; status: Status; proposal: string | null }
export type Preset = "clean" | "hexagonal" | "layered" | "modular_monolith" | "none";
export interface DepRule { from: string; to: string; reason: string | null }
export interface TypeRule { layer: Layer; pattern: string; reason: string | null }
export interface Thresholds { god_members: number; god_responsibilities: number; high_fan_out: number; max_scenario_hops: number }
export interface Rules { preset: Preset; forbidden: DepRule[]; forbidden_types: TypeRule[]; thresholds: Thresholds }
export type PartnerMode = "examiner" | "socratic" | "devils_advocate" | "sketch";
export type DecisionState = "open" | "evaluating" | "rejected" | "accepted" | "overruled" | "dismissed";
export interface Attempt { option: string | null; custom: string | null; defense: string; verdict: string | null; feedback: string | null }
export interface Decision {
  id: string;
  question: string;
  context: string | null;
  options: DecisionOption[];
  state: DecisionState;
  chosen: string | null;
  custom: string | null;
  attempts: Attempt[];
  applied_ops: Op[];
}
export interface Actor { name: string; description: string | null; status: Status; feedback: string | null }
export interface UseCase {
  name: string;
  actors: string[];
  includes: string[];
  extends: string[];
  classes: string[];
  description: string | null;
  status: Status;
  feedback: string | null;
}
export interface UseCaseModel { system: string | null; actors: Actor[]; use_cases: UseCase[]; generated_rev: number | null }
export type MessageKind = "sync" | "async" | "return" | "create";
export interface Message {
  from: string;
  to: string;
  method: string;
  args: string | null;
  returns: string | null;
  kind: MessageKind;
  note: string | null;
  problem: string | null;
  status: Status;
  feedback: string | null;
}
export interface Sequence {
  name: string;
  use_case: string | null;
  participants: string[];
  messages: Message[];
  generated_rev: number | null;
  status: Status;
  feedback: string | null;
}
export interface Pos { x: number; y: number }
export interface Design {
  version: number;
  name: string;
  rev: number;
  model_rev: number;
  forces: Force[];
  classes: Class[];
  relations: Relation[];
  packages: Package[];
  proposals: Proposal[];
  decisions: Decision[];
  use_cases: UseCaseModel;
  sequences: Sequence[];
  feedback: { target: string; text: string; at: number }[];
  log: { rev: number; at: number; actor: string; summary: string }[];
  layout: Record<string, Record<string, Pos>>;
  settings: { examiner: "lenient" | "strict"; show_pruned: boolean; partner: PartnerMode; brief_skipped: boolean };
  rules: Rules;
}

export interface ClassMetrics {
  fan_in: number;
  fan_out: number;
  instability: number;
  lcom: number | null;
  members: number;
  in_cycle: boolean;
  god: boolean;
  orphan: boolean;
  layer_violations: number;
  heat: number;
  issues: string[];
}
export interface PackageMetrics {
  name: string;
  layer: Layer | null;
  classes: number;
  fan_in: number;
  fan_out: number;
  instability: number;
  abstractness: number;
  distance: number;
  in_cycle: boolean;
}
export interface PackageDep { from: string; to: string; count: number; in_cycle: boolean; layer_violation: boolean }
export interface ImplicitEdge { from: string; to: string; via: string; in_cycle: boolean }
export interface Issue { kind: string; severity: number; message: string; targets: string[]; rule: string | null; explain: string; fix_hint: string | null }
export interface Metrics {
  classes: Record<string, ClassMetrics>;
  packages: PackageMetrics[];
  package_deps: PackageDep[];
  cycles: string[][];
  cycle_edges: string[];
  package_cycles: string[][];
  layer_violations: string[];
  edge_rules: Record<string, string>;
  implicit_edges: ImplicitEdge[];
  issues: Issue[];
  health: number;
  tension: number;
}
export interface OptionPreview {
  decision: string;
  option: string;
  design: Design | null;
  health: number | null;
  error: string | null;
  focus: string[];
  impact: Impact | null;
}
export interface ClassDelta { class: string; fan_in: number; fan_out: number; heat: number }
export interface Impact {
  added: string[];
  changed: string[];
  removed: string[];
  health_before: number;
  health_after: number;
  new_issues: string[];
  resolved_issues: string[];
  flipped_deps: [string, string][];
  rule_breaks: string[];
  affected_scenarios: string[];
  class_deltas: ClassDelta[];
}
export interface DomainClass { name: string; stereotype: string | null; attributes: Attribute[]; status: Status }
export interface Derived {
  domain: { classes: DomainClass[]; relations: Relation[]; excluded: string[] };
  packages: { packages: { name: string; layer: Layer | null; classes: string[]; status: Status }[] };
  use_cases_stale: boolean;
  stale_sequences: string[];
  suggested_scenarios: string[];
}
export type PiStatus = "starting" | "idle" | "busy" | "offline";
export interface Snapshot {
  rev: number;
  view: Design;
  metrics: Metrics;
  ghost_health: number | null;
  ghost_metrics: Metrics | null;
  previews: OptionPreview[];
  proposal_impacts: { proposal: string; impact: Impact }[];
  derived: Derived;
  can_undo: boolean;
  can_redo: boolean;
  project_dir: string;
  pi_status: PiStatus;
  pi_error: string | null;
  mcp_url: string;
}

export type Target =
  | { kind: "class"; name: string }
  | { kind: "attribute"; class: string; name: string }
  | { kind: "method"; class: string; name: string }
  | { kind: "relation"; id: string }
  | { kind: "package"; name: string }
  | { kind: "actor"; name: string }
  | { kind: "use_case"; name: string }
  | { kind: "sequence"; name: string }
  | { kind: "message"; sequence: string; index: number }
  | { kind: "proposal"; id: string }
  | { kind: "force"; name: string };

export type Tab = "class" | "domain" | "package" | "usecase" | "sequence";

export type AgentEvent =
  | { kind: "user"; text: string }
  | { kind: "assistant_start" }
  | { kind: "text"; delta: string }
  | { kind: "assistant_end" }
  | { kind: "thinking" }
  | { kind: "tool"; name: string }
  | { kind: "tool_error"; name: string; text: string }
  | { kind: "status"; text: string }
  | { kind: "error"; text: string }
  | { kind: "settled" }
  | { kind: "exit" };

export function targetLabel(t: Target): string {
  switch (t.kind) {
    case "class": return t.name;
    case "attribute": return `${t.class}.${t.name}`;
    case "method": return `${t.class}.${t.name}()`;
    case "relation": return t.id.replace(/:.*/, "");
    case "package": return `package ${t.name}`;
    case "actor": return `actor ${t.name}`;
    case "use_case": return t.name;
    case "sequence": return t.name;
    case "message": return `message #${t.index + 1}`;
    case "proposal": return `proposal ${t.id}`;
    case "force": return `force ${t.name}`;
  }
}

export function targetKey(t: Target): string {
  return JSON.stringify(t);
}
