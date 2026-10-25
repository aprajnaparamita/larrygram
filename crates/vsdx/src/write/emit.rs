//! Building elements from model entities (task P2-07).
//!
//! [`crate::write::from_model`] assembles whole parts from these builders, and
//! the patcher inserts a new model entity as one of these elements. Elements
//! carry the model's mapped attributes first and the entity's [`Preserved`]
//! content after (the reader keeps the two disjoint, so nothing is written
//! twice); an unmapped attribute is emitted only when no mapped attribute
//! takes its name.

use larrygram_model::{
    Cell, CellValue, Connect, Document, Master, Page, Preserved, Property, QName, Row, Section,
    Shape, Style, XmlAttribute, XmlChild, XmlNode,
};

use super::value;
use crate::package::PartName;

/// The namespace URI of the Visio XML vocabulary ([MS-VSDX]).
pub(crate) const VISIO_NS: &str = "http://schemas.microsoft.com/office/visio/2012/main";
/// The namespace URI of the `r` prefix (OPC relationships).
pub(crate) const REL_NS: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

/// How a part spells the names of the Visio namespace.
///
/// A patched part keeps the prefix spelling of its source ([MS-VSDX] rule:
/// never emit a prefix the source did not use for the same URI); a generated
/// part uses the schema's default namespace.
#[derive(Debug, Clone, Default)]
pub(crate) struct NameStyle {
    /// The prefix Visio names are written with, `None` for no prefix.
    visio: Option<String>,
    /// The prefix the relationship namespace is written with, if any.
    rel: Option<String>,
}

impl NameStyle {
    /// The style for a generated part: default namespace, `r` prefix.
    pub(crate) fn generated() -> Self {
        Self {
            visio: None,
            rel: Some("r".to_owned()),
        }
    }

    /// The style of an existing part, read off its root element's namespace
    /// declarations. The Visio namespace takes the default namespace when the
    /// root declares it as such, else the first prefix bound to it, else no
    /// prefix (the fallback of a part with no Visio namespace at all).
    pub(crate) fn of_root(root: &XmlNode) -> Self {
        let mut default_uri = None;
        let mut bindings: Vec<(String, String)> = Vec::new();
        for attribute in &root.attributes {
            match (
                attribute.name.prefix.as_deref(),
                attribute.name.local.as_str(),
            ) {
                (None, "xmlns") => default_uri = Some(attribute.value.clone()),
                (Some("xmlns"), prefix) => {
                    bindings.push((prefix.to_owned(), attribute.value.clone()));
                }
                _ => {}
            }
        }
        let visio = if default_uri.as_deref() == Some(VISIO_NS) {
            None
        } else {
            bindings
                .iter()
                .find(|(_, uri)| uri == VISIO_NS)
                .map(|(prefix, _)| prefix.clone())
        };
        let rel = bindings
            .iter()
            .find(|(_, uri)| uri == REL_NS)
            .map(|(prefix, _)| prefix.clone());
        Self { visio, rel }
    }

    /// The element name of a Visio element.
    pub(crate) fn visio(&self, local: &str) -> QName {
        QName {
            prefix: self.visio.clone(),
            uri: Some(VISIO_NS.to_owned()),
            local: local.to_owned(),
        }
    }

    /// The name of the `id` attribute of a `Rel` element (`r:id` when the
    /// part binds the relationship namespace).
    pub(crate) fn rel_id(&self) -> QName {
        QName {
            prefix: self.rel.clone(),
            uri: self.rel.as_ref().map(|_| REL_NS.to_owned()),
            local: "id".to_owned(),
        }
    }
}

/// The root element of a generated Visio part, with its namespace
/// declarations.
pub(crate) fn visio_root(style: &NameStyle, local: &str) -> XmlNode {
    let mut root = empty(style, local);
    root.attributes.push(XmlAttribute {
        name: QName::local("xmlns"),
        value: VISIO_NS.to_owned(),
    });
    root.attributes.push(XmlAttribute {
        name: QName {
            prefix: Some("xmlns".to_owned()),
            uri: None,
            local: "r".to_owned(),
        },
        value: REL_NS.to_owned(),
    });
    root
}

