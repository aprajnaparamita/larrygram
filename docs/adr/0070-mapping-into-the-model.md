# 0070: Mapping the binary records into the model, and the unit rule

- Status: accepted
- Date: 2026-10-23
- Task: P8-01

## Context

The `.vsd` importer must produce **the same model the `.vsdx` reader
produces**, so that a `.vsd` file saved as `.vsdx` goes through the
existing writer unchanged (KICKOFF.md Phase 8). The model is
format-agnostic (ADR 0011): entities keep their Visio ids and names,
cells keep formula, cached value and unit separately, and master and
stylesheet inheritance is never flattened (ADR 0013).

The binary format has no formulas: records carry computed values.

## Decision

The four domain modules map record groups onto model entities:

| Records | Model |
|---|---|
| pages, page sheets, page properties | `Document::pages`, `Page::sheet`, `Page::visio_id` and names |
| background-page references | `Page::background` resolved to a `PageId`, else a warning |
| connects | `Page::connects` |
| stencils, stencil pages, master shapes, name lists | `Document::masters`, `Master::shapes` |
| shape groups, shapes, guide shapes, shape ids | `Document::shapes` (`Shape`), nesting level to `parent`/`children`, record ids to `visio_id` |
| transforms (`XFormData`, `XForm1D`, text transforms) | the `Xform`, `Xform1D` and `TextTransform` cells |
| geometry rows (move, line, arc, elliptical arc, spline, NURBS, polyline, infinite line) | `Geometry` section rows |
| line, fill and shadow, text block | the `Line`, `FillForegnd`/`FillBkgnd`, `Shadow*` and `TextBlock` cells, on a shape or a style |
| character, paragraph, field and text records | `Character` and `Paragraph` rows, and `TextContent` parts (`Text`, `CharRun`, `ParaRun`, `TabsRun`, `Field`) |
| style sheets, style properties | `Document::stylesheet` (`Style` rows with their own sheets) |
| the colour table | `Document::color_table` |
| layers and layer membership | the `Layer` section rows and the `LayerMember` cell (amended 2026-10-26) |
| shape data, user-defined cells, custom properties | the `Property` and `User.*` sections; document custom properties to `Document::properties` (deferred 2026-10-26, dropped with a warning) |
| connection points and controls | `Connections` and `Controls` section rows (deferred 2026-10-26, dropped with a warning) |
| document properties (also from the summary streams) | `Document::properties`, `Document::kind` |

The document kind comes from the records (stencil pages mean
`DocumentKind::Stencil`, the template marker `DocumentKind::Template`),
not from the file name (ADR 0016).

**Cells carry cached values only.** A mapped record field becomes the
cell's cached value; `formula` is `None`, because the binary format has
no formulas. The Phase 6 fill-in-on-open policy (ADR 0057) applies to
these cells as it does to the `.vsdx` reader's.

**Unit rule.** Units convert only at the edges (AGENTS.md section 6):
the mapping layer converts each record field into the unit the target
cell uses in the model, exactly as the `.vsdx` reader spells it —
lengths in inches, angles in radians, percentages as the model's
`CellValue` carries them. A field with an unknown unit is a
`BadValue` warning and the cell stays unset.

**Dropped with a warning** (the plan lists them): foreign data (OLE
objects, pictures), hyperlinks, events and macro references,
annotations, reviewers, smart tags, the document font list and the
custom-label plumbing of shape data. Nothing is dropped silently.

## Consequences

- The mapping table above is the contract the domain tasks implement;
  a row a task cannot honour becomes an Open issue, never a silent
  difference.
- Fidelity is limited to what the records carry: there is nothing to
  preserve verbatim (no `Preserved` XML, unlike ADR 0012), so the
  round-trip guarantee is model-level, not byte-level.
- P8-04 (shapes), P8-05 (text), P8-06 (styles) and P8-07 (pages and
  masters) implement this ADR in their own modules.

## Amendments

- **2026-10-26 (P8-14), the layers row.** The row is implemented: a
  page sheet gains its `Layer` rows (`Name`, `NameUniv`, `Color`,
  `Visible`, `Print`, with the row `IX`), and a shape gains its
  `LayerMember` cell. The cell spelling follows a Visio twin's
  `Layers` rows one for one; the other cells of the row (`Status`,
  `Active`, `Lock`, `Snap`, `Glue`, `ColorTrans`) stay unset because
  no allowed source spells them.
- **2026-10-26 (P8-14), the shape-data and connection rows.** The rows
  *shape data, user-defined cells, custom properties* and *connection
  points and controls* are **deferred**. No allowed source spells
  those row payloads: the libvisio reference reads fragments of the
  records and maps none of the rows, and a mapping derived from file
  bytes alone would risk a silent wrong value, which this ADR forbids
  worse than a warning. Every record of these families is dropped with
  a `Dropped` warning that names what it carries, per shape or page,
  and the rows are open issues of `docs/progress/phase-8.md`. The same
  rule now covers the sheet rows no module maps (scratch, protection,
  misc, style properties, group, layout, image): nothing is dropped
  silently any more.
- **2026-10-26 (P8-14), the colour row.** Quick-style and theme
  colours are outside the alpha scope. The records carry the colour a
  quick-style index resolves to through the document theme, and
  neither the fields nor the theme are read. A file with fill or line
  records gets one `Unsupported` warning per import; the mapping
  decision (the model has `CellValue::Str("Themed")` and
  `Document::themes`) stays open.

## Alternatives considered

- **Flattening master inheritance into the shapes.** Rejected: the
  lookup resolves it (ADR 0013), and flattening would change the saved
  `.vsdx`.
- **Reconstructing formulas from the values.** Rejected: the binary
  format does not carry them; invented formulas would recalculate to
  something else.
