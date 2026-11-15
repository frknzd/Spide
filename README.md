# Spide

Spide is a desktop app for designing a system live, together with an AI agent. The class diagram is the centre; the other views are derived from it.

You start with an empty dark canvas beside a real interactive pi terminal. The agent first asks for the **brief** (goals, quality attributes, constraints, expected changes), then proposes classes and raises **design decisions**. Small changes are single-option decisions; larger improvements offer alternatives. You review them **on the diagram**: proposed changes appear in place, one click switches between alternatives, and **Approve design** applies the plan you see and moves on to the next one—notes are optional, and no defence or examiner verdict is required. Every option shows its consequences before you choose. The diagram keeps showing you how clean the design is — judged by the principles of the DOSS course (minimise coupling, increase cohesion, distribute responsibilities; RDD, GRASP, bad smells): hot classes glow, dependency cycles are drawn in red, methods in the wrong class point to their better home, and duplicated behaviour is measured. You can check the design against scenarios and expected changes, branch it and rewind it, look at it as C4 context and container diagrams, and keep it in sync with its Java code as a Spide Project.

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

Spide opens the project folder `~/Documents/Spide/untitled`. Set `SPIDE_PROJECT=/path/to/folder` to open another one, or use `/spide new` and `/spide open` inside pi (or the terminal header's **Commands** button). A design-only folder contains the files below; a [Spide Project](#spide-project-design-and-code-together) keeps its design next to its code instead.

| file | purpose |
|---|---|
| `design.json` | The design (the active branch). It is autosaved on every change and watched for edits made outside Spide. |
| `.pi/mcp.json` | Points pi to Spide's MCP server (`spide`, `http://127.0.0.1:47800/mcp`). |
| `.spide/terminal-bridge.ts` | App-owned pi extension for private structured prompts, context and `/spide` commands. The skill in `agent/spide-skill.md` is appended at launch. |
| `.spide/sessions/` | The agent's pi sessions. Spide continues the latest one on start. |
| `.spide/history.jsonl` | One snapshot per change of the model, for the timeline. |
| `.spide/branches/` | Inactive branches (`<name>.json`); `.spide/branch` names the active one. |
| `spide/project.json`, `spide/design.json`, `spide/sync.json` | Spide Project only (committed with the code): manifest, the design, the sync baseline. `.spide/` and `.pi/mcp.json` stay local. |
| `.spide/skills/` | Spide Project only: the bundled `spide-readjust` and `spide-implement` pi skills, refreshed at launch. |

Older `design.json` files (schema 1) open unchanged. Files of the removed source attachment and code import (`.spide/source.json`, `implementations.json`, `reality.json`) are ignored.

## Using it

### Native pi and workspace

- Pi runs once in a real PTY in the project folder, with **all available tools** (`--tools '*'`), configured extensions/MCP servers and the user's permission controls. Code edits and shell commands are allowed; diagram mutations still use Spide MCP proposals/decisions. Trust/permission dialogs remain interactive. On first opening a project you may need to approve pi's **Trust project folder?** prompt before Spide's integration becomes ready.
- The native editor is the only chat input. `Ctrl+P` cycles configured models, `Ctrl+Shift+P` cycles backward, `Ctrl+L` opens the model picker; Shift+Tab, Escape, multiline input and transcript scrolling belong to pi. Canvas shortcuts do not run while the terminal is focused. Selection/copy/paste uses the native terminal widget.
- Drag separators to resize panes; collapse/reopen without restarting pi. Widths/visibility are saved locally, not in the design. Narrow windows put the review rail in a drawer that doesn't cover pi. **Restart** is in the terminal header, even when pi is offline.
- **All Spide `/command` examples below mean `/spide command` in the terminal**, or the same command in **Commands**. Native pi `/new`, `/model`, `/settings`, `/resume`, `/export`, etc. retain their own meanings. For example, use `/spide detail 3`, `/spide tidy`, `/spide project`, `/spide open`, `/spide save`.
- Examiner/fix/generation requests go through an authenticated private Unix socket into that same session, never by pasting into a partly typed draft. Busy requests use pi follow-ups. Terminal bytes never drive app/design state.

### The loop

1. **Brief.** On an empty design the agent asks one question at a time and records your answers as forces (goals, quality attributes, constraints, expected changes) with a priority. ⌘B or `/brief` opens the brief panel, where you can add, edit, reprioritise or remove forces yourself. `/skip` (or **Skip brief** on the empty canvas) starts designing without a brief.
2. **Review on the diagram.** Pending changes (small single-option changes and substantive alternatives alike) open in the **Review** rail beside the canvas, and the canvas shows the selected plan in place: new and changed elements in teal, removals in red, everything else faded. The rail lists all pending plans by title — one click switches — and shows, without expanding anything, the question, the alternatives with their pros/cons and health, the consequences, blocking problems and a readable list of changes (click one to locate it). Clicking an alternative replaces the plan on the diagram; **Before / Proposed** and **Fit changes** sit at the bottom of the canvas, and positions stay put while you toggle. `compare` (or `/compare`, **Compare side by side**) still compares alternatives on a shared layout.
   - **Click elements on the diagram** to see exactly what the plan does to them (before → after) and to add them to a change request; they appear as chips above the notes. **Request changes** sends your notes, the selected elements, the decision/option and the design revision to the same pi session — nothing is applied, and the revised plan replaces the one under review. Requests need pi running (the rail says why when they can't be sent); failed requests keep your notes and selection.
   - **Approve design** applies exactly the saved, visible plan (notes are recorded with it) and then opens the next pending plan in queue order — it never approves that one. New proposals arriving meanwhile don't steal the selection. **Dismiss** rejects without applying. ⌘Z undoes either. When the queue is empty the rail shows **Review complete** with next steps: check behaviour in the Sequence view, **Implement in code** (in a Spide Project with approved changes the code doesn't have yet) or the decision history.
   - **Edit plan manually (advanced)** keeps the structured operation editor and JSON for when you'd rather edit than ask; **Save plan** saves amendments, not the design. Unsaved manual edits block approval and alternative switching until saved or discarded. There is no defence/examiner gate, even in legacy strict projects. **History** lists resolved decisions read-only, including legacy verdicts.
3. **Check behaviour.** In the Sequence tab each scenario gets a fit score: calls, call depth, package, layer and container crossings, aggregates touched, unknown calls. In the brief panel, **change scenarios** ("what if we must support e-books?") are analysed by the agent: Spide shows which classes must change, which depend on them (the ripple) and a locality score. **stress test** (or `/stress <name>`) highlights that footprint on the canvas.
4. **Evolve.** ⌘T or `/timeline` scrubs through the design's history (read-only preview); *restore here* proposes the changes that bring a past state back, *fork* starts a branch from it. `/branch <name>` branches from now and switches to it, `/switch`, `/compare <branch>` and `/merge <branch>` (the other branch's changes arrive as ghost proposals, additions and removals separately).
5. **Close the loop.** `/export adr` writes an ADR per approved decision, including optional notes and consequences (legacy defences/verdicts remain historical evidence). `/export archunit` writes Java architecture tests. A **Spide Project** keeps the design with its Java code and the agent keeps both in sync in either direction; see below.

