---
name: spide-implement
description: Implement a Spide Project's approved design changes in the Java code (design → code). Use only when the user or Spide explicitly asks to implement, e.g. after "Implement in code"; the request lists files and exact changes.
---

# Implement the approved design in the code

Spide sends an implementation brief: per class the target file (existing, or where a new class belongs), the exact
structural changes (classes, fields, methods with signatures, extends/implements, package moves), the class's
responsibilities and the decisions behind them. Only the user can approve design changes; this brief contains
approved ones only.

1. Read the repository's instructions first (`AGENTS.md`, contributing notes, code style). Follow them.
2. Read each target file before editing. Use `get_code` or your normal file tools; `get_sync` shows the
   remaining differences with `file:line`.
3. Implement exactly the listed changes:
   - signatures as listed (parameter and return types; static/abstract);
   - new classes in the listed file and package, following the module's conventions (license headers,
     annotations, logging, tests);
   - behaviour that fulfils the class's responsibilities and the method descriptions, not stubs, unless the brief
     or the user says otherwise;
   - keep existing behaviour working; adapt callers you break.
4. If a listed class is a framework or library type (ZAP, Swing, the JDK, …), don't create it: tell the user to mark it
   (or its package) external so Spide stops comparing it.
5. Never change the design through Spide's tools in this workflow. If the design looks wrong, say so; the user can
   raise a decision.
6. Never commit, stash, reset, rebase or switch branches. Ask the user before deleting a file. Don't touch build
   output or unrelated files.
7. If the manifest has a test command, run it in its working directory and fix what you broke.
8. Finish with a two-line summary: what you changed and anything you couldn't do.

Spide re-indexes the code when you're done and shows what is still out of sync. Structural agreement doesn't prove
behaviour — say which behaviour you couldn't verify.
