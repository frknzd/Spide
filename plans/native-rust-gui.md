# Migrate Spide's frontend from TypeScript/React (Tauri webview) to native Rust (egui)

Status: **ready for review**

## Context

Spide's backend is already Rust (`crates/spide-core`, `crates/spide-import`, `src-tauri/src`). The UI is a React/TS app running in a Tauri webview. It is about 8.7k dense lines (544 KB) in `src/` plus 831 lines of CSS, and it depends on elkjs, d3-force, xterm.js and framer-motion. Goal: one native Rust binary with an **egui/eframe** UI. It must keep **every feature and behaviour**, look **similar** (same dark palette, layout, diagram notation and visual vocabulary) and use native widgets. This is a **big-bang** replacement: `src/`, Vite, Tauri and Playwright are removed at the end.

Decisions (from the user):

- **GUI:** egui/eframe.
- **Look:** similar to today, native widgets.
- **Behaviour:** identical features and behaviour.
- **Layout:** use `rust-sugiyama` or another layout crate. Port ELK's subset if the crates fall short.
- **Dropped:** the browser dev mode (`/dev/*` HTTP routes and Vite preview), the perf overlay, and any Windows ambitions. The app stays macOS/Unix only, as today, because the pi control transport uses Unix sockets.
- **Tests:** port the TS tests to Rust; `egui_kittest` replaces Playwright.
- **Uncommitted changes:** commit them first.

## Approach

### Crate layout

```
crates/spide-core      unchanged (model, metrics, derive, png/pdf, plantuml …)
crates/spide-import    unchanged
crates/spide-diagram   NEW, no egui: scene model, layout, physics, routing, visuals, SVG export
app/                   renamed from src-tauri; package `spide`, bin; eframe app + existing services
```

`spide-diagram` is GUI-independent, so routing, layout and export can be tested headlessly. The SVG export and the on-screen painter draw the **same `Scene`**. That keeps exports faithful to the screen, the way `export/svg.ts` is today.

### Backend decoupling (`app/`)

- Remove `tauri`, `tauri-build`, `tauri-plugin-dialog`, `tauri.conf.json`, `build.rs` and `gen/`.
- `AppState` drops its `AppHandle` field and gets an `Events` sink: an `mpsc::Sender<UiEvent>` plus `egui::Context`, so a send can `request_repaint()`.
  - `UiEvent` covers `Snapshot(Arc<Snapshot>)`, `Toast{kind,text}`, `SpideCommand{id,generation,project,command}`, `Menu(Export | ExportAgain)` and `TerminalOutput`.
  - It replaces `emit("snapshot")`, `emit("toast")` and `emit("spide-command")` in `state.rs`, `pi_control.rs` and `pi_bridge.rs`.
- `commands.rs` keeps its plain functions but loses the `#[tauri::command]` attribute and `State<'_>`; they take `&AppState` / `&PiBridge`.
  - The UI calls them through `Backend::run(|st| …)` on tokio's `spawn_blocking`.
  - Results come back as a `UiEvent::Reply(id, Result<Value,String>)`, or as a typed closure posted to the UI queue.
  - Errors become toasts, which is what `run()` in `App.tsx` does today.
- Remove `dev_dispatch` and the `/dev/snapshot` and `/dev/invoke` routes from `mcp.rs`.
- Startup goes in `main.rs`, in this order:
  1. Create the tokio runtime.
  2. Create `AppState` and start the MCP server.
  3. Start the watcher.
  4. Start the pi thread.
  5. Run `eframe::run_native`.
- Shut down `PiBridge` when the eframe app exits.
- **macOS menu:** `muda` keeps the default Edit/Window menus and adds File › Export… (⌘E) and Export Again (⇧⌘E). **Dialogs:** `rfd` (open, save, folder, ask/confirm).

### Terminal

