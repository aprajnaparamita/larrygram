//! Scene hit-testing (ADR 0041): the rules of the hit policy against
//! real scene graphs, and the scene identity the builder records
//! (`scene::Shape::shape_id`).

use std::f64::consts::FRAC_PI_2;

use larrygram_geom::{Angle, Path, Point, Rect, Segment, Transform};
use larrygram_model::{
    Cell, CellName, CellValue, Document, Page, PageId, Row, Section, SectionName,
    Shape as ModelShape, ShapeId,
};
use larrygram_render::hit::{
    HitKind, HitOptions, NodePath, hit_marquee, hit_test, node_bounds, scene_bounds,
};
use larrygram_render::scene::{Node, Paint, Rgba, Scene, Shape, TextNode};
use larrygram_render::{BuildOptions, build_scene};
use larrygram_text::Layout;

// --- scene helpers ---------------------------------------------------

/// A rectangle path from `(0, 0)` to `(width, height)`.
fn rect_path(width: f64, height: f64) -> Path {
    let mut path = Path::new();
    path.push(Segment::MoveTo(Point::new(0.0, 0.0)));
    path.push(Segment::LineTo(Point::new(width, 0.0)));
    path.push(Segment::LineTo(Point::new(width, height)));
    path.push(Segment::LineTo(Point::new(0.0, height)));
    path.push(Segment::Close);
    path
}

/// One shape node.
fn shape_node(
    shape_id: Option<ShapeId>,
    transform: Transform,
    path: Path,
    filled: bool,
    text: Option<TextNode>,
) -> Node {
    Node::Shape(Box::new(Shape {
        shape_id,
        transform,
        path,
        fill: filled.then(|| Paint::Solid(Rgba::opaque(200, 200, 200))),
        stroke: None,
        shadow: None,
        clip: None,
        text,
    }))
}

/// A filled rectangle shape.
fn filled(id: Option<ShapeId>, transform: Transform, width: f64, height: f64) -> Node {
    shape_node(id, transform, rect_path(width, height), true, None)
}

/// An unfilled rectangle shape.
fn outlined(id: Option<ShapeId>, transform: Transform, width: f64, height: f64) -> Node {
    shape_node(id, transform, rect_path(width, height), false, None)
}

/// A text block at `origin`, `width` by `height` inches.
fn text_block(origin: Point, width: f64, height: f64) -> TextNode {
    TextNode {
        block: larrygram_geom::Size::new(width, height),
        margins: larrygram_text::Insets::default(),
        horz_align: larrygram_text::Align::Left,
        fonts: Vec::new(),
        runs: Vec::new(),
        layout: Layout {
            width,
            height,
            ..Layout::default()
        },
        color: Rgba::opaque(0, 0, 0),
        origin,
    }
}

/// The hit kinds and nodes of a hit list, for order assertions.
fn shape(hits: &[larrygram_render::hit::Hit]) -> Vec<(Vec<usize>, Option<ShapeId>, HitKind)> {
    hits.iter()
        .map(|hit| (hit.node.0.clone(), hit.shape_id, hit.kind))
        .collect()
}

// --- fill, outline and text -----------------------------------------

#[test]
fn filled_shapes_hit_on_their_fill() {
    let mut scene = Scene::new(8.5, 11.0);
    scene.nodes.push(filled(
        Some(ShapeId { index: 7 }),
        Transform::identity(),
        1.0,
        1.0,
    ));
    let hits = hit_test(&scene, Point::new(0.5, 0.5), &HitOptions::default()).expect("no error");
    assert_eq!(
        shape(&hits),
        vec![(vec![0], Some(ShapeId { index: 7 }), HitKind::Fill)]
    );
    assert_eq!(hits[0].distance, 0.0);
}

