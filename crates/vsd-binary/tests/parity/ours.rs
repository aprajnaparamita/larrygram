// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Our side of the comparison: an [`Import`] mapped into the neutral
//! [`Doc`] (P8-08, ADR 0072).
//!
//! # Conventions
//!
//! The neutral structure is in the oracle's dump space (inches, y
//! down). Our model is y up, so every point converts with
//! `y_dump = page_height - y`. The transform from a shape's local
//! coordinates to the page is the standard Visio chain: for each shape
//! from the shape itself up to the page's top level, subtract the
//! `LocPin`, apply `FlipX`/`FlipY`, rotate by `Angle`, add the `Pin`
//! (the black-box rule the reference applies; see ADR 0022).
//!
//! The text box is the top-left corner of the text block's box: the
//! box's (0, 0) corner in the block frame is
//! `(TxtPinX - TxtLocPinX, TxtPinY - TxtLocPinY)` in the shape's local
//! coordinates and goes through the same chain. Its rotation is the
//! chain's angles plus `TxtAngle`, negated for the y flip and reduced
//! to `[0, 360)`. Flips are not stated by the dump and are not
//! compared.
//!
//! Effective cell values are read through [`CellReader`], so style
//! inheritance (the shape, its style links and its master chain) is
//! resolved exactly as the renderer resolves it.

use std::collections::BTreeMap;

use larrygram_vsd_binary::model::{
    Cell, CellName, CellReader, CellValue, Document, GeometryRow, Shape, ShapeId, SheetRef,
    TextPart,
};

use crate::neutral::{Doc, Fill, Geometry, Page, Rgb, Shape as NeutralShape, Stroke, TextBox};

/// Maps an imported document into the neutral structure.
pub fn doc(import: &larrygram_vsd_binary::Import) -> Doc {
    let document = &import.document;
    let metadata = metadata(document);
    let mut reader = CellReader::new(document);
    let mut pages = Vec::new();
    for page in &document.pages {
        let page_sheet = SheetRef::Page(page.id);
        let width = reader
            .number(page_sheet, &CellName::from_static("PageWidth"))
            .ok();
        let height = reader
            .number(page_sheet, &CellName::from_static("PageHeight"))
            .ok();
        let page_height = height.unwrap_or(0.0);
        let mut shapes = Vec::new();
        for (position, id) in page.shapes.iter().copied().enumerate() {
            shapes.push(shape_of(
                document,
                &mut reader,
                id,
                &format!("shape {}", position + 1),
                page_height,
            ));
        }
        pages.push(Page {
            name: Some(page.name.clone()),
            width,
            height,
            shapes,
        });
    }
    Doc { metadata, pages }
}

/// The compared document metadata (ADR 0072: title, creator, template,
/// dates).
fn metadata(document: &Document) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for property in &document.properties {
        let name = property.name.to_ascii_lowercase();
        let key = match name.as_str() {
            "title" | "dc:title" => "title",
            "creator" | "dc:creator" => "creator",
            "template" | "librevenge:template" => "template",
            "created" | "creation-date" | "dc:date" | "meta:creation-date" => "date",
            _ => continue,
        };
        // The first property with a name wins.
        out.entry(key.to_owned())
            .or_insert_with(|| property.value.clone());
    }
    out
}

