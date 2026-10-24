//! The command registry (ADR 0028): one dispatch table for every user
//! action (AGENTS.md section 6).
//!
//! Menus, the command bar, keyboard shortcuts, the CLI, the JSON control
//! channel and MCP dispatch by [`CommandId`] through [`Registry`].
//! A command has an id, a label, an enabled-state predicate over
//! [`AppCtx`] and an execution step with undo support ([`Edit`]).
//! Programmatic calls never open dialogs: a command that needs input
//! returns [`CommandError::NeedsInput`] and the caller supplies the
//! value as an [`Args`] entry.
//!
//! Layer L4 (ADR 0002): the crate depends on the model, the renderer,
//! the readers and the ui-spec data, never on egui. Adding a user
//! action is one handler in [`builtins`] plus (when it has a Microsoft
//! counterpart) extractor mapping rules.

#![forbid(unsafe_code)]

pub mod app;
pub mod builtins;
pub mod clipboard;
pub mod edit;
pub mod error;
pub mod recalc;
pub mod registry;
pub mod undo;

pub use app::{
    ActiveTool, AppCtx, DisplayUnits, DocumentSession, Selection, SessionId, Settings,
    SettingsStore, ViewState,
};
pub use clipboard::{Clipboard, ClipboardContent, ClipboardKind};
pub use edit::CompositeEdit;
pub use error::{
    CommandError, CommandIdError, NeededInput, PathPurpose, RegistryError, SettingsError,
};
pub use recalc::{RecalcReport, recalc_after_edit};
pub use registry::{CommandMeta, CommandStatus, Registry, StubCommand, dispatch};
pub use undo::{Edit, UndoStack};

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The fixed scope tokens of the id grammar (ADR 0006). A new scope is
/// an ADR change; the list is closed on purpose.
const SCOPES: &[&str] = &[
    "app",
    "file",
    "edit",
    "view",
    "insert",
    "format",
    "text",
    "arrange",
    "shape",
    "connector",
    "page",
    "layer",
    "tools",
    "data",
    "review",
    "developer",
    "help",
];

/// Maximum number of ids [`CommandId::intern`] will leak (ADR 0028).
const INTERN_CAP: usize = 4096;

/// A Larrygram command id (ADR 0006).
///
/// The grammar is `scope '.' verb ['.' qualifier]*` with lowercase ASCII
/// letters, digits and underscore in the parts, and one of the fixed
/// scope tokens as the first part. The wrapped string is always `&'static str`: static
/// ids go through [`CommandId::parse`], ids loaded from
/// `ui-spec/commands.yaml` through [`CommandId::intern`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CommandId(pub &'static str);

impl CommandId {
    /// Validates `s` against the ADR 0006 grammar and wraps it.
    pub fn parse(s: &'static str) -> Result<CommandId, CommandIdError> {
        validate_id(s)?;
        Ok(CommandId(s))
    }

    /// Validates `s` and interns it for the lifetime of the process.
    ///
    /// Ids coming from `ui-spec/commands.yaml` are not `'static`; they
    /// are leaked once into a bounded pool (cap: 4096 ids, ADR
    /// 0028). Past the cap this returns [`CommandIdError`].
    pub fn intern(s: &str) -> Result<CommandId, CommandIdError> {
        use std::sync::{Mutex, OnceLock};

        static POOL: OnceLock<Mutex<Vec<&'static str>>> = OnceLock::new();
        let pool = POOL.get_or_init(|| Mutex::new(Vec::new()));
        // The pool holds immutable leaked ids, so a poisoned lock holds
        // no half-built value: recovery is safe (task P5-19 coverage:
        // `PoisonError::into_inner` instead of an unreachable `Err`
        // arm).
        let mut guard = pool
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        intern_into(&mut guard, INTERN_CAP, s)
    }

    /// The id as a string slice.
    pub fn as_str(&self) -> &'static str {
        self.0
    }
}

impl std::fmt::Display for CommandId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

impl From<CommandId> for larrygram_ui_spec::CommandId {
    fn from(id: CommandId) -> Self {
        larrygram_ui_spec::CommandId::new(id.0)
    }
}

impl TryFrom<&larrygram_ui_spec::CommandId> for CommandId {
    type Error = CommandIdError;

    fn try_from(id: &larrygram_ui_spec::CommandId) -> Result<CommandId, CommandIdError> {
        CommandId::intern(id.as_str())
    }
}

