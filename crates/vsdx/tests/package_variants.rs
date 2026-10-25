//! Hand-built package variants the wide corpus rejected (task P8-19).
//!
//! One regression test per reader-rejection cause, with a package built
//! here byte by byte (ADR 0017 hand-authored fixtures, never a corpus
//! file) and a round trip: read, write, re-read and re-write, with the
//! first write semantically equivalent to the source and the second
//! write byte-identical (the Phase 2 criterion, `tests/roundtrip.rs`).
//!
//! The causes:
//!
//! 1. a `Shape` whose unsigned `ID` the model's signed id cannot hold
//!    (Visio writes `ID='4294967295'` on a deleted sub-shape placeholder);
//! 2. a currency cached value whose literal carries its currency code
//!    (`V='0.0000USD' U='CY'`, [MS-VSDX] 2.2.11.2.2.2.4);
//! 3. a non-finite numeric literal (`V='NaN'`; Visio writes it);
//! 4. a page contents part that is zero bytes.
//!
//! Two causes stay errors, pinned here: a zip that is not a Visio package
//! at all (`MissingPart`) and a damaged container (`InvalidPackage`).
//!
//! The fixes also exposed two writer round-trip faults (`src/write/**`,
//! listed in the task's Result): a short `Connect` spelling was dropped
//! and rebuilt with the omitted attributes, and a `U` code the reader
//! kept preserved was written over. Both are pinned here too.

// The never-crash lints apply to non-test code only (clippy.toml).
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::io::Write as _;

use larrygram_model::{Cell, XmlNode, diff};
use larrygram_vsdx::{Error, Limits, Package, PartData, read_package, to_model, write_document};
use zip::CompressionMethod;
use zip::write::{SimpleFileOptions, ZipWriter};

const TYPES_XML: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/visio/document.xml\" ContentType=\"application/vnd.ms-visio.drawing.main+xml\"/><Override PartName=\"/visio/pages/pages.xml\" ContentType=\"application/vnd.ms-visio.pages+xml\"/><Override PartName=\"/visio/pages/page1.xml\" ContentType=\"application/vnd.ms-visio.page+xml\"/></Types>";

const ROOT_RELS_XML: &str = "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.microsoft.com/visio/2010/relationships/document\" Target=\"visio/document.xml\"/></Relationships>";

const DOCUMENT_XML: &str = "<VisioDocument xmlns=\"http://schemas.microsoft.com/office/visio/2012/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xml:space=\"preserve\"><DocumentSettings TopPage=\"0\" DefaultTextStyle=\"0\" DefaultLineStyle=\"0\" DefaultFillStyle=\"0\" DefaultGuideStyle=\"0\"/></VisioDocument>";

const DOCUMENT_RELS_XML: &str = "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.microsoft.com/visio/2010/relationships/pages\" Target=\"pages/pages.xml\"/></Relationships>";

/// The page list of a one-page drawing.
const PAGES_XML: &str = "<Pages xmlns=\"http://schemas.microsoft.com/office/visio/2012/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xml:space=\"preserve\"><Page ID=\"0\" NameU=\"Page-1\" Name=\"Page-1\"><Rel r:id=\"rId1\"/></Page></Pages>";

const PAGES_RELS_XML: &str = "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.microsoft.com/visio/2010/relationships/page\" Target=\"page1.xml\"/></Relationships>";

/// Builds zip bytes from entries, deflate-compressed. A large entry is
/// stored instead: the fixture must stay readable under the reader's
/// compression-ratio cap whatever it holds (the corpus files' own
/// producers deflate weakly enough to pass it).
fn build_zip(entries: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut writer = ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, data) in entries {
        let method = if data.len() > 100_000 {
            CompressionMethod::Stored
        } else {
            CompressionMethod::Deflated
        };
        let options = SimpleFileOptions::default().compression_method(method);
        writer.start_file(*name, options).expect("entry starts");
        writer.write_all(data).expect("entry writes");
    }
    writer.finish().expect("zip finishes").into_inner()
}

