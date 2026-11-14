# Canvas layout improvements

## Context
- Request: “Improve the test layout of the canvas.” Scope needs clarification: diagram/text presentation versus layout test coverage.
- Spide is a React/TypeScript canvas app. Existing layout uses ELK plus canvas physics.

## Approach
- Confirm the intended visual or testing outcome before selecting changes.
- Reuse current canvas/layout infrastructure rather than introduce another layout system.

## Files to modify
- To be confirmed after scope clarification.
- Relevant existing files: `src/canvas/layout.ts`, `src/canvas/Canvas.tsx`, `src/canvas/ClassNode.tsx`, `src/styles.css`, `tests/`, `scripts/`.

## Reuse
- `elkLayout` and `elkCompoundLayout` in `src/canvas/layout.ts` already handle initial/tidy placement, hierarchy direction, and package separation.
- `package.json` defines Node unit tests and browser smoke scripts.

## Steps
- [ ] Clarify what “test layout” refers to and desired outcome.
- [ ] Inspect the relevant rendering or test paths and existing reusable helpers.
- [ ] Specify focused implementation changes and regression checks.

## Verification
- Select unit/browser checks appropriate to confirmed scope.
- For visual changes, manually inspect representative canvas diagrams.
- Follow workspace test-plan maintenance requirements if tested behaviour or design decisions change; make no non-markdown changes during planning.
