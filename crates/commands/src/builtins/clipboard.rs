//! `clipboard.*` commands (P5-09).
//!
//! Clipboard commands over the internal clipboard (ADR 0047):
//! `edit.copy`, `edit.cut`, `edit.paste` and `edit.duplicate`.
//!
//! A copy stores a deep clone of the selected subtrees and the page's
//! `Connects` records whose two ends are both copied. A paste re-inserts
//! the fragment on the active page with fresh arena and Visio ids,
//! keeping each shape's `Master` link. The edit of a paste is one
//! [`CompositeEdit`](crate::CompositeEdit) of `StructuralEdit::insertion`
//! steps, so one undo removes the whole fragment (ADR 0040).
//!
//! Paste offset (ADR 0047): without `x`/`y` the fragment lands on the
//! recorded position offset by `(0.1 in, -0.1 in)` per consecutive paste
//! (the first paste is offset once). With `x` and `y` the first root
//! shape's pin lands on the pointer instead, and the offset counter does
//! not advance.

use std::collections::{BTreeMap, BTreeSet};

use larrygram_model::{CellName, CellValue, CellWrite, Connect, PageId, Shape, ShapeId};

use crate::app::{AppCtx, SessionId};
use crate::clipboard::{ClipboardContent, ClipboardKind};
use crate::edit::CompositeEdit;
use crate::error::CommandError;
use crate::{ArgValue, Args, Invocation, Outcome};

use super::edit::{
    StructuralEdit, document_mut, number_literal, read_cell_number, selection_targets,
};
use super::{CommandTable, edit_outcome, has_pages, has_selection, no_document, outcome};

/// The paste offset of ADR 0047, in inches.
const PASTE_OFFSET: f64 = 0.1;

/// The implemented clipboard commands.
pub(crate) const TABLE: CommandTable = &[
    ("edit.copy", "Copy", has_selection, run_copy),
    ("edit.cut", "Cut", has_selection, run_cut),
    ("edit.paste", "Paste", can_paste, run_paste),
    (
        "edit.paste.in.place",
        "Paste in Place",
        can_paste,
        run_paste_in_place,
    ),
    ("edit.duplicate", "Duplicate", has_selection, run_duplicate),
    // Panel Modern's Paste menu calls the duplicate "Duplicate"
    // (task P7-20: one action, one handler).
    (
        "edit.paste.duplicate",
        "Duplicate",
        has_selection,
        run_duplicate,
    ),
];

/// Enabled when the active document has pages and the clipboard holds a
/// fragment (ADR 0047).
fn can_paste(ctx: &AppCtx) -> crate::Enabled {
    if ctx.clipboard.is_empty() {
        return crate::Enabled::no("the clipboard is empty");
    }
    has_pages(ctx)
}

/// `edit.copy`: copies the selection into the internal clipboard.
pub(super) fn run_copy(ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
    let (content, warnings) = content_from_selection(ctx)?;
    let count = content.shapes.len();
    ctx.clipboard = crate::clipboard::Clipboard {
        content: Some(content),
        pastes: 0,
    };
    Ok(outcome(format!("Copied {count} shapes"), warnings))
}

/// `edit.cut`: copy plus delete in one undo step (ADR 0047).
pub(super) fn run_cut(ctx: &mut AppCtx, inv: &Invocation) -> Result<Outcome, CommandError> {
    let (content, copy_warnings) = content_from_selection(ctx)?;
    let count = content.shapes.len();
    ctx.clipboard = crate::clipboard::Clipboard {
        content: Some(content),
        pastes: 0,
    };
    let mut deleted = super::edit::run_delete(ctx, inv)?;
    let mut edit = CompositeEdit::new(format!("Cut {count} shapes"));
    if let Some(step) = deleted.edit.take() {
        edit.push(step);
    }
    let mut warnings = copy_warnings;
    warnings.extend(deleted.warnings);
    Ok(edit_outcome(
        format!("Cut {count} shapes"),
        warnings,
        Box::new(edit),
    ))
}

/// `edit.paste`: pastes the clipboard on the active page.
///
/// Optional `x` and `y` (inches) place the fragment at the pointer; both
/// must be given together.
pub(super) fn run_paste(ctx: &mut AppCtx, inv: &Invocation) -> Result<Outcome, CommandError> {
    paste_run(ctx, inv, false)
}

