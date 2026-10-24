// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Shape text: text, character, paragraph and field records into the
//! model (ADR 0070).
//!
//! This module reads the text records (the text string itself, the
//! character and paragraph lists and their index records, the field
//! list and text fields) and maps them onto
//! [`larrygram_model::TextContent`]: literal text to
//! `TextPart::Text`, run boundaries to `CharRun`, `ParaRun` and
//! `TabsRun` markers, fields to `TextPart::Field` with their cached
//! display text (ADR 0019). The character and paragraph index records
//! become the `Character` and `Paragraph` rows of the owning shape's
//! sheet; the tab sets become the `Tabs` rows.
//!
//! # Record layouts
//!
//! The record ids and payload layouts are derived from the libvisio
//! reference (AGENTS.md section 5; the document structure table and the
//! text readers), so this file is MPL-2.0 like the rest of the crate
//! (ADR 0003). Two layouts differ between the version 6 and the
//! version 11 chunk format ([`FormatVersion`]): the strings are byte
//! strings in version 6 and UTF-16 in version 11, and the paragraph
//! record carries the bullet font fields only in version 11. Fixed
//! record fields are read; the trailing per-cell blocks the format
//! carries are not interpreted (the reference ignores them too).
//!
//! # Runs and markers
//!
//! The character, paragraph and tab runs carry the number of characters
//! they cover; the run boundaries become `CharRun`, `ParaRun` and
//! `TabsRun` markers at those character positions, each referencing the
//! `IX` of its row: the row's position in its list order for character
//! and paragraph runs, the row of the run's first tab stop for tab
//! runs. A field placeholder in the text becomes `TextPart::Field`
//! followed by the field's cached display text, the same parts the
//! `.vsdx` reader builds for a `fld` marker (`crates/vsdx/src/read/text.rs`).
//!
//! # Code pages
//!
//! Byte strings decode with the stream's code page ([`CodePage`]);
//! UTF-16 strings are read as they are. A byte sequence that does not
//! decode keeps its text lossy (U+FFFD replacements) and records a
//! `BadValue` warning; it never fails the import (ADR 0071). In byte
//! strings `0x0d` and `0x0e` are paragraph breaks like `0x0a` and
//! decode to `\n`, and `0x1e` is a field placeholder (U+FFFC).
//!
//! # Scope
//!
//! Character and paragraph records under a stylesheet are style rows:
//! [`crate::style`] maps those and this module skips them silently.
//! Text records outside any shape (for example on a page sheet) are
//! dropped with a warning.

use std::collections::HashMap;

use larrygram_geom::Unit;
use larrygram_model::{
    Cell, CellName, CellValue, Color, Document, Row, Section, SectionName, Shape, ShapeId,
    TextContent, TextPart,
};

use crate::chunks::{Chunk, FormatVersion, Record};
use crate::error::Error;
use crate::limits::Limits;
use crate::warning::{ImportWarning, WarningKind};

// Record ids (derived from the libvisio reference's document structure
// table; AGENTS.md section 5). Each id is one record kind of the
// `VisioDocument` stream.
/// The text string of a shape.
const TEXT: Record = Record(0x0e);
/// The field list of a shape.
const FIELD_LIST: Record = Record(0x66);
/// The character list of a shape.
const CHAR_LIST: Record = Record(0x69);
/// The paragraph list of a shape.
const PARA_LIST: Record = Record(0x6a);
/// The tabs list of a shape.
const TABS_DATA_LIST: Record = Record(0x6b);
/// A group shape.
const SHAPE_GROUP: Record = Record(0x47);
/// A leaf shape.
const SHAPE_SHAPE: Record = Record(0x48);
/// A guide shape.
const SHAPE_GUIDE: Record = Record(0x4d);
/// A foreign (picture) shape.
const SHAPE_FOREIGN: Record = Record(0x4e);
/// The stylesheet stream marker; rows below it are style rows.
const STYLES: Record = Record(0x1a);
/// A stylesheet; rows below it are style rows.
const STYLE_SHEET: Record = Record(0x4a);
/// One character run (`CharIX`).
const CHAR_IX: Record = Record(0x94);
/// One paragraph run (`ParaIX`).
const PARA_IX: Record = Record(0x95);
/// One tab set, first spelling.
const TABS_DATA_1: Record = Record(0x88);
/// One tab set, second spelling.
const TABS_DATA_2: Record = Record(0x96);
/// One tab set, third spelling.
const TABS_DATA_3: Record = Record(0x97);
/// One tab set, fourth spelling.
const TABS_DATA_4: Record = Record(0xb5);
/// One text field (`TextField`).
const TEXT_FIELD: Record = Record(0xa1);
/// One name-list name (`Name2`), the strings fields reference.
const NAME2: Record = Record(0x33);
/// A font with charset, version 6 spelling (`FontIX`).
const FONT_IX: Record = Record(0x19);
/// A font face, version 11 spelling (`FontFace`).
const FONTFACE: Record = Record(0xd7);

/// The code page the byte strings of one stream are stored in.
///
/// Windows-1252 is the ANSI default of the format; code page 65001 is
/// UTF-8 (the `fdo86729` corpus pair stores the same strings in the two
/// spellings). UTF-16 strings ignore the code page: they are read as
/// they are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(dead_code)] // constructed by P8-07 from the stream's code page
pub enum CodePage {
    /// Windows-1252 (the ANSI default).
    Windows1252,
    /// UTF-8 (Windows code page 65001).
    Utf8,
}

impl CodePage {
    /// The code page named by a Windows code page number.
    ///
    /// `0` is the ANSI default. Returns `None` for a code page this
    /// module cannot decode; the caller records an `Unsupported`
    /// warning and falls back to [`CodePage::Windows1252`].
    #[allow(dead_code)] // no caller until P8-07 wires the pipeline
    pub fn from_windows_id(id: u32) -> Option<CodePage> {
        match id {
            65001 => Some(CodePage::Utf8),
            1252 | 0 => Some(CodePage::Windows1252),
            _ => None,
        }
    }
}

impl Default for CodePage {
    /// The ANSI default of the format.
    fn default() -> Self {
        CodePage::Windows1252
    }
}

/// The options of [`collect_with`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)] // no caller until P8-07 wires the pipeline
pub struct TextOptions {
    /// The chunk format version: the string and paragraph record
    /// layouts differ between version 6 and version 11.
    pub version: FormatVersion,
    /// The code page of the byte strings of the stream.
    pub code_page: CodePage,
    /// The input caps to enforce ([`Limits::max_text_bytes`]).
    pub limits: Limits,
}

impl Default for TextOptions {
    /// Version 11, the ANSI code page, the default caps.
    fn default() -> Self {
        Self {
            version: FormatVersion::V11,
            code_page: CodePage::default(),
            limits: Limits::DEFAULT,
        }
    }
}

/// Maps every text, character, paragraph and field record in `chunks`
/// onto the shapes of `document`.
///
/// Character and paragraph records become the `Character` and
/// `Paragraph` rows of the owning shape's sheet; run markers reference
/// their row `IX`. Text the model cannot represent is recorded in
/// `warnings`, never fatal. The record layouts are read as version 11
/// and byte strings as Windows-1252; use [`collect_with`] for other
/// streams.
#[allow(dead_code)] // no caller until P8-07 wires the pipeline
pub fn collect(
    chunks: &[Chunk<'_>],
    document: &mut Document,
    warnings: &mut Vec<ImportWarning>,
) -> Result<(), Error> {
    collect_with(chunks, document, warnings, &TextOptions::default())
}

/// Maps the text records like [`collect`], with the stream's format
/// version, code page and caps.
///
/// The chunk reader hands this module the records of one stream; the
/// version and code page are properties of that stream that the frozen
/// [`collect`] seam of P8-01 cannot carry, so they travel in
/// [`TextOptions`] instead.
#[allow(dead_code)] // no caller until P8-07 wires the pipeline
pub fn collect_with(
    chunks: &[Chunk<'_>],
    document: &mut Document,
    warnings: &mut Vec<ImportWarning>,
    options: &TextOptions,
) -> Result<(), Error> {
    let mut fonts = HashMap::new();
    let mut names = HashMap::new();
    for chunk in chunks {
        match chunk.record {
            FONT_IX | FONTFACE => {
                if let Some(name) = parse_font(chunk.data, options, warnings) {
                    fonts.insert(chunk.id, name);
                }
            }
            NAME2 => {
                if let Some(name) = parse_name(chunk.data, options, warnings) {
                    names.insert(chunk.id, name);
                }
            }
            _ => {}
        }
    }

    let mut shapes: Vec<(ShapeId, ShapeText)> = Vec::new();
    let mut frames: Vec<Frame> = Vec::new();
    let mut shape_cursor: usize = 0;
    let mut text_bytes: u64 = 0;
    for chunk in chunks {
        while frames
            .last()
            .is_some_and(|frame| frame.level >= chunk.level)
        {
            frames.pop();
        }
        if is_shape_record(chunk.record) {
            let ordinal = shapes.len();
            let owner = u32::try_from(shape_cursor)
                .ok()
                .map(|index| ShapeId { index })
                .filter(|id| document.shape(*id).is_some())
                .map(|id| {
                    shapes.push((id, ShapeText::new(ordinal)));
                    ordinal
                });
            shape_cursor = shape_cursor.saturating_add(1);
            frames.push(Frame {
                level: chunk.level,
                record: chunk.record,
                owner,
            });
            continue;
        }
        if chunk.record == STYLES || chunk.record == STYLE_SHEET {
            frames.push(Frame {
                level: chunk.level,
                record: chunk.record,
                owner: None,
            });
            continue;
        }
        if !is_text_record(chunk.record) {
            continue;
        }
        if in_style(&frames) {
            // Character and paragraph rows below a stylesheet are style
            // rows; `crate::style` maps them.
            continue;
        }
        let Some(owner) = owner_of(&frames) else {
            warnings.push(ImportWarning {
                context: "text record without a shape".to_owned(),
                kind: WarningKind::Dropped { what: "shape text" },
            });
            continue;
        };
        // The owner exists by construction; the loop over the one
        // lookup avoids an untaken `else` branch (and a panic path).
        #[allow(for_loops_over_fallibles)]
        for (_, shape) in shapes.get_mut(owner) {
            shape.read(chunk, options, warnings, &mut text_bytes)?;
        }
    }

    for (id, shape) in &shapes {
        #[allow(for_loops_over_fallibles)]
        for target in document.shape_mut(*id) {
            shape.flush(target, &fonts, &names, options, warnings, &mut text_bytes)?;
        }
    }
    Ok(())
}

/// One shape or stylesheet record on the walk's stack.
struct Frame {
    /// The nesting level of the record.
    level: u16,
    /// The record kind.
    record: Record,
    /// The index of the owning shape accumulator, or `None`.
    owner: Option<usize>,
}

/// Whether `record` opens a shape.
fn is_shape_record(record: Record) -> bool {
    record == SHAPE_GROUP
        || record == SHAPE_SHAPE
        || record == SHAPE_GUIDE
        || record == SHAPE_FOREIGN
}

/// Whether `record` belongs to the text family this module maps.
fn is_text_record(record: Record) -> bool {
    matches!(
        record,
        TEXT | FIELD_LIST
            | CHAR_LIST
            | PARA_LIST
            | TABS_DATA_LIST
            | CHAR_IX
            | PARA_IX
            | TABS_DATA_1
            | TABS_DATA_2
            | TABS_DATA_3
            | TABS_DATA_4
            | TEXT_FIELD
    )
}

/// Whether any frame of the walk is a stylesheet context.
fn in_style(frames: &[Frame]) -> bool {
    frames
        .iter()
        .any(|frame| frame.record == STYLES || frame.record == STYLE_SHEET)
}

/// The owning shape accumulator: the nearest enclosing shape record's.
fn owner_of(frames: &[Frame]) -> Option<usize> {
    frames
        .iter()
        .rev()
        .find(|frame| is_shape_record(frame.record))
        .and_then(|frame| frame.owner)
}

/// A cursor over one record payload; every read is bounds-checked and
/// a short payload ends the parse (never a panic).
struct Cursor<'a> {
    /// The payload bytes.
    bytes: &'a [u8],
    /// The current offset.
    pos: usize,
}

impl<'a> Cursor<'a> {
    /// A cursor at the start of `bytes`.
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    /// One byte, or `None` at the end of the payload.
    fn u8(&mut self) -> Option<u8> {
        let byte = *self.bytes.get(self.pos)?;
        self.pos = self.pos.saturating_add(1);
        Some(byte)
    }

    /// One little-endian 16-bit word.
    fn u16(&mut self) -> Option<u16> {
        let end = self.pos.checked_add(2)?;
        let bytes = self.bytes.get(self.pos..end)?;
        let mut word = [0u8; 2];
        for (slot, byte) in word.iter_mut().zip(bytes.iter()) {
            *slot = *byte;
        }
        self.pos = end;
        Some(u16::from_le_bytes(word))
    }

    /// One little-endian 32-bit word.
    fn u32(&mut self) -> Option<u32> {
        let end = self.pos.checked_add(4)?;
        let bytes = self.bytes.get(self.pos..end)?;
        let mut word = [0u8; 4];
        for (slot, byte) in word.iter_mut().zip(bytes.iter()) {
            *slot = *byte;
        }
        self.pos = end;
        Some(u32::from_le_bytes(word))
    }

    /// One little-endian signed 32-bit word.
    fn i32(&mut self) -> Option<i32> {
        self.u32().map(|word| word as i32)
    }

    /// One little-endian IEEE-754 double.
    fn f64(&mut self) -> Option<f64> {
        let end = self.pos.checked_add(8)?;
        let bytes = self.bytes.get(self.pos..end)?;
        let mut word = [0u8; 8];
        for (slot, byte) in word.iter_mut().zip(bytes.iter()) {
            *slot = *byte;
        }
        self.pos = end;
        Some(f64::from_le_bytes(word))
    }

    /// A finite double, or `None` when the payload ends or the stored
    /// value is NaN or infinite (a malformed value never reaches the
    /// model).
    fn finite(&mut self) -> Option<f64> {
        let value = self.f64()?;
        value.is_finite().then_some(value)
    }

    /// Skips `count` bytes.
    fn skip(&mut self, count: usize) -> Option<()> {
        let end = self.pos.checked_add(count)?;
        self.bytes.get(self.pos..end)?;
        self.pos = end;
        Some(())
    }

    /// The bytes from the cursor to the end of the payload.
    fn rest(&self) -> &'a [u8] {
        self.bytes.get(self.pos..).unwrap_or(&[])
    }
}

/// A decoded string and whether any input did not decode.
struct Decoded {
    /// The decoded text.
    text: String,
    /// Whether some input was replaced with U+FFFD.
    lossy: bool,
}

/// The Windows-1252 character of one byte, or `None` where the code
/// page leaves the byte undefined.
fn cp1252_char(byte: u8) -> Option<char> {
    match byte {
        0x00..=0x7f | 0xa0..=0xff => Some(char::from(byte)),
        0x80 => Some('\u{20ac}'),
        0x82 => Some('\u{201a}'),
        0x83 => Some('\u{0192}'),
        0x84 => Some('\u{201e}'),
        0x85 => Some('\u{2026}'),
        0x86 => Some('\u{2020}'),
        0x87 => Some('\u{2021}'),
        0x88 => Some('\u{02c6}'),
        0x89 => Some('\u{2030}'),
        0x8a => Some('\u{0160}'),
        0x8b => Some('\u{2039}'),
        0x8c => Some('\u{0152}'),
        0x8e => Some('\u{017d}'),
        0x91 => Some('\u{2018}'),
        0x92 => Some('\u{2019}'),
        0x93 => Some('\u{201c}'),
        0x94 => Some('\u{201d}'),
        0x95 => Some('\u{2022}'),
        0x96 => Some('\u{2013}'),
        0x97 => Some('\u{2014}'),
        0x98 => Some('\u{02dc}'),
        0x99 => Some('\u{2122}'),
        0x9a => Some('\u{0161}'),
        0x9b => Some('\u{203a}'),
        0x9c => Some('\u{0153}'),
        0x9e => Some('\u{017e}'),
        0x9f => Some('\u{0178}'),
        // 0x81, 0x8d, 0x8f, 0x90 and 0x9d are undefined.
        _ => None,
    }
}

