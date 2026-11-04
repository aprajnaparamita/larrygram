//! The connector style commands (task P7-36 item 3): the
//! `connector.style` table, the page default, the style controls'
//! dispatch and read order, the curved re-route with its one-step
//! undo, and the disabled line jump controls (item 4).

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use support::dispatch;

use larrygram_commands::app::{AppCtx, DocumentSession, Settings};
use larrygram_commands::builtins::{connector_style, registry};
use larrygram_commands::{ArgValue, Args, Registry};
use larrygram_geom::RouteStyle;
use larrygram_model::{
    Cell, CellName, CellValue, Document, GeometryRow, PageId, Preserved, Shape, ShapeId, SheetRef,
};

/// Two boxes on one page (Visio ids 1 and 2), as in the move suite.
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

/// A 2 x 1 in box at `(x, y)` with one connection point per side
/// midpoint.
fn box_shape(visio_id: i32, x: f64, y: f64) -> larrygram_model::Shape {
    use larrygram_model::{Cell, Preserved, Row, Section, SectionName, Shape};
    let cell = |name: &str, value: f64| Cell {
        name: CellName::new(name),
        formula: Some(format!("{value}")),
        value: Some(CellValue::Number(value)),
        unit: larrygram_geom::Unit::Inches,
        preserved: Preserved::new(),
    };
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

/// A connector glued from box 1 (point glue, row 0) to box 2 (dynamic
/// glue), selected afterwards.
fn draw_glued(ctx: &mut AppCtx, registry: &Registry, page: PageId) -> ShapeId {
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
    let connector = doc(ctx)
        .shape_by_visio_id(page, 3)
        .expect("the drawn connector");
    ctx.selection.shapes = vec![connector];
    ctx.selection.anchor = Some(connector);
    ctx.selection.page = Some(page);
    connector
}

fn doc(ctx: &AppCtx) -> &Document {
    &ctx.active_document().expect("session").document
}

fn shape(ctx: &AppCtx, page: PageId, visio_id: i32) -> ShapeId {
    doc(ctx)
        .shape_by_visio_id(page, visio_id)
        .expect("shape by visio id")
}

/// The value of one of the shape's own cells.
fn own(document: &Document, shape: ShapeId, name: &str) -> Option<CellValue> {
    document
        .shape(shape)?
        .sheet
        .cells
        .iter()
        .find(|cell| cell.name.as_str() == name)
        .and_then(|cell| cell.value.clone())
}

/// The value of one of the page sheet's cells.
fn page_cell(ctx: &AppCtx, page: PageId, name: &str) -> Option<CellValue> {
    doc(ctx)
        .resolve_page_cell(page, &CellName::new(name))
        .ok()
        .and_then(|resolved| resolved.value.cloned())
}

/// The connector's own `ShapeRouteStyle` and `ConLineRouteExt` values.
fn style_cells(document: &Document, shape: ShapeId) -> (Option<CellValue>, Option<CellValue>) {
    (
        own(document, shape, "ShapeRouteStyle"),
        own(document, shape, "ConLineRouteExt"),
    )
}

/// The drawn path's row types.
fn row_types(document: &Document, shape: ShapeId) -> Vec<&'static str> {
    let geometry = document.shape_geometry(shape).expect("geometry");
    geometry
        .paths
        .first()
        .expect("a path")
        .rows
        .iter()
        .map(|row| match row {
            GeometryRow::MoveTo { .. } => "MoveTo",
            GeometryRow::LineTo { .. } => "LineTo",
            GeometryRow::NurbsTo { .. } => "NURBSTo",
            _ => "other",
        })
        .collect()
}

/// One style command on the selection writes the table's pair (task
/// P7-36 item 3, ADR 0084 rule 8).
#[test]
fn each_style_choice_writes_the_tables_cells() {
    for (style, route, ext) in [
        ("right_angle", 1.0, 1.0),
        ("straight", 2.0, 1.0),
        ("curved", 1.0, 2.0),
    ] {
        let (mut ctx, registry, page) = fixture();
        let connector = draw_glued(&mut ctx, &registry, page);
        dispatch(
            &mut ctx,
            &registry,
            "connector.style",
            Args::new().with("style", ArgValue::Text(style.to_owned())),
        )
        .expect("style");
        assert_eq!(
            style_cells(doc(&ctx), connector),
            (Some(CellValue::Number(route)), Some(CellValue::Number(ext))),
            "{style}"
        );
    }
}

/// With nothing selected (or with `page_default`) the cells go on the
/// page sheet (task P7-36 item 3).
#[test]
fn the_page_default_writes_the_page_cells() {
    let (mut ctx, registry, page) = fixture();
    draw_glued(&mut ctx, &registry, page);
    // Nothing selected: the page default.
    ctx.selection.shapes.clear();
    ctx.selection.anchor = None;
    dispatch(
        &mut ctx,
        &registry,
        "connector.style",
        Args::new().with("style", ArgValue::Text("straight".to_owned())),
    )
    .expect("page style");
    assert_eq!(
        page_cell(&ctx, page, "RouteStyle"),
        Some(CellValue::Number(2.0))
    );
    assert_eq!(
        page_cell(&ctx, page, "LineRouteExt"),
        Some(CellValue::Number(1.0))
    );

    // `page_default` with a selection ignores the selection.
    let connector = shape(&ctx, page, 3);
    ctx.selection.shapes = vec![connector];
    dispatch(
        &mut ctx,
        &registry,
        "connector.style",
        Args::new()
            .with("style", ArgValue::Text("curved".to_owned()))
            .with("page_default", ArgValue::Flag(true)),
    )
    .expect("page default");
    assert_eq!(
        page_cell(&ctx, page, "RouteStyle"),
        Some(CellValue::Number(1.0))
    );
    assert_eq!(
        page_cell(&ctx, page, "LineRouteExt"),
        Some(CellValue::Number(2.0))
    );
    assert_eq!(
        style_cells(doc(&ctx), connector),
        (None, None),
        "the selection is not written"
    );
}

