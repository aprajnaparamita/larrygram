//! `page.*` commands (P5-09).
//!
//! Page commands: add, rename, delete and reorder (ADR 0047), plus the
//! `page.move_left` / `page.move_right` tab wrappers. Every command uses
//! the ADR 0039 page primitives and returns one undoable [`PagesEdit`]
//! snapshot of the page list. `PageId` is a list position, so an edit
//! that moves or deletes a page swaps the whole list back on undo; the
//! active page is part of the snapshot and a page switch clears the
//! selection (ADR 0041).

use larrygram_model::{CellValue, CellWrite, Connect, Page, PageId, Shape, SheetRef};

use crate::app::{AppCtx, SessionId};
use crate::clipboard::{ClipboardContent, ClipboardKind};
use crate::edit::CompositeEdit;
use crate::error::CommandError;
use crate::undo::Edit;
use crate::{ArgValue, Invocation, Outcome, UiRequest};

use super::clipboard::{Placement, paste_content};
use super::edit::{CellEdit, document_mut, number_literal};
use super::view::page_size;
use super::{CommandTable, edit_outcome, has_document, has_pages, model_error, outcome};

/// The implemented page commands (`page.next` / `page.previous` stay in
/// `view.rs`, where they started).
///
/// Task P7-20 adds the aliases the Panel Classic and Panel Modern
/// layouts link: the New Page family (`tools.options.new.page`,
/// `tools.blank.page.insert`, `tools.blank.page.insert.visio`), the
/// duplicate (`edit.duplicate.page`), the reorder dialog id
/// (`page.reorder_pages`), the Go to Page dialog id
/// (`tools.options.go.to.drawing`) and the background-page family
/// (`insert.new.background.page`, `tools.backgrounds`). Each aliases
/// one handler: one action, one implementation (ADR 0006).
pub(crate) const TABLE: CommandTable = &[
    ("page.add", "New Page", has_document, run_add),
    ("tools.options.new.page", "New Page", has_document, run_add),
    ("tools.blank.page.insert", "New Page", has_document, run_add),
    (
        "tools.blank.page.insert.visio",
        "Blank Page",
        has_document,
        run_add,
    ),
    ("page.delete", "Delete Page", can_delete, run_delete),
    ("page.rename", "Rename Page", has_pages, run_rename),
    (
        "edit.duplicate.page",
        "Duplicate Page",
        has_pages,
        run_duplicate_page,
    ),
    ("page.reorder", "Reorder", can_move, run_reorder),
    ("page.reorder_pages", "Reorder Pages", can_move, run_reorder),
    ("page.move_left", "Move Page Left", can_move, run_move_left),
    (
        "page.move_right",
        "Move Page Right",
        can_move,
        run_move_right,
    ),
    (
        "tools.options.go.to.drawing",
        "Go to Page",
        has_pages,
        run_go_to_page,
    ),
    (
        "insert.new.background.page",
        "Background Page",
        has_document,
        run_new_background_page,
    ),
    (
        "tools.backgrounds",
        "Backgrounds",
        has_pages,
        run_set_background,
    ),
    (
        "tools.options.page.setup",
        "Page Setup",
        has_document,
        run_page_setup,
    ),
    (
        "format.set.page.orientation",
        "Orientation",
        has_document,
        run_set_orientation,
    ),
    ("page.size.dlg", "Size", has_document, run_page_size),
];

/// Enabled when there is more than one page to delete from.
fn can_delete(ctx: &AppCtx) -> crate::Enabled {
    match ctx.active_document() {
        None => crate::Enabled::no("no open document"),
        Some(session) if session.document.pages.len() < 2 => {
            crate::Enabled::no("the document has only one page")
        }
        Some(_) => crate::Enabled::yes(),
    }
}

/// Enabled when the document has at least two pages.
fn can_move(ctx: &AppCtx) -> crate::Enabled {
    match ctx.active_document() {
        None => crate::Enabled::no("no open document"),
        Some(session) if session.document.pages.len() < 2 => {
            crate::Enabled::no("the document has only one page")
        }
        Some(_) => crate::Enabled::yes(),
    }
}

/// The active session and its active page (the first page as a fallback).
fn active_page(ctx: &AppCtx) -> Result<(SessionId, PageId), CommandError> {
    let session = ctx.active_document().ok_or_else(super::no_document)?;
    let page = session
        .active_page
        .or_else(|| session.document.pages.first().map(|page| page.id))
        .ok_or_else(|| CommandError::failed("the document has no pages"))?;
    Ok((session.id, page))
}

/// The page snapshot an edit restores in either direction.
struct PagesEdit {
    session: SessionId,
    label: String,
    old: Vec<Page>,
    old_active: Option<PageId>,
    new: Vec<Page>,
    new_active: Option<PageId>,
}

impl PagesEdit {
    fn new(
        session: SessionId,
        label: impl Into<String>,
        old: Vec<Page>,
        old_active: Option<PageId>,
        new: Vec<Page>,
        new_active: Option<PageId>,
    ) -> Self {
        Self {
            session,
            label: label.into(),
            old,
            old_active,
            new,
            new_active,
        }
    }

    /// Replaces the page list and active page, clears the selection when
    /// the active page changed, and bumps the change counter (ADR 0040).
    ///
    /// The pages' names and membership come from the snapshot, but a
    /// page that is still open keeps its live `shapes` and `Connects`
    /// lists (task P5-11): those belong to the edits that moved shapes
    /// or changed glue, and a snapshot copy can name arena ids that a
    /// re-done group or insert has replaced.
    fn restore(
        &self,
        ctx: &mut AppCtx,
        pages: &[Page],
        active: Option<PageId>,
    ) -> Result<(), CommandError> {
        let session = self.session;
        let doc = document_mut(ctx, session)?;
        let live = doc.pages.clone();
        let mut restored: Vec<Page> = Vec::with_capacity(pages.len());
        for wanted in pages {
            let mut page = wanted.clone();
            if let Some(current) = live.iter().find(|page| page.visio_id == wanted.visio_id) {
                page.shapes = current.shapes.clone();
                page.connects = current.connects.clone();
            }
            restored.push(page);
        }
        doc.pages = restored;
        let active_changed = {
            let slot = ctx
                .documents
                .iter_mut()
                .find(|slot| slot.id == session)
                .ok_or_else(|| CommandError::failed("the document session is gone"))?;
            let previous = slot.active_page;
            slot.active_page = active;
            previous != active
        };
        if active_changed && ctx.active_document().map(|slot| slot.id) == Some(session) {
            ctx.selection.clear();
        }
        ctx.touch_session(session);
        Ok(())
    }
}

impl Edit for PagesEdit {
    fn label(&self) -> &str {
        &self.label
    }

    fn apply(&mut self, ctx: &mut AppCtx) -> Result<(), CommandError> {
        self.restore(ctx, &self.new, self.new_active)
    }

    fn revert(&mut self, ctx: &mut AppCtx) -> Result<(), CommandError> {
        self.restore(ctx, &self.old, self.old_active)
    }
}

