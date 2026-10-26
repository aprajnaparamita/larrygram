//! The registry itself (ADR 0028): one entry per command id.
//!
//! The registry is built from `ui-spec/commands.yaml` (ADR 0006) and
//! then has its implemented commands registered over the stubs. The UI
//! reads [`CommandStatus`] and [`Registry::enabled`] to decide whether a
//! control renders enabled, greyed with a reason, or disabled with the
//! "Not yet implemented" tooltip (ADR 0031).

use std::collections::BTreeMap;

use larrygram_ui_spec::{CommandMapping, FluentId, MappingConfidence};

use crate::app::AppCtx;
use crate::error::{CommandError, RegistryError};
use crate::{Command, CommandId, Enabled, Invocation, Outcome};

/// Whether a command id has a real handler (ADR 0028).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CommandStatus {
    /// A real handler is registered.
    Implemented,
    /// The id exists (usually from `commands.yaml`) but has no handler
    /// yet: the UI renders it disabled with "Not yet implemented".
    Stub,
}

/// The metadata of one registered command (ADR 0028).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandMeta {
    /// The command id (ADR 0006).
    pub id: CommandId,
    /// Short UI label (from `commands.yaml`).
    pub label: String,
    /// Whether a real handler backs this id.
    pub status: CommandStatus,
    /// Fluent control names that surface this command (ADR 0006).
    pub fluent_ids: Vec<FluentId>,
    /// The VisUICmds constant, when one maps to this command.
    pub visuicmd: Option<String>,
    /// How confident the mapping is (ADR 0006).
    pub mapping_confidence: MappingConfidence,
}

impl CommandMeta {
    /// Metadata for a registered command id and label.
    pub fn new(id: CommandId, label: impl Into<String>, status: CommandStatus) -> Self {
        Self {
            id,
            label: label.into(),
            status,
            fluent_ids: Vec::new(),
            visuicmd: None,
            mapping_confidence: MappingConfidence::Unknown,
        }
    }

    /// Builds metadata from one `commands.yaml` entry (ADR 0006).
    pub fn from_mapping(mapping: &CommandMapping) -> Result<CommandMeta, RegistryError> {
        let id = CommandId::try_from(&mapping.id).map_err(|error| RegistryError::BadId {
            id: mapping.id.as_str().to_owned(),
            reason: error.reason,
        })?;
        Ok(CommandMeta {
            id,
            label: mapping.label.clone(),
            status: CommandStatus::Stub,
            fluent_ids: mapping.fluent_ids.clone(),
            visuicmd: mapping.visuicmd.clone(),
            mapping_confidence: mapping.mapping_confidence,
        })
    }
}

/// A command that reports "not implemented" (ADR 0006, ADR 0031).
///
/// [`Registry::from_mappings`] creates one per `commands.yaml` entry;
/// registering a real command over it flips the id to
/// [`CommandStatus::Implemented`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StubCommand {
    meta: CommandMeta,
}

impl StubCommand {
    /// A stub for the given metadata.
    pub fn new(meta: CommandMeta) -> Self {
        Self { meta }
    }
}

impl Command for StubCommand {
    fn meta(&self) -> &CommandMeta {
        &self.meta
    }

    fn enabled(&self, _ctx: &AppCtx) -> Enabled {
        Enabled::no("not yet implemented")
    }

    fn execute(
        &self,
        _ctx: &mut AppCtx,
        _invocation: &Invocation,
    ) -> Result<Outcome, CommandError> {
        Err(CommandError::Unsupported {
            id: self.meta.id.to_string(),
        })
    }
}

/// The command registry (ADR 0028): `CommandId` to [`Command`].
///
/// One dispatch table for the menus, the command bar, shortcuts, the CLI, the
/// JSON control channel and MCP (AGENTS.md section 6). Backed by a
/// `BTreeMap`, as ADR 0028 specifies: lookups are O(log n) and the id
/// iteration is sorted.
#[derive(Default)]
pub struct Registry {
    commands: BTreeMap<CommandId, Box<dyn Command>>,
}

