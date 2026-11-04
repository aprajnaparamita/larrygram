//! The connector storage round trip (task P7-36 item 5): everything a
//! file must carry for our glue to open in Visio.
//!
//! Documents our commands build — point glue, dynamic glue, free ends
//! and each style — are saved and reopened with the endpoint formulas,
//! triggers, values, style cells, geometry rows and `Connect` records
//! coming back exactly. Every corpus `.vsdx` file with `Connects` is
//! opened, saved unedited and reopened with every record, endpoint
//! formula and trigger unchanged. The corpus is fetched, never
//! committed (ADR 0005): the test skips with a note when
//! `corpus/vsdx/` is absent and fails instead when
//! `LARRYGRAM_REQUIRE_CORPUS=1` is set.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::path::{Path, PathBuf};

use larrygram_geom::{Point, Unit};
use larrygram_model::mutate::{self as model_mutate, CellWrite};
use larrygram_model::{
    Cell, CellName, CellValue, Connect, ConnectorEnd, Document, EndGlue, PageId, Preserved, Row,
    Section, SectionName, Shape, ShapeId, SheetRef,
};
use larrygram_vsdx::{Limits, Package, read_document, write_document};

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// A cell with a literal formula.
fn cell(name: &str, value: f64) -> Cell {
    Cell {
        name: CellName::new(name),
        formula: Some(model_mutate::literal(value)),
        value: Some(CellValue::Number(value)),
        unit: Unit::Inches,
        preserved: Preserved::new(),
    }
}

/// A 2 x 1 in box at `(x, y)` with two connection points (the top and
/// bottom edge midpoints).
fn box_shape(visio_id: i32, x: f64, y: f64) -> Shape {
    let mut shape = Shape::new(ShapeId { index: 0 }, visio_id);
    shape.sheet.cells = vec![
        cell("Width", 2.0),
        cell("Height", 1.0),
        cell("PinX", x),
        cell("PinY", y),
        cell("LocPinX", 1.0),
        cell("LocPinY", 0.5),
        cell("Angle", 0.0),
    ];
    let mut connection = Section::new(SectionName::from_static("Connection"));
    for (ix, (px, py)) in [(1.0, 1.0), (1.0, 0.0)].into_iter().enumerate() {
        let mut row = Row::new();
        row.row_type = Some("Connection".to_owned());
        row.ix = Some(i32::try_from(ix).unwrap_or(0));
        row.cells = vec![cell("X", px), cell("Y", py)];
        connection.rows.push(row);
    }
    shape.sheet.sections.push(connection);
    shape
}

/// A routable 1-D connector at `(begin, end)` (the ADR 0084 rule 1
/// structure): endpoint cells, the guard formulas of the pin and size
/// cells, `ObjType` 2 and `GlueType` 2.
fn connector_shape(visio_id: i32, begin: Point, end: Point) -> Shape {
    let text = |name: &str, formula: &str, value: f64| Cell {
        name: CellName::new(name),
        formula: Some(formula.to_owned()),
        value: Some(CellValue::Number(value)),
        unit: Unit::Inches,
        preserved: Preserved::new(),
    };
    let mut shape = Shape::new(ShapeId { index: 0 }, visio_id);
    shape.sheet.cells = vec![
        text("BeginX", "0", begin.x),
        text("BeginY", "0", begin.y),
        text("EndX", "0", end.x),
        text("EndY", "0", end.y),
        text("PinX", "GUARD((BeginX+EndX)/2)", (begin.x + end.x) / 2.0),
        text("PinY", "GUARD((BeginY+EndY)/2)", (begin.y + end.y) / 2.0),
        text("Width", "GUARD(EndX-BeginX)", end.x - begin.x),
        text("Height", "GUARD(0.2DL)", 0.2),
        text("LocPinX", "GUARD(Width*0.5)", (end.x - begin.x) / 2.0),
        text("LocPinY", "GUARD(Height*0.5)", 0.1),
        text("Angle", "GUARD(0DA)", 0.0),
        text("ObjType", "2", 2.0),
        text("GlueType", "2", 2.0),
    ];
    shape
}

