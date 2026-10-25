//! JSON-RPC 2.0 messages (ADR 0075).
//!
//! Both front ends speak JSON-RPC 2.0, one message per frame
//! ([`crate::framing`]). The server receives requests and notifications
//! and sends responses; it never sends requests (as MCP 2026-07-28
//! requires of servers, ADR 0080). Batches (a JSON array) are refused
//! with `InvalidRequest`: neither front end needs them and MCP dropped
//! them.
//!
//! Parsing is bounded: the frame size is capped by the framing layer,
//! the JSON nesting depth by [`Limits::max_json_depth`], and nothing in a
//! malformed message can panic (ADR 0081).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{ControlError, error_data};
use crate::limits::Limits;

/// The JSON-RPC version string every message carries.
pub const JSONRPC_VERSION: &str = "2.0";

/// A request id: a number or a string (JSON-RPC 2.0; `null` ids are
/// refused for requests).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Id {
    /// A numeric id. Fractional ids are refused.
    Number(i64),
    /// A string id.
    Text(String),
}

/// A request: a method call that gets exactly one [`Response`].
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    /// The caller's id, echoed in the response.
    pub id: Id,
    /// The method name.
    pub method: String,
    /// The parameters (an object, or absent).
    pub params: Option<Value>,
}

/// A notification: a method call without an id, never answered.
#[derive(Debug, Clone, PartialEq)]
pub struct Notification {
    /// The method name.
    pub method: String,
    /// The parameters (an object, or absent).
    pub params: Option<Value>,
}

/// One incoming message.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Message {
    /// A request (has an id).
    Request(Request),
    /// A notification (no id).
    Notification(Notification),
}

/// The error member of a response (JSON-RPC 2.0).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RpcError {
    /// The code ([`crate::ErrorCode::value`]).
    pub code: i64,
    /// One line of text.
    pub message: String,
    /// Structured detail ([`crate::error::error_data`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl RpcError {
    /// The wire form of a [`ControlError`]: its code, its one-line
    /// message and its data (ADR 0075).
    pub fn from_control(error: &ControlError) -> RpcError {
        RpcError {
            code: error.code().value(),
            message: error.to_string(),
            data: error_data(error),
        }
    }

    /// The client-side value of a received error
    /// ([`ControlError::Remote`], unchanged).
    pub fn into_control(self) -> ControlError {
        ControlError::Remote {
            code: self.code,
            message: self.message,
            data: self.data,
        }
    }
}

/// A response: the result or the error of one request.
#[derive(Debug, Clone, PartialEq)]
pub struct Response {
    /// The request's id; `None` (`null` on the wire) only when the
    /// request was too broken to read its id.
    pub id: Option<Id>,
    /// The result value or the error.
    pub outcome: Result<Value, RpcError>,
}

impl Response {
    /// A success response.
    pub fn ok(id: Id, result: Value) -> Response {
        Response {
            id: Some(id),
            outcome: Ok(result),
        }
    }

    /// An error response for `error`.
    pub fn error(id: Option<Id>, error: &ControlError) -> Response {
        Response {
            id,
            outcome: Err(RpcError::from_control(error)),
        }
    }
}

/// A message the parser refused, with the id when it could be read, so
/// the error response can still be correlated (ADR 0075).
#[derive(Debug, Clone, PartialEq)]
pub struct Rejected {
    /// The id, when the message had a readable one.
    pub id: Option<Id>,
    /// Why the message was refused.
    pub error: ControlError,
}

impl Rejected {
    /// The error response to send back for this rejection.
    pub fn response(&self) -> Response {
        Response::error(self.id.clone(), &self.error)
    }
}