impl Registry {
    /// An empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of registered commands.
    pub fn len(&self) -> usize {
        self.commands.len()
    }

    /// True when nothing is registered.
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    /// The command under `id`, if one is registered.
    pub fn get(&self, id: &CommandId) -> Option<&dyn Command> {
        self.commands.get(id).map(|command| command.as_ref())
    }

    /// The metadata under `id`, if one is registered.
    pub fn meta(&self, id: &CommandId) -> Option<&CommandMeta> {
        self.get(id).map(Command::meta)
    }

    /// The status under `id`; unknown ids are [`CommandStatus::Stub`].
    pub fn status(&self, id: &CommandId) -> CommandStatus {
        self.meta(id)
            .map(|meta| meta.status)
            .unwrap_or(CommandStatus::Stub)
    }

    /// Iterates the registered ids in id order.
    pub fn ids(&self) -> impl Iterator<Item = CommandId> + '_ {
        self.commands.keys().copied()
    }

    /// Registers a command.
    ///
    /// A real ([`CommandStatus::Implemented`]) command registered over
    /// a [`StubCommand`] replaces it: that is how
    /// [`builtins::registry()`](crate::builtins::registry) puts the
    /// implemented commands over their `commands.yaml` stubs. Two
    /// commands with one id (and a stub over anything) are a build bug
    /// and return [`RegistryError::Duplicate`].
    pub fn register(&mut self, command: Box<dyn Command>) -> Result<(), RegistryError> {
        use std::collections::btree_map::Entry;

        let id = command.meta().id;
        let new_real = command.meta().status == CommandStatus::Implemented;
        match self.commands.entry(id) {
            Entry::Occupied(mut entry) => {
                let existing_real = entry.get().meta().status == CommandStatus::Implemented;
                if !new_real || existing_real {
                    return Err(RegistryError::Duplicate { id: id.to_string() });
                }
                entry.insert(command);
                Ok(())
            }
            Entry::Vacant(entry) => {
                entry.insert(command);
                Ok(())
            }
        }
    }

    /// Registers or replaces a command.
    ///
    /// Unlike [`Registry::register`] this is not a build-time
    /// consistency check: it exists for the test rigs that latch one
    /// command id onto an always-enabled stand-in (task P7-21: an id
    /// that gained a real handler replaces the stand-in's predecessor
    /// instead of tripping [`RegistryError::Duplicate`]).
    pub fn replace(&mut self, command: Box<dyn Command>) {
        use std::collections::btree_map::Entry;

        let id = command.meta().id;
        match self.commands.entry(id) {
            Entry::Occupied(mut entry) => {
                entry.insert(command);
            }
            Entry::Vacant(entry) => {
                entry.insert(command);
            }
        }
    }

    /// Builds a registry of stubs from `ui-spec/commands.yaml` (ADR 0006).
    ///
    /// One [`StubCommand`] per entry, in file order. A duplicated id in
    /// the table is [`RegistryError::Duplicate`]; a malformed id is
    /// [`RegistryError::BadId`].
    pub fn from_mappings(
        mappings: &larrygram_ui_spec::Commands,
    ) -> Result<Registry, RegistryError> {
        let mut registry = Registry::new();
        for mapping in &mappings.entries {
            let meta = CommandMeta::from_mapping(mapping)?;
            registry.register(Box::new(StubCommand::new(meta)))?;
        }
        Ok(registry)
    }

    /// Whether the command can run in the current state (ADR 0028).
    ///
    /// Unknown ids report disabled with the "not yet implemented"
    /// reason, the same tooltip the UI shows for stubs (ADR 0031).
    pub fn enabled(&self, id: &CommandId, ctx: &AppCtx) -> Enabled {
        match self.get(id) {
            Some(command) => command.enabled(ctx),
            None => Enabled::no("not yet implemented"),
        }
    }

    /// Dispatches one invocation (ADR 0028).
    ///
    /// Unknown ids return [`CommandError::UnknownCommand`]; a stub
    /// reports [`CommandError::Unsupported`] (ADR 0037); a command
    /// whose enabled-state says no returns [`CommandError::NotEnabled`]
    /// (never a dialog, never a side effect).
    pub fn execute(
        &self,
        id: &CommandId,
        ctx: &mut AppCtx,
        invocation: Invocation,
    ) -> Result<Outcome, CommandError> {
        let Some(command) = self.get(id) else {
            return Err(CommandError::UnknownCommand { id: id.to_string() });
        };
        if command.meta().status == CommandStatus::Stub {
            return Err(CommandError::Unsupported { id: id.to_string() });
        }
        let enabled = command.enabled(ctx);
        if !enabled.value {
            return Err(CommandError::NotEnabled {
                id: id.to_string(),
                reason: enabled.reason,
            });
        }
        command.execute(ctx, &invocation)
    }
}

