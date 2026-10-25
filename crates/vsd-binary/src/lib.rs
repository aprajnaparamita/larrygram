// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Read-only import of binary `.vsd`, `.vss` and `.vst` files into the
//! Larrygram model.
//!
//! This crate is the only place where code ported or derived from
//! libvisio may live. It is licensed MPL-2.0; see NOTICE at the
//! repository root. The Microsoft Open Specifications (MS-CFB) are the
//! source for the compound-file container; libvisio is the reference
//! for the Visio stream and record formats (AGENTS.md section 5).
//!
//! The one entry point is [`import`]: it takes the bytes of a whole
//! file and returns the [`model::Document`] plus the
//! [`ImportWarning`] values that record everything dropped or
//! degraded. The model is the one the `.vsdx` reader produces, so a
//! `.vsd` file saved as `.vsdx` goes through the existing writer
//! unchanged (ADR 0070).
//!
//! Layers (ADR 0068, ADR 0069): the `container` module reads the OLE
//! compound file and its streams, `chunks` flattens the
//! `VisioDocument` stream into records, and the domain modules
//! `style`, `pages`, `shape` and `text` map the records into the model
//! under `pipeline`.
//!
//! The reader entry points of every layer are re-exported here (the
//! container and chunk readers, the four domain collectors and the
//! text options) so a test harness or a later phase can drive one layer
//! on its own; [`import`] drives all of them.
//!
//! # Never crash
//!
//! Malformed input produces an [`Error`] or a warning, never a panic
//! (AGENTS.md section 6). [`Limits`] caps every input-derived size.

#![forbid(unsafe_code)]
#![deny(clippy::indexing_slicing)]

mod chunks;
mod container;
mod error;
mod limits;
mod pages;
mod pipeline;
mod scale;
mod shape;
mod style;
mod text;
mod warning;

pub use chunks::{Chunk, FormatVersion, Record, VERSION_OFFSET, read_chunks, version};
pub use container::{
    DOCUMENT_SUMMARY_INFORMATION, SUMMARY_INFORMATION, Stream, Streams, SummaryProperty,
    SummaryValue, VISIO_DOCUMENT, decompress, read_streams, read_summary_properties,
};
pub use error::Error;
pub use limits::Limits;
pub use pages::{
    collect as collect_pages, collect_summary_properties, collect_with as collect_pages_with,
};
pub use shape::{collect as collect_shapes, collect_with as collect_shapes_with};
pub use style::{collect as collect_styles, collect_with as collect_styles_with};
pub use text::{CodePage, TextOptions, collect as collect_text, collect_with as collect_text_with};
pub use warning::{ImportWarning, WarningKind};

pub use larrygram_model as model;

/// The result of one import: the document and the warnings collected
/// on the way (ADR 0071).
#[derive(Debug, Clone, PartialEq)]
pub struct Import {
    /// The imported document. Master and stylesheet inheritance is
    /// never flattened (ADR 0013).
    pub document: model::Document,
    /// Everything dropped or degraded while reading the file.
    pub warnings: Vec<ImportWarning>,
}

/// Reads a whole binary `.vsd`, `.vss` or `.vst` file into the model.
///
/// `bytes` are the whole file; `limits` caps every input-derived size
/// (ADR 0071). The document kind is derived from the file's records,
/// not from any file name (ADR 0070).
///
/// This is the public entry point of the crate; it delegates to the
/// import pipeline (ADR 0070).
pub fn import(bytes: &[u8], limits: &Limits) -> Result<Import, Error> {
    pipeline::import(bytes, limits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_import_pairs_a_document_with_its_warnings() {
        let import = Import {
            document: model::Document::new(),
            warnings: vec![ImportWarning {
                context: "page 1".to_owned(),
                kind: WarningKind::SkippedRecord { name: "OLE data" },
            }],
        };
        assert_eq!(import.document.pages, Vec::new());
        assert_eq!(import.warnings.len(), 1);
    }

    #[test]
    fn the_reexports_name_the_layers() {
        assert_eq!(VISIO_DOCUMENT, "VisioDocument");
        assert_eq!(SUMMARY_INFORMATION, "\u{5}SummaryInformation");
        assert_eq!(
            DOCUMENT_SUMMARY_INFORMATION,
            "\u{5}DocumentSummaryInformation"
        );
        assert_eq!(VERSION_OFFSET, 0x1a);
    }

    #[test]
    fn empty_bytes_are_an_error_not_a_panic() {
        let err = import(&[], &Limits::DEFAULT).unwrap_err();
        assert!(matches!(err, Error::NotAVisioFile(_) | Error::Invalid(_)));
    }
}
