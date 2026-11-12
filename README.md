# Spide

Spide is a desktop app for designing a system live, together with an AI agent. The class diagram is the centre; the other views are derived from it.

You start with an empty dark canvas and a single prompt. The agent first asks for the **brief** (goals, quality attributes, constraints, expected changes), then proposes classes and raises **design decisions**. Before a decision takes effect, you have to choose an option and **defend** it to the agent, which acts as an examiner. Every option shows its consequences before you choose. The diagram keeps showing you how clean the architecture is: hot classes glow, dependency cycles are drawn in red, and a tangled design keeps trembling while a clean one settles. You can check the design against scenarios and expected changes, branch it and rewind it, look at it as C4 context and container diagrams, and compare it with real Java code.

## Run

Requirements: Rust (`rustup`), Node 20+, and [pi](https://pi.dev) installed and logged in. Optional: PlantUML, used only by the export tests.

```sh
npm install
npm run app            # = tauri dev (loads ~/.cargo/env first)
npm run app:build      # release .app bundle in target/release/bundle/macos
```

Spide opens the project folder `~/Documents/Spide/untitled`. Set `SPIDE_PROJECT=/path/to/folder` to open another one, or use `/new` and `/open` inside the app. Each project folder contains:

| file | purpose |
|---|---|
| `design.json` | The design (the active branch). It is autosaved on every change and watched for edits made outside Spide. |
| `.pi/mcp.json` | Points pi to Spide's MCP server (`spide`, `http://127.0.0.1:47800/mcp`). |
| `.spide/agent.md` | The system prompt Spide appends for its agent (copied from `agent/spide-skill.md`). |
| `.spide/sessions/` | The agent's pi sessions. Spide continues the latest one on start. |
| `.spide/history.jsonl` | One snapshot per change of the model, for the timeline. |
| `.spide/branches/` | Inactive branches (`<name>.json`); `.spide/branch` names the active one. |
| `.spide/reality.json` | The last imported code (read-only), for the drift view. |

Older `design.json` files (schema 1) open unchanged.

## Using it

### The loop

1. **Brief.** On an empty design the agent asks one question at a time and records your answers as forces (goals, quality attributes, constraints, expected changes) with a priority. ⌘B or `/brief` opens the brief panel, where you can add, edit, reprioritise or remove forces yourself. `/skip` starts designing without a brief.
2. **Decide.** The decision panel on the right shows the agent's question and 2–3 options. Each option has a mini preview, the health it would give, which forces it helps (+) or hurts (−), and a line of **consequences** (added, changed and removed elements, new and resolved issues, flipped dependencies, scenarios to re-check; click it for details). `compare` (or `/compare`) shows the options side by side on one shared layout. Hover over an option to preview it on the canvas, pick one or describe your own, and **defend** it. The examiner judges the defence against the brief. In *lenient* mode you can **overrule** a rejection (the elements are marked `⚠ overruled`); in *strict* mode you have to keep defending.
3. **Check behaviour.** In the Sequence tab each scenario gets a fit score: calls, call depth, package, layer and container crossings, aggregates touched, unknown calls. In the brief panel, **change scenarios** ("what if we must support e-books?") are analysed by the agent: Spide shows which classes must change, which depend on them (the ripple) and a locality score. **stress test** (or `/stress <name>`) highlights that footprint on the canvas.
4. **Evolve.** ⌘T or `/timeline` scrubs through the design's history (read-only preview); *restore here* proposes the changes that bring a past state back, *fork* starts a branch from it. `/branch <name>` branches from now and switches to it, `/switch`, `/compare <branch>` and `/merge <branch>` (the other branch's changes arrive as ghost proposals, additions and removals separately).
5. **Close the loop.** `/export adr` writes one Markdown ADR per decided decision (context, drivers, options, defence, examiner verdict, consequences). `/export archunit` writes an ArchUnit test that enforces the rules in Java (set `/coderoot org.your.app` first). `/import` reads a Java source folder; `/drift` then shows what is designed but not in the code, what is only in the code (faded "only in code" classes on the canvas), members, relations and packages that differ, and which rules the code breaks. *adopt code into design* turns the code-only parts into ghost proposals; *copy code to-do list* copies a Markdown checklist.

### Partner modes and rules

