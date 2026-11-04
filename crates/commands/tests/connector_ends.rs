//! `connector.move_end` through the registry (task P7-33 item 4).
//!
//! One undoable move of one connector end: point glue writes the
//! `PAR(PNT(...))` formula and the row record, dynamic glue the
//! `_WALKGLUE(...)` formula and the pin record, a free end the literal
//! point with no record; one undo restores the cells, formulas and
//! records exactly. A missing required argument is a clear error with
//! no dialog request (AGENTS.md section 6). A connector inside a group
//! keeps its endpoint cells in its parent's coordinates: `x`,`y`
//! arrive in page inches and map through the parent's frame.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use support::dispatch;

use larrygram_commands::app::{AppCtx, DocumentSession, Settings};
use larrygram_commands::builtins::registry;
use larrygram_commands::{ArgValue, Args, CommandError, Registry};
use larrygram_geom::Point;
use larrygram_model::{
    Cell, CellName, CellValue, Connect, ConnectorEnd, Document, Page, PageId, Preserved, Row,
    Section, SectionName, Shape, ShapeId,
};

/// A cell with a literal formula.
fn cell(name: &str, value: f64) -> Cell {
    Cell {
        name: CellName::new(name),
        formula: Some(format!("{value}")),
        value: Some(CellValue::Number(value)),
        unit: larrygram_geom::Unit::Inches,
        preserved: Preserved::new(),
    }
}

/// A 2 x 1 in box at `(x, y)` with two connection points (the top and
/// bottom edge midpoints).
fn box_shape(visio_id: i32, x: f64, y: f64) -> Shape {
    let mut shape = Shape::new(ShapeId { index: 0 }, visio_id);
    shape.sheet.cells = vec![
        cell("Width", 2.0),
        cell("Height", 1.0),
        cell("PinX", x),
        cell("PinY", y),
        cell("LocPinX", 1.0),
        cell("LocPinY", 0.5),
        cell("Angle", 0.0),
    ];
    let mut connection = Section::new(SectionName::from_static("Connection"));
    for (ix, (px, py)) in [(1.0, 1.0), (1.0, 0.0)].into_iter().enumerate() {
        let mut row = Row::new();
        row.row_type = Some("Connection".to_owned());
        row.ix = Some(i32::try_from(ix).unwrap_or(0));
        row.cells = vec![cell("X", px), cell("Y", py)];
        connection.rows.push(row);
    }
    shape.sheet.sections.push(connection);
    shape
}

/// Two boxes on one page (Visio ids 1 and 2): box 1 at `(2, 2)` and
/// box 2 at `(6, 2)`.
fn fixture() -> (AppCtx, Registry, PageId) {
    let mut doc = Document::new();
    let page = doc.add_page("Page-1").expect("page");
    for (visio_id, x) in [(1, 2.0), (2, 6.0)] {
        doc.insert_shape(page, box_shape(visio_id, x, 2.0), None)
            .expect("insert");
    }
    let mut ctx = AppCtx::new(Settings::default());
    ctx.open_document(DocumentSession::new(doc, None));
    (ctx, registry().expect("registry"), page)
}

fn doc(ctx: &AppCtx) -> &Document {
    &ctx.active_document().expect("session").document
}

fn shape(ctx: &AppCtx, page: PageId, visio_id: i32) -> ShapeId {
    doc(ctx)
        .shape_by_visio_id(page, visio_id)
        .expect("shape by visio id")
}

/// A connector drawn free from `(1.5, 4.0)` to `(4.5, 5.0)`, returned
/// by its Visio id.
fn draw_free(ctx: &mut AppCtx, registry: &Registry) -> i32 {
    dispatch(
        ctx,
        registry,
        "connector.draw",
        Args::new()
            .with("stencil", ArgValue::Text("Basic Flowchart".to_owned()))
            .with("master", ArgValue::Text("Connector".to_owned()))
            .with("x", ArgValue::Number(3.0))
            .with("y", ArgValue::Number(4.5))
            .with("begin_x", ArgValue::Number(1.5))
            .with("begin_y", ArgValue::Number(4.0))
            .with("end_x", ArgValue::Number(4.5))
            .with("end_y", ArgValue::Number(5.0)),
    )
    .expect("draw free");
    let session = ctx.active_document().expect("session");
    let page = session.active_page.expect("page");
    let shapes = session.document.page(page).expect("page").shapes.clone();
    session
        .document
        .shape(*shapes.last().expect("the drawn connector"))
        .expect("connector")
        .visio_id
}

