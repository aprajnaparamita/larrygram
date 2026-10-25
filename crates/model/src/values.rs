//! Cached-value reading with the Phase 3 default and warning policy
//! (ADR 0021).
//!
//! Phase 3 renders from cached values only: no formula evaluation. Every
//! cell the renderer reads goes through [`CellReader`], which resolves the
//! value through the inheritance lookup (ADR 0013) and, when the cached
//! value is missing, an error or of the wrong kind, substitutes the hard
//! default of the kind and records a [`CellWarning`]. A warning never fails
//! a render.
//!
//! Two corpus findings amend the policy (ADR 0021 amendments):
//!
//! - A cached value that is the literal `Themed` is a marker, not a string:
//!   the value comes from the theme. [`CellReader::color`] resolves it from
//!   the shape's `QuickStyle*` cells against `themes[0]`; anything the theme
//!   cannot answer (a transparency, a variant colour) substitutes the hard
//!   default with [`WarningKind::UnresolvedTheme`] and never a kind
//!   mismatch.
//! - A cached numeric colour is a palette index (0..23, [`palette`]) or a
//!   document colour table index (24 and up, `Document::color_table`).
//!
//! Booleans and lists are read the way the files write them: a boolean cell
//! caches `0`/`1` without a `U` code and a one-element list caches a plain
//! number, so [`CellReader::boolean`] accepts numbers and
//! [`CellReader::list`] accepts one number as a one-element list. Those are
//! coercions, not kind mismatches, and record no warning.
//!
//! A cached number that is not finite (`inf`, `nan`, `1e999`) is a kind
//! mismatch (ADR 0021 amendment, P3-16): one [`WarningKind::KindMismatch`]
//! warning and the hard default, never a silent `0`.

use crate::cell::{Cell, CellValue, Color};
use crate::defaults::{DocumentedDefault, documented_default};
use crate::document::Document;
use crate::error::Error;
use crate::id::{PageId, ShapeId};
use crate::lookup::{CellLookup, ValueSource};
use crate::name::CellName;

use larrygram_geom::Angle;

/// The sheet a cell is read from (ADR 0021).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SheetRef {
    /// A shape's sheet.
    Shape(ShapeId),
    /// A page sheet.
    Page(PageId),
}

/// One row of a repeated section, named for the `CellReader::row_*`
/// readers (ADR 0019 decision 5).
///
/// A row is matched by `IX` when both sides have one and by position
/// otherwise, in the sheet's own rows as in the master chain's. The text
/// run markers (`cp`/`pp`) name rows by `IX` alone
/// ([`RowTarget::by_ix`]); the row-count driven reads (gradient stops, tab
/// stops) name rows by position ([`RowTarget::at`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RowTarget {
    /// The row's position in the section, when the caller knows one.
    pub position: Option<usize>,
    /// The row's `IX`, when both sides have one.
    pub ix: Option<i32>,
}

impl RowTarget {
    /// The row at `position`; a row `IX` is taken from the sheet's own row
    /// when it has one.
    pub const fn at(position: usize) -> Self {
        Self {
            position: Some(position),
            ix: None,
        }
    }

    /// The row with `IX` `ix`, wherever it sits (a text run marker).
    pub const fn by_ix(ix: i32) -> Self {
        Self {
            position: None,
            ix: Some(ix),
        }
    }
}

/// The repeated-section identity of one [`RowTarget`]: the first section
/// with that name (ADR 0019 decision 5), the row as named.
fn repeated<'a>(section: &'a str, row: RowTarget) -> crate::lookup::RepeatedCell<'a> {
    crate::lookup::RepeatedCell {
        section,
        section_ix: None,
        section_pos: Some(0),
        row: row.position,
        row_ix: row.ix,
    }
}

/// The cached value of the sheet's own entry for a direct or
/// section-level cell (ADR 0054 amendment, ADR 0057): a cell whose own
/// entry has a cached value, an `Inh` snapshot included, returns that
/// value. A row cell needs the row identity its caller carries
/// ([`own_row_value`]); this name-only search never picks one row's
/// snapshot for another row.
fn own_value(doc: &Document, sheet: SheetRef, cell: &CellName) -> Option<CellValue> {
    let sheet = doc.sheet_of(sheet).ok()?;
    let found = sheet
        .cells
        .iter()
        .find(|entry| entry.name == *cell)
        .or_else(|| {
            sheet
                .sections
                .iter()
                .flat_map(|section| section.cells.iter())
                .find(|entry| entry.name == *cell)
        })?;
    found.value.clone()
}

/// The cached value of one row's own entry (ADR 0054 amendment, ADR
/// 0057): the snapshot of an `Inh` cell is this sheet's own value at save
/// time. The row is the one the lookup matched ([`crate::lookup::RepeatedCell`]).
pub(crate) fn own_row_value<'d>(
    doc: &'d Document,
    sheet: SheetRef,
    target: &crate::lookup::RepeatedCell<'_>,
    cell: &CellName,
) -> Option<&'d CellValue> {
    let sheet = doc.sheet_of(sheet).ok()?;
    let section = crate::lookup::match_repeated_section(
        sheet,
        target.section,
        target.section_ix,
        target.section_pos,
    )?;
    let row_cell = matched_row_cell(section, target.row, target.row_ix, cell)?;
    row_cell.value.as_ref()
}

/// The cell of the row that matches a row at `row` with `IX` `row_ix`, by
/// `IX` when both sides have one and by position otherwise (the row rule
/// of [`crate::lookup::RepeatedCell`], kept here so the snapshot rule
/// matches the lookup's own row).
fn matched_row_cell<'a>(
    section: &'a crate::section::Section,
    row: Option<usize>,
    row_ix: Option<i32>,
    cell: &CellName,
) -> Option<&'a Cell> {
    let matched = match (row, row_ix) {
        (None, None) => return section.cells.iter().find(|entry| entry.name == *cell),
        (None, Some(row_ix)) => section
            .rows
            .iter()
            .find(|candidate| candidate.ix == Some(row_ix)),
        (Some(row), None) => section.rows.get(row),
        (Some(row), Some(row_ix)) => match section
            .rows
            .iter()
            .find(|candidate| candidate.ix == Some(row_ix))
        {
            Some(found) => Some(found),
            None => section
                .rows
                .get(row)
                .filter(|candidate| candidate.ix.is_none()),
        },
    };
    matched?.cells.iter().find(|entry| entry.name == *cell)
}

/// Applies the ADR 0054/0057 snapshot rule to one row resolution: the
/// row's own cached value wins over the rung's, and provenance reads as
/// `Local` for the value policy only (the lookup's own `Resolved` is
/// unchanged).
pub(crate) fn prefer_row<'d>(
    doc: &'d Document,
    sheet: SheetRef,
    target: &crate::lookup::RepeatedCell<'_>,
    cell: &CellName,
    resolved: &mut crate::lookup::Resolved<'d>,
) {
    if let Some(value) = own_row_value(doc, sheet, target, cell) {
        resolved.value = Some(value);
        resolved.source = ValueSource::Local;
    }
}

/// Why [`CellReader`] substituted a default (ADR 0021).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum WarningKind {
    /// No source defines the value.
    Missing,
    /// The cached value is an error, e.g. `"#VALUE!"`.
    ErrorValue(String),
    /// The cached value has another kind than requested.
    KindMismatch,
    /// The cached value is the `Themed` marker and the theme cannot answer
    /// the request (ADR 0021 amendment).
    UnresolvedTheme,
}

/// One substituted cell value (ADR 0021).
#[derive(Debug, Clone, PartialEq)]
pub struct CellWarning {
    /// The sheet the cell was read from.
    pub sheet: SheetRef,
    /// The cell name.
    pub cell: CellName,
    /// Why the default was substituted.
    pub kind: WarningKind,
    /// The default that was used instead.
    pub substituted: CellValue,
}

/// The literal cached value of a cell whose value comes from the theme
/// (ADR 0021 amendment).
const THEMED: &str = "Themed";

/// Reads resolved cell values with the Phase 3 policy (ADR 0021).
///
/// Construct one reader per build (scene, layout) and collect its warnings
/// at the end. The reader borrows the document and changes nothing in it
/// (ADR 0013).
pub struct CellReader<'d> {
    doc: &'d Document,
    warnings: Vec<CellWarning>,
}

impl<'d> CellReader<'d> {
    /// A reader over `doc` with no warnings yet.
    pub fn new(doc: &'d Document) -> Self {
        Self {
            doc,
            warnings: Vec::new(),
        }
    }

    /// Reads a number in canonical units (inches, radians or fraction,
    /// ADR 0011).
    pub fn number(&mut self, sheet: SheetRef, cell: &CellName) -> Result<f64, Error> {
        let resolved = self.resolved(sheet, cell)?;
        Ok(self.number_from(sheet, cell, resolved))
    }

    /// Reads an angle in radians (ADR 0011).
    pub fn angle(&mut self, sheet: SheetRef, cell: &CellName) -> Result<Angle, Error> {
        Ok(Angle::from_radians(self.number(sheet, cell)?))
    }

    /// Reads a boolean.
    ///
    /// A cached `0`/`1` number is read as `false`/`true` without a warning:
    /// the files write booleans that way when the cell has no `U` code.
    pub fn boolean(&mut self, sheet: SheetRef, cell: &CellName) -> Result<bool, Error> {
        let resolved = self.resolved(sheet, cell)?;
        Ok(self.boolean_from(sheet, cell, resolved))
    }

    /// Reads a string.
    pub fn text(&mut self, sheet: SheetRef, cell: &CellName) -> Result<String, Error> {
        let resolved = self.resolved(sheet, cell)?;
        Ok(self.text_from(sheet, cell, resolved))
    }

    /// Reads a color, resolving palette and document-table indices
    /// (ADR 0019).
    ///
    /// A cached number is an index: 0..23 into the fixed palette
    /// ([`palette`]), 24 and up into `Document::color_table`.
    pub fn color(&mut self, sheet: SheetRef, cell: &CellName) -> Result<Color, Error> {
        let resolved = self.resolved(sheet, cell)?;
        Ok(self.color_from(sheet, cell, resolved))
    }

    /// Reads a numeric list (`LayerMember` and friends) as its numbers.
    ///
    /// A cached list or string splits at `;`, `,` and whitespace; a cached
    /// number is a one-element list. A list with an unparsable entry is a
    /// kind mismatch and substitutes the empty list.
    pub fn list(&mut self, sheet: SheetRef, cell: &CellName) -> Result<Vec<f64>, Error> {
        let resolved = self.resolved(sheet, cell)?;
        Ok(self.list_from(sheet, cell, resolved))
    }

    /// Reads a number from one row of a repeated section (ADR 0019).
    ///
    /// `row` names the row by position, by `IX` or by both
    /// ([`RowTarget`]); `section` names the section (the first section with
    /// that name when a sheet has several). The value policy is the one of
    /// [`CellReader::number`].
    pub fn row_number(
        &mut self,
        sheet: SheetRef,
        section: &str,
        row: RowTarget,
        cell: &CellName,
    ) -> Result<f64, Error> {
        self.row_number_in(sheet, &repeated(section, row), cell)
    }

    /// Reads a string from one row of a repeated section (ADR 0019).
    ///
    /// `row` names the row by position, by `IX` or by both
    /// ([`RowTarget`]); `section` names the section (the first section with
    /// that name when a sheet has several). The value policy is the one of
    /// [`CellReader::text`].
    pub fn row_text(
        &mut self,
        sheet: SheetRef,
        section: &str,
        row: RowTarget,
        cell: &CellName,
    ) -> Result<String, Error> {
        let resolved = self.row_resolved(sheet, section, row, cell)?;
        Ok(self.text_from(sheet, cell, resolved))
    }

    /// Reads a boolean from one row of a repeated section (ADR 0019).
    ///
    /// `row` names the row by position, by `IX` or by both
    /// ([`RowTarget`]); `section` names the section (the first section with
    /// that name when a sheet has several). The value policy is the one of
    /// [`CellReader::boolean`].
    pub fn row_boolean(
        &mut self,
        sheet: SheetRef,
        section: &str,
        row: RowTarget,
        cell: &CellName,
    ) -> Result<bool, Error> {
        let resolved = self.row_resolved(sheet, section, row, cell)?;
        Ok(self.boolean_from(sheet, cell, resolved))
    }

    /// Reads a color from one row of a repeated section (ADR 0019).
    ///
    /// `row` names the row by position, by `IX` or by both
    /// ([`RowTarget`]); `section` names the section (the first section with
    /// that name when a sheet has several). The value policy is the one of
    /// [`CellReader::color`]: palette and document-table indices resolve
    /// and `Themed` goes through the theme.
    pub fn row_color(
        &mut self,
        sheet: SheetRef,
        section: &str,
        row: RowTarget,
        cell: &CellName,
    ) -> Result<Color, Error> {
        let resolved = self.row_resolved(sheet, section, row, cell)?;
        Ok(self.color_from(sheet, cell, resolved))
    }

