//! Undo and redo (ADR 0028).
//!
//! An [`Edit`] is one undoable step: `apply` redoes it, `revert` undoes
//! it, both over the application state. Commands return their edit in
//! [`Outcome::edit`](crate::Outcome::edit); the dispatcher pushes it onto
//! the per-document [`UndoStack`] (ADR 0029). Phase 4 commands produce
//! no edits; Phase 5 fills the stack and property-tests that `revert` is
//! the inverse of `apply`.

use crate::app::AppCtx;
use crate::edit::CompositeEdit;
use crate::error::CommandError;
use larrygram_shapesheet::CellAddress;

/// Runs `step` with the undo-recording gate closed (ADR 0040 rule 2).
///
/// Every [`Edit`]'s `apply`/`revert` runs inside this: while it is
/// open, [`AppCtx::push_edit`] drops any edit a nested command or edit
/// tries to record and warns once. The gate is a saved-and-restored
/// flag, so nested runs keep it closed until the outermost step ends.
pub(crate) fn run_suspended<T>(ctx: &mut AppCtx, step: impl FnOnce(&mut AppCtx) -> T) -> T {
    let previous = ctx.set_undo_suspended(true);
    let out = step(ctx);
    ctx.set_undo_suspended(previous);
    out
}

/// One undoable step (ADR 0028).
///
/// Implementations hold whatever they need to replay the step; state
/// lives in [`AppCtx`]. `apply` and `revert` must not panic on any
/// state the stack can produce (AGENTS.md section 6).
///
/// Two rules every implementation keeps (ADR 0040, task P5-04):
///
/// - `apply`/`revert` never record an edit:
///   [`AppCtx::push_edit`] drops it and warns while the recording gate
///   is closed (which is whenever this trait's methods run).
/// - Both bump the change counter of the document they changed, with
///   [`AppCtx::touch_session`], after a successful mutation.
pub trait Edit: Send {
    /// Short label for the undo menu ("Move shape").
    fn label(&self) -> &str;

    /// Re-applies the step (redo).
    fn apply(&mut self, ctx: &mut AppCtx) -> Result<(), CommandError>;

    /// Reverses the step (undo).
    fn revert(&mut self, ctx: &mut AppCtx) -> Result<(), CommandError>;

    /// The cells this step writes, so the recalc pass knows what is
    /// dirty (ADR 0056).
    ///
    /// [`AppCtx::push_edit`](crate::AppCtx::push_edit) runs the pass over
    /// these cells after `apply` and again on every re-apply (the redo
    /// that follows a revert); `revert` restores what the pass changed by
    /// snapshot. The default is an empty dirty set: a step that changes
    /// no cell needs nothing recomputed.
    fn dirty_cells(&self) -> &[CellAddress] {
        &[]
    }
}

/// One open undo group (ADR 0040, amendment 2026-11-03): the edits
/// pushed since [`UndoStack::begin_group`], folded into one step by
/// [`UndoStack::end_group`] or reverted by
/// [`UndoStack::cancel_group`].
struct Group {
    /// The label of the one step the group becomes.
    label: String,
    /// The document's unsaved-changes state when the group opened, the
    /// derived [`crate::DocumentSession::is_dirty`] answer (task
    /// P7-42); [`UndoStack::cancel_group`] returns it so the caller
    /// can restore that state.
    unsaved: bool,
    /// The collected steps, in push order.
    edits: Vec<Box<dyn Edit>>,
}

/// The undo stack of one document (ADR 0028, ADR 0029).
///
/// The cursor splits the stack: entries before it are undoable, entries
/// after it are redoable. Pushing truncates the redo tail.
///
/// An open group (amendment 2026-11-03) collects every push into one
/// pending step instead of the stack; see [`UndoStack::begin_group`].
///
/// [`UndoStack::default`] builds a stack with
/// [`UndoStack::DEFAULT_CAPACITY`]: a defaulted stack keeps its edits
/// (P4-15 review minor 2). A deliberately silent stack is
/// `UndoStack::new(0)`, which is what `builtins::with_undo` uses as
/// its short-lived placeholder.
pub struct UndoStack {
    entries: Vec<Box<dyn Edit>>,
    cursor: usize,
    capacity: usize,
    group: Option<Group>,
}

impl Default for UndoStack {
    fn default() -> Self {
        Self::new(Self::DEFAULT_CAPACITY)
    }
}

impl std::fmt::Debug for UndoStack {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UndoStack")
            .field("len", &self.entries.len())
            .field("cursor", &self.cursor)
            .field("capacity", &self.capacity)
            .finish()
    }
}

impl UndoStack {
    /// The default stack capacity (ADR 0028).
    pub const DEFAULT_CAPACITY: usize = 100;

