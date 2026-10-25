// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Record payload layouts in this file are derived from the libvisio
// reference (AGENTS.md section 5, MPL-2.0); see NOTICE at the repository
// root (ADR 0003).

//! Shapes, transforms and geometry: chunk records into the model
//! (ADR 0070).
//!
//! This module reads the shape records (shape groups, shapes, guide
//! shapes), the transform records (pin, size, angle, flips, 1-D
//! endpoints) and the geometry records (move, line, arc, elliptical
//! arc, spline, NURBS, polyline, infinite line) and maps them onto
//! [`larrygram_model::Shape`] and its ShapeSheet: transforms to the
//! `Xform` cells, `Xform1D` to the 1-D cells, geometry rows to
//! `Geometry` sections. Lengths are inches and angles are radians, the
//! model's units (AGENTS.md section 6).
//!
//! # What maps where
//!
//! | Record | Model |
//! |---|---|
//! | shape group (`0x47`), shape (`0x48`), guide shape (`0x4d`), foreign shape (`0x4e`) | one [`Shape`] in the document arena, in stream order |
//! | shape lists (`0x65`) and shape-id records (`0x83`) | [`Shape::visio_id`] |
//! | name records (`0x2d`, `0x33`) and name indexes (`0xc9`, `0x34`) | [`Shape::name`] and [`Shape::name_u`] |
//! | `XFormData` (`0x9b`) | `PinX`, `PinY`, `Width`, `Height`, `LocPinX`, `LocPinY`, `Angle`, `FlipX`, `FlipY` |
//! | `XForm1D` (`0x9d`) | `BeginX`, `BeginY`, `EndX`, `EndY` |
//! | text transform (`0x9c`) | `TxtPinX`, `TxtPinY`, `TxtWidth`, `TxtHeight`, `TxtLocPinX`, `TxtLocPinY`, `TxtAngle` |
//! | geometry list (`0x6c`) | one `Geometry` section; its child ids set the row order |
//! | geometry flags (`0x89`) | the section's `NoFill`, `NoLine`, `NoShow` |
//! | geometry rows (`0x8a`, `0x8b`, `0x8c`, `0x8d`, `0x8f`, `0x90`, `0xa5`, `0xa6`, `0xc1`, `0xc3`) | `Geometry` rows with their `X`, `Y`, `A`..`D` and `E` cells |
//! | curve data (`0xd1`) | the `NURBS(...)` and `POLYLINE(...)` data a row references |
//!
//! The line, fill and text style references of a shape record stay Visio
//! ids, and the master reference resolves to the master with the same
//! Visio id — the way the `.vsdx` reader resolves `Master` — or is left
//! unset with a warning. Nothing is flattened (ADR 0013).
//!
//! # The mapping rules
//!
//! **Nesting.** The nesting level of the shape records builds the tree:
//! a shape record is a child of the nearest open shape record at a lower
//! level, and the deeper records that follow belong to it. Parent and
//! child links follow the levels.
//!
//! **Ids.** `Shape::visio_id` comes from the shape-id records: a shape
//! list (`0x65`) gathers the ids of the `0x83` records that follow it,
//! and each new shape takes the next unconsumed id of the most recent
//! list (a page's list names the page's shapes, a group's list names the
//! group's sub-shapes). A shape with no shape-id record left falls back
//! to the shape record's own id; one with neither gets `visio_id` 0 and
//! a `BadValue` warning.
//!
//! **Names.** Name records define names by their record id. Name
//! indexes (`0xc9`, `0x34`) map element ids to name ids; the ones
//! outside any shape record group (a page's own index) name the shapes,
//! and the nearest preceding index with an entry for the shape's id
//! wins. The binary format has one name per element, so [`Shape::name`]
//! and [`Shape::name_u`] receive the same string (the `.vsdx` corpus
//! spells both alike). A name index inside a shape's record group names
//! the shape's property rows, not the shape, and is not consulted here.
//!
//! **Units.** Every numeric field is a unit-tagged double: one byte
//! naming the unit the value is *displayed* in (the `CELL_TYPE` table
//! of the format), then the value. The value itself is always canonical
//! (lengths in inches, angles in radians — the libvisio readers skip
//! the tag byte the same way). The tag byte becomes the cell's [`Unit`]
//! exactly as the `.vsdx` reader maps a `U` code; a tag byte that names
//! no unit of the format is a `BadValue` warning and the cell stays
//! unset (ADR 0070). `FlipX` and `FlipY` are booleans (ADR 0070).
//!
//! **Degrading, never crashing.** A record this module does not consume
//! is skipped: the dropped list of ADR 0070 (foreign data, OLE,
//! hyperlinks, events and macros, annotations, reviewers, smart tags,
//! the font list) warns `Dropped`, a record kind outside the format's
//! id table warns `SkippedRecord`, and the records other domain modules
//! map (line, fill, text, character, paragraph, layers, ...) stay
//! silent here. Warnings fire only inside shape record groups; a page
//! or container record is another module's to report. A truncated or
//! nonsensical payload sets the cells that did parse and warns
//! `BadValue` for the rest; every count read from a payload is capped by
//! what the payload can hold.
//!
//! # Versions
//!
//! The chunk header and the shape, transform and geometry payloads are
//! the same in the version 6 and the version 11 formats (the libvisio
//! readers share them). The name payloads differ: version 11 writes
//! UTF-16LE text, version 6 single-byte text (the libvisio version 6
//! readers). [`collect_with`] takes the [`FormatVersion`] for that one
//! difference.
//!
//! # Derivation
//!
//! The record layouts and field offsets above are derived from the
//! libvisio reference (`VSDParser.cpp`, `VSDDocumentStructure.h`,
//! MPL-2.0; AGENTS.md section 5). Only derived facts (record ids, field
//! order, unit semantics) are implemented here, in our own code.

use std::collections::{BTreeMap, VecDeque};

use larrygram_geom::Unit;
use larrygram_model::{
    Cell, CellName, CellValue, Document, Master, MasterId, Row, Section, SectionName, Shape,
    ShapeId,
};

use crate::chunks::{Chunk, FormatVersion, Record};
use crate::error::Error;
use crate::limits::Limits;
use crate::style;
use crate::warning::{ImportWarning, WarningKind};

// The record ids this module consumes (ADR 0069: each domain module
// holds its own local constants; nothing else sees the raw ids).

/// A group shape record.
const SHAPE_GROUP: Record = Record(0x47);
/// A shape record.
const SHAPE_SHAPE: Record = Record(0x48);
/// A guide shape record.
const SHAPE_GUIDE: Record = Record(0x4d);
/// A foreign (picture or OLE) shape record.
const SHAPE_FOREIGN: Record = Record(0x4e);
/// A shape list record: it groups the shape-id records that follow it.
const SHAPE_LIST: Record = Record(0x65);
/// A shape-id record: its payload carries one Visio shape id.
const SHAPE_ID: Record = Record(0x83);
/// A name record of a record group.
const NAME: Record = Record(0x2d);
/// A name record of the document name list.
const NAME2: Record = Record(0x33);
/// A name index: element id to name id.
const NAME_IDX: Record = Record(0xc9);
/// A name index with 8-byte entries.
const NAME_IDX123: Record = Record(0x34);
/// A shape transform record.
const XFORM_DATA: Record = Record(0x9b);
/// A text transform record.
const TEXT_XFORM: Record = Record(0x9c);
/// A 1-D transform record.
const XFORM_1D: Record = Record(0x9d);
/// A geometry list record: one `Geometry` section.
const GEOM_LIST: Record = Record(0x6c);
/// A geometry section record: the `NoFill`, `NoLine`, `NoShow` flags.
const GEOMETRY: Record = Record(0x89);
/// A `MoveTo` geometry row record.
const MOVE_TO: Record = Record(0x8a);
/// A `LineTo` geometry row record.
const LINE_TO: Record = Record(0x8b);
/// An `ArcTo` geometry row record.
const ARC_TO: Record = Record(0x8c);
/// An `InfiniteLine` geometry row record.
const INFINITE_LINE: Record = Record(0x8d);
/// An `Ellipse` geometry row record.
const ELLIPSE: Record = Record(0x8f);
/// An `EllipticalArcTo` geometry row record.
const ELLIPTICAL_ARC_TO: Record = Record(0x90);
/// A `SplineStart` geometry row record.
const SPLINE_START: Record = Record(0xa5);
/// A `SplineKnot` geometry row record.
const SPLINE_KNOT: Record = Record(0xa6);
/// A `PolylineTo` geometry row record.
const POLYLINE_TO: Record = Record(0xc1);
/// A `NurbsTo` geometry row record.
const NURBS_TO: Record = Record(0xc3);
/// A curve-data record: the points a geometry row references by id.
const SHAPE_DATA: Record = Record(0xd1);
/// The styles stream record: its rows belong to the stylesheet, not to
/// the shape whose record group the walk is inside.
const STYLES_STREAM: Record = Record(0x1a);
/// A line-properties record. Below a stylesheet it is a style row
/// ([`crate::style`] maps it); inside a shape record group it is the
/// shape's own `Line` row (P8-15 goal 1).
const LINE: Record = Record(0x85);
/// A fill-and-shadow record: the `FillForegnd`/`FillBkgnd`/`Shadow*`
/// cells, on a style or on a shape.
const FILL_AND_SHADOW: Record = Record(0x86);
/// A text-block record: the margins and alignment, on a style or on a
/// shape.
const TEXT_BLOCK: Record = Record(0x87);

// The records ADR 0070 drops with a warning. Every other record kind
// this module does not consume belongs to another domain module (line,
// fill, text, styles, pages, ...) or to the container, and is skipped
// silently here.

/// The dropped record kinds of ADR 0070, with what they carry.
const DROPPED: &[(Record, &str)] = &[
    (Record(0x0c), "foreign data"),
    (Record(0x0d), "OLE object list"),
    (Record(0x1f), "OLE data"),
    (Record(0x98), "foreign data type"),
    (Record(0x73), "hyperlink list"),
    (Record(0xc4), "hyperlink"),
    (Record(0x84), "event"),
    (Record(0x6e), "action id list"),
    (Record(0xa9), "action id"),
    (Record(0xc6), "annotation"),
    (Record(0xc5), "reviewer"),
    (Record(0x76), "smart tag list"),
    (Record(0xc7), "smart tag definition"),
    (Record(0x18), "font list"),
    (Record(0x19), "font"),
    (Record(0xd7), "font face"),
    (Record(0xd8), "font face list"),
];

/// The record kinds of the binary format (the id table derived from the
/// libvisio reference, `VSDDocumentStructure.h`). A record kind outside
/// this table is skipped with a `SkippedRecord` warning.
const KNOWN: &[Record] = &[
    Record(0x0c),
    Record(0x0d),
    Record(0x0e),
    Record(0x14),
    Record(0x15),
    Record(0x16),
    Record(0x18),
    Record(0x19),
    Record(0x1a),
    Record(0x1d),
    Record(0x1e),
    Record(0x1f),
    Record(0x27),
    Record(0x2c),
    Record(0x2d),
    Record(0x32),
    Record(0x33),
    Record(0x34),
    Record(0x46),
    Record(0x47),
    Record(0x48),
    Record(0x4a),
    Record(0x4d),
    Record(0x4e),
    Record(0x64),
    Record(0x65),
    Record(0x66),
    Record(0x68),
    Record(0x69),
    Record(0x6a),
    Record(0x6b),
    Record(0x6c),
    Record(0x6d),
    Record(0x6e),
    Record(0x6f),
    Record(0x70),
    Record(0x71),
    Record(0x72),
    Record(0x73),
    Record(0x76),
    Record(0x83),
    Record(0x84),
    Record(0x85),
    Record(0x86),
    Record(0x87),
    Record(0x88),
    Record(0x89),
    Record(0x8a),
    Record(0x8b),
    Record(0x8c),
    Record(0x8d),
    Record(0x8f),
    Record(0x90),
    Record(0x92),
    Record(0x93),
    Record(0x94),
    Record(0x95),
    Record(0x96),
    Record(0x97),
    Record(0x98),
    Record(0x99),
    Record(0x9b),
    Record(0x9c),
    Record(0x9d),
    Record(0x9e),
    Record(0xa0),
    Record(0xa1),
    Record(0xa2),
    Record(0xa4),
    Record(0xa5),
    Record(0xa6),
    Record(0xa7),
    Record(0xa8),
    Record(0xa9),
    Record(0xaa),
    Record(0xb4),
    Record(0xb5),
    Record(0xb6),
    Record(0xb7),
    Record(0xba),
    Record(0xbc),
    Record(0xbd),
    Record(0xbe),
    Record(0xbf),
    Record(0xc0),
    Record(0xc1),
    Record(0xc3),
    Record(0xc4),
    Record(0xc5),
    Record(0xc6),
    Record(0xc7),
    Record(0xc8),
    Record(0xc9),
    Record(0xd1),
    Record(0xd7),
    Record(0xd8),
];

