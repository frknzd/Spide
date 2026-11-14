# Member-level problem feedback

## Context
Extend the existing class-level visual diagnostics to the specific problematic methods and attributes, so users can see where a problem originates.

Initial findings:
- `src/canvas/ClassNode.tsx` already renders individual member rows with hover/interaction targets, class heat borders/halos, and proposal/removal styling.
- Method rows support placement annotations (`marks`) and concern-colour strips, but not general member diagnostics.
- `src/state/insights.ts` provides `methodMarks`, restricted to placement-related issues and one annotation per method. General diagnostics must retain multiple issues and not treat all dotted targets as methods.

## Approach
Tentative: derive a shared member-diagnostic index from existing findings, add restrained severity feedback to affected rows, and expose explanations through existing member hover cards. Preserve status colours, concern lenses, and current interactions. Do not infer blame for every member from a class-wide issue.

Confirmed requirements:
- Always show diagnostic cues in the app for both methods and attributes, tied to the actual finding.
- Hidden problematic members need a count/indicator and a reveal action.
- Exports offer diagnostics versus clean shapes. Reuse the existing `diagnostics` export toggle (`src/ui/ExportDialog.tsx`, `src/export/svg.ts`, default off); audit all diagnostic-driven styling, not just removable `[data-diag]` overlays.
- Show a small public / package-private / protected / private tag on member hover (also accessible via keyboard inspection), based on model visibility rather than signature guessing.
- Add distinct, understandable indicators for design criteria grounded in the course and OO principles. Scope of new analysis versus presentation remains to confirm; do not claim LSP violations from inheritance shape alone or treat KISS as a precisely measurable rule.

Further findings:
- `src/ui/HoverCard.tsx` already has separate method and attribute cards; reuse these for visibility tags and grouped diagnostics.
- `crates/spide-core/src/course.rs` already checks long methods, Demeter, Creator, repeated switches and long parameter lists. Prefer exposing these existing findings before adding parallel analysis.
- `Issue` metadata already includes explanations, confidence, evidence basis and fix hints. Surface these rather than unqualified red warnings.
- Local course PDFs are available under `~/Desktop/Study/Toledo/Design of Software Systems Lecture [H0N00a]/`, including GRASP, UML, design patterns and Bad Smells. Read relevant sections before finalizing the criterion taxonomy.

## Files to modify
- `src/state/insights.ts`: reusable diagnostic indexing (pending deeper target-schema inspection).
- `src/canvas/ClassNode.tsx`: member row feedback.
- `src/ui/HoverCard.tsx`: member visibility tags and diagnostic explanations.
- `src/ui/ExportDialog.tsx`, `src/export/svg.ts`: existing diagnostics toggle and clean-export styling audit.
- `crates/spide-core/src/course.rs`, `crates/spide-core/src/metrics.rs`: only if additional evidence-backed checks are approved.
- Canvas integration and detail-reveal state: exact paths to confirm.
- Focused unit/browser tests: paths to confirm.

## Reuse
- `methodMarks` and `methodConcerns` in `src/state/insights.ts`.
- Existing row `data-target` / `data-hover`, status styling, and `ClassNode` shared rendering.
- Existing issue severity and hover presentation (inspect next).

## Calibrated diagnostics and drill-down
User delegates calibration and accepts clearly labelled potential concerns. Extend analysis selectively, not with an indiscriminate checklist of SOLID violations.

