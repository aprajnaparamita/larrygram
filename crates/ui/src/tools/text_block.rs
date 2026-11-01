//! The text block tool (task P7-25 item 4).
//!
//! With a shape or text box selected, the tool shows its text block as
//! a handle frame (the `Txt*` cells' box, ADR 0024) and one drag moves
//! or resizes it: the interior (or any press on the shape) moves the
//! block, the edge and corner handles resize it. The drag previews
//! with overlay marks and commits exactly one command intent on
//! release (`shape.set.text_block`, one undo step, ADR 0040): the tool
//! never mutates the model itself (ADR 0042).
//!
//! The block's shape-local placement is the render's rule (ADR 0024's
//! `TextPlacement`): the box `(0..width, 0..height)` maps into the
//! shape by translating by `-TxtLocPin*`, turning by `TxtAngle` and
//! translating by `TxtPin*`; the shape's own transform takes it to the
//! page. A shape with no `Txt*` cells of its own reads their
//! documented defaults (the whole shape box), so the drag writes the
//! cells seamlessly from the first press.

use larrygram_commands::{AppCtx, ArgValue, Args, CommandError, CommandId, Invocation, Source};
use larrygram_geom::{Angle, Point, Rect, Transform};
use larrygram_model::{CellName, CellReader, ShapeId, SheetRef};
use larrygram_render::{HitOptions, Overlay, OverlayMark, Scene, hit_test};

use crate::UiError;
use crate::handles::{HandleKind, ResizeEdge, handle_at, rect_center, render_error};
use crate::text_edit::find_shape;

use super::{Modifiers, ToolEffect, ToolEvent, ToolMachine};

/// The smallest size a resize drag may produce, in inches (the
/// pointer tool's rule).
const MIN_SIZE_IN: f64 = 1.0 / 96.0;

/// The drag distance below which a release is a click, in screen
/// pixels (the pointer tool's threshold).
const DRAG_THRESHOLD_PX: f64 = 2.0;

/// The text block tool's drag state (owner: this module).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    /// The current gesture.
    mode: Mode,
}

/// What the tool is doing between events.
#[derive(Debug, Clone, Default, PartialEq)]
enum Mode {
    /// Nothing pressed.
    #[default]
    Idle,
    /// A block drag (move or resize).
    Drag {
        /// [`HandleKind::Move`] or [`HandleKind::Resize`] (the rotate
        /// handle is not a text block action and never starts a drag).
        kind: HandleKind,
        /// Where the drag started, in page inches.
        start: Point,
        /// The current pointer position, in page inches.
        current: Point,
        /// The block geometry at the drag start.
        frame: Frame,
    },
}

/// The text block geometry of one shape at one moment.
#[derive(Debug, Clone, PartialEq)]
struct Frame {
    /// The shape whose block is dragged.
    shape: ShapeId,
    /// The handle frame in page inches: the unrotated bounds and the
    /// page angle of the block's box.
    bounds: Rect,
    angle: Angle,
    /// `TxtPinX`/`TxtPinY` at the drag start (shape-local inches).
    pin: Point,
    /// `TxtLocPinX`/`TxtLocPinY` at the drag start (shape-local).
    loc: Point,
    /// `TxtWidth`/`TxtHeight` at the drag start (inches).
    size: (f64, f64),
    /// `TxtAngle` at the drag start (radians, shape-local).
    block_angle: f64,
    /// The shape-local to page transform.
    to_page: Transform,
}

/// Handles one event for the text block tool (task P7-25 item 4).
pub fn handle(
    state: &mut ToolMachine,
    event: &super::ToolEvent,
    ctx: &AppCtx,
) -> Result<super::ToolEffect, UiError> {
    let zoom = ctx
        .active_document()
        .map(|session| session.view.zoom)
        .filter(|zoom| zoom.is_finite() && *zoom > 0.0)
        .unwrap_or(1.0);
    match event {
        ToolEvent::PointerDown { pos, modifiers } => down(state, ctx, *pos, *modifiers, zoom),
        ToolEvent::PointerMove { pos, .. } => moved(state, ctx, *pos, zoom),
        ToolEvent::PointerUp { pos, .. } => up(state, ctx, *pos, zoom),
        ToolEvent::DoubleClick { .. } => Ok(ToolEffect::new()),
        ToolEvent::Cancel => {
            state.text_block.mode = Mode::Idle;
            let overlay = idle_overlay(state, ctx)?.unwrap_or_default();
            Ok(ToolEffect {
                overlay,
                ..ToolEffect::new()
            })
        }
    }
}

