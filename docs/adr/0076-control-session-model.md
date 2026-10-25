# 0076: The control session model: headless and attached

- Status: proposed
- Date: 2026-10-25
- Task: P9-01

## Context

A remote caller needs a session: a registry, open documents and their
undo stacks. Two cases (ADR 0075):

- **Headless**: a script, a test or an MCP client starts
  `larrygram-cli serve` or `larrygram-cli mcp` and works on documents
  with no window.
- **Attached**: an agent works in the desktop app the user has open, on
  the user's documents, while the user watches (and may edit too).

ADR 0029 already put everything a command touches in `AppCtx`
(`crates/commands`, L4) and made the CLI build its own `AppCtx`. The
desktop app's `AppCtx` lives in `crates/ui`'s `Shell`, owned by the UI
thread, and egui gives no other thread access to it. The two cases must
share one implementation (P9-01 goal; AGENTS.md section 6: one
registry), and remote edits must be undoable in the app like the
user's own.

## Decision

### One executor, two hosts

`crates/control` defines a `Host` trait and an `Executor`:

```rust
pub trait Host {
    fn registry(&self) -> &Registry;
    fn app(&self) -> &AppCtx;                       // read-only
    fn dispatch(&mut self, id: &CommandId, invocation: Invocation)
        -> Result<Outcome, CommandError>;           // the only way to change state
    fn busy(&self) -> Option<String> { None }      // "a drag is in progress"
}

pub struct Executor { policy: Policy, limits: Limits, attached: bool, handles: Handles }
impl Executor {
    pub fn execute(&mut self, host: &mut dyn Host, op: Op) -> Result<OpResult, ControlError>;
}
```

The executor is the one implementation of every operation (ADR 0080).
It reads state through `Host::app` and changes it only through
`Host::dispatch`, so nothing but commands mutates `AppCtx` (ADR 0029).

- **`HeadlessHost`** (`crates/control`): a `Registry` from
  `builtins::registry()` and an `AppCtx` with default settings, in the
  calling process. `dispatch` is `larrygram_commands::dispatch`.
  `HeadlessBackend` pairs it with an executor; the CLI serves it over
  stdio.
- **The app's shell host** (`apps/larrygram`, task P9-10): an adapter
  over `crates/ui`'s `Shell` whose `dispatch` goes through
  `Shell::dispatch` with `Source::Control`, so the canvas, the panes and
  the status bar see the change exactly as they see a menu command.
  `busy` reports an open text editor, a drag or a modal state.

### Attaching: the inbox

The UI thread owns the app's state, so a socket connection never
touches it:

1. The app (task P9-10), when agent control is on, binds the listener
   (ADR 0081) and creates `inbox::channel(waker, limits)`. The waker
   is egui's `Context::request_repaint`.
2. Each connection thread runs the serve loop with a `ControlServer`
   front end over an `InboxSender` backend (a clone per connection).
   `InboxSender::call` sends the `Op` with a reply channel, wakes the
   UI and waits up to `Limits::host_reply_timeout_ms` (30 s); a timeout
   or a closed inbox is `Busy`.
3. Every frame, before drawing, the app calls
   `Inbox::drain(&mut executor, &mut shell_host, max_ops)`. Operations
   run on the UI thread between frames, so a remote edit never races
   a user edit, and the next frame draws the result.

`max_ops` is small (8 per frame) so a burst of requests cannot freeze
the window; the rest wait for the next frame, which the waker has
already requested.

The executor of an attached app is created with `attached: true`. That
flag changes three things only: commands declared `headless_only` are
refused (ADR 0081), the default path roots differ (ADR 0081), and
`ServerInfo::attached` is true.

### Documents: several, by handle

A session holds as many documents as `AppCtx` does (ADR 0029: open
order, one active). Each open document gets a **handle**, `"doc-<n>"`,
minted by the executor in open order and never reused while the
process lives (`inspect::Handles`, synced with `AppCtx::documents`
before every operation). A handle is stable while the document stays
open; the user switching tabs does not change it. This is the MCP
2026-07-28 pattern for state across stateless calls: a server-minted
handle passed back as an ordinary argument.

