//! Connector segment moves, Reset Connector and the reroute options
//! (task P7-35, ADR 0084 rules 5 and 7).
//!
//! `connector.move_segment` writes one dragged segment move as the
//! connector's geometry (the stored route is the memory, rule 5);
//! `connector.dyn.conn.reroute` (Reset Connector) re-routes the
//! selected connectors fresh; and the three
//! `format.set.dyn.conn.reroute.*` controls write `ConFixedCode` 0, 3
//! and 2 on the selected connectors.
//!
//! [`never_reroutes`] and [`kept_route`] are the `ConFixedCode` check
//! [`reroute_document`](super::connector) makes after an edit (rule 7):
//! with Never Reroute the scoped pass keeps the stored route's
//! interior and stretches its end segments to the resolved glue.
//!
//! Part of Larrygram. Alpha.

use larrygram_geom::{Angle, Point, Transform};
use larrygram_model::{
    CellName, CellReader, CellValue, CellWrite, Connect, Document, GeometryRow, PageId, RowTarget,
    ShapeId, Sheet, SheetRef, mutate,
};
use larrygram_shapesheet::CellAddress;

use crate::app::{AppCtx, SessionId};
use crate::edit::CompositeEdit;
use crate::error::CommandError;
use crate::undo::Edit;
use crate::{Args, Invocation, Outcome};

use super::connector::{end_cells_point, reroute_connectors, resolved_ends, style_targets};
use super::edit::{CellEdit, document_mut};
use super::{
    CommandTable, edit_outcome, has_pages, model_error, need_finite, no_document, outcome,
};

/// `ConFixedCode` 2: Never Reroute (the behaviour dialog's Connector
/// tab, `docs/ui/connectors.md` section 4).
const NEVER: f64 = 2.0;

/// The implemented commands of this module (task P7-35).
pub(crate) const TABLE: CommandTable = &[
    (
        "connector.move_segment",
        "Move Connector Segment",
        has_pages,
        run_move_segment,
    ),
    (
        "connector.dyn.conn.reroute",
        "Reset Connector",
        connectors_selected,
        run_reset,
    ),
    (
        "format.set.dyn.conn.reroute.freely",
        "Reroute Freely",
        connectors_selected,
        run_reroute_freely,
    ),
    (
        "format.set.dyn.conn.reroute.on.crossover",
        "Reroute on Crossover",
        connectors_selected,
        run_reroute_on_crossover,
    ),
    (
        "format.set.dyn.conn.reroute.never",
        "Never Reroute",
        connectors_selected,
        run_reroute_never,
    ),
];

/// `connector.move_segment`: one undoable move of one connector
/// segment (task P7-35 item 2).
///
/// Arguments: `connector` (Visio id), `segment` (index in the current
/// path) and `to` (page inches: the new y of a horizontal segment, the
/// new x of a vertical one). The moved route — the segment on `to` with
/// its neighbours stretched and one bend at a glued end — replaces the
/// connector's geometry in one undoable edit. A missing required
/// argument is a clear error and opens no dialog (AGENTS.md section 6).
pub(crate) fn run_move_segment(
    ctx: &mut AppCtx,
    inv: &Invocation,
) -> Result<Outcome, CommandError> {
    let connector_visio = required_integer(&inv.args, "connector")?;
    let index = required_integer(&inv.args, "segment")?;
    let to = required_finite(&inv.args, "to")?;
    let index = usize::try_from(index)
        .map_err(|_| CommandError::invalid_args("segment must not be negative"))?;
    let (session, page) = active_page(ctx)?;
    let connector = {
        let doc = &ctx.active_document().ok_or_else(no_document)?.document;
        let connector = doc
            .shape_by_visio_id(page, connector_visio)
            .ok_or_else(|| {
                CommandError::invalid_args(format!(
                    "unknown connector shape id {connector_visio} on the active page"
                ))
            })?;
        if !doc.is_connector(connector) {
            return Err(CommandError::invalid_args(format!(
                "shape {connector_visio} has no connector ends"
            )));
        }
        connector
    };
    // The current path in page inches (the drawn route), moved along
    // the segment's normal. The write maps it into the parent's space
    // on its way to the geometry rows.
    let moved = {
        let doc = &ctx.active_document().ok_or_else(no_document)?.document;
        let path = stored_path(doc, connector).ok_or_else(|| {
            CommandError::failed(format!(
                "connector {connector_visio} has no straight-segment path to move"
            ))
        })?;
        let moved = larrygram_geom::move_segment(&path, index, to).map_err(|error| {
            CommandError::invalid_args(format!("cannot move the segment: {error}"))
        })?;
        let mut parent = Vec::with_capacity(moved.len());
        for point in moved {
            parent.push(end_cells_point(doc, connector, point)?);
        }
        parent
    };
    let mut edit = {
        let doc = &ctx.active_document().ok_or_else(no_document)?.document;
        SegmentEdit::new(doc, session, connector, moved)?
    };
    edit.apply(ctx)?;
    Ok(edit_outcome(
        "Moved connector segment".to_owned(),
        Vec::new(),
        Box::new(edit),
    ))
}