/// An empty Visio element with no attributes or children.
///
/// Every `map_or_else(|| emit::empty(...), ...)` miss arm in the patcher
/// lands here (coverage audit minor 6): those arms are defensive and
/// unreachable today, because `sync_children` enumerates the same entity
/// list the `build` closure indexes (so the index is always in range). They
/// exist to satisfy `clippy::indexing_slicing` without a panic; a future
/// refactor that makes one reachable gets an empty element of the right
/// kind rather than an element guessed at wrong.
pub(crate) fn empty(style: &NameStyle, local: &str) -> XmlNode {
    XmlNode {
        name: style.visio(local),
        attributes: Vec::new(),
        children: Vec::new(),
        self_closing: true,
    }
}

/// Clears the self-closing flag of an element that has children.
pub(crate) fn finish(node: &mut XmlNode) {
    if !node.children.is_empty() {
        node.self_closing = false;
    }
}

/// Appends a mapped attribute with an unprefixed name.
pub(crate) fn push(node: &mut XmlNode, local: &str, value: String) {
    node.attributes.push(XmlAttribute {
        name: QName::local(local),
        value,
    });
}

/// Appends the preserved attributes whose names no mapped attribute took.
pub(crate) fn push_preserved(node: &mut XmlNode, preserved: &Preserved, managed: &[&str]) {
    for attribute in preserved
        .attributes
        .iter()
        .filter(|a| !managed.contains(&a.name.local.as_str()))
    {
        node.attributes.push(attribute.clone());
    }
}

/// Appends the preserved children after the mapped content.
pub(crate) fn push_preserved_children(node: &mut XmlNode, preserved: &Preserved) {
    for child in &preserved.children {
        node.children.push(XmlChild::Element(child.clone()));
    }
}

/// Whether the cell writes its cached value to `V` (an error goes to `E` and
/// keeps `V` and `U` preserved).
fn writes_v(cell: &Cell) -> bool {
    !matches!(cell.value, Some(CellValue::Error(_)))
}

/// One `Cell` element (task P2-07).
pub(crate) fn cell_element(style: &NameStyle, cell: &Cell) -> XmlNode {
    let mut node = empty(style, "Cell");
    let mut managed: Vec<&str> = vec!["N"];
    push(&mut node, "N", cell.name.as_str().to_owned());
    if let Some(formula) = &cell.formula {
        managed.push("F");
        push(&mut node, "F", formula.clone());
    }
    match &cell.value {
        Some(CellValue::Error(error)) => {
            managed.push("E");
            push(&mut node, "E", error.clone());
        }
        Some(value) => {
            managed.push("V");
            push(&mut node, "V", value::encode_value(value));
        }
        None => {}
    }
    if writes_v(cell) {
        // The `U` the reader needs to get the value's kind back (see
        // [`value::unit_spelling`]); a `U` the file spelled is kept in
        // `Preserved` and pushed below verbatim.
        let current_code = cell
            .preserved
            .attributes
            .iter()
            .find(|attribute| attribute.name.local == "U")
            .map(|attribute| attribute.value.clone());
        match value::unit_spelling(cell.value.as_ref(), cell.unit, current_code.as_deref()) {
            value::UnitSpelling::Keep => {}
            value::UnitSpelling::Code(code) => {
                managed.push("U");
                push(&mut node, "U", code.to_owned());
            }
            value::UnitSpelling::Absent => {}
        }
    }
    push_preserved(&mut node, &cell.preserved, &managed);
    push_preserved_children(&mut node, &cell.preserved);
    finish(&mut node);
    node
}

/// One `Row` element: name, type and index, its cells and its preserved
/// content.
pub(crate) fn row_element(style: &NameStyle, row: &Row) -> XmlNode {
    let mut node = empty(style, "Row");
    let mut managed: Vec<&str> = Vec::new();
    if let Some(name) = &row.name {
        managed.push("N");
        push(&mut node, "N", name.clone());
    }
    if let Some(row_type) = &row.row_type {
        managed.push("T");
        push(&mut node, "T", row_type.clone());
    }
    if let Some(ix) = row.ix {
        managed.push("IX");
        push(&mut node, "IX", ix.to_string());
    }
    push_preserved(&mut node, &row.preserved, &managed);
    for cell in &row.cells {
        node.children
            .push(XmlChild::Element(cell_element(style, cell)));
        node.self_closing = false;
    }
    push_preserved_children(&mut node, &row.preserved);
    finish(&mut node);
    node
}

