//! The model mutation API (ADR 0039): creating, deleting and re-parenting
//! shapes, writing cells, adding and re-ordering pages, master instances
//! and `Connects`.
//!
//! Every operation is a method on [`Document`] that returns
//! `Result<_, Error>`; the model never panics (AGENTS.md section 6). The
//! invariants each operation keeps are recorded in ADR 0039; the short
//! version:
//!
//! - Arena ids are stable: an operation never moves or removes a
//!   [`Shape`] of `Document::shapes`, so ids held by an undo step stay
//!   valid across undo. A deleted shape is **detached** (removed from its
//!   page or parent) and comes back through [`Document::restore_shapes`].
//! - Every page and parent list holds live shapes in draw order (z-order
//!   for `Page::shapes`); the arena may hold detached shapes that no page
//!   reaches.
//! - Input-derived counts are capped and arithmetic is checked
//!   (AGENTS.md section 6).
//!
//! The `Connect` glue constants below are the `VisFromParts` /
//! `VisToParts` enumerations transcribed from Microsoft Learn (the source
//! rows are in `docs/sources.md`).
//!
//! Resizing (ADR 0038 rule 3) is the editor's job (ADR 0056): the
//! command layer scales the constant rows and the recalc pass computes
//! the rows whose formulas compute. A cell write (ADR 0038 rules 1-2)
//! stores the formula and its cached value together, and also refreshes
//! the cells the documented-defaults table derives from the written
//! cell by one multiplication (`LocPinX = Width*0.5`).

use crate::cell::{Cell, CellValue};
use crate::defaults::documented_default;
use crate::document::Document;
use crate::error::Error;
use crate::generated::CELLS;
use crate::id::{MasterId, PageId, ShapeId};
use crate::lookup::{PROPERTY_SECTION, find_property_row};
use crate::master::Master;
use crate::name::{CellName, SectionName};
use crate::page::{Connect, Page};
use crate::raw::{Preserved, QName, XmlAttribute, XmlChild, XmlNode};
use crate::section::{Row, Section};
use crate::shape::Shape;
use crate::text::{TextContent, TextPart};
use crate::values::{CellReader, RowTarget, SheetRef};

use larrygram_geom::{Angle, Point, Size, Transform, Unit};

use std::collections::{BTreeSet, HashSet};

// ---------------------------------------------------------------------------
// `VisFromParts` (Microsoft Learn, "VisFromParts enumeration (Visio)")
// ---------------------------------------------------------------------------

/// `visFromNone`: the connection starts from nothing.
pub const VIS_FROM_NONE: i32 = 0;
/// `visLeftEdge`: the connection starts from the left edge of a shape.
pub const VIS_LEFT_EDGE: i32 = 1;
/// `visCenterEdge`: the connection starts from the centre (x) of a 1D
/// shape.
pub const VIS_CENTER_EDGE: i32 = 2;
/// `visRightEdge`: the connection starts from the right edge of a shape.
pub const VIS_RIGHT_EDGE: i32 = 3;
/// `visBottomEdge`: the connection starts from the bottom edge of a shape.
pub const VIS_BOTTOM_EDGE: i32 = 4;
/// `visMiddleEdge`: the connection starts from the middle (y) of a shape.
pub const VIS_MIDDLE_EDGE: i32 = 5;
/// `visTopEdge`: the connection starts from the top edge of a shape.
pub const VIS_TOP_EDGE: i32 = 6;
/// `visBeginX`: the connection starts from the begin point x of a 1D shape.
pub const VIS_BEGIN_X: i32 = 7;
/// `visBeginY`: the connection starts from the begin point y of a 1D shape.
pub const VIS_BEGIN_Y: i32 = 8;
/// `visBegin`: the connection starts from the begin point of a 1D shape.
pub const VIS_BEGIN: i32 = 9;
/// `visEndX`: the connection starts from the end point x of a 1D shape.
pub const VIS_END_X: i32 = 10;
/// `visEndY`: the connection starts from the end point y of a 1D shape.
pub const VIS_END_Y: i32 = 11;
/// `visEnd`: the connection starts from the end point of a 1D shape.
pub const VIS_END: i32 = 12;
/// `visFromAngle`: the connection starts from the direction of a
/// connection point.
pub const VIS_FROM_ANGLE: i32 = 13;
/// `visFromPin`: the connection starts from the pin of a shape.
pub const VIS_FROM_PIN: i32 = 14;
/// `visControlPoint`: the connection starts from a control point; the
/// value is 100 plus the zero-based row index.
pub const VIS_CONTROL_POINT: i32 = 100;

// ---------------------------------------------------------------------------
// `VisToParts` (Microsoft Learn, "VisToParts enumeration (Visio)")
// ---------------------------------------------------------------------------

/// `visToNone`: do not connect.
pub const VIS_TO_NONE: i32 = 0;
/// `visGuideX`: connect to a vertical guide on the target shape.
pub const VIS_GUIDE_X: i32 = 1;
/// `visGuideY`: connect to a horizontal guide on the target shape.
pub const VIS_GUIDE_Y: i32 = 2;
/// `visWholeShape`: connect to the entire target shape, dynamic glue.
pub const VIS_WHOLE_SHAPE: i32 = 3;
/// `visGuideIntersect`: connect to the intersection of guides on the
/// target shape.
pub const VIS_GUIDE_INTERSECT: i32 = 4;
/// `visToAngle`: connect to the angle on the target shape.
pub const VIS_TO_ANGLE: i32 = 7;
/// `visConnectionPoint`: connect to a connection point; the value is 100
/// plus the zero-based row index.
pub const VIS_CONNECTION_POINT: i32 = 100;
/// `visConnectToError`: error connecting to the shape.
pub const VIS_CONNECT_TO_ERROR: i32 = -1;

/// The cap on the shape arena (an input-derived count, AGENTS.md section
/// 6). A mutation past it is [`Error::TooManyShapes`].
const MAX_SHAPES: usize = 1 << 24;
/// The cap on the page and master lists.
const MAX_LIST: usize = 1 << 16;
/// The cap on the points of one connector path.
const MAX_PATH_POINTS: usize = 1 << 16;
/// The cap on any parent or children walk (never recurse unbounded).
const MAX_WALK: usize = 1 << 16;

/// One cell write (ADR 0038): the formula to store and the cached value
/// that goes with it.
///
/// An edit writes **both**: a literal formula (or the documented simple
/// formula, e.g. `Width*0.5` for `LocPinX`) and the cached `V` value, so a
/// file we save reads back the same and a later formula engine (Phase 6)
/// recalculates to the same numbers. The formula is written as spelled;
/// `F='Inh'` is expressed by *not* writing the cell.
#[derive(Debug, Clone, PartialEq)]
pub struct CellWrite {
    /// The formula text, e.g. `"2.5"` or `"Width*0.5"`.
    pub formula: String,
    /// The cached value to store alongside the formula.
    pub value: CellValue,
}

impl CellWrite {
    /// A write of one formula and its cached value.
    pub fn new(formula: impl Into<String>, value: CellValue) -> Self {
        Self {
            formula: formula.into(),
            value,
        }
    }
}

/// A detached shape subtree, kept so [`Document::restore_shapes`] can put
/// it back (ADR 0039: delete is detach, ids stay stable).
#[derive(Debug, Clone, PartialEq)]
pub struct DetachedShapes {
    /// The page the subtree was detached from.
    pub page: PageId,
    /// The parent the root was detached from, or `None` for a top-level
    /// shape.
    pub parent: Option<ShapeId>,
    /// The index the root held in its page or parent list.
    pub index: usize,
    /// The detached root first, then its descendants (arena order).
    pub shapes: Vec<Shape>,
    /// The `Connects` records removed with the subtree, each with the
    /// list index it held (so [`Document::restore_shapes`] can put it
    /// back exactly where it was, task P5-11).
    pub connects: Vec<(usize, Connect)>,
}

/// The cells the documented-defaults table derives from `Width` or
/// `Height` by one multiplication (ADR 0038 rule 2). An edit to `Width`
/// refreshes these; any other dependent cell is left stale until Phase 6.
const DERIVED_CELLS: &[&str] = &[
    "LocPinX",
    "LocPinY",
    "TxtPinX",
    "TxtPinY",
    "TxtLocPinX",
    "TxtLocPinY",
    "TxtWidth",
    "TxtHeight",
];

/// The documented formula of one derived cell when it derives from
/// `source` by one multiplication: `(cell, formula, factor)`.
fn derived_from(source: &str) -> Vec<(&'static str, String, f64)> {
    let mut out = Vec::new();
    for name in DERIVED_CELLS {
        let factor = match documented_default(name) {
            Some(crate::defaults::DocumentedDefault::HalfOf(other)) if other == source => 0.5,
            Some(crate::defaults::DocumentedDefault::WholeOf(other)) if other == source => 1.0,
            _ => continue,
        };
        out.push((*name, format!("{source}*{}", literal(factor)), factor));
    }
    out
}

/// The literal spelling of a finite number (the formula of a literal
/// cell, ADR 0038). `-0.0` folds to `"0"`.
///
/// This is the **one** number-spelling rule of the workspace (task
/// P5-19 review F6): the model writes it and the command layer spells
/// its `CellWrite` formulas with it (`builtins::edit::number_literal`),
/// so an undo/redo replay can never write a different formula text than
/// the first apply.
pub fn literal(value: f64) -> String {
    if value == 0.0 {
        return "0".to_owned();
    }
    format!("{value}")
}

/// Whether `formula` is absent, `Inh`, or the documented formula
/// `wanted` (compared without spaces or a leading `=`): such a cell still
/// follows its documented default and is refreshed by an edit to the cell
/// it derives from (ADR 0038 rule 2). A cell with any other formula is a
/// custom value and stays.
fn formula_follows(formula: Option<&str>, wanted: &str) -> bool {
    let Some(formula) = formula else {
        return true;
    };
    let now = normalize_formula(formula);
    now == "inh" || now == normalize_formula(wanted)
}

/// A formula without spaces or its leading `=`, lower-cased (the cell
/// names of a formula compare case-insensitively).
fn normalize_formula(formula: &str) -> String {
    formula
        .trim()
        .trim_start_matches('=')
        .chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect()
}

/// The [`Unit`] a ShapeSheet-table unit name spells (the table's
/// `default_unit`, ADR 0008). An unknown spelling is
/// [`Unit::Unitsless`].
fn unit_from_name(name: &str) -> Unit {
    match name.trim().to_ascii_lowercase().as_str() {
        "in" | "inch" | "inches" => Unit::Inches,
        "ft" | "foot" | "feet" => Unit::Feet,
        "yd" | "yard" | "yards" => Unit::Yards,
        "mi" | "mile" | "miles" => Unit::Miles,
        "mm" | "millimeter" | "millimeters" => Unit::Millimeters,
        "cm" | "centimeter" | "centimeters" => Unit::Centimeters,
        "m" | "meter" | "meters" => Unit::Meters,
        "km" | "kilometer" | "kilometers" => Unit::Kilometers,
        "pt" | "point" | "points" => Unit::Points,
        "pica" | "picas" => Unit::Picas,
        "cicero" | "ciceros" => Unit::Ciceros,
        "ang" | "angstrom" | "angstroms" => Unit::Angstroms,
        "nm" | "nanometer" | "nanometers" => Unit::Nanometers,
        "micron" | "microns" | "micrometer" | "micrometers" => Unit::Microns,
        "millipoint" | "millipoints" => Unit::Millipoints,
        "deg" | "degree" | "degrees" => Unit::Degrees,
        "rad" | "radian" | "radians" => Unit::Radians,
        "%" | "percent" => Unit::Percent,
        _ => Unit::Unitsless,
    }
}

/// The unit of one cell: the ShapeSheet table's `default_unit` for its
/// kind (ADR 0038: "the cell's unit follows the ShapeSheet table's
/// kind"), [`Unit::Unitsless`] when the table has no entry.
fn cell_unit(name: &str) -> Unit {
    CELLS
        .iter()
        .find(|def| def.name == name)
        .map_or(Unit::Unitsless, |def| unit_from_name(def.default_unit))
}

/// The number a cell write carries, or [`Error::BadCell`]: a numeric
/// value must be finite.
fn checked_number(write: &CellWrite) -> Result<(), Error> {
    match write.value {
        CellValue::Number(value) if !value.is_finite() => Err(Error::BadCell("non-finite number")),
        _ => Ok(()),
    }
}

/// Writes one cell of one sheet (formula and cached value together, ADR
/// 0038): the unit follows the ShapeSheet table's kind.
fn write_sheet_cell(sheet: &mut crate::sheet::Sheet, cell: &CellName, write: &CellWrite) {
    write_entry(&mut sheet.cells, cell, write, cell_unit(cell.as_str()));
}

/// The shared tail of every cell write: `cell` gets `write`'s formula
/// and cached value in `unit`, keeping its place when `cells` holds it
/// and appended at the end when it does not. The caller picks the unit
/// (the ShapeSheet table's kind for a sheet cell, the unit the row
/// records for a Shape Data value).
fn write_entry(cells: &mut Vec<Cell>, cell: &CellName, write: &CellWrite, unit: Unit) {
    match cells
        .iter_mut()
        .find(|existing| existing.name.as_str() == cell.as_str())
    {
        Some(existing) => {
            existing.formula = Some(write.formula.clone());
            existing.value = Some(write.value.clone());
            existing.unit = unit;
        }
        None => cells.push(Cell {
            name: cell.clone(),
            formula: Some(write.formula.clone()),
            value: Some(write.value.clone()),
            unit,
            preserved: Preserved::new(),
        }),
    }
}

/// The unit a Shape Data `Value` cell keeps through a write: the unit
/// the cell records (own or inherited), or the ShapeSheet table's kind
/// for a cell the write creates (task P6-18: the file's `U` spelling is
/// data and survives an edit).
fn value_unit(row: &Row) -> Unit {
    row.cells
        .iter()
        .find(|cell| cell.name.as_str() == "Value")
        .map(|cell| cell.unit)
        .unwrap_or_else(|| cell_unit("Value"))
}

/// A number cell with a literal formula (ADR 0038: formula and value).
fn number_cell(name: &str, value: f64) -> Cell {
    Cell {
        name: CellName::new(name),
        formula: Some(literal(value)),
        value: Some(CellValue::Number(value)),
        unit: cell_unit(name),
        preserved: Preserved::new(),
    }
}

