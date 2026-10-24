// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Pages, masters, stencils and the document properties: chunk records
//! into the model (ADR 0070).
//!
//! This module reads the page records (pages, page sheets, page
//! properties, background pages), the master records (stencils,
//! stencil pages, master shapes, names) and the document properties,
//! and maps them onto [`larrygram_model::Page`],
//! [`larrygram_model::Master`], [`larrygram_model::Property`] and
//! [`larrygram_model::DocumentKind`].
//!
//! The document kind comes from the records (a document of stencil
//! pages and no drawing pages is a stencil), not from the file name, so
//! `.vsd`, `.vss` and `.vst` all import through the same path
//! (ADR 0016).
//!
//! # What maps where
//!
//! | Record | Model |
//! |---|---|
//! | page (`0x15`) | one [`Page`]: the background-page reference and, from the name index, the name and `visio_id` |
//! | stencil page (`0x1e`) | one [`Master`]: the record id is its `visio_id`, the name index names it |
//! | page properties (`0x92`) under a page | the page sheet's `PageWidth`, `PageHeight`, `ShdwOffsetX`, `ShdwOffsetY`, `PageScale` and `DrawingScale` cells |
//! | name lists (`0x2c`, `0x32`) and names (`0x2d`, `0x33`) | the `name` and `name_u` of pages and masters |
//! | name indexes (`0xc9`, `0x34`) | the element id to name id map the names resolve through |
//! | the shape records of a page or stencil page | [`Page::shapes`] and [`Master::shapes`] (the arena ids of the top-level shapes) |
//! | document properties (the summary streams) | [`larrygram_model::Document::properties`] |
//!
//! # Names and ids
//!
//! A name index maps element ids to name ids at one nesting level. The
//! name indexes that name pages and masters sit at the level of the
//! page and stencil-page records themselves; deeper indexes name shape
//! and property rows and are not consulted here. A master takes the
//! entry whose element is its record id (the id its shapes' master
//! references carry); a page takes the first entry no master claimed
//! and that entry's element is the page's Visio id, exactly the `ID`
//! the `.vsdx` reader reads from the `Page` element. A container no
//! index names is an [`WarningKind::UnresolvedReference`] warning, as
//! is a name id the name lists do not hold (ADR 0071). The binary
//! format carries one name per element, so [`larrygram_model::Page::name`]
//! and [`larrygram_model::Page::name_u`] receive the same string.
//!
//! # Page properties
//!
//! The page-properties record holds six unit-tagged doubles (a unit
//! byte naming the *display* unit, then the value; the same spelling
//! [`crate::shape`] maps) in the order of
//! the `Page Properties` row's cell indices (Microsoft Learn,
//! `VisCellIndices`): `PageWidth`, `PageHeight`, `ShdwOffsetX`,
//! `ShdwOffsetY`, `PageScale`, then `DrawingScale` behind its own unit
//! byte. The size pair is scaled to inches by `PageScale`/`DrawingScale`
//! (see [`read_page_props`]); the shadow offsets and the two scale cells
//! keep their raw values. Fields the record carries after them have no
//! model slot on the page sheet and stay unset. A master's page sheet
//! has no model slot at all ([`larrygram_model::Master`] holds no
//! sheet), so its page properties are dropped with a warning.
//!
//! # Connects
//!
//! The connect list (`0x72`) groups the connect records, but no source
//! this module may read (AGENTS.md section 5) spells the connect entry
//! payload, and no reference `.vsd` file holds one. A connect list is
//! skipped with a [`WarningKind::SkippedRecord`] warning and
//! [`larrygram_model::Page::connects`] stays empty; see the task
//! result's open issues.
//!
//! # Degrading, never crashing
//!
//! A record this module does not consume inside a page or master
//! (page-layout, print, ruler, user-cell and icon records and the rest)
//! is skipped with a [`WarningKind::SkippedRecord`] warning; the
//! document properties record (`0xbc`) and a master's page sheet are
//! dropped with a [`WarningKind::Dropped`] warning. Records inside a
//! shape record group belong to [`crate::shape`], [`crate::text`] and
//! [`crate::style`] and stay silent here. A truncated payload keeps
//! what parsed and warns [`WarningKind::BadValue`]; every read is
//! bounds-checked (AGENTS.md section 6).

use std::collections::BTreeMap;

use larrygram_geom::Unit;
use larrygram_model::{
    Cell, CellName, CellValue, Document, DocumentKind, Master, MasterId, Page, PageId, Property,
    ShapeId,
};

use crate::chunks::{Chunk, FormatVersion, Record};
use crate::container::{Streams, SummaryProperty, SummaryValue};
use crate::error::Error;
use crate::warning::{ImportWarning, WarningKind};

/// The pages list.
const PAGES: Record = Record(0x27);
/// One drawing page.
const PAGE: Record = Record(0x15);
/// The stencils list.
const STENCILS: Record = Record(0x1d);
/// One stencil page (a master's page).
const STENCIL_PAGE: Record = Record(0x1e);
/// A page sheet (a page's or master's ShapeSheet).
const PAGE_SHEET: Record = Record(0x46);
/// The page properties row.
const PAGE_PROPS: Record = Record(0x92);
/// A group shape record.
const SHAPE_GROUP: Record = Record(0x47);
/// A shape record.
const SHAPE_SHAPE: Record = Record(0x48);
/// A guide shape record.
const SHAPE_GUIDE: Record = Record(0x4d);
/// A foreign shape record.
const SHAPE_FOREIGN: Record = Record(0x4e);
/// A version 6 name (shape-local and document lists).
const NAME: Record = Record(0x2d);
/// A version 11 name in the document name list.
const NAME2: Record = Record(0x33);
/// The version 6 name list header.
const NAME_LIST: Record = Record(0x2c);
/// The version 11 name list header.
const NAME_LIST2: Record = Record(0x32);
/// The map from element ids to name ids.
const NAME_IDX: Record = Record(0xc9);
/// The other name-id map spelling.
const NAME_IDX_123: Record = Record(0x34);
/// The list of the page's shape ids.
const SHAPE_LIST: Record = Record(0x65);
/// One shape id of a shape list.
const SHAPE_ID: Record = Record(0x83);
/// The list of a sheet's property records.
const PROP_LIST: Record = Record(0x68);
/// The connect list (entries are not yet readable; see the module docs).
const CONNECT_LIST: Record = Record(0x72);
/// The document properties record.
const DOC_PROPS: Record = Record(0xbc);

/// The value of a page field that names no page: the page record's own
/// background id is `0xFFFFFFFF` for a plain page.
const NO_REFERENCE: u32 = u32::MAX;

