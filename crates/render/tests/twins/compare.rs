// MIT License
//
// Copyright (c) 2026 the Larrygram project. Permission is hereby
// granted, free of charge, to any person obtaining a copy of this
// software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the
// rights to use, copy, modify, merge, publish, distribute, sublicense,
// and/or sell copies of the Software, and to permit persons to whom the
// Software is furnished to do so, subject to the following conditions:
// The above copyright notice and this permission notice shall be
// included in all copies or substantial portions of the Software.
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
// EXPRESS OR IMPLIED.
//
// Our own code, not ported or derived from libvisio: the comparison is
// format-agnostic and reads only the model (AGENTS.md section 5 keeps
// the MPL code inside `crates/vsd-binary`, so this file moved out of
// it, P8-17 goal 7).

//! The comparison of one `.vsd` import against its `.vsdx` twin
//! (P8-15 goal 8): pages, shapes, the cells the renderer reads, and
//! text, per page.
//!
//! Both sides are [`Document`] values, so the comparison is
//! format-agnostic: the `.vsd` side comes from
//! [`larrygram_vsd_binary::import`] and the twin from the `.vsdx`
//! reader. Shapes join by their Visio `ID` within a page (the file
//! order of the two formats is not the same thing; the twin test does
//! not compare it). A field one side does not state is not compared,
//! never a mismatch (ADR 0072's rule).
//!
//! Numbers compare with ADR 0072's tolerances: lengths within 1/1000
//! inch, angles within 0.01 degree. Colours compare as RGB triples
//! after [`larrygram_model::CellReader`] resolves them (the palette and
//! the theme), which is what the renderer sees. The text compares after
//! trailing whitespace is dropped (ADR 0072's rule; the Unicode NFC the
//! ADR also names is the writer's and the reader's concern).
//!
//! ## Drawing units (P8-21 goal 1)
//!
//! A scaled drawing (a plan at a quarter inch to the foot) stores its
//! lengths in the units the drawing is scaled in and the scale cells
//! beside them; the model stores physical inches (ADR 0070). The
//! comparison therefore multiplies the **twin's** lengths by
//! `PageScale / DrawingScale` before it compares them, the same rule
//! the import's scale pass applies in reverse (each page by its own
//! pair, a zero `DrawingScale` dividing as 1, a missing or unusable
//! pair scaling by 1). The twin's page size follows the same rule.
//!
//! Only lengths scale. Angles, patterns, colours, booleans, the
//! fraction cells of the `Rel*` rows and the knot, weight, ratio and
//! degree cells of the curve rows are not lengths and compare as
//! stored ([`cell_is_length`]: what a cell *is*, per ADR 0022 and the
//! ShapeSheet reference, not what the import happens to scale). A
//! value the twin's file does not state compares as the documented
//! default it substitutes (canonical units, [`scales_with_the_drawing`])
//! except the formula defaults, which derive from the twin's own cells.
//! A scaled length that still disagrees after the normalisation is a
//! fault in a reader or the import, never a unit artefact.
//!
//! ## Cell families
//!
//! Every mismatch carries the [`Family`] of the value it reports (the
//! wide twin sweep's report columns): page facts, the shape join, the
//! transform, the line, the fill, the text block, the geometry rows and
//! the text.

use larrygram_model::{CellName, Document, Sheet, SheetRef, TextPart};

/// The length tolerance: 1/1000 inch (ADR 0072).
pub const LENGTH_TOLERANCE: f64 = 0.001;

/// The angle tolerance: 0.01 degree (ADR 0072).
pub const ANGLE_TOLERANCE: f64 = 0.01;

/// The number cells the renderer reads, compared as lengths.
const LENGTH_CELLS: &[&str] = &[
    "PinX",
    "PinY",
    "LocPinX",
    "LocPinY",
    "Width",
    "Height",
    "BeginX",
    "BeginY",
    "EndX",
    "EndY",
    "LineWeight",
    "Rounding",
    "TxtPinX",
    "TxtPinY",
    "TxtLocPinX",
    "TxtLocPinY",
    "TxtWidth",
    "TxtHeight",
    "LeftMargin",
    "RightMargin",
    "TopMargin",
    "BottomMargin",
];

/// The number cells the renderer reads, compared as angles (degrees).
const ANGLE_CELLS: &[&str] = &["Angle", "TxtAngle"];

/// The number cells the renderer reads, compared as plain numbers.
const NUMBER_CELLS: &[&str] = &["LinePattern", "FillPattern", "ShdwPattern"];

/// The colour cells the renderer reads, compared as RGB triples.
const COLOR_CELLS: &[&str] = &["LineColor", "FillForegnd", "FillBkgnd", "ShdwForegnd"];

/// The boolean cells the renderer reads.
const BOOL_CELLS: &[&str] = &["FlipX", "FlipY"];

