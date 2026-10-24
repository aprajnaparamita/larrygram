//! The built-in registry: one function that registers every implemented
//! command (ADR 0028).
//!
//! The desktop app, the web app, the CLI and the parity generator
//! (ADR 0037) all build their registry here, so "which commands are
//! implemented" has exactly one answer. Implementers add their handlers
//! to the `BUILTINS` table when their command lands.
//!
//! [`registry`] starts from the stubs of `ui-spec/commands.yaml`
//! ([`Registry::from_mappings`], ADR 0006) and registers the real
//! builtins over them, so [`CommandStatus`] is accurate for every id
//! the data file knows.

mod arrange;
mod clipboard;
mod connector;
mod data;
mod edit;
mod file;
mod insert;
mod page;
mod shape;
mod text;
mod tools;
mod view;

pub use view::{ZOOM_MAX, ZOOM_MIN};

use std::sync::OnceLock;

use larrygram_ui_spec::Commands;

use crate::app::AppCtx;
use crate::error::{CommandError, RegistryError};
use crate::registry::{CommandMeta, CommandStatus, Registry};
use crate::{Command, CommandId, Enabled, Invocation, Outcome};

pub use file::{
    PageSummary, Summary, kind_text, shape_count, sheet_text, value_text, warning_line,
};

/// Whether a command can run: its enabled-state predicate (ADR 0028).
pub(crate) type EnabledFn = fn(&AppCtx) -> Enabled;

/// What a command does when dispatched (ADR 0028).
pub(crate) type RunFn = fn(&mut AppCtx, &Invocation) -> Result<Outcome, CommandError>;

/// One module's table of implemented commands (ADR 0040): each Phase 5
/// command module owns one `TABLE`, so parallel tasks never edit the same
/// list (task P5-01 wires the seams; see `docs/tasks/P5-01.md`).
pub(crate) type CommandTable = &'static [(&'static str, &'static str, EnabledFn, RunFn)];

/// The per-module tables of the Phase 5 command modules. Each is empty
/// until its owner task fills it; `registry` chains them onto
/// [`BUILTINS`] (ADR 0040).
pub(crate) const STUB_TABLES: &[CommandTable] = &[
    arrange::TABLE,
    clipboard::TABLE,
    connector::TABLE,
    data::TABLE,
    edit::TABLE,
    insert::TABLE,
    page::TABLE,
    shape::TABLE,
    text::TABLE,
    tools::TABLE,
];

/// One registered command: metadata plus its two function pointers.
///
/// The builtins are stateless; all mutable state lives in [`AppCtx`]
/// (ADR 0029).
struct Builtin {
    meta: CommandMeta,
    enabled_fn: EnabledFn,
    run: RunFn,
}

impl Command for Builtin {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    fn enabled(&self, ctx: &AppCtx) -> Enabled {
        (self.enabled_fn)(ctx)
    }

    fn execute(&self, ctx: &mut AppCtx, invocation: &Invocation) -> Result<Outcome, CommandError> {
        (self.run)(ctx, invocation)
    }
}

/// Every implemented command: `(id, fallback label, enabled, run)`.
///
/// The label is only used when `commands.yaml` has no row for the id;
/// the row's label wins otherwise. Argument names are the command
/// contract (ADR 0028): `file.open` takes `path` (or `name` + `data`),
/// `file.save_as` takes `path`, `file.render` takes `path`, `out`,
/// `page`, `format`, `scale`, `background` and `strict`, `file.export`
/// takes `path`, `out`, `format`, `scale` and `strict`, `file.inspect`
/// takes `path`, `page` and `json`, `view.zoom.fit` takes `width` and
/// `height` (points), `view.units` takes `units` (a `DisplayUnits`
/// name, P4-15), and `file.close` takes an optional `confirm` flag
/// (the answer to its `ConfirmClose` request).
const BUILTINS: &[(&str, &str, EnabledFn, RunFn)] = &[
    (
        "app.office.center.options",
        "Options",
        always,
        view::run_options,
    ),
    ("app.quit", "Exit", always, file::run_quit),
    ("file.new", "New", always, file::run_new),
    ("file.open", "Open", always, file::run_open),
    ("file.save", "Save", has_document, file::run_save),
    ("file.save_as", "Save As", has_document, file::run_save_as),
    ("file.close", "Close", has_document, file::run_close),
    ("file.inspect", "Inspect", always, file::run_inspect),
    ("file.render", "Render", always, file::run_render),
    ("file.export", "Export", always, file::run_export),
    ("file.exit", "Exit", always, file::run_quit),
    ("edit.undo", "Undo", can_undo, view::run_undo),
    ("edit.redo", "Redo", can_redo, view::run_redo),
    ("view.zoom.in", "Zoom In", has_document, view::run_zoom_in),
    (
        "view.zoom.out",
        "Zoom Out",
        has_document,
        view::run_zoom_out,
    ),
    (
        "view.zoom.fit",
        "Fit to Window",
        has_pages,
        view::run_zoom_fit,
    ),
    (
        "view.toggle_grid",
        "Toggle Grid",
        always,
        view::run_toggle_grid,
    ),
    ("view.rulers", "Rulers", always, view::run_toggle_rulers),
    ("view.units", "Units", always, view::run_units),
    (
        "view.ui_layout.panel_2016",
        "Panel (2016 data)",
        always,
        view::run_ui_layout_2016,
    ),
    (
        "view.ui_layout.panel_365",
        "Panel (365 data)",
        always,
        view::run_ui_layout_365,
    ),
    (
        "view.ui_layout.panel_classic",
        "Panel Classic Layout",
        always,
        view::run_ui_layout_panel_classic,
    ),
    (
        "view.ui_layout.panel_modern",
        "Panel Modern Layout",
        always,
        view::run_ui_layout_panel_modern,
    ),
    ("page.next", "Next Page", has_pages, view::run_page_next),
    (
        "page.previous",
        "Previous Page",
        has_pages,
        view::run_page_previous,
    ),
    ("help.about", "About Larrygram", always, view::run_about),
    ("help.welcome", "Welcome", always, view::run_welcome),
];

