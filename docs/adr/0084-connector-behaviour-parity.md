# 0084: Connector behaviour parity: the alpha scope

- Status: accepted (drafted by P7-30; revised by the master after the
  Opus review, 2026-11-02)
- Date: 2026-11-02
- Task: P7-30
- Amends: ADR 0045 rules 1, 3 and 4

## Context

The user's manual pass (2026-11-02): the Connector tool "connects
objects in a seemingly random location, with no way to edit the
location of the connections or move them". ADR 0045 gave us glue
records, one-elbow routing and a re-route pass. P7-30 studied the
documented behaviour (`docs/ui/connectors.md`) and audited our code
(`docs/tasks/P7-30.md`). The review of that work found more:

- **The landing point.** `run_draw` resolves a glued end against the
  connector master's default endpoints, `reroute_document` against
  the other target's bounds centre, and both clamp to the axis-aligned
  bounds (`nearest_on_rect`). The same end jumps between passes and
  lands on no point the user chose.
- **Our "Connector" master is not a connector.** It is a 2-D 1.2 ×
  0.6 in box with a diagonal line and six connection points
  (`crates/model/src/stencils/basic_flowchart.rs`). It has no 1-D
  endpoint cells, no `ObjType`, no route style. Our `Connect` records
  name `FromCell` `BeginX` on a shape that has no `BeginX`. Other
  connectors glue to its box midpoints, and a selected connector
  shows eight resize handles.
- **Visio keeps glue in formulas.** Every glued end in the
  Visio-written corpus carries an endpoint formula (`PAR(PNT(…))` for
  point glue, `_WALKGLUE(…)` for dynamic glue) and a trigger
  (`_XFTRIGGER(Sheet.<n>!EventXFMod)`). The `Connects` records sit
  alongside (spec section 8). We write the records only, so our glue
  very likely opens unglued in Visio. Re-gluing a Visio-written
  connector leaves its old formula in place, so Visio would glue it
  back to the old shape.

This ADR decides what the alpha implements and what it leaves out.

## Decision

### 1. A connector is a 1-D routable shape (amends ADR 0045 rule 4)

The connector master is rebuilt as our own 1-D shape, using the
structure Visio's files show (spec section 8; the cells are
structure, the master is ours):

- 1-D Endpoints cells `BeginX`, `BeginY`, `EndX`, `EndY`.
- `PinX` = `GUARD((BeginX+EndX)/2)`, `PinY` = `GUARD((BeginY+EndY)/2)`,
  `Width` = `GUARD(EndX-BeginX)`, `Height` = `GUARD(0.2DL)`,
  `LocPinX` = `GUARD(Width*0.5)`, `LocPinY` = `GUARD(Height*0.5)`,
  `Angle` = `GUARD(0DA)`.
- `ObjType` 2 (routable), `GlueType` 2, `ShapeRouteStyle` 1
  (right angle, rule 8), `ConLineRouteExt` 1 (straight lines), and
  `BegTrigger`/`EndTrigger` = `_XFTRIGGER(EventXFMod)`.
- **No `Connection` section**: connectors are not glue targets in the
  alpha.

`Width` is negative when the end lies left of the begin. Every
transform, bounds and hit-test path must accept that without a panic
or a mirrored drawing.

`Document::is_connector(shape)` is true when the shape has the four
1-D endpoint cells (its own or inherited). It replaces "has a glue
record" as the test for "is a connector" (ADR 0045 rule 4). A shape
that is the `FromSheet` of a glue record but has no endpoint cells
(a file saved by an earlier Larrygram) still re-routes as before.
Nothing migrates it.

Every path write sets the four endpoint cells to the route's first and
last points, recalculates the pin and size cells, and writes the
geometry rows in the box's local coordinates.

### 2. Glue is stored the way Visio stores it (replaces P7-30's rule 6)

Each glue and unglue writes the endpoint formulas, their cached values
and the `Connects` record together, in one undoable edit:

| End state | `BeginX` (and `EndX`, mirrored) | `BeginY` | Trigger | Record |
|---|---|---|---|---|
| Point glue to row *k* of sheet *n* | `PAR(PNT(Sheet.n!Connections.Xk,Sheet.n!Connections.Yk))` | same formula | `_XFTRIGGER(Sheet.n!EventXFMod)` | `ToCell` `Connections.X<k>`, `ToPart` 100 + ix |
| Dynamic glue to sheet *n* | `_WALKGLUE(BegTrigger,EndTrigger,WalkPreference)` (`EndX`: `_WALKGLUE(EndTrigger,BegTrigger,WalkPreference)`) | same formula | `_XFTRIGGER(Sheet.n!EventXFMod)` | `ToCell` `PinX`, `ToPart` 3 |
| Unglued | literal value, no formula | literal | `_XFTRIGGER(EventXFMod)` | none |