- Feed `alacritty_terminal::Term` (vte parsing, grid, scrollback 10 000) **in-process** from the existing `terminal.rs` byte stream. The frame/ack/generation logic stays; the IPC polling goes away.
- Write our own egui widget, `app/src/ui/terminal_view.rs`. `egui_term` spawns its own PTY and is pinned to an old egui, so we use it only as a reference.
  - **Drawing:** a cell grid in Menlo 12, 256 colours/truecolor, bold/italic/underline/inverse, cursor, selection and copy (⌘C), paste (⌘V) and wheel scrollback.
  - **Theme:** same as xterm's: bg `#1b1e24`, fg `#e5e7eb`, cursor `#a8c7ff`, selection `#435576`.
  - **Resizing:** cols/rows come from the pane size, then call `terminal_resize`.
  - **Keys:** standard xterm encoding (arrows, Home/End, F-keys, Ctrl/Alt) plus a port of `terminal-keys.ts`. That covers modifyOtherKeys, toggled by `CSI > 4 ; n m`, which alacritty's handler doesn't expose, so a small pre-scan of the byte stream detects it.
  - Ctrl+P and Ctrl+L go to pi. Canvas shortcuts are suppressed while the terminal has focus.
- **Pane header:** status dot and text, Commands palette, Restart with confirmation, collapse, and the "pi needs attention" error banner.

### Diagram (`spide-diagram`)

| TS source | Rust module | notes |
|---|---|---|
| `canvas/visuals.ts`, `export/theme.ts` | `visuals.rs`, `theme.rs` | palette `C`, `LAYER_COLOR`, heat→colour, `attrText`/`methodText`, light export theme |
| text metrics (`textWidth` via canvas) | `text.rs` | `ab_glyph` on the same font bytes egui uses, so sizes match between screen and export |
| `canvas/layout.ts` (elkjs) | `layout.rs` | flat layout: `rust-sugiyama` 0.4 (petgraph 0.8, already in the tree). Compound layout: lay out each package separately, then the package graph in rank bands; external packages are rect-packed beside their clients; components side by side; centred at the origin. Fallback: hand-port Sugiyama with Brandes–Köpf + node sizes, or try `graphlay` |
| `canvas/physics.ts` (d3-force) | `physics.rs` | port of the d3-force subset (velocity Verlet, alpha decay, link/charge/x/y) plus the custom rect-collide, package and layer forces; pinned nodes; stops when settled |
| `canvas/route.ts`, `useRoutes.ts` | `route.rs` | 1:1 port of orthogonal routing |
| `DiagramContent`, `ClassNode`, `Edge`, `views/*` | `scene.rs`, `views/{class,package,system,usecase,sequence}.rs` | builds a `Scene` (rects, text runs, polylines, markers, hulls, halos, badges) plus hit regions (`Target`) from `Snapshot` + positions + detail/lens/focus state |
| `export/svg.ts` | `svg.rs` | `Scene` → standalone SVG (title, date, health, legend, padding, background, ghosts/diagnostics/packages toggles), then the existing `export_png`/`export_pdf`/`export_svg` in core |

### GUI (`app/src/ui/`)

| TS | Rust |
|---|---|
| `App.tsx` (state, actions, shortcuts) | `app.rs`: `SpideApp` holds the UI state from `App.tsx` (tab, preview, compare, drift, timeline, review, lens, inline edit, prune, toasts…) plus a `handle_command` port of the `/spide` dispatcher |
| `Workspace.tsx` | `workspace.rs`: left terminal / centre canvas / right inspector with draggable separators and collapse; narrow (<1120 px) right drawer; widths persisted via eframe storage (`spide.workspace.v1`) |
| `Viewport.tsx`, `Canvas.tsx` | `canvas.rs`: pan, pinch/⌘-scroll zoom, double-click to fit, drag nodes, ⌥-click pin, right-click prune, right-drag box select, drop on package, double-click rename, zoom-dependent detail, find/focus/dim |
| `Chrome.tsx` | `chrome.rs`: tabs, health ring + popover (five principle bars, grouped issues, jump-to, fix with agent), proposal bar, toasts |
| `DecisionCard`, `DecisionHistory`, `ComparePanel`, `Impact` | `decision.rs`, `compare.rs`: radio options, health delta, lazy Details with mini-diagram (a small `Scene`), consequences, Preview/Stop preview, defend footer, overrule; drafts persisted |
| `ProposalReview`, `ProposalOperationEditor` | `proposal.rs`: one plan at a time, before/after, op forms + advanced JSON, Save/Accept/Reject, revalidation, local drafts |
| `BriefPanel`, `RulesSection`, `ChangeSection` | `brief.rs` |
| `DriftPanel`, `Timeline`, `CodeView`, `HoverCard`, `HoverLayer`, `MemberEvidence`, `FeedbackPopover`, `InlineEdit`, `DetailSlider`, `ExportDialog`, `SpideCommandPalette` | matching `*.rs` modules, using egui `Area`, `Window` and popups |
| `state/{hover,proposal-focus,proposal-review,insights,decision-drafts,command-catalog}.ts` | `app/src/ui/state/*.rs`: pure logic, unit-tested |
| `state/types.ts` | dropped: the GUI uses the `spide-core` / `app` types directly |
| `PerfOverlay`, `perf.ts`, dev query params | dropped |

