# Phase 9 plan: agents (the control channel and MCP)

- Status: proposed (written by task P9-01, 2026-10-25); the master
  accepts it at merge
- Inputs: KICKOFF.md (Phase 9), AGENTS.md sections 6 and 7, ADRs 0002,
  0006, 0026, 0028, 0029, 0036, 0048, the registry
  (`crates/commands`), the shell (`crates/ui/src/shell.rs`), the CLI
  (`apps/larrygram-cli`), the renderer (`crates/render`), the Phase 5
  flowchart test (`apps/larrygram/tests/flowchart.rs`), and the MCP
  specification revision 2026-07-28 (modelcontextprotocol.io, read
  2026-10-25)

## 1. Scope

Larrygram gets a **JSON control channel** and an **MCP server** that
expose every registered command, document inspection and
render-to-PNG, so an agent can drive the app and check its own work
(KICKOFF.md). Implementation starts after R7 (the `.dmg`); see
`docs/STATUS.md`.

**Deliverables:**

| Deliverable | Where | ADR |
|---|---|---|
| The control protocol: JSON-RPC 2.0, one message per line, over stdio and a local socket; `hello`, versions, capabilities, the error table | `crates/control` (`jsonrpc`, `framing`, `serve`, `protocol`) | 0075 |
| The session model: one `Executor` over a `Host`; the headless host; attaching to the running app through an inbox on the UI thread; documents by handle; remote edits on the normal undo stack | `crates/control` (`exec`, `host`, `inbox`), `crates/ui`, `apps/larrygram` | 0076 |
| The command surface: argument schemas and remote access declared by each command in the registry; list, describe, invoke; no dialogs | `crates/commands` (`schema`), `crates/control` (`args`) | 0077 |
| Document inspection as stable, paged JSON: documents, pages, shapes, connections, cells, selection | `crates/control` (`inspect`) | 0078 |
| Render-to-PNG (and SVG) of a page or a region, inline or to a file, with size caps | `crates/control` (`render`), `crates/render` (regions) | 0079 |
| The MCP server: dual-era (2026-07-28 and the legacy handshake), nine tools over the same operations; no MCP crate | `crates/control` (`mcp`) | 0080 |
| Security: local only, opt-in in the app, token on the socket, remote access per command, path roots, write extensions, overwrite flag, limits | `crates/control` (`auth`, `transport`, `policy`, `limits`) | 0081 |
| The CLI front: `larrygram-cli serve`, `larrygram-cli mcp`, `mcp --attach`, `--allow-dir` | `apps/larrygram-cli` | 0076, 0080 |
| Tests: protocol transcripts, property tests, the fuzz target, the end-to-end MCP flowchart | `crates/control/tests`, `crates/control/fuzz`, `apps/larrygram-cli/tests` | 0082 |
| Documentation | `docs/agents.md`, `docs/control-protocol.md` | section 7 |

**Out of scope for the alpha** (each is a later ADR if wanted):

- Any network listener, HTTP transport (MCP Streamable HTTP), TLS, or
  access from another machine.
- MCP resources, prompts, sampling, roots, logging, elicitation,
  tasks, subscriptions and list-changed notifications.
- Server-to-client notifications of document changes; request
  cancellation (`notifications/cancelled` is accepted and ignored).
- Transactions: grouping several remote calls into one undo step.
- Per-tool confirmation prompts in the app; an audit log beyond the
  status bar.
- Windows named pipes (Windows uses loopback TCP with the token).
- The control channel in the web build (wasm): `crates/control` is not
  a dependency of `crates/ui` or `apps/larrygram-web`.
- Rendering off the UI thread in the attached app.
- JSON-RPC batches.

## 2. Architecture

```
 MCP client (stdio)        script / test (stdio)        agent beside the app
        |                          |                              |
 larrygram-cli mcp          larrygram-cli serve       larrygram-cli mcp --attach
        |                          |                              |
   McpServer                 ControlServer                   McpServer
        \                          |                              |
         \                         |                        RemoteBackend
          HeadlessBackend <--------+                              |
          (Executor + HeadlessHost)                    local socket + token
                                                                  |
                                              desktop app (agent control on)
                                              Listener -> ControlServer
                                                  -> InboxSender
                                              UI thread: Inbox::drain
                                                  -> Executor + shell host
```

