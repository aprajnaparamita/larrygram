//! Connection points inherited from masters (ADR 0013, ADR 0043).
//!
//! A shape's `Connection` section rows live in the shape's local
//! coordinates. A master instance usually has no rows of its own and
//! inherits the section from its master; a resized instance must keep the
//! inherited points on its own outline, so the master's coordinates are
//! scaled by the instance's size over the master shape's size.
//!
//! The walk follows the master chain of [`Document::master_chain_shapes`]
//! and is bounded like the lookup (ADR 0013): a cyclic chain stops at the
//! first repeat, and at most a fixed rung cap (128) are visited.

use larrygram_geom::{Point, Unit};

use crate::cell::Cell;
use crate::cell::CellValue;
use crate::document::Document;
use crate::error::Error;
use crate::id::ShapeId;
use crate::name::{CellName, SectionName};
use crate::section::{Row, Section};
use crate::shape::Shape;

/// The maximum number of master rungs a connection-point walk visits.
///
/// The lookup cuts a cyclic chain through its visited set; this cap is the
/// same belt-and-braces bound the geometry rewrite uses.
const MAX_CHAIN: usize = 128;

/// The section a shape's connection points live in.
const CONNECTION_SECTION: &str = "Connection";

impl Document {
    /// The connection points of `shape`, in the shape's local coordinates
    /// (inches).
    ///
    /// The shape's own `Connection` rows win. A shape without rows
    /// inherits them through its master chain: the nearest master shape
    /// that has rows defines the points. Each point is scaled by the
    /// instance's `Width`/`Height` over that master shape's
    /// `Width`/`Height`, so a resized instance keeps its points on its
    /// outline. A zero or missing master size (or instance size) means no
    /// scaling on that axis.
    ///
    /// Rows with a non-finite or missing `X`/`Y` value are skipped. A
    /// stale shape id or a chain without rows yields an empty list. This
    /// function never panics.
    pub fn connection_points(&self, shape: ShapeId) -> Vec<Point> {
        self.connection_points_indexed(shape)
            .into_iter()
            .map(|(_, point)| point)
            .collect()
    }

    /// The connection points of `shape` with their `Connection` row
    /// indexes (`Row/@IX`), in file order.
    ///
    /// A malformed row (missing or non-finite `X`/`Y`) is skipped
    /// without renumbering the rows around it, so a glue record that
    /// names `IX` addresses the same row the file does (task P5-17,
    /// Goal 4). A row without an `IX` is named by its position in the
    /// section, which matches the readers that assign one.
    pub fn connection_points_indexed(&self, shape: ShapeId) -> Vec<(i32, Point)> {
        let Some(entity) = self.shape(shape) else {
            return Vec::new();
        };
        let own = connection_rows(&entity.sheet.sections);
        if !own.is_empty() {
            return indexed_rows(own);
        }

        for master_id in self.master_chain_shapes(shape, MAX_CHAIN) {
            let Some(master) = self.shape(master_id) else {
                continue;
            };
            let rows = connection_rows(&master.sheet.sections);
            if rows.is_empty() {
                continue;
            }
            let (scale_x, scale_y) = self.connection_scale(entity, master);
            return indexed_rows_scaled(rows, scale_x, scale_y);
        }
        Vec::new()
    }

    /// The row index [`Document::add_connection_point`] gives the next
    /// point: one past the largest effective index, or 0 when the shape
    /// has no points yet (task P7-34 item 2: the tool predicts the new
    /// point's index to select it).
    ///
    /// The answer counts the shape's own rows and, when it has none,
    /// the inherited ones, with the same effective index rule as
    /// [`Document::connection_points_indexed`].
    pub fn next_connection_point_index(&self, shape: ShapeId) -> u32 {
        self.connection_points_indexed(shape)
            .into_iter()
            .map(|(ix, _)| u32::try_from(ix).unwrap_or(0))
            .fold(None, |max: Option<u32>, ix| {
                Some(max.map_or(ix, |m| m.max(ix)))
            })
            .map_or(0, |max| max.saturating_add(1))
    }

