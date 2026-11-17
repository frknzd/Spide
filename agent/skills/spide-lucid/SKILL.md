---
name: spide-lucid
description: Keep Spide's diagrams in sync with Lucid (Lucidchart) documents that colleagues view, comment on and edit — push design changes in place, pull colleagues' edits as proposals, read their comments. Use when the user mentions Lucid/Lucidchart, sharing diagrams with colleagues, or /spide lucid.
---

# Lucid sync (git-like)

Every Spide diagram has its own Lucid document (class, domain, package, usecase, system.context,
system.containers, one per sequence), all from the **frontier branch** (`main` unless configured). Spide owns the
shapes it drew (tagged with a `spide.id`); colleagues' own shapes, notes, styling and comments are never touched.

**Always use the `spide_lucid` tool.** It reads the documents with Spide's read script, asks Spide what to change
and runs the edit scripts with Lucid's MCP — the diagram data never passes through you. Never hand-write Lucid
scripts or Standard Import JSON for Spide's diagrams, and never create a second document for a diagram: pushes
update the existing document in place (its URL, comments and collaborators stay).

## Workflow

1. `spide_lucid action=status` — per diagram: `in_sync`, `local_ahead` (push), `remote_ahead` (pull),
   `diverged` (pull first), `unlinked` (push creates it).
2. `remote_ahead` / `diverged` → `spide_lucid action=pull` (one diagram with `key`). Colleagues' edits become ghost
   proposals, filed separately: additions/changes, removals, **conflicts** (Lucid's version of elements both sides
   changed — the user decides), moved boxes. Their comment threads and sticky notes become Spide comments.
   Tell the user what came in; they decide the proposals on the canvas (you never accept them).
3. When the proposals are decided: `spide_lucid action=push`. A push is refused while the document changed since the
   last sync or while the pull's proposals are undecided — pull/decide first, never work around it. Rejected remote
   edits are overwritten by the push (that is the merge).
4. Report the document URLs after a push.

## Comments are design input

`get_design` lists `open_comments`; `get_comments` has the threads (who said what, about which element). Read them
before changing the elements they are about. When a proposal or decision answers a comment, call
`address_comment` (comment id, decision/proposal id) so it stops showing as open — and mention it in the rationale.

## Setup and errors

- `spide_lucid action=enable` turns sync on for the project. Linking an existing document colleagues made:
  `spide_lucid action=link key=<diagram> url=<lucid url>`, then `pull` imports it as proposals.
- "Is Lucid connected?" errors: ask the user to run `/mcp login lucid` (browser sign-in). On Lucid Team/Enterprise
  accounts an admin must enable MCP access first.
- `not_frontier`: Lucid syncs only the frontier branch; ask the user to switch back before syncing.
- A failed push leaves the last sync untouched; running it again is safe (the edits are idempotent).
