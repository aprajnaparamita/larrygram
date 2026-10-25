//! Hand-authored fixture builder for `larrygram-vsdx` integration tests.
//!
//! Task P2-08. [`build`] generates every fixture byte by byte: a plain zip
//! archive around a minimal, hand-written Visio part set. The XML is our own
//! work (recorded in `corpus/PROVENANCE.md`); the element and attribute names,
//! content types and relationship types are structural facts from [MS-VSDX]
//! and the `docs/vsdx-inventory.md` part table.
//!
//! Determinism matters: the committed files under `corpus/fixtures/` must
//! equal the builder output byte for byte, so every archive entry is written
//! `Stored` (except the zip-bomb fixture, which needs real deflate) with a
//! fixed, clock-free timestamp.
//!
//! Later test crates (P2-09 onwards) include this module with `mod common;`
//! and call [`build`] instead of checking in blobs.

#![allow(dead_code)] // each test binary uses a different subset of the helpers

use std::path::{Path, PathBuf};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime, ZipWriter};

/// What the builder produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fixture {
    /// A minimal `.vsdx` drawing: one page, one rectangle shape.
    Drawing,
    /// A minimal `.vssx` stencil: one master, an empty `pages.xml` and no
    /// page contents (ADR 0016).
    Stencil,
    /// A minimal `.vstx` template: one page, one rectangle shape, one master
    /// (ADR 0016).
    Template,
    /// A broken package, one per failure mode (ADR 0017 section 3).
    Malformed(Malformed),
    /// One rendering feature of KICKOFF's Phase 3 list (ADR 0027 section
    /// 3, task P3-10).
    Feature(Feature),
}

/// One rendering feature fixture: one minimal file per feature of
/// KICKOFF's Phase 3 rendering list (ADR 0027 section 3).
///
/// Each file is a well-formed drawing package whose page exercises one
/// feature end to end (the `.vsdx` reader, the scene builder and both
/// render backends). The XML is our own work; every cell name is a
/// ShapeSheet structural fact (recorded in `docs/vsdx-inventory.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Feature {
    /// Every absolute geometry row type, one subpath each.
    GeometryAbsolute,
    /// Every relative geometry row type (`Rel*`), one subpath each.
    GeometryRelative,
    /// Solid, patterned and gradient fills.
    Fills,
    /// Line weight, pattern, line ends and rounding.
    Lines,
    /// Drop shadows.
    Shadows,
    /// Text with runs, paragraphs, bullets and tabs.
    Text,
    /// A master instance that inherits its geometry.
    MasterInheritance,
    /// Theme colours and theme fonts (`Themed` cells and a theme part).
    Theme,
    /// Layer membership and layer visibility.
    Layers,
    /// A non-default page size.
    PageSize,
    /// A background page behind the drawing page.
    BackgroundPages,
}

/// The malformed-input failure modes the builder can produce (ADR 0017
/// section 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Malformed {
    /// The zip archive is cut in half.
    TruncatedZip,
    /// The central directory signature is destroyed.
    BadCentralDirectory,
    /// `[Content_Types].xml` is missing.
    NoContentTypes,
    /// A relationship targets a part that does not exist.
    DanglingRelationship,
    /// `/visio/document.xml` (the main part) is missing.
    MissingMainPart,
    /// An entry declares an uncompressed size far beyond
    /// `Limits::DEFAULT.max_part_uncompressed`.
    HugeDeclaredSize,
    /// One XML part nests elements deeper than
    /// `Limits::DEFAULT.max_xml_depth`.
    DeepNesting,
    /// `[Content_Types].xml` is not well-formed XML.
    BadContentTypes,
    /// One entry compresses 8 MiB of zeros, far beyond
    /// `Limits::DEFAULT.max_compression_ratio`.
    ZipBomb,
    /// The archive holds more entries than `Limits::DEFAULT.max_entries`.
    ManyEntries,
}

/// The fixtures committed under `corpus/fixtures/`, in the order the
/// regenerate test writes them. The zip-bomb and many-entries fixtures are
/// generated at test time instead (see [`path`]).
pub const COMMITTED_FIXTURES: &[Fixture] = &[
    Fixture::Drawing,
    Fixture::Stencil,
    Fixture::Template,
    Fixture::Malformed(Malformed::TruncatedZip),
    Fixture::Malformed(Malformed::BadCentralDirectory),
    Fixture::Malformed(Malformed::NoContentTypes),
    Fixture::Malformed(Malformed::DanglingRelationship),
    Fixture::Malformed(Malformed::MissingMainPart),
    Fixture::Malformed(Malformed::HugeDeclaredSize),
    Fixture::Malformed(Malformed::DeepNesting),
    Fixture::Malformed(Malformed::BadContentTypes),
    Fixture::Feature(Feature::GeometryAbsolute),
    Fixture::Feature(Feature::GeometryRelative),
    Fixture::Feature(Feature::Fills),
    Fixture::Feature(Feature::Lines),
    Fixture::Feature(Feature::Shadows),
    Fixture::Feature(Feature::Text),
    Fixture::Feature(Feature::MasterInheritance),
    Fixture::Feature(Feature::Theme),
    Fixture::Feature(Feature::Layers),
    Fixture::Feature(Feature::PageSize),
    Fixture::Feature(Feature::BackgroundPages),
];

const XML_DECL: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>";
const XMLNS_VISIO: &str = "http://schemas.microsoft.com/office/visio/2012/main";
const XMLNS_R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const XMLNS_CT: &str = "http://schemas.openxmlformats.org/package/2006/content-types";
const XMLNS_RELS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const CT_RELS: &str = "application/vnd.openxmlformats-package.relationships+xml";
const CT_XML: &str = "application/xml";
const CT_DRAWING_MAIN: &str = "application/vnd.ms-visio.drawing.main+xml";
const CT_STENCIL_MAIN: &str = "application/vnd.ms-visio.stencil.main+xml";
const CT_TEMPLATE_MAIN: &str = "application/vnd.ms-visio.template.main+xml";
const CT_PAGES: &str = "application/vnd.ms-visio.pages+xml";
const CT_PAGE: &str = "application/vnd.ms-visio.page+xml";
const CT_MASTERS: &str = "application/vnd.ms-visio.masters+xml";
const CT_MASTER: &str = "application/vnd.ms-visio.master+xml";
const CT_WINDOWS: &str = "application/vnd.ms-visio.windows+xml";
const REL_DOCUMENT: &str = "http://schemas.microsoft.com/visio/2010/relationships/document";
const REL_PAGES: &str = "http://schemas.microsoft.com/visio/2010/relationships/pages";
const REL_PAGE: &str = "http://schemas.microsoft.com/visio/2010/relationships/page";
const REL_MASTERS: &str = "http://schemas.microsoft.com/visio/2010/relationships/masters";
const REL_MASTER: &str = "http://schemas.microsoft.com/visio/2010/relationships/master";
const REL_WINDOWS: &str = "http://schemas.microsoft.com/visio/2010/relationships/windows";

/// One archive entry: name, bytes and compression method.
struct Entry {
    name: String,
    data: Vec<u8>,
    method: CompressionMethod,
}

/// Which well-formed package kind to assemble.
#[derive(Clone, Copy, PartialEq, Eq)]
enum DocKind {
    Drawing,
    Stencil,
    Template,
}

/// Builds the raw bytes of a fixture package (see [`Fixture`]).
///
/// The output is deterministic: the same fixture always produces the same
/// bytes, so a committed file can be checked against the builder output.
pub fn build(fixture: Fixture) -> Vec<u8> {
    match fixture {
        Fixture::Drawing => zip_entries(&package_entries(DocKind::Drawing)),
        Fixture::Stencil => zip_entries(&package_entries(DocKind::Stencil)),
        Fixture::Template => zip_entries(&package_entries(DocKind::Template)),
        Fixture::Malformed(malformed) => build_malformed(malformed),
        Fixture::Feature(feature) => zip_entries(&render_entries(feature)),
    }
}