/// `page.add`: adds a new page above the current last page and makes it
/// active. The optional `name` names it; without one the name is
/// `Page N`.
fn run_add(ctx: &mut AppCtx, inv: &Invocation) -> Result<Outcome, CommandError> {
    let session = ctx.active_document().ok_or_else(super::no_document)?;
    let session_id = session.id;
    let old = session.document.pages.clone();
    let old_active = session.active_page;
    let default = format!("Page {}", old.len().saturating_add(1));
    let name = match inv.args.get("name") {
        None => default,
        Some(ArgValue::Text(name)) if !name.trim().is_empty() => name.clone(),
        Some(ArgValue::Text(_)) => default,
        Some(_) => return Err(CommandError::invalid_args("name must be text")),
    };
    let page = {
        let doc = document_mut(ctx, session_id)?;
        doc.add_page(&name).map_err(model_error)?
    };
    let new = {
        let doc = document_mut(ctx, session_id)?;
        doc.pages.clone()
    };
    if let Some(session) = ctx.documents.iter_mut().find(|slot| slot.id == session_id) {
        session.active_page = Some(page);
    }
    ctx.selection.clear();
    ctx.touch_session(session_id);
    let edit = PagesEdit::new(session_id, "Add page", old, old_active, new, Some(page));
    Ok(edit_outcome(
        format!("Added {name}"),
        Vec::new(),
        Box::new(edit),
    ))
}

/// `page.delete`: removes the active page and activates its neighbour.
fn run_delete(ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
    let (session_id, page) = active_page(ctx)?;
    let session = ctx.active_document().ok_or_else(super::no_document)?;
    let old = session.document.pages.clone();
    let old_active = session.active_page;
    if old.len() < 2 {
        return Err(CommandError::failed("cannot delete the only page"));
    }
    let index = old
        .iter()
        .position(|entry| entry.id == page)
        .ok_or_else(|| CommandError::failed("the active page is gone"))?;
    let removed = {
        let doc = document_mut(ctx, session_id)?;
        doc.delete_page(page).map_err(model_error)?
    };
    let (new, next) = {
        let doc = document_mut(ctx, session_id)?;
        let next = if index < doc.pages.len() {
            doc.pages.get(index).map(|entry| entry.id)
        } else {
            index
                .checked_sub(1)
                .and_then(|previous| doc.pages.get(previous).map(|entry| entry.id))
        };
        (doc.pages.clone(), next)
    };
    if let Some(session) = ctx.documents.iter_mut().find(|slot| slot.id == session_id) {
        session.active_page = next;
    }
    ctx.selection.clear();
    ctx.touch_session(session_id);
    let edit = PagesEdit::new(session_id, "Delete page", old, old_active, new, next);
    Ok(edit_outcome(
        format!("Deleted {}", page_label(&removed, index)),
        Vec::new(),
        Box::new(edit),
    ))
}

/// `page.rename`: renames the active page from the required `name`
/// argument.
fn run_rename(ctx: &mut AppCtx, inv: &Invocation) -> Result<Outcome, CommandError> {
    let name = match inv.args.get("name") {
        None => {
            return Err(CommandError::NeedsInput(crate::NeededInput::Text {
                prompt: "page name".to_owned(),
                default: None,
            }));
        }
        Some(ArgValue::Text(name)) if !name.trim().is_empty() => name.clone(),
        Some(ArgValue::Text(_)) => {
            return Err(CommandError::invalid_args(
                "the page name must not be empty",
            ));
        }
        Some(_) => return Err(CommandError::invalid_args("name must be text")),
    };
    let (session_id, page) = active_page(ctx)?;
    let session = ctx.active_document().ok_or_else(super::no_document)?;
    let old = session.document.pages.clone();
    let old_active = session.active_page;
    {
        let doc = document_mut(ctx, session_id)?;
        doc.rename_page(page, &name).map_err(model_error)?;
    }
    let new = {
        let doc = document_mut(ctx, session_id)?;
        doc.pages.clone()
    };
    ctx.touch_session(session_id);
    let edit = PagesEdit::new(session_id, "Rename page", old, old_active, new, old_active);
    Ok(edit_outcome(
        format!("Renamed page to {name}"),
        Vec::new(),
        Box::new(edit),
    ))
}

/// `page.reorder`: moves the page at zero-based `from` to `to`.
fn run_reorder(ctx: &mut AppCtx, inv: &Invocation) -> Result<Outcome, CommandError> {
    let from = need_index(&inv.args, "from")?;
    let to = need_index(&inv.args, "to")?;
    reorder(ctx, from, to)
}

/// `page.move_left`: moves the active page one position towards the
/// start.
fn run_move_left(ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
    move_active(ctx, -1)
}

/// `page.move_right`: moves the active page one position towards the
/// end.
fn run_move_right(ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
    move_active(ctx, 1)
}

/// A required non-negative zero-based index argument.
fn need_index(args: &crate::Args, name: &str) -> Result<usize, CommandError> {
    match args.get(name) {
        None => Err(CommandError::NeedsInput(crate::NeededInput::Number {
            prompt: format!("{name} page index (from 0)"),
            default: None,
        })),
        Some(ArgValue::Integer(value)) if *value >= 0 => usize::try_from(*value)
            .map_err(|_| CommandError::invalid_args(format!("{name} is too large"))),
        Some(ArgValue::Number(value))
            if value.is_finite() && *value >= 0.0 && value.fract() == 0.0 =>
        {
            usize::try_from(*value as i64)
                .map_err(|_| CommandError::invalid_args(format!("{name} is too large")))
        }
        Some(_) => Err(CommandError::invalid_args(format!(
            "{name} must be a whole number of pages from 0"
        ))),
    }
}

/// Moves the active page by `delta`, staying inside the list.
fn move_active(ctx: &mut AppCtx, delta: i64) -> Result<Outcome, CommandError> {
    let (_, page) = active_page(ctx)?;
    let session = ctx.active_document().ok_or_else(super::no_document)?;
    let index = session
        .document
        .pages
        .iter()
        .position(|entry| entry.id == page)
        .ok_or_else(|| CommandError::failed("the active page is gone"))?;
    let len = session.document.pages.len();
    let target = if delta < 0 {
        index.saturating_sub(delta.unsigned_abs() as usize)
    } else {
        index
            .saturating_add(usize::try_from(delta).unwrap_or(usize::MAX))
            .min(len.saturating_sub(1))
    };
    reorder(ctx, index, target)
}