/// One `Section` element: its name, its level cells, its rows and its
/// preserved content.
pub(crate) fn section_element(style: &NameStyle, section: &Section) -> XmlNode {
    let mut node = empty(style, "Section");
    push(&mut node, "N", section.name.as_str().to_owned());
    push_preserved(&mut node, &section.preserved, &["N"]);
    for cell in &section.cells {
        node.children
            .push(XmlChild::Element(cell_element(style, cell)));
    }
    for row in &section.rows {
        node.children
            .push(XmlChild::Element(row_element(style, row)));
    }
    push_preserved_children(&mut node, &section.preserved);
    finish(&mut node);
    node
}

/// The `Cell` and `Section` children of a sheet element (shape, page sheet,
/// stylesheet row), in file order.
pub(crate) fn sheet_children(
    node: &mut XmlNode,
    style: &NameStyle,
    sheet: &larrygram_model::Sheet,
) {
    for cell in &sheet.cells {
        node.children
            .push(XmlChild::Element(cell_element(style, cell)));
    }
    for section in &sheet.sections {
        node.children
            .push(XmlChild::Element(section_element(style, section)));
    }
    finish(node);
}

/// One `Connect` element.
pub(crate) fn connect_element(style: &NameStyle, connect: &Connect) -> XmlNode {
    let mut node = empty(style, "Connect");
    let managed = [
        "FromSheet",
        "FromCell",
        "FromPart",
        "ToSheet",
        "ToCell",
        "ToPart",
    ];
    push(&mut node, "FromSheet", connect.from_sheet.to_string());
    push(&mut node, "FromCell", connect.from_cell.clone());
    push(&mut node, "FromPart", connect.from_part.to_string());
    push(&mut node, "ToSheet", connect.to_sheet.to_string());
    push(&mut node, "ToCell", connect.to_cell.clone());
    push(&mut node, "ToPart", connect.to_part.to_string());
    push_preserved(&mut node, &connect.preserved, &managed);
    push_preserved_children(&mut node, &connect.preserved);
    finish(&mut node);
    node
}

/// One `Shape` element and its sub-shapes.
///
/// A shape already being written (a cycle in a hand-built model) is skipped,
/// and nesting deeper than 128 (the read cap) is not written: the walk is
/// bounded, never a stack overflow (AGENTS.md section 6).
pub(crate) fn shape_element(document: &Document, style: &NameStyle, shape: &Shape) -> XmlNode {
    let mut stack = Vec::new();
    shape_node(document, style, shape, 0, &mut stack)
}

/// The recursive half of [`shape_element`], bounded by depth and by the
/// shapes already on the walk.
fn shape_node(
    document: &Document,
    style: &NameStyle,
    shape: &Shape,
    depth: u32,
    walking: &mut Vec<u32>,
) -> XmlNode {
    let mut node = empty(style, "Shape");
    let mut managed: Vec<&str> = vec!["ID"];
    push(&mut node, "ID", shape.visio_id.to_string());
    if !shape.name.is_empty() {
        managed.push("Name");
        push(&mut node, "Name", shape.name.clone());
    }
    if !shape.name_u.is_empty() {
        managed.push("NameU");
        push(&mut node, "NameU", shape.name_u.clone());
    }
    if let Some(master) = shape.master.and_then(|id| document.master(id)) {
        managed.push("Master");
        push(&mut node, "Master", master.visio_id.to_string());
    }
    if shape.is_guide {
        // A guide draws on screen only (P8-15: it never prints or
        // exports), and the type is regenerated here, never preserved.
        managed.push("Type");
        push(&mut node, "Type", "Guide".to_owned());
    }
    for (local, reference) in [
        ("LineStyle", shape.line_style),
        ("FillStyle", shape.fill_style),
        ("TextStyle", shape.text_style),
    ] {
        if let Some(id) = reference {
            managed.push(local);
            push(&mut node, local, id.to_string());
        }
    }
    push_preserved(&mut node, &shape.preserved, &managed);
    sheet_children(&mut node, style, &shape.sheet);
    push_preserved_children(&mut node, &shape.preserved);
    finish(&mut node);
    if walking.contains(&shape.id.index) {
        return node;
    }
    if depth >= crate::package::Limits::DEFAULT.max_xml_depth || shape.children.is_empty() {
        return node;
    }
    walking.push(shape.id.index);
    let mut wrapper = empty(style, "Shapes");
    for child in &shape.children {
        if let Some(sub) = document.shape(*child) {
            let sub = shape_node(document, style, sub, depth.saturating_add(1), walking);
            wrapper.children.push(XmlChild::Element(sub));
        }
    }
    walking.pop();
    finish(&mut wrapper);
    if !wrapper.children.is_empty() {
        node.children.push(XmlChild::Element(wrapper));
        node.self_closing = false;
    }
    node
}

