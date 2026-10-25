//! Writing the model back to parts (task P2-07).
//!
//! Two entry points, one design (ADR 0018):
//!
//! - [`write_document`] **patches** the supported parts of the preserved
//!   package: each element that maps to a model entity is updated in place,
//!   unmapped content stays exactly where the source put it, elements whose
//!   entity is gone are removed and new entities are inserted at their
//!   canonical position. It is the lossless inverse of
//!   [`crate::to_model`] (ADR 0012) and the writer Phase 4 edits go through.
//! - [`from_model`] **generates** the parts from the model alone, for new
//!   documents: mapped content first, each entity's
//!   [`Preserved`](larrygram_model::Preserved) content after it. It is not
//!   lossless for a loaded document (preserved fragments do not record which
//!   source element they came from, P2-06 open issue 7).
//!
//! `[Content_Types].xml` and the relationships come from
//! [`Package::content_types`] and [`Package::relationships`], refreshed for
//! the parts actually written. Unknown parts are copied verbatim.
//! [`Document::kind`] selects the main part's content type and the part set
//! (ADR 0016): a generated stencil has no pages part. The package writer
//! (P2-05) emits the standard UTF-8 XML declaration on every XML part. Two
//! rules of ADR 0073 run on every written package: the windows part triple
//! is added when the package has none, and every page a `BackPage` names is
//! marked `Background="1"`.

mod emit;
mod patch;
// `pub(crate)`: the reader asks the writer's own `U` spelling decision
// which codes it must preserve (task P6-16).
pub(crate) mod value;

use larrygram_model::{
    Document, DocumentKind, Master, Page, Preserved, QName, XmlAttribute, XmlChild, XmlNode,
};

use crate::Error;
use crate::package::discovery::{
    CT_DRAWING_MAIN, CT_STENCIL_MAIN, CT_TEMPLATE_MAIN, MAIN_PART, is_main_content_type, main_part,
    rel_type_local, resolve_target,
};
use crate::package::{
    ContentType, ContentTypeDecl, ContentTypeKind, Package, Part, PartData, PartName, Relationship,
    RelationshipId, TargetMode, write_package,
};
use crate::write::emit::NameStyle;
use crate::write::patch::{Ctx, PropertyPool};

/// The content type of the page list part.
const CT_PAGES: &str = "application/vnd.ms-visio.pages+xml";
/// The content type of a page contents part.
const CT_PAGE: &str = "application/vnd.ms-visio.page+xml";
/// The content type of the master list part.
const CT_MASTERS: &str = "application/vnd.ms-visio.masters+xml";
/// The content type of a master contents part.
const CT_MASTER: &str = "application/vnd.ms-visio.master+xml";
/// The content type of the windows part.
const CT_WINDOWS: &str = "application/vnd.ms-visio.windows+xml";

/// The page list part.
const PAGES_PART: &str = "/visio/pages/pages.xml";
/// The master list part.
const MASTERS_PART: &str = "/visio/masters/masters.xml";
/// The windows part (task P8-20, ADR 0073).
const WINDOWS_PART: &str = "/visio/windows.xml";
/// The core document properties part.
const CORE_PROPS_PART: &str = "/docProps/core.xml";
/// The extended document properties part.
const APP_PROPS_PART: &str = "/docProps/app.xml";

/// The Visio document relationship type.
const REL_DOCUMENT: &str = "http://schemas.microsoft.com/visio/2010/relationships/document";
/// The page list relationship type.
const REL_PAGES: &str = "http://schemas.microsoft.com/visio/2010/relationships/pages";
/// The page contents relationship type.
const REL_PAGE: &str = "http://schemas.microsoft.com/visio/2010/relationships/page";
/// The master list relationship type.
const REL_MASTERS: &str = "http://schemas.microsoft.com/visio/2010/relationships/masters";
/// The master contents relationship type.
const REL_MASTER: &str = "http://schemas.microsoft.com/visio/2010/relationships/master";
/// The windows relationship type.
const REL_WINDOWS: &str = "http://schemas.microsoft.com/visio/2010/relationships/windows";

/// Maps a document back to package parts.
///
/// The parts are generated from the model (mapped content first, preserved
/// content after) with fresh `[Content_Types].xml` and relationships. This is
/// the writer for a new document; [`write_document`] is the lossless one for
/// a document read from a file (ADR 0018).
pub fn from_model(document: &Document) -> Result<Package, Error> {
    generate_package(document)
}

/// Writes a document to the bytes of a file, preserving everything `preserve`
/// holds that the model does not map.
///
/// `preserve` is the package the document was read from (or an empty
/// [`Package`] for a new document). The supported parts of `preserve` are
/// patched from the model in place (ADR 0018); unknown parts are copied
/// verbatim. The result is semantically equivalent to the input of
/// [`crate::to_model`] when the model is unchanged (ADR 0012), and writing it
/// again is byte-identical.
pub fn write_document(document: &Document, preserve: &Package) -> Result<Vec<u8>, Error> {
    let package = package_for(document, preserve)?;
    write_package(&package)
}

/// The package to write: the preserved package patched from the model, or a
/// generated package when `preserve` holds no Visio document at all.
fn package_for(document: &Document, preserve: &Package) -> Result<Package, Error> {
    match main_part(preserve) {
        Ok(main) => patch_package(document, preserve, &main),
        Err(_) if preserve.parts.is_empty() && preserve.relationships.is_empty() => {
            generate_package(document)
        }
        Err(error) => Err(error),
    }
}

// ---------------------------------------------------------------------------
// Package discovery (the shared rules of `crate::package::discovery`)
// ---------------------------------------------------------------------------

/// The content type of the main part for a document kind (ADR 0016).
fn kind_content_type(kind: DocumentKind) -> &'static str {
    match kind {
        DocumentKind::Stencil => CT_STENCIL_MAIN,
        DocumentKind::Template => CT_TEMPLATE_MAIN,
        _ => CT_DRAWING_MAIN,
    }
}

/// The target part of the relationship with this type from `source`.
fn related(package: &Package, source: &PartName, rel_local: &str) -> Option<PartName> {
    package
        .relationships_from(source)
        .filter(|relationship| {
            relationship.target_mode == TargetMode::Internal
                && rel_type_local(&relationship.rel_type) == rel_local
        })
        .map(|relationship| resolve_target(source.as_str(), &relationship.target))
        .find(|name| package.part(name).is_some())
}

/// The root element of an XML part, if the part is XML.
fn xml_root<'p>(package: &'p Package, name: &PartName) -> Option<&'p XmlNode> {
    match &package.part(name)?.data {
        PartData::Xml(root) => Some(root),
        PartData::Binary(_) => None,
    }
}

/// The name style of a part's root element.
fn root_style(package: &Package, name: &PartName) -> NameStyle {
    xml_root(package, name).map_or_else(NameStyle::default, NameStyle::of_root)
}

// ---------------------------------------------------------------------------
// The patch-based writer
// ---------------------------------------------------------------------------

