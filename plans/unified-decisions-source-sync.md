# Unified decisions and source-aware design

## Context
Improve member hover readability, eliminate repetitive scrolling during review, unify proposals and decisions, and make ongoing source/design synchronization a first-class workflow without losing pure-design use.

## Approach
### Unified decision review
- One Decisions inspector/queue/history replaces the separate Proposals destination. Small changes have one option; substantial improvements have alternatives. Each option contains an editable operation plan with impact, explicit preview and validation.
- Approval applies the selected saved plan atomically, without an examiner verdict or required defence. Optional **Notes / refine this change** text records intent. **Request revision** sends those notes to the agent without applying anything; **Approve design** applies exactly the visible plan and records notes, never interprets notes as invisible additional edits. Custom alternatives require the agent to return a concrete reviewable plan before approval.
- Keep existing safe draft editing, expected-revision checks, dependency errors, undo, provenance and selected-only approval. Fold linked proposals into their associated decision; adapt unlinked legacy proposals as single-option decisions, preserving IDs/aliases, ordering and origin. Historical defences/verdicts remain readable; old pending/evaluating records become reviewable, never auto-approved. Persist this conversion once, with versioned migration tests.
- All new producers (MCP, external design-file edits, import adoption, branch merge/history restore, generated suggestions) feed this workflow. Compatibility `propose_ops` creates a single-option decision; legacy examiner callbacks cannot commit changes or revive the gate.
- Collapse operation details, impact and rationale independently, with compact target/change summaries. Remember expansion by workspace/branch/decision/option/operation and preserve scroll/drafts across refresh. Place Approve design / Request revision / Dismiss in a fixed footer outside the scrolling body; invalid or stale plans remain visible with approval disabled.

### Member hovers
- Increase useful method/attribute card height using available canvas bounds rather than the current 70% window cap; short content still shrinks naturally. Preserve padding, placement, pinning and scrolling, including near canvas edges and in narrow layouts.

### Function-length heuristic and health
- Extend the existing `long_method` check rather than add a duplicate warning. Retain the current default of **more than 30 nonblank, non-comment body lines**, configurable through architecture-rule thresholds. Present this as a practical heuristic, not a universal correctness rule; explain the counting convention and recommend Extract Method along responsibility boundaries.
- Ensure each measured over-limit method contributes one cohesion issue to the existing health calculation. Source-linked design health must include matched implementation length findings, not only the separate code-health view; deduplicate the same finding when combining evidence. Pure-design methods without imported bodies have unknown length and receive no penalty. Unmapped/code-only methods affect code health only.
- Clearly indicate the affected method on the canvas using existing member diagnostic marks, and show measured length, configured threshold, source location, health principle and suggested remedy in its hover and health issue list. Keep flagged methods visible at design detail level and allow source navigation / fix-with-agent through the unified decision flow.
- Refresh length evidence after source edits and threshold changes; shortening a method removes its warning and restores the associated health contribution. Invalid/stale source evidence must be labelled and must not silently masquerade as a current measurement.

### Optional Java source attachment
- Attach one existing repository per workspace, choosing one or more production source roots/modules and exclusions; do not import all of `zap-extensions`. Show scope and file counts before initial import. Persist machine-local paths/configuration and snapshot metadata under `.spide/`; keep symbol bindings compatible with portable design files and offer relinking when paths are missing. Legacy `code_dir` imports can become attachments after confirmation.
- Add a Source section with attach/reconfigure/detach, refresh, source status, drift summary, navigation and adoption actions. Pure-design mode never needs an attachment, Git or Java. Detaching stops tracking, retains design and historical implementation records, and clearly marks old source snapshots inactive.
- Reuse Java parsing/body facts, adding fully qualified type identities, relative source paths and member signatures/source ranges. Map design elements to symbols explicitly when ambiguous; never guess same-name matches. Enhance mapped member drift for changed types/signatures and overloads; offer selected additions, updates and removals as decisions. Do not blindly replace the design or erase responsibilities/rationale. Unmapped/out-of-scope elements remain explicit rather than reported as implemented.
- A separate scoped recursive watcher debounces source events and refreshes asynchronously. Use full scoped imports initially, with coalescing and generation cancellation; avoid a new incremental parser until needed. Ignore build/VCS/excluded paths, handle create/edit/delete/rename, prevent symlink escape, stop on detach/project changes and provide manual refresh. Publish snapshot plus metadata atomically only after workspace/scope generation checks; parse failures show partial/stale status, not false synchronization. Do not request model analysis on every filesystem event.
- Three-way reconciliation uses the last acknowledged design/source pair to distinguish source-only, design-only and conflicting changes. Show conflicts for user resolution and retain unresolved differences. A successful source refresh is observation, not acknowledgement or approval.

