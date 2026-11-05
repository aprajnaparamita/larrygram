//! The Connection Point tool (task P7-34 item 2, ADR 0084 rule 7).
//!
//! With the tool armed and a shape selected:
//!
//! - Ctrl+click on the shape adds a point at that local position
//!   (`connector.point_add`) and selects it;
//! - Ctrl+drag moves the selected point (`connector.point_move`);
//! - click selects a point, and the shell's Delete routing removes it
//!   (`connector.point_delete`).
//!
//! The tool shows the selected shape's connection points as P7-33's
//! markers and the selected point in its own style
//! ([`larrygram_render::OverlayMark::ConnectionPoints`]). The tool is
//! read-only over `AppCtx`: it emits intents and overlay marks, never
//! a model mutation (ADR 0042).

use larrygram_commands::{AppCtx, ArgValue, Args, CommandId, Invocation, Source};
use larrygram_geom::Point;
use larrygram_model::ShapeId;
use larrygram_render::{HitOptions, Overlay, OverlayMark, hit_test};

use super::{Modifiers, ToolEffect, ToolEvent, ToolMachine};
use crate::UiError;
use crate::handles::{is_within, node_entries, scene_for};

/// The hit tolerance for the shape's points and outline, in screen
/// pixels.
const TOLERANCE_PX: f64 = 6.0;

/// The smallest move that drags a point, in inches (one screen pixel
/// at 96 ppi): below it a Ctrl+drag is a click.
const MIN_DRAG_IN: f64 = 1.0 / 96.0;

/// The tool's state (owner: task P7-34; opaque to other tools).
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct State {
    /// The selected point: the shape and its row index.
    selected: Option<(ShapeId, u32)>,
    /// The point being dragged (task P7-34 item 2).
    moving: Option<Moving>,
    /// The press of an add gesture (Ctrl on the shape): the point
    /// lands on release when the drag stayed a click.
    pending_add: Option<Point>,
}

/// The move drag of one point: where it started and where the pointer
/// is now (the preview's marker).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Moving {
    /// The shape whose point moves.
    shape: ShapeId,
    /// The point's row index.
    ix: u32,
    /// The press position (page inches): the drag threshold compares
    /// the release against it.
    press: Point,
    /// The current position (page inches).
    at: Point,
}

/// The point under `pos` (page inches) on the target shape's points,
/// within the hit tolerance: the shell's right-click routing (task
/// P7-34 item 3).
pub(crate) fn point_under(ctx: &AppCtx, pos: Point, zoom: f64) -> Option<(ShapeId, u32, Point)> {
    let shape = target_shape(ctx)?;
    hovered_point(ctx, shape, pos, zoom)
}

/// The selected point (the shell's Delete routing, context menu and
/// checked state read it).
pub(crate) fn selected(state: &ToolMachine) -> Option<(ShapeId, u32)> {
    state.connection_point.selected
}

/// Selects one point (the shell's context-menu routing: a right-click
/// on a point selects it before the menu opens).
pub(crate) fn select(state: &mut ToolMachine, shape: ShapeId, ix: u32) {
    state.connection_point.selected = Some((shape, ix));
    state.connection_point.moving = None;
}

/// Handles one event for the Connection Point tool (task P7-34 item
/// 2, ADR 0042).
pub fn handle(
    state: &mut ToolMachine,
    event: &ToolEvent,
    ctx: &AppCtx,
) -> Result<ToolEffect, UiError> {
    if ctx
        .active_document()
        .and_then(|session| session.active_page)
        .is_none()
    {
        state.connection_point = State::default();
        return Ok(ToolEffect::new());
    }
    let zoom = zoom_of(ctx);
    match event {
        ToolEvent::PointerDown { pos, modifiers } => down(state, ctx, *pos, *modifiers, zoom),
        ToolEvent::PointerMove { pos, .. } => moved(state, ctx, *pos),
        ToolEvent::PointerUp { pos, .. } => up(state, ctx, *pos),
        ToolEvent::DoubleClick { .. } => Ok(ToolEffect::new()),
        ToolEvent::Cancel => {
            state.connection_point.moving = None;
            Ok(ToolEffect::new())
        }
    }
}

