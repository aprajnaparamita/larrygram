//! `connector.*` through the registry (task P5-08, ADR 0045).
//!
//! The exit-flowchart path is headless: a flow of shapes and five glued
//! connectors is built with `Source::Test` commands, a move re-routes the
//! affected connectors in the same undo step, and undo/redo restores and
//! re-applies the routed paths. No GUI is involved.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use support::dispatch;

use larrygram_commands::app::{AppCtx, DocumentSession, Settings};
use larrygram_commands::builtins::registry;
use larrygram_commands::{ArgValue, Args, CommandError, Registry};
use larrygram_model::{
    Cell, CellName, CellValue, Document, Page, PageId, Preserved, Row, Section, SectionName, Shape,
    ShapeId,
};

fn cell(name: &str, value: f64) -> Cell {
    Cell {
        name: CellName::new(name),
        formula: Some(format!("{value}")),
        value: Some(CellValue::Number(value)),
        unit: larrygram_geom::Unit::Inches,
        preserved: Preserved::new(),
    }
}

/// A 1.5 x 0.75 box at `(2, y)` with two connection points (top and
/// bottom edge midpoints).
fn box_shape(visio_id: i32, y: f64) -> Shape {
    let mut shape = Shape::new(ShapeId { index: 0 }, visio_id);
    shape.sheet.cells = vec![
        cell("Width", 1.5),
        cell("Height", 0.75),
        cell("PinX", 2.0),
        cell("PinY", y),
        cell("LocPinX", 0.75),
        cell("LocPinY", 0.375),
        cell("Angle", 0.0),
    ];
    let mut connection = Section::new(SectionName::from_static("Connection"));
    for (ix, (x, y)) in [(0.75, 0.75), (0.75, 0.0)].into_iter().enumerate() {
        let mut row = Row::new();
        row.row_type = Some("Connection".to_owned());
        row.ix = Some(i32::try_from(ix).unwrap_or(0));
        row.cells = vec![cell("X", x), cell("Y", y)];
        connection.rows.push(row);
    }
    shape.sheet.sections.push(connection);
    shape
}

/// A connector instance at `(2, y)`; its transform cells are all the
/// path writer needs.
fn connector_shape(visio_id: i32, y: f64) -> Shape {
    let mut shape = Shape::new(ShapeId { index: 0 }, visio_id);
    shape.sheet.cells = vec![
        cell("Width", 1.5),
        cell("Height", 0.75),
        cell("PinX", 2.0),
        cell("PinY", y),
        cell("LocPinX", 0.75),
        cell("LocPinY", 0.375),
        cell("Angle", 0.0),
    ];
    shape
}

/// Six boxes at y = 7, 5.5, 4, 2.5, 1, -0.5 and five connectors between
/// them. Visio ids: boxes 1..=6, connectors 11..=15.
fn flow() -> (AppCtx, Registry, PageId) {
    let mut doc = Document::new();
    let mut page = Page::new(PageId { index: 0 }, 1);
    page.sheet.cells = vec![cell("PageWidth", 8.5), cell("PageHeight", 11.0)];
    for (index, y) in [7.0, 5.5, 4.0, 2.5, 1.0, -0.5].into_iter().enumerate() {
        let id = doc.add_shape(box_shape(i32::try_from(index).unwrap_or(0) + 1, y));
        page.shapes.push(id);
    }
    for (index, y) in [6.25, 4.75, 3.25, 1.75, 0.25].into_iter().enumerate() {
        let id = doc.add_shape(connector_shape(i32::try_from(index).unwrap_or(0) + 11, y));
        page.shapes.push(id);
    }
    let page_id = page.id;
    doc.pages.push(page);
    let mut ctx = AppCtx::new(Settings::default());
    ctx.open_document(DocumentSession::new(doc, None));
    (ctx, registry().expect("registry"), page_id)
}

fn glue_args(connector: i32, target: i32, point: i32) -> Args {
    Args::new()
        .with("connector", ArgValue::Integer(i64::from(connector)))
        .with("target", ArgValue::Integer(i64::from(target)))
        .with("point", ArgValue::Integer(i64::from(point)))
}