### Agent-assisted implementation, separately approved
- **Implement in source** is available after design approval. Prepare a reviewable implementation request showing decision/approved ops, bound symbols, permitted file scope, current drift/conflicts, source fingerprints and intended validation commands. The user explicitly authorizes execution; approval of the diagram alone never edits source.
- Send this context to the existing pi session through its private structured prompt bridge, not a second agent or terminal text injection. Expose attachment, scope, symbol locations, drift and implementation status through Spide MCP/context so ordinary agent turns and revision requests share the same understanding. Keep pi's project/session location stable and use explicit attached-repository paths.
- Agent reads the attached repository's instructions before editing, preserves pre-existing work, and requests renewed scope approval for necessary files outside selected roots (e.g. tests/build files). Use the existing native pi coding tools and permission controls; integration prompts are workflow guidance, not a security sandbox. Do not claim to restrict arbitrary user shell commands. Never auto-commit, reset, stash or change branches.
- Track a durable implementation task linked to the approved decision and exact source/design revisions: awaiting approval, running, verifying, completed, failed, cancelled/interrupted or stale. Revalidate fingerprints before execution; changed design/scope/files require renewed approval. Persist status and changed-file diff from the pre-run baseline (not all repository dirt), test results and residual drift. Cancel/interruption stops further scheduled work, reports any already-written edits and never blindly rolls back user files.
- After execution, refresh source, run only approved validation, and verify the intended mapped structural delta. Report remaining drift and unverified semantic behaviour; an agent's success message alone cannot mark the task synchronized. Source writes are not part of diagram undo: undo/branch changes mark linked tasks stale and offer new reconciliation, not filesystem rollback.

## Findings / reuse
- Native Rust/egui application, not a web UI.
- `crates/spide-core` already owns proposals, decisions, atomic operations, provenance and undo.
- `crates/spide-import` already parses Java; `.spide/reality.json` stores the imported snapshot, and drift/adopt-code workflows exist.
- Pi can already edit source using its existing tools; diagram mutations use MCP review flows.
- Existing review plans document draft safety and explicit preview/accept semantics; preserve those guarantees.
- `app/src/ui/hover_card.rs:697–714` uses `place_card` and a viewport-limited scroll area; inspect `app/src/ui/state/hover.rs` placement before increasing member height.
- `app/src/ui/proposal.rs` already provides structured operation editing and safe local drafts; `app/src/ui/decisions.rs` provides option previews, impact and comparison. Compose these into one workflow, not a second editor.
- `Store::propose_decision`, `submit_defense`, `evaluate_defense` currently encode the examiner gate; replace the default approval path rather than merely relabelling its button.
- `app/src/commands.rs` already has `import_code`, `load_reality`, drift reporting and adopt operations. `app/src/watcher.rs` only watches `design.json`, so source tracking needs a separate lifecycle.
- Java importer recursively skips build/VCS/test directories but does not yet offer selected module scope. Large external repositories need explicit scope and asynchronous import.
- `crates/spide-core/src/course.rs::code_checks` already emits `long_method` for `CodeFacts.loc > 30`; `metrics.rs` maps it to cohesion and includes course checks in health. `crates/spide-import/src/facts.rs` counts body lines, and `crates/spide-core/tests/member_diagnostics.rs` already covers the boundary. Extend this pipeline, including mapped source evidence in design health, rather than create a second heuristic.

