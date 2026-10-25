# Phase 8 coverage audit

- Task: P8-12 (coverage-auditor), branch `agent/P8-12`, 2026-10-25.
- Scope: the Phase 8 code — every module of `crates/vsd-binary`
  (P8-02 to P8-07 and P8-15), the binary open path in
  `crates/commands`, `crates/ui` and `apps/*` (P8-09), and the new
  tests (P8-08, P8-10, P8-15, P8-16). The per-crate table covers the
  whole workspace, for the master's ratchet update.
- The question of this audit: does the near-100% of
  `larrygram-vsd-binary` mean the importer's hostile-input paths are
  actually exercised? Answer: the line table is nearly clean but it
  overstates the strength. Every gap below is a *branch*, not a line:
  the malformed-input arms of the payload readers are dark (findings
  1-2), and the tests that would show behaviour rather than line
  counts — the libvisio oracle parity run, the twin comparison, the
  wide-corpus sweep — never run in the gate (finding 8).

## Measurement

AGENTS.md section 7 puts every workspace-wide run behind the gate
queue, so the measurement was a coverage-only gate job (the queue's
`cargo xtask gate` with every step but `coverage` skipped):

```sh
G=/Users/dara/larrygram/tools/gate-queue/gateq
$G submit P8-12 --skip codegen,names,parity,fmt,clippy,test,doc,deny,layers
```

Job 87 at `3768d10ec7` (current `main`), **GREEN**, 10m42s (coverage
step 552.6s); full log
`/Volumes/Dara/larrygram/gates/logs/P8-12-3768d10ec7-20261025-165908.log`.
The coverage step is `cargo llvm-cov --workspace --all-features
--json --summary-only --output-path target/llvm-cov-summary.json`.
Per-file, per-line and per-branch detail came from the same profile
data in the worktree (no rebuild, no second test run):

```sh
cargo llvm-cov report --lcov --output-path target/lcov.info
```

Reading the numbers:

- Zero-hit lines are exact. llvm-cov folds partially covered lines
  into the covered count, so a file's misses understate its dead arms;
  the branch list below comes from llvm-cov's unentered regions (the
  `?` and `else` arms that no run takes), which is where the real
  gaps are.
