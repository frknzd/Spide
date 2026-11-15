# UML-correct sequence diagrams

## Context
The Sequence tab (`crates/spide-diagram/src/views.rs::sequence_scene`) and its PlantUML export (`plantuml::sequence_diagrams`)
draw a flat list of messages between lifelines that all start at the top and run to the bottom:
- there are no combined fragments (`loop`, `alt`, `opt`, …): `Sequence { participants, messages }` can't express them;
- a `create` message is only a dashed arrow labelled «create», and the created object's header still sits at the top;
- lifetimes are wrong: every lifeline runs to the bottom, there is no destruction (✗), and activation bars only
  close on an explicit return (otherwise a 26px stub).

Decisions with the user:
- **Fragments:** `loop`, `alt`, `opt`, `par`, `break`, `critical`, `ref`.
- **Lifeline names:** `l : Loan`. An unnamed object is `: Loan`; actors keep their stick figure and name.
- **Return arrows:** drawn automatically when an activation ends, and labelled with the call's `returns` value.
  Explicit returns from the agent are still honoured, never doubled.
- **Destruction:** a «destroy» message puts a ✗ on the target's lifeline; an object can also end itself.
- **No new UML validation.** The agent must generate correct diagrams. Spide only draws robustly: malformed input
  is ignored, never crashes. The existing check that a method exists stays.

## Approach

### Model (`crates/spide-core/src/model.rs`), backward compatible
- `Lifeline { name, class, multi: bool }`, kept in `Sequence.lifelines` (ordered left → right).
  - `name` is the instance name and the lifeline's id. Messages refer to it in `from`/`to`.
  - It is displayed as `name : Class`, or `: Class` when `name == class`.
  - Actors have `class = name`.
  - `participants: Vec<String>` stays as the ordered ids. Old sequences without `lifelines` mean `name = class = participant`.
  - `multi` displays as `loans : Loan[*]` (a multi-object); it is optional.
- `Fragment { kind, operands: Vec<Operand { guard: Option<String>, first: usize, last: usize }> }`, kept in `Sequence.fragments`.
  - `kind` is `loop | alt | opt | par | break | critical`. `first`/`last` are message indices, inclusive.
  - Nesting is by containment. `Target::Message { index }` keeps working.
- `MessageKind` gains `Destroy`: `from → to` «destroy», with a ✗ at the target. `from == to` means self-termination.
- `MessageKind` gains `Ref`: an interaction use, `ref <sequence name>`. It occupies one row and spans `from`..`to`;
  `method` is the referenced sequence.
- `Sequence::class_of(id) -> &str` resolves a lifeline id to its class.
  - It is used by validation (`store.rs::validate_sequence`: methods are checked on `class_of(to)`; `Ref` and
    `Destroy` are not checked), `scenarios.rs` (packages, layers and aggregates), `impact.rs:197`, the rename op
    (`ops.rs:1044` renames `lifeline.class` too) and the lifeline hover card (`class:<class>`).

### Agent input: a nested tree, stored flat (`crates/spide-core/src/sequence.rs`, new)
- LLMs write nested steps reliably; ranges of indices less so. So `set_sequence` takes:
  ```json
  {"name": "Borrow a book", "use_case": "Borrow book",
   "lifelines": [{"name": "Member"}, {"name": "ctl", "class": "LoanController"}, {"name": "l", "class": "Loan"}],
   "steps": [
     {"from": "Member", "to": "ctl", "method": "borrow", "args": "isbn"},
     {"fragment": "opt", "guard": "copy available", "steps": [
        {"from": "ctl", "to": "l", "method": "Loan", "kind": "create", "returns": "l"}]},
     {"fragment": "alt", "operands": [
        {"guard": "member blocked", "steps": [...]},
        {"guard": "else", "steps": [...]}]},
     {"ref": "Pay fine", "over": ["Member", "ctl"]},
     {"from": "ctl", "to": "l", "kind": "destroy"}]}
  ```
- `flatten(steps) -> (messages, fragments)` stores the tree flat. `to_steps(sequence)` is the inverse, used by
  `get_view kind=sequence` so the agent reads what it writes.
- The old flat `messages` input is still accepted.

### One timeline for canvas and PlantUML (`sequence.rs::timeline(s) -> Vec<Event>`)
- **Events:** `Message(i)`, `Return { from, to, label }` (implicit), `FragmentStart { f, depth }`,
  `OperandStart { f, k, guard }`, `FragmentEnd { f }`, `Activate { lifeline, depth }`, `Deactivate { lifeline }`,
  `Create(lifeline)`, `Destroy(lifeline)`.
- **Call stack:**
  - A sync or create call activates the receiver.
  - Before a message, if its sender isn't on top of the stack, the frames above the sender unwind with implicit
    returns. When the sender isn't on the stack at all, a non-actor sender gets activated first.
  - An explicit return pops up to its sender.
  - At the end of an operand or fragment, activations opened inside it unwind *inside* the frame.
  - At the end of the diagram, everything unwinds.
  - Self-calls nest a bar without drawing a return arrow. Async calls give the receiver a short bar.