/// Maps every shape, transform and geometry record in `chunks` into
/// `document`.
///
/// Shapes are added to the document arena in stream order, with the
/// parent and child links of the nesting level preserved and the
/// master, line, fill and text style references kept as Visio ids.
/// Values the model must interpret but cannot map are recorded in
/// `warnings`, never fatal.
///
/// This is the seam of task P8-01: the version 11 payloads and the
/// [`Limits::DEFAULT`] caps. Use [`collect_with`] for the version 6
/// payloads or other caps.
pub fn collect(
    chunks: &[Chunk<'_>],
    document: &mut Document,
    warnings: &mut Vec<ImportWarning>,
) -> Result<(), Error> {
    collect_with(
        chunks,
        FormatVersion::V11,
        &Limits::DEFAULT,
        document,
        warnings,
    )
}

/// Maps `chunks` like [`collect`], for one file version and one set of
/// caps.
///
/// `version` selects the name payload layouts (version 6 writes
/// single-byte text, version 11 UTF-16LE; the module docs have the
/// rule) and `limits` caps the shape count. Every other payload layout
/// is shared between the two versions.
pub fn collect_with(
    chunks: &[Chunk<'_>],
    version: FormatVersion,
    limits: &Limits,
    document: &mut Document,
    warnings: &mut Vec<ImportWarning>,
) -> Result<(), Error> {
    let names = name_table(chunks, version);
    let curves = curve_table(chunks);
    let mut walker = Walker::new(limits);
    for chunk in chunks {
        walker.record(chunk, &names, &curves, &document.masters, warnings)?;
    }
    walker.finish(document);
    Ok(())
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

    /// The next little-endian `u16`, or `None` past the end.
    fn u16(&mut self) -> Option<u16> {
        let bytes = self.take(2)?;
        Some(u16::from_le_bytes(bytes.try_into().ok()?))
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

    /// The next unit-tagged double: one unit byte, then the value.
    fn tagged(&mut self) -> Option<(u8, f64)> {
        let unit = self.u8()?;
        Some((unit, self.f64()?))
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

/// The curve data a `NurbsTo` or `PolylineTo` row encodes (ADR 0019).
#[derive(Debug, Clone, PartialEq)]
enum CurveData {
    /// NURBS control points with their knots and weights.
    Nurbs {
        /// The last knot of the formula.
        knot_last: f64,
        /// The degree of the spline.
        degree: u32,
        /// The `xType` of the coordinates.
        x_type: u32,
        /// The `yType` of the coordinates.
        y_type: u32,
        /// The control points: x, y, knot, weight.
        points: Vec<(f64, f64, f64, f64)>,
    },
    /// Polyline points.
    Polyline {
        /// The `xType` of the coordinates.
        x_type: u32,
        /// The `yType` of the coordinates.
        y_type: u32,
        /// The points: x, y.
        points: Vec<(f64, f64)>,
    },
}

/// Builds the name table: name record id to name text.
fn name_table(chunks: &[Chunk<'_>], version: FormatVersion) -> BTreeMap<u32, String> {
    let mut names = BTreeMap::new();
    for chunk in chunks {
        match chunk.record {
            NAME => {
                names.insert(chunk.id, decode_name(chunk.data, version, false));
            }
            NAME2 => {
                names.insert(chunk.id, decode_name(chunk.data, version, true));
            }
            _ => {}
        }
    }
    names
}

/// Decodes one name payload.
///
/// Version 11 writes UTF-16LE text, version 6 single-byte text (the
/// libvisio version 6 readers). A `NAME2` payload starts with a dummy
/// dword before the text; a `NAME` payload is the text itself. NUL
/// characters terminate the text and are not part of the name.
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

/// Builds the curve-data table: curve record id to its points.
fn curve_table(chunks: &[Chunk<'_>]) -> BTreeMap<u32, CurveData> {
    let mut curves = BTreeMap::new();
    for chunk in chunks {
        if chunk.record == SHAPE_DATA
            && let Some(data) = curve_data(chunk.data)
        {
            curves.insert(chunk.id, data);
        }
    }
    curves
}

/// Reads one curve-data payload (the libvisio `VSD_SHAPE_DATA`
/// layout): one kind byte, 15 reserved bytes, then the points.
fn curve_data(data: &[u8]) -> Option<CurveData> {
    let mut fields = Fields::new(data);
    let kind = fields.u8()?;
    fields.skip(15)?;
    match kind {
        0x80 => {
            let x_type = fields.u8()?;
            let y_type = fields.u8()?;
            let count = point_count(&mut fields, 16);
            let mut points = Vec::new();
            for _ in 0..count {
                let x = fields.f64()?;
                let y = fields.f64()?;
                points.push((x, y));
            }
            Some(CurveData::Polyline {
                x_type: u32::from(x_type),
                y_type: u32::from(y_type),
                points,
            })
        }
        0x82 => {
            let knot_last = fields.f64()?;
            let degree = fields.u16()?;
            let x_type = fields.u8()?;
            let y_type = fields.u8()?;
            let count = point_count(&mut fields, 32);
            let mut points = Vec::new();
            for _ in 0..count {
                let x = fields.f64()?;
                let y = fields.f64()?;
                let knot = fields.f64()?;
                let weight = fields.f64()?;
                points.push((x, y, knot, weight));
            }
            Some(CurveData::Nurbs {
                knot_last,
                degree: u32::from(degree),
                x_type: u32::from(x_type),
                y_type: u32::from(y_type),
                points,
            })
        }
        _ => None,
    }
}

/// The declared point count of a curve payload, capped by what the
/// payload can hold (a lying count never over-allocates).
fn point_count(fields: &mut Fields<'_>, point_bytes: usize) -> u64 {
    let Some(count) = fields.u32() else {
        return 0;
    };
    let remaining = (fields.data.len().saturating_sub(fields.pos) / point_bytes) as u64;
    u64::from(count).min(remaining)
}

/// The `NURBS(...)` formula-encoded value of one curve (ADR 0019), or
/// `None` for a polyline.
fn nurbs_text(curve: &CurveData) -> Option<String> {
    let CurveData::Nurbs {
        knot_last,
        degree,
        x_type,
        y_type,
        points,
    } = curve
    else {
        return None;
    };
    let mut text = format!("NURBS({knot_last}, {degree}, {x_type}, {y_type}");
    for (x, y, knot, weight) in points {
        text.push_str(&format!(", {x}, {y}, {knot}, {weight}"));
    }
    text.push(')');
    Some(text)
}

/// The `POLYLINE(...)` formula-encoded value of one curve (ADR 0019),
/// or `None` for a NURBS curve.
fn polyline_text(curve: &CurveData) -> Option<String> {
    let CurveData::Polyline {
        x_type,
        y_type,
        points,
    } = curve
    else {
        return None;
    };
    let mut text = format!("POLYLINE({x_type}, {y_type}");
    for (x, y) in points {
        text.push_str(&format!(", {x}, {y}"));
    }
    text.push(')');
    Some(text)
}

/// One shape being built, with the record-group state of its geometry.
struct Open {
    /// The arena id the shape will get.
    id: ShapeId,
    /// The nesting level of the shape record.
    level: u16,
    /// The shape itself.
    shape: Shape,
    /// The Visio id the shape record named as its parent, when it named
    /// one (P8-15 goal 3: the group join).
    parent_ref: Option<i32>,
    /// The `Geometry` sections finished so far.
    sections: Vec<Section>,
    /// The section-level cells of the open section.
    section_cells: Vec<Cell>,
    /// The rows of the open section: record id, row.
    rows: Vec<(u32, Row)>,
    /// The row order the geometry list declared, when one did.
    order: Option<Vec<u32>>,
}

impl Open {
    /// The shape being built at `id` and `level`.
    fn new(id: ShapeId, level: u16, shape: Shape) -> Self {
        Open {
            id,
            level,
            shape,
            parent_ref: None,
            sections: Vec::new(),
            section_cells: Vec::new(),
            rows: Vec::new(),
            order: None,
        }
    }

    /// Finishes the open geometry section, if any.
    fn finish_section(&mut self) {
        if self.rows.is_empty() && self.section_cells.is_empty() {
            return;
        }
        let mut section = Section::new(SectionName::from_static("Geometry"));
        section.cells = std::mem::take(&mut self.section_cells);
        let mut rows = std::mem::take(&mut self.rows);
        if let Some(order) = self.order.take() {
            // The list's trailer selects its rows and their order (the
            // reference keeps only the rows the order names): a row no
            // list claims is not part of the geometry (P8-15: six
            // `InfiniteLine` rows the lists never named drew as lines
            // across the page).
            rows.retain(|(id, _)| order.iter().any(|wanted| wanted == id));
            rows.sort_by_key(|(id, _)| {
                order
                    .iter()
                    .position(|wanted| wanted == id)
                    .unwrap_or(usize::MAX)
            });
        }
        for (position, (_, row)) in rows.iter_mut().enumerate() {
            row.ix = Some(
                i32::try_from(position)
                    .unwrap_or(i32::MAX)
                    .saturating_add(1),
            );
            section.rows.push(std::mem::take(row));
        }
        self.sections.push(section);
    }
}

/// The state of one [`collect_with`] walk.
struct Walker<'a> {
    /// The caps applied to the walk.
    limits: &'a Limits,
    /// The open shapes, innermost last.
    stack: Vec<Open>,
    /// The finished shapes by their arena index.
    finished: BTreeMap<u32, Shape>,
    /// The next arena index of a shape, in stream order.
    next_index: u32,
    /// The shape ids the shape lists declared, oldest first.
    shape_ids: VecDeque<u32>,
    /// The name-index tables seen outside shape record groups: element
    /// id to name id, oldest first.
    name_index: Vec<Vec<(u32, u32)>>,
    /// The level of the styles stream record while the walk is inside
    /// the stylesheet, else `None`: its line, fill and text-block rows
    /// are style rows, mapped by [`crate::style`].
    styles_level: Option<u16>,
    /// The group join: arena index of a shape to the Visio id its shape
    /// record named as its parent (P8-15 goal 3).
    parents: Vec<(u32, i32)>,
    /// The master-shape references: arena index of an instance shape to
    /// the Visio id of the master's shape it draws when it has no
    /// geometry of its own (P8-15).
    master_shapes: BTreeMap<u32, u32>,
    /// The shape lists, in stream order: each names its shapes in
    /// z-order (P8-15: the paint order).
    orders: Vec<Vec<i32>>,
    /// The shape list being read, if one is open.
    current_list: Option<ShapeList>,
}

/// One shape list being read: the order its trailer declares and the
/// id table of its shape-id records (P8-15: the paint order).
struct ShapeList {
    /// The list's trailer, as [`geometry_order`] reads it: the record
    /// ids of the list's entries in z-order, when the list has one.
    order: Option<Vec<u32>>,
    /// The shape-id records of the list: record id to shape id.
    ids: Vec<(u32, i32)>,
}

impl ShapeList {
    /// The shape ids of the list in z-order: the trailer's order
    /// mapped through the id table, else the table in stream order.
    fn z_order(self) -> Vec<i32> {
        let Some(order) = self.order.filter(|order| !order.is_empty()) else {
            return self.ids.into_iter().map(|(_, shape)| shape).collect();
        };
        order
            .iter()
            .filter_map(|element| {
                self.ids
                    .iter()
                    .find(|(record, _)| record == element)
                    .map(|(_, shape)| *shape)
            })
            .collect()
    }
}

impl<'a> Walker<'a> {
    /// A walker with no shapes yet.
    fn new(limits: &'a Limits) -> Self {
        Walker {
            limits,
            stack: Vec::new(),
            finished: BTreeMap::new(),
            next_index: 0,
            shape_ids: VecDeque::new(),
            name_index: Vec::new(),
            styles_level: None,
            parents: Vec::new(),
            master_shapes: BTreeMap::new(),
            orders: Vec::new(),
            current_list: None,
        }
    }

    /// Ends the open shape list, keeping its z-order.
    fn flush_order(&mut self) {
        if let Some(list) = self.current_list.take()
            && !list.ids.is_empty()
        {
            self.orders.push(list.z_order());
        }
    }

    /// Maps one chunk record.
    fn record(
        &mut self,
        chunk: &Chunk<'_>,
        names: &BTreeMap<u32, String>,
        curves: &BTreeMap<u32, CurveData>,
        masters: &[Master],
        warnings: &mut Vec<ImportWarning>,
    ) -> Result<(), Error> {
        self.close_to(chunk.level);
        if chunk.record == STYLES_STREAM {
            self.styles_level = Some(chunk.level);
            return Ok(());
        }
        if let Some(base) = self.styles_level
            && chunk.level <= base
        {
            self.styles_level = None;
        }
        if matches!(
            chunk.record,
            SHAPE_GROUP | SHAPE_SHAPE | SHAPE_GUIDE | SHAPE_FOREIGN
        ) {
            return self.open_shape(chunk, names, masters, warnings);
        }
        match chunk.record {
            SHAPE_LIST => {
                self.flush_order();
                self.shape_ids.clear();
                self.current_list = Some(ShapeList {
                    order: geometry_order(chunk.data, warnings, &self.context()),
                    ids: Vec::new(),
                });
            }
            SHAPE_ID => match Fields::new(chunk.data).u32() {
                Some(id) => {
                    self.shape_ids.push_back(id);
                    if let Some(list) = self.current_list.as_mut() {
                        list.ids.push((chunk.id, i32::try_from(id).unwrap_or(0)));
                    }
                }
                None => warnings.push(bad_value(self.context(), "shape id")),
            },
            NAME_IDX => self.read_name_index(chunk.data, chunk.level),
            NAME_IDX123 => self.read_name_index_123(chunk.data, chunk.level),
            XFORM_DATA | XFORM_1D | TEXT_XFORM => {
                if let Some(open) = self.stack.last_mut() {
                    read_transform(chunk.record, chunk.data, open, warnings);
                }
            }
            GEOM_LIST => {
                let context = self.context();
                if let Some(open) = self.stack.last_mut() {
                    open.finish_section();
                    open.order = geometry_order(chunk.data, warnings, &context);
                }
            }
            GEOMETRY => {
                if let Some(open) = self.stack.last_mut() {
                    section_flags(chunk.data, &mut open.section_cells);
                }
            }
            _ if is_geometry_row(chunk.record) => {
                if let Some(open) = self.stack.last_mut() {
                    let row = geometry_row(chunk.record, chunk.data, curves, warnings, open);
                    open.rows.push((chunk.id, row));
                }
            }
            LINE | FILL_AND_SHADOW | TEXT_BLOCK => {
                // A shape's own `Line`, `FillAndShadow` and `TextBlock`
                // rows (P8-15 goal 1): the styles stream's rows are
                // `crate::style`'s to map.
                if self.styles_level.is_none()
                    && let Some(open) = self.stack.last_mut()
                    && chunk.level > open.level
                {
                    let context = format!("shape {}", open.shape.visio_id);
                    let sheet = &mut open.shape.sheet;
                    match chunk.record {
                        LINE => style::map_line(chunk.data, sheet, &context, warnings),
                        FILL_AND_SHADOW => style::map_fill(chunk.data, sheet, &context, warnings),
                        _ => style::map_text_block(chunk.data, sheet, &context, warnings),
                    }
                }
            }
            _ => self.skip_other(chunk.record, warnings),
        }
        Ok(())
    }

    /// Closes every open shape at a level of `level` or deeper.
    fn close_to(&mut self, level: u16) {
        while let Some(mut open) = self.stack.pop_if(|open| open.level >= level) {
            open.finish_section();
            open.shape.sheet.sections.append(&mut open.sections);
            if let Some(parent) = open.parent_ref {
                self.parents.push((open.id.index, parent));
            }
            self.finished.insert(open.id.index, open.shape);
        }
    }

    /// Starts a new shape from a shape record.
    fn open_shape(
        &mut self,
        chunk: &Chunk<'_>,
        names: &BTreeMap<u32, String>,
        masters: &[Master],
        warnings: &mut Vec<ImportWarning>,
    ) -> Result<(), Error> {
        if self.next_index >= self.limits.max_shapes {
            return Err(Error::LimitExceeded {
                limit: "max_shapes",
                value: u64::from(self.next_index),
                max: u64::from(self.limits.max_shapes),
            });
        }
        let id = ShapeId {
            index: self.next_index,
        };
        self.next_index += 1;
        let (visio_id, missing) = self.take_shape_id(chunk);
        let mut shape = Shape::new(id, visio_id);
        // A guide shape record draws on screen only: the renderer skips
        // it (P8-15: `Visio6PlanWithDimensions.vsd`'s six dimension
        // guides drew as lines across the page).
        shape.is_guide = chunk.record == SHAPE_GUIDE;
        let context = format!("shape {visio_id}");
        if missing {
            warnings.push(bad_value(context.clone(), "shape id"));
        }
        shape.parent = self.stack.last().map(|open| open.id);
        if let Some(open) = self.stack.last_mut() {
            open.shape.children.push(id);
        }
        if let Some(name_id) = self.lookup_name(visio_id) {
            match names.get(&name_id) {
                Some(name) => {
                    shape.name = name.clone();
                    shape.name_u = name.clone();
                }
                None => warnings.push(ImportWarning {
                    context: context.clone(),
                    kind: WarningKind::UnresolvedReference { what: "shape name" },
                }),
            }
        }
        let (master, master_shape, parent) =
            read_shape_record(chunk.data, &mut shape, &context, warnings);
        if let Some(master_shape) = master_shape {
            self.master_shapes.insert(id.index, master_shape);
        }
        if let Some(master) = master {
            match masters
                .iter()
                .position(|candidate| candidate.visio_id == i32::try_from(master).unwrap_or(0))
            {
                Some(index) => {
                    shape.master = Some(MasterId {
                        index: u32::try_from(index).unwrap_or(u32::MAX),
                    });
                }
                None => warnings.push(ImportWarning {
                    context: context.clone(),
                    kind: WarningKind::UnresolvedReference { what: "master" },
                }),
            }
        }
        let mut open = Open::new(id, chunk.level, shape);
        open.parent_ref = parent;
        self.stack.push(open);
        Ok(())
    }

    /// The Visio id of a new shape and whether it had none: the shape
    /// record's own id, else the next shape-id record of the most
    /// recent shape list (the reference names a shape by its record id;
    /// the list is the group's child order).
    fn take_shape_id(&mut self, chunk: &Chunk<'_>) -> (i32, bool) {
        if chunk.id != u32::MAX {
            return (i32::try_from(chunk.id).unwrap_or(0), false);
        }
        if let Some(id) = self.shape_ids.pop_front() {
            return (i32::try_from(id).unwrap_or(0), false);
        }
        (0, true)
    }

    /// The name of a shape, from the nearest name index with an entry
    /// for its id.
    fn lookup_name(&self, visio_id: i32) -> Option<u32> {
        let wanted = u32::try_from(visio_id).ok()?;
        self.name_index
            .iter()
            .rev()
            .flatten()
            .find(|(element, _)| *element == wanted)
            .map(|(_, name)| *name)
    }

    /// Reads a name-index payload: element id to name id pairs.
    fn read_name_index(&mut self, data: &[u8], level: u16) {
        let mut fields = Fields::new(data);
        let Some(count) = fields.u32() else {
            return;
        };
        let mut table = Vec::new();
        for _ in 0..count {
            let Some(name) = fields.u32() else {
                break;
            };
            let Some(_name_again) = fields.u32() else {
                break;
            };
            let Some(element) = fields.u32() else {
                break;
            };
            let Some(_flag) = fields.u8() else {
                break;
            };
            table.push((element, name));
        }
        self.register_index(table, level);
    }

    /// Reads a name-index payload with 8-byte entries: element id to
    /// name id pairs until the payload ends.
    fn read_name_index_123(&mut self, data: &[u8], level: u16) {
        let mut fields = Fields::new(data);
        let mut table = Vec::new();
        while let Some(name) = fields.u32() {
            let Some(element) = fields.u32() else {
                break;
            };
            table.push((element, name));
        }
        self.register_index(table, level);
    }

    /// Records one name-index table, unless it sits inside a shape
    /// record group (deeper than the innermost open shape): those name
    /// property rows, not shapes.
    fn register_index(&mut self, table: Vec<(u32, u32)>, level: u16) {
        if self.stack.last().is_none_or(|open| level <= open.level) {
            self.name_index.push(table);
        }
    }

    /// Warns for a record this module does not consume, inside a shape
    /// record group (a page or container record is another module's to
    /// report).
    fn skip_other(&mut self, record: Record, warnings: &mut Vec<ImportWarning>) {
        if self.stack.is_empty() {
            return;
        }
        let context = self.context();
        if let Some((_, what)) = DROPPED.iter().find(|(kind, _)| *kind == record) {
            warnings.push(ImportWarning {
                context,
                kind: WarningKind::Dropped { what },
            });
        } else if !KNOWN.contains(&record) {
            warnings.push(ImportWarning {
                context,
                kind: WarningKind::SkippedRecord {
                    name: "unknown record",
                },
            });
        }
    }

    /// Where the walk is, for a warning context.
    fn context(&self) -> String {
        match self.stack.last() {
            Some(open) => format!("shape {}", open.shape.visio_id),
            None => "shapes".to_owned(),
        }
    }

    /// Closes every open shape and adds the finished shapes to the
    /// document, in stream order.
    fn finish(mut self, document: &mut Document) {
        self.close_to(0);
        self.flush_order();
        for (_, shape) in self.finished {
            document.add_shape(shape);
        }
        regroup(document, &self.parents);
        reorder(document, &self.orders);
        inherit_geometry(document, &self.master_shapes);
    }
}

/// Copies the master's geometry onto the instances that draw it (P8-15).
///
/// A `.vsd` instance keeps only the geometry that differs from its
/// master: a shape with no `Geometry` section of its own draws the
/// master's shape its record names. The model's own inheritance (ADR
/// 0013) resolves cells and the master's top-level shape, not the
/// geometry of a named master sub-shape, so the mapping fills the
/// instance in here; the cells stay inherited.
fn inherit_geometry(document: &mut Document, master_shapes: &BTreeMap<u32, u32>) {
    for (&index, &wanted) in master_shapes {
        let Some(instance) = document.shapes.get(index as usize) else {
            continue;
        };
        let has_rows = instance
            .sheet
            .sections
            .iter()
            .any(|section| section.name.as_str() == "Geometry" && !section.rows.is_empty());
        if has_rows {
            continue;
        }
        let Some(master) = instance.master else {
            continue;
        };
        let Some(source) = master_shape_with_geometry(document, master, wanted) else {
            continue;
        };
        let sections: Vec<Section> = source
            .sheet
            .sections
            .iter()
            .filter(|section| section.name.as_str() == "Geometry")
            .cloned()
            .collect();
        if let Some(instance) = document.shapes.get_mut(index as usize) {
            instance.sheet.sections.extend(sections);
        }
    }
}

/// The shape of `master` whose Visio id is `wanted`, with its sub-shape
/// tree searched too.
fn master_shape_with_geometry(
    document: &Document,
    master: MasterId,
    wanted: u32,
) -> Option<&Shape> {
    let master = document.master(master)?;
    let mut pending: Vec<ShapeId> = master.shapes.clone();
    while let Some(id) = pending.pop() {
        let Some(shape) = document.shape(id) else {
            continue;
        };
        if u32::try_from(shape.visio_id).ok() == Some(wanted) {
            return Some(shape);
        }
        pending.extend(shape.children.iter().copied());
    }
    None
}

/// Re-links the shapes whose records name a parent (P8-15 goal 3).
///
/// A group's sub-shapes are its siblings in the stream: the record
/// nesting level alone yields a flat list. The shape record's parent
/// field names the group by its Visio id, so each reference resolves to
/// the closest earlier shape with that id (shape ids repeat per page,
/// and the parent record comes first in the stream). The resolved
/// parent replaces the level-derived one, the children lists and the
/// top-level shape lists of the pages and masters follow, and every
/// shape keeps at most one parent: a reference that would close a cycle
/// (it can only point backwards) cannot.
fn regroup(document: &mut Document, parents: &[(u32, i32)]) {
    let visio_ids: Vec<i32> = document.shapes.iter().map(|shape| shape.visio_id).collect();
    let mut parent_of: Vec<Option<u32>> = document
        .shapes
        .iter()
        .map(|shape| shape.parent.map(|parent| parent.index))
        .collect();
    for &(child, parent_visio) in parents {
        let child_index = child as usize;
        // The walk stays inside the arena whatever the reference
        // carries: an entry past the end simply resolves to nothing.
        let upper = child_index.min(visio_ids.len());
        let target = (0..upper)
            .rev()
            .find(|candidate| visio_ids.get(*candidate) == Some(&parent_visio));
        if let (Some(slot), Some(target)) = (parent_of.get_mut(child_index), target) {
            *slot = u32::try_from(target).ok();
        }
    }
    for shape in &mut document.shapes {
        shape.parent = None;
        shape.children.clear();
    }
    for (index, parent) in parent_of.iter().enumerate() {
        let id = ShapeId {
            index: u32::try_from(index).unwrap_or(u32::MAX),
        };
        if let Some(parent) = parent {
            if let Some(shape) = document.shapes.get_mut(index) {
                shape.parent = Some(ShapeId { index: *parent });
            }
            if let Some(parent_shape) = document
                .shapes
                .get_mut(usize::try_from(*parent).unwrap_or(usize::MAX))
            {
                parent_shape.children.push(id);
            }
        }
    }
    let top_level = |id: &ShapeId| {
        parent_of
            .get(usize::try_from(id.index).unwrap_or(usize::MAX))
            .is_none_or(Option::is_none)
    };
    for page in &mut document.pages {
        page.shapes.retain(top_level);
    }
    for master in &mut document.masters {
        master.shapes.retain(top_level);
    }
}

/// Re-orders the shapes of every page, master and group by the shape
/// lists (P8-15: the paint order).
///
/// A shape list names its shapes in z-order (the order the reference
/// calls the shapes order). The stream order the arena keeps is not
/// that order: a filled shape can be written before the lines drawn
/// over it. Each list is matched to the page, master or group whose
/// shapes it names exactly (the ids are Visio ids and repeat per
/// page, so the match is by the whole set).
fn reorder(document: &mut Document, orders: &[Vec<i32>]) {
    let visio_ids: Vec<i32> = document.shapes.iter().map(|shape| shape.visio_id).collect();
    let mut used = vec![false; orders.len()];
    let apply = |shapes: &mut [ShapeId], used: &mut [bool]| {
        let ids: Vec<i32> = shapes
            .iter()
            .map(|id| {
                visio_ids
                    .get(usize::try_from(id.index).unwrap_or(usize::MAX))
                    .copied()
                    .unwrap_or(i32::MAX)
            })
            .collect();
        let Some(order) = match_order(&ids, orders, used) else {
            return;
        };
        shapes.sort_by_key(|id| {
            let visio = visio_ids
                .get(usize::try_from(id.index).unwrap_or(usize::MAX))
                .copied()
                .unwrap_or(i32::MAX);
            order
                .iter()
                .position(|wanted| *wanted == visio)
                .unwrap_or(usize::MAX)
        });
    };
    for page in &mut document.pages {
        apply(&mut page.shapes, &mut used);
    }
    for master in &mut document.masters {
        apply(&mut master.shapes, &mut used);
    }
    for index in 0..document.shapes.len() {
        if let Some(shape) = document.shapes.get_mut(index) {
            let mut children = std::mem::take(&mut shape.children);
            apply(&mut children, &mut used);
            shape.children = children;
        }
    }
}

/// The unused shape list that names exactly `ids`, marked as used.
fn match_order(ids: &[i32], orders: &[Vec<i32>], used: &mut [bool]) -> Option<Vec<i32>> {
    let mut wanted = ids.to_vec();
    wanted.sort_unstable();
    for (index, order) in orders.iter().enumerate() {
        if used.get(index).copied().unwrap_or(true) {
            continue;
        }
        let mut sorted = order.clone();
        sorted.sort_unstable();
        if sorted == wanted {
            if let Some(slot) = used.get_mut(index) {
                *slot = true;
            }
            return Some(order.clone());
        }
    }
    None
}

/// Reads the shape record payload: the parent, master and style
/// references.
///
/// The first ten bytes are flags this module does not map, then the
/// parent id, the master page and shape ids, and the fill, line and
/// text style ids. A reference of `0xFFFFFFFF` is no reference. The
/// master and master-shape ids are returned (the first for resolution
/// against the master list, the second names the master's shape whose
/// geometry an instance without its own draws), and the parent id for
/// the group join (P8-15 goal 3); the style ids stay Visio ids on the
/// shape.
fn read_shape_record(
    data: &[u8],
    shape: &mut Shape,
    context: &str,
    warnings: &mut Vec<ImportWarning>,
) -> (Option<u32>, Option<u32>, Option<i32>) {
    let mut fields = Fields::new(data);
    if fields.skip(10).is_none() {
        warnings.push(bad_value(context.to_owned(), "shape record"));
        return (None, None, None);
    }
    let parent = fields.u32();
    if fields.skip(4).is_none() {
        warnings.push(bad_value(context.to_owned(), "shape record"));
        return (None, None, None);
    }
    let master = fields.u32();
    fields.skip(4);
    let master_shape = fields.u32();
    fields.skip(4);
    let fill = fields.u32();
    fields.skip(4);
    let line = fields.u32();
    fields.skip(4);
    let text = fields.u32();
    shape.fill_style = reference(fill);
    shape.line_style = reference(line);
    shape.text_style = reference(text);
    let parent = match parent {
        // `0` and `0xFFFFFFFF` both mean "no parent": the shape is one
        // of its page's top-level shapes.
        Some(0) | Some(u32::MAX) | None => None,
        Some(value) => i32::try_from(value).ok(),
    };
    (reference(master), reference(master_shape), parent)
}

/// A style or master reference: `0xFFFFFFFF` means no reference.
fn reference(value: Option<u32>) -> Option<u32> {
    match value {
        Some(value) if value != u32::MAX => Some(value),
        _ => None,
    }
}

/// Whether `record` is a geometry row record.
fn is_geometry_row(record: Record) -> bool {
    matches!(
        record,
        MOVE_TO
            | LINE_TO
            | ARC_TO
            | INFINITE_LINE
            | ELLIPSE
            | ELLIPTICAL_ARC_TO
            | SPLINE_START
            | SPLINE_KNOT
            | POLYLINE_TO
            | NURBS_TO
    )
}

/// The row order of one geometry list: the ids of its children.
fn geometry_order(
    data: &[u8],
    warnings: &mut Vec<ImportWarning>,
    context: &str,
) -> Option<Vec<u32>> {
    let mut fields = Fields::new(data);
    let sub_header = fields.u32()?;
    let bytes = fields.u32()?;
    fields.skip(sub_header as usize)?;
    let count = u64::from(bytes) / 4;
    let mut order = Vec::new();
    for _ in 0..count {
        let Some(id) = fields.u32() else {
            warnings.push(bad_value(context.to_owned(), "geometry row order"));
            return None;
        };
        order.push(id);
    }
    Some(order)
}

/// Reads the `NoFill`, `NoLine` and `NoShow` cells of a geometry
/// section from its flag record.
fn section_flags(data: &[u8], cells: &mut Vec<Cell>) {
    let Some(flags) = data.first() else {
        return;
    };
    for (bit, name) in [(1u8, "NoFill"), (2, "NoLine"), (4, "NoShow")] {
        cells.push(bool_cell(name, flags & bit != 0));
    }
}

/// A cell holding a boolean value.
fn bool_cell(name: &'static str, value: bool) -> Cell {
    let mut cell = Cell::new(CellName::from_static(name));
    cell.value = Some(CellValue::Bool(value));
    cell
}

/// A cell holding a number in `unit`.
fn number_cell(name: &'static str, value: f64, unit: Unit) -> Cell {
    let mut cell = Cell::new(CellName::from_static(name));
    cell.value = Some(CellValue::Number(value));
    cell.unit = unit;
    cell
}

/// A cell holding a formula-encoded string value.
fn text_cell(name: &'static str, value: String) -> Cell {
    let mut cell = Cell::new(CellName::from_static(name));
    cell.value = Some(CellValue::Str(value));
    cell
}

/// The unit a unit-tag byte names, or `None` when it names no unit of
/// the format (the `CELL_TYPE` table of the reference). The tag names
/// the unit a value is *displayed* in; the value itself is canonical.
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

/// A `BadValue` warning in `context`.
fn bad_value(context: String, what: &'static str) -> ImportWarning {
    ImportWarning {
        context,
        kind: WarningKind::BadValue { what },
    }
}

/// Maps one transform record onto the shape's direct cells.
fn read_transform(record: Record, data: &[u8], open: &mut Open, warnings: &mut Vec<ImportWarning>) {
    let names: &[&str] = match record {
        XFORM_DATA => &[
            "PinX", "PinY", "Width", "Height", "LocPinX", "LocPinY", "Angle",
        ],
        TEXT_XFORM => &[
            "TxtPinX",
            "TxtPinY",
            "TxtWidth",
            "TxtHeight",
            "TxtLocPinX",
            "TxtLocPinY",
            "TxtAngle",
        ],
        _ => &["BeginX", "BeginY", "EndX", "EndY"],
    };
    let context = format!("shape {}", open.shape.visio_id);
    let mut fields = Fields::new(data);
    for name in names {
        let Some((tag, value)) = fields.tagged() else {
            warnings.push(bad_value(context, "transform value"));
            return;
        };
        push_number(
            &mut open.shape.sheet.cells,
            name,
            value,
            tag,
            &context,
            warnings,
        );
    }
    if record == XFORM_DATA {
        match (fields.u8(), fields.u8()) {
            (Some(x), Some(y)) => {
                open.shape.sheet.cells.push(bool_cell("FlipX", x != 0));
                open.shape.sheet.cells.push(bool_cell("FlipY", y != 0));
            }
            _ => warnings.push(bad_value(context, "transform value")),
        }
    }
}

/// Adds a numeric cell for one unit-tagged value, or warns and leaves
/// the cell unset when the value or its unit is bad.
fn push_number(
    cells: &mut Vec<Cell>,
    name: &'static str,
    value: f64,
    tag: u8,
    context: &str,
    warnings: &mut Vec<ImportWarning>,
) {
    let Some(unit) = cell_unit(tag) else {
        warnings.push(bad_value(context.to_owned(), "unit"));
        return;
    };
    push_valued(cells, name, value, unit, context, warnings);
}

/// Adds a numeric cell for one untagged value (a knot, weight or
/// degree: a plain number), or warns and leaves the cell unset.
fn push_plain(
    cells: &mut Vec<Cell>,
    name: &'static str,
    value: f64,
    context: &str,
    warnings: &mut Vec<ImportWarning>,
) {
    push_valued(cells, name, value, Unit::Unitsless, context, warnings);
}

/// Adds a numeric cell, or warns and leaves the cell unset when the
/// value is not finite.
fn push_valued(
    cells: &mut Vec<Cell>,
    name: &'static str,
    value: f64,
    unit: Unit,
    context: &str,
    warnings: &mut Vec<ImportWarning>,
) {
    if value.is_finite() {
        cells.push(number_cell(name, value, unit));
    } else {
        warnings.push(bad_value(context.to_owned(), "non-finite value"));
    }
}

/// Maps one geometry row record onto a row of the open section.
fn geometry_row(
    record: Record,
    data: &[u8],
    curves: &BTreeMap<u32, CurveData>,
    warnings: &mut Vec<ImportWarning>,
    open: &Open,
) -> Row {
    let context = format!("shape {}", open.shape.visio_id);
    let mut row = Row::new();
    let mut fields = Fields::new(data);
    row.row_type = Some(
        match record {
            MOVE_TO => "MoveTo",
            LINE_TO => "LineTo",
            ARC_TO => "ArcTo",
            ELLIPTICAL_ARC_TO => "EllipticalArcTo",
            ELLIPSE => "Ellipse",
            INFINITE_LINE => "InfiniteLine",
            SPLINE_START => "SplineStart",
            SPLINE_KNOT => "SplineKnot",
            POLYLINE_TO => "PolylineTo",
            _ => "NurbsTo",
        }
        .to_owned(),
    );
    match record {
        MOVE_TO | LINE_TO => read_tagged(&mut row, &mut fields, &["X", "Y"], &context, warnings),
        ARC_TO => read_tagged(&mut row, &mut fields, &["X", "Y", "A"], &context, warnings),
        INFINITE_LINE => read_tagged(
            &mut row,
            &mut fields,
            &["X", "Y", "A", "B"],
            &context,
            warnings,
        ),
        ELLIPTICAL_ARC_TO | ELLIPSE => read_tagged(
            &mut row,
            &mut fields,
            &["X", "Y", "A", "B", "C", "D"],
            &context,
            warnings,
        ),
        SPLINE_START => {
            read_tagged(&mut row, &mut fields, &["X", "Y"], &context, warnings);
            read_plain(&mut row, &mut fields, &["A", "B", "C"], &context, warnings);
            match fields.u8() {
                Some(degree) => {
                    row.cells
                        .push(number_cell("D", f64::from(degree), Unit::Unitsless));
                }
                None => warnings.push(bad_value(context, "geometry value")),
            }
        }
        SPLINE_KNOT => {
            read_tagged(&mut row, &mut fields, &["X", "Y"], &context, warnings);
            read_plain(&mut row, &mut fields, &["A"], &context, warnings);
        }
        NURBS_TO => {
            read_tagged(&mut row, &mut fields, &["X", "Y"], &context, warnings);
            read_plain(
                &mut row,
                &mut fields,
                &["A", "B", "C", "D"],
                &context,
                warnings,
            );
            if let Some(text) = nurbs_row_text(&mut fields, curves) {
                row.cells.push(text_cell("E", text));
            }
        }
        _ => {
            read_tagged(&mut row, &mut fields, &["X", "Y"], &context, warnings);
            if let Some(text) = polyline_row_text(&mut fields, curves) {
                row.cells.push(text_cell("A", text));
            }
        }
    }
    row
}

/// Reads the unit-tagged cells of one geometry row.
fn read_tagged(
    row: &mut Row,
    fields: &mut Fields<'_>,
    names: &[&'static str],
    context: &str,
    warnings: &mut Vec<ImportWarning>,
) {
    for name in names {
        let Some((tag, value)) = fields.tagged() else {
            warnings.push(bad_value(context.to_owned(), "geometry value"));
            return;
        };
        push_number(&mut row.cells, name, value, tag, context, warnings);
    }
}

/// Reads the plain-number cells of one geometry row (knots, weights).
fn read_plain(
    row: &mut Row,
    fields: &mut Fields<'_>,
    names: &[&'static str],
    context: &str,
    warnings: &mut Vec<ImportWarning>,
) {
    for name in names {
        let Some(value) = fields.f64() else {
            warnings.push(bad_value(context.to_owned(), "geometry value"));
            return;
        };
        push_plain(&mut row.cells, name, value, context, warnings);
    }
}

/// The `NURBS(...)` value of a `NurbsTo` row: the curve data the row
/// references by id, or the inline formula block of its payload.
fn nurbs_row_text(fields: &mut Fields<'_>, curves: &BTreeMap<u32, CurveData>) -> Option<String> {
    fields.skip(1)?;
    match fields.u8()? {
        0x8a => {
            fields.skip(3)?;
            let id = fields.u32()?;
            curves.get(&id).and_then(nurbs_text)
        }
        _ => inline_nurbs(fields).as_ref().and_then(nurbs_text),
    }
}

/// The `POLYLINE(...)` value of a `PolylineTo` row: the curve data the
/// row references by id, or the inline formula block of its payload.
fn polyline_row_text(fields: &mut Fields<'_>, curves: &BTreeMap<u32, CurveData>) -> Option<String> {
    fields.skip(1)?;
    match fields.u8()? {
        0x8b => {
            fields.skip(3)?;
            let id = fields.u32()?;
            curves.get(&id).and_then(polyline_text)
        }
        _ => inline_polyline(fields).as_ref().and_then(polyline_text),
    }
}

/// The inline NURBS formula block of a `NurbsTo` payload (the libvisio
/// `VSD_NURBS_TO` layout): a stream of length-prefixed cell blocks of
/// which the `E` cell (index 6) carries the curve.
fn inline_nurbs(fields: &mut Fields<'_>) -> Option<CurveData> {
    fields.skip(9)?;
    let mut cell_ref = 0u8;
    while cell_ref != 6 {
        let length = fields.u32()?;
        fields.skip(1)?;
        cell_ref = fields.u8()?;
        if cell_ref < 6 {
            fields.skip(length.saturating_sub(6) as usize)?;
        }
    }
    let param = fields.u8()?;
    if param == 0x8a {
        let knot_last = fields.f64()?;
        let degree = fields.u16()?;
        let x_type = fields.u8()?;
        let y_type = fields.u8()?;
        let count = point_count(fields, 32);
        let mut points = Vec::new();
        for _ in 0..count {
            let x = fields.f64()?;
            let y = fields.f64()?;
            let knot = fields.f64()?;
            let weight = fields.f64()?;
            points.push((x, y, knot, weight));
        }
        return Some(CurveData::Nurbs {
            knot_last,
            degree: u32::from(degree),
            x_type: u32::from(x_type),
            y_type: u32::from(y_type),
            points,
        });
    }
    let knot_last = typed_number(fields, param)?;
    fields.skip(1)?;
    let degree = fields.u16()?;
    fields.skip(1)?;
    let x_type = fields.u16()?;
    fields.skip(1)?;
    let y_type = fields.u16()?;
    let mut points = Vec::new();
    let mut flag = fields.u8()?;
    while flag != 0x81 {
        let x = typed_number(fields, flag)?;
        let y_kind = fields.u8()?;
        let y = typed_number(fields, y_kind)?;
        let knot_kind = fields.u8()?;
        let knot = typed_number(fields, knot_kind)?;
        let weight_kind = fields.u8()?;
        let weight = typed_number(fields, weight_kind)?;
        points.push((x, y, knot, weight));
        flag = fields.u8()?;
    }
    Some(CurveData::Nurbs {
        knot_last,
        degree: u32::from(degree),
        x_type: u32::from(x_type),
        y_type: u32::from(y_type),
        points,
    })
}

/// One typed number of an inline formula block: a double when the type
/// byte says `0x20`, an unsigned word otherwise (the libvisio typed
/// block layout).
fn typed_number(fields: &mut Fields<'_>, kind: u8) -> Option<f64> {
    match kind {
        0x20 => fields.f64(),
        _ => fields.u16().map(f64::from),
    }
}

/// The inline polyline formula block of a `PolylineTo` payload (the
/// libvisio `VSD_POLYLINE_TO` layout).
fn inline_polyline(fields: &mut Fields<'_>) -> Option<CurveData> {
    fields.skip(9)?;
    let mut cell_ref = 0u8;
    while cell_ref != 2 {
        let length = fields.u32()?;
        if length == 0 {
            return None;
        }
        fields.skip(1)?;
        cell_ref = fields.u8()?;
        if cell_ref < 2 {
            fields.skip(length.saturating_sub(6) as usize)?;
        }
    }
    fields.skip(1)?;
    let x_type = fields.u16()?;
    fields.skip(1)?;
    let y_type = fields.u16()?;
    let mut points = Vec::new();
    let mut flag = fields.u8()?;
    while flag != 0x81 {
        let x = typed_number(fields, flag)?;
        let y_kind = fields.u8()?;
        let y = typed_number(fields, y_kind)?;
        points.push((x, y));
        flag = fields.u8()?;
    }
    Some(CurveData::Polyline {
        x_type: u32::from(x_type),
        y_type: u32::from(y_type),
        points,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// The bytes of a little-endian double.
    fn double(value: f64) -> Vec<u8> {
        value.to_le_bytes().to_vec()
    }

    /// The bytes of one unit-tagged value.
    fn tagged(value: f64, unit: u8) -> Vec<u8> {
        let mut bytes = vec![unit];
        bytes.extend(double(value));
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

    /// A shape record payload with the given references.
    fn shape_record(master: u32, master_shape: u32, fill: u32, line: u32, text: u32) -> Vec<u8> {
        join(&[
            vec![0; 10],
            word(0), // parent id
            word(0),
            word(master),
            word(0),
            word(master_shape),
            word(0),
            word(fill),
            word(0),
            word(line),
            word(0),
            word(text),
        ])
    }

    /// A shape-id record payload.
    fn shape_id(visio_id: u32) -> Vec<u8> {
        join(&[word(visio_id), word(0x48), vec![0, 0]])
    }

    /// An `XFormData` payload: the seven values and the two flips.
    fn xform(values: &[(f64, u8)], flip_x: u8, flip_y: u8) -> Vec<u8> {
        let mut bytes = Vec::new();
        for (value, unit) in values {
            bytes.extend(tagged(*value, *unit));
        }
        bytes.push(flip_x);
        bytes.push(flip_y);
        bytes
    }

    /// A geometry list payload declaring `order` as its children.
    fn geom_list(order: &[u32]) -> Vec<u8> {
        join(&[
            word(24),
            word((order.len() * 4) as u32),
            vec![0; 24],
            order.iter().flat_map(|id| word(*id)).collect(),
        ])
    }

    /// A name-index payload with one entry per pair.
    fn name_index(entries: &[(u32, u32)]) -> Vec<u8> {
        let mut bytes = word(entries.len() as u32);
        for (name, element) in entries {
            bytes.extend(word(*name));
            bytes.extend(word(*name));
            bytes.extend(word(*element));
            bytes.push(0x11);
        }
        bytes
    }

    /// A curve-data payload for a polyline.
    fn polyline_data(x_type: u8, y_type: u8, points: &[(f64, f64)]) -> Vec<u8> {
        let mut bytes = vec![0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        bytes.push(x_type);
        bytes.push(y_type);
        bytes.extend(word(points.len() as u32));
        for (x, y) in points {
            bytes.extend(double(*x));
            bytes.extend(double(*y));
        }
        bytes
    }

    /// A curve-data payload for a NURBS curve.
    fn nurbs_data(
        knot_last: f64,
        degree: u16,
        x_type: u8,
        y_type: u8,
        points: &[(f64, f64, f64, f64)],
    ) -> Vec<u8> {
        let mut bytes = vec![0x82, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        bytes.extend(double(knot_last));
        bytes.extend(degree.to_le_bytes());
        bytes.push(x_type);
        bytes.push(y_type);
        bytes.extend(word(points.len() as u32));
        for (x, y, knot, weight) in points {
            bytes.extend(double(*x));
            bytes.extend(double(*y));
            bytes.extend(double(*knot));
            bytes.extend(double(*weight));
        }
        bytes
    }

    /// The value of one cell, or `None`.
    fn cell_str<'a>(cells: &'a [Cell], name: &str) -> Option<&'a CellValue> {
        cells
            .iter()
            .find(|cell| cell.name.as_str() == name)
            .and_then(|cell| cell.value.as_ref())
    }

    /// The number of a cell, or `None`.
    fn number(cells: &[Cell], name: &str) -> Option<f64> {
        match cell_str(cells, name) {
            Some(CellValue::Number(value)) => Some(*value),
            _ => None,
        }
    }

    /// The unit of a cell, or `None` when the cell is absent.
    fn unit_of(cells: &[Cell], name: &str) -> Option<Unit> {
        cells
            .iter()
            .find(|cell| cell.name.as_str() == name)
            .map(|cell| cell.unit)
    }

    /// A document with one master of Visio id 7.
    fn document_with_master() -> Document {
        let mut document = Document::new();
        document.masters.push(Master::new(MasterId { index: 0 }, 7));
        document
    }

    /// The warning kinds collected, in order.
    fn kinds(warnings: &[ImportWarning]) -> Vec<WarningKind> {
        warnings
            .iter()
            .map(|warning| warning.kind.clone())
            .collect()
    }

    // -- shapes, ids, names --------------------------------------------

    #[test]
    fn collect_maps_a_shape_with_ids_names_and_transforms() {
        let chunks = vec![
            chunk(0x33, 0, 3, join(&[word(1), name16("Square")])),
            chunk(0xc9, 2, 0, name_index(&[(3, 1)])),
            chunk(0x65, 2, 0, vec![]),
            chunk(0x83, 3, 0, shape_id(1)),
            chunk(0x48, 1, 1, shape_record(u32::MAX, u32::MAX, 4, 5, 6)),
            chunk(
                0x9b,
                2,
                0,
                xform(
                    &[
                        (2.75, 65),
                        (6.07, 65),
                        (1.5, 70),
                        (1.5, 70),
                        (0.75, 64),
                        (0.75, 64),
                        (2.5, 81),
                    ],
                    1,
                    0,
                ),
            ),
            chunk(
                0x9d,
                2,
                0,
                join(&[
                    tagged(1.0, 65),
                    tagged(2.0, 65),
                    tagged(3.0, 65),
                    tagged(4.0, 65),
                ]),
            ),
            chunk(
                0x9c,
                2,
                0,
                join(&[
                    tagged(1.0, 65),
                    tagged(2.0, 65),
                    tagged(3.0, 65),
                    tagged(4.0, 65),
                    tagged(0.5, 65),
                    tagged(0.5, 65),
                    tagged(0.0, 83),
                ]),
            ),
        ];
        let mut document = Document::new();
        let mut warnings = Vec::new();
        collect(&chunks, &mut document, &mut warnings).expect("collect");
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(document.shapes.len(), 1);
        let shape = &document.shapes[0];
        assert_eq!(shape.visio_id, 1);
        assert_eq!(shape.name, "Square");
        assert_eq!(shape.name_u, "Square");
        assert_eq!(shape.fill_style, Some(4));
        assert_eq!(shape.line_style, Some(5));
        assert_eq!(shape.text_style, Some(6));
        let cells = &shape.sheet.cells;
        assert_eq!(number(cells, "PinX"), Some(2.75));
        assert_eq!(unit_of(cells, "PinX"), Some(Unit::Inches));
        assert_eq!(number(cells, "PinY"), Some(6.07));
        assert_eq!(number(cells, "Width"), Some(1.5));
        assert_eq!(unit_of(cells, "Width"), Some(Unit::Millimeters));
        assert_eq!(number(cells, "LocPinX"), Some(0.75));
        assert_eq!(unit_of(cells, "LocPinX"), Some(Unit::Unitsless));
        assert_eq!(number(cells, "Angle"), Some(2.5));
        assert_eq!(unit_of(cells, "Angle"), Some(Unit::Degrees));
        assert_eq!(cell_str(cells, "FlipX"), Some(&CellValue::Bool(true)));
        assert_eq!(cell_str(cells, "FlipY"), Some(&CellValue::Bool(false)));
        assert_eq!(number(cells, "BeginX"), Some(1.0));
        assert_eq!(number(cells, "EndY"), Some(4.0));
        assert_eq!(number(cells, "TxtPinX"), Some(1.0));
        assert_eq!(number(cells, "TxtAngle"), Some(0.0));
        assert_eq!(unit_of(cells, "TxtAngle"), Some(Unit::Radians));
    }

    #[test]
    fn a_shape_without_an_id_warns_and_gets_zero() {
        let chunks = vec![chunk(
            0x48,
            1,
            u32::MAX,
            shape_record(u32::MAX, u32::MAX, u32::MAX, u32::MAX, u32::MAX),
        )];
        let mut document = Document::new();
        let mut warnings = Vec::new();
        collect(&chunks, &mut document, &mut warnings).expect("collect");
        assert_eq!(document.shapes[0].visio_id, 0);
        assert_eq!(
            kinds(&warnings),
            [WarningKind::BadValue { what: "shape id" }]
        );
    }

    #[test]
    fn a_shape_falls_back_to_the_record_id() {
        let chunks = vec![chunk(
            0x48,
            1,
            42,
            shape_record(u32::MAX, u32::MAX, u32::MAX, u32::MAX, u32::MAX),
        )];
        let mut document = Document::new();
        let mut warnings = Vec::new();
        collect(&chunks, &mut document, &mut warnings).expect("collect");
        assert_eq!(document.shapes[0].visio_id, 42);
        assert!(warnings.is_empty());
    }

    #[test]
    fn a_truncated_shape_id_record_warns() {
        let chunks = vec![
            chunk(0x65, 2, 0, vec![]),
            chunk(0x83, 3, 0, vec![1, 2]),
            chunk(
                0x48,
                1,
                9,
                shape_record(u32::MAX, u32::MAX, u32::MAX, u32::MAX, u32::MAX),
            ),
        ];
        let mut document = Document::new();
        let mut warnings = Vec::new();
        collect(&chunks, &mut document, &mut warnings).expect("collect");
        assert_eq!(document.shapes[0].visio_id, 9);
        assert_eq!(
            kinds(&warnings),
            [WarningKind::BadValue { what: "shape id" }]
        );
    }

    #[test]
    fn nesting_levels_build_the_parent_and_child_links() {
        let record = shape_record(u32::MAX, u32::MAX, u32::MAX, u32::MAX, u32::MAX);
        let chunks = vec![
            chunk(0x47, 1, 1, record.clone()),
            chunk(0x48, 2, 2, record.clone()),
            chunk(0x4d, 3, 3, record.clone()),
            chunk(0x48, 2, 4, record),
        ];
        let mut document = Document::new();
        let mut warnings = Vec::new();
        collect(&chunks, &mut document, &mut warnings).expect("collect");
        assert_eq!(document.shapes.len(), 4);
        let ids: Vec<ShapeId> = (0..4).map(|index| ShapeId { index }).collect();
        assert_eq!(document.shapes[0].parent, None);
        assert_eq!(document.shapes[0].children, [ids[1], ids[3]]);
        assert_eq!(document.shapes[1].parent, Some(ids[0]));
        assert_eq!(document.shapes[1].children, [ids[2]]);
        assert_eq!(document.shapes[2].parent, Some(ids[1]));
        assert_eq!(document.shapes[3].parent, Some(ids[0]));
        // Arena order is stream order.
        assert_eq!(document.shapes[2].id, ids[2]);
    }

    #[test]
    fn all_four_shape_records_make_shapes() {
        let record = shape_record(u32::MAX, u32::MAX, u32::MAX, u32::MAX, u32::MAX);
        let chunks = vec![
            chunk(0x47, 1, 1, record.clone()),
            chunk(0x48, 1, 2, record.clone()),
            chunk(0x4d, 1, 3, record.clone()),
            chunk(0x4e, 1, 4, record),
        ];
        let mut document = Document::new();
        let mut warnings = Vec::new();
        collect(&chunks, &mut document, &mut warnings).expect("collect");
        assert_eq!(document.shapes.len(), 4);
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn a_truncated_shape_record_warns_and_keeps_no_references() {
        let chunks = vec![chunk(0x48, 1, 1, vec![0; 8])];
        let mut document = Document::new();
        let mut warnings = Vec::new();
        collect(&chunks, &mut document, &mut warnings).expect("collect");
        let shape = &document.shapes[0];
        assert_eq!(shape.master, None);
        assert_eq!(shape.fill_style, None);
        assert_eq!(
            kinds(&warnings),
            [WarningKind::BadValue {
                what: "shape record"
            }]
        );
    }

    #[test]
    fn a_master_reference_resolves_by_visio_id() {
        let chunks = vec![chunk(
            0x48,
            1,
            1,
            shape_record(7, 5, u32::MAX, u32::MAX, u32::MAX),
        )];
        let mut document = document_with_master();
        let mut warnings = Vec::new();
        collect(&chunks, &mut document, &mut warnings).expect("collect");
        assert_eq!(document.shapes[0].master, Some(MasterId { index: 0 }));
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn an_unresolved_master_warns() {
        let chunks = vec![chunk(
            0x48,
            1,
            1,
            shape_record(8, 5, u32::MAX, u32::MAX, u32::MAX),
        )];
        let mut document = document_with_master();
        let mut warnings = Vec::new();
        collect(&chunks, &mut document, &mut warnings).expect("collect");
        assert_eq!(document.shapes[0].master, None);
        assert_eq!(
            kinds(&warnings),
            [WarningKind::UnresolvedReference { what: "master" }]
        );
    }

    #[test]
    fn names_come_from_the_nearest_name_index() {
        let record = shape_record(u32::MAX, u32::MAX, u32::MAX, u32::MAX, u32::MAX);
        let chunks = vec![
            chunk(0x33, 0, 1, join(&[word(1), name16("Page-1")])),
            chunk(0x33, 0, 2, join(&[word(1), name16("Square")])),
            chunk(0xc9, 1, 0, name_index(&[(2, 1)])),
            chunk(0x48, 1, 1, record.clone()),
            chunk(0xc9, 1, 0, name_index(&[(1, 1)])),
            chunk(0x48, 1, 1, record),
        ];
        let mut document = Document::new();
        let mut warnings = Vec::new();
        collect(&chunks, &mut document, &mut warnings).expect("collect");
        assert_eq!(document.shapes[0].name, "Square");
        assert_eq!(document.shapes[1].name, "Page-1");
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn an_index_inside_a_shape_does_not_name_the_shape() {
        let chunks = vec![
            chunk(0x33, 0, 1, join(&[word(1), name16("visVersion")])),
            chunk(
                0x48,
                1,
                1,
                shape_record(u32::MAX, u32::MAX, u32::MAX, u32::MAX, u32::MAX),
            ),
            chunk(0xc9, 2, 0, name_index(&[(1, 1)])),
        ];
        let mut document = Document::new();
        let mut warnings = Vec::new();
        collect(&chunks, &mut document, &mut warnings).expect("collect");
        assert_eq!(document.shapes[0].name, "");
    }

    #[test]
    fn a_name_entry_that_resolves_to_nothing_warns() {
        let chunks = vec![
            chunk(0xc9, 2, 0, name_index(&[(99, 1)])),
            chunk(
                0x48,
                1,
                1,
                shape_record(u32::MAX, u32::MAX, u32::MAX, u32::MAX, u32::MAX),
            ),
        ];
        let mut document = Document::new();
        let mut warnings = Vec::new();
        collect(&chunks, &mut document, &mut warnings).expect("collect");
        assert_eq!(document.shapes[0].name, "");
        assert_eq!(
            kinds(&warnings),
            [WarningKind::UnresolvedReference { what: "shape name" }]
        );
    }

    #[test]
    fn name_payloads_differ_between_the_versions() {
        let utf16 = vec![chunk(0x33, 0, 1, join(&[word(1), name16("Square")]))];
        let ansi = vec![chunk(0x33, 0, 1, join(&[word(1), name8("Square")]))];
        assert_eq!(
            name_table(&utf16, FormatVersion::V11).get(&1),
            Some(&"Square".to_owned())
        );
        assert_ne!(
            name_table(&ansi, FormatVersion::V11).get(&1),
            Some(&"Square".to_owned())
        );
        assert_eq!(
            name_table(&ansi, FormatVersion::V6).get(&1),
            Some(&"Square".to_owned())
        );
        // A `NAME` payload is the text itself, without the dummy dword.
        let name_v11 = vec![chunk(0x2d, 0, 2, name16("esc(0)"))];
        let name_v6 = vec![chunk(0x2d, 0, 2, name8("esc(0)"))];
        assert_eq!(
            name_table(&name_v11, FormatVersion::V11).get(&2),
            Some(&"esc(0)".to_owned())
        );
        assert_eq!(
            name_table(&name_v6, FormatVersion::V6).get(&2),
            Some(&"esc(0)".to_owned())
        );
    }

    #[test]
    fn an_odd_length_utf16_name_stops_at_the_last_whole_character() {
        assert_eq!(decode_utf16(b"A\x00B\x00C"), "AB");
    }

    #[test]
    fn an_unpaired_surrogate_reads_as_the_replacement_character() {
        assert_eq!(decode_utf16(&[0x00, 0xd8, 0x41, 0x00]), "\u{fffd}A");
    }

    #[test]
    fn the_version_six_names_stop_at_the_first_nul() {
        assert_eq!(decode_name(b"AB\0C", FormatVersion::V6, false), "AB");
    }

    // -- geometry ------------------------------------------------------

    /// One shape with one geometry section built from `rows`.
    fn geometry_document(rows: &[Chunk<'static>], list: Vec<u8>) -> (Document, Vec<ImportWarning>) {
        let mut chunks = vec![chunk(
            0x48,
            1,
            1,
            shape_record(u32::MAX, u32::MAX, u32::MAX, u32::MAX, u32::MAX),
        )];
        chunks.push(chunk(0x6c, 2, 0, list));
        chunks.extend(rows.to_vec());
        let mut document = Document::new();
        let mut warnings = Vec::new();
        collect(&chunks, &mut document, &mut warnings).expect("collect");
        (document, warnings)
    }

    /// The first `Geometry` section of the first shape.
    fn section(document: &Document) -> &Section {
        &document.shapes[0].sheet.sections[0]
    }

    #[test]
    fn the_geometry_rows_map_to_cells() {
        let rows = vec![
            chunk(0x89, 3, 0, vec![0b011]),
            chunk(0x8a, 3, 1, join(&[tagged(0.0, 65), tagged(1.0, 65)])),
            chunk(0x8b, 3, 2, join(&[tagged(1.5, 65), tagged(1.0, 65)])),
            chunk(
                0x8c,
                3,
                3,
                join(&[tagged(2.0, 65), tagged(2.0, 65), tagged(0.5, 65)]),
            ),
            chunk(
                0x90,
                3,
                4,
                join(&[
                    tagged(3.0, 65),
                    tagged(3.0, 65),
                    tagged(2.5, 65),
                    tagged(2.5, 65),
                    tagged(2.5, 81),
                    tagged(0.8, 32),
                ]),
            ),
        ];
        let (document, warnings) = geometry_document(&rows, geom_list(&[0, 1, 2, 3, 4]));
        assert!(warnings.is_empty(), "{warnings:?}");
        let section = section(&document);
        assert_eq!(section.cells.len(), 3);
        assert_eq!(
            cell_str(&section.cells, "NoFill"),
            Some(&CellValue::Bool(true))
        );
        assert_eq!(
            cell_str(&section.cells, "NoLine"),
            Some(&CellValue::Bool(true))
        );
        assert_eq!(
            cell_str(&section.cells, "NoShow"),
            Some(&CellValue::Bool(false))
        );
        assert_eq!(section.rows.len(), 4);
        let first = &section.rows[0];
        assert_eq!(first.row_type.as_deref(), Some("MoveTo"));
        assert_eq!(first.ix, Some(1));
        assert_eq!(number(&first.cells, "X"), Some(0.0));
        assert_eq!(number(&first.cells, "Y"), Some(1.0));
        let arc = &section.rows[2];
        assert_eq!(arc.row_type.as_deref(), Some("ArcTo"));
        assert_eq!(number(&arc.cells, "A"), Some(0.5));
        let elliptical = &section.rows[3];
        assert_eq!(elliptical.row_type.as_deref(), Some("EllipticalArcTo"));
        assert_eq!(number(&elliptical.cells, "B"), Some(2.5));
        assert_eq!(number(&elliptical.cells, "C"), Some(2.5));
        assert_eq!(number(&elliptical.cells, "D"), Some(0.8));
    }

    #[test]
    fn the_spline_ellipse_and_infinite_line_rows_map_to_cells() {
        let rows = vec![
            chunk(
                0xa5,
                3,
                1,
                join(&[
                    tagged(0.0, 65),
                    tagged(0.0, 65),
                    double(0.25),
                    double(0.5),
                    double(1.0),
                    vec![3],
                ]),
            ),
            chunk(
                0xa6,
                3,
                2,
                join(&[tagged(1.0, 65), tagged(1.0, 65), double(0.75)]),
            ),
            chunk(
                0x8f,
                3,
                3,
                join(&[
                    tagged(1.0, 65),
                    tagged(1.0, 65),
                    tagged(0.0, 65),
                    tagged(1.0, 65),
                    tagged(1.0, 65),
                    tagged(0.0, 65),
                ]),
            ),
            chunk(
                0x8d,
                3,
                4,
                join(&[
                    tagged(0.0, 65),
                    tagged(0.0, 65),
                    tagged(1.0, 65),
                    tagged(1.0, 65),
                ]),
            ),
        ];
        let (document, warnings) = geometry_document(&rows, geom_list(&[1, 2, 3, 4]));
        assert!(warnings.is_empty(), "{warnings:?}");
        let rows = &section(&document).rows;
        let start = &rows[0];
        assert_eq!(start.row_type.as_deref(), Some("SplineStart"));
        assert_eq!(number(&start.cells, "A"), Some(0.25));
        assert_eq!(number(&start.cells, "B"), Some(0.5));
        assert_eq!(number(&start.cells, "C"), Some(1.0));
        assert_eq!(number(&start.cells, "D"), Some(3.0));
        assert_eq!(unit_of(&start.cells, "D"), Some(Unit::Unitsless));
        let knot = &rows[1];
        assert_eq!(knot.row_type.as_deref(), Some("SplineKnot"));
        assert_eq!(number(&knot.cells, "A"), Some(0.75));
        let ellipse = &rows[2];
        assert_eq!(ellipse.row_type.as_deref(), Some("Ellipse"));
        assert_eq!(number(&ellipse.cells, "D"), Some(0.0));
        let infinite = &rows[3];
        assert_eq!(infinite.row_type.as_deref(), Some("InfiniteLine"));
        assert_eq!(number(&infinite.cells, "B"), Some(1.0));
    }

    #[test]
    fn the_geometry_list_sets_the_row_order() {
        let rows = vec![
            chunk(0x8a, 3, 1, join(&[tagged(1.0, 65), tagged(0.0, 65)])),
            chunk(0x8b, 3, 2, join(&[tagged(2.0, 65), tagged(0.0, 65)])),
            chunk(0x8b, 3, 3, join(&[tagged(3.0, 65), tagged(0.0, 65)])),
        ];
        let (document, warnings) = geometry_document(&rows, geom_list(&[3, 1, 2]));
        assert!(warnings.is_empty(), "{warnings:?}");
        let rows = &section(&document).rows;
        let xs: Vec<Option<f64>> = rows.iter().map(|row| number(&row.cells, "X")).collect();
        assert_eq!(xs, [Some(3.0), Some(1.0), Some(2.0)]);
        assert_eq!(rows[0].ix, Some(1));
        assert_eq!(rows[2].ix, Some(3));
    }

    #[test]
    fn an_instance_without_geometry_draws_its_master_shapes_geometry() {
        // P8-15: a `.vsd` instance keeps only the geometry that
        // differs from its master, and its record names the master's
        // shape it draws (`no-bgcolor.vsd`'s icon is such a sub-shape).
        let mut document = document_with_master();
        document.masters[0].shapes.push(ShapeId { index: 0 });

        // The master's shape (one `MoveTo` row) and the instance that
        // names it, both mapped by the walk like the pipeline does.
        let mut row = join(&[tagged(1.0, 32), tagged(0.0, 32)]);
        row.insert(0, 0x80);
        let chunks = vec![
            chunk(
                0x48,
                1,
                11,
                shape_record(u32::MAX, u32::MAX, u32::MAX, u32::MAX, u32::MAX),
            ),
            chunk(0x6c, 2, 0, geom_list(&[1])),
            chunk(0x89, 3, 0, vec![0]),
            chunk(0x8a, 3, 1, join(&[tagged(1.0, 32), tagged(0.0, 32)])),
            chunk(
                0x48,
                1,
                5,
                shape_record(7, 11, u32::MAX, u32::MAX, u32::MAX),
            ),
        ];
        let mut warnings = Vec::new();
        collect_with(
            &chunks,
            FormatVersion::V11,
            &Limits::DEFAULT,
            &mut document,
            &mut warnings,
        )
        .expect("collect");
        assert!(warnings.is_empty(), "{warnings:?}");
        let instance = &document.shapes[1];
        assert_eq!(instance.visio_id, 5);
        let rows: usize = instance
            .sheet
            .sections
            .iter()
            .filter(|section| section.name.as_str() == "Geometry")
            .map(|section| section.rows.len())
            .sum();
        assert_eq!(rows, 1, "the master's geometry is the instance's");
    }

    #[test]
    fn a_row_missing_from_the_order_is_dropped() {
        // P8-15: the list's trailer selects the rows (the reference
        // keeps only the rows it names); a row no list claims is not
        // part of the geometry. Before this rule, six `InfiniteLine`
        // rows of `Visio6PlanWithDimensions.vsd` drew as lines across
        // the page.
        let rows = vec![
            chunk(0x8b, 3, 1, join(&[tagged(1.0, 65), tagged(0.0, 65)])),
            chunk(0x8b, 3, 2, join(&[tagged(2.0, 65), tagged(0.0, 65)])),
        ];
        let (document, warnings) = geometry_document(&rows, geom_list(&[2]));
        assert!(warnings.is_empty(), "{warnings:?}");
        let rows = &section(&document).rows;
        let xs: Vec<Option<f64>> = rows.iter().map(|row| number(&row.cells, "X")).collect();
        assert_eq!(xs, [Some(2.0)]);
    }

    #[test]
    fn a_lying_row_order_warns_and_keeps_stream_order() {
        let rows = vec![chunk(0x8b, 3, 1, join(&[tagged(1.0, 65), tagged(0.0, 65)]))];
        let list = join(&[word(24), word(u32::MAX), vec![0; 24]]);
        let (document, warnings) = geometry_document(&rows, list);
        assert_eq!(
            kinds(&warnings),
            [WarningKind::BadValue {
                what: "geometry row order"
            }]
        );
        assert_eq!(section(&document).rows.len(), 1);
    }

    #[test]
    fn rows_outside_a_geometry_list_still_map() {
        let mut chunks = vec![chunk(
            0x48,
            1,
            1,
            shape_record(u32::MAX, u32::MAX, u32::MAX, u32::MAX, u32::MAX),
        )];
        chunks.push(chunk(0x8b, 2, 1, join(&[tagged(1.0, 65), tagged(0.0, 65)])));
        let mut document = Document::new();
        let mut warnings = Vec::new();
        collect(&chunks, &mut document, &mut warnings).expect("collect");
        let section = &document.shapes[0].sheet.sections[0];
        assert_eq!(section.name.as_str(), "Geometry");
        assert_eq!(section.rows.len(), 1);
    }

    #[test]
    fn a_shape_without_geometry_gets_no_section() {
        let chunks = vec![chunk(
            0x48,
            1,
            1,
            shape_record(u32::MAX, u32::MAX, u32::MAX, u32::MAX, u32::MAX),
        )];
        let mut document = Document::new();
        let mut warnings = Vec::new();
        collect(&chunks, &mut document, &mut warnings).expect("collect");
        assert!(document.shapes[0].sheet.sections.is_empty());
    }

    #[test]
    fn an_empty_geometry_flag_record_sets_no_cells() {
        let rows = vec![
            chunk(0x89, 3, 0, vec![]),
            chunk(0x8a, 3, 1, join(&[tagged(0.0, 65), tagged(1.0, 65)])),
        ];
        let (document, _) = geometry_document(&rows, geom_list(&[0, 1]));
        let section = &document.shapes[0].sheet.sections[0];
        assert!(section.cells.is_empty());
        assert_eq!(section.rows.len(), 1);
    }

    // -- NURBS and polyline --------------------------------------------

    /// The `E` cell text of the first row.
    fn e_text(document: &Document) -> Option<String> {
        let rows = &section(document).rows;
        match cell_str(&rows[0].cells, "E") {
            Some(CellValue::Str(text)) => Some(text.clone()),
            _ => None,
        }
    }

    #[test]
    fn a_nurbs_row_reads_the_curve_data_it_references() {
        let curve = chunk(
            0xd1,
            0,
            9,
            nurbs_data(1.0, 3, 0, 1, &[(0.25, 0.5, 0.0, 1.0)]),
        );
        let mut chunks = document_shapes_as_chunks();
        chunks.push(curve);
        let mut document = Document::new();
        let mut warnings = Vec::new();
        collect(&chunks, &mut document, &mut warnings).expect("collect");
        assert!(warnings.is_empty(), "{warnings:?}");
        let row = &section(&document).rows[0];
        assert_eq!(row.row_type.as_deref(), Some("NurbsTo"));
        assert_eq!(number(&row.cells, "A"), Some(0.5));
        assert_eq!(number(&row.cells, "B"), Some(1.0));
        assert_eq!(number(&row.cells, "C"), Some(0.0));
        assert_eq!(number(&row.cells, "D"), Some(1.0));
        assert_eq!(
            e_text(&document).as_deref(),
            Some("NURBS(1, 3, 0, 1, 0.25, 0.5, 0, 1)")
        );
    }

    /// The chunks of the one-shape fixture, without the curve record.
    fn document_shapes_as_chunks() -> Vec<Chunk<'static>> {
        let rows = vec![chunk(
            0xc3,
            3,
            1,
            join(&[
                tagged(1.0, 65),
                tagged(2.0, 65),
                double(0.5),
                double(1.0),
                double(0.0),
                double(1.0),
                vec![0],
                vec![0x8a],
                vec![0, 0, 0],
                word(9),
            ]),
        )];
        let mut chunks = vec![chunk(
            0x48,
            1,
            1,
            shape_record(u32::MAX, u32::MAX, u32::MAX, u32::MAX, u32::MAX),
        )];
        chunks.push(chunk(0x6c, 2, 0, geom_list(&[1])));
        chunks.extend(rows);
        chunks
    }

    #[test]
    fn a_nurbs_row_without_its_curve_data_keeps_its_cells() {
        let chunks = document_shapes_as_chunks();
        let mut document = Document::new();
        let mut warnings = Vec::new();
        collect(&chunks, &mut document, &mut warnings).expect("collect");
        assert!(warnings.is_empty(), "{warnings:?}");
        let row = &section(&document).rows[0];
        assert_eq!(cell_str(&row.cells, "E"), None);
    }

    #[test]
    fn a_nurbs_row_with_an_inline_static_block() {
        let block = join(&[
            word(0),     // block length (never used once the cell is found)
            vec![0],     // separator
            vec![6],     // the E cell
            vec![0x8a],  // static format
            double(1.0), // last knot
            3u16.to_le_bytes().to_vec(),
            vec![0, 1], // xType, yType
            word(1),    // repetitions
            double(0.25),
            double(0.5),
            double(0.0),
            double(2.0),
        ]);
        let row = chunk(
            0xc3,
            3,
            1,
            join(&[
                tagged(1.0, 65),
                tagged(2.0, 65),
                double(0.5),
                double(1.0),
                double(0.0),
                double(1.0),
                vec![0],
                vec![0],
                vec![0; 9],
                block,
            ]),
        );
        let (document, warnings) = geometry_document(&[row], geom_list(&[1]));
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(
            e_text(&document).as_deref(),
            Some("NURBS(1, 3, 0, 1, 0.25, 0.5, 0, 2)")
        );
    }

    #[test]
    fn a_nurbs_row_with_an_inline_typed_block() {
        let block = join(&[
            word(0),
            vec![0],
            vec![5], // a skipped cell first
            word(0),
            vec![0],
            vec![6],    // the E cell
            vec![0x20], // typed format: the last knot is a double
            double(1.0),
            vec![0],
            3u16.to_le_bytes().to_vec(),
            vec![0],
            1u16.to_le_bytes().to_vec(),
            vec![0],
            0u16.to_le_bytes().to_vec(),
            vec![0x20], // x as a double
            double(0.25),
            vec![0x62], // y as a word
            5u16.to_le_bytes().to_vec(),
            vec![0x62],
            6u16.to_le_bytes().to_vec(),
            vec![0x62],
            7u16.to_le_bytes().to_vec(),
            vec![0x81],
        ]);
        let row = chunk(
            0xc3,
            3,
            1,
            join(&[
                tagged(1.0, 65),
                tagged(2.0, 65),
                double(0.5),
                double(1.0),
                double(0.0),
                double(1.0),
                vec![0],
                vec![0],
                vec![0; 9],
                block,
            ]),
        );
        let (document, warnings) = geometry_document(&[row], geom_list(&[1]));
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(
            e_text(&document).as_deref(),
            Some("NURBS(1, 3, 1, 0, 0.25, 5, 6, 7)")
        );
    }

    #[test]
    fn a_polyline_row_reads_the_curve_data_it_references() {
        let row = chunk(
            0xc1,
            3,
            1,
            join(&[
                tagged(1.0, 65),
                tagged(2.0, 65),
                vec![0],
                vec![0x8b],
                vec![0, 0, 0],
                word(9),
            ]),
        );
        let curve = chunk(0xd1, 0, 9, polyline_data(0, 1, &[(0.25, 0.5), (0.75, 1.0)]));
        let mut chunks = vec![
            chunk(
                0x48,
                1,
                1,
                shape_record(u32::MAX, u32::MAX, u32::MAX, u32::MAX, u32::MAX),
            ),
            chunk(0x6c, 2, 0, geom_list(&[1])),
            row,
            curve,
        ];
        chunks.reverse();
        chunks.reverse();
        let mut document = Document::new();
        let mut warnings = Vec::new();
        collect(&chunks, &mut document, &mut warnings).expect("collect");
        assert!(warnings.is_empty(), "{warnings:?}");
        let row = &section(&document).rows[0];
        assert_eq!(row.row_type.as_deref(), Some("PolylineTo"));
        assert_eq!(
            cell_str(&row.cells, "A"),
            Some(&CellValue::Str(
                "POLYLINE(0, 1, 0.25, 0.5, 0.75, 1)".to_owned()
            ))
        );
    }

    #[test]
    fn a_polyline_row_with_an_inline_block() {
        let block = join(&[
            word(24),
            vec![0],
            vec![2], // the A cell
            vec![0],
            0u16.to_le_bytes().to_vec(),
            vec![0],
            1u16.to_le_bytes().to_vec(),
            vec![0x20],
            double(0.25),
            vec![0x62],
            5u16.to_le_bytes().to_vec(),
            vec![0x81],
        ]);
        let row = chunk(
            0xc1,
            3,
            1,
            join(&[
                tagged(1.0, 65),
                tagged(2.0, 65),
                vec![0],
                vec![0],
                vec![0; 9],
                block,
            ]),
        );
        let (document, warnings) = geometry_document(&[row], geom_list(&[1]));
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(
            cell_str(&section(&document).rows[0].cells, "A"),
            Some(&CellValue::Str("POLYLINE(0, 1, 0.25, 5)".to_owned()))
        );
    }

    #[test]
    fn a_polyline_row_without_data_keeps_only_its_end_point() {
        let row = chunk(
            0xc1,
            3,
            1,
            join(&[
                tagged(1.0, 65),
                tagged(2.0, 65),
                vec![0],
                vec![0],
                vec![0; 9],
            ]),
        );
        let (document, warnings) = geometry_document(&[row], geom_list(&[1]));
        assert!(warnings.is_empty(), "{warnings:?}");
        let row = &section(&document).rows[0];
        assert_eq!(row.row_type.as_deref(), Some("PolylineTo"));
        assert_eq!(number(&row.cells, "X"), Some(1.0));
        assert_eq!(cell_str(&row.cells, "A"), None);
    }

    #[test]
    fn a_curve_of_the_other_kind_encodes_nothing() {
        let row = chunk(
            0xc3,
            3,
            1,
            join(&[
                tagged(1.0, 65),
                tagged(2.0, 65),
                double(0.5),
                double(1.0),
                double(0.0),
                double(1.0),
                vec![0],
                vec![0x8a],
                vec![0, 0, 0],
                word(9),
            ]),
        );
        let curve = chunk(0xd1, 0, 9, polyline_data(0, 0, &[(0.25, 0.5)]));
        let chunks = vec![
            chunk(
                0x48,
                1,
                1,
                shape_record(u32::MAX, u32::MAX, u32::MAX, u32::MAX, u32::MAX),
            ),
            chunk(0x6c, 2, 0, geom_list(&[1])),
            row,
            curve,
        ];
        let mut document = Document::new();
        let mut warnings = Vec::new();
        collect(&chunks, &mut document, &mut warnings).expect("collect");
        assert_eq!(e_text(&document), None);
    }

    #[test]
    fn curve_data_of_an_unknown_kind_is_ignored() {
        let mut bytes = vec![0x99];
        bytes.extend(vec![0; 15]);
        let curves = curve_table(&[chunk(0xd1, 0, 1, bytes)]);
        assert!(curves.is_empty());
    }

    #[test]
    fn a_lie_about_the_point_count_is_capped_by_the_payload() {
        let mut bytes = vec![0x80];
        bytes.extend(vec![0; 15]);
        bytes.push(0);
        bytes.push(1);
        bytes.extend(word(u32::MAX));
        bytes.extend(double(0.25));
        bytes.extend(double(0.5));
        let curves = curve_table(&[chunk(0xd1, 0, 1, bytes)]);
        assert_eq!(
            curves.get(&1),
            Some(&CurveData::Polyline {
                x_type: 0,
                y_type: 1,
                points: vec![(0.25, 0.5)]
            })
        );
    }

    #[test]
    fn curve_data_without_a_point_count_is_empty() {
        let mut bytes = vec![0x82];
        bytes.extend(vec![0; 15]);
        bytes.extend(double(1.0));
        bytes.extend(3u16.to_le_bytes());
        bytes.push(0);
        bytes.push(1);
        let curves = curve_table(&[chunk(0xd1, 0, 1, bytes)]);
        assert_eq!(
            curves.get(&1),
            Some(&CurveData::Nurbs {
                knot_last: 1.0,
                degree: 3,
                x_type: 0,
                y_type: 1,
                points: Vec::new(),
            })
        );
    }

    // -- degrading -----------------------------------------------------

    #[test]
    fn a_bad_unit_and_a_bad_value_leave_the_cell_unset() {
        let chunks = vec![chunk(
            0x48,
            1,
            1,
            shape_record(u32::MAX, u32::MAX, u32::MAX, u32::MAX, u32::MAX),
        )];
        let mut chunks = chunks;
        chunks.push(chunk(
            0x9b,
            2,
            0,
            xform(
                &[
                    (1.0, 255),
                    (f64::NAN, 65),
                    (1.5, 65),
                    (1.5, 65),
                    (0.75, 65),
                    (0.75, 65),
                    (0.0, 65),
                ],
                0,
                0,
            ),
        ));
        let mut document = Document::new();
        let mut warnings = Vec::new();
        collect(&chunks, &mut document, &mut warnings).expect("collect");
        let cells = &document.shapes[0].sheet.cells;
        assert_eq!(cell_str(cells, "PinX"), None);
        assert_eq!(cell_str(cells, "PinY"), None);
        assert_eq!(number(cells, "Width"), Some(1.5));
        assert_eq!(
            kinds(&warnings),
            [
                WarningKind::BadValue { what: "unit" },
                WarningKind::BadValue {
                    what: "non-finite value"
                }
            ]
        );
    }

    #[test]
    fn a_truncated_transform_keeps_the_cells_that_did_parse() {
        let mut chunks = vec![chunk(
            0x48,
            1,
            1,
            shape_record(u32::MAX, u32::MAX, u32::MAX, u32::MAX, u32::MAX),
        )];
        chunks.push(chunk(0x9b, 2, 0, join(&[tagged(1.0, 65), tagged(2.0, 65)])));
        let mut document = Document::new();
        let mut warnings = Vec::new();
        collect(&chunks, &mut document, &mut warnings).expect("collect");
        let cells = &document.shapes[0].sheet.cells;
        assert_eq!(number(cells, "PinX"), Some(1.0));
        assert_eq!(cell_str(cells, "Width"), None);
        assert_eq!(
            kinds(&warnings),
            [WarningKind::BadValue {
                what: "transform value"
            }]
        );
    }

    #[test]
    fn a_transform_without_its_flips_warns() {
        let mut chunks = vec![chunk(
            0x48,
            1,
            1,
            shape_record(u32::MAX, u32::MAX, u32::MAX, u32::MAX, u32::MAX),
        )];
        let mut values = Vec::new();
        for value in [1.0, 1.0, 1.0, 1.0, 0.5, 0.5, 0.0] {
            values.push(tagged(value, 65));
        }
        chunks.push(chunk(0x9b, 2, 0, join(&values)));
        let mut document = Document::new();
        let mut warnings = Vec::new();
        collect(&chunks, &mut document, &mut warnings).expect("collect");
        assert_eq!(
            kinds(&warnings),
            [WarningKind::BadValue {
                what: "transform value"
            }]
        );
    }

    #[test]
    fn a_truncated_geometry_row_keeps_the_cells_that_did_parse() {
        let rows = vec![chunk(0x8b, 3, 1, join(&[tagged(1.0, 65)]))];
        let (document, warnings) = geometry_document(&rows, geom_list(&[1]));
        assert_eq!(
            kinds(&warnings),
            [WarningKind::BadValue {
                what: "geometry value"
            }]
        );
        let row = &section(&document).rows[0];
        assert_eq!(number(&row.cells, "X"), Some(1.0));
        assert_eq!(cell_str(&row.cells, "Y"), None);
    }

    #[test]
    fn a_spline_start_without_its_degree_warns() {
        let rows = vec![chunk(
            0xa5,
            3,
            1,
            join(&[
                tagged(0.0, 65),
                tagged(0.0, 65),
                double(0.25),
                double(0.5),
                double(1.0),
            ]),
        )];
        let (document, warnings) = geometry_document(&rows, geom_list(&[1]));
        assert_eq!(
            kinds(&warnings),
            [WarningKind::BadValue {
                what: "geometry value"
            }]
        );
        assert_eq!(section(&document).rows.len(), 1);
    }

    #[test]
    fn a_truncated_name_index_is_read_up_to_the_cut() {
        let entries = name_index(&[(1, 1), (2, 2)]);
        let mut walker = Walker::new(&Limits::DEFAULT);
        walker.read_name_index(&[], 1);
        assert!(walker.name_index.is_empty());
        for cut in 0..entries.len() {
            let mut walker = Walker::new(&Limits::DEFAULT);
            walker.read_name_index(entries.get(..cut).unwrap_or(&[]), 1);
        }
        let mut walker = Walker::new(&Limits::DEFAULT);
        walker.read_name_index(&entries, 1);
        assert_eq!(walker.name_index, [vec![(1, 1), (2, 2)]]);
    }

    #[test]
    fn a_name_index_123_reads_pairs_until_the_payload_ends() {
        let mut walker = Walker::new(&Limits::DEFAULT);
        walker.read_name_index_123(&join(&[word(1), word(2), word(3), word(4), word(5)]), 1);
        assert_eq!(walker.name_index, [vec![(2, 1), (4, 3)]]);
        let mut walker = Walker::new(&Limits::DEFAULT);
        walker.read_name_index_123(&join(&[word(1), word(2), word(3)]), 1);
        assert_eq!(walker.name_index, [vec![(2, 1)]]);
    }

    #[test]
    fn the_dropped_records_warn_only_inside_a_shape() {
        let record = shape_record(u32::MAX, u32::MAX, u32::MAX, u32::MAX, u32::MAX);
        let mut chunks = vec![chunk(0x0c, 2, 0, vec![1, 2, 3])];
        chunks.push(chunk(0x48, 1, 1, record));
        chunks.push(chunk(0x0c, 2, 0, vec![1, 2, 3]));
        chunks.push(chunk(0x1f, 2, 0, vec![]));
        chunks.push(chunk(0xc4, 2, 0, vec![]));
        chunks.push(chunk(0x84, 2, 0, vec![]));
        chunks.push(chunk(0x18, 2, 0, vec![]));
        chunks.push(chunk(0x2b, 2, 0, vec![]));
        let mut document = Document::new();
        let mut warnings = Vec::new();
        collect(&chunks, &mut document, &mut warnings).expect("collect");
        let dropped: Vec<&str> = warnings
            .iter()
            .filter_map(|warning| match warning.kind {
                WarningKind::Dropped { what } => Some(what),
                _ => None,
            })
            .collect();
        assert_eq!(
            dropped,
            [
                "foreign data",
                "OLE data",
                "hyperlink",
                "event",
                "font list"
            ]
        );
    }

    #[test]
    fn an_unknown_record_inside_a_shape_warns() {
        let mut chunks = vec![chunk(
            0x48,
            1,
            1,
            shape_record(u32::MAX, u32::MAX, u32::MAX, u32::MAX, u32::MAX),
        )];
        chunks.push(chunk(0x2b, 2, 0, vec![]));
        // An empty line record: the shape maps it and reports the
        // unparseable payload (P8-15: shape-level rows are mapped).
        chunks.push(chunk(0x85, 2, 0, vec![]));
        let mut document = Document::new();
        let mut warnings = Vec::new();
        collect(&chunks, &mut document, &mut warnings).expect("collect");
        assert_eq!(
            kinds(&warnings),
            [
                WarningKind::SkippedRecord {
                    name: "unknown record"
                },
                WarningKind::BadValue {
                    what: "line record"
                }
            ]
        );
    }

    /// A line-record payload: width, colour, pattern, rounding and the
    /// arrow and cap bytes after the rounding format byte.
    fn line_row(width: f64, colour: [u8; 4], pattern: u8) -> Vec<u8> {
        let mut data = vec![0u8; 28];
        data[1..9].copy_from_slice(&width.to_le_bytes());
        data[10..14].copy_from_slice(&colour);
        data[14] = pattern;
        data[16..24].copy_from_slice(&0.25f64.to_le_bytes());
        data[25] = 2;
        data[26] = 3;
        data[27] = 1;
        data
    }

    /// A fill-and-shadow-record payload.
    fn fill_row(foreground: (u8, [u8; 4]), pattern: u8) -> Vec<u8> {
        let mut data = vec![0u8; 41];
        data[0] = foreground.0;
        data[1..5].copy_from_slice(&foreground.1);
        data[10] = pattern;
        data[21] = 3;
        data
    }

    /// A text-block-record payload: one margin is enough.
    fn text_block_row(margin: f64) -> Vec<u8> {
        let mut data = vec![0u8; 51];
        data[1..9].copy_from_slice(&margin.to_le_bytes());
        data
    }

    #[test]
    fn a_shape_maps_its_own_line_fill_and_text_block_rows() {
        // P8-15 goal 1: a shape record group carries its own `Line`,
        // `FillAndShadow` and `TextBlock` rows; dropping them left
        // every instance at the hard defaults (no line where the file
        // has one, no fill where there is one).
        let mut chunks = vec![chunk(
            0x48,
            1,
            4,
            shape_record(u32::MAX, u32::MAX, u32::MAX, u32::MAX, u32::MAX),
        )];
        chunks.push(chunk(0x85, 2, 0, line_row(0.25, [1, 2, 3, 0], 2)));
        chunks.push(chunk(0x86, 2, 0, fill_row((1, [255, 255, 255, 0]), 1)));
        chunks.push(chunk(0x87, 2, 0, text_block_row(0.05)));
        let mut document = Document::new();
        let mut warnings = Vec::new();
        collect(&chunks, &mut document, &mut warnings).expect("collect");
        assert!(warnings.is_empty(), "{warnings:?}");
        let cells = &document.shapes[0].sheet.cells;
        assert_eq!(number(cells, "LineWeight"), Some(0.25));
        assert_eq!(number(cells, "LinePattern"), Some(2.0));
        assert_eq!(number(cells, "BeginArrow"), Some(2.0));
        assert_eq!(number(cells, "EndArrow"), Some(3.0));
        assert_eq!(number(cells, "LineCap"), Some(1.0));
        assert_eq!(number(cells, "FillForegnd"), Some(1.0));
        assert_eq!(number(cells, "FillPattern"), Some(1.0));
        assert_eq!(number(cells, "ShdwPattern"), Some(3.0));
        assert_eq!(number(cells, "LeftMargin"), Some(0.05));
    }

    #[test]
    fn the_line_rows_of_the_styles_stream_are_not_shape_cells() {
        let mut chunks = vec![chunk(0x1a, 1, 0, vec![])];
        chunks.push(chunk(0x85, 3, 0, line_row(0.25, [1, 2, 3, 0], 2)));
        chunks.push(chunk(
            0x48,
            1,
            4,
            shape_record(u32::MAX, u32::MAX, u32::MAX, u32::MAX, u32::MAX),
        ));
        chunks.push(chunk(0x85, 2, 0, line_row(0.5, [1, 2, 3, 0], 2)));
        let mut document = Document::new();
        let mut warnings = Vec::new();
        collect(&chunks, &mut document, &mut warnings).expect("collect");
        let cells = &document.shapes[0].sheet.cells;
        assert_eq!(number(cells, "LineWeight"), Some(0.5));
    }

    #[test]
    fn the_arrow_bytes_follow_the_rounding_format_byte() {
        // P8-15: byte 24 is the rounding value's format byte. Reading
        // the markers there drew an arrowhead on every outline.
        let mut payload = line_row(0.25, [1, 2, 3, 0], 2);
        payload[24] = 7;
        let mut chunks = vec![chunk(
            0x48,
            1,
            4,
            shape_record(u32::MAX, u32::MAX, u32::MAX, u32::MAX, u32::MAX),
        )];
        chunks.push(chunk(0x85, 2, 0, payload));
        let mut document = Document::new();
        let mut warnings = Vec::new();
        collect(&chunks, &mut document, &mut warnings).expect("collect");
        let cells = &document.shapes[0].sheet.cells;
        assert_eq!(number(cells, "BeginArrow"), Some(2.0));
        assert_eq!(number(cells, "EndArrow"), Some(3.0));
        assert_eq!(number(cells, "LineCap"), Some(1.0));
    }

    /// A shape record payload naming `parent` as its parent shape.
    fn shape_record_with_parent(parent: u32) -> Vec<u8> {
        let mut data = shape_record(u32::MAX, u32::MAX, u32::MAX, u32::MAX, u32::MAX);
        data[10..14].copy_from_slice(&parent.to_le_bytes());
        data
    }

    #[test]
    fn a_parent_reference_to_no_shape_keeps_the_level_nesting() {
        let mut chunks = vec![chunk(0x48, 1, 1, shape_record_with_parent(0))];
        chunks.push(chunk(0x48, 2, 2, shape_record_with_parent(99)));
        let mut document = Document::new();
        let mut warnings = Vec::new();
        collect(&chunks, &mut document, &mut warnings).expect("collect");
        assert_eq!(document.shapes[1].parent, Some(ShapeId { index: 0 }));
    }

    #[test]
    fn a_shape_record_cut_after_the_parent_id_still_maps() {
        let mut data = shape_record_with_parent(0);
        data.truncate(12);
        let chunks = vec![chunk(0x48, 1, 1, data)];
        let mut document = Document::new();
        let mut warnings = Vec::new();
        collect(&chunks, &mut document, &mut warnings).expect("collect");
        assert_eq!(document.shapes.len(), 1);
        assert_eq!(
            kinds(&warnings),
            [WarningKind::BadValue {
                what: "shape record"
            }]
        );
    }

    #[test]
    fn a_guide_shape_record_marks_the_shape_as_a_guide() {
        // P8-15: `Visio6PlanWithDimensions.vsd`'s six dimension guides
        // are guide shape records (`0x4d`): Visio draws guides on
        // screen only, so the renderer skips them.
        let chunks = vec![chunk(0x4d, 1, 44, shape_record_with_parent(0))];
        let mut document = Document::new();
        let mut warnings = Vec::new();
        collect(&chunks, &mut document, &mut warnings).expect("collect");
        assert!(document.shapes[0].is_guide);

        let chunks = vec![chunk(0x48, 1, 45, shape_record_with_parent(0))];
        let mut document = Document::new();
        collect(&chunks, &mut document, &mut warnings).expect("collect");
        assert!(!document.shapes[0].is_guide);
    }

    #[test]
    fn a_shape_takes_its_visioid_from_the_record_id() {
        // The shape list names the group's children in z-order; the
        // shape ids are the record ids (P8-15: the ids the smoke tests
        // pin).
        let mut chunks = vec![chunk(0x65, 1, 0, vec![])];
        chunks.push(chunk(0x83, 2, 0, shape_id(30)));
        chunks.push(chunk(
            0x48,
            1,
            3,
            shape_record(u32::MAX, u32::MAX, u32::MAX, u32::MAX, u32::MAX),
        ));
        chunks.push(chunk(
            0x48,
            1,
            5,
            shape_record(u32::MAX, u32::MAX, u32::MAX, u32::MAX, u32::MAX),
        ));
        let mut document = Document::new();
        let mut warnings = Vec::new();
        collect(&chunks, &mut document, &mut warnings).expect("collect");
        assert_eq!(
            document
                .shapes
                .iter()
                .map(|s| s.visio_id)
                .collect::<Vec<_>>(),
            [3, 5]
        );
    }

    #[test]
    fn the_parent_field_nests_a_group_sub_shape() {
        // P8-15 goal 3: a group's sub-shapes are its siblings in the
        // stream; the parent field names the group.
        let mut chunks = vec![chunk(0x47, 1, 1, shape_record_with_parent(0))];
        chunks.push(chunk(0x48, 1, 2, shape_record_with_parent(1)));
        chunks.push(chunk(0x48, 1, 3, shape_record_with_parent(1)));
        let mut document = Document::new();
        let mut warnings = Vec::new();
        collect(&chunks, &mut document, &mut warnings).expect("collect");
        let group = &document.shapes[0];
        assert_eq!(group.visio_id, 1);
        assert_eq!(group.parent, None);
        assert_eq!(
            group.children,
            vec![ShapeId { index: 1 }, ShapeId { index: 2 }]
        );
        assert_eq!(document.shapes[1].parent, Some(ShapeId { index: 0 }));
        assert_eq!(document.shapes[2].parent, Some(ShapeId { index: 0 }));
    }

    #[test]
    fn a_shape_list_orders_the_shapes_it_names() {
        // P8-15: the paint order comes from the shape list, not from
        // the stream order (a filled shape written before the lines
        // drawn over it must stay behind them).
        let mut document = Document::new();
        for visio_id in [1, 2, 3] {
            document.add_shape(Shape::new(ShapeId { index: 0 }, visio_id));
        }
        let mut page = larrygram_model::Page::new(larrygram_model::PageId { index: 0 }, 0);
        page.shapes = vec![
            ShapeId { index: 0 },
            ShapeId { index: 1 },
            ShapeId { index: 2 },
        ];
        document.pages.push(page);
        reorder(&mut document, &[vec![3, 1, 2]]);
        assert_eq!(
            document.pages[0].shapes,
            vec![
                ShapeId { index: 2 },
                ShapeId { index: 0 },
                ShapeId { index: 1 }
            ]
        );
    }

    #[test]
    fn a_group_children_take_the_order_of_their_list() {
        let mut document = Document::new();
        let mut group = Shape::new(ShapeId { index: 0 }, 1);
        group.children = vec![ShapeId { index: 1 }, ShapeId { index: 2 }];
        document.add_shape(group);
        document.add_shape(Shape::new(ShapeId { index: 1 }, 2));
        document.add_shape(Shape::new(ShapeId { index: 2 }, 3));
        reorder(&mut document, &[vec![3, 2]]);
        assert_eq!(
            document.shapes[0].children,
            vec![ShapeId { index: 2 }, ShapeId { index: 1 }]
        );
    }

    #[test]
    fn a_list_that_names_other_shapes_does_not_reorder() {
        let mut document = Document::new();
        for visio_id in [1, 2] {
            document.add_shape(Shape::new(ShapeId { index: 0 }, visio_id));
        }
        let mut page = larrygram_model::Page::new(larrygram_model::PageId { index: 0 }, 0);
        page.shapes = vec![ShapeId { index: 0 }, ShapeId { index: 1 }];
        document.pages.push(page);
        reorder(&mut document, &[vec![7, 8]]);
        assert_eq!(
            document.pages[0].shapes,
            vec![ShapeId { index: 0 }, ShapeId { index: 1 }]
        );
    }

    #[test]
    fn a_parent_reference_resolves_to_the_closest_earlier_shape() {
        // Shape ids repeat per page: page 2's shape 1 must not nest
        // under page 1's shape 1.
        let mut chunks = vec![chunk(0x48, 1, 1, shape_record_with_parent(0))];
        chunks.push(chunk(0x48, 1, 2, shape_record_with_parent(1)));
        chunks.push(chunk(0x48, 1, 1, shape_record_with_parent(0)));
        chunks.push(chunk(0x48, 1, 2, shape_record_with_parent(1)));
        let mut document = Document::new();
        let mut warnings = Vec::new();
        collect(&chunks, &mut document, &mut warnings).expect("collect");
        assert_eq!(document.shapes[1].parent, Some(ShapeId { index: 0 }));
        assert_eq!(document.shapes[3].parent, Some(ShapeId { index: 2 }));
        assert_eq!(document.shapes[2].parent, None);
    }

    #[test]
    fn an_unknown_record_outside_a_shape_is_silent() {
        let chunks = vec![chunk(0x2b, 1, 0, vec![])];
        let mut document = Document::new();
        let mut warnings = Vec::new();
        collect(&chunks, &mut document, &mut warnings).expect("collect");
        assert!(warnings.is_empty());
    }

    #[test]
    fn the_shape_count_is_capped() {
        let record = shape_record(u32::MAX, u32::MAX, u32::MAX, u32::MAX, u32::MAX);
        let chunks = vec![chunk(0x48, 1, 1, record.clone()), chunk(0x48, 1, 2, record)];
        let limits = Limits {
            max_shapes: 1,
            ..Limits::DEFAULT
        };
        let mut document = Document::new();
        let mut warnings = Vec::new();
        let error = collect_with(
            &chunks,
            FormatVersion::V11,
            &limits,
            &mut document,
            &mut warnings,
        )
        .expect_err("one shape too many");
        assert_eq!(
            error,
            Error::LimitExceeded {
                limit: "max_shapes",
                value: 1,
                max: 1
            }
        );
    }

    #[test]
    fn a_transform_outside_a_shape_is_ignored() {
        let chunks = vec![chunk(0x9b, 2, 0, xform(&[(1.0, 65)], 0, 0))];
        let mut document = Document::new();
        let mut warnings = Vec::new();
        collect(&chunks, &mut document, &mut warnings).expect("collect");
        assert!(document.shapes.is_empty());
    }

    #[test]
    fn a_geometry_row_outside_a_shape_is_ignored() {
        let chunks = vec![chunk(0x8b, 2, 1, join(&[tagged(1.0, 65), tagged(0.0, 65)]))];
        let mut document = Document::new();
        let mut warnings = Vec::new();
        collect(&chunks, &mut document, &mut warnings).expect("collect");
        assert!(document.shapes.is_empty());
    }

    #[test]
    fn a_nurbs_row_whose_inline_block_never_arrives_degrades() {
        let row = chunk(
            0xc3,
            3,
            1,
            join(&[
                tagged(1.0, 65),
                tagged(2.0, 65),
                double(0.5),
                double(1.0),
                double(0.0),
                double(1.0),
                vec![0],
                vec![0],
                vec![0; 9],
                word(24),
                vec![0],
                vec![4],
                vec![0; 3],
            ]),
        );
        let (document, warnings) = geometry_document(&[row], geom_list(&[1]));
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(e_text(&document), None);
    }

    #[test]
    fn a_polyline_block_of_length_zero_degrades() {
        let row = chunk(
            0xc1,
            3,
            1,
            join(&[
                tagged(1.0, 65),
                tagged(2.0, 65),
                vec![0],
                vec![0],
                vec![0; 9],
                word(0),
            ]),
        );
        let (document, warnings) = geometry_document(&[row], geom_list(&[1]));
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(cell_str(&section(&document).rows[0].cells, "A"), None);
    }

    #[test]
    fn a_typed_nurbs_block_of_unknown_parameter_type_stops() {
        let block = join(&[
            word(0),
            vec![0],
            vec![6],
            vec![0x77], // not `0x20`: the knot reads as a word
            1u16.to_le_bytes().to_vec(),
        ]);
        let row = chunk(
            0xc3,
            3,
            1,
            join(&[
                tagged(1.0, 65),
                tagged(2.0, 65),
                double(0.5),
                double(1.0),
                double(0.0),
                double(1.0),
                vec![0],
                vec![0],
                vec![0; 9],
                block,
            ]),
        );
        let (document, warnings) = geometry_document(&[row], geom_list(&[1]));
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(e_text(&document), None);
    }

    // -- the comparison with the `.vsdx` reader ------------------------

    /// One pinned cell: name, value and unit, for the comparison below.
    type PinnedCell<'a> = (&'a str, Option<CellValue>, Unit);

    /// The value and unit of one cell, for the comparison below.
    fn pinned(cell: &Cell) -> PinnedCell<'_> {
        (cell.name.as_str(), cell.value.clone(), cell.unit)
    }

    /// The same drawing as chunk records: one shape with transforms and
    /// geometry, and one curve-data record.
    fn comparison_drawing() -> Vec<Chunk<'static>> {
        vec![
            chunk(0x33, 0, 3, join(&[word(1), name16("Square")])),
            chunk(0xc9, 2, 0, name_index(&[(3, 1)])),
            chunk(0x65, 2, 0, vec![]),
            chunk(0x83, 3, 0, shape_id(1)),
            chunk(0x48, 1, 1, shape_record(7, u32::MAX, 4, 5, 6)),
            chunk(
                0x9b,
                2,
                0,
                join(&[
                    tagged(2.75, 65),
                    tagged(6.07, 65),
                    tagged(1.5, 70),
                    tagged(1.5, 70),
                    tagged(0.75, 64),
                    tagged(0.75, 64),
                    tagged(2.5, 81),
                    vec![1, 0],
                ]),
            ),
            chunk(
                0x9d,
                2,
                0,
                join(&[
                    tagged(1.0, 65),
                    tagged(2.0, 65),
                    tagged(3.0, 65),
                    tagged(4.0, 65),
                ]),
            ),
            chunk(
                0x9c,
                2,
                0,
                join(&[
                    tagged(1.0, 65),
                    tagged(2.0, 65),
                    tagged(3.0, 65),
                    tagged(4.0, 65),
                    tagged(0.5, 65),
                    tagged(0.5, 65),
                    tagged(0.0, 83),
                ]),
            ),
            chunk(0x6c, 2, 0, geom_list(&[1, 2, 3, 4])),
            chunk(0x89, 3, 0, vec![0b011]),
            chunk(0x8a, 3, 1, join(&[tagged(0.0, 65), tagged(1.0, 65)])),
            chunk(0x8b, 3, 2, join(&[tagged(1.5, 65), tagged(1.0, 65)])),
            chunk(
                0xc3,
                3,
                3,
                join(&[
                    tagged(1.0, 65),
                    tagged(2.0, 65),
                    double(0.5),
                    double(1.0),
                    double(0.0),
                    double(1.0),
                    vec![0],
                    vec![0x8a],
                    vec![0, 0, 0],
                    word(9),
                ]),
            ),
            chunk(
                0xc1,
                3,
                4,
                join(&[
                    tagged(2.0, 65),
                    tagged(2.0, 65),
                    vec![0],
                    vec![0x8b],
                    vec![0, 0, 0],
                    word(10),
                ]),
            ),
            chunk(0xd1, 0, 10, {
                let mut bytes = vec![0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
                bytes.push(0);
                bytes.push(1);
                bytes.extend(word(1));
                bytes.extend(double(0.25));
                bytes.extend(double(0.5));
                bytes
            }),
            chunk(0xd1, 0, 9, {
                let mut bytes = vec![0x82, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
                bytes.extend(double(1.0));
                bytes.extend(3u16.to_le_bytes());
                bytes.push(0);
                bytes.push(1);
                bytes.extend(word(1));
                bytes.extend(double(0.25));
                bytes.extend(double(0.5));
                bytes.extend(double(0.0));
                bytes.extend(double(1.0));
                bytes
            }),
        ]
    }

    /// The mapped fields of [`comparison_drawing`] as `crates/vsdx/src/read/**`
    /// produces them for the same drawing (its `map_sheet`, `map_cell` and
    /// `value` rules): cells in file order with their values and units,
    /// sections with their rows.
    ///
    /// The values were validated against that reader during P8-04 and are
    /// pinned here: a live cross-reader test needs a dev-dependency on
    /// `larrygram-vsdx`, which fails `cargo xtask layers` (both crates are
    /// L2). See the task Result.
    #[test]
    fn the_mapped_fields_match_the_vsdx_reader() {
        let mut document = Document::new();
        document.masters.push(Master::new(MasterId { index: 0 }, 7));
        let mut warnings = Vec::new();
        collect(&comparison_drawing(), &mut document, &mut warnings).expect("collect");
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(document.shapes.len(), 1);
        let shape = &document.shapes[0];
        // The shape's own fields, as the reader maps `Shape` attributes.
        assert_eq!(shape.visio_id, 1);
        assert_eq!(shape.name, "Square");
        assert_eq!(shape.name_u, "Square");
        assert_eq!(shape.master, Some(MasterId { index: 0 }));
        assert_eq!(shape.fill_style, Some(4));
        assert_eq!(shape.line_style, Some(5));
        assert_eq!(shape.text_style, Some(6));
        // The direct cells, in the order the reader maps `Cell` children.
        let cells: Vec<(&str, Option<CellValue>, Unit)> =
            shape.sheet.cells.iter().map(pinned).collect();
        assert_eq!(
            cells,
            [
                ("PinX", Some(CellValue::Number(2.75)), Unit::Inches),
                ("PinY", Some(CellValue::Number(6.07)), Unit::Inches),
                ("Width", Some(CellValue::Number(1.5)), Unit::Millimeters),
                ("Height", Some(CellValue::Number(1.5)), Unit::Millimeters),
                ("LocPinX", Some(CellValue::Number(0.75)), Unit::Unitsless),
                ("LocPinY", Some(CellValue::Number(0.75)), Unit::Unitsless),
                ("Angle", Some(CellValue::Number(2.5)), Unit::Degrees),
                ("FlipX", Some(CellValue::Bool(true)), Unit::Unitsless),
                ("FlipY", Some(CellValue::Bool(false)), Unit::Unitsless),
                ("BeginX", Some(CellValue::Number(1.0)), Unit::Inches),
                ("BeginY", Some(CellValue::Number(2.0)), Unit::Inches),
                ("EndX", Some(CellValue::Number(3.0)), Unit::Inches),
                ("EndY", Some(CellValue::Number(4.0)), Unit::Inches),
                ("TxtPinX", Some(CellValue::Number(1.0)), Unit::Inches),
                ("TxtPinY", Some(CellValue::Number(2.0)), Unit::Inches),
                ("TxtWidth", Some(CellValue::Number(3.0)), Unit::Inches),
                ("TxtHeight", Some(CellValue::Number(4.0)), Unit::Inches),
                ("TxtLocPinX", Some(CellValue::Number(0.5)), Unit::Inches),
                ("TxtLocPinY", Some(CellValue::Number(0.5)), Unit::Inches),
                ("TxtAngle", Some(CellValue::Number(0.0)), Unit::Radians),
            ]
        );
        // The `Geometry` section with its level cells and its rows.
        assert_eq!(shape.sheet.sections.len(), 1);
        let section = &shape.sheet.sections[0];
        assert_eq!(section.name.as_str(), "Geometry");
        let flags: Vec<(&str, Option<CellValue>, Unit)> =
            section.cells.iter().map(pinned).collect();
        assert_eq!(
            flags,
            [
                ("NoFill", Some(CellValue::Bool(true)), Unit::Unitsless),
                ("NoLine", Some(CellValue::Bool(true)), Unit::Unitsless),
                ("NoShow", Some(CellValue::Bool(false)), Unit::Unitsless),
            ]
        );
        let expected: [(&str, i32, Vec<PinnedCell>); 4] = [
            (
                "MoveTo",
                1,
                vec![
                    ("X", Some(CellValue::Number(0.0)), Unit::Inches),
                    ("Y", Some(CellValue::Number(1.0)), Unit::Inches),
                ],
            ),
            (
                "LineTo",
                2,
                vec![
                    ("X", Some(CellValue::Number(1.5)), Unit::Inches),
                    ("Y", Some(CellValue::Number(1.0)), Unit::Inches),
                ],
            ),
            (
                "NurbsTo",
                3,
                vec![
                    ("X", Some(CellValue::Number(1.0)), Unit::Inches),
                    ("Y", Some(CellValue::Number(2.0)), Unit::Inches),
                    ("A", Some(CellValue::Number(0.5)), Unit::Unitsless),
                    ("B", Some(CellValue::Number(1.0)), Unit::Unitsless),
                    ("C", Some(CellValue::Number(0.0)), Unit::Unitsless),
                    ("D", Some(CellValue::Number(1.0)), Unit::Unitsless),
                    (
                        "E",
                        Some(CellValue::Str(
                            "NURBS(1, 3, 0, 1, 0.25, 0.5, 0, 1)".to_owned(),
                        )),
                        Unit::Unitsless,
                    ),
                ],
            ),
            (
                "PolylineTo",
                4,
                vec![
                    ("X", Some(CellValue::Number(2.0)), Unit::Inches),
                    ("Y", Some(CellValue::Number(2.0)), Unit::Inches),
                    (
                        "A",
                        Some(CellValue::Str("POLYLINE(0, 1, 0.25, 0.5)".to_owned())),
                        Unit::Unitsless,
                    ),
                ],
            ),
        ];
        assert_eq!(section.rows.len(), expected.len());
        for (row, (kind, ix, cells)) in section.rows.iter().zip(expected) {
            assert_eq!(row.row_type.as_deref(), Some(kind));
            assert_eq!(row.ix, Some(ix));
            let pinned: Vec<PinnedCell> = row.cells.iter().map(pinned).collect();
            assert_eq!(pinned, cells);
        }
    }

    #[test]
    fn the_name_index_123_spelling_also_names_shapes() {
        let chunks = vec![
            chunk(0x33, 0, 1, join(&[word(1), name16("Square")])),
            chunk(0x34, 2, 0, join(&[word(1), word(1)])),
            chunk(
                0x48,
                1,
                1,
                shape_record(u32::MAX, u32::MAX, u32::MAX, u32::MAX, u32::MAX),
            ),
        ];
        let mut document = Document::new();
        let mut warnings = Vec::new();
        collect(&chunks, &mut document, &mut warnings).expect("collect");
        assert_eq!(document.shapes[0].name, "Square");
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn a_polyline_row_pointing_at_a_nurbs_curve_encodes_nothing() {
        let row = chunk(
            0xc1,
            3,
            1,
            join(&[
                tagged(1.0, 65),
                tagged(2.0, 65),
                vec![0],
                vec![0x8b],
                vec![0, 0, 0],
                word(9),
            ]),
        );
        let curve = chunk(
            0xd1,
            0,
            9,
            nurbs_data(1.0, 3, 0, 1, &[(0.25, 0.5, 0.0, 1.0)]),
        );
        let chunks = vec![
            chunk(
                0x48,
                1,
                1,
                shape_record(u32::MAX, u32::MAX, u32::MAX, u32::MAX, u32::MAX),
            ),
            chunk(0x6c, 2, 0, geom_list(&[1])),
            row,
            curve,
        ];
        let mut document = Document::new();
        let mut warnings = Vec::new();
        collect(&chunks, &mut document, &mut warnings).expect("collect");
        assert_eq!(cell_str(&section(&document).rows[0].cells, "A"), None);
    }

    #[test]
    fn a_spline_knot_without_its_knot_warns() {
        let rows = vec![chunk(0xa6, 3, 1, join(&[tagged(1.0, 65), tagged(1.0, 65)]))];
        let (document, warnings) = geometry_document(&rows, geom_list(&[1]));
        assert_eq!(
            kinds(&warnings),
            [WarningKind::BadValue {
                what: "geometry value"
            }]
        );
        assert_eq!(section(&document).rows.len(), 1);
    }

    #[test]
    fn every_unit_tag_of_the_format_maps_to_a_unit() {
        let expected: &[(u8, Unit)] = &[
            (32, Unit::Unitsless),
            (33, Unit::Percent),
            (36, Unit::Unitsless),
            (37, Unit::Unitsless),
            (40, Unit::Unitsless),
            (42, Unit::Unitsless),
            (43, Unit::Unitsless),
            (44, Unit::Unitsless),
            (45, Unit::Unitsless),
            (46, Unit::Unitsless),
            (47, Unit::Unitsless),
            (48, Unit::Unitsless),
            (49, Unit::Unitsless),
            (50, Unit::Points),
            (51, Unit::Picas),
            (52, Unit::Unitsless),
            (53, Unit::Unitsless),
            (54, Unit::Ciceros),
            (63, Unit::Unitsless),
            (64, Unit::Unitsless),
            (65, Unit::Inches),
            (66, Unit::Feet),
            (67, Unit::Unitsless),
            (68, Unit::Miles),
            (69, Unit::Centimeters),
            (70, Unit::Millimeters),
            (71, Unit::Meters),
            (72, Unit::Kilometers),
            (73, Unit::Unitsless),
            (74, Unit::Unitsless),
            (75, Unit::Yards),
            (76, Unit::Unitsless),
            (80, Unit::Unitsless),
            (81, Unit::Degrees),
            (82, Unit::Unitsless),
            (83, Unit::Radians),
            (84, Unit::Unitsless),
            (85, Unit::Unitsless),
            (95, Unit::Unitsless),
            (111, Unit::Unitsless),
            (138, Unit::Unitsless),
            (139, Unit::Unitsless),
            (225, Unit::Unitsless),
            (231, Unit::Unitsless),
            (232, Unit::Unitsless),
            (233, Unit::Unitsless),
            (251, Unit::Unitsless),
            (252, Unit::Unitsless),
        ];
        for (tag, unit) in expected {
            assert_eq!(cell_unit(*tag), Some(*unit), "tag {tag}");
        }
        // A tag the format does not define names no unit at all.
        assert_eq!(cell_unit(255), None);
        assert_eq!(cell_unit(0), None);
    }

    #[test]
    fn a_boolean_cell_reads_as_no_number() {
        let chunks = vec![chunk(
            0x48,
            1,
            1,
            shape_record(u32::MAX, u32::MAX, u32::MAX, u32::MAX, u32::MAX),
        )];
        let mut chunks = chunks;
        chunks.push(chunk(
            0x9b,
            2,
            0,
            xform(
                &[
                    (1.0, 65),
                    (1.0, 65),
                    (1.0, 65),
                    (1.0, 65),
                    (0.5, 65),
                    (0.5, 65),
                    (0.0, 65),
                ],
                1,
                0,
            ),
        ));
        let mut document = Document::new();
        let mut warnings = Vec::new();
        collect(&chunks, &mut document, &mut warnings).expect("collect");
        let cells = &document.shapes[0].sheet.cells;
        assert_eq!(number(cells, "FlipX"), None);
    }

    #[test]
    fn a_polyline_block_that_skips_an_earlier_cell_is_followed() {
        let block = join(&[
            word(8),
            vec![0],
            vec![1], // a cell before the A cell
            vec![0, 0],
            word(24),
            vec![0],
            vec![2],
            vec![0],
            0u16.to_le_bytes().to_vec(),
            vec![0],
            1u16.to_le_bytes().to_vec(),
            vec![0x81],
        ]);
        let row = chunk(
            0xc1,
            3,
            1,
            join(&[
                tagged(1.0, 65),
                tagged(2.0, 65),
                vec![0],
                vec![0],
                vec![0; 9],
                block,
            ]),
        );
        let (document, warnings) = geometry_document(&[row], geom_list(&[1]));
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(
            cell_str(&section(&document).rows[0].cells, "A"),
            Some(&CellValue::Str("POLYLINE(0, 1)".to_owned()))
        );
    }
}