/// A one-page drawing package whose page contents part is `page_xml`.
fn drawing(page_xml: &str) -> Vec<u8> {
    build_zip(&[
        ("[Content_Types].xml", TYPES_XML.as_bytes().to_vec()),
        ("_rels/.rels", ROOT_RELS_XML.as_bytes().to_vec()),
        ("visio/document.xml", DOCUMENT_XML.as_bytes().to_vec()),
        (
            "visio/_rels/document.xml.rels",
            DOCUMENT_RELS_XML.as_bytes().to_vec(),
        ),
        ("visio/pages/pages.xml", PAGES_XML.as_bytes().to_vec()),
        (
            "visio/pages/_rels/pages.xml.rels",
            PAGES_RELS_XML.as_bytes().to_vec(),
        ),
        ("visio/pages/page1.xml", page_xml.as_bytes().to_vec()),
    ])
}

/// The first cell named `name` of the one shape of the page: a direct
/// cell or a row cell of any section.
fn shape_cell<'d>(document: &'d larrygram_model::Document, name: &str) -> Option<&'d Cell> {
    let page = document.pages.first()?;
    let shape = document.shape(*page.shapes.first()?)?;
    if let Some(cell) = shape
        .sheet
        .cells
        .iter()
        .find(|cell| cell.name.as_str() == name)
    {
        return Some(cell);
    }
    shape
        .sheet
        .sections
        .iter()
        .flat_map(|section| section.rows.iter())
        .flat_map(|row| row.cells.iter())
        .find(|cell| cell.name.as_str() == name)
}

/// Whether the preserved children carry a `Shape` element with this `ID`.
fn preserved_shape_id(children: &[XmlNode], id: &str) -> bool {
    children.iter().any(|element| {
        element.name.local == "Shape"
            && element
                .attributes
                .iter()
                .any(|a| a.name.local == "ID" && a.value == id)
    })
}

/// Whether the cell's preserved attributes carry `name` with exactly `value`.
fn preserved_attribute(cell: &Cell, name: &str, value: &str) -> bool {
    cell.preserved
        .attributes
        .iter()
        .any(|attribute| attribute.name.local == name && attribute.value == value)
}

/// Round-trips one package: read, write, re-read and re-write (the
/// Phase 2 criterion of `tests/roundtrip.rs`, as an assertion).
fn assert_round_trip(name: &str, bytes: &[u8]) {
    let source = read_package(bytes, &Limits::DEFAULT).expect("source package reads");
    let document = to_model(&source).expect("source maps");
    let first = write_document(&document, &source).expect("first write");
    let reparsed = read_package(&first, &Limits::DEFAULT).expect("written package reads");
    let remodeled = to_model(&reparsed).expect("written package maps");
    let second = write_document(&remodeled, &reparsed).expect("second write");
    assert_eq!(first, second, "{name}: the second write differs");

    let names = |package: &Package| {
        package
            .parts
            .iter()
            .map(|part| part.name.as_str().to_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        names(&source),
        names(&reparsed),
        "{name}: part lists differ"
    );
    assert_eq!(
        source.content_types, reparsed.content_types,
        "{name}: content types differ"
    );
    assert_eq!(
        source.relationships, reparsed.relationships,
        "{name}: relationships differ"
    );
    for (before, after) in source.parts.iter().zip(reparsed.parts.iter()) {
        assert_eq!(before.name, after.name, "{name}: part names differ");
        assert_eq!(
            before.content_type,
            after.content_type,
            "{name}: part {} has a different content type",
            before.name.as_str()
        );
        match (&before.data, &after.data) {
            (PartData::Xml(a), PartData::Xml(b)) => {
                let differences: Vec<String> = diff(a, b)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|d| d.to_string())
                    .collect();
                assert!(
                    differences.is_empty(),
                    "{name}: part {} differs:\n{}",
                    before.name.as_str(),
                    differences.join("\n")
                );
            }
            (PartData::Binary(a), PartData::Binary(b)) => {
                assert_eq!(a, b, "{name}: binary part {} differs", before.name.as_str());
            }
            _ => panic!("{name}: part {} changed kind", before.name.as_str()),
        }
    }
}

