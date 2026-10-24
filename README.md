# Larrygram

A clean-room, open-source diagramming app in pure Rust that aims to read and write Microsoft Visio files (.vsdx, with read-only .vsd import).

> **Status: pre-alpha, Phase 6 of 9 done; Phases 7 and 8 in progress.** The app opens, renders, edits and saves `.vsdx` files: you can draw, move, connect and type into shapes across pages, undo it, save, reopen and keep editing. ShapeSheet formulas are evaluated: edits recalculate dependent cells, and resizing a master-based shape recomputes its geometry. The Panel Modern and Panel Classic layouts are built and tested but not yet drawn by the app, and the `.vsd` importer reads the corpus files but the app cannot open them yet. It is not ready for real work: visual fidelity is approximate and the control channel is not built.

## Build status

Last full gate on `main`: **green, 2026-10-24** (commit `6ff5611`, mid Phase 7 and 8). Every merge batch since then passed the quick gate with the corpus tests (latest `51b9889`, 2026-10-25). The gate runs locally, on one machine, through a queue (`tools/gate-queue/`); there is no hosted CI.

| Step | Result |
|---|---|
| codegen, parity, fmt, clippy, doc, deny, layers | pass |
| test (`cargo test --workspace --all-features`) | pass |
| coverage (per-crate thresholds and ratchet) | pass |
| wasm (`larrygram-web`) | pass |
| fuzz (`cargo +nightly fuzz build`) | pass |

Line coverage per crate (`cargo xtask coverage`):

| Crate | Coverage | Crate | Coverage |
|---|---:|---|---:|
| geom | 100.0% | text | 99.3% |
| shapesheet | 97.0% | commands | 98.4% |
| model | 99.7% | vsdx | 98.2% |
| ui-spec | 99.4% | larrygram-cli | 97.8% |
| render | 98.5% | ui | 97.2% |
| vsd-binary | 100.0% | larrygram-web | 77.9% |
| larrygram (desktop) | 80.5% | | |

These are the numbers from the `6ff5611` full gate. `control` has no code yet (Phase 9).

## What works today

- **UI and ShapeSheet definitions** (Phase 1), generated from Microsoft's published data by `cargo xtask extract`: the Panel (2016 data) and Panel (365 data) layouts, 977 command ids, the keyboard shortcut table and the ShapeSheet table.
- **Document model and `.vsdx` I/O** (Phase 2): pages, masters, shapes, styles and ShapeSheet cells; lossless read and write, with content the model does not cover kept in place. Every corpus file round-trips.
- **Rendering and CLI** (Phase 3): pages render from cached cell values to PNG (`tiny-skia`) and SVG, with text shaping and bundled open fonts (Carlito, Selawik). `larrygram-cli inspect`, `render` and `export`.
- **App shell** (Phase 4): desktop (eframe) and web (wasm) apps with a command bar generated from the 2016 or 365 layout, switchable without a restart; one command registry behind the command bar, menus, shortcuts and CLI; canvas with zoom, pan, grid and rulers; panes; original stencils; file open, save and export.
- **Editing core** (Phase 5):
  - select, marquee, move, resize and rotate with handles and snapping;
  - pointer, rectangle, ellipse, line, text and connector tools, chosen through registry commands;
  - drag masters from the Shapes window onto the page;
  - dynamic connectors that glue to connection points or outlines and re-route when shapes move;
  - in-place text editing;
  - cut, copy, paste, duplicate, group, align, distribute, z-order, and page add, delete, rename and reorder;
  - undo and redo for every edit.

  The Phase 5 exit test builds a 10-shape flowchart with 5 glued connectors on two pages, saves it, reopens it and keeps editing (`apps/larrygram/tests/flowchart.rs`). Phase report: [docs/progress/phase-5.md](docs/progress/phase-5.md).
- **ShapeSheet engine** (Phase 6), in `crates/shapesheet`:
  - a formula parser and printer: 976 of the 994 distinct formulas in the test corpus parse, and every one of them prints back unchanged;
  - values with units and Visio's type conversions;
  - an evaluator with 42 functions (arithmetic, logic, `IF`, `GUARD`, `SETF`, trig, rounding, string and unit helpers);
  - reference resolution across shapes, masters, pages and styles, with inheritance;
  - a dependency graph with incremental recalculation and cycle detection;
  - every edit recalculates inside its undo step, missing values are filled in on open, and the Shape Data pane edits values (`data.set.property`).

  Against the values Visio saved in the 15 corpus files, the engine agrees on all 5 184 cells it can compute (100.0%). Resizing a master-based shape recomputes its geometry. Phase report: [docs/progress/phase-6.md](docs/progress/phase-6.md).