/// `connector.dyn.conn.reroute` (Reset Connector, task P7-35 item 4):
/// re-routes the selected connectors fresh. Not an edit when nothing
/// changed.
pub(crate) fn run_reset(ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
    let (_session, page) = active_page(ctx)?;
    let targets = selected_connectors(ctx)?;
    let routes = reroute_connectors(ctx, page, &targets)?;
    let message = if targets.len() == 1 {
        "Re-routed 1 connector".to_owned()
    } else {
        format!("Re-routed {} connectors", targets.len())
    };
    Ok(match routes {
        Some(edit) => edit_outcome(message, Vec::new(), edit),
        None => outcome(message, Vec::new()),
    })
}

/// `format.set.dyn.conn.reroute.freely` (task P7-35 item 5): writes
/// `ConFixedCode` 0 on the selected connectors.
fn run_reroute_freely(ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
    set_reroute_code(ctx, 0.0, "reroute freely")
}

/// `format.set.dyn.conn.reroute.on.crossover` (task P7-35 item 5):
/// writes `ConFixedCode` 3 on the selected connectors.
fn run_reroute_on_crossover(ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
    set_reroute_code(ctx, 3.0, "reroute on crossover")
}

/// `format.set.dyn.conn.reroute.never` (task P7-35 item 5): writes
/// `ConFixedCode` 2 on the selected connectors.
fn run_reroute_never(ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
    set_reroute_code(ctx, NEVER, "never reroute")
}

/// The body of the three reroute controls (task P7-35 item 5): one
/// undoable edit writing `ConFixedCode` on each selected connector.
fn set_reroute_code(ctx: &mut AppCtx, code: f64, spelling: &str) -> Result<Outcome, CommandError> {
    let (session, _page) = active_page(ctx)?;
    let targets = selected_connectors(ctx)?;
    let label = format!("Connector reroute behaviour: {spelling}");
    let mut edit = CompositeEdit::new(label.clone());
    {
        let doc = &ctx.active_document().ok_or_else(no_document)?.document;
        for &shape in &targets {
            let step = CellEdit::new(
                doc,
                session,
                label.clone(),
                SheetRef::Shape(shape),
                "ConFixedCode",
                CellWrite::new(mutate::literal(code), CellValue::Number(code)),
            );
            edit.push(Box::new(step?));
        }
    }
    edit.apply(ctx)?;
    Ok(edit_outcome(label, Vec::new(), Box::new(edit)))
}

/// The reroute controls and Reset Connector act on the connectors of
/// the selection (task P7-35 items 4 and 5); with none they are
/// disabled.
fn connectors_selected(ctx: &AppCtx) -> crate::Enabled {
    let has_connector = ctx.active_document().is_some_and(|session| {
        session.active_page.is_some_and(|page| {
            !style_targets(&session.document, page, &ctx.selection.shapes).is_empty()
        })
    });
    if has_connector {
        crate::Enabled::yes()
    } else {
        crate::Enabled::no("no connector in the selection")
    }
}

