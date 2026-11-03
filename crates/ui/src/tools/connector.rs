//! The dynamic connector tool (ADR 0045).
//!
//! A drag previews the routed path (`larrygram_geom::routing`), snaps and
//! glues both ends to connection points or shape outlines, and commits
//! one `insert.shape` intent followed by the `connector.glue_*` intents
//! (the new shape is the selection anchor, so those intents name no
//! connector id). The tool is read-only over `AppCtx`: it emits intents
//! and overlay marks, never a model mutation (ADR 0042).

use larrygram_commands::{AppCtx, ArgValue, Args, CommandId, Invocation, Source};
use larrygram_geom::{Point, RouteOptions, RouteStyle, route};
use larrygram_model::stencils;
use larrygram_render::{HitOptions, Overlay, OverlayMark, hit_test};

use super::{Modifiers, ToolEffect, ToolEvent, ToolMachine};
use crate::UiError;
use crate::handles::{node_entries, render_error, scene_for};

/// The snap distance for connector endpoints, in screen pixels (ADR
/// 0043's default `Snapper` distance).
const SNAP_DISTANCE_PX: f64 = 8.0;

/// The smallest drag that creates a connector, in inches (one screen
/// pixel at 96 ppi).
const MIN_DRAG_IN: f64 = 1.0 / 96.0;

/// A glue target of one connector endpoint: a shape and, for a
/// connection-point glue, its `Connection` row `IX`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Glue {
    /// The target shape's Visio `ID`.
    visio_id: i32,
    /// The connection row `IX`, or `None` for outline glue.
    point: Option<i32>,
}

/// What the connector tool is doing between events.
#[derive(Debug, Clone, Default, PartialEq)]
enum Mode {
    /// Nothing pressed.
    #[default]
    Idle,
    /// A connector drag from `start` to `current`.
    Dragging {
        /// The snapped start point.
        start: Point,
        /// The start's glue target.
        start_glue: Option<Glue>,
        /// The snapped current point.
        current: Point,
        /// The current end's glue target.
        end_glue: Option<Glue>,
    },
}

/// The connector tool's state (owner: P5-08; opaque to other tools).
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct ConnectorState {
    /// The current gesture.
    mode: Mode,
}

/// Handles one event for the connector tool (owner: P5-08, ADR 0042).
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
        state.connector.mode = Mode::Idle;
        return Ok(ToolEffect::new());
    }
    let zoom = ctx
        .active_document()
        .map(|session| session.view.zoom)
        .filter(|zoom| zoom.is_finite() && *zoom > 0.0)
        .unwrap_or(1.0);
    match event {
        ToolEvent::PointerDown { pos, modifiers } => down(state, ctx, *pos, *modifiers, zoom),
        ToolEvent::PointerMove { pos, modifiers } => moved(state, ctx, *pos, *modifiers, zoom),
        ToolEvent::PointerUp { pos, modifiers } => up(state, ctx, *pos, *modifiers, zoom),
        ToolEvent::DoubleClick { .. } => Ok(ToolEffect::new()),
        ToolEvent::Cancel => {
            state.connector.mode = Mode::Idle;
            Ok(ToolEffect::new())
        }
    }
}

/// The drag anchor, in page inches (the preview's start).
pub fn anchor(state: &ToolMachine) -> Option<Point> {
    match &state.connector.mode {
        Mode::Idle => None,
        Mode::Dragging { start, .. } => Some(*start),
    }
}

/// One press: locate the start endpoint and begin the drag.
fn down(
    state: &mut ToolMachine,
    ctx: &AppCtx,
    pos: Point,
    modifiers: Modifiers,
    zoom: f64,
) -> Result<ToolEffect, UiError> {
    let (start, start_glue) = locate(ctx, pos, zoom, modifiers.alt)?;
    state.connector.mode = Mode::Dragging {
        start,
        start_glue,
        current: start,
        end_glue: None,
    };
    Ok(preview(state, RouteStyle::Straight))
}