## Files to modify
- `app/src/ui/hover_card.rs`, `app/src/ui/state/hover.rs`: member sizing/placement.
- `app/src/ui/decisions.rs`, `proposal.rs`, `app.rs`, `chrome.rs`, `brief.rs`, `overlays.rs`; `app/src/ui/state/decision_drafts.rs`, `proposal_review.rs` and inspector/layout state: unified review, approval footer and Source UI.
- `crates/spide-core/src/model.rs`, `store.rs`, `proposal_review.rs`, `tools.rs`, `drift.rs`, `code.rs`, `adr.rs`: decision lifecycle, migration, symbol bindings, reconciliation and accurate exported records.
- `crates/spide-import/src/lib.rs`, `facts.rs`: scoped imports, source identities and locations; verify line-count handling of blank lines, multiline comments and nested declarations.
- `crates/spide-core/src/course.rs`, `rules.rs`, `metrics.rs`, `code.rs`; `crates/spide-diagram/src/insights.rs` and existing member rendering as needed: configurable length threshold, mapped evidence, deduplicated health contribution and visible diagnostics. Extend `crates/spide-core/tests/member_diagnostics.rs` and importer/diagram tests.
- `app/src/commands.rs`, `state.rs`, `mcp.rs`, `backend.rs`, `events.rs`, `watcher.rs`, `branches.rs`, `history.rs`, `prompts.rs`, `pi_bridge.rs`, `pi_control.rs`; new `app/src/source_tracking.rs` and `source_implementation.rs`: attachment persistence, asynchronous tracking and approved task orchestration.
- `agent/spide-skill.md`, `agent/spide-terminal.ts` if structured context/task events need extending, `app/src/ui/state/commands.rs`, `README.md`: agent contract, commands and docs. Read relevant pi extension documentation/examples in full before changing harness integration.
- `crates/spide-core/tests/store.rs`, `proposal_review.rs`, `adr.rs`; importer tests; `app/src/ui/tests.rs`, `app/tests/`, `tests/pi-extension.test.ts`; new focused source-tracking/task tests and `plans/native-rust-verification.md`.

## Reuse
- `Store::mutate`, `preview`, typed `ops::apply_all`, proposal review validation: atomic changes, isolated previews and undo; no parallel operation executor.
- `app/src/ui/proposal.rs` and `ui/state/proposal_review.rs`: operation editor, field diffs, safe drafts and selected-plan navigation.
- `app/src/ui/decisions.rs`: lazy option previews, shared comparison layout and impact details; reuse fixed-footer layout while removing defence requirements.
- `ui/state/hover.rs::place_card`: canvas-bounded hover placement.
- `commands::{import_code, load_reality, drift, code_snippet, drift_adopt}`, `spide_import::import_java`, `spide_core::drift`, `code::analyse`: parsing, body facts and comparison. Current `adopt_ops` is additive only and member drift name-only; extend rather than assume updates/deletions already work.
- `watcher.rs`: notify/debounce pattern; separate source watcher ownership from the existing design watcher.
- `state::write_atomic`, pi generation tickets and private control/prompt transport: durable state and stale-result rejection. Existing import writes reality before its generation check; fix that ordering in the shared refresh path.

## Steps
- [x] Confirm single-option small decisions, optional notes, separate code approval, Java-only external repository attachment and selected module scope integrated with the agent.
- [x] Trace native hover/review, store approval gate, importer, drift and private agent context transport.
- [x] Add failing regression tests and migrate to one decision/approval model, preserving pending work/history and compatibility producers.
- [x] Compose unified editable review with collapsible details and fixed actions; remove mandatory defence and stale examiner language throughout UI, commands, prompts and ADRs.
- [x] Improve hover sizing and edge placement.
- [x] Implement optional scoped attachment, source identities/bindings and atomic asynchronous refresh/watch lifecycle.
- [x] Add signature-aware drift, acknowledged baselines, conflict handling and selected source-to-design reconciliation decisions.
- [x] Extend function-length diagnostics with a configurable 30-line default, measured source evidence, linked-design health penalty and clear method/hover/health-list indication.
- [x] Integrate attachment/drift context with the same agent and add separately authorized source implementation tasks, revision checks, cancellation/reporting and post-edit verification.
- [x] Update docs and verification records; run automated suites and permissioned manual checks. No edits to the actual attached `zap-extensions` repository without a specifically approved implementation task.