/// Parses one frame into a message (task P9-02).
///
/// Refuses, without panicking: bytes that are not UTF-8 JSON
/// (`ParseError`), a value nested deeper than
/// [`Limits::max_json_depth`] (`LimitExceeded`), a batch, a missing or
/// wrong `jsonrpc` member, a non-string method, a `null` or fractional
/// id, params that are neither an object nor absent, and a message that
/// is a response (`InvalidRequest`).
pub fn parse_message(bytes: &[u8], limits: &Limits) -> Result<Message, Rejected> {
    let _ = (bytes, limits);
    Err(Rejected {
        id: None,
        error: ControlError::not_implemented("jsonrpc::parse_message"),
    })
}

/// Encodes a response as one frame's bytes, without the newline (task
/// P9-02). A result larger than [`Limits::max_response_bytes`] becomes a
/// `LimitExceeded` error response for the same id instead.
pub fn encode_response(response: &Response, limits: &Limits) -> Result<Vec<u8>, ControlError> {
    let _ = (response, limits);
    Err(ControlError::not_implemented("jsonrpc::encode_response"))
}

/// Encodes a request as one frame's bytes, without the newline (the
/// client side, task P9-02).
pub fn encode_request(request: &Request) -> Result<Vec<u8>, ControlError> {
    let _ = request;
    Err(ControlError::not_implemented("jsonrpc::encode_request"))
}

/// Encodes a notification as one frame's bytes, without the newline
/// (task P9-02).
pub fn encode_notification(notification: &Notification) -> Result<Vec<u8>, ControlError> {
    let _ = notification;
    Err(ControlError::not_implemented(
        "jsonrpc::encode_notification",
    ))
}

/// Parses one frame as a response (the client side, task P9-02), with
/// the same bounds as [`parse_message`].
pub fn parse_response(bytes: &[u8], limits: &Limits) -> Result<Response, ControlError> {
    let _ = (bytes, limits);
    Err(ControlError::not_implemented("jsonrpc::parse_response"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;

    fn is_stub(error: &ControlError) -> bool {
        error.code() == ErrorCode::NotImplemented
    }

    #[test]
    fn the_stubs_report_not_implemented() {
        let limits = Limits::default();
        let rejected = parse_message(b"{}", &limits).expect_err("stub");
        assert!(rejected.id.is_none());
        assert!(is_stub(&rejected.error));
        let response = Response::ok(Id::Number(1), Value::Null);
        assert!(is_stub(
            &encode_response(&response, &limits).expect_err("stub")
        ));
        let request = Request {
            id: Id::Text("a".into()),
            method: "hello".into(),
            params: None,
        };
        assert!(is_stub(&encode_request(&request).expect_err("stub")));
        let notification = Notification {
            method: "notifications/cancelled".into(),
            params: None,
        };
        assert!(is_stub(
            &encode_notification(&notification).expect_err("stub")
        ));
        assert!(is_stub(&parse_response(b"{}", &limits).expect_err("stub")));
    }

    #[test]
    fn errors_cross_the_wire_unchanged() {
        let error = ControlError::NotFound {
            what: "shape",
            reference: "7".into(),
        };
        let wire = RpcError::from_control(&error);
        assert_eq!(wire.code, -32007);
        assert_eq!(wire.message, error.to_string());
        let back = wire.clone().into_control();
        assert_eq!(back.code(), ErrorCode::NotFound);
        assert_eq!(back.to_string(), error.to_string());

        let rejected = Rejected {
            id: Some(Id::Number(4)),
            error: ControlError::NotReady,
        };
        let response = rejected.response();
        assert_eq!(response.id, Some(Id::Number(4)));
        assert_eq!(response.outcome.expect_err("error").code, -32010);
    }

    #[test]
    fn ids_serialize_as_plain_json_values() {
        assert_eq!(serde_json::to_value(Id::Number(3)).unwrap(), 3);
        assert_eq!(serde_json::to_value(Id::Text("x".into())).unwrap(), "x");
        let id: Id = serde_json::from_value(serde_json::json!("y")).unwrap();
        assert_eq!(id, Id::Text("y".into()));
    }
}
