# gateq: the gate queue

`gateq` runs `cargo xtask gate` for every worker on this machine
through one queue, so that at most two gates run at once (AGENTS.md
section 7). A gate run builds and tests the whole workspace; more than
two at a time exhaust the memory and the disk of the build machine.

Always call it from the main checkout, so every worker uses the same
version:

```sh
G=/Users/dara/larrygram/tools/gate-queue/gateq
$G submit P7-06 --skip wasm,fuzz   # queue the hand-off gate; returns at once
$G wait P7-06                      # block up to 25 min; re-run until it prints a result
$G status                          # running, queued and finished gates
$G show P7-06                      # the last result prompt again
$G bump P7-02                      # move a queued job to the front
$G cancel P7-06                    # drop a queued job or stop a running one
```

## How it works

- `submit <task> [gate args]` records the task, its worktree
  (`/Volumes/Dara/larrygram/worktrees/<task>`, or `--dir PATH`) and its
  `HEAD`, and starts the background runner if none is running. A new
  submit for the same task replaces its queued job. `--front` puts the
  job ahead of the queue and `--idle` behind it. `--checkout REV`
  resolves `REV` in the main repository at submit time and checks that
  commit out (detached) in the job's worktree when the job starts. Other
  arguments go to `cargo xtask gate`.
- The runner starts queued jobs in submit order (bumped jobs first)
  while fewer than `GATEQ_SLOTS` (default 2) gates run. A gate started
  by hand, outside the queue, still takes a slot. One task never has
  two gates running at once. The runner exits when the queue is empty.
- Each job runs in its own process group with no `CARGO_*` variables
  (sccache hashes them, AGENTS.md section 2). The task `main` builds in
  `/Volumes/Dara/larrygram/target/main`. `--corpus` sets
  `LARRYGRAM_REQUIRE_CORPUS=1` for the gate, so its test (or coverage)
  step fails when the corpus is missing instead of skipping. A job that
  runs longer than `GATEQ_MAX_SECS` (default 4 hours) is stopped.
- When a job ends it writes, under `/Volumes/Dara/larrygram/gates/`:
  - `logs/<task>-<sha>-<time>.log`, the full output;
  - `results/<task>.result`, the status: `GREEN`, `RED`, `CANCELLED`,
    `TIMEOUT` or `KILLED`;
  - `results/<task>.prompt`, a ready-to-paste message for the agent:
    the gate summary, the failing lines of each failed step and what to
    do next.
  It also shows a macOS notification (`GATEQ_NOTIFY=0` turns it off).
- `wait <task>` prints that prompt when the task's latest job has a
  result. It exits 0 for green, 1 for any other result and 3 when it
  times out (`--timeout SECS`, default 1500, so it fits the command
  limits of agent harnesses). A worker simply runs it again on exit 3.
- Recovery: each job records its own result, so a killed runner loses
  nothing; the next `submit`, `wait` or finishing job restarts it. A job
  whose process was killed is recorded as `KILLED`.

The master's two jobs (AGENTS.md section 7):

```sh
$G submit main --corpus --front --quick        # merge gate, in the main checkout
$G submit main-full --dir /Volumes/Dara/larrygram/worktrees/main-full \
    --checkout main --corpus --idle               # full check, its own worktree
```

Perl 5 with core modules only (macOS ships it). The state directory is
`GATEQ_HOME` (default `/Volumes/Dara/larrygram/gates`).