/// Dispatches one already-built invocation and records its undo step:
/// the seam every programmatic caller reuses (the shell, the CLI, the
/// tests, the Phase 8 control channel). This is the **only** dispatcher
/// of [`Outcome::edit`]; a caller that runs `Registry::execute` directly
/// must not (it would drop the undo step, task P5-19 review F3).
///
/// Runs the command through [`Registry::execute`], then pushes the
/// command's undoable edit (when it produced one) onto the undo stack of
/// the session the command ran against: the active session **before**
/// the run, found by its [`DocumentSession::id`](crate::DocumentSession)
/// (the `with_undo` way, P4-15 open issue 4), so a command that opens a
/// document does not leave its edit on the new document's stack. With no
/// session before the run (the command opened the first document) the
/// edit belongs to the session it opened. With no session at all the
/// edit is dropped with one warning line (ADR 0040 rule 2:
/// [`AppCtx::push_edit`](crate::AppCtx::push_edit) is the only push
/// path).
pub fn dispatch(
    registry: &Registry,
    ctx: &mut AppCtx,
    id: &CommandId,
    invocation: Invocation,
) -> Result<Outcome, CommandError> {
    let ran_against = ctx.active_document().map(|session| session.id);
    let mut outcome = registry.execute(id, ctx, invocation)?;
    if let Some(edit) = outcome.edit.take() {
        let target = ran_against.or_else(|| ctx.active_document().map(|session| session.id));
        match target {
            Some(session) => ctx.push_edit(session, edit)?,
            None => ctx.record_warning("an undoable edit with no open document was dropped"),
        }
    }
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Settings;
    use crate::{ArgValue, Args, Source};

    fn meta(id: &'static str) -> CommandMeta {
        CommandMeta::new(
            CommandId::parse(id).expect("id"),
            "Test",
            CommandStatus::Stub,
        )
    }

    #[test]
    fn meta_builders() {
        let meta = meta("file.save");
        assert_eq!(meta.id.as_str(), "file.save");
        assert_eq!(meta.label, "Test");
        assert_eq!(meta.status, CommandStatus::Stub);
        assert!(meta.fluent_ids.is_empty());
        assert!(meta.visuicmd.is_none());
        assert_eq!(meta.mapping_confidence, MappingConfidence::Unknown);

        let mapping = CommandMapping {
            id: larrygram_ui_spec::CommandId::new("file.save"),
            label: "Save".to_owned(),
            visuicmd: Some("visCmdFileSave".to_owned()),
            fluent_ids: vec![FluentId::new("FileSave")],
            mapping_confidence: MappingConfidence::Exact,
        };
        let meta = CommandMeta::from_mapping(&mapping).expect("mapping");
        assert_eq!(meta.id.as_str(), "file.save");
        assert_eq!(meta.label, "Save");
        assert_eq!(meta.status, CommandStatus::Stub);
        assert_eq!(meta.visuicmd.as_deref(), Some("visCmdFileSave"));
        assert_eq!(meta.mapping_confidence, MappingConfidence::Exact);
    }

    #[test]
    fn a_stub_command_is_disabled_and_unsupported() {
        let stub = StubCommand::new(meta("file.save"));
        let mut ctx = AppCtx::new(Settings::default());
        let enabled = stub.enabled(&ctx);
        assert!(!enabled.is_enabled());
        assert_eq!(enabled.reason.as_deref(), Some("not yet implemented"));
        let error = stub
            .execute(&mut ctx, &Invocation::empty(Source::Test))
            .expect_err("stub");
        assert_eq!(
            error.to_string(),
            "command file.save is not implemented yet"
        );
    }

    #[test]
    fn an_empty_registry_reports_nothing_registered() {
        let registry = Registry::new();
        let id = CommandId::parse("file.save").expect("id");
        let ctx = AppCtx::new(Settings::default());
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
        assert!(registry.get(&id).is_none());
        assert!(registry.meta(&id).is_none());
        assert_eq!(registry.status(&id), CommandStatus::Stub);
        assert_eq!(registry.ids().count(), 0);
        let enabled = registry.enabled(&id, &ctx);
        assert!(!enabled.is_enabled());
    }

    /// A real (implemented) command for the register/execute paths.
    struct RealCommand {
        meta: CommandMeta,
        enabled: bool,
    }

    impl RealCommand {
        fn new(id: &'static str, enabled: bool) -> Box<Self> {
            let mut meta = meta(id);
            meta.status = CommandStatus::Implemented;
            Box::new(Self { meta, enabled })
        }
    }

    impl Command for RealCommand {
        fn meta(&self) -> &CommandMeta {
            &self.meta
        }

        fn enabled(&self, _ctx: &AppCtx) -> Enabled {
            if self.enabled {
                Enabled::yes()
            } else {
                Enabled::no("test says no")
            }
        }

        fn execute(
            &self,
            _ctx: &mut AppCtx,
            invocation: &Invocation,
        ) -> Result<Outcome, CommandError> {
            if invocation.args.flag("fail").unwrap_or(false) {
                return Err(CommandError::failed("test failure"));
            }
            Ok(Outcome::message(format!("ran {}", self.meta.id)))
        }
    }

    #[test]
    fn register_replaces_a_stub_but_never_a_second_command() {
        let mut registry = Registry::new();
        registry
            .register(Box::new(StubCommand::new(meta("file.save"))))
            .expect("stub registers");
        registry
            .register(RealCommand::new("file.save", true))
            .expect("the real command replaces its stub");
        assert_eq!(registry.len(), 1);
        assert_eq!(
            registry.status(&CommandId::parse("file.save").unwrap()),
            CommandStatus::Implemented
        );

        // `replace` swaps any command for another (the test rigs'
        // latching seam, task P7-21).
        registry.replace(RealCommand::new("file.save", false));
        assert_eq!(registry.len(), 1);

        // Two real commands with one id are a build bug.
        let dup = registry.register(RealCommand::new("file.save", true));
        assert!(matches!(dup, Err(RegistryError::Duplicate { .. })));
        // A stub never displaces anything.
        let dup = registry.register(Box::new(StubCommand::new(meta("file.save"))));
        assert!(matches!(dup, Err(RegistryError::Duplicate { .. })));
        // A second stub for one id is a duplicate too.
        let mut registry = Registry::new();
        registry
            .register(Box::new(StubCommand::new(meta("file.save"))))
            .expect("stub registers");
        let dup = registry.register(Box::new(StubCommand::new(meta("file.save"))));
        assert!(matches!(dup, Err(RegistryError::Duplicate { .. })));
    }

    /// P4-15 review minor 1: the registry is `BTreeMap`-backed (ADR
    /// 0028), so the id iteration is sorted, not insertion order.
    #[test]
    fn ids_iterate_in_id_order() {
        let mut registry = Registry::new();
        registry
            .register(RealCommand::new("view.zoom.in", true))
            .expect("register");
        registry
            .register(RealCommand::new("file.save", true))
            .expect("register");
        registry
            .register(RealCommand::new("app.quit", true))
            .expect("register");
        let ids: Vec<&str> = registry.ids().map(|id| id.as_str()).collect();
        assert_eq!(ids, vec!["app.quit", "file.save", "view.zoom.in"]);
    }

    fn mapping(id: &str) -> larrygram_ui_spec::CommandMapping {
        larrygram_ui_spec::CommandMapping {
            id: larrygram_ui_spec::CommandId::new(id),
            label: "Label".to_owned(),
            visuicmd: None,
            fluent_ids: Vec::new(),
            mapping_confidence: MappingConfidence::Unknown,
        }
    }
    #[test]
    fn from_mappings_builds_one_stub_per_entry() {
        let mappings = larrygram_ui_spec::Commands {
            entries: vec![mapping("file.save"), mapping("file.open")],
        };
        let registry = Registry::from_mappings(&mappings).expect("stubs");
        assert_eq!(registry.len(), 2);
        let id = CommandId::parse("file.save").expect("id");
        assert_eq!(registry.status(&id), CommandStatus::Stub);
        let enabled = registry.enabled(&id, &AppCtx::new(Settings::default()));
        assert!(!enabled.is_enabled());
        assert_eq!(enabled.reason.as_deref(), Some("not yet implemented"));

        // A duplicated id in the table is a build bug.
        let mappings = larrygram_ui_spec::Commands {
            entries: vec![mapping("file.save"), mapping("file.save")],
        };
        assert!(matches!(
            Registry::from_mappings(&mappings),
            Err(RegistryError::Duplicate { .. })
        ));

        // A malformed id names the bad id.
        let mappings = larrygram_ui_spec::Commands {
            entries: vec![mapping("file.")],
        };
        assert!(matches!(
            Registry::from_mappings(&mappings),
            Err(RegistryError::BadId { .. })
        ));
    }

    #[test]
    fn execute_dispatches_and_reports_every_failure() {
        let mut registry = Registry::new();
        registry
            .register(RealCommand::new("file.save", true))
            .expect("register");
        registry
            .register(RealCommand::new("edit.undo", false))
            .expect("register");
        registry
            .register(Box::new(StubCommand::new(meta("file.open"))))
            .expect("register");
        let mut ctx = AppCtx::new(Settings::default());

        let outcome = registry
            .execute(
                &CommandId::parse("file.save").unwrap(),
                &mut ctx,
                Invocation::empty(Source::Test),
            )
            .expect("execute");
        assert_eq!(outcome.message.as_deref(), Some("ran file.save"));

        let error = registry
            .execute(
                &CommandId::parse("file.save").unwrap(),
                &mut ctx,
                Invocation::new(Args::new().with("fail", ArgValue::Flag(true)), Source::Test),
            )
            .expect_err("the command fails");
        assert_eq!(error.to_string(), "test failure");

        let error = registry
            .execute(
                &CommandId::parse("edit.undo").unwrap(),
                &mut ctx,
                Invocation::empty(Source::Test),
            )
            .expect_err("disabled");
        assert_eq!(
            error.to_string(),
            "command edit.undo is disabled: test says no"
        );

        let error = registry
            .execute(
                &CommandId::parse("file.open").unwrap(),
                &mut ctx,
                Invocation::empty(Source::Test),
            )
            .expect_err("stub");
        assert_eq!(
            error.to_string(),
            "command file.open is not implemented yet"
        );

        let error = registry
            .execute(
                &CommandId::parse("file.nope").unwrap(),
                &mut ctx,
                Invocation::empty(Source::Test),
            )
            .expect_err("unknown");
        assert_eq!(error.to_string(), "unknown command file.nope");
    }
}
