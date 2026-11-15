# Spide diagram-first workflow redesign

## Context
The Decisions experience is clunky; redesign the review workflow rather than just restyling controls. Spide is a native Rust/egui application with a class canvas and interactive pi terminal.

Initial findings:
- The current unified review lives in `app/src/ui/proposal.rs`: plan picker/arrows, question, alternative radios with separate details, rationale, draft metadata, validation, baseline toggle, impact and operation editing compete within one inspector.
- `app/src/ui/decisions.rs` includes legacy defence UI as well as reusable preview/impact/comparison rendering. Do not mistake the legacy panel for the current workflow.
- Approval, saved plan amendments and revision requests already have separate semantics; preserve explicit approval and draft safety.

## Approach
Confirmed priorities: fewer clicks, immediately visible important information, easier navigation. Review should happen primarily on the diagram. Reconsider the surrounding brief → design → review → implementation flow too.

Recommended direction: diagram-first review rather than a separate review destination. A compact, directly selectable decision queue accompanies the canvas; selecting a decision immediately previews its saved plan and focuses affected elements. A compact review surface exposes the question, summary, alternatives, key consequences and approval without expanding details. Clicking affected elements reveals their before/after changes. Keep structured operation editing in an explicit advanced mode and keep pi available for refinement.

Surrounding flow should be contextual rather than a mandatory wizard: brief on empty projects; design and review together; after approval offer behaviour checks or separately authorized source implementation when relevant. Preserve free navigation and pure-design use.

Further findings:
- `app/src/ui/app.rs` currently splits Decisions, Source and a History dropdown in the right inspector; proposal focus switches to the Class tab. Canvas already receives a review footprint.
- `app/src/ui/state/proposal_review.rs` already provides stable pending selection, field diffs, operation targets and persisted plan drafts. Reuse these rather than introducing a second review model.

User confirmed: automatically advance after approval (never auto-approve); propose related small changes together as one coherent decision; select diagram elements and ask the agent for refinements, with manual operation editing as an advanced fallback.

### Interaction contract
- Replace the primary Decisions form with a compact review rail beside the diagram: directly selectable titled queue, active question/context, selected option summary, visible pros/cons and change counts, health delta and blocking warnings. Long content scrolls; primary actions remain reachable. IDs and raw operations are secondary details.
- Single-option decisions open already selected. Alternatives are labelled selectors with short tradeoffs; selecting one updates the diagram without a second Preview click. Keep the existing side-by-side comparison as an optional deeper view.
- Show added/changed/removed elements distinctly, including removed elements in the review overlay. Provide a clearly labelled Before/Proposed toggle, fit-to-changes control and legend. Keep camera/layout stable when toggling alternatives; fit on deliberate decision navigation, not every refresh.
- Clicking affected classes, members or relations shows readable field changes and adds/removes refinement targets. Selected targets appear as removable chips beside one refinement input. Request changes sends decision/option IDs, selected element identities, snapshot revision and guidance through the existing private agent bridge. No edits happen merely from selecting or requesting. Non-diagram changes (brief, rules-related evidence, containers, etc.) remain visible in a compact change list with appropriate view links/fallback details.
- Keep approval atomic per decision. Group related operations at proposal creation through agent guidance, not by silently merging existing proposals or approving unrelated decisions together. Preserve prerequisite ordering and show a direct route to blocked prerequisites.
- After successful approval, advance to the next pending decision using stable queue order. New arrivals do not steal selection. At the end show a clear review-complete state and contextual Check behaviour / Implement in source actions; source editing still requires its existing separate authorization.
- Preserve drafts across navigation and workspace changes using existing guards/persistence. Unsaved advanced edits require save/discard before option changes or approval; never silently approve a different saved plan. Explain disabled actions inline. Request failures preserve targets and guidance for retry; clear them after successful submission or explicit discard, and scope them to the decision/workspace.
- Keep pi visible and editable; do not automatically focus it or inject draft text. Diagram refinement is an explicit structured request. Keyboard shortcuts only act when the canvas/review has focus, never while typing in pi or text fields.
- On narrow windows use a compact resizable review strip/drawer without covering pi; retain title, selected alternative and primary actions, and scroll longer evidence. Avoid stacking multiple competing overlays.

