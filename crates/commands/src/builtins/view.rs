//! The `edit`, `view`, `page` and `help` commands (ADR 0028): undo,
//! zoom, the grid and rulers toggles, the display units, the command bar
//! layout switch (ADR 0032), page navigation, the Options request and
//! the About request.
//!
//! Every per-document view command works on the active document's
//! [`ViewState`] (ADR 0029). The grid and rulers toggles always run:
//! without an open document they flip the [`Settings`](crate::Settings)
//! default and say so (ADR 0032 amendment), the other view commands
//! are disabled with a reason instead of doing nothing.

use larrygram_model::{CellValue, Page};

use crate::app::AppCtx;
use crate::error::CommandError;
use crate::{Invocation, Outcome, UiRequest};

use super::connector::reroute_quietly_on;
use super::{no_document, outcome};

/// The zoom change of one `view.zoom.in` / `view.zoom.out` step.
const ZOOM_STEP: f64 = 1.25;

/// The lowest zoom (5%) the view commands and the canvas allow (ADR
/// 0029: `zoom > 0`); our own range choice.
pub const ZOOM_MIN: f64 = 0.05;

/// The highest zoom (4000%) the view commands and the canvas allow.
pub const ZOOM_MAX: f64 = 40.0;

/// Canvas points per page inch at zoom 1, the scale the canvas draws
/// at (ADR 0025: 96 pixels per inch). The fit commands divide by it.
const POINTS_PER_INCH: f64 = 96.0;

// The page size of a sheet with no positive `PageWidth` or
// `PageHeight`: the renderer's own fallback.
use larrygram_render::FALLBACK_PAGE_SIZE;

/// `edit.undo`: reverts the newest edit of the active document.
pub(super) fn run_undo(ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
    let page = ctx
        .active_document()
        .and_then(|session| session.active_page);
    match with_undo(ctx, |stack, ctx| stack.undo(ctx))? {
        Some(label) => {
            reroute_after_step(ctx, page);
            Ok(outcome(format!("Undo {label}"), Vec::new()))
        }
        None => Ok(outcome("Nothing to undo", Vec::new())),
    }
}

/// `edit.redo`: re-applies the newest redoable edit.
pub(super) fn run_redo(ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
    let page = ctx
        .active_document()
        .and_then(|session| session.active_page);
    match with_undo(ctx, |stack, ctx| stack.redo(ctx))? {
        Some(label) => {
            reroute_after_step(ctx, page);
            Ok(outcome(format!("Redo {label}"), Vec::new()))
        }
        None => Ok(outcome("Nothing to redo", Vec::new())),
    }
}

/// Re-routes the glued connectors after an undo or redo step (ADR 0045).
fn reroute_after_step(ctx: &mut AppCtx, page: Option<larrygram_model::PageId>) {
    if let Some(page) = page {
        reroute_quietly_on(ctx, page);
    }
}

/// Runs `step` on the active document's undo stack.
///
/// The stack lives inside [`AppCtx`] (ADR 0029) but the step needs
/// `&mut AppCtx` for its [`Edit`](crate::Edit)s, so it is taken out
/// for the call and put back afterwards. The placeholder is an
/// explicit zero-capacity stack (P4-15 review minor 2).
///
/// The stack goes back onto the session it came from, found by its
/// [`SessionId`](crate::SessionId), even when the step opens or closes
/// documents and the index moves (P4-15 review minor 3). When the step
/// closed its own session, the stack dies with it.
fn with_undo(
    ctx: &mut AppCtx,
    step: fn(&mut crate::undo::UndoStack, &mut AppCtx) -> Result<Option<String>, CommandError>,
) -> Result<Option<String>, CommandError> {
    let Some(index) = ctx.active else {
        return Err(no_document());
    };
    let Some(session) = ctx.documents.get_mut(index) else {
        return Err(no_document());
    };
    let id = session.id;
    let mut stack = std::mem::replace(&mut session.undo, crate::undo::UndoStack::new(0));
    let result = step(&mut stack, ctx);
    if let Some(session) = ctx.documents.iter_mut().find(|session| session.id == id) {
        session.undo = stack;
    }
    result
}

/// `view.zoom.in`: multiplies the zoom by [`ZOOM_STEP`].
pub(super) fn run_zoom_in(ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
    zoom_by(ctx, ZOOM_STEP)
}

/// `view.zoom.out`: divides the zoom by [`ZOOM_STEP`].
pub(super) fn run_zoom_out(ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
    zoom_by(ctx, 1.0 / ZOOM_STEP)
}

/// Sets the zoom of the active document, clamped to
/// [`ZOOM_MIN`]..=[`ZOOM_MAX`].
fn zoom_by(ctx: &mut AppCtx, factor: f64) -> Result<Outcome, CommandError> {
    let session = ctx.active_document_mut().ok_or_else(no_document)?;
    let zoom = (session.view.zoom * factor).clamp(ZOOM_MIN, ZOOM_MAX);
    session.view.zoom = zoom;
    Ok(outcome(format!("Zoom {:.0}%", zoom * 100.0), Vec::new()))
}

/// Sets the zoom of the active document to `zoom` (a multiplier, 1.0 =
/// 100%), clamped to [`ZOOM_MIN`]..=[`ZOOM_MAX`].
fn zoom_to(ctx: &mut AppCtx, zoom: f64) -> Result<Outcome, CommandError> {
    let session = ctx.active_document_mut().ok_or_else(no_document)?;
    let zoom = zoom.clamp(ZOOM_MIN, ZOOM_MAX);
    session.view.zoom = zoom;
    Ok(outcome(format!("Zoom {:.0}%", zoom * 100.0), Vec::new()))
}

