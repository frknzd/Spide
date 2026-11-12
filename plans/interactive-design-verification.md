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

## Edit history
| Author | Version | Edit | Date |
|---|---|---|---|
| AI coding assistant | 1 | Record automated/dev checks and outstanding manual/regression checks | 2026-11-12 |
