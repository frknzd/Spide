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
- [Design evidence](#design-evidence)
- [Languages, LSP and linters](#languages-lsp-and-linters)
- [Neovim](#neovim)
- [Agents, headless mode and permissions](#agents-headless-mode-and-permissions)
- [Migration](#migration)
- [Views](#views)
- [Exports](#exports)
- [Lucid](#lucid)
- [Architecture](#architecture)
- [Development](#development)
- [Known limitations](#known-limitations)

## Features

- **Agent-assisted design.** A real interactive pi terminal runs beside the canvas. The agent gathers a design brief, proposes classes and raises design decisions. All changes go through MCP tools and are only applied after you approve them.
- **Review on the diagram.** Proposed changes are drawn in place as ghosts. You can switch between alternatives with one click, inspect their consequences and approve, dismiss or request revisions.
- **Design evidence.** Inspect findings from the design, source, language servers, and linters. The Evidence panel shows check status, gaps, and exceptions without grading the design.
- **Architecture rules.** Presets for clean, hexagonal, layered and modular-monolith styles, plus custom forbidden dependencies and thresholds. Rules can be exported as ArchUnit tests.
- **Scenarios and change impact.** Spide scores sequence scenarios for fit and shows the ripple effect of anticipated changes.
- **History and branches.** Scrub through the design's timeline, restore earlier states, and fork, compare and merge branches.
- **Multiple views.** C4 context and container diagrams, class, domain, package, use case and UML sequence diagrams.
- **Seven languages.** Rust, TypeScript, Python, Java, C, C++ and Clojure are indexed into one language-neutral graph (modules, types, functions, traits/protocols) — functions are never forced into fake classes.
- **Neovim as the editor.** ⌘-click a class or method to open it in your Neovim (or an embedded one); `:SpideReveal` shows the code under your cursor on the diagram. Agent edits to open buffers are undoable buffer changes that never overwrite unsaved work.
- **Agent first.** Everything meaningful is an MCP tool, also without a window: `spide-headless` serves MCP over stdio or authenticated HTTP. Project permissions (read-only, propose-only, autonomous) and a compact trace of every change.
- **Design ↔ code synchronisation.** A Spide Project keeps the design next to its sources. Spide tracks a three-way sync status per element and supports agent-driven implementation and readjustment in both directions.
- **Exports.** PNG, SVG, PDF, Obsidian Canvas, PlantUML (including C4), JSON, Markdown ADRs and ArchUnit tests, individually or as a batch.
- **Lucid sync.** Every diagram is a Lucidchart document your colleagues view, comment on and edit. pi keeps them in sync, git-like: pushes update the documents in place, colleagues' edits come back as ghost proposals and their comments become design input for the agent.

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

Spide starts on a welcome screen: **New Project…**, **Open Project…**, **Import Design JSON…** and your recent projects (pin, remove, locate moved ones). Tick *Reopen the last project at startup* to skip it. Set `SPIDE_PROJECT` to open a specific folder directly:

```sh
project=$(mktemp -d)
cp tests/fixtures/library.json "$project/design.json"
SPIDE_PROJECT="$project" cargo run -p spide
```

`tests/fixtures/library.json` is a small design with deliberate problems: a dependency cycle, layer violations and an orphan class. If you copy it into a project's `design.json` while Spide is running, it arrives as ghost proposals for review.

The **File** menu has New Project (⌘N), Open Project (⌘O), Open Recent, Close Project (⇧⌘W), Save (⌘S, the design also autosaves), Save Design Copy (⇧⌘S, writes a JSON copy; the project stays where it is), Import Design JSON, Export and Reveal Project in Finder. Leaving a project (switching, closing, ⌘W, ⌘Q) first asks about anything that would be lost: a failed save, unsaved plan drafts, unsaved Neovim buffers, a busy agent or a running export. If a design can't be saved, a banner stays until Retry or Save Design Copy succeeds; a design file that can't be read is never overwritten — **Recover a Copy…** builds a new project from the newest recovery copy (`.spide/recovery/`) or history.

### Opening from Finder, drops and the command line

Drop a project folder, `design.json`, `spide/project.json` or `.spideproject` shortcut onto the app. A standalone design JSON goes through the import preview; project files use the same validated, guarded opener as File → Open. Only one open request is processed at a time.

```sh
cargo run -p spide -- /path/to/project
cargo run -p spide -- --project /path/to/project
```

**File → Save Project Shortcut…** writes a `.spideproject` reference to the current folder (it does not copy or relocate the project). The macOS bundle registers only this extension, not all JSON files; double-clicking the shortcut routes through the same opener. A moved project may need a new shortcut. Finder/Dock integration requires the bundled app.

### Build the macOS app

```sh
./scripts/setup-signing-macos.sh   # once: creates the local identity "Spide Local Signing" in the login keychain
./scripts/bundle-macos.sh          # builds target/release/bundle/macos/Spide.app
./scripts/install-macos.sh         # bundles and replaces /Applications/Spide.app (quit Spide first)
```

Bundles are signed with a stable, self-signed local identity, so macOS keeps granted privacy permissions across rebuilds. Without it, bundles are ad-hoc signed and macOS asks for permissions again after every build. Use `SPIDE_SIGN_IDENTITY` to choose a different identity and `SPIDE_INSTALL_DIR` to install somewhere other than `/Applications`. Developer ID signing and notarization for public distribution are not part of these scripts.

When launching the bundle, make sure `pi` is on `PATH`, or set `SPIDE_PI` to its absolute path.

## Configuration

**Spide → Settings… (⌘,)** controls reopen-last-project, the default new-project location, interface scale, terminal text size, reduced motion, automatic pi startup and embedded/external Neovim preference. Preferences and per-project pane/view/camera state are local, not committed with the design. Reset Workspace Layout is separate. Provider/model selection and credentials remain owned by pi (`/model`, `/settings`).

Unsent decision notes and unfinished forms recover locally per project and branch; changed revisions are flagged and nothing is submitted automatically. A running pi session warns before leaving because its unsent terminal input cannot be inspected or preserved. Local `input-drafts.json` contains your draft text: treat it as private. Existing manual plan drafts continue to use `proposal-drafts.json`.

**Help → Activity and Errors…** retains recent desktop errors/status changes, with Copy Details and safe, workspace-scoped retries. **Help → Integrations…** shows pi, MCP, Neovim and index/tool-discovery status and offers Start/Restart/Stop Agent and Re-index. Tool discovery does not run analysis or grant trust. Stop requests interrupt a turn; they do not certify completion. **Help → Open Logs** opens the local activity-log folder; these logs may include paths and error text, so review them before sharing. **Copy Diagnostics** deliberately omits paths, source, draft text, errors and tokens. About includes the version, source revision and build time; bundled documentation and shortcut reference work offline.

| Variable | Purpose |
|---|---|
| `SPIDE_PROJECT` | Project folder to open at startup (default: the welcome screen). |
| `SPIDE_PREFERENCES` | Local preferences file (default `~/Library/Application Support/Spide/preferences.json`): recent projects, reopen-last. |
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
| `.spide/lucid/` | Lucid sync (local): `config.json` (enabled, frontier branch, folder, one document per diagram) and `base/` (what each document held at the last sync). |
| `.spide/skills/` | App-owned pi skills (Lucid sync; in a Spide Project also readjust/implement), refreshed at every launch. |

A [Spide Project](#spide-projects-design-and-code-together) stores the design next to its code under `spide/` instead.

Older `design.json` files (schema 1) open unchanged. Files from the removed source attachment and code import (`.spide/source.json`, `implementations.json`, `reality.json`) are ignored.

## Usage

### Workspace and pi terminal

- pi runs once in a real PTY in the project folder, with all available tools (`--tools '*'`), its configured extensions and MCP servers, and your permission settings. The agent may edit code and run shell commands. Diagram changes always go through Spide's MCP proposals and decisions. When you first open a project, you may need to approve pi's **Trust project folder?** prompt.
- The terminal editor is the only chat input. `Ctrl+P` / `Ctrl+Shift+P` cycle models and `Ctrl+L` opens the model picker. Shift+Tab, Escape, multiline input and transcript scrolling belong to pi. The mouse wheel and trackpad are forwarded to pi's fullscreen transcript, and PageUp/PageDown also scroll it. Canvas shortcuts are disabled while the terminal has focus.
- Drag the separators to resize panes, and collapse or reopen panes without restarting pi. Layout preferences are stored locally, not in the design. On narrow windows the review rail becomes a drawer. **Restart** is always available in the terminal header.
- **In this document, `/command` means `/spide command` in the terminal** (or the same entry under **Commands**). pi's built-in commands such as `/new`, `/model`, `/settings`, `/resume` and `/export` keep their usual meaning.
- App-initiated requests (examiner, fixes, generation) reach the same session through an authenticated private Unix socket. They are never pasted into a draft you are typing. Terminal output never drives application or design state.

### Everyday conveniences

- **File → Copy Diagram as Image** copies the active diagram at 2× through the existing renderer. **Open Export Folder** opens the most recent export's parent folder for this project.
- **File → Back Up Design State…** writes the current design, history and branches to a new folder outside the project. It does not copy source code, tooling configuration, sessions, credentials, endpoints, locks or local UI drafts. A code-backed backup opens as a design-only project; this is not a repository backup. Existing destinations and symlinks are refused, and any incomplete publication is reported. **Duplicate Design Project…** is available for design-only projects and offers opening the copy without moving the original.
- Click the **☆/★** in either command palette to pin/unpin commands; pinned commands appear first and remain searchable.
- **New Project** offers a blank design or a small **Layered starter** example. Templates create design elements only, never source files.

### Design workflow

1. **Brief.** On an empty design, the agent asks one question at a time and records your answers as prioritised *forces*: goals, quality attributes, constraints and expected changes. ⌘B or `/brief` opens the brief panel for direct editing. `/skip` (or **Skip brief**) starts without a brief.
2. **Review.** Pending decisions open in the **Review** rail, and the selected plan is drawn on the canvas. New and changed elements are teal, removals red, and everything else faded. The rail lists every pending plan with its question, alternatives (pros, cons and findings), consequences, blocking problems and a readable list of changes.
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
- Every issue explains why it matters and how to fix it, cites the principle it is based on (for example *GRASP Controller*), and states its confidence and source: structure, a heuristic, the agent or the code. From the Evidence panel, **fix with agent** can raise a decision with alternatives.

### Canvas

- **Ghost changes.** Click a ghost or **Review N changes on the diagram** to start reviewing. Direct edits are disabled during review; pan, zoom, focus and the brief remain available. There are no bulk actions. Legacy `/accept <plan-id>` and `/reject <plan-id>` require an explicit ID.
- **Direct edits.** Double-click a name to rename it, and drag a class onto a package to move it. `/add <Class> [in <package>]` adds a class. ⌥-click pins a class in place (📌), including during `/tidy`. Direct edits apply immediately and can be undone.
- **Select and edit.** Click selects (⇧-click adds), right-drag a box to select several, ↑/↓ step through classes, Esc clears. Right-click opens the element menu: Inspect, Rename, Edit Properties (⌘I), Reveal in Editor, Pin, Move to Package, Copy Name, Copy/Cut/Paste/Duplicate (⌘C ⌘X ⌘V ⌘D), Remove/Restore. **Edit › New Class… (⇧⌘N)**, New Package, New External System and New Container add elements without the agent. Each save, paste or move is one change that ⌘Z undoes. These edit the design only; code changes go through Implement. Copies get fresh names (`OrderCopy`), keep relations between the copied classes, and never copy code links. Domain models, use cases and sequences are still built by the agent.
- **Prune.** Delete/Backspace (or **Remove from Design**) prunes the selection; ⌥-right-click and ⌥-right-drag are the instant prune gestures. You can say what you didn't like (⏎ sends it to the agent; Esc prunes silently). Pruned elements stay in the model, hidden (`/pruned` shows them), and the agent is told not to reintroduce them. Click a pruned element (or ⌘Z) to restore it. Code is never touched.
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
| Evidence | Findings, check status, gaps, and exceptions. |

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
- **Implement (design → code).** `/implement [Class …]` sends the agent (skill `spide-implement`) a brief per class. The agent edits with pi's normal tools and permissions and runs the test command. It can follow explicit Git requests (including switching branches), subject to repository instructions, and asks before deleting files. Spide then re-indexes and reports progress.
- **Verify.** `/verify` re-indexes and asks the agent to run the tests. Structural agreement does not prove correct behaviour.
- **Agent tools** (Spide Projects only): `get_project`, `get_sync`, `get_code`, and the `link_code` / `unlink_code` operations.

## Design diagnostics

The diagnostics below are findings, not a design grade. Each retains its source and basis.

- **Purpose vs behaviour.** Each class is assigned a role from, in order of precedence: an explicit `role`, the agent's assessment, its name, its stereotype or its layer. Possible roles are presentation, controller, service, domain, persistence, adapter, output, creator, utility and value. Each method is assigned a concern and an RDD kind (knowing, doing, deciding). Mismatches are flagged: UI classes doing domain work, entities persisting or formatting themselves, fat controllers, vague names (`*Manager`, `*Util`) and classes mixing concerns.
- **Duplication and responsibility placement.** Method pairs get an explainable similarity score. Delegation, polymorphic overrides and UI event handlers are not counted as duplication. Spide also detects shotgun surgery and middle men. `suggest_home` offers unranked candidates with legality, data ownership, role fit, and simulated fan-out and cohesion observations.
- **Rule checks.** Law of Demeter, Feature Envy, data classes, UI driving the domain without a controller, Creator, type-code switches (replace with polymorphism), long methods and long parameter lists. UML checks cover argument types, repeated inherited attributes, incomplete Composites and single-element Visitors.
- **Agent judgement.** The agent records a per-class assessment (`assess_class`). Agreement raises an issue's confidence. On serious issues, disagreement is surfaced for you to resolve. Assessments go stale when the class changes.
- **Long methods.** Java methods with more than 30 non-blank, non-comment body lines are flagged. The threshold is configurable as `rules.thresholds.max_method_lines`. Methods in a pure design have unknown length and are not penalised.

## Design evidence

`get_health` returns the versioned `evidence-1` report: scope, findings and provenance, analysis status, evidence gaps, and exceptions. A missing check means unknown. No findings means only that the available checks reported none. Heuristics such as Java `instanceof` and private-method documentation checks are leads, not correctness verdicts.

Agents can `propose_exception` with the rule, scope, reason, evidence, and expiry or review condition. Only the owner approves it; suppressed findings remain visible in Evidence. Numeric rating tools are retired.

## Languages, LSP and linters

| Language | Parser extracts | Language server (default) | Linter (default) |
|---|---|---|---|
| Rust | modules, structs, enums, traits, impls, functions | rust-analyzer | Clippy (`cargo clippy --message-format=json`) |
| TypeScript | modules, classes, interfaces, enums, aliases, functions, arrow functions | vtsls (or typescript-language-server) | ESLint (`node_modules/.bin` first) |
| Python | modules, classes, Protocols, dataclasses, functions | pyright (or basedpyright, pylsp) | Ruff |
| Java | packages, classes, interfaces, enums, records | jdtls (needs a Maven/Gradle build) | — (add one under `tools.java.linters`) |
| C | file modules, structs, unions, enums, functions | clangd | clang-tidy (needs `compile_commands.json`) |
| C++ | namespaces, classes, pure-virtual interfaces, out-of-class methods | clangd | clang-tidy |
| Clojure | namespaces, defn/def, protocols, records/types, multimethods | clojure-lsp | clj-kondo |

- **Tools are found** on `PATH`, in the project's `node_modules/.bin`, and in Neovim's mason directory — Spide reuses what your editor installed. `doctor` (MCP) lists what is available, missing (with install hints) or not configured.
- **Configuration** lives in `spide/project.json`: `"language"`, `"languages"` (mixed workspaces) and per-language `"tools"` (`lsp` argv, `init_options`, `settings`, `linters`, `env`, `enabled`). Example: `"tools": {"java": {"lsp": ["jdtls", "--jvm-arg=-Xmx4G"], "init_options": {"settings": {"java": {"import": {"gradle": {"java": {"home": "/path/to/jdk17"}}}}}}}}`.
- **Running tools executes project code** (build scripts, macros, plugins). Spide only runs them in workspaces you trusted (`spide-headless trust --trusted`), with timeouts, and without an OS sandbox. `analyze` runs servers and linters as a cancellable job; results land in `get_evidence` and health. While Neovim is attached, its live LSP diagnostics are used for open files (unsaved buffers are labelled previews).
- **Limits are explicit:** macros and the C preprocessor aren't expanded, templates aren't instantiated, dynamic dispatch resolves by unique name only; unresolved references are reported, never guessed.

## Neovim

Neovim is Spide's editor — there is no code editor in Spide itself.

- **Install** the plugin from `integrations/nvim` (LazyVim: `{ dir = "~/Projects/Spide/integrations/nvim", opts = { keymaps = "<leader>j" } }`). It auto-connects when a buffer inside a running Spide (desktop or headless) workspace opens.
- **Spide → Neovim:** ⌘-click a class or method on the canvas, `/goto Class.method`, or the agent's `open_source`. Your external Neovim is preferred; without one, Spide starts the **embedded Neovim** (your config, in the side pane: **Neovim ▸** in the pane header or `/nvim`).
- **Neovim → Spide:** `:SpideReveal` (or `:SpideFollow` to follow the cursor) selects the element on the diagram; `:SpideAsk <text>` sends a change request with the code location to the agent; `:SpideHealth`, `:SpideTrace`, `:SpidePatches` / `:SpideApplyPatch <id>` review and apply staged agent patches.
- **Unsaved buffers belong to Neovim.** Agent edits to an open file are applied inside Neovim as one undo step, guarded by `changedtick`; a dirty or changed buffer is a conflict, never overwritten. Closed files are written atomically if their content hash still matches. Spide won't quit while the embedded Neovim has unsaved buffers, and `editor_handoff` refuses to strand them unless told to save or keep them.
- **Security:** Spide calls only the plugin's fixed functions over Neovim's RPC socket (no arbitrary Lua or Ex commands). Human actions from Neovim carry a user token Spide pushes over that socket (never written to disk), so they work even when agents are read-only.

## Agents, headless mode and permissions

- **Headless.** `spide-headless [--project DIR] mcp` serves MCP over stdio; `serve [--port N]` over loopback HTTP with a bearer token published owner-only in `.spide/endpoint.json` and `$XDG_STATE_HOME/spide/endpoints/`. No window, pi or Neovim needed. `trace`, `call TOOL JSON` and `trust` are CLI helpers. One process owns a workspace (`.spide/lock`); when the desktop opens a workspace a headless Spide serves, the headless one hands it over and forwards its agents to the desktop, and takes it back when the desktop quits.
- **Tools** cover the whole application: design (propose, decisions, approve, apply ops, prune, rules, mode), code (`get_symbols`, `find_symbol`, `element_at`, `get_source`, `propose_patch`, `apply_patch`, `revert_op`), evidence and health (`doctor`, `analyze`, `get_evidence`, `get_health`, assessments, exceptions), history and branches, exports, sync, Lucid (`lucid_*`, `get_comments`, `address_comment`), jobs (`run_check`, `job_status`, `cancel_job`), editor (`open_source`, `reveal`, `get_editor`, `editor_handoff`), events and trace. Purely visual commands (`ui_command`: lens, detail, focus …) need the desktop.
- **Permissions** per project, kept outside the project in `~/.config/spide/trust.json` (an agent editing project files can't change them): **read-only** (inspect), **propose-only** (default: proposals, staged patches, assessments), **autonomous** (also apply decisions, ops and patches, and run owner-approved commands in trusted workspaces). Set with `spide-headless trust --permission autonomous [--trusted] [--approve-command "cargo test"]`. Owner-only actions (permissions, trust, rules, exceptions, priorities) are never available to agents. Changes need `expected_rev` (or a content hash) and accept an `idempotency_key`.
- **Trace.** Every mediated operation is journaled (`.spide/trace/journal.jsonl`): intent before acting, outcome after (crashes show as *interrupted*). `/trace` (or `get_trace`, `spide-headless trace`) shows one line per change — intent → changed symbols/files → finding changes → checks → outcome — filterable to failures, conflicts or unverified changes; expand a row for the policy decision, revisions, issues, errors and the exact diff, and revert source changes as new guarded changes. Secrets are redacted and output is bounded. It is an operational record, not a tamper-proof audit log.

## Migration

- Existing `design.json` files and Java Spide Projects open unchanged; schema-1 code links are Java links. New fields (`language`/`kind`/`id` on links, `languages`/`tools` in the manifest, health assessments/exceptions/priorities in the design) are optional and only written when used.
- Numeric health ratings are retired. Existing project ratings and trace history remain readable as historical data; new reports, previews, and exports are score-free.
- The MCP endpoint now requires its bearer token. pi gets it through `SPIDE_MCP_TOKEN` (referenced from `.pi/mcp.json`, never written there); other clients read `.spide/endpoint.json`.
- `history.rs`/`branches.rs` moved into `spide-service`, shared by desktop and headless.

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

**Presentation and filenames.** Automatic layout and Readjust/Tidy aim for a compact landscape drawing, with **16:9 as a soft target**. Readability and hierarchy take priority; long sequence diagrams can remain tall. Unrelated arrows use separate attachment points spread along node sides. Saved class/domain placements are preserved on opening; use Tidy to apply the updated layout. Exports reflect the same canvas geometry, not a separate export layout.

Diagram filenames use `<view>[-<package-or-scenario>][-boundary][-<level>-<detail>][-<profile>].<ext>`, for example `class-model.call-boundary-3-design.pdf`. Package paths stay readable; unsafe characters are sanitized, name components are length-limited, and batch collisions get numeric suffixes. Profiles appear when exporting multiple profiles. Single-export suggestions also include the project name; explicit saved paths and Export Again are unchanged. PlantUML keeps its full-model convention (for example `class-model.call-boundary-plantuml.puml`) rather than suggesting detail-level filtering.

**Batch export.** In Export (⌘E), choose **All diagrams**. Then pick the diagrams, the detail levels, the formats and one or more settings profiles. Stale agent-generated diagrams are regenerated first. Spide writes one file per diagram × detail level × profile × format. Files go into a new timestamped folder, are written atomically, and come with progress reporting and cancellation. If the design changes during the export, the batch stops, so every file comes from the same revision.

**Package + boundary alternatives.** In the Class toolbar or Export dialog select an exact package, or right-click a package and choose **Show Package + Boundary** (`/scope <package>`; `/scope all` restores the full view). The alternative includes every visible class in that package plus direct incoming and outgoing neighbors from other packages—not their siblings or second-hop neighbors. Internal and crossing relationships are kept; context-to-context relationships are omitted. Existing styling/detail controls remain unchanged, and full-project health and saved full-view positions are untouched. Empty packages stay empty; partial dependency graphs retain their existing coverage limitations.

The same scope applies to previews, clipboard images/SVG, PNG/SVG/PDF, Obsidian Canvas, PlantUML and Lucid. Batch export has an opt-in **Include package + boundary alternatives** toggle, with collision-safe filenames. Agent text export accepts `export({"format":"plantuml","package":"ui","path":"ui.puml"})`; scoped Lucid keys are `class.package:<exact package name>`. Package scope does not filter unrelated C4, domain-concept, sequence or use-case views.

## Lucid

Spide keeps one [Lucid](https://lucid.co) (Lucidchart) document per diagram — system context, containers, class,
domain, package, use cases and one per sequence — so people outside Spide can follow, comment on and edit the design.
The embedded pi agent talks to Lucid's hosted MCP server (`https://mcp.lucid.app/mcp`); Spide never sees Lucid
credentials.

- **Setup.** Help → Integrations… → *Enable Lucid sync* (or `/spide lucid enable`), then sign in once in the terminal
  with `/mcp login lucid`. On Lucid Team/Enterprise accounts an admin must enable MCP access first. The documents go
  into a Lucid folder named after the design.
- **Push** (`/spide lucid push`, Export → *Push to Lucid*, or Integrations → *Push*) creates missing documents and
  updates existing ones **in place**: same URL, colleagues' comments, notes, extra shapes and styling stay; only shapes
  Spide drew (tagged with a `spide.id` shape-data field) are changed, moved or deleted. Classes are native Lucid UML
  class shapes with compartments, relations typed UML relationships with multiplicities, sequences Lucid sequence
  diagrams (their markup round-trips exactly). Renaming a class in Spide renames the shape.
- **Pull** (`/spide lucid pull`) turns colleagues' edits since the last sync into ghost proposals — additions and
  changes, removals, *conflicts* (Lucid's version of what both sides changed) and moved boxes, separately — and their
  comment threads and sticky notes into Spide comments. The agent sees open comments in `get_design`/`get_comments`,
  links answers with `address_comment`, and the canvas shows a 💬 badge on commented elements.
- **Git-like.** `/spide lucid status` reports per diagram: in sync, local ahead (push), remote ahead (pull), diverged
  (pull first) or unlinked. A push is refused while the document changed since the last sync or while the last pull's
  proposals are undecided; rejected remote edits are overwritten by the next push. A failed push records nothing and
  can simply be rerun (the edit scripts are idempotent and Lucid applies each one atomically).
- **Frontier branch.** Only `main` (configurable with `lucid_link`) syncs; on other branches Lucid actions are refused.
- **Linking an existing document:** `/spide lucid link class https://lucid.app/lucidchart/…`, then pull imports it.
- **Agents without pi** (e.g. Claude Code with the Lucid connector and `spide-headless`) use the same tools:
  `lucid_read_script` (run with Lucid's `lucid_run_script`), `lucid_status`, `lucid_plan_push` (edit scripts),
  `lucid_record_push`, `lucid_pull`, `lucid_export`, `lucid_link`/`lucid_unlink`. They never change the design
  directly, so they are allowed in propose-only projects.

## Architecture

```
crates/spide-core     Model, operations, store (proposals, decisions, undo), metrics and health,
                      responsibility and overlap analysis, rules, impact, scenarios, project
                      manifest and design ↔ code sync, derived views (incl. C4), diff, agent tool API,
                      exporters (PlantUML, C4, PNG, ADR, ArchUnit), Lucid sync model (UML text,
                      sequence markup, snapshots, three-way pull mapping, in-place patches, sync state)
crates/spide-import   Source indexing with tree-sitter: Java importer and the Rust, TypeScript, Python, C,
                      C++ and Clojure adapters producing the language-neutral semantic graph
crates/spide-lsp      Managed stdio LSP client (encoding negotiation, diagnostics, symbols), default
                      tooling per language, doctor, linter output parsers, a mock server for tests
crates/spide-service  GUI-independent application service: workspace lock, policy, trace journal,
                      jobs, MCP dispatch, code intelligence, evidence, Neovim RPC, patches,
                      assessments, history and branches
crates/spide-headless `spide-headless`: MCP over stdio/HTTP without a window
integrations/nvim     The Neovim plugin (Lua) and its tests
crates/spide-diagram  GUI-independent scenes, compound Sugiyama layout, orthogonal routing, physics,
                      all diagram views, themes, SVG, Obsidian Canvas and Lucid (writer) export
app                   eframe/egui desktop app: canvas, panels, terminal (alacritty_terminal),
                      MCP server (axum), pi PTY and private control socket, file watcher,
                      history and branches
agent                 Agent skill, Spide Project skills and the bundled pi integration extension
```

- **Single source of truth.** The Rust store owns all state. The UI sends intents (prompt, prune, approve, request revision, undo) and renders the snapshots it receives.
- **Agent access through MCP only.** Agents change the design through MCP tools; by default every change becomes a user-approved decision (see permissions). Invalid operations are rejected atomically. Forces, scenarios, containers and external systems are operations too, so they share the same review, undo and watcher pipeline. Architecture rules can only be changed by the user.
  - Core tools: `get_design`, `get_metrics`, `get_view`, `propose_decision`, `propose_ops`, `set_domain_model`, `set_use_cases`, `set_sequence`, `set_change_impact`.
  - Analysis tools: `get_responsibilities`, `get_overlap`, `suggest_home`, `assess_class`.
- **Provenance.** Every committed element records its origin: decision, proposal, `user` or `import`.
- **Collaboration.** Other agents can join. A pi session started in the project folder picks up `.pi/mcp.json`. Any agent may also edit `design.json` directly; Spide turns such edits into ghost proposals instead of applying them silently.
- **Local-only MCP server.** A minimal streamable-HTTP JSON-RPC implementation (`initialize`, `tools/list`, `tools/call`) bound to `127.0.0.1`. Every route requires the bearer token, a loopback `Host` header and, when present, a local `Origin`, which blocks other local users' processes without the token, cross-site requests and DNS rebinding. Desktop and headless dispatch through the same `spide-service`, so policy, guards and the trace are identical.

## Development

```sh
cargo test --workspace                                      # core, import, lsp (mock server), service, headless (+ real nvim if installed), UI
cargo test -p spide-lsp --test real_tools -- --ignored --nocapture   # real language servers/linters on the seven-language fixtures
nvim --headless --clean -l integrations/nvim/tests/run.lua   # plugin unit tests in an isolated Neovim
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
- Source indexing and aggregate-write warnings are heuristic (macros, preprocessor, templates and dynamic dispatch are not resolved).
- The desktop's PNG/SVG/PDF exports need the window; headless exports are text (PlantUML, ADR, ArchUnit, JSON).
- Permissions bound what agents do *through Spide*; an agent with its own filesystem/shell tools isn't sandboxed by them (its edits are detected and show up as external changes).
- Branch merging is not a three-way merge.
- Lucid: positions of new shapes come from Spide's layout and may need tidying in Lucid; colours are set when a shape is
  created and not synced afterwards; the package and C4 dependency lines are derived (edits to them in Lucid are ignored).
