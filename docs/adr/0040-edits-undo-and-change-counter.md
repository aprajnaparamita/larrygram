# 0040: Edits, undo and the document change counter

- Status: accepted
- Date: 2026-10-18
- Task: P5-01

## Context

Every Phase 5 action must be undoable (KICKOFF). ADR 0028 defines
`Edit` (`label`, `apply`, `revert` over `&mut AppCtx`) and the
per-document `UndoStack`; Phase 4 produced no edits. The planning brief
(decision 3) asks how commands build `Edit`s, how edits group (one drag =
one undo step), redo, the property test that undo is the inverse of every
command, and two open fixes:

- `builtins::with_undo` swaps the stack out with a **zero-capacity
  placeholder** that an `Edit` running inside `apply`/`revert` can see
  (P4-10/P4-15);
- the canvas detects document changes by comparing a `Document` snapshot
  every frame (P4-05), which needs a change counter instead.

## Decision

### 1. One command, one edit

A command that changes the document returns exactly one
`Outcome::edit`. The building blocks live in `crates/commands`:

- `CellEdit` — one cell write with its old `CellWrite` (ADR 0038)
  (module `builtins::edit`, P5-04);
- `StructuralEdit` — one `DetachedShapes` (ADR 0039) or insertion
  snapshot (module `builtins::edit`, P5-04);
- `CompositeEdit` (public, `commands::edit`) — an ordered list of edits
  under one label. `apply` runs forwards, `revert` backwards.

A drag is one undo step: the tool collects the transient steps and the
command commits one `CompositeEdit` on release. Undo labels are user
phrases ("Move 3 shapes", "Delete shape").

### 2. Undo and redo run with recording suspended

The borrow split stays as it is (the stack lives inside the session while
an `Edit` needs `&mut AppCtx`, so the runner takes the stack out and puts
it back by `SessionId` — the P4-15 fix). What changes is where the
recording rule lives:

- `AppCtx` gains an **undo-suspension flag**, set while an `Edit`'s
  `apply`/`revert` runs.
- `AppCtx::push_edit(session: SessionId, edit: Box<dyn Edit>)` is the
  **only** push path (the dispatcher of `Outcome::edit` uses it). While
  the flag is set it drops the edit and records one warning line on
  `AppCtx::warnings` ("an edit tried to record during undo; step
  dropped"), and it finds the session by `SessionId`, never by index.
- The zero-capacity placeholder stack the runner swaps in is therefore
  inert: nothing can land on it, because every push goes through
  `push_edit`, which is suspended. It stays a deliberate `UndoStack::new(0)`
  and says so in its docs.

A nested command therefore cannot corrupt the cursor or silently lose an
edit into a placeholder (the P4-10/P4-15 open items close); `with_undo`
shrinks to the runner and keeps its `SessionId` put-back.

### 3. The document change counter

`DocumentSession::changes: u64` (ADR 0029 field) is bumped by
`AppCtx::touch()` after **every** document mutation, including undo and
redo and including programmatic sources. It only moves forward
(`wrapping_add`). The canvas cache key (ADR 0033) uses
`(SessionId, changes, page, raster scale)` and drops the per-frame
`Document` snapshot comparison (P4-05 closed). A view-only change (pan,
zoom, toggles) does not bump it.

### 4. The undo inverse property

For every command `c` and state `s`: run `c`, `undo`, and compare the
document to `s` (model `PartialEq`); run `c`, `undo`, `redo`, and compare
to the post-`c` document. The comparison ignores the change counter, the
undo stack and `AppCtx::warnings`. `proptest` generates command sequences
over a small seeded document (task P5-11).

## Consequences

- One drag never shows up as dozens of undo steps; the undo menu reads
  like the user's actions.
- The canvas redraws only when a command really changed something; the
  O(document) frame comparison is gone.
- A command that edits during undo is a visible warning, not a hidden
  dropped stack entry (the P4-15 open issue closes).
- `CompositeEdit` is public so `crates/ui` can build drag steps without
  knowing the internals (L6 sees only `Edit`).

## Alternatives considered

- **Guard with the placeholder stack alone.** Rejected: an edit pushed
  inside `apply` would land on the placeholder and vanish; the recording
  gate on `AppCtx` makes the rule explicit and the loss visible.
- **A generation counter per edit instead of per document.** Rejected:
  the cache needs one comparable number; per-edit granularity buys
  nothing the raster cache can use.
- **Undo as command replay (re-dispatch with inverted args).** Rejected:
  geometry deltas do not invert cleanly (snap decisions, clamping), and
  the property test would test arg parsing, not state.
- **Snapshot-based undo (clone the whole document per step).** Rejected:
  O(document) memory per drag step; `DetachedShapes` and `CellWrite`
  snapshots are the minimal inverse.

## Amendment 2026-10-20 (task P5-19): the selection an undo step leaves

Review F9: `Edit::apply`/`revert` mutate the selection as a side effect.
That behaviour is now a written rule:

- A group or ungroup step **selects the shapes it affected**: the new
  group on a group (and on a redo of an ungroup), its members on an
  ungroup (and on an undo of a group). The ids it selects are the arena
  ids the shapes ended up at — an undo of an ungroup selects the
  restored group at its old arena slot, never the discarded fresh one.
- A page restore (the `page.*` edits) **clears** the selection when the
  active page changed: the selected ids would be stale on the restored
  page.
- A delete step leaves the removed shapes out of the selection (the
  detached ids name shapes no page reaches).

The rule is pinned by
`undo_steps_select_their_shapes_and_a_page_restore_clears_the_selection`
(`crates/commands/tests/arrange.rs`).

## Amendment 2026-11-03 (task P7-37): undo groups

Rule 1 stays: each command still returns one edit. An **undo group**
lets the shell fold the edits of several commands into one undo step,
for an interaction that spans commands but is one act for the user.
The first user is a new text box that is formatted while it is typed
(P7-37): the insert, the format commands and the final text write are
one step.

- `UndoStack::begin_group(label)` opens a group. While it is open,
  `push` appends to the group instead of the stack. Groups do not nest:
  a second `begin_group` is an error.
- `end_group()` pushes the collected edits as one `CompositeEdit`
  under the label. An empty group pushes nothing.
- `cancel_group(ctx)` reverts the collected edits in reverse order,
  with recording suspended (rule 2), and discards them. The document
  is as it was when the group opened, and its unsaved-changes state is
  restored too. The change counter still only moves forward (rule 3).
- `undo` and `redo` end an open group first.
- Only the shell opens a group, and it always closes it, with
  `end_group` or `cancel_group`, when the interaction ends by any path:
  commit, Escape, a click elsewhere (P7-25), a layout switch, closing
  the document. Programmatic sources never see an open group; a
  command from the control channel ends it first.
