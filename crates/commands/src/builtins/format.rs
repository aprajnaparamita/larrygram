//! `format.*`, `text.*` and `tools.format_painter` commands (task
//! P7-21): the formatting half of the basic set.
//!
//! Every command writes the ShapeSheet cells the renderer reads
//! (`crates/render/src/build.rs`), over **every selected shape**:
//!
//! - **Character** rows (one per text run; row 0 is created when the
//!   shape has none): `Font` (family name), `Size` (inches),
//!   `Style` (bit 1 bold, 2 italic, 4 underline), `Strikethru`,
//!   `Color`;
//! - **Paragraph** rows: `HorzAlign` (0 left, 1 centre, 2 right,
//!   3 justify) and `Bullet` (0 off, 1 the first bullet style);
//! - the sheet cell `VerticalAlign` (0 top, 1 middle, 2 bottom);
//! - **Fill**: `FillForegnd`, `FillPattern` (0 none, 1 solid),
//!   `FillForegndTrans` (fraction 0 to 1);
//! - **Line**: `LineColor`, `LineWeight` (inches), `LinePattern`
//!   (0 none), `BeginArrow` and `EndArrow` (style index, 0 none).
//!
//! A write lands on the sheet's own cells (a local row is created for a
//! shape that inherits its rows from a master: the row's other cells
//! keep resolving through the master rung, ADR 0019). A "use default"
//! write removes the local entry again, so the cell resolves back
//! through the inheritance chain — the model spells `Inh` as "no
//! entry" (ADR 0038).
//!
//! # Argument contract
//!
//! The colour, font, size, weight, pattern and arrow commands take
//! their choice as `value` (what the layout lists dispatch, P7-14) or
//! under their own name (`color`, `font`, `size`, `weight`, `pattern`,
//! `transparency`, `begin`, `end`); the first argument present wins:
//!
//! - `text.font` — `value`/`font`: the family name;
//! - `text.font.size`, `text.font.size.increase`, `.decrease` and the
//!   `format.text.increase_size` / `format.text.decrease_size` aliases
//!   — `value`/`size`: the size in **points** (the picker's unit; the
//!   cell stores inches, `Size` in the renderer's table);
//! - `format.text.bold`, `format.text.italic`, `format.text.underline`
//!   and `tools.strikethrough` — no arguments: each toggles;
//! - `text.font.color`, `format.set.fill.color`,
//!   `format.set.line.color` — `value`/`color`: `#RGB`, `#RRGGBB` or a
//!   colour name (`black`, `white`, `red`, `green`, `blue`, `yellow`,
//!   `orange`, `purple`, `gray`, `brown`, `pink`);
//! - `format.text_color.use_default`, `format.fill_color.use_default`,
//!   `format.line_color.use_default` — no arguments: the cell goes back
//!   to its inherited value;
//! - `format.text.align_left|center|right|justify`,
//!   `format.text.align_top|middle|bottom` — no arguments;
//! - `text.bullets` — no arguments: bullets on (style 1) or off;
//! - `format.fill_color.no_fill`, `format.line_color.no_line` and
//!   `format.line_pattern.no_line` — no arguments;
//! - `shape.transparency` (and its dialog form
//!   `shape.transparency.dlg`) — `value`/`transparency`: percent (0 to
//!   100); `format.transparency.no_transparency` sets 0;
//! - `format.set.line.weight` — `value`/`weight`: the weight in
//!   **points**;
//! - `format.set.line.pattern` — `value`/`pattern`: the pattern index
//!   (1 is solid) or `none`;
//! - `format.set.line.ends` — `value`/`begin`/`end`: the arrowhead
//!   index (0 none) or `none`; `value` alone sets both ends and
//!   `format.line_ends.no_line_ends` clears both;
//! - `tools.format_painter` — no arguments: copy the format of the
//!   selection's anchor shape (or, with a copy already recorded, apply
//!   it to the selection); `apply` (a flag) forces the apply form and
//!   `cancel` (a flag) drops a recorded copy without applying (task
//!   P7-24 item 6, Escape on the canvas).
//! - `format.fill` (task P7-28) — `color`: the fill colour (`FillForegnd`),
//!   `pattern`: the fill pattern index (`FillPattern`, `none`/`solid`
//!   or a pattern name), `background`: the pattern colour
//!   (`FillBkgnd`) and `transparency`: percent (`FillForegndTrans`);
//!   every argument is optional and with none of them the command
//!   requests the Fill dialog (`UiRequest::ShowFillDialog`) and
//!   changes nothing;
//! - `format.line` (task P7-28) — `color` (`LineColor`), `weight`
//!   (points, `LineWeight`), `pattern` (`LinePattern`), `cap`
//!   (`LineCap`: `round`, `square`, `extended` or 0 to 2),
//!   `transparency` (percent, `LineColorTrans`), `begin` and `end`
//!   (arrowhead index or name, `BeginArrow`/`EndArrow`), `begin_size`
//!   and `end_size` (arrowhead size class 0 to 6, or `very small` ..
//!   `colossal`, `BeginArrowSize`/`EndArrowSize`) and `rounding`
//!   (corner radius in inches, `Rounding`); with no arguments it
//!   requests the Line dialog (`UiRequest::ShowLineDialog`);
//! - `format.shape.pane` — no arguments: requests the Format Shape
//!   pane (`UiRequest::ShowFormatShape`) and writes nothing.
//!
//! Every run is one undoable edit ([`FormatEdit`], ADR 0040) and
//! writes the cells of every selected shape in one step.

use larrygram_model::{
    CellName, CellReader, CellValue, CellWrite, Document, Row, RowTarget, Section, SectionName,
    ShapeId, Sheet, SheetRef,
};
use larrygram_shapesheet::CellAddress;

use crate::app::{AppCtx, SessionId};
use crate::error::CommandError;
use crate::undo::Edit;
use crate::{Invocation, Outcome, UiRequest};

use super::edit::{document_mut, number_literal, selection_targets};
use super::{CommandTable, edit_outcome, model_error, outcome};

/// The implemented formatting commands (task P7-21).
pub(crate) const TABLE: CommandTable = &[
    ("text.font", "Font", can_format, run_font),
    ("text.font.size", "Font Size", can_format, run_font_size),
    (
        "text.font.size.increase",
        "Increase Font Size",
        can_format,
        run_font_grow,
    ),
    (
        "text.font.size.decrease",
        "Decrease Font Size",
        can_format,
        run_font_shrink,
    ),
    (
        "format.text.increase_size",
        "Increase Font Size",
        can_format,
        run_font_grow,
    ),
    (
        "format.text.decrease_size",
        "Decrease Font Size",
        can_format,
        run_font_shrink,
    ),
    ("format.text.bold", "Bold", can_format, run_bold),
    ("format.text.italic", "Italic", can_format, run_italic),
    (
        "format.text.underline",
        "Underline",
        can_format,
        run_underline,
    ),
    (
        "tools.strikethrough",
        "Strikethrough",
        can_format,
        run_strikethrough,
    ),
    // The Panel Classic Format Text toolbar's strikethrough id (task
    // P7-21): the same toggle under the id its data links.
    (
        "text.strike.through",
        "Strikethrough",
        can_format,
        run_strikethrough,
    ),
    ("text.font.color", "Font Color", can_format, run_font_color),
    (
        "format.text_color.use_default",
        "Use Default Font Color",
        can_format,
        run_font_color_default,
    ),
    (
        "format.text.align_left",
        "Align Text Left",
        can_format,
        run_align_left,
    ),
    (
        "format.text.align_center",
        "Center Text",
        can_format,
        run_align_center,
    ),
    (
        "format.text.align_right",
        "Align Text Right",
        can_format,
        run_align_right,
    ),
    (
        "format.text.align_justify",
        "Justify Text",
        can_format,
        run_align_justify,
    ),
    (
        "format.text.align_top",
        "Align Text Top",
        can_format,
        run_align_top,
    ),
    (
        "format.text.align_middle",
        "Align Text Middle",
        can_format,
        run_align_middle,
    ),
    (
        "format.text.align_bottom",
        "Align Text Bottom",
        can_format,
        run_align_bottom,
    ),
    ("text.bullets", "Bullets", can_format, run_bullets),
    (
        "format.set.fill.color",
        "Fill Color",
        can_format,
        run_fill_color,
    ),
    (
        "format.fill_color.no_fill",
        "No Fill",
        can_format,
        run_fill_no_fill,
    ),
    (
        "format.fill_color.use_default",
        "Use Default Fill Color",
        can_format,
        run_fill_default,
    ),
    (
        "shape.transparency",
        "Transparency",
        can_format,
        run_transparency,
    ),
    (
        "shape.transparency.dlg",
        "Transparency",
        can_format,
        run_transparency,
    ),
    (
        "format.transparency.no_transparency",
        "No Transparency",
        can_format,
        run_no_transparency,
    ),
    (
        "format.set.line.color",
        "Line Color",
        can_format,
        run_line_color,
    ),
    (
        "format.line_color.use_default",
        "Use Default Line Color",
        can_format,
        run_line_default,
    ),
    (
        "format.line_color.no_line",
        "No Line",
        can_format,
        run_line_no_line,
    ),
    (
        "format.set.line.weight",
        "Line Weight",
        can_format,
        run_line_weight,
    ),
    (
        "format.set.line.pattern",
        "Line Pattern",
        can_format,
        run_line_pattern,
    ),
    (
        "format.line_pattern.no_line",
        "No Line",
        can_format,
        run_line_no_line,
    ),
    (
        "format.set.line.ends",
        "Line Ends",
        can_format,
        run_line_ends,
    ),
    (
        "format.line_ends.no_line_ends",
        "No Line Ends",
        can_format,
        run_line_no_ends,
    ),
    (
        "tools.format_painter",
        "Format Painter",
        can_paint,
        run_format_painter,
    ),
    // Task P7-28: the Fill and Line dialogs and the Format Shape pane.
    ("format.fill", "Fill", can_format, run_fill),
    ("format.line", "Line", can_format, run_line),
    (
        "format.shape.pane",
        "Format Shape",
        super::has_document,
        run_shape_pane,
    ),
];

/// The text-size ladder, in points (grow and shrink step along it).
const SIZE_STEPS_PT: [f64; 18] = [
    8.0, 9.0, 10.0, 11.0, 12.0, 14.0, 16.0, 18.0, 20.0, 22.0, 24.0, 26.0, 28.0, 36.0, 48.0, 60.0,
    72.0, 96.0,
];

/// The largest font size the grow step reaches (points).
const MAX_SIZE_PT: f64 = 96.0;

/// The smallest font size the shrink step reaches (points).
const MIN_SIZE_PT: f64 = 1.0;

/// The points in one inch (the `Size` and `LineWeight` cells store
/// inches; the pickers show points).
const POINTS_PER_INCH: f64 = 72.0;

/// Where one format cell lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Place {
    /// A direct cell of the sheet.
    Direct,
    /// A cell of every `Character` row (row 0 when the shape has none).
    Character,
    /// A cell of every `Paragraph` row (row 0 when the shape has none).
    Paragraph,
}

/// One formatting write on one shape: the place, the cell and the value
/// (`None` removes the local entry, so the cell inherits again).
#[derive(Debug, Clone)]
pub(crate) struct Op {
    /// The shape to write.
    pub shape: ShapeId,
    /// Where the cell lives.
    pub place: Place,
    /// The cell name (`Font`, `FillForegnd`, ...).
    pub cell: &'static str,
    /// The value to write, or `None` to restore inheritance.
    pub write: Option<CellWrite>,
}

/// One write of a direct sheet cell.
pub(crate) fn direct(shape: ShapeId, cell: &'static str, write: CellWrite) -> Op {
    Op {
        shape,
        place: Place::Direct,
        cell,
        write: Some(write),
    }
}

/// One write of a cell of every `Character` row.
pub(crate) fn character(shape: ShapeId, cell: &'static str, write: CellWrite) -> Op {
    Op {
        shape,
        place: Place::Character,
        cell,
        write: Some(write),
    }
}

/// One write of a cell of every `Paragraph` row.
pub(crate) fn paragraph(shape: ShapeId, cell: &'static str, write: CellWrite) -> Op {
    Op {
        shape,
        place: Place::Paragraph,
        cell,
        write: Some(write),
    }
}

/// One removal of a local cell entry (`use default`).
pub(crate) fn remove(shape: ShapeId, place: Place, cell: &'static str) -> Op {
    Op {
        shape,
        place,
        cell,
        write: None,
    }
}

/// The literal write of a number cell (the one spelling rule, ADR 0038).
pub(crate) fn num(value: f64) -> CellWrite {
    CellWrite::new(number_literal(value), CellValue::Number(value))
}

/// The literal write of a text cell (a quoted string formula).
pub(crate) fn text_write(text: &str) -> CellWrite {
    let escaped = text.replace('"', "\"\"");
    CellWrite::new(format!("\"{escaped}\""), CellValue::Str(text.to_owned()))
}

/// The literal write of a boolean cell.
pub(crate) fn bool_write(flag: bool) -> CellWrite {
    let formula = if flag { "TRUE" } else { "FALSE" };
    CellWrite::new(formula, CellValue::Bool(flag))
}

/// The literal write of a colour cell (`RGB(r,g,b)`, the spelling the
/// formula engine writes back unchanged).
pub(crate) fn color_write(color: larrygram_model::Color) -> CellWrite {
    CellWrite::new(
        format!("RGB({},{},{})", color.r, color.g, color.b),
        CellValue::Color(color),
    )
}

// ---------------------------------------------------------------------------
// The undoable step
// ---------------------------------------------------------------------------

/// One undoable formatting step (ADR 0040): the writes of one command
/// over the selection, and the exact prior sheets they overwrite.
///
/// `apply` writes every op (a missing `Character` or `Paragraph` row is
/// created first, so a shape that inherits its text style gets local
/// rows with just the written cells; the other cells keep resolving
/// through the master rung). `revert` restores the captured sheets
/// wholesale, so `revert(apply(x)) == x` holds exactly — including a
/// write that created a row or removed an entry.
#[derive(Debug)]
pub(crate) struct FormatEdit {
    session: SessionId,
    label: String,
    ops: Vec<Op>,
    /// The touched shapes' sheets before the first write.
    before: Vec<(ShapeId, Sheet)>,
    /// The cells the writes touch (the recalc pass's dirty set). A
    /// removal is not dirty: it restores inheritance and recomputing it
    /// would materialise the inherited formula back onto the sheet.
    dirty: Vec<CellAddress>,
}