#[test]
fn filled_shapes_hit_on_their_outline_within_tolerance() {
    let mut scene = Scene::new(8.5, 11.0);
    scene
        .nodes
        .push(filled(None, Transform::identity(), 1.0, 1.0));
    // Just outside the right edge.
    let point = Point::new(1.005, 0.5);
    let hits = hit_test(&scene, point, &HitOptions::new(0.01)).expect("no error");
    assert_eq!(shape(&hits), vec![(vec![0], None, HitKind::Stroke)]);
    assert!((hits[0].distance - 0.005).abs() < 1e-12, "{:?}", hits[0]);
    let hits = hit_test(&scene, point, &HitOptions::new(0.001)).expect("no error");
    assert!(hits.is_empty(), "outside the tolerance: {hits:?}");
}

#[test]
fn unfilled_shapes_hit_on_outline_and_text_only() {
    let mut scene = Scene::new(8.5, 11.0);
    scene
        .nodes
        .push(outlined(None, Transform::identity(), 1.0, 1.0));
    // Inside the hole of an unfilled shape: no hit.
    let hits = hit_test(&scene, Point::new(0.5, 0.5), &HitOptions::new(0.01)).expect("no error");
    assert!(hits.is_empty(), "{hits:?}");
    // On the outline: a hit.
    let hits = hit_test(&scene, Point::new(1.005, 0.5), &HitOptions::new(0.01)).expect("no error");
    assert_eq!(shape(&hits), vec![(vec![0], None, HitKind::Stroke)]);
}

#[test]
fn text_blocks_count_as_hit() {
    let mut scene = Scene::new(8.5, 11.0);
    // A text-only shape: the block sits at (0, 0)..(1, 1) in page space
    // (`origin + (x, -y)` from the layout box).
    scene.nodes.push(shape_node(
        None,
        Transform::identity(),
        Path::new(),
        false,
        Some(text_block(Point::new(0.0, 1.0), 1.0, 1.0)),
    ));
    let hits = hit_test(&scene, Point::new(0.5, 0.5), &HitOptions::default()).expect("no error");
    assert_eq!(shape(&hits), vec![(vec![0], None, HitKind::Text)]);
    assert_eq!(hits[0].distance, 0.0);
    let hits = hit_test(&scene, Point::new(1.5, 0.5), &HitOptions::default()).expect("no error");
    assert!(hits.is_empty(), "{hits:?}");
}

/// Task P7-51 item 4: a text block larger than its laid-out text —
/// the press hits through the whole block, and the shape's bounds are
/// the block's (task P7-48's `TextNode::block` change).
#[test]
fn a_block_larger_than_its_text_hits_through_the_whole_block() {
    let mut scene = Scene::new(8.5, 11.0);
    // A 2 x 1 in block at (0, 0)..(2, 1) in page space whose one line
    // of text lays out in the top-left corner only (0.2 x 0.15 in).
    let mut text = text_block(Point::new(0.0, 1.0), 2.0, 1.0);
    text.layout.width = 0.2;
    text.layout.height = 0.15;
    text.layout.lines.push(larrygram_text::Line {
        glyphs: Vec::new(),
        baseline: 0.12,
        width: 0.2,
        paragraph: 0,
    });
    scene.nodes.push(shape_node(
        Some(ShapeId { index: 5 }),
        Transform::identity(),
        Path::new(),
        false,
        Some(text),
    ));
    // Inside the block, outside the laid-out text: the press hits the
    // shape through the block.
    let hits = hit_test(&scene, Point::new(1.5, 0.2), &HitOptions::default()).expect("no error");
    assert_eq!(
        shape(&hits),
        vec![(vec![0], Some(ShapeId { index: 5 }), HitKind::Text)]
    );
    // Outside the block: no hit.
    let hits = hit_test(&scene, Point::new(2.5, 0.2), &HitOptions::default()).expect("no error");
    assert!(hits.is_empty(), "{hits:?}");
    // The bounds are the block's, not the laid-out text's.
    let bounds = node_bounds(&scene, &NodePath::new().child(0)).expect("no error");
    assert_eq!(
        bounds,
        Rect::new(Point::new(0.0, 0.0), Point::new(2.0, 1.0))
    );
}

