//! The `file.*` commands (ADR 0026, ADR 0036): new, open, save, close,
//! quit, inspect, render and export.
//!
//! The open path accepts both format families (task P8-09): the XML
//! packages `.vsdx`/`.vssx`/`.vstx` and the binary `.vsd`/`.vss`/`.vst`
//! files, which route to `larrygram-vsd-binary`'s `import` (ADR 0070).
//! The binary formats are read-only: a document opened from one saves
//! as its XML counterpart (the document-kind mapping of the writer).
//!
//! The `inspect`, `render` and `export` bodies are the ADR 0026 CLI
//! seam: the CLI's `Command` enum dispatches here, so the CLI, the
//! control channel (Phase 8) and MCP share one implementation. Failures
//! carry the ADR 0026 one-line report (`cannot read input: ...`,
//! `cannot write output: ...`, `render failed: ...`); the CLI maps
//! those prefixes to its exit codes.

use std::path::{Path, PathBuf};

use larrygram_model::{CellValue, Document, DocumentKind, Page, PageId, SheetRef};
use larrygram_render::{
    BuildOptions, PngOptions, Rgba, Scene, SvgOptions, build_scene, render_png, render_svg,
};
use larrygram_vsd_binary as binary;
use larrygram_vsdx::{Limits, Package, read_package, to_model, write_document};

use crate::app::{AppCtx, DocumentSession, ViewState};
use crate::error::{CommandError, NeededInput, PathPurpose};
use crate::{Invocation, Outcome, UiRequest};

use super::{no_document, outcome, record_warnings};

/// The default device scale: 96 pixels per inch (ADR 0025).
pub(crate) const DEFAULT_SCALE: f64 = larrygram_render::build::DEFAULT_SCALE;

/// The output formats of `render` and `export` (ADR 0026).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Format {
    /// PNG raster (ADR 0025).
    Png,
    /// SVG document (ADR 0025).
    Svg,
}

impl Format {
    /// The file extension written for this format.
    fn extension(self) -> &'static str {
        match self {
            Format::Png => "png",
            Format::Svg => "svg",
        }
    }
}

/// What the `background` argument asks for (ADR 0026 `--background`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Background {
    /// No argument: the page's own background.
    Keep,
    /// `none`: a transparent page.
    Transparent,
    /// `#RRGGBB`: an opaque page colour.
    Color(Rgba),
}

// ---------------------------------------------------------------------------
// Arguments (the ADR 0028 names)
// ---------------------------------------------------------------------------

/// The `path` argument, or [`CommandError::NeedsInput`] (ADR 0028).
pub(crate) fn need_path(
    args: &crate::Args,
    name: &str,
    purpose: PathPurpose,
) -> Result<PathBuf, CommandError> {
    match args.path(name) {
        Some(path) => Ok(path.to_owned()),
        None => Err(CommandError::NeedsInput(NeededInput::Path {
            purpose,
            suggested: None,
        })),
    }
}

/// The `page` argument (1-based), or `None` when the caller left it out.
///
/// A number outside `1..=u32::MAX` is an argument error, and every
/// error carries the caller's number (P4-15 review minor 6: `page: 0`
/// is rejected instead of silently selecting the first page, and no
/// saturated value is ever reported back).
fn optional_page(args: &crate::Args) -> Result<Option<u32>, CommandError> {
    match args.integer("page") {
        Some(number) if number < 1 => Err(CommandError::invalid_args(format!(
            "page must be counted from 1, got {number}"
        ))),
        Some(number) => u32::try_from(number).map(Some).map_err(|_| {
            CommandError::invalid_args(format!(
                "page must be counted from 1 and at most {}, got {number}",
                u32::MAX
            ))
        }),
        None => Ok(None),
    }
}

/// The `page` argument (1-based): the first page when left out.
fn page_arg(args: &crate::Args) -> Result<u32, CommandError> {
    Ok(optional_page(args)?.unwrap_or(1))
}

/// The `format` argument, or the format named by `out`'s extension
/// (ADR 0026: "the output extension, overridable with --format").
fn format_arg(args: &crate::Args, out: &Path) -> Result<Format, CommandError> {
    match args.text("format") {
        Some("png") => Ok(Format::Png),
        Some("svg") => Ok(Format::Svg),
        Some(other) => Err(CommandError::invalid_args(format!(
            "unknown format: {other}; expected png or svg"
        ))),
        None => match out.extension().and_then(|e| e.to_str()) {
            Some("svg") | Some("SVG") => Ok(Format::Svg),
            _ => Ok(Format::Png),
        },
    }
}

/// The `scale` argument (pixels per inch), 96 when left out (ADR 0025).
fn scale_arg(args: &crate::Args) -> Result<f64, CommandError> {
    match args.number("scale") {
        Some(scale) if scale.is_finite() && scale > 0.0 => Ok(scale),
        Some(_) => Err(CommandError::invalid_args(
            "scale must be finite and positive",
        )),
        None => Ok(DEFAULT_SCALE),
    }
}

/// The `background` argument (ADR 0026 `--background #RRGGBB|none`).
fn background_arg(args: &crate::Args) -> Result<Background, CommandError> {
    let Some(text) = args.text("background") else {
        return Ok(Background::Keep);
    };
    if text == "none" {
        return Ok(Background::Transparent);
    }
    if let Some(hex) = text.strip_prefix('#')
        && hex.len() == 6
        && let Ok(rgb) = u32::from_str_radix(hex, 16)
    {
        return Ok(Background::Color(Rgba::opaque(
            ((rgb >> 16) & 0xFF) as u8,
            ((rgb >> 8) & 0xFF) as u8,
            (rgb & 0xFF) as u8,
        )));
    }
    Err(CommandError::invalid_args(format!(
        "background must be #RRGGBB or none, got {text:?}"
    )))
}

/// The `json` flag of `inspect`, false when left out.
fn json_arg(args: &crate::Args) -> bool {
    args.flag("json").unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Domain failures (the ADR 0026 report lines)
// ---------------------------------------------------------------------------

/// A failure that names a file (`cannot read input: <label>: <cause>`).
fn read_error(label: &Path, cause: impl std::fmt::Display) -> CommandError {
    read_error_text(format!("{}: {cause}", label.display()))
}

/// A read failure without a file name (a page number, an empty export).
fn read_error_text(message: impl Into<String>) -> CommandError {
    CommandError::failed(format!("cannot read input: {}", message.into()))
}

/// A write failure without a file name (`cannot write output: <cause>`,
/// the ADR 0026 prefix the CLI maps to exit code 3).
fn write_error_text(cause: impl std::fmt::Display) -> CommandError {
    CommandError::failed(format!("cannot write output: {cause}"))
}

/// A write failure (`cannot write output: <label>: <cause>`).
fn io_error(label: &Path, cause: impl std::fmt::Display) -> CommandError {
    write_error_text(format!("{}: {cause}", label.display()))
}

/// A render failure (`render failed: <cause>`).
fn render_error(cause: impl std::fmt::Display) -> CommandError {
    CommandError::failed(format!("render failed: {cause}"))
}

// ---------------------------------------------------------------------------
// Input and output formats (task P8-09)
// ---------------------------------------------------------------------------

/// The binary extensions the open path accepts (read-only).
const BINARY_EXTENSIONS: &[&str] = &["vsd", "vss", "vst"];

/// The lower-cased extension of `path`, empty when there is none.
fn extension_text(path: &Path) -> String {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .unwrap_or_default()
}

/// Whether `path` names a binary `.vsd`, `.vss` or `.vst` file.
fn is_binary(path: &Path) -> bool {
    let extension = extension_text(path);
    BINARY_EXTENSIONS.iter().any(|ext| *ext == extension)
}

/// The XML extension of a document kind: the same mapping the writer's
/// content types use (`larrygram-vsdx`, ADR 0070).
fn xml_extension(kind: DocumentKind) -> &'static str {
    match kind {
        DocumentKind::Stencil => "vssx",
        DocumentKind::Template => "vstx",
        _ => "vsdx",
    }
}