/// Moves the page at list index `from` to index `to` (clamped by the
/// model) and keeps the active page on the same page.
fn reorder(ctx: &mut AppCtx, from: usize, to: usize) -> Result<Outcome, CommandError> {
    let (session_id, page) = active_page(ctx)?;
    let session = ctx.active_document().ok_or_else(super::no_document)?;
    let old = session.document.pages.clone();
    let old_active = session.active_page;
    let active_visio = old
        .iter()
        .find(|entry| entry.id == page)
        .map(|entry| entry.visio_id)
        .ok_or_else(|| CommandError::failed("the active page is gone"))?;
    {
        let doc = document_mut(ctx, session_id)?;
        doc.move_page(from, to).map_err(model_error)?;
    }
    let (new, next) = {
        let doc = document_mut(ctx, session_id)?;
        let next = doc
            .pages
            .iter()
            .find(|entry| entry.visio_id == active_visio)
            .map(|entry| entry.id);
        (doc.pages.clone(), next)
    };
    if let Some(session) = ctx.documents.iter_mut().find(|slot| slot.id == session_id) {
        session.active_page = next;
    }
    let refresh = ctx
        .active_document()
        .filter(|session| session.id == session_id)
        .map(|session| session.document.clone());
    if let Some(document) = refresh {
        ctx.selection.refresh_page(&document);
    }
    ctx.touch_session(session_id);
    let name = new
        .iter()
        .find(|entry| Some(entry.id) == next)
        .map(|entry| page_label(entry, next.map(|id| id.index as usize).unwrap_or(0)))
        .unwrap_or_else(|| "page".to_owned());
    let edit = PagesEdit::new(session_id, "Reorder page", old, old_active, new, next);
    Ok(edit_outcome(
        format!("Moved {name}"),
        Vec::new(),
        Box::new(edit),
    ))
}

/// The display name of one page: its `Name`, else its number.
fn page_label(page: &Page, index: usize) -> String {
    if page.name.is_empty() {
        format!("Page {}", index.saturating_add(1))
    } else {
        page.name.clone()
    }
}

// ---------------------------------------------------------------------------
// The task P7-20 page commands: duplicate, go to, backgrounds and
// page setup.
// ---------------------------------------------------------------------------

/// `edit.duplicate.page`: copies the active page — its shapes and its
/// `Connects` records — into a new page after it (task P7-20).
///
/// One undo step: the page-list change and the copied subtrees in one
/// [`CompositeEdit`], so undo removes the copy and redo brings it back
/// with the same shapes (the `StructuralEdit::insertion` steps own the
/// shapes, the [`PagesEdit`] owns the list).
fn run_duplicate_page(ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
    let (session_id, page) = active_page(ctx)?;
    let session = ctx.active_document().ok_or_else(super::no_document)?;
    let old = session.document.pages.clone();
    let old_active = session.active_page;
    let (content, name, sheet, background) = {
        let doc = &session.document;
        let entity = doc
            .page(page)
            .ok_or_else(|| CommandError::failed("the active page is gone"))?;
        (
            page_fragment(doc, entity)?,
            entity.name.clone(),
            entity.sheet.clone(),
            entity.background,
        )
    };
    let count = content.shapes.len();
    let label = page_label(
        &old.iter()
            .find(|entry| entry.id == page)
            .cloned()
            .unwrap_or_else(|| Page::new(page, 0)),
        page.index as usize,
    );
    // The list snapshot for the redo side is taken before the paste:
    // the copy's shapes belong to the paste step alone. The steps run
    // in this order and revert in reverse: the shapes leave first,
    // then the page.
    let (new_list, copy) = {
        let doc = document_mut(ctx, session_id)?;
        let copy = doc.add_page(&name).map_err(model_error)?;
        // The copy keeps the page's own sheet (its page setup) and its
        // background assignment (task P7-20).
        if let Some(entity) = doc.pages.iter_mut().find(|entry| entry.id == copy) {
            entity.sheet = sheet;
            entity.background = background;
        }
        (doc.pages.clone(), copy)
    };
    let mut edit = CompositeEdit::new("Duplicate page");
    edit.push(Box::new(PagesEdit::new(
        session_id,
        "Duplicate page",
        old,
        old_active,
        new_list,
        Some(copy),
    )));
    let mut warnings = Vec::new();
    if count > 0 {
        // An empty page duplicates as its page-list entry alone.
        let (step, _roots, paste_warnings) =
            paste_content(ctx, session_id, copy, content, Placement::Offset(0.0, 0.0))?;
        warnings.extend(paste_warnings);
        edit.push(Box::new(step));
    }
    if let Some(session) = ctx.documents.iter_mut().find(|slot| slot.id == session_id) {
        session.active_page = Some(copy);
    }
    ctx.selection.clear();
    ctx.touch_session(session_id);
    Ok(edit_outcome(
        format!("Duplicated {label} ({count} shapes)"),
        warnings,
        Box::new(edit),
    ))
}

/// Every shape and `Connect` record of one page, as a clipboard
/// fragment (task P7-20).
fn page_fragment(
    doc: &larrygram_model::Document,
    entity: &Page,
) -> Result<ClipboardContent, CommandError> {
    let mut shapes: Vec<Shape> = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for &root in &entity.shapes {
        let mut stack = vec![root];
        while let Some(id) = stack.pop() {
            if !seen.insert(id.index) {
                continue;
            }
            let Some(shape) = doc.shape(id) else {
                continue;
            };
            shapes.push(shape.clone());
            for child in shape.children.iter().rev() {
                stack.push(*child);
            }
        }
    }
    let connects: Vec<Connect> = entity.connects.clone();
    Ok(ClipboardContent {
        kind: ClipboardKind::Shapes,
        shapes,
        connects,
        source_page: entity.id,
    })
}

/// `tools.options.go.to.drawing`: makes the page named by `name`, or
/// numbered by `page` (1-based), active (task P7-20: the Go to Page
/// dialog's argument form).
///
/// Navigation, not an edit: it carries no undo and does not bump the
/// change counter (the page switch clears the selection, ADR 0041).
fn run_go_to_page(ctx: &mut AppCtx, inv: &Invocation) -> Result<Outcome, CommandError> {
    let session = ctx.active_document().ok_or_else(super::no_document)?;
    let wanted = match (inv.args.text("name"), inv.args.get("page")) {
        (Some(name), _) => Some(name.to_owned()),
        (None, Some(ArgValue::Integer(number))) => Some(number.to_string()),
        (None, Some(ArgValue::Number(number))) if *number >= 1.0 && number.fract() == 0.0 => {
            Some(number.to_string())
        }
        (None, Some(_)) => {
            return Err(CommandError::invalid_args(
                "page must be a whole number of pages from 1",
            ));
        }
        (None, None) => {
            return Err(CommandError::NeedsInput(crate::NeededInput::Number {
                prompt: "page number (from 1)".to_owned(),
                default: None,
            }));
        }
    };
    let wanted = wanted.unwrap_or_default();
    let pages = session.document.pages.clone();
    let target = pages
        .iter()
        .enumerate()
        .find(|(_, entry)| entry.name == wanted || entry.name_u == wanted)
        .or_else(|| {
            wanted
                .parse::<usize>()
                .ok()
                .and_then(|number| number.checked_sub(1))
                .and_then(|index| pages.get(index).map(|entry| (index, entry)))
        })
        .map(|(index, entry)| (index, entry.id));
    let Some((index, page)) = target else {
        return Err(CommandError::failed(format!("no page named {wanted:?}")));
    };
    if let Some(session) = ctx.active_document_mut() {
        session.active_page = Some(page);
    }
    ctx.selection.clear();
    let label = pages
        .get(index)
        .map(|entry| page_label(entry, index))
        .unwrap_or_else(|| wanted.clone());
    Ok(outcome(format!("On {label}"), Vec::new()))
}

