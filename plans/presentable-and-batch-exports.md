# Presentable live diagrams and batch export

## Context
Exports must be readable, compact and logically arranged, with peripheral external classes, non-overlapping nodes and labels, and UML-correct routing. Add batch export across designs and selected detail levels/configuration.

## Findings
- Current implementation is native Rust/egui, not the historical TypeScript export plan.
- PNG/SVG/PDF share retained scenes; PlantUML uses its own layout engine.
- `app/src/ui/export.rs` already provides persisted settings, format selection and live preview.
- `app/src/ui/app.rs::export_svg` builds image exports; `app/src/commands.rs::export_plantuml_all` already enumerates non-empty views for PlantUML.
- Detail levels are 0–4. Batch scope is current-project diagram views, including System variants and sequence scenarios (not branches/projects).
- `export_svg` currently builds Class/Domain from live canvases but takes other views from the current cached scene; batch needs explicit view-scene construction without switching tabs.
- `visuals.rs::class_box` truncates member signatures and caps responsibility text; accurate bounds alone will not guarantee complete text.
- `layout.rs` already implements Sugiyama, component packing and external package classification. `route.rs` already has obstacles, soft package zones, shared ports and lane nudging: improve these rather than replace them.
- `crates/spide-diagram/tests/render.rs` provides the existing rendering-test entry point.

## Approach
- Reuse the scene/rendering/export pipeline; improve measured geometry, compact logical placement and obstacle-aware routing rather than adding a second renderer.
- Treat text/node visibility and UML meaning as constraints; optimize compactness and routing within those constraints (not promise zero crossings for arbitrary graphs).
- Batch jobs use immutable snapshots and selected settings, with deterministic names and explicit results.
- Confirmed: batch covers all diagram views in the current project, with multiple selected formats and detail levels/settings.
- Confirmed: improve the live canvas layout, not a separate export layout. Quality takes priority even on the live canvas. Latest clarification permits layout to override all placements/pins; no pin-preservation constraint is needed.
- Confirmed: wrap long signatures/responsibilities and expand boxes instead of truncating. Preserve intentional detail-level filtering, but fully show content selected by that level.
- Canvas and export use identical measured geometry, layout and routing for the same view/detail/configuration; export never applies a private cosmetic rearrangement. Different detail levels may have different layouts because their boxes differ.
- Use a quality-first deterministic layout pipeline: measure complete visible content → compact logical package/component placement → reserve annotation and routing clearance → route → place labels → validate and repair. Prefer near-square/rectangular bounds and high occupied-area density, subject to readable gaps and architecture/inheritance ordering. External nodes/packages occupy nearby perimeter bands, not remote islands.
- Score candidate packings by area/aspect, excessive gaps, edge length/bends/crossings and semantic ordering; disallow node/text collisions. Use stable tie-breaking. Do not shrink text or hide members to improve density.
- Strengthen existing orthogonal routing with separate lanes and early divergence for unrelated edges, adequate endpoint/head clearance and obstacle-safe nudging. Reserve measured label/multiplicity rectangles; reroute/expand local spacing when placement fails. Share trunks only when UML meaning is unambiguous; preserve relation direction, hollow inheritance/realization triangles, correct diamond ownership, dashed styles and multiplicities.
- Replace budget-expiry L/Z routes as final results with validated quality routes; cache by all geometry/config inputs. Schedule expensive work off the UI thread using owned measured geometry, discard stale revisions, and show an updating state rather than freezing interaction or exporting unfinished geometry. Avoid physics undoing the optimized compact arrangement.
- Batch dialog: all non-empty views by default, both System variants and every sequence scenario; selected detail levels 0–4 for Class/Domain, other views exported once per settings profile because those levels do not apply. Select multiple image formats and named configuration profiles (theme/content toggles/padding/PNG scale), defaulting to the current settings. Preview job count and representative output.
- PNG/SVG/PDF participate in the full product of view × applicable detail × settings profile × format. Offer PlantUML once per applicable view with a clear independent-layout/full-model notice; do not generate misleading duplicate detail/theme variants. JSON/ADR/ArchUnit remain project-level exports, outside diagram batching.
- Batch uses one immutable project snapshot and the same scene preparation as single export, without switching tabs or persisting temporary detail selections. Create a new uniquely named output directory; deterministic sanitized filenames include view/scenario identity, detail and profile, with stable collision suffixes. Write through temporary files, report per-job failures/skips and final paths, support cancellation between jobs, and retain completed outputs. Persist batch selections separately from single Export Again.

## Files to modify
- `app/src/ui/export.rs`: batch selections and progress.
- `app/src/ui/app.rs`: export scene preparation and dispatch.
- `crates/spide-diagram/src/{class_view,layout,route,scene,svg,views}.rs`: relevant geometry/layout/routing and rendering fixes after inspection.
- `crates/spide-diagram/src/visuals.rs`: complete text measurement and class row geometry.
- `crates/spide-diagram/src/physics.rs` and live canvas integration: stable collision resolution and preservation of manual placements.
- `crates/spide-diagram/tests/render.rs`: rendered-output regression coverage.
- `app/src/ui/canvas.rs`, `app/src/ui/views.rs`, `app/src/ui/paint.rs`: shared layout lifecycle, view enumeration and real-font measurements.
- New `app/src/ui/export_jobs.rs`: immutable job enumeration, progress, cancellation and safe writes; wire through `app/src/ui/mod.rs`.
- `app/src/ui/tests.rs`, module unit tests, `README.md`: integration coverage and documented semantics.
- `crates/spide-core/src/plantuml.rs` only if needed to expose existing view/scenario export selection without active-tab mutation.