/// A selection of shapes with no connector is refused (task P7-36
/// item 3).
#[test]
fn a_selection_without_a_connector_is_refused() {
    let (mut ctx, registry, page) = fixture();
    let left = shape(&ctx, page, 1);
    ctx.selection.shapes = vec![left];
    ctx.selection.anchor = Some(left);
    let error = dispatch(
        &mut ctx,
        &registry,
        "connector.style",
        Args::new().with("style", ArgValue::Text("straight".to_owned())),
    )
    .expect_err("no connector");
    assert!(
        error.to_string().contains("no selected connector"),
        "{error}"
    );
}

/// Setting Curved bends the route into one `NURBSTo` row, and one undo
/// restores the cells and the old rows together (task P7-36 item 2;
/// the master note's undo check for a curved style).
#[test]
fn setting_curved_writes_one_nurbs_row_and_one_undo_restores_it() {
    let (mut ctx, registry, page) = fixture();
    let connector = draw_glued(&mut ctx, &registry, page);
    dispatch(&mut ctx, &registry, "connector.reroute_all", Args::new()).expect("reroute");
    let before = row_types(doc(&ctx), connector);
    assert!(before.contains(&"LineTo"), "{before:?}");
    dispatch(
        &mut ctx,
        &registry,
        "connector.style",
        Args::new().with("style", ArgValue::Text("curved".to_owned())),
    )
    .expect("curved");
    let curved = row_types(doc(&ctx), connector);
    assert_eq!(curved, vec!["MoveTo", "NURBSTo"]);
    assert_eq!(
        style_cells(doc(&ctx), connector),
        (Some(CellValue::Number(1.0)), Some(CellValue::Number(2.0)))
    );
    // The E cell carries the NURBS formula text as its cached value
    // and no formula (task P7-36 item 2).
    let geometry = doc(&ctx).shape_geometry(connector).expect("geometry");
    let rows = &geometry.paths.first().expect("path").rows;
    match &rows[1] {
        GeometryRow::NurbsTo { data, .. } => {
            let data = data.as_ref().expect("E decodes");
            assert_eq!(data.degree, 3);
            assert_eq!(data.x_type, 1);
        }
        other => panic!("expected a NURBSTo row, got {other:?}"),
    }

    // One undo restores the cells and the old rows together.
    dispatch(&mut ctx, &registry, "edit.undo", Args::new()).expect("undo");
    assert_eq!(row_types(doc(&ctx), connector), before);
    assert_eq!(style_cells(doc(&ctx), connector), (None, None));
    // Redo puts the curve back with no recompute.
    dispatch(&mut ctx, &registry, "edit.redo", Args::new()).expect("redo");
    assert_eq!(row_types(doc(&ctx), connector), vec!["MoveTo", "NURBSTo"]);
}

/// Straight Lines routes the two endpoints with one `LineTo` (task
/// P7-36 item 3).
#[test]
fn setting_straight_writes_a_two_point_route() {
    let (mut ctx, registry, page) = fixture();
    let connector = draw_glued(&mut ctx, &registry, page);
    dispatch(
        &mut ctx,
        &registry,
        "connector.style",
        Args::new().with("style", ArgValue::Text("straight".to_owned())),
    )
    .expect("straight");
    assert_eq!(row_types(doc(&ctx), connector), vec!["MoveTo", "LineTo"]);
}

/// Every style control dispatches `connector.style` with its own
/// style (task P7-36 item 3); Design > Connectors with nothing
/// selected sets the page default.
#[test]
fn the_style_controls_dispatch_the_style_command() {
    for (command, route, ext) in [
        ("connector.right_angle", 1.0, 1.0),
        ("connector.straight.lines", 2.0, 1.0),
        ("connector.curved.lines", 1.0, 2.0),
        ("connector.straight", 2.0, 1.0),
        ("connector.curved", 1.0, 2.0),
        ("format.set.dyn.conn.routing.style", 1.0, 1.0),
        ("format.set.dyn.conn.appearance.straight", 2.0, 1.0),
        ("format.set.dyn.conn.appearance.curved", 1.0, 2.0),
    ] {
        let (mut ctx, registry, page) = fixture();
        let connector = draw_glued(&mut ctx, &registry, page);
        dispatch(&mut ctx, &registry, command, Args::new()).expect(command);
        assert_eq!(
            style_cells(doc(&ctx), connector),
            (Some(CellValue::Number(route)), Some(CellValue::Number(ext))),
            "{command}"
        );
    }

    // With nothing selected the controls set the page default.
    let (mut ctx, registry, page) = fixture();
    draw_glued(&mut ctx, &registry, page);
    ctx.selection.shapes.clear();
    ctx.selection.anchor = None;
    dispatch(&mut ctx, &registry, "connector.straight.lines", Args::new()).expect("page default");
    assert_eq!(
        page_cell(&ctx, page, "RouteStyle"),
        Some(CellValue::Number(2.0))
    );
    assert_eq!(
        page_cell(&ctx, page, "LineRouteExt"),
        Some(CellValue::Number(1.0))
    );
}

