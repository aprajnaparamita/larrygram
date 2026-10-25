//! Unit-style integration tests for the scene builder (task P3-05).
//!
//! Every `GeometryRow` variant has one test (absolute and relative rows
//! alike; half of the phase exit criterion "every geometry row type has
//! tests"), and the acceptance list of P3-05 gets one test per feature:
//! transform composition, master inheritance (a corpus file), themed fills
//! (a corpus file), fills, strokes, line ends, shadows, text runs, layer
//! visibility, background pages, page size and warnings for bad values.
//! Task P3-15 adds one corpus test per acceptance item: the geometry row
//! union (closed paths from master rows), theme variant fills, and no
//! `Missing` warning for a cell with a documented default.
//! Corpus files are fetched, never committed (ADR 0005); the tests skip
//! with a message when `corpus/vsdx/` is absent, and fail instead when
//! `LARRYGRAM_REQUIRE_CORPUS=1` is set.

use std::path::{Path, PathBuf};

use larrygram_geom::{Angle, Point, Segment, Transform};
use larrygram_model::{
    Cell, CellName, CellValue, Document, Page, PageId, Row, Section, SectionName, Shape, ShapeId,
    TextContent, TextPart, WarningKind,
};
use larrygram_render::scene::{LineCap, LineJoin, Node, Paint, Rgba, Scene, Shape as SceneShape};
use larrygram_render::{BuildOptions, build_scene};
use larrygram_vsdx::{Limits, read_document};

// --- corpus plumbing -------------------------------------------------

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

/// The document of one corpus file, or `None` when the corpus is absent.
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test helper; tests may panic
fn corpus_document(name: &str) -> Option<Document> {
    let bytes = std::fs::read(corpus_dir().join(name)).ok()?;
    Some(read_document(&bytes, &Limits::DEFAULT).expect("reads"))
}

/// Fails the calling test when `LARRYGRAM_REQUIRE_CORPUS=1` is set and
/// the corpus is absent.
fn require_corpus_or(files: &[PathBuf]) -> bool {
    if files.is_empty() {
        assert!(
            std::env::var_os("LARRYGRAM_REQUIRE_CORPUS").is_none(),
            "corpus/vsdx is missing and LARRYGRAM_REQUIRE_CORPUS=1 is set"
        );
        eprintln!("skipping: corpus/vsdx is absent; run `cargo xtask corpus`");
        true
    } else {
        false
    }
}

// --- model fixtures --------------------------------------------------

/// A cell with a cached number.
fn number_cell(name: &str, value: f64) -> Cell {
    let mut cell = Cell::new(CellName::new(name));
    cell.value = Some(CellValue::Number(value));
    cell
}

/// A cell with a cached string.
fn string_cell(name: &str, value: &str) -> Cell {
    let mut cell = Cell::new(CellName::new(name));
    cell.value = Some(CellValue::Str(value.to_owned()));
    cell
}

/// One typed geometry row with numeric cells.
fn grow(row_type: &str, cells: &[(&str, f64)]) -> Row {
    let mut row = Row::new();
    row.row_type = Some(row_type.to_owned());
    row.cells = cells
        .iter()
        .map(|(name, value)| number_cell(name, *value))
        .collect();
    row
}

/// A one-page document with one empty shape.
fn document_with_shape() -> (Document, PageId, ShapeId) {
    let mut doc = Document::new();
    let shape = doc.add_shape(Shape::new(ShapeId { index: 0 }, 1));
    let page = PageId { index: 0 };
    let mut entity = Page::new(page, 1);
    entity.shapes.push(shape);
    doc.pages.push(entity);
    (doc, page, shape)
}

/// Adds one `Geometry` section of `rows` to a shape.
fn with_geometry(doc: &mut Document, shape: ShapeId, rows: Vec<Row>) {
    let mut section = Section::new(SectionName::new("Geometry"));
    section.rows = rows;
    if let Some(entity) = doc.shape_mut(shape) {
        entity.sheet.sections.push(section);
    }
}

/// The scene of the document's first page.
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // test helper; tests may panic
fn build(doc: &Document) -> Scene {
    build_scene(doc, PageId { index: 0 }, &BuildOptions::new()).expect("builds")
}

/// The path of the document's first shape node.
fn first_path(scene: &Scene) -> Vec<Segment> {
    for node in &scene.nodes {
        if let Node::Shape(shape) = node {
            return shape.path.segments().to_vec();
        }
    }
    Vec::new()
}

/// The first shape node of the scene.
fn first_shape(scene: &Scene) -> Option<&SceneShape> {
    scene.nodes.iter().find_map(|node| match node {
        Node::Shape(shape) => Some(shape.as_ref()),
        _ => None,
    })
}

/// The end points of a path's drawing segments, in order.
fn end_points(segments: &[Segment]) -> Vec<Point> {
    segments
        .iter()
        .filter_map(|segment| match segment {
            Segment::MoveTo(point) => Some(*point),
            Segment::LineTo(point) => Some(*point),
            Segment::CubicTo { to, .. } => Some(*to),
            _ => None,
        })
        .collect()
}

// --- geometry rows: one test per variant -----------------------------

#[test]
fn move_to_starts_a_subpath() {
    let (mut doc, _page, shape) = document_with_shape();
    with_geometry(
        &mut doc,
        shape,
        vec![
            grow("MoveTo", &[("X", 1.0), ("Y", 2.0)]),
            grow("LineTo", &[("X", 3.0), ("Y", 4.0)]),
        ],
    );
    let scene = build(&doc);
    assert_eq!(
        first_path(&scene),
        vec![
            Segment::MoveTo(Point::new(1.0, 2.0)),
            Segment::LineTo(Point::new(3.0, 4.0)),
        ]
    );
}

#[test]
fn line_to_continues_the_subpath() {
    let (mut doc, _page, shape) = document_with_shape();
    with_geometry(
        &mut doc,
        shape,
        vec![
            grow("MoveTo", &[("X", 0.0), ("Y", 0.0)]),
            grow("LineTo", &[("X", 1.0), ("Y", 0.0)]),
            grow("LineTo", &[("X", 1.0), ("Y", 1.0)]),
        ],
    );
    let scene = build(&doc);
    let path = first_path(&scene);
    assert_eq!(path.len(), 3);
    assert_eq!(path[2], Segment::LineTo(Point::new(1.0, 1.0)));
}

