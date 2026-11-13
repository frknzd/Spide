# Spide: faster canvas, less clutter, diagnostics based on the course

> Status: **ready for review**. The first round of questions is answered (see *Decisions*).

## Context

The screenshot (`~/Desktop/Screenshot 2026-11-13 at 22.02.11.png`, the 26-class ZAP REST-extraction design in `~/Documents/Spide/untitled`) shows three problems:

1. **Clutter.** Package hulls overlap (`model · domain` covers `ui` and `adapter`). Classes overlap each other (RestExtractor sits under ApiDocumentationWriter and the package hover card). Every box lists all members: accessors, constructors, private helpers, and attributes that repeat a drawn association. Library classes (`zap`, `swing`) take as much space as the design itself.
2. **Performance.** Code reading shows why the canvas is slow:
   - `LivingLayout` never settles while `tension ≥ 0.08` (`alphaTarget > 0` plus `jitter()`). Every tick calls `setFrame`, which re-renders the whole `Canvas`/`DiagramContent` tree, and each node has a `feGaussianBlur` halo that also pulses through CSS.
   - `useAgentFeed()` lives in `App`. Each streamed text delta re-renders `App`, and with it `Canvas`, which isn't memoised and receives inline callbacks.
   - All member rows render at every zoom level, so DOM size grows with members rather than with classes.
3. **Diagnostics are structural only.** They cover coupling, LCOM4, cycles, layers and orphans, but not purpose. Nothing checks whether a class's methods fit what its name and role say (a `*Panel` doing domain calculations), whether the same behaviour exists in several classes, or where it should live instead. The course (Roel Wuyts, DOSS) grades exactly this: minimise coupling, increase cohesion, distribute responsibilities (RDD, GRASP, bad smells, exam notes).

## Decisions (from the user)
1. Physics **settles and stops**. There is no perpetual trembling.
2. Detail is controlled by a **granularity slider** with several levels.
3. The tool is **LLM-first**. The agent records its own judgement of purpose, fit and duplication. That judgement sits on top of the deterministic signals (names, stereotypes, method verbs, calls) and is checked against them.
4. The tool must **check the actual implementation** (imported Java code) for the same diagnostics, not just the design.
5. Health stays a **single indicator**. It is refined to be more accurate and precise, and calibrated against course examples.
6. An **`external` tag** marks packages and classes that belong to external systems or libraries.
7. Order of work: performance → decluttering/layout → responsibility and overlap engine plus tools → course checks and health → diagnostics UI.

## Course material → evaluation criteria

Sources are in `~/Desktop/Study/Toledo/Design of Software Systems Lecture [H0N00a]/`.

