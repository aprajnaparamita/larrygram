# Phase 8 review: binary `.vsd` import

- Task: P8-11 (reviewer), 2026-10-25
- Scope: everything Phase 8 merged into `main` through `3768d10` —
  `crates/vsd-binary` (all modules), the Phase 8 changes in `crates/model`,
  `crates/vsdx`, `crates/render`, `crates/commands`, `crates/ui` and
  `apps/larrygram-cli`, the parity harness and the test suites.
- Against: `docs/plans/phase-8.md`, ADRs 0068–0072, ADR 0002/0003/0013,
  AGENTS.md sections 5 and 6, and the task files P8-02 to P8-10, P8-15 and
  P8-16 with their Results.
- Method: static review of every module, the mechanical lint/header/layer
  checks, and two read-only probes through the crate's public API (a scratch
  project outside the repository): one counting chunk record kinds per corpus
  file and warning kinds per `import`, one listing the sections and direct
  cells of the imported model. The probe numbers below are reproducible with
  `import`, `read_chunks` and `Record` alone.

## Verdict

**Request changes.** Two blockers (one ADR 0070 rule violation with silent
data loss, one crash path), one major, five minor. The code quality is high —
the never-crash discipline in the container, chunk and text layers is real,
the caps are checked before allocation, and the warnings that are emitted are
accurate — but three rows of the ADR 0070 mapping table are not implemented at
all and, worse, are dropped **silently**, and the new scale pass recurses over
the shape tree without a depth bound. All fixes are small and belong in P8-14
(plus one P8-17 item); nothing needs the architecture revisited.

## Findings

### F1 — blocker: three ADR 0070 rows are unmapped and dropped silently

- `crates/vsd-binary/src/shape.rs:214` (`KNOWN`), `crates/vsd-binary/src/shape.rs:1024`
  (`skip_other`), `crates/vsd-binary/src/pages.rs:323` (the `PROP_LIST` arm).

