# AGENTS.md — rules for every agent working on Larrygram

Larrygram is a clean-room, pure-Rust, Visio-compatible diagramming app (alpha). These rules apply to every agent and every harness (Claude Code, pi, others). If a task prompt conflicts with this file, this file wins; stop and report the conflict.

## 1. Session start

1. Read this file in full.
2. Read `docs/STATUS.md` (current phase, task board, who owns what).
3. Read your task file `docs/tasks/<task-id>.md`. It names your role, crate ownership, inputs, outputs and acceptance criteria.
4. Read your role file `.claude/agents/<role>.md`. Agents in other harnesses read it as plain Markdown; ignore the frontmatter.
5. Read the ADRs your task references in `docs/adr/`.

## 2. Shared local repository and git workflow

The **main repo** is `/Users/dara/larrygram`, branch `main`. It is shared by every agent on this machine. Only the master agent commits to `main`.

Each task works in its own git worktree on the external drive:

```sh
TASK=P1-03                                   # your task id
WT=/Volumes/Dara/larrygram/worktrees/$TASK
git -C /Users/dara/larrygram worktree add -b agent/$TASK "$WT" main
cd "$WT"
```

Builds go through a shared `sccache` cache (machine-wide `~/.cargo/config.toml` sets `rustc-wrapper = "sccache"`; the cache lives at `/Volumes/Dara/larrygram/sccache`). A fresh worktree therefore reuses compiled dependencies from every other worktree. Build in the worktree's own `target/` (it is on the external drive and gitignored). **Do not set `CARGO_TARGET_DIR`** (or any other `CARGO_*` variable): sccache hashes them, so a per-task value makes every cache lookup miss. Do not set `RUSTC_WRAPPER` yourself and do not clear the cache. If `sccache` is missing in your harness, builds still work, only slower: say so in your Result.

- Commit only on `agent/<task-id>`. Never commit to `main`, never rewrite `main`, never touch another agent's branch or worktree.
- Before handing off, rebase on the latest `main`: `git rebase main` (worktrees share refs with the main repo, so `main` is always current).
- Hand-off: fill in the `## Result` section of your task file (what changed, gate output summary, open issues) and commit it as the last commit on your branch. Then stop and tell the user the branch is ready.
- The master agent reviews and merges into `main`. If review finds problems, you get a follow-up task.
- Leave the worktree in place; the master removes it (and its `target/`) after merge.

## 3. Disk usage

The main drive is small. Put everything large on `/Volumes/Dara/larrygram/`:

- Worktrees: `/Volumes/Dara/larrygram/worktrees/`
- Cargo target dirs: each worktree's own `target/` (already on the external drive); the shared compile cache is `/Volumes/Dara/larrygram/sccache`. The main checkout is on the small drive: the master builds there with `CARGO_TARGET_DIR=/Volumes/Dara/larrygram/target/main`.
- Raw downloads: `/Volumes/Dara/larrygram/reference/`. The repo's `reference/` directory is a symlink to it (gitignored). In a worktree, create the same symlink: `ln -s /Volumes/Dara/larrygram/reference reference`.
- Corpus files: `corpus/vsdx/` and `corpus/vsd/` are gitignored and exist only in the main checkout. In a worktree, link them: `ln -s /Users/dara/larrygram/corpus/vsdx corpus/vsdx` and the same for `vsd`. If git shows the links as untracked, add them to `.git/info/exclude`, not to `.gitignore`.
- Large builds (libvisio, corpora): `/Volumes/Dara/larrygram/`.
- Gate queue state, logs and result prompts: `/Volumes/Dara/larrygram/gates/` (section 7).

## 4. Clean-room rules (non-negotiable)

Allowed sources only:
- Microsoft Open Specifications ([MS-VSDX]).
- Microsoft Learn: the ShapeSheet reference and the Visio VBA/automation reference, including enumerations.
- Microsoft's published Office Fluent UI control identifiers.
- Microsoft support articles (e.g. keyboard shortcuts).
- libvisio, under section 5.
- Observed behavior of Visio as a black box. Only derived facts (structure, geometry, color values, our own descriptions) are committed.

