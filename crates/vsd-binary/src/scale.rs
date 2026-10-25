// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The drawing-scale pass: stored units to physical inches (P8-15
//! goal 4).
//!
//! A scaled drawing (a plan at a quarter inch to the foot) stores its
//! page size, its shape transforms and every length in the units the
//! drawing is scaled in; the physical inches are
//! `value * PageScale / DrawingScale` (the reference multiplies every
//! coordinate and length it writes by that factor). The page-size pair
//! is scaled where it is mapped ([`crate::pages`]); this pass scales
//! the rest: the transform and text-block cells, the line and fill
//! lengths and the geometry rows, for every shape of every page and of
//! every master.
//!
//! Angles, patterns, colours, point sizes and the formula-encoded
//! `PolylineTo`/`NurbsTo` rows do not scale. A file whose drawings are
//! not scaled (`PageScale == DrawingScale`) passes through unchanged.

use larrygram_model::{Cell, CellValue, Document, Row, Shape, ShapeId};

/// The direct cells that hold a length and scale with the drawing.
const LENGTH_CELLS: &[&str] = &[
    "PinX",
    "PinY",
    "LocPinX",
    "LocPinY",
    "Width",
    "Height",
    "TxtPinX",
    "TxtPinY",
    "TxtLocPinX",
    "TxtLocPinY",
    "TxtWidth",
    "TxtHeight",
    "BeginX",
    "BeginY",
    "EndX",
    "EndY",
    "LineWeight",
    "Rounding",
    "ShapeShdwOffsetX",
    "ShapeShdwOffsetY",
    "LeftMargin",
    "RightMargin",
    "TopMargin",
    "BottomMargin",
    "DefaultTabStop",
];

/// The geometry row kinds that scale, with the cells that hold a
/// length. `EllipticalArcTo`'s C and D (angle and eccentricity) and
/// the spline knots and weights do not scale.
const GEOMETRY_LENGTH_CELLS: &[(&str, &[&str])] = &[
    ("MoveTo", &["X", "Y"]),
    ("LineTo", &["X", "Y"]),
    ("ArcTo", &["X", "Y", "A"]),
    ("InfiniteLine", &["X", "Y", "A", "B"]),
    ("Ellipse", &["X", "Y", "A", "B", "C", "D"]),
    ("EllipticalArcTo", &["X", "Y", "A", "B"]),
    ("SplineStart", &["X", "Y"]),
];

/// Scales every length of the document to physical inches.
///
/// Each page scales by its own `PageScale`/`DrawingScale`; the masters
/// and the stylesheet have no page of their own and take the scale of
/// the first page that has one (the corpus's drawings carry one
/// drawing scale per document).
pub(crate) fn scale_document(document: &mut Document) {
    let mut document_scale = 1.0f64;
    for index in 0..document.pages.len() {
        let scale = page_scale(document, index);
        if index == 0 {
            document_scale = scale;
        }
        let ids: Vec<ShapeId> = document
            .pages
            .get(index)
            .map(|page| page.shapes.clone())
            .unwrap_or_default();
        for id in ids {
            scale_tree(document, id, scale);
        }
    }
    if !scaled(document_scale) {
        return;
    }
    let master_ids: Vec<ShapeId> = document
        .masters
        .iter()
        .flat_map(|master| master.shapes.iter().copied())
        .collect();
    for id in master_ids {
        scale_tree(document, id, document_scale);
    }
    for style in &mut document.stylesheet.styles {
        scale_sheet(&mut style.sheet, document_scale);
    }
}

/// The drawing scale of one page: `|PageScale / DrawingScale|`, with a
/// zero `DrawingScale` dividing as 1 and a missing or unusable pair
/// scaling by 1 (the rule [`crate::pages`] applies to the page-size
/// pair).
fn page_scale(document: &Document, index: usize) -> f64 {
    let number = |name: &str| -> Option<f64> {
        let page = document.pages.get(index)?;
        page.sheet
            .cells
            .iter()
            .find(|cell| cell.name.as_str() == name)
            .and_then(|cell| cell.value.as_ref())
            .and_then(|value| match value {
                CellValue::Number(value) if value.is_finite() => Some(*value),
                _ => None,
            })
    };
    let page_scale = number("PageScale").unwrap_or(1.0);
    let drawing_scale = match number("DrawingScale") {
        Some(value) if value != 0.0 => value,
        _ => 1.0,
    };
    let scale = (page_scale / drawing_scale).abs();
    if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    }
}

