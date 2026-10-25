# Phase 8 clean-room and licence audit

- Task: P8-13, role `cleanroom-auditor`
- Diff audited: the Phase 8 merge commits `83100dd` (P8-02), `ea520f2`
  (P8-03), `13d6641` (P8-04), `55d3d00` (P8-05), `509c176` (P8-06),
  `c0ad967` (P8-08), `cb66c6c` (P8-07), `3c45d0a` (P8-09), `88f8ddb`
  (P8-10), `aadf7b6` (P8-15), `b2338c6` (P8-16): 125 files, the bulk in
  `crates/vsd-binary`
- Date: 2026-10-25, branch `agent/P8-13`

Method: `cargo deny check` in the worktree; every `crates/vsd-binary`
`.rs` file read for the MPL-2.0 header; `git ls-files` scanned for
binary and Microsoft-owned file types across the whole tree and
`git ls-files reference` checked empty; libvisio identifiers, struct
names and comment mentions grepped across `crates`, `tools` and `apps`;
every `#[path]` include in the tree checked for cross-crate MPL files;
the committed strings of the Phase 8 diff compared against the sources
in `reference/` (the corpus files themselves are data, not prose); the
long string literals of `crates/vsd-binary/src` listed and inspected;
`ATTRIBUTION.md`, `corpus/PROVENANCE.md`, `corpus/manifest.toml`,
`NOTICE` and `docs/sources.md` rows checked against the committed
files; `docs/reviews/wide-corpus.md`, `crates/vsd-binary/tests/wide_corpus.rs`,
`crates/vsdx/tests/wide_corpus.rs` and `xtask/src/scout.rs` checked for
file names and quoted content; and **all 15 goldens in
`crates/render/tests/golden/vsd/` opened and inspected image by image**.

## Verdict

**Pass with required fixes: 0 blocker, 1 major, 3 minor**, plus 2
informational notes. Licence confinement is clean: `cargo deny check`
is green, `larrygram-vsd-binary` is the only MPL-2.0 crate, every
source file in it carries the MPL-2.0 header, and no libvisio-derived
code exists outside it. The MS-CFB reader is written from the
specification. The one failing check is "no Microsoft content
committed": the golden `tdf76829-datetime-format-p1.png` renders a
Microsoft-template-derived sample drawing in full, including its
long-form sample text (F1). The minor findings are a `docs/sources.md`
row that contradicts the P8-16 Result (F2), an `ATTRIBUTION.md` row
that misstates the provenance of the `.vsd` goldens (F3) and an
unrecorded code-page table (F4).

The hand-off gate for this task is reported in the Result section
below; the task changes no code.

## Findings

