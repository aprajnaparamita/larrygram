//! The 1-D connector: the master's cells, `is_connector` and the glue
//! formulas of ADR 0084 rules 1 and 2 (task P7-31).
//!
//! The tests pin the master as our own 1-D shape, the four endpoint
//! cells and their formulas as Visio's files carry them
//! (`docs/ui/connectors.md` section 8), and what
//! `Document::write_connector_path` keeps true (including a negative
//! `Width` for a right-to-left connector).

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use larrygram_geom::Point;
use larrygram_model::{
    Cell, CellName, CellValue, Connect, Document, GeometryRow, PageId, Preserved, Shape, ShapeId,
    stencils,
};
use larrygram_model::{ConnectorEnd, EndGlue};

/// The "Connector" master definition (Basic Flowchart, the last master).
fn connector_def() -> stencils::MasterDef {
    stencils::basic_flowchart()
        .masters
        .into_iter()
        .find(|master| master.name_u == "Connector")
        .expect("the Connector master")
}

/// A document with one page and one Connector instance on it.
fn with_connector() -> (Document, ShapeId) {
    let mut doc = Document::new();
    let connector = stencils::instantiate(&mut doc, &connector_def());
    (doc, connector)
}

/// One 2 x 1 in box at the origin (a shape that is not a connector).
fn plain_box(visio_id: i32) -> Shape {
    let mut shape = Shape::new(ShapeId { index: 0 }, visio_id);
    shape.sheet.cells = vec![
        value_cell("Width", 2.0),
        value_cell("Height", 1.0),
        value_cell("PinX", 1.0),
        value_cell("PinY", 0.5),
    ];
    shape
}

/// A cell with a literal value and no formula.
fn value_cell(name: &str, value: f64) -> Cell {
    Cell {
        name: CellName::new(name),
        formula: None,
        value: Some(CellValue::Number(value)),
        unit: larrygram_geom::Unit::Inches,
        preserved: Preserved::new(),
    }
}

/// The shape's own cell, or `None`.
fn own(doc: &Document, shape: ShapeId, name: &str) -> Option<Cell> {
    doc.shape(shape)?
        .sheet
        .cells
        .iter()
        .find(|cell| cell.name.as_str() == name)
        .cloned()
}

/// The formula of the shape's own cell.
fn formula(doc: &Document, shape: ShapeId, name: &str) -> Option<String> {
    own(doc, shape, name).and_then(|cell| cell.formula)
}

/// The numeric value of the shape's own cell.
fn number(doc: &Document, shape: ShapeId, name: &str) -> f64 {
    match own(doc, shape, name).and_then(|cell| cell.value) {
        Some(CellValue::Number(value)) => value,
        other => panic!("{name}: expected a number, got {other:?}"),
    }
}

/// Whether any own cell formula contains `needle`.
fn any_formula_contains(doc: &Document, shape: ShapeId, needle: &str) -> bool {
    doc.shape(shape)
        .map(|entity| {
            entity
                .sheet
                .cells
                .iter()
                .any(|cell| {
                    cell.formula
                        .as_deref()
                        .is_some_and(|formula| formula.contains(needle))
                })
        })
        .unwrap_or(false)
}