/// Where `file.save` writes a document whose session path is `path`:
/// the binary formats are read-only, so a `.vsd`/`.vss`/`.vst` path
/// maps to the XML format of the document kind.
fn save_target(path: &Path, kind: DocumentKind) -> PathBuf {
    if is_binary(path) {
        path.with_extension(xml_extension(kind))
    } else {
        path.to_owned()
    }
}

/// The default save name of a document kind ("drawing.vsdx").
fn default_save_name(kind: DocumentKind) -> &'static str {
    match kind {
        DocumentKind::Stencil => "stencil.vssx",
        DocumentKind::Template => "template.vstx",
        _ => "drawing.vsdx",
    }
}

/// The read-only refusal of a save target in a binary format (the ADR
/// 0026 `cannot write output:` prefix; task P8-09).
fn read_only_save_error(path: &Path, kind: DocumentKind) -> CommandError {
    write_error_text(format!(
        "{}: the binary Visio formats are read-only; save as {}",
        path.display(),
        save_target(path, kind).display()
    ))
}

/// One import warning as a status line (ADR 0071: every degradation
/// surfaces, never silently).
fn import_warning_line(warning: &binary::ImportWarning) -> String {
    let what = match &warning.kind {
        binary::WarningKind::SkippedRecord { name } => format!("skipped record {name}"),
        binary::WarningKind::Dropped { what } => format!("dropped {what}"),
        binary::WarningKind::UnresolvedReference { what } => format!("unresolved {what}"),
        binary::WarningKind::BadValue { what } => format!("unparsable {what}, using a default"),
        binary::WarningKind::Unsupported { what } => format!("unsupported {what}"),
        other => format!("{other:?}"),
    };
    format!("{}: {what}", warning.context)
}

// ---------------------------------------------------------------------------
// Reading and writing
// ---------------------------------------------------------------------------

/// Reads a document and keeps its package for the lossless writer
/// (ADR 0018).
///
/// The fill-in pass (ADR 0057) runs on the model before it is returned:
/// a cell with a formula and no cached value gains one, so every
/// consumer of a read document (the session, `render`, `export`,
/// `inspect`) sees the same values.
fn read_package_document(
    ctx: &mut AppCtx,
    bytes: &[u8],
    label: &Path,
) -> Result<(Document, Package), CommandError> {
    let package =
        read_package(bytes, &Limits::DEFAULT).map_err(|error| read_error(label, error))?;
    let mut document = to_model(&package).map_err(|error| read_error(label, error))?;
    fill_read_document(ctx, &mut document);
    Ok((document, package))
}

/// Fills the cached values a read document is missing (ADR 0057) with
/// the one fill-in pass of the workspace,
/// [`commands::recalc::fill_missing`](crate::recalc::fill_missing)
/// (task P6-18 F10: the copies `fill_in_values`/`missing_value_cells`
/// kept here are gone).
///
/// The pass evaluates every cell that has a formula and no cached value
/// and writes what it computes. A cell that has a value keeps it: the
/// file's cache is Visio's own last computation and the cross-check
/// measures against it (ADR 0059). The pass runs at load, before the
/// document is opened, so it takes no undo entry and does not bump the
/// change counter: the document sits in a transient session slot the
/// helper removes again (nothing changes about the open documents or
/// the active one).
///
/// The pass never fails the read: a formula the engine cannot evaluate
/// keeps its cell as it was, and the pass outcome is dropped. Its
/// warning lines are dropped with it: they belong to editing (ADR
/// 0056), not to opening a file.
fn fill_read_document(ctx: &mut AppCtx, document: &mut Document) {
    let mut slot = DocumentSession::new(std::mem::replace(document, Document::new()), None);
    let session = slot.id;
    slot.graph = None;
    ctx.documents.push(slot);
    let warnings = ctx.warnings.clone();
    let _ = crate::recalc::fill_missing(ctx, session);
    ctx.warnings = warnings;
    if let Some(slot) = ctx.documents.pop() {
        *document = slot.document;
    }
}

/// Reads and parses the input file in the format its extension names
/// (ADR 0026 exit code 2; task P8-09): a binary `.vsd`, `.vss` or
/// `.vst` goes to `larrygram-vsd-binary`, every other extension to the
/// `.vsdx` reader. The import warnings come back with the document.
fn load(
    ctx: &mut AppCtx,
    path: &Path,
) -> Result<(Document, Option<Package>, Vec<String>), CommandError> {
    let bytes = std::fs::read(path).map_err(|error| read_error(path, error))?;
    read_input_document(ctx, &bytes, path)
}

/// Reads one input file in the format `label`'s extension names (task
/// P8-09). `label` names the file in every error; on wasm it is the
/// upload's file name (ADR 0036).
///
/// The document lands in the same model either way, so the editor,
/// `inspect`, `render` and `export` see one shape of a document
/// (ADR 0070). Every import warning is returned as one status line.
fn read_input_document(
    ctx: &mut AppCtx,
    bytes: &[u8],
    label: &Path,
) -> Result<(Document, Option<Package>, Vec<String>), CommandError> {
    if is_binary(label) {
        let imported = binary::import(bytes, &binary::Limits::DEFAULT)
            .map_err(|error| read_error(label, error))?;
        let warnings: Vec<String> = imported.warnings.iter().map(import_warning_line).collect();
        let mut document = imported.document;
        fill_read_document(ctx, &mut document);
        Ok((document, None, warnings))
    } else {
        let (document, package) = read_package_document(ctx, bytes, label)?;
        Ok((document, Some(package), Vec::new()))
    }
}

/// Writes one output file (ADR 0026 exit code 3).
fn write_file(path: &Path, bytes: &[u8]) -> Result<(), CommandError> {
    std::fs::write(path, bytes).map_err(|error| io_error(path, error))
}