/// One `Property` element (a `docProps` or `DocumentProperties` property).
pub(crate) fn property_element(style: &NameStyle, property: &Property) -> XmlNode {
    let mut node = empty(style, &property.name);
    if !property.value.is_empty() {
        node.children.push(XmlChild::Text(property.value.clone()));
        node.self_closing = false;
    }
    node
}

/// One `StyleSheet` element.
pub(crate) fn style_element(style: &NameStyle, row: &Style) -> XmlNode {
    let mut node = empty(style, "StyleSheet");
    let mut managed: Vec<&str> = vec!["ID"];
    push(&mut node, "ID", row.visio_id.to_string());
    if !row.name.is_empty() {
        managed.push("Name");
        push(&mut node, "Name", row.name.clone());
    }
    if !row.name_u.is_empty() {
        managed.push("NameU");
        push(&mut node, "NameU", row.name_u.clone());
    }
    for (local, reference) in [
        ("LineStyle", row.line_style),
        ("FillStyle", row.fill_style),
        ("TextStyle", row.text_style),
    ] {
        if let Some(id) = reference {
            managed.push(local);
            push(&mut node, local, id.to_string());
        }
    }
    push_preserved(&mut node, &row.preserved, &managed);
    sheet_children(&mut node, style, &row.sheet);
    push_preserved_children(&mut node, &row.preserved);
    finish(&mut node);
    node
}

/// One `Rel` element naming the relationship `rel_id`.
pub(crate) fn rel_element(style: &NameStyle, rel_id: &str) -> XmlNode {
    let mut node = empty(style, "Rel");
    node.attributes.push(XmlAttribute {
        name: style.rel_id(),
        value: rel_id.to_owned(),
    });
    node
}

/// The `r:id` a preserved `Rel` child names, if any.
pub(crate) fn preserved_rel_id(preserved: &Preserved) -> Option<String> {
    preserved.children.iter().find_map(|child| {
        if child.name.local != "Rel" {
            return None;
        }
        child
            .attributes
            .iter()
            .find(|a| a.name.local == "id")
            .map(|a| a.value.clone())
    })
}

/// One `Page` element: identity, background page, its page sheet, its
/// preserved content and its `Rel`.
pub(crate) fn page_element(
    document: &Document,
    style: &NameStyle,
    page: &Page,
    rel_id: &str,
) -> XmlNode {
    let mut node = empty(style, "Page");
    let mut managed: Vec<&str> = vec!["ID"];
    push(&mut node, "ID", page.visio_id.to_string());
    if !page.name.is_empty() {
        managed.push("Name");
        push(&mut node, "Name", page.name.clone());
    }
    if !page.name_u.is_empty() {
        managed.push("NameU");
        push(&mut node, "NameU", page.name_u.clone());
    }
    if let Some(background) = page.background.and_then(|id| document.page(id)) {
        managed.push("BackPage");
        push(&mut node, "BackPage", background.visio_id.to_string());
    }
    push_preserved(&mut node, &page.preserved, &managed);
    if !page.sheet.cells.is_empty() || !page.sheet.sections.is_empty() {
        let mut sheet = empty(style, "PageSheet");
        sheet_children(&mut sheet, style, &page.sheet);
        sheet.self_closing = false;
        node.children.push(XmlChild::Element(sheet));
    }
    push_preserved_children(&mut node, &page.preserved);
    if preserved_rel_id(&page.preserved).is_none() {
        node.children
            .push(XmlChild::Element(rel_element(style, rel_id)));
    }
    node.self_closing = false;
    node
}