#[test]
fn the_connector_master_is_a_1d_shape() {
    let document = stencils::stencil_document();
    let master = document
        .masters
        .iter()
        .find(|master| master.name_u == "Connector")
        .expect("the Connector master");
    let shape = document.shape(master.shapes[0]).expect("master shape");
    let cells = &shape.sheet.cells;
    let find = |name: &str| {
        cells
            .iter()
            .find(|cell| cell.name.as_str() == name)
            .unwrap_or_else(|| panic!("{name} missing"))
    };
    // The endpoint cells: the master's own straight ends.
    assert_eq!(find("BeginX").value, Some(CellValue::Number(0.0)));
    assert_eq!(find("BeginY").value, Some(CellValue::Number(0.0)));
    assert_eq!(find("EndX").value, Some(CellValue::Number(1.2)));
    assert_eq!(find("EndY").value, Some(CellValue::Number(0.0)));
    // The pin, size and lock formulas (ADR 0084 rule 1).
    assert_eq!(
        find("PinX").formula.as_deref(),
        Some("GUARD((BeginX+EndX)/2)")
    );
    assert_eq!(find("PinY").formula.as_deref(), Some("GUARD((BeginY+EndY)/2)"));
    assert_eq!(find("Width").formula.as_deref(), Some("GUARD(EndX-BeginX)"));
    assert_eq!(find("Height").formula.as_deref(), Some("GUARD(0.2DL)"));
    assert_eq!(find("LocPinX").formula.as_deref(), Some("GUARD(Width*0.5)"));
    assert_eq!(find("LocPinY").formula.as_deref(), Some("GUARD(Height*0.5)"));
    assert_eq!(find("Angle").formula.as_deref(), Some("GUARD(0DA)"));
    // The kinds and the right-angle defaults.
    assert_eq!(find("ObjType").value, Some(CellValue::Number(2.0)));
    assert_eq!(find("GlueType").value, Some(CellValue::Number(2.0)));
    assert_eq!(find("ShapeRouteStyle").value, Some(CellValue::Number(1.0)));
    assert_eq!(find("ConLineRouteExt").value, Some(CellValue::Number(1.0)));
    // The two triggers, unglued.
    assert_eq!(
        find("BegTrigger").formula.as_deref(),
        Some("_XFTRIGGER(EventXFMod)")
    );
    assert_eq!(
        find("EndTrigger").formula.as_deref(),
        Some("_XFTRIGGER(EventXFMod)")
    );
    // No Connection section: a connector is not a glue target.
    assert!(
        !shape
            .sheet
            .sections
            .iter()
            .any(|section| section.name.as_str() == "Connection"),
        "no Connection section"
    );
    // One straight geometry path.
    let geometry = document
        .shape_geometry(master.shapes[0])
        .expect("geometry");
    assert_eq!(geometry.paths.len(), 1);
    assert_eq!(geometry.paths[0].rows.len(), 2, "MoveTo and LineTo");
}

#[test]
fn is_connector_tells_a_1d_shape_from_the_rest() {
    let document = stencils::stencil_document();
    let master = document
        .masters
        .iter()
        .find(|master| master.name_u == "Connector")
        .expect("the Connector master");
    let master_shape = master.shapes[0];
    assert!(document.is_connector(master_shape), "the master's shape");

    let (mut doc, instance) = with_connector();
    assert!(doc.is_connector(instance), "an instance inherits the cells");

    let ordinary = doc.insert_shape(PageId { index: 0 }, plain_box(2), None).expect("insert");
    assert!(!doc.is_connector(ordinary), "an ordinary shape");

    // A legacy record-only connector: a `Connects` record names it as a
    // connector end, but it has no endpoint cells (a file saved by an
    // earlier Larrygram). It is not a connector and nothing migrates it.
    let legacy = doc.insert_shape(PageId { index: 0 }, plain_box(3), None).expect("insert");
    let mut record = Connect::new();
    record.from_sheet = 3;
    record.from_cell = "BeginX".to_owned();
    record.from_part = 9;
    record.to_sheet = 2;
    record.to_cell = "PinX".to_owned();
    record.to_part = 3;
    doc.add_connect(PageId { index: 0 }, record).expect("record");
    assert!(!doc.is_connector(legacy), "a record-only connector");
    assert!(!doc.is_connector(ShapeId { index: 999 }), "a stale id");
}

#[test]
fn the_rule_2_rows_write_and_read_back() {
    let (mut doc, connector) = with_connector();

    // Point glue to row 4 of sheet 5: the row number in the formula is
    // one-based (the row is `Connections.X5`), `ToPart` 100 + 4.
    doc.write_connector_end(
        connector,
        ConnectorEnd::Begin,
        Point::new(2.5, 3.5),
        EndGlue::Point { sheet: 5, ix: 4 },
    )
    .expect("point glue");
    let expected = "PAR(PNT(Sheet.5!Connections.X5,Sheet.5!Connections.Y5))";
    assert_eq!(formula(&doc, connector, "BeginX"), Some(expected.to_owned()));
    assert_eq!(formula(&doc, connector, "BeginY"), Some(expected.to_owned()));
    assert_eq!(number(&doc, connector, "BeginX"), 2.5);
    assert_eq!(number(&doc, connector, "BeginY"), 3.5);
    assert_eq!(
        formula(&doc, connector, "BegTrigger"),
        Some("_XFTRIGGER(Sheet.5!EventXFMod)".to_owned())
    );

    // Dynamic glue to sheet 7 on the End end.
    doc.write_connector_end(
        connector,
        ConnectorEnd::End,
        Point::new(1.0, 2.0),
        EndGlue::Dynamic { sheet: 7 },
    )
    .expect("dynamic glue");
    assert_eq!(
        formula(&doc, connector, "EndX"),
        Some("_WALKGLUE(EndTrigger,BegTrigger,WalkPreference)".to_owned())
    );
    assert_eq!(
        formula(&doc, connector, "EndY"),
        Some("_WALKGLUE(EndTrigger,BegTrigger,WalkPreference)".to_owned())
    );
    assert_eq!(number(&doc, connector, "EndX"), 1.0);
    assert_eq!(
        formula(&doc, connector, "EndTrigger"),
        Some("_XFTRIGGER(Sheet.7!EventXFMod)".to_owned())
    );

    // Unglued: the literal point, no formula, the trigger names the
    // end's own sheet.
    doc.write_connector_end(
        connector,
        ConnectorEnd::Begin,
        Point::new(9.0, 8.0),
        EndGlue::Free,
    )
    .expect("unglue");
    assert_eq!(formula(&doc, connector, "BeginX"), None);
    assert_eq!(formula(&doc, connector, "BeginY"), None);
    assert_eq!(number(&doc, connector, "BeginX"), 9.0);
    assert_eq!(
        formula(&doc, connector, "BegTrigger"),
        Some("_XFTRIGGER(EventXFMod)".to_owned())
    );
}