- Inline `#[cfg(test)]` code counts as instrumented lines, as in
  Phases 4-7. The files under `tests/` are outside the line table
  (llvm-cov's default scope), so the Phase 8 integration suites show
  up only through what they exercise.
- The corpus was present (`corpus/vsd`, `corpus/vsdx`), so the
  corpus-gated tests ran. The oracle-gated tests
  (`oracle_smoke`, `parity`, `#[ignore]`d), the env-gated `twins` and
  `wide_corpus` sweeps and the fuzz seeds did not run in any gate.
- `larrygram-vsd-binary`'s `container.rs` entry in the summary
  includes its child test module `src/container/tests.rs` (79 unit
  tests); the lcov line table merges the two the same way.

## Per-crate results

| Crate | Lines | Covered | Coverage | Threshold | Ratchet | Delta |
|---|---:|---:|---:|---:|---:|---:|
| larrygram-geom | 2501 | 2501 | 100.0% | 85 | 100.0 | ±0.0 |
| larrygram-vsd-binary | 10720 | 10716 | 100.0% | 85 | 100.0 | −0.04 |
| larrygram-model | 10893 | 10862 | 99.7% | 85 | 99.7 | +0.02 |
| larrygram-ui-spec | 3844 | 3829 | 99.6% | 85 | 99.4 | +0.2 |
| larrygram-text | 1767 | 1754 | 99.3% | 75 | 98.8 | +0.5 |
| larrygram-render | 7237 | 7129 | 98.5% | 75 | 98.5 | +0.01 |
| larrygram-vsdx | 7790 | 7656 | 98.3% | 85 | 98.2 | +0.08 |
| larrygram-commands | 11005 | 10829 | 98.4% | 85 | 98.4 | +0.001 |
| larrygram-ui | 18935 | 18476 | 97.6% | 50 | 97.0 | +0.6 |
| larrygram-cli | 1342 | 1308 | 97.5% | 50 | 96.7 | +0.8 |
| larrygram-shapesheet | 6744 | 6541 | 97.0% | 85 | 96.9 | +0.1 |
| larrygram-xtask | 12645 | 11831 | 93.6% | — | — | see below |
| larrygram | 396 | 338 | 85.4% | 50 | 80.0 | +5.4 |
| larrygram-web | 144 | 113 | 78.5% | — | 77.8 | +0.7 |
| larrygram-control | 0 | 0 | n/a | 50 | — | placeholder |

Ratchet column: `[ratchet]` in `coverage.toml` as of `3768d10ec7`.
Delta is the measured percentage minus the ratchet, on the exact
fraction (the table prints one decimal).

### Statement per crate

- **Blockers (below threshold or below ratchet): none**, but with one
  asterisk: `larrygram-vsd-binary` measures 99.9627% (10716/10720)
  against a 100.0 ratchet. It is green only because `cargo xtask
  coverage` tolerates 0.05 for float noise (`xtask/src/coverage.rs`
  `EPSILON`). Strictly, the crate no longer holds 100.0; see the
  ratchet proposal.
- **Zero-margin crates:** vsd-binary (above), commands (98.4007
  against 98.4), render (98.5077 against 98.5), model (99.7154),
  geom (exactly 100.0). One or two lost lines in any of these fail
  the next phase's gate.
- Movement since the Phase 7 audit's measurement (`d9c77ec0f5`):
  `larrygram-vsd-binary` 9946 to 10720 lines (P8-15 rewrote much of
  the importer), `larrygram-model` +57 (`Shape::is_guide`, the
  repeated-cell policies), `larrygram-vsdx` +39 (the guide attribute
  both ways), `larrygram-render` +16 (the guide skip). `commands`,
  `ui`, `ui-spec`, `cli` are unchanged — the P8-09 open path was
  already inside the Phase 7 measurement but outside its scope, so it
  is audited here for the first time.
- `larrygram-xtask` still has no working ratchet: `coverage.toml`'s
  `xtask = 93.4` key names no package (P7-12 minor 9, unfixed).
  `larrygram-control` stays an empty placeholder.

## The Phase 8 code, file by file

### `crates/vsd-binary` per module (the gate's summary numbers)

| Module | Lines | Covered | Coverage | Zero-hit lines |
|---|---:|---:|---:|---|
| `chunks.rs` | 1602 | 1602 | 100.0% | — |
| `container.rs` | 785 | 785 | 100.0% | — |
| `error.rs` | 41 | 41 | 100.0% | — |
| `lib.rs` | 24 | 24 | 100.0% | — |
| `limits.rs` | 15 | 15 | 100.0% | — |
| `pages.rs` | 1243 | 1243 | 100.0% | — |
| `pipeline.rs` | 32 | 32 | 100.0% | — |
| `scale.rs` | 267 | 267 | 100.0% | — |
| `shape.rs` | 2618 | 2614 | 99.8% | 1077, 1117 |
| `style.rs` | 1408 | 1408 | 100.0% | — |
| `text.rs` | 2658 | 2658 | 100.0% | — |
| `warning.rs` | 27 | 27 | 100.0% | — |

The whole crate's dark *lines* are two stale-reference guards in
`shape.rs` (finding 5). Every other gap in the crate is a branch:
unentered `?` and `else` arms inside covered lines, listed in the
appendix and named in findings 1, 2, 5, 6, 7.

### The open path (P8-09)

| File | Lines | Covered | Coverage | Zero-hit lines |
|---|---:|---:|---:|---|
| `crates/commands/src/builtins/file.rs` | 947 | 928 | 98.0% | 265, 770, 877, 917, 949, 1126, 1231-1233, 1236-1237, 1454, 1464, 1482 |
| `crates/ui/src/dialogs.rs` | 146 | 103 | 70.5% | 130-143, 175-189, 347-356 |
| `crates/ui/src/shell.rs` | 2532 | 2398 | 94.7% | 134 lines, mostly carried (P7-12) |
| `apps/larrygram-cli/src/cli.rs` | 103 | 97 | 94.2% | 254, 270, 281, 299 |
| `apps/larrygram-cli/src/cli/exec.rs` | 622 | 609 | 97.9% | 606-608, 622, 643, 700-702, 705-706, 746 |
| `apps/larrygram-cli/src/cli/inspect.rs` | 122 | 122 | 100.0% | — |
| `apps/larrygram-cli/src/cli/parse.rs` | 489 | 480 | 98.2% | 312, 330, 337, 344, 351, 358 |
| `apps/larrygram-cli/src/main.rs` | 6 | 0 | 0.0% | 10-15 (the binary `main`) |
| `apps/larrygram/src/main.rs` | 396 | 338 | 85.4% | 50 lines (app shell) |
| `apps/larrygram-web/src/main.rs` | 144 | 113 | 78.5% | 29 lines (app shell) |

`crates/commands/src/registry.rs` (command dispatch) is 326/326.

## Is the new code meaningfully tested?

**Mostly yes on the mapping side.** `crates/vsd-binary` carries 361
inline unit tests (79 of them in `src/container/tests.rs`) and 37
integration tests outside the line table (`tests/import_smoke.rs` 4,
`tests/malformed.rs` 9, `tests/twins.rs` 10, `tests/wide_corpus.rs`
13, `tests/oracle_smoke.rs` 1), plus P8-08's 50-test parity harness
under `tests/parity/`. The committed fixtures (`corpus/fixtures/`,
MIT) drive the malformed cases with no corpus dependency, and
P8-10's truncation and byte-flip sweeps push hostile bytes through
`import`. Every module maps its records and reports its warnings
under test; that is why the line table is what it is.

**Where the coverage overstates the strength** is branch depth and
behaviour:

1. The sweeps truncate at record and framing boundaries, where the
   record-level checks fire first. The per-field `?` arms of the
   payload readers — the path a record with a *plausible* length and
   a *short* payload takes — are dark in `shape.rs`, `text.rs`,
   `chunks.rs`, `container.rs`, `style.rs` and `pages.rs` (findings
   1-2, 6-7).
2. The behavioural oracles are all outside the gate: the libvisio
   parity run and `oracle_smoke` are `#[ignore]`d (they need
   `LARRYGRAM_LIBVISIO_ORACLE`), `twins` needs `LARRYGRAM_VSD_TWINS`
   and the twin files, `wide_corpus` needs `LARRYGRAM_WIDE_CORPUS`.
   The gate proves "no crash and the lines ran", never "the picture
   is right" — that is what P8-15's parity table (75.3%) measures,
   and it runs by hand only (finding 8).
3. In the open path the success flow and the version refusal are
   tested end to end (`file.rs`'s fixture and corpus tests, the CLI
   tests, the ui open flow); the failure arms of the same flow are
   half dark (finding 3).

## Findings

Severity follows the role file: blocker (below threshold or
ratchet), major (a critical path no test reaches, or a contract no
test asserts), minor (defensive, line-table, or a narrow gap).
Line numbers are from `3768d10ec7`; "arm" means a branch inside a
covered line (`?` early return, `else`, a match arm).

### Blocker

None. Every crate is at or above its threshold and (within the 0.05
tolerance for `larrygram-vsd-binary`) its ratchet.

### Major

1. **The geometry curve readers' malformed arms are untested.**
   `crates/vsd-binary/src/shape.rs` 519-549 (the inline NURBS/polyline
   formula blocks), 1342-1343 (the sub-header length) and 1653-1785
   (`nurbs_row_text`, `polyline_row_text`, `inline_nurbs`,
   `inline_polyline`, `read_plain`'s siblings: every `fields.u8()?`,
   `fields.f64()?`, `typed_number(...)?`) have their early-return arms
   dark. A hostile file whose record declares a length the framing
   accepts and whose payload ends mid-field lands exactly here. This
   is also the code behind P8-15 open issue 2 (the duplicated
   vertices and the diagonal fill closing edge), so it is under
   change right now. **Needs a test:** a payload-level truncation
   sweep over the curve-data records (truncate at every offset of one
   record, assert warn-or-error and the surviving cells), which the
   module-level `BadValue` contract already promises.
2. **The text row readers' malformed arms are untested.** The same
   class in `crates/vsd-binary/src/text.rs`: the cursor readers
   379-428, the `CharIX` trailing per-cell block 623-642, the `ParaIX`
   block 719-751, the tab rows 850-864, the field blocks 903-943 and
   the name-list readers 1881-1940 all keep their `?` arms dark.
   P8-05's Result claims "truncated or nonsensical records warn
   (`BadValue`) and are skipped" and the record-level truncations do;
   the in-record ones do not. **Needs a test:** the same sweep over
   `CharIX`, `ParaIX`, `Tabs` and field-block records.
3. **The open path's error arms are half dark.** In
   `crates/commands/src/builtins/file.rs`: `read_input_document`'s
   `?` at 444 (a picked or wasm-supplied file that fails to import —
   the bytes seam P8-09 routes by extension), `to_model`'s
   `read_error` at 288 (a corrupt `.vsdx` package), `encoded`'s
   `io_error` at 538 and `write_file`'s `?` at 770 (the write
   fails). File I/O is on the role's critical list, and the "never
   crash" contract says these become one status line, which no test
   observes. **Needs a test:** one per arm — a corrupt fixture
   through the picker fake, a corrupt package, and a save into an
   unwritable target. (The `read_only_save_error` refusal itself is
   covered.)