#[test]
fn the_fill_beats_the_text_of_one_shape() {
    let mut scene = Scene::new(8.5, 11.0);
    scene.nodes.push(shape_node(
        Some(ShapeId { index: 1 }),
        Transform::identity(),
        rect_path(1.0, 1.0),
        true,
        Some(text_block(Point::new(0.0, 1.0), 1.0, 1.0)),
    ));
    let hits = hit_test(&scene, Point::new(0.5, 0.5), &HitOptions::default()).expect("no error");
    assert_eq!(
        shape(&hits),
        vec![(vec![0], Some(ShapeId { index: 1 }), HitKind::Fill)]
    );
}

#[test]
fn a_zero_tolerance_still_hits_the_outline_exactly() {
    let mut scene = Scene::new(8.5, 11.0);
    scene
        .nodes
        .push(outlined(None, Transform::identity(), 1.0, 1.0));
    let hits = hit_test(&scene, Point::new(1.0, 0.5), &HitOptions::default()).expect("no error");
    assert_eq!(shape(&hits), vec![(vec![0], None, HitKind::Stroke)]);
}

// --- z-order and groups ---------------------------------------------

#[test]
fn results_come_topmost_first() {
    let mut scene = Scene::new(8.5, 11.0);
    scene.nodes.push(filled(
        Some(ShapeId { index: 0 }),
        Transform::identity(),
        1.0,
        1.0,
    ));
    scene.nodes.push(filled(
        Some(ShapeId { index: 1 }),
        Transform::identity(),
        1.0,
        1.0,
    ));
    let hits = hit_test(&scene, Point::new(0.5, 0.5), &HitOptions::default()).expect("no error");
    assert_eq!(
        shape(&hits),
        vec![
            (vec![1], Some(ShapeId { index: 1 }), HitKind::Fill),
            (vec![0], Some(ShapeId { index: 0 }), HitKind::Fill),
        ]
    );
}

#[test]
fn nested_groups_hit_through_their_members() {
    let mut scene = Scene::new(8.5, 11.0);
    let member = filled(
        Some(ShapeId { index: 4 }),
        Transform::translate(Point::new(0.0, 3.0)),
        1.0,
        1.0,
    );
    scene
        .nodes
        .push(Node::Group(larrygram_render::scene::Group {
            transform: Transform::translate(Point::new(2.0, 0.0)),
            clip: None,
            children: vec![member],
        }));
    // The member is at (2, 3)..(3, 4) on the page.
    let hits = hit_test(&scene, Point::new(2.5, 3.5), &HitOptions::default()).expect("no error");
    assert_eq!(
        shape(&hits),
        vec![(vec![0, 0], Some(ShapeId { index: 4 }), HitKind::Fill)]
    );
    // A hit in the group reports the member node, never the group node.
    assert!(hits.iter().all(|hit| hit.node.indices() == [0, 0]));
}

#[test]
fn a_groups_own_shape_hits_like_any_shape() {
    let mut scene = Scene::new(8.5, 11.0);
    let own = filled(Some(ShapeId { index: 8 }), Transform::identity(), 1.0, 1.0);
    let member = filled(Some(ShapeId { index: 9 }), Transform::identity(), 1.0, 1.0);
    scene
        .nodes
        .push(Node::Group(larrygram_render::scene::Group {
            transform: Transform::translate(Point::new(5.0, 0.0)),
            clip: None,
            children: vec![own, member],
        }));
    // The group's own shape is its first child (ADR 0020 amendment).
    let hits = hit_test(&scene, Point::new(5.5, 0.5), &HitOptions::default()).expect("no error");
    assert_eq!(
        shape(&hits),
        vec![
            (vec![0, 1], Some(ShapeId { index: 9 }), HitKind::Fill),
            (vec![0, 0], Some(ShapeId { index: 8 }), HitKind::Fill),
        ]
    );
}

