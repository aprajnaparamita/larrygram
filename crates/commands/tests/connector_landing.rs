//! Where connector ends land (task P7-32, ADR 0084 rules 3 to 5).
//!
//! The audit's reproduction: draw between two shapes, then an unrelated
//! edit, then a move of one shape — both ends sit on the rule's points
//! at every step, one undo restores the shape and the route, and a
//! third connector between untouched shapes keeps its geometry rows
//! byte for byte through it all and a save. A corpus connector Visio
//! routed keeps its rows through an unrelated edit and a save. A
//! connector inside a group keeps its endpoint cells in the parent's
//! coordinates, and a group and an ungroup re-base those cells in the
//! model (undo and redo included).

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

#[path = "crosscheck/mod.rs"]
mod helpers;

mod support;

use helpers::{corpus_files, read_corpus_file, skip_without_corpus};
use support::dispatch;

use larrygram_commands::app::{AppCtx, DocumentSession, Settings};
use larrygram_commands::builtins::registry;
use larrygram_commands::{ArgValue, Args, Registry};
use larrygram_model::{
    Cell, CellName, CellValue, ConnectorEnd, Document, GeometryRow, PageId, Preserved, Shape,
    ShapeId,
};
use larrygram_vsdx::{Limits, Package, read_document, write_document};

fn cell(name: &str, value: f64) -> Cell {
    Cell {
        name: CellName::new(name),
        formula: Some(format!("{value}")),
        value: Some(CellValue::Number(value)),
        unit: larrygram_geom::Unit::Inches,
        preserved: Preserved::new(),
    }
}

/// A 2 x 1 in box at `(x, y)` with no connection points: a dynamic end
/// lands on one of its four side midpoints (ADR 0084 rule 3).
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
    shape
}

/// Two pairs of side-by-side boxes (1 and 2, 3 and 4) and one lone box
/// (5) for the unrelated edit.
fn fixture() -> (AppCtx, Registry, PageId) {
    let mut doc = Document::new();
    let page = doc.add_page("Page-1").expect("page");
    for (visio_id, x, y) in [
        (1, 3.0, 3.0),
        (2, 7.0, 3.0),
        (3, 3.0, 7.0),
        (4, 7.0, 7.0),
        (5, 11.0, 3.0),
    ] {
        doc.insert_shape(page, box_shape(visio_id, x, y), None)
            .expect("insert");
    }
    let mut ctx = AppCtx::new(Settings::default());
    ctx.open_document(DocumentSession::new(doc, None));
    (ctx, registry().expect("registry"), page)
}

fn select(ctx: &mut AppCtx, page: PageId, visio_ids: &[i32]) {
    let doc = &ctx.active_document().expect("session").document;
    let shapes: Vec<ShapeId> = visio_ids
        .iter()
        .filter_map(|id| doc.shape_by_visio_id(page, *id))
        .collect();
    ctx.selection.anchor = shapes.last().copied();
    ctx.selection.shapes = shapes;
    ctx.selection.page = Some(page);
}

/// One `connector.draw` from `begin` to `end` (dynamic glue on both
/// ends: the drag runs from shape to shape).
fn draw(ctx: &mut AppCtx, registry: &Registry, begin: i32, end: i32) -> ShapeId {
    dispatch(
        ctx,
        registry,
        "connector.draw",
        Args::new()
            .with("stencil", ArgValue::Text("Basic Flowchart".to_owned()))
            .with("master", ArgValue::Text("Connector".to_owned()))
            .with("x", ArgValue::Number(4.0))
            .with("y", ArgValue::Number(3.0))
            .with("begin_target", ArgValue::Integer(i64::from(begin)))
            .with("end_target", ArgValue::Integer(i64::from(end))),
    )
    .expect("draw");
    ctx.selection
        .anchor
        .expect("the new connector is the anchor")
}

fn document(ctx: &AppCtx) -> Document {
    ctx.active_document().expect("session").document.clone()
}

