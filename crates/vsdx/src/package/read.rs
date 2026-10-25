//! Reading a zip container into a [`Package`] (task P2-05).

use std::io::{Cursor, Read};

use larrygram_model::{Preserved, XmlChild, XmlNode};
use zip::ZipArchive;

use super::xml::parse_xml;
use super::{
    ContentType, ContentTypeDecl, ContentTypeKind, Limits, Package, Part, PartData, PartName,
    Relationship, RelationshipId, TargetMode,
};
use crate::Error;

/// The one part every package must have, at the package root.
const CONTENT_TYPES_PART: &str = "/[Content_Types].xml";

/// The content type of a relationships part.
const RELATIONSHIPS_CONTENT_TYPE: &str = "application/vnd.openxmlformats-package.relationships+xml";

/// Reads an OPC package from the bytes of a `.vsdx`, `.vssx` or `.vstx` file.
///
/// Every [`Limits`] cap is enforced while reading: entry count, per-part and
/// total uncompressed size, and the compression ratio (ADR 0017). The
/// compression ratio is checked against the declared sizes before inflating,
/// and the inflate loop itself is capped at `max_part_uncompressed`, so a zip
/// bomb is refused without allocating its payload. XML caps (`max_xml_depth`,
/// `max_attributes_per_element`) apply while each XML part is parsed.
///
/// `[Content_Types].xml` and every relationships part are parsed into
/// [`Package::content_types`] and [`Package::relationships`]; every other part
/// becomes a [`Part`]. A missing content-type declaration for a part is an
/// [`Error::InvalidPackage`]; a relationship whose target part does not exist
/// is recorded, not an error (internal targets can legitimately dangle).
///
/// Some real-world packages omit `[Content_Types].xml` entirely (two corpus
/// files do). In that case the declarations are inferred from the part names
/// (`xml` parts are `application/xml`, anything else is
/// `application/octet-stream`) and recorded in [`Package::content_types`], so
/// the written package declares what the reader inferred. The strict rule
/// above applies whenever the part exists.
///
/// An XML part whose text is empty or whitespace (a page contents part can be
/// zero bytes, task P8-19) holds no document: it is kept as its raw bytes and
/// the mapping degrades to no content, like a missing part.
///
/// Malformed input returns an [`Error`], never a panic.
pub fn read_package(bytes: &[u8], limits: &Limits) -> Result<Package, Error> {
    let mut archive =
        ZipArchive::new(Cursor::new(bytes)).map_err(|_| invalid("zip container is malformed"))?;

    let entry_count = archive.len();
    if entry_count > limits.max_entries as usize {
        return Err(Error::LimitExceeded {
            limit: "max_entries",
            value: entry_count as u64,
            max: u64::from(limits.max_entries),
        });
    }

    // Pass 1: read every entry under the size caps, before any parsing.
    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
    let mut total_uncompressed: u64 = 0;
    for index in 0..entry_count {
        let mut file = archive
            .by_index(index)
            .map_err(|_| invalid("zip entry is malformed"))?;
        if file.is_dir() {
            continue;
        }
        let raw_name = file
            .name()
            .map_err(|_| invalid("zip entry name is malformed"))?;
        let name = normalize_name(&raw_name);
        let declared = file.size();
        if declared > limits.max_part_uncompressed {
            return Err(Error::LimitExceeded {
                limit: "max_part_uncompressed",
                value: declared,
                max: limits.max_part_uncompressed,
            });
        }
        // Zip-bomb guard: compare the declared sizes before inflating.
        let ratio_cap = file
            .compressed_size()
            .saturating_mul(limits.max_compression_ratio);
        if declared > ratio_cap {
            return Err(Error::LimitExceeded {
                limit: "max_compression_ratio",
                value: declared,
                max: ratio_cap,
            });
        }
        // Cap the inflate loop too, in case the declared size lies.
        let cap = limits.max_part_uncompressed.saturating_add(1);
        let mut data = Vec::new();
        let mut limited = (&mut file).take(cap);
        limited
            .read_to_end(&mut data)
            .map_err(|_| invalid("zip entry data is malformed"))?;
        let read = data.len() as u64;
        if read > limits.max_part_uncompressed {
            return Err(Error::LimitExceeded {
                limit: "max_part_uncompressed",
                value: read,
                max: limits.max_part_uncompressed,
            });
        }
        total_uncompressed = total_uncompressed.saturating_add(read);
        if total_uncompressed > limits.max_total_uncompressed {
            return Err(Error::LimitExceeded {
                limit: "max_total_uncompressed",
                value: total_uncompressed,
                max: limits.max_total_uncompressed,
            });
        }
        entries.push((name, data));
    }

    // `[Content_Types].xml` first: it declares every other part's type. A
    // package without it (seen in the wild) gets inferred declarations.
    let types_index = entries
        .iter()
        .position(|(name, _)| name == CONTENT_TYPES_PART);
    let (has_types_part, content_types) = match types_index {
        Some(index) => {
            let (_, types_bytes) = entries.remove(index);
            let types_text = std::str::from_utf8(&types_bytes)
                .map_err(|_| invalid("[Content_Types].xml is not UTF-8"))?;
            let types_root =
                parse_xml(types_text, limits).map_err(|e| with_part_name(e, CONTENT_TYPES_PART))?;
            (true, parse_content_types(&types_root)?)
        }
        None => (false, infer_content_types(&entries)),
    };

    // Pass 2: classify each entry by its declared content type. A
    // relationships part is recognized by its content type, or by its name
    // when no content type is declared (it is package metadata, like
    // `[Content_Types].xml`, which is never declared either).
    let mut package = Package::new();
    package.content_types = content_types;
    for (name, data) in entries {
        let declared = declared_content_type(&name, &package.content_types);
        let is_relationships = declared.as_deref() == Some(RELATIONSHIPS_CONTENT_TYPE)
            || (declared.is_none() && rels_source(&name).is_some());
        if is_relationships {
            let source = rels_source(&name)
                .ok_or_else(|| invalid("relationships part name is malformed"))?;
            let text = std::str::from_utf8(&data)
                .map_err(|_| invalid("relationships part is not UTF-8"))
                .map_err(|e| with_part_name(e, &name))?;
            let root = parse_xml(text, limits).map_err(|e| with_part_name(e, &name))?;
            let source = PartName::new(source);
            package
                .relationships
                .extend(parse_relationships(&root, &source)?);
        } else {
            let content_type = match declared {
                Some(content_type) => content_type,
                // Only a package without `[Content_Types].xml` can get here:
                // the inferred declarations cover every part otherwise.
                None if !has_types_part => inferred_content_type(&name),
                None => {
                    return Err(Error::InvalidPackage(format!(
                        "no content type is declared for part `{name}`"
                    )));
                }
            };
            if is_xml_content_type(&content_type) {
                let text = std::str::from_utf8(&data)
                    .map_err(|_| invalid("XML part is not UTF-8"))
                    .map_err(|e| with_part_name(e, &name))?;
                if text.trim().is_empty() {
                    // A part with no document at all (a page contents part
                    // can be zero bytes, task P8-19 cause 4): its raw bytes
                    // are kept and the mapping degrades to no content, like
                    // a missing part.
                    package.parts.push(Part {
                        name: PartName::new(name),
                        content_type: ContentType::new(content_type),
                        data: PartData::Binary(data),
                    });
                    continue;
                }
                let root = parse_xml(text, limits).map_err(|e| with_part_name(e, &name))?;
                package.parts.push(Part {
                    name: PartName::new(name),
                    content_type: ContentType::new(content_type),
                    data: PartData::Xml(root),
                });
            } else {
                package.parts.push(Part {
                    name: PartName::new(name),
                    content_type: ContentType::new(content_type),
                    data: PartData::Binary(data),
                });
            }
        }
    }
    Ok(package)
}