/// The read order (task P7-36 item 3): the shape's cells, then the
/// page's, then right angle; value 16, a directed value and an
/// unknown value route as right angle.
#[test]
fn the_read_order_reads_the_shape_then_the_page_then_right_angle() {
    let (mut ctx, registry, page) = fixture();
    let connector = draw_glued(&mut ctx, &registry, page);
    ctx.selection.shapes = vec![connector];
    ctx.selection.anchor = Some(connector);

    // No cells at all: right angle.
    assert_eq!(connector_style(&ctx), Some(RouteStyle::RightAngle));

    // The page's pair reaches the connector.
    {
        let doc = &mut ctx.active_document_mut().expect("session").document;
        for (name, value) in [("RouteStyle", 1.0), ("LineRouteExt", 2.0)] {
            doc.write_cell(
                SheetRef::Page(page),
                &CellName::new(name),
                &larrygram_model::mutate::CellWrite::new(
                    larrygram_model::mutate::literal(value),
                    CellValue::Number(value),
                ),
            )
            .expect("page cell");
        }
    }
    assert_eq!(connector_style(&ctx), Some(RouteStyle::Curved));

    // The shape's own pair wins.
    {
        let doc = &mut ctx.active_document_mut().expect("session").document;
        for (name, value) in [("ShapeRouteStyle", 2.0), ("ConLineRouteExt", 1.0)] {
            doc.write_cell(
                SheetRef::Shape(connector),
                &CellName::new(name),
                &larrygram_model::mutate::CellWrite::new(
                    larrygram_model::mutate::literal(value),
                    CellValue::Number(value),
                ),
            )
            .expect("shape cell");
        }
    }
    assert_eq!(connector_style(&ctx), Some(RouteStyle::Straight));

    // Value 16, a directed value and an unknown value route as right
    // angle (ADR 0084 rule 8) over a curved page.
    for value in [16.0, 6.0, 99.0] {
        let doc = &mut ctx.active_document_mut().expect("session").document;
        doc.write_cell(
            SheetRef::Shape(connector),
            &CellName::new("ShapeRouteStyle"),
            &larrygram_model::mutate::CellWrite::new(
                larrygram_model::mutate::literal(value),
                CellValue::Number(value),
            ),
        )
        .expect("shape cell");
        assert_eq!(
            connector_style(&ctx),
            Some(RouteStyle::RightAngle),
            "ShapeRouteStyle {value}"
        );
    }

    // With nothing selected the read is the page default.
    ctx.selection.shapes.clear();
    ctx.selection.anchor = None;
    assert_eq!(connector_style(&ctx), Some(RouteStyle::Curved));
}

/// A page default change re-routes the connectors that inherit their
/// style (task P7-36 item 3).
#[test]
fn a_page_default_re_routes_the_connectors_that_inherit_it() {
    let (mut ctx, registry, page) = fixture();
    let connector = draw_glued(&mut ctx, &registry, page);
    ctx.selection.shapes.clear();
    ctx.selection.anchor = None;
    dispatch(
        &mut ctx,
        &registry,
        "connector.style",
        Args::new().with("style", ArgValue::Text("curved".to_owned())),
    )
    .expect("page default");
    assert_eq!(
        row_types(doc(&ctx), connector),
        vec!["MoveTo", "NURBSTo"],
        "the inheriting connector bends"
    );
}

/// The line jump controls stay disabled with our own tooltip (task
/// P7-36 item 4).
#[test]
fn the_line_jump_controls_are_disabled_with_our_reason() {
    use larrygram_commands::CommandId;
    let (ctx, registry, _page) = fixture();
    let ids = [
        "connector.show.line.jumps",
        "connector.line_jump_style",
        "connector.line_jump_style.page_default",
        "connector.line_jump_style.arc",
        "connector.line_jump_style.gap",
        "connector.line_jump_style.square",
        "connector.line_jump_style.sides_2",
        "connector.line_jump_style.sides_7",
        "connector.add_line_jumps_to",
        "connector.add_line_jumps_to.no_lines",
        "connector.add_line_jumps_to.horizontal_lines",
        "connector.add_line_jumps_to.vertical_lines",
        "connector.add_line_jumps_to.last_routed_line",
        "connector.add_line_jumps_to.last_displayed_line",
        "connector.add_line_jumps_to.first_displayed_line",
    ];
    for id in ids {
        let command = CommandId::intern(id).expect("id");
        let enabled = registry.enabled(&command, &ctx);
        assert!(!enabled.value, "{id} must be disabled");
        assert_eq!(
            enabled.reason.as_deref(),
            Some("Line jumps aren't available yet"),
            "{id}"
        );
    }
    // The registry refuses a dispatch too.
    let mut ctx = ctx;
    let error = dispatch(
        &mut ctx,
        &registry,
        "connector.line_jump_style.arc",
        Args::new(),
    )
    .expect_err("refused");
    assert!(
        error
            .to_string()
            .contains("Line jumps aren't available yet"),
        "{error}"
    );
}

