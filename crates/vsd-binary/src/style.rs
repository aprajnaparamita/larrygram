// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Styles, colours, fills and lines: chunk records into the model
//! (ADR 0070).
//!
//! This module reads the stylesheet records (style sheets and their
//! line, fill and shadow, text block, character and paragraph cells),
//! the colour table and the font list, and maps them onto
//! [`larrygram_model::Stylesheet`], [`larrygram_model::Style`] and
//! [`larrygram_model::ColorTableEntry`]. A style keeps its Visio id,
//! name and its line, fill and text style references, so shapes and
//! other styles resolve the same way the `.vsdx` reader leaves them
//! (ADR 0013: inheritance is never flattened on load).
//!
//! # Record layouts
//!
//! The payload offsets follow the libvisio reference (AGENTS.md section 5,
//! commit pinned in `NOTICE`): `StyleSheet` (`0x4a`), `Line` (`0x85`),
//! `FillAndShadow` (`0x86`), `TextBlock` (`0x87`), `CharIX` (`0x94`) and
//! `ParaIX` (`0x95`) inside the styles stream (`0x1a`). The style stream is
//! delimited by its record's nesting level: everything between it and the
//! next stream record belongs to the stylesheet. List records (`0x68`,
//! `0x69`, `0x6a`, ...) only group the style-property records and are
//! skipped.
//!
//! The payload layouts differ between file versions 6 and 11 only after the
//! fields this module maps: a version 6 fill record ends after its shadow
//! pattern (no shadow offsets), and its paragraph record has no bullet-font
//! block. Optional fields are read only when the payload is long enough, so
//! both versions map without a version argument on [`crate::Chunk`].
//!
//! # Colours and the unit rule
//!
//! `FillAndShadow` and the colour table carry a palette index and an RGBA
//! value side by side. When the index is non-zero, or the RGBA value is
//! empty, the cell keeps the index as a number exactly as the `.vsdx` reader
//! does for an indexed colour; the model's lookup resolves it through the
//! palette and the colour table. A direct RGBA value with index zero becomes
//! [`larrygram_model::CellValue::Color`]. Lengths are stored in inches
//! (AGENTS.md section 6, ADR 0070); the binary records carry no unit code,
//! so the mapped cells use the model's default unitless spelling.
//!
//! Font records are dropped with a [`WarningKind::Dropped`] warning because
//! the document font list has no model place (ADR 0070). Font names on
//! character rows are text's concern (P8-05), not this module's.
//!
//! Task P8-01 defined the entry point; task P8-06 implements it.

use std::collections::HashMap;

use larrygram_model::{
    Cell, CellName, CellValue, Color, ColorTableEntry, Document, Row, Section, SectionName, Sheet,
    Style, StyleId,
};

use crate::chunks::{Chunk, Record};
use crate::error::Error;
use crate::warning::{ImportWarning, WarningKind};

/// The styles stream record.
const STYLES_STREAM: Record = Record(0x1a);
/// One named style row.
const STYLE_SHEET: Record = Record(0x4a);
/// The style's line properties.
const LINE: Record = Record(0x85);
/// The style's fill and shadow properties.
const FILL_AND_SHADOW: Record = Record(0x86);
/// The style's text block properties.
const TEXT_BLOCK: Record = Record(0x87);
/// One character run row.
const CHAR_IX: Record = Record(0x94);
/// One paragraph row.
const PARA_IX: Record = Record(0x95);
/// The document colour table.
const COLOURS: Record = Record(0x16);
/// The version 6 font list header.
const FONT_LIST: Record = Record(0x18);
/// A version 6 font name.
const FONT_IX: Record = Record(0x19);
/// A version 11 font name.
const FONTFACE: Record = Record(0xd7);
/// The version 11 font list.
const FONTFACES: Record = Record(0xd8);
/// A version 11 UTF-16 name.
const NAME: Record = Record(0x2d);
/// A version 11 UTF-16 name in the document name list.
const NAME2: Record = Record(0x33);
/// The map from name ids to style (and page) element ids.
const NAME_IDX: Record = Record(0xc9);
/// The other name-id map spelling.
const NAME_IDX_123: Record = Record(0x34);

/// The highest `LinePattern` index the model can spell (used by the scene
/// builder; 0 is no line).
const MAX_LINE_PATTERN: u8 = 23;
/// The highest `FillPattern` index the model can spell (25..40 are the
/// gradient patterns of ADR 0022).
const MAX_FILL_PATTERN: u8 = 40;
/// The highest `ShdwPattern` index the model can spell.
const MAX_SHADOW_PATTERN: u8 = 40;

/// Maps every stylesheet, colour and font record in `chunks` into
/// `document`.
///
///
/// Styles become rows of the document stylesheet, colours fill the
/// document colour table, and font records are dropped with a warning
/// where the model has no place for them. Values the model cannot
/// represent are recorded in `warnings`, never fatal.
#[allow(dead_code)] // no caller until P8-07 wires the pipeline
pub fn collect(
    chunks: &[Chunk<'_>],
    document: &mut Document,
    warnings: &mut Vec<ImportWarning>,
) -> Result<(), Error> {
    let names = name_strings(chunks);
    let style_name_ids = style_name_ids(chunks);

    for chunk in chunks {
        match chunk.record {
            COLOURS => map_colours(chunk.data, document, warnings),
            FONT_LIST | FONTFACES => warn(
                warnings,
                "document",
                WarningKind::Dropped {
                    what: "document font list",
                },
            ),
            FONT_IX | FONTFACE => warn(
                warnings,
                "document",
                WarningKind::Dropped {
                    what: "document font",
                },
            ),
            _ => {}
        }
    }

    map_styles(chunks, document, warnings)?;

    for style in &mut document.stylesheet.styles {
        let name = style_name_ids
            .get(&style.visio_id)
            .and_then(|name_id| names.get(name_id))
            .cloned()
            .unwrap_or_default();
        if !name.is_empty() {
            style.name = name.clone();
            style.name_u = name;
        }
    }

    Ok(())
}

/// A checked reader for one record payload.
///
/// Every method returns `None` instead of panicking on a short payload; the
/// callers turn that into a `BadValue` warning (ADR 0071).
#[derive(Clone, Copy)]
struct Reader<'a> {
    data: &'a [u8],
}

impl<'a> Reader<'a> {
    const fn new(data: &'a [u8]) -> Self {
        Self { data }
    }

    fn byte(&self, at: usize) -> Option<u8> {
        self.data.get(at).copied()
    }

    fn u16(&self, at: usize) -> Option<u16> {
        let end = at.checked_add(2)?;
        let bytes: [u8; 2] = self.data.get(at..end)?.try_into().ok()?;
        Some(u16::from_le_bytes(bytes))
    }