fn own(document: &Document, shape: ShapeId, name: &str) -> Option<Cell> {
    document
        .shape(shape)?
        .sheet
        .cells
        .iter()
        .find(|cell| cell.name.as_str() == name)
        .cloned()
}

fn formula(document: &Document, shape: ShapeId, name: &str) -> Option<String> {
    own(document, shape, name).and_then(|cell| cell.formula)
}

fn number(document: &Document, shape: ShapeId, name: &str) -> f64 {
    match own(document, shape, name).and_then(|cell| cell.value) {
        Some(CellValue::Number(value)) => value,
        _ => f64::NAN,
    }
}

fn end(document: &Document, shape: ShapeId, which: ConnectorEnd) -> Point {
    document.connector_end(shape, which).expect("end")
}

fn connects(ctx: &AppCtx, page: PageId) -> Vec<Connect> {
    doc(ctx).page(page).expect("page").connects.clone()
}

fn undo_len(ctx: &AppCtx) -> usize {
    ctx.active_document().expect("session").undo.len()
}

/// The cells and records an undo must restore exactly.
fn state(document: &Document, page: PageId) -> (Vec<(i32, Vec<Cell>)>, Vec<Connect>) {
    let entity = document.page(page).expect("page");
    let mut sheets = Vec::new();
    for shape in &entity.shapes {
        let found = document.shape(*shape).expect("shape");
        sheets.push((found.visio_id, found.sheet.cells.clone()));
    }
    (sheets, entity.connects.clone())
}

/// One `connector.move_end` dispatch with the common arguments.
fn move_end(
    ctx: &mut AppCtx,
    registry: &Registry,
    connector: i32,
    end: &str,
    x: f64,
    y: f64,
    target: Option<(i32, Option<i32>)>,
) -> Result<larrygram_commands::Outcome, CommandError> {
    let mut args = Args::new()
        .with("connector", ArgValue::Integer(i64::from(connector)))
        .with("end", ArgValue::Text(end.to_owned()))
        .with("x", ArgValue::Number(x))
        .with("y", ArgValue::Number(y));
    if let Some((target, point)) = target {
        args.set("target", ArgValue::Integer(i64::from(target)));
        if let Some(point) = point {
            args.set("point", ArgValue::Integer(i64::from(point)));
        }
    }
    dispatch(ctx, registry, "connector.move_end", args)
}

/// The endpoint cells agree with the drawn path (ADR 0084 rule 1).
fn cells_match_path(document: &Document, shape: ShapeId) {
    let geometry = document.shape_geometry(shape).expect("geometry");
    let rows = &geometry.paths.first().expect("a path").rows;
    let point = |row: &larrygram_model::GeometryRow| match row {
        larrygram_model::GeometryRow::MoveTo { x, y }
        | larrygram_model::GeometryRow::LineTo { x, y } => Some(Point::new(*x, *y)),
        _ => None,
    };
    let begin = rows.first().and_then(point).expect("a first point");
    let finish = rows.last().and_then(point).expect("a last point");
    let (pin_x, pin_y) = (
        number(document, shape, "PinX"),
        number(document, shape, "PinY"),
    );
    let (loc_x, loc_y) = (
        number(document, shape, "LocPinX"),
        number(document, shape, "LocPinY"),
    );
    let (sin, cos) = number(document, shape, "Angle").sin_cos();
    let to_page = |p: Point| {
        let (dx, dy) = (p.x - loc_x, p.y - loc_y);
        Point::new(pin_x + dx * cos - dy * sin, pin_y + dx * sin + dy * cos)
    };
    let begin = to_page(begin);
    let finish = to_page(finish);
    let close = |a: Point, b: Point| (a.x - b.x).abs() < 1e-6 && (a.y - b.y).abs() < 1e-6;
    assert!(
        close(begin, end(document, shape, ConnectorEnd::Begin)),
        "the begin cells agree with the path"
    );
    assert!(
        close(finish, end(document, shape, ConnectorEnd::End)),
        "the end cells agree with the path"
    );
}

// ---------------------------------------------------------------------------
// The three glue kinds
// ---------------------------------------------------------------------------

