// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Chunk records and pointers of the `VisioDocument` stream (ADR 0069).
//!
//! The stream starts with a fixed header: the Visio magic
//! `"Visio (TM) Drawing\r\n\0"` and, at offset `0x1A`, the one-byte file
//! version. A pointer record at offset `0x24` names the trailer list:
//! the pointer table that indexes every other region of the stream. The
//! rest of the stream is a sequence of chunk records; each record header
//! names its record kind, its own id, the list it indexes into, its
//! payload length and its nesting level. Some records are pointers: they
//! redirect to another offset of the stream, and list records group the
//! items they index. The exact payload framing differs between the
//! version 6 and the version 11 format.
//!
//! [`read_chunks`] flattens the stream into [`Chunk`] values in pointer
//! order with their nesting level preserved; the domain modules
//! ([`crate::shape`], [`crate::text`], [`crate::style`],
//! [`crate::pages`]) interpret the payloads.
//!
//! # The walk
//!
//! A pointer target is one region of the stream, named by an absolute
//! offset and a length. A region whose `Format` bit 1 is set is stored
//! compressed and goes through [`crate::container::decompress`]
//! (ADR 0068). The high nibble of `Format` names the region's kind:
//!
//! - `0x0`, `0x4`, `0x5` — one blob record. The region is one [`Chunk`]
//!   whose kind, id and level come from the pointer, not from a record
//!   header. A `0x5` region (except a colour table) additionally holds a
//!   nested pointer list, which the walk descends into.
//! - `0x8`, `0xc`, `0xd` — a stream of chunk records with the 19-byte
//!   headers of this format, framed by the per-version payload rules.
//! - anything else is outside the format the reference reads; the region
//!   is skipped with an [`ImportWarning`].
//!
//! Every target offset is resolved at most once (a visited set), so a
//! pointer cycle terminates. Each pointer list is followed in the order
//! its order array names, then in table order. A record that is cut
//! short or nonsensical is skipped with a warning and the walk continues
//! at the next recoverable boundary (ADR 0071): the next pointer target.
//! [`Limits::max_chunks`] and [`Limits::max_chunk_level`] are hard caps.
//!
//! # Chunk data
//!
//! [`Chunk::data`] is the record payload: the declared payload length
//! followed by the trailer the per-version framing rules add (the list
//! records keep their child order there). Padding between records is not
//! part of any record and is skipped. Byte order and field widths follow
//! the file version (ADR 0069).
//!
//! Task P8-01 defined the entry points as interface stubs; task P8-03
//! implements them.

use std::collections::BTreeSet;

use crate::container::decompress;
use crate::error::Error;
use crate::limits::Limits;
use crate::warning::{ImportWarning, WarningKind};

/// The byte offset of the version inside the `VisioDocument` stream,
/// after the Visio magic.
pub const VERSION_OFFSET: usize = 0x1A;

/// The Visio magic that opens the `VisioDocument` stream (ADR 0069).
const MAGIC: &[u8] = b"Visio (TM) Drawing\r\n\0";

/// The byte offset of the trailer pointer: the pointer record that names
/// the pointer list at the root of the stream.
const TRAILER_POINTER_OFFSET: usize = 0x24;

/// One pointer record: `Type` u32, a reserved u32, `Offset` u32,
/// `Length` u32, `Format` u16 (ADR 0069).
const POINTER_LEN: usize = 18;

/// One chunk record header: `chunkType` u32, `id` u32, `list` u32,
/// `dataLength` u32, `level` u16, `unknown` u8 (ADR 0069).
const CHUNK_HEADER_LEN: usize = 19;

/// The hard bound on pointer-list nesting (AGENTS.md section 6: bound
/// recursion). [`Limits::max_chunk_level`] bounds the levels of real
/// files; this bound stops a hostile file that names no records at all.
const MAX_WALK_DEPTH: usize = 256;

/// The record kind of a colour table; such a pointer never has a nested
/// list (the reference's one exception).
const COLORS_KIND: u32 = 0x16;

/// The file format version (ADR 0069).
///
/// The alpha reads the two chunk-format versions. Versions 1 to 5 use
/// an older record layout and are out of scope;
/// [`FormatVersion::from_byte`] rejects them with
/// [`Error::UnsupportedVersion`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FormatVersion {
    /// File version 6 (Visio 2000 and 2002).
    V6,
    /// File version 11 (Visio 2003); every later Visio writes this
    /// version when it saves `.vsd`, `.vss` or `.vst`.
    V11,
}

impl FormatVersion {
    /// The format named by the version byte of the stream header.
    ///
    /// Versions 7 to 10 do not exist in real files; they are rejected
    /// the same way as versions 1 to 5.
    pub fn from_byte(byte: u8) -> Result<FormatVersion, Error> {
        match byte {
            6 => Ok(FormatVersion::V6),
            11 => Ok(FormatVersion::V11),
            other => Err(Error::UnsupportedVersion(other)),
        }
    }
}

/// The kind of one chunk record.
///
/// The value is the record-type id of the binary format. Each domain
/// module compares against the ids it consumes; unknown ids pass
/// through unchanged and are skipped with a warning. The id table is
/// derived from the libvisio reference (AGENTS.md section 5,
/// `VSDDocumentStructure.h`) and lives in this module with its
/// reference note (ADR 0069): a list of format identifiers and names,
/// and nothing else in the workspace sees the raw ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Record(pub u16);

impl Record {
    /// The format identifier of the record kind, as the reference
    /// spells it without its `VSD_` prefix, e.g. `"LINE_TO"`. Unknown
    /// ids have no name and report `"unknown record"`.
    pub fn name(&self) -> &'static str {
        match *self {
            FOREIGN_DATA => "FOREIGN_DATA",
            OLE_LIST => "OLE_LIST",
            TEXT => "TEXT",
            TRAILER_STREAM => "TRAILER_STREAM",
            PAGE => "PAGE",
            COLORS => "COLORS",
            FONT_LIST => "FONT_LIST",
            FONT_IX => "FONT_IX",
            STYLES => "STYLES",
            STENCILS => "STENCILS",
            STENCIL_PAGE => "STENCIL_PAGE",
            OLE_DATA => "OLE_DATA",
            PAGES => "PAGES",
            NAME_LIST => "NAME_LIST",
            NAME => "NAME",
            NAME_LIST2 => "NAME_LIST2",
            NAME2 => "NAME2",
            NAMEIDX123 => "NAMEIDX123",
            PAGE_SHEET => "PAGE_SHEET",
            SHAPE_GROUP => "SHAPE_GROUP",
            SHAPE_SHAPE => "SHAPE_SHAPE",
            STYLE_SHEET => "STYLE_SHEET",
            SHAPE_GUIDE => "SHAPE_GUIDE",
            SHAPE_FOREIGN => "SHAPE_FOREIGN",
            SCRATCH_LIST => "SCRATCH_LIST",
            SHAPE_LIST => "SHAPE_LIST",
            FIELD_LIST => "FIELD_LIST",
            PROP_LIST => "PROP_LIST",
            CHAR_LIST => "CHAR_LIST",
            PARA_LIST => "PARA_LIST",
            TABS_DATA_LIST => "TABS_DATA_LIST",
            GEOM_LIST => "GEOM_LIST",
            CUST_PROPS_LIST => "CUST_PROPS_LIST",
            ACT_ID_LIST => "ACT_ID_LIST",
            LAYER_LIST => "LAYER_LIST",
            CTRL_LIST => "CTRL_LIST",
            C_PNTS_LIST => "C_PNTS_LIST",
            CONNECT_LIST => "CONNECT_LIST",
            HYPER_LNK_LIST => "HYPER_LNK_LIST",
            SMART_TAG_LIST => "SMART_TAG_LIST",
            SHAPE_ID => "SHAPE_ID",
            EVENT => "EVENT",
            LINE => "LINE",
            FILL_AND_SHADOW => "FILL_AND_SHADOW",
            TEXT_BLOCK => "TEXT_BLOCK",
            TABS_DATA_1 => "TABS_DATA_1",
            GEOMETRY => "GEOMETRY",
            MOVE_TO => "MOVE_TO",
            LINE_TO => "LINE_TO",
            ARC_TO => "ARC_TO",
            INFINITE_LINE => "INFINITE_LINE",
            ELLIPSE => "ELLIPSE",
            ELLIPTICAL_ARC_TO => "ELLIPTICAL_ARC_TO",
            PAGE_PROPS => "PAGE_PROPS",
            STYLE_PROPS => "STYLE_PROPS",
            CHAR_IX => "CHAR_IX",
            PARA_IX => "PARA_IX",
            TABS_DATA_2 => "TABS_DATA_2",
            TABS_DATA_3 => "TABS_DATA_3",
            FOREIGN_DATA_TYPE => "FOREIGN_DATA_TYPE",
            CONNECTION_POINTS => "CONNECTION_POINTS",
            XFORM_DATA => "XFORM_DATA",
            TEXT_XFORM => "TEXT_XFORM",
            XFORM_1D => "XFORM_1D",
            SCRATCH => "SCRATCH",
            PROTECTION => "PROTECTION",
            TEXT_FIELD => "TEXT_FIELD",
            CONTROL_ANOTHER_TYPE => "CONTROL_ANOTHER_TYPE",
            MISC => "MISC",
            SPLINE_START => "SPLINE_START",
            SPLINE_KNOT => "SPLINE_KNOT",
            LAYER_MEMBERSHIP => "LAYER_MEMBERSHIP",
            LAYER => "LAYER",
            ACT_ID => "ACT_ID",
            CONTROL => "CONTROL",
            USER_DEFINED_CELLS => "USER_DEFINED_CELLS",
            TABS_DATA_4 => "TABS_DATA_4",
            CUSTOM_PROPS => "CUSTOM_PROPS",
            RULER_GRID => "RULER_GRID",
            CONNECTION_POINTS_ANOTHER_TYPE => "CONNECTION_POINTS_ANOTHER_TYPE",
            DOC_PROPS => "DOC_PROPS",
            IMAGE => "IMAGE",
            GROUP => "GROUP",
            LAYOUT => "LAYOUT",
            PAGE_LAYOUT_IX => "PAGE_LAYOUT_IX",
            POLYLINE_TO => "POLYLINE_TO",
            NURBS_TO => "NURBS_TO",
            HYPERLINK => "HYPERLINK",
            REVIEWER => "REVIEWER",
            ANNOTATION => "ANNOTATION",
            SMART_TAG_DEF => "SMART_TAG_DEF",
            PRINT_PROPS => "PRINT_PROPS",
            NAMEIDX => "NAMEIDX",
            SHAPE_DATA => "SHAPE_DATA",
            FONTFACE => "FONTFACE",
            FONTFACES => "FONTFACES",
            _ => "unknown record",
        }
    }
}

// The record-type id table (derived data; libvisio reference,
// `VSDDocumentStructure.h`; see ADR 0069).

