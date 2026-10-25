//! Structural completeness of every package the writer produces (task
//! P8-20).
//!
//! [MS-VSDX] does not list the windows part, but Visio does not open a
//! package without one: three entries are required together (observed
//! behaviour of Visio, AGENTS.md section 4; ADR 0073):
//!
//! 1. the part `/visio/windows.xml`, root `Windows` in the Visio namespace;
//! 2. a relationship from the document part, type `.../relationships/windows`;
//! 3. the content-type override `application/vnd.ms-visio.windows+xml`.
//!
//! And every page a `BackPage` attribute names must be a background page
//! ([MS-VSDX] `Page_Type`: `BackPage` "MUST be equal to the ID attribute of a
//! Page_Type element that has a Background attribute value of TRUE"), so the
//! writer marks what it references. Every case below reads the package the
//! writer produced and asserts both rules; the "imported `.vsd` saved as
//! `.vsdx`" case lives in `crates/commands/tests/package_completeness.rs`
//! (the layering rules keep `larrygram-vsd-binary` out of this crate's
//! tests, ADR 0002).

// The never-crash lints apply to non-test code only (clippy.toml).
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod common;

use common::{COMMITTED_FIXTURES, Feature, Fixture};
use larrygram_model::{Document, DocumentKind, PageId, XmlChild, XmlNode};
use larrygram_vsdx::{
    ContentTypeKind, Limits, Package, Part, PartData, PartName, TargetMode, from_model,
    read_document, read_package, to_model, write_document,
};

/// The windows part (task P8-20).
const WINDOWS_PART: &str = "/visio/windows.xml";
/// The windows content type.
const CT_WINDOWS: &str = "application/vnd.ms-visio.windows+xml";
/// The windows relationship type.
const REL_WINDOWS: &str = "http://schemas.microsoft.com/visio/2010/relationships/windows";
/// The Visio XML namespace.
const VISIO_NS: &str = "http://schemas.microsoft.com/office/visio/2012/main";

/// The well-formed fixtures: everything but the broken-package set.
fn well_formed_fixtures() -> Vec<Fixture> {
    COMMITTED_FIXTURES
        .iter()
        .filter(|fixture| !matches!(fixture, Fixture::Malformed(_)))
        .copied()
        .collect()
}

/// The part a relationship of `rel_local` from `source` names, resolved.
fn related_part(package: &Package, source: &str, rel_local: &str) -> Option<PartName> {
    package
        .relationships
        .iter()
        .filter(|relationship| {
            relationship.source.as_str() == source
                && relationship.target_mode == TargetMode::Internal
                && relationship.rel_type.rsplit('/').next() == Some(rel_local)
        })
        .map(|relationship| resolve_target(source, &relationship.target))
        .find(|name| package.part(name).is_some())
}

/// The absolute part name `target` names from the part `source`.
fn resolve_target(source: &str, target: &str) -> PartName {
    if target.starts_with('/') {
        return PartName::new(target);
    }
    let directory = source
        .rsplit_once('/')
        .map_or("", |(directory, _)| directory);
    PartName::new(format!("{directory}/{target}"))
}

/// The main document part: the target of the root `document` relationship.
fn main_part(package: &Package) -> PartName {
    related_part(package, "/", "document").expect("a main document part")
}

/// The pages list part, if the package has one.
fn pages_part(package: &Package) -> Option<Part> {
    let name = related_part(package, main_part(package).as_str(), "pages")?;
    package.part(&name).cloned()
}

/// The `Background` attribute value of one element, if it has one.
fn background_value(element: &XmlNode) -> Option<&str> {
    element
        .attributes
        .iter()
        .find(|attribute| attribute.name.local == "Background")
        .map(|attribute| attribute.value.as_str())
}