/// The file name a fixture is committed under, e.g. `"minimal-drawing.vsdx"`.
pub fn file_name(fixture: &Fixture) -> &'static str {
    match fixture {
        Fixture::Drawing => "minimal-drawing.vsdx",
        Fixture::Stencil => "minimal-stencil.vssx",
        Fixture::Template => "minimal-template.vstx",
        Fixture::Malformed(m) => match m {
            Malformed::TruncatedZip => "malformed-truncated-zip.vsdx",
            Malformed::BadCentralDirectory => "malformed-bad-central-directory.vsdx",
            Malformed::NoContentTypes => "malformed-no-content-types.vsdx",
            Malformed::DanglingRelationship => "malformed-dangling-relationship.vsdx",
            Malformed::MissingMainPart => "malformed-missing-main-part.vsdx",
            Malformed::HugeDeclaredSize => "malformed-huge-declared-size.vsdx",
            Malformed::DeepNesting => "malformed-deep-nesting.vsdx",
            Malformed::BadContentTypes => "malformed-bad-content-types.vsdx",
            Malformed::ZipBomb => "malformed-zip-bomb.vsdx",
            Malformed::ManyEntries => "malformed-many-entries.vsdx",
        },
        Fixture::Feature(feature) => match feature {
            Feature::GeometryAbsolute => "render-geometry-absolute.vsdx",
            Feature::GeometryRelative => "render-geometry-relative.vsdx",
            Feature::Fills => "render-fills.vsdx",
            Feature::Lines => "render-lines.vsdx",
            Feature::Shadows => "render-shadows.vsdx",
            Feature::Text => "render-text.vsdx",
            Feature::MasterInheritance => "render-master-inheritance.vsdx",
            Feature::Theme => "render-theme.vsdx",
            Feature::Layers => "render-layers.vsdx",
            Feature::PageSize => "render-page-size.vsdx",
            Feature::BackgroundPages => "render-background-pages.vsdx",
        },
    }
}

/// Path of a committed fixture under `corpus/fixtures/`, if it is committed
/// rather than generated.
///
/// The zip-bomb and many-entries fixtures are generated by [`build`] at test
/// time: the bomb's deflate stream is not stable across `flate2` versions and
/// the many-entries archive is a boring 700 KiB of headers.
pub fn path(name: &str) -> Option<PathBuf> {
    let committed = COMMITTED_FIXTURES
        .iter()
        .any(|fixture| file_name(fixture) == name);
    committed.then(|| fixtures_dir().join(name))
}

/// The `corpus/fixtures/` directory in the repository root.
pub fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/fixtures")
}

fn build_malformed(malformed: Malformed) -> Vec<u8> {
    match malformed {
        Malformed::TruncatedZip => {
            let bytes = build(Fixture::Drawing);
            Vec::from(bytes.get(..bytes.len() / 2).unwrap_or_default())
        }
        Malformed::BadCentralDirectory => corrupt_central_directory(build(Fixture::Drawing)),
        Malformed::NoContentTypes => {
            let entries: Vec<Entry> = package_entries(DocKind::Drawing)
                .into_iter()
                .filter(|e| e.name != "[Content_Types].xml")
                .collect();
            zip_entries(&entries)
        }
        Malformed::DanglingRelationship => {
            let mut entries = package_entries(DocKind::Drawing);
            for entry in &mut entries {
                if entry.name == "visio/_rels/document.xml.rels" {
                    entry.data = add_dangling_relationship(&entry.data);
                }
            }
            zip_entries(&entries)
        }
        Malformed::MissingMainPart => {
            let entries: Vec<Entry> = package_entries(DocKind::Drawing)
                .into_iter()
                .filter(|e| e.name != "visio/document.xml")
                .collect();
            zip_entries(&entries)
        }
        Malformed::HugeDeclaredSize => declare_huge_size(
            zip_entries(&package_entries(DocKind::Drawing)),
            "visio/pages/page1.xml",
            100 * 1024 * 1024,
        ),
        Malformed::DeepNesting => {
            let mut entries = package_entries(DocKind::Drawing);
            for entry in &mut entries {
                if entry.name == "visio/document.xml" {
                    entry.data = deep_document_xml(200).into_bytes();
                }
            }
            zip_entries(&entries)
        }
        Malformed::BadContentTypes => {
            let mut entries = package_entries(DocKind::Drawing);
            for entry in &mut entries {
                if entry.name == "[Content_Types].xml" {
                    // Well-formed opening tag, then the file just stops.
                    entry.data =
                        format!("{XML_DECL}\n<Types xmlns=\"{XMLNS_CT}\"><Default").into_bytes();
                }
            }
            zip_entries(&entries)
        }
        Malformed::ZipBomb => {
            let mut entries = package_entries(DocKind::Drawing);
            entries.push(Entry {
                name: "visio/bomb.bin".to_owned(),
                data: vec![0; 8 * 1024 * 1024],
                method: CompressionMethod::Deflated,
            });
            zip_entries(&entries)
        }
        Malformed::ManyEntries => {
            let entries: Vec<Entry> = (0..=larrygram_vsdx::Limits::DEFAULT.max_entries)
                .map(|i| Entry {
                    name: format!("x{i:05}.bin"),
                    data: Vec::new(),
                    method: CompressionMethod::Stored,
                })
                .collect();
            zip_entries(&entries)
        }
    }
}

/// The part set of one well-formed package (ADR 0016): content types,
/// package and document relationships, `document.xml`, `pages.xml` (empty for
/// a stencil), `page1.xml` (drawings and templates), the master parts
/// (stencils and templates) and the windows part (task P8-20).
fn package_entries(kind: DocKind) -> Vec<Entry> {
    let has_pages_contents = kind != DocKind::Stencil;
    let has_masters = kind != DocKind::Drawing;
    let stored = CompressionMethod::Stored;
    let mut entries = vec![
        Entry {
            name: "[Content_Types].xml".to_owned(),
            data: content_types(kind, has_pages_contents, has_masters).into_bytes(),
            method: stored,
        },
        Entry {
            name: "_rels/.rels".to_owned(),
            data: root_rels().into_bytes(),
            method: stored,
        },
        Entry {
            name: "visio/document.xml".to_owned(),
            data: document_xml().into_bytes(),
            method: stored,
        },
        Entry {
            name: "visio/_rels/document.xml.rels".to_owned(),
            data: document_rels(has_masters).into_bytes(),
            method: stored,
        },
        Entry {
            name: "visio/pages/pages.xml".to_owned(),
            data: pages_xml(has_pages_contents).into_bytes(),
            method: stored,
        },
    ];
    if has_pages_contents {
        entries.push(Entry {
            name: "visio/pages/_rels/pages.xml.rels".to_owned(),
            data: pages_rels().into_bytes(),
            method: stored,
        });
        entries.push(Entry {
            name: "visio/pages/page1.xml".to_owned(),
            data: page1_xml().into_bytes(),
            method: stored,
        });
    }
    if has_masters {
        entries.push(Entry {
            name: "visio/masters/masters.xml".to_owned(),
            data: masters_xml().into_bytes(),
            method: stored,
        });
        entries.push(Entry {
            name: "visio/masters/_rels/masters.xml.rels".to_owned(),
            data: masters_rels().into_bytes(),
            method: stored,
        });
        entries.push(Entry {
            name: "visio/masters/master1.xml".to_owned(),
            data: master1_xml().into_bytes(),
            method: stored,
        });
    }
    entries.push(Entry {
        name: "visio/windows.xml".to_owned(),
        data: windows_xml().into_bytes(),
        method: stored,
    });
    entries
}

/// The next free `rId` for the ids a relationships part already uses.
fn next_rel_id(used: &[&str]) -> String {
    let mut n: u32 = 1;
    loop {
        let id = format!("rId{n}");
        if !used.contains(&id.as_str()) {
            return id;
        }
        n = n.saturating_add(1);
    }
}

/// The windows part: an empty `Windows` root (task P8-20: an empty element
/// is what Visio accepts; the element names are structural facts recorded in
/// `docs/vsdx-inventory.md`, the values are ours).
fn windows_xml() -> String {
    format!("{XML_DECL}\n<Windows xmlns=\"{XMLNS_VISIO}\" xmlns:r=\"{XMLNS_R}\"/>")
}