/// The geometry row cells the renderer reads, compared as lengths
/// (except for the cells [`cell_is_length`] rules out of a row).
const GEOMETRY_CELLS: &[&str] = &["X", "Y", "A", "B", "C", "D"];

/// The family of one compared value (P8-21 goal 2): the report columns
/// of the wide twin sweep.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Family {
    /// The page count, the page name and the page size.
    Page,
    /// The shape join: a shape only one side has.
    Join,
    /// The shape transform (`Pin*`, `Width`, `Height`, `Angle`,
    /// `Flip*`, the 1-D `Begin`/`End`).
    Transform,
    /// The stroke (`LineWeight`, `LinePattern`, `LineColor`, the
    /// corner `Rounding`).
    Line,
    /// The fill and the shadow (`FillPattern`, `FillForegnd`,
    /// `FillBkgnd`, `ShdwPattern`, `ShdwForegnd`).
    Fill,
    /// The text block (`Txt*` and the four margins).
    TextBlock,
    /// The geometry rows: their count and their row cells.
    Geometry,
    /// The literal text.
    Text,
}

impl Family {
    /// Every family, in report order.
    pub const ALL: [Family; 8] = [
        Family::Page,
        Family::Join,
        Family::Transform,
        Family::Line,
        Family::Fill,
        Family::TextBlock,
        Family::Geometry,
        Family::Text,
    ];

    /// The report column name of the family.
    pub const fn as_str(self) -> &'static str {
        match self {
            Family::Page => "page",
            Family::Join => "join",
            Family::Transform => "transform",
            Family::Line => "line",
            Family::Fill => "fill",
            Family::TextBlock => "text_block",
            Family::Geometry => "geometry",
            Family::Text => "text",
        }
    }
}

/// One difference between an import and its twin: the report line, the
/// family of the value it names and the short cause it groups by (the
/// cell name, or what else differs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mismatch {
    /// The report line (`place / cell / twin / import`).
    pub message: String,
    /// The family of the compared value.
    pub family: Family,
    /// The cause the sweep groups by: the cell's name (`PinX`,
    /// `geometry X`), `page count`, `page name`, `shape join`,
    /// `geometry rows` or `text`.
    pub cause: String,
}

/// What one comparison found (P8-21 goal 2): the mismatches and the
/// counts the wide sweep records per file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Comparison {
    /// One entry per difference, in a stable order.
    pub mismatches: Vec<Mismatch>,
    /// The number of values read on both sides.
    pub compared: usize,
    /// The number of compared values that agree.
    pub agreed: usize,
    /// The number of shapes joined by their Visio `ID`.
    pub joined: usize,
}

impl Comparison {
    /// The report lines of the mismatches, in order.
    pub fn messages(&self) -> Vec<String> {
        self.mismatches
            .iter()
            .map(|mismatch| mismatch.message.clone())
            .collect()
    }
}

/// Compares one import against its twin: the mismatches with their
/// families and the counts of one sweep row. The messages are empty
/// when the two documents agree on everything compared.
pub fn compare_documents(our: &Document, twin: &Document) -> Comparison {
    Comparer {
        our,
        twin,
        mismatches: Vec::new(),
        compared: 0,
        agreed: 0,
        joined: 0,
    }
    .run()
}

/// Compares one import against its twin as a report: one message per
/// difference and the number of compared cells (P8-17 goal 7: one
/// report per file). The messages are empty when the two documents
/// agree on everything compared.
pub fn compare_documents_counted(our: &Document, twin: &Document) -> (Vec<String>, usize) {
    let result = compare_documents(our, twin);
    (result.messages(), result.compared)
}

/// The walk over the two documents (one comparison per [`Comparer`]).
struct Comparer<'a> {
    /// The `.vsd` import (physical inches).
    our: &'a Document,
    /// The `.vsdx` twin (the drawing's own units).
    twin: &'a Document,
    /// The differences found so far.
    mismatches: Vec<Mismatch>,
    /// The values read on both sides so far.
    compared: usize,
    /// The compared values that agree so far.
    agreed: usize,
    /// The shapes joined so far.
    joined: usize,
}

