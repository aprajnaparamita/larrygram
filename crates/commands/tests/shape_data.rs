//! The `data.set.property` command (task P6-18): the Shape Data pane's
//! one edit (ADR 0035's reserved id, ADR 0058).
//!
//! The contract under test:
//!
//! - the id is in the registry with a label and an enabled-state
//!   predicate, like every other command (AGENTS.md section 6), and it
//!   writes `Prop.<row>.Value` through the direct-edit seam;
//! - `value` is read in the kind the row's `Value` cell caches (ADR
//!   0058): a grammar literal for a number (`2.5 cm`), `TRUE`/`FALSE`
//!   for a boolean, `#RRGGBB` for a colour, the text for a string;
//! - one dispatch is one undo step whose `revert(apply(x)) == x` and
//!   whose recalc pass follows the edit like every other cell edit
//!   (ADR 0056);
//! - an edit of a row the shape inherits from its master writes a local
//!   row on the instance (the P6-09 note), and undoing it puts the
//!   inherited row back;
//! - what the command refuses: an unknown row, a list or error value, a
//!   row with no cached value, a bad literal, a reference or call, a
//!   non-finite number.

// The never-crash lints apply to non-test code only (clippy.toml).
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use support::dispatch;

use larrygram_commands::app::{AppCtx, DocumentSession, Settings};
use larrygram_commands::builtins::registry;
use larrygram_commands::{ArgValue, Args, CommandError, CommandId, CommandStatus, Registry};
use larrygram_geom::Unit as GeomUnit;
use larrygram_model::mutate::literal;
use larrygram_model::{
    Cell, CellName, CellValue, Color, Document, Master, MasterId, Row, Section, SectionName, Shape,
    ShapeId,
};

/// A `Property` row with the given name and `Value` cell.
fn prop_row(name: &str, value: Option<CellValue>, unit: GeomUnit) -> Row {
    let mut row = Row::new();
    row.name = Some(name.to_owned());
    row.ix = Some(0);
    if let Some(value) = value {
        let mut cell = Cell::new(CellName::from_static("Value"));
        cell.value = Some(value);
        cell.unit = unit;
        row.cells.push(cell);
    }
    row
}

/// A shape whose `Property` section holds `rows`.
fn shape_with(rows: Vec<Row>) -> Shape {
    let mut shape = Shape::new(ShapeId { index: 0 }, 1);
    let mut section = Section::new(SectionName::from_static("Property"));
    section.rows = rows;
    shape.sheet.sections.push(section);
    shape
}

/// A session with one page and one shape carrying one row of every
/// kind, and a master whose `Property` rows the shape inherits.
fn fixture() -> (AppCtx, Registry, ShapeId) {
    let mut doc = Document::new();
    // The master chain first: its `Cost` row clashes with the
    // instance's (the instance wins) and `Mastered` is inherited.
    let rung = shape_with(vec![
        prop_row("Cost", Some(CellValue::Number(9.0)), GeomUnit::Inches),
        prop_row("Mastered", Some(CellValue::Number(3.0)), GeomUnit::Inches),
    ]);
    let rung = doc.add_shape(rung);
    let mut master = Master::new(MasterId { index: 0 }, 1);
    master.shapes = vec![rung];
    doc.masters.push(master);

    let mut shape = shape_with(vec![
        prop_row("Cost", Some(CellValue::Number(2.0)), GeomUnit::Inches),
        prop_row("Span", Some(CellValue::Number(1.0)), GeomUnit::Feet),
        prop_row("Rounded", Some(CellValue::Bool(false)), GeomUnit::Unitsless),
        prop_row(
            "Tint",
            Some(CellValue::Color(Color::new(0, 128, 255))),
            GeomUnit::Unitsless,
        ),
        prop_row(
            "Note",
            Some(CellValue::Str(String::from("hello"))),
            GeomUnit::Unitsless,
        ),
        prop_row(
            "Bag",
            Some(CellValue::List(String::from("a;b"))),
            GeomUnit::Unitsless,
        ),
        prop_row("Empty", None, GeomUnit::Unitsless),
    ]);
    shape.master = Some(MasterId { index: 0 });
    let id = doc.add_shape(shape);
    let page = doc.add_page("Page-1").expect("page");
    if let Some(entry) = doc.pages.get_mut(page.index as usize) {
        entry.shapes.push(id);
    }

    let mut ctx = AppCtx::new(Settings::default());
    ctx.open_document(DocumentSession::new(doc, None));
    ctx.selection.shapes = vec![id];
    ctx.selection.anchor = Some(id);
    let registry = registry().expect("the registry builds");
    (ctx, registry, id)
}

