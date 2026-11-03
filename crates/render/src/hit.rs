//! Hit-testing against the scene graph (ADR 0041).
//!
//! The scene is in page inches (ADR 0020); the caller converts the
//! tolerance from screen pixels (`pixels / (zoom * 96)`). Results come
//! topmost first: the caller picks the first hit it can select. Filled
//! shapes are hit on their fill, unfilled shapes on their outline and
//! text; groups are not hit directly (their leaf shapes are, and the
//! caller maps to the top-level ancestor through the model).
//!
//! The geometry math goes through `larrygram_geom` (L0): paths are mapped
//! into page space through the node's ancestor transforms and measured
//! there, so `distance` is always in page inches even under a scaled or
//! rotated transform. Curve outlines flatten within
//! `larrygram_geom::FLATTEN_TOLERANCE` (ADR 0022 flattening).

use larrygram_geom::{Path, Point, Rect, Segment, Transform, distance_point_path, path_contains};
use larrygram_model::ShapeId;

use crate::error::Error;
use crate::scene::{Node, Scene, Shape, TextNode};

/// The deepest scene nesting the walk enters (the scene builder's depth
/// cap, ADR 0020): bounded recursion, never a stack overflow.
const MAX_DEPTH: usize = 128;

/// The path of one node in the scene tree: the child index at each level
/// (`[0, 2]` = the third child of the first top-level node).
#[derive(Debug, Clone, PartialEq, Eq, Default, Hash)]
pub struct NodePath(pub Vec<usize>);

impl NodePath {
    /// The path of the scene root (the union of the top-level nodes).
    pub fn new() -> Self {
        Self(Vec::new())
    }

    /// The path one level deeper, through child `index`.
    pub fn child(&self, index: usize) -> NodePath {
        let mut indices = self.0.clone();
        indices.push(index);
        NodePath(indices)
    }

    /// The indices, outermost first.
    pub fn indices(&self) -> &[usize] {
        &self.0
    }
}

/// What part of a shape was hit (ADR 0041).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum HitKind {
    /// Inside the fill of a filled shape.
    Fill,
    /// Within tolerance of the stroked outline.
    Stroke,
    /// On the text block.
    Text,
    /// On the shape's bounds only (the marquee case).
    Bounds,
}

/// One hit-test result (ADR 0041).
#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    /// The hit node's path in the scene tree.
    pub node: NodePath,
    /// The model shape behind the node, when it has one.
    pub shape_id: Option<ShapeId>,
    /// What part was hit.
    pub kind: HitKind,
    /// Distance from the query point to the hit part, in inches (0 inside
    /// a fill or a text block). Results come topmost first (z-order,
    /// ADR 0041); the distance is what the caller's tolerance matched.
    pub distance: f64,
}

/// Options for [`hit_test`] (ADR 0041).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct HitOptions {
    /// The outline tolerance, in inches (converted from screen pixels by
    /// the caller).
    pub tolerance: f64,
}

impl HitOptions {
    /// Options with the given tolerance, in inches.
    pub const fn new(tolerance: f64) -> Self {
        Self { tolerance }
    }
}

/// Every shape hit by `point`, topmost first.
///
/// A filled shape is hit when the point is inside its fill or within
/// `tolerance` of its outline; an unfilled shape only on the outline and
/// its text (ADR 0041). The text block bounds count as hit
/// ([`HitKind::Text`]). One hit per shape node: the strongest part wins
/// (`Fill`, then `Stroke`, then `Text`). Groups are not hit; their shape
/// nodes are. A hit on a member of a group reports the member node; the
/// caller maps it to the top-level ancestor (ADR 0041).
///
/// A `tolerance` that is `NaN` or negative is treated as `0`. Empty
/// scenes and degenerate paths yield no hits, never an error and never a
/// panic.
pub fn hit_test(scene: &Scene, point: Point, options: &HitOptions) -> Result<Vec<Hit>, Error> {
    let tolerance = clean_tolerance(options.tolerance);
    let mut hits = Vec::new();
    walk(scene, &mut |node, shape, transform| {
        if let Some(hit) = shape_hit(node, shape, transform, point, tolerance) {
            hits.push(hit);
        }
    });
    // The walk is in paint order (later nodes on top): reverse for
    // topmost first (ADR 0041).
    hits.reverse();
    Ok(hits)
}

