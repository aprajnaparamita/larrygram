//! The pointer tool (P5-05).
//!
//! Select, multi-select, marquee, and move, resize and rotate through the
//! handles ([`crate::handles`], ADR 0041, ADR 0042).
//!
//! A pointer down hit-tests the scene (`larrygram_render::hit`), shift
//! toggles the selection, a drag moves or transforms with snapping
//! ([`crate::snap`], ADR 0043), and one drag commits exactly one undo edit
//! (ADR 0040): the tool emits one command intent on release and previews
//! the drag with overlay marks in between.

use larrygram_commands::{AppCtx, ArgValue, Args, CommandId, Invocation, SessionId, Source};
use larrygram_geom::{Angle, Point, Rect};
use larrygram_model::{Document, PageId, ShapeId};
use larrygram_render::{
    HitOptions, Overlay, OverlayMark, Scene, build_selection_overlay, hit_marquee, hit_test,
};

use super::{Modifiers, ToolEffect, ToolEvent};
use crate::UiError;
use crate::handles::{
    HandleKind, ResizeEdge, handle_at, paths_for_shapes, rect_center, render_error, scene_for,
    selection_frame,
};
use crate::snap::Snapper;
use crate::tools::ToolMachine;

/// The hit tolerance of the pointer, in screen pixels (ADR 0041).
pub const HIT_TOLERANCE_PX: f64 = 4.0;

/// The distance a press must travel before it becomes a drag, in screen
/// pixels (our own choice; a click stays a click).
const DRAG_THRESHOLD_PX: f64 = 2.0;

/// The smallest size a resize drag may produce, in inches (one screen
/// pixel at 96 ppi).
const MIN_SIZE_IN: f64 = 1.0 / 96.0;

/// The scene cache key and value of one pointer session.
#[derive(Debug, Clone, PartialEq)]
struct SceneCache {
    session: SessionId,
    changes: u64,
    page: PageId,
    scene: Scene,
}

/// What the pointer is doing between events.
#[derive(Debug, Clone, Default, PartialEq)]
enum Mode {
    /// Nothing pressed.
    #[default]
    Idle,
    /// A press on a shape, not yet a drag.
    Pressed {
        /// Where the press landed.
        start: Point,
        /// The shape under the press.
        shape: ShapeId,
    },
    /// A marquee from empty space.
    Marquee {
        /// Where the marquee started.
        start: Point,
        /// The current pointer position.
        current: Point,
        /// Shift/Ctrl held at the start (add to the selection).
        add: bool,
    },
    /// A handle drag.
    Drag {
        /// Which handle.
        kind: HandleKind,
        /// Where the drag started.
        start: Point,
        /// The current pointer position.
        current: Point,
        /// The selection frame at the start.
        frame: (Rect, Angle),
        /// The selected shapes (the moving set).
        moving: Vec<ShapeId>,
        /// Alt was held (snapping off).
        alt: bool,
        /// The zoom at the start (screen-pixel thresholds).
        zoom: f64,
    },
}

/// The pointer tool's drag state (owner: P5-05; opaque to the other tool
/// modules).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PointerState {
    /// The current gesture.
    mode: Mode,
    /// The cached page scene (rebuilt when the session or its change
    /// counter moves).
    scene: Option<SceneCache>,
}

impl PointerState {
    /// The page scene for `ctx`, from the cache when the key matches.
    pub(crate) fn scene(&mut self, ctx: &AppCtx) -> Result<Option<&Scene>, UiError> {
        let Some(session) = ctx.active_document() else {
            self.scene = None;
            return Ok(None);
        };
        let Some(page) = session.active_page else {
            self.scene = None;
            return Ok(None);
        };
        let fresh = self.scene.as_ref().is_some_and(|cache| {
            cache.session == session.id && cache.changes == session.changes && cache.page == page
        });
        if !fresh {
            let Some(scene) = scene_for(ctx)? else {
                self.scene = None;
                return Ok(None);
            };
            self.scene = Some(SceneCache {
                session: session.id,
                changes: session.changes,
                page,
                scene,
            });
        }
        Ok(self.scene.as_ref().map(|cache| &cache.scene))
    }

    /// The current mode.
    fn is_idle(&self) -> bool {
        matches!(self.mode, Mode::Idle)
    }
}