#[test]
fn point_glue_writes_the_formula_the_value_and_the_record() {
    let (mut ctx, registry, page) = fixture();
    let connector = draw_free(&mut ctx, &registry);
    let before = state(doc(&ctx), page);
    let steps = undo_len(&ctx);

    move_end(
        &mut ctx,
        &registry,
        connector,
        "end",
        6.0,
        2.5,
        Some((2, Some(0))),
    )
    .expect("point glue");

    assert_eq!(undo_len(&ctx), steps + 1, "one undo step");
    let drawn = shape(&ctx, page, connector);
    assert_eq!(
        formula(doc(&ctx), drawn, "EndX"),
        Some("PAR(PNT(Sheet.2!Connections.X1,Sheet.2!Connections.Y1))".to_owned()),
        "point glue, as Visio stores it"
    );
    assert_eq!(
        formula(doc(&ctx), drawn, "EndY"),
        Some("PAR(PNT(Sheet.2!Connections.X1,Sheet.2!Connections.Y1))".to_owned())
    );
    assert_eq!(
        formula(doc(&ctx), drawn, "EndTrigger"),
        Some("_XFTRIGGER(Sheet.2!EventXFMod)".to_owned())
    );
    // The end lands on the row's page point: box 2's top midpoint.
    assert_eq!(
        end(doc(&ctx), drawn, ConnectorEnd::End),
        Point::new(6.0, 2.5)
    );
    let record = connects(&ctx, page)
        .into_iter()
        .find(|record| record.from_cell == "EndX")
        .expect("the end's record");
    assert_eq!(record.to_sheet, 2);
    assert_eq!(record.to_cell, "Connections.X1");
    assert_eq!(record.to_part, 100);
    cells_match_path(doc(&ctx), drawn);

    dispatch(&mut ctx, &registry, "edit.undo", Args::new()).expect("undo");
    assert_eq!(state(doc(&ctx), page), before, "one undo restores exactly");
}

#[test]
fn dynamic_glue_writes_the_walk_formula_and_the_pin_record() {
    let (mut ctx, registry, page) = fixture();
    let connector = draw_free(&mut ctx, &registry);
    let before = state(doc(&ctx), page);

    move_end(
        &mut ctx,
        &registry,
        connector,
        "end",
        6.0,
        2.6,
        Some((2, None)),
    )
    .expect("dynamic");

    let drawn = shape(&ctx, page, connector);
    assert_eq!(
        formula(doc(&ctx), drawn, "EndX"),
        Some("_WALKGLUE(EndTrigger,BegTrigger,WalkPreference)".to_owned()),
        "dynamic glue, as Visio stores it"
    );
    assert_eq!(
        formula(doc(&ctx), drawn, "EndTrigger"),
        Some("_XFTRIGGER(Sheet.2!EventXFMod)".to_owned())
    );
    // The end walks to the candidate nearest the other end (the begin
    // sits at (1.5, 4)): box 2's top midpoint.
    assert_eq!(
        end(doc(&ctx), drawn, ConnectorEnd::End),
        Point::new(6.0, 2.5)
    );
    let record = connects(&ctx, page)
        .into_iter()
        .find(|record| record.from_cell == "EndX")
        .expect("the end's record");
    assert_eq!(record.to_sheet, 2);
    assert_eq!(record.to_cell, "PinX");
    assert_eq!(record.to_part, 3);
    cells_match_path(doc(&ctx), drawn);

    dispatch(&mut ctx, &registry, "edit.undo", Args::new()).expect("undo");
    assert_eq!(state(doc(&ctx), page), before, "one undo restores exactly");
}

#[test]
fn a_free_end_lands_at_its_point_with_literal_cells() {
    let (mut ctx, registry, page) = fixture();
    let connector = draw_free(&mut ctx, &registry);
    // Glue the end first, then free it: the formula goes with the record.
    move_end(
        &mut ctx,
        &registry,
        connector,
        "end",
        6.0,
        2.5,
        Some((2, Some(1))),
    )
    .expect("glue");
    let before = state(doc(&ctx), page);

    move_end(&mut ctx, &registry, connector, "end", 5.0, 5.5, None).expect("free");

    let drawn = shape(&ctx, page, connector);
    assert_eq!(formula(doc(&ctx), drawn, "EndX"), None, "a literal point");
    assert_eq!(formula(doc(&ctx), drawn, "EndY"), None, "a literal point");
    assert_eq!(
        formula(doc(&ctx), drawn, "EndTrigger"),
        Some("_XFTRIGGER(EventXFMod)".to_owned()),
        "the trigger names its own sheet"
    );
    assert_eq!(
        end(doc(&ctx), drawn, ConnectorEnd::End),
        Point::new(5.0, 5.5),
        "free at the release point"
    );
    assert!(
        connects(&ctx, page)
            .iter()
            .all(|record| record.from_cell != "EndX"),
        "the record goes with the glue"
    );
    // The other end keeps its own state (it was free all along).
    assert_eq!(formula(doc(&ctx), drawn, "BeginX"), None);
    cells_match_path(doc(&ctx), drawn);

    dispatch(&mut ctx, &registry, "edit.undo", Args::new()).expect("undo");
    assert_eq!(state(doc(&ctx), page), before, "one undo restores exactly");
    dispatch(&mut ctx, &registry, "edit.redo", Args::new()).expect("redo");
    assert_eq!(
        end(doc(&ctx), shape(&ctx, page, connector), ConnectorEnd::End),
        Point::new(5.0, 5.5),
        "redo re-applies the free point exactly"
    );
}