/// One shape and its sub-shapes.
fn shape_of(
    document: &Document,
    reader: &mut CellReader<'_>,
    id: ShapeId,
    label: &str,
    page_height: f64,
) -> NeutralShape {
    let Some(shape) = document.shape(id) else {
        return NeutralShape {
            label: label.to_owned(),
            ..NeutralShape::default()
        };
    };
    let sheet = SheetRef::Shape(id);
    let number = |reader: &mut CellReader<'_>, name: &'static str| {
        reader.number(sheet, &CellName::from_static(name)).ok()
    };

    let fill_pattern = number(reader, "FillPattern");
    let fill = fill_pattern.map(|pattern| match pattern.round() as i64 {
        0 => Fill::None,
        1 => Fill::Solid,
        2..=24 => Fill::Hatch,
        25..=40 => Fill::Gradient,
        _ => Fill::Other,
    });
    let fill_color = reader
        .color(sheet, &CellName::from_static("FillForegnd"))
        .ok()
        .map(rgb);
    let line_pattern = number(reader, "LinePattern");
    let stroke = line_pattern.map(|pattern| match pattern.round() as i64 {
        0 => Stroke::None,
        1 => Stroke::Solid,
        _ => Stroke::Dash,
    });
    let stroke_color = reader
        .color(sheet, &CellName::from_static("LineColor"))
        .ok()
        .map(rgb);
    let stroke_width = number(reader, "LineWeight");
    let shadow = number(reader, "ShadowPattern").map(|pattern| pattern.round() as i64 != 0);
    let shadow_color = reader
        .color(sheet, &CellName::from_static("ShadowForegnd"))
        .ok()
        .map(rgb);

    let text = text_of(shape);
    let text_box = if text.is_some() {
        text_box_of(document, reader, id, page_height)
    } else {
        None
    };
    let geometry = geometry_of(document, id, page_height);

    let mut children = Vec::new();
    for (position, child) in shape.children.iter().copied().enumerate() {
        children.push(shape_of(
            document,
            reader,
            child,
            &format!("{label}.{}", position + 1),
            page_height,
        ));
    }

    NeutralShape {
        label: label.to_owned(),
        text,
        fill,
        fill_color,
        stroke,
        stroke_color,
        stroke_width,
        shadow,
        shadow_color,
        text_box,
        geometry,
        children,
    }
}

/// The shape's text: the literal parts concatenated.
fn text_of(shape: &Shape) -> Option<String> {
    let content = shape.text.as_ref()?;
    let mut out = String::new();
    for part in &content.parts {
        if let TextPart::Text(text) = part {
            out.push_str(text);
        }
    }
    if out.trim().is_empty() {
        None
    } else {
        Some(out)
    }
}

/// The text block's box in dump space.
fn text_box_of(
    document: &Document,
    reader: &mut CellReader<'_>,
    id: ShapeId,
    page_height: f64,
) -> Option<TextBox> {
    let sheet = SheetRef::Shape(id);
    let number = |reader: &mut CellReader<'_>, name: &'static str| {
        reader.number(sheet, &CellName::from_static(name)).ok()
    };
    let xform = xform_of(reader, id);
    let width = number(reader, "TxtWidth").unwrap_or(xform.width);
    let height = number(reader, "TxtHeight").unwrap_or(xform.height);
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    let loc_x = number(reader, "TxtLocPinX").unwrap_or(width / 2.0);
    let loc_y = number(reader, "TxtLocPinY").unwrap_or(height / 2.0);
    let pin_x = number(reader, "TxtPinX").unwrap_or(xform.pin_x);
    let pin_y = number(reader, "TxtPinY").unwrap_or(xform.pin_y);
    let angle = number(reader, "TxtAngle").unwrap_or(0.0);

    // The box's corners in the shape's local coordinates, mapped to
    // dump space; the box is reported by its min-x/min-y corner (the
    // top-left corner in the dump's y-down space).
    let corner = (pin_x - loc_x, pin_y - loc_y);
    let corners = [
        corner,
        (corner.0 + width, corner.1),
        (corner.0, corner.1 + height),
        (corner.0 + width, corner.1 + height),
    ];
    let mut mapped = [(0.0, 0.0); 4];
    for (slot, point) in mapped.iter_mut().zip(corners) {
        let mut p = point;
        for step in chain_of(document, id) {
            p = apply(p, &step);
        }
        *slot = (p.0, page_height - p.1);
    }
    let x = mapped.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
    let y = mapped.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
    let mut total_angle = angle;
    for step in chain_of(document, id) {
        total_angle += step.angle;
    }
    Some(TextBox {
        x,
        y,
        width,
        height,
        rotate_deg: (-total_angle).to_degrees().rem_euclid(360.0),
    })
}

/// The geometry summary in dump space.
fn geometry_of(document: &Document, id: ShapeId, page_height: f64) -> Option<Geometry> {
    let sheet = document.shape_geometry(id).ok()?;
    let mut geometry = Geometry {
        polyline: true,
        ..Geometry::default()
    };
    let mut subpaths: Vec<Vec<(f64, f64)>> = Vec::new();
    for path in &sheet.paths {
        for row in &path.rows {
            match row {
                GeometryRow::MoveTo { x, y } => {
                    subpaths.push(vec![page_point(document, id, (*x, *y), page_height)]);
                }
                GeometryRow::LineTo { x, y } => {
                    let point = page_point(document, id, (*x, *y), page_height);
                    match subpaths.last_mut() {
                        Some(last) => last.push(point),
                        None => subpaths.push(vec![point]),
                    }
                }
                // A row the reference flattens into chords: the vertex
                // lists do not align, so the whole shape's coordinates
                // are not compared (ADR 0072's row-kind rule).
                _ => geometry.polyline = false,
            }
        }
    }
    geometry.subpaths = subpaths.len();
    for mut subpath in subpaths {
        // The rows store the closing vertex explicitly; the oracle
        // closes with `Z` instead (the shared rule of `neutral`).
        crate::neutral::drop_closing_vertex(&mut subpath);
        geometry.points.extend(subpath);
    }
    Some(geometry)
}