// --- marquee --------------------------------------------------------

#[test]
fn marquee_intersects_bounds_it_does_not_contain() {
    let mut scene = Scene::new(8.5, 11.0);
    scene.nodes.push(filled(
        Some(ShapeId { index: 0 }),
        Transform::identity(),
        1.0,
        1.0,
    ));
    scene.nodes.push(filled(
        Some(ShapeId { index: 1 }),
        Transform::translate(Point::new(3.0, 3.0)),
        1.0,
        1.0,
    ));
    // The rectangle crosses both shapes (intersection, not containment).
    let rect = Rect::new(Point::new(0.5, 0.5), Point::new(3.5, 3.5));
    let hits = hit_marquee(&scene, rect).expect("no error");
    assert_eq!(
        shape(&hits),
        vec![
            (vec![1], Some(ShapeId { index: 1 }), HitKind::Bounds),
            (vec![0], Some(ShapeId { index: 0 }), HitKind::Bounds),
        ]
    );
    assert!(hits.iter().all(|hit| hit.distance == 0.0));
    // A rectangle fully inside one shape still intersects it.
    let rect = Rect::new(Point::new(0.25, 0.25), Point::new(0.5, 0.5));
    let hits = hit_marquee(&scene, rect).expect("no error");
    assert_eq!(hits.len(), 1);
    // A rectangle between the shapes intersects nothing.
    let rect = Rect::new(Point::new(2.0, 2.0), Point::new(2.5, 2.5));
    let hits = hit_marquee(&scene, rect).expect("no error");
    assert!(hits.is_empty(), "{hits:?}");
}

#[test]
fn marquee_normalizes_swapped_corners() {
    let mut scene = Scene::new(8.5, 11.0);
    scene
        .nodes
        .push(filled(None, Transform::identity(), 1.0, 1.0));
    let rect = Rect::new(Point::new(3.5, 3.5), Point::new(0.5, 0.5));
    let hits = hit_marquee(&scene, rect).expect("no error");
    assert_eq!(hits.len(), 1);
}

// --- bounds ---------------------------------------------------------

#[test]
fn rotated_bounds_are_the_transformed_box() {
    let mut scene = Scene::new(8.5, 11.0);
    // A 2 by 1 rectangle turned 90 degrees about the origin:
    // corners (0, 0), (0, 2), (-1, 2), (-1, 0).
    scene.nodes.push(filled(
        None,
        Transform::rotate(Angle::from_radians(FRAC_PI_2)),
        2.0,
        1.0,
    ));
    let bounds = node_bounds(&scene, &NodePath::new().child(0)).expect("no error");
    let close = |a: f64, b: f64| (a - b).abs() < 1e-12;
    assert!(
        close(bounds.min.x, -1.0) && close(bounds.min.y, 0.0),
        "{bounds:?}"
    );
    assert!(
        close(bounds.max.x, 0.0) && close(bounds.max.y, 2.0),
        "{bounds:?}"
    );
    assert_eq!(scene_bounds(&scene).expect("no error"), bounds);
}

#[test]
fn node_bounds_compose_the_ancestor_transforms() {
    let mut scene = Scene::new(8.5, 11.0);
    let member = filled(None, Transform::translate(Point::new(0.0, 3.0)), 1.0, 1.0);
    scene
        .nodes
        .push(Node::Group(larrygram_render::scene::Group {
            transform: Transform::translate(Point::new(2.0, 0.0)),
            clip: None,
            children: vec![member],
        }));
    let bounds = node_bounds(&scene, &NodePath::new().child(0).child(0)).expect("no error");
    assert_eq!(
        bounds,
        Rect::new(Point::new(2.0, 3.0), Point::new(3.0, 4.0))
    );
    // The group node measures the union of its members.
    let bounds = node_bounds(&scene, &NodePath::new().child(0)).expect("no error");
    assert_eq!(
        bounds,
        Rect::new(Point::new(2.0, 3.0), Point::new(3.0, 4.0))
    );
}

