# Kickoff: Larrygram, a clean-room, Rust, Visio-compatible diagramming app (Alpha MVP)

You are the lead engineer and orchestrator for a new open-source project: a clean-room reimplementation of a Microsoft Visio-style diagramming application, written in pure Rust and structured like the ArtCraft "Crafting Apps" (https://github.com/storytold, e.g. photocraft, gridcraft, cadcraft). Before Phase 0, read the README, AGENTS.md and docs/architecture.md of two of those repos and borrow their patterns. If you copy any of their code, keep their license notices (MIT OR Apache-2.0). 

## Settings (edit before running)

- APP_NAME: `Larrygram`.
- CHECKPOINT_MODE: `pause`.
  - `pause` means: stop after each phase gate and wait for my "continue".
  - `auto` means: proceed when the gate is green.
- LICENSE: MIT for everything except `crates/vsd-binary` (see the libvisio rules).

## Goal of this run

Ship an alpha MVP with basic, working functionality: open, render, edit and save diagrams, with a familiar tabbed command-bar UI.

The UI structure comes from Microsoft's published control-identifier data, with a runtime switch between the Panel (2016 data) and Panel (365 data) layouts.

Visual fidelity (pixel matching, icons, exact spacing) is NOT a goal of this run. A later, separate pass may refine the app's look. Design all UI data formats so that later pass can enrich them without schema changes.

## Ground rules

Put these in AGENTS.md in Phase 0. CLAUDE.md must import it with `@AGENTS.md` so the rules carry across sessions. Multiple agents will be used but are kicked off by the user via a specific prompt for each session. This is due to a variety of models being used based on the job complexity. Not all agents will be running Claude Code, some will be using pi mini agent harness.

Each agent should fork the main git repo and make edits, then merge the
changes into the main repo. The master agent will review and merge the
commits. Ensure agents are aware of being on a shared main repo which is
local to the machine.

Major downloads, large files should be placed on /Volumes/Dara/larrygram
due to limited space on the main drive.

### Clean-room

1. These are the allowed sources:
   - Microsoft Open Specifications ([MS-VSDX]).
   - Microsoft Learn documentation: the ShapeSheet reference and the Visio VBA/automation reference, including its enumerations.
   - Microsoft's published Office Fluent UI control identifiers.
   - Microsoft support articles, e.g. keyboard shortcuts.
   - libvisio, under the rules below.
   - Observed behavior.
2. Never disassemble, decompile or debug any Microsoft binary or installer, and never extract resources from one (command-bar XML, strings, icons, images, fonts).
3. Never commit Microsoft-owned content. That includes spec documents, icons, images, stencils (.vss/.vssx), templates, sample drawings and fonts (Calibri, Segoe UI).
   - Raw downloads live in `reference/`, which is gitignored.
   - Commit only derived facts (IDs, names, structure, enumerations) and our own summaries.
4. Short command labels (e.g. "Align", "Connector", "Pointer Tool") may be used as UI labels. Never copy long-form text such as super-tooltips or help descriptions; write our own.
5. Use open, metric-compatible fonts: Carlito for Calibri and Selawik for Segoe UI, bundled with their OFL licenses.
6. Use an open-licensed icon set with its license recorded, or simple placeholders. No Office icons.
7. Every stencil master we ship is original work.

### libvisio

First, verify libvisio's license in its COPYING file. It is expected to be MPL-2.0. We use it in two ways.

**a) As a test oracle.**
- Build its CLI tools in a dev-only harness under `tools/libvisio-oracle/`. Candidates are vsd2raw, vsd2xhtml and vss2raw; check which ones exist.
- Compare our parsed structure and rendered output against theirs.
- It is never a runtime dependency.

**b) As the reference for the binary .vsd format.**
- Any code ported or derived from libvisio lives only in `crates/vsd-binary`.
- That code carries MPL-2.0 file headers and attribution in NOTICE.
- It is read-only import.
- No libvisio-derived code may appear anywhere else.

For .vsdx, the MS-VSDX spec is the source of truth, not libvisio.

`cargo deny` must enforce that `vsd-binary` is the only MPL crate.

### Engineering

- **Never crash.** Malformed files produce errors, never panics.
  - Non-test code may not use `unwrap`, `expect`, `panic!`, `todo!` or `unimplemented!`. Workspace clippy lints enforce this.
  - `indexing_slicing` is denied in the parser crates (vsdx, vsd-binary, shapesheet).
  - These are allowed in tests via clippy.toml.