/// The drag anchor, in page inches (the cursor hint).
pub fn anchor(state: &ToolMachine) -> Option<Point> {
    match &state.text_block.mode {
        Mode::Idle => None,
        Mode::Drag { current, .. } => Some(*current),
    }
}

/// The block's handle frame while the tool is idle (task P7-25 item
/// 4: the handles are what the drag grabs).
pub fn idle_overlay(state: &mut ToolMachine, ctx: &AppCtx) -> Result<Option<Overlay>, UiError> {
    if !matches!(state.text_block.mode, Mode::Idle) {
        return Ok(None);
    }
    if anchor_shape(ctx).is_none() {
        return Ok(None);
    }
    match frame_of(ctx)? {
        Some(frame) => overlay_for(frame).map(Some),
        None => Ok(Some(Overlay::new())),
    }
}

/// The frame of the anchor shape's text block, or `None` when there
/// is no selection or the values are unusable.
fn frame_of(ctx: &AppCtx) -> Result<Option<Frame>, UiError> {
    let Some(shape) = anchor_shape(ctx) else {
        return Ok(None);
    };
    let Some(session) = ctx.active_document() else {
        return Ok(None);
    };
    let Some(page) = session.active_page else {
        return Ok(None);
    };
    let scene = match crate::handles::scene_for(ctx)? {
        Some(scene) => scene,
        None => return Ok(None),
    };
    block_frame(&session.document, &scene, shape, page)
}

/// The anchor shape: the selection's anchor, else the last selected.
fn anchor_shape(ctx: &AppCtx) -> Option<ShapeId> {
    ctx.selection
        .anchor
        .or_else(|| ctx.selection.shapes.last().copied())
}

/// One press: start a block move or resize (task P7-25 item 4).
fn down(
    state: &mut ToolMachine,
    ctx: &AppCtx,
    pos: Point,
    _modifiers: Modifiers,
    zoom: f64,
) -> Result<ToolEffect, UiError> {
    let mut effect = ToolEffect::new();
    let Some(frame) = frame_of(ctx)? else {
        return Ok(effect);
    };
    let kind = match handle_at(pos, frame.bounds, frame.angle, zoom)? {
        Some(HandleKind::Move) => Some(HandleKind::Move),
        Some(HandleKind::Resize(edge)) => Some(HandleKind::Resize(edge)),
        // The rotate handle is not a text block action (task P7-25
        // item 4 moves and resizes only).
        Some(HandleKind::Rotate) => None,
        // A press on the shape (but off the block) still moves the
        // block, as in Visio.
        None if press_on_shape(ctx, pos, frame.shape, zoom)? => Some(HandleKind::Move),
        None => None,
    };
    let Some(kind) = kind else {
        return Ok(effect);
    };
    state.text_block.mode = Mode::Drag {
        kind,
        start: pos,
        current: pos,
        frame,
    };
    effect.overlay = drag_overlay(&state.text_block.mode)?;
    Ok(effect)
}

/// One move: extend the drag and preview it.
fn moved(
    state: &mut ToolMachine,
    ctx: &AppCtx,
    pos: Point,
    zoom: f64,
) -> Result<ToolEffect, UiError> {
    let _ = ctx;
    let _ = zoom;
    let mut effect = ToolEffect::new();
    let Mode::Drag {
        kind, start, frame, ..
    } = &state.text_block.mode
    else {
        return Ok(effect);
    };
    state.text_block.mode = Mode::Drag {
        kind: *kind,
        start: *start,
        current: pos,
        frame: frame.clone(),
    };
    effect.overlay = drag_overlay(&state.text_block.mode)?;
    Ok(effect)
}