| Course source | Principle | What Spide will measure |
|---|---|---|
| Ch2 *Basic OO Design* | Minimise coupling, increase cohesion, distribute responsibilities ("the whole course in one slide") | Health = 3 principle sub-scores, plus Architecture rules and UML hygiene |
| Ch2 Low Coupling | The coupling kinds are attribute, service use, param/local, inherits, implements. "Inheritance is strong coupling." "Coupling to stable libraries is NOT a problem, to unstable elements IS." | Weighted coupling: inheritance counts more; coupling to `external`/stable packages is discounted; instability of the target is used |
| Ch2 High Cohesion | Cohesion levels range from very low (several functional domains) to high (limited responsibilities in one domain). Watch out for "global system" classes. | LCOM4 (existing), plus **concern purity**: how many functional concerns a class's methods span |
| Ch2 *Coupling & cohesion in practice* | **Law of Demeter**: only talk to self, arguments, objects you create, and your fields. Example 1: the Client knows the IndirectProvider. Example 2: switches on a type code scattered over 11 classes call for Polymorphism. | `demeter` issue: calls to classes that are not friends (reached through a getter chain). `type_switch` risk: an enum/type code used by the methods of ≥3 classes. |
| Ch2 *RDD* | Responsibilities are knowing, doing and deciding. "Make objects smart, not just data + getters/setters." Distribute behaviour with delegated control. | Each method is classified (knowing/doing/deciding plus a concern). `data_class` (anemic) issue. `feature_envy` issue. |
| Ch3 GRASP Controller | The controller coordinates and delegates. Fat controllers become god classes. Decouple the GUI from the domain. The incorrect example has the UI calling `makeLineItem` on the domain directly. | `fat_controller`. `ui_bypasses_controller`: presentation code calls *doing* methods on domain classes directly. |
| Ch3 Creator | B creates A if B composes, contains, records or closely uses A, or has the data to initialise it | `creator` check: who creates A (calls `new A`/`A.<ctor>`, or `create` messages in sequences) compared with who composes/aggregates A |
| Ch3 Information Expert | "That which has the information does the work." | The core of **suggested home**: the candidate's expert score is the share of the data the method touches that the candidate owns |
| Ch3 Pure Fabrication / Indirection / Protected Variations | Invent a class when no expert fits without hurting cohesion or coupling. Wrap a variation point in a stable interface. | Suggested home may be a **new Pure Fabrication class** in the lowest legal layer. Change scenarios already cover Protected Variations. |
| Ch5 Bad Smells | Duplicated code ("number one in the stink parade"), Large Class, Feature Envy, Shotgun Surgery, Long Parameter List | `duplication` (overlap engine), `feature_envy`, `scattered_concept` (shotgun surgery), `long_params` (> 4) |
| Ch6 Exam info | Do not repeat inherited members. Do not add attributes for drawn associations. Getters/setters only when needed. Always give argument types. Class and sequence diagrams must match. Composite needs add/remove/getChildren. A Visitor with one element is a Strategy. "No magic methods/classes." Fail reasons include "no behaviour" and "god class". | `uml_hygiene` issues, which also drive **decluttering**: hide association-duplicate attributes and inherited/accessor members by default |
| Ch4 Monopoly | When there is no clear winner, compare the alternatives' coupling and cohesion, then think about future evolution | Suggested-home candidates come ranked with a simulated health/coupling/cohesion delta. Change scenarios break ties. |

Every issue gets a `principle` (`coupling | cohesion | responsibility | architecture | uml`) and a `source` (e.g. "GRASP Information Expert, Ch3"), so the UI and the agent can cite the course.

## Approach

### A. Performance (first, measurable)
- Measure before and after: a dev `?perf=1` overlay showing FPS, React commits per second for `Canvas`, and the timing of `metrics::compute` (Rust, logged in debug).
- Isolate the canvas from chat streaming:
  - move `useAgentFeed` into the components that need it (`AnswerBubble`, `PromptBar`, conversation).
  - wrap `Canvas` and `DiagramContent` in `memo`.
  - wrap `App`'s callbacks in `useCallback`.
- Physics settles and stops. Remove `alphaTarget` tension and `jitter()` entirely. The simulation runs only after a model change, a drag or a tidy, and stops at `alpha < 0.005`. Tension stays visible through cheap CSS: hot-node halos and strained cycle edges. The README's "trembling canvas" row gets updated.
- Replace `feGaussianBlur` halos with a stroke-based glow (no filters). Pulse only for heat > 0.55, and pause all CSS animations while panning or zooming (`.interacting` class) and when the window is hidden.
- Level of detail: below zoom 0.6, render class header plus chips only; below 0.3, packages as labelled boxes with counts.

