# Spide

Spide is a desktop app for designing a system live, together with an AI agent. The class diagram is the centre; the other views are derived from it.

You start with an empty dark canvas beside a real interactive pi terminal. The agent first asks for the **brief** (goals, quality attributes, constraints, expected changes), then proposes classes and raises **design decisions**. Before a decision takes effect, you have to choose an option and **defend** it to the agent, which acts as an examiner. Every option shows its consequences before you choose. The diagram keeps showing you how clean the design is — judged by the principles of the DOSS course (minimise coupling, increase cohesion, distribute responsibilities; RDD, GRASP, bad smells): hot classes glow, dependency cycles are drawn in red, methods in the wrong class point to their better home, and duplicated behaviour is measured. You can check the design against scenarios and expected changes, branch it and rewind it, look at it as C4 context and container diagrams, and compare it with real Java code.

## Run

Requirements: Rust (`rustup`) and [pi](https://pi.dev) installed and logged in. Node 24+ is needed for pi and its native TypeScript extension test, not for a frontend build. Spide uses native Rust/egui widgets, with no browser or webview. The PTY/control transport targets macOS/Unix; Windows is not supported. Optional: PlantUML for exporter syntax tests. Linux builds need the usual native graphics/windowing development libraries (X11/Wayland and OpenGL/Vulkan).

```sh
cargo run -p spide
./scripts/bundle-macos.sh   # release app: target/release/bundle/macos/Spide.app
```

The bundle is unsigned; signing/notarization for distribution is separate. Launch from a shell with `pi` on `PATH`, or set `SPIDE_PI` to its absolute path. Example project:

```sh
project=$(mktemp -d)
cp examples/library.json "$project/design.json"
SPIDE_PROJECT="$project" cargo run -p spide
```

Spide opens the project folder `~/Documents/Spide/untitled`. Set `SPIDE_PROJECT=/path/to/folder` to open another one, or use `/spide new` and `/spide open` inside pi (or the terminal header's **Commands** button). Each project folder contains:

| file | purpose |
|---|---|
| `design.json` | The design (the active branch). It is autosaved on every change and watched for edits made outside Spide. |
| `.pi/mcp.json` | Points pi to Spide's MCP server (`spide`, `http://127.0.0.1:47800/mcp`). |
| `.spide/terminal-bridge.ts` | App-owned pi extension for private structured prompts, context and `/spide` commands. The skill in `agent/spide-skill.md` is appended at launch. |
| `.spide/sessions/` | The agent's pi sessions. Spide continues the latest one on start. |
| `.spide/history.jsonl` | One snapshot per change of the model, for the timeline. |
| `.spide/branches/` | Inactive branches (`<name>.json`); `.spide/branch` names the active one. |
| `.spide/reality.json` | The last imported code (read-only), for the drift view. |

Older `design.json` files (schema 1) open unchanged.

## Using it

### Native pi and workspace

- Pi runs once in a real PTY in the project folder, with **all available tools** (`--tools '*'`), configured extensions/MCP servers and the user's permission controls. Code edits and shell commands are allowed; diagram mutations still use Spide MCP proposals/decisions. Trust/permission dialogs remain interactive. On first opening a project you may need to approve pi's **Trust project folder?** prompt before Spide's integration becomes ready.
- The native editor is the only chat input. `Ctrl+P` cycles configured models, `Ctrl+Shift+P` cycles backward, `Ctrl+L` opens the model picker; Shift+Tab, Escape, multiline input and transcript scrolling belong to pi. Canvas shortcuts do not run while the terminal is focused. Selection/copy/paste uses the native terminal widget.
- Drag separators to resize panes; collapse/reopen without restarting pi. Widths/visibility are saved locally, not in the design. Narrow windows put decisions in a drawer that doesn't cover pi. **Restart** is in the terminal header, even when pi is offline.
- **All Spide `/command` examples below mean `/spide command` in the terminal**, or the same command in **Commands**. Native pi `/new`, `/model`, `/settings`, `/resume`, `/export`, etc. retain their own meanings. For example, use `/spide detail 3`, `/spide tidy`, `/spide import`, `/spide open`, `/spide save`.
- Examiner/fix/generation requests go through an authenticated private Unix socket into that same session, never by pasting into a partly typed draft. Busy requests use pi follow-ups. Terminal bytes never drive app/design state.

### The loop

1. **Brief.** On an empty design the agent asks one question at a time and records your answers as forces (goals, quality attributes, constraints, expected changes) with a priority. ⌘B or `/brief` opens the brief panel, where you can add, edit, reprioritise or remove forces yourself. `/skip` starts designing without a brief.
2. **Decide.** The decision panel on the right shows the agent's question and 2–3 options. Each compact option has a selectable radio control, summary and health delta. **Details** lazily opens its mini diagram, brief trade-offs and **consequences** (added, changed and removed elements, new and resolved issues, flipped dependencies, scenarios to re-check; click it for details). `compare` (or `/compare`) shows the options side by side on one shared layout. **Preview / Stop preview** explicitly controls the read-only canvas preview; hovering, selecting or expanding never replaces it. Pick an option or describe your own, then **defend** it in the persistent footer. Drafts survive navigation and snapshots; previews close on decision/revision changes. The examiner judges the defence against the brief. In *lenient* mode you can **overrule** a rejection (the elements are marked `⚠ overruled`); in *strict* mode you have to keep defending.
3. **Check behaviour.** In the Sequence tab each scenario gets a fit score: calls, call depth, package, layer and container crossings, aggregates touched, unknown calls. In the brief panel, **change scenarios** ("what if we must support e-books?") are analysed by the agent: Spide shows which classes must change, which depend on them (the ripple) and a locality score. **stress test** (or `/stress <name>`) highlights that footprint on the canvas.
4. **Evolve.** ⌘T or `/timeline` scrubs through the design's history (read-only preview); *restore here* proposes the changes that bring a past state back, *fork* starts a branch from it. `/branch <name>` branches from now and switches to it, `/switch`, `/compare <branch>` and `/merge <branch>` (the other branch's changes arrive as ghost proposals, additions and removals separately).
5. **Close the loop.** `/export adr` writes one Markdown ADR per decided decision (context, drivers, options, defence, examiner verdict, consequences). `/export archunit` writes an ArchUnit test that enforces the rules in Java (set `/coderoot org.your.app` first). `/import` reads a Java source folder; `/drift` then shows what is designed but not in the code, what is only in the code (faded "only in code" classes on the canvas), members, relations and packages that differ, and which rules the code breaks. *adopt code into design* turns the code-only parts into ghost proposals; *copy code to-do list* copies a Markdown checklist.

### Partner modes and rules

- `/mode examiner | socratic | devils_advocate | sketch` (also in the brief panel). *Examiner* is the default. *Socratic* only asks questions and never changes the design. *Devil's advocate* argues against what you lean towards before accepting. *Sketch* proposes freely and keeps decisions for big forks. `/strict` and `/lenient` set how hard the examiner is.
- **Architecture rules** live in the brief panel (and `/rules <preset>`): a style preset — `clean` (dependencies point inward), `hexagonal` (adapters never depend on each other), `layered` (downward only), `modular_monolith` (`<module>.<layer>` packages, other modules only via `<module>.api`), `none` — plus your own forbidden dependencies (`*Controller -> *Repository : go via a service`), forbidden types per layer (`Http*` in domain) and thresholds. Editing `rules` in `design.json` applies directly.
- Every issue explains why it matters and how to fix it, names the course principle and reference it comes from (e.g. *GRASP Controller (Ch. 3)*), and says how sure Spide is (confidence, and whether it comes from structure, a heuristic, the agent or the code). In the health popover, click an issue to jump to it, or **fix with agent** to get a decision with ways to resolve it — for misplaced or duplicated behaviour the options are the ranked homes from `suggest_home` (the Information Expert, a new Pure Fabrication, or keeping it).

### Diagnostics: what the course grades

Health is **one number** (0–100): a weighted geometric mean of five principle scores — **coupling** (.25), **cohesion** (.2), **responsibilities** (.3), **architecture rules** (.15) and **UML hygiene** (.1) — each computed from its issues (severity × confidence) relative to the size of the design, so one bad class in five hurts more than one in fifty. Hover the health ring for the breakdown. The checks:

- **Purpose vs behaviour.** Every class gets a role (presentation, controller, service, domain, persistence, adapter, output, creator, utility, value) from, in order: an explicit `role`, the agent's assessment, its name (`*Panel`, `*Controller`, `*Repository`, `*Writer`, …), its stereotype, its layer. Every method gets a concern (presentation, coordination, domain logic, persistence, formatting, creation, accessor) and an RDD kind (knowing, doing, deciding) from its verb, signature, `uses` and `calls` — or from its body when code is imported. Mismatches are flagged: a UI class doing domain calculations, an entity saving or formatting itself (the `Sale.save()` example), a fat controller, vague names (`*Manager`, `*Util`), name/stereotype/layer disagreeing, a class mixing several concerns.
- **Duplication and the right home.** Method pairs get an explainable similarity (name, signature, what the bodies touch, description, and with code a normalised fingerprint). Pass-through delegation (a controller forwarding to the domain), polymorphic overrides and UI event handlers are not duplication. Spide also flags behaviour that another class claims (its data, or its stated responsibility), concepts scattered over ≥ 3 classes (shotgun surgery) and middle men. For each, `suggest_home` ranks where the behaviour belongs (Information Expert; wholes take over their parts), checks every caller may depend on it, simulates the health/coupling/cohesion effect, and offers a new Pure Fabrication class when no expert fits.
- **Course checks.** Law of Demeter (talking to strangers), Feature Envy, data classes / no behaviour, the UI driving the domain without a controller, Creator, switches on a type code in several places (replace with Polymorphism), long methods, long parameter lists, and the exam notes on UML: argument types, no repeated inherited attributes, a Composite needs add/remove/getChildren, a Visitor with one element is a Strategy.
- **The agent's judgement.** The agent records its own assessment per class (`assess_class`: purpose, role, a verdict per method). Agreement raises an issue's confidence; when it disagrees on a serious issue you see the disagreement and have to resolve it. Assessments go stale when the class changes.
- **The real code.** After `/import`, the same checks run on the implementation (with body facts: lines, call chains, switches, loops, creations, UI calls, clone fingerprints). `/drift` shows *design n · code m* health, real clones, issues only the code has, and where the code contradicts the design (e.g. *design: the panel only presents; code: it loops over the report's calls and counts*). `/code Class[.method]` shows the implementation.

### On the canvas

- **Commands.** Type `/spide ` in pi for completion, or open **Commands** in its header.
- **Ghost proposals.** Click a ghost or **Review proposals (N)** to open the dedicated Proposals inspector alongside the same canvas. Review one plan at a time with before/after values, impact and optional associated decision context. Add, edit or remove planned operations; structured forms cover classes, members, relations and packages, and advanced JSON covers other operations. **Save plan** updates only the pending plan; **Accept proposal** commits it, **Reject proposal** drops only that plan. There are no bulk actions. `/accept <proposal-id>` and `/reject <proposal-id>` require an explicit ID. Canvas editing is disabled during review; focus controls still work.
- **Safe plan drafts.** Draft previews never commit accepted elements. Unsaved plan drafts are kept locally per workspace/branch/proposal; operation forms must be added to the draft first. Workspace changes require explicit revalidation before saving. Dependent plans must have prerequisites accepted first; invalid plans show errors rather than disappearing. Undo/redo restores plan saves and individual resolutions. Decision associations are context, not approval of your amendments.
- **Direct edits.** Double-click a class, attribute or method name to rename it. Drag a class onto another package to move it. `/add <Class> [in <package>]` adds a class. ⌥-click pins a class where it is (📌); pinned classes stay put, also on `/tidy`. Direct edits are yours: they apply immediately and ⌘Z undoes them.
- **Prune.** Right-click an element to remove it, or hold the right mouse button and drag a box around several. A small box asks what you didn't like; ⏎ sends it to the agent, Esc prunes without comment. Pruned elements stay in the model, hidden by default (`/pruned` shows them), and the agent is told not to bring them back. Click a pruned element to restore it (restoring a package restores its classes).
- **Detail.** The slider bottom-left (or `/detail 0–4`) sets how much each class shows: packages · names · responsibilities · **design** (default: public behaviour with argument types; accessors, constructors and private helpers folded into one line; attributes that repeat a drawn association become the edge's role name; inherited attributes not repeated) · full. Zooming far out shows less on its own (names, then packages). Exports follow the slider. Methods the diagnostics flag always stay visible, with `↗ Home` next to them.
- **External.** `/external <package|Class>` tags library/framework code (Spide suggests it once for packages like `zap` or `swing`): drawn as a compact strip, coupling to it counts less, and it isn't judged.
- **Lenses.** `/lens concerns` colours method rows by what they do; `/lens overlap` draws dashed links between classes that duplicate each other and marks scattered concepts (⁂); `/lens off`. `/implicit` shows all implicit dependencies (otherwise only the hovered class's).
- **Hover** over an element for its details: its **purpose** (the agent's sentence, the role Spide reads and why, methods in the wrong place → suggested home, what the code says), members, responsibilities, rationale, metrics, and **why it's here** — the decision (question, chosen option, your defence) or proposal that introduced it. Click an element that came from a decision to see the whole decision record.
- **Navigate.** Drag to pan, pinch or ⌘-scroll to zoom, double-click empty space to fit. `/find <class>` zooms to a class and focuses it; `/focus <package>` zooms to a package and dims the rest; Esc clears. ⌘1–6 switch tabs, ⌘Z / ⇧⌘Z undo and redo, ⌘E opens Export and ⇧⌘E repeats the last export.

### What the visuals mean

| visual | meaning |
|---|---|
| node size | coupling (fan-in + fan-out) |
| halo colour blue → amber → red | heat: coupling, cycles, god class, low cohesion, broken rules |
| red, straining edge | the relation is part of a dependency cycle (animates briefly, then rests) |
| amber edge `layer ↑` / `module` / `forbidden` | the relation breaks a layering, module-boundary or custom rule |
| `«async»` / `«event»` on an edge | the dependency is not a synchronous call |
| faint dotted edge | a hidden dependency through attribute or parameter types, or method calls, that has no drawn relation (shown for the hovered class, or all with `/implicit`) |
| chips | `cycle`, `god class`, `LCOM n`, `rule ✗`, `orphan`, `◆ aggregate`, `«external»`, `only in code`, `⁂ concept` (overlap lens) |
| `↗ Home` / `⚠ kind` after a method | the method is in the wrong place (suggested home) or breaks a course rule |
| dashed purple edge (overlap lens) | duplicated behaviour (share) or a method dealing with another class's concept |
| red / amber ring | stress test: impacted / ripple · drift: missing in code (dashed) / differs from code |
| health ring (canvas toolbar) | 0–100. Hover over it for the five principle scores and the issues per principle. `→ n` shows the health after accepting pending ghosts. |

The layout uses Rust Sugiyama layering with compound package packing (ui and infrastructure on top, application, then domain; external packages beside their users). A bounded force simulation separates nodes and package hulls after changes. It only moves after a change, a drag or `/tidy`, then stops: an idle canvas costs no CPU. Decorative animations (ghosts, hot halos, cycles) play briefly and rest.

Package names such as `domain`, `ui` or `persistence` are mapped to their layer automatically.

### Views

- **System** (C4). *Context*: actors, the system and external systems. *Containers*: deployable units with their technology and replicas, the packages that run in them, datastores, links to external systems. Container-to-container dependencies are derived from the class dependencies. Spide flags single points of failure, synchronous call chains across containers, and views that disagree (a package that runs in no container, an unconnected external system, an actor without use cases).
- **Class.** The main canvas.
- **Domain.** Computed: conceptual classes only (technical stereotypes such as service, controller and repository are left out), attributes only, and associations.
- **Package.** Computed: packages, their aggregated dependencies, and the I / A / D metrics.
- **Use case.** Generated by the agent from the class model. The tab is marked stale when the class model changes.
- **Sequence.** Generated by the agent for a scenario you pick from the use cases or name in the prompt. Every message is checked against the class model; calls to methods that don't exist are red. The side panel shows the scenario's fit.

## Exports

- **PNG, SVG and PDF** use the same retained scene as the native canvas. Export settings include theme, scale, legend, diagnostics and ghost visibility, and persist per project. PNG uses `resvg` (default 2×); PDF uses `svg2pdf`. Animations are frozen. **File → Export Again** repeats the previous format/path.
- **PlantUML** is exported per tab, or for every non-empty diagram at once with `/export all`. The System tab exports C4-PlantUML (context and container diagrams). The export keeps every member with its visibility and type, plus stereotypes, multiplicities, layer-coloured packages, cycles and layer violations in colour, a health legend, and `down`/`right` direction hints taken from the canvas. PlantUML uses its own layout engine, so the PNG is the pixel-exact export.
- **JSON** via `/save` and `/open`. The project's `design.json` is always up to date.
- **ADRs** via `/export adr` (Markdown, MADR style, one per accepted or overruled decision).
- **ArchUnit** via `/export archunit`: a JUnit 5 `ArchitectureTest.java` with the layering of the preset, custom forbidden dependencies and types, module boundaries and a no-package-cycles rule. It needs `com.tngtech.archunit:archunit-junit5` as a test dependency.

## Architecture

```
crates/spide-core     pure Rust: model, ops, store (proposals, decisions, undo), metrics (health per principle),
                      responsibility (roles, concerns), overlap (duplication, delegation), home (suggested homes),
                      course (Demeter, Creator, …), assess (agent judgement), code (implementation findings), rules,
                      impact, scenarios, drift, derived views (incl. C4), diff, agent tool API,
                      exporters (PlantUML, C4, PNG, ADR, ArchUnit)
crates/spide-import   Java source import (tree-sitter) into a read-only design for drift checks, with body facts
crates/spide-diagram  GUI-independent scenes, orthogonal routing, compound Sugiyama layout, physics,
                      diagnostics/focus, all diagram views, themes and shared-scene SVG export
app                   eframe/egui desktop app: canvas, native panels, alacritty_terminal widget,
                      commands, MCP server (axum), real pi PTY + private control socket,
                      design.json watcher, history, branches, structured prompts for the agent
agent/               appended skill (examiner workflow) and bundled native pi integration extension
```

- The Rust store is the single source of truth. The UI only sends intents (prompt, prune, accept, defend, overrule, undo) and renders the snapshots it receives.
- Agents change the design **only** through MCP tools: `get_design`, `get_metrics`, `get_view`, `propose_decision`, `propose_ops`, `evaluate_defense`, `set_use_cases`, `set_sequence`, `set_change_impact`. Analysis tools: `get_responsibilities`, `get_overlap`, `suggest_home`, `get_code` (read the imported implementation) and `assess_class` (record the agent's judgement); the first two take `on: "code"`. Every change becomes a proposal or a decision, and invalid ops are rejected atomically with a clear error. Forces, change scenarios, containers, external systems and links are ops too, so they go through the same proposals, undo and file watcher. Architecture rules are the user's: only the user changes them.
- Every committed element records its origin (decision, proposal, `user` or `import`); accepted decisions keep the consequences they had.
- Other agents can join the session. A pi you start in a terminal in the project folder picks up `.pi/mcp.json`, and any agent can also edit `design.json` directly. Spide turns such edits into ghost proposals instead of applying them silently.
- The MCP server is a small streamable-HTTP JSON-RPC implementation (initialize, tools/list, tools/call) on `127.0.0.1` only. Every route requires a loopback `Host` header and, when a browser sends one, a local `Origin`, which blocks cross-site requests and DNS rebinding. It replaces the `rmcp` crate that was originally planned, to avoid tracking that crate's fast-changing API.
- Environment variables: `SPIDE_PROJECT` (project folder), `SPIDE_PI` (path to the pi binary), native pi configuration and `/model` select the model. Legacy `SPIDE_PI_ARGS` is no longer used (it could override interactive transport/tool flags).

## Development

The native migration is documented in [`plans/native-rust-gui.md`](plans/native-rust-gui.md); test results and remaining manual checks are in [`plans/native-rust-verification.md`](plans/native-rust-verification.md). Older verification notes in `plans/interactive-design-verification.md` describe the pre-egui app. Java import and aggregate-write warnings remain heuristic, and branch merging is not a three-way merge.

```sh
cargo test --workspace                      # core, import, diagrams, headless egui UI, fake PTY, real file exports
node --test tests/pi-extension.test.ts       # or npm test; no npm install needed
python3 scripts/terminal_smoke.py            # real pi PTY: Ctrl+P, /reload; no model requests
cargo test -p spide macos_view_snapshots -- --ignored   # macOS visual comparison (system fonts)
UPDATE_SNAPSHOTS=1 cargo test -p spide macos_view_snapshots -- --ignored  # deliberately update baselines
UPDATE_GOLDEN=1 cargo test -p spide-core      # accept changed exporter golden output
SPIDE_SMOKE_DIR=/abs/path/to/java/project cargo test -p spide-import smoke -- --nocapture
```

Headless UI tests render both examples in all tabs and cover canvas interaction, layout persistence, narrow drawers, panels, export and a fake PTY stream. Diagnostic PNGs are written to `target/ui-shots/`; reviewed macOS baselines live in `app/tests/snapshots/`. Visual comparisons are explicit because system fonts/rasterization differ by OS. Browser dev mode, the browser perf overlay, Playwright and the `/dev/*` routes are removed.

For manual verification, use a disposable project, walk through the workflows above, compare all six tabs against reference images, and check terminal input, resize and restart. Check idle CPU and large-diagram pan/zoom in a native release build; automated headless tests do not establish native frame-rate or interactive permission-dialog behavior.

`examples/library.json` is a small design with deliberate problems (a cycle, layer violations, an orphan). Copy it to a project's `design.json` while Spide is running to see it arrive as ghost proposals.