    fn u32(&self, at: usize) -> Option<u32> {
        let end = at.checked_add(4)?;
        let bytes: [u8; 4] = self.data.get(at..end)?.try_into().ok()?;
        Some(u32::from_le_bytes(bytes))
    }

    fn f64(&self, at: usize) -> Option<f64> {
        let end = at.checked_add(8)?;
        let bytes: [u8; 8] = self.data.get(at..end)?.try_into().ok()?;
        Some(f64::from_le_bytes(bytes))
    }

    fn rgba(&self, at: usize) -> Option<[u8; 4]> {
        let end = at.checked_add(4)?;
        self.data.get(at..end)?.try_into().ok()
    }
}

/// Records one warning with its context.
fn warn(warnings: &mut Vec<ImportWarning>, context: &str, kind: WarningKind) {
    warnings.push(ImportWarning {
        context: context.to_owned(),
        kind,
    });
}

/// A number cell, or a `BadValue` warning for a non-finite value.
fn number_cell(
    name: &'static str,
    value: f64,
    context: &str,
    warnings: &mut Vec<ImportWarning>,
    what: &'static str,
) -> Option<Cell> {
    if !value.is_finite() {
        warn(warnings, context, WarningKind::BadValue { what });
        return None;
    }
    let mut cell = Cell::new(CellName::from_static(name));
    cell.value = Some(CellValue::Number(value));
    Some(cell)
}

/// A colour cell.
fn color_cell(name: &'static str, color: [u8; 4]) -> Cell {
    let mut cell = Cell::new(CellName::from_static(name));
    cell.value = Some(CellValue::Color(Color::new(color[0], color[1], color[2])));
    cell
}

/// A colour cell for a record that carries both a palette index and RGBA.
///
/// A non-zero index, or an empty RGBA value, keeps the index (the `.vsdx`
/// spelling); a direct RGBA value with index zero becomes a `Color`.
fn indexed_color_cell(name: &'static str, index: u8, color: [u8; 4]) -> Cell {
    let mut cell = Cell::new(CellName::from_static(name));
    let empty = color[0] == 0 && color[1] == 0 && color[2] == 0 && color[3] == 0;
    cell.value = Some(if index == 0 && !empty {
        CellValue::Color(Color::new(color[0], color[1], color[2]))
    } else {
        CellValue::Number(f64::from(index))
    });
    cell
}

/// A pattern cell, or a `Dropped` warning for an index the model cannot
/// spell.
fn pattern_cell(
    name: &'static str,
    index: u8,
    max: u8,
    context: &str,
    warnings: &mut Vec<ImportWarning>,
    what: &'static str,
) -> Option<Cell> {
    if index > max {
        warn(warnings, context, WarningKind::Dropped { what });
        return None;
    }
    let mut cell = Cell::new(CellName::from_static(name));
    cell.value = Some(CellValue::Number(f64::from(index)));
    Some(cell)
}

/// Replaces a direct cell of the same name, or appends it.
fn set_cell(sheet: &mut Sheet, cell: Cell) {
    if let Some(existing) = sheet
        .cells
        .iter_mut()
        .find(|existing| existing.name == cell.name)
    {
        *existing = cell;
    } else {
        sheet.cells.push(cell);
    }
}

/// Replaces a row cell of the same name, or appends it.
fn set_row_cell(row: &mut Row, cell: Cell) {
    if let Some(existing) = row
        .cells
        .iter_mut()
        .find(|existing| existing.name == cell.name)
    {
        *existing = cell;
    } else {
        row.cells.push(cell);
    }
}

/// The named section of a sheet, created on first use.
fn section<'a>(sheet: &'a mut Sheet, name: &'static str) -> Option<&'a mut Section> {
    if let Some(index) = sheet
        .sections
        .iter()
        .position(|section| section.name.as_str() == name)
    {
        sheet.sections.get_mut(index)
    } else {
        sheet
            .sections
            .push(Section::new(SectionName::from_static(name)));
        sheet.sections.last_mut()
    }
}

/// The row with this `IX` in a section, created on first use.
fn row(section: &mut Section, ix: i32) -> Option<&mut Row> {
    if let Some(index) = section.rows.iter().position(|row| row.ix == Some(ix)) {
        section.rows.get_mut(index)
    } else {
        let mut new_row = Row::new();
        new_row.ix = Some(ix);
        section.rows.push(new_row);
        section.rows.last_mut()
    }
}

/// Maps the colour table record into the document colour table.
///
/// The binary table starts at index 0 and includes the fixed palette; the
/// model keeps 0..23 as its fixed palette, so only entries from 24 up are
/// stored (ADR 0019).
fn map_colours(data: &[u8], document: &mut Document, warnings: &mut Vec<ImportWarning>) {
    let reader = Reader::new(data);
    // A chunk reader that keeps the compressed blob's four-byte prefix
    // leaves the record fields four bytes later; accept both spellings and
    // prefer the one whose declared count matches the payload length.
    let count_at = |at: usize| {
        reader
            .byte(at)
            .and_then(|count| at.checked_add(2).map(|start| (count, start)))
    };
    let exact = |(count, at): (u8, usize)| {
        at.checked_add(usize::from(count).saturating_mul(4)) == Some(data.len())
    };
    let plausible =
        |(count, at): (u8, usize)| usize::from(count) <= data.len().saturating_sub(at) / 4;
    let plain = count_at(2).filter(|candidate| exact(*candidate));
    let prefixed = count_at(6).filter(|candidate| exact(*candidate));
    let candidate = plain
        .or(prefixed)
        .or_else(|| count_at(2).filter(|candidate| plausible(*candidate)))
        .or_else(|| count_at(6).filter(|candidate| plausible(*candidate)));
    let Some((count, mut at)) = candidate else {
        warn(
            warnings,
            "colour table",
            WarningKind::BadValue {
                what: "colour table record",
            },
        );
        return;
    };
    document.color_table.clear();
    for index in 0..usize::from(count) {
        let Some(color) = reader.rgba(at) else {
            warn(
                warnings,
                "colour table",
                WarningKind::BadValue {
                    what: "colour table entry",
                },
            );
            break;
        };
        let Ok(index) = i32::try_from(index) else {
            warn(
                warnings,
                "colour table",
                WarningKind::BadValue {
                    what: "colour table index",
                },
            );
            break;
        };
        if index >= 24 {
            document.color_table.push(ColorTableEntry {
                index,
                color: Color::new(color[0], color[1], color[2]),
            });
        }
        let Some(next) = at.checked_add(4) else {
            break;
        };
        at = next;
    }
}