/// A number row cell with a literal formula and an explicit unit.
fn row_number_cell(name: &str, value: f64, unit: Unit) -> Cell {
    Cell {
        name: CellName::new(name),
        formula: Some(literal(value)),
        value: Some(CellValue::Number(value)),
        unit,
        preserved: Preserved::new(),
    }
}
/// The unique spelling of `wanted` under the "append ` (n)`" rule (ADR
/// 0039 invariant 4): `wanted` when free, else `wanted (2)`, `wanted
/// (3)`, ...
///
/// `taken` is the indexed set of the names already in use: the callers
/// build one set per operation instead of cloning every existing `name_u`
/// and scanning a `Vec` per candidate (task P5-19 review F10).
fn unique_name_u(wanted: &str, taken: &BTreeSet<&str>) -> String {
    if !taken.contains(wanted) {
        return wanted.to_owned();
    }
    for n in 2..10_000u32 {
        let candidate = format!("{wanted} ({n})");
        if !taken.contains(candidate.as_str()) {
            return candidate;
        }
    }
    wanted.to_owned()
}

/// The next free Visio `ID`: the largest in use plus one (ADR 0039
/// invariant 4), `1` when none is in use.
fn next_id_of(ids: impl Iterator<Item = i32>) -> Result<i32, Error> {
    let mut max = 0i32;
    for id in ids {
        max = max.max(id);
    }
    max.checked_add(1).ok_or(Error::TooManyShapes)
}

/// The transform of one shape's local box (the render builder's rule,
/// `crates/render/src/build.rs`): translate `-LocPin`, flip, rotate,
/// translate `Pin`.
fn shape_transform(
    pin: Point,
    loc_pin: Point,
    angle: f64,
    flip_x: bool,
    flip_y: bool,
) -> Transform {
    Transform::translate(Point::new(-loc_pin.x, -loc_pin.y))
        .then(Transform::scale(
            if flip_x { -1.0 } else { 1.0 },
            if flip_y { -1.0 } else { 1.0 },
        ))
        .then(Transform::rotate(Angle::from_radians(angle)))
        .then(Transform::translate(pin))
}

/// The `(angle, flip_x, flip_y)` half of one shape transform.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Linear {
    /// The rotation, in radians.
    angle: f64,
    /// Whether the x axis is mirrored.
    flip_x: bool,
    /// Whether the y axis is mirrored.
    flip_y: bool,
}

impl Linear {
    /// This linear map applied after `earlier` (the composition of two
    /// shape transforms keeps the dihedral form `rotate * flip`).
    fn compose(self, earlier: Self) -> Self {
        let (sx, sy) = (
            if self.flip_x { -1.0 } else { 1.0 },
            if self.flip_y { -1.0 } else { 1.0 },
        );
        let (tx, ty) = (
            if earlier.flip_x { -1.0 } else { 1.0 },
            if earlier.flip_y { -1.0 } else { 1.0 },
        );
        // A flip pair with sx*sy = -1 (one flip) reverses rotations.
        let reversed = sx * sy < 0.0;
        let angle = if reversed {
            self.angle - earlier.angle
        } else {
            self.angle + earlier.angle
        };
        let (mut nx, mut ny) = (sx * tx, sy * ty);
        let mut angle = angle;
        if nx < 0.0 && ny < 0.0 {
            // Both axes mirrored is a half turn, not a flip pair.
            nx = 1.0;
            ny = 1.0;
            angle += std::f64::consts::PI;
        }
        Self {
            angle,
            flip_x: nx < 0.0,
            flip_y: ny < 0.0,
        }
    }
}

/// The transform cells of one shape, read with the cached-value policy
/// (ADR 0021).
struct Frame {
    /// The pin position in the parent's space.
    pin: Point,
    /// The pin position inside the shape's local box.
    loc_pin: Point,
    /// The rotation and flips.
    linear: Linear,
    /// The local box size.
    size: Size,
}

impl Frame {
    /// The transform from the shape's local box to its parent's space.
    fn transform(&self) -> Transform {
        shape_transform(
            self.pin,
            self.loc_pin,
            self.linear.angle,
            self.linear.flip_x,
            self.linear.flip_y,
        )
    }
}
/// The `Text` element of one shape's text (the inverse of the reader's
/// `map_text`, ADR 0019): literal text nodes and `cp`/`pp`/`tp`/`fld`
/// markers naming their row `IX`. A field's cached display text is the
/// literal text inside its `fld` element, the spelling Visio writes and
/// the reader maps back (task P8-10 round-trip fix).
fn text_element(text: &TextContent, template: Option<&QName>) -> XmlNode {
    let name = |local: &str| -> QName {
        match template {
            Some(template) => QName {
                prefix: template.prefix.clone(),
                uri: template.uri.clone(),
                local: local.to_owned(),
            },
            None => QName::local(local),
        }
    };
    let mut node = XmlNode {
        name: name("Text"),
        attributes: Vec::new(),
        children: Vec::new(),
        self_closing: true,
    };
    let marker_node = |local: &str, ix: u32| -> XmlNode {
        XmlNode {
            name: name(local),
            attributes: vec![XmlAttribute {
                name: name("IX"),
                value: ix.to_string(),
            }],
            children: Vec::new(),
            self_closing: true,
        }
    };
    let mut parts = text.parts.iter().peekable();
    while let Some(part) = parts.next() {
        match part {
            TextPart::Text(literal) => node.children.push(XmlChild::Text(literal.clone())),
            TextPart::CharRun { ix } => node
                .children
                .push(XmlChild::Element(marker_node("cp", *ix))),
            TextPart::ParaRun { ix } => node
                .children
                .push(XmlChild::Element(marker_node("pp", *ix))),
            TextPart::TabsRun { ix } => node
                .children
                .push(XmlChild::Element(marker_node("tp", *ix))),
            TextPart::Field { ix } => {
                let mut element = marker_node("fld", *ix);
                // The cached display text is the literal part right after
                // the marker; any other part is left in the walk.
                if let Some(TextPart::Text(literal)) = parts.peek().copied() {
                    element.children.push(XmlChild::Text(literal.clone()));
                    element.self_closing = false;
                    parts.next();
                }
                node.children.push(XmlChild::Element(element));
            }
        }
    }
    if !node.children.is_empty() {
        node.self_closing = false;
    }
    node
}

impl Document {
    /// Inserts `shape` as a top-level shape of `page`, at z-order `index`
    /// (`None` appends on top).
    ///
    /// Keeps: the new shape gets a fresh arena id and the page's next free
    /// Visio `ID`; `index` past the end clamps; a stale `page` is
    /// [`Error::UnknownPage`].
    pub fn insert_shape(
        &mut self,
        page: PageId,
        shape: Shape,
        index: Option<usize>,
    ) -> Result<ShapeId, Error> {
        self.check_room()?;
        let visio_id = self.next_visio_id(page)?;
        let at = self.insert_index(page, index)?;
        let mut shape = shape;
        shape.parent = None;
        shape.visio_id = visio_id;
        // The children are validated before the shape enters the arena
        // (task P5-19 review F11): a stale child id must not leave an
        // unnamed entry behind.
        let children = shape.children.clone();
        for child in &children {
            if self.shape(*child).is_none() {
                return Err(Error::UnknownShape(*child));
            }
        }
        let id = self.add_shape(shape);
        for child in children {
            if let Some(entry) = self.shape_mut(child) {
                entry.parent = Some(id);
            }
        }
        if let Some(entity) = self.pages.get_mut(page.index as usize) {
            let at = at.min(entity.shapes.len());
            entity.shapes.insert(at, id);
        }
        Ok(id)
    }

    /// Inserts `shape` as a sub-shape of `parent`, at child index `index`
    /// (`None` appends last).
    ///
    /// Keeps: `Shape::parent` and `Shape::children` agree; a stale
    /// `parent` is [`Error::UnknownShape`].
    pub fn insert_subshape(
        &mut self,
        parent: ShapeId,
        shape: Shape,
        index: Option<usize>,
    ) -> Result<ShapeId, Error> {
        self.check_room()?;
        if self.shape(parent).is_none() {
            return Err(Error::UnknownShape(parent));
        }
        let mut shape = shape;
        shape.parent = Some(parent);
        if let Some(page) = self.page_of(parent) {
            shape.visio_id = self.next_visio_id(page)?;
        }
        // The same validate-first rule as `insert_shape` (task P5-19
        // review F11).
        let children = shape.children.clone();
        for child in &children {
            if self.shape(*child).is_none() {
                return Err(Error::UnknownShape(*child));
            }
        }
        let id = self.add_shape(shape);
        for child in children {
            if let Some(entry) = self.shape_mut(child) {
                entry.parent = Some(id);
            }
        }
        if let Some(entity) = self.shape_mut(parent) {
            let at = index
                .unwrap_or(entity.children.len())
                .min(entity.children.len());
            entity.children.insert(at, id);
        }
        Ok(id)
    }

    /// Detaches the shape's subtree from its page or parent (delete,
    /// ADR 0039) and returns it with the `Connects` records that named
    /// any detached shape.
    ///
    /// Keeps: the arena ids survive; the page or parent list no longer
    /// holds the root. A stale `shape` is [`Error::UnknownShape`].
    pub fn detach_shape(&mut self, shape: ShapeId) -> Result<DetachedShapes, Error> {
        let entity = self.shape(shape).ok_or(Error::UnknownShape(shape))?;
        let parent = entity.parent;
        let page = self.page_of(shape).ok_or(Error::UnknownShape(shape))?;
        let subtree = self.subtree_ids(shape);
        let mut shapes = Vec::with_capacity(subtree.len());
        for id in &subtree {
            if let Some(entry) = self.shape(*id) {
                shapes.push(entry.clone());
            }
        }
        let index = match parent {
            Some(owner) => {
                let owner = self.shape_mut(owner).ok_or(Error::UnknownShape(owner))?;
                let at = owner
                    .children
                    .iter()
                    .position(|id| *id == shape)
                    .ok_or(Error::UnknownShape(shape))?;
                owner.children.remove(at);
                at
            }
            None => {
                let entity = self
                    .pages
                    .get_mut(page.index as usize)
                    .ok_or(Error::UnknownPage(PageId { index: page.index }))?;
                let at = entity
                    .shapes
                    .iter()
                    .position(|id| *id == shape)
                    .ok_or(Error::UnknownShape(shape))?;
                entity.shapes.remove(at);
                at
            }
        };
        let removed: Vec<i32> = shapes.iter().map(|entry| entry.visio_id).collect();
        let mut connects = Vec::new();
        if let Some(entity) = self.pages.get_mut(page.index as usize) {
            let mut kept = Vec::with_capacity(entity.connects.len());
            for (at, connect) in entity.connects.drain(..).enumerate() {
                if removed.contains(&connect.from_sheet) || removed.contains(&connect.to_sheet) {
                    connects.push((at, connect));
                } else {
                    kept.push(connect);
                }
            }
            entity.connects = kept;
        }
        Ok(DetachedShapes {
            page,
            parent,
            index,
            shapes,
            connects,
        })
    }

    /// Puts a detached subtree back at its recorded page, parent and
    /// index, and restores its `Connects` at their recorded list
    /// positions. Returns the root's new (old) arena id.
    ///
    /// Keeps: the same invariants as [`Document::insert_shape`]; an index
    /// past the current list length clamps.
    pub fn restore_shapes(&mut self, detached: DetachedShapes) -> Result<ShapeId, Error> {
        if self.page(detached.page).is_none() {
            return Err(Error::UnknownPage(detached.page));
        }
        let root = detached
            .shapes
            .first()
            .ok_or(Error::BadCell("no detached root"))?
            .id;
        for entry in &detached.shapes {
            let slot = self
                .shapes
                .get_mut(entry.id.index as usize)
                .ok_or(Error::UnknownShape(entry.id))?;
            *slot = entry.clone();
        }
        match detached.parent {
            Some(owner) => {
                let owner = self.shape_mut(owner).ok_or(Error::UnknownShape(owner))?;
                owner.children.retain(|id| *id != root);
                let at = detached.index.min(owner.children.len());
                owner.children.insert(at, root);
            }
            None => {
                let page = self
                    .pages
                    .get_mut(detached.page.index as usize)
                    .ok_or(Error::UnknownPage(detached.page))?;
                page.shapes.retain(|id| *id != root);
                let at = detached.index.min(page.shapes.len());
                page.shapes.insert(at, root);
            }
        }
        if let Some(page) = self.pages.get_mut(detached.page.index as usize) {
            // Back at the list indices the records were removed from
            // (ascending, so each index is still the right slot).
            let mut restored = detached.connects;
            restored.sort_by_key(|(at, _)| *at);
            for (at, connect) in restored {
                let at = at.min(page.connects.len());
                page.connects.insert(at, connect);
            }
        }
        Ok(root)
    }

    /// Moves `shape` to child index `index` of `new_parent` (`None`:
    /// top-level of its page), keeping z-order semantics of the target
    /// list.
    ///
    /// Keeps: no cycles (a shape cannot become its own ancestor); the
    /// source list loses the shape at its old position. Moving to another
    /// page is [`Error::NotOnPage`].
    pub fn reparent_shape(
        &mut self,
        shape: ShapeId,
        new_parent: Option<ShapeId>,
        index: usize,
    ) -> Result<(), Error> {
        if self.shape(shape).is_none() {
            return Err(Error::UnknownShape(shape));
        }
        let page = self.page_of(shape).ok_or(Error::UnknownShape(shape))?;
        if let Some(owner) = new_parent {
            if self.shape(owner).is_none() {
                return Err(Error::UnknownShape(owner));
            }
            if owner == shape || self.is_descendant(owner, shape) {
                return Err(Error::WouldCycle);
            }
            if self.page_of(owner) != Some(page) {
                return Err(Error::NotOnPage(owner, page));
            }
        }
        self.unlist(shape);
        if let Some(owner) = new_parent {
            if let Some(entity) = self.shape_mut(owner) {
                let at = index.min(entity.children.len());
                entity.children.insert(at, shape);
            }
        } else if let Some(entity) = self.pages.get_mut(page.index as usize) {
            let at = index.min(entity.shapes.len());
            entity.shapes.insert(at, shape);
        }
        if let Some(entity) = self.shape_mut(shape) {
            entity.parent = new_parent;
        }
        Ok(())
    }

    /// Moves `shape` to z-order `index` of its page (bring forward, send
    /// to back, ...). Indices clamp.
    pub fn set_z_order(&mut self, page: PageId, shape: ShapeId, index: usize) -> Result<(), Error> {
        let entity = self
            .pages
            .get_mut(page.index as usize)
            .ok_or(Error::UnknownPage(PageId { index: page.index }))?;
        let at = entity
            .shapes
            .iter()
            .position(|id| *id == shape)
            .ok_or(Error::NotOnPage(shape, page))?;
        entity.shapes.remove(at);
        let at = index.min(entity.shapes.len());
        entity.shapes.insert(at, shape);
        Ok(())
    }

