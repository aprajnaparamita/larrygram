# Corpus provenance

Third-party test files are **not committed** (ADR 0005). They are listed in `corpus/manifest.toml` with upstream path, pinned commit and sha256, and fetched into `corpus/vsd/` and `corpus/vsdx/` (gitignored) by `cargo xtask corpus`.

Hand-authored fixtures we create ourselves are committed under `corpus/fixtures/` and listed in the second table.

## Fetched files

All from libvisio `src/test/data/` at commit `c7c970775b750a4bf7fd1b4364d69bac9815bfcb`, licensed MPL-2.0 as part of the libvisio source tree. Many embed master shapes that were created from Microsoft templates, which is why they are fetched, not committed.

| Name | Notes |
|---|---|
| `vsd/dwg.vsd` | Small generic .vsd |
| `vsd/fdo86729-ms1252.vsd` | Codepage MS-1252 text (fdo#86729) |
| `vsd/fdo86729-utf8.vsd` | UTF-8 text (fdo#86729) |
| `vsd/no-bgcolor.vsd` | No background colour |
| `vsd/tdf76829-numeric-format.vsd` | Numeric text-field formats (tdf#76829) |
| `vsd/tdf76829-datetime-format.vsd` | Date and time text-field formats (tdf#76829) |
| `vsd/bitmaps.vsd` | Bitmap/foreign data: exercises the drop path with warnings |
| `vsd/bitmaps2.vsd` | The same, smaller |
| `vsd/Visio11FormatLine.vsd` | Line pattern, cap, join and line-end samples (version 11) |
| `vsd/Visio11PlanWithDimensions.vsd` | Dimension shapes and leaders (version 11) |
| `vsd/Visio11TextFieldsWithAngle.vsd` | Angle field formats (version 11) |
| `vsd/Visio11TextFieldsWithCurrency.vsd` | Currency field formats (version 11) |
| `vsd/Visio11TextFieldsWithUnits.vsd` | Field formats with units (version 11) |
| `vsd/Visio6PlanWithDimensions.vsd` | Dimension shapes (version 6, in scope per ADR 0069) |
| `vsd/Visio6TextFieldsWithUnits.vsd` | Text and fields with units (version 6) |
| `vsd/Visio5PlanWithDimensions.vsd` | Version 5: out of scope, must fail cleanly (ADR 0069) |
| `vsd/Visio5TextFieldsWithUnits.vsd` | Version 5: out of scope, must fail cleanly (ADR 0069) |
| `vsdx/bgcolor.vsdx` | Background colour |
| `vsdx/blue-box.vsdx` | Single rectangle |
| `vsdx/color-boxes.vsdx` | Multiple fill colours |
| `vsdx/dwg.vsdx` | Drawing shape data |
| `vsdx/fdo86664.vsdx` | fdo#86664 |
| `vsdx/qs-box.vsdx` | Quick-style boxes |
| `vsdx/recursion-cycle.vsdx` | Malformed: recursion cycle |
| `vsdx/tab-short-prefix.vsdx` | Malformed: short tab prefix |
| `vsdx/tdf136564-WhiteTextBackground.vsdx` | tdf#136564 |
| `vsdx/tdf154379-QuickStyleFillMatrix.vsdx` | tdf#154379, quick-style fill matrix |
| `vsdx/testfile1.vsdx` | Generic, with masters |
| `vsdx/testfile3.vsdx` | Variant |
| `vsdx/testfile4.vsdx` | Variant |
| `vsdx/testfile5.vsdx` | Variant |
| `vsdx/testfile6.vsdx` | Variant |

Not listed (kept in `reference/corpus-candidates/` only): the libvisio
`tdf154379-DrawingUnits-type.vsd` (1.2 MB, skipped for size) and all
LibreOffice core test files (GPL-3.0/MPL-2.0 dual licensed, from a source
the manifest does not pin). They can be added to the manifest later with a
pinned commit. The `vsd/` rows the census of task P8-08 proposed were added
by task P8-10.

## Committed hand-authored fixtures

| Path | Author | License | Notes |
|---|---|---|---|
| `fixtures/minimal-drawing.vsdx` | P2-08 | MIT | Hand-authored minimal drawing (one page, one rectangle shape). Builder output (`crates/vsdx/tests/common/`). |
| `fixtures/minimal-stencil.vssx` | P2-08 | MIT | Hand-authored minimal stencil (one master, empty pages part). Builder output. |
| `fixtures/minimal-template.vstx` | P2-08 | MIT | Hand-authored minimal template (one page, one master). Builder output. |
| `fixtures/malformed-truncated-zip.vsdx` | P2-08 | MIT | Malformed: valid package cut in half. Builder output. |
| `fixtures/malformed-bad-central-directory.vsdx` | P2-08 | MIT | Malformed: central directory signature destroyed. Builder output. |
| `fixtures/malformed-no-content-types.vsdx` | P2-08 | MIT | Malformed: `[Content_Types].xml` missing. Builder output. |
| `fixtures/malformed-dangling-relationship.vsdx` | P2-08 | MIT | Malformed: relationship target `pages/missing.xml` does not exist. Builder output. |
| `fixtures/malformed-missing-main-part.vsdx` | P2-08 | MIT | Malformed: `/visio/document.xml` missing. Builder output. |
| `fixtures/malformed-huge-declared-size.vsdx` | P2-08 | MIT | Malformed: `page1.xml` declares 100 MiB uncompressed. Builder output. |
| `fixtures/malformed-deep-nesting.vsdx` | P2-08 | MIT | Malformed: 200 nested elements in `document.xml`. Builder output. |
| `fixtures/malformed-bad-content-types.vsdx` | P2-08 | MIT | Malformed: `[Content_Types].xml` is not well-formed XML. Builder output. |
| `fixtures/render-geometry-absolute.vsdx` | P3-10 | MIT | Rendering fixture: every absolute geometry row type, one subpath each. Builder output. |
| `fixtures/render-geometry-relative.vsdx` | P3-10 | MIT | Rendering fixture: every relative (`Rel*`) geometry row type, one subpath each. Builder output. |
| `fixtures/render-fills.vsdx` | P3-10 | MIT | Rendering fixture: solid, gradient and pattern fills. Builder output. |
| `fixtures/render-lines.vsdx` | P3-10 | MIT | Rendering fixture: line weight, pattern, line ends and rounding. Builder output. |
| `fixtures/render-shadows.vsdx` | P3-10 | MIT | Rendering fixture: drop shadows. Builder output. |
| `fixtures/render-text.vsdx` | P3-10 | MIT | Rendering fixture: text with runs, paragraphs, bullets and tabs. Builder output. |
| `fixtures/render-master-inheritance.vsdx` | P3-10 | MIT | Rendering fixture: a master instance that inherits its geometry. Builder output. |
| `fixtures/render-theme.vsdx` | P3-10 | MIT | Rendering fixture: theme colours and theme fonts (`Themed` cells plus our own DrawingML theme part). Builder output. |
| `fixtures/render-layers.vsdx` | P3-10 | MIT | Rendering fixture: layer membership and layer visibility. Builder output. |
| `fixtures/render-page-size.vsdx` | P3-10 | MIT | Rendering fixture: a non-default (landscape) page size. Builder output. |
| `fixtures/render-background-pages.vsdx` | P3-10 | MIT | Rendering fixture: a background page behind the drawing page. Builder output. |
| `fixtures/minimal-drawing.vsd` | P8-10 | MIT | Hand-authored minimal binary `.vsd`: an OLE compound container with one `VisioDocument` stream (version 11 header, a trailer pointer list and two chunk records). Builder output (`crates/vsd-binary/tests/malformed.rs` and `tests/common/cfb.rs`). |
| `fixtures/malformed-truncated-container.vsd` | P8-10 | MIT | Malformed: a valid container cut in half. Builder output. |
| `fixtures/malformed-bad-fat.vsd` | P8-10 | MIT | Malformed: the stream's FAT chain loops back to its own start. Builder output. |
| `fixtures/malformed-lying-record-length.vsd` | P8-10 | MIT | Malformed: a chunk record declares a payload length beyond its region. Builder output. |
| `fixtures/malformed-pointer-cycle.vsd` | P8-10 | MIT | Malformed: two pointer lists name each other. Builder output. |
| `fixtures/malformed-absurd-count.vsd` | P8-10 | MIT | Malformed: a pointer list claims 1073741824 pointers and holds one. Builder output. |

Two more malformed fixtures are generated at test time and never committed:
`malformed-zip-bomb.vsdx` (8 MiB of zeros deflated; the deflate stream is not
stable across `flate2` versions) and `malformed-many-entries.vsdx` (8193
boring zip entries). Same author and license as above.

The files under `crates/vsdx/fuzz/corpus/read_package/` (task P2-09; moved
to the per-target directory in P5-11) are byte-identical
copies of every committed fixture above, used as the seed corpus of the
`read_package` fuzz target (ADR 0017 section 4). Same author and license
(MIT); `fixtures/` stays the single source of truth and a copy that drifts is
a bug.

The seed corpus of the `render_document` fuzz target
(`crates/render/fuzz/corpus/render_document/`, task P3-10, ADR 0027 section
6; moved to the per-target directory in P5-11) holds
byte-identical copies of four committed fixtures above
(`render-geometry-absolute.vsdx`, `render-fills.vsdx`, `render-text.vsdx`,
`render-theme.vsdx`) plus three tiny hand-written junk files
(`junk-text.bin`, `junk-zip-header.bin`, `junk-zeros.bin`). Same author and
license (P3-10, MIT).

The seed corpus of the `import` fuzz target
(`crates/vsd-binary/fuzz/corpus/import/`, task P8-10, ADR 0017 section 4)
holds byte-identical copies of the six committed `.vsd` fixtures above
(`minimal-drawing.vsd` and the five `malformed-*.vsd`) plus three tiny
hand-written junk files (`junk-text.bin`, `junk-visio-magic.bin`,
`junk-zeros.bin`). Same author and license (P8-10, MIT); `fixtures/` stays
the single source of truth and a copy that drifts is a bug.

All rows above are our own work: hand-written XML and a plain zip container
produced by `crates/vsdx/tests/common/mod.rs` (task P2-08), and a
hand-written OLE compound container with a `VisioDocument` stream produced
by `crates/vsd-binary/tests/common/cfb.rs` and `tests/malformed.rs` (task
P8-10). Element names, content types and relationship type URIs are
structural facts recorded in `docs/vsdx-inventory.md`; the MS-CFB and Visio
stream layouts are structural facts of the formats (MS-CFB, ADR 0068 and
ADR 0069); no third-party bytes are included. Tests
(`fixtures::committed_fixtures_match_the_builder_output` and
`malformed::committed_fixtures_match_the_builder_output`) prove every
committed file equals the builder output byte for byte.