- **Panel layouts and the welcome wizard** (Phase 7, in progress). The app is renamed to Larrygram. Built and tested, waiting for the shell integration (task P7-09):
  - the layout format v2 loader with its cross-checks, the curated Panel Modern and Panel Classic layouts, and an adapter that converts the 2016 and 365 layouts;
  - the Panel Modern widgets: tabs, groups, every control kind, the quick access toolbar and the File view;
  - the Panel Classic widgets: menu bar, toolbars and context menus;
  - selection contexts that pick the right-click menu and the contextual tabs;
  - theme tokens on egui, with Selawik as the UI font;
  - the icon pipeline: an open-licensed icon set (Tabler), rasterised and cached;
  - the two-page welcome wizard.

  Until P7-09 lands, the app keeps the Panel (2016 data) layout as the default.
- **Binary `.vsd` import** (Phase 8, in progress), in `crates/vsd-binary`: the compound file reader with its decompression, chunks, shapes and geometry, text, styles, pages, masters and the document properties. `import` reads all five corpus `.vsd` files into documents. Not yet done: opening a `.vsd` in the app (P8-09), connectors (the connect records are not mapped yet), and the libvisio parity report (P8-08, P8-10).

## Next

P7-09 builds the window from the active layout (Panel Modern becomes the default) and opens the welcome wizard on first launch. P8-09 then opens `.vsd` files in the app, and P8-10 compares the importer against libvisio.

The live task board is [docs/STATUS.md](docs/STATUS.md). Phase reports are in [docs/progress/](docs/progress/). Command coverage against the command bar is in [docs/parity/controls.md](docs/parity/controls.md).

## What does not work yet

Functions outside the core set (their cells keep the stored value), the document and master sheets in formulas, opening `.vsd` files in the app, the Panel Modern and Panel Classic layouts in the app window, the MCP/control channel, and most command-bar commands (they are listed but stubbed).

## Plan (alpha MVP)

| Phase | Scope | State |
|---|---|---|
| 0 | Bootstrap: workspace, gate, CI, agent process | done |
| 1 | Sources and UI/ShapeSheet definitions | done |
| 2 | Document model and .vsdx I/O (lossless round-trip) | done |
| 3 | Rendering (PNG/SVG) and CLI | done |
| 4 | App shell, switchable 2016/365 panes | done |
| 5 | Editing core | done |
| 6 | ShapeSheet engine (core subset) | done |
| 7 | Panel Modern and Panel Classic layouts, the Larrygram rename, the welcome wizard | in progress |
| 8 | Binary .vsd import (read-only) | in progress |
| 9 | Control channel and MCP | paused |

Out of scope for the alpha: visual fidelity, data linking, containers/lists beyond preservation, VBA/macros, coauthoring, printing, writing .vsd, large stencil libraries.

## Clean room

Larrygram is built only from public specifications ([MS-VSDX]), Microsoft Learn reference documentation, Microsoft's published Office Fluent UI control identifiers, public support articles, observed behavior, and libvisio (MPL-2.0, confined to `crates/vsd-binary`). No Microsoft code, binaries, icons, fonts, stencils or templates are used or shipped. See [AGENTS.md](AGENTS.md).

Visio and Microsoft 365 are trademarks of Microsoft Corporation. Larrygram is not affiliated with or endorsed by Microsoft.

## Building

```sh
cargo build --workspace
cargo xtask gate          # full quality gate
cargo xtask gate --quick  # skip coverage, wasm and fuzz
tools/gate-queue/gateq submit <id> --skip wasm,fuzz   # agents: run the gate through the queue
cargo xtask codegen       # regenerate the ShapeSheet table
cargo xtask corpus        # fetch the test corpus (needs network)
```

The gate needs `cargo-deny`, `cargo-llvm-cov`, a nightly toolchain with `cargo-fuzz`, and the `wasm32-unknown-unknown` target.

## License

MIT (see [LICENSE](LICENSE)), except `crates/vsd-binary`, which is MPL-2.0 (see [NOTICE](NOTICE)).

The brand assets in `crates/ui/assets/brand/` (`about_screen.svg`, `panel-layout.svg`, `classic-layout.svg`, `icon.svg`, `larry.png`) are **not** covered by the MIT licence: the four SVGs are © 2026 Janet Jeffus, made in VectorCraft, all rights reserved, used in Larrygram with permission; `larry.png` is used with Larry Jeffus's permission, all rights reserved. See [ATTRIBUTION.md](ATTRIBUTION.md). A fork must replace these five files before shipping.
