//! The Format Painter's canvas mode (task P7-24 item 6).
//!
//! While a format copy is recorded ([`AppCtx::format_painter`], task
//! P7-21) a canvas release over a shape selects that shape and applies
//! the recorded format through `tools.format_painter`; Escape (as
//! [`ToolEvent::Cancel`]) cancels the copy. The module only computes
//! [`Intent`]s (ADR 0028): it never mutates the model or the selection
//! itself, and every event order is a no-op, never a panic.

use larrygram_commands::{ArgValue, Args, AppCtx, CommandError, CommandId, Invocation, Source};
use larrygram_geom::Point;
use larrygram_model::ShapeId;
use larrygram_render::HitOptions;

use super::pointer::HIT_TOLERANCE_PX;
use super::{ToolEffect, ToolEvent, ToolMachine};
use crate::handles::render_error;
use crate::{Intent, UiError};

/// Handles one event while a format copy is recorded (the shell routes
/// the pointer tool's events here first, task P7-24 item 6).
///
/// A release over a shape emits `edit.select` for the target and then
/// `tools.format_painter` with `apply`, so the recorded format writes
/// the clicked shape and clears. A release over empty page applies
/// nothing and keeps the copy. `Cancel` (Escape) clears the copy.
pub fn handle(
    state: &mut ToolMachine,
    event: &ToolEvent,
    ctx: &AppCtx,
) -> Result<ToolEffect, UiError> {
    let mut effect = ToolEffect::new();
    if ctx.format_painter.is_none() {
        return Ok(effect);
    }
    match event {
        ToolEvent::PointerUp { pos, .. } => {
            let Some(shape) = shape_at(state, ctx, *pos)? else {
                return Ok(effect);
            };
            effect.intents.push(select_intent(ctx, shape)?);
            effect.intents.push(intent(
                "tools.format_painter",
                Args::new().with("apply", ArgValue::Flag(true)),
            )?);
        }
        ToolEvent::Cancel => {
            effect.intents.push(intent(
                "tools.format_painter",
                Args::new().with("cancel", ArgValue::Flag(true)),
            )?);
        }
        _ => {}
    }
    Ok(effect)
}

/// The topmost shape under `pos`, the pointer tool's hit rule (task
/// P7-24 item 6: the click targets one shape).
fn shape_at(
    state: &mut ToolMachine,
    ctx: &AppCtx,
    pos: Point,
) -> Result<Option<ShapeId>, UiError> {
    let zoom = ctx
        .active_document()
        .map(|session| session.view.zoom)
        .filter(|zoom| zoom.is_finite() && *zoom > 0.0)
        .unwrap_or(1.0);
    let Some(scene) = state.state.scene(ctx)? else {
        return Ok(None);
    };
    let tolerance = HIT_TOLERANCE_PX / (zoom * crate::canvas::POINTS_PER_INCH);
    let hits = larrygram_render::hit_test(scene, pos, &HitOptions::new(tolerance))
        .map_err(render_error)?;
    Ok(hits.iter().find_map(|hit| hit.shape_id))
}

/// The `edit.select` intent that makes `shape` the selection (the
/// apply form of `tools.format_painter` writes the selection).
fn select_intent(ctx: &AppCtx, shape: ShapeId) -> Result<Intent, UiError> {
    let Some(session) = ctx.active_document() else {
        return Err(UiError::Command(CommandError::failed("no open document")));
    };
    let Some(entry) = session.document.shape(shape) else {
        return Err(UiError::Command(CommandError::failed("the shape is gone")));
    };
    let mut args = Args::new();
    args.set("shapes", ArgValue::List(vec![entry.visio_id.to_string()]));
    args.set("mode", ArgValue::Text("replace".to_owned()));
    intent("edit.select", args)
}