    /// The resolved cached value of one row cell (ADR 0019 decision 5): the
    /// section named by `section`, the row named by `row` ([`RowTarget`]).
    fn row_resolved(
        &mut self,
        sheet: SheetRef,
        section: &str,
        row: RowTarget,
        cell: &CellName,
    ) -> Result<crate::lookup::Resolved<'d>, Error> {
        let mut resolved = self.doc.resolve_repeated_with_table(
            sheet,
            &repeated(section, row),
            cell,
            crate::generated::CELLS,
        )?;
        prefer_row(
            self.doc,
            sheet,
            &repeated(section, row),
            cell,
            &mut resolved,
        );
        Ok(resolved)
    }

    /// Reads a number from one row of a repeated section, identifying the
    /// section by name, `IX` and position and the row by position and `IX`
    /// (the geometry rows of a sheet with several `Geometry` sections,
    /// ADR 0019 decision 5).
    pub(crate) fn row_number_in(
        &mut self,
        sheet: SheetRef,
        target: &crate::lookup::RepeatedCell<'_>,
        cell: &CellName,
    ) -> Result<f64, Error> {
        let mut resolved =
            self.doc
                .resolve_repeated_with_table(sheet, target, cell, crate::generated::CELLS)?;
        prefer_row(self.doc, sheet, target, cell, &mut resolved);
        Ok(self.number_from(sheet, cell, resolved))
    }

    /// The number policy (ADR 0021) for an already-resolved cell.
    ///
    /// The geometry reader resolves row cells itself (it knows the section
    /// identity) and applies the policy through this method. A cached
    /// number that is not finite (`inf`, `nan`, `1e999`) is a kind
    /// mismatch: it substitutes the hard default with one
    /// [`WarningKind::KindMismatch`] warning (ADR 0021 amendment), never
    /// silently.
    pub(crate) fn number_from(
        &mut self,
        sheet: SheetRef,
        cell: &CellName,
        resolved: crate::lookup::Resolved<'_>,
    ) -> f64 {
        let (value, source) = self.preferred(sheet, cell, &resolved);
        match value {
            Some(CellValue::Str(text)) if text == THEMED => self.themed_number(sheet, cell),
            Some(CellValue::Number(number))
                if source != ValueSource::HardDefault && number.is_finite() =>
            {
                number
            }
            Some(CellValue::Number(_)) if source != ValueSource::HardDefault => {
                self.warn_number(sheet, cell, WarningKind::KindMismatch)
            }
            Some(CellValue::Error(error)) if source != ValueSource::HardDefault => {
                self.warn_number(sheet, cell, WarningKind::ErrorValue(error))
            }
            Some(_) if source != ValueSource::HardDefault => {
                self.warn_number(sheet, cell, WarningKind::KindMismatch)
            }
            _ => self.missing_number(sheet, cell, source),
        }
    }

    /// The substitution for a number nothing defines (ADR 0021 amendment):
    /// the documented default when the ShapeSheet reference documents one
    /// (no warning: a missing cell with a documented default is not an
    /// error), the hard default with a [`WarningKind::Missing`] warning
    /// otherwise. A cell that something defines but that caches no value
    /// always warns.
    fn missing_number(&mut self, sheet: SheetRef, cell: &CellName, source: ValueSource) -> f64 {
        if source == ValueSource::HardDefault
            && let Some(value) = self.documented_number(sheet, cell)
        {
            return value;
        }
        self.warn_number(sheet, cell, WarningKind::Missing)
    }

    /// The documented default of a number cell (ADR 0021 amendment), or
    /// `None` when the ShapeSheet reference documents none. A formula
    /// default (`LocPinX = Width*0.5`) reads the named cell's cached value
    /// and falls back to 0: one multiplication, no formula engine (Phase 6
    /// replaces this).
    fn documented_number(&self, sheet: SheetRef, cell: &CellName) -> Option<f64> {
        match documented_default(cell.as_str())? {
            DocumentedDefault::Number(value) => Some(value),
            DocumentedDefault::HalfOf(other) => Some(self.related_number(sheet, other) * 0.5),
            DocumentedDefault::WholeOf(other) => Some(self.related_number(sheet, other)),
        }
    }

    /// The number another cell resolves to for a documented formula
    /// default: its finite cached value when it has one, `0` otherwise
    /// (the related cell's own read reports its own warnings). A
    /// non-finite cached value is not passed on: the substitution policy
    /// of [`CellReader::number_from`] rejects it with one warning.
    fn related_number(&self, sheet: SheetRef, name: &str) -> f64 {
        let cell = CellName::new(name);
        if let Some(CellValue::Number(value)) = own_value(self.doc, sheet, &cell)
            && value.is_finite()
        {
            return value;
        }
        let resolved = self.resolved(sheet, &cell).ok();
        resolved
            .and_then(|resolved| match resolved.value {
                Some(CellValue::Number(value))
                    if resolved.source != ValueSource::HardDefault && value.is_finite() =>
                {
                    Some(*value)
                }
                _ => None,
            })
            .unwrap_or(0.0)
    }

    /// The number policy for an *optional* row cell (the C and D cells of
    /// an `EllipticalArcTo` row): a cell nothing defines substitutes the
    /// row's own default (ADR 0022: `C = 0`, `D = 1`) without a warning,
    /// and a bad cell substitutes the same default with one warning.
    pub(crate) fn number_from_or(
        &mut self,
        sheet: SheetRef,
        cell: &CellName,
        resolved: crate::lookup::Resolved<'_>,
        default: f64,
    ) -> f64 {
        // A repeated-section cell: the caller resolved the row's
        // identity and applied the snapshot rule already, so the
        // sheet's own name-only search must not override it (P8-15: it
        // returned the first same-named section's cell for every one of
        // them, which kept the `NoLine` of one geometry section on all
        // of a shape's sections).
        let (value, source) = (resolved.value.cloned(), resolved.source);
        if source == ValueSource::HardDefault {
            return default;
        }
        match value {
            Some(CellValue::Str(text)) if text == THEMED => self.themed_number(sheet, cell),
            Some(CellValue::Number(number))
                if source != ValueSource::HardDefault && number.is_finite() =>
            {
                number
            }
            Some(CellValue::Number(_)) if source != ValueSource::HardDefault => {
                self.warn_number(sheet, cell, WarningKind::KindMismatch);
                default
            }
            Some(CellValue::Error(error)) if source != ValueSource::HardDefault => {
                self.warn_number(sheet, cell, WarningKind::ErrorValue(error));
                default
            }
            Some(_) if source != ValueSource::HardDefault => {
                self.warn_number(sheet, cell, WarningKind::KindMismatch);
                default
            }
            _ => default,
        }
    }

    /// The boolean policy for an *optional* cell: a cell nothing defines
    /// substitutes `default` without a warning (the section flags of a
    /// `Geometry` path default to `false`), and a bad cell substitutes the
    /// same default with one warning.
    pub(crate) fn boolean_from_or(
        &mut self,
        sheet: SheetRef,
        cell: &CellName,
        resolved: crate::lookup::Resolved<'_>,
        default: bool,
    ) -> bool {
        // The repeated-section rule of `number_from_or`: the caller's
        // row identity and its snapshot preference already resolved the
        // cell, and the name-only search would answer for the first
        // same-named section (P8-15).
        let (value, source) = (resolved.value.cloned(), resolved.source);
        if source == ValueSource::HardDefault {
            return default;
        }
        match value {
            Some(CellValue::Str(text)) if text == THEMED => self.themed_boolean(sheet, cell),
            Some(CellValue::Bool(flag)) if source != ValueSource::HardDefault => flag,
            Some(CellValue::Number(number)) if source != ValueSource::HardDefault => number != 0.0,
            Some(CellValue::Error(error)) if source != ValueSource::HardDefault => {
                self.warn_boolean(sheet, cell, WarningKind::ErrorValue(error));
                default
            }
            Some(_) if source != ValueSource::HardDefault => {
                self.warn_boolean(sheet, cell, WarningKind::KindMismatch);
                default
            }
            _ => default,
        }
    }

    /// The boolean policy (ADR 0021) for an already-resolved cell.
    pub(crate) fn boolean_from(
        &mut self,
        sheet: SheetRef,
        cell: &CellName,
        resolved: crate::lookup::Resolved<'_>,
    ) -> bool {
        let (value, source) = self.preferred(sheet, cell, &resolved);
        match value {
            Some(CellValue::Str(text)) if text == THEMED => self.themed_boolean(sheet, cell),
            Some(CellValue::Bool(flag)) if source != ValueSource::HardDefault => flag,
            Some(CellValue::Number(number)) if source != ValueSource::HardDefault => number != 0.0,
            Some(CellValue::Error(error)) if source != ValueSource::HardDefault => {
                self.warn_boolean(sheet, cell, WarningKind::ErrorValue(error))
            }
            Some(_) if source != ValueSource::HardDefault => {
                self.warn_boolean(sheet, cell, WarningKind::KindMismatch)
            }
            _ => self.warn_boolean(sheet, cell, WarningKind::Missing),
        }
    }

    /// The warnings recorded so far.
    pub fn warnings(&self) -> &[CellWarning] {
        &self.warnings
    }

    /// The document being read.
    pub fn document(&self) -> &'d Document {
        self.doc
    }

    /// Takes the warnings recorded so far, leaving none.
    pub fn take_warnings(&mut self) -> Vec<CellWarning> {
        std::mem::take(&mut self.warnings)
    }

    /// The ADR 0054/0057 value policy for an already-resolved cell: a
    /// cell whose own entry has a cached value (the snapshot of an `Inh`
    /// cell included) returns that value; the fallback chain runs as
    /// before (the defining rung's cached value, the documented default,
    /// the hard default) and provenance still comes from
    /// [`Document::resolve`](crate::CellLookup::resolve).
    fn preferred(
        &self,
        sheet: SheetRef,
        cell: &CellName,
        resolved: &crate::lookup::Resolved<'_>,
    ) -> (Option<CellValue>, ValueSource) {
        if let Some(value) = own_value(self.doc, sheet, cell) {
            return (Some(value), ValueSource::Local);
        }
        (resolved.value.cloned(), resolved.source)
    }

    /// The string policy (ADR 0021) for an already-resolved cell.
    ///
    /// The row readers resolve row cells themselves and apply the policy
    /// through this method.
    pub(crate) fn text_from(
        &mut self,
        sheet: SheetRef,
        cell: &CellName,
        resolved: crate::lookup::Resolved<'_>,
    ) -> String {
        let (value, source) = self.preferred(sheet, cell, &resolved);
        match value {
            Some(CellValue::Str(text)) if text == THEMED => self.themed_text(sheet, cell),
            Some(CellValue::Str(text)) if source != ValueSource::HardDefault => text,
            Some(CellValue::Error(error)) if source != ValueSource::HardDefault => {
                self.warn_text(sheet, cell, WarningKind::ErrorValue(error))
            }
            Some(_) if source != ValueSource::HardDefault => {
                self.warn_text(sheet, cell, WarningKind::KindMismatch)
            }
            _ => self.warn_text(sheet, cell, WarningKind::Missing),
        }
    }

    /// The color policy (ADR 0021) for an already-resolved cell: palette
    /// and document-table indices resolve (ADR 0019, ADR 0021 amendment).
    pub(crate) fn color_from(
        &mut self,
        sheet: SheetRef,
        cell: &CellName,
        resolved: crate::lookup::Resolved<'_>,
    ) -> Color {
        let (value, source) = self.preferred(sheet, cell, &resolved);
        match value {
            Some(CellValue::Str(text)) if text == THEMED => self.themed_color(sheet, cell),
            Some(CellValue::Color(color)) if source != ValueSource::HardDefault => color,
            Some(CellValue::Number(index)) if source != ValueSource::HardDefault => {
                self.indexed_color(sheet, cell, index)
            }
            Some(CellValue::Error(error)) if source != ValueSource::HardDefault => {
                self.warn_color(sheet, cell, WarningKind::ErrorValue(error))
            }
            Some(_) if source != ValueSource::HardDefault => {
                self.warn_color(sheet, cell, WarningKind::KindMismatch)
            }
            _ => self.warn_color(sheet, cell, WarningKind::Missing),
        }
    }

    /// The list policy (ADR 0021) for an already-resolved cell.
    pub(crate) fn list_from(
        &mut self,
        sheet: SheetRef,
        cell: &CellName,
        resolved: crate::lookup::Resolved<'_>,
    ) -> Vec<f64> {
        let (value, source) = self.preferred(sheet, cell, &resolved);
        match value {
            Some(CellValue::Str(text)) if text == THEMED => self.themed_list(sheet, cell),
            Some(CellValue::List(raw)) if source != ValueSource::HardDefault => {
                self.parse_list(sheet, cell, &raw)
            }
            Some(CellValue::Str(raw)) if source != ValueSource::HardDefault => {
                self.parse_list(sheet, cell, &raw)
            }
            Some(CellValue::Number(number)) if source != ValueSource::HardDefault => vec![number],
            Some(CellValue::Error(error)) if source != ValueSource::HardDefault => {
                self.warn_list(sheet, cell, WarningKind::ErrorValue(error))
            }
            Some(_) if source != ValueSource::HardDefault => {
                self.warn_list(sheet, cell, WarningKind::KindMismatch)
            }
            _ => self.warn_list(sheet, cell, WarningKind::Missing),
        }
    }

    /// Records one warning and returns the substituted number.
    fn warn_number(&mut self, sheet: SheetRef, cell: &CellName, kind: WarningKind) -> f64 {
        self.warn_number_as(sheet, cell, kind, 0.0)
    }

    /// Records one warning and returns `value` in the substituted value's
    /// place (the visible fallbacks of ADR 0021).
    fn warn_number_as(
        &mut self,
        sheet: SheetRef,
        cell: &CellName,
        kind: WarningKind,
        value: f64,
    ) -> f64 {
        self.warn(sheet, cell, kind, CellValue::Number(value));
        value
    }

    /// Records one warning and returns the substituted boolean.
    fn warn_boolean(&mut self, sheet: SheetRef, cell: &CellName, kind: WarningKind) -> bool {
        self.warn(sheet, cell, kind, CellValue::Bool(false));
        false
    }

    /// Records one warning and returns the substituted string.
    fn warn_text(&mut self, sheet: SheetRef, cell: &CellName, kind: WarningKind) -> String {
        self.warn(sheet, cell, kind, CellValue::Str(String::new()));
        String::new()
    }

    /// Records one warning and returns the substituted color.
    fn warn_color(&mut self, sheet: SheetRef, cell: &CellName, kind: WarningKind) -> Color {
        let color = Color::new(0, 0, 0);
        self.warn(sheet, cell, kind, CellValue::Color(color));
        color
    }

    /// Records one warning and returns the substituted list.
    fn warn_list(&mut self, sheet: SheetRef, cell: &CellName, kind: WarningKind) -> Vec<f64> {
        self.warn(sheet, cell, kind, CellValue::List(String::new()));
        Vec::new()
    }

    /// Records one substituted value (ADR 0021).
    fn warn(&mut self, sheet: SheetRef, cell: &CellName, kind: WarningKind, value: CellValue) {
        self.warnings.push(CellWarning {
            sheet,
            cell: cell.clone(),
            kind,
            substituted: value,
        });
    }

    /// Parses a list string into its numbers; an unparsable entry is a kind
    /// mismatch and substitutes the empty list.
    fn parse_list(&mut self, sheet: SheetRef, cell: &CellName, raw: &str) -> Vec<f64> {
        let mut numbers = Vec::new();
        for token in raw.split([';', ',', '\t', '\n', '\r', ' ']) {
            let token = token.trim();
            if token.is_empty() {
                continue;
            }
            match token.parse::<f64>() {
                Ok(number) => numbers.push(number),
                Err(_) => return self.warn_list(sheet, cell, WarningKind::KindMismatch),
            }
        }
        numbers
    }

    /// Resolves a numeric color index: 0..23 is the fixed palette, 24 and
    /// up the document colour table. An index nothing knows is a [`Missing`]
    /// warning; a non-integral number is a kind mismatch.
    fn indexed_color(&mut self, sheet: SheetRef, cell: &CellName, index: f64) -> Color {
        let integral = index.is_finite() && index >= 0.0 && index.fract() == 0.0;
        if !integral || index > u32::MAX as f64 {
            return self.warn_color(sheet, cell, WarningKind::KindMismatch);
        }
        if let Some(color) = palette(index as u32) {
            return color;
        }
        let found = self
            .doc
            .color_table
            .iter()
            .find(|entry| f64::from(entry.index) == index)
            .map(|entry| entry.color);
        found.unwrap_or_else(|| self.warn_color(sheet, cell, WarningKind::Missing))
    }

    /// The theme colour a `Themed` color cell resolves to (ADR 0021
    /// amendment): the shape's `QuickStyle*` colour slot against
    /// `Document::themes[0]`. `None` when the role, the slot or the theme
    /// cannot answer.
    fn theme_color(&self, sheet: SheetRef, cell: &CellName) -> Option<Color> {
        let role = ThemeRole::of_cell(cell.as_str())?;
        let quick_style = CellName::from_static(role.quick_style_cell());
        let resolved = self.resolved(sheet, &quick_style).ok()?;
        let index = match resolved.value? {
            CellValue::Number(index) => *index,
            _ => return None,
        };
        if !index.is_finite() || index < 0.0 || index.fract() != 0.0 || index > u32::MAX as f64 {
            return None;
        }
        let index = index as u32;
        let theme = self.doc.themes.first()?;
        match theme_slot(index)? {
            ThemeSlot::Dk1 => theme.colors.as_ref().map(|colors| colors.dk1),
            ThemeSlot::Lt1 => theme.colors.as_ref().map(|colors| colors.lt1),
            ThemeSlot::Accent(position) => theme
                .colors
                .as_ref()
                .and_then(|colors| colors.accents.get(position).copied()),
            ThemeSlot::Background => theme.background,
            ThemeSlot::Variation(position) => self
                .variation_scheme(sheet, theme)?
                .colors
                .get(position)
                .copied(),
        }
    }

    /// The theme variation colour scheme a sheet's `QuickStyle*` variant
    /// values name ([MS-VSDX] `VariationColorIndex`): the scheme its
    /// `VariationColorIndex` cell selects (0..3 of `Theme::variations`).
    /// A missing cell and the value 65534 defer to the page sheet's cell
    /// ([MS-VSDX]); anything else is variation 0.
    fn variation_scheme<'t>(
        &self,
        sheet: SheetRef,
        theme: &'t crate::theme::Theme,
    ) -> Option<&'t crate::theme::ThemeVariation> {
        theme
            .variations
            .get(self.variation_position(sheet, "VariationColorIndex"))
    }

    /// The theme variation style scheme a sheet's `QuickStyle*Matrix`
    /// variant values name ([MS-VSDX] `VariationStyleIndex`): the scheme
    /// its `VariationStyleIndex` cell selects (0..3 of
    /// [`crate::theme::Theme::variation_styles`]), with the same 65534
    /// fallback.
    fn variation_style_scheme<'t>(
        &self,
        sheet: SheetRef,
        theme: &'t crate::theme::Theme,
    ) -> Option<&'t crate::theme::ThemeVariationStyles> {
        theme
            .variation_styles
            .get(self.variation_position(sheet, "VariationStyleIndex"))
    }

    /// The variation scheme index a sheet selects: its `name` cell, or the
    /// page sheet's when the cell is missing or 65534 ([MS-VSDX]), else 0.
    fn variation_position(&self, sheet: SheetRef, name: &str) -> usize {
        let mut index = self.index_cell(sheet, name).unwrap_or(65534);
        if index == 65534 {
            let page = match sheet {
                SheetRef::Page(page) => Some(page),
                SheetRef::Shape(shape) => self.doc.page_of(shape),
            };
            index = page
                .and_then(|page| self.index_cell(SheetRef::Page(page), name))
                .unwrap_or(0);
        }
        if index == 65534 {
            index = 0;
        }
        usize::try_from(index).unwrap_or(0)
    }

    /// The variation index one sheet's `name` cell names ([MS-VSDX]: 0 to
    /// 65534).
    fn index_cell(&self, sheet: SheetRef, name: &str) -> Option<i32> {
        let resolved = self.resolved(sheet, &CellName::new(name)).ok()?;
        match resolved.value {
            Some(CellValue::Number(index))
                if index.is_finite()
                    && index.fract() == 0.0
                    && *index >= 0.0
                    && *index <= 65534.0 =>
            {
                Some(*index as i32)
            }
            _ => None,
        }
    }

    /// Resolves one cell through the lookup of the sheet's kind.
    fn resolved(
        &self,
        sheet: SheetRef,
        cell: &CellName,
    ) -> Result<crate::lookup::Resolved<'d>, Error> {
        match sheet {
            SheetRef::Shape(shape) => self.doc.resolve(shape, cell),
            SheetRef::Page(page) => self.doc.resolve_page_cell(page, cell),
        }
    }

    /// The substitution for a `Themed` color cell: the theme colour, or
    /// [`WarningKind::UnresolvedTheme`] and black.
    fn themed_color(&mut self, sheet: SheetRef, cell: &CellName) -> Color {
        match self.theme_color(sheet, cell) {
            Some(color) => color,
            None => self.warn_color(sheet, cell, WarningKind::UnresolvedTheme),
        }
    }

    /// The substitution for a `Themed` number cell (ADR 0021 amendment).
    ///
    /// Patterns, weights and transparencies resolve through the theme's
    /// fill and line style lists where a published meaning exists. Where
    /// it does not, a `Themed` pattern falls back to the **visible** value
    /// (`FillPattern`/`LinePattern` 1, `ShdwPattern` 0) and a `Themed`
    /// `LineWeight` to the documented hairline default, each warning
    /// [`WarningKind::UnresolvedTheme`]; never 0 for a fill or a line.
    fn themed_number(&mut self, sheet: SheetRef, cell: &CellName) -> f64 {
        if let Some(value) = self.themed_value(sheet, cell) {
            return value;
        }
        let fallback = match cell.as_str() {
            "FillPattern" | "LinePattern" => 1.0,
            // The `THEMEVAL` reference documents '0.7 pt.' as the value a
            // theme-less document returns for `LineWeight` (a hairline
            // default per the P3-15 note).
            "LineWeight" => 0.7 / 72.0,
            _ => 0.0,
        };
        self.warn_number_as(sheet, cell, WarningKind::UnresolvedTheme, fallback)
    }

    /// The theme answers a `Themed` number cell: the pattern, weight or
    /// transparency its style matrix names (ADR 0021 amendment), or `None`
    /// when the theme cannot answer it.
    fn themed_value(&self, sheet: SheetRef, cell: &CellName) -> Option<f64> {
        match cell.as_str() {
            "FillPattern" => Some(match self.themed_fill_style(sheet)?.kind {
                crate::theme::ThemeFillKind::None => 0.0,
                crate::theme::ThemeFillKind::Solid => 1.0,
                // The scene builder reads 25..40 as a gradient (ADR 0022).
                crate::theme::ThemeFillKind::Gradient => 25.0,
            }),
            "LinePattern" => match self.themed_line_style(sheet)?.kind {
                crate::theme::ThemeLineKind::None => Some(0.0),
                crate::theme::ThemeLineKind::Solid => Some(1.0),
                // A dash preset names no Visio `LinePattern` index (no
                // published meaning): fall back with a warning.
                crate::theme::ThemeLineKind::Dashed => None,
            },
            // `a:ln/@w` is EMU (1/914400 inch, [ISO/IEC29500-1]).
            "LineWeight" => {
                let emu = self.themed_line_style(sheet)?.width_emu?;
                let emu = i32::try_from(emu).ok()?;
                Some(f64::from(emu) / 914400.0)
            }
            "FillForegndTrans" | "FillBkgndTrans" => {
                Some(self.transparency_of(self.themed_fill_style(sheet)?.alpha))
            }
            "LineColorTrans" => Some(self.transparency_of(self.themed_line_style(sheet)?.alpha)),
            _ => None,
        }
    }

    /// The transparency (0..1) of a themed style's `a:alpha` opacity
    /// (0..100000); no alpha is the DrawingML default, opaque.
    fn transparency_of(&self, alpha: Option<u32>) -> f64 {
        1.0 - f64::from(alpha.unwrap_or(100_000).min(100_000)) / 100_000.0
    }

    /// The theme fill style a sheet's `QuickStyleFillMatrix` names (ADR
    /// 0021 amendment), or `None` when the theme cannot answer it.
    fn themed_fill_style(&self, sheet: SheetRef) -> Option<crate::theme::ThemeFillStyle> {
        let theme = self.doc.themes.first()?;
        let slot = self.style_matrix_slot(sheet, "QuickStyleFillMatrix", |styles| {
            styles.fill_indices.as_slice()
        })?;
        theme.fill_styles.get(slot).copied()
    }

    /// The theme line style a sheet's `QuickStyleLineMatrix` names (ADR
    /// 0021 amendment), or `None` when the theme cannot answer it.
    fn themed_line_style(&self, sheet: SheetRef) -> Option<crate::theme::ThemeLineStyle> {
        let theme = self.doc.themes.first()?;
        let slot = self.style_matrix_slot(sheet, "QuickStyleLineMatrix", |styles| {
            styles.line_indices.as_slice()
        })?;
        theme.line_styles.get(slot).copied()
    }

    /// The style slot a `QuickStyle*Matrix` value names ([MS-VSDX]
    /// `QuickStyleFillMatrix`): 1..6 the `a:fmtScheme` styles in order,
    /// 100..103 the `fillIdx`/`lineIdx` of the first..fourth `vt:varStyle`
    /// of the variation style scheme (1-based). 0 names the root style
    /// sheet, not the theme.
    fn style_matrix_slot(
        &self,
        sheet: SheetRef,
        name: &str,
        indices: fn(&crate::theme::ThemeVariationStyles) -> &[u32],
    ) -> Option<usize> {
        let theme = self.doc.themes.first()?;
        let index = self.index_cell(sheet, name)?;
        match index {
            1..=6 => Some(index as usize - 1),
            100..=103 => {
                let scheme = self.variation_style_scheme(sheet, theme)?;
                let entry = *indices(scheme).get(index as usize - 100)?;
                usize::try_from(entry.checked_sub(1)?).ok()
            }
            _ => None,
        }
    }

    /// The substitution for a `Themed` boolean cell.
    fn themed_boolean(&mut self, sheet: SheetRef, cell: &CellName) -> bool {
        self.warn_boolean(sheet, cell, WarningKind::UnresolvedTheme)
    }

    /// The substitution for a `Themed` string cell.
    fn themed_text(&mut self, sheet: SheetRef, cell: &CellName) -> String {
        self.warn_text(sheet, cell, WarningKind::UnresolvedTheme)
    }

    /// The substitution for a `Themed` list cell.
    fn themed_list(&mut self, sheet: SheetRef, cell: &CellName) -> Vec<f64> {
        self.warn_list(sheet, cell, WarningKind::UnresolvedTheme)
    }
}

