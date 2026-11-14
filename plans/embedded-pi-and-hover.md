# Responsive hover and embedded pi terminal

## Context
- Hover feels slow. `src/App.tsx:onHover` cancels and restarts its 260 ms timer on **every pointer move**, including moves over the same element: the card effectively waits for the pointer to stop. Hover state also lives in the top-level App.
- User wants a left-side embedded **real pi terminal**, including pi's native model-switch shortcut, rather than a chat imitation.
- Current `src-tauri/src/pi_bridge.rs` starts `pi --mode rpc` with piped stdin/stdout, not an interactive terminal. Native TUI needs a PTY and a terminal emulator; RPC output cannot simply be rendered as a terminal.
- No PTY/xterm dependency exists yet. Preserve the current working-tree changes in `plans/diagnostics-and-ui.md`.

## Confirmed requirements
- The left pane replaces the bottom prompt and floating chat. Pi's own prompt/editor lives in the embedded terminal.
- Enable **all available pi tools**, including shell and file modification, not the current read-only allowlist. Keep Spide diagram edits on MCP's proposal/decision path; ordinary code/files can be edited directly.
- Improve the right-hand alternatives panel as part of the same workspace redesign.

## Approach

### 1. Fast, stable hover
- Extract hover timing/state into `src/state/hover.ts` and a `HoverLayer` subscriber instead of storing the whole pointer position/card state in `App`.
- Initial intent delay **60 ms**, immediate switches when a card is already open, **80 ms** leave grace. Only target changes schedule the timer: moving within the same target updates a ref, never restarts the delay. Cancel timers on unmount, drag, pan, zoom, project/view change and element removal.
- Canvas subscribes only to the focused **class key**, not pointer coordinates or member-key changes within that class. Only the tooltip layer repositions, at most once per frame; terminal output never drives it.
- Measure the actual card size and clamp/flip within the canvas/workspace bounds (current code assumes a 420 px height and can run below the window). Scroll long contents. Prefer placement beside the hovered element rather than across its title; don't cover terminal/decision controls. Keep hover pointer-transparent and use the command palette for actions such as opening code.
- Cover all existing `data-hover` targets across all six views; no separate timing implementations per view.

### 2. Three-pane workspace
- CSS grid: **pi terminal | canvas | decision inspector**, with draggable separators. Default terminal width ~420 px, minimum 280 px, resizable and collapsible. Default decision width ~360 px, expandable for details. Persist pane widths/visibility locally, not as design mutations.
- The terminal replaces `PromptBar` and `AnswerBubble` entirely. Pi's own editor at its bottom is the only chat input. Keep health, proposals and the detail slider in a compact canvas toolbar; reclaim the old bottom prompt's 150 px viewport reservation.
- Make canvas fitting, tabs, panels, legends, zoom-to-class/package, comparison, hover and exports respect its actual available bounds. Existing brief, sequence, timeline and drift panels belong to the canvas area, not on top of the terminal.
- On narrow windows, collapse the right inspector to a drawer/tab and allow either side pane to collapse; always leave a usable canvas. Resizing a pane must not re-layout the design on every pointer move.