/// Every shape whose transformed bounds intersect `rect`, topmost first
/// (the marquee rule of ADR 0041: intersection, not containment).
///
/// The result carries one [`HitKind::Bounds`] hit per intersecting shape
/// node, with `distance` 0. A shape with no drawable content (no path
/// points and no text) never intersects. A rectangle with its corners
/// swapped is normalized; a non-finite rectangle intersects nothing.
pub fn hit_marquee(scene: &Scene, rect: Rect) -> Result<Vec<Hit>, Error> {
    let Some(rect) = normalized(rect) else {
        return Ok(Vec::new());
    };
    let mut hits = Vec::new();
    walk(scene, &mut |node, shape, transform| {
        if let Some(bounds) = shape_bounds(shape, transform)
            && intersects(bounds, rect)
        {
            hits.push(Hit {
                node: node.clone(),
                shape_id: shape.shape_id,
                kind: HitKind::Bounds,
                distance: 0.0,
            });
        }
    });
    hits.reverse();
    Ok(hits)
}

/// The transformed bounding box of one node, in page inches (handles and
/// overlays, ADR 0042).
///
/// The empty [`NodePath`] names the scene root: its bounds are the union
/// of the top-level nodes (the same as [`scene_bounds`]). A node that
/// draws nothing measures the zero rectangle at its transformed origin.
/// A node path that does not resolve is [`Error::InvalidNode`]: the
/// caller passes paths it got from [`hit_test`] or [`hit_marquee`], so a
/// stale path is a caller bug (the same standing as `Error::Model`).
pub fn node_bounds(scene: &Scene, node: &NodePath) -> Result<Rect, Error> {
    let placement = placement(scene, node)?;
    Ok(placement_bounds(&placement))
}

/// The bounds of every node of the scene, in page inches (`view.zoom.fit`
/// and selection-all). An empty scene measures the zero rectangle at the
/// origin.
pub fn scene_bounds(scene: &Scene) -> Result<Rect, Error> {
    let mut bounds: Option<Rect> = None;
    for node in &scene.nodes {
        if let Some(node) = bounds_in(node, Transform::identity()) {
            bounds = Some(union(bounds, node));
        }
    }
    Ok(bounds.unwrap_or_else(|| Rect::new(Point::new(0.0, 0.0), Point::new(0.0, 0.0))))
}

// --- scene walk ------------------------------------------------------

/// Visits every shape node in paint order, with its page transform and
/// node path. Recursion is bounded by [`MAX_DEPTH`].
fn walk<'a>(scene: &'a Scene, visit: &mut impl FnMut(&NodePath, &'a Shape, Transform)) {
    walk_nodes(
        &scene.nodes,
        &NodePath::new(),
        Transform::identity(),
        0,
        visit,
    );
}

/// The recursion of [`walk`]: `parent` maps the nodes' own space to the
/// page.
fn walk_nodes<'a>(
    nodes: &'a [Node],
    path: &NodePath,
    parent: Transform,
    depth: usize,
    visit: &mut impl FnMut(&NodePath, &'a Shape, Transform),
) {
    if depth >= MAX_DEPTH {
        return;
    }
    for (index, node) in nodes.iter().enumerate() {
        let child = path.child(index);
        match node {
            Node::Group(group) => {
                walk_nodes(
                    &group.children,
                    &child,
                    group.transform.then(parent),
                    depth + 1,
                    visit,
                );
            }
            Node::Shape(shape) => visit(&child, shape, shape.transform.then(parent)),
        }
    }
}