/// Builds the registry with every implemented command registered over
/// its `commands.yaml` stub (ADR 0028).
///
/// Returns [`RegistryError::Duplicate`] when two handlers claim one id;
/// that is a build bug, not input-dependent behaviour.
pub fn registry() -> Result<Registry, RegistryError> {
    let mappings = embedded_commands()?;
    let mut registry = Registry::from_mappings(mappings)?;
    for &(id, label, enabled_fn, run) in
        BUILTINS.iter().chain(STUB_TABLES.iter().copied().flatten())
    {
        let meta = meta_for(mappings, id, label)?;
        registry.register(Box::new(Builtin {
            meta,
            enabled_fn,
            run,
        }))?;
    }
    Ok(registry)
}

/// The committed `ui-spec/commands.yaml`, parsed once per process.
///
/// One parse path (P4-15 review minor 8): the rows come from
/// [`Loader::from_embedded`](larrygram_ui_spec::Loader::from_embedded),
/// the same staged load (parse, schema validation, decode) the command bar
/// and the loader tests run, instead of a second `include_str!` +
/// YAML-to-JSON round trip that never saw the schema. The commands
/// document is shared by both layouts; the 2016 set carries it. A load
/// failure is a [`RegistryError::BadMapping`]: the committed data is
/// generated and validated (ADR 0006), so this cannot fail on input.
fn embedded_commands() -> Result<&'static Commands, RegistryError> {
    static COMMANDS: OnceLock<Result<Commands, String>> = OnceLock::new();
    let cached = COMMANDS.get_or_init(|| {
        let sets = larrygram_ui_spec::Loader::from_embedded().map_err(|error| error.to_string())?;
        Ok(sets.panel_2016.commands)
    });
    match cached {
        Ok(commands) => Ok(commands),
        Err(message) => Err(RegistryError::BadMapping {
            message: message.clone(),
        }),
    }
}

/// The metadata of one builtin: the `commands.yaml` row (label, Fluent
/// ids, VisUICmds constant, confidence) with the status flipped to
/// [`CommandStatus::Implemented`], or a minimal fallback when the
/// table has no row (a build gap: every registry id belongs in
/// `commands.yaml`, ADR 0028).
fn meta_for(
    mappings: &Commands,
    id: &'static str,
    label: &str,
) -> Result<CommandMeta, RegistryError> {
    if let Some(mapping) = mappings.entries.iter().find(|m| m.id.as_str() == id) {
        let mut meta = CommandMeta::from_mapping(mapping)?;
        meta.status = CommandStatus::Implemented;
        return Ok(meta);
    }
    let command_id = CommandId::parse(id).map_err(|error| RegistryError::BadId {
        id: id.to_owned(),
        reason: error.reason,
    })?;
    Ok(CommandMeta::new(
        command_id,
        label,
        CommandStatus::Implemented,
    ))
}

/// Always enabled.
pub(crate) fn always(_ctx: &AppCtx) -> Enabled {
    Enabled::yes()
}

/// Enabled when a document is open (ADR 0029 state).
pub(crate) fn has_document(ctx: &AppCtx) -> Enabled {
    if ctx.active_document().is_some() {
        Enabled::yes()
    } else {
        Enabled::no("no open document")
    }
}

