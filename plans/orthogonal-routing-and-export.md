# Orthogonal "PCB" edge routing + dedicated export

## Context
Edges today are straight lines or single quadratic bends between box-border points (`src/canvas/Edge.tsx` `edgeGeometry`; `borderPoint` in `src/canvas/visuals.ts`; parallel-edge offsets via `bends()` in `src/canvas/DiagramContent.tsx`). They cut through classes and overlap one another. Goal: an elegant PCB look. Segments run only north/south/east/west, corners are rounded, a small hop marks each crossing, and lines never pass behind boxes. Use it everywhere arrows appear.

Export today means ⌘E PNG / ⇧⌘E PlantUML via `standaloneSvg` (`src/export/svg.ts`), `spide_core::png::render` (resvg 0.50) and `/export …` commands. Goal: one dedicated, polished export for the **current view, complete** (not just the visible area). It offers PNG, SVG, PDF and the existing formats, a light print theme (default) and a dark theme. It opens from a native **File › Export…** menu item.

Positions come from ELK (`src/canvas/layout.ts`) plus a settling d3-force sim (`src/canvas/physics.ts` `LivingLayout`), and change live during drags. Routing must therefore work on arbitrary fixed positions in the frontend; ELK's orthogonal router only works for ELK-owned layouts.

## Decisions (confirmed)
- All arrow views: Class, Domain, Package, System, Use case, decision mini previews, Compare. Sequence keeps its own lifelines.
- Class boxes (and system/use-case nodes) are hard obstacles. Arrows may cross package borders at a cost and strongly avoid unrelated packages' interiors.
- Fan-out: edges leaving or entering the same side of a box share **one port point** and a short common stub, then branch with rounded turns (bus/trunk style). The port is the side centre, offset only to separate different head kinds. Inheritance/realization into one parent therefore merges into a single triangle, as in UML. Parallel edges between the same pair are separated by a channel offset.
- While dragging, live re-route touched edges only, with an L/Z fallback when over budget.
- Export: current view in full. Formats are PNG, SVG and PDF, plus existing PlantUML, JSON, ADR and ArchUnit. Light print theme by default; dark is optional. Crisp output. Entry point is the native File menu.

## Approach
### 1. Router — new `src/canvas/route.ts` (pure, deterministic, unit-tested)
- **Input:** boxes `{id, x, y, w, h}`, soft zones (package hulls with owner ids), edges `{id, from, to, headKind}`. **Output:** per edge a list of axis-aligned points, plus crossing points for hops.
- **Ports:** pick a side per endpoint from relative geometry and the free direction (inheritance prefers top of the parent, bottom of the child). Group edges by `(box, side, head kind)` into one port; ports for different head kinds on one side are spaced ~14px apart around the centre. Each edge gets a 18px perpendicular stub out of the port.
- **Search:** sparse orthogonal visibility graph. Candidate x/y lines are obstacle sides ± margin (12px), port stubs and midlines between adjacent obstacles. Run A* with cost = length + bend penalty (~40) + soft-zone penalty (foreign package interior ~4×/px, hull border crossing +30) + penalty for reusing another edge's segment (except along a shared trunk). It never enters a hard obstacle (margin-inflated). Edges sharing a port are routed sequentially and reuse the trunk.
- **Nudging:** collinear overlapping segments of different edges (not the same trunk) within one channel are spread 8px apart. Order them by their ends to avoid self-made crossings, and clamp so they stay out of obstacles.
- **Simplify:** merge collinear points and remove zero-length segments. Self-loops get a fixed rounded rectangular loop at the top-right corner.
- **Crossings:** compute H×V intersections between different edges (excluding shared trunks/ports). A hop goes on the **horizontal** segment of the later edge in a stable order.
- **Budget/fallback:** whole-diagram routing is memoised by a positions + edges key. Over budget (~12ms per frame during drag, ~60ms full), it uses a direct L/Z route with ports. Full routing runs again on layout settle (`LivingLayout` `onSettle`) and on data change.