/// A curved route moves with its targets on a re-route and never
/// panics on degenerate input (task P7-36 item 2).
#[test]
fn a_curved_route_re_routes_when_its_target_moves() {
    let (mut ctx, registry, page) = fixture();
    let connector = draw_glued(&mut ctx, &registry, page);
    dispatch(
        &mut ctx,
        &registry,
        "connector.style",
        Args::new().with("style", ArgValue::Text("curved".to_owned())),
    )
    .expect("curved");
    let left = shape(&ctx, page, 1);
    ctx.selection.shapes = vec![left];
    ctx.selection.anchor = Some(left);
    dispatch(
        &mut ctx,
        &registry,
        "shape.move",
        Args::new()
            .with("dx", ArgValue::Number(0.5))
            .with("dy", ArgValue::Number(0.0)),
    )
    .expect("move");
    assert_eq!(
        row_types(doc(&ctx), connector),
        vec!["MoveTo", "NURBSTo"],
        "the curve survives a re-route"
    );
}

// ---------------------------------------------------------------------------
// The curved draw and move (task P7-36 item 2)
// ---------------------------------------------------------------------------

/// A connector drawn under a curved page default is drawn curved, and
/// a body move keeps its curve (task P7-36 item 2).
#[test]
fn a_curved_draw_and_move_keep_the_curve() {
    let (mut ctx, registry, page) = fixture();
    // The page default is curved (nothing selected).
    dispatch(
        &mut ctx,
        &registry,
        "connector.style",
        Args::new().with("style", ArgValue::Text("curved".to_owned())),
    )
    .expect("page default");
    // A free-ended draw under the default.
    dispatch(
        &mut ctx,
        &registry,
        "connector.draw",
        Args::new()
            .with("stencil", ArgValue::Text("Basic Flowchart".to_owned()))
            .with("master", ArgValue::Text("Connector".to_owned()))
            .with("x", ArgValue::Number(3.0))
            .with("y", ArgValue::Number(4.5))
            .with("begin_x", ArgValue::Number(1.5))
            .with("begin_y", ArgValue::Number(4.0))
            .with("end_x", ArgValue::Number(4.5))
            .with("end_y", ArgValue::Number(6.0)),
    )
    .expect("draw");
    let connector = shape(&ctx, page, 3);
    assert_eq!(
        row_types(doc(&ctx), connector),
        vec!["MoveTo", "NURBSTo"],
        "the draw writes one NURBSTo row"
    );

    // A body move translates the curve and keeps the row (task P7-36
    // item 2: `translate_one_d` moves the control points).
    ctx.selection.shapes = vec![connector];
    ctx.selection.anchor = Some(connector);
    ctx.selection.page = Some(page);
    dispatch(
        &mut ctx,
        &registry,
        "shape.move",
        Args::new()
            .with("dx", ArgValue::Number(0.5))
            .with("dy", ArgValue::Number(0.25)),
    )
    .expect("move");
    assert_eq!(
        row_types(doc(&ctx), connector),
        vec!["MoveTo", "NURBSTo"],
        "the curve survives a body move"
    );
    // One undo restores the pre-move curve.
    dispatch(&mut ctx, &registry, "edit.undo", Args::new()).expect("undo");
    assert_eq!(row_types(doc(&ctx), connector), vec!["MoveTo", "NURBSTo"]);
}

/// Styling two connectors at once writes both and reports the plural
/// label (task P7-36 item 3).
#[test]
fn styling_two_connectors_writes_both() {
    let (mut ctx, registry, page) = fixture();
    draw_glued(&mut ctx, &registry, page);
    dispatch(
        &mut ctx,
        &registry,
        "connector.draw",
        Args::new()
            .with("stencil", ArgValue::Text("Basic Flowchart".to_owned()))
            .with("master", ArgValue::Text("Connector".to_owned()))
            .with("x", ArgValue::Number(2.0))
            .with("y", ArgValue::Number(6.5))
            .with("begin_target", ArgValue::Integer(1))
            .with("end_target", ArgValue::Integer(2)),
    )
    .expect("second draw");
    let first = shape(&ctx, page, 3);
    let second = shape(&ctx, page, 4);
    ctx.selection.shapes = vec![first, second];
    ctx.selection.anchor = Some(second);
    ctx.selection.page = Some(page);
    let outcome = dispatch(
        &mut ctx,
        &registry,
        "connector.style",
        Args::new().with("style", ArgValue::Text("straight".to_owned())),
    )
    .expect("style both");
    assert_eq!(
        outcome.message.as_deref(),
        Some("Connector style: straight")
    );
    for connector in [first, second] {
        assert_eq!(
            style_cells(doc(&ctx), connector),
            (Some(CellValue::Number(2.0)), Some(CellValue::Number(1.0)))
        );
    }
}

// ---------------------------------------------------------------------------
// The read helpers' fallbacks (task P7-36 item 3)
// ---------------------------------------------------------------------------