/// The overlay while the tool is idle (task P7-34 item 2): the
/// selected shape's points as markers and the selected point's own
/// style.
pub fn idle_overlay(state: &ToolMachine, ctx: &AppCtx) -> Result<Option<Overlay>, UiError> {
    Ok(Some(overlay(state, ctx)?))
}

/// One press (task P7-34 item 2):
///
/// - Ctrl on the selected point starts the move drag;
/// - Ctrl on the shape arms the add gesture (the point lands on
///   release, so a drag adds nothing);
/// - a plain press on a point selects it and starts its move drag (a
///   click without a drag only selects; the user's 2026-11-05 pass:
///   a point is dragged directly, as in Visio); off the
///   points it selects the shape under the pointer (as the Pointer
///   tool's first click does), or clears the selection on empty page.
fn down(
    state: &mut ToolMachine,
    ctx: &AppCtx,
    pos: Point,
    modifiers: Modifiers,
    zoom: f64,
) -> Result<ToolEffect, UiError> {
    let shape = target_shape(ctx);
    if modifiers.ctrl {
        if let Some(shape) = shape {
            // Ctrl+drag moves the selected point (task P7-34 item 2).
            if let Some((_, ix, at)) = hovered_point(ctx, shape, pos, zoom)
                && state.connection_point.selected == Some((shape, ix))
            {
                state.connection_point.moving = Some(Moving {
                    shape,
                    ix,
                    press: pos,
                    at,
                });
                return overlay_effect(state, ctx);
            }
            // Ctrl+click on the shape adds a point; the point lands on
            // release, so a drag adds nothing.
            if hits_shape(ctx, shape, pos, zoom) {
                state.connection_point.pending_add = Some(pos);
                return overlay_effect(state, ctx);
            }
        }
        return Ok(ToolEffect::new());
    }
    match shape.and_then(|shape| hovered_point(ctx, shape, pos, zoom)) {
        Some((shape, ix, at)) => {
            state.connection_point.selected = Some((shape, ix));
            state.connection_point.moving = Some(Moving {
                shape,
                ix,
                press: pos,
                at,
            });
        }
        None => {
            state.connection_point.selected = None;
            state.connection_point.moving = None;
            // The tool picks the shape it works on with a plain click
            // (the user's 2026-11-05 pass: no shape could be selected
            // while the tool was on).
            let under = shape_under(ctx, pos, zoom);
            if under != shape || (under.is_none() && !ctx.selection.shapes.is_empty()) {
                let mut effect = overlay_effect(state, ctx)?;
                effect.intents.push(select_intent(ctx, under)?);
                return Ok(effect);
            }
        }
    }
    overlay_effect(state, ctx)
}

/// The top-level shape under `pos` (page inches): the topmost hit,
/// lifted to its top-level group like the Pointer tool's first click.
fn shape_under(ctx: &AppCtx, pos: Point, zoom: f64) -> Option<ShapeId> {
    let session = ctx.active_document()?;
    let scene = scene_for(ctx).ok().flatten()?;
    let tolerance = TOLERANCE_PX / (zoom.max(f64::MIN_POSITIVE) * POINTS_PER_INCH);
    let hits = hit_test(&scene, pos, &HitOptions::new(tolerance)).ok()?;
    let mut shape = hits.iter().find_map(|hit| hit.shape_id)?;
    // Bounded: a malformed parent chain cannot loop.
    for _ in 0..64 {
        match session.document.shape(shape)?.parent {
            Some(parent) => shape = parent,
            None => break,
        }
    }
    Some(shape)
}

