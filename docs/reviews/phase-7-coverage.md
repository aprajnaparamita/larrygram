# Phase 7 coverage audit

- Task: P7-12 (coverage-auditor), branch `agent/P7-12`, 2026-10-25.
- Scope: `larrygram-ui-spec` and `larrygram-ui` after Phase 7 (P7-02
  through P7-10): the layout format v2 loader and adapter, the Panel
  Classic and Panel Modern widgets, the frame, the File view, the
  wizard, the About window, the theme, icons, lists and settings. The
  per-crate table covers the whole workspace, for the ratchet
  proposal.
- The question of this audit: is the new code exercised by meaningful
  tests, not just by lines run? Answer: mostly yes for the data and
  widget layers, with the ten major and ten minor gaps below; the
  largest weakness is not missing lines but assertions weaker than
  the contracts they stand for (findings 5, 9, 10).

## Measurement

AGENTS.md section 7 puts every workspace-wide run behind the gate
queue, so the measurement was a coverage-only gate job (the queue's
`cargo xtask gate` with every step but `coverage` skipped):

```sh
G=/Users/dara/larrygram/tools/gate-queue/gateq
$G submit P7-12 --skip codegen,names,parity,fmt,clippy,test,doc,deny,layers
```

Job 77 at `d9c77ec0f5` (current `main`), **GREEN**, 13m10s (coverage
step 709.5s); full log
`/Volumes/Dara/larrygram/gates/logs/P7-12-d9c77ec0f5-20261025-153114.log`.
The coverage step is `cargo llvm-cov --workspace --all-features
--json --summary-only --output-path target/llvm-cov-summary.json`.
Per-file and per-line detail came from the same profile data in the
worktree (no rebuild, no second test run):

```sh
cargo llvm-cov report --lcov --output-path target/lcov.info
```

Zero-hit lines are exact; llvm-cov folds partially covered lines into
the covered count, so a file's misses understate its dead arms. Inline
`#[cfg(test)]` code counts as instrumented lines, as in Phases 4-6.
The `wasm32` arms of `crates/ui/src/dialogs.rs` (193-380) are
`cfg`-ed out on this run; the wasm target is the master's full check.

Cross-check: the two scoped crates reproduce the P7-10 hand-off
gate's coverage step exactly (`larrygram-ui` 18 476/18 935,
`larrygram-ui-spec` 3 829/3 844).

## Per-crate results

| Crate | Lines | Covered | Coverage | Threshold | Ratchet | Delta |
|---|---:|---:|---:|---:|---:|---:|
| larrygram-geom | 2501 | 2501 | 100.0% | 85 | 100.0 | ±0.0 |
| larrygram-vsd-binary | 9946 | 9945 | 100.0% | 85 | 100.0 | ±0.0 |
| larrygram-model | 10836 | 10805 | 99.7% | 85 | 99.7 | ±0.0 |
| larrygram-ui-spec | 3844 | 3829 | 99.6% | 85 | 99.4 | +0.2 |
| larrygram-text | 1767 | 1754 | 99.3% | 75 | 98.8 | +0.5 |
| larrygram-commands | 11005 | 10829 | 98.4% | 85 | 98.4 | ±0.0 |
| larrygram-render | 7221 | 7113 | 98.5% | 75 | 98.5 | ±0.0 |
| larrygram-vsdx | 7751 | 7612 | 98.2% | 85 | 98.2 | ±0.0 |
| larrygram-ui | 18935 | 18476 | 97.6% | 50 | 97.0 | +0.6 |
| larrygram-cli | 1342 | 1308 | 97.5% | 50 | 96.7 | +0.8 |
| larrygram-shapesheet | 6744 | 6541 | 97.0% | 85 | 96.9 | +0.1 |
| larrygram-xtask | 12645 | 11831 | 93.6% | — | — | see below |
| larrygram-web | 144 | 113 | 78.5% | — | 77.8 | +0.7 |
| larrygram | 396 | 338 | 85.4% | 50 | 80.0 | +5.4 |
| larrygram-control | 0 | 0 | n/a | 50 | — | placeholder |

Ratchet column: `[ratchet]` in `coverage.toml` as of `d9c77ec0f5`.
Deltas are the measured percentage minus the ratchet, one decimal
place.