impl Comparer<'_> {
    /// Records one difference.
    fn record(&mut self, message: String, family: Family, cause: impl Into<String>) {
        self.mismatches.push(Mismatch {
            message,
            family,
            cause: cause.into(),
        });
    }

    /// Runs the comparison and returns what it found.
    fn run(mut self) -> Comparison {
        if self.our.pages.len() != self.twin.pages.len() {
            self.record(
                format!(
                    "document / page count / {} / {}",
                    self.our.pages.len(),
                    self.twin.pages.len()
                ),
                Family::Page,
                "page count",
            );
        }
        for index in 0..self.our.pages.len().min(self.twin.pages.len()) {
            let place = format!("page {}", index + 1);
            let (Some(a), Some(b)) = (self.our.pages.get(index), self.twin.pages.get(index)) else {
                continue;
            };
            if a.name != b.name {
                self.record(
                    format!("{place} / name / {:?} / {:?}", b.name, a.name),
                    Family::Page,
                    "page name",
                );
            }
            let pair = (page(index), page(index));
            let factor = page_factor(self.twin, index);
            self.cell(
                &place,
                "PageWidth",
                CellKind::Length,
                Family::Page,
                pair,
                factor,
            );
            self.cell(
                &place,
                "PageHeight",
                CellKind::Length,
                Family::Page,
                pair,
                factor,
            );
            self.shapes(&place, index, factor);
        }
        Comparison {
            mismatches: self.mismatches,
            compared: self.compared,
            agreed: self.agreed,
            joined: self.joined,
        }
    }

    /// Compares the shapes of one page, joined by Visio `ID`.
    fn shapes(&mut self, place: &str, page_index: usize, factor: f64) {
        let (Some(a_page), Some(b_page)) = (
            self.our.pages.get(page_index),
            self.twin.pages.get(page_index),
        ) else {
            return;
        };
        let mut ids: Vec<i32> = Vec::new();
        collect_ids(self.our, &a_page.shapes, &mut ids);
        let mut twin_ids: Vec<i32> = Vec::new();
        collect_ids(self.twin, &b_page.shapes, &mut twin_ids);
        for id in &ids {
            if !twin_ids.contains(id) {
                self.record(
                    format!("{place} / shape {id}: only in the .vsd import"),
                    Family::Join,
                    "shape join",
                );
            }
        }
        for id in &twin_ids {
            if !ids.contains(id) {
                self.record(
                    format!("{place} / shape {id}: only in the .vsdx twin"),
                    Family::Join,
                    "shape join",
                );
            }
        }
        for id in &ids {
            if !twin_ids.contains(id) {
                continue;
            }
            let (Some(a), Some(b)) = (
                self.our.shape_by_visio_id(a_page.id, *id),
                self.twin.shape_by_visio_id(b_page.id, *id),
            ) else {
                continue;
            };
            self.joined += 1;
            let shape_place = format!("{place} / shape {id}");
            let pair = (SheetRef::Shape(a), SheetRef::Shape(b));
            for &name in LENGTH_CELLS {
                let family = family_of(name);
                self.cell(&shape_place, name, CellKind::Length, family, pair, factor);
            }
            for &name in ANGLE_CELLS {
                let family = family_of(name);
                self.cell(&shape_place, name, CellKind::Angle, family, pair, factor);
            }
            for &name in NUMBER_CELLS {
                let family = family_of(name);
                self.cell(&shape_place, name, CellKind::Plain, family, pair, factor);
            }
            for &name in COLOR_CELLS {
                let family = family_of(name);
                self.cell(&shape_place, name, CellKind::Color, family, pair, factor);
            }
            for &name in BOOL_CELLS {
                let family = family_of(name);
                self.cell(&shape_place, name, CellKind::Flag, family, pair, factor);
            }
            self.geometry(&shape_place, pair.0, pair.1, factor);
            if let (Some(x), Some(y)) = (text_of(self.our, a), text_of(self.twin, b)) {
                self.compared += 1;
                if x == y {
                    self.agreed += 1;
                } else {
                    self.record(
                        format!("{shape_place} / text / {y:?} / {x:?}"),
                        Family::Text,
                        "text",
                    );
                }
            }
        }
    }

    /// Compares one cell of one shape or page, resolved through the
    /// [`larrygram_model::CellReader`] (the value the renderer reads).
    /// A [`CellKind::Length`] value the twin states in its drawing's
    /// units is multiplied by `factor` first
    /// ([`scales_with_the_drawing`]).
    fn cell(
        &mut self,
        place: &str,
        name: &str,
        kind: CellKind,
        family: Family,
        (a, b): (SheetRef, SheetRef),
        factor: f64,
    ) {
        let name_ref = CellName::new(name);
        let mut left = larrygram_model::CellReader::new(self.our);
        let mut right = larrygram_model::CellReader::new(self.twin);
        match kind {
            CellKind::Length | CellKind::Angle | CellKind::Plain => {
                let tolerance = match kind {
                    CellKind::Angle => ANGLE_TOLERANCE,
                    _ => LENGTH_TOLERANCE,
                };
                let (Ok(x), Ok(y)) = (left.number(a, &name_ref), right.number(b, &name_ref)) else {
                    return;
                };
                if !x.is_finite() || !y.is_finite() {
                    return;
                }
                let y = if kind == CellKind::Length
                    && scales_with_the_drawing(self.twin, b, &name_ref)
                {
                    y * factor
                } else {
                    y
                };
                self.compared += 1;
                let delta = match kind {
                    CellKind::Angle => (x - y).to_degrees().abs(),
                    _ => (x - y).abs(),
                };
                if delta <= tolerance {
                    self.agreed += 1;
                } else {
                    self.record(
                        format!("{place} / {name} / {y:.4} / {x:.4}"),
                        family,
                        name.to_owned(),
                    );
                }
            }
            CellKind::Color => {
                let (Ok(x), Ok(y)) = (left.color(a, &name_ref), right.color(b, &name_ref)) else {
                    return;
                };
                self.compared += 1;
                if x == y {
                    self.agreed += 1;
                } else {
                    self.record(
                        format!(
                            "{place} / {name} / #{:02x}{:02x}{:02x} / #{:02x}{:02x}{:02x}",
                            y.r, y.g, y.b, x.r, x.g, x.b
                        ),
                        family,
                        name.to_owned(),
                    );
                }
            }
            CellKind::Flag => {
                let (Ok(x), Ok(y)) = (left.boolean(a, &name_ref), right.boolean(b, &name_ref))
                else {
                    return;
                };
                self.compared += 1;
                if x == y {
                    self.agreed += 1;
                } else {
                    self.record(
                        format!("{place} / {name} / {y} / {x}"),
                        family,
                        name.to_owned(),
                    );
                }
            }
        }
    }

    /// Compares the geometry rows of one shape: the row counts and the
    /// row cells the renderer reads. A twin row cell that is a length
    /// ([`cell_is_length`]) is multiplied by `factor` first. A row
    /// count that differs is one mismatch of its own and the row cells
    /// are then not compared (P8-17 open issue 2: own rows vs the
    /// resolved row union).
    fn geometry(&mut self, place: &str, a: SheetRef, b: SheetRef, factor: f64) {
        use larrygram_model::RowTarget;
        let a_rows = geometry_rows(self.our, a);
        let b_rows = geometry_rows(self.twin, b);
        self.compared += 1;
        if a_rows != b_rows {
            self.record(
                format!("{place} / geometry rows / {b_rows} / {a_rows}"),
                Family::Geometry,
                "geometry rows",
            );
            return;
        }
        self.agreed += 1;
        for ix in 1..=a_rows {
            let ix = i32::try_from(ix).unwrap_or(i32::MAX);
            let row = RowTarget::by_ix(ix);
            let kind =
                geometry_row_type(self.twin, b, ix).or_else(|| geometry_row_type(self.our, a, ix));
            for &name in GEOMETRY_CELLS {
                let name_ref = CellName::new(name);
                let mut left = larrygram_model::CellReader::new(self.our);
                let mut right = larrygram_model::CellReader::new(self.twin);
                let (Ok(x), Ok(y)) = (
                    left.row_number(a, "Geometry", row, &name_ref),
                    right.row_number(b, "Geometry", row, &name_ref),
                ) else {
                    continue;
                };
                if !x.is_finite() || !y.is_finite() {
                    continue;
                }
                // A missing row cell reads as the hard default 0.0,
                // which a factor leaves unchanged.
                let y = if cell_is_length(kind.as_deref(), name) {
                    y * factor
                } else {
                    y
                };
                self.compared += 1;
                if (x - y).abs() <= LENGTH_TOLERANCE {
                    self.agreed += 1;
                } else {
                    self.record(
                        format!("{place} / geometry row {ix} {name} / {y:.4} / {x:.4}"),
                        Family::Geometry,
                        format!("geometry {name}"),
                    );
                }
            }
        }
    }
}