### B. Decluttering and layout
- **ELK hierarchical layout** with packages as compound nodes, so package hulls cannot overlap and classes stay inside them. Layer order runs top to bottom (ui, application, domain), with infrastructure/adapters at the side. It is used for the initial layout, for new batches and for `/tidy`. Physics only nudges during a drag, followed by an overlap-removal pass.
- **Granularity slider** (bottom-left next to the zoom, also `/detail 0–4`; persisted per view in `design.json` settings):
  - 0 **packages**: packages as boxes with class counts and aggregated dependencies.
  - 1 **names**: class name, stereotype and chips.
  - 2 **responsibilities**: names plus the class's responsibilities/purpose line (RDD view).
  - 3 **design** (default, exam style): public behaviour with argument types. Accessors and constructors are folded into a "+n accessors" row. Private helpers are hidden. Inherited members are not repeated. Attributes that duplicate a drawn association become the edge's role name.
  - 4 **full**: everything (today's rendering).

  Semantic zoom can lower the effective level when zoomed out (never raise it), so the DOM stays small. Box sizes come from the effective level, so the layout is recomputed (ELK) when the slider changes. The PNG/PlantUML exports follow the slider.
- **`external` tag**:
  - `external: bool` on `Package` and `Class`, set through `update_class`/`add_package`/a new `update_package` op, or by the user with `/external <pkg|class>`. It is auto-suggested as a ghost proposal for packages without a layer whose classes have no outgoing dependencies, and for imported types outside the code root (`zap`, `swing`).
  - A package can optionally link to a C4 `ExternalSystem` (`system` field), so the System tab and the class canvas agree.
  - Rendering: a compact strip of header-only classes with an `«external»` chip.
  - Metrics: coupling to external elements is discounted (course: stable libraries are fine). External classes are excluded from responsibility, overlap and god/cohesion checks.
- Implicit (dotted) edges show only on hover or focus, or with `/implicit`.

### C. Responsibility engine (new `crates/spide-core/src/responsibility.rs`)

The engine works on any `Design`: the intended design, or the imported code ("reality"; see C+).
- **Class role**, with its source recorded, resolved in this order:
  1. explicit `role` (new optional field on `Class`, set by the agent or user)
  2. stereotype
  3. name suffix lexicon:
     - presentation: `Panel|View|Dialog|Frame|Window|Page|Screen|Widget|TableModel|Renderer|Presenter`
     - controller: `Controller|Handler|Coordinator|Session|Facade`
     - service: `Service|UseCase|Interactor`
     - persistence: `Repository|Dao|Store`
     - adapter: `Adapter|Listener|Client|Gateway|Extension*`
     - output: `Writer|Exporter|Formatter|Serializer|Printer`
     - creator: `Factory|Builder`
     - vague: `Manager|Helper|Util|Processor|Data|Info`
  4. package layer
- **Method concern**, with its source recorded. An explicit `concern` (new optional field on `Method`) wins. Otherwise the concern comes from verb lexicon, signature, `uses` and `calls`:
  - `presentation`
  - `coordination` (pure delegation)
  - `domain_logic` (calculate/count/match/validate/filter/aggregate…, or derives values from another class's data)
  - `persistence`
  - `formatting`
  - `creation`
  - `accessor`

  RDD kind: knowing, doing or deciding.
- **Role × concern fit matrix**, explained per cell. Presentation allows presentation, coordination, accessors and light formatting; domain logic there is `misplaced_responsibility` (severity 0.6). Entities must not do persistence or formatting (the slide's `Sale.save` example points to a Pure Fabrication). Controllers allow coordination and creation; domain logic or many attributes there is `fat_controller`. A vague name is reported as `vague_role`.
- **Other responsibility diagnostics**:
  - `feature_envy`: a method calls more getters on X than it touches its own data.
  - `data_class`: only accessors, while other classes compute with its data.
  - `ui_bypasses_controller`
  - `demeter`
  - `creator`

### C+. LLM judgement layered on the signals
- A new `assessments` map in `Design`, keyed by class, is written by the agent through a new MCP tool, `assess_class`:
  - `purpose`: one sentence
  - `role`
  - per method: `{concern, rdd: knowing|doing|deciding, verdict: fits|misplaced|duplicate|delegates, home?, confidence 0..1, reason}`
  - `basis: design|code`
- Each assessment stores a **content hash** of the class (and of the method source when `basis = code`). It becomes `stale` when the class changes. Stale assessments are listed in `get_design` so the agent refreshes them.
- **Combining**: for each method, the final verdict merges the heuristic signal and the agent's verdict.
  - When they agree, confidence rises.
  - When the agent wins over a heuristic, the issue shows "agent: … (overrides name heuristic)".
  - When they conflict on a high-severity item, Spide raises a `disagreement` note: the user sees both, and the examiner must justify.
  - Unassessed classes fall back to the heuristics alone.
- Spide prompts the agent (`prompts.rs`) to assess new or changed classes after each accepted decision, and to assess everything after `/import`. The work is batched, so one `assess_class` call covers several classes.

### C++. Checking the implementation (extend `spide-import` and drift)
- Extend `RawMethod` in `crates/spide-import/src/lib.rs` with **body facts**:
  - LOC and statement count (Long Method)
  - `new X` creations (Creator)
  - receiver-chain depth, e.g. `a.getB().getC().x()` (Law of Demeter)
  - `switch`/`instanceof`/enum-constant branches (Replace Conditional with Polymorphism)
  - arithmetic and comparison operator counts, loops over another class's collection (domain computation)
  - UI-API usage such as `javax.swing`/`JOptionPane` (presentation)
  - source span (`file`, `start_line`, `end_line`)
  - a **normalised token fingerprint**: identifiers become `ID`, literals become `LIT`, then k-gram winnowing for clone detection

  These facts are stored in a new optional `Method.code` struct on the reality design only.
- Run the responsibility engine, the overlap engine and the course checks on **reality** too.
  - Overlap on code also uses fingerprint similarity (real duplicated code, Type-1/2 clones), not only name and signature.
  - The results go into a `CodeFindings` report next to `Drift`: code issues, and **design-vs-code disagreements** (e.g. "design: RestExtractionPanel only presents data; code: `callsOf` loops over `EndpointReport.getCalls()` and counts → domain logic in UI").
- New MCP tool `get_code {class, method?}` returns the source snippet (read from `settings.code_dir` through the stored span, read-only and size-capped). The LLM can then judge the real implementation and record `assess_class {basis: "code"}`.
- Health of the code is computed the same way as for the design. `/drift` shows both scores ("design 78 · code 61").
- Fixture: the Monopoly case study source from the course (`Chapter 4 Monopoly case study/OSS-3 Monopoly Case Study - src.zip`, unpacked into `crates/spide-import/tests/fixtures/monopoly/` if its licence allows; otherwise a small hand-written equivalent).

### D. Overlap engine (new `crates/spide-core/src/overlap.rs`)
- **Method similarity** is a weighted score in 0..1, with a breakdown:
  - 0.35 × Jaccard of name tokens (camelCase split, light stemming, synonyms such as remove/delete and count/tally)
  - 0.25 × signature similarity (param/return types)
  - 0.25 × Jaccard of call targets and touched data types
  - 0.15 × description/responsibility tokens
- **Exclusions**, so legitimate patterns don't count as duplication:
  - accessors and constructors
  - overrides of the same supertype/interface method (that is polymorphism)
  - **pass-through delegation** (`RestExtractor.addEndpoint` calling `ApiNamespace.addEndpoint`). This is reported separately as delegation. It only becomes the Middle Man smell when most of a non-controller's methods are pass-throughs.
- **Class overlap**: the share of A's behavioural methods that have a near-duplicate (≥ 0.6) in B, plus responsibility-text overlap. The result is an overlap matrix.
- **Scattered concepts** (shotgun surgery): domain tokens (e.g. "trailing slash", "call count") whose behaviour appears in ≥ 3 classes.
- **Suggested home** for a method or a duplicate group. Each candidate class is ranked on:
  - expert score (Information Expert)
  - layer legality for every caller (`rules::allowed`, forbidden deps)
  - **simulated impact**: `impact::impact_of_ops` with remove plus add ops gives the health delta, fan-out change and LCOM change
  - role fit

  There is also always a **new Pure Fabrication** candidate (name derived from the shared tokens, placed in the lowest legal common layer). It wins when no expert scores ≥ 0.5 or when no existing home is legal for all callers.
- Expected findings on the current design (acceptance examples):
  - `RestExtractionPanel.callsOf`: a Demeter chain plus domain counting in UI. Suggested home: `ApiReport`/`ApiNamespace`.
  - `ApiNamespace.withoutTrailingSlash`: duplicates the path rules claimed by `UrlPath` ("the one place for path rules"). Suggested home: `UrlPath`.
  - `Endpoint.countCalls`: uses no own data and overlaps `EndpointReport.callCounts`. Suggested home: `EndpointReport`.
  - `RestExtractor`'s six methods are pass-throughs to `ApiNamespace`. These are reported as delegation, not duplication, because the class is a controller.
  - `ApiDocumentationWriter`: role is output (`Writer`) but the stereotype says `service` and it sits in the `ui` layer. Reported as a role inconsistency.

### E. Scorecard and health
- `Metrics` gains:
  - `principles: {coupling, cohesion, responsibility, architecture, uml}`, each 0–100 with its issues
  - `responsibility` (roles, concerns, mismatches)
  - `overlap` (top pairs, class matrix, scattered concepts)
- **Health stays one number** (0–100), recomputed more accurately:
  - Replace the ad-hoc penalty sum with a weighted mean of the principle sub-scores (coupling 0.25, cohesion 0.2, responsibility 0.3, architecture 0.15, UML hygiene 0.1).
  - Each sub-score is size-normalised: issues are weighted by severity × confidence and divided by the number of classes or methods in scope. A large design is then not punished just for being large, and one bad class in 5 hurts more than one bad class in 50.
  - Confidence comes from the C+ merge: agent agreement raises it, a lone weak heuristic stays low.
  - The sub-scores appear only in the breakdown popover.
- **Calibration suite** (`crates/spide-core/tests/calibration.rs`) asserts *orderings*, not exact numbers:
  - Monopoly "MonopolyGame does everything" < delegating design
  - POS "UI calls `makeLineItem`" < UI → Register controller → Sale
  - `Sale.save()` (Expert) < `PersistentStorage` (Pure Fabrication)
  - CigarOperator switch in many classes < polymorphic operator
  - Brewery exam "fail" sketch (god class, no behaviour) < "pass" sketch
  - the REST-extraction design before < after applying the suggested homes

  Thresholds are tuned until all orderings hold.
- The pairwise overlap is O(methods²). That is fine for hundreds of methods; it is cached per `rev`. `suggest_home` simulations run on demand only, never per snapshot.

### F. Agent tools (MCP, `tools.rs`) and skill
- `get_responsibilities {class?}`: role and its source, per-method concern and RDD kind, mismatches with explanations.
- `get_overlap {class?, min_score?=0.5}`: method pairs with score breakdowns, the class overlap matrix, scattered concepts and delegation chains.
- `suggest_home {class, method} | {group: ["A.m", "B.n"]}`: ranked candidates with expert score, legality, simulated health/coupling/cohesion delta, the new-class option, and ready-to-use `ops` for each candidate.
- `get_metrics` gains the principle breakdown, and each issue gets its `principle`/`source`/`confidence`.
- `assess_class {assessments: [...]}` (C+) records the LLM judgement. `get_code {class, method?}` (C++) returns the implementation. Both overlap/responsibility tools take `on: "design" | "code"`.
- `agent/spide-skill.md`:
  - set `role` on classes and `concern` on unclear methods
  - before proposing members, run `get_overlap`
  - "fix with agent" on a responsibility/duplication issue turns into a `propose_decision` whose options are the top `suggest_home` candidates (expert, new Pure Fabrication, keep with justification)
  - the examiner checks defences against the course principles

### G. Diagnostics UI
- The health ring stays a single number. Its popover shows the 5-principle breakdown, with issues grouped by principle, each with a course citation, its confidence (heuristic/agent/code) and "fix with agent".
- The hover card shows **Purpose**: the agent's purpose sentence, the role and why (e.g. "presentation, from name *Panel*, agent agrees"). It also lists misplaced or duplicated methods with "→ suggested home". With code imported, it adds a "code says" line and a link to open the snippet.
- On the canvas, a misplaced method row gets an amber `↗ Home` marker in `design`/`full` detail. A responsibility lens (`/lens concerns`) colours method rows by concern. An overlap lens (`/lens overlap`) draws dashed "duplicate n%" links between classes, plus chips for scattered concepts. The lenses are off by default to avoid clutter.

## Files to modify
- Perf/UI:
  - `src/App.tsx`
  - `src/state/store.ts`
  - `src/canvas/{Canvas,DiagramContent,ClassNode,Edge,physics,layout,visuals,Viewport}.tsx|ts`
  - `src/styles.css`
  - `src/ui/{HoverCard,Chrome,PromptBar}.tsx`
  - `src/state/types.ts`
- Core:
  - `crates/spide-core/src/{model,metrics,tools,ops,lib}.rs`
  - new `responsibility.rs` and `overlap.rs`
  - `plantuml.rs` (detail level/external)
- App: `src-tauri/src/commands.rs` (`set_detail`, perf timing)
- Agent: `agent/spide-skill.md`, `src-tauri/src/prompts.rs`
- Tests:
  - `crates/spide-core/tests/{responsibility,overlap,principles}.rs`
  - fixture `examples/rest-extraction.json` (copy of the screenshot design)
- Docs: `README.md`, `plans/interactive-design-verification.md`

## Reuse
- `metrics::compute`: the dependency graph (`deps`, `implicit`), `type_refs`, `lcom4` and `Issue::new`/`default_explanation` (extended with `principle`/`source`).
- `impact::impact_of_ops` and `health_delta` for simulating suggested homes.
- `rules::allowed`, `rules::forbidden_by` and `rules::type_tokens` for legality.
- `scenarios::score` for sequence-based creator checks.
- `elkjs` (already a dependency) for compound layout. `boxesFor`/`classBox` get extended with detail-level filtering.

## Steps
- [x] A0 Add the fixture `examples/rest-extraction.json` (copy of the screenshot design)
- [x] A1 Perf overlay and baseline numbers on `examples/rest-extraction.json`
- [x] A2 Agent feed out of `App`; memo `Canvas`/`DiagramContent`; stable callbacks
- [x] A3 Physics settles; filter-free halos; pause animations while interacting; LOD
- [x] B1 ELK compound layout by package and layer; overlap-removal pass
- [x] B2 Granularity slider (levels 0–4) and semantic-zoom clamp; association-attribute, inherited-member and accessor folding; exports follow
- [x] B3 `external` tag on packages and classes, ops, auto-suggestion, compact rendering, metric discount, optional link to a C4 system
- [x] C1 `role`/`concern` model fields plus ops (`update_class`/`update_method`)
- [x] C2 `responsibility.rs` classification, fit matrix and issues, with tests
- [x] D1 `overlap.rs` similarity, exclusions, class matrix and scattered concepts, with tests
- [x] D2 `suggest_home` ranking with simulated impact, with tests
- [x] D3 `assessments` model, `assess_class` tool, staleness hashes, merge with heuristics (confidence, disagreements)
- [x] D4 Import body facts, spans and fingerprints; run the engines on reality; `CodeFindings` and design-vs-code disagreements; `get_code` tool; Monopoly fixture
- [x] E1 Principle breakdown, `principle`/`source`/`confidence` on issues, size-normalised single health score, calibration suite
- [ ] E2 Course checks: Demeter, feature envy, data class, fat controller, UI bypasses controller, creator, long params, UML hygiene, composite/visitor sanity
- [ ] F1 MCP tools and skill/prompt updates
- [ ] G1 Popover scorecard, hover purpose, home markers, lenses
- [ ] Docs and verification notes

## Verification
- `cargo test --workspace`. The new golden tests on `examples/rest-extraction.json` assert the five acceptance findings above, with no duplication reported for the `RestExtractor` delegation.
- `npm run typecheck`.
- Perf, in the running app with the 26-class design:
  - idle CPU is about 0% after the layout settles
  - streaming an agent answer causes 0 `Canvas` commits (React profiler, or the `?perf=1` counter)
  - panning holds about 60 fps
  - `metrics::compute` stays under 20 ms on a synthetic 200-class design
- Visual, via `/dev/snapshot` and a browser on `localhost:1420`: no package hulls overlap and no classes overlap after `/tidy`; `design` detail hides accessors and association attributes. Compare screenshots before and after.
- Calibration: `cargo test -p spide-core calibration`. Every course-example ordering holds.
- Code: `/import` the ZAP REST-extraction add-on source (`zap-extensions/addOns/<the add-on>`) and the Monopoly fixture. `/drift` shows design and code health, code clones with their similarity, Demeter chains and switch smells, plus design-vs-code disagreements. `get_code` returns the right snippet.
- Agent: after `/import`, the agent calls `assess_class` with `basis: "code"`. Changing a class marks its assessment stale.
- Agent: in a session, ask "is there duplicated functionality?". The agent calls `get_overlap`/`suggest_home` and raises a decision with an expert option and a Pure Fabrication option.

## Notes
- `cargo test` golden files (PlantUML) will change because of the detail levels. They get re-accepted with `UPDATE_GOLDEN=1` after review.
- Old `design.json` files still open: every new field is `#[serde(default)]`.