/// An embedded picture or OLE payload.
pub const FOREIGN_DATA: Record = Record(0x0c);
/// The OLE data list of a shape.
pub const OLE_LIST: Record = Record(0x0d);
/// The text of a shape.
pub const TEXT: Record = Record(0x0e);
/// The trailer list: the root pointer list of the stream.
pub const TRAILER_STREAM: Record = Record(0x14);
/// One page.
pub const PAGE: Record = Record(0x15);
/// The document colour table.
pub const COLORS: Record = Record(0x16);
/// The font list.
pub const FONT_LIST: Record = Record(0x18);
/// One font.
pub const FONT_IX: Record = Record(0x19);
/// The stylesheet list.
pub const STYLES: Record = Record(0x1a);
/// The stencil list (`.vss` and `.vst` documents).
pub const STENCILS: Record = Record(0x1d);
/// One stencil page.
pub const STENCIL_PAGE: Record = Record(0x1e);
/// OLE data attached to a record.
pub const OLE_DATA: Record = Record(0x1f);
/// The page list.
pub const PAGES: Record = Record(0x27);
/// A name list.
pub const NAME_LIST: Record = Record(0x2c);
/// One name.
pub const NAME: Record = Record(0x2d);
/// A name list of the newer layout.
pub const NAME_LIST2: Record = Record(0x32);
/// One name of the newer layout.
pub const NAME2: Record = Record(0x33);
/// A name index list of the newer layout.
pub const NAMEIDX123: Record = Record(0x34);
/// The sheet of a page.
pub const PAGE_SHEET: Record = Record(0x46);
/// A group shape.
pub const SHAPE_GROUP: Record = Record(0x47);
/// A plain shape.
pub const SHAPE_SHAPE: Record = Record(0x48);
/// The sheet of a style.
pub const STYLE_SHEET: Record = Record(0x4a);
/// A guide shape.
pub const SHAPE_GUIDE: Record = Record(0x4d);
/// A shape with foreign data.
pub const SHAPE_FOREIGN: Record = Record(0x4e);
/// The scratch list of a sheet.
pub const SCRATCH_LIST: Record = Record(0x64);
/// The shape list of a group.
pub const SHAPE_LIST: Record = Record(0x65);
/// The text-field list of a shape.
pub const FIELD_LIST: Record = Record(0x66);
/// The shape-data property list.
pub const PROP_LIST: Record = Record(0x68);
/// The character list of a shape.
pub const CHAR_LIST: Record = Record(0x69);
/// The paragraph list of a shape.
pub const PARA_LIST: Record = Record(0x6a);
/// The tab-stop list of a shape.
pub const TABS_DATA_LIST: Record = Record(0x6b);
/// The geometry list of a shape.
pub const GEOM_LIST: Record = Record(0x6c);
/// The custom-property list of a sheet.
pub const CUST_PROPS_LIST: Record = Record(0x6d);
/// The action list of a sheet.
pub const ACT_ID_LIST: Record = Record(0x6e);
/// The layer list of a page.
pub const LAYER_LIST: Record = Record(0x6f);
/// The control list of a sheet.
pub const CTRL_LIST: Record = Record(0x70);
/// The connection-point list of a shape.
pub const C_PNTS_LIST: Record = Record(0x71);
/// The connection list of a page.
pub const CONNECT_LIST: Record = Record(0x72);
/// The hyperlink list of a sheet.
pub const HYPER_LNK_LIST: Record = Record(0x73);
/// The smart-tag list of a shape.
pub const SMART_TAG_LIST: Record = Record(0x76);
/// The id of a shape.
pub const SHAPE_ID: Record = Record(0x83);
/// The event list of a sheet.
pub const EVENT: Record = Record(0x84);
/// The line style of a sheet.
pub const LINE: Record = Record(0x85);
/// The fill and shadow style of a sheet.
pub const FILL_AND_SHADOW: Record = Record(0x86);
/// The text block style of a sheet.
pub const TEXT_BLOCK: Record = Record(0x87);
/// A tab-stop row.
pub const TABS_DATA_1: Record = Record(0x88);
/// One geometry section of a shape.
pub const GEOMETRY: Record = Record(0x89);
/// A geometry move-to row.
pub const MOVE_TO: Record = Record(0x8a);
/// A geometry line-to row.
pub const LINE_TO: Record = Record(0x8b);
/// A geometry arc-to row.
pub const ARC_TO: Record = Record(0x8c);
/// A geometry infinite-line row.
pub const INFINITE_LINE: Record = Record(0x8d);
/// A geometry ellipse row.
pub const ELLIPSE: Record = Record(0x8f);
/// A geometry elliptical-arc-to row.
pub const ELLIPTICAL_ARC_TO: Record = Record(0x90);
/// The page properties of a sheet.
pub const PAGE_PROPS: Record = Record(0x92);
/// The style properties of a stylesheet.
pub const STYLE_PROPS: Record = Record(0x93);
/// One character row.
pub const CHAR_IX: Record = Record(0x94);
/// One paragraph row.
pub const PARA_IX: Record = Record(0x95);
/// A second tab-stop row kind.
pub const TABS_DATA_2: Record = Record(0x96);
/// A third tab-stop row kind.
pub const TABS_DATA_3: Record = Record(0x97);
/// The foreign-data type of a shape.
pub const FOREIGN_DATA_TYPE: Record = Record(0x98);
/// The connection points of a shape.
pub const CONNECTION_POINTS: Record = Record(0x99);
/// The transform of a shape.
pub const XFORM_DATA: Record = Record(0x9b);
/// The text transform of a shape.
pub const TEXT_XFORM: Record = Record(0x9c);
/// The one-dimensional transform of a connector.
pub const XFORM_1D: Record = Record(0x9d);
/// The scratch rows of a sheet.
pub const SCRATCH: Record = Record(0x9e);
/// The protection flags of a sheet.
pub const PROTECTION: Record = Record(0xa0);
/// One text field.
pub const TEXT_FIELD: Record = Record(0xa1);
/// A control of the other row kind.
pub const CONTROL_ANOTHER_TYPE: Record = Record(0xa2);
/// The miscellaneous flags of a shape.
pub const MISC: Record = Record(0xa4);
/// A geometry spline-start row.
pub const SPLINE_START: Record = Record(0xa5);
/// A geometry spline-knot row.
pub const SPLINE_KNOT: Record = Record(0xa6);
/// The layer membership of a shape.
pub const LAYER_MEMBERSHIP: Record = Record(0xa7);
/// One layer of a page.
pub const LAYER: Record = Record(0xa8);
/// One action of a sheet.
pub const ACT_ID: Record = Record(0xa9);
/// One control of a sheet.
pub const CONTROL: Record = Record(0xaa);
/// The user-defined cells of a sheet.
pub const USER_DEFINED_CELLS: Record = Record(0xb4);
/// A fourth tab-stop row kind.
pub const TABS_DATA_4: Record = Record(0xb5);
/// The custom properties of a sheet.
pub const CUSTOM_PROPS: Record = Record(0xb6);
/// The ruler and grid settings of a page.
pub const RULER_GRID: Record = Record(0xb7);
/// Connection points of the other row kind.
pub const CONNECTION_POINTS_ANOTHER_TYPE: Record = Record(0xba);
/// The document properties.
pub const DOC_PROPS: Record = Record(0xbc);
/// A bitmap image.
pub const IMAGE: Record = Record(0xbd);
/// The grouping flags of a shape.
pub const GROUP: Record = Record(0xbe);
/// The layout settings of a shape.
pub const LAYOUT: Record = Record(0xbf);
/// One page-layout row.
pub const PAGE_LAYOUT_IX: Record = Record(0xc0);
/// A geometry polyline-to row.
pub const POLYLINE_TO: Record = Record(0xc1);
/// A geometry NURBS-to row.
pub const NURBS_TO: Record = Record(0xc3);
/// One hyperlink.
pub const HYPERLINK: Record = Record(0xc4);
/// One reviewer.
pub const REVIEWER: Record = Record(0xc5);
/// One annotation.
pub const ANNOTATION: Record = Record(0xc6);
/// One smart-tag definition.
pub const SMART_TAG_DEF: Record = Record(0xc7);
/// The print properties of a document.
pub const PRINT_PROPS: Record = Record(0xc8);
/// A name index.
pub const NAMEIDX: Record = Record(0xc9);
/// One shape-data row.
pub const SHAPE_DATA: Record = Record(0xd1);
/// One font face.
pub const FONTFACE: Record = Record(0xd7);
/// The font-face list.
pub const FONTFACES: Record = Record(0xd8);

/// One chunk record of the `VisioDocument` stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chunk<'a> {
    /// The record kind.
    pub record: Record,
    /// The nesting level of the record: its header level plus the level
    /// of the pointer list that reached it (the reference adds one per
    /// pointer depth).
    pub level: u16,
    /// The id of the list this record indexes into, or `0`.
    pub list: u32,
    /// The record's own id within its list. A blob record has no header;
    /// its id is the index of its pointer in the pointer list.
    pub id: u32,
    /// The record payload, after the record header: the declared payload
    /// length plus the trailing region the per-version framing rules add
    /// (ADR 0069). Byte order and field widths follow the file version.
    ///
    /// The trailing region is part of the record: the list records (the
    /// geometry, shape, character and other lists) keep the end of their
    /// child-id list there, past the declared payload length, so a
    /// consumer that stops at the declared length loses rows.
    pub data: &'a [u8],
}

/// The format version of a `VisioDocument` stream.
///
/// The stream must carry the Visio magic and the version byte at
/// [`VERSION_OFFSET`]; a shorter stream is [`Error::Invalid`] and a
/// wrong magic is [`Error::NotAVisioFile`] (ADR 0071).
#[allow(dead_code)] // no caller until P8-07 wires the pipeline
pub fn version(visio_document: &[u8]) -> Result<FormatVersion, Error> {
    let Some(byte) = visio_document.get(VERSION_OFFSET).copied() else {
        return Err(Error::Invalid(
            "VisioDocument stream is shorter than its header".to_owned(),
        ));
    };
    if visio_document.get(..MAGIC.len()) != Some(MAGIC) {
        return Err(Error::NotAVisioFile(
            "the VisioDocument stream does not start with the Visio magic".to_owned(),
        ));
    }
    FormatVersion::from_byte(byte)
}

/// Reads every chunk record of the `VisioDocument` stream.
///
/// `stream` is the whole stream, header included; `file_version` is the
/// version [`version`] read from it. Pointers and lists are resolved to
/// the stream offsets they name, each offset at most once (a visited set,
/// so a pointer cycle terminates), and the result is the records in
/// pointer order with their nesting level preserved. `limits` caps the
/// record count and the nesting level.
///
/// A compressed region is restored through
/// [`crate::container::decompress`] (ADR 0068); the decompressed blocks
/// are appended to `blocks`, which must outlive the returned [`Chunk`]
/// values because their payloads borrow from it. Callers keep `blocks`
/// alive beside the chunks: `let mut blocks = Vec::new(); let (chunks,
/// warnings) = read_chunks(stream, version, &limits, &mut blocks)?;`
///
/// The result carries the records and the [`ImportWarning`] values that
/// record every degradation (ADR 0071). A record that is cut short or
/// nonsensical is skipped with a warning and the walk continues at the
/// next recoverable boundary. When no record at all can be read, the
/// result is [`Error::Invalid`].
#[allow(dead_code)] // no caller until P8-07 wires the pipeline
pub fn read_chunks<'a>(
    stream: &'a [u8],
    file_version: FormatVersion,
    limits: &Limits,
    blocks: &'a mut Vec<Vec<u8>>,
) -> Result<(Vec<Chunk<'a>>, Vec<ImportWarning>), Error> {
    let pointer = read_pointer(stream, TRAILER_POINTER_OFFSET).ok_or_else(|| {
        Error::Invalid("VisioDocument stream is shorter than its trailer pointer".to_owned())
    })?;
    let mut walker = Walker {
        stream,
        version: file_version,
        limits,
        blocks,
        pending: Vec::new(),
        warnings: Vec::new(),
        visited: BTreeSet::new(),
    };
    walker.walk_root(pointer)?;
    let Walker {
        stream,
        blocks,
        pending,
        warnings,
        ..
    } = walker;
    let store: &'a [Vec<u8>] = blocks;
    let mut chunks = Vec::with_capacity(pending.len());
    for record in pending {
        let data = resolve(record.datum, stream, store);
        chunks.push(Chunk {
            record: record.record,
            level: record.level,
            list: record.list,
            id: record.id,
            data,
        });
    }
    if chunks.is_empty() && !warnings.is_empty() {
        return Err(Error::Invalid(
            "no chunk record could be read from the VisioDocument stream".to_owned(),
        ));
    }
    Ok((chunks, warnings))
}