/// The family of one compared cell name (P8-21 goal 2).
fn family_of(name: &str) -> Family {
    match name {
        "PinX" | "PinY" | "LocPinX" | "LocPinY" | "Width" | "Height" | "Angle" | "FlipX"
        | "FlipY" | "BeginX" | "BeginY" | "EndX" | "EndY" => Family::Transform,
        "LineWeight" | "LinePattern" | "LineColor" | "Rounding" => Family::Line,
        "FillPattern" | "FillForegnd" | "FillBkgnd" | "ShdwPattern" | "ShdwForegnd" => Family::Fill,
        "TxtPinX" | "TxtPinY" | "TxtLocPinX" | "TxtLocPinY" | "TxtWidth" | "TxtHeight"
        | "TxtAngle" | "LeftMargin" | "RightMargin" | "TopMargin" | "BottomMargin" => {
            Family::TextBlock
        }
        _ => Family::Geometry,
    }
}

/// Whether the geometry cell `name` of the row kind `kind` holds a
/// length (P8-21 goal 1, from the row semantics of ADR 0022 and the
/// ShapeSheet reference):
///
/// - `Rel*` rows hold fractions of the shape's width and height, never
///   lengths;
/// - `EllipticalArcTo`'s C is the angle of the major axis and its D
///   the major/minor ratio;
/// - `NurbsTo`'s A to D are knots and weights;
/// - `SplineStart`'s A to D are knots and the degree, `SplineKnot`'s A
///   is its knot;
/// - `PolylineTo`'s A is a point list (a string, never read as a
///   number here).
///
/// Every other row (`MoveTo`, `LineTo`, `ArcTo`, `InfiniteLine`,
/// `Ellipse`, and a row kind the comparison does not know) holds
/// coordinates, radii or the bow of an arc: lengths. An unknown row
/// kind falls in that last rule, which is why the caller passes the
/// twin's kind first.
fn cell_is_length(kind: Option<&str>, name: &str) -> bool {
    match kind.map(str::trim) {
        Some(kind) if kind.starts_with("Rel") => false,
        Some("EllipticalArcTo") => matches!(name, "X" | "Y" | "A" | "B"),
        Some("NurbsTo") | Some("SplineStart") | Some("SplineKnot") | Some("SplineEnd") => {
            matches!(name, "X" | "Y")
        }
        Some("PolylineTo") => matches!(name, "X" | "Y"),
        _ => true,
    }
}