/// The theme role a colour cell plays (ADR 0021 amendment): it selects the
/// `QuickStyle*` cell whose slot names the theme colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ThemeRole {
    /// The shape's line colour (`QuickStyleLineColor`).
    Line,
    /// The shape's fill colour (`QuickStyleFillColor`).
    Fill,
    /// The shape's shadow colour (`QuickStyleShadowColor`).
    Shadow,
    /// The shape's text colour (`QuickStyleFontColor`).
    Font,
}

impl ThemeRole {
    /// The role of a cell by its name, or `None` when the cell is not a
    /// themed colour of a role we map.
    fn of_cell(cell: &str) -> Option<Self> {
        match cell {
            "Color" => Some(Self::Font),
            _ if cell.starts_with("Line") => Some(Self::Line),
            _ if cell.starts_with("Fill") || cell.starts_with("Gradient") => Some(Self::Fill),
            _ if cell.contains("Shdw")
                || cell.starts_with("Shadow")
                || cell.starts_with("Glow")
                || cell.starts_with("Bevel") =>
            {
                Some(Self::Shadow)
            }
            _ => None,
        }
    }

    /// The `QuickStyle*` cell that names this role's theme colour.
    fn quick_style_cell(self) -> &'static str {
        match self {
            Self::Line => "QuickStyleLineColor",
            Self::Fill => "QuickStyleFillColor",
            Self::Shadow => "QuickStyleShadowColor",
            Self::Font => "QuickStyleFontColor",
        }
    }
}