/// The endpoint cells' cached points (page inches on a top-level
/// connector; the parent's coordinates inside a group).
fn ends(doc: &Document, connector: ShapeId) -> (larrygram_geom::Point, larrygram_geom::Point) {
    (
        doc.connector_end(connector, ConnectorEnd::Begin)
            .expect("begin"),
        doc.connector_end(connector, ConnectorEnd::End)
            .expect("end"),
    )
}

/// The connector's geometry rows (the literal path, byte for byte).
fn rows(doc: &Document, connector: ShapeId) -> Vec<GeometryRow> {
    let geometry = doc.shape_geometry(connector).expect("geometry");
    geometry.paths.first().expect("a path").rows.clone()
}

fn move_shape(
    ctx: &mut AppCtx,
    registry: &Registry,
    page: PageId,
    visio_id: i32,
    dx: f64,
    dy: f64,
) {
    select(ctx, page, &[visio_id]);
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

/// The audit's reproduction (task P7-30): draw between two shapes,
/// then an unrelated edit, then a move of one shape. Both ends sit on
/// the rule's points at every step, and one undo restores the shape
/// and the route.
#[test]
fn the_audit_reproduction_lands_on_the_rules_points_at_every_step() {
    let (mut ctx, registry, page) = fixture();
    let drawn = draw(&mut ctx, &registry, 1, 2);

    // Draw time: the facing side midpoints, and a straight horizontal
    // run between them.
    let doc = document(&ctx);
    assert_eq!(
        ends(&doc, drawn),
        (
            larrygram_geom::Point::new(4.0, 3.0),
            larrygram_geom::Point::new(6.0, 3.0),
        ),
        "the facing side midpoints"
    );
    let before = rows(&doc, drawn);
    assert_eq!(before.len(), 2, "a straight run: {before:?}");

    // The unrelated edit touches nothing the connector knows.
    move_shape(&mut ctx, &registry, page, 5, 0.5, 0.5);
    let doc = document(&ctx);
    assert_eq!(
        ends(&doc, drawn),
        (
            larrygram_geom::Point::new(4.0, 3.0),
            larrygram_geom::Point::new(6.0, 3.0)
        )
    );
    assert_eq!(
        rows(&doc, drawn),
        before,
        "the unrelated edit keeps the route"
    );

    // One target moves below the other: the ends walk to the facing
    // top and bottom midpoints (the nearest pair).
    move_shape(&mut ctx, &registry, page, 2, -4.0, 3.0);
    let doc = document(&ctx);
    assert_eq!(
        ends(&doc, drawn),
        (
            larrygram_geom::Point::new(3.0, 3.5),
            larrygram_geom::Point::new(3.0, 5.5),
        ),
        "the top and bottom midpoints"
    );
    let moved_rows = rows(&doc, drawn);
    assert_eq!(moved_rows.len(), 2, "a vertical run: {moved_rows:?}");

    // One undo restores the shape and the route together.
    dispatch(&mut ctx, &registry, "edit.undo", Args::new()).expect("undo");
    let doc = document(&ctx);
    assert_eq!(
        ends(&doc, drawn),
        (
            larrygram_geom::Point::new(4.0, 3.0),
            larrygram_geom::Point::new(6.0, 3.0),
        )
    );
    assert_eq!(rows(&doc, drawn), before, "one undo restores the route");
}

/// A third connector between untouched shapes keeps its geometry rows
/// byte for byte through the unrelated edit, the move and its undo,
/// and a save and reopen.
#[test]
fn an_unrelated_edit_keeps_a_third_connectors_rows_byte_for_byte() {
    let (mut ctx, registry, page) = fixture();
    let _drawn = draw(&mut ctx, &registry, 1, 2);
    let third = draw(&mut ctx, &registry, 3, 4);
    let before = rows(&document(&ctx), third);

    move_shape(&mut ctx, &registry, page, 5, 0.5, 0.5);
    assert_eq!(
        rows(&document(&ctx), third),
        before,
        "through the unrelated edit"
    );
    move_shape(&mut ctx, &registry, page, 2, -4.0, 3.0);
    assert_eq!(rows(&document(&ctx), third), before, "through the move");
    dispatch(&mut ctx, &registry, "edit.undo", Args::new()).expect("undo");
    assert_eq!(rows(&document(&ctx), third), before, "through the undo");

    let doc = document(&ctx);
    let bytes = write_document(&doc, &Package::new()).expect("save");
    let back = read_document(&bytes, &Limits::DEFAULT).expect("reopen");
    let reopened = back
        .shape_by_visio_id(page, doc.shape(third).expect("third").visio_id)
        .expect("the third connector");
    assert_eq!(rows(&back, reopened), before, "through a save and reopen");
}

/// Undo restores the rows byte for byte and redo puts the step's rows
/// back exactly (task P7-49 goal 1): a stored custom 4-point route
/// survives a move of one glued target and its undo and redo.
#[test]
fn undo_restores_a_stored_route_and_redo_the_steps() {
    let (mut ctx, registry, page) = fixture();
    let drawn = draw(&mut ctx, &registry, 1, 2);

    // A stored custom route: four points, not one the router would
    // draw again (as a moved segment leaves behind).
    let custom = vec![
        larrygram_geom::Point::new(4.0, 3.0),
        larrygram_geom::Point::new(4.5, 3.2),
        larrygram_geom::Point::new(5.5, 3.2),
        larrygram_geom::Point::new(6.0, 3.0),
    ];
    {
        let doc = &mut ctx.active_document_mut().expect("session").document;
        doc.write_connector_path(drawn, &custom)
            .expect("custom rows");
    }
    let stored = rows(&document(&ctx), drawn);
    assert_eq!(stored.len(), 4, "the custom route: {stored:?}");

    // The move re-routes the connector glued to the moved box.
    move_shape(&mut ctx, &registry, page, 2, -4.0, 3.0);
    let moved_rows = rows(&document(&ctx), drawn);
    assert_ne!(moved_rows, stored, "the step rewrote the rows");

    // Undo puts the stored route back byte for byte.
    dispatch(&mut ctx, &registry, "edit.undo", Args::new()).expect("undo");
    assert_eq!(
        rows(&document(&ctx), drawn),
        stored,
        "undo restores the stored route byte for byte"
    );

    // Redo puts the step's rows back exactly.
    dispatch(&mut ctx, &registry, "edit.redo", Args::new()).expect("redo");
    assert_eq!(rows(&document(&ctx), drawn), moved_rows, "redo is exact");
}

/// A glue on a corpus connector with a Visio route: undo puts the
/// Visio rows back byte for byte (task P7-49 goal 1). (The paired
/// delete half went with task P7-50 minor 4: `detach_shape` removes
/// the `Connects` records before any pass runs, so no pass can pick
/// the connector and "the delete keeps the rows" held with or without
/// every P7-49 change; it pinned nothing.)
#[test]
fn a_glue_restores_a_corpus_route_on_undo() {
    let files = corpus_files();
    if skip_without_corpus(&files) {
        return;
    }
    let path = files
        .iter()
        .find(|path| {
            path.file_name()
                .is_some_and(|name| name == "testfile3.vsdx")
        })
        .cloned()
        .expect("corpus/vsdx/testfile3.vsdx (corpus/PROVENANCE.md)");
    let document = read_corpus_file(&path);
    let mut found = None;
    for entity in &document.pages {
        if let Some(found_at) = glued_connector(&document, entity.id) {
            found = Some((entity.id, found_at));
            break;
        }
    }
    let (_page, (connector, targets)) = found.expect("a glued connector in the corpus file");
    let stored = rows(&document, connector);
    let visio = document.shape(connector).expect("connector").visio_id;

    let mut ctx = AppCtx::new(Settings::default());
    ctx.open_document(DocumentSession::new(document, None));
    let registry = registry().expect("registry");
    let end_visio = targets[0];

    // A glue: re-glue the begin end to the same target again (its own
    // glue changed, so the pass re-routes it).
    dispatch(
        &mut ctx,
        &registry,
        "connector.glue_begin",
        Args::new()
            .with("connector", ArgValue::Integer(i64::from(visio)))
            .with("target", ArgValue::Integer(i64::from(end_visio))),
    )
    .expect("glue");
    assert_ne!(
        rows(&ctx.active_document().expect("s").document, connector),
        stored
    );
    dispatch(&mut ctx, &registry, "edit.undo", Args::new()).expect("undo");
    assert_eq!(
        rows(&ctx.active_document().expect("s").document, connector),
        stored,
        "the glue's undo restores the Visio rows byte for byte"
    );
}

/// The two parent kinds the group tests wrap a connector in (task
/// P7-49 goal 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GroupKind {
    /// A parent that rotates 90 degrees about (10, 0).
    Rotate,
    /// A parent that flips X about x = 10.
    Flip,
}