/// The `edit.select` intent that makes `shape` the selection, or
/// clears it for `None`.
fn select_intent(ctx: &AppCtx, shape: Option<ShapeId>) -> Result<crate::Intent, UiError> {
    let ids = shape
        .and_then(|shape| ctx.active_document()?.document.shape(shape))
        .map(|entry| vec![entry.visio_id.to_string()])
        .unwrap_or_default();
    let command = CommandId::parse("edit.select").map_err(|error| {
        UiError::Command(larrygram_commands::CommandError::failed(error.to_string()))
    })?;
    let args = Args::new()
        .with("shapes", ArgValue::List(ids))
        .with("mode", ArgValue::Text("replace".to_owned()));
    Ok(crate::Intent::new(
        command,
        Invocation::new(args, Source::Ui),
    ))
}

/// One move: the move drag follows the pointer.
fn moved(state: &mut ToolMachine, ctx: &AppCtx, pos: Point) -> Result<ToolEffect, UiError> {
    if let Some(moving) = &mut state.connection_point.moving {
        moving.at = pos;
    }
    overlay_effect(state, ctx)
}

/// One release (task P7-34 item 2):
///
/// - a move drag emits one `connector.point_move` at the local point;
/// - an add gesture emits one `connector.point_add` at the local
///   point and selects the new row.
fn up(state: &mut ToolMachine, ctx: &AppCtx, pos: Point) -> Result<ToolEffect, UiError> {
    let mut effect = overlay_effect(state, ctx)?;
    if let Some(moving) = state.connection_point.moving.take() {
        if ((moving.press.x - pos.x).abs() >= MIN_DRAG_IN
            || (moving.press.y - pos.y).abs() >= MIN_DRAG_IN)
            && let Some(intent) = move_intent(ctx, moving.shape, moving.ix, pos)?
        {
            effect.intents.push(intent);
        }
        return Ok(effect);
    }
    if let Some(start) = state.connection_point.pending_add.take()
        && (start.x - pos.x).abs() < MIN_DRAG_IN
        && (start.y - pos.y).abs() < MIN_DRAG_IN
        && let Some(shape) = target_shape(ctx)
        && let Some(intent) = add_intent(ctx, shape, pos)?
    {
        let ix = ctx
            .active_document()
            .map(|session| session.document.next_connection_point_index(shape))
            .unwrap_or(0);
        state.connection_point.selected = Some((shape, ix));
        effect.intents.push(intent);
    }
    Ok(effect)
}

/// The tool's marker overlay: the selected shape's points and the
/// selected point's own style (task P7-34 item 2).
fn overlay(state: &ToolMachine, ctx: &AppCtx) -> Result<Overlay, UiError> {
    // The selection shows as it does under the Pointer tool, so the
    // shape the tool works on is visible (a connector has no points).
    let mut overlay = crate::handles::selection_overlay(ctx, None, &ctx.selection.shapes)?;
    let Some(shape) = target_shape(ctx) else {
        return Ok(overlay);
    };
    let Some(points) = shape_points(ctx, shape) else {
        return Ok(overlay);
    };
    let selected = match state.connection_point.moving {
        Some(moving) => Some(moving.at),
        None => state
            .connection_point
            .selected
            .and_then(|(selected_shape, ix)| {
                (selected_shape == shape)
                    .then(|| points.iter().find(|(row, _)| *row == ix))
                    .flatten()
                    .map(|(_, at)| *at)
            }),
    };
    overlay.marks.push(OverlayMark::ConnectionPoints {
        points: points.into_iter().map(|(_, at)| at).collect(),
        selected,
    });
    Ok(overlay)
}

/// [`overlay`] as a [`ToolEffect`].
fn overlay_effect(state: &ToolMachine, ctx: &AppCtx) -> Result<ToolEffect, UiError> {
    Ok(ToolEffect {
        intents: Vec::new(),
        overlay: overlay(state, ctx)?,
    })
}

/// The shape the tool acts on: the selection anchor, or the first
/// selected shape.
fn target_shape(ctx: &AppCtx) -> Option<ShapeId> {
    let session = ctx.active_document()?;
    session.active_page?;
    ctx.selection
        .anchor
        .or_else(|| ctx.selection.shapes.first().copied())
        .filter(|shape| session.document.page_of(*shape).is_some())
}