/// The drawing-scale factor of one twin page (P8-21 goal 1):
/// `|PageScale / DrawingScale|`, the rule the import's scale pass
/// applies in reverse (`crates/vsd-binary/src/scale.rs`): a zero
/// `DrawingScale` divides as 1, and a missing or unusable pair scales
/// by 1.
fn page_factor(twin: &Document, index: usize) -> f64 {
    let mut reader = larrygram_model::CellReader::new(twin);
    let sheet = page(index);
    let number = |reader: &mut larrygram_model::CellReader<'_>, name: &str| -> Option<f64> {
        reader
            .number(sheet, &CellName::new(name))
            .ok()
            .filter(|v| v.is_finite())
    };
    let page_scale = number(&mut reader, "PageScale").unwrap_or(1.0);
    let drawing_scale = match number(&mut reader, "DrawingScale") {
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

/// Whether the twin's resolved value of one length cell is stated in
/// the drawing's own units and takes [`page_factor`] (P8-21 goal 1).
///
/// The rungs that state a cell (the shape's or page's own cell, the
/// master's and the style chain's) hold the drawing's units. The rungs
/// that do not (the section default and the hard default) and the
/// documented constant default the reader substitutes are canonical
/// units already: the margins' defaults are documented in inches
/// (ADR 0021). One family derives from the twin's own cells instead and
/// follows the drawing's units: the documented *formula* defaults
/// (`LocPinX = Width * 0.5`, `TxtWidth = Width * 1`).
fn scales_with_the_drawing(document: &Document, sheet: SheetRef, name: &CellName) -> bool {
    use larrygram_model::{CellLookup, DocumentedDefault, ValueSource};
    let resolved = match sheet {
        SheetRef::Shape(id) => document.resolve(id, name),
        SheetRef::Page(id) => document.resolve_page_cell(id, name),
    };
    let Ok(resolved) = resolved else {
        return false;
    };
    if matches!(
        resolved.source,
        ValueSource::Local | ValueSource::Master(_) | ValueSource::Style(_)
    ) {
        return true;
    }
    resolved.source == ValueSource::HardDefault
        && matches!(
            larrygram_model::documented_default(name.as_str()),
            Some(DocumentedDefault::HalfOf(_)) | Some(DocumentedDefault::WholeOf(_))
        )
}

/// The row type (`Row/@T`) of the geometry row `ix` of one sheet, as
/// [`RowTarget::by_ix`] resolves it: the row with that `IX` in the
/// sheet's `Geometry` rows. A row only a master states has no known
/// kind here and falls back to the other side (the caller) and then to
/// [`cell_is_length`]'s default.
fn geometry_row_type(document: &Document, sheet: SheetRef, ix: i32) -> Option<String> {
    let sheet = own_sheet(document, sheet)?;
    sheet
        .sections
        .iter()
        .filter(|section| section.name.as_str() == "Geometry")
        .flat_map(|section| section.rows.iter())
        .find(|row| row.ix == Some(ix))
        .and_then(|row| row.row_type.clone())
}

/// The sheet of a shape or page reference.
fn own_sheet(document: &Document, sheet: SheetRef) -> Option<&Sheet> {
    match sheet {
        SheetRef::Shape(id) => document.shape(id).map(|shape| &shape.sheet),
        SheetRef::Page(id) => document
            .pages
            .get(id.index as usize)
            .map(|page| &page.sheet),
    }
}

/// The sheet reference of one page.
fn page(index: usize) -> SheetRef {
    SheetRef::Page(larrygram_model::PageId {
        index: u32::try_from(index).unwrap_or(u32::MAX),
    })
}

/// The Visio ids of a page's shapes and their sub-shapes, in order.
fn collect_ids(document: &Document, shapes: &[larrygram_model::ShapeId], out: &mut Vec<i32>) {
    for id in shapes {
        let Some(shape) = document.shape(*id) else {
            continue;
        };
        out.push(shape.visio_id);
        collect_ids(document, &shape.children, out);
    }
}

/// The literal text of one shape: the text parts joined, trailing
/// whitespace ignored (ADR 0072), Unicode NFC.
fn text_of(document: &Document, id: larrygram_model::ShapeId) -> Option<String> {
    let content = document.shape(id)?.text.as_ref()?;
    let mut out = String::new();
    for part in &content.parts {
        if let TextPart::Text(text) = part {
            out.push_str(text);
        }
    }
    let trimmed = out.trim_end();
    (!trimmed.is_empty()).then_some(trimmed.to_owned())
}

/// How one compared cell is compared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CellKind {
    /// A length, within [`LENGTH_TOLERANCE`].
    Length,
    /// An angle, within [`ANGLE_TOLERANCE`].
    Angle,
    /// A plain number, within [`LENGTH_TOLERANCE`].
    Plain,
    /// A colour, as an RGB triple.
    Color,
    /// A boolean.
    Flag,
}

