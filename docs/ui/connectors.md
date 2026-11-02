# Connectors: Visio's documented behaviour

Task P7-30. The behaviour spec for dynamic connectors: what the
allowed sources (AGENTS.md section 4) document about Visio's
connectors, glue, connection points, routing and line jumps, in our
own words, with the source of each rule. Where the sources are silent,
the question goes to "Open questions for black-box observation".

Sources are cited in brackets. The list at the end gives each tag its
URL. Microsoft support content, the ShapeSheet
reference and the VBA reference are the behaviour sources; [MS-VSDX]
is the storage source; [corpus] is derived facts from the licensed
corpus files.

## 1. Glue kinds: point (static) and dynamic

- A connector can hold either kind of connection on either end; the
  two ends are independent. [edit-connectors]
- **Point connection** (static): the connector end is glued to one
  specific point on the shape. Moving or rotating the shape keeps the
  end at that same point. [edit-connectors]
- **Dynamic connection** (shape to shape): the connector end is glued
  to the shape, not to a point. When the shape moves or rotates, the
  end moves to the connection point on that shape nearest to the
  connector's point of origin (the other end). [edit-connectors]
- How the user gets each kind with the Connector tool: [edit-connectors]
  - Point: drag from a connection point on one shape to a connection
    point on the other. The endpoints turn green when connected.
  - Dynamic: hover over the center of the first shape until a green
    box appears, drag from there to the center of the second shape,
    release when the green box appears around it.
- Gluing a connector end to a shape's connection point creates a
  static connection; gluing it directly to the shape creates a
  dynamic connection. [glue-unglue]
- **AutoConnect** and the **Connect Shapes** command create dynamic
  connections on both ends. [edit-connectors]
- Glue (the attachment behaviour) is a document-level switch: the
  Snap & Glue dialog's "Glue" check box turns glue on or off for
  newly drawn connectors. It does not change existing connectors.
  [glue-unglue] The automation surface exposes the same switch as
  `Document.GlueSettings`, a bit set: glue to guides, handles,
  vertices, connection points, geometry, or disabled. [vba:VisGlueSettings]
- The glue targets a 1-D end can have (automation `Cell.GlueTo`)
  [vba:GlueTo]:
  - a connection point cell (static glue);
  - the pin of a 2-D shape: dynamic glue. Gluing to `PinX` gives a
    horizontal walking preference, gluing to `PinY` a vertical one.
    The source shape must be routable or walking-glue typed, and must
    not forbid walking glue;
  - any cell of a geometry vertex row: a connection point is created
    at that vertex and the end glues to it;
  - the begin or end cell of another 1-D shape: likewise a created
    connection point;
  - an edge (Alignment section cell) of a 2-D shape: likewise a
    created connection point;
  - a guide or guide point: the end glues to the guide (two records,
    one per axis; see section 8).
- `Cell.GlueToPos(shape, xPercent, yPercent)` creates a connection
  point at the given fraction of the target's width and height and
  glues the end to it. [vba:GlueToPos]
- "Walking" is the behaviour of a dynamic end: it slides along the
  target's connection points as the shapes move, staying on the point
  nearest the other end. [edit-connectors] [vba:GlueTo]

## 2. Connection points

- A connection point is a special point on a shape that connectors
  and other shapes can glue to. A glue to a connection point survives
  moving either shape. [edit-connectors]
- Connection points are not the only glue targets: shape vertices,
  shape handles and shape geometry also accept glue, governed by the
  Snap & Glue dialog. [edit-connectors] [vba:VisGlueSettings]
- **Visibility**: a shape's connection points appear when the user
  hovers near the shape with the Connector tool, or drags a connector
  or line endpoint near the shape. [edit-connectors] They can also be
  shown persistently with the **Connection Points** check box in
  View > Visual Aids, and hidden by clearing it. [edit-connectors]
  The automation surface is `Window.ShowConnectPoints`, "equivalent
  to selecting Connection Points in the Visual Aids group". [vba:ShowConnectPoints]
- **Connection Point tool** (Home > Tools), for add, move, delete:
  [edit-connectors]
  - Add: select the shape, choose the tool, Ctrl+click where the new
    point goes. The new point is selected after placement.
  - Move: with the tool active, Ctrl+drag the point.
  - Delete: with the tool active, select the point (it turns magenta)
    and press Delete.
  - Esc (or choosing the Pointer Tool) leaves the tool.