/// Drops the trailing NUL bytes of a byte string.
fn trimmed(bytes: &[u8]) -> &[u8] {
    let mut end = bytes.len();
    while end > 1 && bytes.get(end.wrapping_sub(1)) == Some(&0) {
        end = end.wrapping_sub(1);
    }
    bytes.get(..end).unwrap_or(&[])
}

/// Decodes one byte string with `code_page`.
///
/// Trailing NUL bytes are dropped. `0x0d` and `0x0e` are paragraph
/// breaks like `0x0a` and decode to `\n`; `0x1e` is a field placeholder
/// and decodes to U+FFFC. Bytes that do not decode become U+FFFD and
/// set `lossy`.
fn decode_bytes(bytes: &[u8], code_page: CodePage) -> Decoded {
    let (mut text, lossy) = match code_page {
        CodePage::Utf8 => decode_utf8(trimmed(bytes)),
        CodePage::Windows1252 => decode_cp1252(trimmed(bytes)),
    };
    text = text
        .replace(&['\r', '\u{0e}'][..], "\n")
        .replace('\u{1e}', "\u{fffc}");
    Decoded { text, lossy }
}

/// Decodes one byte string as Windows-1252.
fn decode_cp1252(bytes: &[u8]) -> (String, bool) {
    let mut text = String::new();
    let mut lossy = false;
    for byte in bytes {
        match cp1252_char(*byte) {
            Some(character) => text.push(character),
            None => {
                text.push('\u{fffd}');
                lossy = true;
            }
        }
    }
    (text, lossy)
}

/// Decodes one UTF-16LE string. Unpaired surrogates and an odd trailing
/// byte become U+FFFD and set `lossy`.
fn decode_utf16(bytes: &[u8]) -> Decoded {
    let mut text = String::new();
    let mut lossy = false;
    let mut units = Vec::with_capacity(bytes.len() / 2);
    let mut index = 0usize;
    while let (Some(high), Some(low)) = (bytes.get(index), bytes.get(index.saturating_add(1))) {
        units.push(u16::from_le_bytes([*high, *low]));
        index = index.saturating_add(2);
    }
    for unit in char::decode_utf16(units.iter().copied()) {
        match unit {
            Ok(character) => text.push(character),
            Err(_) => {
                text.push('\u{fffd}');
                lossy = true;
            }
        }
    }
    if bytes.len() % 2 == 1 {
        text.push('\u{fffd}');
        lossy = true;
    }
    Decoded { text, lossy }
}

/// Decodes UTF-8 bytes lossily, reporting whether any byte was replaced.
fn decode_utf8(bytes: &[u8]) -> (String, bool) {
    let lossy = std::str::from_utf8(bytes).is_err();
    (String::from_utf8_lossy(bytes).into_owned(), lossy)
}

/// The string of one record payload, in the stream's spellings.
fn decode_string(bytes: &[u8], options: &TextOptions) -> Decoded {
    match options.version {
        FormatVersion::V6 => decode_bytes(bytes, options.code_page),
        FormatVersion::V11 => decode_utf16(bytes),
    }
}

/// Counts `text` against the `max_text_bytes` cap.
fn account(text: &str, text_bytes: &mut u64, limits: &Limits) -> Result<(), Error> {
    let total = text_bytes.saturating_add(text.len() as u64);
    if total > limits.max_text_bytes {
        return Err(Error::LimitExceeded {
            limit: "max_text_bytes",
            value: total,
            max: limits.max_text_bytes,
        });
    }
    *text_bytes = total;
    Ok(())
}

/// Records a `BadValue` warning when `decoded` did not decode cleanly.
fn note_lossy(decoded: &Decoded, what: &'static str, warnings: &mut Vec<ImportWarning>) {
    if decoded.lossy {
        warnings.push(ImportWarning {
            context: "shape text".to_owned(),
            kind: WarningKind::BadValue { what },
        });
    }
}

/// One character run (`CharIX`; version 6 and 11 share the layout).
struct CharRow {
    /// The number of characters the run covers.
    char_count: u32,
    /// The font id into the font list.
    font_id: u16,
    /// The font colour.
    color: Color,
    /// The colour transparency (`0` = opaque).
    alpha: u8,
    /// Bold, italic, underline and small caps as the `Style` cell bits.
    style: u8,
    /// The `Case` cell (0 normal, 1 all caps, 2 initial caps).
    case: u8,
    /// The `Pos` cell (0 normal, 1 superscript, 2 subscript).
    pos: u8,
    /// Double underline.
    double_underline: bool,
    /// Strikethrough.
    strikeout: bool,
    /// Double strikethrough.
    double_strikeout: bool,
    /// The `FontScale` cell (the stored factor over 10000).
    scale_width: f64,
    /// The `Size` cell, in inches.
    size: f64,
}

/// Parses one `CharIX` payload; `None` when the payload is cut short.
fn parse_char(data: &[u8], warnings: &mut Vec<ImportWarning>) -> Option<CharRow> {
    let mut cursor = Cursor::new(data);
    let char_count = cursor.u32()?;
    let font_id = cursor.u16()?;
    cursor.skip(1)?;
    let r = cursor.u8()?;
    let g = cursor.u8()?;
    let b = cursor.u8()?;
    let alpha = cursor.u8()?;
    let mod1 = cursor.u8()?;
    let mod2 = cursor.u8()?;
    let mod3 = cursor.u8()?;
    let scale_width = f64::from(cursor.u16()?) / 10000.0;
    cursor.skip(2)?;
    let size = cursor.finite();
    if size.is_none() {
        warnings.push(ImportWarning {
            context: "shape text / character run".to_owned(),
            kind: WarningKind::BadValue { what: "font size" },
        });
    }
    let mod4 = cursor.u8()?;
    Some(CharRow {
        char_count,
        font_id,
        color: Color::new(r, g, b),
        alpha,
        style: mod1 & 0x0f,
        case: if mod2 & 0x01 != 0 {
            1
        } else if mod2 & 0x02 != 0 {
            2
        } else {
            0
        },
        pos: if mod3 & 0x01 != 0 {
            1
        } else if mod3 & 0x02 != 0 {
            2
        } else {
            0
        },
        double_underline: mod4 & 0x01 != 0,
        strikeout: mod4 & 0x04 != 0,
        double_strikeout: mod4 & 0x20 != 0,
        scale_width,
        size: size.unwrap_or(0.0),
    })
}

/// One paragraph run (`ParaIX`); the bullet font fields exist only in
/// the version 11 layout.
struct ParaRow {
    /// The number of characters the run covers.
    char_count: u32,
    /// The `IndFirst` cell, in inches.
    ind_first: f64,
    /// The `IndLeft` cell, in inches.
    ind_left: f64,
    /// The `IndRight` cell, in inches.
    ind_right: f64,
    /// The `SpLine` cell (negative = a multiple of the line height).
    sp_line: f64,
    /// The `SpBefore` cell, in inches.
    sp_before: f64,
    /// The `SpAfter` cell, in inches.
    sp_after: f64,
    /// The `HorzAlign` cell.
    align: u8,
    /// The `Bullet` cell.
    bullet: u8,
    /// The `BulletFont` cell (the font id).
    bullet_font: u16,
    /// The `BulletFontSize` cell, in inches.
    bullet_font_size: f64,
    /// The `TextPosAfterBullet` cell, in inches.
    text_pos_after_bullet: f64,
    /// The `Flags` cell.
    flags: u32,
    /// The `BulletStr` cell.
    bullet_string: Option<String>,
}

/// Parses one `ParaIX` payload; `None` when the payload is cut short.
fn parse_para(
    data: &[u8],
    options: &TextOptions,
    warnings: &mut Vec<ImportWarning>,
) -> Option<ParaRow> {
    let mut cursor = Cursor::new(data);
    let mut bad = false;
    let mut value = |cursor: &mut Cursor<'_>| match cursor.finite() {
        Some(value) => value,
        None => {
            bad = true;
            0.0
        }
    };
    let char_count = cursor.u32()?;
    cursor.skip(1)?;
    let ind_first = value(&mut cursor);
    cursor.skip(1)?;
    let ind_left = value(&mut cursor);
    cursor.skip(1)?;
    let ind_right = value(&mut cursor);
    cursor.skip(1)?;
    let sp_line = value(&mut cursor);
    cursor.skip(1)?;
    let sp_before = value(&mut cursor);
    cursor.skip(1)?;
    let sp_after = value(&mut cursor);
    let align = cursor.u8()?;
    let bullet = cursor.u8()?;
    cursor.skip(4)?;
    let (bullet_font, bullet_font_size, text_pos_after_bullet) = match options.version {
        FormatVersion::V11 => {
            let font = cursor.u16()?;
            cursor.skip(2)?;
            let size = value(&mut cursor);
            cursor.skip(1)?;
            let after = value(&mut cursor);
            (font, size, after)
        }
        FormatVersion::V6 => (0, 0.0, 0.0),
    };
    let flags = cursor.u32()?;
    let skip = match options.version {
        FormatVersion::V11 => 34,
        FormatVersion::V6 => 5,
    };
    cursor.skip(skip)?;
    if bad {
        warnings.push(ImportWarning {
            context: "shape text / paragraph run".to_owned(),
            kind: WarningKind::BadValue {
                what: "paragraph value",
            },
        });
    }
    let bullet_string = parse_para_blocks(cursor.rest(), options, warnings);
    Some(ParaRow {
        char_count,
        ind_first,
        ind_left,
        ind_right,
        sp_line,
        sp_before,
        sp_after,
        align,
        bullet,
        bullet_font,
        bullet_font_size,
        text_pos_after_bullet,
        flags,
        bullet_string,
    })
}

/// Reads the trailing blocks of a paragraph record: the bullet string
/// lives in the block of type 2 index 8.
fn parse_para_blocks(
    bytes: &[u8],
    options: &TextOptions,
    warnings: &mut Vec<ImportWarning>,
) -> Option<String> {
    let mut cursor = Cursor::new(bytes);
    let mut bullet = None;
    loop {
        let start = cursor.pos;
        let Some(length) = cursor.u32() else {
            break;
        };
        if length == 0 {
            break;
        }
        let length = usize::try_from(length).unwrap_or(usize::MAX);
        let Some(block_type) = cursor.u8() else {
            break;
        };
        let Some(block_index) = cursor.u8() else {
            break;
        };
        if block_type == 2 && block_index == 8 {
            if cursor.skip(1).is_none() {
                break;
            }
            let Some(count) = cursor.u8() else {
                break;
            };
            let count = match options.version {
                FormatVersion::V11 => usize::from(count).saturating_mul(2),
                FormatVersion::V6 => usize::from(count),
            };
            match cursor.rest().get(..count) {
                Some(bytes) => {
                    let decoded = decode_string(bytes, options);
                    note_lossy(&decoded, "bullet string", warnings);
                    bullet = Some(decoded.text);
                }
                None => break,
            }
        }
        match start.checked_add(length) {
            Some(next) if cursor.bytes.get(next..).is_some() => cursor.pos = next,
            _ => break,
        }
    }
    bullet
}

/// One tab set (`TabsData`): a run of characters with its tab stops.
struct TabSet {
    /// The number of characters the run covers.
    char_count: u32,
    /// The tab stops, in order.
    stops: Vec<TabStop>,
}

/// One tab stop.
struct TabStop {
    /// The position, in inches.
    position: f64,
    /// The alignment (0 left, 1 centre, 2 right, 3 decimal).
    alignment: u8,
}

/// Parses one `TabsData` payload; `None` when the payload is cut short.
fn parse_tabs(data: &[u8], warnings: &mut Vec<ImportWarning>) -> Option<TabSet> {
    let mut cursor = Cursor::new(data);
    let char_count = cursor.u32()?;
    let num_stops = cursor.u8()?;
    let mut stops = Vec::new();
    let mut bad = false;
    for _ in 0..num_stops {
        cursor.skip(1)?;
        let position = match cursor.finite() {
            Some(position) => position,
            None => {
                bad = true;
                0.0
            }
        };
        let alignment = cursor.u8()?;
        let _leader = cursor.u8()?;
        stops.push(TabStop {
            position,
            alignment,
        });
    }
    if bad {
        warnings.push(ImportWarning {
            context: "shape text / tab set".to_owned(),
            kind: WarningKind::BadValue {
                what: "tab position",
            },
        });
    }
    Some(TabSet { char_count, stops })
}

/// One text field (`TextField`).
enum Field {
    /// A field whose cached display text is a name-list string.
    Text {
        /// The id of the name.
        name_id: i32,
    },
    /// A numeric field cached with a value and a display format.
    Numeric {
        /// The cached value (dates are Visio serials, lengths inches).
        value: f64,
        /// The cell type of the value.
        cell_type: u8,
        /// The display format number.
        format: u16,
    },
}

/// Parses one `TextField` payload; `None` when the payload is cut short.
fn parse_field(data: &[u8], options: &TextOptions) -> Option<Field> {
    let mut cursor = Cursor::new(data);
    cursor.skip(7)?;
    let cell_type = cursor.u8()?;
    if cell_type == CELL_TYPE_STRING_WITHOUT_UNIT {
        let name_id = cursor.i32()?;
        return Some(Field::Text { name_id });
    }
    let value = cursor.finite()?;
    cursor.skip(2)?;
    let scan_start = match options.version {
        FormatVersion::V11 => 0x36,
        FormatVersion::V6 => 0x24,
    };
    let format = scan_field_format(data, scan_start).unwrap_or(if cell_type == CELL_TYPE_DATE {
        FORMAT_MSO_DATE_SHORT
    } else {
        FORMAT_UNKNOWN
    });
    Some(Field::Numeric {
        value,
        cell_type,
        format,
    })
}

/// The format number from the trailing blocks of a numeric field
/// record: the block of type 2 index 2 carries it.
fn scan_field_format(data: &[u8], scan_start: usize) -> Option<u16> {
    let mut cursor = Cursor::new(data);
    cursor.skip(scan_start)?;
    loop {
        let start = cursor.pos;
        let length = cursor.u32()?;
        if length == 0 {
            return None;
        }
        let length = usize::try_from(length).unwrap_or(usize::MAX);
        cursor.skip(1)?;
        let block_index = cursor.u8()?;
        if block_index == 2 {
            cursor.skip(1)?;
            let format = cursor.u16()?;
            if cursor.u8()? == 0x80 && cursor.u8()? == 0xc2 {
                return Some(format);
            }
        }
        match start.checked_add(length) {
            Some(next) if data.get(next..).is_some() => cursor.pos = next,
            _ => return None,
        }
    }
}