#[test]
fn the_other_end_keeps_its_glue_when_one_end_moves() {
    let (mut ctx, registry, page) = fixture();
    let connector = draw_free(&mut ctx, &registry);
    move_end(
        &mut ctx,
        &registry,
        connector,
        "begin",
        2.0,
        2.5,
        Some((1, Some(0))),
    )
    .expect("point glue on the begin");
    let begin_record = connects(&ctx, page)
        .into_iter()
        .find(|record| record.from_cell == "BeginX")
        .expect("the begin's record");

    move_end(&mut ctx, &registry, connector, "end", 5.0, 5.5, None).expect("free end");

    let drawn = shape(&ctx, page, connector);
    assert_eq!(
        formula(doc(&ctx), drawn, "BeginX"),
        Some("PAR(PNT(Sheet.1!Connections.X1,Sheet.1!Connections.Y1))".to_owned()),
        "the begin keeps its formula"
    );
    let record = connects(&ctx, page)
        .into_iter()
        .find(|record| record.from_cell == "BeginX")
        .expect("the begin's record");
    assert_eq!(record, begin_record, "the begin's record is untouched");
    assert_eq!(
        end(doc(&ctx), drawn, ConnectorEnd::Begin),
        Point::new(2.0, 2.5)
    );
}

// ---------------------------------------------------------------------------
// The argument contract (item 4) and the group frame
// ---------------------------------------------------------------------------

#[test]
fn a_missing_argument_is_an_error_with_no_dialog_request() {
    let (mut ctx, registry, _page) = fixture();
    let connector = draw_free(&mut ctx, &registry);
    let full: Vec<(&str, ArgValue)> = vec![
        ("connector", ArgValue::Integer(i64::from(connector))),
        ("end", ArgValue::Text("end".to_owned())),
        ("x", ArgValue::Number(5.0)),
        ("y", ArgValue::Number(5.0)),
    ];
    for missing in 0..full.len() {
        let mut args = Args::new();
        for (index, (name, value)) in full.iter().enumerate() {
            if index != missing {
                args.set(*name, value.clone());
            }
        }
        let steps = undo_len(&ctx);
        let error = dispatch(&mut ctx, &registry, "connector.move_end", args)
            .expect_err("a missing argument fails");
        assert!(
            !matches!(error, CommandError::NeedsInput(_)),
            "{:?} must not ask for input",
            full[missing].0
        );
        assert!(
            matches!(error, CommandError::InvalidArgs { .. }),
            "{:?}: {error}",
            full[missing].0
        );
        assert_eq!(undo_len(&ctx), steps, "no undo step for a failed run");
    }

    // A wrong `end` spelling and a point without a target are argument
    // errors too.
    for args in [
        Args::new()
            .with("connector", ArgValue::Integer(i64::from(connector)))
            .with("end", ArgValue::Text("middle".to_owned()))
            .with("x", ArgValue::Number(5.0))
            .with("y", ArgValue::Number(5.0)),
        Args::new()
            .with("connector", ArgValue::Integer(i64::from(connector)))
            .with("end", ArgValue::Text("end".to_owned()))
            .with("x", ArgValue::Number(5.0))
            .with("y", ArgValue::Number(5.0))
            .with("point", ArgValue::Integer(0)),
    ] {
        let error = dispatch(&mut ctx, &registry, "connector.move_end", args)
            .expect_err("an argument problem fails");
        assert!(matches!(error, CommandError::InvalidArgs { .. }), "{error}");
    }

    // An unknown connector id is an argument error, an ordinary shape
    // has no connector ends, and a 1-D target is refused (connectors
    // are never glue targets).
    let error = move_end(&mut ctx, &registry, 99, "end", 5.0, 5.0, None).expect_err("unknown id");
    assert!(matches!(error, CommandError::InvalidArgs { .. }), "{error}");
    let error = move_end(&mut ctx, &registry, 1, "end", 5.0, 5.0, None)
        .expect_err("an ordinary shape has no ends");
    assert!(matches!(error, CommandError::InvalidArgs { .. }), "{error}");
    let error = move_end(
        &mut ctx,
        &registry,
        connector,
        "end",
        5.0,
        5.0,
        Some((connector, None)),
    )
    .expect_err("a connector is not a target");
    assert!(matches!(error, CommandError::InvalidArgs { .. }), "{error}");
}

