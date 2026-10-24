//! Command execution (ADR 0026).
//!
//! `run_to` maps one [`Command`] to its registry command id and
//! [`Args`](larrygram_commands::Args) and dispatches through
//! [`builtins::registry`](larrygram_commands::builtins::registry) with
//! [`Source::Cli`] (the ADR 0026 seam moved to `crates/commands` in
//! Phase 4). The printed output does not change: nothing to stdout
//! except `inspect` output and `--help`; warnings and errors are one
//! line each on the error stream (ADR 0021/0026).

use std::io::Write;

use larrygram_commands::app::{AppCtx, Settings};
use larrygram_commands::builtins;
use larrygram_commands::{ArgValue, Args, CommandError, CommandId, Invocation, Source};

use crate::cli::{Background, Command, Error, OutputFormat, exit};

/// The default device scale: 96 pixels per inch (ADR 0025).
pub(crate) const DEFAULT_SCALE: f64 = larrygram_render::build::DEFAULT_SCALE;

/// The ADR 0026 report prefixes of [`CommandError::Failed`]: the CLI
/// maps them to its exit codes (`cannot read input` is the input error,
/// the other two are render or output errors).
const READ_PREFIX: &str = "cannot read input: ";
const IO_PREFIX: &str = "cannot write output: ";
const RENDER_PREFIX: &str = "render failed: ";

/// Runs a command against the given output and error streams.
///
/// A failure is reported to `err` as `error: <message>` and mapped to
/// its exit code by [`Error::exit_code`]. `Err` is returned only when
/// that report cannot be written.
pub(crate) fn run_to(
    command: &Command,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<i32, Error> {
    match execute(command, out, err) {
        Ok(code) => Ok(code),
        Err(error) => match report(err, &format!("error: {error}")) {
            Ok(()) => Ok(error.exit_code()),
            Err(_) => Err(error),
        },
    }
}

/// Writes one report line to `to`, adding the newline, and flushes.
pub(crate) fn report(to: &mut dyn Write, line: &str) -> std::io::Result<()> {
    to.write_all(line.as_bytes())?;
    if !line.ends_with('\n') {
        to.write_all(b"\n")?;
    }
    to.flush()
}

/// Dispatches one command through the registry and prints its result
/// (ADR 0026 output contract).
fn execute(command: &Command, out: &mut dyn Write, err: &mut dyn Write) -> Result<i32, Error> {
    let registry = builtins::registry().map_err(|error| Error::Render(error.to_string()))?;
    let mut ctx = AppCtx::new(Settings::default());
    let (id, invocation, strict) = invocation_for(command)?;
    match registry.execute(&id, &mut ctx, invocation) {
        Ok(outcome) => {
            print_warnings(err, &outcome.warnings)?;
            if matches!(command, Command::Inspect(_)) {
                write_summary(out, outcome.message.as_deref().unwrap_or(""))?;
            }
            Ok(strict_exit(strict, outcome.warnings.len()))
        }
        Err(error) => {
            // Warnings the command recorded before it failed still
            // print (ADR 0021).
            print_warnings(err, &ctx.warnings)?;
            Err(classify(&error))
        }
    }
}

/// Maps one CLI command to its registry id, arguments and `--strict`
/// flag (ADR 0028 gives the argument names).
fn invocation_for(command: &Command) -> Result<(CommandId, Invocation, bool), Error> {
    let static_id =
        |id: &'static str| CommandId::parse(id).map_err(|error| Error::Render(error.to_string()));
    let (id, invocation, strict) = match command {
        Command::Inspect(args) => {
            let mut entries = Args::new()
                .with("path", ArgValue::Path(args.file.clone()))
                .with("json", ArgValue::Flag(args.json));
            if let Some(page) = args.page {
                entries.set("page", ArgValue::Integer(page.into()));
            }
            (
                static_id("file.inspect")?,
                Invocation::new(entries, Source::Cli),
                false,
            )
        }
        Command::Render(args) => {
            let mut entries = Args::new()
                .with("path", ArgValue::Path(args.file.clone()))
                .with("out", ArgValue::Path(args.out.clone()))
                .with(
                    "format",
                    ArgValue::Text(format_text(args.format).to_owned()),
                )
                .with("scale", ArgValue::Number(args.scale))
                .with("strict", ArgValue::Flag(args.strict));
            if let Some(page) = args.page {
                entries.set("page", ArgValue::Integer(page.into()));
            }
            if let Some(background) = args.background {
                entries.set("background", ArgValue::Text(background_text(background)));
            }
            (
                static_id("file.render")?,
                Invocation::new(entries, Source::Cli),
                args.strict,
            )
        }
        Command::Export(args) => {
            let entries = Args::new()
                .with("path", ArgValue::Path(args.file.clone()))
                .with("out", ArgValue::Path(args.out.clone()))
                .with(
                    "format",
                    ArgValue::Text(format_text(args.format).to_owned()),
                )
                .with("scale", ArgValue::Number(args.scale))
                .with("strict", ArgValue::Flag(args.strict));
            (
                static_id("file.export")?,
                Invocation::new(entries, Source::Cli),
                args.strict,
            )
        }
    };
    Ok((id, invocation, strict))
}

/// The `format` argument text (ADR 0026).
fn format_text(format: OutputFormat) -> &'static str {
    match format {
        OutputFormat::Png => "png",
        OutputFormat::Svg => "svg",
    }
}