// Cell types of the field records (the unit table of the libvisio
// reference's document structure).
const CELL_TYPE_PERCENT: u8 = 33;
const CELL_TYPE_ACRE: u8 = 36;
const CELL_TYPE_HECTARE: u8 = 37;
const CELL_TYPE_DATE: u8 = 40;
const CELL_TYPE_ELAPSED_WEEK: u8 = 43;
const CELL_TYPE_ELAPSED_DAY: u8 = 44;
const CELL_TYPE_ELAPSED_HOUR: u8 = 45;
const CELL_TYPE_ELAPSED_MIN: u8 = 46;
const CELL_TYPE_ELAPSED_SEC: u8 = 47;
const CELL_TYPE_POINTS: u8 = 50;
const CELL_TYPE_PICAS: u8 = 51;
const CELL_TYPE_DIDOTS: u8 = 53;
const CELL_TYPE_CICEROS: u8 = 54;
const CELL_TYPE_INCHES: u8 = 65;
const CELL_TYPE_FEET: u8 = 66;
const CELL_TYPE_MILES: u8 = 68;
const CELL_TYPE_CENTIMETERS: u8 = 69;
const CELL_TYPE_MILLIMETERS: u8 = 70;
const CELL_TYPE_METERS: u8 = 71;
const CELL_TYPE_KILOMETERS: u8 = 72;
const CELL_TYPE_YARDS: u8 = 75;
const CELL_TYPE_NAUTICAL_MILES: u8 = 76;
const CELL_TYPE_ANGLE: u8 = 80;
const CELL_TYPE_DEGREES: u8 = 81;
const CELL_TYPE_DMS: u8 = 82;
const CELL_TYPE_RADIANS: u8 = 83;
const CELL_TYPE_MINUTES: u8 = 84;
const CELL_TYPE_SEC: u8 = 85;
const CELL_TYPE_STRING_WITHOUT_UNIT: u8 = 232;

// Field format numbers (the same table).
const FORMAT_UNKNOWN: u16 = 0xffff;
const FORMAT_MSO_DATE_SHORT: u16 = 200;

/// The display unit string of one field cell type.
fn unit_string(cell_type: u8) -> &'static str {
    match cell_type {
        CELL_TYPE_PERCENT => "%",
        CELL_TYPE_ACRE => " acres",
        CELL_TYPE_HECTARE => " ha",
        CELL_TYPE_ELAPSED_WEEK => " ew.",
        CELL_TYPE_ELAPSED_DAY => " ed.",
        CELL_TYPE_ELAPSED_HOUR => " eh.",
        CELL_TYPE_ELAPSED_MIN => " em.",
        CELL_TYPE_ELAPSED_SEC => " es.",
        CELL_TYPE_POINTS => " pt",
        CELL_TYPE_PICAS => " p",
        CELL_TYPE_DIDOTS => " d",
        CELL_TYPE_CICEROS => " c",
        CELL_TYPE_INCHES => " in",
        CELL_TYPE_FEET => " ft",
        CELL_TYPE_MILES => " mi",
        CELL_TYPE_CENTIMETERS => " cm",
        CELL_TYPE_MILLIMETERS => " mm",
        CELL_TYPE_METERS => " m",
        CELL_TYPE_KILOMETERS => " km",
        CELL_TYPE_YARDS => " yd",
        CELL_TYPE_NAUTICAL_MILES => " nm.",
        CELL_TYPE_RADIANS | CELL_TYPE_ANGLE => " rad",
        CELL_TYPE_MINUTES => " min",
        CELL_TYPE_SEC => " sec",
        CELL_TYPE_DEGREES | CELL_TYPE_DMS => " deg",
        _ => "",
    }
}

/// Converts one stored field value into the display unit of its cell
/// type (lengths are stored in inches, angles in radians, elapsed
/// times in days).
fn convert_number(cell_type: u8, number: f64) -> f64 {
    match cell_type {
        CELL_TYPE_PERCENT => number * 100.0,
        CELL_TYPE_ELAPSED_WEEK => number / 7.0,
        CELL_TYPE_ELAPSED_HOUR => number * 24.0,
        CELL_TYPE_ELAPSED_MIN => number * 24.0 * 60.0,
        CELL_TYPE_ELAPSED_SEC => number * 24.0 * 60.0 * 60.0,
        CELL_TYPE_POINTS => number * 72.0,
        CELL_TYPE_PICAS => number * 6.0,
        CELL_TYPE_DIDOTS => number * 67.75,
        CELL_TYPE_CICEROS => number * 5.644444444444,
        CELL_TYPE_FEET => number * 0.08333333333333,
        CELL_TYPE_CENTIMETERS => number * 2.54,
        CELL_TYPE_MILES => number / 63360.0,
        CELL_TYPE_MILLIMETERS => number * 25.4,
        CELL_TYPE_METERS => number * 0.0254,
        CELL_TYPE_KILOMETERS => number * 0.0000254,
        CELL_TYPE_YARDS => number * 0.02777777777778,
        CELL_TYPE_NAUTICAL_MILES => number / 72913.386,
        CELL_TYPE_DEGREES | CELL_TYPE_DMS | CELL_TYPE_ANGLE => number * 57.295779513082,
        _ => number,
    }
}

/// A number with at most `decimals` decimals, trailing zeros trimmed
/// (the ShapeSheet format strings `0.####` and `0.#`).
fn trimmed_number(value: f64, decimals: usize) -> String {
    let mut text = format!("{value:.decimals$}");
    if text.contains('.') {
        while text.ends_with('0') {
            text.pop();
        }
        if text.ends_with('.') {
            text.pop();
        }
    }
    text
}

/// The civil date and time of one Visio date serial (days since
/// 1899-12-30), in UTC like the file stores it.
struct DateTime {
    /// The year.
    year: i64,
    /// The month, 1 to 12.
    month: u32,
    /// The day of the month.
    day: u32,
    /// The hour.
    hour: u32,
    /// The minute.
    minute: u32,
    /// The second.
    second: u32,
    /// The day of the week, 0 = Sunday.
    weekday: u32,
}

/// The civil date of a day count relative to 1970-01-01.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719468;
    let era = shifted.div_euclid(146097);
    let day_of_era = shifted.rem_euclid(146097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36524 - day_of_era / 146096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    };
    (
        if month <= 2 { year + 1 } else { year },
        month as u32,
        day as u32,
    )
}

/// The date and time of one Visio date serial.
fn date_time(serial: f64) -> Option<DateTime> {
    if !serial.is_finite() || serial.abs() > 1.0e12 {
        return None;
    }
    let total = (serial * 86400.0) as i64;
    // Visio date serials count days from 1899-12-30, Unix days from
    // 1970-01-01 (25569 days apart).
    let days = total.div_euclid(86400).saturating_sub(25569);
    let seconds_of_day = total.rem_euclid(86400);
    let (year, month, day) = civil_from_days(days);
    Some(DateTime {
        year,
        month,
        day,
        hour: (seconds_of_day / 3600) as u32,
        minute: ((seconds_of_day / 60) % 60) as u32,
        second: (seconds_of_day % 60) as u32,
        weekday: (days + 4).rem_euclid(7) as u32,
    })
}

/// The English month names (the display text of a date field is our own
/// formatting of the stored value).
const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// The English day-of-week names.
const WEEKDAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

/// The three-letter abbreviation of a name.
fn abbreviate(name: &str) -> &str {
    let end = name
        .char_indices()
        .nth(3)
        .map(|(index, _)| index)
        .unwrap_or(name.len());
    name.get(..end).unwrap_or(name)
}

/// The hour on a 12-hour clock.
fn hour12(hour: u32) -> u32 {
    match hour % 12 {
        0 => 12,
        other => other,
    }
}

/// `AM` or `PM`.
fn meridiem(hour: u32) -> &'static str {
    if hour < 12 { "AM" } else { "PM" }
}

/// The display text of one date or time field format.
///
/// The format numbers are the field format table of the reference; the
/// patterns are our own English spelling of the format strings the
/// table names. `None` marks a format this module does not spell.
fn date_display(when: &DateTime, format: u16) -> Option<String> {
    let DateTime {
        year,
        month,
        day,
        hour,
        minute,
        second,
        weekday,
    } = *when;
    let month_name = MONTHS.get(month.saturating_sub(1) as usize).copied()?;
    let weekday_name = WEEKDAYS.get(weekday as usize).copied()?;
    let year2 = year.rem_euclid(100);
    let text = match format {
        20 => abbreviate(weekday_name).to_owned(),
        21 => weekday_name.to_owned(),
        22 | 23 | 203 => format!("{month}/{day}/{year2}"),
        24 => format!("{} {day}, {year}", abbreviate(month_name)),
        25 | 202 => format!("{month_name} {day}, {year}"),
        26 | 27 => format!("{day:02}/{month:02}/{year2}"),
        28 => format!("{day} {}, {year}", abbreviate(month_name)),
        29 => format!("{day} {month_name}, {year}"),
        30 => format!(
            "{}:{minute:02}:{second:02} {}",
            hour12(hour),
            meridiem(hour)
        ),
        31 | 33 | 35 | 36 | 213 => format!("{}:{minute:02} {}", hour12(hour), meridiem(hour)),
        32 | 34 | 215 => format!("{hour:02}:{minute:02}"),
        214 => format!(
            "{}:{minute:02}:{second:02} {}",
            hour12(hour),
            meridiem(hour)
        ),
        216 => format!("{hour:02}:{minute:02}:{second:02}"),
        46 | 66..=75 | 80 | 81 => format!("{}:{minute:02} {}", hour12(hour), meridiem(hour)),
        44 | 45 | 50..=65 | 76..=79 | 200 | 217..=221 => format!("{month}/{day}/{year}"),
        201 => format!("{weekday_name}, {month_name} {day:02}, {year}"),
        204 => format!("{year:04}-{month:02}-{day:02}"),
        205 => format!("{day}-{y}-{year2}", y = abbreviate(month_name)),
        206 => format!("{month}.{day}.{year}"),
        207 => format!("{}.{day:02}, {year2}", abbreviate(month_name)),
        208 => format!("{day} {month_name} {year}"),
        209 => format!("{month_name} {year2}"),
        210 => format!("{}-{year2}", abbreviate(month_name)),
        211 => format!(
            "{month}/{day}/{year} {}:{minute:02} {}",
            hour12(hour),
            meridiem(hour)
        ),
        212 => format!(
            "{month}/{day}/{year} {}:{minute:02}:{second:02} {}",
            hour12(hour),
            meridiem(hour)
        ),
        _ => return None,
    };
    Some(text)
}

/// The display text of one numeric field: the stored value spelled with
/// the format the record names. `None` marks a format this module does
/// not spell (feet-and-inches and fraction formats).
fn numeric_display(value: f64, cell_type: u8, format: u16) -> Option<String> {
    if !value.is_finite() {
        return None;
    }
    let text = match format {
        0 => trimmed_number(convert_number(cell_type, value), 4),
        1 | 11 => format!(
            "{}{}",
            trimmed_number(convert_number(cell_type, value), 4),
            unit_string(cell_type)
        ),
        2 => format!("{:.0}", convert_number(cell_type, value)),
        3 => format!(
            "{:.0}{}",
            convert_number(cell_type, value),
            unit_string(cell_type)
        ),
        4 => format!("{:.1}", convert_number(cell_type, value)),
        5 => format!(
            "{:.1}{}",
            convert_number(cell_type, value),
            unit_string(cell_type)
        ),
        6 => format!("{:.2}", convert_number(cell_type, value)),
        7 => format!(
            "{:.2}{}",
            convert_number(cell_type, value),
            unit_string(cell_type)
        ),
        8 => format!("{:.3}", convert_number(cell_type, value)),
        9 => format!(
            "{:.3}{}",
            convert_number(cell_type, value),
            unit_string(cell_type)
        ),
        12 => {
            let degrees = convert_number(cell_type, value);
            if cell_type == CELL_TYPE_DMS {
                let whole = degrees.trunc();
                let minutes = (degrees.fract() * 60.0).abs();
                let minute_whole = minutes.trunc();
                let seconds = ((minutes - minute_whole) * 60.0).round();
                format!("{whole} deg {minute_whole} min {seconds} sec")
            } else {
                format!("{}{}", trimmed_number(degrees, 1), unit_string(cell_type))
            }
        }
        10 | 13..=18 => return None,
        20..=36 | 44 | 45 | 50..=81 | 200..=221 => {
            let when = date_time(value)?;
            date_display(&when, format)?
        }
        FORMAT_UNKNOWN => return None,
        _ => trimmed_number(convert_number(cell_type, value), 4),
    };
    Some(text)
}

/// The display text of one field, following the `fld` marker.
fn field_display(
    field: &Field,
    names: &HashMap<u32, String>,
    warnings: &mut Vec<ImportWarning>,
) -> String {
    match field {
        Field::Text { name_id } => {
            match u32::try_from(*name_id).ok().and_then(|id| names.get(&id)) {
                Some(name) => name.clone(),
                None => {
                    warnings.push(ImportWarning {
                        context: "shape text / field".to_owned(),
                        kind: WarningKind::UnresolvedReference { what: "field name" },
                    });
                    String::new()
                }
            }
        }
        Field::Numeric {
            value,
            cell_type,
            format,
        } => match numeric_display(*value, *cell_type, *format) {
            Some(text) => text,
            None => {
                warnings.push(ImportWarning {
                    context: "shape text / field".to_owned(),
                    kind: WarningKind::Unsupported {
                        what: "field format",
                    },
                });
                String::new()
            }
        },
    }
}

/// The text records of one shape, gathered in stream order.
struct ShapeText {
    /// The ordinal of the shape record, for warning contexts.
    ordinal: usize,
    /// The character runs by record id.
    char_rows: Vec<(u32, CharRow)>,
    /// The character list order.
    char_order: Vec<u32>,
    /// The paragraph runs by record id.
    para_rows: Vec<(u32, ParaRow)>,
    /// The paragraph list order.
    para_order: Vec<u32>,
    /// The tab sets by record id.
    tab_sets: Vec<(u32, TabSet)>,
    /// The tabs list order.
    tabs_order: Vec<u32>,
    /// The fields by record id.
    fields: Vec<(u32, Field)>,
    /// The field list order.
    field_order: Vec<u32>,
    /// The text string.
    text: Option<Decoded>,
}

impl ShapeText {
    /// An empty accumulator for one shape record.
    fn new(ordinal: usize) -> Self {
        Self {
            ordinal,
            char_rows: Vec::new(),
            char_order: Vec::new(),
            para_rows: Vec::new(),
            para_order: Vec::new(),
            tab_sets: Vec::new(),
            tabs_order: Vec::new(),
            fields: Vec::new(),
            field_order: Vec::new(),
            text: None,
        }
    }

    /// The warning context of this shape.
    fn context(&self, what: &str) -> String {
        format!("shape {} / {what}", self.ordinal.saturating_add(1))
    }

    /// Reads one text-family record into the accumulator.
    fn read(
        &mut self,
        chunk: &Chunk<'_>,
        options: &TextOptions,
        warnings: &mut Vec<ImportWarning>,
        text_bytes: &mut u64,
    ) -> Result<(), Error> {
        let id = chunk.id;
        match chunk.record {
            TEXT => {
                let bytes = match chunk.data.get(8..) {
                    Some(bytes) => bytes,
                    None => {
                        warnings.push(ImportWarning {
                            context: self.context("text"),
                            kind: WarningKind::BadValue {
                                what: "text record",
                            },
                        });
                        &[]
                    }
                };
                let decoded = decode_string(bytes, options);
                note_lossy(&decoded, "shape text", warnings);
                account(&decoded.text, text_bytes, &options.limits)?;
                self.text = Some(decoded);
            }
            CHAR_LIST => self.char_order = parse_list(chunk.data),
            PARA_LIST => self.para_order = parse_list(chunk.data),
            TABS_DATA_LIST => self.tabs_order = parse_list(chunk.data),
            FIELD_LIST => self.field_order = parse_list(chunk.data),
            CHAR_IX => match parse_char(chunk.data, warnings) {
                Some(row) => self.char_rows.push((id, row)),
                None => warnings.push(ImportWarning {
                    context: self.context("character run"),
                    kind: WarningKind::BadValue {
                        what: "character run",
                    },
                }),
            },
            PARA_IX => match parse_para(chunk.data, options, warnings) {
                Some(row) => self.para_rows.push((id, row)),
                None => warnings.push(ImportWarning {
                    context: self.context("paragraph run"),
                    kind: WarningKind::BadValue {
                        what: "paragraph run",
                    },
                }),
            },
            TABS_DATA_1 | TABS_DATA_2 | TABS_DATA_3 | TABS_DATA_4 => {
                match parse_tabs(chunk.data, warnings) {
                    Some(set) => self.tab_sets.push((id, set)),
                    None => warnings.push(ImportWarning {
                        context: self.context("tab set"),
                        kind: WarningKind::BadValue { what: "tab set" },
                    }),
                }
            }
            TEXT_FIELD => match parse_field(chunk.data, options) {
                Some(field) => self.fields.push((id, field)),
                None => warnings.push(ImportWarning {
                    context: self.context("field"),
                    kind: WarningKind::BadValue {
                        what: "field record",
                    },
                }),
            },
            _ => {}
        }
        Ok(())
    }