/// The page point of a parent-space point for one test group.
fn to_page(kind: GroupKind, parent: larrygram_geom::Point) -> larrygram_geom::Point {
    match kind {
        GroupKind::Rotate => larrygram_geom::Point::new(10.0 - parent.y, parent.x),
        GroupKind::Flip => larrygram_geom::Point::new(10.0 - parent.x, parent.y),
    }
}

/// Wraps `drawn` in a fresh group with the kind's transform.
fn wrap_in_group(ctx: &mut AppCtx, page: PageId, drawn: ShapeId, kind: GroupKind) -> ShapeId {
    let doc = &mut ctx.active_document_mut().expect("session").document;
    let mut shape = Shape::new(ShapeId { index: 0 }, 99);
    let angle = match kind {
        GroupKind::Rotate => std::f64::consts::FRAC_PI_2,
        GroupKind::Flip => 0.0,
    };
    shape.sheet.cells = vec![
        cell("Width", 1.0),
        cell("Height", 1.0),
        cell("PinX", 10.0),
        cell("PinY", 0.0),
        cell("LocPinX", 0.0),
        cell("LocPinY", 0.0),
        cell("Angle", angle),
        Cell {
            name: CellName::new("FlipX"),
            formula: Some(if matches!(kind, GroupKind::Flip) {
                "1".to_owned()
            } else {
                "0".to_owned()
            }),
            value: Some(CellValue::Bool(matches!(kind, GroupKind::Flip))),
            unit: larrygram_geom::Unit::Unitsless,
            preserved: Preserved::new(),
        },
    ];
    let group = doc.add_shape(shape);
    if let Some(entity) = doc.pages.get_mut(page.index as usize) {
        entity.shapes.push(group);
    }
    if let Some(entity) = doc.shape_mut(group) {
        entity.children.push(drawn);
    }
    if let Some(entity) = doc.shape_mut(drawn) {
        entity.parent = Some(group);
    }
    group
}