impl FormatEdit {
    /// Stages the writes, capturing the sheets they will touch.
    pub(crate) fn new(
        doc: &Document,
        session: SessionId,
        label: impl Into<String>,
        ops: Vec<Op>,
    ) -> Result<Self, CommandError> {
        let mut before: Vec<(ShapeId, Sheet)> = Vec::new();
        let mut dirty = Vec::new();
        for op in &ops {
            let Some(entry) = doc.shape(op.shape) else {
                return Err(CommandError::failed(format!(
                    "unknown shape id {}",
                    op.shape.index
                )));
            };
            if !before.iter().any(|(id, _)| *id == op.shape) {
                before.push((op.shape, entry.sheet.clone()));
            }
            if op.write.is_none() {
                continue;
            }
            let sheet = SheetRef::Shape(op.shape);
            match op.place {
                Place::Direct => dirty.push(CellAddress::new(sheet, CellName::new(op.cell))),
                Place::Character | Place::Paragraph => {
                    for target in row_targets(&entry.sheet, section_name(op.place)) {
                        dirty.push(CellAddress::in_row(
                            sheet,
                            SectionName::from_static(section_name(op.place)),
                            target,
                            CellName::new(op.cell),
                        ));
                    }
                }
            }
        }
        Ok(Self {
            session,
            label: label.into(),
            ops,
            before,
            dirty,
        })
    }

    /// Writes every op (apply and redo).
    fn write(&self, ctx: &mut AppCtx) -> Result<(), CommandError> {
        for op in &self.ops {
            let sheet = SheetRef::Shape(op.shape);
            match (op.place, &op.write) {
                (Place::Direct, Some(write)) => {
                    let doc = document_mut(ctx, self.session)?;
                    doc.write_cell(sheet, &CellName::new(op.cell), write)
                        .map_err(model_error)?;
                }
                (Place::Direct, None) => {
                    let doc = document_mut(ctx, self.session)?;
                    let Some(entity) = doc.shape_mut(op.shape) else {
                        return Err(CommandError::failed(format!(
                            "unknown shape id {}",
                            op.shape.index
                        )));
                    };
                    entity
                        .sheet
                        .cells
                        .retain(|cell| cell.name.as_str() != op.cell);
                }
                (Place::Character | Place::Paragraph, Some(write)) => {
                    let section = section_name(op.place);
                    let doc = document_mut(ctx, self.session)?;
                    let rows = ensure_rows(doc, op.shape, section)?;
                    for target in rows {
                        doc.write_row_cell(
                            sheet,
                            &SectionName::from_static(section),
                            target,
                            &CellName::new(op.cell),
                            write,
                        )
                        .map_err(model_error)?;
                    }
                }
                (Place::Character | Place::Paragraph, None) => {
                    let section = section_name(op.place);
                    let doc = document_mut(ctx, self.session)?;
                    let Some(entity) = doc.shape_mut(op.shape) else {
                        return Err(CommandError::failed(format!(
                            "unknown shape id {}",
                            op.shape.index
                        )));
                    };
                    if let Some(block) = entity
                        .sheet
                        .sections
                        .iter_mut()
                        .find(|entry| entry.name.as_str() == section)
                    {
                        for row in block.rows.iter_mut() {
                            row.cells.retain(|cell| cell.name.as_str() != op.cell);
                        }
                    }
                }
            }
        }
        ctx.touch_session(self.session);
        Ok(())
    }
}

impl Edit for FormatEdit {
    fn label(&self) -> &str {
        &self.label
    }

    fn dirty_cells(&self) -> &[CellAddress] {
        &self.dirty
    }

    fn apply(&mut self, ctx: &mut AppCtx) -> Result<(), CommandError> {
        self.write(ctx)
    }

    fn revert(&mut self, ctx: &mut AppCtx) -> Result<(), CommandError> {
        for (shape, sheet) in &self.before {
            let doc = document_mut(ctx, self.session)?;
            let Some(entity) = doc.shape_mut(*shape) else {
                return Err(CommandError::failed(format!(
                    "unknown shape id {}",
                    shape.index
                )));
            };
            entity.sheet = sheet.clone();
        }
        ctx.touch_session(self.session);
        Ok(())
    }
}

/// The section name of one row place.
fn section_name(place: Place) -> &'static str {
    match place {
        Place::Character | Place::Direct => "Character",
        Place::Paragraph => "Paragraph",
    }
}

/// The rows one row write lands on: every live row of the section, or
/// the one row the caller will create when the section has none.
fn row_targets(sheet: &Sheet, section: &str) -> Vec<RowTarget> {
    let Some(block) = sheet
        .sections
        .iter()
        .find(|entry| entry.name.as_str() == section)
    else {
        return vec![RowTarget {
            position: Some(0),
            ix: None,
        }];
    };
    let targets: Vec<RowTarget> = block
        .rows
        .iter()
        .enumerate()
        .filter(|(_, row)| !row.is_deleted())
        .map(|(position, row)| RowTarget {
            position: Some(position),
            ix: row.ix,
        })
        .collect();
    if targets.is_empty() {
        vec![RowTarget {
            position: Some(0),
            ix: None,
        }]
    } else {
        targets
    }
}

/// The rows one row write lands on, creating the section and its first
/// row when the shape has none (a shape that inherits its text rows
/// from a master gets a local row with just the written cells).
fn ensure_rows(
    doc: &mut Document,
    shape: ShapeId,
    section: &'static str,
) -> Result<Vec<RowTarget>, CommandError> {
    let Some(entity) = doc.shape_mut(shape) else {
        return Err(CommandError::failed(format!(
            "unknown shape id {}",
            shape.index
        )));
    };
    let live = entity
        .sheet
        .sections
        .iter()
        .find(|entry| entry.name.as_str() == section)
        .map(|block| {
            block
                .rows
                .iter()
                .enumerate()
                .filter(|(_, row)| !row.is_deleted())
                .map(|(position, row)| RowTarget {
                    position: Some(position),
                    ix: row.ix,
                })
                .collect::<Vec<RowTarget>>()
        })
        .unwrap_or_default();
    if !live.is_empty() {
        return Ok(live);
    }
    let Some(block) = entity
        .sheet
        .sections
        .iter_mut()
        .find(|entry| entry.name.as_str() == section)
    else {
        entity
            .sheet
            .sections
            .push(Section::new(SectionName::from_static(section)));
        let Some(created) = entity.sheet.sections.last_mut() else {
            return Err(CommandError::failed("the text section could not be added"));
        };
        let mut row = Row::new();
        // A text run marker names its row by `IX` (ADR 0019 decision
        // 5), so the created row carries the `IX` the markers use.
        row.ix = Some(0);
        created.rows.push(row);
        return Ok(vec![RowTarget {
            position: Some(0),
            ix: Some(0),
        }]);
    };
    let ix = next_row_ix(block);
    let mut row = Row::new();
    row.ix = Some(ix);
    block.rows.push(row);
    let last = block.rows.len().saturating_sub(1);
    Ok(vec![RowTarget {
        position: Some(last),
        ix: Some(ix),
    }])
}

/// The `IX` of the next row of a section: 0 when no row claims it, else
/// one past the largest `IX` (a text run marker names rows by `IX`).
fn next_row_ix(block: &Section) -> i32 {
    let mut largest = -1;
    for row in &block.rows {
        if let Some(ix) = row.ix {
            largest = largest.max(ix);
        }
    }
    largest.saturating_add(1).max(0)
}

// ---------------------------------------------------------------------------
// Reads (the toggles and the format painter)
// ---------------------------------------------------------------------------

/// The effective number of one row cell (row 0), or `None` when the
/// shape has no such value.
fn read_row_number(doc: &Document, shape: ShapeId, section: &str, cell: &str) -> Option<f64> {
    let mut reader = CellReader::new(doc);
    reader
        .row_number(
            SheetRef::Shape(shape),
            section,
            RowTarget {
                position: Some(0),
                ix: None,
            },
            &CellName::new(cell),
        )
        .ok()
}

/// The effective number of one direct sheet cell.
fn read_number(doc: &Document, shape: ShapeId, cell: &str) -> Option<f64> {
    let mut reader = CellReader::new(doc);
    reader
        .number(SheetRef::Shape(shape), &CellName::new(cell))
        .ok()
}

/// The effective boolean of one row cell (row 0).
fn read_row_boolean(doc: &Document, shape: ShapeId, section: &str, cell: &str) -> Option<bool> {
    let mut reader = CellReader::new(doc);
    reader
        .row_boolean(
            SheetRef::Shape(shape),
            section,
            RowTarget {
                position: Some(0),
                ix: None,
            },
            &CellName::new(cell),
        )
        .ok()
}

/// The effective text of one row cell (row 0).
fn read_row_text(doc: &Document, shape: ShapeId, section: &str, cell: &str) -> Option<String> {
    let mut reader = CellReader::new(doc);
    reader
        .row_text(
            SheetRef::Shape(shape),
            section,
            RowTarget {
                position: Some(0),
                ix: None,
            },
            &CellName::new(cell),
        )
        .ok()
}

/// The effective colour of one row cell (row 0).
fn read_row_color(
    doc: &Document,
    shape: ShapeId,
    section: &str,
    cell: &str,
) -> Option<larrygram_model::Color> {
    let mut reader = CellReader::new(doc);
    reader
        .row_color(
            SheetRef::Shape(shape),
            section,
            RowTarget {
                position: Some(0),
                ix: None,
            },
            &CellName::new(cell),
        )
        .ok()
}

/// The effective colour of one direct sheet cell.
fn read_color(doc: &Document, shape: ShapeId, cell: &str) -> Option<larrygram_model::Color> {
    let mut reader = CellReader::new(doc);
    reader
        .color(SheetRef::Shape(shape), &CellName::new(cell))
        .ok()
}

// ---------------------------------------------------------------------------
// Shared run helpers
// ---------------------------------------------------------------------------

/// Enabled when a document is open and at least one selected shape is
/// still on a page.
fn can_format(ctx: &AppCtx) -> crate::Enabled {
    if ctx.active_document().is_none() {
        return crate::Enabled::no("no open document");
    }
    if ctx.selection.is_empty() {
        return crate::Enabled::no("no selection");
    }
    let live = ctx.active_document().is_some_and(|session| {
        ctx.selection
            .shapes
            .iter()
            .any(|id| session.document.page_of(*id).is_some())
    });
    if live {
        crate::Enabled::yes()
    } else {
        crate::Enabled::no("the selected shapes are stale")
    }
}

/// `tools.format_painter` is enabled with a selection: it copies from
/// the selection, or applies a recorded copy to it.
fn can_paint(ctx: &AppCtx) -> crate::Enabled {
    if ctx.format_painter.is_some() {
        return can_format(ctx);
    }
    can_format(ctx)
}

/// Builds the writes for every selected shape and runs them as one
/// edit. `build` sees the document and the live targets (a toggle
/// reads the first target once and writes the same value everywhere).
fn format_run(
    ctx: &mut AppCtx,
    label: impl Into<String>,
    build: impl FnOnce(&Document, &[ShapeId]) -> Result<Vec<Op>, CommandError>,
) -> Result<Outcome, CommandError> {
    let verb = label.into();
    let (session, targets, warnings) = selection_targets(ctx)?;
    let ops = {
        let doc = ctx
            .documents
            .iter()
            .find(|slot| slot.id == session)
            .map(|slot| &slot.document)
            .ok_or_else(|| CommandError::failed("the document session is gone"))?;
        build(doc, &targets)?
    };
    let message = count_message(&verb, targets.len());
    let mut edit = {
        let doc = document_mut(ctx, session)?;
        FormatEdit::new(doc, session, message.clone(), ops)?
    };
    edit.apply(ctx)?;
    Ok(edit_outcome(message, warnings, Box::new(edit)))
}

/// "Bold" / "Bold 3 shapes": the status message and undo label.
fn count_message(verb: &str, count: usize) -> String {
    if count == 1 {
        verb.to_owned()
    } else {
        format!("{verb} ({count} shapes)")
    }
}

/// One required text argument, tried under each name.
fn need_text(inv: &Invocation, names: &[&str], prompt: &str) -> Result<String, CommandError> {
    match arg_text(inv, names) {
        Some(text) => Ok(text),
        None => Err(CommandError::NeedsInput(crate::NeededInput::Text {
            prompt: prompt.to_owned(),
            default: None,
        })),
    }
}

/// The text of a numeric argument without its optional trailing `pt`
/// in any case (the picker labels spell points: "0.25 pt", and a typed
/// value may be "0.25 PT" or "0.25 Pt"; `typed_number` trims the same
/// suffix).
fn strip_pt(text: &str) -> &str {
    let trimmed = text.trim();
    match trimmed.split_at_checked(trimmed.len().saturating_sub(2)) {
        Some((head, tail)) if tail.eq_ignore_ascii_case("pt") => head.trim_end(),
        _ => trimmed,
    }
}

/// The first argument present under any of `names`, spelled as text.
fn arg_text(inv: &Invocation, names: &[&str]) -> Option<String> {
    for name in names {
        match inv.args.get(name) {
            Some(crate::ArgValue::Text(text)) => return Some(text.clone()),
            Some(crate::ArgValue::Number(value)) => return Some(number_literal(*value)),
            Some(crate::ArgValue::Integer(value)) => return Some(value.to_string()),
            Some(crate::ArgValue::Flag(flag)) => return Some(flag.to_string()),
            Some(_) | None => {}
        }
    }
    None
}

