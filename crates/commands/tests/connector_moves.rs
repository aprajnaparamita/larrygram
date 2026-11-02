//! 1-D shapes under move, copy and delete (task P7-39, ADR 0084 rule 1
//! as amended on 2026-11-03).
//!
//! The endpoint cells are a 1-D shape's position: a move translates
//! them and re-writes the path, unglueing each end whose target does
//! not move with it; align and distribute leave 1-D shapes alone;
//! resize, rotate and flip refuse them with no undo step; duplicate
//! and paste offset the endpoint cells and re-glue an end only to a
//! copied target; a drop from the Shapes window places the ends from
//! the drop point. Deleting a group clears the glue of every connector
//! glued to one of its members. A glued 1-D shape that is not routable
//! stays a straight line. Every edit's single undo restores the cells
//! and the records exactly, and no edit leaves the endpoint cells
//! disagreeing with the drawn path.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use support::dispatch;

use larrygram_commands::app::{AppCtx, DocumentSession, Settings};
use larrygram_commands::builtins::registry;
use larrygram_commands::{ArgValue, Args, Registry};
use larrygram_geom::Point;
use larrygram_model::{
    Cell, CellLookup, CellName, CellValue, Connect, ConnectorEnd, Document, GeometryRow, PageId,
    Preserved, Row, Section, SectionName, Shape, ShapeId, mutate as model_mutate,
};
use larrygram_vsdx::{Limits, Package, read_document, write_document};

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

fn snapshot(ctx: &AppCtx) -> Document {
    doc(ctx).clone()
}

fn undo_len(ctx: &AppCtx) -> usize {
    ctx.active_document().expect("session").undo.len()
}

fn select(ctx: &mut AppCtx, shapes: &[ShapeId]) {
    ctx.selection.shapes = shapes.to_vec();
    ctx.selection.anchor = shapes.last().copied();
}

fn shape(ctx: &AppCtx, page: PageId, visio_id: i32) -> ShapeId {
    doc(ctx)
        .shape_by_visio_id(page, visio_id)
        .expect("shape by visio id")
}