/// The payload framing of one record: the bytes the per-version rules
/// append to the declared payload (ADR 0069).
///
/// The rules come from the reference's two header readers
/// (`VSD6Parser::getChunkHeader` for version 6,
/// `VSDParser::getChunkHeader` for version 11). The version 11 rules are
/// heuristics the reference itself calls incomplete; the walk skips the
/// padding between records separately, so an under-counted trailer never
/// loses a record.
fn trailer_len(version: FormatVersion, kind: u32, list: u32, level: u16, unknown: u8) -> u64 {
    match version {
        FormatVersion::V6 => {
            let mut trailer = 0u64;
            if list != 0 || matches!(kind, 0x0d | 0x2c | 0x64..=0x73 | 0x76) {
                trailer += 8;
            }
            if matches!(kind, 0x1f | 0xc9) {
                trailer = 0;
            }
            trailer
        }
        FormatVersion::V11 => {
            let mut trailer = 0u64;
            if list != 0 || matches!(kind, 0x2c | 0x65 | 0x66 | 0x69 | 0x6a | 0x6b | 0x70 | 0x71) {
                trailer += 8;
            }
            if list != 0
                || (level == 2 && unknown == 0x55)
                || (level == 2 && unknown == 0x54 && kind == 0xaa)
                || (level == 3 && unknown != 0x50 && unknown != 0x54)
            {
                trailer += 4;
            }
            if matches!(
                kind,
                0x64 | 0x65
                    | 0x66
                    | 0x69
                    | 0x6a
                    | 0x6b
                    | 0x6f
                    | 0x71
                    | 0x92
                    | 0xa9
                    | 0xb4
                    | 0xb6
                    | 0xb9
                    | 0xc7
            ) && trailer != 12
                && trailer != 4
            {
                trailer += 4;
            }
            if matches!(kind, 0x1f | 0xc9 | 0x2d | 0xd1) {
                trailer = 0;
            }
            trailer
        }
    }
}

/// The extent of a list record's child-id list (ADR 0069).
///
/// A list record (the `0x64..=0x76` family: scratch, shape, field,
/// property, character, paragraph, tabs, geometry and the other lists)
/// opens with a list header: the length of its sub-header and the byte
/// count of its child-id list. The ids follow the sub-header and the
/// last of them can sit past the declared payload length, in the
/// trailing region. When the header names more than the framing rules
/// cover, the record grows to hold the whole child list; a header that
/// cannot be this record's is left to the framing rules alone. The
/// layout is the one the reference's `readGeomList` and its siblings
/// read.
fn list_extent(kind: u32, buf: &[u8], at: usize, frame: u64) -> u64 {
    if !matches!(kind, 0x64..=0x76) {
        return frame;
    }
    let Some(sub_header) = read_u32(buf, at) else {
        return frame;
    };
    let Some(bytes) = read_u32(buf, at.saturating_add(4)) else {
        return frame;
    };
    if bytes == 0 || u64::from(sub_header) > 0x1000 || bytes % 4 != 0 {
        return frame;
    }
    let need = 8u64
        .saturating_add(u64::from(sub_header))
        .saturating_add(u64::from(bytes));
    // Only grow while the child list fits the region: a header naming
    // more than the region holds is not this record's.
    if need > buf.len().saturating_sub(at) as u64 {
        return frame;
    }
    frame.max(need)
}

/// One pointer record of a pointer list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Pointer {
    /// The record kind of the target (`Type`).
    kind: u32,
    /// The absolute offset of the target in the `VisioDocument` stream.
    offset: u32,
    /// The length of the target region in the stream.
    length: u32,
    /// The target's format flags (`Format`).
    format: u16,
}

/// The buffer a [`Chunk`] payload borrows from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Base {
    /// The `VisioDocument` stream.
    Stream,
    /// One decompressed block.
    Block(usize),
}

/// Where a [`Chunk`] payload borrows from: a range of one buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Datum {
    base: Base,
    start: usize,
    len: usize,
}

/// The bytes one datum names, or an empty slice when the range fell out
/// of its buffer (every datum is bounds-checked when the walk records
/// it, so this cannot happen).
fn resolve<'a>(datum: Datum, stream: &'a [u8], store: &'a [Vec<u8>]) -> &'a [u8] {
    let buffer: &[u8] = match datum.base {
        Base::Stream => stream,
        Base::Block(block) => store.get(block).map(Vec::as_slice).unwrap_or_default(),
    };
    buffer
        .get(datum.start..datum.start.saturating_add(datum.len))
        .unwrap_or_default()
}

/// One record the walk found, before its payload is resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Pending {
    record: Record,
    level: u16,
    list: u32,
    id: u32,
    datum: Datum,
}

/// A parsed pointer list: the pointers in table order and the order
/// array the list names (empty when the list has none).
#[derive(Debug, Default)]
struct PointerList {
    entries: Vec<Pointer>,
    order: Vec<u32>,
}

/// The bytes of one region after decompression.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Region {
    source: Datum,
    /// The shift the format applies to the region's content (4 for a
    /// compressed region, 0 otherwise).
    shift: usize,
    /// The region's offset in the `VisioDocument` stream, for warnings.
    stream_offset: u32,
}

/// Reads one pointer record at `at`, or `None` when the buffer ends
/// before it.
fn read_pointer(buf: &[u8], at: usize) -> Option<Pointer> {
    let bytes = buf.get(at..at.checked_add(POINTER_LEN)?)?;
    Some(Pointer {
        kind: read_u32(bytes, 0)?,
        offset: read_u32(bytes, 8)?,
        length: read_u32(bytes, 12)?,
        format: read_u16(bytes, 16)?,
    })
}

/// Reads a little-endian u32 at `at`, or `None` when out of range.
fn read_u32(buf: &[u8], at: usize) -> Option<u32> {
    let bytes = buf.get(at..at.checked_add(4)?)?;
    let array: [u8; 4] = bytes.try_into().ok()?;
    Some(u32::from_le_bytes(array))
}

/// Reads a little-endian i32 at `at`, or `None` when out of range.
fn read_i32(buf: &[u8], at: usize) -> Option<i32> {
    let bytes = buf.get(at..at.checked_add(4)?)?;
    let array: [u8; 4] = bytes.try_into().ok()?;
    Some(i32::from_le_bytes(array))
}

/// Reads a little-endian u16 at `at`, or `None` when out of range.
fn read_u16(buf: &[u8], at: usize) -> Option<u16> {
    let bytes = buf.get(at..at.checked_add(2)?)?;
    let array: [u8; 2] = bytes.try_into().ok()?;
    Some(u16::from_le_bytes(array))
}

/// Parses the pointer list inside one region (the reference's
/// `readPointerInfo` and `readPointer`).
///
/// `shift` is 4 for a compressed region and 0 otherwise. The list's own
/// table position comes from the u32 the format keeps at `shift`. A list
/// whose counts run past the region is read as far as it goes, with one
/// warning per truncated part.
fn parse_pointer_list(
    buf: &[u8],
    shift: usize,
    context: &str,
) -> (PointerList, Vec<ImportWarning>) {
    let mut warnings = Vec::new();
    let mut list = PointerList::default();
    let offset = match read_u32(buf, shift) {
        Some(offset) => offset,
        None => {
            warnings.push(ImportWarning {
                context: context.to_owned(),
                kind: WarningKind::BadValue {
                    what: "pointer list offset",
                },
            });
            return (list, warnings);
        }
    };
    // The table sits at `offset + shift - 4` (the reference's
    // `readPointerInfo`).
    let table = i64::from(offset) + shift as i64 - 4;
    if table < 0 || table as usize > buf.len() {
        warnings.push(ImportWarning {
            context: context.to_owned(),
            kind: WarningKind::BadValue {
                what: "pointer list offset",
            },
        });
        return (list, warnings);
    }
    let table = table as usize;
    if table.saturating_add(12) > buf.len() {
        warnings.push(ImportWarning {
            context: context.to_owned(),
            kind: WarningKind::BadValue {
                what: "pointer list offset",
            },
        });
        return (list, warnings);
    }
    let list_size = read_u32(buf, table).unwrap_or(0);
    let pointer_count = read_i32(buf, table.saturating_add(4)).unwrap_or(0);
    let mut at = table.saturating_add(12);
    if pointer_count < 0 {
        warnings.push(ImportWarning {
            context: context.to_owned(),
            kind: WarningKind::BadValue {
                what: "pointer count",
            },
        });
        return (list, warnings);
    }
    let available = buf.len().saturating_sub(at) / POINTER_LEN;
    let declared = pointer_count as usize;
    let count = declared.min(available);
    if count < declared {
        warnings.push(ImportWarning {
            context: context.to_owned(),
            kind: WarningKind::BadValue {
                what: "pointer list",
            },
        });
    }
    for _ in 0..count {
        if let Some(pointer) = read_pointer(buf, at) {
            list.entries.push(pointer);
        }
        at = at.saturating_add(POINTER_LEN);
    }
    if list_size > 1 {
        let available = buf.len().saturating_sub(at) / 4;
        let declared = list_size as usize;
        let count = declared.min(available);
        if count < declared {
            warnings.push(ImportWarning {
                context: context.to_owned(),
                kind: WarningKind::BadValue {
                    what: "pointer order list",
                },
            });
        }
        for _ in 0..count {
            if let Some(index) = read_u32(buf, at) {
                list.order.push(index);
            }
            at = at.saturating_add(4);
        }
    }
    (list, warnings)
}

/// Parses the chunk records of one region (the reference's
/// `handleChunks` with its per-version `getChunkHeader`).
///
/// The walk stops at the first record that is cut short or whose length
/// runs past the region, with one warning; records before it are kept.
fn parse_chunk_stream(
    buf: &[u8],
    source: Datum,
    version: FormatVersion,
    limits: &Limits,
    level_offset: u32,
    context: &str,
) -> Result<(Vec<Pending>, Vec<ImportWarning>), Error> {
    let mut warnings = Vec::new();
    let mut out = Vec::new();
    let mut at = 0usize;
    while at < buf.len() {
        // The reference skips padding between records.
        while at < buf.len() && buf.get(at) == Some(&0u8) {
            at += 1;
        }
        if at >= buf.len() {
            break;
        }
        if at.saturating_add(CHUNK_HEADER_LEN) > buf.len() {
            warnings.push(ImportWarning {
                context: context.to_owned(),
                kind: WarningKind::SkippedRecord {
                    name: "truncated chunk record",
                },
            });
            break;
        }
        let kind = read_u32(buf, at).unwrap_or(0);
        let id = read_u32(buf, at.saturating_add(4)).unwrap_or(0);
        let list = read_u32(buf, at.saturating_add(8)).unwrap_or(0);
        let data_len = read_u32(buf, at.saturating_add(12)).unwrap_or(0);
        let level = read_u16(buf, at.saturating_add(16)).unwrap_or(0);
        let unknown = buf.get(at.saturating_add(18)).copied().unwrap_or(0);
        let trailer = trailer_len(version, kind, list, level, unknown);
        let start = at.saturating_add(CHUNK_HEADER_LEN);
        let total = list_extent(
            kind,
            buf,
            start,
            u64::from(data_len).saturating_add(trailer),
        );
        if u64::try_from(at)
            .unwrap_or(u64::MAX)
            .saturating_add(CHUNK_HEADER_LEN as u64)
            .saturating_add(total)
            > buf.len() as u64
        {
            warnings.push(ImportWarning {
                context: context.to_owned(),
                kind: WarningKind::BadValue {
                    what: "chunk record length",
                },
            });
            break;
        }
        let computed_level = u64::from(level).saturating_add(u64::from(level_offset));
        if computed_level > u64::from(limits.max_chunk_level) {
            return Err(Error::LimitExceeded {
                limit: "max_chunk_level",
                value: computed_level,
                max: u64::from(limits.max_chunk_level),
            });
        }
        if u64::try_from(out.len())
            .unwrap_or(u64::MAX)
            .saturating_add(1)
            > u64::from(limits.max_chunks)
        {
            return Err(Error::LimitExceeded {
                limit: "max_chunks",
                value: u64::try_from(out.len())
                    .unwrap_or(u64::MAX)
                    .saturating_add(1),
                max: u64::from(limits.max_chunks),
            });
        }
        let len = usize::try_from(total).unwrap_or(usize::MAX);
        if kind > u32::from(u16::MAX) {
            warnings.push(ImportWarning {
                context: context.to_owned(),
                kind: WarningKind::SkippedRecord {
                    name: "chunk record with a wide type id",
                },
            });
        } else {
            out.push(Pending {
                record: Record(kind as u16),
                level: computed_level as u16,
                list,
                id,
                datum: Datum {
                    base: source.base,
                    start: source.start.saturating_add(start),
                    len,
                },
            });
        }
        at = start.saturating_add(len);
    }
    Ok((out, warnings))
}