fn content_types(kind: DocKind, has_pages_contents: bool, has_masters: bool) -> String {
    let main = match kind {
        DocKind::Drawing => CT_DRAWING_MAIN,
        DocKind::Stencil => CT_STENCIL_MAIN,
        DocKind::Template => CT_TEMPLATE_MAIN,
    };
    let mut s = String::new();
    s.push_str(XML_DECL);
    s.push_str(&format!("<Types xmlns=\"{XMLNS_CT}\">"));
    s.push_str(&format!(
        "<Default Extension=\"rels\" ContentType=\"{CT_RELS}\"/>"
    ));
    s.push_str(&format!(
        "<Default Extension=\"xml\" ContentType=\"{CT_XML}\"/>"
    ));
    override_decl(&mut s, "/visio/document.xml", main);
    override_decl(&mut s, "/visio/pages/pages.xml", CT_PAGES);
    if has_pages_contents {
        override_decl(&mut s, "/visio/pages/page1.xml", CT_PAGE);
    }
    if has_masters {
        override_decl(&mut s, "/visio/masters/masters.xml", CT_MASTERS);
        override_decl(&mut s, "/visio/masters/master1.xml", CT_MASTER);
    }
    override_decl(&mut s, "/visio/windows.xml", CT_WINDOWS);
    s.push_str("</Types>");
    s
}

fn override_decl(s: &mut String, part: &str, content_type: &str) {
    s.push_str(&format!(
        "<Override PartName=\"{part}\" ContentType=\"{content_type}\"/>"
    ));
}

fn root_rels() -> String {
    format!(
        "{XML_DECL}\n<Relationships xmlns=\"{XMLNS_RELS}\">\
         <Relationship Id=\"rId1\" Type=\"{REL_DOCUMENT}\" Target=\"visio/document.xml\"/>\
         </Relationships>"
    )
}

fn document_xml() -> String {
    format!(
        "{XML_DECL}\n<VisioDocument xmlns=\"{XMLNS_VISIO}\" xmlns:r=\"{XMLNS_R}\" xml:space=\"preserve\">\
         <DocumentSettings TopPage=\"0\" DefaultTextStyle=\"0\" DefaultLineStyle=\"0\" \
         DefaultFillStyle=\"0\" DefaultGuideStyle=\"0\"/>\
         </VisioDocument>"
    )
}

fn document_rels(has_masters: bool) -> String {
    let mut s = String::new();
    s.push_str(XML_DECL);
    s.push_str(&format!("\n<Relationships xmlns=\"{XMLNS_RELS}\">"));
    s.push_str(&format!(
        "<Relationship Id=\"rId1\" Type=\"{REL_PAGES}\" Target=\"pages/pages.xml\"/>"
    ));
    let mut used = vec!["rId1"];
    if has_masters {
        s.push_str(&format!(
            "<Relationship Id=\"rId2\" Type=\"{REL_MASTERS}\" Target=\"masters/masters.xml\"/>"
        ));
        used.push("rId2");
    }
    s.push_str(&format!(
        "<Relationship Id=\"{}\" Type=\"{REL_WINDOWS}\" Target=\"windows.xml\"/>",
        next_rel_id(&used)
    ));
    s.push_str("</Relationships>");
    s
}

fn pages_xml(has_pages_contents: bool) -> String {
    if !has_pages_contents {
        // A stencil carries no pages (ADR 0016).
        return format!(
            "{XML_DECL}\n<Pages xmlns=\"{XMLNS_VISIO}\" xmlns:r=\"{XMLNS_R}\" xml:space=\"preserve\"/>"
        );
    }
    format!(
        "{XML_DECL}\n<Pages xmlns=\"{XMLNS_VISIO}\" xmlns:r=\"{XMLNS_R}\" xml:space=\"preserve\">\
         <Page ID=\"0\" NameU=\"Page-1\" Name=\"Page-1\">\
         <PageSheet LineStyle=\"0\" FillStyle=\"0\" TextStyle=\"0\">\
         <Cell N=\"PageWidth\" V=\"8.5\"/><Cell N=\"PageHeight\" V=\"11\"/>\
         </PageSheet><Rel r:id=\"rId1\"/></Page></Pages>"
    )
}

/// The `pages.xml` relationships part of a drawing fixture.
pub fn pages_rels() -> String {
    format!(
        "{XML_DECL}\n<Relationships xmlns=\"{XMLNS_RELS}\">\
         <Relationship Id=\"rId1\" Type=\"{REL_PAGE}\" Target=\"page1.xml\"/>\
         </Relationships>"
    )
}

fn page1_xml() -> String {
    format!(
        "{XML_DECL}\n<PageContents xmlns=\"{XMLNS_VISIO}\" xmlns:r=\"{XMLNS_R}\" xml:space=\"preserve\">\
         <Shapes>{}</Shapes></PageContents>",
        rect_shape(1)
    )
}

fn masters_xml() -> String {
    format!(
        "{XML_DECL}\n<Masters xmlns=\"{XMLNS_VISIO}\" xmlns:r=\"{XMLNS_R}\" xml:space=\"preserve\">\
         <Master ID=\"1\" NameU=\"Master.1\" Name=\"Master 1\" \
         UniqueID=\"{{5A4E1C2D-3B6F-4A21-9C0D-8E7F6A5B4C3D}}\" \
         BaseID=\"{{9F8E7D6C-5B4A-4321-8F0E-1D2C3B4A5968}}\" \
         LineStyle=\"0\" FillStyle=\"0\" TextStyle=\"0\"><Rel r:id=\"rId1\"/></Master></Masters>"
    )
}

fn masters_rels() -> String {
    format!(
        "{XML_DECL}\n<Relationships xmlns=\"{XMLNS_RELS}\">\
         <Relationship Id=\"rId1\" Type=\"{REL_MASTER}\" Target=\"master1.xml\"/>\
         </Relationships>"
    )
}

fn master1_xml() -> String {
    format!(
        "{XML_DECL}\n<MasterContents xmlns=\"{XMLNS_VISIO}\" xmlns:r=\"{XMLNS_R}\" xml:space=\"preserve\">\
         <Shapes>{}</Shapes></MasterContents>",
        rect_shape(1)
    )
}

/// A 2 by 1 rectangle with its own geometry rows. The unit-square path
/// (relative coordinates 0 and 1) is the generic way to draw any rectangle
/// and matches the [MS-VSDX] example only because there is no other choice;
/// the shape values are our own.
fn rect_shape(id: u32) -> String {
    format!(
        "<Shape ID=\"{id}\" Type=\"Shape\" LineStyle=\"0\" FillStyle=\"0\" TextStyle=\"0\">\
         <Cell N=\"PinX\" V=\"2\"/><Cell N=\"PinY\" V=\"2\"/>\
         <Cell N=\"Width\" V=\"2\"/><Cell N=\"Height\" V=\"1\"/>\
         <Cell N=\"LocPinX\" V=\"1\"/><Cell N=\"LocPinY\" V=\"0.5\"/>\
         <Section N=\"Geometry\" IX=\"0\"><Cell N=\"NoFill\" V=\"0\"/>\
         <Row T=\"RelMoveTo\" IX=\"1\"><Cell N=\"X\" V=\"0\"/><Cell N=\"Y\" V=\"0\"/></Row>\
         <Row T=\"RelLineTo\" IX=\"2\"><Cell N=\"X\" V=\"1\"/><Cell N=\"Y\" V=\"0\"/></Row>\
         <Row T=\"RelLineTo\" IX=\"3\"><Cell N=\"X\" V=\"1\"/><Cell N=\"Y\" V=\"1\"/></Row>\
         <Row T=\"RelLineTo\" IX=\"4\"><Cell N=\"X\" V=\"0\"/><Cell N=\"Y\" V=\"1\"/></Row>\
         <Row T=\"RelLineTo\" IX=\"5\"><Cell N=\"X\" V=\"0\"/><Cell N=\"Y\" V=\"0\"/></Row>\
         </Section></Shape>"
    )
}

