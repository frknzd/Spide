# Agent-built domain model, plain UML package diagram, remove code features, design Spide Project

## Context

1. **The Domain tab is wrong.** `derive::domain_model` (`crates/spide-core/src/derive.rs`) builds the domain model
   from the *class design*. It guesses which classes are conceptual from their stereotypes and names (`is_conceptual`).
   For the REST extraction design it therefore keeps `ExtensionHook`, `HttpMessage`, `AbstractPanel`,
   `CallTableModel`, `RestTrafficListener`, `ExtensionRestExtraction`… These are software and framework classes,
   not concepts of the problem domain. A domain model needs judgement, so the **agent builds it**, the same way it
   builds the use case diagram.
2. **The Package tab isn't a UML package diagram.** `views::package_scene` (`crates/spide-diagram/src/views.rs:60`)
   draws I/A/D metric chips under every package, a class-dependency count on every edge, and red/amber colours for
   cycles and layer violations. The PlantUML exporter (`plantuml::package_diagram`) adds the
   `note bottom … I= A= D=` notes and `: count` labels.
3. **The code features go:** the Source attachment (repo path, roots, bindings), **Implement in source**, the
   legacy `/import`, `/drift` and `/code`. They are replaced by **Spide Project** (Part 4, implemented in this
   plan): the design lives with the code in a module folder, and the agent keeps the two in sync in both directions
   (readjust and implement).

Decisions taken with the user:
- The domain model works like the use case view. The agent replaces it with one tool call, with no approval step;
  the user can prune parts of it with feedback.
- It is a separate conceptual model. It reflects the class design but has **no references** to design classes.
- It shows "out of date" when the class model changes after it was generated. Batch export regenerates
  out-of-date agent-built views before it writes anything.
- Spide only draws it. There are no checks or metrics on it.
- The package diagram lists each package's classes and draws dependency arrows. It is plain: no numbers and no
  cycle/violation colours. Layer colours per package stay.
- Everything from the Source attachment, Implement in source, `/import`, drift and `/code` is removed.

---

## Part 1 — Agent-built domain model

### Model (`crates/spide-core/src/model.rs`)
```rust
pub struct Concept { name, description: Option<String>, attributes: Vec<ConceptAttribute>, status: Status, feedback: Option<String> }
pub struct ConceptAttribute { name, r#type: Option<String>, multiplicity: Option<String> }   // no visibility, no methods
pub enum ConceptLinkKind { Association, Generalization, Composition, Aggregation }        // snake_case serde
pub struct ConceptLink { id, from, to, kind, label: Option<String>, from_mult, to_mult, status, feedback }
pub struct DomainModel { concepts: Vec<Concept>, associations: Vec<ConceptLink>, generated_rev: Option<u64> }
```
- New field `Design.domain: DomainModel`. It uses `#[serde(default)]`, so old `design.json` files open with an
  empty domain model. This is not a schema bump.
- Association ids are assigned on set: `from-kind-to`, with a numeric suffix when the same pair appears twice.

### Store and tool (`store.rs`, `tools.rs`)
- `Store::set_domain_model(m)` works like `set_use_cases` (`store.rs:424`):
  - it sets `generated_rev = model_rev`;
  - it keeps pruned concepts and associations, with their feedback, when the new model leaves them out;
  - it warns about and drops associations whose ends are unknown, and drops duplicate concept names;
  - it logs as `agent`, and undo works.
- `Target::Concept { name }` and `Target::ConceptLink { id }` are added to `set_status` and `label`. Pruning a
  concept also prunes its associations; restoring a concept brings them back.
- MCP tool **`set_domain_model`**, with a JSON schema for concepts and associations. Its description explains what
  a domain model is (the next section).
- `get_view kind=domain` returns the stored `d.domain`.
- `get_design` includes the domain model and a `domain_stale` flag.

### Derive (`derive.rs`)
- Delete `is_conceptual`, `DomainClass`, the derived `DomainModel` and `domain_model()`.
- `Derived.domain` is replaced by `domain_stale: bool`. It follows the same rule as `use_cases_stale`: classes
  exist and (the domain model is empty or `generated_rev < model_rev`).

### Prompt and skill
- `prompts::gen_domain_model()`: "[spide · generate domain model] Read get_design. Build a conceptual domain
  model (Larman): concepts of the problem domain the class design deals with; attributes (simple types, no
  visibility, no methods); associations with verb-phrase labels and multiplicities; generalisation only for real
  is-a. Leave out software artefacts: UI, controllers, services, repositories, listeners, framework/library
  classes, and patterns. Names may differ from the class design. Keep pruned items out. Call set_domain_model
  once. Reply with one sentence."
- `commands::regenerate` accepts `"domain"`.
- `agent/spide-skill.md`: line 4 says domain, use case and sequence views are generated by the agent; only the
  package view is derived. Replace "The domain model keeps only conceptual classes" (line 101) with the
  guidance above.