    /// Writes the rows and the text of this shape into `target`.
    fn flush(
        &self,
        target: &mut Shape,
        fonts: &HashMap<u32, String>,
        names: &HashMap<u32, String>,
        options: &TextOptions,
        warnings: &mut Vec<ImportWarning>,
        text_bytes: &mut u64,
    ) -> Result<(), Error> {
        let char_rows = ordered(&self.char_rows, &self.char_order);
        let para_rows = ordered(&self.para_rows, &self.para_order);
        let tab_sets = ordered(&self.tab_sets, &self.tabs_order);
        let fields = ordered(&self.fields, &self.field_order);

        let mut character = Section::new(SectionName::from_static("Character"));
        for (index, row) in char_rows.iter().copied().enumerate() {
            let mut mapped = character_row(row, fonts, warnings);
            mapped.ix = Some(index as i32);
            character.rows.push(mapped);
        }
        if !character.rows.is_empty() {
            target.sheet.sections.push(character);
        }

        let mut paragraph = Section::new(SectionName::from_static("Paragraph"));
        for (index, row) in para_rows.iter().copied().enumerate() {
            let mut mapped = paragraph_row(row);
            mapped.ix = Some(index as i32);
            paragraph.rows.push(mapped);
        }
        if !paragraph.rows.is_empty() {
            target.sheet.sections.push(paragraph);
        }

        let mut tabs = Section::new(SectionName::from_static("Tabs"));
        let mut tab_runs = Vec::new();
        for set in &tab_sets {
            let first = tabs.rows.len();
            for stop in &set.stops {
                let mut mapped = tab_row(stop);
                mapped.ix = Some(tabs.rows.len() as i32);
                tabs.rows.push(mapped);
            }
            tab_runs.push((u32::try_from(first).unwrap_or(u32::MAX), set.char_count));
        }
        if !tabs.rows.is_empty() {
            target.sheet.sections.push(tabs);
        }

        let has_text = self.text.is_some()
            || !char_rows.is_empty()
            || !para_rows.is_empty()
            || !tab_runs.is_empty();
        if !has_text {
            return Ok(());
        }
        let text = self
            .text
            .as_ref()
            .map_or("", |decoded| decoded.text.as_str());
        let content = build_text(
            text, &char_rows, &para_rows, &tab_runs, &fields, names, options, warnings, text_bytes,
        )?;
        target.text = Some(content);
        Ok(())
    }
}

/// Sorts `(record id, value)` pairs into the list order of the ids;
/// entries the list does not name keep their stream order at the end.
fn ordered<'a, T>(items: &'a [(u32, T)], order: &[u32]) -> Vec<&'a T> {
    let mut out = Vec::with_capacity(items.len());
    let mut used = vec![false; items.len()];
    for id in order {
        for (index, (item_id, item)) in items.iter().enumerate() {
            if *item_id == *id && !used.get(index).copied().unwrap_or(true) {
                if let Some(slot) = used.get_mut(index) {
                    *slot = true;
                }
                out.push(item);
            }
        }
    }
    for (index, (_, item)) in items.iter().enumerate() {
        if !used.get(index).copied().unwrap_or(true) {
            out.push(item);
        }
    }
    out
}

/// The list order of one list record payload: the sub-header length,
/// the byte length of the id array, the sub-header itself and the ids.
fn parse_list(data: &[u8]) -> Vec<u32> {
    let mut cursor = Cursor::new(data);
    let mut ids = Vec::new();
    let Some(sub_header) = cursor.u32() else {
        return ids;
    };
    let Some(byte_length) = cursor.u32() else {
        return ids;
    };
    let skip = usize::try_from(sub_header)
        .unwrap_or(usize::MAX)
        .saturating_sub(8);
    if cursor.skip(skip).is_none() {
        return ids;
    }
    let count = usize::try_from(byte_length).unwrap_or(usize::MAX) / 4;
    for _ in 0..count {
        match cursor.u32() {
            Some(id) => ids.push(id),
            None => break,
        }
    }
    ids
}

/// One cell of a row.
fn cell(name: &'static str, value: CellValue, unit: Unit) -> Cell {
    let mut cell = Cell::new(CellName::from_static(name));
    cell.value = Some(value);
    cell.unit = unit;
    cell
}

/// The `Character` row of one character run.
fn character_row(
    row: &CharRow,
    fonts: &HashMap<u32, String>,
    warnings: &mut Vec<ImportWarning>,
) -> Row {
    let mut out = Row::new();
    match fonts.get(&u32::from(row.font_id)) {
        Some(name) => out
            .cells
            .push(cell("Font", CellValue::Str(name.clone()), Unit::Unitsless)),
        None => warnings.push(ImportWarning {
            context: "shape text / character run".to_owned(),
            kind: WarningKind::UnresolvedReference { what: "font" },
        }),
    }
    out.cells
        .push(cell("Color", CellValue::Color(row.color), Unit::Unitsless));
    out.cells.push(cell(
        "Style",
        CellValue::Number(f64::from(row.style)),
        Unit::Unitsless,
    ));
    out.cells.push(cell(
        "Case",
        CellValue::Number(f64::from(row.case)),
        Unit::Unitsless,
    ));
    out.cells.push(cell(
        "Pos",
        CellValue::Number(f64::from(row.pos)),
        Unit::Unitsless,
    ));
    out.cells.push(cell(
        "FontScale",
        CellValue::Number(row.scale_width),
        Unit::Unitsless,
    ));
    out.cells
        .push(cell("Size", CellValue::Number(row.size), Unit::Points));
    out.cells.push(cell(
        "DblUnderline",
        CellValue::Number(flag(row.double_underline)),
        Unit::Unitsless,
    ));
    out.cells.push(cell(
        "Strikethru",
        CellValue::Number(flag(row.strikeout)),
        Unit::Unitsless,
    ));
    out.cells.push(cell(
        "DoubleStrikethrough",
        CellValue::Number(flag(row.double_strikeout)),
        Unit::Unitsless,
    ));
    out.cells.push(cell(
        "ColorTrans",
        CellValue::Number(f64::from(row.alpha) / 255.0),
        Unit::Unitsless,
    ));
    out
}

/// The model spelling of one boolean flag: Visio writes `0` and `1`
/// and the `.vsdx` reader reads those as numbers.
fn flag(value: bool) -> f64 {
    if value { 1.0 } else { 0.0 }
}

/// The `Paragraph` row of one paragraph run.
fn paragraph_row(row: &ParaRow) -> Row {
    let mut out = Row::new();
    out.cells.push(cell(
        "IndFirst",
        CellValue::Number(row.ind_first),
        Unit::Points,
    ));
    out.cells.push(cell(
        "IndLeft",
        CellValue::Number(row.ind_left),
        Unit::Points,
    ));
    out.cells.push(cell(
        "IndRight",
        CellValue::Number(row.ind_right),
        Unit::Points,
    ));
    out.cells.push(cell(
        "SpLine",
        CellValue::Number(row.sp_line),
        Unit::Unitsless,
    ));
    out.cells.push(cell(
        "SpBefore",
        CellValue::Number(row.sp_before),
        Unit::Points,
    ));
    out.cells.push(cell(
        "SpAfter",
        CellValue::Number(row.sp_after),
        Unit::Points,
    ));
    out.cells.push(cell(
        "HorzAlign",
        CellValue::Number(f64::from(row.align)),
        Unit::Unitsless,
    ));
    out.cells.push(cell(
        "Bullet",
        CellValue::Number(f64::from(row.bullet)),
        Unit::Unitsless,
    ));
    out.cells.push(cell(
        "BulletStr",
        CellValue::Str(row.bullet_string.clone().unwrap_or_default()),
        Unit::Unitsless,
    ));
    out.cells.push(cell(
        "BulletFont",
        CellValue::Number(f64::from(row.bullet_font)),
        Unit::Unitsless,
    ));
    out.cells.push(cell(
        "BulletFontSize",
        CellValue::Number(row.bullet_font_size),
        Unit::Points,
    ));
    out.cells.push(cell(
        "TextPosAfterBullet",
        CellValue::Number(row.text_pos_after_bullet),
        Unit::Points,
    ));
    out.cells.push(cell(
        "Flags",
        CellValue::Number(f64::from(row.flags)),
        Unit::Unitsless,
    ));
    out
}

/// The `Tabs` row of one tab stop.
fn tab_row(stop: &TabStop) -> Row {
    let mut out = Row::new();
    out.cells.push(cell(
        "Position",
        CellValue::Number(stop.position),
        Unit::Points,
    ));
    out.cells.push(cell(
        "Alignment",
        CellValue::Number(f64::from(stop.alignment)),
        Unit::Unitsless,
    ));
    out
}

/// One run marker at one character position.
struct Marker {
    /// The character position the run starts at.
    position: usize,
    /// The kind order: character before paragraph before tabs runs.
    kind: u8,
    /// The row the marker references.
    ix: u32,
}

/// Builds the [`TextContent`] of one shape: the literal text split at
/// run boundaries, the run markers and the fields with their cached
/// display text (ADR 0019).
#[allow(clippy::too_many_arguments)]
fn build_text(
    text: &str,
    char_rows: &[&CharRow],
    para_rows: &[&ParaRow],
    tab_runs: &[(u32, u32)],
    fields: &[&Field],
    names: &HashMap<u32, String>,
    options: &TextOptions,
    warnings: &mut Vec<ImportWarning>,
    text_bytes: &mut u64,
) -> Result<TextContent, Error> {
    let chars: Vec<char> = text.chars().collect();
    let length = chars.len();
    let mut markers = Vec::new();
    let char_runs = char_rows
        .iter()
        .enumerate()
        .map(|(index, row)| (u32::try_from(index).unwrap_or(u32::MAX), row.char_count));
    let para_runs = para_rows
        .iter()
        .enumerate()
        .map(|(index, row)| (u32::try_from(index).unwrap_or(u32::MAX), row.char_count));
    push_runs(&mut markers, char_runs, 0);
    push_runs(&mut markers, para_runs, 1);
    push_runs(&mut markers, tab_runs.iter().copied(), 2);
    markers.sort_by_key(|marker: &Marker| (marker.position, marker.kind));

    let mut content = TextContent::new();
    let mut literal = String::new();
    let mut marker_index = 0usize;
    let mut field_index = 0usize;
    for (position, character) in chars.iter().enumerate() {
        while let Some(marker) = markers.get(marker_index) {
            if marker.position > position {
                break;
            }
            push_literal(&mut content, &mut literal);
            push_marker(&mut content, marker);
            marker_index = marker_index.saturating_add(1);
        }
        if *character == '\u{fffc}' || *character == '\u{1e}' {
            push_literal(&mut content, &mut literal);
            match fields.get(field_index) {
                Some(field) => {
                    content.parts.push(TextPart::Field {
                        ix: u32::try_from(field_index).unwrap_or(u32::MAX),
                    });
                    let display = field_display(field, names, warnings);
                    account(&display, text_bytes, &options.limits)?;
                    if !display.is_empty() {
                        content.parts.push(TextPart::Text(display));
                    }
                }
                None => warnings.push(ImportWarning {
                    context: "shape text".to_owned(),
                    kind: WarningKind::Dropped {
                        what: "field placeholder",
                    },
                }),
            }
            field_index = field_index.saturating_add(1);
        } else {
            literal.push(*character);
        }
    }
    while let Some(marker) = markers.get(marker_index) {
        marker_index = marker_index.saturating_add(1);
        if marker.position > length {
            warnings.push(ImportWarning {
                context: "shape text".to_owned(),
                kind: WarningKind::Dropped {
                    what: "text run marker",
                },
            });
            continue;
        }
        push_literal(&mut content, &mut literal);
        push_marker(&mut content, marker);
    }
    push_literal(&mut content, &mut literal);
    Ok(content)
}

/// Appends the pending literal text as one part.
fn push_literal(content: &mut TextContent, literal: &mut String) {
    if !literal.is_empty() {
        content.parts.push(TextPart::Text(std::mem::take(literal)));
    }
}

/// Appends one run marker part.
fn push_marker(content: &mut TextContent, marker: &Marker) {
    let part = match marker.kind {
        0 => TextPart::CharRun { ix: marker.ix },
        1 => TextPart::ParaRun { ix: marker.ix },
        _ => TextPart::TabsRun { ix: marker.ix },
    };
    content.parts.push(part);
}

/// Turns `(row ix, character count)` run pairs into markers at their
/// cumulative start positions.
fn push_runs(markers: &mut Vec<Marker>, runs: impl Iterator<Item = (u32, u32)>, kind: u8) {
    let mut position: usize = 0;
    for (ix, count) in runs {
        markers.push(Marker { position, kind, ix });
        position = position.saturating_add(usize::try_from(count).unwrap_or(usize::MAX));
    }
}

/// Parses one font record (version 6 `FontIX` or version 11 `FontFace`)
/// into its name; the caller keys the map by the record id. Version 6
/// stores the name as a byte string after the charset word, version 11
/// as a zero-terminated UTF-16 string of at most 32 characters.
fn parse_font(
    data: &[u8],
    options: &TextOptions,
    warnings: &mut Vec<ImportWarning>,
) -> Option<String> {
    let mut cursor = Cursor::new(data);
    let decoded = match options.version {
        FormatVersion::V11 => {
            cursor.skip(4)?;
            let bytes = cursor.rest();
            let end = (0usize..32).find_map(|index| {
                let at = index.checked_mul(2)?;
                match (bytes.get(at), bytes.get(at.saturating_add(1))) {
                    (Some(0), Some(0)) => Some(at),
                    _ => None,
                }
            });
            let name_bytes = match end {
                Some(end) => bytes.get(..end)?,
                None => bytes,
            };
            decode_utf16(name_bytes)
        }
        FormatVersion::V6 => {
            cursor.skip(6)?;
            let bytes = cursor.rest();
            let end = bytes
                .iter()
                .position(|byte| *byte == 0)
                .unwrap_or(bytes.len());
            decode_bytes(bytes.get(..end)?, options.code_page)
        }
    };
    note_lossy(&decoded, "font name", warnings);
    Some(decoded.text)
}

/// Parses one name-list name (`Name2`) into its text; the caller keys
/// the map by the record id.
fn parse_name(
    data: &[u8],
    options: &TextOptions,
    warnings: &mut Vec<ImportWarning>,
) -> Option<String> {
    let mut cursor = Cursor::new(data);
    cursor.skip(4)?;
    let decoded = match options.version {
        FormatVersion::V11 => {
            // A zero-terminated UTF-16 string.
            let bytes = cursor.rest();
            let end = (0..=bytes.len()).find_map(|index| {
                let at = index.checked_mul(2)?;
                match (bytes.get(at), bytes.get(at.saturating_add(1))) {
                    (Some(0), Some(0)) | (None, _) => Some(at),
                    _ => None,
                }
            });
            let name_bytes = bytes.get(..end.unwrap_or(bytes.len()))?;
            decode_utf16(name_bytes)
        }
        FormatVersion::V6 => {
            // A zero-terminated byte string.
            let bytes = cursor.rest();
            let end = bytes
                .iter()
                .position(|byte| *byte == 0)
                .unwrap_or(bytes.len());
            decode_bytes(bytes.get(..end)?, options.code_page)
        }
    };
    note_lossy(&decoded, "name", warnings);
    Some(decoded.text)
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)] // test payload builders mirror the records
mod tests {
    use super::*;