/// Enabled when the open document has pages.
pub(crate) fn has_pages(ctx: &AppCtx) -> Enabled {
    match ctx.active_document() {
        None => Enabled::no("no open document"),
        Some(session) if session.document.pages.is_empty() => {
            Enabled::no("the document has no pages")
        }
        Some(_) => Enabled::yes(),
    }
}

/// Enabled when the active document has an undoable edit (ADR 0028).
pub(crate) fn can_undo(ctx: &AppCtx) -> Enabled {
    if ctx.active_document().is_some_and(|s| s.undo.can_undo()) {
        Enabled::yes()
    } else {
        Enabled::no("nothing to undo")
    }
}

/// Enabled when the active document has a redoable edit (ADR 0028).
pub(crate) fn can_redo(ctx: &AppCtx) -> Enabled {
    if ctx.active_document().is_some_and(|s| s.undo.can_redo()) {
        Enabled::yes()
    } else {
        Enabled::no("nothing to redo")
    }
}

/// The "no open document" failure of a command whose enabled-state
/// predicate says so (kept for direct calls that skip [`Registry::execute`]).
pub(crate) fn no_document() -> CommandError {
    CommandError::failed("no open document")
}

/// An outcome with a status-bar message and the run's warnings.
pub(crate) fn outcome(message: impl Into<String>, warnings: Vec<String>) -> Outcome {
    Outcome {
        message: Some(message.into()),
        warnings,
        ..Outcome::default()
    }
}

/// An outcome for a command that produced one undoable edit (ADR 0040
/// rule 1). The dispatcher pushes the edit with
/// [`AppCtx::push_edit`](crate::AppCtx::push_edit) after `execute`
/// returns `Ok`.
pub(crate) fn edit_outcome(
    message: impl Into<String>,
    warnings: Vec<String>,
    edit: Box<dyn crate::Edit>,
) -> Outcome {
    Outcome {
        message: Some(message.into()),
        edit: Some(edit),
        warnings,
        ..Outcome::default()
    }
}

/// Enabled when the selection is not empty (ADR 0041).
pub(crate) fn has_selection(ctx: &AppCtx) -> Enabled {
    if ctx.selection.is_empty() {
        Enabled::no("no selection")
    } else {
        Enabled::yes()
    }
}

/// Wraps a model failure as [`CommandError::Failed`] (ADR 0028: the
/// domain crates keep their typed errors, `commands` maps them).
pub(crate) fn model_error(error: larrygram_model::Error) -> CommandError {
    CommandError::failed(error.to_string())
}

/// One required number argument (task P5-04 argument contract).
///
/// A missing argument is [`crate::NeededInput::Number`] (the dispatcher
/// supplies it and dispatches again, ADR 0028); a non-finite value or a
/// value of the wrong kind is [`CommandError::InvalidArgs`]. A whole
/// number is accepted and converted.
pub(crate) fn need_finite(
    args: &crate::Args,
    name: &str,
    prompt: &str,
) -> Result<f64, CommandError> {
    match args.get(name) {
        None => Err(CommandError::NeedsInput(crate::NeededInput::Number {
            prompt: prompt.to_owned(),
            default: None,
        })),
        Some(crate::ArgValue::Number(value)) if value.is_finite() => Ok(*value),
        Some(crate::ArgValue::Number(_)) => {
            Err(CommandError::invalid_args(format!("{name} must be finite")))
        }
        Some(crate::ArgValue::Integer(value)) => Ok(*value as f64),
        Some(_) => Err(CommandError::invalid_args(format!(
            "{name} must be a number"
        ))),
    }
}

/// One required non-negative size argument (task P5-04): [`need_finite`]
/// plus "not negative" (a size of zero collapses the shape, which the
/// model allows).
pub(crate) fn need_size(args: &crate::Args, name: &str, prompt: &str) -> Result<f64, CommandError> {
    let value = need_finite(args, name, prompt)?;
    if value < 0.0 {
        return Err(CommandError::invalid_args(format!(
            "{name} must not be negative"
        )));
    }
    Ok(value)
}

