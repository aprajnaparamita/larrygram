// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Import warnings: everything the importer drops or degrades (ADR 0071).
//!
//! A warning is not fatal: the import still returns a document. The
//! importer records one warning per degradation, with enough context to
//! find the place in the file (page, shape or record), never file
//! content.

use std::fmt;

/// Why an [`ImportWarning`] was recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum WarningKind {
    /// A record kind the importer does not read was skipped whole.
    SkippedRecord {
        /// The record's format name, e.g. `"OLE data"`.
        name: &'static str,
    },
    /// Part of a record was dropped, e.g. an embedded picture or a
    /// hyperlink.
    Dropped {
        /// What was dropped, e.g. `"embedded OLE object"`.
        what: &'static str,
    },
    /// A reference (master, style, list entry) resolved to nothing and
    /// was left unset.
    UnresolvedReference {
        /// What did not resolve, e.g. `"master 7"`.
        what: &'static str,
    },
    /// A value did not parse and a default was used instead.
    BadValue {
        /// What did not parse, e.g. `"fill colour"`.
        what: &'static str,
    },
    /// A file feature outside the alpha scope was ignored (ADR 0070).
    Unsupported {
        /// What is unsupported, e.g. `"version 5 chunk format"`.
        what: &'static str,
    },
}

impl fmt::Display for WarningKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WarningKind::SkippedRecord { name } => write!(f, "skipped record {name}"),
            WarningKind::Dropped { what } => write!(f, "dropped {what}"),
            WarningKind::UnresolvedReference { what } => write!(f, "unresolved {what}"),
            WarningKind::BadValue { what } => {
                write!(f, "unparsable {what}, using a default")
            }
            WarningKind::Unsupported { what } => write!(f, "unsupported {what}"),
        }
    }
}

/// One non-fatal problem found while importing (ADR 0071).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportWarning {
    /// Where the problem sits, best effort, e.g.
    /// `"page 2 / shape 5 / text"`.
    pub context: String,
    /// Why it was recorded.
    pub kind: WarningKind,
}

impl fmt::Display for ImportWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.context, self.kind)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_can_be_recorded() {
        let kinds = [
            (WarningKind::SkippedRecord { name: "OLE data" }, "skipped record OLE data"),
            (
                WarningKind::Dropped {
                    what: "embedded OLE object",
                },
                "dropped embedded OLE object",
            ),
            (
                WarningKind::UnresolvedReference { what: "master 7" },
                "unresolved master 7",
            ),
            (
                WarningKind::BadValue {
                    what: "fill colour",
                },
                "unparsable fill colour, using a default",
            ),
            (
                WarningKind::Unsupported {
                    what: "version 5 chunk format",
                },
                "unsupported version 5 chunk format",
            ),
        ];
        for (kind, spelled) in kinds {
            let warning = ImportWarning {
                context: "page 1 / shape 2".to_owned(),
                kind: kind.clone(),
            };
            assert_eq!(warning.kind, kind);
            assert_eq!(kind.to_string(), spelled, "Display spells the kind");
            assert_eq!(
                warning.to_string(),
                format!("page 1 / shape 2: {spelled}"),
                "Display names the context and the kind"
            );
        }
    }
}
