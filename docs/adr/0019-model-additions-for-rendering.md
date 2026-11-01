# 0019: Model additions for rendering

- Status: accepted
- Date: 2026-10-14
- Task: P3-01

## Context

Phase 3 renders pages from cached cell values (KICKOFF). Today the model
maps the `supported` surface of `docs/vsdx-inventory.md` and keeps the rest
in `Preserved` (ADR 0012): the geometry rows' typed meaning (`Row/@T`), the
`Text` element, the theme parts, `Colors`, `FaceNames` and the layer rows are
all `preserved-only`. The renderer needs typed data, and the Phase 2
round-trip must stay green while we add it.

Two constraints shape the design:

- The writer patches the source trees (ADR 0018): it writes what the model
  maps and leaves everything else in place. Any new *read-side* mapping is
  therefore round-trip safe as long as the writer is not taught to rewrite
  the new fields.
- The inheritance lookup (ADR 0013) resolves by `CellName` on a sheet, which
  cannot disambiguate the repeated cells of a `Geometry` section: every row
  has `X`, `Y`, `A`... Rendering a master instance needs row-aware
  resolution.

## Decision

### 1. Read-side mapping only

Phase 3 maps into the model **for reading only**. The new fields are typed
copies of content that stays in `Preserved`; the writer is not taught to
write them back. Editing text, theme or layers is written back in Phase 4/5
when the editor needs it. This keeps the Phase 2 round-trip criterion
(`LARRYGRAM_REQUIRE_CORPUS=1 cargo test -p larrygram-vsdx --test
roundtrip`) green by construction.

### 2. Typed geometry rows (hand-written enum)

`larrygram_model::geometry` (L1) types one `Geometry` section from the
rows' `T` attribute and cells. The row kinds are a **hand-written enum
backed by [MS-VSDX] and the ShapeSheet reference**, not codegen:

```rust
pub enum GeometryRow {
    MoveTo { x: f64, y: f64 },
    LineTo { x: f64, y: f64 },
    ArcTo { x: f64, y: f64, bow: f64 },
    EllipticalArcTo { x: f64, y: f64, ctrl: Point, angle: Angle, ratio: f64 },
    NurbsTo { x: f64, y: f64, second_last_knot: f64, last_weight: f64,
              first_knot: f64, first_weight: f64, data: Option<NurbsData> },
    PolylineTo { x: f64, y: f64, data: Option<PolylineData> },
    SplineStart { x: f64, y: f64, second_knot: f64, first_knot: f64,
                  last_knot: f64, degree: i32 },
    SplineKnot { x: f64, y: f64, knot: f64 },
    Ellipse { center: Point, a: Point, b: Point },
    InfiniteLine { p1: Point, p2: Point },
    RelMoveTo { x: f64, y: f64 },
    RelLineTo { x: f64, y: f64 },
    RelQuadBezTo { x: f64, y: f64, ctrl: Point },
    RelCubBezTo { x: f64, y: f64, ctrl1: Point, ctrl2: Point },
    RelEllipticalArcTo { x: f64, y: f64, ctrl: Point, angle: Angle, ratio: f64 },
    Unknown { row_type: String },
}
```

The `Rel*` rows keep their values as **fractions of the shape's width and
height** (the ShapeSheet reference: a `RelLineTo` `X` of 0.5 is a `LineTo`
`X` of `Width*0.5`); the scene builder multiplies. `NurbsTo` parses the
E-cell `NURBS(...)` formula text into `NurbsData` (knots, weights, control
points, `xType`/`yType`); `PolylineTo` parses the A-cell point list into
`PolylineData`. These are **formula-encoded cached values**: we parse the
stored string, we do not evaluate formulas (Phase 6). When the encoded data
is missing or unparsable, `data` is `None` and the row degrades to its
`X`/`Y`/`A`..`D` cells (ADR 0022).

Row-type families in `ui-spec/shapesheet.toml` (the Phase 1 deferred item)
stay deferred: the table would repeat what [MS-VSDX] already fixes and the
enum is the single source the renderer matches on. If the formula engine
(Phase 6) needs table-driven row families, revisit.

### 3. Shape text

`larrygram_model::text` types the `Text` element: its mixed content is a
list of parts — literal text, `cp`/`pp`/`tp` run markers carrying the `IX`
of the `Character`/`Paragraph`/`Tabs` row they select, and `fld` field
insertions carrying the `IX` of the field. Run semantics follow the
reference: a run marker starts a run that lasts to the end of the text or
the next marker. `Shape::text: Option<TextContent>` holds it; the `Text`
element also stays in `Preserved` as before. Fields' cached display text is
the literal text that follows the `fld` marker; field evaluation is
Phase 6.