/// Handles one event for the pointer tool (owner: P5-05, ADR 0042).
pub fn handle(
    state: &mut ToolMachine,
    event: &ToolEvent,
    ctx: &AppCtx,
) -> Result<ToolEffect, UiError> {
    let zoom = ctx
        .active_document()
        .map(|session| session.view.zoom)
        .filter(|zoom| zoom.is_finite() && *zoom > 0.0)
        .unwrap_or(1.0);
    let page_size = {
        let Some(scene) = state.state.scene(ctx)? else {
            return Ok(ToolEffect::new());
        };
        (scene.width, scene.height)
    };
    match event {
        ToolEvent::PointerDown { pos, modifiers } => {
            down(state, ctx, *pos, *modifiers, zoom, page_size)
        }
        ToolEvent::PointerMove { pos, modifiers } => {
            moved(state, ctx, *pos, *modifiers, zoom, page_size)
        }
        ToolEvent::PointerUp { pos, modifiers } => {
            up(state, ctx, *pos, *modifiers, zoom, page_size)
        }
        ToolEvent::DoubleClick { pos } => double_click(state, ctx, *pos, zoom),
        ToolEvent::Cancel => {
            state.state.mode = Mode::Idle;
            let overlay = idle_overlay(state, ctx)?.unwrap_or_default();
            Ok(ToolEffect {
                overlay,
                ..ToolEffect::new()
            })
        }
    }
}

/// The drag anchor, in page inches (cursor and overlay hint).
pub fn anchor(state: &ToolMachine) -> Option<Point> {
    match &state.state.mode {
        Mode::Idle => None,
        Mode::Pressed { start, .. } => Some(*start),
        Mode::Marquee { current, .. } | Mode::Drag { current, .. } => Some(*current),
    }
}

/// The selection overlay while the tool is idle; `None` during a gesture.
pub fn idle_overlay(state: &mut ToolMachine, ctx: &AppCtx) -> Result<Option<Overlay>, UiError> {
    if !state.state.is_idle() {
        return Ok(None);
    }
    let Some(scene) = state.state.scene(ctx)? else {
        return Ok(Some(Overlay::new()));
    };
    overlay_for_shapes(ctx, scene, &ctx.selection.shapes).map(Some)
}

/// One press: a handle drag, a shape selection, or a marquee start.
fn down(
    state: &mut ToolMachine,
    ctx: &AppCtx,
    pos: Point,
    modifiers: Modifiers,
    zoom: f64,
    page_size: (f64, f64),
) -> Result<ToolEffect, UiError> {
    let mut effect = ToolEffect::new();
    if state.state.scene(ctx)?.is_none() {
        state.state.mode = Mode::Idle;
        return Ok(effect);
    }
    let Some(session) = ctx.active_document() else {
        return Ok(effect);
    };

    let handle_frame = {
        let scene = state.state.scene(ctx)?;
        if !ctx.selection.is_empty() && !modifiers.shift && !modifiers.ctrl {
            selection_frame(ctx, scene)?
        } else {
            None
        }
    };
    if let Some((bounds, angle)) = handle_frame
        && let Some(kind) = handle_at(pos, bounds, angle, zoom)?
    {
        state.state.mode = Mode::Drag {
            kind,
            start: pos,
            current: pos,
            frame: (bounds, angle),
            moving: ctx.selection.shapes.clone(),
            alt: modifiers.alt,
            zoom,
        };
        let mode = state.state.mode.clone();
        let Some(scene) = state.state.scene(ctx)? else {
            return Ok(effect);
        };
        effect.overlay = drag_overlay(ctx, scene, page_size, &mode)?;
        return Ok(effect);
    }

    let tolerance = HIT_TOLERANCE_PX / (zoom * crate::canvas::POINTS_PER_INCH);
    let hit = {
        let Some(scene) = state.state.scene(ctx)? else {
            return Ok(effect);
        };
        let hits = hit_test(scene, pos, &HitOptions::new(tolerance)).map_err(render_error)?;
        hits.iter()
            .find_map(|hit| hit.shape_id)
            .map(|shape| selectable_shape(&session.document, shape, ctx.selection.entered))
    };
    match hit {
        Some(shape) => {
            let mode = if modifiers.ctrl {
                "add"
            } else if modifiers.shift {
                "toggle"
            } else {
                "replace"
            };
            effect
                .intents
                .push(select_intent(&session.document, mode, &[shape])?);
            state.state.mode = Mode::Pressed { start: pos, shape };
        }
        None => {
            let add = modifiers.shift || modifiers.ctrl;
            if !add {
                effect
                    .intents
                    .push(select_intent(&session.document, "replace", &[])?);
            }
            state.state.mode = Mode::Marquee {
                start: pos,
                current: pos,
                add,
            };
            effect.overlay = marquee_overlay(pos, pos);
        }
    }
    Ok(effect)
}

