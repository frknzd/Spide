# Interactive design — verification record and follow-up test plan

## Automated checks
- `cargo test --workspace`: passed after phases A–G/H/E; includes backward-compatible loading, force operations, rule matrices and metrics, impact, scenarios, provenance, history, C4, Java import, drift and exporter goldens.
- `npm run build`: passed (Vite reports a bundle-size advisory).
- C4-PlantUML syntax checked and rendered with locally installed PlantUML.
- ArchUnit generation tested against goldens. Compilation/execution not checked: no ArchUnit jar installed locally.
- Read-only Java import smoke checks: callhome, network, commonlib and automation add-ons; release imports observed under 100 ms. No source changes made in those repositories.

## Dev-harness checks performed
Temporary projects and a disabled agent (`SPIDE_PI=/usr/bin/false`):
- Force user edits, mode switches, invalid mode rejection, Socratic structural-proposal rejection, undo.
- Rule preset changes, external rules edits, undo, issue metadata.
- Option impacts, side-by-side comparison screenshots.
- Change impact/locality and stress-overlay screenshots.
- History list/read/restore-as-proposal, fork, invalid branch names.
- Branch create/switch/compare/merge, active-branch deletion guard.
- ADR and ArchUnit file export.
- Java import, drift report/overlay, adopt-as-proposals.
- C4 container tab screenshot and derived dependencies.
- `/add` command and `/focus` screenshots.

## Outstanding checks / known limitations
Do not interpret completed implementation checklist items as completion of these manual checks.
- Run real-pi conversations for brief elicitation, all partner modes, defence against forces, change analysis and fix-with-agent.
- Exercise actual mouse/keyboard flows in the desktop app: inline rename, drag-to-package, Option-click pinning, timeline scrub/fork, provenance navigation and clipboard.
- Investigate pin persistence under concurrent views: a single browser page saved pinned coordinates correctly, but repeated headless checks while the desktop view was also running subsequently lost them. Suspected competing layout writers; not resolved.
- Verify read-only historical previews cannot mutate the live design through canvas prune/click interactions.
- History revision lookup across branch switching needs a regression test: branch revisions may overlap.
- Java import is source-structure analysis, not full compiler symbol resolution. Add coverage for duplicate simple names, nested classes, overloads of equal arity, generic inheritance and malformed sources.
- Scenario aggregate checks infer changes from calls, not explicit read/write annotations; treat warnings as heuristics.
- Branch merge is a proposal-based comparison against the other branch, not a conflict-aware three-way merge.
- Verify design-to-code mappings for nonmatching package names and custom rule patterns; ArchUnit should be compiled against a fixture when its dependency is available.


## Diagnostics and UI pass (plans/diagnostics-and-ui.md)

Fixture: `examples/rest-extraction.json` (the 26-class design from the screenshot). Debug build, `SPIDE_PI=/usr/bin/false`.

Performance, measured before and after (idle, after the layout settled):

| | before | after |
|---|---|---|
| WebKit GPU process (Tauri app) | ~42% CPU | ~1% |
| WebKit WebContent process | ~9.5% | ~0.6% |
| spide (Rust) | ~1.8% | 0% |
| React renders while idle (`?perf=1`, headless Chrome) | 4–12 DiagramContent renders/s | 0 |
| SVG elements on the canvas | 1465 (full members) | ~800 at the default design level, ~250 zoomed out |
| `metrics::compute`, 26 classes | — | ~3 ms (dev, opt-level 2 for spide-core); 200 synthetic classes: 12 ms release |
| snapshot build (incl. decision previews) | 8–13 ms (old, fewer checks) | 35–45 ms dev |

What changed: the force simulation settles and stops (no tension jitter), blur-filter halos became plain strokes, decorative CSS animations run a few cycles then rest and pause while panning/zooming, the chat feed no longer re-renders the app/canvas (external store), `Canvas`/`DiagramContent` are memoised with stable callbacks, semantic zoom draws less when zoomed out.

Automated checks:
- `cargo test --workspace`: responsibility, overlap/homes, assessments, code facts and findings (hand-written Monopoly-like Java fixture), calibration orderings (Monopoly, POS controller, Sale.save vs Pure Fabrication, CigarOperator switch, Brewery fail/pass, god class in 5 vs 50, REST before/after suggested homes), 200-class scale test, PlantUML goldens at the full and design levels (PlantUML `-checkonly` passes).
- The five acceptance findings on the fixture: `RestExtractionPanel.callsOf` misplaced → `ApiReport`; `ApiNamespace.withoutTrailingSlash` → `UrlPath`; `Endpoint.countCalls` → `EndpointReport`; `RestExtractor`'s pass-throughs reported as delegation (no duplication, no middle man, no fat controller); `ApiDocumentationWriter` role inconsistency.
- Headless screenshots: no class or package overlaps after `/tidy` (checked by bounding boxes), detail levels 2/3, health popover, concerns and overlap lenses, hover purpose.
- Smoke runs (not committed): the course's Monopoly source (GPL, so not copied into the repo) imports and is judged (cycle Board⇄…⇄Square, god Player, Creator in `Main` skipped as composition root); the ZAP `restextraction` add-on source shows the real clones `ApiNamespace.withoutTrailingSlash ≈ UrlPath.removeTrailingSlash` and `ApiNamespace.parse ≈ EndpointPattern.readPath`, Demeter chains in `CallTableModel`, domain loops in `RestExtractionPanel`.

Still to check by hand:
- With a real pi: the agent uses `get_overlap` / `suggest_home` when asked "is there duplicated functionality?", raises a decision with an expert and a Pure Fabrication option, records `assess_class` (also with `basis: "code"` after `/import`), and an edited class shows up as a stale assessment.
- Streaming an agent answer causes no `Canvas` renders (verified structurally: `App` no longer subscribes to the feed; needs the `?perf=1` counter during a real conversation).
- Panning frame rate in the desktop WebView (headless Chrome holds 60 fps).
- Heuristics stay heuristics: role and concern detection read names, verbs and calls (and bodies when imported); the agent's assessment or an explicit `role`/`concern` overrides them. Calibration asserts orderings, not absolute numbers.

## Edit history
| Author | Version | Edit | Date |
|---|---|---|---|
| AI coding assistant | 1 | Record automated/dev checks and outstanding manual/regression checks | 2026-11-12 |
| AI coding assistant | 2 | Diagnostics and UI pass: performance numbers, automated checks, smoke runs, remaining manual checks | 2026-11-13 |