- **Operations** (`ops.rs`): ten typed operations; the only list of what
  a remote caller can ask for besides the registry's own commands.
- **Executor** (`exec.rs`): the one implementation. Reads `AppCtx`
  through `Host::app`; changes it only through `Host::dispatch`, that
  is, through the registry (ADR 0029).
- **Front ends** (`protocol.rs`, `mcp.rs`): translate messages to
  operations and results back; no feature logic.
- **Backends** (`backend.rs`, `inbox.rs`, `remote.rs`): where the
  operation runs: in process, on the app's UI thread, or across the
  socket.

The P9-01 stubs freeze every public type and signature in
`crates/control`. `crates/control/src/lib.rs` and
`crates/control/Cargo.toml` `[dependencies]` belong to no Phase 9 task:
a task that needs a change there lists it under Open issues and the
master makes it.

## 3. ADRs

| ADR | Topic |
|---|---|
| 0075 | transport and framing; JSON-RPC 2.0; methods; versioning and capabilities; the error table |
| 0076 | the session model: headless and attached, the inbox, documents by handle, undo |
| 0077 | the remote command surface: argument schemas and remote access in the registry; list, describe, invoke; dialogs |
| 0078 | document inspection as stable JSON; identity; paging and filters |
| 0079 | render-to-PNG: params, regions, limits, inline or file |
| 0080 | the MCP server: protocol revisions, no MCP crate, one implementation and two front ends, the tools |
| 0081 | security: reach, token, remote access, paths, limits, never crash |
| 0082 | the Phase 9 test strategy |

## 4. Tasks and order of work

Parallel work is the priority. A task starts as soon as the tasks it
depends on are merged; tasks that run at the same time own disjoint
files.

| Task | Role | Owns (summary) | Depends on | Wave | Tier |
|---|---|---|---|---|---|
| [P9-02](../tasks/P9-02.md) | implementer | `crates/control/src/{jsonrpc,framing,serve,protocol}.rs` | P9-01, R7 | A | medium |
| [P9-03](../tasks/P9-03.md) | implementer | `crates/commands/**` (argument schemas, remote access, `file.activate`, `page.go`, `tools.agent_control`), the extractor's own-commands list, the regenerated `commands.yaml` and parity matrix | P9-01, R7 | A | large |
| [P9-04](../tasks/P9-04.md) | implementer | `crates/control/src/inspect.rs` | P9-01, R7 | A | medium |
| [P9-05](../tasks/P9-05.md) | implementer | `crates/control/src/render.rs`, `crates/render/src/region.rs` (new) and its two `lib.rs` lines | P9-01, R7 | A | medium |
| [P9-06](../tasks/P9-06.md) | implementer | `crates/control/src/{auth,transport,inbox}.rs` | P9-01, R7 | A | large |
| [P9-07](../tasks/P9-07.md) | implementer | `crates/control/src/mcp.rs` | P9-01, R7 | A | large |
| [P9-08](../tasks/P9-08.md) | implementer | `crates/control/src/{exec,args,policy,host,backend}.rs` | P9-03, P9-04, P9-05 | B | large |
| [P9-09](../tasks/P9-09.md) | implementer | `apps/larrygram-cli/**` (`serve`, `mcp`, `--attach`, `--allow-dir`), `crates/control/src/remote.rs` | P9-02, P9-06, P9-07, P9-08 | C | medium |
| [P9-10](../tasks/P9-10.md) | implementer | `crates/ui/src/shell.rs` (the remote seams), `apps/larrygram/**` (the listener, the shell host, the flag) | P9-03, P9-06, P9-08 | C | large |
| [P9-11](../tasks/P9-11.md) | test-engineer | `crates/control/tests/**`, `crates/control/fuzz/**`, `apps/larrygram-cli/tests/mcp_flowchart.rs`, `crates/control/Cargo.toml` `[dev-dependencies]` | P9-09, P9-10 | D | large |
| [P9-12](../tasks/P9-12.md) | implementer | `docs/agents.md`, `docs/control-protocol.md` | P9-09, P9-10 | D | medium |
| [P9-13](../tasks/P9-13.md) | reviewer | `docs/reviews/phase-9-review.md` | P9-11, P9-12 | E | large |
| [P9-14](../tasks/P9-14.md) | coverage-auditor | `docs/reviews/phase-9-coverage.md` | P9-11, P9-12 | E | medium |
| [P9-15](../tasks/P9-15.md) | cleanroom-auditor | `docs/reviews/phase-9-cleanroom.md` | P9-11, P9-12 | E | medium |
| [P9-16](../tasks/P9-16.md) | implementer | the audit fixes, `docs/progress/phase-9.md` | P9-13, P9-14, P9-15 | F | large |