### Partner modes and rules

- `/mode examiner | socratic | devils_advocate | sketch` (also in the brief panel). *Examiner* is the default. *Socratic* only asks questions and never changes the design. *Devil's advocate* explains counterarguments as advice, never an approval gate. *Sketch* proposes freely and keeps alternatives for big forks. Legacy strict/lenient settings do not restrict user approval.
- **Architecture rules** live in the brief panel (and `/rules <preset>`): a style preset — `clean` (dependencies point inward), `hexagonal` (adapters never depend on each other), `layered` (downward only), `modular_monolith` (`<module>.<layer>` packages, other modules only via `<module>.api`), `none` — plus your own forbidden dependencies (`*Controller -> *Repository : go via a service`), forbidden types per layer (`Http*` in domain) and thresholds. Editing `rules` in `design.json` applies directly.
- Every issue explains why it matters and how to fix it, names the course principle and reference it comes from (e.g. *GRASP Controller (Ch. 3)*), and says how sure Spide is (confidence, and whether it comes from structure, a heuristic, the agent or the code). In the health popover, click an issue to jump to it, or **fix with agent** to get a decision with ways to resolve it — for misplaced or duplicated behaviour the options are the ranked homes from `suggest_home` (the Information Expert, a new Pure Fabrication, or keeping it).