/// The hit on one shape node, or `None` (see [`hit_test`] for the
/// rules).
fn shape_hit(
    node: &NodePath,
    shape: &Shape,
    transform: Transform,
    point: Point,
    tolerance: f64,
) -> Option<Hit> {
    let path = mapped_path(&shape.path, transform);
    let outline = distance_point_path(point, &path).unwrap_or(f64::INFINITY);
    let hit = |kind, distance| {
        Some(Hit {
            node: node.clone(),
            shape_id: shape.shape_id,
            kind,
            distance,
        })
    };
    if shape.fill.is_some() {
        if path_contains(&path, point).unwrap_or(false) {
            return hit(HitKind::Fill, 0.0);
        }
        if outline <= tolerance {
            return hit(HitKind::Stroke, outline);
        }
    } else if outline <= tolerance {
        return hit(HitKind::Stroke, outline);
    }
    if let Some(text) = &shape.text
        && let Some(quad) = text_quad(text)
    {
        let quad = quad_path(quad);
        let quad = mapped_path(&quad, transform);
        if path_contains(&quad, point).unwrap_or(false) {
            return hit(HitKind::Text, 0.0);
        }
    }
    None
}

// --- bounds ----------------------------------------------------------

/// A node's placement: its own-space to page transform and its bounds in
/// its own space.
pub(crate) struct Placement {
    /// The transform from the node's own space to the page (the full
    /// ancestor chain, the node's own transform included).
    pub transform: Transform,
    /// The bounds in the node's own space, or `None` when the node draws
    /// nothing (no path points and no text).
    pub local: Option<Rect>,
}

/// The placement of one node (the empty path is the scene root).
pub(crate) fn placement(scene: &Scene, node: &NodePath) -> Result<Placement, Error> {
    resolve(&scene.nodes, node.indices(), Transform::identity())
}

/// The bounds of a placement in page inches (see [`node_bounds`]).
pub(crate) fn placement_bounds(placement: &Placement) -> Rect {
    match placement.local {
        Some(local) => {
            let mapped = rect_bounds(local, placement.transform);
            mapped.unwrap_or_else(|| origin_bounds(placement.transform))
        }
        None => origin_bounds(placement.transform),
    }
}

/// The zero rectangle at the transformed origin, at `(0, 0)` when even
/// that is not finite (a non-finite transform degrades, never panics).
fn origin_bounds(transform: Transform) -> Rect {
    let origin = transform.apply(Point::new(0.0, 0.0));
    if finite(origin) {
        point_bounds(origin)
    } else {
        point_bounds(Point::new(0.0, 0.0))
    }
}

/// The oriented bounds of a placement: the node's own bounds rectangle
/// positioned at its transformed centre and turned by the transform's
/// rotation (`overlay::build_selection_overlay`). The returned point is
/// the rectangle's centre, the angle its rotation in radians.
pub(crate) fn oriented_bounds(placement: &Placement) -> (Rect, f64) {
    let Some(local) = placement.local else {
        return (placement_bounds(placement), 0.0);
    };
    let transform = placement.transform;
    let centre_local = Point::new(
        (local.min.x + local.max.x) / 2.0,
        (local.min.y + local.max.y) / 2.0,
    );
    let centre = transform.apply(centre_local);
    let angle = transform.m21.atan2(transform.m11);
    let width = transform.m11.hypot(transform.m21) * local.width();
    let height = transform.m12.hypot(transform.m22) * local.height();
    if !finite(centre) || !angle.is_finite() || !width.is_finite() || !height.is_finite() {
        return (placement_bounds(placement), 0.0);
    }
    let bounds = Rect::new(
        Point::new(centre.x - width / 2.0, centre.y - height / 2.0),
        Point::new(centre.x + width / 2.0, centre.y + height / 2.0),
    );
    (bounds, angle)
}