/// Whether `scale` is a scale to apply at all.
fn scaled(scale: f64) -> bool {
    scale.is_finite() && scale > 0.0 && scale != 1.0
}

/// Scales one shape and its sub-shapes.
fn scale_tree(document: &mut Document, id: ShapeId, scale: f64) {
    if !scaled(scale) {
        return;
    }
    let children: Vec<ShapeId> = document
        .shape(id)
        .map(|shape| shape.children.clone())
        .unwrap_or_default();
    if let Some(shape) = document.shapes.get_mut(id.index as usize) {
        scale_shape(shape, scale);
    }
    for child in children {
        scale_tree(document, child, scale);
    }
}

/// Scales one shape's direct cells and geometry rows.
fn scale_shape(shape: &mut Shape, scale: f64) {
    scale_sheet(&mut shape.sheet, scale);
}

/// Scales the length cells and geometry rows of one sheet.
fn scale_sheet(sheet: &mut larrygram_model::Sheet, scale: f64) {
    for cell in &mut sheet.cells {
        if LENGTH_CELLS.contains(&cell.name.as_str()) {
            scale_cell(cell, scale);
        }
    }
    for section in &mut sheet.sections {
        for row in &mut section.rows {
            scale_row(row, scale);
        }
    }
}

/// Scales the length cells of one geometry row.
fn scale_row(row: &mut Row, scale: f64) {
    let Some(row_type) = row.row_type.as_deref() else {
        return;
    };
    let Some((_, names)) = GEOMETRY_LENGTH_CELLS
        .iter()
        .find(|(kind, _)| *kind == row_type)
    else {
        return;
    };
    for cell in &mut row.cells {
        if names.contains(&cell.name.as_str()) {
            scale_cell(cell, scale);
        }
    }
}

