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
//! - `tools.text` — text tool;
//! - `tools.text_block` — text block tool (task P7-25 item 4): the
//!   same enablement as the other tools (its `contexts.yaml` row), and
//!   the drag it leads with acts on the selection's text block.
//!
//! The registry has no checked-state seam yet, so a command cannot
//! report itself as checked; the shell derives the command bar and menu
//! state from [`AppCtx::active_tool`] (Open issue in the P5-17 Result).

use crate::app::ActiveTool;
use crate::{Invocation, Outcome};

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
fn choose(ctx: &mut crate::AppCtx, tool: ActiveTool) -> Result<Outcome, super::CommandError> {
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

/// `tools.text`.
fn run_text(ctx: &mut crate::AppCtx, _inv: &Invocation) -> Result<Outcome, super::CommandError> {
    choose(ctx, ActiveTool::Text)
}

/// `tools.text_block` (task P7-25 item 4).
fn run_text_block(
    ctx: &mut crate::AppCtx,
    _inv: &Invocation,
) -> Result<Outcome, super::CommandError> {
    choose(ctx, ActiveTool::TextBlock)
}