    /// Replaces the shape's text (`Text` element and `Shape::text`, ADR
    /// 0019) with `text`.
    ///
    /// The `Text` element is rebuilt in the shape's preserved content
    /// (the write side copies it into the tree), keeping the element
    /// spelling the file used.
    pub fn set_shape_text(&mut self, shape: ShapeId, text: TextContent) -> Result<(), Error> {
        let entity = self.shape_mut(shape).ok_or(Error::UnknownShape(shape))?;
        let template = entity
            .preserved
            .children
            .iter()
            .find(|child| child.name.local == "Text")
            .map(|child| child.name.clone());
        let element = text_element(&text, template.as_ref());
        entity
            .preserved
            .children
            .retain(|child| child.name.local != "Text");
        entity.preserved.children.push(element);
        entity.text = Some(text);
        Ok(())
    }

    /// The sheet a write targets (the shared head of the write
    /// methods): the shape's or page's own sheet, or the error a stale
    /// id carries.
    fn write_target(&mut self, sheet: SheetRef) -> Result<&mut crate::sheet::Sheet, Error> {
        match sheet {
            SheetRef::Shape(id) => self
                .shape_mut(id)
                .map(|entity| &mut entity.sheet)
                .ok_or(Error::UnknownShape(id)),
            SheetRef::Page(id) => self
                .pages
                .get_mut(id.index as usize)
                .map(|entity| &mut entity.sheet)
                .ok_or(Error::UnknownPage(id)),
        }
    }

    /// Writes one cell of a shape or page sheet (ADR 0038: formula and
    /// cached value together).
    ///
    /// Keeps: a sheet the id does not name is an `Err`; the cell's unit
    /// follows the ShapeSheet table's kind. The cells the
    /// documented-defaults table derives from the written cell by one
    /// multiplication (`LocPinX = Width*0.5`, ADR 0038 rule 2) are
    /// refreshed as well, unless their formula is a custom one.
    pub fn write_cell(
        &mut self,
        sheet: SheetRef,
        cell: &CellName,
        write: &CellWrite,
    ) -> Result<(), Error> {
        checked_number(write)?;
        let target = self.write_target(sheet)?;
        write_sheet_cell(target, cell, write);
        if let CellValue::Number(value) = write.value {
            for (derived, formula, factor) in derived_from(cell.as_str()) {
                let follows = target
                    .cells
                    .iter()
                    .find(|existing| existing.name.as_str() == derived)
                    .is_none_or(|existing| formula_follows(existing.formula.as_deref(), &formula));
                if !follows {
                    continue;
                }
                let fresh = CellWrite::new(formula, CellValue::Number(value * factor));
                write_sheet_cell(target, &CellName::new(derived), &fresh);
            }
        }
        Ok(())
    }

    /// Writes one cell of one row of a repeated section (a `Geometry` or
    /// `Character` row), named by [`RowTarget`] (ADR 0021 amendment).
    ///
    /// The row must exist in the sheet's own section (a missing section
    /// or row is [`Error::BadCell`]); the cell's unit follows the
    /// ShapeSheet table's kind.
    pub fn write_row_cell(
        &mut self,
        sheet: SheetRef,
        section: &SectionName,
        row: RowTarget,
        cell: &CellName,
        write: &CellWrite,
    ) -> Result<(), Error> {
        checked_number(write)?;
        let target = self.write_target(sheet)?;
        let section_slot = target
            .sections
            .iter_mut()
            .find(|entry| entry.name.as_str() == section.as_str())
            .ok_or(Error::BadCell("no such section"))?;
        let row_slot = match (row.ix, row.position) {
            (Some(ix), _) => section_slot
                .rows
                .iter_mut()
                .find(|entry| entry.ix == Some(ix)),
            (None, Some(position)) => section_slot.rows.get_mut(position),
            (None, None) => None,
        }
        .ok_or(Error::BadCell("no such row"))?;
        write_entry(&mut row_slot.cells, cell, write, cell_unit(cell.as_str()));
        Ok(())
    }

    /// Writes one **section-level** cell of a repeated section (the
    /// `NoFill` family of `Geometry`, ADR 0019): a cell the section holds
    /// directly, not in a row.
    ///
    /// The section must exist in the sheet's own sections (a missing
    /// section is [`Error::BadCell`]); the cell's unit follows the
    /// ShapeSheet table's kind.
    pub fn write_section_cell(
        &mut self,
        sheet: SheetRef,
        section: &SectionName,
        cell: &CellName,
        write: &CellWrite,
    ) -> Result<(), Error> {
        checked_number(write)?;
        let target = self.write_target(sheet)?;
        let section_slot = target
            .sections
            .iter_mut()
            .find(|entry| entry.name.as_str() == section.as_str())
            .ok_or(Error::BadCell("no such section"))?;
        write_entry(
            &mut section_slot.cells,
            cell,
            write,
            cell_unit(cell.as_str()),
        );
        Ok(())
    }

    /// Writes the `Value` cell of one Shape Data row
    /// (`Prop.<row>.Value`, ADR 0058): `row` names the row by its key
    /// (the row's `N` name, or `Row_<n>` for the row at 1-based
    /// position `n`).
    ///
    /// Keeps (task P6-18, the P6-09 note):
    ///
    /// - a row the shape inherits from its master chain is written as a
    ///   **local row**: the row is copied onto the instance with its
    ///   other cells intact and the `Value` cell is written there;
    /// - the `Value` cell keeps the unit it records (own or inherited);
    ///   a cell the write creates takes the ShapeSheet table's kind;
    /// - a row neither the shape nor its master chain defines is an
    ///   [`Error::BadCell`], and a non-finite number is refused.
    ///
    /// Returns the [`RowTarget`] the write landed on (the undo step's
    /// dirty address names the cell through it).
    pub fn write_property_value(
        &mut self,
        shape: ShapeId,
        row: &str,
        write: &CellWrite,
    ) -> Result<RowTarget, Error> {
        checked_number(write)?;
        if self.shape(shape).is_none() {
            return Err(Error::UnknownShape(shape));
        }
        let cell = CellName::from_static("Value");
        let own = self
            .shape(shape)
            .and_then(|entity| find_property_row(&entity.sheet, row));
        if let Some(position) = own {
            let target = self.write_target(SheetRef::Shape(shape))?;
            let section = target
                .sections
                .iter_mut()
                .find(|entry| entry.name.as_str() == PROPERTY_SECTION)
                .ok_or(Error::BadCell("no such section"))?;
            let row_slot = section
                .rows
                .get_mut(position)
                .ok_or(Error::BadCell("no such row"))?;
            let unit = value_unit(row_slot);
            write_entry(&mut row_slot.cells, &cell, write, unit);
            return Ok(RowTarget {
                position: Some(position),
                ix: row_slot.ix,
            });
        }
        let inherited = self
            .property_rows(shape)
            .into_iter()
            .find(|entry| entry.inherited && entry.key == row);
        if let Some(entry) = inherited {
            let mut materialised = entry.row;
            let unit = value_unit(&materialised);
            write_entry(&mut materialised.cells, &cell, write, unit);
            let target = self.write_target(SheetRef::Shape(shape))?;
            let at = match target
                .sections
                .iter()
                .position(|entry| entry.name.as_str() == PROPERTY_SECTION)
            {
                Some(at) => at,
                None => {
                    target
                        .sections
                        .push(Section::new(SectionName::from_static(PROPERTY_SECTION)));
                    target.sections.len().saturating_sub(1)
                }
            };
            let section = target
                .sections
                .get_mut(at)
                .ok_or(Error::BadCell("no such section"))?;
            section.rows.push(materialised);
            let position = section.rows.len().saturating_sub(1);
            let ix = section.rows.get(position).and_then(|row| row.ix);
            return Ok(RowTarget {
                position: Some(position),
                ix,
            });
        }
        Err(Error::BadCell("no such Shape Data row"))
    }

    /// Adds a new empty page named `name` on top of the page list and
    /// returns its id.
    ///
    /// Keeps: `NameU` is unique in the document (the "append ` (n)`"
    /// rule); the new page's empty sheet resolves to the documented page
    /// defaults of the ShapeSheet table (ADR 0021).
    pub fn add_page(&mut self, name: &str) -> Result<PageId, Error> {
        if self.pages.len() >= MAX_LIST {
            return Err(Error::TooManyShapes);
        }
        let index = u32::try_from(self.pages.len()).map_err(|_| Error::TooManyShapes)?;
        let visio_id = next_id_of(self.pages.iter().map(|page| page.visio_id))?;
        let taken: BTreeSet<&str> = self.pages.iter().map(|page| page.name_u.as_str()).collect();
        let mut page = Page::new(PageId { index }, visio_id);
        page.name = name.to_owned();
        page.name_u = unique_name_u(name, &taken);
        self.pages.push(page);
        Ok(PageId { index })
    }

    /// Removes the page and returns it (with its shapes and connects).
    ///
    /// Keeps: `Page::background` references to the removed page are
    /// cleared (and the later ones follow the list, whose indices are
    /// page ids); the last page may be removed (a document with no pages
    /// is legal in the model; the writer refuses to save it).
    pub fn delete_page(&mut self, page: PageId) -> Result<Page, Error> {
        let at = page.index as usize;
        if at >= self.pages.len() {
            return Err(Error::UnknownPage(page));
        }
        let removed = self.pages.remove(at);
        // Page ids are list positions: every later id moves down one, and
        // the background references follow.
        for (index, entry) in self.pages.iter_mut().enumerate() {
            entry.background = match entry.background {
                Some(background) if background == page => None,
                Some(background) if background.index > page.index => Some(PageId {
                    index: background.index - 1,
                }),
                other => other,
            };
            entry.id = PageId {
                index: u32::try_from(index).unwrap_or(u32::MAX),
            };
        }
        Ok(removed)
    }

    /// Renames the page (`Name` and `NameU`).
    ///
    /// `NameU` keeps the "append ` (n)`" uniqueness rule.
    pub fn rename_page(&mut self, page: PageId, name: &str) -> Result<(), Error> {
        let at = page.index as usize;
        if at >= self.pages.len() {
            return Err(Error::UnknownPage(page));
        }
        let fresh = {
            let taken: BTreeSet<&str> = self
                .pages
                .iter()
                .enumerate()
                .filter(|(index, _)| *index != at)
                .map(|(_, entry)| entry.name_u.as_str())
                .collect();
            unique_name_u(name, &taken)
        };
        if let Some(entity) = self.pages.get_mut(at) {
            entity.name = name.to_owned();
            entity.name_u = fresh;
        }
        Ok(())
    }

    /// Moves the page at list index `from` to index `to` (reorder). The
    /// `to` index clamps; a stale `from` is an [`Error::UnknownPage`].
    ///
    /// Page ids are list positions, so the move rewrites every
    /// `Page::background` reference to follow its page.
    pub fn move_page(&mut self, from: usize, to: usize) -> Result<(), Error> {
        if from >= self.pages.len() {
            return Err(Error::UnknownPage(PageId {
                index: u32::try_from(from).unwrap_or(u32::MAX),
            }));
        }
        let last = self.pages.len().saturating_sub(1);
        let to = to.min(last);
        if from == to {
            return Ok(());
        }
        let old_positions: Vec<PageId> = self.pages.iter().map(|page| page.id).collect::<Vec<_>>();
        let page = self.pages.remove(from);
        self.pages.insert(to, page);
        // Page ids are list positions: rewrite them and the background
        // references so every id still names the page it named.
        let mut remap = vec![0u32; old_positions.len()];
        for (new_index, entry) in self.pages.iter().enumerate() {
            if let Some(slot) = remap.get_mut(entry.id.index as usize) {
                *slot = u32::try_from(new_index).unwrap_or(u32::MAX);
            }
        }
        for (index, entry) in self.pages.iter_mut().enumerate() {
            entry.background = entry.background.map(|background| PageId {
                index: remap
                    .get(background.index as usize)
                    .copied()
                    .unwrap_or(background.index),
            });
            let id = PageId {
                index: u32::try_from(index).unwrap_or(u32::MAX),
            };
            entry.id = id;
        }
        Ok(())
    }

    /// Adds a master to the master list and returns its id.
    ///
    /// Keeps: the master's shapes must already be in the arena and are
    /// linked as the master's top-level shapes; `visio_id` is assigned
    /// when the given one is 0.
    pub fn add_master(&mut self, mut master: Master) -> Result<MasterId, Error> {
        if self.masters.len() >= MAX_LIST {
            return Err(Error::TooManyShapes);
        }
        for shape in &master.shapes {
            if self.shape(*shape).is_none() {
                return Err(Error::UnknownShape(*shape));
            }
        }
        if master.visio_id == 0 {
            master.visio_id = next_id_of(self.masters.iter().map(|entry| entry.visio_id))?;
        }
        let taken: BTreeSet<&str> = self
            .masters
            .iter()
            .map(|entry| entry.name_u.as_str())
            .collect();
        master.name_u = unique_name_u(&master.name_u, &taken);
        let index = u32::try_from(self.masters.len()).map_err(|_| Error::TooManyShapes)?;
        master.id = MasterId { index };
        self.masters.push(master);
        Ok(MasterId { index })
    }

    /// Adds an instance of `master` to `page` at pin position `at` (page
    /// inches) with the given size (`None`: the master's own size, the
    /// P4-06 deferred item) and returns the new shape.
    ///
    /// Keeps: the instance's `Master` link, `PinX`/`PinY`/`Width`/
    /// `Height`/`LocPinX`/`LocPinY` cells (literal values plus the
    /// documented `LocPinX = Width*0.5` formulas, ADR 0038). The instance
    /// geometry stays the master's rows (row union, ADR 0019) until
    /// resized.
    pub fn instantiate_master(
        &mut self,
        master: MasterId,
        page: PageId,
        at: Point,
        size: Option<Size>,
    ) -> Result<ShapeId, Error> {
        let entity = self.master(master).ok_or(Error::UnknownMaster(master))?;
        if self.page(page).is_none() {
            return Err(Error::UnknownPage(page));
        }
        if !at.x.is_finite() || !at.y.is_finite() {
            return Err(Error::BadCell("non-finite pin position"));
        }
        let size = match size {
            Some(size) => {
                if !size.width.is_finite()
                    || !size.height.is_finite()
                    || size.width < 0.0
                    || size.height < 0.0
                {
                    return Err(Error::BadCell("negative or non-finite size"));
                }
                size
            }
            None => self.master_size(master)?,
        };
        let name = entity.name.clone();
        let name_u = entity.name_u.clone();
        let mut shape = Shape::new(ShapeId { index: 0 }, 0);
        shape.name = name.clone();
        // The taken `NameU`s are indexed once per instance (task P5-19
        // review F10: no per-drop clone of every arena `name_u`).
        let taken: BTreeSet<&str> = self
            .shapes
            .iter()
            .map(|entry| entry.name_u.as_str())
            .collect();
        shape.name_u = unique_name_u(&name_u, &taken);
        shape.master = Some(master);
        shape.sheet.cells = vec![
            number_cell("Width", size.width),
            number_cell("Height", size.height),
            number_cell("PinX", at.x),
            number_cell("PinY", at.y),
            Cell {
                name: CellName::from_static("LocPinX"),
                formula: Some("Width*0.5".to_owned()),
                value: Some(CellValue::Number(size.width * 0.5)),
                unit: Unit::Inches,
                preserved: Preserved::new(),
            },
            Cell {
                name: CellName::from_static("LocPinY"),
                formula: Some("Height*0.5".to_owned()),
                value: Some(CellValue::Number(size.height * 0.5)),
                unit: Unit::Inches,
                preserved: Preserved::new(),
            },
        ];
        self.insert_shape(page, shape, None)
    }

