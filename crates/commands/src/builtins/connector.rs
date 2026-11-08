//! `connector.*` commands (P5-08).
//!
//! Dynamic connector commands: glue, unglue, style and re-route (ADR
//! 0045). Glue is data in the page's `Connects` records; the app writes
//! the routed path literally into the connector's own `Geometry` rows,
//! so the file contains what we draw (ADR 0038, ADR 0045).
//!
//! Implemented ids:
//!
//! - `connector.glue_begin`, `connector.glue_end` — args `connector`
//!   (Visio id, optional while the connector is the selection anchor),
//!   `target` (Visio id) and optional `point` (the `Connection` row IX;
//!   absent = outline glue); one undoable edit each;
//! - `connector.unglue` — args `connector` (optional, as above), `end`
//!   (`"begin"` / `"end"`);
//! - `connector.reroute_all` — recompute every glued connector's path on
//!   the active page (idempotent, not undoable);
//! - `connector.style` — arg `style` (`"straight"` / `"right_angle"`)
//!   for the selected shapes/connectors.
//!
//! [`reroute_after_edit`] is the one hook the editing commands call
//! after a move, resize, rotate or delete: it drops glue whose target
//! is detached and re-writes only the affected connector paths.

use larrygram_geom::{
    Candidate, EndSpec, GlueEnd as GlueKind, GlueTarget, Obstacle, Point, Rect, RouteOptions,
    RouteStyle, Transform, resolve_ends, route,
};
use larrygram_model::CellReader;
use larrygram_model::{
    Cell, CellLookup, CellName, CellValue, CellWrite, Connect, ConnectorEnd, Document, EndGlue,
    GeometryRow, PageId, Row, RowTarget, Section, Shape, ShapeId, Sheet, SheetRef, mutate,
    style_cells,
};
use larrygram_shapesheet::CellAddress;

use crate::app::{AppCtx, SessionId};
use crate::edit::CompositeEdit;
use crate::error::CommandError;
use crate::undo::Edit;
use crate::{ArgValue, Args, Invocation, NeededInput, Outcome};

use super::edit::{CellEdit, PageKey, document_mut, page_key, resolve_page};
use super::{
    CommandTable, edit_outcome, has_pages, model_error, need_finite, no_document, outcome,
};

/// The implemented `connector.*` commands (P5-08).
pub(crate) const TABLE: CommandTable = &[
    (
        "connector.glue_begin",
        "Glue Begin",
        has_pages,
        run_glue_begin,
    ),
    ("connector.glue_end", "Glue End", has_pages, run_glue_end),
    ("connector.unglue", "Unglue", has_pages, run_unglue),
    (
        "connector.reroute_all",
        "Re-route Connectors",
        has_pages,
        run_reroute_all,
    ),
    ("connector.style", "Connector Style", has_pages, run_style),
    ("connector.draw", "Draw Connector", has_pages, run_draw),
];

/// Which connector endpoint a glue record names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum End {
    /// The `BeginX` end (`VisFromParts` `visBegin`, 9).
    Begin,
    /// The `EndX` end (`VisFromParts` `visEnd`, 12).
    End,
}

impl End {
    /// The `FromCell` value of this end.
    pub(super) fn cell(self) -> &'static str {
        match self {
            End::Begin => "BeginX",
            End::End => "EndX",
        }
    }

    /// The `FromPart` value of this end.
    pub(super) fn part(self) -> i32 {
        match self {
            End::Begin => mutate::VIS_BEGIN,
            End::End => mutate::VIS_END,
        }
    }

    /// This end's slot in a two-end array (`connects_of` order).
    pub(super) fn slot(self) -> usize {
        match self {
            End::Begin => 0,
            End::End => 1,
        }
    }

    /// The endpoint of one `Connect` record (the `connects_of` rule).
    pub(super) fn of(connect: &Connect) -> Self {
        if connect.from_part == mutate::VIS_END
            || connect.from_part == mutate::VIS_END_X
            || connect.from_part == mutate::VIS_END_Y
            || connect.from_cell.starts_with("End")
        {
            End::End
        } else {
            End::Begin
        }
    }

    /// This end in the model's vocabulary (task P7-31).
    pub(super) fn connector_end(self) -> ConnectorEnd {
        match self {
            End::Begin => ConnectorEnd::Begin,
            End::End => ConnectorEnd::End,
        }
    }

    /// The endpoint spelled by the `connector.unglue` argument.
    pub(super) fn parse(text: &str) -> Option<Self> {
        match text {
            "begin" => Some(End::Begin),
            "end" => Some(End::End),
            _ => None,
        }
    }
}

/// The active page and its session.
pub(super) fn active_page(ctx: &AppCtx) -> Result<(SessionId, PageId), CommandError> {
    let session = ctx.active_document().ok_or_else(no_document)?;
    let page = session
        .active_page
        .ok_or_else(|| CommandError::failed("no active page"))?;
    if session.document.page(page).is_none() {
        return Err(CommandError::failed(format!(
            "unknown page id {}",
            page.index
        )));
    }
    Ok((session.id, page))
}

/// One integer argument: an `ArgValue::Integer`, a whole `Number`, or a
/// decimal `Text` (the control channel may send any of the three).
pub(super) fn need_integer(args: &Args, name: &str, prompt: &str) -> Result<i32, CommandError> {
    let invalid = || CommandError::invalid_args(format!("{name} must be a whole number"));
    match args.get(name) {
        None => Err(CommandError::NeedsInput(NeededInput::Number {
            prompt: prompt.to_owned(),
            default: None,
        })),
        Some(ArgValue::Integer(value)) => i32::try_from(*value).map_err(|_| invalid()),
        Some(ArgValue::Number(value)) if value.is_finite() && value.fract() == 0.0 => {
            if *value >= f64::from(i32::MIN) && *value <= f64::from(i32::MAX) {
                Ok(*value as i32)
            } else {
                Err(invalid())
            }
        }
        Some(ArgValue::Text(text)) => text.parse::<i32>().map_err(|_| invalid()),
        Some(_) => Err(invalid()),
    }
}

/// An optional connection-row `IX` argument (absent = outline glue).
fn optional_point(args: &Args) -> Result<Option<i32>, CommandError> {
    match args.get("point") {
        None => Ok(None),
        Some(_) => need_integer(args, "point", "connection point row index").map(Some),
    }
}

/// The connector shape of a glue/unglue run: the explicit `connector`
/// Visio id, or the selection anchor (the connector tool inserts and
/// then selects the new shape, so its follow-up intents name no id).
fn connector_shape(ctx: &AppCtx, page: PageId, args: &Args) -> Result<ShapeId, CommandError> {
    let session = ctx.active_document().ok_or_else(no_document)?;
    match args.get("connector") {
        Some(_) => {
            let visio_id = need_integer(args, "connector", "connector shape id")?;
            session
                .document
                .shape_by_visio_id(page, visio_id)
                .ok_or_else(|| {
                    CommandError::invalid_args(format!(
                        "unknown connector shape id {visio_id} on the active page"
                    ))
                })
        }
        None => ctx
            .selection
            .anchor
            .filter(|shape| session.document.page_of(*shape) == Some(page))
            .ok_or_else(|| CommandError::failed("no connector selected")),
    }
}

/// The target shape of a glue run. A 1-D shape is refused: connectors
/// are never glue targets (ADR 0084 rule 1, task P7-49 goal 6).
pub(super) fn target_shape(
    ctx: &AppCtx,
    page: PageId,
    args: &Args,
) -> Result<ShapeId, CommandError> {
    let session = ctx.active_document().ok_or_else(no_document)?;
    let visio_id = need_integer(args, "target", "target shape id")?;
    let target = session
        .document
        .shape_by_visio_id(page, visio_id)
        .ok_or_else(|| {
            CommandError::invalid_args(format!(
                "unknown target shape id {visio_id} on the active page"
            ))
        })?;
    refuse_connector_target(&session.document, target, visio_id)?;
    Ok(target)
}

/// Refuses a 1-D shape as a glue target (task P7-49 goal 6): the
/// connector itself included, for every source.
fn refuse_connector_target(
    doc: &Document,
    target: ShapeId,
    visio_id: i32,
) -> Result<(), CommandError> {
    if doc.is_connector(target) {
        return Err(CommandError::invalid_args(format!(
            "target shape {visio_id} is a 1-D shape: connectors are never glue targets"
        )));
    }
    Ok(())
}

/// The `Connect` record for one endpoint glue.
pub(super) fn glue_record(
    connector: ShapeId,
    target: ShapeId,
    target_visio_id: i32,
    point: Option<i32>,
    end: End,
    doc: &Document,
) -> Result<Connect, CommandError> {
    let from_sheet = doc
        .shape(connector)
        .ok_or_else(|| CommandError::failed(format!("unknown shape id {}", connector.index)))?
        .visio_id;
    let (to_cell, to_part) = match point {
        Some(ix) if ix >= 0 => {
            let points = doc.connection_points_indexed(target);
            let point_count = points.len();
            if !points.iter().any(|(row, _)| *row == ix) {
                return Err(CommandError::invalid_args(format!(
                    "connection point {ix} does not exist (the target has {point_count})"
                )));
            }
            let cell = ix
                .checked_add(1)
                .map(|row| format!("Connections.X{row}"))
                .ok_or_else(|| CommandError::invalid_args("connection point is too large"))?;
            let part = mutate::VIS_CONNECTION_POINT
                .checked_add(ix)
                .ok_or_else(|| CommandError::invalid_args("connection point is too large"))?;
            (cell, part)
        }
        Some(_) => {
            return Err(CommandError::invalid_args("point must not be negative"));
        }
        None => ("PinX".to_owned(), mutate::VIS_WHOLE_SHAPE),
    };
    Ok(Connect {
        from_sheet,
        from_cell: end.cell().to_owned(),
        from_part: end.part(),
        to_sheet: target_visio_id,
        to_cell,
        to_part,
        ..Connect::default()
    })
}

/// One undoable endpoint replacement (task P7-31: the glue formula,
/// the cached values, the trigger and the `Connects` record in one
/// edit).
///
/// `apply` writes the end's endpoint cells and trigger the way Visio
/// stores them (ADR 0084 rule 2) and installs the new record (or
/// removes the endpoint for an unglue); `revert` puts the exact
/// previous records and cells back. A shape without the endpoint cells
/// (a legacy record-only connector) keeps its records as its only
/// glue: nothing migrates it.
#[derive(Debug)]
pub(super) struct GlueEdit {
    session: SessionId,
    label: String,
    page: PageKey,
    connector: ShapeId,
    end: End,
    old: Vec<(usize, Connect)>,
    new: Option<Connect>,
    /// The connector's Visio `ID` (unique on the page, stable over undo).
    connector_visio: i32,
    /// The connector's sheet before the glue (restored on `revert`, so
    /// one undo restores the cells and the routed path exactly).
    old_sheet: Sheet,
    /// The point the end lands on (the parent's coordinates, the
    /// cached `V`).
    at: Point,
    /// The glue kind the endpoint formula spells (ADR 0084 rule 2).
    glue: EndGlue,
    /// The cells `apply` writes (the recalc dirty set, ADR 0056).
    dirty: Vec<CellAddress>,
}

impl GlueEdit {
    /// Stages the replacement of `connector`'s `end` glue with `new`
    /// (the end lands on `at`).
    #[allow(clippy::too_many_arguments)] // one staging seam
    pub(super) fn new(
        doc: &Document,
        session: SessionId,
        label: impl Into<String>,
        page: PageId,
        connector: ShapeId,
        connector_visio: i32,
        end: End,
        new: Option<Connect>,
        at: Point,
    ) -> Result<Self, CommandError> {
        let old = endpoint_records(doc, page, connector_visio, end)?;
        let old_sheet = doc
            .shape(connector)
            .map(|entity| entity.sheet.clone())
            .unwrap_or_default();
        let glue = end_glue_of(new.as_ref())?;
        let dirty = end_cells(end)
            .iter()
            .map(|name| CellAddress::new(SheetRef::Shape(connector), CellName::new(*name)))
            .collect();
        Ok(Self {
            session,
            label: label.into(),
            page: page_key(doc, page)?,
            connector,
            end,
            old,
            new,
            connector_visio,
            old_sheet,
            at,
            glue,
            dirty,
        })
    }

    /// The live page of this edit.
    fn page(&self, doc: &Document) -> Result<PageId, CommandError> {
        let fallback = PageId { index: 0 };
        Ok(resolve_page(doc, &self.page, fallback))
    }

    /// Installs one endpoint state.
    fn set(&self, ctx: &mut AppCtx, state: EndpointState<'_>) -> Result<(), CommandError> {
        let session = self.session;
        let doc = document_mut(ctx, session)?;
        let page = self.page(doc)?;
        let connector = self.connector_visio;
        let Some(entity) = doc.pages.get_mut(page.index as usize) else {
            return Ok(());
        };
        let mut kept = Vec::with_capacity(entity.connects.len() + 1);
        for connect in entity.connects.drain(..) {
            if connect.from_sheet == connector && End::of(&connect) == self.end {
                continue;
            }
            kept.push(connect);
        }
        match state {
            EndpointState::One(connect) => kept.push(connect.clone()),
            EndpointState::Many(records) => {
                for (index, connect) in records {
                    let at = (*index).min(kept.len());
                    kept.insert(at, connect.clone());
                }
            }
        }
        entity.connects = kept;
        ctx.touch_session(session);
        Ok(())
    }
}