/// One move: start a move drag after the threshold, extend a marquee, or
/// update a handle drag.
fn moved(
    state: &mut ToolMachine,
    ctx: &AppCtx,
    pos: Point,
    modifiers: Modifiers,
    zoom: f64,
    page_size: (f64, f64),
) -> Result<ToolEffect, UiError> {
    let mut effect = ToolEffect::new();
    match state.state.mode.clone() {
        Mode::Pressed { start, shape } => {
            let threshold = DRAG_THRESHOLD_PX / (zoom * crate::canvas::POINTS_PER_INCH);
            let delta = pos.sub(start);
            if delta.x.hypot(delta.y) < threshold {
                return Ok(effect);
            }
            let handle_frame = {
                let scene = state.state.scene(ctx)?;
                selection_frame(ctx, scene)?
            };
            let Some((bounds, angle)) = handle_frame else {
                return Ok(effect);
            };
            state.state.mode = Mode::Drag {
                kind: HandleKind::Move,
                start,
                current: pos,
                frame: (bounds, angle),
                moving: if ctx.selection.contains(shape) {
                    ctx.selection.shapes.clone()
                } else {
                    vec![shape]
                },
                alt: modifiers.alt,
                zoom,
            };
        }
        Mode::Marquee { start, add, .. } => {
            state.state.mode = Mode::Marquee {
                start,
                current: pos,
                add,
            };
        }
        Mode::Drag {
            kind,
            start,
            current: _,
            frame,
            moving,
            alt: _,
            zoom: start_zoom,
        } => {
            state.state.mode = Mode::Drag {
                kind,
                start,
                current: pos,
                frame,
                moving,
                alt: modifiers.alt,
                zoom: start_zoom,
            };
        }
        Mode::Idle => return Ok(effect),
    }
    let mode = state.state.mode.clone();
    let Some(scene) = state.state.scene(ctx)? else {
        return Ok(effect);
    };
    effect.overlay = drag_overlay(ctx, scene, page_size, &mode)?;
    Ok(effect)
}

/// One release: commit one drag command, apply the marquee, or finish a
/// click.
fn up(
    state: &mut ToolMachine,
    ctx: &AppCtx,
    pos: Point,
    modifiers: Modifiers,
    zoom: f64,
    page_size: (f64, f64),
) -> Result<ToolEffect, UiError> {
    let mut effect = ToolEffect::new();
    let mode = std::mem::replace(&mut state.state.mode, Mode::Idle);
    match mode {
        Mode::Idle => {}
        Mode::Pressed { .. } => {
            let overlay = idle_overlay(state, ctx)?;
            if let Some(overlay) = overlay {
                effect.overlay = overlay;
            }
        }
        Mode::Marquee { start, add, .. } => {
            let Some(scene) = state.state.scene(ctx)? else {
                return Ok(effect);
            };
            let Some(session) = ctx.active_document() else {
                return Ok(effect);
            };
            let rect = Rect::new(
                Point::new(start.x.min(pos.x), start.y.min(pos.y)),
                Point::new(start.x.max(pos.x), start.y.max(pos.y)),
            );
            let hits = hit_marquee(scene, rect).map_err(render_error)?;
            let mut found: Vec<ShapeId> = Vec::new();
            for hit in hits {
                if let Some(shape) = hit.shape_id {
                    let shape = selectable_shape(&session.document, shape, ctx.selection.entered);
                    if !found.contains(&shape) {
                        found.push(shape);
                    }
                }
            }
            let mode = if add { "add" } else { "replace" };
            effect
                .intents
                .push(select_intent(&session.document, mode, &found)?);
            let mut shapes = found;
            if add {
                let mut merged = ctx.selection.shapes.clone();
                for shape in shapes {
                    if !merged.contains(&shape) {
                        merged.push(shape);
                    }
                }
                shapes = merged;
            }
            let overlay = overlay_for_shapes(ctx, scene, &shapes)?;
            effect.overlay = overlay;
        }
        Mode::Drag {
            kind,
            start,
            current: _,
            frame,
            moving,
            alt,
            zoom: start_zoom,
        } => {
            let _ = modifiers;
            let _ = zoom;
            let alt = alt || modifiers.alt;
            let scene = state.state.scene(ctx)?;
            if let Some(intent) = drag_intent(
                ctx, scene, kind, start, pos, frame, &moving, alt, start_zoom,
            )? {
                effect.intents.push(intent);
            }
            let Some(scene) = scene else {
                return Ok(effect);
            };
            effect.overlay = overlay_for_shapes(ctx, scene, &moving)?;
            let _ = page_size;
        }
    }
    Ok(effect)
}

/// A double click enters a group (ADR 0041).
fn double_click(
    state: &mut ToolMachine,
    ctx: &AppCtx,
    pos: Point,
    zoom: f64,
) -> Result<ToolEffect, UiError> {
    let mut effect = ToolEffect::new();
    let Some(scene) = state.state.scene(ctx)? else {
        return Ok(effect);
    };
    let Some(session) = ctx.active_document() else {
        return Ok(effect);
    };
    let tolerance = HIT_TOLERANCE_PX / (zoom * crate::canvas::POINTS_PER_INCH);
    let hits = hit_test(scene, pos, &HitOptions::new(tolerance)).map_err(render_error)?;
    let Some(shape) = hits.iter().find_map(|hit| hit.shape_id) else {
        return Ok(effect);
    };
    let shape = selectable_shape(&session.document, shape, ctx.selection.entered);
    let is_group = session
        .document
        .shape(shape)
        .is_some_and(|entry| !entry.children.is_empty());
    effect
        .intents
        .push(select_intent(&session.document, "replace", &[shape])?);
    if is_group {
        effect
            .intents
            .push(intent("shape.enter_group", Args::new())?);
    } else {
        // A non-group shape starts in-place text editing (ADR 0044,
        // task P5-17 Goal 5). `text.begin` reads the selection the
        // `edit.select` intent just installed.
        effect.intents.push(intent("text.begin", Args::new())?);
    }
    Ok(effect)
}

