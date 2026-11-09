//! The connection-point commands (task P7-34).
//!
//! Points are `Connection` rows (ADR 0084 rule 7), edited through the
//! model's write API. Every point edit is **one** undoable edit and
//! re-routes the connectors glued to the shape (P7-32's hook):
//!
//! - `connector.point_add` (`shape`, `x`, `y`, local inches),
//! - `connector.point_move` (`shape`, `point`, `x`, `y`): the ends
//!   glued to the point follow it,
//! - `connector.point_delete` (`shape`, `point`): the ends glued to the
//!   deleted point re-glue dynamically (the formulas and `Connects`
//!   records ADR 0084 rule 2 stores), and the records and `PAR(PNT(…))`
//!   formulas of the later rows move down by one, so no end changes
//!   target,
//! - `edit.connection_point.inward` / `.outward` /
//!   `.inward_and_outward` (`shape`, `point`): the row's `Type` (0, 1,
//!   2).
//!
//! The tool and the checks:
//!
//! - `tools.connection.point` arms the Connection Point tool,
//! - `tools.connection.points` (Panel Modern) and `view.connections`
//!   (Panel Classic) flip the one per-window check state
//!   ([`AppCtx::show_connection_points`]).
//!
//! The command arguments name the shape by its Visio `ID` and the
//! point by its zero-based row index; every command applies directly
//! with its arguments (no dialog, ADR 0028).

use larrygram_geom::Point;
use larrygram_model::{
    CellName, CellValue, CellWrite, Connect, Document, EndGlue, PageId, ShapeId, Sheet, SheetRef,
    mutate,
};
use larrygram_shapesheet::CellAddress;

use crate::app::{ActiveTool, AppCtx, SessionId};
use crate::error::CommandError;
use crate::undo::Edit;
use crate::{Args, Invocation, Outcome};

use super::connector::{
    End, active_page, connection_index, is_connector_end, need_integer, reroute_quietly,
    stored_end_point, with_routes,
};
use super::edit::{PageKey, document_mut, page_key, resolve_page};
use super::{
    CommandTable, edit_outcome, has_document, has_pages, model_error, need_finite, no_document,
    outcome,
};

/// The implemented connection-point commands (task P7-34).
pub(crate) const TABLE: CommandTable = &[
    (
        "tools.connection.point",
        "Connection Point",
        has_document,
        run_point_tool,
    ),
    (
        "tools.connection.points",
        "Connection Points",
        super::always,
        run_points_check,
    ),
    (
        "view.connections",
        "Connections",
        super::always,
        run_points_check,
    ),
    (
        "connector.point_add",
        "Add Connection Point",
        has_pages,
        run_point_add,
    ),
    (
        "connector.point_move",
        "Move Connection Point",
        has_pages,
        run_point_move,
    ),
    (
        "connector.point_delete",
        "Delete Connection Point",
        has_pages,
        run_point_delete,
    ),
    (
        "edit.connection_point.inward",
        "Inward",
        has_pages,
        run_type_inward,
    ),
    (
        "edit.connection_point.outward",
        "Outward",
        has_pages,
        run_type_outward,
    ),
    (
        "edit.connection_point.inward_and_outward",
        "Inward & Outward",
        has_pages,
        run_type_both,
    ),
];

/// `tools.connection.point`: arms the Connection Point tool (task
/// P7-34 item 2). A tool choice is no edit (ADR 0040); a recorded
/// Format Painter copy never outlives it (task P7-26 item 4).
fn run_point_tool(ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
    if ctx.active_tool != ActiveTool::ConnectionPoint {
        ctx.format_painter = None;
    }
    ctx.active_tool = ActiveTool::ConnectionPoint;
    Ok(outcome("Connection Point tool", Vec::new()))
}

