//! `arrange.*` commands (P5-09).
//!
//! Arrangement commands: align, distribute, group and z-order (ADR
//! 0047). Every command returns **one** edit (`CompositeEdit` when it
//! touches several shapes) and bumps the document change counter through
//! that edit (ADR 0040).
//!
//! Bounds are the shape frame: `PinX`/`PinY`, `LocPinX`/`LocPinY` and
//! `Width`/`Height`, read through the inheritance lookup. Rotation is
//! ignored (the acceptance tests use unrotated shapes); the black-box
//! check against Visio's rotated-shape alignment is on the black-box
//! check list.
//!
//! Group and ungroup are wrappers around `Document::group_shapes` /
//! `Document::ungroup_shape` (ADR 0039). Their undo replays the model
//! primitive in the other direction: a group is dissolved by
//! `ungroup_shape` and re-created on redo. The dissolved group's arena
//! entry stays behind (ADR 0039 invariant 1), so the document equality
//! of the undo test is over the reachable page state, not the arena
//! length. That is the same rule as the P5-04 insert edits.

use larrygram_geom::{Point, Rect};
use larrygram_model::{
    CellName, CellReader, CellValue, CellWrite, Connect, Document, PageId, Shape, ShapeId, SheetRef,
};
use larrygram_shapesheet::CellAddress;

use crate::app::{AppCtx, SessionId};
use crate::edit::CompositeEdit;
use crate::error::CommandError;
use crate::recalc::subtree_addresses;
use crate::undo::Edit;
use crate::{Invocation, Outcome};

use super::connector::reroute_quietly;
use super::edit::{
    CellEdit, PageKey, count_label, document_mut, number_literal, page_key, resolve_page,
    selection_targets,
};
use super::{CommandTable, edit_outcome, has_selection, model_error, warning_line};

/// The implemented arrangement commands.
pub(crate) const TABLE: CommandTable = &[
    (
        "arrange.align.left",
        "Align Left",
        has_selection,
        run_align_left,
    ),
    (
        "arrange.align.center_x",
        "Align Center",
        has_selection,
        run_align_center_x,
    ),
    (
        "arrange.align.right",
        "Align Right",
        has_selection,
        run_align_right,
    ),
    (
        "arrange.align.top",
        "Align Top",
        has_selection,
        run_align_top,
    ),
    (
        "arrange.align.middle",
        "Align Middle",
        has_selection,
        run_align_middle,
    ),
    (
        "arrange.align.bottom",
        "Align Bottom",
        has_selection,
        run_align_bottom,
    ),
    (
        "arrange.distribute.horizontal",
        "Distribute Horizontally",
        has_selection,
        run_distribute_horizontal,
    ),
    (
        "arrange.distribute.vertical",
        "Distribute Vertically",
        has_selection,
        run_distribute_vertical,
    ),
    ("arrange.group", "Group", has_selection, run_group),
    ("arrange.ungroup", "Ungroup", can_ungroup, run_ungroup),
    (
        "arrange.bring_forward",
        "Bring Forward",
        has_selection,
        run_bring_forward,
    ),
    (
        "arrange.send_backward",
        "Send Backward",
        has_selection,
        run_send_backward,
    ),
    (
        "arrange.bring_to_front",
        "Bring to Front",
        has_selection,
        run_bring_to_front,
    ),
    (
        "arrange.send_to_back",
        "Send to Back",
        has_selection,
        run_send_to_back,
    ),
];

/// Which edge or centre an align run uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Align {
    /// Left edges.
    Left,
    /// Horizontal centres.
    CenterX,
    /// Right edges.
    Right,
    /// Top edges.
    Top,
    /// Vertical centres.
    Middle,
    /// Bottom edges.
    Bottom,
}

impl Align {
    /// The command's message and undo label.
    fn label(self, count: usize) -> String {
        let what = match self {
            Align::Left => "left",
            Align::CenterX => "centre",
            Align::Right => "right",
            Align::Top => "top",
            Align::Middle => "middle",
            Align::Bottom => "bottom",
        };
        format!("Align {count} shapes {what}")
    }
}

/// A selected shape's frame and pin, in page inches.
#[derive(Debug, Clone, Copy)]
struct Target {
    /// Arena id.
    id: ShapeId,
    /// The shape's frame (rotation ignored).
    rect: Rect,
    /// The shape's `PinX`/`PinY`.
    pin: Point,
}

fn run_align_left(ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
    align(ctx, Align::Left)
}

fn run_align_center_x(ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
    align(ctx, Align::CenterX)
}

fn run_align_right(ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
    align(ctx, Align::Right)
}

fn run_align_top(ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
    align(ctx, Align::Top)
}

fn run_align_middle(ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
    align(ctx, Align::Middle)
}

fn run_align_bottom(ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
    align(ctx, Align::Bottom)
}