#[test]
fn the_root_path_measures_the_whole_scene() {
    let mut scene = Scene::new(8.5, 11.0);
    scene
        .nodes
        .push(filled(None, Transform::identity(), 1.0, 1.0));
    scene.nodes.push(filled(
        None,
        Transform::translate(Point::new(3.0, 3.0)),
        1.0,
        1.0,
    ));
    let root = node_bounds(&scene, &NodePath::new()).expect("no error");
    assert_eq!(root, Rect::new(Point::new(0.0, 0.0), Point::new(4.0, 4.0)));
    assert_eq!(root, scene_bounds(&scene).expect("no error"));
}

#[test]
fn stale_node_paths_are_caller_errors() {
    let scene = Scene::new(8.5, 11.0);
    assert_eq!(
        node_bounds(&scene, &NodePath::new().child(0)),
        Err(larrygram_render::Error::InvalidNode)
    );
    assert_eq!(
        node_bounds(&scene, &NodePath::new().child(0).child(2)),
        Err(larrygram_render::Error::InvalidNode)
    );
}

#[test]
fn a_shape_node_has_no_children() {
    let mut scene = Scene::new(8.5, 11.0);
    scene
        .nodes
        .push(filled(None, Transform::identity(), 1.0, 1.0));
    assert_eq!(
        node_bounds(&scene, &NodePath::new().child(0).child(1)),
        Err(larrygram_render::Error::InvalidNode)
    );
}

#[test]
fn group_bounds_cover_the_members() {
    let mut scene = Scene::new(8.5, 11.0);
    let member = filled(None, Transform::translate(Point::new(0.0, 3.0)), 1.0, 1.0);
    scene
        .nodes
        .push(Node::Group(larrygram_render::scene::Group {
            transform: Transform::translate(Point::new(2.0, 0.0)),
            clip: None,
            children: vec![member],
        }));
    assert_eq!(
        scene_bounds(&scene).expect("no error"),
        Rect::new(Point::new(2.0, 3.0), Point::new(3.0, 4.0))
    );
}

#[test]
fn text_bounds_include_the_block() {
    let mut scene = Scene::new(8.5, 11.0);
    scene.nodes.push(shape_node(
        None,
        Transform::identity(),
        Path::new(),
        false,
        Some(text_block(Point::new(0.0, 1.0), 1.0, 1.0)),
    ));
    let bounds = node_bounds(&scene, &NodePath::new().child(0)).expect("no error");
    assert_eq!(
        bounds,
        Rect::new(Point::new(0.0, 0.0), Point::new(1.0, 1.0))
    );
    let hits = hit_marquee(
        &scene,
        Rect::new(Point::new(0.5, 0.5), Point::new(2.0, 2.0)),
    )
    .expect("no error");
    assert_eq!(hits.len(), 1, "the text block intersects the marquee");
}

#[test]
fn curve_paths_map_through_the_transform() {
    let mut path = Path::new();
    path.push(Segment::MoveTo(Point::new(0.0, 0.0)));
    path.push(Segment::QuadTo {
        ctrl: Point::new(0.5, 1.0),
        to: Point::new(1.0, 0.0),
    });
    path.push(Segment::CubicTo {
        ctrl1: Point::new(1.0, 1.0),
        ctrl2: Point::new(2.0, 1.0),
        to: Point::new(2.0, 0.0),
    });
    let mut scene = Scene::new(8.5, 11.0);
    scene.nodes.push(shape_node(
        None,
        Transform::translate(Point::new(10.0, 0.0)),
        path,
        false,
        None,
    ));
    // The quadratic passes through (10.5, 0.5) after the translation.
    let hits = hit_test(&scene, Point::new(10.5, 0.5), &HitOptions::new(0.01)).expect("no error");
    assert_eq!(shape(&hits), vec![(vec![0], None, HitKind::Stroke)]);
    // The bounds cover every mapped control point.
    let bounds = node_bounds(&scene, &NodePath::new().child(0)).expect("no error");
    assert_eq!(
        bounds,
        Rect::new(Point::new(10.0, 0.0), Point::new(12.0, 1.0))
    );
}