#[test]
fn arc_to_flattens_into_cubics() {
    let (mut doc, _page, shape) = document_with_shape();
    with_geometry(
        &mut doc,
        shape,
        vec![
            grow("MoveTo", &[("X", 0.0), ("Y", 0.0)]),
            grow("ArcTo", &[("X", 2.0), ("Y", 0.0), ("A", 1.0)]),
        ],
    );
    let path = first_path(&build(&doc));
    assert!(path.len() > 2, "{path:?}");
    assert!(
        path[1..]
            .iter()
            .all(|segment| matches!(segment, Segment::CubicTo { .. }))
    );
}

#[test]
fn elliptical_arc_to_flattens_into_cubics() {
    let (mut doc, _page, shape) = document_with_shape();
    with_geometry(
        &mut doc,
        shape,
        vec![
            grow("MoveTo", &[("X", 0.0), ("Y", 0.0)]),
            grow(
                "EllipticalArcTo",
                &[
                    ("X", 2.0),
                    ("Y", 0.0),
                    ("A", 1.0),
                    ("B", 1.0),
                    ("C", 0.0),
                    ("D", 2.0),
                ],
            ),
        ],
    );
    let path = first_path(&build(&doc));
    assert!(
        path[1..]
            .iter()
            .any(|segment| matches!(segment, Segment::CubicTo { .. }))
    );
}

#[test]
fn nurbs_to_builds_the_curve_from_the_formula() {
    let (mut doc, _page, shape) = document_with_shape();
    let mut row = grow(
        "NURBSTo",
        &[
            ("X", 3.0),
            ("Y", 0.0),
            ("A", 1.0),
            ("B", 1.0),
            ("C", 0.0),
            ("D", 1.0),
        ],
    );
    row.cells
        .push(string_cell("E", "NURBS(2, 1, 1, 1, 1, 1, 0.5, 1)"));
    with_geometry(
        &mut doc,
        shape,
        vec![grow("MoveTo", &[("X", 0.0), ("Y", 0.0)]), row],
    );
    let scene = build(&doc);
    assert_eq!(
        end_points(&first_path(&scene)),
        vec![
            Point::new(0.0, 0.0),
            Point::new(1.0, 1.0),
            Point::new(3.0, 0.0),
        ]
    );
    assert!(
        scene
            .warnings
            .iter()
            .all(|warning| warning.cell.as_str() != "E"),
        "{:?}",
        scene.warnings
    );
}

#[test]
fn polyline_to_walks_its_point_list() {
    let (mut doc, _page, shape) = document_with_shape();
    let mut row = grow("PolylineTo", &[("X", 3.0), ("Y", 0.0)]);
    row.cells.push(string_cell("A", "1 0; 2 1"));
    with_geometry(
        &mut doc,
        shape,
        vec![grow("MoveTo", &[("X", 0.0), ("Y", 0.0)]), row],
    );
    assert_eq!(
        end_points(&first_path(&build(&doc))),
        vec![
            Point::new(0.0, 0.0),
            Point::new(1.0, 0.0),
            Point::new(2.0, 1.0),
            Point::new(3.0, 0.0),
        ]
    );
}

#[test]
fn spline_start_opens_one_curve() {
    let (mut doc, _page, shape) = document_with_shape();
    with_geometry(
        &mut doc,
        shape,
        vec![
            grow("MoveTo", &[("X", 0.0), ("Y", 0.0)]),
            grow(
                "SplineStart",
                &[
                    ("X", 1.0),
                    ("Y", 1.0),
                    ("A", 1.0),
                    ("B", 0.0),
                    ("C", 2.0),
                    ("D", 2.0),
                ],
            ),
        ],
    );
    let path = first_path(&build(&doc));
    assert!(
        path[1..]
            .iter()
            .any(|segment| matches!(segment, Segment::CubicTo { .. }))
    );
}

#[test]
fn spline_knot_extends_the_open_curve() {
    let (mut doc, _page, shape) = document_with_shape();
    with_geometry(
        &mut doc,
        shape,
        vec![
            grow("MoveTo", &[("X", 0.0), ("Y", 0.0)]),
            grow(
                "SplineStart",
                &[
                    ("X", 1.0),
                    ("Y", 1.0),
                    ("A", 1.0),
                    ("B", 0.0),
                    ("C", 2.0),
                    ("D", 2.0),
                ],
            ),
            grow("SplineKnot", &[("X", 2.0), ("Y", 0.0), ("A", 1.5)]),
        ],
    );
    let scene = build(&doc);
    assert_eq!(
        end_points(&first_path(&scene)).last(),
        Some(&Point::new(2.0, 0.0))
    );
}

#[test]
fn ellipse_draws_a_closed_curve() {
    let (mut doc, _page, shape) = document_with_shape();
    with_geometry(
        &mut doc,
        shape,
        vec![
            grow("MoveTo", &[("X", 0.0), ("Y", 0.0)]),
            grow(
                "Ellipse",
                &[
                    ("X", 0.0),
                    ("Y", 0.0),
                    ("A", 1.0),
                    ("B", 0.0),
                    ("C", 0.0),
                    ("D", 1.0),
                ],
            ),
        ],
    );
    let path = first_path(&build(&doc));
    assert!(
        path[1..]
            .iter()
            .any(|segment| matches!(segment, Segment::CubicTo { .. }))
    );
    assert_eq!(path.last(), Some(&Segment::Close));
}

#[test]
fn infinite_line_clips_to_the_page() {
    let (mut doc, page, shape) = document_with_shape();
    if let Some(entity) = doc.pages.get_mut(page.index as usize) {
        entity.sheet.cells = vec![
            number_cell("PageWidth", 10.0),
            number_cell("PageHeight", 8.0),
        ];
    }
    with_geometry(
        &mut doc,
        shape,
        vec![grow(
            "InfiniteLine",
            &[("X", -5.0), ("Y", 4.0), ("A", 15.0), ("B", 4.0)],
        )],
    );
    assert_eq!(
        first_path(&build(&doc)),
        vec![
            Segment::MoveTo(Point::new(0.0, 4.0)),
            Segment::LineTo(Point::new(10.0, 4.0)),
        ]
    );
}

