# Spide — live, agent-driven architecture canvas (MVP)

## Context
Greenfield personal tool in `Spide/` (empty; gets its own local `git init`), used heavily for the DOSS course.
Goal: *live-architect* a system with AI agents, with immediate visual feedback on how clean the design is. **Main focus: the class diagram**; other diagrams are derived from it.

### Requirements (confirmed)
- Desktop app: **Tauri v2** (Rust core + webview UI).
- Start screen: blank dark-grey canvas, a single prompt box bottom-centre. The prompt is the **only** way to add things.
- **Agents**: Spide spawns the user's normal `pi` as `pi --mode rpc` for chat, and runs an **MCP server** whose tools are the only way agents change the design. Any other agent (e.g. a terminal pi) can join through the same MCP server or by editing the watched `design.json` file.
- **Examiner-style decisions**: agent proposes a *Decision* with 2–3 options, each with a **live preview** mini-canvas. User picks one or writes their own, then **defends** it; the agent evaluates the defence; only then is it applied.
- **Living diagram**: coupling (fan-in/out), cycles, god classes / low cohesion, layering violations, orphans, all shown immediately. Node size + heat glow, red straining cycle edges, physics that settles when clean and jitters when tangled.
- **Hover** card: attributes, methods, responsibilities, rationale, metrics.
- **Prune**: right-click an element, or hold right-click + drag a marquee. Then an inline "what didn't you like / what do you want?" box sends feedback to the agent.
- **Tabs**: Class (main), Domain, Package, Use Case, Sequence; prune + feedback everywhere.
- **Persistence/export**: JSON save/load; PlantUML (semantically exact, Spide-like styling + layout hints); PNG pixel-exact via `resvg` from the same SVG the canvas renders. No code generation.

### Defaults chosen (open to change)
- Frontend: **TypeScript + React + Vite** (faster iteration for rich visuals than Rust/WASM).
- Examiner strictness: setting `strict | lenient`; default lenient = after a rejected defence you may **overrule** and the item is marked `overruled ⚠` in the log. Strict = must revise.
- Sequence diagrams: agent proposes a scenario list (derived from use cases); user can also name one in the prompt.
- Domain model: derived from the class diagram (conceptual classes, attributes, associations + multiplicities; no methods; technical stereotypes like `«controller»`, `«repository»`, `«service»` filtered out).

## Approach