/// `view.view50` .. `view.view400`: the zoom presets of the Classic
/// View > Zoom submenu (task P7-20).
pub(super) fn run_zoom_50(ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
    zoom_to(ctx, 0.5)
}

/// `view.view75`: 75%.
pub(super) fn run_zoom_75(ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
    zoom_to(ctx, 0.75)
}

/// `view.view100` ("Zoom to 100%"): the actual size.
pub(super) fn run_zoom_100(ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
    zoom_to(ctx, 1.0)
}

/// `view.view150`: 150%.
pub(super) fn run_zoom_150(ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
    zoom_to(ctx, 1.5)
}

/// `view.view200`: 200%.
pub(super) fn run_zoom_200(ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
    zoom_to(ctx, 2.0)
}

/// `view.view400`: 400%.
pub(super) fn run_zoom_400(ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
    zoom_to(ctx, 4.0)
}

/// `shape.draw.zoom` (the zoom percentage box), `view.zoom.dialog` and
/// `view.custom` (the Zoom dialogs' argument form): sets the zoom from
/// the `percent` argument (a number like `150`, clamped to
/// [`ZOOM_MIN`]..=[`ZOOM_MAX`]).
///
/// The dialog body is a later task's; this command takes its answer as
/// arguments and never opens one (AGENTS.md section 6). A missing
/// value is [`CommandError::NeedsInput`], an invalid one an argument
/// error naming the expected form.
pub(super) fn run_zoom_value(ctx: &mut AppCtx, inv: &Invocation) -> Result<Outcome, CommandError> {
    let percent = match inv.args.get("percent") {
        None => {
            return Err(CommandError::NeedsInput(crate::NeededInput::Number {
                prompt: "zoom percent (100 = actual size)".to_owned(),
                default: Some(100.0),
            }));
        }
        Some(crate::ArgValue::Number(value)) if value.is_finite() && *value > 0.0 => *value,
        Some(crate::ArgValue::Integer(value)) if *value > 0 => *value as f64,
        Some(_) => {
            return Err(CommandError::invalid_args(
                "percent must be a finite positive number",
            ));
        }
    };
    zoom_to(ctx, percent / 100.0)
}

/// `view.zoom.page.width`: zooms so the page width fills the canvas
/// `width` (points), with the same [`ViewState::viewport`] fallback as
/// [`run_zoom_fit`] (task P7-20). Only the width matters here, so
/// only it is asked for.
pub(super) fn run_zoom_page_width(
    ctx: &mut AppCtx,
    inv: &Invocation,
) -> Result<Outcome, CommandError> {
    let width = match optional_positive(&inv.args, "width")? {
        Some(width) => width,
        None => ctx
            .active_document()
            .and_then(|session| session.view.viewport)
            .map(|(width, _)| width)
            .ok_or_else(|| {
                CommandError::NeedsInput(crate::NeededInput::Number {
                    prompt: "canvas width in points".to_owned(),
                    default: None,
                })
            })?,
    };
    let session = ctx.active_document_mut().ok_or_else(no_document)?;
    let page_id = session
        .active_page
        .ok_or_else(|| CommandError::failed("no active page"))?;
    let page = session
        .document
        .pages
        .iter()
        .find(|page| page.id == page_id)
        .ok_or_else(|| CommandError::failed("no active page"))?;
    let (page_width, _) = page_size(page);
    let fit = (width / (page_width * POINTS_PER_INCH)).clamp(ZOOM_MIN, ZOOM_MAX);
    session.view.zoom = fit;
    Ok(outcome(format!("Zoom {:.0}%", fit * 100.0), Vec::new()))
}

/// `view.zoom.fit` and `view.fit.in.window`: zooms so the active page
/// fits the canvas rectangle given by the `width` and `height`
/// arguments, in points (ADR 0028).
///
/// Task P7-20 extension: `width`/`height` are optional; without them
/// the fit uses the last measured [`ViewState::viewport`] of the
/// active document (the host records it each frame). With neither, the
/// command asks for the number ([`CommandError::NeedsInput`], the
/// documented contract: a programmatic caller passes the rectangle).
pub(super) fn run_zoom_fit(ctx: &mut AppCtx, inv: &Invocation) -> Result<Outcome, CommandError> {
    let (width, height) = viewport_size(ctx, &inv.args, "width", "height")?;
    let session = ctx.active_document_mut().ok_or_else(no_document)?;
    let page_id = session
        .active_page
        .ok_or_else(|| CommandError::failed("no active page"))?;
    let page = session
        .document
        .pages
        .iter()
        .find(|page| page.id == page_id)
        .ok_or_else(|| CommandError::failed("no active page"))?;
    let (page_width, page_height) = page_size(page);
    let fit = ((width / (page_width * POINTS_PER_INCH))
        .min(height / (page_height * POINTS_PER_INCH)))
    .clamp(ZOOM_MIN, ZOOM_MAX);
    session.view.zoom = fit;
    Ok(outcome(format!("Zoom {:.0}%", fit * 100.0), Vec::new()))
}

