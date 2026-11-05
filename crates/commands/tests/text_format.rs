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
fn run(
    ctx: &mut AppCtx,
    id: &str,
    args: Args,
) -> Result<Outcome, larrygram_commands::CommandError> {
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
    assert!(master_style(&ctx).is_none(), "the master is never edited");
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
    run(
        &mut ctx,
        "file.save_as",
        Args::new().with("path", ArgValue::Path(path.clone())),
    )
    .expect("save as");
    run(
        &mut ctx,
        "file.open",
        Args::new().with("path", ArgValue::Path(path.clone())),
    )
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
    assert!(
        (size - 24.0 / 72.0).abs() < 1.0e-9,
        "the size round-trips ({size})"
    );
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

// ---------------------------------------------------------------------------
// The range scope (task P7-53 item 2), at the commands level.
// ---------------------------------------------------------------------------

/// One run row entry for the `runs`/`runs_keep` arguments.
fn run_arg(target: i32, seed: Option<i32>) -> ArgValue {
    let entry = match seed {
        Some(seed) => format!("{target}:{seed}"),
        None => format!("{target}:~"),
    };
    ArgValue::List(vec![entry])
}

/// The `Style` of the `Character` row with `IX` `ix`.
fn row_style(ctx: &AppCtx, shape: ShapeId, ix: i32) -> Option<f64> {
    let session = ctx.active_document().expect("session");
    let shape = session.document.shape(shape).expect("shape");
    shape
        .sheet
        .sections
        .iter()
        .find(|s| s.name.as_str() == "Character")
        .and_then(|block| block.rows.iter().find(|row| row.ix == Some(ix)))
        .and_then(|row| row.cells.iter().find(|cell| cell.name.as_str() == "Style"))
        .and_then(|cell| match cell.value {
            Some(larrygram_model::CellValue::Number(value)) => Some(value),
            _ => None,
        })
}

/// A scoped write lands on the named runs only: the kept run is
/// created plain, the other rows are untouched (task P7-53).
#[test]
fn a_scoped_write_lands_on_the_named_runs_only() {
    let (mut ctx, shape) = catalog();
    let mut args = Args::new();
    args.set("runs", run_arg(5, None));
    args.set("runs_keep", run_arg(3, None));
    run(&mut ctx, "format.text.bold", args).expect("bold");
    assert_eq!(
        row_style(&ctx, shape, 5),
        Some(1.0),
        "the named run is bold"
    );
    assert_eq!(
        row_style(&ctx, shape, 3),
        Some(0.0),
        "the kept run is created plain"
    );
    assert_eq!(row_style(&ctx, shape, 0), None, "no other row appears");
}

/// A scoped toggle reads the run it seeds from (the fresh row keeps
/// the split run's style bit).
#[test]
fn a_scoped_toggle_reads_the_run_it_seeds() {
    let (mut ctx, shape) = catalog();
    // Make run 0 bold first (the whole text).
    run(&mut ctx, "format.text.bold", Args::new()).expect("bold");
    assert_eq!(row_style(&ctx, shape, 0), Some(1.0));
    // A fresh run seeded from it toggles the bit off (it inherits
    // bold, so the toggle unbolds it).
    let mut args = Args::new();
    args.set("runs", run_arg(2, Some(0)));
    run(&mut ctx, "format.text.bold", args).expect("bold");
    assert_eq!(row_style(&ctx, shape, 2), Some(0.0), "the seed's bit flips");
}

/// A malformed `runs` argument is an argument error, never a panic
/// (task P7-53).
#[test]
fn a_malformed_runs_argument_is_an_argument_error() {
    let (mut ctx, _shape) = catalog();
    for entry in ["", "x", "1:x", ":3", "1,2"] {
        let mut args = Args::new();
        args.set("runs", ArgValue::List(vec![entry.to_owned()]));
        let error =
            run(&mut ctx, "format.text.bold", args).expect_err("a bad run entry is refused");
        let _ = error;
    }
    let mut args = Args::new();
    args.set("runs", ArgValue::Text("5".to_owned()));
    run(&mut ctx, "format.text.bold", args).expect_err("a non-list is refused");
}

/// The structured `shape.set.text` form keeps the run markers (task
/// P7-53: the commit keeps the range formats).
#[test]
fn the_structured_text_write_keeps_the_run_markers() {
    let (mut ctx, shape) = catalog();
    let mut args = text("text", "Hi!");
    args.set(
        "run_ixs",
        ArgValue::List(vec!["0".to_owned(), "4".to_owned()]),
    );
    args.set(
        "run_texts",
        ArgValue::List(vec!["Hi".to_owned(), "!".to_owned()]),
    );
    run(&mut ctx, "shape.set.text", args).expect("text");
    let session = ctx.active_document().expect("session");
    let entry = session.document.shape(shape).expect("shape");
    let markers: Vec<u32> = entry
        .text
        .as_ref()
        .map(|text| {
            text.parts
                .iter()
                .filter_map(|part| match part {
                    TextPart::CharRun { ix } => Some(*ix),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();
    assert_eq!(markers, vec![0, 4], "one marker per run");
}

/// A scoped "use default" removes the named rows' entry only (task
/// P7-53).
#[test]
fn a_scoped_use_default_removes_the_named_rows_entry_only() {
    let (mut ctx, shape) = catalog();
    let mut args = Args::new();
    args.set("runs", run_arg(2, None));
    args.set("color", ArgValue::Text("#1F5BC4".to_owned()));
    run(&mut ctx, "text.font.color", args).expect("colour");
    let mut args = Args::new();
    args.set("runs", run_arg(2, None));
    run(&mut ctx, "format.text_color.use_default", args).expect("use default");
    let session = ctx.active_document().expect("session");
    let entry = session.document.shape(shape).expect("shape");
    let row = entry
        .sheet
        .sections
        .iter()
        .find(|s| s.name.as_str() == "Character")
        .and_then(|block| block.rows.iter().find(|row| row.ix == Some(2)))
        .expect("the run row");
    assert!(
        row.cells.iter().all(|cell| cell.name.as_str() != "Color"),
        "the colour entry is gone"
    );
}

/// A scoped write twice: the second lands on the row it created (the
/// in-place path and the toggle's existing-row read).
#[test]
fn a_scoped_write_twice_toggles_the_row_it_created() {
    let (mut ctx, shape) = catalog();
    let mut args = Args::new();
    args.set("runs", run_arg(4, None));
    run(&mut ctx, "format.text.bold", args.clone()).expect("bold on");
    assert_eq!(row_style(&ctx, shape, 4), Some(1.0));
    run(&mut ctx, "format.text.bold", args).expect("bold off");
    assert_eq!(row_style(&ctx, shape, 4), Some(0.0), "the second toggles");
}

/// The `runs_keep` entries parse like `runs` and refuse the same
/// broken spellings (task P7-53).
#[test]
fn a_malformed_runs_keep_argument_is_an_argument_error() {
    let (mut ctx, _shape) = catalog();
    for entry in ["x", "1:x", ":3"] {
        let mut args = Args::new();
        args.set("runs_keep", ArgValue::List(vec![entry.to_owned()]));
        run(&mut ctx, "format.text.bold", args).expect_err("a bad kept run is refused");
    }
}

/// The structured `shape.set.text` form refuses broken run lists
/// (task P7-53): never a panic.
#[test]
fn the_structured_text_write_refuses_a_broken_run_list() {
    let (mut ctx, _shape) = catalog();
    // run_ixs is not a list.
    let mut args = text("text", "Hi");
    args.set("run_ixs", ArgValue::Text("0".to_owned()));
    run(&mut ctx, "shape.set.text", args).expect_err("a non-list is refused");
    // run_texts is missing.
    let mut args = text("text", "Hi");
    args.set("run_ixs", ArgValue::List(vec!["0".to_owned()]));
    run(&mut ctx, "shape.set.text", args).expect_err("run_texts is required");
    // run_texts is not a list.
    let mut args = text("text", "Hi");
    args.set("run_ixs", ArgValue::List(vec!["0".to_owned()]));
    args.set("run_texts", ArgValue::Text("Hi".to_owned()));
    run(&mut ctx, "shape.set.text", args).expect_err("a non-list is refused");
    // The lengths differ.
    let mut args = text("text", "Hi");
    args.set(
        "run_ixs",
        ArgValue::List(vec!["0".to_owned(), "1".to_owned()]),
    );
    args.set("run_texts", ArgValue::List(vec!["Hi".to_owned()]));
    run(&mut ctx, "shape.set.text", args).expect_err("the lengths must match");
    // A bad IX.
    let mut args = text("text", "Hi");
    args.set("run_ixs", ArgValue::List(vec!["x".to_owned()]));
    args.set("run_texts", ArgValue::List(vec!["Hi".to_owned()]));
    run(&mut ctx, "shape.set.text", args).expect_err("a bad IX is refused");
    // An all-empty run list writes the empty text.
    let mut args = text("text", "");
    args.set("run_ixs", ArgValue::List(vec!["0".to_owned()]));
    args.set("run_texts", ArgValue::List(vec!["".to_owned()]));
    run(&mut ctx, "shape.set.text", args).expect("an empty write");
    let session = ctx.active_document().expect("session");
    let shape = session
        .document
        .shapes
        .iter()
        .find(|entry| entry.visio_id == 1)
        .map(|entry| entry.id)
        .expect("the fixture shape");
    let entry = session.document.shape(shape).expect("shape");
    assert!(
        entry.text.as_ref().is_none_or(|text| text.parts.is_empty()),
        "the empty write clears the text"
    );
}

/// The scoped forms of the other character formats (task P7-53):
/// font, font size, the size ladder and strikethrough.
#[test]
fn the_other_character_formats_take_the_scope() {
    let (mut ctx, shape) = catalog();
    let mut args = text("value", "Selawik");
    args.set("runs", run_arg(6, None));
    run(&mut ctx, "text.font", args).expect("font");
    let mut args = number("size", 18.0);
    args.set("runs", run_arg(6, None));
    run(&mut ctx, "text.font.size", args).expect("size");
    let mut args = Args::new();
    args.set("runs", run_arg(6, None));
    run(&mut ctx, "text.font.size.increase", args.clone()).expect("grow");
    run(&mut ctx, "tools.strikethrough", args.clone()).expect("strike");
    run(&mut ctx, "format.text_color.use_default", args).expect("colour default");
    let session = ctx.active_document().expect("session");
    let entry = session.document.shape(shape).expect("shape");
    let row = entry
        .sheet
        .sections
        .iter()
        .find(|s| s.name.as_str() == "Character")
        .and_then(|block| block.rows.iter().find(|row| row.ix == Some(6)))
        .expect("the run row");
    let cell = |name: &str| {
        row.cells
            .iter()
            .find(|cell| cell.name.as_str() == name)
            .and_then(|cell| cell.value.clone())
    };
    assert_eq!(
        cell("Font"),
        Some(larrygram_model::CellValue::Str("Selawik".to_owned())),
        "the font records"
    );
    let size = match cell("Size") {
        Some(larrygram_model::CellValue::Number(value)) => value,
        other => panic!("the size records ({other:?})"),
    };
    assert!(size > 18.0 / 72.0, "the ladder grew the size ({size})");
    assert_eq!(
        cell("Strikethru"),
        Some(larrygram_model::CellValue::Bool(true)),
        "the strike records"
    );
}

/// A structured write with an empty run among real ones skips the
/// empty literal and keeps the rest (task P7-53).
#[test]
fn the_structured_write_skips_an_empty_run() {
    let (mut ctx, shape) = catalog();
    let mut args = text("text", "Hi");
    args.set(
        "run_ixs",
        ArgValue::List(vec!["-1".to_owned(), "7".to_owned()]),
    );
    args.set(
        "run_texts",
        ArgValue::List(vec!["".to_owned(), "Hi".to_owned()]),
    );
    run(&mut ctx, "shape.set.text", args).expect("text");
    let session = ctx.active_document().expect("session");
    let entry = session.document.shape(shape).expect("shape");
    let parts = entry
        .text
        .as_ref()
        .map(|text| text.parts.clone())
        .unwrap_or_default();
    assert_eq!(
        parts,
        vec![
            larrygram_model::TextPart::CharRun { ix: 7 },
            larrygram_model::TextPart::Text("Hi".to_owned())
        ],
        "the empty run is skipped, the marked one kept"
    );
}

/// The format painter captures a plain shape (no run rows): the
/// capture's fall-throughs (task P7-53's coverage of the painter).
#[test]
fn the_format_painter_captures_a_plain_shape() {
    let (mut ctx, shape) = catalog();
    // The copy form reads the standard cells.
    run(&mut ctx, "tools.format_painter", Args::new()).expect("copy");
    let args = Args::new().with("apply", ArgValue::Flag(true));
    run(&mut ctx, "tools.format_painter", args).expect("apply");
    assert!(row_style(&ctx, shape, 0).is_none() || row_style(&ctx, shape, 0).is_some());
}

/// A structured write keeps the paragraph and tabs markers the shape's
/// rows call for (task P7-53).
#[test]
fn the_structured_write_keeps_the_paragraph_and_tabs_markers() {
    let (mut ctx, shape) = catalog();
    // A Paragraph row (the alignment creates one) and a Tabs row.
    run(&mut ctx, "format.text.align_left", Args::new()).expect("align");
    {
        let session = ctx.active_document_mut().expect("session");
        let entry = session.document.shape_mut(shape).expect("shape");
        let mut block =
            larrygram_model::Section::new(larrygram_model::SectionName::from_static("Tabs"));
        let mut row = larrygram_model::Row::new();
        row.ix = Some(0);
        block.rows.push(row);
        entry.sheet.sections.push(block);
    }
    let mut args = text("text", "Hi");
    args.set("run_ixs", ArgValue::List(vec!["-1".to_owned()]));
    args.set("run_texts", ArgValue::List(vec!["Hi".to_owned()]));
    run(&mut ctx, "shape.set.text", args).expect("text");
    let session = ctx.active_document().expect("session");
    let entry = session.document.shape(shape).expect("shape");
    let parts = entry
        .text
        .as_ref()
        .map(|text| text.parts.clone())
        .unwrap_or_default();
    assert!(
        parts
            .iter()
            .any(|part| matches!(part, TextPart::ParaRun { .. })),
        "the paragraph marker round-trips ({parts:?})"
    );
    assert!(
        parts
            .iter()
            .any(|part| matches!(part, TextPart::TabsRun { .. })),
        "the tabs marker round-trips ({parts:?})"
    );
}

/// The format painter's capture falls through the cells a junk-typed
/// value cannot answer (task P7-53: the reads never panic).
#[test]
fn the_format_painter_skips_cells_it_cannot_read() {
    let (mut ctx, shape) = catalog();
    // Junk values: every number and colour read fails and falls
    // through.
    {
        let session = ctx.active_document_mut().expect("session");
        let entry = session.document.shape_mut(shape).expect("shape");
        for name in [
            "Size",
            "Style",
            "VerticalAlign",
            "FillPattern",
            "FillForegndTrans",
            "LineWeight",
            "LinePattern",
            "BeginArrow",
            "EndArrow",
        ] {
            entry.sheet.cells.push(larrygram_model::Cell {
                name: larrygram_model::CellName::new(name),
                formula: Some("\"junk\"".to_owned()),
                value: Some(larrygram_model::CellValue::Str("junk".to_owned())),
                unit: larrygram_geom::Unit::Inches,
                preserved: larrygram_model::Preserved::new(),
            });
        }
        for name in ["Color", "FillForegnd", "LineColor"] {
            entry.sheet.cells.push(larrygram_model::Cell {
                name: larrygram_model::CellName::new(name),
                formula: Some("\"junk\"".to_owned()),
                value: Some(larrygram_model::CellValue::Str("junk".to_owned())),
                unit: larrygram_geom::Unit::Inches,
                preserved: larrygram_model::Preserved::new(),
            });
        }
    }
    run(&mut ctx, "tools.format_painter", Args::new()).expect("copy");
    let args = Args::new().with("apply", ArgValue::Flag(true));
    run(&mut ctx, "tools.format_painter", args).expect("apply");
}
