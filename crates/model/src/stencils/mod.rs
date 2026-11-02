//! Hand-authored stencil masters (ADR 0034).
//!
//! The stencils ship as Rust data, not as files: [`basic_shapes`] and
//! [`basic_flowchart`] return [`StencilDef`]s of [`MasterDef`]s whose
//! geometry we authored ourselves (AGENTS.md section 4: every stencil
//! master is original work). Each master carries its geometry rows in
//! local inches on the unit box scaled to its `Width`/`Height` cells,
//! our own line and fill defaults (ADR 0023), 2-4 Shape Data rows and
//! connection points in local coordinates.
//!
//! [`stencil_document`] builds the catalog as one stencil-kind
//! [`Document`] (ADR 0016) so `larrygram-vsdx` can write it as `.vssx`;
//! [`instantiate`] drops one master onto a page as a new instance for
//! Phase 5. Nothing here traces, copies or derives from Microsoft
//! material: the shapes are simple polygons, arcs and ellipses whose
//! coordinates this module defines.

mod basic_flowchart;
mod basic_shapes;

use larrygram_geom::Unit;

use crate::cell::{Cell, CellValue};
use crate::document::{Document, DocumentKind};
use crate::geometry::GeometryRow;
use crate::id::{MasterId, PageId, ShapeId};
use crate::master::Master;
use crate::name::{CellName, SectionName};
use crate::page::Page;
use crate::section::{Row, Section};
use crate::shape::Shape;
use crate::sheet::Sheet;

/// A hand-authored stencil: named masters of our own design (ADR 0034).
#[derive(Debug, Clone, PartialEq)]
pub struct StencilDef {
    /// Display name (e.g. "Basic Shapes").
    pub name: &'static str,
    /// Masters in Shapes window order.
    pub masters: Vec<MasterDef>,
}

/// One stencil master of our own design (ADR 0034).
#[derive(Debug, Clone, PartialEq)]
pub struct MasterDef {
    /// Unique name (`NameU`), e.g. "Rectangle".
    pub name_u: &'static str,
    /// Display name (`Name`).
    pub name: &'static str,
    /// Master size in inches (the master's `Width` cell).
    pub width: f64,
    /// Master size in inches (the master's `Height` cell).
    pub height: f64,
    /// Geometry rows in local coordinates: the unit box scaled by
    /// `width`/`height`, so every coordinate lies in `0..=width`,
    /// `0..=height`, in inches.
    pub geometry: Vec<GeometryRow>,
    /// Default line (our own visual style defaults, ADR 0023).
    pub line: LineDefaults,
    /// Default fill (our own visual style defaults, ADR 0023).
    pub fill: FillDefaults,
    /// Shape Data rows shown in the Shape Data pane (ADR 0035), as
    /// `(label, value)` pairs.
    pub shape_data: Vec<(&'static str, &'static str)>,
    /// Connection points in local coordinates (Phase 5 glues to these).
    ///
    /// A master with no points gets **no** `Connection` section at all:
    /// a connector is not a glue target in the alpha (ADR 0084 rule 1).
    pub connection_points: Vec<(f64, f64)>,
    /// True for a 1-D connector master (ADR 0084 rule 1): the master's
    /// shape carries the endpoint cells and the pin, size and lock
    /// formulas of `basic_flowchart::connector_cells`, not the 2-D
    /// transform cells of `transform_cells`.
    pub connector_1d: bool,
}

/// The line defaults of a master (ADR 0023): our own values, not
/// Visio's.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LineDefaults {
    /// Stroke width in inches.
    pub weight: f64,
    /// Line colour as a fixed palette index (0..=23,
    /// [`crate::values::palette`]).
    pub color: u32,
    /// `LinePattern` index (1 = solid).
    pub pattern: u32,
}

impl Default for LineDefaults {
    /// Our own default line: 0.01 inch solid black (ADR 0023).
    fn default() -> Self {
        Self {
            weight: 0.01,
            color: 0,
            pattern: 1,
        }
    }
}

/// The fill defaults of a master (ADR 0023): our own values, not
/// Visio's.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FillDefaults {
    /// `FillPattern` index (0 = none, 1 = solid).
    pub pattern: u32,
    /// Foreground colour as a fixed palette index (0..=23).
    pub fore: u32,
    /// Background colour as a fixed palette index (0..=23).
    pub back: u32,
}

impl Default for FillDefaults {
    /// Our own default fill: solid white on white (ADR 0023).
    fn default() -> Self {
        Self {
            pattern: 1,
            fore: 1,
            back: 1,
        }
    }
}