4. **`WarningKind`'s and `DocumentKind`'s catch-alls can silently
   rot.** `file.rs` 265 (`other => format!("{other:?}")` in
   `import_warning_line`), 877 (`_ => "unknown"` in `kind_text`),
   917 and 949 are **unreachable today**: the foreign enums are
   `#[non_exhaustive]` (`crates/model/src/document.rs:13`,
   `crates/vsd-binary/src/warning.rs:16`), so the wildcard is
   required and every current variant is named. When
   `larrygram-vsd-binary` gains a `WarningKind` (P8-09's own open
   issue) the debug-print spelling becomes reachable with no failing
   test. **Recommendation:** the `Display` impl in
   `crates/vsd-binary` deletes the mapping (a test then pins the
   spelling); until then, unreachable-but-required, keep.

### Minor

1. **The stale-reference guards in `shape.rs` are the crate's only
   dark lines.** 1077 and 1117 (`continue` when the arena has no such
   index, in `inherit_geometry` and `master_shape_with_geometry`),
   plus the arms at 1102 (`get_mut` miss) and 1113 (`document.master`
   miss, the `?`). The indexes come from the arena itself, so the
   guards are defensive; P8-15's job 67 tests cover the guard's
   caller, not these arms. **Needs a test** (one hand-built document
   with a dangling index each) or a comment that they are belt and
   braces; either keeps the 100.0 ratchet discussion honest.
