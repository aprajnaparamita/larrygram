//! Glue storage (task P7-31, ADR 0084 rule 2): every glue, re-glue and
//! unglue writes the endpoint formulas, their cached values, the
//! triggers and the `Connects` records together in one undoable edit,
//! and the file round trip keeps the `F` strings and the records.
//!
//! The corpus test of item 4 opens `testfile3.vsdx` (the one corpus
//! file with glued connectors), recalculates it and saves it with every
//! endpoint formula and value unchanged. The corpus is fetched, never
//! committed (ADR 0005): the test self-skip with a message and fails
//! instead when `LARRYGRAM_REQUIRE_CORPUS=1` is set.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

#[path = "crosscheck/mod.rs"]
mod helpers;

mod support;

use helpers::{corpus_files, located_cells, read_corpus_file, skip_without_corpus};
use support::dispatch;

use larrygram_commands::app::{AppCtx, DocumentSession, Settings};
use larrygram_commands::builtins::registry;
use larrygram_commands::{ArgValue, Args, Registry};
use larrygram_model::mutate as model_mutate;
use larrygram_model::{
    Cell, CellName, CellValue, Connect, Document, PageId, Preserved, Row, Section, SectionName,
    Shape, ShapeId,
};
use larrygram_shapesheet::{CellAddress, RecalcOptions, recalc};
use larrygram_vsdx::{Limits, Package, read_document, write_document};

