//! `tools.*` commands: choosing the active interaction tool (task
//! P5-17).
//!
//! Choosing a tool is a user action (AGENTS.md section 6), so every
//! tool id dispatches through the one registry. The handlers only set
//! [`AppCtx::active_tool`]; they add no undo step and do not bump the
//! document change counter (a tool choice does not edit the page).
//! `crates/ui` reads the field each frame and mirrors it into its
//! `ToolMachine`, which also cancels any drag in flight when the tool
//! changes (ADR 0042).
//!
//! The aliases:
//!
//! - `tools.pointer` — pointer tool;
//! - `tools.rectangle` and `shape.rectangle` — rectangle tool (the
//!   command bar uses the first, `Ctrl+8` the second);
//! - `tools.ellipse` and `shape.ellipse` — ellipse tool;
//! - `shape.line` — line tool (there is no `tools.line` row);
//! - `tools.connector` — connector tool;
//! - `tools.text` — text tool (task P7-29: chosen with exactly one
//!   shape selected, it starts that shape's text; otherwise it only
//!   arms the tool);
//! - `tools.text_block` — text block tool (task P7-25 item 4): the
//!   same enablement as the other tools (its `contexts.yaml` row), and
//!   the drag it leads with acts on the selection's text block.
//!
//! The registry has no checked-state seam yet, so a command cannot
//! report itself as checked; the shell derives the command bar and menu
//! state from [`AppCtx::active_tool`] (Open issue in the P5-17 Result).

use crate::app::ActiveTool;
use crate::{Invocation, Outcome, UiRequest};

use super::{CommandTable, has_document, outcome};

/// The implemented tool-choice commands (task P5-17).
pub(crate) const TABLE: CommandTable = &[
    ("tools.pointer", "Pointer Tool", has_document, run_pointer),
    ("tools.rectangle", "Rectangle", has_document, run_rectangle),
    ("shape.rectangle", "Rectangle", has_document, run_rectangle),
    ("tools.ellipse", "Ellipse", has_document, run_ellipse),
    ("shape.ellipse", "Ellipse", has_document, run_ellipse),
    ("shape.line", "Line", has_document, run_line),
    (
        "tools.connector",
        "Connector Tool",
        has_document,
        run_connector,
    ),
    ("tools.text", "Text Tool", has_document, run_text),
    (
        "tools.text_block",
        "Text Block Tool",
        has_document,
        run_text_block,
    ),
];

/// Sets the active tool and reports it.
///
/// A recorded Format Painter copy never outlives a tool change (task
/// P7-26 item 4): choosing another tool clears it, as in Visio.
fn choose(ctx: &mut crate::AppCtx, tool: ActiveTool) -> Result<Outcome, super::CommandError> {
    if ctx.active_tool != tool {
        ctx.format_painter = None;
    }
    ctx.active_tool = tool;
    Ok(outcome(format!("{} tool", tool.name()), Vec::new()))
}

/// `tools.pointer`.
fn run_pointer(ctx: &mut crate::AppCtx, _inv: &Invocation) -> Result<Outcome, super::CommandError> {
    choose(ctx, ActiveTool::Pointer)
}

/// `tools.rectangle` and `shape.rectangle`.
fn run_rectangle(
    ctx: &mut crate::AppCtx,
    _inv: &Invocation,
) -> Result<Outcome, super::CommandError> {
    choose(ctx, ActiveTool::Rectangle)
}

/// `tools.ellipse` and `shape.ellipse`.
fn run_ellipse(ctx: &mut crate::AppCtx, _inv: &Invocation) -> Result<Outcome, super::CommandError> {
    choose(ctx, ActiveTool::Ellipse)
}

/// `shape.line`.
fn run_line(ctx: &mut crate::AppCtx, _inv: &Invocation) -> Result<Outcome, super::CommandError> {
    choose(ctx, ActiveTool::Line)
}

/// `tools.connector`.
fn run_connector(
    ctx: &mut crate::AppCtx,
    _inv: &Invocation,
) -> Result<Outcome, super::CommandError> {
    choose(ctx, ActiveTool::Connector)
}