/// The selected page (1-based), or the first one.
fn select_page(doc: &Document, page: Option<u32>) -> Result<&Page, CommandError> {
    let number = page.unwrap_or(1);
    doc.pages
        .get(number.saturating_sub(1) as usize)
        .ok_or_else(|| {
            read_error_text(format!(
                "page {number} does not exist; the document has {} pages",
                doc.pages.len()
            ))
        })
}

/// Builds the scene of one page (ADR 0020/0021).
fn build(doc: &Document, page: PageId, scale: f64) -> Result<Scene, CommandError> {
    let mut options = BuildOptions::new();
    options.scale = scale;
    build_scene(doc, page, &options).map_err(render_error)
}

/// Encodes a scene in the requested format (ADR 0025).
fn encode(scene: &Scene, format: Format, scale: f64) -> Result<Vec<u8>, CommandError> {
    match format {
        Format::Png => {
            let options = PngOptions {
                scale,
                antialias: true,
            };
            render_png(scene, &options).map_err(render_error)
        }
        Format::Svg => {
            let options = SvgOptions { scale };
            let svg = render_svg(scene, &options).map_err(render_error)?;
            Ok(svg.into_bytes())
        }
    }
}

// ---------------------------------------------------------------------------
// The commands
// ---------------------------------------------------------------------------

/// `file.new`: an empty document session, which becomes active.
///
/// The session's view toggles are seeded from the settings defaults
/// (ADR 0032 amendment, P4-15 review major 1).
pub(super) fn run_new(ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
    let mut session = DocumentSession::new(Document::new(), None);
    session.view = ViewState::from_settings(&ctx.settings);
    ctx.open_document(session);
    Ok(outcome("New drawing", Vec::new()))
}

/// `file.open`: reads `path` (or `name` + `data` on wasm, ADR 0036)
/// into a new session, which becomes active. The session's view
/// toggles are seeded from the settings defaults (ADR 0032 amendment).
///
/// The format follows the extension (task P8-09): `.vsd`, `.vss` and
/// `.vst` import through `larrygram-vsd-binary`, the rest through the
/// `.vsdx` reader. Every import warning surfaces as a status line
/// (ADR 0071).
pub(super) fn run_open(ctx: &mut AppCtx, inv: &Invocation) -> Result<Outcome, CommandError> {
    let (document, package, warnings, path, label) = match inv.args.path("path") {
        Some(path) => {
            let bytes = std::fs::read(path).map_err(|error| read_error(path, error))?;
            let (document, package, warnings) = read_input_document(ctx, &bytes, path)?;
            (
                document,
                package,
                warnings,
                Some(path.to_owned()),
                path.display().to_string(),
            )
        }
        None => match (inv.args.text("name"), inv.args.bytes("data")) {
            (Some(name), Some(data)) => {
                let (document, package, warnings) =
                    read_input_document(ctx, data, Path::new(name))?;
                (document, package, warnings, None, name.to_owned())
            }
            _ => {
                return Err(CommandError::NeedsInput(NeededInput::Path {
                    purpose: PathPurpose::OpenFile,
                    suggested: None,
                }));
            }
        },
    };
    let recent = path.clone();
    let mut session = DocumentSession::new(document, path);
    if let Some(package) = package {
        session = session.with_preserve(package);
    }
    session.view = ViewState::from_settings(&ctx.settings);
    ctx.open_document(session);
    if let Some(path) = recent {
        remember_recent(&mut ctx.settings, path);
    }
    record_warnings(ctx, &warnings);
    Ok(outcome(format!("Opened {label}"), warnings))
}

/// Records a recently opened file: most recent first, de-duplicated and
/// capped at [`RECENT_FILES_MAX`] entries (P4-02, task P5-09).
fn remember_recent(settings: &mut crate::Settings, path: PathBuf) {
    settings.recent_files.retain(|entry| entry != &path);
    settings.recent_files.insert(0, path);
    settings.recent_files.truncate(RECENT_FILES_MAX);
}

/// How many recently opened files the settings keep.
const RECENT_FILES_MAX: usize = 10;

/// `file.save`: writes the active document to its `path`.
///
/// A never-saved document has no target: interactive callers get
/// [`CommandError::NeedsInput`] (the desktop save dialog, ADR 0036) and
/// the wasm build returns the package in [`Outcome::payload`] for the
/// browser download (ADR 0036).
pub(super) fn run_save(ctx: &mut AppCtx, inv: &Invocation) -> Result<Outcome, CommandError> {
    save_document(ctx, inv, false)
}

/// `file.save_as`: like `file.save`, and the `path` argument becomes
/// the session's `path`.
pub(super) fn run_save_as(ctx: &mut AppCtx, inv: &Invocation) -> Result<Outcome, CommandError> {
    save_document(ctx, inv, true)
}

/// The shared body of `file.save` and `file.save_as`.
fn save_document(
    ctx: &mut AppCtx,
    inv: &Invocation,
    set_path: bool,
) -> Result<Outcome, CommandError> {
    let arg_path = inv.args.path("path").map(PathBuf::from);
    // On wasm there is no file system: a `path` argument still hands
    // the package back for the browser download (P4-02, task P5-09)
    // instead of failing inside `std::fs::write`.
    #[cfg(target_arch = "wasm32")]
    if arg_path.is_some() {
        return payload_save(ctx);
    }
    let kind = ctx
        .active_document()
        .map(|session| session.document.kind)
        .unwrap_or_default();
    // The binary formats are read-only (task P8-09): they name an
    // input, never a save target.
    if let Some(path) = &arg_path
        && is_binary(path)
    {
        return Err(read_only_save_error(path, kind));
    }
    // The target: the `path` argument, or the session's own path
    // mapped out of the read-only binary format (task P8-09), so a
    // document opened from `plan.vsd` saves as `plan.vsdx`.
    let own = ctx
        .active_document()
        .and_then(|session| session.path.clone());
    let mapped = arg_path.is_none() && own.as_deref().is_some_and(is_binary);
    let target = arg_path
        .clone()
        .or_else(|| own.as_deref().map(|path| save_target(path, kind)));
    let Some(path) = target else {
        return save_without_path(ctx);
    };
    let session = ctx.active_document_mut().ok_or_else(no_document)?;
    let empty = Package::new();
    let preserve = session.preserve.as_ref().unwrap_or(&empty);
    let encoded = write_document(&session.document, preserve);
    let bytes = encoded.map_err(|error| io_error(&path, error))?;
    write_file(&path, &bytes)?;
    // A mapped save re-homes the session on the file it just wrote, so
    // the next save goes to the same place and the title follows.
    if set_path || mapped {
        session.path = Some(path.clone());
    }
    session.dirty = false;
    Ok(outcome(format!("Saved {}", path.display()), Vec::new()))
}