/// The number of `Geometry` rows of one shape.
fn geometry_rows(document: &Document, sheet: SheetRef) -> usize {
    let larrygram_model::SheetRef::Shape(id) = sheet else {
        return 0;
    };
    document
        .shape(id)
        .map(|shape| {
            shape
                .sheet
                .sections
                .iter()
                .filter(|section| section.name.as_str() == "Geometry")
                .map(|section| section.rows.len())
                .sum()
        })
        .unwrap_or(0)
}

/// The contract of the comparison, with hand-built documents and no
/// file (the same style as the copy's other suite,
/// `crates/vsd-binary/tests/twins.rs`): the drawing-units
/// normalisation, the family of a mismatch and the counts. The module
/// runs in every suite that includes this file.
#[cfg(test)]
mod tests {
    use super::*;
    use larrygram_model::{
        Cell, CellValue, Document, Page, PageId, Row, Section, Shape, ShapeId, TextContent,
        TextPart,
    };

    /// A number cell.
    fn number(name: &'static str, value: f64) -> Cell {
        let mut cell = Cell::new(CellName::from_static(name));
        cell.value = Some(CellValue::Number(value));
        cell
    }

    /// A shape with one geometry row and one transform.
    fn shape(visio_id: i32, pin_x: f64, row_x: f64) -> Shape {
        let mut shape = Shape::new(ShapeId { index: 0 }, visio_id);
        shape.text = Some(TextContent {
            parts: vec![TextPart::Text("one".to_owned())],
        });
        shape.sheet.cells.push(number("PinX", pin_x));
        shape.sheet.cells.push(number("Width", 2.0));
        shape.sheet.cells.push(number("Angle", 0.25));
        shape.sheet.cells.push(number("FillForegnd", 1.0));
        shape.sheet.cells.push(number("LeftMargin", 0.1));
        let mut section = Section::new(larrygram_model::SectionName::from_static("Geometry"));
        let mut row = Row::new();
        row.row_type = Some("LineTo".to_owned());
        row.ix = Some(1);
        row.cells.push(number("X", row_x));
        row.cells.push(number("Y", 0.0));
        section.rows.push(row);
        shape.sheet.sections.push(section);
        shape
    }

    /// A one-page document with one shape and no drawing scale.
    fn document() -> Document {
        let mut document = Document::new();
        let mut page = Page::new(PageId { index: 0 }, 0);
        page.name = "Page-1".to_owned();
        page.sheet.cells.push(number("PageWidth", 11.0));
        page.sheet.cells.push(number("PageHeight", 8.5));
        page.shapes.push(ShapeId { index: 0 });
        document.pages.push(page);
        document.add_shape(shape(1, 1.0, 2.0));
        document
    }

    /// The same document as [`document`] in drawing units at a quarter
    /// inch to the foot: every length multiplied by 48 and the scale
    /// cells beside them.
    fn scaled_twin() -> Document {
        let mut twin = document();
        twin.pages[0].sheet.cells = vec![
            number("PageWidth", 11.0 * 48.0),
            number("PageHeight", 8.5 * 48.0),
            number("PageScale", 0.25),
            number("DrawingScale", 12.0),
        ];
        let shape = twin.pages[0].shapes.first().copied();
        let shape = shape.and_then(|id| twin.shape_mut(id));
        let Some(shape) = shape else {
            return twin;
        };
        for cell in &mut shape.sheet.cells {
            match cell.name.as_str() {
                "PinX" => cell.value = Some(CellValue::Number(1.0 * 48.0)),
                "Width" => cell.value = Some(CellValue::Number(2.0 * 48.0)),
                "LeftMargin" => cell.value = Some(CellValue::Number(0.1 * 48.0)),
                _ => {}
            }
        }
        for section in &mut shape.sheet.sections {
            for row in &mut section.rows {
                for cell in &mut row.cells {
                    if cell.name.as_str() == "X" {
                        cell.value = Some(CellValue::Number(2.0 * 48.0));
                    }
                }
            }
        }
        twin
    }