### Spide Project: design and code together

Pure-design projects need no code, Git or Java. A **Spide Project** is a module folder (for example `zap-extensions/addOns/restextraction`) where the design lives with its Java code:

```
addOns/restextraction/      ← project root; pi runs here (AGENTS.md of the repo and parents apply)
  spide/project.json        manifest: name, roots (src/main/java), exclude, code_root, compare filters, test command   (committed)
  spide/design.json         the design (active branch)                                                              (committed)
  spide/sync.json           sync baseline: both sides when they last agreed                                         (committed)
  .spide/  .pi/mcp.json     local (sessions, history, branches, skills) — kept out of git via .git/info/exclude
```

- **Create / open.** **Project** in the rail (or `/spide project`, File › Open Spide Project…). *Create Spide Project in a code folder…* picks the module folder; the form proposes roots, finds the Java root package (**Preview**), takes a test command for the agent (e.g. `./gradlew :addOns:restextraction:test` in `../..`) and copies the current design (with its history) into `spide/`. The old folder is untouched.
- **Index.** The Java sources under the roots are parsed (tree-sitter) on open and whenever a `.java` file changes. A partial or failed index is labelled; absences aren't trusted then.
- **Identity is exact.** A design class is the Java type `code_root.package.Name`, or the symbol of an explicit `link_code` op (for another name or package). Same simple names elsewhere never match. Methods match by name and parameter types (untyped design parameters match the only overload with that arity); fields by name. A code field the design draws as an association counts as present.
- **Compared:** classes (package, kind), fields (type, static), methods (parameter types, return type, static, abstract), extends/implements. Not compared by default: private methods, plain getters/setters, constructors, static code fields the design doesn't list, external (library) classes. Empty design types match any code type.
- **Sync status per element**, three-way against the baseline: **in sync** · **design ahead** (implement it) · **code ahead** (readjust the design) · **conflict** (both changed; **Keep design** / **Keep code**) · **unreconciled** (no baseline yet) · **accepted** (a difference you accepted). Elements that agree move the baseline along automatically; **Mark in sync…** (`/spide synced`) accepts the remaining differences. Only approved design is compared, never pending ghosts.
- **Readjust (code → design).** **Readjust design** / `/spide readjust [Class …]`: the agent (skill `spide-readjust`) reads `get_sync` and `get_code` and raises proposals and decisions — the usual review on the diagram. With an empty design it bootstraps the class model per package, then the domain model and use cases.
- **Implement (design → code).** **Implement in code** / `/spide implement [Class …]`: Spide sends the agent (skill `spide-implement`) a brief per class — target file, exact changes, responsibilities, the decisions behind them. The agent edits with pi's normal tools and permissions, follows the repository's instructions, never commits, stashes, resets or switches branches, asks before deleting files and runs the test command. When it settles, Spide re-indexes and shows *Implemented n/m* with what remains.
- **Verify.** `/spide verify` re-indexes and asks the agent to run the test command. Structural agreement doesn't prove behaviour.
- **On the canvas.** `/lens sync` rings classes (teal design ahead, violet code ahead, red conflict, amber dashed unreconciled), dots changed members and draws code-only classes as faded *only in code* ghosts. Hover cards show `code: org.x.Foo · src/…/Foo.java:12 · status`. Linked methods carry the code's facts, so long methods, real clones and the code-based course checks count in health.
- **Agent tools** (project only): `get_project`, `get_sync`, `get_code`; ops `link_code` / `unlink_code`.

### Diagnostics: what the course grades

Health is **one number** (0–100): a weighted geometric mean of five principle scores — **coupling** (.25), **cohesion** (.2), **responsibilities** (.3), **architecture rules** (.15) and **UML hygiene** (.1) — each computed from its issues (severity × confidence) relative to the size of the design, so one bad class in five hurts more than one in fifty. Hover the health ring for the breakdown. The checks:

