//! Selection handles: move, resize and rotate (ADR 0042).
//!
//! Handles live at screen-size anchors around the selection bounds; the
//! caller converts the tolerance from screen pixels with the zoom. The
//! pointer tool ([`crate::tools::pointer`], P5-05) owns the drag that
//! follows a hit.
//!
//! The scene helpers here are shared by the pointer tool and the snapper:
//! one page scene build, one shape-to-node mapping and one selection
//! frame (the `render::build_selection_overlay` rules, ADR 0041/0042).

use std::collections::BTreeSet;

use larrygram_commands::AppCtx;
use larrygram_geom::{Angle, Point, Rect, Transform};
use larrygram_model::{Document, ShapeId};
use larrygram_render::{
    BuildOptions, NodePath, OverlayMark, Scene, build_scene, build_selection_overlay,
};

use crate::UiError;

/// Which edge or corner a resize handle stretches (ADR 0042).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ResizeEdge {
    /// The top edge.
    North,
    /// The bottom edge.
    South,
    /// The right edge.
    East,
    /// The left edge.
    West,
    /// The top-right corner.
    NorthEast,
    /// The top-left corner.
    NorthWest,
    /// The bottom-right corner.
    SouthEast,
    /// The bottom-left corner.
    SouthWest,
}

/// What a handle does (ADR 0042).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum HandleKind {
    /// Move the selection (the interior of the bounds).
    Move,
    /// Stretch one edge or corner.
    Resize(ResizeEdge),
    /// Rotate around the selection's pin (the handle above the bounds).
    Rotate,
}

/// The screen-pixel hit tolerance of a handle outside the selection
/// bounds (ADR 0042; task P7-19: a little more than the drawn square's
/// half-diagonal, so the whole square and a margin hit).
pub const TOLERANCE_PX: f64 = 7.0;

/// Half the edge of a drawn handle square, in screen pixels (egui
/// points). Inside the bounds a handle hits only on its square, so a
/// small shape keeps a move interior.
pub const HANDLE_HALF_PX: f64 = 4.0;

/// The rotate handle's distance above the bounds' top edge, in screen
/// pixels (task P7-19: a page offset put it on the north handle at 25%
/// zoom and an inch away at 400%).
pub const ROTATE_OFFSET_PX: f64 = 20.0;

/// One screen pixel in page inches at `zoom` (an invalid zoom counts as
/// 100%).
fn inches_per_pixel(zoom: f64) -> f64 {
    let zoom = if zoom.is_finite() && zoom > 0.0 {
        zoom
    } else {
        1.0
    };
    1.0 / (zoom * crate::canvas::POINTS_PER_INCH)
}

/// The handle at `point` for one selected shape (page inches, `zoom` for
/// the screen-pixel sizes of [`TOLERANCE_PX`], [`HANDLE_HALF_PX`] and
/// [`ROTATE_OFFSET_PX`]).
///
/// The nearest handle within reach wins over the move interior: a
/// handle hits on its drawn square, and outside the bounds within
/// [`TOLERANCE_PX`] as well. `None` when the point is on no handle and outside the
/// rotated bounds.
pub fn handle_at(
    point: Point,
    bounds: Rect,
    angle: Angle,
    zoom: f64,
) -> Result<Option<HandleKind>, UiError> {
    if !finite(point) || !finite_rect(bounds) || !angle.radians().is_finite() {
        return Ok(None);
    }
    let pixel = inches_per_pixel(zoom);
    let inside = rotated_contains(bounds, angle, point);
    let square = HANDLE_HALF_PX * pixel;
    let reach = TOLERANCE_PX * pixel;
    let mut best: Option<(HandleKind, f64)> = None;
    for (kind, anchor) in handles_for(bounds, angle, zoom)? {
        let delta = point.sub(anchor);
        let distance = delta.x.hypot(delta.y);
        // The drawn square (axis-aligned on screen) always hits; the
        // wider round reach applies outside the shape only.
        let on_square = delta.x.abs().max(delta.y.abs()) <= square;
        let hit = on_square || (!inside && distance <= reach);
        if hit && best.is_none_or(|(_, best)| distance < best) {
            best = Some((kind, distance));
        }
    }
    if let Some((kind, _)) = best {
        return Ok(Some(kind));
    }
    if inside {
        Ok(Some(HandleKind::Move))
    } else {
        Ok(None)
    }
}