- `/mode examiner | socratic | devils_advocate | sketch` (also in the brief panel). *Examiner* is the default. *Socratic* only asks questions and never changes the design. *Devil's advocate* argues against what you lean towards before accepting. *Sketch* proposes freely and keeps decisions for big forks. `/strict` and `/lenient` set how hard the examiner is.
- **Architecture rules** live in the brief panel (and `/rules <preset>`): a style preset — `clean` (dependencies point inward), `hexagonal` (adapters never depend on each other), `layered` (downward only), `modular_monolith` (`<module>.<layer>` packages, other modules only via `<module>.api`), `none` — plus your own forbidden dependencies (`*Controller -> *Repository : go via a service`), forbidden types per layer (`Http*` in domain) and thresholds. Editing `rules` in `design.json` applies directly.
- Every issue explains why it matters and how to fix it. In the health popover, click an issue to jump to it, or **fix with agent** to get a decision with ways to resolve it.

### On the canvas

- **Prompt.** Type `/` to see all commands.
- **Ghost proposals.** Changes from the agent show up as translucent, breathing ghosts. Click a ghost to accept it, right-click to reject it.
- **Direct edits.** Double-click a class, attribute or method name to rename it. Drag a class onto another package to move it. `/add <Class> [in <package>]` adds a class. ⌥-click pins a class where it is (📌); pinned classes stay put, also on `/tidy`. Direct edits are yours: they apply immediately and ⌘Z undoes them.
- **Prune.** Right-click an element to remove it, or hold the right mouse button and drag a box around several. A small box asks what you didn't like; ⏎ sends it to the agent, Esc prunes without comment. Pruned elements stay in the model, hidden by default (`/pruned` shows them), and the agent is told not to bring them back. Click a pruned element to restore it (restoring a package restores its classes).
- **Hover** over an element for its details: members, responsibilities, rationale, metrics, and **why it's here** — the decision (question, chosen option, your defence) or proposal that introduced it. Click an element that came from a decision to see the whole decision record.
- **Navigate.** Drag to pan, pinch or ⌘-scroll to zoom, double-click empty space to fit. `/find <class>` zooms to a class and focuses it; `/focus <package>` zooms to a package and dims the rest; Esc clears. ⌘1–6 switch tabs, ⌘Z / ⇧⌘Z undo and redo, ⌘J opens the conversation, ⌘E exports a PNG and ⇧⌘E PlantUML.

### What the visuals mean

| visual | meaning |
|---|---|
| node size | coupling (fan-in + fan-out) |
| halo colour blue → amber → red | heat: coupling, cycles, god class, low cohesion, broken rules |
| red, animated, straining edge | the relation is part of a dependency cycle |
| amber edge `layer ↑` / `module` / `forbidden` | the relation breaks a layering, module-boundary or custom rule |
| `«async»` / `«event»` on an edge | the dependency is not a synchronous call |
| faint dotted edge | a hidden dependency through attribute or parameter types, or method calls, that has no drawn relation |
| chips | `cycle`, `god class`, `LCOM n`, `rule ✗`, `orphan`, `◆ aggregate`, `only in code` |
| red / amber ring | stress test: impacted / ripple · drift: missing in code (dashed) / differs from code |
| trembling canvas | overall tension, i.e. 100 − health. A clean design comes to rest. |
| health ring (next to the prompt) | 0–100. Hover over it for the issues. `→ n` shows the health after accepting pending ghosts. |

Package names such as `domain`, `ui` or `persistence` are mapped to their layer automatically.

### Views

- **System** (C4). *Context*: actors, the system and external systems. *Containers*: deployable units with their technology and replicas, the packages that run in them, datastores, links to external systems. Container-to-container dependencies are derived from the class dependencies. Spide flags single points of failure, synchronous call chains across containers, and views that disagree (a package that runs in no container, an unconnected external system, an actor without use cases).
- **Class.** The main canvas.
- **Domain.** Computed: conceptual classes only (technical stereotypes such as service, controller and repository are left out), attributes only, and associations.
- **Package.** Computed: packages, their aggregated dependencies, and the I / A / D metrics.
- **Use case.** Generated by the agent from the class model. The tab is marked stale when the class model changes.
- **Sequence.** Generated by the agent for a scenario you pick from the use cases or name in the prompt. Every message is checked against the class model; calls to methods that don't exist are red. The side panel shows the scenario's fit.

## Exports