/// Asserts the windows triple: the part, its relationship and its override.
fn assert_windows_triple(name: &str, package: &Package) {
    let part = package
        .part(&PartName::new(WINDOWS_PART))
        .unwrap_or_else(|| panic!("{name}: the package has no windows part"));
    assert_eq!(
        part.content_type.as_str(),
        CT_WINDOWS,
        "{name}: the windows part's content type"
    );
    let PartData::Xml(root) = &part.data else {
        panic!("{name}: the windows part is not XML");
    };
    assert_eq!(root.name.local, "Windows", "{name}: the windows root");
    assert_eq!(
        root.name.uri.as_deref(),
        Some(VISIO_NS),
        "{name}: the windows root namespace"
    );
    let main = main_part(package);
    let rel = package
        .relationships_from(&main)
        .find(|relationship| {
            relationship.rel_type == REL_WINDOWS && relationship.target_mode == TargetMode::Internal
        })
        .unwrap_or_else(|| panic!("{name}: the package has no windows relationship"));
    assert_eq!(
        resolve_target(main.as_str(), &rel.target).as_str(),
        WINDOWS_PART,
        "{name}: the windows relationship target"
    );
    assert!(
        package.content_types.iter().any(|decl| {
            decl.kind == ContentTypeKind::Override
                && decl.name.trim_start_matches('/') == "visio/windows.xml"
                && decl.content_type == CT_WINDOWS
        }),
        "{name}: the package has no windows content-type override"
    );
}

/// Asserts every page a `BackPage` names is marked as a background page.
fn assert_background_pages(name: &str, package: &Package) {
    let Some(pages) = pages_part(package) else {
        return; // a stencil has no page list to check
    };
    let PartData::Xml(root) = &pages.data else {
        panic!("{name}: the pages part is not XML");
    };
    let elements: Vec<&XmlNode> = root
        .children
        .iter()
        .filter_map(|child| match child {
            XmlChild::Element(element) if element.name.local == "Page" => Some(element),
            _ => None,
        })
        .collect();
    for element in &elements {
        let Some(back_page) = element
            .attributes
            .iter()
            .find(|attribute| attribute.name.local == "BackPage")
            .map(|attribute| attribute.value.clone())
        else {
            continue;
        };
        let target = elements
            .iter()
            .find(|candidate| {
                candidate
                    .attributes
                    .iter()
                    .any(|attribute| attribute.name.local == "ID" && attribute.value == back_page)
            })
            .unwrap_or_else(|| panic!("{name}: BackPage={back_page} names no page of the list"));
        assert!(
            matches!(background_value(target), Some("1") | Some("true")),
            "{name}: BackPage={back_page} names a page that is not a background page"
        );
    }
}

/// Asserts both rules of ADR 0073 on one package.
fn assert_complete(name: &str, package: &Package) {
    assert_windows_triple(name, package);
    assert_background_pages(name, package);
}

/// A one-page document with a rectangle, of this kind.
fn sample(kind: DocumentKind) -> Document {
    let mut document = Document::new();
    document.kind = kind;
    let mut page = larrygram_model::Page::new(PageId { index: 0 }, 1);
    page.name = "Page-1".to_owned();
    document.pages.push(page);
    document
}

#[test]
fn a_new_document_carries_the_windows_triple() {
    for kind in [
        DocumentKind::Drawing,
        DocumentKind::Stencil,
        DocumentKind::Template,
    ] {
        let document = sample(kind);
        let generated = from_model(&document).expect("from_model");
        assert_complete("from_model", &generated);
        let bytes = write_document(&document, &Package::new()).expect("write_document");
        let reparsed = read_package(&bytes, &Limits::DEFAULT).expect("reads back");
        assert_complete("write_document", &reparsed);
    }
}

#[test]
fn a_new_document_with_a_background_page_marks_it() {
    let mut document = sample(DocumentKind::Drawing);
    let mut background = larrygram_model::Page::new(PageId { index: 1 }, 2);
    background.name = "Background".to_owned();
    document.pages.push(background);
    document.pages[0].background = Some(PageId { index: 1 });
    for package in [
        from_model(&document).expect("from_model"),
        read_package(
            &write_document(&document, &Package::new()).expect("write"),
            &Limits::DEFAULT,
        )
        .expect("reads back"),
    ] {
        assert_complete("a background link", &package);
    }
}

#[test]
fn every_well_formed_fixture_carries_the_windows_triple() {
    for fixture in well_formed_fixtures() {
        let name = common::file_name(&fixture);
        let package = read_package(&common::build(fixture), &Limits::DEFAULT)
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_complete(name, &package);
    }
}