/// Undo of an edit to an inherited Shape Data value on a shape with no
/// local rows removes the whole local section the write created (task
/// P6-18's exact restore).
#[test]
fn undo_of_an_inherited_value_removes_the_section_it_created() {
    let mut doc = Document::new();
    let rung = shape_with(vec![prop_row(
        "Mastered",
        Some(CellValue::Number(3.0)),
        GeomUnit::Inches,
    )]);
    let rung = doc.add_shape(rung);
    let mut master = Master::new(MasterId { index: 0 }, 1);
    master.shapes = vec![rung];
    doc.masters.push(master);
    // The instance has no local `Property` section at all.
    let mut shape = Shape::new(ShapeId { index: 0 }, 2);
    shape.master = Some(MasterId { index: 0 });
    let id = doc.add_shape(shape);
    let page = doc.add_page("Page-1").expect("page");
    if let Some(entry) = doc.pages.get_mut(page.index as usize) {
        entry.shapes.push(id);
    }
    let mut ctx = AppCtx::new(Settings::default());
    ctx.open_document(DocumentSession::new(doc, None));
    ctx.selection.shapes = vec![id];
    ctx.selection.anchor = Some(id);
    let registry = registry().expect("the registry builds");

    dispatch(
        &mut ctx,
        &registry,
        "data.set.property",
        value_args("Mastered", "7"),
    )
    .expect("the edit runs");
    {
        let session = ctx.active_document().expect("document");
        let entity = session.document.shape(id).expect("shape");
        assert!(
            entity
                .sheet
                .sections
                .iter()
                .any(|section| section.name.as_str() == "Property"),
            "the write created the local section"
        );
    }
    dispatch(&mut ctx, &registry, "edit.undo", Args::new()).expect("undo");
    let session = ctx.active_document().expect("document");
    let entity = session.document.shape(id).expect("shape");
    assert!(
        !entity
            .sheet
            .sections
            .iter()
            .any(|section| section.name.as_str() == "Property"),
        "the undo removes the section it created"
    );
}

/// The `value` argument set of one `row`.
fn value_args(row: &str, value: &str) -> Args {
    Args::new()
        .with("row", ArgValue::Text(row.to_owned()))
        .with("value", ArgValue::Text(value.to_owned()))
}

/// The undo steps of the active document (one dispatch, one step).
fn undo_len(ctx: &AppCtx) -> usize {
    ctx.active_document().expect("document").undo.len()
}

/// The cached `Value` of the shape's own row `row`.
fn own_value(ctx: &AppCtx, shape: ShapeId, row: &str) -> Option<CellValue> {
    let session = ctx.active_document().expect("document");
    session
        .document
        .property_rows(shape)
        .into_iter()
        .find(|entry| !entry.inherited && entry.key == row)
        .and_then(|entry| {
            entry
                .row
                .cells
                .into_iter()
                .find(|cell| cell.name.as_str() == "Value")
                .and_then(|cell| cell.value)
        })
}

/// The formula of the shape's own row `row`'s `Value` cell.
fn own_formula(ctx: &AppCtx, shape: ShapeId, row: &str) -> Option<String> {
    let session = ctx.active_document().expect("document");
    session
        .document
        .property_rows(shape)
        .into_iter()
        .find(|entry| !entry.inherited && entry.key == row)
        .and_then(|entry| {
            entry
                .row
                .cells
                .into_iter()
                .find(|cell| cell.name.as_str() == "Value")
                .and_then(|cell| cell.formula)
        })
}

#[test]
fn data_set_property_is_registered_with_a_label_and_a_predicate() {
    let (mut ctx, registry, _shape) = fixture();
    let id = CommandId::parse("data.set.property").expect("id");
    let meta = registry.meta(&id).expect("meta");
    assert_eq!(meta.label, "Value");
    assert_eq!(registry.status(&id), CommandStatus::Implemented);
    assert!(
        registry.enabled(&id, &ctx).is_enabled(),
        "a selection exists"
    );

    ctx.selection.shapes.clear();
    ctx.selection.anchor = None;
    let enabled = registry.enabled(&id, &ctx);
    assert!(!enabled.is_enabled());
    assert_eq!(enabled.reason.as_deref(), Some("no selection"));
}

#[test]
fn a_number_value_edit_is_one_undo_step() {
    let (mut ctx, registry, shape) = fixture();
    let before = undo_len(&ctx);
    dispatch(
        &mut ctx,
        &registry,
        "data.set.property",
        value_args("Cost", "2.5 cm"),
    )
    .expect("the edit runs");
    assert_eq!(undo_len(&ctx), before + 1, "one undo step");
    let value = own_value(&ctx, shape, "Cost").expect("the value");
    let CellValue::Number(number) = value else {
        panic!("the value is a number, got {value:?}");
    };
    assert!(
        (number - 2.5 / 2.54).abs() < 1.0e-12,
        "2.5 cm is {number} in"
    );
    assert_eq!(
        own_formula(&ctx, shape, "Cost"),
        Some(literal(number)),
        "the formula is the literal the model spells"
    );

    // Undo puts the old value back and redo replays the edit.
    dispatch(&mut ctx, &registry, "edit.undo", Args::new()).expect("undo");
    assert_eq!(own_value(&ctx, shape, "Cost"), Some(CellValue::Number(2.0)));
    dispatch(&mut ctx, &registry, "edit.redo", Args::new()).expect("redo");
    match own_value(&ctx, shape, "Cost").expect("the value") {
        CellValue::Number(redone) => assert!((redone - number).abs() < 1.0e-12, "{redone}"),
        other => panic!("the value is a number, got {other:?}"),
    }
}