/// The theme colour a `QuickStyle*` slot names (ADR 0021 amendment,
/// [MS-VSDX] `QuickStyleFillColor`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ThemeSlot {
    /// Slot 0: the dark 1 theme colour.
    Dk1,
    /// Slot 1: the light 1 theme colour.
    Lt1,
    /// Slots 2..7: accent 1..6, by position in `ThemeColors::accents`.
    Accent(usize),
    /// Slot 8: the theme's background colour (`Theme::background`).
    Background,
    /// Slots 100..106 and 200..206: variation colour 1..7 of the active
    /// theme variation (`ThemeVariation::colors`).
    Variation(usize),
}

/// The theme slot a `QuickStyle*` colour value names, or `None` for a value
/// no published meaning covers.
///
/// The mapping ([MS-VSDX] `QuickStyleFillColor`, `VisQuickStyleColors`):
/// 0 Dark (`dk1`), 1 Light (`lt1`), 2..7 Accent 1..6, 8 the background
/// colour, 100..106 the variation colours 1..7 (`varColor1`..`varColor7`
/// of the variation scheme the `VariationColorIndex` cell selects). The
/// 200..206 family has **no published meaning**; the corpus shows the same
/// variation colours (100+k and 200+k both name `varColor{k+1}`, ADR 0021
/// amendment), so we resolve it that way and record it as corpus-derived.
fn theme_slot(index: u32) -> Option<ThemeSlot> {
    match index {
        0 => Some(ThemeSlot::Dk1),
        1 => Some(ThemeSlot::Lt1),
        2..=7 => Some(ThemeSlot::Accent(index as usize - 2)),
        8 => Some(ThemeSlot::Background),
        100..=106 => Some(ThemeSlot::Variation(index as usize - 100)),
        200..=206 => Some(ThemeSlot::Variation(index as usize - 200)),
        _ => None,
    }
}

/// The fixed Visio palette (indices 0..23), our own transcription of the
/// default colour table (ADR 0019 decision 6).
///
/// Colour names come from the `VisDefaultColors` enumeration on Microsoft
/// Learn (source 13 of `docs/sources.md`); the RGB values are the
/// [MS-VSDX] section 2.2.7.6 "Color Table" (source 3).
///
/// Returns `None` for an index the table does not hold (24 and up resolve
/// through `Document::color_table`).
pub fn palette(index: u32) -> Option<Color> {
    /// Index, name and RGB of every fixed palette entry.
    const TABLE: &[(u32, &str, Color)] = &[
        (0, "Black", Color::new(0, 0, 0)),
        (1, "White", Color::new(255, 255, 255)),
        (2, "Red", Color::new(255, 0, 0)),
        (3, "Green", Color::new(0, 255, 0)),
        (4, "Blue", Color::new(0, 0, 255)),
        (5, "Yellow", Color::new(255, 255, 0)),
        (6, "Magenta", Color::new(255, 0, 255)),
        (7, "Cyan", Color::new(0, 255, 255)),
        (8, "Dark red", Color::new(128, 0, 0)),
        (9, "Dark green", Color::new(0, 128, 0)),
        (10, "Dark blue", Color::new(0, 0, 128)),
        (11, "Dark yellow", Color::new(128, 128, 0)),
        (12, "Purple", Color::new(128, 0, 128)),
        (13, "Dark cyan", Color::new(0, 128, 128)),
        (14, "Gray", Color::new(192, 192, 192)),
        (15, "10% gray", Color::new(230, 230, 230)),
        (16, "20% gray", Color::new(205, 205, 205)),
        (17, "30% gray", Color::new(179, 179, 179)),
        (18, "40% gray", Color::new(154, 154, 154)),
        (19, "50% gray", Color::new(128, 128, 128)),
        (20, "60% gray", Color::new(102, 102, 102)),
        (21, "70% gray", Color::new(77, 77, 77)),
        (22, "80% gray", Color::new(51, 51, 51)),
        (23, "90% gray", Color::new(26, 26, 26)),
    ];
    TABLE
        .iter()
        .find(|(seen, _, _)| *seen == index)
        .map(|(_, _, color)| *color)
}

#[cfg(test)]
mod tests {
    use crate::cell::Cell;
    use crate::document::Document;
    use crate::id::ShapeId;
    use crate::name::CellName;
    use crate::shape::Shape;
    use crate::theme::{
        ColorTableEntry, Theme, ThemeColors, ThemeFillKind, ThemeFillStyle, ThemeFonts,
        ThemeLineKind, ThemeLineStyle, ThemeVariation, ThemeVariationStyles,
    };

    use super::*;

    /// A document with one empty shape (id 0).
    fn document_with_shape() -> (Document, ShapeId) {
        let mut doc = Document::new();
        let shape = doc.add_shape(Shape::new(ShapeId { index: 0 }, 1));
        (doc, shape)
    }

    /// A document with one shape whose sheet has `cells`.
    fn document_with_cells(cells: Vec<Cell>) -> (Document, ShapeId) {
        let (mut doc, shape) = document_with_shape();
        if let Some(shape) = doc.shape_mut(shape) {
            shape.sheet.cells = cells;
        }
        (doc, shape)
    }

    /// A cell with a cached value.
    fn cell(name: &str, value: CellValue) -> Cell {
        let mut cell = Cell::new(CellName::new(name));
        cell.value = Some(value);
        cell
    }

    /// The colour scheme of `themes[0]`, with recognizable slots.
    fn colors() -> ThemeColors {
        ThemeColors {
            name: "Test".to_owned(),
            dk1: Color::new(1, 1, 1),
            lt1: Color::new(2, 2, 2),
            dk2: Color::new(3, 3, 3),
            lt2: Color::new(4, 4, 4),
            accents: [
                Color::new(10, 0, 0),
                Color::new(11, 0, 0),
                Color::new(12, 0, 0),
                Color::new(13, 0, 0),
                Color::new(14, 0, 0),
                Color::new(15, 0, 0),
            ],
            hlink: Color::new(5, 5, 5),
            fol_hlink: Color::new(6, 6, 6),
        }
    }

    /// Adds one theme with `colors` to the document.
    fn with_theme(doc: &mut Document) {
        let mut theme = Theme::new("Test");
        theme.colors = Some(colors());
        theme.fonts = Some(ThemeFonts {
            name: "Test".to_owned(),
            major_latin: "Major".to_owned(),
            minor_latin: "Minor".to_owned(),
            major_ea: String::new(),
            minor_ea: String::new(),
            major_cs: String::new(),
            minor_cs: String::new(),
        });
        doc.themes.push(theme);
    }

    #[test]
    fn reader_starts_without_warnings() {
        let doc = Document::new();
        let reader = CellReader::new(&doc);
        assert!(reader.warnings().is_empty());
        assert!(std::ptr::eq(reader.document(), &doc));
    }

    #[test]
    fn take_warnings_drains_the_list() {
        let doc = Document::new();
        let mut reader = CellReader::new(&doc);
        reader.warnings.push(CellWarning {
            sheet: SheetRef::Shape(ShapeId { index: 0 }),
            cell: CellName::from_static("PinX"),
            kind: WarningKind::Missing,
            substituted: CellValue::Number(0.0),
        });
        assert_eq!(reader.warnings().len(), 1);
        let taken = reader.take_warnings();
        assert_eq!(taken.len(), 1);
        assert!(reader.warnings().is_empty());
    }

    #[test]
    fn sheet_refs_are_distinct() {
        assert_ne!(
            SheetRef::Shape(ShapeId { index: 1 }),
            SheetRef::Page(PageId { index: 1 })
        );
    }

    // --- numbers and angles -------------------------------------------

    #[test]
    fn number_reads_a_cached_number() {
        let (doc, shape) = document_with_cells(vec![cell("Width", CellValue::Number(2.5))]);
        let mut reader = CellReader::new(&doc);
        let got = reader.number(SheetRef::Shape(shape), &CellName::from_static("Width"));
        assert_eq!(got, Ok(2.5));
        assert!(reader.warnings().is_empty());
    }

    #[test]
    fn angle_reads_radians() {
        let (doc, shape) = document_with_cells(vec![cell("Angle", CellValue::Number(1.5))]);
        let mut reader = CellReader::new(&doc);
        let got = reader
            .angle(SheetRef::Shape(shape), &CellName::from_static("Angle"))
            .expect("reads");
        assert_eq!(got.radians(), 1.5);
        assert!(reader.warnings().is_empty());
    }

    #[test]
    fn a_missing_value_substitutes_zero_with_one_warning() {
        let (doc, shape) = document_with_shape();
        let mut reader = CellReader::new(&doc);
        assert_eq!(
            reader.number(SheetRef::Shape(shape), &CellName::from_static("PinX")),
            Ok(0.0)
        );
        assert_eq!(reader.warnings().len(), 1);
        let warning = &reader.warnings()[0];
        assert_eq!(warning.kind, WarningKind::Missing);
        assert_eq!(warning.substituted, CellValue::Number(0.0));
        assert_eq!(warning.cell, CellName::from_static("PinX"));
        assert_eq!(warning.sheet, SheetRef::Shape(shape));
    }

    #[test]
    fn an_error_value_substitutes_zero_and_keeps_the_error() {
        let (doc, shape) =
            document_with_cells(vec![cell("PinX", CellValue::Error("#VALUE!".to_owned()))]);
        let mut reader = CellReader::new(&doc);
        assert_eq!(
            reader.number(SheetRef::Shape(shape), &CellName::from_static("PinX")),
            Ok(0.0)
        );
        assert_eq!(
            reader.warnings()[0].kind,
            WarningKind::ErrorValue("#VALUE!".to_owned())
        );
    }

    #[test]
    fn a_non_finite_cached_number_is_one_kind_mismatch() {
        // `V='inf'` parses to a non-finite number (`f64::from_str`);
        // ADR 0021 forbids substituting it silently.
        for bad in [f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
            let (doc, shape) = document_with_cells(vec![cell("PinX", CellValue::Number(bad))]);
            let mut reader = CellReader::new(&doc);
            assert_eq!(
                reader.number(SheetRef::Shape(shape), &CellName::from_static("PinX")),
                Ok(0.0)
            );
            assert_eq!(reader.warnings().len(), 1);
            let warning = &reader.warnings()[0];
            assert_eq!(warning.kind, WarningKind::KindMismatch);
            assert_eq!(warning.substituted, CellValue::Number(0.0));
        }
    }

    #[test]
    fn a_non_finite_optional_row_cell_keeps_the_row_default() {
        let (doc, shape) = document_with_cells(vec![cell("C", CellValue::Number(f64::INFINITY))]);
        let name = CellName::from_static("C");
        let resolved = doc.resolve(shape, &name).expect("resolves");
        let mut reader = CellReader::new(&doc);
        assert_eq!(
            reader.number_from_or(SheetRef::Shape(shape), &name, resolved, 1.0),
            1.0
        );
        assert_eq!(reader.warnings().len(), 1);
        assert_eq!(reader.warnings()[0].kind, WarningKind::KindMismatch);
    }

    #[test]
    fn a_kind_mismatch_is_reported_per_requested_kind() {
        let (doc, shape) = document_with_cells(vec![cell("PinX", CellValue::Str("hello".into()))]);
        let mut reader = CellReader::new(&doc);
        assert_eq!(
            reader.number(SheetRef::Shape(shape), &CellName::from_static("PinX")),
            Ok(0.0)
        );
        assert_eq!(reader.warnings()[0].kind, WarningKind::KindMismatch);

        let (doc, shape) = document_with_cells(vec![cell("PinX", CellValue::Number(1.0))]);
        let mut reader = CellReader::new(&doc);
        assert_eq!(
            reader.text(SheetRef::Shape(shape), &CellName::from_static("PinX")),
            Ok(String::new())
        );
        assert_eq!(reader.warnings()[0].kind, WarningKind::KindMismatch);

        let (doc, shape) = document_with_cells(vec![cell("PinX", CellValue::Bool(true))]);
        let mut reader = CellReader::new(&doc);
        assert_eq!(
            reader.number(SheetRef::Shape(shape), &CellName::from_static("PinX")),
            Ok(0.0)
        );
        assert_eq!(reader.warnings()[0].kind, WarningKind::KindMismatch);
    }