/// The style reads fall back cleanly: no document, an unplaced shape,
/// a stale page and a non-numeric page cell (task P7-36 item 3).
#[test]
fn the_style_reads_fall_back_cleanly() {
    use larrygram_model::{Cell, Preserved, Section, SectionName, Shape};
    // No document: no style.
    let bare = AppCtx::new(Settings::default());
    assert_eq!(connector_style(&bare), None);

    // An unplaced shape: right angle (no cells reach it).
    let (mut ctx, registry, page) = fixture();
    let placed = {
        let doc = &mut ctx.active_document_mut().expect("session").document;
        let mut shape = Shape::new(ShapeId { index: 0 }, 7);
        shape.sheet.cells = Vec::new();
        doc.add_shape(shape)
    };
    ctx.selection.shapes = vec![placed];
    ctx.selection.anchor = Some(placed);
    assert_eq!(connector_style(&ctx), Some(RouteStyle::RightAngle));

    // A page cell that is not a number falls through to right angle.
    {
        let doc = &mut ctx.active_document_mut().expect("session").document;
        if let Some(entry) = doc.pages.get_mut(page.index as usize) {
            entry.sheet.cells.push(Cell {
                name: CellName::new("RouteStyle"),
                formula: None,
                value: Some(CellValue::Str("x".to_owned())),
                unit: larrygram_geom::Unit::Unitsless,
                preserved: Preserved::new(),
            });
        }
    }
    ctx.selection.shapes.clear();
    ctx.selection.anchor = None;
    assert_eq!(connector_style(&ctx), Some(RouteStyle::RightAngle));

    // A stale active page: right angle (the page read fails).
    ctx.active_document_mut().expect("session").active_page = Some(PageId { index: 9 });
    assert_eq!(connector_style(&ctx), Some(RouteStyle::RightAngle));
    let _ = (
        &registry,
        SectionName::from_static("Geometry"),
        Section::new,
    );
}

/// A page default change re-routes the inheritors and skips a record
/// that names a gone shape (task P7-36 item 3).
#[test]
fn a_page_default_skips_a_record_naming_a_gone_shape() {
    use larrygram_model::Connect;
    let (mut ctx, registry, page) = fixture();
    draw_glued(&mut ctx, &registry, page);
    {
        let doc = &mut ctx.active_document_mut().expect("session").document;
        if let Some(entry) = doc.pages.get_mut(page.index as usize) {
            let mut record = Connect::new();
            record.from_sheet = 99;
            record.from_cell = "BeginX".to_owned();
            record.from_part = larrygram_model::mutate::VIS_BEGIN;
            record.to_sheet = 1;
            record.to_cell = "PinX".to_owned();
            record.to_part = larrygram_model::mutate::VIS_WHOLE_SHAPE;
            entry.connects.push(record);
        }
    }
    ctx.selection.shapes.clear();
    ctx.selection.anchor = None;
    dispatch(
        &mut ctx,
        &registry,
        "connector.style",
        Args::new().with("style", ArgValue::Text("curved".to_owned())),
    )
    .expect("page default");
    assert_eq!(
        row_types(doc(&ctx), shape(&ctx, page, 3)),
        vec!["MoveTo", "NURBSTo"]
    );
}

// ---------------------------------------------------------------------------
// The glue and draw argument arms (task P7-36 item 3's neighbours)
// ---------------------------------------------------------------------------

/// The glue and draw commands name their bad arguments (the glue
/// record and draw validation arms).
#[test]
fn the_glue_and_draw_commands_name_their_bad_arguments() {
    let (mut ctx, registry, page) = fixture();
    draw_glued(&mut ctx, &registry, page);
    for (point, expected) in [
        (-1, "point must not be negative"),
        (99, "connection point 99 does not exist"),
    ] {
        let error = dispatch(
            &mut ctx,
            &registry,
            "connector.glue_begin",
            Args::new()
                .with("connector", ArgValue::Integer(3))
                .with("target", ArgValue::Integer(1))
                .with("point", ArgValue::Integer(point)),
        )
        .expect_err("bad point");
        assert!(error.to_string().contains(expected), "{error}");
    }
    let error = dispatch(
        &mut ctx,
        &registry,
        "connector.draw",
        Args::new()
            .with("stencil", ArgValue::Text("Basic Flowchart".to_owned()))
            .with("master", ArgValue::Text("Connector".to_owned()))
            .with("x", ArgValue::Number(3.0))
            .with("y", ArgValue::Number(4.5))
            .with("begin_point", ArgValue::Integer(0)),
    )
    .expect_err("point without target");
    assert!(
        error
            .to_string()
            .contains("begin_point needs a begin_target"),
        "{error}"
    );
    let error = dispatch(
        &mut ctx,
        &registry,
        "connector.draw",
        Args::new()
            .with("stencil", ArgValue::Text("Basic Flowchart".to_owned()))
            .with("master", ArgValue::Text("Connector".to_owned()))
            .with("x", ArgValue::Number(3.0))
            .with("y", ArgValue::Number(4.5))
            .with("end_point", ArgValue::Integer(0)),
    )
    .expect_err("end point without target");
    assert!(
        error.to_string().contains("end_point needs an end_target"),
        "{error}"
    );
}