#[test]
fn rel_move_to_scales_by_the_shape_size() {
    let (mut doc, _page, shape) = document_with_shape();
    {
        let entity = doc.shape_mut(shape).expect("shape");
        entity.sheet.cells = vec![number_cell("Width", 2.0), number_cell("Height", 4.0)];
    }
    with_geometry(
        &mut doc,
        shape,
        vec![
            grow("RelMoveTo", &[("X", 0.5), ("Y", 0.5)]),
            grow("RelLineTo", &[("X", 1.0), ("Y", 1.0)]),
        ],
    );
    assert_eq!(
        first_path(&build(&doc)),
        vec![
            Segment::MoveTo(Point::new(1.0, 2.0)),
            Segment::LineTo(Point::new(2.0, 4.0)),
        ]
    );
}

#[test]
fn rel_line_to_scales_by_the_shape_size() {
    let (mut doc, _page, shape) = document_with_shape();
    {
        let entity = doc.shape_mut(shape).expect("shape");
        entity.sheet.cells = vec![number_cell("Width", 4.0), number_cell("Height", 2.0)];
    }
    with_geometry(
        &mut doc,
        shape,
        vec![
            grow("RelMoveTo", &[("X", 0.0), ("Y", 0.0)]),
            grow("RelLineTo", &[("X", 0.5), ("Y", 0.5)]),
        ],
    );
    assert_eq!(
        first_path(&build(&doc))[1],
        Segment::LineTo(Point::new(2.0, 1.0))
    );
}

#[test]
fn rel_quad_bez_to_scales_control_points() {
    let (mut doc, _page, shape) = document_with_shape();
    {
        let entity = doc.shape_mut(shape).expect("shape");
        entity.sheet.cells = vec![number_cell("Width", 4.0), number_cell("Height", 2.0)];
    }
    with_geometry(
        &mut doc,
        shape,
        vec![
            grow("RelMoveTo", &[("X", 0.0), ("Y", 0.0)]),
            grow(
                "RelQuadBezTo",
                &[("X", 0.5), ("Y", 0.5), ("A", 0.25), ("B", 1.0)],
            ),
        ],
    );
    match first_path(&build(&doc)).get(1) {
        Some(Segment::CubicTo { ctrl1, to, .. }) => {
            // The quadratic maps to one cubic (ADR 0022): the control
            // point sits two thirds along `from`..`ctrl`.
            assert!((ctrl1.x - 2.0 / 3.0).abs() < 1e-9, "{ctrl1:?}");
            assert!((ctrl1.y - 4.0 / 3.0).abs() < 1e-9, "{ctrl1:?}");
            assert_eq!(*to, Point::new(2.0, 1.0));
        }
        other => panic!("expected a cubic, got {other:?}"),
    }
}

#[test]
fn rel_cub_bez_to_scales_control_points() {
    let (mut doc, _page, shape) = document_with_shape();
    {
        let entity = doc.shape_mut(shape).expect("shape");
        entity.sheet.cells = vec![number_cell("Width", 2.0), number_cell("Height", 2.0)];
    }
    with_geometry(
        &mut doc,
        shape,
        vec![
            grow("RelMoveTo", &[("X", 0.0), ("Y", 0.0)]),
            grow(
                "RelCubBezTo",
                &[
                    ("X", 1.0),
                    ("Y", 0.0),
                    ("A", 0.25),
                    ("B", 1.0),
                    ("C", 0.75),
                    ("D", 1.0),
                ],
            ),
        ],
    );
    match first_path(&build(&doc)).get(1) {
        Some(Segment::CubicTo { ctrl1, ctrl2, to }) => {
            assert_eq!(*ctrl1, Point::new(0.5, 2.0));
            assert_eq!(*ctrl2, Point::new(1.5, 2.0));
            assert_eq!(*to, Point::new(2.0, 0.0));
        }
        other => panic!("expected a cubic, got {other:?}"),
    }
}

#[test]
fn rel_elliptical_arc_to_scales_the_control_point() {
    let (mut doc, _page, shape) = document_with_shape();
    {
        let entity = doc.shape_mut(shape).expect("shape");
        entity.sheet.cells = vec![number_cell("Width", 2.0), number_cell("Height", 2.0)];
    }
    with_geometry(
        &mut doc,
        shape,
        vec![
            grow("RelMoveTo", &[("X", 0.0), ("Y", 0.0)]),
            grow(
                "RelEllipticalArcTo",
                &[
                    ("X", 1.0),
                    ("Y", 1.0),
                    ("A", 0.5),
                    ("B", 0.5),
                    ("C", 0.0),
                    ("D", 1.0),
                ],
            ),
        ],
    );
    let path = first_path(&build(&doc));
    assert_eq!(end_points(&path).last(), Some(&Point::new(2.0, 2.0)));
}

#[test]
fn an_unknown_row_type_draws_nothing_and_warns() {
    let (mut doc, _page, shape) = document_with_shape();
    with_geometry(
        &mut doc,
        shape,
        vec![grow("Mystery", &[("X", 1.0), ("Y", 1.0)])],
    );
    let scene = build(&doc);
    assert!(first_path(&scene).is_empty());
    assert!(
        scene
            .warnings
            .iter()
            .any(|warning| warning.cell.as_str() == "Mystery")
    );
}

#[test]
fn an_unparsable_nurbs_formula_wins_a_warning_and_a_line() {
    let (mut doc, _page, shape) = document_with_shape();
    let mut row = grow(
        "NURBSTo",
        &[
            ("X", 2.0),
            ("Y", 0.0),
            ("A", 1.0),
            ("B", 1.0),
            ("C", 0.0),
            ("D", 1.0),
        ],
    );
    row.cells.push(string_cell("E", "NURBS(what?"));
    with_geometry(
        &mut doc,
        shape,
        vec![grow("MoveTo", &[("X", 0.0), ("Y", 0.0)]), row],
    );
    let scene = build(&doc);
    assert_eq!(first_path(&scene)[1], Segment::LineTo(Point::new(2.0, 0.0)));
    // Formula-parse degradations warn `KindMismatch` (ADR 0021
    // amendment, P3-16), never `ErrorValue`: the cached value is not an
    // error.
    assert!(scene.warnings.iter().any(|warning| {
        warning.cell.as_str() == "E" && warning.kind == WarningKind::KindMismatch
    }));
}

// --- transforms ------------------------------------------------------

