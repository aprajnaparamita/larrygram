# 0081: Control channel security

- Status: proposed
- Date: 2026-10-25
- Task: P9-01

## Context

The control channel lets another program drive Larrygram: open and
save files, change drawings, render images. The callers are agents,
which act on text they read, so a prompt injection in a document or a
web page can make an agent call a tool it should not. The app runs with
the user's permissions. The KICKOFF and P9-01 master notes fix the
frame: everything runs on the user's machine, no hosted services.

What must hold:

1. Nobody but the user can reach the channel.
2. A caller cannot make Larrygram read or write files it should not,
   in particular overwrite a file that is not a drawing or an image.
3. A caller cannot close the user's app or lose the user's work.
4. No input can crash the app, exhaust its memory or freeze it.

## Decision

### Reach: local only, opt-in, authenticated

- **No network listener.** The alpha has no option to listen on
  anything but a Unix domain socket or `127.0.0.1` (ADR 0075). A
  network listener would need its own ADR (TLS, authentication beyond
  a file token) and is out of scope.
- **Headless sessions** listen on nothing: `larrygram-cli serve` and
  `larrygram-cli mcp` speak over the stdio of the process that started
  them, which already runs as the user. No token.
- **The desktop app** listens only when the user turns agent control
  on (`tools.agent_control`, persisted in `Settings::agent_control`,
  default off) or starts it with `--agent-control`. The status bar
  shows when it is on and when a client is connected.
- **The socket.** On macOS and Linux a Unix domain socket in the
  per-user runtime directory (`$XDG_RUNTIME_DIR/larrygram` on Linux, the
  per-user `$TMPDIR/larrygram` on macOS), created with mode `0700`; the
  socket file is `0600`. On Windows a TCP socket on `127.0.0.1` with an
  ephemeral port, and the discovery file in
  `%LOCALAPPDATA%\Larrygram`.
- **The token.** At start the listener draws 32 bytes from the
  operating system's random source (`getrandom`) and writes them, as 64
  hex digits, with the endpoint and the process id into
  `control.json` in the runtime directory, readable by the user only
  (`0600`; on Windows the user's own local-app-data directory). A
  client must send the token in `hello` before anything else; the
  comparison takes constant time; a wrong token is `Unauthorized` and
  the connection closes. The token changes at every start, never
  appears in logs or `Debug` output, and the file is removed when the
  listener stops. A file left by a crashed app names a dead process;
  the next start replaces it.

### What a remote caller may run

- **Commands** run only through the registry and only if their own
  declaration allows it (ADR 0077): `RemoteAccess::Allowed` runs,
  `HeadlessOnly` is refused when attached (`app.quit` and `file.exit`
  in the alpha: an agent must not close the user's app), `Never` is
  refused for every remote caller (`tools.agent_control`: an agent
  cannot turn agent control on or off). Refusals are `Denied` with the
  reason.
- **No dialogs, no UI side effects**: a remote call never opens a
  dialog and its `UiRequest` is reported, not acted on (ADR 0077). A
  dirty document is never closed without `confirm: true`.

### Paths

Every `Path` argument (by its declared kind, ADR 0077) and every render
`output` goes through the policy (`crates/control/src/policy.rs`)
before the command runs:

- **Roots.** The path, resolved (symbolic links followed; a file that
  does not exist yet is resolved by its parent directory, which must
  exist), must lie inside one of the policy's roots. Headless default:
  the process's working directory. Attached default: the user's home
  directory. Both add roots with `--allow-dir <dir>` (the CLI flags and
  the app flag). A path outside every root is `Denied`.
- **Writes.** A file written (a `SaveFile` path, a render `output`)
  must have one of the extensions `vsdx`, `png`, `svg`: Larrygram
  writes drawings and images and nothing else, so an agent cannot be
  led to overwrite a shell profile or a key file. `ChooseFolder`
  outputs (`file.export`) must be a directory inside a root; the files
  the command writes there are checked by name the same way.
- **Overwrites.** A write to an existing file is `Denied` unless the
  call sets `overwrite: true` (`commands/invoke` and `render/page`
  params). The answer names the file, so an agent can ask its user.
- **Reads.** An `OpenFile` path must be inside a root; the readers
  already refuse what they cannot parse.

### Limits

Every limit is checked before the work or allocation it guards
(`crates/control/src/limits.rs`, `Limits::DEFAULT`):

| Limit | Default | Guards |
|---|---|---|
| `max_frame_bytes` | 4 MiB | one incoming line (longer lines are discarded and answered) |
| `max_json_depth` | 64 | nesting of one incoming value |
| `max_response_bytes` | 48 MiB | one outgoing frame |
| `max_args` | 64 | arguments in one invocation |
| `max_arg_bytes` | 1 MiB | one text, path or decoded bytes argument |
| `max_list_items` | 10 000 | one list argument |
| `default_page_items` / `max_page_items` | 100 / 1000 | one page of inspection or commands |
| `max_summary_text_bytes` | 1 KiB | text in a shape summary |
| `max_render_pixels` / `max_render_side` | 16 777 216 / 8192 | one rendered image (ADR 0079) |
| `requests_per_second` / `request_burst` | 100 / 200 | one connection's request rate (token bucket) |
| `max_connections` | 4 | socket connections at once |
| `hello_timeout_ms` | 10 s | a socket connection that never says hello |
| `host_reply_timeout_ms` | 30 s | an operation waiting for the app's UI thread |

A request over the rate is answered `LimitExceeded`, not queued; the
connection stays open. A fifth socket connection is refused at accept.
In the app, at most 8 operations run per frame (ADR 0076), so a flood
cannot freeze the window.

### Never crash on hostile input

The AGENTS.md section 6 rules apply in full: no `unwrap`, `expect`,
`panic!` or indexing panics in non-test code, checked arithmetic on
every input-derived number, capped allocations, bounded recursion
(the JSON depth cap bounds serde's recursion). A malformed frame gets
an error response and the loop reads on; nothing a client sends ends
the process or the app. The fuzz target (ADR 0082) drives the framing,
the JSON-RPC parser and both front ends with arbitrary bytes.

### Not in scope for the alpha

Per-tool user confirmation in the app (MCP leaves that to clients),
an audit log beyond the status bar, read-only sessions, and
per-client roots.

## Consequences

- A user who never turns agent control on has no listener at all.
- Agents working on files outside the working directory (headless) or
  the home directory (attached) need an `--allow-dir`; the error says
  so.
- The extension rule blocks writing other file types even where a
  command could (none can today); a later export format adds its
  extension here.
- The token file is the only secret; it lives as long as the app runs.

## Alternatives considered

- **Peer-credential checks (`SO_PEERCRED`, `getpeereid`) instead of a
  token.** Rejected: they need `libc` and `unsafe` code, and the token
  covers Windows too. The directory permissions already give the same
  guarantee on Unix; the token is the uniform check.
- **No token on Unix.** Rejected: one rule on every platform is easier
  to test and to document.
- **Trust the agent, no path policy.** Rejected: prompt injection makes
  the agent's caller untrusted.
- **A deny-list of paths.** Rejected: allow-lists (roots and
  extensions) fail closed.
- **Confirm every remote write in the app.** Rejected for the alpha:
  it would block headless use and duplicate the client's own
  confirmations; the status bar and Undo cover the attached case.
