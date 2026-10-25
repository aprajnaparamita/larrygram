//! The control protocol: the [`crate::ops`] operations as JSON-RPC
//! methods (ADR 0075).
//!
//! A request's method is [`Op::method`] and its params are the op's
//! params struct; the result is the matching [`OpResult`] struct. On a
//! socket the first request must be [`HELLO`] with the session token and
//! the versions the client speaks; on stdio `hello` is optional (the
//! parent process is trusted, ADR 0081) and version [`PROTOCOL_VERSION`]
//! applies. Notifications are accepted and ignored
//! (`notifications/cancelled` included: requests run one at a time and
//! are short, ADR 0075).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::auth::Token;
use crate::backend::Backend;
use crate::error::ControlError;
use crate::jsonrpc::{Id, Message, Request, Response};
use crate::ops::{Op, OpResult, ServerInfo};
use crate::serve::FrontEnd;

/// The control-protocol version this build speaks (ADR 0075:
/// `major.minor`; a client and server agree when the majors match).
pub const PROTOCOL_VERSION: &str = "1.0";

/// Every version this build speaks, newest first.
pub const SUPPORTED_VERSIONS: &[&str] = &[PROTOCOL_VERSION];

/// The handshake method.
pub const HELLO: &str = "hello";

/// Params of [`HELLO`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Hello {
    /// The versions the client speaks, newest first.
    pub versions: Vec<String>,
    /// The session token from the discovery file (sockets only,
    /// ADR 0081).
    pub token: Option<String>,
    /// A name for the client, shown in the app's status bar.
    pub client: Option<String>,
}

/// Result of [`HELLO`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelloResult {
    /// The version both sides now speak.
    pub version: String,
    /// The server's identity, capabilities and limits.
    pub server: ServerInfo,
}

/// Picks the version to speak: the newest offered version whose major
/// matches a supported one (task P9-02). An empty offer means
/// [`PROTOCOL_VERSION`]; no match is `UnsupportedVersion`.
pub fn negotiate(offered: &[String]) -> Result<String, ControlError> {
    let _ = offered;
    Err(ControlError::not_implemented("protocol::negotiate"))
}

/// Reads an operation out of a request (the server side, task P9-02):
/// the method picks the op, the params must fit its struct
/// (`MethodNotFound`, `InvalidParams` otherwise).
pub fn decode_op(request: &Request) -> Result<Op, ControlError> {
    let _ = request;
    Err(ControlError::not_implemented("protocol::decode_op"))
}

/// Writes an operation as a request (the client side, task P9-02).
pub fn encode_op(id: Id, op: &Op) -> Result<Request, ControlError> {
    let _ = (id, op);
    Err(ControlError::not_implemented("protocol::encode_op"))
}

/// Writes a result as the response's `result` value (task P9-02).
pub fn encode_result(result: &OpResult) -> Result<Value, ControlError> {
    let _ = result;
    Err(ControlError::not_implemented("protocol::encode_result"))
}

/// Reads a response's `result` value as the result of `op` (the client
/// side, task P9-02).
pub fn decode_result(op: &Op, value: Value) -> Result<OpResult, ControlError> {
    let _ = (op, value);
    Err(ControlError::not_implemented("protocol::decode_result"))
}

/// The control-protocol front end of one connection (task P9-02).
///
/// It tracks the handshake: with a token, every request before a
/// successful [`HELLO`] gets `NotReady` (or `Unauthorized` for a wrong
/// token, after which the connection is closed by the serve loop);
/// without a token, `hello` is optional.
#[derive(Debug)]
pub struct ControlServer {
    info: ServerInfo,
    token: Option<Token>,
    version: Option<String>,
}

impl ControlServer {
    /// A front end announcing `info`; `token` is `Some` on a socket.
    pub fn new(info: ServerInfo, token: Option<Token>) -> Self {
        Self {
            info,
            token,
            version: None,
        }
    }

    /// The server identity this front end announces.
    pub fn info(&self) -> &ServerInfo {
        &self.info
    }

    /// Whether requests are accepted now: after a successful `hello`, or
    /// at once when no token is required.
    pub fn ready(&self) -> bool {
        self.token.is_none() || self.version.is_some()
    }

    /// The negotiated version, once `hello` succeeded.
    pub fn version(&self) -> Option<&str> {
        self.version.as_deref()
    }
}

impl FrontEnd for ControlServer {
    fn handle(&mut self, backend: &mut dyn Backend, message: Message) -> Option<Response> {
        let _ = backend;
        match message {
            Message::Request(request) => Some(Response::error(
                Some(request.id),
                &ControlError::not_implemented("protocol::ControlServer::handle"),
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
    use crate::jsonrpc::Notification;
    use crate::ops::Op;

    struct NoBackend;

    impl Backend for NoBackend {
        fn call(&mut self, _op: Op) -> Result<OpResult, ControlError> {
            panic!("the stub front end must not call the backend")
        }
    }

    fn is_stub(error: &ControlError) -> bool {
        error.code() == ErrorCode::NotImplemented
    }

    #[test]
    fn the_stubs_report_not_implemented() {
        assert!(is_stub(&negotiate(&[]).expect_err("stub")));
        let request = Request {
            id: Id::Number(1),
            method: "documents/list".into(),
            params: None,
        };
        assert!(is_stub(&decode_op(&request).expect_err("stub")));
        assert!(is_stub(
            &encode_op(Id::Number(1), &Op::ListDocuments).expect_err("stub")
        ));
        let result = OpResult::Discover(ServerInfo::default());
        assert!(is_stub(&encode_result(&result).expect_err("stub")));
        assert!(is_stub(
            &decode_result(&Op::Discover, Value::Null).expect_err("stub")
        ));
    }

    #[test]
    fn the_front_end_stub_answers_requests_and_drops_notifications() {
        let mut server = ControlServer::new(ServerInfo::default(), None);
        assert!(server.ready());
        assert_eq!(server.version(), None);
        assert_eq!(server.info(), &ServerInfo::default());
        assert!(!server.close_after());
        let response = server
            .handle(
                &mut NoBackend,
                Message::Request(Request {
                    id: Id::Number(7),
                    method: HELLO.into(),
                    params: None,
                }),
            )
            .expect("a request gets a response");
        assert_eq!(response.id, Some(Id::Number(7)));
        assert_eq!(response.outcome.expect_err("stub").code, -32013);
        let dropped = server.handle(
            &mut NoBackend,
            Message::Notification(Notification {
                method: "notifications/cancelled".into(),
                params: None,
            }),
        );
        assert!(dropped.is_none());

        let token = Token::from_bytes([7; 32]);
        let guarded = ControlServer::new(ServerInfo::default(), Some(token));
        assert!(!guarded.ready(), "a socket connection waits for hello");
    }

    #[test]
    fn hello_params_refuse_unknown_fields() {
        let hello: Hello = serde_json::from_value(serde_json::json!({
            "versions": ["1.0"], "token": "ab"
        }))
        .unwrap();
        assert_eq!(hello.versions, ["1.0"]);
        assert!(serde_json::from_value::<Hello>(serde_json::json!({ "tokn": "ab" })).is_err());
        assert_eq!(SUPPORTED_VERSIONS.first(), Some(&PROTOCOL_VERSION));
    }
}