### 2. Rendering — `src/canvas/Edge.tsx`
- `Edge` accepts an optional `route: {points, hops}`. It builds the path with rounded corners (quadratic/arc, radius 10, clamped to half of each adjacent segment) and semicircular hops (r 5, lifted 5px) on horizontal segments. Use the straight geometry only where no route is given (none left after migration).
- Arrowheads, diamonds and triangles align to the first/last segment axis; shared-port heads are drawn once per port group. Multiplicities go just beside the port on the outer side, and the label goes at the midpoint of the longest segment with its background plate. Warn text stays above the label. Strain/ghost/removing styles are unchanged, and the hit-area path follows the route.
- Line caps/joins round; stroke widths unchanged (1.4). Keep `vector-effect` off so exports scale cleanly.

### 3. Integration
- `src/canvas/Canvas.tsx`: compute routes with `useMemo` from settled positions. During a drag, take the previous routes and re-route edges touching the dragged node each frame. Pass routes into `DiagramContent`.
- `DiagramContent.tsx`: class relations, implicit edges (focus or `/implicit`), overlap-lens edges and overview package deps are all routed. Package hulls are passed as soft zones. Drop `bends()` for routed edges; parallels are handled by the router.
- `src/views/PackageView.tsx`, `SystemView.tsx`, `UseCaseView.tsx`, mini previews in `src/ui/DecisionCard.tsx`, and `src/ui/ComparePanel.tsx` route with the same function over their node rects. Static views compute routes once per layout.

### 4. Export
- **Native menu (`src-tauri/src/main.rs`):** build the macOS menu from Tauri's default menu and insert **Export…** (⌘E) into the File submenu. Also add **Export Again** (⇧⌘E, repeats the last settings and path) if a previous export exists. Clicking emits `menu-export`; `App.tsx` listens and opens the dialog. The JS ⌘E/⇧⌘E handlers become fallbacks only outside Tauri. The Edit/Window menus stay so copy/paste keep working.
- **`src/ui/ExportDialog.tsx`:** a modal sheet with a live preview of the exact export (scaled thumbnail).
  - Format: PNG · SVG · PDF · PlantUML · JSON (design) · ADR · ArchUnit. Options irrelevant to the chosen format are hidden. ADR/ArchUnit/JSON reuse the existing commands; PlantUML exports the current tab.
  - Theme (image formats): Light (default) · Dark. Background: solid · transparent (PNG/SVG).
  - PNG scale: 1×, 2× (default), 3×, 4×, with the resulting pixel size shown. The PDF page is fitted to the diagram, vector, with embedded fonts.
  - Content toggles: title, date, design health, legend (relation notation and colours used in this view), ghost proposals, diagnostics chips/heat halos, package frames. Padding: compact / comfortable.
  - File name defaults to `<design>-<view>.<ext>`, then the native save panel. It shows progress and errors, and on success reports the path with a **Reveal in Finder** action.
  - Remember settings per project (`localStorage` keyed by project dir).
- **`src/export/svg.ts`:**
  - Export the full diagram bounds (the world bbox, not the viewport) at identity transform.
  - Before cloning, clear hover/spotlight/focus. Strip dimming, interaction classes, hit areas and animations so the export always shows the calm, un-hovered state.
  - Add `data-ghost` / `data-diag` markers in node/edge rendering so ghosts and diagnostics can be filtered.
  - Theme: a palette map from the dark tokens in `visuals.ts` `C`/`LAYER_COLOR` and the few hard-coded view colours to light equivalents (white paper, ink text, darker semantic colours for contrast). Applied by attribute rewrite. A test asserts that no unmapped dark fill/stroke remains.
  - Emit fonts as generic family stacks matching what resvg/svg2pdf resolve (Helvetica Neue/Menlo). Add title/date/health header and legend blocks built in SVG.
- **Rust:**
  - `crates/spide-core`: new `pdf.rs` using **`svg2pdf` 0.13**. It depends on usvg 0.45, so a second usvg version enters the tree alongside resvg 0.50; that's acceptable, and no newer compatible PDF crate exists. Add an `export_pdf(svg, path)` command. `export_svg(svg, path)` writes text.
  - `export_png` stays as is (resvg, scale clamp 0.5–6).
  - Wire the commands into `src-tauri/src/commands.rs`, `main.rs` and `src/state/api.ts`.