#[test]
fn a_reglue_replaces_a_visio_formula() {
    let (mut doc, connector) = with_connector();
    doc.write_connector_end(
        connector,
        ConnectorEnd::Begin,
        Point::new(1.0, 1.0),
        EndGlue::Point { sheet: 1, ix: 0 },
    )
    .expect("first glue");
    assert!(any_formula_contains(&doc, connector, "Sheet.1"));

    // Re-glue to sheet 2: no stale `Sheet.1` reference survives.
    doc.write_connector_end(
        connector,
        ConnectorEnd::Begin,
        Point::new(2.0, 2.0),
        EndGlue::Dynamic { sheet: 2 },
    )
    .expect("re-glue");
    assert_eq!(
        formula(&doc, connector, "BeginX"),
        Some("_WALKGLUE(BegTrigger,EndTrigger,WalkPreference)".to_owned())
    );
    assert_eq!(
        formula(&doc, connector, "BegTrigger"),
        Some("_XFTRIGGER(Sheet.2!EventXFMod)".to_owned())
    );
    assert!(!any_formula_contains(&doc, connector, "Sheet.1"), "no stale");
}

#[test]
fn the_cells_after_a_path_write_follow_the_formulas() {
    let (mut doc, connector) = with_connector();
    doc.write_connector_path(
        connector,
        &[Point::new(2.0, 3.0), Point::new(5.0, 4.5)],
    )
    .expect("path");
    assert_eq!(number(&doc, connector, "BeginX"), 2.0);
    assert_eq!(number(&doc, connector, "BeginY"), 3.0);
    assert_eq!(number(&doc, connector, "EndX"), 5.0);
    assert_eq!(number(&doc, connector, "EndY"), 4.5);
    assert_eq!(
        formula(&doc, connector, "PinX"),
        Some("GUARD((BeginX+EndX)/2)".to_owned())
    );
    assert!((number(&doc, connector, "PinX") - 3.5).abs() < 1e-12);
    assert!((number(&doc, connector, "PinY") - 3.75).abs() < 1e-12);
    assert_eq!(
        formula(&doc, connector, "Width"),
        Some("GUARD(EndX-BeginX)".to_owned())
    );
    assert!((number(&doc, connector, "Width") - 3.0).abs() < 1e-12);
    assert_eq!(
        formula(&doc, connector, "Height"),
        Some("GUARD(0.2DL)".to_owned())
    );
    assert!((number(&doc, connector, "Height") - 0.2).abs() < 1e-12);
    assert_eq!(
        formula(&doc, connector, "LocPinX"),
        Some("GUARD(Width*0.5)".to_owned())
    );
    assert!((number(&doc, connector, "LocPinX") - 1.5).abs() < 1e-12);
    assert!((number(&doc, connector, "LocPinY") - 0.1).abs() < 1e-12);
    // The geometry rows are the box's local coordinates.
    let geometry = doc.shape_geometry(connector).expect("geometry");
    let rows = &geometry.paths[0].rows;
    match rows[0] {
        GeometryRow::MoveTo { x, y } => {
            assert!(x.abs() < 1e-12, "{x}");
            assert!((y - (-0.65)).abs() < 1e-12, "{y}");
        }
        ref other => panic!("expected MoveTo, got {other:?}"),
    }
    match rows[1] {
        GeometryRow::LineTo { x, y } => {
            assert!((x - 3.0).abs() < 1e-12, "{x}");
            assert!((y - 0.85).abs() < 1e-12, "{y}");
        }
        ref other => panic!("expected LineTo, got {other:?}"),
    }
}

