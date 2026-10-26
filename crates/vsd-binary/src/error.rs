// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Errors of the binary Visio import (ADR 0071).
//!
//! The crate never panics on input (AGENTS.md section 6): malformed
//! input produces an error or an [`crate::ImportWarning`], never a
//! crash. Messages name the stream or record context and never carry
//! file content.

use std::fmt;

/// An import error.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// The file version is out of scope for the alpha (ADR 0069):
    /// versions 6 and 11 are read, versions 1 to 5 are not.
    UnsupportedVersion(u8),
    /// The bytes are not a compound file with a `VisioDocument` stream,
    /// or the stream does not start with the Visio magic.
    NotAVisioFile(String),
    /// A container, stream or record is malformed. The message names
    /// the place, never the content.
    Invalid(String),
    /// An input exceeded one of the [`crate::Limits`] caps.
    LimitExceeded {
        /// The name of the cap, e.g. `"max_stream_bytes"`.
        limit: &'static str,
        /// The offending value.
        value: u64,
        /// The cap.
        max: u64,
    },
    /// A [`larrygram_model`] operation failed while mapping.
    Model(larrygram_model::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::UnsupportedVersion(v) => {
                write!(f, "unsupported file version {v} (alpha reads 6 and 11)")
            }
            Error::NotAVisioFile(why) => write!(f, "not a binary Visio file: {why}"),
            Error::Invalid(why) => write!(f, "invalid file: {why}"),
            Error::LimitExceeded { limit, value, max } => {
                write!(f, "limit `{limit}` exceeded: {value} > {max}")
            }
            Error::Model(e) => write!(f, "model error: {e}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<larrygram_model::Error> for Error {
    fn from(e: larrygram_model::Error) -> Self {
        Error::Model(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_covers_every_variant() {
        assert_eq!(
            Error::UnsupportedVersion(3).to_string(),
            "unsupported file version 3 (alpha reads 6 and 11)"
        );
        assert_eq!(
            Error::NotAVisioFile("no VisioDocument stream".to_owned()).to_string(),
            "not a binary Visio file: no VisioDocument stream"
        );
        assert_eq!(
            Error::Invalid("truncated chunk header".to_owned()).to_string(),
            "invalid file: truncated chunk header"
        );
        assert_eq!(
            Error::LimitExceeded {
                limit: "max_chunks",
                value: 9,
                max: 8
            }
            .to_string(),
            "limit `max_chunks` exceeded: 9 > 8"
        );
        assert_eq!(
            Error::Model(larrygram_model::Error::UnknownPage(
                larrygram_model::PageId { index: 3 }
            ))
            .to_string(),
            "model error: unknown page id 3"
        );
    }

    #[test]
    fn model_errors_convert() {
        let converted = Error::from(larrygram_model::Error::UnknownPage(
            larrygram_model::PageId { index: 3 },
        ));
        assert_eq!(
            converted,
            Error::Model(larrygram_model::Error::UnknownPage(
                larrygram_model::PageId { index: 3 }
            ))
        );
    }
}