- The cached `V` of every endpoint cell is the resolved point (rule 3)
  in page inches.
- A glue, re-glue or unglue **replaces** the end's formulas and
  trigger, including formulas that Visio wrote. No stale
  `Sheet.<old>` reference survives.
- Deleting a glue target clears the formulas of every end glued to it
  (the end keeps its last point as a literal), as well as the records.
- `PAR`, `PNT`, `_WALKGLUE` and `_XFTRIGGER` are Visio-owned
  functions. Our recalc keeps their cached value and never reports
  them as errors or warnings. Our router is what moves the ends.
- The `<k>` and `ix` spellings follow spec section 8: row number *k*
  is one-based, `ToPart` 100 + the zero-based index.

### 3. One endpoint resolution (replaces ADR 0045 rule 3's endpoint rule)

One module, `larrygram_geom::glue` (L0), resolves both ends of a
connector together. Draw, glue, unglue, end drags and every re-route
call it:

- **Point glue**: the named row's point through the target's page
  transform. If the row is gone, the end falls back to dynamic glue
  with one warning.
- **Dynamic glue**: the end picks from the target's **candidate
  points**: its connection points, or, when it has none, the four side
  midpoints of its local box mapped through its page transform. That
  fallback matches our masters' `edge_midpoints` and follows
  rotation, so a rotated shape's ends do not slide along an
  axis-aligned edge.
  - One dynamic end facing a fixed end (point glue or free): the
    candidate nearest the fixed point.
  - **Both ends dynamic** (centre-to-centre drags, Connect Shapes):
    `resolve_pair` takes the candidate pair, one on each target, with
    the smallest distance. Ties go to the lower candidate index on the
    begin target, then on the end target. The result is deterministic,
    with no circular dependency.
- **Free ends** resolve to their own endpoint cells and never move on
  a re-route.

The exact Visio tie-break and the rule for shapes with no points stay
open questions 5 and 7. This rule is ours until they are answered.

### 4. Routes leave the shape the way they should (amends ADR 0045 rule 2)

Each resolved end carries an **exit direction**:

- the connection row's `DirX`/`DirY` when it is non-zero;
- otherwise the outward normal of the side the point lies on (side
  midpoints) or of the nearest side of the target's local box
  (connection points without a direction), through the transform;
- none for a free end.

A right-angle route leaves each glued end along its exit direction
with a short stub (0.25 in, ours), then joins the stubs with
axis-aligned segments. Facing sides make a Z or a straight run,
perpendicular sides an L, and the same side a U. The route never
runs back along the shape's own edge. Straight routes ignore exit
directions. Obstacle avoidance stays out (ADR 0045).

### 5. Re-route only what an edit touched (amends ADR 0045 rule 3)

After an edit, the re-route pass recomputes only:

- connectors with an end glued to a shape that the edit moved,
  resized, rotated, flipped, deleted, or whose connection points it
  changed;
- connectors whose own ends, glue or style the edit changed.

Every other connector keeps its stored geometry, including routes
Visio drew and segments the user moved (rule 7). The stored geometry
is the memory: nothing goes in session state, and it survives save,
reopen, undo and redo. The re-route runs inside the edit, so one undo
restores both the shapes and the routes.

### 6. Editing the ends (spec section 3)

- **Drawing**: the Connector tool glues an end to a connection point
  when the pointer is within the snap distance of one (point glue).
  Otherwise, over a shape, it glues to the topmost non-connector shape
  whose outline contains the pointer (dynamic glue). Elsewhere the end
  is free. Alt suppresses glue. Moving an end uses the same rule.
- **Selection**: a selected connector shows its begin and end handles
  only. It has no resize box and no rotation handle. Glued ends and
  free ends show different markers (Visio's green and white, in our
  theme colours).
- **End drags**: dragging an end handle re-glues it, by the drawing
  rule, or unglues it on empty page. While dragging, the glue target
  highlights: the shape's outline for dynamic glue, the point for
  point glue. The connection points of the shape under the pointer
  show while any connector end is being drawn or dragged near it.
  The drag is one undoable `connector.move_end`.
- **Arrow keys**: an arrow key with connectors selected unglues both
  ends of each selected connector and nudges them (spec section 3).
  Shift+arrow nudges by one pixel, as for shapes.

### 7. Connection points and segments (spec sections 2 and 4)

- **Connection points**: the Connection Point tool
  (`tools.connection.point`) adds a point with Ctrl+click, moves the
  selected point with Ctrl+drag, and deletes it with Delete. The
  points show when the view check is on (`tools.connection.points` in
  Panel Modern, `view.connections` in Panel Classic). A point's
  right-click menu sets Inward, Outward or Inward & Outward (the row's
  `Type`). Points are `Connection` rows, edited through the model's
  mutation API (undoable). The first edit of an instance's inherited
  points copies them into its own section.