/// `edit.paste.in.place`: pastes the clipboard at the recorded
/// coordinates (task P7-20): no paste offset and no pointer, so the
/// fragment lands exactly where it was copied.
///
/// The `x`/`y` pointer pair has no meaning here and is rejected, not
/// ignored.
pub(super) fn run_paste_in_place(
    ctx: &mut AppCtx,
    inv: &Invocation,
) -> Result<Outcome, CommandError> {
    if inv.args.get("x").is_some() || inv.args.get("y").is_some() {
        return Err(CommandError::invalid_args(
            "paste in place takes neither x nor y; use edit.paste for a paste at the pointer",
        ));
    }
    paste_run(ctx, inv, true)
}

/// The shared body of `edit.paste` and `edit.paste.in.place`.
fn paste_run(ctx: &mut AppCtx, inv: &Invocation, in_place: bool) -> Result<Outcome, CommandError> {
    let pointer = if in_place {
        None
    } else {
        pointer_offset(&inv.args)?
    };
    let Some(content) = ctx.clipboard.content.clone() else {
        return Err(CommandError::failed("the clipboard is empty"));
    };
    let session = ctx.active_document().ok_or_else(no_document)?;
    if session.document.pages.is_empty() {
        return Err(CommandError::failed("the document has no pages"));
    }
    let session_id = session.id;
    let page = session
        .active_page
        .or_else(|| session.document.pages.first().map(|page| page.id))
        .ok_or_else(|| CommandError::failed("no active page"))?;
    let placement = match pointer {
        Some((x, y)) => Placement::Pointer(x, y),
        None if in_place => Placement::Offset(0.0, 0.0),
        None => {
            let steps = f64::from(ctx.clipboard.pastes.saturating_add(1));
            Placement::Offset(PASTE_OFFSET * steps, -PASTE_OFFSET * steps)
        }
    };
    let count = content.shapes.len();
    let (edit, roots, warnings) = paste_content(ctx, session_id, page, content, placement)?;
    if pointer.is_none() && !in_place {
        ctx.clipboard.pastes = ctx.clipboard.pastes.saturating_add(1);
    }
    select_shapes(ctx, page, &roots);
    super::connector::reroute_quietly(ctx);
    let message = if in_place {
        format!("Pasted {count} shapes in place")
    } else {
        format!("Pasted {count} shapes")
    };
    Ok(edit_outcome(message, warnings, Box::new(edit)))
}

/// `edit.duplicate`: copies the selection at the paste offset without
/// touching the clipboard (ADR 0047).
pub(super) fn run_duplicate(ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
    let (content, mut warnings) = content_from_selection(ctx)?;
    let session = ctx.active_document().ok_or_else(no_document)?;
    let session_id = session.id;
    let page = session
        .active_page
        .or_else(|| session.document.pages.first().map(|page| page.id))
        .ok_or_else(|| CommandError::failed("no active page"))?;
    let count = content.shapes.len();
    let (edit, roots, paste_warnings) = paste_content(
        ctx,
        session_id,
        page,
        content,
        Placement::Offset(PASTE_OFFSET, -PASTE_OFFSET),
    )?;
    warnings.extend(paste_warnings);
    select_shapes(ctx, page, &roots);
    super::connector::reroute_quietly(ctx);
    Ok(edit_outcome(
        format!("Duplicated {count} shapes"),
        warnings,
        Box::new(edit),
    ))
}

/// The `x`/`y` pointer pair, `None` when neither is present.
fn pointer_offset(args: &Args) -> Result<Option<(f64, f64)>, CommandError> {
    let x = optional_number(args, "x")?;
    let y = optional_number(args, "y")?;
    match (x, y) {
        (None, None) => Ok(None),
        (Some(x), Some(y)) => Ok(Some((x, y))),
        _ => Err(CommandError::invalid_args(
            "x and y must be given together for a paste at the pointer",
        )),
    }
}

