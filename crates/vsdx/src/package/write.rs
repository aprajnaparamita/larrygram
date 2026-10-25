//! Writing a [`Package`] back to a zip container (task P2-05).

use std::io::{Cursor, Write};

use flate2::Compression;
use flate2::write::DeflateEncoder;
use zip::CompressionMethod;
use zip::write::{SimpleFileOptions, ZipWriter};

use super::Limits;

use super::read::{invalid, normalize_name, rels_source};
use super::xml::{escape_attribute, write_qualified_name, write_xml};
use super::{ContentTypeDecl, ContentTypeKind, Package, PartData, Relationship, TargetMode};
use crate::Error;

/// The part name of the content-types part, as a zip entry.
const CONTENT_TYPES_ENTRY: &str = "[Content_Types].xml";

/// The XML declaration every written XML part starts with. `XmlNode` has no
/// slot for the source's declaration, so the writer always emits this one.
const XML_DECLARATION: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n";

/// Serializes a package to the bytes of a `.vsdx`, `.vssx` or `.vstx` file.
///
/// Parts are written in `Package::parts` order, relationships and
/// `[Content_Types].xml` are generated from the package's own lists, so a read
/// followed by a write keeps every part and relationship (ADR 0012). The
/// output is a deterministic function of the package: writing a package read
/// from the output again yields identical bytes.
pub fn write_package(package: &Package) -> Result<Vec<u8>, Error> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let types = content_types_xml(&package.content_types);
    write_entry(&mut writer, CONTENT_TYPES_ENTRY, types.as_bytes())?;

    for part in &package.parts {
        let name = entry_name(&part.name)?;
        match &part.data {
            PartData::Xml(node) => {
                let mut text = String::from(XML_DECLARATION);
                text.push_str(&write_xml(node));
                write_entry(&mut writer, &name, text.as_bytes())?;
            }
            PartData::Binary(data) => write_entry(&mut writer, &name, data)?,
        }
    }

    for (source, relationships) in group_relationships(&package.relationships) {
        let text = relationships_xml(&relationships);
        write_entry(&mut writer, &rels_entry_name(source), text.as_bytes())?;
    }

    let cursor = writer.finish().map_err(|_| invalid("zip writer failed"))?;
    Ok(cursor.into_inner())
}

/// Writes one zip entry with deflate compression, unless deflating would
/// push the entry past the reader's own compression-ratio cap
/// ([`Limits::max_compression_ratio`], ADR 0017): a written package must
/// always read back (task P8-19, two corpus files whose huge, highly
/// compressible parts the writer deflated past the cap and its own reader
/// then refused).
fn write_entry(
    writer: &mut ZipWriter<Cursor<Vec<u8>>>,
    name: &str,
    data: &[u8],
) -> Result<(), Error> {
    let options = SimpleFileOptions::default().compression_method(compression_for(data)?);
    writer
        .start_file(name, options)
        .map_err(|_| invalid("zip writer failed"))?;
    writer
        .write_all(data)
        .map_err(|_| invalid("zip writer failed"))?;
    Ok(())
}

/// The compression method for one entry: `Deflated`, except for an entry
/// whose deflated form the reader's compression-ratio cap refuses (it is
/// stored). The deflated size is measured with the zip writer's own codec
/// and level (flate2 raw deflate, `Compression::default()`), so it is the
/// written size exactly.
fn compression_for(data: &[u8]) -> Result<CompressionMethod, Error> {
    let len = u64::try_from(data.len()).unwrap_or(u64::MAX);
    let ratio = Limits::DEFAULT.max_compression_ratio;
    let compressed = u64::try_from(deflated_size(data)?).unwrap_or(u64::MAX);
    if len > compressed.saturating_mul(ratio) {
        Ok(CompressionMethod::Stored)
    } else {
        Ok(CompressionMethod::Deflated)
    }
}

/// The size of `data` under the writer's own deflate.
fn deflated_size(data: &[u8]) -> Result<usize, Error> {
    let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(data)
        .map_err(|_| invalid("zip writer failed"))?;
    let out = encoder.finish().map_err(|_| invalid("zip writer failed"))?;
    Ok(out.len())
}

/// The zip entry name for a part, rejecting package-metadata names.
fn entry_name(part: &super::PartName) -> Result<String, Error> {
    let name = normalize_name(part.as_str());
    if name == "/[Content_Types].xml" || rels_source(&name).is_some() {
        return Err(invalid("part name is reserved for package metadata"));
    }
    Ok(name.trim_start_matches('/').to_owned())
}