#[test]
fn the_scene_walk_depth_is_bounded() {
    // A shape nested deeper than the walk cap is never visited; the
    // walk degrades instead of overflowing the stack.
    let deep = |groups: usize| {
        let mut node = filled(None, Transform::identity(), 1.0, 1.0);
        for _ in 0..groups {
            node = Node::Group(larrygram_render::scene::Group {
                transform: Transform::identity(),
                clip: None,
                children: vec![node],
            });
        }
        let mut scene = Scene::new(8.5, 11.0);
        scene.nodes.push(node);
        scene
    };
    let hits =
        hit_test(&deep(127), Point::new(0.5, 0.5), &HitOptions::default()).expect("no error");
    assert_eq!(hits.len(), 1, "a shape at the depth cap still hits");
    let hits =
        hit_test(&deep(200), Point::new(0.5, 0.5), &HitOptions::default()).expect("no error");
    assert!(hits.is_empty(), "a shape past the depth cap is skipped");
}

#[test]
fn empty_scenes_measure_the_zero_rectangle() {
    let scene = Scene::new(8.5, 11.0);
    assert_eq!(
        scene_bounds(&scene).expect("no error"),
        Rect::new(Point::new(0.0, 0.0), Point::new(0.0, 0.0))
    );
    assert_eq!(
        node_bounds(&scene, &NodePath::new()).expect("no error"),
        Rect::new(Point::new(0.0, 0.0), Point::new(0.0, 0.0))
    );
}

// --- degenerate input ----------------------------------------------

#[test]
fn degenerate_paths_never_panic() {
    let mut nan_path = Path::new();
    nan_path.push(Segment::MoveTo(Point::new(f64::NAN, 0.0)));
    nan_path.push(Segment::LineTo(Point::new(1.0, 1.0)));
    nan_path.push(Segment::Close);
    let mut scene = Scene::new(8.5, 11.0);
    scene.nodes.push(shape_node(
        None,
        Transform::identity(),
        nan_path,
        true,
        None,
    ));
    // A degenerate path: no hit anywhere and sane bounds.
    let hits = hit_test(&scene, Point::new(0.5, 0.5), &HitOptions::new(1.0)).expect("no error");
    assert!(hits.is_empty(), "{hits:?}");
    let bounds = node_bounds(&scene, &NodePath::new().child(0)).expect("no error");
    assert_eq!(
        bounds,
        Rect::new(Point::new(1.0, 1.0), Point::new(1.0, 1.0))
    );
    let hits = hit_marquee(
        &scene,
        Rect::new(Point::new(0.0, 0.0), Point::new(2.0, 2.0)),
    )
    .expect("no error");
    assert_eq!(hits.len(), 1, "the finite corner still has bounds");
}

#[test]
fn a_degenerate_outline_hits_its_point() {
    let mut point_path = Path::new();
    point_path.push(Segment::MoveTo(Point::new(0.5, 0.5)));
    point_path.push(Segment::Close);
    let mut scene = Scene::new(8.5, 11.0);
    scene.nodes.push(shape_node(
        None,
        Transform::identity(),
        point_path,
        false,
        None,
    ));
    let hits = hit_test(&scene, Point::new(0.5, 0.5), &HitOptions::default()).expect("no error");
    assert_eq!(shape(&hits), vec![(vec![0], None, HitKind::Stroke)]);
    let hits = hit_test(&scene, Point::new(0.6, 0.5), &HitOptions::default()).expect("no error");
    assert!(hits.is_empty(), "{hits:?}");
}