/// The selected connectors the reroute controls act on.
fn selected_connectors(ctx: &AppCtx) -> Result<Vec<ShapeId>, CommandError> {
    let (_, page) = active_page(ctx)?;
    let doc = &ctx.active_document().ok_or_else(no_document)?.document;
    let targets = style_targets(doc, page, &ctx.selection.shapes);
    if targets.is_empty() {
        return Err(CommandError::failed("no connector in the selection"));
    }
    Ok(targets)
}

/// The `ConFixedCode` of the connector the reroute controls show as
/// checked (task P7-35 item 5): the selection anchor's (or first
/// selected connector's) value, `None` with no connector selected or
/// no active document.
pub fn connector_reroute_code(app: &AppCtx) -> Option<f64> {
    let session = app.active_document()?;
    let doc = &session.document;
    let selected = app
        .selection
        .anchor
        .or_else(|| app.selection.shapes.first().copied())
        .filter(|shape| doc.is_connector(*shape))?;
    Some(conn_fixed_code(doc, selected))
}

/// Whether the connector is a Never Reroute one (`ConFixedCode` 2, ADR
/// 0084 rule 7): the scoped re-route keeps its stored route.
pub(crate) fn never_reroutes(doc: &Document, connector: ShapeId) -> bool {
    conn_fixed_code(doc, connector) == NEVER
}

/// The connector's `ConFixedCode`, inherited or own (0 when unread:
/// reroute freely is the default).
fn conn_fixed_code(doc: &Document, connector: ShapeId) -> f64 {
    let mut reader = CellReader::new(doc);
    reader
        .number(SheetRef::Shape(connector), &CellName::new("ConFixedCode"))
        .unwrap_or(0.0)
}

/// The kept route of a Never Reroute connector (task P7-35 item 5, ADR
/// 0084 rule 7): the stored path with its ends replaced by the resolved
/// glue points — the interior points keep their positions and the end
/// segments stretch to reach their glue. `None` when there is no
/// stored straight-segment path to keep (a curved or formula-driven
/// geometry is left to a fresh route or its formulas).
pub(crate) fn kept_route(
    doc: &Document,
    page: PageId,
    connector: ShapeId,
    ends: &[Option<Connect>; 2],
    warnings: &mut Vec<String>,
) -> Option<KeptRoute> {
    if crate::recalc::geometry_is_formula_driven(doc, connector) {
        return None;
    }
    // The stored interior points, in the parent's coordinates (the
    // write's space): a 1-D path write re-frames the connector's box,
    // so the points that stay are the ones the parent holds.
    let mut path = parent_path(doc, connector)?;
    if path.len() < 2 {
        return None;
    }
    // Both ends resolve together (ADR 0084 rule 3); a glued end that
    // resolves nowhere leaves the route to the fresh pass, which keeps
    // it unchanged with the same warning.
    let (begin, end, _boxes) = resolved_ends(
        doc,
        page,
        connector,
        [
            ends.first().and_then(Option::as_ref),
            ends.get(1).and_then(Option::as_ref),
        ],
        [None, None],
        warnings,
    )
    .ok()?;
    for (candidate, connect) in [
        (begin, ends.first().and_then(Option::as_ref)),
        (end, ends.get(1).and_then(Option::as_ref)),
    ] {
        let finite = candidate.point.x.is_finite() && candidate.point.y.is_finite();
        if connect.is_some() && !finite {
            return None;
        }
    }
    let begin = end_cells_point(doc, connector, begin.point).ok()?;
    let finish = end_cells_point(doc, connector, end.point).ok()?;
    if let Some(first) = path.first_mut() {
        *first = begin;
    }
    if let Some(last) = path.last_mut() {
        *last = finish;
    }
    Some(KeptRoute { path })
}