/// The walk itself: pointers into [`Pending`] records.
struct Walker<'a, 'l> {
    stream: &'a [u8],
    version: FormatVersion,
    limits: &'l Limits,
    blocks: &'a mut Vec<Vec<u8>>,
    pending: Vec<Pending>,
    warnings: Vec<ImportWarning>,
    visited: BTreeSet<u32>,
}

impl<'a, 'l> Walker<'a, 'l> {
    /// Records one degradation.
    fn warn(&mut self, context: String, kind: WarningKind) {
        self.warnings.push(ImportWarning { context, kind });
    }

    /// Appends one record, enforcing [`Limits::max_chunks`].
    fn push(&mut self, pending: Pending) -> Result<(), Error> {
        if u64::try_from(self.pending.len())
            .unwrap_or(u64::MAX)
            .saturating_add(1)
            > u64::from(self.limits.max_chunks)
        {
            return Err(Error::LimitExceeded {
                limit: "max_chunks",
                value: u64::try_from(self.pending.len())
                    .unwrap_or(u64::MAX)
                    .saturating_add(1),
                max: u64::from(self.limits.max_chunks),
            });
        }
        self.pending.push(pending);
        Ok(())
    }

    /// Restores one pointer target's region, decompressing it when the
    /// format marks it compressed (ADR 0068). `None` is a degradation
    /// that was recorded with a warning; `Err` is a cap or a stub seam.
    fn region(&mut self, pointer: &Pointer) -> Result<Option<Region>, Error> {
        let start = pointer.offset as usize;
        let len = pointer.length as usize;
        if start > self.stream.len() || len > self.stream.len().saturating_sub(start) {
            let context = format!("pointer target at offset {:#x}", pointer.offset);
            self.warn(
                context,
                WarningKind::BadValue {
                    what: "pointer range",
                },
            );
            return Ok(None);
        }
        let raw = self
            .stream
            .get(start..start.saturating_add(len))
            .unwrap_or_default();
        if pointer.format & 2 == 2 {
            match decompress(raw, self.limits) {
                Ok(bytes) => {
                    let block = self.blocks.len();
                    let len = bytes.len();
                    self.blocks.push(bytes);
                    Ok(Some(Region {
                        source: Datum {
                            base: Base::Block(block),
                            start: 0,
                            len,
                        },
                        shift: 4,
                        stream_offset: pointer.offset,
                    }))
                }
                // The seam fails only on its caps (ADR 0068), and a
                // cap is an error, not a degradation (ADR 0071).
                Err(e) => Err(e),
            }
        } else {
            Ok(Some(Region {
                source: Datum {
                    base: Base::Stream,
                    start,
                    len,
                },
                shift: 0,
                stream_offset: pointer.offset,
            }))
        }
    }

    /// Walks the trailer list at the root of the stream.
    fn walk_root(&mut self, pointer: Pointer) -> Result<(), Error> {
        let Some(region) = self.region(&pointer)? else {
            return Ok(());
        };
        let (list, warnings) = {
            let buf = self.region_bytes(&region);
            let context = format!("pointer list at offset {:#x}", region.stream_offset);
            parse_pointer_list(buf, region.shift, &context)
        };
        self.warnings.extend(warnings);
        self.walk_list(list, 2, 1)
    }

    /// Walks one pointer list at `level_offset` (the level its blob
    /// records get and the offset its chunk records add to their header
    /// level) and nesting `depth`.
    fn walk_list(
        &mut self,
        list: PointerList,
        level_offset: u32,
        depth: usize,
    ) -> Result<(), Error> {
        if depth > MAX_WALK_DEPTH {
            self.warn(
                "pointer list".to_owned(),
                WarningKind::Dropped {
                    what: "deeply nested pointer list",
                },
            );
            return Ok(());
        }
        // The order array first, then the rest of the table (the
        // reference's traversal order).
        let mut seen = BTreeSet::new();
        let mut sequence = Vec::with_capacity(list.entries.len());
        for index in &list.order {
            let index = *index as usize;
            if index < list.entries.len() && seen.insert(index) {
                sequence.push(index);
            }
        }
        for index in 0..list.entries.len() {
            if seen.insert(index) {
                sequence.push(index);
            }
        }
        for (index, pointer) in sequence
            .into_iter()
            .filter_map(|index| Some((index, *list.entries.get(index)?)))
        {
            self.walk_target(index as u32, pointer, level_offset, depth)?;
        }
        Ok(())
    }

    /// Walks one pointer target: a blob record, a nested pointer list or
    /// a stream of chunk records.
    fn walk_target(
        &mut self,
        index: u32,
        pointer: Pointer,
        level_offset: u32,
        depth: usize,
    ) -> Result<(), Error> {
        if pointer.kind == 0 {
            return Ok(());
        }
        if !self.visited.insert(pointer.offset) {
            let context = format!("pointer target at offset {:#x}", pointer.offset);
            self.warn(
                context,
                WarningKind::Dropped {
                    what: "repeated pointer target",
                },
            );
            return Ok(());
        }
        let Some(region) = self.region(&pointer)? else {
            return Ok(());
        };
        let nibble = u32::from(pointer.format) >> 4;
        match nibble {
            0x0 | 0x4 | 0x5 => self.walk_blob(index, pointer, &region, level_offset, depth),
            0x8 | 0xc | 0xd => {
                let context = format!("chunk stream at offset {:#x}", region.stream_offset);
                let (records, warnings) = {
                    let buf = self.region_bytes(&region);
                    parse_chunk_stream(
                        buf,
                        region.source,
                        self.version,
                        self.limits,
                        level_offset,
                        &context,
                    )?
                };
                self.warnings.extend(warnings);
                for record in records {
                    self.push(record)?;
                }
                Ok(())
            }
            _ => {
                let context = format!("pointer target at offset {:#x}", pointer.offset);
                self.warn(
                    context,
                    WarningKind::Unsupported {
                        what: "pointer format",
                    },
                );
                Ok(())
            }
        }
    }

    /// Walks one blob record: one [`Chunk`] whose kind, id and level
    /// come from the pointer, and (for a `0x5` region that is not a
    /// colour table) the nested pointer list inside it.
    fn walk_blob(
        &mut self,
        index: u32,
        pointer: Pointer,
        region: &Region,
        level_offset: u32,
        depth: usize,
    ) -> Result<(), Error> {
        let (region_start, region_len) = (region.source.start, region.source.len);
        if region_len < region.shift {
            self.warn(
                format!("pointer target at offset {:#x}", pointer.offset),
                WarningKind::BadValue {
                    what: "pointer range",
                },
            );
            return Ok(());
        }
        // The blob payload is the region after the format's shift.
        let datum = Datum {
            base: region.source.base,
            start: region_start.saturating_add(region.shift),
            len: region_len.saturating_sub(region.shift),
        };
        if u64::from(level_offset) > u64::from(self.limits.max_chunk_level) {
            return Err(Error::LimitExceeded {
                limit: "max_chunk_level",
                value: u64::from(level_offset),
                max: u64::from(self.limits.max_chunk_level),
            });
        }
        if pointer.kind > u32::from(u16::MAX) {
            self.warn(
                format!("pointer target at offset {:#x}", pointer.offset),
                WarningKind::SkippedRecord {
                    name: "blob record with a wide type id",
                },
            );
        } else {
            self.push(Pending {
                record: Record(pointer.kind as u16),
                level: level_offset as u16,
                list: 0,
                id: index,
                datum,
            })?;
        }
        if u32::from(pointer.format) >> 4 == 0x5 && pointer.kind != COLORS_KIND {
            let (list, warnings) = {
                let buf = self.region_bytes(region);
                let context = format!("pointer list at offset {:#x}", region.stream_offset);
                parse_pointer_list(buf, region.shift, &context)
            };
            self.warnings.extend(warnings);
            self.walk_list(
                list,
                level_offset.saturating_add(2),
                depth.saturating_add(1),
            )?;
        }
        Ok(())
    }

    /// The bytes of one region, borrowed only for the duration of a
    /// free parse call (a decompressed block lives in `self.blocks`).
    fn region_bytes(&self, region: &Region) -> &[u8] {
        resolve(region.source, self.stream, self.blocks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    // --- test fixtures -------------------------------------------------
    //
    // Streams are hand-built byte by byte: `header` is the fixed stream
    // header, `pointer` and `chunk_header` the two record layouts, and
    // `build` assembles a small but valid stream around one pointer
    // list. Every pointer offset is absolute in the stream.

    fn header(version: u8) -> Vec<u8> {
        let mut stream = vec![0u8; TRAILER_POINTER_OFFSET + POINTER_LEN];
        stream[..MAGIC.len()].copy_from_slice(MAGIC);
        stream[VERSION_OFFSET] = version;
        stream
    }

    fn pointer(kind: u32, offset: u32, length: u32, format: u16) -> [u8; POINTER_LEN] {
        let mut bytes = [0u8; POINTER_LEN];
        bytes[0..4].copy_from_slice(&kind.to_le_bytes());
        bytes[8..12].copy_from_slice(&offset.to_le_bytes());
        bytes[12..16].copy_from_slice(&length.to_le_bytes());
        bytes[16..18].copy_from_slice(&format.to_le_bytes());
        bytes
    }

    fn chunk_header(
        kind: u32,
        id: u32,
        list: u32,
        payload: u32,
        level: u16,
        unknown: u8,
    ) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&kind.to_le_bytes());
        bytes.extend_from_slice(&id.to_le_bytes());
        bytes.extend_from_slice(&list.to_le_bytes());
        bytes.extend_from_slice(&payload.to_le_bytes());
        bytes.extend_from_slice(&level.to_le_bytes());
        bytes.push(unknown);
        assert_eq!(bytes.len(), CHUNK_HEADER_LEN);
        bytes
    }

