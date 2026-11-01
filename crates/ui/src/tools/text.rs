//! The text tool and the in-place editor hand-off (ADR 0044, task
//! P7-25 item 9).
//!
//! A click with the `Text` tool commits whatever is being typed (the
//! shell does that before the event reaches this module) and then
//! decides the click: a click inside an existing text object starts
//! editing that object's text, a click on empty page starts a new text
//! box. The new box is a [`PendingBox`] — the tool records where it
//! goes and the shell opens the editor over it; the box itself is
//! inserted once, with its text, on commit
//! (`insert.drawing.text_box`'s `text` argument, one undo step). The
//! tool never mutates the model or the selection itself (ADR 0042).
//!
//! The pointer tool's double click (item 8: edit a shape's text) is
//! `mod@pointer`'s `double_click`; it keeps the group click rule.

use larrygram_commands::{ArgValue, Args, CommandError, CommandId, Invocation, Source};
use larrygram_geom::Point;
use larrygram_model::ShapeId;
use larrygram_render::{HitOptions, hit_test};

use crate::UiError;
use crate::handles::render_error;

use super::pointer::{HIT_TOLERANCE_PX, selectable_shape};
use super::{ToolEffect, ToolEvent};

/// The default width of a new text box, in inches (our own choice).
pub const TEXT_BOX_WIDTH_IN: f64 = 2.0;

/// The default height of a new text box, in inches (our own choice).
pub const TEXT_BOX_HEIGHT_IN: f64 = 0.5;

/// Where a new text box goes once its text is committed (task P7-25
/// item 9): the insert command's coordinate rule, `x`/`y` the
/// lower-left corner in page inches.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PendingBox {
    /// The lower-left corner x, in page inches.
    pub x: f64,
    /// The lower-left corner y, in page inches.
    pub y: f64,
    /// The box width, in page inches.
    pub width: f64,
    /// The box height, in page inches.
    pub height: f64,
}

/// The text tool's click state (owner: this module).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    /// The new text box a click on empty page started (task P7-25
    /// item 9); the shell consumes it when it opens the editor.
    pub pending: Option<PendingBox>,
}

impl State {
    /// Takes the pending box, leaving the state idle.
    pub fn take_pending(&mut self) -> Option<PendingBox> {
        self.pending.take()
    }
}

/// Handles one event for this tool (owner: P5-07, ADR 0042; the click
/// policy is task P7-25 item 9).
///
/// A release decides the click: edit the clicked text object, or
/// start a new box at the release point. The editor state itself
/// lives in [`crate::text_edit::TextEditor`]; the tool only records
/// the pending box and emits registry intents (ADR 0042).
pub fn handle(
    state: &mut super::ToolMachine,
    event: &super::ToolEvent,
    ctx: &larrygram_commands::AppCtx,
) -> Result<super::ToolEffect, UiError> {
    let mut effect = ToolEffect::new();
    if ctx.active_document().is_none() {
        return Ok(effect);
    }
    match event {
        ToolEvent::PointerUp { pos, .. } => click(state, ctx, *pos, &mut effect)?,
        ToolEvent::Cancel => {
            state.text.pending = None;
        }
        // The release of the double click's second click decides the
        // click already (task P7-25 item 9).
        ToolEvent::DoubleClick { .. }
        | ToolEvent::PointerDown { .. }
        | ToolEvent::PointerMove { .. } => {}
    }
    Ok(effect)
}

/// The drag anchor, in page inches (the preview's start).
///
/// The text tool has no drag preview, so it reports no anchor.
pub fn anchor(_state: &super::ToolMachine) -> Option<Point> {
    None
}

