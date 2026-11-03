//! Text centring through the commands (task P7-45): a drawn rectangle
//! or ellipse centres its text like our masters, an instance's own
//! alignment wins over the master's, and the master's alignment cells
//! survive save and reopen. Every command runs through
//! [`support::dispatch`], the shell's path.

// The never-crash lints apply to non-test code only (clippy.toml).
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod support;

use support::dispatch;

use larrygram_commands::app::{AppCtx, DocumentSession, Settings};
use larrygram_commands::builtins::registry;
use larrygram_commands::{ArgValue, Args, Registry};
use larrygram_model::{CellLookup, CellName, Document, PageId, SheetRef, ValueSource};

/// The alignment the lookup resolves for `shape`: `VerticalAlign` and
/// the first `Paragraph` row's `HorzAlign`, with their sources.
fn alignment(
    ctx: &AppCtx,
    shape: larrygram_model::ShapeId,
) -> (
    Option<larrygram_model::CellValue>,
    Option<larrygram_model::CellValue>,
    ValueSource,
    ValueSource,
) {
    let session = ctx.active_document().expect("session");
    let document = &session.document;
    let vertical = document
        .resolve(shape, &CellName::new("VerticalAlign"))
        .expect("VerticalAlign resolves");
    let horz = document
        .resolve_row_cell(
            SheetRef::Shape(shape),
            "Paragraph",
            0,
            &CellName::new("HorzAlign"),
        )
        .expect("HorzAlign resolves");
    (
        vertical.value.cloned(),
        horz.value.cloned(),
        vertical.source,
        horz.source,
    )
}

/// A cached number, for the alignment assertions.
fn num(value: f64) -> Option<larrygram_model::CellValue> {
    Some(larrygram_model::CellValue::Number(value))
}

/// One drawing invocation at (1, 1), 2 x 1 inches.
fn drawing_args() -> Args {
    Args::new()
        .with("x", ArgValue::Number(1.0))
        .with("y", ArgValue::Number(1.0))
        .with("width", ArgValue::Number(2.0))
        .with("height", ArgValue::Number(1.0))
}

/// An app with one empty page and the builtin registry.
fn fixture() -> (AppCtx, Registry, PageId) {
    let mut document = Document::new();
    let page = document.add_page("Page-1").expect("page");
    let mut ctx = AppCtx::new(Settings::default());
    ctx.open_document(DocumentSession::new(document, None));
    let registry = registry().expect("registry");
    (ctx, registry, page)
}

/// The newest shape on `page`.
fn last_shape(ctx: &AppCtx, page: PageId) -> larrygram_model::ShapeId {
    let session = ctx.active_document().expect("session");
    *session
        .document
        .page(page)
        .expect("page")
        .shapes
        .last()
        .expect("a shape")
}

#[test]
fn drawn_rectangles_and_ellipses_centre_their_text() {
    // Task P7-45 goal 3: the same cells our masters carry.
    for id in ["insert.drawing.rectangle", "insert.drawing.ellipse"] {
        let (mut ctx, registry, page) = fixture();
        dispatch(&mut ctx, &registry, id, drawing_args()).expect("the drawing draws");
        let shape = last_shape(&ctx, page);
        let (vertical, horz, v_src, h_src) = alignment(&ctx, shape);
        assert_eq!(vertical, num(1.0), "{id}: VerticalAlign");
        assert_eq!(horz, num(1.0), "{id}: HorzAlign");
        assert_eq!(v_src, ValueSource::Local, "{id}: its own cell");
        assert_eq!(h_src, ValueSource::Local, "{id}: its own row");
    }
}

#[test]
fn the_line_and_the_text_box_keep_the_documented_defaults() {
    // Task P7-45 goal 3 pins the boundary: the text box waits for the
    // Visio check (P7-44 open issue 1) and the line tool is not in the
    // task's files, so neither sets the cells and both stay top-left.
    for id in ["insert.drawing.line", "insert.drawing.text_box"] {
        let (mut ctx, registry, page) = fixture();
        dispatch(&mut ctx, &registry, id, drawing_args()).expect("the drawing draws");
        let shape = last_shape(&ctx, page);
        let (vertical, horz, v_src, h_src) = alignment(&ctx, shape);
        assert_ne!(vertical, num(1.0), "{id}: VerticalAlign stays a default");
        assert_ne!(horz, num(1.0), "{id}: HorzAlign stays a default");
        assert_ne!(v_src, ValueSource::Local, "{id}: no own cell");
        assert_ne!(h_src, ValueSource::Local, "{id}: no own row");
    }
}