### Statement per crate

- **Blockers (below threshold or below ratchet): none.** Every crate
  with instrumented lines is at or above its threshold and its
  ratchet.
- **Zero-margin crates:** commands (98.4007 against a 98.4 ratchet),
  render (98.5043 against 98.5), vsdx (98.2067 against 98.2). The
  gate is green partly because `cargo xtask coverage` tolerates 0.05
  for float noise; one or two lost lines in any of these three fail
  the next phase. The ratchet proposal below floors to what was
  measured, so the margin stays thin by construction.
- The two audited crates are among the workspace's best-covered
  large crates and both moved up since Phase 6 (from
  `docs/reviews/phase-6-coverage.md`): `larrygram-ui-spec` 99.4 to
  99.6 on 2 043 to 3 844 lines (the v2 loader and adapter are new
  code at 100%), `larrygram-ui` 97.0 to 97.6 on 11 671 to 18 935
  lines (the two panel widget sets are new code at 97-100% per file).
  Adding nearly 7 000 lines at that level is the phase's real
  achievement.
- **`larrygram-xtask` has no working ratchet** (see minor 9).
- `larrygram-control` stays an empty placeholder.

### Per-file detail of the audited crates

Misses out of instrumented lines, the files that moved this phase
first. Files at 0 misses include `ui-spec/src/v2/{mod,load,adapter}.rs`
(261/261, 697/698, 751/751), `ui/src/theme.rs` (477/477),
`ui/src/panel_modern/place.rs` (352/352),
`ui/src/panel_modern/metrics.rs` (152/152),
`ui/src/panel_classic/menu.rs` (247/247) and
`ui/src/about.rs` (58/58).

| File | Misses | Lines |
|---|---:|---:|
| `crates/ui/src/shell.rs` | 120 | 2532 |
| `crates/ui/src/dialogs.rs` | 38 | 146 |
| `crates/ui/src/shapes_window.rs` | 31 | 271 |
| `crates/ui/src/canvas.rs` | 28 | 1254 |
| `crates/ui/src/tools/pointer.rs` | 26 | 955 |
| `crates/ui/src/tab_bar.rs` | 22 | 1571 |
| `crates/ui/src/panes.rs` | 20 | 1623 |
| `crates/ui/src/tools/stencil.rs` | 16 | 480 |
| `crates/ui/src/tools/connector.rs` | 14 | 399 |
| `crates/ui/src/settings.rs` | 12 | 384 |
| `crates/ui-spec/src/loader.rs` | 11 | 885 (all 11 in test code) |
| `crates/ui/src/tools/draw.rs` | 11 | 369 |
| `crates/ui/src/contexts.rs` | 8 | 326 |
| `crates/ui/src/snap.rs` | 8 | 442 |
| `crates/ui/src/text_edit.rs` | 7 | 454 |
| `crates/ui/src/panel_classic/mod.rs` | 6 | 788 |
| everything else in the two crates | ≤4 each | |

## Is the new code meaningfully tested?

**Yes on the data side.** `crates/ui-spec`'s new code (the v2 loader,
its cross-checks, the v1 adapter) is at 100% or one test-panic away
from it, and the tests assert behaviour, not just completion: the
malformed and cross-reference fixtures drive hostile documents through
`tests/malformed.rs` and `tests/loader.rs`, `tests/v2_layouts.rs`
checks the four committed layouts against the schema and each other,
`tests/v2_snapshot.rs` pins every surface and kind count with `insta`,
`tests/theme_tokens.rs` scans `crates/ui/src` for the tokens the
widgets read and checks both committed themes, and
`tests/icon_counts.rs` pins the icon counts and resolves every curated
key. The `loader` and `layout_v2` fuzz targets cover the same surface
outside the coverage numbers.

**Mostly yes on the widget side.** The ADR 0065 sweeps
(`crates/ui/tests/phase7/{menus,modern,placement,surfaces}.rs`) drive
the real widgets headlessly and assert exact command ids and sources,
disabled states with their reason tooltips, submenu structure, the
View > Toolbars toggles and the contextual tab sets. The unit tests in
`crates/ui/src/panel_classic/tests.rs` and
`crates/ui/src/panel_modern/tests.rs` pin the rest.

