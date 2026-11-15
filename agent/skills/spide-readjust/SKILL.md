---
name: spide-readjust
description: Bring a Spide Project's design up to date with its Java code (code → design). Use when Spide asks to readjust the design, when get_sync shows code_ahead, conflict or unreconciled items, or to bootstrap an empty design from existing code.
---

# Readjust the design to the code

The design lives in `spide/design.json` next to the code; you change it **only** through Spide's MCP tools
(proposals and decisions the user approves on the diagram). Never edit code in this workflow.

1. `get_project` for the scope and freshness. If the index is `indexing`, wait; if it is partial, say which files
   were skipped.
2. `get_sync` with `status: ["code_ahead", "conflict", "unreconciled"]` (and `classes` when the user gave a
   scope). Each item has the design's and the code's value, `file:line`, and `suggested_ops` that bring the
   code's version into the design.
3. Where the intent isn't obvious from the names, read the implementation with `get_code` (class or one method).
4. Turn the differences into proposals:
   - **Mechanical** changes (a new method or field, a changed type, a moved class, a missing link):
     `propose_ops`, starting from `suggested_ops`. Group related items of one class or one feature into one call
     with a short title-like rationale. Add what the code can't say: responsibilities, method descriptions,
     `concern` when the name is vague.
   - **Structural or responsibility** changes (a class split or merged, behaviour moved, a new collaborator):
     `propose_decision` with alternatives — typically *adopt the code's structure* vs. *keep the design and
     implement it back* — with pros/cons in course terms (Information Expert, Creator, Controller, coupling,
     cohesion).
   - **Conflicts** (both sides changed): always a decision; show both versions as options.
   - A `rename_of` hint means a design-only and a code-only class look like a rename: propose `rename_class`
     (plus `link_code` if the name doesn't give the right fully qualified name) instead of remove + add.
5. Identity is exact. When a design class deliberately has another name or package than its Java type, add
   `{"op":"link_code","class":…,"symbol":"org.x.Type"}`; never rely on simple names.
6. Never remove design intent just because the code lacks it: a design-only element is *design ahead* (to be
   implemented), not something to delete, unless the user says the code is right.
7. Framework/library types (ZAP, Swing, …) are `external`; keep them compact.

**Bootstrap** (empty design): one decision per package with the public API only (no private helpers, accessors,
constructors or framework plumbing), responsibilities per class, external classes marked. Afterwards generate the
domain model (`set_domain_model`) and use cases (`set_use_cases`).

Accepted proposals put their items in sync; Spide moves the baseline along. Marking the remaining differences as
accepted is the user's decision (Project panel → Mark in sync), not yours. Reply briefly: what you proposed and
what you left for the user to decide.
