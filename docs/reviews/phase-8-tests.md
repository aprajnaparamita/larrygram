# Phase 8 tests: the corpus, the fuzz target and the golden images

Task P8-10 (test-engineer), 2026-10-25. This report says what the
Phase 8 tests cover, what they do not, and the parity numbers after
this task's fixes. The tests themselves live in
`crates/vsd-binary/tests/`, `crates/vsd-binary/fuzz/`,
`crates/render/tests/corpus_vsd.rs` and `crates/render/tests/golden/vsd/`.

## What is covered

### End-to-end corpus tests

- **Every corpus `.vsd` opens and renders** (`crates/render/tests/corpus_vsd.rs`):
  17 files (the original five plus the twelve the P8-08 census
  proposed, now in `corpus/manifest.toml`), every page rendered
  through `render_png` and `render_svg`, warnings allowed (ADR 0071),
  an error or a panic fails. The two version 5 files must fail with a
  clean `UnsupportedVersion(5)` (ADR 0069).
- **The round trip** (`the_vsd_to_vsdx_round_trip_keeps_the_model_equal`):
  open a `.vsd`, save it as `.vsdx` through `write_document`, reopen
  it, and the model diff is empty for all 15 importable corpus files.
  Two things are not compared, with the reasons in the test: `Preserved`
  content (the file's verbatim XML; the `.vsdx` reader keeps elements
  the `.vsd` file never had) and arena shapes no page or master
  references (the document-scope shape below).
- **The two `fdo86729` files give the same text** (in
  `tests/import_smoke.rs`): the two summary-stream encodings carry
  identical drawing text.
