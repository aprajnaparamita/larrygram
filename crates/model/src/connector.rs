//! 1-D connectors: the endpoint cells, the glue formulas and the glue
//! triggers (ADR 0084 rules 1 and 2, task P7-31).
//!
//! A connector is a 1-D shape: it has the four endpoint cells
//! `BeginX`/`BeginY`/`EndX`/`EndY`, its own or inherited from its
//! master. Glue is stored the way Visio stores it (`docs/ui/connectors.md`
//! section 8): the endpoint formula (`PAR(PNT(...))` for point glue,
//! `_WALKGLUE(...)` for dynamic glue), the cached `V` (the resolved
//! point in page inches), the end's trigger formula and the
//! `Connects` record, all in one edit. [`Document::write_connector_end`]
//! writes the cells and the trigger and leaves the records to the
//! caller (the commands crate writes them in the same undoable edit).

use larrygram_geom::Point;

use crate::cell::CellValue;
use crate::document::Document;
use crate::error::Error;
use crate::id::ShapeId;
use crate::lookup::{CellLookup, ValueSource};
use crate::mutate::CellWrite;
use crate::name::CellName;
use crate::values::SheetRef;

/// Which end of a 1-D shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectorEnd {
    /// `BeginX`/`BeginY`.
    Begin,
    /// `EndX`/`EndY`.
    End,
}

impl ConnectorEnd {
    /// The endpoint cell names of this end (X and Y).
    fn cells(self) -> (&'static str, &'static str) {
        match self {
            ConnectorEnd::Begin => ("BeginX", "BeginY"),
            ConnectorEnd::End => ("EndX", "EndY"),
        }
    }

    /// The glue trigger cell of this end (`Glue Info`).
    fn trigger(self) -> &'static str {
        match self {
            ConnectorEnd::Begin => "BegTrigger",
            ConnectorEnd::End => "EndTrigger",
        }
    }

    /// The `_WALKGLUE` formula of this end (ADR 0084 rule 2): the end's
    /// own trigger first, then the other end's.
    fn walk_glue(self) -> &'static str {
        match self {
            ConnectorEnd::Begin => "_WALKGLUE(BegTrigger,EndTrigger,WalkPreference)",
            ConnectorEnd::End => "_WALKGLUE(EndTrigger,BegTrigger,WalkPreference)",
        }
    }
}

/// What one connector end is glued to, as ADR 0084 rule 2 stores it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndGlue {
    /// Point glue to the connection row with zero-based index `ix` of
    /// the shape whose Visio `ID` is `sheet`.
    Point {
        /// The target shape's Visio `ID`.
        sheet: i32,
        /// The connection row's zero-based index (`ToPart` is 100 + this).
        ix: u32,
    },
    /// Dynamic glue to the shape whose Visio `ID` is `sheet`.
    Dynamic {
        /// The target shape's Visio `ID`.
        sheet: i32,
    },
    /// Unglued.
    Free,
}

/// The four 1-D endpoint cells (ADR 0084 rule 1).
pub(crate) const ENDPOINT_CELLS: [&str; 4] = ["BeginX", "BeginY", "EndX", "EndY"];

/// Whether `formula` is one of the glue forms of ADR 0084 rule 2 (the
/// spellings of `docs/ui/connectors.md` section 8): a re-glue replaces
/// them, a path write keeps them and refreshes only their cached `V`.
fn is_glue_formula(formula: &str) -> bool {
    let text = formula.trim().trim_start_matches('=');
    let upper = text.trim_start().to_ascii_uppercase();
    upper.starts_with("PAR(PNT(") || upper.starts_with("_WALKGLUE(")
}

impl Document {
    /// True when `shape` has the four 1-D endpoint cells, its own or
    /// inherited from its master (ADR 0084 rule 1).
    ///
    /// It replaces "has a glue record" as the test for "is a
    /// connector". A shape that is the `FromSheet` of a glue record but
    /// has no endpoint cells (a file saved by an earlier Larrygram) is
    /// **not** a connector; nothing migrates it.
    pub fn is_connector(&self, shape: ShapeId) -> bool {
        ENDPOINT_CELLS.iter().all(|name| {
            self.resolve(shape, &CellName::new(*name))
                .is_ok_and(|found| {
                    matches!(found.source, ValueSource::Local | ValueSource::Master(_))
                })
        })
    }

