# Phase 8 report: binary `.vsd`, `.vss` and `.vst` import

Phase 8 reads the binary Visio formats, read-only: open a `.vsd`,
`.vss` or `.vst`, edit it, and save it as `.vsdx` through the Phase 5
writer. One new crate carries it, `crates/vsd-binary` (L2, MPL-2.0,
the only place libvisio-derived code may live, AGENTS.md section 5),
with the MS-CFB container, the chunk and pointer layer, and the four
domain modules that map records into the Phase 1 model. The model is
the one the `.vsdx` reader produces (ADR 0070), so the writer needs no
changes. Started 2026-10-23 (P8-01), closed 2026-10-26. The master
tags `phase-8` after this merges.

## What was built

| Area | Content | Tasks |
|---|---|---|
| Container and streams (L2) | The MS-CFB compound file from the specification (sector chains, directory, mini stream), the LZ decompression of the compressed blocks (the one libvisio-derived algorithm, in `container.rs` with its MPL header), and the two summary streams' property sets ([MS-OLEPS]). Every cap of `Limits` is checked before the allocation it guards. | P8-02 |
| Chunks and pointers | The `VisioDocument` header, the version dispatch (6 and 11 in scope, 1–5 and 7–10 rejected with `UnsupportedVersion`, ADR 0069), the 19-byte record headers, pointer lists and list-record trailers, with the visited set and nesting bounds that make the walk terminate on any input. | P8-03 |
| Shapes, text, styles | Shapes with transforms and geometry (including the NURBS and polyline curve data), text with character, paragraph, tab and field runs through the code pages, and the stylesheet, colour table, fill and line rows. Records are mapped with cached values only (the binary format has no formulas) and the ADR 0070 unit rule. | P8-04, P8-05, P8-06 |
| Pages, masters, pipeline | Pages and stencil pages into pages and masters, names through the name lists and indexes, background-page references resolved or warned, the document kind from the records, and the one entry point `import(bytes, limits)`. | P8-07 |
| Oracle parity | The ADR 0072 harness (`vsd2raw`, `vsd2xhtml` as subprocesses, dev-only), its census and its per-file reports; the P8-15 fidelity pass (own line/fill/text rows, line-end offsets, record ids, groups, paint order, drawing scale, master geometry on instances, guide shapes) and the P8-17 fixes (duplicated points, per-section `NoLine`/`NoFill`/`NoShow`, open sub-path fills). | P8-08, P8-15, P8-17 |
| The open path | `.vsd`/`.vss`/`.vst` open in the app and the CLI by extension, read-only: the import warnings reach the status line, Save writes the XML format and keeps the binary source (ADR 0026's exit codes unchanged). | P8-09 |
| Tests and fixtures | The 17-file corpus, 15 render goldens plus one file pinned by numbers, hand-authored malformed fixtures with provenance rows, truncation and byte-flip sweeps, the import fuzz target, and the round-trip test. | P8-10 |
| Robustness | Placeholder ids, currency and NaN literals, empty page parts on write, the windows part and BackPage targets (ADR 0073), and the completeness sweep: 1 993 wide-corpus packages open and round trip. | P8-19, P8-20 |
| Wide corpus | 5 075 files swept with `scout`; 32 gate-corpus proposals and 206 export rows; risk facts per file (849 of 5 075 flagged); the twin sweep over Visio's own conversions of 77 binary files and the picture comparison over the 206 export files. | P8-16, P8-18, P8-21 |
| The twin comparison | `crates/render/tests/twins/compare.rs`: the import's model against the `.vsdx` reader's model of Visio's own conversion of the same drawing, joined by Visio id, with the drawing-unit normalisation of P8-21. | P8-15, P8-17, P8-21 |
| Audits and fixes | Review, coverage and clean-room audits (below), and P8-14's fixes: nothing is dropped silently any more, the layers map, the hostile-input sweeps cover the payload readers, and the phase report. | P8-11..P8-14 |

## Exit criteria (plan §6)

1. **Every corpus `.vsd` opens without a panic and renders
   recognisably: met.** All 17 corpus files import (the two version 5
   files as a clean `UnsupportedVersion(5)`); 14 render goldens in
   `crates/render/tests/golden/vsd/` were reviewed image by image, and
   the 15th file (`tdf76829-datetime-format.vsd`) is pinned by its
   import numbers instead of a golden (see the clean-room findings).
2. **The parity numbers against the oracle are in this report: met**
   (the table below, ADR 0072).
3. **A `.vsd` saved as `.vsdx` goes through the writer unchanged and
   reopens to the same model: met.** `a_document_opened_from_a_vsd_saves_as_vsdx`
   in `crates/commands` covers the open-edit-save-reopen flow, and the
   corpus round-trip tests cover the model identity.
4. **No malformed input crashes: met.** The import fuzz target runs
   clean, the malformed fixtures and the truncation and byte-flip
   sweeps cover the record layer, and the P8-14 sweeps truncate one
   record at every offset for the curve and text payload readers. The
   one crash the audits found (the unbounded recursion of the scale
   pass) is fixed with a regression test (P8-17).
5. **The open path works end to end: met** (P8-09: the dialog filters,
   the open command and the CLI accept all three binary formats
   read-only; Save defaults to the XML format).
6. **The clean-room audit is clean: met** with the one required fix
   done (the Microsoft-template-derived golden is deleted from the
   tree; the master removes it from history in R6). `NOTICE`,
   `ATTRIBUTION.md` and `docs/sources.md` carry the rows the phase
   needs.

## Mapping coverage against ADR 0070

Row by row, as the audits and P8-14 leave it. ADR 0070 was amended
once (2026-10-26, P8-14): the layers row is implemented, and the
shape-data and connection rows are deferred with warnings.

| ADR 0070 row | Status |
|---|---|
| pages, page sheets, page properties | honoured (`pages.rs`); the page-size pair scales by `PageScale`/`DrawingScale` (P8-10) |
| background-page references | honoured, resolved or warned |
| connects | not honoured: `Page::connects` stays empty, connect lists warn `Dropped` ("connect list"); no source spells the entry payload |
| stencils, masters, name lists | honoured |
| shape groups, shapes, guide shapes, shape ids | honoured (the id and parent joins of P8-15); a shape no page or master tree references is dropped with a warning (P8-14) |
| transforms | honoured |
| geometry rows | honoured with the known row-union gap (below) |
| line, fill, shadow, text block | honoured since P8-15; quick-style and theme colours are not resolved and warn once per import (P8-11 F3) |
| character, paragraph, field, text | honoured; a field's display text is our own spelling of its format and warns per field (P8-11 F4) |
| style sheets, style properties | honoured; the style-sheet property record is dropped with a warning |
| the colour table | honoured |
| layers and layer membership | **honoured since P8-14**: the page sheet's `Layer` rows (`Name`, `NameUniv`, `Color`, `Visible`, `Print`, with the row `IX`) and the shape's `LayerMember` cell, spelled exactly as a Visio twin's rows; `Status`, `Active`, `Lock`, `Snap`, `Glue` and `ColorTrans` stay unset (no source spells them) |
| shape data, user-defined cells, custom properties | **deferred** (P8-14): dropped with a `Dropped` warning naming what the record carries, per shape or page. No allowed source spells the row payloads (the reference reads fragments and maps no rows), and a mapping derived from bytes alone risks a silent wrong value |
| connection points and controls | **deferred** (P8-14), same rule and same warning |
| document properties and kind | honoured except the creation date and the custom-property section (open issues); `DocumentKind::Template` is never produced (documented) |

## Parity against the libvisio oracle

`vsd2raw`/`vsd2xhtml` against the import, per corpus file (ADR 0072,
the harness of P8-08 as run by P8-17; `matching/compared` cells of the
facts the harness compares). This is the final number of the phase:
**75.9% (1 248/1 644)**.

| file | matching / compared | main mismatch classes and their disposition |
|---|---|---|
| `dwg.vsd` | 95/95 (100.0%) | — |
| `Visio11TextFieldsWithCurrency.vsd` | 55/55 (100.0%) | — |
| `Visio11TextFieldsWithUnits.vsd` | 271/280 (96.8%) | field display text (our own spelling; P8-11 F4 warns it, P8-17 open issue 4) |
| `Visio6TextFieldsWithUnits.vsd` | 88/90 (97.8%) | field display text |
| `fdo86729-ms1252.vsd` | 55/56 (98.2%) | field display text |
| `fdo86729-utf8.vsd` | 55/56 (98.2%) | field display text |
| `no-bgcolor.vsd` | 108/115 (93.9%) | field display text, text box placement (P8-17 open issue 5) |
| `tdf76829-numeric-format.vsd` | 74/83 (89.2%) | numeric field formats (feet-and-inches and fractions are not spelled) |
| `bitmaps.vsd` | 6/7 (85.7%) | field display text |
| `tdf76829-datetime-format.vsd` | 152/218 (69.7%) | date and time field formats (every field text is ours) |
| `Visio11TextFieldsWithAngle.vsd` | 18/27 (66.7%) | angle field formats |
| `Visio11PlanWithDimensions.vsd` | 56/86 (65.1%) | text box placement, hairline strokes, field display text |
| `Visio6PlanWithDimensions.vsd` | 49/82 (59.8%) | as above, plus the hatch fill and the labels' text backgrounds |
| `bitmaps2.vsd` | 17/38 (44.7%) | text box placement, field display text |
| `Visio11FormatLine.vsd` | 149/356 (41.9%) | quick-style and theme colours (P8-10 open issue 1: the record's quick-style index resolves through the document theme, which the import does not read; warned since P8-11 F3) and the line-end offsets |
| `Visio5PlanWithDimensions.vsd`, `Visio5TextFieldsWithUnits.vsd` | — (out of scope) | version 5 is a different record layout (ADR 0069); both import as `UnsupportedVersion(5)` |

The history of the number: 68.7% (241/351) on the first five files
(P8-08), 74.4% after the style-link offsets fix (P8-10), 61.7% over
the 17-file corpus when P8-10 widened the comparison (the new files
carry the field and colour classes), 75.3% after the P8-15 fidelity
pass, 75.9% after the P8-17 geometry fixes.

## The twin comparison

The model of `import(original)` against the `.vsdx` reader's model of
Visio's own conversion of the same drawing, joined by Visio id,
drawing units normalised (P8-21). `agree / differ` of the compared
cells; the causes are the open issues below.

| file | agree / differ | why the differences |
|---|---|---|
| `Visio11FormatLine.vsd` | 5 052 / 142 | `FillPattern`/`FillForegnd` on 43 shapes, `LineWeight`/`LinePattern` |
| `Visio11PlanWithDimensions.vsd` | 1 556 / 345 | text box placement, field display text |
| `Visio11TextFieldsWithAngle.vsd` | 195 / 2 | the field display text |
| `Visio11TextFieldsWithCurrency.vsd` | 450 / 7 | the field display text |
| `Visio11TextFieldsWithUnits.vsd` | 2 571 / 30 | the field display text (26), the text margins |
| `Visio6PlanWithDimensions.vsd` | 2 185 / 162 | text box placement, field display text |
| `Visio6TextFieldsWithUnits.vsd` | 415 / 7 | the field display text |
| `bitmaps.vsd` | 1 222 / 60 | the text margins |
| `bitmaps2.vsd` | 188 / 6 | the text margins |
| `dwg.vsd` | 134 / 4 | the geometry row union (below) |
| `fdo86729-ms1252.vsd` | 102 / 3 | the geometry row union |
| `fdo86729-utf8.vsd` | 102 / 3 | the geometry row union |
| `no-bgcolor.vsd` | 274 / 21 | the geometry row union, the text margins, the icon's cells |
| `tdf76829-datetime-format.vsd` | 183 / 681 | the two formats list the pages in different orders (the comparison joins different pages; P8-21 open issue 3) |
| `tdf76829-numeric-format.vsd` | 693 / 24 | the text margins, the field display text |
| **total** | **15 322 / 1 497** | 13 948 / 2 871 before the drawing-unit normalisation (P8-17), 1 374 mismatches gone |

**The wide twin sweep** (P8-21, 77 converted files): 249 289 cells
compared, **239 510 agree (96.1%)**, 11 045 differ, 0 panics and 0
read errors; 22 of the 77 files agree on everything the comparison
reads. The pictures (206 export files, 399 pages, our PNG against
Visio's PDF raster) score median 0.064 and mean 0.110 differing
pixels: 53 pages sit inside the ADR 0027 golden tolerance, 215 differ
on less than a quarter of their pixels.

Three causes explain the remaining differences:

1. **The field display text** (the largest class): the records carry a
   value and a format number but not the display text, so the text is
   spelled by our own English pattern, locale-blind (Visio uses the
   system locale). It warns per field since P8-11 F4; the spellings
   themselves are open issue 4 of P8-17.
2. **The geometry row union vs the twin's own rows:** a `.vsdx` states
   only the rows that differ from its master (row-level inheritance)
   while the `.vsd` record group carries the rows it draws. The
   comparison compares the two different things (P8-17 open issue 2,
   a comparison fix, not a product fault).
3. **Text boxes:** the text margins (`LeftMargin`, `RightMargin`,
   `TopMargin`, `BottomMargin`) import as 0 and the `TxtPin*`
   resolution through the master chain lands the text on the wrong box
   (P8-17 open issues 3 and 5).

## What is dropped with a warning

Every degradation records one `ImportWarning`; the UI and the CLI
surface each as a status line (ADR 0071). Since P8-14 nothing is
dropped silently: a record kind the importer drops warns `Dropped`
with what it carries, and a kind no module consumes warns
`SkippedRecord` with its format name (the policy tables sit beside the
id table in `chunks.rs`).

- **ADR 0070's dropped list:** foreign data, OLE objects and data,
  hyperlinks, events and action ids, annotations, reviewers, smart
  tags, and the document font list (the font names reach the text
  runs as cells).
- **Deferred rows (P8-14):** shape data, user-defined cells, custom
  properties, connection points and controls — one warning per record,
  naming what it carries.
- **Sheet rows no module maps (P8-14):** scratch rows, protection
  settings, misc settings, style-sheet properties, group settings,
  layout settings, image settings.
- **Structural drops:** connect lists (no source spells the entries),
  the document properties record of the record stream (the summary
  streams' properties do map), a master's page sheet, a shape no page
  or master references, and a style record outside a stylesheet.
- **Scope:** quick-style and theme colours (one warning per import),
  fabricated field display text (one per field), values that do not
  parse (a warned default), references that resolve to nothing, and
  the version 5 formats.

## What Phase 8 does not do yet

1. **Shape data, user-defined cells, custom properties, connection
   points and controls are not mapped** (the P8-14 deferral). They
   warn; mapping them needs the row payloads spelled (an ADR-level
   decision again) and lands in a later phase. This is also why the
   sweeps report "0 connects from binary import".
2. **Quick-style and theme colours** are not resolved: the record's
   quick-style index names a theme colour, and neither the fields nor
   the document theme are read. `dwg.vsd` stores white where Visio
   draws Office accent 1 (`#5b9bd5`). The model already has
   `CellValue::Str("Themed")` and `Document::themes` from the `.vsdx`
   side.
3. **Field display text** is our own spelling of the format number;
   feet-and-inches and fraction formats print empty. Visio's text
   depends on the system locale.
4. **Text boxes:** the text-block margins import as 0 and `TxtPin*`
   resolves through the master chain onto the wrong box; the labels'
   coloured text backgrounds do not draw.
5. **The renderer's remaining gaps:** hairline strokes draw sub-pixel
   (Visio floors a hairline at one device pixel), the hatch fill draws
   as a dot grid where Visio draws diagonal lines (the ADR 0023 tile
   spacing).
6. **Document level:** the creation date, the summary code page, the
   custom-property section, `DocumentKind::Template` (the template
   marker is not read) and the document-scope shape record.
7. **The comparison** does not compare resolved geometry rows
   (P8-17 open issue 2) and joins pages in file order (P8-21 open
   issue 3).
8. **Versions 1 to 5** are out of scope (ADR 0069); writing binary
   Visio files is out of scope (KICKOFF).

## Reviewer findings and resolution

| Report | Verdict | Blocker / major / minor | Resolution |
|---|---|---|---|
| [Review](../reviews/phase-8-review.md) (P8-11) | request changes | 2 / 1 / 5 | F1 (three ADR 0070 rows dropped silently) fixed in P8-14: every dropped family warns, the layers map, and the ADR is amended for the deferred rows. F2 (the unbounded recursion of the scale pass) fixed in P8-17 with two regression tests. F3 (quick-style and theme colours) warned in P8-14 with an ADR amendment; the mapping stays open. F4 (fabricated field text) warns per field. F5 (the `for`-loops over `Option`) are explicit lookups that warn on the miss. F6 (the `Type` round trip) fixed in P8-19. F7 (the tests that pinned literals) renamed to say what they pin. F8 (the `Debug` warning text, `Error::NotImplemented`) fixed: `WarningKind` has a `Display` and the stub variant is gone. |
| [Coverage](../reviews/phase-8-coverage.md) (P8-12) | no blockers | 0 / 4 / 10 | Majors 1 and 2 (the dark per-field arms of the curve and text readers) closed with the P8-14 truncation sweeps, which found and fixed one silent drop (a cut short bullet-string block). Major 3 (the open path's error arms) closed with one test per arm. Major 4 (the `non_exhaustive` catch-alls): the `Debug` fallback is deleted (the `Display` covers it) and the ones that stay carry a comment saying why they cannot be reached. Minors: the stale-index guards have a test (1), the out-of-range date arm has a test (2), the CLI positional arm has a test (6), the picker seam has the comment it asked for (7); the rest are accepted as the report recommends (defensive arms behind length checks, test-only dark arms) or are process items (the parity run per phase round). |
| [Clean-room](../reviews/phase-8-cleanroom.md) (P8-13) | pass with one required fix | 0 / 1 / 3 | F1 (the golden rendering a Microsoft-template-derived sample drawing) fixed: the golden is deleted and the file is pinned by its import numbers instead (P8-17); the master removes the file from history in R6. F2–F4 fixed on main by the master (the `docs/sources.md` and `ATTRIBUTION.md` rows, the Windows-1252 clause). |

## Fixes made for the audits (P8-14)

- **Nothing is dropped silently** (P8-11 F1): the drop policy lives
  beside the id table (`Record::dropped_what`, `Record::is_mapped`);
  the `PROP_LIST` arm of `pages.rs` is gone and the id-table comment
  that hid the bug is rewritten. Regression tests pin every family's
  warning and the silence of the mapped ones.
- **Layers map for real** (the ADR 0070 amendment): the `Layer` rows
  and the `LayerMember` cell, verified cell for cell against a Visio
  twin's `Layers` rows.
- **Unreferenced shapes go** (the wide-corpus blank-page check): a
  shape record outside every page and master tree is dropped with a
  warning and the arena renumbers; the four wide-corpus files whose
  only shape is the "Gesture Format" helper now import with zero
  shapes, as Visio converts them.
- **The quick-style and theme colours warn** once per import (P8-11
  F3), with the mapping decision recorded in ADR 0070's amendments.
- **The fabricated field display text warns** per field (P8-11 F4),
  and a date field outside the name tables has its test (P8-12 minor
  2).
- **Hostile input sweeps** truncate one curve record and one text
  record at every offset and assert the contract: map or warn, never a
  panic, never an invented value (P8-12 majors 1 and 2).
- **The open path's failure arms** have one test each (P8-12 major 3).

## Coverage

The P8-12 measurement (job 87 at `3768d10ec7`; per-crate numbers in
that report). `larrygram-vsd-binary` measures 99.9627% against its
100.0 ratchet and passes within the gate's 0.05 tolerance; the
ratchet proposal is the master's call.

| Crate | Lines | Coverage | Threshold | Ratchet |
|---|---:|---:|---:|---:|
| larrygram-geom | 2501 | 100.0% | 85 | 100.0 |
| larrygram-vsd-binary | 10720 | 100.0% | 85 | 100.0 |
| larrygram-model | 10893 | 99.7% | 85 | 99.7 |
| larrygram-ui-spec | 3844 | 99.6% | 85 | 99.4 |
| larrygram-text | 1767 | 99.3% | 75 | 98.8 |
| larrygram-render | 7237 | 98.5% | 75 | 98.5 |
| larrygram-vsdx | 7790 | 98.3% | 85 | 98.2 |
| larrygram-commands | 11005 | 98.4% | 85 | 98.4 |
| larrygram-ui | 18935 | 97.6% | 50 | 97.0 |
| larrygram-cli | 1342 | 97.5% | 50 | 96.7 |
| larrygram-shapesheet | 6744 | 97.0% | 85 | 96.9 |
| larrygram | 396 | 85.4% | 50 | 80.0 |
| larrygram-web | 144 | 78.5% | — | 77.8 |

## Gate results

The hand-off gates of the phase are green (the task Results carry
their job numbers); the coverage measurement above ran as job 87, and
the P8-14 hand-off gate is recorded in its Result. The full gate
(`cargo xtask gate` with wasm and fuzz) runs in the master's full
check after the merge, as every phase's does.

## Decisions made during the phase

- **The model is the contract** (ADR 0070): a record becomes a model
  entity with cached values and no formula, and the writer needs no
  changes. Units convert at the mapping layer, inches and radians.
- **Warn, never guess** (ADR 0071, amended by the audits): a row
  family no allowed source spells is deferred with a warning and an
  ADR amendment, not mapped from bytes alone. Nothing is dropped
  silently.
- **The oracle is a dev harness** (ADR 0072): libvisio's tools run as
  subprocesses in ignored tests; `larrygram-vsd-binary` is the only
  MPL crate and `cargo deny` keeps it so.
- **The comparison compares models** (the twin runner): Visio's own
  conversion of a binary file is the ground truth for what the drawing
  says, and drawing units normalise in the comparison (P8-21).
- **The version scope is 6 and 11** (ADR 0069); version 1–5 is a
  different parser behind `FormatVersion` for a later phase.

## ADRs added

- 0068 the compound container and the stream layer
- 0069 the chunk and pointer model; the alpha version scope
- 0070 mapping into the model and the unit rule (amended 2026-10-26:
  the layers row, the deferred rows, the quick-style colours)
- 0071 error handling, warnings and limits (its `NotImplemented`
  promise is kept: the variant is gone)
- 0072 the oracle comparison
- 0073 the windows part and background pages on write
