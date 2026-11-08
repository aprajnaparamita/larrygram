//! Headless clipboard command tests (task P5-09, ADR 0047).
//!
//! Cover: copy/paste of a group subtree with internal connects and a
//! kept `Master` link; cut and duplicate as one undo step; consecutive
//! paste offsets and paste at the pointer; and the argument and
//! enabled-state failures.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use support::dispatch;

use larrygram_commands::app::{AppCtx, DocumentSession, Settings};
use larrygram_commands::builtins::registry;
use larrygram_commands::{
    ArgValue, Args, CommandError, CommandId, Invocation, Registry, SessionId, Source,
};
use larrygram_geom::{Point, Unit};
use larrygram_model::{
    Cell, CellName, CellValue, Connect, Document, Master, MasterId, PageId, Preserved, Shape,
    ShapeId,
};

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

fn fixture() -> (AppCtx, Registry, SessionId, PageId) {
    let mut doc = Document::new();
    let page = doc.add_page("Page-1").expect("page");
    let mut ctx = AppCtx::new(Settings::default());
    let index = ctx.open_document(DocumentSession::new(doc, None));
    let session = ctx.documents.get(index).expect("session").id;
    (ctx, registry().expect("registry"), session, page)
}

fn select(ctx: &mut AppCtx, page: PageId, shapes: &[ShapeId]) {
    ctx.selection.shapes = shapes.to_vec();
    ctx.selection.anchor = shapes.last().copied();
    ctx.selection.page = Some(page);
}

fn document(ctx: &AppCtx) -> &Document {
    &ctx.active_document().expect("document").document
}

fn pin(doc: &Document, shape: ShapeId) -> (f64, f64) {
    let entry = doc.shape(shape).expect("shape");
    let get = |name: &str| {
        entry
            .sheet
            .cells
            .iter()
            .find(|cell| cell.name.as_str() == name)
            .and_then(|cell| match cell.value {
                Some(CellValue::Number(value)) => Some(value),
                _ => None,
            })
            .unwrap_or(0.0)
    };
    (get("PinX"), get("PinY"))
}

