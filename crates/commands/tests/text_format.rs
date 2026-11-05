//! Task P7-53: text formatting reaches a shape's text, at the
//! commands level: the writes land on local `Character`/`Paragraph`
//! rows (ADR 0013's inheritance rule: an instance overrides its
//! master, the master is never edited), and a formatted shape keeps
//! its format through `file.save_as` and `file.open` (item 3).

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use larrygram_commands::app::{AppCtx, Settings};
use larrygram_commands::{ArgValue, Args, Outcome};
use larrygram_model::{ShapeId, TextPart};

mod support;
use support::dispatch;

/// The builtin registry.
fn registry() -> larrygram_commands::Registry {
    larrygram_commands::builtins::registry().expect("the registry builds")
}

/// One command with args.
fn run(ctx: &mut AppCtx, id: &str, args: Args) -> Result<Outcome, larrygram_commands::CommandError> {
    dispatch(ctx, &registry(), id, args)
}

/// One text argument.
fn text(name: &str, value: &str) -> Args {
    Args::new().with(name, ArgValue::Text(value.to_owned()))
}

/// One number argument.
fn number(name: &str, value: f64) -> Args {
    Args::new().with(name, ArgValue::Number(value))
}

/// A blank drawing with one Basic Shapes Rectangle dropped at (3, 3)
/// and its text set: the acceptance criteria's catalog shape.
fn catalog() -> (AppCtx, ShapeId) {
    let mut ctx = AppCtx::new(Settings::default());
    run(&mut ctx, "file.new.blank.drawing", Args::new()).expect("new drawing");
    let mut args = text("stencil", "Basic Shapes");
    args.set("master", ArgValue::Text("Rectangle".to_owned()));
    args.set("x", ArgValue::Number(3.0));
    args.set("y", ArgValue::Number(3.0));
    run(&mut ctx, "insert.shape", args).expect("insert");
    let shape = {
        let session = ctx.active_document().expect("session");
        *session
            .document
            .page(session.active_page.expect("page"))
            .expect("page")
            .shapes
            .last()
            .expect("a shape")
    };
    run(&mut ctx, "shape.set.text", text("text", "Hello")).expect("text");
    (ctx, shape)
}

/// The number of one `section` row cell of `shape` (row `row`).
fn row_number(ctx: &AppCtx, shape: ShapeId, section: &str, name: &str, row: usize) -> Option<f64> {
    let session = ctx.active_document().expect("session");
    let shape = session.document.shape(shape).expect("shape");
    shape
        .sheet
        .sections
        .iter()
        .find(|s| s.name.as_str() == section)
        .and_then(|block| block.rows.get(row))
        .and_then(|row| row.cells.iter().find(|cell| cell.name.as_str() == name))
        .and_then(|cell| match cell.value {
            Some(larrygram_model::CellValue::Number(value)) => Some(value),
            _ => None,
        })
}

/// The master's own `Character` `Style` cell, to prove a format never
/// edits the master (ADR 0013).
fn master_style(ctx: &AppCtx) -> Option<f64> {
    let session = ctx.active_document().expect("session");
    let document = &session.document;
    let shape = document.masters.first()?.shapes.first().copied()?;
    let entry = document.shape(shape)?;
    entry
        .sheet
        .sections
        .iter()
        .find(|s| s.name.as_str() == "Character")
        .and_then(|block| block.rows.first())
        .and_then(|row| row.cells.iter().find(|cell| cell.name.as_str() == "Style"))
        .and_then(|cell| match cell.value {
            Some(larrygram_model::CellValue::Number(value)) => Some(value),
            _ => None,
        })
}

/// The rendered runs of `shape` (`None` when it draws no text): the
/// bold face flag of every glyph run and the first glyph's size in
/// points, as the picture sees them.
fn drawn(ctx: &AppCtx, shape: ShapeId) -> Option<(Vec<bool>, f64)> {
    let session = ctx.active_document().expect("session");
    let page = session.active_page.expect("page");
    let scene = larrygram_render::build_scene(
        &session.document,
        page,
        &larrygram_render::BuildOptions::default(),
    )
    .expect("scene");
    let mut stack: Vec<&larrygram_render::scene::Node> = scene.nodes.iter().collect();
    while let Some(node) = stack.pop() {
        match node {
            larrygram_render::scene::Node::Shape(body) => {
                if body.shape_id == Some(shape)
                    && let Some(text) = body.text.as_ref()
                {
                    let bold = text
                        .layout
                        .lines
                        .iter()
                        .flat_map(|line| line.glyphs.iter())
                        .map(|glyph| text.fonts[glyph.font].bold())
                        .collect();
                    let size = text
                        .layout
                        .lines
                        .first()
                        .and_then(|line| line.glyphs.first())
                        .map(|glyph| glyph.size)?;
                    return Some((bold, size));
                }
            }
            larrygram_render::scene::Node::Group(group) => {
                stack.extend(group.children.iter());
            }
            _ => {}
        }
    }
    None
}