/// Aligns every selected shape to the selection's bounds (ADR 0047).
fn align(ctx: &mut AppCtx, align: Align) -> Result<Outcome, CommandError> {
    let (session, targets, warnings) = targets(ctx)?;
    let bounds = union(&targets);
    let mut edit = CompositeEdit::new(align.label(targets.len()));
    {
        let doc = document_mut(ctx, session)?;
        for target in &targets {
            let (dx, dy) = match align {
                Align::Left => (bounds.min.x - target.rect.min.x, 0.0),
                Align::CenterX => (centre_x(bounds) - centre_x(target.rect), 0.0),
                Align::Right => (bounds.max.x - target.rect.max.x, 0.0),
                Align::Top => (0.0, bounds.max.y - target.rect.max.y),
                Align::Middle => (0.0, centre_y(bounds) - centre_y(target.rect)),
                Align::Bottom => (0.0, bounds.min.y - target.rect.min.y),
            };
            if let Some(step) = move_cell_edit(doc, session, "Align shape", target, dx, dy)? {
                edit.push(step);
            }
        }
    }
    edit.apply(ctx)?;
    reroute_quietly(ctx);
    Ok(edit_outcome(
        align.label(targets.len()),
        warnings,
        Box::new(edit),
    ))
}

/// `arrange.distribute.horizontal`: equal gaps between the selected
/// shapes, leaving the leftmost and rightmost in place.
fn run_distribute_horizontal(ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
    distribute(ctx, Axis::Horizontal)
}

/// `arrange.distribute.vertical`: equal gaps, top and bottom fixed.
fn run_distribute_vertical(ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
    distribute(ctx, Axis::Vertical)
}

/// Which axis a distribute run spaces along.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Axis {
    /// Equal horizontal gaps.
    Horizontal,
    /// Equal vertical gaps.
    Vertical,
}

/// Spaces the selected shapes equally along `axis` (ADR 0047).
fn distribute(ctx: &mut AppCtx, axis: Axis) -> Result<Outcome, CommandError> {
    let (session, mut targets, warnings) = targets(ctx)?;
    let label = match axis {
        Axis::Horizontal => "Distribute horizontally",
        Axis::Vertical => "Distribute vertically",
    };
    let mut edit = CompositeEdit::new(format!("{label} ({} shapes)", targets.len()));
    if targets.len() > 1 {
        match axis {
            Axis::Horizontal => targets.sort_by(|a, b| {
                centre_x(a.rect)
                    .partial_cmp(&centre_x(b.rect))
                    .unwrap_or(std::cmp::Ordering::Equal)
            }),
            Axis::Vertical => targets.sort_by(|a, b| {
                centre_y(a.rect)
                    .partial_cmp(&centre_y(b.rect))
                    .unwrap_or(std::cmp::Ordering::Equal)
            }),
        }
        let bounds = union(&targets);
        let (span, sizes, first) = match axis {
            Axis::Horizontal => (
                bounds.width(),
                targets
                    .iter()
                    .map(|target| target.rect.width())
                    .collect::<Vec<_>>(),
                bounds.min.x,
            ),
            Axis::Vertical => (
                bounds.height(),
                targets
                    .iter()
                    .map(|target| target.rect.height())
                    .collect::<Vec<_>>(),
                bounds.min.y,
            ),
        };
        let gaps = u32::try_from(targets.len().saturating_sub(1)).unwrap_or(u32::MAX);
        let gap = (span - sizes.iter().sum::<f64>()) / f64::from(gaps);
        let mut cursor = first;
        {
            let doc = document_mut(ctx, session)?;
            for (index, target) in targets.iter().enumerate() {
                let size = sizes.get(index).copied().unwrap_or(0.0);
                let wanted = if index == 0 {
                    match axis {
                        Axis::Horizontal => target.rect.min.x,
                        Axis::Vertical => target.rect.min.y,
                    }
                } else {
                    cursor
                };
                let (dx, dy) = match axis {
                    Axis::Horizontal => (wanted - target.rect.min.x, 0.0),
                    Axis::Vertical => (0.0, wanted - target.rect.min.y),
                };
                if let Some(step) =
                    move_cell_edit(doc, session, "Distribute shape", target, dx, dy)?
                {
                    edit.push(step);
                }
                cursor = wanted + size + gap;
            }
        }
    }
    edit.apply(ctx)?;
    reroute_quietly(ctx);
    Ok(edit_outcome(label, warnings, Box::new(edit)))
}

/// The selected shapes with their frames and pins, and the page they are
/// on.
fn targets(ctx: &AppCtx) -> Result<(SessionId, Vec<Target>, Vec<String>), CommandError> {
    let (session, ids, mut warnings) = selection_targets(ctx)?;
    let doc = ctx
        .documents
        .iter()
        .find(|slot| slot.id == session)
        .map(|slot| &slot.document)
        .ok_or_else(|| CommandError::failed("the document session is gone"))?;
    let targets: Vec<Target> = ids
        .iter()
        .filter_map(|id| target_frame(doc, *id, &mut warnings).ok())
        .collect();
    Ok((session, targets, warnings))
}

/// Reads one shape's frame and pin.
fn target_frame(
    doc: &Document,
    id: ShapeId,
    warnings: &mut Vec<String>,
) -> Result<Target, CommandError> {
    let mut reader = CellReader::new(doc);
    let number = |reader: &mut CellReader<'_>, cell: &str| {
        reader
            .number(SheetRef::Shape(id), &CellName::new(cell))
            .map_err(model_error)
    };
    let width = number(&mut reader, "Width")?;
    let height = number(&mut reader, "Height")?;
    let pin_x = number(&mut reader, "PinX")?;
    let pin_y = number(&mut reader, "PinY")?;
    let loc_x = number(&mut reader, "LocPinX")?;
    let loc_y = number(&mut reader, "LocPinY")?;
    let recorded = reader.take_warnings();
    warnings.extend(recorded.iter().map(|warning| warning_line(doc, warning)));
    let min = Point::new(pin_x - loc_x, pin_y - loc_y);
    let max = Point::new(min.x + width, min.y + height);
    Ok(Target {
        id,
        rect: Rect::new(min, max),
        pin: Point::new(pin_x, pin_y),
    })
}