/// A point glue to an instance whose `Connection` rows live on its
/// master walks the master chain and scales by the instance's size
/// (the resolution's master rungs).
#[test]
fn point_glue_to_an_instances_master_points_resolves_them() {
    use larrygram_geom::Point;
    use larrygram_model::{Cell, Master, MasterId, Preserved, Row, Section, SectionName};
    let (mut ctx, registry, page) = fixture();
    let cell = |name: &str, value: f64| Cell {
        name: CellName::new(name),
        formula: Some(format!("{value}")),
        value: Some(CellValue::Number(value)),
        unit: larrygram_geom::Unit::Inches,
        preserved: Preserved::new(),
    };
    // A master with one directed connection point, instantiated at
    // twice its size.
    let mut master_shape = box_shape(4, 2.0, 2.0);
    {
        let mut connection = Section::new(SectionName::from_static("Connection"));
        let mut row = Row::new();
        row.row_type = Some("Connection".to_owned());
        row.ix = Some(0);
        row.cells = vec![
            cell("X", 2.0),
            cell("Y", 1.0),
            cell("DirX", 1.0),
            cell("DirY", 0.0),
        ];
        connection.rows.push(row);
        master_shape.sheet.sections.push(connection);
    }
    let instance = {
        let doc = &mut ctx.active_document_mut().expect("session").document;
        let master_shape = doc.add_shape(master_shape);
        let mut master = Master::new(MasterId { index: 0 }, 5);
        master.name_u = "Pointed".to_owned();
        master.shapes.push(master_shape);
        let master = doc.add_master(master).expect("master");
        doc.instantiate_master(master, page, Point::new(5.0, 2.0), None)
            .expect("instance")
    };
    let instance_visio = doc(&ctx).shape(instance).expect("instance").visio_id;
    // Glue the connector's begin end to the instance's point 0.
    let outcome = dispatch(
        &mut ctx,
        &registry,
        "connector.glue_begin",
        Args::new()
            .with("connector", ArgValue::Integer(3))
            .with("target", ArgValue::Integer(i64::from(instance_visio)))
            .with("point", ArgValue::Integer(0)),
    )
    .expect("glue to the master's point");
    assert!(
        outcome
            .message
            .as_deref()
            .is_some_and(|m| m.contains("glued")),
        "{:?}",
        outcome.message
    );
}

/// A connection point index that overflows names its problem: a file
/// may carry a row at `i32::MAX` (task P7-36 item 3's neighbours).
#[test]
fn an_overflowing_connection_point_index_is_named() {
    use larrygram_model::{Cell, Preserved, Row, Section, SectionName};
    let (mut ctx, registry, page) = fixture();
    let cell = |name: &str, value: f64| Cell {
        name: CellName::new(name),
        formula: Some(format!("{value}")),
        value: Some(CellValue::Number(value)),
        unit: larrygram_geom::Unit::Inches,
        preserved: Preserved::new(),
    };
    // A target whose one connection row sits at `i32::MAX`.
    draw_glued(&mut ctx, &registry, page);
    let mut odd = box_shape(5, 5.0, 2.0);
    odd.sheet.sections.clear();
    {
        let mut connection = Section::new(SectionName::from_static("Connection"));
        let mut row = Row::new();
        row.row_type = Some("Connection".to_owned());
        row.ix = Some(i32::MAX);
        row.cells = vec![cell("X", 1.0), cell("Y", 1.0)];
        connection.rows.push(row);
        odd.sheet.sections.push(connection);
    }
    let odd_visio = {
        let doc = &mut ctx.active_document_mut().expect("session").document;
        let odd = doc.insert_shape(page, odd, None).expect("odd target");
        doc.shape(odd).expect("odd target").visio_id
    };
    let error = dispatch(
        &mut ctx,
        &registry,
        "connector.glue_begin",
        Args::new()
            .with("connector", ArgValue::Integer(3))
            .with("target", ArgValue::Integer(i64::from(odd_visio)))
            .with("point", ArgValue::Integer(i64::from(i32::MAX))),
    )
    .expect_err("overflow");
    assert!(error.to_string().contains("too large"), "{error}");
}

// ---------------------------------------------------------------------------
// The move paths of a 1-D shape (task P7-36 item 2's neighbours)
// ---------------------------------------------------------------------------

/// A connector whose geometry rows carry formulas moves by its box:
/// the rows belong to recalc and only the ends and box move.
#[test]
fn a_formula_driven_connector_moves_by_its_box() {
    use larrygram_model::RowTarget;
    use larrygram_model::mutate::CellWrite as Write;
    let (mut ctx, registry, page) = fixture();
    draw_glued(&mut ctx, &registry, page);
    let connector = shape(&ctx, page, 3);
    {
        let doc = &mut ctx.active_document_mut().expect("session").document;
        doc.write_row_cell(
            SheetRef::Shape(connector),
            &larrygram_model::SectionName::from_static("Geometry"),
            RowTarget {
                position: Some(1),
                ix: Some(1),
            },
            &CellName::new("X"),
            &Write::new("Width*0.5", CellValue::Number(1.0)),
        )
        .expect("formula row");
    }
    let rows_before = row_types(doc(&ctx), connector);
    ctx.selection.shapes = vec![connector];
    ctx.selection.anchor = Some(connector);
    ctx.selection.page = Some(page);
    dispatch(
        &mut ctx,
        &registry,
        "shape.move",
        Args::new()
            .with("dx", ArgValue::Number(1.0))
            .with("dy", ArgValue::Number(0.0)),
    )
    .expect("move");
    assert_eq!(
        row_types(doc(&ctx), connector),
        rows_before,
        "the formula-driven rows belong to recalc"
    );
}