/// The view check's marker mark for every shape on the active page
/// (task P7-34 item 1): P7-33's markers, no selection.
pub(crate) fn all_points_mark(ctx: &AppCtx) -> Option<OverlayMark> {
    let scene = scene_for(ctx).ok().flatten()?;
    let entries = node_entries(&scene);
    let session = ctx.active_document()?;
    let mut points = Vec::new();
    for entry in &entries {
        for (_, local) in session.document.connection_points_indexed(entry.shape) {
            let at = entry.transform.apply(local);
            if at.x.is_finite() && at.y.is_finite() {
                points.push(at);
            }
        }
    }
    (!points.is_empty()).then_some(OverlayMark::ConnectionPoints {
        points,
        selected: None,
    })
}

/// One shape's connection points in page space, with their row
/// indices.
fn shape_points(ctx: &AppCtx, shape: ShapeId) -> Option<Vec<(u32, Point)>> {
    let entry = entry_of(ctx, shape)?;
    let session = ctx.active_document()?;
    Some(
        session
            .document
            .connection_points_indexed(shape)
            .into_iter()
            .filter_map(|(ix, local)| {
                let at = entry.transform.apply(local);
                let ix = u32::try_from(ix).ok()?;
                (at.x.is_finite() && at.y.is_finite()).then_some((ix, at))
            })
            .collect(),
    )
}

/// The shape's scene entry (its page transform).
fn entry_of(ctx: &AppCtx, shape: ShapeId) -> Option<crate::handles::NodeEntry> {
    let scene = scene_for(ctx).ok().flatten()?;
    node_entries(&scene)
        .into_iter()
        .find(|entry| entry.shape == shape)
}

/// The point under `pos` (page inches) on the shape's points, within
/// the hit tolerance.
fn hovered_point(
    ctx: &AppCtx,
    shape: ShapeId,
    pos: Point,
    zoom: f64,
) -> Option<(ShapeId, u32, Point)> {
    let tolerance = TOLERANCE_PX / (zoom.max(f64::MIN_POSITIVE) * POINTS_PER_INCH);
    let points = shape_points(ctx, shape)?;
    points
        .into_iter()
        .filter(|(_, at)| (at.x - pos.x).abs() <= tolerance && (at.y - pos.y).abs() <= tolerance)
        .min_by(|(_, a), (_, b)| {
            let da = (a.x - pos.x).hypot(a.y - pos.y);
            let db = (b.x - pos.x).hypot(b.y - pos.y);
            da.total_cmp(&db)
        })
        .map(|(ix, at)| (shape, ix, at))
}

/// Whether `pos` hits `shape` (its own shape or one of its members).
fn hits_shape(ctx: &AppCtx, shape: ShapeId, pos: Point, zoom: f64) -> bool {
    let Some(session) = ctx.active_document() else {
        return false;
    };
    let Some(scene) = scene_for(ctx).ok().flatten() else {
        return false;
    };
    let tolerance = TOLERANCE_PX / (zoom.max(f64::MIN_POSITIVE) * POINTS_PER_INCH);
    let Ok(hits) = hit_test(&scene, pos, &HitOptions::new(tolerance)) else {
        return false;
    };
    hits.iter()
        .filter_map(|hit| hit.shape_id)
        .any(|hit| hit == shape || is_within(&session.document, hit, std::slice::from_ref(&shape)))
}

/// The `connector.point_add` intent at the local point of `pos`.
fn add_intent(ctx: &AppCtx, shape: ShapeId, pos: Point) -> Result<Option<crate::Intent>, UiError> {
    let Some(local) = local_of(ctx, shape, pos)? else {
        return Ok(None);
    };
    let Some(visio_id) = visio_id_of(ctx, shape) else {
        return Ok(None);
    };
    Ok(Some(intent(
        "connector.point_add",
        Args::new()
            .with("shape", ArgValue::Integer(i64::from(visio_id)))
            .with("x", ArgValue::Number(local.x))
            .with("y", ArgValue::Number(local.y)),
    )?))
}