- **Malformed input is dropped quietly:** out-of-range or crossing fragments, operands without messages, and
  messages to unknown lifelines.

### Drawing (`crates/spide-diagram/src/views.rs`)
- `sequence_layout` assigns a y to every timeline event. Rows grow for fragment headers, operand guards, implicit
  returns, creation headers and ✗. Column gaps also fit `ref` labels and fragment tags.
- **Lifelines:**
  - Header text is `name : Class`.
  - A created object's header box is drawn at the create row, and the create arrow (dashed, open head, «create»)
    ends on the box's edge.
  - The dashed lifeline runs from the header to the ✗, or to the bottom.
- **Activations** come from Activate/Deactivate. Nested bars are offset by 5px.
- **Returns:** implicit and explicit returns are both dashed with an open arrowhead and an optional value label.
- **Fragments:**
  - The frame spans the lifelines its messages touch (and the labels), inset by nesting depth.
  - It has a pentagon tag (`loop`, `alt`, …) at the top left and `[guard]` under the tag.
  - Alt and par operands are separated by dashed lines with their own `[guard]`.
  - Frames are drawn under the messages.
- **ref:** a frame with a `ref` tag over the covered lifelines, with the referenced name centred in it.
  Clicking it opens that sequence.
- Hits are kept: messages for prune/hover, lifeline headers for the class hover.

### PlantUML (`crates/spide-core/src/plantuml.rs::sequence_diagrams`)
- It is driven by the same timeline:
  - `participant "l : Loan" as l` (`actor` for actors);
  - `create l` before a create message; `destroy l`;
  - `loop`/`alt`/`else`/`opt`/`par`/`break`/`critical … end`;
  - `ref over a, b : name`;
  - explicit `activate`/`deactivate` and `return`-style dashed replies, instead of `autoactivate on`.
- Update the golden file `library-sequence.puml`.

### Agent text
- `tools.rs` `set_sequence` gets the new schema and a description: UML rules (lifeline names, create before use,
  fragments with guards, `else`, ref, destroy). Returns are drawn automatically: give `returns` on the call; explicit
  returns are optional.
- `get_view sequence` returns the steps.
- `prompts::gen_sequence` and `agent/spide-skill.md` get a short section on sequence diagrams.

## Files to modify
- core: `model.rs`, `sequence.rs` (new) and `lib.rs`; `store.rs` (validate via `class_of`); `ops.rs` (rename);
  `scenarios.rs`; `impact.rs`; `tools.rs`; `plantuml.rs`; tests `sequence.rs` (new), `store.rs`, `export.rs`,
  `scenarios.rs`, and the golden `library-sequence.puml`
- diagram: `views.rs` (layout and scene)
- app: `ui/hover_card.rs` (lifeline → class, message labels); `ui/views.rs` (ref click → select that sequence);
  `ui/tests.rs` (rich sequence shot); `prompts.rs`
- agent: `spide-skill.md`
- README: Views → Sequence

## Reuse
- `msg_label` numbering, `sequence_layout` column spacing, the actor glyph, `Hit`/`HitShape::Line` for messages, and
  `banner`/regenerate in `ui/views.rs`.
- The call-stack idea in `scenarios.rs::score`, which moves to `sequence::timeline` and is shared.
- `store.rs::set_use_cases` pattern; `set_sequence` keeps its staleness (`generated_rev`).

## Steps
- [x] Core model: `Lifeline`, `Fragment`, `Destroy`/`Ref`, `class_of`, serde compatibility; rename, validate, scenarios
      and impact via `class_of`.
- [x] `sequence.rs`: `flatten`/`to_steps`, `timeline` (call stack, implicit returns, frame-scoped unwinding,
      create/destroy, robust to malformed input); unit tests.
- [x] `set_sequence` schema and `get_view`; prompt and skill text.
- [x] PlantUML from the timeline; update the golden.
- [x] Canvas: layout from the timeline, lifelines (created header, ✗ end), activations, returns, fragments, ref; hits
      and hover.
- [x] UI: ref click, lifeline hover, test shot of a sequence with loop/alt/opt/par/ref/create/destroy.
- [x] README; full test suite; update the snapshots after a visual check.

## Verification
- Core tests:
  - `flatten`/`to_steps` round-trip;
  - the timeline for: missing returns (implicit returns are produced), an explicit return (not doubled), a self-call,
    async, create (activation from the create row), destroy and self-destroy, a loop whose inner call unwinds inside
    the frame, nested alt inside loop, and malformed ranges (ignored);
  - old JSON with only `participants`/`messages` loads and draws as before;
  - rename of a class updates `lifeline.class`;
  - method validation uses the lifeline's class.
- PlantUML golden reviewed; if `plantuml` is installed, `plantuml_accepts` checks the syntax.
- A headless shot (`target/ui-shots/sequence-uml.png`) of a sequence with `loop`, `alt` (+ `else`), `opt`, `par`,
  `ref`, create, destroy and implicit returns, inspected visually; macOS snapshot updated deliberately.
- `cargo test --workspace`, `npm test`.