## Verification
- Run `cargo test --workspace` and `node --test tests/pi-extension.test.ts`; use existing headless egui fixtures and temporary Java/Git repositories, not real source modifications or model requests in automated tests.
- Review/store: single/multi-option approval with empty notes and zero examiner calls; request-revision doesn't commit; amended plans need fresh approval; migration is idempotent and preserves linked/unlinked/dependent pending plans and historical verdicts; stale callbacks cannot apply changes; undo/redo, provenance and ADRs remain correct.
- UI: tall member cards stay within canvas at every edge; long diagnostics scroll and pin correctly. Hundreds of operations remain collapsible; expansion/drafts survive snapshots; footer reachable in narrow drawers/small windows; selecting, expanding, previewing and focusing never approves anything.
- Import/drift: duplicate simple names across packages, overloads, signature/type changes, explicit bindings, additions/removals/renames, ambiguous identities, partial parse failures, out-of-scope classes, design-only/source-only changes and concurrent conflicts. Preserve design annotations.
- Function length: test below/at/above the default and custom thresholds, blank/comment-only lines including multiline comments, overload-specific source binding and nested method bodies. An otherwise identical over-limit fixture must have lower cohesion/overall health and a visible method warning with measured length/threshold; shortening it restores health. Verify no duplicate penalty, no fabricated pure-design penalty, code-only versus mapped-design attribution, stale-source labelling, source navigation and refresh after edits/threshold changes.
- Tracking: scoped roots/exclusions and symlinks; burst events and deletions; no idle polling/model-request loops; stale refresh can't overwrite another workspace/scope; missing repo/relink, restart, detach, branch changes and pure-design mode.
- Implementation: design approval writes no source; explicit second approval dispatches exactly once to the same fake pi session with correct paths, instructions and scope; native permissions remain active. Fingerprint changes invalidate approvals. Test failures, unavailable tools, interrupted sessions and cancellation preserve pre-existing dirty files and expose partial edits. Refresh/verifier—not agent claims—determines completion; diagram undo never resets source.
- Manual: create a design-only project, approve one-option and alternative decisions without notes, refine another via notes, then attach selected Java roots in a disposable clone/fixture and exercise drift → adopt/revise → approve design → approve implementation → inspect diff/tests/residual drift. For real `zap-extensions`, obtain explicit approval before source writes and use its repository instructions and Java-17 wrapper without changing shell Java defaults.
- Update `plans/native-rust-verification.md`. Before implementation changes to tests/design, inspect the workspace-required REST Extraction DOCX for applicable scope; follow backup, concurrent-file check, formatting-preserving XML edits and edit-history rules for relevant changes only. Do not rewrite unrelated REST test cases; no DOCX writes during planning.

## Confirmed requirements
- Small changes are single-option decisions; substantive improvements present alternatives.
- No defence/examiner approval gate. Optional user text refines intent; user approval authorizes design changes.
- Agent implements approved design changes in source with user approval; also track source changes and offer design reconciliation.
- Java initially; attach an existing external repository (e.g. sibling `zap-extensions`). Pure design remains fully supported.
- Taller member hovers and collapsible review details with reachable actions are approved.
- Overlong functions must reduce health and be visibly indicated; extend the existing measured long-method heuristic with a configurable threshold (30 body lines by default).

## Scope / decisions settled
- Separate source implementation approval after design approval.
- Attach a repository with selected modules/source roots; attachment context is first-class in the same Spide agent session.
- Initial implementation is agent-assisted Java editing, not a Java code generator or guaranteed behavioural equivalence checker. One attached repository per workspace initially; multiple selected roots are supported.
- No blocking product questions remain.