/// Two boxes and one connector on one page, as the commands build
/// them.
fn fixture() -> (Document, PageId, ShapeId, ShapeId, ShapeId) {
    let mut doc = Document::new();
    let page = doc.add_page("Page-1").expect("page");
    let left = doc
        .insert_shape(page, box_shape(1, 2.0, 2.0), None)
        .expect("left");
    let right = doc
        .insert_shape(page, box_shape(2, 6.0, 2.0), None)
        .expect("right");
    let connector = doc
        .insert_shape(
            page,
            connector_shape(3, Point::new(3.0, 1.0), Point::new(5.0, 1.0)),
            None,
        )
        .expect("connector");
    (doc, page, left, right, connector)
}

/// The glue record the commands write for one end (ADR 0084 rule 2,
/// spec section 8).
fn glue_record(
    connector_visio: i32,
    target_visio: i32,
    end: ConnectorEnd,
    point: Option<i32>,
) -> Connect {
    let (from_cell, from_part) = match end {
        ConnectorEnd::Begin => ("BeginX", model_mutate::VIS_BEGIN),
        ConnectorEnd::End => ("EndX", model_mutate::VIS_END),
    };
    let (to_cell, to_part) = match point {
        Some(ix) => (
            format!("Connections.X{}", ix + 1),
            model_mutate::VIS_CONNECTION_POINT + ix,
        ),
        None => ("PinX".to_owned(), model_mutate::VIS_WHOLE_SHAPE),
    };
    Connect {
        from_sheet: connector_visio,
        from_cell: from_cell.to_owned(),
        from_part,
        to_sheet: target_visio,
        to_cell,
        to_part,
        ..Connect::default()
    }
}

/// Adds one record to the page's `Connects`.
fn add_record(doc: &mut Document, page: PageId, record: Connect) {
    if let Some(entry) = doc.pages.get_mut(page.index as usize) {
        entry.connects.push(record);
    }
}

// ---------------------------------------------------------------------------
// Comparison helpers
// ---------------------------------------------------------------------------

/// One sheet cell as `(formula, value)`.
type CellState = (Option<String>, Option<CellValue>);

fn own_cell(doc: &Document, shape: ShapeId, name: &str) -> CellState {
    doc.shape(shape)
        .and_then(|entity| {
            entity
                .sheet
                .cells
                .iter()
                .find(|cell| cell.name.as_str() == name)
        })
        .map(|cell| (cell.formula.clone(), cell.value.clone()))
        .unwrap_or((None, None))
}

fn page_cell(doc: &Document, page: PageId, name: &str) -> CellState {
    doc.page(page)
        .and_then(|entity| {
            entity
                .sheet
                .cells
                .iter()
                .find(|cell| cell.name.as_str() == name)
        })
        .map(|cell| (cell.formula.clone(), cell.value.clone()))
        .unwrap_or((None, None))
}

/// The six cells a glue round trip must keep (ADR 0084 rule 2).
fn endpoint_state(doc: &Document, shape: ShapeId) -> Vec<(&'static str, CellState)> {
    [
        "BeginX",
        "BeginY",
        "EndX",
        "EndY",
        "BegTrigger",
        "EndTrigger",
    ]
    .iter()
    .map(|name| (*name, own_cell(doc, shape, name)))
    .collect()
}

/// The connector's `Geometry` rows as their stored content: the row
/// type and index and every cell's name, formula and cached value (the
/// unit and the verbatim leftovers are the file's spelling choice, not
/// the row's content).
fn geometry_rows(doc: &Document, shape: ShapeId) -> Vec<RowState> {
    doc.shape(shape)
        .map(|entity| {
            entity
                .sheet
                .sections
                .iter()
                .filter(|section| section.name.as_str() == "Geometry")
                .flat_map(|section| section.rows.iter().map(row_state))
                .collect()
        })
        .unwrap_or_default()
}

/// One row's stored content (the exact comparison of the round trip).
type RowState = (
    Option<String>,
    Option<i32>,
    Vec<(String, Option<String>, Option<CellValue>)>,
);

fn row_state(row: &Row) -> RowState {
    (
        row.row_type.clone(),
        row.ix,
        row.cells
            .iter()
            .map(|cell| {
                (
                    cell.name.as_str().to_owned(),
                    cell.formula.clone(),
                    cell.value.clone(),
                )
            })
            .collect(),
    )
}

/// The page's `Connect` records in file order.
fn records(doc: &Document, page: PageId) -> Vec<Connect> {
    doc.page(page)
        .map(|entry| entry.connects.clone())
        .unwrap_or_default()
}

