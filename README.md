# Spide

**Spide is a native desktop app for designing software systems live, together with an AI agent.**

The UML class diagram is the centre of the workspace and every other view is derived from it. An embedded [pi](https://pi.dev) agent proposes changes as reviewable design decisions. Spide continuously measures the design against established object-oriented design principles: coupling, cohesion, responsibility assignment (RDD, GRASP) and code smells.

Spide is written in Rust with [egui](https://github.com/emilk/egui). It uses native widgets throughout, with no browser or webview.

## Contents

- [Features](#features)
- [Requirements](#requirements)
- [Getting started](#getting-started)
- [Configuration](#configuration)
- [Project layout](#project-layout)
- [Usage](#usage)
- [Spide Projects: design and code together](#spide-projects-design-and-code-together)
- [Design diagnostics](#design-diagnostics)
- [Views](#views)
- [Exports](#exports)
- [Architecture](#architecture)
- [Development](#development)
- [Known limitations](#known-limitations)

## Features

- **Agent-assisted design.** A real interactive pi terminal runs beside the canvas. The agent gathers a design brief, proposes classes and raises design decisions. All changes go through MCP tools and are only applied after you approve them.
- **Review on the diagram.** Proposed changes are drawn in place as ghosts. You can switch between alternatives with one click, inspect their consequences and approve, dismiss or request revisions.
- **Continuous design health.** A single 0–100 score combines coupling, cohesion, responsibility assignment, architecture rules and UML hygiene. Problems are shown on the canvas: hot classes, red dependency cycles, misplaced methods with a suggested home, and duplicated behaviour.
- **Architecture rules.** Presets for clean, hexagonal, layered and modular-monolith styles, plus custom forbidden dependencies and thresholds. Rules can be exported as ArchUnit tests.
- **Scenarios and change impact.** Spide scores sequence scenarios for fit and shows the ripple effect of anticipated changes.
- **History and branches.** Scrub through the design's timeline, restore earlier states, and fork, compare and merge branches.
- **Multiple views.** C4 context and container diagrams, class, domain, package, use case and UML sequence diagrams.
- **Design ↔ code synchronisation.** A Spide Project keeps the design next to its Java sources. Spide tracks a three-way sync status per element and supports agent-driven implementation and readjustment in both directions.
- **Exports.** PNG, SVG, PDF, Obsidian Canvas, PlantUML (including C4), JSON, Markdown ADRs and ArchUnit tests, individually or as a batch.

## Requirements

| Requirement | Notes |
|---|---|
| Rust toolchain | Install via [`rustup`](https://rustup.rs). |
| [pi](https://pi.dev) | Installed and logged in. Must be on `PATH`, or set `SPIDE_PI`. |
| Node.js 24+ | Required by pi and the TypeScript extension test. No frontend build is involved. |
| macOS or Linux | The PTY and control transport target macOS/Unix. **Windows is not supported.** Linux builds need the usual native graphics and windowing development libraries (X11/Wayland, OpenGL/Vulkan). |
| PlantUML *(optional)* | Only used by the exporter syntax tests. |

## Getting started

### Run from source

```sh
cargo run -p spide
```

By default Spide opens the project folder `~/Documents/Spide/untitled`. Set `SPIDE_PROJECT` to open a different folder:

```sh
project=$(mktemp -d)
cp tests/fixtures/library.json "$project/design.json"
SPIDE_PROJECT="$project" cargo run -p spide
```

`tests/fixtures/library.json` is a small design with deliberate problems: a dependency cycle, layer violations and an orphan class. If you copy it into a project's `design.json` while Spide is running, it arrives as ghost proposals for review.

Inside the app, use `/spide new` and `/spide open` in the pi terminal (or the terminal header's **Commands** button) to create or open projects.

### Build the macOS app

```sh
./scripts/setup-signing-macos.sh   # once: creates the local identity "Spide Local Signing" in the login keychain
./scripts/bundle-macos.sh          # builds target/release/bundle/macos/Spide.app
./scripts/install-macos.sh         # bundles and replaces /Applications/Spide.app (quit Spide first)
```

Bundles are signed with a stable, self-signed local identity, so macOS keeps granted privacy permissions across rebuilds. Without it, bundles are ad-hoc signed and macOS asks for permissions again after every build. Use `SPIDE_SIGN_IDENTITY` to choose a different identity and `SPIDE_INSTALL_DIR` to install somewhere other than `/Applications`. Developer ID signing and notarization for public distribution are not part of these scripts.

When launching the bundle, make sure `pi` is on `PATH`, or set `SPIDE_PI` to its absolute path.

## Configuration

| Variable | Purpose |
|---|---|
| `SPIDE_PROJECT` | Project folder to open (default `~/Documents/Spide/untitled`). |
| `SPIDE_PI` | Absolute path to the `pi` binary. |
| `SPIDE_SIGN_IDENTITY` | Code-signing identity used by the macOS scripts (default `Spide Local Signing`). |
| `SPIDE_INSTALL_DIR` | Install location used by `install-macos.sh` (default `/Applications`). |

The model is selected through pi's own configuration or `/model`. The legacy `SPIDE_PI_ARGS` variable is no longer read.

## Project layout

A design-only project folder contains:

| Path | Purpose |
|---|---|
| `design.json` | The design (active branch). Autosaved on every change and watched for external edits. |
| `.pi/mcp.json` | Points pi to Spide's MCP server (`spide`, `http://127.0.0.1:47800/mcp`). |
| `.spide/terminal-bridge.ts` | App-owned pi extension for structured prompts, context and `/spide` commands. The skill in `agent/spide-skill.md` is appended at launch. |
| `.spide/sessions/` | The agent's pi sessions. Spide continues the latest one on start. |
| `.spide/history.jsonl` | One snapshot per model change, used by the timeline. |
| `.spide/branches/` | Inactive branches (`<name>.json`). `.spide/branch` names the active one. |

A [Spide Project](#spide-projects-design-and-code-together) stores the design next to its code under `spide/` instead.

Older `design.json` files (schema 1) open unchanged. Files from the removed source attachment and code import (`.spide/source.json`, `implementations.json`, `reality.json`) are ignored.

## Usage

### Workspace and pi terminal

- pi runs once in a real PTY in the project folder, with all available tools (`--tools '*'`), its configured extensions and MCP servers, and your permission settings. The agent may edit code and run shell commands. Diagram changes always go through Spide's MCP proposals and decisions. When you first open a project, you may need to approve pi's **Trust project folder?** prompt.
- The terminal editor is the only chat input. `Ctrl+P` / `Ctrl+Shift+P` cycle models and `Ctrl+L` opens the model picker. Shift+Tab, Escape, multiline input and transcript scrolling belong to pi. The mouse wheel and trackpad are forwarded to pi's fullscreen transcript, and PageUp/PageDown also scroll it. Canvas shortcuts are disabled while the terminal has focus.
- Drag the separators to resize panes, and collapse or reopen panes without restarting pi. Layout preferences are stored locally, not in the design. On narrow windows the review rail becomes a drawer. **Restart** is always available in the terminal header.
- **In this document, `/command` means `/spide command` in the terminal** (or the same entry under **Commands**). pi's built-in commands such as `/new`, `/model`, `/settings`, `/resume` and `/export` keep their usual meaning.
- App-initiated requests (examiner, fixes, generation) reach the same session through an authenticated private Unix socket. They are never pasted into a draft you are typing. Terminal output never drives application or design state.

### Design workflow

1. **Brief.** On an empty design, the agent asks one question at a time and records your answers as prioritised *forces*: goals, quality attributes, constraints and expected changes. ⌘B or `/brief` opens the brief panel for direct editing. `/skip` (or **Skip brief**) starts without a brief.
2. **Review.** Pending decisions open in the **Review** rail, and the selected plan is drawn on the canvas. New and changed elements are teal, removals red, and everything else faded. The rail lists every pending plan with its question, alternatives (pros, cons and health), consequences, blocking problems and a readable list of changes.
   - Click an alternative to show it on the diagram. **Before / Proposed** and **Fit changes** are at the bottom of the canvas. `/compare` (or **Compare side by side**) compares alternatives on a shared layout.
   - Click diagram elements to see the before → after change and to attach them to a change request. **Request changes** sends your notes, the selected elements and the design revision to the agent. Nothing is applied; the revised plan replaces the current one.
   - **Approve design** applies exactly the visible plan, records any notes, and opens the next pending plan. **Dismiss** rejects it. ⌘Z undoes either. When the queue is empty, the rail suggests next steps.
   - **Edit plan manually (advanced)** opens a structured operation editor and the plan's JSON. Unsaved manual edits block approval until they are saved or discarded. **History** lists resolved decisions.
3. **Check behaviour.** In the Sequence view, each scenario gets a fit score: number of calls, call depth, package, layer and container crossings, aggregates touched and unknown calls. In the brief panel, the agent analyses **change scenarios** (for example "what if we must support e-books?") and Spide shows the affected classes, the ripple effect and a locality score. `/stress <name>` highlights the footprint on the canvas.
4. **Evolve.** ⌘T or `/timeline` previews the design's history. *Restore here* proposes the changes that bring that state back, and *fork* starts a branch from it. Use `/branch <name>`, `/switch`, `/compare <branch>` and `/merge <branch>`. Merged changes arrive as ghost proposals.
5. **Close the loop.** `/export adr` writes an Architecture Decision Record for each approved decision, and `/export archunit` writes Java architecture tests. A Spide Project keeps the design and Java code in sync.

### Partner modes and architecture rules

- `/mode examiner | socratic | devils_advocate | sketch`:
  - *Examiner* (default) proposes and reviews changes.
  - *Socratic* only asks questions.
  - *Devil's advocate* offers counterarguments as advice.
  - *Sketch* proposes freely and keeps alternatives for major forks.

  No mode blocks your approval.
- **Architecture rules** (brief panel, or `/rules <preset>`) are owned by the user. The agent cannot change them. They consist of a style preset, custom rules and thresholds:
  - **Presets:** `clean` (dependencies point inward), `hexagonal` (adapters never depend on each other), `layered` (downward only), `modular_monolith` (`<module>.<layer>` packages; other modules only via `<module>.api`) or `none`.
  - **Custom rules:** forbidden dependencies (`*Controller -> *Repository : go via a service`) and forbidden types per layer (`Http*` in domain).
  - **Thresholds.**
- Every issue explains why it matters and how to fix it, cites the principle it is based on (for example *GRASP Controller*), and states its confidence and source: structure, a heuristic, the agent or the code. From the health popover, **fix with agent** raises a decision with ranked resolutions.

### Canvas

- **Ghost changes.** Click a ghost or **Review N changes on the diagram** to start reviewing. Direct edits are disabled during review; pan, zoom, focus and the brief remain available. There are no bulk actions. Legacy `/accept <plan-id>` and `/reject <plan-id>` require an explicit ID.
- **Direct edits.** Double-click a name to rename it, and drag a class onto a package to move it. `/add <Class> [in <package>]` adds a class. ⌥-click pins a class in place (📌), including during `/tidy`. Direct edits apply immediately and can be undone.
- **Prune.** Right-click an element, or right-drag a box around several, to remove them. You can say what you didn't like (⏎ sends it to the agent; Esc prunes silently). Pruned elements stay in the model, hidden (`/pruned` shows them), and the agent is told not to reintroduce them. Click a pruned element to restore it.
- **Detail levels.** The slider (or `/detail 0–4`) switches between five levels:

  | Level | Shows |
  |---|---|
  | 0 | packages |
  | 1 | names |
  | 2 | responsibilities |
  | 3 *(default)* | **design**: public behaviour with argument types; accessors, constructors and private helpers folded together; associations as role names |
  | 4 | full |

  Zooming out reduces detail automatically. Exports follow the slider. Flagged methods are always visible.
- **External code.** `/external <package|Class>` marks library or framework code. External code is drawn compactly, counts less towards coupling and is not judged.
- **Lenses.**
  - `/lens concerns` colours methods by concern.
  - `/lens overlap` links duplicated behaviour and marks scattered concepts (⁂).
  - `/lens sync` shows the design ↔ code status.
  - `/lens off` turns lenses off.
  - `/implicit` shows all implicit dependencies.
- **Hover** shows an element's purpose, role, members, responsibilities, rationale, metrics and the decision that introduced it. Click an element to open its full decision record.
- **Navigation.**

  | Action | Input |
  |---|---|
  | Pan | drag |
  | Zoom | pinch or ⌘-scroll |
  | Fit | double-click empty space |
  | Zoom to and focus a class | `/find <class>` |
  | Focus a package | `/focus <package>` (Esc clears) |
  | Switch tabs | ⌘1–6 |
  | Undo / redo | ⌘Z / ⇧⌘Z |
  | Export / export again | ⌘E / ⇧⌘E |

### Visual language

| Visual | Meaning |
|---|---|
| Node size | Coupling (fan-in + fan-out). |
| Halo, blue → amber → red | Heat: coupling, cycles, god class, low cohesion, broken rules. |
| Red, straining edge | The relation is part of a dependency cycle. |
| Amber edge `layer ↑` / `module` / `forbidden` | The relation breaks a layering, module-boundary or custom rule. |
| `«async»` / `«event»` on an edge | The dependency is not a synchronous call. |
| Faint dotted edge | An implicit dependency (via attribute or parameter types or method calls) with no drawn relation. |
| Chips | `cycle`, `god class`, `LCOM n`, `rule ✗`, `orphan`, `◆ aggregate`, `«external»`, `only in code`, `⁂ concept`. |
| `↗ Home` / `⚠ kind` after a method | The method belongs elsewhere (suggested home), or it breaks a design rule. |
| Dashed purple edge (overlap lens) | Duplicated behaviour, or a method handling another class's concept. |
| Red / amber ring | Stress test: directly impacted / ripple. |
| Teal / violet / red / amber ring (sync lens) | Design ahead / code ahead / conflict / unreconciled. |
| Health ring | Overall score, 0–100. Hover for the per-principle breakdown. `→ n` is the score after accepting pending ghosts. |

### Layout

The layout optimises for presentation quality over positional stability. Any change to classes, relations or box sizes triggers a full relayout. Positions are held only while you toggle between alternatives during review.

- **Packages** are compact layered (Sugiyama) drawings that never separate a parent from its subclasses.
- **Package placement:** packages sit in layer bands (UI and infrastructure, then application, then domain). External code goes along the right or bottom edge.
- **Layers** are inferred from package names such as `domain`, `ui` and `persistence`.
- **Edges** are orthogonal, with lanes for parallel runs and shared triangles for generalisations.
- **Labels** are placed so they clear every box and chip.
- **Animation:** the layout settles without animated drift. Physics only moves classes you drag, and an idle canvas uses no CPU.

## Spide Projects: design and code together

Pure-design projects need no code, Git or Java. A **Spide Project** is a module folder where the design lives alongside its Java sources:

```
addOns/restextraction/      project root; pi runs here (repository AGENTS.md files apply)
  spide/project.json        manifest: name, roots, excludes, code root, compare filters, test command   (committed)
  spide/design.json         the design (active branch)                                                  (committed)
  spide/sync.json           sync baseline: both sides as they last agreed                               (committed)
  .spide/  .pi/mcp.json     local state, excluded from Git via .git/info/exclude
```

- **Create or open** via **Project** in the rail, `/project`, or *File › Open Spide Project…*. The setup form detects source roots and the Java root package, records a test command for the agent, and copies the current design and its history into `spide/`.
- **Indexing.** Java sources are parsed with tree-sitter when the project opens and whenever a `.java` file changes. A partial or failed index is labelled as such, and absences are not trusted.
- **Identity** is exact: a design class maps to `code_root.package.Name`, or to an explicit `link_code` target. Methods match by name and parameter types, and fields by name.
- **What is compared:** package, kind, fields, method signatures and modifiers, and inheritance. **Not compared by default:** private methods, plain accessors, constructors, unlisted static fields and external classes.
- **Sync status** is a three-way comparison against the baseline: **in sync**, **design ahead**, **code ahead**, **conflict** (resolve with **Keep design** or **Keep code**), **unreconciled** or **accepted**. Matching elements advance the baseline automatically. `/synced` accepts the remaining differences. Pending ghosts are never compared.
- **Readjust (code → design).** `/readjust [Class …]` has the agent (skill `spide-readjust`) raise proposals from the code. If the design is empty, this bootstraps it.
- **Implement (design → code).** `/implement [Class …]` sends the agent (skill `spide-implement`) a brief per class. The agent edits with pi's normal tools and permissions and runs the test command. It never commits, stashes, resets or switches branches, and it asks before deleting files. Spide then re-indexes and reports progress.
- **Verify.** `/verify` re-indexes and asks the agent to run the tests. Structural agreement does not prove correct behaviour.
- **Agent tools** (Spide Projects only): `get_project`, `get_sync`, `get_code`, and the `link_code` / `unlink_code` operations.

## Design diagnostics

Health is a weighted geometric mean of five principle scores. Each score is computed from the severity and confidence of its issues, relative to the size of the design:

| Principle | Weight |
|---|---|
| Coupling | 0.25 |
| Cohesion | 0.20 |
| Responsibilities | 0.30 |
| Architecture rules | 0.15 |
| UML hygiene | 0.10 |

- **Purpose vs behaviour.** Each class is assigned a role from, in order of precedence: an explicit `role`, the agent's assessment, its name, its stereotype or its layer. Possible roles are presentation, controller, service, domain, persistence, adapter, output, creator, utility and value. Each method is assigned a concern and an RDD kind (knowing, doing, deciding). Mismatches are flagged: UI classes doing domain work, entities persisting or formatting themselves, fat controllers, vague names (`*Manager`, `*Util`) and classes mixing concerns.
- **Duplication and responsibility placement.** Method pairs get an explainable similarity score. Delegation, polymorphic overrides and UI event handlers are not counted as duplication. Spide also detects shotgun surgery and middle men. `suggest_home` ranks better homes for misplaced behaviour, using Information Expert and, where no expert fits, a new Pure Fabrication. It checks caller dependencies and simulates the effect on health.
- **Rule checks.** Law of Demeter, Feature Envy, data classes, UI driving the domain without a controller, Creator, type-code switches (replace with polymorphism), long methods and long parameter lists. UML checks cover argument types, repeated inherited attributes, incomplete Composites and single-element Visitors.
- **Agent judgement.** The agent records a per-class assessment (`assess_class`). Agreement raises an issue's confidence. On serious issues, disagreement is surfaced for you to resolve. Assessments go stale when the class changes.
- **Long methods.** Java methods with more than 30 non-blank, non-comment body lines are flagged. The threshold is configurable as `rules.thresholds.max_method_lines`. Methods in a pure design have unknown length and are not penalised.

## Views

| View | Description |
|---|---|
| **System** (C4) | *Context*: actors, the system and external systems. *Containers*: deployable units, their packages, datastores and links. Container dependencies are derived from class dependencies. Spide flags single points of failure, synchronous cross-container call chains and inconsistencies between views. |
| **Class** | The main design canvas. |
| **Domain** | A conceptual domain model generated by the agent (`set_domain_model`): concepts, attributes, associations with multiplicities, and generalisations. It is marked stale when the class model changes. |
| **Package** | A computed UML package diagram with layers and dependencies. |
| **Use case** | Generated by the agent from the class model. |
| **Sequence** | UML sequence diagrams for a scenario: objects, multi-objects and actors; `loop`, `alt`, `opt`, `par`, `break`, `critical` and `ref` fragments; creation and destruction; activation bars and implied replies. Every call is checked against the receiver's class. |

## Exports

| Format | Notes |
|---|---|
| **PNG / SVG / PDF** | Rendered from the same scene and layout as the canvas. You can set the theme, scale, legend, diagnostics and ghost visibility, and these settings are saved per project. PNGs record their DPI (72 × scale), so they display at the intended size and stay sharp on Retina screens. |
| **Obsidian Canvas** | [JSON Canvas 1.0](https://jsoncanvas.org) for every diagram, in the same layout. Elements become Markdown cards, package frames become groups, and UML notation is spelled out in edge labels. |
| **PlantUML** | Exported per view, or all views at once with `/export all`. The System view exports C4-PlantUML. PlantUML uses its own layout; PNG is the pixel-exact export. |
| **JSON** | `/save` and `/open`. The project's `design.json` is always current. |
| **ADR** | `/export adr`: one Markdown (MADR) record per decision. |
| **ArchUnit** | `/export archunit`: a JUnit 5 `ArchitectureTest.java` enforcing layering, custom rules, module boundaries and the absence of package cycles. Requires the test dependency `com.tngtech.archunit:archunit-junit5`. |

**Batch export.** In Export (⌘E), choose **All diagrams**. Then pick the diagrams, the detail levels, the formats and one or more settings profiles. Stale agent-generated diagrams are regenerated first. Spide writes one file per diagram × detail level × profile × format. Files go into a new timestamped folder, are written atomically, and come with progress reporting and cancellation. If the design changes during the export, the batch stops, so every file comes from the same revision.

## Architecture

```
crates/spide-core     Model, operations, store (proposals, decisions, undo), metrics and health,
                      responsibility and overlap analysis, rules, impact, scenarios, project
                      manifest and design ↔ code sync, derived views (incl. C4), diff, agent tool API,
                      exporters (PlantUML, C4, PNG, ADR, ArchUnit)
crates/spide-import   Java source indexing with tree-sitter for Spide Project sync
crates/spide-diagram  GUI-independent scenes, compound Sugiyama layout, orthogonal routing, physics,
                      all diagram views, themes, SVG and Obsidian Canvas export
app                   eframe/egui desktop app: canvas, panels, terminal (alacritty_terminal),
                      MCP server (axum), pi PTY and private control socket, file watcher,
                      history and branches
agent                 Agent skill, Spide Project skills and the bundled pi integration extension
```

- **Single source of truth.** The Rust store owns all state. The UI sends intents (prompt, prune, approve, request revision, undo) and renders the snapshots it receives.
- **Agent access through MCP only.** Agents change the design through MCP tools, and every change becomes a user-approved decision. Invalid operations are rejected atomically. Forces, scenarios, containers and external systems are operations too, so they share the same review, undo and watcher pipeline. Architecture rules can only be changed by the user.
  - Core tools: `get_design`, `get_metrics`, `get_view`, `propose_decision`, `propose_ops`, `set_domain_model`, `set_use_cases`, `set_sequence`, `set_change_impact`.
  - Analysis tools: `get_responsibilities`, `get_overlap`, `suggest_home`, `assess_class`.
- **Provenance.** Every committed element records its origin: decision, proposal, `user` or `import`.
- **Collaboration.** Other agents can join. A pi session started in the project folder picks up `.pi/mcp.json`. Any agent may also edit `design.json` directly; Spide turns such edits into ghost proposals instead of applying them silently.
- **Local-only MCP server.** A minimal streamable-HTTP JSON-RPC implementation (`initialize`, `tools/list`, `tools/call`) bound to `127.0.0.1`. Every route requires a loopback `Host` header and, when present, a local `Origin`, which blocks cross-site requests and DNS rebinding.

## Development

```sh
cargo test --workspace                                      # core, import, diagrams, headless UI, fake PTY, file exports
npm test                                                    # pi extension test (node --test; no npm install needed)
python3 scripts/terminal_smoke.py                           # real pi PTY smoke test; makes no model requests
cargo test -p spide macos_view_snapshots -- --ignored       # macOS visual snapshot comparison
UPDATE_SNAPSHOTS=1 cargo test -p spide macos_view_snapshots -- --ignored   # update visual baselines
UPDATE_GOLDEN=1 cargo test -p spide-core                    # accept changed exporter golden output
SPIDE_SMOKE_DIR=/abs/path/to/java/project cargo test -p spide-import smoke -- --nocapture
```

- **Fixtures.** Test designs live in `tests/fixtures/`.
- **Headless UI tests** render every view and cover canvas interaction, layout persistence, panels, export and a fake PTY stream.
- **Snapshots.** Diagnostic screenshots are written to `target/ui-shots/`, and reviewed macOS baselines live in `app/tests/snapshots/`. Visual comparisons are opt-in because font rasterisation differs between operating systems.
- **Manual checks.** Before a release, use a disposable project to walk through the workflows above and compare all views against reference images. Also check terminal input, resizing and restart, and measure idle CPU and large-diagram pan/zoom in a release build. Headless tests cannot establish native frame rates or interactive permission-dialog behaviour.

## Known limitations

- Windows is not supported.
- macOS bundles are signed locally only; Developer ID signing and notarization are not provided.
- Java indexing and aggregate-write warnings are heuristic.
- Branch merging is not a three-way merge.