ADR 0070 maps "shape data, user-defined cells, custom properties", "layers and
layer membership" and "connection points and controls" onto `Property`/`User.*`
sections, `Layers` section rows and `Connections`/`Controls` section rows, and
states "Nothing is dropped silently". None of the three rows is implemented
anywhere in the crate (the only sections the import builds are `Geometry`,
`Character`, `Paragraph`, `Tabs` and the style sheets' own sections), and the
records are swallowed without a warning: `skip_other` warns only for the
`DROPPED` list and for record kinds outside `KNOWN`, and every record kind of
these rows sits in `KNOWN` (they were assumed to be "another domain module's to
report" — no module reports them). `pages.rs:323` skips `PROP_LIST` in an
explicit silent arm.

Evidence (probe over the five corpus files of `corpus/manifest.toml`, record
counts from `read_chunks`; the imported model has zero rows of these kinds):

| record | name | occurrences | warnings emitted |
|---|---|---|---|
| `0xb4` | `USER_DEFINED_CELLS` | 2 127 | 1 (a page-level skip) |
| `0x68` | `PROP_LIST` | 893 | 0 |
| `0xb6` | `CUSTOM_PROPS` | 262 | 0 |
| `0x6d` | `CUST_PROPS_LIST` | 59 | 0 |
| `0xa7` | `LAYER_MEMBERSHIP` | 205 | 0 |
| `0xa8` | `LAYER` | 28 | 2 (page-level skips) |
| `0x6f` | `LAYER_LIST` | 18 | 2 (page-level skips) |
| `0x99` | `CONNECTION_POINTS` | 206 | 0 |
| `0x71` | `C_PNTS_LIST` | 44 | 0 |
| `0xaa` | `CONTROL` | 165 | 0 |
| `0x70` | `CTRL_LIST` | 94 | 0 |

The same table shows other silent shape-scope drops the mapping table does not
even name: `SCRATCH_LIST`/`SCRATCH` (518), `PROTECTION` (154), `MISC` (300),
`STYLE_PROPS` (240), `GROUP` (58), `LAYOUT` (61), `IMAGE` (26).

Impact: a `.vsd` opened and saved as `.vsdx` loses every custom property, every
shape-data row, every user cell, all layer membership and all connection points
and controls, and the user is told nothing. Custom properties and shape data are
user-authored content; this is data loss, and it also explains the wide-corpus
sweep's "0 groups and 0 connects from binary import" (P8-16).

What to do:

1. Map the three rows (the payloads are the same row records the character and
   paragraph rows already parse; the section-row machinery in `text.rs` is the
   template), **or** drop them with a `Dropped` warning each and file the three
   mapping rows as Open issues in the phase report. The ADR allows either; it
   does not allow silence.
2. Either way, shrink `KNOWN` in `shape.rs` to the record kinds a module really
   consumes, so an unmapped kind warns by default. The comment above the list
   ("every other record kind ... belongs to another domain module") is the
   source of the bug and should be rewritten.
3. Close the silent `PROP_LIST` arm in `pages.rs:323` the same way.

### F2 — blocker: unbounded recursion over the shape tree in the scale pass

- `crates/vsd-binary/src/scale.rs:140` (`scale_tree`), reachable from
  `crates/vsd-binary/src/scale.rs:72` (`scale_document`), which the pipeline
  runs on every import (`crates/vsd-binary/src/pipeline.rs:64`).

`scale_tree` recurses once per shape of a parent chain. The chain length is
capped only by `Limits::max_shapes` (100 000 by default): `regroup`
(`crates/vsd-binary/src/shape.rs:1138`) resolves each shape's parent field to an
earlier arena entry, so a cycle is impossible, but a file whose shape *i* names
shape *i-1* as its parent yields a tree 100 000 deep. At a few hundred bytes of
stack per frame that overflows the stack — a crash on hostile input, against
AGENTS.md section 6 ("bound recursion") and the never-crash rule of ADR 0071.
The fuzz target will not find it (it needs a well-formed deep file) and the
rest of the workspace already knows the hazard: `crates/render/src/build.rs`
walks the same tree with a `depth` parameter and a repeat guard.

What to do: walk the tree iteratively, or pass a depth counter and stop with a
`Dropped`/`BadValue` warning past a bound (`Limits::max_chunk_level` is the
natural one; the level-based nesting in `shape.rs` is already bounded by it).
Whatever bound is chosen, add the regression test: a hand-built document with a
deep parent chain must not overflow.

### F3 — major: quick-style and theme colours are dropped without a warning

- `crates/vsd-binary/src/style.rs` (the colour mapping, ~lines 300–360): the
  `qsFillColour`/`qsLineColour`/`qsShadowColour` fields and the document theme
  are not read at all (no occurrence of either in the crate).

ADR 0070's colour row is honoured only for colour-table indices and direct
RGBA. A `.vsd` whose colour comes from a quick-style index resolves through the
document theme (P8-10 open issue 1: `dwg.vsd` stores white and the oracle draws
`#5b9bd5`); such a shape imports with the wrong colour and no diagnostic. The
task Results document the gap (P8-10 open issue 1, P8-15 open issue 4) but the
runtime behaviour is a silent wrong value, which the ADR's "no silent
difference" rule forbids.

What to do: either map the quick-style fields (the model already has
`CellValue::Str("Themed")` and `Document::themes` from the `.vsdx` side), or
emit one `Unsupported`/`Dropped` warning per shape that carries a quick-style
colour the import cannot resolve. The mapping decision is ADR-level: if it
needs a new ADR or an amendment of ADR 0070, say so in the fix task.

### F4 — minor: a field's display text is fabricated with no warning

- `crates/vsd-binary/src/text.rs:988` and `crates/vsd-binary/src/text.rs:1128`
  ("the display text of a date field is our own ... patterns are our own
  English spelling").

The binary records carry a field's value and format number but not its display
text, so the importer spells the date, time and number formats with our own
English patterns. That text is invented, locale-blind (Visio formats dates by
the system locale) and enters the saved `.vsdx` as if the file carried it. It
is documented (P8-05 deviation 3, P8-15 open issue 3) and P8-17 works on it,
but nothing warns, so the parity report's `text` mismatches and the saved text
are the only trace.

What to do: when the display text is spelled by our own pattern, emit an
`Unsupported` warning (like the empty feet-and-inches and fraction formats
already do) so the degradation is visible per shape; P8-17 can then replace the
patterns or record them as accepted.

### F5 — minor: `for` over `Option` with an allowed lint hides the miss case

- `crates/vsd-binary/src/text.rs:277` and `crates/vsd-binary/src/text.rs:284`
  (`#[allow(for_loops_over_fallibles)]`).

The two loops iterate `shapes.get_mut(owner)` and `document.shape_mut(*id)` to
avoid an `else` branch. The allow defeats a workspace lint for stylistic
reasons (P8-05 flagged it for this review), and when the lookup misses the
shape's text is dropped with no warning — the same silence rule as F1.

What to do: use `match`/`if let` and warn (`UnresolvedReference`/`Dropped`) on
the miss; drop the two `#[allow]` lines.

### F6 — minor: a non-`Guide` `Type` attribute no longer round-trips

- `crates/vsdx/src/read/mod.rs:627` (the `Type` arm), `crates/vsdx/src/write/emit.rs:343`.

Before P8-15 the `Type` attribute fell into the catch-all and was preserved
verbatim (ADR 0012); now the reader maps `Type="Guide"` and **discards** every
other value (`Group`, `Shape`, ...), and the writer emits `Type` only for
guides. A `.vsdx` shape carrying `Type="Group"` therefore loses the attribute
on save. None of the corpus `.vsdx` files or the 200-file wide-corpus sample
carries a shape `Type` at all, so the blast radius is small, but it is a silent
loss against the round-trip guarantee.

What to do: keep preserving non-guide values (push the attribute to
`preserved` when `is_guide` is false), or map the full attribute vocabulary.

### F7 — minor: the "matches the vsdx reader" tests pin literals, not the reader

- `crates/vsd-binary/src/shape.rs:3714`
  (`the_mapped_fields_match_the_vsdx_reader`),
  `crates/vsd-binary/src/style.rs:1792`
  (`the_mapped_style_matches_the_vsdx_reader_spelling`), and the two text
  comparison tests.

The names claim agreement with `crates/vsdx`, but the tests compare against
literal expectations written out by hand (the layer rule forbids the dev
dependency; the P8-04 result documents this). The literals can drift from the
real reader unnoticed. The live cross-reader comparison now exists (P8-15's
`twin` tests in `crates/render/tests/corpus_vsd.rs`), so the risk is covered
when the twins arrive.

What to do: rename the tests to `..._matches_the_pinned_vsdx_spelling` (or add
a comment pointing at the twin runner) so the claim and the mechanism agree.

### F8 — minor: user-facing Debug text and the stub error variant

- `crates/commands/src/builtins/file.rs:265` (`other => format!("{other:?}")`),
  `crates/vsd-binary/src/error.rs:40` (`Error::NotImplemented`).

The status-line formatter falls back to the `Debug` spelling of `WarningKind`
for future variants, and `ImportWarning`'s own `Display` prints the kind as
`Debug` too (P8-09's open issue). And `Error::NotImplemented` survives although
nothing constructs it; ADR 0071 says the last Phase 8 task removes it.

What to do: give `WarningKind` a `Display` in `crates/vsd-binary` and use it in
both places; remove `NotImplemented` and its tests in P8-14 (it is already on
that task's list).

## ADR 0070 mapping table, row by row

| Row | Status |
|---|---|
| pages, page sheets, page properties | honoured (`pages.rs`) |
| background-page references | honoured, resolved or warned |
| connects | **not honoured** — `Page::connects` stays empty; connect lists warn `SkippedRecord` (honest, documented; no source spells the entry payload) |
| stencils, masters, name lists | honoured |
| shape groups, shapes, guide shapes, shape ids | honoured (P8-15's id and parent joins) |
| transforms | honoured |
| geometry rows | honoured with a known fidelity gap (duplicated rows of one list, P8-15 open issue 2) |
| line, fill, shadow, text block | honoured since P8-15 |
| character, paragraph, field, text | honoured; field display text fabricated (F4) |
| style sheets, style properties | honoured |
| the colour table | honoured |
| **layers and layer membership** | **not honoured, silent (F1)** |
| **shape data, user cells, custom properties** | **not honoured, silent (F1)** |
| **connection points and controls** | **not honoured, silent (F1)** |
| document properties and kind | honoured except the creation date and the custom-property section (documented open issues); `DocumentKind::Template` is never produced (documented) |

## What passes

- **Never crash, outside F2.** No `unwrap`/`expect`/`panic!` outside test code
  (the workspace lints and `clippy.toml` cover this; the only occurrences are in
  `#[cfg(test)]` modules), `#![forbid(unsafe_code)]` and
  `#![deny(clippy::indexing_slicing)]` in the crate root, every file of
  `crates/vsd-binary` carries the MPL-2.0 header. The container's chain walks
  are cycle-safe and bounded, the chunk walk bounds pointer-list nesting at 256
  and record nesting at `max_chunk_level`, and the record loops in `shape.rs`,
  `text.rs` and `pages.rs` terminate on payload exhaustion (`Fields`/`Cursor`
  return `Option`, and the length-jumping loops advance by `checked_add`).
  `decompress` caps output before each push; `with_capacity` calls in
  `container.rs` follow a cap check.
- **Limits.** Every cap of `Limits` is enforced at a boundary
  (`max_streams`, `max_stream_bytes`, `max_total_uncompressed`,
  `max_compression_ratio`, `max_chunks`, `max_chunk_level`, `max_shapes`,
  `max_text_bytes`); `tests/import_smoke.rs` asserts the caps fire.
- **Seams.** `Chunk` kept its fields, `Record` its opaque newtype, `import` its
  signature, and the frozen `collect` functions still exist beside the
  `collect_with` extensions. The one signature change (`read_chunks` returning
  its warnings and taking a block store) is documented as a deviation with the
  reason. Acceptable.
- **Layering and licensing.** `vsd-binary` is L2 and depends only on
  `larrygram-geom` and `larrygram-model`; `commands` (L4) and `render` (L3,
  dev-only) may depend on it; `cargo xtask layers` and `cargo deny` are green
  in every recorded gate. The `unicode-normalization` dev-dependency is
  permissive and recorded.
- **Warnings that are emitted are honest.** The dropped list of ADR 0070 (OLE,
  hyperlinks, events, annotations, smart tags, the font list) warns with clear
  kinds; the font-record double reporting (P8-06 vs P8-05) reports two distinct
  facts and is fine; page-level unmapped records warn with their record names;
  errors name the place and never the content.
- **Tests are real.** The malformed fixtures are hand-authored, committed with
  provenance rows and pinned byte-for-byte against their builders; the
  truncation and byte-flip sweeps exist; the fuzz target drives `import` plus
  the model lookup and geometry with 25 000 runs clean; the corpus smoke tests
  skip cleanly without the corpus and assert content, not just non-panic; the
  parity harness follows ADR 0072 (tolerances unchanged, mismatches listed as
  `place / field / oracle value / our value`, "not compared" lists published).
  The P8-15 rule corrections to the harness fixed oracle-side parsing errors
  and are documented; they did not loosen a tolerance.
- **Cross-crate changes are sound.** The repeated-cell value policy fix
  (`crates/model/src/values.rs`) is narrowly scoped and covered by a regression
  test; the `Type="Guide"` chain (model, vsdx, render) is coherent apart from
  F6; the field-display-text write-back in `crates/model/src/mutate.rs` matches
  the reader's convention; the `Colors` element generation in
  `crates/vsdx/src/write/emit.rs` only fills what the preserved content lacks;
  the `file.open` routing and the read-only save refusal in
  `crates/commands/src/builtins/file.rs` are correct and tested, and every
  import warning reaches the UI and the CLI.

## Notes for the master

- The fix list is small: F1 and F2 are the only must-fix items; F3 needs an
  ADR-level decision (map the quick-style fields or warn); F4 belongs with
  P8-17's field-text work but its warning can land in P8-14; F5 to F8 are
  minutes each.
- P8-14 should also remember the ADR 0071 promise to remove
  `Error::NotImplemented` (F8) and the open-issue pile the Results already
  carry (creation date, summary code page and custom properties, the template
  marker, the document-scope shape, `NoLine`/`NoFill` in the renderer).
- The parity numbers in the task Results are reproducible and honestly
  reported; the phase report should carry the "not compared" lists beside them.