### 3. One real interactive pi, not a simulated terminal
- Add `portable-pty` in Rust, `@xterm/xterm` and `@xterm/addon-fit` in the frontend. Run the user's pi binary directly in the PTY in the project's working directory with `TERM=xterm-256color` and true-colour support. Keep binary discovery, login-shell PATH recovery for Finder, `.spide/sessions`, `--continue`, appended skill and existing MCP configuration.
- Remove `--mode rpc` and the read-only tool allowlist. Select all available tools with a single literal `--tools '*'` argv argument, retaining configured extensions/MCP servers and native tool controls. Do not install optional pi packages or change global model/keybinding settings. Existing user permission extensions remain in charge: **remove the RPC bridge's automatic confirmation approvals**.
- Load a bundled, app-owned pi extension explicitly with `-e` from `.spide/terminal-bridge.ts`. Adjust `agent/spide-skill.md`: ordinary source files may be inspected, edited and commands executed; only diagram mutations must use Spide MCP proposals/decisions. Don't prohibit code output or longer explanations when the user is coding.
- Keep terminal instance mounted while hidden/tab switching; resize only when visible. xterm owns text selection, ANSI rendering, alternate screen, scrolling, native editor and model controls. Forward terminal input/output through Tauri IPC, not a new browser-accessible HTTP endpoint.
- Batch output with bounded queues and backpressure (no dropping escape sequences); preserve UTF-8 split across reads. Use generation IDs and ordered delivery. Attach/listen before output starts; a bounded replay buffer handles initial attach/reconnect without duplicating output. Test React StrictMode's mount/unmount cycle.
- Track process exit, expose restart and clear error state. Stop/reap pi and its process group/owned descendants on app exit, restart and project switch, without holding the design mutex while waiting. Ignore callbacks from old generations. Closing/collapsing the pane must **not** terminate pi.
- Terminal focus suppresses canvas/global shortcuts, type-to-focus, app context-menu prevention and scroll gestures. `Ctrl+P`, `Ctrl+L`, `Shift+Tab`, Escape, arrows, Tab, Enter, Ctrl+C/D reach pi. Support modified-key encoding needed for multiline input/backward cycling without remapping the user's pi configuration. Standard copy/paste uses the terminal selection/clipboard; do not bind Ctrl+P to a web print dialog. No per-keystroke React updates.

### 4. Preserve Spide's app↔agent workflow with an extension side channel
- Add `agent/spide-terminal.ts`, using supported `ExtensionAPI` hooks (not a second RPC process). Rust owns a private Unix-domain socket in a short random temporary directory (0700, socket 0600); pass its location and per-generation token via the child environment. The extension connects on `session_start` and closes on `session_shutdown`; reconnect correctly after pi `/reload`, `/new` and `/resume`.
- Newline-delimited, size-capped JSON with request IDs, generation IDs, acknowledgements, timeouts and structured errors. Token/connection state never goes into design snapshots, terminal logs, the MCP API or frontend storage. No new unauthenticated prompt/file/shell endpoint.
- Existing `PiBridge.prompt()` callers (defence, fix-with-agent, use-case/sequence generation, change analysis, import assessment) send **structured prompts** through this connection. The extension invokes `pi.sendUserMessage`, with `followUp` when busy. Never paste prompts into the user's partly typed editor or fabricate Enter/Ctrl-key input. Preserve the draft while an examiner request is delivered.
- Use `agent_start`/`agent_settled` and model/session events for status; retain stuck-defence recovery on final settle. Terminal bytes are display-only, not parsed for state. Readiness is signalled only after extension handshake, not immediately after spawning.
- Native interactive prompts get current brief/stage/mode context through the extension's `input` hook and Rust's existing `prompts::user_prompt`; skip already decorated extension-origin messages and preserve images and streaming delivery semantics. Fail with visible integration errors, not silently submitted context-free design turns.
- Register **`/spide <command>`** with completion for existing Spide commands, forwarding app actions to the existing frontend command dispatcher. Native pi commands (`/model`, `/settings`, `/new`, `/resume`, `/export`, etc.) retain their original meaning. Example: `/spide tidy`, `/spide import`, `/spide open`, `/spide detail 3`. Show a Spide command palette/button in the left pane header as well, including open/save/import dialogs and restart even if pi is offline. Refactor the existing App command switch instead of duplicating its actions.
- Validate stale/duplicate side-channel requests; make retries safe via request IDs. Project switch must stop the old pi/control connection **before** replacing project state, then start the new session. Queued prompts cannot cross projects.