#[test]
fn a_right_to_left_path_keeps_its_pixels() {
    let (mut doc, connector) = with_connector();
    let path = [Point::new(4.0, 3.0), Point::new(1.0, 3.0)];
    doc.write_connector_path(connector, &path).expect("path");
    // Negative `Width`: the end lies left of the begin.
    assert!((number(&doc, connector, "Width") - (-3.0)).abs() < 1e-12);
    assert_eq!(number(&doc, connector, "BeginX"), 4.0);
    assert_eq!(number(&doc, connector, "EndX"), 1.0);
    assert!((number(&doc, connector, "PinX") - 2.5).abs() < 1e-12);

    // The written rows map back to the same page pixels.
    let mapped = map_back(&doc, connector);
    assert_eq!(mapped.len(), 2, "{mapped:?}");
    assert!((mapped[0].x - 4.0).abs() < 1e-12 && (mapped[0].y - 3.0).abs() < 1e-12, "{mapped:?}");
    assert!((mapped[1].x - 1.0).abs() < 1e-12 && (mapped[1].y - 3.0).abs() < 1e-12, "{mapped:?}");

    // The same connector drawn left to right covers the same segment.
    let (mut doc2, back) = with_connector();
    doc2.write_connector_path(back, &path.map(|p| Point::new(5.0 - p.x, p.y)))
        .expect("path");
    assert!((number(&doc2, back, "Width") - 3.0).abs() < 1e-12);
    let mirrored = map_back(&doc2, back);
    assert!((mirrored[0].x - 1.0).abs() < 1e-12, "{mirrored:?}");
    assert!((mirrored[1].x - 4.0).abs() < 1e-12, "{mirrored:?}");
}

/// The written geometry rows mapped back through the shape's transform
/// (the render's rule: `Pin + local - LocPin`, no rotation or flip).
fn map_back(doc: &Document, connector: ShapeId) -> Vec<Point> {
    let pin = Point::new(number(doc, connector, "PinX"), number(doc, connector, "PinY"));
    let loc = Point::new(
        number(doc, connector, "LocPinX"),
        number(doc, connector, "LocPinY"),
    );
    let geometry = doc.shape_geometry(connector).expect("geometry");
    geometry.paths[0]
        .rows
        .iter()
        .map(|row| match row {
            GeometryRow::MoveTo { x, y } | GeometryRow::LineTo { x, y } => {
                Point::new(pin.x + *x - loc.x, pin.y + *y - loc.y)
            }
            other => panic!("unexpected row {other:?}"),
        })
        .collect()
}

#[test]
fn a_glued_end_keeps_its_formula_across_a_path_write() {
    let (mut doc, connector) = with_connector();
    doc.write_connector_end(
        connector,
        ConnectorEnd::Begin,
        Point::new(2.0, 3.0),
        EndGlue::Point { sheet: 2, ix: 0 },
    )
    .expect("glue");
    doc.write_connector_path(
        connector,
        &[Point::new(2.0, 3.0), Point::new(5.0, 4.5)],
    )
    .expect("path");
    // The glue formula survives the path write; only its cached `V`
    // follows the routed point.
    assert_eq!(
        formula(&doc, connector, "BeginX"),
        Some("PAR(PNT(Sheet.2!Connections.X1,Sheet.2!Connections.Y1))".to_owned())
    );
    assert_eq!(number(&doc, connector, "BeginX"), 2.0);
    // The free end is its literal point.
    assert_eq!(formula(&doc, connector, "EndX"), None);
    assert_eq!(number(&doc, connector, "EndX"), 5.0);
}

#[test]
fn a_legacy_connector_gets_geometry_rows_only() {
    let mut doc = Document::new();
    let page = doc.add_page("Page-1").expect("page");
    let legacy = doc
        .insert_shape(page, plain_box(4), None)
        .expect("insert");
    doc.write_connector_path(legacy, &[Point::new(0.0, 0.0), Point::new(1.0, 1.0)])
        .expect("path");
    assert!(own(&doc, legacy, "BeginX").is_none(), "no endpoint cells");
    assert!(own(&doc, legacy, "EndX").is_none());
    assert!(
        formula(&doc, legacy, "Width").is_none(),
        "the 2-D cells are untouched"
    );
    let geometry = doc.shape_geometry(legacy).expect("geometry");
    assert_eq!(geometry.paths[0].rows.len(), 2);
}