/// The union of the selected frames (an empty list is the origin).
fn union(targets: &[Target]) -> Rect {
    let first = targets
        .first()
        .map(|target| target.rect)
        .unwrap_or_default();
    targets
        .iter()
        .fold(first, |bounds, target| bounds.union(target.rect))
}

fn centre_x(rect: Rect) -> f64 {
    (rect.min.x + rect.max.x) * 0.5
}

fn centre_y(rect: Rect) -> f64 {
    (rect.min.y + rect.max.y) * 0.5
}

/// A cell edit that shifts one shape's pin by `(dx, dy)`, or `None` when
/// the delta is zero.
fn move_cell_edit(
    doc: &Document,
    session: SessionId,
    label: &str,
    target: &Target,
    dx: f64,
    dy: f64,
) -> Result<Option<Box<dyn Edit>>, CommandError> {
    if dx == 0.0 && dy == 0.0 {
        return Ok(None);
    }
    if !dx.is_finite() || !dy.is_finite() {
        return Err(CommandError::invalid_args(
            "the arrangement delta is not finite",
        ));
    }
    let pin = target.pin;
    let mut steps: Vec<Box<dyn Edit>> = Vec::new();
    if dx != 0.0 {
        let value = pin.x + dx;
        let step = CellEdit::new(
            doc,
            session,
            label,
            SheetRef::Shape(target.id),
            "PinX",
            CellWrite::new(number_literal(value), CellValue::Number(value)),
        )?;
        steps.push(Box::new(step));
    }
    if dy != 0.0 {
        let value = pin.y + dy;
        let step = CellEdit::new(
            doc,
            session,
            label,
            SheetRef::Shape(target.id),
            "PinY",
            CellWrite::new(number_literal(value), CellValue::Number(value)),
        )?;
        steps.push(Box::new(step));
    }
    let mut edit = CompositeEdit::new(label);
    for step in steps {
        edit.push(step);
    }
    Ok(Some(Box::new(edit)))
}

/// `arrange.group`: groups the selection into a new group shape.
fn run_group(ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
    let (session, targets, warnings) = selection_targets(ctx)?;
    let doc = ctx
        .documents
        .iter()
        .find(|slot| slot.id == session)
        .map(|slot| &slot.document)
        .ok_or_else(|| CommandError::failed("the document session is gone"))?;
    let page = targets
        .first()
        .and_then(|id| doc.page_of(*id))
        .ok_or_else(|| CommandError::failed("the selected shapes are not on a page"))?;
    let key = page_key(doc, page)?;
    let mut edit = GroupEdit::new(session, key, targets.clone());
    edit.apply(ctx)?;
    reroute_quietly(ctx);
    let group = edit.group;
    if let Some(group) = group {
        select(ctx, session, page, &[group]);
    }
    Ok(edit_outcome(
        count_label("Group", targets.len()),
        warnings,
        Box::new(edit),
    ))
}

/// The shapes' snapshots with their list positions: the exact-inverse
/// seam of the group edits (task P5-11: undo restores each member's
/// sheet and its position, not just its existence).
fn capture_shapes(doc: &Document, ids: &[ShapeId]) -> Result<Vec<(Shape, usize)>, CommandError> {
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        let entry = doc
            .shape(*id)
            .ok_or_else(|| CommandError::failed("a grouped shape is gone"))?;
        let list: Vec<ShapeId> = match entry.parent {
            Some(owner) => doc
                .shape(owner)
                .map(|owner| owner.children.clone())
                .unwrap_or_default(),
            None => doc
                .page_of(*id)
                .and_then(|page| doc.page(page))
                .map(|page| page.shapes.clone())
                .unwrap_or_default(),
        };
        let index = list.iter().position(|entry| entry == id).unwrap_or(0);
        out.push((entry.clone(), index));
    }
    Ok(out)
}

/// Inserts `shape` at `index` of its page or parent list
/// (`Document::set_z_order` is top-level only).
fn place_at(doc: &mut Document, page: PageId, shape: ShapeId, index: usize) {
    match doc.shape(shape).and_then(|entry| entry.parent) {
        Some(owner) => {
            if let Some(entity) = doc.shape_mut(owner) {
                entity.children.retain(|id| *id != shape);
                let at = index.min(entity.children.len());
                entity.children.insert(at, shape);
            }
        }
        None => {
            if let Some(entity) = doc.pages.get_mut(page.index as usize) {
                entity.shapes.retain(|id| *id != shape);
                let at = index.min(entity.shapes.len());
                entity.shapes.insert(at, shape);
            }
        }
    }
}

/// Re-creates a group at `made`'s arena slot instead of `fresh`.
///
/// An undo of a group detaches the group's slot, and a redo must reuse
/// it (task P5-11): later edits can name the old arena id (a text edit
/// on the group), and a fresh id would leave them without a subject.
fn reuse_slot(doc: &mut Document, page: PageId, fresh: ShapeId, made: Shape) -> ShapeId {
    let slot = made.id;
    if slot == fresh || doc.shape(slot).is_none() || doc.shape(fresh).is_none() {
        return fresh;
    }
    let children = doc
        .shape(fresh)
        .map(|entry| entry.children.clone())
        .unwrap_or_default();
    if let Some(entity) = doc.shape_mut(slot) {
        *entity = made;
    }
    for child in &children {
        if let Some(entity) = doc.shape_mut(*child) {
            entity.parent = Some(slot);
        }
    }
    if let Some(entity) = doc.shape_mut(fresh) {
        entity.children.clear();
    }
    if let Some(entity) = doc.pages.get_mut(page.index as usize) {
        for id in &mut entity.shapes {
            if *id == fresh {
                *id = slot;
            }
        }
    }
    slot
}

