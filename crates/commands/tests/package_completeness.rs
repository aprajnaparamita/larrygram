//! The "imported `.vsd` saved as `.vsdx`" case of the package-completeness
//! rule (task P8-20 goal 4).
//!
//! `crates/vsdx/tests/package_completeness.rs` asserts the same two rules
//! (ADR 0073) on every other package the writer produces; this one covers
//! a binary import, which that crate cannot reach: `larrygram-vsd-binary`
//! and `larrygram-vsdx` are both L2, and a workspace crate may depend only
//! on strictly lower layers (ADR 0002). The test is here because
//! `larrygram-commands` depends on both and owns the "save as the XML
//! format" path (P8-09).

// The never-crash lints apply to non-test code only (clippy.toml).
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use larrygram_model::{XmlChild, XmlNode};
use larrygram_vsd_binary::{Limits as BinaryLimits, import};
use larrygram_vsdx::{
    ContentTypeKind, Limits, Package, PartData, PartName, TargetMode, read_package, to_model,
    write_document,
};

/// The windows part (task P8-20).
const WINDOWS_PART: &str = "/visio/windows.xml";
/// The windows content type.
const CT_WINDOWS: &str = "application/vnd.ms-visio.windows+xml";
/// The windows relationship type.
const REL_WINDOWS: &str = "http://schemas.microsoft.com/visio/2010/relationships/windows";
/// The Visio XML namespace.
const VISIO_NS: &str = "http://schemas.microsoft.com/office/visio/2012/main";

/// The committed `.vsd` fixture and the fetched `.vsd` corpus, if present.
fn vsd_files() -> Vec<std::path::PathBuf> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus");
    let mut files = vec![root.join("fixtures/minimal-drawing.vsd")];
    if let Ok(entries) = std::fs::read_dir(root.join("vsd")) {
        let mut corpus: Vec<_> = entries
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "vsd"))
            .collect();
        corpus.sort();
        files.extend(corpus);
    }
    files
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

/// Asserts the windows triple (task P8-20): the part, its relationship and
/// its content-type override.
fn assert_windows_triple(name: &str, package: &Package) {
    let part = package
        .part(&PartName::new(WINDOWS_PART))
        .unwrap_or_else(|| panic!("{name}: the saved package has no windows part"));
    assert_eq!(
        part.content_type.as_str(),
        CT_WINDOWS,
        "{name}: content type"
    );
    let PartData::Xml(root) = &part.data else {
        panic!("{name}: the windows part is not XML");
    };
    assert_eq!(root.name.local, "Windows", "{name}: the windows root");
    assert_eq!(
        root.name.uri.as_deref(),
        Some(VISIO_NS),
        "{name}: namespace"
    );
    let main = related_part(package, "/", "document").expect("a main document part");
    let rel = package
        .relationships_from(&main)
        .find(|relationship| {
            relationship.rel_type == REL_WINDOWS && relationship.target_mode == TargetMode::Internal
        })
        .unwrap_or_else(|| panic!("{name}: the saved package has no windows relationship"));
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
        "{name}: the saved package has no windows override"
    );
}

/// Asserts every page a `BackPage` names is marked as a background page.
fn assert_background_pages(name: &str, package: &Package) {
    let main = related_part(package, "/", "document").expect("a main document part");
    let Some(pages) = related_part(package, main.as_str(), "pages") else {
        return;
    };
    let PartData::Xml(root) = &package.part(&pages).expect("pages part").data else {
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
            .unwrap_or_else(|| panic!("{name}: BackPage={back_page} names no page"));
        let marked = target
            .attributes
            .iter()
            .find(|attribute| attribute.name.local == "Background")
            .map(|attribute| attribute.value.as_str());
        assert!(
            matches!(marked, Some("1") | Some("true")),
            "{name}: BackPage={back_page} names a page that is not a background page"
        );
    }
}

#[test]
fn an_imported_vsd_saved_as_vsdx_carries_the_windows_triple() {
    for file in vsd_files() {
        let name = file.file_name().unwrap().to_string_lossy().into_owned();
        let bytes = std::fs::read(&file).unwrap_or_else(|error| panic!("{name}: {error}"));
        // A file the importer refuses (the alpha reads versions 6 and 11)
        // never reaches the writer; the rule covers what it produces.
        let Ok(imported) = import(&bytes, &BinaryLimits::DEFAULT) else {
            continue;
        };
        let saved = write_document(&imported.document, &Package::new()).expect("save");
        let package = read_package(&saved, &Limits::DEFAULT).expect("the save reads back");
        assert_windows_triple(&name, &package);
        assert_background_pages(&name, &package);
    }
}

#[test]
fn an_imported_vsd_round_trips_through_its_save() {
    // The saved file keeps the document the import produced (P8-09) and
    // writes stably (the Phase 2 criterion).
    let mut files = vsd_files();
    let file = files.remove(0);
    let bytes = std::fs::read(&file).expect("fixture");
    let imported = import(&bytes, &BinaryLimits::DEFAULT).expect("import");
    let saved = write_document(&imported.document, &Package::new()).expect("save");
    let package = read_package(&saved, &Limits::DEFAULT).expect("reads back");
    let document = to_model(&package).expect("maps");
    assert_eq!(document.pages.len(), imported.document.pages.len());
    assert_eq!(document.masters.len(), imported.document.masters.len());
    let again = write_document(&document, &package).expect("save again");
    assert_eq!(saved, again, "the second save is byte-identical");
}