/// Resolves `indices` into `nodes` (the accumulated `parent` maps the
/// nodes' own space to the page).
fn resolve(nodes: &[Node], indices: &[usize], parent: Transform) -> Result<Placement, Error> {
    let Some((&first, rest)) = indices.split_first() else {
        // The empty path names the scene root.
        let mut local = None;
        for node in nodes {
            if let Some(bounds) = bounds_in(node, Transform::identity()) {
                local = Some(union(local, bounds));
            }
        }
        return Ok(Placement {
            transform: parent,
            local,
        });
    };
    let Some(node) = nodes.get(first) else {
        return Err(Error::InvalidNode);
    };
    match node {
        Node::Group(group) => {
            let transform = group.transform.then(parent);
            if rest.is_empty() {
                let mut local = None;
                for child in &group.children {
                    if let Some(bounds) = bounds_in(child, Transform::identity()) {
                        local = Some(union(local, bounds));
                    }
                }
                Ok(Placement { transform, local })
            } else {
                resolve(&group.children, rest, transform)
            }
        }
        Node::Shape(shape) => {
            if !rest.is_empty() {
                // A shape node has no children (a caller bug, as above).
                return Err(Error::InvalidNode);
            }
            Ok(Placement {
                transform: shape.transform.then(parent),
                local: local_bounds(shape),
            })
        }
    }
}

/// The bounds of one node in the space `parent` maps to the page (the
/// node's own transform included), or `None` when it draws nothing.
fn bounds_in(node: &Node, parent: Transform) -> Option<Rect> {
    match node {
        Node::Shape(shape) => {
            let local = local_bounds(shape)?;
            rect_bounds(local, shape.transform.then(parent))
        }
        Node::Group(group) => {
            let transform = group.transform.then(parent);
            let mut bounds: Option<Rect> = None;
            for child in &group.children {
                if let Some(child) = bounds_in(child, transform) {
                    bounds = Some(union(bounds, child));
                }
            }
            bounds
        }
    }
}

/// The bounds of one shape in its own space: its path and text block.
fn local_bounds(shape: &Shape) -> Option<Rect> {
    let mut bounds = path_bounds(&shape.path);
    if let Some(text) = &shape.text
        && let Some(quad) = text_quad(text)
        && let Some(text) = path_bounds(&quad_path(quad))
    {
        bounds = Some(union(bounds, text));
    }
    bounds
}

/// The bounds of one shape node in page inches.
fn shape_bounds(shape: &Shape, transform: Transform) -> Option<Rect> {
    let local = local_bounds(shape)?;
    rect_bounds(local, transform)
}

/// Like [`larrygram_geom::Path::bounds`], but non-finite points are
/// skipped (a degenerate path degrades instead of poisoning the box).
fn path_bounds(path: &Path) -> Option<Rect> {
    let mut bounds: Option<Rect> = None;
    for segment in path.segments() {
        let (points, count): ([Point; 3], usize) = match segment {
            Segment::MoveTo(p) | Segment::LineTo(p) => ([*p, *p, *p], 1),
            Segment::QuadTo { ctrl, to } => ([*ctrl, *to, *to], 2),
            Segment::CubicTo { ctrl1, ctrl2, to } => ([*ctrl1, *ctrl2, *to], 3),
            Segment::Close => (
                [
                    Point::new(0.0, 0.0),
                    Point::new(0.0, 0.0),
                    Point::new(0.0, 0.0),
                ],
                0,
            ),
            // A future `Segment` variant (the enum is non-exhaustive):
            // no points to measure.
            _ => (
                [
                    Point::new(0.0, 0.0),
                    Point::new(0.0, 0.0),
                    Point::new(0.0, 0.0),
                ],
                0,
            ),
        };
        for point in points.iter().take(count) {
            if finite(*point) {
                bounds = Some(union(bounds, Rect::new(*point, *point)));
            }
        }
    }
    bounds
}

/// The text block's corners in shape-local coordinates (see
/// [`text_quad`]), as a closed path.
fn quad_path(points: [Point; 4]) -> Path {
    let mut path = Path::new();
    path.push(Segment::MoveTo(points[0]));
    for point in points.iter().skip(1) {
        path.push(Segment::LineTo(*point));
    }
    path.push(Segment::Close);
    path
}