/// Patches every supported part of `preserve` from the model (ADR 0018).
fn patch_package(
    document: &Document,
    preserve: &Package,
    main: &PartName,
) -> Result<Package, Error> {
    let mut package = preserve.clone();
    let style = root_style(&package, main);
    let ctx = Ctx {
        document,
        style: style.clone(),
    };
    let mut pool = PropertyPool::new(&document.properties);

    // The `docProps` parts first, in the order the reader mapped their
    // properties, then the main part's own `DocumentProperties`.
    patch_root(&mut package, &PartName::new(CORE_PROPS_PART), |root| {
        patch::patch_properties_root(root, &mut pool, &document.preserved)
    });
    patch_root(&mut package, &PartName::new(APP_PROPS_PART), |root| {
        patch::patch_properties_root(root, &mut pool, &document.preserved)
    });
    patch_root(&mut package, main, |root| {
        patch::patch_visio_document(root, &ctx, &mut pool)
    });

    patch_lists(&mut package, &ctx, main)?;
    ensure_windows_part(&mut package, main, &style);
    refresh_content_types(&mut package, document, main);
    Ok(package)
}

/// Patches the root element of one XML part of the package.
fn patch_root(package: &mut Package, name: &PartName, patch: impl FnOnce(&mut XmlNode)) {
    let Some(part) = package.parts.iter_mut().find(|part| &part.name == name) else {
        return;
    };
    if let PartData::Xml(root) = &mut part.data {
        patch(root);
    }
}

/// Patches (or creates) the page and master lists and their contents parts.
///
/// A page or master whose entity is gone from the model also loses its
/// contents part: the part, its relationships and its content-type
/// override are dropped (ADR 0048 deletes, `Package::remove_part`).
fn patch_lists(package: &mut Package, ctx: &Ctx, main: &PartName) -> Result<(), Error> {
    let pages_index = related(package, main, "pages").unwrap_or_else(|| PartName::new(PAGES_PART));
    let pages_exist = package.part(&pages_index).is_some();
    if pages_exist || !ctx.document.pages.is_empty() {
        let rel_ids = ensure_list(
            package,
            ctx,
            &pages_index,
            CT_PAGES,
            main,
            REL_PAGES,
            "pages/pages.xml",
            &ctx.document.pages,
            emit::pages_part,
        )?;
        patch_root(package, &pages_index, |root| {
            patch::patch_pages_root(root, ctx, &rel_ids);
            ensure_background_marks(root);
        });
        let mut keep: Vec<PartName> = Vec::new();
        for (index, page) in ctx.document.pages.iter().enumerate() {
            let Some(rel_id) = rel_ids.get(index).and_then(Option::as_deref) else {
                continue;
            };
            let contents = contents_part(
                package,
                &pages_index,
                rel_id,
                emit::page_part_name(index),
                REL_PAGE,
            )?;
            ensure_contents(
                package,
                &contents,
                CT_PAGE,
                |root| patch::patch_page_contents(root, ctx, page),
                || emit::page_contents(ctx.document, &ctx.style, page),
            )?;
            keep.push(contents);
        }
        prune_orphan_parts(package, &pages_index, "page", &keep);
    }

    let masters_index =
        related(package, main, "masters").unwrap_or_else(|| PartName::new(MASTERS_PART));
    let masters_exist = package.part(&masters_index).is_some();
    if masters_exist || !ctx.document.masters.is_empty() {
        let rel_ids = ensure_list(
            package,
            ctx,
            &masters_index,
            CT_MASTERS,
            main,
            REL_MASTERS,
            "masters/masters.xml",
            &ctx.document.masters,
            emit::masters_part,
        )?;
        patch_root(package, &masters_index, |root| {
            patch::patch_masters_root(root, ctx, &rel_ids)
        });
        let mut keep: Vec<PartName> = Vec::new();
        for (index, master) in ctx.document.masters.iter().enumerate() {
            let Some(rel_id) = rel_ids.get(index).and_then(Option::as_deref) else {
                continue;
            };
            let contents = contents_part(
                package,
                &masters_index,
                rel_id,
                emit::master_part_name(index),
                REL_MASTER,
            )?;
            ensure_contents(
                package,
                &contents,
                CT_MASTER,
                |root| patch::patch_master_contents(root, ctx, master),
                || emit::master_contents(ctx.document, &ctx.style, master),
            )?;
            keep.push(contents);
        }
        prune_orphan_parts(package, &masters_index, "master", &keep);
    }
    Ok(())
}

/// Removes every contents part a relationship of type `rel_local` from
/// `index` points at that `keep` does not name: the part of a deleted
/// page or master, with its relationships and content-type entry (ADR
/// 0048 deletes).
fn prune_orphan_parts(package: &mut Package, index: &PartName, rel_local: &str, keep: &[PartName]) {
    let orphans: Vec<PartName> = package
        .relationships_from(index)
        .filter(|relationship| {
            relationship.target_mode == TargetMode::Internal
                && rel_type_local(&relationship.rel_type) == rel_local
        })
        .map(|relationship| resolve_target(index.as_str(), &relationship.target))
        .filter(|name| !keep.contains(name))
        .collect();
    for name in orphans {
        // A failure (never the root part) leaves the part in place.
        let _ = package.remove_part(&name);
    }
}

/// The shared view of a page or a master: the two lists share one pipeline.
trait ListEntity {
    /// Whether the entity has content that needs a contents part.
    fn has_content(&self) -> bool;
    /// The entity's preserved fragment.
    fn preserved(&self) -> &Preserved;
    /// Visio's `ID`, the key the source list element is matched by.
    fn visio_id(&self) -> i32;
}

impl ListEntity for Page {
    fn has_content(&self) -> bool {
        !self.shapes.is_empty() || !self.connects.is_empty()
    }
    fn preserved(&self) -> &Preserved {
        &self.preserved
    }
    fn visio_id(&self) -> i32 {
        self.visio_id
    }
}

impl ListEntity for Master {
    fn has_content(&self) -> bool {
        !self.shapes.is_empty()
    }
    fn preserved(&self) -> &Preserved {
        &self.preserved
    }
    fn visio_id(&self) -> i32 {
        self.visio_id
    }
}

/// Ensures a page or master list part exists (generating it from the model
/// when it does not) and returns the relationship id of each entity's
/// contents part (`None` when the entity has neither a `Rel` nor content).
#[allow(clippy::too_many_arguments)] // one pipeline shared by pages and masters
fn ensure_list<E: ListEntity>(
    package: &mut Package,
    ctx: &Ctx,
    index: &PartName,
    content_type: &str,
    main: &PartName,
    rel_type: &str,
    target: &str,
    entities: &[E],
    build: impl FnOnce(&Document, &NameStyle, &[String]) -> XmlNode,
) -> Result<Vec<Option<String>>, Error> {
    let mut used: Vec<String> = package
        .relationships_from(index)
        .map(|relationship| relationship.id.as_str().to_owned())
        .collect();
    let mut rel_ids: Vec<Option<String>> = Vec::with_capacity(entities.len());
    for entity in entities {
        let mut rel_id = emit::preserved_rel_id(entity.preserved());
        if rel_id.is_none() {
            // The reader no longer keeps the contents `Rel` of a mapped
            // page or master (task P6-16); the id is recovered from the
            // source element it was read from.
            rel_id = source_rel_id(package, index, entity.visio_id());
        }
        if rel_id.is_none() && entity.has_content() {
            rel_id = Some(fresh_rel_id(&mut used));
        }
        if let Some(id) = &rel_id
            && !used.contains(id)
        {
            used.push(id.clone());
        }
        rel_ids.push(rel_id);
    }
    if package.part(index).is_none() {
        let plain: Vec<String> = rel_ids
            .iter()
            .map(|id| id.clone().unwrap_or_else(|| "rId1".to_owned()))
            .collect();
        let root = build(ctx.document, &ctx.style, &plain);
        add_part(package, index.clone(), content_type, root);
        ensure_relationship(package, main, rel_type, target, fresh_rel_id(&mut used));
    }
    Ok(rel_ids)
}