/// The first numeric argument present under any of `names`. A text
/// argument must parse as a finite number, with an optional trailing
/// `pt` (the line weight and font size pickers spell their values
/// with one: "2.25 pt").
fn arg_number(inv: &Invocation, names: &[&str]) -> Result<Option<f64>, CommandError> {
    for name in names {
        match inv.args.get(name) {
            Some(crate::ArgValue::Number(value)) if value.is_finite() => {
                return Ok(Some(*value));
            }
            Some(crate::ArgValue::Number(_)) => {
                return Err(CommandError::invalid_args(format!("{name} must be finite")));
            }
            Some(crate::ArgValue::Integer(value)) => return Ok(Some(*value as f64)),
            Some(crate::ArgValue::Text(text)) => {
                return match strip_pt(text).parse::<f64>() {
                    Ok(value) if value.is_finite() => Ok(Some(value)),
                    Ok(_) => Err(CommandError::invalid_args(format!("{name} must be finite"))),
                    Err(_) => Err(CommandError::invalid_args(format!(
                        "{name} must be a number"
                    ))),
                };
            }
            Some(_) | None => {}
        }
    }
    Ok(None)
}

/// A required number argument under any of `names` (the programmatic
/// contract of `text.font.size` and friends).
fn need_number(inv: &Invocation, names: &[&str], prompt: &str) -> Result<f64, CommandError> {
    match arg_number(inv, names)? {
        Some(value) => Ok(value),
        None => Err(CommandError::NeedsInput(crate::NeededInput::Number {
            prompt: prompt.to_owned(),
            default: None,
        })),
    }
}

/// The colour the arguments name (`value`/`color`): a hex literal or a
/// colour name.
fn need_color(inv: &Invocation, names: &[&str]) -> Result<larrygram_model::Color, CommandError> {
    let text = need_text(inv, names, "color")?;
    parse_color(&text).ok_or_else(|| {
        CommandError::invalid_args(format!(
            "unknown color {text:?}; use #RGB, #RRGGBB or a color name"
        ))
    })
}

/// The colour of a `#RGB`, `#RRGGBB` or name spelling.
pub(crate) fn parse_color(text: &str) -> Option<larrygram_model::Color> {
    let trimmed = text.trim();
    if let Some(hex) = trimmed.strip_prefix('#') {
        return parse_hex_color(hex);
    }
    let key = trimmed.to_ascii_lowercase().replace([' ', '-', '_'], "");
    let (r, g, b) = match key.as_str() {
        "black" => (0x00, 0x00, 0x00),
        "white" => (0xFF, 0xFF, 0xFF),
        "red" => (0xFF, 0x00, 0x00),
        "green" => (0x00, 0x80, 0x00),
        "blue" => (0x00, 0x00, 0xFF),
        "yellow" => (0xFF, 0xFF, 0x00),
        "orange" => (0xFF, 0xA5, 0x00),
        "purple" => (0x80, 0x00, 0x80),
        "gray" | "grey" => (0x80, 0x80, 0x80),
        "brown" => (0x8B, 0x45, 0x13),
        "pink" => (0xFF, 0xC0, 0xCB),
        _ => return None,
    };
    Some(larrygram_model::Color::new(r, g, b))
}

/// The colour of a hex literal without the `#` (`RGB` or `RRGGBB`).
fn parse_hex_color(hex: &str) -> Option<larrygram_model::Color> {
    let nibble =
        |at: usize| -> Option<u8> { u8::from_str_radix(hex.get(at..at.checked_add(1)?)?, 16).ok() };
    let byte =
        |at: usize| -> Option<u8> { u8::from_str_radix(hex.get(at..at.checked_add(2)?)?, 16).ok() };
    match hex.len() {
        3 => Some(larrygram_model::Color::new(
            nibble(0)? * 17,
            nibble(1)? * 17,
            nibble(2)? * 17,
        )),
        6 => Some(larrygram_model::Color::new(byte(0)?, byte(2)?, byte(4)?)),
        _ => None,
    }
}

/// The display spelling of a colour (`#RRGGBB`), used by the pickers'
/// accessible names and the tests.
pub(crate) fn color_name(color: larrygram_model::Color) -> String {
    format!("#{:02X}{:02X}{:02X}", color.r, color.g, color.b)
}

/// Points to the inches the `Size` and `LineWeight` cells store.
fn points_to_inches(points: f64) -> f64 {
    points / POINTS_PER_INCH
}

/// The inches of a `Size` or `LineWeight` cell to display points.
fn inches_to_points(inches: f64) -> f64 {
    inches * POINTS_PER_INCH
}

// ---------------------------------------------------------------------------
// Text commands
// ---------------------------------------------------------------------------

/// `text.font`: the family of every text run.
fn run_font(ctx: &mut AppCtx, inv: &Invocation) -> Result<Outcome, CommandError> {
    let family = need_text(inv, &["font", "value"], "font family")?;
    if family.trim().is_empty() {
        return Err(CommandError::invalid_args("font family must not be empty"));
    }
    format_run(ctx, "Set font", |_, targets| {
        Ok(targets
            .iter()
            .map(|shape| character(*shape, "Font", text_write(&family)))
            .collect())
    })
}

/// `text.font.size`: the size of every text run, in points.
fn run_font_size(ctx: &mut AppCtx, inv: &Invocation) -> Result<Outcome, CommandError> {
    let points = need_number(inv, &["size", "value"], "font size (points)")?;
    let inches = check_size(points)?;
    format_run(ctx, "Set font size", |_, targets| {
        Ok(targets
            .iter()
            .map(|shape| character(*shape, "Size", num(inches)))
            .collect())
    })
}

/// The inches of a font size in points, or the error for a size the
/// renderer cannot use.
fn check_size(points: f64) -> Result<f64, CommandError> {
    if !points.is_finite() || points <= 0.0 || points > MAX_SIZE_PT * 4.0 {
        return Err(CommandError::invalid_args(
            "font size must be a positive number of points",
        ));
    }
    Ok(points_to_inches(points))
}

/// `text.font.size.increase` / `format.text.increase_size`: the next
/// size up the ladder.
fn run_font_grow(ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
    step_font_size(ctx, true)
}

/// `text.font.size.decrease` / `format.text.decrease_size`: the next
/// size down the ladder.
fn run_font_shrink(ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
    step_font_size(ctx, false)
}

/// Moves every run's size one step up or down the ladder. The step is
/// taken from the first target's current size (a uniform result over a
/// mixed selection).
fn step_font_size(ctx: &mut AppCtx, grow: bool) -> Result<Outcome, CommandError> {
    let (session, targets, _warnings) = selection_targets(ctx)?;
    let doc = ctx
        .documents
        .iter()
        .find(|slot| slot.id == session)
        .map(|slot| &slot.document)
        .ok_or_else(|| CommandError::failed("the document session is gone"))?;
    let current = targets
        .first()
        .and_then(|shape| read_row_number(doc, *shape, "Character", "Size"))
        .map_or(DEFAULT_SIZE_PT, inches_to_points);
    let fresh = if grow {
        next_size_up(current)
    } else {
        next_size_down(current)
    };
    let inches = points_to_inches(fresh);
    let verb = if grow { "Grow font" } else { "Shrink font" };
    format_run(ctx, verb, |_, targets| {
        Ok(targets
            .iter()
            .map(|shape| character(*shape, "Size", num(inches)))
            .collect())
    })
}

/// The default text size of a shape that carries none (points).
const DEFAULT_SIZE_PT: f64 = 12.0;

/// The next size above `points` on the ladder. A size at or past the
/// top of the ladder never grows.
fn next_size_up(points: f64) -> f64 {
    SIZE_STEPS_PT
        .iter()
        .copied()
        .find(|step| *step > points)
        .unwrap_or(points)
}

/// The next size below `points` on the ladder (floored at
/// [`MIN_SIZE_PT`]; a size at the floor never grows).
fn next_size_down(points: f64) -> f64 {
    if points <= MIN_SIZE_PT {
        return points;
    }
    for step in SIZE_STEPS_PT.iter().rev() {
        if *step < points {
            return *step;
        }
    }
    (points * 0.9).max(MIN_SIZE_PT)
}

/// `format.text.bold`: toggles `Style` bit 1 on every run.
fn run_bold(ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
    toggle_style(ctx, 1, "Bold")
}

/// `format.text.italic`: toggles `Style` bit 2.
fn run_italic(ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
    toggle_style(ctx, 2, "Italic")
}

/// `format.text.underline`: toggles `Style` bit 4.
fn run_underline(ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
    toggle_style(ctx, 4, "Underline")
}

/// Flips one `Style` bit of every run (the flip is taken from the first
/// target).
fn toggle_style(ctx: &mut AppCtx, bit: i64, label: &str) -> Result<Outcome, CommandError> {
    let (session, targets, _warnings) = selection_targets(ctx)?;
    let doc = ctx
        .documents
        .iter()
        .find(|slot| slot.id == session)
        .map(|slot| &slot.document)
        .ok_or_else(|| CommandError::failed("the document session is gone"))?;
    let style = targets
        .first()
        .and_then(|shape| read_row_number(doc, *shape, "Character", "Style"))
        .unwrap_or(0.0)
        .round();
    let fresh = if (style as i64) & bit != 0 {
        style - bit as f64
    } else {
        style + bit as f64
    };
    format_run(ctx, label, |_, targets| {
        Ok(targets
            .iter()
            .map(|shape| character(*shape, "Style", num(fresh)))
            .collect())
    })
}

/// `tools.strikethrough`: toggles the `Strikethru` cell of every run.
fn run_strikethrough(ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
    let (session, targets, _warnings) = selection_targets(ctx)?;
    let doc = ctx
        .documents
        .iter()
        .find(|slot| slot.id == session)
        .map(|slot| &slot.document)
        .ok_or_else(|| CommandError::failed("the document session is gone"))?;
    let on = targets
        .first()
        .and_then(|shape| read_row_boolean(doc, *shape, "Character", "Strikethru"))
        .unwrap_or(false);
    format_run(ctx, "Strikethrough", |_, targets| {
        Ok(targets
            .iter()
            .map(|shape| character(*shape, "Strikethru", bool_write(!on)))
            .collect())
    })
}

/// `text.font.color`: the colour of every run.
fn run_font_color(ctx: &mut AppCtx, inv: &Invocation) -> Result<Outcome, CommandError> {
    let color = need_color(inv, &["color", "value"])?;
    format_run(
        ctx,
        format!("Set font color {}", color_name(color)),
        |_, targets| {
            Ok(targets
                .iter()
                .map(|shape| character(*shape, "Color", color_write(color)))
                .collect())
        },
    )
}

/// `format.text_color.use_default`: every run inherits its colour
/// again.
fn run_font_color_default(ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
    format_run(ctx, "Use default font color", |_, targets| {
        Ok(targets
            .iter()
            .map(|shape| remove(*shape, Place::Character, "Color"))
            .collect())
    })
}

/// `format.text.align_left`: `HorzAlign` 0.
fn run_align_left(ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
    set_horz_align(ctx, 0.0, "Align text left")
}

/// `format.text.align_center`: `HorzAlign` 1.
fn run_align_center(ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
    set_horz_align(ctx, 1.0, "Center text")
}

/// `format.text.align_right`: `HorzAlign` 2.
fn run_align_right(ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
    set_horz_align(ctx, 2.0, "Align text right")
}

/// `format.text.align_justify`: `HorzAlign` 3.
fn run_align_justify(ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
    set_horz_align(ctx, 3.0, "Justify text")
}

/// Writes `HorzAlign` on every paragraph of the selection.
fn set_horz_align(ctx: &mut AppCtx, value: f64, label: &str) -> Result<Outcome, CommandError> {
    format_run(ctx, label, |_, targets| {
        Ok(targets
            .iter()
            .map(|shape| paragraph(*shape, "HorzAlign", num(value)))
            .collect())
    })
}

/// `format.text.align_top`: `VerticalAlign` 0.
fn run_align_top(ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
    set_vert_align(ctx, 0.0, "Align text top")
}

/// `format.text.align_middle`: `VerticalAlign` 1.
fn run_align_middle(ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
    set_vert_align(ctx, 1.0, "Align text middle")
}

/// `format.text.align_bottom`: `VerticalAlign` 2.
fn run_align_bottom(ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
    set_vert_align(ctx, 2.0, "Align text bottom")
}

/// Writes `VerticalAlign` on every selected shape.
fn set_vert_align(ctx: &mut AppCtx, value: f64, label: &str) -> Result<Outcome, CommandError> {
    format_run(ctx, label, |_, targets| {
        Ok(targets
            .iter()
            .map(|shape| direct(*shape, "VerticalAlign", num(value)))
            .collect())
    })
}

/// `text.bullets`: turns the bullet of every paragraph on (style 1) or
/// off.
fn run_bullets(ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
    let (session, targets, _warnings) = selection_targets(ctx)?;
    let doc = ctx
        .documents
        .iter()
        .find(|slot| slot.id == session)
        .map(|slot| &slot.document)
        .ok_or_else(|| CommandError::failed("the document session is gone"))?;
    let on = targets
        .first()
        .and_then(|shape| read_row_number(doc, *shape, "Paragraph", "Bullet"))
        .is_some_and(|value| value.round() >= 0.5);
    let fresh = if on { 0.0 } else { 1.0 };
    format_run(ctx, "Bullets", |_, targets| {
        Ok(targets
            .iter()
            .map(|shape| paragraph(*shape, "Bullet", num(fresh)))
            .collect())
    })
}

// ---------------------------------------------------------------------------
// Fill and line commands
// ---------------------------------------------------------------------------

/// `format.set.fill.color`: a solid fill of `color`.
fn run_fill_color(ctx: &mut AppCtx, inv: &Invocation) -> Result<Outcome, CommandError> {
    let color = need_color(inv, &["color", "value"])?;
    format_run(
        ctx,
        format!("Set fill color {}", color_name(color)),
        |_, targets| {
            let mut ops = Vec::new();
            for shape in targets {
                ops.push(direct(*shape, "FillForegnd", color_write(color)));
                ops.push(direct(*shape, "FillPattern", num(1.0)));
            }
            Ok(ops)
        },
    )
}

/// `format.fill_color.no_fill`: `FillPattern` 0.
fn run_fill_no_fill(ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
    format_run(ctx, "No fill", |_, targets| {
        Ok(targets
            .iter()
            .map(|shape| direct(*shape, "FillPattern", num(0.0)))
            .collect())
    })
}