/// One click (task P7-25 item 9): a click on an existing text object
/// edits that object's text, a click on empty page starts a new box.
fn click(
    state: &mut super::ToolMachine,
    ctx: &larrygram_commands::AppCtx,
    pos: Point,
    effect: &mut ToolEffect,
) -> Result<(), UiError> {
    let zoom = ctx
        .active_document()
        .map(|session| session.view.zoom)
        .filter(|zoom| zoom.is_finite() && *zoom > 0.0)
        .unwrap_or(1.0);
    let hit = {
        let Some(scene) = state.state.scene(ctx)? else {
            return Ok(());
        };
        let tolerance = HIT_TOLERANCE_PX / (zoom * crate::canvas::POINTS_PER_INCH);
        let hits = hit_test(scene, pos, &HitOptions::new(tolerance)).map_err(render_error)?;
        hits.iter().find_map(|hit| hit.shape_id)
    };
    let Some(session) = ctx.active_document() else {
        return Ok(());
    };
    match hit {
        Some(raw) => {
            let shape = selectable_shape(&session.document, raw, ctx.selection.entered);
            state.text.pending = None;
            effect
                .intents
                .push(select_intent(&session.document, shape)?);
            effect.intents.push(intent("text.begin", Args::new())?);
        }
        None => {
            state.text.pending = Some(PendingBox {
                x: pos.x,
                y: pos.y,
                width: TEXT_BOX_WIDTH_IN,
                height: TEXT_BOX_HEIGHT_IN,
            });
        }
    }
    Ok(())
}

/// The `edit.select` intent for `shape` (the editor target rule: the
/// selection names what the editor opens on).
fn select_intent(
    doc: &larrygram_model::Document,
    shape: ShapeId,
) -> Result<crate::Intent, UiError> {
    let ids = doc
        .shape(shape)
        .map(|entry| vec![entry.visio_id.to_string()])
        .unwrap_or_default();
    intent(
        "edit.select",
        Args::new()
            .with("shapes", ArgValue::List(ids))
            .with("mode", ArgValue::Text("replace".to_owned())),
    )
}