/// A relationship id no id of `used` takes.
fn fresh_rel_id(used: &mut Vec<String>) -> String {
    let id = fresh_rel_id_from(used);
    used.push(id.clone());
    id
}

/// A relationship id not present in `used`, without recording it.
fn fresh_rel_id_from(used: &[String]) -> String {
    let mut n = used.len().saturating_add(1);
    loop {
        let id = format!("rId{n}");
        if !used.contains(&id) {
            return id;
        }
        n = n.saturating_add(1);
    }
}

/// The `r:id` the source list element of `visio_id` names in its `Rel`
/// child, if any.
///
/// The reader drops the contents `Rel` from the preserved content of a
/// mapped page or master (task P6-16), so a patched entity recovers the id
/// from the source tree the same element was read from. An entity with no
/// source element (a new page or master) has none and allocates a fresh id
/// in [`ensure_list`].
fn source_rel_id(package: &Package, index: &PartName, visio_id: i32) -> Option<String> {
    let PartData::Xml(root) = &package.part(index)?.data else {
        return None;
    };
    root.children.iter().find_map(|child| {
        let element = match child {
            larrygram_model::XmlChild::Element(element) => element,
            _ => return None,
        };
        if !matches!(element.name.local.as_str(), "Page" | "Master") {
            return None;
        }
        let id = element
            .attributes
            .iter()
            .find(|attribute| attribute.name.local == "ID")
            .and_then(|attribute| attribute.value.trim().parse::<i32>().ok());
        if id != Some(visio_id) {
            return None;
        }
        element
            .children
            .iter()
            .filter_map(|child| match child {
                larrygram_model::XmlChild::Element(element) => Some(element),
                _ => None,
            })
            .find(|element| element.name.local == "Rel")
            .and_then(|rel| {
                rel.attributes
                    .iter()
                    .find(|attribute| attribute.name.local == "id")
                    .map(|attribute| attribute.value.clone())
            })
    })
}

/// Adds the relationship (`source`, `rel_type`) to `target` when it is not
/// there yet.
fn ensure_relationship(
    package: &mut Package,
    source: &PartName,
    rel_type: &str,
    target: &str,
    id: String,
) {
    let exists = package
        .relationships_from(source)
        .any(|relationship| relationship.target == target);
    if exists {
        return;
    }
    package.relationships.push(Relationship {
        source: source.clone(),
        id: RelationshipId::new(id),
        rel_type: rel_type.to_owned(),
        target: target.to_owned(),
        target_mode: TargetMode::Internal,
        preserved: Preserved::new(),
    });
}

/// The windows part of the package: the part named [`WINDOWS_PART`], else a
/// part whose content type is the windows content type (task P8-20).
fn windows_part(package: &Package) -> Option<PartName> {
    let named = PartName::new(WINDOWS_PART);
    if package.part(&named).is_some() {
        return Some(named);
    }
    package
        .parts
        .iter()
        .find(|part| part.content_type.as_str() == CT_WINDOWS)
        .map(|part| part.name.clone())
}

/// Makes the package carry the windows part, its relationship and its
/// content-type override (task P8-20, ADR 0073): a package without one does
/// not open in Visio. A package that already has a windows part keeps it
/// exactly as it is; only a missing relationship or declaration is added.
fn ensure_windows_part(package: &mut Package, main: &PartName, style: &NameStyle) {
    let name = match windows_part(package) {
        Some(existing) => existing,
        None => {
            add_part(
                package,
                PartName::new(WINDOWS_PART),
                CT_WINDOWS,
                emit::visio_root(style, "Windows"),
            );
            PartName::new(WINDOWS_PART)
        }
    };
    let has_rel = package.relationships_from(main).any(|relationship| {
        relationship.target_mode == TargetMode::Internal
            && rel_type_local(&relationship.rel_type) == "windows"
    });
    if !has_rel {
        let used: Vec<String> = package
            .relationships_from(main)
            .map(|relationship| relationship.id.as_str().to_owned())
            .collect();
        ensure_relationship(
            package,
            main,
            REL_WINDOWS,
            &windows_target(main, &name),
            fresh_rel_id_from(&used),
        );
    }
    // An `Override` of its own for the windows part, unless the source
    // already names the part in its declarations (kept as it is spelled).
    let has_override = package.content_types.iter().any(|decl| {
        decl.kind == ContentTypeKind::Override && normalize(&decl.name) == normalize(name.as_str())
    });
    if !has_override {
        package.content_types.push(ContentTypeDecl {
            kind: ContentTypeKind::Override,
            name: normalize(name.as_str()),
            content_type: CT_WINDOWS.to_owned(),
        });
    }
}

/// The relationship target naming `name` from the main part: `windows.xml`
/// for the standard part, else the shortest spelling that resolves back.
fn windows_target(main: &PartName, name: &PartName) -> String {
    let directory = main
        .as_str()
        .rsplit_once('/')
        .map_or("", |(directory, _)| directory);
    let prefix = format!("{directory}/");
    name.as_str()
        .strip_prefix(&prefix)
        .map_or_else(|| name.as_str().to_owned(), str::to_owned)
}

/// Marks every page a `BackPage` attribute names as a background page (task
/// P8-20, ADR 0073): [MS-VSDX] requires `BackPage` to name a page whose
/// `Background` attribute is true, and a reference to an unmarked page is
/// silently ignored by Visio, so the writer makes the pair consistent. A
/// repeated `ID` is resolved the way the reader resolves it: the first
/// element with that `ID` takes the mark.
fn ensure_background_marks(root: &mut XmlNode) {
    let mut referenced: Vec<i32> = Vec::new();
    for child in &root.children {
        let XmlChild::Element(element) = child else {
            continue;
        };
        if element.name.local == "Page"
            && let Some(back_page) = attribute_int(element, "BackPage")
        {
            referenced.push(back_page);
        }
    }
    let mut marked: Vec<i32> = Vec::new();
    for child in &mut root.children {
        let XmlChild::Element(element) = child else {
            continue;
        };
        if element.name.local != "Page" {
            continue;
        }
        if let Some(id) = attribute_int(element, "ID")
            && referenced.contains(&id)
            && !marked.contains(&id)
        {
            mark_background(element);
            marked.push(id);
        }
    }
}

/// Sets the `Background` attribute of one element to true, keeping a true
/// spelling the source already has.
fn mark_background(element: &mut XmlNode) {
    match element
        .attributes
        .iter_mut()
        .find(|attribute| attribute.name.local == "Background")
    {
        Some(attribute) => {
            if attribute.value != "1" && attribute.value != "true" {
                attribute.value = "1".to_owned();
            }
        }
        None => element.attributes.push(XmlAttribute {
            name: QName::local("Background"),
            value: "1".to_owned(),
        }),
    }
}

/// The integer value of the element's attribute, if it has one and it parses.
fn attribute_int(node: &XmlNode, local: &str) -> Option<i32> {
    node.attributes
        .iter()
        .find(|attribute| attribute.name.local == local)
        .and_then(|attribute| attribute.value.trim().parse::<i32>().ok())
}