### 4. Theme, colour table, fonts, layers, page geometry

- `larrygram_model::theme`: the theme parts' `a:clrScheme` (dk1, lt1, dk2,
  lt2, accent1..6, hlink, folHlink) and `a:fontScheme` (major/minor latin,
  ea, cs) into `ThemeColors`/`ThemeFonts`; effects (`a:fmtScheme`) stay
  preserved. `Document::themes: Vec<Theme>`, one per theme part. For the
  alpha the renderer uses `themes[0]`; per-shape `ThemeIndex` mapping is
  deferred (recorded in the P3-02 Result if corpus evidence needs it).
- `Document::color_table: Vec<ColorTableEntry>` from the document colour
  table (`Colors`/`ColorEntry`, indices from 24). Indices 0..23 are the
  fixed Visio palette, our own table of derived facts in
  `larrygram_model::values`. A color cell may cache an RGB (`#rrggbb`), a
  palette index or a table index; the reader resolves all three to `Color`.
- `FaceNames` stays preserved; the font a run uses is its `Character` row's
  `Font` cell (cached), with theme fonts as fallback (ADR 0024).
- Layers: `Layer` types the page sheet's `Layer` section rows (`ix`, `name`,
  `visible`, ...); `Document::shape_layers` parses the shape's
  `LayerMember` list cell into layer indices. The renderer skips shapes on
  invisible layers.
- Page geometry: `PageWidth`, `PageHeight`, `PageScale` and `DrawingScale`
  read from the page sheet through the lookup (below); `Page::background`
  already names the background page. The renderer draws the background page
  first, with a visited set so a background cycle renders once.

### 5. Row-aware and page-sheet resolution

The lookup (ADR 0013) gains two capabilities, in `larrygram-model`:

- `Document::resolve_row_cell(sheet, section, row_pos, cell)` resolves one
  cell of one row of a repeated section. The rungs are: the local row's
  cell, the master's row at the same position in the same section (matched
  by `IX` when both sides have one, else by position), **no style rung** (a
  `Geometry` cell never comes from a style), the section default and the
  hard default. `Inh` does not define (ADR 0013 amendment), cycles are cut
  as before.
- `Document::resolve_page_cell(page, cell)` resolves a page-sheet cell:
  the page's own cell, the style chain (by the page sheet's style links
  where present), the section default, the hard default.

`Document::shape_geometry(shape)` returns the typed `Geometry` sections of a
shape with row-level inheritance applied: the shape's own rows where it has
any, the master chain's rows otherwise (the ADR 0013 master rung), each row
cell resolved per the rules above.

### 6. Fixed Visio palette (derived facts)

Indices 0..23 of Visio's color list are fixed. The table is our own
transcription of observed Visio output (black-box rule 7 of AGENTS.md
section 4), kept in `larrygram_model::values::palette` as a constant with
the observation noted in `docs/sources.md`. It is data, not code: if a later
source contradicts it, one constant changes.

## Consequences

- Round-trip stays green: nothing the writer emits changes. When Phase 4
  edits text or theme, the writer grows a patch step for them then.
- The renderer gets typed rows and resolved values with one call per shape;
  provenance (`Resolved::via`) is still available for debugging.
- `resolve_row_cell` duplicates part of `resolve`'s walk; both share the
  visited-set and default rungs. If the two drift, a later task should
  factor one walker.
- `Unknown` row types render as nothing and warn, so a future row type is a
  data change, not a crash.

## Alternatives considered

- **Codegen row-type families from `ui-spec/shapesheet.toml`.** Rejected for
  Phase 3: the extractor change is Phase 1 work, the row set is fixed by
  [MS-VSDX], and generated rows would still need the hand-written semantics
  of decision 4 (ADR 0022). Revisit with the formula engine.
- **Map the new fields write-side too (full round-trip of typed data).**
  Rejected: it doubles the Phase 3 surface and risks breaking the exit
  criterion of Phase 2 for zero rendering benefit.
- **Resolve geometry per cell through the existing `resolve`.** Rejected:
  `CellName` alone cannot name "the `X` of row 3", so the lookup would
  silently return row 0's cell.
- **Flatten master geometry into instances on load.** Rejected by
  KICKOFF and ADR 0013.
- **Put the palette in `ui-spec/visual-styles.yaml` (ADR 0023).** Rejected:
  color resolution is model semantics (color cells resolve before the
  renderer sees them), and `model` must not depend on a UI data file.

## Amendments

### Amended 2026-10-15, P3-15: the geometry row union

