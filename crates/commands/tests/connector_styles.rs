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
use larrygram_model::{CellName, CellValue, Document, GeometryRow, PageId, ShapeId, SheetRef};

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