/// One optional, finite, positive number argument (an integer counts).
fn optional_positive(args: &crate::Args, name: &str) -> Result<Option<f64>, CommandError> {
    match args.get(name) {
        None => Ok(None),
        Some(crate::ArgValue::Number(value)) if value.is_finite() && *value > 0.0 => {
            Ok(Some(*value))
        }
        Some(crate::ArgValue::Integer(value)) if *value > 0 => Ok(Some(*value as f64)),
        Some(_) => Err(CommandError::invalid_args(format!(
            "{name} must be finite and positive"
        ))),
    }
}

/// The canvas rectangle of one fit: the `width` and `height` arguments
/// when given, else the active document's last measured
/// [`ViewState::viewport`], else [`CommandError::NeedsInput`] naming
/// the missing side (task P7-20).
fn viewport_size(
    ctx: &AppCtx,
    args: &crate::Args,
    width_name: &str,
    height_name: &str,
) -> Result<(f64, f64), CommandError> {
    let width = optional_positive(args, width_name)?;
    let height = optional_positive(args, height_name)?;
    let measured = ctx
        .active_document()
        .and_then(|session| session.view.viewport);
    match (width, height) {
        (Some(width), Some(height)) => Ok((width, height)),
        (Some(width), None) => match measured {
            Some((_, height)) => Ok((width, height)),
            None => Err(CommandError::NeedsInput(crate::NeededInput::Number {
                prompt: "canvas height in points".to_owned(),
                default: None,
            })),
        },
        (None, Some(height)) => match measured {
            Some((width, _)) => Ok((width, height)),
            None => Err(CommandError::NeedsInput(crate::NeededInput::Number {
                prompt: "canvas width in points".to_owned(),
                default: None,
            })),
        },
        (None, None) => match measured {
            Some((width, height)) => Ok((width, height)),
            None => Err(CommandError::NeedsInput(crate::NeededInput::Number {
                prompt: "canvas width in points".to_owned(),
                default: None,
            })),
        },
    }
}

/// The page size in inches, mirroring the renderer's per-axis fallback.
pub(super) fn page_size(page: &Page) -> (f64, f64) {
    (
        positive_cell(page, "PageWidth", FALLBACK_PAGE_SIZE.0),
        positive_cell(page, "PageHeight", FALLBACK_PAGE_SIZE.1),
    )
}

/// The positive cached number of one page-sheet cell, or `fallback`.
fn positive_cell(page: &Page, name: &str, fallback: f64) -> f64 {
    page.sheet
        .cells
        .iter()
        .find(|cell| cell.name.as_str() == name)
        .and_then(|cell| match &cell.value {
            Some(CellValue::Number(value)) if value.is_finite() && *value > 0.0 => Some(*value),
            _ => None,
        })
        .unwrap_or(fallback)
}

/// `view.toggle_grid`: flips the grid of the active document and
/// writes the new value to the settings default (ADR 0032 amendment).
pub(super) fn run_toggle_grid(
    ctx: &mut AppCtx,
    _inv: &Invocation,
) -> Result<Outcome, CommandError> {
    toggle(ctx, "Grid", Which::Grid)
}

/// `view.rulers`: flips the rulers of the active document and writes
/// the new value to the settings default (ADR 0032 amendment).
pub(super) fn run_toggle_rulers(
    ctx: &mut AppCtx,
    _inv: &Invocation,
) -> Result<Outcome, CommandError> {
    toggle(ctx, "Rulers", Which::Rulers)
}

/// Which of the two view toggles a `view.toggle_grid` / `view.rulers`
/// run flips.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Which {
    /// The grid.
    Grid,
    /// The rulers.
    Rulers,
}

/// Flips one toggle of the active [`ViewState`] and copies the new
/// value into the settings default, so new sessions inherit it (P4-15
/// review major 1). Without an open document only the settings default
/// flips, and the outcome says so.
fn toggle(ctx: &mut AppCtx, what: &str, which: Which) -> Result<Outcome, CommandError> {
    let mut flipped = None;
    if let Some(session) = ctx.active_document_mut() {
        let slot = match which {
            Which::Grid => &mut session.view.show_grid,
            Which::Rulers => &mut session.view.show_rulers,
        };
        *slot = !*slot;
        flipped = Some(*slot);
    }
    let default = match which {
        Which::Grid => &mut ctx.settings.show_grid,
        Which::Rulers => &mut ctx.settings.show_rulers,
    };
    let on = match flipped {
        Some(on) => {
            *default = on;
            on
        }
        None => {
            *default = !*default;
            *default
        }
    };
    let message = match flipped {
        Some(_) => format!("{what} {}", on_off_word(on)),
        None => format!("{what} {} (default for new drawings)", on_off_word(on)),
    };
    Ok(outcome(message, Vec::new()))
}

/// `on` or `off` for a boolean view toggle.
fn on_off_word(on: bool) -> &'static str {
    if on { "on" } else { "off" }
}

/// `view.guides` and `view.toggle_guides`: flips the guides of the
/// active document (task P7-20).
///
/// Like the grid and rulers it is a per-document view toggle; unlike
/// them it has no settings default (Visio keeps it in the document),
/// so without an open document the command is disabled instead of
/// flipping a default.
pub(super) fn run_toggle_guides(
    ctx: &mut AppCtx,
    _inv: &Invocation,
) -> Result<Outcome, CommandError> {
    toggle_view_flag(ctx, "Guides", ViewFlag::Guides)
}