- **Look:** use `egui::Visuals` with the CSS palette (`--bg #1d1e22`, panel `#25272d`, line `#353943`, accent `#7aa2ff`, ghost `#5eead4`…) and corner radii and spacing close to `styles.css`.
- **Fonts:** load Helvetica Neue and Menlo from `/System/Library/Fonts` at runtime, falling back to egui's defaults.
- **Animations:** node enter (520 ms overshoot scale), ghost breathe (5×), halo pulse (4×), strained-edge dash march (7×) and preview transitions. They use `ctx.animate_*` and time-based easing and keep the same finite repeat counts, so the app repaints only while something animates.
- **Keyboard:** ⌘1–6, ⌘Z / ⇧⌘Z, ⌘B, ⌘T, ⌘E / ⇧⌘E and Esc behave as today.

## Files

- **Delete:**
  - `src/`, `index.html`, `vite.config.ts`, `tsconfig.json`, `dist/`, `node_modules/`, `package-lock.json`
  - Playwright scripts: `scripts/{browser-smoke,terminal-browser-smoke,ui-audit-smoke,ghost-focus-smoke,export-routing-smoke}.mjs`, plus `scripts/perf_probe.py`
  - `src-tauri/{tauri.conf.json,build.rs,gen}`
  - `tests/*.test.ts` except `pi-extension.test.ts`
- **Move:** `src-tauri/` → `app/` (`git mv`). Change `main.rs`, `commands.rs`, `state.rs`, `mcp.rs`, `pi_bridge.rs`, `pi_control.rs`, `terminal.rs` (subscriber API) and `Cargo.toml`.
- **New:** `crates/spide-diagram/**` and `app/src/ui/**`.
- **Keep:**
  - `agent/spide-terminal.ts`, `tests/pi-extension.test.ts` and `scripts/terminal_smoke.py`. These belong to the pi extension, which must stay TS.
  - `package.json`, cut down to no dependencies and only `"test": "node --test tests/pi-extension.test.ts"`.
- **Update:**
  - Root `Cargo.toml`: members, plus a `[profile.dev.package.spide-diagram] opt-level = 2`.
  - `README.md`: Run, Development and Architecture sections.
  - `.gitignore`.

## Reuse

- `spide-core`: `Snapshot` builders, `metrics`, `derive`, `detail`, `diff`, `home`, `overlap`, `png.rs`/`pdf.rs` (SVG→PNG/PDF), `plantuml`, `adr`, `archunit`.
- `app/` (formerly `src-tauri/`), mostly unchanged: `mcp.rs`, `pi_bridge.rs`, `pi_control.rs`, `terminal.rs` (PTY, frames, generation), `watcher.rs`, `history.rs`, `branches.rs`, `prompts.rs`, and all the logic in `commands.rs`.
- `petgraph` 0.8, `resvg`, `svg2pdf` (already dependencies).
- New crates:
  - `eframe`/`egui`, a current version pinned at implementation time; `egui_extras`
  - `egui_kittest` (dev)
  - `alacritty_terminal`
  - `rust-sugiyama`
  - `rfd`
  - `muda`
  - `ab_glyph`

## Steps