/// Builds an [`Error::InvalidPackage`] with a fixed, content-free message.
pub(crate) fn invalid(why: &str) -> Error {
    Error::InvalidPackage(why.to_owned())
}

/// Adds the part name to an invalid-package error, for context.
pub(crate) fn with_part_name(error: Error, name: &str) -> Error {
    match error {
        Error::InvalidPackage(why) => Error::InvalidPackage(format!("{why} in part `{name}`")),
        other => other,
    }
}

/// Normalizes a zip entry or declaration name to an absolute part name.
pub(crate) fn normalize_name(raw: &str) -> String {
    let separators = raw.replace('\\', "/");
    let trimmed = separators.trim_start_matches('/');
    format!("/{trimmed}")
}

/// The source part of a relationships part name, e.g.
/// `"/visio/_rels/document.xml.rels"` maps to `"/visio/document.xml"` and
/// `"/_rels/.rels"` maps to `"/"`.
pub(crate) fn rels_source(name: &str) -> Option<String> {
    let trimmed = name.trim_start_matches('/');
    let stripped = trimmed.strip_suffix(".rels")?;
    let (directory, file) = stripped.rsplit_once('/')?;
    if directory == "_rels" {
        if file == "." {
            return Some("/".to_owned());
        }
        return Some(format!("/{file}"));
    }
    let parent = directory.strip_suffix("/_rels")?;
    if parent.is_empty() {
        Some(format!("/{file}"))
    } else {
        Some(format!("/{parent}/{file}"))
    }
}