- **No unsafe code.** Every crate has `#![forbid(unsafe_code)]` unless an ADR justifies an exception.
- **One command registry.** Every user action is a command in a single registry, with an id, a label, an enabled-state predicate and undo support. Menus, the command bar, shortcuts, CLI, the JSON control channel and MCP all call the same commands. This is the ArtCraft pattern.
- **Units.** Internal units follow Visio's (inches); convert at the edges.
- **Decisions.** Record each decision in `docs/adr/NNNN-title.md`.
- **Commits.** Small commits with conventional commit messages, one logical change each.

## Sub-agents

In Phase 0, create these sub-agents in `.claude/agents/`. Each file's frontmatter has name, description, tools and `model: inherit`. Delegate work to them; don't do everything in the main thread.

| Agent | Tools | Job |
|---|---|---|
| researcher | Read, Write, Bash, WebFetch, WebSearch, Grep, Glob | Finds and downloads sources into `reference/`, records provenance, writes extraction code |
| architect | Read, Write, Grep, Glob | Splits each phase into tasks with crate ownership, defines interfaces before implementation, writes ADRs |
| implementer | Read, Write, Edit, Bash, Grep, Glob | Implements one task in one crate and writes unit tests alongside the code |
| test-engineer | Read, Write, Edit, Bash, Grep, Glob | Adds property tests, round-trip tests, golden/snapshot tests, fuzz targets and egui_kittest UI tests; hunts untested paths |
| reviewer | Read, Grep, Glob, Bash | Read-only review of correctness, error handling, API design, idiomatic Rust, dead code and duplication; returns findings with severity (blocker/major/minor); never edits |
| coverage-auditor | Read, Bash, Grep, Glob | Runs coverage, compares it to thresholds and to the previous phase, lists uncovered critical paths |
| cleanroom-auditor | Read, Bash, Grep, Glob | Checks each phase diff against the clean-room and license rules: runs cargo deny, scans for committed Microsoft assets, copied long-form text, and libvisio-derived code outside vsd-binary |

Workflow for every phase:

1. The architect plans the phase as tasks with owners and interfaces.
2. Implementers run in parallel only on disjoint crates or files. Integrate their work sequentially.
3. The test-engineer fills test gaps.
4. The reviewer, coverage-auditor and cleanroom-auditor run in parallel.
5. Fix every blocker and major finding, then re-run the gate until it is green.
6. Write the phase report and obey CHECKPOINT_MODE.

## Quality gate

Implement the gate as `cargo xtask gate`. It is a single command, and CI runs it via GitHub Actions on macOS, Windows and Linux. It must pass at the end of every phase.

1. `cargo fmt --all -- --check`
2. `cargo clippy --workspace --all-targets --all-features -- -D warnings`, with the workspace lints above. `missing_docs` warns on public items in library crates.
3. `cargo test --workspace --all-features`
4. `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps`
5. `cargo llvm-cov` with per-crate thresholds read from `coverage.toml`; xtask parses the JSON report.
   - geom, model, vsdx, vsd-binary, shapesheet, commands, ui-spec: at least 85% of lines.
   - render, text: at least 75%.
   - App and UI crates: at least 50%, using egui_kittest.
   - Ratchet: no crate's coverage may drop below its number from the previous phase.
6. `cargo deny check`, covering licenses, bans, advisories and sources.
7. The wasm build of the web app, from the phase where it exists.
8. Fuzz targets compile (`cargo +nightly fuzz build`). Short fuzz runs are optional.

Write each phase report to `docs/progress/phase-N.md` with these sections:
- What was built.
- Gate results.
- A per-crate coverage table compared with the previous phase.
- Reviewer findings and how each was resolved.
- ADRs added.
- Known issues.
- The plan for the next phase.

## Repository layout

This is a starting point; the architect may refine it via ADR.

```
crates/
  geom/          paths, arcs, elliptical arcs, NURBS, splines, transforms, hit testing
  model/         documents, pages, shapes, masters, stylesheets, themes, sections/rows/cells; preserves unknown XML
  vsdx/          OPC package + XML read/write for .vsdx/.vssx/.vstx, lossless round-trip
  vsd-binary/    read-only .vsd/.vss/.vst import (MPL-2.0, libvisio-derived)
  shapesheet/    formula lexer/parser/evaluator, dependency graph, recalc
  text/          text layout (cosmic-text or parley; decide via ADR)
  render/        scene graph shared with the UI, tiny-skia PNG renderer, SVG writer
  commands/      command registry, undo/redo, selection model
  ui-spec/       loader for command-bar, menu and shortcut layout data, layout switching
  control/       JSON control channel + MCP server
apps/
  {{app}}/       desktop app (eframe/egui)
  {{app}}-web/   wasm build (trunk)
  {{app}}-cli/   inspect / render / export / run-command
tools/
  libvisio-oracle/
xtask/
ui-spec/         committed, generated layout data (Phase 1)
reference/       gitignored raw downloads
corpus/          test drawings + PROVENANCE.md
docs/            adr/, progress/, parity/, sources.md
```