Never:
- Disassemble, decompile or debug any Microsoft binary or installer, or extract resources from one (UI XML, strings, icons, images, fonts).
- Commit Microsoft-owned content: spec documents, icons, images, stencils (.vss/.vssx), templates, sample drawings, fonts (Calibri, Segoe UI).
- Copy long-form text (super-tooltips, help descriptions). Short command labels ("Align", "Connector", "Pointer Tool") are fine as UI labels. Write our own descriptions.
- Copy GPL/LGPL/AGPL code.

Always:
- Raw downloads stay in `reference/` (gitignored). Commit only derived facts (IDs, names, structure, enumerations) and our own summaries.
- Record every source in `reference/SOURCES.md` (local) and `docs/sources.md` (committed). Data the project compiles itself (layouts, themes, icon mappings) is recorded as compiled by the project, with its `ATTRIBUTION.md` row.
- Fonts: Carlito (for Calibri) and Selawik (for Segoe UI), bundled with their OFL licenses.
- Icons: an open-licensed set with its license recorded in `ATTRIBUTION.md`, or simple placeholders. No Office icons, no traced or redrawn copies of them, and no Microsoft-published icon sets. Open icons may be recolored to match our theme palette.
- Every stencil master we ship is original work.
- Every committed asset file has a row in `ATTRIBUTION.md`, added in the same commit.

## 5. libvisio rules

libvisio is MPL-2.0 (verify `COPYING` when you fetch it; record the commit hash).

- **Test oracle:** its CLI tools (vsd2raw, vsd2xhtml, vss2raw — whichever exist) are built in a dev-only harness under `tools/libvisio-oracle/`. Never a runtime dependency.
- **Binary .vsd reference:** code ported or derived from libvisio lives **only** in `crates/vsd-binary`, with MPL-2.0 file headers and attribution in `NOTICE`. No libvisio-derived code anywhere else, including tests of other crates.
- For .vsdx, MS-VSDX is the source of truth, not libvisio.
- `cargo deny` enforces that `larrygram-vsd-binary` is the only MPL crate.

## 6. Engineering rules

- **Never crash.** Malformed input produces errors, never panics. Non-test code may not use `unwrap`, `expect`, `panic!`, `unreachable!`, `todo!` or `unimplemented!` (workspace clippy lints). Parser crates (vsdx, vsd-binary, shapesheet) also deny `clippy::indexing_slicing`: use `get()`. Use checked arithmetic and capped allocations on input-derived values. Bound recursion. Tests may use these (clippy.toml). Every crash fix gets a regression test.
- **No unsafe.** Every crate root has `#![forbid(unsafe_code)]` unless an ADR justifies an exception.
- **One command registry** (`crates/commands`). Every user action is a command with an id, label, enabled-state predicate and undo support. Menus, command bar, shortcuts, CLI, the JSON control channel and MCP all dispatch by command id through the same registry. Programmatic calls never open dialogs.
- **Units.** Internal units are Visio's (inches). Convert only at the edges (UI pixels, file formats that differ).
- **Layering** (ADR 0002, checked by `cargo xtask layers`). A crate depends only on lower layers. Nothing below L6 depends on egui/eframe/winit/rfd.
  - L0 `geom`, `ui-spec` · L1 `model`, `text` · L2 `vsdx`, `vsd-binary`, `shapesheet` · L3 `render` · L4 `commands` · L5 `control` · L6 `ui` · L7 `apps/*`
- **Docs.** `missing_docs` is on for library crates; document public items.
- **Decisions** go in `docs/adr/NNNN-title.md` (copy `docs/adr/0000-template.md`).
- **Stay in your lane.** Edit only the crates/files your task owns. If you need a change elsewhere, write it under "Open issues" in your Result section.
- **Dependencies.** New crates must have permissive licenses (see `deny.toml`). Prefer well-maintained crates. Mention new dependencies in your Result section.

## 7. Quality gate

`cargo xtask gate` is the single gate. It runs at two levels:

- **Hand-off gate (workers).** `cargo xtask gate --quick` (codegen, parity, fmt, clippy, test, doc, deny, layers) plus every slow step your change triggers (table below). It must be green before hand-off.
- **Merge gate (master).** `cargo xtask gate --quick` on `main` with the corpus tests required, after each batch of up to three merges.
- **Full check (master).** The full `cargo xtask gate` with the corpus tests required, at low priority in its own worktree, on the latest merged `main`. Merging goes on while it runs; a step that goes red there becomes a master fix or a follow-up task.