/// One optional finite number argument.
fn optional_number(args: &Args, name: &str) -> Result<Option<f64>, CommandError> {
    match args.get(name) {
        None => Ok(None),
        Some(ArgValue::Number(value)) if value.is_finite() => Ok(Some(*value)),
        Some(ArgValue::Integer(value)) => Ok(Some(*value as f64)),
        Some(ArgValue::Number(_)) => {
            Err(CommandError::invalid_args(format!("{name} must be finite")))
        }
        Some(_) => Err(CommandError::invalid_args(format!(
            "{name} must be a number"
        ))),
    }
}

/// Builds the clipboard payload from the selection.
fn content_from_selection(ctx: &AppCtx) -> Result<(ClipboardContent, Vec<String>), CommandError> {
    let (session, targets, warnings) = selection_targets(ctx)?;
    let doc = ctx
        .documents
        .iter()
        .find(|slot| slot.id == session)
        .map(|slot| &slot.document)
        .ok_or_else(|| CommandError::failed("the document session is gone"))?;
    // Every target passed `selection_targets`, so it is on a page.
    let page = targets
        .first()
        .and_then(|id| doc.page_of(*id))
        .unwrap_or(PageId { index: 0 });

    // A selected shape inside another selected shape is copied with its
    // ancestor, not a second time.
    let selected: BTreeSet<u32> = targets.iter().map(|id| id.index).collect();
    let mut roots = Vec::new();
    for &target in &targets {
        if ancestor_selected(doc, target, &selected) {
            continue;
        }
        roots.push(target);
    }

    // The copied subtree, root first then descendants, bounded and
    // cycle-safe.
    let mut shapes = Vec::new();
    let mut seen = BTreeSet::new();
    for root in &roots {
        let mut stack = vec![*root];
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

    let visio_ids: BTreeSet<i32> = shapes.iter().map(|shape| shape.visio_id).collect();
    let connects: Vec<Connect> = doc
        .page(page)
        .map(|page| {
            page.connects
                .iter()
                .filter(|connect| {
                    visio_ids.contains(&connect.from_sheet) && visio_ids.contains(&connect.to_sheet)
                })
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    Ok((
        ClipboardContent {
            kind: ClipboardKind::Shapes,
            shapes,
            connects,
            source_page: page,
        },
        warnings,
    ))
}

/// Whether `shape` has a selected ancestor (walking up the parent chain,
/// cycle-safe).
fn ancestor_selected(
    doc: &larrygram_model::Document,
    shape: ShapeId,
    selected: &BTreeSet<u32>,
) -> bool {
    let mut current = shape;
    let mut seen = BTreeSet::new();
    loop {
        if !seen.insert(current.index) {
            return false;
        }
        let Some(parent) = doc.shape(current).and_then(|entry| entry.parent) else {
            return false;
        };
        if selected.contains(&parent.index) {
            return true;
        }
        current = parent;
    }
}

/// Where a paste puts the fragment: at an absolute pin or as a delta
/// from the copied position.
#[derive(Debug, Clone, Copy)]
pub(super) enum Placement {
    /// Move by this many inches.
    Offset(f64, f64),
    /// Put the first root's pin on this page position.
    Pointer(f64, f64),
}

/// Inserts `content` on `page` and returns the undo step plus the new
/// top-level roots.
pub(super) fn paste_content(
    ctx: &mut AppCtx,
    session: SessionId,
    page: PageId,
    content: ClipboardContent,
    placement: Placement,
) -> Result<(CompositeEdit, Vec<ShapeId>, Vec<String>), CommandError> {
    let mut warnings = Vec::new();
    let by_id: BTreeMap<u32, usize> = content
        .shapes
        .iter()
        .enumerate()
        .map(|(index, shape)| (shape.id.index, index))
        .collect();
    let copied: BTreeSet<u32> = by_id.keys().copied().collect();
    let roots: Vec<usize> = content
        .shapes
        .iter()
        .enumerate()
        .filter(|(_, shape)| {
            shape
                .parent
                .is_none_or(|parent| !copied.contains(&parent.index))
        })
        .map(|(index, _)| index)
        .collect();
    if roots.is_empty() {
        return Err(CommandError::failed("the clipboard holds no root shape"));
    }

    let mut visio_map = BTreeMap::new();
    let mut root_ids = Vec::new();
    {
        let doc = document_mut(ctx, session)?;
        for (index, shape) in content.shapes.iter().enumerate() {
            if !roots.contains(&index) {
                continue;
            }
            let clone = prepared(shape);
            let old_visio = shape.visio_id;
            let id = doc
                .insert_shape(page, clone, None)
                .map_err(super::model_error)?;
            if let Some(entry) = doc.shape(id) {
                visio_map.insert(old_visio, entry.visio_id);
            }
            insert_descendants(doc, &content, &by_id, &copied, id, index, &mut visio_map)?;
            root_ids.push(id);
        }

        // The pointer or the consecutive-paste offset moves the roots
        // only; their descendants follow through the group transform.
        let delta = match placement {
            Placement::Offset(dx, dy) => (dx, dy),
            Placement::Pointer(x, y) => match root_ids.first().copied() {
                Some(first) => {
                    let (pin_x, pin_y) =
                        read_cell_number(doc, first, "PinX").and_then(|(pin_x, _)| {
                            read_cell_number(doc, first, "PinY").map(|(pin_y, _)| (pin_x, pin_y))
                        })?;
                    (x - pin_x, y - pin_y)
                }
                None => (0.0, 0.0),
            },
        };
        if delta.0 != 0.0 || delta.1 != 0.0 {
            for root in &root_ids {
                let (pin_x, pin_y) =
                    read_cell_number(doc, *root, "PinX").and_then(|(pin_x, _)| {
                        read_cell_number(doc, *root, "PinY").map(|(pin_y, _)| (pin_x, pin_y))
                    })?;
                let next = (pin_x + delta.0, pin_y + delta.1);
                doc.write_cell(
                    larrygram_model::SheetRef::Shape(*root),
                    &CellName::from_static("PinX"),
                    &CellWrite::new(number_literal(next.0), CellValue::Number(next.0)),
                )
                .map_err(super::model_error)?;
                doc.write_cell(
                    larrygram_model::SheetRef::Shape(*root),
                    &CellName::from_static("PinY"),
                    &CellWrite::new(number_literal(next.1), CellValue::Number(next.1)),
                )
                .map_err(super::model_error)?;
            }
        }

        for connect in &content.connects {
            let (Some(from_sheet), Some(to_sheet)) = (
                visio_map.get(&connect.from_sheet),
                visio_map.get(&connect.to_sheet),
            ) else {
                warnings
                    .push("a connect record referred to a shape that was not copied".to_owned());
                continue;
            };
            let mut fresh = connect.clone();
            fresh.from_sheet = *from_sheet;
            fresh.to_sheet = *to_sheet;
            doc.add_connect(page, fresh).map_err(super::model_error)?;
        }
    }

    let mut edit = CompositeEdit::new(format!("Paste {} shapes", content.shapes.len()));
    {
        let doc = document_mut(ctx, session)?;
        for root in &root_ids {
            let step = StructuralEdit::insertion(doc, session, "Paste shape", *root)?;
            edit.push(Box::new(step));
        }
    }
    ctx.touch_session(session);
    Ok((edit, root_ids, warnings))
}

/// A clone prepared for insertion: no arena links or child ids (they are
/// rebuilt with fresh ids).
fn prepared(shape: &Shape) -> Shape {
    let mut clone = shape.clone();
    clone.parent = None;
    clone.children.clear();
    clone
}

/// Inserts one node's descendants and links the fresh ids.
#[allow(clippy::too_many_arguments)]
fn insert_descendants(
    doc: &mut larrygram_model::Document,
    content: &ClipboardContent,
    by_id: &BTreeMap<u32, usize>,
    copied: &BTreeSet<u32>,
    parent: ShapeId,
    index: usize,
    visio_map: &mut BTreeMap<i32, i32>,
) -> Result<(), CommandError> {
    let Some(source) = content.shapes.get(index) else {
        return Ok(());
    };
    let children = source.children.clone();
    for child in children {
        if !copied.contains(&child.index) {
            continue;
        }
        let Some(&child_index) = by_id.get(&child.index) else {
            continue;
        };
        let Some(child_shape) = content.shapes.get(child_index) else {
            continue;
        };
        let clone = prepared(child_shape);
        let old_visio = child_shape.visio_id;
        let id = doc
            .insert_subshape(parent, clone, None)
            .map_err(super::model_error)?;
        if let Some(entry) = doc.shape(id) {
            visio_map.insert(old_visio, entry.visio_id);
        }
        insert_descendants(doc, content, by_id, copied, id, child_index, visio_map)?;
    }
    Ok(())
}

/// Selects the pasted roots on `page` (ADR 0041).
fn select_shapes(ctx: &mut AppCtx, page: PageId, roots: &[ShapeId]) {
    ctx.selection.shapes = roots.to_vec();
    ctx.selection.anchor = roots.last().copied();
    ctx.selection.page = Some(page);
    ctx.selection.entered = None;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{AppCtx, DocumentSession, Settings};
    use crate::clipboard::{Clipboard, ClipboardContent, ClipboardKind};
    use larrygram_model::Document;

    fn content(shapes: Vec<Shape>, connects: Vec<Connect>) -> ClipboardContent {
        ClipboardContent {
            kind: ClipboardKind::Shapes,
            shapes,
            connects,
            source_page: PageId { index: 0 },
        }
    }

    fn context(with_page: bool) -> (AppCtx, SessionId, PageId) {
        let mut doc = Document::new();
        let page = if with_page {
            doc.add_page("Page-1").expect("page")
        } else {
            PageId { index: 0 }
        };
        let mut ctx = AppCtx::new(Settings::default());
        let index = ctx.open_document(DocumentSession::new(doc, None));
        let session = ctx.documents.get(index).expect("session").id;
        (ctx, session, page)
    }

    #[test]
    fn paste_reports_an_empty_clipboard_and_a_document_without_pages() {
        let (mut ctx, _session, _page) = context(true);
        let invocation = Invocation::empty(crate::Source::Test);
        let error = run_paste(&mut ctx, &invocation).expect_err("empty clipboard");
        assert!(error.to_string().contains("clipboard is empty"));

        let (mut no_pages, _session, _page) = context(false);
        no_pages.clipboard = Clipboard {
            content: Some(content(Vec::new(), Vec::new())),
            pastes: 0,
        };
        let error = run_paste(&mut no_pages, &invocation).expect_err("no pages");
        assert!(error.to_string().contains("no pages"));
    }

    #[test]
    fn copy_skips_a_child_whose_ancestor_is_selected() {
        let (mut ctx, _session, page) = context(true);
        let (group, child) = {
            let doc = &mut ctx.active_document_mut().expect("session").document;
            let child = doc
                .insert_shape(page, Shape::new(ShapeId { index: 0 }, 0), None)
                .expect("child");
            let group = doc.group_shapes(page, &[child]).expect("group");
            (group, child)
        };
        ctx.selection.shapes = vec![group, child];
        ctx.selection.anchor = Some(child);
        ctx.selection.page = Some(page);
        let (copied, _) = content_from_selection(&ctx).expect("copy");
        assert_eq!(copied.shapes.len(), 2, "the child is copied once");

        // A lone child walks up to its unselected parent (the second
        // `ancestor_selected` loop iteration).
        ctx.selection.shapes = vec![child];
        ctx.selection.anchor = Some(child);
        let (copied, _) = content_from_selection(&ctx).expect("child copy");
        assert_eq!(copied.shapes.len(), 1);

        // A repeated selection entry is copied once (the seen set).
        ctx.selection.shapes = vec![child, child];
        let (copied, _) = content_from_selection(&ctx).expect("repeated child");
        assert_eq!(copied.shapes.len(), 1);
    }

    #[test]
    fn paste_rejects_an_empty_fragment_and_warns_on_foreign_connects() {
        let (mut ctx, session, page) = context(true);
        let empty = content(Vec::new(), Vec::new());
        let error = paste_content(&mut ctx, session, page, empty, Placement::Offset(0.0, 0.0))
            .map(|_| ())
            .expect_err("an empty fragment has no roots");
        assert!(error.to_string().contains("no root shape"));

        let shape = Shape::new(ShapeId { index: 0 }, 1);
        let mut connect = Connect::new();
        connect.from_sheet = 90;
        connect.to_sheet = 91;
        let fragment = content(vec![shape], vec![connect]);
        let (edit, roots, warnings) = paste_content(
            &mut ctx,
            session,
            page,
            fragment,
            Placement::Offset(0.0, 0.0),
        )
        .expect("paste");
        assert_eq!(edit.len(), 1);
        assert_eq!(roots.len(), 1);
        assert!(
            warnings
                .iter()
                .any(|warning| warning.contains("not copied")),
            "{warnings:?}"
        );
    }

    #[test]
    fn insert_descendants_ignores_defensive_mismatches() {
        let (mut ctx, _session, page) = context(true);
        let doc = &mut ctx.active_document_mut().expect("session").document;
        let parent = doc
            .insert_shape(page, Shape::new(ShapeId { index: 0 }, 0), None)
            .expect("parent");
        let by_id = BTreeMap::new();
        let copied = BTreeSet::new();
        let mut visio_map = BTreeMap::new();
        let fragment = content(Vec::new(), Vec::new());
        // index past the end
        insert_descendants(doc, &fragment, &by_id, &copied, parent, 9, &mut visio_map)
            .expect("no descendants");

        // A child that was not copied.
        let mut shape = Shape::new(ShapeId { index: 0 }, 1);
        shape.children.push(ShapeId { index: 7 });
        let fragment = content(vec![shape], Vec::new());
        insert_descendants(doc, &fragment, &by_id, &copied, parent, 0, &mut visio_map)
            .expect("child skipped");

        // A copied child missing from the index map.
        let mut copied = BTreeSet::new();
        copied.insert(7);
        insert_descendants(doc, &fragment, &by_id, &copied, parent, 0, &mut visio_map)
            .expect("missing index skipped");

        // A copied child whose index points past the fragment.
        let mut by_id = BTreeMap::new();
        by_id.insert(7, 9);
        insert_descendants(doc, &fragment, &by_id, &copied, parent, 0, &mut visio_map)
            .expect("missing shape skipped");
    }

    #[test]
    fn copy_skips_a_stale_child_id() {
        let (mut ctx, _session, page) = context(true);
        let root = {
            let doc = &mut ctx.active_document_mut().expect("session").document;
            let root = doc
                .insert_shape(page, Shape::new(ShapeId { index: 0 }, 0), None)
                .expect("root");
            if let Some(shape) = doc.shape_mut(root) {
                shape.children.push(ShapeId { index: 99 });
            }
            root
        };
        ctx.selection.shapes = vec![root];
        ctx.selection.anchor = Some(root);
        ctx.selection.page = Some(page);
        let (copied, _) = content_from_selection(&ctx).expect("copy");
        assert_eq!(copied.shapes.len(), 1, "the stale child is skipped");
    }

    #[test]
    fn ancestor_selected_stops_on_a_cycle() {
        let mut doc = Document::new();
        let a = doc.add_shape(Shape::new(ShapeId { index: 0 }, 1));
        let b = doc.add_shape(Shape::new(ShapeId { index: 0 }, 2));
        if let Some(shape) = doc.shape_mut(a) {
            shape.parent = Some(b);
        }
        if let Some(shape) = doc.shape_mut(b) {
            shape.parent = Some(a);
        }
        let selected = BTreeSet::new();
        assert!(!ancestor_selected(&doc, a, &selected));
    }

    #[test]
    fn pointer_arguments_accept_integers_and_require_a_pair() {
        let mut args = Args::new();
        args.set("x", ArgValue::Integer(1));
        args.set("y", ArgValue::Integer(2));
        assert_eq!(pointer_offset(&args).expect("integers"), Some((1.0, 2.0)));
        args.set("y", ArgValue::Integer(-2));
        assert_eq!(
            pointer_offset(&args).expect("negative y"),
            Some((1.0, -2.0))
        );
    }

    /// Task P5-19 coverage: the `?` of `paste_content` in `run_paste`
    /// (a fragment with no root shape).
    #[test]
    fn run_paste_reports_a_fragment_with_no_roots() {
        let (mut ctx, _session, _page) = context(true);
        ctx.clipboard = Clipboard {
            content: Some(content(Vec::new(), Vec::new())),
            pastes: 0,
        };
        let error = run_paste(&mut ctx, &crate::Invocation::empty(crate::Source::Test))
            .expect_err("no roots");
        assert!(error.to_string().contains("no root shape"), "{error}");
    }

    /// Task P5-19 coverage: the `?` of `paste_content` in
    /// `run_duplicate` (the copy succeeded but the paste step failed —
    /// here the page's Visio ids are exhausted).
    #[test]
    fn run_duplicate_reports_a_failed_paste_step() {
        let (mut ctx, _session, page) = context(true);
        let shape = {
            let doc = &mut ctx.active_document_mut().expect("session").document;
            let id = doc
                .insert_shape(page, Shape::new(ShapeId { index: 0 }, 0), None)
                .expect("shape");
            doc.shape_mut(id).expect("shape").visio_id = i32::MAX;
            id
        };
        ctx.selection.shapes = vec![shape];
        ctx.selection.anchor = Some(shape);
        ctx.selection.page = Some(page);
        let error = run_duplicate(&mut ctx, &crate::Invocation::empty(crate::Source::Test))
            .map(|_| ())
            .expect_err("id exhaustion");
        assert!(error.to_string().contains("too many shapes"), "{error}");
    }

    /// Task P7-20: `edit.paste.in.place` pastes at the recorded
    /// coordinates (no paste offset, no counter bump) and rejects a
    /// pointer pair.
    #[test]
    fn paste_in_place_keeps_the_coordinates() {
        let (mut ctx, _session, page) = context(true);
        let shape = {
            let doc = &mut ctx.active_document_mut().expect("session").document;
            let shape = doc
                .insert_shape(page, Shape::new(ShapeId { index: 0 }, 1), None)
                .expect("shape");
            for (cell, value) in [("PinX", 2.0), ("PinY", 3.0)] {
                doc.write_cell(
                    larrygram_model::SheetRef::Shape(shape),
                    &CellName::from_static(cell),
                    &CellWrite::new(number_literal(value), CellValue::Number(value)),
                )
                .expect("write");
            }
            shape
        };
        ctx.selection.shapes = vec![shape];
        ctx.selection.anchor = Some(shape);
        ctx.selection.page = Some(page);
        run_copy(&mut ctx, &crate::Invocation::empty(crate::Source::Test)).expect("copy");

        let outcome = run_paste_in_place(&mut ctx, &crate::Invocation::empty(crate::Source::Test))
            .expect("in place");
        assert_eq!(outcome.message.as_deref(), Some("Pasted 1 shapes in place"));
        let pasted = *ctx.selection.shapes.first().expect("pasted root");
        let doc = &ctx.active_document().expect("session").document;
        let (pin_x, _) = read_cell_number(doc, pasted, "PinX").expect("PinX");
        let (pin_y, _) = read_cell_number(doc, pasted, "PinY").expect("PinY");
        assert_eq!((pin_x, pin_y), (2.0, 3.0), "no paste offset");
        assert_eq!(ctx.clipboard.pastes, 0, "the offset counter stays put");

        // The plain paste offsets by (0.1, -0.1) and advances it.
        run_paste(&mut ctx, &crate::Invocation::empty(crate::Source::Test)).expect("paste");
        let pasted = *ctx.selection.shapes.first().expect("pasted root");
        let doc = &ctx.active_document().expect("session").document;
        let (pin_x, _) = read_cell_number(doc, pasted, "PinX").expect("PinX");
        let (pin_y, _) = read_cell_number(doc, pasted, "PinY").expect("PinY");
        assert!((pin_x - 2.1).abs() < 1e-9, "{pin_x}");
        assert!((pin_y - 2.9).abs() < 1e-9, "{pin_y}");
        assert_eq!(ctx.clipboard.pastes, 1);

        // A pointer pair is rejected, not ignored.
        let args = crate::Args::new().with("x", crate::ArgValue::Number(1.0));
        let error =
            run_paste_in_place(&mut ctx, &crate::Invocation::new(args, crate::Source::Test))
                .expect_err("x");
        assert!(error.to_string().contains("edit.paste"), "{error}");
    }
}