/// The pool half of [`CommandId::intern`]: reuse the pool's copy of `s`,
/// else leak `s` into `pool` while it holds fewer than `cap` entries.
///
/// The id is validated before the pool cap is consulted, so a full
/// pool never masks a malformed id (P4-15 review minor 12) and a
/// rejected id never leaks its string (P4-10). Split out from `intern`
/// so tests can exercise the cap without filling the process-global
/// pool (P4-10 finding 5: the global pool is shared by every test in
/// one binary).
fn intern_into(
    pool: &mut Vec<&'static str>,
    cap: usize,
    s: &str,
) -> Result<CommandId, CommandIdError> {
    validate_id(s)?;
    if let Some(found) = pool.iter().find(|known| **known == s) {
        return Ok(CommandId(found));
    }
    if pool.len() >= cap {
        return Err(CommandIdError {
            value: s.to_owned(),
            reason: "id pool is full",
        });
    }
    let leaked: &'static str = Box::leak(s.to_owned().into_boxed_str());
    pool.push(leaked);
    Ok(CommandId(leaked))
}

/// Checks the ADR 0006 grammar (parts, characters, scope).
///
/// Every reject arm is reachable (`split` always yields a first part
/// and an empty part is rejected before its characters are read), so
/// each one has a test.
fn validate_id(s: &str) -> Result<(), CommandIdError> {
    fn reject(s: &str, reason: &'static str) -> Result<(), CommandIdError> {
        Err(CommandIdError {
            value: s.to_owned(),
            reason,
        })
    }

    let mut parts = s.split('.');
    let mut count = 1;
    // `str::split` always yields at least one part.
    let scope = parts.next().unwrap_or("");
    if scope.is_empty() {
        return reject(s, "empty first part");
    }
    if !SCOPES.contains(&scope) {
        return reject(s, "unknown scope (ADR 0006)");
    }
    for part in parts {
        count += 1;
        if part.is_empty() {
            return reject(s, "empty part");
        }
        if !part.starts_with(|c: char| c.is_ascii_lowercase()) {
            return reject(s, "part must start with a lowercase letter");
        }
        if !part
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        {
            return reject(s, "bad character in part");
        }
    }
    if count < 2 {
        return reject(s, "id needs scope and verb");
    }
    Ok(())
}

/// A named argument of a command invocation (ADR 0028).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ArgValue {
    /// Free text.
    Text(String),
    /// A file system path (or a virtual name on wasm, ADR 0036).
    Path(PathBuf),
    /// A whole number (e.g. a page number).
    Integer(i64),
    /// A real number (e.g. a scale).
    Number(f64),
    /// A boolean flag.
    Flag(bool),
    /// A list of strings.
    List(Vec<String>),
    /// Raw bytes (the wasm upload case of `file.open`, ADR 0036).
    Bytes(Vec<u8>),
}

/// Named arguments of one invocation.
///
/// Names are part of the command contract (ADR 0028 lists them per
/// command). The map is `BTreeMap`-backed so iteration is sorted.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Args {
    entries: BTreeMap<String, ArgValue>,
}

impl Args {
    /// An empty argument set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Builder-style insertion.
    pub fn with(mut self, name: impl Into<String>, value: ArgValue) -> Self {
        self.entries.insert(name.into(), value);
        self
    }

    /// Inserts or replaces one argument.
    pub fn set(&mut self, name: impl Into<String>, value: ArgValue) {
        self.entries.insert(name.into(), value);
    }

    /// The argument under `name`, if any.
    pub fn get(&self, name: &str) -> Option<&ArgValue> {
        self.entries.get(name)
    }

    /// The argument under `name` as a path, when it is a [`ArgValue::Path`].
    pub fn path(&self, name: &str) -> Option<&Path> {
        match self.entries.get(name) {
            Some(ArgValue::Path(p)) => Some(p.as_path()),
            _ => None,
        }
    }

    /// The argument under `name` as text, when it is a [`ArgValue::Text`].
    pub fn text(&self, name: &str) -> Option<&str> {
        match self.entries.get(name) {
            Some(ArgValue::Text(t)) => Some(t.as_str()),
            _ => None,
        }
    }

    /// The argument under `name` as an integer, when it is one.
    pub fn integer(&self, name: &str) -> Option<i64> {
        match self.entries.get(name) {
            Some(ArgValue::Integer(i)) => Some(*i),
            _ => None,
        }
    }

