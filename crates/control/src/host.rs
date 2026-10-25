//! The [`Host`] a session runs against (ADR 0076).
//!
//! A host owns the registry and the application state. The executor
//! reads the state through [`Host::app`] and changes it only through
//! [`Host::dispatch`], so every remote edit goes through the registry,
//! lands on the document's undo stack and is undone like any other edit
//! (ADR 0029: nothing but commands mutates `AppCtx`).
//!
//! Two hosts implement it: [`HeadlessHost`] here (the CLI), and the
//! desktop app's shell adapter in `apps/larrygram` (task P9-10), which
//! dispatches through the shell so the canvas and panes see the change.

use larrygram_commands::app::{AppCtx, Settings};
use larrygram_commands::{CommandError, CommandId, Invocation, Outcome, Registry};

use crate::error::ControlError;

/// The registry and state an executor runs against (ADR 0076).
pub trait Host {
    /// The command registry.
    fn registry(&self) -> &Registry;

    /// The application state, read-only.
    fn app(&self) -> &AppCtx;

    /// Runs one command through the registry and records its edit on the
    /// undo stack (`larrygram_commands::dispatch` semantics).
    fn dispatch(&mut self, id: &CommandId, invocation: Invocation)
    -> Result<Outcome, CommandError>;

    /// Why the host cannot take a remote operation right now (a drag or
    /// a text edit in progress in the app), or `None` when it can. The
    /// executor answers `Busy` with the reason (ADR 0076).
    fn busy(&self) -> Option<String> {
        None
    }
}

/// A session without a window: the registry and an [`AppCtx`] in the
/// calling process (ADR 0076).
pub struct HeadlessHost {
    registry: Registry,
    app: AppCtx,
}

impl std::fmt::Debug for HeadlessHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `Registry` holds trait objects and has no `Debug`; its size
        // says enough.
        f.debug_struct("HeadlessHost")
            .field("commands", &self.registry.len())
            .field("app", &self.app)
            .finish()
    }
}

impl HeadlessHost {
    /// A host over `registry` and `app`.
    pub fn new(registry: Registry, app: AppCtx) -> Self {
        Self { registry, app }
    }

    /// The builtin registry (`larrygram_commands::builtins::registry`)
    /// and default settings, no document open: the CLI's session.
    pub fn builtin() -> Result<Self, ControlError> {
        let registry =
            larrygram_commands::builtins::registry().map_err(|error| ControlError::Internal {
                message: error.to_string(),
            })?;
        Ok(Self::new(registry, AppCtx::new(Settings::default())))
    }

    /// The application state, mutably (tests and the CLI seed it).
    pub fn app_mut(&mut self) -> &mut AppCtx {
        &mut self.app
    }

    /// The registry and the state, by value.
    pub fn into_parts(self) -> (Registry, AppCtx) {
        (self.registry, self.app)
    }
}

impl Host for HeadlessHost {
    fn registry(&self) -> &Registry {
        &self.registry
    }

    fn app(&self) -> &AppCtx {
        &self.app
    }

    fn dispatch(
        &mut self,
        id: &CommandId,
        invocation: Invocation,
    ) -> Result<Outcome, CommandError> {
        larrygram_commands::dispatch(&self.registry, &mut self.app, id, invocation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use larrygram_commands::{Args, Source};

    #[test]
    fn the_builtin_host_runs_commands_through_the_registry() {
        let mut host = HeadlessHost::builtin().expect("the builtin registry builds");
        assert!(!host.registry().is_empty());
        assert!(host.app().documents.is_empty());
        assert_eq!(host.busy(), None);
        let new = CommandId::parse("file.new").expect("id");
        host.dispatch(&new, Invocation::new(Args::new(), Source::Control))
            .expect("file.new runs headless");
        assert_eq!(host.app().documents.len(), 1);
        let unknown = CommandId::parse("file.nothing_here").expect("id");
        let error = host
            .dispatch(&unknown, Invocation::empty(Source::Control))
            .expect_err("unknown");
        assert!(matches!(error, CommandError::UnknownCommand { .. }));
        assert!(format!("{host:?}").starts_with("HeadlessHost"));
        host.app_mut().documents.clear();
        let (registry, app) = host.into_parts();
        assert!(!registry.is_empty());
        assert!(app.documents.is_empty());
    }
}