## Phases

### Phase 0: Bootstrap

Set up the following:
- **Workspace config:** the Cargo workspace, `rust-toolchain.toml`, workspace lints, `clippy.toml`, `deny.toml`, and `.gitignore` (including `reference/`).
- **Agent and rule files:** AGENTS.md, CLAUDE.md (`@AGENTS.md` plus a short project summary), and the sub-agent files.
- **Tooling:** xtask with `gate`, and the CI workflow.
- **Crates:** empty crates, each with `#![forbid(unsafe_code)]`.
- **README:** states the alpha status honestly.

Exit: the gate is green on the empty workspace.

### Phase 1: Gather resources and definitions

The researcher downloads sources into `reference/` and records provenance in two places:
- `reference/SOURCES.md`, local only, with URL, retrieval date, version, license or terms, and what we commit versus keep local.
- `docs/sources.md`, committed, with the same information minus local paths.

Sources:

1. **Office Fluent UI control identifiers:** https://github.com/OfficeDev/office-fluent-ui-command-identifiers. Get the Visio files for both Office 2016 and Microsoft 365. If the 2016 Visio data is missing from the repo, use the Office 2016 download: https://www.microsoft.com/en-us/download/details.aspx?id=50745
2. **VisUICmds enumeration:** https://learn.microsoft.com/en-us/office/vba/api/visio.visuicmds. Also get the other Visio enumerations the model needs (section, row and cell indices; units).
3. **[MS-VSDX]: Visio Graphics Service VSDX File Format**, from Microsoft Open Specifications on learn.microsoft.com, including its XML schemas.
4. **The Visio ShapeSheet reference** on Microsoft Learn (sections, rows, cells, functions).
5. **"Keyboard shortcuts for Visio"** on support.microsoft.com.
6. **libvisio source**, from git.libreoffice.org/libvisio or its GitHub mirror. Record the commit hash.
7. **Openly licensed .vsd/.vsdx test files**, e.g. libvisio's and LibreOffice's test data.
   - Record each file's license in `corpus/PROVENANCE.md`.
   - Skip anything whose license is unclear.
   - Add hand-authored minimal fixtures and malformed files.

Extraction is Rust code in xtask, with tests. Produce these outputs:

- **`ui-spec/panel-2016.yaml` and `ui-spec/panel-365.yaml`.**
  - Content: tabs → groups → controls, plus backstage and context menus.
  - Fields per control: id, type, label, parent and order.
  - Also include empty optional fields reserved for the later screenshot pass: `bounds`, `icon_ref`, `keytip`, `size_variant`, `visual_notes`.
- **`ui-spec/schema.json`**, which validates both layout files.
- **`ui-spec/commands.yaml`**, which maps our command ids to VisUICmds names and to control ids. Each mapping has a `mapping_confidence` field: exact, inferred or unknown.
- **`ui-spec/shortcuts.yaml`.**
- **`ui-spec/diff-2016-vs-365.md`**, a generated structural diff of the two layouts.
- **A ShapeSheet table** of sections, rows and cells with types and default units, used as codegen input for `crates/model`.
- **`docs/vsdx-inventory.md`**, listing every MS-VSDX part, element and attribute with an MVP status: supported, preserved-only or later.
- **`docs/parity/controls.md`**, a generated matrix of every control id in both layouts with a status: implemented, stub or out-of-scope. Regenerate it every phase.

Exit:
- The gate is green.
- Every Phase 1 output can be regenerated from `reference/` with one xtask command.
- The schema validation tests pass.

### Phase 2: Document model and .vsdx I/O

Build:
- Read .vsdx/.vssx/.vstx into `model` and write it back.
- Preserve all unknown parts, elements and attributes.
- Resolve master/instance inheritance and stylesheet inheritance through a lookup API. Do not flatten them on load.

Tests:
- Round-trip every corpus file, checking semantic XML equivalence.
- Property tests.
- Malformed-input tests.
- A fuzz target for the package reader.
- An oracle comparison against libvisio's raw dump: shape, page and master counts, plus key cell values.
- Written files validate against the MS-VSDX schemas.