### 5. Clearer alternatives and defence
- The user confirmed this redesign: compact alternatives, expandable details, explicit preview buttons and a persistent defence editor.
- Keep a fixed header with question, decision index/navigation and Compare. Scroll **only the options/details area**. Each option shows a real selectable control, name, concise summary and health delta; selected state is distinct from preview state.
- Expand previews, pros/cons, brief trade-offs and `ImpactLine` details on demand. Mount mini-diagrams lazily when expanded, not for every collapsed option; cache/reuse layouts where inputs haven't changed.
- Explicit **Preview / Stop preview** controls. Selecting/expanding/hovering does not replace the canvas. Close preview when a decision changes/resolves or its underlying model revision changes; restore the accepted model. Previews are read-only and never save positions or mutate the live design. Ignore stale asynchronous preview/layout completions.
- Sticky footer: selected option/custom alternative, defence editor, examiner feedback, Defend/Defend again, overrule (lenient only), skip. Disable invalid submissions and show evaluation/failure status; keep drafts by decision ID when switching decisions, resizing or receiving snapshots. Buttons inside cards must not accidentally select or submit.
- Preserve `ComparePanel`, shared-layout comparison, consequences and existing backend decisions/examination APIs. Use accessible labels, keyboard selection and focus-visible controls; do not capture terminal keystrokes globally.

## Files to modify
- Workspace: `src/App.tsx`, `src/styles.css`; new `src/ui/{TerminalPane,Workspace,SpideCommandPalette}.tsx` and `src/state/commands.ts`.
- Hover: new `src/state/hover.ts`, `src/ui/HoverLayer.tsx`; `src/ui/HoverCard.tsx`, `src/canvas/{Canvas,Viewport,DiagramContent}.tsx` and derived views' hover/focus integration.
- Terminal/frontend transport: `src/state/{api,store,types}.ts`; retire `src/ui/{PromptBar,AnswerBubble}.tsx` and unused reconstructed chat feed. Add terminal bridge module/types/tests as needed.
- Decisions: `src/ui/{DecisionCard,ComparePanel,Impact}.tsx`; adjust overlay positioning of brief, timeline, drift, code and sequence panels to the workspace.
- Backend: `src-tauri/src/{pi_bridge,commands,main,state}.rs`; new `src-tauri/src/{terminal,pi_control}.rs` with tests. Preserve `mcp.rs` tool server; don't expose PTY control through its public/dev routes.
- Pi: new `agent/spide-terminal.ts`; `agent/spide-skill.md`, `src-tauri/src/prompts.rs`.
- Dependencies/resources: `package.json`, `package-lock.json`, `src-tauri/Cargo.toml`, `Cargo.lock`; Tauri resource/IPC configuration only where needed.
- Verification/docs: automated UI/PTY/control tests, `README.md`, `plans/interactive-design-verification.md`.

## Reuse
- `pi_bridge.rs`: `pi_binary`, `child_path`, `write_mcp_config`, session setup, generation-based shutdown/restart; replace only RPC-specific transport/parser.
- `prompts.rs`: `user_prompt`, `examine`, `fix_issue`, `analyse_change`, regeneration and import assessment prompts.
- `commands.rs`: existing intents, dialogs/API calls, `submit_defense`, `switch_project`; preserve their semantics while routing to the single interactive session.
- `App.tsx` command dispatcher: extract for `/spide` and the command palette; avoid duplicate command implementations.
- `Viewport`: `toWorld`, `fit`, `zoomTo`, `onTransform`, delegated `data-hover`; `Canvas`: memoised diagram and focus. Replace fixed inset assumptions with actual grid dimensions.
- `DecisionCard.tsx`: existing validation, API calls, feedback, custom option, lenient overrule; `ComparePanel.decisionCols` and `ImpactLine`/`ImpactDetails` for comparison/consequences.
- Pi docs inspected: `docs/{extensions,cli,keybindings,mcp}.md`; examples `send-user-message.ts` and `input-transform.ts`. Follow documented lifecycle, `input` transformation, `sendUserMessage` and settled events; no private pi APIs.

## Steps
- [x] Add focused hover timing/placement tests; implement local hover store/layer and class-only focus subscriptions across all views.
- [x] Add PTY dependencies and backend I/O/lifecycle tests; implement bounded ordered streaming, attach/resize, exit/restart and cleanup.
- [x] Implement the private authenticated control protocol and bundled pi extension; connect status, app prompts, native-input context and `/spide` command routing. Test reload, busy follow-ups and failures.
- [x] Introduce the resizable three-pane workspace and xterm component; remove bottom prompt/floating chat, relocate toolbar/panels, scope keyboard/clipboard/scroll handling and update viewport measurements.
- [x] Refactor the alternatives inspector: compact selectable cards, lazy details, explicit read-only preview and sticky defence/feedback/actions; preserve drafts and comparison.
- [x] Run unit/integration/UI checks and real-pi desktop tests; update documentation and verification notes with measured results and outstanding platform limitations.

