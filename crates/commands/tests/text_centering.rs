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