- **Purpose vs behaviour.** Every class gets a role (presentation, controller, service, domain, persistence, adapter, output, creator, utility, value) from, in order: an explicit `role`, the agent's assessment, its name (`*Panel`, `*Controller`, `*Repository`, `*Writer`, …), its stereotype, its layer. Every method gets a concern (presentation, coordination, domain logic, persistence, formatting, creation, accessor) and an RDD kind (knowing, doing, deciding) from its verb, signature, `uses` and `calls` — or from its body when code is imported. Mismatches are flagged: a UI class doing domain calculations, an entity saving or formatting itself (the `Sale.save()` example), a fat controller, vague names (`*Manager`, `*Util`), name/stereotype/layer disagreeing, a class mixing several concerns.
- **Duplication and the right home.** Method pairs get an explainable similarity (name, signature, what the bodies touch, description, and with code a normalised fingerprint). Pass-through delegation (a controller forwarding to the domain), polymorphic overrides and UI event handlers are not duplication. Spide also flags behaviour that another class claims (its data, or its stated responsibility), concepts scattered over ≥ 3 classes (shotgun surgery) and middle men. For each, `suggest_home` ranks where the behaviour belongs (Information Expert; wholes take over their parts), checks every caller may depend on it, simulates the health/coupling/cohesion effect, and offers a new Pure Fabrication class when no expert fits.
- **Course checks.** Law of Demeter (talking to strangers), Feature Envy, data classes / no behaviour, the UI driving the domain without a controller, Creator, switches on a type code in several places (replace with Polymorphism), long methods, long parameter lists, and the exam notes on UML: argument types, no repeated inherited attributes, a Composite needs add/remove/getChildren, a Visitor with one element is a Strategy.
- **The agent's judgement.** The agent records its own assessment per class (`assess_class`: purpose, role, a verdict per method). Agreement raises an issue's confidence; when it disagrees on a serious issue you see the disagreement and have to resolve it. Assessments go stale when the class changes.
- **Long functions.** Java methods/constructors over **30 nonblank, non-comment body lines** get a `long_method` warning and lower cohesion/overall health. Configure **long method: code lines >** in Brief → Thresholds (`rules.thresholds.max_method_lines`). Brace-only lines and nested type bodies are excluded. This is a heuristic: hovers show measured length, threshold, source location and an Extract Method suggestion. In a Spide Project, methods linked to the code carry its measured facts (also call chains, switches, loops, creations, UI calls, clone fingerprints) into design health; pure-design methods have unknown length, not an invented penalty.

### On the canvas