2. **Date-field lookups can fail on hostile input and are dark.**
   `text.rs` 1194-1195 (`MONTHS.get(...)?`, `WEEKDAYS.get(...)?`) and
   1297 (`date_display(...)?`): a field carrying month 13 or weekday 7
   reaches these arms. **Needs one test** with an out-of-range date
   field (assert the placeholder text and the warning).
3. **Defensive checked arithmetic behind length pre-checks.** The
   `checked_add`/`get`/`try_into` arms of the scalar readers:
   `chunks.rs` 719-745, 1047, 1094, 1779-1785 (the list-record child
   ids); `container.rs` 559 (`DirEntry::parse`), 605 (`sector_chain`),
   713-739 (sector offsets); `style.rs` 196-214, 1079; `pages.rs`
   944-960; `pipeline.rs` 59-63 (the pipeline's `?` chain). The
   callers check lengths first, so most of these arms are unreachable
   for input that reaches them; where the length check is the arm's
   only guard (the pointer reads in `chunks.rs`, the list-record
   reader) a truncated-pointer-list test already exists at record
   level. **Recommendation:** verify per arm and mark unreachable
   ones with a comment; do not chase the last branch with tests.
4. **The CLI export and render commands' error arms.**
   `file.rs` 723-770 (`page_arg`, `build`, `encode`, `write_file`,
   `need_path`, `format_arg`, `scale_arg`, `load`), 663 and 694. The
   same failures are tested at the CLI layer
   (`apps/larrygram-cli/src/cli/exec.rs`'s `FailingWriter` tests,
   622 and 643's panic arms aside), so this is a narrow dispatch-layer
   gap. **One test per arm** with a bad argument, or accept.
5. **Test-only dark arms.** `file.rs` 1126 (the `if let` else in the
   no-target save test), 1231-1237 and 1464, 1482 (the corpus-skip
   arms of `corpus_vsd` and the two corpus tests), 1454 (a test's
   `panic!` else); `chunks.rs` 2397, 2473-2485 (the corpus smoke
   test's skip arms); `pipeline.rs` 88, 94 and `style.rs` 1549
   (assertion else-arms); `cli.rs` 254/270/281/299 and `parse.rs`
   330/337/344/351/358 (test-helper `panic!` arms),
   `exec.rs` 606-608 (`FailingWriter::flush`), 700-706, 746
   (corpus-skip). Line-table noise: they run only when the corpus is
   absent or an assertion fails. Delete or ignore.
6. **`parse.rs` 312** (`Arg::Value(value)` in `unknown_option`): a
   bare positional where an option is expected is untested. One
   command line closes it.
7. **The native picker wiring stays dark, now with the Phase 8
   filters.** `crates/ui/src/dialogs.rs` 130-143
   (`NativeFilePicker::open`/`save`) and 175-189 (`open_dialog`)
   include P8-09's `add_filter("Visio files", ...)` lines (139-140,
   177-178), so the claim "the open dialog accepts all six formats"
   rests on the `OPEN_EXTENSIONS` list test, not on the dialog that
   reaches rfd; lines 347-356 are the dead `Sync` test helper. This
   is P7-12 major 2 and its minor, unchanged: the OS dialog cannot be
   driven headlessly, but `open_dialog` is pure and `rfd::FileDialog`
   construction is not. **Recommendation:** record the request
   through a seam (as the pickers' fakes do) or keep as is with the
   comment the P7-12 finding asks for.
8. **The behavioural oracles never run in the gate.** `tests/parity/`
   (50 tests, `#[ignore]`d, needs `LARRYGRAM_LIBVISIO_ORACLE`),
   `tests/oracle_smoke.rs`, `tests/twins.rs` (needs
   `LARRYGRAM_VSD_TWINS` and the twin files), `tests/wide_corpus.rs`
   (needs `LARRYGRAM_WIDE_CORPUS`). Their code is outside the line
   table, so nothing in the coverage gate notices when a mapping fix
   silently drops parity. The 75.3% of P8-15 is the real fidelity
   number and it is hand-run. **Recommendation (process, not a
   test):** run the parity harness in one gate job per phase round
   (it is a test-step flag, not a code change), or add a small
   committed golden that pins a handful of parity facts without the
   oracle.
9. **`corpus_vsd`'s render goldens are invisible here.**
   `crates/render/tests/corpus_vsd.rs` (6 tests) and the 15 PNGs live
   outside the line table and outside this run's assertions; their
   review is the manual one P8-15 records. Nothing to fix in
   coverage; listed so the number is not read as more than it is.
10. **Carried, unchanged by Phase 8** (from `docs/reviews/phase-7-coverage.md`,
    for the file-I/O and dispatch items the role names): the app
    shells' `eframe::App` wiring and error lines
    (`apps/larrygram/src/main.rs` 136-343,
    `apps/larrygram-web/src/main.rs` 96-233: `update_ui`'s and
    `persist`'s `note_error` arms, `main`, the window-icon raster
    errors, the dead `MemoryStorage` methods), and `shell.rs`'s
    `note_error` arms and interactive gaps (P7-12 findings 1, 3, 6,
    7). `xtask`'s dead `coverage.toml` key (P7-12 minor 9).

## Ratchet proposal for the master (numbers only)

`cargo xtask coverage --update-ratchet` floors to one decimal place
(`(p * 10.0).floor() / 10.0`), so the proposal is the floor of the
measured fraction, keeping a ratchet from ever dropping (AGENTS.md
section 7). This audit does not edit `coverage.toml`:

| Crate | Current | Proposed | Measured |
|---|---:|---:|---:|
| larrygram | 80.0 | **85.3** | 338/396 |
| larrygram-cli | 96.7 | **97.4** | 1308/1342 |
| larrygram-commands | 98.4 | 98.4 | 10829/11005 |
| larrygram-geom | 100.0 | 100.0 | 2501/2501 |
| larrygram-model | 99.7 | 99.7 | 10862/10893 |
| larrygram-render | 98.5 | 98.5 | 7129/7237 |
| larrygram-shapesheet | 96.9 | 96.9 | 6541/6744 |
| larrygram-text | 98.8 | **99.2** | 1754/1767 |
| larrygram-ui | 97.0 | **97.5** | 18476/18935 |
| larrygram-ui-spec | 99.4 | **99.6** | 3829/3844 |
| larrygram-vsd-binary | 100.0 | **99.9** (see note) | 10716/10720 |
| larrygram-vsdx | 98.2 | 98.2 | 7656/7790 |
| larrygram-web | 77.8 | **78.4** | 113/144 |
| larrygram-xtask | 93.4 (dead key) | **93.5** (`larrygram-xtask`) | 11831/12645 |

Notes for the update pass (the file is master-owned):

- **`larrygram-vsd-binary` is the decision point.** The strict rule of
  the role file keeps 100.0, which now refuses to accept the loss of
  any one of the 10 720 lines and passes today only through the
  coverage step's 0.05 tolerance (99.9627). The floor rule writes
  99.9, one notch below what is measured. Either is defensible; with
  P8-17 still to change this crate, 99.9 is the number that will not
  turn the next gate red on float noise alone. The master decides.
- `larrygram-shapesheet` floors to 96.9 (the table rounds 96.9899 to
  97.0), `larrygram-ui` to 97.5 (97.5759), `larrygram-text` to 99.2
  (99.2643), `larrygram-cli` to 97.4 (97.4665), `larrygram` to 85.3
  (85.3535), `larrygram-web` to 78.4 (78.4722).
- `larrygram-commands` (98.4007) and `larrygram-render` (98.5077)
  floor onto their current values; their margins are 0.0007 and
  0.0077, so both pass the next gate only as long as no line is lost.
- Fix the `xtask` key at the same time (P7-12 minor 9).

## Appendix: unentered branches by file

llvm-cov regions no run enters, at `3768d10ec7`. A `?` mark is the
early-return arm of a `?` or `ok()?` on that line; `}` marks the end
of a never-entered `else` or match block. Full extraction:
`target/llvm-cov-export.json` in the worktree.

- `crates/vsd-binary/src/shape.rs` (83): 387, 388, 394, 400, 406,
  416 (`Fields` scalar readers `?`); 519, 520, 523, 524, 528, 529
  (inline polyline block `?`); 539-549 (inline NURBS block `?`);
  812, 828, 833 `}`; 964 `?` (negative `visio_id` guard); 1077,
  1102, 1113, 1117 (finding 5); 1168, 1174, 1237, 1254 `}`;
  1342, 1343 `?`; 1653-1661, 1667-1675 (`nurbs_row_text`,
  `polyline_row_text` `?`); 1682-1704, 1715-1733 (`inline_nurbs`
  `?`); 1757-1781 (`inline_polyline` `?`).
- `crates/vsd-binary/src/text.rs` (63): 212, 217 `}`; 297 `?`
  (model error); 379, 391, 408, 409, 422, 428 (cursor `?`);
  623-642 (`CharIX` block `?`); 719-751 (`ParaIX` block `?`);
  850-864 (tabs `?`); 903-943 (field block `?`); 1194, 1195, 1297
  (finding 2 minor); 1541 `}`; 1881-1940 (name lists `?`); 2066
  `}`; 2118 `?` (the version-6 scan start).
- `crates/vsd-binary/src/chunks.rs` (26): 719-745 (pointer and
  scalar reads `?`); 823, 841, 1085 `}`; 1047, 1094 `?`; 1779-1785
  (list-record child ids `?`); 2397, 2473-2485 (test arms).
- `crates/vsd-binary/src/style.rs` (30): 160 `?`; 196-214 (scalar
  readers `?`); 563, 640, 652, 676, 745, 757, 773, 822, 844, 855,
  861, 872, 883, 886, 901, 930, 954, 966, 977, 1070 `}`; 1079 `?`;
  1549 (test arm).
- `crates/vsd-binary/src/container.rs` (7): 559, 605, 713, 714,
  723, 731, 739 `?`.
- `crates/vsd-binary/src/pages.rs` (5): 623, 684 `}`; 944, 950,
  960 `?`.
- `crates/vsd-binary/src/pipeline.rs` (5): 59, 62, 63 `?`; 88, 94
  (test arms).
- `crates/vsd-binary/src/scale.rs` (3): 110, 248 `?`; 197 `}`.
- `crates/vsd-binary/src/{error,lib,limits,warning}.rs`: none.
- `crates/commands/src/builtins/file.rs` (33): 107, 288, 322, 399,
  444, 538, 663, 694, 723-770, 862, 877, 899, 917, 949, 265
  (findings 3, 4, minor 4); 1119-1482 (test arms, minor 5).

The `larrygram-ui` and `apps/*` lists are unchanged from
`docs/reviews/phase-7-coverage.md` (ui: 134 dark lines in `shell.rs`
plus the dialogs and tool gaps audited there) except for the P8-09
dialog lines named in minor 7.