/// A connector inside a translated group: the endpoint cells are in
/// the parent's coordinates (ADR 0084 rule 2 as amended), so `x`,`y`
/// in page inches map through the parent's frame.
#[test]
fn a_connector_inside_a_group_maps_the_point_through_the_parent_frame() {
    let mut document = Document::new();
    let page = document.add_page("Page-1").expect("page");
    // The group: pin (3, 3), local pin (0, 0): its frame translates
    // its children by (3, 3).
    let mut group = box_shape(1, 3.0, 3.0);
    group.sheet.cells = vec![
        cell("Width", 2.0),
        cell("Height", 1.0),
        cell("PinX", 3.0),
        cell("PinY", 3.0),
        cell("LocPinX", 0.0),
        cell("LocPinY", 0.0),
        cell("Angle", 0.0),
    ];
    let group = document.add_shape(group);
    // The connector member: a 1-D shape with endpoint cells in the
    // parent's coordinates (page (2, 4)..(4, 4) minus the group's
    // (3, 3) offset).
    let mut connector = Shape::new(ShapeId { index: 0 }, 2);
    connector.sheet.cells = vec![
        cell("Width", 2.0),
        cell("Height", 0.2),
        cell("PinX", 0.5),
        cell("PinY", 1.0),
        cell("LocPinX", 1.0),
        cell("LocPinY", 0.1),
        cell("Angle", 0.0),
        cell("BeginX", -1.0),
        cell("BeginY", 1.0),
        cell("EndX", 1.0),
        cell("EndY", 1.0),
    ];
    let mut geometry = Section::new(SectionName::from_static("Geometry"));
    for (ix, (kind, px, py)) in [("MoveTo", -1.0, 1.0), ("LineTo", 1.0, 1.0)]
        .into_iter()
        .enumerate()
    {
        let mut row = Row::new();
        row.row_type = Some(kind.to_owned());
        row.ix = Some(i32::try_from(ix).unwrap_or(0));
        row.cells = vec![cell("X", px), cell("Y", py)];
        geometry.rows.push(row);
    }
    connector.sheet.sections.push(geometry);
    let connector = document.add_shape(connector);
    if let Some(entity) = document.shape_mut(group) {
        entity.children.push(connector);
    }
    if let Some(entity) = document.shape_mut(connector) {
        entity.parent = Some(group);
    }
    if let Some(entity) = document.pages.get_mut(page.index as usize) {
        entity.shapes.push(group);
    }
    let mut ctx = AppCtx::new(Settings::default());
    ctx.open_document(DocumentSession::new(document, None));
    let registry = registry().expect("registry");

    // A free end at page (7, 8): the cells hold the parent's
    // coordinates, (4, 5).
    move_end(&mut ctx, &registry, 2, "end", 7.0, 8.0, None).expect("free end");
    let moved = shape(&ctx, page, 2);
    assert_eq!(number(doc(&ctx), moved, "EndX"), 4.0, "through the frame");
    assert_eq!(number(doc(&ctx), moved, "EndY"), 5.0, "through the frame");
    assert_eq!(
        end(doc(&ctx), moved, ConnectorEnd::End),
        Point::new(4.0, 5.0),
        "the parent's coordinates"
    );
    cells_match_path(doc(&ctx), moved);
}

// ---------------------------------------------------------------------------
// Model edges the end writes read through (the coverage ratchets: these
// paths are what `connector.move_end`'s lookup, row and cell reads
// take on odd input)
// ---------------------------------------------------------------------------

