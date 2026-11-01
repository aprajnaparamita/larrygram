//! `shape.*` commands (P5-04): move, resize, rotate and the Size &
//! Position pane edits (ADR 0035), plus the group enter/exit commands.
//!
//! Argument names are the command contract (task P5-04):
//!
//! - `shape.move` takes `dx`, `dy` (deltas in inches);
//! - `shape.resize` takes `width`, `height` (the target size of each
//!   selected shape, in inches);
//! - `shape.rotate` takes `angle` (the rotation **delta** in radians;
//!   `shape.set.angle` sets the absolute angle);
//! - `shape.set.width` / `shape.set.height` take `width` / `height`;
//! - `shape.set.pin` takes `x`, `y` (the absolute pin position);
//! - `shape.set.angle` takes `angle` (radians, absolute);
//! - `shape.set.text_block` (task P7-25) takes any of `x`, `y`,
//!   `width`, `height` (the text block's `TxtPinX`/`TxtPinY`/
//!   `TxtWidth`/`TxtHeight`), at least one.
//!
//! `shape.move`, `shape.resize` and `shape.rotate` work on every
//! selected shape; the `shape.set.*` pane fields edit the anchor
//! shape (the panes' subject, ADR 0041). Every command returns one
//! [`CompositeEdit`] (one undo step, ADR 0040) and each step bumps
//! `DocumentSession::changes` (ADR 0040 rule 3).
//!
//! The numbers are read as cached values through the inheritance
//! lookup (ADR 0054), so a shape whose cells have no cached values
//! anywhere moves to the delta and warns. Every write is an indirect
//! programmatic write (ADR 0056): a `GUARD`-ed cell keeps its formula
//! and warns once. A resize writes `Width`/`Height` and lets the recalc
//! pass compute the geometry rows whose formulas compute (ADR 0056);
//! [`GeometryEdit`] scales the constant rows beside it.

use larrygram_geom::Size;
use larrygram_model::{CellValue, CellWrite, ShapeId, SheetRef};

use crate::app::{AppCtx, SessionId};
use crate::edit::CompositeEdit;
use crate::error::CommandError;
use crate::undo::Edit;
use crate::{Invocation, Outcome};

use super::connector::reroute_quietly_on;
use super::edit::{
    CellEdit, GeometryEdit, count_label, document_mut, number_literal, read_cell_number,
    selection_targets,
};
use super::{
    CommandTable, edit_outcome, has_selection, need_finite, need_size, no_document, outcome,
};

/// The implemented `shape.*` commands (task P5-04).
pub(crate) const TABLE: CommandTable = &[
    ("shape.move", "Move", has_selection, run_move),
    ("shape.resize", "Resize", has_selection, run_resize),
    ("shape.rotate", "Rotate", has_selection, run_rotate),
    ("shape.set.width", "Width", has_selection, run_set_width),
    ("shape.set.height", "Height", has_selection, run_set_height),
    ("shape.set.pin", "Position", has_selection, run_set_pin),
    ("shape.set.angle", "Angle", has_selection, run_set_angle),
    (
        "shape.set.text_block",
        "Text Block",
        has_selection,
        run_set_text_block,
    ),
    (
        "shape.enter_group",
        "Enter Group",
        anchor_is_group,
        run_enter_group,
    ),
    ("shape.exit_group", "Exit Group", in_group, run_exit_group),
];

/// Enabled when the anchor shape is a group (`shape.enter_group`).
fn anchor_is_group(ctx: &AppCtx) -> crate::Enabled {
    let Some(shape) = anchor_shape(ctx) else {
        return crate::Enabled::no("no selection");
    };
    match ctx.active_document().and_then(|s| s.document.shape(shape)) {
        Some(entry) if !entry.children.is_empty() => crate::Enabled::yes(),
        _ => crate::Enabled::no("the selected shape is not a group"),
    }
}