/// The endpoint state an [`GlueEdit`] installs.
enum EndpointState<'a> {
    /// One new record (glue).
    One(&'a Connect),
    /// The captured records at their original positions (revert).
    Many(&'a [(usize, Connect)]),
}

impl Edit for GlueEdit {
    fn label(&self) -> &str {
        &self.label
    }

    fn dirty_cells(&self) -> &[CellAddress] {
        &self.dirty
    }

    fn apply(&mut self, ctx: &mut AppCtx) -> Result<(), CommandError> {
        match &self.new {
            Some(connect) => self.set(ctx, EndpointState::One(connect))?,
            None => self.set(ctx, EndpointState::Many(&[]))?,
        }
        self.write_end(ctx)
    }

    fn revert(&mut self, ctx: &mut AppCtx) -> Result<(), CommandError> {
        let old = self.old.clone();
        let sheet = self.old_sheet.clone();
        self.set(ctx, EndpointState::Many(&old))?;
        let session = self.session;
        let doc = document_mut(ctx, session)?;
        if let Some(entity) = doc.shape_mut(self.connector) {
            entity.sheet = sheet;
        }
        ctx.touch_session(session);
        Ok(())
    }
}

impl GlueEdit {
    /// Writes the end's endpoint cells and trigger (ADR 0084 rule 2).
    ///
    /// A shape without the endpoint cells (a legacy record-only
    /// connector) is not migrated: only its records change.
    fn write_end(&self, ctx: &mut AppCtx) -> Result<(), CommandError> {
        let session = self.session;
        let doc = document_mut(ctx, session)?;
        if !doc.is_connector(self.connector) {
            return Ok(());
        }
        doc.write_connector_end(self.connector, self.end.connector_end(), self.at, self.glue)
            .map_err(model_error)?;
        ctx.touch_session(session);
        Ok(())
    }
}

/// The three cells one end's glue writes: its endpoint pair and its
/// trigger (ADR 0084 rule 2).
fn end_cells(end: End) -> [&'static str; 3] {
    match end {
        End::Begin => ["BeginX", "BeginY", "BegTrigger"],
        End::End => ["EndX", "EndY", "EndTrigger"],
    }
}

/// The glue kind of a record (ADR 0084 rule 2's table): point glue for
/// a `Connections.X<n>` row (`ToPart` 100 + the row index), dynamic
/// glue for a shape's pin, free when there is no record.
fn end_glue_of(record: Option<&Connect>) -> Result<EndGlue, CommandError> {
    let Some(connect) = record else {
        return Ok(EndGlue::Free);
    };
    Ok(match connection_index(connect) {
        Some(ix) => EndGlue::Point {
            sheet: connect.to_sheet,
            ix: u32::try_from(ix).map_err(|_| {
                CommandError::invalid_args("the connection row index is out of range")
            })?,
        },
        None => EndGlue::Dynamic {
            sheet: connect.to_sheet,
        },
    })
}

/// One undoable clear of the glue formulas of every end glued to a
/// shape a delete removes (ADR 0084 rule 2, task P7-31 goal 3): each
/// end keeps its last point as a literal and its trigger names its own
/// sheet. The `Connects` records go with the delete itself (the model's
/// `detach_shape` removes them; the delete's own edit restores them).
///
/// A legacy record-only connector (no endpoint cells) has no formulas
/// to clear and is not migrated.
#[derive(Debug)]
pub(crate) struct GlueClearEdit {
    session: SessionId,
    items: Vec<ClearedConnector>,
}

/// One connector whose glued ends [`GlueClearEdit`] frees.
#[derive(Debug)]
struct ClearedConnector {
    connector: ShapeId,
    /// The freed ends and the points they keep.
    ends: Vec<(End, Point)>,
    /// The connector's sheet before the clear (restored on `revert`).
    old_sheet: Sheet,
}

impl GlueClearEdit {
    /// Stages the clear for every connector end glued to one of
    /// `targets`, or `None` when no end is glued to them. Staged before
    /// the delete applies: the records still name their targets.
    pub(crate) fn stage(
        doc: &Document,
        session: SessionId,
        page: PageId,
        targets: &[ShapeId],
    ) -> Option<Box<dyn Edit>> {
        // Every shape the delete removes, members of a removed group
        // included (P7-31 review finding 3).
        let removed = subtree_visio_ids(doc, targets);
        let entity = doc.page(page)?;
        // The ends glued to a removed target, in file order, grouped by
        // connector (one connector may lose both ends). A record whose
        // connector is gone (a dangling record) is skipped; it is no
        // reason to clear nothing (review finding 5).
        let mut grouped: Vec<(ShapeId, Vec<(End, Point)>)> = Vec::new();
        for connect in &entity.connects {
            if !is_connector_end(connect) || !removed.contains(&connect.to_sheet) {
                continue;
            }
            let Some(connector) = doc.shape_by_visio_id(page, connect.from_sheet) else {
                continue;
            };
            if !doc.is_connector(connector) {
                continue;
            }
            let end = End::of(connect);
            // The freed end keeps its stored point (review finding 6),
            // not a fresh resolution of the glue.
            let at = stored_end_point(doc, connector, end);
            match grouped.iter_mut().find(|(shape, _)| *shape == connector) {
                Some((_, ends)) => ends.push((end, at)),
                None => grouped.push((connector, vec![(end, at)])),
            }
        }
        let items: Vec<ClearedConnector> = grouped
            .into_iter()
            .filter_map(|(connector, ends)| {
                let old_sheet = doc.shape(connector)?.sheet.clone();
                Some(ClearedConnector {
                    connector,
                    ends,
                    old_sheet,
                })
            })
            .collect();
        (!items.is_empty()).then(|| Box::new(Self { session, items }) as Box<dyn Edit>)
    }
}

/// The Visio ids of every shape in the subtrees rooted at `roots` (the
/// roots and all their members), bounded and cycle-safe.
pub(crate) fn subtree_visio_ids(doc: &Document, roots: &[ShapeId]) -> Vec<i32> {
    let mut ids = Vec::new();
    let mut seen: std::collections::HashSet<ShapeId> = std::collections::HashSet::new();
    let mut stack: Vec<ShapeId> = roots.to_vec();
    // The visited set bounds the walk: each arena shape is seen once,
    // so a cyclic child list cannot loop.
    while let Some(shape) = stack.pop() {
        if !seen.insert(shape) {
            continue;
        }
        if let Some(entity) = doc.shape(shape) {
            ids.push(entity.visio_id);
            stack.extend(entity.children.iter().copied());
        }
    }
    ids
}

/// The endpoint cells' value for one resolved page point: the cells of
/// a connector inside a group are in its parent's coordinates (task
/// P7-39's open issue), so the resolved page point maps into the
/// parent's space on its way to the cells.
pub(super) fn end_cells_point(
    doc: &Document,
    connector: ShapeId,
    at: Point,
) -> Result<Point, CommandError> {
    let parent = parent_transform(doc, connector)?;
    let inverse = parent.invert().map_err(|_| singular())?;
    Ok(inverse.apply(at))
}

/// One end's stored point as the cells hold it: the endpoint cells of a
/// connector inside a group are in its parent's coordinates, so this is
/// the value a freed end's cells keep (ADR 0084 rule 2). A shape
/// without the endpoint cells falls back to its geometry path's end in
/// page space, then the pin.
pub(super) fn stored_end_point(doc: &Document, connector: ShapeId, end: End) -> Point {
    if doc.is_connector(connector)
        && let Ok(point) = doc.connector_end(connector, end.connector_end())
    {
        return point;
    }
    end_point_page(doc, connector, end)
        .or_else(|| fallback_point(doc, connector))
        .unwrap_or(Point::new(0.0, 0.0))
}

impl Edit for GlueClearEdit {
    fn label(&self) -> &str {
        "Clear glue to deleted shapes"
    }

    fn apply(&mut self, ctx: &mut AppCtx) -> Result<(), CommandError> {
        let session = self.session;
        let doc = document_mut(ctx, session)?;
        for item in &self.items {
            for (end, at) in &item.ends {
                if doc.is_connector(item.connector) {
                    doc.write_connector_end(
                        item.connector,
                        end.connector_end(),
                        *at,
                        EndGlue::Free,
                    )
                    .map_err(model_error)?;
                }
            }
        }
        ctx.touch_session(session);
        Ok(())
    }

    fn revert(&mut self, ctx: &mut AppCtx) -> Result<(), CommandError> {
        let session = self.session;
        let doc = document_mut(ctx, session)?;
        for item in &self.items {
            if let Some(entity) = doc.shape_mut(item.connector) {
                entity.sheet = item.old_sheet.clone();
            }
        }
        ctx.touch_session(session);
        Ok(())
    }
}

/// Every record naming `connector`'s `end`, with its list position.
fn endpoint_records(
    doc: &Document,
    page: PageId,
    connector_visio_id: i32,
    end: End,
) -> Result<Vec<(usize, Connect)>, CommandError> {
    let entity = doc
        .page(page)
        .ok_or_else(|| CommandError::failed(format!("unknown page id {}", page.index)))?;
    Ok(entity
        .connects
        .iter()
        .enumerate()
        .filter(|(_, connect)| connect.from_sheet == connector_visio_id && End::of(connect) == end)
        .map(|(index, connect)| (index, connect.clone()))
        .collect())
}

/// `connector.glue_begin`: one undoable glue of the connector's begin.
pub(super) fn run_glue_begin(ctx: &mut AppCtx, inv: &Invocation) -> Result<Outcome, CommandError> {
    glue(ctx, inv, End::Begin)
}

/// `connector.glue_end`: one undoable glue of the connector's end.
pub(super) fn run_glue_end(ctx: &mut AppCtx, inv: &Invocation) -> Result<Outcome, CommandError> {
    glue(ctx, inv, End::End)
}

/// The shared glue handler.
fn glue(ctx: &mut AppCtx, inv: &Invocation, end: End) -> Result<Outcome, CommandError> {
    let (session, page) = active_page(ctx)?;
    let connector = connector_shape(ctx, page, &inv.args)?;
    let target = target_shape(ctx, page, &inv.args)?;
    let point = optional_point(&inv.args)?;
    let (target_visio_id, record, at, warnings) = {
        let doc = &ctx.active_document().ok_or_else(no_document)?.document;
        let target_visio_id = doc
            .shape(target)
            .ok_or_else(|| CommandError::failed("the target shape no longer exists"))?
            .visio_id;
        let record = glue_record(connector, target, target_visio_id, point, end, doc)?;
        let current = doc.connects_of(page, connector).map_err(model_error)?;
        let mut slots: [Option<&Connect>; 2] = [
            current.first().and_then(Option::as_ref),
            current.get(1).and_then(Option::as_ref),
        ];
        slots[end.slot()] = Some(&record);
        let mut resolve_warnings = Vec::new();
        let (begin, finish, _boxes) = resolved_ends(
            doc,
            page,
            connector,
            slots,
            [None, None],
            &mut resolve_warnings,
        )?;
        let at = match end {
            End::Begin => begin.point,
            End::End => finish.point,
        };
        // A resolution that lands nowhere keeps the end's current
        // point: the cached `V` must stay finite (the model refuses a
        // non-finite endpoint).
        let at = if at.x.is_finite() && at.y.is_finite() {
            at
        } else {
            end_point_page(doc, connector, end).unwrap_or(Point::new(0.0, 0.0))
        };
        let at = end_cells_point(doc, connector, at)?;
        (target_visio_id, record, at, resolve_warnings)
    };
    let label = match end {
        End::Begin => "Glue connector begin",
        End::End => "Glue connector end",
    };
    let staged = {
        let doc = &ctx.active_document().ok_or_else(no_document)?.document;
        GlueEdit::new(
            doc,
            session,
            label,
            page,
            connector,
            record.from_sheet,
            end,
            Some(record),
            at,
        )
    };
    let mut edit = staged?;
    edit.apply(ctx)?;
    let routes = reroute_connectors(ctx, page, &[connector])?;
    Ok(edit_outcome(
        match end {
            End::Begin => format!("Connector begin glued to shape {target_visio_id}"),
            End::End => format!("Connector end glued to shape {target_visio_id}"),
        },
        warnings,
        with_routes(Box::new(edit), routes),
    ))
}

/// `connector.unglue`: removes one endpoint's glue record.
pub(super) fn run_unglue(ctx: &mut AppCtx, inv: &Invocation) -> Result<Outcome, CommandError> {
    let (session, page) = active_page(ctx)?;
    let connector = connector_shape(ctx, page, &inv.args)?;
    let end = match inv.args.text("end") {
        Some(text) => End::parse(text)
            .ok_or_else(|| CommandError::invalid_args("end must be begin or end"))?,
        None => {
            return Err(CommandError::NeedsInput(NeededInput::Text {
                prompt: "which end to unglue (begin or end)".to_owned(),
                default: None,
            }));
        }
    };
    let label = match end {
        End::Begin => "Unglue connector begin",
        End::End => "Unglue connector end",
    };
    let mut edit = {
        let doc = &ctx.active_document().ok_or_else(no_document)?.document;
        let connector_visio = doc
            .shape(connector)
            .map(|entry| entry.visio_id)
            .unwrap_or_default();
        let at = stored_end_point(doc, connector, end);
        let staged = GlueEdit::new(
            doc,
            session,
            label,
            page,
            connector,
            connector_visio,
            end,
            None,
            at,
        );
        staged?
    };
    edit.apply(ctx)?;
    let warnings = Vec::new();
    let routes = reroute_connectors(ctx, page, &[connector])?;
    Ok(edit_outcome(
        match end {
            End::Begin => "Connector begin unglued",
            End::End => "Connector end unglued",
        },
        warnings,
        with_routes(Box::new(edit), routes),
    ))
}

/// `connector.reroute_all`: recomputes every glued connector's path on
/// the active page. Not an edit (ADR 0040 rule 3): no undo step.
pub(super) fn run_reroute_all(
    ctx: &mut AppCtx,
    _inv: &Invocation,
) -> Result<Outcome, CommandError> {
    let (_, page) = active_page(ctx)?;
    let count = reroute_all_on(ctx, page)?;
    Ok(outcome(
        if count == 1 {
            "Re-routed 1 connector".to_owned()
        } else {
            format!("Re-routed {count} connectors")
        },
        Vec::new(),
    ))
}

/// `connector.style`: sets the connector style (ADR 0084 rule 8, task
/// P7-36 item 3). Arg `style` (`"right_angle"`, `"straight"` or
/// `"curved"`), optional `page_default` flag. Writes the cells on the
/// selected connectors, or on the page sheet when `page_default` is
/// true or nothing is selected, and re-routes the affected connectors
/// in one undoable edit.
pub(super) fn run_style(ctx: &mut AppCtx, inv: &Invocation) -> Result<Outcome, CommandError> {
    let style = match inv.args.get("style") {
        Some(ArgValue::Text(text)) => style_from_value(text).ok_or_else(|| {
            CommandError::invalid_args("style must be right_angle, straight or curved")
        })?,
        Some(_) => {
            return Err(CommandError::invalid_args(
                "style must be right_angle, straight or curved",
            ));
        }
        None => {
            return Err(CommandError::NeedsInput(NeededInput::Text {
                prompt: "connector style (right_angle, straight or curved)".to_owned(),
                default: None,
            }));
        }
    };
    set_style(ctx, style, inv.args.flag("page_default"))
}

/// The body of `connector.style` and of every style control (task
/// P7-36 item 3): one undoable edit that writes the style cells and
/// re-routes the affected connectors.
///
/// With `page_default` true, or with nothing selected, the cells go on
/// the page sheet (`RouteStyle`, `LineRouteExt`); otherwise on each
/// selected routable connector (`ShapeRouteStyle`, `ConLineRouteExt`,
/// the table of ADR 0084 rule 8). The re-route covers the styled
/// connectors and, for a page default, the page's connectors that
/// inherit their style from the page.
pub(crate) fn set_style(
    ctx: &mut AppCtx,
    style: RouteStyle,
    page_default: Option<bool>,
) -> Result<Outcome, CommandError> {
    let (session, page) = active_page(ctx)?;
    let targets: Vec<ShapeId> = {
        let doc = &ctx.active_document().ok_or_else(no_document)?.document;
        style_targets(doc, page, &ctx.selection.shapes)
    };
    let on_page = page_default.unwrap_or(false) || ctx.selection.shapes.is_empty();
    if !on_page && targets.is_empty() {
        return Err(CommandError::failed("no selected connector to style"));
    }
    let spelling = route_style_value(style);
    let mut edit = CompositeEdit::new(if on_page {
        "Set page connector style".to_owned()
    } else if targets.len() == 1 {
        "Set connector style".to_owned()
    } else {
        format!("Set connector style on {} connectors", targets.len())
    });
    {
        let doc = &ctx.active_document().ok_or_else(no_document)?.document;
        let (route_value, ext_value) = style_cells::style_values(style);
        let cells: [(&str, f64); 2] = if on_page {
            [
                (style_cells::PAGE_ROUTE_STYLE, route_value),
                (style_cells::PAGE_LINE_ROUTE_EXT, ext_value),
            ]
        } else {
            [
                (style_cells::SHAPE_ROUTE_STYLE, route_value),
                (style_cells::CON_LINE_ROUTE_EXT, ext_value),
            ]
        };
        // A page default writes the page sheet only: the selection
        // keeps its own cells.
        let shapes: &[ShapeId] = if on_page { &[] } else { &targets };
        for &shape in shapes {
            for (name, value) in cells {
                let step = CellEdit::new(
                    doc,
                    session,
                    "Set connector style",
                    SheetRef::Shape(shape),
                    name,
                    CellWrite::new(mutate::literal(value), CellValue::Number(value)),
                );
                edit.push(Box::new(step?));
            }
        }
        if on_page {
            for (name, value) in cells {
                let step = CellEdit::new(
                    doc,
                    session,
                    "Set connector style",
                    SheetRef::Page(page),
                    name,
                    CellWrite::new(mutate::literal(value), CellValue::Number(value)),
                );
                edit.push(Box::new(step?));
            }
        }
    }
    edit.apply(ctx)?;
    let affected: Vec<ShapeId> = if on_page {
        let doc = &ctx.active_document().ok_or_else(no_document)?.document;
        page_inheritors(doc, page)
    } else {
        targets.clone()
    };
    let routes = reroute_connectors(ctx, page, &affected)?;
    Ok(edit_outcome(
        if on_page {
            format!("Page connector style: {spelling}")
        } else {
            format!("Connector style: {spelling}")
        },
        Vec::new(),
        with_routes(Box::new(edit), routes),
    ))
}

/// The style targets of a selection (task P7-36 item 3): the selected
/// shapes the style cells apply to — routable 1-D shapes (ADR 0084
/// rule 1, amended: a line or a 1-D arrow takes none) and legacy
/// record-only connectors (a `FromSheet` of a glue record without
/// endpoint cells).
pub(crate) fn style_targets(doc: &Document, page: PageId, shapes: &[ShapeId]) -> Vec<ShapeId> {
    let routed = connector_visio_ids(doc, page).unwrap_or_default();
    shapes
        .iter()
        .copied()
        .filter(|shape| doc.page_of(*shape) == Some(page))
        .filter(|shape| {
            if doc.is_connector(*shape) {
                doc.is_routable(*shape)
            } else {
                doc.shape(*shape)
                    .is_some_and(|entity| routed.contains(&entity.visio_id))
            }
        })
        .collect()
}

/// The `connector.style` `style` argument spelling of a [`RouteStyle`]
/// (task P7-36 item 3).
pub fn route_style_value(style: RouteStyle) -> &'static str {
    match style {
        RouteStyle::Straight => "straight",
        RouteStyle::Curved => "curved",
        _ => "right_angle",
    }
}

/// The [`RouteStyle`] a `connector.style` `style` argument spells
/// (task P7-36 item 3).
pub fn style_from_value(value: &str) -> Option<RouteStyle> {
    match value {
        "right_angle" => Some(RouteStyle::RightAngle),
        "straight" => Some(RouteStyle::Straight),
        "curved" => Some(RouteStyle::Curved),
        _ => None,
    }
}

/// The style the style controls show as checked (task P7-36 item 3):
/// the selection anchor's (or first selected connector's) style, else
/// the page default, else right angle. `None` with no active document.
pub fn connector_style(app: &AppCtx) -> Option<RouteStyle> {
    let session = app.active_document()?;
    let doc = &session.document;
    let selected = app
        .selection
        .anchor
        .or_else(|| app.selection.shapes.first().copied());
    if let Some(shape) = selected.filter(|shape| doc.is_connector(*shape)) {
        return Some(shape_style(doc, shape));
    }
    let page = session.active_page?;
    Some(page_style(doc, page))
}

/// The style a connector routes with (ADR 0084 rule 8, task P7-36
/// item 1): its own cells first, then the page's, then right angle
/// ([`style_cells::connector_style`]). The **own** cells decide: a
/// master's cells must not block the page default from reaching a
/// fresh connector. A value that is only read is never rewritten.
fn shape_style(doc: &Document, connector: ShapeId) -> RouteStyle {
    let page_value = |name: &str| -> Option<f64> {
        let page = doc.page_of(connector)?;
        match doc
            .resolve_page_cell(page, &CellName::new(name))
            .ok()?
            .value?
        {
            CellValue::Number(value) => Some(*value),
            _ => None,
        }
    };
    style_cells::connector_style(
        own_number(doc, connector, style_cells::SHAPE_ROUTE_STYLE),
        own_number(doc, connector, style_cells::CON_LINE_ROUTE_EXT),
        page_value(style_cells::PAGE_ROUTE_STYLE),
        page_value(style_cells::PAGE_LINE_ROUTE_EXT),
    )
}

/// The page's default style (task P7-36 item 3): the page sheet's
/// `RouteStyle` and `LineRouteExt`, then right angle.
fn page_style(doc: &Document, page: PageId) -> RouteStyle {
    let value = |name: &str| -> Option<f64> {
        match doc
            .resolve_page_cell(page, &CellName::new(name))
            .ok()?
            .value?
        {
            CellValue::Number(value) => Some(*value),
            _ => None,
        }
    };
    style_cells::connector_style(
        None,
        None,
        value(style_cells::PAGE_ROUTE_STYLE),
        value(style_cells::PAGE_LINE_ROUTE_EXT),
    )
}

/// The page's routable connectors that inherit their style: a cell
/// that is missing or 0 falls through to the page (ADR 0084 rule 8),
/// so a page default change re-routes exactly these (task P7-36 item
/// 3).
fn page_inheritors(doc: &Document, page: PageId) -> Vec<ShapeId> {
    let mut out = Vec::new();
    let ids = connector_visio_ids(doc, page).unwrap_or_default();
    for visio_id in ids {
        let Some(shape) = doc.shape_by_visio_id(page, visio_id) else {
            continue;
        };
        if doc.is_routable(shape) && inherits_style(doc, shape) {
            out.push(shape);
        }
    }
    out
}

/// Whether either of the connector's own style cells is missing or 0
/// (so the page's value reaches it).
fn inherits_style(doc: &Document, connector: ShapeId) -> bool {
    let absent = |name: &str| {
        !matches!(
            own_number(doc, connector, name),
            Some(value) if value.is_finite() && value != 0.0
        )
    };
    absent(style_cells::SHAPE_ROUTE_STYLE) || absent(style_cells::CON_LINE_ROUTE_EXT)
}

/// The shape's own cached cell value, with no master chain and no
/// defaults (task P7-36 item 1: the read order names "the shape's
/// cells", and a master's cells must not block the page default).
fn own_number(doc: &Document, shape: ShapeId, name: &str) -> Option<f64> {
    match doc
        .shape(shape)?
        .sheet
        .cells
        .iter()
        .find(|cell| cell.name.as_str() == name)
        .and_then(|cell| cell.value.as_ref())
    {
        Some(CellValue::Number(value)) => Some(*value),
        _ => None,
    }
}

/// The style one connector's path takes (ADR 0084 rule 1, amended): a
/// 1-D shape that is not routable stays a straight line from begin to
/// end; a routable one (or a legacy record-only connector) follows
/// [`shape_style`].
fn route_style_for(doc: &Document, connector: ShapeId) -> RouteStyle {
    if doc.is_connector(connector) && !doc.is_routable(connector) {
        RouteStyle::Straight
    } else {
        shape_style(doc, connector)
    }
}

