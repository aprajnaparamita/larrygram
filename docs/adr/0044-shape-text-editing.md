# 0044: Text editing on shapes

- Status: accepted
- Date: 2026-10-18
- Task: P5-01

## Context

Phase 5 edits shape text in place (KICKOFF; planning brief decision 7).
The model keeps text as `TextContent`: mixed parts with `cp`/`pp`/`tp`
run markers carrying row `IX` values (ADR 0019). A file's text can have
many runs with different fonts and sizes; a naive editor destroys that.
ADR 0036 defines a `Text` shortcut scope that Phase 4 never activates.

## Decision

### The editor

`crates/ui::text_edit::TextEditor` overlays an egui text field on the
shape's text block (positioned from the scene's `TextNode` bounds; the
canvas supplies the screen rect). It opens on:

- double-click with the pointer tool (the shape under the cursor), or
- a click with the `Text` tool (which creates a text box first), or
- the `text.begin` command.

Escape or a click outside **commits** and closes (matching the common
Visio behaviour; the black-box check records it), `text.cancel` (the
command, bound to a scoped shortcut) discards. The `Text` shortcut scope
is active while the editor is open (ADR 0036): typing and egui's own
editing chords go to egui; other bound chords dispatch as usual.

### Writing text back

Phase 5 writes the buffer as **one text run**: the literal text plus the
first `Character`/`Paragraph`/`Tabs` row markers (`IX` 0 when the shape
has rows). A shape's existing multi-run formatting collapses to the first
run's — a documented limit of this phase (the run-aware editor needs run
ranges in the buffer and is deferred to a later task; the model keeps
`TextContent` unchanged, so nothing is lost at the API level).

`shape.set.text` (`text` argument) is the command; it returns one
undoable edit whose revert restores the old `TextContent` verbatim
(ADR 0040). The write goes through `Document::set_shape_text` (ADR 0039).

### New text boxes

The `Text` tool creates a shape with no fill, no line and a text block
(`insert.drawing.text_box`, ADR 0046's insert family) and opens the
editor on it immediately. Text-only shapes are ordinary shapes with
`NoFill`/`NoLine` cells; nothing new in the model.

## Consequences

- Text editing works end to end for the single-run case (flowchart
  labels), which is the exit criterion's need.
- Multi-run text degrades visibly but predictably; the limitation is in
  the ADR and the task acceptance criteria, not hidden.
- The `Text` scope finally activates (ADR 0036), which also closes the
  P4-08 note about `Ctrl+Backspace` deferring to egui inside edits.

## Amendments

### Amended 2026-11-02, P7-26: the new text box is inserted on commit

The `Text` tool no longer "creates a shape … and opens the editor on it
immediately". The box is **deferred** (task P7-25 item 9): a click on
empty page only records where the box goes (`text::PendingBox`) and
opens the editor over that rectangle while no shape exists yet. On
commit the box and its text are inserted **in one undo step**:
`insert.drawing.text_box` takes a `text` argument and writes the shape
and its text inside one `CompositeEdit`, so one undo removes the new
text object with its text (the P7-25 exit rule). A commit whose insert
fails leaves nothing behind (review B5: `insert_drawing` stages its
undo record and rolls the shape back out of the document on an error).

While the box is being typed, a click **inside** it is a caret move
that keeps editing (task P7-26 item 3); any other click commits the
text first and then decides the click, and Escape commits as before.

## Alternatives considered

- **A run-aware editor now.** Rejected for the alpha: run ranges over an
  egui `TextEdit` buffer, re-inserting `cp`/`pp` markers on commit, is a
  task of its own and the exit criterion does not need it.
- **A plain dialog box for text.** Rejected: not Visio-like; the exit
  flowchart test expects in-place editing.
- **Writing each paragraph as its own run.** Rejected: it invents
  formatting the user did not ask for; one run is the honest collapse.
- **Storing text as `TextContent` with a single part only.** Rejected:
  the writer and reader already handle markers; collapsing is a display
  policy of the editor, not a model change.