/// `VisioDocument` with `depth` nested `<Deep>` elements: deeper than
/// `Limits::DEFAULT.max_xml_depth`.
fn deep_document_xml(depth: u32) -> String {
    let mut s = String::new();
    s.push_str(XML_DECL);
    s.push_str(&format!(
        "\n<VisioDocument xmlns=\"{XMLNS_VISIO}\" xmlns:r=\"{XMLNS_R}\" xml:space=\"preserve\">"
    ));
    for _ in 0..depth {
        s.push_str("<Deep>");
    }
    for _ in 0..depth {
        s.push_str("</Deep>");
    }
    s.push_str("</VisioDocument>");
    s
}

/// Adds a relationship whose target part does not exist.
pub fn add_dangling_relationship(rels: &[u8]) -> Vec<u8> {
    let text = String::from_utf8_lossy(rels);
    let extra = "<Relationship Id=\"rId9\" Type=\"http://example.invalid/dangling\" \
                 Target=\"pages/missing.xml\"/>";
    text.replace("</Relationships>", &format!("{extra}</Relationships>"))
        .into_bytes()
}

/// Writes the entries as a plain zip archive with a fixed, clock-free
/// timestamp, so the bytes are a pure function of the entries.
fn zip_entries(entries: &[Entry]) -> Vec<u8> {
    use std::io::Write;
    let mut cursor = std::io::Cursor::new(Vec::new());
    {
        let mut zip = ZipWriter::new(&mut cursor);
        for entry in entries {
            let options = SimpleFileOptions::default()
                .compression_method(entry.method)
                .last_modified_time(DateTime::default());
            if zip.start_file(entry.name.as_str(), options).is_err() {
                continue;
            }
            if zip.write_all(&entry.data).is_err() {
                continue;
            }
        }
        let _ = zip.finish();
    }
    cursor.into_inner()
}

// Byte surgery on the zip container: the `zip` crate writes honest headers,
// and some failure modes need a lie. The helpers are `pub` so their unit tests
// live in `tests/fixtures.rs` and run once per `cargo test`, not once per test
// binary that includes this module (task P2-09).

/// The zip local-file-header signature.
pub const LOCAL_SIG: &[u8] = b"PK\x03\x04";
/// The zip central-directory-header signature.
pub const CENTRAL_SIG: &[u8] = b"PK\x01\x02";
/// The zip end-of-central-directory signature.
pub const EOCD_SIG: &[u8] = b"PK\x05\x06";

/// The start offset of the last subslice equal to `needle`.
pub fn rfind_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    (0..=haystack.len() - needle.len())
        .rev()
        .find(|&i| haystack.get(i..i + needle.len()) == Some(needle))
}

/// The start offset of the zip header with `sig` whose file-name field
/// (`name_at` bytes into the header) is `name`.
pub fn find_header(bytes: &[u8], sig: &[u8], name: &str, name_at: usize) -> Option<usize> {
    if sig.is_empty() {
        return None;
    }
    let name = name.as_bytes();
    let mut i = 0;
    while let Some(tail) = bytes.get(i..) {
        let rel = tail.windows(sig.len()).position(|w| w == sig)?;
        let start = i + rel;
        if bytes.get(start + name_at..start + name_at + name.len()) == Some(name) {
            return Some(start);
        }
        i = start + 1;
    }
    None
}

/// The little-endian `u32` at `offset`, or `None` when it does not fit.
pub fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let slot = bytes.get(offset..offset + 4)?;
    Some(u32::from_le_bytes([slot[0], slot[1], slot[2], slot[3]]))
}

/// Writes `value` as a little-endian `u32` at `offset`, or does nothing when
/// it does not fit.
pub fn patch_u32(bytes: &mut [u8], offset: usize, value: u32) {
    if let Some(slot) = bytes.get_mut(offset..offset + 4) {
        slot.copy_from_slice(&value.to_le_bytes());
    }
}

/// Declares `declared` uncompressed bytes for `name` in both its local and
/// central-directory header, while the entry data stays as it is.
pub fn declare_huge_size(mut zip_bytes: Vec<u8>, name: &str, declared: u32) -> Vec<u8> {
    // Local file header: file name at +30, uncompressed size at +22.
    if let Some(start) = find_header(&zip_bytes, LOCAL_SIG, name, 30) {
        patch_u32(&mut zip_bytes, start + 22, declared);
    }
    // Central directory header: file name at +46, uncompressed size at +24.
    if let Some(start) = find_header(&zip_bytes, CENTRAL_SIG, name, 46) {
        patch_u32(&mut zip_bytes, start + 24, declared);
    }
    zip_bytes
}

/// Destroys the first central-directory header signature the end-of-central-
/// directory record points at.
pub fn corrupt_central_directory(mut zip_bytes: Vec<u8>) -> Vec<u8> {
    if let Some(offset) =
        rfind_subslice(&zip_bytes, EOCD_SIG).and_then(|eocd| read_u32(&zip_bytes, eocd + 16))
    {
        patch_u32(&mut zip_bytes, offset as usize, 0xFFFF_FFFF);
    }
    zip_bytes
}

// --- rendering feature fixtures (task P3-10) --------------------------
//
// One minimal drawing package per KICKOFF Phase 3 rendering feature
// (ADR 0027 section 3). The XML is our own work: element and attribute
// names, content types and relationship types are structural facts from
// [MS-VSDX] and `docs/vsdx-inventory.md`; every value is chosen by us.

const CT_THEME: &str = "application/vnd.openxmlformats-officedocument.theme+xml";
const REL_THEME: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme";

/// The part set of one rendering feature package (see [`Feature`]).
fn render_entries(feature: Feature) -> Vec<Entry> {
    let stored = CompressionMethod::Stored;
    let mut entries = vec![
        Entry {
            name: "[Content_Types].xml".to_owned(),
            data: render_content_types(feature).into_bytes(),
            method: stored,
        },
        Entry {
            name: "_rels/.rels".to_owned(),
            data: root_rels().into_bytes(),
            method: stored,
        },
        Entry {
            name: "visio/document.xml".to_owned(),
            data: document_xml().into_bytes(),
            method: stored,
        },
        Entry {
            name: "visio/_rels/document.xml.rels".to_owned(),
            data: render_document_rels(feature).into_bytes(),
            method: stored,
        },
        Entry {
            name: "visio/pages/pages.xml".to_owned(),
            data: render_pages_xml(feature).into_bytes(),
            method: stored,
        },
        Entry {
            name: "visio/pages/_rels/pages.xml.rels".to_owned(),
            data: render_pages_rels(feature).into_bytes(),
            method: stored,
        },
        Entry {
            name: "visio/pages/page1.xml".to_owned(),
            data: page_contents(&render_shapes(feature)).into_bytes(),
            method: stored,
        },
    ];
    if feature == Feature::BackgroundPages {
        entries.push(Entry {
            name: "visio/pages/page2.xml".to_owned(),
            data: page_contents(&background_shapes()).into_bytes(),
            method: stored,
        });
    }
    if feature == Feature::MasterInheritance {
        entries.push(Entry {
            name: "visio/masters/masters.xml".to_owned(),
            data: masters_xml().into_bytes(),
            method: stored,
        });
        entries.push(Entry {
            name: "visio/masters/_rels/masters.xml.rels".to_owned(),
            data: masters_rels().into_bytes(),
            method: stored,
        });
        entries.push(Entry {
            name: "visio/masters/master1.xml".to_owned(),
            data: master1_xml().into_bytes(),
            method: stored,
        });
    }
    if feature == Feature::Theme {
        entries.push(Entry {
            name: "visio/theme/theme1.xml".to_owned(),
            data: theme_xml().into_bytes(),
            method: stored,
        });
    }
    entries.push(Entry {
        name: "visio/windows.xml".to_owned(),
        data: windows_xml().into_bytes(),
        method: stored,
    });
    entries
}