/// Saves the document and reopens it (the round trip every test
/// makes).
fn round_trip(doc: &Document) -> Document {
    let bytes = write_document(doc, &Package::new()).expect("save");
    read_document(&bytes, &Limits::DEFAULT).expect("reopen")
}

/// Finds a shape by its Visio id on the reopened page.
fn shape_by_id(doc: &Document, page: PageId, visio_id: i32) -> ShapeId {
    doc.shape_by_visio_id(page, visio_id)
        .expect("shape by visio id")
}

// ---------------------------------------------------------------------------
// The built documents
// ---------------------------------------------------------------------------

/// Point glue carries its formula, trigger, value and record over the
/// file round trip (task P7-36 item 5).
#[test]
fn point_glue_round_trips_its_formula_trigger_value_and_record() {
    let (mut doc, page, _left, _right, connector) = fixture();
    let begin = Point::new(3.0, 3.0);
    doc.write_connector_end(
        connector,
        ConnectorEnd::Begin,
        begin,
        EndGlue::Point { sheet: 1, ix: 0 },
    )
    .expect("point glue");
    doc.write_connector_end(
        connector,
        ConnectorEnd::End,
        Point::new(5.0, 1.0),
        EndGlue::Free,
    )
    .expect("free end");
    add_record(
        &mut doc,
        page,
        glue_record(3, 1, ConnectorEnd::Begin, Some(0)),
    );
    doc.write_connector_path(connector, &[begin, Point::new(5.0, 1.0)])
        .expect("path");

    let before_endpoints = endpoint_state(&doc, connector);
    let before_rows = geometry_rows(&doc, connector);
    let before_records = records(&doc, page);

    // The file carries the documented formula and trigger.
    assert_eq!(
        own_cell(&doc, connector, "BeginX"),
        (
            Some("PAR(PNT(Sheet.1!Connections.X1,Sheet.1!Connections.Y1))".to_owned()),
            Some(CellValue::Number(3.0))
        )
    );
    assert_eq!(
        own_cell(&doc, connector, "BegTrigger"),
        (
            Some("_XFTRIGGER(Sheet.1!EventXFMod)".to_owned()),
            Some(CellValue::Number(0.0))
        )
    );
    assert_eq!(
        own_cell(&doc, connector, "EndTrigger"),
        (
            Some("_XFTRIGGER(EventXFMod)".to_owned()),
            Some(CellValue::Number(0.0))
        )
    );

    let back = round_trip(&doc);
    let reopened = shape_by_id(&back, page, 3);
    assert_eq!(endpoint_state(&back, reopened), before_endpoints);
    assert_eq!(geometry_rows(&back, reopened), before_rows);
    assert_eq!(records(&back, page), before_records);
}

/// Dynamic glue carries its `_WALKGLUE` formula, trigger and record
/// (task P7-36 item 5).
#[test]
fn dynamic_glue_round_trips_its_walk_glue_and_record() {
    let (mut doc, page, _left, _right, connector) = fixture();
    doc.write_connector_end(
        connector,
        ConnectorEnd::Begin,
        Point::new(3.0, 1.0),
        EndGlue::Dynamic { sheet: 1 },
    )
    .expect("dynamic begin");
    doc.write_connector_end(
        connector,
        ConnectorEnd::End,
        Point::new(7.0, 2.0),
        EndGlue::Dynamic { sheet: 2 },
    )
    .expect("dynamic end");
    add_record(&mut doc, page, glue_record(3, 1, ConnectorEnd::Begin, None));
    add_record(&mut doc, page, glue_record(3, 2, ConnectorEnd::End, None));
    doc.write_connector_path(connector, &[Point::new(3.0, 1.0), Point::new(7.0, 2.0)])
        .expect("path");

    assert_eq!(
        own_cell(&doc, connector, "BeginX").0.as_deref(),
        Some("_WALKGLUE(BegTrigger,EndTrigger,WalkPreference)")
    );
    assert_eq!(
        own_cell(&doc, connector, "EndX").0.as_deref(),
        Some("_WALKGLUE(EndTrigger,BegTrigger,WalkPreference)")
    );

    let before_endpoints = endpoint_state(&doc, connector);
    let before_rows = geometry_rows(&doc, connector);
    let before_records = records(&doc, page);
    let back = round_trip(&doc);
    let reopened = shape_by_id(&back, page, 3);
    assert_eq!(endpoint_state(&back, reopened), before_endpoints);
    assert_eq!(geometry_rows(&back, reopened), before_rows);
    assert_eq!(records(&back, page), before_records);
}