/// Asserts the two invariants of task P7-49 goal 5: the endpoint cells
/// are in the parent's coordinates and the drawn ends sit on the page
/// points in `expected`. `at` maps a parent-space point to the page
/// through the group's current transform.
fn assert_group_invariants(
    ctx: &AppCtx,
    drawn: ShapeId,
    kind: GroupKind,
    expected: [larrygram_geom::Point; 2],
) {
    let doc = document(ctx);
    let parent = doc
        .shape(drawn)
        .expect("connector")
        .parent
        .expect("a parent");
    let shift = larrygram_geom::Point::new(
        number(&doc, parent, "PinX") - 10.0,
        number(&doc, parent, "PinY"),
    );
    let at = |parent_point: larrygram_geom::Point| to_page(kind, parent_point).add(shift);
    let (begin, end) = ends(&doc, drawn);
    assert!(
        close(at(begin), expected[0]),
        "the begin cells are in the parent's coordinates: {:?}",
        at(begin)
    );
    assert!(
        close(at(end), expected[1]),
        "the end cells are in the parent's coordinates: {:?}",
        at(end)
    );
    let path = rows(&doc, drawn);
    let first = row_point(path.first().expect("a first row"));
    let last = row_point(path.last().expect("a last row"));
    assert!(
        close(local_to_parent(&doc, drawn, first), begin),
        "the path and the cells agree: {first:?} vs {begin:?}"
    );
    assert!(
        close(local_to_parent(&doc, drawn, last), end),
        "the path and the cells agree: {last:?} vs {end:?}"
    );
    assert!(
        close(at(local_to_parent(&doc, drawn, first)), expected[0]),
        "the drawn begin sits on its point"
    );
    assert!(
        close(at(local_to_parent(&doc, drawn, last)), expected[1]),
        "the drawn end sits on its point"
    );
}