#[test]
fn copy_paste_round_trips_a_group_with_connects_and_master_links() {
    let (mut ctx, registry, _session, page) = fixture();
    let (group, source_visio) = {
        let doc = &mut ctx.active_document_mut().expect("session").document;
        let mut master_shape = shape(1.0, 1.0, 0.0, 0.0);
        master_shape.name_u = "Box".to_owned();
        let master_shape = doc.add_shape(master_shape);
        let mut master = Master::new(MasterId { index: 0 }, 1);
        master.name = "Box".to_owned();
        master.name_u = "Box".to_owned();
        master.shapes = vec![master_shape];
        let master = doc.add_master(master).expect("master");
        let instance = doc
            .instantiate_master(master, page, Point::new(4.0, 0.0), None)
            .expect("instance");
        let other = doc
            .insert_shape(page, shape(2.0, 1.0, 0.0, 0.0), None)
            .expect("other");
        let instance_visio = doc.shape(instance).expect("shape").visio_id;
        let other_visio = doc.shape(other).expect("shape").visio_id;
        doc.add_connect(
            page,
            Connect {
                from_sheet: instance_visio,
                to_sheet: other_visio,
                ..Connect::new()
            },
        )
        .expect("connect");
        let group = doc.group_shapes(page, &[instance, other]).expect("group");
        let visio: Vec<i32> = doc
            .shapes
            .iter()
            .filter(|shape| shape.visio_id != 0)
            .map(|shape| shape.visio_id)
            .collect();
        (group, visio)
    };
    select(&mut ctx, page, &[group]);
    dispatch(&mut ctx, &registry, "edit.copy", Args::new()).expect("copy");
    let content = ctx.clipboard.content.clone().expect("clipboard");
    assert_eq!(content.shapes.len(), 3, "group plus two members");
    assert_eq!(content.connects.len(), 1, "internal connect");
    assert_eq!(ctx.clipboard.pastes, 0);
    assert_eq!(document(&ctx).pages[0].shapes.len(), 1);

    let outcome = dispatch(&mut ctx, &registry, "edit.paste", Args::new()).expect("paste");
    assert!(outcome.message.as_deref().unwrap_or("").contains("Pasted"));
    let doc = document(&ctx);
    assert_eq!(
        doc.pages[0].shapes.len(),
        2,
        "pasted fragment is one page shape"
    );
    let pasted = *doc.pages[0].shapes.last().expect("pasted root");
    let pasted_entry = doc.shape(pasted).expect("pasted");
    assert_eq!(pasted_entry.children.len(), 2, "group subtree survived");
    assert!(
        !source_visio.contains(&pasted_entry.visio_id),
        "the pasted root got a fresh Visio id"
    );
    assert_eq!(
        doc.pages[0].connects.len(),
        2,
        "the internal connect was copied"
    );
    let pasted_visio: Vec<i32> = pasted_entry
        .children
        .iter()
        .filter_map(|child| doc.shape(*child).map(|shape| shape.visio_id))
        .collect();
    let copied_connect = doc.pages[0]
        .connects
        .iter()
        .find(|connect| {
            pasted_visio.contains(&connect.from_sheet) && pasted_visio.contains(&connect.to_sheet)
        })
        .expect("the copied connect names the pasted members");
    assert!(
        !source_visio.contains(&copied_connect.from_sheet)
            && !source_visio.contains(&copied_connect.to_sheet),
        "connect endpoints were remapped: {copied_connect:?}"
    );
    let master_kept = pasted_entry.children.iter().any(|child| {
        doc.shape(*child)
            .is_some_and(|shape| shape.master == Some(MasterId { index: 0 }))
    });
    assert!(master_kept, "the Master link survives the copy");
    let source_pin = {
        let group = doc
            .shape(*doc.pages[0].shapes.first().expect("source"))
            .expect("source");
        (
            cell_number(group, "PinX").unwrap_or(0.0),
            cell_number(group, "PinY").unwrap_or(0.0),
        )
    };
    let pasted_pin = pin(doc, pasted);
    assert!((pasted_pin.0 - (source_pin.0 + 0.1)).abs() < 1e-9);
    assert!((pasted_pin.1 - (source_pin.1 - 0.1)).abs() < 1e-9);

    dispatch(&mut ctx, &registry, "edit.undo", Args::new()).expect("undo");
    let doc = document(&ctx);
    assert_eq!(
        doc.pages[0].shapes.len(),
        1,
        "one undo removed the fragment"
    );
    assert_eq!(doc.pages[0].connects.len(), 1);
}

fn cell_number(shape: &Shape, name: &str) -> Option<f64> {
    shape
        .sheet
        .cells
        .iter()
        .find(|cell| cell.name.as_str() == name)
        .and_then(|cell| match cell.value {
            Some(CellValue::Number(value)) => Some(value),
            _ => None,
        })
}

#[test]
fn cut_is_copy_plus_delete_in_one_undo_step() {
    let (mut ctx, registry, _session, page) = fixture();
    let shape = ctx
        .active_document_mut()
        .expect("session")
        .document
        .insert_shape(page, shape(2.0, 1.0, 0.0, 0.0), None)
        .expect("shape");
    select(&mut ctx, page, &[shape]);
    dispatch(&mut ctx, &registry, "edit.cut", Args::new()).expect("cut");
    assert!(ctx.clipboard.content.is_some());
    assert!(document(&ctx).pages[0].shapes.is_empty());

    dispatch(&mut ctx, &registry, "edit.undo", Args::new()).expect("undo");
    assert_eq!(document(&ctx).pages[0].shapes.len(), 1, "cut is one step");
    assert_eq!(document(&ctx).pages[0].shapes[0], shape);
    assert!(document(&ctx).shape(shape).is_some());
}

#[test]
fn duplicate_copies_the_selection_without_touching_the_clipboard() {
    let (mut ctx, registry, _session, page) = fixture();
    let shape = ctx
        .active_document_mut()
        .expect("session")
        .document
        .insert_shape(page, shape(2.0, 1.0, 1.0, 2.0), None)
        .expect("shape");
    select(&mut ctx, page, &[shape]);
    dispatch(&mut ctx, &registry, "edit.copy", Args::new()).expect("copy");
    let before = ctx.clipboard.clone();
    dispatch(&mut ctx, &registry, "edit.duplicate", Args::new()).expect("duplicate");
    assert_eq!(
        ctx.clipboard, before,
        "duplicate leaves the clipboard alone"
    );
    let doc = document(&ctx);
    assert_eq!(doc.pages[0].shapes.len(), 2);
    let pasted = *doc.pages[0].shapes.last().expect("pasted");
    assert_eq!(pin(doc, pasted), (1.1, 1.9));
    dispatch(&mut ctx, &registry, "edit.undo", Args::new()).expect("undo");
    assert_eq!(document(&ctx).pages[0].shapes.len(), 1);
}

