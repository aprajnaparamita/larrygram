//! Headless arrangement command tests (task P5-09, ADR 0047).
//!
//! Cover: the six align edges/centres and the delta math, horizontal
//! and vertical distribution with two and three shapes, one undo step
//! for every multi-shape command, group/ungroup position round-trip and
//! z-order.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use support::dispatch;

use larrygram_commands::app::{AppCtx, DocumentSession, Settings};
use larrygram_commands::builtins::registry;
use larrygram_commands::{Args, CommandError, CommandId, Invocation, Registry, SessionId, Source};
use larrygram_geom::Unit;
use larrygram_model::{Cell, CellName, CellValue, Document, PageId, Preserved, Shape, ShapeId};

fn number_cell(name: &str, value: f64) -> Cell {
    Cell {
        name: CellName::new(name),
        formula: Some(format!("{value}")),
        value: Some(CellValue::Number(value)),
        unit: Unit::Inches,
        preserved: Preserved::new(),
    }
}

fn shape(width: f64, height: f64, x: f64, y: f64) -> Shape {
    let mut shape = Shape::new(ShapeId { index: 0 }, 0);
    for (name, value) in [
        ("PinX", x),
        ("PinY", y),
        ("Width", width),
        ("Height", height),
        ("LocPinX", width * 0.5),
        ("LocPinY", height * 0.5),
        ("Angle", 0.0),
    ] {
        shape.sheet.cells.push(number_cell(name, value));
    }
    shape
}

fn fixture(shapes: &[(f64, f64, f64, f64)]) -> (AppCtx, Registry, SessionId, PageId, Vec<ShapeId>) {
    let mut doc = Document::new();
    let page = doc.add_page("Page-1").expect("page");
    let mut ids = Vec::new();
    for (width, height, x, y) in shapes {
        ids.push(
            doc.insert_shape(page, shape(*width, *height, *x, *y), None)
                .expect("shape"),
        );
    }
    let mut ctx = AppCtx::new(Settings::default());
    let index = ctx.open_document(DocumentSession::new(doc, None));
    let session = ctx.documents.get(index).expect("session").id;
    (ctx, registry().expect("registry"), session, page, ids)
}

fn select(ctx: &mut AppCtx, page: PageId, shapes: &[ShapeId]) {
    ctx.selection.shapes = shapes.to_vec();
    ctx.selection.anchor = shapes.last().copied();
    ctx.selection.page = Some(page);
}

fn document(ctx: &AppCtx) -> &Document {
    &ctx.active_document().expect("document").document
}

fn cell(doc: &Document, shape: ShapeId, name: &str) -> f64 {
    doc.shape(shape)
        .and_then(|entry| {
            entry
                .sheet
                .cells
                .iter()
                .find(|cell| cell.name.as_str() == name)
        })
        .and_then(|cell| match cell.value {
            Some(CellValue::Number(value)) => Some(value),
            _ => None,
        })
        .unwrap_or_else(|| panic!("{name} of shape {}", shape.index))
}

fn pins(doc: &Document, shapes: &[ShapeId]) -> Vec<(f64, f64)> {
    shapes
        .iter()
        .map(|id| (cell(doc, *id, "PinX"), cell(doc, *id, "PinY")))
        .collect()
}

fn rects(doc: &Document, shapes: &[ShapeId]) -> Vec<(f64, f64, f64, f64)> {
    shapes
        .iter()
        .map(|id| {
            let left = cell(doc, *id, "PinX") - cell(doc, *id, "LocPinX");
            let bottom = cell(doc, *id, "PinY") - cell(doc, *id, "LocPinY");
            (
                left,
                bottom,
                left + cell(doc, *id, "Width"),
                bottom + cell(doc, *id, "Height"),
            )
        })
        .collect()
}

