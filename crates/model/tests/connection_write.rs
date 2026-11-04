//! The connection-point write API (task P7-34 item 5): add, move,
//! delete and set type over the shape's own `Connection` rows, the row
//! renumbering on delete, and the copy-in of an instance's inherited
//! rows on the first edit.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use larrygram_geom::Point;
use larrygram_model::{
    Cell, CellName, CellValue, Document, Master, MasterId, Page, PageId, Preserved, Row, Section,
    SectionName, Shape, ShapeId,
};

fn cell(name: &str, value: f64) -> Cell {
    Cell {
        name: CellName::new(name),
        formula: Some(format!("{value}")),
        value: Some(CellValue::Number(value)),
        unit: larrygram_geom::Unit::Inches,
        preserved: Preserved::new(),
    }
}

fn row(x: f64, y: f64) -> Row {
    let mut row = Row::new();
    row.row_type = Some("Connection".to_owned());
    row.cells = vec![cell("X", x), cell("Y", y)];
    row
}

fn connection_section(rows: Vec<Row>) -> Section {
    let mut section = Section::new(SectionName::from_static("Connection"));
    section.rows = rows;
    section
}

fn shape(cells: &[(&str, f64)], sections: Vec<Section>) -> Shape {
    let mut shape = Shape::new(ShapeId { index: 0 }, 1);
    shape.sheet.cells = cells
        .iter()
        .map(|(name, value)| cell(name, *value))
        .collect();
    shape.sheet.sections = sections;
    shape
}

fn on_a_page(document: &mut Document, shape: Shape) -> ShapeId {
    let id = document.add_shape(shape);
    let mut page = Page::new(PageId { index: 0 }, 1);
    page.shapes.push(id);
    document.pages.push(page);
    id
}

/// The number of one row cell, or `None`.
fn row_number(document: &Document, shape: ShapeId, ix: usize, name: &str) -> Option<f64> {
    let entity = document.shape(shape)?;
    let section = entity
        .sheet
        .sections
        .iter()
        .find(|section| section.name.as_str() == "Connection")?;
    let row = section.rows.get(ix)?;
    row.cells
        .iter()
        .find(|cell| cell.name.as_str() == name)
        .and_then(|cell| match &cell.value {
            Some(CellValue::Number(value)) => Some(*value),
            _ => None,
        })
}

/// The `IX` values of the shape's own `Connection` rows.
fn row_indices(document: &Document, shape: ShapeId) -> Vec<Option<i32>> {
    let entity = document.shape(shape).expect("shape");
    let mut out = Vec::new();
    for section in &entity.sheet.sections {
        if section.name.as_str() == "Connection" {
            out.extend(section.rows.iter().map(|row| row.ix));
        }
    }
    out
}

/// The row count of the shape's own `Connection` section.
fn own_row_count(document: &Document, shape: ShapeId) -> usize {
    document
        .shape(shape)
        .map(|entity| {
            entity
                .sheet
                .sections
                .iter()
                .filter(|section| section.name.as_str() == "Connection")
                .map(|section| section.rows.len())
                .sum()
        })
        .unwrap_or(0)
}

#[test]
fn add_move_delete_and_set_type_write_the_rows() {
    let mut document = Document::new();
    let id = on_a_page(
        &mut document,
        shape(&[("Width", 2.0), ("Height", 1.0)], Vec::new()),
    );

    let ix = document
        .add_connection_point(id, Point::new(0.5, 0.25))
        .expect("add");
    assert_eq!(ix, 0, "the first point is row 0");
    assert_eq!(
        document.connection_points(id),
        vec![Point::new(0.5, 0.25)],
        "the point reads back"
    );

    document
        .move_connection_point(id, 0, Point::new(1.5, 0.75))
        .expect("move");
    assert_eq!(document.connection_points(id), vec![Point::new(1.5, 0.75)]);

    document
        .set_connection_point_type(id, 0, 2)
        .expect("set type");
    assert_eq!(row_number(&document, id, 0, "Type"), Some(2.0));

    document.delete_connection_point(id, 0).expect("delete");
    assert!(document.connection_points(id).is_empty());
    assert_eq!(own_row_count(&document, id), 0, "the row is gone");
}

#[test]
fn add_returns_the_next_row_index() {
    let mut document = Document::new();
    let id = on_a_page(
        &mut document,
        shape(&[("Width", 2.0), ("Height", 1.0)], Vec::new()),
    );
    assert_eq!(document.next_connection_point_index(id), 0);
    let first = document
        .add_connection_point(id, Point::new(0.0, 0.0))
        .expect("first");
    let second = document
        .add_connection_point(id, Point::new(1.0, 0.0))
        .expect("second");
    assert_eq!((first, second), (0, 1));
    assert_eq!(document.next_connection_point_index(id), 2);
}

#[test]
fn move_keeps_the_type_direction_and_name() {
    let mut document = Document::new();
    let mut with_cells = row(0.0, 0.0);
    with_cells.name = Some("MyPoint".to_owned());
    with_cells.ix = Some(0);
    with_cells.cells.push(cell("DirX", 1.0));
    with_cells.cells.push(cell("DirY", 0.0));
    with_cells.cells.push(cell("Type", 1.0));
    let id = on_a_page(
        &mut document,
        shape(
            &[("Width", 2.0), ("Height", 1.0)],
            vec![connection_section(vec![with_cells])],
        ),
    );

    document
        .move_connection_point(id, 0, Point::new(1.0, 0.5))
        .expect("move");

    let entity = document.shape(id).expect("shape");
    let section = entity
        .sheet
        .sections
        .iter()
        .find(|section| section.name.as_str() == "Connection")
        .expect("section");
    let moved = section.rows.first().expect("row");
    assert_eq!(moved.name.as_deref(), Some("MyPoint"), "the row name stays");
    assert_eq!(row_number(&document, id, 0, "DirX"), Some(1.0));
    assert_eq!(row_number(&document, id, 0, "DirY"), Some(0.0));
    assert_eq!(row_number(&document, id, 0, "Type"), Some(1.0));
    assert_eq!(document.connection_points(id), vec![Point::new(1.0, 0.5)]);
}