Every operation takes an optional `document` handle and, where it
matters, a `page` (1-based index or name); without them it works on the
active document and page. An agent working beside a user should pass
both, because the user may switch tabs between two calls.

`commands/invoke` with a `document` or `page` first makes that document
or page active **through the registry**, with two commands task P9-03
adds: `file.activate` (`document`: the 1-based position in open order)
and `page.go` (`page`: a 1-based index, or `name`). The switch is
visible in the attached app, as it would be if the user clicked the
tab; the executor does not switch back.

### Undo sees remote edits as ordinary edits

A remote command dispatches through the registry, so its edit lands on
the document's own `UndoStack` (ADR 0028) with the command's label, one
step per `commands/invoke`. In the attached app the user's Undo undoes
the agent's last step, and an agent can undo with `edit.undo` and
`edit.redo` like anything else. `Invoked::undo_label` and
`DocumentInfo::undo`/`redo` tell the agent what Undo would do. Grouping
several remote calls into one undo step (a transaction) is out of
scope for the alpha.

### What the UI does differently for a remote call

For `Source::Control` and `Source::Mcp`:

- A `NeedsInput` error is returned to the caller; no picker opens.
- A `UiRequest` in the outcome (about, options, welcome, confirm close,
  quit) is **not acted on**; the executor reports it by name in
  `Invoked::ui_request` (ADR 0077).

Both are already true: `Shell::dispatch` asks the picker and
`Shell::apply_outcome` acts on a UI request only for an interactive
source (ADR 0028, P4-16). Task P9-10 keeps them true with tests and
adds one thing: the status bar shows that an agent is connected and
the last remote command, so the user can see who changed the drawing.

### Lifetimes

- Headless: the session ends when the client closes stdin. Unsaved
  documents are dropped with it; there is no autosave. An agent saves
  with `file.save_as`.
- Attached: agent control is **off by default**. The user turns it on
  with a command (`tools.agent_control`, task P9-03, persisted in
  `Settings::agent_control`) or starts the app with `--agent-control`.
  Turning it off, or quitting, stops the listener, closes the
  connections and removes the socket and the discovery file.
  Connections do not survive an app restart; a client reconnects.

## Consequences

- The CLI, the app and the tests run the same executor code; only the
  host differs. The control crate's own tests use `HeadlessHost`.
- `crates/ui` needs small additions (task P9-10): read access to the
  shell's registry and `AppCtx`, `busy()`, the handling of the new
  `UiRequest::AgentControl` and the status-bar line; the host adapter
  dispatches with the existing `Shell::dispatch`. No control types enter `crates/ui`, so the web build
  is untouched.
- `crates/commands` gains `file.activate`, `page.go`,
  `tools.agent_control` and `Settings::agent_control` (task P9-03).
- An operation waits at most one frame plus its own run time in the
  app; a modal state (a dialog open) answers `Busy` instead of
  queueing forever.

## Alternatives considered

- **Share `AppCtx` behind a mutex.** Rejected: egui's `Shell` owns it
  by value on the UI thread; a lock would touch every UI code path and
  a remote edit mid-frame would race the canvas.
- **One document per session.** Rejected: `AppCtx` already holds
  several, the user's app usually has several open, and an agent may
  need a scratch document beside the user's.
- **Index-based document references.** Rejected: indexes shift when a
  document closes; handles do not.
- **Set the active document directly from the executor.** Rejected:
  ADR 0029 lets only commands mutate `AppCtx`; two small commands keep
  that rule and make the switch scriptable from the command bar too.
- **A separate headless app process that the UI also talks to.**
  Rejected: two copies of the state, and the user's open documents
  would not be the agent's.
- **Remote edits on a separate undo stack.** Rejected: the user must be
  able to undo what the agent did with the same Undo.