/// Maps every style sheet and its style-property records.
fn map_styles(
    chunks: &[Chunk<'_>],
    document: &mut Document,
    warnings: &mut Vec<ImportWarning>,
) -> Result<(), Error> {
    let base = chunks
        .iter()
        .find(|chunk| chunk.record == STYLES_STREAM)
        .map(|chunk| chunk.level);
    let mut in_styles = base.is_none();
    let mut current: Option<(usize, u32, u16)> = None;

    for chunk in chunks {
        if let Some(base_level) = base {
            if chunk.record == STYLES_STREAM && chunk.level == base_level {
                in_styles = true;
                current = None;
                continue;
            }
            if in_styles && chunk.level <= base_level {
                in_styles = false;
                current = None;
            }
        }
        if !in_styles {
            continue;
        }

        if chunk.record == STYLE_SHEET {
            let index = map_style_sheet(chunk, document, warnings);
            current = Some((index, chunk.id, chunk.level));
            continue;
        }

        let Some((style_index, visio_id, sheet_level)) = current else {
            continue;
        };
        if chunk.level <= sheet_level {
            current = None;
            continue;
        }
        if chunk.level != sheet_level.saturating_add(2) {
            continue;
        }
        let context = format!("style {visio_id} / {}", record_name(chunk.record));
        let Some(style) = document.stylesheet.styles.get_mut(style_index) else {
            continue;
        };
        match chunk.record {
            LINE => map_line(chunk.data, &mut style.sheet, &context, warnings),
            FILL_AND_SHADOW => map_fill(chunk.data, &mut style.sheet, &context, warnings),
            TEXT_BLOCK => map_text_block(chunk.data, &mut style.sheet, &context, warnings),
            CHAR_IX => map_character(chunk, &mut style.sheet, &context, warnings),
            PARA_IX => map_paragraph(chunk.data, chunk.id, &mut style.sheet, &context, warnings),
            record if is_structural(record) => {}
            _ => warn(
                warnings,
                &context,
                WarningKind::SkippedRecord {
                    name: "style record",
                },
            ),
        }
    }
    Ok(())
}

/// Whether a record only groups style-property records or carries names.
fn is_structural(record: Record) -> bool {
    matches!(
        record.0,
        0x1a | 0x2c
            | 0x2d
            | 0x32
            | 0x33
            | 0x68
            | 0x69
            | 0x6a
            | 0x6b
            | 0x88
            | 0x93
            | 0x96
            | 0x97
            | 0xb5
            | 0xc9
            | 0x34
    )
}

/// A readable name for a record kind in a warning context.
fn record_name(record: Record) -> &'static str {
    match record {
        LINE => "line",
        FILL_AND_SHADOW => "fill",
        TEXT_BLOCK => "text block",
        CHAR_IX => "character",
        PARA_IX => "paragraph",
        _ => "record",
    }
}

/// Maps one style sheet record and appends the style to the stylesheet.
fn map_style_sheet(
    chunk: &Chunk<'_>,
    document: &mut Document,
    warnings: &mut Vec<ImportWarning>,
) -> usize {
    let Ok(index) = u32::try_from(document.stylesheet.styles.len()) else {
        warn(
            warnings,
            "stylesheet",
            WarningKind::BadValue {
                what: "stylesheet row count",
            },
        );
        return document.stylesheet.styles.len().saturating_sub(1);
    };
    let context = format!("style {}", chunk.id);
    let reader = Reader::new(chunk.data);
    let line_style = style_reference(&reader, 0x22, &context, warnings, "line style reference");
    let fill_style = style_reference(&reader, 0x26, &context, warnings, "fill style reference");
    let text_style = style_reference(&reader, 0x2a, &context, warnings, "text style reference");

    let mut style = Style::new(StyleId { index });
    style.visio_id = chunk.id;
    style.line_style = line_style;
    style.fill_style = fill_style;
    style.text_style = text_style;
    document.stylesheet.styles.push(style);
    document.stylesheet.styles.len().saturating_sub(1)
}

/// One style reference field, where `u32::MAX` means no parent.
fn style_reference(
    reader: &Reader<'_>,
    at: usize,
    context: &str,
    warnings: &mut Vec<ImportWarning>,
    what: &'static str,
) -> Option<u32> {
    match reader.u32(at) {
        Some(u32::MAX) => None,
        Some(id) => Some(id),
        None => {
            warn(warnings, context, WarningKind::BadValue { what });
            None
        }
    }
}

/// Maps a line record to the style's line cells.
fn map_line(data: &[u8], sheet: &mut Sheet, context: &str, warnings: &mut Vec<ImportWarning>) {
    let reader = Reader::new(data);
    let Some(width) = reader.f64(1) else {
        warn(
            warnings,
            context,
            WarningKind::BadValue {
                what: "line record",
            },
        );
        return;
    };
    if let Some(cell) = number_cell("LineWeight", width, context, warnings, "line weight") {
        set_cell(sheet, cell);
    }
    if let Some(color) = reader.rgba(10) {
        set_cell(sheet, color_cell("LineColor", color));
        let transparency = f64::from(color[3]) / 255.0;
        if let Some(cell) = number_cell(
            "LineColorTrans",
            transparency,
            context,
            warnings,
            "line transparency",
        ) {
            set_cell(sheet, cell);
        }
    }
    if let Some(pattern) = reader.byte(14)
        && let Some(cell) = pattern_cell(
            "LinePattern",
            pattern,
            MAX_LINE_PATTERN,
            context,
            warnings,
            "line pattern",
        )
    {
        set_cell(sheet, cell);
    }
    if let Some(rounding) = reader.f64(16)
        && let Some(cell) = number_cell("Rounding", rounding, context, warnings, "line rounding")
    {
        set_cell(sheet, cell);
    }
    if let Some(begin) = reader.byte(24)
        && let Some(cell) = number_cell(
            "BeginArrow",
            f64::from(begin),
            context,
            warnings,
            "begin arrow",
        )
    {
        set_cell(sheet, cell);
    }
    if let Some(end) = reader.byte(25)
        && let Some(cell) = number_cell("EndArrow", f64::from(end), context, warnings, "end arrow")
    {
        set_cell(sheet, cell);
    }
    if let Some(cap) = reader.byte(26)
        && let Some(cell) = number_cell("LineCap", f64::from(cap), context, warnings, "line cap")
    {
        set_cell(sheet, cell);
    }
}