/// Dissolves `group` and restores the members from `before` (their
/// exact shapes and list positions).
///
/// `Document::ungroup_shape` re-bases the children's cells through the
/// group's transform and leaves them at the group's slot; an undo step
/// needs the exact inverse (task P5-11), which is the snapshots.
fn dissolve(
    doc: &mut Document,
    page: PageId,
    group: ShapeId,
    before: &[(Shape, usize)],
) -> Result<Vec<ShapeId>, CommandError> {
    // The group leaves the page first (its own `Connects` go with it),
    // with its children list emptied so only the group is detached.
    if let Some(entity) = doc.shape_mut(group) {
        entity.children.clear();
    }
    doc.detach_shape(group).map_err(model_error)?;
    let mut children = Vec::with_capacity(before.len());
    for (shape, _) in before {
        if let Some(entity) = doc.shape_mut(shape.id) {
            *entity = shape.clone();
        }
        children.push(shape.id);
    }
    // Back at their recorded positions (ascending, so every index is
    // still the right slot).
    let mut order: Vec<(usize, ShapeId)> = before
        .iter()
        .map(|(shape, index)| (*index, shape.id))
        .collect();
    order.sort_by_key(|(index, _)| *index);
    for (index, id) in order {
        place_at(doc, page, id, index);
    }
    Ok(children)
}

/// One undoable group: `apply` groups the members, `revert` puts them
/// back exactly as they were (task P5-11).
struct GroupEdit {
    session: SessionId,
    page: PageKey,
    members: Vec<ShapeId>,
    group: Option<ShapeId>,
    label: String,
    /// The members' shapes and list positions before `apply` ran (a
    /// redo re-captures them).
    before: Vec<(Shape, usize)>,
    /// The group `apply` created (a redo re-creates it at the same
    /// arena slot, [`reuse_slot`]).
    made: Option<Shape>,
    /// The cells the members' re-based pins and the new group write
    /// (ADR 0056: filled in by `apply`, read by the recalc pass).
    dirty: Vec<CellAddress>,
}

impl GroupEdit {
    fn new(session: SessionId, page: PageKey, members: Vec<ShapeId>) -> Self {
        Self {
            session,
            page,
            label: count_label("Group", members.len()),
            members,
            group: None,
            before: Vec::new(),
            made: None,
            dirty: Vec::new(),
        }
    }
}

impl Edit for GroupEdit {
    fn label(&self) -> &str {
        &self.label
    }

    fn dirty_cells(&self) -> &[CellAddress] {
        &self.dirty
    }

    fn apply(&mut self, ctx: &mut AppCtx) -> Result<(), CommandError> {
        let session = self.session;
        let (page, group, dirty) = {
            let doc = document_mut(ctx, session)?;
            let page = resolve_page(doc, &self.page, PageId { index: 0 });
            self.before = capture_shapes(doc, &self.members)?;
            let mut dirty = Vec::new();
            for member in &self.members {
                dirty.extend(subtree_addresses(doc, *member));
            }
            let fresh = doc.group_shapes(page, &self.members).map_err(model_error)?;
            let group = match self.made.clone() {
                Some(made) => reuse_slot(doc, page, fresh, made),
                None => {
                    self.made = doc.shape(fresh).cloned();
                    fresh
                }
            };
            dirty.extend(subtree_addresses(doc, group));
            (page, group, dirty)
        };
        self.group = Some(group);
        self.dirty = dirty;
        ctx.touch_session(session);
        select(ctx, session, page, &[group]);
        Ok(())
    }

    fn revert(&mut self, ctx: &mut AppCtx) -> Result<(), CommandError> {
        let session = self.session;
        let (page, children) = {
            let doc = document_mut(ctx, session)?;
            let page = resolve_page(doc, &self.page, PageId { index: 0 });
            // A redo of an intervening ungroup re-created the group
            // under a new arena id; find it by its members.
            let group = group_for_members(doc, page, &self.members)
                .or(self.group.filter(|group| doc.page_of(*group) == Some(page)))
                .ok_or_else(|| CommandError::failed("nothing to ungroup"))?;
            let children = dissolve(doc, page, group, &self.before)?;
            (page, children)
        };
        self.group = None;
        ctx.touch_session(session);
        select(ctx, session, page, &children);
        Ok(())
    }
}

/// The top-level group on `page` whose direct children are exactly
/// `members`, if one exists (the stable identity across a re-created
/// group).
fn group_for_members(doc: &Document, page: PageId, members: &[ShapeId]) -> Option<ShapeId> {
    if members.is_empty() {
        return None;
    }
    let list = &doc.page(page)?.shapes;
    list.iter().copied().find(|id| {
        doc.shape(*id).is_some_and(|shape| {
            shape.children.len() == members.len()
                && members.iter().all(|member| shape.children.contains(member))
        })
    })
}