/// A 2 x 1 in box at `(x, y)` with two connection points (the top and
/// bottom edge midpoints), as in `tests/connector.rs`.
fn box_shape(visio_id: i32, x: f64, y: f64) -> Shape {
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

/// The document (a clone, for before/after comparisons).
fn snapshot(ctx: &AppCtx) -> Document {
    ctx.active_document().expect("session").document.clone()
}

/// The shape's own cell, or `None`.
fn own(doc: &Document, shape: ShapeId, name: &str) -> Option<Cell> {
    doc.shape(shape)?
        .sheet
        .cells
        .iter()
        .find(|cell| cell.name.as_str() == name)
        .cloned()
}

/// The formula of the shape's own cell.
fn formula(doc: &Document, shape: ShapeId, name: &str) -> Option<String> {
    own(doc, shape, name).and_then(|cell| cell.formula)
}

/// The numeric value of the shape's own cell.
fn number(doc: &Document, shape: ShapeId, name: &str) -> f64 {
    match own(doc, shape, name).and_then(|cell| cell.value) {
        Some(CellValue::Number(value)) => value,
        other => panic!("{name}: expected a number, got {other:?}"),
    }
}

/// The connector shape of the fixture page (the shape with the endpoint
/// cells; the boxes have none).
fn connector(doc: &Document, page: PageId) -> ShapeId {
    let entity = doc.page(page).expect("page");
    entity
        .shapes
        .iter()
        .copied()
        .find(|shape| doc.is_connector(*shape))
        .expect("a connector shape")
}

/// The page's `Connects` records.
fn connects(ctx: &AppCtx, page: PageId) -> Vec<Connect> {
    ctx.active_document()
        .expect("session")
        .document
        .page(page)
        .expect("page")
        .connects
        .clone()
}

/// One `connector.draw` with point glue on the begin and dynamic glue
/// on the end (Visio ids 1 and 2 of the fixture).
fn draw_point_and_dynamic(ctx: &mut AppCtx, registry: &Registry) {
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
    .expect("draw");
}

/// The drawn state plus one `connector.reroute_all`: the fixed point of
/// the re-route pass.
///
/// The pass lands a dynamic end by its own reference points (P7-32
/// replaces the landing), so a state it has not seen can move after an
/// undo re-routes it. From a fixed point the pass writes nothing
/// visible, and an undo restores exactly.
fn draw_rerouted(ctx: &mut AppCtx, registry: &Registry) -> Document {
    draw_point_and_dynamic(ctx, registry);
    dispatch(ctx, registry, "connector.reroute_all", Args::new()).expect("reroute");
    snapshot(ctx)
}

#[test]
fn a_draw_writes_the_formulas_and_the_records_together() {
    let (mut ctx, registry, page) = fixture();
    draw_point_and_dynamic(&mut ctx, &registry);
    let doc = snapshot(&ctx);
    let drawn = connector(&doc, page);

    // The `Connects` records (spec section 8's spellings).
    let records = connects(&ctx, page);
    assert_eq!(records.len(), 2, "{records:?}");
    let begin = records
        .iter()
        .find(|record| record.from_cell == "BeginX")
        .expect("begin record");
    assert_eq!(begin.from_part, model_mutate::VIS_BEGIN);
    assert_eq!(begin.to_sheet, 1);
    assert_eq!(begin.to_cell, "Connections.X1");
    assert_eq!(begin.to_part, model_mutate::VIS_CONNECTION_POINT);
    let end = records
        .iter()
        .find(|record| record.from_cell == "EndX")
        .expect("end record");
    assert_eq!(end.from_part, model_mutate::VIS_END);
    assert_eq!(end.to_sheet, 2);
    assert_eq!(end.to_cell, "PinX");
    assert_eq!(end.to_part, model_mutate::VIS_WHOLE_SHAPE);

    // Point glue: the row number in the formula is one-based.
    assert_eq!(
        formula(&doc, drawn, "BeginX"),
        Some("PAR(PNT(Sheet.1!Connections.X1,Sheet.1!Connections.Y1))".to_owned())
    );
    assert_eq!(
        formula(&doc, drawn, "BeginY"),
        Some("PAR(PNT(Sheet.1!Connections.X1,Sheet.1!Connections.Y1))".to_owned())
    );
    assert_eq!(
        formula(&doc, drawn, "BegTrigger"),
        Some("_XFTRIGGER(Sheet.1!EventXFMod)".to_owned())
    );
    // Dynamic glue: `_WALKGLUE` with the end's own trigger first.
    assert_eq!(
        formula(&doc, drawn, "EndX"),
        Some("_WALKGLUE(EndTrigger,BegTrigger,WalkPreference)".to_owned())
    );
    assert_eq!(
        formula(&doc, drawn, "EndY"),
        Some("_WALKGLUE(EndTrigger,BegTrigger,WalkPreference)".to_owned())
    );
    assert_eq!(
        formula(&doc, drawn, "EndTrigger"),
        Some("_XFTRIGGER(Sheet.2!EventXFMod)".to_owned())
    );
    // The cached values are the resolved points in page inches: the
    // point glue lands exactly on the row, the dynamic glue on the
    // nearest bounds point (the old landing; P7-32 replaces it).
    assert!((number(&doc, drawn, "BeginX") - 2.0).abs() < 1e-9);
    assert!((number(&doc, drawn, "BeginY") - 2.5).abs() < 1e-9);
    assert!((number(&doc, drawn, "EndX") - 5.0).abs() < 1e-9);
    assert!((number(&doc, drawn, "EndY") - 2.5).abs() < 1e-9);
}

#[test]
fn a_reglue_replaces_the_formula_and_one_undo_restores_everything() {
    let (mut ctx, registry, page) = fixture();
    let before = draw_rerouted(&mut ctx, &registry);
    let doc = before.clone();
    let drawn = connector(&doc, page);

    // Re-glue the begin end to row 1 of box 2.
    dispatch(
        &mut ctx,
        &registry,
        "connector.glue_begin",
        Args::new()
            .with("connector", ArgValue::Integer(3))
            .with("target", ArgValue::Integer(2))
            .with("point", ArgValue::Integer(1)),
    )
    .expect("re-glue");
    {
        let doc = snapshot(&ctx);
        assert_eq!(
            formula(&doc, drawn, "BeginX"),
            Some("PAR(PNT(Sheet.2!Connections.X2,Sheet.2!Connections.Y2))".to_owned()),
            "the formula is replaced, no stale Sheet.1"
        );
        assert_eq!(
            formula(&doc, drawn, "BegTrigger"),
            Some("_XFTRIGGER(Sheet.2!EventXFMod)".to_owned())
        );
    }

    // One undo restores the cells and the records exactly.
    dispatch(&mut ctx, &registry, "edit.undo", Args::new()).expect("undo");
    assert_eq!(snapshot(&ctx), before, "the undo restores the exact state");
}

#[test]
fn an_unglue_writes_the_literal_point() {
    let (mut ctx, registry, page) = fixture();
    let before = draw_rerouted(&mut ctx, &registry);
    let doc = before.clone();
    let drawn = connector(&doc, page);
    let end_x = number(&doc, drawn, "EndX");
    let end_y = number(&doc, drawn, "EndY");

    dispatch(
        &mut ctx,
        &registry,
        "connector.unglue",
        Args::new()
            .with("connector", ArgValue::Integer(3))
            .with("end", ArgValue::Text("end".to_owned())),
    )
    .expect("unglue");
    {
        let doc = snapshot(&ctx);
        // The end keeps its last point as a literal: no formula.
        assert_eq!(formula(&doc, drawn, "EndX"), None);
        assert_eq!(formula(&doc, drawn, "EndY"), None);
        assert!((number(&doc, drawn, "EndX") - end_x).abs() < 1e-9);
        assert!((number(&doc, drawn, "EndY") - end_y).abs() < 1e-9);
        assert_eq!(
            formula(&doc, drawn, "EndTrigger"),
            Some("_XFTRIGGER(EventXFMod)".to_owned()),
            "the trigger names the end's own sheet"
        );
        // The other end keeps its glue.
        assert!(formula(&doc, drawn, "BeginX")
            .expect("begin formula")
            .starts_with("PAR(PNT("));
        let records = connects(&ctx, page);
        assert_eq!(records.len(), 1, "one record left: {records:?}");
    }

    // Undo puts the glue back.
    dispatch(&mut ctx, &registry, "edit.undo", Args::new()).expect("undo");
    assert_eq!(snapshot(&ctx), before);
}

#[test]
fn deleting_a_target_clears_the_formulas_in_the_same_edit() {
    let (mut ctx, registry, page) = fixture();
    let before = draw_rerouted(&mut ctx, &registry);
    let doc = before.clone();
    let drawn = connector(&doc, page);

    // Delete box 1 (the begin end's target).
    let box1 = doc
        .shape_by_visio_id(page, 1)
        .expect("box 1");
    ctx.selection.shapes = vec![box1];
    ctx.selection.anchor = Some(box1);
    ctx.selection.page = Some(page);
    dispatch(&mut ctx, &registry, "edit.delete", Args::new()).expect("delete");
    {
        let doc = snapshot(&ctx);
        // The begin end keeps its last point as a literal and its
        // trigger names its own sheet; the record goes with the delete.
        assert_eq!(formula(&doc, drawn, "BeginX"), None);
        assert_eq!(formula(&doc, drawn, "BeginY"), None);
        assert_eq!(
            formula(&doc, drawn, "BegTrigger"),
            Some("_XFTRIGGER(EventXFMod)".to_owned())
        );
        assert!((number(&doc, drawn, "BeginX") - 2.0).abs() < 1e-9);
        assert!((number(&doc, drawn, "BeginY") - 2.5).abs() < 1e-9);
        let records = connects(&ctx, page);
        assert!(records.iter().all(|record| record.to_sheet != 1), "{records:?}");
        // The end glued to box 2 keeps its glue.
        assert!(formula(&doc, drawn, "EndX")
            .expect("end formula")
            .starts_with("_WALKGLUE("));
    }

    // One undo restores the cells and the records exactly.
    dispatch(&mut ctx, &registry, "edit.undo", Args::new()).expect("undo");
    assert_eq!(snapshot(&ctx), before, "the undo restores the exact state");
}

#[test]
fn a_save_and_reopen_keeps_the_f_strings_and_the_records() {
    let (mut ctx, registry, page) = fixture();
    draw_point_and_dynamic(&mut ctx, &registry);
    let before = snapshot(&ctx);
    let drawn = connector(&before, page);
    let records = connects(&ctx, page);

    let bytes = write_document(&before, &Package::new()).expect("save");
    let back = read_document(&bytes, &Limits::DEFAULT).expect("reopen");
    let back_connector = connector(&back, page);

    for name in [
        "BeginX",
        "BeginY",
        "EndX",
        "EndY",
        "BegTrigger",
        "EndTrigger",
    ] {
        assert_eq!(
            formula(&back, back_connector, name),
            formula(&before, drawn, name),
            "{name}: the F string"
        );
        assert_eq!(
            own(&back, back_connector, name).and_then(|cell| cell.value),
            own(&before, drawn, name).and_then(|cell| cell.value),
            "{name}: the cached V"
        );
    }
    assert_eq!(
        back.page(page).expect("page").connects,
        records,
        "the Connects records"
    );
}

/// The endpoint and trigger cells of every shape that has one, as
/// `(shape visio id, cell name) -> (formula, value)` pairs.
fn endpoint_cells(document: &Document) -> Vec<(i32, String, Option<String>, Option<CellValue>)> {
    let mut out = Vec::new();
    let mut shapes: Vec<ShapeId> = Vec::new();
    for page in &document.pages {
        shapes.extend(page.shapes.iter().copied());
    }
    for master in &document.masters {
        shapes.extend(master.shapes.iter().copied());
    }
    for shape in shapes {
        let Some(entity) = document.shape(shape) else {
            continue;
        };
        for name in ["BeginX", "BeginY", "EndX", "EndY", "BegTrigger", "EndTrigger"] {
            if let Some(cell) = entity
                .sheet
                .cells
                .iter()
                .find(|cell| cell.name.as_str() == name)
            {
                out.push((entity.visio_id, name.to_owned(), cell.formula.clone(), cell.value.clone()));
            }
        }
    }
    out
}

/// Item 4 (task P7-31): the corpus file with glued connectors opens,
/// recalcs and saves with every endpoint formula and value unchanged,
/// and the Visio-owned functions report nothing.
#[test]
fn the_corpus_file_keeps_its_endpoint_formulas_over_open_recalc_save() {
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
    let mut document = read_corpus_file(&path);
    let before = endpoint_cells(&document);
    assert!(!before.is_empty(), "the file has glued connectors");

    let dirty: Vec<CellAddress> = located_cells(&document)
        .iter()
        .map(|cell| cell.address.clone())
        .collect();
    let report = recalc(&mut document, &dirty, &RecalcOptions::new()).expect("recalc");
    for warning in &report.warnings {
        let text = format!("{warning:?}");
        for owned in ["PAR", "PNT", "_WALKGLUE", "_XFTRIGGER"] {
            assert!(
                !text.contains(owned),
                "no warning for a Visio-owned function: {text}"
            );
        }
    }
    assert_eq!(
        endpoint_cells(&document),
        before,
        "the recalc leaves every endpoint formula and value unchanged"
    );

    let bytes = write_document(&document, &Package::new()).expect("save");
    let back = read_document(&bytes, &Limits::DEFAULT).expect("reopen");
    assert_eq!(
        endpoint_cells(&back),
        before,
        "the save keeps every endpoint formula and value"
    );
}
