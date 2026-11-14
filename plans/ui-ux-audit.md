# Spide UI/UX audit and improvement plan

## Context
Spide is a Tauri desktop system-design workspace for students/designers working with a native pi terminal, six diagram views, proposals, defended decisions, diagnostics, brief/rules, history/branches, code comparison and exports. Preserve the Rust source of truth, agent permission boundaries and native terminal input.

Planning only: no implementation until approval. The working tree contains substantial existing modifications and untracked UI/tests; preserve these as the baseline, without reverting or overwriting them.

## Initial findings
- `src/App.tsx` renders an entirely blank app until a snapshot arrives; loading/failure/retry need investigation.
- Diagram navigation and project identity are hidden when there are no classes, potentially hiding useful System/Use case workflows.
- Toolbar Fit uses only `canvasRef`, unlike `/fit`, which selects the active view's viewport; this can fail outside Class/Domain.
- Preview blocks some mutations with silent no-ops rather than visibly disabled controls.
- Many important workflows are command-led; evaluate discoverable UI entry points without replacing native pi commands.
- Existing hover, terminal, decision draft and workspace improvements and tests must be inspected/reused before proposing replacements.
- No Spide-specific AGENTS.md found; workspace instructions apply, including the shared test-plan update requirement (scope to clarify).

## Confirmed scope
- macOS desktop only; narrow desktop windows, not mobile/touch product support.
- Retain the dark, diagram-first workspace and native pi terminal.
- User requests no test-documentation work. Keep reporting minimal; the higher-priority workspace requirement to maintain the test plan still applies. During implementation, make only a clearly scoped Spide coverage note in the mandated shared DOCX, without changing REST test expectations; check for concurrent edits, preserve formatting, back up under `.local/testplan/`, and add the required edit-history row. Do not touch the DOCX during planning.

## Additional confirmed findings
- P1: Health uses immediate mouse-leave dismissal with a 10px CSS gap, and has no keyboard/click trigger (`Chrome.tsx`, `styles.css`).
- P1: Snapshot failures only log to the console; startup never recovers in Tauri without reload (`state/store.ts`).
- P1: Brief additions clear text before save succeeds; edits dismiss before acknowledgement; editing relies on double-click (`BriefPanel.tsx`).
- P1: Palette lists bare command names including commands requiring arguments; input has no associated label or discoverable argument help (`SpideCommandPalette.tsx`).
- P2: Timeline has no loading/empty/retry presentation and bubbling arrow-key handlers can also handle the range input's native key behavior (`Timeline.tsx`).
- P2: Toasts are click-only dismissible with no live-region semantics; health issue navigation uses clickable spans (`Chrome.tsx`).
- Reuse: decision cards already implement radio alternatives, persistent drafts, explicit preview, pending submission and inline errors; do not replace these.
- Reuse: diagram hover is intentionally passive; keep its fast intent timer, measured placement and localized rendering, distinct from interactive health popovers.

## Approach
1. Audit actual components, state transport, commands, styling, existing plans and tests.
2. Prioritize workflow correctness/recovery and interaction accessibility before visual refinement.
3. Retain a restrained, diagram-first design and native terminal; consolidate reusable state/overlay patterns rather than rewrite architecture.
4. Add regression coverage and document verified results and remaining native-only/manual checks.

## Prioritized implementation
### P1 — Complete and recoverable core workflows
- Expose snapshot loading/error/retry state; subscribe safely before initial fetch and ignore superseded responses. Preserve the last valid snapshot on transient failure, clearly label unavailable state, and do not claim background reconnection detection without a backend signal.
- Always show project identity and six-view navigation. Add a compact empty-canvas welcome with **Edit brief**, **Open design**, and **Focus pi** actions. Explain the brief → proposals → defend loop without adding another chat editor. Preserve existing System/Use case/Sequence generation flows.
- Add a compact workspace action menu reachable even when pi is collapsed: Brief, Find, Timeline, Import/compare code, Open/New, Export and Commands. Use the existing dispatcher/API; labels must distinguish opening a design JSON from creating a project folder.
- Replace bare palette strings with typed metadata (label, group, keywords, example, required arguments). Selecting an argument-taking command fills its template and keeps the palette open; execute only valid complete input. Add no-results, pending and actionable error states, keyboard navigation and focus return. Keep native `/spide` behavior intact.
- Share active-viewport routing for toolbar Fit and `/fit`. Use explicit preview source (option/history), visible **Exit preview**, and genuinely disabled controls with explanatory text. Clear preview on incompatible navigation, project/revision changes; preserve drafts. Ensure preview cannot write layouts or mutate the accepted design through canvas interactions.
- Handle dialog rejection/cancellation and async failures consistently. Prevent duplicate submissions; clear drafts only after successful mutation. Brief/rules/change-scenario/rename forms get labels, inline validation, explicit Save/Add/Cancel and pending feedback. Numeric thresholds edit locally and commit validated values instead of writing every keystroke. Separate successful scenario creation from failed agent analysis so retry cannot duplicate it.
- Show timeline/comparison/mini-preview loading, empty and layout-error/retry states rather than blank panels. Cancel stale requests on project/branch changes. Fix timeline arrow-key bubbling and preserve the selected historical revision identity.