/// One move: update the end endpoint and the routed preview.
fn moved(
    state: &mut ToolMachine,
    ctx: &AppCtx,
    pos: Point,
    modifiers: Modifiers,
    zoom: f64,
) -> Result<ToolEffect, UiError> {
    let Mode::Dragging {
        start, start_glue, ..
    } = state.connector.mode.clone()
    else {
        return Ok(ToolEffect::new());
    };
    let (current, end_glue) = locate(ctx, pos, zoom, modifiers.alt)?;
    state.connector.mode = Mode::Dragging {
        start,
        start_glue,
        current,
        end_glue,
    };
    Ok(preview(state, RouteStyle::Straight))
}

/// One release: commit `insert.shape` and the glue intents.
fn up(
    state: &mut ToolMachine,
    _ctx: &AppCtx,
    pos: Point,
    modifiers: Modifiers,
    zoom: f64,
) -> Result<ToolEffect, UiError> {
    let mode = std::mem::replace(&mut state.connector.mode, Mode::Idle);
    let Mode::Dragging {
        start,
        start_glue,
        current: _,
        end_glue: _,
    } = mode
    else {
        return Ok(ToolEffect::new());
    };
    let (current, end_glue) = locate(_ctx, pos, zoom, modifiers.alt)?;
    let delta = current.sub(start);
    if delta.x.hypot(delta.y) < MIN_DRAG_IN {
        return Ok(ToolEffect::new());
    }
    let Some((stencil, master)) = connector_master() else {
        return Err(UiError::Io {
            message: "the Basic Flowchart Connector master is missing".to_owned(),
        });
    };
    let mut args = Args::new()
        .with("stencil", ArgValue::Text(stencil.to_owned()))
        .with("master", ArgValue::Text(master.to_owned()))
        .with("x", ArgValue::Number(start.x))
        .with("y", ArgValue::Number(start.y));
    match start_glue {
        Some(glue) => {
            args.set("begin_target", ArgValue::Integer(i64::from(glue.visio_id)));
            if let Some(point) = glue.point {
                args.set("begin_point", ArgValue::Integer(i64::from(point)));
            }
        }
        None => {
            args.set("begin_x", ArgValue::Number(start.x));
            args.set("begin_y", ArgValue::Number(start.y));
        }
    }
    match end_glue {
        Some(glue) => {
            args.set("end_target", ArgValue::Integer(i64::from(glue.visio_id)));
            if let Some(point) = glue.point {
                args.set("end_point", ArgValue::Integer(i64::from(point)));
            }
        }
        None => {
            args.set("end_x", ArgValue::Number(current.x));
            args.set("end_y", ArgValue::Number(current.y));
        }
    }
    let mut effect = ToolEffect::new();
    effect.intents.push(intent("connector.draw", args)?);
    Ok(effect)
}

/// The routed preview overlay of the current drag.
fn preview(state: &ToolMachine, style: RouteStyle) -> ToolEffect {
    let Mode::Dragging {
        start,
        current,
        start_glue: _,
        end_glue: _,
    } = &state.connector.mode
    else {
        return ToolEffect::new();
    };
    let mut overlay = Overlay::new();
    if let Ok(points) = route(*start, *current, &RouteOptions::new(style))
        && points.len() >= 2
    {
        overlay.marks.push(OverlayMark::ConnectorPreview { points });
    }
    ToolEffect {
        intents: Vec::new(),
        overlay,
    }
}

/// The top-level ancestor of `shape` (the shape itself when it sits on
/// the page): a hit on a group member glues to its top-level ancestor,
/// as the render doc says (task P7-49 goal 8).
fn top_level(
    doc: &larrygram_model::Document,
    shape: larrygram_model::ShapeId,
) -> larrygram_model::ShapeId {
    let mut top = shape;
    for _ in 0..128 {
        let Some(parent) = doc.shape(top).and_then(|entity| entity.parent) else {
            break;
        };
        top = parent;
    }
    top
}

