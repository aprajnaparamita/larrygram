//! The [`Executor`]: the one implementation behind both front ends
//! (ADR 0080).
//!
//! It runs an [`Op`] against a [`Host`]: listing and describing
//! commands from the registry ([`crate::args`]), invoking them after the
//! policy checks ([`crate::policy`]), inspecting the state
//! ([`crate::inspect`]) and rendering ([`crate::render`]). It owns the
//! document handles ([`Handles`]) and is the only place the
//! `attached` flag changes behaviour (ADR 0076, ADR 0081).

use crate::error::ControlError;
use crate::host::Host;
use crate::inspect::Handles;
use crate::limits::Limits;
use crate::ops::{Capabilities, LimitsInfo, Op, OpResult, ServerInfo};
use crate::policy::Policy;
use crate::protocol::SUPPORTED_VERSIONS;

/// Runs operations against a host (ADR 0080; task P9-08).
#[derive(Debug)]
pub struct Executor {
    policy: Policy,
    limits: Limits,
    attached: bool,
    handles: Handles,
}

impl Executor {
    /// An executor with `policy` and `limits`; `attached` is true when
    /// the host is the running desktop app (ADR 0076).
    pub fn new(policy: Policy, limits: Limits, attached: bool) -> Self {
        Self {
            policy,
            limits,
            attached,
            handles: Handles::new(),
        }
    }

    /// Whether the host is the running desktop app.
    pub fn attached(&self) -> bool {
        self.attached
    }

    /// The security policy (ADR 0081).
    pub fn policy(&self) -> &Policy {
        &self.policy
    }

    /// The limits (ADR 0081).
    pub fn limits(&self) -> &Limits {
        &self.limits
    }

    /// The document handles minted so far.
    pub fn handles(&self) -> &Handles {
        &self.handles
    }

    /// What `server/discover` and `hello` announce (ADR 0075).
    pub fn server_info(&self) -> ServerInfo {
        ServerInfo {
            name: "larrygram".to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            protocol_versions: SUPPORTED_VERSIONS.iter().map(|v| (*v).to_owned()).collect(),
            attached: self.attached,
            capabilities: Capabilities {
                commands: true,
                inspect: true,
                render_formats: vec!["png".to_owned(), "svg".to_owned()],
            },
            limits: LimitsInfo {
                max_frame_bytes: self.limits.max_frame_bytes,
                max_page_items: self.limits.max_page_items,
                max_render_pixels: self.limits.max_render_pixels,
                max_render_side: self.limits.max_render_side,
            },
        }
    }

    /// Runs one operation (task P9-08).
    ///
    /// Before anything else: syncs the handles with the host's open
    /// documents and answers `Busy` when [`Host::busy`] says so. An
    /// `Invoke` resolves its `document` and `page` through the registry
    /// (`file.activate`, `page.go`, ADR 0077), checks the command against
    /// the policy, converts the JSON arguments with the command's schema
    /// and dispatches with `Source::Control`.
    pub fn execute(&mut self, host: &mut dyn Host, op: Op) -> Result<OpResult, ControlError> {
        let _ = (host, op, &mut self.handles);
        Err(ControlError::not_implemented("exec::Executor::execute"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;
    use crate::host::HeadlessHost;
    use larrygram_commands::Registry;
    use larrygram_commands::app::{AppCtx, Settings};

    #[test]
    fn the_executor_describes_itself_and_stubs_execute() {
        let mut executor = Executor::new(Policy::default(), Limits::default(), true);
        assert!(executor.attached());
        assert_eq!(executor.policy(), &Policy::default());
        assert_eq!(executor.limits(), &Limits::default());
        assert!(executor.handles().is_empty());
        let info = executor.server_info();
        assert_eq!(info.name, "larrygram");
        assert!(info.attached);
        assert_eq!(info.protocol_versions, ["1.0"]);
        assert_eq!(info.capabilities.render_formats, ["png", "svg"]);
        assert_eq!(info.limits.max_page_items, Limits::default().max_page_items);

        let mut host = HeadlessHost::new(Registry::new(), AppCtx::new(Settings::default()));
        let error = executor.execute(&mut host, Op::Discover).expect_err("stub");
        assert_eq!(error.code(), ErrorCode::NotImplemented);
    }
}