/// `tools.text` (task P7-29 item 2): the choice arms the Text tool,
/// and with exactly one live shape selected it also starts editing
/// that shape's text (the `EditText` request opens the UI's editor
/// with the caret at the end). Nothing selected, or several shapes:
/// the choice only arms the tool.
fn run_text(ctx: &mut crate::AppCtx, _inv: &Invocation) -> Result<Outcome, super::CommandError> {
    let edit_one = one_selected_shape(ctx);
    let mut result = choose(ctx, ActiveTool::Text)?;
    if edit_one {
        result.ui_request = Some(UiRequest::EditText);
    }
    Ok(result)
}

/// Whether the selection names exactly one live shape (task P7-29
/// item 2's editor target).
fn one_selected_shape(ctx: &crate::AppCtx) -> bool {
    let [shape] = ctx.selection.shapes.as_slice() else {
        return false;
    };
    ctx.active_document()
        .is_some_and(|session| session.document.page_of(*shape).is_some())
}

/// `tools.text_block` (task P7-25 item 4).
fn run_text_block(
    ctx: &mut crate::AppCtx,
    _inv: &Invocation,
) -> Result<Outcome, super::CommandError> {
    choose(ctx, ActiveTool::TextBlock)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{DocumentSession, PaintSource, Settings};
    use larrygram_model::ShapeId;

    /// A tool change clears a recorded Format Painter copy (task
    /// P7-26 item 4: a copy never outlives a tool change); re-choosing
    /// the active tool is no change and keeps it.
    #[test]
    fn a_tool_change_clears_the_format_painter_copy() {
        let mut ctx = crate::AppCtx::new(Settings::default());
        let mut doc = larrygram_model::Document::new();
        doc.add_page("Page-1").expect("page");
        let session = DocumentSession::new(doc, None);
        let session_id = session.id;
        ctx.open_document(session);
        ctx.format_painter = Some(PaintSource {
            session: session_id,
            shape: ShapeId { index: 0 },
        });
        choose(&mut ctx, ActiveTool::Pointer).expect("pointer");
        assert!(
            ctx.format_painter.is_some(),
            "re-choosing the active tool keeps the copy"
        );
        choose(&mut ctx, ActiveTool::Rectangle).expect("rectangle");
        assert!(
            ctx.format_painter.is_none(),
            "a tool change clears the copy"
        );
    }

    /// A document with `count` selected shapes.
    fn app_with_shapes(count: usize) -> (crate::AppCtx, Vec<ShapeId>) {
        let mut doc = larrygram_model::Document::new();
        let page = doc.add_page("Page-1").expect("page");
        let mut ids = Vec::new();
        for index in 0..count {
            let visio_id = i32::try_from(index + 1).unwrap_or(1);
            let shape = larrygram_model::Shape::new(ShapeId { index: 0 }, visio_id);
            ids.push(doc.insert_shape(page, shape, None).expect("shape"));
        }
        let mut ctx = crate::AppCtx::new(Settings::default());
        ctx.open_document(DocumentSession::new(doc, None));
        ctx.selection.shapes = ids.clone();
        (ctx, ids)
    }

    /// Task P7-29 item 2: `tools.text` with exactly one selected
    /// shape asks for the in-place editor on it (and still arms the
    /// tool); with nothing or several shapes it only arms the tool.
    #[test]
    fn the_text_tool_on_one_selected_shape_requests_the_editor() {
        let (mut ctx, ids) = app_with_shapes(1);
        let invocation = Invocation::empty(crate::Source::Test);
        let outcome = run_text(&mut ctx, &invocation).expect("text tool");
        assert_eq!(ctx.active_tool, ActiveTool::Text, "the tool is armed");
        assert_eq!(
            outcome.ui_request,
            Some(UiRequest::EditText),
            "one selected shape starts its text"
        );

        let (mut ctx, _) = app_with_shapes(0);
        let outcome = run_text(&mut ctx, &invocation).expect("text tool");
        assert_eq!(ctx.active_tool, ActiveTool::Text);
        assert_eq!(outcome.ui_request, None, "nothing selected: arm only");

        let (mut ctx, _) = app_with_shapes(2);
        let outcome = run_text(&mut ctx, &invocation).expect("text tool");
        assert_eq!(outcome.ui_request, None, "several shapes: arm only");

        // A single stale selection is not an editor target.
        let (mut ctx, _) = app_with_shapes(1);
        ctx.selection.shapes = vec![ShapeId { index: 99 }];
        let outcome = run_text(&mut ctx, &invocation).expect("text tool");
        assert_eq!(outcome.ui_request, None, "a stale shape: arm only");
        assert_eq!(ids.len(), 1);
    }
}