    /// The argument under `name` as a number, when it is one.
    pub fn number(&self, name: &str) -> Option<f64> {
        match self.entries.get(name) {
            Some(ArgValue::Number(n)) => Some(*n),
            _ => None,
        }
    }

    /// The argument under `name` as a flag, when it is one.
    pub fn flag(&self, name: &str) -> Option<bool> {
        match self.entries.get(name) {
            Some(ArgValue::Flag(b)) => Some(*b),
            _ => None,
        }
    }

    /// The argument under `name` as raw bytes, when it is one.
    pub fn bytes(&self, name: &str) -> Option<&[u8]> {
        match self.entries.get(name) {
            Some(ArgValue::Bytes(b)) => Some(b.as_slice()),
            _ => None,
        }
    }

    /// Number of arguments.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True when no arguments are set.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Where an invocation came from (ADR 0028). Only interactive sources
/// may be answered with a dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Source {
    /// A widget click in the app.
    Ui,
    /// The command bar.
    CommandBar,
    /// A menu or context menu.
    Menu,
    /// A keyboard shortcut (ADR 0036).
    Shortcut,
    /// The command-line tool (ADR 0026).
    Cli,
    /// The JSON control channel (Phase 8).
    Control,
    /// The MCP server (Phase 8).
    Mcp,
    /// A test.
    Test,
}

impl Source {
    /// True when the dispatcher may open a dialog for
    /// [`CommandError::NeedsInput`] (ADR 0028).
    pub fn is_interactive(self) -> bool {
        matches!(
            self,
            Source::Ui | Source::CommandBar | Source::Menu | Source::Shortcut
        )
    }
}

/// One dispatch request: named arguments plus where it came from.
#[derive(Debug, Clone, PartialEq)]
pub struct Invocation {
    /// The named arguments.
    pub args: Args,
    /// The dispatch source.
    pub source: Source,
}

impl Invocation {
    /// An invocation with the given arguments and source.
    pub fn new(args: Args, source: Source) -> Self {
        Self { args, source }
    }

    /// An invocation with no arguments.
    pub fn empty(source: Source) -> Self {
        Self {
            args: Args::new(),
            source,
        }
    }
}

/// Whether a command can run in the current state (ADR 0028).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Enabled {
    /// The verdict.
    pub value: bool,
    /// Why the command is disabled, shown as a tooltip.
    pub reason: Option<String>,
}

impl Enabled {
    /// The enabled verdict.
    pub fn yes() -> Self {
        Self {
            value: true,
            reason: None,
        }
    }

    /// The disabled verdict with a tooltip reason.
    pub fn no(reason: impl Into<String>) -> Self {
        Self {
            value: false,
            reason: Some(reason.into()),
        }
    }

    /// The verdict as a bool.
    pub fn is_enabled(&self) -> bool {
        self.value
    }
}

/// A request the UI may act on after a command runs (ADR 0028).
///
/// Commands never open dialogs; they return requests. A programmatic
/// caller records or ignores them.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum UiRequest {
    /// Show the About window.
    ShowAbout,
    /// Show the welcome wizard (ADR 0066).
    ShowWelcome,
    /// Show the View/Options dialog (ADR 0032).
    ShowOptions,
    /// Ask the user to confirm closing a document.
    ConfirmClose {
        /// The document's display name.
        name: String,
    },
    /// Quit the application (the `app.quit` / `file.exit` handler,
    /// P4-15). The shell turns this into a viewport close; a
    /// programmatic caller just records it and keeps running.
    Quit,
}

/// A binary payload a command hands back instead of writing a file
/// (ADR 0036: the wasm download case of `file.save`).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Payload {
    /// Raw bytes with the file name the caller should save them under.
    Bytes {
        /// Suggested file name, e.g. `drawing.vsdx`.
        name: String,
        /// The file content.
        data: Vec<u8>,
    },
}

/// The result of a successful execution (ADR 0028).
#[derive(Default)]
#[non_exhaustive]
pub struct Outcome {
    /// A one-line message for the status bar (the `file.inspect`
    /// summary is the one multi-line exception, ADR 0026).
    pub message: Option<String>,
    /// The undoable edit the command produced, if any (ADR 0028).
    pub edit: Option<Box<dyn Edit>>,
    /// A follow-up request for interactive dispatchers.
    pub ui_request: Option<UiRequest>,
    /// A file to hand to the caller instead of writing it to disk
    /// (ADR 0036).
    pub payload: Option<Payload>,
    /// Warning lines the run produced (ADR 0021), one line each,
    /// without the `warning: ` prefix.
    pub warnings: Vec<String>,
}