/// The page's 1-D shapes, in page order.
fn connectors(document: &Document, page: PageId) -> Vec<ShapeId> {
    document
        .page(page)
        .expect("page")
        .shapes
        .iter()
        .copied()
        .filter(|shape| document.is_connector(*shape))
        .collect()
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

fn end(document: &Document, shape: ShapeId, which: ConnectorEnd) -> Point {
    document.connector_end(shape, which).expect("end")
}

fn connects(ctx: &AppCtx, page: PageId) -> Vec<Connect> {
    doc(ctx).page(page).expect("page").connects.clone()
}

/// A connector drawn free from `(1.5, 4.0)` to `(4.5, 5.0)`.
fn draw_free(ctx: &mut AppCtx, registry: &Registry) {
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
}

/// A connector glued with point glue to box 1 and dynamic glue to box
/// 2, at the re-route pass's fixed point (P7-32 owns the landing; from
/// a fixed point one undo restores exactly).
fn draw_glued(ctx: &mut AppCtx, registry: &Registry) {
    dispatch(
        ctx,
        registry,
        "connector.draw",
        Args::new()
            .with("stencil", ArgValue::Text("Basic Flowchart".to_owned()))
            .with("master", ArgValue::Text("Connector".to_owned()))
            .with("x", ArgValue::Number(2.0))
            .with("y", ArgValue::Number(2.5))
            .with("begin_target", ArgValue::Integer(1))
            .with("begin_point", ArgValue::Integer(0))
            .with("end_target", ArgValue::Integer(2)),
    )
    .expect("draw glued");
    dispatch(ctx, registry, "connector.reroute_all", Args::new()).expect("reroute");
}

/// The page transform of a top-level shape from its cached cells.
fn page_point(document: &Document, shape: ShapeId, local: Point) -> Point {
    let number = |name: &str| match own(document, shape, name).and_then(|cell| cell.value) {
        Some(CellValue::Number(value)) => value,
        _ => document
            .resolve(shape, &CellName::new(name))
            .ok()
            .and_then(|found| match found.value {
                Some(CellValue::Number(value)) => Some(*value),
                _ => None,
            })
            .unwrap_or(0.0),
    };
    let (pin_x, pin_y) = (number("PinX"), number("PinY"));
    let (loc_x, loc_y) = (number("LocPinX"), number("LocPinY"));
    let angle = number("Angle");
    let (dx, dy) = (local.x - loc_x, local.y - loc_y);
    let (sin, cos) = angle.sin_cos();
    Point::new(pin_x + dx * cos - dy * sin, pin_y + dx * sin + dy * cos)
}

/// The drawn path's points in page inches.
fn drawn(document: &Document, shape: ShapeId) -> Vec<Point> {
    let geometry = document.shape_geometry(shape).expect("geometry");
    geometry
        .paths
        .first()
        .expect("a path")
        .rows
        .iter()
        .filter_map(|row| match row {
            GeometryRow::MoveTo { x, y } | GeometryRow::LineTo { x, y } => {
                Some(page_point(document, shape, Point::new(*x, *y)))
            }
            _ => None,
        })
        .collect()
}

fn close(a: Point, b: Point) -> bool {
    (a.x - b.x).abs() < 1e-6 && (a.y - b.y).abs() < 1e-6
}

/// No edit may leave the endpoint cells disagreeing with the drawn path
/// (ADR 0084 rule 1, amended).
fn assert_cells_match_path(document: &Document, shape: ShapeId) {
    let path = drawn(document, shape);
    let begin = end(document, shape, ConnectorEnd::Begin);
    let finish = end(document, shape, ConnectorEnd::End);
    assert!(
        close(*path.first().expect("first"), begin),
        "begin cell {begin:?} vs drawn {path:?}"
    );
    assert!(
        close(*path.last().expect("last"), finish),
        "end cell {finish:?} vs drawn {path:?}"
    );
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

fn no_formula_names_a_sheet(document: &Document, shape: ShapeId) -> bool {
    document
        .shape(shape)
        .expect("shape")
        .sheet
        .cells
        .iter()
        .all(|cell| !cell.formula.as_deref().unwrap_or("").contains("Sheet."))
}

fn move_by(ctx: &mut AppCtx, registry: &Registry, dx: f64, dy: f64) {
    dispatch(
        ctx,
        registry,
        "shape.move",
        Args::new()
            .with("dx", ArgValue::Number(dx))
            .with("dy", ArgValue::Number(dy)),
    )
    .expect("move");
}

// ---------------------------------------------------------------------------
// Moves
// ---------------------------------------------------------------------------

#[test]
fn moving_a_free_connector_moves_its_endpoint_cells_and_its_path() {
    let (mut ctx, registry, page) = fixture();
    draw_free(&mut ctx, &registry);
    let before = snapshot(&ctx);
    let connector = connectors(&before, page)[0];
    let begin = end(&before, connector, ConnectorEnd::Begin);
    let finish = end(&before, connector, ConnectorEnd::End);
    select(&mut ctx, &[connector]);
    move_by(&mut ctx, &registry, 1.0, -0.5);

    let after = snapshot(&ctx);
    let moved = end(&after, connector, ConnectorEnd::Begin);
    assert!(
        close(moved, Point::new(begin.x + 1.0, begin.y - 0.5)),
        "{moved:?}"
    );
    let moved = end(&after, connector, ConnectorEnd::End);
    assert!(
        close(moved, Point::new(finish.x + 1.0, finish.y - 0.5)),
        "{moved:?}"
    );
    // The pin keeps its GUARD formula and follows the ends.
    assert_eq!(
        formula(&after, connector, "PinX"),
        Some("GUARD((BeginX+EndX)/2)".to_owned())
    );
    assert_cells_match_path(&after, connector);

    dispatch(&mut ctx, &registry, "edit.undo", Args::new()).expect("undo");
    assert_eq!(state(doc(&ctx), page), state(&before, page));
}

#[test]
fn moving_a_glued_connector_alone_unglues_both_ends_in_one_edit() {
    let (mut ctx, registry, page) = fixture();
    draw_glued(&mut ctx, &registry);
    let before = snapshot(&ctx);
    let connector = connectors(&before, page)[0];
    assert_eq!(connects(&ctx, page).len(), 2);
    let begin = end(&before, connector, ConnectorEnd::Begin);
    select(&mut ctx, &[connector]);
    move_by(&mut ctx, &registry, 0.0, 1.0);

    let after = snapshot(&ctx);
    assert!(connects(&ctx, page).is_empty(), "both records go");
    for name in ["BeginX", "BeginY", "EndX", "EndY"] {
        assert_eq!(formula(&after, connector, name), None, "{name} is literal");
    }
    for name in ["BegTrigger", "EndTrigger"] {
        assert_eq!(
            formula(&after, connector, name),
            Some("_XFTRIGGER(EventXFMod)".to_owned()),
            "{name} names its own sheet"
        );
    }
    assert!(close(
        end(&after, connector, ConnectorEnd::Begin),
        Point::new(begin.x, begin.y + 1.0)
    ));
    assert_cells_match_path(&after, connector);

    dispatch(&mut ctx, &registry, "edit.undo", Args::new()).expect("undo");
    assert_eq!(state(doc(&ctx), page), state(&before, page));
}

#[test]
fn moving_a_connector_with_both_targets_keeps_the_glue() {
    let (mut ctx, registry, page) = fixture();
    draw_glued(&mut ctx, &registry);
    let before = snapshot(&ctx);
    let connector = connectors(&before, page)[0];
    let (left, right) = (shape(&ctx, page, 1), shape(&ctx, page, 2));
    select(&mut ctx, &[connector, left, right]);
    move_by(&mut ctx, &registry, 0.5, 0.5);

    let after = snapshot(&ctx);
    assert_eq!(connects(&ctx, page).len(), 2, "both ends stay glued");
    assert_eq!(
        formula(&after, connector, "BeginX"),
        Some("PAR(PNT(Sheet.1!Connections.X1,Sheet.1!Connections.Y1))".to_owned())
    );
    assert_eq!(
        formula(&after, connector, "EndX"),
        Some("_WALKGLUE(EndTrigger,BegTrigger,WalkPreference)".to_owned())
    );
    // The point glue still lands on box 1's top midpoint, moved.
    assert!(close(
        end(&after, connector, ConnectorEnd::Begin),
        Point::new(2.5, 3.0)
    ));
    assert_cells_match_path(&after, connector);

    dispatch(&mut ctx, &registry, "edit.undo", Args::new()).expect("undo");
    assert_eq!(state(doc(&ctx), page), state(&before, page));
}

#[test]
fn moving_one_target_with_the_connector_frees_only_the_other_end() {
    let (mut ctx, registry, page) = fixture();
    draw_glued(&mut ctx, &registry);
    let connector = connectors(doc(&ctx), page)[0];
    let left = shape(&ctx, page, 1);
    select(&mut ctx, &[connector, left]);
    move_by(&mut ctx, &registry, 0.0, -1.0);
    let after = snapshot(&ctx);
    let records = connects(&ctx, page);
    assert_eq!(records.len(), 1, "{records:?}");
    assert_eq!(records[0].from_cell, "BeginX");
    assert!(formula(&after, connector, "BeginX").is_some());
    assert_eq!(formula(&after, connector, "EndX"), None);
    assert_cells_match_path(&after, connector);
}

#[test]
fn set_pin_moves_a_connector_by_its_midpoint() {
    let (mut ctx, registry, page) = fixture();
    draw_free(&mut ctx, &registry);
    let connector = connectors(doc(&ctx), page)[0];
    select(&mut ctx, &[connector]);
    dispatch(
        &mut ctx,
        &registry,
        "shape.set.pin",
        Args::new()
            .with("x", ArgValue::Number(5.0))
            .with("y", ArgValue::Number(6.0)),
    )
    .expect("set pin");
    let after = snapshot(&ctx);
    let begin = end(&after, connector, ConnectorEnd::Begin);
    let finish = end(&after, connector, ConnectorEnd::End);
    assert!(close(
        Point::new((begin.x + finish.x) / 2.0, (begin.y + finish.y) / 2.0),
        Point::new(5.0, 6.0)
    ));
    assert!(close(
        Point::new(finish.x - begin.x, finish.y - begin.y),
        Point::new(3.0, 1.0)
    ));
    assert_cells_match_path(&after, connector);
}

#[test]
fn align_and_distribute_leave_a_connector_where_it_is() {
    let (mut ctx, registry, page) = fixture();
    draw_free(&mut ctx, &registry);
    let connector = connectors(doc(&ctx), page)[0];
    let (left, right) = (shape(&ctx, page, 1), shape(&ctx, page, 2));
    let before = snapshot(&ctx);
    select(&mut ctx, &[connector, left, right]);
    for command in ["arrange.align.left", "arrange.distribute.horizontal"] {
        let outcome = dispatch(&mut ctx, &registry, command, Args::new()).expect(command);
        assert!(
            outcome
                .warnings
                .iter()
                .any(|line| line.contains("connector")),
            "{command}: {:?}",
            outcome.warnings
        );
        let after = snapshot(&ctx);
        assert_eq!(
            own(&after, connector, "BeginX"),
            own(&before, connector, "BeginX"),
            "{command}"
        );
        assert_eq!(
            drawn(&after, connector),
            drawn(&before, connector),
            "{command}"
        );
    }
    // A selection of the connector alone: nothing to align, no step.
    select(&mut ctx, &[connector]);
    let steps = undo_len(&ctx);
    let refused = dispatch(&mut ctx, &registry, "arrange.align.left", Args::new());
    assert!(refused.is_err());
    assert_eq!(undo_len(&ctx), steps);
}

#[test]
fn resize_rotate_and_flip_refuse_a_connector_with_no_undo_step() {
    let (mut ctx, registry, page) = fixture();
    draw_free(&mut ctx, &registry);
    let connector = connectors(doc(&ctx), page)[0];
    select(&mut ctx, &[connector]);
    let before = snapshot(&ctx);
    let steps = undo_len(&ctx);
    for (command, args) in [
        (
            "shape.resize",
            Args::new()
                .with("width", ArgValue::Number(5.0))
                .with("height", ArgValue::Number(1.0)),
        ),
        (
            "shape.rotate",
            Args::new().with("angle", ArgValue::Number(0.5)),
        ),
        (
            "shape.set.width",
            Args::new().with("width", ArgValue::Number(5.0)),
        ),
        (
            "shape.set.height",
            Args::new().with("height", ArgValue::Number(5.0)),
        ),
        (
            "shape.set.angle",
            Args::new().with("angle", ArgValue::Number(0.5)),
        ),
        ("arrange.rotate90.clockwise", Args::new()),
        ("arrange.object.rotate90", Args::new()),
        ("arrange.flip_horizontal", Args::new()),
        ("arrange.flip_vertical", Args::new()),
    ] {
        let refused = dispatch(&mut ctx, &registry, command, args);
        let message = format!("{:?}", refused.as_ref().err());
        assert!(refused.is_err(), "{command} refuses a 1-D shape");
        assert!(
            message.contains("connector or a line"),
            "{command}: {message}"
        );
        assert_eq!(undo_len(&ctx), steps, "{command} pushes no undo step");
        assert_eq!(state(doc(&ctx), page), state(&before, page), "{command}");
    }
    // A mixed selection: the box resizes, the connector is skipped.
    let left = shape(&ctx, page, 1);
    select(&mut ctx, &[connector, left]);
    let outcome = dispatch(
        &mut ctx,
        &registry,
        "shape.resize",
        Args::new()
            .with("width", ArgValue::Number(3.0))
            .with("height", ArgValue::Number(1.0)),
    )
    .expect("resize the box");
    assert!(outcome.warnings.iter().any(|line| line.contains("skipped")));
    assert_eq!(
        own(doc(&ctx), connector, "Width"),
        own(&before, connector, "Width")
    );
}

// ---------------------------------------------------------------------------
// Copies and drops
// ---------------------------------------------------------------------------

#[test]
fn duplicating_a_glued_connector_alone_frees_the_copy() {
    let (mut ctx, registry, page) = fixture();
    draw_glued(&mut ctx, &registry);
    let original = connectors(doc(&ctx), page)[0];
    select(&mut ctx, &[original]);
    dispatch(&mut ctx, &registry, "edit.duplicate", Args::new()).expect("duplicate");
    let after = snapshot(&ctx);
    let copy = *connectors(&after, page).last().expect("copy");
    assert_ne!(copy, original);
    let (from, to) = (
        end(&after, original, ConnectorEnd::Begin),
        end(&after, copy, ConnectorEnd::Begin),
    );
    assert!(
        close(to, Point::new(from.x + 0.1, from.y - 0.1)),
        "{from:?} {to:?}"
    );
    assert!(no_formula_names_a_sheet(&after, copy), "the copy is free");
    let copy_visio = after.shape(copy).expect("copy").visio_id;
    assert!(
        connects(&ctx, page)
            .iter()
            .all(|record| record.from_sheet != copy_visio)
    );
    assert_eq!(connects(&ctx, page).len(), 2, "the original keeps its glue");
    assert_cells_match_path(&after, copy);

    dispatch(&mut ctx, &registry, "edit.undo", Args::new()).expect("undo");
    assert_eq!(connectors(doc(&ctx), page), vec![original]);
}

#[test]
fn duplicating_a_connector_with_its_targets_reglues_to_the_copies() {
    let (mut ctx, registry, page) = fixture();
    draw_glued(&mut ctx, &registry);
    let original = connectors(doc(&ctx), page)[0];
    let (left, right) = (shape(&ctx, page, 1), shape(&ctx, page, 2));
    select(&mut ctx, &[left, right, original]);
    dispatch(&mut ctx, &registry, "edit.duplicate", Args::new()).expect("duplicate");
    let after = snapshot(&ctx);
    let copy = *connectors(&after, page).last().expect("copy");
    let copy_visio = after.shape(copy).expect("copy").visio_id;
    let records: Vec<Connect> = connects(&ctx, page)
        .into_iter()
        .filter(|record| record.from_sheet == copy_visio)
        .collect();
    assert_eq!(records.len(), 2, "{records:?}");
    let begin = records
        .iter()
        .find(|record| record.from_cell == "BeginX")
        .expect("begin record");
    assert!(begin.to_sheet != 1 && begin.to_sheet != 2, "{begin:?}");
    let target = begin.to_sheet;
    assert_eq!(
        formula(&after, copy, "BeginX"),
        Some(format!(
            "PAR(PNT(Sheet.{target}!Connections.X1,Sheet.{target}!Connections.Y1))"
        ))
    );
    let end_record = records
        .iter()
        .find(|record| record.from_cell == "EndX")
        .expect("end record");
    assert_eq!(
        formula(&after, copy, "EndTrigger"),
        Some(format!(
            "_XFTRIGGER(Sheet.{}!EventXFMod)",
            end_record.to_sheet
        ))
    );
    assert_cells_match_path(&after, copy);
}

#[test]
fn copy_and_paste_offsets_the_endpoint_cells() {
    let (mut ctx, registry, page) = fixture();
    draw_free(&mut ctx, &registry);
    let original = connectors(doc(&ctx), page)[0];
    select(&mut ctx, &[original]);
    dispatch(&mut ctx, &registry, "edit.copy", Args::new()).expect("copy");
    dispatch(&mut ctx, &registry, "edit.paste", Args::new()).expect("paste");
    let after = snapshot(&ctx);
    let copy = *connectors(&after, page).last().expect("copy");
    let (from, to) = (
        end(&after, original, ConnectorEnd::End),
        end(&after, copy, ConnectorEnd::End),
    );
    assert!(
        close(to, Point::new(from.x + 0.1, from.y - 0.1)),
        "{from:?} {to:?}"
    );
    assert_cells_match_path(&after, copy);
    // A paste at the pointer puts the copy's midpoint there.
    dispatch(
        &mut ctx,
        &registry,
        "edit.paste",
        Args::new()
            .with("x", ArgValue::Number(7.0))
            .with("y", ArgValue::Number(7.0)),
    )
    .expect("paste at");
    let after = snapshot(&ctx);
    let copy = *connectors(&after, page).last().expect("copy");
    let (begin, finish) = (
        end(&after, copy, ConnectorEnd::Begin),
        end(&after, copy, ConnectorEnd::End),
    );
    assert!(close(
        Point::new((begin.x + finish.x) / 2.0, (begin.y + finish.y) / 2.0),
        Point::new(7.0, 7.0)
    ));
    assert_cells_match_path(&after, copy);
}

#[test]
fn a_drop_of_the_connector_master_places_its_ends_at_the_drop_point() {
    let (mut ctx, registry, page) = fixture();
    dispatch(
        &mut ctx,
        &registry,
        "insert.shape",
        Args::new()
            .with("stencil", ArgValue::Text("Basic Flowchart".to_owned()))
            .with("master", ArgValue::Text("Connector".to_owned()))
            .with("x", ArgValue::Number(4.0))
            .with("y", ArgValue::Number(5.0)),
    )
    .expect("drop");
    let after = snapshot(&ctx);
    let dropped = connectors(&after, page)[0];
    // The master runs 1.2 in left to right: the drop point is the
    // midpoint.
    assert!(close(
        end(&after, dropped, ConnectorEnd::Begin),
        Point::new(3.4, 5.0)
    ));
    assert!(close(
        end(&after, dropped, ConnectorEnd::End),
        Point::new(4.6, 5.0)
    ));
    // No literal over the master's GUARD formulas.
    for (name, expected) in [
        ("PinX", "GUARD((BeginX+EndX)/2)"),
        ("Width", "GUARD(EndX-BeginX)"),
        ("LocPinX", "GUARD(Width*0.5)"),
    ] {
        assert_eq!(
            formula(&after, dropped, name),
            Some(expected.to_owned()),
            "{name}"
        );
    }
    assert_cells_match_path(&after, dropped);
    // It moves like any connector.
    select(&mut ctx, &[dropped]);
    move_by(&mut ctx, &registry, 1.0, 0.0);
    assert!(close(
        end(doc(&ctx), dropped, ConnectorEnd::Begin),
        Point::new(4.4, 5.0)
    ));
}

// ---------------------------------------------------------------------------
// Deletes
// ---------------------------------------------------------------------------

#[test]
fn deleting_a_group_clears_the_glue_to_its_members() {
    for command in ["edit.delete", "edit.cut"] {
        let (mut ctx, registry, page) = fixture();
        draw_glued(&mut ctx, &registry);
        let connector = connectors(doc(&ctx), page)[0];
        let (left, right) = (shape(&ctx, page, 1), shape(&ctx, page, 2));
        select(&mut ctx, &[left, right]);
        dispatch(&mut ctx, &registry, "arrange.group", Args::new()).expect("group");
        let before = snapshot(&ctx);
        let stored = end(&before, connector, ConnectorEnd::Begin);
        let group = ctx.selection.anchor.expect("the group");
        select(&mut ctx, &[group]);
        dispatch(&mut ctx, &registry, command, Args::new()).expect(command);

        let after = snapshot(&ctx);
        assert!(connects(&ctx, page).is_empty(), "{command}");
        assert!(
            no_formula_names_a_sheet(&after, connector),
            "{command}: no stale formula"
        );
        assert!(
            close(end(&after, connector, ConnectorEnd::Begin), stored),
            "{command}: the freed end keeps its stored point"
        );
        dispatch(&mut ctx, &registry, "edit.undo", Args::new()).expect("undo");
        assert_eq!(state(doc(&ctx), page), state(&before, page), "{command}");
    }
}

#[test]
fn a_dangling_record_does_not_stop_the_clear() {
    let (mut ctx, registry, page) = fixture();
    draw_glued(&mut ctx, &registry);
    let connector = connectors(doc(&ctx), page)[0];
    {
        let session = ctx.active_document_mut().expect("session");
        let mut dangling = Connect::new();
        dangling.from_sheet = 99;
        dangling.from_cell = "BeginX".to_owned();
        dangling.from_part = model_mutate::VIS_BEGIN;
        dangling.to_sheet = 2;
        dangling.to_cell = "PinX".to_owned();
        dangling.to_part = model_mutate::VIS_WHOLE_SHAPE;
        session.document.pages[0].connects.insert(0, dangling);
    }
    let right = shape(&ctx, page, 2);
    select(&mut ctx, &[right]);
    dispatch(&mut ctx, &registry, "edit.delete", Args::new()).expect("delete");
    let after = snapshot(&ctx);
    assert_eq!(
        formula(&after, connector, "EndX"),
        None,
        "the end comes loose"
    );
    assert_eq!(
        formula(&after, connector, "EndTrigger"),
        Some("_XFTRIGGER(EventXFMod)".to_owned())
    );
}

// ---------------------------------------------------------------------------
// Routability
// ---------------------------------------------------------------------------

/// A 1-D line that is not routable (`ObjType` 1, Visio's 1-D line
/// family): `Angle` and `Width` follow the ends through `ATAN2` and
/// `SQRT`, and its geometry rows are literal.
fn line_shape(visio_id: i32) -> Shape {
    let formula_cell = |name: &str, formula: &str, value: f64| Cell {
        name: CellName::new(name),
        formula: Some(formula.to_owned()),
        value: Some(CellValue::Number(value)),
        unit: larrygram_geom::Unit::Inches,
        preserved: Preserved::new(),
    };
    let length = 2.0_f64.hypot(1.0);
    let mut shape = Shape::new(ShapeId { index: 0 }, visio_id);
    shape.sheet.cells = vec![
        cell("BeginX", 3.0),
        cell("BeginY", 4.0),
        cell("EndX", 5.0),
        cell("EndY", 5.0),
        formula_cell("PinX", "GUARD((BeginX+EndX)/2)", 4.0),
        formula_cell("PinY", "GUARD((BeginY+EndY)/2)", 4.5),
        formula_cell(
            "Width",
            "GUARD(SQRT((EndX-BeginX)^2+(EndY-BeginY)^2))",
            length,
        ),
        cell("Height", 0.0),
        formula_cell("LocPinX", "GUARD(Width*0.5)", length / 2.0),
        formula_cell("LocPinY", "GUARD(Height*0.5)", 0.0),
        formula_cell(
            "Angle",
            "GUARD(ATAN2(EndY-BeginY,EndX-BeginX))",
            1.0_f64.atan2(2.0),
        ),
        cell("ObjType", 1.0),
    ];
    let mut geometry = Section::new(SectionName::from_static("Geometry"));
    for (ix, (kind, x)) in [("MoveTo", 0.0), ("LineTo", length)]
        .into_iter()
        .enumerate()
    {
        let mut row = Row::new();
        row.row_type = Some(kind.to_owned());
        row.ix = Some(i32::try_from(ix).unwrap_or(0));
        row.cells = vec![cell("X", x), cell("Y", 0.0)];
        geometry.rows.push(row);
    }
    shape.sheet.sections.push(geometry);
    shape
}

#[test]
fn a_glued_line_that_is_not_routable_stays_straight() {
    let (mut ctx, registry, page) = fixture();
    {
        let session = ctx.active_document_mut().expect("session");
        session
            .document
            .insert_shape(page, line_shape(3), None)
            .expect("line");
    }
    let line = shape(&ctx, page, 3);
    assert!(doc(&ctx).is_connector(line));
    assert!(!doc(&ctx).is_routable(line));
    for (command, target) in [("connector.glue_begin", 1), ("connector.glue_end", 2)] {
        dispatch(
            &mut ctx,
            &registry,
            command,
            Args::new()
                .with("connector", ArgValue::Integer(3))
                .with("target", ArgValue::Integer(target)),
        )
        .expect(command);
    }
    // A box moves: the line follows its glue and stays one straight
    // segment, with no style cells written.
    let left = shape(&ctx, page, 1);
    select(&mut ctx, &[left]);
    move_by(&mut ctx, &registry, 0.0, 2.0);
    let after = snapshot(&ctx);
    let path = drawn(&after, line);
    assert_eq!(path.len(), 2, "a straight line: {path:?}");
    assert!(own(&after, line, "ShapeRouteStyle").is_none());
    // Its formulas survive the path write.
    assert_eq!(
        formula(&after, line, "Width"),
        Some("GUARD(SQRT((EndX-BeginX)^2+(EndY-BeginY)^2))".to_owned())
    );
    assert_eq!(
        formula(&after, line, "Angle"),
        Some("GUARD(ATAN2(EndY-BeginY,EndX-BeginX))".to_owned())
    );
    assert_cells_match_path(&after, line);
    // The style command leaves it alone.
    select(&mut ctx, &[line]);
    let refused = dispatch(
        &mut ctx,
        &registry,
        "connector.style",
        Args::new().with("style", ArgValue::Text("right_angle".to_owned())),
    );
    assert!(refused.is_err());
}

// ---------------------------------------------------------------------------
// The file round trip
// ---------------------------------------------------------------------------

#[test]
fn save_and_reopen_keeps_the_formulas_after_a_move_and_a_copy() {
    let (mut ctx, registry, page) = fixture();
    draw_glued(&mut ctx, &registry);
    let original = connectors(doc(&ctx), page)[0];
    let (left, right) = (shape(&ctx, page, 1), shape(&ctx, page, 2));
    select(&mut ctx, &[left, right, original]);
    move_by(&mut ctx, &registry, 0.5, 0.0);
    dispatch(&mut ctx, &registry, "edit.duplicate", Args::new()).expect("duplicate");
    let before = snapshot(&ctx);
    let bytes = write_document(&before, &Package::new()).expect("save");
    let back = read_document(&bytes, &Limits::DEFAULT).expect("reopen");
    let page_back = back.pages[0].id;
    for connector in connectors(&before, page) {
        let visio_id = before.shape(connector).expect("shape").visio_id;
        let reopened = back
            .shape_by_visio_id(page_back, visio_id)
            .expect("reopened connector");
        for name in [
            "BeginX",
            "BeginY",
            "EndX",
            "EndY",
            "BegTrigger",
            "EndTrigger",
            "PinX",
        ] {
            assert_eq!(
                formula(&back, reopened, name),
                formula(&before, connector, name),
                "{visio_id} {name}"
            );
        }
    }
    assert_eq!(
        back.pages[0].connects.len(),
        before.page(page).expect("page").connects.len()
    );
}