#[test]
fn huge_coordinates_degrade_instead_of_panicking() {
    let huge = 1.0e308;
    let mut scene = Scene::new(8.5, 11.0);
    scene.nodes.push(filled(
        None,
        Transform::translate(Point::new(huge, huge)),
        2.0,
        2.0,
    ));
    let hits = hit_test(&scene, Point::new(0.0, 0.0), &HitOptions::new(1.0)).expect("no error");
    assert!(hits.is_empty(), "{hits:?}");
    let bounds = node_bounds(&scene, &NodePath::new().child(0)).expect("no error");
    assert!(bounds.min.x.is_finite() || bounds.min.x == huge);
    // A non-finite query or marquee rect degrades to no hits.
    let hits = hit_test(
        &scene,
        Point::new(f64::NAN, 0.0),
        &HitOptions::new(f64::NAN),
    )
    .expect("no error");
    assert!(hits.is_empty(), "{hits:?}");
    let hits = hit_marquee(
        &scene,
        Rect::new(Point::new(f64::NAN, 0.0), Point::new(1.0, 1.0)),
    )
    .expect("no error");
    assert!(hits.is_empty(), "{hits:?}");
}

#[test]
fn a_non_finite_transform_degrades_to_nothing() {
    let mut scene = Scene::new(8.5, 11.0);
    let transform = Transform {
        m11: f64::NAN,
        ..Transform::identity()
    };
    scene.nodes.push(filled(None, transform, 1.0, 1.0));
    let hits = hit_test(&scene, Point::new(0.5, 0.5), &HitOptions::new(1.0)).expect("no error");
    assert!(hits.is_empty(), "{hits:?}");
    let _ = node_bounds(&scene, &NodePath::new().child(0)).expect("no error");
}

// --- scene identity (the builder) ----------------------------------

/// A one-page document with one shape drawn as a unit square.
fn document_with_square() -> (Document, PageId, ShapeId) {
    let mut doc = Document::new();
    let shape = doc.add_shape(ModelShape::new(ShapeId { index: 0 }, 1));
    let page = PageId { index: 0 };
    let mut entity = Page::new(page, 1);
    entity.shapes.push(shape);
    doc.pages.push(entity);
    let mut section = Section::new(SectionName::new("Geometry"));
    let row = |row_type: &str, x: f64, y: f64| {
        let mut row = Row::new();
        row.row_type = Some(row_type.to_owned());
        for (name, value) in [("X", x), ("Y", y)] {
            let mut cell = Cell::new(CellName::new(name));
            cell.value = Some(CellValue::Number(value));
            row.cells.push(cell);
        }
        row
    };
    section.rows = vec![
        row("MoveTo", 0.0, 0.0),
        row("LineTo", 1.0, 0.0),
        row("LineTo", 1.0, 1.0),
        row("LineTo", 0.0, 1.0),
    ];
    if let Some(entity) = doc.shape_mut(shape) {
        entity.sheet.sections.push(section);
    }
    (doc, page, shape)
}

#[test]
fn the_builder_records_the_model_shape_id() {
    let (doc, page, shape) = document_with_square();
    let scene = build_scene(&doc, page, &BuildOptions::new()).expect("builds");
    match scene.nodes.first() {
        Some(Node::Shape(node)) => assert_eq!(node.shape_id, Some(shape)),
        other => panic!("expected a shape node, got {other:?}"),
    }
    // The hit maps back to the same model shape: just outside the
    // bottom edge of the node's bounds (the builder's placement of the
    // shape does not matter here).
    let bounds = node_bounds(&scene, &NodePath::new().child(0)).expect("no error");
    let point = Point::new((bounds.min.x + bounds.max.x) / 2.0, bounds.min.y - 0.005);
    let hits = hit_test(&scene, point, &HitOptions::new(0.01)).expect("no error");
    assert_eq!(hits.first().map(|hit| hit.shape_id), Some(Some(shape)));
}