/// Whether `point` lies inside `bounds` turned by `angle` about its
/// centre (task P7-19: the exact test; the axis-aligned envelope of a
/// turned shape counted its empty corners as the move interior).
fn rotated_contains(bounds: Rect, angle: Angle, point: Point) -> bool {
    let center = rect_center(bounds);
    let local = rotate_about(center, Angle::from_radians(-angle.radians()), point);
    local.x >= bounds.min.x
        && local.x <= bounds.max.x
        && local.y >= bounds.min.y
        && local.y <= bounds.max.y
}

/// The handle anchor points of one selection: `(kind, point)` pairs in
/// page inches, rotated with `angle` (ADR 0042). The rotate handle sits
/// [`ROTATE_OFFSET_PX`] screen pixels above the top edge at `zoom`.
pub fn handles_for(
    bounds: Rect,
    angle: Angle,
    zoom: f64,
) -> Result<Vec<(HandleKind, Point)>, UiError> {
    if !finite_rect(bounds) {
        return Ok(Vec::new());
    }
    let rotate_offset = ROTATE_OFFSET_PX * inches_per_pixel(zoom);
    let center = rect_center(bounds);
    let (min, max) = (bounds.min, bounds.max);
    let mid_x = (min.x + max.x) / 2.0;
    let mid_y = (min.y + max.y) / 2.0;
    let rotate = |point: Point| rotate_about(center, angle, point);
    let mut handles = Vec::with_capacity(9);
    for (kind, point) in [
        (
            HandleKind::Resize(ResizeEdge::NorthWest),
            Point::new(min.x, max.y),
        ),
        (
            HandleKind::Resize(ResizeEdge::North),
            Point::new(mid_x, max.y),
        ),
        (
            HandleKind::Resize(ResizeEdge::NorthEast),
            Point::new(max.x, max.y),
        ),
        (
            HandleKind::Resize(ResizeEdge::East),
            Point::new(max.x, mid_y),
        ),
        (
            HandleKind::Resize(ResizeEdge::SouthEast),
            Point::new(max.x, min.y),
        ),
        (
            HandleKind::Resize(ResizeEdge::South),
            Point::new(mid_x, min.y),
        ),
        (
            HandleKind::Resize(ResizeEdge::SouthWest),
            Point::new(min.x, min.y),
        ),
        (
            HandleKind::Resize(ResizeEdge::West),
            Point::new(min.x, mid_y),
        ),
        (HandleKind::Rotate, Point::new(mid_x, max.y + rotate_offset)),
    ] {
        handles.push((kind, rotate(point)));
    }
    Ok(handles)
}

/// The union bounds of the selection in page inches (one shape: its
/// rotated bounds; several: the axis-aligned union).
///
/// An empty selection or one with no drawable scene node is an
/// [`UiError::Io`] naming the state; callers that accept an empty
/// selection use the internal `selection_frame` helper.
pub fn selection_bounds(ctx: &AppCtx) -> Result<Rect, UiError> {
    match selection_frame(ctx, None)? {
        Some((bounds, _angle)) => Ok(bounds),
        None => Err(UiError::Io {
            message: "the selection has no bounds".to_owned(),
        }),
    }
}

/// The selection's handle frame: bounds and rotation.
///
/// One selected shape gives its own rotated bounds; several give the
/// axis-aligned union with a zero angle (the `build_selection_overlay`
/// rule). `None` when the selection is empty or nothing is drawable.
pub(crate) fn selection_frame(
    ctx: &AppCtx,
    scene: Option<&Scene>,
) -> Result<Option<(Rect, Angle)>, UiError> {
    // The caller's page scene when it has one (task P5-11: rebuilding
    // the scene per drag frame blew the ADR 0042 budget).
    let built;
    let scene: &Scene = match scene {
        Some(scene) => scene,
        None => {
            built = scene_for(ctx)?;
            match &built {
                Some(scene) => scene,
                None => return Ok(None),
            }
        }
    };
    let paths = paths_for_shapes(scene, ctx, &ctx.selection.shapes)?;
    if paths.is_empty() {
        return Ok(None);
    }
    let overlay = build_selection_overlay(scene, &paths).map_err(render_error)?;
    for mark in overlay.marks {
        if let OverlayMark::Handles { bounds, angle } = mark {
            return Ok(Some((bounds, angle)));
        }
    }
    Ok(None)
}

