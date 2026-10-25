# 0075: Control channel transport and framing

- Status: proposed
- Date: 2026-10-25
- Task: P9-01

## Context

KICKOFF.md Phase 9 asks for a JSON control channel and an MCP server
that expose every registered command, document inspection and
render-to-PNG, so that an agent can drive Larrygram and check its own
work. ADR 0028 already made every user action a registry command that
takes its input as `Args` and never opens a dialog, and ADR 0029 put
all state a command touches in `AppCtx`, below the UI. What is left is
the wire: how bytes get to the registry, how requests and answers are
framed and matched, how errors are reported, and how the two sides
agree on a version.

There are two kinds of caller:

- A **script or test** that starts Larrygram itself and talks over the
  child's standard streams: no window, one process per session.
- An **agent beside the user's running app**, which must reach the
  app's own state without restarting it.

MCP 2026-07-28 (the current revision, published 2026-07-28) frames its
stdio transport as newline-delimited JSON-RPC 2.0 and recommends the
same framing for custom transports over Unix domain sockets or TCP.

## Decision

### Two transports, one framing

| Transport | Used by | How it starts |
|---|---|---|
| **stdio** | `larrygram-cli serve` (control protocol) and `larrygram-cli mcp` (MCP), headless | the client starts the process; the session ends when stdin closes |
| **local socket** | the desktop app with agent control turned on (ADR 0076) | the app listens; clients find it through the discovery file (ADR 0081) |

The local socket is a Unix domain socket on macOS and Linux and a TCP
socket bound to `127.0.0.1` on an ephemeral port on Windows (the
standard library has no named pipes, and no extra crate is needed for
either). There is no listener on any other address (ADR 0081).

Both transports carry the same framing: **one JSON-RPC 2.0 message per
line**, UTF-8, terminated by `\n`, with no newline inside a message (a
trailing `\r` is tolerated and dropped). This is the MCP stdio framing
unchanged, so one reader and one writer (`crates/control/src/framing.rs`)
serve both protocols on both transports. A line longer than
`Limits::max_frame_bytes` (4 MiB) is discarded up to its newline and
answered with a `LimitExceeded` error; the stream stays usable.

### JSON-RPC 2.0, not a smaller framing

The control protocol is JSON-RPC 2.0:

- **Requests** have an `id` (a number or a string; `null` and
  fractional ids are refused) and get exactly one response with the
  same `id`.
- **Notifications** have no `id` and get no answer. The server accepts
  and ignores them, `notifications/cancelled` included: requests run
  one at a time per connection and are short, so cancellation has
  nothing to stop (a later version may honour it for renders).
- **Batches** (JSON arrays) are refused with `InvalidRequest`; MCP
  dropped them and neither front end needs them.
- The server never sends requests (as MCP 2026-07-28 requires of
  servers), and in version 1.0 it sends no notifications.
- One request at a time per connection: the serve loop reads, answers,
  then reads the next. A client may pipeline requests; they are
  answered in order.

A smaller framing (bare `{"command": ...}` lines) was the alternative.
JSON-RPC costs a few bytes per message and buys request ids, a standard
error object and the same parser as MCP.

### Methods

The control protocol's methods are the typed operations of
`crates/control/src/ops.rs` (ADR 0080), one method per operation, with
the operation's params struct as `params` and its result struct as
`result`:

| Method | Params | Result | ADR |
|---|---|---|---|
| `hello` | `Hello` | `HelloResult` | this ADR |
| `server/discover` | none | `ServerInfo` | this ADR |
| `commands/list` | `ListCommands` | `CommandList` | 0077 |
| `commands/describe` | `DescribeCommand` | `CommandInfo` | 0077 |
| `commands/invoke` | `Invoke` | `Invoked` | 0077 |
| `documents/list` | none | `DocumentList` | 0076 |
| `inspect/document` | `DocumentQuery` | `DocumentView` | 0078 |
| `inspect/page` | `PageQuery` | `PageView` | 0078 |
| `inspect/shape` | `ShapeQuery` | `ShapeView` | 0078 |
| `inspect/selection` | `DocumentQuery` | `SelectionView` | 0078 |
| `render/page` | `Render` | `Rendered` | 0079 |

Params refuse unknown fields (`InvalidParams`), so a misspelt option is
an error, not a silent default. Field names are `snake_case`.

