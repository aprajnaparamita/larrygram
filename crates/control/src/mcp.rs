//! The MCP front end (ADR 0080; task P9-07).
//!
//! A dual-era MCP server over the same framing as the control protocol:
//!
//! - **Modern** ([`MODERN_VERSION`], 2026-07-28): stateless. Every
//!   request carries its protocol version in `_meta`; `server/discover`
//!   announces the versions, capabilities and identity; every result
//!   carries `resultType` and the list results carry `ttlMs` and
//!   `cacheScope`.
//! - **Legacy** ([`LEGACY_VERSIONS`]): an `initialize` request selects
//!   the legacy semantics for the rest of the stdio process, with
//!   `notifications/initialized` and `ping`.
//!
//! Only the `tools` capability is offered. Each tool in [`TOOLS`] maps
//! onto one [`Op`]; the tool list is fixed and in a deterministic order
//! (the registry's commands are reached through `list_commands` and
//! `run_command`, never one tool per command). A failed operation is a
//! tool result with `isError: true` and the error line, so the model
//! can correct itself; an unknown tool or malformed params are JSON-RPC
//! errors.

use serde_json::{Map, Value};

use crate::backend::Backend;
use crate::error::ControlError;
use crate::jsonrpc::{Message, Response};
use crate::ops::{Op, OpResult, ServerInfo};
use crate::serve::FrontEnd;

/// The modern MCP revision this server implements.
pub const MODERN_VERSION: &str = "2026-07-28";

/// The legacy (handshake) revisions it answers `initialize` with, newest
/// first.
pub const LEGACY_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18"];

/// The `_meta` key of the per-request protocol version (modern).
pub const META_PROTOCOL_VERSION: &str = "io.modelcontextprotocol/protocolVersion";

/// The `ttlMs` the tool list announces: the list never changes while
/// the process lives, so one hour.
pub const TOOLS_TTL_MS: u64 = 3_600_000;

/// Which operation a tool runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ToolOp {
    /// [`Op::ListCommands`].
    ListCommands,
    /// [`Op::DescribeCommand`].
    DescribeCommand,
    /// [`Op::Invoke`].
    RunCommand,
    /// [`Op::ListDocuments`].
    ListDocuments,
    /// [`Op::InspectDocument`].
    InspectDocument,
    /// [`Op::InspectPage`].
    InspectPage,
    /// [`Op::InspectShape`].
    InspectShape,
    /// [`Op::InspectSelection`].
    GetSelection,
    /// [`Op::Render`].
    RenderPage,
}

/// One MCP tool (ADR 0080). The input schema is built from the op's
/// params struct by [`input_schema`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tool {
    /// The tool name (`snake_case`, `[a-z_]`).
    pub name: &'static str,
    /// A short display title.
    pub title: &'static str,
    /// What it does, for the model (our own words).
    pub description: &'static str,
    /// The operation it runs.
    pub op: ToolOp,
    /// Whether it only reads (MCP `readOnlyHint`).
    pub read_only: bool,
}

/// The tools, in the order `tools/list` returns them (ADR 0080).
pub const TOOLS: &[Tool] = &[
    Tool {
        name: "list_commands",
        title: "List commands",
        description: "List the Larrygram commands you can run, with their ids, labels and \
                      whether they are enabled now. Filter by text; page with the cursor.",
        op: ToolOp::ListCommands,
        read_only: true,
    },
    Tool {
        name: "describe_command",
        title: "Describe a command",
        description: "Show one command's arguments: names, types, which are required, \
                      allowed values and ranges. Call this before run_command.",
        op: ToolOp::DescribeCommand,
        read_only: true,
    },
    Tool {
        name: "run_command",
        title: "Run a command",
        description: "Run one Larrygram command by id with its arguments, on the active or \
                      a named document and page. Each run is one undo step. Commands never \
                      open dialogs: a missing argument is reported back.",
        op: ToolOp::RunCommand,
        read_only: false,
    },
    Tool {
        name: "list_documents",
        title: "List documents",
        description: "List the open documents with their handles, names, paths, page \
                      counts and unsaved state.",
        op: ToolOp::ListDocuments,
        read_only: true,
    },
    Tool {
        name: "inspect_document",
        title: "Inspect a document",
        description: "Show a document's pages (names, sizes in inches, shape counts) and \
                      its masters.",
        op: ToolOp::InspectDocument,
        read_only: true,
    },
    Tool {
        name: "inspect_page",
        title: "Inspect a page",
        description: "List a page's shapes (ids, names, masters, text, position and size \
                      in inches) and its glue connections. Filter and page through large \
                      pages.",
        op: ToolOp::InspectPage,
        read_only: true,
    },
    Tool {
        name: "inspect_shape",
        title: "Inspect a shape",
        description: "Show one shape by its id: its full text and, on request, its \
                      ShapeSheet cells by section.",
        op: ToolOp::InspectShape,
        read_only: true,
    },
    Tool {
        name: "get_selection",
        title: "Get the selection",
        description: "Show which shapes are selected in a document.",
        op: ToolOp::GetSelection,
        read_only: true,
    },
    Tool {
        name: "render_page",
        title: "Render a page",
        description: "Render a page, or a region of it, as a PNG image (or SVG) so you can \
                      check your work.",
        op: ToolOp::RenderPage,
        read_only: true,
    },
];