/// Enabled when at least one selected shape is a group.
///
/// The command dissolves groups only; a selection of ordinary shapes is
/// greyed out instead of producing a run-time warning (task P7-08
/// cross-check).
fn can_ungroup(ctx: &AppCtx) -> crate::Enabled {
    if ctx.selection.is_empty() {
        return crate::Enabled::no("no selection");
    }
    let Some(session) = ctx.active_document() else {
        return crate::Enabled::no("no open document");
    };
    let has_group = ctx.selection.shapes.iter().any(|shape| {
        session
            .document
            .shape(*shape)
            .is_some_and(|entity| !entity.children.is_empty())
    });
    if has_group {
        crate::Enabled::yes()
    } else {
        crate::Enabled::no("the selection has no group")
    }
}

/// `arrange.ungroup`: dissolves every selected group.
fn run_ungroup(ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
    let (session, targets, mut warnings) = selection_targets(ctx)?;
    let (page, key, groups) = {
        let doc = ctx
            .documents
            .iter()
            .find(|slot| slot.id == session)
            .map(|slot| &slot.document)
            .ok_or_else(|| CommandError::failed("the document session is gone"))?;
        let page = targets
            .first()
            .and_then(|id| doc.page_of(*id))
            .ok_or_else(|| CommandError::failed("the selected shapes are not on a page"))?;
        let key = page_key(doc, page)?;
        let mut groups = Vec::new();
        for id in &targets {
            let is_group = doc
                .shape(*id)
                .is_some_and(|shape| !shape.children.is_empty());
            if is_group {
                groups.push(*id);
            } else {
                warnings.push(format!("shape {} is not a group; skipped", id.index));
            }
        }
        (page, key, groups)
    };
    let mut edit = CompositeEdit::new(count_label("Ungroup", groups.len()));
    let mut children = Vec::new();
    for id in groups {
        let mut step = UngroupEdit::new(session, key.clone(), id);
        step.apply(ctx)?;
        children.extend(step.children.iter().copied());
        edit.push(Box::new(step));
    }
    if edit.is_empty() {
        return Err(CommandError::failed("no selected group to ungroup"));
    }
    reroute_quietly(ctx);
    select(ctx, session, page, &children);
    Ok(edit_outcome(
        count_label("Ungroup", edit.len()),
        warnings,
        Box::new(edit),
    ))
}

/// What one ungroup must put back (task P5-11): the group's exact
/// shape, list position and `Connects`, and its children's shapes.
struct UngroupSnapshot {
    /// The group's shape.
    shape: Shape,
    /// The group's index in its page or parent list.
    index: usize,
    /// The `Connects` records naming the group, with their list indices.
    connects: Vec<(usize, Connect)>,
    /// The children's shapes, in the group's child order.
    children: Vec<Shape>,
}

/// The snapshot `UngroupEdit::apply` takes before it dissolves a group.
fn capture_group(
    doc: &Document,
    page: PageId,
    group: ShapeId,
) -> Result<UngroupSnapshot, CommandError> {
    let (shape, index) = capture_shapes(doc, &[group])?
        .into_iter()
        .next()
        .ok_or_else(|| CommandError::failed("the group is gone"))?;
    let visio_id = shape.visio_id;
    let entity = doc
        .page(page)
        .ok_or_else(|| CommandError::failed("the page is gone"))?;
    let connects = entity
        .connects
        .iter()
        .enumerate()
        .filter(|(_, record)| record.from_sheet == visio_id || record.to_sheet == visio_id)
        .map(|(at, record)| (at, record.clone()))
        .collect();
    let mut children = Vec::new();
    for id in &shape.children {
        if let Some(entry) = doc.shape(*id) {
            children.push(entry.clone());
        }
    }
    Ok(UngroupSnapshot {
        shape,
        index,
        connects,
        children,
    })
}

/// Re-creates `group` from `snapshot` exactly (task P5-11): the group's
/// own shape and position, its children's shapes and its `Connects`.
/// Returns the arena id the group ended up at (its old slot, task P5-19
/// review F9: the selection must name the restored group, not the
/// discarded fresh slot).
fn regroup_exact(
    doc: &mut Document,
    page: PageId,
    fresh: ShapeId,
    snapshot: &UngroupSnapshot,
) -> Result<ShapeId, CommandError> {
    // The group comes back at its own arena slot (its old id), so edits
    // recorded against it keep their subject (task P5-11).
    let group = reuse_slot(doc, page, fresh, snapshot.shape.clone());
    if let Some(entity) = doc.shape_mut(group) {
        let mut restored = snapshot.shape.clone();
        restored.id = group;
        *entity = restored;
    }
    for child in &snapshot.children {
        if let Some(entity) = doc.shape_mut(child.id) {
            let mut restored = child.clone();
            restored.parent = Some(group);
            *entity = restored;
        }
    }
    if let Some(entity) = doc.pages.get_mut(page.index as usize) {
        let mut restored = snapshot.connects.clone();
        restored.sort_by_key(|(at, _)| *at);
        for (at, record) in restored {
            let at = at.min(entity.connects.len());
            entity.connects.insert(at, record);
        }
    }
    place_at(doc, page, group, snapshot.index);
    Ok(group)
}

/// One undoable ungroup: `apply` dissolves the group, `revert` puts the
/// exact group back (task P5-11).
struct UngroupEdit {
    session: SessionId,
    page: PageKey,
    group: Option<ShapeId>,
    children: Vec<ShapeId>,
    /// What `apply` must put back (a redo re-captures it).
    before: Option<UngroupSnapshot>,
    /// The cells the group and its members write (ADR 0056: filled in
    /// by `apply`, read by the recalc pass).
    dirty: Vec<CellAddress>,
}

