// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The `.vsdx` twin comparison (P8-15 goal 8).
//!
//! Each corpus `.vsd` has a `.vsdx` twin under
//! `LARRYGRAM_VSD_TWINS/vsdx/<name>.vsdx`, and the import of the one
//! must agree with the `.vsdx` reader's document for the other: pages,
//! shapes, the cells the renderer reads, and text, per page
//! ([`compare::compare_documents`]). The test skips cleanly when
//! `LARRYGRAM_VSD_TWINS` is unset or the directory is not there yet,
//! exactly like the libvisio oracle (ADR 0010).
//!
//! The `.vsdx` reader lives in `larrygram-vsdx`, a same-layer crate
//! this one may not depend on (ADR 0002; `cargo xtask layers` counts
//! dev-dependencies), so the corpus run is driven from the
//! `larrygram-render` suite where both readers are dependencies
//! (`crates/render/tests/corpus_vsd.rs`, `the_vsd_imports_match_their_
//! vsdx_twins`). This test owns the comparison and its contract; the
//! hand-built documents below exercise it without any file.

#[path = "twins/compare.rs"]
mod compare;

use std::path::{Path, PathBuf};

use larrygram_model::{
    Cell, CellName, CellValue, Document, Page, PageId, Shape, ShapeId, TextContent, TextPart,
};

/// The twins directory one environment value names: `None` when the
/// value is unset, when it names no directory or when the directory
/// has no `vsdx/` (the skip contract of the corpus run).
pub fn twins_dir(value: Option<std::ffi::OsString>) -> Option<PathBuf> {
    let dir = PathBuf::from(value?);
    (dir.is_dir() && dir.join("vsdx").is_dir()).then_some(dir)
}

/// The twin of one corpus file, `<twins>/vsdx/<name>.vsdx`.
pub fn twin_path(twins: &Path, name: &str) -> PathBuf {
    twins.join("vsdx").join(format!("{name}.vsdx"))
}

/// A number cell.
fn number(name: &'static str, value: f64) -> Cell {
    let mut cell = Cell::new(CellName::from_static(name));
    cell.value = Some(CellValue::Number(value));
    cell
}

/// A shape with the id, text and one transform.
fn shape(visio_id: i32, text: &str, pin_x: f64) -> Shape {
    let mut shape = Shape::new(ShapeId { index: 0 }, visio_id);
    shape.text = Some(TextContent {
        parts: vec![TextPart::CharRun { ix: 0 }, TextPart::Text(text.to_owned())],
    });
    shape.sheet.cells.push(number("PinX", pin_x));
    shape.sheet.cells.push(number("PinY", 1.0));
    shape.sheet.cells.push(number("Width", 2.0));
    shape
}

/// A document with one page named `Page-1` and two shapes.
fn document() -> Document {
    let mut document = Document::new();
    let mut page = Page::new(PageId { index: 0 }, 0);
    page.name = "Page-1".to_owned();
    page.sheet.cells.push(number("PageWidth", 11.0));
    page.sheet.cells.push(number("PageHeight", 8.5));
    page.shapes.push(ShapeId { index: 0 });
    page.shapes.push(ShapeId { index: 1 });
    document.pages.push(page);
    document.add_shape(shape(1, "one", 1.0));
    document.add_shape(shape(2, "two", 3.0));
    document
}

#[test]
fn the_twin_lookup_needs_the_directory_with_its_vsdx_folder() {
    assert_eq!(twins_dir(None), None);
    assert_eq!(twins_dir(Some("no/such/directory".into())), None);
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let dir = twins_dir(Some(root.as_os_str().to_owned()));
    assert!(
        dir.is_none(),
        "the repository root has no vsdx/ folder: {dir:?}"
    );
}

#[test]
fn equal_documents_compare_clean() {
    let mismatches = compare::compare_documents(&document(), &document());
    assert!(mismatches.is_empty(), "{mismatches:?}");
}

#[test]
fn a_different_page_size_is_reported() {
    let mut twin = document();
    twin.pages[0].sheet.cells = vec![number("PageWidth", 12.0), number("PageHeight", 8.5)];
    let mismatches = compare::compare_documents(&document(), &twin);
    assert_eq!(
        mismatches,
        ["page 1 / PageWidth / 12.0000 / 11.0000".to_owned()]
    );
}

#[test]
fn a_shape_only_one_side_has_is_reported_by_its_id() {
    let mut twin = document();
    twin.shapes.pop();
    twin.pages[0].shapes.pop();
    let mismatches = compare::compare_documents(&document(), &twin);
    assert_eq!(
        mismatches,
        ["page 1 / shape 2: only in the .vsd import".to_owned()]
    );
}

#[test]
fn a_different_cell_is_reported_with_both_values() {
    let mut twin = document();
    twin.shapes[1].sheet.cells[0] = number("PinX", 3.5);
    let mismatches = compare::compare_documents(&document(), &twin);
    assert_eq!(
        mismatches,
        ["page 1 / shape 2 / PinX / 3.5000 / 3.0000".to_owned()]
    );
}

#[test]
fn a_different_text_is_reported() {
    let mut twin = document();
    twin.shapes[0].text = Some(TextContent {
        parts: vec![TextPart::Text("uno".to_owned())],
    });
    let mismatches = compare::compare_documents(&document(), &twin);
    assert_eq!(
        mismatches,
        ["page 1 / shape 1 / text / \"uno\" / \"one\"".to_owned()]
    );
}

#[test]
fn trailing_whitespace_in_the_text_is_ignored() {
    let mut twin = document();
    twin.shapes[0].text = Some(TextContent {
        parts: vec![TextPart::Text("one  \n".to_owned())],
    });
    let mismatches = compare::compare_documents(&document(), &twin);
    assert!(mismatches.is_empty(), "{mismatches:?}");
}

#[test]
fn a_cleared_sheet_still_compares_through_the_defaults() {
    // The reader substitutes the documented defaults, which is what
    // the renderer draws, so a cleared sheet differs like any other.
    let mut twin = document();
    twin.shapes[0].sheet.cells.clear();
    let mismatches = compare::compare_documents(&document(), &twin);
    assert!(!mismatches.is_empty(), "the defaults differ from the cells");
}

#[test]
fn text_only_one_side_has_is_not_compared() {
    let mut twin = document();
    twin.shapes[0].text = None;
    let mismatches = compare::compare_documents(&document(), &twin);
    assert!(mismatches.is_empty(), "{mismatches:?}");
}

#[test]
fn a_length_within_the_tolerance_agrees() {
    let mut twin = document();
    twin.shapes[0].sheet.cells[0] = number("PinX", 1.0 + 0.0005);
    let mismatches = compare::compare_documents(&document(), &twin);
    assert!(mismatches.is_empty(), "{mismatches:?}");
}