/// The `[Content_Types].xml` of a rendering feature package.
fn render_content_types(feature: Feature) -> String {
    let mut s = String::new();
    s.push_str(XML_DECL);
    s.push_str(&format!("<Types xmlns=\"{XMLNS_CT}\">"));
    s.push_str(&format!(
        "<Default Extension=\"rels\" ContentType=\"{CT_RELS}\"/>"
    ));
    s.push_str(&format!(
        "<Default Extension=\"xml\" ContentType=\"{CT_XML}\"/>"
    ));
    override_decl(&mut s, "/visio/document.xml", CT_DRAWING_MAIN);
    override_decl(&mut s, "/visio/pages/pages.xml", CT_PAGES);
    override_decl(&mut s, "/visio/pages/page1.xml", CT_PAGE);
    if feature == Feature::BackgroundPages {
        override_decl(&mut s, "/visio/pages/page2.xml", CT_PAGE);
    }
    if feature == Feature::MasterInheritance {
        override_decl(&mut s, "/visio/masters/masters.xml", CT_MASTERS);
        override_decl(&mut s, "/visio/masters/master1.xml", CT_MASTER);
    }
    if feature == Feature::Theme {
        override_decl(&mut s, "/visio/theme/theme1.xml", CT_THEME);
    }
    override_decl(&mut s, "/visio/windows.xml", CT_WINDOWS);
    s.push_str("</Types>");
    s
}

/// The `document.xml` relationships of a rendering feature package.
fn render_document_rels(feature: Feature) -> String {
    let mut s = String::new();
    s.push_str(XML_DECL);
    s.push_str(&format!("\n<Relationships xmlns=\"{XMLNS_RELS}\">"));
    s.push_str(&format!(
        "<Relationship Id=\"rId1\" Type=\"{REL_PAGES}\" Target=\"pages/pages.xml\"/>"
    ));
    let mut used = vec!["rId1"];
    if feature == Feature::MasterInheritance {
        s.push_str(&format!(
            "<Relationship Id=\"rId2\" Type=\"{REL_MASTERS}\" Target=\"masters/masters.xml\"/>"
        ));
        used.push("rId2");
    }
    if feature == Feature::Theme {
        s.push_str(&format!(
            "<Relationship Id=\"rId3\" Type=\"{REL_THEME}\" Target=\"theme/theme1.xml\"/>"
        ));
        used.push("rId3");
    }
    s.push_str(&format!(
        "<Relationship Id=\"{}\" Type=\"{REL_WINDOWS}\" Target=\"windows.xml\"/>",
        next_rel_id(&used)
    ));
    s.push_str("</Relationships>");
    s
}

/// The `pages.xml` relationships of a rendering feature package.
fn render_pages_rels(feature: Feature) -> String {
    let mut s = String::new();
    s.push_str(XML_DECL);
    s.push_str(&format!("\n<Relationships xmlns=\"{XMLNS_RELS}\">"));
    s.push_str(&format!(
        "<Relationship Id=\"rId1\" Type=\"{REL_PAGE}\" Target=\"page1.xml\"/>"
    ));
    if feature == Feature::BackgroundPages {
        s.push_str(&format!(
            "<Relationship Id=\"rId2\" Type=\"{REL_PAGE}\" Target=\"page2.xml\"/>"
        ));
    }
    s.push_str("</Relationships>");
    s
}

/// The `pages.xml` of a rendering feature package: the page list and the
/// page sheets (page size and the `Layer` section where the feature
/// needs them).
fn render_pages_xml(feature: Feature) -> String {
    let (width, height) = match feature {
        Feature::PageSize => ("11", "8.5"),
        _ => ("8.5", "11"),
    };
    let mut sheet =
        format!("<Cell N=\"PageWidth\" V=\"{width}\"/><Cell N=\"PageHeight\" V=\"{height}\"/>");
    if feature == Feature::Layers {
        sheet.push_str(
            "<Section N=\"Layer\">\
             <Row IX=\"0\"><Cell N=\"Name\" V=\"Shown\"/><Cell N=\"Visible\" V=\"1\"/></Row>\
             <Row IX=\"1\"><Cell N=\"Name\" V=\"Hidden\"/><Cell N=\"Visible\" V=\"0\"/></Row>\
             </Section>",
        );
    }
    let mut pages = format!(
        "<Page ID=\"0\" NameU=\"Page-1\" Name=\"Page-1\"{back}>\
         <PageSheet LineStyle=\"0\" FillStyle=\"0\" TextStyle=\"0\">{sheet}</PageSheet>\
         <Rel r:id=\"rId1\"/></Page>",
        back = if feature == Feature::BackgroundPages {
            " BackPage=\"1\""
        } else {
            ""
        }
    );
    if feature == Feature::BackgroundPages {
        pages.push_str(
            "<Page ID=\"1\" NameU=\"Page-2\" Name=\"Background\" Background=\"1\">\
             <PageSheet LineStyle=\"0\" FillStyle=\"0\" TextStyle=\"0\">\
             <Cell N=\"PageWidth\" V=\"8.5\"/><Cell N=\"PageHeight\" V=\"11\"/>\
             </PageSheet><Rel r:id=\"rId2\"/></Page>",
        );
    }
    format!(
        "{XML_DECL}\n<Pages xmlns=\"{XMLNS_VISIO}\" xmlns:r=\"{XMLNS_R}\" xml:space=\"preserve\">\
         {pages}</Pages>"
    )
}

/// A `PageContents`/`MasterContents` body around shape markup.
fn page_contents(shapes: &str) -> String {
    format!(
        "{XML_DECL}\n<PageContents xmlns=\"{XMLNS_VISIO}\" xmlns:r=\"{XMLNS_R}\" xml:space=\"preserve\">\
         <Shapes>{shapes}</Shapes></PageContents>"
    )
}

/// One `<Cell>` with a raw `V` value (numeric, `#RRGGBB` or string).
fn cell_xml(name: &str, value: &str) -> String {
    format!("<Cell N=\"{name}\" V=\"{value}\"/>")
}

/// One typed geometry `<Row>` with raw cell values.
fn geo_row_xml(row_type: &str, ix: u32, cells: &[(&str, &str)]) -> String {
    let cells: String = cells
        .iter()
        .map(|(name, value)| cell_xml(name, value))
        .collect();
    format!("<Row T=\"{row_type}\" IX=\"{ix}\">{cells}</Row>")
}

/// A `Geometry` section of `rows`: `no_fill` for stroked open paths.
fn geometry_section_xml(rows: &[String], no_fill: bool) -> String {
    let no_fill = if no_fill { "1" } else { "0" };
    format!(
        "<Section N=\"Geometry\" IX=\"0\"><Cell N=\"NoFill\" V=\"{no_fill}\"/>\
         <Cell N=\"NoLine\" V=\"0\"/>{}\
         </Section>",
        rows.concat()
    )
}

/// A filled rectangle shape (relative geometry rows) with `cells`.
fn rect_xml(id: u32, pin: (&str, &str), size: (&str, &str), cells: &[(&str, &str)]) -> String {
    let (px, py) = pin;
    let (w, h) = size;
    let half = |value: &str| {
        let size: f64 = value.parse().unwrap_or(0.0);
        format!("{}", size / 2.0)
    };
    let own: String = cells.iter().map(|(n, v)| cell_xml(n, v)).collect();
    format!(
        "<Shape ID=\"{id}\" Type=\"Shape\" LineStyle=\"0\" FillStyle=\"0\" TextStyle=\"0\">\
         <Cell N=\"PinX\" V=\"{px}\"/><Cell N=\"PinY\" V=\"{py}\"/>\
         <Cell N=\"Width\" V=\"{w}\"/><Cell N=\"Height\" V=\"{h}\"/>\
         <Cell N=\"LocPinX\" V=\"{lx}\"/><Cell N=\"LocPinY\" V=\"{ly}\"/>{own}\
         <Section N=\"Geometry\" IX=\"0\"><Cell N=\"NoFill\" V=\"0\"/><Cell N=\"NoLine\" V=\"0\"/>\
         <Row T=\"RelMoveTo\" IX=\"1\"><Cell N=\"X\" V=\"0\"/><Cell N=\"Y\" V=\"0\"/></Row>\
         <Row T=\"RelLineTo\" IX=\"2\"><Cell N=\"X\" V=\"1\"/><Cell N=\"Y\" V=\"0\"/></Row>\
         <Row T=\"RelLineTo\" IX=\"3\"><Cell N=\"X\" V=\"1\"/><Cell N=\"Y\" V=\"1\"/></Row>\
         <Row T=\"RelLineTo\" IX=\"4\"><Cell N=\"X\" V=\"0\"/><Cell N=\"Y\" V=\"1\"/></Row>\
         <Row T=\"RelLineTo\" IX=\"5\"><Cell N=\"X\" V=\"0\"/><Cell N=\"Y\" V=\"0\"/></Row>\
         </Section></Shape>",
        lx = half(w),
        ly = half(h)
    )
}