impl UngroupEdit {
    fn new(session: SessionId, page: PageKey, group: ShapeId) -> Self {
        Self {
            session,
            page,
            group: Some(group),
            children: Vec::new(),
            before: None,
            dirty: Vec::new(),
        }
    }
}

impl Edit for UngroupEdit {
    fn label(&self) -> &str {
        "Ungroup shapes"
    }

    fn dirty_cells(&self) -> &[CellAddress] {
        &self.dirty
    }

    fn apply(&mut self, ctx: &mut AppCtx) -> Result<(), CommandError> {
        let session = self.session;
        let (page, children, dirty) = {
            let doc = document_mut(ctx, session)?;
            let page = resolve_page(doc, &self.page, PageId { index: 0 });
            // The group may have been re-created by an intervening
            // undo/redo of the group command, under a new arena id.
            let group = self
                .group
                .filter(|group| doc.page_of(*group) == Some(page))
                .or_else(|| group_for_members(doc, page, &self.children))
                .ok_or_else(|| CommandError::failed("nothing to ungroup"))?;
            self.before = Some(capture_group(doc, page, group)?);
            let dirty = subtree_addresses(doc, group);
            let children = doc.ungroup_shape(group).map_err(model_error)?;
            (page, children, dirty)
        };
        self.group = None;
        self.children = children.clone();
        self.dirty = dirty;
        ctx.touch_session(session);
        select(ctx, session, page, &children);
        Ok(())
    }

    fn revert(&mut self, ctx: &mut AppCtx) -> Result<(), CommandError> {
        if self.children.is_empty() {
            return Err(CommandError::failed("nothing to group"));
        }
        let session = self.session;
        let (page, group) = {
            let doc = document_mut(ctx, session)?;
            let page = resolve_page(doc, &self.page, PageId { index: 0 });
            let fresh = doc
                .group_shapes(page, &self.children)
                .map_err(model_error)?;
            // The restored group sits at its old arena slot; the
            // selection must name that id, not the discarded fresh one
            // (task P5-19 review F9).
            let group = match self.before.as_ref() {
                Some(snapshot) => regroup_exact(doc, page, fresh, snapshot)?,
                None => fresh,
            };
            (page, group)
        };
        self.group = Some(group);
        ctx.touch_session(session);
        select(ctx, session, page, &[group]);
        Ok(())
    }
}

fn run_bring_forward(ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
    z_order(ctx, ZOrder::Forward)
}

fn run_send_backward(ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
    z_order(ctx, ZOrder::Backward)
}

fn run_bring_to_front(ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
    z_order(ctx, ZOrder::Front)
}

fn run_send_to_back(ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
    z_order(ctx, ZOrder::Back)
}

/// Which direction a z-order command moves the selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ZOrder {
    /// Up one position.
    Forward,
    /// Down one position.
    Backward,
    /// Above every other shape.
    Front,
    /// Below every other shape.
    Back,
}

fn z_order(ctx: &mut AppCtx, order: ZOrder) -> Result<Outcome, CommandError> {
    let (session, targets, mut warnings) = selection_targets(ctx)?;
    let doc = ctx
        .documents
        .iter()
        .find(|slot| slot.id == session)
        .map(|slot| &slot.document)
        .ok_or_else(|| CommandError::failed("the document session is gone"))?;
    let page = targets
        .first()
        .and_then(|id| doc.page_of(*id))
        .ok_or_else(|| CommandError::failed("the selected shapes are not on a page"))?;
    let key = page_key(doc, page)?;
    let list = doc
        .page(page)
        .map(|page| page.shapes.clone())
        .unwrap_or_default();
    let len = list.len();
    let selected: std::collections::BTreeSet<u32> = targets.iter().map(|id| id.index).collect();
    let mut entries: Vec<(usize, ShapeId, usize)> = Vec::new();
    for (index, shape) in list.iter().enumerate() {
        if !selected.contains(&shape.index) {
            continue;
        }
        let target = match order {
            ZOrder::Forward => (index + 1).min(len.saturating_sub(1)),
            ZOrder::Backward => index.saturating_sub(1),
            ZOrder::Front => len.saturating_sub(1),
            ZOrder::Back => 0,
        };
        if target != index {
            entries.push((index, *shape, target));
        }
    }
    let skipped = targets.len().saturating_sub(entries.len());
    if skipped > 0 {
        warnings.push(format!(
            "{skipped} selected shape(s) are not top-level on the active page; skipped"
        ));
    }
    if entries.is_empty() {
        return Err(CommandError::failed(
            "no selected shape can move in z-order",
        ));
    }
    match order {
        ZOrder::Forward | ZOrder::Back => {
            entries.sort_by_key(|entry| std::cmp::Reverse(entry.0));
        }
        ZOrder::Backward | ZOrder::Front => entries.sort_by_key(|entry| entry.0),
    }
    let label = match order {
        ZOrder::Forward => "Bring forward",
        ZOrder::Backward => "Send backward",
        ZOrder::Front => "Bring to front",
        ZOrder::Back => "Send to back",
    };
    let mut edit = CompositeEdit::new(label);
    for (old, shape, target) in entries {
        edit.push(Box::new(ZOrderEdit {
            session,
            page: key.clone(),
            shape,
            old,
            target,
            label: label.to_owned(),
        }));
    }
    edit.apply(ctx)?;
    Ok(edit_outcome(label, warnings, Box::new(edit)))
}