/// `tools.connection.points` and `view.connections`: flip the one
/// per-window connection-points check (task P7-34 item 1). Both ids
/// read and write the same state.
fn run_points_check(ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
    ctx.show_connection_points = !ctx.show_connection_points;
    Ok(outcome(
        format!(
            "Connection points {}",
            on_off_word(ctx.show_connection_points)
        ),
        Vec::new(),
    ))
}

/// `on` or `off` for a boolean check.
fn on_off_word(on: bool) -> &'static str {
    if on { "on" } else { "off" }
}

/// `connector.point_add` (task P7-34 item 2 and 5).
fn run_point_add(ctx: &mut AppCtx, inv: &Invocation) -> Result<Outcome, CommandError> {
    let (session, page) = active_page(ctx)?;
    let shape = point_shape(ctx, page, &inv.args)?;
    let x = need_finite(&inv.args, "x", "the connection point's x (inches)")?;
    let y = need_finite(&inv.args, "y", "the connection point's y (inches)")?;
    let mut edit = PointEdit::stage(ctx, session, page, shape, "Add connection point", &[])?;
    {
        let doc = document_mut(ctx, session)?;
        doc.add_connection_point(shape, Point::new(x, y))
            .map_err(model_error)?;
    }
    edit.finish(ctx);
    let routes = reroute_quietly(ctx, &[shape]);
    Ok(edit_outcome(
        "Connection point added",
        Vec::new(),
        with_routes(Box::new(edit), routes),
    ))
}

/// `connector.point_move` (task P7-34 item 4): the ends glued to the
/// point follow it through the re-route pass.
fn run_point_move(ctx: &mut AppCtx, inv: &Invocation) -> Result<Outcome, CommandError> {
    let (session, page) = active_page(ctx)?;
    let shape = point_shape(ctx, page, &inv.args)?;
    let ix = point_index(&inv.args)?;
    let x = need_finite(&inv.args, "x", "the connection point's x (inches)")?;
    let y = need_finite(&inv.args, "y", "the connection point's y (inches)")?;
    let mut edit = PointEdit::stage(ctx, session, page, shape, "Move connection point", &[])?;
    {
        let doc = document_mut(ctx, session)?;
        doc.move_connection_point(shape, ix, Point::new(x, y))
            .map_err(model_error)?;
    }
    edit.finish(ctx);
    let routes = reroute_quietly(ctx, &[shape]);
    Ok(edit_outcome(
        "Connection point moved",
        Vec::new(),
        with_routes(Box::new(edit), routes),
    ))
}

/// `connector.point_delete` (task P7-34 item 4): the ends glued to the
/// deleted point re-glue dynamically, and the records and formulas of
/// the later rows move down by one. All of it lands in one edit.
fn run_point_delete(ctx: &mut AppCtx, inv: &Invocation) -> Result<Outcome, CommandError> {
    let (session, page) = active_page(ctx)?;
    let shape = point_shape(ctx, page, &inv.args)?;
    let ix = point_index(&inv.args)?;
    // The glue state before the delete: which connector names the
    // target shape (the snapshot's connector sheets).
    let (target_visio, touched) = {
        let doc = &ctx.active_document().ok_or_else(no_document)?.document;
        let target_visio = doc
            .shape(shape)
            .map(|entity| entity.visio_id)
            .unwrap_or_default();
        glue_ends(doc, page, target_visio)
    };
    let mut edit = PointEdit::stage(
        ctx,
        session,
        page,
        shape,
        "Delete connection point",
        &touched,
    )?;
    {
        let doc = document_mut(ctx, session)?;
        doc.delete_connection_point(shape, ix)
            .map_err(model_error)?;
        renumber_and_reglue(doc, page, target_visio, ix)?;
    }
    edit.finish(ctx);
    let routes = reroute_quietly(ctx, &[shape]);
    Ok(edit_outcome(
        "Connection point deleted",
        Vec::new(),
        with_routes(Box::new(edit), routes),
    ))
}

/// `edit.connection_point.inward` (task P7-34 item 3).
fn run_type_inward(ctx: &mut AppCtx, inv: &Invocation) -> Result<Outcome, CommandError> {
    run_set_type(ctx, inv, 0, "Inward")
}