/// Maps a scene hit to the shape the user selects: the top-level ancestor
/// at page level, or the direct child of the entered group inside one.
fn selectable_shape(doc: &Document, hit: ShapeId, entered: Option<ShapeId>) -> ShapeId {
    let mut current = hit;
    let mut seen = Vec::new();
    loop {
        if seen.contains(&current) {
            return hit;
        }
        seen.push(current);
        let Some(shape) = doc.shape(current) else {
            return hit;
        };
        let Some(parent) = shape.parent else {
            return current;
        };
        if entered == Some(parent) {
            return current;
        }
        current = parent;
    }
}

/// The marquee overlay mark.
fn marquee_overlay(start: Point, current: Point) -> Overlay {
    let rect = Rect::new(
        Point::new(start.x.min(current.x), start.y.min(current.y)),
        Point::new(start.x.max(current.x), start.y.max(current.y)),
    );
    let mut overlay = Overlay::new();
    overlay.marks.push(OverlayMark::RubberBand(rect));
    overlay
}

/// The selection outline and handles of `shapes`.
fn overlay_for_shapes(ctx: &AppCtx, scene: &Scene, shapes: &[ShapeId]) -> Result<Overlay, UiError> {
    let paths = paths_for_shapes(scene, ctx, shapes)?;
    if paths.is_empty() {
        return Ok(Overlay::new());
    }
    build_selection_overlay(scene, &paths).map_err(render_error)
}

/// The preview marks of the current gesture.
fn drag_overlay(
    ctx: &AppCtx,
    scene: &Scene,
    page_size: (f64, f64),
    mode: &Mode,
) -> Result<Overlay, UiError> {
    match mode {
        Mode::Idle | Mode::Pressed { .. } => overlay_for_shapes(ctx, scene, &ctx.selection.shapes),
        Mode::Marquee { start, current, .. } => Ok(marquee_overlay(*start, *current)),
        Mode::Drag {
            kind,
            start,
            current,
            frame,
            moving,
            alt,
            zoom,
        } => {
            let mut overlay = Overlay::new();
            let (bounds, angle, snapped) = preview_frame(
                ctx,
                Some(scene),
                *kind,
                *start,
                *current,
                *frame,
                moving,
                *alt,
                *zoom,
            )?;
            overlay
                .marks
                .push(OverlayMark::SelectionOutline { bounds, angle });
            overlay.marks.push(OverlayMark::Handles { bounds, angle });
            if let Some(kind) = snapped {
                let center = rect_center(bounds);
                let half = 0.25 / zoom.max(1.0e-9);
                let _ = half;
                overlay.marks.push(OverlayMark::SnapGuide {
                    a: Point::new(center.x, 0.0),
                    b: Point::new(center.x, page_size.1),
                });
                overlay.marks.push(OverlayMark::SnapGuide {
                    a: Point::new(0.0, center.y),
                    b: Point::new(page_size.0, center.y),
                });
                let _ = kind;
            }
            Ok(overlay)
        }
    }
}

/// The preview frame of a drag: `(bounds, angle, snapped)`.
#[allow(clippy::too_many_arguments)] // one preview seam
fn preview_frame(
    ctx: &AppCtx,
    scene: Option<&Scene>,
    kind: HandleKind,
    start: Point,
    current: Point,
    frame: (Rect, Angle),
    moving: &[ShapeId],
    alt: bool,
    zoom: f64,
) -> Result<(Rect, Angle, Option<larrygram_geom::SnapKind>), UiError> {
    let (bounds, angle) = frame;
    let snapper = Snapper::default();
    match kind {
        HandleKind::Move => {
            let delta = current.sub(start);
            let result = snapper.snap_move(ctx, scene, moving, delta, zoom, alt)?;
            let moved = Rect::new(
                Point::new(bounds.min.x + result.point.x, bounds.min.y + result.point.y),
                Point::new(bounds.max.x + result.point.x, bounds.max.y + result.point.y),
            );
            Ok((moved, angle, result.kind))
        }
        HandleKind::Resize(edge) => {
            let point = snapper.snap_point(ctx, scene, current, zoom, alt)?;
            let (width, height) = resized_size(bounds, start, point.point, edge);
            let center = rect_center(bounds);
            let resized = Rect::new(
                Point::new(center.x - width / 2.0, center.y - height / 2.0),
                Point::new(center.x + width / 2.0, center.y + height / 2.0),
            );
            Ok((resized, angle, point.kind))
        }
        HandleKind::Rotate => {
            let center = rect_center(bounds);
            let start_angle = (start.y - center.y).atan2(start.x - center.x);
            let now_angle = (current.y - center.y).atan2(current.x - center.x);
            let delta = now_angle - start_angle;
            Ok((bounds, Angle::from_radians(angle.radians() + delta), None))
        }
    }
}