- **PNG** renders the same SVG the canvas shows, using `resvg` at 2×. Animations are frozen in the image.
- **PlantUML** is exported per tab, or for every non-empty diagram at once with `/export all`. The System tab exports C4-PlantUML (context and container diagrams). The export keeps every member with its visibility and type, plus stereotypes, multiplicities, layer-coloured packages, cycles and layer violations in colour, a health legend, and `down`/`right` direction hints taken from the canvas. PlantUML uses its own layout engine, so the PNG is the pixel-exact export.
- **JSON** via `/save` and `/open`. The project's `design.json` is always up to date.
- **ADRs** via `/export adr` (Markdown, MADR style, one per accepted or overruled decision).
- **ArchUnit** via `/export archunit`: a JUnit 5 `ArchitectureTest.java` with the layering of the preset, custom forbidden dependencies and types, module boundaries and a no-package-cycles rule. It needs `com.tngtech.archunit:archunit-junit5` as a test dependency.

## Architecture

```
crates/spide-core     pure Rust: model, ops, store (proposals, decisions, undo), metrics, rules,
                      impact, scenarios, drift, derived views (incl. C4), diff, agent tool API,
                      exporters (PlantUML, C4, PNG, ADR, ArchUnit)
crates/spide-import   Java source import (tree-sitter) into a read-only design for drift checks
src-tauri             Tauri v2 shell: commands, MCP server (axum), pi bridge (pi --mode rpc),
                      design.json watcher, history, branches, prompts for the agent
src                   React + TypeScript UI: SVG canvas with d3-force physics and ELK layout
agent/spide-skill.md  the agent's instructions (examiner workflow, modelling conventions)
```

- The Rust store is the single source of truth. The UI only sends intents (prompt, prune, accept, defend, overrule, undo) and renders the snapshots it receives.
- Agents change the design **only** through MCP tools: `get_design`, `get_metrics`, `get_view`, `propose_decision`, `propose_ops`, `evaluate_defense`, `set_use_cases`, `set_sequence`, `set_change_impact`. Every change becomes a proposal or a decision, and invalid ops are rejected atomically with a clear error. Forces, change scenarios, containers, external systems and links are ops too, so they go through the same proposals, undo and file watcher. Architecture rules are the user's: only the user changes them.
- Every committed element records its origin (decision, proposal, `user` or `import`); accepted decisions keep the consequences they had.
- Other agents can join the session. A pi you start in a terminal in the project folder picks up `.pi/mcp.json`, and any agent can also edit `design.json` directly. Spide turns such edits into ghost proposals instead of applying them silently.
- The MCP server is a small streamable-HTTP JSON-RPC implementation (initialize, tools/list, tools/call) on `127.0.0.1` only. Every route (including the dev routes) requires a loopback `Host` header and, when a browser sends one, a local `Origin`, which blocks cross-site requests and DNS rebinding. It replaces the `rmcp` crate that was originally planned, to avoid tracking that crate's fast-changing API.
- Environment variables: `SPIDE_PROJECT` (project folder), `SPIDE_PI` (path to the pi binary), `SPIDE_PI_ARGS` (extra pi arguments, e.g. `--model …`).

## Development

Verification results, known limitations and remaining manual checks are tracked in [`plans/interactive-design-verification.md`](plans/interactive-design-verification.md). In particular, concurrent views may overwrite pinned layouts; Java import and aggregate-write warnings are heuristic, and branch merging is not a three-way merge.

```sh
cargo test --workspace                         # core, import, app (golden files; PlantUML syntax check if installed)
UPDATE_GOLDEN=1 cargo test -p spide-core       # accept changed golden output (PlantUML, ADR, ArchUnit)
SPIDE_SMOKE_DIR=/abs/path/to/java/project cargo test -p spide-import smoke -- --nocapture   # import smoke test
npm run typecheck
```

Debug builds also serve `GET /dev/snapshot` and `POST /dev/invoke/<command>` on the MCP port, so `http://localhost:1420` can drive the running app from a normal browser (used for automated UI checks). In dev, `?tab=<tab>&cmd=/command` in that URL opens a tab and runs prompt commands once the first snapshot arrives. Release builds don't expose `/dev/invoke`.

`examples/library.json` is a small design with deliberate problems (a cycle, layer violations, an orphan). Copy it to a project's `design.json` while Spide is running to see it arrive as ghost proposals.