- **Commands.** Type `/spide ` in pi for completion, or open **Commands** in its header.
- **Ghost changes.** Click a ghost or **Review N changes on the diagram** to start reviewing. During review, clicking an element of an earlier pending plan switches to that plan; clicking any other element inspects it and toggles it as a change-request target. Review actions stay outside the scroll area. There are no bulk actions; legacy `/accept <plan-id>` and `/reject <plan-id>` require an explicit ID. Direct canvas edits (rename, drag into packages, prune) are disabled during review; pan, zoom and focus still work. The brief stays available.
- **Safe plan drafts.** Draft previews never commit accepted elements. Unsaved plan drafts are kept locally per workspace/branch/proposal; operation forms must be added to the draft first. Workspace changes require explicit revalidation before saving. Dependent plans must have prerequisites accepted first; invalid plans show errors rather than disappearing. Undo/redo restores plan saves and individual resolutions. Amendments update the associated option but never approve it. Legacy proposal IDs remain stable internal plan IDs.
- **Direct edits.** Double-click a class, attribute or method name to rename it. Drag a class onto another package to move it. `/add <Class> [in <package>]` adds a class. ⌥-click pins a class where it is (📌); pinned classes stay put, also on `/tidy`. Direct edits are yours: they apply immediately and ⌘Z undoes them.
- **Prune.** Right-click an element to remove it, or hold the right mouse button and drag a box around several. A small box asks what you didn't like; ⏎ sends it to the agent, Esc prunes without comment. Pruned elements stay in the model, hidden by default (`/pruned` shows them), and the agent is told not to bring them back. Click a pruned element to restore it (restoring a package restores its classes).
- **Detail.** The slider bottom-left (or `/detail 0–4`) sets how much each class shows: packages · names · responsibilities · **design** (default: public behaviour with argument types; accessors, constructors and private helpers folded into one line; attributes that repeat a drawn association become the edge's role name; inherited attributes not repeated) · full. Zooming far out shows less on its own (names, then packages). Exports follow the slider. Methods the diagnostics flag always stay visible, with `↗ Home` next to them.
- **External.** `/external <package|Class>` tags library/framework code (Spide suggests it once for packages like `zap` or `swing`): drawn as a compact strip, coupling to it counts less, and it isn't judged.
- **Lenses.** `/lens concerns` colours method rows by what they do; `/lens overlap` draws dashed links between classes that duplicate each other and marks scattered concepts (⁂); `/lens off`. `/implicit` shows all implicit dependencies (otherwise only the hovered class's).
- **Hover** over an element for its details: its **purpose** (the agent's sentence, the role Spide reads and why, methods in the wrong place → suggested home, what the code says), members, responsibilities, rationale, metrics, and **why it's here** — the decision (question, chosen option, optional notes and historical evidence) that introduced it. Member hovers use more of the available canvas height and keep long evidence scrollable. Click an element that came from a decision to see the whole decision record.
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
| red / amber ring | stress test: impacted / ripple |
| teal / violet / red / amber ring, member dots (sync lens) | Spide Project: design ahead / code ahead / conflict / unreconciled (dashed); faded *only in code* boxes exist only in the code |
| health ring (canvas toolbar) | 0–100. Hover over it for the five principle scores and the issues per principle. `→ n` shows the health after accepting pending ghosts. |

The layout favours presentation quality over keeping old positions. Every change to classes, relations or box sizes (including a different detail level) lays the whole diagram out again; review keeps positions only while you toggle alternatives. Packages are compact layered drawings: tight layers, boxes centred under their neighbours, and long chains wrapped beside each other, but never splitting a parent from its subclasses. Packages sit in bands (ui and infrastructure on top, application, then domain). Each band flows into rows, and the row width and package shapes are chosen for the most compact, near-rectangular result. External (library) packages and classes go into a band along the right or bottom edge, next to their clients.

Class footprints include their status chips and metric badges, so nothing overlaps. Package frames are wide enough for their titles. Long member signatures and responsibilities wrap rather than truncate. Edge labels go where they clear every box, chip and other label, and are drawn above the lines and boxes. Edges are orthogonal, with short stubs so they turn early, 12px lanes for parallel runs, and shared triangles for generalisations.

The layout lands exactly on its computed positions and stops; physics only moves classes you drag. An idle canvas costs no CPU. Decorative animations (ghosts, hot halos, cycles) play briefly and rest.

Package names such as `domain`, `ui` or `persistence` are mapped to their layer automatically.

### Views

- **System** (C4). *Context*: actors, the system and external systems. *Containers*: deployable units with their technology and replicas, the packages that run in them, datastores, links to external systems. Container-to-container dependencies are derived from the class dependencies. Spide flags single points of failure, synchronous call chains across containers, and views that disagree (a package that runs in no container, an unconnected external system, an actor without use cases).
- **Class.** The main canvas.
- **Domain.** Built by the agent (`set_domain_model`): a conceptual model of the problem domain — concepts with attributes (no methods, no software or framework classes), associations with verb-phrase labels and multiplicities, generalisations. It is separate from the class design (no references to it) and marked out of date when the class model changes; **↻ generate / regenerate** asks the agent. Prune concepts with feedback like any element.
- **Package.** Computed: a plain UML package diagram — folders with the package name (and «layer»), their classes, and dashed dependency arrows. Package metrics (I / A / D) stay with the agent's `get_metrics`.
- **Use case.** Generated by the agent from the class model. The tab is marked stale when the class model changes.
- **Sequence.** A UML sequence diagram generated by the agent for a scenario you pick from the use cases or name in the prompt. Lifelines are objects (`l : Loan`, `: Catalog`, multi-objects `loans : Loan[*]`) or actors. Combined fragments `loop`, `alt` (with `else`), `opt`, `par`, `break` and `critical` frame their messages with `[guards]`; `ref` frames another interaction (click it to open that sequence). A created object's header appears at its «create» message, a «destroy» message (or an object ending itself) ends the lifeline with ✗. Activation bars follow the calls, and replies are drawn automatically (dashed, with the returned value) when an activation ends; explicit replies are never doubled. Calls are numbered. Every call is checked against the receiver's class; calls to methods that don't exist are red. The side panel shows the scenario's fit. The PlantUML export draws the same diagram.

## Exports

- **PNG, SVG and PDF** use the same retained scene and layout as the native canvas, including classes you dragged. Exports wait for the layout to settle. Hiding diagnostics hides their marks but keeps box sizes, so an export never rearranges the canvas. Export settings include theme, scale, legend, diagnostics and ghost visibility, and persist per project. PNG uses `resvg` (default 2×) and records its resolution (72 dpi × scale, so 2× is 144 dpi): Preview, Pages, Word and other viewers show it at its 1× size with every pixel sharp on Retina, instead of enlarging a "72-dpi" image and blurring it. PDF uses `svg2pdf`. The export dialog's preview renders at the screen's pixel density. Animations are frozen. **File → Export Again** repeats the previous format/path. Detail 0 (packages) exports the package overview.
- **Obsidian Canvas** (`.canvas`, [JSON Canvas 1.0](https://jsoncanvas.org)) for every diagram, in the same layout as the PNG (scaled up so the cards fit Obsidian's 16 px text). Classes, concepts, packages (package diagram and detail 0), C4 elements, actors and use cases become Markdown cards with the text the diagram shows at its detail level: stereotype, **name**, compartments separated by `---`, members as `code`. Package frames become groups; the use case system boundary is a group too. Classes and packages keep their layer colours, C4 elements their kind colours. Edges run between the sides the routes use. A canvas edge is a plain arrow, so UML notation is spelled out in its label: `extends`, `implements`, `is a` (domain), `◆ composed of`, `◇ aggregates`, `depends on`, `«include»`, `«extend»`, `«async»`, the relation name and `1 → 0..*` multiplicities; directed relations get an arrowhead. Sequence diagrams: each lifeline is a group with a header card, each message an edge between small numbered anchor cards on the two lifelines (replies `↩`, a «create» ends on the new object's header, self-calls loop), fragments are groups labelled e.g. `alt [ok] / [else]`. Options: title, date, package frames and ghost proposals; diagnostics, legend and health are never included.
- **All diagrams (batch).** In Export (⌘E), switch to **All diagrams**. Out-of-date agent-built diagrams (domain model, use cases, sequences) are regenerated first: the batch asks the agent and waits, then writes; if the agent can't (pi offline, or it didn't update them) you choose **Export anyway** or **Stop**. Choose:
  - the diagrams: by default every non-empty one, including both C4 levels and every sequence scenario;
  - the detail levels: these apply to Class and Domain, while other views export once;
  - PNG, SVG, PDF and/or Obsidian Canvas;
  - settings profiles: by default the current settings; or add several, e.g. light 2× and dark with diagnostics.

  You get one file for each diagram × applicable detail × profile × format. The canvas's own detail level reuses the live canvas. Other levels use an off-screen canvas with the same layout pipeline, so the tab, slider and canvas stay untouched. Files go into a new `<design>-diagrams-<date>-<time>` folder inside the folder you pick. Names are like `class-3-design.png` or `sequence-borrow-a-book-dark.pdf`, with suffixes for duplicate names. Files are written atomically, one per frame, with progress and **Stop**. Finished files are kept. Empty diagrams are skipped and reported. A design change during the batch stops it, so everything comes from one revision. Optionally, it writes one PlantUML file per view; these use PlantUML's own layout and the full model, without detail levels or themes.
- **PlantUML** is exported per tab, or for every non-empty diagram at once with `/export all`. The System tab exports C4-PlantUML (context and container diagrams). The export keeps every member with its visibility and type, plus stereotypes, multiplicities, layer-coloured packages, cycles and layer violations in colour, a health legend, and `down`/`right` direction hints taken from the canvas. PlantUML uses its own layout engine, so the PNG is the pixel-exact export.
- **JSON** via `/save` and `/open`. The project's `design.json` is always up to date.
- **ADRs** via `/export adr` (Markdown, MADR style, one per accepted or overruled decision).
- **ArchUnit** via `/export archunit`: a JUnit 5 `ArchitectureTest.java` with the layering of the preset, custom forbidden dependencies and types, module boundaries and a no-package-cycles rule. It needs `com.tngtech.archunit:archunit-junit5` as a test dependency.

## Architecture

```
crates/spide-core     pure Rust: model, ops, store (proposals, decisions, undo), metrics (health per principle),
                      responsibility (roles, concerns), overlap (duplication, delegation), home (suggested homes),
                      course (Demeter, Creator, …), assess (agent judgement), rules, impact, scenarios,
                      project (manifest), sync (design ↔ code shapes, three-way status, briefs), derived views (incl. C4), diff, agent tool API,
                      exporters (PlantUML, C4, PNG with dpi, ADR, ArchUnit)
crates/spide-import   Java source index (tree-sitter) into a read-only design with body facts, for Spide Project sync
crates/spide-diagram  GUI-independent scenes, orthogonal routing, compound Sugiyama layout, physics,
                      diagnostics/focus, all diagram views, themes, shared-scene SVG and Obsidian Canvas export
app                   eframe/egui desktop app: canvas, native panels, alacritty_terminal widget,
                      commands, MCP server (axum), real pi PTY + private control socket,
                      design.json watcher, history, branches, structured prompts for the agent
agent/               appended skill, Spide Project skills (spide-readjust, spide-implement) and bundled native pi integration extension
```

- The Rust store is the single source of truth. The UI only sends intents (prompt, prune, approve, request revision, undo) and renders the snapshots it receives.
- Agents change the design **only** through MCP tools: `get_design`, `get_metrics`, `get_view`, `propose_decision`, `propose_ops`, `set_domain_model`, `set_use_cases`, `set_sequence`, `set_change_impact`. Analysis tools: `get_responsibilities`, `get_overlap`, `suggest_home` and `assess_class` (record the agent's judgement); in a Spide Project also `get_project`, `get_sync` and `get_code`. Every change becomes a user-approved decision (`propose_ops` is the single-option compatibility path), and invalid ops are rejected atomically. Deprecated `evaluate_defense` cannot mutate anything. Forces, change scenarios, containers, external systems and links are ops too, so they go through the same proposals, undo and file watcher. Architecture rules are the user's: only the user changes them.
- Every committed element records its origin (decision, proposal, `user` or `import`); accepted decisions keep the consequences they had.
- Other agents can join the session. A pi you start in a terminal in the project folder picks up `.pi/mcp.json`, and any agent can also edit `design.json` directly. Spide turns such edits into ghost proposals instead of applying them silently.
- The MCP server is a small streamable-HTTP JSON-RPC implementation (initialize, tools/list, tools/call) on `127.0.0.1` only. Every route requires a loopback `Host` header and, when a browser sends one, a local `Origin`, which blocks cross-site requests and DNS rebinding. It replaces the `rmcp` crate that was originally planned, to avoid tracking that crate's fast-changing API.
- Environment variables: `SPIDE_PROJECT` (project folder), `SPIDE_PI` (path to the pi binary), native pi configuration and `/model` select the model. Legacy `SPIDE_PI_ARGS` is no longer used (it could override interactive transport/tool flags).

## Development

The native migration is documented in [`plans/native-rust-gui.md`](plans/native-rust-gui.md); test results and remaining manual checks are in [`plans/native-rust-verification.md`](plans/native-rust-verification.md). Older verification notes in `plans/interactive-design-verification.md` describe the pre-egui app. Java indexing and aggregate-write warnings remain heuristic, and branch merging is not a three-way merge.

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
