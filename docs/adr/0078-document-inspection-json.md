# 0078: Document inspection as stable JSON

- Status: proposed
- Date: 2026-10-25
- Task: P9-01

## Context

An agent that builds a drawing must be able to read it back: which
pages exist, which shapes are on a page, where they are, what they say,
how they are connected, what is selected, and, when it needs detail,
a shape's ShapeSheet cells. The answer must be stable JSON (an agent
and a test compare it across versions), small enough for a model's
context, and bounded for a large drawing (corpus pages hold hundreds of
shapes; the wide corpus has pages with thousands).

The CLI's `inspect --json` (ADR 0026) prints a document summary for
people. It is not paged, has no shape detail and is tied to the CLI's
output; it stays as it is.

## Decision

### Four views

| Method | Returns |
|---|---|
| `inspect/document` | `DocumentView`: the `DocumentInfo` (handle, name, path, dirty, active, page count, active page, undo and redo labels), the kind (`drawing`, `stencil`, `template`), every page's `PageSummary` and every master's `MasterSummary` (name, instance count) |
| `inspect/page` | `PageView`: the page's summary, one page of `ShapeSummary` values, `next_cursor`, `total`, and the page's glue connections touching the returned shapes |
| `inspect/shape` | `ShapeView`: the summary, the full text and the requested cells |
| `inspect/selection` | `SelectionView`: the page, the selected shapes' ids in selection order, the primary shape |

The structs are in `crates/control/src/ops.rs`; the protocol document
(`docs/control-protocol.md`) prints one example of each.

### Identity

- A **document** is its handle (ADR 0076).
- A **page** is its 1-based index or its exact name (`PageRef`). The
  index counts every page, background pages included, in document
  order, as the page tabs show them.
- A **shape** is its **Visio id on its page** (`Shape::visio_id`), the
  id the file stores and the one `edit.select` already takes. It
  survives save and reopen, unlike the model's arena index, so an agent
  can save, reopen and keep its references.
- A **master** is its name, the value `insert.shape` takes.

### Shape summary

`id`, `name`, `master` (name, for an instance), `kind` (`shape`,
`group`, `connector` for a 1-D shape, `foreign` for an embedded
object), `text` (plain text, cut at `max_summary_text_bytes` = 1 KiB on
a character boundary, with `text_truncated`), `pin_x`, `pin_y`,
`width`, `height` (inches), `angle` (degrees counter-clockwise),
`parent` and `children` (Visio ids). The values are the effective
values the canvas uses (`CellReader`, ADR 0021 and 0054: inherited
from the master and style when the shape does not set them).

Connections are listed per page as `ConnectView` (`from_shape`,
`from_cell`, `to_shape`, `to_cell`), the model's `Connect` rows with
shape ids as Visio ids: enough to check that a flowchart's connectors
are glued where they should be.

### Cells on request only

`inspect/shape` returns no cells unless `cells` names what to return:
section names as the ShapeSheet shows them (`Geometry1`, `Character`,
`Connection`, `Property`, ...), `cells` for the shape's single cells
(the Shape Transform, Line, Fill and other one-row sections), or `all`.
Each `CellView` has the section (absent for single cells), the 0-based
row, the cell name, the formula as stored, the value as Visio would
display it, the numeric value in internal units when numeric, and the
stored unit. The formula and value come from the model and the
ShapeSheet engine's cached values (ADR 0057); inspection never
recalculates.

### Paging and filtering

- `inspect/page` and `commands/list` return at most `limit` items
  (default `Limits::default_page_items` = 100, capped at
  `max_page_items` = 1000) and a `next_cursor` while more remain;
  `total` counts all matches. A cursor is an opaque string (an encoded
  offset); passing it back with the same query returns the next page.
  A cursor that does not decode is `InvalidParams`. Paging is by
  position: an edit between two calls may shift items, which an agent
  sees in `total`.
- `ShapeFilter` keeps shapes that match every set field: `ids`,
  `name_contains`, `master` (exact name), `text_contains` (both
  contains matches are case insensitive).
- `recursive: true` walks into groups, children right after their
  group; the default lists the page's top-level shapes in z-order
  (back to front).
- Connections touching the returned shapes are capped at
  `max_page_items` with `connects_truncated`.

### Stability rules

- Field names are `snake_case` and never change meaning within a
  protocol major version (ADR 0075). New fields may appear; absent
  optional fields are omitted, not `null`.
- Lengths are inches, angles degrees, positions page coordinates with
  the origin at the bottom left and y up, as Visio stores them (AGENTS.md
  section 6: internal units at the edge). No display-unit conversion:
  `Settings::units` does not change the JSON.
- Numbers are plain JSON numbers; a non-finite model value (which the
  readers already refuse) would be omitted, never written as `NaN`.
- Order is deterministic: pages in document order, shapes in z-order,
  cells in ShapeSheet order, masters in document order.

Inspection is pure: the functions in `crates/control/src/inspect.rs`
take `&AppCtx` and never change it.

## Consequences

- An end-to-end test (ADR 0082) can assert a flowchart by its page
  view: ten shapes with the right masters and texts, five connects.
- Big pages cost the agent what it asks for: a filtered, paged list,
  not the whole page.
- Cell output depends on the engine's cached values; a cell whose
  value is not cached shows its formula only.
- `kind` is a summary, not the model's full shape type; a later
  version may add values (clients must accept unknown kinds).

## Alternatives considered

- **Reuse the CLI's `inspect --json`.** Rejected: unpaged, document
  level only, and its shape is the CLI's contract (ADR 0026).
- **The model's arena ids.** Rejected: they change on save and reopen
  (the P5-10 flowchart test renames them to compare).
- **Return every cell by default.** Rejected: a shape has hundreds of
  cells; a page of them would flood a model's context.
- **JSON Pointer or a query language.** Rejected: four views and a
  filter cover the alpha's needs and stay easy to document.
- **Display units in the output.** Rejected: an agent needs one unit;
  inches are the model's.