/// One release: commit the drag as one `shape.set.text_block` intent
/// (one undo step, ADR 0040), or do nothing for a click.
fn up(
    state: &mut ToolMachine,
    _ctx: &AppCtx,
    pos: Point,
    zoom: f64,
) -> Result<ToolEffect, UiError> {
    let mut effect = ToolEffect::new();
    let Mode::Drag {
        kind, start, frame, ..
    } = std::mem::replace(&mut state.text_block.mode, Mode::Idle)
    else {
        return Ok(effect);
    };
    let threshold = DRAG_THRESHOLD_PX / (zoom * crate::canvas::POINTS_PER_INCH);
    let delta = pos.sub(start);
    if delta.x.hypot(delta.y) < threshold {
        effect.overlay = overlay_for(frame)?;
        return Ok(effect);
    }
    if let Some(intent) = drag_intent(kind, start, pos, &frame)? {
        effect.intents.push(intent);
    }
    effect.overlay = overlay_for(frame)?;
    Ok(effect)
}

/// The command intent of one completed block drag (task P7-25 item
/// 4), or `None` when the drag changed nothing.
fn drag_intent(
    kind: HandleKind,
    start: Point,
    current: Point,
    frame: &Frame,
) -> Result<Option<crate::Intent>, UiError> {
    match kind {
        HandleKind::Move => {
            let d_page = current.sub(start);
            let to_local = frame.to_page.invert().map_err(|error| UiError::Io {
                message: format!("the shape transform is not invertible: {error}"),
            })?;
            let anchor = Point::new(0.0, 0.0);
            let moved = to_local
                .apply(anchor.add(d_page))
                .sub(to_local.apply(anchor));
            if moved.x == 0.0 && moved.y == 0.0 {
                return Ok(None);
            }
            let pin = frame.pin.add(moved);
            text_block_intent(pin.x, pin.y, None, None, None)
        }
        HandleKind::Resize(edge) => {
            let (w_page, h_page) = resized_size(frame, start, current, edge);
            let (w, h) = frame.size;
            let scale_x = frame.bounds.width() / w.max(f64::MIN_POSITIVE);
            let scale_y = frame.bounds.height() / h.max(f64::MIN_POSITIVE);
            let width = (w_page / scale_x.max(f64::MIN_POSITIVE)).max(MIN_SIZE_IN);
            let height = (h_page / scale_y.max(f64::MIN_POSITIVE)).max(MIN_SIZE_IN);
            if (width - w).abs() < f64::EPSILON && (height - h).abs() < f64::EPSILON {
                return Ok(None);
            }
            // The `TxtLocPin*` offset follows the size (ADR 0038 rule
            // 2) when it holds the centre, and keeps its custom offset
            // otherwise. The drag writes it beside the pin (task P7-26
            // item 1): the cells' documented default follows the
            // **shape's** `Width`, not `TxtWidth`, so the pin alone
            // drew the block away from this preview.
            let default_loc = (w / 2.0, h / 2.0);
            let (loc_x, loc_y) = if (frame.loc.x - default_loc.0).abs() < 1.0e-9
                && (frame.loc.y - default_loc.1).abs() < 1.0e-9
            {
                (width / 2.0, height / 2.0)
            } else {
                (frame.loc.x, frame.loc.y)
            };
            // The opposite edge or corner stays put (task P7-26 item
            // 8), so the pin follows the block's own axes.
            let pin = resized_pin(frame, edge, width, height, loc_x, loc_y);
            text_block_intent(
                pin.x,
                pin.y,
                Some(width),
                Some(height),
                Some((loc_x, loc_y)),
            )
        }
        // A rotate drag never starts (task P7-25 item 4).
        HandleKind::Rotate => Ok(None),
    }
}