/// Maps a fill-and-shadow record to the style's fill and shadow cells.
fn map_fill(data: &[u8], sheet: &mut Sheet, context: &str, warnings: &mut Vec<ImportWarning>) {
    let reader = Reader::new(data);
    let Some(foreground_index) = reader.byte(0) else {
        warn(
            warnings,
            context,
            WarningKind::BadValue {
                what: "fill record",
            },
        );
        return;
    };
    if let Some(foreground) = reader.rgba(1) {
        set_cell(
            sheet,
            indexed_color_cell("FillForegnd", foreground_index, foreground),
        );
        if let Some(cell) = number_cell(
            "FillForegndTrans",
            f64::from(foreground[3]) / 255.0,
            context,
            warnings,
            "fill transparency",
        ) {
            set_cell(sheet, cell);
        }
    }
    if let (Some(index), Some(background)) = (reader.byte(5), reader.rgba(6)) {
        set_cell(sheet, indexed_color_cell("FillBkgnd", index, background));
        if let Some(cell) = number_cell(
            "FillBkgndTrans",
            f64::from(background[3]) / 255.0,
            context,
            warnings,
            "fill background transparency",
        ) {
            set_cell(sheet, cell);
        }
    }
    if let Some(pattern) = reader.byte(10)
        && let Some(cell) = pattern_cell(
            "FillPattern",
            pattern,
            MAX_FILL_PATTERN,
            context,
            warnings,
            "fill pattern",
        )
    {
        set_cell(sheet, cell);
    }
    if let (Some(index), Some(shadow)) = (reader.byte(11), reader.rgba(12)) {
        set_cell(sheet, indexed_color_cell("ShdwForegnd", index, shadow));
        if let Some(cell) = number_cell(
            "ShdwForegndTrans",
            f64::from(shadow[3]) / 255.0,
            context,
            warnings,
            "shadow transparency",
        ) {
            set_cell(sheet, cell);
        }
    }
    if let Some(pattern) = reader.byte(21)
        && let Some(cell) = pattern_cell(
            "ShdwPattern",
            pattern,
            MAX_SHADOW_PATTERN,
            context,
            warnings,
            "shadow pattern",
        )
    {
        set_cell(sheet, cell);
    }
    // Version 11 adds the shadow offsets after the shadow pattern; a version
    // 6 record ends there.
    if let Some(offset) = reader.f64(24)
        && let Some(cell) = number_cell(
            "ShapeShdwOffsetX",
            offset,
            context,
            warnings,
            "shadow offset x",
        )
    {
        set_cell(sheet, cell);
    }
    if let Some(offset) = reader.f64(33)
        && let Some(cell) = number_cell(
            "ShapeShdwOffsetY",
            offset,
            context,
            warnings,
            "shadow offset y",
        )
    {
        set_cell(sheet, cell);
    }
}

/// Maps a text block record to the style's text block cells.
fn map_text_block(
    data: &[u8],
    sheet: &mut Sheet,
    context: &str,
    warnings: &mut Vec<ImportWarning>,
) {
    let reader = Reader::new(data);
    let Some(left) = reader.f64(1) else {
        warn(
            warnings,
            context,
            WarningKind::BadValue {
                what: "text block record",
            },
        );
        return;
    };
    let fields = [
        ("LeftMargin", Some(left), "left margin"),
        ("RightMargin", reader.f64(10), "right margin"),
        ("TopMargin", reader.f64(19), "top margin"),
        ("BottomMargin", reader.f64(28), "bottom margin"),
    ];
    for (name, value, what) in fields {
        if let Some(value) = value
            && let Some(cell) = number_cell(name, value, context, warnings, what)
        {
            set_cell(sheet, cell);
        }
    }
    if let Some(vertical) = reader.byte(36)
        && let Some(cell) = number_cell(
            "VerticalAlign",
            f64::from(vertical),
            context,
            warnings,
            "vertical alignment",
        )
    {
        set_cell(sheet, cell);
    }
    if let (Some(index), Some(background)) = (reader.byte(37), reader.rgba(38)) {
        let cell = if index == 0 || index == 0xff {
            indexed_color_cell("TextBkgnd", 0, [0, 0, 0, 0])
        } else {
            indexed_color_cell("TextBkgnd", index, background)
        };
        set_cell(sheet, cell);
        if let Some(cell) = number_cell(
            "TextBkgndTrans",
            f64::from(background[3]) / 255.0,
            context,
            warnings,
            "text background transparency",
        ) {
            set_cell(sheet, cell);
        }
    }
    if let Some(tab_stop) = reader.f64(43)
        && let Some(cell) = number_cell(
            "DefaultTabStop",
            tab_stop,
            context,
            warnings,
            "default tab stop",
        )
    {
        set_cell(sheet, cell);
    }
    if let Some(direction) = reader.byte(63)
        && let Some(cell) = number_cell(
            "TextDirection",
            f64::from(direction),
            context,
            warnings,
            "text direction",
        )
    {
        set_cell(sheet, cell);
    }
}

/// Maps a character record to a `Character` row of the style's sheet.
fn map_character(
    chunk: &Chunk<'_>,
    sheet: &mut Sheet,
    context: &str,
    warnings: &mut Vec<ImportWarning>,
) {
    let Ok(ix) = i32::try_from(chunk.id) else {
        warn(
            warnings,
            context,
            WarningKind::BadValue {
                what: "character row index",
            },
        );
        return;
    };
    let reader = Reader::new(chunk.data);
    let Some(size) = reader.f64(18) else {
        warn(
            warnings,
            context,
            WarningKind::BadValue {
                what: "character record",
            },
        );
        return;
    };
    let Some(section) = section(sheet, "Character") else {
        return;
    };
    let Some(row) = row(section, ix) else {
        return;
    };

    if let (Some(index), Some(color)) = (reader.byte(6), reader.rgba(7)) {
        set_row_cell(row, indexed_color_cell("Color", index, color));
        if let Some(cell) = number_cell(
            "ColorTrans",
            f64::from(color[3]) / 255.0,
            context,
            warnings,
            "character colour transparency",
        ) {
            set_row_cell(row, cell);
        }
    }
    if let Some(style) = reader.byte(11)
        && let Some(cell) = number_cell(
            "Style",
            f64::from(style),
            context,
            warnings,
            "character style",
        )
    {
        set_row_cell(row, cell);
    }
    if let Some(case) = reader.byte(12)
        && let Some(cell) =
            number_cell("Case", f64::from(case), context, warnings, "character case")
    {
        set_row_cell(row, cell);
    }
    if let Some(position) = reader.byte(13)
        && let Some(cell) = number_cell(
            "Pos",
            f64::from(position),
            context,
            warnings,
            "character position",
        )
    {
        set_row_cell(row, cell);
    }
    if let Some(scale) = reader.u16(14)
        && let Some(cell) = number_cell(
            "FontScale",
            f64::from(scale) / 10_000.0,
            context,
            warnings,
            "character font scale",
        )
    {
        set_row_cell(row, cell);
    }
    if let Some(cell) = number_cell("Size", size, context, warnings, "character size") {
        set_row_cell(row, cell);
    }
    if let Some(flags) = reader.byte(26) {
        let booleans = [
            ("DblUnderline", flags & 1 != 0, "double underline"),
            ("Strikethru", flags & 4 != 0, "strike-through"),
            (
                "DoubleStrikethrough",
                flags & 0x20 != 0,
                "double strike-through",
            ),
        ];
        for (name, value, what) in booleans {
            let value = if value { 1.0 } else { 0.0 };
            if let Some(cell) = number_cell(name, value, context, warnings, what) {
                set_row_cell(row, cell);
            }
        }
    }
}