/// Enabled while a group is entered (`shape.exit_group`).
fn in_group(ctx: &AppCtx) -> crate::Enabled {
    if ctx.selection.entered.is_some() {
        crate::Enabled::yes()
    } else {
        crate::Enabled::no("not inside a group")
    }
}

/// The active page, when one exists (the re-route hook's argument).
fn active_page(ctx: &AppCtx) -> Option<larrygram_model::PageId> {
    ctx.active_document()
        .and_then(|session| session.active_page)
}

/// Re-routes the page's glued connectors after an editing command.
fn reroute(ctx: &mut AppCtx, page: Option<larrygram_model::PageId>) {
    if let Some(page) = page {
        reroute_quietly_on(ctx, page);
    }
}

/// The panes' subject: the anchor, or the last selected shape when
/// there is no anchor.
fn anchor_shape(ctx: &AppCtx) -> Option<ShapeId> {
    ctx.selection
        .anchor
        .or_else(|| ctx.selection.shapes.last().copied())
}

/// The session and the anchor shape of a `shape.set.*` /
/// `data.set.property` run.
pub(super) fn anchor_target(
    ctx: &AppCtx,
) -> Result<(SessionId, ShapeId, Vec<String>), CommandError> {
    let shape = anchor_shape(ctx).ok_or_else(|| CommandError::failed("no selection"))?;
    let session = ctx.active_document().ok_or_else(no_document)?;
    if session.document.page_of(shape).is_none() {
        return Err(CommandError::failed(format!(
            "shape {} is no longer on a page",
            shape.index
        )));
    }
    Ok((session.id, shape, Vec::new()))
}

/// One number cell write of `shape` (the shared tail of the geometry
/// commands).
fn cell_edit(
    doc: &larrygram_model::Document,
    session: SessionId,
    label: &str,
    shape: ShapeId,
    cell: &str,
    value: f64,
) -> Result<Box<dyn Edit>, CommandError> {
    if !value.is_finite() {
        return Err(CommandError::invalid_args(
            "the computed value is not finite",
        ));
    }
    Ok(Box::new(CellEdit::new(
        doc,
        session,
        label,
        SheetRef::Shape(shape),
        cell,
        CellWrite::new(number_literal(value), CellValue::Number(value)),
    )?))
}

/// The constant-geometry rescale step of a size change (ADR 0056), when
/// the current size can be scaled (a scale factor needs a positive
/// source box).
fn scale_edit(
    doc: &larrygram_model::Document,
    session: SessionId,
    shape: ShapeId,
    from: (f64, f64),
    to: (f64, f64),
    warnings: &mut Vec<String>,
) -> Result<Option<Box<dyn Edit>>, CommandError> {
    if from.0.is_finite() && from.1.is_finite() && from.0 > 0.0 && from.1 > 0.0 {
        Ok(Some(Box::new(GeometryEdit::new(
            doc,
            session,
            "Resize shape",
            shape,
            Size::new(from.0, from.1),
            Size::new(to.0, to.1),
        )?)))
    } else {
        warnings.push(format!(
            "shape {} has no usable size; its geometry was not scaled",
            shape.index
        ));
        Ok(None)
    }
}

/// `shape.move`: shifts every selected shape by (`dx`, `dy`) inches.
pub(super) fn run_move(ctx: &mut AppCtx, inv: &Invocation) -> Result<Outcome, CommandError> {
    let dx = need_finite(&inv.args, "dx", "x offset in inches")?;
    let dy = need_finite(&inv.args, "dy", "y offset in inches")?;
    let (session, targets, mut warnings) = selection_targets(ctx)?;
    let mut edit = CompositeEdit::new(count_label("Move", targets.len()));
    {
        let doc = document_mut(ctx, session)?;
        for &shape in &targets {
            let (x, read) = read_cell_number(doc, shape, "PinX")?;
            warnings.extend(read);
            let (y, read) = read_cell_number(doc, shape, "PinY")?;
            warnings.extend(read);
            let step = cell_edit(doc, session, "Move shape", shape, "PinX", x + dx);
            edit.push(step?);
            let step = cell_edit(doc, session, "Move shape", shape, "PinY", y + dy);
            edit.push(step?);
        }
    }
    edit.apply(ctx)?;
    reroute(ctx, active_page(ctx));
    Ok(edit_outcome(
        count_label("Moved", targets.len()),
        warnings,
        Box::new(edit),
    ))
}