#[test]
fn every_align_command_moves_the_selection_to_the_shared_edge() {
    // Frames: A [-1,1]x[-0.5,0.5], B [2,4]x[1.5,2.5], C [5,7]x[0.5,1.5].
    let checks: &[(&str, &str, f64)] = &[
        ("arrange.align.left", "min_x", -1.0),
        ("arrange.align.center_x", "center_x", 3.0),
        ("arrange.align.right", "max_x", 7.0),
        ("arrange.align.top", "max_y", 2.5),
        ("arrange.align.middle", "center_y", 1.0),
        ("arrange.align.bottom", "min_y", -0.5),
    ];
    for (id, edge, value) in checks {
        let (mut ctx, registry, _session, page, shapes) = fixture(&[
            (2.0, 1.0, 0.0, 0.0),
            (2.0, 1.0, 3.0, 2.0),
            (2.0, 1.0, 6.0, 1.0),
        ]);
        select(&mut ctx, page, &shapes);
        let before = pins(document(&ctx), &shapes);
        dispatch(&mut ctx, &registry, id, Args::new()).expect(id);
        for rect in rects(document(&ctx), &shapes) {
            let actual = match *edge {
                "min_x" => rect.0,
                "center_x" => (rect.0 + rect.2) * 0.5,
                "max_x" => rect.2,
                "max_y" => rect.3,
                "center_y" => (rect.1 + rect.3) * 0.5,
                "min_y" => rect.1,
                other => panic!("unknown edge {other}"),
            };
            assert!((actual - value).abs() < 1e-9, "{id}: {actual} != {value}");
        }
        dispatch(&mut ctx, &registry, "edit.undo", Args::new()).expect("undo");
        assert_eq!(
            pins(document(&ctx), &shapes),
            before,
            "{id}: one undo restores"
        );
    }
}

#[test]
fn align_is_one_undo_step_for_every_shape() {
    let (mut ctx, registry, _session, page, shapes) = fixture(&[
        (2.0, 1.0, 0.0, 0.0),
        (2.0, 1.0, 3.0, 2.0),
        (2.0, 1.0, 6.0, 1.0),
    ]);
    select(&mut ctx, page, &shapes);
    let before = document(&ctx).clone();
    dispatch(&mut ctx, &registry, "arrange.align.left", Args::new()).expect("align");
    assert!(pins(document(&ctx), &shapes).iter().all(|(x, _)| *x == 0.0));
    let stack = &ctx.active_document().expect("session").undo;
    assert_eq!(stack.undo_labels().len(), 1, "one align = one undo step");
    dispatch(&mut ctx, &registry, "edit.undo", Args::new()).expect("undo");
    assert_eq!(document(&ctx), &before, "the whole selection came back");
}

#[test]
fn distribute_horizontal_spaces_three_shapes_and_leaves_two_alone() {
    let (mut ctx, registry, _session, page, shapes) = fixture(&[
        (1.0, 1.0, 0.0, 0.0),
        (1.0, 1.0, 2.0, 0.0),
        (1.0, 1.0, 10.0, 0.0),
    ]);
    select(&mut ctx, page, &shapes);
    dispatch(
        &mut ctx,
        &registry,
        "arrange.distribute.horizontal",
        Args::new(),
    )
    .expect("distribute");
    let after = pins(document(&ctx), &shapes);
    assert!((after[0].0 - 0.0).abs() < 1e-9);
    assert!((after[1].0 - 5.0).abs() < 1e-9, "{after:?}");
    assert!((after[2].0 - 10.0).abs() < 1e-9);
    dispatch(&mut ctx, &registry, "edit.undo", Args::new()).expect("undo");
    assert!((pins(document(&ctx), &shapes)[1].0 - 2.0).abs() < 1e-9);

    let (mut ctx, registry, _session, page, shapes) =
        fixture(&[(1.0, 1.0, 0.0, 0.0), (1.0, 1.0, 10.0, 0.0)]);
    select(&mut ctx, page, &shapes);
    dispatch(
        &mut ctx,
        &registry,
        "arrange.distribute.horizontal",
        Args::new(),
    )
    .expect("distribute two");
    let after = pins(document(&ctx), &shapes);
    assert!((after[0].0 - 0.0).abs() < 1e-9);
    assert!((after[1].0 - 10.0).abs() < 1e-9, "{after:?}");
}