/// Maps every page, master and document record in `chunks` into
/// `document`.
///
/// Pages and masters are created in stream order, the top-level shapes
/// of each are attached as arena ids, and the document kind is set from
/// the records. Names are resolved through the name lists; a reference
/// that resolves to nothing is recorded in `warnings`, never fatal.
///
/// This is the seam of task P8-01: the version 11 name payloads. Use
/// [`collect_with`] for the version 6 names.
pub fn collect(
    chunks: &[Chunk<'_>],
    document: &mut Document,
    warnings: &mut Vec<ImportWarning>,
) -> Result<(), Error> {
    collect_with(chunks, FormatVersion::V11, document, warnings)
}

/// Maps `chunks` like [`collect`], for one file version.
///
/// `version` selects the name payload layout (version 6 writes
/// single-byte text, version 11 UTF-16LE); every other payload layout
/// is shared between the two versions.
pub fn collect_with(
    chunks: &[Chunk<'_>],
    version: FormatVersion,
    document: &mut Document,
    warnings: &mut Vec<ImportWarning>,
) -> Result<(), Error> {
    let names = name_strings(chunks, version);
    let indexes = name_indexes(chunks);
    let mut walk = Walk::default();
    for chunk in chunks {
        walk.record(chunk, warnings);
    }
    walk.finish(&names, &indexes, document, warnings);
    Ok(())
}

/// Maps the document properties of the two summary streams onto
/// [`larrygram_model::Document::properties`] (ADR 0070).
///
/// The property names are the ones the `.vsdx` reader writes for the
/// same properties (`title`, `creator`, `Template`, ...). A stream that
/// does not parse, and a value the model cannot spell, is a warning
/// (ADR 0071): the document is still returned.
pub fn collect_summary_properties(
    streams: &Streams,
    document: &mut Document,
    warnings: &mut Vec<ImportWarning>,
) {
    for name in [
        crate::container::SUMMARY_INFORMATION,
        crate::container::DOCUMENT_SUMMARY_INFORMATION,
    ] {
        let Some(bytes) = streams.get(name) else {
            continue;
        };
        let properties = match crate::container::read_summary_properties(bytes) {
            Ok(properties) => properties,
            Err(_) => {
                warn(
                    warnings,
                    "document properties",
                    WarningKind::BadValue {
                        what: "summary stream",
                    },
                );
                continue;
            }
        };
        for property in properties {
            map_property(document, warnings, property);
        }
    }
}

/// Maps one summary property onto [`larrygram_model::Document::properties`].
///
/// A value with no text spelling (a binary property) is dropped with a
/// warning (ADR 0070: nothing is dropped silently).
fn map_property(
    document: &mut Document,
    warnings: &mut Vec<ImportWarning>,
    property: SummaryProperty,
) {
    let value = match property.value {
        SummaryValue::Text(text) => Some(text),
        SummaryValue::Integer(value) => Some(value.to_string()),
        SummaryValue::Real(value) => Some(value.to_string()),
        SummaryValue::Boolean(value) => Some(value.to_string()),
        SummaryValue::DateTime(ticks) => Some(w3cdtf(ticks)),
        SummaryValue::Binary(_) => None,
    };
    match value {
        Some(value) => document
            .properties
            .push(Property::new(property.name, value)),
        None => warn(
            warnings,
            "document properties",
            WarningKind::Dropped {
                what: "binary document property",
            },
        ),
    }
}

/// One page or master being built.
#[derive(Debug)]
struct Container {
    /// Whether this is a drawing page (else a master's stencil page).
    page: bool,
    /// The record id: the master's Visio id, or `0xFFFFFFFF` on the
    /// page records of the corpus.
    record_id: u32,
    /// The nesting level of the container record.
    level: u16,
    /// The background-page id the page record carries.
    background: Option<u32>,
    /// The arena ids of the top-level shapes, in stream order.
    shapes: Vec<ShapeId>,
    /// The page sheet's direct cells.
    cells: Vec<Cell>,
}

/// The state of one [`collect_with`] walk.
///
/// Pages and masters never nest, so one container is open at a time;
/// the shape records nest by level and are tracked by their levels.
#[derive(Debug, Default)]
struct Walk {
    /// The levels of the open shape records, innermost last.
    shapes: Vec<u16>,
    /// The page or master the walk is inside, if any.
    current: Option<Container>,
    /// The pages and masters finished so far, in stream order.
    done: Vec<Container>,
    /// The shape record count so far: the arena id of the next shape.
    shape_ordinal: u32,
}

impl Walk {
    /// Maps one chunk record.
    fn record(&mut self, chunk: &Chunk<'_>, warnings: &mut Vec<ImportWarning>) {
        while self
            .shapes
            .last()
            .is_some_and(|level| *level >= chunk.level)
        {
            self.shapes.pop();
        }
        if self
            .current
            .as_ref()
            .is_some_and(|container| chunk.level <= container.level)
        {
            self.close_container();
        }
        match chunk.record {
            PAGE | STENCIL_PAGE => self.open_container(chunk),
            SHAPE_GROUP | SHAPE_SHAPE | SHAPE_GUIDE | SHAPE_FOREIGN => {
                self.open_shape(chunk);
            }
            PAGE_PROPS => self.page_props(chunk, warnings),
            CONNECT_LIST => warn(
                warnings,
                self.context(),
                WarningKind::SkippedRecord {
                    name: "connect list",
                },
            ),
            DOC_PROPS if self.current.is_none() => warn(
                warnings,
                "document",
                WarningKind::Dropped {
                    what: "document properties record",
                },
            ),
            NAME_LIST | NAME_LIST2 | NAME | NAME2 | NAME_IDX | NAME_IDX_123 | SHAPE_LIST
            | SHAPE_ID | PROP_LIST | PAGES | STENCILS | PAGE_SHEET => {}
            _ => self.skip_other(chunk, warnings),
        }
    }

    /// Closes the open container, if any.
    fn close_container(&mut self) {
        if let Some(container) = self.current.take() {
            self.done.push(container);
        }
    }

    /// Opens a page or master container.
    fn open_container(&mut self, chunk: &Chunk<'_>) {
        self.close_container();
        let background = if chunk.record == PAGE {
            read_background(chunk.data)
        } else {
            None
        };
        self.current = Some(Container {
            page: chunk.record == PAGE,
            record_id: chunk.id,
            level: chunk.level,
            background,
            shapes: Vec::new(),
            cells: Vec::new(),
        });
    }

    /// Opens a shape record: the next arena shape, in stream order.
    fn open_shape(&mut self, chunk: &Chunk<'_>) {
        let id = ShapeId {
            index: self.shape_ordinal,
        };
        self.shape_ordinal = self.shape_ordinal.saturating_add(1);
        if self.shapes.is_empty()
            && let Some(container) = self.current.as_mut()
        {
            container.shapes.push(id);
        }
        self.shapes.push(chunk.level);
    }

    /// Maps one page-properties record onto the open page's sheet.
    fn page_props(&mut self, chunk: &Chunk<'_>, warnings: &mut Vec<ImportWarning>) {
        match self.current.as_mut() {
            Some(container) if !container.page => warn(
                warnings,
                format!("master {}", container.record_id),
                WarningKind::Dropped {
                    what: "master page sheet",
                },
            ),
            Some(container) => {
                let context = "page".to_owned();
                container
                    .cells
                    .extend(read_page_props(chunk.data, &context, warnings));
            }
            // The document sheet's own page properties have no model
            // slot; the page records of the corpus carry none.
            None => {}
        }
    }

    /// Warns for a record this module does not consume inside a page or
    /// master (the module docs have the rule).
    fn skip_other(&mut self, chunk: &Chunk<'_>, warnings: &mut Vec<ImportWarning>) {
        if self.current.is_none() || !self.shapes.is_empty() {
            return;
        }
        warn(
            warnings,
            self.context(),
            WarningKind::SkippedRecord {
                name: chunk.record.name(),
            },
        );
    }

    /// Where the walk is, for a warning context.
    fn context(&self) -> String {
        match self.current.as_ref() {
            Some(container) if container.page => "page".to_owned(),
            Some(container) => format!("master {}", container.record_id),
            None => "document".to_owned(),
        }
    }

    /// Resolves names and background pages and adds the pages and
    /// masters to the document, in stream order.
    fn finish(
        mut self,
        names: &BTreeMap<u32, String>,
        indexes: &[NameIndex],
        document: &mut Document,
        warnings: &mut Vec<ImportWarning>,
    ) {
        self.close_container();
        let mut used: Vec<Vec<bool>> = indexes
            .iter()
            .map(|index| vec![false; index.entries.len()])
            .collect();
        let mut pages: Vec<(Page, Option<u32>)> = Vec::new();
        let mut masters: Vec<Master> = Vec::new();
        let mut has_stencil_pages = false;
        for container in &self.done {
            let fallback = i32::try_from(if container.page {
                pages.len()
            } else {
                masters.len()
            })
            .unwrap_or(i32::MAX);
            let (name, visio_id) = resolve_name(
                container,
                indexes,
                &mut used,
                names,
                warnings,
                if container.page {
                    "page name"
                } else {
                    "master name"
                },
                fallback,
            );
            if container.page {
                let mut page = Page::new(
                    PageId {
                        index: u32::try_from(pages.len()).unwrap_or(u32::MAX),
                    },
                    visio_id,
                );
                if !name.is_empty() {
                    page.name = name.clone();
                    page.name_u = name;
                }
                page.shapes = container.shapes.clone();
                page.sheet.cells = container.cells.clone();
                pages.push((page, container.background));
            } else {
                has_stencil_pages = true;
                let mut master = Master::new(
                    MasterId {
                        index: u32::try_from(masters.len()).unwrap_or(u32::MAX),
                    },
                    visio_id,
                );
                if !name.is_empty() {
                    master.name = name.clone();
                    master.name_u = name;
                }
                master.shapes = container.shapes.clone();
                masters.push(master);
            }
        }
        resolve_backgrounds(&mut pages, warnings);
        document.kind = if has_stencil_pages && pages.is_empty() {
            DocumentKind::Stencil
        } else {
            DocumentKind::Drawing
        };
        for (page, _) in pages {
            document.pages.push(page);
        }
        for master in masters {
            document.masters.push(master);
        }
    }
}

/// One name-index record: element ids to name ids at one level.
#[derive(Debug)]
struct NameIndex {
    /// The nesting level the record sits at.
    level: u16,
    /// The entries in file order: element id, name id.
    entries: Vec<(u32, u32)>,
}

/// Reads the document name strings (`Name` and `Name2` records).
///
/// The document name list (`Name2`) wins over the shape-local `Name`
/// records, which use the same id space (the same rule
/// [`crate::style`] applies).
fn name_strings(chunks: &[Chunk<'_>], version: FormatVersion) -> BTreeMap<u32, String> {
    let mut names = BTreeMap::new();
    for chunk in chunks {
        if chunk.record == NAME2 {
            names.insert(chunk.id, decode_name(chunk.data, version, true));
        }
    }
    for chunk in chunks {
        if chunk.record == NAME {
            names
                .entry(chunk.id)
                .or_insert(decode_name(chunk.data, version, false));
        }
    }
    names
}

/// Reads every name-index record in the stream.
fn name_indexes(chunks: &[Chunk<'_>]) -> Vec<NameIndex> {
    let mut indexes = Vec::new();
    for chunk in chunks {
        let entries = match chunk.record {
            NAME_IDX => read_name_index(chunk.data),
            NAME_IDX_123 => read_name_index_123(chunk.data),
            _ => continue,
        };
        indexes.push(NameIndex {
            level: chunk.level,
            entries,
        });
    }
    indexes
}

/// Reads one name-index payload: a count followed by 13-byte entries
/// (`name id`, `name id again`, `element id`, one flag byte).
fn read_name_index(data: &[u8]) -> Vec<(u32, u32)> {
    let mut entries = Vec::new();
    let mut fields = Fields::new(data);
    let Some(count) = fields.u32() else {
        return entries;
    };
    for _ in 0..count {
        let Some(name) = fields.u32() else {
            break;
        };
        if fields.u32().is_none() {
            break;
        }
        let Some(element) = fields.u32() else {
            break;
        };
        if fields.u8().is_none() {
            break;
        }
        entries.push((element, name));
    }
    entries
}

/// Reads one name-index payload with 8-byte entries: element id to
/// name id pairs until the payload ends.
fn read_name_index_123(data: &[u8]) -> Vec<(u32, u32)> {
    let mut entries = Vec::new();
    let mut fields = Fields::new(data);
    while let Some(name) = fields.u32() {
        let Some(element) = fields.u32() else {
            break;
        };
        entries.push((element, name));
    }
    entries
}

/// Resolves one container's name and Visio id through the name indexes
/// at its level (the module docs have the rule). A container no index
/// names takes `fallback` as its Visio id.
fn resolve_name(
    container: &Container,
    indexes: &[NameIndex],
    used: &mut [Vec<bool>],
    names: &BTreeMap<u32, String>,
    warnings: &mut Vec<ImportWarning>,
    what: &'static str,
    fallback: i32,
) -> (String, i32) {
    let wanted = if container.page {
        None
    } else {
        Some(container.record_id)
    };
    let mut found = None;
    for (index, table) in indexes.iter().enumerate() {
        if table.level != container.level {
            continue;
        }
        for (position, (element, name)) in table.entries.iter().enumerate() {
            if used
                .get(index)
                .is_none_or(|row| row.get(position).copied().unwrap_or(true))
            {
                continue;
            }
            let matches = match wanted {
                Some(id) => *element == id,
                None => true,
            };
            if !matches {
                continue;
            }
            if let Some(row) = used.get_mut(index)
                && let Some(slot) = row.get_mut(position)
            {
                *slot = true;
            }
            found = Some((*element, *name));
            break;
        }
        if found.is_some() {
            break;
        }
    }
    let Some((element, name_id)) = found else {
        warnings.push(ImportWarning {
            context: what.to_owned(),
            kind: WarningKind::UnresolvedReference { what },
        });
        return (String::new(), fallback);
    };
    let visio_id = i32::try_from(element).unwrap_or(i32::MAX);
    let Some(name) = names.get(&name_id) else {
        warnings.push(ImportWarning {
            context: what.to_owned(),
            kind: WarningKind::UnresolvedReference { what },
        });
        return (String::new(), visio_id);
    };
    (name.clone(), visio_id)
}

/// Resolves every page's background-page reference to a [`PageId`]
/// (ADR 0070): a reference to no page of the document is an
/// [`WarningKind::UnresolvedReference`] warning and stays unset.
fn resolve_backgrounds(pages: &mut [(Page, Option<u32>)], warnings: &mut Vec<ImportWarning>) {
    let mut resolved: Vec<(usize, PageId)> = Vec::new();
    for (position, (page, background)) in pages.iter().enumerate() {
        let wanted = match background {
            Some(id) if *id != NO_REFERENCE => i32::try_from(*id).unwrap_or(i32::MAX),
            _ => continue,
        };
        if page.visio_id == wanted {
            // A page is not its own background (the reference drops
            // the self-reference the same way).
            continue;
        }
        let found = pages
            .iter()
            .enumerate()
            .find(|(other, (candidate, _))| *other != position && candidate.visio_id == wanted)
            .map(|(index, _)| PageId {
                index: u32::try_from(index).unwrap_or(u32::MAX),
            });
        match found {
            Some(id) => resolved.push((position, id)),
            None => warnings.push(ImportWarning {
                context: "page".to_owned(),
                kind: WarningKind::UnresolvedReference {
                    what: "background page",
                },
            }),
        }
    }
    for (position, id) in resolved {
        if let Some((page, _)) = pages.get_mut(position) {
            page.background = Some(id);
        }
    }
}

/// Reads the background-page id of a page record payload: the record
/// opens with the two list-header lengths, then the id.
fn read_background(data: &[u8]) -> Option<u32> {
    let mut fields = Fields::new(data);
    fields.skip(8)?;
    fields.u32()
}

/// The page properties row (the module docs have the layout).
///
/// Returns the cells that parsed; a field that did not parse is a
/// [`WarningKind::BadValue`] warning and its cell stays unset.
///
/// The size pair is not stored in inches: the record holds the page's
/// size in the units the drawing is scaled in, and the physical size in
/// inches is `value * PageScale / DrawingScale` (the reference's page
/// computation, confirmed against the oracle: `Visio11PlanWithDimensions.vsd`
/// stores 528 x 408 with `PageScale` 0.25 and `DrawingScale` 12 and
/// renders 11 x 8.5 inches). A zero `DrawingScale` divides as 1 (the
/// reference's fallback). The other four cells keep their raw values,
/// which are already inches or radians (ADR 0070's unit rule).
fn read_page_props(data: &[u8], context: &str, warnings: &mut Vec<ImportWarning>) -> Vec<Cell> {
    const CELLS: [&str; 6] = [
        "PageWidth",
        "PageHeight",
        "ShdwOffsetX",
        "ShdwOffsetY",
        "PageScale",
        "DrawingScale",
    ];
    let mut fields = Fields::new(data);
    let mut read: Vec<(&str, u8, f64)> = Vec::with_capacity(CELLS.len());
    for name in CELLS {
        let Some(tag) = fields.u8() else {
            warnings.push(bad_value(context, "page properties"));
            break;
        };
        let Some(value) = fields.f64() else {
            warnings.push(bad_value(context, "page properties"));
            break;
        };
        read.push((name, tag, value));
    }
    // The scale the size pair needs: `PageScale / DrawingScale`, with
    // the reference's fallback for a zero drawing scale.
    let scale_of = |name: &str| {
        read.iter()
            .find(|(seen, _, _)| *seen == name)
            .map(|(_, _, value)| *value)
    };
    let page_scale = scale_of("PageScale").unwrap_or(1.0);
    let drawing_scale = match scale_of("DrawingScale") {
        Some(value) if value.abs() >= 1e-9 => value,
        _ => 1.0,
    };
    let size_scale = (page_scale / drawing_scale).abs();
    let size_scale = if size_scale.is_finite() && size_scale > 0.0 {
        size_scale
    } else {
        1.0
    };
    let mut cells = Vec::new();
    for (name, tag, value) in read {
        let Some(unit) = cell_unit(tag) else {
            warnings.push(bad_value(context, "page properties unit"));
            continue;
        };
        if !value.is_finite() {
            warnings.push(bad_value(context, "page properties"));
            continue;
        }
        let value = if name == "PageWidth" || name == "PageHeight" {
            value * size_scale
        } else {
            value
        };
        let mut cell = Cell::new(CellName::from_static(name));
        cell.value = Some(CellValue::Number(value));
        cell.unit = unit;
        cells.push(cell);
    }
    cells
}

/// The unit a unit-tag byte names, or `None` when it names no unit of
/// the format (the `CELL_TYPE` table of the reference; the same table
/// [`crate::shape`] maps).
fn cell_unit(tag: u8) -> Option<Unit> {
    Some(match tag {
        32 | 48 | 63 | 64 | 252 => Unit::Unitsless,
        33 => Unit::Percent,
        50 => Unit::Points,
        51 => Unit::Picas,
        54 => Unit::Ciceros,
        65 => Unit::Inches,
        66 => Unit::Feet,
        68 => Unit::Miles,
        69 => Unit::Centimeters,
        70 => Unit::Millimeters,
        71 => Unit::Meters,
        72 => Unit::Kilometers,
        75 => Unit::Yards,
        81 => Unit::Degrees,
        83 => Unit::Radians,
        36
        | 37
        | 40
        | 42..=47
        | 49
        | 52
        | 53
        | 67
        | 73
        | 74
        | 76
        | 80
        | 82
        | 84
        | 85
        | 95
        | 111
        | 138
        | 139
        | 225
        | 231
        | 232
        | 233
        | 251 => Unit::Unitsless,
        _ => return None,
    })
}

/// Decodes one name payload in the version's layout.
///
/// Version 11 writes UTF-16LE text, version 6 single-byte text (the
/// libvisio version 6 readers). A `Name2` payload starts with a dummy
/// dword before the text; a `Name` payload is the text itself. NUL
/// characters terminate the text and are not part of the name (the same
/// rule [`crate::shape`] applies).
fn decode_name(data: &[u8], version: FormatVersion, skip_dword: bool) -> String {
    let body = if skip_dword {
        data.get(4..).unwrap_or_default()
    } else {
        data
    };
    match version {
        FormatVersion::V6 => body
            .iter()
            .take_while(|byte| **byte != 0)
            .map(|byte| char::from(*byte))
            .collect(),
        FormatVersion::V11 => decode_utf16(body),
    }
}

/// Lossy UTF-16LE decoding of `bytes`, up to the first NUL (a trailing
/// odd byte is dropped).
fn decode_utf16(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes(pair.try_into().unwrap_or([0, 0])))
        .take_while(|unit| *unit != 0)
        .collect();
    char::decode_utf16(units)
        .map(|decoded| decoded.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect()
}

/// The `W3CDTF` spelling of a Windows timestamp (the same text the
/// `.vsdx` reader reads from `docProps`), in whole seconds.
fn w3cdtf(ticks: u64) -> String {
    /// Ticks between the Windows epoch (1601) and the Unix epoch.
    const EPOCH_TICKS: u64 = 116_444_736_000_000_000;
    /// Ticks per second.
    const TICKS_PER_SECOND: u64 = 10_000_000;
    /// Seconds per day.
    const SECONDS_PER_DAY: u64 = 86_400;
    let Some(ticks) = ticks.checked_sub(EPOCH_TICKS) else {
        return String::new();
    };
    let seconds = ticks / TICKS_PER_SECOND;
    let (days, rest) = (seconds / SECONDS_PER_DAY, seconds % SECONDS_PER_DAY);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3600,
        (rest % 3600) / 60,
        rest % 60
    )
}

/// The calendar date of a day count since the Unix epoch (the
/// proleptic-Gregorian civil calendar).
fn civil_from_days(days: u64) -> (u64, u64, u64) {
    let z = days.saturating_add(719_468);
    let era = z / 146_097;
    let day_of_era = z % 146_097;
    let year_of_era = (day_of_era
        .saturating_sub(day_of_era / 1460)
        .saturating_add(day_of_era / 36_524)
        .saturating_sub(day_of_era / 146_096))
        / 365;
    let day_of_year =
        day_of_era.saturating_sub(365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year.saturating_sub((153 * shifted_month + 2) / 5) + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = 400 * era + year_of_era + u64::from(month <= 2);
    (year, month, day)
}

/// A `BadValue` warning in `context`.
fn bad_value(context: &str, what: &'static str) -> ImportWarning {
    ImportWarning {
        context: context.to_owned(),
        kind: WarningKind::BadValue { what },
    }
}

/// A warning in `context`.
fn warn(warnings: &mut Vec<ImportWarning>, context: impl Into<String>, kind: WarningKind) {
    warnings.push(ImportWarning {
        context: context.into(),
        kind,
    });
}

/// A bounded reader over one record payload.
///
/// Every read is checked against the payload length; a read past the
/// end is `None`, never a panic (AGENTS.md section 6).
struct Fields<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Fields<'a> {
    /// A reader over `data`, starting at its first byte.
    fn new(data: &'a [u8]) -> Self {
        Fields { data, pos: 0 }
    }

    /// The next byte, or `None` past the end.
    fn u8(&mut self) -> Option<u8> {
        let byte = *self.data.get(self.pos)?;
        self.pos += 1;
        Some(byte)
    }

    /// The next little-endian `u32`, or `None` past the end.
    fn u32(&mut self) -> Option<u32> {
        let bytes = self.take(4)?;
        Some(u32::from_le_bytes(bytes.try_into().ok()?))
    }

    /// The next little-endian IEEE double, or `None` past the end.
    fn f64(&mut self) -> Option<f64> {
        let bytes = self.take(8)?;
        Some(f64::from_le_bytes(bytes.try_into().ok()?))
    }

    /// Skips `count` bytes, or fails past the end.
    fn skip(&mut self, count: usize) -> Option<()> {
        self.take(count).map(|_| ())
    }

    /// The next `count` bytes, or `None` past the end.
    fn take(&mut self, count: usize) -> Option<&'a [u8]> {
        let end = self.pos.checked_add(count)?;
        let bytes = self.data.get(self.pos..end)?;
        self.pos = end;
        Some(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container::{Stream, SummaryProperty};

    /// A chunk over bytes the test owns for the duration of the binary.
    fn chunk(record: u16, level: u16, id: u32, data: Vec<u8>) -> Chunk<'static> {
        Chunk {
            record: Record(record),
            level,
            list: 0,
            id,
            data: Box::leak(data.into_boxed_slice()),
        }
    }

    /// The bytes of a little-endian `u32`.
    fn word(value: u32) -> Vec<u8> {
        value.to_le_bytes().to_vec()
    }

    /// The bytes of one unit-tagged value.
    fn tagged(value: f64, unit: u8) -> Vec<u8> {
        let mut bytes = vec![unit];
        bytes.extend(value.to_le_bytes());
        bytes
    }

    /// The concatenation of payload pieces.
    fn join(parts: &[Vec<u8>]) -> Vec<u8> {
        parts.concat()
    }

    /// UTF-16LE bytes of one name, NUL-terminated.
    fn name16(text: &str) -> Vec<u8> {
        let mut bytes: Vec<u8> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
        bytes.extend([0, 0]);
        bytes
    }

    /// One-byte-per-character bytes of one name, NUL-terminated.
    fn name8(text: &str) -> Vec<u8> {
        let mut bytes = text.as_bytes().to_vec();
        bytes.push(0);
        bytes
    }

    /// A `Name2` payload: a dummy dword, then the UTF-16LE name.
    fn name2_payload(text: &str) -> Vec<u8> {
        join(&[word(1), name16(text)])
    }

    /// A page record payload with a background-page id.
    fn page_record(background: u32) -> Vec<u8> {
        join(&[word(0x60), word(0), word(background)])
    }

    /// A name-index payload: element id to name id, 13-byte entries.
    fn name_index(entries: &[(u32, u32)]) -> Vec<u8> {
        let mut data = word(u32::try_from(entries.len()).unwrap_or(u32::MAX));
        for (element, name) in entries {
            data.extend(word(*name));
            data.extend(word(*name));
            data.extend(word(*element));
            data.push(0);
        }
        data
    }

    /// A name-index payload with plain element/name pairs.
    fn name_index_pairs(entries: &[(u32, u32)]) -> Vec<u8> {
        let mut data = Vec::new();
        for (element, name) in entries {
            data.extend(word(*name));
            data.extend(word(*element));
        }
        data
    }

    /// A page-properties payload: six unit-tagged doubles.
    fn page_props(values: &[(u8, f64)]) -> Vec<u8> {
        let mut data = Vec::new();
        for (unit, value) in values {
            data.extend(tagged(*value, *unit));
        }
        data
    }

    /// The two records that name page element 0 "Page-1".
    fn page_name() -> Vec<Chunk<'static>> {
        vec![
            chunk(0xc9, 2, 0, name_index(&[(0, 7)])),
            chunk(0x33, 4, 7, name2_payload("Page-1")),
        ]
    }

    /// The two records that name master element 7 "Square".
    fn master_name() -> Vec<Chunk<'static>> {
        vec![
            chunk(0xc9, 2, 0, name_index(&[(7, 3)])),
            chunk(0x33, 4, 3, name2_payload("Square")),
        ]
    }

    /// Runs [`collect`] over `chunks` and returns the document and the
    /// warnings.
    fn run(chunks: &[Chunk<'_>]) -> (Document, Vec<ImportWarning>) {
        let mut document = Document::new();
        let mut warnings = Vec::new();
        collect(chunks, &mut document, &mut warnings).unwrap();
        (document, warnings)
    }

    /// The kinds of the warnings, in order.
    fn kinds(warnings: &[ImportWarning]) -> Vec<WarningKind> {
        warnings.iter().map(|w| w.kind.clone()).collect()
    }

    #[test]
    fn collect_maps_a_page_with_its_name_and_shapes() {
        let (document, warnings) = run(&[
            chunk(0x15, 2, u32::MAX, page_record(u32::MAX)),
            chunk(0x46, 3, 0, Vec::new()),
            chunk(0x48, 3, 1, Vec::new()),
            chunk(0x48, 3, 2, Vec::new()),
            chunk(0xc9, 2, 0, name_index(&[(0, 7)])),
            chunk(0x33, 4, 7, name2_payload("Page-1")),
        ]);
        assert_eq!(document.pages.len(), 1);
        let page = &document.pages[0];
        assert_eq!(page.name, "Page-1");
        assert_eq!(page.name_u, "Page-1");
        assert_eq!(page.visio_id, 0);
        assert_eq!(
            page.shapes,
            vec![ShapeId { index: 0 }, ShapeId { index: 1 }]
        );
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn collect_delegates_to_the_version_eleven_names() {
        let (document, _) = run(&[
            chunk(0x15, 2, u32::MAX, page_record(u32::MAX)),
            chunk(0xc9, 2, 0, name_index(&[(3, 7)])),
            chunk(0x33, 4, 7, name2_payload("Page-1")),
        ]);
        assert_eq!(document.pages[0].name, "Page-1");
        assert_eq!(document.pages[0].visio_id, 3);
    }

    #[test]
    fn version_six_names_decode_as_single_bytes() {
        let mut document = Document::new();
        let mut warnings = Vec::new();
        let chunks = [
            chunk(0x15, 2, u32::MAX, page_record(u32::MAX)),
            chunk(0xc9, 2, 0, name_index(&[(0, 7)])),
            chunk(0x33, 4, 7, join(&[word(1), name8("Page-1")])),
            chunk(0x2d, 4, 8, name8("Local")),
        ];
        collect_with(&chunks, FormatVersion::V6, &mut document, &mut warnings).unwrap();
        assert_eq!(document.pages[0].name, "Page-1");
    }

    #[test]
    fn the_name_list_wins_over_shape_local_names() {
        let (document, _) = run(&[
            chunk(0x15, 2, u32::MAX, page_record(u32::MAX)),
            chunk(0xc9, 2, 0, name_index(&[(0, 7)])),
            chunk(0x33, 4, 7, name2_payload("Document")),
            chunk(0x2d, 4, 7, name16("Local")),
        ]);
        assert_eq!(document.pages[0].name, "Document");
    }

    #[test]
    fn a_shape_local_name_fills_in_an_unnamed_id() {
        let (document, _) = run(&[
            chunk(0x15, 2, u32::MAX, page_record(u32::MAX)),
            chunk(0xc9, 2, 0, name_index(&[(0, 7)])),
            chunk(0x2d, 4, 7, name16("Local")),
        ]);
        assert_eq!(document.pages[0].name, "Local");
    }

    #[test]
    fn masters_take_their_record_id_name_and_shapes() {
        let (document, warnings) = run(&[
            chunk(0x1e, 2, 7, Vec::new()),
            chunk(0x48, 3, 1, Vec::new()),
            chunk(0xc9, 2, 0, name_index(&[(7, 3)])),
            chunk(0x33, 4, 3, name2_payload("Square")),
        ]);
        assert_eq!(document.masters.len(), 1);
        let master = &document.masters[0];
        assert_eq!(master.visio_id, 7);
        assert_eq!(master.name, "Square");
        assert_eq!(master.name_u, "Square");
        assert_eq!(master.shapes, vec![ShapeId { index: 0 }]);
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn masters_match_their_record_id_not_the_first_entry() {
        let (document, _) = run(&[
            chunk(0x1e, 2, 7, Vec::new()),
            chunk(0x15, 2, u32::MAX, page_record(u32::MAX)),
            chunk(0xc9, 2, 0, name_index(&[(5, 2), (7, 3)])),
            chunk(0x33, 4, 2, name2_payload("Page-2")),
            chunk(0x33, 4, 3, name2_payload("Square")),
        ]);
        assert_eq!(document.masters[0].name, "Square");
        assert_eq!(document.masters[0].visio_id, 7);
        assert_eq!(document.pages[0].name, "Page-2");
        assert_eq!(document.pages[0].visio_id, 5);
    }

    #[test]
    fn stencil_pages_without_drawing_pages_make_a_stencil() {
        let (document, _) = run(&[
            chunk(0x1e, 2, 2, Vec::new()),
            chunk(0xc9, 2, 0, name_index(&[(2, 3)])),
            chunk(0x33, 4, 3, name2_payload("Square")),
        ]);
        assert_eq!(document.kind, DocumentKind::Stencil);
    }

    #[test]
    fn drawing_pages_keep_the_drawing_kind() {
        let (document, _) = run(&[
            chunk(0x1e, 2, 2, Vec::new()),
            chunk(0x15, 2, u32::MAX, page_record(u32::MAX)),
            chunk(0xc9, 2, 0, name_index(&[(0, 4), (2, 3)])),
            chunk(0x33, 4, 3, name2_payload("Square")),
            chunk(0x33, 4, 4, name2_payload("Page-1")),
        ]);
        assert_eq!(document.kind, DocumentKind::Drawing);
    }

    #[test]
    fn an_unnamed_page_warns_and_falls_back_to_its_list_position() {
        let (document, warnings) = run(&[chunk(0x15, 2, u32::MAX, page_record(u32::MAX))]);
        assert_eq!(document.pages[0].name, "");
        assert_eq!(document.pages[0].visio_id, 0);
        assert_eq!(
            kinds(&warnings),
            vec![WarningKind::UnresolvedReference { what: "page name" }]
        );
    }

    #[test]
    fn an_unnamed_master_warns_and_falls_back_to_its_list_position() {
        let (document, warnings) = run(&[chunk(0x1e, 2, u32::MAX, Vec::new())]);
        assert_eq!(document.masters[0].name, "");
        assert_eq!(document.masters[0].visio_id, 0);
        assert_eq!(
            kinds(&warnings),
            vec![WarningKind::UnresolvedReference {
                what: "master name"
            }]
        );
    }

    #[test]
    fn a_name_id_the_lists_do_not_hold_warns() {
        let (document, warnings) = run(&[
            chunk(0x15, 2, u32::MAX, page_record(u32::MAX)),
            chunk(0xc9, 2, 0, name_index(&[(4, 9)])),
        ]);
        assert_eq!(document.pages[0].name, "");
        assert_eq!(document.pages[0].visio_id, 4);
        assert_eq!(
            kinds(&warnings),
            vec![WarningKind::UnresolvedReference { what: "page name" }]
        );
    }

    #[test]
    fn the_pair_name_index_layout_resolves_too() {
        let (document, _) = run(&[
            chunk(0x15, 2, u32::MAX, page_record(u32::MAX)),
            chunk(0x34, 2, 0, name_index_pairs(&[(0, 7)])),
            chunk(0x33, 4, 7, name2_payload("Page-1")),
        ]);
        assert_eq!(document.pages[0].name, "Page-1");
    }

    #[test]
    fn name_indexes_at_another_level_do_not_name_a_container() {
        let (document, warnings) = run(&[
            chunk(0x15, 2, u32::MAX, page_record(u32::MAX)),
            chunk(0xc9, 5, 0, name_index(&[(0, 7)])),
            chunk(0x33, 4, 7, name2_payload("Page-1")),
        ]);
        assert_eq!(document.pages[0].name, "");
        assert_eq!(
            kinds(&warnings),
            vec![WarningKind::UnresolvedReference { what: "page name" }]
        );
    }

    #[test]
    fn a_truncated_name_index_keeps_what_parsed() {
        let mut data = word(3);
        data.extend(name_index(&[(0, 7)])[4..].to_vec());
        let (document, _) = run(&[
            chunk(0x15, 2, u32::MAX, page_record(u32::MAX)),
            chunk(0xc9, 2, 0, data),
            chunk(0x33, 4, 7, name2_payload("Page-1")),
        ]);
        assert_eq!(document.pages[0].name, "Page-1");
    }

    #[test]
    fn a_name_index_entry_without_a_flag_byte_is_dropped() {
        let mut data = word(1);
        data.extend(word(7));
        data.extend(word(7));
        data.extend(word(0));
        let (document, warnings) = run(&[
            chunk(0x15, 2, u32::MAX, page_record(u32::MAX)),
            chunk(0xc9, 2, 0, data),
            chunk(0x33, 4, 7, name2_payload("Page-1")),
        ]);
        assert_eq!(document.pages[0].name, "");
        assert_eq!(
            kinds(&warnings),
            vec![WarningKind::UnresolvedReference { what: "page name" }]
        );
    }

    #[test]
    fn an_empty_name_index_and_a_lone_pair_hold_nothing() {
        let (document, warnings) = run(&[
            chunk(0x15, 2, u32::MAX, page_record(u32::MAX)),
            chunk(0xc9, 2, 0, Vec::new()),
            chunk(0x34, 2, 1, word(7)),
            chunk(0x33, 4, 7, name2_payload("Page-1")),
        ]);
        assert_eq!(document.pages[0].name, "");
        assert_eq!(
            kinds(&warnings),
            vec![WarningKind::UnresolvedReference { what: "page name" }]
        );
    }

    #[test]
    fn page_props_map_the_page_property_cells() {
        let values = [
            (65, 11.0),
            (65, 8.5),
            (65, 0.125),
            (65, -0.125),
            (65, 1.0),
            (73, 1.0),
        ];
        let mut chunks = vec![
            chunk(0x15, 2, u32::MAX, page_record(u32::MAX)),
            chunk(0x92, 3, 0, page_props(&values)),
        ];
        chunks.extend(page_name());
        let (document, warnings) = run(&chunks);
        let cells = &document.pages[0].sheet.cells;
        let names: Vec<&str> = cells.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "PageWidth",
                "PageHeight",
                "ShdwOffsetX",
                "ShdwOffsetY",
                "PageScale",
                "DrawingScale"
            ]
        );
        assert_eq!(
            cells[0].value,
            Some(CellValue::Number(11.0)),
            "the cells are in the Page Properties row's order"
        );
        assert_eq!(cells[0].unit, Unit::Inches);
        assert_eq!(cells[1].unit, Unit::Inches);
        assert_eq!(cells[5].unit, Unit::Unitsless);
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn the_page_size_pair_scales_by_the_page_and_drawing_scale() {
        // Regression (P8-10): `Visio11PlanWithDimensions.vsd` stores its
        // page as 528 x 408 with `PageScale` 0.25 and `DrawingScale` 12
        // (a 1/4" = 1' plan); the physical page is 11 x 8.5 inches
        // (the reference's page computation, confirmed against the
        // oracle).
        let values = [
            (66, 528.0),
            (66, 408.0),
            (63, 0.125),
            (63, -0.125),
            (65, 0.25),
            (66, 12.0),
        ];
        let mut chunks = vec![
            chunk(0x15, 2, u32::MAX, page_record(u32::MAX)),
            chunk(0x92, 3, 0, page_props(&values)),
        ];
        chunks.extend(page_name());
        let (document, warnings) = run(&chunks);
        let cells = &document.pages[0].sheet.cells;
        assert_eq!(cells[0].value, Some(CellValue::Number(11.0)));
        assert_eq!(cells[1].value, Some(CellValue::Number(8.5)));
        assert_eq!(
            cells[2].value,
            Some(CellValue::Number(0.125)),
            "only the size pair is scaled"
        );
        assert_eq!(cells[4].value, Some(CellValue::Number(0.25)));
        assert_eq!(cells[5].value, Some(CellValue::Number(12.0)));
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn a_zero_drawing_scale_scales_the_size_pair_by_one() {
        // The reference's fallback: a zero `DrawingScale` divides as 1.
        let values = [
            (65, 11.0),
            (65, 8.5),
            (65, 0.0),
            (65, 0.0),
            (65, 1.0),
            (65, 0.0),
        ];
        let mut chunks = vec![
            chunk(0x15, 2, u32::MAX, page_record(u32::MAX)),
            chunk(0x92, 3, 0, page_props(&values)),
        ];
        chunks.extend(page_name());
        let (document, _) = run(&chunks);
        let cells = &document.pages[0].sheet.cells;
        assert_eq!(cells[0].value, Some(CellValue::Number(11.0)));
    }

    #[test]
    fn the_cell_type_table_maps_every_tag_the_format_uses() {
        for tag in [
            32, 33, 36, 37, 40, 42, 47, 48, 49, 50, 51, 52, 53, 54, 63, 64, 65, 66, 67, 68, 69, 70,
            71, 72, 73, 74, 75, 76, 80, 81, 82, 83, 84, 85, 95, 111, 138, 139, 225, 231, 232, 233,
            251, 252,
        ] {
            assert!(cell_unit(tag).is_some(), "tag {tag} must map");
        }
        assert_eq!(cell_unit(33), Some(Unit::Percent));
        assert_eq!(cell_unit(50), Some(Unit::Points));
        assert_eq!(cell_unit(65), Some(Unit::Inches));
        assert_eq!(cell_unit(70), Some(Unit::Millimeters));
        assert_eq!(cell_unit(81), Some(Unit::Degrees));
        assert_eq!(cell_unit(83), Some(Unit::Radians));
    }

    #[test]
    fn an_unknown_page_props_unit_warns_and_the_cell_stays_unset() {
        let values = [
            (1, 11.0),
            (65, 8.5),
            (65, 0.125),
            (65, -0.125),
            (65, 1.0),
            (73, 1.0),
        ];
        let mut chunks = vec![
            chunk(0x15, 2, u32::MAX, page_record(u32::MAX)),
            chunk(0x92, 3, 0, page_props(&values)),
        ];
        chunks.extend(page_name());
        let (document, warnings) = run(&chunks);
        let cells = &document.pages[0].sheet.cells;
        assert_eq!(cells.len(), 5);
        assert_eq!(cells[0].name.as_str(), "PageHeight");
        assert_eq!(
            kinds(&warnings),
            vec![WarningKind::BadValue {
                what: "page properties unit"
            }]
        );
    }

    #[test]
    fn a_short_page_props_payload_warns_and_keeps_what_parsed() {
        let mut chunks = vec![
            chunk(0x15, 2, u32::MAX, page_record(u32::MAX)),
            chunk(0x92, 3, 0, page_props(&[(65, 11.0), (65, 8.5)])),
        ];
        chunks.extend(page_name());
        let (document, warnings) = run(&chunks);
        assert_eq!(document.pages[0].sheet.cells.len(), 2);
        assert_eq!(
            kinds(&warnings),
            vec![WarningKind::BadValue {
                what: "page properties"
            }]
        );
    }

    #[test]
    fn an_empty_page_props_payload_warns_once() {
        let mut chunks = vec![
            chunk(0x15, 2, u32::MAX, page_record(u32::MAX)),
            chunk(0x92, 3, 0, Vec::new()),
        ];
        chunks.extend(page_name());
        let (document, warnings) = run(&chunks);
        assert!(document.pages[0].sheet.cells.is_empty());
        assert_eq!(
            kinds(&warnings),
            vec![WarningKind::BadValue {
                what: "page properties"
            }]
        );
    }

    #[test]
    fn a_non_finite_page_props_value_warns_and_the_cell_stays_unset() {
        let values = [
            (65, f64::NAN),
            (65, 8.5),
            (65, 0.125),
            (65, -0.125),
            (65, 1.0),
            (73, 1.0),
        ];
        let mut chunks = vec![
            chunk(0x15, 2, u32::MAX, page_record(u32::MAX)),
            chunk(0x92, 3, 0, page_props(&values)),
        ];
        chunks.extend(page_name());
        let (document, warnings) = run(&chunks);
        let cells = &document.pages[0].sheet.cells;
        assert_eq!(cells.len(), 5);
        assert_eq!(cells[0].name.as_str(), "PageHeight");
        assert_eq!(
            kinds(&warnings),
            vec![WarningKind::BadValue {
                what: "page properties"
            }]
        );
    }

    #[test]
    fn a_master_page_sheet_is_dropped_with_a_warning() {
        let mut chunks = vec![
            chunk(0x1e, 2, 7, Vec::new()),
            chunk(0x92, 3, 0, page_props(&[(65, 1.0)])),
        ];
        chunks.extend(master_name());
        let (document, warnings) = run(&chunks);
        assert!(document.masters[0].shapes.is_empty());
        assert_eq!(
            kinds(&warnings),
            vec![WarningKind::Dropped {
                what: "master page sheet"
            }]
        );
    }

    #[test]
    fn page_props_outside_a_container_stay_unset() {
        let (document, warnings) = run(&[chunk(0x92, 3, 0, page_props(&[(65, 1.0)]))]);
        assert!(document.pages.is_empty());
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn background_pages_resolve_to_their_page() {
        let (document, warnings) = run(&[
            chunk(0x15, 2, u32::MAX, page_record(u32::MAX)),
            chunk(0x15, 2, u32::MAX, page_record(0)),
            chunk(0xc9, 2, 0, name_index(&[(0, 7), (1, 8)])),
            chunk(0x33, 4, 7, name2_payload("Background")),
            chunk(0x33, 4, 8, name2_payload("Page-1")),
        ]);
        assert_eq!(document.pages[0].background, None);
        assert_eq!(document.pages[1].background, Some(PageId { index: 0 }));
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn an_unresolved_background_page_warns() {
        let (document, warnings) = run(&[
            chunk(0x15, 2, u32::MAX, page_record(5)),
            chunk(0xc9, 2, 0, name_index(&[(0, 7)])),
            chunk(0x33, 4, 7, name2_payload("Page-1")),
        ]);
        assert_eq!(document.pages[0].background, None);
        assert_eq!(
            kinds(&warnings),
            vec![WarningKind::UnresolvedReference {
                what: "background page"
            }]
        );
    }

    #[test]
    fn a_page_is_not_its_own_background() {
        let (document, warnings) = run(&[
            chunk(0x15, 2, u32::MAX, page_record(0)),
            chunk(0xc9, 2, 0, name_index(&[(0, 7)])),
            chunk(0x33, 4, 7, name2_payload("Page-1")),
        ]);
        assert_eq!(document.pages[0].background, None);
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn connect_lists_are_skipped_with_a_warning() {
        let mut chunks = vec![
            chunk(0x15, 2, u32::MAX, page_record(u32::MAX)),
            chunk(0x72, 3, 0, Vec::new()),
        ];
        chunks.extend(page_name());
        let (_, warnings) = run(&chunks);
        assert_eq!(
            kinds(&warnings),
            vec![WarningKind::SkippedRecord {
                name: "connect list"
            }]
        );
    }

    #[test]
    fn a_connect_list_outside_a_container_warns_too() {
        let (_, warnings) = run(&[chunk(0x72, 2, 0, Vec::new())]);
        assert_eq!(
            kinds(&warnings),
            vec![WarningKind::SkippedRecord {
                name: "connect list"
            }]
        );
    }

    #[test]
    fn the_document_properties_record_is_dropped() {
        let (_, warnings) = run(&[chunk(0xbc, 2, 0, Vec::new())]);
        assert_eq!(
            kinds(&warnings),
            vec![WarningKind::Dropped {
                what: "document properties record"
            }]
        );
    }

    #[test]
    fn the_document_properties_record_inside_a_container_is_skipped() {
        let mut chunks = vec![
            chunk(0x15, 2, u32::MAX, page_record(u32::MAX)),
            chunk(0xbc, 3, 0, Vec::new()),
        ];
        chunks.extend(page_name());
        let (_, warnings) = run(&chunks);
        assert_eq!(
            kinds(&warnings),
            vec![WarningKind::SkippedRecord { name: "DOC_PROPS" }]
        );
    }

    #[test]
    fn unmapped_container_records_are_skipped_with_a_warning() {
        let mut chunks = vec![
            chunk(0x15, 2, u32::MAX, page_record(u32::MAX)),
            chunk(0xb7, 3, 0, Vec::new()),
        ];
        chunks.extend(page_name());
        let (_, warnings) = run(&chunks);
        assert_eq!(
            kinds(&warnings),
            vec![WarningKind::SkippedRecord { name: "RULER_GRID" }]
        );
    }

    #[test]
    fn records_outside_a_container_stay_silent() {
        let (_, warnings) = run(&[
            chunk(0xb7, 2, 0, Vec::new()),
            chunk(0x27, 2, 0, Vec::new()),
            chunk(0x1d, 2, 0, Vec::new()),
        ]);
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn shape_records_and_their_children_stay_silent() {
        let (document, warnings) = run(&[
            chunk(0x15, 2, u32::MAX, page_record(u32::MAX)),
            chunk(0x48, 3, 1, Vec::new()),
            chunk(0x85, 4, 0, Vec::new()),
            chunk(0xc9, 2, 0, name_index(&[(0, 7)])),
            chunk(0x33, 4, 7, name2_payload("Page-1")),
        ]);
        assert_eq!(document.pages[0].shapes, vec![ShapeId { index: 0 }]);
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn only_top_level_shapes_reach_the_page() {
        let (document, _) = run(&[
            chunk(0x15, 2, u32::MAX, page_record(u32::MAX)),
            chunk(0x47, 3, 1, Vec::new()),
            chunk(0x48, 5, 2, Vec::new()),
            chunk(0x48, 3, 3, Vec::new()),
        ]);
        assert_eq!(
            document.pages[0].shapes,
            vec![ShapeId { index: 0 }, ShapeId { index: 2 }]
        );
    }

    #[test]
    fn the_master_page_sheet_warns_for_a_master_with_an_icon_record() {
        let (_, warnings) = run(&[
            chunk(0x1e, 2, 7, Vec::new()),
            chunk(0x23, 3, 1, Vec::new()),
            chunk(0xc9, 2, 0, name_index(&[(7, 3)])),
            chunk(0x33, 4, 3, name2_payload("Square")),
        ]);
        assert_eq!(
            kinds(&warnings),
            vec![WarningKind::SkippedRecord {
                name: "unknown record"
            }]
        );
    }

    #[test]
    fn map_property_spells_every_value_kind() {
        let cases = [
            (SummaryValue::Text("Title".to_owned()), "Title"),
            (SummaryValue::Integer(42), "42"),
            (SummaryValue::Real(1.5), "1.5"),
            (SummaryValue::Boolean(true), "true"),
            (
                SummaryValue::DateTime(116_444_736_000_000_000 + 1_388_534_400 * 10_000_000),
                "2014-01-01T00:00:00Z",
            ),
        ];
        for (value, expected) in cases {
            let mut document = Document::new();
            let mut warnings = Vec::new();
            map_property(
                &mut document,
                &mut warnings,
                SummaryProperty {
                    name: "name".to_owned(),
                    value,
                },
            );
            assert_eq!(document.properties.len(), 1);
            assert_eq!(document.properties[0].value, expected);
            assert!(warnings.is_empty(), "{warnings:?}");
        }
    }

    #[test]
    fn a_binary_document_property_is_dropped_with_a_warning() {
        let mut document = Document::new();
        let mut warnings = Vec::new();
        map_property(
            &mut document,
            &mut warnings,
            SummaryProperty {
                name: "thumbnail".to_owned(),
                value: SummaryValue::Binary(vec![1, 2, 3]),
            },
        );
        assert!(document.properties.is_empty());
        assert_eq!(
            kinds(&warnings),
            vec![WarningKind::Dropped {
                what: "binary document property"
            }]
        );
    }

    #[test]
    fn w3cdtf_spells_windows_timestamps_in_whole_seconds() {
        let instant = 116_444_736_000_000_000 + 1_388_534_400 * 10_000_000;
        assert_eq!(w3cdtf(instant + 3_000_000), w3cdtf(instant));
        assert_eq!(w3cdtf(0), "");
        // January and July, so both sides of the month and year rules
        // of the civil calendar are covered.
        assert_eq!(w3cdtf(instant), "2014-01-01T00:00:00Z");
        assert_eq!(
            w3cdtf(116_444_736_000_000_000 + 1_404_172_800 * 10_000_000),
            "2014-07-01T00:00:00Z"
        );
    }

    #[test]
    fn collect_summary_properties_maps_the_streams_it_finds() {
        let mut document = Document::new();
        let mut warnings = Vec::new();
        let streams = Streams {
            streams: vec![Stream {
                name: crate::container::SUMMARY_INFORMATION.to_owned(),
                data: b"not a property set".to_vec(),
            }],
        };
        collect_summary_properties(&streams, &mut document, &mut warnings);
        assert!(document.properties.is_empty());
        assert_eq!(
            kinds(&warnings),
            vec![WarningKind::BadValue {
                what: "summary stream"
            }]
        );
        let streams = Streams {
            streams: Vec::new(),
        };
        collect_summary_properties(&streams, &mut document, &mut warnings);
        assert_eq!(
            warnings.len(),
            1,
            "a file without summary streams is silent"
        );
    }

    #[test]
    fn a_page_record_shorter_than_its_header_has_no_background() {
        let (document, warnings) = run(&[
            chunk(0x15, 2, u32::MAX, word(0x60)),
            chunk(0xc9, 2, 0, name_index(&[(0, 7)])),
            chunk(0x33, 4, 7, name2_payload("Page-1")),
        ]);
        assert_eq!(document.pages[0].background, None);
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn a_name_index_entry_truncated_mid_entry_keeps_what_parsed() {
        let mut data = word(2);
        data.extend(word(7));
        let (document, warnings) = run(&[
            chunk(0x15, 2, u32::MAX, page_record(u32::MAX)),
            chunk(0xc9, 2, 0, data),
            chunk(0x33, 4, 7, name2_payload("Page-1")),
        ]);
        assert_eq!(document.pages[0].name, "");
        assert_eq!(
            kinds(&warnings),
            vec![WarningKind::UnresolvedReference { what: "page name" }]
        );
    }

    #[test]
    fn a_name_index_entry_without_an_element_id_keeps_what_parsed() {
        let mut data = word(1);
        data.extend(word(7));
        data.extend(word(7));
        let (document, warnings) = run(&[
            chunk(0x15, 2, u32::MAX, page_record(u32::MAX)),
            chunk(0xc9, 2, 0, data),
            chunk(0x33, 4, 7, name2_payload("Page-1")),
        ]);
        assert_eq!(document.pages[0].name, "");
        assert_eq!(
            kinds(&warnings),
            vec![WarningKind::UnresolvedReference { what: "page name" }]
        );
    }

    #[test]
    fn a_page_props_unit_without_its_value_warns_and_stops() {
        let mut data = page_props(&[(65, 11.0), (65, 8.5)]);
        data.push(65);
        let mut chunks = vec![
            chunk(0x15, 2, u32::MAX, page_record(u32::MAX)),
            chunk(0x92, 3, 0, data),
        ];
        chunks.extend(page_name());
        let (document, warnings) = run(&chunks);
        assert_eq!(document.pages[0].sheet.cells.len(), 2);
        assert_eq!(
            kinds(&warnings),
            vec![WarningKind::BadValue {
                what: "page properties"
            }]
        );
    }
}