/// A 1-D shape whose end cells are not numeric cannot move: the
/// command names the problem and writes nothing.
#[test]
fn a_move_without_numeric_ends_is_named() {
    let (mut ctx, registry, page) = fixture();
    draw_glued(&mut ctx, &registry, page);
    let connector = shape(&ctx, page, 3);
    {
        let doc = &mut ctx.active_document_mut().expect("session").document;
        if let Some(entity) = doc.shape_mut(connector) {
            for cell in &mut entity.sheet.cells {
                if cell.name.as_str() == "BeginX" {
                    cell.value = Some(CellValue::Str("x".to_owned()));
                    cell.formula = None;
                }
            }
        }
    }
    ctx.selection.shapes = vec![connector];
    ctx.selection.anchor = Some(connector);
    ctx.selection.page = Some(page);
    let error = dispatch(
        &mut ctx,
        &registry,
        "shape.move",
        Args::new()
            .with("dx", ArgValue::Number(1.0))
            .with("dy", ArgValue::Number(0.0)),
    )
    .expect_err("no numeric ends");
    assert!(error.to_string().contains("no numeric point"), "{error}");
}

/// `connector.unglue` asks which end; `connector.draw` names its bad
/// numbers and its unknown master (the input arms).
#[test]
fn the_draw_and_unglue_input_arms_are_named() {
    use larrygram_commands::CommandError;
    let (mut ctx, registry, page) = fixture();
    draw_glued(&mut ctx, &registry, page);
    let missing = dispatch(
        &mut ctx,
        &registry,
        "connector.unglue",
        Args::new().with("connector", ArgValue::Integer(3)),
    )
    .expect_err("missing end");
    assert!(matches!(
        missing,
        CommandError::NeedsInput(larrygram_commands::NeededInput::Text { .. })
    ));
    let bad = dispatch(
        &mut ctx,
        &registry,
        "connector.draw",
        Args::new()
            .with("stencil", ArgValue::Text("Basic Flowchart".to_owned()))
            .with("master", ArgValue::Text("Connector".to_owned()))
            .with("x", ArgValue::Number(f64::NAN))
            .with("y", ArgValue::Number(4.5)),
    )
    .expect_err("non-finite x");
    assert!(bad.to_string().contains("x"), "{bad}");
    let unknown = dispatch(
        &mut ctx,
        &registry,
        "connector.draw",
        Args::new()
            .with("stencil", ArgValue::Text("No Such Stencil".to_owned()))
            .with("master", ArgValue::Text("Connector".to_owned()))
            .with("x", ArgValue::Number(3.0))
            .with("y", ArgValue::Number(4.5)),
    )
    .expect_err("unknown stencil");
    assert!(!unknown.to_string().is_empty(), "{unknown}");
    let _ = page;
}

/// Glue to an instance whose sizes are missing scales nothing: the
/// master rung's fallback axis scale is 1 (the resolution's master
/// rungs).
#[test]
fn master_points_on_an_unsized_instance_resolve_unscaled() {
    use larrygram_model::{Cell, Master, MasterId, Preserved, Row, Section, SectionName, Shape};
    let (mut ctx, registry, page) = fixture();
    let cell = |name: &str, value: f64| Cell {
        name: CellName::new(name),
        formula: Some(format!("{value}")),
        value: Some(CellValue::Number(value)),
        unit: larrygram_geom::Unit::Inches,
        preserved: Preserved::new(),
    };
    // A master shape with one connection row and no size cells, and
    // an instance of the master with no cells at all.
    let mut master_shape = Shape::new(ShapeId { index: 0 }, 5);
    {
        let mut connection = Section::new(SectionName::from_static("Connection"));
        let mut row = Row::new();
        row.row_type = Some("Connection".to_owned());
        row.ix = Some(0);
        row.cells = vec![cell("X", 1.0), cell("Y", 1.0)];
        connection.rows.push(row);
        master_shape.sheet.sections.push(connection);
    }
    let instance_visio = {
        let doc = &mut ctx.active_document_mut().expect("session").document;
        let master_shape = doc.add_shape(master_shape);
        let mut master = Master::new(MasterId { index: 0 }, 6);
        master.name_u = "Unsized".to_owned();
        master.shapes.push(master_shape);
        let master = doc.add_master(master).expect("master");
        let mut instance = Shape::new(ShapeId { index: 0 }, 7);
        instance.master = Some(master);
        let instance = doc.insert_shape(page, instance, None).expect("instance");
        doc.shape(instance).expect("instance").visio_id
    };
    let outcome = dispatch(
        &mut ctx,
        &registry,
        "connector.glue_begin",
        Args::new()
            .with("connector", ArgValue::Integer(3))
            .with("target", ArgValue::Integer(i64::from(instance_visio)))
            .with("point", ArgValue::Integer(0)),
    )
    .expect("glue to the master's point");
    assert!(
        outcome
            .message
            .as_deref()
            .is_some_and(|m| m.contains("glued")),
        "{:?}",
        outcome.message
    );
}