/// One point from shape-local coordinates into dump space.
fn page_point(document: &Document, id: ShapeId, point: (f64, f64), page_height: f64) -> (f64, f64) {
    let mut p = point;
    for step in chain_of(document, id) {
        p = apply(p, &step);
    }
    (p.0, page_height - p.1)
}

/// One step of the shape chain: the shape's own transform, then its
/// ancestors' up to the page.
fn chain_of(document: &Document, id: ShapeId) -> Vec<Xform> {
    let mut chain = Vec::new();
    let mut current = Some(id);
    let mut visited = 0usize;
    while let Some(id) = current {
        if visited > 256 {
            break;
        }
        visited += 1;
        let Some(shape) = document.shape(id) else {
            break;
        };
        let mut reader = CellReader::new(document);
        chain.push(xform_of(&mut reader, id));
        current = shape.parent;
    }
    chain
}

/// The shape's `Xform` cells, with the documented fallbacks when a
/// cell is missing.
fn xform_of(reader: &mut CellReader<'_>, id: ShapeId) -> Xform {
    let sheet = SheetRef::Shape(id);
    let number = |reader: &mut CellReader<'_>, name: &'static str| {
        reader.number(sheet, &CellName::from_static(name)).ok()
    };
    let width = number(reader, "Width").unwrap_or(0.0);
    let height = number(reader, "Height").unwrap_or(0.0);
    Xform {
        pin_x: number(reader, "PinX").unwrap_or(0.0),
        pin_y: number(reader, "PinY").unwrap_or(0.0),
        loc_x: number(reader, "LocPinX").unwrap_or(width / 2.0),
        loc_y: number(reader, "LocPinY").unwrap_or(height / 2.0),
        angle: number(reader, "Angle").unwrap_or(0.0),
        flip_x: reader
            .boolean(sheet, &CellName::from_static("FlipX"))
            .unwrap_or(false),
        flip_y: reader
            .boolean(sheet, &CellName::from_static("FlipY"))
            .unwrap_or(false),
        width,
        height,
    }
}

/// One shape-chain transform step.
#[derive(Debug, Clone, Copy, Default)]
struct Xform {
    pin_x: f64,
    pin_y: f64,
    loc_x: f64,
    loc_y: f64,
    angle: f64,
    flip_x: bool,
    flip_y: bool,
    width: f64,
    height: f64,
}

/// Applies one transform step to a point in the step's local
/// coordinates.
fn apply((x, y): (f64, f64), xform: &Xform) -> (f64, f64) {
    let mut x = x - xform.loc_x;
    let mut y = y - xform.loc_y;
    if xform.flip_x {
        x = -x;
    }
    if xform.flip_y {
        y = -y;
    }
    let (sin, cos) = xform.angle.sin_cos();
    let rx = x * cos - y * sin;
    let ry = y * cos + x * sin;
    (rx + xform.pin_x, ry + xform.pin_y)
}

/// The model's colour as an RGB triple.
fn rgb(color: larrygram_vsd_binary::model::Color) -> Rgb {
    Rgb::new(color.r, color.g, color.b)
}

/// Builds a numeric cell, for the hand-built fixtures.
pub fn number_cell(name: &'static str, value: f64) -> Cell {
    let mut cell = Cell::new(CellName::from_static(name));
    cell.set_value(Some(CellValue::Number(value)));
    cell
}

/// Builds a boolean cell, for the hand-built fixtures.
pub fn bool_cell(name: &'static str, value: bool) -> Cell {
    let mut cell = Cell::new(CellName::from_static(name));
    cell.set_value(Some(CellValue::Bool(value)));
    cell
}