/// The zip entry name of the relationships part for a source part.
fn rels_entry_name(source: &str) -> String {
    if source == "/" {
        return "_rels/.rels".to_owned();
    }
    let trimmed = source.trim_start_matches('/');
    match trimmed.rsplit_once('/') {
        Some((directory, file)) => format!("{directory}/_rels/{file}.rels"),
        None => format!("_rels/{trimmed}.rels"),
    }
}

/// Groups relationships by source part, in first-appearance order.
fn group_relationships(relationships: &[Relationship]) -> Vec<(&str, Vec<&Relationship>)> {
    let mut groups: Vec<(&str, Vec<&Relationship>)> = Vec::new();
    for relationship in relationships {
        let source = relationship.source.as_str();
        match groups.iter_mut().find(|(s, _)| *s == source) {
            Some((_, group)) => group.push(relationship),
            None => groups.push((source, vec![relationship])),
        }
    }
    groups
}

/// Generates `[Content_Types].xml` from the declarations.
fn content_types_xml(decls: &[ContentTypeDecl]) -> String {
    let mut out = String::new();
    out.push_str(XML_DECLARATION);
    out.push_str("<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">");
    for decl in decls {
        let (element, name_attribute) = if decl.kind == ContentTypeKind::Default {
            ("Default", "Extension")
        } else {
            ("Override", "PartName")
        };
        out.push('<');
        out.push_str(element);
        out.push(' ');
        out.push_str(name_attribute);
        out.push_str("=\"");
        escape_attribute(&decl.name, &mut out);
        out.push_str("\" ContentType=\"");
        escape_attribute(&decl.content_type, &mut out);
        out.push_str("\"/>");
    }
    out.push_str("</Types>");
    out
}

/// Generates one relationships part from its entries.
fn relationships_xml(relationships: &[&Relationship]) -> String {
    let mut out = String::new();
    out.push_str(XML_DECLARATION);
    out.push_str(
        "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">",
    );
    for relationship in relationships {
        out.push_str("<Relationship Id=\"");
        escape_attribute(relationship.id.as_str(), &mut out);
        out.push_str("\" Type=\"");
        escape_attribute(&relationship.rel_type, &mut out);
        out.push_str("\" Target=\"");
        escape_attribute(&relationship.target, &mut out);
        out.push('"');
        if relationship.target_mode == TargetMode::External {
            out.push_str(" TargetMode=\"External\"");
        }
        for attribute in &relationship.preserved.attributes {
            out.push(' ');
            write_qualified_name(&attribute.name, &mut out);
            out.push_str("=\"");
            escape_attribute(&attribute.value, &mut out);
            out.push('"');
        }
        if relationship.preserved.children.is_empty() {
            out.push_str("/>");
        } else {
            out.push('>');
            for child in &relationship.preserved.children {
                out.push_str(&write_xml(child));
            }
            out.push_str("</Relationship>");
        }
    }
    out.push_str("</Relationships>");
    out
}

#[cfg(test)]
mod tests {
    use larrygram_model::{Preserved, QName, XmlAttribute, XmlChild, XmlNode};

    use super::*;
    use crate::package::{ContentType, Limits, Part, PartName, RelationshipId};

    fn empty_package() -> Package {
        Package::new()
    }

    #[test]
    fn writes_an_empty_package_with_content_types() {
        let bytes = write_package(&empty_package()).expect("write");
        let package = crate::read_package(&bytes, &Limits::DEFAULT).expect("read");
        assert!(package.parts.is_empty());
        assert!(package.content_types.is_empty());
        assert!(package.relationships.is_empty());
    }

    #[test]
    fn a_highly_compressible_entry_is_stored_for_the_reader_caps() {
        // A part whose deflated form the reader's compression-ratio cap
        // would refuse is stored instead (task P8-19): a written package
        // always reads back, whatever it contains.
        let repetitive = vec![b'x'; 400_000];
        assert_eq!(
            compression_for(&repetitive).expect("method"),
            CompressionMethod::Stored
        );
        let varied = b"<A>0123456789</A>".repeat(64);
        assert_eq!(
            compression_for(&varied).expect("method"),
            CompressionMethod::Deflated
        );
        assert_eq!(
            compression_for(b"").expect("method"),
            CompressionMethod::Deflated
        );
    }

    #[test]
    fn rels_entry_names_match_the_reader() {
        assert_eq!(rels_entry_name("/"), "_rels/.rels");
        assert_eq!(
            rels_entry_name("/visio/document.xml"),
            "visio/_rels/document.xml.rels"
        );
        assert_eq!(rels_entry_name("/page.xml"), "_rels/page.xml.rels");
        assert_eq!(
            crate::package::read::rels_source(&format!(
                "/{}",
                rels_entry_name("/visio/document.xml")
            )),
            Some("/visio/document.xml".to_owned())
        );
    }

    #[test]
    fn reserved_part_names_are_refused() {
        let mut package = empty_package();
        package.parts.push(Part {
            name: PartName::new("/[Content_Types].xml"),
            content_type: ContentType::new("application/xml"),
            data: PartData::Binary(Vec::new()),
        });
        assert!(matches!(
            write_package(&package),
            Err(Error::InvalidPackage(_))
        ));

        let mut package = empty_package();
        package.parts.push(Part {
            name: PartName::new("/visio/_rels/document.xml.rels"),
            content_type: ContentType::new("application/xml"),
            data: PartData::Binary(Vec::new()),
        });
        assert!(matches!(
            write_package(&package),
            Err(Error::InvalidPackage(_))
        ));
    }

    #[test]
    fn parts_and_relationships_round_trip() {
        let mut package = empty_package();
        package.content_types.push(ContentTypeDecl {
            kind: ContentTypeKind::Default,
            name: "xml".to_owned(),
            content_type: "application/xml".to_owned(),
        });
        package.content_types.push(ContentTypeDecl {
            kind: ContentTypeKind::Override,
            name: "/visio/document.xml".to_owned(),
            content_type: "image/png".to_owned(),
        });
        package.parts.push(Part {
            name: PartName::new("/visio/document.xml"),
            content_type: ContentType::new("image/png"),
            data: PartData::Binary(b"not xml".to_vec()),
        });
        package.relationships.push(Relationship {
            source: PartName::new("/"),
            id: RelationshipId::new("rId1"),
            rel_type: "http://rel/officeDocument".to_owned(),
            target: "visio/document.xml".to_owned(),
            target_mode: TargetMode::Internal,
            preserved: Preserved::new(),
        });
        package.relationships.push(Relationship {
            source: PartName::new("/"),
            id: RelationshipId::new("rId2"),
            rel_type: "http://rel/thumbnail".to_owned(),
            target: "http://example.com/image.png".to_owned(),
            target_mode: TargetMode::External,
            preserved: Preserved {
                attributes: vec![XmlAttribute {
                    name: QName::local("Extra"),
                    value: "kept".to_owned(),
                }],
                children: Vec::new(),
            },
        });

        let bytes = write_package(&package).expect("write");
        let reread = crate::read_package(&bytes, &Limits::DEFAULT).expect("read");
        assert_eq!(reread, package);
    }

    #[test]
    fn relationship_children_round_trip() {
        // Review finding 5: an unknown child element of a `Relationship` is
        // preserved content and must survive read -> write -> read.
        let mut package = empty_package();
        package.content_types.push(ContentTypeDecl {
            kind: ContentTypeKind::Override,
            name: "/visio/document.xml".to_owned(),
            content_type: "image/png".to_owned(),
        });
        package.parts.push(Part {
            name: PartName::new("/visio/document.xml"),
            content_type: ContentType::new("image/png"),
            data: PartData::Binary(b"not xml".to_vec()),
        });
        package.relationships.push(Relationship {
            source: PartName::new("/"),
            id: RelationshipId::new("rId1"),
            rel_type: "http://rel/officeDocument".to_owned(),
            target: "visio/document.xml".to_owned(),
            target_mode: TargetMode::Internal,
            preserved: Preserved {
                attributes: Vec::new(),
                children: vec![XmlNode {
                    name: QName {
                        // The relationships part declares its namespace as
                        // the default one, so an unprefixed child takes it.
                        prefix: None,
                        uri: Some(
                            "http://schemas.openxmlformats.org/package/2006/relationships"
                                .to_owned(),
                        ),
                        local: "Extra".to_owned(),
                    },
                    attributes: vec![XmlAttribute {
                        name: QName::local("note"),
                        value: "kept".to_owned(),
                    }],
                    children: vec![XmlChild::Text("text".to_owned())],
                    self_closing: false,
                }],
            },
        });
        let bytes = write_package(&package).expect("write");
        let reread = crate::read_package(&bytes, &Limits::DEFAULT).expect("read");
        assert_eq!(reread, package);
    }

    #[test]
    fn xml_parts_round_trip_with_prefixes_and_whitespace() {
        let mut package = empty_package();
        package.content_types.push(ContentTypeDecl {
            kind: ContentTypeKind::Default,
            name: "xml".to_owned(),
            content_type: "application/xml".to_owned(),
        });
        let source = "<v:Root xmlns:v=\"urn:x\" b=\"2\" a=\"1\">\n  <Child/>\n</v:Root>";
        let tree = crate::package::xml::parse_xml(source, &Limits::DEFAULT).expect("parse");
        package.parts.push(Part {
            name: PartName::new("/visio/document.xml"),
            content_type: ContentType::new("application/xml"),
            data: PartData::Xml(tree),
        });

        let bytes = write_package(&package).expect("write");
        let reread = crate::read_package(&bytes, &Limits::DEFAULT).expect("read");
        assert_eq!(reread, package);

        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).expect("zip");
        let mut text = String::new();
        std::io::Read::read_to_string(
            &mut archive.by_name("visio/document.xml").expect("entry"),
            &mut text,
        )
        .expect("utf-8");
        assert!(text.starts_with(XML_DECLARATION), "{text}");
    }

    #[test]
    fn writer_is_stable() {
        let mut package = empty_package();
        package.content_types.push(ContentTypeDecl {
            kind: ContentTypeKind::Default,
            name: "jpeg".to_owned(),
            content_type: "image/jpeg".to_owned(),
        });
        package.parts.push(Part {
            name: PartName::new("/docProps/thumbnail.jpeg"),
            content_type: ContentType::new("image/jpeg"),
            data: PartData::Binary(vec![1, 2, 3]),
        });
        let first = write_package(&package).expect("write");
        let reread = crate::read_package(&first, &Limits::DEFAULT).expect("read");
        let second = write_package(&reread).expect("write");
        assert_eq!(first, second);
    }

    #[test]
    fn generated_types_escaping_survives_a_round_trip() {
        let mut package = empty_package();
        package.content_types.push(ContentTypeDecl {
            kind: ContentTypeKind::Default,
            name: "we\"ird".to_owned(),
            content_type: "a&b".to_owned(),
        });
        let bytes = write_package(&package).expect("write");
        let reread = crate::read_package(&bytes, &Limits::DEFAULT).expect("read");
        assert_eq!(reread.content_types, package.content_types);
    }

    #[test]
    fn text_escaping_round_trips() {
        let source = "<R a=\"x&quot;y&#x9;z\"><!--c--><?t d?><![CDATA[<raw>]]>5 &amp; 6</R>";
        let tree = crate::package::xml::parse_xml(source, &Limits::DEFAULT).expect("parse");
        let text = write_xml(&tree);
        let again = crate::package::xml::parse_xml(&text, &Limits::DEFAULT).expect("reparse");
        assert_eq!(tree, again);
    }

    #[test]
    fn helper_parsers_accept_only_their_schema() {
        let tree = crate::package::xml::parse_xml("<Nope/>", &Limits::DEFAULT).expect("parse");
        assert!(crate::package::read::parse_content_types(&tree).is_err());
        assert!(crate::package::read::parse_relationships(&tree, &PartName::new("/")).is_err());
    }

    #[test]
    fn preserved_relationship_attributes_keep_their_order() {
        let tree = crate::package::xml::parse_xml(
            "<Relationships><Relationship Id=\"rId1\" Type=\"t\" Target=\"x\" Z=\"1\" A=\"2\"/></Relationships>",
            &Limits::DEFAULT,
        )
        .expect("parse");
        let rels =
            crate::package::read::parse_relationships(&tree, &PartName::new("/")).expect("parse");
        assert_eq!(rels.len(), 1);
        let names: Vec<&str> = rels[0]
            .preserved
            .attributes
            .iter()
            .map(|a| a.name.local.as_str())
            .collect();
        assert_eq!(names, vec!["Z", "A"]);
    }

    #[test]
    fn empty_root_element_writes_self_closing() {
        let node = XmlNode {
            name: QName::local("A"),
            attributes: Vec::new(),
            children: Vec::new(),
            self_closing: true,
        };
        assert_eq!(write_xml(&node), "<A/>");
    }
}