/// `edit.connection_point.outward` (task P7-34 item 3).
fn run_type_outward(ctx: &mut AppCtx, inv: &Invocation) -> Result<Outcome, CommandError> {
    run_set_type(ctx, inv, 1, "Outward")
}

/// `edit.connection_point.inward_and_outward` (task P7-34 item 3).
fn run_type_both(ctx: &mut AppCtx, inv: &Invocation) -> Result<Outcome, CommandError> {
    run_set_type(ctx, inv, 2, "Inward & Outward")
}

/// The shared `edit.connection_point.*` handler: writes the row's
/// `Type` and re-routes the glued connectors.
fn run_set_type(
    ctx: &mut AppCtx,
    inv: &Invocation,
    kind: u8,
    name: &str,
) -> Result<Outcome, CommandError> {
    let (session, page) = active_page(ctx)?;
    let shape = point_shape(ctx, page, &inv.args)?;
    let ix = point_index(&inv.args)?;
    let mut edit = PointEdit::stage(ctx, session, page, shape, "Connection point type", &[])?;
    {
        let doc = document_mut(ctx, session)?;
        doc.set_connection_point_type(shape, ix, kind)
            .map_err(model_error)?;
    }
    edit.finish(ctx);
    let routes = reroute_quietly(ctx, &[shape]);
    Ok(edit_outcome(
        format!("Connection point: {name}"),
        Vec::new(),
        with_routes(Box::new(edit), routes),
    ))
}

/// The target shape of a point command: the `shape` argument is its
/// Visio `ID` on the active page.
fn point_shape(ctx: &AppCtx, page: PageId, args: &Args) -> Result<ShapeId, CommandError> {
    let session = ctx.active_document().ok_or_else(no_document)?;
    let visio_id = need_integer(args, "shape", "shape id")?;
    session
        .document
        .shape_by_visio_id(page, visio_id)
        .ok_or_else(|| {
            CommandError::invalid_args(format!("unknown shape id {visio_id} on the active page"))
        })
}

/// The `point` argument: the row's zero-based index.
fn point_index(args: &Args) -> Result<u32, CommandError> {
    let ix = need_integer(args, "point", "connection point row index")?;
    u32::try_from(ix).map_err(|_| CommandError::invalid_args("point must not be negative"))
}

/// Every connector end whose record names `target_visio`, in file
/// order, and the connector shapes to snapshot (task P7-34 item 4). A
/// dangling record (no connector shape) names no shape to snapshot.
fn glue_ends(doc: &Document, page: PageId, target_visio: i32) -> (i32, Vec<ShapeId>) {
    let Some(entity) = doc.page(page) else {
        return (target_visio, Vec::new());
    };
    let mut touched: Vec<ShapeId> = Vec::new();
    for connect in &entity.connects {
        if connect.to_sheet != target_visio || !is_connector_end(connect) {
            continue;
        }
        if let Some(connector) = doc.shape_by_visio_id(page, connect.from_sheet)
            && !touched.contains(&connector)
        {
            touched.push(connector);
        }
    }
    (target_visio, touched)
}