/// The "Basic Shapes" stencil (original work).
pub fn basic_shapes() -> StencilDef {
    StencilDef {
        name: "Basic Shapes",
        masters: basic_shapes::masters(),
    }
}

/// The "Basic Flowchart" stencil (original work).
pub fn basic_flowchart() -> StencilDef {
    StencilDef {
        name: "Basic Flowchart",
        masters: basic_flowchart::masters(),
    }
}

/// Every stencil the Shapes window lists (ADR 0034).
pub fn catalog() -> Vec<StencilDef> {
    vec![basic_shapes(), basic_flowchart()]
}

/// Builds the whole catalog as one stencil-kind [`Document`] (ADR 0016),
/// so `larrygram-vsdx` can save it as `.vssx`.
///
/// Each [`MasterDef`] becomes one [`Master`] whose single shape carries
/// the geometry rows, the line and fill cells, the Shape Data rows and
/// the connection points.
pub fn stencil_document() -> Document {
    let mut document = Document::new();
    document.kind = DocumentKind::Stencil;
    for stencil in catalog() {
        for master in &stencil.masters {
            ensure_master(&mut document, master);
        }
    }
    document
}

/// Builds one master's shape onto a page as a new instance (ADR 0034).
///
/// The master is added to `target` once (matched by `NameU`) and the
/// instance references it, so the geometry rows and the line and fill
/// cells resolve through the master rung (ADR 0013). The instance's own
/// cells set the shape transform of ADR 0020: `Width`/`Height` are the
/// master size (the unit box scaled by them is the master's local
/// coordinates) and the pin cells place the box's lower-left corner at
/// the page origin (`PinX = Width*0.5`, `PinY = Height*0.5`, the
/// documented `LocPinX`/`LocPinY` defaults), so the local box maps to
/// `(0, 0)..(width, height)` on the page.
///
/// A target without pages gets one ("Page-1"): an instance always lands
/// on a page.
pub fn instantiate(target: &mut Document, master: &MasterDef) -> ShapeId {
    let owner = ensure_master(target, master);
    let page = first_page(target);
    let visio_id = target
        .pages
        .get(page.index as usize)
        .map(|entity| i32::try_from(entity.shapes.len() + 1).unwrap_or(i32::MAX))
        .unwrap_or(1);
    let mut shape = Shape::new(ShapeId { index: 0 }, visio_id);
    shape.name = master.name.to_owned();
    shape.name_u = master.name_u.to_owned();
    shape.master = Some(owner);
    if master.connector_1d {
        // A 1-D master (ADR 0084 rule 1, amended 2026-11-03): only the
        // endpoint cells are the instance's own; the path write fills
        // the pin and size cells through the master's GUARD formulas.
        shape.sheet.cells = vec![
            number_cell("BeginX", 0.0, Unit::Inches),
            number_cell("BeginY", 0.0, Unit::Inches),
            number_cell("EndX", master.width, Unit::Inches),
            number_cell("EndY", 0.0, Unit::Inches),
        ];
    } else {
        shape.sheet.cells = transform_cells(
            master.width,
            master.height,
            master.width * 0.5,
            master.height * 0.5,
        );
    }
    let id = target.add_shape(shape);
    if let Some(entity) = target.pages.get_mut(page.index as usize) {
        entity.shapes.push(id);
    }
    if master.connector_1d {
        // The ends are finite and the path has two points: the write
        // cannot fail, and a failure would leave the master's own cells.
        let _ = target.write_connector_path(
            id,
            &[
                larrygram_geom::Point::new(0.0, 0.0),
                larrygram_geom::Point::new(master.width, 0.0),
            ],
        );
    }
    id
}

/// The `MasterDef` in `target` as a [`Master`], adding it (with its one
/// shape) when no master of that `NameU` is present yet.
fn ensure_master(target: &mut Document, def: &MasterDef) -> MasterId {
    if let Some(id) = target.master_by_name_u(def.name_u) {
        return id;
    }
    let id = MasterId {
        index: u32::try_from(target.masters.len()).unwrap_or(u32::MAX),
    };
    let visio_id = i32::try_from(target.masters.len() + 1).unwrap_or(i32::MAX);
    let mut master = Master::new(id, visio_id);
    master.name = def.name.to_owned();
    master.name_u = def.name_u.to_owned();
    let mut shape = Shape::new(ShapeId { index: 0 }, 1);
    shape.name = def.name.to_owned();
    shape.name_u = def.name_u.to_owned();
    shape.sheet = master_sheet(def);
    let content = target.add_shape(shape);
    master.shapes.push(content);
    target.masters.push(master);
    id
}