/// The `background` argument text (ADR 0026 `--background #RRGGBB|none`).
fn background_text(background: Background) -> String {
    match background {
        Background::Transparent => "none".to_owned(),
        Background::Color(color) => format!("#{:02X}{:02X}{:02X}", color.r, color.g, color.b),
    }
}

/// Maps a registry failure to the ADR 0026 error class (and so to the
/// exit code): the report prefix the file commands write decides.
fn classify(error: &CommandError) -> Error {
    let CommandError::Failed { message } = error else {
        // The CLI parser validates its arguments, so no other variant
        // can reach `run`; treat one as a render failure (exit 3).
        return Error::Render(error.to_string());
    };
    if let Some(cause) = message.strip_prefix(READ_PREFIX) {
        Error::Read(cause.to_owned())
    } else if let Some(cause) = message.strip_prefix(IO_PREFIX) {
        Error::Io(cause.to_owned())
    } else if let Some(cause) = message.strip_prefix(RENDER_PREFIX) {
        Error::Render(cause.to_owned())
    } else {
        Error::Render(message.clone())
    }
}

/// Prints one `warning: ` line per warning to `err` (ADR 0021).
fn print_warnings(err: &mut dyn Write, warnings: &[String]) -> Result<(), Error> {
    for warning in warnings {
        report(err, &format!("warning: {warning}"))
            .map_err(|error| Error::Io(format!("standard error: {error}")))?;
    }
    Ok(())
}

/// Writes the `inspect` summary to `out`, ending in one newline.
fn write_summary(out: &mut dyn Write, text: &str) -> Result<(), Error> {
    let mut text = text.as_bytes().to_vec();
    if text.last() != Some(&b'\n') {
        text.push(b'\n');
    }
    out.write_all(&text)
        .and_then(|()| out.flush())
        .map_err(|error| Error::Io(format!("standard output: {error}")))
}

/// The exit code of a run that produced warnings (ADR 0021).
fn strict_exit(strict: bool, warnings: usize) -> i32 {
    if strict && warnings > 0 {
        exit::WARNINGS
    } else {
        exit::OK
    }
}