- [ ] **0. Commit the current work:** `git add -A && git commit -m "…"` on the uncommitted changes (README, agent skill, core, commands, main, smoke scripts). Then tag a baseline `pre-egui` and take reference screenshots of the TS app with `examples/library.json` and `examples/rest-extraction.json`, saved to `.local/egui-ref/`.
- [ ] **1. Restructure:** `git mv src-tauri app`, add `crates/spide-diagram`, update the workspace, remove Tauri, introduce the `Events` sink and the `Backend` call helper, and delete the dev routes. A minimal eframe window shows the snapshot's rev and health. `cargo test` passes.
- [ ] **2. `spide-diagram` core:** visuals/theme/text, then port `route.rs` and `physics.rs`, then `layout.rs` on `rust-sugiyama`. Check the layout on the examples; if it is poor, hand-port the layered layout.
- [ ] **3. Scene + class view:** class nodes at every detail level (0–4), with folding, role names, `↗ Home` markers, inherited-attribute rules and zoom-based detail. Edge notation (association, directed, inheritance, realization, dependency, composition, aggregation), cycles, violations, ghosts and removals, package hulls, heat glow, pins, overruled and pruned states, the external strip, lenses (concerns, overlap), implicit dependencies.
- [ ] **4. Canvas interaction:** viewport, drag, physics loop, tidy, pin, prune (click and box), feedback popover, restore, inline rename, drag into a package, hover cards (purpose, members, metrics, why it's here), find/focus/dim, stress footprint, layout persistence via `set_layout`.
- [ ] **5. Other views:** domain, package, system (C4 context/container), use case, sequence (scenario fit scores, regenerate).
- [ ] **6. Workspace + chrome:** panes, drawer, tabs, health ring and popover, toasts, proposal bar, detail slider, keyboard shortcuts, menu, fonts and visuals.
- [ ] **7. Terminal widget:** connect it to `terminal.rs`, add key encoding and modifyOtherKeys, resizing, selection and clipboard, restart and status, the command palette, and the `spide-command` bridge into `handle_command` with dedup (256 seen).
- [ ] **8. Panels:**
  - decisions: cards, compare, history, preview, defend, overrule
  - proposals: review and op editor with drafts
  - brief, rules and change scenarios
  - drift
  - timeline: preview, restore, fork
  - branch compare
  - code view
- [ ] **9. Export:** the `Scene` → SVG writer and export dialog (settings persisted, Export Again), PNG/PDF/SVG through core, PlantUML/ADR/ArchUnit, reveal.
- [ ] **10. Tests:**
  - Port `route`, `hover`, `proposal-focus`, `proposal-review`, `commands`, `terminal-keys`, `export-theme`, `member-diagnostics` and `decision-drafts` to Rust `#[test]`.
  - Add `egui_kittest` UI tests that replace the Playwright smokes:
    - startup and tabs
    - ghost focus and proposal review
    - the UI audit: panes, drawer, no overlapping panels
    - export routing: routes are orthogonal and no box is crossed, exported files are non-empty
    - the terminal pane, with a fake PTY
  - Add snapshot images for each view on `examples/library.json`.
- [ ] **11. Remove the TS frontend:** delete the files listed above, cut down `package.json`, update the README (Run: `cargo run -p spide`; release bundle via `cargo bundle` or a documented `.app` script) and `.gitignore`. Commit.

## Verification

- `cargo test --workspace` covers the core tests, the ported TS tests, the `spide-diagram` route, layout and SVG tests, and the `egui_kittest` UI and snapshot tests.
- `node --test tests/pi-extension.test.ts` and `python3 scripts/terminal_smoke.py` still pass.
- **Manual:** `SPIDE_PROJECT=<copy of examples/library.json> cargo run -p spide`.
  - Compare each tab side by side against the `.local/egui-ref/` screenshots.
  - Walk through the README's "Using it" section:
    - brief → decide → defend → overrule
    - ghosts → review → accept
    - prune and restore
    - timeline and branches
    - import and drift
    - export (PNG, PDF, SVG, PlantUML)
    - all `/spide` commands
    - all keyboard shortcuts
  - Check that pi starts and takes input (Ctrl+P, Ctrl+L, Shift+Tab, Esc, multiline), resizes, and restarts.
- **Performance:** the CPU is idle when nothing animates or settles, and pan/zoom stays smooth on `rest-extraction.json`.