    /// Records one `Connect` (glue) on the page (ADR 0045).
    ///
    /// Keeps: one record per connector endpoint; a repeat for the same
    /// endpoint replaces the old record instead of adding a second (ADR
    /// 0039 invariant 5).
    pub fn add_connect(&mut self, page: PageId, connect: Connect) -> Result<(), Error> {
        let entity = self
            .pages
            .get_mut(page.index as usize)
            .ok_or(Error::UnknownPage(page))?;
        match entity.connects.iter_mut().find(|existing| {
            existing.from_sheet == connect.from_sheet
                && existing.from_cell == connect.from_cell
                && existing.from_part == connect.from_part
        }) {
            Some(slot) => *slot = connect,
            None => entity.connects.push(connect),
        }
        Ok(())
    }

    /// Removes every `Connect` record naming `shape` and returns them.
    pub fn remove_connects(&mut self, page: PageId, shape: ShapeId) -> Result<Vec<Connect>, Error> {
        let visio_id = self
            .shape(shape)
            .ok_or(Error::UnknownShape(shape))?
            .visio_id;
        let entity = self
            .pages
            .get_mut(page.index as usize)
            .ok_or(Error::UnknownPage(page))?;
        let mut removed = Vec::new();
        let mut kept = Vec::with_capacity(entity.connects.len());
        for connect in entity.connects.drain(..) {
            if connect.from_sheet == visio_id || connect.to_sheet == visio_id {
                removed.push(connect);
            } else {
                kept.push(connect);
            }
        }
        entity.connects = kept;
        Ok(removed)
    }

    /// The connector's glued endpoints: `(begin target, end target)` as
    /// `Connect` records, `None` for an unglued end.
    pub fn connects_of(
        &self,
        page: PageId,
        connector: ShapeId,
    ) -> Result<[Option<Connect>; 2], Error> {
        let visio_id = self
            .shape(connector)
            .ok_or(Error::UnknownShape(connector))?
            .visio_id;
        let entity = self
            .pages
            .get(page.index as usize)
            .ok_or(Error::UnknownPage(page))?;
        let mut ends: [Option<Connect>; 2] = [None, None];
        for connect in &entity.connects {
            if connect.from_sheet != visio_id {
                continue;
            }
            let slot = if connect.from_part == VIS_END
                || connect.from_part == VIS_END_X
                || connect.from_part == VIS_END_Y
                || connect.from_cell.starts_with("End")
            {
                1
            } else {
                0
            };
            if let Some(end) = ends.get_mut(slot)
                && end.is_none()
            {
                *end = Some(connect.clone());
            }
        }
        Ok(ends)
    }

    /// Groups the shapes into a new group shape on their page (the
    /// ADR 0019 group semantics) and returns the group's id.
    ///
    /// Keeps: z-order of the members relative to each other; the group
    /// takes the members' bounding box as its `Width`/`Height` (and its
    /// pin at the box centre, with the documented `LocPinX = Width*0.5`
    /// formulas, so the group's own transform is the identity offset and
    /// the members' absolute positions do not move); an empty `shapes`
    /// slice or a mixed-page list is an `Err`.
    pub fn group_shapes(&mut self, page: PageId, shapes: &[ShapeId]) -> Result<ShapeId, Error> {
        if self.page(page).is_none() {
            return Err(Error::UnknownPage(page));
        }
        if shapes.is_empty() {
            return Err(Error::BadCell("no shapes to group"));
        }
        let mut members: Vec<ShapeId> = Vec::new();
        for id in shapes {
            if self.shape(*id).is_none() {
                return Err(Error::UnknownShape(*id));
            }
            if self.page_of(*id) != Some(page) {
                return Err(Error::NotOnPage(*id, page));
            }
            if !members.contains(id) {
                members.push(*id);
            }
        }
        members.sort_by_key(|id| self.walk_position(page, *id));
        let frames: Vec<(ShapeId, Frame)> = {
            let mut frames = Vec::new();
            for id in &members {
                frames.push((*id, self.frame_of(*id)?));
            }
            frames
        };
        // The members' bounding box in page space.
        let mut min = Point::new(f64::INFINITY, f64::INFINITY);
        let mut max = Point::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
        for (_, frame) in &frames {
            let transform = frame.transform();
            for corner in [
                Point::new(0.0, 0.0),
                Point::new(frame.size.width, 0.0),
                Point::new(0.0, frame.size.height),
                Point::new(frame.size.width, frame.size.height),
            ] {
                let point = transform.apply(corner);
                min = Point::new(min.x.min(point.x), min.y.min(point.y));
                max = Point::new(max.x.max(point.x), max.y.max(point.y));
            }
        }
        if !min.x.is_finite() || !min.y.is_finite() || !max.x.is_finite() || !max.y.is_finite() {
            return Err(Error::BadCell("non-finite bounding box"));
        }
        let width = (max.x - min.x).max(0.0);
        let height = (max.y - min.y).max(0.0);
        let center = Point::new(min.x + width * 0.5, min.y + height * 0.5);
        // Every derived value is computed and checked before the first
        // mutation (ADR 0039 amendment: a failed mutation changes
        // nothing): extreme member pins make `max.x - min.x` overflow
        // to infinity, and the group's cells and the members' re-based
        // pins must never hold a non-finite number.
        if !width.is_finite()
            || !height.is_finite()
            || !center.x.is_finite()
            || !center.y.is_finite()
        {
            return Err(Error::BadCell("non-finite group extent"));
        }
        // The members' new pins (page position minus the box minimum),
        // checked up front: the loop below only writes what this
        // computed, so no write can fail and leave a half-built group.
        let mut rebase: Vec<(ShapeId, Point)> = Vec::with_capacity(frames.len());
        for (id, frame) in &frames {
            let pin = Point::new(frame.pin.x - min.x, frame.pin.y - min.y);
            if !pin.x.is_finite() || !pin.y.is_finite() {
                return Err(Error::BadCell("non-finite member position"));
            }
            rebase.push((*id, pin));
        }

        // The group's frame is the identity offset (its pin minus its
        // local pin is the box minimum), so a member's page position is
        // its group-local position plus `min`.
        let mut group = Shape::new(ShapeId { index: 0 }, 0);
        group.sheet.cells = vec![
            number_cell("Width", width),
            number_cell("Height", height),
            number_cell("PinX", center.x),
            number_cell("PinY", center.y),
            Cell {
                name: CellName::from_static("LocPinX"),
                formula: Some("Width*0.5".to_owned()),
                value: Some(CellValue::Number(width * 0.5)),
                unit: Unit::Inches,
                preserved: Preserved::new(),
            },
            Cell {
                name: CellName::from_static("LocPinY"),
                formula: Some("Height*0.5".to_owned()),
                value: Some(CellValue::Number(height * 0.5)),
                unit: Unit::Inches,
                preserved: Preserved::new(),
            },
        ];
        // The group is inserted first, with the members still on the
        // page: the page's next free Visio `ID` must count them.
        let group = self.insert_shape(page, group, None)?;
        for (id, pin) in &rebase {
            self.unlist(*id);
            if let Some(entity) = self.shape_mut(*id) {
                entity.parent = Some(group);
            }
            if let Some(entity) = self.shape_mut(group) {
                entity.children.push(*id);
            }
            for (cell, write) in [
                (
                    "PinX",
                    CellWrite::new(literal(pin.x), CellValue::Number(pin.x)),
                ),
                (
                    "PinY",
                    CellWrite::new(literal(pin.y), CellValue::Number(pin.y)),
                ),
            ] {
                self.write_cell(SheetRef::Shape(*id), &CellName::from_static(cell), &write)?;
            }
        }
        Ok(group)
    }

    /// Dissolves the group: its children become top-level shapes (or
    /// children of the grandparent group) in their old order, and the
    /// group shape is detached. Returns the former children's ids.
    ///
    /// Each child's pin keeps its page position: it is mapped through the
    /// dissolved group's transform, and its rotation and flips absorb the
    /// group's. The `Connects` records naming the group itself go with
    /// it (the caller only gets the children's ids back).
    pub fn ungroup_shape(&mut self, group: ShapeId) -> Result<Vec<ShapeId>, Error> {
        let entity = self.shape(group).ok_or(Error::UnknownShape(group))?;
        let parent = entity.parent;
        let children = entity.children.clone();
        let frames: Vec<(ShapeId, Frame)> = {
            let mut frames = Vec::new();
            for id in &children {
                frames.push((*id, self.frame_of(*id)?));
            }
            frames
        };
        let group_frame = self.frame_of(group)?;
        // Every derived value is computed and checked before the first
        // mutation (ADR 0039 amendment: a failed mutation changes
        // nothing): the writes below only repeat what this computed, so
        // a mid-loop failure cannot leave some members re-based and the
        // group intact.
        let mut rebase: Vec<(ShapeId, f64, f64, f64, bool, bool)> =
            Vec::with_capacity(frames.len());
        for (id, frame) in &frames {
            let pin = group_frame.transform().apply(frame.pin);
            let linear = group_frame.linear.compose(frame.linear);
            if !pin.x.is_finite() || !pin.y.is_finite() || !linear.angle.is_finite() {
                return Err(Error::BadCell("non-finite member transform"));
            }
            rebase.push((
                *id,
                pin.x,
                pin.y,
                linear.angle,
                linear.flip_x,
                linear.flip_y,
            ));
        }
        for (id, pin_x, pin_y, angle, flip_x, flip_y) in &rebase {
            let (pin_x, pin_y, angle) = (*pin_x, *pin_y, *angle);
            let (flip_x, flip_y) = (*flip_x, *flip_y);
            for (cell, write) in [
                (
                    "PinX",
                    CellWrite::new(literal(pin_x), CellValue::Number(pin_x)),
                ),
                (
                    "PinY",
                    CellWrite::new(literal(pin_y), CellValue::Number(pin_y)),
                ),
                (
                    "Angle",
                    CellWrite::new(literal(angle), CellValue::Number(angle)),
                ),
                (
                    "FlipX",
                    CellWrite::new(
                        if flip_x { "TRUE" } else { "FALSE" },
                        CellValue::Bool(flip_x),
                    ),
                ),
                (
                    "FlipY",
                    CellWrite::new(
                        if flip_y { "TRUE" } else { "FALSE" },
                        CellValue::Bool(flip_y),
                    ),
                ),
            ] {
                self.write_cell(SheetRef::Shape(*id), &CellName::from_static(cell), &write)?;
            }
        }
        // The children leave the group first, so the detach only takes
        // the group itself (and only its own `Connects`) away.
        for id in &children {
            self.unlist(*id);
        }
        let detached = self.detach_shape(group)?;
        for (count, id) in children.iter().enumerate() {
            match parent {
                Some(owner) => {
                    if let Some(entity) = self.shape_mut(owner) {
                        let at = detached
                            .index
                            .saturating_add(count)
                            .min(entity.children.len());
                        entity.children.insert(at, *id);
                    }
                    if let Some(entity) = self.shape_mut(*id) {
                        entity.parent = Some(owner);
                    }
                }
                None => {
                    if let Some(page) = self.pages.get_mut(detached.page.index as usize) {
                        let at = detached.index.saturating_add(count).min(page.shapes.len());
                        page.shapes.insert(at, *id);
                    }
                    if let Some(entity) = self.shape_mut(*id) {
                        entity.parent = None;
                    }
                }
            }
        }
        Ok(children)
    }

    /// Writes the connector's geometry rows as one path (the routed
    /// points, in page inches), as literal rows (ADR 0045: the app
    /// re-routes; a formula engine may replace this later without a
    /// migration, ADR 0038).
    ///
    /// The page points are mapped into the connector's own local box (the
    /// inverse of its shape transform) before they are written; the path
    /// lands in the shape's own `Geometry` section (the row union rules of
    /// ADR 0019).
    ///
    /// The routed path **replaces** the connector's geometry: the rows go
    /// into the first `Geometry` section and any further `Geometry`
    /// sections are dropped (task P5-19 review F7, recorded in ADR 0045).
    /// A connector is one path; a stale second path must not survive a
    /// re-route. A test pins the collapse.
    pub fn write_connector_path(
        &mut self,
        connector: ShapeId,
        path: &[Point],
    ) -> Result<(), Error> {
        if self.shape(connector).is_none() {
            return Err(Error::UnknownShape(connector));
        }
        if path.is_empty() {
            return Err(Error::BadCell("empty connector path"));
        }
        if path.len() > MAX_PATH_POINTS {
            return Err(Error::BadCell("connector path too long"));
        }
        for point in path {
            if !point.x.is_finite() || !point.y.is_finite() {
                return Err(Error::BadCell("non-finite path point"));
            }
        }
        let frame = self.frame_of(connector)?;
        let inverse = frame
            .transform()
            .invert()
            .map_err(|_| Error::BadCell("singular shape transform"))?;
        let mut rows = Vec::with_capacity(path.len());
        for (position, point) in path.iter().enumerate() {
            let local = inverse.apply(*point);
            let mut row = Row::new();
            row.row_type = Some(if position == 0 { "MoveTo" } else { "LineTo" }.to_owned());
            row.ix = Some(i32::try_from(position).unwrap_or(i32::MAX));
            row.cells = vec![
                row_number_cell("X", local.x, Unit::Inches),
                row_number_cell("Y", local.y, Unit::Inches),
            ];
            rows.push(row);
        }
        let mut sections = vec![Section::new(SectionName::from_static("Geometry"))];
        if let Some(section) = sections.first_mut() {
            section.rows = rows;
        }
        if let Some(entity) = self.shape_mut(connector) {
            replace_geometry_sections(&mut entity.sheet.sections, sections);
        }
        Ok(())
    }

