//! The connection-point commands through the registry (task P7-34):
//! `connector.point_add`, `connector.point_move`, `connector.point_delete`
//! and `edit.connection_point.*`, each one undoable edit.
//!
//! The delete's glue cases (item 4) with the formulas and records
//! checked: an end glued to the deleted point re-glues dynamically
//! (`_WALKGLUE(…)` formula, `PinX` record), and a record naming
//! `Connections.X<k+1>` and up, with its `PAR(PNT(…))` formula, moves
//! down by one so no end changes target. One undo restores the exact
//! rows, formulas and records.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use support::dispatch;

use larrygram_commands::app::{ActiveTool, AppCtx, DocumentSession, Settings};
use larrygram_commands::builtins::registry;
use larrygram_commands::{ArgValue, Args, CommandError, Registry};
use larrygram_geom::Point;
use larrygram_model::{
    Cell, CellName, CellValue, Connect, Document, PageId, Preserved, Row, Section, SectionName,
    Shape, ShapeId,
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

/// A 2 x 1 in box at `(x, y)` with two connection points (the local
/// `(1, 1)` and `(1, 0)`, i.e. the top and bottom edge midpoints).
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

/// One box on one page (Visio id 1) at `(2, 2)`.
fn fixture() -> (AppCtx, Registry, PageId) {
    let mut doc = Document::new();
    let page = doc.add_page("Page-1").expect("page");
    doc.insert_shape(page, box_shape(1, 2.0, 2.0), None)
        .expect("insert");
    let mut ctx = AppCtx::new(Settings::default());
    ctx.open_document(DocumentSession::new(doc, None));
    (ctx, registry().expect("registry"), page)
}

fn doc(ctx: &AppCtx) -> &Document {
    &ctx.active_document().expect("session").document
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

/// Glues `connector`'s begin to connection point `point` of `target`.
fn glue_begin(ctx: &mut AppCtx, registry: &Registry, connector: i32, target: i32, point: i32) {
    dispatch(
        ctx,
        registry,
        "connector.glue_begin",
        Args::new()
            .with("connector", ArgValue::Integer(i64::from(connector)))
            .with("target", ArgValue::Integer(i64::from(target)))
            .with("point", ArgValue::Integer(i64::from(point))),
    )
    .expect("glue begin");
}

fn shape(ctx: &AppCtx, page: PageId, visio_id: i32) -> ShapeId {
    doc(ctx)
        .shape_by_visio_id(page, visio_id)
        .expect("shape by visio id")
}

fn formula(document: &Document, shape: ShapeId, name: &str) -> Option<String> {
    document
        .shape(shape)?
        .sheet
        .cells
        .iter()
        .find(|cell| cell.name.as_str() == name)
        .and_then(|cell| cell.formula.clone())
}

fn number(document: &Document, shape: ShapeId, name: &str) -> f64 {
    match document.shape(shape).and_then(|entity| {
        entity
            .sheet
            .cells
            .iter()
            .find(|cell| cell.name.as_str() == name)
            .and_then(|cell| cell.value.clone())
    }) {
        Some(CellValue::Number(value)) => value,
        _ => f64::NAN,
    }
}

fn connects(ctx: &AppCtx, page: PageId) -> Vec<Connect> {
    doc(ctx).page(page).expect("page").connects.clone()
}

/// The shape's own `Connection` rows (the exact rows an undo restores).
fn connection_rows(document: &Document, shape: ShapeId) -> Vec<Row> {
    let entity = document.shape(shape).expect("shape");
    let mut rows = Vec::new();
    for section in &entity.sheet.sections {
        if section.name.as_str() == "Connection" {
            rows.extend(section.rows.iter().cloned());
        }
    }
    rows
}

/// The state one undo must restore exactly.
fn state(ctx: &AppCtx, page: PageId, shapes: &[ShapeId]) -> (Vec<(i32, Vec<Row>)>, Vec<Connect>) {
    let mut rows = Vec::new();
    for shape in shapes {
        let visio = doc(ctx).shape(*shape).expect("shape").visio_id;
        rows.push((visio, connection_rows(doc(ctx), *shape)));
    }
    (rows, connects(ctx, page))
}

fn undo(ctx: &mut AppCtx, registry: &Registry) {
    dispatch(ctx, registry, "edit.undo", Args::new()).expect("undo");
}

#[test]
fn add_move_set_type_and_delete_write_the_rows_and_undo_restores_them() {
    let (mut ctx, registry, page) = fixture();
    let box_id = shape(&ctx, page, 1);
    let before = state(&ctx, page, &[box_id]);

    // Add: one undoable edit, the row reads back at the local point.
    dispatch(
        &mut ctx,
        &registry,
        "connector.point_add",
        Args::new()
            .with("shape", ArgValue::Integer(1))
            .with("x", ArgValue::Number(0.5))
            .with("y", ArgValue::Number(0.25)),
    )
    .expect("add");
    let rows = connection_rows(doc(&ctx), box_id);
    assert_eq!(rows.len(), 3, "the two fixture rows plus the new one");
    let new_row = rows.last().expect("the new row");
    assert_eq!(new_row.ix, Some(2));
    assert_eq!(doc(&ctx).connection_points(box_id).len(), 3);

    // Move: the point follows and the other rows stay.
    dispatch(
        &mut ctx,
        &registry,
        "connector.point_move",
        Args::new()
            .with("shape", ArgValue::Integer(1))
            .with("point", ArgValue::Integer(2))
            .with("x", ArgValue::Number(0.75))
            .with("y", ArgValue::Number(0.5)),
    )
    .expect("move");
    assert_eq!(
        doc(&ctx).connection_points(box_id)[2],
        Point::new(0.75, 0.5)
    );

    // Type: the `Type` cell of the row.
    dispatch(
        &mut ctx,
        &registry,
        "edit.connection_point.outward",
        Args::new()
            .with("shape", ArgValue::Integer(1))
            .with("point", ArgValue::Integer(2)),
    )
    .expect("type");
    let rows = connection_rows(doc(&ctx), box_id);
    let type_cell = rows
        .last()
        .expect("row")
        .cells
        .iter()
        .find(|cell| cell.name.as_str() == "Type")
        .expect("the Type cell");
    assert_eq!(type_cell.value, Some(CellValue::Number(1.0)));

    // Delete, then one undo per step: every undo restores the exact
    // rows.
    dispatch(
        &mut ctx,
        &registry,
        "connector.point_delete",
        Args::new()
            .with("shape", ArgValue::Integer(1))
            .with("point", ArgValue::Integer(2)),
    )
    .expect("delete");
    assert_eq!(connection_rows(doc(&ctx), box_id).len(), 2);

    undo(&mut ctx, &registry);
    assert_eq!(
        connection_rows(doc(&ctx), box_id).len(),
        3,
        "the row is back"
    );
    undo(&mut ctx, &registry);
    undo(&mut ctx, &registry);
    undo(&mut ctx, &registry);
    assert_eq!(
        state(&ctx, page, &[box_id]),
        before,
        "four undos restore the exact rows"
    );
}

#[test]
fn moving_a_point_moves_the_ends_glued_to_it() {
    let (mut ctx, registry, page) = fixture();
    let connector = draw_free(&mut ctx, &registry);
    glue_begin(&mut ctx, &registry, connector, 1, 0);
    let connector_id = shape(&ctx, page, connector);
    let before = state(&ctx, page, &[shape(&ctx, page, 1), connector_id]);

    // The fixture point 0 is the local `(1, 1)` of a 2 x 1 box pinned
    // at `(2, 2)`, i.e. page `(2, 2.5)`.
    assert_eq!(number(doc(&ctx), connector_id, "BeginX"), 2.0);
    assert_eq!(number(doc(&ctx), connector_id, "BeginY"), 2.5);
    let glued = formula(doc(&ctx), connector_id, "BeginX").expect("the glue formula");
    assert!(
        glued.starts_with("PAR(PNT("),
        "point glue spells PAR(PNT(…)): {glued}"
    );

    // Move the point to the local `(0.5, 0.5)`, i.e. page `(1.5, 2)`.
    dispatch(
        &mut ctx,
        &registry,
        "connector.point_move",
        Args::new()
            .with("shape", ArgValue::Integer(1))
            .with("point", ArgValue::Integer(0))
            .with("x", ArgValue::Number(0.5))
            .with("y", ArgValue::Number(0.5)),
    )
    .expect("move");

    assert_eq!(
        formula(doc(&ctx), connector_id, "BeginX").as_deref(),
        Some(glued.as_str()),
        "the glue formula stays"
    );
    assert_eq!(
        number(doc(&ctx), connector_id, "BeginX"),
        1.5,
        "the glued end follows the point"
    );
    assert_eq!(number(doc(&ctx), connector_id, "BeginY"), 2.0);

    // One undo restores the point and the end together.
    undo(&mut ctx, &registry);
    assert_eq!(number(doc(&ctx), connector_id, "BeginX"), 2.0);
    assert_eq!(number(doc(&ctx), connector_id, "BeginY"), 2.5);
    assert_eq!(
        state(&ctx, page, &[shape(&ctx, page, 1), connector_id]),
        before
    );
}

#[test]
fn deleting_a_point_re_glues_its_ends_and_renumbers_the_later_rows() {
    let (mut ctx, registry, page) = fixture();
    let first = draw_free(&mut ctx, &registry);
    glue_begin(&mut ctx, &registry, first, 1, 0);
    let second = draw_free(&mut ctx, &registry);
    glue_begin(&mut ctx, &registry, second, 1, 1);
    let first_id = shape(&ctx, page, first);
    let second_id = shape(&ctx, page, second);
    let box_id = shape(&ctx, page, 1);
    let before = state(&ctx, page, &[box_id, first_id, second_id]);

    // Both begins glue to points of shape 1 (`Connections.X1` and
    // `Connections.X2`, one-based row numbers).
    let begin_of = |records: &[Connect], visio: i32| {
        records
            .iter()
            .find(|record| record.from_sheet == visio)
            .expect("the begin record")
            .clone()
    };
    assert_eq!(
        begin_of(&connects(&ctx, page), first).to_cell,
        "Connections.X1"
    );
    assert_eq!(
        begin_of(&connects(&ctx, page), second).to_cell,
        "Connections.X2"
    );

    dispatch(
        &mut ctx,
        &registry,
        "connector.point_delete",
        Args::new()
            .with("shape", ArgValue::Integer(1))
            .with("point", ArgValue::Integer(0)),
    )
    .expect("delete");

    // The rows move down by one.
    let rows = connection_rows(doc(&ctx), box_id);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].ix, Some(0), "the later row moved down by one");

    // Case 2: the end glued to the deleted point re-glues dynamically
    // (the formulas and records ADR 0084 rule 2 stores).
    let first_formula = formula(doc(&ctx), first_id, "BeginX").expect("the begin formula");
    assert!(
        first_formula.starts_with("_WALKGLUE("),
        "dynamic glue spells _WALKGLUE(…): {first_formula}"
    );
    let first_trigger = formula(doc(&ctx), first_id, "BegTrigger").expect("the trigger");
    assert!(
        first_trigger.starts_with("_XFTRIGGER("),
        "the trigger names the target: {first_trigger}"
    );
    let record = begin_of(&connects(&ctx, page), first);
    assert_eq!(record.to_cell, "PinX", "dynamic glue names the pin");
    assert_eq!(record.to_part, 3, "ToPart is the whole shape");
    assert!(
        number(doc(&ctx), first_id, "BeginX").is_finite(),
        "the freed end keeps a finite point"
    );

    // Case 3: the later row's record and formula move down by one.
    let record = begin_of(&connects(&ctx, page), second);
    assert_eq!(record.to_cell, "Connections.X1");
    assert_eq!(record.to_part, 100, "the zero-based row index");
    let second_formula = formula(doc(&ctx), second_id, "BeginX").expect("the begin formula");
    assert!(
        second_formula.contains("Connections.X1") && second_formula.contains("Connections.Y1"),
        "the formula names the moved-down row: {second_formula}"
    );

    // One undo restores the rows, formulas and records exactly.
    undo(&mut ctx, &registry);
    assert_eq!(
        state(&ctx, page, &[box_id, first_id, second_id]),
        before,
        "one undo restores the exact rows, formulas and records"
    );
}