/// Builds one UI-source intent, mapping a bad static id to a command
/// failure (the ids are compile-time constants; this cannot happen
/// with a valid build, and it must never panic).
fn intent(id: &'static str, args: Args) -> Result<Intent, UiError> {
    let command = CommandId::parse(id)
        .map_err(|error| UiError::Command(CommandError::failed(error.to_string())))?;
    Ok(Intent::new(command, Invocation::new(args, Source::Ui)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use larrygram_commands::Settings;
    use larrygram_commands::app::PaintSource;
    use larrygram_commands::DocumentSession;
    use larrygram_model::{
        Cell, CellName, CellValue, Document, Row, Section, SectionName, Shape,
    };

    /// One fixture cell: a literal number.
    fn number(name: &'static str, value: f64) -> Cell {
        let mut cell = Cell::new(CellName::from_static(name));
        cell.value = Some(CellValue::Number(value));
        cell
    }

    /// An app with one page and one box shape at (3, 3), a format copy
    /// recorded from it.
    fn app_with_shape() -> AppCtx {
        let mut doc = Document::new();
        let page = doc.add_page("Page-1").expect("page");
        let mut shape = Shape::new(ShapeId { index: 0 }, 1);
        shape.sheet.cells = vec![
            number("Width", 2.0),
            number("Height", 1.0),
            number("PinX", 3.0),
            number("PinY", 3.0),
            number("LocPinX", 1.0),
            number("LocPinY", 0.5),
            number("Angle", 0.0),
            number("FillPattern", 1.0),
            number("LinePattern", 1.0),
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
            row.cells = vec![number("X", px), number("Y", py)];
            geometry.rows.push(row);
        }
        shape.sheet.sections.push(geometry);
        let shape = doc.insert_shape(page, shape, None).expect("shape");
        let mut app = AppCtx::new(Settings::default());
        let session = DocumentSession::new(doc, None);
        let session_id = session.id;
        app.open_document(session);
        app.format_painter = Some(PaintSource {
            session: session_id,
            shape,
        });
        app
    }

    #[test]
    fn a_release_over_a_shape_selects_and_applies() {
        let ctx = app_with_shape();
        let mut machine = ToolMachine::new();
        let effect = handle(
            &mut machine,
            &ToolEvent::PointerUp {
                pos: Point::new(3.0, 3.0),
                modifiers: super::super::Modifiers::none(),
            },
            &ctx,
        )
        .expect("handle");
        assert_eq!(effect.intents.len(), 2);
        assert_eq!(effect.intents[0].command.as_str(), "edit.select");
        assert_eq!(effect.intents[1].command.as_str(), "tools.format_painter");
        assert_eq!(
            effect.intents[1].invocation.args.flag("apply"),
            Some(true),
            "the apply flag never re-copies"
        );
    }

    #[test]
    fn a_release_over_empty_page_applies_nothing() {
        let ctx = app_with_shape();
        let mut machine = ToolMachine::new();
        let effect = handle(
            &mut machine,
            &ToolEvent::PointerUp {
                pos: Point::new(9.0, 9.0),
                modifiers: super::super::Modifiers::none(),
            },
            &ctx,
        )
        .expect("handle");
        assert!(effect.intents.is_empty());
    }

    #[test]
    fn cancel_clears_and_no_copy_is_a_no_op() {
        let ctx = app_with_shape();
        let mut machine = ToolMachine::new();
        let effect = handle(&mut machine, &ToolEvent::Cancel, &ctx).expect("cancel");
        assert_eq!(effect.intents.len(), 1);
        assert_eq!(effect.intents[0].command.as_str(), "tools.format_painter");
        assert_eq!(effect.intents[0].invocation.args.flag("cancel"), Some(true));

        // With no recorded copy every event is a no-op.
        let mut idle = app_with_shape();
        idle.format_painter = None;
        let mut machine = ToolMachine::new();
        let effect = handle(&mut machine, &ToolEvent::Cancel, &idle).expect("idle cancel");
        assert!(effect.intents.is_empty());
        let effect = handle(
            &mut machine,
            &ToolEvent::PointerUp {
                pos: Point::new(3.0, 3.0),
                modifiers: super::super::Modifiers::none(),
            },
            &idle,
        )
        .expect("idle release");
        assert!(effect.intents.is_empty());
    }

    #[test]
    fn a_move_or_press_is_a_no_op() {
        let ctx = app_with_shape();
        let mut machine = ToolMachine::new();
        for event in [
            ToolEvent::PointerDown {
                pos: Point::new(3.0, 3.0),
                modifiers: super::super::Modifiers::none(),
            },
            ToolEvent::PointerMove {
                pos: Point::new(3.0, 3.0),
                modifiers: super::super::Modifiers::none(),
            },
            ToolEvent::DoubleClick {
                pos: Point::new(3.0, 3.0),
            },
        ] {
            let effect = handle(&mut machine, &event, &ctx).expect("handle");
            assert!(effect.intents.is_empty());
        }
    }
}