/// The text block rectangle as four corners in shape-local coordinates:
/// the block box `(0, 0)`..`(width, height)` of the layout, turned by the
/// layout rotation about the box centre and placed at
/// [`TextNode::origin`] (`origin + (x, -y)`, the module docs of
/// `scene`). `None` when the layout has no box (zero size) or a value is
/// not finite.
fn text_quad(text: &TextNode) -> Option<[Point; 4]> {
    let (width, height) = (text.block.width, text.block.height);
    if !finite(text.origin) || !width.is_finite() || !height.is_finite() {
        return None;
    }
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    let (sin, cos) = text.layout.rotation.radians().sin_cos();
    if !sin.is_finite() || !cos.is_finite() {
        return None;
    }
    let (cx, cy) = (width / 2.0, height / 2.0);
    let corners = [
        Point::new(0.0, 0.0),
        Point::new(width, 0.0),
        Point::new(width, height),
        Point::new(0.0, height),
    ];
    let mut out = [Point::new(0.0, 0.0); 4];
    for (index, corner) in corners.iter().enumerate() {
        let (dx, dy) = (corner.x - cx, corner.y - cy);
        let x = cx + dx * cos - dy * sin;
        let y = cy + dx * sin + dy * cos;
        if let Some(slot) = out.get_mut(index) {
            *slot = Point::new(text.origin.x + x, text.origin.y - y);
        }
    }
    Some(out)
}

/// The axis-aligned bounds of the rectangle's corners under `transform`.
fn rect_bounds(rect: Rect, transform: Transform) -> Option<Rect> {
    let corners = [
        rect.min,
        Point::new(rect.max.x, rect.min.y),
        rect.max,
        Point::new(rect.min.x, rect.max.y),
    ];
    let mut bounds: Option<Rect> = None;
    for corner in corners {
        let mapped = transform.apply(corner);
        if !finite(mapped) {
            return None;
        }
        bounds = Some(union(bounds, Rect::new(mapped, mapped)));
    }
    bounds
}

/// The zero rectangle at one point.
fn point_bounds(point: Point) -> Rect {
    Rect::new(point, point)
}

/// The smallest rectangle containing both (a `None` is the empty union).
fn union(a: Option<Rect>, b: Rect) -> Rect {
    match a {
        Some(a) => a.union(b),
        None => b,
    }
}

/// The rectangle of two arbitrary corners (the bounding-box rule of
/// [`rect_bounds`] for the marquee input), or `None` when a corner is not
/// finite.
fn normalized(rect: Rect) -> Option<Rect> {
    if !finite(rect.min) || !finite(rect.max) {
        return None;
    }
    let min = Point::new(rect.min.x.min(rect.max.x), rect.min.y.min(rect.max.y));
    let max = Point::new(rect.min.x.max(rect.max.x), rect.min.y.max(rect.max.y));
    Some(Rect::new(min, max))
}

/// Whether two rectangles share at least one point (edges included).
fn intersects(a: Rect, b: Rect) -> bool {
    a.min.x <= b.max.x && a.max.x >= b.min.x && a.min.y <= b.max.y && a.max.y >= b.min.y
}

/// The path with every point mapped through `transform` (affine images of
/// curve control points are the control points of the image curve).
fn mapped_path(path: &Path, transform: Transform) -> Path {
    let map = |p: Point| transform.apply(p);
    let mut out = Path::new();
    for segment in path.segments() {
        out.push(match segment {
            Segment::MoveTo(p) => Segment::MoveTo(map(*p)),
            Segment::LineTo(p) => Segment::LineTo(map(*p)),
            Segment::QuadTo { ctrl, to } => Segment::QuadTo {
                ctrl: map(*ctrl),
                to: map(*to),
            },
            Segment::CubicTo { ctrl1, ctrl2, to } => Segment::CubicTo {
                ctrl1: map(*ctrl1),
                ctrl2: map(*ctrl2),
                to: map(*to),
            },
            Segment::Close => Segment::Close,
            // A future `Segment` variant (the enum is non-exhaustive):
            // dropped, as it draws nothing known.
            _ => continue,
        });
    }
    out
}

/// True when both coordinates are finite.
fn finite(point: Point) -> bool {
    point.x.is_finite() && point.y.is_finite()
}