### Architecture
```
┌─────────────── Tauri app ────────────────┐
│ Webview (TS/React)                       │
│  Canvas (SVG) · Tabs · Prompt · Decision │
│  cards · Hover cards · Prune marquee     │
│          ▲ events      │ commands        │
│ Rust core ─────────────┴─────────────────│
│  DesignStore (model + op log + undo)     │
│  Metrics · Derivations · Exporters       │
│  MCP server (rmcp, HTTP 127.0.0.1:PORT)  │◄── pi / other agents (MCP tools)
│  Pi bridge (spawn `pi --mode rpc`)       │──► pi (prompts, events)
│  File watcher (design.json)              │◄── agents editing the file
└──────────────────────────────────────────┘
```
- **Rust is the single source of truth.** UI only sends user intents (`prompt`, `prune`, `choose_option`, `defend`, `overrule`, `undo`) and renders snapshots/diffs pushed via Tauri events.
- **Pi bridge**: on startup spawns `pi --mode rpc --session-dir <project>/.spide/sessions` in the project folder; writes `.pi/mcp.json` pointing at Spide's MCP URL (`spide` server) and a Spide system prompt / skill describing the examiner workflow. Bottom prompt → RPC `prompt`; assistant text streams into a fading answer bubble above the prompt (no permanent chat pane, keeping the UI minimal; a toggle shows history).
- **MCP tools** (agents' only write path):
  - read: `get_design`, `get_metrics`, `get_view(kind)`, `get_pending_decisions`
  - write (always become *proposals*): `propose_decision{question, options[{label, ops[]} ]}`, `propose_ops{ops[], rationale}` (small, unambiguous additions still shown as ghosts needing a one-click accept)
  - examiner: `evaluate_defense{decision_id, verdict: accept|reject, feedback}`
  - derived: `set_use_cases{...}`, `set_sequence{scenario, steps[]}` (agent-generated views)
  - Ops are typed: `add_class`, `add_attribute`, `add_method`, `add_relation{kind: association|aggregation|composition|inheritance|realization|dependency, multiplicities, navigability}`, `move_to_package`, `rename`, `remove`…
- **File path**: `<project>/design.json` is watched; external edits are diffed into ops and treated as proposals too (never silently applied).

### Model (`spide-core`)
- `Design { classes, relations, packages, decisions, use_cases, actors, sequences, layers, log }`
- `Class { id, name, stereotype, package, attributes[], methods[], responsibilities[], rationale, status }`, `status ∈ proposed | accepted | pruned | overruled`
- `Decision { id, question, options[], chosen, custom, defense, verdict, feedback }`
- Op log append-only → undo/redo, replay animation, and "why" history per element.
- Pruned items kept (faded, toggle to show) with the user's feedback attached.

### Metrics engine (Rust, recomputed on each op, < 5 ms target)
- Fan-in/out and instability `I = Ce/(Ca+Ce)` per class/package.
- Cycles: Tarjan SCC on dependency graph (class + package level).
- God class: members / responsibilities / fan-out thresholds; cohesion via LCOM4 (methods↔attributes usage declared by agent in `method.uses[]`).
- Layering: packages tagged with layer (`ui > application > domain > infrastructure`); violations = upward deps.
- Orphans: zero relations.
- Overall **health score** 0–100 shown as a subtle ring near the prompt.

### Visual layer (TS)
- Rendering: SVG with `d3-force` physics (custom forces: package clustering, layer bands, edge springs); `elkjs` for initial placement + "tidy" action. Motion via `framer-motion` / spring interpolation.
- Mappings: node scale ∝ coupling; heat glow (blue→amber→red); cycle edges red, thicker, with tension animation; jitter amplitude ∝ unhealthiness; proposals as translucent dashed ghosts that materialise on accept; pruned = strike + fade.
- UML-accurate class boxes (name/stereotype, attributes, methods, visibility glyphs), proper arrowheads/diamonds, multiplicities.
- Interactions: hover card; right-click = prune one; right-drag = marquee prune; feedback popover; scroll/drag pan/zoom; keyboard `⌘Z`.
- Decision card: question + option cards each with a live mini-canvas preview (model + ops applied in-memory), "write your own", defence textarea, examiner verdict, overrule button (lenient mode).

### Derived diagrams
- **Domain** (deterministic): filter conceptual classes, drop methods.
- **Package** (deterministic): packages + aggregated dependencies + cycle/layer metrics.
- **Use case** (agent): Spide asks pi via MCP-driven prompt to produce actors/use cases from the class model; regenerated when class model changes (marked stale until refreshed).
- **Sequence** (agent): scenario list → lifelines must be existing classes, messages must be existing methods (validated in Rust; invalid → highlighted, pushes a decision to add the method).
- Pruning in derived tabs sends feedback back to the agent (and for deterministic views, prunes the underlying element).

### Export
- JSON: `design.json` (versioned schema, `serde`).
- PlantUML: one `.puml` per diagram; full members, visibility, stereotypes, multiplicities, packages, notes; `skinparam` theme matching Spide; `-[hidden]->` / direction hints from the canvas layout.
- PNG: frontend serialises the current SVG (with embedded fonts) → Rust `resvg` renders at 2×/3× → file.

## Files (new project)
```
Spide/
  .gitignore  README.md  PLAN.md
  package.json  vite.config.ts  tsconfig.json  index.html
  src/                       # TS frontend
    main.tsx  App.tsx
    canvas/ (Canvas.tsx, ClassNode.tsx, Edge.tsx, physics.ts, layout.ts, visuals.ts)
    ui/ (PromptBar.tsx, AnswerBubble.tsx, HoverCard.tsx, PruneMarquee.tsx, FeedbackPopover.tsx, DecisionCard.tsx, Tabs.tsx, HealthRing.tsx)
    views/ (DomainView.tsx, PackageView.tsx, UseCaseView.tsx, SequenceView.tsx)
    state/ (store.ts, api.ts)   # Tauri invoke/listen wrappers
    export/svg.ts
  src-tauri/
    Cargo.toml  tauri.conf.json
    src/main.rs  src/commands.rs  src/pi_bridge.rs  src/mcp.rs  src/watcher.rs
  crates/spide-core/
    src/{model.rs, ops.rs, store.rs, metrics.rs, derive.rs, plantuml.rs, png.rs}
    tests/
  agent/spide-skill.md          # system prompt / skill for pi (examiner workflow)
```

## Reuse
- pi RPC: `pi --mode rpc` JSONL (`pi-coding-agent/docs/rpc.md`, `rpc-commands.md`, `json.md`); wait for `agent_settled`.
- pi MCP: project `.pi/mcp.json`, streamable HTTP (SSE unsupported) (`docs/mcp.md`).
- Crates: `tauri` v2, `rmcp` (streamable-http server), `serde`, `notify`, `resvg`/`usvg`/`tiny-skia`, `petgraph` (SCC), `tokio`.
- npm: `react`, `vite`, `@tauri-apps/api`, `d3-force`, `elkjs`, `framer-motion`.
- Env: macOS 26 arm64, Xcode CLT present, Node 26, pi 1.5.2. **Rust to install via rustup** (approved).

## Steps
- [x] 0. Install Rust via `rustup` (user-level); `git init` in `Spide/`; scaffold Tauri v2 + React/TS + workspace crate.
- [x] 1. `spide-core`: model, ops, store with op log/undo, JSON (de)serialisation + unit tests.
- [x] 2. Metrics engine (fan-in/out, instability, SCC cycles, LCOM4, god class, layering, orphans, health score) + tests.
- [x] 3. Frontend shell: dark canvas, bottom prompt, Tauri command/event plumbing, snapshot rendering.
- [x] 4. Class rendering: UML boxes, relations with correct notation, elk initial layout, d3-force living physics, metric→visual mappings, ghost/accept/prune states, animations.
- [x] 5. Hover cards; right-click prune; right-drag marquee prune; feedback popover → agent.
- [x] 6. MCP server (rmcp, localhost) with read/propose/evaluate tools; validation of ops.
- [x] 7. Pi bridge: spawn `pi --mode rpc`, write `.pi/mcp.json` + skill/system prompt, stream answers into bubble.
- [x] 8. Decision flow: decision cards, option preview mini-canvases, custom option, defence → examiner verdict, overrule (lenient/strict setting).
- [x] 9. `design.json` file watcher → diff → proposals.
- [x] 10. Derived tabs: Domain + Package (deterministic), Use Case + Sequence (agent, with validation + stale marker); prune/feedback in tabs.
- [x] 11. Export: JSON save/load, PlantUML per diagram (+ golden tests rendered with PlantUML if available), PNG via resvg.
- [ ] 12. README with run instructions; polish pass on visuals.

## Verification
- `cargo test -p spide-core`: ops/undo, metrics on fixture designs (known cycle, god class, layer violation), derivations, PlantUML golden files.
- `npm run tauri dev`: manual run-through:
  1. Blank screen + prompt only.
  2. "Let's start a class diagram for a library system" → pi proposes decision(s); previews render; pick/defend → examiner verdict → classes animate in.
  3. Prompt a change that introduces a cycle → red straining edges, jitter, health score drops; fix → settles.
  4. Hover shows members/metrics; right-click and marquee prune → feedback popover → agent responds.
  5. Tabs: Domain/Package update instantly; Use Case/Sequence generated, sequence messages validated.
  6. Edit `design.json` externally → appears as proposals.
  7. Save/load JSON roundtrip; export PNG (compare to screen) and `.puml` (render with PlantUML if installed, check members/relations match).
- `pi mcp list` from the project folder shows `spide` connected.
