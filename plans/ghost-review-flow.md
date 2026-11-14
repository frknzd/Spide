# Ghost proposal review and decision integration

## Context
Ghost proposals can currently be located one at a time, but the proposal bar only offers Accept all / Reject all. Details live in a transient hover panel. Decision review is a separate side panel, so the connection between a proposed change and its design rationale is unclear.

## Approach
- Provide a dedicated persistent Ghost proposals workspace/inspector with a proposal queue, selected-plan change list, editable operations, impact, and associated decision context alongside review.
- Reuse the main canvas; focusing a plan spotlights its footprint and selecting a change reveals detailed before/after information. Do not make another diagram editor.
- Allow adding, updating, and deleting planned operations without changing the accepted design. Distinguish “remove this planned change” from “plan deletion of this element”.
- Accept or reject only the selected proposal, then move to the next. Remove bulk accept/reject from UI and command discovery/execution.
- Decision context is informational, not an extra defence gate. Standalone proposals explicitly show no associated decision.

## Files to modify
- `src/ui/Chrome.tsx`: existing proposal bar and detail rendering (potentially extract dedicated review UI).
- `src/App.tsx`: shared selection, review actions, and decision-panel coordination.
- `src/ui/DecisionCard.tsx`, `src/ui/DecisionHistory.tsx`: decision/proposal navigation as needed.
- New `src/ui/ProposalReview.tsx`, `src/ui/ProposalOperationEditor.tsx`, and `src/state/proposal-review.ts`: dedicated review, structured/advanced editors, draft and selection helpers.
- `src/ui/Workspace.tsx`, `src/styles.css`: Proposals/Decisions inspector modes, persistent spotlight, responsive layout and accessible controls.
- `src/state/api.ts`, `src/state/types.ts`, `src/state/proposal-focus.ts`: draft preview/save APIs, review/provenance data and complete change summaries.
- `crates/spide-core/src/store.rs`, `model.rs`, `tools.rs`: transactional proposal replacement/preview, explicit optional decision association, validation and tool support.
- `src-tauri/src/commands.rs`, `main.rs`, `state.rs`: native/dev command registration and review data.
- `src/state/command-catalog.ts`, `src/App.tsx`, `agent/spide-skill.md`, `README.md`: remove bulk review affordances and document the new flow; update any additional bulk references found during implementation.
- `crates/spide-core/tests/store.rs`, new proposal review tests under `tests/`, `scripts/ghost-focus-smoke.mjs`, and affected UI smoke scripts.

## Reuse
- `src/state/proposal-focus.ts`: proposal summaries and shared ghost spotlight.
- `src/ui/DecisionCard.tsx`: existing decision navigation, previews, and defence submission.
- `src/ui/DecisionHistory.tsx`: decision rationale and examiner history.
- `src/ui/Impact.tsx`: existing impact presentation; snapshot already exposes `proposal_impacts` and decision option previews.
- `crates/spide-core/src/store.rs`: `mutate` for atomic undoable saves, `accept_proposal` / `reject_proposal` for individual resolution, and `preview` as the basis for isolated draft previews.
- `crates/spide-core/src/ops.rs`: typed operation deserialization and `apply_all` validation; do not create a parallel operation execution engine.
- `src/ui/Workspace.tsx`: existing resizable inspector and narrow-screen drawer.
- `src/ui/InlineEdit.tsx`: reuse field interaction conventions, but not its immediate accepted-design mutation path.

