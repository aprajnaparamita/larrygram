//! Fuzz target for the binary `.vsd`/`.vss`/`.vst` importer (task
//! P8-10, ADR 0017 section 4).
//!
//! `import` is the entry point for untrusted bytes, so raw bytes go
//! straight into it with the default [`Limits`]. A panic fails the fuzz
//! run; an error or a document with warnings is a valid outcome. When
//! the bytes do read, the pages and shapes are walked once more through
//! the model lookup (cells and geometry), the paths a caller uses, so
//! those are fuzzed too.

#![no_main]

use larrygram_vsd_binary::model::{CellLookup, CellName};
use larrygram_vsd_binary::{Limits, import};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(imported) = import(data, &Limits::DEFAULT) {
        let document = &imported.document;
        for page in &document.pages {
            for cell in ["PageWidth", "PageHeight"] {
                let _ = document.resolve_page_cell(page.id, &CellName::from_static(cell));
            }
            for id in &page.shapes {
                let _ = document.resolve(*id, &CellName::from_static("FillPattern"));
                let _ = document.shape_geometry(*id);
            }
        }
    }
});
