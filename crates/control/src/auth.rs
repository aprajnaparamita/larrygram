//! The session token of a socket listener (ADR 0081; task P9-06).
//!
//! 32 bytes from the operating system's random source, written as 64
//! lowercase hex digits into the discovery file (readable by the user
//! only) and presented by the client in `hello`. The comparison takes
//! the same time whatever the input, and the token never appears in
//! `Debug` output or logs.

use crate::error::ControlError;

/// A 256-bit session token (ADR 0081).
#[derive(Clone, PartialEq, Eq)]
pub struct Token([u8; 32]);

impl Token {
    /// A token with these bytes (tests; production uses
    /// [`Token::generate`]).
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Token(bytes)
    }

    /// A fresh token from the operating system's random source (task
    /// P9-06). A random-source failure is `Internal`, never a weak
    /// token.
    pub fn generate() -> Result<Token, ControlError> {
        Err(ControlError::not_implemented("auth::Token::generate"))
    }

    /// The token as 64 lowercase hex digits (task P9-06).
    pub fn to_hex(&self) -> String {
        String::new()
    }

    /// Parses 64 hex digits (task P9-06); anything else is
    /// `InvalidParams`.
    pub fn from_hex(text: &str) -> Result<Token, ControlError> {
        let _ = text;
        Err(ControlError::not_implemented("auth::Token::from_hex"))
    }

    /// Whether `presented` (hex) is this token, in time independent of
    /// where the two differ (task P9-06).
    pub fn verify(&self, presented: &str) -> bool {
        let _ = (presented, &self.0);
        false
    }
}

impl std::fmt::Debug for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Token(redacted)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;

    #[test]
    fn the_stubs_report_not_implemented_and_debug_redacts() {
        let error = Token::generate().expect_err("stub");
        assert_eq!(error.code(), ErrorCode::NotImplemented);
        let error = Token::from_hex("00").expect_err("stub");
        assert_eq!(error.code(), ErrorCode::NotImplemented);
        let token = Token::from_bytes([0xab; 32]);
        assert_eq!(token.to_hex(), "");
        assert!(!token.verify("ab"), "the stub accepts nothing");
        assert_eq!(format!("{token:?}"), "Token(redacted)");
        assert_eq!(token.clone(), token);
    }
}