/// The tolerance to hit with: a `NaN` or negative request is `0`.
fn clean_tolerance(tolerance: f64) -> f64 {
    if tolerance.is_nan() || tolerance < 0.0 {
        0.0
    } else {
        tolerance
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_paths_nest() {
        let root = NodePath::new();
        assert!(root.indices().is_empty());
        let child = root.child(2);
        assert_eq!(child.indices(), &[2]);
        assert_eq!(child.child(0).indices(), &[2, 0]);
    }

    #[test]
    fn hits_hold_their_data() {
        let hit = Hit {
            node: NodePath::new(),
            shape_id: Some(ShapeId { index: 3 }),
            kind: HitKind::Stroke,
            distance: 0.01,
        };
        assert_eq!(hit.shape_id, Some(ShapeId { index: 3 }));
        assert_eq!(hit.kind, HitKind::Stroke);
        assert_eq!(HitOptions::new(0.02).tolerance, 0.02);
        assert_eq!(HitOptions::default().tolerance, 0.0);
    }

    #[test]
    fn empty_scenes_have_nothing_to_hit() {
        let scene = Scene::new(8.5, 11.0);
        let point = Point::new(1.0, 1.0);
        let hits = hit_test(&scene, point, &HitOptions::default()).expect("no error");
        assert!(hits.is_empty());
        let marquee =
            hit_marquee(&scene, Rect::new(point, Point::new(2.0, 2.0))).expect("no error");
        assert!(marquee.is_empty());
        let bounds = scene_bounds(&scene).expect("no error");
        assert_eq!(
            bounds,
            Rect::new(Point::new(0.0, 0.0), Point::new(0.0, 0.0))
        );
    }

    #[test]
    fn a_stale_node_path_is_a_caller_bug() {
        let scene = Scene::new(1.0, 1.0);
        assert_eq!(
            node_bounds(&scene, &NodePath::new().child(3)),
            Err(Error::InvalidNode)
        );
    }

    #[test]
    fn tolerance_cleanup_is_deterministic() {
        assert_eq!(clean_tolerance(f64::NAN), 0.0);
        assert_eq!(clean_tolerance(-1.0), 0.0);
        assert_eq!(clean_tolerance(0.5), 0.5);
        assert_eq!(clean_tolerance(f64::INFINITY), f64::INFINITY);
    }

    #[test]
    fn text_quads_follow_the_layout_placement() {
        let text = TextNode {
            fonts: Vec::new(),
            runs: Vec::new(),
            layout: larrygram_text::Layout {
                width: 2.0,
                height: 1.0,
                ..larrygram_text::Layout::default()
            },
            color: crate::scene::Rgba::opaque(0, 0, 0),
            block: larrygram_geom::Size::new(2.0, 1.0),
            origin: Point::new(1.0, 3.0),
        };
        let quad = text_quad(&text).expect("a laid-out block");
        assert_eq!(quad[0], Point::new(1.0, 3.0));
        assert_eq!(quad[1], Point::new(3.0, 3.0));
        assert_eq!(quad[2], Point::new(3.0, 2.0));
        assert_eq!(quad[3], Point::new(1.0, 2.0));
    }

    #[test]
    fn text_quads_reject_bad_input() {
        let nan = f64::NAN;
        let base = TextNode {
            fonts: Vec::new(),
            runs: Vec::new(),
            layout: larrygram_text::Layout {
                width: 1.0,
                height: 1.0,
                ..larrygram_text::Layout::default()
            },
            color: crate::scene::Rgba::opaque(0, 0, 0),
            block: larrygram_geom::Size::new(1.0, 1.0),
            origin: Point::new(0.0, 0.0),
        };
        let moved = TextNode {
            origin: Point::new(nan, 0.0),
            ..base.clone()
        };
        assert_eq!(text_quad(&moved), None);
        let turned = TextNode {
            layout: larrygram_text::Layout {
                rotation: larrygram_geom::Angle::from_radians(nan),
                ..base.layout.clone()
            },
            ..base.clone()
        };
        assert_eq!(text_quad(&turned), None);
        let shrunk = TextNode {
            block: larrygram_geom::Size::new(1.0, 0.0),
            ..base
        };
        assert_eq!(text_quad(&shrunk), None);
    }
}
