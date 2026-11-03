//! Unsaved changes (task P7-42, ADR 0040 amendment 2026-10-25): the
//! one rule for "changed since save" and the `file.close` answers.
//!
//! The contract under test:
//!
//! - a session is changed when its change counter differs from the
//!   counter recorded at the last open or save
//!   ([`DocumentSession::is_dirty`]); every edit, undo and redo moves
//!   the counter, so undoing back to the saved state still reads as
//!   changed and a cancelled undo group restores what it saw;
//! - `file.close` on a changed drawing asks on interactive sources
//!   (`UiRequest::ConfirmClose`) and closes without asking on
//!   programmatic ones; `confirm` closes without asking and `save`
//!   saves first and closes only when the save succeeds.

// The never-crash lints apply to non-test code only (clippy.toml).
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use larrygram_commands::app::{AppCtx, DocumentSession, Settings};
use larrygram_commands::builtins::registry;
use larrygram_commands::{
    ArgValue, Args, CommandError, CommandId, CommandStatus, Invocation, Outcome, SessionId, Source,
    UiRequest,
};
use larrygram_model::{
    Cell, CellName, CellValue, Document, PageId, Row, Section, SectionName, Shape, ShapeId,
};
use larrygram_vsdx::{from_model, write_package};

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// A private temp directory for one test.
fn temp_dir() -> PathBuf {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let name = format!(
        "larrygram-unsaved-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    let dir = std::env::temp_dir().join(name);
    std::fs::create_dir_all(&dir).expect("temp directory");
    dir
}

fn number_cell(name: &str, value: f64) -> Cell {
    let mut cell = Cell::new(CellName::new(name));
    cell.value = Some(CellValue::Number(value));
    cell.unit = larrygram_geom::Unit::Inches;
    cell
}

/// A state with one page and `count` shapes of 2x1 inches at (n, 0),
/// each with its own geometry rows (the `editing` fixture shape).
fn fixture(count: usize) -> (AppCtx, SessionId, PageId, Vec<ShapeId>) {
    let mut doc = Document::new();
    let page = doc.add_page("Page-1").expect("page");
    let mut shapes = Vec::new();
    for n in 0..count {
        let mut shape = Shape::new(ShapeId { index: 0 }, i32::try_from(n).unwrap_or(0) + 1);
        for (name, value) in [
            ("PinX", n as f64),
            ("PinY", 0.0),
            ("Width", 2.0),
            ("Height", 1.0),
            ("Angle", 0.0),
        ] {
            shape.sheet.cells.push(number_cell(name, value));
        }
        let mut geometry = Section::new(SectionName::from_static("Geometry"));
        for (ix, (kind, x, y)) in [
            ("MoveTo", 0.0, 0.0),
            ("LineTo", 2.0, 0.0),
            ("LineTo", 2.0, 1.0),
        ]
        .into_iter()
        .enumerate()
        {
            let mut row = Row::new();
            row.row_type = Some(kind.to_owned());
            row.ix = Some(i32::try_from(ix).unwrap_or(0) + 1);
            row.cells.push(number_cell("X", x));
            row.cells.push(number_cell("Y", y));
            geometry.rows.push(row);
        }
        shape.sheet.sections.push(geometry);
        shapes.push(doc.insert_shape(page, shape, None).expect("insert"));
    }
    let mut ctx = AppCtx::new(Settings::default());
    let index = ctx.open_document(DocumentSession::new(doc, None));
    let session = ctx.documents.get(index).expect("session").id;
    (ctx, session, page, shapes)
}

/// Selects `shapes` as a click sequence (the last one is the anchor).
fn select(ctx: &mut AppCtx, page: PageId, shapes: &[ShapeId]) {
    ctx.selection.shapes = shapes.to_vec();
    ctx.selection.anchor = shapes.last().copied();
    ctx.selection.page = Some(page);
}

fn number_args(pairs: &[(&str, f64)]) -> Args {
    let mut args = Args::new();
    for (name, value) in pairs {
        args.set(*name, ArgValue::Number(*value));
    }
    args
}

/// Dispatches one command with an explicit source, through the one
/// push seam (the undo-step recording of `Outcome::edit`).
fn run_from(
    ctx: &mut AppCtx,
    id: &str,
    args: Args,
    source: Source,
) -> Result<Outcome, CommandError> {
    let registry = registry().expect("registry");
    larrygram_commands::dispatch(
        &registry,
        ctx,
        &CommandId::intern(id).expect("id"),
        Invocation::new(args, source),
    )
}

/// Dispatches one command with `Source::Test`.
fn run(ctx: &mut AppCtx, id: &str, args: Args) -> Result<Outcome, CommandError> {
    run_from(ctx, id, args, Source::Test)
}

/// Whether the active document reads as changed.
fn dirty(ctx: &AppCtx) -> bool {
    ctx.active_document().expect("session").is_dirty()
}

/// The bytes of a one-page document as a `.vsdx` package.
fn package_bytes() -> Vec<u8> {
    let mut doc = Document::new();
    doc.add_page("Page-1").expect("page");
    let package = from_model(&doc).expect("package");
    write_package(&package).expect("bytes")
}

// ---------------------------------------------------------------------------
// Item 1: the one rule for "changed since save".
// ---------------------------------------------------------------------------

/// A fresh `file.new` drawing and a just-opened file both start
/// unchanged (task P7-42 goal 1).
#[test]
fn a_new_drawing_and_an_opened_one_start_unchanged() {
    let mut ctx = AppCtx::new(Settings::default());
    run(&mut ctx, "file.new", Args::new()).expect("file.new");
    assert!(!dirty(&ctx), "a fresh file.new drawing is not changed");

    let mut ctx = AppCtx::new(Settings::default());
    run(
        &mut ctx,
        "file.open",
        Args::new()
            .with("name", ArgValue::Text("probe.vsdx".to_owned()))
            .with("data", ArgValue::Bytes(package_bytes())),
    )
    .expect("file.open");
    assert!(!dirty(&ctx), "an opened drawing is not changed");
}

/// Every kind of edit moves the change counter, so the drawing reads
/// as changed (task P7-42 goal 1: a cell write, a structural step and
/// a composite step).
#[test]
fn every_kind_of_edit_marks_the_drawing_changed() {
    // A cell write: `shape.set.width` (one `CellEdit`).
    let (mut ctx, _session, page, shapes) = fixture(1);
    select(&mut ctx, page, &shapes);
    assert!(!dirty(&ctx));
    run(&mut ctx, "shape.set.width", number_args(&[("width", 3.0)]))
        .expect("set.width runs");
    assert!(dirty(&ctx), "a cell write marks the drawing changed");

    // A structural step: `edit.delete` (a `StructuralEdit`).
    let (mut ctx, _session, page, shapes) = fixture(1);
    select(&mut ctx, page, &shapes);
    run(&mut ctx, "edit.delete", Args::new()).expect("delete runs");
    assert!(dirty(&ctx), "a delete marks the drawing changed");

    // A composite step: `shape.move` (a `CompositeEdit`).
    let (mut ctx, _session, page, shapes) = fixture(1);
    select(&mut ctx, page, &shapes);
    run(&mut ctx, "shape.move", number_args(&[("dx", 1.0), ("dy", 0.0)]))
        .expect("move runs");
    assert!(dirty(&ctx), "a move marks the drawing changed");
}

/// Undo and redo both move the counter: after either the drawing
/// still reads as changed, even when the undo returned it to the
/// saved state (task P7-42 goal 1: simple and safe).
#[test]
fn undo_and_redo_keep_the_drawing_changed() {
    let (mut ctx, _session, page, shapes) = fixture(1);
    select(&mut ctx, page, &shapes);
    run(&mut ctx, "shape.set.width", number_args(&[("width", 3.0)]))
        .expect("set.width runs");
    assert!(dirty(&ctx));

    run(&mut ctx, "edit.undo", Args::new()).expect("undo runs");
    assert!(dirty(&ctx), "undoing back to the saved state still reads as changed");

    run(&mut ctx, "edit.redo", Args::new()).expect("redo runs");
    assert!(dirty(&ctx), "redo reads as changed");
}

/// Save and Save As record the saved state (task P7-42 goal 1).
#[test]
fn save_and_save_as_clear_the_changed_state() {
    let dir = temp_dir();
    let target = dir.join("out.vsdx");

    // `file.save_as` writes the target and records it as the save.
    let (mut ctx, _session, page, shapes) = fixture(1);
    select(&mut ctx, page, &shapes);
    run(&mut ctx, "shape.set.width", number_args(&[("width", 3.0)]))
        .expect("set.width runs");
    run(
        &mut ctx,
        "file.save_as",
        Args::new().with("path", ArgValue::Path(target.clone())),
    )
    .expect("save_as runs");
    assert!(!dirty(&ctx), "save_as leaves the drawing unchanged");
    assert!(target.is_file(), "the file was written");

    // An edit marks it changed again; `file.save` writes the session
    // path and records the save.
    run(&mut ctx, "shape.set.width", number_args(&[("width", 4.0)]))
        .expect("set.width runs");
    assert!(dirty(&ctx));
    run(&mut ctx, "file.save", Args::new()).expect("save runs");
    assert!(!dirty(&ctx), "save leaves the drawing unchanged");
    std::fs::remove_dir_all(&dir).expect("clean up");
}

/// A cancelled undo group restores the unsaved-changes state it saw
/// (task P7-42 goal 1): a cancelled empty text box leaves the drawing
/// unchanged, and a group that saw a changed drawing leaves it
/// changed. A programmatic source ends an open group first (ADR 0040),
/// so the commands inside the group run from an interactive source
/// like the shell's.
#[test]
fn a_cancelled_undo_group_leaves_the_drawing_unchanged() {
    let (mut ctx, session, page, shapes) = fixture(1);
    select(&mut ctx, page, &shapes);
    ctx.begin_group(session, "Insert Text Box").expect("begin");
    run_from(
        &mut ctx,
        "shape.set.width",
        number_args(&[("width", 3.0)]),
        Source::Ui,
    )
    .expect("set.width runs");
    assert!(dirty(&ctx), "the group's edits count while it runs");
    ctx.cancel_group(session).expect("cancel");
    assert!(!dirty(&ctx), "a cancelled group leaves the drawing unchanged");

    // A group that saw a changed drawing restores "changed".
    run(&mut ctx, "shape.set.width", number_args(&[("width", 5.0)]))
        .expect("set.width runs");
    ctx.begin_group(session, "Insert Text Box").expect("begin");
    run_from(
        &mut ctx,
        "shape.set.width",
        number_args(&[("width", 6.0)]),
        Source::Ui,
    )
    .expect("set.width runs");
    ctx.cancel_group(session).expect("cancel");
    assert!(dirty(&ctx), "the state the group saw is restored");

    // And the drawing really is as it was before the cancelled steps.
    let width = width_of(&ctx, shapes[0]);
    assert_eq!(width, 5.0, "both cancels reverted their step");
}

/// The cached `Width` of `shape`.
fn width_of(ctx: &AppCtx, shape: ShapeId) -> f64 {
    match ctx
        .active_document()
        .expect("session")
        .document
        .shape(shape)
        .expect("shape")
        .sheet
        .cells
        .iter()
        .find(|cell| cell.name.as_str() == "Width")
        .and_then(|cell| cell.value.clone())
    {
        Some(CellValue::Number(value)) => value,
        other => panic!("Width is a number, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Item 2: `file.close` on a changed drawing.
// ---------------------------------------------------------------------------

/// A changed drawing asks on every interactive source and closes
/// without asking on every programmatic one (task P7-42 goal 2).
#[test]
fn file_close_asks_interactive_sources_and_closes_programmatic_ones() {
    for source in [
        Source::Ui,
        Source::CommandBar,
        Source::Menu,
        Source::Shortcut,
    ] {
        let (mut ctx, _session, page, shapes) = fixture(1);
        select(&mut ctx, page, &shapes);
        run(&mut ctx, "shape.set.width", number_args(&[("width", 3.0)]))
            .expect("set.width runs");
        let outcome = run_from(&mut ctx, "file.close", Args::new(), source).expect("close");
        match &outcome.ui_request {
            Some(UiRequest::ConfirmClose { name }) => assert_eq!(name, "Untitled", "{source:?}"),
            other => panic!("{source:?}: expected ConfirmClose, got {other:?}"),
        }
        assert!(ctx.active_document().is_some(), "{source:?}: nothing closed");
    }

    for source in [Source::Cli, Source::Control, Source::Mcp, Source::Test] {
        let (mut ctx, _session, page, shapes) = fixture(1);
        select(&mut ctx, page, &shapes);
        run(&mut ctx, "shape.set.width", number_args(&[("width", 3.0)]))
            .expect("set.width runs");
        let outcome = run_from(&mut ctx, "file.close", Args::new(), source).expect("close");
        assert!(
            outcome.ui_request.is_none(),
            "{source:?}: no dialog for a programmatic source"
        );
        assert!(ctx.active_document().is_none(), "{source:?}: closed");
    }

    // An unchanged drawing closes without asking, on any source.
    for source in [Source::Ui, Source::Cli] {
        let (mut ctx, _session, _page, _shapes) = fixture(1);
        let outcome = run_from(&mut ctx, "file.close", Args::new(), source).expect("close");
        assert!(outcome.ui_request.is_none(), "{source:?}: nothing to ask about");
        assert!(ctx.active_document().is_none());
    }

    // The Don't Save answer: `confirm` closes without asking.
    let (mut ctx, _session, page, shapes) = fixture(1);
    select(&mut ctx, page, &shapes);
    run(&mut ctx, "shape.set.width", number_args(&[("width", 3.0)]))
        .expect("set.width runs");
    run(
        &mut ctx,
        "file.close",
        Args::new().with("confirm", ArgValue::Flag(true)),
    )
    .expect("confirmed close");
    assert!(ctx.active_document().is_none(), "confirm closes");
}

/// The Save answer: `save` runs the normal save and closes only when
/// it succeeds (task P7-42 goal 2).
#[test]
fn file_close_save_writes_the_file_and_closes_on_success_only() {
    let dir = temp_dir();
    let target = dir.join("saved.vsdx");

    // With a target the save writes it and the drawing closes.
    let (mut ctx, _session, page, shapes) = fixture(1);
    select(&mut ctx, page, &shapes);
    run(&mut ctx, "shape.set.width", number_args(&[("width", 3.0)]))
        .expect("set.width runs");
    let outcome = run(
        &mut ctx,
        "file.close",
        Args::new()
            .with("save", ArgValue::Flag(true))
            .with("path", ArgValue::Path(target.clone())),
    )
    .expect("save and close");
    assert!(
        outcome.message.as_deref().is_some_and(|m| m.starts_with("Saved and closed")),
        "{:?}",
        outcome.message
    );
    assert!(target.is_file(), "the save wrote the file");
    assert!(ctx.active_document().is_none(), "the drawing closed");

    // A failing save leaves the drawing open and changed (never a
    // close before the write succeeded).
    let (mut ctx, _session, page, shapes) = fixture(1);
    select(&mut ctx, page, &shapes);
    run(&mut ctx, "shape.set.width", number_args(&[("width", 3.0)]))
        .expect("set.width runs");
    let error = run(
        &mut ctx,
        "file.close",
        Args::new()
            .with("save", ArgValue::Flag(true))
            .with("path", ArgValue::Path(dir.join("no/such/dir/a.vsdx"))),
    )
    .expect_err("unwritable target");
    assert!(
        error.to_string().starts_with("cannot write output: "),
        "{error}"
    );
    assert!(dirty(&ctx), "the drawing is still changed");
    assert!(ctx.active_document().is_some(), "the drawing is still open");

    // No target on the desktop: the save asks for one (`NeedsInput`)
    // and nothing closes until it has one (ADR 0036).
    let error = run(
        &mut ctx,
        "file.close",
        Args::new().with("save", ArgValue::Flag(true)),
    )
    .expect_err("no save target");
    assert!(
        matches!(
            &error,
            CommandError::NeedsInput(larrygram_commands::NeededInput::Path { .. })
        ),
        "{error:?}"
    );
    assert!(ctx.active_document().is_some(), "nothing closed");

    // `confirm` and `save` are two different answers, never both.
    let error = run(
        &mut ctx,
        "file.close",
        Args::new()
            .with("confirm", ArgValue::Flag(true))
            .with("save", ArgValue::Flag(true)),
    )
    .expect_err("mutually exclusive");
    assert!(error.to_string().contains("mutually exclusive"), "{error}");
    assert!(ctx.active_document().is_some(), "nothing closed");
    std::fs::remove_dir_all(&dir).expect("clean up");
}

/// The `file.close` row is implemented with both answer flags (the
/// dialog's three choices name it).
#[test]
fn file_close_is_implemented() {
    let registry = registry().expect("registry");
    for id in ["file.close", "app.quit", "file.exit"] {
        let command = CommandId::parse(id).expect("id");
        assert_eq!(
            registry.status(&command),
            CommandStatus::Implemented,
            "{id}"
        );
    }
}

/// A cancelled group restores the state it saw even when a save ran
/// inside it (task P7-42 goal 1): the group saw a changed drawing, so
/// the drawing reads as changed again after the cancel.
#[test]
fn a_cancelled_group_restores_changed_over_a_save_inside_it() {
    let dir = temp_dir();
    let target = dir.join("mid.vsdx");
    let (mut ctx, session, page, shapes) = fixture(1);
    select(&mut ctx, page, &shapes);
    run(&mut ctx, "shape.set.width", number_args(&[("width", 3.0)]))
        .expect("set.width runs");
    ctx.begin_group(session, "Insert Text Box").expect("begin");
    run_from(
        &mut ctx,
        "shape.set.width",
        number_args(&[("width", 4.0)]),
        Source::Ui,
    )
    .expect("set.width runs");
    run_from(
        &mut ctx,
        "file.save_as",
        Args::new().with("path", ArgValue::Path(target.clone())),
        Source::Ui,
    )
    .expect("save_as runs");
    assert!(!dirty(&ctx), "the save recorded the saved marker");
    assert!(
        ctx.documents
            .iter()
            .find(|slot| slot.id == session)
            .expect("session")
            .undo
            .has_group(),
        "an interactive source keeps the group"
    );
    ctx.cancel_group(session).expect("cancel");
    assert!(
        dirty(&ctx),
        "the cancelled group restores the changed state it saw"
    );
    assert_eq!(width_of(&ctx, shapes[0]), 3.0, "the cancel reverted its step");
    std::fs::remove_dir_all(&dir).expect("clean up");
}

/// The suggested save name of a drawing with no path comes from the
/// document kind (task P7-42 goal 2: Save As for a drawing with no
/// path); this pins the `NeedsInput` shape of that ask.
#[test]
fn the_no_target_save_ask_names_the_kind() {
    let (mut ctx, _session, _page, _shapes) = fixture(1);
    let error = run(
        &mut ctx,
        "file.close",
        Args::new().with("save", ArgValue::Flag(true)),
    )
    .expect_err("no save target");
    match error {
        CommandError::NeedsInput(larrygram_commands::NeededInput::Path {
            purpose,
            suggested,
        }) => {
            assert_eq!(purpose, larrygram_commands::PathPurpose::SaveFile);
            assert_eq!(suggested, Some(PathBuf::from("drawing.vsdx")));
        }
        other => panic!("expected a path ask, got {other:?}"),
    }
}