#[test]
fn transform_composition_matches_the_adr_formula() {
    let (mut doc, _page, shape) = document_with_shape();
    {
        let entity = doc.shape_mut(shape).expect("shape");
        entity.sheet.cells = vec![
            number_cell("PinX", 3.0),
            number_cell("PinY", 4.0),
            number_cell("LocPinX", 1.0),
            number_cell("LocPinY", 2.0),
            number_cell("Width", 2.0),
            number_cell("Height", 4.0),
        ];
    }
    with_geometry(
        &mut doc,
        shape,
        vec![grow("MoveTo", &[("X", 0.0), ("Y", 0.0)])],
    );
    let transform = first_shape(&build(&doc)).expect("shape").transform;
    // translate(Pin) · translate(-LocPin): (0, 0) -> (2, 2).
    assert_eq!(transform.apply(Point::new(0.0, 0.0)), Point::new(2.0, 2.0));
    assert_eq!(transform.apply(Point::new(1.0, 2.0)), Point::new(3.0, 4.0));
}

#[test]
fn angle_and_flips_compose_like_visio() {
    let (mut doc, _page, shape) = document_with_shape();
    {
        let entity = doc.shape_mut(shape).expect("shape");
        entity.sheet.cells = vec![
            number_cell("Angle", core::f64::consts::FRAC_PI_2),
            number_cell("FlipX", 1.0),
        ];
    }
    with_geometry(
        &mut doc,
        shape,
        vec![grow("MoveTo", &[("X", 1.0), ("Y", 0.0)])],
    );
    let transform = first_shape(&build(&doc)).expect("shape").transform;
    // rotate(90°) · scale(-1, 1) sends (1, 0) to (0, -1).
    let at = transform.apply(Point::new(1.0, 0.0));
    assert!((at.x).abs() < 1e-9, "{at:?}");
    assert!((at.y + 1.0).abs() < 1e-9, "{at:?}");
}

#[test]
fn group_children_compose_through_the_parent_chain() {
    let (mut doc, _page, group) = document_with_shape();
    let child = doc.add_shape(Shape::new(ShapeId { index: 1 }, 2));
    if let Some(entity) = doc.shape_mut(group) {
        entity.children.push(child);
        entity.sheet.cells = vec![number_cell("PinX", 5.0), number_cell("PinY", 1.0)];
    }
    {
        let entity = doc.shape_mut(child).expect("child");
        entity.parent = Some(group);
        entity.sheet.cells = vec![number_cell("PinX", 1.0), number_cell("PinY", 2.0)];
    }
    with_geometry(
        &mut doc,
        child,
        vec![grow("MoveTo", &[("X", 0.0), ("Y", 0.0)])],
    );
    let scene = build(&doc);
    match scene.nodes.first() {
        Some(Node::Group(group)) => {
            assert_eq!(
                group.transform.apply(Point::new(0.0, 0.0)),
                Point::new(5.0, 1.0)
            );
            match group.children.first() {
                Some(Node::Shape(shape)) => {
                    // Node transforms are relative to the enclosing group;
                    // the backends compose the chain (ADR 0020).
                    assert_eq!(
                        shape.transform.apply(Point::new(0.0, 0.0)),
                        Point::new(1.0, 2.0)
                    );
                    assert_eq!(
                        shape
                            .transform
                            .then(group.transform)
                            .apply(Point::new(0.0, 0.0)),
                        Point::new(6.0, 3.0)
                    );
                }
                other => panic!("expected a shape child, got {other:?}"),
            }
        }
        other => panic!("expected a group, got {other:?}"),
    }
}

// --- fills, strokes, shadows, text -----------------------------------

#[test]
fn a_solid_fill_uses_the_foreground_colour() {
    let (mut doc, _page, shape) = document_with_shape();
    {
        let entity = doc.shape_mut(shape).expect("shape");
        entity.sheet.cells = vec![
            number_cell("FillPattern", 1.0),
            number_cell("FillForegnd", 4.0),
        ];
    }
    with_geometry(
        &mut doc,
        shape,
        vec![grow("MoveTo", &[("X", 0.0), ("Y", 0.0)])],
    );
    let scene = build(&doc);
    let shape = first_shape(&scene).expect("shape");
    assert_eq!(shape.fill, Some(Paint::Solid(Rgba::opaque(0, 0, 255))));
}

#[test]
fn a_linear_gradient_uses_the_stop_rows() {
    let (mut doc, _page, shape) = document_with_shape();
    {
        let entity = doc.shape_mut(shape).expect("shape");
        entity.sheet.cells = vec![
            number_cell("FillPattern", 1.0),
            number_cell("FillGradientEnabled", 1.0),
            number_cell("FillGradientDir", 0.0),
            number_cell("FillGradientAngle", 0.0),
            number_cell("Width", 2.0),
            number_cell("Height", 1.0),
        ];
        let mut section = Section::new(SectionName::new("FillGradient"));
        for (position, index) in [(0.0, 0.0), (1.0, 2.0)] {
            let mut row = Row::new();
            row.cells = vec![
                number_cell("GradientStopPosition", position),
                number_cell("GradientStopColor", index),
            ];
            section.rows.push(row);
        }
        entity.sheet.sections.push(section);
    }
    with_geometry(
        &mut doc,
        shape,
        vec![grow("MoveTo", &[("X", 0.0), ("Y", 0.0)])],
    );
    match &first_shape(&build(&doc)).expect("shape").fill {
        Some(Paint::LinearGradient { start, end, stops }) => {
            assert_eq!(*start, Point::new(0.0, 0.5));
            assert_eq!(*end, Point::new(2.0, 0.5));
            assert_eq!(stops.len(), 2);
            assert_eq!(stops[0].color, Rgba::opaque(0, 0, 0));
            assert_eq!(stops[1].color, Rgba::opaque(255, 0, 0));
        }
        other => panic!("expected a linear gradient, got {other:?}"),
    }
}

#[test]
fn a_radial_gradient_comes_from_the_direction_cell() {
    let (mut doc, _page, shape) = document_with_shape();
    {
        let entity = doc.shape_mut(shape).expect("shape");
        entity.sheet.cells = vec![
            number_cell("FillPattern", 30.0),
            number_cell("FillGradientDir", 1.0),
            number_cell("Width", 3.0),
            number_cell("Height", 4.0),
        ];
    }
    with_geometry(
        &mut doc,
        shape,
        vec![grow("MoveTo", &[("X", 0.0), ("Y", 0.0)])],
    );
    match &first_shape(&build(&doc)).expect("shape").fill {
        Some(Paint::RadialGradient {
            center,
            radius,
            stops,
        }) => {
            assert_eq!(*center, Point::new(1.5, 2.0));
            assert!((*radius - 2.5).abs() < 1e-9);
            assert_eq!(stops.len(), 2);
        }
        other => panic!("expected a radial gradient, got {other:?}"),
    }
}