/// The tool named `name`, if any.
pub fn tool(name: &str) -> Option<&'static Tool> {
    TOOLS.iter().find(|tool| tool.name == name)
}

/// The JSON Schema (2020-12) of a tool's arguments (task P9-07): the
/// op's params struct, every field described, `additionalProperties:
/// false`.
pub fn input_schema(tool: &Tool) -> Value {
    let _ = tool;
    Value::Null
}

/// The operation a `tools/call` asks for (task P9-07). Arguments that do
/// not fit the tool are `InvalidParams`.
pub fn tool_call_op(tool: &Tool, arguments: &Map<String, Value>) -> Result<Op, ControlError> {
    let _ = (tool, arguments);
    Err(ControlError::not_implemented("mcp::tool_call_op"))
}

/// The MCP `CallToolResult` for an operation's outcome (task P9-07):
/// `structuredContent` with the result struct and a text block with the
/// same JSON; a `render_page` image as image content; an error as
/// `isError: true` with its line.
pub fn tool_result(outcome: Result<OpResult, ControlError>) -> Value {
    let _ = outcome;
    Value::Null
}

/// Which MCP era a connection speaks (ADR 0080).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Era {
    /// Nothing received yet.
    Unknown,
    /// Per-request metadata, no handshake.
    Modern,
    /// `initialize` was answered with this version.
    Legacy {
        /// The negotiated legacy version.
        version: String,
    },
}

/// The MCP front end of one connection (task P9-07).
#[derive(Debug)]
pub struct McpServer {
    info: ServerInfo,
    era: Era,
}

impl McpServer {
    /// A server announcing `info` (its `name` and `version` become the
    /// MCP `serverInfo`).
    pub fn new(info: ServerInfo) -> Self {
        Self {
            info,
            era: Era::Unknown,
        }
    }

    /// The era chosen so far.
    pub fn era(&self) -> &Era {
        &self.era
    }

    /// The server identity this front end announces.
    pub fn info(&self) -> &ServerInfo {
        &self.info
    }
}

impl FrontEnd for McpServer {
    fn handle(&mut self, backend: &mut dyn Backend, message: Message) -> Option<Response> {
        let _ = backend;
        match message {
            Message::Request(request) => Some(Response::error(
                Some(request.id),
                &ControlError::not_implemented("mcp::McpServer::handle"),
            )),
            Message::Notification(_) => None,
        }
    }

    fn close_after(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;
    use crate::jsonrpc::{Id, Notification, Request};

    struct NoBackend;

    impl Backend for NoBackend {
        fn call(&mut self, _op: Op) -> Result<OpResult, ControlError> {
            panic!("the stub front end must not call the backend")
        }
    }

    #[test]
    fn the_tool_table_is_well_formed() {
        let mut names = std::collections::BTreeSet::new();
        for tool in TOOLS {
            assert!(names.insert(tool.name), "{} is listed twice", tool.name);
            assert!(
                tool.name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c == '_'),
                "{}",
                tool.name
            );
            assert!(!tool.title.is_empty() && !tool.description.is_empty());
            assert!(!tool.description.contains("  "), "{}", tool.name);
            assert_eq!(super::tool(tool.name), Some(tool));
        }
        assert_eq!(TOOLS.len(), 9);
        assert_eq!(
            TOOLS.iter().filter(|tool| !tool.read_only).count(),
            1,
            "only run_command changes anything"
        );
        assert_eq!(super::tool("nope"), None);
    }

    #[test]
    fn the_stubs_report_not_implemented() {
        let run = super::tool("run_command").expect("listed");
        assert_eq!(input_schema(run), Value::Null);
        let error = tool_call_op(run, &Map::new()).expect_err("stub");
        assert_eq!(error.code(), ErrorCode::NotImplemented);
        assert_eq!(tool_result(Err(ControlError::NoDocument)), Value::Null);

        let mut server = McpServer::new(ServerInfo::default());
        assert_eq!(server.era(), &Era::Unknown);
        assert_eq!(server.info(), &ServerInfo::default());
        assert!(!server.close_after());
        let response = server
            .handle(
                &mut NoBackend,
                Message::Request(Request {
                    id: Id::Number(1),
                    method: "server/discover".into(),
                    params: None,
                }),
            )
            .expect("a request gets a response");
        assert_eq!(response.outcome.expect_err("stub").code, -32013);
        let dropped = server.handle(
            &mut NoBackend,
            Message::Notification(Notification {
                method: "notifications/initialized".into(),
                params: None,
            }),
        );
        assert!(dropped.is_none());
        assert_ne!(
            Era::Modern,
            Era::Legacy {
                version: LEGACY_VERSIONS[0].into()
            }
        );
        assert_eq!(MODERN_VERSION, "2026-07-28");
        assert!(META_PROTOCOL_VERSION.ends_with("protocolVersion"));
        assert_eq!(TOOLS_TTL_MS, 3_600_000);
    }
}
