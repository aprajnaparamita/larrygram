//! JSON control channel and MCP server that dispatch to the command
//! registry (Phase 9, ADRs 0075 to 0082).
//!
//! One implementation, two front ends (ADR 0080): the typed operations
//! in [`ops`] are executed by one [`exec::Executor`] against a
//! [`host::Host`] (a headless session or the running desktop app,
//! ADR 0076). The control protocol ([`protocol`], ADR 0075) and MCP
//! ([`mcp`], ADR 0080) are two encodings of the same operations, both
//! newline-delimited JSON-RPC 2.0 ([`jsonrpc`], [`framing`]).
//! Commands run only through the registry (AGENTS.md section 6): the
//! registry is the single list of what a remote caller can do, and a
//! programmatic call never opens a dialog (ADR 0077).
//!
//! Layer L5 (ADR 0002): the crate depends on `commands` and the layers
//! below it, never on egui. The desktop app (L7) attaches its shell
//! through [`inbox`]; the CLI (L7) runs a [`HeadlessBackend`].
//!
//! Alpha: the public types and signatures are frozen by task P9-01;
//! the bodies are "not implemented" stubs ([`ControlError::NotImplemented`])
//! that the Phase 9 tasks fill in (`docs/plans/phase-9.md`).

#![forbid(unsafe_code)]

pub mod args;
pub mod auth;
pub mod backend;
pub mod error;
pub mod exec;
pub mod framing;
pub mod host;
pub mod inbox;
pub mod inspect;
pub mod jsonrpc;
pub mod limits;
pub mod mcp;
pub mod ops;
pub mod policy;
pub mod protocol;
pub mod remote;
pub mod render;
pub mod serve;
pub mod transport;

pub use backend::{Backend, HeadlessBackend};
pub use error::{ControlError, ErrorCode};
pub use exec::Executor;
pub use host::{HeadlessHost, Host};
pub use limits::Limits;
pub use ops::{Op, OpResult};
pub use policy::Policy;
pub use serve::{FrontEnd, ServeEnd, serve};
