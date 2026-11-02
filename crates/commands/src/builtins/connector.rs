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
//! [`reroute_after_edit`] is the one hook the editing commands call after
//! a move, resize, rotate or delete (and the undo/redo runner after a
//! step): it drops glue whose target is detached and re-writes every
//! affected connector path.

use larrygram_geom::{Point, Rect, RouteOptions, RouteStyle, Transform, route};
use larrygram_model::CellReader;
use larrygram_model::{
    CellLookup, CellName, CellValue, CellWrite, Connect, ConnectorEnd, Document, EndGlue, PageId,
    Resolved, RowTarget, ShapeId, Sheet, SheetRef, mutate,
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
enum End {
    /// The `BeginX` end (`VisFromParts` `visBegin`, 9).
    Begin,
    /// The `EndX` end (`VisFromParts` `visEnd`, 12).
    End,
}

impl End {
    /// The `FromCell` value of this end.
    fn cell(self) -> &'static str {
        match self {
            End::Begin => "BeginX",
            End::End => "EndX",
        }
    }

    /// The `FromPart` value of this end.
    fn part(self) -> i32 {
        match self {
            End::Begin => mutate::VIS_BEGIN,
            End::End => mutate::VIS_END,
        }
    }

    /// The endpoint of one `Connect` record (the `connects_of` rule).
    fn of(connect: &Connect) -> Self {
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
    fn connector_end(self) -> ConnectorEnd {
        match self {
            End::Begin => ConnectorEnd::Begin,
            End::End => ConnectorEnd::End,
        }
    }

    /// The endpoint spelled by the `connector.unglue` argument.
    fn parse(text: &str) -> Option<Self> {
        match text {
            "begin" => Some(End::Begin),
            "end" => Some(End::End),
            _ => None,
        }
    }
}

/// The active page and its session.
fn active_page(ctx: &AppCtx) -> Result<(SessionId, PageId), CommandError> {
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
fn need_integer(args: &Args, name: &str, prompt: &str) -> Result<i32, CommandError> {
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

/// The target shape of a glue run.
fn target_shape(ctx: &AppCtx, page: PageId, args: &Args) -> Result<ShapeId, CommandError> {
    let session = ctx.active_document().ok_or_else(no_document)?;
    let visio_id = need_integer(args, "target", "target shape id")?;
    session
        .document
        .shape_by_visio_id(page, visio_id)
        .ok_or_else(|| {
            CommandError::invalid_args(format!(
                "unknown target shape id {visio_id} on the active page"
            ))
        })
}

/// The `Connect` record for one endpoint glue.
fn glue_record(
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
struct GlueEdit {
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
    /// The point the end lands on (page inches, the cached `V`).
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
    fn new(
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

/// One end's stored point: its endpoint cells or, without them, its
/// geometry path's end (then the pin as the last resort).
fn stored_end_point(doc: &Document, connector: ShapeId, end: End) -> Point {
    current_end_point(doc, connector, end)
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
    let (target_visio_id, record, at) = {
        let doc = &ctx.active_document().ok_or_else(no_document)?.document;
        let target_visio_id = doc
            .shape(target)
            .ok_or_else(|| CommandError::failed("the target shape no longer exists"))?
            .visio_id;
        let record = glue_record(connector, target, target_visio_id, point, end, doc)?;
        let mut warnings = Vec::new();
        let at = end_point_for(doc, page, connector, end, Some(&record), &mut warnings);
        (target_visio_id, record, at)
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
    let warnings = Vec::new();
    let _ = reroute_after_edit(ctx, page);
    Ok(edit_outcome(
        match end {
            End::Begin => format!("Connector begin glued to shape {target_visio_id}"),
            End::End => format!("Connector end glued to shape {target_visio_id}"),
        },
        warnings,
        Box::new(edit),
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
        let mut warnings = Vec::new();
        let at = end_point_for(doc, page, connector, end, None, &mut warnings);
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
    let _ = reroute_after_edit(ctx, page);
    Ok(edit_outcome(
        match end {
            End::Begin => "Connector begin unglued",
            End::End => "Connector end unglued",
        },
        warnings,
        Box::new(edit),
    ))
}

/// `connector.reroute_all`: recomputes every glued connector's path on
/// the active page. Not an edit (ADR 0040 rule 3): no undo step.
pub(super) fn run_reroute_all(
    ctx: &mut AppCtx,
    _inv: &Invocation,
) -> Result<Outcome, CommandError> {
    let (_, page) = active_page(ctx)?;
    let count = reroute_after_edit(ctx, page)?;
    Ok(outcome(
        if count == 1 {
            "Re-routed 1 connector".to_owned()
        } else {
            format!("Re-routed {count} connectors")
        },
        Vec::new(),
    ))
}

/// `connector.style`: sets `ShapeRouteStyle` on every selected shape.
pub(super) fn run_style(ctx: &mut AppCtx, inv: &Invocation) -> Result<Outcome, CommandError> {
    let style = match inv.args.get("style") {
        Some(ArgValue::Text(text)) => match text.as_str() {
            "straight" => RouteStyle::Straight,
            "right_angle" => RouteStyle::RightAngle,
            _ => {
                return Err(CommandError::invalid_args(
                    "style must be straight or right_angle",
                ));
            }
        },
        Some(_) => {
            return Err(CommandError::invalid_args(
                "style must be straight or right_angle",
            ));
        }
        None => {
            return Err(CommandError::NeedsInput(NeededInput::Text {
                prompt: "connector style (straight or right_angle)".to_owned(),
                default: None,
            }));
        }
    };
    let (session, page) = active_page(ctx)?;
    let targets: Vec<ShapeId> = {
        let doc = &ctx.active_document().ok_or_else(no_document)?.document;
        ctx.selection
            .shapes
            .iter()
            .copied()
            .filter(|shape| doc.page_of(*shape) == Some(page))
            // A 1-D shape that is not routable (a line or a 1-D arrow)
            // takes no style cells (ADR 0084 rule 1, amended).
            .filter(|shape| !doc.is_connector(*shape) || doc.is_routable(*shape))
            .collect()
    };
    if targets.is_empty() {
        return Err(CommandError::failed("no selected shape to style"));
    }
    let route_style = route_style_value(style);
    let mut edit = CompositeEdit::new(if targets.len() == 1 {
        "Set connector style".to_owned()
    } else {
        format!("Set connector style on {} shapes", targets.len())
    });
    {
        let doc = &ctx.active_document().ok_or_else(no_document)?.document;
        for &shape in &targets {
            let step = CellEdit::new(
                doc,
                session,
                "Set connector style",
                SheetRef::Shape(shape),
                "ShapeRouteStyle",
                CellWrite::new(
                    route_style.to_string(),
                    CellValue::Number(f64::from(route_style)),
                ),
            );
            edit.push(Box::new(step?));
        }
    }
    edit.apply(ctx)?;
    let _ = reroute_after_edit(ctx, page);
    Ok(edit_outcome(
        if style == RouteStyle::Straight {
            "Connector style: straight"
        } else {
            "Connector style: right angle"
        },
        Vec::new(),
        Box::new(edit),
    ))
}

/// The `ShapeRouteStyle` value of a [`RouteStyle`] (Microsoft Learn,
/// "ShapeRouteStyle Cell"): 1 = right angle, 2 = straight.
fn route_style_value(style: RouteStyle) -> i32 {
    if style == RouteStyle::RightAngle {
        1
    } else {
        2
    }
}

/// The [`RouteStyle`] a `ShapeRouteStyle` cached value spells (ADR 0084
/// rule 8): 2 is straight lines; 1 (right angle), 16, the directed
/// styles (3-15, 17-22) and unknown values all route as **right
/// angle** in the alpha. Only 2 means straight; the old straight
/// default goes.
fn style_from_value(value: f64) -> RouteStyle {
    if value == 2.0 {
        RouteStyle::Straight
    } else {
        RouteStyle::RightAngle
    }
}

/// The style a connector routes with (ADR 0084 rule 8): its
/// `ShapeRouteStyle` value when it has one, else the page's
/// `RouteStyle`, else right angle. A missing or 0 cell value falls
/// through to the page; a missing page value means right angle.
fn connector_style(doc: &Document, connector: ShapeId) -> RouteStyle {
    if let Some(value) = nonzero_number(doc.resolve(connector, &CellName::new("ShapeRouteStyle"))) {
        return style_from_value(value);
    }
    if let Some(page) = doc.page_of(connector)
        && let Some(value) =
            nonzero_number(doc.resolve_page_cell(page, &CellName::new("RouteStyle")))
    {
        return style_from_value(value);
    }
    RouteStyle::RightAngle
}

/// The style one connector's path takes (ADR 0084 rule 1, amended): a
/// 1-D shape that is not routable stays a straight line from begin to
/// end; a routable one (or a legacy record-only connector) follows
/// [`connector_style`].
fn route_style_for(doc: &Document, connector: ShapeId) -> RouteStyle {
    if doc.is_connector(connector) && !doc.is_routable(connector) {
        RouteStyle::Straight
    } else {
        connector_style(doc, connector)
    }
}

/// The finite non-zero number a resolution carries, or `None` for a
/// missing cell, 0 (page default, ADR 0084 rule 8) or a non-number.
fn nonzero_number(resolved: Result<Resolved<'_>, larrygram_model::Error>) -> Option<f64> {
    match resolved.ok()?.value? {
        CellValue::Number(value) if value.is_finite() && *value != 0.0 => Some(*value),
        _ => None,
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
    let warnings = insert.warnings;

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
        for (end, target, point) in [
            (End::Begin, begin_target, begin_point),
            (End::End, end_target, end_point),
        ] {
            let Some(target_visio_id) = target else {
                continue;
            };
            let label = match end {
                End::Begin => "Glue connector begin",
                End::End => "Glue connector end",
            };
            let mut edit = {
                let doc = &ctx.active_document().ok_or_else(no_document)?.document;
                let target = doc
                    .shape_by_visio_id(page, target_visio_id)
                    .ok_or_else(|| {
                        CommandError::invalid_args(format!(
                            "unknown glue target {target_visio_id} on the active page"
                        ))
                    })?;
                let record = glue_record(connector, target, target_visio_id, point, end, doc)?;
                let mut resolve_warnings = Vec::new();
                let at = end_point_for(
                    doc,
                    page,
                    connector,
                    end,
                    Some(&record),
                    &mut resolve_warnings,
                );
                let staged = GlueEdit::new(
                    doc,
                    session,
                    label,
                    page,
                    connector,
                    connector_visio,
                    end,
                    Some(record),
                    at,
                );
                staged?
            };
            edit.apply(ctx)?;
            edits.push(Box::new(edit));
        }

        let mut path_edit = {
            let doc = &ctx.active_document().ok_or_else(no_document)?.document;
            let transform = page_transform(doc, connector)?;
            let current = geometry_endpoints(doc, connector, transform);
            let fallback = transform_point(transform, loc_pin(doc, connector));
            let begin = resolved_end(
                doc,
                page,
                connector,
                End::Begin,
                begin_free,
                current,
                fallback,
            );
            let start = begin?;
            let resolved =
                resolved_end(doc, page, connector, End::End, end_free, current, fallback);
            let end = resolved?;
            let points = route(
                start,
                end,
                &RouteOptions::new(route_style_for(doc, connector)),
            )
            .map_err(|error| CommandError::failed(format!("cannot route connector: {error}")))?;
            PathEdit::new(doc, session, connector, points)?
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

/// One connector endpoint: its glue target's resolved point when that
/// end is glued, else the given free point (or the connector's current
/// geometry point). Task P5-17 Goal 6 routes both kinds the same way.
fn resolved_end(
    doc: &Document,
    page: PageId,
    connector: ShapeId,
    end: End,
    free: Option<Point>,
    current: Option<(Point, Point)>,
    fallback: Point,
) -> Result<Point, CommandError> {
    let connector_visio = doc
        .shape(connector)
        .ok_or_else(|| CommandError::failed("the connector shape is missing"))?
        .visio_id;
    let connects = doc.connects_of(page, connector).map_err(model_error)?;
    let (index, reference, current_point) = match end {
        End::Begin => (
            0,
            current.map(|(start, _)| start),
            current.map(|(start, _)| start),
        ),
        End::End => (1, current.map(|(_, end)| end), current.map(|(_, end)| end)),
    };
    if let Some(connect) = connects.get(index).and_then(Option::as_ref) {
        let mut lines = Vec::new();
        return resolve_endpoint(
            doc,
            page,
            connect,
            reference.unwrap_or(fallback),
            connector_visio,
            &mut lines,
        )
        .ok_or_else(|| CommandError::failed("cannot resolve a glued connector end"));
    }
    free.or(current_point)
        .ok_or_else(|| CommandError::invalid_args("a free connector end needs its x and y"))
}

/// The resolved point of one end (page inches) with the resolution the
/// re-route uses (task P7-31 keeps the old landing; P7-32 replaces
/// it): the glue target's row point for point glue, the nearest bounds
/// point for dynamic glue, and the end's own current point when it is
/// free. A point that cannot be resolved falls back to that current
/// point (the re-route refreshes the cached `V` afterwards).
fn end_point_for(
    doc: &Document,
    page: PageId,
    connector: ShapeId,
    end: End,
    record: Option<&Connect>,
    warnings: &mut Vec<String>,
) -> Point {
    let current = current_end_point(doc, connector, end);
    let reference = current
        .or_else(|| fallback_point(doc, connector))
        .unwrap_or(Point::new(0.0, 0.0));
    match record {
        Some(connect) => {
            let visio = doc
                .shape(connector)
                .map(|entity| entity.visio_id)
                .unwrap_or_default();
            resolve_endpoint(doc, page, connect, reference, visio, warnings).unwrap_or(reference)
        }
        None => reference,
    }
}

/// The end's current point in page inches: its endpoint cells when it
/// has them, else its geometry path's end.
fn current_end_point(doc: &Document, connector: ShapeId, end: End) -> Option<Point> {
    if let Ok(point) = doc.connector_end(connector, end.connector_end()) {
        return Some(point);
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
fn optional_visio_id(args: &Args, name: &str) -> Result<Option<i32>, CommandError> {
    match args.get(name) {
        None => Ok(None),
        Some(_) => need_integer(args, name, "shape id").map(Some),
    }
}

/// One optional connection row `IX` argument.
fn optional_row(args: &Args, name: &str) -> Result<Option<i32>, CommandError> {
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
        write_path(doc, connector, &points)?;
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
    // its bends. Rows that give fewer than two points become the
    // straight line between the ends.
    let transform = page_transform(doc, shape)?;
    let mut path: Vec<Point> = doc
        .shape_geometry(shape)
        .ok()
        .and_then(|geometry| geometry.paths.into_iter().next())
        .map(|path| {
            path.rows
                .iter()
                .filter_map(row_point)
                .map(|point| transform.apply(point))
                .collect()
        })
        .unwrap_or_default();
    if path.len() < 2 {
        path = vec![begin, end];
    }
    let moved: Vec<Point> = path.into_iter().map(shift).collect();
    write_path(doc, shape, &moved)
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

/// Recomputes every glued connector's path on `page` (ADR 0045).
///
/// The editing commands call this after a move/resize/rotate/delete, and
/// the undo/redo runner after a step. It is idempotent, not undoable, and
/// returns the number of connectors whose path it rewrote. It acts on
/// the active document only. A glue whose
/// target is detached is dropped with one warning line. A missing page or
/// session is `Ok(0)`: a re-route pass never fails an edit that already
/// succeeded.
pub(crate) fn reroute_after_edit(ctx: &mut AppCtx, page: PageId) -> Result<usize, CommandError> {
    // `PageId` is a position, so only the active document can own it: the
    // editing commands and undo/redo all act on the active document.
    let Some(slot) = ctx.active_document_mut() else {
        return Ok(0);
    };
    if slot.document.page(page).is_none() {
        return Ok(0);
    }
    let session = slot.id;
    let mut warnings = Vec::new();
    let count = reroute_document(&mut slot.document, page, &mut warnings)?;
    for line in warnings {
        ctx.record_warning(line);
    }
    ctx.touch_session(session);
    Ok(count)
}

/// Re-routes the active page's glued connectors after an editing
/// command that moved, deleted or glued shapes (ADR 0045 rule 3).
///
/// One pass after the edit, and one warning line when the pass cannot
/// run: the edit itself already succeeded and stays applied (the
/// [`reroute_after_edit`] contract).
pub(crate) fn reroute_quietly(ctx: &mut AppCtx) {
    let page = ctx
        .active_document()
        .and_then(|session| session.active_page);
    if let Some(page) = page {
        reroute_quietly_on(ctx, page);
    }
}

/// One re-route pass on `page`, with one warning line when the pass
/// cannot run (task P5-19 coverage: the one helper and the one spelling
/// of the "connectors were not re-routed" message).
pub(crate) fn reroute_quietly_on(ctx: &mut AppCtx, page: PageId) {
    if let Err(error) = reroute_after_edit(ctx, page) {
        ctx.record_warning(format!("connectors were not re-routed: {error}"));
    }
}

/// The re-route pass over one document.
fn reroute_document(
    doc: &mut Document,
    page: PageId,
    warnings: &mut Vec<String>,
) -> Result<usize, CommandError> {
    let connector_ids = connector_visio_ids(doc, page)?;
    let mut routed = 0usize;
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
        // ADR 0056: a connector whose geometry rows carry formulas is
        // owned by recalc (a master-driven dynamic connector); the
        // router never writes its rows.
        if crate::recalc::geometry_is_formula_driven(doc, connector) {
            continue;
        }
        let transform = match page_transform(doc, connector) {
            Ok(transform) => transform,
            Err(error) => {
                warnings.push(format!(
                    "connector {visio_id} has no usable transform: {error}"
                ));
                continue;
            }
        };
        let mut references = [None, None];
        for (slot, end) in ends.iter().enumerate() {
            let Some(connect) = end else {
                continue;
            };
            let Some(target) = doc.shape_by_visio_id(page, connect.to_sheet) else {
                continue;
            };
            let Some(rect) = page_bounds(doc, target) else {
                continue;
            };
            let center = rect_center(rect);
            if let Some(other) = references.get_mut(1 - slot) {
                *other = Some(center);
            }
        }
        let pin = transform_point(transform, loc_pin(doc, connector));
        let current = geometry_endpoints(doc, connector, transform);
        let begin_ref = references
            .first()
            .copied()
            .flatten()
            .or(current.map(|(start, _)| start))
            .unwrap_or(pin);
        let end_ref = references
            .get(1)
            .copied()
            .flatten()
            .or(current.map(|(_, end)| end))
            .unwrap_or(pin);
        let begin = match ends.first().and_then(Option::as_ref) {
            Some(connect) => resolve_endpoint(doc, page, connect, begin_ref, visio_id, warnings),
            None => current.map(|(start, _)| start).or(Some(pin)),
        };
        let end = match ends.get(1).and_then(Option::as_ref) {
            Some(connect) => resolve_endpoint(doc, page, connect, end_ref, visio_id, warnings),
            None => current.map(|(_, end)| end).or(Some(pin)),
        };
        let (Some(start), Some(end)) = (begin, end) else {
            continue;
        };
        let style = route_style_for(doc, connector);
        let points = route(start, end, &RouteOptions::new(style)).map_err(|error| {
            CommandError::failed(format!("cannot route connector {visio_id}: {error}"))
        })?;
        write_path(doc, connector, &points)?;
        routed = routed.saturating_add(1);
    }
    Ok(routed)
}

/// Whether one `Connect` record names a **connector end** (`VisFromParts`
/// begin/end parts, ADR 0045), as opposed to a shape-to-shape glue such
/// as the glue-on-drop of ADR 0046 (`Connections.X*` from a connection
/// point): only connector ends are routed (task P5-11).
fn is_connector_end(connect: &Connect) -> bool {
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
            .and_then(|shape| {
                current_end_point(doc, shape, end).or_else(|| fallback_point(doc, shape))
            })
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

/// One connector endpoint in page inches: from its glue target, or the
/// nearest point of the target's bounds.
fn resolve_endpoint(
    doc: &Document,
    page: PageId,
    connect: &Connect,
    reference: Point,
    connector_visio_id: i32,
    warnings: &mut Vec<String>,
) -> Option<Point> {
    let target = doc.shape_by_visio_id(page, connect.to_sheet)?;
    let transform = page_transform(doc, target).ok()?;
    if connect.to_part >= mutate::VIS_CONNECTION_POINT
        || connect.to_cell.starts_with("Connections.")
    {
        let ix = connection_index(connect);
        if let Some(local) = ix.and_then(|ix| {
            let ix = i32::try_from(ix).ok()?;
            doc.connection_points_indexed(target)
                .into_iter()
                .find(|(row, _)| *row == ix)
                .map(|(_, point)| point)
        }) {
            let point = transform_point(transform, local);
            if point.x.is_finite() && point.y.is_finite() {
                return Some(point);
            }
        }
        warnings.push(format!(
            "connector {connector_visio_id} has no connection point {}; using the outline",
            ix.map_or(-1, |ix| ix as i32)
        ));
    }
    let bounds = page_bounds(doc, target)?;
    Some(nearest_on_rect(bounds, reference))
}

/// The `Connection` row index of a record (`VisToParts` = 100 + IX, or
/// the `Connections.Xn` cell with a one-based `n`).
fn connection_index(connect: &Connect) -> Option<usize> {
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
    for id in chain.iter().rev() {
        let local = local_transform(&mut reader, *id)?;
        transform = transform.then(local);
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

/// The page-space bounds of a shape's local box.
fn page_bounds(doc: &Document, shape: ShapeId) -> Option<Rect> {
    let entity = doc.shape(shape)?;
    let mut reader = CellReader::new(doc);
    let sheet = SheetRef::Shape(shape);
    let width = reader.number(sheet, &CellName::new("Width")).unwrap_or(0.0);
    let height = reader
        .number(sheet, &CellName::new("Height"))
        .unwrap_or(0.0);
    let transform = page_transform(doc, shape).ok()?;
    let corners = [
        Point::new(0.0, 0.0),
        Point::new(width, 0.0),
        Point::new(width, height),
        Point::new(0.0, height),
    ];
    let mut min = Point::new(f64::INFINITY, f64::INFINITY);
    let mut max = Point::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
    for corner in corners {
        let point = transform_point(transform, corner);
        if !point.x.is_finite() || !point.y.is_finite() {
            return None;
        }
        min = Point::new(min.x.min(point.x), min.y.min(point.y));
        max = Point::new(max.x.max(point.x), max.y.max(point.y));
    }
    let _ = entity;
    Some(Rect::new(min, max))
}

/// Maps a local point, keeping only finite results callers can use.
fn transform_point(transform: Transform, point: Point) -> Point {
    transform.apply(point)
}

/// The centre of a rectangle.
fn rect_center(rect: Rect) -> Point {
    Point::new(
        (rect.min.x + rect.max.x) / 2.0,
        (rect.min.y + rect.max.y) / 2.0,
    )
}

/// The nearest point on a rectangle's outline to `reference`.
fn nearest_on_rect(rect: Rect, reference: Point) -> Point {
    let inside = rect.contains(reference);
    if !inside {
        return Point::new(
            reference.x.clamp(rect.min.x, rect.max.x),
            reference.y.clamp(rect.min.y, rect.max.y),
        );
    }
    let left = reference.x - rect.min.x;
    let right = rect.max.x - reference.x;
    let bottom = reference.y - rect.min.y;
    let top = rect.max.y - reference.y;
    let smallest = left.min(right).min(bottom).min(top);
    if smallest == left {
        Point::new(rect.min.x, reference.y)
    } else if smallest == right {
        Point::new(rect.max.x, reference.y)
    } else if smallest == bottom {
        Point::new(reference.x, rect.min.y)
    } else {
        Point::new(reference.x, rect.max.y)
    }
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
    let parent = match doc
        .shape(connector)
        .ok_or_else(|| CommandError::failed("the connector is gone"))?
        .parent
    {
        Some(parent) => page_transform(doc, parent)?,
        None => Transform::identity(),
    };
    let inverse = parent.invert().map_err(|_| singular())?;
    let local: Vec<Point> = points.iter().map(|point| inverse.apply(*point)).collect();
    doc.write_connector_path(connector, &local)
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
    fn the_style_values_match_the_shapesheet_reference() {
        assert_eq!(route_style_value(RouteStyle::RightAngle), 1);
        assert_eq!(route_style_value(RouteStyle::Straight), 2);
        assert_eq!(style_from_value(1.0), RouteStyle::RightAngle);
        assert_eq!(style_from_value(2.0), RouteStyle::Straight);
        // ADR 0084 rule 8: right angle everywhere the old default was
        // straight (0, 16, the directed styles and unknown values).
        assert_eq!(style_from_value(0.0), RouteStyle::RightAngle);
        assert_eq!(style_from_value(f64::NAN), RouteStyle::RightAngle);
        assert_eq!(style_from_value(16.0), RouteStyle::RightAngle);
        assert_eq!(style_from_value(3.0), RouteStyle::RightAngle);
        assert_eq!(style_from_value(22.0), RouteStyle::RightAngle);
    }

    #[test]
    fn the_style_falls_through_to_the_page_and_to_right_angle() {
        let (mut ctx, session, page, connector, _left, _right) = fixture();
        // No local style and no page value: right angle (ADR 0084 rule 8).
        let doc = ctx.active_document().expect("session").document.clone();
        assert_eq!(connector_style(&doc, connector), RouteStyle::RightAngle);
        // A missing or 0 cell value falls through to the page's
        // `RouteStyle`.
        let doc = document_mut(&mut ctx, session).expect("session");
        doc.write_cell(
            SheetRef::Page(page),
            &CellName::new("RouteStyle"),
            &CellWrite::new("2", CellValue::Number(2.0)),
        )
        .expect("page style");
        let doc = ctx.active_document().expect("session").document.clone();
        assert_eq!(connector_style(&doc, connector), RouteStyle::Straight);
        // A local value of 16 (center-to-centre) routes as right angle.
        let doc = document_mut(&mut ctx, session).expect("session");
        doc.write_cell(
            SheetRef::Shape(connector),
            &CellName::new("ShapeRouteStyle"),
            &CellWrite::new("16", CellValue::Number(16.0)),
        )
        .expect("local style");
        let doc = ctx.active_document().expect("session").document.clone();
        assert_eq!(connector_style(&doc, connector), RouteStyle::RightAngle);
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
        assert_eq!(before.last(), Some(&Point::new(4.0, 2.0)));

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
        assert_eq!(points.len(), 3, "{points:?}");
        assert_eq!(points.first(), Some(&Point::new(0.0, 1.5)));
        assert_eq!(points.last(), Some(&Point::new(4.0, 2.0)));
        dispatch(&mut ctx, "edit.undo", Args::new()).expect("undo style");
        // The undo restores no local style; the default routes right
        // angle too (ADR 0084 rule 8), so the same elbow comes back.
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
        let (mut ctx, session, page, _connector, _left, _right) = fixture();
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
        assert_eq!(reroute_after_edit(&mut ctx, page).expect("reroute"), 0);
        assert_eq!(ctx.documents.get(first).expect("first").changes, before);

        ctx.active = Some(first);
        assert_eq!(reroute_after_edit(&mut ctx, page).expect("reroute"), 1);
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
            Args::new().with("style", ArgValue::Text("curved".to_owned())),
        )
        .expect_err("bad style");
        assert!(
            bad_style.to_string().contains("straight or right_angle"),
            "{bad_style}"
        );

        let no_selection = dispatch(
            &mut ctx,
            "connector.style",
            Args::new().with("style", ArgValue::Text("straight".to_owned())),
        )
        .expect_err("no selection");
        assert!(
            no_selection.to_string().contains("no selected shape"),
            "{no_selection}"
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
    fn nearest_point_handles_inside_and_outside_references() {
        let rect = Rect::new(Point::new(0.0, 0.0), Point::new(4.0, 2.0));
        assert_eq!(
            nearest_on_rect(rect, Point::new(-1.0, 1.0)),
            Point::new(0.0, 1.0)
        );
        assert_eq!(
            nearest_on_rect(rect, Point::new(1.0, -3.0)),
            Point::new(1.0, 0.0)
        );
        assert_eq!(
            nearest_on_rect(rect, Point::new(0.5, 1.0)),
            Point::new(0.0, 1.0)
        );
        assert_eq!(
            nearest_on_rect(rect, Point::new(3.5, 1.0)),
            Point::new(4.0, 1.0)
        );
        assert_eq!(
            nearest_on_rect(rect, Point::new(2.0, 0.25)),
            Point::new(2.0, 0.0)
        );
        assert_eq!(
            nearest_on_rect(rect, Point::new(2.0, 1.75)),
            Point::new(2.0, 2.0)
        );
        assert_eq!(rect_center(rect), Point::new(2.0, 1.0));
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
        let count = reroute_after_edit(&mut ctx, page).expect("reroute");
        assert_eq!(count, 0);
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
        assert_eq!(reroute_after_edit(&mut ctx, PageId { index: 0 }), Ok(0));
        let (mut ctx, _session, page, _connector, _left, _right) = fixture();
        assert_eq!(reroute_after_edit(&mut ctx, page), Ok(0));
        assert_eq!(reroute_after_edit(&mut ctx, PageId { index: 9 }), Ok(0));
    }

    #[test]
    fn a_missing_connection_point_falls_back_to_the_outline() {
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
        reroute_after_edit(&mut ctx, page).expect("reroute");
        assert!(
            ctx.warnings
                .iter()
                .any(|line| line.contains("has no connection point 8")),
            "{:?}",
            ctx.warnings
        );
        assert_eq!(
            path_points(&ctx, connector).first(),
            Some(&Point::new(2.0, 2.0))
        );
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
        assert_eq!(
            reroute_after_edit(&mut ctx, PageId { index: 0 }).expect("no document"),
            0
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
        reroute_document(&mut doc, page, &mut warnings).expect("pass");
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
}
