// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The comparator and the report (P8-08, ADR 0072).
//!
//! Two neutral documents (the oracle's and ours) are compared field by
//! field with the normalisation rules of ADR 0072: lengths in inches
//! with a tolerance of 1/1000 in, angles in degrees with 0.01 degree,
//! colours as RGB triples, names after Unicode NFC, trailing
//! whitespace in strings ignored. A field one side does not state is
//! "not compared", never a mismatch. The parity number is
//! `matching / compared`.

use unicode_normalization::UnicodeNormalization;

use crate::neutral::{Doc, Geometry, Page, Shape};

/// The length tolerance: 1/1000 inch (ADR 0072).
pub const LENGTH_TOLERANCE: f64 = 0.001;

/// The angle tolerance: 0.01 degree (ADR 0072).
pub const ANGLE_TOLERANCE: f64 = 0.01;

/// One mismatch line: `place / field / oracle value / our value`
/// (ADR 0072).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mismatch {
    /// Where the field sits, e.g. `page 1 / shape 2`.
    pub place: String,
    /// The field name, e.g. `fill-color`.
    pub field: String,
    /// The oracle's value, printable.
    pub oracle: String,
    /// Our value, printable.
    pub ours: String,
}

impl std::fmt::Display for Mismatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} / {} / {} / {}",
            self.place, self.field, self.oracle, self.ours
        )
    }
}

/// One field neither side states, with the reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    /// Where the field would sit.
    pub place: String,
    /// The field name.
    pub field: String,
    /// Why it is not compared.
    pub reason: String,
}

impl std::fmt::Display for Skipped {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} / {} ({})", self.place, self.field, self.reason)
    }
}

/// The report for one corpus file (ADR 0072: compared fields, matching
/// fields, the parity number, one line per mismatch).
#[derive(Debug, Clone, Default)]
pub struct Report {
    /// The file name the report is about.
    pub file: String,
    /// How many fields were compared.
    pub compared: usize,
    /// How many of them matched.
    pub matching: usize,
    /// The mismatches, one per differing field.
    pub mismatches: Vec<Mismatch>,
    /// The fields not compared, with their reasons.
    pub skipped: Vec<Skipped>,
    /// A note printed with the report (e.g. the import is still a
    /// stub).
    pub note: Option<String>,
}

impl Report {
    /// The parity number: matching / compared, as a percentage.
    pub fn parity(&self) -> f64 {
        if self.compared == 0 {
            100.0
        } else {
            100.0 * self.matching as f64 / self.compared as f64
        }
    }

    /// The printed report.
    pub fn render(&self) -> String {
        let mut out = format!("== {}\n", self.file);
        if let Some(note) = &self.note {
            out.push_str(&format!("note: {note}\n"));
        }
        out.push_str(&format!("compared fields: {}\n", self.compared));
        out.push_str(&format!("matching fields: {}\n", self.matching));
        out.push_str(&format!(
            "parity: {:.1}% ({}/{})\n",
            self.parity(),
            self.matching,
            self.compared
        ));
        out.push_str(&format!("mismatches ({}):\n", self.mismatches.len()));
        for mismatch in &self.mismatches {
            out.push_str(&format!("  {mismatch}\n"));
        }
        out.push_str(&format!("not compared ({}):\n", self.skipped.len()));
        for skipped in &self.skipped {
            out.push_str(&format!("  {skipped}\n"));
        }
        out.push_str("not compared for every file:\n");
        for line in ALWAYS_NOT_COMPARED {
            out.push_str(&format!("  - {line}\n"));
        }
        out
    }
}