### P1 — Forgiving pointer and keyboard interaction
- Convert health to an accessible button with click/keyboard toggle plus optional hover opening. Bridge the trigger-to-popover gap and use cancellable dismissal delay; keep open while pointer or focus is inside. Escape/outside click dismisses; restore focus for keyboard-opened interaction. Health issue navigation becomes buttons; show filtered empty state and a way to reach all issues rather than truncating at 40.
- Keep diagram hover cards passive (no pointer interception); expose the same content through a persistent keyboard-accessible selection/details surface. Reuse card content rather than duplicate descriptions.
- Add a shared overlay/focus utility for palette, comparison, code and confirmation surfaces: named region/dialog as appropriate, initial focus, topmost-only Escape and focus restoration. Trap focus only for genuinely modal confirmations; keep pi available beside nonmodal panels. Guard global shortcuts while editing, in contenteditable/select controls, or in pi.
- Preserve fast reversible prune behavior with clear Undo; confirm irreversible branch deletion and explain terminal restart consequences. Do not add blanket confirmations to every reversible edit. Verify backend undo semantics before promising recovery for reject/dismiss/overrule.

### P2 — Cohesive desktop polish and accessibility
- Retain dark neutral surfaces, restrained blue accent and semantic warning/error/proposal colors. Consolidate spacing/radius/type/control tokens and active states; replace low-contrast essential `--faint` text. Provide visible focus, selected/expanded states, field labels and non-color status cues.
- Make toolbar height/layout adapt to available canvas width instead of fixed offsets; prevent proposal controls, long project names and preview banners overlapping diagrams. Bound panels/popovers to the workspace; comparison columns scroll internally at narrow widths. Keep decision footer and terminal usable at the supported **800×560** minimum.
- Make inspector empty state reachable (currently `!!inspector` prevents opening it); expose pending count and expanded state. Clamp keyboard resizing to the same actual geometry as pointer resizing and announce the actual separator value. Never remount/restart pi or rerun ELK just because panes resize.
- Add keyboard selection/navigation for diagram targets without thousands of tab stops (roving focus or an accessible target list backed by existing target metadata). Offer inspect, focus, accept/reject/restore and rename where supported; preserve existing click/provenance behavior. Add visible zoom controls and keyboard pan/zoom only while the canvas is focused.
- Respect reduced motion in CSS decorations and imperative viewport fit/zoom animation. Make toasts live-announced with explicit dismiss buttons, pausing timed dismissal while hovered/focused; retain actionable errors in their originating forms.
- Explain pi `starting`/trust/offline states without automatically approving permissions. Restart has pending/error feedback. Agent actions distinguish submitted/queued from completed, based only on existing signals.

## Files to modify
- `src/App.tsx`, `src/state/store.ts`, `src/state/commands.ts`: startup, shared actions, explicit preview state and command metadata; `src/state/api.ts` only if existing result/error typing needs adjustment.
- `src/ui/Chrome.tsx`, `Workspace.tsx`, `TerminalPane.tsx`, `SpideCommandPalette.tsx`: chrome, navigation, health, pane state and command discovery.
- `src/ui/BriefPanel.tsx`, `RulesSection.tsx`, `ChangeSection.tsx`, `InlineEdit.tsx`: recoverable validated forms.
- `src/ui/Timeline.tsx`, `ComparePanel.tsx`, `DecisionCard.tsx`, `DriftPanel.tsx`, `CodeView.tsx`, `FeedbackPopover.tsx`, `DetailSlider.tsx`: async/preview/overlay and recovery refinements, preserving existing business logic.
- `src/ui/HoverCard.tsx`, `HoverLayer.tsx`, `src/canvas/Viewport.tsx`, `Canvas.tsx`, `DiagramContent.tsx`, and target rendering in `src/views/{System,Package,UseCase,Sequence}View.tsx`: accessible inspection and active-view affordances. Leave passive hover timing unchanged unless tests reveal defects.
- New small `src/ui/overlay.ts` and `src/ui/SelectionDetails.tsx` utilities/components only where existing code cannot be reused; no new UI framework.
- `src/styles.css`, `src/canvas/visuals.ts`: shared accessible tokens, desktop geometry and reduced motion.
- `tests/` and `scripts/browser-smoke.mjs`, `scripts/terminal-browser-smoke.mjs`: targeted regression coverage. Add a separate audit browser script if needed for maintainability and wire it into `package.json`.
- Mandated shared test-plan note as scoped above; no broad documentation rewrite or unrelated repository changes.

