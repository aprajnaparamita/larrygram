# Status

Task board. Only the master agent edits this file, on `main`.

- Current phase: **7 and 8 side by side** (the boundary closed on 2026-10-24: R1, P7-00, R4 and R5 (`67adffd`, the image loaders) done). Wave 1 and wave A launch: twelve workers; P7-16 and P8-08 start when the first two finish
- Phase 6 goal: ShapeSheet engine, core subset (KICKOFF.md). Exit: resizing master-based shapes from the corpus recomputes geometry correctly for supported functions; the phase report gives the agreement rate with cached values.
- Phase 5 goal: editing core (KICKOFF.md). Exit: a user can build a 10-shape flowchart with connected shapes across two pages, save it, reopen it and keep editing.
- Phase 6 complete (2026-10-24, report: [progress/phase-6.md](progress/phase-6.md), tag `phase-6`).
- Phase 5 complete (2026-10-21, report: [progress/phase-5.md](progress/phase-5.md), tag `phase-5`).
- Phase 4 complete (2026-10-18, report: [progress/phase-4.md](progress/phase-4.md), tag `phase-4`).
- Phase 3 complete (2026-10-16, report: [progress/phase-3.md](progress/phase-3.md), tag `phase-3`).
- Phase 2 complete (2026-10-14, report: [progress/phase-2.md](progress/phase-2.md), tag `phase-2`).

## In flight (master, 2026-10-24 evening)

- Gate queue: `tools/gate-queue/gateq` (AGENTS.md section 7). Merge gates are `main --quick --corpus`; the idle full check is `main-full` in `/Volumes/Dara/larrygram/worktrees/main-full`.
- Merged today: P8-04, P8-02, P8-05, P7-02 (merge gate green at `d156f9c`). P7-08 started; P7-06 and P7-07 resumed.
- Sent back: P7-04 (remove its seven `#[rustfmt::skip]`, test the `currentColor` tint and non-blank icons). P7-03 must rebase on main (conflict in `crates/ui-spec/tests/panel_layouts.rs`) and resubmit.
- Merged since the restart: P8-06, P7-05, P7-15; P8-03 RED at `51ce31e7`, resubmitted at `fc3fec7b`. Queued (paused for the iTerm2 restart): P7-06, P7-05, P7-03, P7-07, P7-08, P8-03, P7-04, then `main-full` at `13942e0`.
- Queued, not started: P7-16, P8-08.
- Master items: the parity matrix's one column per layout (P7-02 open issue); `UiRequest::ShowWelcome` in the shell (P7-09); P8-02/P8-04/P8-05 open issues for P8-03, P8-07, P8-08, P8-10 (in their Result sections); the remote still holds the pre-R4 history until the user force-pushes, and the three backup files wait for that.

## Phase 7 task board (plan: [plans/phase-7.md](plans/phase-7.md))

Phase 7: Panel and Classic layouts, the Larrygram rename and the welcome wizard. Phase 8 is binary `.vsd` import; Phase 9 (agents) is paused. The rename boundary (R1, P7-00, R4, R5) waits for the `phase-6` tag; nothing else runs during it. Settings: `welcome_seen` and the default layout are stubbed in `crates/ui/src/settings.rs` and move onto `Settings` in `crates/commands` in P7-09.