#[test]
fn stale_arguments_are_errors_with_no_dialog_request() {
    let (mut ctx, registry, _page) = fixture();

    let error = dispatch(
        &mut ctx,
        &registry,
        "connector.point_add",
        Args::new()
            .with("shape", ArgValue::Integer(99))
            .with("x", ArgValue::Number(0.0))
            .with("y", ArgValue::Number(0.0)),
    )
    .expect_err("a stale shape id");
    assert!(
        matches!(error, CommandError::InvalidArgs { .. }),
        "{error:?}"
    );

    let error = dispatch(
        &mut ctx,
        &registry,
        "connector.point_delete",
        Args::new()
            .with("shape", ArgValue::Integer(1))
            .with("point", ArgValue::Integer(9)),
    )
    .expect_err("a stale point index");
    assert!(matches!(error, CommandError::Failed { .. }), "{error:?}");

    let error = dispatch(
        &mut ctx,
        &registry,
        "connector.point_move",
        Args::new()
            .with("shape", ArgValue::Integer(1))
            .with("point", ArgValue::Integer(0)),
    )
    .expect_err("a missing coordinate");
    assert!(matches!(error, CommandError::NeedsInput(_)), "{error:?}");
}

#[test]
fn the_tool_and_the_checks_take_their_window_state() {
    let (mut ctx, registry, _page) = fixture();

    dispatch(&mut ctx, &registry, "tools.connection.point", Args::new()).expect("arm the tool");
    assert_eq!(ctx.active_tool, ActiveTool::ConnectionPoint);

    assert!(!ctx.show_connection_points);
    dispatch(&mut ctx, &registry, "tools.connection.points", Args::new()).expect("flip the check");
    assert!(ctx.show_connection_points);
    dispatch(&mut ctx, &registry, "view.connections", Args::new()).expect("flip the same check");
    assert!(
        !ctx.show_connection_points,
        "both ids read and write the same state"
    );
}
