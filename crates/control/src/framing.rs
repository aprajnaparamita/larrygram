//! Newline-delimited framing (ADR 0075).
//!
//! One JSON-RPC message per line, UTF-8, no embedded newlines: the MCP
//! stdio framing, reused unchanged on the local socket (MCP 2026-07-28
//! recommends it for byte-stream transports). A frame longer than
//! [`Limits::max_frame_bytes`] is refused before it is buffered whole:
//! the reader discards the rest of the line and reports
//! [`FrameError::TooLong`], and the stream stays usable.

use std::io::{BufRead, Write};

use crate::error::ControlError;
use crate::limits::Limits;

/// One read from a [`FrameReader`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Frame {
    /// One complete line, without its `\n` (and without a trailing `\r`).
    Line(Vec<u8>),
    /// The peer closed the stream cleanly at a line boundary.
    Eof,
}

/// A framing failure that leaves the stream usable (the caller answers
/// with an error and reads on).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum FrameError {
    /// The line exceeded the cap; its bytes were discarded up to the
    /// next newline.
    TooLong {
        /// The bytes discarded (at least the cap plus one).
        discarded: u64,
    },
}

/// Reads frames from a buffered byte stream with a size cap (task
/// P9-02).
#[derive(Debug)]
pub struct FrameReader<R> {
    reader: R,
    max_frame_bytes: usize,
}

impl<R: BufRead> FrameReader<R> {
    /// A reader capped at `limits.max_frame_bytes`.
    pub fn new(reader: R, limits: &Limits) -> Self {
        Self {
            reader,
            max_frame_bytes: limits.max_frame_bytes,
        }
    }

    /// The cap this reader enforces, in bytes.
    pub fn max_frame_bytes(&self) -> usize {
        self.max_frame_bytes
    }

    /// Reads the next frame (task P9-02).
    ///
    /// `Ok(Ok(Frame::Line))` for a line, `Ok(Ok(Frame::Eof))` at a clean
    /// end of stream, `Ok(Err(FrameError))` for an oversized line the
    /// stream survived, and `Err` for an I/O failure. Empty lines are
    /// skipped. A final line without a newline is still a frame.
    pub fn read_frame(&mut self) -> Result<Result<Frame, FrameError>, ControlError> {
        let _ = &mut self.reader;
        Err(ControlError::not_implemented(
            "framing::FrameReader::read_frame",
        ))
    }
}

/// Writes one frame: the bytes, then `\n`, then a flush (task P9-02).
///
/// Refuses bytes that contain a newline (`Internal`: a serializer bug,
/// never sent half-written).
pub fn write_frame(writer: &mut dyn Write, bytes: &[u8]) -> Result<(), ControlError> {
    let _ = (writer, bytes);
    Err(ControlError::not_implemented("framing::write_frame"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;

    #[test]
    fn the_stubs_report_not_implemented() {
        let limits = Limits::default();
        let mut reader = FrameReader::new(&b"{}\n"[..], &limits);
        assert_eq!(reader.max_frame_bytes(), limits.max_frame_bytes);
        let error = reader.read_frame().expect_err("stub");
        assert_eq!(error.code(), ErrorCode::NotImplemented);

        let mut out = Vec::new();
        let error = write_frame(&mut out, b"{}").expect_err("stub");
        assert_eq!(error.code(), ErrorCode::NotImplemented);
        assert!(out.is_empty());
    }

    #[test]
    fn frame_values_compare() {
        assert_eq!(Frame::Line(b"x".to_vec()), Frame::Line(b"x".to_vec()));
        assert_ne!(Frame::Eof, Frame::Line(Vec::new()));
        assert_eq!(
            FrameError::TooLong { discarded: 9 },
            FrameError::TooLong { discarded: 9 }
        );
    }
}