/// The fields the comparison never covers (ADR 0070's drops and the
/// limits of the oracle's dump).
pub const ALWAYS_NOT_COMPARED: [&str; 7] = [
    "shape transforms as values (position, size, angle): vsd2raw bakes them into the path coordinates; they enter the geometry comparison",
    "FlipX/FlipY: the dump does not state flips",
    "background pages: vsd2raw does not state them",
    "stylesheet rows and master shapes: compared through each shape's resolved cells instead",
    "foreign data (OLE objects, pictures), hyperlinks, events and macro references, annotations, reviewers, smart tags, the document font list and shape-data custom labels (ADR 0070 drops)",
    "curved geometry coordinates: the oracle flattens curves into line chords, so vertex lists do not align (row kinds are compared through `geometry.polyline`)",
    "group structure (shape nesting): vsd2raw states one layer per drawing shape and nests a master's rendering groups inside the layer, while our model nests group sub-shapes like the `.vsdx` reader does; both sides are flattened to one entry per drawing shape",
];

/// Compares the oracle's document against ours.
pub fn compare(file: &str, oracle: &Doc, ours: &Doc) -> Report {
    let mut report = Report {
        file: file.to_owned(),
        ..Report::default()
    };
    for key in crate::neutral::METADATA_KEYS {
        let place = "document".to_owned();
        let field = key.to_owned();
        match (oracle.metadata.get(key), ours.metadata.get(key)) {
            (Some(a), Some(b)) if key == "template" => {
                // The oracle emits the template's file name; the file
                // stores the whole path. Compare the file name.
                check_string(
                    &mut report,
                    &place,
                    &field,
                    file_name_of(a),
                    file_name_of(b),
                )
            }
            (Some(a), Some(b)) => check_string(&mut report, &place, &field, a, b),
            (a, b) => skip_stated(&mut report, &place, &field, a.is_some(), b.is_some()),
        }
    }
    check_number(
        &mut report,
        "document",
        "page count",
        Some(oracle.pages.len() as f64),
        Some(ours.pages.len() as f64),
    );
    for (index, (a, b)) in oracle.pages.iter().zip(ours.pages.iter()).enumerate() {
        let place = format!("page {}", index + 1);
        compare_page(&mut report, &place, a, b);
    }
    report
}

/// Compares one page.
fn compare_page(report: &mut Report, place: &str, oracle: &Page, ours: &Page) {
    match (&oracle.name, &ours.name) {
        (Some(a), Some(b)) => check_string(report, place, "name", a, b),
        (a, b) => skip_stated(report, place, "name", a.is_some(), b.is_some()),
    }
    check_length(report, place, "width", oracle.width, ours.width);
    check_length(report, place, "height", oracle.height, ours.height);
    check_number(
        report,
        place,
        "shape count",
        Some(oracle.shapes.len() as f64),
        Some(drawing_shapes(&ours.shapes).len() as f64),
    );
    let ours_shapes = drawing_shapes(&ours.shapes);
    for (index, (a, b)) in oracle.shapes.iter().zip(ours_shapes.iter()).enumerate() {
        let a = flatten(a);
        let shape_place = if a.label.is_empty() {
            format!("{place} / shape {}", index + 1)
        } else {
            format!("{place} / {}", a.label)
        };
        compare_shape(report, &shape_place, &a, b);
    }
}

/// Whether a shape draws anything: geometry, text or a text box.
///
/// A shape that draws nothing (a group whose content is all in its
/// sub-shapes) is not an entry of the per-shape list.
fn draws(shape: &Shape) -> bool {
    let geometry = shape.geometry.as_ref();
    let draws_geometry =
        geometry.is_some_and(|geometry| geometry.subpaths > 0 || !geometry.polyline);
    draws_geometry || shape.text.is_some() || shape.text_box.is_some()
}

/// Our shapes flattened to one entry per drawing shape.
///
/// Our model nests group sub-shapes the way the `.vsdx` reader keeps
/// them (P8-15 goal 3), while the oracle's dump states one layer per
/// drawing shape and no group structure. The comparison therefore
/// walks our tree in document order and keeps each drawing shape with
/// its own content: a group with its own geometry or text is an entry,
/// and so is each of its sub-shapes.
fn drawing_shapes(shapes: &[Shape]) -> Vec<Shape> {
    let mut out = Vec::new();
    for shape in shapes {
        push_drawing_shapes(shape, &mut out);
    }
    out
}