/// `view.show.page.breaks` and `view.page.breaks`: flips the page
/// breaks of the active document (task P7-20), the same rule as the
/// guides.
pub(super) fn run_toggle_page_breaks(
    ctx: &mut AppCtx,
    _inv: &Invocation,
) -> Result<Outcome, CommandError> {
    toggle_view_flag(ctx, "Page Breaks", ViewFlag::PageBreaks)
}

/// Which per-document view flag one [`run_toggle_guides`] /
/// [`run_toggle_page_breaks`] run flips.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ViewFlag {
    /// The guides.
    Guides,
    /// The page breaks.
    PageBreaks,
}

/// Flips one per-document view flag (task P7-20).
fn toggle_view_flag(ctx: &mut AppCtx, what: &str, flag: ViewFlag) -> Result<Outcome, CommandError> {
    let session = ctx.active_document_mut().ok_or_else(no_document)?;
    let slot = match flag {
        ViewFlag::Guides => &mut session.view.show_guides,
        ViewFlag::PageBreaks => &mut session.view.show_page_breaks,
    };
    *slot = !*slot;
    Ok(outcome(
        format!("{what} {}", on_off_word(*slot)),
        Vec::new(),
    ))
}

/// `view.full_screen_mode`: turns the full-screen window mode on or
/// off (task P7-20; the Classic Full Screen item, the Modern
/// Presentation Mode button, and the F5 / Esc bindings of
/// `shortcuts.yaml`).
///
/// With the optional `show` argument (`"on"` or `"off"`, the
/// [`run_welcome_next_start`] spelling) the mode is set; without one
/// it toggles. The command only sets [`AppCtx::full_screen`]: the
/// shell reads the flag and draws the window accordingly (the tool
/// pattern, task P5-17), so a programmatic caller never gets a window
/// mode change it did not ask for.
pub(super) fn run_full_screen(ctx: &mut AppCtx, inv: &Invocation) -> Result<Outcome, CommandError> {
    let on = match inv.args.get("show") {
        None => !ctx.full_screen,
        Some(crate::ArgValue::Text(text)) => match text.as_str() {
            "on" => true,
            "off" => false,
            _ => {
                return Err(CommandError::invalid_args(format!(
                    "unknown show value {text:?}; expected one of on, off"
                )));
            }
        },
        Some(_) => {
            return Err(CommandError::invalid_args(
                "show must be text; expected one of on, off".to_owned(),
            ));
        }
    };
    ctx.full_screen = on;
    Ok(outcome(
        format!("Full screen {}", on_off_word(on)),
        Vec::new(),
    ))
}

/// `view.units`: sets the display units from the `units` argument, a
/// `DisplayUnits` name (P4-15, the ADR 0032 Options dialog).
///
/// A missing or unknown value is [`CommandError::InvalidArgs`] naming
/// the accepted values; the command itself is always enabled (units
/// are a setting, not document state).
pub(super) fn run_units(ctx: &mut AppCtx, inv: &Invocation) -> Result<Outcome, CommandError> {
    let names = crate::DisplayUnits::NAMES.join(", ");
    let text = match inv.args.get("units") {
        None => {
            return Err(CommandError::invalid_args(format!(
                "units is required; expected one of {names}"
            )));
        }
        Some(crate::ArgValue::Text(text)) => text.as_str(),
        Some(_) => {
            return Err(CommandError::invalid_args(format!(
                "units must be text; expected one of {names}"
            )));
        }
    };
    let Some(units) = crate::DisplayUnits::parse(text) else {
        return Err(CommandError::invalid_args(format!(
            "unknown units {text:?}; expected one of {names}"
        )));
    };
    ctx.settings.units = units;
    Ok(outcome(format!("Units: {}", units.name()), Vec::new()))
}

/// `help.welcome.next_start`: sets whether the welcome wizard opens on
/// the next start, from the `show` argument (`"on"` or `"off"`).
///
/// A settings flag, not a document action: it carries no undo, like
/// the other Options dialog rows (ADR 0032 amendment). A missing or
/// unknown value is [`CommandError::InvalidArgs`] naming the accepted
/// values; the command itself is always enabled.
pub(super) fn run_welcome_next_start(
    ctx: &mut AppCtx,
    inv: &Invocation,
) -> Result<Outcome, CommandError> {
    let show = match inv.args.get("show") {
        None => {
            return Err(CommandError::invalid_args(
                "show is required; expected one of on, off".to_owned(),
            ));
        }
        Some(crate::ArgValue::Text(text)) => match text.as_str() {
            "on" => true,
            "off" => false,
            _ => {
                return Err(CommandError::invalid_args(format!(
                    "unknown show value {text:?}; expected one of on, off"
                )));
            }
        },
        Some(_) => {
            return Err(CommandError::invalid_args(
                "show must be text; expected one of on, off".to_owned(),
            ));
        }
    };
    ctx.settings.welcome_seen = !show;
    Ok(outcome(
        format!("Welcome wizard on next start {}", on_off_word(show)),
        Vec::new(),
    ))
}

/// `app.office.center.options`: asks the dispatcher to open the
/// View/Options dialog (ADR 0032 amendment: the dialog body dispatches
/// commands; this handler just requests the dialog, never opens one).
pub(super) fn run_options(_ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
    Ok(Outcome {
        message: Some("Options".to_owned()),
        ui_request: Some(UiRequest::ShowOptions),
        ..Outcome::default()
    })
}