impl std::fmt::Debug for Outcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Outcome")
            .field("message", &self.message)
            .field("edit", &self.edit.as_ref().map(|edit| edit.label()))
            .field("ui_request", &self.ui_request)
            .field(
                "payload",
                &self.payload.as_ref().map(|payload| match payload {
                    Payload::Bytes { name, data } => format!("{name} ({} bytes)", data.len()),
                }),
            )
            .field("warnings", &self.warnings)
            .finish()
    }
}

impl Outcome {
    /// An outcome with only a message.
    pub fn message(message: impl Into<String>) -> Self {
        Self {
            message: Some(message.into()),
            edit: None,
            ui_request: None,
            payload: None,
            warnings: Vec::new(),
        }
    }
}

/// One registered command (ADR 0028).
///
/// Implementations are stateless or hold immutable configuration; all
/// mutable state lives in [`AppCtx`].
pub trait Command: Send + Sync {
    /// The command's metadata (id, label, status).
    fn meta(&self) -> &CommandMeta;

    /// Whether the command can run in the current state.
    fn enabled(&self, ctx: &AppCtx) -> Enabled;

    /// Runs the command.
    ///
    /// Returns [`CommandError::NeedsInput`] when a required argument is
    /// missing (the caller supplies it and dispatches again) and
    /// [`CommandError::Unsupported`] when the command is a stub.
    fn execute(&self, ctx: &mut AppCtx, invocation: &Invocation) -> Result<Outcome, CommandError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_accepts_the_grammar() {
        assert_eq!(CommandId::parse("file.save").unwrap().as_str(), "file.save");
        assert_eq!(
            CommandId::parse("view.ui_layout.panel_2016")
                .unwrap()
                .as_str(),
            "view.ui_layout.panel_2016"
        );
    }

    #[test]
    fn parse_rejects_bad_ids() {
        for bad in [
            "",
            "file",
            "file.",
            ".save",
            "file..save",
            "File.save",
            "file.Save",
            "file.save!",
            "mystery.save",
            "file.2save",
        ] {
            let err = CommandId::parse(bad).expect_err(bad);
            assert!(!err.value.is_empty() || bad.is_empty());
        }
    }

    /// Every `validate_id` reject arm fires with its own reason (P4-12
    /// coverage minor 2: the two unreachable arms are gone).
    #[test]
    fn every_reject_arm_names_its_reason() {
        let cases: &[(&str, &str)] = &[
            ("", "empty first part"),
            (".save", "empty first part"),
            ("mystery.save", "unknown scope (ADR 0006)"),
            ("file.", "empty part"),
            ("file..save", "empty part"),
            ("file.Save", "part must start with a lowercase letter"),
            ("file.2save", "part must start with a lowercase letter"),
            ("file.save!", "bad character in part"),
            ("file", "id needs scope and verb"),
        ];
        for (bad, reason) in cases {
            let error = CommandId::parse(bad).expect_err(bad);
            assert_eq!(error.reason, *reason, "{bad:?}");
            assert_eq!(error.value, *bad);
        }
    }

    /// The pool seam (P4-15 review minors 12 and 2): the id is
    /// validated before the pool cap is consulted, so a full pool
    /// never masks a malformed id, and a full pool rejects a
    /// well-formed one without leaking it.
    #[test]
    fn intern_validates_before_the_pool_cap() {
        let mut pool: Vec<&'static str> = Vec::new();
        let error = intern_into(&mut pool, 0, "file..save").expect_err("malformed id");
        assert_eq!(error.reason, "empty part", "validation runs first");
        assert!(pool.is_empty(), "a rejected id leaks nothing");

        let error = intern_into(&mut pool, 0, "file.save").expect_err("full pool");
        assert_eq!(error.reason, "id pool is full");
        assert!(pool.is_empty());

        // Room for one id: the second distinct id hits the cap, a
        // repeat reuses the pooled copy.
        let first = intern_into(&mut pool, 1, "file.save").expect("intern");
        let again = intern_into(&mut pool, 1, "file.save").expect("reuse");
        assert_eq!(first, again);
        assert_eq!(pool.len(), 1);
        let error = intern_into(&mut pool, 1, "file.open").expect_err("cap");
        assert_eq!(error.reason, "id pool is full");
        assert_eq!(error.value, "file.open");
    }