/// `connector.draw`: one connector drag as one undoable step
/// (task P5-17, Goal 6).
///
/// Arguments: `stencil`, `master`, `x`, `y` (the connector's insert
/// point), optional `begin_target`/`begin_point` and
/// `end_target`/`end_point` (Visio ids and connection row `IX`), and
/// optional `begin_x`/`begin_y` or `end_x`/`end_y` free endpoints in
/// page inches. A free end is routed exactly like a glued one; the
/// result is the connector's literal path, so a later re-route keeps
/// the free point until the connector itself moves.
///
/// The command inserts the connector through [`insert::run_insert_shape`]
/// and folds its edit, the endpoint glues and the routed path into one
/// [`CompositeEdit`], so undo restores the page in one step.
fn run_draw(ctx: &mut AppCtx, inv: &Invocation) -> Result<Outcome, CommandError> {
    let stencil = need_text(&inv.args, "stencil", "the stencil name")?;
    let master = need_text(&inv.args, "master", "the master name")?;
    let x = need_finite(&inv.args, "x", "connector start x in inches")?;
    let y = need_finite(&inv.args, "y", "connector start y in inches")?;
    let begin_target = optional_visio_id(&inv.args, "begin_target")?;
    let begin_point = optional_row(&inv.args, "begin_point")?;
    let end_target = optional_visio_id(&inv.args, "end_target")?;
    let end_point = optional_row(&inv.args, "end_point")?;
    let begin_free = optional_free_point(&inv.args, "begin_x", "begin_y")?;
    let end_free = optional_free_point(&inv.args, "end_x", "end_y")?;
    if begin_point.is_some() && begin_target.is_none() {
        return Err(CommandError::invalid_args(
            "begin_point needs a begin_target",
        ));
    }
    if end_point.is_some() && end_target.is_none() {
        return Err(CommandError::invalid_args("end_point needs an end_target"));
    }

    let insert_args = Args::new()
        .with("stencil", ArgValue::Text(stencil))
        .with("master", ArgValue::Text(master))
        .with("x", ArgValue::Number(x))
        .with("y", ArgValue::Number(y));
    let insert = super::insert::run_insert_shape(ctx, &Invocation::new(insert_args, inv.source))?;
    let insert_edit = insert
        .edit
        .ok_or_else(|| CommandError::failed("the connector insert produced no edit"))?;
    let mut edits: Vec<Box<dyn Edit>> = vec![insert_edit];
    let mut warnings = insert.warnings;

    let result = (|| -> Result<(), CommandError> {
        let (session, page) = active_page(ctx)?;
        let connector = ctx
            .selection
            .anchor
            .ok_or_else(|| CommandError::failed("the inserted connector is not selected"))?;
        let connector_visio = {
            let doc = &ctx.active_document().ok_or_else(no_document)?.document;
            doc.shape(connector)
                .ok_or_else(|| CommandError::failed("the inserted connector is missing"))?
                .visio_id
        };

        // Both ends resolve together (ADR 0084 rule 3): a glued end
        // lands where glue and every re-route land it, a free end at the
        // point where the user released it. No master default endpoint
        // is involved (task P7-32 goal 2).
        let mut records: [Option<Connect>; 2] = [None, None];
        for (end, target, point) in [
            (End::Begin, begin_target, begin_point),
            (End::End, end_target, end_point),
        ] {
            let Some(target_visio_id) = target else {
                continue;
            };
            let record = {
                let doc = &ctx.active_document().ok_or_else(no_document)?.document;
                let target = doc
                    .shape_by_visio_id(page, target_visio_id)
                    .ok_or_else(|| {
                        CommandError::invalid_args(format!(
                            "unknown glue target {target_visio_id} on the active page"
                        ))
                    })?;
                refuse_connector_target(doc, target, target_visio_id)?;
                glue_record(connector, target, target_visio_id, point, end, doc)?
            };
            records[end.slot()] = Some(record);
        }
        let (begin, finish, boxes) = {
            let doc = &ctx.active_document().ok_or_else(no_document)?.document;
            let mut resolve_warnings = Vec::new();
            let resolved = resolved_ends(
                doc,
                page,
                connector,
                [
                    records.first().and_then(Option::as_ref),
                    records.get(1).and_then(Option::as_ref),
                ],
                [begin_free, end_free],
                &mut resolve_warnings,
            )?;
            warnings.extend(resolve_warnings);
            resolved
        };
        let [begin_record, end_record] = records;
        for (end, record) in [(End::Begin, begin_record), (End::End, end_record)] {
            let Some(record) = record else {
                continue;
            };
            let label = match end {
                End::Begin => "Glue connector begin",
                End::End => "Glue connector end",
            };
            let mut edit = {
                let doc = &ctx.active_document().ok_or_else(no_document)?.document;
                let at = match end {
                    End::Begin => begin.point,
                    End::End => finish.point,
                };
                let at = if at.x.is_finite() && at.y.is_finite() {
                    at
                } else {
                    end_point_page(doc, connector, end).unwrap_or(Point::new(0.0, 0.0))
                };
                let at = end_cells_point(doc, connector, at)?;
                GlueEdit::new(
                    doc,
                    session,
                    label,
                    page,
                    connector,
                    connector_visio,
                    end,
                    Some(record),
                    at,
                )?
            };
            edit.apply(ctx)?;
            edits.push(Box::new(edit));
        }

        let mut path_edit = {
            let doc = &ctx.active_document().ok_or_else(no_document)?.document;
            let style = route_style_for(doc, connector);
            let options = RouteOptions::new(style)
                .with_exits(begin.exit, finish.exit)
                .with_boxes(
                    boxes.first().copied().flatten(),
                    boxes.get(1).copied().flatten(),
                );
            let points = route(begin.point, finish.point, &options).map_err(|error| {
                CommandError::failed(format!("cannot route connector: {error}"))
            })?;
            if style == RouteStyle::Curved {
                PathEdit::curved(doc, session, connector, points)?
            } else {
                PathEdit::new(doc, session, connector, points)?
            }
        };
        path_edit.apply(ctx)?;
        edits.push(Box::new(path_edit));
        Ok(())
    })();

    if let Err(error) = result {
        for edit in edits.iter_mut().rev() {
            let _ = edit.revert(ctx);
        }
        return Err(error);
    }

    let mut composite = CompositeEdit::new("Draw connector");
    for edit in edits {
        composite.push(edit);
    }
    Ok(edit_outcome(
        "Connector drawn",
        warnings,
        Box::new(composite),
    ))
}

/// Both resolved endpoints of one connector, in page space, through
/// the one resolution of ADR 0084 rule 3 (`larrygram_geom::glue`).
///
/// `records` is the glue state the resolution sees: the `Connect`
/// record of each end, `None` for a free end. `free_at` names the page
/// point a free end sits at when it is not the end's own stored point
/// (a fresh draw lands a free end where the user released it). A record
/// whose target is gone, or a free end without a readable point, falls
/// back to the end's own stored point. The returned flag is true when a
/// record named a connection row its target does not have (the end fell
/// back to dynamic glue, with one warning per such end).
pub(super) fn resolved_ends(
    doc: &Document,
    page: PageId,
    connector: ShapeId,
    records: [Option<&Connect>; 2],
    free_at: [Option<Point>; 2],
    warnings: &mut Vec<String>,
) -> Result<(Candidate, Candidate, [Option<Obstacle>; 2]), CommandError> {
    let mut targets: [Option<GlueTarget>; 2] = [None, None];
    let mut kinds: [Option<GlueKind>; 2] = [None, None];
    let mut free: [Option<Point>; 2] = [None, None];
    let connector_visio = doc
        .shape(connector)
        .map(|entity| entity.visio_id)
        .unwrap_or_default();
    for (slot, end) in [End::Begin, End::End].into_iter().enumerate() {
        match records.get(slot).copied().flatten() {
            Some(connect) => {
                let target = doc
                    .shape_by_visio_id(page, connect.to_sheet)
                    .map(|target| glue_target(doc, target))
                    .transpose();
                match target {
                    Ok(Some(target)) => {
                        targets[slot] = Some(target);
                        kinds[slot] = Some(match connection_index(connect) {
                            Some(ix) => GlueKind::Point(u32::try_from(ix).unwrap_or(u32::MAX)),
                            None => GlueKind::Shape,
                        });
                    }
                    Ok(None) => {
                        free[slot] = free_at
                            .get(slot)
                            .copied()
                            .flatten()
                            .or_else(|| end_point_page(doc, connector, end))
                            .or_else(|| fallback_point(doc, connector));
                    }
                    Err(error) => {
                        // A target whose facts cannot be read (a cyclic
                        // parent chain, a broken transform): the end
                        // keeps its point and the run degrades to one
                        // warning (never a failure, AGENTS.md section 6).
                        warnings.push(format!(
                            "connector {connector_visio} cannot resolve its glue to shape {}: {error}",
                            connect.to_sheet
                        ));
                        free[slot] = free_at
                            .get(slot)
                            .copied()
                            .flatten()
                            .or_else(|| end_point_page(doc, connector, end))
                            .or_else(|| fallback_point(doc, connector));
                    }
                }
            }
            None => {
                free[slot] = free_at
                    .get(slot)
                    .copied()
                    .flatten()
                    .or_else(|| end_point_page(doc, connector, end))
                    .or_else(|| fallback_point(doc, connector));
            }
        }
    }
    let spec = |slot: usize| -> EndSpec<'_> {
        match (
            targets.get(slot).and_then(Option::as_ref),
            kinds.get(slot).copied().flatten(),
        ) {
            (Some(target), Some(kind)) => EndSpec::Glued(target, kind),
            _ => EndSpec::Free(
                free.get(slot)
                    .copied()
                    .flatten()
                    .unwrap_or(Point::new(0.0, 0.0)),
            ),
        }
    };
    let (begin, end, missing) = resolve_ends(&spec(0), &spec(1));
    if missing {
        for slot in [0usize, 1usize] {
            let (Some(target), Some(GlueKind::Point(ix))) = (
                targets.get(slot).and_then(Option::as_ref),
                kinds.get(slot).copied().flatten(),
            ) else {
                continue;
            };
            let landed = target
                .points
                .iter()
                .position(|(row, _, _)| *row == ix)
                .and_then(|position| target.candidates().into_iter().nth(position))
                .filter(|candidate| candidate.point.x.is_finite() && candidate.point.y.is_finite());
            if landed.is_none() {
                warnings.push(format!(
                    "connector {connector_visio} has no connection point {ix}; glued to the shape instead"
                ));
            }
        }
    }
    let boxes = [
        targets
            .first()
            .and_then(Option::as_ref)
            .map(GlueTarget::obstacle),
        targets
            .get(1)
            .and_then(Option::as_ref)
            .map(GlueTarget::obstacle),
    ];
    Ok((begin, end, boxes))
}

/// The end's current point in page inches: its endpoint cells (which a
/// connector inside a group keeps in its parent's coordinates) through
/// the parent transform, else its geometry path's end. A shape without
/// the endpoint cells (a legacy record-only connector) reads its
/// geometry: `connector_end` alone would answer the cell defaults.
fn end_point_page(doc: &Document, connector: ShapeId, end: End) -> Option<Point> {
    if doc.is_connector(connector)
        && let Ok(point) = doc.connector_end(connector, end.connector_end())
    {
        return Some(parent_transform(doc, connector).ok()?.apply(point));
    }
    let transform = page_transform(doc, connector).ok()?;
    let (start, last) = geometry_endpoints(doc, connector, transform)?;
    Some(match end {
        End::Begin => start,
        End::End => last,
    })
}

/// The connector's pin in page inches (the last-resort end point).
fn fallback_point(doc: &Document, connector: ShapeId) -> Option<Point> {
    let transform = page_transform(doc, connector).ok()?;
    Some(transform_point(transform, loc_pin(doc, connector)))
}

/// One required text argument.
fn need_text(args: &Args, name: &str, prompt: &str) -> Result<String, CommandError> {
    match args.text(name) {
        Some(text) => Ok(text.to_owned()),
        None => Err(CommandError::NeedsInput(NeededInput::Text {
            prompt: prompt.to_owned(),
            default: None,
        })),
    }
}

/// One optional Visio shape id argument.
pub(super) fn optional_visio_id(args: &Args, name: &str) -> Result<Option<i32>, CommandError> {
    match args.get(name) {
        None => Ok(None),
        Some(_) => need_integer(args, name, "shape id").map(Some),
    }
}

/// One optional connection row `IX` argument.
pub(super) fn optional_row(args: &Args, name: &str) -> Result<Option<i32>, CommandError> {
    match args.get(name) {
        None => Ok(None),
        Some(_) => {
            let value = need_integer(args, name, "connection point row index")?;
            if value < 0 {
                return Err(CommandError::invalid_args(format!(
                    "{name} must not be negative"
                )));
            }
            Ok(Some(value))
        }
    }
}

/// One free endpoint `(x, y)` pair, or `None` when neither is given.
fn optional_free_point(
    args: &Args,
    x_name: &str,
    y_name: &str,
) -> Result<Option<Point>, CommandError> {
    match (args.get(x_name), args.get(y_name)) {
        (None, None) => Ok(None),
        (Some(_), None) => Err(CommandError::invalid_args(format!(
            "{y_name} is required with {x_name}"
        ))),
        (None, Some(_)) => Err(CommandError::invalid_args(format!(
            "{x_name} is required with {y_name}"
        ))),
        (Some(_), Some(_)) => {
            let x = need_finite(args, x_name, "x in inches")?;
            let y = need_finite(args, y_name, "y in inches")?;
            Ok(Some(Point::new(x, y)))
        }
    }
}

/// One undoable connector path write; `revert` restores the exact prior
/// `Geometry` sections.
struct PathEdit {
    session: SessionId,
    connector: ShapeId,
    /// The connector's sheet before the write: a 1-D path write also
    /// moves its endpoint, pin and size cells, so `revert` restores the
    /// whole sheet.
    old_sheet: Sheet,
    points: Vec<Point>,
    /// Whether the write is the curved one (one `NURBSTo` row, task
    /// P7-36 item 2).
    curved: bool,
    /// The row cells the path write replaces (ADR 0056's dirty set).
    dirty: Vec<CellAddress>,
}

impl PathEdit {
    fn new(
        doc: &Document,
        session: SessionId,
        connector: ShapeId,
        points: Vec<Point>,
    ) -> Result<Self, CommandError> {
        Self::build(doc, session, connector, points, false)
    }

    /// A curved draw (task P7-36 item 2): the write is one `MoveTo`
    /// and one `NURBSTo` row.
    fn curved(
        doc: &Document,
        session: SessionId,
        connector: ShapeId,
        points: Vec<Point>,
    ) -> Result<Self, CommandError> {
        Self::build(doc, session, connector, points, true)
    }

    fn build(
        doc: &Document,
        session: SessionId,
        connector: ShapeId,
        points: Vec<Point>,
        curved: bool,
    ) -> Result<Self, CommandError> {
        let old_sheet = doc
            .shape(connector)
            .ok_or_else(|| CommandError::failed("the connector shape is missing"))?
            .sheet
            .clone();
        let mut dirty = Vec::new();
        if curved {
            // One `MoveTo` row and one `NURBSTo` row (task P7-36 item
            // 2).
            for (position, cells) in [
                (0usize, ["X", "Y"].as_slice()),
                (1, ["X", "Y", "A", "B", "C", "D", "E"].as_slice()),
            ] {
                let target = RowTarget {
                    position: Some(position),
                    ix: Some(i32::try_from(position).unwrap_or(i32::MAX)),
                };
                for &cell in cells {
                    dirty.push(CellAddress::in_row(
                        SheetRef::Shape(connector),
                        larrygram_model::SectionName::from_static("Geometry"),
                        target,
                        CellName::new(cell),
                    ));
                }
            }
        } else {
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
            curved,
            dirty,
        })
    }
}

#[rustfmt::skip]
impl Edit for PathEdit {
    fn label(&self) -> &str { "Route connector" }

    fn dirty_cells(&self) -> &[CellAddress] {
        &self.dirty
    }