/// Cause 1: a `Shape` whose unsigned `ID` the model's id cannot hold is
/// kept verbatim (it is not modeled, and it is not lost).
#[test]
fn a_shape_id_the_model_cannot_hold_stays_verbatim() {
    let page = "<PageContents xmlns=\"http://schemas.microsoft.com/office/visio/2012/main\" xml:space=\"preserve\"><Shapes><Shape ID=\"1\" Type=\"Shape\"><Cell N=\"PinX\" V=\"2\"/></Shape><Shape Del=\"1\" MasterShape=\"7\" ID=\"4294967295\"/></Shapes></PageContents>";
    let bytes = drawing(page);
    let document = to_model(&read_package(&bytes, &Limits::DEFAULT).expect("reads")).expect("maps");
    assert_eq!(document.shapes.len(), 1, "the placeholder is not modeled");
    assert_eq!(document.shapes[0].visio_id, 1);
    let page = document.pages.first().expect("page");
    assert!(preserved_shape_id(&page.preserved.children, "4294967295"));
    assert_round_trip("shape id sentinel", &bytes);
}

/// A `Shape` without an `ID` attribute keeps the model's default id of
/// zero, as before.
#[test]
fn a_shape_without_an_id_keeps_the_default_id() {
    let page = "<PageContents xmlns=\"http://schemas.microsoft.com/office/visio/2012/main\" xml:space=\"preserve\"><Shapes><Shape Type=\"Shape\"/></Shapes></PageContents>";
    let bytes = drawing(page);
    let document = to_model(&read_package(&bytes, &Limits::DEFAULT).expect("reads")).expect("maps");
    assert_eq!(document.shapes.len(), 1);
    assert_eq!(document.shapes[0].visio_id, 0);
    assert_round_trip("id-less shape", &bytes);
}

/// Cause 1, nested: the placeholder of a group's own `Shapes` wrapper
/// stays with the group.
#[test]
fn a_nested_shape_id_the_model_cannot_hold_stays_verbatim() {
    let page = "<PageContents xmlns=\"http://schemas.microsoft.com/office/visio/2012/main\" xml:space=\"preserve\"><Shapes><Shape ID=\"1\" Type=\"Group\"><Shapes><Shape ID=\"2\" Type=\"Shape\"/><Shape Del=\"1\" MasterShape=\"7\" ID=\"4294967295\"/></Shapes></Shape></Shapes></PageContents>";
    let bytes = drawing(page);
    let document = to_model(&read_package(&bytes, &Limits::DEFAULT).expect("reads")).expect("maps");
    assert_eq!(document.shapes.len(), 2);
    let group = &document.shapes[0];
    assert_eq!(group.children.len(), 1);
    assert!(preserved_shape_id(&group.preserved.children, "4294967295"));
    assert_round_trip("nested shape id sentinel", &bytes);
}

/// Cause 2: a currency literal carries its currency code and is kept as
/// written, with the file's `U` spelling.
#[test]
fn a_currency_value_is_kept_as_written() {
    let page = "<PageContents xmlns=\"http://schemas.microsoft.com/office/visio/2012/main\" xml:space=\"preserve\"><Shapes><Shape ID=\"1\" Type=\"Shape\"><Section N=\"Property\"><Row N=\"ProductNumber\"><Cell N=\"Value\" V=\"0.0000USD\" U=\"CY\" F=\"CY(0,&quot;USD&quot;)\"/></Row></Section></Shape></Shapes></PageContents>";
    let bytes = drawing(page);
    let document = to_model(&read_package(&bytes, &Limits::DEFAULT).expect("reads")).expect("maps");
    let cell = shape_cell(&document, "Value").expect("the Value cell");
    assert_eq!(
        cell.value,
        Some(larrygram_model::CellValue::Str("0.0000USD".to_owned()))
    );
    assert_eq!(cell.formula.as_deref(), Some("CY(0,\"USD\")"));
    assert!(preserved_attribute(cell, "U", "CY"));
    assert_round_trip("currency value", &bytes);
}