/// The glue rule of one endpoint (ADR 0084 rule 6): point glue within
/// the snap distance of a connection point, otherwise dynamic glue to
/// the topmost non-connector shape whose outline contains the pointer,
/// otherwise free. A tie between connection points goes to the
/// topmost shape. Point glue names the shape that owns the `Connection`
/// row (a group member's row glues to the member, task P7-50 goal 4);
/// dynamic glue on a group member maps to its top-level ancestor. The
/// outline test ignores fill: a shape with no fill glues inside its
/// outline (the old bounds test glued the corner area of a diamond's
/// box, task P7-50 minor 1), and a text block larger than its shape
/// does not glue outside the outline. Connectors are never glue
/// targets. Alt suppresses glue entirely.
fn locate(
    ctx: &AppCtx,
    pos: Point,
    zoom: f64,
    alt: bool,
) -> Result<(Point, Option<Glue>), UiError> {
    if alt {
        return Ok((pos, None));
    }
    let Some(scene) = scene_for(ctx)? else {
        return Ok((pos, None));
    };
    let Some(session) = ctx.active_document() else {
        return Ok((pos, None));
    };
    let doc = &session.document;
    let entries = node_entries(&scene);
    let max_distance = SNAP_DISTANCE_PX / (zoom * crate::canvas::POINTS_PER_INCH);
    let mut nearest: Option<(f64, Point, Glue)> = None;
    // Paint order reversed: a tie between connection points goes to
    // the topmost shape (task P7-49 goal 8).
    for entry in entries.iter().rev() {
        // Point glue names the shape that owns the `Connection` row
        // (task P7-50 goal 4): a group member's row glues to the
        // member, not to its top-level ancestor.
        let Some(owner) = doc.shape(entry.shape) else {
            continue;
        };
        if doc.is_connector(entry.shape) {
            continue;
        }
        for (ix, local) in doc.connection_points_indexed(entry.shape) {
            let point = entry.transform.apply(local);
            if !point.x.is_finite() || !point.y.is_finite() {
                continue;
            }
            let delta = point.sub(pos);
            let gap = delta.x.hypot(delta.y);
            if gap <= max_distance && nearest.as_ref().is_none_or(|(best, _, _)| gap < *best) {
                nearest = Some((
                    gap,
                    point,
                    Glue {
                        visio_id: owner.visio_id,
                        point: Some(ix),
                    },
                ));
            }
        }
    }
    if let Some((_, point, glue)) = nearest {
        return Ok((point, Some(glue)));
    }

    let snapped = {
        let snapper = crate::snap::Snapper::default();
        let result = snapper.snap_point(ctx, None, pos, zoom, false)?;
        result.point
    };
    // Dynamic glue: the topmost non-connector shape whose own outline
    // contains the pointer (fill ignored, task P7-50 minor 1). A hit
    // on a group member maps to its top-level ancestor.
    let hit_glue = |shape: larrygram_model::ShapeId| {
        let target = top_level(doc, shape);
        if doc.is_connector(target) {
            return None;
        }
        let entry = entries.iter().find(|entry| entry.shape == shape)?;
        if !inside_outline(&scene, entry, pos) {
            return None;
        }
        doc.shape(target).map(|found| Glue {
            visio_id: found.visio_id,
            point: None,
        })
    };
    let hits = hit_test(&scene, pos, &HitOptions::new(max_distance)).map_err(render_error)?;
    let mut glue = hits.iter().find_map(|entry| hit_glue(entry.shape_id?));
    if glue.is_none() {
        // The outline fallback: a press inside the outline of a shape
        // with no fill takes dynamic glue (the old bounds test).
        glue = entries.iter().rev().find_map(|entry| hit_glue(entry.shape));
    }
    Ok((snapped, glue))
}

/// Whether the pointer is inside the shape's own outline, ignoring
/// fill (task P7-50 minor 1): containment in the shape's drawable
/// path, tested in the shape's own frame.
fn inside_outline(
    scene: &larrygram_render::Scene,
    entry: &crate::handles::NodeEntry,
    pos: Point,
) -> bool {
    let Some(shape) = scene_shape(scene, entry.path.indices()) else {
        return false;
    };
    let Ok(inverse) = entry.transform.invert() else {
        return false;
    };
    larrygram_geom::path_contains(&shape.path, inverse.apply(pos)).unwrap_or(false)
}