// The three names below exist for the unchanged ADR 0026 tests, which
// take them from `super::*`: `warning_line` and friends format the
// warning report, `Document` and `Path` build the test fixtures.
#[cfg(test)]
use crate::cli::{Export, Inspect, Render};
#[cfg(test)]
use larrygram_commands::builtins::{sheet_text, value_text, warning_line};
#[cfg(test)]
use larrygram_model::Document;
#[cfg(test)]
use std::path::Path;

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    use larrygram_model::{
        Cell, CellName, CellValue, CellWarning, Color, Page, PageId, Shape, ShapeId, SheetRef,
        Stylesheet, Theme, WarningKind,
    };
    use larrygram_vsdx::{from_model, write_package};

    use super::*;
    use crate::cli::parse::USAGE_TEXT;

    /// A private temp directory for one test.
    fn temp_dir() -> PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let name = format!(
            "larrygram-cli-{}-{}",
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

    /// A cell with a formula but no cached value: the reader warns.
    fn formula_cell(name: &str, formula: &str) -> Cell {
        let mut cell = Cell::new(CellName::new(name));
        cell.formula = Some(formula.to_owned());
        cell
    }

    /// A two-page document; the first page has one shape whose `PinX` is a
    /// string (a kind mismatch warning), the second is empty.
    fn document() -> Document {
        let mut doc = Document::new();
        let mut shape = Shape::new(ShapeId { index: 0 }, 1);
        shape.name = "Rect".to_owned();
        shape
            .sheet
            .cells
            .push(cell("PinX", CellValue::Str("oops".to_owned())));
        shape.sheet.cells.push(formula_cell("PinY", "1.5"));
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
    fn package_bytes(doc: &Document) -> Vec<u8> {
        let package = from_model(doc).expect("package");
        write_package(&package).expect("bytes")
    }

    /// Writes the sample document to a temp file and returns its path.
    fn sample_file() -> (PathBuf, PathBuf) {
        let dir = temp_dir();
        let file = dir.join("sample.vsdx");
        std::fs::write(&file, package_bytes(&document())).expect("sample file");
        (dir, file)
    }

    /// Writes raw bytes to a temp file and returns its path.
    fn raw_file(bytes: &[u8]) -> (PathBuf, PathBuf) {
        let dir = temp_dir();
        let file = dir.join("raw.vsdx");
        std::fs::write(&file, bytes).expect("raw file");
        (dir, file)
    }

    fn render_command(file: &Path, out: &Path) -> Render {
        Render {
            file: file.to_owned(),
            out: out.to_owned(),
            page: None,
            format: OutputFormat::Png,
            scale: DEFAULT_SCALE,
            background: None,
            strict: false,
        }
    }

    fn run_buffered(command: &Command) -> (Result<i32, Error>, String, String) {
        let mut out = Vec::new();
        let mut err = Vec::new();
        let result = run_to(command, &mut out, &mut err);
        (
            result,
            String::from_utf8_lossy(&out).into_owned(),
            String::from_utf8_lossy(&err).into_owned(),
        )
    }

    #[test]
    fn render_writes_a_png_and_exits_zero() {
        let (dir, file) = sample_file();
        let out = dir.join("page.png");
        let (result, stdout, stderr) = run_buffered(&Command::Render(render_command(&file, &out)));
        assert_eq!(result, Ok(exit::OK));
        assert!(stdout.is_empty(), "render must not write to stdout");
        let bytes = std::fs::read(&out).expect("png");
        assert_eq!(&bytes[..4], &[0x89, b'P', b'N', b'G']);
        assert!(
            stderr.contains("warning: "),
            "warnings go to stderr: {stderr}"
        );
    }

    #[test]
    fn render_writes_svg_and_applies_the_background() {
        let (dir, file) = sample_file();
        let out = dir.join("page.svg");
        let mut command = render_command(&file, &out);
        command.format = OutputFormat::Svg;
        command.background = Some(Background::Color(larrygram_render::Rgba::opaque(1, 2, 3)));
        let (result, _, _) = run_buffered(&Command::Render(command));
        assert_eq!(result, Ok(exit::OK));
        let svg = std::fs::read_to_string(&out).expect("svg");
        assert!(svg.contains("<svg"), "not an SVG document: {svg}");
    }

    #[test]
    fn render_background_none_is_transparent() {
        let (dir, file) = sample_file();
        let out = dir.join("page.svg");
        let mut command = render_command(&file, &out);
        command.format = OutputFormat::Svg;
        command.background = Some(Background::Transparent);
        let (result, _, _) = run_buffered(&Command::Render(command));
        assert_eq!(result, Ok(exit::OK));
    }

    #[test]
    fn render_selects_a_page() {
        let (dir, file) = sample_file();
        let out = dir.join("page-2.png");
        let mut command = render_command(&file, &out);
        command.page = Some(2);
        let (result, _, _) = run_buffered(&Command::Render(command));
        assert_eq!(result, Ok(exit::OK));
        assert!(out.exists());
    }

    #[test]
    fn export_writes_every_page() {
        let (dir, file) = sample_file();
        let out = dir.join("pages");
        let command = Command::Export(Export {
            file,
            out: out.clone(),
            format: OutputFormat::Png,
            scale: DEFAULT_SCALE,
            strict: false,
        });
        let (result, stdout, _) = run_buffered(&command);
        assert_eq!(result, Ok(exit::OK));
        assert!(stdout.is_empty(), "export must not write to stdout");
        assert!(out.join("page-1.png").exists());
        assert!(out.join("page-2.png").exists());
    }

    #[test]
    fn export_writes_svg_names() {
        let (dir, file) = sample_file();
        let out = dir.join("pages-svg");
        let command = Command::Export(Export {
            file,
            out: out.clone(),
            format: OutputFormat::Svg,
            scale: DEFAULT_SCALE,
            strict: false,
        });
        let (result, _, _) = run_buffered(&command);
        assert_eq!(result, Ok(exit::OK));
        assert!(out.join("page-1.svg").exists());
    }

    #[test]
    fn inspect_prints_the_text_summary() {
        let (_dir, file) = sample_file();
        let command = Command::Inspect(Inspect {
            file,
            json: false,
            page: None,
        });
        let (result, stdout, _) = run_buffered(&command);
        assert_eq!(result, Ok(exit::OK));
        assert!(stdout.contains("kind: drawing"), "{stdout}");
        assert!(stdout.contains("pages: 2"), "{stdout}");
    }

    #[test]
    fn inspect_json_lists_the_pages() {
        let (_dir, file) = sample_file();
        let command = Command::Inspect(Inspect {
            file,
            json: true,
            page: Some(1),
        });
        let (result, stdout, _) = run_buffered(&command);
        assert_eq!(result, Ok(exit::OK));
        let value: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
        assert_eq!(value["pages"].as_array().map(Vec::len), Some(1));
        assert_eq!(value["kind"], "drawing");
        assert!(stdout.ends_with('\n'), "output ends with a newline");
    }

    #[test]
    fn a_missing_file_is_exit_two() {
        let (dir, _file) = sample_file();
        let out = dir.join("page.png");
        let mut command = render_command(&dir.join("missing.vsdx"), &out);
        command.page = None;
        let (result, stdout, stderr) = run_buffered(&Command::Render(command));
        assert_eq!(result, Ok(exit::READ));
        assert!(stdout.is_empty());
        assert!(stderr.starts_with("error: cannot read input:"), "{stderr}");
    }

    #[test]
    fn an_unparsable_package_is_exit_two() {
        let (dir, file) = raw_file(b"this is not a zip archive");
        let out = dir.join("page.png");
        let (result, _, stderr) = run_buffered(&Command::Render(render_command(&file, &out)));
        assert_eq!(result, Ok(exit::READ));
        assert!(stderr.starts_with("error: cannot read input:"), "{stderr}");
    }

    #[test]
    fn a_page_beyond_the_document_is_exit_two() {
        let (dir, file) = sample_file();
        let out = dir.join("page.png");
        let mut command = render_command(&file, &out);
        command.page = Some(9);
        let (result, _, stderr) = run_buffered(&Command::Render(command));
        assert_eq!(result, Ok(exit::READ));
        assert!(stderr.contains("page 9 does not exist"), "{stderr}");
    }

    #[test]
    fn an_unwritable_output_is_exit_three() {
        let (_dir, file) = sample_file();
        let out = PathBuf::from("/this/directory/does/not/exist/page.png");
        let (result, _, stderr) = run_buffered(&Command::Render(render_command(&file, &out)));
        assert_eq!(result, Ok(exit::RENDER));
        assert!(stderr.contains("error: cannot write output:"), "{stderr}");
    }

    #[test]
    fn a_huge_scale_is_exit_three_not_a_panic() {
        // A bad `--scale` is `InvalidOptions` (ADR 0025 amendment): still
        // exit 3 (ADR 0026), but reported as a bad option, not as a
        // document that is too big.
        let (dir, file) = sample_file();
        let out = dir.join("page.png");
        let mut command = render_command(&file, &out);
        command.scale = 1.0e300;
        let (result, _, stderr) = run_buffered(&Command::Render(command));
        assert_eq!(result, Ok(exit::RENDER));
        assert!(
            stderr.contains("the render options are not valid"),
            "{stderr}"
        );
        assert!(!out.exists(), "no output for a capped raster");
    }

    #[test]
    fn a_huge_page_is_exit_three_not_a_panic() {
        let dir = temp_dir();
        let mut doc = Document::new();
        let mut shape = Shape::new(ShapeId { index: 0 }, 1);
        shape.sheet.cells.push(cell("PinX", CellValue::Number(1.0)));
        let id = doc.add_shape(shape);
        let mut page = Page::new(PageId { index: 0 }, 1);
        page.sheet
            .cells
            .push(cell("PageWidth", CellValue::Number(1.0e6)));
        page.sheet
            .cells
            .push(cell("PageHeight", CellValue::Number(1.0e6)));
        page.shapes.push(id);
        doc.pages.push(page);
        let file = dir.join("huge.vsdx");
        std::fs::write(&file, package_bytes(&doc)).expect("huge file");
        let out = dir.join("page.png");
        let (result, _, stderr) = run_buffered(&Command::Render(render_command(&file, &out)));
        assert_eq!(result, Ok(exit::RENDER));
        assert!(stderr.contains("larger than the allowed caps"), "{stderr}");
    }

    #[test]
    fn warnings_exit_zero_and_strict_exits_four() {
        let (dir, file) = sample_file();
        let out = dir.join("page.png");
        let (result, _, stderr) = run_buffered(&Command::Render(render_command(&file, &out)));
        assert_eq!(result, Ok(exit::OK));
        assert!(stderr.contains("warning: "), "{stderr}");

        let mut command = render_command(&file, &out);
        command.strict = true;
        let (result, _, stderr) = run_buffered(&Command::Render(command));
        assert_eq!(result, Ok(exit::WARNINGS));
        assert!(out.exists(), "the output is written even under --strict");
        assert!(stderr.contains("warning: "), "{stderr}");
    }

    #[test]
    fn export_strict_exits_four_on_warnings() {
        let (dir, file) = sample_file();
        let out = dir.join("pages");
        let command = Command::Export(Export {
            file,
            out: out.clone(),
            format: OutputFormat::Png,
            scale: DEFAULT_SCALE,
            strict: true,
        });
        let (result, _, _) = run_buffered(&command);
        assert_eq!(result, Ok(exit::WARNINGS));
        assert!(out.join("page-1.png").exists());
    }

    #[test]
    fn a_document_without_pages_cannot_export() {
        let dir = temp_dir();
        let mut doc = Document::new();
        doc.stylesheet = Stylesheet::new();
        doc.themes.push(Theme::new("none"));
        let file = dir.join("empty.vsdx");
        std::fs::write(&file, package_bytes(&doc)).expect("empty file");
        let out = dir.join("pages");
        let command = Command::Export(Export {
            file,
            out,
            format: OutputFormat::Png,
            scale: DEFAULT_SCALE,
            strict: false,
        });
        let (result, _, stderr) = run_buffered(&command);
        assert_eq!(result, Ok(exit::READ));
        assert!(stderr.contains("no pages"), "{stderr}");
    }

    #[test]
    fn report_writes_one_line_and_flushes() {
        let mut sink = Vec::new();
        report(&mut sink, "hello").expect("report");
        report(&mut sink, "line\n").expect("report");
        assert_eq!(String::from_utf8_lossy(&sink), "hello\nline\n");
    }

    /// A writer whose sink always fails, for the unreportable-error path.
    struct FailingWriter;

    impl Write for FailingWriter {
        fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("sink full"))
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn an_unwritable_error_report_returns_the_error() {
        let (dir, _file) = sample_file();
        let command = Command::Inspect(Inspect {
            file: dir.join("missing.vsdx"),
            json: false,
            page: None,
        });
        let result = run_to(&command, &mut Vec::new(), &mut FailingWriter);
        match result {
            Err(Error::Read(message)) => assert!(message.contains("missing.vsdx")),
            other => panic!("expected Error::Read, got {other:?}"),
        }
    }

    #[test]
    fn an_unwritable_inspect_output_is_an_io_error() {
        let (_dir, file) = sample_file();
        let command = Command::Inspect(Inspect {
            file,
            json: false,
            page: None,
        });
        // The failure is reported to stderr and mapped to the output error
        // code (ADR 0026).
        let result = run_to(&command, &mut FailingWriter, &mut Vec::new());
        assert_eq!(result, Ok(exit::RENDER));
        // When the report cannot be written either, `run_to` returns the
        // error itself.
        let result = run_to(&command, &mut FailingWriter, &mut FailingWriter);
        match result {
            Err(Error::Io(message)) => assert!(message.contains("sink full"), "{message}"),
            other => panic!("expected Error::Io, got {other:?}"),
        }
    }

    #[test]
    fn main_from_maps_usage_errors_to_exit_one() {
        let result = crate::cli::main_from(["inspect"].map(OsString::from).into_iter());
        assert_eq!(result, Ok(exit::USAGE));
    }

    #[test]
    fn main_from_maps_help_to_exit_zero() {
        let result = crate::cli::main_from(["--help"].map(OsString::from).into_iter());
        assert_eq!(result, Ok(exit::OK));
    }

    #[test]
    fn main_from_runs_a_command() {
        let (_dir, file) = sample_file();
        let mut args: Vec<OsString> = vec!["inspect".into(), "--json".into()];
        args.push(file.into_os_string());
        let result = crate::cli::main_from(args.into_iter());
        assert_eq!(result, Ok(exit::OK));
    }

    #[test]
    fn usage_text_names_the_commands() {
        for name in ["inspect", "render", "export", "Exit codes:"] {
            assert!(USAGE_TEXT.contains(name), "usage text misses {name}");
        }
    }

    #[test]
    fn strict_exit_only_fails_with_warnings() {
        assert_eq!(strict_exit(true, 0), exit::OK);
        assert_eq!(strict_exit(true, 1), exit::WARNINGS);
        assert_eq!(strict_exit(false, 1), exit::OK);
    }

    // --- the warning report (P3-17, coverage finding major 5) ---------

    /// One warning of `kind`, substituting `value`, on `sheet`.
    fn warning(sheet: SheetRef, cell: &str, kind: WarningKind, value: CellValue) -> CellWarning {
        CellWarning {
            sheet,
            cell: CellName::new(cell),
            kind,
            substituted: value,
        }
    }

    #[test]
    fn warning_lines_name_every_kind() {
        let doc = Document::new();
        let sheet = SheetRef::Shape(ShapeId { index: 0 });
        let line = warning_line(
            &doc,
            &warning(
                sheet,
                "LineWeight",
                WarningKind::Missing,
                CellValue::Number(0.5),
            ),
        );
        assert_eq!(line, "shape #0 LineWeight: no cached value, using 0.5");

        let line = warning_line(
            &doc,
            &warning(
                sheet,
                "PinX",
                WarningKind::ErrorValue("#VALUE!".to_owned()),
                CellValue::Number(0.0),
            ),
        );
        assert_eq!(line, "shape #0 PinX: cached error \"#VALUE!\", using 0");

        let line = warning_line(
            &doc,
            &warning(
                sheet,
                "Width",
                WarningKind::KindMismatch,
                CellValue::Number(0.0),
            ),
        );
        assert_eq!(line, "shape #0 Width: wrong kind, using 0");

        let line = warning_line(
            &doc,
            &warning(
                sheet,
                "FillForegnd",
                WarningKind::UnresolvedTheme,
                CellValue::Color(Color::new(0, 0, 0)),
            ),
        );
        assert_eq!(
            line,
            "shape #0 FillForegnd: the theme cannot supply it, using #000000"
        );
    }

    #[test]
    fn sheet_text_names_pages_shapes_and_fallbacks() {
        let mut doc = Document::new();
        doc.add_shape(Shape::new(ShapeId { index: 0 }, 12));
        doc.pages.push(Page::new(PageId { index: 0 }, 5));
        assert_eq!(
            sheet_text(&doc, SheetRef::Shape(ShapeId { index: 0 })),
            "shape 12"
        );
        assert_eq!(
            sheet_text(&doc, SheetRef::Page(PageId { index: 0 })),
            "page 5"
        );
        // A sheet the document does not hold falls back to the arena id.
        assert_eq!(
            sheet_text(&doc, SheetRef::Shape(ShapeId { index: 7 })),
            "shape #7"
        );
        assert_eq!(
            sheet_text(&doc, SheetRef::Page(PageId { index: 7 })),
            "page #7"
        );
    }

    #[test]
    fn value_text_names_every_value_kind() {
        assert_eq!(value_text(&CellValue::Number(1.5)), "1.5");
        assert_eq!(value_text(&CellValue::Str("wide".to_owned())), "wide");
        assert_eq!(value_text(&CellValue::Bool(true)), "true");
        assert_eq!(
            value_text(&CellValue::Color(Color::new(1, 2, 3))),
            "#010203"
        );
        assert_eq!(value_text(&CellValue::List("1;2".to_owned())), "1;2");
        assert_eq!(
            value_text(&CellValue::Error("#VALUE!".to_owned())),
            "#VALUE!"
        );
    }

    #[test]
    fn classify_maps_unprefixed_and_non_failed_errors_to_render() {
        // A `Failed` message without a known prefix keeps its text.
        assert_eq!(
            classify(&CommandError::failed("plain cause")),
            Error::Render("plain cause".to_owned())
        );
        // Any other variant is a render failure carrying its display text.
        assert_eq!(
            classify(&CommandError::invalid_args("no page")),
            Error::Render("bad arguments: no page".to_owned())
        );
    }
}
