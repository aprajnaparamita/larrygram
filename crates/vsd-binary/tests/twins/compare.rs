// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

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
//! after [`CellReader`] resolves them (the palette and the theme),
//! which is what the renderer sees. The text compares after trailing
//! whitespace is dropped (ADR 0072's rule; the Unicode NFC the ADR
//! also names is the writer's and the reader's concern).

use larrygram_model::{CellName, Document, SheetRef, TextPart};

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

/// The geometry row cells the renderer reads, compared as lengths.
const GEOMETRY_CELLS: &[&str] = &["X", "Y", "A", "B", "C", "D"];

/// Compares one import against its twin: one message per difference,
/// empty when the two documents agree on everything compared.
pub fn compare_documents(our: &Document, twin: &Document) -> Vec<String> {
    let mut out = Vec::new();
    if our.pages.len() != twin.pages.len() {
        out.push(format!(
            "document / page count / {} / {}",
            our.pages.len(),
            twin.pages.len()
        ));
    }
    for (index, (a, b)) in our.pages.iter().zip(twin.pages.iter()).enumerate() {
        let place = format!("page {}", index + 1);
        if a.name != b.name {
            out.push(format!("{place} / name / {:?} / {:?}", b.name, a.name));
        }
        let sheets = (our, twin);
        let pair = (page(index), page(index));
        compare_cell(
            &mut out,
            &place,
            "PageWidth",
            CellKind::Length,
            sheets,
            pair,
        );
        compare_cell(
            &mut out,
            &place,
            "PageHeight",
            CellKind::Length,
            sheets,
            pair,
        );
        compare_shapes(&mut out, &place, our, twin, index);
    }
    out
}

/// The sheet reference of one page.
fn page(index: usize) -> SheetRef {
    SheetRef::Page(larrygram_model::PageId {
        index: u32::try_from(index).unwrap_or(u32::MAX),
    })
}

/// Compares the shapes of one page, joined by Visio `ID`.
fn compare_shapes(
    out: &mut Vec<String>,
    place: &str,
    our: &Document,
    twin: &Document,
    page_index: usize,
) {
    let Some(a_page) = our.pages.get(page_index) else {
        return;
    };
    let Some(b_page) = twin.pages.get(page_index) else {
        return;
    };
    let mut ids: Vec<i32> = Vec::new();
    collect_ids(our, &a_page.shapes, &mut ids);
    let mut twin_ids: Vec<i32> = Vec::new();
    collect_ids(twin, &b_page.shapes, &mut twin_ids);
    for id in &ids {
        if !twin_ids.contains(id) {
            out.push(format!("{place} / shape {id}: only in the .vsd import"));
        }
    }
    for id in &twin_ids {
        if !ids.contains(id) {
            out.push(format!("{place} / shape {id}: only in the .vsdx twin"));
        }
    }
    for id in &ids {
        let Some(twin_id) = twin_ids.contains(id).then_some(*id) else {
            continue;
        };
        let (Some(a), Some(b)) = (
            our.shape_by_visio_id(a_page.id, *id),
            twin.shape_by_visio_id(b_page.id, twin_id),
        ) else {
            continue;
        };
        let shape_place = format!("{place} / shape {id}");
        let (a_sheet, b_sheet) = (SheetRef::Shape(a), SheetRef::Shape(b));
        let sheets = (our, twin);
        let pair = (a_sheet, b_sheet);
        for &name in LENGTH_CELLS {
            compare_cell(out, &shape_place, name, CellKind::Length, sheets, pair);
        }
        for &name in ANGLE_CELLS {
            compare_cell(out, &shape_place, name, CellKind::Angle, sheets, pair);
        }
        for &name in NUMBER_CELLS {
            compare_cell(out, &shape_place, name, CellKind::Plain, sheets, pair);
        }
        for &name in COLOR_CELLS {
            compare_cell(out, &shape_place, name, CellKind::Color, sheets, pair);
        }
        for &name in BOOL_CELLS {
            compare_cell(out, &shape_place, name, CellKind::Flag, sheets, pair);
        }

        compare_geometry(out, &shape_place, our, twin, a_sheet, b_sheet);
        if let (Some(x), Some(y)) = (text_of(our, a), text_of(twin, b))
            && x != y
        {
            out.push(format!("{shape_place} / text / {y:?} / {x:?}"));
        }
    }
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

/// Compares one cell of one shape or page, resolved through the
/// [`CellReader`] (the value the renderer reads).
fn compare_cell(
    out: &mut Vec<String>,
    place: &str,
    name: &str,
    kind: CellKind,
    (our, twin): (&Document, &Document),
    (a, b): (SheetRef, SheetRef),
) {
    let name_ref = CellName::new(name);
    let mut left = larrygram_model::CellReader::new(our);
    let mut right = larrygram_model::CellReader::new(twin);
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
            let delta = match kind {
                CellKind::Angle => (x - y).to_degrees().abs(),
                _ => (x - y).abs(),
            };
            if delta > tolerance {
                out.push(format!("{place} / {name} / {y:.4} / {x:.4}"));
            }
        }
        CellKind::Color => {
            let (Ok(x), Ok(y)) = (left.color(a, &name_ref), right.color(b, &name_ref)) else {
                return;
            };
            if x != y {
                out.push(format!(
                    "{place} / {name} / #{:02x}{:02x}{:02x} / #{:02x}{:02x}{:02x}",
                    y.r, y.g, y.b, x.r, x.g, x.b
                ));
            }
        }
        CellKind::Flag => {
            let (Ok(x), Ok(y)) = (left.boolean(a, &name_ref), right.boolean(b, &name_ref)) else {
                return;
            };
            if x != y {
                out.push(format!("{place} / {name} / {y} / {x}"));
            }
        }
    }
}

/// Compares the geometry rows of one shape: the row counts and the
/// row cells the renderer reads.
fn compare_geometry(
    out: &mut Vec<String>,
    place: &str,
    our: &Document,
    twin: &Document,
    a: SheetRef,
    b: SheetRef,
) {
    use larrygram_model::RowTarget;
    let a_rows = geometry_rows(our, a);
    let b_rows = geometry_rows(twin, b);
    if a_rows != b_rows {
        out.push(format!("{place} / geometry rows / {} / {}", b_rows, a_rows));
        return;
    }
    let mut left = larrygram_model::CellReader::new(our);
    let mut right = larrygram_model::CellReader::new(twin);
    for ix in 1..=a_rows {
        let row = RowTarget::by_ix(i32::try_from(ix).unwrap_or(i32::MAX));
        for &name in GEOMETRY_CELLS {
            let name_ref = CellName::new(name);
            let (Ok(x), Ok(y)) = (
                left.row_number(a, "Geometry", row, &name_ref),
                right.row_number(b, "Geometry", row, &name_ref),
            ) else {
                continue;
            };
            if x.is_finite() && y.is_finite() && (x - y).abs() > LENGTH_TOLERANCE {
                out.push(format!(
                    "{place} / geometry row {ix} {name} / {y:.4} / {x:.4}"
                ));
            }
        }
    }
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