/// Maps a paragraph record to a `Paragraph` row of the style's sheet.
fn map_paragraph(
    data: &[u8],
    id: u32,
    sheet: &mut Sheet,
    context: &str,
    warnings: &mut Vec<ImportWarning>,
) {
    let Ok(ix) = i32::try_from(id) else {
        warn(
            warnings,
            context,
            WarningKind::BadValue {
                what: "paragraph row index",
            },
        );
        return;
    };
    let reader = Reader::new(data);
    let Some(ind_first) = reader.f64(5) else {
        warn(
            warnings,
            context,
            WarningKind::BadValue {
                what: "paragraph record",
            },
        );
        return;
    };
    let Some(section) = section(sheet, "Paragraph") else {
        return;
    };
    let Some(row) = row(section, ix) else {
        return;
    };
    let fields = [
        ("IndFirst", Some(ind_first), "first line indent"),
        ("IndLeft", reader.f64(14), "left indent"),
        ("IndRight", reader.f64(23), "right indent"),
        ("SpLine", reader.f64(32), "line spacing"),
        ("SpBefore", reader.f64(41), "space before"),
        ("SpAfter", reader.f64(50), "space after"),
    ];
    for (name, value, what) in fields {
        if let Some(value) = value
            && let Some(cell) = number_cell(name, value, context, warnings, what)
        {
            set_row_cell(row, cell);
        }
    }
    if let Some(align) = reader.byte(58)
        && let Some(cell) = number_cell(
            "HorzAlign",
            f64::from(align),
            context,
            warnings,
            "paragraph alignment",
        )
    {
        set_row_cell(row, cell);
    }
    if let Some(bullet) = reader.byte(59)
        && let Some(cell) = number_cell(
            "Bullet",
            f64::from(bullet),
            context,
            warnings,
            "paragraph bullet",
        )
    {
        set_row_cell(row, cell);
    }
}

