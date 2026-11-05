//! Connector segment moves, Reset Connector and the reroute options
//! through the registry (task P7-35).
//!
//! `connector.move_segment` writes one moved segment as the
//! connector's geometry in one undoable edit; a moved segment survives
//! an unrelated edit and a save and reopen, and is replaced by a fresh
//! route when a glued shape moves. `connector.dyn.conn.reroute` (Reset
//! Connector) re-routes the selection fresh, and the three
//! `format.set.dyn.conn.reroute.*` controls write `ConFixedCode` 0, 3
//! and 2. With Never Reroute the scoped pass keeps the route's
//! interior rows byte for byte: only its ends follow their glue.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use support::dispatch;

use larrygram_commands::app::{AppCtx, DocumentSession, Settings};
use larrygram_commands::builtins::{connector_reroute_code, registry};
use larrygram_commands::{ArgValue, Args, CommandError, CommandId, Registry};
use larrygram_geom::Point;
use larrygram_model::{
    Cell, CellLookup, CellName, CellValue, Document, GeometryRow, PageId, Preserved, Row, Section,
    SectionName, Shape, ShapeId,
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

/// Two boxes on one page (Visio ids 1 and 2), offset so the connector
/// routes with several segments: box 1 at `(2, 2)` and box 2 at
/// `(6, 5)`.
fn fixture() -> (AppCtx, Registry, PageId) {
    let mut doc = Document::new();
    let page = doc.add_page("Page-1").expect("page");
    for (visio_id, x, y) in [(1, 2.0, 2.0), (2, 6.0, 5.0)] {
        doc.insert_shape(page, box_shape(visio_id, x, y), None)
            .expect("insert");
    }
    let mut ctx = AppCtx::new(Settings::default());
    ctx.open_document(DocumentSession::new(doc, None));
    (ctx, registry().expect("registry"), page)
}

fn doc(ctx: &AppCtx) -> &Document {
    &ctx.active_document().expect("session").document
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

fn own(document: &Document, shape: ShapeId, name: &str) -> Option<Cell> {
    document
        .shape(shape)?
        .sheet
        .cells
        .iter()
        .find(|cell| cell.name.as_str() == name)
        .cloned()
}

fn number(document: &Document, shape: ShapeId, name: &str) -> f64 {
    match own(document, shape, name).and_then(|cell| cell.value) {
        Some(CellValue::Number(value)) => value,
        _ => f64::NAN,
    }
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

/// The connector's stored path points in page inches.
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

/// The connector's raw geometry rows (the stored route, byte for
/// byte).
fn rows(document: &Document, shape: ShapeId) -> Vec<Row> {
    document
        .shape(shape)
        .expect("shape")
        .sheet
        .sections
        .iter()
        .find(|section| section.name.as_str() == "Geometry")
        .expect("a Geometry section")
        .rows
        .clone()
}

fn close(a: Point, b: Point) -> bool {
    (a.x - b.x).abs() < 1e-6 && (a.y - b.y).abs() < 1e-6
}

/// A connector drawn with point glue to box 1 and dynamic glue to box
/// 2 (Visio id 3), at the re-route pass's fixed point.
fn draw_glued(ctx: &mut AppCtx, registry: &Registry) -> i32 {
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
    3
}

/// One `connector.move_segment` dispatch on segment `index` of
/// `path`, moved one notch along its normal, and the `to` it used.
fn move_segment(
    ctx: &mut AppCtx,
    registry: &Registry,
    connector: i32,
    path: &[Point],
    index: usize,
) -> f64 {
    let (a, b) = (path[index], path[index + 1]);
    let to = if a.y == b.y { a.y + 0.75 } else { a.x + 0.75 };
    dispatch(
        ctx,
        registry,
        "connector.move_segment",
        Args::new()
            .with("connector", ArgValue::Integer(i64::from(connector)))
            .with("segment", ArgValue::Integer(i64::try_from(index).unwrap()))
            .with("to", ArgValue::Number(to)),
    )
    .expect("move segment");
    to
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
// Item 2: one undoable segment move
// ---------------------------------------------------------------------------

#[test]
fn move_segment_moves_one_segment_and_one_undo_restores_the_geometry() {
    let (mut ctx, registry, page) = fixture();
    let connector = draw_glued(&mut ctx, &registry);
    let connector_shape = shape(&ctx, page, connector);
    let before = rows(doc(&ctx), connector_shape);
    let path = drawn(doc(&ctx), connector_shape);
    assert!(path.len() >= 3, "the glued route has a bend: {path:?}");

    let steps = undo_len(&ctx);
    let to = move_segment(&mut ctx, &registry, connector, &path, 1);
    assert_eq!(undo_len(&ctx), steps + 1, "one undo step");

    let after = rows(doc(&ctx), connector_shape);
    assert_ne!(after, before, "the geometry changed");
    let moved = drawn(doc(&ctx), connector_shape);
    // The moved segment sits on `to` and the ends stay where they are
    // glued.
    let (a, b) = (moved[1], moved[2]);
    if a.y == b.y {
        assert!(
            close(a, Point::new(a.x, to)),
            "the segment moved to y={to}: {a:?}"
        );
    } else {
        assert!(
            close(a, Point::new(to, a.y)),
            "the segment moved to x={to}: {a:?}"
        );
    }
    assert!(
        close(*moved.first().unwrap(), *path.first().unwrap()),
        "the begin stays: {moved:?}"
    );
    assert!(
        close(*moved.last().unwrap(), *path.last().unwrap()),
        "the end stays: {moved:?}"
    );

    dispatch(&mut ctx, &registry, "edit.undo", Args::new()).expect("undo");
    assert_eq!(rows(doc(&ctx), connector_shape), before, "undo restores");
}

#[test]
fn move_segment_requires_its_arguments_with_no_dialog_request() {
    let (mut ctx, registry, _page) = fixture();
    draw_glued(&mut ctx, &registry);
    for args in [
        Args::new(),
        Args::new().with("connector", ArgValue::Integer(3)),
        Args::new()
            .with("connector", ArgValue::Integer(3))
            .with("segment", ArgValue::Integer(1)),
    ] {
        let error = dispatch(&mut ctx, &registry, "connector.move_segment", args)
            .expect_err("a missing argument is an error");
        match error {
            CommandError::InvalidArgs { message } => {
                assert!(message.contains("is required"), "{message}")
            }
            other => panic!("expected a clear error, got {other:?}"),
        }
    }
}

// ---------------------------------------------------------------------------
// Item 3: the stored route is the memory
// ---------------------------------------------------------------------------

#[test]
fn a_moved_segment_survives_an_unrelated_edit_and_a_save_and_reopen() {
    let (mut ctx, registry, page) = fixture();
    let connector = draw_glued(&mut ctx, &registry);
    let connector_shape = shape(&ctx, page, connector);
    let path = drawn(doc(&ctx), connector_shape);
    move_segment(&mut ctx, &registry, connector, &path, 1);
    let moved = rows(doc(&ctx), connector_shape);
    let moved_path = drawn(doc(&ctx), connector_shape);

    // An unrelated edit: a third shape (no glue to the connector)
    // moves. The route is untouched, byte for byte.
    let third = {
        let session = ctx.active_document_mut().expect("session");
        session
            .document
            .insert_shape(page, box_shape(4, 9.0, 9.0), None)
            .expect("insert")
    };
    select(&mut ctx, &[third]);
    move_by(&mut ctx, &registry, 0.5, 0.25);
    assert_eq!(rows(doc(&ctx), connector_shape), moved, "unrelated edit");

    // A save and reopen keeps the moved route.
    let bytes = write_document(doc(&ctx), &Package::new()).expect("save");
    let back = read_document(&bytes, &Limits::DEFAULT).expect("reopen");
    let reopened = back
        .shape_by_visio_id(PageId { index: 0 }, connector)
        .expect("connector");
    let reopened_path = drawn(&back, reopened);
    assert_eq!(reopened_path.len(), moved_path.len(), "{reopened_path:?}");
    for (before, after) in moved_path.iter().zip(reopened_path.iter()) {
        assert!(close(*before, *after), "{before:?} vs {after:?}");
    }
}

#[test]
fn a_moved_segment_is_replaced_when_a_glued_shape_moves() {
    let (mut ctx, registry, page) = fixture();
    let connector = draw_glued(&mut ctx, &registry);
    let connector_shape = shape(&ctx, page, connector);
    let path = drawn(doc(&ctx), connector_shape);
    move_segment(&mut ctx, &registry, connector, &path, 1);
    let moved = rows(doc(&ctx), connector_shape);

    // Box 1 is the begin's glue target: the scoped re-route picks the
    // connector and replaces the stored route with a fresh one (rule
    // 5).
    let picked = shape(&ctx, page, 1);
    select(&mut ctx, &[picked]);
    move_by(&mut ctx, &registry, 0.5, 0.0);
    assert_ne!(rows(doc(&ctx), connector_shape), moved, "fresh route");
}

// ---------------------------------------------------------------------------
// Item 4: Reset Connector
// ---------------------------------------------------------------------------

#[test]
fn reset_connector_reroutes_the_selection_fresh() {
    let (mut ctx, registry, page) = fixture();
    let connector = draw_glued(&mut ctx, &registry);
    let connector_shape = shape(&ctx, page, connector);
    let fresh = rows(doc(&ctx), connector_shape);
    let path = drawn(doc(&ctx), connector_shape);
    move_segment(&mut ctx, &registry, connector, &path, 1);
    let moved = rows(doc(&ctx), connector_shape);
    assert_ne!(moved, fresh);

    select(&mut ctx, &[connector_shape]);
    dispatch(
        &mut ctx,
        &registry,
        "connector.dyn.conn.reroute",
        Args::new(),
    )
    .expect("reset");
    assert_eq!(rows(doc(&ctx), connector_shape), fresh, "a fresh route");

    // One undo restores the moved route.
    dispatch(&mut ctx, &registry, "edit.undo", Args::new()).expect("undo");
    assert_eq!(rows(doc(&ctx), connector_shape), moved, "undo restores");
}

#[test]
fn reset_connector_is_disabled_with_no_connector_selected() {
    let (mut ctx, registry, page) = fixture();
    draw_glued(&mut ctx, &registry);
    let id = CommandId::intern("connector.dyn.conn.reroute").expect("id");
    select(&mut ctx, &[]);
    assert!(!registry.enabled(&id, &ctx).value, "no selection");
    let picked = shape(&ctx, page, 1);
    select(&mut ctx, &[picked]);
    assert!(
        !registry.enabled(&id, &ctx).value,
        "a shape is no connector"
    );
    let picked = shape(&ctx, page, 3);
    select(&mut ctx, &[picked]);
    assert!(registry.enabled(&id, &ctx).value, "the connector is");
}

// ---------------------------------------------------------------------------
// Item 5: the reroute options
// ---------------------------------------------------------------------------

#[test]
fn each_reroute_option_writes_its_con_fixed_code() {
    for (command, value) in [
        ("format.set.dyn.conn.reroute.freely", 0.0),
        ("format.set.dyn.conn.reroute.on.crossover", 3.0),
        ("format.set.dyn.conn.reroute.never", 2.0),
    ] {
        let (mut ctx, registry, page) = fixture();
        draw_glued(&mut ctx, &registry);
        let connector_shape = shape(&ctx, page, 3);
        select(&mut ctx, &[connector_shape]);
        dispatch(&mut ctx, &registry, command, Args::new()).expect("option");
        assert_eq!(
            number(doc(&ctx), connector_shape, "ConFixedCode"),
            value,
            "{command}"
        );
        assert_eq!(
            connector_reroute_code(&ctx),
            Some(value),
            "the control shows its value checked"
        );
        dispatch(&mut ctx, &registry, "edit.undo", Args::new()).expect("undo");
        assert_eq!(
            own(doc(&ctx), connector_shape, "ConFixedCode"),
            None,
            "{command} undo removes the cell"
        );
    }
}

#[test]
fn never_reroute_keeps_the_interior_when_a_glued_shape_moves() {
    let (mut ctx, registry, page) = fixture();
    let connector = draw_glued(&mut ctx, &registry);
    let connector_shape = shape(&ctx, page, connector);
    select(&mut ctx, &[connector_shape]);
    dispatch(
        &mut ctx,
        &registry,
        "format.set.dyn.conn.reroute.never",
        Args::new(),
    )
    .expect("never");
    let before = rows(doc(&ctx), connector_shape);
    let path = drawn(doc(&ctx), connector_shape);

    // Box 1 moves: the begin follows its glue, the interior rows stay
    // byte for byte, and the end (box 2 unmoved) stays.
    let picked = shape(&ctx, page, 1);
    select(&mut ctx, &[picked]);
    move_by(&mut ctx, &registry, 0.25, 0.5);

    let after = rows(doc(&ctx), connector_shape);
    assert_eq!(after.len(), before.len(), "the row count is kept");
    let moved = drawn(doc(&ctx), connector_shape);
    for (kept, was) in path[1..path.len() - 1]
        .iter()
        .zip(moved[1..moved.len() - 1].iter())
    {
        assert!(
            close(*was, *kept),
            "the interior point {was:?} moved from {kept:?}"
        );
    }
    assert!(
        close(
            *moved.first().unwrap(),
            Point::new(path[0].x + 0.25, path[0].y + 0.5)
        ),
        "the begin follows its glue: {:?}",
        moved.first()
    );
    assert!(
        close(*moved.last().unwrap(), *path.last().unwrap()),
        "the end stays: {:?}",
        moved.last()
    );
}

#[test]
fn reset_connector_routes_even_a_never_connector_fresh() {
    let (mut ctx, registry, page) = fixture();
    let connector = draw_glued(&mut ctx, &registry);
    let connector_shape = shape(&ctx, page, connector);
    select(&mut ctx, &[connector_shape]);
    dispatch(
        &mut ctx,
        &registry,
        "format.set.dyn.conn.reroute.never",
        Args::new(),
    )
    .expect("never");
    let before_path = drawn(doc(&ctx), connector_shape);

    // A glued shape moves: the interior is kept (rule 7).
    let picked = shape(&ctx, page, 1);
    select(&mut ctx, &[picked]);
    move_by(&mut ctx, &registry, 0.25, 0.5);
    let kept = drawn(doc(&ctx), connector_shape);
    for (kept, was) in before_path[1..before_path.len() - 1]
        .iter()
        .zip(kept[1..kept.len() - 1].iter())
    {
        assert!(
            close(*was, *kept),
            "the interior point {was:?} moved from {kept:?}"
        );
    }
    let kept = rows(doc(&ctx), connector_shape);

    // Reset Connector re-routes it fresh (task P7-35 item 4), and the
    // fresh route is what the full pass computes.
    select(&mut ctx, &[connector_shape]);
    dispatch(
        &mut ctx,
        &registry,
        "connector.dyn.conn.reroute",
        Args::new(),
    )
    .expect("reset");
    let reset = rows(doc(&ctx), connector_shape);
    assert_ne!(reset, kept, "Reset Connector routes fresh");
    dispatch(&mut ctx, &registry, "connector.reroute_all", Args::new()).expect("reroute all");
    assert_eq!(
        rows(doc(&ctx), connector_shape),
        reset,
        "the same fresh route"
    );
}

// ---------------------------------------------------------------------------
// The argument and subject contract
// ---------------------------------------------------------------------------

#[test]
fn move_segment_names_its_bad_subjects_and_arguments() {
    let (mut ctx, registry, page) = fixture();
    let connector = draw_glued(&mut ctx, &registry);
    let segment = |mut args: Args| {
        args.set("segment", ArgValue::Integer(1));
        args
    };
    // An unknown connector id.
    let error = dispatch(
        &mut ctx,
        &registry,
        "connector.move_segment",
        segment(
            Args::new()
                .with("connector", ArgValue::Integer(99))
                .with("to", ArgValue::Number(5.0)),
        ),
    )
    .expect_err("an unknown id");
    assert!(
        matches!(&error, CommandError::InvalidArgs { message } if message.contains("unknown connector shape id 99")),
        "{error:?}"
    );
    // A box has no connector ends.
    let error = dispatch(
        &mut ctx,
        &registry,
        "connector.move_segment",
        segment(
            Args::new()
                .with("connector", ArgValue::Integer(1))
                .with("to", ArgValue::Number(5.0)),
        ),
    )
    .expect_err("a box");
    assert!(
        matches!(&error, CommandError::InvalidArgs { message } if message.contains("has no connector ends")),
        "{error:?}"
    );
    // A non-finite target.
    let error = dispatch(
        &mut ctx,
        &registry,
        "connector.move_segment",
        segment(
            Args::new()
                .with("connector", ArgValue::Integer(i64::from(connector)))
                .with("to", ArgValue::Number(f64::NAN)),
        ),
    )
    .expect_err("a non-finite to");
    assert!(
        matches!(&error, CommandError::InvalidArgs { message } if message.contains("to must be finite")),
        "{error:?}"
    );
    // A segment the path has not.
    let error = dispatch(
        &mut ctx,
        &registry,
        "connector.move_segment",
        Args::new()
            .with("connector", ArgValue::Integer(i64::from(connector)))
            .with("segment", ArgValue::Integer(99))
            .with("to", ArgValue::Number(5.0)),
    )
    .expect_err("an out-of-range segment");
    assert!(
        matches!(&error, CommandError::InvalidArgs { message } if message.contains("cannot move the segment")),
        "{error:?}"
    );
    // A curved connector has no straight-segment path to move.
    let picked = shape(&ctx, page, connector);
    select(&mut ctx, &[picked]);
    dispatch(
        &mut ctx,
        &registry,
        "connector.style",
        Args::new().with("style", ArgValue::Text("curved".to_owned())),
    )
    .expect("curve");
    let error = dispatch(
        &mut ctx,
        &registry,
        "connector.move_segment",
        segment(
            Args::new()
                .with("connector", ArgValue::Integer(i64::from(connector)))
                .with("to", ArgValue::Number(5.0)),
        ),
    )
    .expect_err("a curved path");
    assert!(
        matches!(&error, CommandError::Failed { message } if message.contains("no straight-segment path")),
        "{error:?}"
    );
}

#[test]
fn reset_connector_with_nothing_to_route_pushes_no_edit() {
    let (mut ctx, registry, page) = fixture();
    let connector = draw_glued(&mut ctx, &registry);
    let connector_shape = shape(&ctx, page, connector);
    // A geometry that computes belongs to recalc (ADR 0056): the
    // router never writes its rows, so there is nothing to undo.
    {
        let session = ctx.active_document_mut().expect("session");
        let entity = session.document.shape_mut(connector_shape).expect("shape");
        for section in &mut entity.sheet.sections {
            if section.name.as_str() == "Geometry" {
                for row in &mut section.rows {
                    for cell in &mut row.cells {
                        cell.formula = Some("Width*0.5".to_owned());
                    }
                }
            }
        }
    }
    let picked = shape(&ctx, page, connector);
    select(&mut ctx, &[picked]);
    let before = undo_len(&ctx);
    let outcome = dispatch(
        &mut ctx,
        &registry,
        "connector.dyn.conn.reroute",
        Args::new(),
    )
    .expect("reset");
    assert!(outcome.edit.is_none(), "a formula-driven connector is kept");
    assert_eq!(undo_len(&ctx), before, "no undo step");
}

// ---------------------------------------------------------------------------
// The model read paths the kept route relies on (the coverage ratchet's
// margin: these branches only ever saw proptest-driven coverage)
// ---------------------------------------------------------------------------

/// A `Connection` row whose `X` cell holds text: the reader skips the
/// row without renumbering the ones around it.
#[test]
fn a_connection_row_with_a_text_cell_is_skipped() {
    let mut document = Document::new();
    let page = document.add_page("Page-1").expect("page");
    let mut shape = box_shape(1, 3.0, 3.0);
    if let Some(section) = shape
        .sheet
        .sections
        .iter_mut()
        .find(|section| section.name.as_str() == "Connection")
    {
        for row in &mut section.rows {
            for cell in &mut row.cells {
                if cell.name.as_str() == "X" {
                    cell.value = Some(CellValue::Str("left".to_owned()));
                }
            }
        }
    }
    let shape = document.insert_shape(page, shape, None).expect("insert");
    assert!(
        document.connection_points_indexed(shape).is_empty(),
        "a row without numbers yields no point"
    );
}

/// The raw diff walks element and non-element children alike (the
/// paths it reports name every step).
#[test]
fn the_raw_diff_walks_every_child_kind() {
    use larrygram_model::{QName, XmlChild, XmlNode};
    let leaf = |name: &str, self_closing: bool| XmlNode {
        name: QName::local(name),
        attributes: Vec::new(),
        children: Vec::new(),
        self_closing,
    };
    let tree = |text: &str, second: &str| XmlNode {
        name: QName::local("root"),
        attributes: Vec::new(),
        children: vec![
            XmlChild::Text(text.to_owned()),
            XmlChild::Comment("c".to_owned()),
            XmlChild::ProcessingInstruction {
                target: "pi".to_owned(),
                data: "d".to_owned(),
            },
            XmlChild::Element(leaf("one", true)),
            XmlChild::Element(leaf(second, false)),
            XmlChild::Text(text.to_owned()),
        ],
        self_closing: false,
    };
    let a = tree("a", "two");
    let b = tree("b", "three");
    let differences = larrygram_model::diff(&a, &b).expect("diff");
    assert!(!differences.is_empty(), "the trees differ");
    let back = larrygram_model::diff(&b, &a).expect("diff");
    assert!(!back.is_empty(), "the trees differ both ways");
}

/// A master instance inherits its connection points; the per-axis
/// scale read skips a non-numeric size without failing (the read paths
/// of `Document::connection_points_indexed`).
#[test]
fn a_master_instance_inherits_its_connection_points() {
    use larrygram_model::{Master, MasterId};
    let mut document = Document::new();
    let page = document.add_page("Page-1").expect("page");
    // The master's content: one connection point and a text `Width`
    // (the scale read skips it).
    let mut content = Shape::new(ShapeId { index: 0 }, 1);
    content.sheet.cells = vec![cell("Width", 2.0), {
        let mut width = cell("Height", 1.0);
        width.value = Some(CellValue::Str("tall".to_owned()));
        width
    }];
    let mut connection = Section::new(SectionName::from_static("Connection"));
    let mut row = Row::new();
    row.row_type = Some("Connection".to_owned());
    row.ix = Some(0);
    row.cells = vec![cell("X", 1.0), cell("Y", 1.0)];
    connection.rows.push(row);
    content.sheet.sections.push(connection);
    let content = document.add_shape(content);
    let mid = document.masters.len();
    let mut master = Master::new(MasterId { index: mid as u32 }, 1);
    master.shapes = vec![content];
    document.masters.push(master);
    // The instance: no rows of its own, linked to the master.
    let mut instance = Shape::new(ShapeId { index: 0 }, 2);
    instance.master = Some(MasterId { index: mid as u32 });
    let instance = document.insert_shape(page, instance, None).expect("insert");
    let points = document.connection_points_indexed(instance);
    assert_eq!(points.len(), 1, "{points:?}");
    assert_eq!(points[0], (0, Point::new(1.0, 1.0)), "{points:?}");
}