    // ---- payload builders (the record layouts of the module docs) ----

    /// UTF-16LE bytes of one string.
    fn utf16(text: &str) -> Vec<u8> {
        text.encode_utf16()
            .flat_map(|unit| unit.to_le_bytes())
            .collect()
    }

    /// One `CharIX` payload.
    fn char_payload(
        char_count: u32,
        font_id: u16,
        rgba: [u8; 4],
        mod1: u8,
        mod2: u8,
        mod3: u8,
        scale: u16,
        size: f64,
        mod4: u8,
    ) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&char_count.to_le_bytes());
        data.extend_from_slice(&font_id.to_le_bytes());
        data.push(0);
        data.extend_from_slice(&rgba);
        data.push(mod1);
        data.push(mod2);
        data.push(mod3);
        data.extend_from_slice(&scale.to_le_bytes());
        data.extend_from_slice(&0u16.to_le_bytes());
        data.extend_from_slice(&size.to_le_bytes());
        data.push(mod4);
        data
    }

    /// The trailing bullet-string block of a paragraph record.
    fn bullet_block(text: &str, v6: bool) -> Vec<u8> {
        let bytes = if v6 {
            text.as_bytes().to_vec()
        } else {
            utf16(text)
        };
        let mut block = Vec::new();
        block.extend_from_slice(&((bytes.len() + 8) as u32).to_le_bytes());
        block.push(2);
        block.push(8);
        block.push(0);
        block.push(if v6 {
            bytes.len() as u8
        } else {
            (bytes.len() / 2) as u8
        });
        block.extend_from_slice(&bytes);
        block.extend_from_slice(&0u32.to_le_bytes());
        block
    }

    /// One version 11 `ParaIX` payload.
    fn para_payload_v11(
        char_count: u32,
        values: [f64; 6],
        align: u8,
        bullet: u8,
        bullet_font: u16,
        bullet_font_size: f64,
        text_pos: f64,
        flags: u32,
        bullet_string: Option<&str>,
    ) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&char_count.to_le_bytes());
        for value in values {
            data.push(0x3f);
            data.extend_from_slice(&value.to_le_bytes());
        }
        data.push(align);
        data.push(bullet);
        data.extend_from_slice(&[0; 4]);
        data.extend_from_slice(&bullet_font.to_le_bytes());
        data.extend_from_slice(&[0; 2]);
        data.extend_from_slice(&bullet_font_size.to_le_bytes());
        data.push(0x3f);
        data.extend_from_slice(&text_pos.to_le_bytes());
        data.extend_from_slice(&flags.to_le_bytes());
        data.extend_from_slice(&[0; 34]);
        if let Some(text) = bullet_string {
            data.extend_from_slice(&bullet_block(text, false));
        }
        data
    }

    /// One version 6 `ParaIX` payload.
    fn para_payload_v6(
        char_count: u32,
        values: [f64; 6],
        align: u8,
        bullet: u8,
        flags: u32,
        bullet_string: Option<&str>,
    ) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&char_count.to_le_bytes());
        for value in values {
            data.push(0x3f);
            data.extend_from_slice(&value.to_le_bytes());
        }
        data.push(align);
        data.push(bullet);
        data.extend_from_slice(&[0; 4]);
        data.extend_from_slice(&flags.to_le_bytes());
        data.extend_from_slice(&[0; 5]);
        if let Some(text) = bullet_string {
            data.extend_from_slice(&bullet_block(text, true));
        }
        data
    }

    /// One `TabsData` payload.
    fn tabs_payload(char_count: u32, stops: &[(f64, u8)]) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&char_count.to_le_bytes());
        data.push(stops.len() as u8);
        for (position, alignment) in stops {
            data.push(0x3f);
            data.extend_from_slice(&position.to_le_bytes());
            data.push(*alignment);
            data.push(0);
        }
        data
    }

    /// One `Text` record payload: the eight skipped bytes and the
    /// string bytes as the caller spells them.
    fn text_payload(bytes: &[u8]) -> Vec<u8> {
        let mut data = vec![0u8; 8];
        data.extend_from_slice(bytes);
        data
    }

    /// One list record payload (the two lengths and the ids).
    fn list_payload(ids: &[u32]) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&8u32.to_le_bytes());
        data.extend_from_slice(&((ids.len() * 4) as u32).to_le_bytes());
        for id in ids {
            data.extend_from_slice(&id.to_le_bytes());
        }
        data
    }

    /// One string-field `TextField` payload.
    fn field_string_payload(name_id: i32) -> Vec<u8> {
        let mut data = vec![0u8; 7];
        data.push(CELL_TYPE_STRING_WITHOUT_UNIT);
        data.extend_from_slice(&name_id.to_le_bytes());
        data
    }

    /// One numeric-field `TextField` payload with its format block.
    fn field_numeric_payload(value: f64, cell_type: u8, format: u16, v6: bool) -> Vec<u8> {
        let mut data = vec![0u8; 7];
        data.push(cell_type);
        data.extend_from_slice(&value.to_le_bytes());
        data.extend_from_slice(&[0, 0]);
        data.extend_from_slice(&0i32.to_le_bytes());
        let scan_start = if v6 { 0x24 } else { 0x36 };
        while data.len() < scan_start {
            data.push(0);
        }
        data.extend_from_slice(&11u32.to_le_bytes());
        data.push(0);
        data.push(2);
        data.push(0);
        data.extend_from_slice(&format.to_le_bytes());
        data.push(0x80);
        data.push(0xc2);
        data.extend_from_slice(&0u32.to_le_bytes());
        data
    }

    /// One font record payload (`FontIX` or `FontFace`).
    fn font_payload(name: &str) -> Vec<u8> {
        let mut data = vec![0u8; 4];
        data.extend_from_slice(&utf16(name));
        data.extend_from_slice(&[0, 0]);
        data
    }

    /// One `Name2` record payload.
    fn name2_payload(name: &str) -> Vec<u8> {
        let mut data = vec![0u8; 4];
        data.extend_from_slice(&utf16(name));
        data.extend_from_slice(&[0, 0]);
        data
    }

    // ---- drivers ----

    /// A document with `count` shapes in the arena, in stream order.
    fn document_with_shapes(count: usize) -> Document {
        let mut document = Document::new();
        for index in 0..count {
            document.shapes.push(Shape::new(
                ShapeId {
                    index: index as u32,
                },
                index as i32 + 1,
            ));
        }
        document
    }

    /// Runs [`collect_with`] over `chunks` against `count` shapes.
    fn run(
        chunks: &[Chunk<'_>],
        count: usize,
        options: &TextOptions,
    ) -> (Document, Vec<ImportWarning>) {
        let mut document = document_with_shapes(count);
        let mut warnings = Vec::new();
        collect_with(chunks, &mut document, &mut warnings, options).expect("collects");
        (document, warnings)
    }

    /// Runs [`collect_with`] with the default options.
    fn run_default(chunks: &[Chunk<'_>], count: usize) -> (Document, Vec<ImportWarning>) {
        run(chunks, count, &TextOptions::default())
    }

    /// The version 6 options with one code page.
    fn v6_options(code_page: CodePage) -> TextOptions {
        TextOptions {
            version: FormatVersion::V6,
            code_page,
            limits: Limits::DEFAULT,
        }
    }

    /// The cell of one row by name.
    fn find_cell<'a>(row: &'a Row, name: &str) -> Option<&'a Cell> {
        row.cells.iter().find(|cell| cell.name.as_str() == name)
    }

    /// The section of one sheet by name.
    fn find_section<'a>(shape: &'a Shape, name: &str) -> Option<&'a Section> {
        shape
            .sheet
            .sections
            .iter()
            .find(|section| section.name.as_str() == name)
    }

    /// A shape chunk at level 1.
    fn shape_chunk(id: u32) -> Chunk<'static> {
        Chunk {
            record: SHAPE_SHAPE,
            level: 1,
            list: 0,
            id,
            data: &[],
        }
    }

    /// A text-family chunk at level 2.
    fn text_chunk<'a>(record: Record, id: u32, data: &'a [u8]) -> Chunk<'a> {
        Chunk {
            record,
            level: 2,
            list: 0,
            id,
            data,
        }
    }

    // ---- decoding and code pages ----

    #[test]
    fn code_pages_come_from_windows_ids() {
        assert_eq!(CodePage::from_windows_id(1252), Some(CodePage::Windows1252));
        assert_eq!(CodePage::from_windows_id(0), Some(CodePage::Windows1252));
        assert_eq!(CodePage::from_windows_id(65001), Some(CodePage::Utf8));
        assert_eq!(CodePage::from_windows_id(1251), None);
        assert_eq!(CodePage::default(), CodePage::Windows1252);
    }

    #[test]
    fn the_two_fdo86729_spellings_decode_to_the_same_string() {
        // The corpus pair stores the same title in one code page and in
        // UTF-8 (fdo#86729); here are the two spellings by hand.
        let cp1252 = b"mytitle\xe9\xe1".to_vec();
        let utf8 = "mytitle\u{e9}\u{e1}".as_bytes().to_vec();
        let one = decode_bytes(&cp1252, CodePage::Windows1252);
        let two = decode_bytes(&utf8, CodePage::Utf8);
        assert_eq!(one.text, two.text);
        assert_eq!(one.text, "mytitle\u{e9}\u{e1}");
        assert!(!one.lossy);
        assert!(!two.lossy);
    }

    #[test]
    fn utf8_decodes_non_ascii() {
        let decoded = decode_bytes(
            "mytitle\u{e9}\u{e1}\u{151}\u{171}".as_bytes(),
            CodePage::Utf8,
        );
        assert_eq!(decoded.text, "mytitle\u{e9}\u{e1}\u{151}\u{171}");
        assert!(!decoded.lossy);
    }

    #[test]
    fn windows1252_maps_its_special_bytes_and_rejects_the_undefined_ones() {
        let decoded = decode_bytes(&[0x80, 0x96, 0xe9, 0x81], CodePage::Windows1252);
        assert_eq!(decoded.text, "\u{20ac}\u{2013}\u{e9}\u{fffd}");
        assert!(decoded.lossy);
    }

    #[test]
    fn a_bad_byte_sequence_is_lossy_but_keeps_its_text() {
        let bytes = text_payload(b"ok\x81\xff");
        let chunks = [shape_chunk(1), text_chunk(TEXT, 0, &bytes)];
        let options = v6_options(CodePage::Windows1252);
        let (document, warnings) = run(&chunks, 1, &options);
        let shape = document.shape(ShapeId { index: 0 }).expect("shape");
        let content = shape.text.as_ref().expect("text");
        assert_eq!(
            content.parts,
            vec![TextPart::Text("ok\u{fffd}\u{ff}".to_owned())]
        );
        assert!(
            warnings
                .iter()
                .any(|warning| warning.kind == WarningKind::BadValue { what: "shape text" })
        );
    }

    #[test]
    fn byte_strings_map_breaks_and_field_placeholders() {
        let decoded = decode_bytes(b"a\r\nb\x0ec\x1ed", CodePage::Windows1252);
        assert_eq!(decoded.text, "a\n\nb\nc\u{fffc}d");
        assert!(!decoded.lossy);
    }

    #[test]
    fn utf16_decodes_pairs_and_replaces_surrogates() {
        let mut bytes = utf16("hi ");
        bytes.extend_from_slice(&(0xd83du16).to_le_bytes());
        bytes.extend_from_slice(&0xde00u16.to_le_bytes());
        let decoded = decode_utf16(&bytes);
        assert_eq!(decoded.text, "hi \u{1f600}");
        assert!(!decoded.lossy);

        let lone = decode_utf16(&[0x3d, 0xd8]);
        assert_eq!(lone.text, "\u{fffd}");
        assert!(lone.lossy);

        let odd = decode_utf16(&[0x41, 0x00, 0x42]);
        assert_eq!(odd.text, "A\u{fffd}");
        assert!(odd.lossy);

        let low_first = decode_utf16(&0xdc00u16.to_le_bytes());
        assert_eq!(low_first.text, "\u{fffd}");
        assert!(low_first.lossy);
    }

    #[test]
    fn trailing_nul_bytes_are_dropped_from_byte_strings() {
        let decoded = decode_bytes(b"abc\0\0", CodePage::Windows1252);
        assert_eq!(decoded.text, "abc");
    }

    // ---- text and run markers ----

    #[test]
    fn collect_maps_no_records_without_error() {
        let mut document = Document::new();
        let mut warnings = Vec::new();
        collect(&[], &mut document, &mut warnings).expect("collects");
        assert!(warnings.is_empty());
    }

    #[test]
    fn a_text_record_maps_to_literal_text() {
        let bytes = text_payload(&utf16("Hello\nworld"));
        let chunks = [shape_chunk(1), text_chunk(TEXT, 0, &bytes)];
        let (document, _) = run_default(&chunks, 1);
        let shape = document.shape(ShapeId { index: 0 }).expect("shape");
        assert_eq!(
            shape.text.as_ref().expect("text").parts,
            vec![TextPart::Text("Hello\nworld".to_owned())]
        );
    }

    #[test]
    fn version_6_and_version_11_text_decode_to_the_same_parts() {
        let v11 = text_payload(&utf16("caf\u{e9}"));
        let v6 = text_payload(b"caf\xe9");
        let chunks_v11 = [shape_chunk(1), text_chunk(TEXT, 0, &v11)];
        let chunks_v6 = [shape_chunk(1), text_chunk(TEXT, 0, &v6)];
        let (one, _) = run_default(&chunks_v11, 1);
        let (two, _) = run(&chunks_v6, 1, &v6_options(CodePage::Windows1252));
        let one = one.shape(ShapeId { index: 0 }).expect("shape");
        let two = two.shape(ShapeId { index: 0 }).expect("shape");
        assert_eq!(one.text, two.text);
        assert_eq!(
            one.text.as_ref().expect("text").parts,
            vec![TextPart::Text("caf\u{e9}".to_owned())]
        );
    }

    #[test]
    fn run_markers_sit_at_the_run_boundaries() {
        let bytes = text_payload(&utf16("helloworld"));
        let chars = [
            char_payload(5, 0, [0, 0, 0, 0], 0, 0, 0, 10000, 0.1, 0),
            char_payload(5, 0, [0, 0, 0, 0], 0, 0, 0, 10000, 0.2, 0),
        ];
        let chunks = [
            shape_chunk(1),
            text_chunk(TEXT, 0, &bytes),
            text_chunk(CHAR_IX, 0, &chars[0]),
            text_chunk(CHAR_IX, 1, &chars[1]),
        ];
        let (document, _) = run_default(&chunks, 1);
        let shape = document.shape(ShapeId { index: 0 }).expect("shape");
        assert_eq!(
            shape.text.as_ref().expect("text").parts,
            vec![
                TextPart::CharRun { ix: 0 },
                TextPart::Text("hello".to_owned()),
                TextPart::CharRun { ix: 1 },
                TextPart::Text("world".to_owned()),
            ]
        );
    }

    #[test]
    fn zero_length_runs_stack_at_one_position() {
        let bytes = text_payload(&utf16("x"));
        let chars = [
            char_payload(0, 0, [0, 0, 0, 0], 0, 0, 0, 10000, 0.1, 0),
            char_payload(1, 0, [0, 0, 0, 0], 0, 0, 0, 10000, 0.2, 0),
        ];
        let chunks = [
            shape_chunk(1),
            text_chunk(TEXT, 0, &bytes),
            text_chunk(CHAR_IX, 0, &chars[0]),
            text_chunk(CHAR_IX, 1, &chars[1]),
        ];
        let (document, _) = run_default(&chunks, 1);
        let shape = document.shape(ShapeId { index: 0 }).expect("shape");
        assert_eq!(
            shape.text.as_ref().expect("text").parts,
            vec![
                TextPart::CharRun { ix: 0 },
                TextPart::CharRun { ix: 1 },
                TextPart::Text("x".to_owned()),
            ]
        );
    }

    #[test]
    fn runs_past_the_end_are_dropped_with_a_warning() {
        let bytes = text_payload(&utf16("ab"));
        let chars = [
            char_payload(5, 0, [0, 0, 0, 0], 0, 0, 0, 10000, 0.1, 0),
            char_payload(5, 0, [0, 0, 0, 0], 0, 0, 0, 10000, 0.2, 0),
        ];
        let chunks = [
            shape_chunk(1),
            text_chunk(TEXT, 0, &bytes),
            text_chunk(CHAR_IX, 0, &chars[0]),
            text_chunk(CHAR_IX, 1, &chars[1]),
        ];
        let (document, warnings) = run_default(&chunks, 1);
        let shape = document.shape(ShapeId { index: 0 }).expect("shape");
        assert_eq!(
            shape.text.as_ref().expect("text").parts,
            vec![TextPart::CharRun { ix: 0 }, TextPart::Text("ab".to_owned()),]
        );
        assert!(warnings.iter().any(|warning| warning.kind
            == WarningKind::Dropped {
                what: "text run marker"
            }));
    }

    #[test]
    fn markers_without_a_text_record_still_map() {
        let chars = [char_payload(0, 0, [0, 0, 0, 0], 0, 0, 0, 10000, 0.1, 0)];
        let chunks = [shape_chunk(1), text_chunk(CHAR_IX, 0, &chars[0])];
        let (document, _) = run_default(&chunks, 1);
        let shape = document.shape(ShapeId { index: 0 }).expect("shape");
        assert_eq!(
            shape.text.as_ref().expect("text").parts,
            vec![TextPart::CharRun { ix: 0 }]
        );
    }

    // ---- fields ----

    #[test]
    fn a_field_marker_carries_its_cached_display_text() {
        let mut bytes = text_payload(&utf16("a"));
        bytes.extend_from_slice(&utf16("\u{fffc}"));
        bytes.extend_from_slice(&utf16("b"));
        let fields = list_payload(&[10]);
        let field = field_string_payload(7);
        let name = name2_payload("42");
        let chunks = [
            shape_chunk(1),
            text_chunk(TEXT, 0, &bytes),
            text_chunk(FIELD_LIST, 0, &fields),
            text_chunk(TEXT_FIELD, 10, &field),
            Chunk {
                record: NAME2,
                level: 1,
                list: 0,
                id: 7,
                data: &name,
            },
        ];
        let (document, _) = run_default(&chunks, 1);
        let shape = document.shape(ShapeId { index: 0 }).expect("shape");
        assert_eq!(
            shape.text.as_ref().expect("text").parts,
            vec![
                TextPart::Text("a".to_owned()),
                TextPart::Field { ix: 0 },
                TextPart::Text("42".to_owned()),
                TextPart::Text("b".to_owned()),
            ]
        );
    }

    #[test]
    fn a_field_placeholder_without_a_field_is_dropped_with_a_warning() {
        let mut bytes = text_payload(&utf16("a"));
        bytes.extend_from_slice(&utf16("\u{fffc}"));
        let chunks = [shape_chunk(1), text_chunk(TEXT, 0, &bytes)];
        let (document, warnings) = run_default(&chunks, 1);
        let shape = document.shape(ShapeId { index: 0 }).expect("shape");
        assert_eq!(
            shape.text.as_ref().expect("text").parts,
            vec![TextPart::Text("a".to_owned())]
        );
        assert!(warnings.iter().any(|warning| warning.kind
            == WarningKind::Dropped {
                what: "field placeholder"
            }));
    }

    #[test]
    fn an_unresolved_field_name_warns_and_stays_empty() {
        let mut bytes = text_payload(&utf16(""));
        bytes.extend_from_slice(&utf16("\u{fffc}"));
        let field = field_string_payload(9);
        let chunks = [
            shape_chunk(1),
            text_chunk(TEXT, 0, &bytes),
            text_chunk(TEXT_FIELD, 1, &field),
        ];
        let (document, warnings) = run_default(&chunks, 1);
        let shape = document.shape(ShapeId { index: 0 }).expect("shape");
        assert_eq!(
            shape.text.as_ref().expect("text").parts,
            vec![TextPart::Field { ix: 0 }]
        );
        assert!(
            warnings
                .iter()
                .any(|warning| warning.kind
                    == WarningKind::UnresolvedReference { what: "field name" })
        );
    }

    #[test]
    fn a_numeric_field_spells_its_cached_value() {
        let mut bytes = text_payload(&utf16(""));
        bytes.extend_from_slice(&utf16("\u{fffc}"));
        let field = field_numeric_payload(0.5, CELL_TYPE_DEGREES, 12, false);
        let chunks = [
            shape_chunk(1),
            text_chunk(TEXT, 0, &bytes),
            text_chunk(TEXT_FIELD, 1, &field),
        ];
        let (document, _) = run_default(&chunks, 1);
        let shape = document.shape(ShapeId { index: 0 }).expect("shape");
        assert_eq!(
            shape.text.as_ref().expect("text").parts,
            vec![
                TextPart::Field { ix: 0 },
                TextPart::Text("28.6 deg".to_owned()),
            ]
        );
    }

    #[test]
    fn a_malformed_field_record_warns_and_skips() {
        let field = vec![0u8; 3];
        let chunks = [shape_chunk(1), text_chunk(TEXT_FIELD, 1, &field)];
        let (document, warnings) = run_default(&chunks, 1);
        assert_eq!(
            document.shape(ShapeId { index: 0 }).expect("shape").text,
            None
        );
        assert!(warnings.iter().any(|warning| warning.kind
            == WarningKind::BadValue {
                what: "field record"
            }));
    }

    // ---- rows ----

    #[test]
    fn character_rows_map_the_run_cells() {
        let font = font_payload("Calibri");
        let chars = [char_payload(
            1,
            4,
            [1, 2, 3, 64],
            0x07,
            0x01,
            0x01,
            15000,
            0.1666666666666667,
            0x21,
        )];
        let chunks = [
            shape_chunk(1),
            text_chunk(CHAR_IX, 0, &chars[0]),
            Chunk {
                record: FONTFACE,
                level: 1,
                list: 0,
                id: 4,
                data: &font,
            },
        ];
        let (document, warnings) = run_default(&chunks, 1);
        assert!(warnings.is_empty(), "{warnings:?}");
        let shape = document.shape(ShapeId { index: 0 }).expect("shape");
        let section = find_section(shape, "Character").expect("section");
        let row = section.rows.first().expect("row");
        assert_eq!(row.ix, Some(0));
        assert_eq!(
            find_cell(row, "Font").and_then(|cell| cell.value.clone()),
            Some(CellValue::Str("Calibri".to_owned()))
        );
        assert_eq!(
            find_cell(row, "Color").and_then(|cell| cell.value.clone()),
            Some(CellValue::Color(Color::new(1, 2, 3)))
        );
        assert_eq!(
            find_cell(row, "Style").and_then(|cell| cell.value.clone()),
            Some(CellValue::Number(7.0))
        );
        assert_eq!(
            find_cell(row, "Case").and_then(|cell| cell.value.clone()),
            Some(CellValue::Number(1.0))
        );
        assert_eq!(
            find_cell(row, "Pos").and_then(|cell| cell.value.clone()),
            Some(CellValue::Number(1.0))
        );
        assert_eq!(
            find_cell(row, "FontScale").and_then(|cell| cell.value.clone()),
            Some(CellValue::Number(1.5))
        );
        let size = find_cell(row, "Size").expect("size");
        assert_eq!(size.value, Some(CellValue::Number(0.1666666666666667)));
        assert_eq!(size.unit, Unit::Points);
        assert_eq!(
            find_cell(row, "DblUnderline").and_then(|cell| cell.value.clone()),
            Some(CellValue::Number(1.0))
        );
        assert_eq!(
            find_cell(row, "Strikethru").and_then(|cell| cell.value.clone()),
            Some(CellValue::Number(0.0))
        );
        assert_eq!(
            find_cell(row, "DoubleStrikethrough").and_then(|cell| cell.value.clone()),
            Some(CellValue::Number(1.0))
        );
        assert_eq!(
            find_cell(row, "ColorTrans").and_then(|cell| cell.value.clone()),
            Some(CellValue::Number(64.0 / 255.0))
        );
    }

    #[test]
    fn an_unresolved_font_warns_and_the_row_stays() {
        let chars = [char_payload(1, 9, [0, 0, 0, 0], 0, 0, 0, 10000, 0.1, 0)];
        let chunks = [shape_chunk(1), text_chunk(CHAR_IX, 0, &chars[0])];
        let (document, warnings) = run_default(&chunks, 1);
        assert!(
            warnings
                .iter()
                .any(|warning| warning.kind == WarningKind::UnresolvedReference { what: "font" })
        );
        let shape = document.shape(ShapeId { index: 0 }).expect("shape");
        let row = find_section(shape, "Character")
            .expect("section")
            .rows
            .first()
            .expect("row");
        assert!(find_cell(row, "Font").is_none());
    }

    #[test]
    fn paragraph_rows_map_the_run_cells() {
        let paragraph = para_payload_v11(
            1,
            [0.01, 0.02, 0.03, -1.2, 0.04, 0.05],
            1,
            3,
            5,
            -1.0,
            0.06,
            7,
            Some("x."),
        );
        let chunks = [shape_chunk(1), text_chunk(PARA_IX, 0, &paragraph)];
        let (document, _) = run_default(&chunks, 1);
        let shape = document.shape(ShapeId { index: 0 }).expect("shape");
        let section = find_section(shape, "Paragraph").expect("section");
        let row = section.rows.first().expect("row");
        let expect = [
            ("IndFirst", 0.01),
            ("IndLeft", 0.02),
            ("IndRight", 0.03),
            ("SpLine", -1.2),
            ("SpBefore", 0.04),
            ("SpAfter", 0.05),
        ];
        for (name, value) in expect {
            let cell = find_cell(row, name).expect(name);
            assert_eq!(cell.value, Some(CellValue::Number(value)), "{name}");
        }
        assert_eq!(find_cell(row, "IndFirst").expect("cell").unit, Unit::Points);
        assert_eq!(
            find_cell(row, "SpLine").expect("cell").unit,
            Unit::Unitsless
        );
        assert_eq!(
            find_cell(row, "HorzAlign").and_then(|cell| cell.value.clone()),
            Some(CellValue::Number(1.0))
        );
        assert_eq!(
            find_cell(row, "Bullet").and_then(|cell| cell.value.clone()),
            Some(CellValue::Number(3.0))
        );
        assert_eq!(
            find_cell(row, "BulletStr").and_then(|cell| cell.value.clone()),
            Some(CellValue::Str("x.".to_owned()))
        );
        assert_eq!(
            find_cell(row, "BulletFont").and_then(|cell| cell.value.clone()),
            Some(CellValue::Number(5.0))
        );
        let bullet_size = find_cell(row, "BulletFontSize").expect("cell");
        assert_eq!(bullet_size.value, Some(CellValue::Number(-1.0)));
        assert_eq!(bullet_size.unit, Unit::Points);
        assert_eq!(
            find_cell(row, "TextPosAfterBullet").and_then(|cell| cell.value.clone()),
            Some(CellValue::Number(0.06))
        );
        assert_eq!(
            find_cell(row, "Flags").and_then(|cell| cell.value.clone()),
            Some(CellValue::Number(7.0))
        );
    }

    #[test]
    fn version_6_paragraphs_carry_no_bullet_font_fields() {
        let paragraph = para_payload_v6(1, [0.0; 6], 0, 0, 0, Some("b"));
        let chunks = [shape_chunk(1), text_chunk(PARA_IX, 0, &paragraph)];
        let (document, _) = run(&chunks, 1, &v6_options(CodePage::Windows1252));
        let shape = document.shape(ShapeId { index: 0 }).expect("shape");
        let row = find_section(shape, "Paragraph")
            .expect("section")
            .rows
            .first()
            .expect("row");
        assert_eq!(
            find_cell(row, "BulletFont").and_then(|cell| cell.value.clone()),
            Some(CellValue::Number(0.0))
        );
        assert_eq!(
            find_cell(row, "BulletStr").and_then(|cell| cell.value.clone()),
            Some(CellValue::Str("b".to_owned()))
        );
    }

    #[test]
    fn tab_sets_become_tabs_rows_and_markers() {
        let bytes = text_payload(&utf16("a\tb"));
        let tabs = tabs_payload(3, &[(0.5, 0), (1.0, 2)]);
        let chunks = [
            shape_chunk(1),
            text_chunk(TEXT, 0, &bytes),
            text_chunk(TABS_DATA_2, 0, &tabs),
        ];
        let (document, _) = run_default(&chunks, 1);
        let shape = document.shape(ShapeId { index: 0 }).expect("shape");
        let section = find_section(shape, "Tabs").expect("section");
        assert_eq!(section.rows.len(), 2);
        assert_eq!(
            find_cell(&section.rows[0], "Position").and_then(|cell| cell.value.clone()),
            Some(CellValue::Number(0.5))
        );
        assert_eq!(
            find_cell(&section.rows[1], "Alignment").and_then(|cell| cell.value.clone()),
            Some(CellValue::Number(2.0))
        );
        assert_eq!(
            shape.text.as_ref().expect("text").parts,
            vec![
                TextPart::TabsRun { ix: 0 },
                TextPart::Text("a\tb".to_owned()),
            ]
        );
    }

    #[test]
    fn list_records_order_the_rows() {
        let chars = [
            char_payload(1, 0, [0, 0, 0, 0], 0, 0, 0, 10000, 0.1, 0),
            char_payload(1, 0, [0, 0, 0, 0], 0, 0, 0, 10000, 0.2, 0),
        ];
        let list = list_payload(&[1, 0]);
        let chunks = [
            shape_chunk(1),
            text_chunk(CHAR_LIST, 0, &list),
            text_chunk(CHAR_IX, 0, &chars[0]),
            text_chunk(CHAR_IX, 1, &chars[1]),
        ];
        let (document, _) = run_default(&chunks, 1);
        let shape = document.shape(ShapeId { index: 0 }).expect("shape");
        let rows = &find_section(shape, "Character").expect("section").rows;
        assert_eq!(rows.len(), 2);
        let first = find_cell(&rows[0], "Size").expect("cell");
        assert_eq!(first.value, Some(CellValue::Number(0.2)));
    }

    #[test]
    fn rows_without_a_list_keep_their_stream_order() {
        let chars = [
            char_payload(1, 0, [0, 0, 0, 0], 0, 0, 0, 10000, 0.1, 0),
            char_payload(1, 0, [0, 0, 0, 0], 0, 0, 0, 10000, 0.2, 0),
        ];
        let list = list_payload(&[1]);
        let chunks = [
            shape_chunk(1),
            text_chunk(CHAR_LIST, 0, &list),
            text_chunk(CHAR_IX, 0, &chars[0]),
            text_chunk(CHAR_IX, 1, &chars[1]),
        ];
        let (document, _) = run_default(&chunks, 1);
        let shape = document.shape(ShapeId { index: 0 }).expect("shape");
        let rows = &find_section(shape, "Character").expect("section").rows;
        assert_eq!(rows.len(), 2);
        assert_eq!(
            find_cell(&rows[0], "Size").and_then(|cell| cell.value.clone()),
            Some(CellValue::Number(0.2))
        );
        assert_eq!(
            find_cell(&rows[1], "Size").and_then(|cell| cell.value.clone()),
            Some(CellValue::Number(0.1))
        );
    }

    // ---- ownership and scope ----

    #[test]
    fn text_follows_its_shape_in_stream_order() {
        let one = text_payload(&utf16("one"));
        let two = text_payload(&utf16("two"));
        let chunks = [
            shape_chunk(1),
            text_chunk(TEXT, 0, &one),
            shape_chunk(2),
            text_chunk(TEXT, 0, &two),
        ];
        let (document, _) = run_default(&chunks, 2);
        let first = document.shape(ShapeId { index: 0 }).expect("shape");
        let second = document.shape(ShapeId { index: 1 }).expect("shape");
        assert_eq!(
            first.text.as_ref().expect("text").parts,
            vec![TextPart::Text("one".to_owned())]
        );
        assert_eq!(
            second.text.as_ref().expect("text").parts,
            vec![TextPart::Text("two".to_owned())]
        );
    }

    #[test]
    fn text_without_a_shape_is_dropped_with_a_warning() {
        let bytes = text_payload(&utf16("orphan"));
        let chunks = [text_chunk(TEXT, 0, &bytes)];
        let (document, warnings) = run_default(&chunks, 0);
        assert!(document.shapes.is_empty());
        assert!(
            warnings
                .iter()
                .any(|warning| warning.kind == WarningKind::Dropped { what: "shape text" })
        );
    }

    #[test]
    fn style_character_rows_are_skipped_silently() {
        let chars = [char_payload(1, 0, [0, 0, 0, 0], 0, 0, 0, 10000, 0.1, 0)];
        let chunks = [
            Chunk {
                record: STYLES,
                level: 1,
                list: 0,
                id: 0,
                data: &[],
            },
            Chunk {
                record: STYLE_SHEET,
                level: 2,
                list: 0,
                id: 0,
                data: &[],
            },
            text_chunk(CHAR_IX, 0, &chars[0]),
        ];
        let (document, warnings) = run_default(&chunks, 1);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert!(
            find_section(
                document.shape(ShapeId { index: 0 }).expect("shape"),
                "Character"
            )
            .is_none()
        );
    }

    #[test]
    fn a_shape_without_a_model_shape_keeps_its_text_out() {
        let bytes = text_payload(&utf16("x"));
        let chunks = [shape_chunk(1), text_chunk(TEXT, 0, &bytes)];
        let (document, warnings) = run_default(&chunks, 0);
        assert!(document.shapes.is_empty());
        assert!(
            warnings
                .iter()
                .any(|warning| warning.kind == WarningKind::Dropped { what: "shape text" })
        );
    }

    // ---- malformed records and limits ----

    #[test]
    fn malformed_records_warn_and_skip_without_panicking() {
        let short = [0u8; 4];
        let chunks = [
            shape_chunk(1),
            text_chunk(CHAR_IX, 0, &short),
            text_chunk(PARA_IX, 0, &short),
            text_chunk(TABS_DATA_1, 0, &short),
            text_chunk(TEXT, 0, &short),
        ];
        let (document, warnings) = run_default(&chunks, 1);
        assert!(warnings.len() >= 4, "{warnings:?}");
        assert_eq!(
            document
                .shape(ShapeId { index: 0 })
                .expect("shape")
                .text
                .as_ref()
                .expect("text")
                .parts,
            Vec::new()
        );
    }

    #[test]
    fn non_finite_values_never_reach_the_model() {
        let chars = [char_payload(
            1,
            0,
            [0, 0, 0, 0],
            0,
            0,
            0,
            10000,
            f64::NAN,
            0,
        )];
        let paragraph = para_payload_v11(
            1,
            [f64::INFINITY, 0.0, 0.0, 0.0, 0.0, 0.0],
            0,
            0,
            0,
            0.0,
            0.0,
            0,
            None,
        );
        let tabs = tabs_payload(1, &[(f64::NAN, 0)]);
        let chunks = [
            shape_chunk(1),
            text_chunk(CHAR_IX, 0, &chars[0]),
            text_chunk(PARA_IX, 0, &paragraph),
            text_chunk(TABS_DATA_3, 0, &tabs),
        ];
        let (document, warnings) = run_default(&chunks, 1);
        let shape = document.shape(ShapeId { index: 0 }).expect("shape");
        let char_row = find_section(shape, "Character")
            .expect("section")
            .rows
            .first()
            .expect("row");
        assert_eq!(
            find_cell(char_row, "Size").and_then(|cell| cell.value.clone()),
            Some(CellValue::Number(0.0))
        );
        let para_row = find_section(shape, "Paragraph")
            .expect("section")
            .rows
            .first()
            .expect("row");
        assert_eq!(
            find_cell(para_row, "IndFirst").and_then(|cell| cell.value.clone()),
            Some(CellValue::Number(0.0))
        );
        let tab_row = find_section(shape, "Tabs")
            .expect("section")
            .rows
            .first()
            .expect("row");
        assert_eq!(
            find_cell(tab_row, "Position").and_then(|cell| cell.value.clone()),
            Some(CellValue::Number(0.0))
        );
        assert!(
            warnings
                .iter()
                .filter(|warning| warning.kind == WarningKind::BadValue { what: "font size" })
                .count()
                >= 1
        );
    }

    #[test]
    fn the_text_byte_cap_is_enforced() {
        let bytes = text_payload(&utf16("hello"));
        let chunks = [shape_chunk(1), text_chunk(TEXT, 0, &bytes)];
        let options = TextOptions {
            version: FormatVersion::V11,
            code_page: CodePage::default(),
            limits: Limits {
                max_text_bytes: 3,
                ..Limits::DEFAULT
            },
        };
        let mut document = document_with_shapes(1);
        let mut warnings = Vec::new();
        let error =
            collect_with(&chunks, &mut document, &mut warnings, &options).expect_err("fails");
        assert!(
            error
                .to_string()
                .contains("limit `max_text_bytes` exceeded"),
            "{error}"
        );
    }

    // ---- the vsdx reader comparison (hand-built) ----

    // These fixtures mirror the cases of `crates/vsdx/src/read/text.rs`
    // (`mixed_content_maps_in_document_order` and
    // `a_field_keeps_its_cached_display_text`): the same logical text
    // maps to the same parts through both readers.

    #[test]
    fn parts_match_the_vsdx_reader_mapping() {
        let mut bytes = text_payload(&utf16("Hello world"));
        bytes.extend_from_slice(&utf16("\u{fffc}"));
        let fields = list_payload(&[10]);
        let field = field_string_payload(7);
        let name = name2_payload("42");
        let chars = [char_payload(0, 0, [0, 0, 0, 0], 0, 0, 0, 10000, 0.1, 0)];
        let paras = para_payload_v11(0, [0.0; 6], 0, 0, 0, 0.0, 0.0, 0, None);
        let tabs = tabs_payload(0, &[]);
        let chunks = [
            shape_chunk(1),
            text_chunk(TEXT, 0, &bytes),
            text_chunk(CHAR_IX, 0, &chars[0]),
            text_chunk(PARA_IX, 0, &paras),
            text_chunk(TABS_DATA_4, 0, &tabs),
            text_chunk(FIELD_LIST, 0, &fields),
            text_chunk(TEXT_FIELD, 10, &field),
            Chunk {
                record: NAME2,
                level: 1,
                list: 0,
                id: 7,
                data: &name,
            },
        ];
        let (document, _) = run_default(&chunks, 1);
        let shape = document.shape(ShapeId { index: 0 }).expect("shape");
        // The same parts the `.vsdx` reader builds for
        // `<Text><cp IX='0'/><pp IX='0'/><tp IX='0'/>Hello world<fld IX='0'>42</fld></Text>`.
        assert_eq!(
            shape.text.as_ref().expect("text").parts,
            vec![
                TextPart::CharRun { ix: 0 },
                TextPart::ParaRun { ix: 0 },
                TextPart::TabsRun { ix: 0 },
                TextPart::Text("Hello world".to_owned()),
                TextPart::Field { ix: 0 },
                TextPart::Text("42".to_owned()),
            ]
        );
    }

    #[test]
    fn run_boundaries_match_the_vsdx_reader_mapping() {
        let bytes = text_payload(&utf16("Hello world"));
        let chars = [
            char_payload(6, 0, [0, 0, 0, 0], 0, 0, 0, 10000, 0.1, 0),
            char_payload(5, 0, [0, 0, 0, 0], 0, 0, 0, 10000, 0.2, 0),
        ];
        let chunks = [
            shape_chunk(1),
            text_chunk(TEXT, 0, &bytes),
            text_chunk(CHAR_IX, 0, &chars[0]),
            text_chunk(CHAR_IX, 1, &chars[1]),
        ];
        let (document, _) = run_default(&chunks, 1);
        let shape = document.shape(ShapeId { index: 0 }).expect("shape");
        // The same parts the `.vsdx` reader builds for
        // `<Text><cp IX='0'/>Hello <cp IX='1'/>world</Text>`.
        assert_eq!(
            shape.text.as_ref().expect("text").parts,
            vec![
                TextPart::CharRun { ix: 0 },
                TextPart::Text("Hello ".to_owned()),
                TextPart::CharRun { ix: 1 },
                TextPart::Text("world".to_owned()),
            ]
        );
    }

    // ---- field display formatting ----

    #[test]
    fn number_formats_spell_their_precision() {
        assert_eq!(
            numeric_display(30060.9167, 65, 0),
            Some("30060.9167".to_owned())
        );
        assert_eq!(
            numeric_display(30060.9167, 65, 1),
            Some("30060.9167 in".to_owned())
        );
        assert_eq!(numeric_display(30060.9167, 65, 2), Some("30061".to_owned()));
        assert_eq!(
            numeric_display(30060.9167, 65, 3),
            Some("30061 in".to_owned())
        );
        assert_eq!(
            numeric_display(30060.9167, 65, 4),
            Some("30060.9".to_owned())
        );
        assert_eq!(
            numeric_display(30060.9167, 65, 5),
            Some("30060.9 in".to_owned())
        );
        assert_eq!(
            numeric_display(30060.9167, 65, 6),
            Some("30060.92".to_owned())
        );
        assert_eq!(
            numeric_display(30060.9167, 65, 7),
            Some("30060.92 in".to_owned())
        );
        assert_eq!(
            numeric_display(30060.9167, 65, 8),
            Some("30060.917".to_owned())
        );
        assert_eq!(
            numeric_display(30060.9167, 65, 9),
            Some("30060.917 in".to_owned())
        );
        assert_eq!(numeric_display(0.5, 81, 12), Some("28.6 deg".to_owned()));
        assert_eq!(
            numeric_display(0.5, CELL_TYPE_DMS, 12),
            Some("28 deg 38 min 52 sec".to_owned())
        );
        assert_eq!(numeric_display(1.0, 50, 1), Some("72 pt".to_owned()));
        assert_eq!(numeric_display(f64::NAN, 65, 0), None);
        assert_eq!(numeric_display(1.0, 65, 10), None);
        assert_eq!(numeric_display(1.0, 65, 13), None);
    }

    #[test]
    fn date_and_time_formats_spell_their_patterns() {
        let serial = 30060.0; // 1982-04-19, a Monday
        assert_eq!(numeric_display(serial, 40, 20), Some("Mon".to_owned()));
        assert_eq!(numeric_display(serial, 40, 21), Some("Monday".to_owned()));
        assert_eq!(numeric_display(serial, 40, 22), Some("4/19/82".to_owned()));
        assert_eq!(
            numeric_display(serial, 40, 24),
            Some("Apr 19, 1982".to_owned())
        );
        assert_eq!(
            numeric_display(serial, 40, 25),
            Some("April 19, 1982".to_owned())
        );
        assert_eq!(numeric_display(serial, 40, 26), Some("19/04/82".to_owned()));
        assert_eq!(
            numeric_display(serial, 40, 28),
            Some("19 Apr, 1982".to_owned())
        );
        assert_eq!(
            numeric_display(serial, 40, 29),
            Some("19 April, 1982".to_owned())
        );
        assert_eq!(
            numeric_display(serial, 40, 200),
            Some("4/19/1982".to_owned())
        );
        assert_eq!(
            numeric_display(serial, 40, 201),
            Some("Monday, April 19, 1982".to_owned())
        );
        assert_eq!(
            numeric_display(serial, 40, 202),
            Some("April 19, 1982".to_owned())
        );
        assert_eq!(numeric_display(serial, 40, 203), Some("4/19/82".to_owned()));
        assert_eq!(
            numeric_display(serial, 40, 204),
            Some("1982-04-19".to_owned())
        );
        assert_eq!(
            numeric_display(serial, 40, 205),
            Some("19-Apr-82".to_owned())
        );
        assert_eq!(
            numeric_display(serial, 40, 206),
            Some("4.19.1982".to_owned())
        );
        assert_eq!(
            numeric_display(serial, 40, 207),
            Some("Apr.19, 82".to_owned())
        );
        assert_eq!(
            numeric_display(serial, 40, 208),
            Some("19 April 1982".to_owned())
        );
        assert_eq!(
            numeric_display(serial, 40, 209),
            Some("April 82".to_owned())
        );
        assert_eq!(numeric_display(serial, 40, 210), Some("Apr-82".to_owned()));
        let timed = 30060.9167; // 22:00:02
        assert_eq!(
            numeric_display(timed, 40, 30),
            Some("10:00:02 PM".to_owned())
        );
        assert_eq!(numeric_display(timed, 40, 31), Some("10:00 PM".to_owned()));
        assert_eq!(numeric_display(timed, 40, 32), Some("22:00".to_owned()));
        assert_eq!(numeric_display(timed, 40, 213), Some("10:00 PM".to_owned()));
        assert_eq!(
            numeric_display(timed, 40, 214),
            Some("10:00:02 PM".to_owned())
        );
        assert_eq!(numeric_display(timed, 40, 215), Some("22:00".to_owned()));
        assert_eq!(numeric_display(timed, 40, 216), Some("22:00:02".to_owned()));
        assert_eq!(
            numeric_display(timed, 40, 211),
            Some("4/19/1982 10:00 PM".to_owned())
        );
        assert_eq!(
            numeric_display(timed, 40, 212),
            Some("4/19/1982 10:00:02 PM".to_owned())
        );
    }

    #[test]
    fn every_date_and_time_format_spells_something() {
        let serial = 30060.25;
        for format in (20..=36).chain(44..=45).chain(50..=81).chain(200..=221) {
            assert!(
                numeric_display(serial, 40, format).is_some(),
                "format {format}"
            );
        }
        // 47 to 49 are gaps in the format table and fall back to the
        // number spelling.
        for format in 47..=49 {
            assert!(
                numeric_display(serial, 40, format).is_some(),
                "format {format}"
            );
        }
        assert_eq!(numeric_display(serial, 40, FORMAT_UNKNOWN), None);
    }

    #[test]
    fn a_date_field_record_defaults_to_the_short_date() {
        let mut data = vec![0u8; 7];
        data.push(CELL_TYPE_DATE);
        data.extend_from_slice(&30060.0f64.to_le_bytes());
        data.extend_from_slice(&[0, 0]);
        data.extend_from_slice(&0i32.to_le_bytes());
        let field = parse_field(&data, &TextOptions::default()).expect("field");
        let mut warnings = Vec::new();
        let mut names = HashMap::new();
        names.insert(1u32, "unused".to_owned());
        assert_eq!(field_display(&field, &names, &mut warnings), "4/19/1982");
        assert!(warnings.is_empty());
    }

    #[test]
    fn string_fields_return_their_name_unchanged() {
        let mut names = HashMap::new();
        names.insert(3u32, "MiXeD".to_owned());
        let mut warnings = Vec::new();
        let field = Field::Text { name_id: 3 };
        assert_eq!(field_display(&field, &names, &mut warnings), "MiXeD");
        assert!(warnings.is_empty());
    }

    #[test]
    fn an_unsupported_field_format_warns() {
        let field = Field::Numeric {
            value: 1.0,
            cell_type: CELL_TYPE_INCHES,
            format: 10,
        };
        let mut warnings = Vec::new();
        assert_eq!(field_display(&field, &HashMap::new(), &mut warnings), "");
        assert!(warnings.iter().any(|warning| warning.kind
            == WarningKind::Unsupported {
                what: "field format"
            }));
    }

    #[test]
    fn an_unknown_field_format_warns() {
        let field = Field::Numeric {
            value: 1.0,
            cell_type: CELL_TYPE_INCHES,
            format: FORMAT_UNKNOWN,
        };
        let mut warnings = Vec::new();
        assert_eq!(field_display(&field, &HashMap::new(), &mut warnings), "");
        assert!(warnings.iter().any(|warning| warning.kind
            == WarningKind::Unsupported {
                what: "field format"
            }));
    }

    // ---- helpers ----

    #[test]
    fn trimmed_numbers_drop_trailing_zeros() {
        assert_eq!(trimmed_number(1.0, 4), "1");
        assert_eq!(trimmed_number(1.5, 4), "1.5");
        assert_eq!(trimmed_number(-1.2, 4), "-1.2");
        assert_eq!(trimmed_number(0.0, 2), "0");
    }

    #[test]
    fn civil_days_cover_the_epoch() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
        assert_eq!(civil_from_days(4491), (1982, 4, 19));
    }

    #[test]
    fn lists_parse_their_ids_and_survive_truncation() {
        assert_eq!(parse_list(&list_payload(&[3, 1])), vec![3, 1]);
        assert_eq!(parse_list(&[]), Vec::new());
        assert_eq!(parse_list(&[0, 0]), Vec::new());
        assert_eq!(parse_list(&[1, 0, 0, 0]), Vec::new());
        let mut truncated = list_payload(&[3, 1]);
        truncated.truncate(10);
        assert_eq!(parse_list(&truncated), Vec::new());
    }

    #[test]
    fn ordered_fills_the_gaps_in_stream_order() {
        let items = [(7u32, 1), (3, 2), (9, 3)];
        assert_eq!(ordered(&items, &[9, 7]), vec![&3, &1, &2]);
        assert_eq!(ordered(&items, &[]), vec![&1, &2, &3]);
        assert!(ordered::<i32>(&[], &[]).is_empty());
    }

    // ---- coverage of the defensive branches ----

    #[test]
    fn every_windows1252_byte_decodes() {
        for byte in 0..=255u8 {
            let decoded = decode_bytes(&[byte], CodePage::Windows1252);
            assert!(!decoded.text.is_empty(), "byte {byte}");
        }
    }

    #[test]
    fn utf8_byte_strings_normalize_breaks_and_placeholders() {
        let decoded = decode_bytes("a\r\u{0e}b\u{1e}c".as_bytes(), CodePage::Utf8);
        assert_eq!(decoded.text, "a\n\nb\u{fffc}c");
        let lossy = decode_bytes(&[0xff, 0x41], CodePage::Utf8);
        assert!(lossy.lossy);
        assert!(lossy.text.contains('A'));
    }

    #[test]
    fn character_runs_carry_initial_caps_and_subscript() {
        let chars = [char_payload(
            1,
            0,
            [0, 0, 0, 0],
            0x00,
            0x02,
            0x02,
            10000,
            0.1,
            0,
        )];
        let chunks = [shape_chunk(1), text_chunk(CHAR_IX, 0, &chars[0])];
        let (document, _) = run_default(&chunks, 1);
        let row = find_section(
            document.shape(ShapeId { index: 0 }).expect("shape"),
            "Character",
        )
        .expect("section")
        .rows
        .first()
        .expect("row");
        assert_eq!(
            find_cell(row, "Case").and_then(|cell| cell.value.clone()),
            Some(CellValue::Number(2.0))
        );
        assert_eq!(
            find_cell(row, "Pos").and_then(|cell| cell.value.clone()),
            Some(CellValue::Number(2.0))
        );
    }

    #[test]
    fn truncated_paragraph_blocks_survive() {
        let blocks: [&[u8]; 6] = [
            &[4, 0, 0, 0],
            &[5, 0, 0, 0, 2],
            &[8, 0, 0, 0, 2, 8],
            &[9, 0, 0, 0, 2, 8, 0],
            &[11, 0, 0, 0, 2, 8, 0, 5, 1, 0],
            &[200, 0, 0, 0, 2, 8, 0, 0, 1, 0],
        ];
        for block in blocks {
            let mut paragraph = para_payload_v11(1, [0.0; 6], 0, 0, 0, 0.0, 0.0, 0, None);
            paragraph.extend_from_slice(block);
            let chunks = [shape_chunk(1), text_chunk(PARA_IX, 0, &paragraph)];
            let (document, _) = run_default(&chunks, 1);
            let row = find_section(
                document.shape(ShapeId { index: 0 }).expect("shape"),
                "Paragraph",
            )
            .expect("section")
            .rows
            .first()
            .expect("row");
            assert_eq!(
                find_cell(row, "BulletStr").and_then(|cell| cell.value.clone()),
                Some(CellValue::Str(String::new())),
                "{block:?}"
            );
        }
    }

    #[test]
    fn numeric_field_blocks_survive_truncation() {
        // A zero length, a block that lies beyond the record and a
        // block cut short inside the format marker all end the scan
        // without a format.
        for extra in [
            &[0u8; 4][..],
            &[200, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0][..],
            &[6, 0, 0, 0, 0, 2, 0][..],
        ] {
            let mut data = vec![0u8; 7];
            data.push(CELL_TYPE_INCHES);
            data.extend_from_slice(&1.0f64.to_le_bytes());
            data.extend_from_slice(&[0, 0]);
            data.extend_from_slice(&0i32.to_le_bytes());
            while data.len() < 0x24 {
                data.push(0);
            }
            data.extend_from_slice(extra);
            let field = parse_field(&data, &v6_options(CodePage::Windows1252)).expect("field");
            let mut warnings = Vec::new();
            assert_eq!(field_display(&field, &HashMap::new(), &mut warnings), "");
            assert!(warnings.iter().any(|warning| warning.kind
                == WarningKind::Unsupported {
                    what: "field format"
                }));
        }
    }

    #[test]
    fn unit_strings_and_conversions_cover_the_table() {
        for cell_type in 0..=255u8 {
            let _ = unit_string(cell_type);
            let _ = convert_number(cell_type, 1.0);
        }
        assert_eq!(unit_string(CELL_TYPE_PERCENT), "%");
        assert_eq!(convert_number(CELL_TYPE_PERCENT, 0.5), 50.0);
    }

    #[test]
    fn noon_and_midnight_spell_twelve() {
        assert_eq!(
            numeric_display(30060.5, 40, 31),
            Some("12:00 PM".to_owned())
        );
        assert_eq!(
            numeric_display(30061.0, 40, 31),
            Some("12:00 AM".to_owned())
        );
    }

    #[test]
    fn date_display_rejects_an_unknown_format() {
        let when = date_time(30060.0).expect("date");
        assert_eq!(date_display(&when, 9999), None);
    }

    #[test]
    fn paragraph_list_records_order_the_rows() {
        let paras = [
            para_payload_v11(1, [0.1; 6], 0, 0, 0, 0.0, 0.0, 0, None),
            para_payload_v11(1, [0.2; 6], 0, 0, 0, 0.0, 0.0, 0, None),
        ];
        let list = list_payload(&[1, 0]);
        let chunks = [
            shape_chunk(1),
            text_chunk(PARA_LIST, 0, &list),
            text_chunk(PARA_IX, 0, &paras[0]),
            text_chunk(PARA_IX, 1, &paras[1]),
        ];
        let (document, _) = run_default(&chunks, 1);
        let rows = &find_section(
            document.shape(ShapeId { index: 0 }).expect("shape"),
            "Paragraph",
        )
        .expect("section")
        .rows;
        assert_eq!(
            find_cell(&rows[0], "IndFirst").and_then(|cell| cell.value.clone()),
            Some(CellValue::Number(0.2))
        );
    }

    #[test]
    fn text_record_kinds_outside_the_map_are_ignored() {
        let mut shape = ShapeText::new(0);
        let mut warnings = Vec::new();
        let mut text_bytes = 0;
        let chunk = text_chunk(Record(0x7f), 0, &[]);
        shape
            .read(
                &chunk,
                &TextOptions::default(),
                &mut warnings,
                &mut text_bytes,
            )
            .expect("reads");
        assert!(warnings.is_empty());
    }

    #[test]
    fn a_huge_list_sub_header_is_ignored() {
        let mut data = Vec::new();
        data.extend_from_slice(&200u32.to_le_bytes());
        data.extend_from_slice(&4u32.to_le_bytes());
        assert_eq!(parse_list(&data), Vec::new());
    }

    #[test]
    fn long_font_names_survive_without_a_terminator() {
        let mut data = vec![0u8; 4];
        for index in 0..40u16 {
            data.extend_from_slice(&('a' as u16 + index).to_le_bytes());
        }
        let name = parse_font(&data, &TextOptions::default(), &mut Vec::new()).expect("name");
        assert!(name.starts_with('a'));
    }

    #[test]
    fn version_6_names_and_fonts_decode_with_the_code_page() {
        let mut font = vec![0u8; 6];
        font.extend_from_slice(b"caf\xe9");
        font.push(0);
        let mut name = vec![0u8; 4];
        name.extend_from_slice(b"caf\xe9");
        name.push(0);
        let chars = [char_payload(1, 4, [0, 0, 0, 0], 0, 0, 0, 10000, 0.1, 0)];
        let chunks = [
            shape_chunk(1),
            text_chunk(CHAR_IX, 0, &chars[0]),
            Chunk {
                record: FONT_IX,
                level: 1,
                list: 0,
                id: 4,
                data: &font,
            },
            Chunk {
                record: NAME2,
                level: 1,
                list: 0,
                id: 3,
                data: &name,
            },
        ];
        let options = v6_options(CodePage::Windows1252);
        let (document, warnings) = run(&chunks, 1, &options);
        assert!(warnings.is_empty(), "{warnings:?}");
        let row = find_section(
            document.shape(ShapeId { index: 0 }).expect("shape"),
            "Character",
        )
        .expect("section")
        .rows
        .first()
        .expect("row");
        assert_eq!(
            find_cell(row, "Font").and_then(|cell| cell.value.clone()),
            Some(CellValue::Str("caf\u{e9}".to_owned()))
        );
    }

    #[test]
    fn every_shape_kind_owns_its_text() {
        let one = text_payload(&utf16("one"));
        let two = text_payload(&utf16("two"));
        let three = text_payload(&utf16("three"));
        let chunks = [
            Chunk {
                record: SHAPE_GROUP,
                level: 1,
                list: 0,
                id: 1,
                data: &[],
            },
            text_chunk(TEXT, 0, &one),
            Chunk {
                record: SHAPE_GUIDE,
                level: 1,
                list: 0,
                id: 2,
                data: &[],
            },
            text_chunk(TEXT, 0, &two),
            Chunk {
                record: SHAPE_FOREIGN,
                level: 1,
                list: 0,
                id: 3,
                data: &[],
            },
            text_chunk(TEXT, 0, &three),
        ];
        let (document, _) = run_default(&chunks, 3);
        for (index, expected) in ["one", "two", "three"].iter().enumerate() {
            let shape = document
                .shape(ShapeId {
                    index: index as u32,
                })
                .expect("shape");
            assert_eq!(
                shape.text.as_ref().expect("text").parts,
                vec![TextPart::Text((*expected).to_owned())]
            );
        }
    }

    #[test]
    fn paragraph_blocks_other_than_the_bullet_are_skipped() {
        let mut paragraph = para_payload_v11(1, [0.0; 6], 0, 0, 0, 0.0, 0.0, 0, None);
        paragraph.extend_from_slice(&8u32.to_le_bytes());
        paragraph.extend_from_slice(&[0, 2, 3, 0, 0, 0]);
        paragraph.extend_from_slice(&0u32.to_le_bytes());
        let chunks = [shape_chunk(1), text_chunk(PARA_IX, 0, &paragraph)];
        let (document, _) = run_default(&chunks, 1);
        let row = find_section(
            document.shape(ShapeId { index: 0 }).expect("shape"),
            "Paragraph",
        )
        .expect("section")
        .rows
        .first()
        .expect("row");
        assert_eq!(
            find_cell(row, "BulletStr").and_then(|cell| cell.value.clone()),
            Some(CellValue::Str(String::new()))
        );
    }

    #[test]
    fn field_format_blocks_with_another_index_are_skipped() {
        let mut data = vec![0u8; 7];
        data.push(CELL_TYPE_INCHES);
        data.extend_from_slice(&1.0f64.to_le_bytes());
        data.extend_from_slice(&[0, 0]);
        data.extend_from_slice(&0i32.to_le_bytes());
        while data.len() < 0x36 {
            data.push(0);
        }
        data.extend_from_slice(&6u32.to_le_bytes());
        data.extend_from_slice(&[0, 5, 0, 0]);
        data.extend_from_slice(&0u32.to_le_bytes());
        let field = parse_field(&data, &TextOptions::default()).expect("field");
        let mut warnings = Vec::new();
        assert_eq!(field_display(&field, &HashMap::new(), &mut warnings), "");
        assert!(warnings.iter().any(|warning| warning.kind
            == WarningKind::Unsupported {
                what: "field format"
            }));
    }

    #[test]
    fn trimmed_numbers_can_drop_the_point() {
        assert_eq!(trimmed_number(2.0, 0), "2");
    }

    #[test]
    fn date_values_out_of_range_spell_nothing() {
        assert_eq!(numeric_display(1.0e300, 40, 22), None);
    }

    #[test]
    fn tabs_list_records_order_the_tab_sets() {
        let tabs = [tabs_payload(3, &[(0.5, 0)]), tabs_payload(3, &[(1.0, 1)])];
        let list = list_payload(&[1, 0]);
        let bytes = text_payload(&utf16("a\tb\tc"));
        let chunks = [
            shape_chunk(1),
            text_chunk(TEXT, 0, &bytes),
            text_chunk(TABS_DATA_LIST, 0, &list),
            text_chunk(TABS_DATA_1, 0, &tabs[0]),
            text_chunk(TABS_DATA_1, 1, &tabs[1]),
        ];
        let (document, _) = run_default(&chunks, 1);
        let rows = &find_section(document.shape(ShapeId { index: 0 }).expect("shape"), "Tabs")
            .expect("section")
            .rows;
        assert_eq!(
            find_cell(&rows[0], "Position").and_then(|cell| cell.value.clone()),
            Some(CellValue::Number(1.0))
        );
    }

    #[test]
    fn a_field_display_over_the_cap_fails_the_flush() {
        let mut bytes = text_payload(&utf16(""));
        bytes.extend_from_slice(&utf16("\u{fffc}"));
        let fields = list_payload(&[10]);
        let field = field_string_payload(7);
        let name = name2_payload("long display");
        let chunks = [
            shape_chunk(1),
            text_chunk(TEXT, 0, &bytes),
            text_chunk(FIELD_LIST, 0, &fields),
            text_chunk(TEXT_FIELD, 10, &field),
            Chunk {
                record: NAME2,
                level: 1,
                list: 0,
                id: 7,
                data: &name,
            },
        ];
        let options = TextOptions {
            version: FormatVersion::V11,
            code_page: CodePage::default(),
            limits: Limits {
                max_text_bytes: 4,
                ..Limits::DEFAULT
            },
        };
        let mut document = document_with_shapes(1);
        let mut warnings = Vec::new();
        let error =
            collect_with(&chunks, &mut document, &mut warnings, &options).expect_err("fails");
        assert!(error.to_string().contains("max_text_bytes"), "{error}");
    }
}
