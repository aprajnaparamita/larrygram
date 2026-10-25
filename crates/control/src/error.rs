//! The one error type of the control crate and its JSON-RPC codes
//! (ADR 0075).
//!
//! Every failure a remote caller can see is a [`ControlError`]. It maps
//! to exactly one [`ErrorCode`] and a one-line message; the code table is
//! part of the protocol and never changes meaning within a major
//! version. The codes stay in the JSON-RPC implementation-defined range
//! `-32000..=-32019`, which MCP leaves to implementations (MCP reserves
//! `-32020..=-32099` for itself, ADR 0080).

use std::fmt;

use larrygram_commands::{CommandError, NeededInput};

/// A JSON-RPC error code (ADR 0075).
///
/// The five standard codes and the Larrygram codes. The numeric value is
/// what goes on the wire ([`ErrorCode::value`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ErrorCode {
    /// `-32700`: the bytes are not JSON.
    ParseError,
    /// `-32600`: JSON, but not a valid JSON-RPC 2.0 request.
    InvalidRequest,
    /// `-32601`: the method does not exist.
    MethodNotFound,
    /// `-32602`: the parameters do not fit the method.
    InvalidParams,
    /// `-32603`: a fault inside the server.
    InternalError,
    /// `-32000`: the command ran and failed (`CommandError::Failed`).
    CommandFailed,
    /// `-32001`: no command has this id.
    UnknownCommand,
    /// `-32002`: the command is disabled in the current state.
    NotEnabled,
    /// `-32003`: the command needs an argument the call did not supply.
    NeedsInput,
    /// `-32004`: the command is registered but not implemented yet.
    Unsupported,
    /// `-32005`: the security policy refuses the call (ADR 0081).
    Denied,
    /// `-32006`: a size, count or rate limit was hit (ADR 0081).
    LimitExceeded,
    /// `-32007`: a document, page or shape reference matched nothing.
    NotFound,
    /// `-32008`: the operation needs an open document and there is none.
    NoDocument,
    /// `-32009`: the connection has not presented the right token.
    Unauthorized,
    /// `-32010`: the connection has not said `hello` yet.
    NotReady,
    /// `-32011`: no protocol version both sides support.
    UnsupportedVersion,
    /// `-32012`: the host cannot run the operation now (a drag or a text
    /// edit in progress in the app, or the app did not answer in time).
    Busy,
    /// `-32013`: the operation is not implemented yet (the P9-01 stubs).
    NotImplemented,
}

impl ErrorCode {
    /// Every code, in wire order (for tests and the protocol document).
    pub const ALL: [ErrorCode; 19] = [
        ErrorCode::ParseError,
        ErrorCode::InvalidRequest,
        ErrorCode::MethodNotFound,
        ErrorCode::InvalidParams,
        ErrorCode::InternalError,
        ErrorCode::CommandFailed,
        ErrorCode::UnknownCommand,
        ErrorCode::NotEnabled,
        ErrorCode::NeedsInput,
        ErrorCode::Unsupported,
        ErrorCode::Denied,
        ErrorCode::LimitExceeded,
        ErrorCode::NotFound,
        ErrorCode::NoDocument,
        ErrorCode::Unauthorized,
        ErrorCode::NotReady,
        ErrorCode::UnsupportedVersion,
        ErrorCode::Busy,
        ErrorCode::NotImplemented,
    ];

    /// The numeric code on the wire.
    pub fn value(self) -> i64 {
        match self {
            ErrorCode::ParseError => -32700,
            ErrorCode::InvalidRequest => -32600,
            ErrorCode::MethodNotFound => -32601,
            ErrorCode::InvalidParams => -32602,
            ErrorCode::InternalError => -32603,
            ErrorCode::CommandFailed => -32000,
            ErrorCode::UnknownCommand => -32001,
            ErrorCode::NotEnabled => -32002,
            ErrorCode::NeedsInput => -32003,
            ErrorCode::Unsupported => -32004,
            ErrorCode::Denied => -32005,
            ErrorCode::LimitExceeded => -32006,
            ErrorCode::NotFound => -32007,
            ErrorCode::NoDocument => -32008,
            ErrorCode::Unauthorized => -32009,
            ErrorCode::NotReady => -32010,
            ErrorCode::UnsupportedVersion => -32011,
            ErrorCode::Busy => -32012,
            ErrorCode::NotImplemented => -32013,
        }
    }

    /// The code for a wire value, or `None` for a value outside the table.
    pub fn from_value(value: i64) -> Option<ErrorCode> {
        ErrorCode::ALL
            .into_iter()
            .find(|code| code.value() == value)
    }
}

