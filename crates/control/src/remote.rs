//! The client side of the control protocol (ADR 0080; task P9-06).
//!
//! [`RemoteBackend`] reads the discovery file, connects to the attached
//! app, says `hello` with the token and then runs each [`Op`] as one
//! control-protocol request. The CLI's `mcp --attach` puts the MCP
//! front end on top of it, so an MCP client drives the running app
//! through the same operations as a headless session.

use std::path::Path;

use crate::backend::Backend;
use crate::error::ControlError;
use crate::limits::Limits;
use crate::ops::{Op, OpResult, ServerInfo};
use crate::transport::Connection;

/// A backend that forwards operations to an attached app (task P9-06).
#[derive(Debug)]
pub struct RemoteBackend {
    connection: Connection,
    server: ServerInfo,
    next_id: i64,
}

impl RemoteBackend {
    /// Connects through the discovery file in `dir` and says `hello`
    /// as `client` (task P9-06). The server's errors come back as
    /// [`ControlError::Remote`].
    pub fn connect(dir: &Path, client: &str, limits: &Limits) -> Result<Self, ControlError> {
        let _ = (dir, client, limits);
        Err(ControlError::not_implemented(
            "remote::RemoteBackend::connect",
        ))
    }

    /// What the server announced in `hello`.
    pub fn server(&self) -> &ServerInfo {
        &self.server
    }
}

impl Backend for RemoteBackend {
    fn call(&mut self, op: Op) -> Result<OpResult, ControlError> {
        let _ = (op, &self.connection, &mut self.next_id);
        Err(ControlError::not_implemented("remote::RemoteBackend::call"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;
    use crate::transport::Endpoint;

    #[test]
    fn the_stubs_report_not_implemented() {
        let limits = Limits::default();
        let error =
            RemoteBackend::connect(Path::new("/nonexistent"), "test", &limits).expect_err("stub");
        assert_eq!(error.code(), ErrorCode::NotImplemented);
        // A value built by hand still answers the contract.
        let endpoint = Endpoint::Unix {
            path: "/nonexistent/control.sock".into(),
        };
        let error = Connection::connect(&endpoint).expect_err("stub");
        assert_eq!(error.code(), ErrorCode::NotImplemented);
    }
}