/// The no-target save of a never-saved document on the desktop: the
/// caller gets [`CommandError::NeedsInput`] and picks a path (ADR
/// 0036). The suggested name follows the document kind (task P8-09:
/// `stencil.vssx`, `template.vstx`, `drawing.vsdx`).
#[cfg(not(target_arch = "wasm32"))]
fn save_without_path(ctx: &mut AppCtx) -> Result<Outcome, CommandError> {
    let kind = ctx
        .active_document()
        .map(|session| session.document.kind)
        .unwrap_or_default();
    Err(CommandError::NeedsInput(NeededInput::Path {
        purpose: PathPurpose::SaveFile,
        suggested: Some(PathBuf::from(default_save_name(kind))),
    }))
}

/// The no-target save on wasm: there is no disk to write to, so the
/// serialized package goes back in [`Outcome::payload`] and the web
/// app downloads it (ADR 0036).
#[cfg(target_arch = "wasm32")]
fn save_without_path(ctx: &mut AppCtx) -> Result<Outcome, CommandError> {
    payload_save(ctx)
}

/// The body of the no-target save, kept on every target so the host
/// tests cover the wasm download path (P4-15 review minor 7): a
/// `write_document` failure is a write failure with the ADR 0026
/// `cannot write output:` prefix, not a `render failed:` one.
#[cfg(any(test, target_arch = "wasm32"))]
fn payload_save(ctx: &mut AppCtx) -> Result<Outcome, CommandError> {
    let session = ctx.active_document_mut().ok_or_else(no_document)?;
    let empty = Package::new();
    let preserve = session.preserve.as_ref().unwrap_or(&empty);
    let bytes = write_document(&session.document, preserve).map_err(write_error_text)?;
    let name = download_name(&session.display_name(), session.document.kind);
    session.dirty = false;
    Ok(Outcome {
        message: Some(format!("Saved {name}")),
        payload: Some(crate::Payload::Bytes { name, data: bytes }),
        ..Outcome::default()
    })
}

/// The suggested download name of a never-saved document (task P8-09:
/// a binary source name maps to the XML format of its kind).
#[cfg(any(test, target_arch = "wasm32"))]
fn download_name(display_name: &str, kind: DocumentKind) -> String {
    let mapped = save_target(Path::new(display_name), kind);
    let name = mapped.to_string_lossy().into_owned();
    if name.contains('.') {
        name
    } else {
        format!("{name}.{}", xml_extension(kind))
    }
}

/// `file.close`: closes the active session.
///
/// Unsaved work on an interactive source returns
/// [`UiRequest::ConfirmClose`] instead of closing (AGENTS.md section 6:
/// the command never opens the dialog itself); the caller answers by
/// dispatching `file.close` again with `confirm: true`. Programmatic
/// sources close without asking.
pub(super) fn run_close(ctx: &mut AppCtx, inv: &Invocation) -> Result<Outcome, CommandError> {
    let Some(index) = ctx.active else {
        return Err(no_document());
    };
    let confirm = inv.args.flag("confirm").unwrap_or(false);
    let (name, dirty) = match ctx.active_document() {
        Some(session) => (session.display_name(), session.dirty),
        None => return Err(no_document()),
    };
    if dirty && inv.source.is_interactive() && !confirm {
        return Ok(Outcome {
            message: Some(format!("Unsaved changes in {name}")),
            ui_request: Some(UiRequest::ConfirmClose { name }),
            ..Outcome::default()
        });
    }
    ctx.close_document(index);
    Ok(outcome(format!("Closed {name}"), Vec::new()))
}

/// `app.quit` and `file.exit`: asks the dispatcher to quit (P4-15; one
/// handler for both ids, so "one quit id" is answered by binding both).
///
/// The command closes nothing itself and never exits a process: an
/// interactive dispatcher turns [`UiRequest::Quit`] into a viewport
/// close, a programmatic caller records it and keeps running
/// (AGENTS.md section 6).
pub(super) fn run_quit(_ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
    Ok(Outcome {
        message: Some("Quit".to_owned()),
        ui_request: Some(UiRequest::Quit),
        ..Outcome::default()
    })
}

/// `file.inspect`: the ADR 0026 document summary in
/// [`Outcome::message`], as text or (with `json`) as one JSON object.
///
/// `path` reads a file; without it the active document is summarised,
/// and a caller without either gets [`CommandError::NeedsInput`].
pub(super) fn run_inspect(ctx: &mut AppCtx, inv: &Invocation) -> Result<Outcome, CommandError> {
    let json = json_arg(&inv.args);
    let page = optional_page(&inv.args)?;
    let (text, warnings) = match inv.args.path("path") {
        Some(path) => {
            let (document, _package, mut warnings) = load(ctx, path)?;
            let (text, scene_warnings) = inspect_document(&document, page, json)?;
            warnings.extend(scene_warnings);
            (text, warnings)
        }
        None => match ctx.active_document() {
            Some(session) => inspect_document(&session.document, page, json)?,
            None => {
                return Err(CommandError::NeedsInput(NeededInput::Path {
                    purpose: PathPurpose::OpenFile,
                    suggested: None,
                }));
            }
        },
    };
    record_warnings(ctx, &warnings);
    Ok(outcome(text, warnings))
}

/// The summary body of `file.inspect`.
fn inspect_document(
    doc: &Document,
    page: Option<u32>,
    json: bool,
) -> Result<(String, Vec<String>), CommandError> {
    let pages: Vec<(usize, &Page)> = match page {
        Some(number) => vec![(number as usize, select_page(doc, Some(number))?)],
        None => doc
            .pages
            .iter()
            .enumerate()
            .map(|(index, page)| (index.saturating_add(1), page))
            .collect(),
    };
    let mut summaries = Vec::new();
    let mut warnings = Vec::new();
    for (number, page) in pages {
        let scene = build(doc, page.id, DEFAULT_SCALE)?;
        warnings.extend(scene.warnings.iter().map(|w| warning_line(doc, w)));
        summaries.push(PageSummary {
            index: number,
            name: page.name.clone(),
            width: scene.width,
            height: scene.height,
            shapes: shape_count(doc, page),
            warnings: scene.warnings.len(),
        });
    }
    let summary = Summary {
        kind: kind_text(doc.kind),
        pages: summaries,
        masters: doc.masters.len(),
        styles: doc.stylesheet.styles.len(),
        themes: doc.themes.len(),
    };
    let text = if json { summary.json() } else { summary.text() };
    Ok((text, warnings))
}

/// `file.render`: renders one page to `out` (ADR 0026).
pub(super) fn run_render(ctx: &mut AppCtx, inv: &Invocation) -> Result<Outcome, CommandError> {
    let path = need_path(&inv.args, "path", PathPurpose::OpenFile)?;
    let out = need_path(&inv.args, "out", PathPurpose::SaveFile)?;
    let format = format_arg(&inv.args, &out)?;
    let scale = scale_arg(&inv.args)?;
    let background = background_arg(&inv.args)?;
    let page = page_arg(&inv.args)?;
    let (document, _package, mut warnings) = load(ctx, &path)?;
    let selected = select_page(&document, Some(page))?;
    let mut scene = build(&document, selected.id, scale)?;
    match background {
        Background::Keep => {}
        Background::Transparent => scene.background = None,
        Background::Color(color) => scene.background = Some(color),
    }
    let scene_warnings: Vec<String> = scene
        .warnings
        .iter()
        .map(|warning| warning_line(&document, warning))
        .collect();
    warnings.extend(scene_warnings);
    let bytes = encode(&scene, format, scale)?;
    write_file(&out, &bytes)?;
    record_warnings(ctx, &warnings);
    Ok(outcome(format!("Rendered {}", out.display()), warnings))
}