| Step | Command |
|---|---|
| codegen | `cargo xtask codegen --check` (generated files are fresh; never skipped) |
| fmt | `cargo fmt --all -- --check` |
| clippy | `cargo clippy --workspace --all-targets --all-features -- -D warnings` |
| test | `cargo test --workspace --all-features`; when coverage also runs, only the doc tests (`--doc`), because coverage runs every other test |
| doc | `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps` |
| coverage | `cargo llvm-cov`, per-crate thresholds and ratchet from `coverage.toml` |
| deny | `cargo deny check` |
| layers | `cargo xtask layers` |
| wasm | `cargo build -p larrygram-web --target wasm32-unknown-unknown` |
| fuzz | `cargo +nightly fuzz build` in every `crates/*/fuzz` |

Slow steps a worker adds to the hand-off gate:

| Step | Run it when your change |
|---|---|
| wasm | touches `crates/ui`, `apps/larrygram-web`, `#[cfg(target_arch)]` code, or any `Cargo.toml` / `Cargo.lock` |
| fuzz | touches a `crates/*/fuzz` directory, or changes the public API of a crate that has one (`ui-spec`, `vsdx`, `render`, `ui`) |
| coverage | changes any non-test code in a crate. The ratchets sit near 99%, so new untested code fails them. Give each interface stub a test of its "not implemented" contract. Skip coverage only for docs-only or test-only tasks |

Skip only the untriggered steps: for example `cargo xtask gate --skip wasm,fuzz` runs coverage. If unsure, run the step. Name the steps you ran in the Result's gate summary.

**Every gate goes through the gate queue** (`tools/gate-queue/README.md`). Many workers share one machine, and more than two workspace-wide builds at once exhaust its memory and disk. Never run `cargo xtask gate`, or any `--workspace` cargo command (`test`, `clippy`, `llvm-cov`, `check`), directly:

```sh
G=/Users/dara/larrygram/tools/gate-queue/gateq   # always the main checkout's copy
$G submit <task-id> --skip wasm,fuzz            # your gate args; returns at once
$G wait <task-id>                                # run again while it exits 3 (no result yet)
```

`wait` prints a prompt with the result, the failing lines and what to do next; follow it. Commit before you submit, and do not edit the worktree until the result arrives: the gate tests the working tree as it is when the job starts. If your session must end while waiting, the prompt stays in `/Volumes/Dara/larrygram/gates/results/<task-id>.prompt`.

- After each change, check only the crates you touched: `cargo test -p <crate>` and `cargo clippy -p <crate> --all-targets --all-features -- -D warnings`.
- Submit `--quick` at milestones only (before a rebase, after a large change).
- Submit the hand-off gate with its slow steps **once**, after the last code commit and the rebase. Submit again only if code changes after that; a Result-only commit does not need it.
- The master submits the merge gate as `gateq submit main --corpus --front --quick` and the full check as `gateq submit main-full --dir /Volumes/Dara/larrygram/worktrees/main-full --checkout main --corpus --idle`.

Coverage thresholds: geom, model, vsdx, vsd-binary, shapesheet, commands, ui-spec ≥ 85%; render, text ≥ 75%; control, ui, apps ≥ 50% (UI via egui_kittest). No crate may drop below its previous-phase number (`[ratchet]` in `coverage.toml`; only the master updates it).

## 8. Commits

- Conventional commits: `feat(vsdx): read OPC content types`, `test(geom): ...`, `fix`, `docs`, `chore`, `refactor`, `ci`, `build`.
- Scope = crate or area. One logical change per commit. Small commits.
- Include the task id in the body: `Task: P1-03`.

## 9. Tests

- Unit tests live next to the code. Integration tests in `crates/<name>/tests/`.
- Property tests: `proptest`. Snapshots: `insta`. UI: `egui_kittest`. Fuzz: `cargo fuzz` in `crates/<name>/fuzz/`.
- Test files from the corpus are read from `corpus/` and listed in `corpus/PROVENANCE.md`. Never commit a file whose license is unclear.

## 10. Shell tips (macOS)

- `cp`/`mv` may be aliased interactively: use `/bin/cp -f`, `/bin/mv -f`.
- No `timeout` binary: `perl -e 'alarm 600; exec @ARGV' <cmd>`.
- Never run `cargo clean` on another task's target dir.
