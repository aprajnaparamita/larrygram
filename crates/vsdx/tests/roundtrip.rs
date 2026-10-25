//! The Phase 2 exit criterion: every corpus `.vsdx` file round-trips through
//! the writer without loss (task P2-07, ADR 0012).
//!
//! For each corpus file: read, write, re-read and re-write. The first write
//! must be semantically equivalent to the source (`diff` empty on every XML
//! part, same part list, content types and relationships, byte-equal binary
//! parts) and the second write must be byte-identical to the first (writer
//! stability). The one difference the criterion allows is the windows part
//! and its entries, which the writer adds to a source that has none (task
//! P8-20, ADR 0073). The corpus is fetched, never committed (ADR 0005): the
//! tests skip with a message when `corpus/vsdx/` is absent, and fail instead
//! when `LARRYGRAM_REQUIRE_CORPUS=1` is set.

use std::path::{Path, PathBuf};

use larrygram_model::{XmlNode, diff};
use larrygram_vsdx::{
    ContentTypeDecl, ContentTypeKind, Limits, Package, PartData, Relationship, read_package,
    to_model, write_document,
};

/// The corpus directory with the real `.vsdx` packages.
fn corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("corpus")
        .join("vsdx")
}

/// The `.vsdx` files of the corpus, sorted for a stable order.
fn corpus_files() -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = match std::fs::read_dir(corpus_dir()) {
        Ok(entries) => entries
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "vsdx"))
            .collect(),
        Err(_) => Vec::new(),
    };
    files.sort();
    files
}

/// Returns `true` when no corpus is available and the run may skip.
#[allow(clippy::panic)] // test helper: a missing corpus must be loud here
fn corpus_missing(files: &[PathBuf]) -> bool {
    if files.is_empty() {
        if std::env::var_os("LARRYGRAM_REQUIRE_CORPUS").is_some_and(|v| v == "1") {
            panic!("corpus/vsdx is absent and LARRYGRAM_REQUIRE_CORPUS=1");
        }
        eprintln!("skipping: corpus/vsdx is absent; run `cargo xtask corpus`");
    }
    files.is_empty()
}

/// The paths of every XML difference between two parts, empty when the parts
/// are equivalent (ADR 0012).
fn part_differences(a: &XmlNode, b: &XmlNode) -> Vec<String> {
    diff(a, b)
        .unwrap_or_default()
        .into_iter()
        .map(|difference| difference.to_string())
        .collect()
}

/// The display name of a root, for failure messages.
fn root_name(node: &XmlNode) -> String {
    match &node.name.prefix {
        Some(prefix) => format!("{}:{}", prefix, node.name.local),
        None => node.name.local.clone(),
    }
}

/// The windows part the writer adds to a package that has none (task
/// P8-20, ADR 0073).
const WINDOWS_PART: &str = "/visio/windows.xml";

/// Whether a content-type declaration names the windows part.
fn is_windows_decl(decl: &ContentTypeDecl) -> bool {
    decl.kind == ContentTypeKind::Override
        && decl.name.trim_start_matches('/') == WINDOWS_PART.trim_start_matches('/')
}

/// Whether a relationship is the main part's windows relationship.
fn is_windows_relationship(relationship: &Relationship) -> bool {
    relationship.source.as_str() == "/visio/document.xml"
        && relationship.rel_type.ends_with("/windows")
}

/// The part names, without the windows part the writer adds when
/// `skip_windows` is set.
fn part_names(package: &Package, skip_windows: bool) -> Vec<&str> {
    package
        .parts
        .iter()
        .map(|part| part.name.as_str())
        .filter(|part| !(skip_windows && *part == WINDOWS_PART))
        .collect()
}

/// The content-type declarations, without the windows entry the writer adds
/// when `skip_windows` is set.
fn content_types(package: &Package, skip_windows: bool) -> Vec<&ContentTypeDecl> {
    package
        .content_types
        .iter()
        .filter(|decl| !(skip_windows && is_windows_decl(decl)))
        .collect()
}

/// The relationships, without the windows entry the writer adds when
/// `skip_windows` is set.
fn relationships(package: &Package, skip_windows: bool) -> Vec<&Relationship> {
    package
        .relationships
        .iter()
        .filter(|relationship| !(skip_windows && is_windows_relationship(relationship)))
        .collect()
}

/// Round-trips one package: read, write, re-read, re-write.
///
/// Asserts semantic equivalence of the first write with the source and
/// byte-identity of the two writes. Prints the difference paths on failure.
/// The written package adds the windows part and its entries to a source
/// that has none (task P8-20); the criterion compares everything else.
#[allow(clippy::panic)] // test helper: the assertions report the failing file
fn round_trip(name: &str, bytes: &[u8]) {
    let source = read_package(bytes, &Limits::DEFAULT).unwrap_or_else(|error| {
        panic!("{name}: source package does not read: {error}");
    });
    let document = to_model(&source).unwrap_or_else(|error| {
        panic!("{name}: source does not map to the model: {error}");
    });
    let first = write_document(&document, &source).unwrap_or_else(|error| {
        panic!("{name}: write failed: {error}");
    });
    let reparsed = read_package(&first, &Limits::DEFAULT).unwrap_or_else(|error| {
        panic!("{name}: written package does not read: {error}");
    });
    let remodeled = to_model(&reparsed).unwrap_or_else(|error| {
        panic!("{name}: written package does not map: {error}");
    });
    let second = write_document(&remodeled, &reparsed).unwrap_or_else(|error| {
        panic!("{name}: second write failed: {error}");
    });

    assert_eq!(
        first, second,
        "{name}: the second write differs from the first"
    );

    let added_windows = !source
        .parts
        .iter()
        .any(|part| part.name.as_str() == WINDOWS_PART);
    assert_eq!(
        part_names(&source, added_windows),
        part_names(&reparsed, added_windows),
        "{name}: part lists differ"
    );
    assert_eq!(
        content_types(&source, added_windows),
        content_types(&reparsed, added_windows),
        "{name}: content types differ"
    );
    assert_eq!(
        relationships(&source, added_windows),
        relationships(&reparsed, added_windows),
        "{name}: relationships differ"
    );

    for before in &source.parts {
        let after = reparsed.part(&before.name).unwrap_or_else(|| {
            panic!("{name}: part {} is missing", before.name.as_str());
        });
        assert_eq!(before.name, after.name, "{name}: part names differ");
        assert_eq!(
            before.content_type,
            after.content_type,
            "{name}: part {} has a different content type",
            before.name.as_str()
        );
        match (&before.data, &after.data) {
            (PartData::Xml(a), PartData::Xml(b)) => {
                let differences = part_differences(a, b);
                assert!(
                    differences.is_empty(),
                    "{name}: part {} ({}) differs:\n{}",
                    before.name.as_str(),
                    root_name(a),
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

#[test]
fn every_corpus_file_round_trips() {
    let files = corpus_files();
    if corpus_missing(&files) {
        return;
    }
    for file in &files {
        let name = file.file_name().map_or_else(
            || file.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        let bytes = std::fs::read(file).unwrap_or_else(|error| {
            panic!("{name}: cannot read: {error}");
        });
        round_trip(&name, &bytes);
    }
}