/// Builds a colour cell, for the hand-built fixtures.
pub fn color_cell(name: &'static str, r: u8, g: u8, b: u8) -> Cell {
    let mut cell = Cell::new(CellName::from_static(name));
    cell.set_value(Some(CellValue::Color(larrygram_vsd_binary::model::Color {
        r,
        g,
        b,
    })));
    cell
}

#[cfg(test)]
mod tests {
    use super::*;
    use larrygram_vsd_binary::model::{
        DocumentKind, Page as ModelPage, Property, Section, SectionName, TextContent,
    };

    /// An import with one page (8 x 10 in) and one shape at (1, 2),
    /// 3 x 4 in, solid fill, dashed stroke and the text `Hi`.
    fn fixture_import() -> larrygram_vsd_binary::Import {
        let mut document = larrygram_vsd_binary::model::Document::new();
        document.properties.push(Property::new("title", "T"));
        document.properties.push(Property::new("creator", "Ada"));
        document
            .properties
            .push(Property::new("Template", "T.VSTX"));
        document
            .properties
            .push(Property::new("created", "2020-01-01"));

        let mut shape = Shape::new(larrygram_vsd_binary::model::ShapeId { index: 0 }, 7);
        shape.sheet.cells.extend([
            number_cell("PinX", 1.0),
            number_cell("PinY", 2.0),
            number_cell("LocPinX", 1.5),
            number_cell("LocPinY", 2.0),
            number_cell("Width", 3.0),
            number_cell("Height", 4.0),
            number_cell("Angle", 0.0),
            bool_cell("FlipX", false),
            bool_cell("FlipY", false),
            number_cell("FillPattern", 1.0),
            color_cell("FillForegnd", 0x5b, 0x9b, 0xd5),
            number_cell("LinePattern", 2.0),
            color_cell("LineColor", 0xc8, 0xc8, 0xc8),
            number_cell("LineWeight", 0.0139),
            number_cell("ShadowPattern", 0.0),
        ]);
        shape.text = Some(TextContent {
            parts: vec![TextPart::Text("Hi".to_owned())],
        });
        let id = document.add_shape(shape);

        let mut page = ModelPage::new(larrygram_vsd_binary::model::PageId { index: 0 }, 1);
        page.name = "Page-1".to_owned();
        page.sheet.cells.extend([
            number_cell("PageWidth", 8.0),
            number_cell("PageHeight", 10.0),
        ]);
        page.shapes.push(id);
        document.pages.push(page);

        larrygram_vsd_binary::Import {
            document,
            warnings: Vec::new(),
        }
    }

    #[test]
    fn maps_metadata_pages_and_shapes() {
        let doc = doc(&fixture_import());
        assert_eq!(doc.metadata.get("title").map(String::as_str), Some("T"));
        assert_eq!(doc.metadata.get("creator").map(String::as_str), Some("Ada"));
        assert_eq!(
            doc.metadata.get("template").map(String::as_str),
            Some("T.VSTX")
        );
        assert_eq!(
            doc.metadata.get("date").map(String::as_str),
            Some("2020-01-01")
        );
        assert_eq!(doc.pages.len(), 1);
        let page = &doc.pages[0];
        assert_eq!(page.name.as_deref(), Some("Page-1"));
        assert_eq!(page.width, Some(8.0));
        assert_eq!(page.height, Some(10.0));
        assert_eq!(page.shapes.len(), 1);
    }

    #[test]
    fn maps_style_text_and_geometry() {
        let doc = doc(&fixture_import());
        let shape = &doc.pages[0].shapes[0];
        assert_eq!(shape.label, "shape 1");
        assert_eq!(shape.fill, Some(Fill::Solid));
        assert_eq!(shape.fill_color, Some(Rgb::new(0x5b, 0x9b, 0xd5)));
        assert_eq!(shape.stroke, Some(Stroke::Dash));
        assert_eq!(shape.stroke_color, Some(Rgb::new(0xc8, 0xc8, 0xc8)));
        assert_eq!(shape.stroke_width, Some(0.0139));
        assert_eq!(shape.shadow, Some(false));
        assert_eq!(shape.text.as_deref(), Some("Hi"));
    }

    #[test]
    fn the_text_box_is_the_top_left_corner_in_dump_space() {
        let doc = doc(&fixture_import());
        let shape = &doc.pages[0].shapes[0];
        let text_box = shape.text_box.unwrap();
        // The box spans (-0.5, 0) to (2.5, 4) on the page; in the
        // dump's y-down space that is the corner (-0.5, 6).
        assert_eq!(text_box.x, -0.5);
        assert_eq!(text_box.y, 6.0);
        assert_eq!(text_box.width, 3.0);
        assert_eq!(text_box.height, 4.0);
        assert_eq!(text_box.rotate_deg, 0.0);
    }