### Versioning and capability negotiation

The control protocol is versioned `major.minor`, starting at **1.0**.
Within a major version fields and methods may be added, never removed
or retyped; a client ignores fields it does not know. A client and a
server agree when their majors match.

- On a **socket**, the first request must be `hello`:
  `{"versions": ["1.0"], "token": "<64 hex>", "client": "my-agent"}`.
  The server picks the newest offered version whose major it speaks
  and answers `{"version": "1.0", "server": ServerInfo}`; with no match
  it answers `UnsupportedVersion` with the supported list in `data`.
  Any other request before a successful `hello` is `NotReady`; a wrong
  token is `Unauthorized` and the connection is closed.
- On **stdio**, `hello` is optional (the parent process is trusted,
  ADR 0081) and version 1.0 applies until it is sent.
- `server/discover` can be called at any time (after `hello` on a
  socket) and returns `ServerInfo`: name, Larrygram version, the
  protocol versions, `attached` (true for the desktop app, ADR 0076),
  the capabilities (`commands`, `inspect`, `render_formats`) and the
  limits a client should respect.

Capabilities are announced, not negotiated: the client reads them and
adapts. There is nothing for a client to switch on or off in 1.0.

### Errors

An error response carries `code`, a one-line `message` and, where it
helps, `data`. The standard codes keep their JSON-RPC meaning; the
Larrygram codes sit in `-32000..=-32019`, the range MCP leaves to
implementations (it reserves `-32020..=-32099`):

| Code | Name | When |
|---|---|---|
| -32700 | ParseError | not JSON |
| -32600 | InvalidRequest | JSON but not a valid request (also batches) |
| -32601 | MethodNotFound | unknown method |
| -32602 | InvalidParams | params or command arguments do not fit |
| -32603 | InternalError | a server fault, an I/O failure |
| -32000 | CommandFailed | the command ran and failed (`CommandError::Failed`) |
| -32001 | UnknownCommand | no command has this id |
| -32002 | NotEnabled | the command is disabled now (`data`: none; the message has the reason) |
| -32003 | NeedsInput | a required argument is missing (`data`: kind, purpose or prompt) |
| -32004 | Unsupported | registered but not implemented yet |
| -32005 | Denied | the policy refuses (ADR 0081) |
| -32006 | LimitExceeded | a limit was hit (`data`: limit, value, max) |
| -32007 | NotFound | no such document, page, shape or cursor |
| -32008 | NoDocument | the operation needs an open document |
| -32009 | Unauthorized | wrong or missing token |
| -32010 | NotReady | `hello` first |
| -32011 | UnsupportedVersion | no common version (`data`: requested, supported) |
| -32012 | Busy | the app cannot take it now, or did not answer in time (ADR 0076) |
| -32013 | NotImplemented | a Phase 9 stub (gone at the end of the phase) |

The table is `ErrorCode` in `crates/control/src/error.rs`. A parse
failure whose id could still be read is answered with that id;
otherwise the id is `null`.

## Consequences

- One framing module and one JSON-RPC module serve both protocols and
  both transports; the fuzz target (ADR 0082) covers them once.
- No async runtime: a blocking reader per connection and one thread
  per socket client are enough for a local, low-rate channel.
- Scripts in any language can drive Larrygram with a JSON library and
  a pipe; the protocol document (`docs/control-protocol.md`) is the
  reference.
- Windows uses a loopback TCP port, so the token (ADR 0081) is the only
  thing that keeps other local users out there; on Unix the socket's
  directory permissions do that as well.

## Alternatives considered

- **HTTP on localhost.** Rejected: a listener any local process or a
  browser page can reach, an HTTP stack to depend on, and nothing the
  line framing does not already do.
- **Length-prefixed frames.** Rejected: not what MCP uses, harder to
  type by hand and to read in a log.
- **Named pipes on Windows.** Rejected for the alpha: they need a crate
  or `unsafe` code; loopback TCP with the token is enough until a
  Windows build ships.
- **The socket only, no stdio.** Rejected: tests and scripts want a
  child process they own, and MCP clients start servers over stdio.
- **gRPC or Cap'n Proto.** Rejected: code generation and a large
  dependency for a channel that carries a few requests per second.
