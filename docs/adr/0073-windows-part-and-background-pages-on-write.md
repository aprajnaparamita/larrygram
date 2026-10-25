# 0073: The windows part and background pages on write

- Status: accepted
- Date: 2026-10-25
- Task: P8-20
- Amends: 0018 (the writer)

## Context

Two facts about the `.vsdx` format are outside the round-trip machinery of
ADR 0018, and both decide whether a file Larrygram writes opens in Visio at
all.

**The windows part.** [MS-VSDX] does not describe it: section 2.3.1 (Part
Enumeration) lists the parts of a web drawing and states that all other
parts are unused and must be ignored, and no `Windows` element appears in
its schema. The Microsoft Learn Visio XML schema reference documents the
`Windows` element and the `Windows_Type` and `Window_Type` complex types
(the structural rows in `docs/vsdx-inventory.md`), and every Visio-produced
package in the corpus carries the part. Observed behaviour of Visio (AGENTS.md
section 4): a `.vsdx` package does not open without it. The smallest form
Visio accepts is three entries, all required together:

1. a part `/visio/windows.xml` whose root is `Windows` in the namespace
   `http://schemas.microsoft.com/office/visio/2012/main` (an empty element
   is enough; no `Window` child is needed);
2. a relationship from the document part to it, type
   `http://schemas.microsoft.com/visio/2010/relationships/windows`;
3. an `Override` for `/visio/windows.xml` with content type
   `application/vnd.ms-visio.windows+xml`.

Style sheets, document properties, a theme and masters are not needed. None
of the eleven `corpus/fixtures/render-*.vsdx` files had the part before this
task, and the writer had no notion of it, so a document created from
nothing, or an imported `.vsd` saved as `.vsdx`, most likely did not open.

**Background pages.** [MS-VSDX] `Page_Type` (2.3.4.2.65) gives `Background`
(xsd:boolean) and `BackPage` (xsd:unsignedInt) and states the rule: `BackPage`
must equal the `ID` of a `Page_Type` element whose `Background` value is
true. Sections 2.2.2.4 and 2.2.2.5 say the same in prose: a page is a
background page when its `Background` attribute is one, and the `BackPage`
attribute names the background page of the current page. Observed behaviour
of Visio (AGENTS.md section 4): a page referenced through `BackPage` but not
marked `Background="1"` is an ordinary page and nothing is composited behind
the referencing page. `render-background-pages.vsdx` had exactly that
inconsistency, while our renderer composited the page regardless (the
renderer's mark handling is not part of this task; see Open issues).

## Decision

1. **Every written package carries the windows triple.** `ensure_windows_part`
   (`crates/vsdx/src/write/mod.rs`) runs in both writers (patch and
   generate). A package with no windows part — none named
   `/visio/windows.xml` and none with the windows content type — gets the
   three entries: an empty `Windows` root generated like any other Visio
   part, a relationship from the main part with the next free relationship
   id of that part, and the `Override`. A package that already has a windows
   part keeps it exactly as it is: only a missing relationship or a missing
   `Override` is added, and the part's own bytes are never rewritten.

2. **The writer marks what it references ("mark it").** After the pages list
   is written, `ensure_background_marks` gives every page a `BackPage`
   attribute names a `Background` attribute of `1`, flipping a false spelling
   and keeping a true one. [MS-VSDX] makes the pair mandatory, so a
   reference to an unmarked page is an invalid package that Visio silently
   misdraws; "warn and leave it" would ship exactly that, and the writer has
   no warning stream to warn through. A repeated `ID` is marked at its first
   element, the same page the reader resolves the reference to.

3. **The reader consumes the derived mark.** `Background` on a page another
   page references is no longer kept in `Preserved`: the model holds the
   fact as the reference and the writer writes the mark again, so keeping it
   would make `from_model -> to_model` unequal (`tests/properties.rs`). On a
   page nobody references the attribute stays preserved: an orphan mark
   ("this page may be used as a background") is content the model cannot
   derive, and it round-trips untouched.

4. **The round-trip criterion grows one tolerance.** `read -> write -> read`
   may add the windows entries to a source that had none (the writer's
   purpose is that the file opens); everything else of the Phase 2 criterion
   stands, and `write -> read -> write` stays byte-identical. The criterion
   assertions in `tests/roundtrip.rs` and `tests/package_variants.rs`
   compare the packages without the added entries and pin everything else.

## Consequences

- Every file Larrygram writes opens in Visio: new documents, edited files,
  saved `.vsd` imports and the fixtures the test builders produce. The
  structural test `crates/vsdx/tests/package_completeness.rs` (and its
  `crates/commands/tests/` twin for the `.vsd` import case) asserts the
  triple and the mark rule on every package the test suite writes.
- A source without a windows part, or with a `BackPage` that names an
  unmarked page, changes on save. That is the decision: the source was not a
  package Visio opens, and the writer fixes it. A source that is already
  consistent changes nowhere: the windows part is byte-for-byte kept and a
  true `Background` spelling (`1`, `true`) is left alone.
- `Background` is writer-owned on a referenced page: the attribute is always
  spelled `1` there unless the source spelled it `true`, and a model that
  preserved the attribute on a page that is also referenced loses it (the
  writer re-derives it). `Page_Type` `Background` on an unreferenced page is
  ordinary preserved content.
- The deflated-vs-stored decision of the package writer (ADR 0017) now
  measures the zip writer's own deflate where it lands, so each part is
  deflated once (task P8-20; before, each part was deflated twice, once to
  measure and once to write) and only an entry over the cap is rewritten,
  stored.

## Alternatives considered

- **Warn and leave an unmarked reference alone.** Rejected: [MS-VSDX]
  requires the mark, Visio drops the compositing without it, and the writer
  has no warning stream. The fixture rule in the task (goal 3) would also be
  unreachable for new documents and `.vsd` imports, which carry only the
  reference.
- **Give `Page` a `Background` flag in the model.** The fully lossless
  spelling: the reader maps the attribute, the writer writes it back from
  the model. Rejected for this task: it is a model change (outside this
  task's ownership), it moves the renderer's mark handling with it, and the
  writer-level rule needs no model change at all. Recorded as an open issue.
- **Always regenerate the windows part.** Rejected: it would rewrite a part
  the source wrote (Visio keeps view state in it), against the preservation
  rule of ADR 0012.
- **Emit no `Background` and keep the mark preserved.** Rejected: it leaves
  `BackPage` references Visio ignores, and the model round trip breaks
  whichever side of the pair the writer owns.
