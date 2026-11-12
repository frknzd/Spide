# Spide

Spide is a desktop app for designing a system's class diagram live, together with an AI agent.

You start with an empty dark canvas and a single prompt. You describe what you want to build, and the agent proposes classes and raises **design decisions**. Before a decision takes effect, you have to choose an option and **defend** it to the agent, which acts as an examiner. The diagram keeps showing you how clean the architecture is: hot classes glow, dependency cycles are drawn in red, and a tangled design keeps trembling while a clean one settles. Domain, package, use case and sequence diagrams are derived from the class model.

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
| `design.json` | The design. It is autosaved on every change and watched for edits made outside Spide. |
| `.pi/mcp.json` | Points pi to Spide's MCP server (`spide`, `http://127.0.0.1:47800/mcp`). |
| `.spide/agent.md` | The system prompt Spide appends for its agent (copied from `agent/spide-skill.md`). |
| `.spide/sessions/` | The agent's pi sessions. Spide continues the latest one on start. |

## Using it

- **Prompt.** The prompt at the bottom is the only way to add things, for example "let's start a class diagram for a library system". Type `/` to see the commands: `/undo`, `/accept`, `/tidy`, `/find <class>`, `/pruned`, `/strict`, `/lenient`, `/export png`, `/export puml`, `/export all`, `/save`, `/open`, `/new`, `/history`, `/stop`, `/agent restart`.
- **Ghost proposals.** Small changes from the agent show up as translucent, breathing "ghosts". Click a ghost to accept it, or right-click it to reject it.
- **Decisions.**
  1. The panel on the right shows the agent's question and 2–3 options. Each option has a mini preview and the health score you would get by choosing it.
  2. Hover over an option to preview it on the main canvas.
  3. Pick an option, or describe your own alternative.
  4. Defend your choice. The examiner either accepts it, which applies the change, or rejects it and tells you what is still missing.
  5. In *lenient* mode you can **overrule** a rejection. The affected elements are then marked `⚠ overruled`. In *strict* mode you have to keep defending.
- **Prune.** Right-click a class, member, relation or package to remove it. To remove several elements at once, hold the right mouse button and drag a box around them. A small box then asks what you didn't like. Press ⏎ to send your answer to the agent, or Esc to prune without comment. Pruned elements stay in the model, hidden by default (`/pruned` shows them), and the agent is told not to bring them back. Click any pruned element to restore it. Restoring a package also restores its classes.
- **Hover** over any element to see its details: members with full signatures, responsibilities, rationale, and metrics (fan-in/out, instability, LCOM4).
- **Canvas.** Drag to pan, pinch or ⌘-scroll to zoom, double-click to fit. `/find <class>` zooms to a class and keeps it in focus mode (neighbours lit, the rest dimmed) until Esc. ⌘1–5 switches tabs, ⌘Z / ⇧⌘Z undo and redo, ⌘J opens the conversation, ⌘E exports a PNG and ⇧⌘E exports PlantUML.

### What the visuals mean

| visual | meaning |
|---|---|
| node size | coupling (fan-in + fan-out) |
| halo colour blue → amber → red | heat: coupling, cycles, god class, low cohesion, layer violations |
| red, animated, straining edge | the relation is part of a dependency cycle |
| amber edge `layer ↑` | an inner layer depends on an outer one |
| faint dotted edge | a hidden dependency through attribute or parameter types, or method calls, that has no drawn relation |
| chips under a class | `cycle`, `god class`, `LCOM n`, `layer ↑`, `orphan` |
| trembling canvas | overall tension, i.e. 100 − health. A clean design comes to rest. |
| health ring (next to the prompt) | 0–100. Hover over it for the list of issues. `→ n` shows the health after accepting pending ghosts. |

Layers follow clean architecture. `ui` and `infrastructure` form the outer ring, `application` is in the middle and `domain` is the core, and dependencies must point inward. Package names such as `domain`, `ui` or `persistence` are mapped to their layer automatically.

### Derived diagrams

- **Domain.** Computed from the class model: conceptual classes only (technical stereotypes such as service, controller and repository are left out), attributes only, and associations.
- **Package.** Computed from the class model: packages, their aggregated dependencies, and the I / A / D metrics.
- **Use case.** Generated by the agent from the class model. The tab is marked stale when the class model changes.
- **Sequence.** Generated by the agent for a scenario you pick from the use cases or name in the prompt. Every message is checked against the class model, and calls to methods that don't exist are shown in red.

## Exports

- **PNG** renders the same SVG the canvas shows, using `resvg` at 2×. Animations are frozen in the image.
- **PlantUML** is exported per tab, or for every non-empty diagram at once with `/export all`. The export keeps every member with its visibility and type, plus stereotypes, multiplicities, layer-coloured packages, cycles and layer violations in colour, a health legend, and `down`/`right` direction hints taken from the canvas. PlantUML uses its own layout engine, so the PNG is the pixel-exact export.
- **JSON** via `/save` and `/open`. The project's `design.json` is always up to date.

## Architecture

```
crates/spide-core     pure Rust: model, ops, store (proposals, decisions, undo), metrics,
                      derived views, diff, agent tool API, PlantUML + PNG exporters
src-tauri             Tauri v2 shell: commands, MCP server (axum), pi bridge (pi --mode rpc),
                      design.json watcher, prompts for the agent
src                   React + TypeScript UI: SVG canvas with d3-force physics and ELK layout
agent/spide-skill.md  the agent's instructions (examiner workflow, modelling conventions)
```

- The Rust store is the single source of truth. The UI only sends intents (prompt, prune, accept, defend, overrule, undo) and renders the snapshots it receives.
- Agents change the design **only** through MCP tools: `get_design`, `get_metrics`, `get_view`, `propose_decision`, `propose_ops`, `evaluate_defense`, `set_use_cases`, `set_sequence`. Every change becomes a proposal or a decision, and invalid ops are rejected atomically with a clear error.
- Other agents can join the session. A pi you start in a terminal in the project folder picks up `.pi/mcp.json`, and any agent can also edit `design.json` directly. Spide turns such edits into ghost proposals instead of applying them silently.
- The MCP server is a small streamable-HTTP JSON-RPC implementation (initialize, tools/list, tools/call) on `127.0.0.1` only. Every route (including the dev routes) requires a loopback `Host` header and, when a browser sends one, a local `Origin`, which blocks cross-site requests and DNS rebinding. It replaces the `rmcp` crate that was originally planned, to avoid tracking that crate's fast-changing API.
- Environment variables: `SPIDE_PROJECT` (project folder), `SPIDE_PI` (path to the pi binary), `SPIDE_PI_ARGS` (extra pi arguments, e.g. `--model …`).

## Development

```sh
cargo test -p spide-core                       # model, metrics, export (golden files, PlantUML syntax check if installed)
UPDATE_GOLDEN=1 cargo test -p spide-core       # accept changed PlantUML output
npm run typecheck
```

Debug builds also serve `GET /dev/snapshot` and `POST /dev/invoke/<command>` on the MCP port, so `http://localhost:1420` can drive the running app from a normal browser (used for automated UI checks). Release builds don't expose `/dev/invoke`.

`examples/library.json` is a small design with deliberate problems (a cycle, layer violations, an orphan). Copy it to a project's `design.json` while Spide is running to see it arrive as ghost proposals.