/// An open polyline shape (absolute rows through `points`) with `cells`.
fn polyline_xml(id: u32, points: &[(&str, &str)], cells: &[(&str, &str)]) -> String {
    let own: String = cells.iter().map(|(n, v)| cell_xml(n, v)).collect();
    let mut rows = vec![geo_row_xml(
        "MoveTo",
        1,
        &[("X", points[0].0), ("Y", points[0].1)],
    )];
    for (position, (x, y)) in points.iter().enumerate().skip(1) {
        let ix = u32::try_from(position).unwrap_or(u32::MAX) + 1;
        rows.push(geo_row_xml("LineTo", ix, &[("X", x), ("Y", y)]));
    }
    format!(
        "<Shape ID=\"{id}\" Type=\"Shape\" LineStyle=\"0\" FillStyle=\"0\" TextStyle=\"0\">\
         <Cell N=\"PinX\" V=\"0\"/><Cell N=\"PinY\" V=\"0\"/>\
         <Cell N=\"Width\" V=\"8.5\"/><Cell N=\"Height\" V=\"11\"/>\
         <Cell N=\"LocPinX\" V=\"0\"/><Cell N=\"LocPinY\" V=\"0\"/>{own}{}</Shape>",
        geometry_section_xml(&rows, true)
    )
}

/// The shapes of the feature's main page.
fn render_shapes(feature: Feature) -> String {
    match feature {
        Feature::GeometryAbsolute => geometry_absolute_shapes(),
        Feature::GeometryRelative => geometry_relative_shapes(),
        Feature::Fills => fills_shapes(),
        Feature::Lines => lines_shapes(),
        Feature::Shadows => shadows_shapes(),
        Feature::Text => text_shapes(),
        Feature::MasterInheritance => master_instance_shape(),
        Feature::Theme => theme_shapes(),
        Feature::Layers => layers_shapes(),
        Feature::PageSize => format!(
            "{}{}",
            rect_xml(
                1,
                ("3", "4.25"),
                ("2", "1"),
                &[("FillPattern", "1"), ("FillForegnd", "#27AE60")]
            ),
            rect_xml(
                2,
                ("8", "4.25"),
                ("2", "1"),
                &[("FillPattern", "1"), ("FillForegnd", "#C0392B")]
            )
        ),
        Feature::BackgroundPages => rect_xml(
            1,
            ("4.25", "6"),
            ("3", "1.5"),
            &[("FillPattern", "1"), ("FillForegnd", "#C0392B")],
        ),
    }
}

/// The background page's shapes (a full-page tint).
fn background_shapes() -> String {
    rect_xml(
        1,
        ("4.25", "5.5"),
        ("8.5", "11"),
        &[("FillPattern", "1"), ("FillForegnd", "#D9E2EC")],
    )
}

/// One shape with an arbitrary geometry `body` and extra `cells`.
fn free_shape_xml(id: u32, cells: &[(&str, &str)], body: &str, text: &str) -> String {
    let own: String = cells.iter().map(|(n, v)| cell_xml(n, v)).collect();
    let text_element = if text.is_empty() {
        String::new()
    } else {
        format!("<Text>{text}</Text>")
    };
    format!(
        "<Shape ID=\"{id}\" Type=\"Shape\" LineStyle=\"0\" FillStyle=\"0\" TextStyle=\"0\">{own}{body}{text_element}</Shape>"
    )
}

/// One shape of every absolute geometry row type (one subpath each).
fn geometry_absolute_shapes() -> String {
    let base = [
        ("PinX", "0"),
        ("PinY", "0"),
        ("Width", "8.5"),
        ("Height", "11"),
        ("LocPinX", "0"),
        ("LocPinY", "0"),
        ("FillPattern", "0"),
        ("LinePattern", "1"),
        ("LineWeight", "0.02"),
        ("LineColor", "#1F2A44"),
    ];
    let rows = vec![
        geo_row_xml("MoveTo", 1, &[("X", "1"), ("Y", "2")]),
        geo_row_xml("LineTo", 2, &[("X", "3"), ("Y", "2")]),
        geo_row_xml("MoveTo", 3, &[("X", "1"), ("Y", "3.5")]),
        geo_row_xml("ArcTo", 4, &[("X", "3"), ("Y", "3.5"), ("A", "0.5")]),
        geo_row_xml("MoveTo", 5, &[("X", "1"), ("Y", "5")]),
        geo_row_xml(
            "EllipticalArcTo",
            6,
            &[
                ("X", "3"),
                ("Y", "5"),
                ("A", "2"),
                ("B", "5.5"),
                ("C", "0"),
                ("D", "1"),
            ],
        ),
        geo_row_xml("MoveTo", 7, &[("X", "1"), ("Y", "6.5")]),
        geo_row_xml(
            "NURBSTo",
            8,
            &[
                ("X", "3"),
                ("Y", "6.5"),
                ("A", "1"),
                ("B", "1"),
                ("C", "0"),
                ("D", "1"),
                ("E", "NURBS(2, 1, 1, 1, 2, 7.5, 0.5, 1)"),
            ],
        ),
        geo_row_xml("MoveTo", 9, &[("X", "1"), ("Y", "8")]),
        geo_row_xml(
            "PolylineTo",
            10,
            &[("X", "3"), ("Y", "8"), ("A", "1.75 8.4; 2.25 7.6")],
        ),
        geo_row_xml("MoveTo", 11, &[("X", "4"), ("Y", "2")]),
        geo_row_xml(
            "SplineStart",
            12,
            &[
                ("X", "5"),
                ("Y", "2.6"),
                ("A", "1"),
                ("B", "0"),
                ("C", "2"),
                ("D", "2"),
            ],
        ),
        geo_row_xml("SplineKnot", 13, &[("X", "6"), ("Y", "2"), ("A", "1.5")]),
        geo_row_xml(
            "Ellipse",
            14,
            &[
                ("X", "5"),
                ("Y", "4"),
                ("A", "6"),
                ("B", "4"),
                ("C", "5"),
                ("D", "4.75"),
            ],
        ),
        geo_row_xml(
            "InfiniteLine",
            15,
            &[("X", "4"), ("Y", "6"), ("A", "7"), ("B", "6.5")],
        ),
    ];
    free_shape_xml(1, &base, &geometry_section_xml(&rows, true), "")
}

/// One shape of every relative geometry row type (one subpath each).
fn geometry_relative_shapes() -> String {
    let base = [
        ("PinX", "4.25"),
        ("PinY", "5.5"),
        ("Width", "6"),
        ("Height", "8"),
        ("LocPinX", "3"),
        ("LocPinY", "4"),
        ("FillPattern", "0"),
        ("LinePattern", "1"),
        ("LineWeight", "0.02"),
        ("LineColor", "#1F2A44"),
    ];
    let rows = vec![
        geo_row_xml("RelMoveTo", 1, &[("X", "0"), ("Y", "0")]),
        geo_row_xml("RelLineTo", 2, &[("X", "1"), ("Y", "0")]),
        geo_row_xml("RelLineTo", 3, &[("X", "1"), ("Y", "1")]),
        geo_row_xml("RelMoveTo", 4, &[("X", "0"), ("Y", "0.25")]),
        geo_row_xml(
            "RelQuadBezTo",
            5,
            &[("X", "1"), ("Y", "0.25"), ("A", "0.5"), ("B", "0.5")],
        ),
        geo_row_xml("RelMoveTo", 6, &[("X", "0"), ("Y", "0.5")]),
        geo_row_xml(
            "RelCubBezTo",
            7,
            &[
                ("X", "1"),
                ("Y", "0.5"),
                ("A", "0.25"),
                ("B", "0.75"),
                ("C", "0.75"),
                ("D", "0.25"),
            ],
        ),
        geo_row_xml("RelMoveTo", 8, &[("X", "0"), ("Y", "0.75")]),
        geo_row_xml(
            "RelEllipticalArcTo",
            9,
            &[
                ("X", "1"),
                ("Y", "0.75"),
                ("A", "0.5"),
                ("B", "0.8"),
                ("C", "0"),
                ("D", "1"),
            ],
        ),
    ];
    free_shape_xml(1, &base, &geometry_section_xml(&rows, true), "")
}