/// The staged keep of a Never Reroute connector's stored route (task
/// P7-35 item 5): the path whose interior points keep their positions
/// and whose ends follow the glue.
pub(crate) struct KeptRoute {
    /// The path in the parent's coordinates (the write's space).
    path: Vec<Point>,
}

impl KeptRoute {
    /// Writes the kept route: the path write refreshes the ends, the
    /// pins and the rows, and lands the interior points back where the
    /// parent holds them.
    pub(crate) fn apply(
        &self,
        doc: &mut Document,
        connector: ShapeId,
    ) -> Result<(), larrygram_model::Error> {
        doc.write_connector_path(connector, &self.path)
    }
}

/// The connector's stored geometry as its path in page inches: the
/// `MoveTo`/`LineTo` rows through the shape's page transform. A
/// geometry with any other row (a curve) answers `None`.
fn stored_path(doc: &Document, connector: ShapeId) -> Option<Vec<Point>> {
    let transform = page_transform(doc, connector).ok()?;
    polyline(doc, connector, transform)
}

/// The connector's stored geometry in its parent's coordinates (the
/// write's space of `Document::write_connector_path`).
fn parent_path(doc: &Document, connector: ShapeId) -> Option<Vec<Point>> {
    let mut reader = CellReader::new(doc);
    let frame = local_transform(&mut reader, connector).ok()?;
    polyline(doc, connector, frame)
}

/// The connector's `MoveTo`/`LineTo` rows through `transform`. A
/// geometry with any other row (a curve) answers `None`.
fn polyline(doc: &Document, connector: ShapeId, transform: Transform) -> Option<Vec<Point>> {
    let geometry = doc.shape_geometry(connector).ok()?;
    let path = geometry.paths.first()?;
    let mut points = Vec::with_capacity(path.rows.len());
    for row in &path.rows {
        match row {
            GeometryRow::MoveTo { x, y } | GeometryRow::LineTo { x, y } => {
                let point = transform.apply(Point::new(*x, *y));
                if !point.x.is_finite() || !point.y.is_finite() {
                    return None;
                }
                points.push(point);
            }
            _ => return None,
        }
    }
    Some(points)
}

/// The shape's own-box transform into page inches (the chain of its
/// parents' local transforms, innermost first).
fn page_transform(doc: &Document, shape: ShapeId) -> Result<Transform, CommandError> {
    let mut chain = vec![shape];
    let mut current = shape;
    for _ in 0..128 {
        let parent = doc
            .shape(current)
            .ok_or_else(|| CommandError::failed(format!("unknown shape id {}", current.index)))?
            .parent;
        let Some(parent) = parent else {
            break;
        };
        if chain.contains(&parent) {
            return Err(CommandError::failed("cyclic shape parent chain"));
        }
        chain.push(parent);
        current = parent;
    }
    let mut reader = CellReader::new(doc);
    let mut transform = Transform::identity();
    for id in chain.iter().rev() {
        let local = local_transform(&mut reader, *id)?;
        transform = local.then(transform);
    }
    Ok(transform)
}

/// The local-box transform of one shape (translate `-LocPin`, flip,
/// rotate, translate `Pin`).
fn local_transform(reader: &mut CellReader<'_>, shape: ShapeId) -> Result<Transform, CommandError> {
    let sheet = SheetRef::Shape(shape);
    let number = |reader: &mut CellReader<'_>, name: &str| {
        reader
            .number(sheet, &CellName::new(name))
            .map_err(model_error)
    };
    let pin = Point::new(number(reader, "PinX")?, number(reader, "PinY")?);
    let loc_pin = Point::new(number(reader, "LocPinX")?, number(reader, "LocPinY")?);
    let angle = number(reader, "Angle")?;
    let flip_x = reader
        .boolean(sheet, &CellName::new("FlipX"))
        .map_err(model_error)?;
    let flip_y = reader
        .boolean(sheet, &CellName::new("FlipY"))
        .map_err(model_error)?;
    Ok(Transform::translate(Point::new(-loc_pin.x, -loc_pin.y))
        .then(Transform::scale(
            if flip_x { -1.0 } else { 1.0 },
            if flip_y { -1.0 } else { 1.0 },
        ))
        .then(Transform::rotate(Angle::from_radians(angle)))
        .then(Transform::translate(pin)))
}