/// The number of the shape's own cell.
fn number(document: &Document, shape: ShapeId, name: &str) -> f64 {
    document
        .shape(shape)
        .and_then(|entity| {
            entity
                .sheet
                .cells
                .iter()
                .find(|cell| cell.name.as_str() == name)
        })
        .and_then(|cell| cell.value.clone())
        .and_then(|value| match value {
            CellValue::Number(number) => Some(number),
            _ => None,
        })
        .unwrap_or(f64::NAN)
}

/// A parent that rotates or flips (task P7-49 goal 5): glue, unglue,
/// move the group, move the connector and their undo and redo keep the
/// endpoint cells in the parent's coordinates and the drawn ends on the
/// targets' points.
fn a_rotating_or_flipping_group_keeps_the_ends_on_the_targets(kind: GroupKind) {
    let (mut ctx, registry, page) = fixture();
    let drawn = draw(&mut ctx, &registry, 1, 2);
    wrap_in_group(&mut ctx, page, drawn, kind);

    // The targets' facing side midpoints: (4, 3) and (6, 3).
    let left_point = larrygram_geom::Point::new(4.0, 3.0);
    let right_point = larrygram_geom::Point::new(6.0, 3.0);

    // Re-glue the end inside the rotated (or flipped) parent.
    let drawn_visio = document(&ctx).shape(drawn).expect("connector").visio_id;
    dispatch(
        &mut ctx,
        &registry,
        "connector.glue_end",
        Args::new()
            .with("connector", ArgValue::Integer(i64::from(drawn_visio)))
            .with("target", ArgValue::Integer(2)),
    )
    .expect("glue end");
    assert_group_invariants(&ctx, drawn, kind, [left_point, right_point]);

    // Unglue the begin: it keeps its point where it was.
    dispatch(
        &mut ctx,
        &registry,
        "connector.unglue",
        Args::new()
            .with("connector", ArgValue::Integer(i64::from(drawn_visio)))
            .with("end", ArgValue::Text("begin".to_owned())),
    )
    .expect("unglue");
    assert_group_invariants(&ctx, drawn, kind, [left_point, right_point]);

    // Move the group: the glued end stays on the target, the free end
    // travels with the group.
    move_shape(&mut ctx, &registry, page, 99, 0.7, -0.3);
    let free = larrygram_geom::Point::new(left_point.x + 0.7, left_point.y - 0.3);
    assert_group_invariants(&ctx, drawn, kind, [free, right_point]);
    let moved_rows = rows(&document(&ctx), drawn);

    dispatch(&mut ctx, &registry, "edit.undo", Args::new()).expect("undo");
    assert_group_invariants(&ctx, drawn, kind, [left_point, right_point]);
    dispatch(&mut ctx, &registry, "edit.redo", Args::new()).expect("redo");
    assert_eq!(rows(&document(&ctx), drawn), moved_rows, "redo is exact");

    // Move the connector itself (OneDMoveEdit): both ends travel with
    // it and come loose from the target.
    move_shape(&mut ctx, &registry, page, drawn_visio, -0.2, 0.4);
    let free2 = larrygram_geom::Point::new(free.x - 0.2, free.y + 0.4);
    let end_free = larrygram_geom::Point::new(right_point.x - 0.2, right_point.y + 0.4);
    assert_group_invariants(&ctx, drawn, kind, [free2, end_free]);
    let moved_rows = rows(&document(&ctx), drawn);

    dispatch(&mut ctx, &registry, "edit.undo", Args::new()).expect("undo");
    assert_group_invariants(&ctx, drawn, kind, [free, right_point]);
    dispatch(&mut ctx, &registry, "edit.redo", Args::new()).expect("redo");
    assert_eq!(rows(&document(&ctx), drawn), moved_rows, "redo is exact");
}

#[test]
fn a_rotating_group_keeps_the_ends_on_the_targets() {
    a_rotating_or_flipping_group_keeps_the_ends_on_the_targets(GroupKind::Rotate);
}