    /// An empty stack with a maximum number of entries.
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: Vec::new(),
            cursor: 0,
            capacity,
            group: None,
        }
    }

    /// Whether an undo step is available.
    ///
    /// A non-empty open group counts (task P7-41 item 5): its edits are
    /// one step from the stack even before [`UndoStack::end_group`]
    /// folds them, and Undo commits the interaction first and then
    /// undoes the folded step whole. An empty group counts for nothing.
    pub fn can_undo(&self) -> bool {
        self.cursor > 0
            || self
                .group
                .as_ref()
                .is_some_and(|group| !group.edits.is_empty())
    }

    /// Whether a redo step is available.
    ///
    /// Redo is disabled while a non-empty group is open (task P7-48
    /// item 5): its step is what Undo would reach first, and folding
    /// it drops the redo tail.
    pub fn can_redo(&self) -> bool {
        self.cursor < self.entries.len() && self.pending_label().is_none()
    }

    /// The unsaved-changes verdict the open group saw when it opened
    /// (task P7-48 item 6): what the drawing read as before the
    /// pending interaction. `None` without an open group.
    pub fn group_unsaved(&self) -> Option<bool> {
        self.group.as_ref().map(|group| group.unsaved)
    }

    /// The label of the open group's pending step (task P7-48 item
    /// 5): the step its `end_group` becomes. `None` without an open
    /// group or with an empty one.
    pub fn pending_label(&self) -> Option<&str> {
        self.group
            .as_ref()
            .filter(|group| !group.edits.is_empty())
            .map(|group| group.label.as_str())
    }

    /// Number of entries on the stack (undoable and redoable).
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True when the stack holds no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Drops every entry (document closed or saved). An open group is
    /// dropped with them.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.cursor = 0;
        self.group = None;
    }

    /// The labels of the undoable edits, oldest first (the undo menu).
    pub fn undo_labels(&self) -> Vec<&str> {
        self.entries
            .iter()
            .take(self.cursor)
            .map(|edit| edit.label())
            .collect()
    }

    /// The labels of the redoable edits, oldest first (the redo menu).
    pub fn redo_labels(&self) -> Vec<&str> {
        self.entries
            .iter()
            .skip(self.cursor)
            .map(|edit| edit.label())
            .collect()
    }

    /// Pushes a completed edit (the command already applied it).
    ///
    /// While a group is open the edit joins the group instead of the
    /// stack (ADR 0040, amendment 2026-11-03). Otherwise the redo tail
    /// after the cursor is dropped first (the new edit supersedes it)
    /// and the stack is clamped to its capacity by dropping the oldest
    /// entries.
    pub fn push(&mut self, edit: Box<dyn Edit>) {
        if let Some(group) = &mut self.group {
            group.edits.push(edit);
            return;
        }
        self.entries.truncate(self.cursor);
        self.entries.push(edit);
        if self.entries.len() > self.capacity {
            let drop = self.entries.len() - self.capacity;
            self.entries.drain(..drop);
        }
        self.cursor = self.entries.len();
    }

    /// Opens an undo group (ADR 0040, amendment 2026-11-03): every
    /// push until [`UndoStack::end_group`] or [`UndoStack::cancel_group`]
    /// collects into one step labelled `label`.
    ///
    /// `unsaved` is the document's unsaved-changes state as the group
    /// opened; [`UndoStack::cancel_group`] returns it so the caller can
    /// restore that state. Groups do not nest: a second `begin_group`
    /// while one is open is an error.
    pub fn begin_group(
        &mut self,
        label: impl Into<String>,
        unsaved: bool,
    ) -> Result<(), CommandError> {
        if self.group.is_some() {
            return Err(CommandError::failed("an undo group is already open"));
        }
        self.group = Some(Group {
            label: label.into(),
            unsaved,
            edits: Vec::new(),
        });
        Ok(())
    }

    /// Whether a group is open.
    pub fn has_group(&self) -> bool {
        self.group.is_some()
    }

    /// Closes the open group: its edits become one [`CompositeEdit`]
    /// under the group's label, pushed as one step. An empty group
    /// pushes nothing. Without an open group this does nothing.
    pub fn end_group(&mut self) {
        let Some(group) = self.group.take() else {
            return;
        };
        if group.edits.is_empty() {
            return;
        }
        let mut composite = CompositeEdit::new(group.label);
        for edit in group.edits {
            composite.push(edit);
        }
        self.push(Box::new(composite));
    }

    /// Cancels the open group: the collected edits are reverted in
    /// reverse order with the recording gate closed (ADR 0040 rule 2)
    /// and discarded. The document is as it was when the group opened.
    ///
    /// Returns the unsaved-changes state the group saw when it opened
    /// (`Ok(None)` without an open group), so the caller can restore it;
    /// the change counter still only moves forward (rule 3).
    ///
    /// A failing `revert` (task P7-41 item 6) stops the rollback: the
    /// edits that were not reverted — the failing one and the ones its
    /// failure spared — are pushed as one step labelled as the group,
    /// so the stack stays consistent and their undo finishes the
    /// rollback later. The failure is returned as [`CommandError`], and
    /// the caller keeps the current unsaved state (a partial revert
    /// must never mark the drawing saved).
    pub fn cancel_group(&mut self, ctx: &mut AppCtx) -> Result<Option<bool>, CommandError> {
        let Some(mut group) = self.group.take() else {
            return Ok(None);
        };
        let failure = run_suspended(ctx, |ctx| {
            let mut failure = None;
            while let Some(edit) = group.edits.last_mut() {
                match edit.revert(ctx) {
                    Ok(()) => {
                        group.edits.pop();
                    }
                    Err(error) => {
                        failure = Some(error);
                        break;
                    }
                }
            }
            failure
        });
        let Some(error) = failure else {
            return Ok(Some(group.unsaved));
        };
        if !group.edits.is_empty() {
            let mut composite = CompositeEdit::new(group.label.clone());
            for edit in group.edits {
                composite.push(edit);
            }
            self.push(Box::new(composite));
        }
        Err(error)
    }

    /// Reverses the newest undoable edit and returns its label.
    ///
    /// An open group ends first (ADR 0040, amendment 2026-11-03), so
    /// its edits are one step before the undo runs.
    ///
    /// `Ok(None)` when nothing is undoable. A failing `revert` is
    /// returned as [`CommandError`] and leaves the cursor where it was.
    /// The call runs with the recording gate closed (ADR 0040).
    pub fn undo(&mut self, ctx: &mut AppCtx) -> Result<Option<String>, CommandError> {
        self.end_group();
        let Some(index) = self.cursor.checked_sub(1) else {
            return Ok(None);
        };
        let Some(edit) = self.entries.get_mut(index) else {
            return Ok(None);
        };
        run_suspended(ctx, |ctx| edit.revert(ctx))?;
        self.cursor = index;
        Ok(Some(edit.label().to_owned()))
    }

    /// Re-applies the newest redoable edit and returns its label.
    ///
    /// An open group ends first, the same as [`UndoStack::undo`].
    ///
    /// `Ok(None)` when nothing is redoable. A failing `apply` is
    /// returned as [`CommandError`] and leaves the cursor where it was.
    /// The call runs with the recording gate closed (ADR 0040).
    pub fn redo(&mut self, ctx: &mut AppCtx) -> Result<Option<String>, CommandError> {
        self.end_group();
        let Some(edit) = self.entries.get_mut(self.cursor) else {
            return Ok(None);
        };
        run_suspended(ctx, |ctx| edit.apply(ctx))?;
        self.cursor = self.cursor.saturating_add(1);
        Ok(Some(edit.label().to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Settings;

    struct NoopEdit;

    impl Edit for NoopEdit {
        fn label(&self) -> &str {
            "noop"
        }

        fn apply(&mut self, _ctx: &mut AppCtx) -> Result<(), CommandError> {
            Ok(())
        }

        fn revert(&mut self, _ctx: &mut AppCtx) -> Result<(), CommandError> {
            Ok(())
        }
    }

    /// An edit with a label of its own.
    struct NamedEdit(&'static str);

    impl Edit for NamedEdit {
        fn label(&self) -> &str {
            self.0
        }

        fn apply(&mut self, _ctx: &mut AppCtx) -> Result<(), CommandError> {
            Ok(())
        }

        fn revert(&mut self, _ctx: &mut AppCtx) -> Result<(), CommandError> {
            Ok(())
        }
    }

    /// An edit whose steps always fail.
    struct BadEdit;

    impl Edit for BadEdit {
        fn label(&self) -> &str {
            "bad"
        }

        fn apply(&mut self, _ctx: &mut AppCtx) -> Result<(), CommandError> {
            Err(CommandError::failed("apply failed"))
        }

        fn revert(&mut self, _ctx: &mut AppCtx) -> Result<(), CommandError> {
            Err(CommandError::failed("revert failed"))
        }
    }

    #[test]
    fn noop_edit_applies_and_reverts() {
        let mut edit = NoopEdit;
        let mut ctx = AppCtx::new(Settings::default());
        assert_eq!(edit.label(), "noop");
        assert!(edit.apply(&mut ctx).is_ok());
        assert!(edit.revert(&mut ctx).is_ok());
    }

    #[test]
    fn a_new_stack_holds_nothing() {
        let stack = UndoStack::new(UndoStack::DEFAULT_CAPACITY);
        assert!(!stack.can_undo());
        assert!(!stack.can_redo());
        assert!(stack.is_empty());
        assert_eq!(stack.len(), 0);
        assert_eq!(UndoStack::DEFAULT_CAPACITY, 100);
    }

    #[test]
    fn push_undo_and_redo_round_trip() {
        let mut stack = UndoStack::new(UndoStack::DEFAULT_CAPACITY);
        let mut ctx = AppCtx::new(Settings::default());
        stack.push(Box::new(NamedEdit("one")));
        stack.push(Box::new(NamedEdit("two")));
        assert!(stack.can_undo());
        assert!(!stack.can_redo());
        assert_eq!(stack.len(), 2);
        assert_eq!(stack.undo_labels(), vec!["one", "two"]);
        assert!(stack.redo_labels().is_empty());

        assert_eq!(stack.undo(&mut ctx).expect("undo").as_deref(), Some("two"));
        assert_eq!(stack.undo(&mut ctx).expect("undo").as_deref(), Some("one"));
        assert!(!stack.can_undo());
        assert_eq!(stack.undo_labels(), Vec::<&str>::new());
        assert_eq!(stack.redo_labels(), vec!["one", "two"]);

        assert_eq!(stack.redo(&mut ctx).expect("redo").as_deref(), Some("one"));
        assert_eq!(stack.redo(&mut ctx).expect("redo").as_deref(), Some("two"));
        assert!(!stack.can_redo());

        // Nothing left in either direction.
        assert_eq!(stack.undo(&mut ctx).unwrap().as_deref(), Some("two"));
        assert_eq!(stack.undo(&mut ctx).unwrap().as_deref(), Some("one"));
        assert_eq!(stack.undo(&mut ctx).unwrap(), None);
        assert_eq!(stack.redo(&mut ctx).unwrap().as_deref(), Some("one"));
        assert_eq!(stack.redo(&mut ctx).unwrap().as_deref(), Some("two"));
        assert_eq!(stack.redo(&mut ctx).unwrap(), None);
    }

    #[test]
    fn push_truncates_the_redo_tail() {
        let mut stack = UndoStack::new(UndoStack::DEFAULT_CAPACITY);
        let mut ctx = AppCtx::new(Settings::default());
        stack.push(Box::new(NamedEdit("one")));
        stack.push(Box::new(NamedEdit("two")));
        stack.undo(&mut ctx).expect("undo");
        assert!(stack.can_redo());
        stack.push(Box::new(NamedEdit("three")));
        assert!(!stack.can_redo());
        assert_eq!(stack.len(), 2);
        assert_eq!(stack.undo_labels(), vec!["one", "three"]);
    }

    #[test]
    fn push_clamps_to_the_capacity() {
        let mut stack = UndoStack::new(2);
        for label in ["one", "two", "three"] {
            stack.push(Box::new(NamedEdit(label)));
        }
        assert_eq!(stack.len(), 2);
        assert_eq!(stack.undo_labels(), vec!["two", "three"]);
        stack.push(Box::new(NamedEdit("four")));
        assert_eq!(stack.undo_labels(), vec!["three", "four"]);

        // A zero-capacity stack keeps nothing undoable.
        let mut stack = UndoStack::new(0);
        stack.push(Box::new(NamedEdit("one")));
        assert!(stack.is_empty());
        assert!(!stack.can_undo());
    }

    #[test]
    fn a_failing_edit_leaves_the_cursor_where_it_was() {
        let mut stack = UndoStack::new(UndoStack::DEFAULT_CAPACITY);
        let mut ctx = AppCtx::new(Settings::default());
        stack.push(Box::new(BadEdit));
        let error = stack.undo(&mut ctx).expect_err("revert fails");
        assert_eq!(error.to_string(), "revert failed");
        assert!(stack.can_undo(), "the cursor did not move");

        let mut stack = UndoStack::new(UndoStack::DEFAULT_CAPACITY);
        stack.push(Box::new(BadEdit));
        stack.undo(&mut ctx).expect_err("revert fails");
        stack.push(Box::new(NamedEdit("good")));
        assert_eq!(stack.undo_labels(), vec!["bad", "good"]);
    }

    /// The failing `apply` of `BadEdit` and the "nothing to redo"
    /// answer of a stack whose cursor sits at the end.
    #[test]
    fn a_failing_apply_and_an_empty_redo_answer_cleanly() {
        let mut edit = BadEdit;
        let mut ctx = AppCtx::new(Settings::default());
        let error = edit.apply(&mut ctx).expect_err("apply fails");
        assert_eq!(error.to_string(), "apply failed");

        let mut stack = UndoStack::new(UndoStack::DEFAULT_CAPACITY);
        assert!(stack.redo(&mut ctx).expect("redo").is_none());
        assert!(stack.undo(&mut ctx).expect("undo").is_none());
    }

    #[test]
    fn clear_drops_everything() {
        let mut stack = UndoStack::default();
        let mut ctx = AppCtx::new(Settings::default());
        stack.push(Box::new(NoopEdit));
        stack.undo(&mut ctx).expect("undo");
        stack.clear();
        assert!(stack.is_empty());
        assert!(!stack.can_undo());
        assert!(!stack.can_redo());
    }

    #[test]
    fn the_label_views_are_empty_on_a_new_stack() {
        let stack = UndoStack::new(UndoStack::DEFAULT_CAPACITY);
        assert!(stack.undo_labels().is_empty());
        assert!(stack.redo_labels().is_empty());
    }

    /// P4-15 review minor 2: a defaulted stack keeps its edits (the
    /// old capacity-0 default silently dropped every push). The silent
    /// placeholder is the explicit `UndoStack::new(0)`.
    #[test]
    fn a_default_stack_keeps_its_entries() {
        let mut stack = UndoStack::default();
        let mut ctx = AppCtx::new(Settings::default());
        stack.push(Box::new(NamedEdit("one")));
        stack.push(Box::new(NamedEdit("two")));
        assert_eq!(stack.len(), 2);
        assert_eq!(stack.undo(&mut ctx).expect("undo").as_deref(), Some("two"));

        let mut silent = UndoStack::new(0);
        silent.push(Box::new(NamedEdit("dropped")));
        assert!(silent.is_empty());
        assert!(!silent.can_undo());
    }

    /// P4-12 coverage minor 5: the `Debug` impl reports the shape.
    #[test]
    fn debug_shows_len_cursor_and_capacity() {
        let mut stack = UndoStack::new(2);
        stack.push(Box::new(NamedEdit("one")));
        let text = format!("{stack:?}");
        assert!(text.contains("len: 1"), "{text}");
        assert!(text.contains("cursor: 1"), "{text}");
        assert!(text.contains("capacity: 2"), "{text}");
    }

    /// An edit that runs while `revert`/`apply` records a nested edit
    /// and notes whether the gate was closed (ADR 0040 rule 2).
    struct NestedEdit {
        session: crate::SessionId,
        gate_was_closed: std::sync::Arc<std::sync::Mutex<Vec<bool>>>,
    }

    impl Edit for NestedEdit {
        fn label(&self) -> &str {
            "nested"
        }

        fn apply(&mut self, ctx: &mut AppCtx) -> Result<(), CommandError> {
            self.push(ctx)
        }

        fn revert(&mut self, ctx: &mut AppCtx) -> Result<(), CommandError> {
            self.push(ctx)
        }
    }

    impl NestedEdit {
        fn push(&self, ctx: &mut AppCtx) -> Result<(), CommandError> {
            if let Ok(mut seen) = self.gate_was_closed.lock() {
                seen.push(ctx.undo_recording_suspended());
            }
            ctx.push_edit(self.session, Box::new(NamedEdit("dropped")))
        }
    }

    /// ADR 0040 rule 2: undo and redo run the edit with the recording
    /// gate closed, so a nested push is dropped with one warning.
    #[test]
    fn undo_and_redo_run_with_the_recording_gate_closed() {
        let mut ctx = AppCtx::new(Settings::default());
        let index = ctx.open_document(crate::DocumentSession::new(
            larrygram_model::Document::new(),
            None,
        ));
        let session = ctx.documents.get(index).expect("session").id;
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut stack = UndoStack::new(UndoStack::DEFAULT_CAPACITY);
        stack.push(Box::new(NestedEdit {
            session,
            gate_was_closed: seen.clone(),
        }));

        stack.undo(&mut ctx).expect("undo");
        assert_eq!(&seen.lock().expect("seen")[..], [true], "gate closed");
        assert_eq!(ctx.warnings.len(), 1, "one dropped-edit warning");
        stack.redo(&mut ctx).expect("redo");
        assert_eq!(&seen.lock().expect("seen")[..], [true, true]);
        assert_eq!(ctx.warnings.len(), 2);
        let entry = ctx.documents.get(index).expect("session");
        assert!(
            entry.undo.is_empty(),
            "the nested edit never landed on the session's stack"
        );
    }

    // --- ADR 0040, amendment 2026-11-03: undo groups -----------------

    /// One step that appends `mark` to `log` (apply) or removes it
    /// again (revert), so order and direction are observable.
    struct MarkEdit {
        mark: char,
        log: std::sync::Arc<std::sync::Mutex<String>>,
    }

    impl Edit for MarkEdit {
        fn label(&self) -> &str {
            "mark"
        }

        fn apply(&mut self, _ctx: &mut AppCtx) -> Result<(), CommandError> {
            if let Ok(mut log) = self.log.lock() {
                log.push(self.mark);
            }
            Ok(())
        }

        fn revert(&mut self, _ctx: &mut AppCtx) -> Result<(), CommandError> {
            if let Ok(mut log) = self.log.lock() {
                assert_eq!(log.pop(), Some(self.mark), "reverts run in reverse");
            }
            Ok(())
        }
    }

    /// The two test edits carry their own labels (the undo menu seam
    /// `Edit::label` provides).
    #[test]
    fn the_test_edits_label_themselves() {
        let log = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        assert_eq!(Edit::label(&MarkEdit { mark: 'a', log }), "mark");
        assert_eq!(Edit::label(&AddPageEdit), "add page");
    }

    /// One step that adds a page (apply) or removes it again (revert):
    /// the document restoration `cancel_group` promises is observable.
    struct AddPageEdit;

    impl Edit for AddPageEdit {
        fn label(&self) -> &str {
            "add page"
        }

        fn apply(&mut self, ctx: &mut AppCtx) -> Result<(), CommandError> {
            let session = ctx
                .active_document_mut()
                .ok_or_else(|| CommandError::failed("no open document"))?;
            session
                .document
                .add_page("added")
                .map_err(|_| CommandError::failed("add page failed"))?;
            Ok(())
        }

        fn revert(&mut self, ctx: &mut AppCtx) -> Result<(), CommandError> {
            let session = ctx
                .active_document_mut()
                .ok_or_else(|| CommandError::failed("no open document"))?;
            session.document.pages.pop();
            Ok(())
        }
    }

    /// A session over a document with no pages.
    fn app_with_session() -> (AppCtx, crate::SessionId) {
        let mut ctx = AppCtx::new(Settings::default());
        let index = ctx.open_document(crate::DocumentSession::new(
            larrygram_model::Document::new(),
            None,
        ));
        let session = ctx.documents.get(index).expect("session").id;
        (ctx, session)
    }

    /// An empty group pushes nothing and leaves a redo tail alone.
    #[test]
    fn an_empty_group_pushes_nothing() {
        let mut stack = UndoStack::new(UndoStack::DEFAULT_CAPACITY);
        let mut ctx = AppCtx::new(Settings::default());
        stack.push(Box::new(NamedEdit("one")));
        stack.undo(&mut ctx).expect("undo");
        stack.begin_group("group", false).expect("begin");
        assert!(stack.has_group());
        stack.end_group();
        assert!(!stack.has_group());
        assert_eq!(stack.len(), 1, "no entry was pushed");
        assert!(stack.can_redo(), "the redo tail survived");
    }

    /// Task P7-48 item 5: the pending step's label and Redo's state.
    #[test]
    fn the_pending_step_names_itself_and_disables_redo() {
        let mut stack = UndoStack::new(UndoStack::DEFAULT_CAPACITY);
        let mut ctx = AppCtx::new(Settings::default());
        stack.push(Box::new(NamedEdit("one")));
        stack.undo(&mut ctx).expect("undo");
        assert!(stack.can_redo(), "a redo tail without a group");
        assert_eq!(stack.pending_label(), None);

        stack.begin_group("Insert Text Box", false).expect("begin");
        assert_eq!(stack.pending_label(), None, "an empty group has no step");
        assert!(stack.can_redo(), "an empty group keeps Redo");
        stack.push(Box::new(NamedEdit("in group")));
        assert_eq!(stack.pending_label(), Some("Insert Text Box"));
        assert!(
            !stack.can_redo(),
            "Redo is disabled while the group is open"
        );

        stack.end_group();
        assert_eq!(stack.pending_label(), None);
        assert!(!stack.can_redo(), "the fold dropped the redo tail");
    }

    /// A non-empty open group is undoable and an empty one is not
    /// (task P7-41 item 5).
    #[test]
    fn can_undo_counts_a_non_empty_open_group() {
        let mut stack = UndoStack::new(UndoStack::DEFAULT_CAPACITY);
        let mut ctx = AppCtx::new(Settings::default());
        stack.begin_group("Insert Text Box", false).expect("begin");
        assert!(!stack.can_undo(), "an empty group is nothing to undo");
        stack.push(Box::new(NamedEdit("in group")));
        assert!(stack.can_undo(), "a non-empty group is one step away");
        assert_eq!(
            stack.undo(&mut ctx).expect("undo").as_deref(),
            Some("Insert Text Box"),
            "the undo commits the group and undoes it whole"
        );
        assert!(!stack.can_undo(), "the folded step is gone again");
    }

    /// A group folds every push into one step: undo reverts the steps
    /// in reverse, redo applies them in order.
    #[test]
    fn a_group_folds_its_pushes_into_one_step() {
        let log = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let mut stack = UndoStack::new(UndoStack::DEFAULT_CAPACITY);
        let mut ctx = AppCtx::new(Settings::default());
        stack.begin_group("Insert Text Box", false).expect("begin");
        for mark in ['a', 'b'] {
            let mut edit = MarkEdit {
                mark,
                log: log.clone(),
            };
            edit.apply(&mut ctx).expect("apply");
            stack.push(Box::new(edit));
        }
        assert_eq!(stack.len(), 0, "the pushes collect in the group");
        stack.end_group();
        assert_eq!(stack.len(), 1, "one folded step");
        assert_eq!(stack.undo_labels(), vec!["Insert Text Box"]);

        stack.undo(&mut ctx).expect("undo");
        assert_eq!(&*log.lock().expect("log"), "", "both steps reverted");
        stack.redo(&mut ctx).expect("redo");
        assert_eq!(&*log.lock().expect("log"), "ab", "both steps re-applied");
    }

    /// `cancel_group` reverts the collected steps in reverse, restores
    /// the document and answers the unsaved-changes state the group
    /// saw when it opened (ADR 0040, amendment 2026-11-03).
    #[test]
    fn cancel_group_reverts_the_steps_and_restores_the_document() {
        let (mut ctx, _session) = app_with_session();
        let mut stack = UndoStack::new(UndoStack::DEFAULT_CAPACITY);
        stack.begin_group("Insert Text Box", true).expect("begin");
        let mut edit = AddPageEdit;
        edit.apply(&mut ctx).expect("apply");
        stack.push(Box::new(edit));
        assert_eq!(
            ctx.active_document().expect("session").document.pages.len(),
            1
        );

        let unsaved = stack.cancel_group(&mut ctx).expect("cancel");
        assert_eq!(unsaved, Some(true), "the group's unsaved state returns");
        assert!(!stack.has_group());
        assert!(stack.is_empty(), "nothing is left on the stack");
        assert_eq!(
            ctx.active_document().expect("session").document.pages.len(),
            0,
            "the document is as it was when the group opened"
        );
    }

    /// Cancelling without an open group answers `None` and changes
    /// nothing.
    #[test]
    fn cancel_without_a_group_is_a_no_op() {
        let (mut ctx, _session) = app_with_session();
        let mut stack = UndoStack::new(UndoStack::DEFAULT_CAPACITY);
        assert_eq!(stack.cancel_group(&mut ctx).expect("cancel"), None);
    }

    /// An edit whose first `revert` fails and whose later ones behave
    /// like [`MarkEdit`] (task P7-41 item 6's rollback completion).
    struct FailsOnceEdit {
        mark: char,
        log: std::sync::Arc<std::sync::Mutex<String>>,
        failed: std::sync::Arc<std::sync::Mutex<bool>>,
    }

    impl Edit for FailsOnceEdit {
        fn label(&self) -> &str {
            "fails once"
        }

        fn apply(&mut self, _ctx: &mut AppCtx) -> Result<(), CommandError> {
            if let Ok(mut log) = self.log.lock() {
                log.push(self.mark);
            }
            Ok(())
        }

        fn revert(&mut self, _ctx: &mut AppCtx) -> Result<(), CommandError> {
            if let Ok(mut failed) = self.failed.lock()
                && !*failed
            {
                *failed = true;
                return Err(CommandError::failed("revert failed"));
            }
            if let Ok(mut log) = self.log.lock() {
                assert_eq!(log.pop(), Some(self.mark), "reverts run in reverse");
            }
            Ok(())
        }
    }

    /// A failing `revert` pushes the edits that were not reverted as
    /// one step labelled as the group and returns the error (task
    /// P7-41 item 6): the stack stays consistent and one undo finishes
    /// the rollback.
    #[test]
    fn a_failed_cancel_keeps_the_unreverted_edits_as_one_step() {
        let log = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let (mut ctx, _session) = app_with_session();
        let mut stack = UndoStack::new(UndoStack::DEFAULT_CAPACITY);
        stack.begin_group("Insert Text Box", false).expect("begin");
        let edits: Vec<Box<dyn Edit>> = vec![
            Box::new(MarkEdit {
                mark: 'a',
                log: log.clone(),
            }),
            Box::new(FailsOnceEdit {
                mark: 'b',
                log: log.clone(),
                failed: std::sync::Arc::new(std::sync::Mutex::new(false)),
            }),
            Box::new(MarkEdit {
                mark: 'c',
                log: log.clone(),
            }),
        ];
        for mut edit in edits {
            edit.apply(&mut ctx).expect("apply");
            stack.push(edit);
        }
        assert_eq!(&*log.lock().expect("log"), "abc");

        let error = stack.cancel_group(&mut ctx).expect_err("the revert fails");
        assert_eq!(error.to_string(), "revert failed", "the error is reported");
        assert!(!stack.has_group(), "the group closed either way");
        assert_eq!(
            &*log.lock().expect("log"),
            "ab",
            "the reverts before the failure landed"
        );
        assert_eq!(
            stack.undo_labels(),
            vec!["Insert Text Box"],
            "the unreverted edits are one step labelled as the group"
        );

        // One undo finishes the rollback: 'b' and then 'a' revert.
        assert_eq!(
            stack.undo(&mut ctx).expect("undo").as_deref(),
            Some("Insert Text Box")
        );
        assert_eq!(&*log.lock().expect("log"), "", "every edit is reverted");
        assert!(!stack.can_undo(), "the stack is consistent");
    }

    /// The undo menu reads a step's own label through `Edit::label`:
    /// one `FailsOnceEdit` past its failure undoes under its own name.
    #[test]
    fn a_step_reports_its_label_through_the_undo() {
        let log = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let (mut ctx, _session) = app_with_session();
        let mut stack = UndoStack::new(UndoStack::DEFAULT_CAPACITY);
        let mut edit = FailsOnceEdit {
            mark: 'x',
            log: log.clone(),
            failed: std::sync::Arc::new(std::sync::Mutex::new(true)),
        };
        edit.apply(&mut ctx).expect("apply");
        stack.push(Box::new(edit));
        assert_eq!(
            stack.undo(&mut ctx).expect("undo").as_deref(),
            Some("fails once"),
            "the step names itself"
        );
        assert_eq!(&*log.lock().expect("log"), "", "the revert popped the mark");
    }

    /// A failure at the very first revert keeps every collected edit
    /// as the one group step (task P7-41 item 6).
    #[test]
    fn a_failed_cancel_at_the_first_revert_keeps_every_edit() {
        let log = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let (mut ctx, _session) = app_with_session();
        let mut stack = UndoStack::new(UndoStack::DEFAULT_CAPACITY);
        stack.begin_group("group", true).expect("begin");
        let mut first = MarkEdit {
            mark: 'a',
            log: log.clone(),
        };
        first.apply(&mut ctx).expect("apply");
        stack.push(Box::new(first));
        // `BadEdit` only fails its steps; the push is the "already
        // applied" seam (ADR 0028).
        stack.push(Box::new(BadEdit));
        let error = stack.cancel_group(&mut ctx).expect_err("the revert fails");
        assert_eq!(error.to_string(), "revert failed");
        assert_eq!(stack.undo_labels(), vec!["group"]);
        assert_eq!(&*log.lock().expect("log"), "a", "nothing reverted");
    }

    /// Undo and redo end an open group first: the collected edits are
    /// one step before the step runs.
    #[test]
    fn undo_and_redo_end_an_open_group_first() {
        let log = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let mut stack = UndoStack::new(UndoStack::DEFAULT_CAPACITY);
        let mut ctx = AppCtx::new(Settings::default());
        stack.begin_group("Insert Text Box", false).expect("begin");
        let mut edit = MarkEdit {
            mark: 'a',
            log: log.clone(),
        };
        edit.apply(&mut ctx).expect("apply");
        stack.push(Box::new(edit));

        let label = stack.undo(&mut ctx).expect("undo");
        assert_eq!(
            label.as_deref(),
            Some("Insert Text Box"),
            "the group's label"
        );
        assert!(!stack.has_group(), "the undo ended the group");
        assert_eq!(&*log.lock().expect("log"), "", "the step reverted");
        assert!(!stack.can_undo(), "the group was the only step");
    }

    /// Groups do not nest: a second `begin_group` is an error.
    #[test]
    fn groups_do_not_nest() {
        let mut stack = UndoStack::new(UndoStack::DEFAULT_CAPACITY);
        stack.begin_group("outer", false).expect("begin");
        let error = stack.begin_group("inner", false).expect_err("nested");
        assert!(
            error.to_string().contains("already open"),
            "the error names the problem: {error}"
        );
        assert!(stack.has_group(), "the outer group is untouched");
    }

    /// `clear` drops an open group with everything else.
    #[test]
    fn clear_drops_an_open_group() {
        let mut stack = UndoStack::new(UndoStack::DEFAULT_CAPACITY);
        stack.begin_group("group", false).expect("begin");
        stack.push(Box::new(NoopEdit));
        stack.clear();
        assert!(!stack.has_group());
        assert!(stack.is_empty());
    }
}
