# Larrygram

A clean-room, open-source diagramming app in pure Rust that aims to read and write Microsoft Visio files (.vsdx, with read-only .vsd import).

> **Status: alpha in preparation, Phases 7 and 8 of 9 done.** The app opens, renders, edits and saves `.vsdx` files, and opens `.vsd`, `.vss` and `.vst` files read-only (save them as `.vsdx`). You can draw shapes and connectors, glue and re-route them, type and format text, format fills and lines, set page backgrounds, and undo all of it. ShapeSheet formulas are evaluated as you edit. The window uses the Panel Modern (command bar) or Panel Classic (menus and toolbars) layout. It is not ready for real work: visual fidelity is approximate, pictures are not drawn, most of the 2016/365 command set is still stubbed and the control channel is not built.

## Build status

Last full gate on `main`: **green, 2026-11-09** (commit `de3b2a3`, the Phase 7 gate). Every merge batch passes the quick gate with the corpus tests first. The gate runs locally, on one machine, through a queue (`tools/gate-queue/`); there is no hosted CI.

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
| shapesheet | 97.1% | commands | 98.6% |
| model | 99.7% | vsdx | 98.4% |
| ui-spec | 99.6% | larrygram-cli | 97.5% |
| render | 98.5% | ui | 97.2% |
| vsd-binary | 100.0% | larrygram-web | 78.5% |
| larrygram (desktop) | 85.5% | control | 98.4% |

These are the numbers from the `de3b2a3` full gate.

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
- **Panel layouts and the app** (Phase 7). The app is renamed to Larrygram and draws its window from layout data:
  - Panel Modern (tabs, groups, the command bar, the quick access toolbar and the File view) and Panel Classic (menu bars, docked and floating toolbars, context menus), switchable at run time; the 2016 and 365 layouts load through an adapter;
  - the welcome wizard, the About window, one theme for both layouts, and the Tabler icon set;
  - formatting: fonts, sizes, styles and alignment on the selected text, Fill and Line dialogs, a custom colour dialog, Format Painter, transparency, Page Setup and page backgrounds;
  - text: the Text Block tool, text boxes, in-place editing that matches the drawn text, and inherited master text;
  - connectors that follow Visio's glue rules: right-angle, straight and curved routes that clear the shapes, end and segment drags, the reroute options and the Connection Point tool;
  - files: recent files, Save As, and Save / Don't Save / Cancel on close and quit;
  - `cargo xtask dist` packages for macOS, Windows and Linux, the last two cross-built on the Mac.

  Phase report: [docs/progress/phase-7.md](docs/progress/phase-7.md).
- **Binary `.vsd` import** (Phase 8), in `crates/vsd-binary`: the compound file reader with its decompression, chunks, shapes and geometry, text, styles, pages, masters and the document properties. The app and the CLI open `.vsd`, `.vss` and `.vst` files read-only and save them as `.vsdx`; the importer is compared against libvisio file by file. Pictures in `.vsd` files are not imported yet. Phase report: [docs/progress/phase-8.md](docs/progress/phase-8.md).

## Next

A history clean-up (R6), then the first alpha packages for macOS, Windows and Linux on GitHub Releases. Phase 9 (the control channel and MCP) follows the alpha.

The live task board is [docs/STATUS.md](docs/STATUS.md). Phase reports are in [docs/progress/](docs/progress/). Command coverage against the command bar is in [docs/parity/controls.md](docs/parity/controls.md).

## What does not work yet

Drawing pictures (inserted images and the bitmaps in `.vsd` and `.vsdx` files), functions outside the core set (their cells keep the stored value), the document and master sheets in formulas, the MCP/control channel, and most of the 2016/365 command set (listed but stubbed; see the parity table).

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
| 7 | Panel Modern and Panel Classic layouts, the Larrygram rename, the welcome wizard | done |
| 8 | Binary .vsd import (read-only) | done |
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
cargo xtask dist mac      # macOS: the .dmg with Larrygram.app (Apple silicon)
cargo xtask dist windows  # Windows: the .zip (64-bit Windows only)
cargo xtask dist linux    # Linux: the .tar.gz (x86-64 Linux only)
cargo xtask dist windows --cross  # cross-build Windows on macOS (task R14)
cargo xtask dist linux --cross    # cross-build Linux on macOS (task R14)
```

The gate needs `cargo-deny`, `cargo-llvm-cov`, a nightly toolchain with `cargo-fuzz`, and the `wasm32-unknown-unknown` target.

The testers' packages come from `cargo xtask dist <mac|windows|linux>`: one command per platform, each run on its own system, writing `Larrygram-<version>-<os>-<arch>.<ext>` under `<target dir>/dist/`. With `--cross`, the Windows and Linux packages cross-build on an Apple-silicon Mac (task R14, ADR 0085). Every package carries both binaries (`larrygram`, `larrygram-cli`), the licences and, on Linux, the desktop entry with its icon. The builds are unsigned and have no installer. See [docs/building/macos.md](docs/building/macos.md), [docs/building/windows.md](docs/building/windows.md) and [docs/building/linux.md](docs/building/linux.md) for the builds, and [docs/building/testers.md](docs/building/testers.md) for the testers: download, checksum check, first launch of an unsigned build, bug reports. The CI (`.github/workflows/ci.yml`) runs the gate on the hosted runners; `.github/workflows/release.yml` builds the three packages for every `v*` tag and publishes a GitHub pre-release.

## License

MIT (see [LICENSE](LICENSE)), except `crates/vsd-binary`, which is MPL-2.0 (see [NOTICE](NOTICE)).

The app icon, `crates/ui/assets/brand/icon.svg` (© 2026 Janet Jeffus, made in VectorCraft), is under the MIT licence like the code. The other brand assets in `crates/ui/assets/brand/` (`about_screen.svg`, `panel-layout.svg`, `classic-layout.svg`, `larry.png`) are **not** covered by the MIT licence: the three SVGs are © 2026 Janet Jeffus, made in VectorCraft, all rights reserved, used in Larrygram with permission; `larry.png` is used with Larry Jeffus's permission, all rights reserved. See [ATTRIBUTION.md](ATTRIBUTION.md). A fork must replace these four files before shipping. Three test goldens render them (`crates/ui/tests/phase7/golden/about.png`, `wizard-welcome.png`, `wizard-layout.png`) and carry the same restriction; a fork regenerates them after replacing the art.
