// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The corpus smoke test: every corpus `.vsd` imports without a panic
//! and yields the pages, masters, shapes and text its records carry
//! (task P8-07).
//!
//! The expectations are what the corpus files contain, checked against
//! the files' own name and property records; `dwg.vsd`'s master names
//! and ids are additionally the ones its `.vsdx` twin writes in
//! `visio/masters.xml`. The tests skip cleanly when `corpus/vsd/` is
//! absent and fail when `LARRYGRAM_REQUIRE_CORPUS=1` is set.

use std::fs;
use std::path::{Path, PathBuf};

use larrygram_vsd_binary::model::{
    CellValue, Document, DocumentKind, MasterId, Sheet, TextContent, TextPart,
};
use larrygram_vsd_binary::{Error, Limits, import};

/// The corpus directory with the real `.vsd` files.
fn corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("corpus")
        .join("vsd")
}

/// The `.vsd` files of the corpus, sorted for a stable order.
fn corpus_files() -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = match fs::read_dir(corpus_dir()) {
        Ok(entries) => entries
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "vsd"))
            .collect(),
        Err(_) => Vec::new(),
    };
    files.sort();
    files
}

/// Returns `false` (after a clean skip) when the corpus is absent.
fn require_corpus(files: &[PathBuf]) -> bool {
    if files.is_empty() {
        eprintln!("skipping: corpus/vsd is absent; run `cargo xtask corpus`");
        assert!(
            std::env::var_os("LARRYGRAM_REQUIRE_CORPUS").is_none(),
            "corpus/vsd is missing and LARRYGRAM_REQUIRE_CORPUS=1 is set"
        );
        return false;
    }
    true
}

/// The numeric value of the named cell of one sheet, or `None` when
/// the sheet has no such numeric cell.
fn cell(sheet: &Sheet, name: &str) -> Option<f64> {
    sheet
        .cells
        .iter()
        .find(|cell| cell.name.as_str() == name)
        .and_then(|cell| cell.value.as_ref())
        .and_then(|value| match value {
            CellValue::Number(value) => Some(*value),
            _ => None,
        })
}

/// Asserts that `value` is `expected` within a rounding error.
fn approx(value: Option<f64>, expected: f64) {
    assert!(
        value.is_some_and(|value| (value - expected).abs() < 1e-9),
        "{value:?} is not within a rounding error of {expected}"
    );
}

/// The document property with this name, or an empty string.
fn property(document: &Document, name: &str) -> String {
    document
        .properties
        .iter()
        .find(|property| property.name == name)
        .map(|property| property.value.clone())
        .unwrap_or_default()
}