    /// Adds a connection point at the local point `local` (inches) and
    /// returns its row index (task P7-34 item 5).
    ///
    /// A first edit of an instance's inherited points copies them into
    /// the shape's own `Connection` section first (instance rows win,
    /// ADR 0013), with the indices unchanged, so existing glue keeps
    /// its target. The new row is a plain row with `X`/`Y` literals and
    /// the documented default type (inward). A stale shape id or a
    /// non-finite point is an error; this function never panics.
    pub fn add_connection_point(&mut self, shape: ShapeId, local: Point) -> Result<u32, Error> {
        if !local.x.is_finite() || !local.y.is_finite() {
            return Err(Error::BadCell("non-finite connection point"));
        }
        self.materialise_connection_rows(shape)?;
        let ix = self.next_connection_point_index(shape);
        let row_ix =
            i32::try_from(ix).map_err(|_| Error::BadCell("connection row index too large"))?;
        let mut row = Row::new();
        row.row_type = Some(CONNECTION_SECTION.to_owned());
        row.ix = Some(row_ix);
        row.cells = vec![number_cell("X", local.x), number_cell("Y", local.y)];
        let target = self.shape_mut(shape).ok_or(Error::UnknownShape(shape))?;
        match target
            .sheet
            .sections
            .iter_mut()
            .find(|section| section.name.as_str() == CONNECTION_SECTION)
        {
            Some(section) => section.rows.push(row),
            None => {
                let mut section = Section::new(SectionName::from_static(CONNECTION_SECTION));
                section.rows.push(row);
                target.sheet.sections.push(section);
            }
        }
        Ok(ix)
    }

    /// Moves row `ix` to the local point `local` (inches), keeping its
    /// `Type`, `DirX`, `DirY` and row name (task P7-34 item 2).
    ///
    /// A first edit of an instance's inherited points copies them into
    /// the shape's own section first, with the indices unchanged. A
    /// stale shape id or row index, or a non-finite point, is an error.
    pub fn move_connection_point(
        &mut self,
        shape: ShapeId,
        ix: u32,
        local: Point,
    ) -> Result<(), Error> {
        if !local.x.is_finite() || !local.y.is_finite() {
            return Err(Error::BadCell("non-finite connection point"));
        }
        self.materialise_connection_rows(shape)?;
        let (position, section_at) = self.connection_row_slot(shape, ix)?;
        let target = self.shape_mut(shape).ok_or(Error::UnknownShape(shape))?;
        let section = target
            .sheet
            .sections
            .get_mut(section_at)
            .ok_or(Error::BadCell("no such section"))?;
        let row = section
            .rows
            .get_mut(position)
            .ok_or(Error::BadCell("no such row"))?;
        write_row_number(row, "X", local.x);
        write_row_number(row, "Y", local.y);
        Ok(())
    }

    /// Deletes row `ix`; later rows move down by one (task P7-34 item
    /// 4): every row whose index is larger loses one, so the glue
    /// formulas and `Connects` records the caller renumbers keep their
    /// target.
    ///
    /// A first edit of an instance's inherited points copies them into
    /// the shape's own section first, with the indices unchanged. A
    /// stale shape id or row index is an error.
    pub fn delete_connection_point(&mut self, shape: ShapeId, ix: u32) -> Result<(), Error> {
        self.materialise_connection_rows(shape)?;
        let (position, section_at) = self.connection_row_slot(shape, ix)?;
        let deleted = ix
            .try_into()
            .map_err(|_| Error::BadCell("connection row index too large"))?;
        let target = self.shape_mut(shape).ok_or(Error::UnknownShape(shape))?;
        let section = target
            .sheet
            .sections
            .get_mut(section_at)
            .ok_or(Error::BadCell("no such section"))?;
        section.rows.remove(position);
        for section in target
            .sheet
            .sections
            .iter_mut()
            .filter(|section| section.name.as_str() == CONNECTION_SECTION)
        {
            for row in &mut section.rows {
                if let Some(ix) = row.ix.as_mut()
                    && *ix > deleted
                {
                    *ix -= 1;
                }
            }
        }
        Ok(())
    }

    /// Writes row `ix`'s `Type` (0 inward, 1 outward, 2 both; task
    /// P7-34 item 3).
    ///
    /// A first edit of an instance's inherited points copies them into
    /// the shape's own section first, with the indices unchanged. A
    /// stale shape id or row index, or a kind outside 0..=2, is an
    /// error.
    pub fn set_connection_point_type(
        &mut self,
        shape: ShapeId,
        ix: u32,
        kind: u8,
    ) -> Result<(), Error> {
        if kind > 2 {
            return Err(Error::BadCell("connection point type must be 0, 1 or 2"));
        }
        self.materialise_connection_rows(shape)?;
        let (position, section_at) = self.connection_row_slot(shape, ix)?;
        let target = self.shape_mut(shape).ok_or(Error::UnknownShape(shape))?;
        let section = target
            .sheet
            .sections
            .get_mut(section_at)
            .ok_or(Error::BadCell("no such section"))?;
        let row = section
            .rows
            .get_mut(position)
            .ok_or(Error::BadCell("no such row"))?;
        write_row_number(row, "Type", f64::from(kind));
        Ok(())
    }