/// The first page of `target`, adding "Page-1" when it has none.
fn first_page(target: &mut Document) -> PageId {
    if let Some(entity) = target.pages.first() {
        return entity.id;
    }
    let id = PageId { index: 0 };
    let mut page = Page::new(id, 1);
    page.name = "Page-1".to_owned();
    page.name_u = "Page-1".to_owned();
    target.pages.push(page);
    id
}

/// The shape sheet of one master's shape: the transform cells, the line
/// and fill cells and the geometry, Shape Data and connection sections.
fn master_sheet(def: &MasterDef) -> Sheet {
    let mut sheet = Sheet::new();
    if def.connector_1d {
        sheet.cells = basic_flowchart::connector_cells(def.width, def.height);
    } else {
        sheet.cells = transform_cells(def.width, def.height, def.width * 0.5, def.height * 0.5);
    }
    sheet
        .cells
        .push(number_cell("LineWeight", def.line.weight, Unit::Inches));
    sheet.cells.push(number_cell(
        "LinePattern",
        f64::from(def.line.pattern),
        Unit::Unitsless,
    ));
    sheet.cells.push(number_cell(
        "LineColor",
        f64::from(def.line.color),
        Unit::Unitsless,
    ));
    sheet.cells.push(number_cell(
        "FillPattern",
        f64::from(def.fill.pattern),
        Unit::Unitsless,
    ));
    sheet.cells.push(number_cell(
        "FillForegnd",
        f64::from(def.fill.fore),
        Unit::Unitsless,
    ));
    sheet.cells.push(number_cell(
        "FillBkgnd",
        f64::from(def.fill.back),
        Unit::Unitsless,
    ));
    sheet.sections.push(geometry_section(&def.geometry));
    if !def.shape_data.is_empty() {
        sheet.sections.push(property_section(&def.shape_data));
    }
    if !def.connection_points.is_empty() {
        sheet
            .sections
            .push(connection_section(&def.connection_points));
    }
    sheet
}

/// The shape transform cells of ADR 0020 for a box of `width` x
/// `height` inches pinned at `(pin_x, pin_y)`.
fn transform_cells(width: f64, height: f64, pin_x: f64, pin_y: f64) -> Vec<Cell> {
    vec![
        number_cell("Width", width, Unit::Inches),
        number_cell("Height", height, Unit::Inches),
        number_cell("PinX", pin_x, Unit::Inches),
        number_cell("PinY", pin_y, Unit::Inches),
        number_cell("LocPinX", width * 0.5, Unit::Inches),
        number_cell("LocPinY", height * 0.5, Unit::Inches),
        number_cell("Angle", 0.0, Unit::Radians),
        bool_cell("FlipX", false),
        bool_cell("FlipY", false),
    ]
}

/// The `Geometry` section of one master's typed rows.
fn geometry_section(rows: &[GeometryRow]) -> Section {
    let mut section = Section::new(SectionName::from_static("Geometry"));
    for (position, row) in rows.iter().enumerate() {
        let mut mapped = geometry_row(row);
        mapped.ix = Some(i32::try_from(position).unwrap_or(i32::MAX));
        section.rows.push(mapped);
    }
    section
}