/// Cause 3: a non-finite numeric literal leaves the value slot empty and
/// keeps the file's `V` and `U` spellings.
#[test]
fn a_non_finite_value_keeps_its_spelling() {
    let page = "<PageContents xmlns=\"http://schemas.microsoft.com/office/visio/2012/main\" xml:space=\"preserve\"><Shapes><Shape ID=\"1\" Type=\"Shape\"><Cell N=\"Height\" U=\"DL\" F=\"Inh\" V=\"NaN\"/></Shape></Shapes></PageContents>";
    let bytes = drawing(page);
    let document = to_model(&read_package(&bytes, &Limits::DEFAULT).expect("reads")).expect("maps");
    let cell = shape_cell(&document, "Height").expect("the Height cell");
    assert_eq!(cell.value, None);
    assert_eq!(cell.formula.as_deref(), Some("Inh"));
    assert!(preserved_attribute(cell, "V", "NaN"));
    assert!(preserved_attribute(cell, "U", "DL"));
    assert_round_trip("non-finite value", &bytes);
}

/// Cause 4: a page contents part of zero bytes is kept as its raw bytes
/// and the page degrades to no shapes.
#[test]
fn an_empty_page_contents_part_degrades() {
    let pages = "<Pages xmlns=\"http://schemas.microsoft.com/office/visio/2012/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xml:space=\"preserve\"><Page ID=\"0\" NameU=\"Page-1\" Name=\"Page-1\"><Rel r:id=\"rId1\"/></Page><Page ID=\"1\" NameU=\"Page-2\" Name=\"Page-2\"><Rel r:id=\"rId2\"/></Page></Pages>";
    let pages_rels = "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.microsoft.com/visio/2010/relationships/page\" Target=\"page1.xml\"/><Relationship Id=\"rId2\" Type=\"http://schemas.microsoft.com/visio/2010/relationships/page\" Target=\"page2.xml\"/></Relationships>";
    let page1 = "<PageContents xmlns=\"http://schemas.microsoft.com/office/visio/2012/main\" xml:space=\"preserve\"><Shapes><Shape ID=\"1\" Type=\"Shape\"/></Shapes></PageContents>";
    let bytes = build_zip(&[
        ("[Content_Types].xml", TYPES_XML.as_bytes().to_vec()),
        ("_rels/.rels", ROOT_RELS_XML.as_bytes().to_vec()),
        ("visio/document.xml", DOCUMENT_XML.as_bytes().to_vec()),
        (
            "visio/_rels/document.xml.rels",
            DOCUMENT_RELS_XML.as_bytes().to_vec(),
        ),
        ("visio/pages/pages.xml", pages.as_bytes().to_vec()),
        (
            "visio/pages/_rels/pages.xml.rels",
            pages_rels.as_bytes().to_vec(),
        ),
        ("visio/pages/page1.xml", page1.as_bytes().to_vec()),
        ("visio/pages/page2.xml", Vec::new()),
    ]);
    let document = to_model(&read_package(&bytes, &Limits::DEFAULT).expect("reads")).expect("maps");
    assert_eq!(document.pages.len(), 2);
    assert_eq!(document.pages[0].shapes.len(), 1);
    assert!(document.pages[1].shapes.is_empty());
    assert_round_trip("empty page contents", &bytes);
}

/// A `U` code the reader keeps preserved (a colour table index is a
/// `COLOR` value that is not a `#RGB` literal) is never written over.
#[test]
fn a_colour_index_cell_keeps_its_u_spelling() {
    let page = "<PageContents xmlns=\"http://schemas.microsoft.com/office/visio/2012/main\" xml:space=\"preserve\"><Shapes><Shape ID=\"1\" Type=\"Shape\"><Cell N=\"FillForegnd\" U=\"COLOR\" V=\"1\"/></Shape></Shapes></PageContents>";
    let bytes = drawing(page);
    let document = to_model(&read_package(&bytes, &Limits::DEFAULT).expect("reads")).expect("maps");
    let cell = shape_cell(&document, "FillForegnd").expect("the FillForegnd cell");
    assert_eq!(
        cell.value,
        Some(larrygram_model::CellValue::Str("1".to_owned()))
    );
    assert!(preserved_attribute(cell, "U", "COLOR"));
    assert_round_trip("colour index", &bytes);
}