/// The size a resize drag produces, clamped positive.
fn resized_size(bounds: Rect, start: Point, current: Point, edge: ResizeEdge) -> (f64, f64) {
    let dx = current.x - start.x;
    let dy = current.y - start.y;
    let mut width = bounds.width();
    let mut height = bounds.height();
    match edge {
        ResizeEdge::East | ResizeEdge::NorthEast | ResizeEdge::SouthEast => width += dx,
        ResizeEdge::West | ResizeEdge::NorthWest | ResizeEdge::SouthWest => width -= dx,
        _ => {}
    }
    match edge {
        ResizeEdge::North | ResizeEdge::NorthEast | ResizeEdge::NorthWest => height += dy,
        ResizeEdge::South | ResizeEdge::SouthEast | ResizeEdge::SouthWest => height -= dy,
        _ => {}
    }
    (width.max(MIN_SIZE_IN), height.max(MIN_SIZE_IN))
}

/// The command intent of one completed drag (one undo step, ADR 0040).
#[allow(clippy::too_many_arguments)] // one drag commit seam
fn drag_intent(
    ctx: &AppCtx,
    scene: Option<&Scene>,
    kind: HandleKind,
    start: Point,
    current: Point,
    frame: (Rect, Angle),
    moving: &[ShapeId],
    alt: bool,
    zoom: f64,
) -> Result<Option<crate::Intent>, UiError> {
    if moving.is_empty() {
        return Ok(None);
    }
    let snapper = Snapper::default();
    match kind {
        HandleKind::Move => {
            let delta = current.sub(start);
            let result = snapper.snap_move(ctx, scene, moving, delta, zoom, alt)?;
            if result.point.x == 0.0 && result.point.y == 0.0 {
                return Ok(None);
            }
            intent(
                "shape.move",
                Args::new()
                    .with("dx", ArgValue::Number(result.point.x))
                    .with("dy", ArgValue::Number(result.point.y)),
            )
            .map(Some)
        }
        HandleKind::Resize(edge) => {
            let point = snapper.snap_point(ctx, scene, current, zoom, alt)?;
            let (width, height) = resized_size(frame.0, start, point.point, edge);
            if (width - frame.0.width()).abs() < f64::EPSILON
                && (height - frame.0.height()).abs() < f64::EPSILON
            {
                return Ok(None);
            }
            intent(
                "shape.resize",
                Args::new()
                    .with("width", ArgValue::Number(width))
                    .with("height", ArgValue::Number(height)),
            )
            .map(Some)
        }
        HandleKind::Rotate => {
            let center = rect_center(frame.0);
            let start_angle = (start.y - center.y).atan2(start.x - center.x);
            let now_angle = (current.y - center.y).atan2(current.x - center.x);
            let delta = now_angle - start_angle;
            if !delta.is_finite() || delta.abs() < f64::EPSILON {
                return Ok(None);
            }
            intent(
                "shape.rotate",
                Args::new().with("angle", ArgValue::Number(delta)),
            )
            .map(Some)
        }
    }
}

/// The `edit.select` intent for `shapes`, with Visio ids as strings.
fn select_intent(doc: &Document, mode: &str, shapes: &[ShapeId]) -> Result<crate::Intent, UiError> {
    let ids = shapes
        .iter()
        .filter_map(|shape| doc.shape(*shape).map(|entry| entry.visio_id.to_string()))
        .collect();
    intent(
        "edit.select",
        Args::new()
            .with("shapes", ArgValue::List(ids))
            .with("mode", ArgValue::Text(mode.to_owned())),
    )
}