/// Solid, gradient and pattern fills: four shapes in a row.
fn fills_shapes() -> String {
    let gradient_stops = "<Section N=\"FillGradient\">\
         <Row IX=\"0\"><Cell N=\"GradientStopPosition\" V=\"0\"/>\
         <Cell N=\"GradientStopColor\" V=\"0\"/></Row>\
         <Row IX=\"1\"><Cell N=\"GradientStopPosition\" V=\"1\"/>\
         <Cell N=\"GradientStopColor\" V=\"2\"/></Row></Section>";
    let solid = rect_xml(
        1,
        ("1.4", "6.5"),
        ("1.8", "1.8"),
        &[
            ("FillPattern", "1"),
            ("FillForegnd", "#3366CC"),
            ("FillBkgnd", "#FFFFFF"),
        ],
    );
    let linear = free_shape_xml(
        2,
        &[
            ("PinX", "3.2"),
            ("PinY", "6.5"),
            ("Width", "1.8"),
            ("Height", "1.8"),
            ("LocPinX", "0.9"),
            ("LocPinY", "0.9"),
            ("FillPattern", "1"),
            ("FillGradientEnabled", "1"),
            ("FillGradientDir", "0"),
            ("FillGradientAngle", "0"),
        ],
        &format!(
            "{}{gradient_stops}",
            geometry_section_xml(&rect_rows(), false)
        ),
        "",
    );
    let radial_stops = "<Section N=\"FillGradient\">\
         <Row IX=\"0\"><Cell N=\"GradientStopPosition\" V=\"0\"/>\
         <Cell N=\"GradientStopColor\" V=\"5\"/></Row>\
         <Row IX=\"1\"><Cell N=\"GradientStopPosition\" V=\"1\"/>\
         <Cell N=\"GradientStopColor\" V=\"4\"/></Row></Section>";
    let radial = free_shape_xml(
        3,
        &[
            ("PinX", "5"),
            ("PinY", "6.5"),
            ("Width", "1.8"),
            ("Height", "1.8"),
            ("LocPinX", "0.9"),
            ("LocPinY", "0.9"),
            ("FillPattern", "30"),
            ("FillGradientDir", "1"),
        ],
        &format!(
            "{}{radial_stops}",
            geometry_section_xml(&rect_rows(), false)
        ),
        "",
    );
    let pattern = rect_xml(
        4,
        ("6.8", "6.5"),
        ("1.8", "1.8"),
        &[
            ("FillPattern", "7"),
            ("FillForegnd", "#3366CC"),
            ("FillBkgnd", "#FFFFFF"),
        ],
    );
    format!("{solid}{linear}{radial}{pattern}")
}

/// The relative rectangle rows of [`rect_xml`] (without the section).
fn rect_rows() -> Vec<String> {
    vec![
        geo_row_xml("RelMoveTo", 1, &[("X", "0"), ("Y", "0")]),
        geo_row_xml("RelLineTo", 2, &[("X", "1"), ("Y", "0")]),
        geo_row_xml("RelLineTo", 3, &[("X", "1"), ("Y", "1")]),
        geo_row_xml("RelLineTo", 4, &[("X", "0"), ("Y", "1")]),
        geo_row_xml("RelLineTo", 5, &[("X", "0"), ("Y", "0")]),
    ]
}

/// Line weight, pattern, line ends and rounding: four open polylines.
fn lines_shapes() -> String {
    let thick = polyline_xml(
        1,
        &[("1", "8"), ("3", "9"), ("5", "8")],
        &[
            ("LineWeight", "0.05"),
            ("LineColor", "#C0392B"),
            ("LinePattern", "1"),
        ],
    );
    let dashed = polyline_xml(
        2,
        &[("1", "6.5"), ("3", "7.5"), ("5", "6.5")],
        &[
            ("LineWeight", "0.02"),
            ("LineColor", "#2980B9"),
            ("LinePattern", "2"),
        ],
    );
    let ends = polyline_xml(
        3,
        &[("1", "5"), ("5", "5")],
        &[
            ("LineWeight", "0.02"),
            ("LineColor", "#27AE60"),
            ("BeginArrow", "4"),
            ("BeginArrowSize", "2"),
            ("EndArrow", "1"),
            ("EndArrowSize", "1"),
        ],
    );
    let rounded = polyline_xml(
        4,
        &[("1", "3"), ("3", "4.5"), ("5", "3")],
        &[
            ("LineWeight", "0.03"),
            ("LineColor", "#8E44AD"),
            ("LineCap", "1"),
            ("Rounding", "0.5"),
        ],
    );
    format!("{thick}{dashed}{ends}{rounded}")
}

/// Two rectangles with drop shadows.
fn shadows_shapes() -> String {
    let first = rect_xml(
        1,
        ("2.75", "6.5"),
        ("2", "2"),
        &[
            ("FillPattern", "1"),
            ("FillForegnd", "#CC6633"),
            ("ShapeShdwShow", "1"),
            ("ShdwPattern", "1"),
            ("ShdwForegnd", "#404040"),
            ("ShapeShdwOffsetX", "0.15"),
            ("ShapeShdwOffsetY", "-0.15"),
            ("ShapeShdwBlur", "0.05"),
        ],
    );
    let second = rect_xml(
        2,
        ("5.75", "6.5"),
        ("2", "2"),
        &[
            ("FillPattern", "1"),
            ("FillForegnd", "#3366CC"),
            ("ShapeShdwShow", "1"),
            ("ShdwPattern", "1"),
            ("ShdwForegnd", "#404040"),
            ("ShdwForegndTrans", "0.5"),
            ("ShapeShdwOffsetX", "-0.2"),
            ("ShapeShdwOffsetY", "-0.1"),
            ("ShapeShdwBlur", "0.1"),
        ],
    );
    format!("{first}{second}")
}