```
P9-01 (this plan, the ADRs, the stubs, the task files) ─ merge
R7 (.dmg) ─ Phase 9 starts
wave A (6 parallel, disjoint files):
  P9-02 the wire ‖ P9-03 the registry's schemas ‖ P9-04 inspection ‖
  P9-05 rendering ‖ P9-06 socket, token, inbox ‖ P9-07 MCP
wave B (the integration point):
  P9-08 the executor, arguments and policy (after P9-03, P9-04, P9-05)
wave C (2 parallel):
  P9-09 the CLI front (after P9-02, P9-06, P9-07, P9-08) ‖
  P9-10 attaching the app (after P9-03, P9-06, P9-08)
wave D (2 parallel):
  P9-11 protocol tests, fuzz, end-to-end ‖ P9-12 documentation
wave E (3 parallel):
  P9-13 review ‖ P9-14 coverage ‖ P9-15 clean room
wave F:
  P9-16 fixes, phase report, tag phase-9
```

Notes:

- **Wave A codes against the frozen seams** (the P7-01 and P8-01
  pattern). P9-02, P9-06 and P9-07 test with fake `Backend` values;
  P9-04 and P9-05 with hand-built `AppCtx` and `Document` values.
  None of them may write a test that depends on another wave A task's
  stub answering `NotImplemented`: when that task merges, the test
  must still pass. (P9-06's inbox tests compare the inbox's answer
  with a direct `Executor::execute` on the same host, whatever that
  answer is.)
- **P9-08 integrates.** It is the first task that runs operations end
  to end (headless) and owns the executor's tests over real commands,
  inspection and rendering. It may start once P9-03 merges if P9-04
  and P9-05 are close; its inspect and render arms then wait for them.
- **P9-03 changes the registry for everyone.** `Command::args` and
  `Command::remote` are provided methods, so existing implementors
  compile unchanged; the new commands go through the extractor's own
  list and `cargo xtask codegen` (ADR 0028). It is the only wave A task
  outside `crates/control` and `crates/render`.
- **P9-10 is the only task in `crates/ui` and `apps/larrygram`.** It
  must not add `larrygram-control` to `crates/ui` (the web build
  depends on `crates/ui`); the control types stay in `apps/larrygram`.
- **The three audits** follow the Phase 6 to 8 pattern: review,
  coverage and clean-room reports under `docs/reviews/`, then one fix
  task.

## 5. Machine load

Phase 9 starts after Phases 7 and 8 have finished, so the twelve-worker
cap (2026-10-23) has room: all of wave A (six workers) may run at once.
Later waves are small (one, two, two, three, one). The large-tier tasks
in wave A (P9-03, P9-06, P9-07) should start first if slots are short.
As before: check for orphaned processes before blaming a slow worker.

## 6. Exit criteria

1. **The end-to-end MCP flowchart passes in the gate**: the
   `larrygram-cli mcp` binary, started as a child process, builds the
   Phase 5 flowchart over MCP (ten shapes, five glued connectors, two
   pages), saves it, reopens it and checks it by inspection and by a
   rendered PNG, in both MCP eras (ADR 0082).
2. **Protocol tests** cover every control-protocol method, every MCP
   method and tool, and every error code; the fuzz target builds and a
   local run of at least ten minutes finds nothing (ADR 0082).