/// Appends `shape` and its descendants, in document order.
fn push_drawing_shapes(shape: &Shape, out: &mut Vec<Shape>) {
    if draws(shape) {
        let mut own = shape.clone();
        own.children = Vec::new();
        out.push(own);
    }
    for child in &shape.children {
        push_drawing_shapes(child, out);
    }
}

/// One shape with every descendant's content merged in (the "shape"
/// unit of the oracle's side of the comparison).
///
/// `vsd2raw` nests rendering groups (a master's geometry, a group's
/// sub-shape content) as anonymous layers inside the shape's layer, so
/// the dump states one shape per drawing shape. On our side the model
/// nests group sub-shapes like the `.vsdx` reader does, so its
/// per-shape list is [`drawing_shapes`]. Text concatenates, style
/// facts take the first stated value, the geometry merges and the
/// first text box wins.
fn flatten(shape: &Shape) -> Shape {
    let mut flat = shape.clone();
    flat.children = Vec::new();
    let mut descendants = Vec::new();
    collect_descendants(shape, &mut descendants);
    for child in descendants {
        if let Some(text) = &child.text {
            flat.text = Some(match flat.text.take() {
                Some(mut own) => {
                    own.push_str(text);
                    own
                }
                None => text.clone(),
            });
        }
        if flat.fill.is_none() {
            flat.fill = child.fill;
        }
        if flat.fill_color.is_none() {
            flat.fill_color = child.fill_color;
        }
        if flat.stroke.is_none() {
            flat.stroke = child.stroke;
        }
        if flat.stroke_color.is_none() {
            flat.stroke_color = child.stroke_color;
        }
        if flat.stroke_width.is_none() {
            flat.stroke_width = child.stroke_width;
        }
        if flat.shadow.is_none() {
            flat.shadow = child.shadow;
        }
        if flat.shadow_color.is_none() {
            flat.shadow_color = child.shadow_color;
        }
        if flat.text_box.is_none() {
            flat.text_box = child.text_box;
        }
        if let Some(geometry) = &child.geometry {
            let merged = flat.geometry.get_or_insert_with(|| Geometry {
                polyline: true,
                ..Geometry::default()
            });
            merged.subpaths += geometry.subpaths;
            merged.polyline = merged.polyline && geometry.polyline;
            merged.points.extend(geometry.points.iter().copied());
        }
    }
    flat
}

/// Every descendant of `shape`, depth first.
fn collect_descendants<'a>(shape: &'a Shape, out: &mut Vec<&'a Shape>) {
    for child in &shape.children {
        out.push(child);
        collect_descendants(child, out);
    }
}