## Verification
- **Automated:** `cargo test --workspace`, `npm run typecheck`, `npm run build`; add a small frontend test runner/fake-timer tests if needed and a runnable browser smoke script. Build the release `.app`; test Finder launch (PATH recovery, bundled extension, native WebKit terminal).
- **Hover:** move continuously within the same class/member and see the card after ~60 ms without stopping; switching targets while open is immediate; no stale card after pan/zoom/drag/tab/project change. Measured-size placement stays inside available bounds, including long classes near each edge. Mouse movement inside the same class doesn't re-render App/diagram every frame. Test class/member/relation/package/use-case/sequence targets.
- **PTY/control tests:** deterministic fake child emits ANSI and split UTF-8, takes input, reports size and exits. Verify byte ordering/no loss, bounded memory/backpressure, late attach, resize, collapse/reopen without restart, process-group cleanup, stale-generation rejection. Test protocol auth, message size/IDs/timeouts, duplicate requests, busy follow-ups, disconnect/reconnect and pi extension reload.
- **Real pi, inside the app:** `Ctrl+P` cycles the configured models (configure a temporary test scope with ≥2 models if necessary); `Ctrl+L` picker, Shift+Tab, arrows, Tab, multiline paste/input, Escape cancellation, selection/copy/paste, tool expansion, transcript scrolling and `/reload` all operate natively. Confirm read/bash/edit/write/grep/find/ls plus configured extension/MCP tools are available. Test source editing/shell only against a disposable fixture, not user code. Permission dialogs must remain interactive, never silently auto-approved.
- **End-to-end Spide:** `/spide tidy`, detail/lens, open/save/import dialogs, project/branch changes; proposals appear on canvas and can be accepted/rejected; selecting an option + submitting defence invokes the **same** pi session and yields a verdict; fix-with-agent and generation/import prompts preserve context. Trigger an app prompt while a draft is typed in pi and while pi is busy: draft stays intact, request queues once, doesn't leak into the next project.
- **Right panel:** at 1440×900 and 800×560 verify no overlapping panes and persistent defence actions. Hovering/selecting never changes the canvas. Preview is explicit and reversible; expanding details doesn't reset selection/draft; navigation and rejection/evaluation states work; comparison remains available. Preview mode must not prune/rename/accept/save layout to the accepted design.
- **Performance:** `?perf=1` and CPU sampling: terminal streaming and same-target pointer movement must not produce App/canvas render storms; hidden terminal retains session without unnecessary fits/output React renders. Confirm no reintroduced continuous force layout.
- Record actual results and limitations in `plans/interactive-design-verification.md`. Follow workspace test-plan maintenance instructions for any relevant shared test-plan update: read current DOCX, preserve formatting, back up under `.local/testplan/`, recheck unchanged before writing, add edit-history row; no document writes during planning.

## Additional findings
- Pi docs confirm `Ctrl+P` cycles models, `Ctrl+Shift+P` cycles backward, `Ctrl+L` opens the model selector. Pass native bindings through; do not replace them with app shortcuts.
- Interactive pi requires terminal stdin/stdout; current RPC mode can't supply the TUI. Use a real PTY.
- Pi extensions support `input`, `sendUserMessage`, `agent_start`, `agent_settled`, and `session_shutdown`: an explicit app-owned extension can connect the existing Spide prompt/examiner operations to that same TUI session without typing synthetic keystrokes into its editor.
- Current alternatives panel renders every mini-diagram and all pros/cons in full. Hovering an option immediately replaces the main canvas with a preview. Selection, preview and defence are not clearly separated.

## Scope and decisions
- User confirmed the alternatives redesign above. No remaining product questions.
- macOS is the implementation/desktop verification target (the user's installed app); the private socket transport is Unix-specific. Do not claim Windows support without implementing/testing another transport.
- This work changes Spide and its bundled extension, not the installed pi harness or global model/keybinding settings. It does not automatically reinstall the app; release build verification is included.