- Command catalog: `/spide export` opens the dialog. Existing `/export png|puml|…` subcommands remain direct shortcuts.

## Files to modify
- New: `src/canvas/route.ts`, `tests/route.test.ts`, `src/ui/ExportDialog.tsx`, `src/export/theme.ts`, `tests/export-theme.test.ts`, `crates/spide-core/src/pdf.rs`, `scripts/export-routing-smoke.mjs`
- `src/canvas/Edge.tsx`, `DiagramContent.tsx`, `Canvas.tsx`, `ClassNode.tsx` (markers), `visuals.ts`
- `src/views/PackageView.tsx`, `SystemView.tsx`, `UseCaseView.tsx`; `src/ui/DecisionCard.tsx`, `ComparePanel.tsx`
- `src/export/svg.ts`, `src/App.tsx`, `src/state/api.ts`, `src/state/command-catalog.ts`, `src/styles.css`
- `src-tauri/src/main.rs`, `src-tauri/src/commands.rs`, `crates/spide-core/Cargo.toml`, `crates/spide-core/src/lib.rs`; `package.json` (browser test script)

## Reuse
- `rectOf`, `boxesFor`, `HULL` hull geometry (`DiagramContent.tsx`, `layout.ts`) for obstacles and soft zones.
- `LivingLayout` settle callback (`physics.ts`) for full re-route timing.
- `standaloneSvg`, `api.exportPng`, `api.exportPlantUml`, `exportAdr`, `exportArchUnit`, `saveAs` flows in `App.tsx`. `useOverlay` (`src/ui/overlay.ts`) for the dialog. Tauri dialog `save`. Existing toasts.
- `spide_core::png` font database setup for consistent fonts in PDF.

## Steps
- [x] Router core: ports/fan-out, visibility graph A*, nudging, simplification, crossings, fallback; unit tests for invariants.
- [x] Edge rendering: rounded corners, hops, heads/labels on routes; migrate class/domain canvas (incl. drag re-routing).
- [x] Route package, system, use-case views, mini previews and compare.
- [x] Export pipeline: full-bounds clean SVG, light/dark theme mapping, header/legend, ghost/diagnostic filtering; SVG/PDF Rust commands (svg2pdf).
- [x] Export dialog with live preview and remembered settings; native File › Export… / Export Again menu; command catalog.
- [x] Verify (tests, screenshots, performance, real exports), rebuild and install the app.

## Verification
- **Unit (`npm test`):** every route segment is axis-aligned. No segment intersects an inflated hard obstacle. Endpoints lie on the correct box sides, and shared-port groups start at one point. Output is deterministic. Hops appear only at true H×V crossings between different edges. Fallback triggers under a tiny budget. The theme map leaves no dark colours.
- `cargo test --workspace`: the PDF of a fixture SVG is valid (starts with `%PDF`, parses, contains text). PNG unchanged.
- **Browser smoke (mocked backend, disposable fixtures, all six views):** screenshot each view. Assert edge-path bounding segments avoid node rects, measured via DOM geometry. Run the existing hover/drafts/terminal suites.
- **Performance:** route time for library (≈10 edges) and the larger REST fixture (≈40–60 edges); drag frame time stays ~60 fps in the `?perf=1` overlay; idle canvas does no routing work.
- **Export:** produce PNG (2×, 4×), SVG and PDF in light and dark themes for Class/Package/System. Inspect them visually (sharp text, nothing clipped, no hover dimming, ghosts toggle works). Open the PDF/SVG in Preview. PlantUML/JSON/ADR/ArchUnit still write files.
- **Desktop:** the File menu shows Export… with ⌘E, Edit-menu copy/paste still works in pi, and the dialog round-trip through the native save panel works. Then rebuild and install `/Applications/Spide.app` with a backup.