/// Compares one shape and its sub-shapes.
fn compare_shape(report: &mut Report, place: &str, oracle: &Shape, ours: &Shape) {
    match (&oracle.text, &ours.text) {
        (Some(a), Some(b)) => check_string(report, place, "text", a, b),
        (a, b) => skip_stated(report, place, "text", a.is_some(), b.is_some()),
    }
    match (oracle.fill, ours.fill) {
        (Some(a), Some(b)) => check_equal(report, place, "fill", a.label(), b.label()),
        (a, b) => skip_stated(report, place, "fill", a.is_some(), b.is_some()),
    }
    match (oracle.fill_color, ours.fill_color) {
        (Some(a), Some(b)) => check_equal(report, place, "fill-color", &a.hex(), &b.hex()),
        (a, b) => skip_stated(report, place, "fill-color", a.is_some(), b.is_some()),
    }
    match (oracle.stroke, ours.stroke) {
        (Some(a), Some(b)) => check_equal(report, place, "stroke", a.label(), b.label()),
        (a, b) => skip_stated(report, place, "stroke", a.is_some(), b.is_some()),
    }
    match (oracle.stroke_color, ours.stroke_color) {
        (Some(a), Some(b)) => check_equal(report, place, "stroke-color", &a.hex(), &b.hex()),
        (a, b) => skip_stated(report, place, "stroke-color", a.is_some(), b.is_some()),
    }
    check_length(
        report,
        place,
        "stroke-width",
        oracle.stroke_width,
        ours.stroke_width,
    );
    match (oracle.shadow, ours.shadow) {
        (Some(a), Some(b)) => check_equal(report, place, "shadow", &a.to_string(), &b.to_string()),
        (a, b) => skip_stated(report, place, "shadow", a.is_some(), b.is_some()),
    }
    match (oracle.shadow_color, ours.shadow_color) {
        (Some(a), Some(b)) => check_equal(report, place, "shadow-color", &a.hex(), &b.hex()),
        (a, b) => skip_stated(report, place, "shadow-color", a.is_some(), b.is_some()),
    }
    match (oracle.text_box, ours.text_box) {
        (Some(a), Some(b)) => {
            check_length(report, place, "text-box.x", Some(a.x), Some(b.x));
            check_length(report, place, "text-box.y", Some(a.y), Some(b.y));
            check_length(
                report,
                place,
                "text-box.width",
                Some(a.width),
                Some(b.width),
            );
            check_length(
                report,
                place,
                "text-box.height",
                Some(a.height),
                Some(b.height),
            );
            check_angle(
                report,
                place,
                "text-box.rotate",
                Some(a.rotate_deg),
                Some(b.rotate_deg),
            );
        }
        (a, b) => skip_stated(report, place, "text-box", a.is_some(), b.is_some()),
    }
    compare_geometry(
        report,
        place,
        oracle.geometry.as_ref(),
        ours.geometry.as_ref(),
    );
    for (index, (a, b)) in oracle.children.iter().zip(ours.children.iter()).enumerate() {
        let child_place = if a.label.is_empty() {
            format!("{place} / shape {}", index + 1)
        } else {
            format!("{place} / {}", a.label)
        };
        compare_shape(report, &child_place, a, b);
    }
    if oracle.children.len() != ours.children.len() {
        report.mismatches.push(Mismatch {
            place: place.to_owned(),
            field: "sub-shape count".to_owned(),
            oracle: oracle.children.len().to_string(),
            ours: ours.children.len().to_string(),
        });
        report.compared += 1;
    }
}

/// Compares the geometry summary.
fn compare_geometry(
    report: &mut Report,
    place: &str,
    oracle: Option<&Geometry>,
    ours: Option<&Geometry>,
) {
    let (Some(oracle), Some(ours)) = (oracle, ours) else {
        skip_stated(report, place, "geometry", oracle.is_some(), ours.is_some());
        return;
    };
    check_number(
        report,
        place,
        "geometry.subpaths",
        Some(oracle.subpaths as f64),
        Some(ours.subpaths as f64),
    );
    check_equal(
        report,
        place,
        "geometry.polyline",
        &oracle.polyline.to_string(),
        &ours.polyline.to_string(),
    );
    if oracle.polyline && ours.polyline {
        if oracle.points.len() != ours.points.len() {
            report.mismatches.push(Mismatch {
                place: place.to_owned(),
                field: "geometry.vertex count".to_owned(),
                oracle: oracle.points.len().to_string(),
                ours: ours.points.len().to_string(),
            });
            report.compared += 1;
        } else {
            for (i, (a, b)) in oracle.points.iter().zip(ours.points.iter()).enumerate() {
                check_length(
                    report,
                    place,
                    &format!("geometry.vertex[{i}].x"),
                    Some(a.0),
                    Some(b.0),
                );
                check_length(
                    report,
                    place,
                    &format!("geometry.vertex[{i}].y"),
                    Some(a.1),
                    Some(b.1),
                );
            }
        }
    } else {
        report.skipped.push(Skipped {
            place: place.to_owned(),
            field: "geometry.vertices".to_owned(),
            reason: "curved geometry: the oracle flattens curves into line chords".to_owned(),
        });
    }
}