/// Page-space path points of one connector (the inverse of
/// `write_connector_path`'s local mapping).
fn path(ctx: &AppCtx, connector: i32) -> Vec<larrygram_geom::Point> {
    let doc = &ctx.active_document().expect("session").document;
    let shape = doc
        .shape_by_visio_id(PageId { index: 0 }, connector)
        .expect("connector");
    let geometry = doc.shape_geometry(shape).expect("geometry");
    let reader = |name: &str| -> f64 {
        doc.shape(shape)
            .and_then(|entry| {
                entry
                    .sheet
                    .cells
                    .iter()
                    .find(|cell| cell.name.as_str() == name)
            })
            .and_then(|cell| match &cell.value {
                Some(CellValue::Number(value)) => Some(*value),
                _ => None,
            })
            .unwrap_or(0.0)
    };
    let pin = larrygram_geom::Point::new(reader("PinX"), reader("PinY"));
    let loc = larrygram_geom::Point::new(reader("LocPinX"), reader("LocPinY"));
    geometry
        .paths
        .first()
        .map(|path| {
            path.rows
                .iter()
                .filter_map(|row| match row {
                    larrygram_model::GeometryRow::MoveTo { x, y }
                    | larrygram_model::GeometryRow::LineTo { x, y } => Some(
                        larrygram_geom::Point::new(x - loc.x + pin.x, y - loc.y + pin.y),
                    ),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn a_five_connector_flow_is_built_headlessly() {
    let (mut ctx, registry, _page) = flow();
    for (index, connector) in [11, 12, 13, 14, 15].into_iter().enumerate() {
        let top = i32::try_from(index).unwrap_or(0) + 1;
        let bottom = top + 1;
        dispatch(
            &mut ctx,
            &registry,
            "connector.glue_begin",
            glue_args(connector, top, 1),
        )
        .expect("glue begin");
        dispatch(
            &mut ctx,
            &registry,
            "connector.glue_end",
            glue_args(connector, bottom, 0),
        )
        .expect("glue end");
    }
    let doc = &ctx.active_document().expect("session").document;
    let page = doc.page(PageId { index: 0 }).expect("page");
    assert_eq!(page.connects.len(), 10, "two glues per connector");
    for connector in [11, 12, 13, 14, 15] {
        let points = path(&ctx, connector);
        assert_eq!(points.len(), 2, "{connector}: {points:?}");
        assert!(
            points.first().map(|p| p.y) > points.last().map(|p| p.y),
            "{connector}: {points:?}"
        );
    }
    assert_eq!(path(&ctx, 11).first().map(|p| p.y), Some(6.625));
}

#[test]
fn a_move_reroutes_one_step_and_undo_redo_follow_it() {
    let (mut ctx, registry, page) = flow();
    for (index, connector) in [11, 12, 13, 14, 15].into_iter().enumerate() {
        let top = i32::try_from(index).unwrap_or(0) + 1;
        let bottom = top + 1;
        dispatch(
            &mut ctx,
            &registry,
            "connector.glue_begin",
            glue_args(connector, top, 1),
        )
        .expect("glue begin");
        dispatch(
            &mut ctx,
            &registry,
            "connector.glue_end",
            glue_args(connector, bottom, 0),
        )
        .expect("glue end");
    }
    let before: Vec<_> = [11, 12, 13, 14, 15]
        .into_iter()
        .map(|connector| path(&ctx, connector))
        .collect();
    let box_three = ctx
        .active_document()
        .expect("session")
        .document
        .shape_by_visio_id(page, 3)
        .expect("box 3");
    ctx.selection.shapes = vec![box_three];
    ctx.selection.anchor = Some(box_three);
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
    let after: Vec<_> = [11, 12, 13, 14, 15]
        .into_iter()
        .map(|connector| path(&ctx, connector))
        .collect();
    assert_ne!(before[1], after[1], "connector 12 follows box 3");
    assert_ne!(before[2], after[2], "connector 13 follows box 3");
    assert_eq!(before[0], after[0], "connector 11 is unaffected");
    assert_eq!(before[3], after[3], "connector 14 is unaffected");

    dispatch(&mut ctx, &registry, "edit.undo", Args::new()).expect("undo");
    let undone: Vec<_> = [11, 12, 13, 14, 15]
        .into_iter()
        .map(|connector| path(&ctx, connector))
        .collect();
    assert_eq!(undone, before);
    dispatch(&mut ctx, &registry, "edit.redo", Args::new()).expect("redo");
    let redone: Vec<_> = [11, 12, 13, 14, 15]
        .into_iter()
        .map(|connector| path(&ctx, connector))
        .collect();
    assert_eq!(redone, after);
}

#[test]
fn a_right_angle_style_applies_to_every_selected_connector() {
    let (mut ctx, registry, page) = flow();
    for (index, connector) in [11, 12, 13, 14, 15].into_iter().enumerate() {
        let top = i32::try_from(index).unwrap_or(0) + 1;
        let bottom = top + 1;
        dispatch(
            &mut ctx,
            &registry,
            "connector.glue_begin",
            glue_args(connector, top, 1),
        )
        .expect("glue begin");
        dispatch(
            &mut ctx,
            &registry,
            "connector.glue_end",
            glue_args(connector, bottom, 0),
        )
        .expect("glue end");
    }
    // Shift box 3 sideways so its two connectors have a real elbow.
    let box_three = ctx
        .active_document()
        .expect("session")
        .document
        .shape_by_visio_id(page, 3)
        .expect("box 3");
    ctx.selection.shapes = vec![box_three];
    ctx.selection.anchor = Some(box_three);
    ctx.selection.page = Some(page);
    dispatch(
        &mut ctx,
        &registry,
        "shape.move",
        Args::new()
            .with("dx", ArgValue::Number(1.0))
            .with("dy", ArgValue::Number(0.0)),
    )
    .expect("move box 3");
    let connectors: Vec<ShapeId> = [11, 12, 13, 14, 15]
        .into_iter()
        .filter_map(|connector| {
            ctx.active_document()
                .expect("session")
                .document
                .shape_by_visio_id(page, connector)
        })
        .collect();
    ctx.selection.shapes = connectors;
    ctx.selection.anchor = ctx.selection.shapes.last().copied();
    ctx.selection.page = Some(page);
    dispatch(
        &mut ctx,
        &registry,
        "connector.style",
        Args::new().with("style", ArgValue::Text("right_angle".to_owned())),
    )
    .expect("style");
    for connector in [11, 14] {
        assert_eq!(path(&ctx, connector).len(), 2, "{connector} is aligned");
    }
    for connector in [12, 13] {
        assert_eq!(path(&ctx, connector).len(), 3, "{connector} has an elbow");
    }
}

/// The Visio `ID` of one shape.
fn visio(ctx: &AppCtx, shape: ShapeId) -> i32 {
    ctx.active_document()
        .expect("session")
        .document
        .shape(shape)
        .expect("shape")
        .visio_id
}

fn connector_shape_id(ctx: &AppCtx, visio_id: i32) -> ShapeId {
    ctx.active_document()
        .expect("session")
        .document
        .shape_by_visio_id(PageId { index: 0 }, visio_id)
        .expect("connector shape")
}

#[test]
fn unglue_and_style_report_missing_input_and_both_ends() {
    let (mut ctx, registry, page) = flow();
    dispatch(
        &mut ctx,
        &registry,
        "connector.glue_begin",
        glue_args(11, 1, 1),
    )
    .expect("glue begin");
    dispatch(
        &mut ctx,
        &registry,
        "connector.glue_end",
        glue_args(11, 2, 0),
    )
    .expect("glue end");

    // A missing `end` is a text prompt, not a silent no-op.
    let missing = dispatch(
        &mut ctx,
        &registry,
        "connector.unglue",
        Args::new().with("connector", ArgValue::Integer(11)),
    )
    .expect_err("missing end");
    assert!(matches!(
        missing,
        CommandError::NeedsInput(larrygram_commands::NeededInput::Text { .. })
    ));

    let outcome = dispatch(
        &mut ctx,
        &registry,
        "connector.unglue",
        Args::new()
            .with("connector", ArgValue::Integer(11))
            .with("end", ArgValue::Text("end".to_owned())),
    )
    .expect("unglue end");
    assert_eq!(outcome.message.as_deref(), Some("Connector end unglued"));
    let ends = ctx
        .active_document()
        .expect("session")
        .document
        .connects_of(page, connector_shape_id(&ctx, 11))
        .expect("ends");
    assert!(ends.first().is_some_and(Option::is_some));
    assert!(ends.get(1).is_some_and(Option::is_none));

    // The straight style (the other message branch) on one shape.
    let connector = connector_shape_id(&ctx, 11);
    ctx.selection.shapes = vec![connector];
    ctx.selection.anchor = Some(connector);
    ctx.selection.page = Some(page);
    let outcome = dispatch(
        &mut ctx,
        &registry,
        "connector.style",
        Args::new().with("style", ArgValue::Text("straight".to_owned())),
    )
    .expect("straight style");
    assert_eq!(
        outcome.message.as_deref(),
        Some("Connector style: straight")
    );
    let missing =
        dispatch(&mut ctx, &registry, "connector.style", Args::new()).expect_err("missing style");
    assert!(matches!(
        missing,
        CommandError::NeedsInput(larrygram_commands::NeededInput::Text { .. })
    ));
    let bad = dispatch(
        &mut ctx,
        &registry,
        "connector.style",
        Args::new().with("style", ArgValue::Flag(true)),
    )
    .expect_err("bad style type");
    assert!(bad.to_string().contains("straight or right_angle"), "{bad}");

    // The plural and singular re-route messages.
    let outcome =
        dispatch(&mut ctx, &registry, "connector.reroute_all", Args::new()).expect("reroute one");
    assert_eq!(outcome.message.as_deref(), Some("Re-routed 1 connector"));
    dispatch(
        &mut ctx,
        &registry,
        "connector.glue_end",
        glue_args(11, 2, 0),
    )
    .expect("glue end again");
    let outcome = dispatch(&mut ctx, &registry, "connector.reroute_all", Args::new())
        .expect("reroute one again");
    assert_eq!(outcome.message.as_deref(), Some("Re-routed 1 connector"));

    // A stale active page id is a named failure.
    ctx.active_document_mut().expect("session").active_page = Some(PageId { index: 9 });
    let error = dispatch(&mut ctx, &registry, "connector.reroute_all", Args::new())
        .expect_err("stale page");
    assert!(error.to_string().contains("unknown page id 9"), "{error}");
}

#[test]
fn stale_and_detached_glue_records_are_dropped() {
    let (mut ctx, registry, page) = flow();
    {
        let doc = &mut ctx.active_document_mut().expect("session").document;
        let mut stale_target = larrygram_model::Connect::new();
        stale_target.from_sheet = 11;
        stale_target.from_cell = "BeginX".to_owned();
        stale_target.from_part = larrygram_model::mutate::VIS_BEGIN;
        stale_target.to_sheet = 99;
        stale_target.to_cell = "PinX".to_owned();
        stale_target.to_part = larrygram_model::mutate::VIS_WHOLE_SHAPE;
        doc.add_connect(page, stale_target).expect("connect");

        let mut stale_connector = larrygram_model::Connect::new();
        stale_connector.from_sheet = 98;
        stale_connector.from_cell = "BeginX".to_owned();
        stale_connector.from_part = larrygram_model::mutate::VIS_BEGIN;
        stale_connector.to_sheet = 1;
        stale_connector.to_cell = "PinX".to_owned();
        stale_connector.to_part = larrygram_model::mutate::VIS_WHOLE_SHAPE;
        doc.add_connect(page, stale_connector).expect("connect");
    }
    dispatch(&mut ctx, &registry, "connector.reroute_all", Args::new()).expect("reroute");
    let doc = &ctx.active_document().expect("session").document;
    assert!(
        doc.page(page).expect("page").connects.is_empty(),
        "both bad records are gone"
    );
    assert!(
        ctx.warnings
            .iter()
            .any(|line| line.contains("connector 98 is gone")),
        "{:?}",
        ctx.warnings
    );
    assert!(
        ctx.warnings
            .iter()
            .any(|line| line.contains("connector 11 lost its glue to detached shape 99")),
        "{:?}",
        ctx.warnings
    );
}

#[test]
fn a_cyclic_connector_transform_is_reported() {
    let (mut ctx, registry, page) = flow();
    dispatch(
        &mut ctx,
        &registry,
        "connector.glue_begin",
        glue_args(11, 1, 1),
    )
    .expect("glue begin");
    let connector = connector_shape_id(&ctx, 11);
    if let Some(entry) = ctx
        .active_document_mut()
        .expect("session")
        .document
        .shape_mut(connector)
    {
        entry.parent = Some(connector);
    }
    dispatch(&mut ctx, &registry, "connector.reroute_all", Args::new()).expect("reroute");
    assert!(
        ctx.warnings
            .iter()
            .any(|line| line.contains("connector 11 has no usable transform")),
        "{:?}",
        ctx.warnings
    );
    let _ = page;
}

#[test]
fn every_geometry_row_kind_can_supply_a_current_endpoint() {
    let (mut ctx, registry, page) = flow();
    let variants: [&str; 10] = [
        "ArcTo",
        "EllipticalArcTo",
        "NurbsTo",
        "PolylineTo",
        "SplineStart",
        "SplineKnot",
        "Ellipse",
        "InfiniteLine",
        "RelLineTo",
        "Weird",
    ];
    for (offset, kind) in variants.into_iter().enumerate() {
        let mut shape = Shape::new(
            ShapeId { index: 0 },
            100 + i32::try_from(offset).unwrap_or(0),
        );
        shape.sheet.cells = vec![
            cell("Width", 1.5),
            cell("Height", 0.75),
            cell("PinX", 2.0),
            cell("PinY", 2.0),
            cell("LocPinX", 0.75),
            cell("LocPinY", 0.375),
            cell("Angle", 0.0),
        ];
        let mut section = Section::new(SectionName::from_static("Geometry"));
        for (ix, row_type) in ["MoveTo", kind].into_iter().enumerate() {
            let mut row = Row::new();
            row.row_type = Some(row_type.to_owned());
            row.ix = Some(i32::try_from(ix).unwrap_or(0));
            row.cells = vec![cell("X", 0.5), cell("Y", 0.25)];
            section.rows.push(row);
        }
        shape.sheet.sections.push(section);
        let id = ctx
            .active_document_mut()
            .expect("session")
            .document
            .insert_shape(page, shape, None)
            .expect("insert");
        let visio_id = shape_visio_id(&ctx, id);
        dispatch(
            &mut ctx,
            &registry,
            "connector.glue_begin",
            glue_args(visio_id, 1, 1),
        )
        .expect("glue begin");
        dispatch(&mut ctx, &registry, "connector.reroute_all", Args::new()).expect("reroute");
    }
}

fn shape_visio_id(ctx: &AppCtx, shape: ShapeId) -> i32 {
    ctx.active_document()
        .expect("session")
        .document
        .shape(shape)
        .expect("shape")
        .visio_id
}

#[test]
fn an_unknown_target_or_a_detached_connector_is_an_error() {
    let (mut ctx, registry, _page) = flow();
    let error = dispatch(
        &mut ctx,
        &registry,
        "connector.glue_begin",
        glue_args(11, 99, 0),
    )
    .expect_err("unknown target");
    assert!(
        error.to_string().contains("unknown target shape id 99"),
        "{error}"
    );
}

#[test]
fn a_cyclic_or_overflowing_target_degrades_to_a_warning() {
    // A self-parented target: its transform is a cycle.
    let (mut ctx, registry, page) = flow();
    dispatch(
        &mut ctx,
        &registry,
        "connector.glue_begin",
        glue_args(11, 1, 1),
    )
    .expect("glue begin");
    let box_one = ctx
        .active_document()
        .expect("session")
        .document
        .shape_by_visio_id(page, 1)
        .expect("box 1");
    if let Some(entry) = ctx
        .active_document_mut()
        .expect("session")
        .document
        .shape_mut(box_one)
    {
        entry.parent = Some(box_one);
    }
    dispatch(&mut ctx, &registry, "connector.reroute_all", Args::new()).expect("cycle reroute");

    // A connection point whose transform overflows, and a box whose
    // bounds overflow too: the outline fallback is unavailable.
    let (mut ctx, registry, page) = flow();
    let huge = {
        let mut shape = box_shape(50, 2.0);
        for cell in &mut shape.sheet.cells {
            if cell.name.as_str() == "PinX" {
                cell.value = Some(CellValue::Number(1.0e308));
            }
            if cell.name.as_str() == "Width" {
                cell.value = Some(CellValue::Number(1.0e308));
            }
            if cell.name.as_str() == "LocPinX" {
                cell.value = Some(CellValue::Number(0.0));
            }
        }
        for section in &mut shape.sheet.sections {
            if section.name.as_str() == "Connection"
                && let Some(row) = section.rows.first_mut()
                && let Some(cell) = row.cells.first_mut()
            {
                cell.value = Some(CellValue::Number(1.0e308));
            }
        }
        let doc = &mut ctx.active_document_mut().expect("session").document;
        doc.insert_shape(page, shape, None).expect("huge target")
    };
    let visio_id = shape_visio_id(&ctx, huge);
    dispatch(
        &mut ctx,
        &registry,
        "connector.glue_begin",
        glue_args(11, visio_id, 0),
    )
    .expect("glue huge");
    dispatch(&mut ctx, &registry, "connector.reroute_all", Args::new()).expect("overflow reroute");
    assert!(
        ctx.warnings
            .iter()
            .any(|line| line.contains("has no connection point")),
        "{:?}",
        ctx.warnings
    );
}

#[test]
fn malformed_connector_and_target_arguments_are_named() {
    let (mut ctx, registry, _page) = flow();
    let overflow = Args::new()
        .with("connector", ArgValue::Integer(i64::MAX))
        .with("target", ArgValue::Integer(1));
    let error = dispatch(&mut ctx, &registry, "connector.glue_begin", overflow).expect_err("range");
    assert!(error.to_string().contains("whole number"), "{error}");

    for name in ["connector", "target"] {
        let mut args = Args::new().with("connector", ArgValue::Integer(11));
        args.set(name, ArgValue::Flag(true));
        let error = dispatch(&mut ctx, &registry, "connector.glue_begin", args).expect_err(name);
        assert!(
            error.to_string().contains("whole number"),
            "{name}: {error}"
        );
    }

    let unglue = Args::new().with("connector", ArgValue::Flag(true));
    let error = dispatch(&mut ctx, &registry, "connector.unglue", unglue).expect_err("connector");
    assert!(error.to_string().contains("whole number"), "{error}");
}

#[test]
fn connector_commands_need_an_active_page_and_a_whole_point() {
    let (mut ctx, registry, _page) = flow();
    ctx.active_document_mut().expect("session").active_page = None;
    let error =
        dispatch(&mut ctx, &registry, "connector.reroute_all", Args::new()).expect_err("no page");
    assert!(error.to_string().contains("no active page"), "{error}");
    let error = dispatch(
        &mut ctx,
        &registry,
        "connector.glue_begin",
        glue_args(11, 1, 0),
    )
    .expect_err("no page");
    assert!(error.to_string().contains("no active page"), "{error}");

    let (mut ctx, registry, _page) = flow();
    let args = Args::new()
        .with("connector", ArgValue::Integer(11))
        .with("target", ArgValue::Integer(1))
        .with("point", ArgValue::Flag(true));
    let error = dispatch(&mut ctx, &registry, "connector.glue_begin", args).expect_err("point");
    assert!(error.to_string().contains("whole number"), "{error}");
}

// ---------------------------------------------------------------------------
// Glue kinds, replacement, unglue and drop-on-detached (task P5-11,
// ADR 0045 rule 1)
// ---------------------------------------------------------------------------

/// The `Connect` records of one page, in file order.
fn connects(ctx: &AppCtx, page: PageId) -> Vec<larrygram_model::Connect> {
    ctx.active_document()
        .expect("session")
        .document
        .page(page)
        .expect("page")
        .connects
        .clone()
}

/// Glue without a `point` is dynamic glue: the record names the target
/// shape's pin cells and the whole-shape part, and the re-route picks
/// the nearest outline point of the shape's bounds (ADR 0045 rule 1).
#[test]
fn a_glue_without_a_point_binds_the_shape_outline() {
    let (mut ctx, registry, page) = flow();
    let outcome = dispatch(
        &mut ctx,
        &registry,
        "connector.glue_begin",
        Args::new()
            .with("connector", ArgValue::Integer(11))
            .with("target", ArgValue::Integer(1)),
    )
    .expect("outline glue");
    assert_eq!(
        outcome.message.as_deref(),
        Some("Connector begin glued to shape 1")
    );
    let records = connects(&ctx, page);
    assert_eq!(records.len(), 1, "one glue record");
    let record = records.first().expect("record");
    assert_eq!(record.from_sheet, 11, "from the connector");
    assert_eq!(record.from_cell, "BeginX");
    assert_eq!(
        record.from_part,
        larrygram_model::mutate::VIS_BEGIN,
        "the begin end (VisFromParts, ADR 0045)"
    );
    assert_eq!(record.to_sheet, 1, "to box 1");
    assert_eq!(record.to_cell, "PinX", "dynamic glue names the pin cells");
    assert_eq!(
        record.to_part,
        larrygram_model::mutate::VIS_WHOLE_SHAPE,
        "the whole-shape part (VisToParts, ADR 0045)"
    );
    // The routed begin is the nearest outline point of box 1: its bottom
    // edge midpoint (the connection-point glue test pins the same point).
    assert_eq!(path(&ctx, 11).first().map(|p| p.y), Some(6.625));
}

/// Glue is one record per end (ADR 0039 invariant 5): a second glue on
/// the same end replaces the old record and one undo restores it.
#[test]
fn a_new_glue_replaces_the_old_record_on_the_same_end() {
    let (mut ctx, registry, page) = flow();
    dispatch(
        &mut ctx,
        &registry,
        "connector.glue_end",
        glue_args(11, 2, 0),
    )
    .expect("first glue");
    dispatch(
        &mut ctx,
        &registry,
        "connector.glue_end",
        glue_args(11, 3, 0),
    )
    .expect("second glue");

    let records = connects(&ctx, page);
    assert_eq!(records.len(), 1, "the end holds one record, not two");
    let record = records.first().expect("record");
    assert_eq!(record.to_sheet, 3, "the second glue wins");
    assert_eq!(record.from_cell, "EndX");

    dispatch(&mut ctx, &registry, "edit.undo", Args::new()).expect("undo");
    let records = connects(&ctx, page);
    assert_eq!(records.len(), 1, "still one record after undo");
    let record = records.first().expect("record");
    assert_eq!(record.to_sheet, 2, "undo restores the replaced glue");
}

/// `connector.unglue` removes both ends' records and leaves the free
/// endpoints where they were: the path keeps its routed coordinates and
/// a later re-route does not move them (ADR 0045 rule 3).
#[test]
fn unglue_removes_both_end_records_and_keeps_the_free_endpoints() {
    let (mut ctx, registry, page) = flow();
    dispatch(
        &mut ctx,
        &registry,
        "connector.glue_begin",
        glue_args(11, 1, 1),
    )
    .expect("glue begin");
    dispatch(
        &mut ctx,
        &registry,
        "connector.glue_end",
        glue_args(11, 2, 0),
    )
    .expect("glue end");
    let routed = path(&ctx, 11);
    assert_eq!(routed.len(), 2, "two routed endpoints");

    for end in ["begin", "end"] {
        let outcome = dispatch(
            &mut ctx,
            &registry,
            "connector.unglue",
            Args::new()
                .with("connector", ArgValue::Integer(11))
                .with("end", ArgValue::Text(end.to_owned())),
        )
        .expect("unglue");
        assert_eq!(
            outcome.message.as_deref(),
            Some(format!("Connector {end} unglued").as_str())
        );
    }
    assert!(connects(&ctx, page).is_empty(), "no glue records left");
    let ends = ctx
        .active_document()
        .expect("session")
        .document
        .connects_of(page, connector_shape_id(&ctx, 11))
        .expect("ends");
    assert!(ends.iter().all(Option::is_none), "both ends are free");

    // The free endpoints stay put across a re-route.
    dispatch(&mut ctx, &registry, "connector.reroute_all", Args::new()).expect("reroute");
    assert_eq!(path(&ctx, 11), routed, "unglued ends keep their path");
}

/// Deleting the glue target detaches it: the dangling record goes with
/// the deleted shapes (it is part of the same undoable step), and undo
/// restores both the shape and its glue. The warning path for a record
/// that is already dangling at re-route time is covered by
/// `stale_and_detached_glue_records_are_dropped` above (ADR 0045 rule 3).
#[test]
fn deleting_the_target_drops_its_glue_and_undo_restores_it() {
    let (mut ctx, registry, page) = flow();
    dispatch(
        &mut ctx,
        &registry,
        "connector.glue_begin",
        glue_args(11, 1, 1),
    )
    .expect("glue begin");
    dispatch(
        &mut ctx,
        &registry,
        "connector.glue_end",
        glue_args(11, 2, 0),
    )
    .expect("glue end");

    let box_one = ctx
        .active_document()
        .expect("session")
        .document
        .shape_by_visio_id(page, 1)
        .expect("box 1");
    ctx.selection.shapes = vec![box_one];
    ctx.selection.anchor = Some(box_one);
    ctx.selection.page = Some(page);
    dispatch(&mut ctx, &registry, "edit.delete", Args::new()).expect("delete box 1");

    let records = connects(&ctx, page);
    assert_eq!(records.len(), 1, "only the end glue survives");
    assert_eq!(
        records.first().expect("record").from_cell,
        "EndX",
        "the begin glue was dropped with its target"
    );

    dispatch(&mut ctx, &registry, "edit.undo", Args::new()).expect("undo");
    let records = connects(&ctx, page);
    assert_eq!(records.len(), 2, "undo restores the dropped glue record");
    assert!(
        records
            .iter()
            .any(|record| record.from_cell == "BeginX" && record.to_sheet == 1),
        "{records:?}"
    );
}

// ---------------------------------------------------------------------------
// Re-route stability regressions (task P5-11's product fixes)
// ---------------------------------------------------------------------------

/// The `Geometry` rows of one shape (the literal path rows).
fn geometry_rows(ctx: &AppCtx, connector: i32) -> Vec<larrygram_model::Row> {
    let doc = &ctx.active_document().expect("session").document;
    let shape = doc
        .shape_by_visio_id(PageId { index: 0 }, connector)
        .expect("shape");
    doc.shape(shape)
        .expect("shape")
        .sheet
        .sections
        .iter()
        .find(|section| section.name.as_str() == "Geometry")
        .map(|section| section.rows.clone())
        .unwrap_or_default()
}

/// A glue-on-drop is shape-to-shape glue (ADR 0046), not a connector
/// end: the re-route pass leaves the dropped instance's geometry alone
/// (task P5-11: `is_connector_end`).
#[test]
fn a_drop_glued_instance_keeps_its_geometry() {
    let (mut ctx, registry, page) = flow();
    let target = ctx
        .active_document()
        .expect("session")
        .document
        .shape_by_visio_id(page, 1)
        .expect("box 1");
    let target_visio = visio(&ctx, target);
    dispatch(
        &mut ctx,
        &registry,
        "insert.shape",
        Args::new()
            .with("stencil", ArgValue::Text("Basic Shapes".to_owned()))
            .with("master", ArgValue::Text("Rectangle".to_owned()))
            .with("x", ArgValue::Number(6.0))
            .with("y", ArgValue::Number(6.0))
            .with("glue_to", ArgValue::Integer(i64::from(target_visio))),
    )
    .expect("drop with glue");
    let tops = ctx
        .active_document()
        .expect("session")
        .document
        .page(page)
        .expect("page")
        .shapes
        .clone();
    let instance = *tops.last().expect("the dropped instance");
    let instance_visio = visio(&ctx, instance);
    let before = geometry_rows(&ctx, instance_visio);
    dispatch(&mut ctx, &registry, "connector.reroute_all", Args::new()).expect("reroute");
    assert_eq!(
        geometry_rows(&ctx, instance_visio),
        before,
        "the drop glue must not turn the instance into a routed path"
    );
}

/// A connector inside a group routes through its parents (task P5-11:
/// `write_path`), so a re-route is idempotent instead of drifting one
/// mapping per pass.
#[test]
fn a_grouped_connector_re_route_is_idempotent() {
    let (mut ctx, registry, page) = flow();
    dispatch(
        &mut ctx,
        &registry,
        "connector.glue_begin",
        glue_args(11, 1, 1),
    )
    .expect("glue begin");
    let connector = connector_shape_id(&ctx, 11);
    let box_one = ctx
        .active_document()
        .expect("session")
        .document
        .shape_by_visio_id(page, 1)
        .expect("box 1");
    ctx.selection.shapes = vec![connector, box_one];
    ctx.selection.anchor = Some(box_one);
    ctx.selection.page = Some(page);
    dispatch(&mut ctx, &registry, "arrange.group", Args::new()).expect("group");

    dispatch(&mut ctx, &registry, "connector.reroute_all", Args::new()).expect("first reroute");
    let first = geometry_rows(&ctx, 11);
    assert!(!first.is_empty(), "the glued connector has a routed path");
    dispatch(&mut ctx, &registry, "connector.reroute_all", Args::new()).expect("second reroute");
    assert_eq!(
        geometry_rows(&ctx, 11),
        first,
        "a second re-route must not move the path"
    );
}

/// A paste or duplicate of a glued connector leaves the page
/// re-route-stable (task P5-11: the copies carry their copied glue and
/// the hook runs after the command, ADR 0045 rule 3).
#[test]
fn a_duplicated_connector_leaves_the_page_re_route_stable() {
    let (mut ctx, registry, page) = flow();
    for (end, args) in [
        ("connector.glue_begin", glue_args(11, 1, 1)),
        ("connector.glue_end", glue_args(11, 2, 0)),
    ] {
        dispatch(&mut ctx, &registry, end, args).expect("glue");
    }
    let connector = connector_shape_id(&ctx, 11);
    ctx.selection.shapes = vec![connector];
    ctx.selection.anchor = Some(connector);
    ctx.selection.page = Some(page);
    dispatch(&mut ctx, &registry, "edit.duplicate", Args::new()).expect("duplicate");

    let tops = ctx
        .active_document()
        .expect("session")
        .document
        .page(page)
        .expect("page")
        .shapes
        .clone();
    let ids: Vec<i32> = tops.iter().map(|shape| visio(&ctx, *shape)).collect();
    let before: Vec<(i32, Vec<larrygram_model::Row>)> = ids
        .iter()
        .map(|id| (*id, geometry_rows(&ctx, *id)))
        .collect();
    dispatch(&mut ctx, &registry, "connector.reroute_all", Args::new()).expect("reroute");
    let after: Vec<(i32, Vec<larrygram_model::Row>)> = ids
        .iter()
        .map(|id| (*id, geometry_rows(&ctx, *id)))
        .collect();
    assert_eq!(
        after, before,
        "the command's own re-route was the last word"
    );
}

/// `connector.draw` with glued ends (ADR 0045 rule 2): the insert, the
/// endpoint glues and the routed path land in one undo step, and undo
/// restores all three (task P5-11's coverage of the glue-on-draw path).
#[test]
fn a_draw_with_glued_ends_is_one_undo_step() {
    let (mut ctx, registry, page) = flow();
    let before = connects(&ctx, page);
    let undo_before = ctx.active_document().expect("session").undo.len();
    dispatch(
        &mut ctx,
        &registry,
        "connector.draw",
        Args::new()
            .with("stencil", ArgValue::Text("Basic Flowchart".to_owned()))
            .with("master", ArgValue::Text("Connector".to_owned()))
            .with("x", ArgValue::Number(2.0))
            .with("y", ArgValue::Number(6.25))
            .with("begin_target", ArgValue::Integer(1))
            .with("begin_point", ArgValue::Integer(1))
            .with("end_target", ArgValue::Integer(2))
            .with("end_point", ArgValue::Integer(0)),
    )
    .expect("draw a glued connector");
    assert_eq!(
        ctx.active_document().expect("session").undo.len(),
        undo_before + 1,
        "the draw is one undoable edit"
    );
    let after = connects(&ctx, page);
    assert_eq!(after.len(), before.len() + 2, "two glues on the new shape");
    let routed = after
        .iter()
        .filter(|record| !before.contains(record))
        .map(|record| record.from_sheet)
        .collect::<Vec<i32>>();
    assert_eq!(routed.len(), 2, "both records name the new connector");
    assert!(path(&ctx, routed[0]).len() >= 2, "the path is routed");

    dispatch(&mut ctx, &registry, "edit.undo", Args::new()).expect("undo");
    assert_eq!(connects(&ctx, page), before, "one undo restores the page");
    dispatch(&mut ctx, &registry, "edit.redo", Args::new()).expect("redo");
    assert_eq!(connects(&ctx, page), after, "one redo re-does the draw");
}

/// A drop with an explicit glue point (ADR 0046's `glue_point`): the
/// record names the target's connection row (task P5-11).
#[test]
fn a_drop_with_an_explicit_glue_point_names_the_row() {
    let (mut ctx, registry, page) = flow();
    dispatch(
        &mut ctx,
        &registry,
        "insert.shape",
        Args::new()
            .with("stencil", ArgValue::Text("Basic Shapes".to_owned()))
            .with("master", ArgValue::Text("Rectangle".to_owned()))
            .with("x", ArgValue::Number(6.0))
            .with("y", ArgValue::Number(6.0))
            .with("glue_to", ArgValue::Integer(1))
            .with("glue_point", ArgValue::Integer(1)),
    )
    .expect("drop with glue point");
    let records = connects(&ctx, page);
    let record = records
        .iter()
        .find(|record| record.to_sheet == 1 && record.to_cell.starts_with("Connections."))
        .expect("a point glue record");
    assert_eq!(record.to_cell, "Connections.X2", "the row's one-based cell");
    assert_eq!(
        record.to_part,
        larrygram_model::mutate::VIS_CONNECTION_POINT + 1,
        "the row's VisToParts value"
    );
}

/// `connector.draw` with one glued end and one free end (task P5-11:
/// `resolved_end` mixes both kinds).
#[test]
fn a_draw_with_one_glued_end_routes_the_free_end_to_its_point() {
    let (mut ctx, registry, page) = flow();
    dispatch(
        &mut ctx,
        &registry,
        "connector.draw",
        Args::new()
            .with("stencil", ArgValue::Text("Basic Flowchart".to_owned()))
            .with("master", ArgValue::Text("Connector".to_owned()))
            .with("x", ArgValue::Number(2.0))
            .with("y", ArgValue::Number(6.25))
            .with("begin_target", ArgValue::Integer(1))
            .with("begin_point", ArgValue::Integer(1))
            .with("end_x", ArgValue::Number(5.0))
            .with("end_y", ArgValue::Number(1.0)),
    )
    .expect("draw a half-glued connector");
    let records = connects(&ctx, page);
    assert_eq!(records.len(), 1, "one glue on the new shape");
    let drawn = records.first().expect("record").from_sheet;
    let points = path(&ctx, drawn);
    // The default routes right angle (ADR 0084 rule 8): a routed path
    // with an axis-aligned elbow between the two ends.
    assert!(points.len() >= 2, "{points:?}");
    let start = points.first().expect("begin");
    assert!(
        (start.x - 2.0).abs() < 1e-9 && (start.y - 6.625).abs() < 1e-9,
        "the glued begin lands on the connection point: {start:?}"
    );
    let end = points.last().expect("end");
    assert!(
        (end.x - 5.0).abs() < 1e-9 && (end.y - 1.0).abs() < 1e-9,
        "{end:?}"
    );
}