/// A short `Connect` spelling (the omitted attributes mean the model's
/// defaults) keeps its element: it is not dropped and rebuilt with the
/// attributes Visio left out.
#[test]
fn a_short_connect_spelling_is_not_rebuilt() {
    let page = "<PageContents xmlns=\"http://schemas.microsoft.com/office/visio/2012/main\" xml:space=\"preserve\"><Shapes><Shape ID=\"1\" Type=\"Shape\"/><Shape ID=\"2\" Type=\"Shape\"/></Shapes><Connects><Connect FromSheet=\"1\" FromCell=\"BeginX\" ToSheet=\"2\"/></Connects></PageContents>";
    let bytes = drawing(page);
    let package = read_package(&bytes, &Limits::DEFAULT).expect("reads");
    let document = to_model(&package).expect("maps");
    let connect = document.pages[0].connects.first().expect("connect");
    assert_eq!(connect.from_part, 0);
    assert_eq!(connect.to_cell, "");
    assert_eq!(connect.to_part, 0);
    assert_round_trip("short connect", &bytes);
}

/// A part whose deflated form would pass the reader's zip-bomb cap is
/// still deflated; one that would not is stored, so the written package
/// always reads back (the writer round-trip fault of task P8-19).
#[test]
fn a_highly_compressible_part_still_round_trips() {
    let page = format!(
        "<PageContents xmlns=\"http://schemas.microsoft.com/office/visio/2012/main\" xml:space=\"preserve\"><Shapes><Shape ID=\"1\" Type=\"Shape\"><Text>{}</Text></Shape></Shapes></PageContents>",
        "x".repeat(400_000)
    );
    let bytes = drawing(&page);
    assert_round_trip("compressible part", &bytes);
}

/// A zip that is not a Visio package at all keeps its clean, specific
/// error (task P8-19 goal 4: it stays an error).
#[test]
fn a_zip_that_is_no_visio_package_stays_missing_part() {
    let types = TYPES_XML.replace(
        "<Default Extension=\"xml\"",
        "<Default Extension=\"wav\" ContentType=\"audio/wav\"/><Default Extension=\"xml\"",
    );
    let bytes = build_zip(&[
        ("[Content_Types].xml", types.as_bytes().to_vec()),
        ("config.xml", b"<config/>".to_vec()),
        ("sounds/horn.wav", b"RIFF0000WAVE".to_vec()),
    ]);
    match read_package(&bytes, &Limits::DEFAULT) {
        Ok(package) => match to_model(&package) {
            Err(Error::MissingPart(part)) => assert_eq!(part, "/visio/document.xml"),
            other => panic!("expected MissingPart, got {other:?}"),
        },
        other => panic!("the package itself reads: {other:?}"),
    }
}

/// A damaged container keeps its clean error (task P8-19 goal 4: the
/// central directory offset of a damaged zip is wrong and no reader can
/// open it).
#[test]
fn a_damaged_central_directory_stays_a_clean_error() {
    let mut bytes = drawing("<PageContents xmlns=\"urn:ms-visio\"/>");
    let eocd = bytes
        .windows(4)
        .rposition(|window| window == b"PK\x05\x06")
        .expect("end of central directory");
    let at = eocd + 16;
    let old = u32::from_le_bytes(
        bytes[at..at + 4]
            .try_into()
            .expect("four bytes of the offset"),
    );
    bytes[at..at + 4].copy_from_slice(&old.wrapping_add(1).to_le_bytes());
    match read_package(&bytes, &Limits::DEFAULT) {
        Err(Error::InvalidPackage(message)) => {
            assert!(message.contains("zip container"), "{message}");
        }
        other => panic!("expected InvalidPackage, got {other:?}"),
    }
}