/// Free ends keep their literal points and their own trigger, with no
/// record (task P7-36 item 5).
#[test]
fn free_ends_keep_their_literal_points_and_their_own_trigger() {
    let (mut doc, page, _left, _right, connector) = fixture();
    doc.write_connector_end(
        connector,
        ConnectorEnd::Begin,
        Point::new(3.0, 1.0),
        EndGlue::Free,
    )
    .expect("free begin");
    doc.write_connector_end(
        connector,
        ConnectorEnd::End,
        Point::new(5.0, 4.0),
        EndGlue::Free,
    )
    .expect("free end");
    doc.write_connector_path(connector, &[Point::new(3.0, 1.0), Point::new(5.0, 4.0)])
        .expect("path");

    assert_eq!(own_cell(&doc, connector, "BeginX").0, None);
    assert_eq!(
        own_cell(&doc, connector, "BeginX").1,
        Some(CellValue::Number(3.0))
    );
    assert_eq!(
        own_cell(&doc, connector, "EndTrigger").0.as_deref(),
        Some("_XFTRIGGER(EventXFMod)")
    );
    assert!(records(&doc, page).is_empty());

    let before_endpoints = endpoint_state(&doc, connector);
    let before_rows = geometry_rows(&doc, connector);
    let back = round_trip(&doc);
    let reopened = shape_by_id(&back, page, 3);
    assert_eq!(endpoint_state(&back, reopened), before_endpoints);
    assert_eq!(geometry_rows(&back, reopened), before_rows);
    assert!(records(&back, page).is_empty());
}

/// Each style carries its cells and its rows (task P7-36 item 5, the
/// ADR 0084 rule 8 table).
#[test]
fn each_style_carries_its_cells_and_rows() {
    // (style cells, the path write): right angle, straight, curved.
    for (name, route, ext, curved) in [
        ("right angle", 1.0, 1.0, false),
        ("straight lines", 2.0, 1.0, false),
        ("curved lines", 1.0, 2.0, true),
    ] {
        let (mut doc, page, _left, _right, connector) = fixture();
        for (cell_name, value) in [("ShapeRouteStyle", route), ("ConLineRouteExt", ext)] {
            doc.write_cell(
                SheetRef::Shape(connector),
                &CellName::new(cell_name),
                &CellWrite::new(model_mutate::literal(value), CellValue::Number(value)),
            )
            .expect("style cell");
        }
        let points = [
            Point::new(3.0, 1.0),
            Point::new(4.0, 3.0),
            Point::new(5.0, 1.0),
        ];
        if curved {
            doc.write_connector_curve(connector, &points)
                .expect("curve");
        } else {
            doc.write_connector_path(connector, &points[..2])
                .expect("path");
        }

        let before_style = (
            own_cell(&doc, connector, "ShapeRouteStyle"),
            own_cell(&doc, connector, "ConLineRouteExt"),
        );
        let before_rows = geometry_rows(&doc, connector);
        assert_eq!(
            before_style,
            (
                (
                    Some(model_mutate::literal(route)),
                    Some(CellValue::Number(route))
                ),
                (
                    Some(model_mutate::literal(ext)),
                    Some(CellValue::Number(ext))
                ),
            ),
            "{name} cells"
        );
        if curved {
            assert_eq!(before_rows.len(), 2);
            assert_eq!(before_rows[1].0.as_deref(), Some("NURBSTo"));
        }

        let back = round_trip(&doc);
        let reopened = shape_by_id(&back, page, 3);
        assert_eq!(
            (
                own_cell(&back, reopened, "ShapeRouteStyle"),
                own_cell(&back, reopened, "ConLineRouteExt"),
            ),
            before_style,
            "{name} style cells"
        );
        assert_eq!(geometry_rows(&back, reopened), before_rows, "{name} rows");
    }
}