/// A control-channel failure (ADR 0075). One line of text, never a
/// panic path.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ControlError {
    /// The incoming bytes are not JSON.
    Parse {
        /// What the JSON reader reported.
        message: String,
    },
    /// The message is JSON but not a valid request.
    InvalidRequest {
        /// What is wrong with it.
        message: String,
    },
    /// The method name is unknown to this front end.
    MethodNotFound {
        /// The method the caller asked for.
        method: String,
    },
    /// The parameters do not fit the method or the command's arguments.
    InvalidParams {
        /// What is wrong with them.
        message: String,
    },
    /// A command reported an error through the registry.
    Command(CommandError),
    /// The security policy refuses the call (ADR 0081).
    Denied {
        /// Why, in one line the caller can act on.
        reason: String,
    },
    /// A limit was hit (ADR 0081).
    LimitExceeded {
        /// Which limit (the [`crate::Limits`] field name).
        limit: &'static str,
        /// The value that was asked for or reached.
        value: u64,
        /// The limit's maximum.
        max: u64,
    },
    /// A document, page or shape reference matched nothing.
    NotFound {
        /// What kind of thing (`document`, `page`, `shape`, `cursor`).
        what: &'static str,
        /// The reference as the caller wrote it.
        reference: String,
    },
    /// The operation needs an open document.
    NoDocument,
    /// The connection has not authenticated (ADR 0081).
    Unauthorized,
    /// The connection must say `hello` first (ADR 0075).
    NotReady,
    /// No protocol version in common (ADR 0075).
    UnsupportedVersion {
        /// The version the caller asked for.
        requested: String,
        /// The versions this server speaks.
        supported: Vec<String>,
    },
    /// The host cannot run the operation now (ADR 0076).
    Busy {
        /// Why, in one line.
        reason: String,
    },
    /// A transport or file-system failure (sockets, the discovery file).
    Io {
        /// The operating system's message.
        message: String,
    },
    /// A fault inside the server that is not the caller's doing.
    Internal {
        /// What went wrong.
        message: String,
    },
    /// An error a remote server returned (the client side,
    /// [`crate::remote`]): its wire code, message and data, unchanged.
    Remote {
        /// The wire code.
        code: i64,
        /// The server's one-line message.
        message: String,
        /// The server's `data` member.
        data: Option<serde_json::Value>,
    },
    /// The operation is a P9-01 stub that a Phase 9 task fills in.
    NotImplemented {
        /// The stubbed item, as `module::function`.
        what: &'static str,
    },
}

impl ControlError {
    /// The [`ControlError::NotImplemented`] value for one stub.
    pub fn not_implemented(what: &'static str) -> Self {
        ControlError::NotImplemented { what }
    }

    /// The JSON-RPC code of this error (ADR 0075 table).
    pub fn code(&self) -> ErrorCode {
        match self {
            ControlError::Parse { .. } => ErrorCode::ParseError,
            ControlError::InvalidRequest { .. } => ErrorCode::InvalidRequest,
            ControlError::MethodNotFound { .. } => ErrorCode::MethodNotFound,
            ControlError::InvalidParams { .. } => ErrorCode::InvalidParams,
            ControlError::Command(error) => command_code(error),
            ControlError::Denied { .. } => ErrorCode::Denied,
            ControlError::LimitExceeded { .. } => ErrorCode::LimitExceeded,
            ControlError::NotFound { .. } => ErrorCode::NotFound,
            ControlError::NoDocument => ErrorCode::NoDocument,
            ControlError::Unauthorized => ErrorCode::Unauthorized,
            ControlError::NotReady => ErrorCode::NotReady,
            ControlError::UnsupportedVersion { .. } => ErrorCode::UnsupportedVersion,
            ControlError::Busy { .. } => ErrorCode::Busy,
            ControlError::Io { .. } | ControlError::Internal { .. } => ErrorCode::InternalError,
            ControlError::Remote { code, .. } => {
                ErrorCode::from_value(*code).unwrap_or(ErrorCode::InternalError)
            }
            ControlError::NotImplemented { .. } => ErrorCode::NotImplemented,
        }
    }
}

/// The code of a registry error (ADR 0077: one code per
/// [`CommandError`] variant).
fn command_code(error: &CommandError) -> ErrorCode {
    match error {
        CommandError::UnknownCommand { .. } => ErrorCode::UnknownCommand,
        CommandError::NotEnabled { .. } => ErrorCode::NotEnabled,
        CommandError::NeedsInput(_) => ErrorCode::NeedsInput,
        CommandError::InvalidArgs { .. } => ErrorCode::InvalidParams,
        CommandError::Unsupported { .. } => ErrorCode::Unsupported,
        // `Failed` and any variant added later are domain failures.
        _ => ErrorCode::CommandFailed,
    }
}