/// The delete's glue work (task P7-34 item 4), in the same edit as the
/// row delete:
///
/// - an end glued to the deleted row re-glues dynamically: its
///   endpoint cells take the `_WALKGLUE(…)` formula and its record the
///   target's `PinX` (ADR 0084 rule 2), keeping the end's stored point
///   until the re-route re-resolves it;
/// - a record naming `Connections.X<k+1>` and up, and the `PAR(PNT(…))`
///   formula of that end, move down by one (`k` is the deleted row's
///   one-based number), so no end changes target.
fn renumber_and_reglue(
    doc: &mut Document,
    page: PageId,
    target_visio: i32,
    deleted_ix: u32,
) -> Result<(), CommandError> {
    // Phase 1 (read-only): plan the record rewrites and the cell
    // writes, one entry per record that changes.
    struct Plan {
        position: usize,
        record: Connect,
        connector: Option<ShapeId>,
        end: End,
        /// `Some` for the deleted row's end: re-glue dynamically to
        /// this kind; `None` for a row number that moves down.
        glue: Option<EndGlue>,
    }
    let mut plans: Vec<Plan> = Vec::new();
    {
        let Some(entity) = doc.page(page) else {
            return Ok(());
        };
        for (position, connect) in entity.connects.iter().enumerate() {
            if connect.to_sheet != target_visio || !is_connector_end(connect) {
                continue;
            }
            let Some(ix) = connection_index(connect) else {
                continue;
            };
            let end = End::of(connect);
            if ix == deleted_ix as usize {
                // Re-glue dynamically: the record names the target's
                // pin (ADR 0084 rule 2's table).
                let mut dynamic = connect.clone();
                dynamic.to_cell = String::from("PinX");
                dynamic.to_part = mutate::VIS_WHOLE_SHAPE;
                plans.push(Plan {
                    position,
                    record: dynamic,
                    connector: doc.shape_by_visio_id(page, connect.from_sheet),
                    end,
                    glue: Some(EndGlue::Dynamic {
                        sheet: target_visio,
                    }),
                });
            } else if ix > deleted_ix as usize {
                // Renumber: the row number moves down by one. The
                // `ToCell` spelling is the one-based row number, so the
                // old zero-based `ix` is exactly the new number.
                let mut moved = connect.clone();
                moved.to_part = mutate::VIS_CONNECTION_POINT
                    + i32::try_from(ix - 1)
                        .map_err(|_| CommandError::failed("connection row index too large"))?;
                moved.to_cell = format!("Connections.X{ix}");
                plans.push(Plan {
                    position,
                    record: moved,
                    connector: doc.shape_by_visio_id(page, connect.from_sheet),
                    end,
                    glue: None,
                });
            }
        }
    }
    // Phase 2: the records, rewritten in place so the list order and
    // every untouched record survive.
    {
        let Some(entity) = doc.pages.get_mut(page.index as usize) else {
            return Ok(());
        };
        for plan in &plans {
            if let Some(slot) = entity.connects.get_mut(plan.position) {
                *slot = plan.record.clone();
            }
        }
    }
    // Phase 3: the endpoint cells. A dangling record (no connector
    // shape) has nothing to write; a legacy record-only connector is
    // not migrated (its records are its only glue).
    let deleted_row = deleted_ix
        .checked_add(1)
        .ok_or_else(|| CommandError::invalid_args("connection point is too large"))?;
    for plan in plans {
        let Some(connector) = plan.connector else {
            continue;
        };
        if !doc.is_connector(connector) {
            continue;
        }
        match plan.glue {
            Some(glue) => {
                let at = stored_end_point(doc, connector, plan.end);
                doc.write_connector_end(connector, plan.end.connector_end(), at, glue)
                    .map_err(model_error)?;
            }
            None => renumber_end_formulas(doc, connector, plan.end, deleted_row)?,
        }
    }
    Ok(())
}

/// The two endpoint cells of one end (the `PAR(PNT(…))` formulas).
fn end_formula_cells(end: End) -> [&'static str; 2] {
    match end {
        End::Begin => ["BeginX", "BeginY"],
        End::End => ["EndX", "EndY"],
    }
}