/// The page default cells carry over (task P7-36 item 5).
#[test]
fn the_page_default_cells_survive_the_round_trip() {
    let (mut doc, page, ..) = fixture();
    for (cell_name, value) in [("RouteStyle", 1.0), ("LineRouteExt", 2.0)] {
        doc.write_cell(
            SheetRef::Page(page),
            &CellName::new(cell_name),
            &CellWrite::new(model_mutate::literal(value), CellValue::Number(value)),
        )
        .expect("page cell");
    }
    let before = (
        page_cell(&doc, page, "RouteStyle"),
        page_cell(&doc, page, "LineRouteExt"),
    );
    let back = round_trip(&doc);
    assert_eq!(
        (
            page_cell(&back, page, "RouteStyle"),
            page_cell(&back, page, "LineRouteExt"),
        ),
        before
    );
}

// ---------------------------------------------------------------------------
// The corpus
// ---------------------------------------------------------------------------

/// The corpus directory with the real `.vsdx` packages.
fn corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("corpus")
        .join("vsdx")
}

/// The `.vsdx` files of the corpus, sorted for a stable order.
fn corpus_files() -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = match std::fs::read_dir(corpus_dir()) {
        Ok(entries) => entries
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "vsdx"))
            .collect(),
        Err(_) => Vec::new(),
    };
    files.sort();
    files
}

/// Returns `true` when no corpus is available and the run may skip.
fn corpus_missing(files: &[PathBuf]) -> bool {
    if files.is_empty() {
        if std::env::var("LARRYGRAM_REQUIRE_CORPUS").is_ok_and(|value| value == "1") {
            panic!("corpus/vsdx is absent and LARRYGRAM_REQUIRE_CORPUS=1");
        }
        eprintln!("skipping: corpus/vsdx is absent; run `cargo xtask corpus`");
    }
    files.is_empty()
}

/// Every shape's endpoint state by page position and Visio id, and
/// every page's records.
type GlueState = (
    std::collections::BTreeMap<(usize, i32), Vec<(&'static str, CellState)>>,
    Vec<Vec<Connect>>,
);

/// Every shape's endpoint formulas, trigger formulas and cached values
/// (by page position and Visio id), and every page's records: what a
/// round trip must not change (task P7-36 item 5).
fn glue_state(doc: &Document) -> GlueState {
    use std::collections::BTreeMap;
    let mut endpoints: BTreeMap<(usize, i32), Vec<(&'static str, CellState)>> = BTreeMap::new();
    for (index, page) in doc.pages.iter().enumerate() {
        let mut stack: Vec<ShapeId> = page.shapes.clone();
        while let Some(shape) = stack.pop() {
            if let Some(entity) = doc.shape(shape) {
                stack.extend(entity.children.iter().copied());
                endpoints.insert((index, entity.visio_id), endpoint_state(doc, shape));
            }
        }
    }
    let records = doc.pages.iter().map(|page| page.connects.clone()).collect();
    (endpoints, records)
}

/// Whether any page carries a `Connect` record.
fn has_connects(doc: &Document) -> bool {
    doc.pages.iter().any(|page| !page.connects.is_empty())
}

/// Every corpus `.vsdx` file with `Connects` opens, saves unedited and
/// reopens with every record, endpoint formula and trigger unchanged
/// (task P7-36 item 5). The two files the corpus carries today are
/// pinned below; more may join and are covered the same way.
#[test]
fn every_corpus_file_with_connects_keeps_its_records_and_formulas() {
    let files = corpus_files();
    if corpus_missing(&files) {
        return;
    }
    let mut checked: Vec<String> = Vec::new();
    for path in &files {
        let bytes = std::fs::read(path).expect("read corpus file");
        let document = read_document(&bytes, &Limits::DEFAULT).expect("open corpus file");
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let before = glue_state(&document);
        let saved = write_document(&document, &Package::new()).expect("save unedited");
        let back = read_document(&saved, &Limits::DEFAULT).expect("reopen");
        assert_eq!(
            glue_state(&back),
            before,
            "{name} changed in the round trip"
        );
        if has_connects(&document) {
            checked.push(name);
        }
    }
    // The corpus files with `Connects` today (task P7-36 item 5).
    for expected in ["testfile3.vsdx", "tdf136564-WhiteTextBackground.vsdx"] {
        assert!(
            checked.iter().any(|name| name == expected),
            "{expected} must carry Connects (found: {checked:?})"
        );
    }
}