### UI (`app/src/ui/app.rs`, `ui/views.rs`)
- `App::domain_data` (app.rs:533) builds `ClassData` from `snap.view.domain`:
  - concepts become `Class` values with only `name` and attributes (type and multiplicity);
  - links become `Relation` values, where generalization maps to inheritance.
  - It no longer falls back to the class layout: it uses only `layout["domain"]`.
- Target translation on the Domain tab: the canvas emits `Class`, `Attribute` and `Relation` targets. Before
  prune, restore or click, `App` maps them to `Concept` and `ConceptLink` targets. Attribute prune is ignored.
  This prevents the bug where pruning on the Domain tab would prune a design class.
- Hover on the Domain tab shows a small card: the concept description and attributes, or the association label
  and multiplicities. It does not show the class-diagnostics card. Canvas diagnostics are off for the Domain tab,
  as they are today (`metrics: None`).
- A banner works like the use case view (`ui/views.rs:164`): "No domain model yet. ↻ generate" or "Out of date
  with the class model. ↻ regenerate". The domain model is generated automatically once, when the tab opens empty
  and pi is idle (`asked_domain` flag).
- Export views: `export_views()` includes Domain when there are live concepts. The legend for the Domain tab
  contains no diagnostics.

### PlantUML
- `plantuml::domain_model` renders the stored model: `class X { name : Type }`, `hide methods`, and the
  associations with multiplicities and labels.
- Update the golden file `library-domain.puml`.

### Batch export regenerates out-of-date views (`ui/export.rs`, `ui/app.rs`, `export_jobs.rs`)
- On **Start**, after the folder is chosen, compute the stale *selected* agent-built views:
  - Domain if `domain_stale`;
  - Use cases if `use_cases_stale`;
  - each selected Sequence whose name is in `stale_sequences`.
  - Missing models whose tab would otherwise be skipped also count: an empty domain or empty use cases while
    classes exist.
- If there are none, the batch starts as today.
- Otherwise `BatchRun` gets a `phase: Regenerating { pending: Vec<ExportView>, started_at }`:
  - Spide sends one combined regenerate prompt through `pi.prompt`. The prompt lists the views and asks for
    `set_domain_model`, `set_use_cases` and `set_sequence` calls, in that order.
  - The footer shows "Regenerating domain model, use cases… with the agent" and **Stop**.
  - Each frame checks the snapshot. When nothing is stale any more and pi is idle, the batch sets
    `run.rev = snap.rev`, recomputes `export_views()`/`jobs`, and starts writing. A design change during writing
    still stops the batch.
  - If pi is idle and something is still stale, the footer shows "The agent didn't update: …". The user can choose
    **Export anyway** (writes the out-of-date diagrams) or **Stop**.
  - If pi is offline, the same choice is shown immediately ("pi isn't running").
- Pure helper `export_jobs::stale_views(selected, derived, design) -> Vec<ExportView>`, with unit tests.

### Examples and tests
- `examples/library.json`: add a hand-written `domain` matching today's output (Catalog, Book, BookCopy, Member,
  Loan with their associations). This keeps the domain snapshot and the golden file meaningful.
- `examples/rest-extraction.json`: leave the domain model empty, so the agent builds it.
- Tests in `crates/spide-core/tests/store.rs` (or a new `domain.rs`):
  - set, then the model is not stale; changing a class makes it stale;
  - prune keeps the item across a later set;
  - unknown ends produce warnings;
  - old JSON without `domain` loads;
  - `tools::call("set_domain_model")` works.
- UI tests: the Domain tab with an empty model shows the banner; prune on the Domain tab doesn't touch design
  classes; the batch export regenerating phase (fake pi idle and stale → choice shown).
- Update the macOS snapshot `library-domain.png` only if the rendering changes (`UPDATE_SNAPSHOTS=1`, after a
  visual check).

---

## Part 2 — Plain UML package diagram

### Scene (`crates/spide-diagram/src/views.rs::package_scene`)
- **Remove:**
  - the I/A/D chips (lines 171–177);
  - the edge count label (`label: Some(dep.count…)` becomes `None`);
  - the cycle glow and the `in_cycle`/`layer_violation` colours and warn tags;
  - the extra 40px below each box in the routing boxes (it existed only for the chips).
- **UML notation:**
  - The package name goes in the folder tab: bold name, with `«layer»` above it when the layer name differs from
    the package name. The tab is sized to its text, and the body starts directly under it.
  - Contained classes are plain names in the body (no `▫`), and abstract classes and interfaces are shown in
    italics.
  - The border and tab tint use the layer colour, as today. Proposed packages keep the dashed ghost border.
  - Dependencies are dashed lines with an open arrowhead, from client to supplier, with no label. Check that
    `EdgeKind::Rel(Dependency)` with `directed` already draws a dashed line with an open arrow; otherwise use the
    dependency style from `class_view`.
- The `metrics` argument is still used for `package_deps` (the edges), but not for package metrics.
- The canvas legend for the Package tab (`legend_for`): remove the cycle/violation entries.
- Hover on a package: remove I/A/D from the hover card if it is shown there. Check `hover_card.rs` for
  `package:`.