/// The dragged sides of one resize edge (`1.0` when the frame's plus
/// side moved, `-1.0` for the minus side, `0.0` on an axis that kept
/// its size). The side that **stays** is the opposite one (task P7-26
/// item 8).
fn fixed_signs(edge: ResizeEdge) -> (f64, f64) {
    let x = match edge {
        ResizeEdge::East | ResizeEdge::NorthEast | ResizeEdge::SouthEast => 1.0,
        ResizeEdge::West | ResizeEdge::NorthWest | ResizeEdge::SouthWest => -1.0,
        _ => 0.0,
    };
    let y = match edge {
        ResizeEdge::North | ResizeEdge::NorthEast | ResizeEdge::NorthWest => 1.0,
        ResizeEdge::South | ResizeEdge::SouthEast | ResizeEdge::SouthWest => -1.0,
        _ => 0.0,
    };
    (x, y)
}

/// The new `(width, height)` of a resize drag along the frame's own
/// axes, in the frame's inches (clamped positive). Task P7-26 item 8:
/// the pointer's delta is projected on those axes, so a rotated block
/// resizes when its handle is dragged along the page's other axis.
fn resized_size(frame: &Frame, start: Point, current: Point, edge: ResizeEdge) -> (f64, f64) {
    let (sin, cos) = frame.angle.radians().sin_cos();
    let delta = current.sub(start);
    let dx = delta.x * cos + delta.y * sin;
    let dy = delta.y * cos - delta.x * sin;
    let mut width = frame.bounds.width();
    let mut height = frame.bounds.height();
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

/// The `shape.set.text_block` pin of a resize drag: the opposite edge
/// or corner stays put (task P7-26 item 8), so the pin follows the
/// block's own axes.
fn resized_pin(
    frame: &Frame,
    edge: ResizeEdge,
    width: f64,
    height: f64,
    loc_x: f64,
    loc_y: f64,
) -> Point {
    let (sx, sy) = fixed_signs(edge);
    // The corner that stays, in box coordinates: the minus side when
    // the plus side moved.
    let corner = |sx: f64, sy: f64, w: f64, h: f64| {
        (
            if sx > 0.0 {
                0.0
            } else if sx < 0.0 {
                w
            } else {
                w / 2.0
            },
            if sy > 0.0 {
                0.0
            } else if sy < 0.0 {
                h
            } else {
                h / 2.0
            },
        )
    };
    let (w, h) = frame.size;
    let (fx, fy) = corner(sx, sy, w, h);
    let (tx, ty) = corner(sx, sy, width, height);
    let dx = (fx - frame.loc.x) - (tx - loc_x);
    let dy = (fy - frame.loc.y) - (ty - loc_y);
    let (sin, cos) = frame.block_angle.sin_cos();
    Point::new(
        frame.pin.x + dx * cos - dy * sin,
        frame.pin.y + dx * sin + dy * cos,
    )
}

/// The `shape.set.text_block` intent for one finished drag (task
/// P7-25 item 4). `loc` is the pin's offset inside the block box
/// (`TxtLocPinX`/`TxtLocPinY`, task P7-26 item 1): a resize sends it
/// with the pin so the block lands where the preview showed it.
fn text_block_intent(
    x: f64,
    y: f64,
    width: Option<f64>,
    height: Option<f64>,
    loc: Option<(f64, f64)>,
) -> Result<Option<crate::Intent>, UiError> {
    let mut args = Args::new()
        .with("x", ArgValue::Number(x))
        .with("y", ArgValue::Number(y));
    if let Some(width) = width {
        args = args.with("width", ArgValue::Number(width));
    }
    if let Some(height) = height {
        args = args.with("height", ArgValue::Number(height));
    }
    if let Some((loc_x, loc_y)) = loc {
        args = args
            .with("loc_x", ArgValue::Number(loc_x))
            .with("loc_y", ArgValue::Number(loc_y));
    }
    let command = CommandId::parse("shape.set.text_block").map_err(|error| {
        UiError::Command(CommandError::failed(format!(
            "the text block command id is not valid: {error}"
        )))
    })?;
    Ok(Some(crate::Intent::new(
        command,
        Invocation::new(args, Source::Ui),
    )))
}

/// Whether the press at `pos` lands on `shape` (or one of its
/// descendants): the off-block move rule of task P7-25 item 4.
fn press_on_shape(ctx: &AppCtx, pos: Point, shape: ShapeId, zoom: f64) -> Result<bool, UiError> {
    let Some(scene) = crate::handles::scene_for(ctx)? else {
        return Ok(false);
    };
    let tolerance = 4.0 / (zoom * crate::canvas::POINTS_PER_INCH);
    let hits = hit_test(&scene, pos, &HitOptions::new(tolerance)).map_err(render_error)?;
    let Some(session) = ctx.active_document() else {
        return Ok(false);
    };
    Ok(hits.iter().any(|hit| {
        hit.shape_id
            .is_some_and(|hit| hit == shape || is_descendant(&session.document, shape, hit))
    }))
}

/// Whether `hit` lies under `shape` in the parent chain (a group's
/// member counts as the group's shape).
fn is_descendant(doc: &larrygram_model::Document, shape: ShapeId, hit: ShapeId) -> bool {
    let mut current = hit;
    let mut seen = Vec::new();
    loop {
        if current == shape {
            return true;
        }
        if seen.contains(&current) {
            return false;
        }
        seen.push(current);
        let Some(parent) = doc.shape(current).and_then(|entry| entry.parent) else {
            return false;
        };
        current = parent;
    }
}

/// The frame of the anchor shape's text block, from the `Txt*` cells
/// (ADR 0024's placement rule) and the shape's scene transform.
fn block_frame(
    doc: &larrygram_model::Document,
    scene: &Scene,
    shape: ShapeId,
    page: larrygram_model::PageId,
) -> Result<Option<Frame>, UiError> {
    if doc.page_of(shape) != Some(page) {
        return Ok(None);
    }
    let Some((_, _, to_page)) = find_shape(
        &scene.nodes,
        shape,
        &Default::default(),
        Transform::identity(),
        0,
    ) else {
        return Ok(None);
    };
    let mut reader = CellReader::new(doc);
    let sheet = SheetRef::Shape(shape);
    let read = |reader: &mut CellReader<'_>, name: &str| {
        reader
            .number(sheet, &CellName::new(name))
            .ok()
            .filter(|v| v.is_finite())
    };
    let pin_x = read(&mut reader, "TxtPinX");
    let pin_y = read(&mut reader, "TxtPinY");
    let loc_x = read(&mut reader, "TxtLocPinX");
    let loc_y = read(&mut reader, "TxtLocPinY");
    let width = read(&mut reader, "TxtWidth");
    let height = read(&mut reader, "TxtHeight");
    let block_angle = read(&mut reader, "TxtAngle");
    let (Some(pin_x), Some(pin_y), Some(loc_x), Some(loc_y), Some(width), Some(height)) =
        (pin_x, pin_y, loc_x, loc_y, width, height)
    else {
        return Ok(None);
    };
    if width <= 0.0 || height <= 0.0 {
        return Ok(None);
    }
    let block_angle = block_angle.unwrap_or(0.0);
    let placement = Transform::translate(Point::new(-loc_x, -loc_y))
        .then(Transform::rotate(Angle::from_radians(block_angle)))
        .then(Transform::translate(Point::new(pin_x, pin_y)));
    let to_page = placement.then(to_page);
    // The handle frame: the box's centre and side lengths in page
    // inches, with the page angle of the box's x axis.
    let origin = to_page.apply(Point::new(0.0, 0.0));
    let along_x = to_page.apply(Point::new(width, 0.0));
    let along_y = to_page.apply(Point::new(0.0, height));
    let center = to_page.apply(Point::new(width / 2.0, height / 2.0));
    let u = along_x.sub(origin);
    let v = along_y.sub(origin);
    let (w_page, h_page) = (u.x.hypot(u.y), v.x.hypot(v.y));
    if !center.x.is_finite() || !center.y.is_finite() || !w_page.is_finite() || !h_page.is_finite()
    {
        return Ok(None);
    }
    let bounds = Rect::new(
        Point::new(center.x - w_page / 2.0, center.y - h_page / 2.0),
        Point::new(center.x + w_page / 2.0, center.y + h_page / 2.0),
    );
    Ok(Some(Frame {
        shape,
        bounds,
        angle: Angle::from_radians(u.y.atan2(u.x)),
        pin: Point::new(pin_x, pin_y),
        loc: Point::new(loc_x, loc_y),
        size: (width, height),
        block_angle,
        to_page,
    }))
}