    fn chunk(
        kind: u32,
        id: u32,
        list: u32,
        payload: &[u8],
        level: u16,
        unknown: u8,
        trailer: usize,
    ) -> Vec<u8> {
        let mut bytes = chunk_header(kind, id, list, payload.len() as u32, level, unknown);
        bytes.extend_from_slice(payload);
        bytes.resize(bytes.len() + trailer, 0);
        bytes
    }

    /// An uncompressed pointer list at table 4: the u32 at 0 names 8.
    /// `listSize` is the length of the order array (the reference reads
    /// one only when it is greater than one).
    fn list_bytes(pointers: &[[u8; POINTER_LEN]], order: &[u32]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&8u32.to_le_bytes());
        bytes.extend_from_slice(&(order.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&(pointers.len() as i32).to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        for entry in pointers {
            bytes.extend_from_slice(entry);
        }
        for entry in order {
            bytes.extend_from_slice(&entry.to_le_bytes());
        }
        bytes
    }

    /// A `VisioDocument` stream whose trailer list names `targets` in
    /// order (kind, format, region bytes), with an optional order array.
    fn build(version: u8, targets: &[(u32, u16, Vec<u8>)], order: &[u32]) -> Vec<u8> {
        let list_len = 16 + targets.len() * POINTER_LEN + order.len() * 4;
        let base = TRAILER_POINTER_OFFSET + POINTER_LEN + list_len;
        let mut offsets = Vec::new();
        let mut cursor = base;
        for (_, _, region) in targets {
            offsets.push(cursor);
            cursor += region.len();
        }
        let pointers: Vec<[u8; POINTER_LEN]> = targets
            .iter()
            .enumerate()
            .map(|(i, (kind, format, region))| {
                pointer(*kind, offsets[i] as u32, region.len() as u32, *format)
            })
            .collect();
        let mut stream = header(version);
        let trailer = pointer(
            0x14,
            (TRAILER_POINTER_OFFSET + POINTER_LEN) as u32,
            list_len as u32,
            0x50,
        );
        stream[TRAILER_POINTER_OFFSET..].copy_from_slice(&trailer);
        stream.extend_from_slice(&list_bytes(&pointers, order));
        for (_, _, region) in targets {
            stream.extend_from_slice(region);
        }
        stream
    }

    /// A `VisioDocument` stream whose trailer list is `list` verbatim.
    fn build_raw_list(version: u8, list: &[u8]) -> Vec<u8> {
        let mut stream = header(version);
        let trailer = pointer(
            0x14,
            (TRAILER_POINTER_OFFSET + POINTER_LEN) as u32,
            list.len() as u32,
            0x50,
        );
        stream[TRAILER_POINTER_OFFSET..].copy_from_slice(&trailer);
        stream.extend_from_slice(list);
        stream
    }

    /// A literal-only encoding of the format's compressed blocks: one
    /// flag byte of set bits per group of up to eight literal bytes.
    fn compress_literals(data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        for group in data.chunks(8) {
            let flags = if group.len() == 8 {
                0xff
            } else {
                (1u8 << group.len()) - 1
            };
            out.push(flags);
            out.extend_from_slice(group);
        }
        out
    }

    // --- the header ----------------------------------------------------

    #[test]
    fn from_byte_accepts_the_two_chunk_versions() {
        assert_eq!(FormatVersion::from_byte(6), Ok(FormatVersion::V6));
        assert_eq!(FormatVersion::from_byte(11), Ok(FormatVersion::V11));
    }

    #[test]
    fn from_byte_rejects_versions_out_of_scope() {
        for byte in [0, 1, 2, 3, 4, 5, 7, 8, 9, 10, 12, 255] {
            assert_eq!(
                FormatVersion::from_byte(byte),
                Err(Error::UnsupportedVersion(byte)),
                "version {byte}"
            );
        }
    }

    #[test]
    fn version_reads_the_byte_after_the_magic() {
        let mut stream = header(11);
        assert_eq!(version(&stream), Ok(FormatVersion::V11));
        stream[VERSION_OFFSET] = 6;
        assert_eq!(version(&stream), Ok(FormatVersion::V6));
        stream[VERSION_OFFSET] = 3;
        assert_eq!(version(&stream), Err(Error::UnsupportedVersion(3)));
    }

    #[test]
    fn version_rejects_a_stream_without_the_magic() {
        let mut stream = header(11);
        stream[0] = b'X';
        assert_eq!(
            version(&stream),
            Err(Error::NotAVisioFile(
                "the VisioDocument stream does not start with the Visio magic".to_owned()
            ))
        );
    }

    #[test]
    fn version_rejects_a_short_stream() {
        let short = vec![0u8; VERSION_OFFSET];
        assert_eq!(
            version(&short),
            Err(Error::Invalid(
                "VisioDocument stream is shorter than its header".to_owned()
            ))
        );
    }

    #[test]
    fn a_chunk_keeps_its_fields() {
        let chunk = Chunk {
            record: Record(0x8a),
            level: 2,
            list: 7,
            id: 1,
            data: &[1, 2],
        };
        assert_eq!(chunk.record, Record(0x8a));
        assert_eq!(chunk.level, 2);
        assert_eq!(chunk.list, 7);
        assert_eq!(chunk.id, 1);
        assert_eq!(chunk.data, &[1, 2]);
    }

    #[test]
    fn every_id_in_the_table_has_its_format_name() {
        assert_eq!(MOVE_TO.name(), "MOVE_TO");
        assert_eq!(TEXT.name(), "TEXT");
        assert_eq!(TRAILER_STREAM.name(), "TRAILER_STREAM");
        assert_eq!(FONTFACES.name(), "FONTFACES");
        assert_eq!(Record(0x1234).name(), "unknown record");
    }

    // --- the per-version payload framing -------------------------------

    #[test]
    fn the_trailer_rules_differ_between_the_versions() {
        // A connect list with no list id: 8 bytes in version 6, none in
        // version 11.
        assert_eq!(trailer_len(FormatVersion::V6, 0x72, 0, 1, 0x50), 8);
        assert_eq!(trailer_len(FormatVersion::V11, 0x72, 0, 1, 0x50), 0);
        // A list record: 8 plus the version 11 word separator.
        assert_eq!(trailer_len(FormatVersion::V6, 0x65, 3, 1, 0x50), 8);
        assert_eq!(trailer_len(FormatVersion::V11, 0x65, 3, 1, 0x50), 12);
        // The version 11 word separator also follows some level 2 and
        // level 3 records with no list id.
        assert_eq!(trailer_len(FormatVersion::V11, 0x85, 0, 2, 0x55), 4);
        assert_eq!(trailer_len(FormatVersion::V11, 0xaa, 0, 2, 0x54), 4);
        assert_eq!(trailer_len(FormatVersion::V11, 0x9b, 0, 3, 0x51), 4);
        assert_eq!(trailer_len(FormatVersion::V11, 0x9b, 0, 3, 0x50), 0);
        // OLE data and name ids never have a trailer.
        assert_eq!(trailer_len(FormatVersion::V6, 0x1f, 5, 2, 0x55), 0);
        assert_eq!(trailer_len(FormatVersion::V11, 0xc9, 0, 3, 0x51), 0);
        assert_eq!(trailer_len(FormatVersion::V11, 0xd1, 0, 2, 0x55), 0);
    }

    // --- the walk ------------------------------------------------------

    #[test]
    fn read_chunks_walks_the_chunk_records_of_a_version_11_stream() {
        let mut region = chunk(0x8a, 7, 3, &[7u8; 6], 1, 0x50, 12);
        region.extend_from_slice(&chunk(0x8b, 8, 0, &[1, 2], 2, 0x55, 4));
        let stream = build(11, &[(0x1a, 0xd0, region)], &[]);
        let mut blocks = Vec::new();
        let (chunks, warnings) =
            read_chunks(&stream, FormatVersion::V11, &Limits::DEFAULT, &mut blocks).unwrap();
        assert_eq!(warnings, Vec::new());
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].record, MOVE_TO);
        assert_eq!(chunks[0].id, 7);
        assert_eq!(chunks[0].list, 3);
        assert_eq!(chunks[0].level, 3);
        assert_eq!(chunks[0].data.len(), 18);
        assert_eq!(&chunks[0].data[..6], &[7u8; 6]);
        assert_eq!(chunks[1].record, LINE_TO);
        assert_eq!(chunks[1].id, 8);
        assert_eq!(chunks[1].list, 0);
        assert_eq!(chunks[1].level, 4);
        assert_eq!(chunks[1].data.len(), 6);
        assert_eq!(&chunks[1].data[..2], &[1, 2]);
    }

    #[test]
    fn read_chunks_uses_the_per_version_payload_framing() {
        // The same bytes: a connect list has an 8 byte trailer in
        // version 6 and none in version 11.
        let region = chunk(0x72, 1, 0, &[5, 5], 1, 0x50, 8);
        let stream = build(11, &[(0x1a, 0xd0, region)], &[]);
        let mut blocks_v6 = Vec::new();
        let (v6, _) =
            read_chunks(&stream, FormatVersion::V6, &Limits::DEFAULT, &mut blocks_v6).unwrap();
        let mut blocks_v11 = Vec::new();
        let (v11, _) = read_chunks(
            &stream,
            FormatVersion::V11,
            &Limits::DEFAULT,
            &mut blocks_v11,
        )
        .unwrap();
        assert_eq!(v6.len(), 1);
        assert_eq!(v6[0].record, CONNECT_LIST);
        assert_eq!(v6[0].data.len(), 10);
        assert_eq!(v11.len(), 1);
        assert_eq!(v11[0].data.len(), 2);
    }

    #[test]
    fn a_blob_pointer_yields_one_chunk_with_the_pointer_fields() {
        let stream = build(11, &[(0x15, 0x40, vec![3u8; 10])], &[]);
        let mut blocks = Vec::new();
        let (chunks, warnings) =
            read_chunks(&stream, FormatVersion::V11, &Limits::DEFAULT, &mut blocks).unwrap();
        assert_eq!(warnings, Vec::new());
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].record, PAGE);
        assert_eq!(chunks[0].id, 0);
        assert_eq!(chunks[0].list, 0);
        assert_eq!(chunks[0].level, 2);
        assert_eq!(chunks[0].data, &[3u8; 10]);
    }

    #[test]
    fn a_nested_pointer_list_is_walked_one_level_down() {
        // The outer list names one blob; the blob holds a nested list
        // that names a chunk stream.
        let inner = chunk(0x83, 4, 0, &[9, 9], 1, 0x50, 0);
        let list_len = 16 + POINTER_LEN;
        let blob_offset = TRAILER_POINTER_OFFSET + POINTER_LEN + list_len;
        let blob_len = list_len;
        let inner_offset = blob_offset + blob_len;
        let blob = list_bytes(
            &[pointer(0x1a, inner_offset as u32, inner.len() as u32, 0xd0)],
            &[],
        );
        assert_eq!(blob.len(), blob_len);
        let mut stream = build(11, &[(0x46, 0x50, blob)], &[]);
        stream.extend_from_slice(&inner);
        assert_eq!(stream.len(), inner_offset + inner.len());
        let mut blocks = Vec::new();
        let (chunks, warnings) =
            read_chunks(&stream, FormatVersion::V11, &Limits::DEFAULT, &mut blocks).unwrap();
        assert_eq!(warnings, Vec::new());
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].record, PAGE_SHEET);
        assert_eq!(chunks[0].level, 2);
        assert_eq!(chunks[0].data.len(), blob_len);
        assert_eq!(chunks[1].record, SHAPE_ID);
        assert_eq!(chunks[1].id, 4);
        assert_eq!(chunks[1].level, 5);
        assert_eq!(chunks[1].data, &[9, 9]);
    }

    #[test]
    fn the_order_array_decides_the_walk_order() {
        let stream = build(
            11,
            &[(0x15, 0x40, vec![1u8; 4]), (0x15, 0x40, vec![2u8; 4])],
            &[1, 0],
        );
        let mut blocks = Vec::new();
        let (chunks, _) =
            read_chunks(&stream, FormatVersion::V11, &Limits::DEFAULT, &mut blocks).unwrap();
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].id, 1);
        assert_eq!(chunks[0].data, &[2u8; 4]);
        assert_eq!(chunks[1].id, 0);
        assert_eq!(chunks[1].data, &[1u8; 4]);
    }

    #[test]
    fn the_record_cap_is_enforced_through_a_nested_list() {
        // The outer blob is record one; the nested list names a chunk
        // stream whose record trips the cap.
        let inner = chunk(0x83, 4, 0, &[9, 9], 1, 0x50, 0);
        let list_len = 16 + POINTER_LEN;
        let blob_offset = TRAILER_POINTER_OFFSET + POINTER_LEN + list_len;
        let blob_len = list_len;
        let inner_offset = blob_offset + blob_len;
        let blob = list_bytes(
            &[pointer(0x1a, inner_offset as u32, inner.len() as u32, 0xd0)],
            &[],
        );
        let mut stream = build(11, &[(0x46, 0x50, blob)], &[]);
        stream.extend_from_slice(&inner);
        let limits = Limits {
            max_chunks: 1,
            ..Limits::DEFAULT
        };
        let mut blocks = Vec::new();
        let err = read_chunks(&stream, FormatVersion::V11, &limits, &mut blocks).unwrap_err();
        assert_eq!(
            err,
            Error::LimitExceeded {
                limit: "max_chunks",
                value: 2,
                max: 1
            }
        );
    }

    #[test]
    fn a_pointer_cycle_terminates_and_warns() {
        // One blob whose nested list points back at the blob itself.
        let list_len = 16 + POINTER_LEN;
        let blob_offset = TRAILER_POINTER_OFFSET + POINTER_LEN + list_len;
        let blob = list_bytes(
            &[pointer(0x46, blob_offset as u32, list_len as u32, 0x51)],
            &[],
        );
        let stream = build(11, &[(0x46, 0x51, blob)], &[]);
        let mut blocks = Vec::new();
        let (chunks, warnings) =
            read_chunks(&stream, FormatVersion::V11, &Limits::DEFAULT, &mut blocks).unwrap();
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].record, PAGE_SHEET);
        assert!(warnings.iter().any(|w| w.kind
            == WarningKind::Dropped {
                what: "repeated pointer target"
            }));
    }

    // --- degradation ---------------------------------------------------

    #[test]
    fn a_record_whose_length_runs_past_the_stream_is_skipped() {
        // The first record lies about its payload length; the walk
        // recovers at the next pointer target.
        let mut lying = chunk_header(0x8a, 1, 0, 100, 1, 0x50);
        lying.extend_from_slice(&[1, 2, 3, 4]);
        let good = chunk(0x8b, 2, 0, &[2, 2], 1, 0x50, 0);
        let stream = build(11, &[(0x1a, 0xd0, lying), (0x1a, 0xd0, good)], &[]);
        let mut blocks = Vec::new();
        let (chunks, warnings) =
            read_chunks(&stream, FormatVersion::V11, &Limits::DEFAULT, &mut blocks).unwrap();
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].record, LINE_TO);
        assert!(warnings.iter().any(|w| w.kind
            == WarningKind::BadValue {
                what: "chunk record length"
            }));
    }

    #[test]
    fn a_truncated_header_ends_its_stream_with_a_warning() {
        let short = vec![0x8au8, 0, 0, 0, 1, 0, 0, 0];
        let good = chunk(0x8b, 2, 0, &[2, 2], 1, 0x50, 0);
        let stream = build(11, &[(0x1a, 0xd0, short), (0x1a, 0xd0, good)], &[]);
        let mut blocks = Vec::new();
        let (chunks, warnings) =
            read_chunks(&stream, FormatVersion::V11, &Limits::DEFAULT, &mut blocks).unwrap();
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].id, 2);
        assert!(warnings.iter().any(|w| w.kind
            == WarningKind::SkippedRecord {
                name: "truncated chunk record"
            }));
    }

    #[test]
    fn nothing_readable_is_an_error() {
        let short = vec![0x8au8, 0, 0, 0, 1, 0, 0, 0];
        let stream = build(11, &[(0x1a, 0xd0, short)], &[]);
        let mut blocks = Vec::new();
        let err =
            read_chunks(&stream, FormatVersion::V11, &Limits::DEFAULT, &mut blocks).unwrap_err();
        assert_eq!(
            err,
            Error::Invalid(
                "no chunk record could be read from the VisioDocument stream".to_owned()
            )
        );
    }

    #[test]
    fn padding_between_records_is_skipped() {
        let mut region = chunk(0x8a, 1, 0, &[1, 1], 1, 0x50, 0);
        region.extend_from_slice(&[0u8; 5]);
        region.extend_from_slice(&chunk(0x8b, 2, 0, &[2, 2], 1, 0x50, 0));
        let stream = build(11, &[(0x1a, 0xd0, region)], &[]);
        let mut blocks = Vec::new();
        let (chunks, warnings) =
            read_chunks(&stream, FormatVersion::V11, &Limits::DEFAULT, &mut blocks).unwrap();
        assert_eq!(warnings, Vec::new());
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[1].record, LINE_TO);
    }

    #[test]
    fn a_pointer_out_of_the_stream_is_skipped_with_a_warning() {
        let stream = build(
            11,
            &[(0x15, 0x40, vec![3u8; 4]), (0x15, 0x40, vec![4u8; 4])],
            &[],
        );
        let mut stream = stream;
        // The first pointer's offset field: beyond the stream.
        let offset_field = TRAILER_POINTER_OFFSET + POINTER_LEN + 16 + 8;
        stream[offset_field..offset_field + 4].copy_from_slice(&0xf000_0000u32.to_le_bytes());
        let mut blocks = Vec::new();
        let (chunks, warnings) =
            read_chunks(&stream, FormatVersion::V11, &Limits::DEFAULT, &mut blocks).unwrap();
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].data, &[4u8; 4]);
        assert!(warnings.iter().any(|w| w.kind
            == WarningKind::BadValue {
                what: "pointer range"
            }));
    }

    #[test]
    fn a_pointer_format_out_of_scope_is_unsupported() {
        let stream = build(
            11,
            &[(0x15, 0x70, vec![1u8; 4]), (0x15, 0x40, vec![3u8; 4])],
            &[],
        );
        let mut blocks = Vec::new();
        let (chunks, warnings) =
            read_chunks(&stream, FormatVersion::V11, &Limits::DEFAULT, &mut blocks).unwrap();
        assert_eq!(chunks.len(), 1);
        assert!(warnings.iter().any(|w| w.kind
            == WarningKind::Unsupported {
                what: "pointer format"
            }));
    }

    #[test]
    fn a_pointer_list_with_a_truncated_array_is_read_as_far_as_it_goes() {
        // Two declared pointers, but the list ends inside the second
        // one: the readable first pointer still walks.
        let region_offset = TRAILER_POINTER_OFFSET + POINTER_LEN + 16 + POINTER_LEN + 6;
        let mut list = Vec::new();
        list.extend_from_slice(&8u32.to_le_bytes());
        list.extend_from_slice(&0u32.to_le_bytes());
        list.extend_from_slice(&2i32.to_le_bytes());
        list.extend_from_slice(&0u32.to_le_bytes());
        list.extend_from_slice(&pointer(0x15, region_offset as u32, 4, 0x40));
        list.extend_from_slice(&[0u8; 6]);
        let mut stream = build_raw_list(11, &list);
        stream.extend_from_slice(&[3u8; 4]);
        let mut blocks = Vec::new();
        let (chunks, warnings) =
            read_chunks(&stream, FormatVersion::V11, &Limits::DEFAULT, &mut blocks).unwrap();
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].data, &[3u8; 4]);
        assert!(warnings.iter().any(|w| w.kind
            == WarningKind::BadValue {
                what: "pointer list"
            }));
    }

    // --- list records and their child ids -------------------------------

    /// Reads a child-id list the way the domain modules do: a sub-header
    /// length, a byte count, the sub-header, then the ids.
    fn child_ids(data: &[u8]) -> Option<Vec<u32>> {
        let mut fields = data;
        let sub_header = u32::from_le_bytes(fields.get(0..4)?.try_into().ok()?) as usize;
        let bytes = u32::from_le_bytes(fields.get(4..8)?.try_into().ok()?) as usize;
        fields = fields.get(8 + sub_header..)?;
        let mut ids = Vec::new();
        for _ in 0..bytes / 4 {
            ids.push(u32::from_le_bytes(fields.get(0..4)?.try_into().ok()?));
            fields = fields.get(4..)?;
        }
        Some(ids)
    }

    #[test]
    fn a_list_records_child_ids_are_kept_in_the_record() {
        // The corpus shape of a geometry list of six rows: the sub-header
        // is 24 bytes, four ids sit in the declared payload (48 bytes)
        // and two in the trailing region (12 bytes).
        let mut payload = Vec::new();
        payload.extend_from_slice(&24u32.to_le_bytes());
        payload.extend_from_slice(&24u32.to_le_bytes());
        payload.extend_from_slice(&[0u8; 24]);
        for id in [10u32, 11, 12, 13] {
            payload.extend_from_slice(&id.to_le_bytes());
        }
        assert_eq!(payload.len(), 48);
        let mut record = chunk_header(0x6c, 4, 6, 48, 2, 0x55);
        record.extend_from_slice(&payload);
        for id in [14u32, 15] {
            record.extend_from_slice(&id.to_le_bytes());
        }
        record.extend_from_slice(&[0u8; 4]);
        let stream = build(11, &[(0x1a, 0xd0, record)], &[]);
        let mut blocks = Vec::new();
        let (chunks, warnings) =
            read_chunks(&stream, FormatVersion::V11, &Limits::DEFAULT, &mut blocks).unwrap();
        assert_eq!(warnings, Vec::new());
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].record, GEOM_LIST);
        assert_eq!(chunks[0].data.len(), 60);
        assert_eq!(
            child_ids(chunks[0].data),
            Some(vec![10, 11, 12, 13, 14, 15])
        );
    }

    #[test]
    fn a_child_id_list_past_the_trailer_grows_the_record() {
        // A geometry list whose declared payload ends with its
        // sub-header: the six ids follow it, past the framing rules.
        let mut payload = Vec::new();
        payload.extend_from_slice(&24u32.to_le_bytes());
        payload.extend_from_slice(&24u32.to_le_bytes());
        payload.extend_from_slice(&[0u8; 24]);
        assert_eq!(payload.len(), 32);
        let mut record = chunk_header(0x6c, 4, 0, 32, 1, 0x50);
        record.extend_from_slice(&payload);
        for id in [20u32, 21, 22, 23, 24, 25] {
            record.extend_from_slice(&id.to_le_bytes());
        }
        let stream = build(11, &[(0x1a, 0xd0, record)], &[]);
        let mut blocks = Vec::new();
        let (chunks, warnings) =
            read_chunks(&stream, FormatVersion::V11, &Limits::DEFAULT, &mut blocks).unwrap();
        assert_eq!(warnings, Vec::new());
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].data.len(), 56);
        assert_eq!(
            child_ids(chunks[0].data),
            Some(vec![20, 21, 22, 23, 24, 25])
        );
    }

    #[test]
    fn a_list_record_with_a_nonsense_header_keeps_the_framing_rules() {
        // The list header names an impossible sub-header length: the
        // record keeps its declared frame and the walk continues.
        let mut payload = Vec::new();
        payload.extend_from_slice(&u32::MAX.to_le_bytes());
        payload.extend_from_slice(&24u32.to_le_bytes());
        payload.extend_from_slice(&[0u8; 24]);
        assert_eq!(payload.len(), 32);
        let mut region = chunk_header(0x6c, 4, 0, 32, 1, 0x50);
        region.extend_from_slice(&payload);
        region.extend_from_slice(&chunk(0x8b, 2, 0, &[2, 2], 1, 0x50, 0));
        let stream = build(11, &[(0x1a, 0xd0, region)], &[]);
        let mut blocks = Vec::new();
        let (chunks, warnings) =
            read_chunks(&stream, FormatVersion::V11, &Limits::DEFAULT, &mut blocks).unwrap();
        assert_eq!(warnings, Vec::new());
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].record, GEOM_LIST);
        assert_eq!(chunks[0].data.len(), 32);
        assert_eq!(chunks[1].record, LINE_TO);
    }

    // --- the limits ----------------------------------------------------

    #[test]
    fn a_record_count_over_the_cap_is_an_error() {
        let mut region = chunk(0x8a, 1, 0, &[1, 1], 1, 0x50, 0);
        region.extend_from_slice(&chunk(0x8b, 2, 0, &[2, 2], 1, 0x50, 0));
        let stream = build(11, &[(0x1a, 0xd0, region)], &[]);
        let limits = Limits {
            max_chunks: 1,
            ..Limits::DEFAULT
        };
        let mut blocks = Vec::new();
        let err = read_chunks(&stream, FormatVersion::V11, &limits, &mut blocks).unwrap_err();
        assert_eq!(
            err,
            Error::LimitExceeded {
                limit: "max_chunks",
                value: 2,
                max: 1
            }
        );
    }

    #[test]
    fn a_level_over_the_cap_is_an_error() {
        let region = chunk(0x8a, 1, 0, &[1, 1], 3, 0x50, 0);
        let stream = build(11, &[(0x1a, 0xd0, region)], &[]);
        let limits = Limits {
            max_chunk_level: 4,
            ..Limits::DEFAULT
        };
        let mut blocks = Vec::new();
        let err = read_chunks(&stream, FormatVersion::V11, &limits, &mut blocks).unwrap_err();
        assert_eq!(
            err,
            Error::LimitExceeded {
                limit: "max_chunk_level",
                value: 5,
                max: 4
            }
        );
    }

    #[test]
    fn every_id_in_the_table_walks_its_name_arm() {
        for record in [
            FOREIGN_DATA,
            OLE_LIST,
            TEXT,
            TRAILER_STREAM,
            PAGE,
            COLORS,
            FONT_LIST,
            FONT_IX,
            STYLES,
            STENCILS,
            STENCIL_PAGE,
            OLE_DATA,
            PAGES,
            NAME_LIST,
            NAME,
            NAME_LIST2,
            NAME2,
            NAMEIDX123,
            PAGE_SHEET,
            SHAPE_GROUP,
            SHAPE_SHAPE,
            STYLE_SHEET,
            SHAPE_GUIDE,
            SHAPE_FOREIGN,
            SCRATCH_LIST,
            SHAPE_LIST,
            FIELD_LIST,
            PROP_LIST,
            CHAR_LIST,
            PARA_LIST,
            TABS_DATA_LIST,
            GEOM_LIST,
            CUST_PROPS_LIST,
            ACT_ID_LIST,
            LAYER_LIST,
            CTRL_LIST,
            C_PNTS_LIST,
            CONNECT_LIST,
            HYPER_LNK_LIST,
            SMART_TAG_LIST,
            SHAPE_ID,
            EVENT,
            LINE,
            FILL_AND_SHADOW,
            TEXT_BLOCK,
            TABS_DATA_1,
            GEOMETRY,
            MOVE_TO,
            LINE_TO,
            ARC_TO,
            INFINITE_LINE,
            ELLIPSE,
            ELLIPTICAL_ARC_TO,
            PAGE_PROPS,
            STYLE_PROPS,
            CHAR_IX,
            PARA_IX,
            TABS_DATA_2,
            TABS_DATA_3,
            FOREIGN_DATA_TYPE,
            CONNECTION_POINTS,
            XFORM_DATA,
            TEXT_XFORM,
            XFORM_1D,
            SCRATCH,
            PROTECTION,
            TEXT_FIELD,
            CONTROL_ANOTHER_TYPE,
            MISC,
            SPLINE_START,
            SPLINE_KNOT,
            LAYER_MEMBERSHIP,
            LAYER,
            ACT_ID,
            CONTROL,
            USER_DEFINED_CELLS,
            TABS_DATA_4,
            CUSTOM_PROPS,
            RULER_GRID,
            CONNECTION_POINTS_ANOTHER_TYPE,
            DOC_PROPS,
            IMAGE,
            GROUP,
            LAYOUT,
            PAGE_LAYOUT_IX,
            POLYLINE_TO,
            NURBS_TO,
            HYPERLINK,
            REVIEWER,
            ANNOTATION,
            SMART_TAG_DEF,
            PRINT_PROPS,
            NAMEIDX,
            SHAPE_DATA,
            FONTFACE,
            FONTFACES,
        ] {
            assert_ne!(record.name(), "unknown record", "id {:#x}", record.0);
        }
    }

    #[test]
    fn a_record_with_a_wide_type_id_is_skipped_and_the_walk_continues() {
        let mut region = chunk_header(0x1_008a, 1, 0, 2, 1, 0x50);
        region.extend_from_slice(&[1, 1]);
        region.extend_from_slice(&chunk(0x8b, 2, 0, &[2, 2], 1, 0x50, 0));
        let stream = build(11, &[(0x1a, 0xd0, region)], &[]);
        let mut blocks = Vec::new();
        let (chunks, warnings) =
            read_chunks(&stream, FormatVersion::V11, &Limits::DEFAULT, &mut blocks).unwrap();
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].record, LINE_TO);
        assert!(warnings.iter().any(|w| w.kind
            == WarningKind::SkippedRecord {
                name: "chunk record with a wide type id"
            }));
    }

    #[test]
    fn a_blob_with_a_wide_type_id_is_skipped() {
        let stream = build(
            11,
            &[(0x1_0000, 0x40, vec![1u8; 4]), (0x15, 0x40, vec![3u8; 4])],
            &[],
        );
        let mut blocks = Vec::new();
        let (chunks, warnings) =
            read_chunks(&stream, FormatVersion::V11, &Limits::DEFAULT, &mut blocks).unwrap();
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].record, PAGE);
        assert!(warnings.iter().any(|w| w.kind
            == WarningKind::SkippedRecord {
                name: "blob record with a wide type id"
            }));
    }

    #[test]
    fn a_pointer_of_kind_zero_is_skipped() {
        let stream = build(
            11,
            &[(0, 0x40, vec![1u8; 4]), (0x15, 0x40, vec![3u8; 4])],
            &[],
        );
        let mut blocks = Vec::new();
        let (chunks, warnings) =
            read_chunks(&stream, FormatVersion::V11, &Limits::DEFAULT, &mut blocks).unwrap();
        assert_eq!(warnings, Vec::new());
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].data, &[3u8; 4]);
    }

    #[test]
    fn a_pointer_list_without_its_offset_is_skipped() {
        let (list, warnings) = parse_pointer_list(&[0u8; 2], 0, "test");
        assert_eq!(list.entries, Vec::new());
        assert_eq!(warnings.len(), 1);
        assert_eq!(
            warnings[0].kind,
            WarningKind::BadValue {
                what: "pointer list offset"
            }
        );
    }

    #[test]
    fn a_pointer_list_beyond_its_buffer_is_skipped() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&8u32.to_le_bytes());
        let (list, warnings) = parse_pointer_list(&buf, 0, "test");
        assert_eq!(list.entries, Vec::new());
        assert_eq!(
            warnings[0].kind,
            WarningKind::BadValue {
                what: "pointer list offset"
            }
        );
        // The same list with a table position before the buffer.
        buf.clear();
        buf.extend_from_slice(&0u32.to_le_bytes());
        let (list, warnings) = parse_pointer_list(&buf, 0, "test");
        assert_eq!(list.entries, Vec::new());
        assert_eq!(
            warnings[0].kind,
            WarningKind::BadValue {
                what: "pointer list offset"
            }
        );
    }

    #[test]
    fn a_negative_pointer_count_is_skipped() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&8u32.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&(-1i32).to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());
        let (list, warnings) = parse_pointer_list(&buf, 0, "test");
        assert_eq!(list.entries, Vec::new());
        assert_eq!(
            warnings[0].kind,
            WarningKind::BadValue {
                what: "pointer count"
            }
        );
    }

    #[test]
    fn a_truncated_order_array_is_read_as_far_as_it_goes() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&8u32.to_le_bytes());
        buf.extend_from_slice(&2u32.to_le_bytes());
        buf.extend_from_slice(&1i32.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&pointer(0x15, 0, 4, 0x40));
        buf.extend_from_slice(&1u32.to_le_bytes());
        let (list, warnings) = parse_pointer_list(&buf, 0, "test");
        assert_eq!(list.entries.len(), 1);
        assert_eq!(list.order, vec![1]);
        assert_eq!(
            warnings[0].kind,
            WarningKind::BadValue {
                what: "pointer order list"
            }
        );
    }

    #[test]
    fn deep_pointer_nesting_is_dropped_with_a_warning() {
        // Each level is one blob whose nested list names the next blob;
        // the recursion bound (AGENTS.md section 6) drops the rest.
        let mut limits = Limits::DEFAULT;
        limits.max_chunk_level = 1000;
        let depth = MAX_WALK_DEPTH + 2;
        let list_len = 16 + POINTER_LEN;
        // The regions are laid out from the innermost out: region i
        // starts after the outer list and all outer regions.
        let base = TRAILER_POINTER_OFFSET + POINTER_LEN + list_len;
        let mut stream = header(11);
        let trailer = pointer(
            0x14,
            (TRAILER_POINTER_OFFSET + POINTER_LEN) as u32,
            list_len as u32,
            0x50,
        );
        stream[TRAILER_POINTER_OFFSET..].copy_from_slice(&trailer);
        let mut regions: Vec<Vec<u8>> = Vec::new();
        for level in 0..depth {
            let next_offset = base + (level + 1) * list_len;
            let kind = if level + 1 == depth { 0x15 } else { 0x46 };
            regions.push(list_bytes(
                &[pointer(kind, next_offset as u32, list_len as u32, 0x51)],
                &[],
            ));
        }
        let first = list_bytes(&[pointer(0x46, base as u32, list_len as u32, 0x51)], &[]);
        stream.extend_from_slice(&first);
        for region in regions {
            stream.extend_from_slice(&region);
        }
        let mut blocks = Vec::new();
        let (chunks, warnings) = read_chunks(&stream, FormatVersion::V11, &limits, &mut blocks)
            .expect("a deep but bounded walk succeeds");
        assert_eq!(chunks.len(), MAX_WALK_DEPTH);
        assert!(warnings.iter().any(|w| w.kind
            == WarningKind::Dropped {
                what: "deeply nested pointer list"
            }));
    }

    #[test]
    fn the_integer_readers_report_a_truncated_buffer() {
        assert_eq!(read_u32(&[0, 0, 0], 0), None);
        assert_eq!(read_u32(&[0, 0, 0, 0], 1), None);
        assert_eq!(read_i32(&[0, 0, 0], 0), None);
        assert_eq!(read_u16(&[0], 0), None);
        assert_eq!(read_pointer(&[0u8; 17], 0), None);
        assert_eq!(read_pointer(&[0u8; 18], 1), None);
    }

    #[test]
    fn the_trailer_rules_cover_every_branch() {
        // Version 6: any list record or list kind gets 8 bytes.
        assert_eq!(trailer_len(FormatVersion::V6, 0x65, 0, 1, 0x50), 8);
        assert_eq!(trailer_len(FormatVersion::V6, 0x0d, 0, 1, 0x50), 8);
        assert_eq!(trailer_len(FormatVersion::V6, 0x2c, 0, 1, 0x50), 8);
        assert_eq!(trailer_len(FormatVersion::V6, 0x8a, 0, 1, 0x50), 0);
        // Version 11: the first rule knows fewer kinds, and the third
        // rule adds the separator to the list kinds it shares.
        assert_eq!(trailer_len(FormatVersion::V11, 0x70, 0, 1, 0x50), 8);
        assert_eq!(trailer_len(FormatVersion::V11, 0x65, 0, 1, 0x50), 12);
        assert_eq!(trailer_len(FormatVersion::V11, 0x72, 0, 1, 0x50), 0);
        // The word separator is skipped when it is already modelled.
        assert_eq!(trailer_len(FormatVersion::V11, 0x92, 0, 2, 0x55), 4);
        assert_eq!(trailer_len(FormatVersion::V11, 0x65, 1, 1, 0x50), 12);
        assert_eq!(trailer_len(FormatVersion::V11, 0x64, 0, 1, 0x50), 4);
        // A level 3 record keeps the separator except for 0x50 and 0x54.
        assert_eq!(trailer_len(FormatVersion::V11, 0x8a, 0, 3, 0x50), 0);
        assert_eq!(trailer_len(FormatVersion::V11, 0x8a, 0, 3, 0x54), 0);
        // The name and OLE ids lose any trailer.
        assert_eq!(trailer_len(FormatVersion::V11, 0x2d, 2, 2, 0x55), 0);
    }

    #[test]
    fn the_blob_record_count_is_capped_too() {
        let stream = build(
            11,
            &[(0x15, 0x40, vec![1u8; 4]), (0x15, 0x40, vec![2u8; 4])],
            &[],
        );
        let limits = Limits {
            max_chunks: 1,
            ..Limits::DEFAULT
        };
        let mut blocks = Vec::new();
        let err = read_chunks(&stream, FormatVersion::V11, &limits, &mut blocks).unwrap_err();
        assert_eq!(
            err,
            Error::LimitExceeded {
                limit: "max_chunks",
                value: 2,
                max: 1
            }
        );
    }

    #[test]
    fn a_trailer_pointer_out_of_the_stream_is_an_error() {
        let mut stream = header(11);
        let trailer = pointer(0x14, 0xf000_0000, 32, 0x50);
        stream[TRAILER_POINTER_OFFSET..].copy_from_slice(&trailer);
        let mut blocks = Vec::new();
        let err =
            read_chunks(&stream, FormatVersion::V11, &Limits::DEFAULT, &mut blocks).unwrap_err();
        assert_eq!(
            err,
            Error::Invalid(
                "no chunk record could be read from the VisioDocument stream".to_owned()
            )
        );
    }

    #[test]
    fn a_datum_resolves_through_its_buffer() {
        let stream = [1u8; 4];
        let store = vec![vec![5u8; 8]];
        let in_block = Datum {
            base: Base::Block(0),
            start: 2,
            len: 3,
        };
        assert_eq!(resolve(in_block, &stream, &store), &[5, 5, 5]);
        let in_stream = Datum {
            base: Base::Stream,
            start: 1,
            len: 2,
        };
        assert_eq!(resolve(in_stream, &stream, &store), &[1, 1]);
        // A datum out of its buffer resolves to nothing, never a panic.
        let missing = Datum {
            base: Base::Block(7),
            start: 0,
            len: 2,
        };
        assert_eq!(resolve(missing, &stream, &store), &[]);
        let over = Datum {
            base: Base::Stream,
            start: 3,
            len: 9,
        };
        assert_eq!(resolve(over, &stream, &store), &[]);
    }

    #[test]
    fn region_bytes_read_a_decompressed_block() {
        let mut blocks = vec![vec![7u8; 4]];
        let walker = Walker {
            stream: &[0u8; 2],
            version: FormatVersion::V11,
            limits: &Limits::DEFAULT,
            blocks: &mut blocks,
            pending: Vec::new(),
            warnings: Vec::new(),
            visited: BTreeSet::new(),
        };
        let region = Region {
            source: Datum {
                base: Base::Block(0),
                start: 1,
                len: 2,
            },
            shift: 4,
            stream_offset: 0,
        };
        assert_eq!(walker.region_bytes(&region), &[7, 7]);
    }

    #[test]
    fn a_blob_level_over_the_cap_is_an_error() {
        let stream = build(11, &[(0x15, 0x40, vec![3u8; 4])], &[]);
        let limits = Limits {
            max_chunk_level: 1,
            ..Limits::DEFAULT
        };
        let mut blocks = Vec::new();
        let err = read_chunks(&stream, FormatVersion::V11, &limits, &mut blocks).unwrap_err();
        assert_eq!(
            err,
            Error::LimitExceeded {
                limit: "max_chunk_level",
                value: 2,
                max: 1
            }
        );
    }

    // --- compressed blocks (the ADR 0068 seam) -------------------------

    #[test]
    fn a_compressed_region_is_restored_through_the_decompress_seam() {
        // A compressed blob whose decompressed content is a four byte
        // prefix plus twelve payload bytes (the format's shift): two
        // full literal groups.
        let mut content = vec![0u8; 4];
        content.extend_from_slice(&[9u8; 12]);
        let region = compress_literals(&content);
        let stream = build(11, &[(0x15, 0x42, region)], &[]);
        let mut blocks = Vec::new();
        // A blob that restores to less than the format's shift has no
        // payload and is dropped with a warning.
        let tiny_stream = build(
            11,
            &[
                (0x15, 0x42, compress_literals(&[6, 6])),
                (0x15, 0x40, vec![3u8; 4]),
            ],
            &[],
        );
        let mut tiny_blocks = Vec::new();
        let tiny_result = read_chunks(
            &tiny_stream,
            FormatVersion::V11,
            &Limits::DEFAULT,
            &mut tiny_blocks,
        );
        let (chunks, warnings) =
            read_chunks(&stream, FormatVersion::V11, &Limits::DEFAULT, &mut blocks)
                .expect("a restored block walks");
        assert_eq!(warnings, Vec::new());
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].record, PAGE);
        assert_eq!(chunks[0].data, &[9u8; 12]);
        assert_eq!(blocks.len(), 1);
        let (tiny_chunks, tiny_warnings) = tiny_result.expect("a tiny restored block degrades");
        assert_eq!(tiny_chunks.len(), 1);
        assert!(tiny_warnings.iter().any(|w| w.kind
            == WarningKind::BadValue {
                what: "pointer range"
            }));
    }

    #[test]
    fn a_compression_bomb_is_a_limit_error() {
        // One literal byte, then back-references that copy it forever:
        // the block expands far past the ratio cap of ADR 0068, and a
        // cap is an error, not a degradation.
        let mut group = vec![0x00u8];
        for _ in 0..8 {
            group.extend_from_slice(&[0xee, 0xff]);
        }
        let mut bomb = vec![0x01u8, 0xaa];
        bomb.extend(group.iter().copied().cycle().take(64 * group.len()));
        let stream = build(11, &[(0x15, 0x42, bomb)], &[]);
        let limits = Limits {
            max_compression_ratio: 2,
            ..Limits::DEFAULT
        };
        let mut blocks = Vec::new();
        let err = read_chunks(&stream, FormatVersion::V11, &limits, &mut blocks).unwrap_err();
        assert!(matches!(err, Error::LimitExceeded { .. }), "{err:?}");
    }

    #[test]
    fn a_list_header_shorter_than_its_own_keeps_the_framing_rules() {
        // A payload too short to hold the list header cannot be one,
        // and a payload that cuts the header in half cannot be one
        // either: both keep the framing rules. The record ends its
        // region, so the header reads nothing beyond it.
        for (payload, size) in [(&[1u8, 2][..], 2usize), (&[24, 0, 0, 0, 5, 6][..], 6)] {
            let mut record = chunk_header(0x6c, 4, 0, size as u32, 1, 0x50);
            record.extend_from_slice(payload);
            let mut trailing = chunk(0x8b, 2, 0, &[2, 2], 1, 0x50, 0);
            trailing.truncate(CHUNK_HEADER_LEN);
            let stream = build(11, &[(0x1a, 0xd0, record), (0x1a, 0xd0, trailing)], &[]);
            let mut blocks = Vec::new();
            let (chunks, warnings) =
                read_chunks(&stream, FormatVersion::V11, &Limits::DEFAULT, &mut blocks).unwrap();
            assert_eq!(warnings.len(), 1);
            assert_eq!(chunks.len(), 1);
            assert_eq!(chunks[0].record, GEOM_LIST);
            assert_eq!(chunks[0].data.len(), size);
        }
    }

    #[test]
    fn a_child_id_list_beyond_the_region_keeps_the_framing_rules() {
        // The list header names six ids but the region holds none: the
        // record keeps its declared frame.
        let mut payload = Vec::new();
        payload.extend_from_slice(&24u32.to_le_bytes());
        payload.extend_from_slice(&24u32.to_le_bytes());
        payload.extend_from_slice(&[0u8; 24]);
        assert_eq!(payload.len(), 32);
        let record = chunk_header(0x6c, 4, 0, 32, 1, 0x50)
            .into_iter()
            .chain(payload)
            .collect::<Vec<u8>>();
        let stream = build(11, &[(0x1a, 0xd0, record)], &[]);
        let mut blocks = Vec::new();
        let (chunks, warnings) =
            read_chunks(&stream, FormatVersion::V11, &Limits::DEFAULT, &mut blocks).unwrap();
        assert_eq!(warnings, Vec::new());
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].data.len(), 32);
    }

    #[test]
    fn a_short_stream_is_an_error() {
        let mut blocks = Vec::new();
        let err =
            read_chunks(&[0u8; 8], FormatVersion::V11, &Limits::DEFAULT, &mut blocks).unwrap_err();
        assert_eq!(
            err,
            Error::Invalid("VisioDocument stream is shorter than its trailer pointer".to_owned())
        );
    }

    // --- the corpus ----------------------------------------------------

    /// The corpus smoke test: every `.vsd` in `corpus/vsd/` yields a
    /// non-empty chunk list through the container seams of ADR 0068.
    /// Without a corpus directory there is nothing to walk and the test
    /// passes silently (AGENTS.md section 9).
    #[test]
    fn every_corpus_vsd_yields_a_non_empty_chunk_list() -> Result<(), Box<dyn std::error::Error>> {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus/vsd");
        let mut files: Vec<PathBuf> = fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .filter_map(std::result::Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "vsd"))
            .collect();
        files.sort();
        for path in files {
            let bytes = fs::read(&path)?;
            let streams = crate::container::read_streams(&bytes, &Limits::DEFAULT)?;
            let document = streams
                .visio_document()
                .expect("a corpus .vsd carries its VisioDocument stream");
            let file_version = version(document)?;
            let mut blocks = Vec::new();
            let (chunks, _) = read_chunks(document, file_version, &Limits::DEFAULT, &mut blocks)?;
            assert!(!chunks.is_empty(), "{path:?} yields no chunks");
        }
        Ok(())
    }
}