#[test]
fn a_stroke_carries_dash_cap_join_and_line_ends() {
    let (mut doc, _page, shape) = document_with_shape();
    {
        let entity = doc.shape_mut(shape).expect("shape");
        entity.sheet.cells = vec![
            number_cell("LinePattern", 2.0),
            number_cell("LineWeight", 0.01),
            number_cell("LineCap", 0.0),
            number_cell("Rounding", 0.5),
            number_cell("BeginArrow", 4.0),
            number_cell("BeginArrowSize", 2.0),
        ];
    }
    with_geometry(
        &mut doc,
        shape,
        vec![grow("MoveTo", &[("X", 0.0), ("Y", 0.0)])],
    );
    let scene = build(&doc);
    let shape = first_shape(&scene).expect("shape");
    let stroke = shape.stroke.as_ref().expect("stroke");
    assert_eq!(stroke.width, 0.01);
    assert_eq!(stroke.dash.len(), 2);
    assert_eq!(stroke.cap, LineCap::Round);
    assert_eq!(stroke.join, LineJoin::Round);
    let begin = stroke.start.as_ref().expect("begin arrow");
    assert!(begin.filled);
    assert!((begin.length - 0.325).abs() < 1e-12);
}

#[test]
fn a_shadow_uses_the_shadow_cells_and_the_template() {
    let (mut doc, _page, shape) = document_with_shape();
    {
        let entity = doc.shape_mut(shape).expect("shape");
        entity.sheet.cells = vec![
            number_cell("ShapeShdwShow", 1.0),
            number_cell("ShdwPattern", 1.0),
            number_cell("ShapeShdwOffsetX", 0.1),
            number_cell("ShapeShdwOffsetY", -0.2),
            number_cell("ShapeShdwBlur", 0.05),
        ];
    }
    with_geometry(
        &mut doc,
        shape,
        vec![grow("MoveTo", &[("X", 0.0), ("Y", 0.0)])],
    );
    let shadow = first_shape(&build(&doc))
        .expect("shape")
        .shadow
        .expect("shadow");
    assert_eq!(shadow.offset, Point::new(0.1, -0.2));
    assert!((shadow.blur - 0.05).abs() < 1e-12);
}

#[test]
fn text_runs_resolve_through_the_character_rows() {
    let (mut doc, _page, shape) = document_with_shape();
    {
        let entity = doc.shape_mut(shape).expect("shape");
        entity.sheet.cells = vec![number_cell("Width", 2.0), number_cell("Height", 1.0)];
        let mut section = Section::new(SectionName::new("Character"));
        let mut first = Row::new();
        first.ix = Some(0);
        first.cells = vec![
            string_cell("Font", "Calibri"),
            number_cell("Size", 0.1666666666666667),
            number_cell("Style", 3.0),
        ];
        section.rows.push(first);
        let mut second = Row::new();
        second.ix = Some(1);
        second.cells = vec![number_cell("Size", 0.25)];
        section.rows.push(second);
        entity.sheet.sections.push(section);
        entity.text = Some(TextContent {
            parts: vec![
                TextPart::Text("Hi".to_owned()),
                TextPart::CharRun { ix: 1 },
                TextPart::Text(" there".to_owned()),
            ],
        });
    }
    with_geometry(
        &mut doc,
        shape,
        vec![grow("MoveTo", &[("X", 0.0), ("Y", 0.0)])],
    );
    let scene = build(&doc);
    let text = first_shape(&scene)
        .expect("shape")
        .text
        .as_ref()
        .expect("text");
    assert_eq!(text.runs.len(), 2);
    assert!(!text.layout.lines.is_empty());
    // The glyph size comes from `GlyphPos::size` (P3-16: `TextRun` no
    // longer carries one).
    for glyph in text.layout.lines.iter().flat_map(|line| &line.glyphs) {
        let expected = if glyph.run == 0 { 12.0 } else { 18.0 };
        assert!((glyph.size - expected).abs() < 1e-9, "{glyph:?}");
    }
}

// --- layers, pages, warnings ----------------------------------------

#[test]
fn shapes_on_invisible_layers_dropped_and_visible_layers_keep() {
    let (mut doc, page, shape) = document_with_shape();
    let second = doc.add_shape(Shape::new(ShapeId { index: 1 }, 2));
    for (id, member) in [(shape, "0"), (second, "1")] {
        if let Some(entity) = doc.shape_mut(id) {
            entity.sheet.cells = vec![string_cell("LayerMember", member)];
        }
        with_geometry(
            &mut doc,
            id,
            vec![grow("MoveTo", &[("X", 0.0), ("Y", 0.0)])],
        );
    }
    if let Some(entity) = doc.pages.get_mut(page.index as usize) {
        entity.shapes.push(second);
        let mut section = Section::new(SectionName::new("Layer"));
        for (ix, name, visible) in [(0, "Hidden", 0.0), (1, "Shown", 1.0)] {
            let mut row = Row::new();
            row.ix = Some(ix);
            row.cells = vec![string_cell("Name", name), number_cell("Visible", visible)];
            section.rows.push(row);
        }
        entity.sheet.sections.push(section);
    }
    let scene = build(&doc);
    assert_eq!(scene.nodes.len(), 1, "{:?}", scene.nodes);
}

#[test]
fn a_background_page_renders_first() {
    let (mut doc, page, shape) = document_with_shape();
    with_geometry(
        &mut doc,
        shape,
        vec![grow("MoveTo", &[("X", 0.0), ("Y", 0.0)])],
    );
    let background = PageId { index: 1 };
    let bg_shape = doc.add_shape(Shape::new(ShapeId { index: 1 }, 2));
    let mut entity = Page::new(background, 2);
    entity.shapes.push(bg_shape);
    doc.pages.push(entity);
    if let Some(entity) = doc.pages.get_mut(page.index as usize) {
        entity.background = Some(background);
    }
    with_geometry(
        &mut doc,
        bg_shape,
        vec![grow("MoveTo", &[("X", 1.0), ("Y", 1.0)])],
    );
    let scene = build(&doc);
    assert_eq!(scene.nodes.len(), 2);
    match scene.nodes.first() {
        Some(Node::Group(group)) => assert_eq!(group.children.len(), 1),
        other => panic!("expected a leading background group, got {other:?}"),
    }
}