Exit: every corpus .vsdx file round-trips without loss and without panics.

### Phase 3: Rendering from cached values, plus the CLI

Render pages from cached cell values. No formula evaluation yet. Rendering must cover:
- All Geometry row types.
- Fills: solid and basic gradients.
- Lines: weight, pattern, line ends/arrows, rounding.
- Basic shadows.
- Text blocks with basic formatting.
- Master inheritance.
- Theme colors and fonts.
- Layer visibility.
- Page size and background pages.

Output PNG (tiny-skia) and SVG. Add CLI commands `inspect`, `render` and `export`.

Tests:
- Golden image snapshots with tolerance.
- A structural comparison against libvisio's SVG output where meaningful.
- Unit tests for each geometry row type.

Exit: every corpus file renders without panics, and every geometry row type has tests.

### Phase 4: App shell and switchable panes

Build the eframe desktop app:
- **Command bar:** generated from `ui-spec`, covering tabs, groups and controls. Galleries can start as basic dropdowns.
- **Quick access toolbar.**
- **Shapes window:** with original "Basic Shapes" and "Basic Flowchart" stencils.
- **Drawing canvas:** pan, zoom, rulers and grid.
- **Page tabs and status bar.**
- **Panes:** basic Shape Data and Size & Position panes.

Layout switching (2016 ↔ 365) is available three ways, and both layouts bind to the same command registry:
- From View/Options at runtime, with no restart.
- From the CLI flag `--ui-layout`.
- As a persisted setting.

Also:
- Controls without an implemented command render disabled, with a "not yet implemented" tooltip.
- Keyboard shortcuts come from `shortcuts.yaml`.
- Open and save use native dialogs.
- The wasm build uses file upload and download.

Tests: egui_kittest tests for command-bar generation, layout switching, enabled/disabled state and shortcut dispatch.

Exit: both layouts load and switch without a restart, and opening corpus files shows them on the canvas.

### Phase 5: Editing core

Build these editing features:
- **Pointer tool:** select, multi-select, marquee, and move/resize/rotate with handles.
- **Snapping:** to the grid and to connection points.
- **Text editing** on shapes.
- **Drawing tools:** rectangle, ellipse, line and connector.
- **Stencil drag:** drag shapes from the stencil onto the page.
- **Dynamic connector:** glues to connection points and to shapes, routes straight or with simple right angles, and re-routes when shapes move.
- **Clipboard and deletion:** copy, cut, paste, duplicate and delete.
- **Arrangement:** align, distribute, group/ungroup, bring forward/send backward.
- **Pages:** add, rename, delete and reorder.
- **Undo/redo** for everything.
- **Saving:** saved .vsdx files re-open identically in our app.

Tests:
- Headless command-level tests through the registry for every editing command.
- Property tests that undo is the inverse of each command.
- Connector glue tests.
- egui_kittest interaction tests.

Exit:
- A user can build a 10-shape flowchart with connected shapes across two pages.
- The user can save it, reopen it and keep editing.

### Phase 6: ShapeSheet engine (core subset)

**Parser.** A parser for the full formula grammar, covering:
- Units.
- Cross-shape references (`Sheet.N!Cell`).
- Section and row references.

**Evaluator.** An evaluator for a core function set:
- Arithmetic and comparison.
- Unit conversion.
- IF, AND, OR, NOT.
- MIN, MAX, ABS, INT, ROUND.
- GUARD and basic SETF.
- Trig.
- Any helper functions that common masters need.

**Recalc.** A dependency graph with incremental recalc driven by edits, so that size and position changes recompute geometry.

Unknown functions keep their cached value and log a structured warning. They never fail the document.

Tests:
- Parser property tests and a fuzz target.
- Function table tests.
- Recalc order tests.
- A cross-check against cached values in corpus files: where every function in a formula is supported, our result must match the stored value.

Exit:
- Resizing master-based shapes from the corpus recomputes geometry correctly for supported functions.
- The phase report includes the agreement rate with cached values.

### Phase 7: Panel Modern and Panel Classic layouts, the rename, the welcome wizard

Two complete, designed layouts on top of the command registry:

- **Panel Classic**: a menu bar with access keys, docked toolbars (Standard
  and Formatting shown; 14 more under View > Toolbars) and context
  menus. Separate menu bars apply when a ShapeSheet window is active or
  a stencil is being edited.