impl From<CommandError> for ControlError {
    fn from(error: CommandError) -> Self {
        ControlError::Command(error)
    }
}

impl From<std::io::Error> for ControlError {
    fn from(error: std::io::Error) -> Self {
        ControlError::Io {
            message: error.to_string(),
        }
    }
}

impl fmt::Display for ControlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ControlError::Parse { message } => write!(f, "parse error: {message}"),
            ControlError::InvalidRequest { message } => write!(f, "invalid request: {message}"),
            ControlError::MethodNotFound { method } => write!(f, "unknown method {method}"),
            ControlError::InvalidParams { message } => write!(f, "invalid params: {message}"),
            ControlError::Command(error) => write!(f, "{error}"),
            ControlError::Denied { reason } => write!(f, "denied: {reason}"),
            ControlError::LimitExceeded { limit, value, max } => {
                write!(f, "limit {limit} exceeded: {value} > {max}")
            }
            ControlError::NotFound { what, reference } => write!(f, "no {what} {reference:?}"),
            ControlError::NoDocument => f.write_str("no open document"),
            ControlError::Unauthorized => f.write_str("unauthorized: present the session token"),
            ControlError::NotReady => f.write_str("say hello first"),
            ControlError::UnsupportedVersion {
                requested,
                supported,
            } => write!(
                f,
                "unsupported protocol version {requested:?} (supported: {})",
                supported.join(", ")
            ),
            ControlError::Busy { reason } => write!(f, "busy: {reason}"),
            ControlError::Io { message } => write!(f, "i/o error: {message}"),
            ControlError::Internal { message } => write!(f, "internal error: {message}"),
            ControlError::Remote { message, .. } => f.write_str(message),
            ControlError::NotImplemented { what } => write!(f, "{what} is not implemented yet"),
        }
    }
}

impl std::error::Error for ControlError {}

/// The `data` member a [`ControlError`] carries on the wire, when it
/// has one (ADR 0075): the needed input of a `NeedsInput`, the supported
/// versions of an `UnsupportedVersion`, the limit of a `LimitExceeded`.
///
/// `None` for errors whose message says everything.
pub fn error_data(error: &ControlError) -> Option<serde_json::Value> {
    match error {
        ControlError::Command(CommandError::NeedsInput(needed)) => Some(needed_input_data(needed)),
        ControlError::UnsupportedVersion {
            requested,
            supported,
        } => Some(serde_json::json!({ "requested": requested, "supported": supported })),
        ControlError::LimitExceeded { limit, value, max } => {
            Some(serde_json::json!({ "limit": limit, "value": value, "max": max }))
        }
        ControlError::Remote { data, .. } => data.clone(),
        _ => None,
    }
}

