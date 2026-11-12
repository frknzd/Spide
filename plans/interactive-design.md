# Spide 2 — interactive system design

Follows the MVP in `PLAN.md` (all steps done). The user asked for *all* directions from the
"interactive system design" discussion and delegated the design choices; those choices are recorded under
**Decisions** so they can be challenged at review.

## Context
Spide already does decision-centric, examiner-style class design with live metrics. Missing:
the *why* (forces), structured consequences of choices, behavioural/change validation, history beyond the in-memory
undo stack, levels above classes, project-specific architecture rules, low-friction direct edits, and a bridge to
real code. Goal: make Spide a complete loop: **brief → decide → see consequences → validate behaviour → evolve
(branch/rewind) → enforce in code**.

## Decisions (made on the user's behalf)
| # | Question | Decision | Why |
|---|---|---|---|
| D1 | Delivery | 8 phases, **one commit (or more) per phase**, each usable alone. Order: A forces/modes → F rules → B impact → C scenarios → D history/branches → H reality loop → E levels → G friction | Rules feed impact; impact feeds scenarios; history needs provenance; levels/friction are polish on top |
| D2 | Agent changes | Every new element kind (forces, change scenarios, systems, containers) is a typed `Op` → goes through the existing **proposal/decision** machinery | Keeps "agents only propose" invariant, undo, ghosts, diff, file watcher for free |
| D3 | Forces | Soft-required: on an empty design the agent first elicits a brief (one question at a time); user can `/skip`. Brief editable in a panel | Forces-first without blocking quick sketches |
| D4 | Partner modes | `examiner` (default, current behaviour), `socratic` (questions only, no ops), `devils_advocate` (argues against the user's pick before examining), `sketch` (may propose freely, decisions only for big forks). `/mode <name>` anytime; examiner strictness (strict/lenient) stays orthogonal | Course use wants examiner by default; others are prompt variants |
| D5 | Rules | Stored **in `design.json`** (`rules`), user-edited (panel + `/rules`), agent may only suggest. Presets: `clean` (current), `hexagonal`, `layered`, `modular_monolith`, `none` + custom forbidden-dependency rules + thresholds + forbidden types per layer | Rules are part of the design; watched + undoable like everything else |
| D6 | Layers | Keep the 4-variant `Layer` enum; the **preset decides the allowed directions** (replaces hard-coded `Layer::rank`). Modular monolith uses package prefixes as modules (`orders.domain`), cross-module deps only via `<module>.api` | Minimal model churn, all presets expressible |
| D7 | Quality attributes | Lightweight annotations, no deployment view: relation `interaction: sync|async|event`, class `aggregate_root: bool` (consistency boundary), container `replicas` (SPOF when 1 and depended on by ≥2) | Enough for useful checks without a new diagram type |
| D8 | Levels (C4) | Real **Context** and **Container** levels: new `systems` (external), `containers` (name, technology, replicas), `Package.container`; container deps are **derived** from class deps; one new tab **System** with a Context/Container toggle; Component = Package tab, Code = Class tab | C4 gives the zoom ladder; deriving container deps keeps one source of truth |
| D9 | History | Persist model snapshots per revision in `.spide/history.jsonl` (model-only, deduped, cap 5 000); timeline scrubber previews any rev read-only; **fork from any rev** | Undo stack is in-memory only today; JSONL is append-only and cheap |
| D10 | Branches | Separate files `.spide/branches/<name>.json`; `design.json` is always the active branch (switch = save + swap). Compare = side-by-side; merge = `diff()` → proposals on the active branch | Git-friendly, reuses `diff.rs` + proposals, watcher keeps working |
| D11 | Provenance | `origin: Option<String>` on classes, members, relations, packages (decision id / proposal id / `user` / `import`), set when committed | "why is this here?" one hover away |
| D12 | Code import / drift | **Java first**, source-only via `tree-sitter` + `tree-sitter-java` in a new crate `crates/spide-import` (keeps `spide-core` pure). Imported code is a separate read-only **reality** model, never merged silently; "adopt" goes through `diff()` → proposals | Java matches the ZAP work; tree-sitter needs no JVM |
| D13 | Architecture tests | Generate **ArchUnit (JUnit 5)** `ArchitectureTest.java` from rules + package→Java-package mapping (`settings.code_root`) | Same Java stack as D12 |
| D14 | ADRs | MADR-style markdown, one file per accepted/overruled decision, `docs/adr/NNNN-<slug>.md`, `/export adr` | Standard, readable in any repo |
| D15 | Schema | `SCHEMA_VERSION` 1 → 2; all new fields `#[serde(default)]`; v1 files load unchanged (test with `examples/library.json`) | No migration code needed |

## Approach

### Phase A — Forces, decisions-as-unit, partner modes (directions 1, 4)
- Model: `Design.brief: Brief { goals: Vec<Force>, qualities: Vec<Force{name, priority 1–3, scenario}>, constraints, expected_changes }`.
  Ops `add_force` / `update_force` / `remove_force` (kind = goal|quality|constraint|change).
- `DecisionOption.tradeoffs: Vec<{force, effect: "+"|"-", note}>`; DecisionCard shows ± chips per force.
- Examiner prompt (`prompts::examine`) includes the brief and the option trade-offs; defence judged against them.
- `Settings.partner: PartnerMode`; `prompts::user_prompt` prefixes the mode; skill gets a section per mode;
  `socratic` mode: `tools.rs` rejects `propose_ops`/`propose_decision` with a hint to ask instead.
- Design-level guidance in skill: boundaries → responsibilities → interactions → details; `get_design` reports the
  current "stage" (heuristic: no packages → boundaries; classes without methods → responsibilities, …).
- UI: **Brief panel** (left, ⌘B / `/brief`) listing forces, editable inline (user edits commit directly, origin `user`).

### Phase F — Explicit architecture rules (direction 7)
- New `rules.rs`: `Rules { preset, forbidden: Vec<DepRule{from, to (glob on package/class), reason}>, thresholds
  {god_members, god_responsibilities, high_fan_out, max_scenario_hops}, forbidden_types: Vec<{layer, pattern}> }`,
  `fn allowed(from_layer, to_layer, preset) -> Result<(), Explanation>`.
- `metrics::compute(d, ghosts)` reads `d.rules` instead of constants (`GOD_MEMBERS`, … become defaults) and
  `Layer::rank`; every `Issue` gains `rule: Option<String>`, `explain: String`, `fix_hint: Option<String>`.
- New issue kinds: `forbidden_dep`, `forbidden_type` (e.g. `Http*` in domain), `module_boundary`.
- UI: rules section in the Brief panel (preset picker, custom rules list, thresholds); issue rows get
  **"fix with agent"** → `prompts::fix_issue(issue)`. MCP: `get_metrics` includes rule ids + explanations.

### Phase B — Consequence previews (direction 2)
- New `impact.rs`: `impact(before: &Design, after: &Design) -> Impact { ops: diff(before, after), added/changed/removed
  targets, health Δ, new_issues, resolved_issues, flipped_deps (edge direction reversed), rule_breaks,
  affected_scenarios (sequences/change scenarios touching changed classes), metric deltas per touched class }`.
  Reuses `diff::diff`, `metrics::compute`, and the issue diff from `tools::health_delta` (moved into `impact.rs`).
- `OptionPreview.impact` and per-proposal impact in the snapshot; MCP `propose_*` responses use it too.
- UI: DecisionCard option gets an impact summary line + expandable list; **Compare** overlay (`/compare` or
  button) shows 2–3 option previews side by side at full height using `DiagramContent` + `highlight` (exists),
  differences coloured (added green, removed red, changed amber).

### Phase C — Behavioural validation (direction 3)
- `scenarios.rs`: `score(seq, d, m) -> ScenarioScore { hops, max_depth, package_crossings, layer_crossings,
  container_crossings, aggregates_touched (aggregate roots / entities receiving calls), unknown_calls }`;
  issue `chatty_scenario` when hops > threshold; shown in SequenceView header and scenario list.
- Change scenarios: `Design.change_scenarios: Vec<ChangeScenario{ name, description, impacted: Vec<{class, why}>,
  generated_rev }>`; ops `add_change_scenario`/`remove_change_scenario`; MCP tool `set_change_impact{name, impacted}`
  (agent analyses; Spide computes **ripple** = impacted ∪ classes depending on impacted, **locality** =
  1 − |ripple| / |classes|, packages/containers touched). Seeded from `brief.expected_changes`.
- `/stress <scenario>`: class canvas highlights the footprint (impacted solid, ripple faint), others dimmed.
- QA annotations (D7) + checks: `sync_chain` (sync calls crossing ≥ 2 containers), `spof`, `multi_aggregate_write`
  (scenario touches > 1 aggregate root in one call chain).
- Stale marking reuses `generated_rev` vs `model_rev` (as use cases/sequences do).

### Phase D — Provenance, history, branches (direction 5)
- `Mode::Commit(Status)` → `Mode::Commit(Status, Origin)`; `apply_all` stamps `origin` on new elements
  (7 call sites). `accept_proposal` → proposal id, `evaluate_defense`/`overrule` → decision id.
- HoverCard shows origin with the decision question + chosen option + verdict; click → opens decision history.
- History: `state::save` appends `{rev, at, actor, summary, model}` to `.spide/history.jsonl` when `model_rev`
  changed. Command `history_at(rev)` → design. UI `/timeline` (⌘T): scrubber above the prompt; scrubbing sets
  the existing read-only `preview` state in `App.tsx`; buttons "fork from here", "restore here" (= proposals via diff).
- Branches: `branches.rs` (core: compare/merge helpers) + tauri `branch_list/create/switch/delete/compare/merge`.
  `/branch <name>`, `/switch <name>`, `/compare <name>`, `/merge <name>`. Compare overlay reuses Phase B's.
  Top-left shows the branch name.

### Phase H — Close the loop with reality (direction 9)
- `adr.rs`: `adrs(d) -> Vec<(filename, markdown)>` (title, status, context = question + context + forces,
  options with pros/cons/trade-offs, decision + defence + examiner verdict, consequences = stored impact).
  `/export adr` → folder; golden test.
- `archunit.rs`: from rules + `settings.code_root` + package mapping → `ArchitectureTest.java`
  (`layeredArchitecture()` per preset, `noClasses()…dependOnClassesThat()` per forbidden rule, `slices()…beFreeOfCycles()`,
  forbidden types). `/export archunit`; golden test.
- `crates/spide-import`: `import_java(root) -> Design` (classes/interfaces/enums/records, fields → attributes +
  associations when typed by a project class, extends/implements, methods with params/returns, calls resolved
  through field/param types, package = last segment(s) mapped to design packages). Command `import_code(dir)`.
- Drift (`drift.rs` in core): `drift(intended, reality) -> Drift { missing_in_code, extra_in_code,
  relation_mismatches, rule_violations_in_code }`. UI `/drift`: class canvas overlay (missing = dashed red,
  extra = grey ghost) + report list; "adopt into design" → proposals via `diff()`; "update code" → copyable
  checklist. Reality cached in `.spide/reality.json`, re-import on demand.

### Phase E — Multi-level views (direction 6)
- Model (D8): `systems`, `containers`, `Package.container`, relation-like `links` (container/system ↔ system,
  `interaction`, `protocol`). Ops: `add_system`, `add_container`, `assign_container`, `add_link`, removes/updates.
- `derive::system_view(d, m)`: context = our system box + actors (from use cases) + external systems;
  container = containers + derived container deps (aggregate class deps via package→container) + links.
- New tab **System** (⌘1, other tabs shift: System, Class, Domain, Package, Use case, Sequence; ⌘0 fits) with
  Context/Container toggle; rendered with the existing `Viewport` + a small `SystemView.tsx` (like `PackageView`).
- Consistency issues (`consistency` kind): package without container (when containers exist), use case actor not
  in context, sequence participant crossing containers synchronously, container dep without link to external system.
- PlantUML: C4-PlantUML export for context/container.

### Phase G — Friction (direction 8)
- Direct user edits (commit immediately, origin `user`, undoable): new command `apply_user_ops(ops)` → `Store::mutate`.
  - drag a class onto another package box → `move_to_package`
  - double-click class/member name → inline rename (`rename_class` / `update_attribute` / `update_method`)
  - `/add <Class> [in <package>]` quick-add
- Layout: pin node (⌥-click) so physics doesn't move it; pins saved in layout; `/tidy` respects pins.
- Focus: `/find` (done) + `/focus <package>`; issue rows in health popover click → `/find` the target.

## Files
- core: `model.rs`, `ops.rs`, `store.rs`, `metrics.rs`, `derive.rs`, `tools.rs`, `plantuml.rs`, `diff.rs`,
  new `rules.rs`, `impact.rs`, `scenarios.rs`, `branches.rs`, `drift.rs`, `adr.rs`, `archunit.rs`
- new crate: `crates/spide-import` (workspace member)
- tauri: `commands.rs`, `state.rs` (snapshot + history append), `prompts.rs`, `main.rs`, new `history.rs`, `branches.rs`
- agent: `agent/spide-skill.md`
- UI: `App.tsx`, `state/{types,api}.ts`, `ui/{DecisionCard,HoverCard,Chrome,PromptBar}.tsx`, new
  `ui/{BriefPanel,ComparePanel,Timeline,DriftReport}.tsx`, `views/SystemView.tsx`, `views/SequenceView.tsx`,
  `canvas/{Canvas,DiagramContent,ClassNode}.tsx`
- docs: `README.md`, `PLAN.md` (link to this plan)

## Reuse
- `ops::apply_all` + `Mode` — single mutation path for all new element kinds (`crates/spide-core/src/ops.rs:581`)
- `Store::mutate` / `propose_ops` / `propose_decision` / `preview` — proposals, undo, previews (`store.rs`)
- `diff::diff` — impact ops, branch merge, history restore, drift adoption (`diff.rs:17`)
- `tools::health_delta` — issue diffing, moves into `impact.rs` (`tools.rs`)
- `metrics::compute` + `Issue` — extended, not replaced (`metrics.rs:106`)
- `generated_rev` vs `model_rev` staleness pattern (use cases/sequences) — for change scenarios
- `validate_sequence` / `has_method` — scenario scoring base (`store.rs`)
- `DiagramContent` `highlight`/`focusClass` props — compare, stress, drift overlays (`src/canvas/DiagramContent.tsx`)
- `Viewport` + `PackageView` pattern — `SystemView`; `Canvas.focus` / `Viewport.zoomTo` (added last round)
- `state::write_atomic`, watcher `last_written` — branch switching without self-triggered proposals
- PlantUML golden-test harness (`tests/export.rs`, `UPDATE_GOLDEN=1`) — ADR, ArchUnit, C4 goldens
- dev harness `/dev/invoke` — end-to-end checks of every new command

## Steps
**A — forces & modes**
- [x] A1 model `Brief`/`Force`, force ops, `DecisionOption.tradeoffs`, `Settings.partner`, schema v2 + v1 load test
- [x] A2 prompts: brief in examine/user prompts, mode prefixes, socratic tool guard; skill: modes, stages, brief elicitation
- [x] A3 MCP: forces in `get_design`, `tradeoffs` in `propose_decision` schema, stage hint
- [x] A4 UI: BriefPanel, trade-off chips on options, `/mode`, `/brief`, `/skip`
**F — rules**
- [x] F1 `rules.rs` + presets + tests per preset (allowed/forbidden matrices)
- [x] F2 metrics read rules; issue `rule`/`explain`/`fix_hint`; new issue kinds; existing metric tests still green
- [x] F3 rules UI in BriefPanel, `/rules`, "fix with agent"
**B — impact**
- [ ] B1 `impact.rs` (+ move `health_delta`), tests (cycle introduced/resolved, flipped dep, rule break)
- [ ] B2 impact in snapshot previews + proposals + MCP replies
- [ ] B3 DecisionCard impact line; ComparePanel
**C — scenarios**
- [ ] C1 `scenarios.rs` scoring + `chatty_scenario`; SequenceView header
- [ ] C2 change scenarios model/ops/MCP `set_change_impact`, ripple/locality, staleness, `/stress`
- [ ] C3 QA annotations + `sync_chain`/`spof`/`multi_aggregate_write` checks
**D — history & branches**
- [ ] D1 `Origin` in `Mode::Commit`, stamping, HoverCard provenance
- [ ] D2 `.spide/history.jsonl` append + `history_at`; Timeline UI (preview, fork, restore)
- [ ] D3 branches (create/switch/list/delete/compare/merge) + commands + UI
**H — reality loop**
- [ ] H1 `adr.rs` + `/export adr` + golden
- [ ] H2 `archunit.rs` + `settings.code_root` + `/export archunit` + golden
- [ ] H3 `crates/spide-import` (tree-sitter-java) + fixture project tests
- [ ] H4 `drift.rs` + `/drift` overlay/report + adopt-as-proposals
**E — levels**
- [ ] E1 systems/containers/links model + ops + MCP docs
- [ ] E2 `derive::system_view`, consistency issues
- [ ] E3 SystemView tab (Context/Container), tab renumbering, C4-PlantUML export + golden
**G — friction**
- [ ] G1 `apply_user_ops` command; drag-to-package; inline rename; `/add`
- [ ] G2 pinning; issue → focus; `/focus <package>`
**Wrap-up**
- [ ] README (features, commands, files), PLAN.md link, skill review

## Verification
- Per phase: `cargo test --workspace` (new unit tests per module + goldens), `npm run typecheck`, no warnings.
- Backward compatibility: v1 `examples/library.json` loads; round-trip equals v2 with defaults.
- End-to-end per phase via the dev harness (temp project, `SPIDE_PI=/usr/bin/false`, `curl /dev/invoke/...`), as done
  for `/export all`: e.g. set preset `layered` → metrics change; branch create/switch/merge → design.json and
  `.spide/branches/` contents; history scrub → `history_at`; export ADR/ArchUnit → files match goldens.
- Import: fixture Java project in `crates/spide-import/tests/fixtures/library/` (classes, interface, enum, cycle,
  layer violation) → expected design; smoke run (read-only) on one `zap-extensions/addOns/<addon>` for robustness/speed
  (< 2 s), no files written there.
- ArchUnit: golden text; if `javac` + an ArchUnit jar are available locally, compile the generated test against the fixture
  (no downloads without asking).
- Agent flows (manual, with real pi): brief elicitation on an empty design; each partner mode; examiner cites forces;
  `set_change_impact`; "fix with agent" on a rule violation.
- UI manual pass: Brief panel, compare overlay, timeline scrub/fork, System tab, drift overlay, drag-to-package, inline rename.

## Risks
- Scope is large: phases are independent commits so work can stop after any phase with a coherent product.
- `tree-sitter` adds a C build step (Xcode CLT is present).
- History file growth: model-only snapshots, dedupe identical models, cap + rotate.
- Agent prompt bloat from brief/rules: `get_design` stays compact (top forces, active rules only).
