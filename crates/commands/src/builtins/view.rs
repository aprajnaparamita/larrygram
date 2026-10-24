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

/// `view.zoom.fit`: zooms so the active page fits the canvas rectangle
/// given by the `width` and `height` arguments, in points (ADR 0028).
pub(super) fn run_zoom_fit(ctx: &mut AppCtx, inv: &Invocation) -> Result<Outcome, CommandError> {
    let width = need_number(&inv.args, "width", "canvas width in points")?;
    let height = need_number(&inv.args, "height", "canvas height in points")?;
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
    let fit = ((width / (page_width * 72.0)).min(height / (page_height * 72.0)))
        .clamp(ZOOM_MIN, ZOOM_MAX);
    session.view.zoom = fit;
    Ok(outcome(format!("Zoom {:.0}%", fit * 100.0), Vec::new()))
}

/// A positive `width` / `height` argument in points, or
/// [`CommandError::NeedsInput`] when it is missing.
fn need_number(args: &crate::Args, name: &str, prompt: &str) -> Result<f64, CommandError> {
    match args.number(name) {
        Some(value) if value.is_finite() && value > 0.0 => Ok(value),
        Some(_) => Err(CommandError::invalid_args(format!(
            "{name} must be finite and positive"
        ))),
        None => Err(CommandError::NeedsInput(crate::NeededInput::Number {
            prompt: prompt.to_owned(),
            default: None,
        })),
    }
}

/// The page size in inches, mirroring the renderer's per-axis fallback.
fn page_size(page: &Page) -> (f64, f64) {
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
}