| Id | Severity | Location | Rule | Suggested fix |
|---|---|---|---|---|
| F1 | major | `crates/render/tests/golden/vsd/tdf76829-datetime-format-p1.png`; its P8-10 predecessor `tdf76829-datetime-format.png` (full page, same content) is in the git history of merge `88f8ddb` | AGENTS.md 4: never commit Microsoft-owned content (templates, sample drawings) or long-form text; ADR 0005: no Microsoft-derived master content in git history | The golden renders the whole first page of `vsd/tdf76829-datetime-format.vsd`: a German product-launch timeline titled "Zeitplan der Produktveröffentlichung / Contoso Pharmaceuticals, Inc." with timeline callout shapes and a multi-line German sample disclaimer footer. "Contoso" is Microsoft's sample-company name and the drawing is built from a Visio stock template (`corpus/PROVENANCE.md` itself says many fetched files "embed master shapes that were created from Microsoft templates, which is why they are fetched, not committed"). A full-page raster of the drawing puts that content, including the sample text, into git history — the exact exposure ADR 0005 rejected for the drawing files themselves. Fix: drop this golden (assert on the imported model's numbers for this file instead, or render with text masked and only the trivial shapes visible), and remove the committed PNGs from history in the pending force-push window (STATUS: the remote still holds the pre-R4 history and is waiting for a force-push). The other 14 `.vsd` goldens show only trivial geometry and short field-format labels; they are fine. |
| F2 | minor | `docs/sources.md` row 26 ("nothing file-specific is committed") vs `docs/tasks/P8-16.md` Result | AGENTS.md 4: committed provenance must be accurate | The P8-16 Result lists the 32 gate-corpus proposals with repo, pinned commit, path, sha256, size and licence (its own task spec directs the proposals "in the Result"), so file-specific rows are committed in the task file. Reword row 26 to "nothing file-specific is committed except the Tier A gate-corpus proposal rows of `docs/tasks/P8-16.md`", or move those lists to `reference/` and keep the task file aggregate. |
| F3 | minor | `ATTRIBUTION.md`, the `crates/render/tests/golden/vsd/*.png` row | AGENTS.md 4: every committed asset row states the true source and licence | The row labels the goldens "MIT (project license), our own rendering" but the rendered drawings are libvisio test data (MPL-2.0) that `corpus/PROVENANCE.md` describes as often embedding masters created from Microsoft templates. After F1 is fixed, extend the row's note to say the goldens render the fetched corpus drawings and show only trivial geometry and short test labels (and treat the renders' licence as tied to the MPL-2.0 test data, not plain MIT). |
| F4 | minor | `crates/vsd-binary/src/container.rs:1117-1160` (`code_page_char`); `docs/sources.md` row 25 | AGENTS.md 4: every derived fact from a source is recorded | The 32-entry Windows-1252 `0x80..=0x9F` mapping is committed as Unicode escape facts. P8-02's open issue argues (correctly) that this is a standard code-page mapping, not prose; recording it costs one clause. Add "the Windows-1252 code-page mapping" to row 25's committed-derived-data cell (or note in row 25 that the mapping is the published Windows-1252/Unicode table). |

## Informational notes (no action required)