/// `insert.new.background.page`: inserts a new background page and
/// assigns it to the current page (task P7-20).
///
/// The optional `name` names it; without one the name is
/// `Background-1`, `Background-2`, ... (Visio's spelling). The new
/// page becomes active, so the user draws on it right away. One undo
/// step (the [`PagesEdit`] holds the page list and the assignment).
fn run_new_background_page(ctx: &mut AppCtx, inv: &Invocation) -> Result<Outcome, CommandError> {
    let name = match inv.args.get("name") {
        Some(ArgValue::Text(name)) if !name.trim().is_empty() => name.clone(),
        Some(ArgValue::Text(_)) | None => background_name(ctx),
        Some(_) => return Err(CommandError::invalid_args("name must be text")),
    };
    let (session_id, page) = active_page(ctx)?;
    let session = ctx.active_document().ok_or_else(super::no_document)?;
    let old = session.document.pages.clone();
    let old_active = session.active_page;
    let (new_list, copy) = {
        let doc = document_mut(ctx, session_id)?;
        let copy = doc.add_page(&name).map_err(model_error)?;
        if let Some(entity) = doc.pages.iter_mut().find(|entry| entry.id == page) {
            entity.background = Some(copy);
        }
        (doc.pages.clone(), copy)
    };
    if let Some(session) = ctx.documents.iter_mut().find(|slot| slot.id == session_id) {
        session.active_page = Some(copy);
    }
    ctx.selection.clear();
    ctx.touch_session(session_id);
    let edit = PagesEdit::new(
        session_id,
        "Insert background page",
        old,
        old_active,
        new_list,
        Some(copy),
    );
    Ok(edit_outcome(
        format!("Inserted background page {name}"),
        Vec::new(),
        Box::new(edit),
    ))
}

/// The next free `Background-N` name of the document.
fn background_name(ctx: &AppCtx) -> String {
    let taken: Vec<String> = ctx
        .active_document()
        .map(|session| {
            session
                .document
                .pages
                .iter()
                .map(|page| page.name.clone())
                .collect()
        })
        .unwrap_or_default();
    // The candidates number one more than the taken names, so the loop
    // always finds one; the seeded value is the last candidate, the
    // documented fallback.
    let mut name = format!("Background-{}", taken.len().saturating_add(1));
    for number in 1..=taken.len().saturating_add(1) {
        let candidate = format!("Background-{number}");
        if !taken.contains(&candidate) {
            name = candidate;
            break;
        }
    }
    name
}

/// `tools.backgrounds`: assigns the background page named by the
/// `background` argument to the current page (task P7-20: the
/// Design-tab Backgrounds gallery's argument form).
///
/// `"none"` (or an empty string) removes the assignment. The named
/// page must exist and must not be the current page (a page cannot be
/// its own background). One undo step.
fn run_set_background(ctx: &mut AppCtx, inv: &Invocation) -> Result<Outcome, CommandError> {
    let wanted = match inv.args.get("background") {
        None => {
            return Err(CommandError::NeedsInput(crate::NeededInput::Text {
                prompt: "background page name, or none".to_owned(),
                default: Some("none".to_owned()),
            }));
        }
        Some(ArgValue::Text(text)) => text.clone(),
        Some(_) => return Err(CommandError::invalid_args("background must be text")),
    };
    let (session_id, page) = active_page(ctx)?;
    let session = ctx.active_document().ok_or_else(super::no_document)?;
    let old = session.document.pages.clone();
    let old_active = session.active_page;
    let wanted = wanted.trim().to_owned();
    let target = if wanted.is_empty() || wanted.eq_ignore_ascii_case("none") {
        None
    } else {
        let found = session
            .document
            .pages
            .iter()
            .find(|entry| entry.name == wanted || entry.name_u == wanted)
            .map(|entry| entry.id);
        let Some(found) = found else {
            return Err(CommandError::failed(format!(
                "no page named {wanted:?} to use as the background"
            )));
        };
        if found == page {
            return Err(CommandError::failed(
                "a page cannot be its own background page",
            ));
        }
        Some(found)
    };
    let new_list = {
        let doc = document_mut(ctx, session_id)?;
        if let Some(entity) = doc.pages.iter_mut().find(|entry| entry.id == page) {
            entity.background = target;
        }
        doc.pages.clone()
    };
    ctx.touch_session(session_id);
    let edit = PagesEdit::new(
        session_id,
        "Set background page",
        old,
        old_active,
        new_list,
        old_active,
    );
    let message = match target {
        Some(_) => format!("Background page: {wanted}"),
        None => "Background page: none".to_owned(),
    };
    Ok(edit_outcome(message, Vec::new(), Box::new(edit)))
}

// ---------------------------------------------------------------------------
// Page setup (task P7-20).
// ---------------------------------------------------------------------------

/// The requested page setup: any subset of page size, orientation and
/// display units (task P7-20).
#[derive(Debug, Default, Clone, Copy, PartialEq)]
struct PageSetup {
    /// The page size in inches (`width`, `height`).
    size: Option<(f64, f64)>,
    /// The page orientation.
    orientation: Option<Orientation>,
    /// The display units.
    units: Option<crate::DisplayUnits>,
}

/// The page orientation of `tools.options.page.setup` (task P7-20).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Orientation {
    /// Height at least the width.
    Portrait,
    /// Width at least the height.
    Landscape,
}

impl Orientation {
    /// Parses the `orientation` spelling (`portrait` / `landscape`,
    /// ASCII case-insensitive).
    fn parse(text: &str) -> Option<Self> {
        match text.to_ascii_lowercase().as_str() {
            "portrait" => Some(Orientation::Portrait),
            "landscape" => Some(Orientation::Landscape),
            _ => None,
        }
    }

    /// The name as [`Self::parse`] spells it.
    fn name(self) -> &'static str {
        match self {
            Orientation::Portrait => "portrait",
            Orientation::Landscape => "landscape",
        }
    }

    /// Swaps the size around when it contradicts the orientation (the
    /// Visio rule: orientation chooses which side is longer).
    fn fit(self, (width, height): (f64, f64)) -> (f64, f64) {
        match self {
            Orientation::Portrait if width > height => (height, width),
            Orientation::Landscape if height > width => (height, width),
            _ => (width, height),
        }
    }
}

/// Parses the page-setup arguments (`width` and `height` in inches,
/// `orientation`, `units`); every one is optional.
fn parse_page_setup(inv: &Invocation) -> Result<PageSetup, CommandError> {
    let width = optional_size(&inv.args, "width")?;
    let height = optional_size(&inv.args, "height")?;
    let size = match (width, height) {
        (Some(width), Some(height)) => Some((width, height)),
        (None, None) => None,
        _ => {
            return Err(CommandError::invalid_args(
                "width and height must be given together",
            ));
        }
    };
    let orientation = match inv.args.get("orientation") {
        None => None,
        Some(ArgValue::Text(text)) => match Orientation::parse(text) {
            Some(orientation) => Some(orientation),
            None => {
                return Err(CommandError::invalid_args(format!(
                    "unknown orientation {text:?}; expected one of portrait, landscape"
                )));
            }
        },
        Some(_) => {
            return Err(CommandError::invalid_args(
                "orientation must be text; expected one of portrait, landscape",
            ));
        }
    };
    let units = match inv.args.get("units") {
        None => None,
        Some(ArgValue::Text(text)) => match crate::DisplayUnits::parse(text) {
            Some(units) => Some(units),
            None => {
                let names = crate::DisplayUnits::NAMES.join(", ");
                return Err(CommandError::invalid_args(format!(
                    "unknown units {text:?}; expected one of {names}"
                )));
            }
        },
        Some(_) => {
            return Err(CommandError::invalid_args(
                "units must be text; expected a display-units name",
            ));
        }
    };
    Ok(PageSetup {
        size,
        orientation,
        units,
    })
}