/// One undoable z-order move.
///
/// `old` is the index the shape held when **this step's** `apply` ran:
/// a z-order command moves several shapes in one [`CompositeEdit`], so
/// the index it had before any of its siblings moved is stale by the
/// time `revert` runs (task P5-11: undo is the exact inverse).
struct ZOrderEdit {
    session: SessionId,
    page: PageKey,
    shape: ShapeId,
    old: usize,
    target: usize,
    label: String,
}

impl ZOrderEdit {
    fn move_to(&self, ctx: &mut AppCtx, index: usize) -> Result<(), CommandError> {
        let session = self.session;
        let doc = document_mut(ctx, session)?;
        let page = resolve_page(doc, &self.page, PageId { index: 0 });
        doc.set_z_order(page, self.shape, index)
            .map_err(model_error)?;
        ctx.touch_session(session);
        Ok(())
    }

    /// The shape's current index on its page list.
    fn current_index(&self, ctx: &mut AppCtx) -> Result<usize, CommandError> {
        let session = self.session;
        let doc = document_mut(ctx, session)?;
        let page = resolve_page(doc, &self.page, PageId { index: 0 });
        doc.page(page)
            .and_then(|entry| entry.shapes.iter().position(|id| *id == self.shape))
            .ok_or_else(|| CommandError::failed("the shape is not top-level on its page"))
    }
}

impl Edit for ZOrderEdit {
    fn label(&self) -> &str {
        &self.label
    }

    fn apply(&mut self, ctx: &mut AppCtx) -> Result<(), CommandError> {
        self.old = self.current_index(ctx)?;
        self.move_to(ctx, self.target)
    }

    fn revert(&mut self, ctx: &mut AppCtx) -> Result<(), CommandError> {
        self.move_to(ctx, self.old)
    }
}