#[test]
fn a_node_without_a_model_shape_keeps_none() {
    let mut scene = Scene::new(8.5, 11.0);
    scene
        .nodes
        .push(filled(None, Transform::identity(), 1.0, 1.0));
    let hits = hit_test(&scene, Point::new(0.5, 0.5), &HitOptions::default()).expect("no error");
    assert_eq!(hits.first().map(|hit| hit.shape_id), Some(None));
}

// --- the hit-test fallback -----------------------------------------

/// A one-page document with one shape whose only `Geometry` row is a row
/// kind the builder does not map (`GeometryRow::Unknown`): the shape
/// draws no path at all.
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)] // test helper
fn document_with_unmapped_row() -> (Document, PageId, ShapeId) {
    let mut doc = Document::new();
    let shape = doc.add_shape(ModelShape::new(ShapeId { index: 0 }, 1));
    let page = PageId { index: 0 };
    let mut entity = Page::new(page, 1);
    entity.shapes.push(shape);
    doc.pages.push(entity);
    let mut section = Section::new(SectionName::new("Geometry"));
    let mut row = Row::new();
    row.row_type = Some("Mystery".to_owned());
    for (name, value) in [("X", 1.0), ("Y", 1.0)] {
        let mut cell = Cell::new(CellName::new(name));
        cell.value = Some(CellValue::Number(value));
        row.cells.push(cell);
    }
    section.rows.push(row);
    doc.shape_mut(shape)
        .expect("shape")
        .sheet
        .sections
        .push(section);
    (doc, page, shape)
}

#[test]
fn an_unmapped_geometry_row_kind_falls_back_to_the_origin_bounds() {
    // The builder maps the row to `GeometryRow::Unknown` and warns: the
    // shape draws no path at all and (with no text either) drops out of
    // the scene (`build`'s "no drawable content" rule). Hit testing then
    // has nothing to match: no hits, never an error and never a panic
    // (`hit_test`'s degenerate-path contract), at any tolerance.
    let (doc, page, _shape) = document_with_unmapped_row();
    let scene = build_scene(&doc, page, &BuildOptions::new()).expect("builds");
    assert!(
        scene.nodes.is_empty(),
        "the unmapped row kind draws no node: {:?}",
        scene.nodes
    );
    let hits = hit_test(&scene, Point::new(1.0, 1.0), &HitOptions::new(10.0)).expect("no error");
    assert!(hits.is_empty(), "{hits:?}");
    assert_eq!(
        scene_bounds(&scene).expect("no error"),
        Rect::new(Point::new(0.0, 0.0), Point::new(0.0, 0.0)),
        "a scene with nothing drawable measures the origin"
    );

    // The same fallback inside `render/src/hit.rs`: a node whose path has
    // no measurable points (the `path_bounds` catch-all's "no points to
    // measure" case) measures the zero rectangle at its transformed
    // origin and never intersects a marquee.
    let mut scene = Scene::new(8.5, 11.0);
    scene.nodes.push(shape_node(
        Some(ShapeId { index: 3 }),
        Transform::identity(),
        Path::new(),
        true,
        None,
    ));
    let hits = hit_test(&scene, Point::new(1.0, 1.0), &HitOptions::new(10.0)).expect("no error");
    assert!(hits.is_empty(), "{hits:?}");
    let bounds = node_bounds(&scene, &NodePath::new().child(0)).expect("no error");
    assert_eq!(
        bounds,
        Rect::new(Point::new(0.0, 0.0), Point::new(0.0, 0.0)),
        "a node that draws nothing measures its origin"
    );
    let hits = hit_marquee(
        &scene,
        Rect::new(Point::new(-1.0, -1.0), Point::new(3.0, 3.0)),
    )
    .expect("no error");
    assert!(hits.is_empty(), "{hits:?}");
}
