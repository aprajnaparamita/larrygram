# 0082: Phase 9 test strategy

- Status: proposed
- Date: 2026-10-25
- Task: P9-01

## Context

KICKOFF.md Phase 9: "Tests: protocol tests, plus an end-to-end script
that builds a flowchart over MCP and checks the result." The control
crate is new code that parses hostile input (ADR 0081), threads (ADR
0076) and two protocols (ADRs 0075, 0080), and its threshold is 50%
(`coverage.toml`, no ratchet yet) while the rest of the workspace sits
near 99%. The earlier phases' strategies (ADRs 0049, 0060, 0065) are
the pattern: unit tests in each task, one test-engineer task for the
cross-cutting tests, then the three audits.

## Decision

### In every implementation task

- Unit tests next to the code, replacing the stub-contract tests of
  P9-01 (a stub test that asserts `NotImplemented` is rewritten, not
  deleted, when its function lands).
- Malformed-input tests for every parser path the task owns: each
  refusal in `parse_message`, the framing's long-line and EOF cases,
  each `InvalidParams` of `args_from_json`, each policy refusal.
- Hand-built `AppCtx` values for inspection and rendering; the
  `HeadlessHost` for anything that dispatches.
- The control crate's coverage target for the phase is 90%; the master
  sets the ratchet at the phase end (P9-16).

### Protocol tests (task P9-11)

`crates/control/tests/` (integration tests over the public API):

- **Golden transcripts**: request and response lines for every
  control-protocol method and every MCP method and tool, in both MCP
  eras, as committed `.jsonl` files under
  `crates/control/tests/transcripts/`. A test replays each request
  through `serve` with a headless backend over a seeded document and
  compares the responses (with `insta` snapshots for the JSON). The
  MCP fixtures follow the 2026-07-28 and 2025-11-25 specification
  examples (shapes only; our own text).
- **Error table**: one request per `ErrorCode` that produces it, so the
  ADR 0075 table is tested end to end.
- **Handshake and auth**: a socket server on a temporary runtime
  directory (`LARRYGRAM_RUNTIME_DIR`); `hello` with the right token,
  a wrong token, none, a late one; the discovery file's permissions
  on Unix; `max_connections`; the rate limiter with a fake clock.
- **Property tests** (`proptest`): any byte string through the framing
  and `parse_message` never panics and yields a message or a
  rejection; `encode_op` then `decode_op` is the identity for
  generated ops; `encode_cursor` then `decode_cursor` likewise.

### The fuzz target (task P9-11)

`crates/control/fuzz/` with one target, `request`, that feeds
arbitrary bytes as frames through `serve` with each front end over a
headless backend holding a small seeded document (so `commands/invoke`
and the inspect and render paths run on fuzzed params). Seeds: the
golden transcripts' request lines. Rendering inside the fuzzer runs at
small caps (`Limits` with `max_render_pixels` = 65 536) to keep the
executions fast. The target is built by the master's full check
(`cargo +nightly fuzz build`); a short local run is in P9-11's Result.

### The end-to-end MCP flowchart (task P9-11)

`apps/larrygram-cli/tests/mcp_flowchart.rs` mirrors the Phase 5 exit
test (`apps/larrygram/tests/flowchart.rs`) over MCP:

1. Start the built `larrygram-cli mcp` binary as a child process
   (`env!("CARGO_BIN_EXE_larrygram-cli")`) in a temporary working
   directory, exactly as an MCP client would.
2. Speak to it over its stdin and stdout: `server/discover`, then
   `tools/list` (assert the nine tools), then `run_command` calls:
   `file.new`, ten `insert.shape` (the "Basic Flowchart" masters,
   with texts), five connectors glued with `connector.draw`, `page.add`
   and four shapes on page two, `file.save_as` to `flowchart.vsdx`
   inside the working directory.
3. Check the result over MCP: `inspect_page` on both pages (shape
   counts, masters, texts, five connects with both ends glued) and
   `render_page` (a PNG of the expected size whose bytes decode).
4. Reopen with `run_command` `file.open`, inspect again, and compare
   with the first inspection (the ADR 0048 equality, through the JSON
   views).
5. Repeat steps 2 and 3 once over the legacy `initialize` handshake.
6. Close stdin and assert the process exits with status 0 and wrote
   nothing but MCP messages to stdout.

The test is the phase's exit criterion 1 (`docs/plans/phase-9.md`).
A second, smaller test drives `larrygram-cli serve` with the control
protocol the same way (create, insert, inspect, render), so both front
ends are covered end to end.

### The attached app (tasks P9-10 and P9-11)

The desktop app's attach path is tested without a window: the shell
host adapter and the inbox run in `egui_kittest` with a headless egui
context; a test thread connects through the socket, invokes a command,
and the test asserts that the next frame shows the change and that
`edit.undo` from the UI reverts it. No test opens a real window
(AGENTS.md: the master runs the app manually).

### Audits (tasks P9-13 to P9-15)

The reviewer, coverage and clean-room audits as in Phases 6 to 8. The
clean-room audit also checks that no MCP specification text was copied
into the repository (the tool descriptions and the protocol document
are our own words; schema shapes and method names are facts).

## Consequences

- The test-engineer task depends on every implementation task; it is
  the phase's integration point for tests, as P8-10 was for Phase 8.
- The end-to-end test needs the CLI binary built; it runs in the
  normal `cargo test` of the workspace (cargo builds the binary for
  integration tests).
- Fuzzing the executor through `serve` finds crashes in the inspect and
  render code too, not only in the parsers.

## Alternatives considered

- **Test against a real MCP client.** Rejected: it would need a hosted
  model or a third-party client in the gate. The transcripts pin the
  wire shapes; the user's manual check with a real client is noted in
  the plan.
- **Use `rmcp` as a dev-dependency client.** Rejected for now: it would
  bring the async runtime into the test tree (ADR 0080); revisit if the
  hand-written fixtures miss an interoperability bug.
- **The flowchart test in-process (no child process).** Rejected: the
  point is the real binary over real stdio, as a client starts it; an
  in-process version exists anyway in the protocol tests.