/// `view.ui_layout.panel_2016`: switches the command bar to the 2016 layout
/// (ADR 0032; always enabled, no restart).
pub(super) fn run_ui_layout_2016(
    ctx: &mut AppCtx,
    _inv: &Invocation,
) -> Result<Outcome, CommandError> {
    ctx.settings.ui_layout = larrygram_ui_spec::LayoutKind::Panel2016;
    Ok(outcome("Panel layout: Panel (2016 data)", Vec::new()))
}

/// `view.ui_layout.panel_365`: switches the command bar to the Microsoft
/// 365 layout (ADR 0032).
pub(super) fn run_ui_layout_365(
    ctx: &mut AppCtx,
    _inv: &Invocation,
) -> Result<Outcome, CommandError> {
    ctx.settings.ui_layout = larrygram_ui_spec::LayoutKind::Panel365;
    Ok(outcome("Panel layout: Panel (365 data)", Vec::new()))
}

/// `view.ui_layout.panel_classic`: switches the command bar to the
/// Panel Classic layout (ADR 0061; always enabled, no restart, like
/// the two Fluent-data layouts).
pub(super) fn run_ui_layout_panel_classic(
    ctx: &mut AppCtx,
    _inv: &Invocation,
) -> Result<Outcome, CommandError> {
    ctx.settings.ui_layout = larrygram_ui_spec::LayoutKind::PanelClassic;
    Ok(outcome("Panel layout: Panel Classic", Vec::new()))
}

/// `view.ui_layout.panel_modern`: switches the command bar to the
/// Panel Modern layout (ADR 0061; always enabled, no restart).
pub(super) fn run_ui_layout_panel_modern(
    ctx: &mut AppCtx,
    _inv: &Invocation,
) -> Result<Outcome, CommandError> {
    ctx.settings.ui_layout = larrygram_ui_spec::LayoutKind::PanelModern;
    Ok(outcome("Panel layout: Panel Modern", Vec::new()))
}

/// `page.next`: moves to the next page, wrapping at the end.
pub(super) fn run_page_next(ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
    move_page(ctx, 1)
}

/// `page.previous`: moves to the previous page, wrapping at the start.
pub(super) fn run_page_previous(
    ctx: &mut AppCtx,
    _inv: &Invocation,
) -> Result<Outcome, CommandError> {
    move_page(ctx, -1)
}

/// Moves the active page by `delta`, wrapping over the document's
/// pages.
fn move_page(ctx: &mut AppCtx, delta: i64) -> Result<Outcome, CommandError> {
    let session = ctx.active_document_mut().ok_or_else(no_document)?;
    let len = session.document.pages.len();
    let current = session
        .active_page
        .and_then(|id| session.document.pages.iter().position(|page| page.id == id));
    let index = match current {
        Some(index) => (index as i64 + delta).rem_euclid(len as i64) as usize,
        None if delta >= 0 => 0,
        None => len.saturating_sub(1),
    };
    // The one "no pages" check (task P5-19: the list lookup is the
    // guard): with no pages `index` is 0 and the lookup misses.
    let Some(page) = session.document.pages.get(index) else {
        return Err(CommandError::failed("the document has no pages"));
    };
    let number = index.saturating_add(1);
    let previous = session.active_page;
    session.active_page = Some(page.id);
    let label = if page.name.is_empty() {
        format!("Page {number}")
    } else {
        page.name.clone()
    };
    if previous != Some(page.id) {
        ctx.selection.clear();
    }
    Ok(outcome(label, Vec::new()))
}

/// `help.about`: asks the dispatcher to show the About window
/// (never a dialog from inside the command, AGENTS.md section 6).
pub(super) fn run_about(_ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
    Ok(Outcome {
        message: Some("About Larrygram".to_owned()),
        ui_request: Some(UiRequest::ShowAbout),
        ..Outcome::default()
    })
}