    fn apply(&mut self, ctx: &mut AppCtx) -> Result<(), CommandError> {
        let session = self.session;
        let connector = self.connector;
        let points = self.points.clone();
        let doc = document_mut(ctx, session)?;
        if self.curved {
            write_curve(doc, connector, &points)?;
        } else {
            write_path(doc, connector, &points)?;
        }
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

// ---------------------------------------------------------------------------
// Moving and copying 1-D shapes (ADR 0084 rule 1, amended 2026-11-03)
// ---------------------------------------------------------------------------

/// The page transform of a shape's parent (identity on the page).
fn parent_transform(doc: &Document, shape: ShapeId) -> Result<Transform, CommandError> {
    let parent = doc
        .shape(shape)
        .ok_or_else(|| CommandError::failed("the shape is gone"))?
        .parent;
    parent.map_or(Ok(Transform::identity()), |parent| {
        page_transform(doc, parent)
    })
}

/// Translates one 1-D shape by (`dx`, `dy`) page inches: its endpoint
/// cells move and its path is re-written in its parent's space. A
/// shape whose geometry is formula-driven keeps its rows (recalc owns
/// them): only its ends and box move. A glued end keeps its formula
/// here; the caller frees it first when it must come loose.
pub(crate) fn translate_one_d(
    doc: &mut Document,
    shape: ShapeId,
    dx: f64,
    dy: f64,
) -> Result<(), CommandError> {
    let shift = |point: Point| Point::new(point.x + dx, point.y + dy);
    let parent = parent_transform(doc, shape)?;
    let ends = [ConnectorEnd::Begin, ConnectorEnd::End].map(|end| {
        doc.connector_end(shape, end)
            .map(|point| parent.apply(point))
    });
    let [Ok(begin), Ok(end)] = ends else {
        return Err(CommandError::failed("the 1-D shape has no numeric ends"));
    };
    if crate::recalc::geometry_is_formula_driven(doc, shape) {
        let inverse = parent.invert().map_err(|_| singular())?;
        return doc
            .write_connector_box(
                shape,
                inverse.apply(shift(begin)),
                inverse.apply(shift(end)),
            )
            .map_err(model_error);
    }
    // The drawn rows through the page transform: a moved route keeps
    // its bends and, for a curved route, its control points (task
    // P7-36 item 2). Rows that give fewer than two points become the
    // straight line between the ends.
    let transform = page_transform(doc, shape)?;
    let mut curved = false;
    let mut path: Vec<Point> = Vec::new();
    if let Some(rowed) = doc
        .shape_geometry(shape)
        .ok()
        .and_then(|geometry| geometry.paths.into_iter().next())
    {
        let (width, height) = size_cells(doc, shape);
        for row in &rowed.rows {
            match row {
                GeometryRow::NurbsTo { x, y, data, .. } => {
                    curved = true;
                    if let Some(data) = data {
                        for point in &data.points {
                            let local = fraction_point(
                                point.x,
                                point.y,
                                data.x_type,
                                data.y_type,
                                width,
                                height,
                            );
                            path.push(transform.apply(local));
                        }
                    }
                    path.push(transform.apply(Point::new(*x, *y)));
                }
                _ => {
                    if let Some(point) = row_point(row) {
                        path.push(transform.apply(point));
                    }
                }
            }
        }
    }
    if path.len() < 2 {
        path = vec![begin, end];
        curved = false;
    }
    let moved: Vec<Point> = path.into_iter().map(shift).collect();
    if curved {
        write_curve(doc, shape, &moved)
    } else {
        write_path(doc, shape, &moved)
    }
}

/// The shape's cached `Width`/`Height`, for fraction-of-size NURBS
/// control points; 1.0 when the cell is missing or not a finite
/// number.
fn size_cells(doc: &Document, shape: ShapeId) -> (f64, f64) {
    let value = |name: &str| -> f64 {
        match doc
            .resolve(shape, &CellName::new(name))
            .ok()
            .and_then(|resolved| resolved.value)
        {
            Some(CellValue::Number(value)) if value.is_finite() => *value,
            _ => 1.0,
        }
    };
    (value("Width"), value("Height"))
}

/// A `NURBS()` control point in local coordinates (`xType`/`yType` 0
/// scales by the shape's size, 1 is local; the `NURBS()` reference).
fn fraction_point(x: f64, y: f64, x_type: u32, y_type: u32, width: f64, height: f64) -> Point {
    Point::new(
        if x_type == 0 { x * width } else { x },
        if y_type == 0 { y * height } else { y },
    )
}

/// The error of a parent transform that cannot be inverted.
fn singular() -> CommandError {
    CommandError::failed("singular parent transform")
}

/// The cells a 1-D move or copy writes (the recalc dirty set, ADR 0056).
fn one_d_dirty(shape: ShapeId) -> Vec<CellAddress> {
    [
        "BeginX",
        "BeginY",
        "EndX",
        "EndY",
        "BegTrigger",
        "EndTrigger",
        "PinX",
        "PinY",
        "Width",
        "Height",
        "LocPinX",
        "LocPinY",
        "Angle",
    ]
    .iter()
    .map(|name| CellAddress::new(SheetRef::Shape(shape), CellName::new(*name)))
    .collect()
}

/// One 1-D shape a [`OneDMoveEdit`] moves.
#[derive(Debug)]
struct MovedOneD {
    shape: ShapeId,
    visio_id: i32,
    /// The ends that come loose (their target does not move) and the
    /// stored point each keeps before the translation.
    freed: Vec<End>,
    /// The shape's sheet before the move (restored on `revert`).
    old_sheet: Sheet,
}

/// One undoable move of 1-D shapes (ADR 0084 rule 1, amended): the
/// endpoint cells move with the path, and a glued end comes loose,
/// formula and record together, unless its target moves in the same
/// edit.
#[derive(Debug)]
pub(crate) struct OneDMoveEdit {
    session: SessionId,
    page: PageId,
    dx: f64,
    dy: f64,
    items: Vec<MovedOneD>,
    /// The page's records before the move (restored on `revert`).
    old_connects: Vec<Connect>,
    dirty: Vec<CellAddress>,
}

impl OneDMoveEdit {
    /// Stages the move of `shapes` (1-D shapes on `page`) by (`dx`,
    /// `dy`) page inches; `moving` is every shape the whole edit moves
    /// (an end glued to one of them, or to a member of one, stays
    /// glued). `None` when `shapes` holds no 1-D shape.
    pub(crate) fn stage(
        doc: &Document,
        session: SessionId,
        page: PageId,
        shapes: &[ShapeId],
        moving: &[ShapeId],
        (dx, dy): (f64, f64),
    ) -> Result<Option<Self>, CommandError> {
        let moved = subtree_visio_ids(doc, moving);
        let entity = doc
            .page(page)
            .ok_or_else(|| CommandError::failed("the page is gone"))?;
        let mut items = Vec::new();
        let mut dirty = Vec::new();
        for &shape in shapes {
            let Some(found) = doc.shape(shape).filter(|_| doc.is_connector(shape)) else {
                continue;
            };
            let visio_id = found.visio_id;
            let freed: Vec<End> = entity
                .connects
                .iter()
                .filter(|connect| {
                    connect.from_sheet == visio_id
                        && is_connector_end(connect)
                        && !moved.contains(&connect.to_sheet)
                })
                .map(End::of)
                .fold(Vec::new(), |mut ends, end| {
                    if !ends.contains(&end) {
                        ends.push(end);
                    }
                    ends
                });
            dirty.extend(one_d_dirty(shape));
            items.push(MovedOneD {
                shape,
                visio_id,
                freed,
                old_sheet: found.sheet.clone(),
            });
        }
        if items.is_empty() {
            return Ok(None);
        }
        Ok(Some(Self {
            session,
            page,
            dx,
            dy,
            items,
            old_connects: entity.connects.clone(),
            dirty,
        }))
    }
}

impl Edit for OneDMoveEdit {
    fn label(&self) -> &str {
        "Move 1-D shape"
    }

    fn dirty_cells(&self) -> &[CellAddress] {
        &self.dirty
    }

    fn apply(&mut self, ctx: &mut AppCtx) -> Result<(), CommandError> {
        let session = self.session;
        let doc = document_mut(ctx, session)?;
        for item in &self.items {
            for end in &item.freed {
                let at = doc
                    .connector_end(item.shape, end.connector_end())
                    .map_err(model_error)?;
                doc.write_connector_end(item.shape, end.connector_end(), at, EndGlue::Free)
                    .map_err(model_error)?;
                if let Some(entity) = doc.pages.get_mut(self.page.index as usize) {
                    entity.connects.retain(|connect| {
                        !(connect.from_sheet == item.visio_id
                            && is_connector_end(connect)
                            && End::of(connect) == *end)
                    });
                }
            }
            translate_one_d(doc, item.shape, self.dx, self.dy)?;
        }
        ctx.touch_session(session);
        Ok(())
    }

    fn revert(&mut self, ctx: &mut AppCtx) -> Result<(), CommandError> {
        let session = self.session;
        let doc = document_mut(ctx, session)?;
        for item in &self.items {
            if let Some(entity) = doc.shape_mut(item.shape) {
                entity.sheet = item.old_sheet.clone();
            }
        }
        if let Some(entity) = doc.pages.get_mut(self.page.index as usize) {
            entity.connects = self.old_connects.clone();
        }
        ctx.touch_session(session);
        Ok(())
    }
}

/// The glue of one copied 1-D shape (ADR 0084 rule 1, amended): an end
/// glued to a shape copied in the same operation re-glues to the copy,
/// as the renumbered record in `records` names it (the caller adds the
/// records); every other end becomes free at its stored point, so no
/// formula names the source's targets.
pub(crate) fn rebind_copied_glue(
    doc: &mut Document,
    shape: ShapeId,
    records: &[Connect],
) -> Result<(), CommandError> {
    if !doc.is_connector(shape) {
        return Ok(());
    }
    let visio_id = doc
        .shape(shape)
        .map(|entity| entity.visio_id)
        .unwrap_or_default();
    for end in [End::Begin, End::End] {
        let Ok(at) = doc.connector_end(shape, end.connector_end()) else {
            continue;
        };
        let record = records.iter().find(|connect| {
            connect.from_sheet == visio_id && is_connector_end(connect) && End::of(connect) == end
        });
        let glue = end_glue_of(record)?;
        doc.write_connector_end(shape, end.connector_end(), at, glue)
            .map_err(model_error)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Re-route
// ---------------------------------------------------------------------------

/// Which connectors a re-route pass picks (ADR 0084 rule 5).
#[derive(Debug, Clone, Copy)]
enum Scope<'a> {
    /// Every glued connector on the page (`connector.reroute_all`).
    All,
    /// The shapes one edit changed: the pass re-routes connectors
    /// whose glued end and the connector sit on **different sides** of
    /// the move — the target is in the moved set (or its subtree) and
    /// the connector is not, or the other way round (task P7-50 goal
    /// 2). A rigid move carries a connector and both targets together,
    /// so its stored route survives it byte for byte.
    Changed(&'a [ShapeId]),
    /// The connectors whose own ends, glue or style the edit changed:
    /// exactly these re-route.
    Forced(&'a [ShapeId]),
}

/// One undoable record of the rows a re-route rewrote (task P7-49
/// goal 1): `revert` puts the old rows back byte for byte and `apply`
/// (redo) the new ones, so one undo restores the shapes and the routes
/// together and a stored custom or Visio route survives both. It keeps
/// only what `write_path` touches — the `Geometry` sections and the
/// endpoint, pin and size cells (task P7-50 minor 5) — not two whole
/// sheets. `connector.reroute_all` is **not** an edit (ADR 0040 rule
/// 3) and records nothing here.
#[derive(Debug)]
pub(crate) struct RouteEdit {
    session: SessionId,
    /// The connectors the pass rewrote, with their path rows around it.
    entries: Vec<RouteRows>,
}

/// One connector's path rows before and after the pass rewrote it
/// (task P7-50 minor 5: only what `write_path` touches).
#[derive(Debug)]
struct RouteRows {
    shape: ShapeId,
    before: PathRows,
    after: PathRows,
}

/// The rows and cells one path write touches (task P7-50 minor 5):
/// the `Geometry` sections and the 1-D endpoint, pin and size cells,
/// each with the position it holds in the sheet.
#[derive(Debug, Clone, Default)]
struct PathRows {
    geometry: Vec<(usize, Section)>,
    cells: Vec<(usize, Cell)>,
}

/// The cells a path write writes or derives: the four endpoint cells,
/// the pin and size cells of ADR 0084 rule 1 (`write_connector_ends`
/// and `write_connector_pins`), and the `LocPin*` and `Txt*`
/// derivations of `write_cell` from `Width` and `Height` (the model's
/// documented-defaults table).
const PATH_CELLS: [&str; 17] = [
    "BeginX",
    "BeginY",
    "EndX",
    "EndY",
    "PinX",
    "PinY",
    "Width",
    "Height",
    "LocPinX",
    "LocPinY",
    "Angle",
    "TxtPinX",
    "TxtPinY",
    "TxtLocPinX",
    "TxtLocPinY",
    "TxtWidth",
    "TxtHeight",
];

impl PathRows {
    /// The shape's `Geometry` sections and path cells, with their
    /// positions (a path write touches nothing else in the sheet).
    fn capture(doc: &Document, shape: ShapeId) -> Self {
        let mut out = Self::default();
        if let Some(entity) = doc.shape(shape) {
            for (index, section) in entity.sheet.sections.iter().enumerate() {
                if section.name.as_str() == "Geometry" {
                    out.geometry.push((index, section.clone()));
                }
            }
            for (index, cell) in entity.sheet.cells.iter().enumerate() {
                if PATH_CELLS.contains(&cell.name.as_str()) {
                    out.cells.push((index, cell.clone()));
                }
            }
        }
        out
    }

    /// Puts the captured rows and cells back: every `Geometry` section
    /// and path cell the write changed (or added) goes, and the
    /// captured ones return at their recorded positions (ascending, so
    /// every index is still the right slot).
    fn restore(&self, doc: &mut Document, shape: ShapeId) {
        let Some(entity) = doc.shape_mut(shape) else {
            return;
        };
        entity
            .sheet
            .sections
            .retain(|section| section.name.as_str() != "Geometry");
        for (index, section) in &self.geometry {
            let at = (*index).min(entity.sheet.sections.len());
            entity.sheet.sections.insert(at, section.clone());
        }
        entity
            .sheet
            .cells
            .retain(|cell| !PATH_CELLS.contains(&cell.name.as_str()));
        for (index, cell) in &self.cells {
            let at = (*index).min(entity.sheet.cells.len());
            entity.sheet.cells.insert(at, cell.clone());
        }
    }
}

impl Edit for RouteEdit {
    fn label(&self) -> &str {
        "Route connector"
    }

    fn apply(&mut self, ctx: &mut AppCtx) -> Result<(), CommandError> {
        let session = self.session;
        let doc = document_mut(ctx, session)?;
        for entry in &self.entries {
            entry.after.restore(doc, entry.shape);
        }
        ctx.touch_session(session);
        Ok(())
    }

    fn revert(&mut self, ctx: &mut AppCtx) -> Result<(), CommandError> {
        let session = self.session;
        let doc = document_mut(ctx, session)?;
        for entry in self.entries.iter().rev() {
            entry.before.restore(doc, entry.shape);
        }
        ctx.touch_session(session);
        Ok(())
    }
}

/// Folds a re-route's snapshot into the step it belongs to (task P7-49
/// goal 1): one undo restores the shapes and the routes together, and
/// redo puts the recorded rows back with no recompute.
pub(crate) fn with_routes(step: Box<dyn Edit>, routes: Option<Box<dyn Edit>>) -> Box<dyn Edit> {
    match routes {
        Some(routes) => {
            let mut composite = CompositeEdit::new(step.label().to_owned());
            composite.push(step);
            composite.push(routes);
            Box::new(composite)
        }
        None => step,
    }
}

/// The scoped re-route after an editing command that moved, resized,
/// rotated, flipped or deleted shapes (ADR 0084 rule 5, amended
/// 2026-11-04 for task P7-50 goal 2): connectors whose glued end and
/// own body sit on different sides of the move. Every other connector
/// keeps its stored geometry rows byte for byte.
///
/// Returns the snapshot edit the caller folds into its step, `None`
/// when the pass rewrote nothing. One warning line per dropped glue;
/// a missing page or session is `Ok(None)`: a re-route pass never
/// fails an edit that already succeeded.
pub(crate) fn reroute_after_edit(
    ctx: &mut AppCtx,
    page: PageId,
    changed: &[ShapeId],
) -> Result<Option<Box<dyn Edit>>, CommandError> {
    let (_, routes) = reroute_scoped(ctx, page, Scope::Changed(changed))?;
    Ok(routes)
}

/// The forced re-route of the connectors whose own ends, glue or style
/// the edit changed (glue, unglue, style): exactly these connectors
/// re-route, whatever the scope says.
pub(crate) fn reroute_connectors(
    ctx: &mut AppCtx,
    page: PageId,
    connectors: &[ShapeId],
) -> Result<Option<Box<dyn Edit>>, CommandError> {
    let (_, routes) = reroute_scoped(ctx, page, Scope::Forced(connectors))?;
    Ok(routes)
}

/// The full pass of `connector.reroute_all`: every glued connector on
/// the page (ADR 0045), not scoped to one edit and not an edit itself.
pub(crate) fn reroute_all_on(ctx: &mut AppCtx, page: PageId) -> Result<usize, CommandError> {
    let (count, _) = reroute_scoped(ctx, page, Scope::All)?;
    Ok(count)
}

/// The shared hook body: one pass on the active document, one warning
/// line per dropped glue, never a failed edit (the
/// [`reroute_after_edit`] contract).
fn reroute_scoped(
    ctx: &mut AppCtx,
    page: PageId,
    scope: Scope<'_>,
) -> Result<(usize, Option<Box<dyn Edit>>), CommandError> {
    // `PageId` is a position, so only the active document can own it: the
    // editing commands and undo/redo all act on the active document.
    let Some(slot) = ctx.active_document_mut() else {
        return Ok((0, None));
    };
    if slot.document.page(page).is_none() {
        return Ok((0, None));
    }
    let session = slot.id;
    let mut warnings = Vec::new();
    let (count, entries) = reroute_document(&mut slot.document, page, scope, &mut warnings)?;
    for line in warnings {
        ctx.record_warning(line);
    }
    ctx.touch_session(session);
    // The pass writes values only (the rows and the cached endpoint
    // points), so the snapshot dirties nothing: recalc owns no cell it
    // must recompute, and a recompute would replace cached values the
    // file kept (task P7-49 goal 1).
    let edit = (count > 0).then(|| Box::new(RouteEdit { session, entries }) as Box<dyn Edit>);
    Ok((count, edit))
}

/// One re-route pass on `page`, with one warning line when the pass
/// cannot run (task P5-19 coverage: the one helper and the one spelling
/// of the "connectors were not re-routed" message). The returned edit
/// is the caller's to fold into its step.
pub(crate) fn reroute_quietly(ctx: &mut AppCtx, changed: &[ShapeId]) -> Option<Box<dyn Edit>> {
    let page = ctx
        .active_document()
        .and_then(|session| session.active_page);
    page.and_then(|page| reroute_quietly_on(ctx, page, changed))
}

/// One re-route pass on `page`, with one warning line when the pass
/// cannot run (task P5-19 coverage: the one helper and the one spelling
/// of the "connectors were not re-routed" message). The returned edit
/// is the caller's to fold into its step.
pub(crate) fn reroute_quietly_on(
    ctx: &mut AppCtx,
    page: PageId,
    changed: &[ShapeId],
) -> Option<Box<dyn Edit>> {
    match reroute_after_edit(ctx, page, changed) {
        Ok(routes) => routes,
        Err(error) => {
            ctx.record_warning(format!("connectors were not re-routed: {error}"));
            None
        }
    }
}

/// The re-route pass over one document (ADR 0084 rule 5).
///
/// The candidates are the connectors the page's records name. The pass
/// recomputes only the ones the scope picks: on different sides of a
/// move (rule 5), or (for [`Scope::Forced`]) named outright. It
/// returns the sheets it rewrote, around the write, so the caller's
/// undo step can put them back.
fn reroute_document(
    doc: &mut Document,
    page: PageId,
    scope: Scope<'_>,
    warnings: &mut Vec<String>,
) -> Result<(usize, Vec<RouteRows>), CommandError> {
    let connector_ids = connector_visio_ids(doc, page)?;
    let changed_visio = match scope {
        Scope::All | Scope::Forced(_) => Vec::new(),
        Scope::Changed(changed) => subtree_visio_ids(doc, changed),
    };
    let mut routed = 0usize;
    let mut entries: Vec<RouteRows> = Vec::new();
    for visio_id in connector_ids {
        let Some(connector) = doc.shape_by_visio_id(page, visio_id) else {
            drop_all_for(doc, page, visio_id);
            warnings.push(format!(
                "connector {visio_id} is gone; its glue records were dropped"
            ));
            continue;
        };
        drop_detached_targets(doc, page, visio_id, warnings);
        let ends = doc.connects_of(page, connector).map_err(model_error)?;
        if ends.first().is_none_or(Option::is_none) && ends.get(1).is_none_or(Option::is_none) {
            continue;
        }
        let visio_id = doc
            .shape(connector)
            .map(|entity| entity.visio_id)
            .unwrap_or_default();
        if !picks(scope, &changed_visio, &ends, connector, visio_id) {
            continue;
        }
        // The `ConFixedCode` check (ADR 0084 rule 7, task P7-35 item
        // 5): Never Reroute keeps the stored route through the scoped
        // pass; only its ends follow their glue and its end segments
        // stretch to reach them. Any other value routes fresh (rule 5).
        if matches!(scope, Scope::Changed(_))
            && super::connector_segments::never_reroutes(doc, connector)
            && let Some(kept) =
                super::connector_segments::kept_route(doc, page, connector, &ends, warnings)
        {
            let before = PathRows::capture(doc, connector);
            kept.apply(doc, connector).map_err(model_error)?;
            let after = PathRows::capture(doc, connector);
            entries.push(RouteRows {
                shape: connector,
                before,
                after,
            });
            routed = routed.saturating_add(1);
            continue;
        }
        routed = routed.saturating_add(route_one(
            doc,
            page,
            connector,
            &ends,
            warnings,
            &mut entries,
        )?);
    }
    // A forced scope also re-routes its connectors when no record
    // names them (free ends and a style change). A moved connector is
    // never one of them: its edit translated its path (task P7-49
    // goal 2, rule 1).
    if let Scope::Forced(changed) = scope {
        for &connector in changed {
            if !doc.is_connector(connector) {
                continue;
            }
            let ends = doc.connects_of(page, connector).map_err(model_error)?;
            let glued = ends.first().is_some_and(Option::is_some)
                || ends.get(1).is_some_and(Option::is_some);
            if glued {
                continue; // the record loop above already picked it
            }
            routed = routed.saturating_add(route_one(
                doc,
                page,
                connector,
                &ends,
                warnings,
                &mut entries,
            )?);
        }
    }
    Ok((routed, entries))
}

/// Whether the scope picks one connector (ADR 0084 rule 5, amended
/// 2026-11-04 for task P7-50 goal 2): under [`Scope::Changed`] a
/// connector re-routes only when a glued end's target and the
/// connector are on different sides of the move — the target is in the
/// moved set (or its subtree) and the connector is not, or the other
/// way round. A rigid group move carries a connector and both its
/// targets together, so a stored route survives it byte for byte; a
/// connector whose own ends, glue or style changed is named outright
/// by [`Scope::Forced`].
fn picks(
    scope: Scope<'_>,
    changed_visio: &[i32],
    ends: &[Option<Connect>; 2],
    connector: ShapeId,
    connector_visio: i32,
) -> bool {
    match scope {
        Scope::All => true,
        Scope::Forced(connectors) => connectors.contains(&connector),
        Scope::Changed(_) => {
            let moved = changed_visio.contains(&connector_visio);
            ends.iter()
                .flatten()
                .any(|connect| changed_visio.contains(&connect.to_sheet) != moved)
        }
    }
}

/// The routes one connector fresh and writes its path, recording its
/// sheet around the write; `1` when it rewrote the rows, `0` when it
/// left them alone (a formula-driven geometry belongs to recalc, ADR
/// 0056).
fn route_one(
    doc: &mut Document,
    page: PageId,
    connector: ShapeId,
    ends: &[Option<Connect>; 2],
    warnings: &mut Vec<String>,
    entries: &mut Vec<RouteRows>,
) -> Result<usize, CommandError> {
    // ADR 0056: a connector whose geometry rows carry formulas is
    // owned by recalc (a master-driven dynamic connector); the
    // router never writes its rows.
    if crate::recalc::geometry_is_formula_driven(doc, connector) {
        return Ok(0);
    }
    let visio_id = doc
        .shape(connector)
        .map(|entity| entity.visio_id)
        .unwrap_or_default();
    if page_transform(doc, connector).is_err() {
        warnings.push(format!("connector {visio_id} has no usable transform"));
        return Ok(0);
    }
    let (begin, end, boxes) = resolved_ends(
        doc,
        page,
        connector,
        [
            ends.first().and_then(Option::as_ref),
            ends.get(1).and_then(Option::as_ref),
        ],
        [None, None],
        warnings,
    )?;
    for (candidate, connect) in [
        (begin, ends.first().and_then(Option::as_ref)),
        (end, ends.get(1).and_then(Option::as_ref)),
    ] {
        let finite = candidate.point.x.is_finite() && candidate.point.y.is_finite();
        if connect.is_some() && !finite {
            warnings.push(format!(
                "connector {visio_id} has no resolvable glue; its path is unchanged"
            ));
            return Ok(0);
        }
    }
    let style = route_style_for(doc, connector);
    let options = RouteOptions::new(style)
        .with_exits(begin.exit, end.exit)
        .with_boxes(
            boxes.first().copied().flatten(),
            boxes.get(1).copied().flatten(),
        );
    let points = route(begin.point, end.point, &options).map_err(|error| {
        CommandError::failed(format!("cannot route connector {visio_id}: {error}"))
    })?;
    let before = PathRows::capture(doc, connector);
    if style == RouteStyle::Curved {
        write_curve(doc, connector, &points)?;
    } else {
        write_path(doc, connector, &points)?;
    }
    let after = PathRows::capture(doc, connector);
    entries.push(RouteRows {
        shape: connector,
        before,
        after,
    });
    Ok(1)
}

/// Whether one `Connect` record names a **connector end** (`VisFromParts`
/// begin/end parts, ADR 0045), as opposed to a shape-to-shape glue such
/// as the glue-on-drop of ADR 0046 (`Connections.X*` from a connection
/// point): only connector ends are routed (task P5-11).
pub(super) fn is_connector_end(connect: &Connect) -> bool {
    matches!(
        connect.from_part,
        mutate::VIS_BEGIN
            | mutate::VIS_BEGIN_X
            | mutate::VIS_BEGIN_Y
            | mutate::VIS_END
            | mutate::VIS_END_X
            | mutate::VIS_END_Y
    ) || connect.from_cell.starts_with("Begin")
        || connect.from_cell.starts_with("End")
}

/// The distinct connector Visio ids the pass routes: the connector
/// ends the page's `Connect` records name, in file order (connector
/// ends only, [`is_connector_end`]). A connector with no record has no
/// glued end and nothing to re-route.
fn connector_visio_ids(doc: &Document, page: PageId) -> Result<Vec<i32>, CommandError> {
    let entity = doc
        .page(page)
        .ok_or_else(|| CommandError::failed(format!("unknown page id {}", page.index)))?;
    let mut ids = Vec::new();
    for connect in &entity.connects {
        if is_connector_end(connect) && !ids.contains(&connect.from_sheet) {
            ids.push(connect.from_sheet);
        }
    }
    Ok(ids)
}

/// Drops every record naming a connector that is no longer on the page.
fn drop_all_for(doc: &mut Document, page: PageId, connector_visio_id: i32) {
    if let Some(entity) = doc.pages.get_mut(page.index as usize) {
        entity
            .connects
            .retain(|connect| connect.from_sheet != connector_visio_id);
    }
}

/// Drops the records whose target shape is no longer on the page, with
/// one warning line per dropped glue. The ends glued to a detached
/// target keep their last point as a literal (ADR 0084 rule 2): the
/// endpoint formulas naming the gone shape go with the records.
fn drop_detached_targets(
    doc: &mut Document,
    page: PageId,
    connector_visio_id: i32,
    warnings: &mut Vec<String>,
) {
    let Some(page_entity) = doc.page(page) else {
        return;
    };
    let missing: Vec<i32> = page_entity
        .connects
        .iter()
        .filter(|connect| connect.from_sheet == connector_visio_id)
        .map(|connect| connect.to_sheet)
        .filter(|target| doc.shape_by_visio_id(page, *target).is_none())
        .collect();
    if missing.is_empty() {
        return;
    }
    let mut lost_ends: Vec<(End, Point)> = Vec::new();
    for connect in &page_entity.connects {
        if connect.from_sheet != connector_visio_id || !missing.contains(&connect.to_sheet) {
            continue;
        }
        let end = End::of(connect);
        let at = doc
            .shape_by_visio_id(page, connector_visio_id)
            .map(|shape| stored_end_point(doc, shape, end))
            .unwrap_or(Point::new(0.0, 0.0));
        lost_ends.push((end, at));
    }
    let clear = doc
        .shape_by_visio_id(page, connector_visio_id)
        .filter(|shape| doc.is_connector(*shape));
    if let Some(shape) = clear {
        for (end, at) in &lost_ends {
            if let Err(error) =
                doc.write_connector_end(shape, end.connector_end(), *at, EndGlue::Free)
            {
                warnings.push(format!(
                    "connector {connector_visio_id} kept a stale glue formula: {error}"
                ));
            }
        }
    }
    if let Some(entity) = doc.pages.get_mut(page.index as usize) {
        entity.connects.retain(|connect| {
            let lost =
                connect.from_sheet == connector_visio_id && missing.contains(&connect.to_sheet);
            if lost {
                warnings.push(format!(
                    "connector {connector_visio_id} lost its glue to detached shape {}",
                    connect.to_sheet
                ));
            }
            !lost
        });
    }
}

/// The facts one glue target offers the resolver (ADR 0084 rule 3):
/// its connection rows in file order, its local box and its page
/// transform.
fn glue_target(doc: &Document, shape: ShapeId) -> Result<GlueTarget, CommandError> {
    let mut reader = CellReader::new(doc);
    let sheet = SheetRef::Shape(shape);
    let number = |reader: &mut CellReader<'_>, name: &str| {
        reader
            .number(sheet, &CellName::new(name))
            .map_err(model_error)
    };
    let width = box_size(number(&mut reader, "Width")?);
    let height = box_size(number(&mut reader, "Height")?);
    let points = doc
        .connection_points_indexed(shape)
        .into_iter()
        .map(|(ix, local)| {
            let direction = connection_direction(doc, shape, ix);
            (u32::try_from(ix).unwrap_or(u32::MAX), local, direction)
        })
        .collect();
    Ok(GlueTarget {
        points,
        local_box: Rect::new(Point::new(0.0, 0.0), Point::new(width, height)),
        transform: page_transform(doc, shape)?,
    })
}

/// A local box side length: a negative or non-finite size counts as
/// zero (the degenerate box never panics the resolver).
fn box_size(value: f64) -> f64 {
    if value.is_finite() && value > 0.0 {
        value
    } else {
        0.0
    }
}

/// The connection row's local `DirX`/`DirY` direction (ADR 0084 rule
/// 4), or `None` when the row has none (both cells zero, missing or
/// non-finite).
///
/// The row is found where [`Document::connection_points_indexed`] finds
/// its point: the shape's own `Connection` rows when it has any, else
/// the nearest master with rows. A master's directions scale to the
/// instance like its points do.
fn connection_direction(doc: &Document, shape: ShapeId, ix: i32) -> Option<Point> {
    let entity = doc.shape(shape)?;
    if !connection_rows(&entity.sheet.sections).is_empty() {
        return row_direction(&entity.sheet.sections, ix, 1.0, 1.0);
    }
    for master in doc.master_chain_shapes(shape, 128) {
        let found = doc.shape(master)?;
        if connection_rows(&found.sheet.sections).is_empty() {
            continue;
        }
        let (scale_x, scale_y) = master_scales(entity, found);
        return row_direction(&found.sheet.sections, ix, scale_x, scale_y);
    }
    None
}

/// The `Connection` rows of a sheet, across every section of that name
/// (the model's [`Document::connection_points_indexed`] rule).
fn connection_rows(sections: &[Section]) -> Vec<&Row> {
    sections
        .iter()
        .filter(|section| section.name.as_str() == "Connection")
        .flat_map(|section| section.rows.iter())
        .collect()
}

/// The direction of the row whose `IX` is `ix`, scaled per axis like
/// [`Document::connection_points_indexed`] scales its point. Only
/// `DirX`/`DirY` are directions: an extended row's `A` and `B` are not
/// (task P7-49 goal 9).
fn row_direction(sections: &[Section], ix: i32, scale_x: f64, scale_y: f64) -> Option<Point> {
    for (position, row) in connection_rows(sections).into_iter().enumerate() {
        let row_ix = row
            .ix
            .filter(|ix| *ix >= 0)
            .unwrap_or_else(|| i32::try_from(position).unwrap_or(i32::MAX));
        if row_ix != ix {
            continue;
        }
        let x = row_number(row, "DirX").unwrap_or(0.0);
        let y = row_number(row, "DirY").unwrap_or(0.0);
        let point = Point::new(x * scale_x, y * scale_y);
        if point.x.is_finite() && point.y.is_finite() && (point.x != 0.0 || point.y != 0.0) {
            return Some(point);
        }
        return None;
    }
    None
}

/// The cached number of one row cell, or `None`.
fn row_number(row: &Row, name: &str) -> Option<f64> {
    row.cells
        .iter()
        .find(|cell| cell.name.as_str() == name)
        .and_then(|cell| match &cell.value {
            Some(CellValue::Number(value)) => Some(*value),
            _ => None,
        })
}

/// The per-axis scale from a master shape's local box to the
/// instance's box (the model's connection-point rule). A zero or
/// non-finite size leaves that axis unscaled.
fn master_scales(instance: &Shape, master: &Shape) -> (f64, f64) {
    (
        axis_scale(
            direct_number(instance, "Width"),
            direct_number(master, "Width"),
        ),
        axis_scale(
            direct_number(instance, "Height"),
            direct_number(master, "Height"),
        ),
    )
}

/// The direct cached number of one shape cell, or `None`.
fn direct_number(shape: &Shape, name: &str) -> Option<f64> {
    shape
        .sheet
        .cells
        .iter()
        .find(|cell| cell.name.as_str() == name)
        .and_then(|cell| match &cell.value {
            Some(CellValue::Number(value)) => Some(*value),
            _ => None,
        })
}

/// One axis' scale factor: the instance size over the master size, or
/// `1.0` when either size is missing, non-finite or the master size is
/// zero.
fn axis_scale(instance: Option<f64>, master: Option<f64>) -> f64 {
    match (instance, master) {
        (Some(instance), Some(master))
            if instance.is_finite() && master.is_finite() && master != 0.0 =>
        {
            instance / master
        }
        _ => 1.0,
    }
}

/// The `Connection` row index of a record (`VisToParts` = 100 + IX, or
/// the `Connections.Xn` cell with a one-based `n`).
pub(super) fn connection_index(connect: &Connect) -> Option<usize> {
    if connect.to_part >= mutate::VIS_CONNECTION_POINT {
        return usize::try_from(connect.to_part - mutate::VIS_CONNECTION_POINT).ok();
    }
    let cells = connect.to_cell.strip_prefix("Connections.X")?;
    let one_based = cells.parse::<usize>().ok()?;
    one_based.checked_sub(1)
}

/// The page transform of a shape: its own local box transform composed
/// with its ancestors' (the render builder's rule).
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
    // Innermost first: each local transform runs before its parent's
    // (task P7-49 goal 5: a translating parent hid the old reversed
    // order).
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
        .then(Transform::rotate(larrygram_geom::Angle::from_radians(
            angle,
        )))
        .then(Transform::translate(pin)))
}

/// The `LocPin` of a shape, or the origin when it cannot be read.
fn loc_pin(doc: &Document, shape: ShapeId) -> Point {
    let mut reader = CellReader::new(doc);
    let sheet = SheetRef::Shape(shape);
    let read = |reader: &mut CellReader<'_>, name: &str| {
        reader.number(sheet, &CellName::new(name)).unwrap_or(0.0)
    };
    Point::new(read(&mut reader, "LocPinX"), read(&mut reader, "LocPinY"))
}

/// Maps a local point, keeping only finite results callers can use.
fn transform_point(transform: Transform, point: Point) -> Point {
    transform.apply(point)
}

/// The connector's current path endpoints in page inches, when it has a
/// readable absolute `MoveTo` and a final `LineTo`/`MoveTo`.
fn geometry_endpoints(
    doc: &Document,
    connector: ShapeId,
    transform: Transform,
) -> Option<(Point, Point)> {
    let geometry = doc.shape_geometry(connector).ok()?;
    let path = geometry.paths.first()?;
    let first = path.rows.first().and_then(row_point)?;
    let last = path.rows.last().and_then(row_point)?;
    Some((transform.apply(first), transform.apply(last)))
}

/// One typed geometry row's absolute point, or `None` for a row without
/// one (relative and curve-only rows).
fn row_point(row: &larrygram_model::GeometryRow) -> Option<Point> {
    use larrygram_model::GeometryRow as G;
    match row {
        G::MoveTo { x, y }
        | G::LineTo { x, y }
        | G::ArcTo { x, y, .. }
        | G::EllipticalArcTo { x, y, .. }
        | G::NurbsTo { x, y, .. }
        | G::PolylineTo { x, y, .. }
        | G::SplineStart { x, y, .. }
        | G::SplineKnot { x, y, .. } => Some(Point::new(*x, *y)),
        G::Ellipse { center, .. } => Some(*center),
        G::InfiniteLine { p1, .. } => Some(*p1),
        _ => None,
    }
}

/// The connector's **page-space** path mapped into its parent's space
/// (task P5-11: `geometry_endpoints` reads the rows back through the
/// same chain; the model's writes map on into the shape's own local
/// box).
fn mapped_path(
    doc: &Document,
    connector: ShapeId,
    points: &[Point],
) -> Result<Vec<Point>, CommandError> {
    let parent = match doc
        .shape(connector)
        .ok_or_else(|| CommandError::failed("the connector is gone"))?
        .parent
    {
        Some(parent) => page_transform(doc, parent)?,
        None => Transform::identity(),
    };
    let inverse = parent.invert().map_err(|_| singular())?;
    Ok(points.iter().map(|point| inverse.apply(*point)).collect())
}

/// Writes the connector's **page-space** path as its literal geometry
/// rows (ADR 0045).
///
/// `Document::write_connector_path` maps page points into the shape's
/// own local box; a connector inside a group is addressed through its
/// parents, so the points are mapped into the parent's space first
/// (task P5-11: `geometry_endpoints` reads them back through the same
/// chain).
fn write_path(
    doc: &mut Document,
    connector: ShapeId,
    points: &[Point],
) -> Result<(), CommandError> {
    let local = mapped_path(doc, connector, points)?;
    doc.write_connector_path(connector, &local)
        .map_err(model_error)
}

/// Writes the connector's **page-space** curved path (task P7-36 item
/// 2): [`Document::write_connector_curve`] bends the route's points
/// into one `NURBSTo` row, in the same parent mapping as
/// [`write_path`].
fn write_curve(
    doc: &mut Document,
    connector: ShapeId,
    points: &[Point],
) -> Result<(), CommandError> {
    let local = mapped_path(doc, connector, points)?;
    doc.write_connector_curve(connector, &local)
        .map_err(model_error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{DocumentSession, Settings};
    use larrygram_model::{Cell, Preserved, Row, Section, SectionName, Shape};

    fn cell(name: &str, value: f64) -> Cell {
        Cell {
            name: CellName::new(name),
            formula: Some(format!("{value}")),
            value: Some(CellValue::Number(value)),
            unit: larrygram_geom::Unit::Inches,
            preserved: Preserved::new(),
        }
    }

    fn shape_at(visio_id: i32, x: f64, y: f64, width: f64, height: f64) -> Shape {
        let mut shape = Shape::new(ShapeId { index: 0 }, visio_id);
        shape.sheet.cells = vec![
            cell("PinX", x),
            cell("PinY", y),
            cell("Width", width),
            cell("Height", height),
            cell("LocPinX", width * 0.5),
            cell("LocPinY", height * 0.5),
            cell("Angle", 0.0),
            Cell {
                name: CellName::new("FlipX"),
                formula: Some("0".to_owned()),
                value: Some(CellValue::Bool(false)),
                unit: larrygram_geom::Unit::Unitsless,
                preserved: Preserved::new(),
            },
            Cell {
                name: CellName::new("FlipY"),
                formula: Some("0".to_owned()),
                value: Some(CellValue::Bool(false)),
                unit: larrygram_geom::Unit::Unitsless,
                preserved: Preserved::new(),
            },
        ];
        shape
    }

    fn with_connection_points(mut shape: Shape, points: &[(f64, f64)]) -> Shape {
        let mut section = Section::new(SectionName::from_static("Connection"));
        for (ix, (x, y)) in points.iter().enumerate() {
            let mut row = Row::new();
            row.row_type = Some("Connection".to_owned());
            row.ix = Some(i32::try_from(ix).unwrap_or(0));
            row.cells = vec![cell("X", *x), cell("Y", *y)];
            section.rows.push(row);
        }
        shape.sheet.sections.push(section);
        shape
    }

    fn insert(doc: &mut Document, page: PageId, shape: Shape) -> ShapeId {
        doc.insert_shape(page, shape, None).expect("insert")
    }

    /// A page with a connector and two 2 x 1 targets whose connection
    /// point 0 is their top-left corner.
    fn fixture() -> (AppCtx, SessionId, PageId, ShapeId, ShapeId, ShapeId) {
        let mut doc = Document::new();
        let page = doc.add_page("Page-1").expect("page");
        let left = insert(
            &mut doc,
            page,
            with_connection_points(shape_at(1, 1.0, 2.0, 2.0, 1.0), &[(0.0, 0.0)]),
        );
        let right = insert(
            &mut doc,
            page,
            with_connection_points(shape_at(2, 5.0, 2.0, 2.0, 1.0), &[(2.0, 1.0)]),
        );
        let connector = insert(&mut doc, page, shape_at(3, 3.0, 2.0, 2.0, 1.0));
        let mut ctx = AppCtx::new(Settings::default());
        let index = ctx.open_document(DocumentSession::new(doc, None));
        let session = ctx.documents.get(index).expect("session").id;
        (ctx, session, page, connector, left, right)
    }

    fn args(pairs: &[(&str, i64)]) -> Args {
        let mut args = Args::new();
        for (name, value) in pairs {
            args.set(*name, ArgValue::Integer(*value));
        }
        args
    }

    fn dispatch(ctx: &mut AppCtx, id: &str, args: Args) -> Result<Outcome, CommandError> {
        let registry = crate::builtins::registry().expect("registry");
        let ran_against = ctx.active_document().map(|session| session.id);
        let mut outcome = registry.execute(
            &crate::CommandId::intern(id).expect("id"),
            ctx,
            Invocation::new(args, crate::Source::Test),
        )?;
        if let Some(edit) = outcome.edit.take()
            && let Some(session) = ran_against.or_else(|| ctx.active_document().map(|s| s.id))
        {
            ctx.push_edit(session, edit)?;
        }
        Ok(outcome)
    }

    fn path_points(ctx: &AppCtx, connector: ShapeId) -> Vec<Point> {
        let doc = &ctx.active_document().expect("session").document;
        let transform = page_transform(doc, connector).expect("transform");
        let geometry = doc.shape_geometry(connector).expect("geometry");
        geometry
            .paths
            .first()
            .map(|path| {
                path.rows
                    .iter()
                    .filter_map(row_point)
                    .map(|point| transform.apply(point))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Criterion 6 (task P6-07): `PathEdit` writes geometry row cells,
    /// so it reports them.
    #[test]
    fn path_edits_dirty_their_row_cells() {
        let (ctx, session, _page, connector, _l, _r) = fixture();
        let edit = {
            let doc = ctx.active_document().expect("session").document.clone();
            PathEdit::new(
                &doc,
                session,
                connector,
                vec![Point::new(0.0, 0.0), Point::new(1.0, 1.0)],
            )
            .expect("edit")
        };
        let dirty = edit.dirty_cells();
        assert_eq!(dirty.len(), 4, "two points of X and Y: {dirty:?}");
        assert!(dirty.iter().all(|address| address.row.is_some()));
        assert!(
            dirty
                .iter()
                .all(|address| address.sheet == SheetRef::Shape(connector))
        );
    }

    #[test]
    fn glue_records_name_the_end_and_the_connection_row() {
        let (ctx, _session, page, connector, left, right) = fixture();
        let doc = &ctx.active_document().expect("session").document;
        let begin = glue_record(
            connector,
            left,
            doc.shape(left).expect("left").visio_id,
            Some(0),
            End::Begin,
            doc,
        )
        .expect("begin");
        assert_eq!(begin.from_cell, "BeginX");
        assert_eq!(begin.from_part, mutate::VIS_BEGIN);
        assert_eq!(begin.to_cell, "Connections.X1");
        assert_eq!(begin.to_part, mutate::VIS_CONNECTION_POINT);
        let outline = glue_record(
            connector,
            right,
            doc.shape(right).expect("right").visio_id,
            None,
            End::End,
            doc,
        )
        .expect("outline");
        assert_eq!(outline.from_cell, "EndX");
        assert_eq!(outline.to_cell, "PinX");
        assert_eq!(outline.to_part, mutate::VIS_WHOLE_SHAPE);
        assert_eq!(
            doc.pages.get(page.index as usize).map(|p| p.connects.len()),
            Some(0)
        );
        assert!(glue_record(connector, left, 1, Some(9), End::Begin, doc).is_err());
        assert!(glue_record(connector, left, 1, Some(-1), End::Begin, doc).is_err());
    }

    #[test]
    fn endpoints_parse_and_classify() {
        assert_eq!(End::parse("begin"), Some(End::Begin));
        assert_eq!(End::parse("end"), Some(End::End));
        assert_eq!(End::parse("middle"), None);
        let mut connect = Connect::new();
        connect.from_part = mutate::VIS_END;
        assert_eq!(End::of(&connect), End::End);
        connect.from_part = mutate::VIS_BEGIN;
        connect.from_cell = "BeginX".to_owned();
        assert_eq!(End::of(&connect), End::Begin);
        connect.from_cell = "EndY".to_owned();
        assert_eq!(End::of(&connect), End::End);
    }

    #[test]
    fn the_style_spellings_round_trip_the_table() {
        assert_eq!(route_style_value(RouteStyle::RightAngle), "right_angle");
        assert_eq!(route_style_value(RouteStyle::Straight), "straight");
        assert_eq!(route_style_value(RouteStyle::Curved), "curved");
        for style in [
            RouteStyle::RightAngle,
            RouteStyle::Straight,
            RouteStyle::Curved,
        ] {
            assert_eq!(style_from_value(route_style_value(style)), Some(style));
        }
        assert_eq!(style_from_value("right-angle"), None);
        assert_eq!(style_from_value(""), None);
        // The cells a choice writes (ADR 0084 rule 8's table).
        assert_eq!(
            style_cells::style_values(RouteStyle::RightAngle),
            (1.0, 1.0)
        );
        assert_eq!(style_cells::style_values(RouteStyle::Straight), (2.0, 1.0));
        assert_eq!(style_cells::style_values(RouteStyle::Curved), (1.0, 2.0));
    }

    #[test]
    fn the_style_falls_through_to_the_page_and_to_right_angle() {
        let (mut ctx, session, page, connector, _left, _right) = fixture();
        // No local style and no page value: right angle (ADR 0084 rule 8).
        let doc = ctx.active_document().expect("session").document.clone();
        assert_eq!(shape_style(&doc, connector), RouteStyle::RightAngle);
        // A missing or 0 cell value falls through to the page's cells.
        let doc = document_mut(&mut ctx, session).expect("session");
        doc.write_cell(
            SheetRef::Page(page),
            &CellName::new("RouteStyle"),
            &CellWrite::new("2", CellValue::Number(2.0)),
        )
        .expect("page style");
        let doc = ctx.active_document().expect("session").document.clone();
        assert_eq!(shape_style(&doc, connector), RouteStyle::Straight);
        // The curved pair on the page reaches a connector with no cells
        // of its own (task P7-36 item 1).
        let doc = document_mut(&mut ctx, session).expect("session");
        doc.write_cell(
            SheetRef::Page(page),
            &CellName::new("RouteStyle"),
            &CellWrite::new("1", CellValue::Number(1.0)),
        )
        .expect("page style");
        doc.write_cell(
            SheetRef::Page(page),
            &CellName::new("LineRouteExt"),
            &CellWrite::new("2", CellValue::Number(2.0)),
        )
        .expect("page ext");
        let doc = ctx.active_document().expect("session").document.clone();
        assert_eq!(shape_style(&doc, connector), RouteStyle::Curved);
        // Local values 16 and 6 (center-to-centre and multi-connector)
        // route as right angle over a curved page (ADR 0084 rule 8).
        for value in [16.0, 6.0, 23.0] {
            let doc = document_mut(&mut ctx, session).expect("session");
            doc.write_cell(
                SheetRef::Shape(connector),
                &CellName::new("ShapeRouteStyle"),
                &CellWrite::new(mutate::literal(value), CellValue::Number(value)),
            )
            .expect("local style");
            let doc = ctx.active_document().expect("session").document.clone();
            assert_eq!(
                shape_style(&doc, connector),
                RouteStyle::RightAngle,
                "{value}"
            );
        }
    }

    #[test]
    fn a_glue_and_unglue_round_trip_the_connects() {
        let (mut ctx, _session, page, connector, left, right) = fixture();
        let before = ctx.active_document().expect("session").document.clone();
        dispatch(
            &mut ctx,
            "connector.glue_begin",
            args(&[("connector", 3), ("target", 1), ("point", 0)]),
        )
        .expect("glue begin");
        dispatch(
            &mut ctx,
            "connector.glue_end",
            args(&[("connector", 3), ("target", 2)]),
        )
        .expect("glue end");
        {
            let doc = &ctx.active_document().expect("session").document;
            let ends = doc.connects_of(page, connector).expect("ends");
            assert_eq!(ends[0].as_ref().map(|c| c.to_sheet), Some(1));
            assert_eq!(ends[1].as_ref().map(|c| c.to_sheet), Some(2));
            // The default is right angle (ADR 0084 rule 8): a routed
            // path with at least the two endpoints.
            assert!(path_points(&ctx, connector).len() >= 2);
            let _ = (left, right);
        }

        dispatch(
            &mut ctx,
            "connector.unglue",
            Args::new()
                .with("connector", ArgValue::Integer(3))
                .with("end", ArgValue::Text("begin".to_owned())),
        )
        .expect("unglue begin");
        {
            let doc = &ctx.active_document().expect("session").document;
            let ends = doc.connects_of(page, connector).expect("ends");
            assert!(ends[0].is_none());
            assert_eq!(ends[1].as_ref().map(|c| c.to_sheet), Some(2));
        }

        // Undo both unglue and glues (each through the registry).
        for _ in 0..3 {
            dispatch(&mut ctx, "edit.undo", Args::new()).expect("undo");
        }
        assert_eq!(ctx.active_document().expect("session").document, before);
    }

    #[test]
    fn the_connector_argument_defaults_to_the_selection_anchor() {
        let (mut ctx, _session, page, connector, _left, right) = fixture();
        ctx.selection.anchor = Some(connector);
        ctx.selection.shapes = vec![connector];
        ctx.selection.page = Some(page);
        dispatch(&mut ctx, "connector.glue_end", args(&[("target", 2)]))
            .expect("glue by selection");
        let doc = &ctx.active_document().expect("session").document;
        let ends = doc.connects_of(page, connector).expect("ends");
        assert_eq!(ends[1].as_ref().map(|c| c.to_sheet), Some(2));
        let _ = right;
    }

    #[test]
    fn a_new_glue_replaces_the_old_one() {
        let (mut ctx, _session, page, connector, left, right) = fixture();
        dispatch(
            &mut ctx,
            "connector.glue_begin",
            args(&[("connector", 3), ("target", 1), ("point", 0)]),
        )
        .expect("first");
        dispatch(
            &mut ctx,
            "connector.glue_begin",
            args(&[("connector", 3), ("target", 2), ("point", 0)]),
        )
        .expect("replace");
        let doc = &ctx.active_document().expect("session").document;
        let ends = doc.connects_of(page, connector).expect("ends");
        assert_eq!(ends[0].as_ref().map(|c| c.to_sheet), Some(2));
        assert_eq!(
            doc.page(page).map(|p| p.connects.len()),
            Some(1),
            "one record per endpoint"
        );
        let _ = (left, right);
    }

    #[test]
    fn moving_a_target_reroutes_and_undo_restores_the_path() {
        let (mut ctx, _session, page, connector, left, _right) = fixture();
        dispatch(
            &mut ctx,
            "connector.glue_begin",
            args(&[("connector", 3), ("target", 1), ("point", 0)]),
        )
        .expect("glue begin");
        dispatch(
            &mut ctx,
            "connector.glue_end",
            args(&[("connector", 3), ("target", 2)]),
        )
        .expect("glue end");
        let before = path_points(&ctx, connector);
        assert_eq!(before.first(), Some(&Point::new(0.0, 1.5)));
        assert_eq!(before.last(), Some(&Point::new(6.0, 2.5)));

        ctx.selection.shapes = vec![left];
        ctx.selection.anchor = Some(left);
        ctx.selection.page = Some(page);
        dispatch(
            &mut ctx,
            "shape.move",
            Args::new()
                .with("dx", ArgValue::Number(1.0))
                .with("dy", ArgValue::Number(1.0)),
        )
        .expect("move");
        let after = path_points(&ctx, connector);
        assert_eq!(after.first(), Some(&Point::new(1.0, 2.5)));
        assert_ne!(before, after);

        dispatch(&mut ctx, "edit.undo", Args::new()).expect("undo");
        assert_eq!(path_points(&ctx, connector), before);
        dispatch(&mut ctx, "edit.redo", Args::new()).expect("redo");
        assert_eq!(path_points(&ctx, connector), after);
    }

    #[test]
    fn a_right_angle_style_writes_an_elbow() {
        let (mut ctx, _session, page, connector, _left, _right) = fixture();
        ctx.selection.shapes = vec![connector];
        ctx.selection.anchor = Some(connector);
        ctx.selection.page = Some(page);
        dispatch(
            &mut ctx,
            "connector.glue_begin",
            args(&[("connector", 3), ("target", 1), ("point", 0)]),
        )
        .expect("glue begin");
        dispatch(
            &mut ctx,
            "connector.glue_end",
            args(&[("connector", 3), ("target", 2)]),
        )
        .expect("glue end");
        dispatch(
            &mut ctx,
            "connector.style",
            Args::new().with("style", ArgValue::Text("right_angle".to_owned())),
        )
        .expect("style");
        let points = path_points(&ctx, connector);
        assert!(points.len() > 2, "{points:?}");
        assert_eq!(points.first(), Some(&Point::new(0.0, 1.5)));
        assert_eq!(points.last(), Some(&Point::new(6.0, 2.5)));
        dispatch(&mut ctx, "edit.undo", Args::new()).expect("undo style");
        // The undo restores no local style; the default routes right
        // angle too (ADR 0084 rule 8), so the same route comes back.
        assert_eq!(path_points(&ctx, connector), points);
    }

    #[test]
    fn deleting_a_target_drops_its_glue_with_one_warning() {
        let (mut ctx, _session, page, connector, left, _right) = fixture();
        dispatch(
            &mut ctx,
            "connector.glue_begin",
            args(&[("connector", 3), ("target", 1), ("point", 0)]),
        )
        .expect("glue begin");
        ctx.selection.shapes = vec![left];
        ctx.selection.anchor = Some(left);
        ctx.selection.page = Some(page);
        let outcome = dispatch(&mut ctx, "edit.delete", Args::new()).expect("delete");
        let doc = &ctx.active_document().expect("session").document;
        let ends = doc.connects_of(page, connector).expect("ends");
        assert!(ends[0].is_none(), "the detached glue is dropped");
        let warnings = &outcome.warnings;
        assert_eq!(
            warnings
                .iter()
                .filter(|line| line.contains("lost its glue to detached shape 1"))
                .count(),
            1,
            "{warnings:?}"
        );
    }

    #[test]
    fn reroute_all_is_idempotent_and_not_an_edit() {
        let (mut ctx, _session, _page, connector, _left, _right) = fixture();
        dispatch(
            &mut ctx,
            "connector.glue_begin",
            args(&[("connector", 3), ("target", 1), ("point", 0)]),
        )
        .expect("glue begin");
        dispatch(
            &mut ctx,
            "connector.glue_end",
            args(&[("connector", 3), ("target", 2)]),
        )
        .expect("glue end");
        let first = path_points(&ctx, connector);
        let outcome = dispatch(&mut ctx, "connector.reroute_all", Args::new()).expect("reroute");
        assert!(outcome.edit.is_none());
        assert_eq!(path_points(&ctx, connector), first);
        let again = path_points(&ctx, connector);
        assert_eq!(first, again);
    }

    #[test]
    fn reroute_acts_on_the_active_document_only() {
        let (mut ctx, session, page, connector, _left, _right) = fixture();
        dispatch(
            &mut ctx,
            "connector.glue_end",
            args(&[("connector", 3), ("target", 2)]),
        )
        .expect("glue end");
        let first = ctx
            .documents
            .iter()
            .position(|slot| slot.id == session)
            .expect("first");
        let mut other = Document::new();
        let _ = other.add_page("Page-1").expect("page");
        let second = ctx.open_document(DocumentSession::new(other, None));
        ctx.active = Some(second);
        let before = ctx.documents.get(first).expect("first").changes;

        // The second document also has a page 0, but it is the active one:
        // the pass must not touch the first document.
        assert!(
            reroute_connectors(&mut ctx, page, &[connector])
                .expect("reroute")
                .is_none(),
            "the pass rewrites nothing on the other document"
        );
        assert_eq!(ctx.documents.get(first).expect("first").changes, before);

        ctx.active = Some(first);
        assert!(
            reroute_connectors(&mut ctx, page, &[connector])
                .expect("reroute")
                .is_some(),
            "the pass rewrites the connector on its own document"
        );
    }

    #[test]
    fn bad_arguments_are_named() {
        let (mut ctx, _session, _page, _connector, _left, _right) = fixture();
        let missing =
            dispatch(&mut ctx, "connector.glue_begin", Args::new()).expect_err("missing connector");
        assert!(
            missing.to_string().contains("no connector selected"),
            "{missing}"
        );

        let unknown = dispatch(
            &mut ctx,
            "connector.glue_begin",
            args(&[("connector", 99), ("target", 1)]),
        )
        .expect_err("unknown connector");
        assert!(
            unknown
                .to_string()
                .contains("unknown connector shape id 99"),
            "{unknown}"
        );

        let bad_end = dispatch(
            &mut ctx,
            "connector.unglue",
            Args::new()
                .with("connector", ArgValue::Integer(3))
                .with("end", ArgValue::Text("middle".to_owned())),
        )
        .expect_err("bad end");
        assert!(bad_end.to_string().contains("begin or end"), "{bad_end}");

        let bad_style = dispatch(
            &mut ctx,
            "connector.style",
            Args::new().with("style", ArgValue::Text("diagonal".to_owned())),
        )
        .expect_err("bad style");
        assert!(
            bad_style
                .to_string()
                .contains("style must be right_angle, straight or curved"),
            "{bad_style}"
        );

        let no_style = dispatch(&mut ctx, "connector.style", Args::new()).expect_err("no style");
        assert!(
            no_style.to_string().contains("connector style"),
            "{no_style}"
        );
    }

    #[test]
    fn integer_arguments_accept_the_three_shapes() {
        let mut args = Args::new().with("a", ArgValue::Integer(-2));
        assert_eq!(need_integer(&args, "a", "a"), Ok(-2));
        args.set("a", ArgValue::Number(3.0));
        assert_eq!(need_integer(&args, "a", "a"), Ok(3));
        args.set("a", ArgValue::Number(3.5));
        assert!(need_integer(&args, "a", "a").is_err());
        args.set("a", ArgValue::Number(f64::NAN));
        assert!(need_integer(&args, "a", "a").is_err());
        args.set("a", ArgValue::Number(1.0e20));
        assert!(need_integer(&args, "a", "a").is_err());
        args.set("a", ArgValue::Text("7".to_owned()));
        assert_eq!(need_integer(&args, "a", "a"), Ok(7));
        args.set("a", ArgValue::Text("x".to_owned()));
        assert!(need_integer(&args, "a", "a").is_err());
        args.set("a", ArgValue::Flag(true));
        assert!(need_integer(&args, "a", "a").is_err());
        assert!(matches!(
            need_integer(&Args::new(), "a", "a"),
            Err(CommandError::NeedsInput(_))
        ));
        assert_eq!(optional_point(&Args::new()), Ok(None));
    }

    #[test]
    fn connection_indices_read_both_spellings() {
        let mut connect = Connect::new();
        connect.to_part = mutate::VIS_CONNECTION_POINT + 4;
        connect.to_cell = "Connections.X5".to_owned();
        assert_eq!(connection_index(&connect), Some(4));
        connect.to_part = 0;
        assert_eq!(connection_index(&connect), Some(4));
        connect.to_cell = "Connections.X0".to_owned();
        assert_eq!(connection_index(&connect), None);
        connect.to_cell = "PinX".to_owned();
        assert_eq!(connection_index(&connect), None);
    }

    #[test]
    fn a_stale_connector_is_reported_and_its_records_dropped() {
        let (mut ctx, _session, page, connector, _left, _right) = fixture();
        dispatch(
            &mut ctx,
            "connector.glue_begin",
            args(&[("connector", 3), ("target", 1), ("point", 0)]),
        )
        .expect("glue begin");
        ctx.active_document_mut()
            .expect("session")
            .document
            .detach_shape(connector)
            .expect("detach");
        assert!(
            reroute_after_edit(&mut ctx, page, &[])
                .expect("reroute")
                .is_none(),
            "nothing was routed"
        );
        let doc = &ctx.active_document().expect("session").document;
        assert!(doc.page(page).expect("page").connects.is_empty());
        assert_eq!(
            ctx.warnings.len(),
            0,
            "the model detach already dropped the record"
        );
    }

    #[test]
    fn a_re_route_without_a_page_or_connectors_is_a_no_op() {
        let mut ctx = AppCtx::new(Settings::default());
        assert!(reroute_after_edit(&mut ctx, PageId { index: 0 }, &[]).is_ok());
        let (mut ctx, _session, page, _connector, _left, _right) = fixture();
        assert!(reroute_after_edit(&mut ctx, page, &[]).is_ok());
        assert!(reroute_after_edit(&mut ctx, PageId { index: 9 }, &[]).is_ok());
    }

    #[test]
    fn a_missing_connection_point_falls_back_to_dynamic_glue() {
        let (mut ctx, _session, page, connector, left, _right) = fixture();
        {
            let doc = ctx.active_document_mut().expect("session");
            let mut connect = Connect::new();
            connect.from_sheet = doc.document.shape(connector).expect("connector").visio_id;
            connect.from_cell = "BeginX".to_owned();
            connect.from_part = mutate::VIS_BEGIN;
            connect.to_sheet = doc.document.shape(left).expect("left").visio_id;
            connect.to_cell = "Connections.X9".to_owned();
            connect.to_part = mutate::VIS_CONNECTION_POINT + 8;
            doc.document.add_connect(page, connect).expect("connect");
        }
        reroute_connectors(&mut ctx, page, &[connector]).expect("reroute");
        assert!(
            ctx.warnings
                .iter()
                .any(|line| line.contains("has no connection point 8")),
            "{:?}",
            ctx.warnings
        );
        // The end falls back to dynamic glue: the left box has one
        // candidate (its connection point 0 at the page point (0, 1.5)),
        // so the end lands there.
        assert_eq!(
            path_points(&ctx, connector).first(),
            Some(&Point::new(0.0, 1.5))
        );
    }

    #[test]
    fn an_extended_rows_a_and_b_are_not_a_direction() {
        // A connection row with A/B (the extended spellings) but no
        // DirX/DirY: the exit is the nearest side's normal, not A/B.
        let mut doc = Document::new();
        let page = doc.add_page("Page-1").expect("page");
        let mut shape = shape_at(1, 2.0, 2.0, 2.0, 1.0);
        let mut section = Section::new(SectionName::from_static("Connection"));
        let mut row = Row::new();
        row.row_type = Some("Connection".to_owned());
        row.ix = Some(0);
        row.cells = vec![
            cell("X", 1.0),
            cell("Y", 1.0),
            cell("A", 3.0),
            cell("B", 0.0),
        ];
        section.rows.push(row);
        shape.sheet.sections.push(section);
        let target = doc.insert_shape(page, shape, None).expect("target");
        let mut ctx = AppCtx::new(Settings::default());
        ctx.open_document(DocumentSession::new(doc, None));
        let session = ctx.active_document().expect("session");
        let built = glue_target(&session.document, target).expect("target");
        let candidates = built.candidates();
        assert_eq!(
            candidates[0].exit,
            Some(larrygram_geom::Point::new(0.0, 1.0)),
            "the nearest side's normal (the row's top), not A/B"
        );
    }

    #[test]
    fn a_route_edit_labels_and_restores_itself() {
        let (mut ctx, session, page, connector, _left, _right) = fixture();
        {
            let doc = ctx.active_document_mut().expect("session");
            let mut connect = Connect::new();
            connect.from_sheet = doc.document.shape(connector).expect("c").visio_id;
            connect.from_cell = "BeginX".to_owned();
            connect.from_part = mutate::VIS_BEGIN;
            connect.to_sheet = 1;
            connect.to_cell = "PinX".to_owned();
            connect.to_part = mutate::VIS_WHOLE_SHAPE;
            doc.document.add_connect(page, connect).expect("connect");
        }
        let _ = session;
        // A forced pass re-routes the connector and hands back its
        // snapshot: the edit labels itself and its apply and revert
        // write the same sheets.
        let mut edit = reroute_connectors(&mut ctx, page, &[connector])
            .expect("pass")
            .expect("it rewrote rows");
        assert_eq!(edit.label(), "Route connector");
        let sheet = |ctx: &AppCtx| {
            ctx.active_document()
                .expect("session")
                .document
                .shape(connector)
                .expect("c")
                .sheet
                .clone()
        };
        let before = sheet(&ctx);
        edit.revert(&mut ctx).expect("revert");
        edit.apply(&mut ctx).expect("apply");
        assert_eq!(sheet(&ctx), before, "apply writes the same sheets");
    }

    /// Task P7-50 minor 5: the re-route snapshot keeps only what
    /// `write_path` touches (the `Geometry` sections and the endpoint,
    /// pin and size cells) and restores the sheet exactly, the cells
    /// the write added included, with the unrelated cell and section
    /// never in the snapshot.
    #[test]
    fn the_route_snapshot_restores_only_what_the_path_write_touches() {
        let mut doc = Document::new();
        let page = doc.add_page("Page-1").expect("page");
        // A 1-D shape with no local-pin or angle cells (the path write
        // adds them), an unrelated cell and an unrelated section.
        let mut shape = Shape::new(ShapeId { index: 0 }, 1);
        shape.sheet.cells = vec![
            cell("BeginX", 1.0),
            cell("BeginY", 1.0),
            cell("EndX", 3.0),
            cell("EndY", 1.0),
            cell("PinX", 2.0),
            cell("PinY", 1.0),
            cell("Width", 2.0),
            cell("Height", 0.2),
        ];
        shape.sheet.cells.push(Cell {
            name: CellName::new("Comment"),
            formula: None,
            value: Some(CellValue::Str("keep me".to_owned())),
            unit: larrygram_geom::Unit::Unitsless,
            preserved: Preserved::new(),
        });
        let mut geometry = Section::new(SectionName::from_static("Geometry"));
        for (ix, (kind, x, y)) in [("MoveTo", 0.0, 0.1), ("LineTo", 2.0, 0.1)]
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
            .sheet
            .sections
            .push(Section::new(SectionName::from_static("Character")));
        let id = doc.insert_shape(page, shape, None).expect("shape");
        let original = doc.shape(id).expect("shape").sheet.clone();
        let before = PathRows::capture(&doc, id);
        assert!(
            !before
                .cells
                .iter()
                .any(|(_, cell)| cell.name.as_str() == "Comment"),
            "the snapshot holds no unrelated cell"
        );
        assert!(
            before
                .geometry
                .iter()
                .all(|(_, section)| section.name.as_str() == "Geometry"),
            "the snapshot holds no unrelated section"
        );
        write_path(&mut doc, id, &[Point::new(2.0, 2.0), Point::new(5.0, 4.0)]).expect("write");
        let written = doc.shape(id).expect("shape").sheet.clone();
        let after = PathRows::capture(&doc, id);
        assert!(
            before.cells.len() < after.cells.len(),
            "the write added the pin cells: {} < {}",
            before.cells.len(),
            after.cells.len()
        );
        before.restore(&mut doc, id);
        assert_eq!(
            doc.shape(id).expect("shape").sheet,
            original,
            "revert is exact"
        );
        after.restore(&mut doc, id);
        assert_eq!(
            doc.shape(id).expect("shape").sheet,
            written,
            "redo is exact"
        );
    }

    #[test]
    fn a_dangling_record_falls_back_to_the_stored_point() {
        let (mut ctx, _session, page, connector, _left, _right) = fixture();
        {
            let doc = ctx.active_document_mut().expect("session");
            let mut connect = Connect::new();
            connect.from_sheet = doc.document.shape(connector).expect("c").visio_id;
            connect.from_cell = "EndX".to_owned();
            connect.from_part = mutate::VIS_END;
            connect.to_sheet = 99;
            connect.to_cell = "PinX".to_owned();
            connect.to_part = mutate::VIS_WHOLE_SHAPE;
            doc.document.add_connect(page, connect).expect("connect");
        }
        let session = ctx.active_document().expect("session");
        let doc = &session.document;
        let ends = doc.connects_of(page, connector).expect("ends");
        let mut warnings = Vec::new();
        let (_begin, _end, boxes) = resolved_ends(
            doc,
            page,
            connector,
            [
                ends.first().and_then(Option::as_ref),
                ends.get(1).and_then(Option::as_ref),
            ],
            [None, None],
            &mut warnings,
        )
        .expect("resolve");
        assert!(
            boxes.iter().all(Option::is_none),
            "no target box: {boxes:?}"
        );
    }

    #[test]
    fn a_forced_pass_re_routes_a_connector_with_free_ends() {
        // The Forced scope re-routes its connectors even when no
        // record names them (rule 5's "its own ends" category).
        let (mut ctx, _session, page, _connector, _left, _right) = fixture();
        let line = {
            let session = ctx.active_document_mut().expect("session");
            session
                .document
                .insert_shape(page, one_d(8, (1.0, 6.0), (5.0, 6.5), false), None)
                .expect("line")
        };
        let edit = reroute_connectors(&mut ctx, page, &[line])
            .expect("pass")
            .expect("it rewrote rows");
        assert_eq!(edit.label(), "Route connector");
    }

    #[test]
    fn a_singular_parent_names_the_failure() {
        // The guard's contract (the transform construction itself is
        // never singular: the scale is always plus or minus one, so no
        // flow reaches it).
        assert_eq!(singular().to_string(), "singular parent transform");
    }

    #[test]
    fn connectors_are_never_glue_targets() {
        let (mut ctx, _session, page, _connector, _left, _right) = fixture();
        let line = {
            let session = ctx.active_document_mut().expect("session");
            session
                .document
                .insert_shape(page, one_d(7, (1.0, 1.0), (5.0, 1.0), false), None)
                .expect("line")
        };
        let visio = ctx
            .active_document()
            .expect("session")
            .document
            .shape(line)
            .expect("line")
            .visio_id;

        // Gluing the line to itself: refused, for any source.
        let error = dispatch(
            &mut ctx,
            "connector.glue_begin",
            args(&[
                ("connector", i64::from(visio)),
                ("target", i64::from(visio)),
            ]),
        )
        .expect_err("the connector itself");
        assert!(error.to_string().contains("never glue targets"), "{error}");

        // And through a draw.
        let error = dispatch(
            &mut ctx,
            "connector.draw",
            Args::new()
                .with("stencil", ArgValue::Text("Basic Flowchart".to_owned()))
                .with("master", ArgValue::Text("Connector".to_owned()))
                .with("x", ArgValue::Number(1.0))
                .with("y", ArgValue::Number(1.0))
                .with("end_target", ArgValue::Integer(i64::from(visio))),
        )
        .expect_err("a 1-D target through the draw");
        assert!(error.to_string().contains("never glue targets"), "{error}");
    }

    #[test]
    fn a_glue_edit_restores_the_old_record_at_its_position() {
        let (mut ctx, _session, page, connector, left, right) = fixture();
        dispatch(
            &mut ctx,
            "connector.glue_begin",
            args(&[("connector", 3), ("target", 1), ("point", 0)]),
        )
        .expect("glue begin");
        let doc = ctx.active_document().expect("session").document.clone();
        let session = ctx.active_document().expect("session").id;
        let mut edit = {
            let record = glue_record(
                connector,
                right,
                doc.shape(right).expect("right").visio_id,
                None,
                End::Begin,
                &doc,
            )
            .expect("record");
            GlueEdit::new(
                &doc,
                session,
                "Swap glue",
                page,
                connector,
                doc.shape(connector).expect("connector").visio_id,
                End::Begin,
                Some(record),
                Point::new(2.0, 2.0),
            )
            .expect("edit")
        };
        edit.apply(&mut ctx).expect("apply");
        let ends = ctx
            .active_document()
            .expect("session")
            .document
            .connects_of(page, connector)
            .expect("ends");
        assert_eq!(ends[0].as_ref().map(|c| c.to_sheet), Some(2));
        edit.revert(&mut ctx).expect("revert");
        let ends = ctx
            .active_document()
            .expect("session")
            .document
            .connects_of(page, connector)
            .expect("ends");
        assert_eq!(ends[0].as_ref().map(|c| c.to_sheet), Some(1));
        let _ = left;
    }

    #[test]
    fn page_transform_composes_a_parent_chain_and_rejects_cycles() {
        let (mut ctx, _session, page, connector, _left, _right) = fixture();
        let parent = {
            let doc = ctx.active_document_mut().expect("session");
            let parent = insert(&mut doc.document, page, shape_at(9, 0.0, 0.0, 10.0, 10.0));
            if let Some(entry) = doc.document.shape_mut(parent) {
                entry.children.push(connector);
            }
            if let Some(entry) = doc.document.shape_mut(connector) {
                entry.parent = Some(parent);
            }
            parent
        };
        let doc = &ctx.active_document().expect("session").document;
        let transform = page_transform(doc, connector).expect("transform");
        // Connector local (0, 0) is page (2, 1.5); the parent's local
        // transform shifts it by (-5, -5).
        assert_eq!(
            transform.apply(Point::new(0.0, 0.0)),
            Point::new(-3.0, -3.5)
        );

        // A cycle is an error, never a loop.
        {
            let doc = ctx.active_document_mut().expect("session");
            if let Some(entry) = doc.document.shape_mut(parent) {
                entry.parent = Some(connector);
            }
        }
        let doc = &ctx.active_document().expect("session").document;
        assert!(page_transform(doc, connector).is_err());
    }
    #[test]
    fn direct_calls_without_a_document_or_page_name_the_failure() {
        let invocation = Invocation::empty(crate::Source::Test);
        let mut ctx = AppCtx::new(Settings::default());
        assert_eq!(
            run_reroute_all(&mut ctx, &invocation)
                .expect_err("no document")
                .to_string(),
            "no open document"
        );
        ctx.open_document(DocumentSession::new(Document::new(), None));
        assert_eq!(
            run_reroute_all(&mut ctx, &invocation)
                .expect_err("no page")
                .to_string(),
            "no active page"
        );
    }

    #[test]
    fn unknown_connector_and_target_ids_are_argument_errors() {
        let (mut ctx, _session, _page, _connector, _left, _right) = fixture();
        let invocation = Invocation::new(
            args(&[("connector", 99)]).with("end", ArgValue::Text("begin".to_owned())),
            crate::Source::Test,
        );
        let error = run_unglue(&mut ctx, &invocation).expect_err("connector");
        assert!(error.to_string().contains("unknown connector shape id 99"));

        let invocation = Invocation::new(
            args(&[("connector", 3), ("target", 99)]),
            crate::Source::Test,
        );
        let error = run_glue_begin(&mut ctx, &invocation).expect_err("target");
        assert!(error.to_string().contains("unknown target shape id 99"));
    }

    #[test]
    fn unglue_validates_the_end_argument() {
        let (mut ctx, _session, _page, _connector, _left, _right) = fixture();
        let missing = Invocation::new(args(&[("connector", 3)]), crate::Source::Test);
        assert!(matches!(
            run_unglue(&mut ctx, &missing),
            Err(CommandError::NeedsInput(_))
        ));
        let bad = Invocation::new(
            args(&[("connector", 3)]).with("end", ArgValue::Text("side".to_owned())),
            crate::Source::Test,
        );
        let error = run_unglue(&mut ctx, &bad).expect_err("bad end");
        assert!(
            error.to_string().contains("end must be begin or end"),
            "{error}"
        );
    }

    #[test]
    fn a_reroute_with_no_document_is_a_no_op() {
        let mut ctx = AppCtx::new(Settings::default());
        assert!(
            reroute_after_edit(&mut ctx, PageId { index: 0 }, &[])
                .expect("no document")
                .is_none()
        );
    }

    /// Task P5-19 coverage: `PathEdit`'s label (nothing read it before)
    /// and the two page-gone glue-drop guards.
    #[test]
    fn path_edit_labels_itself_and_the_page_gone_guards_are_no_ops() {
        let mut doc = Document::new();
        let page = doc.add_page("Page-1").expect("page");
        let id = insert(&mut doc, page, shape_at(1, 0.0, 0.0, 1.0, 1.0));
        let edit = PathEdit::new(
            &doc,
            DocumentSession::new(Document::new(), None).id,
            id,
            vec![Point::new(0.0, 0.0), Point::new(1.0, 1.0)],
        )
        .expect("edit");
        assert_eq!(edit.label(), "Route connector");

        let mut warnings = Vec::new();
        drop_detached_targets(&mut doc, PageId { index: 9 }, 1, &mut warnings);
        drop_all_for(&mut doc, PageId { index: 9 }, 1);
        assert!(warnings.is_empty());
    }

    // --- Task P7-39: 1-D moves, copies, deletes ---------------------------

    /// A 1-D shape from `begin` to `end` with literal endpoint cells and
    /// the master's pin and size formulas, its geometry rows literal or,
    /// with `formula_rows`, formula-driven (`Width*0`, `Width*1`).
    fn one_d(visio_id: i32, begin: (f64, f64), end: (f64, f64), formula_rows: bool) -> Shape {
        let formula = |name: &str, text: &str, value: f64| Cell {
            name: CellName::new(name),
            formula: Some(text.to_owned()),
            value: Some(CellValue::Number(value)),
            unit: larrygram_geom::Unit::Inches,
            preserved: Preserved::new(),
        };
        let width = end.0 - begin.0;
        let mut shape = Shape::new(ShapeId { index: 0 }, visio_id);
        shape.sheet.cells = vec![
            cell("BeginX", begin.0),
            cell("BeginY", begin.1),
            cell("EndX", end.0),
            cell("EndY", end.1),
            formula("PinX", "GUARD((BeginX+EndX)/2)", (begin.0 + end.0) / 2.0),
            formula("PinY", "GUARD((BeginY+EndY)/2)", (begin.1 + end.1) / 2.0),
            formula("Width", "GUARD(EndX-BeginX)", width),
            formula("Height", "GUARD(0.2DL)", 0.2),
            formula("LocPinX", "GUARD(Width*0.5)", width / 2.0),
            formula("LocPinY", "GUARD(Height*0.5)", 0.1),
            formula("Angle", "GUARD(0DA)", 0.0),
            cell("ObjType", 2.0),
        ];
        let mut geometry = Section::new(SectionName::from_static("Geometry"));
        for (ix, (kind, x, text)) in [("MoveTo", 0.0, "Width*0"), ("LineTo", width, "Width*1")]
            .into_iter()
            .enumerate()
        {
            let mut row = Row::new();
            row.row_type = Some(kind.to_owned());
            row.ix = Some(i32::try_from(ix).unwrap_or(0));
            row.cells = if formula_rows {
                vec![formula("X", text, x), formula("Y", "Height*0.5", 0.1)]
            } else {
                vec![cell("X", x), cell("Y", 0.1)]
            };
            geometry.rows.push(row);
        }
        shape.sheet.sections.push(geometry);
        shape
    }

    fn open(doc: Document) -> (AppCtx, SessionId) {
        let mut ctx = AppCtx::new(Settings::default());
        let index = ctx.open_document(DocumentSession::new(doc, None));
        let session = ctx.documents.get(index).expect("session").id;
        (ctx, session)
    }

    fn end_at(ctx: &AppCtx, shape: ShapeId, end: ConnectorEnd) -> Point {
        ctx.active_document()
            .expect("session")
            .document
            .connector_end(shape, end)
            .expect("end")
    }

    #[test]
    fn end_glue_of_refuses_an_out_of_range_row() {
        let mut connect = Connect::new();
        connect.to_sheet = 2;
        connect.to_cell = "Connections.X99999999999".to_owned();
        connect.to_part = 0;
        assert!(end_glue_of(Some(&connect)).is_err());
        assert_eq!(end_glue_of(None).expect("free"), EndGlue::Free);
    }

    #[test]
    fn a_formula_driven_1d_shape_moves_its_ends_and_keeps_its_rows() {
        let mut doc = Document::new();
        let page = doc.add_page("Page-1").expect("page");
        let line = insert(&mut doc, page, one_d(4, (1.0, 1.0), (3.0, 1.0), true));
        let rows = doc.shape(line).expect("line").sheet.sections.clone();
        translate_one_d(&mut doc, line, 1.0, 2.0).expect("move");
        assert_eq!(
            doc.connector_end(line, ConnectorEnd::Begin).expect("begin"),
            Point::new(2.0, 3.0)
        );
        assert_eq!(
            doc.shape(line).expect("line").sheet.sections,
            rows,
            "rows kept"
        );
    }

    #[test]
    fn a_1d_shape_without_rows_moves_as_a_straight_line() {
        let mut doc = Document::new();
        let page = doc.add_page("Page-1").expect("page");
        let mut bare = one_d(4, (1.0, 1.0), (3.0, 2.0), false);
        bare.sheet.sections.clear();
        let line = insert(&mut doc, page, bare);
        translate_one_d(&mut doc, line, 0.5, 0.0).expect("move");
        assert_eq!(
            doc.connector_end(line, ConnectorEnd::End).expect("end"),
            Point::new(3.5, 2.0)
        );
        // A shape whose ends are not numbers cannot move.
        let mut broken = one_d(5, (0.0, 0.0), (1.0, 0.0), false);
        if let Some(cell) = broken
            .sheet
            .cells
            .iter_mut()
            .find(|cell| cell.name.as_str() == "BeginX")
        {
            cell.value = Some(CellValue::Str("x".to_owned()));
        }
        let broken = insert(&mut doc, page, broken);
        assert!(translate_one_d(&mut doc, broken, 1.0, 0.0).is_err());
        assert!(parent_transform(&doc, ShapeId { index: 999 }).is_err());
    }

    #[test]
    fn a_1d_member_of_a_group_moves_in_its_parents_space() {
        let mut doc = Document::new();
        let page = doc.add_page("Page-1").expect("page");
        let group = insert(&mut doc, page, shape_at(1, 4.0, 4.0, 4.0, 4.0));
        let line = doc
            .insert_subshape(group, one_d(2, (1.0, 1.0), (3.0, 1.0), false), None)
            .expect("member");
        let (mut ctx, _) = open(doc);
        ctx.selection.shapes = vec![line];
        ctx.selection.anchor = Some(line);
        dispatch(
            &mut ctx,
            "shape.move",
            Args::new()
                .with("dx", ArgValue::Number(1.0))
                .with("dy", ArgValue::Number(0.0)),
        )
        .expect("move");
        // The group's local box starts at (2, 2) on the page: the cells
        // are in the group's space and move by the page delta.
        assert_eq!(
            end_at(&ctx, line, ConnectorEnd::Begin),
            Point::new(2.0, 1.0)
        );
    }

    #[test]
    fn the_staged_edits_skip_what_they_do_not_touch() {
        let (ctx, session, page, connector, left, _right) = fixture();
        let doc = &ctx.active_document().expect("session").document;
        // No 1-D shape to move: nothing staged.
        let staged = OneDMoveEdit::stage(doc, session, page, &[left, connector], &[], (1.0, 0.0))
            .expect("stage");
        assert!(staged.is_none());
        assert!(
            OneDMoveEdit::stage(doc, session, PageId { index: 9 }, &[], &[], (0.0, 0.0)).is_err()
        );
        // A legacy record-only connector glued to a deleted shape has no
        // formulas to clear.
        let mut doc = doc.clone();
        let mut connect = Connect::new();
        connect.from_sheet = 3;
        connect.from_cell = "BeginX".to_owned();
        connect.from_part = mutate::VIS_BEGIN;
        connect.to_sheet = 1;
        connect.to_cell = "PinX".to_owned();
        connect.to_part = mutate::VIS_WHOLE_SHAPE;
        doc.add_connect(page, connect).expect("record");
        assert!(GlueClearEdit::stage(&doc, session, page, &[left]).is_none());
    }

    #[test]
    fn the_1d_edits_carry_their_labels() {
        let mut doc = Document::new();
        let page = doc.add_page("Page-1").expect("page");
        let line = insert(&mut doc, page, one_d(4, (1.0, 1.0), (3.0, 1.0), false));
        let (ctx, session) = open(doc.clone());
        let staged = OneDMoveEdit::stage(&doc, session, page, &[line], &[line], (1.0, 0.0))
            .expect("stage")
            .expect("one 1-D shape");
        assert_eq!(staged.label(), "Move 1-D shape");
        assert!(!staged.dirty_cells().is_empty());
        let target = insert(&mut doc, page, shape_at(5, 6.0, 1.0, 2.0, 1.0));
        let visio = |doc: &Document, shape: ShapeId| doc.shape(shape).expect("shape").visio_id;
        let mut connect = Connect::new();
        connect.from_sheet = visio(&doc, line);
        connect.from_cell = "EndX".to_owned();
        connect.from_part = mutate::VIS_END;
        connect.to_sheet = visio(&doc, target);
        connect.to_cell = "PinX".to_owned();
        connect.to_part = mutate::VIS_WHOLE_SHAPE;
        doc.add_connect(page, connect).expect("record");
        let clear = GlueClearEdit::stage(&doc, session, page, &[target]).expect("staged");
        assert_eq!(clear.label(), "Clear glue to deleted shapes");
        let _ = ctx;
    }

    #[test]
    fn the_subtree_walk_survives_a_cyclic_child_list() {
        let mut doc = Document::new();
        let page = doc.add_page("Page-1").expect("page");
        let group = insert(&mut doc, page, shape_at(1, 4.0, 4.0, 4.0, 4.0));
        let member = doc
            .insert_subshape(group, shape_at(2, 1.0, 1.0, 1.0, 1.0), None)
            .expect("member");
        if let Some(entity) = doc.shape_mut(member) {
            entity.children.push(group);
        }
        let mut ids = subtree_visio_ids(&doc, &[group]);
        ids.sort_unstable();
        assert_eq!(ids, vec![1, 2]);
    }

    #[test]
    fn a_copy_with_unreadable_ends_keeps_them() {
        let mut doc = Document::new();
        let page = doc.add_page("Page-1").expect("page");
        let mut broken = one_d(4, (0.0, 0.0), (1.0, 0.0), false);
        if let Some(cell) = broken
            .sheet
            .cells
            .iter_mut()
            .find(|cell| cell.name.as_str() == "EndY")
        {
            cell.value = Some(CellValue::Str("x".to_owned()));
        }
        let broken = insert(&mut doc, page, broken);
        rebind_copied_glue(&mut doc, broken, &[]).expect("rebind");
        assert_eq!(
            doc.connector_end(broken, ConnectorEnd::Begin)
                .expect("begin"),
            Point::new(0.0, 0.0)
        );
        // A shape that is not 1-D has nothing to rebind.
        let plain = insert(&mut doc, page, shape_at(5, 1.0, 1.0, 1.0, 1.0));
        rebind_copied_glue(&mut doc, plain, &[]).expect("plain");
    }

    #[test]
    fn a_target_gone_without_a_delete_frees_the_end_on_the_next_pass() {
        let mut doc = Document::new();
        let page = doc.add_page("Page-1").expect("page");
        let target = insert(&mut doc, page, shape_at(1, 6.0, 1.0, 2.0, 1.0));
        let line = insert(&mut doc, page, one_d(4, (1.0, 1.0), (5.0, 1.0), false));
        let target_visio = doc.shape(target).expect("target").visio_id;
        doc.write_connector_end(
            line,
            ConnectorEnd::End,
            Point::new(5.0, 1.0),
            EndGlue::Dynamic {
                sheet: target_visio,
            },
        )
        .expect("glue formula");
        let mut connect = Connect::new();
        connect.from_sheet = doc.shape(line).expect("line").visio_id;
        connect.from_cell = "EndX".to_owned();
        connect.from_part = mutate::VIS_END;
        connect.to_sheet = target_visio;
        connect.to_cell = "PinX".to_owned();
        connect.to_part = mutate::VIS_WHOLE_SHAPE;
        doc.add_connect(page, connect).expect("record");
        // The target leaves the page's list but the record survives (a
        // file with a dangling record).
        if let Some(entity) = doc.pages.get_mut(0) {
            entity.shapes.retain(|shape| *shape != target);
        }
        let mut warnings = Vec::new();
        reroute_document(&mut doc, page, Scope::Changed(&[]), &mut warnings).expect("pass");
        assert!(doc.page(page).expect("page").connects.is_empty());
        let end_x = doc
            .shape(line)
            .expect("line")
            .sheet
            .cells
            .iter()
            .find(|cell| cell.name.as_str() == "EndX")
            .cloned()
            .expect("EndX");
        assert_eq!(end_x.formula, None, "the stale glue formula goes");
        assert!(warnings.iter().any(|line| line.contains("lost its glue")));
    }

    #[test]
    fn a_legacy_connector_glues_from_its_geometry_or_its_pin() {
        // The fixture's connector has no endpoint cells and no rows: its
        // ends fall back to its pin.
        let (mut ctx, _session, _page, connector, _left, _right) = fixture();
        dispatch(
            &mut ctx,
            "connector.glue_begin",
            args(&[("connector", 3), ("target", 1)]),
        )
        .expect("glue");
        // With rows, the geometry's ends are the stored points.
        if let Some(slot) = ctx.active_document_mut() {
            slot.document
                .write_connector_path(connector, &[Point::new(0.0, 0.0), Point::new(2.0, 1.0)])
                .expect("rows");
        }
        dispatch(
            &mut ctx,
            "connector.glue_end",
            args(&[("connector", 3), ("target", 2)]),
        )
        .expect("glue");
        let doc = &ctx.active_document().expect("session").document;
        assert!(!doc.is_connector(connector), "nothing migrates it");
        assert_eq!(
            doc.page(PageId { index: 0 }).expect("page").connects.len(),
            2
        );
    }

    /// The facts the route reads: `size_cells` on a shape without
    /// sizes, `direct_number` on a non-number cell, `row_direction`
    /// with and without a matching row, and `inherits_style`'s
    /// own-cell read.
    #[test]
    fn the_route_facts_read_only_what_they_can() {
        // A shape whose sizes are not numbers counts as 1.0 x 1.0.
        let mut text = Shape::new(ShapeId { index: 0 }, 2);
        for name in ["Width", "Height"] {
            text.sheet.cells.push(Cell {
                name: CellName::new(name),
                formula: None,
                value: Some(CellValue::Str("wide".to_owned())),
                unit: larrygram_geom::Unit::Unitsless,
                preserved: Preserved::new(),
            });
        }
        let mut doc = Document::new();
        let page = doc.add_page("Page-1").expect("page");
        let text = insert(&mut doc, page, text);
        assert_eq!(size_cells(&doc, text), (1.0, 1.0));

        // A non-number cell reads as nothing.
        let text = doc.shape(text).expect("text shape");
        assert_eq!(direct_number(text, "Width"), None);
        assert_eq!(direct_number(text, "Missing"), None);

        // A row direction: the matching row's finite non-zero
        // direction scales by the rung's box.
        let mut row = Row::new();
        row.row_type = Some("Connection".to_owned());
        row.ix = Some(3);
        row.cells = vec![cell("DirX", 1.0), cell("DirY", 0.0)];
        let mut sections = vec![Section::new(SectionName::from_static("Connection"))];
        sections[0].rows.push(row);
        assert_eq!(
            row_direction(&sections, 3, 2.0, 3.0),
            Some(Point::new(2.0, 0.0))
        );
        assert_eq!(
            row_direction(&sections, 4, 2.0, 3.0),
            None,
            "a row that is not there has no direction"
        );

        // `inherits_style` reads the connector's own style cells: both
        // present and non-zero keep the page's value away, one missing
        // lets it through.
        let mut own = one_d(3, (0.0, 0.0), (2.0, 0.0), false);
        own.sheet
            .cells
            .push(cell(style_cells::SHAPE_ROUTE_STYLE, 1.0));
        own.sheet
            .cells
            .push(cell(style_cells::CON_LINE_ROUTE_EXT, 3.0));
        let doc = {
            let mut doc = Document::new();
            let page = doc.add_page("Page-1").expect("page");
            insert(&mut doc, page, own);
            doc
        };
        assert!(
            !inherits_style(&doc, ShapeId { index: 0 }),
            "the connector styles itself"
        );
        let mut doc = Document::new();
        let page = doc.add_page("Page-1").expect("page");
        let plain = insert(&mut doc, page, one_d(4, (0.0, 0.0), (2.0, 0.0), false));
        assert!(
            inherits_style(&doc, plain),
            "an absent style cell inherits the page's"
        );
    }

    /// `connection_direction` walks the master rungs: a rung without
    /// `Connection` rows is skipped, and none with rows answers nothing.
    #[test]
    fn connection_direction_walks_the_master_rungs() {
        use larrygram_model::{Master, MasterId};
        let pointed = |visio_id: i32| {
            let mut shape = shape_at(visio_id, 2.0, 2.0, 2.0, 1.0);
            let mut connection = Section::new(SectionName::from_static("Connection"));
            let mut row = Row::new();
            row.row_type = Some("Connection".to_owned());
            row.ix = Some(0);
            row.cells = vec![
                cell("X", 1.0),
                cell("Y", 1.0),
                cell("DirX", 1.0),
                cell("DirY", 0.0),
            ];
            connection.rows.push(row);
            shape.sheet.sections.push(connection);
            shape
        };
        let mut doc = Document::new();
        let page = doc.add_page("Page-1").expect("page");
        // The chain holds a rowless rung before the pointed one (master
        // A's rung is an instance of master B): the walk skips the
        // first and answers the second's direction.
        let rowed = doc.add_shape(pointed(6));
        let mut master_b = Master::new(MasterId { index: 1 }, 7);
        master_b.name_u = "Pointed".to_owned();
        master_b.shapes = vec![rowed];
        let master_b = doc.add_master(master_b).expect("master B");
        let mut rung = shape_at(5, 2.0, 2.0, 2.0, 1.0);
        rung.master = Some(master_b);
        let rowless = doc.add_shape(rung);
        let mut master = Master::new(MasterId { index: 0 }, 5);
        master.name_u = "Mixed".to_owned();
        master.shapes = vec![rowless];
        let master = doc.add_master(master).expect("master");
        let instance = doc
            .instantiate_master(master, page, Point::new(5.0, 2.0), None)
            .expect("instance");
        assert_eq!(
            connection_direction(&doc, instance, 0),
            Some(Point::new(1.0, 0.0)),
            "the rowed rung's direction, scaled by the instance"
        );
        assert_eq!(
            connection_direction(&doc, instance, 5),
            None,
            "a row index no rung holds"
        );

        // A master without rows anywhere answers nothing.
        let mut doc = Document::new();
        let page = doc.add_page("Page-1").expect("page");
        let rowless = doc.add_shape(shape_at(5, 2.0, 2.0, 2.0, 1.0));
        let mut master = Master::new(MasterId { index: 1 }, 7);
        master.name_u = "Bare".to_owned();
        master.shapes = vec![rowless];
        let master = doc.add_master(master).expect("master");
        let instance = doc
            .instantiate_master(master, page, Point::new(5.0, 2.0), None)
            .expect("instance");
        assert_eq!(connection_direction(&doc, instance, 0), None);
    }

    /// The route snapshot skips the shape it cannot see: a gone shape
    /// captures nothing and a restore onto it changes nothing.
    #[test]
    fn the_route_snapshot_skips_a_gone_shape() {
        let (mut ctx, _session, _page, connector, _left, _right) = fixture();
        let doc = &ctx.active_document().expect("session").document;
        let empty = PathRows::capture(doc, ShapeId { index: 9 });
        assert!(
            empty.geometry.is_empty() && empty.cells.is_empty(),
            "a gone shape captures nothing"
        );
        let before = PathRows::capture(doc, connector);
        let sheet = doc.shape(connector).expect("connector").sheet.clone();
        empty.restore(
            &mut ctx.active_document_mut().expect("session").document,
            ShapeId { index: 9 },
        );
        let doc = &ctx.active_document().expect("session").document;
        assert_eq!(
            doc.shape(connector).expect("connector").sheet,
            sheet,
            "a restore onto a gone shape touches nothing"
        );
        let after = PathRows::capture(doc, connector);
        assert_eq!(before.cells.len(), after.cells.len());
    }

    /// The glue clear skips an item whose connector is gone or not a
    /// connector (a replayed delete): its sheet is never written.
    #[test]
    fn the_clear_edit_skips_a_connector_that_is_gone() {
        let (mut ctx, session, _page, _connector, left, _right) = fixture();
        let sheet = ctx
            .active_document()
            .expect("session")
            .document
            .shape(left)
            .expect("plain shape")
            .sheet
            .clone();
        let mut edit = GlueClearEdit {
            session,
            items: vec![ClearedConnector {
                connector: left,
                ends: vec![(End::Begin, Point::new(1.0, 2.0))],
                old_sheet: Sheet::default(),
            }],
        };
        edit.apply(&mut ctx).expect("the skip applies cleanly");
        assert_eq!(
            ctx.active_document()
                .expect("session")
                .document
                .shape(left)
                .expect("plain shape")
                .sheet,
            sheet,
            "a plain shape has no glue formulas to clear"
        );
    }

    /// A 1-D move whose page is gone (a replayed edit) still moves its
    /// shape: only the record pass is skipped.
    #[test]
    fn a_one_d_move_apply_skips_a_gone_page() {
        let mut doc = Document::new();
        let page = doc.add_page("Page-1").expect("page");
        let line = insert(&mut doc, page, one_d(1, (0.0, 0.0), (2.0, 0.0), false));
        let old_sheet = doc.shape(line).expect("line").sheet.clone();
        let mut ctx = AppCtx::new(Settings::default());
        let index = ctx.open_document(DocumentSession::new(doc, None));
        let session = ctx.documents.get(index).expect("session").id;
        let mut edit = OneDMoveEdit {
            session,
            page: PageId { index: 9 },
            dx: 1.0,
            dy: 0.0,
            items: vec![MovedOneD {
                shape: line,
                visio_id: 1,
                freed: vec![End::Begin],
                old_sheet,
            }],
            old_connects: Vec::new(),
            dirty: Vec::new(),
        };
        edit.apply(&mut ctx).expect("the move applies");
        let moved = ctx
            .active_document()
            .expect("session")
            .document
            .shape(line)
            .expect("line")
            .sheet
            .cells
            .iter()
            .find(|cell| cell.name.as_str() == "BeginX")
            .and_then(|cell| cell.value.clone())
            .expect("BeginX");
        assert_eq!(moved, CellValue::Number(1.0), "the path moved");
    }

    /// A moved curve whose NURBS row carries no control-point data
    /// keeps its rows: the data block is optional (task P7-36 item 2).
    #[test]
    fn a_moved_curve_without_control_points_keeps_its_rows() {
        let mut shape = one_d(1, (0.0, 0.0), (2.0, 0.0), false);
        let mut geometry = Section::new(SectionName::from_static("Geometry"));
        let mut move_to = Row::new();
        move_to.row_type = Some("MoveTo".to_owned());
        move_to.ix = Some(0);
        move_to.cells = vec![cell("X", 0.0), cell("Y", 0.0)];
        geometry.rows.push(move_to);
        let mut nurbs = Row::new();
        nurbs.row_type = Some("NurbsTo".to_owned());
        nurbs.ix = Some(1);
        nurbs.cells = vec![
            cell("X", 2.0),
            cell("Y", 0.0),
            cell("A", 0.0),
            cell("B", 1.0),
            cell("C", 0.0),
            cell("D", 1.0),
        ];
        geometry.rows.push(nurbs);
        shape.sheet.sections = vec![geometry];
        let mut doc = Document::new();
        let page = doc.add_page("Page-1").expect("page");
        let line = insert(&mut doc, page, shape);
        translate_one_d(&mut doc, line, 1.0, 0.0).expect("move");
        let rows = doc.shape(line).expect("line").sheet.sections[0]
            .rows
            .clone();
        assert_eq!(rows.len(), 2, "the curve keeps its rows: {rows:?}");
    }
}