/// A `Connection` row whose `X`/`Y` cells are not numbers reads as no
/// point (model `connection.rs`), and a row whose `DirX`/`DirY` cells
/// are not numbers reads as no direction (the resolver's row read).
#[test]
fn a_connection_row_with_non_number_cells_reads_as_nothing() {
    let mut document = Document::new();
    let page = document.add_page("Page-1").expect("page");
    let mut target = box_shape(1, 2.0, 2.0);
    // One row with a text `Y` (no point) and one with a text `DirX`
    // (a point, but no direction on that axis).
    target.sheet.sections.clear();
    let mut connection = Section::new(SectionName::from_static("Connection"));
    let mut bad = Row::new();
    bad.row_type = Some("Connection".to_owned());
    bad.ix = Some(0);
    let mut cells = vec![cell("X", 1.0)];
    let mut text = Cell {
        name: CellName::new("Y"),
        formula: None,
        value: Some(CellValue::Str("east".to_owned())),
        unit: larrygram_geom::Unit::Inches,
        preserved: Preserved::new(),
    };
    cells.push(text.clone());
    bad.cells = cells;
    connection.rows.push(bad);
    let mut good = Row::new();
    good.row_type = Some("Connection".to_owned());
    good.ix = Some(1);
    text.name = CellName::new("DirX");
    good.cells = vec![cell("X", 1.0), cell("Y", 0.0), text];
    connection.rows.push(good);
    target.sheet.sections.push(connection);
    document.insert_shape(page, target, None).expect("target");
    let mut ctx = AppCtx::new(Settings::default());
    ctx.open_document(DocumentSession::new(document, None));
    let registry = registry().expect("registry");

    // The row with the text `Y` has no point and drops out; the row
    // with the text `DirX` keeps its point and glues.
    let target = shape(&ctx, page, 1);
    let points = doc(&ctx).connection_points_indexed(target);
    assert_eq!(points, vec![(1, Point::new(1.0, 0.0))], "{points:?}");

    // A connector drawn free, then point-glued to the good row.
    let connector = draw_free(&mut ctx, &registry);
    move_end(
        &mut ctx,
        &registry,
        connector,
        "end",
        3.0,
        2.0,
        Some((1, Some(1))),
    )
    .expect("point glue");
    let subject = shape(&ctx, page, connector);
    assert_eq!(
        (
            number(doc(&ctx), subject, "EndX"),
            number(doc(&ctx), subject, "EndY"),
        ),
        (2.0, 1.5),
        "the row's local point through the box transform"
    );
}

/// The page walk skips a page entry whose shape id no longer exists
/// (model `document.rs`), so `connector.move_end` still finds its
/// connector beside the dangling entry.
#[test]
fn a_dangling_page_entry_does_not_break_the_connector_lookup() {
    let (mut ctx, registry, page) = fixture();
    let connector = draw_free(&mut ctx, &registry);
    if let Some(session) = ctx.active_document_mut()
        && let Some(entity) = session.document.pages.get_mut(page.index as usize)
    {
        entity.shapes.push(ShapeId { index: 9999 });
    }
    move_end(&mut ctx, &registry, connector, "end", 5.0, 5.5, None).expect("free end");
    let subject = shape(&ctx, page, connector);
    assert_eq!(number(doc(&ctx), subject, "EndX"), 5.0);
}

/// A page sheet whose `PageWidth` is `Inh` defines no page size (model
/// `lookup.rs`): the documented default stands.
#[test]
fn an_inherit_page_size_cell_defines_nothing() {
    let mut document = Document::new();
    let mut page = Page::new(PageId { index: 0 }, 1);
    let mut width = cell("PageWidth", 0.0);
    width.formula = Some("Inh".to_owned());
    width.value = None;
    page.sheet.cells = vec![width, cell("PageHeight", 11.0)];
    document.pages.push(page);
    assert_eq!(document.page_size(PageId { index: 0 }), (8.5, 11.0));
}

/// The strict argument contract's remaining edges: a negative point
/// and a non-numeric one are argument errors with no dialog request.
#[test]
fn a_bad_point_is_an_argument_error_with_no_dialog_request() {
    let (mut ctx, registry, _page) = fixture();
    let connector = draw_free(&mut ctx, &registry);
    for point in [-1, 9] {
        let mut args = Args::new()
            .with("connector", ArgValue::Integer(i64::from(connector)))
            .with("end", ArgValue::Text("end".to_owned()))
            .with("x", ArgValue::Number(5.0))
            .with("y", ArgValue::Number(5.0))
            .with("target", ArgValue::Integer(2))
            .with("point", ArgValue::Integer(i64::from(point)));
        let error = dispatch(&mut ctx, &registry, "connector.move_end", args)
            .expect_err("a bad point fails");
        assert!(
            !matches!(error, CommandError::NeedsInput(_)),
            "{point} must not ask for input"
        );
        assert!(
            matches!(error, CommandError::InvalidArgs { .. }),
            "{point}: {error}"
        );
    }
}
