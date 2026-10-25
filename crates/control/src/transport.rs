//! The local socket of the attached app and its discovery file (ADR
//! 0075, ADR 0081; task P9-06).
//!
//! - **Endpoint**: a Unix domain socket on macOS and Linux (in a
//!   directory only the user can open); a TCP socket bound to
//!   `127.0.0.1` on an ephemeral port on Windows. Never another address:
//!   the alpha has no network listener at all (ADR 0081).
//! - **Discovery**: `control.json` in the per-user runtime directory
//!   ([`runtime_dir`]) names the endpoint, the process id and the token;
//!   it is readable by the user only and removed when the listener
//!   stops.
//! - **Serving**: one thread per connection, at most
//!   `Limits::max_connections`, each running [`crate::serve()`] with a
//!   [`crate::protocol::ControlServer`] that requires the token.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::auth::Token;
use crate::backend::Backend;
use crate::error::ControlError;
use crate::limits::Limits;
use crate::ops::ServerInfo;

/// The discovery file's name inside the runtime directory.
pub const DISCOVERY_FILE: &str = "control.json";

/// Where a listener accepts connections.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
pub enum Endpoint {
    /// A Unix domain socket (macOS, Linux).
    Unix {
        /// The socket path.
        path: PathBuf,
    },
    /// A loopback TCP socket (Windows).
    Tcp {
        /// `127.0.0.1:<port>`.
        address: SocketAddr,
    },
}

/// The discovery file's content (ADR 0081).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Discovery {
    /// The control-protocol version the listener speaks.
    pub version: String,
    /// The app's process id (a stale file names a dead process).
    pub pid: u32,
    /// Where to connect.
    pub endpoint: Endpoint,
    /// The session token, 64 hex digits.
    pub token: String,
}

/// The per-user runtime directory (task P9-06): `$XDG_RUNTIME_DIR` on
/// Linux, the per-user `$TMPDIR` on macOS, `%LOCALAPPDATA%` on Windows,
/// each with a `larrygram` subdirectory created with owner-only
/// permissions. `LARRYGRAM_RUNTIME_DIR` overrides it (tests).
pub fn runtime_dir() -> Result<PathBuf, ControlError> {
    Err(ControlError::not_implemented("transport::runtime_dir"))
}

/// Writes `discovery` into `dir` with owner-only permissions (task
/// P9-06), replacing a stale file; returns the file's path.
pub fn write_discovery(dir: &Path, discovery: &Discovery) -> Result<PathBuf, ControlError> {
    let _ = (dir, discovery);
    Err(ControlError::not_implemented("transport::write_discovery"))
}

/// Reads the discovery file in `dir` (task P9-06). A missing file is
/// `NotFound` ("is the app running with agent control on?").
pub fn read_discovery(dir: &Path) -> Result<Discovery, ControlError> {
    let _ = dir;
    Err(ControlError::not_implemented("transport::read_discovery"))
}

/// A bound, not yet serving, listener (task P9-06).
#[derive(Debug)]
pub struct Listener {
    endpoint: Endpoint,
}

impl Listener {
    /// Binds the platform endpoint inside `dir` (task P9-06). A live
    /// listener of another process is `Busy`; a stale socket file is
    /// removed first.
    pub fn bind(dir: &Path, limits: &Limits) -> Result<Listener, ControlError> {
        let _ = (dir, limits);
        Err(ControlError::not_implemented("transport::Listener::bind"))
    }

    /// Where the listener accepts connections.
    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    /// Starts the accept thread (task P9-06): writes the discovery file,
    /// then serves each connection on its own thread with a
    /// `ControlServer` announcing `info` and requiring `token`, over a
    /// backend from `make_backend`.
    pub fn spawn<B, F>(
        self,
        dir: &Path,
        token: Token,
        info: ServerInfo,
        make_backend: F,
        limits: Limits,
    ) -> Result<ListenerHandle, ControlError>
    where
        B: Backend + Send + 'static,
        F: Fn() -> B + Send + 'static,
    {
        let _ = (dir, token, info, make_backend, limits);
        Err(ControlError::not_implemented("transport::Listener::spawn"))
    }
}