/// `help.welcome`: asks the dispatcher to show the welcome wizard
/// (ADR 0066; never a dialog from inside the command, AGENTS.md
/// section 6).
pub(super) fn run_welcome(_ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
    Ok(Outcome {
        message: Some("Welcome".to_owned()),
        ui_request: Some(UiRequest::ShowWelcome),
        ..Outcome::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{AppCtx, DocumentSession, Settings};
    use larrygram_model::{Cell, CellName, Page, PageId};

    #[test]
    fn undo_and_redo_without_history_say_so() {
        let mut ctx = AppCtx::new(Settings::default());
        ctx.open_document(DocumentSession::new(larrygram_model::Document::new(), None));
        let invocation = Invocation::empty(crate::Source::Test);
        let outcome = run_undo(&mut ctx, &invocation).expect("undo");
        assert_eq!(outcome.message.as_deref(), Some("Nothing to undo"));
        let outcome = run_redo(&mut ctx, &invocation).expect("redo");
        assert_eq!(outcome.message.as_deref(), Some("Nothing to redo"));
    }

    /// One `help.welcome.next_start` invocation with `show` set.
    fn welcome_invocation(show: &str) -> Invocation {
        let args = crate::Args::new().with("show", crate::ArgValue::Text(show.to_owned()));
        Invocation::new(args, crate::Source::Test)
    }

    #[test]
    fn welcome_next_start_sets_the_flag_from_show() {
        let mut ctx = AppCtx::new(Settings::default());
        ctx.settings.welcome_seen = true;
        let outcome = run_welcome_next_start(&mut ctx, &welcome_invocation("on")).expect("on");
        assert!(!ctx.settings.welcome_seen, "on: the wizard opens again");
        assert_eq!(
            outcome.message.as_deref(),
            Some("Welcome wizard on next start on")
        );
        let outcome = run_welcome_next_start(&mut ctx, &welcome_invocation("off")).expect("off");
        assert!(ctx.settings.welcome_seen, "off: the wizard stays seen");
        assert_eq!(
            outcome.message.as_deref(),
            Some("Welcome wizard on next start off")
        );
    }

    #[test]
    fn welcome_next_start_names_the_accepted_values() {
        let mut ctx = AppCtx::new(Settings::default());
        let error = run_welcome_next_start(&mut ctx, &Invocation::empty(crate::Source::Test))
            .expect_err("show is required");
        assert!(
            error.to_string().contains("on, off"),
            "the error names the values: {error}"
        );
        let error = run_welcome_next_start(&mut ctx, &welcome_invocation("maybe"))
            .expect_err("unknown value");
        assert!(
            error.to_string().contains("maybe"),
            "the error names the value: {error}"
        );
        let args = crate::Args::new().with("show", crate::ArgValue::Flag(true));
        let error = run_welcome_next_start(&mut ctx, &Invocation::new(args, crate::Source::Test))
            .expect_err("not a flag");
        assert!(error.to_string().contains("must be text"), "{error}");
    }

    #[test]
    fn page_navigation_without_pages_reports_it() {
        let mut ctx = AppCtx::new(Settings::default());
        ctx.open_document(DocumentSession::new(larrygram_model::Document::new(), None));
        let invocation = Invocation::empty(crate::Source::Test);
        let error = run_page_next(&mut ctx, &invocation).expect_err("no pages");
        assert_eq!(error.to_string(), "the document has no pages");
    }

    #[test]
    fn zoom_fit_falls_back_for_a_non_numeric_page_size() {
        let mut document = larrygram_model::Document::new();
        let mut page = Page::new(PageId { index: 0 }, 1);
        let mut width = Cell::new(CellName::from_static("PageWidth"));
        width.value = Some(CellValue::Str("wide".to_owned()));
        page.sheet.cells.push(width);
        document.pages.push(page);
        let mut ctx = AppCtx::new(Settings::default());
        ctx.open_document(DocumentSession::new(document, None));
        let args = crate::Args::new()
            .with("width", crate::ArgValue::Number(600.0))
            .with("height", crate::ArgValue::Number(600.0));
        let outcome =
            run_zoom_fit(&mut ctx, &Invocation::new(args, crate::Source::Test)).expect("zoom fit");
        assert!(outcome.message.unwrap_or_default().starts_with("Zoom "));
    }

    /// The four layout switches (ADR 0032 for the two Fluent-data
    /// layouts, ADR 0061 for Panel Classic and Panel Modern): each
    /// sets the layout and names it in the status line.
    #[test]
    fn the_four_layout_switches_set_the_layout() {
        let mut ctx = AppCtx::new(Settings::default());
        let invocation = Invocation::empty(crate::Source::Test);

        let outcome = run_ui_layout_panel_classic(&mut ctx, &invocation).expect("classic");
        assert_eq!(
            outcome.message.as_deref(),
            Some("Panel layout: Panel Classic")
        );
        assert_eq!(
            ctx.settings.ui_layout,
            larrygram_ui_spec::LayoutKind::PanelClassic
        );

        let outcome = run_ui_layout_panel_modern(&mut ctx, &invocation).expect("modern");
        assert_eq!(
            outcome.message.as_deref(),
            Some("Panel layout: Panel Modern")
        );
        assert_eq!(
            ctx.settings.ui_layout,
            larrygram_ui_spec::LayoutKind::PanelModern
        );

        let outcome = run_ui_layout_2016(&mut ctx, &invocation).expect("2016");
        assert_eq!(
            outcome.message.as_deref(),
            Some("Panel layout: Panel (2016 data)")
        );
        assert_eq!(
            ctx.settings.ui_layout,
            larrygram_ui_spec::LayoutKind::Panel2016
        );

        let outcome = run_ui_layout_365(&mut ctx, &invocation).expect("365");
        assert_eq!(
            outcome.message.as_deref(),
            Some("Panel layout: Panel (365 data)")
        );
        assert_eq!(
            ctx.settings.ui_layout,
            larrygram_ui_spec::LayoutKind::Panel365
        );
    }

    /// The two Help commands ask the dispatcher for their window
    /// (ADR 0066); they never open one themselves.
    #[test]
    fn the_help_commands_request_their_windows() {
        let mut ctx = AppCtx::new(Settings::default());
        let invocation = Invocation::empty(crate::Source::Test);
        let outcome = run_about(&mut ctx, &invocation).expect("about");
        assert_eq!(outcome.message.as_deref(), Some("About Larrygram"));
        assert_eq!(outcome.ui_request, Some(UiRequest::ShowAbout));
        let outcome = run_welcome(&mut ctx, &invocation).expect("welcome");
        assert_eq!(outcome.message.as_deref(), Some("Welcome"));
        assert_eq!(outcome.ui_request, Some(UiRequest::ShowWelcome));
    }

    /// A context with one open document and one page (task P7-20).
    fn one_page_ctx() -> AppCtx {
        let mut document = larrygram_model::Document::new();
        document.add_page("Page-1").expect("page");
        let mut ctx = AppCtx::new(Settings::default());
        ctx.open_document(DocumentSession::new(document, None));
        ctx
    }

    /// The six zoom presets of the Classic View > Zoom submenu (task
    /// P7-20).
    #[test]
    fn the_zoom_presets_set_the_zoom() {
        let mut ctx = one_page_ctx();
        let presets: [(crate::builtins::RunFn, f64); 6] = [
            (run_zoom_50, 50.0),
            (run_zoom_75, 75.0),
            (run_zoom_100, 100.0),
            (run_zoom_150, 150.0),
            (run_zoom_200, 200.0),
            (run_zoom_400, 400.0),
        ];
        for (run, percent) in presets {
            let outcome = run(&mut ctx, &Invocation::empty(crate::Source::Test)).expect("preset");
            assert_eq!(
                outcome.message.as_deref(),
                Some(format!("Zoom {percent:.0}%").as_str())
            );
            let session = ctx.active_document().expect("session");
            assert_eq!(session.view.zoom, percent / 100.0);
        }
    }

    /// The fit rectangle mixes its arguments with the measured
    /// viewport in every combination (task P7-20).
    #[test]
    fn the_fit_rectangle_mixes_arguments_and_the_viewport() {
        let mut ctx = one_page_ctx();
        ctx.active_document_mut().expect("session").view.viewport = Some((816.0, 1056.0));
        // Only the height is given: the measured width fills in.
        let args = crate::Args::new().with("height", crate::ArgValue::Number(1056.0));
        let outcome =
            run_zoom_fit(&mut ctx, &Invocation::new(args, crate::Source::Test)).expect("height");
        assert_eq!(outcome.message.as_deref(), Some("Zoom 100%"));
        // Only the width is given: the measured height fills in.
        let args = crate::Args::new().with("width", crate::ArgValue::Number(816.0));
        run_zoom_fit(&mut ctx, &Invocation::new(args, crate::Source::Test)).expect("width");
        // An integer rectangle counts.
        let args = crate::Args::new()
            .with("width", crate::ArgValue::Integer(816))
            .with("height", crate::ArgValue::Integer(1056));
        run_zoom_fit(&mut ctx, &Invocation::new(args, crate::Source::Test)).expect("integers");

        // Without a measured viewport each half asks in turn.
        let mut ctx = one_page_ctx();
        let args = crate::Args::new().with("height", crate::ArgValue::Number(1056.0));
        let error = run_zoom_fit(&mut ctx, &Invocation::new(args, crate::Source::Test))
            .expect_err("width missing");
        assert!(matches!(error, CommandError::NeedsInput(_)), "{error:?}");
        let args = crate::Args::new().with("width", crate::ArgValue::Number(816.0));
        let error = run_zoom_fit(&mut ctx, &Invocation::new(args, crate::Source::Test))
            .expect_err("height missing");
        assert!(matches!(error, CommandError::NeedsInput(_)), "{error:?}");
    }

    /// A `percent` argument for [`run_zoom_value`].
    fn percent_invocation(value: f64) -> Invocation {
        let args = crate::Args::new().with("percent", crate::ArgValue::Number(value));
        Invocation::new(args, crate::Source::Test)
    }

    /// The zoom percentage box sets the zoom from `percent`, taking
    /// integers too and clamping out-of-range values (task P7-20).
    #[test]
    fn the_zoom_box_sets_the_zoom_from_percent() {
        let mut ctx = one_page_ctx();
        let outcome = run_zoom_value(&mut ctx, &percent_invocation(150.0)).expect("150");
        assert_eq!(outcome.message.as_deref(), Some("Zoom 150%"));
        let args = crate::Args::new().with("percent", crate::ArgValue::Integer(200));
        let outcome =
            run_zoom_value(&mut ctx, &Invocation::new(args, crate::Source::Test)).expect("200");
        assert_eq!(outcome.message.as_deref(), Some("Zoom 200%"));
        let outcome = run_zoom_value(&mut ctx, &percent_invocation(0.01)).expect("clamp low");
        assert_eq!(
            outcome.message.as_deref(),
            Some(format!("Zoom {:.0}%", ZOOM_MIN * 100.0).as_str())
        );
        let outcome = run_zoom_value(&mut ctx, &percent_invocation(100_000.0)).expect("clamp high");
        assert_eq!(
            outcome.message.as_deref(),
            Some(format!("Zoom {:.0}%", ZOOM_MAX * 100.0).as_str())
        );
    }

    /// A missing `percent` asks for it; a bad one is an argument error
    /// (task P7-20).
    #[test]
    fn the_zoom_box_needs_a_percent_and_rejects_bad_ones() {
        let mut ctx = one_page_ctx();
        let error =
            run_zoom_value(&mut ctx, &Invocation::empty(crate::Source::Test)).expect_err("missing");
        assert!(matches!(error, CommandError::NeedsInput(_)), "{error:?}");
        let args = crate::Args::new().with("percent", crate::ArgValue::Text("wide".to_owned()));
        let error = run_zoom_value(&mut ctx, &Invocation::new(args, crate::Source::Test))
            .expect_err("text");
        assert!(
            matches!(error, CommandError::InvalidArgs { .. }),
            "{error:?}"
        );
        let args = crate::Args::new().with("percent", crate::ArgValue::Number(-5.0));
        let error = run_zoom_value(&mut ctx, &Invocation::new(args, crate::Source::Test))
            .expect_err("negative");
        assert!(
            matches!(error, CommandError::InvalidArgs { .. }),
            "{error:?}"
        );
    }

    /// The fit commands fall back to the measured viewport and ask for
    /// the rectangle when there is none (task P7-20).
    #[test]
    fn the_fit_commands_fall_back_to_the_measured_viewport() {
        let mut ctx = one_page_ctx();
        ctx.active_document_mut().expect("session").view.viewport = Some((816.0, 1056.0));
        // The fallback page is 8.5 x 11 in at 96 points per inch: 816/816 and 1056/1056.
        let outcome = run_zoom_fit(&mut ctx, &Invocation::empty(crate::Source::Test)).expect("fit");
        assert_eq!(outcome.message.as_deref(), Some("Zoom 100%"));
        let outcome =
            run_zoom_page_width(&mut ctx, &Invocation::empty(crate::Source::Test)).expect("width");
        assert_eq!(outcome.message.as_deref(), Some("Zoom 100%"));

        // One argument plus the measured viewport fills in the other.
        let args = crate::Args::new().with("width", crate::ArgValue::Number(408.0));
        let outcome = run_zoom_page_width(&mut ctx, &Invocation::new(args, crate::Source::Test))
            .expect("half");
        assert_eq!(outcome.message.as_deref(), Some("Zoom 50%"));
    }

    /// With no arguments and no measured viewport the fit asks for the
    /// rectangle (task P7-20); a bad rectangle is an argument error.
    #[test]
    fn the_fit_commands_need_a_viewport() {
        let mut ctx = one_page_ctx();
        let error = run_zoom_fit(&mut ctx, &Invocation::empty(crate::Source::Test))
            .expect_err("no viewport");
        assert!(matches!(error, CommandError::NeedsInput(_)), "{error:?}");
        let error = run_zoom_page_width(&mut ctx, &Invocation::empty(crate::Source::Test))
            .expect_err("no width");
        assert!(matches!(error, CommandError::NeedsInput(_)), "{error:?}");
        let args = crate::Args::new().with("width", crate::ArgValue::Number(-3.0));
        let error =
            run_zoom_fit(&mut ctx, &Invocation::new(args, crate::Source::Test)).expect_err("bad");
        assert!(
            matches!(error, CommandError::InvalidArgs { .. }),
            "{error:?}"
        );
    }

    /// The guides and page breaks toggle per document and are
    /// disabled with no open document (task P7-20: no settings
    /// defaults, unlike the grid and rulers).
    #[test]
    fn the_guides_and_page_breaks_toggles_flip_per_document() {
        let mut ctx = one_page_ctx();
        let outcome =
            run_toggle_guides(&mut ctx, &Invocation::empty(crate::Source::Test)).expect("guides");
        assert_eq!(outcome.message.as_deref(), Some("Guides off"));
        assert!(!ctx.active_document().expect("session").view.show_guides);
        let outcome = run_toggle_page_breaks(&mut ctx, &Invocation::empty(crate::Source::Test))
            .expect("page breaks");
        assert_eq!(outcome.message.as_deref(), Some("Page Breaks off"));
        let session = ctx.active_document().expect("session");
        assert!(!session.view.show_page_breaks);

        let mut ctx = AppCtx::new(Settings::default());
        let error = run_toggle_guides(&mut ctx, &Invocation::empty(crate::Source::Test))
            .expect_err("no document");
        assert!(matches!(error, CommandError::Failed { .. }), "{error:?}");
        let error = run_toggle_page_breaks(&mut ctx, &Invocation::empty(crate::Source::Test))
            .expect_err("no document");
        assert!(matches!(error, CommandError::Failed { .. }), "{error:?}");
    }

    /// `view.full_screen_mode` toggles the window-mode flag and can
    /// set it from `show` (task P7-20).
    #[test]
    fn full_screen_toggles_and_can_be_set() {
        let mut ctx = AppCtx::new(Settings::default());
        let outcome =
            run_full_screen(&mut ctx, &Invocation::empty(crate::Source::Test)).expect("toggle");
        assert_eq!(outcome.message.as_deref(), Some("Full screen on"));
        assert!(ctx.full_screen);
        let outcome =
            run_full_screen(&mut ctx, &Invocation::empty(crate::Source::Test)).expect("toggle");
        assert_eq!(outcome.message.as_deref(), Some("Full screen off"));
        assert!(!ctx.full_screen);

        let show = |value: &str| {
            let args = crate::Args::new().with("show", crate::ArgValue::Text(value.to_owned()));
            Invocation::new(args, crate::Source::Test)
        };
        let outcome = run_full_screen(&mut ctx, &show("on")).expect("on");
        assert_eq!(outcome.message.as_deref(), Some("Full screen on"));
        let outcome = run_full_screen(&mut ctx, &show("off")).expect("off");
        assert_eq!(outcome.message.as_deref(), Some("Full screen off"));
        let error = run_full_screen(&mut ctx, &show("maybe")).expect_err("bad value");
        assert!(
            matches!(error, CommandError::InvalidArgs { .. }),
            "{error:?}"
        );
        let args = crate::Args::new().with("show", crate::ArgValue::Flag(true));
        let error = run_full_screen(&mut ctx, &Invocation::new(args, crate::Source::Test))
            .expect_err("not text");
        assert!(
            matches!(error, CommandError::InvalidArgs { .. }),
            "{error:?}"
        );
    }
}