#[test]
fn page_size_comes_from_the_page_sheet() {
    let (mut doc, page, _shape) = document_with_shape();
    if let Some(entity) = doc.pages.get_mut(page.index as usize) {
        entity.sheet.cells = vec![
            number_cell("PageWidth", 11.0),
            number_cell("PageHeight", 8.5),
        ];
    }
    let scene = build(&doc);
    assert_eq!((scene.width, scene.height), (11.0, 8.5));
}

#[test]
fn a_bad_cell_value_becomes_a_warning_not_a_panic() {
    let (mut doc, _page, shape) = document_with_shape();
    {
        let entity = doc.shape_mut(shape).expect("shape");
        let mut cell = Cell::new(CellName::new("PinX"));
        cell.value = Some(CellValue::Error("#VALUE!".to_owned()));
        entity.sheet.cells = vec![cell];
    }
    with_geometry(
        &mut doc,
        shape,
        vec![grow("MoveTo", &[("X", 0.0), ("Y", 0.0)])],
    );
    let scene = build(&doc);
    assert!(
        scene
            .warnings
            .iter()
            .any(|warning| matches!(warning.kind, WarningKind::ErrorValue(_)))
    );
}

// --- corpus ----------------------------------------------------------

#[test]
fn every_corpus_page_builds_without_panics() {
    let files = corpus_files();
    if require_corpus_or(&files) {
        return;
    }
    let mut built = 0;
    for file in &files {
        let bytes = std::fs::read(file).expect("reads");
        let doc = read_document(&bytes, &Limits::DEFAULT).expect("reads as a document");
        for (index, _) in doc.pages.iter().enumerate() {
            let page = PageId {
                index: u32::try_from(index).expect("page index"),
            };
            let scene = build_scene(&doc, page, &BuildOptions::new()).expect("builds");
            assert!(scene.width >= 0.0 && scene.height >= 0.0);
            built += 1;
        }
    }
    assert!(built > 0);
}

#[test]
fn an_instance_without_local_geometry_renders_the_master_geometry() {
    if require_corpus_or(&corpus_files()) {
        return;
    }
    // dwg.vsdx: instance shapes carry no `Geometry` section of their own.
    let doc = corpus_document("dwg.vsdx").expect("corpus file");
    let scene = build(&doc);
    assert!(
        scene
            .nodes
            .iter()
            .any(|node| matches!(node, Node::Shape(shape) if !shape.path.is_empty())),
        "no shape drew the master geometry"
    );
}

#[test]
fn a_themed_fill_resolves_to_a_colour_that_is_not_the_hard_default() {
    if require_corpus_or(&corpus_files()) {
        return;
    }
    // bgcolor.vsdx: the shapes' `FillForegnd` caches the theme-derived
    // colour of `THEMEGUARD(MSOTINT(THEMEVAL("AccentColor6"), 80))`
    // (`#fff2cc`, a themed value read through `CellReader::color`). The
    // fill must arrive in the scene as that colour, not as the hard
    // default black (ADR 0021 amendment).
    let doc = corpus_document("bgcolor.vsdx").expect("corpus file");
    let scene = build(&doc);
    assert!(
        scene
            .nodes
            .iter()
            .any(|node| matches!(node, Node::Shape(shape) if shape.fill
            == Some(Paint::Solid(Rgba::opaque(255, 242, 204))))),
        "no shape filled with the themed colour"
    );
}

#[test]
fn a_corpus_page_size_matches_the_file() {
    if require_corpus_or(&corpus_files()) {
        return;
    }
    let doc = corpus_document("bgcolor.vsdx").expect("corpus file");
    let scene = build(&doc);
    assert!((scene.width - 8.26771653543307).abs() < 1e-9);
    assert!((scene.height - 11.69291338582677).abs() < 1e-9);
}

#[test]
fn master_instances_keep_their_transform_chain() {
    if require_corpus_or(&corpus_files()) {
        return;
    }
    let doc = corpus_document("fdo86664.vsdx").expect("corpus file");
    let scene = build(&doc);
    for node in &scene.nodes {
        if let Node::Shape(shape) = node {
            let transform: Transform = shape.transform;
            let at = transform.apply(Point::new(0.0, 0.0));
            assert!(at.x.is_finite() && at.y.is_finite());
            let _ = Angle::from_radians(0.0);
        }
    }
}

#[test]
fn sparse_instance_rows_draw_a_closed_path_with_the_master_rows() {
    if require_corpus_or(&corpus_files()) {
        return;
    }
    // Acceptance (P3-15, fix 1): bgcolor.vsdx's `Rectangle` instances
    // carry two `LineTo` rows (`IX` 2..3, all cells `F='Inh'`) and their
    // master holds `MoveTo` + the closing rows (`IX` 1..5). With the row
    // union (ADR 0019 amendment) each draws the master's closed
    // rectangle: `MoveTo` + four `LineTo` segments, back at the start.
    let doc = corpus_document("bgcolor.vsdx").expect("corpus file");
    let scene = build(&doc);
    let mut closed = 0;
    for node in &scene.nodes {
        let Node::Shape(shape) = node else {
            continue;
        };
        let segments = shape.path.segments();
        let (Some(Segment::MoveTo(start)), Some(Segment::LineTo(_))) =
            (segments.first(), segments.get(1))
        else {
            continue;
        };
        let ends = end_points(segments);
        if segments.len() == 5 && ends.last() == Some(start) {
            closed += 1;
        }
    }
    assert_eq!(closed, 3, "the three `Rectangle` instances");
}

#[test]
fn a_themed_variant_fill_lands_in_the_scene() {
    if require_corpus_or(&corpus_files()) {
        return;
    }
    // Acceptance (P3-15, fix 2): color-boxes.vsdx's shapes 68..74 fill
    // with the theme's variation colours (`QuickStyleFillColor` 200..206,
    // see `read_mapping.rs` for the derivation). The scene carries the
    // colours, not the hard default black.
    let doc = corpus_document("color-boxes.vsdx").expect("corpus file");
    let scene = build(&doc);
    let mut fills: Vec<Rgba> = Vec::new();
    for node in &scene.nodes {
        if let Node::Shape(shape) = node {
            match shape.fill.as_ref() {
                Some(Paint::Solid(color)) => fills.push(*color),
                Some(Paint::LinearGradient { stops, .. })
                | Some(Paint::RadialGradient { stops, .. }) => {
                    fills.extend(stops.iter().map(|stop| stop.color));
                }
                _ => {}
            }
        }
    }
    for rgb in [(0x5B, 0x9B, 0xD5), (0x75, 0x9F, 0xCC), (0xFE, 0xC0, 0x00)] {
        assert!(
            fills.contains(&Rgba::opaque(rgb.0, rgb.1, rgb.2)),
            "no fill of {rgb:?} in {fills:?}"
        );
    }
}