- Course grounding: `Chapter 5 Design Patterns/16 Bad Smells.pdf` explicitly says smells inspire refactoring and are not designed as metrics. It covers duplication, long methods, large classes, feature envy, shotgun surgery and long parameter lists. Use measured facts as evidence, not proof of poor design; suggestions should mention tradeoffs and testing.
- Audit current thresholds, confidence and exclusions in `course.rs`, `metrics.rs`, and `overlap.rs`. Keep current long-method threshold (>30 nonblank/noncomment lines) as a review signal, not a hard failure. Add boundary fixtures before changing thresholds. Avoid double-counting overlapping symptoms in health; retain severity separately from confidence.
- Indicator families: complexity/simplicity, duplication, responsibility/cohesion, coupling/Demeter, encapsulation, architecture, and inheritance/contract compatibility. Give each a compact labelled marker; aggregate multiple findings into one severity cue plus count, with all explanations available on inspection. Do not introduce animation or a chip for every metric.
- Add a conservative encapsulation concern for exposed instance state with exclusions for explicitly modelled data/value types. Do not treat visibility alone as a confirmed error.
- Audit inheritance checks; flag only provable model-level signature/visibility incompatibilities where the model can resolve them unambiguously. Describe these as compatibility concerns, not proof of behavioural LSP failure. No automatic LSP score from class shape; no speculative pattern recommendations solely to increase abstraction.
- Reuse `CodeFacts` (`model.rs`: file, source range, LOC, statements, chains, loops, switches) for an expandable member evidence section. Extend frontend types for existing optional facts rather than creating another parser. Missing imported code must say unavailable, never zero.
- Reuse `overlap.rs` / `MethodPair` similarity evidence. Show top related members with separate implementation-fingerprint similarity versus name/signature/responsibility similarity, source locations when available, and navigation to the counterpart. Do not present similarity percentages as probabilities of a defect; scope is the loaded/imported code, not an unscanned repository.
- Resolve diagnostic targets against actual members. Do not propagate class-level findings to every row; handle ambiguous attribute/method names conservatively and test overloaded/unresolved targets.
- Reveal hidden affected rows per class without permanently changing global detail preferences. Preserve the Domain view's intentional method hiding: offer inspection/class navigation for method findings rather than silently changing view semantics.

## Proposal dock expansion
- Expand the existing bottom proposal dock on hover or keyboard focus, showing proposal summary/rationale and a bounded change list. Multiple proposals get an overview; existing previous/next controls select the detailed proposal and retain locate/spotlight behaviour.
- Reuse `footprintOf` and proposal metadata; never generate a new explanation from guesses. Show a factual change summary when rationale is absent.
- Keep accept/reject controls stationary and explicitly labelled as bulk actions. Expand upward without moving the canvas; support pointer travel into the panel, keyboard focus, Escape dismissal, scrolling for long content, and safe bounds on narrow windows. Hover must never accept/reject or change the design.

## Additional files and reuse
- `src/state/types.ts`: optional imported facts and typed diagnostic evidence.
- `src/canvas/DiagramContent.tsx`, `src/canvas/Canvas.tsx`, `src/canvas/visuals.ts`: memoized diagnostics, row visibility/reveal and layout sizing; preserve previews and export mode.
- `src/ui/Chrome.tsx`, `src/App.tsx`, `src/styles.css`: proposal dock expansion and evidence/tag styling.
- `src/state/proposal-focus.ts`: reuse `footprintOf`, `ghostSpotlight` and proposal navigation.
- `crates/spide-core/src/overlap.rs`, `crates/spide-core/src/model.rs`: reuse current similarity and imported facts; avoid a new similarity engine.
- Existing member hover cards, status/concern styling, diagnostics export mode and browser smoke harness remain the integration points.

## Steps
- [x] Confirm scope and visual behaviour with the user.
- [x] Audit existing checks against course material; establish boundary/false-positive fixtures and document calibrated criteria.
- [x] Expose optional code facts, similarity drill-down and selective evidence-backed diagnostic improvements.
- [x] Trace diagnostic targets, member hovers, detail filtering, previews, and exports.
- [x] Implement shared member issue indexing without inventing member-specific diagnoses.
- [x] Add composable row cues and explanations across shared class renderers.
- [x] Extend the existing export diagnostics toggle to cover every new cue, retaining clean shapes by default.
- [x] Implement the proposal dock's hover/focus expansion using existing metadata and footprints.
- [x] Verify visible/hidden members, status/lens coexistence, accessibility, and performance.

