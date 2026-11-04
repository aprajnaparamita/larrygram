//! `connector.move_end`: one undoable move of one connector end
//! (task P7-33 item 4, ADR 0084 rule 6).
//!
//! The end drag of the UI lands here: the run writes the end's glue
//! (P7-31's [`Document::write_connector_end`] and the `Connects`
//! record) and re-routes the connector, all in one edit. The argument
//! contract is strict: a missing required argument is a clear error,
//! never a dialog request (AGENTS.md section 6).

use larrygram_geom::Point;
use larrygram_model::Connect;

use crate::app::AppCtx;
use crate::error::CommandError;
use crate::undo::Edit;
use crate::{Args, Invocation, Outcome};

use super::connector::{
    End, GlueEdit, active_page, end_cells_point, glue_record, need_integer, optional_row,
    optional_visio_id, reroute_connectors, resolved_ends, target_shape, with_routes,
};
use super::{CommandTable, edit_outcome, has_pages, model_error, need_finite, no_document};

/// The implemented `connector.*` commands of this module (task P7-33).
pub(crate) const TABLE: CommandTable = &[(
    "connector.move_end",
    "Move Connector End",
    has_pages,
    run_move_end,
)];

/// `connector.move_end`: one undoable move of one connector end.
///
/// Arguments: `connector` (Visio id), `end` ("begin" | "end"), `x`,
/// `y` (page inches), optional `target` (Visio id) and optional
/// `point` (connection row ix). `target` with `point` is point glue,
/// `target` alone dynamic glue, neither a free end at `x`,`y`.
/// Resolves with `larrygram_geom::glue`, writes the end (P7-31's
/// `write_connector_end` and the record) and re-routes that connector,
/// all in one edit. A missing required argument is a clear error and
/// opens no dialog.
pub(crate) fn run_move_end(ctx: &mut AppCtx, inv: &Invocation) -> Result<Outcome, CommandError> {
    let connector_visio = required_integer(&inv.args, "connector")?;
    let end = End::parse(&required_text(&inv.args, "end")?)
        .ok_or_else(|| CommandError::invalid_args("end must be begin or end"))?;
    let x = required_finite(&inv.args, "x")?;
    let y = required_finite(&inv.args, "y")?;
    let target_visio = optional_visio_id(&inv.args, "target")?;
    let point = optional_row(&inv.args, "point")?;
    if point.is_some() && target_visio.is_none() {
        return Err(CommandError::invalid_args("point needs a target"));
    }
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
    // The target of the new glue (a 1-D shape is refused), if any.
    let target = match target_visio {
        Some(_) => Some(target_shape(ctx, page, &inv.args)?),
        None => None,
    };
    let (record, at, warnings) = {
        let doc = &ctx.active_document().ok_or_else(no_document)?.document;
        let record = match target {
            Some(target) => {
                let target_visio = doc
                    .shape(target)
                    .ok_or_else(|| CommandError::failed("the target shape no longer exists"))?
                    .visio_id;
                Some(glue_record(
                    connector,
                    target,
                    target_visio,
                    point,
                    end,
                    doc,
                )?)
            }
            None => None,
        };
        // Both ends resolve together (ADR 0084 rule 3): the other end
        // keeps its own glue, the dragged end lands where its new glue
        // resolves it, or at `x`,`y` when it is free.
        let current = doc.connects_of(page, connector).map_err(model_error)?;
        let mut slots: [Option<Connect>; 2] = [current[0].clone(), current[1].clone()];
        slots[end.slot()] = record.clone();
        let mut free_at = [None, None];
        free_at[end.slot()] = Some(Point::new(x, y));
        let mut resolve_warnings = Vec::new();
        let (begin, finish, _boxes) = resolved_ends(
            doc,
            page,
            connector,
            [
                slots.first().and_then(Option::as_ref),
                slots.get(1).and_then(Option::as_ref),
            ],
            free_at,
            &mut resolve_warnings,
        )?;
        let landed = match end {
            End::Begin => begin.point,
            End::End => finish.point,
        };
        // A resolution that lands nowhere keeps the requested point:
        // the cached `V` must stay finite (the model refuses a
        // non-finite endpoint).
        let at = if landed.x.is_finite() && landed.y.is_finite() {
            landed
        } else {
            Point::new(x, y)
        };
        // The endpoint cells are in the parent's coordinates (ADR 0084
        // rule 2 as amended): `x`,`y` arrive in page inches.
        let at = end_cells_point(doc, connector, at)?;
        (record, at, resolve_warnings)
    };
    let label = match end {
        End::Begin => "Move connector begin",
        End::End => "Move connector end",
    };
    let mut edit = {
        let doc = &ctx.active_document().ok_or_else(no_document)?.document;
        GlueEdit::new(
            doc,
            session,
            label,
            page,
            connector,
            connector_visio,
            end,
            record,
            at,
        )?
    };
    edit.apply(ctx)?;
    let routes = reroute_connectors(ctx, page, &[connector])?;
    let message = match (target_visio, point) {
        (Some(target), Some(ix)) => {
            format!("Connector end glued to shape {target} point {ix}")
        }
        (Some(target), None) => format!("Connector end glued to shape {target}"),
        (None, _) => "Connector end freed".to_owned(),
    };
    Ok(edit_outcome(
        message,
        warnings,
        with_routes(Box::new(edit), routes),
    ))
}

/// One required integer argument (task P7-33 item 4: a missing
/// argument is a clear error, never a dialog request).
fn required_integer(args: &Args, name: &str) -> Result<i32, CommandError> {
    match args.get(name) {
        None => Err(CommandError::invalid_args(format!("{name} is required"))),
        Some(_) => need_integer(args, name, name),
    }
}

/// One required text argument (the strict rule of [`required_integer`]).
fn required_text(args: &Args, name: &str) -> Result<String, CommandError> {
    if args.get(name).is_none() {
        return Err(CommandError::invalid_args(format!("{name} is required")));
    }
    args.text(name)
        .map(str::to_owned)
        .ok_or_else(|| CommandError::invalid_args(format!("{name} must be text")))
}

/// One required finite number argument (the strict rule of
/// [`required_integer`]).
fn required_finite(args: &Args, name: &str) -> Result<f64, CommandError> {
    match args.get(name) {
        None => Err(CommandError::invalid_args(format!("{name} is required"))),
        Some(_) => need_finite(args, name, name),
    }
}