    #[test]
    fn a_shape_without_text_has_no_text_box() {
        let mut import = fixture_import();
        import.document.shapes[0].text = None;
        let doc = doc(&import);
        assert!(doc.pages[0].shapes[0].text_box.is_none());
    }

    #[test]
    fn geometry_points_land_in_dump_space() {
        let mut import = fixture_import();
        let mut section = Section::new(SectionName::from_static("Geometry"));
        let mut move_to = larrygram_vsd_binary::model::Row::new();
        move_to.row_type = Some("MoveTo".to_owned());
        move_to.cells.push(number_cell("X", 0.0));
        move_to.cells.push(number_cell("Y", 0.0));
        let mut line_to = larrygram_vsd_binary::model::Row::new();
        line_to.row_type = Some("LineTo".to_owned());
        line_to.cells.push(number_cell("X", 3.0));
        line_to.cells.push(number_cell("Y", 4.0));
        section.rows.push(move_to);
        section.rows.push(line_to);
        import.document.shapes[0].sheet.sections.push(section);

        let doc = doc(&import);
        let geometry = doc.pages[0].shapes[0].geometry.as_ref().unwrap();
        assert!(geometry.polyline);
        assert_eq!(geometry.subpaths, 1);
        // Local (0, 0) is at (1 - 1.5, 2 - 2) on the page, y-flipped
        // against the 10 in page: (-0.5, 10) and (2.5, 6).
        assert_eq!(geometry.points, vec![(-0.5, 10.0), (2.5, 6.0)]);
    }

    #[test]
    fn a_closing_vertex_repeating_the_first_is_dropped() {
        let mut import = fixture_import();
        let mut section = Section::new(SectionName::from_static("Geometry"));
        let mut rows = Vec::new();
        for (kind, x, y) in [
            ("MoveTo", 0.0, 0.0),
            ("LineTo", 3.0, 0.0),
            ("LineTo", 3.0, 4.0),
            ("LineTo", 0.0, 4.0),
            ("LineTo", 0.0, 0.0),
        ] {
            let mut row = larrygram_vsd_binary::model::Row::new();
            row.row_type = Some(kind.to_owned());
            row.cells.push(number_cell("X", x));
            row.cells.push(number_cell("Y", y));
            rows.push(row);
        }
        section.rows.extend(rows);
        import.document.shapes[0].sheet.sections.push(section);

        let doc = doc(&import);
        let geometry = doc.pages[0].shapes[0].geometry.as_ref().unwrap();
        assert_eq!(geometry.subpaths, 1);
        // The explicit closing vertex (0, 0) is dropped like the
        // oracle's `Z` close.
        assert_eq!(geometry.points.len(), 4);
    }

    #[test]
    fn a_curved_row_makes_the_geometry_non_polyline() {
        let mut import = fixture_import();
        let mut section = Section::new(SectionName::from_static("Geometry"));
        let mut move_to = larrygram_vsd_binary::model::Row::new();
        move_to.row_type = Some("MoveTo".to_owned());
        move_to.cells.push(number_cell("X", 0.0));
        move_to.cells.push(number_cell("Y", 0.0));
        let mut arc = larrygram_vsd_binary::model::Row::new();
        arc.row_type = Some("ArcTo".to_owned());
        arc.cells.push(number_cell("X", 3.0));
        arc.cells.push(number_cell("Y", 4.0));
        arc.cells.push(number_cell("A", 0.5));
        section.rows.push(move_to);
        section.rows.push(arc);
        import.document.shapes[0].sheet.sections.push(section);

        let doc = doc(&import);
        let geometry = doc.pages[0].shapes[0].geometry.as_ref().unwrap();
        assert!(!geometry.polyline);
    }

    #[test]
    fn an_empty_document_maps_to_an_empty_doc() {
        let import = larrygram_vsd_binary::Import {
            document: larrygram_vsd_binary::model::Document {
                kind: DocumentKind::Drawing,
                ..larrygram_vsd_binary::model::Document::default()
            },
            warnings: Vec::new(),
        };
        let doc = doc(&import);
        assert!(doc.pages.is_empty());
        assert!(doc.metadata.is_empty());
    }
}