/// The part a `Rel` points at: the part of the relationship with this id,
/// creating the relationship and the part name when the reference dangles.
fn contents_part(
    package: &mut Package,
    index: &PartName,
    rel_id: &str,
    fallback: PartName,
    rel_type: &str,
) -> Result<PartName, Error> {
    let existing = package
        .relationships_from(index)
        .find(|relationship| relationship.id.as_str() == rel_id)
        .map(|relationship| resolve_target(index.as_str(), &relationship.target));
    let name = existing.unwrap_or(fallback);
    if package
        .relationships_from(index)
        .all(|relationship| relationship.id.as_str() != rel_id)
    {
        let file = name
            .as_str()
            .rsplit_once('/')
            .map_or_else(|| name.as_str().to_owned(), |(_, file)| file.to_owned());
        package.relationships.push(Relationship {
            source: index.clone(),
            id: RelationshipId::new(rel_id.to_owned()),
            rel_type: rel_type.to_owned(),
            target: file,
            target_mode: TargetMode::Internal,
            preserved: Preserved::new(),
        });
    }
    Ok(name)
}

/// Patches a contents part in place, or generates it when the model has
/// content and the part is missing.
fn ensure_contents(
    package: &mut Package,
    name: &PartName,
    content_type: &str,
    patch: impl FnOnce(&mut XmlNode),
    build: impl FnOnce() -> XmlNode,
) -> Result<(), Error> {
    if let Some(part) = package.parts.iter_mut().find(|part| &part.name == name) {
        if let PartData::Xml(root) = &mut part.data {
            patch(root);
        }
        return Ok(());
    }
    let root = build();
    add_part(package, name.clone(), content_type, root);
    Ok(())
}

/// Appends a generated XML part to the package.
fn add_part(package: &mut Package, name: PartName, content_type: &str, root: XmlNode) {
    package.content_types.push(ContentTypeDecl {
        kind: ContentTypeKind::Override,
        name: name.as_str().to_owned(),
        content_type: content_type.to_owned(),
    });
    package.parts.push(Part {
        name,
        content_type: ContentType::new(content_type),
        data: PartData::Xml(root),
    });
}

/// Refreshes the content-type declarations for the parts actually written.
///
/// Every part keeps or gets a declaration; the main part's content type
/// follows [`Document::kind`] when the source declared a main content type at
/// all (ADR 0016). A main part the source declared generically (`xml`, the
/// inferred spelling of a package without `[Content_Types].xml`) is left
/// alone: the round-trip criterion compares the declarations (ADR 0012).
fn refresh_content_types(package: &mut Package, document: &Document, main: &PartName) {
    let wanted = kind_content_type(document.kind);
    let declared = package
        .part(main)
        .map(|part| part.content_type.as_str().to_owned());
    if let Some(declared) = declared
        && is_main_content_type(&declared)
        && declared != wanted
    {
        if let Some(part) = package.parts.iter_mut().find(|part| &part.name == main) {
            part.content_type = ContentType::new(wanted);
        }
        match package.content_types.iter_mut().find(|decl| {
            decl.kind == ContentTypeKind::Override && normalize(&decl.name) == main.as_str()
        }) {
            Some(decl) => decl.content_type = wanted.to_owned(),
            None => package.content_types.push(ContentTypeDecl {
                kind: ContentTypeKind::Override,
                name: main.as_str().to_owned(),
                content_type: wanted.to_owned(),
            }),
        }
    }
    let declared: Vec<(String, String)> = package
        .parts
        .iter()
        .map(|part| {
            (
                part.name.as_str().to_owned(),
                part.content_type.as_str().to_owned(),
            )
        })
        .collect();
    for (name, content_type) in declared {
        if declared_type(&name, &package.content_types).is_none() {
            package.content_types.push(ContentTypeDecl {
                kind: ContentTypeKind::Override,
                name: normalize(&name),
                content_type,
            });
        }
    }
}

/// The content type the declarations give a part: `Override` by name, else
/// `Default` by extension.
fn declared_type(name: &str, decls: &[ContentTypeDecl]) -> Option<String> {
    let wanted = normalize(name);
    for decl in decls {
        if decl.kind == ContentTypeKind::Override && normalize(&decl.name) == wanted {
            return Some(decl.content_type.clone());
        }
    }
    let extension = wanted
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())?;
    for decl in decls {
        if decl.kind == ContentTypeKind::Default && decl.name.eq_ignore_ascii_case(&extension) {
            return Some(decl.content_type.clone());
        }
    }
    None
}

/// A declaration name as an absolute part name.
fn normalize(name: &str) -> String {
    let trimmed = name.trim_start_matches('/');
    format!("/{trimmed}")
}

// ---------------------------------------------------------------------------
// The generating writer
// ---------------------------------------------------------------------------

/// Generates every part from the model (task P2-07).
///
/// The part set follows [`Document::kind`] (ADR 0016): a stencil writes no
/// pages part, a drawing without masters writes no master parts. Preserved
/// content is emitted after the mapped content of each entity.
fn generate_package(document: &Document) -> Result<Package, Error> {
    let style = NameStyle::generated();
    let mut package = Package::new();
    let page_rel_ids = generated_rel_ids(&document.pages);
    let master_rel_ids = generated_rel_ids(&document.masters);

    let main = PartName::new(MAIN_PART);
    add_part(
        &mut package,
        main.clone(),
        kind_content_type(document.kind),
        emit::document_part(document, &style),
    );

    let has_pages = document.kind != DocumentKind::Stencil;
    let has_masters = document.kind == DocumentKind::Stencil || !document.masters.is_empty();
    let mut document_rel: u32 = 0;

    if has_pages {
        let mut root = emit::pages_part(document, &style, &page_rel_ids);
        ensure_background_marks(&mut root);
        add_part(&mut package, PartName::new(PAGES_PART), CT_PAGES, root);
        for (index, page) in document.pages.iter().enumerate() {
            add_part(
                &mut package,
                emit::page_part_name(index),
                CT_PAGE,
                emit::page_contents(document, &style, page),
            );
        }
        document_rel = document_rel.saturating_add(1);
        ensure_relationship(
            &mut package,
            &main,
            REL_PAGES,
            "pages/pages.xml",
            format!("rId{document_rel}"),
        );
    }
    if has_masters {
        add_part(
            &mut package,
            PartName::new(MASTERS_PART),
            CT_MASTERS,
            emit::masters_part(document, &style, &master_rel_ids),
        );
        for (index, master) in document.masters.iter().enumerate() {
            add_part(
                &mut package,
                emit::master_part_name(index),
                CT_MASTER,
                emit::master_contents(document, &style, master),
            );
        }
        document_rel = document_rel.saturating_add(1);
        ensure_relationship(
            &mut package,
            &main,
            REL_MASTERS,
            "masters/masters.xml",
            format!("rId{document_rel}"),
        );
    }
    ensure_relationship(
        &mut package,
        &PartName::new("/"),
        REL_DOCUMENT,
        "visio/document.xml",
        "rId1".to_owned(),
    );
    if has_pages {
        let index = PartName::new(PAGES_PART);
        for (position, rel_id) in page_rel_ids.iter().enumerate() {
            let file = format!("page{}.xml", position.saturating_add(1));
            ensure_relationship(&mut package, &index, REL_PAGE, &file, rel_id.clone());
        }
    }
    if has_masters {
        let index = PartName::new(MASTERS_PART);
        for (position, rel_id) in master_rel_ids.iter().enumerate() {
            let file = format!("master{}.xml", position.saturating_add(1));
            ensure_relationship(&mut package, &index, REL_MASTER, &file, rel_id.clone());
        }
    }
    ensure_windows_part(&mut package, &main, &style);
    Ok(package)
}