/// Rewrites the `PAR(PNT(…))` formulas of one end: every
/// `Connections.X<n>` / `Connections.Y<n>` row number above the deleted
/// row's moves down by one (task P7-34 item 4). The cached values stay.
fn renumber_end_formulas(
    doc: &mut Document,
    connector: ShapeId,
    end: End,
    deleted_row: u32,
) -> Result<(), CommandError> {
    for name in end_formula_cells(end) {
        let current = {
            let Some(entity) = doc.shape(connector) else {
                continue;
            };
            let Some(cell) = entity
                .sheet
                .cells
                .iter()
                .find(|cell| cell.name.as_str() == name)
            else {
                continue;
            };
            (cell.formula.clone(), cell.value.clone())
        };
        let (Some(formula), value) = current else {
            continue;
        };
        let Some(renumbered) = renumber_formula(&formula, deleted_row) else {
            continue;
        };
        let write = CellWrite::new(renumbered, value.unwrap_or(CellValue::Number(0.0)));
        doc.write_cell(SheetRef::Shape(connector), &CellName::new(name), &write)
            .map_err(model_error)?;
    }
    Ok(())
}

/// The formula with every `Connections.X<n>` / `Connections.Y<n>` row
/// number above `deleted_row` moved down by one, or `None` when no
/// number changes (so no cell write lands).
fn renumber_formula(formula: &str, deleted_row: u32) -> Option<String> {
    let mut out = String::with_capacity(formula.len());
    let bytes = formula.as_bytes();
    let mut index = 0usize;
    let mut changed = false;
    while index < bytes.len() {
        // Match "connections.x" / "connections.y" (ASCII case-blind).
        let rest = &formula[index..];
        let lowered = rest.get(..13).map(str::to_ascii_lowercase);
        let matches = matches!(
            lowered.as_deref(),
            Some("connections.x") | Some("connections.y")
        );
        if !matches {
            let ch = formula[index..].chars().next().unwrap_or('\0');
            out.push(ch);
            index += ch.len_utf8();
            continue;
        }
        out.push_str(&formula[index..index + 12]);
        out.push(formula[index + 12..].chars().next().unwrap_or('\0'));
        index += 13;
        let start = index;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        let digits = &formula[start..index];
        match digits.parse::<u32>() {
            Ok(number) if number > deleted_row => {
                out.push_str(&(number - 1).to_string());
                changed = true;
            }
            _ => out.push_str(digits),
        }
    }
    changed.then_some(out)
}

/// One undoable point edit (task P7-34 item 4): the model's row change
/// plus any glue rewrite of the same command, in one step. `apply` and
/// `revert` install the recorded sheets and `Connects` records
/// wholesale, so one undo restores the exact rows (the test contract).
#[derive(Debug)]
struct PointEdit {
    session: SessionId,
    label: String,
    /// The page as an identity (`PageId` is a list position).
    page: PageKey,
    /// The shapes whose sheets the edit may rewrite.
    shapes: Vec<ShapeId>,
    /// The state before the edit.
    before: Snapshot,
    /// The state after the edit (filled by [`PointEdit::finish`]).
    after: Snapshot,
    /// The endpoint and trigger cells the glue rewrites touch (the
    /// recalc dirty set, ADR 0056).
    dirty: Vec<CellAddress>,
}

/// The sheets and records one side of a [`PointEdit`] restores.
#[derive(Debug, Default)]
struct Snapshot {
    /// The recorded sheets, by shape.
    sheets: Vec<(ShapeId, Sheet)>,
    /// The page's `Connects` records.
    connects: Vec<Connect>,
}

impl Snapshot {
    /// Records the current state of `shapes`' sheets and the page's
    /// records.
    fn capture(doc: &Document, page: PageId, shapes: &[ShapeId]) -> Self {
        let sheets = shapes
            .iter()
            .filter_map(|shape| Some((*shape, doc.shape(*shape)?.sheet.clone())))
            .collect();
        let connects = doc
            .page(page)
            .map(|entity| entity.connects.clone())
            .unwrap_or_default();
        Self { sheets, connects }
    }
}