#[test]
fn distribute_vertical_spaces_three_shapes() {
    let (mut ctx, registry, _session, page, shapes) = fixture(&[
        (1.0, 1.0, 0.0, 0.0),
        (1.0, 1.0, 0.0, 2.0),
        (1.0, 1.0, 0.0, 10.0),
    ]);
    select(&mut ctx, page, &shapes);
    dispatch(
        &mut ctx,
        &registry,
        "arrange.distribute.vertical",
        Args::new(),
    )
    .expect("distribute");
    let after = pins(document(&ctx), &shapes);
    assert!((after[0].1 - 0.0).abs() < 1e-9);
    assert!((after[1].1 - 5.0).abs() < 1e-9, "{after:?}");
    assert!((after[2].1 - 10.0).abs() < 1e-9);
    dispatch(&mut ctx, &registry, "edit.undo", Args::new()).expect("undo");
    assert!((pins(document(&ctx), &shapes)[1].1 - 2.0).abs() < 1e-9);
}

#[test]
fn group_and_ungroup_round_trip_positions_and_undo() {
    let (mut ctx, registry, _session, page, shapes) = fixture(&[
        (2.0, 1.0, 0.0, 0.0),
        (2.0, 1.0, 3.0, 2.0),
        (2.0, 1.0, 6.0, 1.0),
    ]);
    select(&mut ctx, page, &shapes);
    let before = pins(document(&ctx), &shapes);
    dispatch(&mut ctx, &registry, "arrange.group", Args::new()).expect("group");
    let doc = document(&ctx);
    assert_eq!(doc.pages[0].shapes.len(), 1, "the group is one page shape");
    let group = doc.pages[0].shapes[0];
    assert_eq!(doc.shape(group).expect("group").children.len(), 3);
    assert_eq!(
        ctx.selection.shapes,
        vec![group],
        "the new group is selected"
    );

    dispatch(&mut ctx, &registry, "arrange.ungroup", Args::new()).expect("ungroup");
    let doc = document(&ctx);
    assert_eq!(doc.pages[0].shapes.len(), 3);
    assert_eq!(pins(doc, &shapes), before, "page positions did not move");
    assert_eq!(ctx.selection.shapes, shapes, "the members are selected");

    // Undo the ungroup: a group again; undo the group: the members.
    dispatch(&mut ctx, &registry, "edit.undo", Args::new()).expect("undo ungroup");
    assert_eq!(document(&ctx).pages[0].shapes.len(), 1);
    dispatch(&mut ctx, &registry, "edit.undo", Args::new()).expect("undo group");
    assert_eq!(document(&ctx).pages[0].shapes.len(), 3);
    assert_eq!(pins(document(&ctx), &shapes), before);

    // Redo both: group and dissolve again.
    dispatch(&mut ctx, &registry, "edit.redo", Args::new()).expect("redo group");
    dispatch(&mut ctx, &registry, "edit.redo", Args::new()).expect("redo ungroup");
    assert_eq!(document(&ctx).pages[0].shapes.len(), 3);
    assert_eq!(pins(document(&ctx), &shapes), before);
}

/// The ADR 0040 selection rule (task P5-19 review F9, pinned): an undo
/// or redo of a group/ungroup selects the shapes it affected (the new
/// group on a group, its members on an ungroup), and a page restore
/// clears a stale selection when the active page changed.
#[test]
fn undo_steps_select_their_shapes_and_a_page_restore_clears_the_selection() {
    let (mut ctx, registry, _session, page, shapes) =
        fixture(&[(2.0, 1.0, 0.0, 0.0), (2.0, 1.0, 3.0, 2.0)]);
    select(&mut ctx, page, &shapes);
    dispatch(&mut ctx, &registry, "arrange.group", Args::new()).expect("group");
    let group = document(&ctx).pages[0].shapes[0];
    assert_eq!(ctx.selection.shapes, vec![group], "the group is selected");

    // Undo the group: the members are the affected shapes.
    dispatch(&mut ctx, &registry, "edit.undo", Args::new()).expect("undo group");
    assert_eq!(ctx.selection.shapes, shapes, "undo selects the members");

    // Redo the group, then undo the ungroup path: the group is back.
    dispatch(&mut ctx, &registry, "edit.redo", Args::new()).expect("redo group");
    assert_eq!(ctx.selection.shapes, vec![group]);
    dispatch(&mut ctx, &registry, "arrange.ungroup", Args::new()).expect("ungroup");
    assert_eq!(ctx.selection.shapes, shapes);
    dispatch(&mut ctx, &registry, "edit.undo", Args::new()).expect("undo ungroup");
    assert_eq!(
        ctx.selection.shapes,
        vec![group],
        "undo of an ungroup selects the group"
    );

    // A page restore that lands on another active page clears the
    // selection: the ids would be stale on the restored page.
    dispatch(&mut ctx, &registry, "page.add", Args::new()).expect("add");
    select(&mut ctx, page, &[group]);
    dispatch(&mut ctx, &registry, "edit.undo", Args::new()).expect("undo page add");
    assert!(
        ctx.selection.is_empty(),
        "the page restore cleared the stale selection: {:?}",
        ctx.selection.shapes
    );
}