    /// One geometry row of one kind with the given cells.
    fn row_of_kind(kind: &str, cells: &[(&'static str, f64)]) -> Row {
        let mut row = Row::new();
        row.row_type = Some(kind.to_owned());
        row.ix = Some(1);
        for (name, value) in cells {
            row.cells.push(number(name, *value));
        }
        row
    }

    /// A one-shape document whose one shape holds the one geometry row.
    fn document_with_row(row: Row) -> Document {
        let mut document = document();
        let Some(id) = document.pages[0].shapes.first().copied() else {
            return document;
        };
        let Some(shape) = document.shape_mut(id) else {
            return document;
        };
        shape.sheet.sections.clear();
        let mut section = Section::new(larrygram_model::SectionName::from_static("Geometry"));
        section.rows.push(row);
        shape.sheet.sections.push(section);
        document
    }

    /// [`scaled_twin`] with its shape's geometry rows replaced by the
    /// one row.
    fn scaled_twin_with_row(row: Row) -> Document {
        let mut twin = scaled_twin();
        let Some(id) = twin.pages[0].shapes.first().copied() else {
            return twin;
        };
        let Some(shape) = twin.shape_mut(id) else {
            return twin;
        };
        shape.sheet.sections.clear();
        let mut section = Section::new(larrygram_model::SectionName::from_static("Geometry"));
        section.rows.push(row);
        shape.sheet.sections.push(section);
        twin
    }

    #[test]
    fn the_drawing_units_of_a_scaled_twin_normalise_before_the_comparison() {
        let result = compare_documents(&document(), &scaled_twin());
        assert!(result.mismatches.is_empty(), "{:?}", result.mismatches);
        assert!(result.compared > 0);
        assert_eq!(result.compared, result.agreed);
        assert_eq!(result.joined, 1);
    }

    #[test]
    fn a_real_difference_survives_the_normalisation() {
        let mut twin = scaled_twin();
        let Some(id) = twin.pages[0].shapes.first().copied() else {
            panic!("the twin has a shape");
        };
        let Some(shape) = twin.shape_mut(id) else {
            panic!("the twin has the shape");
        };
        for cell in &mut shape.sheet.cells {
            if cell.name.as_str() == "PinX" {
                cell.value = Some(CellValue::Number(5.0 * 48.0));
            }
        }
        let result = compare_documents(&document(), &twin);
        assert_eq!(
            result.messages(),
            ["page 1 / shape 1 / PinX / 5.0000 / 1.0000"]
        );
    }

    #[test]
    fn the_drawing_scale_of_a_missing_pair_is_one() {
        let mut twin = scaled_twin();
        twin.pages[0]
            .sheet
            .cells
            .retain(|cell| !matches!(cell.name.as_str(), "PageScale" | "DrawingScale"));
        let result = compare_documents(&document(), &twin);
        assert!(
            result
                .messages()
                .iter()
                .any(|message| message.contains("Width")),
            "{:?}",
            result.messages()
        );
    }

    #[test]
    fn a_zero_drawing_scale_divides_as_one() {
        let mut twin = scaled_twin();
        for cell in &mut twin.pages[0].sheet.cells {
            if cell.name.as_str() == "DrawingScale" {
                cell.value = Some(CellValue::Number(0.0));
            }
        }
        // The factor becomes `PageScale / 1` = 0.25, so the twin's
        // lengths (48 times the model's) stay 12 times the model's.
        let result = compare_documents(&document(), &twin);
        assert!(
            result
                .messages()
                .iter()
                .any(|message| message.contains("Width")),
            "{:?}",
            result.messages()
        );
    }

    #[test]
    fn the_angle_and_ratio_cells_of_a_curve_row_do_not_scale() {
        // `EllipticalArcTo`'s C is the angle of the major axis and its
        // D the major/minor ratio: neither is a length.
        let ours = document_with_row(row_of_kind(
            "EllipticalArcTo",
            &[("X", 2.0), ("Y", 0.0), ("C", 0.5), ("D", 0.25)],
        ));
        let twin = scaled_twin_with_row(row_of_kind(
            "EllipticalArcTo",
            &[("X", 2.0 * 48.0), ("Y", 0.0), ("C", 0.5), ("D", 0.25)],
        ));
        let result = compare_documents(&ours, &twin);
        let geometry: Vec<String> = result
            .mismatches
            .iter()
            .filter(|mismatch| mismatch.family == Family::Geometry)
            .map(|mismatch| mismatch.message.clone())
            .collect();
        assert!(geometry.is_empty(), "{geometry:?}");
    }

    #[test]
    fn the_length_cells_of_an_ellipse_row_scale() {
        // `Ellipse`'s C and D are the second semi-diameter endpoint:
        // lengths.
        let ours = document_with_row(row_of_kind(
            "Ellipse",
            &[("X", 2.0), ("Y", 0.0), ("C", 1.0), ("D", 1.5)],
        ));
        let twin = scaled_twin_with_row(row_of_kind(
            "Ellipse",
            &[
                ("X", 2.0 * 48.0),
                ("Y", 0.0),
                ("C", 1.0 * 48.0),
                ("D", 1.5 * 48.0),
            ],
        ));
        let result = compare_documents(&ours, &twin);
        assert!(result.mismatches.is_empty(), "{:?}", result.mismatches);
    }

    #[test]
    fn the_mismatches_carry_their_cell_family() {
        let mut twin = document();
        {
            let Some(id) = twin.pages[0].shapes.first().copied() else {
                panic!("the twin has a shape");
            };
            let Some(shape) = twin.shape_mut(id) else {
                panic!("the twin has the shape");
            };
            for cell in &mut shape.sheet.cells {
                match cell.name.as_str() {
                    "PinX" => cell.value = Some(CellValue::Number(9.0)),
                    "FillForegnd" => cell.value = Some(CellValue::Number(2.0)),
                    "LeftMargin" => cell.value = Some(CellValue::Number(9.0)),
                    _ => {}
                }
            }
            for section in &mut shape.sheet.sections {
                for row in &mut section.rows {
                    for cell in &mut row.cells {
                        if cell.name.as_str() == "X" {
                            cell.value = Some(CellValue::Number(9.0));
                        }
                    }
                }
            }
        }
        let extra = twin.add_shape(shape(2, 1.0, 2.0));
        twin.pages[0].shapes.push(extra);
        twin.pages[0].sheet.cells = vec![number("PageWidth", 12.0), number("PageHeight", 8.5)];

        let result = compare_documents(&document(), &twin);
        let family = |needle: &str| -> Family {
            result
                .mismatches
                .iter()
                .find(|mismatch| mismatch.message.contains(needle))
                .unwrap_or_else(|| panic!("a mismatch for {needle}"))
                .family
        };
        assert_eq!(family("PageWidth"), Family::Page);
        assert_eq!(family("only in the .vsdx twin"), Family::Join);
        assert_eq!(family("shape 1 / PinX"), Family::Transform);
        assert_eq!(family("shape 1 / FillForegnd"), Family::Fill);
        assert_eq!(family("shape 1 / LeftMargin"), Family::TextBlock);
        assert_eq!(family("geometry row 1 X"), Family::Geometry);
    }

    #[test]
    fn the_text_mismatch_carries_the_text_family() {
        let mut twin = document();
        let Some(id) = twin.pages[0].shapes.first().copied() else {
            panic!("the twin has a shape");
        };
        let Some(shape) = twin.shape_mut(id) else {
            panic!("the twin has the shape");
        };
        shape.text = Some(TextContent {
            parts: vec![TextPart::Text("two".to_owned())],
        });
        let result = compare_documents(&document(), &twin);
        assert_eq!(
            result.mismatches,
            [Mismatch {
                message: "page 1 / shape 1 / text / \"two\" / \"one\"".to_owned(),
                family: Family::Text,
                cause: "text".to_owned(),
            }]
        );
    }

    #[test]
    fn the_mismatches_name_their_cause() {
        let mut twin = document();
        let Some(id) = twin.pages[0].shapes.first().copied() else {
            panic!("the twin has a shape");
        };
        let Some(shape) = twin.shape_mut(id) else {
            panic!("the twin has the shape");
        };
        for cell in &mut shape.sheet.cells {
            if cell.name.as_str() == "PinX" {
                cell.value = Some(CellValue::Number(9.0));
            }
        }
        twin.pages[0].sheet.cells = vec![number("PageWidth", 12.0), number("PageHeight", 8.5)];
        let result = compare_documents(&document(), &twin);
        let causes: Vec<&str> = result
            .mismatches
            .iter()
            .map(|mismatch| mismatch.cause.as_str())
            .collect();
        assert_eq!(causes, ["PageWidth", "PinX"]);
    }

    #[test]
    fn the_counted_report_wraps_the_rich_one() {
        let (messages, compared) = compare_documents_counted(&document(), &scaled_twin());
        assert!(messages.is_empty(), "{messages:?}");
        assert!(compared > 0);
    }

    #[test]
    fn the_family_names_are_the_report_columns() {
        let names: Vec<&str> = Family::ALL.iter().map(|family| family.as_str()).collect();
        assert_eq!(
            names,
            [
                "page",
                "join",
                "transform",
                "line",
                "fill",
                "text_block",
                "geometry",
                "text"
            ]
        );
    }
}