/// The literal text of one shape's text, run markers dropped.
fn text_of(content: Option<&TextContent>) -> String {
    let Some(TextContent { parts, .. }) = content else {
        return String::new();
    };
    parts
        .iter()
        .filter_map(|part| match part {
            TextPart::Text(text) => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

/// Every corpus file imports into a drawing with pages, shapes and
/// text.
#[test]
fn every_corpus_file_opens_with_its_pages_shapes_and_text() {
    let files = corpus_files();
    if !require_corpus(&files) {
        return;
    }
    assert_eq!(files.len(), 5, "the corpus holds five .vsd files");
    let mut failures = Vec::new();
    for path in files {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let bytes = fs::read(&path).unwrap_or_default();
        let Ok(imported) = import(&bytes, &Limits::DEFAULT) else {
            failures.push(format!("{name}: import failed"));
            continue;
        };
        let document = &imported.document;
        assert_eq!(document.kind, DocumentKind::Drawing, "{name}");
        assert!(!document.pages.is_empty(), "{name}: no pages");
        for page in &document.pages {
            for shape in &page.shapes {
                assert!(
                    document.shape(*shape).is_some(),
                    "{name}: page {} names a shape outside the arena",
                    page.visio_id
                );
            }
        }
        match name.as_str() {
            "dwg.vsd" => assert_dwg(document),
            "fdo86729-ms1252.vsd" => assert_fdo(document, "mytitleéá", "vmiklos creator"),
            "fdo86729-utf8.vsd" => assert_fdo(document, "mytitleéáőű", "Miklos"),
            "no-bgcolor.vsd" => assert_no_bgcolor(document),
            "tdf76829-numeric-format.vsd" => assert_tdf(document),
            other => failures.push(format!("unexpected corpus file {other}")),
        }
    }
    assert!(failures.is_empty(), "{failures:?}");
}

/// `dwg.vsd`: one A4 page with four master instances, four masters and
/// the metadata the file was written to test.
fn assert_dwg(document: &Document) {
    assert_eq!(document.pages.len(), 1);
    let page = &document.pages[0];
    assert_eq!(page.name, "Page-1");
    assert_eq!(page.name_u, "Page-1");
    assert_eq!(page.visio_id, 0);
    assert_eq!(page.background, None);
    assert_eq!(page.shapes.len(), 4);
    approx(cell(&page.sheet, "PageWidth"), 11.692_913_385_826_772);
    approx(cell(&page.sheet, "PageHeight"), 8.267_716_535_433_07);
    approx(cell(&page.sheet, "ShdwOffsetX"), 0.118_110_236_220_472_4);
    approx(cell(&page.sheet, "ShdwOffsetY"), -0.118_110_236_220_472_4);
    approx(cell(&page.sheet, "PageScale"), 0.039_370_078_740_157_48);
    approx(cell(&page.sheet, "DrawingScale"), 0.039_370_078_740_157_48);
    // The names and ids the twin's `visio/masters.xml` writes.
    let names: Vec<&str> = document
        .masters
        .iter()
        .map(|master| master.name.as_str())
        .collect();
    assert_eq!(names, ["Square", "Right Triangle", "Octagon", "Decagon"]);
    let ids: Vec<i32> = document
        .masters
        .iter()
        .map(|master| master.visio_id)
        .collect();
    assert_eq!(ids, [2, 4, 5, 6]);
    for master in &document.masters {
        assert_eq!(master.name, master.name_u, "one name per element");
        assert_eq!(master.shapes.len(), 1, "{}", master.name);
    }
    // The page's shapes are instances of the four masters, in order.
    for (position, id) in page.shapes.iter().enumerate() {
        let master = document.shape(*id).map(|shape| shape.master);
        assert_eq!(
            master,
            Some(Some(MasterId {
                index: u32::try_from(position).unwrap_or(u32::MAX)
            })),
            "shape {position}"
        );
    }
    assert_eq!(property(document, "title"), "Title test");
    assert_eq!(property(document, "subject"), "Subject test");
    assert_eq!(property(document, "creator"), "Mihai Varga");
    assert_eq!(property(document, "keywords"), "Tag_test");
    assert_eq!(property(document, "description"), "Comments test");
    assert!(property(document, "Template").ends_with("BASICD_M.VSTX"));
    assert_eq!(property(document, "Application"), "Microsoft Visio");
    assert!(document.shapes.iter().any(|shape| shape.text.is_some()));
}

/// Both `fdo86729` spellings: one Letter page, three masters, one
/// shape with text, and the summary stream that differs between the
/// two files.
fn assert_fdo(document: &Document, title: &str, creator: &str) {
    assert_eq!(document.pages.len(), 1);
    let page = &document.pages[0];
    assert_eq!(page.name, "Page-1");
    assert_eq!(page.visio_id, 0);
    assert_eq!(page.shapes.len(), 3);
    approx(cell(&page.sheet, "PageWidth"), 11.0);
    approx(cell(&page.sheet, "PageHeight"), 8.5);
    let names: Vec<&str> = document
        .masters
        .iter()
        .map(|master| master.name.as_str())
        .collect();
    assert_eq!(names, ["Rectangle", "Circle", "Right Triangle"]);
    let ids: Vec<i32> = document
        .masters
        .iter()
        .map(|master| master.visio_id)
        .collect();
    assert_eq!(ids, [2, 4, 5]);
    let masters: Vec<Option<Option<MasterId>>> = page
        .shapes
        .iter()
        .map(|id| document.shape(*id).map(|shape| shape.master))
        .collect();
    assert_eq!(
        masters,
        [
            Some(Some(MasterId { index: 0 })),
            Some(Some(MasterId { index: 1 })),
            Some(Some(MasterId { index: 2 }))
        ]
    );
    assert_eq!(property(document, "title"), title);
    assert_eq!(property(document, "creator"), creator);
    assert!(
        text_of(document.shapes[3].text.as_ref()).contains("aaa"),
        "the rectangle's text"
    );
}

/// `no-bgcolor.vsd`: a page that keeps its Visio id after earlier pages
/// were deleted, and two German-named masters.
fn assert_no_bgcolor(document: &Document) {
    assert_eq!(document.pages.len(), 1);
    let page = &document.pages[0];
    assert_eq!(page.name, "1577");
    assert_eq!(page.visio_id, 4, "pages keep their id when ids are gone");
    assert_eq!(page.shapes.len(), 7);
    let names: Vec<&str> = document
        .masters
        .iter()
        .map(|master| master.name.as_str())
        .collect();
    assert_eq!(names, ["Component", "Dynamischer Verbinder"]);
    let ids: Vec<i32> = document
        .masters
        .iter()
        .map(|master| master.visio_id)
        .collect();
    assert_eq!(ids, [2, 4]);
    assert_eq!(document.masters[0].shapes.len(), 7);
    assert!(
        document.masters[1].shapes.is_empty(),
        "the shared subtree resolves once (the P8-03 rule), so the \
         second master keeps none of it"
    );
    assert!(
        document
            .shapes
            .iter()
            .any(|shape| text_of(shape.text.as_ref()).contains("My hovercraft is full of eels.")),
        "the shape text joins to its shape"
    );
    // The Windows timestamp of the summary stream, spelled like the
    // `.vsdx` reader reads it from `docProps`.
    assert_eq!(property(document, "modified"), "2016-11-29T15:37:31Z");
}

/// `tdf76829-numeric-format.vsd`: one page of text shapes with fields
/// and no masters at all.
fn assert_tdf(document: &Document) {
    assert_eq!(document.pages.len(), 1);
    let page = &document.pages[0];
    assert_eq!(page.name, "Page-1");
    assert_eq!(page.visio_id, 0);
    assert_eq!(page.shapes.len(), 11);
    assert!(document.masters.is_empty());
    approx(cell(&page.sheet, "PageWidth"), 11.692_913_385_826_772);
    approx(cell(&page.sheet, "PageHeight"), 8.267_716_535_433_07);
    let texts: Vec<String> = document
        .shapes
        .iter()
        .map(|shape| text_of(shape.text.as_ref()))
        .collect();
    assert!(
        texts[0].contains("Number of lines, generic format:"),
        "{:?}",
        texts[0]
    );
    assert!(
        texts.iter().any(|text| text.contains("TheDoc")),
        "the field display text: {texts:?}"
    );
    assert_eq!(property(document, "creator"), "Bartosz Kosiorek");
    assert!(property(document, "Template").ends_with("BASFLO_M.VST"));
}

/// Truncated and damaged corpus bytes degrade or error; they never
/// panic (ADR 0071).
#[test]
fn truncated_and_damaged_files_never_panic() {
    let files = corpus_files();
    if !require_corpus(&files) {
        return;
    }
    let bytes = fs::read(&files[0]).unwrap_or_default();
    assert!(!bytes.is_empty(), "the corpus file reads");
    for cut in [0, 1, 7, 64, 512, 4096, bytes.len() / 2, bytes.len() - 1] {
        let _ = import(&bytes[..cut], &Limits::DEFAULT);
    }
    for at in [0, 1, 512, 1024, 8192] {
        if let Some(byte) = bytes.get(at) {
            let mut damaged = bytes.clone();
            damaged[at] = byte.wrapping_add(1);
            let _ = import(&damaged, &Limits::DEFAULT);
        }
    }
}

/// The caps of [`Limits`] bound the import with an error, not a panic.
#[test]
fn the_caps_stop_the_import_with_an_error() {
    let files = corpus_files();
    if !require_corpus(&files) {
        return;
    }
    let bytes = fs::read(&files[0]).unwrap_or_default();
    let limits = Limits {
        max_shapes: 1,
        ..Limits::DEFAULT
    };
    let err = import(&bytes, &limits).unwrap_err();
    assert!(matches!(err, Error::LimitExceeded { .. }), "{err:?}");
    // The text cap is checked against the text a shape carries; the
    // numeric-format drawing has long texts.
    let bytes = fs::read(corpus_dir().join("tdf76829-numeric-format.vsd")).unwrap_or_default();
    let limits = Limits {
        max_text_bytes: 8,
        ..Limits::DEFAULT
    };
    let err = import(&bytes, &limits).unwrap_err();
    assert!(matches!(err, Error::LimitExceeded { .. }), "{err:?}");
}