/// `file.export`: renders every page into `out` as `page-<N>.<ext>`
/// (ADR 0026).
pub(super) fn run_export(ctx: &mut AppCtx, inv: &Invocation) -> Result<Outcome, CommandError> {
    let path = need_path(&inv.args, "path", PathPurpose::OpenFile)?;
    let out = need_path(&inv.args, "out", PathPurpose::SaveFile)?;
    let format = format_arg(&inv.args, &out)?;
    let scale = scale_arg(&inv.args)?;
    let (document, _package, mut warnings) = load(ctx, &path)?;
    if document.pages.is_empty() {
        return Err(read_error_text("the document has no pages to export"));
    }
    std::fs::create_dir_all(&out).map_err(|error| io_error(&out, error))?;
    for (index, page) in document.pages.iter().enumerate() {
        let number = index.saturating_add(1);
        let scene = build(&document, page.id, scale)?;
        let page_warnings: Vec<String> = scene
            .warnings
            .iter()
            .map(|warning| warning_line(&document, warning))
            .collect();
        record_warnings(ctx, &page_warnings);
        warnings.extend(page_warnings);
        let bytes = encode(&scene, format, scale)?;
        write_file(
            &out.join(format!("page-{number}.{}", format.extension())),
            &bytes,
        )?;
    }
    Ok(outcome(
        format!(
            "Exported {} pages to {}",
            document.pages.len(),
            out.display()
        ),
        warnings,
    ))
}

// ---------------------------------------------------------------------------
// The inspect summary (ADR 0026) and the warning report (ADR 0021)
// ---------------------------------------------------------------------------

/// One page row of the inspect summary (ADR 0026).
pub struct PageSummary {
    /// The page number, counted from 1 in document order.
    pub index: usize,
    /// The page name, as written in the file.
    pub name: String,
    /// The page width in inches.
    pub width: f64,
    /// The page height in inches.
    pub height: f64,
    /// The number of shapes on the page, groups included.
    pub shapes: usize,
    /// The number of rendering warnings of the page (ADR 0021).
    pub warnings: usize,
}

/// The inspect summary of a document (ADR 0026).
pub struct Summary {
    /// The document kind: `drawing`, `stencil` or `template`.
    pub kind: &'static str,
    /// The pages, in document order.
    pub pages: Vec<PageSummary>,
    /// The number of masters.
    pub masters: usize,
    /// The number of stylesheet rows.
    pub styles: usize,
    /// The number of theme parts.
    pub themes: usize,
}

impl Summary {
    /// The total rendering warning count of the summarised pages.
    pub fn warnings(&self) -> usize {
        self.pages.iter().map(|page| page.warnings).sum()
    }

    /// The compact text form (ADR 0026).
    pub fn text(&self) -> String {
        let mut text = String::new();
        text.push_str(&format!("kind: {}\n", self.kind));
        text.push_str(&format!("pages: {}\n", self.pages.len()));
        for page in &self.pages {
            let name = if page.name.is_empty() {
                "(unnamed)"
            } else {
                page.name.as_str()
            };
            text.push_str(&format!(
                "  {}: {} {} x {} in, {} shapes, {} warnings\n",
                page.index, name, page.width, page.height, page.shapes, page.warnings
            ));
        }
        text.push_str(&format!("masters: {}\n", self.masters));
        text.push_str(&format!("styles: {}\n", self.styles));
        text.push_str(&format!("themes: {}\n", self.themes));
        text.push_str(&format!("warnings: {}\n", self.warnings()));
        text
    }

    /// The JSON form: one object, stable key order (ADR 0026).
    pub fn json(&self) -> String {
        let value = serde_json::json!({
            "kind": self.kind,
            "pages": self.pages.iter().map(|page| serde_json::json!({
                "index": page.index,
                "name": page.name,
                "width": finite(page.width),
                "height": finite(page.height),
                "shapes": page.shapes,
                "warnings": page.warnings,
            })).collect::<Vec<_>>(),
            "masters": self.masters,
            "styles": self.styles,
            "themes": self.themes,
            "warnings": self.warnings(),
        });
        serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string())
    }
}

/// A finite JSON number, or 0 (serde_json rejects NaN and infinity).
fn finite(value: f64) -> f64 {
    if value.is_finite() { value } else { 0.0 }
}

/// The document kind as a word (ADR 0026).
pub fn kind_text(kind: DocumentKind) -> &'static str {
    match kind {
        DocumentKind::Drawing => "drawing",
        DocumentKind::Stencil => "stencil",
        DocumentKind::Template => "template",
        _ => "unknown",
    }
}

/// The number of shapes on a page, group sub-shapes included.
///
/// A broken or cyclic `children` list is walked once, never looped on.
pub fn shape_count(doc: &Document, page: &Page) -> usize {
    let mut seen = vec![false; doc.shapes.len()];
    let mut count = 0usize;
    let mut stack: Vec<larrygram_model::ShapeId> = page.shapes.to_vec();
    while let Some(id) = stack.pop() {
        let Some(slot) = seen.get_mut(id.index as usize) else {
            continue;
        };
        if *slot {
            continue;
        }
        *slot = true;
        count = count.saturating_add(1);
        if let Some(shape) = doc.shape(id) {
            stack.extend(shape.children.iter().copied());
        }
    }
    count
}

/// One warning line, e.g. `shape 12 LineWeight: no cached value, using 0`
/// (ADR 0021).
pub fn warning_line(doc: &Document, warning: &larrygram_model::CellWarning) -> String {
    let used = value_text(&warning.substituted);
    let reason = match &warning.kind {
        larrygram_model::WarningKind::Missing => format!("no cached value, using {used}"),
        larrygram_model::WarningKind::ErrorValue(error) => {
            format!("cached error {error:?}, using {used}")
        }
        larrygram_model::WarningKind::KindMismatch => format!("wrong kind, using {used}"),
        larrygram_model::WarningKind::UnresolvedTheme => {
            format!("the theme cannot supply it, using {used}")
        }
        _ => format!("value unavailable, using {used}"),
    };
    format!(
        "{} {}: {reason}",
        sheet_text(doc, warning.sheet),
        warning.cell.as_str()
    )
}