/// One optional finite positive size argument in inches.
fn optional_size(args: &crate::Args, name: &str) -> Result<Option<f64>, CommandError> {
    match args.get(name) {
        None => Ok(None),
        Some(ArgValue::Number(value)) if value.is_finite() && *value > 0.0 => Ok(Some(*value)),
        Some(ArgValue::Integer(value)) if *value > 0 => Ok(Some(*value as f64)),
        Some(_) => Err(CommandError::invalid_args(format!(
            "{name} must be a finite positive number of inches"
        ))),
    }
}

/// `tools.options.page.setup`: sets the page size, orientation and
/// display units of the active page from arguments (task P7-20).
///
/// The dialog body is a later task's; with no arguments the command
/// requests it ([`UiRequest::ShowPageSetup`]) and changes nothing
/// (AGENTS.md section 6: a programmatic call never opens a dialog).
/// `width` and `height` are in inches (the internal unit); `units`
/// names the display units and carries no undo (a setting, like
/// `view.units`). The size and orientation changes are one undo step.
fn run_page_setup(ctx: &mut AppCtx, inv: &Invocation) -> Result<Outcome, CommandError> {
    let setup = parse_page_setup(inv)?;
    if setup == PageSetup::default() {
        return Ok(Outcome {
            message: Some("Page Setup".to_owned()),
            ui_request: Some(UiRequest::ShowPageSetup),
            ..Outcome::default()
        });
    }
    apply_page_setup(ctx, setup, "Page setup")
}

/// `format.set.page.orientation`: sets the page orientation from the
/// `orientation` argument (task P7-20).
fn run_set_orientation(ctx: &mut AppCtx, inv: &Invocation) -> Result<Outcome, CommandError> {
    let orientation = match inv.args.get("orientation") {
        None => {
            return Err(CommandError::NeedsInput(crate::NeededInput::Text {
                prompt: "orientation (portrait or landscape)".to_owned(),
                default: Some("portrait".to_owned()),
            }));
        }
        Some(ArgValue::Text(text)) => match Orientation::parse(text) {
            Some(orientation) => orientation,
            None => {
                return Err(CommandError::invalid_args(format!(
                    "unknown orientation {text:?}; expected one of portrait, landscape"
                )));
            }
        },
        Some(_) => {
            return Err(CommandError::invalid_args(
                "orientation must be text; expected one of portrait, landscape",
            ));
        }
    };
    apply_page_setup(
        ctx,
        PageSetup {
            orientation: Some(orientation),
            ..PageSetup::default()
        },
        "Page orientation",
    )
}

/// `page.size.dlg`: sets the page size from the `width` and `height`
/// arguments (task P7-20: the Design-tab Size dropdown's argument
/// form); with no arguments it requests the Page Setup dialog, like
/// [`run_page_setup`].
fn run_page_size(ctx: &mut AppCtx, inv: &Invocation) -> Result<Outcome, CommandError> {
    let setup = parse_page_setup(inv)?;
    if setup.size.is_none() && setup.orientation.is_none() && setup.units.is_none() {
        return Ok(Outcome {
            message: Some("Page Setup".to_owned()),
            ui_request: Some(UiRequest::ShowPageSetup),
            ..Outcome::default()
        });
    }
    apply_page_setup(ctx, setup, "Page size")
}