    /// The next free Visio `ID` on the page (the largest in use plus one).
    ///
    /// The walk covers the page's whole shape tree (sub-shape ids are
    /// page-unique) and is bounded; a page with no shapes starts at 1.
    pub fn next_visio_id(&self, page: PageId) -> Result<i32, Error> {
        let entity = self.page(page).ok_or(Error::UnknownPage(page))?;
        let mut ids = Vec::new();
        let mut stack: Vec<ShapeId> = entity.shapes.clone();
        let mut seen: HashSet<ShapeId> = HashSet::new();
        while let Some(id) = stack.pop() {
            if seen.len() >= MAX_WALK || !seen.insert(id) {
                continue;
            }
            if let Some(shape) = self.shape(id) {
                ids.push(shape.visio_id);
                stack.extend(shape.children.iter().copied());
            }
        }
        next_id_of(ids.into_iter())
    }
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

/// The `Geometry` sections' indices of one sheet, in order.
fn geometry_positions(sections: &[Section]) -> Vec<usize> {
    sections
        .iter()
        .enumerate()
        .filter(|(_, section)| section.name.as_str() == "Geometry")
        .map(|(index, _)| index)
        .collect()
}

/// Replaces the sheet's `Geometry` sections with `fresh`: existing
/// sections keep their place (and their preserved content and section
/// cells), trailing ones the new list does not cover are dropped, and
/// extra new sections land at the end.
fn replace_geometry_sections(sections: &mut Vec<Section>, fresh: Vec<Section>) {
    let positions = geometry_positions(sections);
    let total = fresh.len();
    let mut incoming = fresh.into_iter();
    for (count, position) in positions.iter().enumerate() {
        if count >= total {
            break;
        }
        if let (Some(section), Some(slot)) = (incoming.next(), sections.get_mut(*position)) {
            let mut merged = section;
            merged.preserved = std::mem::take(&mut slot.preserved);
            merged.cells = std::mem::take(&mut slot.cells);
            *slot = merged;
        }
    }
    for section in incoming {
        sections.push(section);
    }
    if positions.len() > total {
        let mut seen = 0;
        sections.retain(|section| {
            if section.name.as_str() != "Geometry" {
                return true;
            }
            seen += 1;
            seen <= total
        });
    }
}

impl Document {
    /// The mutation-count room check (AGENTS.md section 6).
    fn check_room(&self) -> Result<(), Error> {
        if self.shapes.len() >= MAX_SHAPES {
            return Err(Error::TooManyShapes);
        }
        Ok(())
    }

    /// The clamped insertion index of a page list.
    fn insert_index(&self, page: PageId, index: Option<usize>) -> Result<usize, Error> {
        let entity = self.page(page).ok_or(Error::UnknownPage(page))?;
        Ok(index
            .unwrap_or(entity.shapes.len())
            .min(entity.shapes.len()))
    }

    /// The root's arena ids: the root first, then its descendants
    /// (arena order), bounded and cycle-safe. The visited set is a
    /// `HashSet` (task P5-19 review F10: a `Vec::contains` walk is
    /// O(n²) in a big subtree).
    fn subtree_ids(&self, root: ShapeId) -> Vec<ShapeId> {
        let mut ids = vec![root];
        let mut stack = vec![root];
        let mut seen: HashSet<ShapeId> = HashSet::from([root]);
        while let Some(id) = stack.pop() {
            if seen.len() >= MAX_WALK {
                break;
            }
            if let Some(shape) = self.shape(id) {
                for child in &shape.children {
                    if seen.insert(*child) {
                        ids.push(*child);
                        stack.push(*child);
                    }
                }
            }
        }
        let mut descendants = ids.split_off(1);
        descendants.sort();
        ids.extend(descendants);
        ids
    }

    /// Whether `id` is a descendant of `ancestor` (bounded, cycle-safe).
    fn is_descendant(&self, id: ShapeId, ancestor: ShapeId) -> bool {
        let mut current = id;
        let mut steps = 0;
        while steps < MAX_WALK {
            match self.shape(current).and_then(|shape| shape.parent) {
                Some(parent) if parent == ancestor => return true,
                Some(parent) => current = parent,
                None => return false,
            }
            steps += 1;
        }
        false
    }

    /// Removes `shape` from whichever list holds it (page or parent).
    fn unlist(&mut self, shape: ShapeId) {
        let parent = self.shape(shape).and_then(|entity| entity.parent);
        match parent {
            Some(owner) => {
                if let Some(entity) = self.shape_mut(owner) {
                    entity.children.retain(|id| *id != shape);
                }
            }
            None => {
                for page in &mut self.pages {
                    page.shapes.retain(|id| *id != shape);
                }
            }
        }
    }

    /// The position of `shape` in the page's depth-first draw order
    /// (bounded, cycle-safe), for the z-order rule of `group_shapes`.
    /// The visited set is a `HashSet` (task P5-19 review F10).
    fn walk_position(&self, page: PageId, shape: ShapeId) -> usize {
        let Some(entity) = self.page(page) else {
            return usize::MAX;
        };
        let mut position = 0;
        let mut stack: Vec<ShapeId> = entity.shapes.iter().rev().copied().collect();
        let mut seen: HashSet<ShapeId> = HashSet::new();
        while let Some(id) = stack.pop() {
            if seen.len() >= MAX_WALK || !seen.insert(id) {
                continue;
            }
            if id == shape {
                return position;
            }
            position = position.saturating_add(1);
            if let Some(entry) = self.shape(id) {
                stack.extend(entry.children.iter().rev().copied());
            }
        }
        usize::MAX
    }