/// Builds one UI-source intent, mapping a bad static id to a command
/// failure (the ids are compile-time constants; this cannot happen with
/// a valid build, and it must never panic).
fn intent(id: &'static str, args: Args) -> Result<crate::Intent, UiError> {
    let command = CommandId::parse(id)
        .map_err(|error| UiError::Command(CommandError::failed(error.to_string())))?;
    Ok(crate::Intent::new(
        command,
        Invocation::new(args, Source::Ui),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use larrygram_commands::{DocumentSession, Settings};
    use larrygram_model::{Document, TextContent, TextPart};

    fn app_with_text_shape() -> (larrygram_commands::AppCtx, ShapeId) {
        let mut doc = Document::new();
        let page = doc.add_page("Page-1").expect("page");
        let mut shape = larrygram_model::Shape::new(ShapeId { index: 0 }, 1);
        shape.sheet.cells = vec![
            {
                let mut cell = larrygram_model::Cell::new(larrygram_model::CellName::new("Width"));
                cell.value = Some(larrygram_model::CellValue::Number(2.0));
                cell
            },
            {
                let mut cell = larrygram_model::Cell::new(larrygram_model::CellName::new("Height"));
                cell.value = Some(larrygram_model::CellValue::Number(1.0));
                cell
            },
            {
                let mut cell = larrygram_model::Cell::new(larrygram_model::CellName::new("PinX"));
                cell.value = Some(larrygram_model::CellValue::Number(3.0));
                cell
            },
            {
                let mut cell = larrygram_model::Cell::new(larrygram_model::CellName::new("PinY"));
                cell.value = Some(larrygram_model::CellValue::Number(3.0));
                cell
            },
            {
                let mut cell =
                    larrygram_model::Cell::new(larrygram_model::CellName::new("LocPinX"));
                cell.value = Some(larrygram_model::CellValue::Number(1.0));
                cell
            },
            {
                let mut cell =
                    larrygram_model::Cell::new(larrygram_model::CellName::new("LocPinY"));
                cell.value = Some(larrygram_model::CellValue::Number(0.5));
                cell
            },
            {
                let mut cell =
                    larrygram_model::Cell::new(larrygram_model::CellName::new("FillPattern"));
                cell.value = Some(larrygram_model::CellValue::Number(1.0));
                cell
            },
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
            let mut row = larrygram_model::Row::new();
            row.row_type = Some(kind.to_owned());
            row.ix = Some(i32::try_from(ix).unwrap_or(0));
            row.cells = vec![
                {
                    let mut cell = larrygram_model::Cell::new(larrygram_model::CellName::new("X"));
                    cell.value = Some(larrygram_model::CellValue::Number(px));
                    cell
                },
                {
                    let mut cell = larrygram_model::Cell::new(larrygram_model::CellName::new("Y"));
                    cell.value = Some(larrygram_model::CellValue::Number(py));
                    cell
                },
            ];
            geometry.rows.push(row);
        }
        shape.sheet.sections.push(geometry);
        shape.text = Some(TextContent {
            parts: vec![TextPart::Text("Hi".to_owned())],
        });
        let shape = doc.insert_shape(page, shape, None).expect("shape");
        let mut app = larrygram_commands::AppCtx::new(Settings::default());
        app.open_document(DocumentSession::new(doc, None));
        (app, shape)
    }

    #[test]
    fn a_click_on_a_text_object_edits_its_text() {
        let (ctx, shape) = app_with_text_shape();
        let mut machine = super::super::ToolMachine::new();
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
        assert_eq!(effect.intents[1].command.as_str(), "text.begin");
        assert_eq!(state_pending(&machine), None);
        let _ = shape;
    }

    #[test]
    fn a_click_on_empty_page_starts_a_pending_box() {
        let (ctx, _) = app_with_text_shape();
        let mut machine = super::super::ToolMachine::new();
        let effect = handle(
            &mut machine,
            &ToolEvent::PointerUp {
                pos: Point::new(0.5, 0.5),
                modifiers: super::super::Modifiers::none(),
            },
            &ctx,
        )
        .expect("handle");
        assert!(effect.intents.is_empty(), "nothing is inserted yet");
        let pending = state_pending(&machine).expect("a pending box");
        assert_eq!(pending.x, 0.5);
        assert_eq!(pending.y, 0.5);
        assert_eq!(pending.width, TEXT_BOX_WIDTH_IN);
        assert_eq!(pending.height, TEXT_BOX_HEIGHT_IN);
    }

    #[test]
    fn cancel_drops_the_pending_box_and_a_second_click_replaces_it() {
        let (ctx, _) = app_with_text_shape();
        let mut machine = super::super::ToolMachine::new();
        for event in [
            ToolEvent::PointerUp {
                pos: Point::new(0.5, 0.5),
                modifiers: super::super::Modifiers::none(),
            },
            ToolEvent::Cancel,
        ] {
            handle(&mut machine, &event, &ctx).expect("handle");
        }
        assert_eq!(state_pending(&machine), None);
        handle(
            &mut machine,
            &ToolEvent::PointerUp {
                pos: Point::new(1.5, 1.5),
                modifiers: super::super::Modifiers::none(),
            },
            &ctx,
        )
        .expect("handle");
        let pending = state_pending(&machine).expect("a pending box");
        assert_eq!((pending.x, pending.y), (1.5, 1.5));
    }

    #[test]
    fn no_document_or_other_events_do_nothing() {
        let ctx = larrygram_commands::AppCtx::new(Settings::default());
        let mut machine = super::super::ToolMachine::new();
        let effect = handle(
            &mut machine,
            &ToolEvent::PointerUp {
                pos: Point::new(0.0, 0.0),
                modifiers: super::super::Modifiers::none(),
            },
            &ctx,
        )
        .expect("handle");
        assert!(effect.intents.is_empty());

        let (ctx, _) = app_with_text_shape();
        for event in [
            ToolEvent::PointerDown {
                pos: Point::new(0.0, 0.0),
                modifiers: super::super::Modifiers::none(),
            },
            ToolEvent::PointerMove {
                pos: Point::new(1.0, 1.0),
                modifiers: super::super::Modifiers::none(),
            },
            ToolEvent::DoubleClick {
                pos: Point::new(3.0, 3.0),
            },
        ] {
            let effect = handle(&mut machine, &event, &ctx).expect("handle");
            assert!(effect.intents.is_empty(), "{event:?}");
        }
        assert_eq!(anchor(&machine), None);
    }

    #[test]
    fn the_static_ids_are_valid() {
        assert!(intent("text.begin", Args::new()).is_ok());
        let (ctx, shape) = app_with_text_shape();
        let session = ctx.active_document().expect("session");
        assert!(select_intent(&session.document, shape).is_ok());
    }

    /// The pending box of the machine's text state (the test seam).
    fn state_pending(machine: &super::super::ToolMachine) -> Option<PendingBox> {
        machine.text.pending
    }
}