/// The ordered non-empty texts of every shape in the document,
/// depth-first (the order the XHTML rendering states).
pub fn shape_texts(doc: &Doc) -> Vec<String> {
    fn walk(shapes: &[Shape], out: &mut Vec<String>) {
        for shape in shapes {
            if let Some(text) = &shape.text {
                let normalised = normalise(text);
                if !normalised.is_empty() {
                    out.push(normalised);
                }
            }
            walk(&shape.children, out);
        }
    }
    let mut out = Vec::new();
    for page in &doc.pages {
        walk(&page.shapes, &mut out);
    }
    out
}

/// Compares the text strings and their order (ADR 0072: the
/// `vsd2xhtml` side of the comparison).
///
/// The strings concatenate in order and compare as one string: the
/// two sides split runs differently (a field's display text is its
/// own `svg:tspan` in the XHTML), so the run boundaries are not
/// compared.
pub fn compare_text(report: &mut Report, oracle: &[String], ours: &[String]) {
    report.skipped.push(Skipped {
        place: "document".to_owned(),
        field: "text run boundaries".to_owned(),
        reason: "the two sides split runs differently (a field's display text is its own span in the XHTML)"
            .to_owned(),
    });
    check_string(
        report,
        "document",
        "xhtml.text",
        &oracle.join(""),
        &ours.join(""),
    );
}

/// Records a string comparison after NFC and trailing-whitespace
/// normalisation.
fn check_string(report: &mut Report, place: &str, field: &str, oracle: &str, ours: &str) {
    let a = normalise(oracle);
    let b = normalise(ours);
    check_equal(report, place, field, &a, &b);
}

/// Records a length comparison in inches.
fn check_length(
    report: &mut Report,
    place: &str,
    field: &str,
    oracle: Option<f64>,
    ours: Option<f64>,
) {
    check_number_with(report, place, field, oracle, ours, LENGTH_TOLERANCE);
}

/// Records an angle comparison in degrees.
fn check_angle(
    report: &mut Report,
    place: &str,
    field: &str,
    oracle: Option<f64>,
    ours: Option<f64>,
) {
    check_number_with(report, place, field, oracle, ours, ANGLE_TOLERANCE);
}

/// Records an exact number comparison.
fn check_number(
    report: &mut Report,
    place: &str,
    field: &str,
    oracle: Option<f64>,
    ours: Option<f64>,
) {
    check_number_with(report, place, field, oracle, ours, 0.0);
}

/// Records a numeric comparison with a tolerance.
fn check_number_with(
    report: &mut Report,
    place: &str,
    field: &str,
    oracle: Option<f64>,
    ours: Option<f64>,
    tolerance: f64,
) {
    match (oracle, ours) {
        (Some(a), Some(b)) => {
            report.compared += 1;
            if (a - b).abs() <= tolerance {
                report.matching += 1;
            } else {
                report.mismatches.push(Mismatch {
                    place: place.to_owned(),
                    field: field.to_owned(),
                    oracle: format!("{a:.4}"),
                    ours: format!("{b:.4}"),
                });
            }
        }
        (a, b) => skip_stated(report, place, field, a.is_some(), b.is_some()),
    }
}

/// Records an exact comparison of printable values.
fn check_equal(report: &mut Report, place: &str, field: &str, oracle: &str, ours: &str) {
    report.compared += 1;
    if oracle == ours {
        report.matching += 1;
    } else {
        report.mismatches.push(Mismatch {
            place: place.to_owned(),
            field: field.to_owned(),
            oracle: oracle.to_owned(),
            ours: ours.to_owned(),
        });
    }
}