    /// The `Type` of row `ix` (0 inward, 1 outward, 2 both; task
    /// P7-34 item 3): the documented default, 0, when the row has no
    /// `Type` cell. A stale shape id or row index answers `None` (the
    /// checked state of the point's type menu).
    pub fn connection_point_type(&self, shape: ShapeId, ix: u32) -> Option<u8> {
        let (position, section_at) = self.connection_row_slot(shape, ix).ok()?;
        let row = self
            .shape(shape)?
            .sheet
            .sections
            .get(section_at)?
            .rows
            .get(position)?;
        match row_number(row, "Type") {
            Some(1.0) => Some(1),
            Some(2.0) => Some(2),
            _ => Some(0),
        }
    }

    /// Copies the inherited `Connection` rows of `shape` into its own
    /// section (task P7-34 item 5), the indices unchanged and the
    /// `X`/`Y` values scaled to the instance's box, so the effective
    /// points do not move. A shape that has own rows keeps them and
    /// nothing is copied (instance rows win, ADR 0013).
    fn materialise_connection_rows(&mut self, shape: ShapeId) -> Result<(), Error> {
        let Some(entity) = self.shape(shape) else {
            return Err(Error::UnknownShape(shape));
        };
        if !connection_rows(&entity.sheet.sections).is_empty() {
            return Ok(());
        }
        let Some(rows) = self.inherited_connection_rows(shape) else {
            return Ok(());
        };
        let target = self.shape_mut(shape).ok_or(Error::UnknownShape(shape))?;
        match target
            .sheet
            .sections
            .iter_mut()
            .find(|section| section.name.as_str() == CONNECTION_SECTION)
        {
            Some(section) => section.rows.extend(rows),
            None => {
                let mut section = Section::new(SectionName::from_static(CONNECTION_SECTION));
                section.rows = rows;
                target.sheet.sections.push(section);
            }
        }
        Ok(())
    }

    /// The inherited `Connection` rows of `shape`, copied for
    /// materialisation: the nearest master shape that has rows defines
    /// them, its `X`/`Y` values scaled by the instance's box (the same
    /// walk and scale as [`Document::connection_points_indexed`]). A
    /// copied `X`/`Y` becomes a literal: the master's formula would
    /// compute the master's box, not the instance's.
    fn inherited_connection_rows(&self, shape: ShapeId) -> Option<Vec<Row>> {
        let entity = self.shape(shape)?;
        for master_id in self.master_chain_shapes(shape, MAX_CHAIN) {
            let master = self.shape(master_id)?;
            let rows = connection_rows(&master.sheet.sections);
            if rows.is_empty() {
                continue;
            }
            let (scale_x, scale_y) = self.connection_scale(entity, master);
            return Some(
                rows.into_iter()
                    .map(|row| materialised_row(row, scale_x, scale_y))
                    .collect(),
            );
        }
        None
    }

    /// The row of effective index `ix` in `shape`'s own `Connection`
    /// rows: its position in its section and the section's position in
    /// the sheet. A stale index is an error.
    fn connection_row_slot(&self, shape: ShapeId, ix: u32) -> Result<(usize, usize), Error> {
        let Some(entity) = self.shape(shape) else {
            return Err(Error::UnknownShape(shape));
        };
        let mut position_all = 0usize;
        for (section_at, section) in entity.sheet.sections.iter().enumerate() {
            if section.name.as_str() != CONNECTION_SECTION {
                continue;
            }
            for (position, row) in section.rows.iter().enumerate() {
                if effective_index(row, position_all) == ix {
                    return Ok((position, section_at));
                }
                position_all = position_all.saturating_add(1);
            }
        }
        Err(Error::BadCell("no such connection point"))
    }

    /// The per-axis scale from a master shape's local box to the
    /// instance's box. A missing or non-finite size, or a zero master
    /// size, leaves that axis unscaled.
    fn connection_scale(&self, instance: &Shape, master: &Shape) -> (f64, f64) {
        (
            axis_scale(
                direct_number(instance, "Width"),
                direct_number(master, "Width"),
            ),
            axis_scale(
                direct_number(instance, "Height"),
                direct_number(master, "Height"),
            ),
        )
    }
}

