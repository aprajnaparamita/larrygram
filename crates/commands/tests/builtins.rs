//! Headless command tests (ADR 0037): every command of the Phase 4
//! table runs through `Registry::execute` with `Source::Test`, missing
//! arguments come back as `CommandError::NeedsInput`, and no path can
//! open a dialog (AGENTS.md section 6).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use larrygram_commands::app::{AppCtx, DocumentSession, Settings};
use larrygram_commands::builtins::registry;
use larrygram_commands::{
    ArgValue, Args, CommandError, CommandId, Edit, Invocation, NeededInput, PathPurpose, Source,
    UiRequest,
};
use larrygram_model::{
    Cell, CellName, CellValue, Document, Page, PageId, Shape, ShapeId, Stylesheet, Theme,
};
use larrygram_vsdx::{from_model, read_package, to_model, write_package};

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// A private temp directory for one test.
#[allow(clippy::expect_used)] // test helper: a failure here is the bug
fn temp_dir() -> PathBuf {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let name = format!(
        "larrygram-commands-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    let dir = std::env::temp_dir().join(name);
    std::fs::create_dir_all(&dir).expect("temp directory");
    dir
}

/// A cell with a cached value.
fn cell(name: &str, value: CellValue) -> Cell {
    let mut cell = Cell::new(CellName::new(name));
    cell.value = Some(value);
    cell
}

/// A two-page document; the first page has one shape (whose `PinX` is
/// a string, a kind-mismatch warning), the second is empty.
fn sample_document() -> Document {
    let mut doc = Document::new();
    let mut shape = Shape::new(ShapeId { index: 0 }, 1);
    shape.name = "Rect".to_owned();
    shape
        .sheet
        .cells
        .push(cell("PinX", CellValue::Str("oops".to_owned())));
    let id = doc.add_shape(shape);

    let mut page = Page::new(PageId { index: 0 }, 1);
    page.name = "Page-1".to_owned();
    page.sheet
        .cells
        .push(cell("PageWidth", CellValue::Number(8.5)));
    page.sheet
        .cells
        .push(cell("PageHeight", CellValue::Number(11.0)));
    page.shapes.push(id);
    doc.pages.push(page);

    let mut page = Page::new(PageId { index: 1 }, 2);
    page.name = "Page-2".to_owned();
    page.sheet
        .cells
        .push(cell("PageWidth", CellValue::Number(11.0)));
    page.sheet
        .cells
        .push(cell("PageHeight", CellValue::Number(8.5)));
    doc.pages.push(page);
    doc
}

/// The bytes of a document as a `.vsdx` package.
#[allow(clippy::expect_used)] // test helper: a failure here is the bug
fn package_bytes(doc: &Document) -> Vec<u8> {
    let package = from_model(doc).expect("package");
    write_package(&package).expect("bytes")
}

/// Writes the sample document to a temp file and returns its path.
#[allow(clippy::expect_used)] // test helper: a failure here is the bug
fn sample_file() -> (PathBuf, PathBuf) {
    let dir = temp_dir();
    let file = dir.join("sample.vsdx");
    std::fs::write(&file, package_bytes(&sample_document())).expect("sample file");
    (dir, file)
}

/// Dispatches one command with `Source::Test`.
#[allow(clippy::expect_used)] // test helper: a failure here is the bug
fn run(
    ctx: &mut AppCtx,
    id: &str,
    args: Args,
) -> Result<larrygram_commands::Outcome, CommandError> {
    run_from(ctx, id, args, Source::Test)
}

/// Dispatches one command with an explicit source.
#[allow(clippy::expect_used)] // test helper
fn run_from(
    ctx: &mut AppCtx,
    id: &str,
    args: Args,
    source: Source,
) -> Result<larrygram_commands::Outcome, CommandError> {
    let registry = registry().expect("registry");
    let id = CommandId::intern(id).expect("id");
    registry.execute(&id, ctx, Invocation::new(args, source))
}

/// The index of the active page of the active document.
#[allow(clippy::expect_used)] // test helper
fn active_page(ctx: &AppCtx) -> Option<u32> {
    ctx.active_document()
        .expect("session")
        .active_page
        .map(|id| id.index)
}

/// A test edit that remembers its label only.
struct NamedEdit(&'static str);
impl Edit for NamedEdit {
    fn label(&self) -> &str {
        self.0
    }

    fn apply(&mut self, _ctx: &mut AppCtx) -> Result<(), CommandError> {
        Ok(())
    }

    fn revert(&mut self, _ctx: &mut AppCtx) -> Result<(), CommandError> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// The table dispatches (acceptance)
// ---------------------------------------------------------------------------

#[test]
fn every_command_in_the_table_executes_with_source_test() {
    let (_dir, file) = sample_file();
    let out_dir = temp_dir();

    // file.new
    let mut ctx = AppCtx::new(Settings::default());
    run(&mut ctx, "file.new", Args::new()).expect("file.new");
    assert!(ctx.active_document().is_some());

    // file.open (the wasm upload form works headless too)
    let mut ctx = AppCtx::new(Settings::default());
    run(
        &mut ctx,
        "file.open",
        Args::new()
            .with("name", ArgValue::Text("sample.vsdx".to_owned()))
            .with("data", ArgValue::Bytes(package_bytes(&sample_document()))),
    )
    .expect("file.open");
    assert!(ctx.active_document().unwrap().path.is_none());
    assert!(ctx.active_document().unwrap().preserve.is_some());

    // file.save_as, then file.save to the session path
    let saved = out_dir.join("saved.vsdx");
    run(
        &mut ctx,
        "file.save_as",
        Args::new().with("path", ArgValue::Path(saved.clone())),
    )
    .expect("file.save_as");
    assert_eq!(
        ctx.active_document().unwrap().path.as_deref(),
        Some(Path::new(&saved))
    );
    run(&mut ctx, "file.save", Args::new()).expect("file.save");

    // file.inspect, file.render, file.export
    run(
        &mut ctx,
        "file.inspect",
        Args::new().with("path", ArgValue::Path(file.clone())),
    )
    .expect("file.inspect");
    run(
        &mut ctx,
        "file.render",
        Args::new()
            .with("path", ArgValue::Path(file.clone()))
            .with("out", ArgValue::Path(out_dir.join("page.png"))),
    )
    .expect("file.render");
    run(
        &mut ctx,
        "file.export",
        Args::new()
            .with("path", ArgValue::Path(file.clone()))
            .with("out", ArgValue::Path(out_dir.join("pages"))),
    )
    .expect("file.export");

    // edit.undo, edit.redo over a pushed edit
    ctx.active_document_mut()
        .expect("session")
        .undo
        .push(Box::new(NamedEdit("move shape")));
    let outcome = run(&mut ctx, "edit.undo", Args::new()).expect("edit.undo");
    assert_eq!(outcome.message.as_deref(), Some("Undo move shape"));
    let outcome = run(&mut ctx, "edit.redo", Args::new()).expect("edit.redo");
    assert_eq!(outcome.message.as_deref(), Some("Redo move shape"));

    // view.zoom.in, view.zoom.out, view.zoom.fit
    run(&mut ctx, "view.zoom.in", Args::new()).expect("view.zoom.in");
    run(&mut ctx, "view.zoom.out", Args::new()).expect("view.zoom.out");
    run(
        &mut ctx,
        "view.zoom.fit",
        Args::new()
            .with("width", ArgValue::Number(800.0))
            .with("height", ArgValue::Number(600.0)),
    )
    .expect("view.zoom.fit");

    // view.toggle_grid, view.rulers
    run(&mut ctx, "view.toggle_grid", Args::new()).expect("view.toggle_grid");
    run(&mut ctx, "view.rulers", Args::new()).expect("view.rulers");

    // view.ui_layout.panel_2016, view.ui_layout.panel_365
    run(&mut ctx, "view.ui_layout.panel_365", Args::new()).expect("365");
    assert_eq!(
        ctx.settings.ui_layout,
        larrygram_ui_spec::LayoutKind::Panel365
    );
    run(&mut ctx, "view.ui_layout.panel_2016", Args::new()).expect("2016");

    // page.next, page.previous (wrapping)
    let outcome = run(&mut ctx, "page.next", Args::new()).expect("page.next");
    assert_eq!(outcome.message.as_deref(), Some("Page-2"));
    run(&mut ctx, "page.previous", Args::new()).expect("page.previous");

    // help.about
    let outcome = run(&mut ctx, "help.about", Args::new()).expect("help.about");
    assert_eq!(outcome.ui_request, Some(UiRequest::ShowAbout));

    // file.close
    run(&mut ctx, "file.close", Args::new()).expect("file.close");
    assert!(ctx.active_document().is_none());
}

#[test]
fn missing_arguments_come_back_as_needs_input() {
    let needs_path = |error: CommandError, purpose: PathPurpose| match error {
        CommandError::NeedsInput(NeededInput::Path { purpose: got, .. }) => {
            assert_eq!(got, purpose)
        }
        other => panic!("expected NeedsInput::Path, got {other:?}"),
    };

    let mut ctx = AppCtx::new(Settings::default());
    needs_path(
        run(&mut ctx, "file.open", Args::new()).expect_err("file.open"),
        PathPurpose::OpenFile,
    );
    // `name` without `data` is not enough either.
    needs_path(
        run(
            &mut ctx,
            "file.open",
            Args::new().with("name", ArgValue::Text("a.vsdx".into())),
        )
        .expect_err("file.open name only"),
        PathPurpose::OpenFile,
    );
    needs_path(
        run(&mut ctx, "file.inspect", Args::new()).expect_err("file.inspect"),
        PathPurpose::OpenFile,
    );
    needs_path(
        run(&mut ctx, "file.render", Args::new()).expect_err("file.render"),
        PathPurpose::OpenFile,
    );
    needs_path(
        run(
            &mut ctx,
            "file.render",
            Args::new().with("path", ArgValue::Path(PathBuf::from("a.vsdx"))),
        )
        .expect_err("file.render without out"),
        PathPurpose::SaveFile,
    );
    needs_path(
        run(&mut ctx, "file.export", Args::new()).expect_err("file.export"),
        PathPurpose::OpenFile,
    );

    // A never-saved document has no save target (ADR 0036).
    run(&mut ctx, "file.new", Args::new()).expect("file.new");
    needs_path(
        run(&mut ctx, "file.save", Args::new()).expect_err("file.save"),
        PathPurpose::SaveFile,
    );
    needs_path(
        run(&mut ctx, "file.save_as", Args::new()).expect_err("file.save_as"),
        PathPurpose::SaveFile,
    );

    // view.zoom.fit wants the canvas rectangle in points.
    run(
        &mut ctx,
        "file.open",
        Args::new()
            .with("name", ArgValue::Text("sample.vsdx".into()))
            .with("data", ArgValue::Bytes(package_bytes(&sample_document()))),
    )
    .expect("open");
    let error = run(&mut ctx, "view.zoom.fit", Args::new()).expect_err("view.zoom.fit");
    match error {
        CommandError::NeedsInput(NeededInput::Number { prompt, .. }) => {
            assert!(prompt.contains("width"), "{prompt}")
        }
        other => panic!("expected NeedsInput::Number, got {other:?}"),
    }
    let error = run(
        &mut ctx,
        "view.zoom.fit",
        Args::new().with("width", ArgValue::Number(10.0)),
    )
    .expect_err("view.zoom.fit without height");
    assert!(matches!(error, CommandError::NeedsInput(_)), "{error:?}");
}

#[test]
fn no_command_reports_a_dialog_request_when_input_is_missing() {
    // The acceptance contract: a programmatic caller gets the error,
    // never an outcome asking for a dialog.
    let mut ctx = AppCtx::new(Settings::default());
    for (id, args) in [
        ("file.open", Args::new()),
        ("file.render", Args::new()),
        ("file.inspect", Args::new()),
    ] {
        let outcome = run(&mut ctx, id, args).expect_err(id);
        assert!(
            matches!(outcome, CommandError::NeedsInput(_)),
            "{id}: {outcome:?}"
        );
    }
    // The save commands need an open document first (they are disabled
    // without one); a never-saved one then has no save target.
    run(&mut ctx, "file.new", Args::new()).expect("file.new");
    for (id, args) in [("file.save", Args::new()), ("file.save_as", Args::new())] {
        let outcome = run(&mut ctx, id, args).expect_err(id);
        assert!(
            matches!(outcome, CommandError::NeedsInput(_)),
            "{id}: {outcome:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// file.* behaviour
// ---------------------------------------------------------------------------

#[test]
fn file_open_reads_the_package_and_keeps_it_for_writing() {
    let (dir, file) = sample_file();
    let mut ctx = AppCtx::new(Settings::default());
    let outcome = run(
        &mut ctx,
        "file.open",
        Args::new().with("path", ArgValue::Path(file.clone())),
    )
    .expect("file.open");
    assert!(
        outcome
            .message
            .as_deref()
            .unwrap_or("")
            .contains("sample.vsdx"),
        "{outcome:?}"
    );
    let session = ctx.active_document().expect("session");
    assert_eq!(session.document.pages.len(), 2);
    assert_eq!(session.path.as_deref(), Some(file.as_path()));
    assert!(session.preserve.is_some());
    assert!(!session.dirty);

    // A missing file and an unparsable one are `cannot read input: ...`.
    let error = run(
        &mut ctx,
        "file.open",
        Args::new().with("path", ArgValue::Path(dir.join("nope.vsdx"))),
    )
    .expect_err("missing file");
    assert!(
        error.to_string().starts_with("cannot read input: "),
        "{error}"
    );
    let bad = dir.join("bad.vsdx");
    std::fs::write(&bad, b"not a zip").expect("bad file");
    let error = run(
        &mut ctx,
        "file.open",
        Args::new().with("path", ArgValue::Path(bad)),
    )
    .expect_err("bad file");
    assert!(
        error.to_string().starts_with("cannot read input: "),
        "{error}"
    );
}

#[test]
fn save_as_writes_the_package_and_save_follows_the_session_path() {
    let (_dir, file) = sample_file();
    let out_dir = temp_dir();
    let mut ctx = AppCtx::new(Settings::default());
    run(
        &mut ctx,
        "file.open",
        Args::new().with("path", ArgValue::Path(file)),
    )
    .expect("file.open");
    ctx.active_document_mut().expect("session").dirty = true;

    let saved = out_dir.join("copy.vsdx");
    let outcome = run(
        &mut ctx,
        "file.save_as",
        Args::new().with("path", ArgValue::Path(saved.clone())),
    )
    .expect("file.save_as");
    assert!(outcome.message.unwrap().contains("copy.vsdx"));
    assert!(!ctx.active_document().expect("session").dirty);

    // The written bytes are a package holding the same document.
    let bytes = std::fs::read(&saved).expect("saved bytes");
    let package = read_package(&bytes, &larrygram_vsdx::Limits::DEFAULT).expect("package");
    let document = to_model(&package).expect("document");
    assert_eq!(document.pages.len(), 2);

    // file.save now writes to the session path again.
    ctx.active_document_mut().expect("session").dirty = true;
    run(&mut ctx, "file.save", Args::new()).expect("file.save");
    assert!(!ctx.active_document().expect("session").dirty);
    assert!(saved.exists());

    // An unwritable target is `cannot write output: ...`.
    let error = run(
        &mut ctx,
        "file.save_as",
        Args::new().with("path", ArgValue::Path(PathBuf::from("/no/such/dir/a.vsdx"))),
    )
    .expect_err("unwritable");
    assert!(
        error.to_string().starts_with("cannot write output: "),
        "{error}"
    );
}

#[test]
fn file_close_asks_on_interactive_sources_only() {
    let mut ctx = AppCtx::new(Settings::default());
    run(&mut ctx, "file.new", Args::new()).expect("file.new");
    ctx.active_document_mut().expect("session").dirty = true;

    // Interactive + unsaved: a ConfirmClose request, nothing closed.
    let outcome = run_from(&mut ctx, "file.close", Args::new(), Source::Ui).expect("close");
    match &outcome.ui_request {
        Some(UiRequest::ConfirmClose { name }) => assert_eq!(name, "Untitled"),
        other => panic!("expected ConfirmClose, got {other:?}"),
    }
    assert!(ctx.active_document().is_some(), "nothing was closed");

    // The caller answers with `confirm`.
    run_from(
        &mut ctx,
        "file.close",
        Args::new().with("confirm", ArgValue::Flag(true)),
        Source::Ui,
    )
    .expect("confirmed close");
    assert!(ctx.active_document().is_none());

    // A programmatic source closes without asking.
    run(&mut ctx, "file.new", Args::new()).expect("file.new");
    ctx.active_document_mut().expect("session").dirty = true;
    let outcome = run(&mut ctx, "file.close", Args::new()).expect("close");
    assert!(outcome.ui_request.is_none());
    assert!(ctx.active_document().is_none());

    // Without a document the command is disabled (never a dialog).
    let error = run(&mut ctx, "file.close", Args::new()).expect_err("no document");
    assert!(
        matches!(error, CommandError::NotEnabled { .. }),
        "{error:?}"
    );
}

#[test]
fn inspect_summarises_a_file_or_the_open_document() {
    let (_dir, file) = sample_file();
    let mut ctx = AppCtx::new(Settings::default());

    let outcome = run(
        &mut ctx,
        "file.inspect",
        Args::new().with("path", ArgValue::Path(file.clone())),
    )
    .expect("inspect");
    let text = outcome.message.expect("summary");
    assert!(text.contains("kind: drawing"), "{text}");
    assert!(text.contains("pages: 2"), "{text}");
    assert!(!outcome.warnings.is_empty(), "the sample warns");
    assert!(
        ctx.warnings.contains(&outcome.warnings[0].clone()),
        "warnings are recorded on the state"
    );

    let outcome = run(
        &mut ctx,
        "file.inspect",
        Args::new()
            .with("path", ArgValue::Path(file.clone()))
            .with("json", ArgValue::Flag(true))
            .with("page", ArgValue::Integer(2)),
    )
    .expect("inspect json");
    let value: serde_json::Value =
        serde_json::from_str(&outcome.message.expect("json")).expect("valid JSON");
    assert_eq!(value["pages"].as_array().map(Vec::len), Some(1));

    // A page beyond the document is a read failure (ADR 0026).
    let error = run(
        &mut ctx,
        "file.inspect",
        Args::new()
            .with("path", ArgValue::Path(file))
            .with("page", ArgValue::Integer(9)),
    )
    .expect_err("bad page");
    assert!(
        error.to_string().contains("page 9 does not exist"),
        "{error}"
    );
    let error = run(
        &mut ctx,
        "file.inspect",
        Args::new().with("page", ArgValue::Integer(-1)),
    )
    .expect_err("negative page");
    assert!(
        matches!(error, CommandError::InvalidArgs { .. }),
        "{error:?}"
    );

    // No file: the open document is summarised.
    run(
        &mut ctx,
        "file.open",
        Args::new()
            .with("name", ArgValue::Text("sample.vsdx".into()))
            .with("data", ArgValue::Bytes(package_bytes(&sample_document()))),
    )
    .expect("open");
    let outcome = run(&mut ctx, "file.inspect", Args::new()).expect("inspect active");
    assert!(outcome.message.expect("summary").contains("pages: 2"));
}

#[test]
fn render_writes_one_page_with_the_requested_options() {
    let (_dir, file) = sample_file();
    let out_dir = temp_dir();
    let mut ctx = AppCtx::new(Settings::default());

    let out = out_dir.join("page.png");
    let outcome = run(
        &mut ctx,
        "file.render",
        Args::new()
            .with("path", ArgValue::Path(file.clone()))
            .with("out", ArgValue::Path(out.clone())),
    )
    .expect("render");
    let bytes = std::fs::read(&out).expect("png");
    assert_eq!(&bytes[..4], &[0x89, b'P', b'N', b'G']);
    assert!(!outcome.warnings.is_empty(), "the sample warns");

    // SVG by extension, background overrides.
    let out = out_dir.join("page.svg");
    run(
        &mut ctx,
        "file.render",
        Args::new()
            .with("path", ArgValue::Path(file.clone()))
            .with("out", ArgValue::Path(out.clone()))
            .with("page", ArgValue::Integer(2))
            .with("scale", ArgValue::Number(72.0))
            .with("background", ArgValue::Text("none".into())),
    )
    .expect("render svg");
    let svg = std::fs::read_to_string(&out).expect("svg");
    assert!(svg.contains("<svg"), "not an SVG document");
    run(
        &mut ctx,
        "file.render",
        Args::new()
            .with("path", ArgValue::Path(file.clone()))
            .with("out", ArgValue::Path(out.clone()))
            .with("background", ArgValue::Text("#010203".into())),
    )
    .expect("render coloured");

    // Bad options are InvalidArgs, bad renders are `render failed: ...`.
    for (name, value) in [
        ("background", ArgValue::Text("chartreuse".into())),
        ("format", ArgValue::Text("gif".into())),
        ("scale", ArgValue::Number(0.0)),
    ] {
        let error = run(
            &mut ctx,
            "file.render",
            Args::new()
                .with("path", ArgValue::Path(file.clone()))
                .with("out", ArgValue::Path(out.clone()))
                .with(name, value),
        )
        .expect_err(name);
        assert!(
            matches!(error, CommandError::InvalidArgs { .. }),
            "{name}: {error:?}"
        );
    }
    let error = run(
        &mut ctx,
        "file.render",
        Args::new()
            .with("path", ArgValue::Path(file.clone()))
            .with("out", ArgValue::Path(out.clone()))
            .with("format", ArgValue::Text("png".into()))
            .with("scale", ArgValue::Number(1.0e300)),
    )
    .expect_err("huge scale");
    assert!(error.to_string().starts_with("render failed: "), "{error}");
    let error = run(
        &mut ctx,
        "file.render",
        Args::new()
            .with("path", ArgValue::Path(file))
            .with("out", ArgValue::Path(out))
            .with("page", ArgValue::Integer(9)),
    )
    .expect_err("bad page");
    assert!(
        error.to_string().starts_with("cannot read input: "),
        "{error}"
    );
}

#[test]
fn export_writes_every_page_or_reports_the_failure() {
    let (_dir, file) = sample_file();
    let out_dir = temp_dir();
    let mut ctx = AppCtx::new(Settings::default());

    let out = out_dir.join("pages");
    run(
        &mut ctx,
        "file.export",
        Args::new()
            .with("path", ArgValue::Path(file.clone()))
            .with("out", ArgValue::Path(out.clone()))
            .with("format", ArgValue::Text("svg".into())),
    )
    .expect("export");
    assert!(out.join("page-1.svg").exists());
    assert!(out.join("page-2.svg").exists());

    // A document without pages cannot export.
    let mut empty = Document::new();
    empty.stylesheet = Stylesheet::new();
    empty.themes.push(Theme::new("none"));
    let dir = temp_dir();
    let empty_file = dir.join("empty.vsdx");
    std::fs::write(&empty_file, package_bytes(&empty)).expect("empty file");
    let error = run(
        &mut ctx,
        "file.export",
        Args::new()
            .with("path", ArgValue::Path(empty_file))
            .with("out", ArgValue::Path(out)),
    )
    .expect_err("no pages");
    assert!(error.to_string().contains("no pages"), "{error}");

    // An unwritable directory is `cannot write output: ...`.
    let error = run(
        &mut ctx,
        "file.export",
        Args::new()
            .with("path", ArgValue::Path(file))
            .with("out", ArgValue::Path(PathBuf::from("/no/such/dir/pages"))),
    )
    .expect_err("unwritable");
    assert!(
        error.to_string().starts_with("cannot write output: "),
        "{error}"
    );
}

// ---------------------------------------------------------------------------
// View, page and help behaviour
// ---------------------------------------------------------------------------

#[test]
fn zoom_commands_clamp_and_fit() {
    let mut ctx = AppCtx::new(Settings::default());
    run(
        &mut ctx,
        "file.open",
        Args::new()
            .with("name", ArgValue::Text("sample.vsdx".into()))
            .with("data", ArgValue::Bytes(package_bytes(&sample_document()))),
    )
    .expect("open");

    // 100% * 1.25 = 125%; out steps back by the same factor.
    run(&mut ctx, "view.zoom.in", Args::new()).expect("in");
    let zoom = ctx.active_document().expect("session").view.zoom;
    assert!((zoom - 1.25).abs() < 1e-9, "{zoom}");
    run(&mut ctx, "view.zoom.out", Args::new()).expect("out");
    let zoom = ctx.active_document().expect("session").view.zoom;
    assert!((zoom - 1.0).abs() < 1e-9, "{zoom}");

    // Clamped at 40 and at 0.05.
    ctx.active_document_mut().expect("session").view.zoom = 39.0;
    run(&mut ctx, "view.zoom.in", Args::new()).expect("in");
    assert_eq!(ctx.active_document().expect("session").view.zoom, 40.0);
    ctx.active_document_mut().expect("session").view.zoom = 0.06;
    run(&mut ctx, "view.zoom.out", Args::new()).expect("out");
    assert_eq!(ctx.active_document().expect("session").view.zoom, 0.05);

    // Fit: page 1 is 8.5 x 11 in = 612 x 792 pt; an 612 x 792 canvas
    // fits at 100%.
    run(
        &mut ctx,
        "view.zoom.fit",
        Args::new()
            .with("width", ArgValue::Number(612.0))
            .with("height", ArgValue::Number(792.0)),
    )
    .expect("fit");
    let zoom = ctx.active_document().expect("session").view.zoom;
    assert!((zoom - 1.0).abs() < 1e-9, "{zoom}");

    // Page 2 is 11 x 8.5 in; the canvas limits by height: 612/792.
    run(&mut ctx, "page.next", Args::new()).expect("next");
    run(
        &mut ctx,
        "view.zoom.fit",
        Args::new()
            .with("width", ArgValue::Number(612.0))
            .with("height", ArgValue::Number(792.0)),
    )
    .expect("fit");
    let zoom = ctx.active_document().expect("session").view.zoom;
    assert!((zoom - 612.0 / 792.0).abs() < 1e-9, "{zoom}");

    // A non-positive size is InvalidArgs.
    let error = run(
        &mut ctx,
        "view.zoom.fit",
        Args::new()
            .with("width", ArgValue::Number(-1.0))
            .with("height", ArgValue::Number(10.0)),
    )
    .expect_err("negative");
    assert!(
        matches!(error, CommandError::InvalidArgs { .. }),
        "{error:?}"
    );

    // A page with no size cells falls back to US Letter (8.5 x 11 in).
    let mut doc = Document::new();
    doc.pages.push(Page::new(PageId { index: 0 }, 1));
    let mut ctx = AppCtx::new(Settings::default());
    ctx.open_document(DocumentSession::new(doc, None));
    run(
        &mut ctx,
        "view.zoom.fit",
        Args::new()
            .with("width", ArgValue::Number(408.0))
            .with("height", ArgValue::Number(528.0)),
    )
    .expect("fit");
    let zoom = ctx.active_document().expect("session").view.zoom;
    assert!((zoom - 408.0 / (8.5 * 72.0)).abs() < 1e-9, "{zoom}");
}

#[test]
fn view_toggles_and_the_layout_switch_mutate_the_state() {
    let mut ctx = AppCtx::new(Settings::default());
    run(&mut ctx, "file.new", Args::new()).expect("new");
    let session = ctx.active_document_mut().expect("session");
    assert!(session.view.show_grid);
    assert!(session.view.show_rulers);

    let outcome = run(&mut ctx, "view.toggle_grid", Args::new()).expect("grid");
    assert_eq!(outcome.message.as_deref(), Some("Grid off"));
    assert!(!ctx.settings.show_grid, "the settings default follows");
    let outcome = run(&mut ctx, "view.toggle_grid", Args::new()).expect("grid");
    assert_eq!(outcome.message.as_deref(), Some("Grid on"));
    assert!(ctx.settings.show_grid);
    let outcome = run(&mut ctx, "view.rulers", Args::new()).expect("rulers");
    assert_eq!(outcome.message.as_deref(), Some("Rulers off"));
    assert!(!ctx.settings.show_rulers);
    let session = ctx.active_document().expect("session");
    assert!(session.view.show_grid);
    assert!(!session.view.show_rulers);

    let outcome = run(&mut ctx, "view.ui_layout.panel_365", Args::new()).expect("365");
    assert_eq!(
        outcome.message.as_deref(),
        Some("Panel layout: Panel (365 data)")
    );
    assert_eq!(
        ctx.settings.ui_layout,
        larrygram_ui_spec::LayoutKind::Panel365
    );
    let outcome = run(&mut ctx, "view.ui_layout.panel_2016", Args::new()).expect("2016");
    assert_eq!(
        outcome.message.as_deref(),
        Some("Panel layout: Panel (2016 data)")
    );
    assert_eq!(
        ctx.settings.ui_layout,
        larrygram_ui_spec::LayoutKind::Panel2016
    );

    // Without a document the two toggles still flip the settings
    // default and say so (ADR 0032 amendment, P4-15 review major 1);
    // the per-document view commands stay disabled with a reason.
    let mut ctx = AppCtx::new(Settings::default());
    let outcome = run(&mut ctx, "view.toggle_grid", Args::new()).expect("grid");
    assert_eq!(
        outcome.message.as_deref(),
        Some("Grid off (default for new drawings)")
    );
    assert!(!ctx.settings.show_grid);
    assert!(ctx.active_document().is_none(), "no document was opened");
    let outcome = run(&mut ctx, "view.rulers", Args::new()).expect("rulers");
    assert_eq!(
        outcome.message.as_deref(),
        Some("Rulers off (default for new drawings)")
    );
    assert!(!ctx.settings.show_rulers);
    let error = run(&mut ctx, "view.zoom.in", Args::new()).expect_err("zoom needs a document");
    match error {
        CommandError::NotEnabled { reason, .. } => {
            assert_eq!(reason.as_deref(), Some("no open document"))
        }
        other => panic!("expected NotEnabled, got {other:?}"),
    }
}

/// Settings with the two view toggles set. `Settings` is
/// `non_exhaustive`, so a struct literal is not available outside the
/// crate and the field-assignment lint is allowed here.
#[allow(clippy::field_reassign_with_default)]
fn settings_with_toggles(show_grid: bool, show_rulers: bool) -> Settings {
    let mut settings = Settings::default();
    settings.show_grid = show_grid;
    settings.show_rulers = show_rulers;
    settings
}

/// P4-15 review major 1: `file.new` and `file.open` seed the new
/// session's view toggles from the settings defaults.
#[test]
fn new_sessions_seed_their_view_toggles_from_the_settings() {
    let mut ctx = AppCtx::new(settings_with_toggles(false, false));

    run(&mut ctx, "file.new", Args::new()).expect("new");
    let session = ctx.active_document().expect("session");
    assert!(!session.view.show_grid);
    assert!(!session.view.show_rulers);

    let mut ctx = AppCtx::new(settings_with_toggles(false, true));
    run(
        &mut ctx,
        "file.open",
        Args::new()
            .with("name", ArgValue::Text("sample.vsdx".into()))
            .with("data", ArgValue::Bytes(package_bytes(&sample_document()))),
    )
    .expect("open");
    let session = ctx.active_document().expect("session");
    assert!(!session.view.show_grid);
    assert!(session.view.show_rulers);
}

#[test]
fn page_navigation_wraps_around() {
    let mut ctx = AppCtx::new(Settings::default());
    run(
        &mut ctx,
        "file.open",
        Args::new()
            .with("name", ArgValue::Text("sample.vsdx".into()))
            .with("data", ArgValue::Bytes(package_bytes(&sample_document()))),
    )
    .expect("open");

    assert_eq!(active_page(&ctx), Some(0));
    let outcome = run(&mut ctx, "page.next", Args::new()).expect("next");
    assert_eq!(outcome.message.as_deref(), Some("Page-2"));
    assert_eq!(active_page(&ctx), Some(1));
    run(&mut ctx, "page.next", Args::new()).expect("next");
    assert_eq!(active_page(&ctx), Some(0), "wraps");
    run(&mut ctx, "page.previous", Args::new()).expect("previous");
    assert_eq!(active_page(&ctx), Some(1), "wraps backwards");
    let outcome = run(&mut ctx, "page.previous", Args::new()).expect("previous");
    assert_eq!(outcome.message.as_deref(), Some("Page-1"));

    // A single-page document wraps to itself.
    let mut doc = Document::new();
    doc.pages.push(Page::new(PageId { index: 0 }, 1));
    let mut ctx = AppCtx::new(Settings::default());
    ctx.open_document(DocumentSession::new(doc, None));
    run(&mut ctx, "page.next", Args::new()).expect("next");
    assert_eq!(
        ctx.active_document().expect("session").active_page,
        Some(PageId { index: 0 })
    );

    // A document without pages is disabled (task P7-20: `file.new`
    // now opens a blank drawing with one page, so a page-less session
    // is opened directly here).
    let mut ctx = AppCtx::new(Settings::default());
    ctx.open_document(DocumentSession::new(Document::new(), None));
    let error = run(&mut ctx, "page.next", Args::new()).expect_err("no pages");
    assert!(
        matches!(error, CommandError::NotEnabled { .. }),
        "{error:?}"
    );
}

#[test]
fn help_about_only_requests_the_window() {
    let mut ctx = AppCtx::new(Settings::default());
    let outcome = run(&mut ctx, "help.about", Args::new()).expect("about");
    assert_eq!(outcome.ui_request, Some(UiRequest::ShowAbout));
    assert_eq!(outcome.message.as_deref(), Some("About Larrygram"));
}

// ---------------------------------------------------------------------------
// Undo dispatch
// ---------------------------------------------------------------------------

#[test]
fn undo_and_redo_dispatch_through_the_registry() {
    let mut ctx = AppCtx::new(Settings::default());
    run(&mut ctx, "file.new", Args::new()).expect("new");

    // Empty stack: disabled with a reason.
    let error = run(&mut ctx, "edit.undo", Args::new()).expect_err("empty");
    match error {
        CommandError::NotEnabled { reason, .. } => {
            assert_eq!(reason.as_deref(), Some("nothing to undo"))
        }
        other => panic!("expected NotEnabled, got {other:?}"),
    }
    let error = run(&mut ctx, "edit.redo", Args::new()).expect_err("empty");
    assert!(
        matches!(error, CommandError::NotEnabled { .. }),
        "{error:?}"
    );

    ctx.active_document_mut()
        .expect("session")
        .undo
        .push(Box::new(NamedEdit("rename page")));
    let outcome = run(&mut ctx, "edit.undo", Args::new()).expect("undo");
    assert_eq!(outcome.message.as_deref(), Some("Undo rename page"));
    let outcome = run(&mut ctx, "edit.redo", Args::new()).expect("redo");
    assert_eq!(outcome.message.as_deref(), Some("Redo rename page"));
    assert!(
        ctx.active_document().expect("session").undo.can_undo(),
        "the stack is back in the session"
    );
}

#[test]
fn disabled_commands_name_their_reason_and_never_run() {
    let mut ctx = AppCtx::new(Settings::default());
    let registry = registry().expect("registry");
    let id = CommandId::intern("file.save").expect("id");
    let enabled = registry.enabled(&id, &ctx);
    assert!(!enabled.is_enabled());
    assert_eq!(enabled.reason.as_deref(), Some("no open document"));
    let error = registry
        .execute(&id, &mut ctx, Invocation::empty(Source::Test))
        .expect_err("disabled");
    assert!(
        matches!(error, CommandError::NotEnabled { .. }),
        "{error:?}"
    );
}

/// The package round-trip used by the save path (ADR 0018): writing a
/// document read from a package keeps unmapped parts.
#[test]
fn saving_keeps_the_source_package() {
    let (_dir, file) = sample_file();
    let mut ctx = AppCtx::new(Settings::default());
    run(
        &mut ctx,
        "file.open",
        Args::new().with("path", ArgValue::Path(file)),
    )
    .expect("open");
    let session = ctx.active_document_mut().expect("session");
    let parts = session
        .preserve
        .as_ref()
        .map(|package| package.parts.len())
        .unwrap_or(0);
    assert!(parts > 1, "the source package is kept");
    let out = temp_dir().join("kept.vsdx");
    run(
        &mut ctx,
        "file.save_as",
        Args::new().with("path", ArgValue::Path(out.clone())),
    )
    .expect("save_as");
    let saved = std::fs::read(&out).expect("saved");
    let package = read_package(&saved, &larrygram_vsdx::Limits::DEFAULT).expect("package");
    assert!(package.parts.len() > 1, "the package keeps its parts");
}

// ---------------------------------------------------------------------------
// P4-15: settings, view state and the Options/quit commands
// ---------------------------------------------------------------------------

/// P4-15 Goal 2: `view.units` sets the display units from a
/// `DisplayUnits` name (ASCII case-insensitive); a missing or unknown
/// value is `InvalidArgs` naming the accepted values.
#[test]
fn view_units_sets_the_display_units() {
    let mut ctx = AppCtx::new(Settings::default());
    for (input, expected) in [
        ("Millimeters", "Millimeters"),
        ("centimeters", "Centimeters"),
        ("METERS", "Meters"),
        ("Feet", "Feet"),
        ("Inches", "Inches"),
    ] {
        let outcome = run(
            &mut ctx,
            "view.units",
            Args::new().with("units", ArgValue::Text(input.to_owned())),
        )
        .expect(input);
        assert_eq!(outcome.message, Some(format!("Units: {expected}")));
        assert_eq!(
            ctx.settings.units,
            larrygram_commands::DisplayUnits::parse(expected).expect("name")
        );
    }

    let bad = [
        Args::new(),
        Args::new().with("units", ArgValue::Text("furlongs".to_owned())),
        Args::new().with("units", ArgValue::Integer(3)),
    ];
    for args in bad {
        let error = run(&mut ctx, "view.units", args).expect_err("bad units");
        assert!(
            matches!(error, CommandError::InvalidArgs { .. }),
            "expected InvalidArgs, got {error:?}"
        );
        let message = error.to_string();
        for name in larrygram_commands::DisplayUnits::NAMES {
            assert!(message.contains(name), "{message} must name {name}");
        }
    }
    assert_eq!(
        ctx.settings.units,
        larrygram_commands::DisplayUnits::Inches,
        "the failed runs changed nothing (the last good run set Inches)"
    );
}

/// An edit whose `revert` closes the first document, so the document
/// indices move while the undo step runs (P4-15 review minor 3).
struct CloseFirstEdit;

impl Edit for CloseFirstEdit {
    fn label(&self) -> &str {
        "close first"
    }

    fn apply(&mut self, _ctx: &mut AppCtx) -> Result<(), CommandError> {
        Ok(())
    }

    fn revert(&mut self, ctx: &mut AppCtx) -> Result<(), CommandError> {
        ctx.close_document(0);
        Ok(())
    }
}

/// P4-15 review minor 3: the undo stack goes back to the session it
/// came from (by identity), even when the step's `revert` closes
/// another document and the index moves. The old put-back-by-index
/// dropped the stack on the floor here.
#[test]
fn undo_returns_the_stack_to_the_session_it_came_from() {
    let mut ctx = AppCtx::new(Settings::default());
    run(&mut ctx, "file.new", Args::new()).expect("new 1");
    run(&mut ctx, "file.new", Args::new()).expect("new 2");
    run(&mut ctx, "file.new", Args::new()).expect("new 3");
    assert_eq!(ctx.active, Some(2));
    let third = ctx.active_document().expect("third").id;

    ctx.active_document_mut()
        .expect("third")
        .undo
        .push(Box::new(CloseFirstEdit));
    let outcome = run(&mut ctx, "edit.undo", Args::new()).expect("undo");
    assert_eq!(outcome.message.as_deref(), Some("Undo close first"));

    assert_eq!(ctx.documents.len(), 2, "the revert closed the first");
    assert_eq!(ctx.active, Some(1), "the active document did not move");
    let session = ctx.active_document().expect("third");
    assert_eq!(session.id, third, "the same session is active");
    assert!(
        session.undo.can_redo(),
        "the stack came back to the same session"
    );
}

/// P4-15 Goal 2 (ADR 0032 amendment): the window commands are real
/// handlers that only return requests (AGENTS.md section 6: never a
/// dialog, never a side effect from the command itself).
#[test]
fn the_window_commands_only_request_their_windows() {
    let registry = registry().expect("registry");
    let mut ctx = AppCtx::new(Settings::default());

    let outcome = run(&mut ctx, "app.office.center.options", Args::new()).expect("options");
    assert_eq!(outcome.ui_request, Some(UiRequest::ShowOptions));
    assert!(outcome.payload.is_none());
    assert!(ctx.active_document().is_none(), "no side effect");

    for id in ["app.quit", "file.exit"] {
        let outcome = run(&mut ctx, id, Args::new()).expect(id);
        assert_eq!(outcome.message.as_deref(), Some("Quit"));
        assert_eq!(outcome.ui_request, Some(UiRequest::Quit));
        assert!(ctx.active_document().is_none(), "nothing was closed");

        // Always enabled, whatever the state (P4-15 Goal 2).
        let command = CommandId::parse(id).expect("id");
        assert!(registry.enabled(&command, &ctx).is_enabled(), "{id}");
    }
}

/// P4-15 review minor 6: `page: 0` is an argument error, and every
/// page error carries the caller's number (the old code silently
/// saturated to `u32::MAX`).
#[test]
fn page_arguments_are_validated_with_the_callers_number() {
    let (_dir, file) = sample_file();
    let mut ctx = AppCtx::new(Settings::default());
    let open = Args::new().with("path", ArgValue::Path(file));

    for (number, expected) in [
        (0, "got 0"),
        (-3, "got -3"),
        (5_000_000_000, "got 5000000000"),
    ] {
        let error = run(
            &mut ctx,
            "file.inspect",
            open.clone().with("page", ArgValue::Integer(number)),
        )
        .expect_err("bad page");
        match &error {
            CommandError::InvalidArgs { message } => {
                assert!(message.contains("counted from 1"), "{message}");
                assert!(message.contains(expected), "{message}");
            }
            other => panic!("expected InvalidArgs, got {other:?}"),
        }
    }

    // An in-range page that does not exist reports the caller's own
    // number (the sample file has two pages).
    let error = run(
        &mut ctx,
        "file.inspect",
        open.with("page", ArgValue::Integer(9)),
    )
    .expect_err("page 9");
    assert!(
        error.to_string().contains("page 9 does not exist"),
        "{error}"
    );
}

/// P4-12 coverage minor 4: `view.zoom.fit` on a page whose sheet has
/// no cached cells falls back to the renderer's 8.5 x 11 in page size
/// (the same per-axis fallback `larrygram_render::build` applies).
#[test]
fn zoom_fit_falls_back_to_letter_for_a_page_without_sheet_cells() {
    let mut doc = Document::new();
    doc.pages.push(Page::new(PageId { index: 0 }, 1));
    let mut ctx = AppCtx::new(Settings::default());
    run(
        &mut ctx,
        "file.open",
        Args::new()
            .with("name", ArgValue::Text("no-cells.vsdx".into()))
            .with("data", ArgValue::Bytes(package_bytes(&doc))),
    )
    .expect("open");
    let outcome = run(
        &mut ctx,
        "view.zoom.fit",
        Args::new()
            .with("width", ArgValue::Number(408.0))
            .with("height", ArgValue::Number(528.0)),
    )
    .expect("fit");
    assert_eq!(outcome.message.as_deref(), Some("Zoom 67%"));
    let zoom = ctx.active_document().expect("session").view.zoom;
    assert!((zoom - 408.0 / (8.5 * 72.0)).abs() < 1e-9, "{zoom}");
}