- `crates/geom/src/curve.rs:31` names libvisio in a doc comment
  ("libvisio pads the same way; that is corroboration only, not the
  source"). It is a mention, not derived code, and the comment cites
  [MS-VSDX] and the VBA reference as the sources; `NOTICE`'s statement
  ("no libvisio-derived code outside `crates/vsd-binary`") stays true.
- `crates/vsd-binary/fuzz/Cargo.toml` has no `license` field (the crate
  is `publish = false` and its `.rs` files carry the header). Harmless;
  adding `license = "MPL-2.0"` would make the confinement visible to
  tooling.

## Checklist

1. **`cargo deny check` passes; `larrygram-vsd-binary` is the only
   MPL-2.0 crate.** Pass. `cargo deny check` in the worktree ends
   "advisories ok, bans ok, licenses ok, sources ok". `deny.toml`
   keeps MPL-2.0 out of the permissive list with a single exception for
   the crate and `[licenses.private] ignore = false` checks the
   workspace crates too.
2. **Every `crates/vsd-binary` source file carries the MPL-2.0
   header.** Pass. All 29 tracked `.rs` files (including
   `fuzz/fuzz_targets/import.rs`) begin with the MPL-2.0 boilerplate;
   `crates/vsd-binary/Cargo.toml` declares `license = "MPL-2.0"`
   (see the informational note for `fuzz/Cargo.toml`).
3. **No libvisio-derived code outside `crates/vsd-binary`.** Pass.
   Greps across `crates`, `tools` and `apps` find only the
   `curve.rs:31` comment (informational) and test-local names like
   `LARRYGRAM_VSD_TWINS`. The decompressor is the one libvisio-derived
   algorithm and is declared as such in `container.rs`'s header
   ("derived from the libvisio reference; this file therefore carries
   the MPL-2.0 header"). `tools/libvisio-oracle/` holds only the build
   script and README (ADR 0010): the oracle binaries are built outside
   the repo and invoked by tests as subprocesses.
4. **The MS-CFB reader is written from the specification.** Pass.
   `container.rs` cites MS-CFB sections on the structures it reads
   (the signature 2.2, the sector kinds 2.3, the directory entry
   2.6.1), ADR 0068 records the decision, and `docs/sources.md` rows
   24 and 25 record [MS-CFB] and [MS-OLEPS]. The summary-stream
   decoder follows [MS-OLEPS] and maps property ids to our own
   property names.
5. **No Microsoft content or long-form text copied.** **Fail (F1)**;
   everything else passes. No Microsoft-owned file is committed: no
   spec PDF/DOCX, no `.vss`/`.vst`/`.vsd`/`.vsdx` except our own
   hand-authored fixtures, no Calibri or Segoe UI (Carlito and Selawik,
   OFL licences committed, rows present), no Office icons (Tabler,
   MIT, rows present). The committed strings of the Phase 8 diff are
   short and our own ("the binary Visio formats are read-only; save as
   {}", warning and test-skip messages); the only long literals in
   `vsd-binary/src` are the Windows-1252 escape table (F4) and short
   error messages. The failure is the single golden F1: a committed
   raster of a Microsoft-template-derived sample drawing including its
   sample text.
6. **`NOTICE` and `docs/sources.md` list what Phase 8 needs.** Pass
   with F2. `NOTICE` carries the libvisio attribution with commit
   `c7c970775b750a4bf7fd1b4364d69bac9815bfcb`; `docs/sources.md` has
   the [MS-CFB] row (24) and the [MS-OLEPS] row (25) that P8-02's open
   issue requests, and the wide-corpus row (26) P8-16 records. The
   tasks' open-issue lists are complete: no Phase 8 source is missing
   a row.
7. **No corpus or candidate file committed (ADR 0005).** Pass. The
   fetched files live in `corpus/vsd/` and `corpus/vsdx/` (gitignored,
   symlinked in the worktree), the wide corpus and the candidates in
   `reference/` (untracked; `git ls-files reference` is empty). The
   committed `.vsd`/`.vsdx` files under `corpus/fixtures/` and
   `crates/*/fuzz/corpus/` are our own builder output with
   `corpus/PROVENANCE.md` and `ATTRIBUTION.md` rows for every file
   (checked one by one). F1 is the one place where fetched-corpus
   *content* reaches git, as pixels.
8. **The wide corpus names or quotes no file.** Pass.
   `docs/reviews/wide-corpus.md` is aggregates only (counts by tier,
   kind, version, licence; failure and warning kinds; extension
   counts). The two sweep tests and `xtask/src/scout.rs` use synthetic
   names only (`a b/x.vsd`, `gpl.vsd`, `minimal-drawing.vsd`, our own
   fixture). The repo names of P8-16's gate-corpus proposals appear
   only in `docs/tasks/P8-16.md`, which its own task spec directs; no
   Tier B file is named anywhere.
9. **`crates/vsd-binary/tests/twins/compare.rs` is the only MPL file
   another crate compiles.** Pass. The tree has exactly one cross-crate
   `#[path]` include of a `vsd-binary` file:
   `crates/render/tests/corpus_vsd.rs:35`. `compare.rs` is our own
   format-agnostic model comparison code (it carries the MPL header
   because it lives in the crate). P8-17 moves it; until then this is
   the known single exception.
10. **Sources are in the allowed list and recorded.** Pass. [MS-CFB]
    and [MS-OLEPS] are Microsoft Open Specifications (AGENTS.md
    section 4), libvisio is allowed by section 5 with its commit
    recorded, and the wide corpus is recorded as source 26 with its
    licence tiers. Open policy item for the master (P8-16's open
    issues): 3 150 wide-corpus files are template-origin and were
    swept, not excluded; the sweeps stay local-only and commit
    aggregates, so nothing reaches git either way.

## Rows the master must add or fix

The rows the Phase 8 tasks request are **already present and
complete** (sources 24, 25, 26; the `NOTICE` attribution). The
remaining row edits:

1. **F1 fix**: drop `crates/render/tests/golden/vsd/tdf76829-datetime-format-p1.png`
   from the tree (and both it and the P8-10 full-page
   `tdf76829-datetime-format.png` from history in the pending
   force-push window), then adjust the `crates/render/tests/golden/vsd`
   and `corpus/fixtures`/goldens `ATTRIBUTION.md` rows accordingly.
2. **F2**: reword `docs/sources.md` row 26's "nothing file-specific is
   committed" clause (or move the P8-16 proposal lists to
   `reference/`).
3. **F3**: extend the `ATTRIBUTION.md` `crates/render/tests/golden/vsd/*.png`
   row's note with the corpus provenance and the trivial-content
   statement.
4. **F4** (optional): one clause for the Windows-1252 mapping in
   `docs/sources.md` row 25.