/// Applies one [`PageSetup`] to the active page: one undoable size
/// write ([`CellEdit`]s on `PageWidth`/`PageHeight`), then the display
/// units setting.
fn apply_page_setup(
    ctx: &mut AppCtx,
    setup: PageSetup,
    label: &str,
) -> Result<Outcome, CommandError> {
    let (session_id, page) = active_page(ctx)?;
    let session = ctx.active_document().ok_or_else(super::no_document)?;
    let entity = session
        .document
        .page(page)
        .ok_or_else(|| CommandError::failed("the active page is gone"))?;
    let mut size = setup.size.unwrap_or(page_size(entity));
    if let Some(orientation) = setup.orientation {
        size = orientation.fit(size);
    }
    let current = page_size(entity);
    let mut edit = CompositeEdit::new(label);
    let mut changed = false;
    if size != current {
        let doc = document_mut(ctx, session_id)?;
        for (cell, value) in [("PageWidth", size.0), ("PageHeight", size.1)] {
            edit.push(Box::new(CellEdit::new(
                doc,
                session_id,
                label,
                SheetRef::Page(page),
                cell,
                CellWrite::new(number_literal(value), CellValue::Number(value)),
            )?));
        }
        changed = true;
    }
    if changed {
        edit.apply(ctx)?;
        ctx.touch_session(session_id);
    }
    if let Some(units) = setup.units {
        ctx.settings.units = units;
    }
    let mut parts: Vec<String> = Vec::new();
    if changed {
        parts.push(format!("{:.2} x {:.2} in", size.0, size.1));
    }
    if let Some(orientation) = setup.orientation {
        parts.push(orientation.name().to_owned());
    }
    if let Some(units) = setup.units {
        parts.push(units.name().to_owned());
    }
    if parts.is_empty() {
        return Ok(outcome("Page setup unchanged", Vec::new()));
    }
    let message = format!("{label}: {}", parts.join(", "));
    if changed {
        Ok(edit_outcome(message, Vec::new(), Box::new(edit)))
    } else {
        // Only the settings changed: no undo step (like `view.units`).
        Ok(outcome(message, Vec::new()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use larrygram_model::Document;

    #[test]
    fn page_label_falls_back_to_the_number() {
        let mut page = Page::new(PageId { index: 0 }, 1);
        assert_eq!(page_label(&page, 0), "Page 1");
        page.name = "Flow".to_owned();
        assert_eq!(page_label(&page, 4), "Flow");
    }

    #[test]
    fn need_index_accepts_both_number_forms_and_rejects_bad_ones() {
        let mut args = crate::Args::new();
        args.set("from", ArgValue::Integer(2));
        assert_eq!(need_index(&args, "from").expect("integer"), 2);
        args.set("from", ArgValue::Number(3.0));
        assert_eq!(need_index(&args, "from").expect("number"), 3);
        args.set("from", ArgValue::Number(3.5));
        assert!(matches!(
            need_index(&args, "from"),
            Err(CommandError::InvalidArgs { .. })
        ));
        args.set("from", ArgValue::Integer(-1));
        assert!(matches!(
            need_index(&args, "from"),
            Err(CommandError::InvalidArgs { .. })
        ));
        args.set("from", ArgValue::Text("two".to_owned()));
        assert!(matches!(
            need_index(&args, "from"),
            Err(CommandError::InvalidArgs { .. })
        ));
        args = crate::Args::new();
        let error = need_index(&args, "to").expect_err("missing");
        assert!(matches!(error, CommandError::NeedsInput(_)));
    }

    #[test]
    fn add_and_rename_names_are_validated() {
        let mut ctx = AppCtx::new(crate::Settings::default());
        let index = ctx.open_document(crate::DocumentSession::new(Document::new(), None));
        let session = ctx.documents.get(index).expect("session").id;
        let mut invocation = Invocation::empty(crate::Source::Test);
        invocation.args.set("name", ArgValue::Integer(1));
        assert!(matches!(
            run_add(&mut ctx, &invocation),
            Err(CommandError::InvalidArgs { .. })
        ));
        let _ = session;

        let mut invocation = Invocation::empty(crate::Source::Test);
        invocation.args.set("name", ArgValue::Text("  ".to_owned()));
        let outcome = run_add(&mut ctx, &invocation).expect("default name");
        assert_eq!(outcome.message.as_deref(), Some("Added Page 1"));
        let session = ctx.active_document().expect("session");
        assert_eq!(session.document.pages.len(), 1);
        assert_eq!(session.document.pages[0].name, "Page 1");

        let mut invocation = Invocation::empty(crate::Source::Test);
        invocation
            .args
            .set("name", ArgValue::Text("Flow".to_owned()));
        let outcome = run_rename(&mut ctx, &invocation).expect("rename");
        assert_eq!(outcome.message.as_deref(), Some("Renamed page to Flow"));

        let mut invocation = Invocation::empty(crate::Source::Test);
        invocation.args.set("name", ArgValue::Text(String::new()));
        assert!(matches!(
            run_rename(&mut ctx, &invocation),
            Err(CommandError::InvalidArgs { .. })
        ));
        let invocation = Invocation::empty(crate::Source::Test);
        let error = run_rename(&mut ctx, &invocation).expect_err("missing name");
        assert!(matches!(error, CommandError::NeedsInput(_)));
        let mut invocation = Invocation::empty(crate::Source::Test);
        invocation.args.set("name", ArgValue::Integer(1));
        assert!(matches!(
            run_rename(&mut ctx, &invocation),
            Err(CommandError::InvalidArgs { .. })
        ));
    }

    #[test]
    fn draw_without_pages_reports_it() {
        let mut ctx = AppCtx::new(crate::Settings::default());
        assert!(active_page(&ctx).is_err());
        ctx.open_document(crate::DocumentSession::new(Document::new(), None));
        assert!(active_page(&ctx).is_err());
        assert!(run_add(&mut ctx, &Invocation::empty(crate::Source::Test)).is_ok());
        assert!(active_page(&ctx).is_ok());
        assert!(matches!(
            run_delete(&mut ctx, &Invocation::empty(crate::Source::Test)),
            Err(CommandError::Failed { .. })
        ));
    }

    // -----------------------------------------------------------------------
    // Task P7-20: duplicate, go to, backgrounds and page setup.
    // -----------------------------------------------------------------------

    /// A context with one document, one page and one shape on it.
    fn one_shape_ctx() -> AppCtx {
        use larrygram_model::{Shape, ShapeId};
        let mut document = Document::new();
        let page = document.add_page("Page-1").expect("page");
        let shape = Shape::new(ShapeId { index: 0 }, 1);
        document.insert_shape(page, shape, None).expect("shape");
        let mut ctx = AppCtx::new(crate::Settings::default());
        ctx.open_document(crate::DocumentSession::new(document, None));
        ctx
    }

    /// An invocation with the given text or number arguments.
    fn invocation(pairs: &[(&str, ArgValue)]) -> Invocation {
        let mut args = crate::Args::new();
        for (name, value) in pairs {
            args.set(*name, value.clone());
        }
        Invocation::new(args, crate::Source::Test)
    }

    /// `edit.duplicate.page` copies the shapes into a new page, and
    /// one undo removes the copy while redo brings it back with the
    /// shapes (task P7-20).
    #[test]
    fn duplicate_page_copies_the_shapes_and_round_trips() {
        let mut ctx = one_shape_ctx();
        let outcome = run_duplicate_page(&mut ctx, &Invocation::empty(crate::Source::Test))
            .expect("duplicate");
        assert!(
            outcome
                .message
                .as_deref()
                .is_some_and(|m| m.starts_with("Duplicated ")),
            "{:?}",
            outcome.message
        );
        let session = ctx.active_document().expect("session");
        assert_eq!(session.document.pages.len(), 2);
        assert_eq!(
            session.document.pages[1].shapes.len(),
            1,
            "the copy has the shape"
        );
        assert_eq!(session.active_page, Some(PageId { index: 1 }));

        let mut edit = outcome.edit.expect("one undo step");
        edit.revert(&mut ctx).expect("undo");
        let session = ctx.active_document().expect("session");
        assert_eq!(session.document.pages.len(), 1, "undo removes the copy");
        edit.apply(&mut ctx).expect("redo");
        let session = ctx.active_document().expect("session");
        assert_eq!(session.document.pages.len(), 2, "redo brings it back");
        assert_eq!(session.document.pages[1].shapes.len(), 1, "with the shapes");
    }

    /// The copy keeps the page's own sheet (its page setup) and its
    /// background assignment (task P7-20).
    #[test]
    fn duplicate_page_keeps_the_page_setup_and_background() {
        let mut ctx = one_shape_ctx();
        // A second page to point the background at.
        run_add(
            &mut ctx,
            &invocation(&[("name", ArgValue::Text("Back".to_owned()))]),
        )
        .expect("add");
        {
            let session = ctx.active_document_mut().expect("session");
            session.document.pages[0].background = Some(PageId { index: 1 });
            session.active_page = Some(PageId { index: 0 });
        }
        run_page_setup(
            &mut ctx,
            &invocation(&[
                ("width", ArgValue::Number(5.0)),
                ("height", ArgValue::Number(7.0)),
            ]),
        )
        .expect("size");
        run_duplicate_page(&mut ctx, &Invocation::empty(crate::Source::Test)).expect("duplicate");
        let session = ctx.active_document().expect("session");
        assert_eq!(session.document.pages.len(), 3);
        let copy = &session.document.pages[2];
        assert_eq!(page_size(copy), (5.0, 7.0), "the copy keeps the page setup");
        assert_eq!(copy.background, Some(PageId { index: 1 }));
    }

    /// `tools.options.go.to.drawing` switches by name or number and
    /// says so; a missing or unknown target is an error (task P7-20).
    #[test]
    fn go_to_page_switches_by_name_or_number() {
        let mut ctx = one_shape_ctx();
        run_add(
            &mut ctx,
            &invocation(&[("name", ArgValue::Text("Flow".to_owned()))]),
        )
        .expect("add");
        assert_eq!(
            ctx.active_document().expect("session").active_page,
            Some(PageId { index: 1 })
        );
        let outcome = run_go_to_page(
            &mut ctx,
            &invocation(&[("name", ArgValue::Text("Page-1".to_owned()))]),
        )
        .expect("by name");
        assert_eq!(outcome.message.as_deref(), Some("On Page-1"));
        assert_eq!(
            ctx.active_document().expect("session").active_page,
            Some(PageId { index: 0 })
        );
        let outcome = run_go_to_page(&mut ctx, &invocation(&[("page", ArgValue::Integer(2))]))
            .expect("by number");
        assert_eq!(outcome.message.as_deref(), Some("On Flow"));
        assert_eq!(
            ctx.active_document().expect("session").active_page,
            Some(PageId { index: 1 })
        );

        let error = run_go_to_page(&mut ctx, &Invocation::empty(crate::Source::Test))
            .expect_err("no target");
        assert!(matches!(error, CommandError::NeedsInput(_)), "{error:?}");
        // A whole number as a real number counts; a fraction does not.
        let outcome = run_go_to_page(&mut ctx, &invocation(&[("page", ArgValue::Number(1.0))]))
            .expect("by number");
        assert_eq!(outcome.message.as_deref(), Some("On Page-1"));
        let error = run_go_to_page(&mut ctx, &invocation(&[("page", ArgValue::Number(1.5))]))
            .expect_err("fraction");
        assert!(
            matches!(error, CommandError::InvalidArgs { .. }),
            "{error:?}"
        );
        let error = run_go_to_page(
            &mut ctx,
            &invocation(&[("name", ArgValue::Text("Nope".to_owned()))]),
        )
        .expect_err("unknown");
        assert!(error.to_string().contains("no page named"), "{error}");
        let error = run_go_to_page(
            &mut ctx,
            &invocation(&[("page", ArgValue::Text("2".to_owned()))]),
        )
        .expect_err("bad page");
        assert!(
            matches!(error, CommandError::InvalidArgs { .. }),
            "{error:?}"
        );
    }

    /// `insert.new.background.page` creates a background page, assigns
    /// it and switches to it; the name defaults to `Background-N`
    /// (task P7-20).
    #[test]
    fn new_background_page_creates_and_assigns() {
        let mut ctx = one_shape_ctx();
        let outcome = run_new_background_page(&mut ctx, &Invocation::empty(crate::Source::Test))
            .expect("background page");
        assert_eq!(
            outcome.message.as_deref(),
            Some("Inserted background page Background-1")
        );
        let session = ctx.active_document().expect("session");
        assert_eq!(session.document.pages.len(), 2);
        assert_eq!(
            session.document.pages[0].background,
            Some(PageId { index: 1 })
        );
        assert_eq!(session.active_page, Some(PageId { index: 1 }));

        // The next one takes the next free name; a name argument wins.
        run_new_background_page(&mut ctx, &Invocation::empty(crate::Source::Test)).expect("second");
        let session = ctx.active_document().expect("session");
        assert_eq!(session.document.pages[2].name, "Background-2");
        let outcome = run_new_background_page(
            &mut ctx,
            &invocation(&[("name", ArgValue::Text("Watermark".to_owned()))]),
        )
        .expect("named");
        assert_eq!(
            outcome.message.as_deref(),
            Some("Inserted background page Watermark")
        );
        let error =
            run_new_background_page(&mut ctx, &invocation(&[("name", ArgValue::Integer(3))]))
                .expect_err("bad name");
        assert!(
            matches!(error, CommandError::InvalidArgs { .. }),
            "{error:?}"
        );
    }

    /// `tools.backgrounds` assigns and clears the background page and
    /// refuses an unknown or self-referential one (task P7-20).
    #[test]
    fn set_background_assigns_and_clears() {
        let mut ctx = one_shape_ctx();
        run_add(
            &mut ctx,
            &invocation(&[("name", ArgValue::Text("Back".to_owned()))]),
        )
        .expect("add");
        // The active page is "Back"; assign "Page-1" as its background.
        let outcome = run_set_background(
            &mut ctx,
            &invocation(&[("background", ArgValue::Text("Page-1".to_owned()))]),
        )
        .expect("assign");
        assert_eq!(outcome.message.as_deref(), Some("Background page: Page-1"));
        let session = ctx.active_document().expect("session");
        assert_eq!(
            session.document.pages[1].background,
            Some(PageId { index: 0 })
        );

        // None clears it again.
        let outcome = run_set_background(
            &mut ctx,
            &invocation(&[("background", ArgValue::Text("none".to_owned()))]),
        )
        .expect("clear");
        assert_eq!(outcome.message.as_deref(), Some("Background page: none"));
        let session = ctx.active_document().expect("session");
        assert_eq!(session.document.pages[1].background, None);

        let error = run_set_background(
            &mut ctx,
            &invocation(&[("background", ArgValue::Text("Nope".to_owned()))]),
        )
        .expect_err("unknown");
        assert!(error.to_string().contains("no page named"), "{error}");
        let error = run_set_background(
            &mut ctx,
            &invocation(&[("background", ArgValue::Text("Back".to_owned()))]),
        )
        .expect_err("self");
        assert!(error.to_string().contains("own background"), "{error}");
        let error = run_set_background(&mut ctx, &Invocation::empty(crate::Source::Test))
            .expect_err("no value");
        assert!(matches!(error, CommandError::NeedsInput(_)), "{error:?}");
        let error = run_set_background(
            &mut ctx,
            &invocation(&[("background", ArgValue::Integer(2))]),
        )
        .expect_err("bad type");
        assert!(
            matches!(error, CommandError::InvalidArgs { .. }),
            "{error:?}"
        );
    }

    /// `tools.options.page.setup` with no arguments requests the
    /// dialog and changes nothing (task P7-20).
    #[test]
    fn page_setup_without_arguments_requests_the_dialog() {
        let mut ctx = one_shape_ctx();
        let outcome =
            run_page_setup(&mut ctx, &Invocation::empty(crate::Source::Test)).expect("setup");
        assert_eq!(outcome.message.as_deref(), Some("Page Setup"));
        assert_eq!(outcome.ui_request, Some(UiRequest::ShowPageSetup));
        assert!(outcome.edit.is_none(), "nothing changed");
    }

    /// `tools.options.page.setup` writes the size cells with one undo
    /// step, honours the orientation rule and sets the units without
    /// an edit (task P7-20).
    #[test]
    fn page_setup_writes_the_size_orientation_and_units() {
        let mut ctx = one_shape_ctx();
        let outcome = run_page_setup(
            &mut ctx,
            &invocation(&[
                ("width", ArgValue::Number(11.0)),
                ("height", ArgValue::Number(8.5)),
                ("orientation", ArgValue::Text("landscape".to_owned())),
                ("units", ArgValue::Text("millimeters".to_owned())),
            ]),
        )
        .expect("setup");
        assert_eq!(
            outcome.message.as_deref(),
            Some("Page setup: 11.00 x 8.50 in, landscape, Millimeters")
        );
        let session = ctx.active_document().expect("session");
        let (width, height) = page_size(&session.document.pages[0]);
        assert_eq!((width, height), (11.0, 8.5));
        assert_eq!(ctx.settings.units, crate::DisplayUnits::Millimeters);

        // One undo restores the fallback size (the unit setting stays:
        // it carries no undo).
        let mut edit = outcome.edit.expect("one undo step");
        edit.revert(&mut ctx).expect("undo");
        let session = ctx.active_document().expect("session");
        assert_eq!(
            page_size(&session.document.pages[0]),
            larrygram_render::FALLBACK_PAGE_SIZE,
            "undo restores the size"
        );
    }

    /// The orientation alone swaps the sides when they contradict it
    /// (task P7-20).
    #[test]
    fn page_setup_orientation_swaps_the_sides() {
        let mut ctx = one_shape_ctx();
        // Make the page landscape first.
        run_page_setup(
            &mut ctx,
            &invocation(&[
                ("width", ArgValue::Number(11.0)),
                ("height", ArgValue::Number(8.5)),
            ]),
        )
        .expect("landscape size");
        let outcome = run_set_orientation(
            &mut ctx,
            &invocation(&[("orientation", ArgValue::Text("portrait".to_owned()))]),
        )
        .expect("portrait");
        assert_eq!(
            outcome.message.as_deref(),
            Some("Page orientation: 8.50 x 11.00 in, portrait")
        );
        let session = ctx.active_document().expect("session");
        assert_eq!(
            page_size(&session.document.pages[0]),
            (8.5, 11.0),
            "swapped"
        );

        run_set_orientation(
            &mut ctx,
            &invocation(&[("orientation", ArgValue::Text("landscape".to_owned()))]),
        )
        .expect("landscape");
        let session = ctx.active_document().expect("session");
        assert_eq!(
            page_size(&session.document.pages[0]),
            (11.0, 8.5),
            "swapped back"
        );

        // An orientation that already holds changes nothing.
        let outcome = run_set_orientation(
            &mut ctx,
            &invocation(&[("orientation", ArgValue::Text("landscape".to_owned()))]),
        )
        .expect("unchanged");
        assert_eq!(
            outcome.message.as_deref(),
            Some("Page orientation: landscape")
        );
        assert!(outcome.edit.is_none(), "nothing to undo");

        // Without a value it asks; a bad one is an argument error.
        let error = run_set_orientation(&mut ctx, &Invocation::empty(crate::Source::Test))
            .expect_err("no value");
        assert!(matches!(error, CommandError::NeedsInput(_)), "{error:?}");
        let error = run_set_orientation(
            &mut ctx,
            &invocation(&[("orientation", ArgValue::Text("sideways".to_owned()))]),
        )
        .expect_err("bad value");
        assert!(
            matches!(error, CommandError::InvalidArgs { .. }),
            "{error:?}"
        );
        let error = run_set_orientation(
            &mut ctx,
            &invocation(&[("orientation", ArgValue::Flag(true))]),
        )
        .expect_err("bad type");
        assert!(
            matches!(error, CommandError::InvalidArgs { .. }),
            "{error:?}"
        );
    }

    /// `page.size.dlg` applies the size from its arguments and
    /// requests the dialog with none (task P7-20).
    #[test]
    fn page_size_applies_or_requests_the_dialog() {
        let mut ctx = one_shape_ctx();
        let outcome =
            run_page_size(&mut ctx, &Invocation::empty(crate::Source::Test)).expect("ask");
        assert_eq!(outcome.ui_request, Some(UiRequest::ShowPageSetup));
        let outcome = run_page_size(
            &mut ctx,
            &invocation(&[
                ("width", ArgValue::Number(5.0)),
                ("height", ArgValue::Number(7.0)),
            ]),
        )
        .expect("apply");
        assert_eq!(
            outcome.message.as_deref(),
            Some("Page size: 5.00 x 7.00 in")
        );
        let session = ctx.active_document().expect("session");
        assert_eq!(page_size(&session.document.pages[0]), (5.0, 7.0));
    }

    /// The page-setup parser rejects half a size and unknown values
    /// (task P7-20).
    #[test]
    fn page_setup_arguments_are_validated() {
        let error = parse_page_setup(&invocation(&[("width", ArgValue::Number(5.0))]))
            .expect_err("width alone");
        assert!(
            matches!(error, CommandError::InvalidArgs { .. }),
            "{error:?}"
        );
        let error = parse_page_setup(&invocation(&[("width", ArgValue::Number(-1.0))]))
            .expect_err("negative");
        assert!(
            matches!(error, CommandError::InvalidArgs { .. }),
            "{error:?}"
        );
        let error = parse_page_setup(&invocation(&[
            ("width", ArgValue::Number(5.0)),
            ("height", ArgValue::Number(7.0)),
            ("units", ArgValue::Text("furlongs".to_owned())),
        ]))
        .expect_err("bad units");
        assert!(error.to_string().contains("furlongs"), "{error}");
        let error = parse_page_setup(&invocation(&[("units", ArgValue::Flag(true))]))
            .expect_err("bad type");
        assert!(
            matches!(error, CommandError::InvalidArgs { .. }),
            "{error:?}"
        );
        let error = parse_page_setup(&invocation(&[(
            "orientation",
            ArgValue::Text("sideways".to_owned()),
        )]))
        .expect_err("bad orientation");
        assert!(error.to_string().contains("sideways"), "{error}");
        let error = parse_page_setup(&invocation(&[("orientation", ArgValue::Flag(true))]))
            .expect_err("orientation type");
        assert!(
            matches!(error, CommandError::InvalidArgs { .. }),
            "{error:?}"
        );
        // An integer size counts.
        let setup = parse_page_setup(&invocation(&[
            ("width", ArgValue::Integer(5)),
            ("height", ArgValue::Integer(7)),
        ]))
        .expect("integers");
        assert_eq!(setup.size, Some((5.0, 7.0)));
        // The unit-only setup applies without an edit.
        let mut ctx = one_shape_ctx();
        let outcome = run_page_setup(
            &mut ctx,
            &invocation(&[("units", ArgValue::Text("meters".to_owned()))]),
        )
        .expect("units");
        assert_eq!(outcome.message.as_deref(), Some("Page setup: Meters"));
        assert!(outcome.edit.is_none(), "a setting carries no undo");
    }

    /// The fragment walk skips a repeated id and a stale one (task
    /// P7-20): never a panic, one copy per shape.
    #[test]
    fn page_fragment_skips_stale_and_repeated_ids() {
        use larrygram_model::{Shape, ShapeId};
        let mut document = Document::new();
        let page = document.add_page("Page-1").expect("page");
        let shape = document
            .insert_shape(page, Shape::new(ShapeId { index: 0 }, 1), None)
            .expect("shape");
        {
            let entity = document.pages.first_mut().expect("page");
            entity.shapes.push(shape);
            entity.shapes.push(ShapeId { index: 999 });
        }
        let entity = document.pages.first().expect("page");
        let fragment = page_fragment(&document, entity).expect("fragment");
        assert_eq!(fragment.shapes.len(), 1, "one copy of the one shape");
    }
}
