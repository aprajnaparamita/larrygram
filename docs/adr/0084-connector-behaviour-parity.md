# 0084: Connector behaviour parity: the alpha scope

- Status: accepted
- Date: 2026-11-02
- Task: P7-30

## Context

The user's manual pass (2026-11-02): the Connector tool "connects
objects in a seemingly random location, with no way to edit the
location of the connections or move them". ADR 0045 gave us glue
records, one-elbow routing and a re-route pass, but the endpoint
choice is our own invention: outline glue resolves to "the nearest
outline point of the shape's bounds", measured from a reference point
that differs between draw time and every later re-route, and there are
no tools to see, add or move connection points or to move a
connector's ends or segments. P7-30 studied the documented behaviour
(`docs/ui/connectors.md`) and audited our code; this ADR decides what
the alpha implements and what it leaves out.

## Decision

### 1. Glue kinds follow Visio's (spec section 1)

Two glue kinds on each end, chosen the way Visio does:

- **Point glue**: an end glued to a `Connection` row of a shape. It
  stays on that row's point when either shape moves. The row is stored
  as `ToCell` `Connections.X<n>` (one-based row number) and `ToPart`
  100 + row index, as the corpus shows (spec section 8).
- **Dynamic glue**: an end glued to the shape (`ToCell` `PinX`,
  `ToPart` 3). The re-route places it on the connection point of the
  target nearest to the connector's **other end** (the "point of
  origin", spec section 1). When the target has no connection points,
  it lands on the nearest point of the target's geometry bounding box
  to the other end. Our `PinX` spelling gives the horizontal walking
  preference (spec section 1).

### 2. One endpoint resolution for every occasion (amends ADR 0045 rule 3)

The "random location" cause (P7-30 audit): `run_draw` resolves a
glued end against the freshly inserted master's default geometry
endpoints, while `reroute_document` resolves it against the other
target's bounding-box centre; both then clamp to the target's
bounding-box edge. The same end therefore jumps between passes, and
the two references match neither what the user drew nor Visio's
nearest-point rule.

The fix: one resolution function (new `larrygram_geom::glue`, L0) used
by draw, glue, unglue and every re-route:

```rust
/// Resolves one glued connector end to a page-space point.
pub fn resolve_glued_end(
    target: &GlueTarget,      // shape bounds + connection points (local), its page transform
    end: GlueEnd,             // the record's target (point row or the shape)
    origin: Point,            // the other end's resolved point
) -> Point;
```

- Point glue: the named row's point through the target's page
  transform (as today).
- Dynamic glue: the connection point nearest `origin`; ties broken by
  the lower row index; no points, the nearest point of the
  axis-aligned bounds to `origin`.
- Unglued ends resolve to their own last geometry point, so they do
  not move on unrelated re-routes.

ADR 0045 rule 3's "nearest outline point of the shape's bounds" is
replaced by this rule; the re-route hook, the one-warning contract and
the formula-driven skip stay as they are.

### 3. Editing the ends and the points (spec sections 2 and 3)

- **End handles**: dragging a connector's begin or end handle re-glues
  (drop on a shape outline or a connection point) or unglues (drop
  away from every shape); the glue target highlights while hovering,
  and a selected connector shows its glue state at the ends. The drag
  dispatches through the command registry (`connector.move_end`, the
  existing `connector.glue_*`/`connector.unglue_*` semantics).
- **Connection points**: a Connection Point tool (add with
  Ctrl+click, move with Ctrl+drag, delete with Delete) and a View >
  Visual Aids toggle that shows the points. Points are `Connection`
  rows; add, move and delete edit those rows through the model's
  mutation API (undoable). Point types (inward/outward) are preserved
  but not editable in the alpha.

### 4. Segments (spec section 4)

- Right-angle connectors get midpoint handles on their segments.
  Dragging one moves the whole segment perpendicular to itself and
  extends its neighbours (the classic segment drag).
- A moved segment is **preserved** until an edit moves one of the two
  glued shapes, and then the whole path re-routes (our re-route is
  still one pass, no obstacles). This approximates Visio's
  "reroute as needed"; the full `ConFixedCode` matrix is out of
  scope.

### 5. Styles (spec section 5)

Right-angle, straight and curved (NURBS) as `ShapeRouteStyle` /
`ConLineRouteExt` values, settable per connector and as the default
for new connectors (the page's `RouteStyle`); stored so a file keeps
the user's choice. The directed styles (org chart, flowchart, tree,
network) are read and preserved but never authored.

### 6. Storage parity (spec section 8)

Glue is stored as `Connects` records in the documented spellings
(section 1 above), and a file saved by us keeps them losslessly (ADR
0048). We do not write glue formulas (`BeginX`/`EndX` triggers,
`GlueInfo`): formula-less editing (ADR 0038) owns that boundary.
Whether Visio re-glues from records alone is open question 1
(`docs/ui/connectors.md`) for the observation track; until it is
answered, record-only glue is our documented limitation.

### Out of scope for the alpha (with reasons)

- **AutoConnect** and **drop a shape on a connector** (spec section
  6): new-tool work with its own mini-toolbar surface; the connectors
  themselves must be right first. A later task (post-alpha) takes it.
- **Line jumps** (spec section 7): rendering work (crossing detection
  plus jump glyphs) with no defect behind it; deferred. The cells are
  preserved on round trip.
- **Obstacle-avoiding routing**: unchanged from ADR 0045 (a
  research-sized problem); dynamic glue's point walking covers the
  visible cases.
- **Inward/outward point types, direction vectors, extended rows**:
  preserved, not editable (spec section 2).
- **Glue to guides, vertices, handles, geometry edges** (spec
  section 1): the `VisGlueSettings` matrix stays at connection points
  and shape outlines; vertices and geometry glue come with the Snap &
  Glue work.

## Consequences

- The connector end lands where the user dropped it and stays on the
  same rule across draw, glue and every re-route; "random location"
  ends.
- The alpha has the four visible connector affordances Visio users
  expect: sensible glue, movable ends, visible and editable connection
  points, movable segments.
- Files carry the documented `Connects` spellings, so the corpus
  cross-checks and a later Visio round trip can verify them.
- The deferred items are recorded here and in the parity notes, not
  hidden.

## Alternatives considered

- **Amend ADR 0045 instead of a new ADR.** Rejected: the endpoint
  rule, the editing surface and the alpha scope are new decisions;
  0045 keeps its history and its rule 3 is superseded by rule 2 here.
- **Keep per-pass references and only fix outline glue.** Rejected:
  the draw/re-route disagreement is the defect's cause (P7-30 audit);
  a single resolution rule removes the whole class.
- **Write glue formulas too (SETF into `BeginX`/`EndX`, `GlueInfo`
  triggers).** Rejected for the alpha: ADR 0038 keeps editing
  formula-less, and the `Connects` records are the format's glue
  representation. Open question 1 decides it later.
- **Visio's full re-route engine (avenues, crossover, directed
  styles).** Rejected: research-sized; our contract is documented
  divergence (ADR 0045).
