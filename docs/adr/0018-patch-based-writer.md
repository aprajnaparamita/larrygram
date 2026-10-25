# 0018: The writer patches the source trees

- Status: accepted
- Date: 2026-10-14
- Task: P2-07

## Context

The reader maps only the `supported` surface of `docs/vsdx-inventory.md`; the
rest is preserved (ADR 0012). The round-trip criterion is *semantic*
equivalence (`diff` empty per XML part), but the corpus interleaves unmapped
and mapped children in ways a regenerating writer cannot reproduce:

- `PageSheet` children are `Cell*8, Trigger, Cell*…, Section*` in 9 corpus
  files: an unmapped `Trigger` sits *between* mapped cells;
- `VisioDocument` children are `DocumentSettings, Colors, FaceNames,
  StyleSheets, DocumentSheet`: the stylesheet wrapper sits in the middle of
  unmapped siblings;
- `docProps/app.xml` has `HeadingPairs` and `TitlesOfParts` between simple
  properties;
- several source elements merge into one `Preserved` fragment (a page and its
  page sheet and its page-contents root all merge into `Page::preserved`, P2-06
  open issues 1 and 7).

A writer that emits "mapped content, then preserved content" loses those
orders. The model would have to grow position anchors on every preserved
fragment to make regeneration faithful.

## Decision

**`write_document(document, preserve)` patches the supported parts of the
preserved package in place.** Each part's source tree is walked and updated
from the model:

1. **Every element that maps to a model entity is updated from that entity.**
   Only the mapped attributes are written; an attribute the model does not
   carry stays exactly where it is.
2. **Every unmapped element and attribute stays where the source put it.**
   The tree, not the `Preserved` fragment, is the authoritative record of
   positions (the fragments merge across source elements, so they cannot be
   routed back).
3. **A source element whose entity is gone from the model is removed**, unless
   an identical node is still in the entity's `Preserved` fragment (that is
   where the reader keeps the elements it does not map: an unnamed `Cell`, a
   `Cell` with a comment, a `Connect` with loose content).
4. **A new model entity is inserted after the last sibling of the same kind**,
   or before the first sibling of a later kind where the [MS-VSDX] sequence
   puts it. The ranks come from the schema: `Cell` before `Section` before
   `Text` before `Shapes` in a sheet element, `Cell` before `Row` in a
   section, `Shapes` before `Connects` in a contents part, and the
   `VisioDocument_Type` sequence for the main part's wrappers.

`from_model(document)` (no source package) generates the parts from the model
alone: mapped content first, each entity's `Preserved` content after it. It is
the writer for new documents and is **not** part of the round-trip criterion.

### Matching keys

Source elements are matched to entities by identity, never by position:

| Entity | Key |
|---|---|
| shape, page, master | `ID` (Visio id) |
| style | `ID` (stylesheet id) |
| section | `N`, and `IX` when either side has one |
| row | `IX` when present, else `N`, else the next unkeyed row |
| cell | `N` |
| property | name and value first, then name alone |
| connect | the attribute tuple (`FromSheet`, `FromCell`, …) |

An element without its key attribute is unkeyed and claims the next unkeyed
entity of its kind. Claiming is first-unmatched in list order, so duplicate
keys still pair up.

### Attribute writes

An attribute is rewritten only when it no longer matches the model, and a
matching spelling is kept (`2.0` stays `2.0`, `true` stays `true`, `007` stays
`007`): an unedited write changes nothing, and a second write is byte-identical
to the first. A reference attribute (`Master`, `BackPage`) the reader could not
resolve lives in `Preserved`, so `None` removes it only when the fragment does
not hold it. A cell whose cached value is an error writes `E` and keeps its
preserved `V` and `U`. A new `Rel`, relationship and contents part appear when
a page or master gains content the existing parts cannot hold; part names and
relationship ids come from the preserved `Rel` where there is one.

`[Content_Types].xml` and the relationships come from `Package::content_types`
and `Package::relationships`, refreshed for the parts actually written:
every part gets or keeps a declaration, and the main part's content type
follows `Document::kind` (ADR 0016) when the source declared a main content
type at all. A main part the source declared generically (`application/xml`,
the inferred spelling of a package without `[Content_Types].xml`) is left
alone, because the round-trip criterion compares the declarations.

### How this serves Phase 4 edits

A Phase 4 command mutates the model and calls `write_document` again:

- a changed cell, row, section, shape, page, master or style rewrites only its
  own mapped attributes, at its own place in the tree;
- a deleted entity removes its element and nothing else (unmapped siblings,
  whitespace and all, keep their positions);
- a new entity lands after its siblings of the same kind, schema-order-safe;
- unmapped content (`Trigger`, `Text`, `DocumentSettings`, `HeadingPairs`, …)
  is never disturbed, whatever the edit.

The model's `Preserved` fragments are the reader's record; the patcher
consults them only to tell "unmapped" from "deleted" (rule 3) and to find a
`Rel` id. Editing the fragments themselves is not round-tripped: a fragment
does not say which of the merged source elements a child came from (P2-06
open issue 7), so the tree wins.

## Consequences

- `write -> read -> write` is byte-identical (writer stability) and
  `read -> write -> read` is `diff`-empty on every corpus file: the exit
  criterion of Phase 2 (`tests/roundtrip.rs`).
- The writer never reorders or re-spells what the model did not change, which
  keeps `diff` output small and reviewable.
- New parts (a page contents part for a page that gained shapes) are
  generated; deleting an entity does **not** delete its parts and
  relationships, which stay as harmless unreferenced content until a later
  task prunes them (recorded as an open issue).
- A `Property` records no source part: a property the model adds is written
  into the main part's `DocumentProperties`, with the namespace spelling of a
  sibling property.
- Two writers worth of code: the patcher (`write/patch.rs`) and the generator
  (`write/emit.rs`), sharing one value encoder (`write/value.rs`).

## Alternatives considered

- **Regenerate with position anchors on `Preserved`.** The model would record
  where each unmapped fragment sat relative to the mapped children. Rejected:
  it grows the model for every entity, and the merged fragments still cannot
  say which source element a child belongs to (a page merges three source
  elements). The patcher needs no model change at all.
- **Regenerate and accept reordering.** Emitting "mapped, then preserved"
  keeps the data but changes every interleaving. Rejected: `diff` compares
  children in document order (ADR 0012), so every corpus file would fail the
  exit criterion, and Visio's own `Trigger`-between-cells layout would be
  lost.
- **Keep raw byte slices of the source XML per entity.** Rejected in
  ADR 0012 already: byte slices pin formatting and make insertion ambiguous.
- **Patch, but rewrite every attribute canonically.** Rejected: re-spelling
  unchanged values (`2.0` to `2`, `true` to `1`) turns a no-op write into a
  noisy diff and risks comparisons that are not numeric under `diff`.

## Amendment (task P8-20, ADR 0073)

Two additions to the writer, both outside the mapped surface this ADR
covers; see ADR 0073 for the decisions and their citations:

- every written package carries the windows part, its relationship and its
  content-type override (added when the package has none, byte-for-byte kept
  when it has one);
- every page a `BackPage` attribute names is marked `Background="1"` on
  write ("mark it"), and the reader consumes that derived mark.

The round-trip criterion's "same part list, content types and relationships"
grows one tolerance: the written package may add the windows entries to a
source that had none.