/// The scene's shape node at `indices` (the [`NodePath`] rule), or
/// `None` for a group node or a path that does not resolve.
fn scene_shape<'a>(
    scene: &'a larrygram_render::Scene,
    indices: &[usize],
) -> Option<&'a larrygram_render::scene::Shape> {
    use larrygram_render::scene::Node;
    let (last, parents) = indices.split_last()?;
    let mut nodes = scene.nodes.as_slice();
    for index in parents {
        match nodes.get(*index)? {
            Node::Group(group) => nodes = group.children.as_slice(),
            _ => return None,
        }
    }
    match nodes.get(*last)? {
        Node::Shape(shape) => Some(shape.as_ref()),
        _ => None,
    }
}

/// The `(stencil, master)` names of the connector master in the Basic
/// Flowchart catalog (ADR 0034).
fn connector_master() -> Option<(&'static str, &'static str)> {
    for stencil in stencils::catalog() {
        for master in &stencil.masters {
            if master.name_u == "Connector" {
                return Some((stencil.name, master.name_u));
            }
        }
    }
    None
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
        Cell, CellName, CellValue, Document, Page, PageId, Preserved, Row, Section, SectionName,
        Shape, ShapeId,
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

    fn rectangle(id: ShapeId, visio_id: i32, pin: (f64, f64), points: &[(f64, f64)]) -> Shape {
        let mut shape = Shape::new(id, visio_id);
        shape.sheet.cells = vec![
            cell("Width", 2.0),
            cell("Height", 1.0),
            cell("PinX", pin.0),
            cell("PinY", pin.1),
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
        let mut connection = Section::new(SectionName::from_static("Connection"));
        for (ix, (x, y)) in points.iter().enumerate() {
            let mut row = Row::new();
            row.row_type = Some("Connection".to_owned());
            row.ix = Some(i32::try_from(ix).unwrap_or(0));
            row.cells = vec![cell("X", *x), cell("Y", *y)];
            connection.rows.push(row);
        }
        shape.sheet.sections.push(connection);
        shape
    }

    fn fixture() -> AppCtx {
        let mut doc = Document::new();
        let mut page = Page::new(PageId { index: 0 }, 1);
        page.sheet.cells = vec![cell("PageWidth", 10.0), cell("PageHeight", 8.0)];
        for (visio_id, pin, points) in [
            (1, (2.0, 3.0), vec![(0.0, 0.5), (2.0, 0.5)]),
            (2, (7.0, 3.0), vec![(0.0, 0.5), (2.0, 0.5)]),
        ] {
            let shape = rectangle(ShapeId { index: 0 }, visio_id, pin, &points);
            let id = doc.add_shape(shape);
            page.shapes.push(id);
        }
        doc.pages.push(page);
        let mut ctx = AppCtx::new(Settings::default());
        ctx.open_document(DocumentSession::new(doc, None));
        ctx
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

    fn draw_intent(effect: &ToolEffect) -> Option<&crate::Intent> {
        effect
            .intents
            .iter()
            .find(|intent| intent.command.as_str() == "connector.draw")
    }

    #[test]
    fn a_drag_between_connection_points_emits_one_draw_command() {
        let app = fixture();
        let mut machine = ToolMachine::new();
        machine.tool = super::super::Tool::Connector;
        // Left shape connection point 0 is local (0, 0.5) at page (1, 3).
        let effect = machine
            .handle(down(Point::new(1.0, 3.0), Modifiers::none()), &app)
            .expect("down");
        assert!(effect.intents.is_empty());
        let effect = machine
            .handle(moved(Point::new(6.0, 3.0), Modifiers::none()), &app)
            .expect("move");
        assert!(matches!(
            effect.overlay.marks.first(),
            Some(OverlayMark::ConnectorPreview { .. })
        ));
        // Right shape connection point 0 is local (0, 0.5) at page (6, 3).
        let effect = machine
            .handle(up(Point::new(6.0, 3.0), Modifiers::none()), &app)
            .expect("up");
        assert_eq!(effect.intents.len(), 1, "{:?}", effect.intents);
        let draw = draw_intent(&effect).expect("connector.draw");
        assert_eq!(
            draw.invocation.args.text("stencil"),
            Some("Basic Flowchart")
        );
        assert_eq!(draw.invocation.args.text("master"), Some("Connector"));
        assert_eq!(draw.invocation.args.number("x"), Some(1.0));
        assert_eq!(draw.invocation.args.number("y"), Some(3.0));
        assert_eq!(draw.invocation.args.integer("begin_target"), Some(1));
        assert_eq!(draw.invocation.args.integer("begin_point"), Some(0));
        assert_eq!(draw.invocation.args.integer("end_target"), Some(2));
        assert_eq!(draw.invocation.args.integer("end_point"), Some(0));
        assert_eq!(draw.invocation.args.number("end_x"), None);
    }

    #[test]
    fn a_drag_from_an_outline_carries_the_outline_glue() {
        let app = fixture();
        let mut machine = ToolMachine::new();
        machine.tool = super::super::Tool::Connector;
        // The left shape's middle is (2, 3), away from its two points.
        machine
            .handle(down(Point::new(2.0, 3.0), Modifiers::none()), &app)
            .expect("down");
        let effect = machine
            .handle(up(Point::new(6.5, 3.4), Modifiers::none()), &app)
            .expect("up");
        let draw = draw_intent(&effect).expect("connector.draw");
        assert_eq!(draw.invocation.args.integer("begin_target"), Some(1));
        assert_eq!(draw.invocation.args.integer("begin_point"), None);
        assert_eq!(draw.invocation.args.integer("end_target"), Some(2));
        assert_eq!(draw.invocation.args.integer("end_point"), None);
    }

    #[test]
    fn a_degenerate_or_cancelled_drag_emits_nothing() {
        let app = fixture();
        let mut machine = ToolMachine::new();
        machine.tool = super::super::Tool::Connector;
        machine
            .handle(down(Point::new(1.0, 3.0), Modifiers::none()), &app)
            .expect("down");
        let effect = machine
            .handle(up(Point::new(1.0, 3.0), Modifiers::none()), &app)
            .expect("up");
        assert!(effect.intents.is_empty());
        assert_eq!(anchor(&machine), None);

        machine
            .handle(down(Point::new(1.0, 3.0), Modifiers::none()), &app)
            .expect("down");
        assert_eq!(anchor(&machine), Some(Point::new(1.0, 3.0)));
        let effect = machine.handle(ToolEvent::Cancel, &app).expect("cancel");
        assert!(effect.intents.is_empty());
        assert_eq!(anchor(&machine), None);
    }

    #[test]
    fn alt_turns_connector_snapping_off() {
        let app = fixture();
        let mut machine = ToolMachine::new();
        machine.tool = super::super::Tool::Connector;
        let alt = Modifiers {
            alt: true,
            ..Modifiers::none()
        };
        machine
            .handle(down(Point::new(1.02, 3.0), alt), &app)
            .expect("down");
        let effect = machine
            .handle(up(Point::new(6.01, 3.0), alt), &app)
            .expect("up");
        assert_eq!(effect.intents.len(), 1, "no glue without snapping");
        let draw = draw_intent(&effect).expect("connector.draw");
        assert_eq!(draw.invocation.args.number("x"), Some(1.02));
        assert_eq!(draw.invocation.args.integer("begin_target"), None);
        assert_eq!(draw.invocation.args.number("begin_x"), Some(1.02));
        assert_eq!(draw.invocation.args.integer("end_target"), None);
        assert_eq!(draw.invocation.args.number("end_x"), Some(6.01));
    }

    #[test]
    fn events_without_a_page_or_in_any_order_are_no_ops() {
        let ctx = AppCtx::new(Settings::default());
        let mut machine = ToolMachine::new();
        machine.tool = super::super::Tool::Connector;
        for event in [
            moved(Point::new(1.0, 1.0), Modifiers::none()),
            up(Point::new(1.0, 1.0), Modifiers::none()),
            ToolEvent::Cancel,
            ToolEvent::DoubleClick {
                pos: Point::new(1.0, 1.0),
            },
        ] {
            machine.handle(event, &ctx).expect("no error");
        }
        assert_eq!(anchor(&machine), None);
    }

    /// Drags from `from` to `to` with the Connector tool and returns
    /// the draw intent's arguments.
    fn drag_for_glue(app: &AppCtx, from: Point, to: Point) -> Args {
        let mut machine = ToolMachine::new();
        machine.tool = super::super::Tool::Connector;
        machine
            .handle(down(from, Modifiers::none()), app)
            .expect("down");
        machine
            .handle(moved(to, Modifiers::none()), app)
            .expect("move");
        let effect = machine.handle(up(to, Modifiers::none()), app).expect("up");
        draw_intent(&effect)
            .expect("connector.draw")
            .invocation
            .args
            .clone()
    }

    #[test]
    fn a_press_inside_an_unfilled_shape_takes_dynamic_glue() {
        let mut shape = rectangle(ShapeId { index: 0 }, 1, (2.0, 3.0), &[]);
        for cell in &mut shape.sheet.cells {
            if cell.name.as_str() == "FillPattern" {
                cell.value = Some(CellValue::Number(0.0));
            }
        }
        let mut doc = Document::new();
        let mut page = Page::new(PageId { index: 0 }, 1);
        let id = doc.add_shape(shape);
        page.shapes.push(id);
        doc.pages.push(page);
        let mut ctx = AppCtx::new(Settings::default());
        ctx.open_document(DocumentSession::new(doc, None));

        // The centre of the unfilled box: the bounds fallback glues.
        let args = drag_for_glue(&ctx, Point::new(2.0, 3.0), Point::new(4.0, 4.0));
        assert_eq!(args.integer("begin_target"), Some(1));
    }

    /// A 2 x 1 diamond with no fill: the outline is the inscribed
    /// rhombus, smaller than its axis-aligned box (task P7-50 minor
    /// 1).
    fn diamond(id: ShapeId, visio_id: i32, pin: (f64, f64)) -> Shape {
        let mut shape = rectangle(id, visio_id, pin, &[]);
        for cell in &mut shape.sheet.cells {
            if cell.name.as_str() == "FillPattern" {
                cell.value = Some(CellValue::Number(0.0));
            }
        }
        shape
            .sheet
            .sections
            .retain(|section| section.name.as_str() != "Geometry");
        let mut geometry = Section::new(SectionName::from_static("Geometry"));
        for (ix, (kind, x, y)) in [
            ("MoveTo", 1.0, 0.0),
            ("LineTo", 2.0, 0.5),
            ("LineTo", 1.0, 1.0),
            ("LineTo", 0.0, 0.5),
            ("LineTo", 1.0, 0.0),
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
        shape
    }

    #[test]
    fn a_press_in_a_diamond_boxs_corner_outside_the_outline_does_not_glue() {
        // One unfilled diamond at (2, 3): its box is (1, 2.5)..(3,
        // 3.5), its outline the inscribed rhombus.
        let mut doc = Document::new();
        let mut page = Page::new(PageId { index: 0 }, 1);
        let id = doc.add_shape(diamond(ShapeId { index: 0 }, 1, (2.0, 3.0)));
        page.shapes.push(id);
        doc.pages.push(page);
        let mut ctx = AppCtx::new(Settings::default());
        ctx.open_document(DocumentSession::new(doc, None));

        // The corner area of the box (2.75, 3.25) is outside the
        // rhombus: no glue (the old bounds test glued).
        let args = drag_for_glue(&ctx, Point::new(2.75, 3.25), Point::new(5.0, 4.0));
        assert_eq!(args.integer("begin_target"), None, "outside the outline");
        // The centre (2, 3) is inside the outline: glue.
        let args = drag_for_glue(&ctx, Point::new(2.0, 3.0), Point::new(5.0, 4.0));
        assert_eq!(args.integer("begin_target"), Some(1), "inside the outline");
    }

    #[test]
    fn point_glue_on_a_group_member_names_the_member() {
        // A group (its own transform the identity) whose member has
        // connection point 0 at page (3, 3).
        let member = rectangle(ShapeId { index: 0 }, 2, (4.0, 3.0), &[(0.0, 0.5)]);
        let mut group = rectangle(ShapeId { index: 0 }, 1, (1.0, 0.5), &[]);
        group.children = Vec::new();
        let mut doc = Document::new();
        let mut page = Page::new(PageId { index: 0 }, 1);
        let member = doc.add_shape(member);
        let group = doc.add_shape(group);
        page.shapes.push(group);
        if let Some(entity) = doc.shape_mut(group) {
            entity.children.push(member);
        }
        if let Some(entity) = doc.shape_mut(member) {
            entity.parent = Some(group);
        }
        doc.pages.push(page);
        let mut ctx = AppCtx::new(Settings::default());
        ctx.open_document(DocumentSession::new(doc, None));

        // The drag snaps to the member's point: the glue names the
        // member and that row (task P7-50 goal 4), not the top-level
        // group with the member's row index.
        let args = drag_for_glue(&ctx, Point::new(3.0, 3.0), Point::new(5.0, 4.0));
        assert_eq!(args.integer("begin_target"), Some(2), "the row's owner");
        assert_eq!(args.integer("begin_point"), Some(0), "the member's row");
    }

    #[test]
    fn a_hit_on_a_group_member_maps_to_its_top_level_ancestor() {
        let member = rectangle(ShapeId { index: 0 }, 2, (4.0, 3.0), &[]);
        let mut group = rectangle(ShapeId { index: 0 }, 1, (0.0, 0.0), &[]);
        group.children = Vec::new();
        let mut doc = Document::new();
        let mut page = Page::new(PageId { index: 0 }, 1);
        let member = doc.add_shape(member);
        let group = doc.add_shape(group);
        page.shapes.push(group);
        if let Some(entity) = doc.shape_mut(group) {
            entity.children.push(member);
        }
        if let Some(entity) = doc.shape_mut(member) {
            entity.parent = Some(group);
        }
        doc.pages.push(page);
        let mut ctx = AppCtx::new(Settings::default());
        ctx.open_document(DocumentSession::new(doc, None));

        // The press is on the member; the glue names the group.
        let args = drag_for_glue(&ctx, Point::new(4.0, 3.0), Point::new(6.0, 4.0));
        assert_eq!(
            args.integer("begin_target"),
            Some(1),
            "the top-level ancestor"
        );
    }

    #[test]
    fn a_text_block_larger_than_the_shape_does_not_glue_outside() {
        let mut shape = rectangle(ShapeId { index: 0 }, 1, (2.0, 3.0), &[]);
        shape.text = Some(larrygram_model::TextContent {
            parts: vec![larrygram_model::TextPart::Text("a long text".to_owned())],
        });
        shape.sheet.cells.push(cell("TxtWidth", 10.0));
        shape.sheet.cells.push(cell("TxtHeight", 6.0));
        let mut doc = Document::new();
        let mut page = Page::new(PageId { index: 0 }, 1);
        let id = doc.add_shape(shape);
        page.shapes.push(id);
        doc.pages.push(page);
        let mut ctx = AppCtx::new(Settings::default());
        ctx.open_document(DocumentSession::new(doc, None));

        // The press is beyond the shape's own box (over its large text
        // block): no glue.
        let args = drag_for_glue(&ctx, Point::new(6.0, 3.0), Point::new(7.0, 4.0));
        assert_eq!(args.integer("begin_target"), None, "no glue outside");
    }

    #[test]
    fn a_tie_between_connection_points_goes_to_the_topmost_shape() {
        let mut doc = Document::new();
        let mut page = Page::new(PageId { index: 0 }, 1);
        // Both boxes put connection point 0 at the same page point.
        let bottom = doc.add_shape(rectangle(
            ShapeId { index: 0 },
            1,
            (2.0, 3.0),
            &[(2.0, 0.5)],
        ));
        let top = doc.add_shape(rectangle(
            ShapeId { index: 0 },
            2,
            (2.0, 3.0),
            &[(2.0, 0.5)],
        ));
        page.shapes.push(bottom);
        page.shapes.push(top);
        doc.pages.push(page);
        let mut ctx = AppCtx::new(Settings::default());
        ctx.open_document(DocumentSession::new(doc, None));

        // The shared point is at (3, 3); the topmost shape wins.
        let args = drag_for_glue(&ctx, Point::new(3.0, 3.0), Point::new(6.0, 4.0));
        assert_eq!(args.integer("begin_point"), Some(0));
        assert_eq!(args.integer("begin_target"), Some(2), "the topmost shape");
    }

    #[test]
    fn the_connector_master_is_in_the_catalog() {
        assert_eq!(connector_master(), Some(("Basic Flowchart", "Connector")));
    }
}