### Surrounding workflow
- Empty project: a lightweight brief/start prompt with direct access to the existing brief editor and skip path; no mandatory wizard.
- Designing: canvas and pi remain the centre, with pending-review count/queue available without changing tabs. Arrival of proposals indicates work to review without interrupting typing or camera navigation.
- Reviewing: use the diagram workspace above. History remains separately accessible as read-only evidence, not mixed into pending navigation.
- After review: offer existing scenario/Sequence checks and, only for attached repositories, source implementation. Keep Source configuration, verification and explicit permissions intact; improve entry points rather than rebuild those subsystems.
- Scope excludes new approval semantics, schema migrations, automatic source edits and removal of legacy historical evidence.

## Files to modify
- `app/src/ui/proposal.rs` — current review inspector.
- `app/src/ui/decisions.rs` — reuse/adapt comparison and impact views as needed.
- `app/src/ui/app.rs` — diagram-first review orchestration and surrounding navigation.
- `app/src/ui/state/proposal_review.rs` — reuse selection/diff/footprint helpers; add only needed review state.
- `app/src/ui/chrome.rs` — canvas review entry and queue controls.
- `app/src/ui/canvas.rs` — review hit targets, selection feedback and before/after interaction.
- `app/src/ui/tests.rs` — headless UI and visual regression coverage.
- `app/src/commands.rs` — extend revision request context while retaining revision checks and private bridge delivery.
- `agent/spide-skill.md` — coherent proposal grouping and targeted refinement guidance.
- `README.md` — document revised workflow.

## Reuse
- Existing option previews, `mini_preview` and `impact_line` in `app/src/ui/decisions.rs`.
- Existing proposal draft persistence, conflict revalidation, option selection and approval commands invoked by `app/src/ui/proposal.rs`.
- `next_proposal_id`, `field_diffs`, `operation_target`, `review_footprint` and `PlanDrafts` in `app/src/ui/state/proposal_review.rs`.
- `refresh_proposal_canvas`, `focus_canvas` and review enter/leave guards in `app/src/ui/app.rs`.
- `revise_decision` in `app/src/commands.rs` for explicit, non-committing refinement requests.
- `harness`, `steps`, `shot` in `app/src/ui/tests.rs` for egui_kittest interaction and screenshots.

## Steps
- [x] Agree diagram-first interaction, surrounding-flow scope and key navigation choices.
- [x] Identify current inspector, canvas integration, draft helpers, revision command and UI harness.
- [x] Build a diagram-first review rail and directly selectable queue; separate advanced editing from default review.
- [x] Connect immediate option preview, stable before/proposed views, visible consequences and element-level diffs.
- [x] Add scoped refinement selection/input and extend structured revision requests; cover offline/busy/stale cases.
- [x] Connect successful approval to stable next-decision selection and contextual completion actions.
- [x] Improve empty-project brief entry, pending indicators, History and existing behaviour/source handoffs.
- [x] Update agent grouping guidance, README and applicable test documentation; verify draft and authorization guarantees.

## Verification
- Exercise single-option and multi-option reviews, multiple pending decisions, comparison and revision requests.
- Check draft preservation, conflicts, prerequisite errors, approval/dismissal and undo.
- Check narrow windows, long text, scrolling and terminal accessibility.
- Run `cargo test -p spide-core` (including `crates/spide-core/tests/decision_approval.rs`) and `cargo test -p spide`; use the existing headless UI harness and `cargo run -p spide` for visual/manual checks. No tests/builds are run during this planning phase.
- Add UI tests for one-click queue/alternative selection, immediate preview without mutation, visible summary/actions, selected-target revision payload, successful next-decision advancement, new-arrival selection stability, final completion state, and preservation of unsaved drafts. Verify disconnected/busy agent errors and stale revision rejection.
- Capture wide/narrow layouts with long questions, many options, 100 operations, removals and non-class changes. Confirm terminal focus/shortcuts are unaffected, and Before/Proposed switching does not cause layout jumps.
- Acceptance: entering review shows the selected proposal on the diagram immediately; switching decision/alternative takes one click; summary, tradeoffs and critical warnings require no disclosure clicks; approval advances only after success; source authorization remains separate.
- Update the required shared test plan during implementation: `~/Library/CloudStorage/OneDrive-KULeuven/REST Extraction Extension Test Plan.docx`. Read/check its current hash before writing, back up under `.local/testplan/`, preserve formatting via XML edits and add author/version/edit/date history. Coordinate relevant Spide coverage with the document's existing structure rather than altering unrelated REST tests. No DOCX changes during planning.