/// `shape.resize`: gives every selected shape the size (`width`,
/// `height`), scaling its constant geometry and letting recalc compute
/// the rows with formulas (ADR 0056).
pub(super) fn run_resize(ctx: &mut AppCtx, inv: &Invocation) -> Result<Outcome, CommandError> {
    let width = need_size(&inv.args, "width", "new width in inches")?;
    let height = need_size(&inv.args, "height", "new height in inches")?;
    let (session, targets, mut warnings) = selection_targets(ctx)?;
    let mut edit = CompositeEdit::new(count_label("Resize", targets.len()));
    {
        let doc = document_mut(ctx, session)?;
        for &shape in &targets {
            let (old_width, read) = read_cell_number(doc, shape, "Width")?;
            warnings.extend(read);
            let (old_height, read) = read_cell_number(doc, shape, "Height")?;
            warnings.extend(read);
            if let Some(step) = scale_edit(
                doc,
                session,
                shape,
                (old_width, old_height),
                (width, height),
                &mut warnings,
            )? {
                edit.push(step);
            }
            let step = cell_edit(doc, session, "Resize shape", shape, "Width", width);
            edit.push(step?);
            let step = cell_edit(doc, session, "Resize shape", shape, "Height", height);
            edit.push(step?);
        }
    }
    edit.apply(ctx)?;
    reroute(ctx, active_page(ctx));
    Ok(edit_outcome(
        count_label("Resized", targets.len()),
        warnings,
        Box::new(edit),
    ))
}

/// `shape.rotate`: rotates every selected shape by `angle` radians
/// (around its own pin).
pub(super) fn run_rotate(ctx: &mut AppCtx, inv: &Invocation) -> Result<Outcome, CommandError> {
    let delta = need_finite(&inv.args, "angle", "rotation in radians")?;
    let (session, targets, mut warnings) = selection_targets(ctx)?;
    let mut edit = CompositeEdit::new(count_label("Rotate", targets.len()));
    {
        let doc = document_mut(ctx, session)?;
        for &shape in &targets {
            let (angle, read) = read_cell_number(doc, shape, "Angle")?;
            warnings.extend(read);
            let step = cell_edit(doc, session, "Rotate shape", shape, "Angle", angle + delta);
            edit.push(step?);
        }
    }
    edit.apply(ctx)?;
    reroute(ctx, active_page(ctx));
    Ok(edit_outcome(
        count_label("Rotated", targets.len()),
        warnings,
        Box::new(edit),
    ))
}

/// `shape.set.width`: sets the anchor shape's `Width` (inches) and
/// rescales its geometry (ADR 0056).
pub(super) fn run_set_width(ctx: &mut AppCtx, inv: &Invocation) -> Result<Outcome, CommandError> {
    set_size(ctx, inv, Axis::Width)
}

/// `shape.set.height`: sets the anchor shape's `Height` (inches) and
/// rescales its geometry (ADR 0056).
pub(super) fn run_set_height(ctx: &mut AppCtx, inv: &Invocation) -> Result<Outcome, CommandError> {
    set_size(ctx, inv, Axis::Height)
}

/// Which axis of the anchor shape a `shape.set.width` / `shape.set.height`
/// run sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Axis {
    /// The width.
    Width,
    /// The height.
    Height,
}