#[test]
fn delete_renumbers_the_later_rows_down() {
    let mut document = Document::new();
    let mut rows = Vec::new();
    for x in [0.0, 1.0, 2.0] {
        let mut row = row(x, 0.0);
        row.ix = Some(i32::try_from(rows.len()).unwrap());
        rows.push(row);
    }
    let id = on_a_page(
        &mut document,
        shape(
            &[("Width", 2.0), ("Height", 1.0)],
            vec![connection_section(rows)],
        ),
    );

    document.delete_connection_point(id, 1).expect("delete");

    assert_eq!(
        row_indices(&document, id),
        vec![Some(0), Some(1)],
        "the rows after the deleted one move down by one"
    );
    assert_eq!(
        document.connection_points(id),
        vec![Point::new(0.0, 0.0), Point::new(2.0, 0.0)],
        "the surviving points keep their coordinates"
    );
}

#[test]
fn set_type_refuses_a_kind_above_two() {
    let mut document = Document::new();
    let id = on_a_page(
        &mut document,
        shape(
            &[("Width", 2.0), ("Height", 1.0)],
            vec![connection_section(vec![row(0.0, 0.0)])],
        ),
    );
    let error = document
        .set_connection_point_type(id, 0, 3)
        .expect_err("kind 3");
    assert!(error.to_string().contains("0, 1 or 2"), "{error}");
    assert!(document.set_connection_point_type(id, 0, 2).is_ok());
}

#[test]
fn the_first_edit_copies_inherited_rows_with_their_indices() {
    let mut document = Document::new();
    let mut inherited = row(1.0, 1.0);
    inherited.ix = Some(3);
    inherited.cells.push(cell("Type", 1.0));
    let master_shape = document.add_shape(shape(
        &[("Width", 2.0), ("Height", 2.0)],
        vec![connection_section(vec![inherited])],
    ));
    let mut master = Master::new(MasterId { index: 0 }, 1);
    master.shapes.push(master_shape);
    document.masters.push(master);
    let mut instance = shape(&[("Width", 4.0), ("Height", 4.0)], Vec::new());
    instance.master = Some(MasterId { index: 0 });
    let id = on_a_page(&mut document, instance);

    // Before the first edit the points come from the master, scaled.
    assert_eq!(document.connection_points(id), vec![Point::new(2.0, 2.0)]);

    // The first edit copies the rows in, with the same indices and the
    // scaled values, so existing glue keeps its target.
    document
        .add_connection_point(id, Point::new(0.0, 0.0))
        .expect("add");
    assert_eq!(
        own_row_count(&document, id),
        2,
        "the inherited row is copied"
    );
    assert_eq!(row_indices(&document, id), vec![Some(3), Some(4)]);
    assert_eq!(row_number(&document, id, 0, "Type"), Some(1.0));
    assert_eq!(
        document.connection_points(id),
        vec![Point::new(2.0, 2.0), Point::new(0.0, 0.0)],
        "the copied point keeps its effective position"
    );

    // Edits now land on the instance's own rows (instance rows win).
    document
        .move_connection_point(id, 3, Point::new(1.0, 3.0))
        .expect("move");
    assert_eq!(
        document.connection_points(id),
        vec![Point::new(1.0, 3.0), Point::new(0.0, 0.0)]
    );
    assert_eq!(own_row_count(&document, id), 2, "no second copy");
}

#[test]
fn stale_ids_are_errors() {
    let mut document = Document::new();
    let id = on_a_page(
        &mut document,
        shape(
            &[("Width", 2.0), ("Height", 1.0)],
            vec![connection_section(vec![row(0.0, 0.0)])],
        ),
    );
    let ghost = ShapeId { index: 99 };

    assert!(
        document
            .add_connection_point(ghost, Point::new(0.0, 0.0))
            .is_err()
    );
    assert!(
        document
            .move_connection_point(ghost, 0, Point::new(0.0, 0.0))
            .is_err()
    );
    assert!(document.delete_connection_point(ghost, 0).is_err());
    assert!(document.set_connection_point_type(ghost, 0, 0).is_err());

    assert!(
        document
            .add_connection_point(id, Point::new(f64::NAN, 0.0))
            .is_err()
    );
    assert!(
        document
            .move_connection_point(id, 0, Point::new(0.0, f64::INFINITY))
            .is_err()
    );

    assert!(
        document
            .move_connection_point(id, 7, Point::new(0.0, 0.0))
            .is_err()
    );
    assert!(document.delete_connection_point(id, 7).is_err());
    assert!(document.set_connection_point_type(id, 7, 0).is_err());
}

#[test]
fn a_shape_without_points_gains_them_from_nothing() {
    let mut document = Document::new();
    // A shape with no master and no rows: an edit adds rows without a
    // copy-in, and a delete of the last row leaves an empty section.
    let id = on_a_page(
        &mut document,
        shape(&[("Width", 2.0), ("Height", 1.0)], Vec::new()),
    );
    assert!(document.connection_points(id).is_empty());
    document
        .add_connection_point(id, Point::new(1.0, 1.0))
        .expect("add");
    document.delete_connection_point(id, 0).expect("delete");
    assert!(document.connection_points(id).is_empty());
    assert_eq!(document.next_connection_point_index(id), 0);
}