#[test]
fn a_flipping_group_keeps_the_ends_on_the_targets() {
    a_rotating_or_flipping_group_keeps_the_ends_on_the_targets(GroupKind::Flip);
}

/// One connector with a glue record to a shape on `page`, as the file
/// carries it (the `FromSheet` of a connector-end record).
fn glued_connector(doc: &Document, page: PageId) -> Option<(ShapeId, Vec<i32>)> {
    let entity = doc.page(page)?;
    for connect in &entity.connects {
        let from_begin =
            connect.from_cell.starts_with("Begin") || connect.from_cell.starts_with("End");
        if !from_begin {
            continue;
        }
        let shape = doc.shape_by_visio_id(page, connect.from_sheet)?;
        let targets: Vec<i32> = entity
            .connects
            .iter()
            .filter(|record| record.from_sheet == connect.from_sheet)
            .map(|record| record.to_sheet)
            .collect();
        return Some((shape, targets));
    }
    None
}

/// A corpus connector Visio routed keeps its geometry rows byte for
/// byte through an unrelated edit and a save (rule 5: the scoped pass
/// never picks it).
#[test]
fn a_corpus_connector_visio_routed_keeps_its_rows_through_an_unrelated_edit_and_a_save() {
    let files = corpus_files();
    if skip_without_corpus(&files) {
        return;
    }
    let path = files
        .iter()
        .find(|path| {
            path.file_name()
                .is_some_and(|name| name == "testfile3.vsdx")
        })
        .cloned()
        .expect("corpus/vsdx/testfile3.vsdx (corpus/PROVENANCE.md)");
    let document = read_corpus_file(&path);
    let mut found = None;
    for entity in &document.pages {
        if let Some(found_at) = glued_connector(&document, entity.id) {
            found = Some((entity.id, found_at));
            break;
        }
    }
    let (page, (connector, targets)) = found.expect("a glued connector in the corpus file");
    let before = rows(&document, connector);
    let visio = document.shape(connector).expect("connector").visio_id;

    // An unrelated edit: move a shape the connector does not know.
    let unrelated = document
        .page(page)
        .expect("page")
        .shapes
        .iter()
        .copied()
        .find(|shape| {
            let id = document.shape(*shape).map(|entry| entry.visio_id);
            id.is_some_and(|id| id != visio && !targets.contains(&id))
        })
        .expect("an unrelated shape");
    let unrelated_visio = document.shape(unrelated).expect("unrelated").visio_id;
    let mut ctx = AppCtx::new(Settings::default());
    ctx.open_document(DocumentSession::new(document, None));
    select(&mut ctx, page, &[unrelated_visio]);
    dispatch(
        &mut ctx,
        &registry().expect("registry"),
        "shape.move",
        Args::new()
            .with("dx", ArgValue::Number(0.1))
            .with("dy", ArgValue::Number(0.1)),
    )
    .expect("the unrelated move");
    let moved = &ctx.active_document().expect("session").document;
    assert_eq!(
        rows(moved, connector),
        before,
        "the scoped pass never picks the Visio-routed connector"
    );

    // And a save and reopen.
    let bytes = write_document(moved, &Package::new()).expect("save");
    let back = read_document(&bytes, &Limits::DEFAULT).expect("reopen");
    let reopened = back.shape_by_visio_id(page, visio).expect("the connector");
    assert_eq!(rows(&back, reopened), before, "through a save");
}