### PlantUML (`plantuml::package_diagram`)
- Drop the `note bottom … I= A= D=` notes, the `: count` labels and the red/amber edge styles. The output is
  `a ..> b`. Keep the stereotype and layer line colour.
- Update the golden file `library-package.puml`.

### Unchanged
- The package metrics stay in `get_metrics` for the agent, and in the health diagnostics on the class canvas.

---

## Part 3 — Remove the Source attachment, Implement in source, /import, drift and /code

### Delete
- `app/src/source_tracking.rs`, `app/src/source_implementation.rs`, `app/src/source_implementation_tests.rs`,
  `app/src/ui/source.rs`
- `crates/spide-core/src/source.rs`, `drift.rs` and `code.rs` are deleted in Part 4, once their reusable parts have
  moved into `sync.rs` (see "Kept and reused" below). Until then, Part 3 only removes their callers.
- `crates/spide-import/tests/source_scope.rs` stays, because Part 4 uses `scoped_files`.

### App wiring
- `main.rs`: remove the `mod source_tracking` and `mod source_implementation` declarations, `watch` and
  `refresh_async`.
- `state.rs`:
  - remove the `source` and `implementations` fields from `AppState` and `Snapshot`, and their loading;
  - remove the `invalidate` calls;
  - remove the `with_bound_length_evidence` hook (state.rs:203–228).
- `commands.rs`:
  - remove the `[spide source context]` prompt suffix (lines 27 and 79) and the source/implementation loading
    in `switch_project`;
  - remove `reality_path`, `load_reality`, `import_code`, `DriftReport`, `drift`, `code_snippet` and
    `drift_adopt` (618–725).
- `pi_control.rs:122`: remove the source context suffix.
- `mcp.rs`:
  - remove `get_source_context`, `begin_implementation` and `get_code`, plus the `on_code`/`judged_code` branch;
  - stop adding `source_context` to `get_design`.
- `ui/app.rs`:
  - remove the `source_ui` and `source_open` fields, the **Source** mode button (1839) and its panel (1859);
  - remove `drift`, `drift_rev`, `drift_sets()`, the phantom handling in `class_data`/`do_prune`, the drift panel
    (2213–2240), the drift refresh (1628) and `/code`, `/drift`, `/import`;
  - simplify the `class_data` cache key.
- `ui/timeline.rs`: delete `DriftUi`, `DriftOut` and `drift_panel`, and update the module doc.
- `ui/overlays.rs`: delete the `/code` viewer (169…).
- `ui/hover_card.rs`:
  - remove the `drift` field in `CardCtx`, the "code says" disagreements and the `/code` hint (62, 110);
  - remove the source evidence warning (335).
- `ui/proposal.rs:389`: the **Implement in source →** step becomes **Implement in code →** (Part 4). It is shown
  only in project mode when there are design-ahead items.
- `ui/canvas.rs` and `spide-diagram/src/class_view.rs`: rename `DriftSets` to `SyncSets` and keep the phantom
  ("only in code") rendering for the Part 4 sync lens. Remove the drift-specific wiring in the app.
  - Keep `ring_dashed` for proposals.
- `ui/state/commands.rs`: remove `import`, `drift` and `code`. Keep `coderoot`: the ArchUnit export uses it.
- `prompts.rs`: remove `assess_code`.
- `ui/tests.rs`: remove the source attachment fixture test (around line 68).
- `agent/spide-terminal.ts`: remove `import`, `drift` and `code` from the command list and from the description
  example (`/spide import` becomes `/spide tidy`).
  - Update `tests/pi-extension.test.ts` if it asserts the list.

### Core tool surface (`crates/spide-core/src/tools.rs`)
- Remove `get_code` from `tool_list` and the `call` stub.
- Remove the `on` parameter of `get_responsibilities`/`get_overlap`, and the `basis=code` wording in
  `assess_class`.
  - The `basis` enum keeps `code` so that old stored assessments still deserialize.

### Kept and reused by Spide Project (Part 4)
- The `spide-import` crate: the tree-sitter Java parser, `import_java_scoped`, `scoped_files` and its test, body
  facts, and `SourceSymbol` on types and methods.
- `Method.code: CodeFacts` and the engines that read it (course, overlap, responsibility, metrics).
- `Class.source` and `Method.source` (`SourceSymbol`): they become the explicit trace link.
- The phantom ("only in code") rendering in `class_view`. It is renamed from `DriftSets` to `SyncSets` and reused
  by the sync lens instead of being deleted.
- The useful parts of `drift.rs` and `source.rs` move into the new `sync.rs`:
  - `signature`;
  - the member comparison;
  - the code → design op generation from `adopt_ops`/`reconcile`;
  - `with_bound_length_evidence`, which becomes `with_code_facts`.
  After that, `drift.rs`, `code.rs` and `source.rs` are deleted.