    #[test]
    fn an_unknown_shape_id_is_a_caller_error() {
        let doc = Document::new();
        let mut reader = CellReader::new(&doc);
        assert_eq!(
            reader.number(
                SheetRef::Shape(ShapeId { index: 3 }),
                &CellName::from_static("PinX")
            ),
            Err(Error::UnknownShape(ShapeId { index: 3 }))
        );
        assert_eq!(
            reader.row_number(
                SheetRef::Shape(ShapeId { index: 3 }),
                "Geometry",
                RowTarget::at(0),
                &CellName::from_static("X")
            ),
            Err(Error::UnknownShape(ShapeId { index: 3 }))
        );
        assert_eq!(
            reader.number(
                SheetRef::Page(PageId { index: 3 }),
                &CellName::from_static("PageWidth")
            ),
            Err(Error::UnknownPage(PageId { index: 3 }))
        );
    }

    // --- booleans, strings, lists -------------------------------------

    #[test]
    fn boolean_reads_bools_and_coerces_zero_and_one() {
        let (doc, shape) = document_with_cells(vec![
            cell("FlipX", CellValue::Bool(true)),
            cell("FlipY", CellValue::Number(0.0)),
            cell("NoFill", CellValue::Number(3.0)),
        ]);
        let mut reader = CellReader::new(&doc);
        let sheet = SheetRef::Shape(shape);
        assert_eq!(
            reader.boolean(sheet, &CellName::from_static("FlipX")),
            Ok(true)
        );
        assert_eq!(
            reader.boolean(sheet, &CellName::from_static("FlipY")),
            Ok(false)
        );
        assert_eq!(
            reader.boolean(sheet, &CellName::from_static("NoFill")),
            Ok(true)
        );
        assert!(reader.warnings().is_empty());
    }

    #[test]
    fn boolean_substitutes_false_on_bad_values() {
        let (doc, shape) = document_with_cells(vec![
            cell("A", CellValue::Str("nope".into())),
            cell("B", CellValue::Error("#DIV/0!".into())),
        ]);
        let mut reader = CellReader::new(&doc);
        let sheet = SheetRef::Shape(shape);
        assert_eq!(
            reader.boolean(sheet, &CellName::from_static("A")),
            Ok(false)
        );
        assert_eq!(reader.warnings()[0].kind, WarningKind::KindMismatch,);
        assert_eq!(
            reader.boolean(sheet, &CellName::from_static("B")),
            Ok(false)
        );
        assert_eq!(
            reader.warnings()[1].kind,
            WarningKind::ErrorValue("#DIV/0!".to_owned())
        );
        assert_eq!(
            reader.boolean(sheet, &CellName::from_static("C")),
            Ok(false)
        );
        assert_eq!(reader.warnings()[2].kind, WarningKind::Missing);
    }

    #[test]
    fn text_reads_strings_and_substitutes_empty() {
        let (doc, shape) = document_with_cells(vec![
            cell("Prop", CellValue::Str("hello".into())),
            cell("Bad", CellValue::Number(1.0)),
        ]);
        let mut reader = CellReader::new(&doc);
        let sheet = SheetRef::Shape(shape);
        assert_eq!(
            reader.text(sheet, &CellName::from_static("Prop")),
            Ok("hello".to_owned())
        );
        assert_eq!(
            reader.text(sheet, &CellName::from_static("Bad")),
            Ok(String::new())
        );
        assert_eq!(reader.warnings()[0].kind, WarningKind::KindMismatch);
    }

    #[test]
    fn list_parses_the_written_separators() {
        let (doc, shape) = document_with_cells(vec![
            cell("LayerMember", CellValue::List("0;2;5".to_owned())),
            cell("One", CellValue::Number(3.0)),
            cell("Spaced", CellValue::Str("1, 2 ;3".to_owned())),
        ]);
        let mut reader = CellReader::new(&doc);
        let sheet = SheetRef::Shape(shape);
        assert_eq!(
            reader.list(sheet, &CellName::from_static("LayerMember")),
            Ok(vec![0.0, 2.0, 5.0])
        );
        assert_eq!(
            reader.list(sheet, &CellName::from_static("One")),
            Ok(vec![3.0])
        );
        assert_eq!(
            reader.list(sheet, &CellName::from_static("Spaced")),
            Ok(vec![1.0, 2.0, 3.0])
        );
        assert!(reader.warnings().is_empty());
    }

    #[test]
    fn list_substitutes_empty_on_bad_entries_and_kinds() {
        let (doc, shape) = document_with_cells(vec![
            cell("Bad", CellValue::List("1;x".to_owned())),
            cell("Color", CellValue::Color(Color::new(1, 2, 3))),
            cell("Err", CellValue::Error("#VALUE!".to_owned())),
        ]);
        let mut reader = CellReader::new(&doc);
        let sheet = SheetRef::Shape(shape);
        assert_eq!(
            reader.list(sheet, &CellName::from_static("Bad")),
            Ok(vec![])
        );
        assert_eq!(reader.warnings()[0].kind, WarningKind::KindMismatch);
        assert_eq!(
            reader.list(sheet, &CellName::from_static("Color")),
            Ok(vec![])
        );
        assert_eq!(reader.warnings()[1].kind, WarningKind::KindMismatch);
        assert_eq!(
            reader.list(sheet, &CellName::from_static("Err")),
            Ok(vec![])
        );
        assert_eq!(
            reader.warnings()[2].kind,
            WarningKind::ErrorValue("#VALUE!".to_owned())
        );
    }

    // --- colors and indices -------------------------------------------

    #[test]
    fn color_reads_a_cached_color() {
        let (doc, shape) = document_with_cells(vec![cell(
            "LineColor",
            CellValue::Color(Color::new(1, 2, 3)),
        )]);
        let mut reader = CellReader::new(&doc);
        assert_eq!(
            reader.color(SheetRef::Shape(shape), &CellName::from_static("LineColor")),
            Ok(Color::new(1, 2, 3))
        );
        assert!(reader.warnings().is_empty());
    }

    #[test]
    fn color_numbers_resolve_palette_and_table_indices() {
        let (mut doc, shape) = document_with_cells(vec![
            cell("A", CellValue::Number(2.0)),
            cell("B", CellValue::Number(25.0)),
            cell("C", CellValue::Number(400.0)),
            cell("D", CellValue::Number(-1.0)),
            cell("E", CellValue::Number(1.5)),
        ]);
        doc.color_table.push(ColorTableEntry {
            index: 25,
            color: Color::new(9, 9, 9),
        });
        let mut reader = CellReader::new(&doc);
        let sheet = SheetRef::Shape(shape);
        assert_eq!(
            reader.color(sheet, &CellName::from_static("A")),
            Ok(Color::new(255, 0, 0))
        );
        assert_eq!(
            reader.color(sheet, &CellName::from_static("B")),
            Ok(Color::new(9, 9, 9))
        );
        assert_eq!(
            reader.color(sheet, &CellName::from_static("C")),
            Ok(Color::new(0, 0, 0))
        );
        assert_eq!(reader.warnings()[0].kind, WarningKind::Missing);
        assert_eq!(
            reader.color(sheet, &CellName::from_static("D")),
            Ok(Color::new(0, 0, 0))
        );
        assert_eq!(reader.warnings()[1].kind, WarningKind::KindMismatch);
        assert_eq!(
            reader.color(sheet, &CellName::from_static("E")),
            Ok(Color::new(0, 0, 0))
        );
        assert_eq!(reader.warnings()[2].kind, WarningKind::KindMismatch);
    }

    #[test]
    fn color_substitutes_black_on_errors_and_mismatches() {
        let (doc, shape) = document_with_cells(vec![
            cell("A", CellValue::Error("#VALUE!".to_owned())),
            cell("B", CellValue::Bool(true)),
        ]);
        let mut reader = CellReader::new(&doc);
        let sheet = SheetRef::Shape(shape);
        assert_eq!(
            reader.color(sheet, &CellName::from_static("A")),
            Ok(Color::new(0, 0, 0))
        );
        assert_eq!(
            reader.warnings()[0].kind,
            WarningKind::ErrorValue("#VALUE!".to_owned())
        );
        assert_eq!(
            reader.color(sheet, &CellName::from_static("B")),
            Ok(Color::new(0, 0, 0))
        );
        assert_eq!(reader.warnings()[1].kind, WarningKind::KindMismatch);
    }

    // --- Themed values ------------------------------------------------

    #[test]
    fn themed_colors_resolve_through_the_quick_style_cells() {
        let (mut doc, shape) = document_with_cells(vec![
            cell("LineColor", CellValue::Str("Themed".to_owned())),
            cell("FillForegnd", CellValue::Str("Themed".to_owned())),
            cell("ShapeShdwColor", CellValue::Str("Themed".to_owned())),
            cell("Color", CellValue::Str("Themed".to_owned())),
            cell("QuickStyleLineColor", CellValue::Number(0.0)),
            cell("QuickStyleFillColor", CellValue::Number(2.0)),
            cell("QuickStyleShadowColor", CellValue::Number(7.0)),
            cell("QuickStyleFontColor", CellValue::Number(1.0)),
        ]);
        with_theme(&mut doc);
        let mut reader = CellReader::new(&doc);
        let sheet = SheetRef::Shape(shape);
        assert_eq!(
            reader.color(sheet, &CellName::from_static("LineColor")),
            Ok(Color::new(1, 1, 1))
        );
        assert_eq!(
            reader.color(sheet, &CellName::from_static("FillForegnd")),
            Ok(Color::new(10, 0, 0))
        );
        assert_eq!(
            reader.color(sheet, &CellName::from_static("ShapeShdwColor")),
            Ok(Color::new(15, 0, 0))
        );
        assert_eq!(
            reader.color(sheet, &CellName::from_static("Color")),
            Ok(Color::new(2, 2, 2))
        );
        assert!(reader.warnings().is_empty());
    }

    #[test]
    fn a_themed_color_with_no_answer_substitutes_black() {
        // No theme, no QuickStyle cell, a variant slot and an unknown role.
        let (doc, shape) = document_with_cells(vec![
            cell("LineColor", CellValue::Str("Themed".to_owned())),
            cell("FillForegnd", CellValue::Str("Themed".to_owned())),
            cell("QuickStyleLineColor", CellValue::Number(106.0)),
            cell("QuickStyleFillColor", CellValue::Number(0.0)),
            cell("MysteryColor", CellValue::Str("Themed".to_owned())),
        ]);
        let mut reader = CellReader::new(&doc);
        let sheet = SheetRef::Shape(shape);
        assert_eq!(
            reader.color(sheet, &CellName::from_static("LineColor")),
            Ok(Color::new(0, 0, 0))
        );
        assert_eq!(
            reader.color(sheet, &CellName::from_static("FillForegnd")),
            Ok(Color::new(0, 0, 0))
        );
        assert_eq!(
            reader.color(sheet, &CellName::from_static("MysteryColor")),
            Ok(Color::new(0, 0, 0))
        );
        assert_eq!(reader.warnings().len(), 3);
        for warning in reader.warnings() {
            assert_eq!(warning.kind, WarningKind::UnresolvedTheme);
        }
    }

    #[test]
    fn a_themed_quick_style_cell_of_the_wrong_kind_does_not_resolve() {
        let (mut doc, shape) = document_with_cells(vec![
            cell("LineColor", CellValue::Str("Themed".to_owned())),
            cell("QuickStyleLineColor", CellValue::Str("Themed".to_owned())),
        ]);
        with_theme(&mut doc);
        let mut reader = CellReader::new(&doc);
        assert_eq!(
            reader.color(SheetRef::Shape(shape), &CellName::from_static("LineColor")),
            Ok(Color::new(0, 0, 0))
        );
        assert_eq!(reader.warnings()[0].kind, WarningKind::UnresolvedTheme);
    }

    #[test]
    fn a_themed_number_boolean_text_and_list_substitute_with_one_warning() {
        let (mut doc, shape) = document_with_cells(vec![
            cell("LineColorTrans", CellValue::Str("Themed".to_owned())),
            cell("LineWeight", CellValue::Str("Themed".to_owned())),
            cell("FillGradientEnabled", CellValue::Str("Themed".to_owned())),
            cell("Label", CellValue::Str("Themed".to_owned())),
            cell("LayerMember", CellValue::Str("Themed".to_owned())),
        ]);
        with_theme(&mut doc);
        let mut reader = CellReader::new(&doc);
        let sheet = SheetRef::Shape(shape);
        assert_eq!(
            reader.number(sheet, &CellName::from_static("LineColorTrans")),
            Ok(0.0)
        );
        // The `LineWeight` fallback is the documented hairline default
        // (0.7 point), never 0 (ADR 0021 amendment).
        assert_eq!(
            reader.number(sheet, &CellName::from_static("LineWeight")),
            Ok(0.7 / 72.0)
        );
        assert_eq!(
            reader.boolean(sheet, &CellName::from_static("FillGradientEnabled")),
            Ok(false)
        );
        assert_eq!(
            reader.text(sheet, &CellName::from_static("Label")),
            Ok(String::new())
        );
        assert_eq!(
            reader.list(sheet, &CellName::from_static("LayerMember")),
            Ok(vec![])
        );
        assert_eq!(reader.warnings().len(), 5);
        for warning in reader.warnings() {
            assert_eq!(warning.kind, WarningKind::UnresolvedTheme);
        }
    }