/// One typed geometry row as a `Row` element's data (the inverse of the
/// reader's `type_row`, ADR 0019): the row type of the variant and the
/// cached cells it reads.
///
/// The formula-encoded data of a `NurbsTo` or `PolylineTo` row (the `E`
/// and `A` cells) is not written: our stencils never author those rows
/// and the cells the reader falls back to (`X`, `Y`, `A`..`D`) are kept.
fn geometry_row(row: &GeometryRow) -> Row {
    let mut mapped = Row::new();
    match row {
        GeometryRow::MoveTo { x, y } => {
            mapped.row_type = Some("MoveTo".to_owned());
            push_xy(&mut mapped, *x, *y);
        }
        GeometryRow::LineTo { x, y } => {
            mapped.row_type = Some("LineTo".to_owned());
            push_xy(&mut mapped, *x, *y);
        }
        GeometryRow::ArcTo { x, y, bow } => {
            mapped.row_type = Some("ArcTo".to_owned());
            push_xy(&mut mapped, *x, *y);
            push_number(&mut mapped, "A", *bow, Unit::Inches);
        }
        GeometryRow::EllipticalArcTo {
            x,
            y,
            ctrl,
            angle,
            ratio,
        } => {
            mapped.row_type = Some("EllipticalArcTo".to_owned());
            push_xy(&mut mapped, *x, *y);
            push_point(&mut mapped, "A", "B", *ctrl);
            push_number(&mut mapped, "C", angle.radians(), Unit::Radians);
            push_number(&mut mapped, "D", *ratio, Unit::Unitsless);
        }
        GeometryRow::NurbsTo {
            x,
            y,
            second_last_knot,
            last_weight,
            first_knot,
            first_weight,
            ..
        } => {
            mapped.row_type = Some("NurbsTo".to_owned());
            push_xy(&mut mapped, *x, *y);
            push_number(&mut mapped, "A", *second_last_knot, Unit::Unitsless);
            push_number(&mut mapped, "B", *last_weight, Unit::Unitsless);
            push_number(&mut mapped, "C", *first_knot, Unit::Unitsless);
            push_number(&mut mapped, "D", *first_weight, Unit::Unitsless);
        }
        GeometryRow::PolylineTo { x, y, .. } => {
            mapped.row_type = Some("PolylineTo".to_owned());
            push_xy(&mut mapped, *x, *y);
        }
        GeometryRow::SplineStart {
            x,
            y,
            second_knot,
            first_knot,
            last_knot,
            degree,
        } => {
            mapped.row_type = Some("SplineStart".to_owned());
            push_xy(&mut mapped, *x, *y);
            push_number(&mut mapped, "A", *second_knot, Unit::Unitsless);
            push_number(&mut mapped, "B", *first_knot, Unit::Unitsless);
            push_number(&mut mapped, "C", *last_knot, Unit::Unitsless);
            push_number(&mut mapped, "D", f64::from(*degree), Unit::Unitsless);
        }
        GeometryRow::SplineKnot { x, y, knot } => {
            mapped.row_type = Some("SplineKnot".to_owned());
            push_xy(&mut mapped, *x, *y);
            push_number(&mut mapped, "A", *knot, Unit::Unitsless);
        }
        GeometryRow::Ellipse { center, a, b } => {
            mapped.row_type = Some("Ellipse".to_owned());
            push_point(&mut mapped, "X", "Y", *center);
            push_point(&mut mapped, "A", "B", *a);
            push_point(&mut mapped, "C", "D", *b);
        }
        GeometryRow::InfiniteLine { p1, p2 } => {
            mapped.row_type = Some("InfiniteLine".to_owned());
            push_point(&mut mapped, "X", "Y", *p1);
            push_point(&mut mapped, "A", "B", *p2);
        }
        GeometryRow::RelMoveTo { x, y } => {
            mapped.row_type = Some("RelMoveTo".to_owned());
            push_xy(&mut mapped, *x, *y);
        }
        GeometryRow::RelLineTo { x, y } => {
            mapped.row_type = Some("RelLineTo".to_owned());
            push_xy(&mut mapped, *x, *y);
        }
        GeometryRow::RelQuadBezTo { x, y, ctrl } => {
            mapped.row_type = Some("RelQuadBezTo".to_owned());
            push_xy(&mut mapped, *x, *y);
            push_point(&mut mapped, "A", "B", *ctrl);
        }
        GeometryRow::RelCubBezTo { x, y, ctrl1, ctrl2 } => {
            mapped.row_type = Some("RelCubBezTo".to_owned());
            push_xy(&mut mapped, *x, *y);
            push_point(&mut mapped, "A", "B", *ctrl1);
            push_point(&mut mapped, "C", "D", *ctrl2);
        }
        GeometryRow::RelEllipticalArcTo {
            x,
            y,
            ctrl,
            angle,
            ratio,
        } => {
            mapped.row_type = Some("RelEllipticalArcTo".to_owned());
            push_xy(&mut mapped, *x, *y);
            push_point(&mut mapped, "A", "B", *ctrl);
            push_number(&mut mapped, "C", angle.radians(), Unit::Radians);
            push_number(&mut mapped, "D", *ratio, Unit::Unitsless);
        }
        GeometryRow::Unknown { row_type } => {
            mapped.row_type = Some(row_type.clone());
        }
    }
    mapped
}

/// The `Shape Data` section of one master: one row per `(label, value)`.
fn property_section(data: &[(&'static str, &'static str)]) -> Section {
    let mut section = Section::new(SectionName::from_static("Property"));
    for (position, (label, value)) in data.iter().enumerate() {
        let mut row = Row::new();
        row.name = Some((*label).to_owned());
        row.ix = Some(i32::try_from(position).unwrap_or(i32::MAX));
        row.cells = vec![text_cell("Label", label), text_cell("Value", value)];
        section.rows.push(row);
    }
    section
}