#[test]
fn paste_offsets_each_time_and_pointer_paste_places_the_pin() {
    let (mut ctx, registry, _session, page) = fixture();
    let shape = ctx
        .active_document_mut()
        .expect("session")
        .document
        .insert_shape(page, shape(2.0, 1.0, 1.0, 2.0), None)
        .expect("shape");
    select(&mut ctx, page, &[shape]);
    dispatch(&mut ctx, &registry, "edit.copy", Args::new()).expect("copy");

    dispatch(&mut ctx, &registry, "edit.paste", Args::new()).expect("first");
    let doc = document(&ctx);
    let first = *doc.pages[0].shapes.last().expect("first");
    assert_eq!(pin(doc, first), (1.1, 1.9));
    assert_eq!(ctx.clipboard.pastes, 1);

    dispatch(&mut ctx, &registry, "edit.paste", Args::new()).expect("second");
    let doc = document(&ctx);
    let second = *doc.pages[0].shapes.last().expect("second");
    assert_eq!(pin(doc, second), (1.2, 1.8));
    assert_eq!(ctx.clipboard.pastes, 2);

    let mut pointer = Args::new();
    pointer.set("x", ArgValue::Number(5.0));
    pointer.set("y", ArgValue::Number(6.0));
    dispatch(&mut ctx, &registry, "edit.paste", pointer).expect("pointer paste");
    let doc = document(&ctx);
    let third = *doc.pages[0].shapes.last().expect("third");
    assert_eq!(pin(doc, third), (5.0, 6.0));
    assert_eq!(ctx.clipboard.pastes, 2, "pointer paste does not advance");
}

/// An empty fragment is refused by name before any placement (the
/// pointer has no root pin to move).
#[test]
fn a_pointer_paste_of_an_empty_fragment_is_refused() {
    let (mut ctx, registry, _session, page) = fixture();
    ctx.clipboard.content = Some(larrygram_commands::clipboard::ClipboardContent {
        kind: larrygram_commands::clipboard::ClipboardKind::Shapes,
        shapes: Vec::new(),
        connects: Vec::new(),
        source_page: page,
    });
    let mut pointer = Args::new();
    pointer.set("x", ArgValue::Number(5.0));
    pointer.set("y", ArgValue::Number(6.0));
    let error = dispatch(&mut ctx, &registry, "edit.paste", pointer).expect_err("no root shape");
    assert!(
        error.to_string().contains("the clipboard holds no root shape"),
        "{error}"
    );
    let doc = document(&ctx);
    assert!(doc.pages[0].shapes.is_empty(), "nothing was pasted");
}