    #[test]
    fn row_number_reads_row_cells_with_the_same_policy() {
        let (mut doc, shape) = document_with_shape();
        {
            let shape = doc.shape_mut(shape).expect("exists");
            let mut section =
                crate::section::Section::new(crate::name::SectionName::from_static("Geometry"));
            let mut row = crate::section::Row::new();
            row.row_type = Some("LineTo".to_owned());
            row.cells.push(cell("X", CellValue::Number(1.25)));
            let mut missing = crate::section::Row::new();
            missing.row_type = Some("MoveTo".to_owned());
            section.rows.push(row);
            section.rows.push(missing);
            shape.sheet.sections.push(section);
        }
        let mut reader = CellReader::new(&doc);
        let sheet = SheetRef::Shape(shape);
        assert_eq!(
            reader.row_number(
                sheet,
                "Geometry",
                RowTarget::at(0),
                &CellName::from_static("X")
            ),
            Ok(1.25)
        );
        assert!(reader.warnings().is_empty());
        assert_eq!(
            reader.row_number(
                sheet,
                "Geometry",
                RowTarget::at(1),
                &CellName::from_static("X")
            ),
            Ok(0.0)
        );
        assert_eq!(reader.warnings()[0].kind, WarningKind::Missing);
        // A row past the end substitutes too.
        assert_eq!(
            reader.row_number(
                sheet,
                "Geometry",
                RowTarget::at(9),
                &CellName::from_static("X")
            ),
            Ok(0.0)
        );
    }

    #[test]
    fn row_text_boolean_and_color_read_row_cells_with_the_same_policy() {
        let (mut doc, shape) = document_with_shape();
        {
            let shape = doc.shape_mut(shape).expect("exists");
            let mut section =
                crate::section::Section::new(crate::name::SectionName::from_static("Character"));
            let mut row = crate::section::Row::new();
            row.row_type = Some("Character".to_owned());
            row.cells
                .push(cell("Font", CellValue::Str("Calibri".into())));
            row.cells.push(cell("Strikethru", CellValue::Number(1.0)));
            row.cells.push(cell("Color", CellValue::Number(2.0)));
            section.rows.push(row);
            let mut bad = crate::section::Row::new();
            bad.row_type = Some("Character".to_owned());
            bad.cells.push(cell("Font", CellValue::Number(1.0)));
            section.rows.push(bad);
            shape.sheet.sections.push(section);
        }
        let mut reader = CellReader::new(&doc);
        let sheet = SheetRef::Shape(shape);
        let font = CellName::from_static("Font");
        let strike = CellName::from_static("Strikethru");
        let color = CellName::from_static("Color");
        assert_eq!(
            reader.row_text(sheet, "Character", RowTarget::at(0), &font),
            Ok("Calibri".to_owned())
        );
        assert_eq!(
            reader.row_boolean(sheet, "Character", RowTarget::at(0), &strike),
            Ok(true)
        );
        assert_eq!(
            reader.row_color(sheet, "Character", RowTarget::at(0), &color),
            Ok(Color::new(255, 0, 0))
        );
        assert!(reader.warnings().is_empty());
        // A bad kind substitutes with one warning, like the sheet readers.
        assert_eq!(
            reader.row_text(sheet, "Character", RowTarget::at(1), &font),
            Ok(String::new())
        );
        assert_eq!(reader.warnings()[0].kind, WarningKind::KindMismatch);
        // A row past the end substitutes too.
        assert_eq!(
            reader.row_boolean(sheet, "Character", RowTarget::at(9), &strike),
            Ok(false)
        );
        assert_eq!(reader.warnings()[1].kind, WarningKind::Missing);
    }

    #[test]
    fn a_row_read_by_ix_matches_the_master_row_by_ix() {
        // A master whose `Character` rows are `IX` 0 and 5, and an
        // instance without a `Character` section: a run marked `IX` 5
        // reads the master's row 5 (matched by `IX`, never by position).
        let (mut doc, shape) = document_with_shape();
        let mid = doc.masters.len();
        let mut master = crate::master::Master::new(crate::id::MasterId { index: mid as u32 }, 1);
        let content = doc.add_shape(Shape::new(ShapeId { index: 0 }, 1));
        {
            let content = doc.shape_mut(content).expect("content exists");
            let mut section =
                crate::section::Section::new(crate::name::SectionName::from_static("Character"));
            let mut row_zero = crate::section::Row::new();
            row_zero.ix = Some(0);
            row_zero
                .cells
                .push(cell("Font", CellValue::Str("Zero".into())));
            let mut row_five = crate::section::Row::new();
            row_five.ix = Some(5);
            row_five
                .cells
                .push(cell("Font", CellValue::Str("Five".into())));
            section.rows.push(row_zero);
            section.rows.push(row_five);
            content.sheet.sections.push(section);
        }
        master.shapes = vec![content];
        doc.masters.push(master);
        doc.shape_mut(shape).expect("shape exists").master =
            Some(crate::id::MasterId { index: mid as u32 });

        let mut reader = CellReader::new(&doc);
        let sheet = SheetRef::Shape(shape);
        let font = CellName::from_static("Font");
        assert_eq!(
            reader.row_text(sheet, "Character", RowTarget::by_ix(5), &font),
            Ok("Five".to_owned())
        );
        assert_eq!(
            reader.row_text(sheet, "Character", RowTarget::by_ix(0), &font),
            Ok("Zero".to_owned())
        );
        assert!(reader.warnings().is_empty(), "{:?}", reader.warnings());
        // A marker `IX` no row has substitutes the defaults with one
        // warning.
        assert_eq!(
            reader.row_text(sheet, "Character", RowTarget::by_ix(9), &font),
            Ok(String::new())
        );
        assert_eq!(reader.warnings().len(), 1);
        assert_eq!(reader.warnings()[0].kind, WarningKind::Missing);
    }

    #[test]
    fn row_readers_report_a_stale_shape() {
        let doc = Document::new();
        let mut reader = CellReader::new(&doc);
        let sheet = SheetRef::Shape(ShapeId { index: 3 });
        let cell = CellName::from_static("Font");
        assert_eq!(
            reader.row_text(sheet, "Character", RowTarget::at(0), &cell),
            Err(Error::UnknownShape(ShapeId { index: 3 }))
        );
        assert_eq!(
            reader.row_boolean(sheet, "Character", RowTarget::at(0), &cell),
            Err(Error::UnknownShape(ShapeId { index: 3 }))
        );
        assert_eq!(
            reader.row_color(sheet, "Character", RowTarget::at(0), &cell),
            Err(Error::UnknownShape(ShapeId { index: 3 }))
        );
    }

    // --- the fixed palette --------------------------------------------

    #[test]
    fn palette_holds_every_index_from_zero_to_twenty_three() {
        assert_eq!(palette(0), Some(Color::new(0, 0, 0)));
        assert_eq!(palette(1), Some(Color::new(255, 255, 255)));
        assert_eq!(palette(2), Some(Color::new(255, 0, 0)));
        // [MS-VSDX] 2.2.7.6: 14 is #C0C0C0, 16 is #CDCDCD, 18 is #9A9A9A
        // (not the conventional 128 / 204 / 153).
        assert_eq!(palette(14), Some(Color::new(192, 192, 192)));
        assert_eq!(palette(16), Some(Color::new(205, 205, 205)));
        assert_eq!(palette(18), Some(Color::new(154, 154, 154)));
        assert_eq!(palette(19), Some(Color::new(128, 128, 128)));
        assert_eq!(palette(23), Some(Color::new(26, 26, 26)));
        for index in 0..=23 {
            assert!(palette(index).is_some(), "palette {index}");
        }
        assert_eq!(palette(24), None);
        assert_eq!(palette(255), None);
    }

    #[test]
    fn palette_entries_are_named_distinct_table_rows() {
        // Every row is one entry; the names are documentation of the
        // provenance (docs/sources.md).
        for index in 0..=23 {
            assert!(palette(index).is_some());
        }
    }

    // --- theme role and slot tables -----------------------------------

    #[test]
    fn theme_roles_cover_the_four_quick_style_cells() {
        assert_eq!(ThemeRole::of_cell("LineColor"), Some(ThemeRole::Line));
        assert_eq!(
            ThemeRole::of_cell("LineGradientStopColor"),
            Some(ThemeRole::Line)
        );
        assert_eq!(ThemeRole::of_cell("FillForegnd"), Some(ThemeRole::Fill));
        assert_eq!(ThemeRole::of_cell("FillBkgnd"), Some(ThemeRole::Fill));
        assert_eq!(
            ThemeRole::of_cell("GradientStopColor"),
            Some(ThemeRole::Fill)
        );
        assert_eq!(
            ThemeRole::of_cell("ShapeShdwColor"),
            Some(ThemeRole::Shadow)
        );
        assert_eq!(ThemeRole::of_cell("GlowColor"), Some(ThemeRole::Shadow));
        assert_eq!(
            ThemeRole::of_cell("BevelDepthColor"),
            Some(ThemeRole::Shadow)
        );
        assert_eq!(ThemeRole::of_cell("Color"), Some(ThemeRole::Font));
        assert_eq!(ThemeRole::of_cell("PinX"), None);
        assert_eq!(
            ThemeRole::quick_style_cell(ThemeRole::Line),
            "QuickStyleLineColor"
        );
        assert_eq!(
            ThemeRole::quick_style_cell(ThemeRole::Fill),
            "QuickStyleFillColor"
        );
        assert_eq!(
            ThemeRole::quick_style_cell(ThemeRole::Shadow),
            "QuickStyleShadowColor"
        );
        assert_eq!(
            ThemeRole::quick_style_cell(ThemeRole::Font),
            "QuickStyleFontColor"
        );
    }

    #[test]
    fn theme_slots_map_the_documented_quick_style_values() {
        assert_eq!(theme_slot(0), Some(ThemeSlot::Dk1));
        assert_eq!(theme_slot(1), Some(ThemeSlot::Lt1));
        assert_eq!(theme_slot(2), Some(ThemeSlot::Accent(0)));
        assert_eq!(theme_slot(7), Some(ThemeSlot::Accent(5)));
        // 8 is the theme's background colour ([MS-VSDX] `QuickStyleFillColor`).
        assert_eq!(theme_slot(8), Some(ThemeSlot::Background));
        // 100..106 are the variation colours 1..7 ([MS-VSDX],
        // `VisQuickStyleColors` "Color variant 1..7").
        assert_eq!(theme_slot(100), Some(ThemeSlot::Variation(0)));
        assert_eq!(theme_slot(106), Some(ThemeSlot::Variation(6)));
        // 200..206 have no published meaning; the corpus shows the same
        // variation colours (ADR 0021 amendment).
        assert_eq!(theme_slot(200), Some(ThemeSlot::Variation(0)));
        assert_eq!(theme_slot(206), Some(ThemeSlot::Variation(6)));
        // Everything else has no published meaning.
        assert_eq!(theme_slot(9), None);
        assert_eq!(theme_slot(99), None);
        assert_eq!(theme_slot(107), None);
        assert_eq!(theme_slot(199), None);
        assert_eq!(theme_slot(207), None);
    }

    #[test]
    fn text_substitutes_empty_on_errors_and_missing_values() {
        let (doc, shape) =
            document_with_cells(vec![cell("Bad", CellValue::Error("#VALUE!".to_owned()))]);
        let mut reader = CellReader::new(&doc);
        let sheet = SheetRef::Shape(shape);
        assert_eq!(
            reader.text(sheet, &CellName::from_static("Bad")),
            Ok(String::new())
        );
        assert_eq!(
            reader.warnings()[0].kind,
            WarningKind::ErrorValue("#VALUE!".to_owned())
        );
        assert_eq!(
            reader.text(sheet, &CellName::from_static("Gone")),
            Ok(String::new())
        );
        assert_eq!(reader.warnings()[1].kind, WarningKind::Missing);
    }

    #[test]
    fn color_substitutes_black_on_a_missing_value() {
        let (doc, shape) = document_with_shape();
        let mut reader = CellReader::new(&doc);
        assert_eq!(
            reader.color(SheetRef::Shape(shape), &CellName::from_static("LineColor")),
            Ok(Color::new(0, 0, 0))
        );
        assert_eq!(reader.warnings()[0].kind, WarningKind::Missing);
    }

    #[test]
    fn page_sheet_cells_read_through_the_page_lookup() {
        let mut doc = Document::new();
        let mut page = crate::page::Page::new(PageId { index: 0 }, 1);
        page.sheet
            .cells
            .push(cell("PageWidth", CellValue::Number(11.0)));
        doc.pages.push(page);
        let mut reader = CellReader::new(&doc);
        assert_eq!(
            reader.number(
                SheetRef::Page(PageId { index: 0 }),
                &CellName::from_static("PageWidth")
            ),
            Ok(11.0)
        );
        assert_eq!(
            reader.text(
                SheetRef::Page(PageId { index: 0 }),
                &CellName::from_static("PageWidth")
            ),
            Ok(String::new())
        );
        assert_eq!(
            reader.list(
                SheetRef::Page(PageId { index: 0 }),
                &CellName::from_static("PageWidth")
            ),
            Ok(vec![11.0])
        );
        assert_eq!(reader.warnings()[0].kind, WarningKind::KindMismatch);
    }

