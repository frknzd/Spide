# Sharp PNG exports + Obsidian Canvas export

## Context
- Exported PNGs look blurry and not sharp (e.g. `~/Downloads/us10-diagrams-latest/*.png`, exported at 2×).
- Add **Obsidian Canvas** (`.canvas`, [JSON Canvas 1.0](https://jsoncanvas.org/spec/1.0/)) as an export format for **all diagrams**, in single and batch export. Relation semantics go into edge labels. No diagnostics; package and class (layer) colours are kept.

## Findings
### PNG
- Pipeline: `build_export_svg` (`crates/spide-diagram/src/svg.rs`) → `spide_core::png::render(svg, scale)` (`crates/spide-core/src/png.rs`, resvg + tiny-skia `encode_png`). Callers: `app/src/ui/app.rs::run_export` → `commands::export_png`, `batch_step`, quick export (`scale: 2`).
- The pixels are fine. A 2× export inspected at 1:1 (crops of `class-3-design.png` and `domain-3-design.png`) is crisp.
- **Root cause:** `encode_png` writes no `pHYs` chunk, so every file is 72 dpi. macOS Preview, Quick Look, Pages/Keynote and Word lay a 72-dpi image out at 1 px = 1 pt. On Retina (2 device px per pt), a 2× export at "actual size" is therefore upsampled another 2× and looks blurry. It also lands at twice the intended physical size, so documents shrink it and resample it again.
- The export dialog's preview has the same problem. It is rasterized at ≤ `min(1100/max(w,h), 2)` *pixels* but drawn at that many *points*, ignoring `ctx.pixels_per_point()`, so it is blurry on Retina (`app/src/ui/export.rs::dialog`).
- `png 0.17.16` is already in `Cargo.lock` (via tiny-skia 0.11). Its encoder supports `set_pixel_dims`.

### Canvas
- `SpideApp::export_scene(view, level, options)` (`app/src/ui/app.rs`) is the single scene builder for preview, single and batch exports. It uses the same settled layout as the live canvas and builds with `export_plain` (no diagnostics) when `options.diagnostics` is off.
- Scenes (`crates/spide-diagram/src/scene.rs`) contain `Prim`s (rects, lines, text in nested translated `Group`s, see `Xf`) plus `Hit`s with world rects/polylines and a `Target` (`Class`, `Concept`, `Package`, `Actor`, `UseCase`, `System`, `Container`, `Relation`, `ConceptLink`, `Link`, `Message`, …).
- Colours: classes use `layer_color(package.layer)` (`visuals.rs`) for header/fill, and package frames use the same colour (`class_view.rs:1056`). Other nodes use the neutral palette.
- `Format`/`FORMATS` live in `app/src/ui/export.rs`. Batch formats are hard-coded to PNG/SVG/PDF in `export_jobs::jobs` and `batch_panel`.

## Approach
### 1. Sharp PNGs
- `png::render(svg, scale)` encodes through the `png` crate instead of `encode_png`. It demultiplies the tiny-skia pixels and writes `pHYs` = **72 × scale ppi** (2× → 144 dpi). On macOS the image then shows at its 1× logical size with 1:1 device pixels on Retina, and documents insert it at the intended size instead of double. Applies to single, quick and batch exports.
- Export-dialog preview: rasterize at `display_scale × pixels_per_point` (capped by the existing 64 Mpx guard) and draw it at its point size, so the preview is crisp on Retina.
- Keep the 1–4× scale options and the size label (pixel size, plus the dpi).

### 2. Obsidian Canvas export
- New pure module `crates/spide-diagram/src/canvas.rs`: `to_json_canvas(scene: &Scene, m: &dyn Measure, design: &Design, kind: CanvasView, opts) -> String`. It is built from the **same export scene**, so positions match the canvas, PNG and SVG exactly. The scene is always built with diagnostics off for this format.
- **Nodes** come from scene `Hit` rects with targets:
  - Class / Concept / Actor / UseCase / System / Container → `text` node (x, y, w, h rounded to integers; stable ids such as `class:LendingService`).
  - Package → `group` node (`label` = package name, `color` = layer hex).
  - Card text = the scene text drawn inside that node's world rect, so the same detail level is honoured. It is ordered top-down and converted to Markdown: stereotype in italics, name as a bold heading, compartments separated by `---` (detected from the full-width separator lines), members in inline code. Text is assigned only to leaf nodes, never to groups.
  - Colour: the class's or package's layer colour as `#rrggbb` (Obsidian accepts hex); neutral nodes get no colour.
- **Edges** come from the model relation (from/to/kind/label/multiplicities), using the route polyline from the scene `Hit::Line` when present:
  - `fromSide`/`toSide` = the side of each node rect the route's end touches (nearest side as a fallback).
  - `label` carries the UML meaning, e.g. `extends`, `implements`, `◆ owns · 1 → 0..*`, `◇ aggregates`, `uses «include»`, `depends on`, plus the relation label and multiplicities.
  - `toEnd: "arrow"` for directed kinds and `"none"` otherwise. Edges are uncoloured.
- **Sequence diagrams:** each lifeline is a tall `group` (label = participant) with a header card. Each message is an anchor card ("n") on the sender's and on the receiver's lifeline at the message's y. The edge between them carries the message (`n. method(args)`; replies as `↩ result`). Combined fragments become groups labelled `alt`, `loop`, ….
- **Obsidian sizing:** Obsidian cards render at ~16 px, but our boxes use 12–13 px. Scale all coordinates and sizes by one factor `k`, the smallest that lets every card's estimated Markdown height and width fit, clamped to 1–2. This keeps relative layout and avoids overlaps.
- Title/date (if enabled) become a text node above the drawing. Legend, theme, background, margins and scale don't apply and are hidden for this format.
- **App:**
  - `Format::Canvas` ("Obsidian Canvas", ext `canvas`, `image: false`, but it shows the scene preview because the layout is the same).
  - Options shown: Title, Date, Package frames, Ghost proposals.
  - Single export goes through `run_export`; batch gets a "Canvas" checkbox next to PNG/SVG/PDF and joins the view × detail × profile product (file `class-3-design.canvas`, …).

## Files to modify
- `crates/spide-core/Cargo.toml` (+ `png = "0.17"`, already in the lockfile), `crates/spide-core/src/png.rs`
- `crates/spide-diagram/src/canvas.rs` (new), `crates/spide-diagram/src/lib.rs`
- `app/src/ui/export.rs` (format, options, crisp preview, batch checkbox), `app/src/ui/export_jobs.rs` (formats list), `app/src/ui/app.rs` (`run_export`, `batch_step`), `app/src/commands.rs` (`export_canvas` writer)
- Tests: `crates/spide-core/tests/export.rs`, `crates/spide-diagram/tests/render.rs` (or new `tests/canvas.rs`), `app/src/ui/tests.rs`
- `README.md` (export formats, PNG dpi)

## Reuse
- `SpideApp::export_scene` / `export_svg_for` (same scene for canvas); `export_jobs::{jobs, write_atomic, unique, slug}`.
- `scene::{Hit, HitShape, Prim, Group, Xf, bbox}`; `visuals::{layer_color, Rgb}`; `class_view::package_label`.
- Example designs `examples/{library,rest-extraction}.json` and the headless `render_design` helper in `crates/spide-diagram/tests/render.rs`.

## Steps
- [x] PNG: encode with `png` crate + `pHYs` (72 × scale ppi), demultiplied RGBA; test reads back the IHDR size and `pHYs` value and checks that a transparent background stays transparent.
- [x] Preview: render at `pixels_per_point`, draw at logical size; show dpi in the size label.
- [x] `canvas.rs`: node extraction (hits → nodes, text → Markdown cards, groups, colours), edge mapping with sides and UML labels, sequence mapping, `k` scaling, deterministic ids and output.
- [x] Unit tests: valid JSON Canvas (unique ids, edges reference existing nodes, integer geometry, sides valid); class level 1 vs 4 card contents; no diagnostic text (`cycle`, `rule ✗`, …); layer colours; every relation kind's label/arrow; no card overlaps after scaling; sequence/usecase/C4/package views from the examples.
- [x] App: `Format::Canvas` in `FORMATS`, its option set, `run_export`, batch checkbox + job planning, `batch_step` writer; update batch job-count tests.
- [x] README: formats table, PNG dpi note, Canvas mapping/limitations (UML notation as labels).

## Verification
- `cargo test --workspace` and `npm test`.
- Manual check of the PNGs: export 2× → `sips -g dpiWidth` reports 144. Open in Preview at Actual Size on Retina and confirm the text is crisp and the logical size is the same as 1×. Insert into Pages/Word and check the size and sharpness. Check that the dialog preview is crisp.
- Manual check of Canvas: export the class (levels 1 and 4), domain, package, C4 (both), use-case and one sequence diagram, plus a batch with Canvas checked, into an Obsidian vault folder. Confirm it opens without errors, the layout matches the PNG, cards aren't clipped, package groups and layer colours look right, edge labels read correctly and there are no diagnostics.
- The ZAP test plan .docx is about the ZAP add-on, not Spide, so it doesn't need an update.
