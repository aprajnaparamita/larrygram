# The wide corpus (P8-16)

Aggregates only (task P8-16, master note "Privacy"): this report names
no file. Every number comes from the local CSVs under
`reference/wide-corpus/` (never committed, ADR 0005); the sweeps record
per-file outcomes there, and the report counts them.

Sources: the BigQuery export over the public GitHub dataset
(`bigquery-public-data.github_repos`, `files` joined with `licenses`),
14 873 rows for paths ending in `.vsd`, `.vsdx`, `.vss`, `.vssx`,
`.vst`, `.vstx` or `.vdx` (see `docs/sources.md` source 26). The rows
carry 6 254 distinct git blobs.

## Fetch

`cargo xtask scout fetch` downloads each distinct blob once into
`reference/wide-corpus/<tier>/<blob_id>.<ext>`, verifies the git blob
SHA-1 against the export, and pins each kept file to a commit with
`git ls-remote`. The run is resumable (`fetch.csv` is append-only).

| Outcome | Files |
|---|---|
| Kept (verified and pinned) | 5 238 |
| Dropped: HTTP 404 (the branch moved on) | 1 003 |
| Dropped: the blob id did not verify | 9 |
| Dropped: over the 20 MB cap | 4 |
| Kept but unpinned (`git ls-remote` failed) | 15 |

Of the kept files, 3 457 are Tier A (permissive licences) and 1 781
Tier B (GPL family: local sweeps only, never named). Disk use:
1.1 GB.

## Classification

`cargo xtask scout classify` writes `index.csv` (one row per kept
file) and `excluded.csv`. The OLE facts of `.vsd`, `.vss` and `.vst`
files come from `ole-facts.csv`, written by the sweep tests through
`larrygram-vsd-binary`'s public API (2 936 OLE files).

**By kind** (5 075 non-excluded files):

| Kind | Files |
|---|---|
| `ole-drawing` (OLE with a `VisioDocument` stream) | 2 784 |
| `ooxml-drawing` (zip with `visio/document.xml`) | 1 977 |
| `vdx` (XML drawing; classify only, out of the alpha scope) | 152 |
| `ole-invalid` (not a readable compound file) | 100 |
| `ole-unreadable` (container ok, the import failed) | 41 |
| `ooxml-stencil` | 10 |
| `ooxml-template` | 6 |
| `other` / `zip-invalid` | 5 |

**By extension:** `.vsd` 2 844, `.vsdx` 1 982, `.vdx` 152, `.vst` 60,
`.vss` 21, `.vssx` 10, `.vstx` 6.

**By version byte** (2 925 OLE files): version 11: 2 519, version 6:
268, version 5: 26, version 4: 9, version 3: 2, unreadable: 101.
Versions 3 to 5 are out of the alpha scope (ADR 0069).

**By tier:** Tier A 3 339, Tier B 1 736.

**By licence:** MIT 1 751, Apache-2.0 1 025, GPL-3.0 788, GPL-2.0 701,
BSD-3-Clause 247, LGPL-3.0 146, Unlicense 89, LGPL-2.1 62, MPL-2.0 59,
CC0-1.0 56, BSD-2-Clause 47, EPL-1.0 44, AGPL-3.0 39, Artistic-2.0 20,
ISC 1.