**Where the coverage overstates the strength** are the findings
below: interactive paths no test enters (1, 2, 6, 7), error handling
no test provokes (3), a covered path whose wrong behaviour no test
sees (4), and three oracles weaker than the contracts they stand for
(5, 9, 10). The sweep oracle gap is the one to keep in mind when
reading the 97.6%: it bounds what the sweeps can ever fail on.

## Findings

Severity follows the role file: blocker (below threshold or
ratchet), major (a critical path no test reaches, or a contract no
test asserts), minor (defensive, line-table, or a narrow gap).
Line numbers are from `d9c77ec0f5`.

### Blocker

None. Every crate is at or above its threshold and ratchet.

### Major

1. **The Panel Modern title-bar Help menu never opens in a test.**
   `crates/ui/src/shell.rs:609-616` draws the `Help` menu button with
   `help.welcome` and `help.about`, but no test opens it:
   `crates/ui/tests/phase7/first_run.rs:112-121` dispatches the two
   command ids directly through `rig.dispatch`. The commands'
   outcomes are covered; the interactive path a user actually takes
   (open the menu, click the entry, the intent reaches the registry)
   is dark. One kittest click through the menu closes it.
2. **The real file picker never runs.** `crates/ui/src/dialogs.rs`
   `NativeFilePicker`: `open`/`save` (130-143) and the `open_dialog`
   builder (175-189) have zero hits; only the stub seam (347-356) is
   tested. File I/O is on the role's critical list, and this is the
   only place the open/save dialogs pick a path and write bytes.
   The helper functions behind it are covered (`picked_path` and
   `save_picked`, 148-173, through their closure seams), so the dark
   part is the real dialog wiring: request title, the "Visio files"
   filter, the suggested directory and file name. The OS dialog
   cannot be driven headlessly, but `open_dialog` is pure and
   deserves one test. (The wasm arms are `cfg`-ed out on this run.)
3. **No test provokes a shell error.** The `note_error` arms of
   `crates/ui/src/shell.rs` are all dark: 206-207, 414, 440-441,
   565-570, 665, 799-801, 951, 989-1031, 1188-1189, 1278, 1330, 1378,
   1403, 1427, 1484-1486, 1694-1695 (about 30 lines of error
   handling). A widget error or a failing command is supposed to
   surface as a status-line warning and keep the frame alive (the
   "never crash" contract of AGENTS.md section 6); nothing tests what
   the user then sees. Even the `Err(UiError::NotImplemented)` match
   arms at 1007 and 1188 are dark.
4. **The toolbar popups' nested submenus are wrong and untested.**
   `crates/ui/src/panel_classic/toolbar.rs:552-603`
   (`draw_popup_level`) opens child levels on a stateless hover, so
   moving the pointer into a child closes it (P7-10 open issue 4).
   The sweep passes only because the committed toolbars have no
   enabled nested rows: the code runs over shallow rows and its
   broken hover behaviour is never entered. This is the audit's one
   case of a covered path that is nonetheless untested in behaviour;
   a fixture toolbar with a nested row would have caught the bug the
   context-menu fix addressed.