/// A connector inside a group keeps its endpoint cells in the parent's
/// coordinates (the group translates by (10, 0)): the glue write and
/// the path write agree, and no later path write corrects the other.
#[test]
fn a_connector_inside_a_group_keeps_its_cells_in_the_parents_coordinates() {
    let (mut ctx, registry, page) = fixture();
    let drawn = draw(&mut ctx, &registry, 1, 2);

    // Wrap the connector in a group that translates by (10, 0).
    let group = {
        let doc = &mut ctx.active_document_mut().expect("session").document;
        let mut shape = Shape::new(ShapeId { index: 0 }, 99);
        shape.sheet.cells = vec![
            cell("Width", 1.0),
            cell("Height", 1.0),
            cell("PinX", 10.0),
            cell("PinY", 0.0),
            cell("LocPinX", 0.0),
            cell("LocPinY", 0.0),
            cell("Angle", 0.0),
        ];
        let group = doc.add_shape(shape);
        if let Some(entity) = doc.pages.get_mut(page.index as usize) {
            entity.shapes.push(group);
        }
        if let Some(entity) = doc.shape_mut(group) {
            entity.children.push(drawn);
        }
        if let Some(entity) = doc.shape_mut(drawn) {
            entity.parent = Some(group);
        }
        group
    };

    // Re-glue the end: the resolution lands on (6, 3) in page inches
    // and the cells take (-4, 3), the parent's coordinates.
    let drawn_visio = document(&ctx).shape(drawn).expect("connector").visio_id;
    dispatch(
        &mut ctx,
        &registry,
        "connector.glue_end",
        Args::new()
            .with("connector", ArgValue::Integer(i64::from(drawn_visio)))
            .with("target", ArgValue::Integer(2)),
    )
    .expect("glue end");
    let doc = document(&ctx);
    let (_begin, end) = ends(&doc, drawn);
    assert_eq!(
        end,
        larrygram_geom::Point::new(-4.0, 3.0),
        "the cached value is the parent's coordinates"
    );
    assert_eq!(
        formula(&doc, drawn, "EndX"),
        Some("_WALKGLUE(EndTrigger,BegTrigger,WalkPreference)".to_owned()),
        "the glue formula is unchanged"
    );

    // The path write agrees with the cells: the rows map through the
    // connector's own box to the same parent-space points.
    dispatch(&mut ctx, &registry, "connector.reroute_all", Args::new()).expect("reroute");
    let doc = document(&ctx);
    let (begin, end) = ends(&doc, drawn);
    assert_eq!(begin, larrygram_geom::Point::new(-6.0, 3.0));
    assert_eq!(end, larrygram_geom::Point::new(-4.0, 3.0));
    let path = rows(&doc, drawn);
    let first = row_point(path.first().expect("a first row"));
    let last = row_point(path.last().expect("a last row"));
    assert!(
        close(local_to_parent(&doc, drawn, first), begin),
        "the path's first row {first:?} agrees with the cells {begin:?}"
    );
    assert!(
        close(local_to_parent(&doc, drawn, last), end),
        "the path's last row {last:?} agrees with the cells {end:?}"
    );
    let _ = group;
}

/// No edit may leave the endpoint cells disagreeing with the drawn
/// path (ADR 0084 rule 1, amended; task P7-50 goal 1): the cells and
/// the path's first and last row land on the same parent-space points.
fn assert_cells_match_path(document: &Document, connector: ShapeId) {
    let path = rows(document, connector);
    let (begin, end) = ends(document, connector);
    let first = row_point(path.first().expect("a first row"));
    let last = row_point(path.last().expect("a last row"));
    assert!(
        close(local_to_parent(document, connector, first), begin),
        "the begin cells {begin:?} vs the drawn first row {first:?}"
    );
    assert!(
        close(local_to_parent(document, connector, last), end),
        "the end cells {end:?} vs the drawn last row {last:?}"
    );
}

/// One free-ended `connector.draw` from `(1.0, 8.0)` to `(5.0, 8.0)`.
fn draw_free(ctx: &mut AppCtx, registry: &Registry) -> ShapeId {
    dispatch(
        ctx,
        registry,
        "connector.draw",
        Args::new()
            .with("stencil", ArgValue::Text("Basic Flowchart".to_owned()))
            .with("master", ArgValue::Text("Connector".to_owned()))
            .with("x", ArgValue::Number(3.0))
            .with("y", ArgValue::Number(8.0))
            .with("begin_x", ArgValue::Number(1.0))
            .with("begin_y", ArgValue::Number(8.0))
            .with("end_x", ArgValue::Number(5.0))
            .with("end_y", ArgValue::Number(8.0)),
    )
    .expect("draw free");
    ctx.selection
        .anchor
        .expect("the new connector is the anchor")
}

