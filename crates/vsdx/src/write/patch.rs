//! Patching a source part tree from the model (task P2-07, ADR 0018).
//!
//! [`crate::write::write_document`] regenerates a package by patching the
//! part trees of the preserved package in place:
//!
//! - every element that maps to a model entity is updated from that entity,
//!   matched by identity (shapes, pages, masters and styles by `ID`;
//!   sections by `N` and `IX`; rows by `IX` or `N`; cells and properties by
//!   name), never by position;
//! - every unmapped element and attribute stays exactly where the source put
//!   it (the model's [`Preserved`](larrygram_model::Preserved) fragments are
//!   not consulted: several source elements merge into one fragment, so the
//!   tree is the authoritative record of their positions);
//! - a source element whose entity is gone from the model is removed, unless
//!   an identical node is still in the entity's preserved fragment (which is
//!   where the reader keeps elements it does not map);
//! - a new model entity is inserted after the last sibling of the same kind,
//!   or where the [MS-VSDX] sequence puts it.
//!
//! An attribute value is rewritten only when it no longer matches the model,
//! and a matching spelling is kept (`2.0` stays `2.0`, `true` stays `true`),
//! so an unedited write changes nothing and a second write is byte-identical.

use larrygram_model::{
    Cell, CellValue, Connect, Document, Master, Page, Preserved, Property, Row, Section, Shape,
    Style, XmlChild, XmlNode,
};

use super::emit::{self, NameStyle};
use super::value;
use crate::read::node::text_content;

/// The patcher's shared inputs: the model and the part's name spelling.
pub(crate) struct Ctx<'d> {
    /// The document the part is written from.
    pub(crate) document: &'d Document,
    /// How this part spells Visio names.
    pub(crate) style: NameStyle,
}

// ---------------------------------------------------------------------------
// Attribute synchronization
// ---------------------------------------------------------------------------

/// The value of the first attribute with this local name.
fn current<'n>(node: &'n XmlNode, local: &str) -> Option<&'n str> {
    node.attributes
        .iter()
        .find(|attribute| attribute.name.local == local)
        .map(|attribute| attribute.value.as_str())
}

/// Sets the attribute, appending it when the element does not have it.
fn set(node: &mut XmlNode, local: &str, value: String) {
    match node
        .attributes
        .iter_mut()
        .find(|attribute| attribute.name.local == local)
    {
        Some(attribute) => attribute.value = value,
        None => node.attributes.push(larrygram_model::XmlAttribute {
            name: larrygram_model::QName::local(local),
            value,
        }),
    }
}

/// Removes the attribute, if the element has it.
fn remove(node: &mut XmlNode, local: &str) {
    node.attributes
        .retain(|attribute| attribute.name.local != local);
}

/// Whether the attribute of `preserved` with this local name exists.
fn preserved_has(preserved: &Preserved, local: &str) -> bool {
    preserved
        .attributes
        .iter()
        .any(|attribute| attribute.name.local == local)
}

/// Syncs a string attribute with a default (absent) value.
fn sync_string(node: &mut XmlNode, local: &str, value: &str, default: &str) {
    match current(node, local) {
        Some(now) if now == value => {}
        Some(_) => set(node, local, value.to_owned()),
        None if value != default => set(node, local, value.to_owned()),
        None => {}
    }
}

/// Syncs an optional string attribute.
fn sync_opt_string(node: &mut XmlNode, local: &str, value: Option<&str>) {
    match (current(node, local), value) {
        (Some(now), Some(want)) if now == want => {}
        (_, Some(want)) => set(node, local, want.to_owned()),
        (Some(_), None) => remove(node, local),
        (None, None) => {}
    }
}

/// Syncs an integer attribute with a default (absent) value, keeping any
/// spelling that parses to the same number.
fn sync_int(node: &mut XmlNode, local: &str, value: i32, default: i32) {
    match current(node, local) {
        Some(now) if now.trim().parse::<i32>() == Ok(value) => {}
        Some(_) => set(node, local, value.to_string()),
        None if value != default => set(node, local, value.to_string()),
        None => {}
    }
}

/// Syncs an optional integer attribute.
fn sync_opt_int(node: &mut XmlNode, local: &str, value: Option<i32>) {
    match (current(node, local), value) {
        (Some(now), Some(want)) if now.trim().parse::<i32>() == Ok(want) => {}
        (_, Some(want)) => set(node, local, want.to_string()),
        (Some(_), None) => remove(node, local),
        (None, None) => {}
    }
}

/// Syncs an optional unsigned attribute (a style reference or a stylesheet
/// `ID`).
fn sync_opt_uint(node: &mut XmlNode, local: &str, value: Option<u32>) {
    match (current(node, local), value) {
        (Some(now), Some(want)) if now.trim().parse::<u32>() == Ok(want) => {}
        (_, Some(want)) => set(node, local, want.to_string()),
        (Some(_), None) => remove(node, local),
        (None, None) => {}
    }
}

/// Syncs an unsigned attribute with a default (absent) value.
fn sync_uint(node: &mut XmlNode, local: &str, value: u32, default: u32) {
    match current(node, local) {
        Some(now) if now.trim().parse::<u32>() == Ok(value) => {}
        Some(_) => set(node, local, value.to_string()),
        None if value != default => set(node, local, value.to_string()),
        None => {}
    }
}

/// Whether the written boolean spells `value`.
fn bool_matches(now: &str, value: bool) -> bool {
    match now.trim() {
        "1" | "true" | "TRUE" => value,
        "0" | "false" | "FALSE" => !value,
        _ => false,
    }
}

/// Syncs a boolean attribute: absent stays absent when the value is the
/// default `false`.
fn sync_bool(node: &mut XmlNode, local: &str, value: bool) {
    match current(node, local) {
        Some(now) if bool_matches(now, value) => {}
        Some(_) => set(node, local, if value { "1" } else { "0" }.to_owned()),
        None if value => set(node, local, "1".to_owned()),
        None => {}
    }
}