impl PointEdit {
    /// Captures the state before the edit; `shapes` are the target and
    /// every connector whose glue the command may rewrite.
    fn stage(
        ctx: &AppCtx,
        session: SessionId,
        page: PageId,
        shape: ShapeId,
        label: &str,
        connectors: &[ShapeId],
    ) -> Result<Self, CommandError> {
        let session_doc = ctx.active_document().ok_or_else(no_document)?;
        let doc = &session_doc.document;
        let mut shapes = Vec::with_capacity(connectors.len().saturating_add(1));
        shapes.push(shape);
        for connector in connectors {
            if !shapes.contains(connector) {
                shapes.push(*connector);
            }
        }
        let before = Snapshot::capture(doc, page, &shapes);
        let mut dirty = Vec::new();
        for connector in connectors {
            for name in [
                "BeginX",
                "BeginY",
                "BegTrigger",
                "EndX",
                "EndY",
                "EndTrigger",
            ] {
                dirty.push(CellAddress::new(
                    SheetRef::Shape(*connector),
                    CellName::new(name),
                ));
            }
        }
        Ok(Self {
            session,
            label: label.to_owned(),
            page: page_key(doc, page)?,
            shapes,
            before,
            after: Snapshot::default(),
            dirty,
        })
    }

    /// Captures the state after the edit (called once the command's
    /// model and glue writes have run).
    fn finish(&mut self, ctx: &AppCtx) {
        if let Some(doc) = ctx.active_document().map(|session| &session.document) {
            self.after = Snapshot::capture(doc, resolve_page_static(doc, &self.page), &self.shapes);
        }
    }

    /// Installs one side of the edit.
    fn install(&self, ctx: &mut AppCtx, snapshot: &Snapshot) -> Result<(), CommandError> {
        let session = self.session;
        let doc = document_mut(ctx, session)?;
        let fallback = PageId { index: 0 };
        let page = resolve_page(doc, &self.page, fallback);
        if let Some(entity) = doc.pages.get_mut(page.index as usize) {
            entity.connects = snapshot.connects.clone();
        }
        for (shape, sheet) in &snapshot.sheets {
            if let Some(entity) = doc.shape_mut(*shape) {
                entity.sheet = sheet.clone();
            }
        }
        ctx.touch_session(session);
        Ok(())
    }
}

/// The live page of a staged edit (`PageId` is a list position).
fn resolve_page_static(doc: &Document, key: &PageKey) -> PageId {
    resolve_page(doc, key, PageId { index: 0 })
}

impl Edit for PointEdit {
    fn label(&self) -> &str {
        &self.label
    }

    fn dirty_cells(&self) -> &[CellAddress] {
        &self.dirty
    }

    fn apply(&mut self, ctx: &mut AppCtx) -> Result<(), CommandError> {
        let after = Snapshot {
            sheets: self.after.sheets.clone(),
            connects: self.after.connects.clone(),
        };
        self.install(ctx, &after)
    }