/// The `Connection` rows of a sheet, across every section of that name.
fn connection_rows(sections: &[Section]) -> Vec<&Row> {
    sections
        .iter()
        .filter(|section| section.name.as_str() == "Connection")
        .flat_map(|section| section.rows.iter())
        .collect()
}

/// The `(IX, point)` pairs of one row list, skipping malformed rows.
fn indexed_rows(rows: Vec<&Row>) -> Vec<(i32, Point)> {
    indexed_rows_scaled(rows, 1.0, 1.0)
}

/// The `(IX, point)` pairs of one row list with per-axis scaling,
/// skipping malformed rows.
fn indexed_rows_scaled(rows: Vec<&Row>, scale_x: f64, scale_y: f64) -> Vec<(i32, Point)> {
    rows.into_iter()
        .enumerate()
        .filter_map(|(position, row)| {
            let point = row_point_scaled(row, scale_x, scale_y)?;
            let ix = row
                .ix
                .filter(|ix| *ix >= 0)
                .unwrap_or_else(|| i32::try_from(position).unwrap_or(i32::MAX));
            Some((ix, point))
        })
        .collect()
}

/// One row's `X`/`Y` scaled by the per-axis factors, or `None` when either
/// value is missing or non-finite.
fn row_point_scaled(row: &Row, scale_x: f64, scale_y: f64) -> Option<Point> {
    let x = row_number(row, "X")?;
    let y = row_number(row, "Y")?;
    let point = Point::new(x * scale_x, y * scale_y);
    if point.x.is_finite() && point.y.is_finite() {
        Some(point)
    } else {
        None
    }
}

/// The cached number of one row cell, or `None`.
fn row_number(row: &Row, name: &str) -> Option<f64> {
    row.cells
        .iter()
        .find(|cell| cell.name.as_str() == name)
        .and_then(|cell| match &cell.value {
            Some(CellValue::Number(value)) => Some(*value),
            _ => None,
        })
}

/// The direct cached number of one shape cell, or `None`.
fn direct_number(shape: &Shape, name: &str) -> Option<f64> {
    shape
        .sheet
        .cells
        .iter()
        .find(|cell| cell.name.as_str() == name)
        .and_then(|cell| match &cell.value {
            Some(CellValue::Number(value)) => Some(*value),
            _ => None,
        })
}

/// One axis' scale factor: the instance size over the master size, or
/// `1.0` when either size is missing, non-finite or the master size is
/// zero.
fn axis_scale(instance: Option<f64>, master: Option<f64>) -> f64 {
    match (instance, master) {
        (Some(instance), Some(master))
            if instance.is_finite() && master.is_finite() && master != 0.0 =>
        {
            instance / master
        }
        _ => 1.0,
    }
}

/// The effective index of a `Connection` row: its `Row/@IX` when it has
/// a non-negative one, else its position in the flattened row list
/// (the rule [`Document::connection_points_indexed`] reads by).
fn effective_index(row: &Row, position: usize) -> u32 {
    row.ix
        .filter(|ix| *ix >= 0)
        .and_then(|ix| u32::try_from(ix).ok())
        .unwrap_or_else(|| u32::try_from(position).unwrap_or(u32::MAX))
}

/// A literal numeric cell (`X`, `Y`, `Type` of a `Connection` row).
fn number_cell(name: &str, value: f64) -> Cell {
    Cell {
        name: CellName::new(name),
        formula: None,
        value: Some(CellValue::Number(value)),
        unit: Unit::Inches,
        preserved: crate::raw::Preserved::new(),
    }
}

/// Writes one numeric cell of a `Connection` row as a literal, creating
/// it when the row has none. The other cells (and the row name) stay.
fn write_row_number(row: &mut Row, name: &str, value: f64) {
    match row.cells.iter_mut().find(|cell| cell.name.as_str() == name) {
        Some(cell) => {
            cell.formula = None;
            cell.value = Some(CellValue::Number(value));
        }
        None => row.cells.push(number_cell(name, value)),
    }
}

/// A master row copied into an instance's own section (task P7-34 item
/// 5): every cell verbatim, `X`/`Y` replaced by the scaled literal so
/// the effective point does not move.
fn materialised_row(row: &Row, scale_x: f64, scale_y: f64) -> Row {
    let mut copy = row.clone();
    for (name, scale) in [("X", scale_x), ("Y", scale_y)] {
        let value = row_number(row, name).map(|value| value * scale);
        if let Some(value) = value
            && value.is_finite()
        {
            write_row_number(&mut copy, name, value);
        }
    }
    copy
}