`Document::shape_geometry` returns **the row union** of a shape and its
master chain, not "the shape's own rows whenever it has any". The corpus
shows why: instances often carry only some rows of a `Geometry` section
(`bgcolor.vsdx`'s `Rectangle` shapes carry two `LineTo` rows at `IX` 2..3
with every cell `F='Inh'` and only the `X` cell at that; `testfile1.vsdx`
shapes 8, 11 and 14 carry `IX` 2..4 and 1, 3, 4), while the master holds
the `MoveTo` and the closing rows. The old rule rendered those as open
polylines in the master's row positions, not as the master's closed paths.

The rule, per `Geometry` section (matched by its `IX`, else by its position
among the same-named sections):

1. The rows of the **furthest** rung of the master chain form the base, in
   file order. Each nearer rung — the shape's own section last — applies
   its rows on top.
2. A row **overrides** the merged row it matches: by `IX` when both sides
   have one, else by position. A row whose `IX` differs from the matched
   row's never matches by position (both sides have an `IX` and they
   disagree); the positional fallback applies only to a row without `IX`.
3. A row **overrides per cell**: the merged row's cells resolve through
   `Document::resolve_row_cell` as before — the nearer rung's cell wins
   when it defines (a formula that is not `Inh`), otherwise the further
   rung's cell, otherwise the defaults. A cell the nearer row omits is
   inherited like an `Inh` cell.
4. A row with `Del` (`Row/@Del`, spelled `1` or `true`) **removes** the
   merged row it matches; an unmatched `Del` row removes nothing, and a
   `Del` row never enters the union on its own.
5. An **unmatched** row is inserted in `IX` order (before the first merged
   row with a greater `IX`), or at its own position when it has no `IX`.
   The corpus never exercises this (every instance row matches a master
   row); it is our deterministic rule for added rows.
6. The row's `T` comes from the nearest rung that declares one.

The section-level cells (`NoFill`, `NoLine`, `NoShow`, `NoSnap`,
`NoQuickDrag`) merge the same way, per cell, through the lookup's
section-level rung (already row-free).

Divergences and limits:

- A deleted row removes the merged row, but a *re-added* row of a nearer
  rung may still take cells from a further rung's row of the same `IX`
  (the lookup skips nothing; only the union drops rows). Unobserved in the
  corpus; revisit if a file needs it.
- A merged row without `IX` carries no identity for the cell lookup: the
  lookup falls back to the positional row of each rung. Every corpus
  `Geometry` row has an `IX`.
- Inherited row cells are the master's **cached** values, in the master's
  coordinate space: a resized instance whose own cells are all `Inh` draws
  the master's geometry at master coordinates (the instance's snapshots are
  not used, ADR 0013). Formula evaluation (Phase 6) fixes this.

### Amended 2026-11-01, P7-27: the page-size pair, one resolver

`Document::page_size(page)` resolves `PageWidth` and `PageHeight`
**together**, as one pair, and every reader of the page size calls it: the
scene builder (`render::build`), the canvas, zoom-to-fit
(`commands::builtins::view`) and the Page Setup dialog. Each of them had a
private copy of the rule and the copies disagreed (the P7-24 review, A2).

The rule:

1. **Both axes come from one sheet.** The rungs are *sheets*: the page's
   own sheet first, then the sheets of its style chain (the page sheet's
   `LineStyle`/`FillStyle`/`TextStyle` links, each chain followed through
   the styles' own links of the same kind, cycles cut as in
   `resolve_page_cell`). The first rung sheet that defines at least one
   positive finite `PageWidth`/`PageHeight` supplies the pair; the axis it
   does not supply takes that axis of the default (US Letter, portrait).
   A width from one sheet and a height from another is therefore
   impossible.
2. **No background-page rung.** [MS-VSDX] 2.2.2.4 and 2.2.2.5 describe a
   foreground page's `BackPage` reference ("the latter drawing page is to
   be used as the background page for the former") and note that a
   background page "can have a different drawing scale than a foreground
   page"; the ShapeSheet reference's `PageWidth`/`PageHeight` cells say
   nothing about a background page. No allowed source says a foreground
   page takes its *size* from its background page, so the rung the P7-24
   dialog added is dropped (it made the dialog and the canvas disagree).
3. **The default pair** (8.5 x 11 in) answers when no rung supplies a
   positive size and when the page id is stale. The constant lives in
   `larrygram_model::DEFAULT_PAGE_SIZE`; `render::FALLBACK_PAGE_SIZE` is
   the same value.

The pair is deliberately *not* two `resolve_page_cell` calls: that
function's style rung is selected by the cell's kind, and a `Page
Properties` cell has none, so a page whose size lives on a style sheet
would never find it. The pair rule above is the page-size contract; it
lives beside `resolve_page_cell` and shares its style-walk discipline.