/// A point glue to a shape with no connection points anywhere names
/// the empty list (the resolution's empty-rung return).
#[test]
fn point_glue_to_a_shape_without_points_is_named() {
    let (mut ctx, registry, page) = fixture();
    let mut plain = Shape::new(ShapeId { index: 0 }, 5);
    plain.sheet.cells.push(Cell {
        name: CellName::new("PinX"),
        formula: Some("2".to_owned()),
        value: Some(CellValue::Number(2.0)),
        unit: larrygram_geom::Unit::Inches,
        preserved: Preserved::new(),
    });
    let plain_visio = {
        let doc = &mut ctx.active_document_mut().expect("session").document;
        let plain = doc.insert_shape(page, plain, None).expect("plain");
        doc.shape(plain).expect("plain").visio_id
    };
    let error = dispatch(
        &mut ctx,
        &registry,
        "connector.glue_begin",
        Args::new()
            .with("connector", ArgValue::Integer(3))
            .with("target", ArgValue::Integer(i64::from(plain_visio)))
            .with("point", ArgValue::Integer(0)),
    )
    .expect_err("no points");
    assert!(error.to_string().contains("does not exist"), "{error}");
}

/// The page style read walks the page's style chain and stops at a
/// dangling link (the lookup chain's break arm).
#[test]
fn the_page_style_read_stops_at_a_dangling_style_link() {
    let (mut ctx, registry, page) = fixture();
    draw_glued(&mut ctx, &registry, page);
    {
        let doc = &mut ctx.active_document_mut().expect("session").document;
        if let Some(entry) = doc.pages.get_mut(page.index as usize) {
            entry.sheet.cells.push(Cell {
                name: CellName::new("LineStyle"),
                formula: None,
                value: Some(CellValue::Number(999.0)),
                unit: larrygram_geom::Unit::Unitsless,
                preserved: Preserved::new(),
            });
        }
    }
    ctx.selection.shapes.clear();
    ctx.selection.anchor = None;
    dispatch(
        &mut ctx,
        &registry,
        "connector.style",
        Args::new().with("style", ArgValue::Text("straight".to_owned())),
    )
    .expect("page default");
    assert_eq!(
        page_cell(&ctx, page, "RouteStyle"),
        Some(CellValue::Number(2.0))
    );
    let _ = &registry;
}

/// The style reads handle an unplaced 1-D shape and a document with
/// no active page (task P7-36 item 3's read arms).
#[test]
fn the_style_reads_handle_an_unplaced_connector_and_no_page() {
    use larrygram_model::Shape;
    let (mut ctx, _registry, page) = fixture();
    let cell = |name: &str, value: f64| Cell {
        name: CellName::new(name),
        formula: Some(format!("{value}")),
        value: Some(CellValue::Number(value)),
        unit: larrygram_geom::Unit::Inches,
        preserved: Preserved::new(),
    };
    // A routable 1-D shape that is not on any page.
    let mut one_d = Shape::new(ShapeId { index: 0 }, 8);
    one_d.sheet.cells = vec![
        cell("BeginX", 0.0),
        cell("BeginY", 0.0),
        cell("EndX", 1.0),
        cell("EndY", 0.0),
        cell("Width", 1.0),
        cell("Height", 0.2),
        cell("ObjType", 2.0),
    ];
    let placed = {
        let doc = &mut ctx.active_document_mut().expect("session").document;
        doc.add_shape(one_d)
    };
    ctx.selection.shapes = vec![placed];
    ctx.selection.anchor = Some(placed);
    assert_eq!(connector_style(&ctx), Some(RouteStyle::RightAngle));

    // With nothing selected and no active page there is no style.
    ctx.selection.shapes.clear();
    ctx.selection.anchor = None;
    ctx.active_document_mut().expect("session").active_page = None;
    assert_eq!(connector_style(&ctx), None);
    let _ = page;
}

/// A 1-D shape whose rows give fewer than two points moves as the
/// straight line between its ends (the translate fallback).
#[test]
fn a_move_of_a_single_row_path_uses_the_ends() {
    use larrygram_model::{Row, SectionName};
    let (mut ctx, registry, page) = fixture();
    draw_glued(&mut ctx, &registry, page);
    let connector = shape(&ctx, page, 3);
    {
        let doc = &mut ctx.active_document_mut().expect("session").document;
        if let Some(entity) = doc.shape_mut(connector) {
            for section in &mut entity.sheet.sections {
                if section.name.as_str() == "Geometry" {
                    let mut row = Row::new();
                    row.row_type = Some("MoveTo".to_owned());
                    row.ix = Some(0);
                    row.cells = vec![
                        Cell {
                            name: CellName::new("X"),
                            formula: Some("0".to_owned()),
                            value: Some(CellValue::Number(0.0)),
                            unit: larrygram_geom::Unit::Inches,
                            preserved: Preserved::new(),
                        },
                        Cell {
                            name: CellName::new("Y"),
                            formula: Some("0".to_owned()),
                            value: Some(CellValue::Number(0.0)),
                            unit: larrygram_geom::Unit::Inches,
                            preserved: Preserved::new(),
                        },
                    ];
                    section.rows = vec![row];
                }
            }
        }
        let _ = SectionName::from_static("Geometry");
    }
    ctx.selection.shapes = vec![connector];
    ctx.selection.anchor = Some(connector);
    ctx.selection.page = Some(page);
    dispatch(
        &mut ctx,
        &registry,
        "shape.move",
        Args::new()
            .with("dx", ArgValue::Number(1.0))
            .with("dy", ArgValue::Number(0.0)),
    )
    .expect("move");
    assert_eq!(
        row_types(doc(&ctx), connector),
        vec!["MoveTo", "LineTo"],
        "the move falls back to the straight line between the ends"
    );
}