#[test]
fn every_written_fixture_carries_the_windows_triple() {
    for fixture in well_formed_fixtures() {
        let name = common::file_name(&fixture);
        let bytes = common::build(fixture);
        let source = read_package(&bytes, &Limits::DEFAULT)
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        let document = to_model(&source).unwrap_or_else(|error| panic!("{name}: {error}"));
        let first = write_document(&document, &source).expect("write");
        assert_complete(
            name,
            &read_package(&first, &Limits::DEFAULT).expect("reads back"),
        );
    }
}

#[test]
fn corpus_writes_carry_the_windows_triple() {
    let mut files: Vec<std::path::PathBuf> = match std::fs::read_dir(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/vsdx"),
    ) {
        Ok(entries) => entries
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "vsdx"))
            .collect(),
        Err(_) => Vec::new(),
    };
    files.sort();
    if files.is_empty() {
        eprintln!("skipping: corpus/vsdx is absent; run `cargo xtask corpus`");
        return;
    }
    for file in &files {
        let name = file.file_name().unwrap().to_string_lossy().into_owned();
        let bytes = std::fs::read(file).unwrap_or_else(|error| panic!("{name}: {error}"));
        let source = read_package(&bytes, &Limits::DEFAULT)
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        let mut document = to_model(&source).unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_complete(&name, &source_for(&document, &source));
        // and once more with an edit in the model
        document.add_page("Page-added").expect("add page");
        assert_complete(&name, &source_for(&document, &source));
    }
}

/// The written package of `document` over the preserved `source`.
fn source_for(document: &Document, source: &Package) -> Package {
    let bytes = write_document(document, source).expect("write");
    read_package(&bytes, &Limits::DEFAULT).expect("reads back")
}

#[test]
fn an_existing_windows_part_is_untouched() {
    let bytes = common::build(Fixture::Drawing);
    let source = read_package(&bytes, &Limits::DEFAULT).expect("reads");
    let document = to_model(&source).expect("maps");
    let mut edited = source.clone();
    {
        let part = edited
            .parts
            .iter_mut()
            .find(|part| part.name.as_str() == WINDOWS_PART)
            .expect("windows part");
        let PartData::Xml(root) = &mut part.data else {
            panic!("the windows part is XML");
        };
        root.attributes.push(larrygram_model::XmlAttribute {
            name: larrygram_model::QName::local("ClientWidth"),
            value: "42".to_owned(),
        });
    }
    let untouched = edited
        .part(&PartName::new(WINDOWS_PART))
        .expect("windows part")
        .clone();
    let first = write_document(&document, &edited).expect("write");
    let reparsed = read_package(&first, &Limits::DEFAULT).expect("reads back");
    assert_eq!(
        reparsed.part(&PartName::new(WINDOWS_PART)),
        Some(&untouched),
        "the windows part is kept byte for byte"
    );
    let remodeled = to_model(&reparsed).expect("maps");
    let second = write_document(&remodeled, &reparsed).expect("write again");
    assert_eq!(first, second, "the second write is byte-identical");
}

/// The `Background` value of the page at `position` of the written pages
/// list.
fn written_background(document: &Document, source: &Package, position: usize) -> Option<String> {
    let package = source_for(document, source);
    let pages = pages_part(&package).expect("pages part");
    let PartData::Xml(root) = &pages.data else {
        panic!("the pages part is XML");
    };
    let element = root
        .children
        .iter()
        .filter_map(|child| match child {
            XmlChild::Element(element) if element.name.local == "Page" => Some(element),
            _ => None,
        })
        .nth(position)
        .expect("the page element");
    background_value(element).map(str::to_owned)
}

#[test]
fn a_referenced_page_is_marked_on_write() {
    // The fixture's background page is marked the way Visio does (task
    // P8-20 goal 3); a model built from a `.vsd` import carries only the
    // reference, and the writer marks what it references.
    let bytes = common::build(Fixture::Feature(Feature::BackgroundPages));
    let source = read_package(&bytes, &Limits::DEFAULT).expect("reads");
    let document = to_model(&source).expect("maps");
    assert_eq!(document.pages[0].background, Some(PageId { index: 1 }));
    assert_eq!(
        written_background(&document, &source, 1).as_deref(),
        Some("1"),
        "the referenced page is marked"
    );
}