## Calibration audit results
- Read Bad Smells and GRASP Information Expert material: keep smells as qualified review opportunities and preserve responsibility tradeoffs.
- Baselines retained: LOC >30, parameter count >4; no generic KISS penalty. Severity and confidence remain separate.
- Existing safeguards: Demeter design inference is skipped when code facts exist; feature envy skips already-misplaced methods and accessors; data-class rules exempt explicit value/record/DTO types. Similarity caps name-only matches and excludes trivial accessors and delegation. Preserve these rather than adding duplicate principle penalties.
- Inherited-field findings currently target only the class; improve actual target precision before displaying member blame. Behavioural LSP cannot be established from this model.
- Added `crates/spide-core/tests/member_diagnostics.rs`: 30/31 LOC boundary, missing source evidence, 4/5 parameter boundary, explicit data-type exemptions, explanations/confidence and health direction.
- Verified Rust member diagnostics, calibration and overlap suites: 16 passed, 3 existing debug tests ignored.
- Step 3: method hover cards now show collapsible code facts and separated similarity evidence with counterpart drill-down. Added qualified `exposed_state`, unambiguous `narrowed_visibility`, and member targets for repeated inherited fields.
- Step 4 trace: `Canvas.tsx` memoizes `methodMarks`, passes them to `boxesFor` (`keep`) and `DiagramContent` → `ClassNode`. Detail 1–2 has no member rows; detail 3 folds accessors/private helpers plus inherited/association attributes; Domain sets `showMethods=false`. Hover keys are `attribute:Class.name` / `method:Class.name`; overloaded methods share a row name. Export sets `plain` (metrics removed, `[data-diag]` stripped) while placement marks still flow, so new cues must check `plain`. Decision/Compare previews call `boxesFor` without diagnostics; keep them clean.
- Step 5: `memberDiagnostics`, family labels/icons and `evidenceLabel` in `src/state/insights.ts`; unit tests cover multiple findings, class-only exclusion, ambiguous attribute/method names, unresolved targets and evidence wording.
- Step 6: rows get a faint severity tint, edge bar and family glyph (+count); markers reserve row width. Hovered/keyboard-inspected rows show a transient visibility pill. Hover cards list each finding with family, evidence label, confidence, explanation, consideration and course source; class cards summarise member findings. Clicking a member pins its card (Esc/outside/close). A per-class chip reveals hidden findings (Class view) or points to inspection (Domain view methods) without changing the detail setting.
- Step 7: diagnostics-off export drops member findings, placement marks and reserved marker width (plain mode in `Canvas`/`DiagramContent`); `[data-transient]` visibility tags and reveal chips are always stripped in `src/export/svg.ts`. `.local/ui-audit/export-members.mjs` checked off/on previews on REST: no markers/tags when off, markers when on.
- Step 8: `proposalSummary` (rationale verbatim or null, footprint items, operation counts) feeds an upward panel in `ProposalBar` (overview list → pick to locate; ‹ › selects detail; 8-item bound; scroll; Escape; focus/hover; pointer bridge). Bulk actions have explicit labels and stay put. `.local/ui-audit/dock.mjs` passed single/multiple and 900px width, with no accept/reject calls from hover.
- Step 9: verification found `Visibility` defaults to public in sketched designs, so `exposed_state` now only judges classes with imported code (health of clean designs unchanged; REST stays 46). Passed: typecheck; 27 Node tests; full Rust workspace; 4 browser suites + ghost focus; library export smoke; REST keyboard inspection (tag opacity 1, finding card), concern-lens coexistence (5 rows), reveal/fold, pinned evidence drill-down, export on/off, dock checks; drag 60.5 fps. Domain view is a derived conceptual model without methods/metrics, so it intentionally shows no member findings. Code-fact UI verified with injected facts (examples carry none). App rebuilt and installed (backup `.local/app-backups/20261114-161718`).

## Verification
- Unit coverage: exact/ambiguous member matching, multiple findings, severity versus confidence, class-only issues, missing code facts, similarity evidence, and visibility labels.
- Rust fixtures: threshold boundaries, legitimate data types, signature compatibility where resolvable, unrelated similar names, small accessors and real duplicated bodies. Assert explanations and health impact, not just issue counts.
- Browser checks: visible and hidden rows, local reveal/reset, keyboard inspection, status/concern coexistence, counterpart navigation, and stable member hover.
- Proposal checks: one/multiple proposals, missing rationale, long summaries, pointer bridging, focus/Escape, disabled/preview states, revision changes, spotlight/locate and unchanged bulk actions.
- Export checks: diagnostics on/off in light/dark PNG/SVG/PDF; off removes row cues, diagnostic text, heat and warning colours but retains UML visibility, proposal/status semantics when requested and geometry. Transient hover tags/popovers never export.
- Run `npm run typecheck`, `npm test`, `npm run test:browser`, relevant Rust tests and `npm run test:export` with disposable project data. Exercise library and REST fixtures and compare idle/drag rendering cost.
- Keep this plan's verification checklist current. The shared REST-extension test-plan document is outside this Spide feature's scope; do not modify unrelated course documentation.
