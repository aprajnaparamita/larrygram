# Phase 7 code review (P7-11)

Task P7-11, reviewer, 2026-10-25, branch `agent/P7-11`. Review of the
Phase 7 merges (`git diff phase-6..main` restricted to the Phase 7
scope; the interleaved Phase 8 `.vsd` work in `crates/vsd-binary` and
its docs is out of scope and audited by the P8 tasks) against AGENTS.md
section 6, the layering (ADR 0002) and ADRs 0061-0065 plus the ADR 0066
wording and the guides in `docs/ui/`. The reviewer role is read-only:
this task changed no code.

Scope covered: `crates/ui-spec` (`v2/mod.rs`, `v2/load.rs`,
`v2/adapter.rs`, `error.rs`, `loader.rs` cross-checks, the `ui-spec/`
data files and `layout-v2.schema.json`), `crates/ui` (`shell.rs` Phase 7
wiring, `panel_classic/*`, `panel_modern/*`, `file_view.rs`,
`contexts.rs`, `theme.rs`, `icons/mod.rs`, `lists.rs`, `settings.rs`,
`wizard.rs`, `about.rs`), `crates/commands` (`Settings`, the
`view.ui_layout.*` commands), `apps/larrygram` (`--ui-layout`), the
rename boundary P7-00 where it touches these crates, the ADRs
0061-0065 and the guides `docs/ui/*.md`. Checks that passed are listed
first; findings follow, ranked.

## Verdict

**Approve with fixes: 0 blocker, 2 major, 4 minor.** The phase's hard
rules hold: layout data loading is staged, bounded and panic-free (a
64-level document bound, iterative depth measuring and item walking, a
cycle- and depth-safe v1 adapter, decode errors for values wider than
the typed model), the v2 model matches ADR 0061 and the schema, the
icon pipeline is bounded (512 px cap, 1024-entry cache, one warning per
key) and falls back to our own placeholder mark, the contexts enum and
its `id()` mapping match ADR 0064, and no crate below L6 touches egui.
The two majors are both theme-contract gaps: most theme tokens the ADRs
and guides name are declared and documented but never read by the
widgets (so tuning the YAML does nothing for those surfaces), and the
ADR 0062 "disabled controls draw the icon at 40% alpha" rule is
implemented in `icons` but wired to nothing.

## Checks that passed

- **Loader hardening (`crates/ui-spec/src/v2/load.rs`).** The staged
  load parses, bounds the document nesting (`bounded_value`,
  `MAX_VALUE_DEPTH = 64`), validates against
  `ui-spec/layout-v2.schema.json`, decodes and cross-checks
  (`load.rs:66-133`). `value_depth` is iterative and the item walk is
  an explicit stack (`load.rs:230-237`), so neither can overflow the
  stack on hostile input. Schema-valid values the types cannot
  represent (a `column` of 4294967296) fail the decode rather than
  wrap, pinned by `decode_layout_rejects_values_the_types_cannot_represent`.
  Cross-checks cover commands (with the `new_command` proposal valve),
  icon keys, `shown_in` contexts, duplicate placement ids across all
  surfaces, `contexts.yaml` rows and the on-disk SVGs, each with a
  failure test.
- **The v1 adapter (`crates/ui-spec/src/v2/adapter.rs`).** A `parent`
  chain deeper than `MAX_TREE_DEPTH = 32` or in a cycle breaks at the
  control itself (`link_depths`, `adapter.rs:270-289`), so the built
  tree is always bounded and acyclic; `assemble` moves items
  deepest-first so an item carries its children. Placement ids are
  de-duplicated per scope. The output is snapshot-pinned (ADR 0061),
  including the known limits (File view pages, unmapped tab sets).
- **ADR 0061 surface.** `v2::LayoutKind` has the four variants with
  the serde aliases for the pre-P7-00 stored values
  (`v2/mod.rs:36-60`), `from_v1` returns `Result<LayoutV2, V2Error>`,
  `LayoutSets::get` answers for all four kinds (the P7-03 open issue is
  closed), `--ui-layout` accepts the four values
  (`apps/larrygram/src/main.rs:105-108`), the registry ids are
  `view.ui_layout.panel_*`, and the settings default is
  `LayoutKind::PanelModern` (`commands/src/app.rs:329-341`) with an old
  store still loading (the `Settings` compatibility tests).
- **ADR 0062 pipeline (`crates/ui/src/icons/mod.rs`).** SVG bytes come
  from the generated `table.rs`; `rasterize` replaces `currentColor`
  with the tint, rasterises once per (icon, pixels, tint, scale) into a
  cache capped at 1024 entries with a bounded texture side
  (`MAX_ICON_PIXELS = 512`, `physical_pixels` clamps), an unknown key
  or a rasterisation failure draws our own placeholder mark and logs
  once per key, and a non-finite size or pixels-per-point falls back.
  A missing icon file is caught at load time (`check_icon_files`).