| Task | Role | Depends on | Group | State | Branch |
|---|---|---|---|---|---|
| [P7-01](tasks/P7-01.md) | architect | none | 0 | merged (ADRs 0061–0067, data, brand assets, stubs, task files; master recovered six misplaced files) | — |
| [P7-00](tasks/P7-00.md) | implementer | P7-01, `phase-6` tag | boundary | merged (the Larrygram rename) | ec182e7 |
| [P7-02](tasks/P7-02.md) | implementer | boundary | 1 | merged (151 Phase 7 ids, the Fluent joins, the Panel Classic and Panel Modern switches, `help.welcome`; open: the parity matrix's one column per layout, `UiRequest::ShowWelcome` in the shell for P7-09) | c2a5fa2 |
| [P7-03](tasks/P7-03.md) | implementer | boundary | 1 | ready (v2 loader, cross-checks, adapter) | — |
| [P7-04](tasks/P7-04.md) | implementer | boundary | 1 | ready (icon pipeline; the image loaders are in since R5) | — |
| [P7-05](tasks/P7-05.md) | implementer | boundary | 1 | merged (theme tokens on egui, Selawik, canvas colours from the theme; minor: Selawik embedded a second time from `crates/text/fonts`, for P7-09/P7-11) | 484ec1b |
| [P7-06](tasks/P7-06.md) | implementer | boundary | 1 | ready (Panel Classic widgets, against the frozen v2 types) | — |
| [P7-07](tasks/P7-07.md) | implementer | boundary | 1 | ready (Panel Modern widgets, against the frozen v2 types) | — |
| [P7-08](tasks/P7-08.md) | implementer | P7-02 | 1+ | ready (selection contexts; P7-02 merged) | — |
| [P7-15](tasks/P7-15.md) | implementer | boundary | 1 | merged (welcome wizard, both pages, kittest flow tests) | 96dbcc1 |
| [P7-16](tasks/P7-16.md) | implementer | boundary | 1 | ready, starts second (About window, app icon) | — |
| [P7-09](tasks/P7-09.md) | implementer | P7-05..P7-08, P7-15, P7-16 | C | waiting (shell integration) | — |
| [P7-10](tasks/P7-10.md) | test-engineer | P7-09 | D | waiting (tests, golden images) | — |
| [P7-11](tasks/P7-11.md) | reviewer | P7-10 | E | waiting | — |
| [P7-12](tasks/P7-12.md) | coverage-auditor | P7-10 | E | waiting | — |
| [P7-13](tasks/P7-13.md) | cleanroom-auditor | P7-10 | E | waiting | — |
| [P7-14](tasks/P7-14.md) | implementer | P7-11..P7-13 | F | waiting (fixes, phase report) | — |

Tasks start as soon as their own dependencies merge (no whole-group waits); run at most twelve workers at once across Phase 7 and Phase 8.

## Phase 8 task board (plan: [plans/phase-8.md](plans/phase-8.md))

Phase 8: binary `.vsd`, `.vss` and `.vst` import, read-only, in `crates/vsd-binary`. It runs beside Phase 7 after the boundary. P8-01 plans it now, beside P6-17 and P6-18, and must merge before the boundary.

| Task | Role | Depends on | Group | State | Branch |
|---|---|---|---|---|---|
| [P8-01](tasks/P8-01.md) | architect | none | 0 | merged (plan, ADRs 0068–0072, `vsd-binary` skeleton, task files) | 00769a3 |
| [P8-02](tasks/P8-02.md) | implementer | P8-01, boundary | A | merged (MS-CFB reader, LZ decompression, summary streams; `allow(dead_code)` until P8-07) | 83100dd |
| [P8-03](tasks/P8-03.md) | implementer | P8-01, boundary | A | ready (chunks and pointers) | — |
| [P8-04](tasks/P8-04.md) | implementer | P8-01, boundary | A | merged (shapes, transforms, geometry; `collect_with` beside the frozen `collect`; open issues for P8-03, P8-07, P8-08, P8-10) | 13d6641 |
| [P8-05](tasks/P8-05.md) | implementer | P8-01, boundary | A | merged (text, runs, fields, Character/Paragraph/Tabs rows; P8-11 note: `for` over `Option` with `allow(for_loops_over_fallibles)` in `text.rs` to dodge an untaken branch) | 55d3d00 |
| [P8-06](tasks/P8-06.md) | implementer | P8-01, boundary | A | merged (styles, colour table, fills, lines; open: the version 6 style names and the style Font cell, for P8-07) | 509c176 |
| [P8-08](tasks/P8-08.md) | implementer | P8-01, boundary | A | ready, starts second (oracle harness, census) | — |
| [P8-07](tasks/P8-07.md) | implementer | P8-02..P8-06 | B | waiting (pages, masters, pipeline) | — |
| [P8-09](tasks/P8-09.md) | implementer | P8-07, P7-09 | C | waiting (open path) | — |
| [P8-10](tasks/P8-10.md) | test-engineer | P8-07, P8-08 | D | waiting (tests, fuzz, goldens, first parity report) | — |
| [P8-11](tasks/P8-11.md) | reviewer | P8-10 | E | waiting | — |
| [P8-12](tasks/P8-12.md) | coverage-auditor | P8-10 | E | waiting | — |
| [P8-13](tasks/P8-13.md) | cleanroom-auditor | P8-10 | E | waiting | — |
| [P8-14](tasks/P8-14.md) | implementer | P8-11..P8-13 | F | waiting (fixes, phase report) | — |

## Phase 6 task board

| Task | Role | Depends on | Group | State | Branch |
|---|---|---|---|---|---|
| [P6-01](tasks/P6-01.md) | architect | Phase 5 | — | merged `3171835` (ADRs 0051–0060, stubs, task files) | — |
| [P6-02](tasks/P6-02.md) | implementer | P6-01 | A | merged (lexer, parser, printer, `parse` fuzz target) | — |
| [P6-03](tasks/P6-03.md) | implementer | P6-01 | A | merged (values, units, coercions; `Coerced` exported by the master) | — |
| [P6-04](tasks/P6-04.md) | implementer | P6-01 | A | merged (42 functions, evaluator) | — |
| [P6-05](tasks/P6-05.md) | implementer | P6-01 | A | merged (reference resolution, inheritance; P3-15 closed) | — |
| [P6-06](tasks/P6-06.md) | implementer | P6-01 | A | merged (dependency graph, recalc; exports added by the master) | — |
| [P6-07](tasks/P6-07.md) | implementer | P6-02..P6-06 | B | merged (recalc after edits, snapshot undo; `scale_geometry` retired in commands) | — |
| [P6-08](tasks/P6-08.md) | implementer | P6-05, P6-06 | B | merged (U spelling for every value kind, fill-in on open; P2-14 closed) | — |
| [P6-15](tasks/P6-15.md) | implementer | P6-08 | B | merged (effective style link; `dwg`, `fdo86664`, `testfile5` draw) | — |
| [P6-16](tasks/P6-16.md) | implementer | P6-08 | B | merged (reader drops the P5-10 artefacts; writer recovers the contents relationship id) | — |
| [P6-09](tasks/P6-09.md) | implementer | P6-03, P6-07 | B2 | merged (typed Shape Data and Size & Position panes) | — |
| [P6-10](tasks/P6-10.md) | implementer | P6-07, P6-08 | C | merged (100.0% agreement on 5 184 counted cells; exit resize test) | — |
| [P6-11](tasks/P6-11.md) | test-engineer | P6-10 | D | merged | 7393cdc |
| [P6-12](tasks/P6-12.md) | reviewer | P6-11 | E | merged | 14ed944 |
| [P6-13](tasks/P6-13.md) | coverage-auditor | P6-11 | E | merged | da276fb |
| [P6-14](tasks/P6-14.md) | cleanroom-auditor | P6-11 | E | merged | ddf0aac |
| [P6-17](tasks/P6-17.md) | implementer | P6-12, P6-13, P6-14 | F | merged | 51f3f57 |
| [P6-18](tasks/P6-18.md) | implementer | P6-12, P6-13, P6-14 | F | merged | f6c311b |

Group A runs in parallel, one `crates/shapesheet` module set per task; `crates/shapesheet/src/lib.rs` belongs to no task and `error.rs` to P6-02. The `larrygram-shapesheet` ratchet was reset from 100.0 (all stubs) to 85.0 when the plan was merged.

## Phase 5 task board

| Task | Role | Depends on | Group | State | Branch |
|---|---|---|---|---|---|
| [P5-01](tasks/P5-01.md) | architect | Phase 4 | — | merged (ADRs 0038–0049, stubs, task files) | agent/P5-01 |
| [P5-02](tasks/P5-02.md) | implementer | P5-01 | A | merged (mutation API, connects, writer deletes and text) | agent/P5-02 |
| [P5-03](tasks/P5-03.md) | implementer | P5-01 | A | merged (hit-testing, snapping, overlay; error variants fixed by the master) | agent/P5-03 |
| [P5-04](tasks/P5-04.md) | implementer | P5-02 | B | merged `075a37f` (push_edit, undo steps, editing commands) | — |
| [P5-15](tasks/P5-15.md) | test-engineer | P5-02 | B | merged `285fe06` (model 97.70% to 99.62%, tests only) | — |
| [P5-05](tasks/P5-05.md) | implementer | P5-03, P5-04 | C | merged `4e59631` (pointer tool, handles, marquee, snapping; P4-05, P4-07, P4-14 closed) | — |
| [P5-06](tasks/P5-06.md) | implementer | P5-05 | D | merged `6e01bf8` (insert.shape, drawing commands, draw and stencil tools; state moved into `ToolMachine` by the master) | — |
| [P5-07](tasks/P5-07.md) | implementer | P5-05 | D | merged `e7ba878` (shape.set.text, text.* commands, TextEditor, text tool; editor not yet shown, see P5-17) | — |
| [P5-16](tasks/P5-16.md) | implementer | P5-05 | D | merged (`edit.select`, `Document::connection_points`) | — |
| [P5-08](tasks/P5-08.md) | implementer | P5-05, P5-16 | D | merged `78bc8e9` (routing, glue commands, re-route hook, connector tool; master fixed the re-route document lookup) | — |
| [P5-09](tasks/P5-09.md) | implementer | P5-05 | D | merged (clipboard, arrange, pages, recent files, page-tab menu) | — |
| [P5-17](tasks/P5-17.md) | implementer | P5-06, P5-07, P5-08 | D2 | merged `ce9e86a` (tool commands, Shapes-window drag, text editor in the shell, indexed glue, `connector.draw`) | — |
| [P5-18](tasks/P5-18.md) | test-engineer | P5-08 | D2 | merged `4efb56c` (tests only; commands coverage 98.1% on main, at the ratchet; merge gate green) | — |
| [P5-10](tasks/P5-10.md) | implementer | P5-06..P5-09, P5-16..P5-18 | E | merged `a31f511` (flowchart exit harness, parity commands table: 977 rows, 84 implemented / 880 stub / 13 out-of-scope) | — |
| [P5-11](tasks/P5-11.md) | test-engineer | P5-10 | F | merged `e4f94de` (undo property, glue tests, drag timing, per-target fuzz seeds; 8 product fixes) | — |
| [P5-12](tasks/P5-12.md) | reviewer | P5-11 | G | merged `414a098` (approve with fixes: 0 blocker, 3 major, 7 minor; fixes go to one follow-up task after P5-13/14) | — |
| [P5-13](tasks/P5-13.md) | coverage-auditor | P5-11 | G | merged `02752d0` (all crates at threshold; commands 98.07 and render 98.39 pass the 98.1/98.4 ratchets only by rounding; fixes go to the follow-up task) | — |
| [P5-14](tasks/P5-14.md) | cleanroom-auditor | P5-11 | G | merged `44afac5` (pass with findings: 0 blocker, 0 major, 2 minor; both fixed by the master) | — |
| [P5-19](tasks/P5-19.md) | implementer | P5-12..P5-14 | H | merged `fcd9c8f` (review F1..F11 fixed; commands 98.3%, model 99.7%; merge gate green after the rustls-webpki advisory bump) | — |
| [P5-20](tasks/P5-20.md) | test-engineer | P5-13 | H | merged `c89c86f` (tests only; render 98.5%, vsdx 98.1%, ui 96.8%; merge gate green) | — |

Tasks in one group run in parallel on disjoint files. Shared files: `crates/commands/src/builtins/mod.rs`, `crates/commands/src/app.rs` and `xtask/src/extract/commands.rs` belong to P5-04 (later tasks use their own `TABLE` modules); `crates/ui/src/{lib,shell,canvas}.rs` and `crates/ui/Cargo.toml` to P5-05 (later tasks request lines in their Results); `crates/model/src/mutate.rs` and the `vsdx` writer to P5-02. The master resolves `Cargo.lock`. Fix tasks follow the audits.

## Phase 4 task board

| Task | Role | Depends on | Group | State | Branch |
|---|---|---|---|---|---|
| [P4-01](tasks/P4-01.md) | architect | Phase 3 | — | merged (ADRs 0028–0037, stubs) | agent/P4-01 |
| [P4-02](tasks/P4-02.md) | implementer | P4-01 | A | merged (CLI on the registry; zoom range shared by the master) | agent/P4-02 |
| [P4-04](tasks/P4-04.md) | implementer | P4-01 | A | merged (registry-backed test added by the master) | agent/P4-04 |
| [P4-05](tasks/P4-05.md) | implementer | P4-01 | A | merged (`show_rulers` takes units; notes for P4-03) | agent/P4-05 |
| [P4-06](tasks/P4-06.md) | implementer | P4-01 | A | merged (12 + 9 original masters) | agent/P4-06 |
| [P4-03](tasks/P4-03.md) | implementer | P4-02 | B | merged (shell, settings, pickers, both apps) | agent/P4-03 |
| [P4-07](tasks/P4-07.md) | implementer | P4-03 | C | merged (read-only panes) | agent/P4-07 |
| [P4-08](tasks/P4-08.md) | implementer | P4-03 | C | merged (shortcut map and dispatch; shell pass in P4-14) | agent/P4-08 |
| [P4-14](tasks/P4-14.md) | implementer | P4-08 | C2 | merged (scope-aware bindings, shell shortcut pass) | agent/P4-14 |
| [P4-09](tasks/P4-09.md) | implementer | P4-03..P4-08, P4-14 | D | merged (parity generator and gate step, Phase 4 harness) | agent/P4-09 |
| [P4-10](tasks/P4-10.md) | test-engineer | P4-09 | E | merged (54 tests, ui fuzz crate, one interner fix) | agent/P4-10 |
| [P4-11](tasks/P4-11.md) | reviewer | P4-10 | F | merged (approve with fixes: 0 blocker, 4 major, 12 minor) | agent/P4-11 |
| [P4-12](tasks/P4-12.md) | coverage-auditor | P4-10 | F | merged (pass: 0 blocker, 6 major, 10 minor) | agent/P4-12 |
| [P4-13](tasks/P4-13.md) | cleanroom-auditor | P4-10 | F | merged (pass: 0 blocker, 0 major, 1 minor, fixed on main) | agent/P4-13 |
| [P4-15](tasks/P4-15.md) | implementer | P4-11, P4-12 | G | merged (commands fixes, `view.units`, options/quit commands) | agent/P4-15 |
| [P4-17](tasks/P4-17.md) | test-engineer | P4-12 | G | merged (gate.rs 90.6%, parity 99.1%, ui fuzz seeds) | agent/P4-17 |
| [P4-16](tasks/P4-16.md) | implementer | P4-15 | H | merged (shell/UI fixes, Options through the registry, ui 96.5%) | agent/P4-16 |

Tasks in the same group run in parallel on disjoint files. Shared files: `crates/ui/src/lib.rs` and `crates/ui/Cargo.toml` belong to P4-03 (others request additions under Open issues); `xtask/src/extract/*` mapping rules to P4-02; `xtask/src/{main,gate}.rs` and `xtask/Cargo.toml` to P4-09; `ATTRIBUTION.md` rows come from P4-03 and P4-06; the master resolves `Cargo.lock`. Fix tasks follow the audits.

## Phase 3 task board

| Task | Role | Depends on | Group | State | Branch |
|---|---|---|---|---|---|
| [P3-01](tasks/P3-01.md) | architect | phase-2 | — | merged (ADRs 0019–0027, stubs, task files) | agent/P3-01 |
| [P3-02](tasks/P3-02.md) | implementer | P3-01 | A | merged (ADR 0021 amended: `Themed`, colour indices) | agent/P3-02 |
| [P3-03](tasks/P3-03.md) | implementer | P3-01 | A | merged (knot rule flagged for P3-14) | agent/P3-03 |
| [P3-04](tasks/P3-04.md) | implementer | P3-01 | A | merged (sources rows renumbered 11–12 on main) | agent/P3-04 |
| [P3-08](tasks/P3-08.md) | implementer | P3-01 | A | merged (added `Error::InvalidStyleData` in `error.rs`) | agent/P3-08 |
| [P3-05](tasks/P3-05.md) | implementer | P3-02, P3-03, P3-04, P3-08 | B | merged (added `CellReader` row readers in model; `TextNode` gained `runs` and `rotation`) | agent/P3-05 |
| [P3-15](tasks/P3-15.md) | implementer | P3-05 | B | merged (themed patterns and weights included) | agent/P3-15 |
| [P3-06](tasks/P3-06.md) | implementer | P3-02, P3-03, P3-04 | B | merged (conflict with P3-05 resolved on main; glyphs use their run colour) | agent/P3-06 |
| [P3-07](tasks/P3-07.md) | implementer | P3-02, P3-03, P3-04 | B | merged (stale base; glyph size, rotation and run colour wired on main) | agent/P3-07 |
| [P3-09](tasks/P3-09.md) | implementer | P3-05, P3-06, P3-07 | C | merged (page size fallback fixed on main) | agent/P3-09 |
| [P3-10](tasks/P3-10.md) | implementer | P3-05..P3-08, P3-15 | C | merged (render fuzz seed corpus re-included by the master) | agent/P3-10 |
| [P3-11](tasks/P3-11.md) | test-engineer | P3-09, P3-10 | — | merged (fixed a negative zero-advance line width in text) | agent/P3-11 |
| [P3-12](tasks/P3-12.md) | reviewer | P3-11 | D | merged (approve with fixes: 0 blocker, 4 major, 8 minor) | agent/P3-12 |
| [P3-13](tasks/P3-13.md) | coverage-auditor | P3-11 | D | merged (pass: 0 blocker, 5 major, 10 minor; ratchet proposal in report) | agent/P3-13 |
| [P3-14](tasks/P3-14.md) | cleanroom-auditor | P3-11 | D | merged (pass: 3 minor; F4 crash fixed by the master) | agent/P3-14 |
| [P3-16](tasks/P3-16.md) | implementer | P3-12..P3-14 | E | merged (also fixed PNG tile marks drawn pitch times too small) | agent/P3-16 |
| [P3-18](tasks/P3-18.md) | implementer | P3-12, P3-14 | E | merged (docs only; knot rule re-sourced) | agent/P3-18 |
| [P3-17](tasks/P3-17.md) | test-engineer | P3-16 | — | merged (56 tests; render 98.5%) | agent/P3-17 |

Tasks in the same group run in parallel on disjoint files. Shared files: `docs/sources.md` is owned by P3-04, and P3-02 and P3-08 each append rows (the master merges them). `ATTRIBUTION.md` is edited by P3-04 first, then P3-10. In `crates/render`, P3-05 owns `lib.rs`, `scene.rs`, `build.rs` and `Cargo.toml`. The master resolves `Cargo.lock`. Fix tasks follow the audits.

## Phase 2 task board

| Task | Role | Depends on | State | Branch |
|---|---|---|---|---|
| [P2-01](tasks/P2-01.md) | architect | phase-1 | merged (ADRs 0011–0017, stubs, task files) | agent/P2-01 |
| [P2-02](tasks/P2-02.md) | implementer | P2-01 | merged | agent/P2-02 |
| [P2-03](tasks/P2-03.md) | implementer | P2-01 | merged (use `UnitKind` for kind-checked conversion) | agent/P2-03 |
| [P2-05](tasks/P2-05.md) | implementer | P2-01 | merged (XML declaration added on main) | agent/P2-05 |
| [P2-08](tasks/P2-08.md) | implementer | P2-01 | merged (ATTRIBUTION row added on main) | agent/P2-08 |
| [P2-04](tasks/P2-04.md) | implementer | P2-02, P2-03 | merged | agent/P2-04 |
| [P2-06](tasks/P2-06.md) | implementer | P2-04, P2-05 | merged (angles are radians; 25 name aliases) | agent/P2-06 |
| [P2-06b](tasks/P2-06b.md) | implementer | P2-06 | merged (`Inh` fix on main) | agent/P2-06b |
| [P2-07](tasks/P2-07.md) | implementer | P2-06b | merged (exit criterion met: 15/15 corpus files round-trip) | agent/P2-07 |
| [P2-09](tasks/P2-09.md) | implementer | P2-06, P2-08 | merged (docs XSD is 1.1 and out of date; see P2-10 rule) | agent/P2-09 |
| [P2-10](tasks/P2-10.md) | test-engineer | P2-07, P2-09 | merged (1 writer bug fixed; vsdx 97.1%) | agent/P2-10 |
| [P2-11](tasks/P2-11.md) | reviewer | P2-10 | merged (approve with fixes: 0 blocker, 1 major, 6 minor; finding 7 fixed on main) | agent/P2-11 |
| [P2-12](tasks/P2-12.md) | coverage-auditor | P2-10 | merged (pass: 0 blocker, 3 major, 11 minor) | agent/P2-12 |
| [P2-14](tasks/P2-14.md) | implementer | P2-11, P2-12 | merged | agent/P2-14 |
| [P2-13](tasks/P2-13.md) | cleanroom-auditor | P2-10 | merged (pass: 0 blocker, 0 major, 2 minor; F1 fixed on main) | agent/P2-13 |

Tasks in the same group run in parallel on disjoint files. Fix tasks (P2-14 onward) follow the audits, as in Phase 1.

## Phase 1 task board

| Task | Role | Depends on | State | Branch |
|---|---|---|---|---|
| [P1-01](tasks/P1-01.md) | researcher | — | merged (corpus binaries excluded, ADR 0005) | agent/P1-01 |
| [P1-02](tasks/P1-02.md) | architect | P1-01 | merged ([review](tasks/P1-02-review.md)) | agent/P1-02 |
| [P1-03](tasks/P1-03.md) | implementer | P1-02 | merged ([review](tasks/P1-03-review.md)) | agent/P1-03 |
| [P1-04](tasks/P1-04.md) | implementer | P1-03 | merged (align ids fixed on main) | agent/P1-04 |
| [P1-05](tasks/P1-05.md) | implementer | P1-02 | merged (OPC plumbing parts set to supported on main) | agent/P1-05 |
| [P1-06](tasks/P1-06.md) | implementer | P1-02 | merged (schema issues 1–2 fixed on main) | agent/P1-06 |
| [P1-06b](tasks/P1-06b.md) | implementer | P1-06 | merged | agent/P1-06b |
| [P1-07](tasks/P1-07.md) | implementer | P1-02 | merged (oracle built locally, smoke test passes) | agent/P1-07 |
| [P1-12](tasks/P1-12.md) | implementer | — | merged | agent/P1-12 |
| [P1-08](tasks/P1-08.md) | test-engineer | P1-03..07 | merged | agent/P1-08 |
| [P1-09](tasks/P1-09.md) | reviewer | P1-08 | merged (approve with fixes: 0 blocker, 2 major, 8 minor) | agent/P1-09 |
| [P1-10](tasks/P1-10.md) | coverage-auditor | P1-08 | merged (pass: 0 blocker, 2 major, 7 minor) | agent/P1-10 |
| [P1-11](tasks/P1-11.md) | cleanroom-auditor | P1-08 | merged (pass with findings: 0 blocker, 1 major, 4 minor) | agent/P1-11 |
| [P1-13](tasks/P1-13.md) | implementer | P1-09..11 | merged | agent/P1-13 |
| [P1-14](tasks/P1-14.md) | implementer | P1-09..10 | merged | agent/P1-14 |

States: not written · outline · ready · in progress · in review · merged · blocked.

## Phase history

| Phase | Report | Gate |
|---|---|---|
| 0 | [phase-0.md](progress/phase-0.md) | green (2026-10-12) |
| 1 | [phase-1.md](progress/phase-1.md) | green (2026-10-13) |
| 2 | [phase-2.md](progress/phase-2.md) | green (2026-10-14) |
| 3 | [phase-3.md](progress/phase-3.md) | green (2026-10-16) |
| 4 | [phase-4.md](progress/phase-4.md) | green (2026-10-18) |
| 5 | [phase-5.md](progress/phase-5.md) | green (2026-10-21) |
| 6 | [phase-6.md](progress/phase-6.md) | green (2026-10-24) |

Shared file note: P1-03, P1-04, P1-05 all add dependencies to `xtask/Cargo.toml` (and `Cargo.lock`); P1-07 adds an arm to `xtask/src/main.rs`. The master resolves these merge conflicts.

Open master items: none (the codegen task is part of the P2-01 plan).

## Deferred minor findings (for the phase report)

- P1-12: `download` writes the file before verifying sha256; verifying the in-memory body first would avoid ever placing a bad file at the final path.
- P1-12: `ureq` uses the private `_ring` feature; it may break on a ureq minor update. Revisit if the webpki-roots licence (CDLA-Permissive-2.0) is accepted in `deny.toml`.
- P1-07: CI does not build the oracle yet. When the GitHub remote exists, add an oracle build step on Linux and macOS (apt/brew deps from `tools/libvisio-oracle/README.md`), set `LARRYGRAM_LIBVISIO_ORACLE` and `LARRYGRAM_REQUIRE_ORACLE=1`, and run the ignored oracle tests. Windows skips.
- P1-07: `cargo llvm-cov` excludes `tests/` by default, so a crate's coverage counts only `src/`. The vsd-binary threshold becomes meaningful once it has library code (Phase 7).
- P1-06b: a test that loads the committed `ui-spec/*.yaml` with real `commands.yaml` and `shortcuts.yaml` and pins the real control counts. P1-03 covers the layouts with empty tables; P1-04 adds a loader check; P1-08 (test-engineer) owns the final version.
- P1-04: 15 VisUICmds constants contain underscores (`visCmdSetPageLineJumpCode_None` family and similar) and are skipped because the `commands.yaml` pattern is `^visCmd[A-Za-z0-9]+$`. Widen the pattern if Phase 4 needs them.
- P1-04: `xtask/src/extract/shortcuts.rs` `ACTION_TO_COMMAND` uses ~61 short phrases from the support page as lookup keys. They are needed to match rows and are not shipped. P1-11 (cleanroom-auditor) must confirm this is acceptable.
- P1-04: `shapesheet.toml` has no row-type families (MoveTo, LineTo, ...); `shortcuts.yaml` has no way to mark held keys such as "Arrow Keys". Revisit in Phase 2 (codegen) and Phase 4 (shortcuts).
- P1-08: the fuzz crate `crates/ui-spec/fuzz` uses edition 2021 (cargo-fuzz default); move to 2024 with the next fuzz target. Seed a fuzz corpus for CI runs later.
- P1-08: `cargo xtask extract --check` is not in the gate (CI has no `reference/`). The master runs it at each phase end.
- P1-14 (decided): a Fluent id is not unique within a layout. The same control appears in several places (command bar, QAT, backstage), so a placement is identified by its path in the layout, not by its id. Command ids stay unique (enforced). Repeats inside one container (`TabSave` x9 in one backstage tab, `LineStylesDialog` twice in one group) look like extractor artefacts; review them when Phase 4 builds the command bar, and record the rule in ADR 0007 then.
- P1-14: serialized layouts are not schema-valid for write-back (the model lacks `backstage.kind` and `quick_access_toolbar.id`). Fix before the screenshot pass writes layouts back.
- History rewrite (P1-11 F1), done 2026-10-13 with the user's approval: `git filter-repo` replaced the blob of `docs/tasks/P1-04.md` that held the verbatim support-page text with the cleaned version. The final tree is unchanged; commit hashes from `f338256` onward changed, and hashes quoted in docs were updated. A pre-rewrite mirror is at `/Volumes/Dara/larrygram/backup/larrygram-pre-rewrite.git`. Delete it before the first push.
- P2-01: the canonical angle unit is radians (ADR 0011, amended by P2-06 from corpus evidence; P2-06b moves `geom` to radians). The MS-VSDX XSD licence is checked in P2-09; until then the XSD is never committed. The coverage ratchet for geom, model, vsdx and shapesheet is set at the Phase 2 end.
- P2-02: the ShapeSheet table has gaps, so the generated defs use `Option`: 10 of 420 cells have no cell index, the Layer Membership section has no section index, and 8 of 51 rows have no owning section. Fill them if a later source gives the values.
- P2-05 (decided): the package reader accepts a package without `[Content_Types].xml` and infers declarations, because two libvisio corpus files have none. A dangling internal relationship is recorded, not an error. The XML helpers live in `crates/vsdx/src/package/xml.rs`; there is no `src/xml/` module.
- P2-04 (decided): the model keeps cell and section names as spelled in the `.vsdx` file; P2-06 adds a name-alias table in `crates/model/src/aliases.rs` for the ShapeSheet-table lookups. `larrygram_model::Error::NotImplemented` and the `vsdx` one are removed after P2-07. Table defaults are parsed lazily and interned with `Box::leak` (bounded by the static table); move the parsing into codegen once `shapesheet.toml` records default values.
- P2-06 (decided): the writer patches the source part trees (P2-07, ADR 0018), because unmapped children interleave with mapped ones in the corpus. Model gaps (section-level cells, several master shapes, style references by ID, `Connect` preservation, radians in geom) go to P2-06b. `tests/roundtrip.rs` moved from P2-09 to P2-07; P2-10 adds schema validation of written parts.
- P2-06b (decided on main): a cell whose formula is `Inh` does not define the cell for the lookup (ADR 0013 amendment); its cached snapshot is not used as a fallback yet. Duplicate style IDs resolve first-wins. A chained master rung reuses the instance's sibling position; revisit if a file needs more.
- P2-07 (deferred to Phase 4): deleting a page or master leaves its part behind; a new `Property` is written into `DocumentProperties`, not `docProps/*`; edits to `Preserved` are not written back; only the first `Shapes` wrapper of a shape is patched; changed booleans are written as `0`/`1`. The generator silently stops at 128 sub-shape levels; make that an error when Phase 4 can build deep models.
- P2-09 (decided): the only public XSD (Office developer docs) is XSD 1.1, uses an older namespace and is out of sync with files Visio writes; 3 of 106 corpus source parts validate against the derived schema. The schema test checks that the writer adds no new validation errors, not that parts are valid. The libvisio oracle compares only page counts and page sizes for `.vsdx`; full comparison waits for Phase 7.
- P2-10 (open, for the audits and fix tasks): a model built in memory loses `Bool`, `List` and number-like `Str` kinds on write then read, because the writer emits no `U` for them; the arena holds nested sub-shapes in wrapper order, not document order; the schema test skips without `xmllint` and `reference/` (CI should install libxml2 and set `LARRYGRAM_REQUIRE_SCHEMA=1`).
- P2-14 (open): a hand-built `Color`/`List` value with a unit, or a `Bool`/`Str` with a real unit, does not read back the same (formula-engine task); the relationships writer always uses the default namespace for the relationships vocabulary.
- P3-01 (open): the fixed palette 0..23 is not transcribed yet (P3-02). Per-shape `ThemeIndex` is deferred; the renderer uses `themes[0]`. The `Ellipse` row reading (conjugate semi-diameters) needs a black-box check for the oblique case. Linux golden drift may need the metric loosened in `tests/golden.rs`. Corpus colour cells cache `Themed` and palette indices; ADR 0021 did not cover them (master note in P3-02).
- P3-08 (open): dashes scale with line width (ADR 0023), which may not match Visio; line ends 25..45, fill patterns 25..40 (gradients) and patterned shadow bodies are undefined and return `None` with a warning; the `.vsd` corpus was not scanned for style indices. The scene builder (P3-05) must follow the unit-box and tile-box conventions documented in `styles.rs` and `visual-styles.yaml`.
- Master (2026-10-14): new advisories RUSTSEC-2026-0346..0349 against `rustls` 0.23.45 (xtask via `ureq`) failed the deny step; fixed with `cargo update -p rustls` (0.23.46).
- P3-04 (decided, ADR 0024 amendment): `layout` bakes the block rotation into glyph positions, and the backends (P3-06, P3-07) rotate each glyph outline; `Warning::MissingGlyph` carries the `char`; a bullet sits at `indent_left + bullet.indent`; Selawik has no italic faces, so an italic Segoe UI run falls back to Carlito Italic with a `MissingFont` warning (a synthetic slant needs an API extension). The bundled fonts are 2.96 MB and are compiled into every binary, including the web build; revisit lazy loading in Phase 6.
- P3-03 (open): the knot-vector rule (stored `n + 1` knots padded with the last knot) is unconfirmed: no corpus file has `NURBSTo` or spline rows, and the Result cites libvisio's reading, so P3-14 checks its provenance (master note). Spline and NURBS curves come out as straight chords within tolerance, not as fitted curves. The `NURBSTo` fallback rule and the knot-list assembly go to P3-05 (master note).
- P3-02 (open): palette indices 0..23 have names from `VisDefaultColors` but no Microsoft source gives their RGB values, and the corpus has none below index 24; the RGB values are the conventional ones and are flagged unverified (check in the screenshot pass). Deferred: variant theme colours (`QuickStyle*` values 100+ and 200+), the `a:fmtScheme` line and effect matrices, the `dk1`/`lt1` slot check, the packed-RGB index guess, `PolylineTo` `xType`/`yType` (dropped by the fixed enum), and addressing several sections of the same name through the public API.
- P3-05 (open, the rest go to P3-15): `Txt*` placement cells are used only when the shape defines them; `TextBkgnd` has no slot in `TextNode`; tab stops ignore the `tp` row and multi-stop rows; bullets use the run family and our own symbols. Approximations for the screenshot pass: `Rounding > 0` becomes a round join; line gradients are ignored; rectangular and path gradients degrade to linear with a warning; the arrow size scale and the pattern tile pitch are ours. `Scene::background` is always `None`. A group with its own geometry emits its shape as the first child of its `Group`.
- P3-06 (open): underline and strike-through are not drawn yet (`TextRun` carries them; no backend draws them); `Shadow::blur` is not applied (tiny-skia has no blur); a shape clip does not clip its own shadow; the pattern tile is a background box plus a filled tile path. `text::Layout` now carries `rotation` and `GlyphPos::size`, which overlap with `TextNode::rotation` and `TextRun::size` from P3-05: the backends use the layout fields. The reviewer (P3-12) should decide whether to keep one.
- P3-07 (open): `PatternPaint` has no `filled` flag or tile stroke width, so the SVG backend strokes open tiles as hairlines while the PNG backend fills the tile path; the two backends can differ on pattern fills (P3-12 decides the fix). Underline and strike-through are drawn by neither backend. Shadow blur needs `filter` in the ADR 0025 SVG subset. Line ends on dashed strokes are drawn solid. `render_svg`/`render_png` have no `InvalidOptions` error for a bad scale.
- P3-09 (open): without `--background`, PNG output is transparent (`Scene::background` is always `None`); `export` takes no `--background`; the inspect shape count, the main.rs error branch and the page/export exit-code split are listed in the P3-09 Result. Master smoke run (2026-10-15): all 15 corpus files render to PNG and SVG after the page-size fix, but themed shapes are invisible because `Themed` patterns resolve to 0 (added to P3-15).
- Master fix (2026-10-15): pages without a positive `PageWidth`/`PageHeight` (two corpus files have no page sheet) now render at 8.5 x 11 in instead of failing with `TooLarge`.
- P3-15 (open): the style-matrix colour modulation (about a 0.73 factor) is not applied; themed dash presets and shadow patterns fall back; cells with no documented default still warn `Missing`; inherited geometry row cells are the master's cached values (master-sized geometry on resized instances) until the formula engine (Phase 6; closed by P6-05); `QuickStyle*Color` 200..206 has no published meaning and is resolved like 100..106 from corpus evidence; a themed `LineWeight` falls back to 0.7 pt.
- Master fixes (2026-10-15): scene node transforms are now relative to their parent group (the builder stored page transforms, so nested shapes were offset twice); the fixed palette RGB values come from [MS-VSDX] 2.2.7.6 (indices 14, 16 and 18 corrected), which closes the P3-02 "unverified" item. Master smoke render: all 15 corpus files render to PNG and SVG with exit 0; themed fills, gradients and text are visible.
- P3-10 (open): the oracle comparison covers 3 of 15 corpus pages (`blue-box`, `qs-box`, `color-boxes`); the others skip on text drawn as paths, group flattening, unresolved themed paints, and a smaller geometry box than the oracle's on `testfile1`/`testfile5` arcs and ellipses (check the `Ellipse` and inherited-geometry cases). Tighten `skip_reason` in `crates/render/tests/oracle_render.rs` as those close. Golden metric may need loosening on Linux. The text fixture pins approximate tab and bullet rendering. Master fixes at merge: the root `**/fuzz/corpus/` ignore rule dropped the `render_document` seed corpus (now re-included by `crates/render/fuzz/.gitignore`), and `crates/vsdx/tests/fixtures.rs` now walks every fixture variant.
- P3-11 (open): `text::layout` can emit non-finite glyph positions for extreme finite input (`line_spacing: f64::MAX`); the backends sanitize, so it is hardening for Phase 6. `render_svg` has no upper bound on `scale` (PNG caps the raster); P3-12 decides whether both backends share one cap. Report: `docs/reviews/phase-3-tests.md`.
- P3-14 (fixed on main): `render_png` panicked in tiny-skia on huge finite coordinates under a rotation (F4). Fills, strokes and clips whose device-space bounds, widened by the stroke, pass +/-2^22 px are now skipped (`fits_device` in `png.rs`), with a regression test. Clean-room minors F1-F3 go to P3-18; the knot rule rests on [MS-VSDX] and the `DrawNURBS` remarks, not libvisio.
- P3-18 (open): the black-box check of the knot rule stays open (no corpus file has `NURBSTo` or spline rows); a periodic spline (first knot repeated fewer than `degree + 1` times) is read as clamped, a documented divergence.
- P3-16 (open): `text::Layout` `MissingFont`/`MissingGlyph` warnings do not reach `Scene::warnings` or the CLI; boolean and list coercions still accept non-finite values (Phase 6); `Character`/`Paragraph`/`Tabs` rows without an `IX` no longer match run markers by position (every corpus row has an `IX`); un-drawable glyphs are skipped by documented rule, not warned.
- P4-05 (open): the canvas cache detects document changes by comparing a `Document` snapshot every frame (O(document)) because `DocumentSession` has no generation counter; switch to a counter once `crates/commands` has one. Our own UI choices to revisit in the parity pass: 1/8 in grid and ruler ticks, zoom range 0.1..64, 10% per wheel notch, Ctrl+0 for `view.zoom.fit`. The raster scale is also capped by the GPU `max_texture_side`.
- P4-06 (open, Phase 5): stencil drag onto the page should set `PinX`/`PinY` at the drop point and glue to `MasterDef::connection_points`; `instantiate` has no size parameter; Shape Data values are strings until the formula engine. Master fix at merge: the preview test now requires a raster for every master (it predated `render_raster`).
- P4-02 (open): `file.open` does not update `Settings::recent_files`; `view.zoom.fit` repeats the renderer's 8.5 x 11 in page fallback (keep the two in step); a wasm `file.save` with a `path` argument fails at runtime; `app.quit` stays a stub (the shell quits itself). `DocumentSession` has no change counter, so the canvas keeps its per-frame document comparison: Phase 5 should bump a counter on every document edit (undo stack) and the canvas should use it.
- P4-04 (open): the ADR 0031 dedup key (id, label, order) does not collapse the extractor's nine `TabSave` placements (their orders differ), so nine widgets render; decide in an ADR 0007 review or fix in the extractor. A dropdown button with items does not dispatch its own command. P4-04 added `insta` and egui's `default_fonts` feature to `crates/ui/Cargo.toml` (P4-03's file).
- P4-03 (open): the shell does not host `shortcuts::dispatch` (P4-14) or call `TabBar::set_context_menu` yet (the committed context menus carry no controls). `app.quit` and `app.office.center.options` are intercepted by `Shell::dispatch` because their registry rows are stubs; a `view.options` builtin returning `UiRequest::ShowOptions` and one quit id (`app.quit` vs `file.exit`) would remove the intercepts. `Shell::show(&Context)` panics inside `eframe::App::ui` (egui 0.37 `root_ui`), so the apps call `Shell::show_in`. On wasm only one async pick runs at a time; the desktop `NativeFilePicker` bodies are untested (flows tested through a fake picker). Settings persist on change, at `App::save` and on shutdown.
- P4-07 (open): pane warnings ride in a "Warning" row instead of `AppCtx::warnings` (the builders take `&AppCtx`; give them a warning sink in Phase 5); the panes show a non-positive page size as is while the canvas falls back to 8.5 x 11 in (one rule in the parity pass); property labels are the raw element names; the `egui::Grid` id is the pane title. Master fix at merge: the shell frame test recorded a warning and then asserted it; it now asserts a real frame raises no widget stub error.
- P4-08 (open): the first conflict rule keyed on the sequence alone and dropped 29 of 90 entries, so `Ctrl+C`/`Ctrl+V`/`Delete`/arrows worked in one scope only and `tools.format_painter` was lost; ADR 0036 is amended (key = sequence + scope, most specific active scope wins) and P4-14 implements it plus the shell pass. Load errors use `UiError::Layout(LoadError::Parse)` (no shortcut variant); `Ctrl+Backspace` in text defers to egui; shifted symbols vary by real keyboard layout.
- P4-14 (open): focus tracking is pointer-press based (the previous frame's canvas and command bar rects), so a keyboard-only focus move does not change the scope; `ShortcutMap::resolve` gained an `&ActiveScopes` argument; the committed table has 74 bindings, 16 exact repeats and no real conflicts. Master fix at merge: P4-14 let every bound `Ctrl` chord dispatch inside a text edit (so `Ctrl+C`/`Ctrl+V`/`Ctrl+Z` ran registry commands instead of egui's copy, paste and undo in dialog fields); egui's own editing chords defer to egui again (ADR 0036 amended), and other bound chords such as `Ctrl+Shift+C` still dispatch.
- P4-09 (open): parity is now derived (7 implemented, 685 stub, 14 out-of-scope) and checked by the `parity` gate step. 5 of 15 corpus files rasterize blank: their shape or page cells have no cached values and the builder substitutes 0 (`FillPattern`/`LinePattern`/`LineWeight` on `dwg`, `fdo86664`, `testfile5`; `Width`/`Height`/`PinX`/`PinY` on `tab-short-prefix`), and `recursion-cycle.vsdx` has no page part; `SKIP_RASTER`/`KNOWN_BLANK` in `apps/larrygram/tests/open_corpus.rs` go when the value pipeline (P3-15/P3-16, Phase 6 formula engine) closes this. `Shell` exposes no canvas raster, so the exit test reads it through a `canvas::show` frame. The 365 backstage has 205 placements but 127 unique ids (same as the P4-04 `TabSave` item). P4-09 also added `crates/commands/tests/headless.rs` (outside its listed files; test-only, kept). ADR 0037 now says out-of-scope beats implemented.
- Master fix (2026-10-17): RUSTSEC-2026-0367..0369 (zip `extract()` bugs) turned `cargo deny` red on main. `crates/vsdx` and `xtask` now use zip 9; zip 8.6 remains only through calamine in xtask, which never calls `extract()`, and `deny.toml` ignores the three ids with that reason (ADR 0014 updated).
- P4-10 (open): fixed `CommandId::intern` leaking every rejected id. Open for the fix pass: a chord that mismatches a half-typed sequence is dropped when the mismatch fires the held-back completion (retry it as a fresh start?); `builtins::with_undo` puts the undo stack back by document index (Phase 5 risk); `UndoStack::default()` has capacity 0; the 4096-id intern pool is a test hazard; `crates/ui` has no `proptest` dev-dependency; `crates/ui-spec/fuzz/.gitignore` lacks `!corpus/` and the new `crates/ui/fuzz` targets have no seeds. Report: `docs/reviews/phase-4-tests.md`.
- P4-13 (fixed on main): the two ui `insta` snapshots and the `proptest-regressions` seed files had no `ATTRIBUTION.md` rows; rows added. Report: `docs/reviews/phase-4-cleanroom.md`.
- P4-17 (open): the `render` and `vsdx` fuzz seed corpora are flat, but cargo-fuzz auto-loads only `corpus/<target>/`; move them to per-target directories in a later fuzz pass. Master fix (2026-10-18): RUSTSEC-2026-0370..0372 (zvariant, zbus_names, zvariant_derive unmaintained, no fix) under eframe's Linux accessibility bridge turned `cargo deny` red on main; ignored in `deny.toml` with that reason.
- P4-15 (open): the parity matrix has one row per Fluent control, so command-only ids (`app.office.center.options`, `app.quit`, `file.exit`, `view.units`) do not appear in `docs/parity/controls.md`; the master's P4-15 criterion asked for it by mistake. Phase 5: add a `| command | label | status |` table to the generator. `with_undo`'s placeholder stack is zero-capacity and visible to an `Edit` while it runs. The 4096-id global intern pool is still a test hazard (`intern_into` gives tests a private pool). `commands` dropped `serde-saphyr` and builds its stubs from `Loader::from_embedded()`; `DocumentSession::id` is the new session identity.
- P4-16 (open): the context menus stay parked (`TabBar::show_context_menu` / `set_context_menu` never called; the committed menus carry no controls). The canvas "inconsistent raster size" guard is unreachable through `render_raster`. The rfd `pick_file`/`save_file` calls and `open_dialog`'s configuration are desktop-only and untested (the rest sits behind `picked_path`/`save_picked`). `ShortcutMap::resolve` and `shortcuts::dispatch` return `Vec<Intent>`: a mismatching chord fires the held-back completion and then starts a new sequence (ADR 0036 sketch updated by the master). A programmatic outcome still updates the status line; dialogs, downloads, quit and confirm are interactive-only. Closes P4-03's intercepts and `Shell::show` items, the P4-10 mismatch item and P4-15 open issue 4 (edits land on the session the command ran against).
- P5-02 (open): a new page with no shapes gets no contents part and no `Rel` (the P2-10 write-stability rule kept over ADR 0048's wording); check with Visio as a black box that such a file opens, and change the rule and `write_edge_cases.rs` together if it does not. `scale_geometry` drops cells the typed rows do not model (`NurbsTo` E, `PolylineTo` A, `Unknown` rows) and does not scale arc bows, elliptical ratios or spline knots, so non-uniform resizes distort them slightly (Phase 6 recalc replaces it). Master rows without `IX` are overridden by position. Group bounds use the shape frame, not the geometry. The writer still allocates relationship ids with `fresh_rel_id`, not `Package::next_relationship_id`. `model::mutate` is now `pub` (the glue constants).
- P5-03 (open): `snap_axis` takes the nearest candidate with ties broken by order, with no ADR 0043 kind priority; check against Visio in the screenshot pass. Text selection bounds are the layout box, not the glyphs (revisit with text editing, P5-07). A rotated group's bounds overestimate slightly. `hit_test` ignores clips. The marquee rule and the 4 px / 8 px constants are our own. Master fix at merge: `geom::Error::InvalidArgument` and `render::Error::InvalidNode` replace the borrowed `WrongUnit` and `InvalidOptions`, and `geom` exports `FLATTEN_TOLERANCE`. Merge gate after P5-02: model coverage 97.7% < 99.6% ratchet (P5-02 skipped coverage under the new trigger rules). P5-15 restores it, and AGENTS.md now runs coverage for every code change.
- P5-15 (open): `lookup.rs` `repeated_cell`'s `Some(row)` arm is dead (only caller passes `None`). Unreachable defensive fall-throughs in `document.rs` 134, `raw.rs` 243/265, `stencils/mod.rs` 189. Uncovered test-module arms in `geometry.rs` and `stencils/mod.rs`. A tidy pass may remove dead arms, but never by switching to `expect` (AGENTS.md section 6).
- P5-04 (open): undo restores cells straight into `Sheet::cells` (`restore_cells`) because `Document::write_cell` cannot express "no entry"; a later model task may add `Document::restore_cell`. A third undo step `GeometryEdit` exists for resize. `StructuralEdit::insertion` is `#[allow(dead_code)]` until P5-06..P5-08 use it. `touched_cells` mirrors the model's `DERIVED_CELLS` (pinned by a test). Our own semantics, to check with a black-box check: rotate is a delta, multi-shape resize gives each shape the target size, the enter-group state. `need_finite` accepts integers.
- P5-05 (open): the rotate handle sits 0.25 in above the shape, not a fixed screen distance (`handles_for` has no zoom). Editable pane fields take inches and degrees, not `Settings::units`. `builtins::view::page_size` is still private, so the panes keep their own page-size reader. Black-box checks: the rotate handle offset, the marquee intersection rule, Alt as the snap modifier. P5-11 can reuse `CanvasRig` (`crates/ui/tests/phase5/main.rs`) for the 8 ms drag budget. Selection through the registry and master connection points moved to P5-16.
- P5-16 (open): `edit.select` has no explicit entered-group argument (callers map a hit to the entered group's child first). `Document::connection_points` takes the nearest master that has rows and scales by direct `Width`/`Height` cells only; inherited sizes without a cached value stay unscaled until the Phase 6 formula engine.
- P5-06 (open): undoing the first drop of a catalog master leaves the materialised master in the document (needs a master-removal mutation). Glue records use `Connections.X<n>` and connection-point parts; compare 2D glue cell spelling with Visio with a black-box check. Master fix: the draw and stencil drags moved from address-keyed thread-locals into `ToolMachine` fields (`11817bb`). Tool picking, the Shapes-window drag and indexed connection points moved to P5-17.
- P5-07 (open): the editor commits one run. Field text becomes literal, and multi-run formatting markers collapse to the first run. The editor box is axis-aligned for rotated text. A new text box is a fixed 2 x 0.5 in, our own size. Black-box checks: whether Escape or a click outside commits, the default box size, and where the caret starts. Master fix at merge: the text-tool test now uses P5-06's real `insert.drawing.text_box`. Showing the editor in the app moved to P5-17.
- P5-08 (open): the tool finds the connector through `insert.shape`'s selection and the Basic Flowchart "Connector" master name. Outline glue uses page-aligned bounds, not the rotated outline. A connector inside a group writes its path without the parent transform. `connector.reroute_all` bumps `changes` even when no path changes. Re-routing rewrites every glued connector on the page, so the first edit of a Visio-routed file replaces Visio's paths with ours (ADR 0045 accepts this). One-end unglue edits `Page::connects` directly, not through the model's mutation API. Master fix at merge: `reroute_after_edit` acts on the active document only. Indexed glue and the connector free end moved to P5-17. Merge gate after P5-08: `larrygram-commands` coverage 97.9% < 98.1% ratchet (P5-08 handed off at exactly 98.1%; the P5-07 and P5-08 merges together fell under it). P5-18 restores it; main is red on that step only.
- P5-17 (open): the registry has no checked-state seam, so tool commands cannot report themselves checked. Typing on a selected shape does not open the editor, and Escape does not return to the pointer tool (no binding; check with a black-box check). The tool choice is application-wide, not per document. A free connector end lives in the connector's local geometry and moves with it. The shell takes Escape from `view.full_screen_mode` while the editor or a stencil drag is active. `connector.draw` is a new Larrygram-only id (977 rows). The worker also removed two unreachable checks in `insert.rs`. Commands coverage was still 97.9% after the rebase; P5-18 restores it.
- P5-18 (closed): tests only, no bugs found. The branch measured 98.2% before P5-17; on main after both merges it is 98.1% (7491/7635), exactly at the ratchet, so the merge gate is green with no margin. The remaining uncovered lines are listed in its Result; most are `?` arms of constructors after validated ids.
- P5-10 (open): the ADR 0048 comparison in the flowchart harness canonicalises `ShapeId`s and drops two reader artefacts. `crates/vsdx` keeps the writer's `U="BOOL"`/`U="STR"` kind spelling in `Cell::preserved`, and keeps the `<Rel>` child of a read `Page`/`Master` in `Preserved`; a reader fix would remove both rules (Phase 6 candidate). The out-of-scope match for command rows is case-insensitive, and a command whose every Fluent id is out of scope is out of scope too. Some `commands.yaml` labels are extractor artefacts (`tools.macro.dlg` "Dlg"). `file.new` makes a document with no pages; check Visio's new-drawing behavior with a black-box check. The `larrygram-model` dev-dependency of `apps/larrygram` was accepted with its `Cargo.lock` line.
- P5-11 (open): tests found and fixed eight product bugs (`Connect` restore order, z-order and group undo, page snapshot restore, missing re-route after paste/duplicate/drop glue, grouped connector transform, drop glue treated as a connector, the 80–110 ms pointer drag frame now 0.6–1.6 ms). The draw, connector and stencil drags still rebuild the page scene per snap call. A pasted fragment's `Connect` records naming targets outside the copied set need a test. Routed-path comparisons fold to 12 significant digits because a free end drifts by an ulp per re-route pass.
- P5-19 (open): the three 8.5 x 11 in page-size fallbacks are now one `larrygram_render::FALLBACK_PAGE_SIZE` (master, `28f6774`). `larrygram-commands` misses are now the audit's keep arms and the staging `?` arms of the connector commands. Clippy 1.95 added `err_expect`. The `master_size` guard now reads the raw cached master cells, and the audit's stale-target re-route arm proved unreachable. Merge gate: a new advisory (RUSTSEC-2026-0382, `rustls-webpki`, via `ureq` in xtask only) turned `deny` red; the master bumped it to 0.103.16.
- P5-20 (open): `write/patch.rs` `sync_uint` 138-140 need an inline unit test; the connector tool finds its master by name, so its missing-master arm needs an injectable lookup; the render `_` arms for future `#[non_exhaustive]` variants and the proven-dead arms are listed in its Result for the next coverage audit. The page-tab Rename asks for a name in the status bar because the shell has no text dialog.
- P5-09 (open): align and distribute ignore `Angle`; check Visio's rotated behavior with a black-box check. `arrange.align.center` (the Fluent row) is not implemented; `arrange.align.center_x` is. Group, ungroup and insert undo leave detached arena entries, so the P5-11 undo property must compare reachable state, not `Document` equality (or an arena-compaction ADR). Page undo snapshots the whole page list. Cross-document paste keeps a `Master` link the target may lack; the writer needs a dangling-master rule before that ships. The shortcut extractor drops "open the page tab context menu" (no mapped action).
- P6-03 (open): `coerce` returns `Coerced { value, defaulted }` instead of `Result` (the stub could not carry the warning). No `proptest` dev-dependency in `shapesheet` (seeded loops instead; add it with the P6-11 test pass). Area units (`SQ IN`, `HA`, `ACRES`) are a syntax error by decision. Black-box checks: how a number with a unit prints as a string; boolean-to-number conversions (basis recorded in the Result). List-entry kind flips are documented caveats.
- P6-06 (open): master rungs are addressed as the master's first top-level shape and style rungs have no `CellAddress`, so a style edit does not dirty instances through the graph (fine while only page shapes and instances are edited). `write_cell`'s derived-cell refresh is outside `RecalcOutcome::updates` (P6-07 note). A unit suffix in a `SETF`-written formula needs the printer. `DEFAULT_MAX_EVALUATIONS` is unmeasured (P6-10). `formula_types.rs` still asserts the `parse` stub error (P6-02 updates it).
- P6-05 (open): `CellReader` now prefers a cell's own `Inh` snapshot (ADR 0054/0057), so instance geometry renders at the instance's size (P3-15 closed); the preference sits in `values.rs` plus a 3-line hook in `geometry.rs`, not in `lookup.rs` (moving it there would flip two `Inh` lookup tests). `TheDoc!` resolves against `Document::properties`, because the model has no document sheet. The missing-reference warning is named `UnsupportedReference` until P6-04 renames it.
- P6-04 (open): an unknown function keeps the cached value through a warning plus an `#N/A!` placeholder (the evaluator signature has no keep-cached channel). `EvalWarningKind::UnsupportedReference` is ADR 0054's `MissingReference` (name kept). Four warning kinds do not reach `RecalcOutcome` (P6-11 note). Black-box checks: `MODULUS` sign, `CEILING`/`FLOOR` on negatives, unit-ed number to string (`1 in & ""`), string to boolean. `MOD` is an alias of `MODULUS`; `EXP` has no reference page and is kept per ADR 0053.
- P6-02 (open): 976 of 994 unique corpus formulas parse and all of those round-trip; the 18 others are `FORMATEX` calls with omitted arguments, rejected by decision. `37%` is the percent operator; the `%` unit needs brackets (`5 [%]`). `Expr` equality includes spans, unlike ADR 0051 (P6-12 rules). The master switched `resolve.rs`'s refused-`SETF` errors to the new `Error::SetfRefused`.
- P6-08 (open): the fill-in pass is a no-op on the corpus (no cell has a formula without a cached value). The blank `dwg`, `fdo86664` and `testfile5` come from the model's style lookup ignoring the master's style links (P6-15). `tab-short-prefix` and `recursion-cycle` have nothing to draw by construction. The `Value::to_cached` round-trip test lives in `crates/commands/tests/` (vsdx may not depend on shapesheet). P6-16 removed the P5-10 reader artefacts; P6-11 fixes section-level cells in `graph::effective`.
- P6-07 (open): an edit's undo entry restores sheet snapshots taken before the recalc pass (so derived refreshes and `SETF` targets revert too); redo re-runs the pass. Our catalog masters have constant geometry, so catalog and drawn shapes still scale their rows literally on resize (formula geometry for the masters is a model task). An instance with no own geometry rows cannot be materialised (needs a row-insertion mutation). Page add and delete recalculate nothing. `Document::scale_geometry` has no caller in `commands`. `builtins/file.rs` still runs the fill-in pass inline; switch it to `recalc::fill_missing` in the fix task. Black-box check: does Visio scale literal-constant geometry rows on resize?
- P6-16 (open): the reader keeps a cell's `U` code only when the writer would not write the same code back for that cell (a per-cell rule, because the writer strips `U='STR'` on text such as `Bottom` and `U='COLOR'` on `#ffffff`); a read `COLOR` code is never re-derived. The writer change (`write/mod.rs`, `source_rel_id`, outside the task's files) recovers the contents relationship id from the source element, or every patched file grew a second `Rel`. Stale doc comments in `read/value.rs` and `write/value.rs`; a keep-first tweak in `write/value.rs` would let the reader rule become a static list. P5-10 comparison rules removed from `flowchart.rs`.
- P6-09 (open): `crates/ui` now depends on `larrygram-shapesheet` (runtime). Shape Data values are display-only: no command writes a `Prop.Value` cell (`data.set.property`, reserved in ADR 0035, would write through `CellEdit::direct`, which is `pub(crate)`); pane edits dispatch `shape.set.*` and refuse formula cells first. The Shape Data pane lists the shape's own `Property` rows, not the union with the master chain. An unknown error spelling such as `#NAME?` shows a dash. Open design question: whether Size & Position lists the `LocPin`/`Flip` cells and whether formula fields become editable through a formula bar.
- P6-10 (open): cross-check over 15 corpus files: 5 184 counted cells agree (100.0%); 4 058 skipped (2 589 no formula, 27 do not parse, 629 unknown function, 781 missing reference, 32 documented exclusions in ADR 0059, six causes). The resize exit test runs on `testfile5.vsdx` (22 evaluations against the 100 000 default budget). Fixed in `resolve.rs`: reads prefer cached values; `Geometry1` is the first Geometry section; a row index is the row's `IX`; an `Inh` snapshot on a style sheet is local. The document sheet and the master sheet are not modelled (most of the 781 missing references); style rows are not addressable by `SheetRef`. ADR 0059 needs the boolean bridge and the hard-default reading (P6-12 rules).
- P6-15 (open): an instance with no style link of its own uses the first link of that kind on its master chain; a dangling link of its own still wins. `tab-short-prefix` and `recursion-cycle` stay marked as having nothing to draw by construction. `Resolved::via` does not name the master that supplied the style link. P6-08 open issues 1 and 6 closed; ADRs 0013 and 0057 amended.
- P6-11 (open): four product fixes with regression tests: section-level cells are evaluated and written (new `Document::write_section_cell` in model), a formula that does not parse keeps its cached value with a `Syntax` warning, the four dropped warning kinds reach `RecalcOutcome`, and a master rung without the section no longer stops the chain. `commands::recalc::warning_line` prints the four new kinds through its fallback text (fix task). `vsdx` reads non-finite numeric literals that the writer rejects (pinned; hardening in the fix task). The master removed 541 libFuzzer-generated inputs committed beside the 14 seeds (`509fcec`). Report: `docs/reviews/phase-6-tests.md`.
- P6-12 (open): approve with fixes, 0 blocker, 2 major, 15 minor (`docs/reviews/phase-6-review.md`). F1: a `SETF` evaluated inline during a read writes through `DocumentResolver::set_cell`, bypassing `Evaluation::writes` and undo (ADR 0056). F2: `graph::is_column` and `resolve.rs::column_known` disagree on column versus named row, so a dependency edge can be lost. Rulings: hand-written `PartialEq` for `Expr` ignoring spans; the omitted-`FORMATEX`-argument error stands (record in ADR 0051); ADR 0059 gets one amendment for the boolean bridge and the hard default, which stops being labelled missing-reference. All go to the fix task.
- P6-13 (open): every crate at or above its threshold; `commands` 98.27 against its 98.3 ratchet, green only inside the 0.05 tolerance. Ratchet proposal for phase close: shapesheet 85.0 to 95.5, ui 96.8 to 97.0, ui-spec 99.3 to 99.4, vsdx 98.1 to 98.2. Critical uncovered paths for the fix task (`docs/reviews/phase-6-coverage.md`): the redo-pass failure arm, the hard-error abort chain in the engine pass, `SETF` into section/row/page, graph edges for cross-shape references by name and bare row cells, `rung_unit` never returning a real unit, coercion edges, junk `COLOR` text in the reader, and the formula read-only marker in the panes.
- P6-14 (open): pass, 0 blocker, 0 major, 4 minor plus 1 outside the diff (`docs/reviews/phase-6-cleanroom.md`). For the fix task: two error-value glosses in `crates/shapesheet/src/value.rs` are quoted verbatim (reword); fixtures and seeds copied from a reference example column carry an "our own bytes" attribution (replace or re-attribute); `docs/sources.md` row 3 does not name the `U`/`Rel` facts of P6-08/P6-16; the proptest-regressions attribution note does not name the Phase 6 files; a blank line in `docs/sources.md` drops rows 21 to 23 out of the table.
- P6-17 (open): review F1 to F5, F7, F8, F13, F16, F17 and clean-room F1, F2 closed. `SETF` has one write path (the resolver records, the pass applies, undo restores); one name rule in the new `shapesheet::names`, and the graph walks master rungs. Warnings carry their originating cell on `TracedWarning` (`EvalWarning` keeps its fields, because `crates/ui` builds it). Cross-check 5 184 of 5 184 counted cells agree. Coverage: shapesheet 97.0, commands 98.33. The master renamed the example strings in `docs/tasks/P6-05.md` to match.
- P6-18 (open): review F6, F9 to F12, F14, F15 and clean-room F3, F5, F6 closed. `data.set.property` writes Shape Data values with undo; the Shape Data pane lists the master chain's rows too (own row wins) and an inherited edit writes a local row. Non-finite numeric literals are rejected at read. Cross-check: 5 184 of 5 184 counted cells agree; skips no-formula 2 589, does-not-parse 27, unknown-function 629, missing-reference 750, default-only 31, documented-exclusion 32. Coverage: commands 98.45. Left for later: the recalc pass re-records the `Value` unit display hint (ADR 0039); `Prop.<row>` formula references build no dependency edge (`shapesheet::names`); `Document::scale_geometry` is deleted.