/// A running listener (task P9-06). Dropping it stops it, as
/// [`ListenerHandle::stop`] does.
#[derive(Debug)]
pub struct ListenerHandle {
    endpoint: Endpoint,
}

impl ListenerHandle {
    /// Where the listener accepts connections.
    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    /// Connections being served now.
    pub fn connections(&self) -> usize {
        0
    }

    /// Stops accepting, closes the open connections, and removes the
    /// socket and the discovery file (task P9-06).
    pub fn stop(self) -> Result<(), ControlError> {
        Err(ControlError::not_implemented(
            "transport::ListenerHandle::stop",
        ))
    }
}

/// A client connection to an endpoint (task P9-06): a byte stream the
/// remote backend frames.
#[derive(Debug)]
pub struct Connection {
    endpoint: Endpoint,
}

impl Connection {
    /// Connects to `endpoint` (task P9-06).
    pub fn connect(endpoint: &Endpoint) -> Result<Connection, ControlError> {
        let _ = endpoint;
        Err(ControlError::not_implemented(
            "transport::Connection::connect",
        ))
    }

    /// The endpoint this connection reaches.
    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;
    use crate::ops::{Op, OpResult};

    fn stub(error: ControlError) {
        assert_eq!(error.code(), ErrorCode::NotImplemented);
    }

    struct NoBackend;

    impl Backend for NoBackend {
        fn call(&mut self, _op: Op) -> Result<OpResult, ControlError> {
            Err(ControlError::NoDocument)
        }
    }

    #[test]
    fn the_stubs_report_not_implemented() {
        let dir = Path::new("/nonexistent/larrygram-test");
        stub(runtime_dir().expect_err("stub"));
        let endpoint = Endpoint::Unix {
            path: dir.join("control.sock"),
        };
        let discovery = Discovery {
            version: "1.0".into(),
            pid: 1,
            endpoint: endpoint.clone(),
            token: "00".into(),
        };
        stub(write_discovery(dir, &discovery).expect_err("stub"));
        stub(read_discovery(dir).expect_err("stub"));
        stub(Listener::bind(dir, &Limits::default()).expect_err("stub"));
        let listener = Listener {
            endpoint: endpoint.clone(),
        };
        assert_eq!(listener.endpoint(), &endpoint);
        stub(
            listener
                .spawn(
                    dir,
                    Token::from_bytes([1; 32]),
                    ServerInfo::default(),
                    || NoBackend,
                    Limits::default(),
                )
                .expect_err("stub"),
        );
        let handle = ListenerHandle {
            endpoint: endpoint.clone(),
        };
        assert_eq!(handle.endpoint(), &endpoint);
        assert_eq!(handle.connections(), 0);
        stub(handle.stop().expect_err("stub"));
        stub(Connection::connect(&endpoint).expect_err("stub"));
        let connection = Connection {
            endpoint: endpoint.clone(),
        };
        assert_eq!(connection.endpoint(), &endpoint);
        let mut backend = NoBackend;
        assert!(backend.call(Op::Discover).is_err());
    }

    #[test]
    fn the_discovery_file_has_a_tagged_endpoint() {
        let discovery = Discovery {
            version: "1.0".into(),
            pid: 42,
            endpoint: Endpoint::Tcp {
                address: SocketAddr::from(([127, 0, 0, 1], 4100)),
            },
            token: "ab".into(),
        };
        let value = serde_json::to_value(&discovery).unwrap();
        assert_eq!(value["endpoint"]["kind"], "tcp");
        assert_eq!(value["endpoint"]["address"], "127.0.0.1:4100");
        let back: Discovery = serde_json::from_value(value).unwrap();
        assert_eq!(back, discovery);
        assert_eq!(DISCOVERY_FILE, "control.json");
    }
}