/// Records warning lines on the state (the status-bar ring, ADR 0021).
pub(crate) fn record_warnings(ctx: &mut AppCtx, warnings: &[String]) {
    for line in warnings {
        ctx.record_warning(line.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_builtin_registry_covers_every_builtins_row() {
        let registry = registry().expect("registry");
        assert!(registry.len() >= BUILTINS.len());
        for &(id, _, _, _) in BUILTINS.iter().chain(STUB_TABLES.iter().copied().flatten()) {
            let id = CommandId::parse(id).expect("builtin id");
            assert_eq!(
                registry.status(&id),
                CommandStatus::Implemented,
                "{id} must be implemented"
            );
        }
    }

    #[test]
    fn the_builtin_registry_has_the_stubs_of_the_data_file() {
        let registry = registry().expect("registry");
        let mappings = embedded_commands().expect("mappings");
        assert_eq!(registry.len(), mappings.entries.len());
        // A row nothing implements is a stub with the tooltip.
        let id = CommandId::parse("file.print").expect("id");
        assert_eq!(registry.status(&id), CommandStatus::Stub);
    }

    #[test]
    fn the_embedded_table_parses_and_every_row_is_valid() {
        let mappings = embedded_commands().expect("mappings");
        assert!(!mappings.entries.is_empty());
        for entry in &mappings.entries {
            CommandId::try_from(&entry.id).expect("row id");
        }
    }

    /// P4-12 coverage minor 3: `meta_for` maps an unparseable builtin
    /// id to `RegistryError::BadId` (a build bug in the `BUILTINS`
    /// table, never input).
    #[test]
    fn meta_for_rejects_a_malformed_builtin_id() {
        let mappings = Commands::default();
        let error = meta_for(&mappings, "file.", "Broken").expect_err("bad id");
        assert!(matches!(error, RegistryError::BadId { .. }), "{error}");
    }

    #[test]
    fn meta_for_falls_back_when_the_table_has_no_row() {
        let mappings = Commands::default();
        let meta = meta_for(&mappings, "file.save", "Save").expect("meta");
        assert_eq!(meta.id.as_str(), "file.save");
        assert_eq!(meta.label, "Save");
        assert_eq!(meta.status, CommandStatus::Implemented);

        let mappings = embedded_commands().expect("mappings");
        let meta = meta_for(mappings, "file.save", "Fallback").expect("meta");
        assert_eq!(meta.label, "Save", "the table row wins");
        assert_eq!(meta.status, CommandStatus::Implemented);
        assert!(meta.visuicmd.is_some());
    }

    /// P4-12 coverage minors 3 and 4: the `no_document` failure and
    /// `with_undo`'s no-document branches, reached only by direct
    /// calls (the enabled-state predicates block these paths in
    /// `Registry::execute`).
    #[test]
    fn direct_calls_without_a_document_report_no_open_document() {
        use crate::{ArgValue, Args, Source};

        let inv = Invocation::empty(Source::Test);
        let mut ctx = AppCtx::new(crate::Settings::default());
        let mut failures: Vec<(&str, Result<Outcome, CommandError>)> =
            vec![("edit.undo", view::run_undo(&mut ctx, &inv))];
        failures.push(("edit.redo", view::run_redo(&mut ctx, &inv)));
        failures.push(("view.zoom.in", view::run_zoom_in(&mut ctx, &inv)));
        failures.push(("page.next", view::run_page_next(&mut ctx, &inv)));
        failures.push(("file.close", file::run_close(&mut ctx, &inv)));
        let mut args = Args::new();
        args.set("path", ArgValue::Path(std::path::PathBuf::from("x.vsdx")));
        failures.push((
            "file.save",
            file::run_save(&mut ctx, &Invocation::new(args, Source::Test)),
        ));
        for (id, result) in failures {
            let error = result.expect_err(id);
            assert_eq!(error.to_string(), "no open document", "{id}");
        }

        // A stale `active` index is the same failure (P4-12 minor 4).
        ctx.active = Some(3);
        let error = view::run_undo(&mut ctx, &inv).expect_err("stale index");
        assert_eq!(error.to_string(), "no open document");

        // A document with no pages says so (P4-12 coverage minor 4).
        ctx.open_document(crate::DocumentSession::new(
            larrygram_model::Document::new(),
            None,
        ));
        let error = view::run_page_next(&mut ctx, &inv).expect_err("no pages");
        assert_eq!(error.to_string(), "the document has no pages");
    }

    #[test]
    fn enabled_predicates_name_their_reasons() {
        let ctx = AppCtx::new(crate::Settings::default());
        assert!(always(&ctx).is_enabled());
        let disabled = has_document(&ctx);
        assert_eq!(disabled.reason.as_deref(), Some("no open document"));
        assert_eq!(has_pages(&ctx).reason.as_deref(), Some("no open document"));
        assert_eq!(can_undo(&ctx).reason.as_deref(), Some("nothing to undo"));
        assert_eq!(can_redo(&ctx).reason.as_deref(), Some("nothing to redo"));

        let mut ctx = AppCtx::new(crate::Settings::default());
        ctx.open_document(crate::DocumentSession::new(
            larrygram_model::Document::new(),
            None,
        ));
        assert!(has_document(&ctx).is_enabled());
        assert_eq!(
            has_pages(&ctx).reason.as_deref(),
            Some("the document has no pages")
        );
    }
}