### Docs
- `README.md`:
  - remove the "Source attachment and implementation" section, the `/import` and `/drift` text, "The real code"
    in Diagnostics, `/code`, and the `.spide/reality.json`, `source.json` and `implementations.json` rows;
  - update the Views section: Domain is generated by the agent, and Package is a plain package diagram;
  - add a **Spide Project** section that follows Part 4, and add the project files to the file table.
- `agent/spide-skill.md`: remove the bullet at line 63 and lines 83–84. Add a short "Spide Project" section that
  points to the two skills.
- Mark `plans/unified-decisions-source-sync.md` as superseded at the top.

---

## Part 4 — Spide Project: the design and its code in one folder

**Goal.** A design and its code live together in a module of a repository. Spide works out, element by element,
whether design and code agree. Where they differ, it shows which side changed, and the agent closes the gap in
the direction the user picks:
- **Readjust** (code → design): the agent proposes design changes. The user approves them on the diagram, as
  with any decision.
- **Implement** (design → code): the agent edits the code from the approved design, with pi's normal tools and
  permissions.

Design-only projects (a folder with a `design.json`) keep working unchanged.

### 4.1 Decisions (made for this plan — annotate to change)
| # | decision | why |
|---|---|---|
| D1 | A project is **one module folder**. For example `zap-extensions/addOns/restextraction`, not the whole repo. pi runs with that folder as cwd. | pi loads `AGENTS.md` from the cwd and its parents, so the repo and workspace instructions still apply. The scope stays small. |
| D2 | Shared files go in `<root>/spide/`: `project.json`, `design.json`, `sync.json`. They are committed. Local files go in `<root>/.spide/` and `<root>/.pi/mcp.json`, and are kept out of git through `.git/info/exclude`. | The design is reviewed and versioned with the code, and nothing local leaks into commits. `.git/info/exclude` is never committed. |
| D3 | Identity is an **exact fully qualified name** (`code_root` + package + name), or an explicit link stored in `design.json`. Spide never matches classes by their simple names alone. | This keeps the earlier rule that same-named classes are not automatically the same class, without per-machine bindings. |
| D4 | Sync is a **three-way comparison** against a committed baseline. In-sync elements advance the baseline automatically; remaining differences need an explicit **Mark in sync**. | This lets Spide tell which side changed. A design can stay deliberately more abstract than the code. |
| D5 | Spide itself never edits code, runs build commands, commits, stashes, resets or switches branches. The agent edits code only after the user clicks Implement, within pi's permissions. There is no separate authorization or task store. | The flow is simpler than the old implementation tasks, and pi already guards file edits and shell commands. |
| D6 | Only **accepted** design elements are compared; proposals (ghosts) are not. | Only approved design is implemented. |
| D7 | The language is Java only. Verify asks the agent to run the manifest's test command. | This reuses the existing parser. Spide needs no command runner. |

### 4.2 Files on disk
```
addOns/restextraction/            ← project root, pi cwd
  spide/project.json              manifest                       (committed)
  spide/design.json               the design (active branch)     (committed)
  spide/sync.json                 sync baseline                  (committed)
  .spide/                         sessions/, history.jsonl, branches/, branch, terminal-bridge.ts, skills/   (local)
  .pi/mcp.json                    Spide MCP server entry                                                    (local)
```
- **Project detection** (`app/src/project.rs::detect(dir)`):
  - a folder with `spide/project.json` opens in **project mode**;
  - otherwise it opens as today (`design.json` in the folder);
  - opening `…/spide/` or `…/spide/project.json` directly resolves to its parent.
- `Shared::design_path()` returns `spide/design.json` in project mode. The file watcher, `state::save` and
  `branches` use `design_path()` instead of `project_dir.join("design.json")`.
- **Git exclusion.** On init, Spide finds the git directory: it walks up to `.git` and follows `gitdir:` for
  worktrees. If `.spide/` and `.pi/mcp.json` are not already ignored, it appends them, relative to the work tree,
  to `info/exclude`. This is idempotent, controlled by a checkbox in the form (on by default), and silently
  skipped outside git.

### 4.3 Manifest `spide/project.json` (`crates/spide-core/src/project.rs`)
```json
{
  "version": 1,
  "name": "REST extraction",
  "language": "java",
  "roots": ["src/main/java"],
  "exclude": [],
  "code_root": "org.zaproxy.addon.restextraction",
  "compare": { "private_methods": false, "accessors": false, "constructors": false },
  "commands": { "test": { "run": "./gradlew :addOns:restextraction:test", "cwd": "../.." } }
}
```
- `Manifest::load/validate`:
  - `roots` and `exclude` must be relative, have no `..`, and resolve inside the root (reusing the
    `scoped_files` checks);
  - `commands.*.cwd` is relative and must stay inside the git work tree, or inside the root when there is no git;
  - `code_root` must be a Java package name;
  - unknown fields are kept when Spide rewrites the file (`#[serde(flatten)] extra`).
- In project mode, `code_root` comes from the manifest. The ArchUnit export uses it, and `/coderoot` edits the
  manifest. `settings.code_root` is used only in design-only mode.