/// Whether a content type marks an XML part (`[Content_Types].xml` and
/// relationships parts are XML too, but are handled separately).
pub(crate) fn is_xml_content_type(content_type: &str) -> bool {
    content_type == "application/xml"
        || content_type == "text/xml"
        || content_type.ends_with("+xml")
}

/// The file extension of a part name, used for `Default` declarations.
pub(crate) fn part_extension(name: &str) -> Option<&str> {
    let (_, extension) = name.rsplit_once('.')?;
    if extension.contains('/') {
        return None;
    }
    Some(extension)
}

/// The content type declared for a part name, by `Override` then `Default`.
pub(crate) fn declared_content_type(name: &str, decls: &[ContentTypeDecl]) -> Option<String> {
    for decl in decls {
        if decl.kind == ContentTypeKind::Override && normalize_name(&decl.name) == name {
            return Some(decl.content_type.clone());
        }
    }
    let extension = part_extension(name)?;
    for decl in decls {
        if decl.kind == ContentTypeKind::Default && decl.name.eq_ignore_ascii_case(extension) {
            return Some(decl.content_type.clone());
        }
    }
    None
}

/// The content type inferred for a part when nothing declares it: `xml` parts
/// are XML, anything else is opaque binary.
pub(crate) fn inferred_content_type(name: &str) -> String {
    match part_extension(name) {
        Some(extension) if extension.eq_ignore_ascii_case("xml") => "application/xml".to_owned(),
        _ => "application/octet-stream".to_owned(),
    }
}

/// Infers content-type declarations for a package without `[Content_Types].xml`.
///
/// One `Default` per extension, in first-appearance order, plus an `Override`
/// for a part without a usable extension. The result round-trips: re-reading
/// the written `[Content_Types].xml` yields the same list.
pub(crate) fn infer_content_types(entries: &[(String, Vec<u8>)]) -> Vec<ContentTypeDecl> {
    let mut decls: Vec<ContentTypeDecl> = Vec::new();
    for (name, _) in entries {
        if rels_source(name).is_some() {
            continue;
        }
        match part_extension(name) {
            Some(extension) => {
                let known = decls.iter().any(|d| {
                    d.kind == ContentTypeKind::Default && d.name.eq_ignore_ascii_case(extension)
                });
                if !known {
                    decls.push(ContentTypeDecl {
                        kind: ContentTypeKind::Default,
                        name: extension.to_owned(),
                        content_type: inferred_content_type(name),
                    });
                }
            }
            None => {
                let known = decls
                    .iter()
                    .any(|d| d.kind == ContentTypeKind::Override && d.name == *name);
                if !known {
                    decls.push(ContentTypeDecl {
                        kind: ContentTypeKind::Override,
                        name: name.clone(),
                        content_type: inferred_content_type(name),
                    });
                }
            }
        }
    }
    decls
}

/// Parses the `[Content_Types].xml` tree into its declarations.
pub(crate) fn parse_content_types(root: &XmlNode) -> Result<Vec<ContentTypeDecl>, Error> {
    if root.name.local != "Types" {
        return Err(invalid("[Content_Types].xml root element is not `Types`"));
    }
    let mut decls = Vec::new();
    for child in &root.children {
        let element = match child {
            XmlChild::Element(element) => element,
            _ => continue,
        };
        let kind = match element.name.local.as_str() {
            "Default" => ContentTypeKind::Default,
            "Override" => ContentTypeKind::Override,
            _ => return Err(invalid("[Content_Types].xml has an unknown element")),
        };
        let (name_attr, type_attr) = match kind {
            ContentTypeKind::Default => ("Extension", "ContentType"),
            _ => ("PartName", "ContentType"),
        };
        let name = required_attribute(element, name_attr)?;
        let content_type = required_attribute(element, type_attr)?;
        decls.push(ContentTypeDecl {
            kind,
            name: name.to_owned(),
            content_type: content_type.to_owned(),
        });
    }
    Ok(decls)
}