    #[test]
    fn a_themed_color_with_an_unusable_quick_style_value_does_not_resolve() {
        // A QuickStyle slot that is not a whole number, or beyond u32.
        let (mut doc, shape) = document_with_cells(vec![
            cell("LineColor", CellValue::Str("Themed".to_owned())),
            cell("QuickStyleLineColor", CellValue::Number(2.5)),
            cell("FillForegnd", CellValue::Str("Themed".to_owned())),
            cell("QuickStyleFillColor", CellValue::Number(1.0e30)),
        ]);
        with_theme(&mut doc);
        let mut reader = CellReader::new(&doc);
        let sheet = SheetRef::Shape(shape);
        assert_eq!(
            reader.color(sheet, &CellName::from_static("LineColor")),
            Ok(Color::new(0, 0, 0))
        );
        assert_eq!(
            reader.color(sheet, &CellName::from_static("FillForegnd")),
            Ok(Color::new(0, 0, 0))
        );
        assert_eq!(reader.warnings().len(), 2);
        for warning in reader.warnings() {
            assert_eq!(warning.kind, WarningKind::UnresolvedTheme);
        }
    }

    /// A theme with two variation schemes (`varColor1`..`varColor7` at
    /// `scheme * 100 + position + 1`) and a background colour.
    fn theme_with_variations() -> Theme {
        let mut theme = Theme::new("Test");
        theme.colors = Some(colors());
        theme.variations = vec![
            ThemeVariation {
                colors: [
                    Color::new(1, 0, 0),
                    Color::new(2, 0, 0),
                    Color::new(3, 0, 0),
                    Color::new(4, 0, 0),
                    Color::new(5, 0, 0),
                    Color::new(6, 0, 0),
                    Color::new(7, 0, 0),
                ],
            },
            ThemeVariation {
                colors: [
                    Color::new(101, 0, 0),
                    Color::new(102, 0, 0),
                    Color::new(103, 0, 0),
                    Color::new(104, 0, 0),
                    Color::new(105, 0, 0),
                    Color::new(106, 0, 0),
                    Color::new(107, 0, 0),
                ],
            },
        ];
        theme.background = Some(Color::new(9, 9, 9));
        theme
    }

    #[test]
    fn a_themed_colour_resolves_the_variation_slot() {
        // `QuickStyleFillColor` 102 names `varColor3` of the active theme
        // variation ([MS-VSDX]); 202 names the same slot (corpus-derived).
        for slot in [102.0, 202.0] {
            let (mut doc, shape) = document_with_cells(vec![
                cell("QuickStyleFillColor", CellValue::Number(slot)),
                cell("FillForegnd", CellValue::Str("Themed".to_owned())),
            ]);
            doc.themes.push(theme_with_variations());
            let mut reader = CellReader::new(&doc);
            let color = reader
                .color(
                    SheetRef::Shape(shape),
                    &CellName::from_static("FillForegnd"),
                )
                .expect("resolves");
            assert_eq!(color, Color::new(3, 0, 0), "{slot}");
            assert!(reader.warnings().is_empty(), "{:?}", reader.warnings());
        }
    }

    #[test]
    fn the_background_slot_reads_the_theme_background_colour() {
        let (mut doc, shape) = document_with_cells(vec![
            cell("QuickStyleLineColor", CellValue::Number(8.0)),
            cell("LineColor", CellValue::Str("Themed".to_owned())),
        ]);
        doc.themes.push(theme_with_variations());
        let mut reader = CellReader::new(&doc);
        let color = reader
            .color(SheetRef::Shape(shape), &CellName::from_static("LineColor"))
            .expect("resolves");
        assert_eq!(color, Color::new(9, 9, 9));
        assert!(reader.warnings().is_empty(), "{:?}", reader.warnings());
    }

    #[test]
    fn the_variation_scheme_follows_the_variation_color_index() {
        // The shape's own `VariationColorIndex` selects the scheme.
        let (mut doc, shape) = document_with_cells(vec![
            cell("VariationColorIndex", CellValue::Number(1.0)),
            cell("QuickStyleFillColor", CellValue::Number(100.0)),
            cell("FillForegnd", CellValue::Str("Themed".to_owned())),
        ]);
        doc.themes.push(theme_with_variations());
        let mut reader = CellReader::new(&doc);
        let color = reader
            .color(
                SheetRef::Shape(shape),
                &CellName::from_static("FillForegnd"),
            )
            .expect("resolves");
        assert_eq!(color, Color::new(101, 0, 0));

        // A missing cell and the value 65534 defer to the page sheet's
        // cell ([MS-VSDX]); a page cell of 65534 and anything unusable
        // fall back to variation 0.
        for (own, on_page, want) in [
            (None, Some(1.0), Color::new(101, 0, 0)),
            (None, Some(0.0), Color::new(1, 0, 0)),
            (Some(65534.0), Some(1.0), Color::new(101, 0, 0)),
            (Some(65534.0), Some(65534.0), Color::new(1, 0, 0)),
            (Some(65534.0), Some(2.5), Color::new(1, 0, 0)),
            (Some(2.5), None, Color::new(1, 0, 0)),
        ] {
            let mut cells = vec![
                cell("QuickStyleFillColor", CellValue::Number(100.0)),
                cell("FillForegnd", CellValue::Str("Themed".to_owned())),
            ];
            if let Some(own) = own {
                cells.push(cell("VariationColorIndex", CellValue::Number(own)));
            }
            let (mut doc, shape) = document_with_cells(cells);
            let mut page_entity = crate::page::Page::new(crate::id::PageId { index: 0 }, 1);
            if let Some(on_page) = on_page {
                page_entity
                    .sheet
                    .cells
                    .push(cell("VariationColorIndex", CellValue::Number(on_page)));
            }
            page_entity.shapes.push(shape);
            doc.pages.push(page_entity);
            doc.themes.push(theme_with_variations());
            let mut reader = CellReader::new(&doc);
            let color = reader
                .color(
                    SheetRef::Shape(shape),
                    &CellName::from_static("FillForegnd"),
                )
                .expect("resolves");
            assert_eq!(color, want, "{own:?} {on_page:?}");
        }
    }

    #[test]
    fn a_page_sheet_resolves_the_theme_colours_too() {
        let mut doc = Document::new();
        let mut page = crate::page::Page::new(crate::id::PageId { index: 0 }, 1);
        page.sheet.cells = vec![
            cell("QuickStyleFillColor", CellValue::Number(201.0)),
            cell("FillForegnd", CellValue::Str("Themed".to_owned())),
        ];
        doc.pages.push(page);
        doc.themes.push(theme_with_variations());
        let mut reader = CellReader::new(&doc);
        let color = reader
            .color(
                SheetRef::Page(crate::id::PageId { index: 0 }),
                &CellName::from_static("FillForegnd"),
            )
            .expect("resolves");
        assert_eq!(color, Color::new(2, 0, 0));
        assert!(reader.warnings().is_empty(), "{:?}", reader.warnings());
    }

    #[test]
    fn documented_defaults_replace_the_missing_warning() {
        // Literal defaults: no warning, the documented value.
        let (doc, shape) = document_with_shape();
        let mut reader = CellReader::new(&doc);
        let sheet = SheetRef::Shape(shape);
        assert_eq!(
            reader.number(sheet, &CellName::from_static("LeftMargin")),
            Ok(0.1)
        );
        assert_eq!(
            reader.number(sheet, &CellName::from_static("TopMargin")),
            Ok(4.0 / 72.0)
        );
        assert!(reader.warnings().is_empty(), "{:?}", reader.warnings());

        // A formula default: `LocPinX = Width*0.5` with `Width` defined.
        let (doc, shape) = document_with_cells(vec![cell("Width", CellValue::Number(2.0))]);
        let mut reader = CellReader::new(&doc);
        assert_eq!(
            reader.number(SheetRef::Shape(shape), &CellName::from_static("LocPinX")),
            Ok(1.0)
        );
        // `TxtWidth = Width*1`.
        assert_eq!(
            reader.number(SheetRef::Shape(shape), &CellName::from_static("TxtWidth")),
            Ok(2.0)
        );
        assert!(reader.warnings().is_empty(), "{:?}", reader.warnings());

        // `Width` absent (or of another kind): the formula default is 0,
        // quietly; `Width` itself warns only when it is read.
        let (doc, shape) = document_with_cells(vec![cell("Width", CellValue::Str("x".to_owned()))]);
        let mut reader = CellReader::new(&doc);
        assert_eq!(
            reader.number(SheetRef::Shape(shape), &CellName::from_static("LocPinX")),
            Ok(0.0)
        );
        assert!(reader.warnings().is_empty(), "{:?}", reader.warnings());
    }

    #[test]
    fn a_non_finite_related_value_does_not_leak_into_a_documented_default() {
        // `LocPinX = Width*0.5`: a non-finite cached `Width` is not
        // passed on (the number policy rejects it), the default is 0.
        let (doc, shape) =
            document_with_cells(vec![cell("Width", CellValue::Number(f64::INFINITY))]);
        let mut reader = CellReader::new(&doc);
        assert_eq!(
            reader.number(SheetRef::Shape(shape), &CellName::from_static("LocPinX")),
            Ok(0.0)
        );
    }

    #[test]
    fn a_documented_default_does_not_hide_a_malformed_cell() {
        // The cell is defined but caches no value: it is not "absent at
        // every rung", so it still warns (ADR 0021 amendment).
        let (mut doc, shape) = document_with_shape();
        let mut only_formula = Cell::new(CellName::from_static("LocPinX"));
        only_formula.formula = Some("Width*0.5".to_owned());
        if let Some(shape) = doc.shape_mut(shape) {
            shape.sheet.cells.push(only_formula);
        }
        let mut reader = CellReader::new(&doc);
        assert_eq!(
            reader.number(SheetRef::Shape(shape), &CellName::from_static("LocPinX")),
            Ok(0.0)
        );
        assert_eq!(reader.warnings().len(), 1);
        assert_eq!(reader.warnings()[0].kind, WarningKind::Missing);
    }

    /// A theme with three fill styles, three line styles and one
    /// variation style scheme.
    fn theme_with_fills() -> Theme {
        let mut theme = theme_with_variations();
        theme.fill_styles = vec![
            ThemeFillStyle {
                kind: ThemeFillKind::Solid,
                alpha: Some(50_000),
            },
            ThemeFillStyle {
                kind: ThemeFillKind::Gradient,
                alpha: None,
            },
            ThemeFillStyle {
                kind: ThemeFillKind::None,
                alpha: Some(100_000),
            },
        ];
        theme.line_styles = vec![
            ThemeLineStyle {
                kind: ThemeLineKind::Solid,
                alpha: Some(100_000),
                width_emu: Some(12_700),
            },
            ThemeLineStyle {
                kind: ThemeLineKind::Dashed,
                alpha: None,
                width_emu: None,
            },
            ThemeLineStyle {
                kind: ThemeLineKind::None,
                alpha: None,
                width_emu: Some(25_400),
            },
        ];
        theme.variation_styles = vec![ThemeVariationStyles {
            fill_indices: vec![3, 2, 1],
            line_indices: vec![2, 1, 3],
        }];
        theme
    }

    #[test]
    fn a_themed_fill_pattern_resolves_through_the_fill_style_matrix() {
        // `QuickStyleFillMatrix` 1..6 names the `a:fmtScheme` fill styles
        // in order ([MS-VSDX]): 1 solid -> FillPattern 1, 2 gradient ->
        // 25 (the renderer's gradient range), 3 no fill -> 0.
        for (matrix, want) in [(1.0, 1.0), (2.0, 25.0), (3.0, 0.0)] {
            let (mut doc, shape) = document_with_cells(vec![
                cell("QuickStyleFillMatrix", CellValue::Number(matrix)),
                cell("FillPattern", CellValue::Str("Themed".to_owned())),
            ]);
            doc.themes.push(theme_with_fills());
            let mut reader = CellReader::new(&doc);
            assert_eq!(
                reader.number(
                    SheetRef::Shape(shape),
                    &CellName::from_static("FillPattern")
                ),
                Ok(want),
                "{matrix}"
            );
            assert!(reader.warnings().is_empty(), "{:?}", reader.warnings());
        }
    }

    #[test]
    fn a_themed_fill_pattern_resolves_through_the_variation_style_matrix() {
        // `QuickStyleFillMatrix` 100..103 names the `fillIdx` of the
        // first..fourth `vt:varStyle` of the variation style scheme
        // ([MS-VSDX]); the `fillIdx` values are 1-based.
        for (matrix, want) in [(100.0, 0.0), (101.0, 25.0), (102.0, 1.0)] {
            let (mut doc, shape) = document_with_cells(vec![
                cell("QuickStyleFillMatrix", CellValue::Number(matrix)),
                cell("FillPattern", CellValue::Str("Themed".to_owned())),
            ]);
            doc.themes.push(theme_with_fills());
            let mut reader = CellReader::new(&doc);
            assert_eq!(
                reader.number(
                    SheetRef::Shape(shape),
                    &CellName::from_static("FillPattern")
                ),
                Ok(want),
                "{matrix}"
            );
            assert!(reader.warnings().is_empty(), "{:?}", reader.warnings());
        }
    }