**Stencil/template reality:** the extension and the real kind agree on
4 905 files and disagree on 27 (mostly `.vss`/`.vst` files the binary
importer reports as drawings: the OLE template marker is not mapped
yet, P8-07's open issue).

## The Microsoft-content filter

Applied before any test reads a file (AGENTS.md section 4). 163 files
are marked and excluded; no test, sweep or export touches them.

| Reason | Files |
|---|---|
| A document property names Microsoft as the author | 61 |
| A document property names Microsoft as the company | 102 |
| The file is a copy of a Visio stock template or stencil | 0 |

150 of the excluded files are OPC drawings, 11 binary files and 2 OPC
stencils.

**Template origin, counted but not excluded:** 3 150 of the 5 075
files were made from a Visio stock template (the `Template` property
names `BASICD_M.VSTX`, `BASFLO_M.VSTX`, `USEQME_M.VSTX` and friends,
usually as a full Office path). Such drawings may embed master shapes
copied from a Microsoft template; the project's precedent (ADR 0005)
is to use those files in tests and never commit them, so the filter
records the flag and the sweeps run. Anything stricter is the master's
call.

No file in the corpus is a bare copy of a stock template or stencil
under the name rule (short code plus `_M`/`_U`, or the known names
`BASICD_M`, `BASFLO_M`, `XFUNC_M`). Visio *sample* drawings are only
caught through the Microsoft properties; a name-based sample rule is
an open gap.

## The sweeps

Both sweeps ran over every non-excluded file (5 075), each under
`catch_unwind`: a panic is a bug and would be listed here.

| Run | Opened | Errors | Panics |
|---|---|---|---|
| `import` (`larrygram-vsd-binary`) | 2 784 | 2 291 | 0 |
| `.vsdx` reader (`larrygram-vsdx`) | 1 899 | 3 176 | 0 |

Each reader also met the other family's files: those fail cleanly
(`NotAVisioFile` for the binary importer, `InvalidPackage` for the
package reader), which is the intended outcome.

**Import errors by kind** (2 291):

| Error | Files | Note |
|---|---|---|
| `NotAVisioFile` | 2 239 | OOXML, `.vdx` and non-Visio OLE files, plus 89 binary-family files without a readable `VisioDocument` stream |
| `UnsupportedVersion` | 37 | version 5: 26, version 4: 9, version 3: 2 (out of scope, ADR 0069) |
| `Invalid` | 14 | 12 without a readable version byte (damaged containers), 2 in scope (versions 6 and 11) |
| `LimitExceeded` | 1 | a size cap on a version 6 drawing |

**Reader errors by kind** (3 176): `InvalidPackage` 3 168,
`MissingPart` 8. Of the `InvalidPackage` failures, 3 069 are binary or
`.vdx` files (expected), 99 are OPC packages: 94 of those look like
real Visio packages in `[Content_Types].xml` and are reader-rejection
candidates for a fix task (the per-file rows are in the local
`reference/wide-corpus/sweep-vsdx.csv`).

**Warnings** (the `.vsdx` reader has no warning stream yet; the binary
importer recorded 1 550 073 warnings across all 2 784 opened files):

| Kind | Warnings |
|---|---|
| `skipped_record` | 901 297 |
| `dropped` | 598 330 |
| `bad_value` | 46 672 |
| `unsupported` | 3 055 |
| `unresolved_reference` | 719 |

The ten most common warning kinds (by the record's `what`):

| What | Warnings |
|---|---|
| unknown record | 699 388 |
| event | 282 785 |
| action id | 90 346 |
| action id list | 75 343 |
| style record | 74 421 |
| document font | 60 041 |
| master page sheet | 37 342 |
| PRINT_PROPS | 29 020 |
| shape id | 21 757 |
| unit | 19 481 |

Every opened binary file carried at least one warning; the long tail of
record kinds the importer skips is the reason.

## What the corpus shows about import fidelity

Counts only (no file names):

- **Groups:** the binary importer produced 0 group shapes from 2 784
  opened files, while the `.vsdx` reader produced 1 202 from 1 899.
  Group nesting on the binary side is P8-10's open issue, visible now
  at scale.
- **Connect records:** 0 from binary files (P8-07: connects unmapped),
  1 522 of the opened OPC files carry connect records. 2 602 binary
  files do carry 1-D shapes, which is the connector proxy for `.vsd`.
- **Template marker:** no OLE file is recognised as a stencil or
  template, though 24 opened `.vss`/`.vst` files exist (P8-07's open
  issue).
- **Text:** 2 718 of 2 784 opened binary files carry text; 569 are
  multi-page.
- **Themes:** 1 430 of the opened OPC files carry theme parts.

## The proposal lists

In the P8-16 Result section (not here): about 30 Tier A files for the
gate corpus with full manifest rows, and about 200 Tier A files in
`reference/wide-corpus/export-set.csv` for a reference export run.
Both were chosen for coverage (versions 6 and 11, drawing scales,
groups, connectors, text-heavy drawings, multi-page drawings, real
`.vss`/`.vst` files, `.vsdx` files with themes) by the local
`reference/wide-corpus/select.py`, which records the same rules as
this section; only opened files were eligible, and gate-corpus files
are capped at 2 MB.

## Reproducing

```sh
cargo xtask scout fetch reference/bq-github-visio-2026-10-25.csv
LARRYGRAM_WIDE_CORPUS=reference/wide-corpus \
  cargo test -p larrygram-vsd-binary --test wide_corpus -- --ignored wide_corpus_classify_ole
cargo xtask scout classify
LARRYGRAM_WIDE_CORPUS=reference/wide-corpus \
  cargo test -p larrygram-vsd-binary --test wide_corpus -- --ignored wide_corpus_import_sweep
LARRYGRAM_WIDE_CORPUS=reference/wide-corpus \
  cargo test -p larrygram-vsdx --test wide_corpus -- --ignored wide_corpus_read_sweep
```