/// The `Connection` section of one master: one row per point.
fn connection_section(points: &[(f64, f64)]) -> Section {
    let mut section = Section::new(SectionName::from_static("Connection"));
    for (position, (x, y)) in points.iter().enumerate() {
        let mut row = Row::new();
        row.row_type = Some("Connection".to_owned());
        row.ix = Some(i32::try_from(position).unwrap_or(i32::MAX));
        row.cells = vec![
            number_cell("X", *x, Unit::Inches),
            number_cell("Y", *y, Unit::Inches),
        ];
        section.rows.push(row);
    }
    section
}

/// The `X`/`Y` cells of one geometry row.
fn push_xy(row: &mut Row, x: f64, y: f64) {
    push_point(row, "X", "Y", larrygram_geom::Point::new(x, y));
}

/// The two numbered cells of one point.
fn push_point(
    row: &mut Row,
    x_name: &'static str,
    y_name: &'static str,
    point: larrygram_geom::Point,
) {
    push_number(row, x_name, point.x, Unit::Inches);
    push_number(row, y_name, point.y, Unit::Inches);
}

/// Appends one numeric cell to a row.
fn push_number(row: &mut Row, name: &'static str, value: f64, unit: Unit) {
    row.cells.push(number_cell(name, value, unit));
}

/// A cell with a cached number in `unit`.
pub(super) fn number_cell(name: &'static str, value: f64, unit: Unit) -> Cell {
    let mut cell = Cell::new(CellName::from_static(name));
    cell.value = Some(CellValue::Number(value));
    cell.unit = unit;
    cell
}

/// A cell with a formula and its cached value (ADR 0011): the master's
/// documented formula and the number it computes.
pub(super) fn formula_cell(
    name: &'static str,
    formula: &'static str,
    value: f64,
    unit: Unit,
) -> Cell {
    let mut cell = number_cell(name, value, unit);
    cell.formula = Some(formula.to_owned());
    cell
}

/// A cell with a cached boolean.
fn bool_cell(name: &'static str, value: bool) -> Cell {
    let mut cell = Cell::new(CellName::from_static(name));
    cell.value = Some(CellValue::Bool(value));
    cell
}

/// A cell with a cached string.
fn text_cell(name: &'static str, value: &str) -> Cell {
    let mut cell = Cell::new(CellName::from_static(name));
    cell.value = Some(CellValue::Str(value.to_owned()));
    cell
}

/// The vertices of a regular polygon inscribed in the unit box scaled by
/// `width`/`height`, counter-clockwise from `start` (radians).
pub(crate) fn regular_polygon(
    sides: usize,
    width: f64,
    height: f64,
    start: f64,
) -> Vec<(f64, f64)> {
    (0..sides)
        .map(|step| {
            let angle = start + std::f64::consts::TAU * step as f64 / sides as f64;
            (
                width * 0.5 * (1.0 + angle.cos()),
                height * 0.5 * (1.0 + angle.sin()),
            )
        })
        .collect()
}

/// The vertices of a `points`-pointed star inscribed in the unit box
/// scaled by `width`/`height`: outer vertices counter-clockwise from
/// `start` (radians), inner vertices at `inner` of the outer radius.
pub(crate) fn star_points(
    points: usize,
    width: f64,
    height: f64,
    inner: f64,
    start: f64,
) -> Vec<(f64, f64)> {
    (0..points * 2)
        .map(|step| {
            let angle = start + std::f64::consts::TAU * step as f64 / (points * 2) as f64;
            let radius = if step % 2 == 0 { 1.0 } else { inner };
            (
                width * 0.5 * (1.0 + radius * angle.cos()),
                height * 0.5 * (1.0 + radius * angle.sin()),
            )
        })
        .collect()
}

/// The geometry rows of a closed polygon: `MoveTo` the first vertex,
/// `LineTo` each next one and `LineTo` back to the start.
pub(crate) fn closed_polygon(points: &[(f64, f64)]) -> Vec<GeometryRow> {
    let mut rows = Vec::with_capacity(points.len() + 1);
    let mut vertices = points.iter();
    if let Some((x, y)) = vertices.next() {
        rows.push(GeometryRow::MoveTo { x: *x, y: *y });
    }
    for (x, y) in vertices {
        rows.push(GeometryRow::LineTo { x: *x, y: *y });
    }
    if let Some((x, y)) = points.first() {
        rows.push(GeometryRow::LineTo { x: *x, y: *y });
    }
    rows
}