    /// The `QuickStyle*Matrix` cell of a themed pattern cell.
    fn matrix_cell(pattern: &str) -> &'static str {
        match pattern {
            "FillPattern" => "QuickStyleFillMatrix",
            _ => "QuickStyleLineMatrix",
        }
    }

    #[test]
    fn a_themed_pattern_falls_back_to_the_visible_value_with_a_warning() {
        // Matrix 0 names the root style sheet (not the theme), 7 has no
        // published meaning, `fillIdx` 0 is out of range and a dashed line
        // names no Visio pattern index: the fallback is the *visible*
        // value (1) with `UnresolvedTheme`, never 0 (ADR 0021 amendment).
        for (matrix, name) in [
            (0.0, "FillPattern"),
            (7.0, "FillPattern"),
            (103.0, "FillPattern"),
            (100.0, "FillPattern"),
            (0.0, "LinePattern"),
            (2.0, "LinePattern"),
            (7.0, "LinePattern"),
        ] {
            let (mut doc, shape) = document_with_cells(vec![
                Cell::new(CellName::new(matrix_cell(name))),
                cell(name, CellValue::Str("Themed".to_owned())),
            ]);
            let mut theme = theme_with_fills();
            if matrix == 100.0 {
                theme.variation_styles[0].fill_indices = vec![0];
            }
            if matrix == 103.0 {
                theme.variation_styles[0].fill_indices = vec![1, 1, 1];
            }
            doc.themes.push(theme);
            let mut reader = CellReader::new(&doc);
            assert_eq!(
                reader.number(SheetRef::Shape(shape), &CellName::new(name)),
                Ok(1.0),
                "{name} {matrix}"
            );
            assert_eq!(
                reader.warnings()[0].kind,
                WarningKind::UnresolvedTheme,
                "{name} {matrix}"
            );
        }
    }

    #[test]
    fn a_themed_shadow_pattern_falls_back_to_no_shadow_with_a_warning() {
        let (mut doc, shape) = document_with_cells(vec![cell(
            "ShdwPattern",
            CellValue::Str("Themed".to_owned()),
        )]);
        doc.themes.push(theme_with_fills());
        let mut reader = CellReader::new(&doc);
        assert_eq!(
            reader.number(
                SheetRef::Shape(shape),
                &CellName::from_static("ShdwPattern")
            ),
            Ok(0.0)
        );
        assert_eq!(reader.warnings()[0].kind, WarningKind::UnresolvedTheme);
    }

    #[test]
    fn a_themed_line_pattern_resolves_through_the_line_style_matrix() {
        // `QuickStyleLineMatrix` 1..3 name `lnStyleLst` entries 1..3:
        // solid (1), dashed (no published index: the fallback 1 with a
        // warning) and no line (0).
        for (matrix, want, warns) in [(1.0, 1.0, false), (2.0, 1.0, true), (3.0, 0.0, false)] {
            let (mut doc, shape) = document_with_cells(vec![
                cell("QuickStyleLineMatrix", CellValue::Number(matrix)),
                cell("LinePattern", CellValue::Str("Themed".to_owned())),
            ]);
            doc.themes.push(theme_with_fills());
            let mut reader = CellReader::new(&doc);
            assert_eq!(
                reader.number(
                    SheetRef::Shape(shape),
                    &CellName::from_static("LinePattern")
                ),
                Ok(want),
                "{matrix}"
            );
            assert_eq!(!reader.warnings().is_empty(), warns, "{matrix}");
        }
    }

    #[test]
    fn a_themed_line_weight_resolves_from_the_line_width() {
        // `a:ln/@w` is EMU (1/914400 inch): 12700 EMU is 1 point. Without
        // a width the fallback is 0.7 point with `UnresolvedTheme`.
        let (mut doc, shape) = document_with_cells(vec![
            cell("QuickStyleLineMatrix", CellValue::Number(1.0)),
            cell("LineWeight", CellValue::Str("Themed".to_owned())),
        ]);
        doc.themes.push(theme_with_fills());
        let mut reader = CellReader::new(&doc);
        assert_eq!(
            reader.number(SheetRef::Shape(shape), &CellName::from_static("LineWeight")),
            Ok(12_700.0 / 914_400.0)
        );
        assert!(reader.warnings().is_empty(), "{:?}", reader.warnings());

        let (mut doc, shape) = document_with_cells(vec![
            cell("QuickStyleLineMatrix", CellValue::Number(2.0)),
            cell("LineWeight", CellValue::Str("Themed".to_owned())),
        ]);
        doc.themes.push(theme_with_fills());
        let mut reader = CellReader::new(&doc);
        assert_eq!(
            reader.number(SheetRef::Shape(shape), &CellName::from_static("LineWeight")),
            Ok(0.7 / 72.0)
        );
        assert_eq!(reader.warnings()[0].kind, WarningKind::UnresolvedTheme);
    }

    #[test]
    fn themed_transparencies_resolve_from_the_style_alpha() {
        // `a:alpha` is opacity: 50000 is 50% transparent. No alpha is the
        // DrawingML default, opaque.
        let (mut doc, shape) = document_with_cells(vec![
            cell("QuickStyleFillMatrix", CellValue::Number(1.0)),
            cell("FillForegndTrans", CellValue::Str("Themed".to_owned())),
        ]);
        doc.themes.push(theme_with_fills());
        let mut reader = CellReader::new(&doc);
        assert_eq!(
            reader.number(
                SheetRef::Shape(shape),
                &CellName::from_static("FillForegndTrans")
            ),
            Ok(0.5)
        );
        assert!(reader.warnings().is_empty(), "{:?}", reader.warnings());

        let (mut doc, shape) = document_with_cells(vec![
            cell("QuickStyleFillMatrix", CellValue::Number(3.0)),
            cell("FillBkgndTrans", CellValue::Str("Themed".to_owned())),
        ]);
        doc.themes.push(theme_with_fills());
        let mut reader = CellReader::new(&doc);
        assert_eq!(
            reader.number(
                SheetRef::Shape(shape),
                &CellName::from_static("FillBkgndTrans")
            ),
            Ok(0.0)
        );
        assert!(reader.warnings().is_empty(), "{:?}", reader.warnings());

        let (mut doc, shape) = document_with_cells(vec![
            cell("QuickStyleLineMatrix", CellValue::Number(1.0)),
            cell("LineColorTrans", CellValue::Str("Themed".to_owned())),
        ]);
        doc.themes.push(theme_with_fills());
        let mut reader = CellReader::new(&doc);
        assert_eq!(
            reader.number(
                SheetRef::Shape(shape),
                &CellName::from_static("LineColorTrans")
            ),
            Ok(0.0)
        );
        assert!(reader.warnings().is_empty(), "{:?}", reader.warnings());
    }

    /// A cell whose formula is the `Inh` inherit marker, with the cached
    /// snapshot of the value for this sheet (ADR 0054 amendment).
    fn inherit_cell(name: &str, snapshot: f64) -> Cell {
        let mut cell = Cell::new(CellName::new(name));
        cell.formula = Some("Inh".to_owned());
        cell.value = Some(CellValue::Number(snapshot));
        cell
    }

    #[test]
    fn an_inh_snapshot_is_the_value_and_the_master_stays_the_source() {
        // Acceptance (P6-05, ADR 0054 amendment): a cell with `F='Inh'`
        // and a cached value returns that value while `Document::resolve`
        // still reports `Master(..)` as the source.
        let mut doc = Document::new();
        let content = doc.add_shape(Shape::new(ShapeId { index: 0 }, 1));
        doc.shape_mut(content)
            .expect("content")
            .sheet
            .cells
            .push(cell("PinX", CellValue::Number(0.98)));
        let mut master = crate::master::Master::new(crate::id::MasterId { index: 0 }, 1);
        master.shapes.push(content);
        doc.masters.push(master);
        let shape = doc.add_shape(Shape::new(ShapeId { index: 0 }, 2));
        {
            let entity = doc.shape_mut(shape).expect("shape");
            entity.master = Some(crate::id::MasterId { index: 0 });
            entity.sheet.cells.push(inherit_cell("PinX", 2.16));
        }

        let mut reader = CellReader::new(&doc);
        assert_eq!(
            reader
                .number(SheetRef::Shape(shape), &CellName::from_static("PinX"))
                .expect("reads"),
            2.16
        );
        assert!(reader.warnings().is_empty(), "{:?}", reader.warnings());

        // The lookup's own answer is unchanged: the master defines the
        // formula, so it is the source (its value is the fallback).
        let resolved = doc
            .resolve(shape, &CellName::from_static("PinX"))
            .expect("resolves");
        assert_eq!(
            resolved.source,
            ValueSource::Master(crate::id::MasterId { index: 0 })
        );
        assert_eq!(resolved.value, Some(&CellValue::Number(0.98)));
    }

    #[test]
    fn an_inh_snapshot_without_any_rung_does_not_warn() {
        // The snapshot is a value now (ADR 0057): nothing else defines
        // the cell and the reader still records no `Missing` warning.
        let (doc, shape) = document_with_cells(vec![inherit_cell("LineWeight", 0.02)]);
        let mut reader = CellReader::new(&doc);
        assert_eq!(
            reader
                .number(SheetRef::Shape(shape), &CellName::from_static("LineWeight"))
                .expect("reads"),
            0.02
        );
        assert!(reader.warnings().is_empty(), "{:?}", reader.warnings());
    }

    #[test]
    fn a_section_level_inh_snapshot_is_the_value() {
        // The same rule for a section-level cell (the `NoFill` family).
        let mut doc = Document::new();
        let content = doc.add_shape(Shape::new(ShapeId { index: 0 }, 1));
        doc.shape_mut(content)
            .expect("content")
            .sheet
            .sections
            .push(crate::section::Section::new(
                crate::name::SectionName::from_static("Geometry"),
            ));
        doc.shape_mut(content)
            .expect("content")
            .sheet
            .sections
            .get_mut(0)
            .expect("section")
            .cells
            .push({
                let mut cell = Cell::new(CellName::from_static("NoFill"));
                cell.value = Some(CellValue::Bool(false));
                cell.formula = Some("Inh".to_owned());
                cell
            });
        let mut master = crate::master::Master::new(crate::id::MasterId { index: 0 }, 1);
        master.shapes.push(content);
        doc.masters.push(master);
        let shape = doc.add_shape(Shape::new(ShapeId { index: 0 }, 2));
        {
            let entity = doc.shape_mut(shape).expect("shape");
            entity.master = Some(crate::id::MasterId { index: 0 });
            let mut section =
                crate::section::Section::new(crate::name::SectionName::from_static("Geometry"));
            let mut cell = Cell::new(CellName::from_static("NoFill"));
            cell.formula = Some("Inh".to_owned());
            cell.value = Some(CellValue::Bool(false));
            section.cells.push(cell);
            entity.sheet.sections.push(section);
        }

        let mut reader = CellReader::new(&doc);
        assert!(
            !reader
                .boolean(SheetRef::Shape(shape), &CellName::from_static("NoFill"))
                .expect("reads")
        );
        assert!(reader.warnings().is_empty(), "{:?}", reader.warnings());
    }

    #[test]
    fn a_row_snapshot_is_the_value() {
        // A geometry row's own `Inh` snapshot is the value (the P3-15
        // gap): the row is matched by position here.
        let (mut doc, shape) = document_with_shape();
        let mut section =
            crate::section::Section::new(crate::name::SectionName::from_static("Geometry"));
        let mut row = crate::section::Row::new();
        row.cells.push(inherit_cell("X", 2.16));
        section.rows.push(row);
        doc.shape_mut(shape)
            .expect("shape")
            .sheet
            .sections
            .push(section);
        let mut reader = CellReader::new(&doc);
        assert_eq!(
            reader
                .row_number(
                    SheetRef::Shape(shape),
                    "Geometry",
                    RowTarget::at(0),
                    &CellName::from_static("X")
                )
                .expect("reads"),
            2.16
        );
    }

    #[test]
    fn a_row_ix_matches_the_row_that_carries_it() {
        // `RowTarget::by_ix` matches the row with that `IX`, and a target
        // naming both falls back to the position when no row carries the
        // `IX`.
        let mut doc = Document::new();
        let shape = doc.add_shape(Shape::new(ShapeId { index: 0 }, 1));
        let mut section =
            crate::section::Section::new(crate::name::SectionName::from_static("Geometry"));
        let mut first = crate::section::Row::new();
        first.cells.push(inherit_cell("X", 1.5));
        let mut second = crate::section::Row::new();
        second.ix = Some(2);
        second.cells.push(inherit_cell("X", 2.5));
        section.rows.push(first);
        section.rows.push(second);
        doc.shape_mut(shape)
            .expect("shape")
            .sheet
            .sections
            .push(section);

        let mut reader = CellReader::new(&doc);
        let sheet = SheetRef::Shape(shape);
        let cell = CellName::from_static("X");
        assert_eq!(
            reader
                .row_number(sheet, "Geometry", RowTarget::by_ix(2), &cell)
                .expect("reads"),
            2.5
        );
        assert_eq!(
            reader
                .row_number(
                    sheet,
                    "Geometry",
                    RowTarget {
                        position: Some(0),
                        ix: Some(2)
                    },
                    &cell
                )
                .expect("reads"),
            2.5
        );
        // An `IX` no row carries falls back to the position when that
        // row has no `IX` of its own.
        assert_eq!(
            reader
                .row_number(
                    sheet,
                    "Geometry",
                    RowTarget {
                        position: Some(0),
                        ix: Some(7)
                    },
                    &cell
                )
                .expect("reads"),
            1.5
        );
    }
}