/// One Process drop at (4, 3).
fn process_args() -> Args {
    Args::new()
        .with("stencil", ArgValue::Text("Basic Flowchart".to_owned()))
        .with("master", ArgValue::Text("Process".to_owned()))
        .with("x", ArgValue::Number(4.0))
        .with("y", ArgValue::Number(3.0))
}

#[test]
fn local_alignment_wins_over_the_master_paragraph_row() {
    // Task P7-45 goal 5: drop Process, run `format.text.align_left`,
    // and the instance's own Paragraph row answers 0 — not the
    // master's 1.
    let (mut ctx, registry, page) = fixture();
    dispatch(&mut ctx, &registry, "insert.shape", process_args()).expect("the drop drops");
    let shape = last_shape(&ctx, page);
    let (_, horz, _, h_src) = alignment(&ctx, shape);
    assert_eq!(horz, num(1.0), "the master's row before the edit");
    assert!(
        matches!(h_src, ValueSource::Master(_)),
        "from the master rung"
    );
    // The formatting commands need text on the shape (the `ADD_TEXT`
    // predicate), so the shape gets its label first.
    let session = ctx.active_document().expect("session").id;
    {
        let session = ctx
            .documents
            .iter_mut()
            .find(|candidate| candidate.id == session)
            .expect("session");
        let entity = session.document.shape_mut(shape).expect("shape");
        entity.text = Some(larrygram_model::TextContent {
            parts: vec![larrygram_model::TextPart::Text("Label".to_owned())],
        });
    }
    ctx.selection.shapes = vec![shape];
    ctx.selection.anchor = Some(shape);
    ctx.selection.page = Some(page);
    dispatch(&mut ctx, &registry, "format.text.align_left", Args::new())
        .expect("the alignment writes");
    let (vertical, horz, v_src, h_src) = alignment(&ctx, shape);
    assert_eq!(horz, num(0.0), "the instance's own row wins");
    assert_eq!(h_src, ValueSource::Local, "from the instance's row");
    assert_eq!(
        vertical,
        num(1.0),
        "the master's VerticalAlign is untouched"
    );
    assert!(
        matches!(v_src, ValueSource::Master(_)),
        "still the master rung"
    );
}

#[test]
fn the_masters_alignment_survives_save_and_reopen() {
    // Task P7-45 goal 5: the master's cells are in the reopened file,
    // and an instance still resolves to 1/1 through the master rung.
    let (mut ctx, registry, page) = fixture();
    dispatch(&mut ctx, &registry, "insert.shape", process_args()).expect("the drop drops");
    let (vertical, horz, _, _) = alignment(&ctx, last_shape(&ctx, page));
    assert_eq!(
        (vertical, horz),
        (num(1.0), num(1.0)),
        "before the round trip"
    );
    let document = {
        let session = ctx.active_document().expect("session");
        session.document.clone()
    };
    let bytes =
        larrygram_vsdx::write_document(&document, &larrygram_vsdx::Package::new()).expect("write");
    let reopened = larrygram_vsdx::read_document(&bytes, &larrygram_vsdx::Limits::DEFAULT)
        .expect("the file reopens");

    // The master's own sheet carries the cells.
    let master = reopened
        .masters
        .iter()
        .find(|master| master.name_u == "Process")
        .expect("the Process master is in the file");
    let sheet = &reopened
        .shape(master.shapes.first().copied().expect("one shape"))
        .expect("the master shape")
        .sheet;
    let vertical = sheet
        .cells
        .iter()
        .find(|cell| cell.name.as_str() == "VerticalAlign")
        .expect("VerticalAlign is in the file");
    assert_eq!(
        vertical.value,
        Some(larrygram_model::CellValue::Number(1.0))
    );
    let paragraph = sheet
        .sections
        .iter()
        .find(|section| section.name.as_str() == "Paragraph")
        .expect("the Paragraph section is in the file");
    let horz = paragraph.rows[0]
        .cells
        .iter()
        .find(|cell| cell.name.as_str() == "HorzAlign")
        .expect("HorzAlign is in the file");
    assert_eq!(horz.value, Some(larrygram_model::CellValue::Number(1.0)));

    // The instance in the reopened file still resolves 1/1.
    let page = reopened.pages.first().expect("a page");
    let shape = page.shapes.first().copied().expect("an instance");
    let mut reader = larrygram_model::CellReader::new(&reopened);
    assert_eq!(
        reader
            .number(SheetRef::Shape(shape), &CellName::new("VerticalAlign"))
            .expect("VerticalAlign"),
        1.0
    );
    assert_eq!(
        reader
            .row_number(
                SheetRef::Shape(shape),
                "Paragraph",
                larrygram_model::RowTarget::at(0),
                &CellName::new("HorzAlign"),
            )
            .expect("HorzAlign"),
        1.0
    );
}