/// Records a field one side does not state.
fn skip_stated(report: &mut Report, place: &str, field: &str, oracle: bool, ours: bool) {
    let reason = match (oracle, ours) {
        (false, false) => "neither side states it",
        (false, true) => "the oracle does not state it",
        (true, false) => "our side does not state it",
        (true, true) => "not compared",
    };
    report.skipped.push(Skipped {
        place: place.to_owned(),
        field: field.to_owned(),
        reason: reason.to_owned(),
    });
}

/// The last path component of a `Template` value (`a\\b\\NAME.VSTX` or
/// `a/b/NAME.VSTX` becomes `NAME.VSTX`).
pub fn file_name_of(value: &str) -> &str {
    match value.rfind(['\\', '/']) {
        Some(pos) => value.get(pos + 1..).unwrap_or(value),
        None => value,
    }
}

/// ADR 0072's string normalisation: Unicode NFC and trailing
/// whitespace ignored.
pub fn normalise(text: &str) -> String {
    let mut out: String = text.nfc().collect();
    while out.ends_with(char::is_whitespace) {
        out.pop();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::neutral::{Fill, Rgb, Shape as NeutralShape, Stroke, TextBox};

    /// A one-page, one-shape document that agrees with itself.
    fn agreeing_docs() -> (Doc, Doc) {
        let shape = NeutralShape {
            label: "id1".to_owned(),
            text: Some("Hi".to_owned()),
            fill: Some(Fill::Solid),
            fill_color: Some(Rgb::new(0x5b, 0x9b, 0xd5)),
            stroke: Some(Stroke::Dash),
            stroke_color: Some(Rgb::new(0, 0, 0)),
            stroke_width: Some(0.0139),
            shadow: Some(false),
            shadow_color: Some(Rgb::new(0x10, 0x10, 0x10)),
            text_box: Some(TextBox {
                x: 1.0,
                y: 2.0,
                width: 3.0,
                height: 4.0,
                rotate_deg: 90.0,
            }),
            geometry: Some(Geometry {
                subpaths: 1,
                polyline: true,
                points: vec![(1.0, 2.0), (3.0, 4.0)],
            }),
            children: Vec::new(),
        };
        let page = Page {
            name: Some("Page-1".to_owned()),
            width: Some(8.0),
            height: Some(10.0),
            shapes: vec![shape],
        };
        let mut metadata = std::collections::BTreeMap::new();
        metadata.insert("title".to_owned(), "T".to_owned());
        metadata.insert("creator".to_owned(), "Ada".to_owned());
        metadata.insert("template".to_owned(), "T.VSTX".to_owned());
        metadata.insert("date".to_owned(), "2020-01-01".to_owned());
        let oracle = Doc {
            metadata: metadata.clone(),
            pages: vec![page.clone()],
        };
        let ours = Doc {
            metadata,
            pages: vec![page],
        };
        (oracle, ours)
    }

    #[test]
    fn agreeing_documents_compare_clean() {
        let (oracle, ours) = agreeing_docs();
        let report = compare("fixture", &oracle, &ours);
        assert!(report.mismatches.is_empty(), "{}", report.render());
        assert!(report.skipped.is_empty(), "{}", report.render());
        assert_eq!(report.parity(), 100.0);
        assert!(report.compared >= 20);
    }

    #[test]
    fn each_mismatch_is_one_line_with_place_field_and_both_values() {
        let (oracle, mut ours) = agreeing_docs();
        ours.metadata.insert("title".to_owned(), "Other".to_owned());
        ours.pages[0].shapes[0].fill_color = Some(Rgb::new(1, 2, 3));
        ours.pages[0].width = Some(8.005);
        ours.pages[0].shapes[0].geometry.as_mut().unwrap().points[1].0 = 3.1;
        let report = compare("fixture", &oracle, &ours);
        let lines: Vec<String> = report.mismatches.iter().map(|m| m.to_string()).collect();
        assert!(
            lines.contains(&"document / title / T / Other".to_owned()),
            "{lines:?}"
        );
        assert!(
            lines.contains(&"page 1 / id1 / fill-color / #5b9bd5 / #010203".to_owned()),
            "{lines:?}"
        );
        assert!(
            lines.iter().any(|l| l.starts_with("page 1 / width /")),
            "{lines:?}"
        );
        assert!(
            lines.contains(&"page 1 / id1 / geometry.vertex[1].x / 3.0000 / 3.1000".to_owned()),
            "{lines:?}"
        );
        assert_eq!(
            report.parity(),
            100.0 * (report.matching as f64) / (report.compared as f64)
        );
    }

    #[test]
    fn lengths_use_the_one_thousandth_inch_tolerance() {
        let (oracle, mut ours) = agreeing_docs();
        ours.pages[0].height = Some(10.0 + 0.0009);
        let report = compare("fixture", &oracle, &ours);
        assert!(report.mismatches.is_empty(), "{}", report.render());
        ours.pages[0].height = Some(10.0 + 0.002);
        let report = compare("fixture", &oracle, &ours);
        assert_eq!(report.mismatches.len(), 1, "{}", report.render());
    }

    #[test]
    fn angles_use_the_hundredth_degree_tolerance() {
        let (oracle, mut ours) = agreeing_docs();
        ours.pages[0].shapes[0]
            .text_box
            .as_mut()
            .unwrap()
            .rotate_deg = 90.005;
        let report = compare("fixture", &oracle, &ours);
        assert!(report.mismatches.is_empty(), "{}", report.render());
        ours.pages[0].shapes[0]
            .text_box
            .as_mut()
            .unwrap()
            .rotate_deg = 90.02;
        let report = compare("fixture", &oracle, &ours);
        assert_eq!(report.mismatches.len(), 1, "{}", report.render());
    }

    #[test]
    fn strings_compare_after_nfc_and_ignoring_trailing_whitespace() {
        let (oracle, mut ours) = agreeing_docs();
        ours.pages[0].shapes[0].text = Some("Hi  \n".to_owned());
        let report = compare("fixture", &oracle, &ours);
        assert!(report.mismatches.is_empty(), "{}", report.render());
        // Decomposed `e` + combining acute vs precomposed `é`.
        ours.pages[0].shapes[0].text = Some("cafe\u{301}".to_owned());
        let mut oracle2 = oracle.clone();
        oracle2.pages[0].shapes[0].text = Some("caf\u{e9}".to_owned());
        let report = compare("fixture", &oracle2, &ours);
        assert!(report.mismatches.is_empty(), "{}", report.render());
    }

    #[test]
    fn a_field_one_side_lacks_is_not_compared() {
        let (oracle, mut ours) = agreeing_docs();
        ours.pages[0].shapes[0].fill_color = None;
        let report = compare("fixture", &oracle, &ours);
        assert!(report.mismatches.is_empty(), "{}", report.render());
        assert!(report.skipped.iter().any(|s| s.field == "fill-color"));
        assert_eq!(report.parity(), 100.0);
    }

    #[test]
    fn curved_geometry_skips_the_vertex_lists() {
        let (oracle, mut ours) = agreeing_docs();
        ours.pages[0].shapes[0].geometry = Some(Geometry {
            subpaths: 1,
            polyline: false,
            points: vec![],
        });
        let report = compare("fixture", &oracle, &ours);
        assert!(
            report
                .skipped
                .iter()
                .any(|s| s.field == "geometry.vertices")
        );
        assert!(
            report
                .mismatches
                .iter()
                .any(|m| m.field == "geometry.polyline")
        );
    }

    #[test]
    fn mismatched_shape_counts_are_reported() {
        let (oracle, mut ours) = agreeing_docs();
        ours.pages[0].shapes.push(NeutralShape {
            label: "id2".to_owned(),
            text: Some("a shape of its own".to_owned()),
            ..NeutralShape::default()
        });
        let report = compare("fixture", &oracle, &ours);
        assert!(report.mismatches.iter().any(|m| m.field == "shape count"));
    }

    #[test]
    fn descendants_are_merged_into_the_shape_they_belong_to() {
        let (oracle, ours) = agreeing_docs();
        // The oracle states the content of one shape as a nested
        // rendering group; our side keeps it on the shape itself.
        let mut nested = oracle.clone();
        let shape = nested.pages[0].shapes.remove(0);
        let (text, rest) = (shape.text.clone(), shape);
        let mut parent = rest.clone();
        parent.text = None;
        parent.children = vec![NeutralShape {
            label: String::new(),
            text,
            ..NeutralShape::default()
        }];
        nested.pages[0].shapes.push(parent);
        let report = compare("fixture", &nested, &ours);
        assert!(report.mismatches.is_empty(), "{}", report.render());
    }

    #[test]
    fn a_group_that_draws_nothing_is_not_an_entry_of_our_list() {
        // Our model nests group sub-shapes (P8-15 goal 3) while the
        // dump states one layer per drawing shape; the comparison
        // flattens our tree to one entry per drawing shape.
        let (oracle, ours) = agreeing_docs();
        let mut nested = ours.clone();
        let shape = nested.pages[0].shapes.remove(0);
        let group = NeutralShape {
            label: "the group".to_owned(),
            children: vec![shape],
            ..NeutralShape::default()
        };
        nested.pages[0].shapes = vec![group];
        let report = compare("fixture", &oracle, &nested);
        assert!(report.mismatches.is_empty(), "{}", report.render());
    }

    #[test]
    fn the_report_renders_the_adr_0072_shape() {
        let (oracle, mut ours) = agreeing_docs();
        ours.pages[0].shapes[0].stroke = Some(Stroke::None);
        let report = compare("fixture.vsd", &oracle, &ours);
        let rendered = report.render();
        assert!(rendered.starts_with("== fixture.vsd\n"), "{rendered}");
        assert!(rendered.contains("compared fields: "));
        assert!(rendered.contains("matching fields: "));
        assert!(rendered.contains("parity: "));
        assert!(rendered.contains("page 1 / id1 / stroke / dash / none"));
        assert!(rendered.contains("not compared for every file:"));
    }

    #[test]
    fn an_empty_comparison_is_one_hundred_percent() {
        let report = compare("empty", &Doc::default(), &Doc::default());
        assert_eq!(report.parity(), 100.0);
    }

    #[test]
    fn shape_texts_walks_depth_first_in_order() {
        let (oracle, ours) = agreeing_docs();
        let mut nested = ours.clone();
        nested.pages[0].shapes[0].children.push(NeutralShape {
            label: "id2".to_owned(),
            text: Some("Sub".to_owned()),
            ..NeutralShape::default()
        });
        assert_eq!(
            shape_texts(&nested),
            vec!["Hi".to_owned(), "Sub".to_owned()]
        );
        assert_eq!(shape_texts(&oracle), vec!["Hi".to_owned()]);
    }

    #[test]
    fn text_order_mismatches_are_reported() {
        let (oracle, ours) = agreeing_docs();
        let mut report = compare("fixture", &oracle, &ours);
        compare_text(
            &mut report,
            &["Hi".to_owned(), "Extra".to_owned()],
            &shape_texts(&ours),
        );
        assert!(report.mismatches.iter().any(|m| m.field == "xhtml.text"));
        // Run boundaries are not compared.
        assert!(
            report
                .skipped
                .iter()
                .any(|s| s.field == "text run boundaries")
        );
    }

    #[test]
    fn equal_text_in_different_run_splits_matches() {
        let (oracle, ours) = agreeing_docs();
        let mut report = compare("fixture", &oracle, &ours);
        compare_text(
            &mut report,
            &["H".to_owned(), "i".to_owned()],
            &shape_texts(&ours),
        );
        assert!(report.mismatches.is_empty(), "{}", report.render());
    }
}