/// Syncs a reference attribute (`Master`, `BackPage`) that names another
/// entity by its Visio `ID`.
///
/// An unresolved reference is preserved verbatim by the reader, so `None`
/// removes the attribute only when the entity's preserved fragment does not
/// hold it: that spelling is unmapped content, not a cleared reference.
fn sync_ref(node: &mut XmlNode, local: &str, value: Option<i32>, preserved: &Preserved) {
    match value {
        Some(id) => sync_opt_int(node, local, Some(id)),
        None => {
            if current(node, local).is_some() && !preserved_has(preserved, local) {
                remove(node, local);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Child synchronization
// ---------------------------------------------------------------------------

/// Claims each entity to at most one source element: the first unclaimed
/// source with the same key. Entities and sources without a key claim each
/// other positionally.
fn claim<K: PartialEq>(
    sources: &[Option<K>],
    wanted: &[Option<K>],
    used: &mut [bool],
) -> Vec<Option<usize>> {
    wanted
        .iter()
        .map(|want| {
            let hit = sources
                .iter()
                .enumerate()
                .find(|(index, source)| {
                    !used.get(*index).copied().unwrap_or(true) && *source == want
                })
                .map(|(index, _)| index);
            if let Some(index) = hit
                && let Some(slot) = used.get_mut(index)
            {
                *slot = true;
            }
            hit
        })
        .collect()
}

/// Whether the text is empty or made only of whitespace.
fn blank(text: &str) -> bool {
    text.is_empty() || text.chars().all(char::is_whitespace)
}

/// Removes the children at these indices.
///
/// When elements were dropped and nothing but whitespace text is left, that
/// pretty-printing whitespace goes too: [`diff`](larrygram_model::diff) would
/// otherwise read it as significant text (ADR 0012 whitespace rule). When
/// nothing was dropped, blank text is the element's own significant content
/// and stays.
fn remove_children(children: &mut Vec<XmlChild>, drop: &[usize]) {
    let mut kept: Vec<XmlChild> = Vec::with_capacity(children.len());
    for (index, child) in children.iter().enumerate() {
        if !drop.contains(&index) {
            kept.push(child.clone());
        }
    }
    let only_whitespace = kept
        .iter()
        .all(|child| matches!(child, XmlChild::Text(text) if blank(text)));
    if only_whitespace && !drop.is_empty() {
        kept.clear();
    }
    *children = kept;
}

/// Inserts the element after the last sibling of the same kind or an earlier
/// one, before the first sibling of a later kind.
fn insert_child(children: &mut Vec<XmlChild>, node: XmlNode, rank: fn(&str) -> u8, mine: u8) {
    let mut last: Option<usize> = None;
    let mut first_later: Option<usize> = None;
    for (index, child) in children.iter().enumerate() {
        let XmlChild::Element(element) = child else {
            continue;
        };
        if rank(&element.name.local) <= mine {
            last = Some(index);
        } else if first_later.is_none() {
            first_later = Some(index);
        }
    }
    let at = match (last, first_later) {
        (Some(index), _) => index.saturating_add(1),
        (None, Some(index)) => index,
        (None, None) => children.len(),
    };
    children.insert(at, XmlChild::Element(node));
}

/// Synchronizes the mapped children of `node` against the entities of one
/// kind (see the module docs): patch claimed elements, remove unclaimed ones
/// that are not preserved, insert entities no source element claimed.
#[allow(clippy::too_many_arguments)]
fn sync_children<K: PartialEq>(
    node: &mut XmlNode,
    is_kind: impl Fn(&XmlNode) -> bool,
    source_key: impl Fn(&XmlNode) -> Option<K>,
    entity_keys: &[Option<K>],
    preserved: &Preserved,
    patch: impl Fn(&mut XmlNode, usize),
    build: impl Fn(usize) -> XmlNode,
    rank: fn(&str) -> u8,
    my_rank: u8,
) {
    let candidates: Vec<usize> = node
        .children
        .iter()
        .enumerate()
        .filter(|(_, child)| matches!(child, XmlChild::Element(element) if is_kind(element)))
        .map(|(index, _)| index)
        .collect();
    let sources: Vec<Option<K>> = candidates
        .iter()
        .map(|&index| match node.children.get(index) {
            Some(XmlChild::Element(element)) => source_key(element),
            _ => None,
        })
        .collect();
    let mut used = vec![false; sources.len()];
    let claims = claim(&sources, entity_keys, &mut used);
    for (entity, source) in claims.iter().enumerate() {
        if let Some(source) = source
            && let Some(index) = candidates.get(*source)
            && let Some(XmlChild::Element(element)) = node.children.get_mut(*index)
        {
            patch(element, entity);
        }
    }
    let mut drop: Vec<usize> = Vec::new();
    for (source, &index) in candidates.iter().enumerate() {
        if used.get(source).copied().unwrap_or(true) {
            continue;
        }
        let kept = match node.children.get(index) {
            Some(XmlChild::Element(element)) => {
                preserved.children.iter().any(|saved| saved == element)
            }
            _ => true,
        };
        if !kept {
            drop.push(index);
        }
    }
    remove_children(&mut node.children, &drop);
    for (entity, source) in claims.iter().enumerate() {
        if source.is_none() {
            let element = build(entity);
            insert_child(&mut node.children, element, rank, my_rank);
        }
    }
}

// ---------------------------------------------------------------------------
// Insertion ranks (MS-VSDX sequences)
// ---------------------------------------------------------------------------

/// The rank of a sheet-content child (`ShapeSheet_Type`: a free choice of
/// `Cell` and `Section`, with `Text`, `Data1`..`Data3`, `ForeignData` and
/// `Shapes` after them in Visio's own files).
fn sheet_rank(local: &str) -> u8 {
    match local {
        "Cell" => 1,
        "Section" => 2,
        "Shapes" => 4,
        _ => 3,
    }
}

/// The rank of a section child (`Cell` before `Row`).
fn section_rank(local: &str) -> u8 {
    match local {
        "Cell" => 1,
        _ => 2,
    }
}

/// The rank of a row child.
fn row_rank(_local: &str) -> u8 {
    1
}

/// The rank of a `Shapes` wrapper child.
fn shapes_rank(local: &str) -> u8 {
    match local {
        "Shape" => 1,
        _ => 2,
    }
}

/// The rank of a `Connects` wrapper child.
fn connects_rank(local: &str) -> u8 {
    match local {
        "Connect" => 1,
        _ => 2,
    }
}

/// The rank of a page-list child (`Pages_Type`: `Page*`).
fn pages_rank(local: &str) -> u8 {
    match local {
        "Page" => 1,
        _ => 2,
    }
}

/// The rank of a master-list child (`Masters_Type`: `Master*` then
/// `MasterShortcut*`).
fn masters_rank(local: &str) -> u8 {
    match local {
        "Master" => 1,
        _ => 2,
    }
}

/// The rank of a `Page` or `Master` child (`PageSheet` then `Rel` then
/// `Icon`).
fn page_rank(local: &str) -> u8 {
    match local {
        "PageSheet" => 1,
        "Rel" => 2,
        _ => 3,
    }
}

/// The rank of a `VisioDocument` child (the `VisioDocument_Type` sequence).
fn visio_document_rank(local: &str) -> u8 {
    match local {
        "DocumentProperties" => 1,
        "DocumentSettings" => 2,
        "Colors" => 3,
        "FaceNames" => 4,
        "StyleSheets" => 5,
        "DocumentSheet" => 6,
        "EventList" => 7,
        "HeaderFooter" => 8,
        "DataTransferInfo" => 9,
        "PublishSettings" => 10,
        "Comments" => 11,
        _ => 12,
    }
}

/// The rank of a contents-part child (`MasterContents_Type`: `Shapes` then
/// `Connects`).
fn contents_rank(local: &str) -> u8 {
    match local {
        "Shapes" => 1,
        "Connects" => 2,
        _ => 3,
    }
}

/// The rank of a list-wrapper child (`StyleSheets_Type`).
fn style_sheets_rank(local: &str) -> u8 {
    match local {
        "StyleSheet" => 1,
        _ => 2,
    }
}

// ---------------------------------------------------------------------------
// Sheet content: cells and sections
// ---------------------------------------------------------------------------

/// Patches one `Cell` element from a model cell.
fn patch_cell(node: &mut XmlNode, cell: &Cell) {
    sync_string(node, "N", cell.name.as_str(), "");
    sync_opt_string(node, "F", cell.formula.as_deref());
    match &cell.value {
        Some(CellValue::Error(error)) => {
            // The error takes the value slot; `V` and `U` stay preserved.
            sync_opt_string(node, "E", Some(error));
        }
        Some(value) => {
            sync_opt_string(node, "E", None);
            match current(node, "V") {
                Some(now) if value::value_matches(now, value) => {}
                _ => set(node, "V", value::encode_value(value)),
            }
            sync_unit(node, cell);
        }
        None => {
            sync_opt_string(node, "E", None);
            if current(node, "V").is_some() && !preserved_has(&cell.preserved, "V") {
                remove(node, "V");
            }
            sync_unit(node, cell);
        }
    }
}

/// Syncs the `U` attribute from the cached value and the cell's unit (see
/// [`value::unit_spelling`]): the code the value's kind needs to read back
/// (`BOOL`, `STR`) comes first, then the unit code. A code the file used
/// but the model does not spell stays in [`Preserved`](larrygram_model::Preserved)
/// and is never touched (the reader keeps it exactly when the writer would
/// spell something else over it, task P8-19): the attribute is removed only
/// when the model takes the spelling over.
fn sync_unit(node: &mut XmlNode, cell: &Cell) {
    if preserved_has(&cell.preserved, "U") {
        return;
    }
    let current_code = current(node, "U").map(str::to_owned);
    match value::unit_spelling(cell.value.as_ref(), cell.unit, current_code.as_deref()) {
        value::UnitSpelling::Keep => {}
        value::UnitSpelling::Code(code) => sync_string(node, "U", code, ""),
        value::UnitSpelling::Absent => {
            if current(node, "U").is_some() {
                remove(node, "U");
            }
        }
    }
}

/// Patches one `Row` element from a model row.
fn patch_row(node: &mut XmlNode, row: &Row) {
    sync_opt_string(node, "N", row.name.as_deref());
    sync_opt_string(node, "T", row.row_type.as_deref());
    sync_opt_int(node, "IX", row.ix);
}

/// The row key: `IX` when the row has one, else `N`, else nothing.
#[derive(Debug, PartialEq, Eq)]
enum RowKey {
    /// The row index.
    Ix(i32),
    /// The row name.
    Name(String),
}

/// The key of a `Row` element.
fn row_key(node: &XmlNode) -> Option<RowKey> {
    if let Some(ix) = current(node, "IX")
        && let Ok(ix) = ix.trim().parse::<i32>()
    {
        return Some(RowKey::Ix(ix));
    }
    current(node, "N").map(|name| RowKey::Name(name.to_owned()))
}

/// The key of a model row.
fn row_wanted(row: &Row) -> Option<RowKey> {
    match (row.ix, row.name.as_ref()) {
        (Some(ix), _) => Some(RowKey::Ix(ix)),
        (None, Some(name)) => Some(RowKey::Name(name.clone())),
        (None, None) => None,
    }
}

/// Patches the `Cell` children of a row.
fn patch_row_cells(node: &mut XmlNode, style: &NameStyle, row: &Row) {
    let keys: Vec<Option<String>> = row
        .cells
        .iter()
        .map(|c| Some(c.name.as_str().to_owned()))
        .collect();
    sync_children(
        node,
        |element| element.name.local == "Cell",
        |element| current(element, "N").map(str::to_owned),
        &keys,
        &row.preserved,
        |element, index| {
            if let Some(cell) = row.cells.get(index) {
                patch_cell(element, cell);
            }
        },
        |index| {
            row.cells.get(index).map_or_else(
                || emit::empty(style, "Cell"),
                |cell| emit::cell_element(style, cell),
            )
        },
        row_rank,
        1,
    );
}

/// The section key: the name plus the `IX` spelling, when the section has
/// one.
fn section_key(name: &str, ix: Option<&str>) -> Option<(String, Option<String>)> {
    Some((name.to_owned(), ix.map(str::to_owned)))
}

/// The `IX` spelling of a source `Section` element.
fn section_ix(node: &XmlNode) -> Option<&str> {
    current(node, "IX")
}

/// The `IX` spelling a model section kept in its preserved attributes.
fn model_section_ix(section: &Section) -> Option<String> {
    section
        .preserved
        .attributes
        .iter()
        .find(|attribute| attribute.name.local == "IX")
        .map(|attribute| attribute.value.clone())
}

/// Patches one `Section` element from a model section.
fn patch_section(node: &mut XmlNode, style: &NameStyle, section: &Section) {
    sync_string(node, "N", section.name.as_str(), "");
    let keys: Vec<Option<String>> = section
        .cells
        .iter()
        .map(|c| Some(c.name.as_str().to_owned()))
        .collect();
    sync_children(
        node,
        |element| element.name.local == "Cell",
        |element| current(element, "N").map(str::to_owned),
        &keys,
        &section.preserved,
        |element, index| {
            if let Some(cell) = section.cells.get(index) {
                patch_cell(element, cell);
            }
        },
        |index| {
            section.cells.get(index).map_or_else(
                || emit::empty(style, "Cell"),
                |cell| emit::cell_element(style, cell),
            )
        },
        section_rank,
        1,
    );
    let row_keys: Vec<Option<RowKey>> = section.rows.iter().map(row_wanted).collect();
    sync_children(
        node,
        |element| element.name.local == "Row",
        row_key,
        &row_keys,
        &section.preserved,
        |element, index| {
            if let Some(row) = section.rows.get(index) {
                patch_row(element, row);
                patch_row_cells(element, style, row);
            }
        },
        |index| {
            section.rows.get(index).map_or_else(
                || emit::empty(style, "Row"),
                |row| emit::row_element(style, row),
            )
        },
        section_rank,
        2,
    );
}

/// Patches the `Cell` and `Section` children of a sheet element (shape, page
/// sheet or stylesheet row).
fn patch_sheet(
    node: &mut XmlNode,
    style: &NameStyle,
    sheet: &larrygram_model::Sheet,
    preserved: &Preserved,
) {
    let cell_keys: Vec<Option<String>> = sheet
        .cells
        .iter()
        .map(|c| Some(c.name.as_str().to_owned()))
        .collect();
    sync_children(
        node,
        |element| element.name.local == "Cell",
        |element| current(element, "N").map(str::to_owned),
        &cell_keys,
        preserved,
        |element, index| {
            if let Some(cell) = sheet.cells.get(index) {
                patch_cell(element, cell);
            }
        },
        |index| {
            sheet.cells.get(index).map_or_else(
                || emit::empty(style, "Cell"),
                |cell| emit::cell_element(style, cell),
            )
        },
        sheet_rank,
        1,
    );
    let section_keys: Vec<Option<(String, Option<String>)>> = sheet
        .sections
        .iter()
        .map(|s| section_key(s.name.as_str(), model_section_ix(s).as_deref()))
        .collect();
    sync_children(
        node,
        |element| element.name.local == "Section",
        |element| current(element, "N").and_then(|name| section_key(name, section_ix(element))),
        &section_keys,
        preserved,
        |element, index| {
            if let Some(section) = sheet.sections.get(index) {
                patch_section(element, style, section);
            }
        },
        |index| {
            sheet.sections.get(index).map_or_else(
                || emit::empty(style, "Section"),
                |section| emit::section_element(style, section),
            )
        },
        sheet_rank,
        2,
    );
}

// ---------------------------------------------------------------------------
// Shapes
// ---------------------------------------------------------------------------

/// Patches the `Text` element of a shape from the model's edited text
/// (ADR 0019 write-back).
///
/// `Shape::text` is a typed copy of the `Text` element; the shape's
/// preserved fragment holds the element the model rebuilt after a
/// [`set_shape_text`](larrygram_model::Document::set_shape_text) edit.
/// When it differs from the tree's element, the tree takes it (keeping
/// the tree's spelling of the element name); when it is identical (an
/// unedited shape), nothing happens and the write is a no-op.
fn patch_text(node: &mut XmlNode, shape: &Shape) {
    let Some(edited) = shape
        .preserved
        .children
        .iter()
        .find(|child| child.name.local == "Text")
    else {
        return;
    };
    let existing = node.children.iter().position(
        |child| matches!(child, XmlChild::Element(element) if element.name.local == "Text"),
    );
    match existing {
        Some(index) => {
            if let Some(XmlChild::Element(element)) = node.children.get_mut(index)
                && element != edited
            {
                let mut replacement = edited.clone();
                replacement.name = element.name.clone();
                *element = replacement;
            }
        }
        None => {
            insert_child(&mut node.children, edited.clone(), sheet_rank, 3);
        }
    }
}

/// Patches one `Shape` element from a model shape, including its sub-shapes.
///
/// The recursion is bounded by the nesting depth of the source tree.
pub(crate) fn patch_shape(node: &mut XmlNode, ctx: &Ctx, shape: &Shape, depth: u32) {
    sync_int(node, "ID", shape.visio_id, 0);
    sync_string(node, "Name", &shape.name, "");
    sync_string(node, "NameU", &shape.name_u, "");
    let master_id = shape
        .master
        .and_then(|id| ctx.document.master(id))
        .map(|master| master.visio_id);
    sync_ref(node, "Master", master_id, &shape.preserved);
    sync_opt_uint(node, "LineStyle", shape.line_style);
    sync_opt_uint(node, "FillStyle", shape.fill_style);
    sync_opt_uint(node, "TextStyle", shape.text_style);
    patch_sheet(node, &ctx.style, &shape.sheet, &shape.preserved);
    patch_text(node, shape);
    if depth >= crate::package::Limits::DEFAULT.max_xml_depth {
        return;
    }
    let wrapper = node
        .children
        .iter_mut()
        .find(|child| matches!(child, XmlChild::Element(element) if element.name.local == "Shapes"))
        .and_then(|child| match child {
            XmlChild::Element(element) => Some(element),
            _ => None,
        });
    match wrapper {
        Some(wrapper) => patch_shapes(ctx, wrapper, &shape.children, &shape.preserved, depth),
        None => {
            if !shape.children.is_empty() {
                let wrapper = emit::shapes_wrapper(ctx.document, &ctx.style, &shape.children);
                insert_child(&mut node.children, wrapper, sheet_rank, 4);
            }
        }
    }
}

/// Patches the `Shape` children of a `Shapes` wrapper against the model's
/// shape list.
pub(crate) fn patch_shapes(
    ctx: &Ctx,
    wrapper: &mut XmlNode,
    ids: &[larrygram_model::ShapeId],
    preserved: &Preserved,
    depth: u32,
) {
    let keys: Vec<Option<i32>> = ids
        .iter()
        .map(|id| ctx.document.shape(*id).map(|shape| shape.visio_id))
        .collect();
    let next_depth = depth.saturating_add(1);
    sync_children(
        wrapper,
        |element| element.name.local == "Shape",
        // An `ID` the element omits is the model's default id of zero (the
        // reader maps it that way); one that is no signed id claims nothing
        // (the reader keeps such a `Shape` verbatim, task P8-19).
        |element| match current(element, "ID") {
            None => Some(0),
            Some(id) => id.trim().parse::<i32>().ok(),
        },
        &keys,
        preserved,
        |element, index| {
            if let Some(shape) = ids.get(index).and_then(|id| ctx.document.shape(*id)) {
                patch_shape(element, ctx, shape, next_depth);
            }
        },
        |index| {
            ids.get(index)
                .and_then(|id| ctx.document.shape(*id))
                .map_or_else(
                    || emit::empty(&ctx.style, "Shape"),
                    |shape| emit::shape_element(ctx.document, &ctx.style, shape),
                )
        },
        shapes_rank,
        1,
    );
}

// ---------------------------------------------------------------------------
// Connects, pages, masters, styles and properties
// ---------------------------------------------------------------------------

/// The `Connect` element's key: the record's own attribute tuple.
///
/// An attribute the element omits is the model's default for it (`0`, or
/// the empty cell name): Visio writes short `Connect` spellings like
/// `<Connect FromSheet="4" FromCell="BeginX" ToSheet="5"/>`, and a key
/// that demanded the omitted attributes would make the writer drop and
/// rebuild every such element (task P8-19 round-trip fault). An attribute
/// that is present but does not parse gives no key at all: the element is
/// unmapped and no model record claims it.
type ConnectKey = (i32, String, i32, i32, String, i32);

/// The key of a `Connect` element.
fn connect_key(node: &XmlNode) -> Option<ConnectKey> {
    let number = |local: &str| -> Option<i32> {
        match current(node, local) {
            None => Some(0),
            Some(value) => value.trim().parse::<i32>().ok(),
        }
    };
    let text = |local: &str| -> Option<String> {
        Some(current(node, local).unwrap_or_default().to_owned())
    };
    Some((
        number("FromSheet")?,
        text("FromCell")?,
        number("FromPart")?,
        number("ToSheet")?,
        text("ToCell")?,
        number("ToPart")?,
    ))
}

/// The attribute tuple of a model connect record.
fn connect_wanted(connect: &Connect) -> Option<ConnectKey> {
    Some((
        connect.from_sheet,
        connect.from_cell.clone(),
        connect.from_part,
        connect.to_sheet,
        connect.to_cell.clone(),
        connect.to_part,
    ))
}

/// Patches one `Connect` element from a model connect record.
fn patch_connect(node: &mut XmlNode, connect: &Connect) {
    sync_int(node, "FromSheet", connect.from_sheet, 0);
    sync_string(node, "FromCell", &connect.from_cell, "");
    sync_int(node, "FromPart", connect.from_part, 0);
    sync_int(node, "ToSheet", connect.to_sheet, 0);
    sync_string(node, "ToCell", &connect.to_cell, "");
    sync_int(node, "ToPart", connect.to_part, 0);
}

/// Patches the `Connect` children of a `Connects` wrapper.
pub(crate) fn patch_connects(
    wrapper: &mut XmlNode,
    style: &NameStyle,
    connects: &[Connect],
    preserved: &Preserved,
) {
    let keys: Vec<Option<ConnectKey>> = connects.iter().map(connect_wanted).collect();
    sync_children(
        wrapper,
        |element| element.name.local == "Connect",
        connect_key,
        &keys,
        preserved,
        |element, index| {
            if let Some(connect) = connects.get(index) {
                patch_connect(element, connect);
            }
        },
        |index| {
            connects.get(index).map_or_else(
                || emit::empty(style, "Connect"),
                |connect| emit::connect_element(style, connect),
            )
        },
        connects_rank,
        1,
    );
}

/// Patches one `Page` element of the page list.
pub(crate) fn patch_page(node: &mut XmlNode, ctx: &Ctx, page: &Page, rel_id: Option<&str>) {
    sync_int(node, "ID", page.visio_id, 0);
    sync_string(node, "Name", &page.name, "");
    sync_string(node, "NameU", &page.name_u, "");
    let background = page
        .background
        .and_then(|id| ctx.document.page(id))
        .map(|target| target.visio_id);
    sync_ref(node, "BackPage", background, &page.preserved);
    let sheet = node
        .children
        .iter_mut()
        .find(|child| matches!(child, XmlChild::Element(element) if element.name.local == "PageSheet"))
        .and_then(|child| match child {
            XmlChild::Element(element) => Some(element),
            _ => None,
        });
    if let Some(sheet) = sheet {
        patch_sheet(sheet, &ctx.style, &page.sheet, &page.preserved);
    }
    if let Some(rel_id) = rel_id {
        let has_rel = node.children.iter().any(|child| {
            matches!(child, XmlChild::Element(element) if element.name.local == "Rel"
                && current(element, "id") == Some(rel_id))
        });
        if !has_rel {
            let rel = emit::rel_element(&ctx.style, rel_id);
            insert_child(&mut node.children, rel, page_rank, 2);
        }
    }
}

/// Patches one `Master` element of the master list.
pub(crate) fn patch_master(node: &mut XmlNode, ctx: &Ctx, master: &Master, rel_id: Option<&str>) {
    sync_int(node, "ID", master.visio_id, 0);
    sync_string(node, "Name", &master.name, "");
    sync_string(node, "NameU", &master.name_u, "");
    sync_opt_string(node, "UniqueID", master.unique_id.as_deref());
    sync_opt_string(node, "Prompt", master.prompt.as_deref());
    sync_bool(node, "Hidden", master.hidden);
    if let Some(rel_id) = rel_id {
        let has_rel = node.children.iter().any(|child| {
            matches!(child, XmlChild::Element(element) if element.name.local == "Rel"
                && current(element, "id") == Some(rel_id))
        });
        if !has_rel {
            let rel = emit::rel_element(&ctx.style, rel_id);
            insert_child(&mut node.children, rel, page_rank, 2);
        }
    }
}

/// Patches one `StyleSheet` element of the stylesheet.
pub(crate) fn patch_style(node: &mut XmlNode, style: &NameStyle, row: &Style) {
    sync_uint(node, "ID", row.visio_id, 0);
    sync_string(node, "Name", &row.name, "");
    sync_string(node, "NameU", &row.name_u, "");
    sync_opt_uint(node, "LineStyle", row.line_style);
    sync_opt_uint(node, "FillStyle", row.fill_style);
    sync_opt_uint(node, "TextStyle", row.text_style);
    patch_sheet(node, style, &row.sheet, &row.preserved);
}

/// Patches the `StyleSheet` children of a `StyleSheets` wrapper.
pub(crate) fn patch_styles(wrapper: &mut XmlNode, ctx: &Ctx, preserved: &Preserved) {
    let keys: Vec<Option<u32>> = ctx
        .document
        .stylesheet
        .styles
        .iter()
        .map(|style| Some(style.visio_id))
        .collect();
    sync_children(
        wrapper,
        |element| element.name.local == "StyleSheet",
        |element| current(element, "ID").and_then(|id| id.trim().parse::<u32>().ok()),
        &keys,
        preserved,
        |element, index| {
            if let Some(row) = ctx.document.stylesheet.styles.get(index) {
                patch_style(element, &ctx.style, row);
            }
        },
        |index| {
            ctx.document.stylesheet.styles.get(index).map_or_else(
                || emit::empty(&ctx.style, "StyleSheet"),
                |style| emit::style_element(&ctx.style, style),
            )
        },
        style_sheets_rank,
        1,
    );
}

/// The property pool shared by the `docProps` parts and the main part's
/// `DocumentProperties`, claimed in the order the reader mapped them.
pub(crate) struct PropertyPool<'d> {
    /// The document properties.
    properties: &'d [Property],
    /// Which properties a source element already claimed.
    used: Vec<bool>,
}

impl<'d> PropertyPool<'d> {
    /// A pool over these properties.
    pub(crate) fn new(properties: &'d [Property]) -> Self {
        Self {
            properties,
            used: vec![false; properties.len()],
        }
    }

    /// The indices of the properties no source element claimed yet. They are
    /// new: a `Property` records no source part (ADR 0018), so the writer
    /// gives them a home in the main part's `DocumentProperties`.
    pub(crate) fn unclaimed_indices(&self) -> Vec<usize> {
        (0..self.properties.len())
            .filter(|entity| !self.used.get(*entity).copied().unwrap_or(true))
            .collect()
    }

    /// Records a property as written.
    pub(crate) fn claim(&mut self, entity: usize) {
        if let Some(slot) = self.used.get_mut(entity) {
            *slot = true;
        }
    }
}

/// Whether the element is a simple property element (the reader maps every
/// child without element children to a [`Property`]).
fn is_property(element: &XmlNode) -> bool {
    !element
        .children
        .iter()
        .any(|child| matches!(child, XmlChild::Element(_)))
}

/// Patches the property children of a property container.
///
/// Properties claim by name and value first, then by name alone: a value the
/// model changed still finds its element. The text is rewritten only when it
/// differs, so a property element's comments and spelling survive untouched
/// writes.
pub(crate) fn patch_properties(node: &mut XmlNode, pool: &mut PropertyPool, preserved: &Preserved) {
    let candidates: Vec<usize> = node
        .children
        .iter()
        .enumerate()
        .filter(|(_, child)| matches!(child, XmlChild::Element(element) if is_property(element)))
        .map(|(index, _)| index)
        .collect();
    let sources: Vec<(String, String)> = candidates
        .iter()
        .filter_map(|&index| match node.children.get(index) {
            Some(XmlChild::Element(element)) => {
                Some((element.name.local.clone(), text_content(element)))
            }
            _ => None,
        })
        .collect();

    // Pass 1: name and value; pass 2: name alone, over what is left.
    let mut claims: Vec<Option<usize>> = vec![None; pool.properties.len()];
    for pass in 0..2 {
        for (entity, property) in pool.properties.iter().enumerate() {
            if claims.get(entity).and_then(Option::as_ref).is_some()
                || pool.used.get(entity).copied().unwrap_or(true)
            {
                continue;
            }
            let hit = candidates.iter().enumerate().find(|(source, _)| {
                if claims.contains(&Some(*source)) {
                    return false;
                }
                let Some((name, value)) = sources.get(*source) else {
                    return false;
                };
                if name != &property.name {
                    return false;
                }
                pass == 0 && value == &property.value || pass == 1
            });
            if let Some((source, _)) = hit {
                if let Some(claim) = claims.get_mut(entity) {
                    *claim = Some(source);
                }
                if let Some(slot) = pool.used.get_mut(entity) {
                    *slot = true;
                }
            }
        }
    }

    for (entity, source) in claims.iter().enumerate() {
        let Some(source) = source else {
            continue;
        };
        let Some(index) = candidates.get(*source) else {
            continue;
        };
        let Some(property) = pool.properties.get(entity) else {
            continue;
        };
        if let Some(XmlChild::Element(element)) = node.children.get_mut(*index)
            && text_content(element) != property.value
        {
            element
                .children
                .retain(|child| !matches!(child, XmlChild::Text(_)));
            if !property.value.is_empty() {
                element
                    .children
                    .push(XmlChild::Text(property.value.clone()));
            }
            element.self_closing = element.children.is_empty();
        }
    }

    let mut drop: Vec<usize> = Vec::new();
    for (source, &index) in candidates.iter().enumerate() {
        if claims.contains(&Some(source)) {
            continue;
        }
        let kept = match node.children.get(index) {
            Some(XmlChild::Element(element)) => preserved.children.contains(element),
            _ => true,
        };
        if !kept {
            drop.push(index);
        }
    }
    remove_children(&mut node.children, &drop);
}

/// The rank of a property container child.
fn property_rank(local: &str) -> u8 {
    match local {
        "DocumentProperties" | "CustomProps" | "HeadingPairs" | "TitlesOfParts" => 2,
        _ => 1,
    }
}

// ---------------------------------------------------------------------------
// Part roots
// ---------------------------------------------------------------------------

/// Drops the generated `Rel` child of a new list entity that has no
/// contents relationship.
///
/// An entity without content and without a preserved `Rel` must get none: a
/// stray `Rel` would name a relationship that does not exist, the next read
/// would keep it as preserved content, and the next write would grow a
/// contents part for it (`write -> read -> write` would not be stable).
fn without_stray_rel(mut node: XmlNode, rel: Option<&str>) -> XmlNode {
    if rel.is_none() {
        node.children.retain(
            |child| !matches!(child, XmlChild::Element(element) if element.name.local == "Rel"),
        );
    }
    node
}

/// Patches the `Pages` root of a page list part.
///
/// `rel_ids` names the relationship of each page (`None` when the page has
/// no contents part and needs no `Rel`).
pub(crate) fn patch_pages_root(root: &mut XmlNode, ctx: &Ctx, rel_ids: &[Option<String>]) {
    let keys: Vec<Option<i32>> = ctx
        .document
        .pages
        .iter()
        .map(|page| Some(page.visio_id))
        .collect();
    sync_children(
        root,
        |element| element.name.local == "Page",
        |element| current(element, "ID").and_then(|id| id.trim().parse::<i32>().ok()),
        &keys,
        &ctx.document.preserved,
        |element, index| {
            if let Some(page) = ctx.document.pages.get(index) {
                let rel = rel_ids.get(index).and_then(Option::as_deref);
                patch_page(element, ctx, page, rel);
            }
        },
        |index| {
            ctx.document.pages.get(index).map_or_else(
                || emit::empty(&ctx.style, "Page"),
                |page| {
                    let rel = rel_ids.get(index).and_then(Option::as_deref);
                    let rel_id = rel.map_or_else(|| "rId1".to_owned(), str::to_owned);
                    without_stray_rel(
                        emit::page_element(ctx.document, &ctx.style, page, &rel_id),
                        rel,
                    )
                },
            )
        },
        pages_rank,
        1,
    );
}

/// Patches the `Masters` root of a master list part.
pub(crate) fn patch_masters_root(root: &mut XmlNode, ctx: &Ctx, rel_ids: &[Option<String>]) {
    let keys: Vec<Option<i32>> = ctx
        .document
        .masters
        .iter()
        .map(|master| Some(master.visio_id))
        .collect();
    sync_children(
        root,
        |element| element.name.local == "Master",
        |element| current(element, "ID").and_then(|id| id.trim().parse::<i32>().ok()),
        &keys,
        &ctx.document.preserved,
        |element, index| {
            if let Some(master) = ctx.document.masters.get(index) {
                let rel = rel_ids.get(index).and_then(Option::as_deref);
                patch_master(element, ctx, master, rel);
            }
        },
        |index| {
            ctx.document.masters.get(index).map_or_else(
                || emit::empty(&ctx.style, "Master"),
                |master| {
                    let rel = rel_ids.get(index).and_then(Option::as_deref);
                    let rel_id = rel.map_or_else(|| "rId1".to_owned(), str::to_owned);
                    without_stray_rel(emit::master_element(&ctx.style, master, &rel_id), rel)
                },
            )
        },
        masters_rank,
        1,
    );
}

/// Patches the `PageContents` root of a page contents part.
pub(crate) fn patch_page_contents(root: &mut XmlNode, ctx: &Ctx, page: &Page) {
    patch_contents_root(
        root,
        ctx,
        &page.shapes,
        Some(&page.connects),
        &page.preserved,
    );
}

/// Patches the `MasterContents` root of a master contents part.
pub(crate) fn patch_master_contents(root: &mut XmlNode, ctx: &Ctx, master: &Master) {
    patch_contents_root(root, ctx, &master.shapes, None, &master.preserved);
}

/// Patches the `Shapes` (and `Connects`) wrappers of a contents root.
fn patch_contents_root(
    root: &mut XmlNode,
    ctx: &Ctx,
    ids: &[larrygram_model::ShapeId],
    connects: Option<&[Connect]>,
    preserved: &Preserved,
) {
    let wrapper = root
        .children
        .iter_mut()
        .find(|child| matches!(child, XmlChild::Element(element) if element.name.local == "Shapes"))
        .and_then(|child| match child {
            XmlChild::Element(element) => Some(element),
            _ => None,
        });
    match wrapper {
        Some(wrapper) => patch_shapes(ctx, wrapper, ids, preserved, 0),
        None => {
            if !ids.is_empty() {
                let wrapper = emit::shapes_wrapper(ctx.document, &ctx.style, ids);
                insert_child(&mut root.children, wrapper, contents_rank, 1);
            }
        }
    }
    let Some(connects) = connects else {
        return;
    };
    let wrapper = root
        .children
        .iter_mut()
        .find(
            |child| matches!(child, XmlChild::Element(element) if element.name.local == "Connects"),
        )
        .and_then(|child| match child {
            XmlChild::Element(element) => Some(element),
            _ => None,
        });
    match wrapper {
        Some(wrapper) => patch_connects(wrapper, &ctx.style, connects, preserved),
        None => {
            if !connects.is_empty() {
                let wrapper = emit::connects_wrapper(&ctx.style, connects);
                insert_child(&mut root.children, wrapper, contents_rank, 2);
            }
        }
    }
}

/// Patches the `VisioDocument` root of the main part.
///
/// The `DocumentProperties` wrapper holds the properties no `docProps` part
/// claimed (a `Property` records no source part); the wrapper is created only
/// when there are such properties, so an unchanged write changes nothing.
pub(crate) fn patch_visio_document(root: &mut XmlNode, ctx: &Ctx, pool: &mut PropertyPool) {
    if let Some(XmlChild::Element(wrapper)) = root.children.iter_mut().find(|child| {
        matches!(child, XmlChild::Element(element) if element.name.local == "DocumentProperties")
    }) {
        patch_properties(wrapper, pool, &ctx.document.preserved);
    }
    let pending: Vec<usize> = pool.unclaimed_indices();
    if !pending.is_empty() {
        // A new property takes the namespace spelling of a sibling property
        // (a `Property` keeps only its local name, ADR 0018), else the Visio
        // namespace of `DocumentProperties`.
        let template = property_template(root);
        if !root.children.iter().any(|child| {
            matches!(child, XmlChild::Element(element) if element.name.local == "DocumentProperties")
        }) {
            let fresh = emit::empty(&ctx.style, "DocumentProperties");
            insert_child(&mut root.children, fresh, visio_document_rank, 1);
        }
        if let Some(XmlChild::Element(wrapper)) = root.children.iter_mut().find(|child| {
            matches!(child, XmlChild::Element(element)
                if element.name.local == "DocumentProperties")
        }) {
            for entity in pending {
                pool.claim(entity);
                if let Some(property) = pool.properties.get(entity) {
                    let mut element = emit::property_element(&ctx.style, property);
                    if let Some(template) = &template {
                        element.name = larrygram_model::QName {
                            prefix: template.prefix.clone(),
                            uri: template.uri.clone(),
                            local: property.name.clone(),
                        };
                    }
                    insert_child(&mut wrapper.children, element, property_rank, 1);
                }
            }
        }
    }
    if let Some(XmlChild::Element(wrapper)) = root.children.iter_mut().find(
        |child| matches!(child, XmlChild::Element(element) if element.name.local == "StyleSheets"),
    ) {
        patch_styles(wrapper, ctx, &ctx.document.preserved);
    } else if !ctx.document.stylesheet.styles.is_empty() {
        let mut wrapper = emit::empty(&ctx.style, "StyleSheets");
        for style in &ctx.document.stylesheet.styles {
            wrapper
                .children
                .push(XmlChild::Element(emit::style_element(&ctx.style, style)));
        }
        emit::finish(&mut wrapper);
        insert_child(&mut root.children, wrapper, visio_document_rank, 5);
    }
}

/// The namespace spelling of the first property element of a property
/// container, for a new property element.
fn property_template(root: &XmlNode) -> Option<larrygram_model::QName> {
    for child in &root.children {
        let XmlChild::Element(container) = child else {
            continue;
        };
        for property in &container.children {
            if let XmlChild::Element(property) = property
                && is_property(property)
            {
                return Some(larrygram_model::QName {
                    prefix: property.name.prefix.clone(),
                    uri: property.name.uri.clone(),
                    local: String::new(),
                });
            }
        }
        if is_property(container) {
            return Some(larrygram_model::QName {
                prefix: container.name.prefix.clone(),
                uri: container.name.uri.clone(),
                local: String::new(),
            });
        }
    }
    None
}

/// Patches the property root of a `docProps` part.
pub(crate) fn patch_properties_root(
    root: &mut XmlNode,
    pool: &mut PropertyPool,
    preserved: &Preserved,
) {
    patch_properties(root, pool, preserved);
}

#[cfg(test)]
mod tests {
    use larrygram_model::{Cell, CellName, CellValue, Preserved, Row, XmlAttribute};
    use larrygram_model::{QName, XmlChild, XmlNode};

    use super::*;

    /// An element with these attributes and raw children.
    fn raw(local: &str, attrs: &[(&str, &str)], children: Vec<XmlChild>) -> XmlNode {
        XmlNode {
            name: QName::local(local),
            attributes: attrs
                .iter()
                .map(|(name, value)| XmlAttribute {
                    name: QName::local(*name),
                    value: (*value).to_owned(),
                })
                .collect(),
            children,
            self_closing: false,
        }
    }

    /// An element with these attributes and child elements.
    fn element(local: &str, attrs: &[(&str, &str)], children: Vec<XmlNode>) -> XmlNode {
        raw(
            local,
            attrs,
            children.into_iter().map(XmlChild::Element).collect(),
        )
    }

    /// An element with one text child.
    fn text_element(local: &str, attrs: &[(&str, &str)], text: &str) -> XmlNode {
        let mut node = element(local, attrs, Vec::new());
        node.children.push(XmlChild::Text(text.to_owned()));
        node
    }

    /// A cell element.
    fn cell(attrs: &[(&str, &str)]) -> XmlNode {
        element("Cell", attrs, Vec::new())
    }

    #[test]
    fn sync_string_keeps_a_default_absent() {
        let mut node = element("X", &[], Vec::new());
        sync_string(&mut node, "Name", "", "");
        assert!(current(&node, "Name").is_none());
        sync_string(&mut node, "Name", "Box", "");
        assert_eq!(current(&node, "Name"), Some("Box"));
        sync_string(&mut node, "Name", "Box", "");
        assert_eq!(node.attributes.len(), 1);
        sync_string(&mut node, "Name", "", "");
        assert_eq!(current(&node, "Name"), Some(""));
    }

    #[test]
    fn sync_opt_string_sets_updates_and_removes() {
        let mut node = element("X", &[("UniqueID", "a")], Vec::new());
        sync_opt_string(&mut node, "UniqueID", None);
        assert!(current(&node, "UniqueID").is_none());
        sync_opt_string(&mut node, "UniqueID", Some("b"));
        assert_eq!(current(&node, "UniqueID"), Some("b"));
        sync_opt_string(&mut node, "Prompt", None);
        assert!(current(&node, "Prompt").is_none());
    }

    #[test]
    fn sync_int_keeps_a_matching_spelling() {
        let mut node = element("X", &[("ID", "007")], Vec::new());
        sync_int(&mut node, "ID", 7, 0);
        assert_eq!(current(&node, "ID"), Some("007"));
        sync_int(&mut node, "ID", 8, 0);
        assert_eq!(current(&node, "ID"), Some("8"));
        let mut node = element("X", &[], Vec::new());
        sync_int(&mut node, "ID", 0, 0);
        assert!(current(&node, "ID").is_none());
        sync_int(&mut node, "ID", 3, 0);
        assert_eq!(current(&node, "ID"), Some("3"));
    }

    #[test]
    fn sync_opt_int_and_uint_remove_when_absent() {
        let mut node = element("X", &[("IX", "4"), ("LineStyle", "2")], Vec::new());
        sync_opt_int(&mut node, "IX", None);
        sync_opt_uint(&mut node, "LineStyle", None);
        assert!(current(&node, "IX").is_none());
        assert!(current(&node, "LineStyle").is_none());
        sync_opt_int(&mut node, "IX", Some(1));
        sync_opt_uint(&mut node, "LineStyle", Some(0));
        assert_eq!(current(&node, "IX"), Some("1"));
        assert_eq!(current(&node, "LineStyle"), Some("0"));
    }

    #[test]
    fn sync_uint_keeps_a_matching_spelling() {
        let mut node = element("X", &[("ID", "0007")], Vec::new());
        sync_uint(&mut node, "ID", 7, 0);
        assert_eq!(current(&node, "ID"), Some("0007"));
    }

    /// P5-20 open item, closed by P6-11: the overwrite and set-new
    /// arms of `sync_uint` (a different spelling is replaced, a missing
    /// attribute is set when the value is not the default and left
    /// absent when it is).
    #[test]
    fn sync_uint_overwrites_a_different_spelling_and_sets_the_new_one() {
        let mut node = element("X", &[("ID", "3")], Vec::new());
        sync_uint(&mut node, "ID", 7, 0);
        assert_eq!(current(&node, "ID"), Some("7"));
        let mut node = element("X", &[], Vec::new());
        sync_uint(&mut node, "ID", 7, 0);
        assert_eq!(current(&node, "ID"), Some("7"));
        let mut node = element("X", &[], Vec::new());
        sync_uint(&mut node, "ID", 0, 0);
        assert!(current(&node, "ID").is_none(), "the default stays absent");
    }

    #[test]
    fn sync_bool_writes_only_what_changed() {
        let mut node = element("X", &[("Hidden", "true")], Vec::new());
        sync_bool(&mut node, "Hidden", true);
        assert_eq!(current(&node, "Hidden"), Some("true"));
        sync_bool(&mut node, "Hidden", false);
        assert_eq!(current(&node, "Hidden"), Some("0"));
        let mut node = element("X", &[], Vec::new());
        sync_bool(&mut node, "Hidden", false);
        assert!(current(&node, "Hidden").is_none());
        sync_bool(&mut node, "Hidden", true);
        assert_eq!(current(&node, "Hidden"), Some("1"));
    }

    #[test]
    fn sync_ref_leaves_an_unresolved_reference_alone() {
        let mut preserved = Preserved::new();
        preserved.attributes.push(XmlAttribute {
            name: QName::local("Master"),
            value: "9".to_owned(),
        });
        let mut node = element("X", &[("Master", "9")], Vec::new());
        sync_ref(&mut node, "Master", None, &preserved);
        assert_eq!(current(&node, "Master"), Some("9"));
        // A cleared reference (not preserved) is removed.
        sync_ref(&mut node, "Master", None, &Preserved::new());
        assert!(current(&node, "Master").is_none());
        sync_ref(&mut node, "Master", Some(3), &Preserved::new());
        assert_eq!(current(&node, "Master"), Some("3"));
    }

    #[test]
    fn claim_matches_by_key_and_positionally() {
        let sources = vec![Some("a"), Some("b"), None, Some("a")];
        let wanted = vec![Some("a"), None, Some("c"), Some("a")];
        let mut used = vec![false; sources.len()];
        let claims = claim(&sources, &wanted, &mut used);
        assert_eq!(claims, vec![Some(0), Some(2), None, Some(3)]);
    }

    #[test]
    fn remove_children_drops_orphaned_whitespace() {
        let mut children = vec![
            XmlChild::Text("\n  ".to_owned()),
            XmlChild::Element(element("X", &[], Vec::new())),
            XmlChild::Text("\n".to_owned()),
        ];
        remove_children(&mut children, &[1]);
        assert!(children.is_empty());
        // Whitespace next to surviving text is significant (ADR 0012).
        let mut children = vec![
            XmlChild::Text("\n  ".to_owned()),
            XmlChild::Element(element("X", &[], Vec::new())),
            XmlChild::Text("text".to_owned()),
        ];
        remove_children(&mut children, &[1]);
        assert_eq!(children.len(), 2);
    }

    #[test]
    fn remove_children_keeps_blank_text_when_nothing_was_dropped() {
        // Blank text with no element sibling is the element's own content
        // (ADR 0012 whitespace rule: only element-like content makes
        // whitespace ignorable).
        let mut children = vec![XmlChild::Text("   ".to_owned())];
        remove_children(&mut children, &[]);
        assert_eq!(children, vec![XmlChild::Text("   ".to_owned())]);
    }

    #[test]
    fn insert_child_follows_the_rank_order() {
        let mut children = vec![
            XmlChild::Element(element("Cell", &[], Vec::new())),
            XmlChild::Element(element("Text", &[], Vec::new())),
        ];
        insert_child(
            &mut children,
            element("Cell", &[], Vec::new()),
            sheet_rank,
            1,
        );
        insert_child(
            &mut children,
            element("Section", &[], Vec::new()),
            sheet_rank,
            2,
        );
        let names: Vec<&str> = children
            .iter()
            .map(|child| match child {
                XmlChild::Element(element) => element.name.local.as_str(),
                _ => "?",
            })
            .collect();
        assert_eq!(names, ["Cell", "Cell", "Section", "Text"]);
    }

    #[test]
    fn insert_child_with_no_anchor_goes_first_among_later_kinds() {
        let mut children = vec![XmlChild::Element(element("Text", &[], Vec::new()))];
        insert_child(
            &mut children,
            element("Cell", &[], Vec::new()),
            sheet_rank,
            1,
        );
        match &children[0] {
            XmlChild::Element(element) => assert_eq!(element.name.local, "Cell"),
            _ => panic!("not an element"),
        }
    }

    #[test]
    fn a_cell_patches_formula_value_unit_and_error() {
        let mut model = Cell::new(CellName::from_static("PinX"));
        model.formula = Some("2*2".to_owned());
        model.value = Some(CellValue::Number(4.0));
        model.unit = larrygram_geom::Unit::Inches;
        let mut node = cell(&[("N", "PinX"), ("V", "4.5")]);
        patch_cell(&mut node, &model);
        assert_eq!(current(&node, "V"), Some("4"));
        assert_eq!(current(&node, "F"), Some("2*2"));
        assert_eq!(current(&node, "U"), Some("IN"));
        // An unchanged spelling is kept.
        let mut node = cell(&[("N", "PinX"), ("V", "4.0")]);
        patch_cell(&mut node, &model);
        assert_eq!(current(&node, "V"), Some("4.0"));
    }

    #[test]
    fn an_error_value_keeps_v_and_u_preserved() {
        let mut model = Cell::new(CellName::from_static("Value"));
        model.value = Some(CellValue::Error("#VALUE!".to_owned()));
        model.preserved.attributes.push(XmlAttribute {
            name: QName::local("V"),
            value: "keep".to_owned(),
        });
        let mut node = cell(&[("N", "Value"), ("V", "keep"), ("E", "#VALUE!")]);
        patch_cell(&mut node, &model);
        assert_eq!(current(&node, "E"), Some("#VALUE!"));
        assert_eq!(current(&node, "V"), Some("keep"));
    }

    #[test]
    fn a_cleared_value_removes_v_and_keeps_a_preserved_u() {
        let mut model = Cell::new(CellName::from_static("Value"));
        model.preserved.attributes.push(XmlAttribute {
            name: QName::local("U"),
            value: "DA".to_owned(),
        });
        let mut node = cell(&[("N", "Value"), ("V", "3"), ("U", "DA"), ("E", "x")]);
        patch_cell(&mut node, &model);
        assert!(current(&node, "V").is_none());
        assert!(current(&node, "E").is_none());
        assert_eq!(current(&node, "U"), Some("DA"));
    }

    #[test]
    fn rows_claim_by_ix_then_name() {
        let mut row = Row::new();
        row.ix = Some(2);
        row.row_type = Some("RelMoveTo".to_owned());
        let mut section =
            larrygram_model::Section::new(larrygram_model::SectionName::from_static("Geometry"));
        section.rows.push(row);
        let mut other = Row::new();
        other.name = Some("Row_1".to_owned());
        section.rows.push(other);
        let mut node = element(
            "Section",
            &[("N", "Geometry")],
            vec![
                element("Row", &[("N", "Row_1"), ("T", "old")], Vec::new()),
                element("Row", &[("IX", "2")], Vec::new()),
            ],
        );
        patch_section(&mut node, &NameStyle::default(), &section);
        let rows: Vec<&XmlNode> = node
            .children
            .iter()
            .filter_map(|child| match child {
                XmlChild::Element(element) if element.name.local == "Row" => Some(element),
                _ => None,
            })
            .collect();
        assert_eq!(rows.len(), 2);
        assert_eq!(current(rows[0], "T"), None);
        assert_eq!(current(rows[1], "T"), Some("RelMoveTo"));
    }

    #[test]
    fn an_unclaimed_cell_is_kept_only_when_preserved() {
        let mut preserved = Preserved::new();
        let kept = cell(&[("V", "no name")]);
        preserved.children.push(kept.clone());
        let sheet = larrygram_model::Sheet::new();
        let mut node = raw(
            "Shape",
            &[],
            vec![
                XmlChild::Element(kept),
                XmlChild::Element(cell(&[("N", "Gone")])),
            ],
        );
        patch_sheet(&mut node, &NameStyle::default(), &sheet, &preserved);
        let names: Vec<Option<&str>> = node
            .children
            .iter()
            .filter_map(|child| match child {
                XmlChild::Element(element) if element.name.local == "Cell" => {
                    Some(current(element, "N"))
                }
                _ => None,
            })
            .collect();
        assert_eq!(names, vec![None]);
    }

    #[test]
    fn properties_claim_by_value_then_name_and_rewrite_only_changed_text() {
        let properties = [Property::new("title", "New")];
        let mut pool = PropertyPool::new(&properties);
        let mut node = element(
            "Properties",
            &[],
            vec![
                text_element("title", &[], "New"),
                text_element("subject", &[], "Old"),
            ],
        );
        patch_properties(&mut node, &mut pool, &Preserved::new());
        let properties: Vec<&XmlNode> = node
            .children
            .iter()
            .filter_map(|child| match child {
                XmlChild::Element(element) if is_property(element) => Some(element),
                _ => None,
            })
            .collect();
        assert_eq!(properties.len(), 1);
        assert_eq!(text_content(properties[0]), "New");

        // A changed value finds its element by name and is rewritten.
        let changed = [Property::new("title", "Changed")];
        let mut pool = PropertyPool::new(&changed);
        let mut node = element("Properties", &[], vec![text_element("title", &[], "Old")]);
        patch_properties(&mut node, &mut pool, &Preserved::new());
        let title = node
            .children
            .iter()
            .find_map(|child| match child {
                XmlChild::Element(element) if element.name.local == "title" => Some(element),
                _ => None,
            })
            .expect("title");
        assert_eq!(text_content(title), "Changed");
    }

    #[test]
    fn an_element_property_is_never_claimed_or_dropped() {
        let mut preserved = Preserved::new();
        let pairs = element(
            "HeadingPairs",
            &[],
            vec![element("vt:vector", &[], Vec::new())],
        );
        preserved.children.push(pairs.clone());
        let mut pool = PropertyPool::new(&[]);
        let mut node = element("Properties", &[], vec![pairs]);
        patch_properties(&mut node, &mut pool, &preserved);
        assert_eq!(node.children.len(), 1);
    }
}