- **Point types** (right-click a point with the tool active):
  inward, outward, or inward and outward. [edit-connectors]
  Inward points attract connector endpoints and the outward points of
  2-D shapes; outward points glue to inward points; inward+outward
  does both. The ShapeSheet `Type` cell is 0 = inward, 1 = outward,
  2 = both; the default is inward. [ss:conn-row]
- **Direction vector**: each point has `DirX`/`DirY` cells: the
  required alignment vector of a matching point, and the direction
  that orients the connector leg attached to the point. Both zero
  means directionless. [ss:conn-row]
- **Storage**: one ShapeSheet `Connection Points` row per point, with
  `X`, `Y` (the point in the shape's local coordinates), `DirX`,
  `DirY`, `Type`, and a scratch `D` cell. Row names appear as
  `Connections.<name>`; unnamed rows are `Connections.Row_<n>`. [ss:conn-section]
- The documented default: points made through the user interface,
  which is most of them, are plain (non-extended) rows with row type `visTagCnnctPt` or `visTagCnnctNamed`; extended
  `A/B/C/D` rows are uncommon. [ss:conn-row] The sources do not state
  which points a shape has before the user adds any: see the open
  questions.

## 3. Moving and re-gluing connector ends

- **Unglue one end**: drag the endpoint away from the shape's
  connection point. Only the dragged end disconnects. [glue-unglue]
- **Unglue both ends of one or more connectors**: select the
  connector(s) and press an arrow key. [glue-unglue]
- **Glue a free end**: drag the end over a shape's outline or a
  connection point; the outline or the point highlights in green
  before release. Gluing to a point makes a static connection;
  gluing to the outline makes a dynamic one. [glue-unglue]
- **Glued indicator**: a selected glued connector shows green dots at
  its endpoints; an unglued one shows white or grey dots. [glue-unglue]
- Nudging a selected connector with Shift+arrow keys adjusts it one
  pixel at a time. [edit-connectors]
- `Shape.Disconnect` (automation) unglues one or both ends, with a
  stated offset for the freed endpoints; zero offset keeps them in
  place. It applies only to 1-D routable shapes. [vba:Disconnect]

## 4. Segments and re-routing

- **Reroute by dragging**: to reroute a connector, select it and drag
  a midpoint to a new location. [edit-connectors]
- The default **Dynamic connector** re-routes itself around other
  shapes on its own; connector shapes from the Visio Extras
  > Connectors stencil mostly do not. [edit-connectors]
- When a connector re-routes after an edit is governed per connector
  by the `ConFixedCode` cell: reroute freely (0), reroute as needed,
  i.e. manual reroute (1), never (2), reroute on crossover (3). The
  Behavior dialog's Connector tab is the UI. [ss:confixedcode]
- Which side or point a dynamic end picks: the connection point
  nearest the connector's point of origin. [edit-connectors]
- What a later re-route does to a segment the user moved is not
  documented in the allowed sources: see the open questions.

## 5. Connector styles

- Three routing styles: **Right-Angle**, **Straight Lines**,
  **Curved Lines**. [edit-connectors]
- Where the user sets them: [edit-connectors]
  - per connector: Design > Layout > Connectors (with the connector
    selected), or the connector's right-click Action menu;
  - as the default for new connectors: the same Connectors dropdown
    with nothing selected, or Page Setup > Layout and Routing >
    Appearance (Straight or Curved).
- Storage: the connector's `ShapeRouteStyle` cell (Shape Layout
  section): 0 = page default, 1 = right angle, 2 = straight, and
  further values for the directed styles (org chart, flowchart,
  tree, network). The page's `RouteStyle` cell (Page Layout section)
  is the default for connectors without a local style; its default is
  right angle. [ss:routestyle] [ss:shaperoutestyle]
- The straight/curved appearance switch is the `ConLineRouteExt`
  cell: 0 = page default, 1 = straight, 2 = curved (NURBS). [ss:conlinerouteext]
- Right-click **Reverse Ends** reverses the flow direction of the
  selected connectors. [edit-connectors]

## 6. AutoConnect and dropping a shape on a connector

- **AutoConnect**: with AutoConnect active (View > Visual Aids >
  AutoConnect), hovering over a shape shows arrows around it;
  hovering over an arrow shows a mini toolbar with the first four
  Quick Shapes; picking one adds the shape in that direction and
  connects it. Dragging a shape from the Shapes pane onto an arrow
  does the same with that shape. [add-connectors] [add-and-connect]