#[test]
fn clipboard_argument_and_state_failures() {
    let (mut ctx, registry, _session, page) = fixture();
    let shape = ctx
        .active_document_mut()
        .expect("session")
        .document
        .insert_shape(page, shape(2.0, 1.0, 0.0, 0.0), None)
        .expect("shape");
    let copy = CommandId::parse("edit.copy").expect("id");
    let paste = CommandId::parse("edit.paste").expect("id");
    let duplicate = CommandId::parse("edit.duplicate").expect("id");

    let error = registry
        .execute(&paste, &mut ctx, Invocation::empty(Source::Test))
        .expect_err("empty clipboard");
    assert!(
        matches!(error, CommandError::NotEnabled { .. }),
        "{error:?}"
    );
    let error = registry
        .execute(&copy, &mut ctx, Invocation::empty(Source::Test))
        .expect_err("no selection");
    assert!(matches!(error, CommandError::NotEnabled { .. }));
    let error = registry
        .execute(&duplicate, &mut ctx, Invocation::empty(Source::Test))
        .expect_err("no selection");
    assert!(matches!(error, CommandError::NotEnabled { .. }));

    select(&mut ctx, page, &[shape]);
    dispatch(&mut ctx, &registry, "edit.copy", Args::new()).expect("copy");
    let mut half_pointer = Args::new();
    half_pointer.set("x", ArgValue::Number(1.0));
    let error = dispatch(&mut ctx, &registry, "edit.paste", half_pointer).expect_err("half pair");
    assert!(
        matches!(error, CommandError::InvalidArgs { .. }),
        "{error:?}"
    );
    let mut bad = Args::new();
    bad.set("x", ArgValue::Text("nope".to_owned()));
    let error = dispatch(&mut ctx, &registry, "edit.paste", bad).expect_err("bad x");
    assert!(matches!(error, CommandError::InvalidArgs { .. }));
    let mut infinite = Args::new();
    infinite.set("x", ArgValue::Number(f64::INFINITY));
    infinite.set("y", ArgValue::Number(1.0));
    let error = dispatch(&mut ctx, &registry, "edit.paste", infinite).expect_err("non-finite");
    assert!(matches!(error, CommandError::InvalidArgs { .. }));

    // A direct call with no selection is the same failure as the
    // enabled-state path (never a panic).
    ctx.selection.clear();
    let error = dispatch(&mut ctx, &registry, "edit.copy", Args::new()).expect_err("no selection");
    assert!(matches!(error, CommandError::NotEnabled { .. }));
}

/// The P5-11 open item (task P5-19 coverage): a copied fragment whose
/// `Connect` record names a shape outside the copied set.
///
/// The record is not copied (a fragment's glue needs both ends). The
/// re-route pass of the paste drops the source's record that names a
/// shape that is nowhere on the page, with one warning line, so the
/// pasted connector carries no stale glue and the live glue stays.
#[test]
fn paste_of_a_fragment_with_a_connect_outside_the_copy_carries_no_stale_glue() {
    let (mut ctx, registry, _session, page) = fixture();
    let (connector, outside) = {
        let doc = &mut ctx.active_document_mut().expect("session").document;
        let outside = doc
            .insert_shape(page, shape(1.0, 1.0, 0.0, 0.0), None)
            .expect("outside");
        let connector = doc
            .insert_shape(page, shape(2.0, 1.0, 3.0, 0.0), None)
            .expect("connector");
        let outside_visio = doc.shape(outside).expect("shape").visio_id;
        let connector_visio = doc.shape(connector).expect("shape").visio_id;
        for (from_cell, to_sheet) in [("EndX", outside_visio), ("BeginX", 99i32)] {
            doc.add_connect(
                page,
                Connect {
                    from_sheet: connector_visio,
                    from_cell: from_cell.to_owned(),
                    from_part: larrygram_model::mutate::VIS_BEGIN,
                    to_sheet,
                    to_cell: "PinX".to_owned(),
                    to_part: larrygram_model::mutate::VIS_WHOLE_SHAPE,
                    ..Connect::new()
                },
            )
            .expect("connect");
        }
        (connector, outside)
    };
    select(&mut ctx, page, &[connector]);
    dispatch(&mut ctx, &registry, "edit.copy", Args::new()).expect("copy");
    let content = ctx.clipboard.content.clone().expect("clipboard");
    assert!(
        content.connects.is_empty(),
        "the outside glue is not copied"
    );

    dispatch(&mut ctx, &registry, "edit.paste", Args::new()).expect("paste");
    let doc = document(&ctx);
    let entity = doc.page(page).expect("page");
    let pasted = *entity.shapes.last().expect("pasted");
    let pasted_visio = doc.shape(pasted).expect("pasted").visio_id;
    let source_visio = doc.shape(connector).expect("connector").visio_id;
    let outside_visio = doc.shape(outside).expect("outside").visio_id;
    assert!(
        entity
            .connects
            .iter()
            .all(|record| record.from_sheet != pasted_visio && record.to_sheet != pasted_visio),
        "the pasted connector has no glue"
    );
    assert!(
        entity
            .connects
            .iter()
            .any(|record| record.from_sheet == source_visio && record.to_sheet == outside_visio),
        "the live glue stays"
    );
    assert!(
        ctx.warnings
            .iter()
            .any(|line| line.contains("lost its glue to detached shape 99")),
        "{:?}",
        ctx.warnings
    );
}