/// One `Master` element: identity, its preserved content (`PageSheet`,
/// `Icon`, `Rel`) and its `Rel` when it has none.
pub(crate) fn master_element(style: &NameStyle, master: &Master, rel_id: &str) -> XmlNode {
    let mut node = empty(style, "Master");
    let mut managed: Vec<&str> = vec!["ID"];
    push(&mut node, "ID", master.visio_id.to_string());
    if !master.name.is_empty() {
        managed.push("Name");
        push(&mut node, "Name", master.name.clone());
    }
    if !master.name_u.is_empty() {
        managed.push("NameU");
        push(&mut node, "NameU", master.name_u.clone());
    }
    if let Some(unique_id) = &master.unique_id {
        managed.push("UniqueID");
        push(&mut node, "UniqueID", unique_id.clone());
    }
    if let Some(prompt) = &master.prompt {
        managed.push("Prompt");
        push(&mut node, "Prompt", prompt.clone());
    }
    if master.hidden {
        managed.push("Hidden");
        push(&mut node, "Hidden", "1".to_owned());
    }
    push_preserved(&mut node, &master.preserved, &managed);
    push_preserved_children(&mut node, &master.preserved);
    if preserved_rel_id(&master.preserved).is_none() {
        node.children
            .push(XmlChild::Element(rel_element(style, rel_id)));
    }
    node.self_closing = false;
    node
}

/// The `Shapes` wrapper of a page's or master's top-level shapes.
pub(crate) fn shapes_wrapper(
    document: &Document,
    style: &NameStyle,
    shapes: &[larrygram_model::ShapeId],
) -> XmlNode {
    let mut wrapper = empty(style, "Shapes");
    for id in shapes {
        if let Some(shape) = document.shape(*id) {
            wrapper
                .children
                .push(XmlChild::Element(shape_element(document, style, shape)));
        }
    }
    finish(&mut wrapper);
    wrapper
}

/// The `Connects` wrapper of a page's connect records.
pub(crate) fn connects_wrapper(style: &NameStyle, connects: &[Connect]) -> XmlNode {
    let mut wrapper = empty(style, "Connects");
    for connect in connects {
        wrapper
            .children
            .push(XmlChild::Element(connect_element(style, connect)));
    }
    finish(&mut wrapper);
    wrapper
}

/// The `PageContents` root of a generated page contents part.
pub(crate) fn page_contents(document: &Document, style: &NameStyle, page: &Page) -> XmlNode {
    let mut root = visio_root(style, "PageContents");
    if !page.shapes.is_empty() {
        root.children.push(XmlChild::Element(shapes_wrapper(
            document,
            style,
            &page.shapes,
        )));
    }
    if !page.connects.is_empty() {
        root.children
            .push(XmlChild::Element(connects_wrapper(style, &page.connects)));
    }
    push_preserved_children(&mut root, &page.preserved);
    root.self_closing = false;
    root
}

/// The `MasterContents` root of a generated master contents part.
pub(crate) fn master_contents(document: &Document, style: &NameStyle, master: &Master) -> XmlNode {
    let mut root = visio_root(style, "MasterContents");
    if !master.shapes.is_empty() {
        root.children.push(XmlChild::Element(shapes_wrapper(
            document,
            style,
            &master.shapes,
        )));
    }
    push_preserved_children(&mut root, &master.preserved);
    root.self_closing = false;
    root
}

/// The `Pages` root of a generated page list part.
pub(crate) fn pages_part(document: &Document, style: &NameStyle, rel_ids: &[String]) -> XmlNode {
    let mut root = visio_root(style, "Pages");
    for (index, page) in document.pages.iter().enumerate() {
        let rel_id = rel_ids.get(index).map_or("rId1", String::as_str);
        root.children.push(XmlChild::Element(page_element(
            document, style, page, rel_id,
        )));
    }
    root.self_closing = false;
    root
}

/// The `Masters` root of a generated master list part.
pub(crate) fn masters_part(document: &Document, style: &NameStyle, rel_ids: &[String]) -> XmlNode {
    let mut root = visio_root(style, "Masters");
    for (index, master) in document.masters.iter().enumerate() {
        let rel_id = rel_ids.get(index).map_or("rId1", String::as_str);
        root.children
            .push(XmlChild::Element(master_element(style, master, rel_id)));
    }
    root.self_closing = false;
    root
}