/// Parses one relationships part tree into its relationship entries.
pub(crate) fn parse_relationships(
    root: &XmlNode,
    source: &PartName,
) -> Result<Vec<Relationship>, Error> {
    if root.name.local != "Relationships" {
        return Err(invalid(
            "relationships part root element is not `Relationships`",
        ));
    }
    let mut relationships = Vec::new();
    for child in &root.children {
        let element = match child {
            XmlChild::Element(element) => element,
            _ => continue,
        };
        if element.name.local != "Relationship" {
            return Err(invalid("relationships part has an unknown element"));
        }
        let id = required_attribute(element, "Id")?;
        let rel_type = required_attribute(element, "Type")?;
        let target = required_attribute(element, "Target")?;
        let target_mode = match find_attribute(element, "TargetMode") {
            None | Some("Internal") => TargetMode::Internal,
            Some("External") => TargetMode::External,
            Some(_) => return Err(invalid("relationship has a bad `TargetMode`")),
        };
        let preserved = Preserved {
            attributes: element
                .attributes
                .iter()
                .filter(|a| !is_relationship_field(&a.name.local))
                .cloned()
                .collect(),
            children: element
                .children
                .iter()
                .filter_map(|child| match child {
                    XmlChild::Element(element) => Some(element.clone()),
                    _ => None,
                })
                .collect(),
        };
        relationships.push(Relationship {
            source: source.clone(),
            id: RelationshipId::new(id),
            rel_type: rel_type.to_owned(),
            target: target.to_owned(),
            target_mode,
            preserved,
        });
    }
    Ok(relationships)
}

/// Whether an attribute name is one of the mapped relationship attributes.
fn is_relationship_field(local: &str) -> bool {
    matches!(local, "Id" | "Type" | "Target" | "TargetMode")
}

/// The value of a mapped attribute, or an error if it is absent.
fn required_attribute<'n>(node: &'n XmlNode, name: &str) -> Result<&'n str, Error> {
    find_attribute(node, name)
        .ok_or_else(|| invalid("mapped element is missing a required attribute"))
}