/// The active page's scene, or `None` with no open document or page.
pub(crate) fn scene_for(ctx: &AppCtx) -> Result<Option<Scene>, UiError> {
    let Some(session) = ctx.active_document() else {
        return Ok(None);
    };
    let Some(page) = session.active_page else {
        return Ok(None);
    };
    build_scene(&session.document, page, &BuildOptions::new())
        .map(Some)
        .map_err(render_error)
}

/// One drawable scene node: the model shape, its page transform and its
/// scene path.
#[derive(Debug, Clone)]
pub(crate) struct NodeEntry {
    /// The model shape behind the node.
    pub shape: ShapeId,
    /// The scene path.
    pub path: NodePath,
    /// The shape-local to page transform.
    pub transform: Transform,
}

/// Every drawable shape node of the scene, in paint order, with its page
/// transform.
pub(crate) fn node_entries(scene: &Scene) -> Vec<NodeEntry> {
    let mut entries = Vec::new();
    collect_entries(
        &scene.nodes,
        &NodePath::new(),
        Transform::identity(),
        0,
        &mut entries,
    );
    entries
}

/// The recursion of [`node_entries`] (depth-capped like the renderer).
fn collect_entries(
    nodes: &[larrygram_render::scene::Node],
    parent: &NodePath,
    transform: Transform,
    depth: usize,
    entries: &mut Vec<NodeEntry>,
) {
    const MAX_DEPTH: usize = 128;
    if depth >= MAX_DEPTH {
        return;
    }
    for (index, node) in nodes.iter().enumerate() {
        let path = parent.child(index);
        match node {
            larrygram_render::scene::Node::Group(group) => collect_entries(
                &group.children,
                &path,
                group.transform.then(transform),
                depth + 1,
                entries,
            ),
            larrygram_render::scene::Node::Shape(shape) => {
                if let Some(id) = shape.shape_id {
                    entries.push(NodeEntry {
                        shape: id,
                        path,
                        transform: shape.transform.then(transform),
                    });
                }
            }
            _ => {}
        }
    }
}

/// The scene paths of the selected shapes and their descendants.
pub(crate) fn paths_for_shapes(
    scene: &Scene,
    ctx: &AppCtx,
    shapes: &[ShapeId],
) -> Result<Vec<NodePath>, UiError> {
    let Some(session) = ctx.active_document() else {
        return Ok(Vec::new());
    };
    let wanted = subtree_ids(&session.document, shapes);
    let mut paths = Vec::new();
    for entry in node_entries(scene) {
        if wanted.contains(&entry.shape) {
            paths.push(entry.path);
        }
    }
    Ok(paths)
}

/// The selected shapes plus every shape below them (group members).
pub(crate) fn subtree_ids(doc: &Document, shapes: &[ShapeId]) -> BTreeSet<ShapeId> {
    let mut wanted = BTreeSet::new();
    let mut stack: Vec<ShapeId> = shapes.to_vec();
    let mut steps = 0usize;
    while let Some(id) = stack.pop() {
        steps += 1;
        if steps > doc.shapes.len().saturating_mul(2).saturating_add(16) {
            break;
        }
        if !wanted.insert(id) {
            continue;
        }
        if let Some(shape) = doc.shape(id) {
            stack.extend(shape.children.iter().copied());
        }
    }
    wanted
}

/// Whether `shape` is one of `roots` or below one (the moving set).
pub(crate) fn is_within(doc: &Document, shape: ShapeId, roots: &[ShapeId]) -> bool {
    let roots = subtree_ids(doc, roots);
    roots.contains(&shape)
}

/// The centre of a rectangle.
pub(crate) fn rect_center(bounds: Rect) -> Point {
    Point::new(
        (bounds.min.x + bounds.max.x) / 2.0,
        (bounds.min.y + bounds.max.y) / 2.0,
    )
}

/// The four corners of `bounds`, rotated about its centre.
pub(crate) fn rotated_corners(bounds: Rect, angle: Angle) -> [Point; 4] {
    let center = rect_center(bounds);
    [
        rotate_about(center, angle, bounds.min),
        rotate_about(center, angle, Point::new(bounds.max.x, bounds.min.y)),
        rotate_about(center, angle, bounds.max),
        rotate_about(center, angle, Point::new(bounds.min.x, bounds.max.y)),
    ]
}

/// Rotates `point` about `center` by `angle`.
pub(crate) fn rotate_about(center: Point, angle: Angle, point: Point) -> Point {
    let (sin, cos) = angle.radians().sin_cos();
    let dx = point.x - center.x;
    let dy = point.y - center.y;
    Point::new(
        center.x + dx * cos - dy * sin,
        center.y + dx * sin + dy * cos,
    )
}

