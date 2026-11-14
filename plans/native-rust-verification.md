# Native Rust GUI verification

## Automated checks

- `cargo test --workspace`: passes, including ported state/routing/diagnostics tests, both example diagrams, six-tab headless renders, hover/prune/rename, pin/drag/layout persistence, shortcuts, narrow proposal drawer, populated use-case/sequence/C4 scenes, compare/timeline/code panels, fake PTY rendering and real SVG/PNG/PDF/PlantUML files.
- `cargo test -p spide macos_view_snapshots -- --ignored`: passes against six checked-in library baselines. Explicit opt-in because Helvetica Neue/Menlo/SF Pro and rasterization are platform-specific. Update intentionally with `UPDATE_SNAPSHOTS=1`.
- `node --test tests/pi-extension.test.ts`: passes; no npm dependencies.
- `python3 scripts/terminal_smoke.py`: passes against real pi. Authenticated reconnect after `/reload`, tool inventory, Ctrl+P model selection and modifyOtherKeys negotiation observed. No model request.
- `cargo build -p spide`: passes.
- `sh -n scripts/bundle-macos.sh`: passes. The unsigned release bundle script is documented; signing/notarization is not provided.

## Unified decisions, source tracking and long-method diagnostics (2026-11-14)

- Automated: decision migration/approval/stale/undo/ADR tests; collapsed 100-operation UI approval with empty notes; source panel accessibility/screenshot; member hover edge placement; scoped Java import (same simple names, overloads, nested types, exclusions, traversal/symlink refusal); signature drift, explicit bindings and three-way conflicts; watcher edit/deletion/detach; implementation tasks (single dispatch, stale source/design/session invalidation, preserving pre-existing edits, cancellation, validation failure, verification); length thresholds, multiline comments, nested bodies and current-mapped-only health evidence.
- Pure-design projects create no source attachment. Tests use temporary Java repositories and fake agent dispatch; they made no model requests and did not modify `zap-extensions`.
- Manual still required in a visible app: attach a disposable `zap-extensions` clone/module, inspect accurate drift for its real Java, request a revision from notes, approve implementation with the real pi session, check native permission prompts, cancel during agent work, inspect resulting diff and residual drift, and verify OS repository dialog/relinking. Real `zap-extensions` writes require explicit task approval and its repository instructions/Java 17 wrapper.

## Visual evidence

- Pre-migration references: `.local/egui-ref/` (local, ignored).
- Native diagnostic images: `target/ui-shots/` (generated, ignored).
- Reviewed library baselines: `app/tests/snapshots/` (tracked).
- Comparison prompted fixes to initial fit, package wrapping, fonts/weights/optical sizing, canvas background, banners and panel labels.

## Remaining manual checks / limits

Headless tests do not replace a human walkthrough in a visible native window. Verify unified decision editing/notes/approval/revision requests, source attachment and separately approved implementation, branch/history restoration, Java import/drift, OS dialogs, all pi editor controls/selection/clipboard/resize/restart, and macOS menu shortcuts with a real project before release. Full end-to-end model conversations were not run automatically. Interactive permissions must remain the user's choice.

Native idle CPU and large-example pan/zoom frame-rate require measurement in a release build. Snapshot success does not establish smoothness. Linux native font fallback/windowing behavior is not validated on this macOS machine. `rustfmt` is unavailable in the installed Rust toolchain; formatting verification could not run. Existing dead-code warnings are non-fatal.