/// Task P7-50 goal 1: a group and an ungroup re-base the endpoint
/// cells in the model, so the cells keep agreeing with the drawn path
/// through the group, the ungroup and their undo and redo — for a
/// glued connector and a free-ended one, with no re-route rewriting
/// their stored rows at any step.
#[test]
fn group_and_ungroup_rebase_the_endpoint_cells_on_undo_and_redo() {
    let (mut ctx, registry, page) = fixture();
    let glued = draw(&mut ctx, &registry, 1, 2);
    let free = draw_free(&mut ctx, &registry);
    let glued_rows = rows(&document(&ctx), glued);
    let free_rows = rows(&document(&ctx), free);
    let glued_id = document(&ctx).shape(glued).expect("glued").visio_id;
    let free_id = document(&ctx).shape(free).expect("free").visio_id;

    let assert_both = |ctx: &AppCtx, step: &str| {
        let doc = document(ctx);
        assert_cells_match_path(&doc, glued);
        assert_cells_match_path(&doc, free);
        assert_eq!(rows(&doc, glued), glued_rows, "the glued rows at {step}");
        assert_eq!(rows(&doc, free), free_rows, "the free rows at {step}");
    };

    // Group both connectors with their shapes: nothing moves on the
    // page, so no re-route may rewrite the stored rows.
    select(&mut ctx, page, &[1, 2, 5, glued_id, free_id]);
    dispatch(&mut ctx, &registry, "arrange.group", Args::new()).expect("group");
    let group = ctx.selection.anchor.expect("the group is the anchor");
    assert_both(&ctx, "the group");

    // Ungroup: the cells map back through the group's frame.
    let group_id = document(&ctx).shape(group).expect("group").visio_id;
    select(&mut ctx, page, &[group_id]);
    dispatch(&mut ctx, &registry, "arrange.ungroup", Args::new()).expect("ungroup");
    assert_both(&ctx, "the ungroup");

    // Undo and redo of both steps agree with the model's re-base.
    dispatch(&mut ctx, &registry, "edit.undo", Args::new()).expect("undo");
    assert_both(&ctx, "the ungroup undone");
    dispatch(&mut ctx, &registry, "edit.redo", Args::new()).expect("redo");
    assert_both(&ctx, "the ungroup redone");
    dispatch(&mut ctx, &registry, "edit.undo", Args::new()).expect("undo");
    dispatch(&mut ctx, &registry, "edit.undo", Args::new()).expect("undo");
    assert_both(&ctx, "the group undone");
    dispatch(&mut ctx, &registry, "edit.redo", Args::new()).expect("redo");
    assert_both(&ctx, "the group redone");
}

/// The typed point of one geometry row.
fn row_point(row: &GeometryRow) -> larrygram_geom::Point {
    match row {
        GeometryRow::MoveTo { x, y } | GeometryRow::LineTo { x, y } => {
            larrygram_geom::Point::new(*x, *y)
        }
        other => panic!("unexpected row {other:?}"),
    }
}

/// One row's point in the connector's parent's space (the inverse of
/// `write_connector_path`'s own box mapping).
fn local_to_parent(
    doc: &Document,
    connector: ShapeId,
    local: larrygram_geom::Point,
) -> larrygram_geom::Point {
    let number = |name: &str| -> f64 {
        doc.shape(connector)
            .and_then(|entity| {
                entity
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
    let (pin_x, pin_y) = (number("PinX"), number("PinY"));
    let (loc_x, loc_y) = (number("LocPinX"), number("LocPinY"));
    let angle = number("Angle");
    let (dx, dy) = (local.x - loc_x, local.y - loc_y);
    let (sin, cos) = angle.sin_cos();
    larrygram_geom::Point::new(pin_x + dx * cos - dy * sin, pin_y + dx * sin + dy * cos)
}

/// Whether two page points agree.
fn close(a: larrygram_geom::Point, b: larrygram_geom::Point) -> bool {
    (a.x - b.x).abs() < 1e-6 && (a.y - b.y).abs() < 1e-6
}

/// The formula of the shape's own cell.
fn formula(doc: &Document, shape: ShapeId, name: &str) -> Option<String> {
    doc.shape(shape)?
        .sheet
        .cells
        .iter()
        .find(|cell| cell.name.as_str() == name)
        .and_then(|cell| cell.formula.clone())
}