- **Segments**: a selected right-angle connector shows a midpoint
  handle on each interior segment. Dragging one moves the segment
  perpendicular to itself and stretches its neighbours. Its ends
  stay glued. The written geometry is the moved route. It stays until
  rule 5 re-routes that connector, which replaces it with a fresh
  route.
- **Reroute behaviour**: Reset Connector (`connector.dyn.conn.reroute`)
  re-routes the selected connectors fresh. The Panel Classic Layout &
  Routing toolbar writes `ConFixedCode`: Reroute Freely 0, Reroute on
  Crossover 3, Never Reroute 2. The alpha honours only Never: such a
  connector is never re-routed automatically, though its ends still
  follow their glue, and its interior is kept. Any other value
  behaves as rule 5.

### 8. Styles (spec section 5)

| Choice | `ShapeRouteStyle` | `ConLineRouteExt` |
|---|---|---|
| Right Angle | 1 | 1 |
| Straight Lines | 2 | 1 |
| Curved Lines | 1 | 2 |

The same cells on the page sheet (`RouteStyle`, `LineRouteExt`) are
the default for new connectors.

- **Reading**: a missing or 0 value falls through to the page, and a
  missing page value means right angle. Value 16 and the directed
  styles (3–15, 17–22) are kept and route as right angle in the
  alpha (open questions 11 and 12). Unknown values route as right
  angle. Today's fallback to straight goes.
- **Curved routes**: one `NURBSTo` row from the begin point to the end
  point. Its control points are the right-angle route's points, of
  degree min(3, points − 1), on a clamped uniform knot vector with
  weights 1. The row's `E` formula follows the ShapeSheet NURBSTo
  documentation, and the geometry crate's NURBS evaluation (ADR 0022)
  renders it.
- **Where the user sets the style**:
  - Panel Modern: Design > Connectors (`connector.right_angle`,
    `connector.straight.lines`, `connector.curved.lines`), with
    connectors selected, or with nothing selected for the page
    default.
  - Both layouts: the connector's context menu (`connector.right_angle`,
    `connector.straight`, `connector.curved`).
  - Panel Classic: Layout & Routing (`format.set.dyn.conn.routing.style`,
    `format.set.dyn.conn.appearance.straight`,
    `format.set.dyn.conn.appearance.curved`).

### Out of scope for the alpha (with reasons)

- **AutoConnect** and **dropping a shape on a connector** (spec
  section 6): a new tool with its own mini toolbar. The connectors
  themselves must be right first. A post-alpha task.
- **Line jumps** (spec section 7): crossing detection and jump
  glyphs, with no defect behind them. The cells are kept on round
  trip, and the Line Jump controls stay disabled with a "not yet"
  tooltip.
- **Obstacle-avoiding routing**: unchanged from ADR 0045 (a
  research-sized problem).
- **Reroute on Crossover** behaviour: the value is written and kept
  but not honoured (rule 7).
- **Glue to guides, vertices, handles and geometry edges**: the
  alpha glues to connection points and shapes only.
- **Connectors as glue targets** (rule 1), and **Reverse Ends**.

## Consequences

- A connector end lands on a point chosen by one documented rule, and
  stays there across draw, re-glue and unrelated edits. The "random
  location" class of defects goes away.
- Files we write carry Visio's glue formulas as well as the records,
  so glue should survive a trip through Visio. Open question 1 checks
  that.
- Selection, hit testing and handles must treat 1-D shapes as their
  own kind. Negative widths become a case every geometry path
  handles.
- The alpha has the visible connector features Visio users expect:
  sensible glue, movable ends, visible and editable connection
  points, movable segments, and the three styles.

## Alternatives considered

- **Records only, no glue formulas** (P7-30's draft). Rejected: the
  corpus shows Visio keeps glue in the formulas. [MS-VSDX] calls the
  `Connect` element unused in the subset it defines, and ADR 0038,
  which the draft relied on, was superseded by ADR 0056.
- **Resolve each dynamic end against the other end's resolved point.**
  Rejected: two dynamic ends depend on each other, the common case.
  `resolve_pair` resolves them together.
- **Clamp to the axis-aligned bounds when a shape has no points.**
  Rejected: wrong for rotated shapes, and it is the old defect.
- **Keep moved segments in session state.** Rejected: lost on save,
  undefined on undo, and erased whenever any edit re-routed every
  connector. Rule 5's scoped re-route makes the stored geometry
  the memory.
- **Visio's full routing engine** (avenues, crossover, directed
  styles). Rejected: research-sized. Our contract is documented
  divergence (ADR 0045).