    fn revert(&mut self, ctx: &mut AppCtx) -> Result<(), CommandError> {
        let before = Snapshot {
            sheets: self.before.sheets.clone(),
            connects: self.before.connects.clone(),
        };
        self.install(ctx, &before)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use larrygram_model::{Cell, Preserved, Shape};

    fn cell(name: &str, value: Option<f64>, formula: Option<&str>) -> Cell {
        Cell {
            name: CellName::new(name),
            formula: formula.map(str::to_owned),
            value: value.map(CellValue::Number),
            unit: larrygram_geom::Unit::Inches,
            preserved: Preserved::new(),
        }
    }

    /// A connector (1-D) shape: the endpoint cells, `BeginX` optional.
    fn connector_at(begin_x: Option<Cell>) -> Shape {
        let mut shape = Shape::new(ShapeId { index: 0 }, 0);
        let mut cells = vec![
            cell("PinX", Some(1.0), None),
            cell("PinY", Some(1.0), None),
            cell("Width", Some(2.0), None),
            cell("Height", Some(0.0), None),
            cell("EndX", Some(2.0), None),
            cell("EndY", Some(0.0), None),
            cell("BeginY", Some(0.0), None),
        ];
        if let Some(begin_x) = begin_x {
            cells.push(begin_x);
        }
        shape.sheet.cells = cells;
        shape
    }

    /// A target and one connector, no records yet. `insert_shape`
    /// assigns the Visio ids, so the tests read them back.
    fn fixture(begin_x: Option<Cell>) -> (Document, PageId, ShapeId, ShapeId) {
        let mut doc = Document::new();
        let page = doc.add_page("Page-1").expect("page");
        let mut target = Shape::new(ShapeId { index: 0 }, 0);
        target.sheet.cells = vec![
            cell("PinX", Some(5.0), None),
            cell("PinY", Some(1.0), None),
            cell("Width", Some(1.0), None),
            cell("Height", Some(1.0), None),
        ];
        let target = doc.insert_shape(page, target, None).expect("target");
        let connector = doc
            .insert_shape(page, connector_at(begin_x), None)
            .expect("connector");
        (doc, page, target, connector)
    }

    /// One `Connect` record for one connector end naming the target's
    /// connection row `ix` (the `Connections.X<ix+1>` spelling).
    fn record(connector_visio: i32, from_cell: &str, target_visio: i32, ix: usize) -> Connect {
        let mut connect = Connect::new();
        connect.from_sheet = connector_visio;
        connect.from_cell = from_cell.to_owned();
        connect.from_part = if from_cell.starts_with("End") {
            mutate::VIS_END
        } else {
            mutate::VIS_BEGIN
        };
        connect.to_sheet = target_visio;
        connect.to_cell = format!("Connections.X{}", ix + 1);
        connect.to_part = mutate::VIS_CONNECTION_POINT + i32::try_from(ix).unwrap_or(0);
        connect
    }

    fn visio_of(doc: &Document, shape: ShapeId) -> i32 {
        doc.shape(shape).expect("shape").visio_id
    }

    /// A gone page and a dangling record leave the file alone (the
    /// stale-page and stale-shape guards).
    #[test]
    fn renumber_and_reglue_skips_what_it_cannot_see() {
        let (mut doc, page, target, connector) = fixture(None);
        let (target_visio, connector_visio) = (visio_of(&doc, target), visio_of(&doc, connector));
        doc.add_connect(page, record(connector_visio, "BeginX", target_visio, 1))
            .expect("record");
        renumber_and_reglue(&mut doc, PageId { index: 9 }, target_visio, 0)
            .expect("a gone page is a no-op");
        assert_eq!(
            doc.page(page).expect("page").connects[0].to_cell,
            "Connections.X2",
            "a gone page rewrites nothing"
        );

        // A dangling record (no connector shape) has nothing to write
        // but its record still moves down.
        let (mut doc, page, target, _connector) = fixture(None);
        let target_visio = visio_of(&doc, target);
        doc.add_connect(page, record(99, "BeginX", target_visio, 1))
            .expect("record");
        renumber_and_reglue(&mut doc, page, target_visio, 0).expect("renumber");
        let connect = &doc.page(page).expect("page").connects[0];
        assert_eq!(connect.to_cell, "Connections.X1");
        assert_eq!(connect.to_part, mutate::VIS_CONNECTION_POINT);
    }

    /// An end glued to a row below the deleted one keeps its row: only
    /// the record and the `PAR(PNT(…))` formulas move down.
    #[test]
    fn an_end_record_renumbers_its_end_formulas() {
        let begin_x = cell(
            "BeginX",
            Some(1.0),
            Some("PAR(PNT(Sheet.7!Connections.X2,Sheet.7!Connections.Y2))"),
        );
        let (mut doc, page, target, connector) = fixture(Some(begin_x));
        let (target_visio, connector_visio) = (visio_of(&doc, target), visio_of(&doc, connector));
        doc.add_connect(page, record(connector_visio, "BeginX", target_visio, 1))
            .expect("record");
        renumber_and_reglue(&mut doc, page, target_visio, 0).expect("renumber");
        let formula = doc
            .shape(connector)
            .expect("connector")
            .sheet
            .cells
            .iter()
            .find(|cell| cell.name.as_str() == "BeginX")
            .and_then(|cell| cell.formula.clone());
        assert_eq!(
            formula.as_deref(),
            Some("PAR(PNT(Sheet.7!Connections.X1,Sheet.7!Connections.Y1))"),
            "the end follows its row down"
        );
    }

    /// `renumber_end_formulas` skips the ends it cannot read: a gone
    /// connector, a cell that is missing, a cell with no formula and a
    /// formula that names no later row.
    #[test]
    fn renumber_end_formulas_skips_what_it_cannot_read() {
        // A gone connector: nothing to rewrite.
        let (mut doc, _page, _target, _connector) = fixture(None);
        renumber_end_formulas(&mut doc, ShapeId { index: 9 }, End::Begin, 1).expect("no-op");

        // A shape without the end cells: nothing to rewrite.
        let (mut doc, _page, target, _connector) = fixture(None);
        renumber_end_formulas(&mut doc, target, End::End, 1).expect("no cells");
        assert!(doc.shape(target).is_some(), "the shape stays");

        // A cell with no formula is left as it is.
        let value_only = cell("BeginX", Some(1.0), None);
        let (mut doc, _page, _target, connector) = fixture(Some(value_only));
        renumber_end_formulas(&mut doc, connector, End::Begin, 1).expect("no formula");

        // A formula that names no later row (the deleted row's own) is
        // the re-glue's job, not the renumber's.
        let own_row = cell(
            "BeginX",
            Some(1.0),
            Some("PAR(PNT(Sheet.7!Connections.X1,Sheet.7!Connections.Y1))"),
        );
        let (mut doc, _page, _target, connector) = fixture(Some(own_row));
        renumber_end_formulas(&mut doc, connector, End::Begin, 1).expect("own row");
        let formula = doc
            .shape(connector)
            .expect("connector")
            .sheet
            .cells
            .iter()
            .find(|cell| cell.name.as_str() == "BeginX")
            .and_then(|cell| cell.formula.clone());
        assert_eq!(
            formula.as_deref(),
            Some("PAR(PNT(Sheet.7!Connections.X1,Sheet.7!Connections.Y1))"),
            "the deleted row's own formula is the re-glue's job"
        );
    }

    /// `glue_ends` answers nothing to snapshot when the page is gone.
    #[test]
    fn glue_ends_on_a_gone_page_names_nothing() {
        let (doc, page, target, connector) = fixture(None);
        let (target_visio, connector_visio) = (visio_of(&doc, target), visio_of(&doc, connector));
        let mut doc = doc;
        doc.add_connect(page, record(connector_visio, "BeginX", target_visio, 1))
            .expect("record");
        let (visio, touched) = glue_ends(&doc, PageId { index: 9 }, target_visio);
        assert_eq!(visio, target_visio);
        assert!(touched.is_empty(), "a gone page glues nothing");
    }

    /// The row-number rewriter moves down only the numbers above the
    /// deleted row, and reports whether anything changed.
    #[test]
    fn renumber_formula_moves_the_later_rows_down() {
        let formula = "PAR(PNT(Sheet.7!Connections.X3,Sheet.7!Connections.Y3))";
        assert_eq!(
            renumber_formula(formula, 3),
            None,
            "the deleted row's own formula is handled by the re-glue"
        );
        assert_eq!(
            renumber_formula(formula, 2).as_deref(),
            Some("PAR(PNT(Sheet.7!Connections.X2,Sheet.7!Connections.Y2))")
        );
        assert_eq!(
            renumber_formula(formula, 4),
            None,
            "a row above the deleted one is untouched"
        );
        let lower = "par(pnt(sheet.7!connections.x10,Sheet.7!Connections.Y1))";
        assert_eq!(
            renumber_formula(lower, 1).as_deref(),
            Some("par(pnt(sheet.7!connections.x9,Sheet.7!Connections.Y1))"),
            "the scan is case-blind and the row 1 stays"
        );
    }
}
