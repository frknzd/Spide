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

## Embedded pi / workspace pass (2026-11-14)

Implementation: `plans/embedded-pi-and-hover.md`. This replaces the RPC/reconstructed-chat transport described in older entries above; it does not certify the older outstanding real-agent workflows.

### Actual results
- `cargo test --workspace`: **88 passed, 4 ignored**, including PTY ANSI/split UTF-8/input/resize/exit, bounded backpressure/replay, active detached-descendant shutdown, control authentication, stale/duplicate response handling, message caps and timeout cleanup. Existing ignored external/scale checks remain ignored.
- `npm test`: **6 passed** (fake-timer hover/placement, draft isolation, modified-key encoding, fake extension handshake, busy follow-up exactly once, reload lifecycle, images/context transformation and visible failure with draft restoration).
- `npm run typecheck`, `npm run build`: passed. Extension separately typechecked against installed pi **1.5.2** public `ExtensionAPI` declarations; corrected the lifecycle to supported shutdown/start hooks (no invented `session_switch` event).
- `npm run test:browser` (Chrome, disposable library snapshot; every browser mutation intercepted): passed at **1440×900** and **800×560**. Checks compact radio selection, zero collapsed mini diagrams, lazy details, explicit reversible preview, no preview layout saves/pruning, revision cancellation, decision drafts/navigation, six-view hover (class/domain/package/system/use-case/sequence), narrow drawer not covering pi, footer bounds, collapse/reopen retaining one xterm and no restart. Screenshots: ignored `.local/workspace-{1440,800}.png`. The second browser script uses deterministic mocked **native IPC** under React StrictMode: one polling stream, split ANSI/UTF-8 decoded intact by xterm, Ctrl+P/L and Shift+Tab, negotiated Shift+Enter / backward-cycle encoding, collapse retention, zero App/Canvas renders while streaming, and stale-generation restart all passed.
- Continuous-motion hover: **83–94 ms** to first DOM appearance over measured browser runs (91 ms in the final run) (intent timer is 60 ms). Further same-target movement produced **0 App / Canvas / DiagramContent renders**. Tooltip frame updates stay local. Removed old entrance animation so it doesn't add perceived delay.
- `npm run test:terminal`: real installed pi in a disposable PTY, **all seven built-in tools active** (`read`, `bash`, `edit`, `write`, `grep`, `find`, `ls`), authenticated handshake, **one model-select event after Ctrl+P**, xterm modifyOtherKeys negotiation, and **native `/reload` followed by authenticated reconnect**. No model requests, source edits, optional package installs or permission auto-approvals. User settings file modification time remained unchanged. Ctrl+L / Escape / Shift+Tab bytes also exercised, but their visual UI is not asserted by this byte-stream smoke.
- Repeated real PTY startup with project MCP config: pi displayed **Trust project folder?** and deliberately did not handshake until trust. This is expected: no automatic approval was added. A first launch can remain `starting` while this native prompt is waiting.
- `npm run app:build -- --bundles app`: release app built (about **21.8 MiB**, existing large-bundle advisory only). Corrected the generated bundle's known resource/signature mismatch with `codesign --force --deep --sign -`; `codesign --verify --deep --strict` passed. **No install/replacement of `/Applications/Spide.app`.**
- LaunchServices/Finder-style launch of the built bundle with **PATH=/usr/bin:/bin** and a disposable project succeeded. It found managed pi and Node under nvm despite no `bin/pi` symlink. Fixed this actual PATH recovery failure during verification. Apple-event Quit terminated both app and owned pi; no live child remained.
- `cargo fmt --check` could not run: the installed Rust toolchain lacks `rustfmt` (not installed as part of this change).

### Remaining manual/platform checks (not claimed as passed)
- A visible native WebKit session is still required for full model picker/backward cycling, multiline input/paste, clipboard/selection, alternate screen, permission prompts, and transcript scroll/tool expansion. Desktop screen capture was unavailable (`could not create image from display`). The real PTY and headless Chrome checks are **not** a substitute for this.
- Real MCP proposal accept/reject, examiner verdict, fix-with-agent, import/generation, dialogs, branches/project switches and typed-draft preservation while a live model is busy remain end-to-end manual checks. The fake extension tests exercise the transport/delivery semantics, not a model verdict. No source-edit/shell model turn was executed against user code.
- Sample native WebKit CPU while streaming a model response. Same-target Chrome motion has no app/diagram render storm; terminal rendering is imperative, but real streaming CPU is not measured here.
- Unix-domain control transport: macOS/Unix only, no Windows support claimed. Active descendants (including separate sessions) are stopped/reaped on owned-session shutdown; independently daemonized descendants already reparented after an externally killed pi are not guaranteed collectible. A renderer recreated after consuming more than the bounded replay window may need a native redraw; collapse/reopen and StrictMode reuse the same xterm/cursor instead.
- Spide-only changes do not alter ZAP's REST Extraction extension or its tested behaviour. The separate shared **REST Extraction Extension Test Plan.docx** was therefore not edited; this repository's verification record is the relevant test-plan update.

### Reproduce safely
1. Copy `examples/library.json` to a fresh temporary folder's `design.json`; start the debug app with `SPIDE_PROJECT` pointing there (optionally `SPIDE_PI=/usr/bin/false` for browser-only checks).
2. Start `npm run dev`; run `npm run test:browser`. Mutations are mocked, including layout saves and agent intents.
3. Run `npm test`, `cargo test --workspace`, `npm run typecheck`, `npm run build`, `npm run test:terminal`.
4. For native desktop checks, launch the built bundle on that disposable project, answer native trust/permission prompts yourself, and use `/spide` for app commands. Do not use user source files as shell/edit fixtures.

## Edit history
| Author | Version | Edit | Date |
|---|---|---|---|
| AI coding assistant | 1 | Record automated/dev checks and outstanding manual/regression checks | 2026-11-12 |
| AI coding assistant | 2 | Diagnostics and UI pass: performance numbers, automated checks, smoke runs, remaining manual checks | 2026-11-13 |
| AI coding assistant | 3 | Real pi PTY/private control, responsive hover, three-pane workspace, explicit read-only alternatives; measured tests, release/Finder verification and native/platform limitations | 2026-11-14 |