/// The `connector.point_move` intent of one completed drag.
fn move_intent(
    ctx: &AppCtx,
    shape: ShapeId,
    ix: u32,
    pos: Point,
) -> Result<Option<crate::Intent>, UiError> {
    let Some(local) = local_of(ctx, shape, pos)? else {
        return Ok(None);
    };
    let Some(visio_id) = visio_id_of(ctx, shape) else {
        return Ok(None);
    };
    Ok(Some(intent(
        "connector.point_move",
        Args::new()
            .with("shape", ArgValue::Integer(i64::from(visio_id)))
            .with("point", ArgValue::Integer(i64::from(ix)))
            .with("x", ArgValue::Number(local.x))
            .with("y", ArgValue::Number(local.y)),
    )?))
}

/// The local coordinates of a page point on the shape.
fn local_of(ctx: &AppCtx, shape: ShapeId, pos: Point) -> Result<Option<Point>, UiError> {
    let Some(entry) = entry_of(ctx, shape) else {
        return Ok(None);
    };
    let inverse = entry
        .transform
        .invert()
        .map_err(|error| UiError::Geometry {
            message: error.to_string(),
        })?;
    Ok(Some(inverse.apply(pos)))
}

/// The Visio `ID` of a shape.
fn visio_id_of(ctx: &AppCtx, shape: ShapeId) -> Option<i32> {
    ctx.active_document()
        .and_then(|session| session.document.shape(shape))
        .map(|entity| entity.visio_id)
}

/// The command intent of one tool action (the connector tool's
/// spelling of an intent).
fn intent(id: &'static str, args: Args) -> Result<crate::Intent, UiError> {
    let command = CommandId::parse(id).map_err(|error| UiError::Io {
        message: format!("the tool command id {id} is not valid: {error}"),
    })?;
    Ok(crate::Intent::new(
        command,
        Invocation::new(args, Source::Ui),
    ))
}

/// The view scale of the active document (the connector tool's rule).
fn zoom_of(ctx: &AppCtx) -> f64 {
    ctx.active_document()
        .map(|session| session.view.zoom)
        .filter(|zoom| zoom.is_finite() && *zoom > 0.0)
        .unwrap_or(1.0)
}

/// The screen points of one inch.
const POINTS_PER_INCH: f64 = 72.0;

#[cfg(test)]
mod tests {
    use larrygram_model::{CellName, Document, Row, Shape};

    use super::*;

    fn app_with_shape() -> (AppCtx, ShapeId) {
        let mut doc = Document::new();
        let page = doc.add_page("Page-1").expect("page");
        let shape = Shape::new(ShapeId { index: 0 }, 1);
        let shape = doc.insert_shape(page, shape, None).expect("insert");
        let mut session = larrygram_commands::DocumentSession::new(doc, None);
        session.active_page = Some(page);
        let mut ctx = AppCtx::new(larrygram_commands::Settings::default());
        ctx.open_document(session);
        ctx.selection.shapes = vec![shape];
        ctx.selection.anchor = Some(shape);
        (ctx, shape)
    }

    /// With no document the tool is a no-op and never panics.
    #[test]
    fn the_tool_is_a_no_op_without_a_document() {
        let ctx = AppCtx::new(larrygram_commands::Settings::default());
        let mut machine = ToolMachine::new();
        for event in [
            ToolEvent::PointerDown {
                pos: Point::new(1.0, 1.0),
                modifiers: Modifiers {
                    ctrl: true,
                    ..Modifiers::none()
                },
            },
            ToolEvent::PointerMove {
                pos: Point::new(2.0, 2.0),
                modifiers: Modifiers::none(),
            },
            ToolEvent::PointerUp {
                pos: Point::new(2.0, 2.0),
                modifiers: Modifiers::none(),
            },
            ToolEvent::Cancel,
        ] {
            let effect = handle(&mut machine, &event, &ctx).expect("no error");
            assert!(effect.intents.is_empty());
        }
        assert_eq!(selected(&machine), None);
    }