/// Scales the cached number of one cell, leaving any other value.
fn scale_cell(cell: &mut Cell, scale: f64) {
    if let Some(CellValue::Number(value)) = cell.value.as_mut() {
        *value *= scale;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use larrygram_geom::Unit;
    use larrygram_model::{CellName, Master, MasterId, Page, Row, Section, SectionName, Sheet};

    /// A number cell.
    fn number(name: &'static str, value: f64) -> Cell {
        let mut cell = Cell::new(CellName::from_static(name));
        cell.value = Some(CellValue::Number(value));
        cell.unit = Unit::Unitsless;
        cell
    }

    /// A page sheet with the given scale pair.
    fn page_sheet(page_scale: f64, drawing_scale: f64) -> Sheet {
        let mut sheet = Sheet::new();
        sheet.cells.push(number("PageScale", page_scale));
        sheet.cells.push(number("DrawingScale", drawing_scale));
        sheet
    }

    /// A shape with one transform, one angle and one geometry row.
    fn shape_with_geometry() -> Shape {
        let mut shape = Shape::new(ShapeId { index: 0 }, 1);
        shape.sheet.cells.push(number("PinX", 24.0));
        shape.sheet.cells.push(number("Width", 12.0));
        shape.sheet.cells.push(number("Angle", 1.5));
        shape.sheet.cells.push(number("LinePattern", 1.0));
        let mut row = Row::new();
        row.row_type = Some("ArcTo".to_owned());
        row.cells.push(number("X", 24.0));
        row.cells.push(number("A", 6.0));
        let mut section = Section::new(SectionName::from_static("Geometry"));
        section.rows.push(row);
        let mut row = Row::new();
        row.row_type = Some("EllipticalArcTo".to_owned());
        row.cells.push(number("X", 24.0));
        row.cells.push(number("A", 6.0));
        row.cells.push(number("C", 0.5));
        row.cells.push(number("D", 1.0));
        section.rows.push(row);
        shape.sheet.sections.push(section);
        shape
    }

    /// The value of one cell of a shape, or `None`.
    fn value(document: &Document, id: ShapeId, name: &str) -> Option<f64> {
        let shape = document.shape(id)?;
        shape
            .sheet
            .cells
            .iter()
            .chain(
                shape
                    .sheet
                    .sections
                    .iter()
                    .flat_map(|section| section.rows.iter())
                    .flat_map(|row| row.cells.iter()),
            )
            .find(|cell| cell.name.as_str() == name)
            .and_then(|cell| cell.value.as_ref())
            .and_then(|value| match value {
                CellValue::Number(value) => Some(*value),
                _ => None,
            })
    }

    /// A document with one page (scale pair `0.25`/`12`), one shape of
    /// it and one master shape.
    fn document() -> Document {
        let mut document = Document::new();
        let mut page = Page::new(larrygram_model::PageId { index: 0 }, 0);
        page.sheet = page_sheet(0.25, 12.0);
        page.shapes.push(ShapeId { index: 0 });
        document.pages.push(page);
        document.add_shape(shape_with_geometry());
        let mut master = Master::new(MasterId { index: 0 }, 1);
        master.shapes.push(ShapeId { index: 1 });
        document.masters.push(master);
        let mut master_shape = shape_with_geometry();
        master_shape.id = ShapeId { index: 1 };
        document.shapes.push(master_shape);
        document
    }

    #[test]
    fn a_scaled_drawing_scales_shapes_masters_and_styles() {
        let mut document = document();
        scale_document(&mut document);
        let k = 0.25 / 12.0;
        assert!((value(&document, ShapeId { index: 0 }, "PinX").unwrap() - 24.0 * k).abs() < 1e-12);
        assert!(
            (value(&document, ShapeId { index: 0 }, "Width").unwrap() - 12.0 * k).abs() < 1e-12
        );
        assert!(
            (value(&document, ShapeId { index: 1 }, "PinX").unwrap() - 24.0 * k).abs() < 1e-12,
            "the master's shapes scale too"
        );
    }

    #[test]
    fn angles_patterns_and_curve_parameters_stay() {
        let mut document = document();
        scale_document(&mut document);
        assert_eq!(value(&document, ShapeId { index: 0 }, "Angle"), Some(1.5));
        assert_eq!(
            value(&document, ShapeId { index: 0 }, "LinePattern"),
            Some(1.0)
        );
        assert_eq!(value(&document, ShapeId { index: 0 }, "C"), Some(0.5));
        assert_eq!(value(&document, ShapeId { index: 0 }, "D"), Some(1.0));
        let k = 0.25 / 12.0;
        assert!(
            (value(&document, ShapeId { index: 0 }, "X").unwrap() - 24.0 * k).abs() < 1e-12,
            "the geometry X of an ArcTo row is a length"
        );
    }

    #[test]
    fn a_curve_row_keeps_its_formula_encoded_values() {
        let mut row = Row::new();
        row.row_type = Some("NurbsTo".to_owned());
        row.cells.push(number("X", 24.0));
        scale_row(&mut row, 0.5);
        assert_eq!(
            row.cells.first().and_then(|cell| cell.value.as_ref()),
            Some(&CellValue::Number(24.0)),
            "a `PolylineTo`/`NurbsTo` row stores formula-encoded \
             strings and does not scale"
        );
        let mut row = Row::new();
        row.cells.push(number("X", 24.0));
        scale_row(&mut row, 0.5);
        assert_eq!(
            row.cells.first().and_then(|cell| cell.value.as_ref()),
            Some(&CellValue::Number(24.0))
        );
    }

    #[test]
    fn an_unscaled_drawing_passes_through() {
        let mut document = document();
        document.pages[0].sheet = page_sheet(1.0, 1.0);
        scale_document(&mut document);
        assert_eq!(value(&document, ShapeId { index: 0 }, "PinX"), Some(24.0));
        assert_eq!(value(&document, ShapeId { index: 1 }, "PinX"), Some(24.0));
    }

    #[test]
    fn a_zero_drawing_scale_divides_as_one() {
        // The rule the page-size pair follows: a zero `DrawingScale`
        // divides as 1.
        let mut document = document();
        document.pages[0].sheet = page_sheet(0.25, 0.0);
        scale_document(&mut document);
        let k = 0.25;
        assert!((value(&document, ShapeId { index: 0 }, "PinX").unwrap() - 24.0 * k).abs() < 1e-12);
    }

    #[test]
    fn a_page_without_the_scale_pair_passes_through() {
        let mut document = document();
        document.pages[0].sheet = Sheet::new();
        scale_document(&mut document);
        assert_eq!(value(&document, ShapeId { index: 0 }, "PinX"), Some(24.0));
    }

    #[test]
    fn a_stale_shape_reference_never_panics() {
        let mut document = document();
        document.pages[0].shapes.push(ShapeId { index: 99 });
        scale_document(&mut document);
    }
}