## Steps
- [x] Trace individual proposal acceptance/rejection and initial provenance: individual APIs already exist; proposals contain ordered typed ops but no decision link. `Store::view()` overlays proposals in order and silently skips invalid ones. `apply_user` commits immediately and must NOT be used for plan edits.
- [x] Trace decision production: accepted defences currently commit directly, not through ghost proposals. Preserve that lifecycle; do not manufacture associations or turn decisions into proposals automatically.
- [x] Agree on dedicated review, editable plans, associated decision context, no bulk actions, and initial structured-editor scope.
- [x] Add proposal review backend primitives. Return accepted baseline plus ordered before/after operation results, errors, impact, and dependency context for one selected proposal/draft. Replay against the accepted design plus valid preceding proposals, never the already-overlaid snapshot view. Show when an earlier proposal is required; block standalone acceptance until prerequisites are accepted. Surface invalid/skipped proposals rather than presenting them as no changes.
- [x] Implement atomic `update_proposal` with proposal ID, expected revision, ordered ops, rationale and optional decision association. Validate typed ops, nonempty plans, decision IDs and affected downstream proposals on a clone before saving through `mutate`; reject stale/invalid saves without mutating accepted state or silently dropping dependent plans. Reject empty saves with guidance to reject the proposal instead. Preview is non-mutating and creates no history entries.
- [x] Add optional, serde-defaulted decision/option provenance to Proposal, mirrored in TypeScript and supported by proposal creation tools without breaking existing callers/files. Existing unlinked plans remain unlinked; offer explicit association with an existing decision in review. Display question, option, status, rationale and defence/verdict where present; edits are user amendments, not examiner-approved alternatives, and must not overwrite decision records or claim a fresh verdict.
- [x] Replace the hover details/bulk dock with a compact “Review proposals (N)” entry point and dedicated Proposals inspector mode alongside Decisions. Proposal list and previous/next controls select one stable ID. Opening a proposal from the canvas opens review, never accepts it: the current `onClickTarget` auto-accept must be removed. Opening review reveals the inspector even when collapsed/on a narrow screen; decision previews/history cannot silently override active review.
- [x] Render all operations with Add / Update / Delete labels, target, grouped field-level before/after differences, full expandable values, rationale, source, impact and validation/dependency notices. Include renames, metadata-only updates and non-class operations; do not rely solely on ghost-status footprint items, which miss some updates. Reuse the main canvas, spotlight selected-plan changes and anchors, and focus selected diagram targets; explain non-diagram changes rather than forcing an unrelated class view. Keep update/removal highlights visible even where status tagging is insufficient.
- [x] Add structured operation forms for classes, attributes, methods, relations and packages (including rename/move/layer operations). Offer validated advanced JSON operation editing for all other families and fields, preserving unknown/unexposed values on structured edits. Allow append/edit/remove planned operations in order; distinguish removing an operation from adding a deletion operation. Render untrusted text as text, never HTML.
- [x] Maintain a local dirty draft with debounced backend preview and explicit Save plan / Discard edits. Preview updates the same canvas without accepting changes; ignore out-of-order preview results. Plan changes, canvas edit gestures and drag/delete actions while reviewing must modify the selected draft or be disabled, never call `apply_user_ops`/prune on accepted elements. Block accept until the draft is valid and saved; keep Reject explicit and confirm dirty edits would be discarded. Protect dirty drafts on selection/inspector/project/branch changes and revision conflicts; never silently overwrite agent updates.
- [x] Accept/reject only the selected saved plan with pending/error guards. Keep selection stable through unrelated snapshots; after successful resolution select the next surviving plan (or previous at the end), and show an empty state when finished. Undo/redo restores saved plan changes and resolution. If rejection breaks later plans, show their explicit dependency errors and disable acceptance until repaired; do not cascade-delete them.
- [x] Remove Accept all / Reject all buttons and bulk `/accept` / `/reject` behavior from user-facing commands/help. Repurpose commands to require a proposal ID (or an unambiguous active saved selection), never default to every plan. Internal compatibility APIs may remain unused. Audit hover cards, keyboard shortcuts and canvas routes so focus/selection cannot accept implicitly.
- [x] Add tests and update feature/help documentation and relevant smoke assertions together with the implementation.

## Verification
- Run `npm run typecheck`, `npm test`, `npm run build`, and `cargo test -p spide-core`; run affected browser smoke scripts with the existing mocked dev-snapshot setup. Update fixture proposals to contain real operations rather than empty synthetic plans.
- Core tests: preview/save do not commit accepted design; add/update/delete/rename ops; atomic validation failures; stale revisions; ordered dependencies and downstream conflicts; accept blocking; rejection-induced errors; optional provenance loading; undo/redo; native/dev command payloads.
- Frontend tests: stable ID selection, next-plan behavior, detailed update diffs, structured/advanced edit roundtrips, preserved hidden fields, dirty guards, stale preview suppression and selected-only commands.
- Browser checks: ghost click opens review with zero mutations; focus persists while interacting with canvas/forms; add a method, edit a relation, remove an operation and plan an element deletion; preview accepted baseline vs plan; save, accept one and verify another remains; reject one; undo/redo; associated decision/no-association display; non-class and invalid/dependent plans.
- Verify no bulk actions or implicit click-to-accept remain, preview mode never calls accepted-design mutation APIs, and save/accept failures keep the draft/review visible.
- Check keyboard navigation, focus restoration, labelled forms, inline errors, scrollable large plans, narrow-screen drawer, collapsed inspector opening, and switching Decisions/Proposals without overlap or losing drafts.
- Keep task-specific verification/help documentation current. Follow the workspace test-plan update requirement during implementation: read the external DOCX, check for concurrent changes before writing, back it up under `.local/testplan/`, preserve XML formatting and add an edit-history row. Clarify the Spide-specific scope rather than changing unrelated REST Extraction test cases. No DOCX changes during planning.

## Confirmed requirements
- Dedicated editable proposal review, sharing the existing canvas with richer focused details.
- Additions, updates, and deletions within each plan.
- Associated decisions alongside review; no additional defence gate.
- No Accept all / Reject all.

## Confirmed editor scope
Structured forms for classes, members, relations and packages; validated advanced operation editor for less-common changes. No further product choices are blocking implementation.