fn set_size(ctx: &mut AppCtx, inv: &Invocation, axis: Axis) -> Result<Outcome, CommandError> {
    let (name, prompt) = match axis {
        Axis::Width => ("width", "new width in inches"),
        Axis::Height => ("height", "new height in inches"),
    };
    let value = need_size(&inv.args, name, prompt)?;
    let (session, shape, mut warnings) = anchor_target(ctx)?;
    let mut edit = CompositeEdit::new(match axis {
        Axis::Width => "Set Width",
        Axis::Height => "Set Height",
    });
    {
        let doc = document_mut(ctx, session)?;
        let (old_width, read) = read_cell_number(doc, shape, "Width")?;
        warnings.extend(read);
        let (old_height, read) = read_cell_number(doc, shape, "Height")?;
        warnings.extend(read);
        let (from, to, cell) = match axis {
            Axis::Width => ((old_width, old_height), (value, old_height), "Width"),
            Axis::Height => ((old_width, old_height), (old_width, value), "Height"),
        };
        if let Some(step) = scale_edit(doc, session, shape, from, to, &mut warnings)? {
            edit.push(step);
        }
        edit.push(cell_edit(doc, session, "Set size", shape, cell, value)?);
    }
    edit.apply(ctx)?;
    reroute(ctx, active_page(ctx));
    Ok(edit_outcome(
        match axis {
            Axis::Width => "Width set",
            Axis::Height => "Height set",
        },
        warnings,
        Box::new(edit),
    ))
}

/// `shape.set.pin`: puts the anchor shape's pin at (`x`, `y`) inches.
pub(super) fn run_set_pin(ctx: &mut AppCtx, inv: &Invocation) -> Result<Outcome, CommandError> {
    let x = need_finite(&inv.args, "x", "pin x in inches")?;
    let y = need_finite(&inv.args, "y", "pin y in inches")?;
    let (session, shape, warnings) = anchor_target(ctx)?;
    let mut edit = CompositeEdit::new("Set Pin");
    {
        let doc = document_mut(ctx, session)?;
        edit.push(cell_edit(doc, session, "Set Pin", shape, "PinX", x)?);
        edit.push(cell_edit(doc, session, "Set Pin", shape, "PinY", y)?);
    }
    edit.apply(ctx)?;
    reroute(ctx, active_page(ctx));
    Ok(edit_outcome("Pin set", warnings, Box::new(edit)))
}

/// `shape.set.angle`: sets the anchor shape's `Angle` to `angle`
/// radians.
pub(super) fn run_set_angle(ctx: &mut AppCtx, inv: &Invocation) -> Result<Outcome, CommandError> {
    let angle = need_finite(&inv.args, "angle", "angle in radians")?;
    let (session, shape, warnings) = anchor_target(ctx)?;
    let mut edit = CompositeEdit::new("Set Angle");
    {
        let doc = document_mut(ctx, session)?;
        edit.push(cell_edit(doc, session, "Set Angle", shape, "Angle", angle)?);
    }
    edit.apply(ctx)?;
    reroute(ctx, active_page(ctx));
    Ok(edit_outcome("Angle set", warnings, Box::new(edit)))
}

/// `shape.set.text_block` (task P7-25 item 4): writes the anchor
/// shape's text block cells, any of the optional `x`/`y`
/// (`TxtPinX`/`TxtPinY`, inches, shape-local) and `width`/`height`
/// (`TxtWidth`/`TxtHeight`, inches). At least one argument must be
/// present. The whole run is one undo step (ADR 0040): one text block
/// drag writes its final values in one command.
pub(super) fn run_set_text_block(
    ctx: &mut AppCtx,
    inv: &Invocation,
) -> Result<Outcome, CommandError> {
    let writes: [(&str, &str, Option<f64>); 4] = [
        ("x", "TxtPinX", optional_number(&inv.args, "x")?),
        ("y", "TxtPinY", optional_number(&inv.args, "y")?),
        (
            "width",
            "TxtWidth",
            optional_non_negative(&inv.args, "width")?,
        ),
        (
            "height",
            "TxtHeight",
            optional_non_negative(&inv.args, "height")?,
        ),
    ];
    if writes.iter().all(|(_, _, value)| value.is_none()) {
        return Err(CommandError::invalid_args(
            "text block needs an x, y, width or height argument",
        ));
    }
    let (session, shape, warnings) = anchor_target(ctx)?;
    let mut edit = CompositeEdit::new("Set Text Block");
    {
        let doc = document_mut(ctx, session)?;
        for (_, cell, value) in writes {
            if let Some(value) = value {
                edit.push(cell_edit(
                    doc,
                    session,
                    "Set Text Block",
                    shape,
                    cell,
                    value,
                )?);
            }
        }
    }
    edit.apply(ctx)?;
    Ok(edit_outcome("Text block set", warnings, Box::new(edit)))
}