/// One dispatch request for a command id and arguments.
fn intent(id: &'static str, args: Args) -> Result<crate::Intent, UiError> {
    let command = CommandId::parse(id).map_err(|error| UiError::Io {
        message: format!("the tool command id {id} is not valid: {error}"),
    })?;
    Ok(crate::Intent::new(
        command,
        Invocation::new(args, Source::Ui),
    ))
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

    fn fixture() -> (AppCtx, ShapeId) {
        let mut doc = Document::new();
        let mut shape = Shape::new(ShapeId { index: 0 }, 1);
        shape.sheet.cells = vec![
            cell("Width", 2.0),
            cell("Height", 1.0),
            cell("PinX", 3.0),
            cell("PinY", 3.0),
            cell("LocPinX", 1.0),
            cell("LocPinY", 0.5),
            cell("Angle", 0.0),
            cell("FillPattern", 1.0),
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
        (app, id)
    }

    fn down(pos: Point, modifiers: Modifiers) -> ToolEvent {
        ToolEvent::PointerDown { pos, modifiers }
    }

    fn moved(pos: Point, modifiers: Modifiers) -> ToolEvent {
        ToolEvent::PointerMove { pos, modifiers }
    }

    fn up(pos: Point, modifiers: Modifiers) -> ToolEvent {
        ToolEvent::PointerUp { pos, modifiers }
    }

    /// The `edit.select` intent of an effect: its mode and Visio ids.
    fn select_command(effect: &ToolEffect) -> Option<(&str, Vec<String>)> {
        let intent = effect
            .intents
            .iter()
            .find(|intent| intent.command.as_str() == "edit.select")?;
        let mode = intent.invocation.args.text("mode")?;
        let ids = match intent.invocation.args.get("shapes")? {
            ArgValue::List(ids) => ids.clone(),
            _ => return None,
        };
        Some((mode, ids))
    }

    #[test]
    fn a_click_selects_and_shift_toggles() {
        let (mut app, id) = fixture();
        let mut machine = ToolMachine::new();
        let effect = machine
            .handle(down(Point::new(3.0, 3.0), Modifiers::none()), &app)
            .expect("down");
        assert_eq!(
            select_command(&effect),
            Some(("replace", vec!["1".to_owned()]))
        );
        let _ = machine
            .handle(up(Point::new(3.0, 3.0), Modifiers::none()), &app)
            .expect("up");
        app.selection.shapes = vec![id];
        app.selection.anchor = Some(id);
        let effect = machine
            .handle(
                down(
                    Point::new(3.0, 3.0),
                    Modifiers {
                        shift: true,
                        ..Modifiers::none()
                    },
                ),
                &app,
            )
            .expect("shift down");
        assert_eq!(
            select_command(&effect),
            Some(("toggle", vec!["1".to_owned()]))
        );
    }

    #[test]
    fn a_marquee_selects_intersecting_shapes() {
        let (app, _id) = fixture();
        let mut machine = ToolMachine::new();
        let effect = machine
            .handle(down(Point::new(1.5, 1.5), Modifiers::none()), &app)
            .expect("down");
        assert_eq!(
            select_command(&effect),
            Some(("replace", Vec::new())),
            "a plain press on empty space clears the selection"
        );
        let effect = machine
            .handle(moved(Point::new(5.0, 5.0), Modifiers::none()), &app)
            .expect("move");
        assert!(matches!(
            effect.overlay.marks.first(),
            Some(OverlayMark::RubberBand(_))
        ));
        let effect = machine
            .handle(up(Point::new(5.0, 5.0), Modifiers::none()), &app)
            .expect("up");
        assert_eq!(
            select_command(&effect),
            Some(("replace", vec!["1".to_owned()]))
        );
    }

    #[test]
    fn one_move_drag_emits_one_command_and_keeps_snapping() {
        let (mut app, id) = fixture();
        app.selection.shapes = vec![id];
        app.selection.anchor = Some(id);
        let mut machine = ToolMachine::new();
        machine
            .handle(down(Point::new(3.0, 3.0), Modifiers::none()), &app)
            .expect("down");
        let effect = machine
            .handle(moved(Point::new(3.5, 3.0), Modifiers::none()), &app)
            .expect("move");
        assert!(effect.intents.is_empty(), "a move previews, no command yet");
        let effect = machine
            .handle(up(Point::new(3.5, 3.0), Modifiers::none()), &app)
            .expect("up");
        assert_eq!(effect.intents.len(), 1);
        assert_eq!(effect.intents[0].command.as_str(), "shape.move");
        assert_eq!(effect.intents[0].invocation.args.number("dx"), Some(0.5));
    }

    #[test]
    fn alt_turns_the_move_snap_off() {
        let (mut app, id) = fixture();
        app.selection.shapes = vec![id];
        app.selection.anchor = Some(id);
        let mut machine = ToolMachine::new();
        machine
            .handle(down(Point::new(3.0, 3.0), Modifiers::none()), &app)
            .expect("down");
        machine
            .handle(
                moved(
                    Point::new(3.51, 3.0),
                    Modifiers {
                        alt: true,
                        ..Modifiers::none()
                    },
                ),
                &app,
            )
            .expect("move");
        let effect = machine
            .handle(
                up(
                    Point::new(3.51, 3.0),
                    Modifiers {
                        alt: true,
                        ..Modifiers::none()
                    },
                ),
                &app,
            )
            .expect("up");
        assert_eq!(effect.intents.len(), 1);
        let dx = effect.intents[0].invocation.args.number("dx").expect("dx");
        assert!((dx - 0.51).abs() < 1.0e-9, "{dx}");
    }

    #[test]
    fn a_resize_handle_emits_a_resize_command() {
        let (mut app, id) = fixture();
        app.selection.shapes = vec![id];
        app.selection.anchor = Some(id);
        let mut machine = ToolMachine::new();
        machine
            .handle(down(Point::new(4.0, 3.0), Modifiers::none()), &app)
            .expect("down");
        machine
            .handle(moved(Point::new(5.0, 3.0), Modifiers::none()), &app)
            .expect("move");
        let effect = machine
            .handle(up(Point::new(5.0, 3.0), Modifiers::none()), &app)
            .expect("up");
        assert_eq!(effect.intents.len(), 1);
        assert_eq!(effect.intents[0].command.as_str(), "shape.resize");
        let width = effect.intents[0]
            .invocation
            .args
            .number("width")
            .expect("width");
        assert!((width - 3.0).abs() < 1.0e-9, "{width}");
    }

    #[test]
    fn a_rotate_handle_emits_a_rotate_command() {
        let (mut app, id) = fixture();
        app.selection.shapes = vec![id];
        app.selection.anchor = Some(id);
        let mut machine = ToolMachine::new();
        // The rotate handle sits above the top edge (y = 3.5 + 0.25).
        machine
            .handle(down(Point::new(3.0, 3.75), Modifiers::none()), &app)
            .expect("down");
        machine
            .handle(moved(Point::new(3.5, 3.5), Modifiers::none()), &app)
            .expect("move");
        let effect = machine
            .handle(up(Point::new(3.5, 3.5), Modifiers::none()), &app)
            .expect("up");
        assert_eq!(effect.intents.len(), 1);
        assert_eq!(effect.intents[0].command.as_str(), "shape.rotate");
        let angle = effect.intents[0]
            .invocation
            .args
            .number("angle")
            .expect("angle");
        assert!(angle.abs() > 0.1, "{angle}");
    }

    #[test]
    fn ctrl_click_adds_to_the_selection() {
        let (app, _id) = fixture();
        let mut machine = ToolMachine::new();
        let effect = machine
            .handle(
                down(
                    Point::new(3.0, 3.0),
                    Modifiers {
                        ctrl: true,
                        ..Modifiers::none()
                    },
                ),
                &app,
            )
            .expect("ctrl down");
        assert_eq!(select_command(&effect), Some(("add", vec!["1".to_owned()])));
    }

    #[test]
    fn the_anchor_tracks_the_current_gesture() {
        let (mut app, id) = fixture();
        app.selection.shapes = vec![id];
        app.selection.anchor = Some(id);
        let mut machine = ToolMachine::new();
        assert_eq!(anchor(&machine), None);
        machine
            .handle(down(Point::new(3.0, 3.0), Modifiers::none()), &app)
            .expect("down");
        assert_eq!(anchor(&machine), Some(Point::new(3.0, 3.0)));
        machine
            .handle(moved(Point::new(3.5, 3.0), Modifiers::none()), &app)
            .expect("move");
        assert_eq!(anchor(&machine), Some(Point::new(3.5, 3.0)));
        machine.handle(ToolEvent::Cancel, &app).expect("cancel");
        assert_eq!(anchor(&machine), None);
    }

    #[test]
    fn a_cancel_returns_to_the_idle_overlay() {
        let (mut app, id) = fixture();
        app.selection.shapes = vec![id];
        app.selection.anchor = Some(id);
        let mut machine = ToolMachine::new();
        machine
            .handle(down(Point::new(1.0, 1.0), Modifiers::none()), &app)
            .expect("down");
        let effect = machine.handle(ToolEvent::Cancel, &app).expect("cancel");
        assert!(!effect.overlay.is_empty(), "the selection overlay returns");
    }

    #[test]
    fn a_tool_without_a_document_is_a_no_op() {
        let ctx = AppCtx::new(Settings::default());
        let mut machine = ToolMachine::new();
        for event in [
            down(Point::new(1.0, 1.0), Modifiers::none()),
            moved(Point::new(2.0, 2.0), Modifiers::none()),
            up(Point::new(2.0, 2.0), Modifiers::none()),
            ToolEvent::DoubleClick {
                pos: Point::new(1.0, 1.0),
            },
            ToolEvent::Cancel,
        ] {
            machine.handle(event, &ctx).expect("no error");
        }
        assert_eq!(
            machine.idle_overlay(&ctx).expect("overlay"),
            Some(Overlay::new())
        );
    }

    #[test]
    fn a_double_click_on_a_group_enters_it() {
        let (mut app, group) = fixture();
        {
            let session = app.active_document_mut().expect("session");
            let child = Shape::new(ShapeId { index: 0 }, 2);
            let child = session.document.add_shape(child);
            if let Some(entry) = session.document.shape_mut(group) {
                entry.children.push(child);
            }
            if let Some(entry) = session.document.shape_mut(child) {
                entry.parent = Some(group);
            }
        }
        let mut machine = ToolMachine::new();
        let effect = machine
            .handle(
                ToolEvent::DoubleClick {
                    pos: Point::new(3.0, 3.0),
                },
                &app,
            )
            .expect("double click");
        assert_eq!(
            select_command(&effect),
            Some(("replace", vec!["1".to_owned()]))
        );
        assert_eq!(effect.intents.len(), 2);
        assert_eq!(effect.intents[0].command.as_str(), "edit.select");
        assert_eq!(effect.intents[1].command.as_str(), "shape.enter_group");

        // A double click on empty space is a no-op.
        let effect = machine
            .handle(
                ToolEvent::DoubleClick {
                    pos: Point::new(99.0, 99.0),
                },
                &app,
            )
            .expect("empty double click");
        assert!(effect.intents.is_empty());
    }

    #[test]
    fn a_double_click_on_a_plain_shape_starts_text_editing() {
        let (app, shape) = fixture();
        let mut machine = ToolMachine::new();
        let effect = machine
            .handle(
                ToolEvent::DoubleClick {
                    pos: Point::new(3.0, 3.0),
                },
                &app,
            )
            .expect("double click");
        assert_eq!(
            select_command(&effect),
            Some(("replace", vec!["1".to_owned()])),
            "the shape becomes the selection"
        );
        assert_eq!(effect.intents.len(), 2);
        assert_eq!(effect.intents[0].command.as_str(), "edit.select");
        assert_eq!(effect.intents[1].command.as_str(), "text.begin");
        let _ = shape;
    }

    #[test]
    fn a_shift_marquee_adds_to_the_selection() {
        let (mut app, _id) = fixture();
        app.selection.shapes = vec![ShapeId { index: 9 }];
        app.selection.anchor = Some(ShapeId { index: 9 });
        let mut machine = ToolMachine::new();
        machine
            .handle(
                down(
                    Point::new(1.5, 1.5),
                    Modifiers {
                        shift: true,
                        ..Modifiers::none()
                    },
                ),
                &app,
            )
            .expect("down");
        machine
            .handle(
                moved(
                    Point::new(5.0, 5.0),
                    Modifiers {
                        shift: true,
                        ..Modifiers::none()
                    },
                ),
                &app,
            )
            .expect("move");
        let effect = machine
            .handle(
                up(
                    Point::new(5.0, 5.0),
                    Modifiers {
                        shift: true,
                        ..Modifiers::none()
                    },
                ),
                &app,
            )
            .expect("up");
        assert_eq!(
            select_command(&effect),
            Some(("add", vec!["1".to_owned()])),
            "the command adds the marquee hits; the registry merges them"
        );
    }

    #[test]
    fn zero_drags_emit_no_command() {
        let (mut app, id) = fixture();
        app.selection.shapes = vec![id];
        app.selection.anchor = Some(id);
        let frame = (
            Rect::new(Point::new(2.0, 2.5), Point::new(4.0, 3.5)),
            Angle::default(),
        );
        assert!(
            drag_intent(
                &app,
                None,
                HandleKind::Move,
                Point::new(3.0, 3.0),
                Point::new(3.0, 3.0),
                frame,
                &[id],
                true,
                1.0
            )
            .expect("no error")
            .is_none()
        );
        assert!(
            drag_intent(
                &app,
                None,
                HandleKind::Resize(ResizeEdge::East),
                Point::new(4.0, 3.0),
                Point::new(4.0, 3.0),
                frame,
                &[id],
                true,
                1.0
            )
            .expect("no error")
            .is_none()
        );
        assert!(
            drag_intent(
                &app,
                None,
                HandleKind::Rotate,
                Point::new(3.0, 3.75),
                Point::new(3.0, 3.75),
                frame,
                &[id],
                true,
                1.0
            )
            .expect("no error")
            .is_none()
        );
        assert!(
            drag_intent(
                &app,
                None,
                HandleKind::Move,
                Point::new(3.0, 3.0),
                Point::new(3.5, 3.0),
                frame,
                &[],
                true,
                1.0
            )
            .expect("no error")
            .is_none()
        );
    }

    #[test]
    fn every_resize_edge_changes_its_axis() {
        let bounds = Rect::new(Point::new(0.0, 0.0), Point::new(2.0, 1.0));
        let start = Point::new(1.0, 0.5);
        assert_eq!(
            resized_size(bounds, start, Point::new(3.0, 2.0), ResizeEdge::NorthEast),
            (4.0, 2.5)
        );
        assert_eq!(
            resized_size(bounds, start, Point::new(3.0, 2.0), ResizeEdge::SouthWest),
            (0.0_f64.max(MIN_SIZE_IN), 0.0_f64.max(MIN_SIZE_IN))
        );
        assert_eq!(
            resized_size(bounds, start, Point::new(1.0, 0.5), ResizeEdge::North),
            (2.0, 1.0)
        );
    }

    #[test]
    fn an_event_in_any_order_is_a_no_op() {
        let (app, _id) = fixture();
        let mut machine = ToolMachine::new();
        for event in [
            moved(Point::new(1.0, 1.0), Modifiers::none()),
            up(Point::new(1.0, 1.0), Modifiers::none()),
            ToolEvent::Cancel,
            ToolEvent::DoubleClick {
                pos: Point::new(99.0, 99.0),
            },
        ] {
            machine.handle(event, &app).expect("no error");
        }
        assert_eq!(anchor(&machine), None);
    }
}