/// Text with runs and paragraphs, and text with bullets and tabs.
fn text_shapes() -> String {
    let runs_sections = "<Section N=\"Character\">\
         <Row IX=\"0\"><Cell N=\"Font\" V=\"Calibri\"/><Cell N=\"Size\" V=\"0.1666666666666667\"/>\
         <Cell N=\"Style\" V=\"0\"/><Cell N=\"Color\" V=\"#1F2A44\"/></Row>\
         <Row IX=\"1\"><Cell N=\"Font\" V=\"Calibri\"/><Cell N=\"Size\" V=\"0.25\"/>\
         <Cell N=\"Style\" V=\"1\"/><Cell N=\"Color\" V=\"#C0392B\"/></Row>\
         </Section><Section N=\"Paragraph\"><Row IX=\"0\"><Cell N=\"HorzAlign\" V=\"0\"/>\
         <Cell N=\"SpBefore\" V=\"0.02\"/><Cell N=\"SpAfter\" V=\"0.02\"/></Row></Section>";
    let runs = free_shape_xml(
        1,
        &[
            ("PinX", "4.25"),
            ("PinY", "7.5"),
            ("Width", "3"),
            ("Height", "1.5"),
            ("LocPinX", "1.5"),
            ("LocPinY", "0.75"),
            ("FillPattern", "1"),
            ("FillForegnd", "#F5F7FA"),
            ("VerticalAlign", "0"),
        ],
        &format!(
            "{}{runs_sections}",
            geometry_section_xml(&rect_rows(), false)
        ),
        "<cp IX=\"0\"/>Plain <cp IX=\"1\"/>bold red<cp IX=\"0\"/> and plain again\r\nsecond line",
    );
    let bullet_sections = "<Section N=\"Character\"><Row IX=\"0\"><Cell N=\"Font\" V=\"Calibri\"/>\
         <Cell N=\"Size\" V=\"0.1666666666666667\"/><Cell N=\"Style\" V=\"0\"/>\
         <Cell N=\"Color\" V=\"#1F2A44\"/></Row></Section>\
         <Section N=\"Paragraph\">\
         <Row IX=\"0\"><Cell N=\"Bullet\" V=\"1\"/><Cell N=\"BulletStr\" V=\"• \"/>\
         <Cell N=\"BulletFontSize\" V=\"0.1666666666666667\"/>\
         <Cell N=\"TextPosAfterBullet\" V=\"0.2\"/><Cell N=\"IndFirst\" V=\"-0.2\"/>\
         <Cell N=\"IndLeft\" V=\"0.2\"/></Row>\
         <Row IX=\"1\"><Cell N=\"Bullet\" V=\"3\"/><Cell N=\"BulletStr\" V=\"▪ \"/>\
         <Cell N=\"BulletFontSize\" V=\"0.1666666666666667\"/>\
         <Cell N=\"TextPosAfterBullet\" V=\"0.2\"/><Cell N=\"IndFirst\" V=\"-0.2\"/>\
         <Cell N=\"IndLeft\" V=\"0.4\"/></Row>\
         <Row IX=\"2\"><Cell N=\"Bullet\" V=\"0\"/><Cell N=\"HorzAlign\" V=\"1\"/></Row>\
         </Section><Section N=\"Tabs\">\
         <Row IX=\"0\"><Cell N=\"Position\" V=\"1\"/></Row>\
         <Row IX=\"1\"><Cell N=\"Position\" V=\"2\"/></Row></Section>";
    let bullets = free_shape_xml(
        2,
        &[
            ("PinX", "4.25"),
            ("PinY", "4.5"),
            ("Width", "4"),
            ("Height", "2"),
            ("LocPinX", "2"),
            ("LocPinY", "1"),
            ("FillPattern", "1"),
            ("FillForegnd", "#F5F7FA"),
            ("VerticalAlign", "1"),
        ],
        &format!(
            "{}{bullet_sections}",
            geometry_section_xml(&rect_rows(), false)
        ),
        "<pp IX=\"0\"/>first bullet<pp IX=\"1\"/>nested bullet<pp IX=\"2\"/>plain <tp IX=\"0\"/>after tab",
    );
    format!("{runs}{bullets}")
}

/// The instance shape of the master-inheritance fixture: no geometry of
/// its own (the master's rows draw it).
fn master_instance_shape() -> String {
    free_shape_xml(
        2,
        &[
            ("PinX", "4"),
            ("PinY", "6"),
            ("Width", "2"),
            ("Height", "1"),
            ("LocPinX", "1"),
            ("LocPinY", "0.5"),
            ("FillPattern", "1"),
            ("FillForegnd", "#3366CC"),
        ],
        "",
        "",
    )
    .replace("<Shape ID=\"2\"", "<Shape ID=\"2\" Master=\"1\"")
}

/// Shapes whose colours and fonts resolve through the theme part.
fn theme_shapes() -> String {
    let themed_fill = rect_xml(
        1,
        ("2", "7"),
        ("2", "1.5"),
        &[
            ("FillPattern", "1"),
            ("FillForegnd", "Themed"),
            ("QuickStyleFillColor", "2"),
            ("LineColor", "Themed"),
            ("QuickStyleLineColor", "3"),
            ("LineWeight", "0.03"),
            ("ShapeShdwShow", "1"),
            ("ShdwPattern", "1"),
            ("ShdwForegnd", "Themed"),
            ("QuickStyleShadowColor", "100"),
            ("ShapeShdwOffsetX", "0.15"),
            ("ShapeShdwOffsetY", "-0.15"),
        ],
    );
    let variation = rect_xml(
        2,
        ("6", "7"),
        ("2", "1.5"),
        &[
            ("FillPattern", "1"),
            ("FillForegnd", "Themed"),
            ("QuickStyleFillColor", "100"),
            ("VariationColorIndex", "0"),
        ],
    );
    let themed_text = free_shape_xml(
        3,
        &[
            ("PinX", "4.25"),
            ("PinY", "4"),
            ("Width", "4"),
            ("Height", "1"),
            ("LocPinX", "2"),
            ("LocPinY", "0.5"),
        ],
        &format!(
            "{}<Section N=\"Character\"><Row IX=\"0\"><Cell N=\"Color\" V=\"Themed\"/>\
             <Cell N=\"QuickStyleFontColor\" V=\"0\"/><Cell N=\"Size\" V=\"0.3333333333333333\"/>\
             </Row></Section>",
            geometry_section_xml(&rect_rows(), false)
        ),
        "Theme text",
    );
    format!("{themed_fill}{variation}{themed_text}")
}

/// Shapes on a visible layer, a hidden layer and no layer at all.
fn layers_shapes() -> String {
    let shown = rect_xml(
        1,
        ("2.5", "7"),
        ("2", "1.5"),
        &[
            ("FillPattern", "1"),
            ("FillForegnd", "#27AE60"),
            ("LayerMember", "0"),
        ],
    );
    let hidden = rect_xml(
        2,
        ("6", "7"),
        ("2", "1.5"),
        &[
            ("FillPattern", "1"),
            ("FillForegnd", "#C0392B"),
            ("LayerMember", "1"),
        ],
    );
    let unlayered = rect_xml(
        3,
        ("4.25", "3.5"),
        ("2", "1.5"),
        &[("FillPattern", "1"), ("FillForegnd", "#2980B9")],
    );
    format!("{shown}{hidden}{unlayered}")
}

/// A DrawingML theme of our own values: a colour scheme, one variation
/// colour scheme and a font scheme ([MS-VSDX] `/visio/theme/theme{N}.xml`;
/// every RGB value and font name here is chosen by us).
fn theme_xml() -> String {
    let scheme =
        |name: &str, value: &str| format!("<a:{name}><a:srgbClr val=\"{value}\"/></a:{name}>");
    let var_color = |n: u32, value: &str| {
        format!("<vt:varColor{n}><a:srgbClr val=\"{value}\"/></vt:varColor{n}>")
    };
    format!(
        "{XML_DECL}\n<a:theme xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" \
         xmlns:vt=\"urn:larrygram:visio-theme\" name=\"Larrygram Test Theme\"><a:themeElements>\
         <a:clrScheme name=\"Larrygram\">{}{}{}{}{}{}{}{}{}{}{}{}\
         <a:extLst><a:ext uri=\"urn:larrygram:theme-ext\"><vt:bkgnd>\
         <a:srgbClr val=\"FFFFFF\"/></vt:bkgnd></a:ext>\
         <a:ext uri=\"urn:larrygram:theme-ext\"><vt:variationClrSchemeLst>\
         <vt:variationClrScheme>{}{}{}{}{}{}{}</vt:variationClrScheme>\
         </vt:variationClrSchemeLst></a:ext></a:extLst></a:clrScheme>\
         <a:fontScheme name=\"Larrygram\"><a:majorFont><a:latin typeface=\"Selawik\"/>\
         <a:ea typeface=\"\"/><a:cs typeface=\"\"/></a:majorFont>\
         <a:minorFont><a:latin typeface=\"Carlito\"/><a:ea typeface=\"\"/>\
         <a:cs typeface=\"\"/></a:minorFont></a:fontScheme></a:themeElements></a:theme>",
        scheme("dk1", "1F2A44"),
        scheme("lt1", "F5F7FA"),
        scheme("dk2", "2E4A6B"),
        scheme("lt2", "D9E2EC"),
        scheme("accent1", "C0392B"),
        scheme("accent2", "27AE60"),
        scheme("accent3", "2980B9"),
        scheme("accent4", "F39C12"),
        scheme("accent5", "8E44AD"),
        scheme("accent6", "16A085"),
        scheme("hlink", "0B5394"),
        scheme("folHlink", "6C3483"),
        var_color(1, "34495E"),
        var_color(2, "E74C3C"),
        var_color(3, "1ABC9C"),
        var_color(4, "F1C40F"),
        var_color(5, "9B59B6"),
        var_color(6, "2ECC71"),
        var_color(7, "D35400"),
    )
}