- **ADR 0063 seam (`crates/ui/src/theme.rs`).** `theme::apply` never
  fails on theme data; `color`, `metric` and `metric_field` fall back
  with one warning per token, font sizes are clamped (1..256 px), and
  the mapped tokens follow the `docs/ui/themes.md` table. The canvas
  reads its six colour tokens (`canvas.rs:121-126`), the icon tints
  read theirs, and the Panel Modern metrics come from the theme
  (`panel_modern/metrics.rs:170-179`, with clamped row counts and
  non-finite guards).
- **ADR 0064 (`crates/ui/src/contexts.rs`).** The twelve-variant enum
  and its `id()` mapping (with `PageSelected` counting as
  `empty_page`), the precedence order (window mode, then text editing,
  then selection), the shape classification (connector, container,
  picture, group) and `menu_for` match the ADR and
  `docs/ui/contexts.md`. The two `pending` contexts are marked as such
  in the guide.
- **Command dispatch and stubs.** The widgets return `Intent`s and
  never execute commands (ADR 0028); stubs are disabled with the
  pinned "Not yet implemented" tooltip and out-of-scope rows with
  theirs (`panel_classic/mod.rs:42-47`), and the ADR 0065 sweeps
  (P7-10) pin the dispatched command of every enabled item.
- **Panics and layering.** No `unwrap`/`expect`/`panic!` outside test
  code in `ui-spec`, `ui` or `commands` (workspace clippy lints); the
  layout data paths use `get()`-style access throughout
  (`place.rs:215-225`, `adapter.rs`, `load.rs`). No crate below L6
  references egui/eframe/winit/rfd. `cargo test -p larrygram-ui-spec`
  is green on this branch.
- **Wizard and About (`wizard.rs`, `about.rs`, `shell.rs:1343-1401`).**
  `VIDEO_URL` is the bare `https://youtu.be/wpgiRvcGY_U` without the
  share-tracking id (pinned by test), Start writes the layout through
  `set_layout_kind` and marks the wizard seen, and the About wording is
  the brief's own text.

## Findings

