# Phase 7 clean-room and licence audit

- Task: P7-13, role `cleanroom-auditor`
- Diff audited: `git diff phase-6..main` (`812d307..d9c77ec`), 713 files,
  +66,720/-7,429 in total; the Phase 7 part (assets, `ui-spec` data, the
  brand art, the UI text and the Phase 7 docs) = about 627 files,
  +40,512/-7,055. The interleaved Phase 8 work (`crates/vsd-binary`,
  `tools/libvisio-oracle`, the `.vsd` fixtures and goldens, ADRs
  0068-0072, `xtask/src/scout.rs`) is left to the Phase 8 clean-room
  audit and is touched here only where a rule names it directly (the
  MPL-header check)
- Date: 2026-10-25, branch `agent/P7-13`

Method: `cargo deny check` in the worktree; `git ls-files` scanned for
binary and Microsoft-owned file types across the whole tree, every
committed asset cross-checked against the `ATTRIBUTION.md` rows; the
245 committed Tabler SVGs and the licence file byte-compared against
the pinned upstream package (`@tabler/icons` 3.49.0, npm tarball
`icons-3.49.0.tgz`, sha256
`aa3042907eb0bdb079fd70733e2d75f785683d4726279128a0ca2343099f7074`,
downloaded to `reference/tabler/` for this audit and kept local-only);
the brand SVGs read element by element (embedded rasters, `xlink:href`
targets, `title` elements); `ui-spec/icons.yaml` checked against the
committed icon directory in both directions; every `label:` in the v2
layouts measured and every `description:`/note read; the added lines of
the Phase 7 `.rs`, `.yaml` and `.md` files shingle-compared against the
reference corpus (`reference/learn`, `reference/support` and
`reference/fluent-ui-ids`: 2,927 text files, about 280k distinct
6-word shingles) at 8 words and 6 words; libvisio identifiers grepped
outside `crates/vsd-binary`, and the MPL-2.0 header checked in every
file of that crate; `git ls-files reference` checked empty;
`corpus/PROVENANCE.md`, `ATTRIBUTION.md` and `docs/sources.md` rows
checked against the committed files.

## Verdict

**Pass with findings: 0 blocker, 0 major, 2 minor** in the Phase 7
assets and data. Every icon is a byte-identical Tabler 3.49.0 outline
file covered by its `ATTRIBUTION.md` row; no icon or image is
third-party artwork outside the recorded sets; the brand assets carry
their all-rights-reserved rows and no third-party application artwork
is in the repository; the layout labels are short UI labels; no
long-form text is copied; the tooltips and guides are our own words;
`docs/sources.md` and `ATTRIBUTION.md` are complete. The two minor
findings are the missing MPL-2.0 header on one `vsd-binary` fuzz
target (F1) and the local-only `reference/SOURCES.md` row 21 that
records no local copy of the Tabler package (F2).

The hand-off gate for this task is **GREEN** (`cargo xtask gate --quick`
through the gate queue: the task changes no code).

## Findings

| Id | Severity | Location | Rule | Suggested fix |
|---|---|---|---|---|
| F1 | minor | `crates/vsd-binary/fuzz/fuzz_targets/import.rs:1` (P8-10, outside the Phase 7 assets but named by the role's check 4) | AGENTS.md section 5 / the role file: every file in `crates/vsd-binary` has the MPL-2.0 file header | The other 25 `.rs` files of the crate carry the standard MPL-2.0 notice; this one starts directly with `//! Fuzz target ...`. Add the same three-line header. The file is our own harness code, not libvisio-derived, so this is a form defect, not a licence one. For P7-14 or a Phase 8 fix. |
| F2 | minor | `reference/SOURCES.md` (local only, the P7-01 section, row 21) | AGENTS.md section 4: record every source in `reference/SOURCES.md` (local) | The row records the Tabler URL and version but no local copy, while the neighbouring rows give paths and sha256 values. The npm tarball now sits at `reference/tabler/icons-3.49.0.tgz` (sha256 `aa3042907eb0bdb079fd70733e2d75f785683d4726279128a0ca2343099f7074`, extracted package beside it), fetched by this audit. Add that path and hash to the row. Nothing committed changes. |

## Checklist

1. **Every icon is an unmodified Tabler file covered by the
   `ATTRIBUTION.md` row.** Pass. All 245 files of
   `crates/ui/assets/icons/tabler/` are byte-identical (`cmp`) to
   `icons/outline/` of the pinned `@tabler/icons` 3.49.0 package, and
   `LICENSE` is byte-identical to the package's `LICENSE` (MIT,
   Copyright (c) 2020-2026 Paweł Kuna). `ui-spec/icons.yaml` names
   exactly those 245 files across its 394 keys: no named icon missing,
   no committed icon unused. The two `ATTRIBUTION.md` rows (the SVGs,
   the `LICENSE`) name the source, the version and the licence, and
   `docs/sources.md` row 21 records the URL and the version. The
   committed SVGs contain no edits at all: recolouring happens at run
   time via `currentColor` (ADR 0062), as the row says.