/// `format.fill_color.use_default`: the fill cells inherit again.
fn run_fill_default(ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
    format_run(ctx, "Use default fill", |_, targets| {
        let mut ops = Vec::new();
        for shape in targets {
            ops.push(remove(*shape, Place::Direct, "FillForegnd"));
            ops.push(remove(*shape, Place::Direct, "FillPattern"));
        }
        Ok(ops)
    })
}

/// `shape.transparency` (and `shape.transparency.dlg`): the fill
/// transparency, in percent.
fn run_transparency(ctx: &mut AppCtx, inv: &Invocation) -> Result<Outcome, CommandError> {
    let percent = need_number(inv, &["transparency", "value"], "transparency (percent)")?;
    if !(0.0..=100.0).contains(&percent) {
        return Err(CommandError::invalid_args(
            "transparency must be between 0 and 100 percent",
        ));
    }
    let fraction = percent / 100.0;
    format_run(ctx, "Set transparency", |_, targets| {
        Ok(targets
            .iter()
            .map(|shape| direct(*shape, "FillForegndTrans", num(fraction)))
            .collect())
    })
}

/// `format.transparency.no_transparency`: `FillForegndTrans` 0.
fn run_no_transparency(ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
    format_run(ctx, "No transparency", |_, targets| {
        Ok(targets
            .iter()
            .map(|shape| direct(*shape, "FillForegndTrans", num(0.0)))
            .collect())
    })
}

/// `format.set.line.color`: the stroke colour.
fn run_line_color(ctx: &mut AppCtx, inv: &Invocation) -> Result<Outcome, CommandError> {
    let color = need_color(inv, &["color", "value"])?;
    format_run(
        ctx,
        format!("Set line color {}", color_name(color)),
        |_, targets| {
            Ok(targets
                .iter()
                .map(|shape| direct(*shape, "LineColor", color_write(color)))
                .collect())
        },
    )
}

/// `format.line_color.use_default`: the stroke colour inherits again.
fn run_line_default(ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
    format_run(ctx, "Use default line color", |_, targets| {
        Ok(targets
            .iter()
            .map(|shape| remove(*shape, Place::Direct, "LineColor"))
            .collect())
    })
}

/// `format.line_color.no_line` / `format.line_pattern.no_line`:
/// `LinePattern` 0.
fn run_line_no_line(ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
    format_run(ctx, "No line", |_, targets| {
        Ok(targets
            .iter()
            .map(|shape| direct(*shape, "LinePattern", num(0.0)))
            .collect())
    })
}

/// `format.set.line.weight`: the stroke weight, in points.
fn run_line_weight(ctx: &mut AppCtx, inv: &Invocation) -> Result<Outcome, CommandError> {
    let points = need_number(inv, &["weight", "value"], "line weight (points)")?;
    if !points.is_finite() || !(0.0..=100.0).contains(&points) {
        return Err(CommandError::invalid_args(
            "line weight must be between 0 and 100 points",
        ));
    }
    let inches = points_to_inches(points);
    format_run(ctx, "Set line weight", |_, targets| {
        Ok(targets
            .iter()
            .map(|shape| direct(*shape, "LineWeight", num(inches)))
            .collect())
    })
}

/// The line patterns: `(index, label)`, one row per pattern the
/// pickers list (1 is solid; the rest dash the stroke). This is the
/// one table the commands and the pickers share (the P7-27 review
/// A6): `format.set.line.pattern` parses these labels and
/// [`crate::builtins`] re-exports the table for the pickers to draw.
pub const LINE_PATTERNS: &[(f64, &str)] = &[
    (1.0, "Solid"),
    (2.0, "Dashed"),
    (3.0, "Dotted"),
    (4.0, "Dash-dot"),
    (5.0, "Dash-dot-dot"),
    (6.0, "Dot-dash-dash"),
    (7.0, "Long dash"),
    (8.0, "Long-short dash"),
];

/// The arrowheads: `(index, label)`, one row per arrowhead the pickers
/// list (0 is no arrowhead). The one table (the P7-27 review A6), as
/// [`LINE_PATTERNS`].
pub const LINE_ARROWS: &[(f64, &str)] = &[
    (0.0, "None"),
    (1.0, "Arrow"),
    (2.0, "Open"),
    (3.0, "Stealth"),
    (4.0, "Diamond"),
    (5.0, "Dot"),
];

/// The index of `key` (trimmed, any case) among the `labels` of one
/// label table.
fn label_index(key: &str, labels: &[(f64, &str)]) -> Option<f64> {
    labels
        .iter()
        .find(|(_, label)| label.eq_ignore_ascii_case(key))
        .map(|(index, _)| *index)
}

/// `format.set.line.pattern`: the stroke pattern index (1 is solid).
fn run_line_pattern(ctx: &mut AppCtx, inv: &Invocation) -> Result<Outcome, CommandError> {
    let index = pattern_index(inv)?;
    format_run(ctx, "Set line pattern", |_, targets| {
        Ok(targets
            .iter()
            .map(|shape| direct(*shape, "LinePattern", num(index)))
            .collect())
    })
}

/// The pattern index of the `value`/`pattern` argument: a number, the
/// name `none` (0), or one of the [`LINE_PATTERNS`] labels the pickers
/// show ("Dashed", "Dotted", ...), in any case.
fn pattern_index(inv: &Invocation) -> Result<f64, CommandError> {
    pattern_index_in(inv, "line pattern", &["pattern", "value"])
}

/// The pattern index under any of `names` (task P7-28: the Fill and
/// Line dialogs name it `pattern` alone), named `what` in the errors.
fn pattern_index_in(inv: &Invocation, what: &str, names: &[&str]) -> Result<f64, CommandError> {
    if let Some(text) = arg_text(inv, names) {
        let key = text.trim().to_ascii_lowercase();
        return match key.as_str() {
            "none" | "no line" | "noline" => Ok(0.0),
            _ => match label_index(&key, LINE_PATTERNS) {
                Some(index) => Ok(index),
                None => match key.parse::<f64>() {
                    Ok(value) if value.is_finite() && (0.0..=24.0).contains(&value) => Ok(value),
                    Ok(_) => Err(CommandError::invalid_args(format!(
                        "{what} must be between 0 and 24"
                    ))),
                    Err(_) => Err(CommandError::invalid_args(format!(
                        "{what} must be a number or a pattern name"
                    ))),
                },
            },
        };
    }
    Err(CommandError::NeedsInput(crate::NeededInput::Text {
        prompt: what.to_owned(),
        default: None,
    }))
}

/// `format.set.line.ends`: the arrowheads. `begin` and `end` set one
/// end each; `value` alone sets both.
fn run_line_ends(ctx: &mut AppCtx, inv: &Invocation) -> Result<Outcome, CommandError> {
    let both = arrow_index(inv, &["value"])?;
    let begin = arrow_index(inv, &["begin"])?;
    let end = arrow_index(inv, &["end"])?;
    let (begin, end) = match (both, begin, end) {
        (Some(value), None, None) => (Some(value), Some(value)),
        (_, begin, end) => (begin, end),
    };
    let (begin, end) = match (begin, end) {
        (None, None) => {
            return Err(CommandError::NeedsInput(crate::NeededInput::Text {
                prompt: "arrowhead".to_owned(),
                default: None,
            }));
        }
        (begin, end) => (begin, end),
    };
    format_run(ctx, "Set line ends", move |_, targets| {
        let mut ops = Vec::new();
        for shape in targets {
            if let Some(value) = begin {
                ops.push(direct(*shape, "BeginArrow", num(value)));
            }
            if let Some(value) = end {
                ops.push(direct(*shape, "EndArrow", num(value)));
            }
        }
        Ok(ops)
    })
}

/// `format.line_ends.no_line_ends`: both arrowheads off.
fn run_line_no_ends(ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
    format_run(ctx, "No line ends", |_, targets| {
        let mut ops = Vec::new();
        for shape in targets {
            ops.push(direct(*shape, "BeginArrow", num(0.0)));
            ops.push(direct(*shape, "EndArrow", num(0.0)));
        }
        Ok(ops)
    })
}

/// The arrowhead index of the `value`/`begin`/`end` argument: a
/// number, the name `none` (0), or one of the [`LINE_ARROWS`] labels
/// the pickers show ("Arrow", "Open", "Stealth", "Diamond", "Dot"), in
/// any case.
fn arrow_index(inv: &Invocation, names: &[&str]) -> Result<Option<f64>, CommandError> {
    if let Some(text) = arg_text(inv, names) {
        let key = text.trim().to_ascii_lowercase();
        return match key.as_str() {
            "none" | "no line ends" | "nolineends" => Ok(Some(0.0)),
            _ => match label_index(&key, LINE_ARROWS) {
                Some(index) => Ok(Some(index)),
                None => match key.parse::<f64>() {
                    Ok(value) if value.is_finite() && (0.0..=24.0).contains(&value) => {
                        Ok(Some(value))
                    }
                    Ok(_) => Err(CommandError::invalid_args(
                        "arrowhead must be between 0 and 24",
                    )),
                    Err(_) => Err(CommandError::invalid_args(
                        "arrowhead must be a number or a name",
                    )),
                },
            },
        };
    }
    Ok(None)
}

// ---------------------------------------------------------------------------
// The Fill and Line dialogs (task P7-28)
// ---------------------------------------------------------------------------

/// The fill cells `format.fill` writes (task P7-28): every field is
/// optional, and the dialog re-dispatches the command with all four at
/// once (one undo step). `transparency` is the fraction the
/// `FillForegndTrans` cell takes (0 to 1), parsed from the percent the
/// dialog and the command line show.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct FillSpec {
    /// `FillForegnd`, the fill colour.
    color: Option<larrygram_model::Color>,
    /// `FillPattern`, the pattern index.
    pattern: Option<f64>,
    /// `FillBkgnd`, the pattern colour.
    background: Option<larrygram_model::Color>,
    /// `FillForegndTrans`, the transparency fraction.
    transparency: Option<f64>,
}

/// The line cells `format.line` writes (task P7-28), the Line dialog's
/// ten fields. `weight` is in points (the picker's unit) and `rounding`
/// in inches, as the dialog shows them; `transparency` is the fraction
/// `LineColorTrans` takes.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct LineSpec {
    /// `LineColor`, the stroke colour.
    color: Option<larrygram_model::Color>,
    /// `LineWeight`, the stroke weight in points.
    weight: Option<f64>,
    /// `LinePattern`, the dash index.
    pattern: Option<f64>,
    /// `LineCap`: 0 round, 1 square, 2 extended.
    cap: Option<f64>,
    /// `LineColorTrans`, the transparency fraction.
    transparency: Option<f64>,
    /// `BeginArrow`, the begin arrowhead index.
    begin: Option<f64>,
    /// `EndArrow`, the end arrowhead index.
    end: Option<f64>,
    /// `BeginArrowSize`, the begin arrowhead size class.
    begin_size: Option<f64>,
    /// `EndArrowSize`, the end arrowhead size class.
    end_size: Option<f64>,
    /// `Rounding`, the corner radius in inches.
    rounding: Option<f64>,
}

impl FillSpec {
    /// The cells the arguments name (`color`, `pattern`, `background`,
    /// `transparency`); every one is optional.
    fn parse(inv: &Invocation) -> Result<Self, CommandError> {
        let pattern = if has_arg(inv, &["pattern"]) {
            Some(pattern_index_in(inv, "fill pattern", &["pattern"])?)
        } else {
            None
        };
        Ok(Self {
            color: opt_color(inv, &["color"])?,
            pattern,
            background: opt_color(inv, &["background"])?,
            transparency: opt_transparency(inv, &["transparency"])?,
        })
    }
}

impl LineSpec {
    /// The cells the arguments name (`color`, `weight`, `pattern`,
    /// `cap`, `transparency`, `begin`, `end`, `begin_size`, `end_size`,
    /// `rounding`); every one is optional.
    fn parse(inv: &Invocation) -> Result<Self, CommandError> {
        let pattern = if has_arg(inv, &["pattern"]) {
            Some(pattern_index_in(inv, "line pattern", &["pattern"])?)
        } else {
            None
        };
        Ok(Self {
            color: opt_color(inv, &["color"])?,
            weight: opt_weight(inv)?,
            pattern,
            cap: opt_cap(inv)?,
            transparency: opt_transparency(inv, &["transparency"])?,
            begin: arrow_index(inv, &["begin"])?,
            end: arrow_index(inv, &["end"])?,
            begin_size: opt_arrow_size(inv, &["begin_size"])?,
            end_size: opt_arrow_size(inv, &["end_size"])?,
            rounding: opt_rounding(inv)?,
        })
    }
}

/// Whether any of `names` carries an argument.
fn has_arg(inv: &Invocation, names: &[&str]) -> bool {
    names.iter().any(|name| inv.args.get(name).is_some())
}

/// The optional colour under any of `names`.
fn opt_color(
    inv: &Invocation,
    names: &[&str],
) -> Result<Option<larrygram_model::Color>, CommandError> {
    let Some(text) = arg_text(inv, names) else {
        return Ok(None);
    };
    parse_color(&text).map(Some).ok_or_else(|| {
        CommandError::invalid_args(format!(
            "unknown color {text:?}; use #RGB, #RRGGBB or a color name"
        ))
    })
}

/// The optional transparency percentage under any of `names`, stored
/// as the fraction the `*Trans` cells take (0 to 1).
fn opt_transparency(inv: &Invocation, names: &[&str]) -> Result<Option<f64>, CommandError> {
    let Some(percent) = arg_number(inv, names)? else {
        return Ok(None);
    };
    if !(0.0..=100.0).contains(&percent) {
        return Err(CommandError::invalid_args(
            "transparency must be between 0 and 100 percent",
        ));
    }
    Ok(Some(percent / 100.0))
}

/// The optional weight under `weight` (points, the picker's unit).
fn opt_weight(inv: &Invocation) -> Result<Option<f64>, CommandError> {
    let Some(points) = arg_number(inv, &["weight"])? else {
        return Ok(None);
    };
    if !(0.0..=100.0).contains(&points) {
        return Err(CommandError::invalid_args(
            "line weight must be between 0 and 100 points",
        ));
    }
    Ok(Some(points))
}