#[test]
fn z_order_commands_reorder_and_undo() {
    let (mut ctx, registry, _session, page, shapes) = fixture(&[
        (1.0, 1.0, 0.0, 0.0),
        (1.0, 1.0, 0.0, 0.0),
        (1.0, 1.0, 0.0, 0.0),
    ]);
    let names = shapes;
    select(&mut ctx, page, &[names[0], names[1]]);
    dispatch(&mut ctx, &registry, "arrange.bring_forward", Args::new()).expect("forward");
    assert_eq!(
        document(&ctx).pages[0].shapes,
        vec![names[2], names[0], names[1]]
    );
    dispatch(&mut ctx, &registry, "edit.undo", Args::new()).expect("undo");
    assert_eq!(
        document(&ctx).pages[0].shapes,
        vec![names[0], names[1], names[2]]
    );

    select(&mut ctx, page, &[names[1], names[2]]);
    dispatch(&mut ctx, &registry, "arrange.send_backward", Args::new()).expect("backward");
    assert_eq!(
        document(&ctx).pages[0].shapes,
        vec![names[1], names[2], names[0]]
    );
    dispatch(&mut ctx, &registry, "edit.undo", Args::new()).expect("undo");

    select(&mut ctx, page, &[names[0]]);
    dispatch(&mut ctx, &registry, "arrange.bring_to_front", Args::new()).expect("front");
    assert_eq!(
        document(&ctx).pages[0].shapes,
        vec![names[1], names[2], names[0]]
    );
    dispatch(&mut ctx, &registry, "edit.undo", Args::new()).expect("undo");

    select(&mut ctx, page, &[names[2]]);
    dispatch(&mut ctx, &registry, "arrange.send_to_back", Args::new()).expect("back");
    assert_eq!(
        document(&ctx).pages[0].shapes,
        vec![names[2], names[0], names[1]]
    );
    dispatch(&mut ctx, &registry, "edit.undo", Args::new()).expect("undo");
    assert_eq!(
        document(&ctx).pages[0].shapes,
        vec![names[0], names[1], names[2]]
    );
}

#[test]
fn arrangement_state_and_argument_failures() {
    let (mut ctx, registry, _session, page, shapes) =
        fixture(&[(1.0, 1.0, 0.0, 0.0), (1.0, 1.0, 5.0, 0.0)]);
    for id in [
        "arrange.align.left",
        "arrange.distribute.horizontal",
        "arrange.group",
        "arrange.ungroup",
        "arrange.bring_forward",
    ] {
        let command = CommandId::intern(id).expect("id");
        let error = registry
            .execute(&command, &mut ctx, Invocation::empty(Source::Test))
            .expect_err(id);
        assert!(
            matches!(error, CommandError::NotEnabled { .. }),
            "{id}: {error:?}"
        );
    }

    select(&mut ctx, page, &[shapes[0]]);
    let error =
        dispatch(&mut ctx, &registry, "arrange.ungroup", Args::new()).expect_err("not a group");
    assert!(
        matches!(
            error,
            CommandError::NotEnabled {
                reason: Some(ref reason),
                ..
            } if reason == "the selection has no group"
        ),
        "{error:?}"
    );

    select(&mut ctx, page, &shapes);
    let doc = ctx.active_document_mut().expect("session");
    let group = doc.document.group_shapes(page, &shapes).expect("group");
    select(&mut ctx, page, &[group]);
    // A group is top-level, so z-order works; a child selected alone is
    // not top-level and cannot move.
    let child = document(&ctx).shape(group).expect("group").children[0];
    select(&mut ctx, page, &[child]);
    let error = dispatch(&mut ctx, &registry, "arrange.bring_forward", Args::new())
        .expect_err("child is not top-level");
    assert!(matches!(error, CommandError::Failed { .. }), "{error:?}");
}