/// One optional finite number argument (task P7-25 item 4).
fn optional_number(args: &crate::Args, name: &str) -> Result<Option<f64>, CommandError> {
    match args.get(name) {
        None => Ok(None),
        Some(crate::ArgValue::Number(value)) if value.is_finite() => Ok(Some(*value)),
        Some(crate::ArgValue::Number(_)) => {
            Err(CommandError::invalid_args(format!("{name} must be finite")))
        }
        Some(crate::ArgValue::Integer(value)) => Ok(Some(*value as f64)),
        Some(_) => Err(CommandError::invalid_args(format!(
            "{name} must be a number"
        ))),
    }
}

/// One optional non-negative size argument (task P7-25 item 4).
fn optional_non_negative(args: &crate::Args, name: &str) -> Result<Option<f64>, CommandError> {
    match optional_number(args, name)? {
        None => Ok(None),
        Some(value) if value >= 0.0 => Ok(Some(value)),
        Some(_) => Err(CommandError::invalid_args(format!(
            "{name} must not be negative"
        ))),
    }
}

/// `shape.enter_group`: enters the anchor group (ADR 0041); the
/// selection becomes its members.
///
/// Selection only: no document change and no undo step.
pub(super) fn run_enter_group(
    ctx: &mut AppCtx,
    _inv: &Invocation,
) -> Result<Outcome, CommandError> {
    let group = anchor_shape(ctx).ok_or_else(|| CommandError::failed("no selection"))?;
    let children = {
        let session = ctx.active_document().ok_or_else(no_document)?;
        let entry = session
            .document
            .shape(group)
            .ok_or_else(|| CommandError::failed(format!("unknown shape id {}", group.index)))?;
        if entry.children.is_empty() {
            return Err(CommandError::failed("the selected shape is not a group"));
        }
        entry.children.clone()
    };
    let count = children.len();
    ctx.selection.entered = Some(group);
    ctx.selection.anchor = children.last().copied();
    ctx.selection.shapes = children;
    Ok(outcome(count_label("Selected", count), Vec::new()))
}