/// The `VisioDocument` root of a generated main part.
pub(crate) fn document_part(document: &Document, style: &NameStyle) -> XmlNode {
    let mut root = visio_root(style, "VisioDocument");
    if !document.properties.is_empty() {
        let mut wrapper = empty(style, "DocumentProperties");
        for property in &document.properties {
            wrapper
                .children
                .push(XmlChild::Element(property_element(style, property)));
        }
        wrapper.self_closing = false;
        root.children.push(XmlChild::Element(wrapper));
    }
    if !document.stylesheet.styles.is_empty() {
        let mut wrapper = empty(style, "StyleSheets");
        for row in &document.stylesheet.styles {
            wrapper
                .children
                .push(XmlChild::Element(style_element(style, row)));
        }
        wrapper.self_closing = false;
        root.children.push(XmlChild::Element(wrapper));
    }
    // The document colour table (`Colors`/`ColorEntry`, ADR 0019). A
    // document whose preserved content already carries the element (one
    // read from a file) keeps it as the source of the spelling; a
    // document with only the typed table gets the element generated
    // here (task P8-10: without it a `.vsd` import saved as `.vsdx`
    // lost its colour table).
    if !document.color_table.is_empty()
        && !document
            .preserved
            .children
            .iter()
            .any(|child| child.name.local == "Colors")
    {
        let mut wrapper = empty(style, "Colors");
        for entry in &document.color_table {
            let mut node = empty(style, "ColorEntry");
            push(&mut node, "IX", entry.index.to_string());
            push(
                &mut node,
                "RGB",
                format!(
                    "#{:02x}{:02x}{:02x}",
                    entry.color.r, entry.color.g, entry.color.b
                ),
            );
            wrapper.children.push(XmlChild::Element(node));
        }
        wrapper.self_closing = false;
        root.children.push(XmlChild::Element(wrapper));
    }
    push_preserved_children(&mut root, &document.preserved);
    root.self_closing = false;
    root
}

/// The part names of a generated or patched package (task P2-07).
pub(crate) fn page_part_name(index: usize) -> PartName {
    PartName::new(format!("/visio/pages/page{}.xml", index.saturating_add(1)))
}

/// The part name of a master contents part.
pub(crate) fn master_part_name(index: usize) -> PartName {
    PartName::new(format!(
        "/visio/masters/master{}.xml",
        index.saturating_add(1)
    ))
}

#[cfg(test)]
mod tests {
    use larrygram_geom::Unit;
    use larrygram_model::{CellName, CellValue, Document, Row, Section, SectionName, ShapeId};

    use super::*;

    /// A qualified name from `prefix:local` or `local`.
    fn qname(name: &str) -> QName {
        match name.split_once(':') {
            Some((prefix, local)) => QName {
                prefix: Some(prefix.to_owned()),
                uri: None,
                local: local.to_owned(),
            },
            None => QName::local(name),
        }
    }

    /// An element with these attributes.
    fn element(local: &str, attrs: &[(&str, &str)]) -> XmlNode {
        XmlNode {
            name: qname(local),
            attributes: attrs
                .iter()
                .map(|(name, value)| XmlAttribute {
                    name: qname(name),
                    value: (*value).to_owned(),
                })
                .collect(),
            children: Vec::new(),
            self_closing: true,
        }
    }

    /// The values of the attributes of `node` with these names, in order.
    fn attrs(node: &XmlNode) -> Vec<(String, String)> {
        node.attributes
            .iter()
            .map(|a| (a.name.local.clone(), a.value.clone()))
            .collect()
    }

    #[test]
    fn generated_names_use_the_default_namespace() {
        let style = NameStyle::generated();
        let name = style.visio("Cell");
        assert_eq!(name.prefix, None);
        assert_eq!(name.uri.as_deref(), Some(VISIO_NS));
        assert_eq!(style.rel_id().prefix.as_deref(), Some("r"));
    }

    #[test]
    fn root_style_follows_the_source_prefixes() {
        let root = element("Root", &[("xmlns", VISIO_NS)]);
        let style = NameStyle::of_root(&root);
        assert_eq!(style.visio("Cell").prefix, None);

        let root = element(
            "Root",
            &[
                ("xmlns:v", VISIO_NS),
                ("xmlns:r", REL_NS),
                ("xmlns", "urn:other"),
            ],
        );
        let style = NameStyle::of_root(&root);
        assert_eq!(style.visio("Cell").prefix.as_deref(), Some("v"));
        assert_eq!(style.rel_id().prefix.as_deref(), Some("r"));
        assert_eq!(style.rel_id().uri.as_deref(), Some(REL_NS));

        // No Visio binding at all: no prefix.
        let root = element("Root", &[]);
        assert_eq!(NameStyle::of_root(&root).visio("Cell").prefix, None);
    }