### 4.4 Code index (`app/src/project.rs`)
- `ProjectState { root, manifest, index: CodeIndex, baseline: Option<Baseline>, sync: SyncReport, implementing: Option<ImplementRun>, warnings }`
  lives in `Shared.project: Option<ProjectState>`.
- `CodeIndex { generation, status: Indexing | Current | Partial(skipped) | Error(msg), design: Design /* from spide-import */, files: usize, parsed_at }`.
- **Refresh:**
  - on open, on **Re-index**, and from a recursive `notify` watcher on every root;
  - the watcher only reacts to `.java` files, with a 400 ms debounce;
  - the import runs on a background thread with `spide_import::import_java_scoped(root, roots, exclude, code_root)`;
  - a result whose generation is older than the latest is dropped;
  - the whole module is parsed each time, with no per-file cache. The fixtures show this is fast at module
    size; a cache can come later if needed.
- After each index or design change, recompute `sync` and emit a snapshot.

### 4.5 Identity and trace links
- **Class ↔ type:**
  - if `class.source.symbol` is set, that is the link;
  - otherwise the implicit FQN is `code_root` `.` package `.` name, with empty parts dropped;
  - an exact match only. Design classes with `external: true` are not compared.
- **Method ↔ method:**
  - an explicit `method.source.symbol` (`Type#name(int,String)`) is the link;
  - otherwise the method key is `name(T1,T2)`, using simple parameter types with generics stripped;
  - when the design method has untyped parameters, it matches the single code overload with the same name and
    arity, if exactly one exists.
- **Field ↔ field:** matched by name. A code field `x : T` or `x : Collection<T>` counts as present in the design
  when the design has an association from the class to `T`, because the design draws it as an edge.
- **Ops** (`ops.rs`, `OPS_DOC`):
  - `{"op":"link_code","class":"Foo","method?":"bar","symbol":"org.x.Foo[#bar(int)]"}` and `unlink_code`;
  - only `symbol` is stored. File and line are filled into the *view* from the index and are never written to
    `design.json`.
  - Links go through proposals like every other agent change.
- **Rename hint:** a design-only class and a code-only class in the same package that share ≥ 60 % of their member
  keys are reported as "possible rename" (`rename_of`). Spide never acts on it automatically.

### 4.6 Comparison shape (`crates/spide-core/src/sync.rs`)
- `Shape = BTreeMap<ItemId, ItemValue>`. Item ids and values:
  - `class:Foo`: `{package, kind: class|abstract|interface|enum}`
  - `field:Foo.x`: `{type?, static}`
  - `method:Foo.bar(int)`: `{returns?, static, abstract}`
  - `extends:Foo>Bar` and `implements:Foo>Bar`: presence only.
- `design_shape(design, manifest)`: accepted, live, non-external classes. Types are normalised (whitespace
  removed); an empty type is a wildcard.
- `code_shape(index, manifest)`: applies the `compare` filters (rules below). Classes are named by their design
  name when linked; otherwise their simple name is used, or the importer's FQN name on a collision.
- **Code filters** (unless enabled in `compare`):
  - **Private methods** are skipped.
  - **Accessors** are skipped. An accessor is a `get/is/set` method whose body facts show at most one statement
    touching a single field.
  - **Constructors** are skipped.
  - Fields are always compared.
- Equality is wildcard-aware: an empty design type matches any code type.

### 4.7 Sync status (three-way)
- `Baseline { version, updated_at, design: Shape, code: Shape }` is stored in `spide/sync.json`. An invalid file,
  for example after a merge conflict, is treated as no baseline, with a warning.
- For every item in design ∪ code ∪ baseline, Spide compares `d` (design), `c` (code), `bd` (baseline design) and
  `bc` (baseline code). Absence is a value.

| condition | status | action |
|---|---|---|
| `d ≈ c` | **in sync** | — |
| no baseline file | **unreconciled** (only in design / only in code / differs) | readjust or implement |
| `d≠bd`, `c=bc` | **design ahead** | implement |
| `d=bd`, `c≠bc` | **code ahead** | readjust |
| `d≠bd`, `c≠bc` | **conflict** | the user picks a side |
| `d=bd`, `c=bc` | **accepted difference** (hidden by default) | — |

- **Auto-advance:** when an item is in sync and the baseline entries differ, Spide sets them to `d`/`c`.
  - The write happens only when the index is `Current` (never `Partial` or `Error`), debounced by 1 s, with an
    atomic write.
  - So `sync.json` only changes towards agreement.
  - When the baseline file is missing, the first auto-advance creates it with only the in-sync items.
- **Mark in sync** is user-only. It sets the baseline to the current shapes. A confirmation lists the differences
  that will become accepted.
- **Class status** is the worst status of its items, in this order: conflict > design ahead = code ahead >
  unreconciled > accepted > in sync.