3. **Every implemented command** declares its arguments and remote
   access; `commands/list`, `commands/describe` and `commands/invoke`
   work for all of them; no remote call opens a dialog or acts on a UI
   request (a test with a picker that fails when called).
4. **Attaching works**: with agent control on, a socket client with
   the token drives the app; the change shows in the next frame and the
   user's Undo reverts it; with it off (the default) there is no
   listener (ADR 0076, `egui_kittest`).
5. **Security**: the policy tests (roots, write extensions, overwrite,
   `headless_only`), the limits and the token checks pass; there is no
   listener on any address but the socket or `127.0.0.1` (ADR 0081).
6. **Documentation**: `docs/agents.md` and `docs/control-protocol.md`
   are complete (section 7). The user's manual check with a real MCP
   client, headless and attached, is recorded in the phase report.
7. **Audits**: the review, coverage and clean-room audits are clean or
   their findings fixed; `larrygram-control` reaches 90% line coverage
   and the master sets its ratchet.

## 7. Documentation outline

P9-12 writes both documents from the merged code; the implementation
tasks put what P9-12 needs in their Results (flags, examples, error
lines). Our own words throughout; the MCP specification is linked,
never quoted at length (AGENTS.md section 4).

### `docs/agents.md`: driving Larrygram from an agent

| Section | What it holds |
|---|---|
| What you can do | One paragraph: commands, inspection, rendering; headless or attached; what is out of scope (section 1 of this plan). |
| Quick start: MCP, headless | The client configuration for `larrygram-cli mcp` (command, arguments, working directory), and a first session: list commands, create a drawing, render it. |
| Quick start: MCP, attached to the app | Turning agent control on (the command and `--agent-control`), the status bar, `larrygram-cli mcp --attach`. |
| Quick start: the control protocol | `larrygram-cli serve` from a script: one `hello`, one `commands/invoke`, one `render/page`, in a few lines of shell or Python. |
| The tools | The nine MCP tools: what each is for, its arguments, an example call and result. |
| Finding and running commands | `list_commands`, `describe_command`, `run_command`; argument kinds; documents and pages by handle and index; missing input; UI requests. |
| Reading the drawing | The four inspection views, shape ids, units, paging and filters, cells on request. |
| Checking your work | `render_page`: scale, regions, limits, inline or file. |
| Undo and the user | One undo step per command; the user's Undo; `edit.undo`; working beside a user who edits too. |
| Files and safety | Roots and `--allow-dir`, write extensions, `overwrite`, `headless_only` commands, the token; what an agent cannot do. |
| Worked example: a flowchart | The end-to-end test's steps as a narrated session. |
| Troubleshooting | The common error lines (`NotReady`, `Unauthorized`, `Denied`, `Busy`, `LimitExceeded`, `NeedsInput`) and what to do. |

### `docs/control-protocol.md`: the protocol reference

| Section | What it holds |
|---|---|
| Overview | The two protocols over one set of operations; the transports; the version (1.0). |
| Transports and framing | stdio, the socket per platform, one JSON message per line, the frame cap. |
| Discovery and authentication | The runtime directory, `control.json` and its fields, the token, `hello`. |
| Messages | JSON-RPC 2.0 as used: ids, notifications, no batches, ordering. |
| Versioning and capabilities | `major.minor`, the compatibility rules, `server/discover`, `ServerInfo`. |
| Methods | One subsection per method: params and result field by field (types, units, defaults), one example request and response. |
| The inspection JSON | `DocumentInfo`, `PageSummary`, `ShapeSummary`, `ConnectView`, `CellView`, `SelectionView`; identity rules; cursors. |
| Command arguments | The argument kinds and their JSON forms; `CommandInfo` and `ArgInfo`. |
| Errors | The full code table with `data` shapes and the one-line messages. |
| Limits | The `Limits` table and what happens at each. |
| The policy | Remote access, roots, write extensions, overwrite. |
| MCP mapping | The two eras and their methods; the tool table with each tool's op; how results and errors map (`structuredContent`, image content, `isError`). |
| Stability promise | What may change within 1.x and what may not. |
