# 0080: The MCP server: one implementation, two front ends

- Status: proposed
- Date: 2026-10-25
- Task: P9-01

## Context

KICKOFF.md Phase 9 asks for an MCP server beside the JSON control
channel. MCP clients (agent hosts and coding agents) start servers as a
child process over stdio and call **tools**; a model sees each tool's
name, description and input schema. The two protocols must not become
two implementations of the same features.

The MCP specification's current revision is **2026-07-28** (checked
2026-10-25 against modelcontextprotocol.io). It made MCP stateless:
there is no `initialize` handshake; every request carries its protocol
version, client capabilities and client info in
`_meta["io.modelcontextprotocol/..."]`; servers must implement
`server/discover`; every result carries `resultType`; list results
carry `ttlMs` and `cacheScope`; `ping` and logging are gone; state
across calls is carried in server-minted handles passed as tool
arguments. Clients written against the previous revision (2025-11-25
and earlier) still open with `initialize`, and the revision defines a
"dual-era" server that serves both.

Rust crates for MCP were checked on 2026-10-25:

| Crate | Licence | Maintenance | Shape |
|---|---|---|---|
| `rmcp` 3.5.1 (the official Rust SDK) | Apache-2.0 | active: 3.5.1 on 2026-10-05, 67 releases; implements 2026-07-28 and stays compatible with 2025-11-25 | async on `tokio`, macros, 9 to 37 MB of dependencies (lib.rs) |
| `rust-mcp-sdk` | MIT | active; claims the 2026-07-28 conformance suite | async on `tokio` |

Both licences pass `deny.toml`. Both bring an async runtime the
workspace does not have, for a server that needs two methods (list and
call tools) over a blocking pipe.

## Decision

### No MCP crate: our own small front end

`crates/control/src/mcp.rs` implements the MCP subset Larrygram needs
over the framing and JSON-RPC types it already has (ADR 0075):

- **Modern (2026-07-28)**: `server/discover` (supported versions, the
  `tools` capability, `serverInfo`), `tools/list`, `tools/call`. Each
  request's `_meta["io.modelcontextprotocol/protocolVersion"]` must be
  a supported version, else the MCP `UnsupportedProtocolVersionError`
  (`-32022`, with `data.supported`). Results carry
  `resultType: "complete"` and `_meta["io.modelcontextprotocol/serverInfo"]`;
  `tools/list` carries `ttlMs` (one hour: the list never changes while
  the process lives) and `cacheScope: "private"`.
- **Legacy (2025-11-25, 2025-06-18)**: an `initialize` request selects
  legacy semantics for the rest of the stdio process (the dual-era rule)
  and is answered with the client's version when supported, else the
  newest legacy one, the `tools` capability and `serverInfo`;
  `notifications/initialized` is accepted; `ping` answers `{}`;
  `tools/list` and `tools/call` work as in the modern era, without the
  modern-only fields.
- **Not offered**: resources, prompts, sampling, roots, logging,
  elicitation, tasks, subscriptions, `listChanged`. Requests for them
  are `MethodNotFound`. The server writes nothing to stdout but MCP
  messages and logs only to stderr.

Rejecting `rmcp` is about weight, not quality: a tokio runtime and its
dependency tree for two methods, beside a blocking design (ADR 0076)
that the control protocol needs anyway. If MCP grows features
Larrygram wants (tasks for long renders, subscriptions for document
changes), switching the MCP front end to `rmcp` is a contained change:
the `Backend` seam below does not move.

### One implementation, two front ends

```
            control protocol            MCP
          (ControlServer, 0075)   (McpServer, this ADR)
                     \                 /
                      FrontEnd::handle(&mut dyn Backend, Message)
                                 |
                         Backend::call(Op) -> OpResult
                    /            |              \
       HeadlessBackend      InboxSender      RemoteBackend
     (Executor + Headless   (to the app's    (control protocol
      Host, in-process)      UI thread)       over the socket)
```

- Every feature is an `Op` (`crates/control/src/ops.rs`), executed once
  by the `Executor` (ADR 0076). A front end only translates.