2. **No icon or image is third-party artwork outside the recorded
   sets.** Pass. The whole tree holds 245 Tabler SVGs, 5 brand files,
   6 test-fixture placeholder SVGs (one empty
   `<svg .../>` per `ui-spec` fixture, our own bytes, `ATTRIBUTION.md`
   row), 24 font files (Carlito and Selawik with their OFL texts,
   rows), our own test goldens and snapshots, and our own fixture and
   fuzz-seed bytes. No Microsoft-owned file type anywhere in
   `git ls-files`: no Office document, stencil, template, icon
   collection, font (Calibri, Segoe UI), spec PDF/DOCX, UI XML or
   Visio screenshot.

3. **The brand assets (ADR 0067) have their all-rights-reserved rows
   and no third-party application artwork entered the repository.**
   Pass. All five files in `crates/ui/assets/brand/` have individual
   `ATTRIBUTION.md` rows with the all-rights-reserved terms (four SVGs
   © 2026 Janet Jeffus made in VectorCraft, `larry.png` used with Larry
   Jeffus's permission), and the three goldens that render them carry
   the split row with the same restriction and the fork note. The
   README "License" section repeats the terms and tells forks to
   replace the five files. The four SVGs contain no embedded raster
   (`base64` absent), no external reference: every `xlink:href` is an
   internal `#id` (`#sparkle`, `#grip`, `#iPage`, ...). The `title`
   elements match the rows' claim ("renamed to the file name").
   The mock toolbar glyphs inside the two layout pictures (a "B", a
   pointer, alignment bars) are generic shapes drawn by the artist in
   VectorCraft; the pictures show Larrygram's own Panel Modern and
   Panel Classic layouts, not another product's artwork.

4. **Layout labels are short UI labels.** Pass. The longest `label:`
   values in the v2 layouts are command labels
   (`ui-spec/layouts/panel-modern.yaml:1352` "Apply after Linking Data
   to Shapes", `panel-classic.yaml:256` "Mail Recipient (as
   Attachment)...", `:2115` "ShapeSheet Formula Tracing Window",
   `:4351` "Delete Markup Overlays on Current Page", `:4364` "Delete
   All Markup Overlays in Document"): 33-45 characters, one command
   each, the kind AGENTS.md section 4 allows as UI labels. Everything
   else is shorter. The longer strings in the layouts are `notes`
   (region and context descriptions), our own words.

5. **No long-form text is copied; tooltips and guides are our own
   words.** Pass. The 8-word shingle scan over the whole Phase 7 diff
   finds only recorded derived facts (the knot-vector numbers of source
   17 in `crates/geom/src/curve.rs`, the `VisFromParts`/`VisToParts`
   constant tables of source 18 in `docs/sources.md`), one
   enumeration-sentence quote in `docs/tasks/P1-01.md` that predates
   Phase 7 (the file's diff is the product rename only)
   and the audit reports quoting findings. The 6-word scan over the
   Phase 7 added lines finds one functional fragment in
   `docs/STATUS.md:308` ("open the page tab context menu", the
   shortcut extractor's dropped description) and nothing else. No
   super-tooltip, help description or wizard sentence matches any
   source: the wizard and About strings (`crates/ui/src/wizard.rs`,
   `about.rs`) are our own sentences, and ADR 0061 states the rule the
   code follows ("Tooltips are written by us, not stored in the
   layout"; `crates/ui/src/tab_bar.rs:53` "the tooltip text is ours,
   never Microsoft's"). The guides in `docs/ui/**` and
   `docs/phase-7/**` are clean. The old Microsoft feature name for the
   tabbed command bar (the word the P7-01 rule bans) appears in no
   Phase 7 file, doc, label or identifier.

6. **`cargo deny check` passes; `larrygram-vsd-binary` is the only
   MPL-2.0 crate.** Pass. `advisories ok, bans ok, licenses ok,
   sources ok` in the worktree.

7. **No libvisio-derived code outside `crates/vsd-binary`.** Pass.
   The greps find only documentation mentions of the oracle harness
   (`crates/vsdx/tests/oracle.rs`, `crates/render/tests/oracle_render.rs`,
   `tools/libvisio-oracle`), the corpus manifest URLs and one factual
   comparison comment (`crates/geom/src/curve.rs:31`). MPL-2.0 headers:
   25 of the 26 `.rs` files of `crates/vsd-binary` carry the standard
   notice and `NOTICE` records the libvisio commit
   `c7c970775b750a4bf7fd1b4364d69bac9815bfcb`; the one exception is F1.

8. **`reference/` is not tracked; corpus and sources records are
   complete.** Pass. `git ls-files reference` is empty. Every
   `corpus/fixtures` file has a `corpus/PROVENANCE.md` row (our own
   bytes). `docs/sources.md` rows 21-23 cover the Tabler set, the brand
   artwork and the photograph with URL, version, terms and committed
   derived data; the table is contiguous (the blank line that P6-14's
   F6 flagged is gone). `ATTRIBUTION.md` covers every committed asset
   file by row or directory glob, checked file by file. The sources are
   all in AGENTS.md section 4's allowed list (Tabler is an
   open-licensed icon set with its licence recorded; the brand assets
   and the photograph are recorded with their permission terms).
