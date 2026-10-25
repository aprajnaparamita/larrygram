# 0077: The remote command surface

- Status: proposed
- Date: 2026-10-25
- Task: P9-01

## Context

A remote caller must be able to find out what it can do, with which
arguments, and do it. AGENTS.md section 6 and the P9-01 master notes
fix two rules: every action dispatches by command id through the one
registry (`crates/commands`), and **the registry is the single source of
truth for what a remote caller can do: no second list of commands**.

Today a command has an id, a label, a status (implemented or stub), an
enabled predicate and an `execute` that reads `Args` by name (ADR 0028).
The argument names and types live only in each handler's code
(`inv.args.path("path")`, `args.get("shapes")`), so a caller cannot
discover them, and nothing says which commands are unsafe remotely.

## Decision

### The registry declares arguments and remote access (task P9-03)

`crates/commands` gains, in a new module `crates/commands/src/schema.rs`:

```rust
/// One declared argument of a command.
pub struct ArgSpec {
    pub name: &'static str,
    pub kind: ArgKind,
    pub required: bool,
    pub description: &'static str,   // one line, our own words
}

#[non_exhaustive]
pub enum ArgKind {
    Text,
    Path(PathPurpose),               // OpenFile, SaveFile, ChooseFolder (ADR 0036)
    Integer { min: Option<i64>, max: Option<i64> },
    Number { min: Option<f64>, max: Option<f64> },
    Flag,
    List,                            // a list of strings (shape ids, ...)
    Bytes,
    Choice(&'static [&'static str]), // a Text restricted to these values
}

/// Whether a command may run for a remote caller (ADR 0081).
#[non_exhaustive]
pub enum RemoteAccess {
    Allowed,                         // the default
    HeadlessOnly,                    // refused when attached (app.quit, file.exit)
    Never,                           // refused for every remote caller
}
```

and two provided methods on the `Command` trait, so existing
implementors compile unchanged:

```rust
fn args(&self) -> &'static [ArgSpec] { &[] }
fn remote(&self) -> RemoteAccess { RemoteAccess::Allowed }
```

The builtins return their declarations from one table keyed by command
id (`schema.rs`), which P9-03 fills for **every implemented command**.
Two tests keep it honest: every implemented command has an entry (an
empty slice when it takes no arguments), and every argument name a
handler reads is declared (a test drives each command with an
undeclared argument and a source scan of `args.<kind>("<name>")`
calls; P9-03 picks the cheaper of the two that is complete). A stub
declares nothing. `StubCommand` keeps the defaults.

P9-03 also adds the commands ADR 0076 needs (`file.activate`,
`page.go`, `tools.agent_control` with `Settings::agent_control`) through
the extractor's "our own commands" list, as ADR 0028 requires for
Larrygram-only ids, and `SessionId::get` (the number behind a session
id, for logs).

### Listing and describing

- `commands/list` returns `CommandInfo` for the registry's commands in
  id order: `id`, `label`, `implemented`, `enabled` (the predicate's
  answer against the current state), `disabled_reason`, `remote`
  (`allowed`, `headless_only`, `never`) and, with `include_args`, the
  arguments. By default stubs are left out (`include_stubs: true`
  brings them back: an agent should not plan with commands that only
  answer `Unsupported`). `filter` keeps ids that start with it or ids
  and labels that contain it, case insensitive. Paged with the inspect
  cursor (ADR 0078).
- `commands/describe` returns one `CommandInfo` with its arguments:
  name, kind (`text`, `path`, `integer`, `number`, `flag`, `list`,
  `bytes`, `choice`), required, description, the choices, the path
  purpose and the numeric range.

Both are built from `CommandMeta`, `Command::args`, `Command::remote`
and `Registry::enabled` (`crates/control/src/args.rs`, task P9-08):
nothing is listed that the registry does not hold.

### Invoking

`commands/invoke` takes `command`, `args` (a JSON object), optional
`document` and `page` (ADR 0076) and `overwrite` (ADR 0081). The
executor:

1. checks `Host::busy`, resolves the document and page and activates
   them through `file.activate` and `page.go`;
2. checks the command against the policy (`RemoteAccess`, ADR 0081);
3. converts `args` by the declared kinds (`args_from_json`): a string
   becomes `Text`, `Path` or a checked `Choice`; an integer `Integer`;
   a number `Number` (range-checked); a boolean `Flag`; an array of
   strings `List`; a base64 string `Bytes`. An undeclared name, a wrong
   type or a missing required argument is `InvalidParams` naming the
   argument; nothing is dropped silently;
4. checks every `Path` argument against the policy (read for
   `OpenFile`, write for `SaveFile`, directory for `ChooseFolder`);
5. dispatches with `Source::Control` (the MCP front end passes
   `Source::Mcp`) and returns `Invoked`: the message, the warnings, the
   undo label when an edit was recorded, the UI request by name, and
   the active document's info after the run.

### Commands that would open a dialog

A command never opens a dialog (AGENTS.md section 6, ADR 0028). For a
remote caller:

- **Missing input** is `NeedsInput` (`-32003`) with `data` saying what
  is missing (`{"kind": "path", "purpose": "save file"}`); the caller
  calls again with the argument. Because arguments are declared, a
  caller that read `commands/describe` never hits this.
- **A UI request** in the outcome (`ShowAbout`, `ShowOptions`,
  `ShowWelcome`, `Quit`) is reported in `Invoked::ui_request` as
  `about`, `options`, `welcome`, `quit` and never acted on: the call
  succeeds and nothing appears on screen.
- **A confirmation** (`file.close` of a dirty document answers
  `UiRequest::ConfirmClose`) is reported as `confirm_close` and the
  document stays open; the caller closes it with `confirm: true` (the
  existing argument) after saving or deciding to drop the changes.

### What runs remotely

Every implemented command is `Allowed` unless P9-03 marks it. The
alpha marks `app.quit` and `file.exit` `HeadlessOnly` (an agent must not
close the user's app; in a headless session they only report the
request) and `tools.agent_control` `Never` (only the user turns agent
control on or off). P9-03 may mark more and must say why in its
Result. Stubs answer `Unsupported` whatever their access.

## Consequences

- One source: the command bar, the menus, the CLI, the control
  protocol and MCP all see the same commands, and a new command is
  remote-callable as soon as it declares its arguments.
- The argument table is a new obligation on every command author;
  the completeness test makes a missing entry a test failure, not a
  silent gap.
- `ArgKind::Choice` lets a schema say `mode` is `replace`, `add` or
  `toggle`; MCP turns it into a JSON Schema `enum`.
- The parity matrix (ADR 0037) is unaffected: it reads ids and status.

## Alternatives considered

- **A list of remote commands in `crates/control`.** Rejected by the
  master note: a second list drifts from the registry.
- **Argument schemas in `ui-spec/commands.yaml`.** Rejected: the file
  is generated from Microsoft data (ADR 0006) and would need hand
  edits; the arguments belong to our handlers.
- **Infer argument types from the JSON value alone.** Rejected: a
  string could be a path or text, and the policy must know which
  strings are paths.
- **One MCP tool per command.** See ADR 0080.
- **A `RemoteAccess` deny-list keyed by id in the policy.** Rejected:
  the same second-list problem; the command declares its own access.