    /// The transform cells of one shape, read with the cached-value
    /// policy (ADR 0021).
    fn frame_of(&self, shape: ShapeId) -> Result<Frame, Error> {
        if self.shape(shape).is_none() {
            return Err(Error::UnknownShape(shape));
        }
        let mut reader = CellReader::new(self);
        let sheet = SheetRef::Shape(shape);
        let number = |reader: &mut CellReader<'_>, name: &str| -> Result<f64, Error> {
            reader.number(sheet, &CellName::new(name))
        };
        let flag = |reader: &mut CellReader<'_>, name: &str| -> Result<bool, Error> {
            reader.boolean(sheet, &CellName::new(name))
        };
        let pin = Point::new(number(&mut reader, "PinX")?, number(&mut reader, "PinY")?);
        let loc_pin = Point::new(
            number(&mut reader, "LocPinX")?,
            number(&mut reader, "LocPinY")?,
        );
        let linear = Linear {
            angle: number(&mut reader, "Angle")?,
            flip_x: flag(&mut reader, "FlipX")?,
            flip_y: flag(&mut reader, "FlipY")?,
        };
        let size = Size::new(
            number(&mut reader, "Width")?,
            number(&mut reader, "Height")?,
        );
        Ok(Frame {
            pin,
            loc_pin,
            linear,
            size,
        })
    }

    /// The size of a master's own shape (the `Width`/`Height` the master
    /// documents, ADR 0034), or [`Error::BadCell`] when it has none or
    /// documents a non-finite one.
    ///
    /// The non-finite check runs on the shape's own cached entries,
    /// before the cached-value policy (ADR 0021) folds a NaN into `0`:
    /// a master that documents a NaN size is an error, not a zero-sized
    /// instance (task P5-19 coverage: the guard is reachable and has a
    /// test).
    fn master_size(&self, master: MasterId) -> Result<Size, Error> {
        let shape = self
            .master(master)
            .and_then(|entity| entity.shapes.first().copied())
            .ok_or(Error::BadCell("master has no shapes"))?;
        let documents_bad = self.shape(shape).is_some_and(|entity| {
            ["Width", "Height"].iter().any(|name| {
                entity.sheet.cells.iter().any(|cell| {
                    cell.name.as_str() == *name
                        && matches!(cell.value, Some(CellValue::Number(value)) if !value.is_finite())
                })
            })
        });
        let frame = self.frame_of(shape)?;
        if documents_bad || !frame.size.width.is_finite() || !frame.size.height.is_finite() {
            return Err(Error::BadCell("non-finite master size"));
        }
        Ok(frame.size)
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    /// A document with one page.
    fn document() -> (Document, PageId) {
        let mut doc = Document::new();
        let page = doc.add_page("Page-1").expect("page");
        (doc, page)
    }

    /// A rectangle shape with `Width`/`Height` transform cells.
    fn box_shape(width: f64, height: f64) -> Shape {
        let mut shape = Shape::new(ShapeId { index: 0 }, 0);
        shape.sheet.cells = vec![
            number_cell("Width", width),
            number_cell("Height", height),
            number_cell("PinX", width * 0.5),
            number_cell("PinY", height * 0.5),
        ];
        shape
    }

    #[test]
    fn cell_write_holds_formula_and_value() {
        let write = CellWrite::new("2.5", CellValue::Number(2.5));
        assert_eq!(write.formula, "2.5");
        assert_eq!(write.value, CellValue::Number(2.5));
    }

    #[test]
    fn detached_shapes_hold_the_subtree() {
        let detached = DetachedShapes {
            page: PageId { index: 0 },
            parent: None,
            index: 1,
            shapes: Vec::new(),
            connects: Vec::new(),
        };
        assert_eq!(detached.index, 1);
        assert!(detached.shapes.is_empty());
    }

    #[test]
    fn insert_shape_assigns_ids_and_clamps_the_index() {
        let (mut doc, page) = document();
        let first = doc
            .insert_shape(page, box_shape(1.0, 1.0), None)
            .expect("first");
        let second = doc
            .insert_shape(page, box_shape(1.0, 1.0), Some(99))
            .expect("second");
        assert_eq!(first, ShapeId { index: 0 });
        assert_eq!(second, ShapeId { index: 1 });
        assert_eq!(doc.shape(first).expect("shape").visio_id, 1);
        assert_eq!(doc.shape(second).expect("shape").visio_id, 2);
        assert_eq!(doc.page(page).expect("page").shapes, vec![first, second]);
        let back = doc
            .insert_shape(page, box_shape(1.0, 1.0), Some(0))
            .expect("third");
        assert_eq!(
            doc.page(page).expect("page").shapes,
            vec![back, first, second]
        );
    }

    #[test]
    fn insert_shape_rejects_a_stale_page() {
        let (mut doc, _) = document();
        let stale = PageId { index: 9 };
        assert_eq!(
            doc.insert_shape(stale, box_shape(1.0, 1.0), None),
            Err(Error::UnknownPage(stale))
        );
    }

    #[test]
    fn insert_subshape_links_parent_and_child() {
        let (mut doc, page) = document();
        let parent = doc
            .insert_shape(page, box_shape(2.0, 2.0), None)
            .expect("p");
        let child = doc
            .insert_subshape(parent, box_shape(1.0, 1.0), None)
            .expect("child");
        assert_eq!(doc.shape(child).expect("child").parent, Some(parent));
        assert_eq!(doc.shape(parent).expect("parent").children, vec![child]);
        assert_eq!(doc.shape(child).expect("child").visio_id, 2);
        let stale = ShapeId { index: 9 };
        assert_eq!(
            doc.insert_subshape(stale, box_shape(1.0, 1.0), None),
            Err(Error::UnknownShape(stale))
        );
    }

    #[test]
    fn detach_and_restore_keep_arena_ids_stable() {
        let (mut doc, page) = document();
        let root = doc
            .insert_shape(page, box_shape(1.0, 1.0), None)
            .expect("r");
        let child = doc
            .insert_subshape(root, box_shape(1.0, 1.0), None)
            .expect("c");
        let detached = doc.detach_shape(root).expect("detach");
        assert_eq!(detached.shapes.len(), 2);
        assert!(doc.page(page).expect("page").shapes.is_empty());
        assert_eq!(doc.page_of(root), None);
        // The arena still holds both shapes at their ids.
        assert!(doc.shape(root).is_some());
        assert!(doc.shape(child).is_some());
        let restored = doc.restore_shapes(detached).expect("restore");
        assert_eq!(restored, root);
        assert_eq!(doc.page(page).expect("page").shapes, vec![root]);
        assert_eq!(doc.shape(root).expect("root").children, vec![child]);
    }

    #[test]
    fn detach_rejects_a_stale_or_unattached_shape() {
        let (mut doc, page) = document();
        let stale = ShapeId { index: 9 };
        assert_eq!(doc.detach_shape(stale), Err(Error::UnknownShape(stale)));
        let root = doc
            .insert_shape(page, box_shape(1.0, 1.0), None)
            .expect("r");
        let _ = doc.detach_shape(root).expect("detach");
        // A detached shape is not in any list: detaching again is an error.
        assert_eq!(doc.detach_shape(root), Err(Error::UnknownShape(root)));
    }

    #[test]
    fn reparent_rejects_cycles_and_clamps_the_index() {
        let (mut doc, page) = document();
        let parent = doc
            .insert_shape(page, box_shape(2.0, 2.0), None)
            .expect("p");
        let child = doc
            .insert_subshape(parent, box_shape(1.0, 1.0), None)
            .expect("c");
        assert_eq!(
            doc.reparent_shape(parent, Some(child), 0),
            Err(Error::WouldCycle)
        );
        assert_eq!(
            doc.reparent_shape(parent, Some(parent), 0),
            Err(Error::WouldCycle)
        );
        let other = doc
            .insert_shape(page, box_shape(1.0, 1.0), None)
            .expect("o");
        doc.reparent_shape(child, Some(other), 99).expect("move");
        assert_eq!(doc.shape(child).expect("child").parent, Some(other));
        assert_eq!(doc.shape(other).expect("other").children, vec![child]);
        assert!(doc.shape(parent).expect("parent").children.is_empty());
        doc.reparent_shape(child, None, 0).expect("top");
        assert_eq!(doc.shape(child).expect("child").parent, None);
        assert_eq!(
            doc.page(page).expect("page").shapes,
            vec![child, parent, other]
        );
    }

    #[test]
    fn set_z_order_clamps_and_rejects_a_shape_of_another_page() {
        let (mut doc, page) = document();
        let first = doc
            .insert_shape(page, box_shape(1.0, 1.0), None)
            .expect("1");
        let second = doc
            .insert_shape(page, box_shape(1.0, 1.0), None)
            .expect("2");
        doc.set_z_order(page, second, 0).expect("z");
        assert_eq!(doc.page(page).expect("page").shapes, vec![second, first]);
        doc.set_z_order(page, second, 99).expect("z");
        assert_eq!(doc.page(page).expect("page").shapes, vec![first, second]);
        let other = doc.add_page("Page-2").expect("p2");
        assert_eq!(
            doc.set_z_order(other, first, 0),
            Err(Error::NotOnPage(first, other))
        );
    }

    #[test]
    fn a_fields_display_text_is_written_inside_its_element() {
        // Regression (P8-10): written after the marker, the display text
        // merged with the text that follows it through the XML round
        // trip. Visio writes it inside the `fld` element and the reader
        // maps it from there.
        let (mut doc, page) = document();
        let shape = doc
            .insert_shape(page, box_shape(1.0, 1.0), None)
            .expect("s");
        let mut text = TextContent::new();
        text.parts.push(TextPart::Field { ix: 0 });
        text.parts.push(TextPart::Text("1".to_owned()));
        text.parts.push(TextPart::Text("\n".to_owned()));
        doc.set_shape_text(shape, text).expect("text");
        let preserved = doc.shape(shape).expect("s").preserved.clone();
        let element = preserved
            .children
            .iter()
            .find(|child| child.name.local == "Text")
            .expect("Text element");
        assert_eq!(element.children.len(), 2);
        match &element.children[0] {
            XmlChild::Element(fld) => {
                assert_eq!(fld.name.local, "fld");
                assert!(
                    matches!(fld.children.as_slice(), [XmlChild::Text(literal)] if literal == "1"),
                    "{:?}",
                    fld.children
                );
            }
            other => panic!("expected a fld element, got {other:?}"),
        }
        assert!(matches!(&element.children[1], XmlChild::Text(literal) if literal == "\n"));
    }

    #[test]
    fn set_shape_text_rebuilds_the_text_element() {
        let (mut doc, page) = document();
        let shape = doc
            .insert_shape(page, box_shape(1.0, 1.0), None)
            .expect("s");
        let mut text = TextContent::new();
        text.parts.push(TextPart::CharRun { ix: 1 });
        text.parts.push(TextPart::Text("Hello".to_owned()));
        text.parts.push(TextPart::Field { ix: 3 });
        doc.set_shape_text(shape, text.clone()).expect("text");
        assert_eq!(doc.shape(shape).expect("s").text, Some(text));
        let preserved = doc.shape(shape).expect("s").preserved.clone();
        let element = preserved
            .children
            .iter()
            .find(|child| child.name.local == "Text")
            .expect("Text element");
        assert_eq!(element.children.len(), 3);
        assert!(matches!(
            &element.children[1],
            XmlChild::Text(literal) if literal == "Hello"
        ));
        // A second write replaces the element instead of adding one.
        doc.set_shape_text(shape, TextContent::new())
            .expect("clear");
        let preserved = doc.shape(shape).expect("s").preserved.clone();
        let texts = preserved
            .children
            .iter()
            .filter(|child| child.name.local == "Text")
            .count();
        assert_eq!(texts, 1);
        let stale = ShapeId { index: 9 };
        assert_eq!(
            doc.set_shape_text(stale, TextContent::new()),
            Err(Error::UnknownShape(stale))
        );
    }

    #[test]
    fn write_cell_stores_formula_value_and_unit() {
        let (mut doc, page) = document();
        let shape = doc
            .insert_shape(page, box_shape(1.0, 1.0), None)
            .expect("s");
        doc.write_cell(
            SheetRef::Shape(shape),
            &CellName::from_static("PinX"),
            &CellWrite::new("2.5", CellValue::Number(2.5)),
        )
        .expect("write");
        let cell = doc
            .shape(shape)
            .expect("s")
            .sheet
            .cells
            .iter()
            .find(|cell| cell.name.as_str() == "PinX")
            .expect("PinX");
        assert_eq!(cell.formula.as_deref(), Some("2.5"));
        assert_eq!(cell.value, Some(CellValue::Number(2.5)));
        assert_eq!(cell.unit, Unit::Inches);
    }

    #[test]
    fn write_cell_updates_the_documented_default_cells() {
        let (mut doc, page) = document();
        let shape = doc
            .insert_shape(page, box_shape(1.0, 1.0), None)
            .expect("s");
        doc.write_cell(
            SheetRef::Shape(shape),
            &CellName::from_static("Width"),
            &CellWrite::new("4", CellValue::Number(4.0)),
        )
        .expect("write");
        let sheet = &doc.shape(shape).expect("s").sheet;
        let find = |name: &str| {
            sheet
                .cells
                .iter()
                .find(|cell| cell.name.as_str() == name)
                .cloned()
        };
        let loc_pin_x = find("LocPinX").expect("LocPinX");
        assert_eq!(loc_pin_x.formula.as_deref(), Some("Width*0.5"));
        assert_eq!(loc_pin_x.value, Some(CellValue::Number(2.0)));
        let txt_width = find("TxtWidth").expect("TxtWidth");
        assert_eq!(txt_width.formula.as_deref(), Some("Width*1"));
        assert_eq!(txt_width.value, Some(CellValue::Number(4.0)));
        let txt_pin_x = find("TxtPinX").expect("TxtPinX");
        assert_eq!(txt_pin_x.value, Some(CellValue::Number(2.0)));
        // The height-derived cells are untouched.
        assert!(find("LocPinY").is_none());
    }

    #[test]
    fn write_cell_keeps_a_custom_derived_cell() {
        let (mut doc, page) = document();
        let shape = doc
            .insert_shape(page, box_shape(1.0, 1.0), None)
            .expect("s");
        doc.write_cell(
            SheetRef::Shape(shape),
            &CellName::from_static("LocPinX"),
            &CellWrite::new("0.25", CellValue::Number(0.25)),
        )
        .expect("custom");
        doc.write_cell(
            SheetRef::Shape(shape),
            &CellName::from_static("Width"),
            &CellWrite::new("4", CellValue::Number(4.0)),
        )
        .expect("write");
        let sheet = &doc.shape(shape).expect("s").sheet;
        let loc_pin_x = sheet
            .cells
            .iter()
            .find(|cell| cell.name.as_str() == "LocPinX")
            .expect("LocPinX");
        assert_eq!(loc_pin_x.value, Some(CellValue::Number(0.25)));
    }

    #[test]
    fn write_cell_rejects_a_non_finite_number() {
        let (mut doc, page) = document();
        let shape = doc
            .insert_shape(page, box_shape(1.0, 1.0), None)
            .expect("s");
        assert_eq!(
            doc.write_cell(
                SheetRef::Shape(shape),
                &CellName::from_static("PinX"),
                &CellWrite::new("inf", CellValue::Number(f64::INFINITY)),
            ),
            Err(Error::BadCell("non-finite number"))
        );
        let stale = ShapeId { index: 9 };
        assert_eq!(
            doc.write_cell(
                SheetRef::Shape(stale),
                &CellName::from_static("PinX"),
                &CellWrite::new("1", CellValue::Number(1.0)),
            ),
            Err(Error::UnknownShape(stale))
        );
        let stale = PageId { index: 9 };
        assert_eq!(
            doc.write_cell(
                SheetRef::Page(stale),
                &CellName::from_static("PageWidth"),
                &CellWrite::new("1", CellValue::Number(1.0)),
            ),
            Err(Error::UnknownPage(stale))
        );
    }

    #[test]
    fn write_row_cell_writes_and_reports_a_missing_row() {
        let (mut doc, page) = document();
        let shape = doc
            .insert_shape(page, box_shape(1.0, 1.0), None)
            .expect("s");
        let mut section = Section::new(SectionName::from_static("Geometry"));
        let mut row = Row::new();
        row.row_type = Some("LineTo".to_owned());
        row.ix = Some(0);
        section.rows.push(row);
        doc.shape_mut(shape)
            .expect("shape")
            .sheet
            .sections
            .push(section);
        doc.write_row_cell(
            SheetRef::Shape(shape),
            &SectionName::from_static("Geometry"),
            RowTarget::at(0),
            &CellName::from_static("X"),
            &CellWrite::new("3", CellValue::Number(3.0)),
        )
        .expect("write");
        let row = &doc.shape(shape).expect("s").sheet.sections[0].rows[0];
        assert_eq!(row.cells[0].value, Some(CellValue::Number(3.0)));
        assert_eq!(row.cells[0].unit, Unit::Inches);
        assert_eq!(
            doc.write_row_cell(
                SheetRef::Shape(shape),
                &SectionName::from_static("Geometry"),
                RowTarget::by_ix(7),
                &CellName::from_static("X"),
                &CellWrite::new("3", CellValue::Number(3.0)),
            ),
            Err(Error::BadCell("no such row"))
        );
        assert_eq!(
            doc.write_row_cell(
                SheetRef::Shape(shape),
                &SectionName::from_static("Property"),
                RowTarget::at(0),
                &CellName::from_static("Value"),
                &CellWrite::new("x", CellValue::Str("x".to_owned())),
            ),
            Err(Error::BadCell("no such section"))
        );
    }
    #[test]
    fn add_page_names_are_unique_and_ids_are_allocated() {
        let (mut doc, page) = document();
        let second = doc.add_page("Page").expect("2");
        let third = doc.add_page("Page").expect("3");
        assert_eq!(doc.page(second).expect("p").name_u, "Page");
        assert_eq!(doc.page(third).expect("p").name_u, "Page (2)");
        assert_eq!(doc.page(second).expect("p").visio_id, 2);
        assert_eq!(doc.page(third).expect("p").visio_id, 3);
        assert_eq!(doc.next_visio_id(page).expect("id"), 1);
    }

    #[test]
    fn delete_page_clears_background_references() {
        let (mut doc, page) = document();
        let second = doc.add_page("Page-2").expect("2");
        let third = doc.add_page("Page-3").expect("3");
        doc.pages
            .get_mut(third.index as usize)
            .expect("third")
            .background = Some(second);
        doc.pages
            .get_mut(page.index as usize)
            .expect("first")
            .background = Some(second);
        let removed = doc.delete_page(second).expect("delete");
        assert_eq!(removed.name_u, "Page-2");
        assert_eq!(doc.pages.len(), 2);
        // The reference to the removed page is cleared, the later page id
        // follows the list.
        assert_eq!(doc.pages[0].background, None);
        assert_eq!(doc.pages[1].id, PageId { index: 1 });
        assert_eq!(
            doc.delete_page(PageId { index: 9 }),
            Err(Error::UnknownPage(PageId { index: 9 }))
        );
    }

    #[test]
    fn move_page_remaps_page_ids_and_background_references() {
        let (mut doc, page) = document();
        let second = doc.add_page("Page-2").expect("2");
        let third = doc.add_page("Page-3").expect("3");
        doc.pages
            .get_mut(page.index as usize)
            .expect("first")
            .background = Some(third);
        doc.move_page(2, 0).expect("move");
        assert_eq!(doc.pages[0].id, PageId { index: 0 });
        assert_eq!(doc.pages[0].name_u, "Page-3");
        assert_eq!(doc.pages[1].name_u, "Page-1");
        // The background of "Page-1" still names "Page-3".
        assert_eq!(doc.pages[1].background, Some(PageId { index: 0 }));
        doc.move_page(0, 99).expect("clamp");
        assert_eq!(doc.pages[2].name_u, "Page-3");
        assert_eq!(
            doc.move_page(9, 0),
            Err(Error::UnknownPage(PageId { index: 9 }))
        );
        assert!(doc.page(second).is_some());
    }

    #[test]
    fn rename_page_follows_the_name_u_rule() {
        let (mut doc, page) = document();
        let second = doc.add_page("Other").expect("2");
        doc.rename_page(page, "Other").expect("rename");
        assert_eq!(doc.page(page).expect("p").name, "Other");
        assert_eq!(doc.page(page).expect("p").name_u, "Other (2)");
        assert_eq!(doc.page(second).expect("p").name_u, "Other");
        assert_eq!(
            doc.rename_page(PageId { index: 9 }, "x"),
            Err(Error::UnknownPage(PageId { index: 9 }))
        );
    }

    #[test]
    fn add_master_validates_shapes_and_allocates_ids() {
        let (mut doc, page) = document();
        let content = doc
            .insert_shape(page, box_shape(1.0, 1.0), None)
            .expect("c");
        let mut master = Master::new(MasterId { index: 0 }, 0);
        master.name_u = "Box".to_owned();
        master.shapes.push(content);
        let id = doc.add_master(master).expect("master");
        assert_eq!(id, MasterId { index: 0 });
        assert_eq!(doc.master(id).expect("m").visio_id, 1);
        assert_eq!(doc.master(id).expect("m").name_u, "Box");
        let mut bad = Master::new(MasterId { index: 0 }, 0);
        bad.shapes.push(ShapeId { index: 9 });
        assert_eq!(
            doc.add_master(bad),
            Err(Error::UnknownShape(ShapeId { index: 9 }))
        );
    }

    #[test]
    fn instantiate_master_writes_the_transform_cells() {
        let (mut doc, page) = document();
        let content = doc
            .insert_shape(page, box_shape(2.0, 4.0), None)
            .expect("c");
        let mut master = Master::new(MasterId { index: 0 }, 0);
        master.name_u = "Box".to_owned();
        master.shapes.push(content);
        let master = doc.add_master(master).expect("master");
        let instance = doc
            .instantiate_master(master, page, Point::new(3.0, 5.0), None)
            .expect("instance");
        let shape = doc.shape(instance).expect("shape");
        assert_eq!(shape.master, Some(master));
        assert_eq!(shape.visio_id, 2);
        let find = |name: &str| {
            shape
                .sheet
                .cells
                .iter()
                .find(|cell| cell.name.as_str() == name)
                .cloned()
                .expect(name)
        };
        assert_eq!(find("PinX").value, Some(CellValue::Number(3.0)));
        assert_eq!(find("Width").value, Some(CellValue::Number(2.0)));
        assert_eq!(find("Height").value, Some(CellValue::Number(4.0)));
        assert_eq!(find("LocPinX").formula.as_deref(), Some("Width*0.5"));
        assert_eq!(find("LocPinX").value, Some(CellValue::Number(1.0)));
        assert_eq!(find("LocPinY").value, Some(CellValue::Number(2.0)));
        // An explicit size wins.
        let other = doc
            .instantiate_master(
                master,
                page,
                Point::new(0.0, 0.0),
                Some(Size::new(1.0, 1.0)),
            )
            .expect("instance");
        let cells = &doc.shape(other).expect("shape").sheet.cells;
        let width = cells
            .iter()
            .find(|c| c.name.as_str() == "Width")
            .expect("w");
        assert_eq!(width.value, Some(CellValue::Number(1.0)));
        assert_eq!(
            doc.instantiate_master(MasterId { index: 9 }, page, Point::new(0.0, 0.0), None),
            Err(Error::UnknownMaster(MasterId { index: 9 }))
        );
        assert_eq!(
            doc.instantiate_master(master, page, Point::new(f64::NAN, 0.0), None),
            Err(Error::BadCell("non-finite pin position"))
        );
    }

    #[test]
    fn group_and_ungroup_keep_the_members_positions() {
        let (mut doc, page) = document();
        let first = doc
            .insert_shape(page, box_shape(2.0, 2.0), None)
            .expect("1");
        let second = doc
            .insert_shape(page, box_shape(2.0, 2.0), None)
            .expect("2");
        // Move `second` to (4, 6): its pin is its centre.
        doc.write_cell(
            SheetRef::Shape(second),
            &CellName::from_static("PinX"),
            &CellWrite::new("4", CellValue::Number(4.0)),
        )
        .expect("pin");
        doc.write_cell(
            SheetRef::Shape(second),
            &CellName::from_static("PinY"),
            &CellWrite::new("6", CellValue::Number(6.0)),
        )
        .expect("pin");
        let group = doc.group_shapes(page, &[second, first]).expect("group");
        // The group takes the members' bounding box: (0,0)..(5,7).
        let g = doc.shape(group).expect("group");
        let cell = |name: &str| {
            g.sheet
                .cells
                .iter()
                .find(|c| c.name.as_str() == name)
                .and_then(|c| c.value.clone())
        };
        assert_eq!(cell("Width"), Some(CellValue::Number(5.0)));
        assert_eq!(cell("Height"), Some(CellValue::Number(7.0)));
        assert_eq!(cell("PinX"), Some(CellValue::Number(2.5)));
        assert_eq!(cell("PinY"), Some(CellValue::Number(3.5)));
        assert_eq!(g.children, vec![first, second]);
        // Members keep their page positions: their pins move into the
        // group's local box (the box minimum is (0, 0) here).
        let first_pin = doc
            .shape(first)
            .expect("f")
            .sheet
            .cells
            .iter()
            .find(|c| c.name.as_str() == "PinX")
            .and_then(|c| c.value.clone());
        assert_eq!(first_pin, Some(CellValue::Number(1.0)));
        let second_pin = doc
            .shape(second)
            .expect("s")
            .sheet
            .cells
            .iter()
            .find(|c| c.name.as_str() == "PinX")
            .and_then(|c| c.value.clone());
        assert_eq!(second_pin, Some(CellValue::Number(4.0)));
        // Ungroup puts them back at their page positions.
        let members = doc.ungroup_shape(group).expect("ungroup");
        assert_eq!(members, vec![first, second]);
        assert_eq!(doc.page(page).expect("page").shapes, vec![first, second]);
        let first_pin = doc
            .shape(first)
            .expect("f")
            .sheet
            .cells
            .iter()
            .find(|c| c.name.as_str() == "PinX")
            .and_then(|c| c.value.clone());
        assert_eq!(first_pin, Some(CellValue::Number(1.0)));
    }

    #[test]
    fn group_rejects_an_empty_or_mixed_page_list() {
        let (mut doc, page) = document();
        let first = doc
            .insert_shape(page, box_shape(1.0, 1.0), None)
            .expect("1");
        assert_eq!(
            doc.group_shapes(page, &[]),
            Err(Error::BadCell("no shapes to group"))
        );
        let other = doc.add_page("Page-2").expect("p2");
        let foreign = doc
            .insert_shape(other, box_shape(1.0, 1.0), None)
            .expect("f");
        assert_eq!(
            doc.group_shapes(page, &[first, foreign]),
            Err(Error::NotOnPage(foreign, page))
        );
        assert_eq!(
            doc.group_shapes(PageId { index: 9 }, &[first]),
            Err(Error::UnknownPage(PageId { index: 9 }))
        );
        assert_eq!(
            doc.ungroup_shape(ShapeId { index: 9 }),
            Err(Error::UnknownShape(ShapeId { index: 9 }))
        );
    }

    /// Review F2 (task P5-19): two shapes whose cached pins sit near
    /// ±1e308 make the bounding-box extent overflow to infinity. The
    /// check fires **before** the first mutation (the ADR 0039
    /// amendment "a failed mutation changes nothing"): one `BadCell`,
    /// the document byte-identical.
    #[test]
    fn group_shapes_rejects_an_overflowing_extent_before_mutating() {
        let (mut doc, page) = document();
        let mut left = box_shape(2.0, 1.0);
        left.sheet.cells = vec![
            number_cell("Width", 2.0),
            number_cell("Height", 1.0),
            number_cell("PinX", -1e308),
            number_cell("PinY", 0.0),
        ];
        let mut right = box_shape(2.0, 1.0);
        right.sheet.cells = vec![
            number_cell("Width", 2.0),
            number_cell("Height", 1.0),
            number_cell("PinX", 1e308),
            number_cell("PinY", 0.0),
        ];
        let a = doc.insert_shape(page, left, None).expect("a");
        let b = doc.insert_shape(page, right, None).expect("b");
        let before = doc.clone();
        let error = doc.group_shapes(page, &[a, b]).expect_err("overflow");
        assert!(matches!(error, Error::BadCell(_)), "got {error:?}");
        assert_eq!(doc, before, "a failed mutation changes nothing");
    }

    /// Review F2 (task P5-19) for `ungroup_shape`: the members'
    /// re-based transforms are computed and checked before the first
    /// write; a mid-loop failure cannot leave some members re-based and
    /// the group intact.
    #[test]
    fn ungroup_rejects_an_overflowing_member_transform_before_mutating() {
        let (mut doc, page) = document();
        let mut group = box_shape(2.0, 1.0);
        group.sheet.cells = vec![
            number_cell("Width", 2.0),
            number_cell("Height", 1.0),
            number_cell("PinX", 1e308),
            number_cell("PinY", 0.0),
        ];
        let group = doc.insert_shape(page, group, None).expect("g");
        let mut member = box_shape(1.0, 1.0);
        member.sheet.cells = vec![
            number_cell("Width", 1.0),
            number_cell("Height", 1.0),
            number_cell("PinX", 1e308),
            number_cell("PinY", 0.0),
        ];
        doc.insert_subshape(group, member, None).expect("m");
        let before = doc.clone();
        let error = doc.ungroup_shape(group).expect_err("overflow");
        assert!(matches!(error, Error::BadCell(_)), "got {error:?}");
        assert_eq!(doc, before, "a failed mutation changes nothing");
    }

    #[test]
    fn master_size_rejects_a_non_finite_master_width() {
        let (mut doc, page) = document();
        let mut content = box_shape(1.0, 1.0);
        content.sheet.cells = vec![number_cell("Width", f64::NAN), number_cell("Height", 1.0)];
        let content = doc.insert_shape(page, content, None).expect("c");
        let mut master = Master::new(MasterId { index: 0 }, 0);
        master.name_u = "Bad".to_owned();
        master.shapes.push(content);
        let master = doc.add_master(master).expect("master");
        let error = doc
            .instantiate_master(master, page, Point::new(0.0, 0.0), None)
            .expect_err("non-finite master size");
        assert_eq!(error, Error::BadCell("non-finite master size"));
    }

    #[test]
    fn write_connector_path_maps_page_points_into_the_local_box() {
        let (mut doc, page) = document();
        let connector = doc
            .insert_shape(page, box_shape(2.0, 2.0), None)
            .expect("c");
        doc.write_connector_path(
            connector,
            &[
                Point::new(1.0, 1.0),
                Point::new(2.0, 3.0),
                Point::new(4.0, 1.0),
            ],
        )
        .expect("path");
        let section = &doc.shape(connector).expect("c").sheet.sections[0];
        assert_eq!(section.rows.len(), 3);
        assert_eq!(section.rows[0].row_type.as_deref(), Some("MoveTo"));
        assert_eq!(section.rows[1].row_type.as_deref(), Some("LineTo"));
        // The connector's pin is (1, 1) and its local pin (1, 1): local
        // coordinates equal page coordinates minus the origin offset.
        assert_eq!(section.rows[1].cells[0].value, Some(CellValue::Number(2.0)));
        assert_eq!(section.rows[1].cells[1].value, Some(CellValue::Number(3.0)));
        assert_eq!(
            doc.write_connector_path(connector, &[]),
            Err(Error::BadCell("empty connector path"))
        );
        assert_eq!(
            doc.write_connector_path(connector, &[Point::new(f64::NAN, 0.0)]),
            Err(Error::BadCell("non-finite path point"))
        );
        assert_eq!(
            doc.write_connector_path(ShapeId { index: 9 }, &[Point::new(0.0, 0.0)]),
            Err(Error::UnknownShape(ShapeId { index: 9 }))
        );
    }

    #[test]
    fn next_visio_id_counts_the_whole_tree() {
        let (mut doc, page) = document();
        assert_eq!(doc.next_visio_id(page).expect("id"), 1);
        let root = doc
            .insert_shape(page, box_shape(1.0, 1.0), None)
            .expect("r");
        let child = doc
            .insert_subshape(root, box_shape(1.0, 1.0), None)
            .expect("c");
        assert_eq!(doc.shape(child).expect("c").visio_id, 2);
        assert_eq!(doc.next_visio_id(page).expect("id"), 3);
        assert_eq!(
            doc.next_visio_id(PageId { index: 9 }),
            Err(Error::UnknownPage(PageId { index: 9 }))
        );
    }

    #[test]
    fn unique_names_append_the_counter() {
        let names = ["Box".to_owned(), "Box (2)".to_owned()];
        let taken: BTreeSet<&str> = names.iter().map(String::as_str).collect();
        assert_eq!(unique_name_u("New", &taken), "New");
        assert_eq!(unique_name_u("Box", &taken), "Box (3)");
    }

    #[test]
    fn literal_numbers_round_trip() {
        assert_eq!(literal(2.5), "2.5");
        assert_eq!(literal(-0.0), "0");
        assert_eq!(literal(3.0), "3");
        assert_eq!(literal(0.1 + 0.2), format!("{}", 0.1 + 0.2));
    }

    #[test]
    fn formula_follows_the_documented_spelling() {
        assert!(formula_follows(None, "Width*0.5"));
        assert!(formula_follows(Some("Inh"), "Width*0.5"));
        assert!(formula_follows(Some("= Width * 0.5"), "Width*0.5"));
        assert!(!formula_follows(Some("0.2"), "Width*0.5"));
    }

    #[test]
    fn linear_composition_handles_flips() {
        let turn = Linear {
            angle: 1.0,
            flip_x: false,
            flip_y: false,
        };
        let sum = turn.compose(turn);
        assert!((sum.angle - 2.0).abs() < 1e-9);
        assert!(!sum.flip_x && !sum.flip_y);
        let flipped = Linear {
            angle: 0.0,
            flip_x: true,
            flip_y: false,
        };
        let reversed = flipped.compose(turn);
        assert!((reversed.angle + 1.0).abs() < 1e-9);
        assert!(reversed.flip_x);
        // A flip on both axes composes to a half turn, not to flips.
        let mirror_y = Linear {
            angle: 0.0,
            flip_x: false,
            flip_y: true,
        };
        let half_turn = flipped.compose(mirror_y);
        assert!(!half_turn.flip_x && !half_turn.flip_y);
        assert!((half_turn.angle - std::f64::consts::PI).abs() < 1e-9);
        // A flip with itself is the identity.
        let identity = flipped.compose(flipped);
        assert!(!identity.flip_x && !identity.flip_y);
        assert!(identity.angle.abs() < 1e-9);
    }

    #[test]
    fn derived_cells_are_the_multiplications_of_the_table() {
        assert_eq!(derived_from("Width").len(), 4);
        assert_eq!(derived_from("Height").len(), 4);
        assert!(derived_from("PinX").is_empty());
        let widths: Vec<&str> = derived_from("Width")
            .into_iter()
            .map(|(n, _, _)| n)
            .collect();
        assert_eq!(widths, vec!["LocPinX", "TxtPinX", "TxtLocPinX", "TxtWidth"]);
    }
    /// One scaled-row expectation: the row, its `Row/@T` kind and the
    /// mapped `(name, value, unit)` triples.
    #[test]
    fn text_element_writes_the_run_markers_with_the_template_name() {
        let text = TextContent {
            parts: vec![
                TextPart::Text("a".to_owned()),
                TextPart::CharRun { ix: 1 },
                TextPart::ParaRun { ix: 2 },
                TextPart::TabsRun { ix: 3 },
                TextPart::Field { ix: 4 },
                TextPart::Text("b".to_owned()),
            ],
        };
        let template = QName {
            prefix: Some("v".to_owned()),
            uri: Some("urn:test".to_owned()),
            local: "Text".to_owned(),
        };
        let node = text_element(&text, Some(&template));
        assert_eq!(node.name.prefix.as_deref(), Some("v"));
        assert_eq!(node.name.uri.as_deref(), Some("urn:test"));
        assert_eq!(node.name.local, "Text");
        assert!(!node.self_closing);
        let mut markers = Vec::new();
        let mut literals = Vec::new();
        let mut field_text = Vec::new();
        for child in &node.children {
            if let XmlChild::Text(text) = child {
                literals.push(text.clone());
            }
            if let XmlChild::Element(element) = child {
                assert_eq!(element.name.prefix.as_deref(), Some("v"));
                assert_eq!(element.name.uri.as_deref(), Some("urn:test"));
                let ix = element
                    .attributes
                    .iter()
                    .find(|attribute| attribute.name.local == "IX")
                    .expect("IX attribute");
                markers.push((element.name.local.clone(), ix.value.clone()));
                if element.name.local == "fld" {
                    field_text.extend(element.children.iter().filter_map(|child| match child {
                        XmlChild::Text(text) => Some(text.clone()),
                        _ => None,
                    }));
                }
            }
        }
        assert_eq!(
            literals,
            vec!["a".to_owned()],
            "the field's display text is inside its element"
        );
        assert_eq!(field_text, vec!["b".to_owned()]);
        assert_eq!(
            markers,
            vec![
                ("cp".to_owned(), "1".to_owned()),
                ("pp".to_owned(), "2".to_owned()),
                ("tp".to_owned(), "3".to_owned()),
                ("fld".to_owned(), "4".to_owned()),
            ]
        );
        // An empty text is a self-closing element with the plain local name.
        let empty = text_element(&TextContent::new(), None);
        assert!(empty.self_closing);
        assert_eq!(empty.name, QName::local("Text"));
    }

    #[test]
    fn unique_name_u_falls_back_when_the_range_is_full() {
        let names: Vec<String> = std::iter::once("Box".to_owned())
            .chain((2..10_000u32).map(|n| format!("Box ({n})")))
            .collect();
        let taken: BTreeSet<&str> = names.iter().map(String::as_str).collect();
        assert_eq!(unique_name_u("Box", &taken), "Box");
    }

    #[test]
    fn insert_shape_links_the_children_and_rejects_a_stale_child() {
        let (mut doc, page) = document();
        let child = doc.add_shape(box_shape(1.0, 1.0));
        let mut shape = box_shape(1.0, 1.0);
        shape.children = vec![child];
        let id = doc.insert_shape(page, shape, None).expect("insert");
        assert_eq!(doc.shape(child).expect("child").parent, Some(id));
        assert_eq!(doc.shape(id).expect("shape").children, vec![child]);
        let mut bad = box_shape(1.0, 1.0);
        bad.children = vec![ShapeId { index: 999 }];
        let before = doc.shapes.len();
        assert_eq!(
            doc.insert_shape(page, bad, None),
            Err(Error::UnknownShape(ShapeId { index: 999 }))
        );
        assert_eq!(
            doc.shapes.len(),
            before,
            "a stale child leaves no unnamed arena entry (task P5-19, F11)"
        );
    }

    #[test]
    fn insert_subshape_links_children_and_keeps_a_detached_subtree_id_free() {
        let (mut doc, page) = document();
        let root = doc
            .insert_shape(page, box_shape(1.0, 1.0), None)
            .expect("root");
        let grandchild = doc.add_shape(box_shape(1.0, 1.0));
        let mut shape = box_shape(1.0, 1.0);
        shape.children = vec![grandchild];
        let id = doc
            .insert_subshape(root, shape, Some(0))
            .expect("insert subshape");
        assert_eq!(doc.shape(id).expect("shape").parent, Some(root));
        assert_eq!(doc.shape(grandchild).expect("grandchild").parent, Some(id));
        assert_eq!(doc.shape(root).expect("root").children, vec![id]);
        let mut bad = box_shape(1.0, 1.0);
        bad.children = vec![ShapeId { index: 999 }];
        assert_eq!(
            doc.insert_subshape(root, bad, None),
            Err(Error::UnknownShape(ShapeId { index: 999 }))
        );
        // A parent no page reaches gets no Visio `ID` for its child.
        let detached = doc.detach_shape(root).expect("detach");
        assert_eq!(detached.parent, None);
        let under = doc
            .insert_subshape(root, box_shape(1.0, 1.0), None)
            .expect("under");
        assert_eq!(doc.shape(under).expect("under").visio_id, 0);
        assert_eq!(doc.shape(under).expect("under").parent, Some(root));
    }

    #[test]
    fn page_and_master_lists_stop_at_the_cap() {
        let mut doc = Document::new();
        doc.pages = vec![Page::new(PageId { index: 0 }, 1); MAX_LIST];
        assert_eq!(doc.add_page("P"), Err(Error::TooManyShapes));
        doc.masters = vec![Master::new(MasterId { index: 0 }, 1); MAX_LIST];
        assert_eq!(
            doc.add_master(Master::new(MasterId { index: 0 }, 1)),
            Err(Error::TooManyShapes)
        );
    }

    #[test]
    fn write_row_cell_needs_a_row_and_overwrites_an_existing_cell() {
        let (mut doc, page) = document();
        let mut shape = box_shape(1.0, 1.0);
        let mut section = Section::new(SectionName::from_static("Geometry"));
        let mut row = Row::new();
        row.ix = Some(0);
        row.cells.push(number_cell("X", 1.0));
        section.rows.push(row);
        shape.sheet.sections.push(section);
        let id = doc.insert_shape(page, shape, None).expect("shape");
        let sheet = SheetRef::Shape(id);
        let section_name = SectionName::from_static("Geometry");
        // Neither a position nor an `IX` names no row at all.
        assert_eq!(
            doc.write_row_cell(
                sheet,
                &section_name,
                RowTarget {
                    position: None,
                    ix: None
                },
                &CellName::new("X"),
                &CellWrite::new("2", CellValue::Number(2.0)),
            ),
            Err(Error::BadCell("no such row"))
        );
        // An existing cell is overwritten in place, not added twice.
        doc.write_row_cell(
            sheet,
            &section_name,
            RowTarget::at(0),
            &CellName::new("X"),
            &CellWrite::new("2", CellValue::Number(2.0)),
        )
        .expect("write");
        doc.write_row_cell(
            sheet,
            &section_name,
            RowTarget {
                position: None,
                ix: Some(0),
            },
            &CellName::new("X"),
            &CellWrite::new("3", CellValue::Number(3.0)),
        )
        .expect("write by ix");
        let rows = &doc
            .shape(id)
            .expect("shape")
            .sheet
            .sections
            .first()
            .expect("section")
            .rows;
        assert_eq!(rows.len(), 1);
        let cells = &rows[0].cells;
        assert_eq!(cells.len(), 1);
        assert_eq!(cells[0].formula.as_deref(), Some("3"));
        assert_eq!(cells[0].value, Some(CellValue::Number(3.0)));
    }

    #[test]
    fn write_row_cell_targets_a_page_sheet() {
        let (mut doc, page) = document();
        let mut section = Section::new(SectionName::from_static("Geometry"));
        let mut row = Row::new();
        row.ix = Some(0);
        row.cells.push(number_cell("X", 1.0));
        section.rows.push(row);
        doc.pages
            .get_mut(page.index as usize)
            .expect("page")
            .sheet
            .sections
            .push(section);
        let write = CellWrite::new("2", CellValue::Number(2.0));
        doc.write_row_cell(
            SheetRef::Page(page),
            &SectionName::from_static("Geometry"),
            RowTarget::at(0),
            &CellName::new("X"),
            &write,
        )
        .expect("write");
        let cells = &doc
            .page(page)
            .expect("page")
            .sheet
            .sections
            .first()
            .expect("section")
            .rows[0]
            .cells;
        assert_eq!(cells.len(), 1);
        assert_eq!(cells[0].formula.as_deref(), Some("2"));
        // A stale page id is an error, not a panic.
        assert_eq!(
            doc.write_row_cell(
                SheetRef::Page(PageId { index: 9 }),
                &SectionName::from_static("Geometry"),
                RowTarget::at(0),
                &CellName::new("X"),
                &write,
            ),
            Err(Error::UnknownPage(PageId { index: 9 }))
        );
    }

    #[test]
    fn replace_geometry_sections_drops_the_extra_geometry_only() {
        let geometry = |name: &str| {
            let mut section = Section::new(SectionName::new(name));
            let mut row = Row::new();
            row.row_type = Some("MoveTo".to_owned());
            section.rows.push(row);
            section
        };
        let mut other = Section::new(SectionName::from_static("Character"));
        other.cells.push(number_cell("Size", 1.0));
        let mut sections = vec![geometry("Geometry"), geometry("Geometry"), other];
        let mut fresh = Section::new(SectionName::from_static("Geometry"));
        fresh.rows.push({
            let mut row = Row::new();
            row.row_type = Some("LineTo".to_owned());
            row
        });
        replace_geometry_sections(&mut sections, vec![fresh]);
        let names: Vec<&str> = sections
            .iter()
            .map(|section| section.name.as_str())
            .collect();
        assert_eq!(names, vec!["Geometry", "Character"]);
        assert_eq!(sections[0].rows.len(), 1);
        assert_eq!(sections[0].rows[0].row_type.as_deref(), Some("LineTo"));
        assert_eq!(sections[1].cells.len(), 1);
    }

    #[test]
    fn walk_position_reports_a_stale_page_a_stale_shape_and_a_repeat() {
        let (mut doc, page) = document();
        let root = doc
            .insert_shape(page, box_shape(1.0, 1.0), None)
            .expect("root");
        let child = doc
            .insert_subshape(root, box_shape(1.0, 1.0), None)
            .expect("child");
        assert_eq!(doc.walk_position(page, root), 0);
        assert_eq!(doc.walk_position(page, child), 1);
        assert_eq!(doc.walk_position(PageId { index: 9 }, root), usize::MAX);
        // A child listed twice is walked once, and a `children` entry the
        // arena does not hold keeps its place in the draw order.
        doc.shape_mut(root).expect("root").children.push(child);
        doc.shape_mut(root)
            .expect("root")
            .children
            .push(ShapeId { index: 999 });
        assert_eq!(doc.walk_position(page, child), 1);
        assert_eq!(doc.walk_position(page, ShapeId { index: 999 }), 2);
        // A shape the tree does not reach ends the walk at `usize::MAX`.
        assert_eq!(doc.walk_position(page, ShapeId { index: 998 }), usize::MAX);
    }

    #[test]
    fn unlist_tolerates_a_stale_owner() {
        let (mut doc, page) = document();
        let root = doc
            .insert_shape(page, box_shape(1.0, 1.0), None)
            .expect("root");
        let orphan = doc.add_shape(box_shape(1.0, 1.0));
        doc.shape_mut(orphan).expect("orphan").parent = Some(ShapeId { index: 999 });
        doc.unlist(orphan);
        assert_eq!(doc.page(page).expect("page").shapes, vec![root]);
    }

    #[test]
    fn frame_of_rejects_a_stale_shape() {
        let (doc, _page) = document();
        assert!(matches!(
            doc.frame_of(ShapeId { index: 999 }),
            Err(Error::UnknownShape(id)) if id.index == 999
        ));
    }

    #[test]
    fn subtree_ids_skip_a_stale_child_and_stop_at_the_walk_cap() {
        let (mut doc, page) = document();
        let root = doc
            .insert_shape(page, box_shape(1.0, 1.0), None)
            .expect("root");
        let child = doc
            .insert_subshape(root, box_shape(1.0, 1.0), None)
            .expect("child");
        // The same child listed twice is walked once and a `children`
        // list naming a shape the arena does not hold is walked past.
        doc.shape_mut(root).expect("root").children.push(child);
        doc.shape_mut(root)
            .expect("root")
            .children
            .push(ShapeId { index: 999 });
        assert_eq!(
            doc.subtree_ids(root),
            vec![root, child, ShapeId { index: 999 }]
        );

        // A subtree wider than the walk cap is cut at the cap.
        let mut wide = Document::new();
        let root = ShapeId { index: 0 };
        let mut top = box_shape(1.0, 1.0);
        top.children = (1..=MAX_WALK as u32)
            .map(|index| ShapeId { index })
            .collect();
        wide.add_shape(top);
        for index in 1..=MAX_WALK as u32 {
            let mut shape = box_shape(1.0, 1.0);
            shape.id = ShapeId { index };
            wide.shapes.push(shape);
        }
        let ids = wide.subtree_ids(root);
        assert_eq!(ids.len(), MAX_WALK + 1);
        assert_eq!(ids.first(), Some(&root));
    }

    #[test]
    fn is_descendant_stops_at_the_walk_cap() {
        let mut doc = Document::new();
        for index in 0..=(MAX_WALK as u32 + 1) {
            let mut shape = box_shape(1.0, 1.0);
            shape.id = ShapeId { index };
            shape.parent = index.checked_sub(1).map(|parent| ShapeId { index: parent });
            doc.shapes.push(shape);
        }
        // The chain is deeper than the walk cap: the answer is "no".
        assert!(!doc.is_descendant(
            ShapeId {
                index: MAX_WALK as u32 + 1
            },
            ShapeId { index: 0 }
        ));
        // The direct parent link is still found.
        assert!(doc.is_descendant(ShapeId { index: 1 }, ShapeId { index: 0 }));
    }
    /// A `Property` row with a `Label` cell and (optionally) a `Value`
    /// cell carrying `value` in `unit`.
    fn property_row(name: &str, value: Option<(CellValue, Unit)>) -> Row {
        let mut row = Row::new();
        row.name = Some(name.to_owned());
        row.ix = Some(7);
        let mut label = Cell::new(CellName::from_static("Label"));
        label.value = Some(CellValue::Str(name.to_owned()));
        row.cells.push(label);
        if let Some((value, unit)) = value {
            let mut cell = Cell::new(CellName::from_static("Value"));
            cell.value = Some(value);
            cell.unit = unit;
            row.cells.push(cell);
        }
        row
    }

    /// A shape whose `Property` section holds `rows`.
    fn shape_with_properties(doc: &mut Document, rows: Vec<Row>) -> ShapeId {
        let id = doc.add_shape(Shape::new(ShapeId { index: 0 }, 1));
        if let Some(entity) = doc.shape_mut(id) {
            let mut section = Section::new(SectionName::from_static("Property"));
            section.rows = rows;
            entity.sheet.sections.push(section);
        }
        id
    }

    #[test]
    fn write_property_value_writes_the_own_row_and_keeps_its_unit() {
        let (mut doc, _page) = document();
        let shape = shape_with_properties(
            &mut doc,
            vec![property_row(
                "Cost",
                Some((CellValue::Number(2.0), Unit::Inches)),
            )],
        );
        let target = doc
            .write_property_value(shape, "Cost", &CellWrite::new("3", CellValue::Number(3.0)))
            .expect("write");
        assert_eq!(
            target,
            RowTarget {
                position: Some(0),
                ix: Some(7)
            }
        );
        let rows = doc.property_rows(shape);
        let value = rows[0]
            .row
            .cells
            .iter()
            .find(|cell| cell.name.as_str() == "Value")
            .expect("value cell");
        assert_eq!(value.value, Some(CellValue::Number(3.0)));
        assert_eq!(value.formula.as_deref(), Some("3"));
        assert_eq!(value.unit, Unit::Inches, "the recorded unit survives");
    }

    #[test]
    fn write_property_value_fills_a_row_without_a_value_cell() {
        let (mut doc, _page) = document();
        let shape = shape_with_properties(&mut doc, vec![property_row("Label", None)]);
        doc.write_property_value(shape, "Label", &CellWrite::new("2", CellValue::Number(2.0)))
            .expect("write");
        let rows = doc.property_rows(shape);
        let value = rows[0]
            .row
            .cells
            .iter()
            .find(|cell| cell.name.as_str() == "Value")
            .expect("value cell");
        assert_eq!(value.value, Some(CellValue::Number(2.0)));
        assert_eq!(value.unit, Unit::Unitsless, "a new cell takes the table");
    }

    #[test]
    fn write_property_value_materialises_an_inherited_row_locally() {
        let (mut doc, _page) = document();
        let rung = shape_with_properties(
            &mut doc,
            vec![property_row(
                "Cost",
                Some((CellValue::Number(9.0), Unit::Inches)),
            )],
        );
        let mut master = Master::new(MasterId { index: 0 }, 1);
        master.shapes = vec![rung];
        doc.masters.push(master);
        let instance = doc.add_shape(Shape::new(ShapeId { index: 0 }, 2));
        if let Some(entity) = doc.shape_mut(instance) {
            entity.master = Some(MasterId { index: 0 });
        }

        let target = doc
            .write_property_value(
                instance,
                "Cost",
                &CellWrite::new("4", CellValue::Number(4.0)),
            )
            .expect("write");
        assert_eq!(
            target,
            RowTarget {
                position: Some(0),
                ix: Some(7)
            }
        );
        let rows = doc.property_rows(instance);
        assert_eq!(rows.len(), 1);
        assert!(!rows[0].inherited, "the edit wrote a local row");
        let value = rows[0]
            .row
            .cells
            .iter()
            .find(|cell| cell.name.as_str() == "Value")
            .expect("value cell");
        assert_eq!(value.value, Some(CellValue::Number(4.0)));
        assert_eq!(value.unit, Unit::Inches, "the inherited unit survives");
        assert!(
            rows[0]
                .row
                .cells
                .iter()
                .any(|cell| cell.name.as_str() == "Label"),
            "the row's other cells came along"
        );
    }

    #[test]
    fn write_property_value_rejects_an_unknown_row_and_a_non_finite_number() {
        let (mut doc, _page) = document();
        let shape = shape_with_properties(
            &mut doc,
            vec![property_row(
                "Cost",
                Some((CellValue::Str("x".to_owned()), Unit::Unitsless)),
            )],
        );
        assert_eq!(
            doc.write_property_value(shape, "Nope", &CellWrite::new("1", CellValue::Number(1.0))),
            Err(Error::BadCell("no such Shape Data row"))
        );
        assert_eq!(
            doc.write_property_value(
                shape,
                "Cost",
                &CellWrite::new("1e999", CellValue::Number(f64::INFINITY))
            ),
            Err(Error::BadCell("non-finite number"))
        );
        assert_eq!(
            doc.write_property_value(
                ShapeId { index: 999 },
                "Cost",
                &CellWrite::new("1", CellValue::Number(1.0))
            ),
            Err(Error::UnknownShape(ShapeId { index: 999 }))
        );
    }
}