    /// The page-space point of one end, from its endpoint cells.
    ///
    /// A shape without a numeric point on both cells is an
    /// [`Error::BadCell`].
    pub fn connector_end(&self, shape: ShapeId, end: ConnectorEnd) -> Result<Point, Error> {
        if self.shape(shape).is_none() {
            return Err(Error::UnknownShape(shape));
        }
        let (x_cell, y_cell) = end.cells();
        let read = |name: &str| -> Result<f64, Error> {
            let found = self.resolve(shape, &CellName::new(name))?;
            match found.value {
                Some(CellValue::Number(value)) if value.is_finite() => Ok(*value),
                _ => Err(Error::BadCell("connector end has no numeric point")),
            }
        };
        Ok(Point::new(read(x_cell)?, read(y_cell)?))
    }

    /// Writes one end's endpoint cells (formula and cached value at
    /// `at`, page inches) and its trigger for `glue`, replacing any
    /// formula already there (ADR 0084 rule 2). It does not touch the
    /// `Connects` records; the caller writes them in the same edit.
    ///
    /// Point glue writes `PAR(PNT(Sheet.<n>!Connections.X<k>,...))`
    /// (row number `k` is one-based), dynamic glue `_WALKGLUE(...)`,
    /// and an unglued end its literal point with no formula. The
    /// trigger is `_XFTRIGGER(Sheet.<n>!EventXFMod)` naming the target,
    /// or `_XFTRIGGER(EventXFMod)` (its own sheet) when unglued.
    pub fn write_connector_end(
        &mut self,
        shape: ShapeId,
        end: ConnectorEnd,
        at: Point,
        glue: EndGlue,
    ) -> Result<(), Error> {
        if self.shape(shape).is_none() {
            return Err(Error::UnknownShape(shape));
        }
        if !at.x.is_finite() || !at.y.is_finite() {
            return Err(Error::BadCell("non-finite connector end"));
        }
        let (x_cell, y_cell) = end.cells();
        let trigger = end.trigger();
        let (x_write, y_write, trigger_write) = match glue {
            EndGlue::Point { sheet, ix } => {
                if sheet < 0 {
                    return Err(Error::BadCell("negative glue target id"));
                }
                let row = ix
                    .checked_add(1)
                    .ok_or(Error::BadCell("connection row index too large"))?;
                let formula = format!(
                    "PAR(PNT(Sheet.{sheet}!Connections.X{row},Sheet.{sheet}!Connections.Y{row}))"
                );
                (
                    CellWrite::new(formula.clone(), CellValue::Number(at.x)),
                    CellWrite::new(formula, CellValue::Number(at.y)),
                    self.trigger_write(shape, Some(sheet)),
                )
            }
            EndGlue::Dynamic { sheet } => {
                if sheet < 0 {
                    return Err(Error::BadCell("negative glue target id"));
                }
                (
                    CellWrite::new(end.walk_glue(), CellValue::Number(at.x)),
                    CellWrite::new(end.walk_glue(), CellValue::Number(at.y)),
                    self.trigger_write(shape, Some(sheet)),
                )
            }
            EndGlue::Free => (
                CellWrite::value_only(CellValue::Number(at.x)),
                CellWrite::value_only(CellValue::Number(at.y)),
                self.trigger_write(shape, None),
            ),
        };
        self.write_cell(SheetRef::Shape(shape), &CellName::new(x_cell), &x_write)?;
        self.write_cell(SheetRef::Shape(shape), &CellName::new(y_cell), &y_write)?;
        self.write_cell(
            SheetRef::Shape(shape),
            &CellName::new(trigger),
            &trigger_write,
        )?;
        Ok(())
    }