/// The optional line cap under `cap`: 0 round, 1 square, 2 extended
/// (the `LineCap` list), or one of those names.
fn opt_cap(inv: &Invocation) -> Result<Option<f64>, CommandError> {
    let Some(text) = arg_text(inv, &["cap"]) else {
        return Ok(None);
    };
    let key = text.trim().to_ascii_lowercase();
    let value = match key.as_str() {
        "round" | "rounded" => 0.0,
        "square" => 1.0,
        "extended" | "butt" => 2.0,
        _ => match key.parse::<f64>() {
            Ok(value) if value.is_finite() && (0.0..=2.0).contains(&value) => value,
            Ok(_) => {
                return Err(CommandError::invalid_args(
                    "cap must be 0, 1 or 2 (round, square, extended)",
                ));
            }
            Err(_) => {
                return Err(CommandError::invalid_args(
                    "cap must be round, square, extended or a number",
                ));
            }
        },
    };
    Ok(Some(value))
}

/// The optional arrowhead size class under any of `names` (the
/// `*ArrowSize` list: 0 very small .. 6 colossal).
fn opt_arrow_size(inv: &Invocation, names: &[&str]) -> Result<Option<f64>, CommandError> {
    let Some(text) = arg_text(inv, names) else {
        return Ok(None);
    };
    let key = text.trim().to_ascii_lowercase();
    let value = match key.as_str() {
        "very small" => 0.0,
        "small" => 1.0,
        "medium" => 2.0,
        "large" => 3.0,
        "extra large" | "very large" => 4.0,
        "jumbo" => 5.0,
        "colossal" => 6.0,
        _ => match key.parse::<f64>() {
            Ok(value) if value.is_finite() && (0.0..=6.0).contains(&value) => value,
            Ok(_) => {
                return Err(CommandError::invalid_args(
                    "arrow size must be between 0 and 6",
                ));
            }
            Err(_) => {
                return Err(CommandError::invalid_args(
                    "arrow size must be a number or a size name",
                ));
            }
        },
    };
    Ok(Some(value))
}

/// The optional corner rounding under `rounding` (a non-negative
/// radius in inches).
fn opt_rounding(inv: &Invocation) -> Result<Option<f64>, CommandError> {
    let Some(value) = arg_number(inv, &["rounding"])? else {
        return Ok(None);
    };
    if !value.is_finite() || !(0.0..=1000.0).contains(&value) {
        return Err(CommandError::invalid_args(
            "rounding must be between 0 and 1000 inches",
        ));
    }
    Ok(Some(value))
}

/// `format.fill` (task P7-28): the fill cells of every selected shape
/// from the arguments — the Fill dialog re-dispatches this command with
/// its fields, and one run is one undo step. With no arguments the
/// command requests the dialog (`UiRequest::ShowFillDialog`) and
/// changes nothing (AGENTS.md section 6).
fn run_fill(ctx: &mut AppCtx, inv: &Invocation) -> Result<Outcome, CommandError> {
    let spec = FillSpec::parse(inv)?;
    if spec == FillSpec::default() {
        return Ok(Outcome {
            message: Some("Fill".to_owned()),
            ui_request: Some(UiRequest::ShowFillDialog),
            ..Outcome::default()
        });
    }
    format_run(ctx, "Fill", move |_, targets| {
        let mut ops = Vec::new();
        for shape in targets {
            if let Some(color) = spec.color {
                ops.push(direct(*shape, "FillForegnd", color_write(color)));
            }
            if let Some(pattern) = spec.pattern {
                ops.push(direct(*shape, "FillPattern", num(pattern)));
            }
            if let Some(background) = spec.background {
                ops.push(direct(*shape, "FillBkgnd", color_write(background)));
            }
            if let Some(transparency) = spec.transparency {
                ops.push(direct(*shape, "FillForegndTrans", num(transparency)));
            }
        }
        Ok(ops)
    })
}

/// `format.line` (task P7-28): the line cells of every selected shape
/// from the arguments — the Line dialog and the Format Shape pane
/// re-dispatch this command with their fields, and one run is one undo
/// step. With no arguments the command requests the dialog
/// (`UiRequest::ShowLineDialog`) and changes nothing.
fn run_line(ctx: &mut AppCtx, inv: &Invocation) -> Result<Outcome, CommandError> {
    let spec = LineSpec::parse(inv)?;
    if spec == LineSpec::default() {
        return Ok(Outcome {
            message: Some("Line".to_owned()),
            ui_request: Some(UiRequest::ShowLineDialog),
            ..Outcome::default()
        });
    }
    format_run(ctx, "Line", move |_, targets| {
        let mut ops = Vec::new();
        for shape in targets {
            if let Some(color) = spec.color {
                ops.push(direct(*shape, "LineColor", color_write(color)));
            }
            if let Some(points) = spec.weight {
                ops.push(direct(*shape, "LineWeight", num(points_to_inches(points))));
            }
            if let Some(pattern) = spec.pattern {
                ops.push(direct(*shape, "LinePattern", num(pattern)));
            }
            if let Some(cap) = spec.cap {
                ops.push(direct(*shape, "LineCap", num(cap)));
            }
            if let Some(transparency) = spec.transparency {
                ops.push(direct(*shape, "LineColorTrans", num(transparency)));
            }
            if let Some(begin) = spec.begin {
                ops.push(direct(*shape, "BeginArrow", num(begin)));
            }
            if let Some(end) = spec.end {
                ops.push(direct(*shape, "EndArrow", num(end)));
            }
            if let Some(size) = spec.begin_size {
                ops.push(direct(*shape, "BeginArrowSize", num(size)));
            }
            if let Some(size) = spec.end_size {
                ops.push(direct(*shape, "EndArrowSize", num(size)));
            }
            if let Some(rounding) = spec.rounding {
                ops.push(direct(*shape, "Rounding", num(rounding)));
            }
        }
        Ok(ops)
    })
}

/// `format.shape.pane` (task P7-28): requests the Format Shape pane
/// (`UiRequest::ShowFormatShape`) and changes nothing; the request
/// toggles the pane (a second run closes it). The pane edits the same
/// cells field by field, through `format.fill` and `format.line`.
fn run_shape_pane(_ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
    Ok(Outcome {
        message: Some("Format Shape".to_owned()),
        ui_request: Some(UiRequest::ShowFormatShape),
        ..Outcome::default()
    })
}

// ---------------------------------------------------------------------------
// Format painter
// ---------------------------------------------------------------------------

/// `tools.format_painter`: with no recorded copy (or a `copy` flag) it
/// copies the anchor shape's format; with one (or an `apply` flag) it
/// applies the recorded format to the selection and clears; a `cancel`
/// flag drops a recorded copy without applying (task P7-24 item 6).
fn run_format_painter(ctx: &mut AppCtx, inv: &Invocation) -> Result<Outcome, CommandError> {
    let force_apply = inv.args.flag("apply").unwrap_or(false);
    let force_copy = inv.args.flag("copy").unwrap_or(false);
    if inv.args.flag("cancel").unwrap_or(false) {
        // Escape on the canvas (task P7-24 item 6): drop the copy.
        ctx.format_painter = None;
        return Ok(outcome("Format Painter cancelled", Vec::new()));
    }
    let recorded = if force_copy {
        None
    } else {
        ctx.format_painter.take()
    };
    if let Some(crate::app::PaintSource {
        session,
        shape: source,
    }) = recorded
    {
        let (target_session, targets, warnings) = selection_targets(ctx)?;
        if session != target_session {
            return Err(CommandError::failed(
                "the recorded format belongs to another document",
            ));
        }
        let ops = {
            let doc = ctx
                .documents
                .iter()
                .find(|slot| slot.id == session)
                .map(|slot| &slot.document)
                .ok_or_else(|| CommandError::failed("the document session is gone"))?;
            let template = capture_format(doc, source)?;
            let mut ops = Vec::new();
            for shape in &targets {
                ops.extend(retarget(&template, *shape));
            }
            ops
        };
        let message = count_message("Apply format", targets.len());
        let mut edit = {
            let doc = document_mut(ctx, session)?;
            FormatEdit::new(doc, session, message.clone(), ops)?
        };
        edit.apply(ctx)?;
        return Ok(edit_outcome(message, warnings, Box::new(edit)));
    }
    if force_apply {
        return Err(CommandError::failed("no format is recorded"));
    }
    let (session, targets, _warnings) = selection_targets(ctx)?;
    let Some(source) = targets.first().copied() else {
        return Err(CommandError::failed("no selected shape to copy"));
    };
    ctx.format_painter = Some(crate::app::PaintSource {
        session,
        shape: source,
    });
    Ok(outcome(
        "Format copied; run Format Painter again to apply it",
        Vec::new(),
    ))
}