    #[test]
    fn generated_roots_declare_their_namespaces() {
        let root = visio_root(&NameStyle::generated(), "Pages");
        let names: Vec<&str> = root
            .attributes
            .iter()
            .map(|a| a.name.local.as_str())
            .collect();
        assert_eq!(names, ["xmlns", "r"]);
    }

    #[test]
    fn a_cell_element_writes_its_managed_attributes_once() {
        let mut cell = Cell::new(CellName::from_static("PinX"));
        cell.formula = Some("1".to_owned());
        cell.value = Some(CellValue::Number(1.0));
        cell.unit = Unit::Millimeters;
        cell.preserved.attributes.push(XmlAttribute {
            name: QName::local("U"),
            value: "bogus".to_owned(),
        });
        cell.preserved.attributes.push(XmlAttribute {
            name: QName::local("Extra"),
            value: "kept".to_owned(),
        });
        let node = cell_element(&NameStyle::generated(), &cell);
        let names: Vec<&str> = node
            .attributes
            .iter()
            .map(|a| a.name.local.as_str())
            .collect();
        // The managed `U` wins; the preserved duplicate is dropped.
        assert_eq!(names, ["N", "F", "V", "U", "Extra"]);
    }

    #[test]
    fn an_error_cell_keeps_v_and_u_from_preserved() {
        let mut cell = Cell::new(CellName::from_static("Value"));
        cell.value = Some(CellValue::Error("#VALUE!".to_owned()));
        for (local, value) in [("U", "STR"), ("V", "kept")] {
            cell.preserved.attributes.push(XmlAttribute {
                name: QName::local(local),
                value: value.to_owned(),
            });
        }
        let node = cell_element(&NameStyle::generated(), &cell);
        let names: Vec<&str> = node
            .attributes
            .iter()
            .map(|a| a.name.local.as_str())
            .collect();
        assert_eq!(names, ["N", "E", "U", "V"]);
    }

    #[test]
    fn row_and_section_elements_write_their_content_in_order() {
        let mut row = Row::new();
        row.ix = Some(1);
        row.cells.push(Cell::new(CellName::from_static("X")));
        let mut section = Section::new(SectionName::from_static("Geometry"));
        section
            .cells
            .push(Cell::new(CellName::from_static("NoFill")));
        section.rows.push(row);
        let node = section_element(&NameStyle::generated(), &section);
        let names: Vec<&str> = node
            .children
            .iter()
            .map(|child| match child {
                XmlChild::Element(element) => element.name.local.as_str(),
                _ => "?",
            })
            .collect();
        assert_eq!(names, ["Cell", "Row"]);
        assert!(!node.self_closing);
    }

    #[test]
    fn a_shape_element_names_its_master_and_sub_shapes() {
        let mut document = Document::new();
        let mut master = Master::new(larrygram_model::MasterId { index: 0 }, 5);
        master.name_u = "Box".to_owned();
        document.masters.push(master);
        let mut group = Shape::new(ShapeId { index: 0 }, 1);
        group.master = Some(larrygram_model::MasterId { index: 0 });
        let group_id = document.add_shape(group);
        let sub_id = document.add_shape(Shape::new(ShapeId { index: 0 }, 2));
        document
            .shape_mut(group_id)
            .expect("group")
            .children
            .push(sub_id);
        let group = document.shape(group_id).expect("group");
        let node = shape_element(&document, &NameStyle::generated(), group);
        let master = node
            .attributes
            .iter()
            .find(|a| a.name.local == "Master")
            .expect("Master attribute");
        assert_eq!(master.value, "5");
        let names: Vec<&str> = node
            .children
            .iter()
            .map(|child| match child {
                XmlChild::Element(element) => element.name.local.as_str(),
                _ => "?",
            })
            .collect();
        assert_eq!(names, ["Shapes"]);
    }

    #[test]
    fn a_cyclic_shape_walk_terminates() {
        let mut document = Document::new();
        let a = document.add_shape(Shape::new(ShapeId { index: 0 }, 1));
        let b = document.add_shape(Shape::new(ShapeId { index: 0 }, 2));
        document.shape_mut(a).expect("a").children.push(b);
        document.shape_mut(b).expect("b").children.push(a);
        let a = document.shape(a).expect("a");
        let node = shape_element(&document, &NameStyle::generated(), a);
        // The cycle is cut: `a` is not written inside its own subtree.
        let names: Vec<&str> = node
            .children
            .iter()
            .map(|child| match child {
                XmlChild::Element(element) => element.name.local.as_str(),
                _ => "?",
            })
            .collect();
        assert_eq!(names, ["Shapes"]);
    }