- **Panel Modern**: tabs whose groups place every control in a stack or rows
  slot at a large, medium or small size. It also has:
  - contextual tabs (Picture Tools, Container Tools, ShapeSheet Tools)
    that follow the selection;
  - a Quick Access Toolbar;
  - a collapsible command bar;
  - a File view.

Both layouts are data (`ui-spec/layouts/*.yaml`, layout format v2, ADR
0061) and are drawn by one set of widgets. The Phase 4 Fluent-data
layouts become **Panel (2016 data)** and **Panel (365 data)** and are
converted to the same model on load. All four layouts switch at run
time without a restart.

Also:

- **The rename:** Ferrogram becomes **Larrygram** at the rename
  boundary: crates, binaries, environment variables, docs, the
  settings storage key, the web page title and the version
  (`1.0.0-alpha.1`, "Larrygram v1.0 alpha"). Side panels become
  panes, so "Pane" names only the layouts.
- **The welcome wizard:** two pages (Welcome, then Choose your
  layout), opened on the first launch and again from Help >
  Welcome (ADR 0066).
- **The About window:** the dedication to Larry Jeffus, the video
  link and Close, from **Why Larrygram?** and **Help > About
  Larrygram** (ADR 0066).
- **Brand assets:** `about_screen.svg`, `panel-layout.svg`,
  `classic-layout.svg`, `icon.svg` and `larry.png` under
  `crates/ui/assets/brand/` (ADR 0067). `icon.svg` is the window
  icon and the favicon, and is MIT since 2026-11-03; the other four
  are all rights reserved, used with permission, and forks must
  replace them.
- **Icons:** Tabler outline icons (MIT), chosen by meaning, mapped in
  `ui-spec/icons.yaml` and tinted with theme tokens (ADR 0062). No
  third-party application artwork.
- **Themes:** per-layout colour, font and metric tokens (ADR 0063).
  Selawik is the UI font.
- **Selection contexts:** ten named contexts drive contextual tabs,
  context menus and a target availability table for every command (ADR
  0064).
- **New commands:** the commands the new controls need, added as stubs.
  Controls without an implemented command stay disabled with "Not yet
  implemented".

Tests:

- data validation and cross-references;
- count pins per layout;
- egui_kittest tests that open every menu and dispatch every enabled
  item;
- a command-bar placement test;
- golden PNGs of both layouts, both wizard pages and the About window;
- the context cross-check against the registry predicates;
- the wasm build.

Exit:

- all four layouts load and switch without a restart;
- every icon key draws its icon;
- every control dispatches its command;
- contextual tabs appear exactly in their contexts;
- a fresh install opens the wizard and the next launch skips it;
- the About window and its video link work from every layout;
- the phase report shows Larrygram's own renders of both layouts.

### Phase 8: Binary .vsd import (read-only)

In `crates/vsd-binary` (MPL-2.0), import .vsd/.vss/.vst into `model`, using libvisio as the reference. The workflow is open → edit → save as .vsdx.

Tests: an oracle comparison with libvisio dumps, a fuzz target, and malformed-input tests.

Exit: corpus .vsd files open without panics, and the report includes parity against libvisio.

### Phase 9: Agents (control channel and MCP) — paused

Paused; it starts after Phase 8 at the earliest.

Build a JSON control channel and an MCP server that expose:
- Every registered command.
- Document inspection.
- Render-to-PNG, so an agent can drive the app and verify its own work.

Document both in `docs/agents.md` and `docs/control-protocol.md`.

Tests: protocol tests, plus an end-to-end script that builds a flowchart over MCP and checks the result.

## Alpha exit criteria

- All gates are green on macOS, Windows, Linux and the wasm build.
- Corpus .vsdx and .vsd files open without crashes and render recognizably.
- The Phase 5 flowchart workflow works end to end.
- Saved .vsdx files validate against the MS-VSDX schemas.
- All four Panel layouts switch at runtime.
- The parity matrix is regenerated and honest.
- The README states the alpha status and lists what does not work yet.

## Out of scope for this run

- Crawling or screenshotting the live Visio UI (a separate later pass).
- Pixel or visual fidelity.
- Data linking and data graphics.
- Containers and lists, beyond preserving them.
- VBA, macros and add-ins.
- Coauthoring and cloud features.
- Printing.
- Writing binary .vsd.
- Large stencil libraries.

## Start

Begin with Phase 0. Before writing any code, post:
- The task breakdown for Phases 0 and 1.
- The sub-agents you will create.
- Any questions that would block you.

Then proceed.