/// The cells one format copy carries: the place, the cell and the
/// effective value of the source shape.
type Template = Vec<(Place, &'static str, CellWrite)>;

/// Reads the standard format cells of `shape` (Character, Paragraph,
/// fill and line).
fn capture_format(doc: &Document, shape: ShapeId) -> Result<Template, CommandError> {
    if doc.shape(shape).is_none() {
        return Err(CommandError::failed("the format painter's source is gone"));
    }
    let mut out: Template = Vec::new();
    if let Some(family) = read_row_text(doc, shape, "Character", "Font") {
        out.push((Place::Character, "Font", text_write(&family)));
    }
    if let Some(size) = read_row_number(doc, shape, "Character", "Size") {
        out.push((Place::Character, "Size", num(size)));
    }
    if let Some(style) = read_row_number(doc, shape, "Character", "Style") {
        out.push((Place::Character, "Style", num(style)));
    }
    let strike = read_row_boolean(doc, shape, "Character", "Strikethru").unwrap_or(false);
    out.push((Place::Character, "Strikethru", bool_write(strike)));
    if let Some(color) = read_row_color(doc, shape, "Character", "Color") {
        out.push((Place::Character, "Color", color_write(color)));
    }
    if let Some(align) = read_row_number(doc, shape, "Paragraph", "HorzAlign") {
        out.push((Place::Paragraph, "HorzAlign", num(align)));
    }
    if let Some(bullet) = read_row_number(doc, shape, "Paragraph", "Bullet") {
        out.push((Place::Paragraph, "Bullet", num(bullet)));
    }
    if let Some(align) = read_number(doc, shape, "VerticalAlign") {
        out.push((Place::Direct, "VerticalAlign", num(align)));
    }
    if let Some(pattern) = read_number(doc, shape, "FillPattern") {
        out.push((Place::Direct, "FillPattern", num(pattern)));
    }
    if let Some(color) = read_color(doc, shape, "FillForegnd") {
        out.push((Place::Direct, "FillForegnd", color_write(color)));
    }
    if let Some(trans) = read_number(doc, shape, "FillForegndTrans") {
        out.push((Place::Direct, "FillForegndTrans", num(trans)));
    }
    if let Some(color) = read_color(doc, shape, "LineColor") {
        out.push((Place::Direct, "LineColor", color_write(color)));
    }
    if let Some(weight) = read_number(doc, shape, "LineWeight") {
        out.push((Place::Direct, "LineWeight", num(weight)));
    }
    if let Some(pattern) = read_number(doc, shape, "LinePattern") {
        out.push((Place::Direct, "LinePattern", num(pattern)));
    }
    if let Some(arrow) = read_number(doc, shape, "BeginArrow") {
        out.push((Place::Direct, "BeginArrow", num(arrow)));
    }
    if let Some(arrow) = read_number(doc, shape, "EndArrow") {
        out.push((Place::Direct, "EndArrow", num(arrow)));
    }
    Ok(out)
}

/// The template's writes aimed at `shape`.
fn retarget(template: &Template, shape: ShapeId) -> Vec<Op> {
    template
        .iter()
        .map(|(place, cell, write)| Op {
            shape,
            place: *place,
            cell,
            write: Some(write.clone()),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{DocumentSession, Settings};
    use crate::{ArgValue, Args, Source};
    use larrygram_model::{Cell, PageId, Preserved, Shape};

    /// A document with one page and one shape, and the selected id.
    fn session() -> (AppCtx, SessionId, ShapeId) {
        let mut doc = Document::new();
        let page = doc.add_page("Page-1").expect("page");
        let mut shape = Shape::new(ShapeId { index: 0 }, 1);
        shape.sheet.cells.push(Cell {
            name: CellName::new("PinX"),
            formula: Some("1".to_owned()),
            value: Some(CellValue::Number(1.0)),
            unit: larrygram_geom::Unit::Inches,
            preserved: Preserved::new(),
        });
        let shape = doc.insert_shape(page, shape, None).expect("shape");
        let mut ctx = AppCtx::new(Settings::default());
        let index = ctx.open_document(DocumentSession::new(doc, None));
        let session = ctx.documents.get(index).expect("session").id;
        ctx.selection.shapes = vec![shape];
        ctx.selection.anchor = Some(shape);
        (ctx, session, shape)
    }

    fn call(ctx: &mut AppCtx, id: &str, args: Args) -> Result<Outcome, CommandError> {
        let inv = Invocation::new(args, Source::Test);
        match id {
            "text.font" => run_font(ctx, &inv),
            "text.font.size" => run_font_size(ctx, &inv),
            "format.text.bold" => run_bold(ctx, &inv),
            "tools.strikethrough" => run_strikethrough(ctx, &inv),
            "text.font.color" => run_font_color(ctx, &inv),
            "format.text.align_left" => run_align_left(ctx, &inv),
            "format.text.align_top" => run_align_top(ctx, &inv),
            "text.bullets" => run_bullets(ctx, &inv),
            "format.set.fill.color" => run_fill_color(ctx, &inv),
            "format.fill_color.no_fill" => run_fill_no_fill(ctx, &inv),
            "shape.transparency" => run_transparency(ctx, &inv),
            "format.set.line.color" => run_line_color(ctx, &inv),
            "format.set.line.weight" => run_line_weight(ctx, &inv),
            "format.set.line.pattern" => run_line_pattern(ctx, &inv),
            "format.set.line.ends" => run_line_ends(ctx, &inv),
            "format.line_ends.no_line_ends" => run_line_no_ends(ctx, &inv),
            "format.fill" => run_fill(ctx, &inv),
            "format.line" => run_line(ctx, &inv),
            "format.shape.pane" => run_shape_pane(ctx, &inv),
            "tools.format_painter" => run_format_painter(ctx, &inv),
            other => panic!("unknown test id {other}"),
        }
    }

    fn cell_of(doc: &Document, shape: ShapeId, name: &str) -> Option<Cell> {
        doc.shape(shape)
            .and_then(|entry| entry.sheet.cells.iter().find(|c| c.name.as_str() == name))
            .cloned()
    }

    fn row_cell_of(doc: &Document, shape: ShapeId, section: &str, name: &str) -> Option<Cell> {
        doc.shape(shape)
            .and_then(|entry| {
                entry
                    .sheet
                    .sections
                    .iter()
                    .find(|s| s.name.as_str() == section)
            })
            .and_then(|block| block.rows.first())
            .and_then(|row| row.cells.iter().find(|c| c.name.as_str() == name))
            .cloned()
    }

    #[test]
    fn font_family_writes_the_character_row() {
        let (mut ctx, _session, shape) = session();
        call(
            &mut ctx,
            "text.font",
            Args::new().with("value", ArgValue::Text("Selawik".to_owned())),
        )
        .expect("font");
        let session = ctx.active_document().expect("document");
        let cell = row_cell_of(&session.document, shape, "Character", "Font").expect("Font");
        assert_eq!(cell.value, Some(CellValue::Str("Selawik".to_owned())));
        assert_eq!(cell.formula.as_deref(), Some("\"Selawik\""));
    }

    #[test]
    fn font_size_is_stored_in_inches() {
        let (mut ctx, _session, shape) = session();
        call(
            &mut ctx,
            "text.font.size",
            Args::new().with("size", ArgValue::Number(12.0)),
        )
        .expect("size");
        let doc = &ctx.active_document().expect("document").document;
        let cell = row_cell_of(doc, shape, "Character", "Size").expect("Size");
        assert_eq!(cell.value, Some(CellValue::Number(1.0 / 6.0)));
    }

    #[test]
    fn font_size_rejects_a_bad_size() {
        let (mut ctx, _session, _shape) = session();
        let error = call(
            &mut ctx,
            "text.font.size",
            Args::new().with("size", ArgValue::Number(-1.0)),
        )
        .expect_err("negative");
        assert!(error.to_string().contains("font size"), "{error}");
        let error = call(&mut ctx, "text.font.size", Args::new()).expect_err("missing");
        assert!(error.to_string().contains("font size"), "{error}");
    }

    #[test]
    fn bold_toggles_the_style_bit() {
        let (mut ctx, _session, shape) = session();
        call(&mut ctx, "format.text.bold", Args::new()).expect("bold");
        {
            let doc = &ctx.active_document().expect("document").document;
            let cell = row_cell_of(doc, shape, "Character", "Style").expect("Style");
            assert_eq!(cell.value, Some(CellValue::Number(1.0)));
        }
        call(&mut ctx, "format.text.bold", Args::new()).expect("bold off");
        let doc = &ctx.active_document().expect("document").document;
        let cell = row_cell_of(doc, shape, "Character", "Style").expect("Style");
        assert_eq!(cell.value, Some(CellValue::Number(0.0)));
    }

    #[test]
    fn strikethrough_toggles_its_boolean_cell() {
        let (mut ctx, _session, shape) = session();
        call(&mut ctx, "tools.strikethrough", Args::new()).expect("strike");
        {
            let doc = &ctx.active_document().expect("document").document;
            let cell = row_cell_of(doc, shape, "Character", "Strikethru").expect("Strikethru");
            assert_eq!(cell.value, Some(CellValue::Bool(true)));
        }
        call(&mut ctx, "tools.strikethrough", Args::new()).expect("strike off");
        let doc = &ctx.active_document().expect("document").document;
        let cell = row_cell_of(doc, shape, "Character", "Strikethru").expect("Strikethru");
        assert_eq!(cell.value, Some(CellValue::Bool(false)));
    }

    #[test]
    fn colors_parse_hex_and_names_and_reject_other_text() {
        assert_eq!(
            parse_color("#1f5bc4"),
            Some(larrygram_model::Color::new(0x1f, 0x5b, 0xc4))
        );
        assert_eq!(
            parse_color("#F0C"),
            Some(larrygram_model::Color::new(0xFF, 0x00, 0xCC))
        );
        assert_eq!(
            parse_color("Blue"),
            Some(larrygram_model::Color::new(0x00, 0x00, 0xFF))
        );
        assert_eq!(parse_color("#12345"), None);
        assert_eq!(parse_color("#xyzxyz"), None);
        assert_eq!(parse_color("chartreuse-ish"), None);
    }

    #[test]
    fn font_color_writes_a_rgb_formula() {
        let (mut ctx, _session, shape) = session();
        call(
            &mut ctx,
            "text.font.color",
            Args::new().with("value", ArgValue::Text("#1F5BC4".to_owned())),
        )
        .expect("color");
        let doc = &ctx.active_document().expect("document").document;
        let cell = row_cell_of(doc, shape, "Character", "Color").expect("Color");
        assert_eq!(
            cell.value,
            Some(CellValue::Color(larrygram_model::Color::new(
                0x1f, 0x5b, 0xc4
            )))
        );
        assert_eq!(cell.formula.as_deref(), Some("RGB(31,91,196)"));
    }

    #[test]
    fn font_color_rejects_an_unknown_color() {
        let (mut ctx, _session, _shape) = session();
        let error = call(
            &mut ctx,
            "text.font.color",
            Args::new().with("value", ArgValue::Text("nope".to_owned())),
        )
        .expect_err("bad color");
        assert!(error.to_string().contains("unknown color"), "{error}");
    }

    #[test]
    fn alignment_writes_paragraph_and_text_block_cells() {
        let (mut ctx, _session, shape) = session();
        call(&mut ctx, "format.text.align_left", Args::new()).expect("left");
        call(&mut ctx, "format.text.align_top", Args::new()).expect("top");
        let doc = &ctx.active_document().expect("document").document;
        let horz = row_cell_of(doc, shape, "Paragraph", "HorzAlign").expect("HorzAlign");
        assert_eq!(horz.value, Some(CellValue::Number(0.0)));
        let vert = cell_of(doc, shape, "VerticalAlign").expect("VerticalAlign");
        assert_eq!(vert.value, Some(CellValue::Number(0.0)));
    }

    #[test]
    fn bullets_toggle_the_paragraph_bullet() {
        let (mut ctx, _session, shape) = session();
        call(&mut ctx, "text.bullets", Args::new()).expect("on");
        {
            let doc = &ctx.active_document().expect("document").document;
            let cell = row_cell_of(doc, shape, "Paragraph", "Bullet").expect("Bullet");
            assert_eq!(cell.value, Some(CellValue::Number(1.0)));
        }
        call(&mut ctx, "text.bullets", Args::new()).expect("off");
        let doc = &ctx.active_document().expect("document").document;
        let cell = row_cell_of(doc, shape, "Paragraph", "Bullet").expect("Bullet");
        assert_eq!(cell.value, Some(CellValue::Number(0.0)));
    }

    #[test]
    fn fill_and_line_cells_land_on_the_sheet() {
        let (mut ctx, _session, shape) = session();
        call(
            &mut ctx,
            "format.set.fill.color",
            Args::new().with("color", ArgValue::Text("red".to_owned())),
        )
        .expect("fill");
        call(
            &mut ctx,
            "shape.transparency",
            Args::new().with("transparency", ArgValue::Number(50.0)),
        )
        .expect("transparency");
        call(
            &mut ctx,
            "format.set.line.weight",
            Args::new().with("weight", ArgValue::Number(2.0)),
        )
        .expect("weight");
        call(
            &mut ctx,
            "format.set.line.pattern",
            Args::new().with("value", ArgValue::Integer(3)),
        )
        .expect("pattern");
        call(
            &mut ctx,
            "format.set.line.ends",
            Args::new().with("value", ArgValue::Text("none".to_owned())),
        )
        .expect("ends");
        let doc = &ctx.active_document().expect("document").document;
        assert_eq!(
            cell_of(doc, shape, "FillForegnd").and_then(|c| c.value),
            Some(CellValue::Color(larrygram_model::Color::new(255, 0, 0)))
        );
        assert_eq!(
            cell_of(doc, shape, "FillPattern").and_then(|c| c.value),
            Some(CellValue::Number(1.0))
        );
        assert_eq!(
            cell_of(doc, shape, "FillForegndTrans").and_then(|c| c.value),
            Some(CellValue::Number(0.5))
        );
        assert_eq!(
            cell_of(doc, shape, "LineWeight").and_then(|c| c.value),
            Some(CellValue::Number(2.0 / 72.0))
        );
        assert_eq!(
            cell_of(doc, shape, "LinePattern").and_then(|c| c.value),
            Some(CellValue::Number(3.0))
        );
        assert_eq!(
            cell_of(doc, shape, "BeginArrow").and_then(|c| c.value),
            Some(CellValue::Number(0.0))
        );
    }

    #[test]
    fn no_fill_and_no_line_write_their_pattern_cells() {
        let (mut ctx, _session, shape) = session();
        call(&mut ctx, "format.fill_color.no_fill", Args::new()).expect("no fill");
        call(&mut ctx, "format.line_ends.no_line_ends", Args::new()).expect("no ends");
        let doc = &ctx.active_document().expect("document").document;
        assert_eq!(
            cell_of(doc, shape, "FillPattern").and_then(|c| c.value),
            Some(CellValue::Number(0.0))
        );
        assert_eq!(
            cell_of(doc, shape, "EndArrow").and_then(|c| c.value),
            Some(CellValue::Number(0.0))
        );
    }

    #[test]
    fn transparency_checks_its_range_and_its_form() {
        let (mut ctx, _session, _shape) = session();
        let error = call(
            &mut ctx,
            "shape.transparency",
            Args::new().with("value", ArgValue::Number(120.0)),
        )
        .expect_err("range");
        assert!(error.to_string().contains("transparency"), "{error}");
        let error = call(&mut ctx, "shape.transparency", Args::new()).expect_err("missing");
        assert!(error.to_string().contains("transparency"), "{error}");
    }

    #[test]
    fn pattern_and_arrow_arguments_validate() {
        let (mut ctx, _session, _shape) = session();
        let error = call(
            &mut ctx,
            "format.set.line.pattern",
            Args::new().with("value", ArgValue::Text("squiggle".to_owned())),
        )
        .expect_err("bad pattern");
        assert!(error.to_string().contains("line pattern"), "{error}");
        let error = call(
            &mut ctx,
            "format.set.line.pattern",
            Args::new().with("value", ArgValue::Number(99.0)),
        )
        .expect_err("pattern range");
        assert!(error.to_string().contains("line pattern"), "{error}");
        let error = call(
            &mut ctx,
            "format.set.line.ends",
            Args::new().with("begin", ArgValue::Text("squiggle".to_owned())),
        )
        .expect_err("bad arrow");
        assert!(error.to_string().contains("arrowhead"), "{error}");
        let error = call(&mut ctx, "format.set.line.ends", Args::new()).expect_err("no arrow");
        assert!(error.to_string().contains("arrowhead"), "{error}");
    }

    #[test]
    fn pattern_and_arrow_labels_parse_like_the_picker_rows() {
        let (mut ctx, _session, shape) = session();
        call(
            &mut ctx,
            "format.set.line.pattern",
            Args::new().with("value", ArgValue::Text("Dashed".to_owned())),
        )
        .expect("pattern label");
        assert_eq!(
            cell_of(
                &ctx.active_document().expect("document").document,
                shape,
                "LinePattern"
            )
            .and_then(|cell| cell.value),
            Some(larrygram_model::CellValue::Number(2.0)),
            "the label spells the index"
        );
        call(
            &mut ctx,
            "format.set.line.ends",
            Args::new()
                .with("begin", ArgValue::Text("Stealth".to_owned()))
                .with("end", ArgValue::Text("Dot".to_owned())),
        )
        .expect("arrow labels");
        let doc = &ctx.active_document().expect("document").document;
        assert_eq!(
            cell_of(doc, shape, "BeginArrow").and_then(|cell| cell.value),
            Some(larrygram_model::CellValue::Number(3.0)),
            "stealth is 3"
        );
        assert_eq!(
            cell_of(doc, shape, "EndArrow").and_then(|cell| cell.value),
            Some(larrygram_model::CellValue::Number(5.0)),
            "dot is 5"
        );
    }

    #[test]
    fn the_labels_and_the_pt_suffix_parse_in_any_case() {
        // The one label table serves both the picker rows and the menu
        // `options` labels; the case of the argument is the user's (the
        // P7-27 review A6).
        let (mut ctx, _session, shape) = session();
        call(
            &mut ctx,
            "format.set.line.pattern",
            Args::new().with("value", ArgValue::Text("DASHED".to_owned())),
        )
        .expect("upper-case pattern label");
        call(
            &mut ctx,
            "format.set.line.ends",
            Args::new().with("value", ArgValue::Text("sTeAlTh".to_owned())),
        )
        .expect("mixed-case arrow label");
        call(
            &mut ctx,
            "format.set.line.weight",
            Args::new().with("value", ArgValue::Text("1.5 Pt".to_owned())),
        )
        .expect("mixed-case pt suffix");
        let doc = &ctx.active_document().expect("document").document;
        assert_eq!(
            cell_of(doc, shape, "LinePattern").and_then(|cell| cell.value),
            Some(larrygram_model::CellValue::Number(2.0)),
            "DASHED is the table's Dashed"
        );
        assert_eq!(
            cell_of(doc, shape, "BeginArrow").and_then(|cell| cell.value),
            Some(larrygram_model::CellValue::Number(3.0)),
            "sTeAlTh is the table's Stealth"
        );
        assert_eq!(
            cell_of(doc, shape, "LineWeight").and_then(|cell| cell.value),
            Some(larrygram_model::CellValue::Number(1.5 / 72.0)),
            "1.5 Pt parses like 1.5 pt"
        );
    }

    #[test]
    fn strip_pt_trims_any_case_and_nothing_else() {
        assert_eq!(strip_pt("0.25 pt"), "0.25");
        assert_eq!(strip_pt("0.25 PT"), "0.25");
        assert_eq!(strip_pt("0.25 Pt"), "0.25");
        assert_eq!(strip_pt("0.25pT"), "0.25");
        assert_eq!(strip_pt("  12  "), "12");
        assert_eq!(strip_pt("12%"), "12%");
        assert_eq!(strip_pt("pt"), "");
    }

    #[test]
    fn line_weight_accepts_a_points_spelling() {
        let (mut ctx, _session, shape) = session();
        call(
            &mut ctx,
            "format.set.line.weight",
            Args::new().with("value", ArgValue::Text("2.25 pt".to_owned())),
        )
        .expect("pt spelling");
        let doc = &ctx.active_document().expect("document").document;
        let value = cell_of(doc, shape, "LineWeight").and_then(|cell| cell.value);
        assert_eq!(value, Some(larrygram_model::CellValue::Number(2.25 / 72.0)));
    }

    #[test]
    fn line_weight_checks_its_range() {
        let (mut ctx, _session, _shape) = session();
        let error = call(
            &mut ctx,
            "format.set.line.weight",
            Args::new().with("weight", ArgValue::Number(-1.0)),
        )
        .expect_err("range");
        assert!(error.to_string().contains("line weight"), "{error}");
    }

    #[test]
    fn use_default_removes_the_local_entry() {
        let (mut ctx, _session, shape) = session();
        call(
            &mut ctx,
            "format.set.fill.color",
            Args::new().with("color", ArgValue::Text("red".to_owned())),
        )
        .expect("fill");
        let outcome = run_fill_default(&mut ctx, &Invocation::empty(Source::Test)).expect("reset");
        assert!(outcome.edit.is_some(), "the reset is undoable");
        let doc = &ctx.active_document().expect("document").document;
        assert!(cell_of(doc, shape, "FillForegnd").is_none(), "entry gone");
    }

    #[test]
    fn font_family_requires_a_name() {
        let (mut ctx, _session, _shape) = session();
        let error = call(
            &mut ctx,
            "text.font",
            Args::new().with("value", ArgValue::Text("  ".to_owned())),
        )
        .expect_err("empty");
        assert!(error.to_string().contains("font family"), "{error}");
        let error = call(&mut ctx, "text.font", Args::new()).expect_err("missing");
        assert!(error.to_string().contains("font family"), "{error}");
    }

    #[test]
    fn size_steps_walk_the_ladder_and_stay_inside_its_bounds() {
        assert_eq!(next_size_up(12.0), 14.0);
        assert_eq!(next_size_down(12.0), 11.0);
        assert_eq!(next_size_up(1000.0), 1000.0, "past the cap it stays");
        assert_eq!(next_size_down(1.5), 1.5 * 0.9);
        assert_eq!(next_size_down(0.5), 0.5, "at the floor it stays");
        assert_eq!(next_size_up(90.0), 96.0);
        assert_eq!(next_size_up(96.0), 96.0);
    }

    #[test]
    fn grow_and_shrink_write_the_stepped_size() {
        let (mut ctx, _session, shape) = session();
        run_font_size(
            &mut ctx,
            &Invocation::new(
                Args::new().with("size", ArgValue::Number(12.0)),
                Source::Test,
            ),
        )
        .expect("size 12");
        run_font_grow(&mut ctx, &Invocation::empty(Source::Test)).expect("grow");
        {
            let doc = &ctx.active_document().expect("document").document;
            let cell = row_cell_of(doc, shape, "Character", "Size").expect("Size");
            assert_eq!(cell.value, Some(CellValue::Number(14.0 / 72.0)));
        }
        run_font_shrink(&mut ctx, &Invocation::empty(Source::Test)).expect("shrink");
        let doc = &ctx.active_document().expect("document").document;
        let cell = row_cell_of(doc, shape, "Character", "Size").expect("Size");
        assert_eq!(cell.value, Some(CellValue::Number(12.0 / 72.0)));
    }

    #[test]
    fn the_format_painter_copies_then_applies_and_clears() {
        let (mut ctx, _session, shape) = session();
        call(
            &mut ctx,
            "format.set.fill.color",
            Args::new().with("color", ArgValue::Text("red".to_owned())),
        )
        .expect("fill on source");
        call(&mut ctx, "tools.format_painter", Args::new()).expect("copy");
        assert!(ctx.format_painter.is_some(), "the copy is recorded");

        // A second shape to paint onto.
        let session = ctx.active_document().expect("document").id;
        let other = {
            let doc = document_mut(&mut ctx, session).expect("document");
            let mut fresh = Shape::new(ShapeId { index: 9 }, 2);
            fresh.sheet.cells.push(Cell {
                name: CellName::new("PinX"),
                formula: Some("3".to_owned()),
                value: Some(CellValue::Number(3.0)),
                unit: larrygram_geom::Unit::Inches,
                preserved: Preserved::new(),
            });
            doc.insert_shape(PageId { index: 0 }, fresh, None)
                .expect("second shape")
        };
        ctx.selection.shapes = vec![other];
        call(&mut ctx, "tools.format_painter", Args::new()).expect("apply");
        assert!(ctx.format_painter.is_none(), "the painter clears");
        let doc = &ctx.active_document().expect("document").document;
        assert_eq!(
            cell_of(doc, other, "FillForegnd").and_then(|c| c.value),
            Some(CellValue::Color(larrygram_model::Color::new(255, 0, 0))),
            "the format reached the second shape"
        );
        assert_eq!(
            cell_of(doc, shape, "FillForegnd").and_then(|c| c.value),
            Some(CellValue::Color(larrygram_model::Color::new(255, 0, 0)))
        );
    }

    #[test]
    fn the_format_painter_needs_a_recorded_copy_to_apply() {
        let (mut ctx, _session, _shape) = session();
        let error = call(
            &mut ctx,
            "tools.format_painter",
            Args::new().with("apply", ArgValue::Flag(true)),
        )
        .expect_err("nothing recorded");
        assert!(error.to_string().contains("no format"), "{error}");
    }

    #[test]
    fn the_format_painter_cancel_drops_the_copy_without_applying() {
        let (mut ctx, _session, shape) = session();
        call(&mut ctx, "tools.format_painter", Args::new()).expect("copy");
        assert!(ctx.format_painter.is_some());
        let outcome = call(
            &mut ctx,
            "tools.format_painter",
            Args::new().with("cancel", ArgValue::Flag(true)),
        )
        .expect("cancel");
        assert!(ctx.format_painter.is_none(), "the copy is dropped");
        assert_eq!(
            outcome.message.as_deref(),
            Some("Format Painter cancelled"),
            "the cancel reports"
        );
        // The shape is untouched: no apply ran.
        let doc = &ctx.active_document().expect("document").document;
        assert_eq!(cell_of(doc, shape, "FillForegnd"), None, "nothing applied");
    }

    #[test]
    fn the_format_painter_reports_a_gone_source() {
        let (mut ctx, _session, _shape) = session();
        ctx.format_painter = Some(crate::app::PaintSource {
            session: ctx.active_document().expect("document").id,
            shape: ShapeId { index: 77 },
        });
        let error = call(&mut ctx, "tools.format_painter", Args::new()).expect_err("gone");
        assert!(error.to_string().contains("source is gone"), "{error}");
    }

    #[test]
    fn the_format_painter_refuses_another_document() {
        let (mut ctx, _session, _shape) = session();
        // A second document, closed again: its session id no longer
        // names a session, which the painter refuses as "another
        // document" when the selection moved there.
        let mut other = Document::new();
        other.add_page("Page-2").expect("page");
        let index = ctx.open_document(DocumentSession::new(other, None));
        let stale = ctx.documents.get(index).expect("session").id;
        ctx.close_document(index);
        ctx.format_painter = Some(crate::app::PaintSource {
            session: stale,
            shape: ShapeId { index: 0 },
        });
        let error = call(&mut ctx, "tools.format_painter", Args::new()).expect_err("other doc");
        assert!(error.to_string().contains("another document"), "{error}");
    }

    #[test]
    fn a_stale_target_is_an_error() {
        let (mut ctx, _session, _shape) = session();
        ctx.selection.shapes = vec![ShapeId { index: 99 }];
        let error = call(&mut ctx, "format.text.bold", Args::new()).expect_err("stale");
        assert!(error.to_string().contains("no selected shape"), "{error}");
    }

    #[test]
    fn enabled_predicates_name_their_reasons() {
        let ctx = AppCtx::new(Settings::default());
        assert_eq!(
            can_format(&ctx).reason.as_deref(),
            Some("no open document"),
            "reason"
        );
        let (mut ctx, _session, _shape) = session();
        assert!(can_format(&ctx).is_enabled());
        assert!(can_paint(&ctx).is_enabled());
        ctx.selection.shapes.clear();
        assert_eq!(can_format(&ctx).reason.as_deref(), Some("no selection"));
    }

    #[test]
    fn a_format_edit_applies_and_reverts() {
        let (mut ctx, session, shape) = session();
        let mut edit = {
            let doc = document_mut(&mut ctx, session).expect("document");
            FormatEdit::new(
                doc,
                session,
                "No fill",
                vec![direct(shape, "FillPattern", num(0.0))],
            )
            .expect("edit")
        };
        assert_eq!(edit.label(), "No fill");
        assert!(!edit.dirty_cells().is_empty(), "the write dirties its cell");
        edit.apply(&mut ctx).expect("apply");
        {
            let doc = &ctx.active_document().expect("document").document;
            assert!(cell_of(doc, shape, "FillPattern").is_some());
        }
        edit.revert(&mut ctx).expect("revert");
        let doc = &ctx.active_document().expect("document").document;
        assert!(
            cell_of(doc, shape, "FillPattern").is_none(),
            "the sheet is back"
        );
    }

    #[test]
    fn ensure_rows_creates_a_section_row_when_the_shape_has_none() {
        let (mut ctx, session, shape) = session();
        {
            let doc = document_mut(&mut ctx, session).expect("document");
            let targets = ensure_rows(doc, shape, "Character").expect("rows");
            assert_eq!(targets.len(), 1);
            assert_eq!(targets.first().map(|t| t.position), Some(Some(0)));
        }
        {
            let doc = document_mut(&mut ctx, session).expect("document");
            let targets = ensure_rows(doc, shape, "Character").expect("rows again");
            assert_eq!(targets.len(), 1, "no second row");
        }
        let error = ensure_rows(
            document_mut(&mut ctx, session).expect("document"),
            ShapeId { index: 99 },
            "Character",
        )
        .expect_err("stale");
        assert!(error.to_string().contains("unknown shape"), "{error}");
    }

    #[test]
    fn a_format_edit_reports_an_unknown_shape() {
        let (mut ctx, session, _shape) = session();
        let doc = document_mut(&mut ctx, session).expect("document");
        let ops = vec![direct(ShapeId { index: 42 }, "FillPattern", num(0.0))];
        let error = FormatEdit::new(doc, session, "No fill", ops).expect_err("stale");
        assert!(error.to_string().contains("unknown shape"), "{error}");
    }

    #[test]
    fn row_targets_cover_every_live_row() {
        let mut sheet = Sheet::new();
        let mut block = Section::new(SectionName::from_static("Character"));
        block.rows.push(Row::new());
        block.rows.push(Row::new());
        sheet.sections.push(block);
        let targets = row_targets(&sheet, "Character");
        assert_eq!(targets.len(), 2);
        let fresh = row_targets(&Sheet::new(), "Character");
        assert_eq!(fresh.len(), 1);
    }

    #[test]
    fn color_name_spells_upper_hex() {
        assert_eq!(
            color_name(larrygram_model::Color::new(0x1f, 0x5b, 0xc4)),
            "#1F5BC4"
        );
    }
    #[test]
    fn a_gone_shape_is_an_error_on_apply_and_on_revert() {
        // One edit per write branch: a direct removal, a row removal
        // and a row write.
        for kind in 0..3 {
            let (mut ctx, id, target) = session();
            let ops = match kind {
                0 => vec![remove(target, Place::Direct, "FillForegnd")],
                1 => vec![remove(target, Place::Character, "Font")],
                _ => vec![character(target, "Font", text_write("Selawik"))],
            };
            let mut edit = {
                let doc = document_mut(&mut ctx, id).expect("document");
                FormatEdit::new(doc, id, "gone", ops).expect("edit")
            };
            edit.apply(&mut ctx).expect("apply on the live shape");
            {
                let doc = document_mut(&mut ctx, id).expect("document");
                // The arena entry goes away (the documented invariant
                // says it cannot; the write path reports it anyway,
                // never a panic).
                doc.shapes.clear();
            }
            assert!(
                edit.apply(&mut ctx).is_err(),
                "apply reports the gone shape"
            );
            assert!(
                edit.revert(&mut ctx).is_err(),
                "revert reports the gone shape"
            );
        }
    }

    #[test]
    fn use_default_removes_the_row_cell_too() {
        let (mut ctx, _session, shape) = session();
        call(
            &mut ctx,
            "text.font.color",
            Args::new().with("value", ArgValue::Text("#123456".to_owned())),
        )
        .expect("color");
        let outcome =
            run_font_color_default(&mut ctx, &Invocation::empty(Source::Test)).expect("reset");
        assert!(outcome.edit.is_some(), "the reset is undoable");
        let doc = &ctx.active_document().expect("document").document;
        assert!(
            row_cell_of(doc, shape, "Character", "Color").is_none(),
            "the row's Color entry is gone"
        );
    }

    #[test]
    fn row_targets_name_the_row_to_create() {
        let mut sheet = Sheet::new();
        let mut block = Section::new(SectionName::from_static("Character"));
        let mut deleted = Row::new();
        deleted.ix = Some(0);
        deleted
            .preserved
            .attributes
            .push(larrygram_model::XmlAttribute {
                name: larrygram_model::QName::local("Del"),
                value: "1".to_owned(),
            });
        block.rows.push(deleted);
        sheet.sections.push(block);
        let targets = row_targets(&sheet, "Character");
        assert_eq!(targets.len(), 1, "only the created row is written");
        assert_eq!(targets[0].position, Some(0));
        assert_eq!(targets[0].ix, None);
    }

    #[test]
    fn ensure_rows_fills_an_empty_section_and_picks_the_next_ix() {
        // A section whose only row is deleted takes the next free IX.
        let (mut ctx, id, target) = session();
        {
            let doc = document_mut(&mut ctx, id).expect("document");
            let entry = doc.shape_mut(target).expect("shape");
            let mut block = Section::new(SectionName::from_static("Character"));
            let mut deleted = Row::new();
            deleted.ix = Some(0);
            deleted
                .preserved
                .attributes
                .push(larrygram_model::XmlAttribute {
                    name: larrygram_model::QName::local("Del"),
                    value: "1".to_owned(),
                });
            block.rows.push(deleted);
            entry.sheet.sections.push(block);
        }
        {
            let doc = document_mut(&mut ctx, id).expect("document");
            let targets = ensure_rows(doc, target, "Character").expect("rows");
            assert_eq!(targets.len(), 1);
            assert_eq!(targets[0].ix, Some(1), "IX 0 is taken by the deleted row");
        }
        // An empty section takes IX 0.
        let (mut ctx, id, target) = session();
        {
            let doc = document_mut(&mut ctx, id).expect("document");
            let entry = doc.shape_mut(target).expect("shape");
            entry
                .sheet
                .sections
                .push(Section::new(SectionName::from_static("Paragraph")));
        }
        let doc = document_mut(&mut ctx, id).expect("document");
        let targets = ensure_rows(doc, target, "Paragraph").expect("rows");
        assert_eq!(targets[0].ix, Some(0));
    }

    #[test]
    fn a_stale_selection_disables_the_format_commands() {
        let (mut ctx, _session, _shape) = session();
        ctx.selection.shapes = vec![ShapeId { index: 99 }];
        let disabled = can_format(&ctx);
        assert_eq!(
            disabled.reason.as_deref(),
            Some("the selected shapes are stale")
        );
    }

    #[test]
    fn the_text_argument_spellings_all_parse() {
        let (mut ctx, _session, shape) = session();
        // A flag spells its value as text.
        call(
            &mut ctx,
            "text.font",
            Args::new().with("font", ArgValue::Flag(true)),
        )
        .expect("flag");
        {
            let doc = &ctx.active_document().expect("document").document;
            let cell = row_cell_of(doc, shape, "Character", "Font").expect("Font");
            assert_eq!(cell.value, Some(CellValue::Str("true".to_owned())));
        }
        // Numbers, integers and text all parse for a size.
        for args in [
            Args::new().with("size", ArgValue::Integer(12)),
            Args::new().with("size", ArgValue::Text("14".to_owned())),
        ] {
            run_font_size(&mut ctx, &Invocation::new(args, Source::Test)).expect("size");
        }
        let doc = &ctx.active_document().expect("document").document;
        let cell = row_cell_of(doc, shape, "Character", "Size").expect("Size");
        assert_eq!(cell.value, Some(CellValue::Number(14.0 / 72.0)));
    }

    #[test]
    fn a_number_argument_rejects_bad_spellings() {
        let (mut ctx, _session, _shape) = session();
        for (args, want) in [
            (
                Args::new().with("size", ArgValue::Text("big".to_owned())),
                "must be a number",
            ),
            (
                Args::new().with("size", ArgValue::Text("nan".to_owned())),
                "must be finite",
            ),
            (
                Args::new().with("size", ArgValue::Number(f64::NAN)),
                "must be finite",
            ),
        ] {
            let error = run_font_size(&mut ctx, &Invocation::new(args, Source::Test))
                .expect_err("bad size");
            assert!(error.to_string().contains(want), "{error}");
        }
    }

    #[test]
    fn a_pattern_without_an_argument_asks_for_one() {
        let (mut ctx, _session, _shape) = session();
        let error =
            run_line_pattern(&mut ctx, &Invocation::empty(Source::Test)).expect_err("no pattern");
        assert!(error.to_string().contains("line pattern"), "{error}");
    }

    #[test]
    fn an_arrowhead_out_of_range_is_an_argument_error() {
        let (mut ctx, _session, _shape) = session();
        let error = run_line_ends(
            &mut ctx,
            &Invocation::new(
                Args::new().with("value", ArgValue::Number(99.0)),
                Source::Test,
            ),
        )
        .expect_err("range");
        assert!(error.to_string().contains("arrowhead"), "{error}");
    }

    #[test]
    fn the_format_painter_can_re_copy_on_command() {
        let (mut ctx, _session, _shape) = session();
        call(&mut ctx, "tools.format_painter", Args::new()).expect("copy");
        let args = Args::new().with("copy", ArgValue::Flag(true));
        call(&mut ctx, "tools.format_painter", args).expect("copy again");
        assert!(ctx.format_painter.is_some(), "the copy is recorded");
    }

    // --- Task P7-28: the Fill and Line dialogs and the pane ---

    #[test]
    fn fill_with_no_arguments_requests_the_dialog() {
        let (mut ctx, _session, shape) = session();
        let outcome = call(&mut ctx, "format.fill", Args::new()).expect("fill");
        assert_eq!(outcome.ui_request, Some(UiRequest::ShowFillDialog));
        assert!(outcome.edit.is_none(), "the request writes nothing");
        let doc = &ctx.active_document().expect("document").document;
        assert!(cell_of(doc, shape, "FillForegnd").is_none());
    }

    #[test]
    fn fill_writes_the_dialog_cells_in_one_undo_step() {
        let (mut ctx, _session, shape) = session();
        let args = Args::new()
            .with("color", ArgValue::Text("#1F5BC4".to_owned()))
            .with("pattern", ArgValue::Number(2.0))
            .with("background", ArgValue::Text("white".to_owned()))
            .with("transparency", ArgValue::Number(50.0));
        let outcome = call(&mut ctx, "format.fill", args).expect("fill");
        assert!(outcome.edit.is_some(), "one undo step");
        {
            let doc = &ctx.active_document().expect("document").document;
            let color = cell_of(doc, shape, "FillForegnd").expect("FillForegnd");
            assert_eq!(
                color.value,
                Some(CellValue::Color(larrygram_model::Color::new(
                    0x1f, 0x5b, 0xc4
                )))
            );
            assert_eq!(
                cell_of(doc, shape, "FillPattern")
                    .expect("FillPattern")
                    .value,
                Some(CellValue::Number(2.0))
            );
            let background = cell_of(doc, shape, "FillBkgnd").expect("FillBkgnd");
            assert_eq!(
                background.value,
                Some(CellValue::Color(larrygram_model::Color::new(
                    0xff, 0xff, 0xff
                )))
            );
            // The percent the dialog shows is the fraction the cell
            // stores (the `shape.transparency` convention).
            assert_eq!(
                cell_of(doc, shape, "FillForegndTrans")
                    .expect("FillForegndTrans")
                    .value,
                Some(CellValue::Number(0.5))
            );
        }
        // One undo restores every cell the run wrote.
        let mut edit = outcome.edit.expect("edit");
        edit.revert(&mut ctx).expect("revert");
        let doc = &ctx.active_document().expect("document").document;
        assert!(
            cell_of(doc, shape, "FillForegnd").is_none(),
            "undo restores"
        );
        assert!(cell_of(doc, shape, "FillPattern").is_none());
    }

    #[test]
    fn fill_rejects_a_bad_colour_and_a_transparency_out_of_range() {
        let (mut ctx, _session, _shape) = session();
        let error = call(
            &mut ctx,
            "format.fill",
            Args::new().with("color", ArgValue::Text("chartreuse-ish".to_owned())),
        )
        .expect_err("colour");
        assert!(error.to_string().contains("unknown color"), "{error}");
        let error = call(
            &mut ctx,
            "format.fill",
            Args::new().with("transparency", ArgValue::Number(101.0)),
        )
        .expect_err("range");
        assert!(error.to_string().contains("transparency"), "{error}");
        let error = call(
            &mut ctx,
            "format.fill",
            Args::new().with("pattern", ArgValue::Text("wavy".to_owned())),
        )
        .expect_err("pattern");
        assert!(error.to_string().contains("fill pattern"), "{error}");
    }

    #[test]
    fn line_with_no_arguments_requests_the_dialog() {
        let (mut ctx, _session, shape) = session();
        let outcome = call(&mut ctx, "format.line", Args::new()).expect("line");
        assert_eq!(outcome.ui_request, Some(UiRequest::ShowLineDialog));
        assert!(outcome.edit.is_none(), "the request writes nothing");
        let doc = &ctx.active_document().expect("document").document;
        assert!(cell_of(doc, shape, "LineColor").is_none());
    }

    #[test]
    fn line_writes_the_dialog_cells_in_one_undo_step() {
        let (mut ctx, _session, shape) = session();
        let args = Args::new()
            .with("color", ArgValue::Text("red".to_owned()))
            .with("weight", ArgValue::Number(1.5))
            .with("pattern", ArgValue::Text("dashed".to_owned()))
            .with("cap", ArgValue::Text("square".to_owned()))
            .with("transparency", ArgValue::Number(25.0))
            .with("begin", ArgValue::Text("stealth".to_owned()))
            .with("end", ArgValue::Number(4.0))
            .with("begin_size", ArgValue::Text("large".to_owned()))
            .with("end_size", ArgValue::Number(1.0))
            .with("rounding", ArgValue::Number(0.25));
        let outcome = call(&mut ctx, "format.line", args).expect("line");
        assert!(outcome.edit.is_some(), "one undo step");
        {
            let doc = &ctx.active_document().expect("document").document;
            assert_eq!(
                cell_of(doc, shape, "LineColor").expect("LineColor").value,
                Some(CellValue::Color(larrygram_model::Color::new(
                    0xff, 0x00, 0x00
                )))
            );
            // The weight is points on the wire and inches in the cell.
            assert_eq!(
                cell_of(doc, shape, "LineWeight").expect("LineWeight").value,
                Some(CellValue::Number(1.5 / 72.0))
            );
            assert_eq!(
                cell_of(doc, shape, "LinePattern")
                    .expect("LinePattern")
                    .value,
                Some(CellValue::Number(2.0))
            );
            assert_eq!(
                cell_of(doc, shape, "LineCap").expect("LineCap").value,
                Some(CellValue::Number(1.0))
            );
            assert_eq!(
                cell_of(doc, shape, "LineColorTrans")
                    .expect("LineColorTrans")
                    .value,
                Some(CellValue::Number(0.25))
            );
            assert_eq!(
                cell_of(doc, shape, "BeginArrow").expect("BeginArrow").value,
                Some(CellValue::Number(3.0))
            );
            assert_eq!(
                cell_of(doc, shape, "EndArrow").expect("EndArrow").value,
                Some(CellValue::Number(4.0))
            );
            assert_eq!(
                cell_of(doc, shape, "BeginArrowSize")
                    .expect("BeginArrowSize")
                    .value,
                Some(CellValue::Number(3.0))
            );
            assert_eq!(
                cell_of(doc, shape, "EndArrowSize")
                    .expect("EndArrowSize")
                    .value,
                Some(CellValue::Number(1.0))
            );
            assert_eq!(
                cell_of(doc, shape, "Rounding").expect("Rounding").value,
                Some(CellValue::Number(0.25))
            );
        }
        let mut edit = outcome.edit.expect("edit");
        edit.revert(&mut ctx).expect("revert");
        let doc = &ctx.active_document().expect("document").document;
        assert!(cell_of(doc, shape, "LineColor").is_none(), "undo restores");
        assert!(cell_of(doc, shape, "Rounding").is_none());
    }

    #[test]
    fn line_rejects_a_bad_cap_size_and_rounding() {
        let (mut ctx, _session, _shape) = session();
        for (name, value, mentions) in [
            ("cap", "bent", "cap"),
            ("end_size", "huge", "arrow size"),
            ("rounding", "-1", "rounding"),
            ("begin", "twisty", "arrowhead"),
        ] {
            let error = call(
                &mut ctx,
                "format.line",
                Args::new().with(name, ArgValue::Text(value.to_owned())),
            )
            .expect_err("a bad spelling is an argument error");
            assert!(error.to_string().contains(mentions), "{name}: {error}");
        }
    }

    #[test]
    fn one_apply_covers_the_whole_selection_in_one_undo_step() {
        let (mut ctx, _session, first) = session();
        let second = {
            let session = ctx.active_document_mut().expect("document");
            let page = session.active_page.expect("page");
            session
                .document
                .insert_shape(page, Shape::new(ShapeId { index: 0 }, 2), None)
                .expect("shape")
        };
        ctx.selection.shapes = vec![first, second];
        ctx.selection.anchor = Some(first);
        let outcome = call(
            &mut ctx,
            "format.fill",
            Args::new().with("pattern", ArgValue::Number(1.0)),
        )
        .expect("fill");
        {
            let doc = &ctx.active_document().expect("document").document;
            for shape in [first, second] {
                assert_eq!(
                    cell_of(doc, shape, "FillPattern")
                        .expect("FillPattern")
                        .value,
                    Some(CellValue::Number(1.0)),
                    "shape {} takes the write",
                    shape.index
                );
            }
        }
        assert!(outcome.edit.is_some(), "one step over the selection");
    }

    #[test]
    fn the_format_shape_pane_requests_the_pane() {
        let (mut ctx, _session, shape) = session();
        let outcome = call(&mut ctx, "format.shape.pane", Args::new()).expect("pane");
        assert_eq!(outcome.ui_request, Some(UiRequest::ShowFormatShape));
        assert!(outcome.edit.is_none(), "the pane writes nothing on its own");
        let doc = &ctx.active_document().expect("document").document;
        assert!(cell_of(doc, shape, "FillPattern").is_none());
    }

    #[test]
    fn fill_and_line_are_disabled_without_a_selection() {
        let (mut ctx, _session, _shape) = session();
        ctx.selection.shapes.clear();
        ctx.selection.anchor = None;
        let verdict = can_format(&ctx);
        assert!(!verdict.is_enabled());
        assert_eq!(verdict.reason.as_deref(), Some("no selection"));
    }

    #[test]
    #[should_panic(expected = "unknown test id")]
    fn the_test_helper_names_an_unknown_id() {
        let (mut ctx, _session, _shape) = session();
        let _ = call(&mut ctx, "nope", Args::new());
    }
}
