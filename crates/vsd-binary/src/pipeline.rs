// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The import pipeline: bytes to [`Import`] (ADR 0070, ADR 0071).
//!
//! The pipeline runs the layers in order and never flattens
//! inheritance:
//!
//! 1. [`crate::container::read_streams`] opens the compound file.
//! 2. [`crate::chunks::version`] and [`crate::chunks::read_chunks`]
//!    flatten the `VisioDocument` stream into records.
//! 3. [`crate::style::collect_with`], [`crate::pages::collect_with`],
//!    [`crate::shape::collect_with`] and [`crate::text::collect_with`]
//!    map the records into one [`larrygram_model::Document`]: styles
//!    first (shapes reference them), then pages and masters (shapes
//!    reference masters), then shapes, then text (which joins to the
//!    shapes by stream order, P8-05).
//!
//! The document properties of the summary streams are mapped first of
//! all (they belong to the document, not to any record group). The
//! last step, [`crate::scale`], turns the stored units of a scaled
//! drawing into physical inches. Every
//! step records degradations in [`crate::ImportWarning`] values; only a
//! structural failure is an [`crate::Error`] (ADR 0071).
//!
//! The byte strings of the `VisioDocument` stream decode with the
//! stream's code page (P8-05). The summary streams name their own code
//! page but [`crate::container::read_summary_properties`] does not
//! expose it, so the importer uses the format's ANSI default (Windows
//! 1252) and the corpus's two spellings stay with the summary decoder;
//! see the task result's open issues.

use crate::Import;
use crate::chunks::{self, FormatVersion};
use crate::container;
use crate::error::Error;
use crate::limits::Limits;
use crate::pages;
use crate::shape;
use crate::style;
use crate::text::{self, CodePage, TextOptions};
use larrygram_model::Document;

/// Reads a whole binary `.vsd`, `.vss` or `.vst` file into the model.
///
/// The bytes are the whole file. `limits` caps every input-derived size
/// (ADR 0071). The result carries the document and the warnings
/// collected on the way; a file outside the alpha scope (ADR 0069) is
/// an error, never a panic.
pub fn import(bytes: &[u8], limits: &Limits) -> Result<Import, Error> {
    let streams = container::read_streams(bytes, limits)?;
    // `read_streams` guarantees the stream (a compound file without one
    // is `Error::NotAVisioFile` there); an empty slice degrades to the
    // structural error of `version`.
    let visio_document = streams.visio_document().unwrap_or_default();
    let version: FormatVersion = chunks::version(visio_document)?;
    let mut blocks = Vec::new();
    let (chunks, mut warnings) = chunks::read_chunks(visio_document, version, limits, &mut blocks)?;
    let mut document = Document::new();
    pages::collect_summary_properties(&streams, &mut document, &mut warnings);
    style::collect_with(&chunks, version, &mut document, &mut warnings)?;
    pages::collect_with(&chunks, version, &mut document, &mut warnings)?;
    shape::collect_with(&chunks, version, limits, &mut document, &mut warnings)?;
    text::collect_with(
        &chunks,
        &mut document,
        &mut warnings,
        &TextOptions {
            version,
            code_page: CodePage::default(),
            limits: limits.clone(),
        },
    )?;
    // A scaled drawing stores its lengths in the units it is scaled
    // in; the model keeps physical inches (P8-15 goal 4).
    crate::scale::scale_document(&mut document);
    Ok(Import { document, warnings })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_bytes_are_not_a_visio_file() {
        let err = import(&[], &Limits::DEFAULT).unwrap_err();
        assert!(matches!(err, Error::NotAVisioFile(_) | Error::Invalid(_)));
    }

    #[test]
    fn garbage_bytes_error_without_panicking() {
        let err = import(&[0u8; 64], &Limits::DEFAULT).unwrap_err();
        assert!(matches!(err, Error::NotAVisioFile(_) | Error::Invalid(_)));
    }
}