- AutoConnect is a per-file option ("remains that way for the current
  file"); some templates start with it on. The application-level
  master switch is File > Options > Advanced > Enable AutoConnect.
  [add-connectors]
- AutoConnect connections are dynamic on both ends. [edit-connectors]
- The mini toolbar shows up to four shapes, taken from the Quick
  Shapes stencil (the first four shapes of the stencil after Quick
  Shapes, in Shapes pane order). [add-connectors]
- Automation: `Shape.AutoConnect` draws the connection with a
  direction (`VisAutoConnectDir`: up, down, left, right, none = "connect
  without relocating the shapes"); `Page.DropConnected` drops a 2-D
  shape at a direction and connects it through a given 1-D connector.
  [vba:AutoConnect] [vba:DropConnected]
- Dropping a shape onto an existing connector: the sources describe
  replacing shapes ("Replace one shape with another", Change Shape)
  but not the drop-on-connector gesture itself: see the open
  questions.

## 7. Line jumps and routing around shapes

- Crossing connectors either cross as straight lines or show line
  jumps; the page-level switch is Design > Layout > Connectors >
  Show Line Jumps. [line-jumps]
- Page settings (Page Setup > Layout and Routing > Line jumps):
  which connectors jump ("Add line jumps to"), the jump style (arc,
  gap, square, multi-sided), and the horizontal and vertical jump
  sizes. "Add line jumps to = None" removes jumps. [line-jumps]
- Storage: the page's `LineJumpCode` cell chooses which lines jump
  (none, horizontal, vertical, last routed, topmost, bottommost) and
  `LineJumpStyle` the shape of the jump; the sizes are
  `LineJumpFactorX`/`LineJumpFactorY`. A connector can override with
  `ConLineJumpCode` (page default, never, always, other connector
  jumps, neither jumps) and `ConLineJumpStyle`, and set the jump
  direction with `ConLineJumpDirX`/`ConLineJumpDirY`. [ss:linejumpcode]
  [ss:conlinejumpcode] [ss:conlinejumpstyle]
- Routing around shapes: the Dynamic connector routes around other
  shapes by itself [edit-connectors]; the page
  layout cells (`AvenueSizeX`/`AvenueSizeY`, `BlockSizeX`/`BlockSizeY`,
  `PageShapeSplit`) parameterise the layout and routing engine, and
  shape-level `Shape Layout` cells override page defaults.
  [ss:pagelayout] [ss:shapelayout]

## 8. How glue is stored in the file

- **`Connects` records**: every glue is one `Connect` element in the
  page's (or master's) `Connects` list, attributes `FromSheet`,
  `FromCell`, `FromPart`, `ToSheet`, `ToCell`, `ToPart`.
  [ms-vsdx 2.3.4.2.12] [ms-vsdx 3.x] Visio writes short spellings
  when the optional attributes are default (observed in the patch
  writer's corpus pin, e.g. `<Connect FromSheet="4" FromCell="BeginX"
  ToSheet="5"/>`). [corpus]
- The `From*` values name the connector end: `FromCell` `BeginX` or
  `EndX`, `FromPart` the `VisFromParts` value `visBegin` (9) or
  `visEnd` (12). [corpus] [vba:VisFromParts]
- **Static (point) glue** names the connection row: `ToCell`
  `Connections.X<n>` and `ToPart` 100 + row index (`VisToParts`
  `visConnectionPoint` = 100 + index). Observed in the corpus:
  `<Connect FromSheet='3' FromCell='EndX' FromPart='12' ToSheet='2'
  ToCell='Connections.X5' ToPart='104'/>` (row number 5 names row
  index 4). [corpus] [vba:VisToParts]
- **Dynamic (shape-to-shape) glue** names the target's pin: `ToCell`
  `PinX` (horizontal walking preference) or `PinY` (vertical), and
  `ToPart` `visToWholeShape` (3). [vba:GlueTo] [vba:VisToParts]
- **A guide glue** produces two records from the same end: one to
  the guide's `Angle` cell from `BeginX`/`EndX`, one from
  `BeginY`/`EndY`. [vba:GlueTo]
- **Glue triggers**: when a 1-D shape is dynamically glued, the
  application generates a trigger formula (`BegTrigger`/`EndTrigger`
  in the Glue Info section) that refers to the other shape's
  `EventXFMod` cell; a change to that shape recalculates the trigger,
  and other formulas move the endpoint to hold the connection.
  [ss:glueinfo]
- **Endpoint cells**: `BeginX`/`BeginY`/`EndX`/`EndY` (1-D Endpoints
  section) hold the end coordinates; glued ends carry formulas that
  keep them on the target. [ss:1d] [vba:GlueTo]
- [MS-VSDX] documents the `Connect` element for the Visio Graphics
  Service subset as "unused and MUST be ignored" there (2.3.4.2.12),
  while the full schema (section 3) keeps the six attributes; the
  records exist for interoperability either way. [ms-vsdx]
- **What Visio-written files carry** (corpus and wide corpus, counted
  over the first 800 wide-corpus `.vsdx` files; derived structure
  only) [corpus] [wide-corpus]:
  - every point-glued end has an endpoint formula
    `BeginX` = `PAR(PNT(Sheet.<n>!Connections.X<k>,Sheet.<n>!Connections.Y<k>))`
    (and the same for `EndX`, with `BeginY`/`EndY` alongside), the
    target sheet and row named in the formula;
  - every dynamically glued end has `BeginX` = `_WALKGLUE(BegTrigger,EndTrigger,WalkPreference)`
    and `EndX` = `_WALKGLUE(EndTrigger,BegTrigger,WalkPreference)`;
  - every glued end has a trigger, `BegTrigger`/`EndTrigger` =
    `_XFTRIGGER(Sheet.<n>!EventXFMod)` naming the target; an unglued
    end's trigger is `_XFTRIGGER(EventXFMod)` (its own sheet);
  - the `Connects` records sit alongside the formulas.
  So the glue lives in the endpoint and trigger formulas; the records
  are the cross-reference. A file that writes records alone is very
  likely to open unglued, and a re-glue that leaves an old
  `PAR(PNT(Sheet.<old>…))` formula in place glues back to the old
  shape.
- **The Dynamic connector master as Visio's files carry it** [corpus]
  [wide-corpus]: a 1-D shape (`BeginX`/`BeginY`/`EndX`/`EndY`
  present) with `PinX` = `GUARD((BeginX+EndX)/2)`, `PinY` =
  `GUARD((BeginY+EndY)/2)`, `Width` = `GUARD(EndX-BeginX)`, a small
  constant `Height` (0.2 in, `GUARD(0.2DL)`), `LocPinX`/`LocPinY` at
  half the width and height, `Angle` 0; `ObjType` 2 (routable),
  `GlueType` 2, `ShapeRouteStyle` 16, `ShapeSplittable` 1, and **no
  `Connection` section**. No `ConFixedCode` cell (the default, 0).
  Its geometry is drawn in the box's local coordinates and leaves the
  box for bent routes; `Width` is negative when the end lies left of
  the begin.
- Value counts in the same sample, for the cells the alpha writes:
  `ShapeRouteStyle` 16, 1, 6, 3, 2, 0 (most to least common);
  `ConLineRouteExt` 1, 2, 0; the page's `RouteStyle` 1, 6, 2, 5;
  `ConFixedCode` 3, 6, 4, 5 on instances.

## 9. Open questions for black-box observation

Questions about Visio's behaviour that the allowed sources do not
answer:

1. (Answered by the corpus, section 8: Visio keeps glue in the
   endpoint and trigger formulas, so we write both.) Does a file with
   the formulas and the records, written by us, open in Visio with the
   same glue and the same route?
2. When Visio opens a file whose glue formulas exist but whose
   `Connects` records are missing, is the glue intact?
3. Which connection points does a freshly drawn shape have before the
   user adds any (for a plain drawn rectangle, and for an instance of
   a master that defines none)?
4. After the user drags a segment midpoint of a right-angle
   connector, what does the next automatic re-route do: keep the
   moved segment, reset the whole path, or only reset the segments
   the moving shape forces to change? Does the answer depend on
   `ConFixedCode`?
5. Which point does a dynamic end pick when several connection points
   tie for nearest to the other end? Does `DirX`/`DirY` or the
   `PinX`/`PinY` walking preference break the tie?
6. What exactly happens when a shape is dropped onto an existing
   connector: is the connector split in two, does the dropped shape
   insert itself along the path, and what happens to the glue
   records?
7. Where does the endpoint land for a dynamic glue on a shape with no
   connection points at all: the outline, the bounding box edge, or
   the nearest point to the other end on the geometry?
8. Does the green hover box appear for any 2-D shape, or only for
   shapes that accept glue, and does it show during a drag from
   anywhere on the shape or only near the center?
9. The stock master carries no `ConFixedCode` (default 0, section 8).
   Does dragging a midpoint change the cell, and to which value?
10. What does the page-level "Show Line Jumps" switch write to the
    page sheet, and do files from us with our jump values draw the
    same jumps in Visio?
11. The stock Dynamic connector carries `ShapeRouteStyle` 16 (the
    center-to-center value) yet draws right-angle routes with a
    default page. Which page and shape cells decide the drawn style
    of a new connector, and which value should a connector we draw
    carry to route the same way?
12. Which `ShapeRouteStyle`, `ConLineRouteExt` and page
    `RouteStyle`/`LineRouteExt` values does each choice of the
    Connectors dropdown (Right Angle, Straight Lines, Curved Lines)
    write, with a connector selected and with nothing selected?

## Sources

- [add-connectors] Microsoft support, "Add connectors between Visio
  shapes", `support.microsoft.com/en-us/visio/add-connectors-between-visio-shapes`
 .
- [edit-connectors] Microsoft support, "Edit connector lines, arrows,
  or points",
  `support.microsoft.com/en-us/visio/edit-connector-lines-arrows-or-points`
 .
- [glue-unglue] Microsoft support, "Glue or unglue connectors",
  `support.microsoft.com/en-gb/visio/glue-or-unglue-connectors`.
- [line-jumps] Microsoft support, "Add or remove connector line
  jumps", `support.microsoft.com/en-us/visio/add-or-remove-connector-line-jumps`
 .
- [add-and-connect] Microsoft support, "Add and connect shapes in
  Visio", `support.microsoft.com/en-us/visio/visio/add-and-connect-shapes-in-visio`
 .
- [ss:conn-section] [ss:conn-row] ShapeSheet reference, "Connection
  Points Section" and "Connection Points Row", plus the `X`, `Y`,
  `DirX/A`, `DirY/B`, `Type/C` cell pages (source 4 batch).
- [ss:1d] ShapeSheet reference, "1-D Endpoints Section", `BeginX`,
  `BeginY`, `EndX`, `EndY` cell pages (same batch).
- [ss:glueinfo] ShapeSheet reference, "Glue Info Section",
  `BegTrigger`, `EndTrigger` cell pages (same batch).
- [ss:confixedcode] ShapeSheet reference, `ConFixedCode` cell (Shape
  Layout section) (same batch).
- [ss:routestyle] [ss:shaperoutestyle] ShapeSheet reference,
  `RouteStyle` cell (Page Layout section) and `ShapeRouteStyle` cell
  (Shape Layout section) (same batch).
- [ss:conlinerouteext] ShapeSheet reference, `ConLineRouteExt` cell
  (Shape Layout section) (same batch).
- [ss:linejumpcode] [ss:conlinejumpcode] [ss:conlinejumpstyle]
  ShapeSheet reference, `LineJumpCode` (Page Layout section),
  `ConLineJumpCode`, `ConLineJumpStyle` (and `ConLineJumpDirX/Y`)
  cell pages (same batch).
- [ss:pagelayout] [ss:shapelayout] ShapeSheet reference, "Page Layout
  Section" and "Shape Layout Section" overviews (same batch).
- [vba:GlueTo] [vba:GlueToPos] VBA reference, `Cell.GlueTo`,
  `Cell.GlueToPos` (source 2 batch).
- [vba:AutoConnect] [vba:DropConnected] [vba:Disconnect]
  [vba:ShowConnectPoints] VBA reference, `Shape.AutoConnect`,
  `Page.DropConnected`, `Shape.Disconnect`, `Window.ShowConnectPoints`
  (same batch).
- [vba:VisGlueSettings] VBA reference, `VisGlueSettings`,
  `VisFromParts`, `VisToParts`, `VisAutoConnectDir`, `VisConnectorEnds`
  enumerations (same batch).
- [ms-vsdx] [MS-VSDX] v20250520 ([MS-VSDX] (source 3)): `Connect_Type`
  (2.3.4.2.12 and the section 3 schema), `Connects_Type`,
  `PageContents_Type`, the `BegTrigger`/`EndTrigger`/`GlueType` cell
  definitions.
- [corpus] Derived facts from the licensed corpus files
  (`corpus/vsdx/`, per `corpus/PROVENANCE.md`): the `Connect` record
  spellings, endpoint and trigger formulas, and the Dynamic connector
  master's cells, observed in saved files.
- [wide-corpus] The wide corpus (`docs/sources.md` row 26): the same
  formula spellings and cell-value counts, as aggregates; no file is
  named.

Raw pages are kept under `reference/` (never committed) and
recorded in `docs/sources.md` row 30 and `reference/SOURCES.md`.