/// A format writes a local `Character` row on the instance and never
/// touches the master (the master note's inheritance rule).
#[test]
fn a_format_writes_a_local_row_and_never_the_master() {
    let (mut ctx, shape) = catalog();
    assert!(
        master_style(&ctx).is_none(),
        "the catalog master carries no Style cell to begin with"
    );
    run(&mut ctx, "format.text.bold", Args::new()).expect("bold");
    assert_eq!(
        row_number(&ctx, shape, "Character", "Style", 0),
        Some(1.0),
        "the write lands on the instance's own Character row"
    );
    assert!(
        master_style(&ctx).is_none(),
        "the master is never edited"
    );
}

/// A format changes the drawn text: the scene shows the bold face and
/// the new size (item 1's picture rule at the scene level).
#[test]
fn a_format_reaches_the_drawn_text() {
    let (mut ctx, shape) = catalog();
    let (bold, size) = drawn(&ctx, shape).expect("the text draws");
    assert!(
        bold.iter().all(|bold| !bold),
        "the plain text starts unbold ({bold:?})"
    );
    assert!((size - 12.0).abs() < 1.0e-9, "the plain size is 12 pt");
    run(&mut ctx, "format.text.bold", Args::new()).expect("bold");
    run(&mut ctx, "text.font.size", number("size", 24.0)).expect("size");
    let (bold, size) = drawn(&ctx, shape).expect("the text draws");
    assert!(
        bold.iter().all(|bold| *bold),
        "the drawn text is bold ({bold:?})"
    );
    assert!((size - 24.0).abs() < 1.0e-9, "the drawn size is 24 pt");
}

/// A formatted shape keeps its format through `file.save_as` and
/// `file.open` (item 3).
#[test]
fn a_formatted_shape_keeps_its_format_through_save_and_reopen() {
    let (mut ctx, _shape) = catalog();
    run(&mut ctx, "format.text.bold", Args::new()).expect("bold");
    run(&mut ctx, "text.font.size", number("size", 24.0)).expect("size");
    run(&mut ctx, "text.font.color", text("value", "#1F5BC4")).expect("colour");
    run(&mut ctx, "format.text.align_left", Args::new()).expect("align");

    let dir = std::env::temp_dir().join(format!("larrygram-p7-53-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("the temp dir");
    let path = dir.join("formatted.vsdx");
    run(&mut ctx, "file.save_as", Args::new().with("path", ArgValue::Path(path.clone())))
        .expect("save as");
    run(&mut ctx, "file.open", Args::new().with("path", ArgValue::Path(path.clone())))
        .expect("open");

    // The reopened shape keeps its text and its rows, and the drawn
    // text keeps the format.
    let session = ctx.active_document().expect("session");
    let reopened = session
        .document
        .shapes
        .iter()
        .find(|entry| entry.text.is_some())
        .map(|entry| entry.id)
        .expect("a shape with text");
    let entry = session.document.shape(reopened).expect("shape");
    let literal: String = entry
        .text
        .as_ref()
        .map(|text| {
            text.parts
                .iter()
                .filter_map(|part| match part {
                    TextPart::Text(part) => Some(part.as_str()),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();
    assert_eq!(literal, "Hello", "the text round-trips");
    assert_eq!(
        entry.text.as_ref().map(|text| text.parts.clone()),
        Some(vec![TextPart::Text("Hello".to_owned())]),
        "the text round-trips unchanged (a text written before its rows has no run marker; the run reads row 0)"
    );
    assert_eq!(
        row_number(&ctx, reopened, "Character", "Style", 0),
        Some(1.0),
        "the bold bit round-trips"
    );
    let size = row_number(&ctx, reopened, "Character", "Size", 0).expect("the Size cell");
    assert!((size - 24.0 / 72.0).abs() < 1.0e-9, "the size round-trips ({size})");
    assert_eq!(
        row_number(&ctx, reopened, "Paragraph", "HorzAlign", 0),
        Some(0.0),
        "the alignment round-trips"
    );
    let (bold, drawn_size) = drawn(&ctx, reopened).expect("the reopened text draws");
    assert!(bold.iter().all(|bold| *bold), "the drawn text stays bold");
    assert!(
        (drawn_size - 24.0).abs() < 1.0e-9,
        "the drawn size stays 24 pt"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