#[test]
fn a_plain_number_is_a_number_of_the_cells_own_unit() {
    let (mut ctx, registry, shape) = fixture();
    // `Cost` records inches, so `5` writes 5 inches (canonical 5).
    dispatch(
        &mut ctx,
        &registry,
        "data.set.property",
        value_args("Cost", "5"),
    )
    .expect("the edit runs");
    assert_eq!(own_value(&ctx, shape, "Cost"), Some(CellValue::Number(5.0)));

    // A ratio cell records percent, so `37` writes 37 % (0.37).
    let mut ctx2 = ctx;
    {
        let session = ctx2.active_document_mut().expect("document");
        if let Some(section) = session.document.shape_mut(shape).and_then(|entity| {
            entity
                .sheet
                .sections
                .iter_mut()
                .find(|entry| entry.name.as_str() == "Property")
        }) {
            let mut row = prop_row("Ratio", Some(CellValue::Number(0.37)), GeomUnit::Percent);
            row.ix = Some(1);
            section.rows.push(row);
        }
    }
    dispatch(
        &mut ctx2,
        &registry,
        "data.set.property",
        value_args("Ratio", "37"),
    )
    .expect("the edit runs");
    assert_eq!(
        own_value(&ctx2, shape, "Ratio"),
        Some(CellValue::Number(0.37)),
        "37 of a percent cell is 37 %"
    );
}

#[test]
fn a_boolean_colour_or_string_takes_its_own_spelling() {
    let (mut ctx, registry, shape) = fixture();
    dispatch(
        &mut ctx,
        &registry,
        "data.set.property",
        value_args("Rounded", "TRUE"),
    )
    .expect("the boolean writes");
    assert_eq!(
        own_value(&ctx, shape, "Rounded"),
        Some(CellValue::Bool(true))
    );
    assert_eq!(own_formula(&ctx, shape, "Rounded").as_deref(), Some("TRUE"));

    dispatch(
        &mut ctx,
        &registry,
        "data.set.property",
        value_args("Tint", "#ff8000"),
    )
    .expect("the colour writes");
    assert_eq!(
        own_value(&ctx, shape, "Tint"),
        Some(CellValue::Color(Color::new(255, 128, 0)))
    );
    assert_eq!(own_formula(&ctx, shape, "Tint").as_deref(), Some("#FF8000"));

    dispatch(
        &mut ctx,
        &registry,
        "data.set.property",
        value_args("Note", "say \"hi\""),
    )
    .expect("the string writes");
    assert_eq!(
        own_value(&ctx, shape, "Note"),
        Some(CellValue::Str(String::from("say \"hi\"")))
    );
    assert_eq!(
        own_formula(&ctx, shape, "Note").as_deref(),
        Some("\"say \"\"hi\"\"\""),
        "the formula is the quoted literal"
    );
}

#[test]
fn every_refused_entry_names_why_and_changes_nothing() {
    let (mut ctx, registry, shape) = fixture();
    let before = ctx.active_document().expect("document").document.clone();
    for (row, value, wanted) in [
        ("Cost", "abc", "not a number"),
        ("Cost", "Width*2", "not a number"),
        ("Cost", "ABS(-2)", "not a number"),
        ("Cost", "1e999", "not a number"),
        ("Cost", "3 in +", "not a number"),
        ("Span", "1e308", "not a finite number"),
        ("Rounded", "maybe", "not TRUE or FALSE"),
        ("Tint", "red", "not a #RRGGBB colour"),
        ("Bag", "x", "not editable"),
        ("Bag", "", "not editable"),
        ("Empty", "x", "no value to edit"),
        ("Nope", "1", "no Shape Data row"),
    ] {
        let error = dispatch(
            &mut ctx,
            &registry,
            "data.set.property",
            value_args(row, value),
        )
        .expect_err("the entry is refused");
        let CommandError::InvalidArgs { message } = error else {
            panic!("expected InvalidArgs for {row}={value:?}, got {error:?}");
        };
        assert!(message.contains(wanted), "{row}={value:?}: {message}");
        assert_eq!(
            ctx.active_document().expect("document").document,
            before,
            "{row}={value:?} changes nothing"
        );
    }
    assert_eq!(
        own_value(&ctx, shape, "Cost"),
        Some(CellValue::Number(2.0)),
        "the value is the one before the refusals"
    );
}