/// The sheet of a warning as `shape 12` or `page 2`.
pub fn sheet_text(doc: &Document, sheet: SheetRef) -> String {
    match sheet {
        SheetRef::Shape(id) => match doc.shape(id) {
            Some(shape) => format!("shape {}", shape.visio_id),
            None => format!("shape #{}", id.index),
        },
        SheetRef::Page(id) => match doc.page(id) {
            Some(page) => format!("page {}", page.visio_id),
            None => format!("page #{}", id.index),
        },
    }
}

/// The substituted value of a warning, for the report line.
pub fn value_text(value: &CellValue) -> String {
    match value {
        CellValue::Number(number) => format!("{number}"),
        CellValue::Str(text) => text.clone(),
        CellValue::Bool(flag) => flag.to_string(),
        CellValue::Color(color) => format!("#{:02X}{:02X}{:02X}", color.r, color.g, color.b),
        CellValue::List(text) => text.clone(),
        CellValue::Error(text) => text.clone(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use larrygram_model::{Cell, CellName, Row, Section, SectionName, Shape, ShapeId};
    use larrygram_vsdx::{ContentType, Part, PartData, PartName, from_model};

    use crate::{ArgValue, Args, Source};

    /// A `Probe` cell with `formula` and `value`.
    fn probe_cell(name: &str, formula: Option<&str>, value: Option<CellValue>) -> Cell {
        let mut cell = Cell::new(CellName::new(name));
        cell.formula = formula.map(str::to_owned);
        cell.value = value;
        cell
    }

    /// The bytes of a one-shape package whose cells carry the given
    /// formulas and cached values.
    #[allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)] // test helper
    fn probe_bytes(cells: Vec<Cell>, sections: Vec<Section>) -> Vec<u8> {
        let mut shape = Shape::new(ShapeId { index: 0 }, 1);
        shape.sheet.cells = cells;
        shape.sheet.sections = sections;
        let mut doc = Document::new();
        let page = doc.add_page("Page-1").expect("page");
        doc.insert_shape(page, shape, None).expect("shape");
        let package = from_model(&doc).expect("from_model");
        write_document(&doc, &package).expect("write")
    }

    /// The value of the first shape's `name` cell.
    #[allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)] // test helper
    fn cell_value(document: &Document, name: &str) -> Option<CellValue> {
        let mut found = None;
        for shape in &document.shapes {
            for cell in &shape.sheet.cells {
                if cell.name.as_str() == name {
                    found = cell.value.clone();
                }
            }
            for section in &shape.sheet.sections {
                for cell in &section.cells {
                    if cell.name.as_str() == name {
                        found = cell.value.clone();
                    }
                }
                for row in &section.rows {
                    for cell in &row.cells {
                        if cell.name.as_str() == name {
                            found = cell.value.clone();
                        }
                    }
                }
            }
        }
        found
    }

    /// P6-08: a document with a formula and no cached value gains one
    /// after the fill-in pass (ADR 0057), wherever the cell sits: a
    /// sheet cell, a section cell or a row cell.
    #[test]
    fn a_formula_without_a_cached_value_gains_one_on_read() {
        let mut section = Section::new(SectionName::from_static("Geometry"));
        section
            .cells
            .push(probe_cell("SectionCell", Some("2+2"), None));
        let mut row = Row::new();
        row.row_type = Some("MoveTo".to_owned());
        row.ix = Some(0);
        row.cells.push(probe_cell("X", Some("3+3"), None));
        section.rows.push(row);
        let bytes = probe_bytes(
            vec![probe_cell("SheetCell", Some("1+1"), None)],
            vec![section],
        );
        let mut ctx = AppCtx::new(crate::Settings::default());
        let (document, _) =
            read_package_document(&mut ctx, &bytes, Path::new("probe.vsdx")).expect("read");
        assert_eq!(
            cell_value(&document, "SheetCell"),
            Some(CellValue::Number(2.0))
        );
        assert_eq!(cell_value(&document, "X"), Some(CellValue::Number(6.0)));
        // The section cell too (P6-11): a section-level cell resolves
        // against its section (the `graph::effective` dispatch), so the
        // fill-in pass evaluates it like any other cell.
        assert_eq!(
            cell_value(&document, "SectionCell"),
            Some(CellValue::Number(4.0))
        );
        for shape in &document.shapes {
            for section in &shape.sheet.sections {
                for cell in &section.cells {
                    assert_eq!(cell.name.as_str(), "SectionCell");
                    assert_eq!(cell.formula.as_deref(), Some("2+2"));
                }
            }
        }
    }

    /// P6-08: a document whose cells carry cached values is unchanged
    /// by the fill-in pass (ADR 0057: the cache is trusted).
    #[test]
    fn a_cached_value_is_unchanged_by_the_fill_in_pass() {
        let bytes = probe_bytes(
            vec![probe_cell(
                "Probe",
                Some("1+1"),
                Some(CellValue::Number(7.0)),
            )],
            Vec::new(),
        );
        let mut ctx = AppCtx::new(crate::Settings::default());
        let (document, _) =
            read_package_document(&mut ctx, &bytes, Path::new("probe.vsdx")).expect("read");
        assert_eq!(cell_value(&document, "Probe"), Some(CellValue::Number(7.0)));
    }

    /// P6-08: the fill-in pass runs at load, so `file.open` takes no
    /// undo entry and does not mark the session dirty (ADR 0057).
    #[test]
    fn the_fill_in_pass_takes_no_undo_entry() {
        let bytes = probe_bytes(vec![probe_cell("Probe", Some("1+1"), None)], Vec::new());
        let mut ctx = AppCtx::new(crate::Settings::default());
        let invocation = Invocation::new(
            Args::new()
                .with("name", ArgValue::Text("probe.vsdx".to_owned()))
                .with("data", ArgValue::Bytes(bytes)),
            Source::Ui,
        );
        let outcome = run_open(&mut ctx, &invocation).expect("open");
        assert!(outcome.message.is_some());
        let session = ctx.active_document().expect("session");
        assert!(
            session.undo.is_empty(),
            "the fill-in pass must take no undo entry"
        );
        assert!(!session.dirty, "the fill-in pass must not mark the session");
        assert_eq!(
            cell_value(&session.document, "Probe"),
            Some(CellValue::Number(2.0))
        );
    }

    /// Task P6-18 (the coverage report): `file.close` over a stale
    /// active index reports no open document, never a panic.
    #[test]
    fn close_with_a_stale_active_index_reports_no_document() {
        let mut ctx = AppCtx::new(crate::Settings::default());
        ctx.active = Some(5);
        let error =
            run_close(&mut ctx, &Invocation::new(Args::new(), Source::Test)).expect_err("stale");
        assert!(error.to_string().contains("no open document"), "{error}");
    }

    /// P4-15 review minor 7: the no-target save (the wasm download
    /// path, kept compiled on every target) returns a payload and
    /// reports a write failure with the ADR 0026 `cannot write
    /// output:` prefix, never a `render failed:` one.
    #[test]
    fn payload_save_returns_a_download_payload() {
        let mut ctx = AppCtx::new(crate::Settings::default());
        ctx.open_document(DocumentSession::new(Document::new(), None));
        let outcome = payload_save(&mut ctx).expect("payload");
        assert_eq!(outcome.message.as_deref(), Some("Saved Untitled.vsdx"));
        assert!(matches!(
            outcome.payload,
            Some(crate::Payload::Bytes { .. })
        ));
        if let Some(crate::Payload::Bytes { name, data }) = outcome.payload {
            assert_eq!(name, "Untitled.vsdx");
            assert!(!data.is_empty(), "a package was written");
        }
        let session = ctx.active_document().expect("session");
        assert!(!session.dirty);
    }

    #[test]
    fn payload_save_without_a_document_reports_no_open_document() {
        let mut ctx = AppCtx::new(crate::Settings::default());
        let error = payload_save(&mut ctx).expect_err("no document");
        assert_eq!(error.to_string(), "no open document");
    }

    #[test]
    fn payload_save_reports_a_write_failure_as_a_write_failure() {
        let mut preserve = Package::new();
        preserve.parts.push(Part {
            name: PartName::new("/not-a-visio-main.bin"),
            content_type: ContentType::new("application/octet-stream"),
            data: PartData::Binary(vec![1, 2, 3]),
        });
        let mut ctx = AppCtx::new(crate::Settings::default());
        let session = DocumentSession::new(Document::new(), None).with_preserve(preserve);
        ctx.open_document(session);
        let error = payload_save(&mut ctx).expect_err("the write fails");
        let message = error.to_string();
        assert!(message.starts_with("cannot write output:"), "{message}");
        assert!(!message.starts_with("render failed:"), "{message}");
    }

    #[test]
    fn recent_files_are_most_recent_first_deduplicated_and_capped() {
        let mut settings = crate::Settings::default();
        remember_recent(&mut settings, PathBuf::from("/a.vsdx"));
        remember_recent(&mut settings, PathBuf::from("/b.vsdx"));
        remember_recent(&mut settings, PathBuf::from("/a.vsdx"));
        assert_eq!(
            settings.recent_files,
            vec![PathBuf::from("/a.vsdx"), PathBuf::from("/b.vsdx")]
        );
        for index in 0..12 {
            remember_recent(&mut settings, PathBuf::from(format!("/{index}.vsdx")));
        }
        assert_eq!(settings.recent_files.len(), RECENT_FILES_MAX);
        assert_eq!(
            settings.recent_files.first(),
            Some(&PathBuf::from("/11.vsdx"))
        );
        let mut unique = settings.recent_files.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), settings.recent_files.len());
    }

    #[test]
    fn download_name_keeps_real_names_and_extends_bare_ones() {
        assert_eq!(
            download_name("drawing.vsdx", DocumentKind::Drawing),
            "drawing.vsdx"
        );
        assert_eq!(
            download_name("Untitled", DocumentKind::Drawing),
            "Untitled.vsdx"
        );
        // Task P8-09: a binary source name maps to its XML format.
        assert_eq!(
            download_name("plan.vsd", DocumentKind::Drawing),
            "plan.vsdx"
        );
        assert_eq!(
            download_name("parts.vss", DocumentKind::Stencil),
            "parts.vssx"
        );
        assert_eq!(
            download_name("Untitled", DocumentKind::Stencil),
            "Untitled.vssx"
        );
    }

    // -----------------------------------------------------------------------
    // The open path over both format families (task P8-09)
    // -----------------------------------------------------------------------

    /// The named committed fixture under `corpus/fixtures/` (always
    /// present, ADR 0005's committed table).
    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("corpus")
            .join("fixtures")
            .join(name)
    }

    /// The named fetched corpus `.vsd` (ADR 0005), or `None` when the
    /// corpus is absent: the test skips then, and fails instead when
    /// `LARRYGRAM_REQUIRE_CORPUS=1` is set (the Phase 3 convention).
    fn corpus_vsd(name: &str) -> Option<PathBuf> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("corpus")
            .join("vsd")
            .join(name);
        if path.is_file() {
            return Some(path);
        }
        assert!(
            std::env::var_os("LARRYGRAM_REQUIRE_CORPUS").is_none(),
            "corpus/vsd is missing and LARRYGRAM_REQUIRE_CORPUS=1 is set"
        );
        eprintln!("skipping: corpus/vsd is absent; run `cargo xtask corpus`");
        None
    }

    /// A private scratch directory for one test.
    fn scratch_dir() -> PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "larrygram-file-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).expect("scratch directory");
        dir
    }

    /// A `file.open` invocation for `path`.
    fn open_invocation(path: &Path) -> Invocation {
        Invocation::new(
            Args::new().with("path", ArgValue::Path(path.to_owned())),
            Source::Ui,
        )
    }

    #[test]
    fn the_binary_extensions_are_vsd_vss_and_vst() {
        assert!(is_binary(Path::new("a.vsd")));
        assert!(is_binary(Path::new("a.vss")));
        assert!(is_binary(Path::new("a.vst")));
        assert!(is_binary(Path::new("dir/A.VSD")), "case-insensitive");
        assert!(!is_binary(Path::new("a.vsdx")));
        assert!(!is_binary(Path::new("a.vssx")));
        assert!(!is_binary(Path::new("a.txt")));
        assert!(!is_binary(Path::new("vsd")), "no extension");
    }

    #[test]
    fn the_save_target_maps_the_binary_formats_to_their_xml_ones() {
        assert_eq!(
            save_target(Path::new("plan.vsd"), DocumentKind::Drawing),
            PathBuf::from("plan.vsdx")
        );
        assert_eq!(
            save_target(Path::new("parts.vss"), DocumentKind::Stencil),
            PathBuf::from("parts.vssx")
        );
        assert_eq!(
            save_target(Path::new("start.vst"), DocumentKind::Template),
            PathBuf::from("start.vstx")
        );
        assert_eq!(
            save_target(Path::new("plan.vsdx"), DocumentKind::Drawing),
            PathBuf::from("plan.vsdx"),
            "an XML path is its own target"
        );
    }

    #[test]
    fn the_default_save_names_follow_the_document_kind() {
        assert_eq!(default_save_name(DocumentKind::Drawing), "drawing.vsdx");
        assert_eq!(default_save_name(DocumentKind::Stencil), "stencil.vssx");
        assert_eq!(default_save_name(DocumentKind::Template), "template.vstx");
    }

    #[test]
    fn import_warning_lines_name_every_kind() {
        use binary::{ImportWarning, WarningKind};
        let cases = [
            (
                ImportWarning {
                    context: "page 1".to_owned(),
                    kind: WarningKind::SkippedRecord { name: "OLE data" },
                },
                "page 1: skipped record OLE data",
            ),
            (
                ImportWarning {
                    context: "shape 2".to_owned(),
                    kind: WarningKind::Dropped {
                        what: "embedded OLE object",
                    },
                },
                "shape 2: dropped embedded OLE object",
            ),
            (
                ImportWarning {
                    context: "shape 3".to_owned(),
                    kind: WarningKind::UnresolvedReference { what: "master 7" },
                },
                "shape 3: unresolved master 7",
            ),
            (
                ImportWarning {
                    context: "page 1".to_owned(),
                    kind: WarningKind::BadValue {
                        what: "fill colour",
                    },
                },
                "page 1: unparsable fill colour, using a default",
            ),
            (
                ImportWarning {
                    context: "document".to_owned(),
                    kind: WarningKind::Unsupported {
                        what: "version 5 chunk format",
                    },
                },
                "document: unsupported version 5 chunk format",
            ),
        ];
        for (warning, expected) in cases {
            assert_eq!(import_warning_line(&warning), expected);
        }
    }

    #[test]
    fn an_unsupported_version_error_names_the_reason() {
        let message =
            read_error(Path::new("old.vsd"), binary::Error::UnsupportedVersion(5)).to_string();
        assert!(message.contains("cannot read input: old.vsd"), "{message}");
        assert!(message.contains("unsupported file version 5"), "{message}");
    }

    /// The committed `.vsd` fixture opens through `file.open`, lands in
    /// the model and surfaces every import warning (task P8-09 goal 1).
    #[test]
    fn a_vsd_fixture_opens_through_the_open_command_with_its_warnings() {
        let path = fixture("minimal-drawing.vsd");
        let mut ctx = AppCtx::new(crate::Settings::default());
        let outcome = run_open(&mut ctx, &open_invocation(&path)).expect("open");
        assert_eq!(
            outcome.message.as_deref(),
            Some(format!("Opened {}", path.display()).as_str())
        );
        let session = ctx.active_document().expect("session");
        assert_eq!(session.document.pages.len(), 1, "pages landed in the model");
        assert_eq!(session.path.as_deref(), Some(path.as_path()));
        assert!(
            session.preserve.is_none(),
            "a binary file has no package to preserve"
        );
        assert!(!outcome.warnings.is_empty(), "import warnings surface");
        assert_eq!(
            outcome.warnings, ctx.warnings,
            "the warnings are also on the status list"
        );
    }

    /// A document opened from a binary file saves as its XML format and
    /// the session moves to the file it wrote (task P8-09 goal 2).
    #[test]
    fn a_document_opened_from_a_vsd_saves_as_vsdx() {
        let dir = scratch_dir();
        let source = fixture("minimal-drawing.vsd");
        let vsd = dir.join("plan.vsd");
        std::fs::copy(&source, &vsd).expect("copy");
        let mut ctx = AppCtx::new(crate::Settings::default());
        run_open(&mut ctx, &open_invocation(&vsd)).expect("open");
        // The document edits before it saves (task P8-09 goal 1).
        let session = ctx.active_document_mut().expect("session");
        session.document.add_page("Page-2").expect("page");
        session.dirty = true;
        let outcome = run_save(&mut ctx, &Invocation::new(Args::new(), Source::Ui)).expect("save");
        let vsdx = dir.join("plan.vsdx");
        assert_eq!(
            outcome.message.as_deref(),
            Some(format!("Saved {}", vsdx.display()).as_str())
        );
        assert!(vsdx.is_file(), "the .vsdx was written");
        let bytes = std::fs::read(&vsdx).expect("read back");
        let package = read_package(&bytes, &Limits::DEFAULT).expect("package");
        let document = to_model(&package).expect("model");
        assert_eq!(document.pages.len(), 2, "the round trip keeps the edit");
        let session = ctx.active_document().expect("session");
        assert_eq!(session.path.as_deref(), Some(vsdx.as_path()));
        assert!(!session.dirty);
        assert!(vsd.is_file(), "the read-only source is never overwritten");
        std::fs::remove_dir_all(&dir).expect("clean up");
    }

    /// The binary formats name inputs, never save targets (task P8-09
    /// goal 2): `file.save_as other.vsd` is refused with the reason and
    /// writes nothing.
    #[test]
    fn a_save_into_a_binary_format_is_refused() {
        let dir = scratch_dir();
        let mut ctx = AppCtx::new(crate::Settings::default());
        ctx.open_document(DocumentSession::new(Document::new(), None));
        let target = dir.join("other.vsd");
        let invocation = Invocation::new(
            Args::new().with("path", ArgValue::Path(target.clone())),
            Source::Ui,
        );
        let error = run_save_as(&mut ctx, &invocation).expect_err("read-only");
        let message = error.to_string();
        assert!(message.starts_with("cannot write output:"), "{message}");
        assert!(message.contains("read-only"), "{message}");
        assert!(message.contains("other.vsdx"), "{message} names the target");
        assert!(!target.exists(), "nothing was written");
        std::fs::remove_dir_all(&dir).expect("clean up");
    }

    /// The no-target save of a never-saved document suggests the name
    /// of its kind (task P8-09 goal 2).
    #[test]
    fn the_no_target_save_suggests_a_name_of_the_document_kind() {
        let mut document = Document::new();
        document.kind = DocumentKind::Stencil;
        let mut ctx = AppCtx::new(crate::Settings::default());
        ctx.open_document(DocumentSession::new(document, None));
        let error =
            run_save(&mut ctx, &Invocation::new(Args::new(), Source::Ui)).expect_err("no target");
        let CommandError::NeedsInput(NeededInput::Path {
            purpose: PathPurpose::SaveFile,
            suggested,
        }) = error
        else {
            panic!("expected a save-path request, got {error}");
        };
        assert_eq!(suggested, Some(PathBuf::from("stencil.vssx")));
    }

    /// The Phase 8 acceptance criterion: a corpus `.vsd` opens through
    /// the command with its pages in the model.
    #[test]
    fn a_corpus_vsd_opens_through_the_open_command() {
        let Some(path) = corpus_vsd("dwg.vsd") else {
            return;
        };
        let mut ctx = AppCtx::new(crate::Settings::default());
        let outcome = run_open(&mut ctx, &open_invocation(&path)).expect("open");
        assert!(outcome.message.is_some());
        let session = ctx.active_document().expect("session");
        assert_eq!(session.document.kind, DocumentKind::Drawing);
        assert!(
            !session.document.pages.is_empty(),
            "the corpus drawing has pages"
        );
    }

    /// An unsupported version fails with the clear message naming the
    /// reason; nothing panics (task P8-09 goal 3).
    #[test]
    fn an_unsupported_corpus_version_fails_with_the_clear_message() {
        let Some(path) = corpus_vsd("Visio5PlanWithDimensions.vsd") else {
            return;
        };
        let mut ctx = AppCtx::new(crate::Settings::default());
        let error = run_open(&mut ctx, &open_invocation(&path)).expect_err("version 5");
        let message = error.to_string();
        assert!(message.contains("cannot read input:"), "{message}");
        assert!(message.contains("unsupported file version 5"), "{message}");
        assert!(ctx.active_document().is_none());
    }
}