- The control protocol is the `Op` values as JSON-RPC methods; MCP is
  the same values as tools. The MCP tool arguments are the op's params
  struct and the tool result's `structuredContent` is the op's result
  struct, field for field, so the protocol document describes both.
- `larrygram-cli mcp` runs `McpServer` over a `HeadlessBackend`.
  `larrygram-cli mcp --attach` runs `McpServer` over a `RemoteBackend`
  connected to the running app's socket (ADR 0081), so an MCP client
  drives the user's app without the app speaking MCP itself.

### The tools

A fixed list, in this order (`mcp::TOOLS`):

| Tool | Op | Read only |
|---|---|---|
| `list_commands` | `ListCommands` | yes |
| `describe_command` | `DescribeCommand` | yes |
| `run_command` | `Invoke` | no |
| `list_documents` | `ListDocuments` | yes |
| `inspect_document` | `InspectDocument` | yes |
| `inspect_page` | `InspectPage` | yes |
| `inspect_shape` | `InspectShape` | yes |
| `get_selection` | `InspectSelection` | yes |
| `render_page` | `Render` | yes |

Names are `snake_case` ASCII (within the spec's recommended
`[A-Za-z0-9_.-]{1,128}`), without a server prefix: the spec leaves
disambiguation across servers to clients. Each tool has a title, a
description in our own words, an input schema (JSON Schema 2020-12,
every property described, `additionalProperties: false`) and
`annotations.readOnlyHint`.

**Generic tools, not one tool per command.** The registry has 89
implemented commands and 1026 stubs (the parity matrix, 2026-10-25); one tool each would
fill a model's context with tools it never uses, and the tool list
would change whenever a command lands. `list_commands`,
`describe_command` and `run_command` reach every command, and the
command schemas come from the registry (ADR 0077), so there is still
no second list.

### Results and errors

- A successful call returns `structuredContent` (the result struct)
  and one text block with the same JSON (the spec's backward-compatible
  form). `render_page` with an inline PNG returns an image block
  (`type: "image"`, base64 `data`, `mimeType: "image/png"`) and the
  structured result without `data_base64`; an SVG is returned as text.
- An **operation error** (a command failed, a shape was not found, the
  policy denied a path, a limit was hit) is a tool result with
  `isError: true` and the error's line, plus `structuredContent`
  `{"code": ..., "data": ...}`, so the model can correct the call.
- A **protocol error** (an unknown tool, arguments that do not fit the
  schema, a malformed request) is a JSON-RPC error with the ADR 0075
  codes (`-32602` for an unknown tool, as the spec shows).
- `run_command` dispatches with `Source::Mcp`.

## Consequences

- The MCP front end is a few hundred lines with no new dependency and
  no async runtime; the fuzz target covers its parsing with the rest
  (ADR 0082).
- Following the spec is our job: the tests pin the 2026-07-28 shapes
  (`server/discover`, `_meta`, `resultType`, `ttlMs`) and the legacy
  handshake with hand-written fixtures taken from the spec's examples
  (ADR 0082).
- Users configure their MCP client with `larrygram-cli mcp` (headless)
  or `larrygram-cli mcp --attach` (the running app); `docs/agents.md`
  shows both.
- When the spec revises again, only `mcp.rs` changes.

## Alternatives considered

- **`rmcp`.** Rejected for now (above): an async runtime and a large
  tree for two methods. Licence and maintenance are fine; revisit when
  Larrygram needs MCP tasks or subscriptions.
- **`rust-mcp-sdk`.** Rejected for the same reason.
- **Modern only.** Rejected: many clients in use still open with
  `initialize`; the dual-era rules cost one method and a flag.
- **Legacy only.** Rejected: 2026-07-28 is the current revision and
  modern clients probe with `server/discover`.
- **MCP as the only protocol.** Rejected: the control protocol's
  `hello` with a token is what makes the socket safe (ADR 0081), and
  scripts and tests want typed methods, not tool calls.
- **One tool per command.** Rejected (above).