    /// The trigger write of one end: the formula naming the glue
    /// target's `EventXFMod` (or the end's own sheet when unglued) and
    /// the cached value that cell carries.
    fn trigger_write(&self, shape: ShapeId, target: Option<i32>) -> CellWrite {
        let formula = match target {
            Some(sheet) => format!("_XFTRIGGER(Sheet.{sheet}!EventXFMod)"),
            None => String::from("_XFTRIGGER(EventXFMod)"),
        };
        CellWrite::new(formula, CellValue::Number(self.event_xf_mod(shape, target)))
    }

    /// The cached `EventXFMod` value of the glue target's sheet (or of
    /// the shape's own sheet), 0 when it has none.
    fn event_xf_mod(&self, shape: ShapeId, target: Option<i32>) -> f64 {
        let subject = match target {
            None => Some(shape),
            Some(visio_id) => self
                .page_of(shape)
                .and_then(|page| self.shape_by_visio_id(page, visio_id)),
        };
        subject
            .and_then(|subject| self.resolve(subject, &CellName::new("EventXFMod")).ok())
            .and_then(|found| match found.value {
                Some(CellValue::Number(value)) if value.is_finite() => Some(*value),
                _ => None,
            })
            .unwrap_or(0.0)
    }

    /// The path write's endpoint half (task P7-31 goal 2): `begin` and
    /// `end`, in the connector's parent coordinates, land in the
    /// endpoint cells.
    ///
    /// A glued end keeps its glue formula (only the cached `V` is
    /// refreshed); any other end becomes its literal point with no
    /// formula (ADR 0084 rule 2's "unglued" row). This replaces a
    /// formula only when it is one of the glue forms; a custom formula
    /// is the caller's business and is replaced too (the path write
    /// owns the routed points).
    pub(crate) fn write_connector_ends(
        &mut self,
        shape: ShapeId,
        begin: Point,
        end: Point,
    ) -> Result<(), Error> {
        for (which, at) in [(ConnectorEnd::Begin, begin), (ConnectorEnd::End, end)] {
            let (x_cell, y_cell) = which.cells();
            for (name, value) in [(x_cell, at.x), (y_cell, at.y)] {
                let kept = self
                    .shape(shape)
                    .and_then(|entity| {
                        entity
                            .sheet
                            .cells
                            .iter()
                            .find(|cell| cell.name.as_str() == name)
                    })
                    .and_then(|cell| cell.formula.as_deref())
                    .filter(|formula| is_glue_formula(formula))
                    .map(|formula| CellWrite::new(formula.to_owned(), CellValue::Number(value)));
                let write = kept.unwrap_or_else(|| CellWrite::value_only(CellValue::Number(value)));
                self.write_cell(SheetRef::Shape(shape), &CellName::new(name), &write)?;
            }
        }
        Ok(())
    }

    /// The path write's pin and size half (task P7-31 goal 2): the
    /// documented formulas of ADR 0084 rule 1 refreshed with the
    /// values the routed ends compute. `Width` is negative when `end`
    /// lies left of `begin`.
    pub(crate) fn write_connector_pins(
        &mut self,
        shape: ShapeId,
        begin: Point,
        end: Point,
    ) -> Result<(), Error> {
        let width = end.x - begin.x;
        let height = CONNECTOR_HEIGHT;
        for (name, formula, value) in [
            ("PinX", "GUARD((BeginX+EndX)/2)", (begin.x + end.x) / 2.0),
            ("PinY", "GUARD((BeginY+EndY)/2)", (begin.y + end.y) / 2.0),
            ("Width", "GUARD(EndX-BeginX)", width),
            ("Height", "GUARD(0.2DL)", height),
            ("LocPinX", "GUARD(Width*0.5)", width * 0.5),
            ("LocPinY", "GUARD(Height*0.5)", height * 0.5),
        ] {
            let write = CellWrite::new(formula, CellValue::Number(value));
            self.write_cell(SheetRef::Shape(shape), &CellName::new(name), &write)?;
        }
        Ok(())
    }
}

/// The connector box's height in inches (`Height` = `GUARD(0.2DL)`,
/// ADR 0084 rule 1).
const CONNECTOR_HEIGHT: f64 = 0.2;
