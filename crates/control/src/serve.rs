//! The serve loop: frames in, front end, frames out (ADR 0075).
//!
//! One loop serves one connection (stdio or one socket client) for
//! either front end. It reads a frame, parses it
//! ([`crate::jsonrpc::parse_message`]), admits it through the
//! connection's [`RateLimiter`], hands it to the [`FrontEnd`] and
//! writes the response, one request at a time. A malformed frame gets
//! an error response and the loop reads on; only end of stream, an I/O
//! failure or a front end that asks to close ends it (ADR 0081: hostile
//! input never ends the process).

use std::io::{BufRead, Write};
use std::time::Instant;

use crate::backend::Backend;
use crate::error::ControlError;
use crate::jsonrpc::{Message, Rejected, Response};
use crate::limits::Limits;

/// A protocol front end: turns one message into at most one response
/// (ADR 0080: the control protocol and MCP are two front ends over one
/// [`Backend`]).
pub trait FrontEnd {
    /// Handles one message. A request gets `Some(response)`; a
    /// notification gets `None`.
    fn handle(&mut self, backend: &mut dyn Backend, message: Message) -> Option<Response>;

    /// The response to a frame the parser refused. The default echoes
    /// the rejection's error.
    fn reject(&mut self, rejected: Rejected) -> Response {
        rejected.response()
    }

    /// Whether the serve loop should close the connection after writing
    /// the last response (for example after a failed authentication).
    fn close_after(&self) -> bool;
}

/// Why a serve loop ended normally.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ServeEnd {
    /// The peer closed the stream.
    Eof,
    /// The front end asked to close the connection.
    Closed,
}

/// Serves one connection until it ends (task P9-02).
///
/// `Ok` when the peer closes the stream or the front end closes it;
/// `Err` only for an I/O failure of the stream itself.
pub fn serve(
    reader: &mut dyn BufRead,
    writer: &mut dyn Write,
    front: &mut dyn FrontEnd,
    backend: &mut dyn Backend,
    limits: &Limits,
) -> Result<ServeEnd, ControlError> {
    let _ = (reader, writer, front, backend, limits);
    Err(ControlError::not_implemented("serve::serve"))
}

/// A token bucket over one connection's requests (ADR 0081; task
/// P9-02). It starts full; a request takes one token; tokens refill at
/// `Limits::requests_per_second` up to `Limits::request_burst`.
#[derive(Debug, Clone)]
pub struct RateLimiter {
    rate: u32,
    burst: u32,
    tokens: f64,
    last: Option<Instant>,
}

impl RateLimiter {
    /// A full bucket for `limits`.
    pub fn new(limits: &Limits) -> Self {
        Self {
            rate: limits.requests_per_second,
            burst: limits.request_burst,
            tokens: f64::from(limits.request_burst),
            last: None,
        }
    }

    /// The bucket's size.
    pub fn burst(&self) -> u32 {
        self.burst
    }

    /// The refill rate, tokens per second.
    pub fn rate(&self) -> u32 {
        self.rate
    }

    /// Takes one token at time `now` (task P9-02). `Ok` admits the
    /// request; `Err(LimitExceeded)` refuses it and the caller answers
    /// with that error.
    pub fn admit(&mut self, now: Instant) -> Result<(), ControlError> {
        let _ = (now, self.tokens, self.last);
        Err(ControlError::not_implemented("serve::RateLimiter::admit"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;
    use crate::jsonrpc::Id;
    use crate::ops::{Op, OpResult};

    struct NoBackend;

    impl Backend for NoBackend {
        fn call(&mut self, _op: Op) -> Result<OpResult, ControlError> {
            panic!("the stub serve loop must not call the backend")
        }
    }

    struct Echo;

    impl FrontEnd for Echo {
        fn handle(&mut self, _backend: &mut dyn Backend, _message: Message) -> Option<Response> {
            None
        }

        fn close_after(&self) -> bool {
            false
        }
    }

    #[test]
    fn the_stubs_report_not_implemented() {
        let limits = Limits::default();
        let mut input = &b"{}\n"[..];
        let mut output = Vec::new();
        let error =
            serve(&mut input, &mut output, &mut Echo, &mut NoBackend, &limits).expect_err("stub");
        assert_eq!(error.code(), ErrorCode::NotImplemented);
        assert!(output.is_empty());

        let mut limiter = RateLimiter::new(&limits);
        assert_eq!(limiter.burst(), limits.request_burst);
        assert_eq!(limiter.rate(), limits.requests_per_second);
        let error = limiter.admit(Instant::now()).expect_err("stub");
        assert_eq!(error.code(), ErrorCode::NotImplemented);
    }

    #[test]
    fn the_default_reject_echoes_the_error() {
        let response = Echo.reject(Rejected {
            id: Some(Id::Number(2)),
            error: ControlError::Parse {
                message: "eof".into(),
            },
        });
        assert_eq!(response.id, Some(Id::Number(2)));
        assert_eq!(response.outcome.expect_err("error").code, -32700);
        assert!(
            Echo.handle(
                &mut NoBackend,
                Message::Notification(crate::jsonrpc::Notification {
                    method: "x".into(),
                    params: None
                })
            )
            .is_none()
        );
        assert_ne!(ServeEnd::Eof, ServeEnd::Closed);
    }
}