#[test]
fn a_missing_argument_asks_for_it_and_an_empty_row_is_refused() {
    let (mut ctx, registry, _shape) = fixture();
    let error = dispatch(
        &mut ctx,
        &registry,
        "data.set.property",
        Args::new().with("value", ArgValue::Text(String::from("1"))),
    )
    .expect_err("row is missing");
    assert!(
        matches!(error, CommandError::NeedsInput(_)),
        "a missing `row` asks for input: {error:?}"
    );

    let error = dispatch(
        &mut ctx,
        &registry,
        "data.set.property",
        Args::new().with("row", ArgValue::Text(String::new())),
    )
    .expect_err("row is empty");
    assert!(
        matches!(error, CommandError::InvalidArgs { .. }),
        "an empty `row` is invalid: {error:?}"
    );

    let error = dispatch(
        &mut ctx,
        &registry,
        "data.set.property",
        Args::new().with("row", ArgValue::Text(String::from("Cost"))),
    )
    .expect_err("value is missing");
    assert!(
        matches!(error, CommandError::NeedsInput(_)),
        "a missing `value` asks for input: {error:?}"
    );
}

#[test]
fn an_inherited_row_edit_writes_a_local_row_and_undo_puts_it_back() {
    let (mut ctx, registry, shape) = fixture();
    assert_eq!(
        own_value(&ctx, shape, "Mastered"),
        None,
        "the row is inherited, not local"
    );
    dispatch(
        &mut ctx,
        &registry,
        "data.set.property",
        value_args("Mastered", "7"),
    )
    .expect("the edit runs");
    let value = own_value(&ctx, shape, "Mastered").expect("a local row now");
    assert_eq!(value, CellValue::Number(7.0));
    let session = ctx.active_document().expect("document");
    let master_rung = *session
        .document
        .master(MasterId { index: 0 })
        .expect("master")
        .shapes
        .first()
        .expect("rung");
    assert_eq!(
        session
            .document
            .property_rows(master_rung)
            .into_iter()
            .find(|entry| entry.key == "Mastered")
            .and_then(|entry| entry.row.cells.into_iter().next())
            .and_then(|cell| cell.value),
        Some(CellValue::Number(3.0)),
        "the master's own row is untouched"
    );

    // Undo removes the local row again: the row is the master's again.
    dispatch(&mut ctx, &registry, "edit.undo", Args::new()).expect("undo");
    assert_eq!(own_value(&ctx, shape, "Mastered"), None);
    let session = ctx.active_document().expect("document");
    let inherited = session
        .document
        .property_rows(shape)
        .into_iter()
        .find(|entry| entry.key == "Mastered")
        .expect("the inherited row is back");
    assert!(inherited.inherited);
    assert_eq!(
        inherited
            .row
            .cells
            .into_iter()
            .find(|cell| cell.name.as_str() == "Value")
            .and_then(|cell| cell.value),
        Some(CellValue::Number(3.0))
    );
}

#[test]
fn the_recalc_pass_follows_the_edit_like_every_other_cell_edit() {
    let (mut ctx, registry, shape) = fixture();
    // `PinX` computes from the Shape Data row: the edit's recalc pass
    // (ADR 0056) recomputes it, and undo puts the old value back.
    {
        let session = ctx.active_document_mut().expect("document");
        if let Some(entity) = session.document.shape_mut(shape) {
            let mut pin = Cell::new(CellName::from_static("PinX"));
            pin.formula = Some(String::from("Property.Cost*2"));
            pin.value = Some(CellValue::Number(4.0));
            entity.sheet.cells.push(pin);
        }
    }
    dispatch(
        &mut ctx,
        &registry,
        "data.set.property",
        value_args("Cost", "3"),
    )
    .expect("the edit runs");
    let session = ctx.active_document().expect("document");
    let pin = session
        .document
        .shape(shape)
        .expect("shape")
        .sheet
        .cells
        .iter()
        .find(|cell| cell.name.as_str() == "PinX")
        .expect("PinX");
    assert_eq!(
        pin.value,
        Some(CellValue::Number(6.0)),
        "the dependent recomputed: {pin:?}"
    );

    dispatch(&mut ctx, &registry, "edit.undo", Args::new()).expect("undo");
    let session = ctx.active_document().expect("document");
    let pin = session
        .document
        .shape(shape)
        .expect("shape")
        .sheet
        .cells
        .iter()
        .find(|cell| cell.name.as_str() == "PinX")
        .expect("PinX");
    assert_eq!(pin.value, Some(CellValue::Number(4.0)), "undo restored it");
}