    #[test]
    fn properties_and_rels_build_small_elements() {
        let property = Property::new("title", "Hello");
        let node = property_element(&NameStyle::generated(), &property);
        assert_eq!(node.children.len(), 1);
        let empty = property_element(&NameStyle::generated(), &Property::new("title", ""));
        assert!(empty.children.is_empty());
        assert!(empty.self_closing);

        let rel = rel_element(&NameStyle::generated(), "rId3");
        assert_eq!(attrs(&rel), vec![("id".to_owned(), "rId3".to_owned())]);
    }

    #[test]
    fn preserved_rel_ids_are_read_from_the_fragment() {
        let mut preserved = Preserved::new();
        preserved.children.push({
            let mut rel = element("Rel", &[("id", "rId9")]);
            rel.name = QName {
                prefix: Some("r".to_owned()),
                uri: Some(REL_NS.to_owned()),
                local: "Rel".to_owned(),
            };
            rel
        });
        assert_eq!(preserved_rel_id(&preserved), Some("rId9".to_owned()));
        assert_eq!(preserved_rel_id(&Preserved::new()), None);
    }

    #[test]
    fn pages_and_masters_parts_carry_their_lists() {
        let mut document = Document::new();
        let mut page = Page::new(larrygram_model::PageId { index: 0 }, 1);
        page.name = "P".to_owned();
        document.pages.push(page);
        let root = pages_part(&document, &NameStyle::generated(), &["rId2".to_owned()]);
        assert_eq!(root.children.len(), 1);
        let pages: Vec<&XmlNode> = root
            .children
            .iter()
            .filter_map(|child| match child {
                XmlChild::Element(element) => Some(element),
                _ => None,
            })
            .collect();
        let has_rel = pages[0].children.iter().any(
            |child| matches!(child, XmlChild::Element(element) if element.name.local == "Rel"),
        );
        assert!(has_rel);

        document
            .masters
            .push(Master::new(larrygram_model::MasterId { index: 0 }, 1));
        let root = masters_part(&document, &NameStyle::generated(), &["rId1".to_owned()]);
        assert_eq!(root.children.len(), 1);
    }

    #[test]
    fn contents_roots_hold_shapes_and_connects() {
        let mut document = Document::new();
        let id = document.add_shape(Shape::new(ShapeId { index: 0 }, 1));
        let mut page = Page::new(larrygram_model::PageId { index: 0 }, 1);
        page.shapes.push(id);
        page.connects.push(Connect::new());
        let root = page_contents(&document, &NameStyle::generated(), &page);
        let names: Vec<&str> = root
            .children
            .iter()
            .map(|child| match child {
                XmlChild::Element(element) => element.name.local.as_str(),
                _ => "?",
            })
            .collect();
        assert_eq!(names, ["Shapes", "Connects"]);

        let master = {
            let mut master = Master::new(larrygram_model::MasterId { index: 0 }, 1);
            master.shapes.push(id);
            master
        };
        let root = master_contents(&document, &NameStyle::generated(), &master);
        assert_eq!(root.children.len(), 1);
    }

    #[test]
    fn the_document_part_holds_properties_and_styles() {
        let mut document = Document::new();
        document.properties.push(Property::new("title", "T"));
        let mut style = Style::new(larrygram_model::StyleId { index: 0 });
        style.visio_id = 1;
        document.stylesheet.styles.push(style);
        let root = document_part(&document, &NameStyle::generated());
        let names: Vec<&str> = root
            .children
            .iter()
            .map(|child| match child {
                XmlChild::Element(element) => element.name.local.as_str(),
                _ => "?",
            })
            .collect();
        assert_eq!(names, ["DocumentProperties", "StyleSheets"]);
    }

    #[test]
    fn push_preserved_children_appends_and_finish_clears_self_closing() {
        let mut preserved = Preserved::new();
        preserved.children.push(element("Extra", &[]));
        let mut node = empty(&NameStyle::generated(), "X");
        push_preserved_children(&mut node, &preserved);
        finish(&mut node);
        assert_eq!(node.children.len(), 1);
        assert!(!node.self_closing);
    }
}