/// `shape.exit_group`: leaves the entered group and selects it again
/// (its own parent group becomes the entered one, when it has one).
pub(super) fn run_exit_group(ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
    let group = ctx
        .selection
        .entered
        .ok_or_else(|| CommandError::failed("not inside a group"))?;
    let parent = {
        let session = ctx.active_document().ok_or_else(no_document)?;
        session
            .document
            .shape(group)
            .ok_or_else(|| CommandError::failed("the entered group no longer exists"))?
            .parent
    };
    ctx.selection.shapes = vec![group];
    ctx.selection.anchor = Some(group);
    ctx.selection.entered = parent;
    Ok(outcome("Exited group", Vec::new()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Source;
    use crate::app::{DocumentSession, Settings};
    use larrygram_model::{Cell, CellName, CellValue, Document, Preserved};

    fn number_cell(name: &str, value: f64) -> Cell {
        Cell {
            name: CellName::new(name),
            formula: Some(number_literal(value)),
            value: Some(CellValue::Number(value)),
            unit: larrygram_geom::Unit::Inches,
            preserved: Preserved::new(),
        }
    }

    /// A state with one 2x1 shape on one page.
    fn fixture() -> (AppCtx, SessionId, larrygram_model::PageId, ShapeId) {
        let mut doc = Document::new();
        let page = doc.add_page("Page-1").expect("page");
        let mut shape = larrygram_model::Shape::new(ShapeId { index: 0 }, 1);
        shape.sheet.cells.push(number_cell("PinX", 1.0));
        shape.sheet.cells.push(number_cell("PinY", 2.0));
        let shape = doc.insert_shape(page, shape, None).expect("insert");
        let mut ctx = AppCtx::new(Settings::default());
        let index = ctx.open_document(DocumentSession::new(doc, None));
        let session = ctx.documents.get(index).expect("session").id;
        (ctx, session, page, shape)
    }

    /// The direct-call path of `shape.enter_group` (the enabled-state
    /// predicate blocks it through the registry).
    #[test]
    fn enter_group_on_a_non_group_reports_it() {
        let (mut ctx, _session, page, shape) = fixture();
        ctx.selection.shapes = vec![shape];
        ctx.selection.anchor = Some(shape);
        ctx.selection.page = Some(page);
        let error =
            run_enter_group(&mut ctx, &Invocation::empty(Source::Test)).expect_err("not a group");
        assert_eq!(error.to_string(), "the selected shape is not a group");
    }

    /// A selection entry that is no longer on a page is refused by the
    /// pane commands (`shape.set.*`) with a named shape.
    #[test]
    fn a_detached_anchor_is_refused() {
        let (mut ctx, _session, _page, shape) = fixture();
        ctx.active_document_mut()
            .expect("document")
            .document
            .detach_shape(shape)
            .expect("detach");
        ctx.selection.shapes = vec![shape];
        ctx.selection.anchor = Some(shape);
        let error = anchor_target(&ctx).expect_err("detached");
        assert_eq!(error.to_string(), "shape 0 is no longer on a page");

        // With no selection at all it says so.
        ctx.selection.shapes.clear();
        ctx.selection.anchor = None;
        let error = anchor_target(&ctx).expect_err("empty");
        assert_eq!(error.to_string(), "no selection");
    }

    /// A shape whose size cannot be scaled warns and adds no step
    /// (the `scale_edit` fallback).
    #[test]
    fn scale_edit_skips_a_degenerate_size() {
        let (ctx, session, _page, shape) = fixture();
        let mut warnings = Vec::new();
        let doc = &ctx.active_document().expect("document").document;
        let step = scale_edit(doc, session, shape, (0.0, 1.0), (4.0, 1.0), &mut warnings)
            .expect("no error");
        assert!(step.is_none());
        assert_eq!(warnings.len(), 1);
        assert!(
            warnings[0].contains("geometry was not scaled"),
            "{warnings:?}"
        );
    }

    /// Task P5-19 coverage: the staging `?` arms of `cell_edit` and
    /// `scale_edit` (a shape that vanished before the step is staged).
    #[test]
    fn edit_staging_reports_a_stale_shape() {
        let (mut ctx, session, _page, _shape) = fixture();
        let doc = document_mut(&mut ctx, session).expect("doc");
        let stale = ShapeId { index: 9 };
        let error = cell_edit(doc, session, "Move shape", stale, "PinX", 1.0)
            .map(|_| ())
            .expect_err("stale");
        assert!(error.to_string().contains("unknown shape id"), "{error}");
        let mut warnings = Vec::new();
        let error = scale_edit(doc, session, stale, (1.0, 1.0), (2.0, 2.0), &mut warnings)
            .map(|_| ())
            .expect_err("stale");
        assert!(error.to_string().contains("unknown shape id"), "{error}");
    }

    /// Task P5-19 coverage: `run_move` stages `PinX` first and then
    /// finds that `PinY` is not finite (the second staging `?`).
    #[test]
    fn move_reports_a_non_finite_pin_y() {
        let (mut ctx, session, page, shape) = fixture();
        {
            let doc = document_mut(&mut ctx, session).expect("doc");
            let entry = doc.shape_mut(shape).expect("shape");
            for cell in &mut entry.sheet.cells {
                if cell.name.as_str() == "PinY" {
                    cell.value = Some(CellValue::Number(1.7e308));
                }
            }
        }
        ctx.selection.shapes = vec![shape];
        ctx.selection.anchor = Some(shape);
        ctx.selection.page = Some(page);
        let inv = Invocation::new(
            crate::Args::new()
                .with("dx", crate::ArgValue::Number(1.0))
                .with("dy", crate::ArgValue::Number(1.7e308)),
            Source::Test,
        );
        let error = run_move(&mut ctx, &inv).expect_err("non-finite pin");
        assert!(error.to_string().contains("not finite"), "{error}");
    }

    /// The text block write of task P7-25 item 4: the four cells, one
    /// undo step whose revert restores them all.
    #[test]
    fn a_text_block_write_lands_in_one_undo_step() {
        let (mut ctx, session, page, shape) = fixture();
        ctx.selection.shapes = vec![shape];
        ctx.selection.anchor = Some(shape);
        ctx.selection.page = Some(page);
        let inv = Invocation::new(
            crate::Args::new()
                .with("x", crate::ArgValue::Number(0.75))
                .with("y", crate::ArgValue::Number(1.25))
                .with("width", crate::ArgValue::Number(3.0))
                .with("height", crate::ArgValue::Number(0.5)),
            Source::Test,
        );
        let outcome = run_set_text_block(&mut ctx, &inv).expect("run");
        let mut edit = outcome.edit.expect("one undo step");
        {
            let doc = document_mut(&mut ctx, session).expect("doc");
            let entry = doc.shape(shape).expect("shape");
            for (name, want) in [
                ("TxtPinX", 0.75),
                ("TxtPinY", 1.25),
                ("TxtWidth", 3.0),
                ("TxtHeight", 0.5),
            ] {
                let cell = entry
                    .sheet
                    .cells
                    .iter()
                    .find(|cell| cell.name.as_str() == name)
                    .unwrap_or_else(|| panic!("no {name}"));
                assert_eq!(cell.value, Some(CellValue::Number(want)), "{name}");
            }
        }
        // One revert restores every cell the run wrote.
        edit.revert(&mut ctx).expect("revert");
        let doc = document_mut(&mut ctx, session).expect("doc");
        let entry = doc.shape(shape).expect("shape");
        for name in ["TxtPinX", "TxtPinY", "TxtWidth", "TxtHeight"] {
            assert!(
                entry
                    .sheet
                    .cells
                    .iter()
                    .all(|cell| cell.name.as_str() != name),
                "{name} was restored away"
            );
        }
    }

    /// `shape.set.text_block` refuses an empty run and bad values.
    #[test]
    fn a_text_block_write_validates_its_arguments() {
        let (mut ctx, _session, page, shape) = fixture();
        ctx.selection.shapes = vec![shape];
        ctx.selection.anchor = Some(shape);
        ctx.selection.page = Some(page);
        let error = run_set_text_block(&mut ctx, &Invocation::empty(Source::Test))
            .expect_err("no arguments");
        assert!(error.to_string().contains("text block needs"), "{error}");
        for (args, want) in [
            (
                crate::Args::new().with("width", crate::ArgValue::Number(-1.0)),
                "must not be negative",
            ),
            (
                crate::Args::new().with("x", crate::ArgValue::Number(f64::NAN)),
                "must be finite",
            ),
            (
                crate::Args::new().with("y", crate::ArgValue::Text("left".to_owned())),
                "must be a number",
            ),
        ] {
            let inv = Invocation::new(args, Source::Test);
            let error = run_set_text_block(&mut ctx, &inv).expect_err("bad argument");
            assert!(error.to_string().contains(want), "{error}");
        }
    }
}