/// The `data` of a `NeedsInput` error: which argument kind is missing
/// and its purpose or prompt (ADR 0077).
fn needed_input_data(needed: &NeededInput) -> serde_json::Value {
    match needed {
        NeededInput::Path { purpose, suggested } => serde_json::json!({
            "kind": "path",
            "purpose": purpose.to_string(),
            "suggested": suggested.as_ref().map(|path| path.display().to_string()),
        }),
        NeededInput::Text { prompt, default } => serde_json::json!({
            "kind": "text",
            "prompt": prompt,
            "default": default,
        }),
        NeededInput::Number { prompt, default } => serde_json::json!({
            "kind": "number",
            "prompt": prompt,
            "default": default,
        }),
        // A kind added to `NeededInput` later still says what it is.
        _ => serde_json::json!({ "kind": "other" }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use larrygram_commands::PathPurpose;

    #[test]
    fn every_code_has_a_distinct_value_and_round_trips() {
        let mut seen = std::collections::BTreeSet::new();
        for code in ErrorCode::ALL {
            assert!(seen.insert(code.value()), "{code:?} reuses a value");
            assert_eq!(ErrorCode::from_value(code.value()), Some(code));
        }
        assert_eq!(ErrorCode::from_value(0), None);
        // The Larrygram codes stay in the implementation-defined range
        // MCP leaves free (ADR 0080).
        for code in &ErrorCode::ALL[5..] {
            assert!((-32019..=-32000).contains(&code.value()), "{code:?}");
        }
    }

    #[test]
    fn every_error_maps_to_its_code_and_one_line() {
        let cases: Vec<(ControlError, ErrorCode)> = vec![
            (
                ControlError::Parse {
                    message: "eof".into(),
                },
                ErrorCode::ParseError,
            ),
            (
                ControlError::InvalidRequest {
                    message: "no method".into(),
                },
                ErrorCode::InvalidRequest,
            ),
            (
                ControlError::MethodNotFound {
                    method: "nope".into(),
                },
                ErrorCode::MethodNotFound,
            ),
            (
                ControlError::InvalidParams {
                    message: "x".into(),
                },
                ErrorCode::InvalidParams,
            ),
            (
                CommandError::UnknownCommand { id: "x.y".into() }.into(),
                ErrorCode::UnknownCommand,
            ),
            (
                CommandError::NotEnabled {
                    id: "x.y".into(),
                    reason: None,
                }
                .into(),
                ErrorCode::NotEnabled,
            ),
            (
                CommandError::NeedsInput(NeededInput::Text {
                    prompt: "name".into(),
                    default: None,
                })
                .into(),
                ErrorCode::NeedsInput,
            ),
            (
                CommandError::invalid_args("bad").into(),
                ErrorCode::InvalidParams,
            ),
            (
                CommandError::Unsupported { id: "x.y".into() }.into(),
                ErrorCode::Unsupported,
            ),
            (CommandError::failed("io").into(), ErrorCode::CommandFailed),
            (
                ControlError::Denied {
                    reason: "outside the roots".into(),
                },
                ErrorCode::Denied,
            ),
            (
                ControlError::LimitExceeded {
                    limit: "max_frame_bytes",
                    value: 9,
                    max: 8,
                },
                ErrorCode::LimitExceeded,
            ),
            (
                ControlError::NotFound {
                    what: "page",
                    reference: "3".into(),
                },
                ErrorCode::NotFound,
            ),
            (ControlError::NoDocument, ErrorCode::NoDocument),
            (ControlError::Unauthorized, ErrorCode::Unauthorized),
            (ControlError::NotReady, ErrorCode::NotReady),
            (
                ControlError::UnsupportedVersion {
                    requested: "9".into(),
                    supported: vec!["1.0".into()],
                },
                ErrorCode::UnsupportedVersion,
            ),
            (
                ControlError::Busy {
                    reason: "drag".into(),
                },
                ErrorCode::Busy,
            ),
            (
                std::io::Error::other("broken pipe").into(),
                ErrorCode::InternalError,
            ),
            (
                ControlError::Internal {
                    message: "x".into(),
                },
                ErrorCode::InternalError,
            ),
            (
                ControlError::not_implemented("a::b"),
                ErrorCode::NotImplemented,
            ),
            (
                ControlError::Remote {
                    code: -32007,
                    message: "no page \"9\"".into(),
                    data: None,
                },
                ErrorCode::NotFound,
            ),
            (
                ControlError::Remote {
                    code: 7,
                    message: "odd".into(),
                    data: None,
                },
                ErrorCode::InternalError,
            ),
        ];
        for (error, code) in cases {
            assert_eq!(error.code(), code, "{error:?}");
            let line = error.to_string();
            assert!(!line.is_empty() && !line.contains('\n'), "{line:?}");
        }
    }

    #[test]
    fn error_data_carries_what_the_caller_needs() {
        let needs_path: ControlError = CommandError::NeedsInput(NeededInput::Path {
            purpose: PathPurpose::SaveFile,
            suggested: None,
        })
        .into();
        let data = error_data(&needs_path).expect("data");
        assert_eq!(data["kind"], "path");
        assert_eq!(data["purpose"], "save file");

        let needs_number: ControlError = CommandError::NeedsInput(NeededInput::Number {
            prompt: "angle".into(),
            default: Some(0.0),
        })
        .into();
        assert_eq!(error_data(&needs_number).expect("data")["kind"], "number");

        let needs_text: ControlError = CommandError::NeedsInput(NeededInput::Text {
            prompt: "name".into(),
            default: None,
        })
        .into();
        assert_eq!(error_data(&needs_text).expect("data")["prompt"], "name");

        let version = ControlError::UnsupportedVersion {
            requested: "2.0".into(),
            supported: vec!["1.0".into()],
        };
        assert_eq!(error_data(&version).expect("data")["supported"][0], "1.0");

        let limit = ControlError::LimitExceeded {
            limit: "max_frame_bytes",
            value: 9,
            max: 8,
        };
        assert_eq!(error_data(&limit).expect("data")["max"], 8);

        let remote = ControlError::Remote {
            code: -32003,
            message: "needs a path".into(),
            data: Some(serde_json::json!({ "kind": "path" })),
        };
        assert_eq!(error_data(&remote).expect("data")["kind"], "path");

        assert_eq!(error_data(&ControlError::NoDocument), None);
    }
}