5. **The Panel Modern dispatch arguments are not asserted.** The
   sweep drops them: `crates/ui/tests/phase7/modern.rs:438, 583, 792`
   match `Expect::Dispatch { command, .. }` with `value: _`, and
   `crates/ui/src/panel_modern/*` never sets a `value` argument (only
   `crates/ui/src/panel_classic/rows.rs:151, 341` does). The known
   defect (P7-10 open issue 3: a combo's choice reaches the command
   as the item's own id with empty args) therefore passes every
   sweep. The `Invocation` args half of the dispatch contract is
   unasserted on the modern side, and the fix will land without a
   failing test unless one asserts it.
6. **The page-tabs "All" menu is drawn but never opened.**
   `crates/ui/src/shell.rs:1853-1860` (the menu body, its page
   buttons) is dark; `crates/ui/tests/phase7/surfaces.rs:89` asserts
   only that the button draws. Page switching from that menu is
   untested, while the plain tab click is covered.
7. **The 2016/365 File view (the v1 backstage) is untested.**
   `crates/ui/src/shell.rs:720-722` (a backstage command leaves the
   File view, ADR 0031) and `crates/ui/src/tab_bar.rs:451, 458` (its
   page rows) are dark. P7-09 kept the v1 widgets for these two
   layouts; their File view is the largest interactive surface the
   phase shipped without a test.
8. **Six selection contexts can never occur, so their rows are
   unreachable.** `crates/ui/src/contexts.rs:285-298` names
   `PageTabNavigation`, `EmbeddedObject`, `SizePositionWindow`,
   `FullScreen`, `MasterEdit` and `StencilEdit`, but the shell
   produces none of them (P7-08 open issues: no window-mode state;
   `shape_selected` and `connector_selected` are still `status:
   pending` in `contexts.yaml`). Every menu and control row gated on
   those contexts is dead in the app and cannot be tested end to end
   until the state lands. One test each when it does.
9. **The sweep oracle mirrors the widgets' own rules.**
   `crates/ui/tests/phase7/expect.rs:1-4` documents it: the
   expectations are "computed from the committed data and the
   builtin registry, mirroring the two widget resolvers
   (`panel_classic::rows` and `panel_modern::common`)". A wrong rule
   implemented in both places passes every sweep. The independent
   pins are the count snapshots, the icon pins, the theme-token
   source scan, the `contexts.yaml` cross-check and the goldens;
   nothing pins the *labels* or the *submenu shapes* of the committed
   data against an external source. For the next phase, one small
   hand-written "known menu" fixture (the ten File/Edit/View menus
   with their real rows) asserted literally would break the circle.
10. **The rendered placement geometry is only half asserted.**
    `crates/ui/tests/phase7/placement.rs:48, 62, 171` zip drawn
    widgets against expected controls (presence and order); the
    rectangles come from the placement model, because the accesskit
    node rect cannot pin positions (P7-10 open issue 6). The visible
    symptom is exactly the bug the goldens now bake in (P7-10 open
    issue 1: `measure_widths` under-measures and command-bar labels
    wrap vertically): a rendered-geometry defect passed the ADR 0065
    placement item. When P7-14 fixes the wrap, the goldens must be
    regenerated and a width assertion added, or the hole stays.

### Minor

1. **The About "Watch Video" arm.** `crates/ui/src/shell.rs:1421-1423`
   (`ui.ctx().open_url`) is dark; `crates/ui/tests/about/main.rs`
   asserts the `about::Outcome::WatchVideo` value only. kittest
   cannot open a URL. Keep the arm and say so in a comment, or route
   the URL open through a seam the test rig can record.
2. **The Options dialog's layout and units rows.**
   `crates/ui/src/settings.rs:222` (the `LayoutKind::PanelModern` arm
   of `option_changes`) is dark, and its comment ("the dialog never
   offers them before then") is stale: P7-09 landed the registry
   rows. The units dropdown (`settings.rs:187-189`) never opens in a
   test; `view.units` with its `units` argument is asserted only at
   the `option_changes` level (`settings.rs:413-420`).
3. **A failed master preview.** `crates/ui/src/shapes_window.rs:215`
   (`Preview::Failed`) and 224-232 (the placeholder tile) are dark:
   a preview that fails to raster has no test.
4. **The context-menu fix's edge arms.**
   `crates/ui/src/panel_classic/mod.rs:443` (close on a row with no
   action), 477 (chain truncation on a deeper click) and 483-484
   (hover reset) are dark; the P7-10 regression tests cover the open
   and dispatch chain, not these leaves.
5. **The wizard's layout fallback.** `crates/ui/src/wizard.rs:233`
   (`layout = LayoutKind::PanelModern`) when the stored layout has no
   radio row. Defensive; one test with a hand-built settings value.
6. **Dead test helpers.** `crates/ui/src/shell.rs:2262-2269` (a
   `noop` edit's `apply`/`revert`) and 2999-3005 (a stub command's
   `execute`) never run. Line-table noise; delete or use.
7. **The goldens pin known defects and three are not MIT.** The seven
   PNGs in `crates/ui/tests/phase7/golden/` bake in the wrapped
   labels (P7-10 open issue 1), and `about.png`, `wizard-welcome.png`
   and `wizard-layout.png` reproduce the brand art (their
   `ATTRIBUTION.md` row records it). Regenerate after the P7-14 fix;
   a fork must replace the three brand renders.
8. **The floating toolbars' shipped positions are never tested.**
   The toolbar sweep hides them (`crates/ui/tests/phase7/menus.rs:624`
   `show_all_docked`, with its comment: the tool windows "land on top
   of the docked rows and eat the clicks"), so the default layout
   arrangement a user first meets has no test and no golden. The fix
   belongs to P7-14; one render with the defaults would pin it.
9. **`coverage.toml`'s xtask ratchet is dead.** `coverage.toml:39`
   (`xtask = 93.4`) names no package; the aggregate key is
   `larrygram-xtask`, so `cargo xtask coverage` never checks that
   ratchet (its table row shows `-`). Rename the key to
   `larrygram-xtask` and move it to the measured value (below).
   `larrygram-control` has no ratchet row either (empty placeholder,
   fine). The file is master-owned; listed here for the update pass.
10. **Carried from earlier phases, unchanged by Phase 7** (the tool
   interaction edges the role file asks about): the drag and abort
   arms of `crates/ui/src/tools/{pointer,draw,stencil,connector}.rs`
   (e.g. `pointer.rs:205-245`, `stencil.rs:83-130`,
   `connector.rs:132-168`), `crates/ui/src/canvas.rs:386-391` (pan
   drag) and 1003-1008 (wheel-unit arms), `crates/ui/src/snap.rs`'s
   no-snap fallbacks (83, 100, 204, 218), `crates/ui/src/panes.rs:703`
   (a value that fails to parse shows "missing") and
   `crates/ui/src/text_edit.rs:183`. Each is one test away; none is
   new this phase.

## Ratchet proposal for the master (numbers only)

`cargo xtask coverage --update-ratchet` floors to one decimal place
(`(p * 10.0).floor() / 10.0`), so the proposal is the floor of the
measured fraction, keeping a ratchet from ever dropping (AGENTS.md
section 7):

| Crate | Current | Proposed | Measured |
|---|---:|---:|---:|
| larrygram | 80.0 | **85.3** | 338/396 |
| larrygram-cli | 96.7 | **97.4** | 1308/1342 |
| larrygram-commands | 98.4 | 98.4 | 10829/11005 |
| larrygram-geom | 100.0 | 100.0 | 2501/2501 |
| larrygram-model | 99.7 | 99.7 | 10805/10836 |
| larrygram-render | 98.5 | 98.5 | 7113/7221 |
| larrygram-shapesheet | 96.9 | 96.9 | 6541/6744 |
| larrygram-text | 98.8 | **99.2** | 1754/1767 |
| larrygram-ui | 97.0 | **97.5** | 18476/18935 |
| larrygram-ui-spec | 99.4 | **99.6** | 3829/3844 |
| larrygram-vsd-binary | 100.0 | 100.0 | 9945/9946 |
| larrygram-vsdx | 98.2 | 98.2 | 7612/7751 |
| larrygram-web | 77.8 | **78.4** | 113/144 |
| larrygram-xtask | 93.4 (dead key) | **93.5** (`larrygram-xtask`) | 11831/12645 |

Notes for the update pass (the file is master-owned):

- `larrygram-shapesheet` floors to 96.9, its current value: the table
  rounds 96.9899 to 97.0 but the update command would write 96.9.
  Same story `larrygram-ui` (97.5759 floors to 97.5, not the 97.6 in
  the table) and `larrygram-text` (99.2643 floors to 99.2).
- `larrygram-vsd-binary` sits at 100.0 but measured 99.9899 (one
  line); keeping 100.0 refuses to accept that line's loss. That is
  the strict rule of the role file; the alternative is the 99.9
  floor. The master decides, with P8-14 in mind.
- Fix the `xtask` key at the same time (minor 9).

The two audited crates land at 97.5 and 99.6 proposed ratchets, both
above their Phase 6 numbers (96.8/99.3 recorded then); the measured
percentages (97.6/99.6) reproduce the P7-10 hand-off gate's coverage
step exactly.