/// Reads the document name strings (`Name2` records).
fn name_strings(chunks: &[Chunk<'_>]) -> HashMap<u32, String> {
    let mut names = HashMap::new();
    // The document name list (0x32/0x33) is the one the style name index
    // points into. Shape-local `Name` records (0x2d) use the same id space,
    // so they only fill in ids the document list did not name.
    for chunk in chunks {
        if chunk.record == NAME2
            && let Some(name) = decode_utf16(chunk.data.get(4..).unwrap_or(&[]))
        {
            names.insert(chunk.id, name);
        }
    }
    for chunk in chunks {
        if chunk.record == NAME
            && let Some(name) = decode_utf16(chunk.data)
        {
            names.entry(chunk.id).or_insert(name);
        }
    }
    names
}

/// Maps style element ids to name ids from the styles stream's name index.
fn style_name_ids(chunks: &[Chunk<'_>]) -> HashMap<u32, u32> {
    let base = chunks
        .iter()
        .find(|chunk| chunk.record == STYLES_STREAM)
        .map(|chunk| chunk.level);
    let Some(base_level) = base else {
        return HashMap::new();
    };
    let mut in_styles = false;
    let mut names = HashMap::new();
    for chunk in chunks {
        if chunk.record == STYLES_STREAM && chunk.level == base_level {
            in_styles = true;
            continue;
        }
        if in_styles && chunk.level <= base_level {
            break;
        }
        if !in_styles {
            continue;
        }
        match chunk.record {
            NAME_IDX => read_name_index(chunk.data, false, &mut names),
            NAME_IDX_123 => read_name_index(chunk.data, true, &mut names),
            _ => {}
        }
    }
    names
}

/// Reads one name-index record: `count` plus 13-byte entries, or plain
/// name-id/element-id pairs.
fn read_name_index(data: &[u8], pairs: bool, names: &mut HashMap<u32, u32>) {
    let reader = Reader::new(data);
    if pairs {
        let mut at = 0usize;
        while let (Some(name_id), Some(element_id)) = (reader.u32(at), reader.u32(at + 4)) {
            names.insert(element_id, name_id);
            let Some(next) = at.checked_add(8) else {
                break;
            };
            at = next;
        }
        return;
    }
    let Some(count) = reader.u32(0) else {
        return;
    };
    let mut at = 4usize;
    for _ in 0..count {
        let Some(name_id) = reader.u32(at) else {
            break;
        };
        let Some(element_id) = reader.u32(at + 8) else {
            break;
        };
        names.insert(element_id, name_id);
        let Some(next) = at.checked_add(13) else {
            break;
        };
        at = next;
    }
}

/// A NUL-terminated UTF-16LE string.
fn decode_utf16(bytes: &[u8]) -> Option<String> {
    let mut units = Vec::new();
    for pair in bytes.chunks_exact(2) {
        let unit = u16::from_le_bytes(<[u8; 2]>::try_from(pair).ok()?);
        if unit == 0 {
            break;
        }
        units.push(unit);
    }
    if units.is_empty() {
        return None;
    }
    String::from_utf16(&units).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk<'a>(record: u16, level: u16, id: u32, data: &'a [u8]) -> Chunk<'a> {
        Chunk {
            record: Record(record),
            level,
            list: 0,
            id,
            data,
        }
    }

    fn style_sheet(line: u32, fill: u32, text: u32) -> Vec<u8> {
        let mut data = vec![0u8; 0x2e];
        data[0x22..0x26].copy_from_slice(&line.to_le_bytes());
        data[0x26..0x2a].copy_from_slice(&fill.to_le_bytes());
        data[0x2a..0x2e].copy_from_slice(&text.to_le_bytes());
        data
    }

    fn line_record(width: f64, color: [u8; 4], pattern: u8) -> Vec<u8> {
        let mut data = vec![0u8; 60];
        data[1..9].copy_from_slice(&width.to_le_bytes());
        data[10..14].copy_from_slice(&color);
        data[14] = pattern;
        data[16..24].copy_from_slice(&0.25f64.to_le_bytes());
        data[24] = 2;
        data[25] = 3;
        data[26] = 1;
        data
    }

    fn fill_record(
        foreground: (u8, [u8; 4]),
        background: (u8, [u8; 4]),
        pattern: u8,
        shadow: (u8, [u8; 4]),
        shadow_pattern: u8,
    ) -> Vec<u8> {
        let mut data = vec![0u8; 60];
        data[0] = foreground.0;
        data[1..5].copy_from_slice(&foreground.1);
        data[5] = background.0;
        data[6..10].copy_from_slice(&background.1);
        data[10] = pattern;
        data[11] = shadow.0;
        data[12..16].copy_from_slice(&shadow.1);
        data[21] = shadow_pattern;
        data[24..32].copy_from_slice(&0.125f64.to_le_bytes());
        data[33..41].copy_from_slice(&0.25f64.to_le_bytes());
        data
    }

    fn text_block_record() -> Vec<u8> {
        let mut data = vec![0u8; 64];
        data[1..9].copy_from_slice(&0.05f64.to_le_bytes());
        data[10..18].copy_from_slice(&0.1f64.to_le_bytes());
        data[19..27].copy_from_slice(&0.15f64.to_le_bytes());
        data[28..36].copy_from_slice(&0.2f64.to_le_bytes());
        data[36] = 2;
        data[37] = 3;
        data[38..42].copy_from_slice(&[255, 0, 0, 0]);
        data[43..51].copy_from_slice(&0.5f64.to_le_bytes());
        data[63] = 1;
        data
    }

    fn character_record() -> Vec<u8> {
        let mut data = vec![0u8; 27];
        data[6] = 4;
        data[7..11].copy_from_slice(&[0, 0, 255, 0]);
        data[11] = 3;
        data[12] = 1;
        data[13] = 2;
        data[14..16].copy_from_slice(&15_000u16.to_le_bytes());
        data[18..26].copy_from_slice(&0.125f64.to_le_bytes());
        data[26] = 0x25;
        data
    }

    fn paragraph_record() -> Vec<u8> {
        let mut data = vec![0u8; 89];
        data[5..13].copy_from_slice(&0.1f64.to_le_bytes());
        data[14..22].copy_from_slice(&0.2f64.to_le_bytes());
        data[23..31].copy_from_slice(&0.3f64.to_le_bytes());
        data[32..40].copy_from_slice(&(-1.2f64).to_le_bytes());
        data[41..49].copy_from_slice(&0.4f64.to_le_bytes());
        data[50..58].copy_from_slice(&0.5f64.to_le_bytes());
        data[58] = 1;
        data[59] = 2;
        data
    }

    fn colour_table() -> Vec<u8> {
        let mut data = vec![0u8; 4 + 4 * 26];
        data[2] = 26;
        data[4 + 4 * 24..4 + 4 * 24 + 4].copy_from_slice(&[20, 20, 20, 0]);
        data[4 + 4 * 25..4 + 4 * 25 + 4].copy_from_slice(&[30, 30, 30, 0]);
        data
    }

    fn name2(name: &str) -> Vec<u8> {
        let mut data = vec![1, 0, 0, 0];
        for unit in name.encode_utf16().chain(std::iter::once(0)) {
            data.extend_from_slice(&unit.to_le_bytes());
        }
        data
    }

    fn name_index(name_id: u32, element_id: u32) -> Vec<u8> {
        let mut data = vec![1, 0, 0, 0];
        data.extend_from_slice(&name_id.to_le_bytes());
        data.extend_from_slice(&name_id.to_le_bytes());
        data.extend_from_slice(&element_id.to_le_bytes());
        data.push(0);
        data
    }

    fn collect(chunks: &[Chunk<'_>]) -> (Document, Vec<ImportWarning>) {
        let mut document = Document::new();
        let mut warnings = Vec::new();
        super::collect(chunks, &mut document, &mut warnings).expect("collect");
        (document, warnings)
    }

    fn value(sheet: &Sheet, name: &str) -> Option<CellValue> {
        sheet
            .cells
            .iter()
            .find(|cell| cell.name.as_str() == name)
            .and_then(|cell| cell.value.clone())
    }

    fn row_value(sheet: &Sheet, section: &str, ix: i32, name: &str) -> Option<CellValue> {
        sheet
            .sections
            .iter()
            .find(|candidate| candidate.name.as_str() == section)
            .and_then(|section| section.rows.iter().find(|row| row.ix == Some(ix)))
            .and_then(|row| {
                row.cells
                    .iter()
                    .find(|cell| cell.name.as_str() == name)
                    .and_then(|cell| cell.value.clone())
            })
    }

    fn number(value: f64) -> Option<CellValue> {
        Some(CellValue::Number(value))
    }

    fn style_chunks<'a>(sheet: &'a [u8], children: &[(u16, u32, &'a [u8])]) -> Vec<Chunk<'a>> {
        let mut chunks = vec![chunk(0x1a, 1, 0, &[])];
        chunks.push(chunk(0x4a, 2, 4, sheet));
        for (record, id, data) in children {
            chunks.push(chunk(*record, 4, *id, data));
        }
        chunks
    }

    #[test]
    fn an_empty_chunk_list_maps_nothing() {
        let (document, warnings) = collect(&[]);
        assert!(document.stylesheet.styles.is_empty());
        assert!(document.color_table.is_empty());
        assert!(warnings.is_empty());
    }

    #[test]
    fn a_style_sheet_keeps_its_id_name_and_references() {
        let sheet = style_sheet(3, u32::MAX, 6);
        let index = name_index(3, 4);
        let name = name2("Guide");
        let chunks = vec![
            chunk(0x33, 1, 3, &name),
            chunk(0x1a, 1, 0, &[]),
            chunk(0x4a, 2, 4, &sheet),
            chunk(0xc9, 3, 0, &index),
        ];
        let (document, warnings) = collect(&chunks);
        let style = &document.stylesheet.styles[0];
        assert_eq!(style.id, StyleId { index: 0 });
        assert_eq!(style.visio_id, 4);
        assert_eq!(style.line_style, Some(3));
        assert_eq!(style.fill_style, None);
        assert_eq!(style.text_style, Some(6));
        assert_eq!(style.name, "Guide");
        assert_eq!(style.name_u, "Guide");
        assert!(warnings.is_empty());
    }

    #[test]
    fn a_style_sheet_without_a_name_index_has_an_empty_name() {
        let sheet = style_sheet(0, 0, 0);
        let chunks = style_chunks(&sheet, &[]);
        let (document, _) = collect(&chunks);
        assert_eq!(document.stylesheet.styles[0].name, "");
        assert_eq!(document.stylesheet.styles[0].name_u, "");
    }

    #[test]
    fn line_fill_text_block_character_and_paragraph_map_their_cells() {
        let sheet = style_sheet(3, 4, 6);
        let line = line_record(0.25, [1, 2, 3, 0], 2);
        let fill = fill_record(
            (1, [255, 255, 255, 0]),
            (0, [0, 0, 0, 0]),
            1,
            (2, [9, 8, 7, 0]),
            3,
        );
        let text = text_block_record();
        let character = character_record();
        let paragraph = paragraph_record();
        let chunks = style_chunks(
            &sheet,
            &[
                (0x85, 1, &line),
                (0x86, 2, &fill),
                (0x87, 3, &text),
                (0x94, 0, &character),
                (0x95, 0, &paragraph),
            ],
        );
        let (document, warnings) = collect(&chunks);
        assert!(warnings.is_empty(), "{warnings:?}");
        let style = &document.stylesheet.styles[0];

        assert_eq!(value(&style.sheet, "LineWeight"), number(0.25));
        assert_eq!(
            value(&style.sheet, "LineColor"),
            Some(CellValue::Color(Color::new(1, 2, 3)))
        );
        assert_eq!(value(&style.sheet, "LineColorTrans"), number(0.0));
        assert_eq!(value(&style.sheet, "LinePattern"), number(2.0));
        assert_eq!(value(&style.sheet, "Rounding"), number(0.25));
        assert_eq!(value(&style.sheet, "BeginArrow"), number(2.0));
        assert_eq!(value(&style.sheet, "EndArrow"), number(3.0));
        assert_eq!(value(&style.sheet, "LineCap"), number(1.0));

        assert_eq!(value(&style.sheet, "FillForegnd"), number(1.0));
        assert_eq!(value(&style.sheet, "FillBkgnd"), number(0.0));
        assert_eq!(value(&style.sheet, "FillPattern"), number(1.0));
        assert_eq!(value(&style.sheet, "ShdwForegnd"), number(2.0));
        assert_eq!(value(&style.sheet, "ShdwPattern"), number(3.0));
        assert_eq!(value(&style.sheet, "ShapeShdwOffsetX"), number(0.125));
        assert_eq!(value(&style.sheet, "ShapeShdwOffsetY"), number(0.25));

        assert_eq!(value(&style.sheet, "LeftMargin"), number(0.05));
        assert_eq!(value(&style.sheet, "RightMargin"), number(0.1));
        assert_eq!(value(&style.sheet, "TopMargin"), number(0.15));
        assert_eq!(value(&style.sheet, "BottomMargin"), number(0.2));
        assert_eq!(value(&style.sheet, "VerticalAlign"), number(2.0));
        assert_eq!(value(&style.sheet, "TextBkgnd"), number(3.0));
        assert_eq!(value(&style.sheet, "DefaultTabStop"), number(0.5));
        assert_eq!(value(&style.sheet, "TextDirection"), number(1.0));

        assert_eq!(
            row_value(&style.sheet, "Character", 0, "Color"),
            number(4.0)
        );
        assert_eq!(
            row_value(&style.sheet, "Character", 0, "Style"),
            number(3.0)
        );
        assert_eq!(
            row_value(&style.sheet, "Character", 0, "FontScale"),
            number(1.5)
        );
        assert_eq!(
            row_value(&style.sheet, "Character", 0, "Size"),
            number(0.125)
        );
        assert_eq!(
            row_value(&style.sheet, "Character", 0, "DblUnderline"),
            number(1.0)
        );
        assert_eq!(
            row_value(&style.sheet, "Character", 0, "Strikethru"),
            number(1.0)
        );
        assert_eq!(
            row_value(&style.sheet, "Character", 0, "DoubleStrikethrough"),
            number(1.0)
        );

        assert_eq!(
            row_value(&style.sheet, "Paragraph", 0, "IndFirst"),
            number(0.1)
        );
        assert_eq!(
            row_value(&style.sheet, "Paragraph", 0, "SpLine"),
            number(-1.2)
        );
        assert_eq!(
            row_value(&style.sheet, "Paragraph", 0, "HorzAlign"),
            number(1.0)
        );
        assert_eq!(
            row_value(&style.sheet, "Paragraph", 0, "Bullet"),
            number(2.0)
        );
    }

    #[test]
    fn a_direct_colour_with_index_zero_is_a_color_value() {
        let sheet = style_sheet(u32::MAX, u32::MAX, u32::MAX);
        let line = line_record(0.1, [10, 20, 30, 0], 1);
        let fill = fill_record(
            (0, [40, 50, 60, 128]),
            (0, [0, 0, 0, 0]),
            0,
            (0, [0, 0, 0, 0]),
            0,
        );
        let chunks = style_chunks(&sheet, &[(0x85, 1, &line), (0x86, 2, &fill)]);
        let (document, _) = collect(&chunks);
        let style = &document.stylesheet.styles[0];
        assert_eq!(
            value(&style.sheet, "LineColor"),
            Some(CellValue::Color(Color::new(10, 20, 30)))
        );
        assert_eq!(
            value(&style.sheet, "FillForegnd"),
            Some(CellValue::Color(Color::new(40, 50, 60)))
        );
        assert_eq!(
            value(&style.sheet, "FillForegndTrans"),
            number(128.0 / 255.0)
        );
    }

    #[test]
    fn the_colour_table_keeps_entries_from_24_up() {
        let table = colour_table();
        let chunks = vec![chunk(0x16, 1, 0, &table)];
        let (document, warnings) = collect(&chunks);
        assert!(warnings.is_empty());
        assert_eq!(
            document.color_table,
            vec![
                ColorTableEntry {
                    index: 24,
                    color: Color::new(20, 20, 20),
                },
                ColorTableEntry {
                    index: 25,
                    color: Color::new(30, 30, 30),
                },
            ]
        );
    }

    #[test]
    fn the_colour_table_accepts_a_leading_blob_prefix() {
        let table = colour_table();
        let mut prefixed = vec![0x78, 0, 0, 0];
        prefixed.extend_from_slice(&table);
        let chunks = vec![chunk(0x16, 1, 0, &prefixed)];
        let (document, warnings) = collect(&chunks);
        assert!(warnings.is_empty());
        assert_eq!(document.color_table.len(), 2);
        assert_eq!(document.color_table[1].index, 25);
    }

    #[test]
    fn a_name_index_123_maps_name_id_pairs() {
        let sheet = style_sheet(u32::MAX, u32::MAX, u32::MAX);
        let name = name2("Normal");
        let mut index = Vec::new();
        index.extend_from_slice(&3u32.to_le_bytes());
        index.extend_from_slice(&4u32.to_le_bytes());
        let chunks = vec![
            chunk(0x33, 1, 3, &name),
            chunk(0x1a, 1, 0, &[]),
            chunk(0x4a, 2, 4, &sheet),
            chunk(0x34, 3, 0, &index),
        ];
        let (document, _) = collect(&chunks);
        assert_eq!(document.stylesheet.styles[0].name, "Normal");
    }

    #[test]
    fn a_name_record_fills_an_id_the_document_list_did_not_name() {
        let sheet = style_sheet(u32::MAX, u32::MAX, u32::MAX);
        let mut name = Vec::new();
        for unit in "Row_1".encode_utf16().chain(std::iter::once(0)) {
            name.extend_from_slice(&unit.to_le_bytes());
        }
        let index = name_index(7, 4);
        let chunks = vec![
            chunk(0x2d, 1, 7, &name),
            chunk(0x1a, 1, 0, &[]),
            chunk(0x4a, 2, 4, &sheet),
            chunk(0xc9, 3, 0, &index),
        ];
        let (document, _) = collect(&chunks);
        assert_eq!(document.stylesheet.styles[0].name, "Row_1");
    }

    #[test]
    fn a_colour_count_beyond_the_payload_warns() {
        let chunks = vec![chunk(0x16, 1, 0, &[0, 0, 255, 0])];
        let (document, warnings) = collect(&chunks);
        assert!(document.color_table.is_empty());
        assert_eq!(warnings.len(), 1);
        assert!(matches!(warnings[0].kind, WarningKind::BadValue { .. }));
    }

    #[test]
    fn font_records_are_dropped_with_a_warning() {
        let font = [0u8; 12];
        let chunks = vec![
            chunk(0xd8, 1, 0, &[]),
            chunk(0xd7, 2, 1, &font),
            chunk(0x18, 1, 0, &[]),
            chunk(0x19, 2, 1, &font),
        ];
        let (_, warnings) = collect(&chunks);
        assert_eq!(warnings.len(), 4);
        assert!(
            warnings
                .iter()
                .all(|warning| matches!(warning.kind, WarningKind::Dropped { .. }))
        );
    }

    #[test]
    fn unknown_style_records_warn_and_are_skipped() {
        let sheet = style_sheet(u32::MAX, u32::MAX, u32::MAX);
        let chunks = style_chunks(&sheet, &[(0x1234, 9, &[1, 2, 3])]);
        let (document, warnings) = collect(&chunks);
        assert_eq!(document.stylesheet.styles.len(), 1);
        assert_eq!(warnings.len(), 1);
        assert!(matches!(
            warnings[0].kind,
            WarningKind::SkippedRecord {
                name: "style record"
            }
        ));
    }

    #[test]
    fn an_index_outside_the_model_patterns_is_dropped() {
        let sheet = style_sheet(u32::MAX, u32::MAX, u32::MAX);
        let line = line_record(0.1, [0, 0, 0, 0], 200);
        let fill = fill_record(
            (0, [0, 0, 0, 0]),
            (0, [0, 0, 0, 0]),
            200,
            (0, [0, 0, 0, 0]),
            200,
        );
        let chunks = style_chunks(&sheet, &[(0x85, 1, &line), (0x86, 2, &fill)]);
        let (document, warnings) = collect(&chunks);
        let style = &document.stylesheet.styles[0];
        assert_eq!(value(&style.sheet, "LinePattern"), None);
        assert_eq!(value(&style.sheet, "FillPattern"), None);
        assert_eq!(value(&style.sheet, "ShdwPattern"), None);
        assert_eq!(warnings.len(), 3);
        assert!(
            warnings
                .iter()
                .all(|warning| matches!(warning.kind, WarningKind::Dropped { .. }))
        );
    }

    #[test]
    fn a_non_finite_value_warns_and_leaves_the_cell_unset() {
        let sheet = style_sheet(u32::MAX, u32::MAX, u32::MAX);
        let mut line = line_record(0.1, [0, 0, 0, 0], 1);
        line[1..9].copy_from_slice(&f64::INFINITY.to_le_bytes());
        let chunks = style_chunks(&sheet, &[(0x85, 1, &line)]);
        let (document, warnings) = collect(&chunks);
        let style = &document.stylesheet.styles[0];
        assert_eq!(value(&style.sheet, "LineWeight"), None);
        assert_eq!(warnings.len(), 1);
        assert!(matches!(
            warnings[0].kind,
            WarningKind::BadValue {
                what: "line weight"
            }
        ));
    }

    #[test]
    fn short_payloads_warn_and_never_panic() {
        let short = [0u8; 3];
        let chunks = vec![
            chunk(0x1a, 1, 0, &[]),
            chunk(0x4a, 2, 4, &short),
            chunk(0x85, 4, 1, &short),
            chunk(0x86, 4, 2, &short),
            chunk(0x87, 4, 3, &short),
            chunk(0x94, 4, 0, &short),
            chunk(0x95, 4, 0, &short),
            chunk(0x16, 1, 0, &short),
        ];
        let (document, warnings) = collect(&chunks);
        assert_eq!(document.stylesheet.styles.len(), 1);
        assert!(warnings.len() >= 7);
        assert!(document.color_table.is_empty());
    }

    #[test]
    fn a_style_sheet_without_the_styles_stream_marker_still_maps() {
        let sheet = style_sheet(u32::MAX, u32::MAX, u32::MAX);
        let character = character_record();
        let chunks = vec![chunk(0x4a, 2, 4, &sheet), chunk(0x94, 4, 0, &character)];
        let (document, _) = collect(&chunks);
        assert_eq!(document.stylesheet.styles.len(), 1);
        assert_eq!(
            row_value(&document.stylesheet.styles[0].sheet, "Character", 0, "Size"),
            number(0.125)
        );
    }

    #[test]
    fn a_version_6_fill_record_has_no_shadow_offsets() {
        let sheet = style_sheet(u32::MAX, u32::MAX, u32::MAX);
        let mut fill = fill_record(
            (0, [0, 0, 0, 0]),
            (0, [0, 0, 0, 0]),
            1,
            (0, [0, 0, 0, 0]),
            0,
        );
        fill.truncate(22);
        let chunks = style_chunks(&sheet, &[(0x86, 2, &fill)]);
        let (document, warnings) = collect(&chunks);
        let style = &document.stylesheet.styles[0];
        assert!(warnings.is_empty());
        assert_eq!(value(&style.sheet, "ShapeShdwOffsetX"), None);
        assert_eq!(value(&style.sheet, "ShapeShdwOffsetY"), None);
    }

    #[test]
    fn the_mapped_style_matches_the_vsdx_reader_spelling() {
        let sheet = style_sheet(3, 4, 6);
        let line = line_record(0.25, [0, 0, 0, 0], 1);
        let chunks = style_chunks(&sheet, &[(0x85, 1, &line)]);
        let (document, _) = collect(&chunks);
        let style = &document.stylesheet.styles[0];

        // The values the `.vsdx` reader produces for the equivalent
        // `StyleSheet` row: number cells for an unqualified literal and a
        // `Color` for a `#RRGGBB` cell.
        let mut expected = Sheet::new();
        for (name, value) in [
            ("LineWeight", CellValue::Number(0.25)),
            ("LineColor", CellValue::Color(Color::new(0, 0, 0))),
            ("LineColorTrans", CellValue::Number(0.0)),
            ("LinePattern", CellValue::Number(1.0)),
            ("Rounding", CellValue::Number(0.25)),
            ("BeginArrow", CellValue::Number(2.0)),
            ("EndArrow", CellValue::Number(3.0)),
            ("LineCap", CellValue::Number(1.0)),
        ] {
            let mut cell = Cell::new(CellName::from_static(name));
            cell.value = Some(value);
            expected.cells.push(cell);
        }
        assert_eq!(style.sheet, expected);
        assert_eq!(style.line_style, Some(3));
        assert_eq!(style.fill_style, Some(4));
        assert_eq!(style.text_style, Some(6));
    }
}