#[test]
fn no_missing_warning_for_a_cell_with_a_documented_default() {
    let files = corpus_files();
    if require_corpus_or(&files) {
        return;
    }
    // Acceptance (P3-15, fix 3): a cell that is absent at every rung and
    // has a default documented on its ShapeSheet reference page resolves
    // to that default without a `Missing` warning (ADR 0021 amendment).
    // This test counts the warnings that remain for documented cells and
    // demands zero; a `Missing` warning for a cell with no documented
    // default is expected and fine.
    let mut documented = Vec::new();
    let mut undocumented = 0;
    for file in &files {
        let bytes = std::fs::read(file).expect("reads");
        let doc = read_document(&bytes, &Limits::DEFAULT).expect("reads as a document");
        for (index, _) in doc.pages.iter().enumerate() {
            let page = PageId {
                index: u32::try_from(index).expect("page index"),
            };
            let scene = build_scene(&doc, page, &BuildOptions::new()).expect("builds");
            for warning in &scene.warnings {
                if !matches!(warning.kind, WarningKind::Missing) {
                    continue;
                }
                if larrygram_model::documented_default(warning.cell.as_str()).is_some() {
                    documented.push((file.clone(), warning.cell.as_str().to_owned()));
                } else {
                    undocumented += 1;
                }
            }
        }
    }
    assert!(
        documented.is_empty(),
        "Missing warnings for cells with documented defaults: {documented:?}"
    );
    eprintln!("Missing warnings without a documented default: {undocumented}");
}
#[test]
fn color_boxes_renders_filled_and_stroked_shapes() {
    if require_corpus_or(&corpus_files()) {
        return;
    }
    // Acceptance (P3-15, the master note): the themed patterns resolve
    // through the theme's style lists and fall back to the *visible*
    // values (`FillPattern` 1, `LinePattern` 1, `LineWeight` the hairline
    // default), so page 1 of color-boxes.vsdx draws filled and stroked
    // shapes instead of a blank page.
    let doc = corpus_document("color-boxes.vsdx").expect("corpus file");
    let scene = build(&doc);
    let mut filled = 0;
    let mut stroked = 0;
    for node in &scene.nodes {
        if let Node::Shape(shape) = node {
            if shape.fill.is_some() {
                filled += 1;
            }
            if shape.stroke.is_some() {
                stroked += 1;
            }
        }
    }
    assert!(filled > 0, "no filled shape in the scene");
    assert!(stroked > 0, "no stroked shape in the scene");
}

// --- builder fallbacks (task P5-20) ---------------------------------

#[test]
fn a_scale_that_is_not_positive_falls_back_to_the_default() {
    // `BuildOptions::scale` that is zero, negative or not finite becomes
    // `DEFAULT_SCALE` (the flattening tolerance must stay positive).
    let (mut doc, page, shape) = document_with_shape();
    with_geometry(
        &mut doc,
        shape,
        vec![
            grow("MoveTo", &[("X", 0.0), ("Y", 0.0)]),
            grow("ArcTo", &[("X", 1.0), ("Y", 0.0), ("A", 0.2)]),
        ],
    );
    for scale in [0.0, -3.0, f64::NAN, f64::INFINITY] {
        let options = BuildOptions {
            background: true,
            scale,
        };
        let scene = build_scene(&doc, page, &options).expect("builds");
        assert!(!scene.nodes.is_empty(), "scale {scale}");
    }
}

#[test]
fn a_background_page_the_document_lacks_draws_an_empty_group() {
    // A `BackPage` that names a page the document does not hold degrades
    // to an empty background group, never an error and never a panic.
    let (mut doc, page, shape) = document_with_shape();
    if let Some(entity) = doc.pages.get_mut(page.index as usize) {
        entity.background = Some(PageId { index: 9 });
    }
    with_geometry(
        &mut doc,
        shape,
        vec![grow("MoveTo", &[("X", 0.0), ("Y", 0.0)])],
    );
    let scene = build(&doc);
    match scene.nodes.first() {
        Some(Node::Group(group)) => assert!(group.children.is_empty(), "{group:?}"),
        other => panic!("expected the empty background group, got {other:?}"),
    }
    assert_eq!(scene.nodes.len(), 2, "the page's own shape follows it");
}

#[test]
fn a_degenerate_infinite_line_row_draws_no_segment() {
    // `clip_infinite_line` degrades to "no segment": a line that misses
    // the page and a zero-length direction both draw nothing instead of
    // poisoning the path with NaN points.
    let cases: [[(&str, f64); 4]; 3] = [
        // Parallel above the page: the line never crosses it.
        [("X", 0.0), ("Y", 50.0), ("A", 1.0), ("B", 50.0)],
        // Diagonal above the page: the clip window is empty.
        [("X", 0.0), ("Y", 50.0), ("A", 1.0), ("B", 51.0)],
        // One point twice: no direction at all.
        [("X", 1.0), ("Y", 1.0), ("A", 1.0), ("B", 1.0)],
    ];
    for cells in cases {
        let (mut doc, _page, shape) = document_with_shape();
        {
            let entity = doc.shape_mut(shape).expect("shape");
            entity.sheet.cells = vec![
                number_cell("Width", 2.0),
                number_cell("Height", 1.0),
                number_cell("PinX", 1.0),
                number_cell("PinY", 0.5),
            ];
        }
        with_geometry(
            &mut doc,
            shape,
            vec![
                grow("MoveTo", &[("X", 0.0), ("Y", 0.0)]),
                grow("InfiniteLine", &cells),
            ],
        );
        let scene = build(&doc);
        assert_eq!(
            first_path(&scene),
            vec![Segment::MoveTo(Point::new(0.0, 0.0))],
            "the row draws nothing: {cells:?}"
        );
    }
}