    #[test]
    fn intern_reuses_and_validates() {
        let a = CommandId::intern("page.next").expect("intern");
        let b = CommandId::intern("page.next").expect("intern");
        assert_eq!(a, b);
        assert!(CommandId::intern("page..next").is_err());
    }

    #[test]
    fn command_id_displays_and_converts() {
        let id = CommandId::parse("file.open").unwrap();
        assert_eq!(id.to_string(), "file.open");
        let ui_id: larrygram_ui_spec::CommandId = id.into();
        assert_eq!(ui_id.as_str(), "file.open");
        let back = CommandId::try_from(&ui_id).unwrap();
        assert_eq!(back, id);
    }

    #[test]
    fn args_round_trip_every_variant() {
        let args = Args::new()
            .with("path", ArgValue::Path(PathBuf::from("a.vsdx")))
            .with("text", ArgValue::Text("hi".into()))
            .with("int", ArgValue::Integer(-2))
            .with("num", ArgValue::Number(1.5))
            .with("flag", ArgValue::Flag(true))
            .with("list", ArgValue::List(vec!["x".into()]))
            .with("data", ArgValue::Bytes(vec![1, 2, 3]));
        assert_eq!(args.len(), 7);
        assert!(!args.is_empty());
        assert_eq!(args.path("path"), Some(Path::new("a.vsdx")));
        assert_eq!(args.text("text"), Some("hi"));
        assert_eq!(args.integer("int"), Some(-2));
        assert_eq!(args.number("num"), Some(1.5));
        assert_eq!(args.flag("flag"), Some(true));
        assert_eq!(args.bytes("data"), Some(&[1u8, 2, 3][..]));
        assert!(matches!(args.get("list"), Some(ArgValue::List(_))));
        assert_eq!(args.get("missing"), None);
        assert_eq!(args.path("text"), None);
        assert_eq!(args.text("int"), None);
        assert_eq!(args.integer("num"), None);
        assert_eq!(args.number("flag"), None);
        assert_eq!(args.flag("list"), None);
        assert_eq!(args.bytes("list"), None);
    }

    #[test]
    fn set_replaces_and_empty_is_empty() {
        let mut args = Args::new();
        assert!(args.is_empty());
        args.set("a", ArgValue::Flag(false));
        args.set("a", ArgValue::Flag(true));
        assert_eq!(args.flag("a"), Some(true));
    }

    #[test]
    fn source_knows_its_interactive_form() {
        assert!(Source::Ui.is_interactive());
        assert!(Source::Shortcut.is_interactive());
        assert!(!Source::Cli.is_interactive());
        assert!(!Source::Mcp.is_interactive());
    }

    #[test]
    fn invocation_builders() {
        let inv = Invocation::empty(Source::Test);
        assert!(inv.args.is_empty());
        let inv = Invocation::new(Args::new(), Source::Cli);
        assert_eq!(inv.source, Source::Cli);
    }

    #[test]
    fn enabled_verdicts() {
        assert!(Enabled::yes().is_enabled());
        assert!(Enabled::yes().reason.is_none());
        let no = Enabled::no("no selection");
        assert!(!no.is_enabled());
        assert_eq!(no.reason.as_deref(), Some("no selection"));
    }

    #[test]
    fn outcome_message_and_default() {
        let outcome = Outcome::message("saved");
        assert_eq!(outcome.message.as_deref(), Some("saved"));
        assert!(outcome.edit.is_none());
        assert!(outcome.ui_request.is_none());
        assert!(outcome.payload.is_none());
        assert!(outcome.warnings.is_empty());
        let empty = Outcome::default();
        assert!(empty.message.is_none());
    }

    #[test]
    fn outcome_carries_a_payload_and_warnings() {
        let mut outcome = Outcome::message("saved");
        outcome.payload = Some(Payload::Bytes {
            name: "drawing.vsdx".into(),
            data: vec![0, 1],
        });
        outcome
            .warnings
            .push("shape 1 PinX: wrong kind, using 0".into());
        let text = format!("{outcome:?}");
        assert!(text.contains("drawing.vsdx (2 bytes)"), "{text}");
        assert!(text.contains("PinX"), "{text}");
        assert!(matches!(outcome.payload, Some(Payload::Bytes { .. })));
        if let Some(Payload::Bytes { name, data }) = outcome.payload {
            assert_eq!(name, "drawing.vsdx");
            assert_eq!(data, vec![0, 1]);
        }
    }
}