/// A render failure as a [`UiError`].
pub(crate) fn render_error(error: larrygram_render::Error) -> UiError {
    UiError::Render {
        message: format!("the scene build failed: {error}"),
    }
}

/// True when both coordinates are finite.
fn finite(point: Point) -> bool {
    point.x.is_finite() && point.y.is_finite()
}

/// True when all four rectangle coordinates are finite.
fn finite_rect(bounds: Rect) -> bool {
    finite(bounds.min) && finite(bounds.max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use larrygram_commands::{DocumentSession, Settings};
    use larrygram_model::{
        Cell, CellName, CellValue, Document, Page, PageId, Row, Section, SectionName, Shape,
    };

    fn cell(name: &str, value: f64) -> Cell {
        let mut cell = Cell::new(CellName::new(name));
        cell.value = Some(CellValue::Number(value));
        cell
    }

    fn shape_document(angle: f64) -> (AppCtx, ShapeId) {
        let mut doc = Document::new();
        let mut shape = Shape::new(ShapeId { index: 0 }, 1);
        shape.sheet.cells = vec![
            cell("Width", 2.0),
            cell("Height", 1.0),
            cell("PinX", 2.0),
            cell("PinY", 3.0),
            cell("LocPinX", 1.0),
            cell("LocPinY", 0.5),
            cell("Angle", angle),
        ];
        let mut geometry = Section::new(SectionName::from_static("Geometry"));
        for (ix, (kind, x, y)) in [
            ("MoveTo", 0.0, 0.0),
            ("LineTo", 2.0, 0.0),
            ("LineTo", 2.0, 1.0),
            ("LineTo", 0.0, 1.0),
            ("LineTo", 0.0, 0.0),
        ]
        .into_iter()
        .enumerate()
        {
            let mut row = Row::new();
            row.row_type = Some(kind.to_owned());
            row.ix = Some(i32::try_from(ix).unwrap_or(0));
            row.cells = vec![cell("X", x), cell("Y", y)];
            geometry.rows.push(row);
        }
        shape.sheet.sections.push(geometry);
        let id = doc.add_shape(shape);
        let mut page = Page::new(PageId { index: 0 }, 1);
        page.shapes.push(id);
        doc.pages.push(page);
        let mut app = AppCtx::new(Settings::default());
        app.open_document(DocumentSession::new(doc, None));
        app.selection.shapes = vec![id];
        app.selection.anchor = Some(id);
        app.selection.page = Some(PageId { index: 0 });
        (app, id)
    }

    #[test]
    fn handles_sit_on_the_edges_corners_and_above() {
        let bounds = Rect::new(Point::new(0.0, 0.0), Point::new(2.0, 1.0));
        let handles = handles_for(bounds, Angle::from_radians(0.0), 1.0).expect("handles");
        assert_eq!(handles.len(), 9);
        assert_eq!(handles[0].0, HandleKind::Resize(ResizeEdge::NorthWest));
        assert_eq!(handles[0].1, Point::new(0.0, 1.0));
        assert_eq!(handles[8].0, HandleKind::Rotate);
        assert_eq!(handles[8].1, Point::new(1.0, 1.0 + ROTATE_OFFSET_PX / 96.0));
        // The rotate offset is in screen pixels: a quarter of the page
        // distance at 400%, four times it at 25%.
        let at = |zoom: f64| {
            handles_for(bounds, Angle::default(), zoom).expect("handles")[8]
                .1
                .y
        };
        assert!((at(4.0) - (1.0 + ROTATE_OFFSET_PX / 384.0)).abs() < 1.0e-12);
        assert!((at(0.25) - (1.0 + ROTATE_OFFSET_PX / 24.0)).abs() < 1.0e-12);
    }

    #[test]
    fn a_rotated_handle_set_turns_about_the_centre() {
        let bounds = Rect::new(Point::new(0.0, 0.0), Point::new(2.0, 2.0));
        let handles = handles_for(
            bounds,
            Angle::from_radians(core::f64::consts::FRAC_PI_2),
            1.0,
        )
        .expect("handles");
        let (_, north) = handles[1];
        // A positive angle turns counterclockwise in page coordinates
        // (y up), so the north handle moves to the west side.
        assert!((north.x - 0.0).abs() < 1.0e-9, "{north:?}");
        assert!((north.y - 1.0).abs() < 1.0e-9, "{north:?}");
    }

    #[test]
    fn handle_hits_prefer_the_handle_and_cover_the_interior() {
        let bounds = Rect::new(Point::new(0.0, 0.0), Point::new(2.0, 1.0));
        let angle = Angle::from_radians(0.0);
        assert_eq!(
            handle_at(Point::new(2.0, 0.5), bounds, angle, 1.0).expect("hit"),
            Some(HandleKind::Resize(ResizeEdge::East))
        );
        assert_eq!(
            handle_at(Point::new(1.0, 0.5), bounds, angle, 1.0).expect("hit"),
            Some(HandleKind::Move)
        );
        assert_eq!(
            handle_at(Point::new(4.0, 4.0), bounds, angle, 1.0).expect("miss"),
            None
        );
    }

    #[test]
    fn the_handle_tolerance_follows_the_zoom() {
        let bounds = Rect::new(Point::new(0.0, 0.0), Point::new(2.0, 1.0));
        let angle = Angle::from_radians(0.0);
        let near = Point::new(2.0 + 0.04, 0.5);
        assert_eq!(
            handle_at(near, bounds, angle, 1.0).expect("hit"),
            Some(HandleKind::Resize(ResizeEdge::East))
        );
        assert_eq!(
            handle_at(near, bounds, angle, 4.0).expect("miss"),
            None,
            "7 px is 0.073 in at zoom 1 but 0.018 in at zoom 4"
        );
    }

    /// Every handle hits on its drawn square at 25%, 100% and 400%,
    /// and the nearest handle wins where two reach (task P7-19).
    #[test]
    fn every_handle_hits_on_its_square_at_every_zoom() {
        let bounds = Rect::new(Point::new(1.0, 1.0), Point::new(3.0, 2.0));
        for angle in [0.0, 0.5] {
            let angle = Angle::from_radians(angle);
            for zoom in [0.25, 1.0, 4.0] {
                let pixel = 1.0 / (zoom * 96.0);
                for (kind, anchor) in handles_for(bounds, angle, zoom).expect("handles") {
                    for (dx, dy) in [(0.0, 0.0), (3.0, 0.0), (0.0, -3.0), (-3.0, 3.0)] {
                        let point = Point::new(anchor.x + dx * pixel, anchor.y + dy * pixel);
                        assert_eq!(
                            handle_at(point, bounds, angle, zoom).expect("hit"),
                            Some(kind),
                            "{kind:?} at zoom {zoom} offset ({dx}, {dy})"
                        );
                    }
                }
            }
        }
    }

    /// The rotate handle at 25% zoom is no longer inside the north
    /// handle's reach, and a small shape keeps a move interior.
    #[test]
    fn the_rotate_handle_and_a_small_interior_stay_reachable() {
        let bounds = Rect::new(Point::new(1.0, 1.0), Point::new(3.0, 2.0));
        let zoom = 0.25;
        let rotate = handles_for(bounds, Angle::default(), zoom).expect("handles")[8].1;
        assert_eq!(
            handle_at(rotate, bounds, Angle::default(), zoom).expect("hit"),
            Some(HandleKind::Rotate)
        );
        // 12 px on screen at 100%: the centre is 6 px from the side
        // handles, beyond their square.
        let small = Rect::new(Point::new(1.0, 1.0), Point::new(1.125, 1.125));
        assert_eq!(
            handle_at(Point::new(1.0625, 1.0625), small, Angle::default(), 1.0).expect("hit"),
            Some(HandleKind::Move)
        );
        // Just outside a corner square (5 px away diagonally) still hits.
        let pixel = 1.0 / 96.0;
        assert_eq!(
            handle_at(
                Point::new(3.0 + 4.0 * pixel, 2.0 + 4.0 * pixel),
                bounds,
                Angle::default(),
                1.0
            )
            .expect("hit"),
            Some(HandleKind::Resize(ResizeEdge::NorthEast))
        );
        // A corner of a rotated shape's envelope outside the shape
        // itself is not the move interior.
        let square = Rect::new(Point::new(0.0, 0.0), Point::new(2.0, 2.0));
        let turned = Angle::from_radians(core::f64::consts::FRAC_PI_4);
        assert_eq!(
            handle_at(Point::new(0.05, 0.05), square, turned, 1.0).expect("miss"),
            None
        );
    }

    #[test]
    fn a_non_finite_query_never_hits() {
        let bounds = Rect::new(Point::new(0.0, 0.0), Point::new(2.0, 1.0));
        assert_eq!(
            handle_at(Point::new(f64::NAN, 0.0), bounds, Angle::default(), 1.0).expect("no panic"),
            None
        );
        assert!(
            handles_for(
                Rect::new(Point::new(f64::NAN, 0.0), Point::new(1.0, 1.0)),
                Angle::default(),
                1.0
            )
            .expect("no panic")
            .is_empty()
        );
    }

    #[test]
    fn the_selection_frame_comes_from_the_scene() {
        let (app, _id) = shape_document(0.0);
        let bounds = selection_bounds(&app).expect("bounds");
        assert!((bounds.width() - 2.0).abs() < 1.0e-9);
        assert!((bounds.height() - 1.0).abs() < 1.0e-9);
        let (frame_bounds, angle) = selection_frame(&app, None).expect("frame").expect("some");
        assert_eq!(frame_bounds, bounds);
        assert_eq!(angle.radians(), 0.0);
    }

    #[test]
    fn an_empty_selection_has_no_bounds() {
        let (mut app, _id) = shape_document(0.0);
        app.selection.clear();
        assert!(selection_bounds(&app).is_err());
        assert_eq!(selection_frame(&app, None).expect("no error"), None);
        assert!(
            paths_for_shapes(&Scene::new(1.0, 1.0), &app, &[])
                .expect("paths")
                .is_empty()
        );
    }

    #[test]
    fn an_invalid_zoom_still_hits_with_the_default_scale() {
        let bounds = Rect::new(Point::new(0.0, 0.0), Point::new(2.0, 1.0));
        assert_eq!(
            handle_at(Point::new(2.0, 0.5), bounds, Angle::default(), f64::NAN).expect("no error"),
            Some(HandleKind::Resize(ResizeEdge::East))
        );
        assert_eq!(
            handle_at(
                Point::new(2.0, 0.5),
                bounds,
                Angle::from_radians(f64::NAN),
                1.0
            )
            .expect("no error"),
            None
        );
    }

    #[test]
    fn an_empty_app_has_no_scene_or_frame() {
        let app = AppCtx::new(Settings::default());
        assert_eq!(scene_for(&app).expect("no error"), None);
        assert_eq!(selection_frame(&app, None).expect("no error"), None);
        assert!(selection_bounds(&app).is_err());
        assert!(
            paths_for_shapes(&Scene::new(1.0, 1.0), &app, &[])
                .expect("no error")
                .is_empty()
        );
        let mut app = app;
        let mut doc = Document::new();
        doc.pages.push(Page::new(PageId { index: 0 }, 1));
        app.open_document(DocumentSession::new(doc, None));
        app.active_document_mut().expect("session").active_page = None;
        assert_eq!(scene_for(&app).expect("no error"), None);
    }

    #[test]
    fn a_cyclic_group_chain_is_walked_once() {
        let (mut app, group) = shape_document(0.0);
        {
            let session = app.active_document_mut().expect("session");
            if let Some(entry) = session.document.shape_mut(group) {
                entry.children.push(group);
            }
        }
        let doc = &app.active_document().expect("session").document;
        let wanted = subtree_ids(doc, &[group]);
        assert_eq!(wanted.len(), 1);
        assert_eq!(subtree_ids(doc, &[ShapeId { index: 99 }]).len(), 1);
    }

    #[test]
    fn render_errors_map_into_ui_errors() {
        let error = render_error(larrygram_render::Error::TooLarge);
        assert!(matches!(error, UiError::Render { .. }), "{error:?}");
    }

    #[test]
    fn a_selected_group_selects_its_members() {
        let (mut app, group) = shape_document(0.0);
        let session = app.active_document_mut().expect("session");
        let child = Shape::new(ShapeId { index: 0 }, 2);
        let child = session.document.add_shape(child);
        if let Some(entry) = session.document.shape_mut(group) {
            entry.children.push(child);
        }
        if let Some(entry) = session.document.shape_mut(child) {
            entry.parent = Some(group);
        }
        app.selection.shapes = vec![group];
        let wanted = subtree_ids(&app.active_document().expect("session").document, &[group]);
        assert!(wanted.contains(&group));
        assert!(wanted.contains(&child));
        assert!(is_within(
            &app.active_document().expect("session").document,
            child,
            &[group]
        ));
    }
}