/// The overlay marks of one block frame (its outline and handles).
fn overlay_for(frame: Frame) -> Result<Overlay, UiError> {
    let mut overlay = Overlay::new();
    overlay.marks.push(OverlayMark::SelectionOutline {
        bounds: frame.bounds,
        angle: frame.angle,
    });
    overlay.marks.push(OverlayMark::Handles {
        bounds: frame.bounds,
        angle: frame.angle,
    });
    Ok(overlay)
}

/// The preview marks of the current drag (the moved or resized frame).
fn drag_overlay(mode: &Mode) -> Result<Overlay, UiError> {
    match mode {
        Mode::Idle => Ok(Overlay::new()),
        Mode::Drag {
            kind,
            start,
            current,
            frame,
        } => match kind {
            HandleKind::Move => {
                let d_page = current.sub(*start);
                let bounds = Rect::new(
                    Point::new(frame.bounds.min.x + d_page.x, frame.bounds.min.y + d_page.y),
                    Point::new(frame.bounds.max.x + d_page.x, frame.bounds.max.y + d_page.y),
                );
                overlay_for(Frame {
                    bounds,
                    ..frame.clone()
                })
            }
            HandleKind::Resize(edge) => {
                let (w_page, h_page) = resized_size(frame, *start, *current, *edge);
                // The opposite edge or corner stays put (task P7-26
                // item 8): the frame's centre follows the grown side,
                // along the frame's own axes.
                let (sx, sy) = fixed_signs(*edge);
                let (sin, cos) = frame.angle.radians().sin_cos();
                let along_x = sx * (w_page - frame.bounds.width()) / 2.0;
                let along_y = sy * (h_page - frame.bounds.height()) / 2.0;
                let center = rect_center(frame.bounds).add(Point::new(
                    along_x * cos - along_y * sin,
                    along_x * sin + along_y * cos,
                ));
                let bounds = Rect::new(
                    Point::new(center.x - w_page / 2.0, center.y - h_page / 2.0),
                    Point::new(center.x + w_page / 2.0, center.y + h_page / 2.0),
                );
                overlay_for(Frame {
                    bounds,
                    ..frame.clone()
                })
            }
            HandleKind::Rotate => Ok(Overlay::new()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use larrygram_commands::{DocumentSession, Settings};
    use larrygram_geom::Unit;
    use larrygram_model::{
        Cell, CellName as ModelCellName, CellValue, Document, Row, Section, SectionName, Shape,
    };

    fn cell(name: &str, value: f64) -> Cell {
        let mut cell = Cell::new(ModelCellName::new(name));
        cell.value = Some(CellValue::Number(value));
        cell.unit = Unit::Inches;
        cell
    }

    /// One 2 x 1 rectangle with its pin at (3, 3) and a centred text
    /// block 1 x 0.5 (the `Txt*` cells).
    fn fixture() -> (AppCtx, ShapeId) {
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
            cell("TxtPinX", 1.0),
            cell("TxtPinY", 0.5),
            cell("TxtLocPinX", 0.5),
            cell("TxtLocPinY", 0.25),
            cell("TxtWidth", 1.0),
            cell("TxtHeight", 0.5),
            cell("TxtAngle", 0.0),
        ];
        let mut geometry = Section::new(SectionName::from_static("Geometry"));
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
        let shape = doc.insert_shape(page, shape, None).expect("shape");
        let mut app = AppCtx::new(Settings::default());
        app.open_document(DocumentSession::new(doc, None));
        app.selection.shapes = vec![shape];
        app.selection.anchor = Some(shape);
        app.selection.page = Some(page);
        (app, shape)
    }

    /// The frame of `shape`'s block in `app`.
    fn frame(app: &AppCtx) -> Option<Frame> {
        let session = app.active_document().expect("session");
        let page = session.active_page.expect("page");
        let scene = crate::handles::scene_for(app)
            .expect("scene")
            .expect("scene");
        block_frame(
            &session.document,
            &scene,
            anchor_shape(app).expect("anchor"),
            page,
        )
        .expect("frame")
    }

    #[test]
    fn the_block_frame_comes_from_the_txt_cells() {
        let (app, _) = fixture();
        let frame = frame(&app).expect("frame");
        // The block is 1 x 0.5 centred in the 2 x 1 shape at (3, 3):
        // page bounds (2.5..3.5, 2.75..3.25).
        assert!(
            (frame.bounds.min.x - 2.5).abs() < 1.0e-9,
            "{:?}",
            frame.bounds
        );
        assert!(
            (frame.bounds.min.y - 2.75).abs() < 1.0e-9,
            "{:?}",
            frame.bounds
        );
        assert!((frame.bounds.width() - 1.0).abs() < 1.0e-9);
        assert!((frame.bounds.height() - 0.5).abs() < 1.0e-9);
        assert_eq!(frame.angle.radians(), 0.0);
    }

    #[test]
    fn a_shape_without_txt_cells_reads_the_shape_box() {
        let (mut app, shape) = fixture();
        {
            let session = app.active_document_mut().expect("session");
            let entry = session.document.shape_mut(shape).expect("shape");
            entry.sheet.cells.retain(|cell| {
                !matches!(
                    cell.name.as_str(),
                    "TxtPinX" | "TxtPinY" | "TxtLocPinX" | "TxtLocPinY" | "TxtWidth" | "TxtHeight"
                )
            });
        }
        let frame = frame(&app).expect("frame");
        assert!((frame.bounds.width() - 2.0).abs() < 1.0e-9);
        assert!((frame.bounds.height() - 1.0).abs() < 1.0e-9);
    }

    #[test]
    fn a_move_drag_commits_the_new_pin_in_one_intent() {
        let (app, _) = fixture();
        let mut machine = ToolMachine::new();
        // Press on the block's centre (inside the block: a move).
        let start = Point::new(3.0, 3.0);
        let effect = handle(
            &mut machine,
            &ToolEvent::PointerDown {
                pos: start,
                modifiers: Modifiers::none(),
            },
            &app,
        )
        .expect("down");
        assert!(effect.intents.is_empty(), "a drag starts with no intent");
        handle(
            &mut machine,
            &ToolEvent::PointerMove {
                pos: Point::new(3.5, 3.25),
                modifiers: Modifiers::none(),
            },
            &app,
        )
        .expect("move");
        let effect = handle(
            &mut machine,
            &ToolEvent::PointerUp {
                pos: Point::new(3.5, 3.25),
                modifiers: Modifiers::none(),
            },
            &app,
        )
        .expect("up");
        assert_eq!(effect.intents.len(), 1);
        let intent = &effect.intents[0];
        assert_eq!(intent.command.as_str(), "shape.set.text_block");
        assert_eq!(intent.invocation.args.number("x"), Some(1.5));
        assert_eq!(intent.invocation.args.number("y"), Some(0.75));
        assert_eq!(intent.invocation.args.number("width"), None);
        assert_eq!(
            intent.invocation.args.number("loc_x"),
            None,
            "a move keeps the block's own offset"
        );
    }

    #[test]
    fn a_resize_drag_commits_the_new_size_and_pin() {
        let (app, _) = fixture();
        let mut machine = ToolMachine::new();
        // Press on the block's north-east corner handle (4, 3.25).
        let start = Point::new(3.5, 3.25);
        handle(
            &mut machine,
            &ToolEvent::PointerDown {
                pos: start,
                modifiers: Modifiers::none(),
            },
            &app,
        )
        .expect("down");
        let effect = handle(
            &mut machine,
            &ToolEvent::PointerUp {
                pos: Point::new(4.0, 3.5),
                modifiers: Modifiers::none(),
            },
            &app,
        )
        .expect("up");
        assert_eq!(effect.intents.len(), 1);
        let intent = &effect.intents[0];
        assert_eq!(intent.command.as_str(), "shape.set.text_block");
        // The size grew by 0.5 x 0.25 (one scale unit) and the
        // opposite corner (the block's south-west) stays put (task
        // P7-26 item 8): the pin follows the grown sides in
        // shape-local inches.
        let width = intent.invocation.args.number("width").expect("width");
        let height = intent.invocation.args.number("height").expect("height");
        assert!((width - 1.5).abs() < 1.0e-9, "{width}");
        assert!((height - 0.75).abs() < 1.0e-9, "{height}");
        assert_eq!(intent.invocation.args.number("x"), Some(1.25));
        assert_eq!(intent.invocation.args.number("y"), Some(0.625));
        // The block's origin (pin - loc) keeps its place.
        let loc_x = intent.invocation.args.number("loc_x").expect("loc_x");
        let loc_y = intent.invocation.args.number("loc_y").expect("loc_y");
        assert!((1.25 - loc_x - 0.5).abs() < 1.0e-9, "{loc_x}");
        assert!((0.625 - loc_y - 0.25).abs() < 1.0e-9, "{loc_y}");
        // Task P7-26 item 1: the drag sends the pin's offset inside the
        // block box (`TxtLocPinX`/`TxtLocPinY`) with the pin, so the
        // block lands where the preview showed it.
        assert_eq!(intent.invocation.args.number("loc_x"), Some(0.75));
        assert_eq!(intent.invocation.args.number("loc_y"), Some(0.375));
    }

    #[test]
    fn a_click_or_a_press_off_the_shape_commits_nothing() {
        let (app, _) = fixture();
        let mut machine = ToolMachine::new();
        // A click (no drag) on the block: no intent.
        let pos = Point::new(3.0, 3.0);
        for event in [
            ToolEvent::PointerDown {
                pos,
                modifiers: Modifiers::none(),
            },
            ToolEvent::PointerUp {
                pos,
                modifiers: Modifiers::none(),
            },
        ] {
            let effect = handle(&mut machine, &event, &app).expect("click");
            assert!(effect.intents.is_empty(), "{event:?}");
        }
        // A press off the shape starts nothing.
        let effect = handle(
            &mut machine,
            &ToolEvent::PointerDown {
                pos: Point::new(0.5, 0.5),
                modifiers: Modifiers::none(),
            },
            &app,
        )
        .expect("down");
        assert!(effect.intents.is_empty());
        assert_eq!(anchor(&machine), None);
    }

    #[test]
    fn cancel_drops_the_drag_and_idle_shows_the_frame() {
        let (app, _) = fixture();
        let mut machine = ToolMachine::new();
        handle(
            &mut machine,
            &ToolEvent::PointerDown {
                pos: Point::new(3.0, 3.0),
                modifiers: Modifiers::none(),
            },
            &app,
        )
        .expect("down");
        let effect = handle(&mut machine, &ToolEvent::Cancel, &app).expect("cancel");
        assert!(effect.intents.is_empty());
        assert!(anchor(&machine).is_none());
        let overlay = idle_overlay(&mut machine, &app)
            .expect("overlay")
            .expect("idle overlay");
        assert_eq!(overlay.marks.len(), 2, "{overlay:?}");
    }

    #[test]
    fn without_a_selection_the_tool_is_a_no_op() {
        let (mut app, _) = fixture();
        app.selection.shapes.clear();
        app.selection.anchor = None;
        let mut machine = ToolMachine::new();
        let effect = handle(
            &mut machine,
            &ToolEvent::PointerDown {
                pos: Point::new(3.0, 3.0),
                modifiers: Modifiers::none(),
            },
            &app,
        )
        .expect("down");
        assert!(effect.intents.is_empty());
        assert_eq!(idle_overlay(&mut machine, &app).expect("overlay"), None);
    }

    #[test]
    fn the_static_command_id_is_valid() {
        assert!(text_block_intent(0.0, 0.0, None, None, None).is_ok());
    }
}