## Reuse
- `ExportSettings`, `ExportDialog`, `FORMATS` in `app/src/ui/export.rs`.
- `build_export_svg` in `crates/spide-diagram/src/svg.rs`.
- Existing image writers and PlantUML all-view enumeration in `app/src/commands.rs`.
- `layout::{compound, layered}` and existing component packing; `class_view::{boxes_for, rect_of, external_classes, build}`.
- `route::{route_all, RouteCache, deco_for, route_cmds}` and existing lane nudging/hops.
- `views::{package_scene, system_scene, usecase_scene, sequence_scene}` and their state/cache types; retain sequence-specific lifelines rather than forcing a class layout.
- `ClassCanvas::build_scene`, `EguiMeasure`, retained scene primitives and SVG bounds; extend these shared paths.

## Steps
- [x] Confirm batch scope and presentation-layout behaviour.
- [x] Establish regression fixtures and shared geometry assertions; inspect remaining geometry/paint paths before editing.
- [x] Remove visible-content truncation; implement measured multiline rows and complete bounds for class text, badges, package titles, view labels and sequence messages. Align native and SVG font/line-height behaviour.
- [x] Improve compound/component packing and external placement; make layout respond to detail/font/configuration changes and finish with collision validation rather than unbounded force drift.
- [x] Improve route lane allocation, label placement and endpoint decoration; validate post-nudging paths and repair unsafe fallbacks. Integrate quality computation/caching with live canvas.
- [x] Extract explicit view/detail scene preparation shared by live, single-export and batch paths; ensure export waits for final quality geometry and preview uses that exact scene.
- [x] Add batch profiles/formats/detail selection, immutable job enumeration, progress/cancellation and safe output handling.
- [x] Verify rendered outputs and canvas parity; document layout overrides, applicability of detail/settings, and PlantUML limitations.

## Verification
- Geometry tests for text bounds, box separation, edge obstacles, label collisions and UML endpoints.
- Dense, disconnected, external-heavy and long-text fixtures across selected detail levels.
- Inspect actual PNG/SVG/PDF outputs, not only serialized markup.
- Batch tests: exact job counts, both C4 variants, all scenarios, empty views, unsupported combinations, duplicate/unsafe names, cancellation, partial failures and settings migration. Export does not change active tab, selection, project model or saved detail settings; shared normal layout settlement is allowed.
- Verify same-state canvas/single/batch node bounds and routes match; detail changes invalidate geometry; stale background results cannot replace current scenes. Test fonts with the native measurement path as well as headless approximations.
- Record compactness/aspect/edge-quality baselines for library and REST-extraction plus dense, cyclic, disconnected, singleton and external-heavy fixtures. Assert meaningful bounded regression thresholds rather than a universal square-shape guarantee.
- UML tests cover every relation kind, self-loops, parallel/opposite edges, shared parents, ownership, multiplicities and labels. Test no post-nudge obstacle intersections and no final unsafe timeout fallback.
- Run `cargo test --workspace`, focused diagram/app tests and existing `npm test` integration suite after implementation. Manually inspect light/dark PNG/SVG/PDF at every applicable detail; exercise batch cancellation and reopen persisted settings. Record timing on representative large diagrams and confirm idle canvas does no layout work.
- Update test documentation alongside implementation. Workspace instructions also require updating the synced REST Extraction Extension Test Plan.docx: inspect applicability, back up under `.local/testplan/`, check unchanged immediately before writing, preserve XML formatting and append an edit-history row. Do this only during implementation, not this markdown-only planning phase.

## Outcome
- Compactness (footprint area / bounding box), before → after: rest-extraction 0.17–0.29 → 0.32–0.41, library 0.19–0.29 → 0.26–0.37, all aspect ratios 0.95–2.2. Asserted in `crates/spide-diagram/tests/render.rs` (no overlaps, routes clear of boxes, no fallback routes, no truncated text, external classes at an edge, every UML relation kind).
- Release timing: layout ≤ 2 ms, routing + scene ≤ 16 ms for rest-extraction (30 classes); kept on the UI thread (below a frame), so no background scheduling was needed.
- Batch: `app/src/ui/export_jobs.rs` (planning, names, atomic writes) and the All diagrams mode in `export.rs`; `SpideApp::export_scene` is the one scene builder for preview, single and batch exports; tests assert single = batch SVG at the live level and that the canvas/tab/detail stay unchanged.
- Known limits: labels on short same-layer horizontal edges can still sit tight against their boxes when no free spot exists. Pre-existing failure, unrelated and also on HEAD: `hover_cards_reserve_height_even_for_short_content_and_fit_small_canvases` (egui texture delta).
- The REST Extraction Extension Test Plan (.docx) covers the ZAP add-on, not Spide; it was not changed.