/// Selects `shapes` when `session` is the active document (ADR 0041).
fn select(ctx: &mut AppCtx, session: SessionId, page: PageId, shapes: &[ShapeId]) {
    if ctx.active_document().map(|slot| slot.id) != Some(session) {
        return;
    }
    ctx.selection.shapes = shapes.to_vec();
    ctx.selection.anchor = shapes.last().copied();
    ctx.selection.page = Some(page);
    ctx.selection.entered = None;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{AppCtx, DocumentSession, Settings};
    use larrygram_model::{Cell, CellValue, Preserved};

    fn shape_cell(name: &str, value: f64) -> Cell {
        Cell {
            name: CellName::new(name),
            formula: Some(format!("{value}")),
            value: Some(CellValue::Number(value)),
            unit: larrygram_geom::Unit::Inches,
            preserved: Preserved::new(),
        }
    }

    fn context() -> (AppCtx, SessionId, PageId, ShapeId) {
        let mut doc = Document::new();
        let page = doc.add_page("Page-1").expect("page");
        let mut shape = larrygram_model::Shape::new(ShapeId { index: 0 }, 0);
        for (name, value) in [
            ("PinX", 0.0),
            ("PinY", 0.0),
            ("Width", 1.0),
            ("Height", 1.0),
            ("LocPinX", 0.5),
            ("LocPinY", 0.5),
        ] {
            shape.sheet.cells.push(shape_cell(name, value));
        }
        let id = doc.insert_shape(page, shape, None).expect("shape");
        let mut ctx = AppCtx::new(Settings::default());
        let index = ctx.open_document(DocumentSession::new(doc, None));
        let session = ctx.documents.get(index).expect("session").id;
        (ctx, session, page, id)
    }

    #[test]
    fn non_finite_frames_and_deltas_are_rejected() {
        let (ctx, session, _page, id) = context();
        let doc = ctx.active_document().expect("session").document.clone();
        let mut warnings = Vec::new();
        let target = target_frame(&doc, id, &mut warnings).expect("a finite frame");
        assert_eq!(target.id, id);
        assert!(move_cell_edit(&doc, session, "Move", &target, f64::NAN, 0.0).is_err());
        assert!(move_cell_edit(&doc, session, "Move", &target, 0.0, f64::NAN).is_err());
    }

    /// Criterion 6 (task P6-07): `GroupEdit` and `UngroupEdit` write
    /// cells (the re-based pins and the new group), so they report them.
    #[test]
    fn group_and_ungroup_edits_dirty_their_cells() {
        let (mut ctx, session, page, first) = context();
        let second = {
            let doc = document_mut(&mut ctx, session).expect("doc");
            let mut shape = larrygram_model::Shape::new(ShapeId { index: 0 }, 0);
            for (name, value) in [
                ("PinX", 2.0),
                ("PinY", 0.0),
                ("Width", 1.0),
                ("Height", 1.0),
                ("LocPinX", 0.5),
                ("LocPinY", 0.5),
            ] {
                shape.sheet.cells.push(shape_cell(name, value));
            }
            doc.insert_shape(page, shape, None).expect("shape")
        };
        let key = PageKey {
            visio_id: 1,
            name_u: "Page-1".to_owned(),
        };
        let mut group = GroupEdit::new(session, key.clone(), vec![first, second]);
        assert!(group.dirty_cells().is_empty(), "apply fills the set");
        group.apply(&mut ctx).expect("group");
        let dirty = group.dirty_cells();
        assert!(!dirty.is_empty(), "{dirty:?}");
        assert!(
            dirty
                .iter()
                .any(|address| address.sheet == SheetRef::Shape(first)),
            "the members' pins are re-based"
        );
        let members: Vec<ShapeId> = dirty
            .iter()
            .filter_map(|address| match address.sheet {
                SheetRef::Shape(id) => Some(id),
                _ => None,
            })
            .collect();
        assert!(members.len() > 2, "the new group's cells are in the set");

        let group_id = ctx
            .active_document()
            .expect("session")
            .document
            .pages
            .get(page.index as usize)
            .and_then(|entry| entry.shapes.first().copied())
            .expect("group");
        let mut ungroup = UngroupEdit::new(session, key, group_id);
        ungroup.apply(&mut ctx).expect("ungroup");
        assert!(!ungroup.dirty_cells().is_empty());
        assert!(
            ungroup
                .dirty_cells()
                .iter()
                .any(|address| address.sheet == SheetRef::Shape(group_id)),
            "the dissolved group's cells are in the set"
        );
    }

    #[test]
    fn group_helpers_labels_and_empty_reverts_are_explicit() {
        assert!(group_for_members(&Document::new(), PageId { index: 0 }, &[]).is_none());

        let (mut ctx, session, _page, id) = context();
        let key = PageKey {
            visio_id: 1,
            name_u: "Page-1".to_owned(),
        };
        let mut ungroup = UngroupEdit::new(session, key.clone(), id);
        assert_eq!(Edit::label(&ungroup), "Ungroup shapes");
        let error = ungroup.revert(&mut ctx).expect_err("no children");
        assert!(error.to_string().contains("nothing to group"));

        let order = ZOrderEdit {
            session,
            page: key,
            shape: id,
            old: 0,
            target: 1,
            label: "Move shape".to_owned(),
        };
        assert_eq!(Edit::label(&order), "Move shape");

        // Selecting for a session that is no longer active is a no-op.
        let other = ctx.open_document(DocumentSession::new(Document::new(), None));
        let other_id = ctx.documents.get(other).expect("session").id;
        assert_ne!(other_id, session);
        select(&mut ctx, session, PageId { index: 0 }, &[id]);
        assert!(ctx.selection.is_empty());
    }

    /// Task P5-19 coverage: the two `CellEdit` staging `?` arms of
    /// `move_cell_edit` (a shape that vanished between the stages).
    #[test]
    fn move_cell_edit_reports_a_stale_target() {
        let (ctx, session, _page, _id) = context();
        let doc = &ctx.active_document().expect("session").document;
        let stale = Target {
            id: ShapeId { index: 9 },
            rect: Rect::new(Point::new(0.0, 0.0), Point::new(1.0, 1.0)),
            pin: Point::new(1.0, 2.0),
        };
        let error = move_cell_edit(doc, session, "Move", &stale, 1.0, 0.0)
            .map(|_| ())
            .expect_err("stale x");
        assert!(error.to_string().contains("unknown shape id"), "{error}");
        let error = move_cell_edit(doc, session, "Move", &stale, 0.0, 1.0)
            .map(|_| ())
            .expect_err("stale y");
        assert!(error.to_string().contains("unknown shape id"), "{error}");
    }

    /// Task P5-19 review F9 coverage: `UngroupEdit::revert` before a
    /// snapshot exists keeps the fresh group (nothing to restore yet).
    #[test]
    fn an_ungroup_revert_without_a_snapshot_keeps_the_fresh_group() {
        let (mut ctx, session, page, id) = context();
        let key = {
            let doc = &ctx.active_document().expect("session").document;
            page_key(doc, page).expect("key")
        };
        let mut ungroup = UngroupEdit::new(session, key, id);
        ungroup.children = vec![id];
        ungroup.revert(&mut ctx).expect("revert");
    }

    /// Task P5-19 coverage: the defensive list arms of `place_at`,
    /// `reuse_slot` and `regroup_exact` (a stale owner, a stale slot or
    /// a stale page).
    #[test]
    fn placement_helpers_tolerate_a_stale_owner_slot_or_page() {
        let (mut ctx, _session, page, id) = context();
        let doc = &mut ctx.active_document_mut().expect("session").document;
        if let Some(entry) = doc.shape_mut(id) {
            entry.parent = Some(ShapeId { index: 9 });
        }
        place_at(doc, page, id, 0);
        if let Some(entry) = doc.shape_mut(id) {
            entry.parent = None;
        }
        place_at(doc, PageId { index: 9 }, id, 0);

        let made = doc.shape(id).expect("shape").clone();
        let second = doc
            .insert_shape(
                page,
                larrygram_model::Shape::new(ShapeId { index: 0 }, 2),
                None,
            )
            .expect("second");
        assert_eq!(reuse_slot(doc, page, id, made.clone()), id, "same slot");
        assert_eq!(
            reuse_slot(doc, page, ShapeId { index: 9 }, made.clone()),
            ShapeId { index: 9 },
            "a stale fresh slot is returned as-is"
        );
        assert_eq!(
            reuse_slot(doc, PageId { index: 9 }, second, made.clone()),
            id,
            "a stale page skips the list rewrite"
        );

        let snapshot = UngroupSnapshot {
            shape: made.clone(),
            index: 0,
            connects: Vec::new(),
            children: Vec::new(),
        };
        assert_eq!(
            regroup_exact(doc, PageId { index: 9 }, id, &snapshot).expect("regroup"),
            id
        );
    }
}
