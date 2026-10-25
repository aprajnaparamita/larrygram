//! The caps every control-channel input is checked against (ADR 0081).
//!
//! Each cap is checked before the allocation or the work it guards, so a
//! hostile client cannot make the server allocate, render or loop
//! without bound. A hit cap is [`crate::ControlError::LimitExceeded`]
//! naming the field.

/// The control-channel limits (ADR 0081). [`Limits::default`] is the
/// production setting; tests shrink single fields.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Limits {
    /// Bytes in one incoming frame (one line), newline excluded.
    pub max_frame_bytes: usize,
    /// Nesting depth of one incoming JSON value.
    pub max_json_depth: usize,
    /// Bytes in one outgoing frame; a larger result is an error, not a
    /// partial write.
    pub max_response_bytes: usize,
    /// Arguments in one command invocation.
    pub max_args: usize,
    /// Bytes in one text, path or decoded bytes argument.
    pub max_arg_bytes: usize,
    /// Items in one list argument.
    pub max_list_items: usize,
    /// Items one inspection page returns when the caller gives no limit.
    pub default_page_items: usize,
    /// Items one inspection page may return.
    pub max_page_items: usize,
    /// Bytes of shape text in an inspection summary (longer text is cut
    /// at a character boundary and flagged).
    pub max_summary_text_bytes: usize,
    /// Pixels (width times height) of one rendered image.
    pub max_render_pixels: u64,
    /// Pixels on either side of one rendered image.
    pub max_render_side: u32,
    /// Requests per second one connection may sustain (token bucket rate).
    pub requests_per_second: u32,
    /// Requests one connection may send in a burst (token bucket size).
    pub request_burst: u32,
    /// Socket connections served at once (ADR 0081).
    pub max_connections: usize,
    /// Milliseconds a socket connection may take to say `hello`.
    pub hello_timeout_ms: u64,
    /// Milliseconds the attached app may take to answer one operation
    /// before the caller gets [`crate::ErrorCode::Busy`] (ADR 0076).
    pub host_reply_timeout_ms: u64,
}

impl Limits {
    /// The production limits (ADR 0081 table).
    pub const DEFAULT: Limits = Limits {
        max_frame_bytes: 4 * 1024 * 1024,
        max_json_depth: 64,
        max_response_bytes: 48 * 1024 * 1024,
        max_args: 64,
        max_arg_bytes: 1024 * 1024,
        max_list_items: 10_000,
        default_page_items: 100,
        max_page_items: 1_000,
        max_summary_text_bytes: 1_024,
        max_render_pixels: 4096 * 4096,
        max_render_side: 8_192,
        requests_per_second: 100,
        request_burst: 200,
        max_connections: 4,
        hello_timeout_ms: 10_000,
        host_reply_timeout_ms: 30_000,
    };
}

impl Default for Limits {
    fn default() -> Self {
        Limits::DEFAULT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_are_the_adr_table() {
        let limits = Limits::default();
        assert_eq!(limits, Limits::DEFAULT);
        assert_eq!(limits.max_frame_bytes, 4 * 1024 * 1024);
        assert_eq!(limits.max_render_pixels, 16_777_216);
        assert!(limits.default_page_items <= limits.max_page_items);
        assert!(limits.max_render_side <= larrygram_render::MAX_DEVICE_EXTENT);
        // A full-size inline PNG (base64 grows by a third) fits a frame.
        assert!(limits.max_response_bytes > limits.max_frame_bytes);
        assert!(limits.request_burst >= limits.requests_per_second);
    }
}