/// One undoable connector path write (task P7-35 item 2): `revert`
/// restores the exact prior sheet, as the router's path edit does.
struct SegmentEdit {
    session: SessionId,
    connector: ShapeId,
    /// The connector's sheet before the write: a 1-D path write also
    /// moves its endpoint, pin and size cells.
    old_sheet: Sheet,
    /// The moved path in the parent's coordinates (the write's space).
    points: Vec<Point>,
    /// The row cells the path write replaces (ADR 0056's dirty set).
    dirty: Vec<CellAddress>,
}

impl SegmentEdit {
    fn new(
        doc: &Document,
        session: SessionId,
        connector: ShapeId,
        points: Vec<Point>,
    ) -> Result<Self, CommandError> {
        let old_sheet = doc
            .shape(connector)
            .ok_or_else(|| CommandError::failed("the connector shape is missing"))?
            .sheet
            .clone();
        let mut dirty = Vec::new();
        for (position, _) in points.iter().enumerate() {
            let target = RowTarget {
                position: Some(position),
                ix: Some(i32::try_from(position).unwrap_or(i32::MAX)),
            };
            for cell in ["X", "Y"] {
                dirty.push(CellAddress::in_row(
                    SheetRef::Shape(connector),
                    larrygram_model::SectionName::from_static("Geometry"),
                    target,
                    CellName::new(cell),
                ));
            }
        }
        if doc.is_connector(connector) {
            // The 1-D cells the path write refreshes (task P7-31).
            for cell in [
                "BeginX", "BeginY", "EndX", "EndY", "PinX", "PinY", "Width", "Height", "LocPinX",
                "LocPinY", "Angle",
            ] {
                dirty.push(CellAddress::new(
                    SheetRef::Shape(connector),
                    CellName::new(cell),
                ));
            }
        }
        Ok(Self {
            session,
            connector,
            old_sheet,
            points,
            dirty,
        })
    }
}

impl Edit for SegmentEdit {
    fn label(&self) -> &str {
        "Move connector segment"
    }

    fn dirty_cells(&self) -> &[CellAddress] {
        &self.dirty
    }

    fn apply(&mut self, ctx: &mut AppCtx) -> Result<(), CommandError> {
        let session = self.session;
        let connector = self.connector;
        let points = self.points.clone();
        let doc = document_mut(ctx, session)?;
        doc.write_connector_path(connector, &points)
            .map_err(model_error)?;
        ctx.touch_session(session);
        Ok(())
    }

    fn revert(&mut self, ctx: &mut AppCtx) -> Result<(), CommandError> {
        let session = self.session;
        let sheet = self.old_sheet.clone();
        let doc = document_mut(ctx, session)?;
        if let Some(entity) = doc.shape_mut(self.connector) {
            entity.sheet = sheet;
        }
        ctx.touch_session(session);
        Ok(())
    }
}

/// The active document and page of `ctx`.
fn active_page(ctx: &AppCtx) -> Result<(SessionId, PageId), CommandError> {
    super::connector::active_page(ctx)
}

/// One required integer argument (a missing argument is a clear error,
/// never a dialog request).
fn required_integer(args: &Args, name: &str) -> Result<i32, CommandError> {
    match args.get(name) {
        None => Err(CommandError::invalid_args(format!("{name} is required"))),
        Some(_) => super::connector::need_integer(args, name, name),
    }
}

/// One required finite number argument (the strict rule of
/// [`required_integer`]).
fn required_finite(args: &Args, name: &str) -> Result<f64, CommandError> {
    match args.get(name) {
        None => Err(CommandError::invalid_args(format!("{name} is required"))),
        Some(_) => need_finite(args, name, name),
    }
}