- `SyncReport { items: Vec<SyncItem>, classes: BTreeMap<String, ClassSync>, counts, partial: bool }`.
  - `SyncItem { id, class, kind, status, design, code, baseline, file?, line?, suggested_ops: Vec<Op>, decisions: Vec<String>, rename_of? }`.
  - `suggested_ops` brings the code value into the design: `add_class` (members after filters, package,
    stereotype), add/update/remove member, inheritance/realization relations, and `move_to_package`. Existing
    responsibilities, descriptions and rationale are kept.
  - `decisions` lists the accepted decisions with `decided_at > baseline.updated_at` whose applied ops touch the
    class.

### 4.8 Readjust (code → design)
- **Triggers:**
  - the Project panel buttons **Readjust design (n)** (all code-ahead, conflict-as-code and unreconciled items),
    **Readjust** on a class, and **Keep code** on a conflict;
  - `/spide readjust [Class …]`.
- Spide sends `prompts::readjust(scope)` to the same pi session. The `spide-readjust` skill guides the agent:
  1. `get_sync` for the scope, then `get_code` where the intent is unclear.
  2. Mechanical changes (new members, types, links) go through `propose_ops`, starting from `suggested_ops`.
     Changes in structure or responsibility go through `propose_decision` with alternatives (for example, adopt
     the code's split, or keep the design and implement it back).
  3. Responsibilities are added; framework classes are marked `external`; and design intent is never deleted just
     because the code lacks it.
  4. **Bootstrap**: with an empty design, the agent makes one decision per package with the public API only, then
     generates the domain model and the use cases.
- Accepted proposals make their items in sync, and auto-advance follows.

### 4.9 Implement (design → code)
- **Triggers:**
  - **Implement in code (n)** and the per-class **Implement**, plus **Keep design** on a conflict;
  - after review is complete, **Implement in code →**, in project mode when design-ahead items exist;
  - `/spide implement [Class …]`.
- **Preconditions:**
  - The index is `Current`. If it isn't, Spide shows "indexing…".
  - A warning is shown when pending proposals touch the scope. Only the accepted design is implemented.
- **Brief:** `sync::implement_brief(report, design, manifest, scope) -> String`, sent with
  `prompts::implement`. Per class it lists:
  - the target file: the existing one from the index, or `roots[0]/<fqn path>.java` for a new class;
  - the exact changes: add `bar(int): String`, change the type of `x` to `Map<…>`, remove `baz()`,
    extend `Bar`;
  - the class's responsibilities and rationale;
  - the related decisions (question, chosen option, notes).
- **The brief's rules:**
  - implement only these changes and keep existing behaviour;
  - follow the repository instructions;
  - don't change the design through Spide tools;
  - never commit, stash, reset or switch branches;
  - ask before deleting a file;
  - run `commands.test` (in `cwd`) if it is set;
  - finish with a two-line summary.
- `ImplementRun { items, started_at }` is set. When pi goes from Busy to Idle and the next index is `Current`,
  Spide computes how many items in scope are now in sync and shows it in a toast and in the panel, for example
  "Implemented 7/9 · remaining: …". The panel then offers **Ask agent to finish** and **Readjust these instead**.

### 4.10 Verify
- `/spide verify`, or **Verify** in the panel, re-indexes and shows the structural result.
- If `commands.test` is set, Spide sends `prompts::verify(cmd, cwd)`: the agent runs the command and reports
  pass/fail in one sentence.
- The panel states that a structural match does not prove the behaviour is correct.

### 4.11 Agent surface
- **MCP tools** (`app/src/mcp.rs`; the data comes from core):
  - `get_project`: the manifest, the root, the design path, the index status (files, classes, skipped,
    `parsed_at`), the baseline (whether it exists, `updated_at`) and the status counts.
    - `get_design` also carries a compact `project` summary.
  - `get_sync {status?: [...], classes?: [...], limit?: 200}`: a list of `SyncItem`.
  - `get_code {class | symbol, method?}`: the file, the line range, the text with line numbers (up to 400 lines;
    ask per method for longer classes), and a summary of the facts.
  - There are no tools to mark in sync or to start implementing; those are user actions.
- **Skills:**
  - `agent/skills/spide-readjust/SKILL.md` and `agent/skills/spide-implement/SKILL.md` are embedded with
    `include_str!`.
  - In project mode they are written to `.spide/skills/` at launch and passed to pi as `--skill <dir>`.
  - They can be forced with `/skill:spide-implement`.
- **Commands** (`spide-terminal.ts` list, palette `ui/state/commands.rs`, `App::command`):
  - `project` opens the panel, or the init form in design-only mode;
  - `readjust [classes]`, `implement [classes]` and `verify`;
  - `synced` opens the Mark-in-sync confirmation;
  - `lens sync`.
- The removed `/import`, `/drift` and `/code` stay removed. In project mode, the hover card and `get_code` replace
  them.

### 4.12 UI
- **Rail mode "Project"** (`app/src/ui/project.rs`) replaces "Source".
  - **Design-only mode:** a short explanation and **Create Spide Project in a code folder…**. The user picks the
    module folder, and the init form opens.
    - **Create** writes the manifest, copies the current `design.json` to `spide/design.json` (with its layout,
      decisions and history) and reopens Spide there.
    - The old folder is left untouched.
    - If the target already has `spide/project.json`, Spide simply opens it.
  - **Init and edit form:**
    - the name;
    - the roots, prefilled with `src/main/java` if it exists;
    - the excludes;
    - `code_root`, prefilled from the common package prefix of a preview import;
    - the compare toggles;
    - the test command and its cwd;
    - the git-exclude checkbox.
    - **Preview** shows the file and class counts, or the errors.
  - **Project mode:**
    - a header with the root, roots, `code_root`, the index status (**Re-index**) and the baseline ("no baseline
      yet" or its date);
    - status-count chips;
    - sections **Conflicts**, **Design ahead**, **Code ahead**, **Unreconciled**, and **Accepted** (collapsed).
      Class rows expand to their items with `file:line`, and each row and section has its action buttons
      (4.8–4.9);
    - the footer buttons **Mark in sync…**, **Verify** and **Edit manifest**.
- **Opening a project:** **Open Spide Project…** in the File menu and palette picks a folder. It opens a folder
  that has `spide/project.json`, or offers the init form.
- **Sync lens** (`/lens sync`, `Lens::Sync` in `class_view`):
  - class rings: design ahead teal, code ahead violet, conflict red, unreconciled amber dashed;
  - code-only classes are drawn as "only in code" ghosts, using the kept phantom rendering;
  - a status dot on each affected member row.
  - Clicking a code-only ghost opens the Project panel at that class.
- **Hover card** in project mode: "code: org.…Foo · src/main/java/…/Foo.java:12 · design ahead (2)".
- **Health:** `sync::with_code_facts` copies `CodeFacts` from linked code methods into the view. `long_method`,
  real clones and the code-based course checks then work again in project mode.

### 4.13 Edge cases
- **Partial or errored index:** the sync report is flagged `partial` and auto-advance is off. The panel shows the
  skipped files.
- **Branch switch, undo or redo:** sync is recomputed; the baseline and the code stay untouched.
- **External edits to `spide/design.json`** (for example `git pull`) become ghost proposals, as today (`watcher.rs`).
- **A `sync.json` changed by git** is reloaded when it changes (watched).
- **Root folder deleted or moved:** the panel shows "root missing" and offers **Open Spide Project…**.
- **Inner classes and code outside `code_root`:** they are handled as the importer names them (an FQN-based name
  on collision). They are never mapped by simple name.

---

## Files to modify
- **core:**
  - `model.rs`: domain model types;
  - `store.rs`: `set_domain_model` and the targets;
  - `tools.rs`: domain tool and `get_view`; remove `get_code` and `on`;
  - `derive.rs`: delete the derived domain;
  - `plantuml.rs`: domain and package exports;
  - `ops.rs`: `link_code`/`unlink_code`;
  - `lib.rs`;
  - **new** `project.rs` and `sync.rs`;
  - **delete** `drift.rs`, `code.rs` and `source.rs`;
  - tests: `store.rs` or a new `domain.rs`, **new** `sync.rs` and `project.rs`, `export.rs`, and the goldens
    `library-domain.puml` and `library-package.puml`.
- **diagram:** `views.rs` (`package_scene`) and `class_view.rs` (`DriftSets`→`SyncSets`, `Lens::Sync`, status
  dots).
- **app:**
  - `main.rs`, `state.rs` (`Shared.project`, `design_path`), `commands.rs` (open, init and convert;
    readjust/implement/verify; regenerate domain), `prompts.rs`, `mcp.rs`, `pi_control.rs`, `pi_bridge.rs` (the
    project root as cwd, `--skill`), `watcher.rs` (`design_path`), `branches.rs`, `history.rs` (if they hardcode
    `design.json`);
  - **new** `project.rs`;
  - the ui files: `app.rs`, `views.rs`, `export.rs`, `export_jobs.rs`, `canvas.rs`, `hover_card.rs`,
    `overlays.rs`, `timeline.rs`, `proposal.rs`, `state/commands.rs`, `tests.rs`, `menu.rs`, and the **new**
    `project.rs`;
  - **delete** `source_tracking.rs`, `source_implementation.rs`, `source_implementation_tests.rs` and
    `ui/source.rs`.
- **agent:** `spide-skill.md`, `spide-terminal.ts`, and the **new** `skills/spide-readjust/SKILL.md` and
  `skills/spide-implement/SKILL.md`; `tests/pi-extension.test.ts`.
- **other:** `examples/library.json`, `README.md`, `plans/unified-decisions-source-sync.md`.

## Reuse
- `Store::set_use_cases` (`store.rs:424`) is the pattern for `set_domain_model`, and the `derive.rs:246` rule is
  the pattern for `domain_stale`.
- The `ui/views.rs::usecase` banner and auto-ask, `commands::regenerate` and `prompts::gen_use_cases` are the
  patterns for the domain model.
- `App::domain_data` and `ClassCanvas` are fed from the stored model.
- `BatchRun`/`batch_step` (`app.rs:927`) gain the regenerating phase.
- `spide_import::import_java_scoped`/`scoped_files` build the code index. The importer's `SourceSymbol` for types
  (`Type`) and methods (`Type#sig`) is the link format.
- From `source.rs`: `signature`, the merge logic of `reconcile` (it keeps responsibilities and descriptions) and
  `with_bound_length_evidence` move into `sync.rs`.
- From `drift.rs`: the member comparison and the `adopt_ops` op building move into `sync::suggested_ops`.
- `source_tracking.rs`: the watcher pattern (recursive `notify`, debounce, generation guard) and `write_atomic`
  are reused in `app/src/project.rs` before the file is deleted.
- The phantom rendering in `class_view` is reused for code-only ghosts.

## Steps
- [x] **Part 3:** remove the Source attachment, implementation tasks, `/import`, `/drift`, `/code` and their MCP
      tools. Rename `DriftSets`→`SyncSets` and keep its rendering. The build stays green.
- [x] **Part 2:** the plain UML package diagram and PlantUML; update the golden.
- [x] **Part 1 core:** the domain model types, `set_domain_model`, the targets, staleness, the tool and
      `get_view`, PlantUML; tests.
- [x] **Part 1 app:** the Domain tab from the stored model, target translation, the hover card, banner and
      auto-ask, and the regenerate prompt.
- [x] **Part 1 batch export:** `stale_views` and the regenerating phase with Export anyway / Stop; tests.
- [x] **Part 4 core:**
      - `project.rs` (manifest and validation);
      - `sync.rs` (shapes, identity, filters, three-way status, auto-advance, mark in sync, `suggested_ops`,
        `implement_brief`, `with_code_facts`, rename hint);
      - `link_code`/`unlink_code` ops;
      - delete `drift.rs`, `code.rs` and `source.rs`;
      - tests.
- [x] **Part 4 app:**
      - project detection and `design_path`, the pi cwd and `--skill`;
      - the index thread and watcher, baseline persistence, git exclude;
      - init and convert, readjust/implement/verify prompts, `ImplementRun` completion;
      - the MCP tools `get_project`, `get_sync` and `get_code`;
      - the commands.
- [x] **Part 4 UI:** the Project rail mode and form, the sync lens, the hover card line, and the review-complete
      step.
- [x] **Part 4 agent:** the two skills, the skill section, and the terminal commands and their test.
- [x] `examples/library.json` domain model; the README; the superseded note.
- [x] Run the full test suite, then update the snapshots after a visual check.

## Verification
- **Automated:**
  - `cargo build --workspace` and `cargo test --workspace`;
  - `UPDATE_GOLDEN=1` only for the domain and package goldens, after reviewing the diff;
  - `npm test`;
  - `cargo test -p spide macos_view_snapshots -- --ignored`, with the package and domain PNGs inspected and
    updated deliberately.
- **Core tests** (`tests/sync.rs`, `tests/project.rs`):
  - identity: an implicit FQN maps; an explicit link maps; the same simple name in another package does **not**
    map;
  - wildcard types; the private, accessor and constructor filters; association-as-field;
  - each row of the status table (4.7), plus the no-baseline case;
  - auto-advance happens only for in-sync items and never with a partial index; Mark in sync sets everything;
  - applying `suggested_ops` puts the item in sync;
  - `implement_brief` contains the file paths, the changes and the decisions since the baseline;
  - the rename hint;
  - manifest validation: absolute or `..` roots are rejected, and a cwd inside the work tree is allowed.
- **App tests:**
  - detection (project, design-only, and the `spide/` subfolder);
  - convert writes `spide/*` and leaves the old folder unchanged;
  - git exclude is idempotent;
  - the index watcher picks up a written `.java` file in a temporary directory;
  - the `get_sync` and `get_code` MCP tools;
  - `ImplementRun` summary with a fake pi going Busy→Idle;
  - headless shots of the Project panel states and the sync lens;
  - domain tab prune isolation; batch export regenerating phase.
- **Manual check, on a disposable copy only:** `cp -r zap-extensions/addOns/restextraction /tmp/spide-rx`. The
  real repository is never touched.
  1. Convert a scratch design-only project to a Spide Project at `/tmp/spide-rx`. `spide/` is created, and the
     exclude is written only if it is a git work tree.
  2. With an empty design, **Readjust** bootstraps the class model as one decision per package. Approve them; the
     items become in sync and `sync.json` appears.
  3. Add a method to a design class and approve it. It shows as **design ahead**. **Implement** makes the agent
     edit the Java file, and the item ends up in sync.
  4. Edit a Java file by hand. The item shows as **code ahead**; **Readjust** proposes the change.
  5. Change the same method on both sides. It shows as **conflict**, and **Keep design** / **Keep code** route the
     work correctly.
  6. **Mark in sync** turns the remaining differences into accepted differences.
  7. **Verify** asks the agent to run the test command.
  8. The sync lens rings and the code-only ghosts match the panel.
- **Earlier parts:** check the domain, package and batch-export behaviour from Parts 1–2 as before. There is no
  Source button, and `/spide import`, `drift` and `code` no longer exist.