/// The value of an unprefixed attribute, if present.
fn find_attribute<'n>(node: &'n XmlNode, name: &str) -> Option<&'n str> {
    node.attributes
        .iter()
        .find(|a| a.name.prefix.is_none() && a.name.local == name)
        .map(|a| a.value.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::{ContentTypeDecl, ContentTypeKind, Limits, PartName};

    #[test]
    fn normalize_name_is_absolute_with_forward_slashes() {
        assert_eq!(normalize_name("visio/document.xml"), "/visio/document.xml");
        assert_eq!(
            normalize_name("\\visio\\document.xml"),
            "/visio/document.xml"
        );
        assert_eq!(normalize_name("/visio/document.xml"), "/visio/document.xml");
    }

    #[test]
    fn rels_source_maps_rels_names_to_their_sources() {
        assert_eq!(rels_source("/_rels/.rels"), Some("/".to_owned()));
        assert_eq!(
            rels_source("/visio/_rels/document.xml.rels"),
            Some("/visio/document.xml".to_owned())
        );
        assert_eq!(
            rels_source("/_rels/page.xml.rels"),
            Some("/page.xml".to_owned())
        );
        assert_eq!(rels_source("/visio/document.xml"), None);
        assert_eq!(rels_source("/_rels/sub/dir/x.rels"), None);
    }

    #[test]
    fn xml_content_types_cover_plain_and_plus_xml() {
        assert!(is_xml_content_type("application/xml"));
        assert!(is_xml_content_type("text/xml"));
        assert!(is_xml_content_type("application/vnd.ms-visio.pages+xml"));
        assert!(!is_xml_content_type("image/png"));
    }

    #[test]
    fn declared_content_type_prefers_override_then_default() {
        let decls = vec![
            ContentTypeDecl {
                kind: ContentTypeKind::Default,
                name: "xml".to_owned(),
                content_type: "application/xml".to_owned(),
            },
            ContentTypeDecl {
                kind: ContentTypeKind::Override,
                name: "/visio/document.xml".to_owned(),
                content_type: "special/xml".to_owned(),
            },
        ];
        assert_eq!(
            declared_content_type("/visio/document.xml", &decls),
            Some("special/xml".to_owned())
        );
        assert_eq!(
            declared_content_type("/visio/pages/pages.xml", &decls),
            Some("application/xml".to_owned())
        );
        assert_eq!(declared_content_type("/visio/image", &decls), None);
        assert_eq!(declared_content_type("/a.b/data", &decls), None);
    }

    #[test]
    fn empty_input_is_an_error_not_a_panic() {
        assert!(matches!(
            read_package(&[], &Limits::DEFAULT),
            Err(Error::InvalidPackage(_))
        ));
    }

    #[test]
    fn inferred_content_types_cover_xml_and_binary() {
        assert_eq!(inferred_content_type("/a/document.xml"), "application/xml");
        assert_eq!(inferred_content_type("/a/document.XML"), "application/xml");
        assert_eq!(
            inferred_content_type("/a/image.emf"),
            "application/octet-stream"
        );
        assert_eq!(
            inferred_content_type("/noextension"),
            "application/octet-stream"
        );
    }

    #[test]
    fn infer_content_types_skips_relationships_and_dedupes() {
        let entries = vec![
            ("/_rels/.rels".to_owned(), Vec::new()),
            ("/visio/document.xml".to_owned(), Vec::new()),
            ("/visio/pages/page1.xml".to_owned(), Vec::new()),
            ("/image.emf".to_owned(), Vec::new()),
            ("/weird".to_owned(), Vec::new()),
        ];
        let decls = infer_content_types(&entries);
        assert_eq!(decls.len(), 3);
        assert_eq!(decls[0].name, "xml");
        assert_eq!(decls[0].content_type, "application/xml");
        assert_eq!(decls[1].name, "emf");
        assert_eq!(decls[2].kind, ContentTypeKind::Override);
        assert_eq!(decls[2].name, "/weird");
    }

    #[test]
    fn part_extension_finds_the_last_segment_extension() {
        assert_eq!(part_extension("/visio/document.xml"), Some("xml"));
        assert_eq!(part_extension("/visio/2.5/data"), None);
        assert_eq!(part_extension("/noextension"), None);
    }

    #[test]
    fn non_element_children_of_content_types_are_skipped() {
        // Coverage (audit minor 2): a comment, a processing instruction or
        // pretty-printing text next to a declaration is not data.
        let text = "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><!--note--> <?pi d?> <Default Extension=\"xml\" ContentType=\"application/xml\"/> </Types>";
        let root = parse_xml(text, &Limits::DEFAULT).expect("parses");
        let decls = parse_content_types(&root).expect("non-element children are skipped");
        assert_eq!(decls.len(), 1);
        assert_eq!(decls[0].name, "xml");
    }

    #[test]
    fn an_unknown_element_in_content_types_is_an_error() {
        // Coverage (audit minor 2): an element that is neither `Default` nor
        // `Override` is rejected (the BadContentTypes fixture truncates the
        // file instead and never reaches this arm).
        let text = "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Nope/></Types>";
        let root = parse_xml(text, &Limits::DEFAULT).expect("parses");
        assert!(matches!(
            parse_content_types(&root),
            Err(Error::InvalidPackage(_))
        ));
    }

    #[test]
    fn parse_relationships_skips_non_element_children() {
        // Mixed content in a relationships entry: the text between the
        // `Relationship` elements is preserved as a non-element child and
        // skipped, never an error.
        let text = concat!(
            "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">",
            "<Relationship Id=\"rId1\" Type=\"t\" Target=\"a.xml\"/>",
            "junk",
            "<Relationship Id=\"rId2\" Type=\"t\" Target=\"b.xml\"/>",
            "</Relationships>"
        );
        let root = parse_xml(text, &Limits::default()).expect("parses");
        let source = PartName::new("/a.xml");
        let relationships = parse_relationships(&root, &source).expect("relationships");
        assert_eq!(relationships.len(), 2);
        assert_eq!(relationships[0].id.as_str(), "rId1");
        assert_eq!(relationships[1].id.as_str(), "rId2");
    }
}