#[test]
fn a_missing_mark_is_added_and_a_false_one_becomes_one() {
    let bytes = common::build(Fixture::Feature(Feature::BackgroundPages));
    let source = read_package(&bytes, &Limits::DEFAULT).expect("reads");
    let document = to_model(&source).expect("maps");
    for value in [None, Some("0"), Some("true")] {
        let mut edited = source.clone();
        let pages = edited
            .parts
            .iter_mut()
            .find(|part| part.name.as_str() == "/visio/pages/pages.xml")
            .expect("pages part");
        let PartData::Xml(root) = &mut pages.data else {
            panic!("the pages part is XML");
        };
        for child in &mut root.children {
            let XmlChild::Element(element) = child else {
                continue;
            };
            element
                .attributes
                .retain(|attribute| attribute.name.local != "Background");
            if let Some(value) = value {
                element.attributes.push(larrygram_model::XmlAttribute {
                    name: larrygram_model::QName::local("Background"),
                    value: value.to_owned(),
                });
            }
        }
        // The rule marks what it references: `None` and `"0"` become `"1"`,
        // a true spelling is kept (ADR 0073).
        let expected = match value {
            Some("true") => "true",
            _ => "1",
        };
        assert_eq!(
            written_background(&document, &edited, 1).as_deref(),
            Some(expected),
            "the mark of a referenced page is {expected:?}"
        );
    }
}

#[test]
fn a_referenced_pages_mark_is_consumed_on_read() {
    // The reader consumes the writer's derived mark (ADR 0073): the model
    // holds the reference, the writer writes the mark again.
    let bytes = common::build(Fixture::Feature(Feature::BackgroundPages));
    let document = read_document(&bytes, &Limits::DEFAULT).expect("reads");
    assert_eq!(document.pages[0].background, Some(PageId { index: 1 }));
    let referenced = &document.pages[1];
    assert!(
        !referenced
            .preserved
            .attributes
            .iter()
            .any(|attribute| attribute.name.local == "Background"),
        "the derived mark is not preserved"
    );
}

#[test]
fn an_unreferenced_pages_mark_stays_preserved() {
    // A page nobody references keeps its mark verbatim: the model cannot
    // derive it, so it is content (ADR 0073).
    let bytes = common::build(Fixture::Feature(Feature::BackgroundPages));
    let mut source = read_package(&bytes, &Limits::DEFAULT).expect("reads");
    let pages = source
        .parts
        .iter_mut()
        .find(|part| part.name.as_str() == "/visio/pages/pages.xml")
        .expect("pages part");
    let PartData::Xml(root) = &mut pages.data else {
        panic!("the pages part is XML");
    };
    for child in &mut root.children {
        let XmlChild::Element(element) = child else {
            continue;
        };
        element
            .attributes
            .retain(|attribute| attribute.name.local != "BackPage");
    }
    let document = to_model(&source).expect("maps");
    let page = &document.pages[1];
    assert!(
        page.preserved
            .attributes
            .iter()
            .any(|attribute| attribute.name.local == "Background" && attribute.value == "1"),
        "an unreferenced mark is preserved"
    );
}

#[test]
fn a_written_package_without_a_windows_part_gets_the_triple() {
    // The two corpus files without a windows part (`recursion-cycle`,
    // `tab-short-prefix`) open after a save (task P8-20 goal 1): the writer
    // adds the part to a package that has none, on the first write.
    let bytes = common::build(Fixture::Feature(Feature::BackgroundPages));
    let mut source = read_package(&bytes, &Limits::DEFAULT).expect("reads");
    source
        .parts
        .retain(|part| part.name.as_str() != WINDOWS_PART);
    source
        .content_types
        .retain(|decl| decl.name.trim_start_matches('/') != "visio/windows.xml");
    source
        .relationships
        .retain(|relationship| relationship.rel_type != REL_WINDOWS);
    let document = to_model(&source).expect("maps");
    assert_complete("a stripped package", &source_for(&document, &source));
}
