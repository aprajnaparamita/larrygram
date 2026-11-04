//! The connector style controls and the disabled line jump controls
//! (task P7-36 items 3 and 4).
//!
//! The style controls dispatch [`connector::run_style`] with their own
//! style, so every entry point — the Panel Modern Design > Connectors
//! gallery, the connector context menu and the Panel Classic Layout &
//! Routing toolbar — writes the same cells in one undoable edit (ADR
//! 0084 rule 8).
//!
//! Line jumps are out of scope for the alpha (ADR 0084): the Show Line
//! Jumps, Line Jump Style and Add Line Jumps To controls stay disabled
//! with a tooltip in our own words.
//!
//! Part of Larrygram. Alpha.

use crate::app::AppCtx;
use crate::error::CommandError;
use crate::{ArgValue, Args, Enabled, Invocation, Outcome};

use super::connector;
use super::{CommandTable, has_pages};

/// The tooltip of the line jump controls (task P7-36 item 4): our own
/// words, never Microsoft's.
pub(crate) const LINE_JUMBS_REASON: &str = "Line jumps aren't available yet";

/// The style controls and the disabled line jump controls (task P7-36
/// items 3 and 4). The labels are fallbacks; `commands.yaml`'s rows
/// win.
pub(crate) const TABLE: CommandTable = &[
    (
        "connector.right_angle",
        "Right-Angle Connector",
        style_enabled,
        run_right_angle,
    ),
    (
        "connector.straight.lines",
        "Straight Lines",
        style_enabled,
        run_straight,
    ),
    (
        "connector.curved.lines",
        "Curved Lines",
        style_enabled,
        run_curved,
    ),
    (
        "connector.straight",
        "Straight Connector",
        style_enabled,
        run_straight,
    ),
    (
        "connector.curved",
        "Curved Connector",
        style_enabled,
        run_curved,
    ),
    (
        "format.set.dyn.conn.routing.style",
        "Style",
        style_enabled,
        run_right_angle,
    ),
    (
        "format.set.dyn.conn.appearance.straight",
        "Straight",
        style_enabled,
        run_straight,
    ),
    (
        "format.set.dyn.conn.appearance.curved",
        "Curved",
        style_enabled,
        run_curved,
    ),
    (
        "connector.show.line.jumps",
        "Show Line Jumps",
        line_jumps_disabled,
        run_line_jumps,
    ),
    (
        "connector.line_jump_style",
        "Line Jump Style",
        line_jumps_disabled,
        run_line_jumps,
    ),
    (
        "connector.line_jump_style.page_default",
        "Page Default",
        line_jumps_disabled,
        run_line_jumps,
    ),
    (
        "connector.line_jump_style.arc",
        "Arc",
        line_jumps_disabled,
        run_line_jumps,
    ),
    (
        "connector.line_jump_style.gap",
        "Gap",
        line_jumps_disabled,
        run_line_jumps,
    ),
    (
        "connector.line_jump_style.square",
        "Square",
        line_jumps_disabled,
        run_line_jumps,
    ),
    (
        "connector.line_jump_style.sides_2",
        "2 Sides",
        line_jumps_disabled,
        run_line_jumps,
    ),
    (
        "connector.line_jump_style.sides_3",
        "3 Sides",
        line_jumps_disabled,
        run_line_jumps,
    ),
    (
        "connector.line_jump_style.sides_4",
        "4 Sides",
        line_jumps_disabled,
        run_line_jumps,
    ),
    (
        "connector.line_jump_style.sides_5",
        "5 Sides",
        line_jumps_disabled,
        run_line_jumps,
    ),
    (
        "connector.line_jump_style.sides_6",
        "6 Sides",
        line_jumps_disabled,
        run_line_jumps,
    ),
    (
        "connector.line_jump_style.sides_7",
        "7 Sides",
        line_jumps_disabled,
        run_line_jumps,
    ),
    (
        "connector.add_line_jumps_to",
        "Add Line Jumps To",
        line_jumps_disabled,
        run_line_jumps,
    ),
    (
        "connector.add_line_jumps_to.no_lines",
        "No Lines",
        line_jumps_disabled,
        run_line_jumps,
    ),
    (
        "connector.add_line_jumps_to.horizontal_lines",
        "Horizontal Lines",
        line_jumps_disabled,
        run_line_jumps,
    ),
    (
        "connector.add_line_jumps_to.vertical_lines",
        "Vertical Lines",
        line_jumps_disabled,
        run_line_jumps,
    ),
    (
        "connector.add_line_jumps_to.last_routed_line",
        "Last Routed Line",
        line_jumps_disabled,
        run_line_jumps,
    ),
    (
        "connector.add_line_jumps_to.last_displayed_line",
        "Last Displayed Line",
        line_jumps_disabled,
        run_line_jumps,
    ),
    (
        "connector.add_line_jumps_to.first_displayed_line",
        "First Displayed Line",
        line_jumps_disabled,
        run_line_jumps,
    ),
];

/// The style controls' enabled state (task P7-36 item 3): with
/// nothing selected they set the page default, and with a styleable
/// connector selected they set its style; any other selection has no
/// connector to style.
fn style_enabled(ctx: &AppCtx) -> Enabled {
    if ctx.selection.shapes.is_empty() {
        return has_pages(ctx);
    }
    let styled = ctx.active_document().is_some_and(|session| {
        session.active_page.is_some_and(|page| {
            !connector::style_targets(&session.document, page, &ctx.selection.shapes).is_empty()
        })
    });
    if styled {
        Enabled::yes()
    } else {
        Enabled::no("no connector in the selection to style")
    }
}

/// The style controls dispatch `connector.style` with their own style
/// (task P7-36 item 3): the context-menu items act on the selection,
/// and a gallery with nothing selected sets the page default (the
/// `connector.style` rule).
fn dispatch_style(
    ctx: &mut AppCtx,
    inv: &Invocation,
    style: &'static str,
) -> Result<Outcome, CommandError> {
    let args = Args::new().with("style", ArgValue::Text(style.to_owned()));
    connector::run_style(ctx, &Invocation::new(args, inv.source))
}

fn run_right_angle(ctx: &mut AppCtx, inv: &Invocation) -> Result<Outcome, CommandError> {
    dispatch_style(ctx, inv, "right_angle")
}

fn run_straight(ctx: &mut AppCtx, inv: &Invocation) -> Result<Outcome, CommandError> {
    dispatch_style(ctx, inv, "straight")
}

fn run_curved(ctx: &mut AppCtx, inv: &Invocation) -> Result<Outcome, CommandError> {
    dispatch_style(ctx, inv, "curved")
}

/// The line jump controls are disabled (task P7-36 item 4): the
/// tooltip says line jumps aren't available yet, in our own words.
fn line_jumps_disabled(_ctx: &AppCtx) -> crate::Enabled {
    crate::Enabled::no(LINE_JUMBS_REASON)
}

/// The line jump controls never run (task P7-36 item 4): the registry
/// refuses them first, and the body fails cleanly if it is reached.
fn run_line_jumps(_ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
    Err(CommandError::failed(LINE_JUMBS_REASON))
}
