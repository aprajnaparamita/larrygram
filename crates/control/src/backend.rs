//! The [`Backend`] seam between the front ends and the work (ADR 0080).
//!
//! A front end turns messages into [`Op`] values and asks a backend to
//! run them. Three backends exist:
//!
//! - [`HeadlessBackend`]: an [`Executor`] over a [`HeadlessHost`] in the
//!   same process (the CLI's `serve` and `mcp`, ADR 0076).
//! - [`crate::inbox::InboxSender`]: hands the op to the desktop app's UI
//!   thread and waits for the answer (the attached app, ADR 0076).
//! - [`crate::remote::RemoteBackend`]: sends the op over a socket to an
//!   attached app (the CLI's `mcp --attach`, ADR 0080).

use crate::error::ControlError;
use crate::exec::Executor;
use crate::host::HeadlessHost;
use crate::ops::{Op, OpResult};

/// Runs operations (ADR 0080).
pub trait Backend {
    /// Runs one operation and returns its result or its error.
    fn call(&mut self, op: Op) -> Result<OpResult, ControlError>;
}

/// An executor and a headless host in one value (ADR 0076).
#[derive(Debug)]
pub struct HeadlessBackend {
    host: HeadlessHost,
    executor: Executor,
}

impl HeadlessBackend {
    /// Pairs a host with the executor that serves it.
    pub fn new(host: HeadlessHost, executor: Executor) -> Self {
        Self { host, executor }
    }

    /// The headless host (its registry and state).
    pub fn host(&self) -> &HeadlessHost {
        &self.host
    }

    /// The headless host, mutably (tests seed documents through it).
    pub fn host_mut(&mut self) -> &mut HeadlessHost {
        &mut self.host
    }

    /// The executor.
    pub fn executor(&self) -> &Executor {
        &self.executor
    }
}

impl Backend for HeadlessBackend {
    fn call(&mut self, op: Op) -> Result<OpResult, ControlError> {
        self.executor.execute(&mut self.host, op)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;
    use crate::host::Host;
    use crate::limits::Limits;
    use crate::policy::Policy;
    use larrygram_commands::Registry;
    use larrygram_commands::app::{AppCtx, Settings};

    #[test]
    fn the_headless_backend_delegates_to_the_executor() {
        let host = HeadlessHost::new(Registry::new(), AppCtx::new(Settings::default()));
        let executor = Executor::new(Policy::default(), Limits::default(), false);
        let mut backend = HeadlessBackend::new(host, executor);
        assert!(backend.host().app().documents.is_empty());
        assert!(backend.host_mut().app().documents.is_empty());
        assert!(!backend.executor().attached());
        let error = backend.call(Op::ListDocuments).expect_err("stub");
        assert_eq!(error.code(), ErrorCode::NotImplemented);
    }
}