| id | severity | file:line | problem | suggested fix |
|----|----------|-----------|---------|---------------|
| F1 | major | crates/ui/src/theme.rs:215-277; crates/ui/src/panel_classic/menu.rs:110-116, 215-222; crates/ui/src/panel_modern.rs:535-546; crates/ui/src/panel_classic/mod.rs:55, menu.rs:14-20 | most theme tokens the ADRs and guides name are dead data. `theme::apply` maps only the `docs/ui/themes.md` table (chrome, button hover/pressed, menu/tooltip background and border, field background) onto egui `Visuals`; no widget reads any other token. `menu.highlight.fill`/`.border` never paint: the hovered and open rows use `visuals.selection.*`, which `theme.rs:261` sets from `button.pressed.fill`, so hover, pressed and `menu.bar.open.fill` (`menu.rs:201-222`) are one colour and `menu.highlight.*`, `menu.bar.open.*`, `menu.separator`, `menu.gutter`, `chrome.separator`, `chrome.title_bar`, `scroll.track`, `pane.*`, `status_bar.*`, `tab.*`, `page_tab.*`, `group.caption.text`, `group.separator`, `file_view.*` and the `contextual.*` colours (`panel_modern.rs:535` paints the contextual band with `visuals.selection.bg_fill`, hard-coded 4 px and 9 px text, not the set's `fill`/`band`/`text`) are never read. The same for the metrics: only `panel_modern/metrics.rs` reads `theme.metrics`; every Panel Classic metric (`menu.row.height`, `menu.gutter.width`, `toolbar.*`, `menu_bar.height`, ...) is a hard-coded constant (`menu.rs:14-20`, `mod.rs:55`). ADR 0063's contract ("a widget asks for a token by name", "colours and sizes can be tuned without touching widget code") and `docs/ui/themes.md:31` ("everything else \| read by the widget that draws it") plus `docs/ui/panel-classic.md:6` and `panel-modern.md:6` ("The colours and sizes named below are tokens in ...") therefore describe behaviour that does not exist; editing a theme YAML silently changes nothing on those surfaces | read the remaining tokens where they are drawn (`theme::color`/`theme::metric`, the ADR 0063 seam): the menu rows and titles take `menu.highlight.*` and `menu.bar.open.*`, the modern panel takes the `contextual.<family>.*` and `group.*`/`tab.*` tokens, and the classic widgets take their metrics from the theme like `Metrics::from_theme` does. Or, if the egui-`Visuals` indirection is the real decision, amend ADR 0063 and the guides and delete the tokens no widget reads. Whichever way, the golden PNGs then regenerate in the same commit |
| F2 | major | crates/ui/src/panel_classic/menu.rs:296-305; crates/ui/src/panel_modern/controls.rs:601-604; crates/ui/src/panel_classic/toolbar.rs:230, 234, 364-368 | the ADR 0062 rule "Disabled controls draw the icon at 40% alpha" (`docs/adr/0062:59`, `docs/ui/panel-classic.md:92`) is implemented in `icons` (`DISABLED_ALPHA`, `paint_texture`) but wired to nothing. A disabled menu row with an icon paints a solid grey `rect_filled` square where the icon goes (`menu.rs:303-304`) instead of the faded icon; `icon_themed` has an `enabled` parameter but every call site passes `true` (`controls.rs:603`, `toolbar.rs:230/234/364`), so a disabled toolbar or command-bar control draws its icon at full opacity next to faded text (egui's `AtomPaintArgs` carries no enabled state, so the raw painter does not fade it) | the menu's disabled branch should call `icons::draw(..., enabled)` (the parameter already exists), and `egui_ui_icon`/the toolbar callers should pass the row's enabled state into `icon_themed`; a kittest golden or an alpha assertion on one disabled icon pins it |
| F3 | minor | crates/ui/src/panel_classic/mod.rs:16-17; crates/ui/src/panel_classic/menu.rs:282-283 | stale comments: "Colours come from the egui defaults until task P7-05 merges the theme tokens" and "The font of the chrome text (egui defaults until P7-05)". P7-05 and P7-10 merged; the comments describe a seam that has moved on (and, per F1, are half the reason the theme contract reads as met) | rewrite to say where the colours actually come from today (the egui `Visuals` that `theme::apply` maps), or delete |
| F4 | minor | crates/ui/src/shell.rs:1369-1373 vs docs/adr/0066-welcome-wizard-and-about-window.md:46 | the ADR says "Cancel on either page closes the wizard and keeps Panel Modern", the shell says "Cancel keeps the current layout". The code behaviour is the sensible one (a wizard re-opened from Help > Welcome must not reset a Panel Classic user to Panel Modern), so the ADR text is the wrong side of the disagreement | **ruling candidate:** amend ADR 0066 to "keeps the current layout (Panel Modern on a first launch, where it is the default)"; no code change |
| F5 | minor | docs/ui/panel-classic.md:122-153 vs crates/ui/src/shell.rs:1040-1098 | the guide's context-menu table (ten targets) and the Shapes-window right-click list read as current behaviour, but only three targets are routed (a shape, the empty page, text being edited, `shell.rs:1060-1081`); the rest are the P7-09 "unrouted right-click targets" open issue | mark the unrouted rows in the guide as pending (as `docs/ui/contexts.md` marks its pending contexts) until P7-14 routes them, so the guide and the app agree |
| F6 | minor | crates/ui/src/settings.rs:13-16, 142, 164-205; crates/ui/src/shell.rs:1396-1399 | the Options dialog's "Show the welcome wizard on next start" row has no command id and comes back as an `OptionsChange` flag the shell writes straight into `Settings::welcome_seen`, while every other row goes through the command registry. AGENTS.md section 6 says every user action is a command with an id, an enabled predicate and undo support; this row is the one action outside the registry | give the row a command (`help.welcome.next_start` or an argument on `help.welcome`) in the P7-14 follow-up, or record the exception as an ADR 0066 amendment ("a settings flag, not a document action"). Undo is meaningless here, which is the likely reason it is a flag |

## Confirmed open items (no new action)

Already recorded in the merged Results or owned by the master; seen
again during the review and left alone:

- the parity matrix's one column per layout (`docs/parity/controls.md`,
  P7-02 open issue, master item);
- the File view page model and the unmapped contextual tab sets (P7-03
  open issues, adapter limits pinned by snapshots);
- the window/page switch ids, the 18 toggles and the two pending
  contexts (P7-06/P7-08 open issues, for P7-14 or later);
- the 2016/365 layouts keep the v1 widgets through the adapter output
  (P7-09 open issue, by design of ADR 0061);
- Selawik embedded a second time from `crates/text/fonts` (P7-05
  minor, noted for P7-09/P7-11): `theme.rs:24-30` includes the three
  faces again beside `crates/text`'s copy. Harmless today (one
  deduplicated asset would be nicer); recorded so the fix task can
  decide whether `crates/ui` should take the bytes from `crates/text`.

## Master notes

- F1 is the one finding worth a ruling before P7-14 touches code: it
  decides whether the theme YAML is the source of truth for the widget
  look (ADR 0063 as written) or only for the egui-mapped subset. The
  fix task should get the ruling in writing, then either wire the
  tokens or amend ADR 0063 and trim the data.
- F2, F3 and F5 are small and safe; F4 and F6 are doc/registry
  decisions, not code risks.
- The golden PNGs (ADR 0065) pin the current look, so F1's fix will
  show up as deliberate golden regenerations; no golden should change
  silently while F2 is fixed except the frames with disabled icons.