- **The corpus smoke expectations** (`tests/import_smoke.rs`): each of
  the 17 files has its own expectations — page and shape counts,
  master names and ids, page sizes and one text each — checked against
  the files themselves (and `dwg.vsd`'s against its `.vsdx` twin).
  Truncated and byte-flipped corpus bytes and the `Limits` caps are
  covered as before (P8-07).

### Malformed inputs (ADR 0071)

Six hand-authored fixtures in `corpus/fixtures/` (MIT, built byte for
byte by `crates/vsd-binary/tests/common/cfb.rs` and
`tests/malformed.rs`; `malformed::committed_fixtures_match_the_builder_output`
keeps the committed files reproducible):

| Fixture | Contract pinned by its test |
|---|---|
| `minimal-drawing.vsd` | imports into a document |
| `malformed-truncated-container.vsd` | `Err`, never a panic |
| `malformed-bad-fat.vsd` (a looping FAT chain) | `Err`, never a panic |
| `malformed-lying-record-length.vsd` | warns `BadValue { "chunk record length" }`, keeps the records before the lie |
| `malformed-pointer-cycle.vsd` (two lists naming each other) | terminates, warns `Dropped { "repeated pointer target" }` |
| `malformed-absurd-count.vsd` (a list claiming 1073741824 pointers) | reads what is there, warns `BadValue { "pointer list" }`, within the caps |

`damaged_fixtures_never_panic` truncates and byte-flips every fixture.

### The fuzz target

`crates/vsd-binary/fuzz/` with the `import` target: raw bytes into
`import`, and after a good import the model lookup and the geometry
walk. Seeds are byte-identical copies of the six committed fixtures
plus three junk files (ADR 0017 section 4). `cargo +nightly fuzz build`
is green and 5000 fuzz runs over the seeds found no crash. The gate
queue skips the fuzz step for workers (AGENTS.md section 7); it was
built and run per crate.

### Golden images

15 PNG goldens in `crates/render/tests/golden/vsd/` (the first page of
every importable corpus file), compared with the ADR 0027 metric
(`tests/golden.rs`'s, copied where the test cannot share it).
Regenerate with `LARRYGRAM_UPDATE_GOLDEN=1 cargo test -p larrygram-render
--test corpus_vsd`. A mapping change that moves geometry or text fails
the comparison.

## What is not covered

- **Quick-style and theme colours.** The colour a `.vsd` shape draws is
  not the colour-table index its fill record stores: the record's
  `qsFillColour`/`qsLineColour`/`qsShadowColour` (quick-style) fields
  select a theme colour and the theme resolves it. `dwg.vsd`'s fill
  record stores white with index 1 while the oracle draws `#5b9bd5`
  (Office accent 1; the file's colour table entry 25). The import maps
  neither the quick-style fields nor the theme, so `FillForegnd` and
  friends resolve to the hard defaults. This is the largest remaining
  class of parity mismatches (fill-colour, stroke-colour, shadow-colour,
  and the `stroke`/`shadow` visibility facts on some files) and the
  reason the goldens show white shapes on white pages. Needs a
  quick-style/theme mapping decision (ADR) before it can be fixed.
- **Shape-level line and fill rows.** A shape record group carries its
  own `Line` (`0x85`) and `FillAndShadow` (`0x86`) rows (the master's
  shape in `dwg.vsd` has one; the chunks carry it at the shape's level).
  Only `style.rs` maps those record kinds, inside the styles stream:
  the shape collector treats them as known and drops them, so a
  master's own line weight and shadow never reach its instances. The
  oracle's effective `stroke-width` (0.0035in for `dwg.vsd`) comes from
  such a row. Fix is in `shape.rs`/`style.rs` (map the row onto the
  shape's sheet), small, but it changes the shape-mapping contract the
  smoke tests pin, so it is a follow-up.
- **Scaled drawings keep their geometry in page units.** The page size
  pair is stored scaled by `PageScale`/`DrawingScale` (see the fixes
  below) and so is every shape coordinate and length — the reference
  multiplies all of them by that scale. The import now scales the page
  size but not the shapes, so `Visio11PlanWithDimensions.vsd` and
  `Visio6PlanWithDimensions.vsd` render with the right page and
  oversized geometry (their goldens show it). The fix applies the same
  per-page scale to the shape transforms and geometry rows.
- **Group nesting on `no-bgcolor.vsd`** (P8-08 open issue 2). Checked
  against the dump: the file's page carries one group record (`0x47`,
  id 1) whose shape list names six sub-shapes (`0x48`, ids 2..7) at the
  same record level. The import nests by record level only (P8-04's
  contract: the parent field is the unused fallback), so it yields
  seven top-level shapes where the oracle renders six top-level layers
  and nests the sub-shape content. Not a harness artefact: the shape
  trees genuinely differ. The geometry mismatches that follow (`id2`
  ..`id7` sub-path and vertex counts) are the shifted comparison
  (P8-08 open issue 7). Fix needs the shape-list/parent join in
  `shape.rs` and a re-pin of the smoke expectations.
- **The document-scope shape.** Every corpus file carries one shape
  record outside every page (`Gesture Format`, unnamed in
  `Visio6TextFieldsWithUnits.vsd`). The oracle renders no layer for it
  and `.vsdx` has no slot for it; the import keeps it in the arena and
  the save drops it. The round trip excludes it (above); a follow-up
  can drop it at import across `pages.rs`, `shape.rs` and `text.rs`
  (they count shape records in one stream-order contract).
- **The known P8-07 gaps**, unchanged: connects are not mapped, the
  template marker is unidentified, `Document::properties` has no
  creation date (the parity `date` field stays "our side does not state
  it"), the field display spellings (P8-05) and the summary code page
  and custom properties (P8-02 seams).

## Bugs the tests found, and their fixes

1. **The style links read the wrong record offsets** (P8-08 open issue
   1). The style sheet record (`0x4a`) carries its line, fill and text
   links at `0x22`, `0x2a` and `0x32`; the mapping read `0x22`, `0x26`
   and `0x2a`, so a style's fill link read the gap and usually
   dangled. Every instance shape then resolved `FillPattern` 0 where
   the style row says 1. Layer found with the probe: the `.vsdx` reader
   resolves the same chain (Local, Master, Style(3), Style(5)) and
   only the `.vsd` import differed, so the fault was in the import
   (`crates/vsd-binary/src/style.rs`), not in `CellReader`. Fixed with
   a regression test; the parity run lifted from 68.7% to 74.4% on the
   five original files.
2. **The page-size pair is stored scaled.** The record stores the size
   in the units the drawing is scaled in; the physical inches are
   `value * PageScale / DrawingScale` (the reference's page
   computation). `Visio11PlanWithDimensions.vsd` stores 528 x 408 at
   `PageScale` 0.25 and `DrawingScale` 12 (a 1/4" = 1' plan) and the
   oracle renders 11 x 8.5 inches; the mapping kept 528 inches (which
   also made the page too large to rasterise). Fixed in
   `crates/vsd-binary/src/pages.rs` with a regression test. This also
   settles P8-07 decision 1: the pair is **width first** (528 scales
   to the oracle's 11in width, 408 to its 8.5in height; the same for
   `dwg.vsd`'s 11.69 x 8.27), so the `dwg.vsdx` twin's disagreement is
   a twin artefact as P8-07 suspected.
3. **The round trip lost the shape text.** The `.vsdx` package
   generator wrote no `Text` element from `Shape::text` (it relied on
   the preserved element that only a `.vsdx` read keeps). The `.vsd`
   text collector now goes through `Document::set_shape_text`, the
   model's write-back convention (ADR 0019).
4. **The write-back misplaced a field's display text.**
   `text_element` wrote it after the `fld` marker; XML merges adjacent
   text so the display text lost its boundary (and a following marker
   was consumed). Visio writes the display text inside the element
   (`<fld IX='0'>06.07.2015 - 13.07.2015</fld>` in
   `tdf154379-QuickStyleFillMatrix.vsdx`) and the reader maps it from
   there. Fixed in `crates/model/src/mutate.rs`.
5. **The generator dropped the colour table.** `document_part` wrote
   no `Colors` element from `Document::color_table`, so a `.vsd`
   import saved without its colour table. Fixed in
   `crates/vsdx/src/write/emit.rs` (generated when the preserved
   content carries no `Colors` element).

## Parity numbers (ADR 0072)

Run against the built oracle
(`/Volumes/Dara/larrygram/build/libvisio-oracle/bin`, libvisio
`c7c97077`, ADR 0010) at this task's branch:

```sh
LARRYGRAM_LIBVISIO_ORACLE=/Volumes/Dara/larrygram/build/libvisio-oracle/bin \
  cargo test -p larrygram-vsd-binary --test parity -- --ignored --nocapture
```

The five files P8-08 measured (the same rows, before this task's fixes
in brackets):

| file | compared | matching | parity | P8-08's first run |
|---|---|---|---|---|
| `dwg.vsd` | 95 | 71 | 74.7% | 70.5% (67/95) |
| `fdo86729-ms1252.vsd` | 56 | 39 | 69.6% | 64.3% (36/56) |
| `fdo86729-utf8.vsd` | 56 | 39 | 69.6% | 64.3% (36/56) |
| `no-bgcolor.vsd` | 61 | 38 | 62.3% | 45.9% (28/61) |
| `tdf76829-numeric-format.vsd` | 83 | 74 | 89.2% | 89.2% (74/83) |
| **total** | **351** | **261** | **74.4%** | 68.7% (241/351) |

The twelve census files the harness picked up (it walks
`corpus/vsd/`); the version 5 files error out before any comparison:

| file | compared | matching | parity |
|---|---|---|---|
| `Visio11FormatLine.vsd` | 378 | 150 | 39.7% |
| `Visio11PlanWithDimensions.vsd` | 87 | 38 | 43.7% |
| `Visio11TextFieldsWithAngle.vsd` | 27 | 18 | 66.7% |
| `Visio11TextFieldsWithCurrency.vsd` | 55 | 55 | 100.0% |
| `Visio11TextFieldsWithUnits.vsd` | 274 | 176 | 64.2% |
| `Visio6PlanWithDimensions.vsd` | 74 | 42 | 56.8% |
| `Visio6TextFieldsWithUnits.vsd` | 90 | 88 | 97.8% |
| `Visio5PlanWithDimensions.vsd` | 0 | 0 | (out of scope) |
| `Visio5TextFieldsWithUnits.vsd` | 0 | 0 | (out of scope) |
| `bitmaps.vsd` | 7 | 6 | 85.7% |
| `bitmaps2.vsd` | 38 | 16 | 42.1% |
| `tdf76829-datetime-format.vsd` | 210 | 131 | 62.4% |
| **all 17 files** | **1591** | **981** | **61.7%** |

The remaining mismatches by field over the whole run: text 77 (the
P8-05 field display spellings and the version 6 text payloads), stroke
44 and stroke-width 40 (the shape-level line rows and the quick-style
line weights), stroke-color 25 and fill-color 24 and shadow-color 10
(the quick-style/theme colours), fill 17 and shadow 8 (the same, on the
new files), geometry 38 (the group-nesting difference and curved
geometry), shape count 8 (the nesting, per file).

## Corpus census (P8-08's list, added)

Twelve files from the pinned libvisio source joined
`corpus/manifest.toml` and `corpus/PROVENANCE.md`: the version 11 plan,
line-format, bitmap and text-field sets and the `tdf76829` datetime twin, the version 6 plan and text-field files (in scope, ADR 0069) and
the version 5 files (out of scope: a clean error). The 1.2 MB
`tdf154379-DrawingUnits-type.vsd` and the LibreOffice-core copies stay
out (dual licensed, source not pinned). Each file has its smoke-test
expectations and joins the merge gate's corpus tests.
