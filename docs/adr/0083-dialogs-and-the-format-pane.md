# 0083: The dialog and pane contract over `UiRequest`

- Status: accepted
- Date: 2026-11-01
- Task: P7-28

## Context

ADR 0028 says a command never opens a dialog: it returns
`Outcome::ui_request` and the dispatcher acts on it. What the *dialog*
does once it is open was never recorded. Task P7-21's Page Setup
invented a pattern for it (the dialog holds typed state and
re-dispatches its own command with the fields as arguments) and task
P7-28's Fill and Line dialogs and Format Shape pane repeat it. The
contract must be one rule, not three implementations guessing.

## Decision

1. **The command is the writer.** A command with a dialog takes the
   dialog's fields as its own arguments and applies them in **one run**
   (one [`Edit`], one undo step — ADR 0040). With no arguments and an
   interactive source it returns the `UiRequest` that names its dialog
   and changes nothing; a programmatic source gets the argument error
   back and never sees a dialog (AGENTS.md section 6).

2. **The dialog is field state, not a writer.** The dialog body is a
   form over the selection anchor's **resolved** cells (the model
   answers the value a shape inherits too). It never touches
   `AppCtx`'s document. OK re-dispatches the command with **every**
   field and closes; Apply does the same and stays open; Cancel closes
   and changes nothing — including after an Apply: the applied run is
   already its own undo step and only later edits are dropped.

3. **A pane applies per committed change.** The Format Shape pane
   draws the same forms without the buttons and re-dispatches the
   command with **one** field per committed change (a combo choice at
   once, a typed value on Enter or when the field loses focus), so one
   user action is one run and one undo step. The pane follows the
   selection anchor: a new anchor re-reads the fields.

4. **One form implementation.** The Fill and Line field sets live in
   `crates/ui/src/format_dialogs.rs`, used by both the dialogs and the
   pane, and reusing the picker tables (`pickers::PATTERNS`,
   `pickers::ARROWS`, the swatch palettes, the weight ladder). Field
   labels and layout are our own (AGENTS.md section 4).

## Consequences

- The undo property "one command run is one undo step" holds for every
  dialog and pane action without extra bookkeeping (P7-28's undo test).
- A dialog can only set what its command's arguments express; adding a
  field means adding an argument first.
- A refused field entry is one status line and no write (the panes'
  ADR 0058 rule); the dialogs keep the message in the form until the
  next accepted value.
- The previews paint the user's own colours as content pixels (a
  tinted textured mesh), like the drawing raster, so they stay outside
  the P7-18 palette rule.

## Alternatives considered

- **The dialog writes the cells itself**: rejected — it breaks ADR
  0028 ("commands never open dialogs; one command writes") and would
  give the CLI and the control channel a different behaviour from the
  UI.
- **One accumulating edit across several Applies**: rejected — every
  Apply must be undoable on its own (goal 4), and the "one command, one
  edit" rule (ADR 0040) already gives that for free.
- **A per-dialog form type**: rejected — the pane and the dialog would
  drift; one form keeps the fields, their validation and their
  arguments in one place.