#[test]
fn a_field_marker_draws_no_run_of_its_own() {
    // The `fld` marker maps to nothing (`TextPart::Field`'s empty arm);
    // its cached display text follows as a literal.
    let (mut doc, _page, shape) = document_with_shape();
    {
        let entity = doc.shape_mut(shape).expect("shape");
        entity.sheet.cells = vec![number_cell("Width", 2.0), number_cell("Height", 1.0)];
        entity.text = Some(TextContent {
            parts: vec![
                TextPart::Field { ix: 0 },
                TextPart::Text("cached".to_owned()),
            ],
        });
    }
    with_geometry(
        &mut doc,
        shape,
        vec![grow("MoveTo", &[("X", 0.0), ("Y", 0.0)])],
    );
    let scene = build(&doc);
    let text = first_shape(&scene)
        .expect("shape")
        .text
        .as_ref()
        .expect("text");
    assert_eq!(text.runs.len(), 1, "only the literal text is a run");
}

#[test]
fn a_zero_character_size_still_lays_the_text_out() {
    // A `Size` of zero is not a layout size: `run_style` substitutes
    // `DEFAULT_SIZE_PT`, so `paragraph_of`'s "no first size" arm
    // (`build.rs`'s line-spacing `else`) is unreachable. What stays
    // pinned here is the outcome: a zero-size paragraph lays out.
    let (mut doc, _page, shape) = document_with_shape();
    {
        let entity = doc.shape_mut(shape).expect("shape");
        entity.sheet.cells = vec![number_cell("Width", 2.0), number_cell("Height", 1.0)];
        let mut character = Section::new(SectionName::new("Character"));
        let mut size = Row::new();
        size.ix = Some(0);
        size.cells = vec![number_cell("Size", 0.0)];
        character.rows.push(size);
        entity.sheet.sections.push(character);
        let mut paragraph = Section::new(SectionName::new("Paragraph"));
        let mut spacing = Row::new();
        spacing.ix = Some(0);
        spacing.cells = vec![number_cell("SpLine", 2.0)];
        paragraph.rows.push(spacing);
        entity.sheet.sections.push(paragraph);
        entity.text = Some(TextContent {
            parts: vec![
                TextPart::CharRun { ix: 0 },
                TextPart::ParaRun { ix: 0 },
                TextPart::Text("Hi".to_owned()),
            ],
        });
    }
    with_geometry(
        &mut doc,
        shape,
        vec![grow("MoveTo", &[("X", 0.0), ("Y", 0.0)])],
    );
    let scene = build(&doc);
    let text = first_shape(&scene)
        .expect("shape")
        .text
        .as_ref()
        .expect("text");
    assert!(
        !text.layout.lines.is_empty(),
        "the text still lays out at the fallback spacing"
    );
}

#[test]
fn a_tiled_fill_pattern_scales_the_unit_box_tile() {
    // `FillPattern` 8 draws the dotted tile: the template's unit-box
    // tile (curves and closes) scales by the pitch (`scale_path`).
    let (mut doc, _page, shape) = document_with_shape();
    {
        let entity = doc.shape_mut(shape).expect("shape");
        entity.sheet.cells = vec![
            number_cell("Width", 2.0),
            number_cell("Height", 1.0),
            number_cell("FillPattern", 8.0),
        ];
    }
    with_geometry(
        &mut doc,
        shape,
        vec![
            grow("MoveTo", &[("X", 0.0), ("Y", 0.0)]),
            grow("LineTo", &[("X", 2.0), ("Y", 0.0)]),
            grow("LineTo", &[("X", 2.0), ("Y", 1.0)]),
            grow("LineTo", &[("X", 0.0), ("Y", 1.0)]),
            grow("LineTo", &[("X", 0.0), ("Y", 0.0)]),
        ],
    );
    let scene = build(&doc);
    match &first_shape(&scene).expect("shape").fill {
        Some(Paint::Pattern(pattern)) => {
            let segments = pattern.tile.segments();
            assert!(
                segments
                    .iter()
                    .any(|segment| matches!(segment, Segment::CubicTo { .. })),
                "the tile carries its curves: {segments:?}"
            );
            assert!(
                segments
                    .iter()
                    .any(|segment| matches!(segment, Segment::Close)),
                "the tile carries its close: {segments:?}"
            );
        }
        other => panic!("expected a pattern fill, got {other:?}"),
    }
}

#[test]
fn a_second_spline_start_flushes_the_first_curve() {
    // Two `SplineStart` rows in one section: the first pending curve is
    // flushed when the second starts (a stray start is not lost).
    let start = |x, y| {
        grow(
            "SplineStart",
            &[
                ("X", x),
                ("Y", y),
                ("A", 1.0),
                ("B", 0.0),
                ("C", 2.0),
                ("D", 2.0),
            ],
        )
    };
    let (mut doc, _page, shape) = document_with_shape();
    with_geometry(
        &mut doc,
        shape,
        vec![
            grow("MoveTo", &[("X", 0.0), ("Y", 0.0)]),
            start(1.0, 1.0),
            start(3.0, 1.0),
            grow("SplineKnot", &[("X", 4.0), ("Y", 0.0), ("A", 1.5)]),
        ],
    );
    let path = first_path(&build(&doc));
    let curves = path
        .iter()
        .filter(|segment| matches!(segment, Segment::CubicTo { .. }))
        .count();
    assert!(curves >= 2, "both splines flush: {path:?}");
}

#[test]
fn a_background_page_chain_nests_its_groups() {
    // A page whose background page has a background of its own: the
    // chain renders as nested background groups, each page once (ADR
    // 0020).
    let (mut doc, page, shape) = document_with_shape();
    with_geometry(
        &mut doc,
        shape,
        vec![grow("MoveTo", &[("X", 0.0), ("Y", 0.0)])],
    );
    let outer = doc.add_page("Back-1").expect("page");
    let inner = doc.add_page("Back-2").expect("page");
    if let Some(entity) = doc.pages.get_mut(page.index as usize) {
        entity.background = Some(outer);
    }
    if let Some(entity) = doc.pages.get_mut(outer.index as usize) {
        entity.background = Some(inner);
    }
    let scene = build(&doc);
    match scene.nodes.first() {
        Some(Node::Group(group)) => assert!(
            group
                .children
                .iter()
                .any(|child| matches!(child, Node::Group(_))),
            "the chain nests: {group:?}"
        ),
        other => panic!("expected the background group, got {other:?}"),
    }
}