/// The geometry rows of a rectangle with rounded corners counter-clockwise
/// from `(radius, 0)`: each corner is an [`GeometryRow::ArcTo`] of bow
/// `radius * (1 - sqrt(2)/2)` (the sagitta of a quarter circle).
pub(crate) fn rounded_rect(width: f64, height: f64, radius: f64) -> Vec<GeometryRow> {
    let bow = -radius * (1.0 - std::f64::consts::FRAC_1_SQRT_2);
    let r = radius.clamp(0.0, width.min(height) * 0.5);
    let (w, h) = (width, height);
    vec![
        GeometryRow::MoveTo { x: r, y: 0.0 },
        GeometryRow::LineTo { x: w - r, y: 0.0 },
        GeometryRow::ArcTo { x: w, y: r, bow },
        GeometryRow::LineTo { x: w, y: h - r },
        GeometryRow::ArcTo {
            x: w - r,
            y: h,
            bow,
        },
        GeometryRow::LineTo { x: r, y: h },
        GeometryRow::ArcTo {
            x: 0.0,
            y: h - r,
            bow,
        },
        GeometryRow::LineTo { x: 0.0, y: r },
        GeometryRow::ArcTo { x: r, y: 0.0, bow },
    ]
}

/// The four edge midpoints of the unit box scaled by `width`/`height`:
/// bottom, right, top, left.
pub(crate) fn edge_midpoints(width: f64, height: f64) -> Vec<(f64, f64)> {
    vec![
        (width * 0.5, 0.0),
        (width, height * 0.5),
        (width * 0.5, height),
        (0.0, height * 0.5),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The `MasterDef` with this `NameU`, or panics when the catalog has
    /// none.
    #[allow(clippy::expect_used, clippy::panic)] // test helper
    fn master_of(name_u: &str) -> MasterDef {
        catalog()
            .into_iter()
            .flat_map(|stencil| stencil.masters)
            .find(|master| master.name_u == name_u)
            .unwrap_or_else(|| panic!("no master {name_u}"))
    }

    #[test]
    fn the_catalog_holds_both_stencils() {
        let catalog = catalog();
        let names: Vec<&str> = catalog.iter().map(|stencil| stencil.name).collect();
        assert_eq!(names, ["Basic Shapes", "Basic Flowchart"]);
        assert_eq!(catalog[0].masters.len(), 12);
        assert_eq!(catalog[1].masters.len(), 9);
    }

    #[test]
    fn every_master_has_a_finite_size_and_geometry() {
        for stencil in catalog() {
            for master in &stencil.masters {
                assert!(
                    master.width.is_finite() && master.width > 0.0,
                    "{}: width",
                    master.name_u
                );
                assert!(
                    master.height.is_finite() && master.height > 0.0,
                    "{}: height",
                    master.name_u
                );
                assert!(!master.geometry.is_empty(), "{}: geometry", master.name_u);
                let rows = geometry_section(&master.geometry).rows;
                assert_eq!(rows.len(), master.geometry.len());
                for row in &rows {
                    for cell in &row.cells {
                        if let Some(CellValue::Number(value)) = &cell.value {
                            assert!(value.is_finite(), "{}: finite cells", master.name_u);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn every_master_sits_in_its_box() {
        for stencil in catalog() {
            for master in &stencil.masters {
                for (x, y) in &master.connection_points {
                    assert!(
                        (0.0..=master.width).contains(x) && (0.0..=master.height).contains(y),
                        "{}: connection point in the box",
                        master.name_u
                    );
                }
                if master.connector_1d {
                    assert!(
                        master.shape_data.is_empty(),
                        "{}: a connector carries no Shape Data rows",
                        master.name_u
                    );
                } else {
                    assert!(
                        (2..=4).contains(&master.shape_data.len()),
                        "{}: 2-4 Shape Data rows",
                        master.name_u
                    );
                }
            }
        }
    }

    #[test]
    fn the_default_line_and_fill_are_our_own() {
        let line = LineDefaults::default();
        assert_eq!(line.weight, 0.01);
        assert_eq!((line.color, line.pattern), (0, 1));
        let fill = FillDefaults::default();
        assert_eq!((fill.pattern, fill.fore, fill.back), (1, 1, 1));
    }

    #[test]
    fn the_stencil_document_holds_every_master() {
        let document = stencil_document();
        assert_eq!(document.kind, DocumentKind::Stencil);
        let expected: Vec<MasterDef> = catalog()
            .into_iter()
            .flat_map(|stencil| stencil.masters)
            .collect();
        assert_eq!(document.masters.len(), expected.len());
        for (def, master) in expected.iter().zip(&document.masters) {
            assert_eq!(master.name, def.name);
            assert_eq!(master.name_u, def.name_u);
            assert_eq!(master.shapes.len(), 1);
            let shape = master.shapes[0];
            let geometry = document
                .shape_geometry(shape)
                .expect("master shape geometry");
            assert_eq!(geometry.paths.len(), 1);
            assert_eq!(geometry.paths[0].rows.len(), def.geometry.len());
        }
    }

    #[test]
    fn instantiate_puts_a_scaled_instance_on_a_page() {
        let mut document = Document::new();
        let def = master_of("Rectangle");
        let shape = instantiate(&mut document, &def);
        assert_eq!(document.pages.len(), 1);
        assert_eq!(document.pages[0].shapes, [shape]);

        // The transform maths of ADR 0020: translate(PinX, PinY) then
        // translate(-LocPinX, -LocPinY) maps the local box to the page.
        let mut reader = crate::values::CellReader::new(&document);
        let mut cell = |name: &str| {
            reader
                .number(crate::values::SheetRef::Shape(shape), &CellName::new(name))
                .expect("cell")
        };
        let (width, height) = (cell("Width"), cell("Height"));
        let (pin_x, pin_y) = (cell("PinX"), cell("PinY"));
        let (loc_x, loc_y) = (cell("LocPinX"), cell("LocPinY"));
        assert_eq!((width, height), (def.width, def.height));
        assert_eq!((pin_x, pin_y), (def.width * 0.5, def.height * 0.5));
        assert_eq!((loc_x, loc_y), (def.width * 0.5, def.height * 0.5));
        let to_page = |x: f64, y: f64| (pin_x - loc_x + x, pin_y - loc_y + y);
        assert_eq!(to_page(0.0, 0.0), (0.0, 0.0));
        assert_eq!(to_page(width, height), (def.width, def.height));
    }

    #[test]
    fn an_instance_inherits_the_master_geometry() {
        let mut document = Document::new();
        let def = master_of("Ellipse");
        let shape = instantiate(&mut document, &def);
        let geometry = document.shape_geometry(shape).expect("geometry");
        assert_eq!(geometry.paths.len(), 1);
        assert_eq!(geometry.paths[0].rows, def.geometry);
    }

    #[test]
    fn instantiate_reuses_the_master_and_the_page_it_added() {
        let mut document = Document::new();
        let def = master_of("Process");
        let first = instantiate(&mut document, &def);
        let second = instantiate(&mut document, &def);
        assert_ne!(first, second);
        assert_eq!(document.masters.len(), 1);
        assert_eq!(document.pages.len(), 1);
        assert_eq!(document.pages[0].shapes, [first, second]);
        for shape in [first, second] {
            let instance = document.shape(shape).expect("shape");
            assert_eq!(instance.master, Some(MasterId { index: 0 }));
        }
    }

    #[test]
    fn a_geometry_row_becomes_a_typed_row() {
        use larrygram_geom::{Angle, Point};
        let rows = [
            GeometryRow::MoveTo { x: 1.0, y: 2.0 },
            GeometryRow::LineTo { x: 3.0, y: 4.0 },
            GeometryRow::ArcTo {
                x: 5.0,
                y: 6.0,
                bow: 0.25,
            },
            GeometryRow::EllipticalArcTo {
                x: 7.0,
                y: 8.0,
                ctrl: Point::new(0.5, 0.6),
                angle: Angle::from_radians(1.0),
                ratio: 2.0,
            },
            GeometryRow::NurbsTo {
                x: 9.0,
                y: 10.0,
                second_last_knot: 1.0,
                last_weight: 2.0,
                first_knot: 3.0,
                first_weight: 4.0,
                data: None,
            },
            GeometryRow::PolylineTo {
                x: 11.0,
                y: 12.0,
                data: None,
            },
            GeometryRow::SplineStart {
                x: 13.0,
                y: 14.0,
                second_knot: 1.0,
                first_knot: 2.0,
                last_knot: 3.0,
                degree: 3,
            },
            GeometryRow::SplineKnot {
                x: 15.0,
                y: 16.0,
                knot: 4.0,
            },
            GeometryRow::Ellipse {
                center: Point::new(17.0, 18.0),
                a: Point::new(1.0, 0.0),
                b: Point::new(0.0, 1.0),
            },
            GeometryRow::InfiniteLine {
                p1: Point::new(19.0, 20.0),
                p2: Point::new(21.0, 22.0),
            },
            GeometryRow::RelMoveTo { x: 0.1, y: 0.2 },
            GeometryRow::RelLineTo { x: 0.3, y: 0.4 },
            GeometryRow::RelQuadBezTo {
                x: 0.5,
                y: 0.6,
                ctrl: Point::new(0.7, 0.8),
            },
            GeometryRow::RelCubBezTo {
                x: 0.9,
                y: 1.0,
                ctrl1: Point::new(1.1, 1.2),
                ctrl2: Point::new(1.3, 1.4),
            },
            GeometryRow::RelEllipticalArcTo {
                x: 1.5,
                y: 1.6,
                ctrl: Point::new(1.7, 1.8),
                angle: Angle::from_radians(0.5),
                ratio: 1.0,
            },
            GeometryRow::Unknown {
                row_type: "Someday".to_owned(),
            },
        ];
        for row in &rows {
            let mapped = geometry_row(row);
            let cells: Vec<(&str, f64)> = mapped
                .cells
                .iter()
                .map(|cell| {
                    (
                        cell.name.as_str(),
                        match cell.value {
                            Some(CellValue::Number(value)) => value,
                            _ => f64::NAN,
                        },
                    )
                })
                .collect();
            assert!(mapped.row_type.is_some());
            match row {
                GeometryRow::MoveTo { x, y }
                | GeometryRow::LineTo { x, y }
                | GeometryRow::RelMoveTo { x, y }
                | GeometryRow::RelLineTo { x, y } => {
                    assert_eq!(cells, [("X", *x), ("Y", *y)]);
                }
                GeometryRow::ArcTo { x, y, bow } => {
                    assert_eq!(cells, [("X", *x), ("Y", *y), ("A", *bow)]);
                }
                GeometryRow::SplineKnot { x, y, knot } => {
                    assert_eq!(cells, [("X", *x), ("Y", *y), ("A", *knot)]);
                }
                GeometryRow::PolylineTo { x, y, .. } => {
                    assert_eq!(cells, [("X", *x), ("Y", *y)]);
                }
                GeometryRow::InfiniteLine { p1, p2 } => {
                    assert_eq!(cells, [("X", p1.x), ("Y", p1.y), ("A", p2.x), ("B", p2.y)]);
                }
                GeometryRow::RelQuadBezTo { x, y, ctrl } => {
                    assert_eq!(cells, [("X", *x), ("Y", *y), ("A", ctrl.x), ("B", ctrl.y)]);
                }
                GeometryRow::Unknown { row_type } => {
                    assert_eq!(mapped.row_type.as_deref(), Some(row_type.as_str()));
                    assert!(cells.is_empty());
                }
                _ => {
                    assert_eq!(cells.len(), 6, "six cells for {row:?}");
                }
            }
        }
    }

    #[test]
    fn the_arc_bow_of_a_rounded_corner_points_at_the_corner() {
        let rows = rounded_rect(2.0, 1.0, 0.2);
        assert_eq!(rows.len(), 9);
        // The first corner arc runs from (1.8, 0) to (2, 0.2). Its
        // points must sit nearer the corner (2, 0) than the opposite
        // chord point (1.8, 0.2): a positive bow would bulge into the
        // rectangle.
        let bow = match rows[2] {
            GeometryRow::ArcTo { bow, .. } => bow,
            _ => panic!("row 2 is an ArcTo"),
        };
        let cubics = larrygram_geom::Curve::Arc(larrygram_geom::Arc {
            start: larrygram_geom::Point::new(1.8, 0.0),
            end: larrygram_geom::Point::new(2.0, 0.2),
            bow,
        })
        .to_cubics(0.001)
        .expect("cubics");
        for cubic in &cubics {
            let mid = cubic.eval(0.5);
            let to_corner = (mid.x - 2.0).hypot(mid.y);
            let to_inner = (mid.x - 1.8).hypot(mid.y - 0.2);
            assert!(
                to_corner < to_inner,
                "arc point {mid:?} bulges to the wrong side"
            );
        }
    }

    #[test]
    fn the_polygon_helpers_fill_the_box() {
        let hexagon = regular_polygon(6, 2.0, 1.0, 0.0);
        assert_eq!(hexagon.len(), 6);
        assert_eq!(hexagon[0], (2.0, 0.5));
        let star = star_points(5, 2.0, 2.0, 0.5, std::f64::consts::FRAC_PI_2);
        assert_eq!(star.len(), 10);
        assert!((star[0].1 - 2.0).abs() < 1e-9, "outer point at the top");
        let closed = closed_polygon(&[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0)]);
        assert_eq!(closed.len(), 4);
        assert!(matches!(closed[0], GeometryRow::MoveTo { .. }));
        assert!(matches!(closed[3], GeometryRow::LineTo { x: 0.0, y: 0.0 }));
        assert!(closed_polygon(&[]).is_empty());
    }
}