/// The relationship id of each list entity's contents part: the `Rel` the
/// entity preserved, else the next free `rId`.
fn generated_rel_ids<E: ListEntity>(list: &[E]) -> Vec<String> {
    let mut used: Vec<String> = Vec::new();
    list.iter()
        .map(|entity| match emit::preserved_rel_id(entity.preserved()) {
            Some(id) if !used.contains(&id) => {
                used.push(id.clone());
                id
            }
            _ => fresh_rel_id(&mut used),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use larrygram_model::{CellName, Preserved, Shape, ShapeId, XmlAttribute};

    use super::*;

    /// A document with one page, one rectangle shape and one master.
    fn sample() -> Document {
        let mut document = Document::new();
        let shape = document.add_shape(Shape::new(ShapeId { index: 0 }, 1));
        let mut page = Page::new(larrygram_model::PageId { index: 0 }, 1);
        page.name = "Page-1".to_owned();
        page.shapes.push(shape);
        document.pages.push(page);
        let mut master = Master::new(larrygram_model::MasterId { index: 0 }, 1);
        master.name_u = "Master.1".to_owned();
        document.masters.push(master);
        document
    }

    #[test]
    fn from_model_generates_the_drawing_part_set() {
        let package = from_model(&sample()).expect("package");
        let names: Vec<&str> = package.parts.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "/visio/document.xml",
                "/visio/pages/pages.xml",
                "/visio/pages/page1.xml",
                "/visio/masters/masters.xml",
                "/visio/masters/master1.xml",
                "/visio/windows.xml",
            ]
        );
        let main = package.part(&PartName::new(MAIN_PART)).expect("main part");
        assert_eq!(main.content_type.as_str(), CT_DRAWING_MAIN);
    }

    #[test]
    fn a_stencil_writes_no_pages_part() {
        let mut document = sample();
        document.kind = DocumentKind::Stencil;
        let package = from_model(&document).expect("package");
        let names: Vec<&str> = package.parts.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "/visio/document.xml",
                "/visio/masters/masters.xml",
                "/visio/masters/master1.xml",
                "/visio/windows.xml",
            ]
        );
        let main = package.part(&PartName::new(MAIN_PART)).expect("main");
        assert_eq!(main.content_type.as_str(), CT_STENCIL_MAIN);
    }

    #[test]
    fn a_template_uses_the_template_content_type() {
        let mut document = sample();
        document.kind = DocumentKind::Template;
        let package = from_model(&document).expect("package");
        let main = package.part(&PartName::new(MAIN_PART)).expect("main");
        assert_eq!(main.content_type.as_str(), CT_TEMPLATE_MAIN);
    }

    #[test]
    fn generated_packages_read_back_to_the_model() {
        let document = sample();
        let package = from_model(&document).expect("package");
        let bytes = write_package(&package).expect("bytes");
        let reread = crate::read_document(&bytes, &crate::Limits::DEFAULT).expect("read");
        assert_eq!(reread.pages.len(), 1);
        assert_eq!(reread.pages[0].name, "Page-1");
        assert_eq!(reread.pages[0].shapes.len(), 1);
        assert_eq!(reread.masters.len(), 1);
        assert_eq!(reread.stylesheet.styles.len(), 0);
    }

    #[test]
    fn write_document_patches_and_is_stable() {
        let document = sample();
        let bytes = from_model(&document)
            .and_then(|package| write_package(&package))
            .expect("bytes");
        let package = crate::read_package(&bytes, &crate::Limits::DEFAULT).expect("package");
        let model = crate::to_model(&package).expect("model");
        let first = write_document(&model, &package).expect("first write");
        let reparsed = crate::read_package(&first, &crate::Limits::DEFAULT).expect("package");
        let remodeled = crate::to_model(&reparsed).expect("model");
        let second = write_document(&remodeled, &reparsed).expect("second write");
        assert_eq!(first, second);
    }

    #[test]
    fn write_document_on_an_empty_package_generates() {
        let document = sample();
        let bytes = write_document(&document, &Package::new()).expect("bytes");
        let package = crate::read_package(&bytes, &crate::Limits::DEFAULT).expect("package");
        assert!(package.part(&PartName::new(MAIN_PART)).is_some());
    }

    #[test]
    fn a_package_without_a_main_part_is_an_error() {
        let mut package = Package::new();
        package.parts.push(Part {
            name: PartName::new("/other.xml"),
            content_type: ContentType::new("application/xml"),
            data: PartData::Binary(Vec::new()),
        });
        let error = write_document(&Document::new(), &package);
        assert!(matches!(error, Err(Error::MissingPart(_))));
    }

    #[test]
    fn a_reserved_part_name_is_an_error() {
        // Coverage (audit major 3): the errors `write_document` can return
        // are `MissingPart` (tested above) and `InvalidPackage` from the
        // package writer (a preserved part name reserved for package
        // metadata, and the zip writer failure that no input can provoke).
        for name in ["/[Content_Types].xml", "/visio/_rels/document.xml.rels"] {
            let mut package = Package::new();
            package.parts.push(Part {
                name: PartName::new(MAIN_PART),
                content_type: ContentType::new(CT_DRAWING_MAIN),
                data: PartData::Binary(Vec::new()),
            });
            package.parts.push(Part {
                name: PartName::new(name),
                content_type: ContentType::new("application/xml"),
                data: PartData::Binary(Vec::new()),
            });
            let error = write_document(&sample(), &package);
            assert!(matches!(error, Err(Error::InvalidPackage(_))), "{name}");
        }
    }

    #[test]
    fn a_dangling_relationship_degrades_instead_of_failing() {
        // Coverage (audit major 3): a relationship whose target part is
        // missing never fails the write; the writer builds what the model
        // needs and the dangling entry stays as it was.
        let mut package = Package::new();
        package.parts.push(Part {
            name: PartName::new(MAIN_PART),
            content_type: ContentType::new(CT_DRAWING_MAIN),
            data: PartData::Xml(emit::visio_root(&NameStyle::generated(), "VisioDocument")),
        });
        package.relationships.push(Relationship {
            source: PartName::new(MAIN_PART),
            id: RelationshipId::new("rId7"),
            rel_type: REL_PAGES.to_owned(),
            target: "pages/pages.xml".to_owned(),
            target_mode: TargetMode::Internal,
            preserved: Preserved::new(),
        });
        let bytes = write_document(&sample(), &package).expect("a dangling relationship degrades");
        let reread = crate::read_package(&bytes, &crate::Limits::DEFAULT).expect("reads");
        assert!(reread.part(&PartName::new(PAGES_PART)).is_some());
        assert!(
            reread
                .relationships
                .iter()
                .any(|relationship| relationship.id.as_str() == "rId7")
        );
    }

    #[test]
    fn main_part_finds_the_relationship_or_the_content_type() {
        let mut package = Package::new();
        package.parts.push(Part {
            name: PartName::new("/custom/doc.xml"),
            content_type: ContentType::new(CT_DRAWING_MAIN),
            data: PartData::Binary(Vec::new()),
        });
        assert_eq!(
            main_part(&package).expect("main"),
            PartName::new("/custom/doc.xml")
        );
        package.relationships.push(Relationship {
            source: PartName::new("/"),
            id: RelationshipId::new("rId1"),
            rel_type: REL_DOCUMENT.to_owned(),
            target: "other.xml".to_owned(),
            target_mode: TargetMode::Internal,
            preserved: Preserved::new(),
        });
        package.parts.push(Part {
            name: PartName::new("/other.xml"),
            content_type: ContentType::new("application/xml"),
            data: PartData::Binary(Vec::new()),
        });
        assert_eq!(
            main_part(&package).expect("main"),
            PartName::new("/other.xml")
        );
    }

    #[test]
    fn resolve_target_resolves_relative_and_absolute() {
        assert_eq!(
            resolve_target("/visio/pages/pages.xml", "page1.xml").as_str(),
            "/visio/pages/page1.xml"
        );
        assert_eq!(
            resolve_target("/visio/document.xml", "../pages/pages.xml").as_str(),
            "/pages/pages.xml"
        );
        assert_eq!(
            resolve_target("/visio/document.xml", "/abs.xml").as_str(),
            "/abs.xml"
        );
    }

    #[test]
    fn refresh_content_types_declares_every_part() {
        let mut package = Package::new();
        package.parts.push(Part {
            name: PartName::new(MAIN_PART),
            content_type: ContentType::new(CT_DRAWING_MAIN),
            data: PartData::Binary(Vec::new()),
        });
        package.parts.push(Part {
            name: PartName::new("/image.emf"),
            content_type: ContentType::new("application/octet-stream"),
            data: PartData::Binary(Vec::new()),
        });
        package.content_types.push(ContentTypeDecl {
            kind: ContentTypeKind::Override,
            name: MAIN_PART.to_owned(),
            content_type: CT_DRAWING_MAIN.to_owned(),
        });
        let document = Document::new();
        refresh_content_types(&mut package, &document, &PartName::new(MAIN_PART));
        assert_eq!(package.content_types.len(), 2);
        assert_eq!(package.content_types[1].name, "/image.emf");
    }

    #[test]
    fn refresh_content_types_follows_the_document_kind() {
        let mut package = Package::new();
        package.parts.push(Part {
            name: PartName::new(MAIN_PART),
            content_type: ContentType::new(CT_DRAWING_MAIN),
            data: PartData::Binary(Vec::new()),
        });
        package.content_types.push(ContentTypeDecl {
            kind: ContentTypeKind::Override,
            name: MAIN_PART.to_owned(),
            content_type: CT_DRAWING_MAIN.to_owned(),
        });
        let mut document = Document::new();
        document.kind = DocumentKind::Template;
        refresh_content_types(&mut package, &document, &PartName::new(MAIN_PART));
        assert_eq!(package.content_types[0].content_type, CT_TEMPLATE_MAIN);
        assert_eq!(
            package
                .part(&PartName::new(MAIN_PART))
                .unwrap()
                .content_type
                .as_str(),
            CT_TEMPLATE_MAIN
        );
    }

    #[test]
    fn a_generic_main_content_type_is_left_alone() {
        let mut package = Package::new();
        package.parts.push(Part {
            name: PartName::new(MAIN_PART),
            content_type: ContentType::new("application/xml"),
            data: PartData::Binary(Vec::new()),
        });
        package.content_types.push(ContentTypeDecl {
            kind: ContentTypeKind::Default,
            name: "xml".to_owned(),
            content_type: "application/xml".to_owned(),
        });
        let mut document = Document::new();
        document.kind = DocumentKind::Stencil;
        refresh_content_types(&mut package, &document, &PartName::new(MAIN_PART));
        assert_eq!(package.content_types.len(), 1);
        assert_eq!(
            package
                .part(&PartName::new(MAIN_PART))
                .unwrap()
                .content_type
                .as_str(),
            "application/xml"
        );
    }

    #[test]
    fn preserved_rel_ids_and_fresh_ids_do_not_collide() {
        let mut preserved = Preserved::new();
        preserved.children.push(XmlNode {
            name: larrygram_model::QName::local("Rel"),
            attributes: vec![XmlAttribute {
                name: larrygram_model::QName::local("id"),
                value: "rId7".to_owned(),
            }],
            children: Vec::new(),
            self_closing: true,
        });
        assert_eq!(emit::preserved_rel_id(&preserved), Some("rId7".to_owned()));
        assert_eq!(emit::preserved_rel_id(&Preserved::new()), None);

        let mut used = vec!["rId1".to_owned(), "rId2".to_owned()];
        assert_eq!(fresh_rel_id(&mut used), "rId3");
        assert_eq!(used.len(), 3);
    }

    #[test]
    fn generated_rel_ids_reuse_the_preserved_rel() {
        let mut page = Page::new(larrygram_model::PageId { index: 0 }, 1);
        let mut preserved = Preserved::new();
        preserved.children.push(XmlNode {
            name: larrygram_model::QName::local("Rel"),
            attributes: vec![XmlAttribute {
                name: larrygram_model::QName::local("id"),
                value: "rId3".to_owned(),
            }],
            children: Vec::new(),
            self_closing: true,
        });
        page.preserved = preserved;
        let ids = generated_rel_ids(&[page]);
        assert_eq!(ids, vec!["rId3".to_owned()]);
    }

    #[test]
    fn cells_round_trip_through_a_generated_shape() {
        let mut document = sample();
        let mut cell = larrygram_model::Cell::new(CellName::from_static("PinX"));
        cell.formula = Some("2".to_owned());
        cell.value = Some(larrygram_model::CellValue::Number(2.0));
        cell.unit = larrygram_geom::Unit::Inches;
        if let Some(shape) = document.shape_mut(ShapeId { index: 0 }) {
            shape.sheet.cells.push(cell);
        }
        let package = from_model(&document).expect("package");
        let bytes = write_package(&package).expect("bytes");
        let reread = crate::read_document(&bytes, &crate::Limits::DEFAULT).expect("read");
        let shape = reread.shape(ShapeId { index: 0 }).expect("shape");
        assert_eq!(shape.sheet.cells.len(), 1);
        assert_eq!(shape.sheet.cells[0].name.as_str(), "PinX");
        assert_eq!(
            shape.sheet.cells[0].value,
            Some(larrygram_model::CellValue::Number(2.0))
        );
        assert_eq!(shape.sheet.cells[0].unit, larrygram_geom::Unit::Inches);
    }

    #[test]
    fn a_generated_value_keeps_its_kind_on_re_read() {
        // Regression (P2-10 open issue 1, task P2-14): a model-built `Bool`
        // or number-like `Str` used to read back as `Number` because the
        // writer emitted no `U` for it.
        let mut document = sample();
        let mut flag = larrygram_model::Cell::new(CellName::from_static("Flag"));
        flag.value = Some(larrygram_model::CellValue::Bool(true));
        let mut label = larrygram_model::Cell::new(CellName::from_static("Label"));
        label.value = Some(larrygram_model::CellValue::Str("2".to_owned()));
        let mut plain = larrygram_model::Cell::new(CellName::from_static("Plain"));
        plain.value = Some(larrygram_model::CellValue::Str("Themed".to_owned()));
        if let Some(shape) = document.shape_mut(ShapeId { index: 0 }) {
            shape.sheet.cells.push(flag);
            shape.sheet.cells.push(label);
            shape.sheet.cells.push(plain);
        }
        let package = from_model(&document).expect("package");
        let bytes = write_package(&package).expect("bytes");
        let reread = crate::read_document(&bytes, &crate::Limits::DEFAULT).expect("read");
        let shape = reread.shape(ShapeId { index: 0 }).expect("shape");
        assert_eq!(
            shape.sheet.cells[0].value,
            Some(larrygram_model::CellValue::Bool(true))
        );
        assert_eq!(
            shape.sheet.cells[1].value,
            Some(larrygram_model::CellValue::Str("2".to_owned()))
        );
        assert_eq!(
            shape.sheet.cells[2].value,
            Some(larrygram_model::CellValue::Str("Themed".to_owned()))
        );
    }

    #[test]
    fn newlines_in_text_and_attributes_round_trip() {
        // Coverage (audit minor 4): a newline in text and a newline or a
        // carriage return in an attribute value survive the write (the
        // attribute escapes are `&#xA;` and `&#xD;`).
        let mut document = sample();
        document
            .properties
            .push(larrygram_model::Property::new("title", "line1\nline2"));
        let mut cell = larrygram_model::Cell::new(CellName::from_static("Note"));
        cell.value = Some(larrygram_model::CellValue::Str("a\nb\rc".to_owned()));
        if let Some(shape) = document.shape_mut(ShapeId { index: 0 }) {
            shape.sheet.cells.push(cell);
        }
        let package = from_model(&document).expect("package");
        let bytes = write_package(&package).expect("bytes");
        let reread = crate::read_document(&bytes, &crate::Limits::DEFAULT).expect("read");
        assert_eq!(reread.properties[0].value, "line1\nline2");
        let shape = reread.shape(ShapeId { index: 0 }).expect("shape");
        assert_eq!(
            shape.sheet.cells[0].value,
            Some(larrygram_model::CellValue::Str("a\nb\rc".to_owned()))
        );
    }

    #[test]
    fn refresh_content_types_adds_a_missing_override() {
        // A preserved package whose main part is declared as a stencil
        // main part and carries no `Override` for it: the refresh follows
        // `Document::kind`, rewrites the part and adds the declaration.
        let document = sample();
        let main = PartName::new(MAIN_PART);
        let mut package = from_model(&document).expect("package");
        if let Some(part) = package.parts.iter_mut().find(|part| part.name == main) {
            part.content_type = ContentType::new(CT_STENCIL_MAIN);
        }
        package.content_types.retain(|decl| {
            !(decl.kind == ContentTypeKind::Override && normalize(&decl.name) == main.as_str())
        });
        assert!(
            !package
                .content_types
                .iter()
                .any(|decl| normalize(&decl.name) == main.as_str()),
            "the setup must leave the main part undeclared"
        );

        refresh_content_types(&mut package, &document, &main);

        let declared: Vec<_> = package
            .content_types
            .iter()
            .filter(|decl| {
                decl.kind == ContentTypeKind::Override && normalize(&decl.name) == main.as_str()
            })
            .collect();
        assert_eq!(declared.len(), 1, "{:?}", package.content_types);
        assert_eq!(declared[0].content_type, CT_DRAWING_MAIN);
        let part = package.part(&main).expect("main part");
        assert_eq!(part.content_type.as_str(), CT_DRAWING_MAIN);
    }

    // --- task P8-20: the windows part and background-page marks ---------

    /// A `Page` element with the attributes the two rules read.
    fn page_node(id: &str, back_page: Option<&str>, background: Option<&str>) -> XmlNode {
        let mut node = XmlNode {
            name: QName::local("Page"),
            attributes: Vec::new(),
            children: Vec::new(),
            self_closing: true,
        };
        node.attributes.push(XmlAttribute {
            name: QName::local("ID"),
            value: id.to_owned(),
        });
        if let Some(value) = back_page {
            node.attributes.push(XmlAttribute {
                name: QName::local("BackPage"),
                value: value.to_owned(),
            });
        }
        if let Some(value) = background {
            node.attributes.push(XmlAttribute {
                name: QName::local("Background"),
                value: value.to_owned(),
            });
        }
        node
    }

    /// A pages-list root of these `Page` elements.
    fn pages_root(pages: Vec<XmlNode>) -> XmlNode {
        XmlNode {
            name: QName::local("Pages"),
            attributes: Vec::new(),
            children: pages.into_iter().map(XmlChild::Element).collect(),
            self_closing: false,
        }
    }

    /// The `Background` value of the `Page` at `position`, `None` without
    /// the attribute.
    fn background_at(root: &XmlNode, position: usize) -> Option<String> {
        root.children
            .get(position)
            .and_then(|child| match child {
                XmlChild::Element(element) => Some(element),
                _ => None,
            })
            .and_then(|element| {
                element
                    .attributes
                    .iter()
                    .find(|attribute| attribute.name.local == "Background")
                    .map(|attribute| attribute.value.clone())
            })
    }

    #[test]
    fn a_generated_package_carries_the_windows_triple() {
        let package = from_model(&sample()).expect("package");
        let part = package.part(&PartName::new(WINDOWS_PART)).expect("part");
        assert_eq!(part.content_type.as_str(), CT_WINDOWS);
        assert!(matches!(&part.data, PartData::Xml(_)), "the part is XML");
        if let PartData::Xml(root) = &part.data {
            assert_eq!(root.name.local, "Windows");
            assert!(root.children.is_empty(), "an empty root is enough");
        }
        let main = PartName::new(MAIN_PART);
        let rel = package
            .relationships_from(&main)
            .find(|relationship| relationship.rel_type == REL_WINDOWS)
            .expect("relationship");
        assert_eq!(rel.target, "windows.xml");
        assert_eq!(rel.target_mode, TargetMode::Internal);
        // Pages and masters take `rId1` and `rId2` first.
        assert_eq!(rel.id.as_str(), "rId3");
        assert!(
            package.content_types.iter().any(|decl| {
                decl.kind == ContentTypeKind::Override
                    && decl.name == WINDOWS_PART
                    && decl.content_type == CT_WINDOWS
            }),
            "the override"
        );
    }

    #[test]
    fn a_patched_package_without_a_windows_part_gains_one() {
        let document = sample();
        let source = from_model(&document).expect("generated");
        let mut stripped = source.clone();
        stripped
            .parts
            .retain(|part| part.name.as_str() != WINDOWS_PART);
        stripped
            .content_types
            .retain(|decl| decl.name != WINDOWS_PART);
        stripped
            .relationships
            .retain(|relationship| relationship.rel_type != REL_WINDOWS);
        let package = package_for(&document, &stripped).expect("package");
        assert!(package.part(&PartName::new(WINDOWS_PART)).is_some());
        let rel = package
            .relationships_from(&PartName::new(MAIN_PART))
            .find(|relationship| relationship.rel_type == REL_WINDOWS)
            .expect("relationship");
        // The stripped source keeps `rId1` and `rId2` occupied.
        assert_eq!(rel.id.as_str(), "rId3");
    }

    #[test]
    fn a_package_with_a_windows_part_keeps_it_byte_for_byte() {
        let document = sample();
        let mut source = from_model(&document).expect("generated");
        let mut root = emit::visio_root(&NameStyle::generated(), "Windows");
        root.attributes.push(XmlAttribute {
            name: QName::local("ClientWidth"),
            value: "42".to_owned(),
        });
        if let Some(part) = source
            .parts
            .iter_mut()
            .find(|part| part.name.as_str() == WINDOWS_PART)
        {
            part.data = PartData::Xml(root.clone());
        }
        let package = package_for(&document, &source).expect("package");
        let part = package.part(&PartName::new(WINDOWS_PART)).expect("part");
        assert_eq!(part.data, PartData::Xml(root), "the part is untouched");
    }

    #[test]
    fn a_windows_part_under_another_name_takes_the_relationship() {
        let document = sample();
        let mut source = from_model(&document).expect("generated");
        let windows = source
            .parts
            .iter()
            .position(|part| part.name.as_str() == WINDOWS_PART)
            .expect("generated windows part");
        source.parts[windows].name = PartName::new("/other/win.xml");
        source
            .content_types
            .retain(|decl| decl.name != WINDOWS_PART);
        source.content_types.push(ContentTypeDecl {
            kind: ContentTypeKind::Override,
            name: "/other/win.xml".to_owned(),
            content_type: CT_WINDOWS.to_owned(),
        });
        source
            .relationships
            .retain(|relationship| relationship.rel_type != REL_WINDOWS);
        let package = package_for(&document, &source).expect("package");
        let rel = package
            .relationships_from(&PartName::new(MAIN_PART))
            .find(|relationship| relationship.rel_type == REL_WINDOWS)
            .expect("relationship");
        assert_eq!(rel.target, "/other/win.xml");
        assert!(
            package.part(&PartName::new(WINDOWS_PART)).is_none(),
            "no second windows part"
        );
    }

    #[test]
    fn a_windows_part_without_a_declaration_gets_one() {
        let document = sample();
        let mut source = from_model(&document).expect("generated");
        source
            .content_types
            .retain(|decl| decl.name != WINDOWS_PART);
        let package = package_for(&document, &source).expect("package");
        let added: Vec<&ContentTypeDecl> = package
            .content_types
            .iter()
            .filter(|decl| decl.name == WINDOWS_PART)
            .collect();
        assert_eq!(added.len(), 1);
        assert_eq!(added[0].content_type, CT_WINDOWS);
    }

    #[test]
    fn a_referenced_page_is_marked_a_background_page() {
        let mut root = pages_root(vec![
            page_node("0", Some("1"), None),
            page_node("1", None, None),
        ]);
        ensure_background_marks(&mut root);
        assert_eq!(background_at(&root, 0), None);
        assert_eq!(background_at(&root, 1).as_deref(), Some("1"));
    }

    #[test]
    fn a_false_mark_on_a_referenced_page_becomes_one() {
        let mut root = pages_root(vec![
            page_node("0", Some("1"), None),
            page_node("1", None, Some("0")),
        ]);
        ensure_background_marks(&mut root);
        assert_eq!(background_at(&root, 1).as_deref(), Some("1"));
    }

    #[test]
    fn a_true_mark_keeps_its_spelling() {
        let mut root = pages_root(vec![
            page_node("0", Some("1"), None),
            page_node("1", None, Some("true")),
        ]);
        ensure_background_marks(&mut root);
        assert_eq!(background_at(&root, 1).as_deref(), Some("true"));
    }

    #[test]
    fn an_unreferenced_page_keeps_its_mark_untouched() {
        let mut root = pages_root(vec![page_node("0", None, Some("0"))]);
        ensure_background_marks(&mut root);
        assert_eq!(background_at(&root, 0).as_deref(), Some("0"));
    }

    #[test]
    fn a_back_page_naming_a_missing_page_marks_nothing() {
        let mut root = pages_root(vec![page_node("0", Some("9"), None)]);
        ensure_background_marks(&mut root);
        assert_eq!(background_at(&root, 0), None);
    }

    #[test]
    fn a_repeated_id_is_marked_at_its_first_element() {
        // The reader resolves `BackPage` to the first page with that `ID`;
        // the writer marks the same one (task P8-20).
        let mut root = pages_root(vec![
            page_node("0", Some("7"), None),
            page_node("7", None, None),
            page_node("7", None, None),
        ]);
        ensure_background_marks(&mut root);
        assert_eq!(background_at(&root, 1).as_deref(), Some("1"));
        assert_eq!(background_at(&root, 2), None);
    }

    #[test]
    fn an_unparsable_back_page_marks_nothing() {
        let mut root = pages_root(vec![
            page_node("0", Some("x"), None),
            page_node("x", None, Some("0")),
        ]);
        // A loose comment and a `Page` without an `ID` are skipped by the
        // rule, never crashed on.
        root.children.push(XmlChild::Comment("loose".to_owned()));
        root.children.push(XmlChild::Element(XmlNode {
            name: QName::local("Page"),
            attributes: Vec::new(),
            children: Vec::new(),
            self_closing: true,
        }));
        ensure_background_marks(&mut root);
        // Neither the unparsable `BackPage` value nor the unparsable `ID`
        // takes part in the rule: nothing changes.
        assert_eq!(background_at(&root, 0), None);
        assert_eq!(background_at(&root, 1).as_deref(), Some("0"));
        assert_eq!(background_at(&root, 2), None);
        assert_eq!(background_at(&root, 3), None);
    }

    #[test]
    fn the_background_mark_lands_on_a_generated_page() {
        let mut document = sample();
        let background = Page::new(larrygram_model::PageId { index: 1 }, 2);
        document.pages.push(background);
        document.pages[0].background = Some(larrygram_model::PageId { index: 1 });
        let package = from_model(&document).expect("package");
        let bytes = write_package(&package).expect("bytes");
        let reparsed = crate::read_package(&bytes, &crate::Limits::DEFAULT).expect("read");
        let pages = reparsed
            .part(&PartName::new(PAGES_PART))
            .expect("pages part");
        if let PartData::Xml(root) = &pages.data {
            assert_eq!(background_at(root, 1).as_deref(), Some("1"));
        }
    }

    #[test]
    fn an_unrecoverable_index_part_yields_no_rel_ids() {
        // `source_rel_id` has no source element to recover a `Rel` id from
        // when the index part is missing or not XML: the list grows fresh
        // ids (or keeps its binary part untouched).
        let document = sample();
        let main_part_only = || {
            let mut source = Package::new();
            source.content_types.push(ContentTypeDecl {
                kind: ContentTypeKind::Override,
                name: MAIN_PART.to_owned(),
                content_type: CT_DRAWING_MAIN.to_owned(),
            });
            source.parts.push(Part {
                name: PartName::new(MAIN_PART),
                content_type: ContentType::new(CT_DRAWING_MAIN),
                data: PartData::Xml(emit::visio_root(&NameStyle::generated(), "VisioDocument")),
            });
            source
        };
        let package = package_for(&document, &main_part_only()).expect("package");
        assert!(package.part(&PartName::new(PAGES_PART)).is_some());
        assert!(package.part(&PartName::new(MASTERS_PART)).is_some());

        let mut source = main_part_only();
        source.content_types.push(ContentTypeDecl {
            kind: ContentTypeKind::Override,
            name: PAGES_PART.to_owned(),
            content_type: CT_PAGES.to_owned(),
        });
        source.parts.push(Part {
            name: PartName::new(PAGES_PART),
            content_type: ContentType::new(CT_PAGES),
            data: PartData::Binary(b"not xml".to_vec()),
        });
        let package = package_for(&document, &source).expect("package");
        assert_eq!(
            package
                .part(&PartName::new(PAGES_PART))
                .map(|part| &part.data),
            Some(&PartData::Binary(b"not xml".to_vec())),
            "a binary index part is kept"
        );
    }

    #[test]
    fn a_loose_child_of_a_source_page_element_is_skipped() {
        // `source_rel_id` skips everything that is not an element when it
        // looks for the `Rel` of a source entity.
        let document = sample();
        let mut source = from_model(&document).expect("generated");
        if let Some(part) = source
            .parts
            .iter_mut()
            .find(|part| part.name.as_str() == PAGES_PART)
            && let PartData::Xml(root) = &mut part.data
            && let Some(XmlChild::Element(page)) = root.children.first_mut()
        {
            page.children
                .insert(0, XmlChild::Comment("loose".to_owned()));
        }
        let package = package_for(&document, &source).expect("package");
        assert!(package.part(&PartName::new(PAGES_PART)).is_some());
    }
}