    /// A literal numeric cell for the fixtures.
    fn cell(name: &str, value: f64) -> larrygram_model::Cell {
        larrygram_model::Cell {
            name: CellName::new(name),
            formula: Some(format!("{value}")),
            value: Some(larrygram_model::CellValue::Number(value)),
            unit: larrygram_geom::Unit::Inches,
            preserved: larrygram_model::Preserved::new(),
        }
    }

    /// A drawable 2 x 1 box at (3, 3) with two connection points (the
    /// local `(0.25, 0.25)` and `(1.75, 0.25)`, i.e. page
    /// `(2.25, 2.75)` and `(3.75, 2.75)`).
    fn app_with_points() -> (AppCtx, ShapeId) {
        let mut doc = Document::new();
        let page = doc.add_page("Page-1").expect("page");
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
            cell("LinePattern", 1.0),
        ];
        let mut geometry =
            larrygram_model::Section::new(larrygram_model::SectionName::from_static("Geometry"));
        for (ix, (kind, px, py)) in [
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
            row.cells = vec![cell("X", px), cell("Y", py)];
            geometry.rows.push(row);
        }
        shape.sheet.sections.push(geometry);
        let mut connection =
            larrygram_model::Section::new(larrygram_model::SectionName::from_static("Connection"));
        for (ix, (px, py)) in [(0.25, 0.25), (1.75, 0.25)].into_iter().enumerate() {
            let mut row = Row::new();
            row.row_type = Some("Connection".to_owned());
            row.ix = Some(i32::try_from(ix).unwrap_or(0));
            row.cells = vec![cell("X", px), cell("Y", py)];
            connection.rows.push(row);
        }
        shape.sheet.sections.push(connection);
        let shape = doc.insert_shape(page, shape, None).expect("insert");
        let mut session = larrygram_commands::DocumentSession::new(doc, None);
        session.active_page = Some(page);
        let mut ctx = AppCtx::new(larrygram_commands::Settings::default());
        ctx.open_document(session);
        ctx.selection.shapes = vec![shape];
        ctx.selection.anchor = Some(shape);
        (ctx, shape)
    }

    /// A shape with no points has no markers, and the state helpers
    /// read and write the selection.
    #[test]
    fn a_shape_without_points_has_no_markers() {
        let (ctx, shape) = app_with_shape();
        let mut machine = ToolMachine::new();
        let effect = handle(
            &mut machine,
            &ToolEvent::PointerMove {
                pos: Point::new(1.0, 1.0),
                modifiers: Modifiers::none(),
            },
            &ctx,
        )
        .expect("no error");
        assert!(effect.overlay.is_empty(), "no points, no markers");
        select(&mut machine, shape, 3);
        assert_eq!(selected(&machine), Some((shape, 3)));
        assert_eq!(all_points_mark(&ctx), None);
    }

    /// Task P7-34 item 2: the quiet edges — a double click and Cancel
    /// do nothing, a plain click off the points clears the selection
    /// and a Ctrl press off the shape arms no gesture.
    #[test]
    fn the_tool_edges_are_quiet() {
        let (ctx, shape) = app_with_points();
        let mut machine = ToolMachine::new();
        machine.tool = crate::tools::Tool::ConnectionPoint;
        select(&mut machine, shape, 1);
        // A double click changes nothing.
        let effect = handle(
            &mut machine,
            &ToolEvent::DoubleClick {
                pos: Point::new(2.25, 2.75),
            },
            &ctx,
        )
        .expect("no error");
        assert!(effect.intents.is_empty());
        assert_eq!(selected(&machine), Some((shape, 1)));
        // Cancel drops the drag in flight.
        machine.connection_point.moving = Some(Moving {
            shape,
            ix: 1,
            press: Point::new(1.0, 1.0),
            at: Point::new(1.0, 1.0),
        });
        handle(&mut machine, &ToolEvent::Cancel, &ctx).expect("no error");
        assert!(machine.connection_point.moving.is_none());
        // A plain click off the points clears the selection.
        handle(
            &mut machine,
            &ToolEvent::PointerDown {
                pos: Point::new(5.0, 5.0),
                modifiers: Modifiers::none(),
            },
            &ctx,
        )
        .expect("no error");
        assert_eq!(selected(&machine), None);
        // A Ctrl press off the shape arms no gesture.
        handle(
            &mut machine,
            &ToolEvent::PointerDown {
                pos: Point::new(9.0, 9.0),
                modifiers: Modifiers {
                    ctrl: true,
                    ..Modifiers::none()
                },
            },
            &ctx,
        )
        .expect("no error");
        handle(
            &mut machine,
            &ToolEvent::PointerUp {
                pos: Point::new(9.0, 9.0),
                modifiers: Modifiers::none(),
            },
            &ctx,
        )
        .expect("no error");
        assert_eq!(selected(&machine), None);
    }

    /// The point under the pointer is the nearest one in tolerance,
    /// and the shape hit test sees the box (task P7-34 items 2 and 3).
    #[test]
    fn the_point_under_the_pointer_is_the_nearest() {
        let (ctx, shape) = app_with_points();
        let near = point_under(&ctx, Point::new(2.28, 2.72), 1.0);
        assert_eq!(
            near.map(|(shape, ix, _)| (shape, ix)),
            Some((shape, 0)),
            "the nearest point within tolerance"
        );
        assert_eq!(
            point_under(&ctx, Point::new(3.0, 5.0), 1.0),
            None,
            "beyond tolerance there is no point"
        );
        assert!(
            hits_shape(&ctx, shape, Point::new(3.0, 3.0), 1.0),
            "the press hits the box"
        );
        assert!(!hits_shape(&ctx, shape, Point::new(8.0, 8.0), 1.0));
    }

    /// The overlay marks the shape's points and the selected one's own
    /// style; with no shape selected it draws nothing (task P7-34
    /// items 1 and 2).
    #[test]
    fn the_overlay_marks_the_points_and_the_selection() {
        let (ctx, shape) = app_with_points();
        let mut machine = ToolMachine::new();
        machine.tool = crate::tools::Tool::ConnectionPoint;
        select(&mut machine, shape, 0);
        let effect = handle(
            &mut machine,
            &ToolEvent::PointerMove {
                pos: Point::new(3.0, 3.0),
                modifiers: Modifiers::none(),
            },
            &ctx,
        )
        .expect("no error");
        // The selection's own marks come first; the points last.
        assert!(effect.overlay.marks.len() > 1, "{:?}", effect.overlay.marks);
        match effect.overlay.marks.last() {
            Some(OverlayMark::ConnectionPoints { points, selected }) => {
                assert_eq!(points.len(), 2);
                assert_eq!(*selected, Some(Point::new(2.25, 2.75)));
            }
            other => panic!("expected connection points, got {other:?}"),
        }
        // With no shape selected the overlay is empty.
        let mut bare = AppCtx::new(larrygram_commands::Settings::default());
        bare.open_document(larrygram_commands::DocumentSession::new(
            Document::new(),
            None,
        ));
        let overlay = overlay(&machine, &bare).expect("no error");
        assert!(overlay.is_empty());
    }

    /// A stale shape produces no intents (task P7-34 item 2).
    #[test]
    fn intents_for_stale_shapes_are_absent() {
        let (ctx, _) = app_with_points();
        let ghost = ShapeId { index: 99 };
        assert_eq!(
            add_intent(&ctx, ghost, Point::new(1.0, 1.0)).expect("no error"),
            None
        );
        assert_eq!(
            move_intent(&ctx, ghost, 0, Point::new(1.0, 1.0)).expect("no error"),
            None
        );
    }
}
