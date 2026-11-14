# Test coverage metric

## Context
- Add a metric called **test coverage** to Spide.
- Spide calculates design metrics in Rust (`crates/spide-core`) and renders them through TypeScript snapshot types and React UI.
- Existing health combines five design principles. Test coverage will be a separate metric and will not affect health (confirmed by user).

## Approach
- Display test coverage separately from design health; leave health calculations and weights unchanged.
- Coverage means actual code execution coverage from test reports (confirmed), not modelled scenario coverage.
- Proposed initial format: JaCoCo XML, matching Spide's existing Java source import; confirm with user before finalising.
- Import existing reports only; do not execute project tests automatically.
- Reuse the existing metrics and snapshot pipeline rather than create a parallel metric system.

## Files to modify
- `crates/spide-import/src/`: report parser (new module; format pending).
- `src-tauri/src/commands.rs`: import/persistence integration alongside existing code import.
- Frontend snapshot/API types and separate coverage display; exact paths pending source tracing.

## Reuse
- `src/state/types.ts`: frontend metrics/snapshot types.
- `src/ui/Chrome.tsx`: existing health/metrics presentation.
- `src/ui/HoverCard.tsx`: per-class metric presentation.
- `src-tauri/src/commands.rs`: `import_code` persists imported Java reality in `.spide/reality.json`; coverage should similarly remain project-local and separate from design health.
- `crates/spide-import`: existing Java import boundary; no coverage parser has been selected yet.

## Steps
- [ ] Confirm coverage definition, source, scope, and presentation.
- [ ] Trace the relevant backend computation/import and UI paths.
- [ ] Implement metric with explicit missing-data handling and regression tests.
- [ ] Document the behaviour and update applicable verification records.

## Verification
- Targeted Rust tests and frontend tests/typecheck.
- Manual checks for coverage values and missing coverage data.

## Open questions
- Which report format should the first version support? Recommended: JaCoCo XML for Java.
- Proposed display: project-wide line coverage beside health, with class-level coverage in class details. Confirm scope.
- Health impact is resolved: none. Coverage semantics are resolved: actual code coverage.