## Reuse
- Existing `api`, `on`, `useSnapshot` in `src/state/api.ts` and `src/state/store.ts`.
- `hoverStore` in `src/state/hover.ts` and `src/ui/HoverLayer.tsx` (inspect behavior and timers).
- `Workspace`, `TerminalPane`, existing decision draft persistence and command bridge.
- Existing `/fit` active-viewport selection and shared action handlers in `src/App.tsx`.
- Existing Node test runner and Playwright smoke infrastructure; no new dependencies planned.

## Steps
- [x] Confirm platform and design direction; inspect screen/component/transport/test implementation.
- [x] Capture disposable-fixture before screenshots and baseline checks; preserve all current uncommitted changes.
- [x] Implement startup/recovery, correct Fit/preview routing and persistent navigation.
- [x] Improve command discovery, forms and async/loading/error states.
- [x] Implement health hover safety, shared overlay behavior and mutation recovery.
- [x] Refine dark desktop layout, diagram keyboard access, contrast and reduced motion.
- [x] Extend regression tests, validate native-only paths separately, make mandated scoped test-plan update and report actual results/limitations.

## Verification
All execution below follows approval; planning has only inspected source, not run the app or tests.
- Automated: `npm run typecheck`, `npm test`, `npm run build`, `npm run test:browser`; add unit coverage for command parsing/required arguments, preview transitions, stale requests and popover dismissal timing using the existing Node runner. No dependency installs. Run Rust tests only if backend code must change (none planned).
- Fixtures: empty, library, larger REST-extraction design, many/long decisions, many health issues and long project names. Use disposable snapshots/projects and intercepted mutations; never run tests against the user's live design.
- Six views: tab keyboard navigation, active Fit/zoom, empty and stale generation states, keyboard target inspection, proposals/undo, preview entry/exit and zero mutation calls from read-only interactions.
- Failure injection: initial snapshot failure and retry, failed form saves retaining drafts, invalid rules, native dialog cancellation/rejection, failed generation/import/history/layout, duplicate clicks, delayed out-of-order responses and project switches. Confirm errors remain recoverable and successful steps aren't repeated.
- Pointer: move slowly, diagonally and repeatedly from health trigger across the gap into issue actions; scroll popover; leave/re-enter; outside-click and Escape. Ensure no flicker and that passive diagram hover still meets existing latency/no-render-storm tests. No nested menus are proposed.
- Keyboard: full first-run → brief → decision → compare → defence flow; selected states, labels, focus visibility, Escape only closes the top overlay, restoration, no focus trap in nonmodal panes, and no canvas shortcuts stealing pi/editor input. Manually inspect VoiceOver output in macOS where available.
- Geometry: 1440×900, 1280×800, 1024×768 and minimum 800×560; pane collapse/reopen/resize, long content, high text zoom, scroll reachability, bounded popovers and decision footer. Measure essential-text contrast (4.5:1), large text/UI indicators (3:1 where applicable) and practical 24px+ interactive targets.
- Motion/performance: reduced-motion emulation stops decorative/fit animation; existing hover and terminal streaming performance regressions remain covered; pane changes do not create new PTYs or ELK work.
- Desktop WebKit: native file dialogs, trust prompts, terminal restart/collapse, keyboard shortcuts, clipboard and a disposable-project undo/export/import/history pass. Do not send real model requests or bypass permissions without authorization. Mocked browser/PTY tests are not evidence of completed real-agent workflows.

## Boundaries and remaining dependencies
No backend features, mobile support, terminal replacement, diagram layout rewrite or new design framework. Preserve existing known backend limitations (non-three-way merges, heuristic diagnostics/import, concurrent layout writers) and report them rather than claiming UI changes solve them. Desktop WebKit/VoiceOver and live-agent checks may require an interactive session; report unrun checks explicitly. Broad WCAG certification is out of scope; target concrete AA improvements and measured regression checks.
