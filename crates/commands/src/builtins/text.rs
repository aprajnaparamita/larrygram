//! `text.*` commands (P5-07).
//!
//! Shape text commands (ADR 0044):
//!
//! - `text.begin` — validate the selection's anchor and announce the
//!   in-place editor request for the UI. The editor state itself lives
//!   in `crates/ui` (L6), so this command never depends on egui.
//! - `text.commit` — end the edit. With a `text` argument it is the same
//!   write as `shape.set.text`; without one it only announces the end
//!   (the UI owns the buffer).
//! - `text.cancel` — end the edit without writing anything.
//! - `shape.set.text` — write the `text` argument as one run (ADR 0044).
//!
//! Multi-run text collapses to the first run: the command keeps the
//! `Character`/`Paragraph`/`Tabs` row markers at `IX` 0 (the first row)
//! when the shape has rows of that kind, then writes the literal text.
//! The old `TextContent` is captured verbatim, so undo restores the
//! shape's text exactly, including a shape that had no text at all.

use larrygram_model::{Document, Preserved, ShapeId, TextContent, TextPart};

use crate::app::{AppCtx, SessionId};
use crate::error::CommandError;
use crate::undo::Edit;
use crate::{Invocation, Outcome};

use super::edit::document_mut;
use super::{CommandTable, edit_outcome, model_error, no_document, outcome};

/// The implemented text commands (task P5-07).
pub(crate) const TABLE: CommandTable = &[
    ("text.begin", "Edit Text", can_edit_text, run_begin),
    ("text.commit", "Commit Text", can_edit_text, run_commit),
    ("text.cancel", "Cancel Text", can_edit_text, run_cancel),
    ("shape.set.text", "Shape Text", can_edit_text, run_set_text),
];

/// The selection's anchor shape, or the last selected shape when there
/// is no anchor (the same subject rule as the Size & Position pane).
fn target_shape(ctx: &AppCtx) -> Option<ShapeId> {
    ctx.selection
        .anchor
        .or_else(|| ctx.selection.shapes.last().copied())
}

/// The session and the editable anchor shape.
fn text_target(ctx: &AppCtx) -> Result<(SessionId, ShapeId), CommandError> {
    let shape =
        target_shape(ctx).ok_or_else(|| CommandError::failed("no selected shape to edit"))?;
    let session = ctx.active_document().ok_or_else(no_document)?;
    if session.document.shape(shape).is_none() || session.document.page_of(shape).is_none() {
        return Err(CommandError::failed(format!(
            "shape {} is no longer on a page",
            shape.index
        )));
    }
    Ok((session.id, shape))
}

/// Enabled when the selection names a live shape.
fn can_edit_text(ctx: &AppCtx) -> crate::Enabled {
    if ctx.active_document().is_none() {
        return crate::Enabled::no("no open document");
    }
    let Some(shape) = target_shape(ctx) else {
        return crate::Enabled::no("no selection");
    };
    match ctx
        .active_document()
        .and_then(|session| session.document.page_of(shape))
    {
        Some(_) => crate::Enabled::yes(),
        None => crate::Enabled::no("the selected shape is stale"),
    }
}

/// `text.begin`: validates the editor target. The UI's `TextEditor`
/// performs the actual buffer hand-off; a programmatic caller gets the
/// same validation and a status message.
fn run_begin(ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
    let (_, shape) = text_target(ctx)?;
    let visio_id = ctx
        .active_document()
        .and_then(|session| session.document.shape(shape))
        .map_or(0, |entry| entry.visio_id);
    Ok(outcome(
        format!("text editing begins on shape {visio_id}"),
        Vec::new(),
    ))
}

/// `text.commit`: with a `text` argument, commits through the same write
/// as `shape.set.text`; without one, the UI owns the buffer and only the
/// end of the edit is announced here.
fn run_commit(ctx: &mut AppCtx, inv: &Invocation) -> Result<Outcome, CommandError> {
    if inv.args.get("text").is_some() {
        return run_set_text(ctx, inv);
    }
    let (_, shape) = text_target(ctx)?;
    let visio_id = ctx
        .active_document()
        .and_then(|session| session.document.shape(shape))
        .map_or(0, |entry| entry.visio_id);
    Ok(outcome(
        format!("text edit committed on shape {visio_id}"),
        Vec::new(),
    ))
}

/// `text.cancel`: discards the edit (the UI drops its buffer).
fn run_cancel(ctx: &mut AppCtx, _inv: &Invocation) -> Result<Outcome, CommandError> {
    let (_, shape) = text_target(ctx)?;
    let visio_id = ctx
        .active_document()
        .and_then(|session| session.document.shape(shape))
        .map_or(0, |entry| entry.visio_id);
    Ok(outcome(
        format!("text edit cancelled on shape {visio_id}"),
        Vec::new(),
    ))
}

/// `shape.set.text`: writes the `text` argument as one run.
fn run_set_text(ctx: &mut AppCtx, inv: &Invocation) -> Result<Outcome, CommandError> {
    let text = match inv.args.get("text") {
        Some(crate::ArgValue::Text(text)) => text.clone(),
        None => {
            return Err(CommandError::NeedsInput(crate::NeededInput::Text {
                prompt: "shape text".to_owned(),
                default: None,
            }));
        }
        Some(_) => return Err(CommandError::invalid_args("text must be a string")),
    };
    let (session, shape) = text_target(ctx)?;
    let mut edit = {
        let doc = document_mut(ctx, session)?;
        TextEdit::new(doc, session, shape, one_run(doc, shape, &text))
    };
    edit.apply(ctx)?;
    Ok(edit_outcome("Wrote shape text", Vec::new(), Box::new(edit)))
}

/// Whether the shape has at least one undeleted row in `section`.
fn has_rows(doc: &Document, shape: ShapeId, section: &str) -> bool {
    doc.shape(shape).is_some_and(|entry| {
        entry.sheet.sections.iter().any(|candidate| {
            candidate.name.as_str() == section && candidate.rows.iter().any(|row| !row.is_deleted())
        })
    })
}

/// The one-run text of ADR 0044: the first `Character`, `Paragraph` and
/// `Tabs` row markers the shape has, then the literal buffer.
fn one_run(doc: &Document, shape: ShapeId, text: &str) -> TextContent {
    if text.is_empty() {
        return TextContent::new();
    }
    let mut parts = Vec::new();
    if has_rows(doc, shape, "Character") {
        parts.push(TextPart::CharRun { ix: 0 });
    }
    if has_rows(doc, shape, "Paragraph") {
        parts.push(TextPart::ParaRun { ix: 0 });
    }
    if has_rows(doc, shape, "Tabs") {
        parts.push(TextPart::TabsRun { ix: 0 });
    }
    parts.push(TextPart::Text(text.to_owned()));
    TextContent { parts }
}

/// The undoable step that writes `literal` as `shape`'s one-run text
/// (ADR 0044), for a command that writes text beside another edit
/// (`insert.drawing.text_box`'s `text` argument, task P7-25 item 9:
/// one text object is one undo step). The caller applies and pushes
/// the step.
pub(crate) fn text_edit_step(
    doc: &Document,
    session: SessionId,
    shape: ShapeId,
    literal: &str,
) -> Result<Box<dyn Edit>, CommandError> {
    Ok(Box::new(TextEdit::new(
        doc,
        session,
        shape,
        one_run(doc, shape, literal),
    )))
}

/// One undoable text replacement (ADR 0044): `apply` writes the one-run
/// text through `Document::set_shape_text`; `revert` restores the exact
/// prior `TextContent` and preserved element, including a shape that had
/// no text (`None`).
struct TextEdit {
    session: SessionId,
    label: &'static str,
    shape: ShapeId,
    old_text: Option<TextContent>,
    old_preserved: Preserved,
    new_text: TextContent,
}

impl TextEdit {
    /// Captures the shape's current text and preserved content.
    fn new(doc: &Document, session: SessionId, shape: ShapeId, new_text: TextContent) -> Self {
        let entry = doc.shape(shape);
        Self {
            session,
            label: "Write shape text",
            shape,
            old_text: entry.and_then(|shape| shape.text.clone()),
            old_preserved: entry
                .map(|shape| shape.preserved.clone())
                .unwrap_or_default(),
            new_text,
        }
    }

    /// The shape entry, or an error when the id went stale.
    fn entry_mut(
        ctx: &mut AppCtx,
        session: SessionId,
        shape: ShapeId,
    ) -> Result<&mut larrygram_model::Shape, CommandError> {
        let doc = document_mut(ctx, session)?;
        doc.shape_mut(shape)
            .ok_or_else(|| CommandError::failed(format!("unknown shape id {}", shape.index)))
    }
}

impl Edit for TextEdit {
    fn label(&self) -> &str {
        self.label
    }

    fn apply(&mut self, ctx: &mut AppCtx) -> Result<(), CommandError> {
        let new_text = self.new_text.clone();
        let doc = document_mut(ctx, self.session)?;
        doc.set_shape_text(self.shape, new_text)
            .map_err(model_error)?;
        ctx.touch_session(self.session);
        Ok(())
    }

    fn revert(&mut self, ctx: &mut AppCtx) -> Result<(), CommandError> {
        let old_text = self.old_text.clone();
        let old_preserved = self.old_preserved.clone();
        let entry = Self::entry_mut(ctx, self.session, self.shape)?;
        entry.text = old_text;
        entry.preserved = old_preserved;
        ctx.touch_session(self.session);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use larrygram_geom::Unit;
    use larrygram_model::{Cell, CellName, CellValue, PageId, Row, Section, SectionName, Shape};

    fn document() -> (Document, PageId, ShapeId) {
        let mut doc = Document::new();
        let page = doc.add_page("Page-1").expect("page");
        let mut shape = Shape::new(ShapeId { index: 0 }, 1);
        shape.sheet.cells.push(Cell {
            name: CellName::new("PinX"),
            formula: Some("1".to_owned()),
            value: Some(CellValue::Number(1.0)),
            unit: Unit::Inches,
            preserved: Preserved::new(),
        });
        let shape = doc.insert_shape(page, shape, None).expect("shape");
        (doc, page, shape)
    }

    fn add_rows(doc: &mut Document, shape: ShapeId, name: &str) {
        let entry = doc.shape_mut(shape).expect("shape");
        let mut section = Section::new(SectionName::new(name.to_owned()));
        section.rows.push(Row::new());
        entry.sheet.sections.push(section);
    }

    #[test]
    fn one_run_uses_the_first_row_markers_of_the_shape() {
        let (mut doc, _page, shape) = document();
        add_rows(&mut doc, shape, "Character");
        add_rows(&mut doc, shape, "Paragraph");
        add_rows(&mut doc, shape, "Tabs");
        let text = one_run(&doc, shape, "hello");
        assert_eq!(text.parts.first(), Some(&TextPart::CharRun { ix: 0 }));
        assert_eq!(text.parts.get(1), Some(&TextPart::ParaRun { ix: 0 }));
        assert_eq!(text.parts.get(2), Some(&TextPart::TabsRun { ix: 0 }));
        assert_eq!(text.parts.get(3), Some(&TextPart::Text("hello".to_owned())));
    }

    #[test]
    fn one_run_omits_missing_sections_and_empty_text_is_empty() {
        let (mut doc, _page, shape) = document();
        add_rows(&mut doc, shape, "Character");
        let text = one_run(&doc, shape, "x");
        assert_eq!(
            text.parts,
            vec![TextPart::CharRun { ix: 0 }, TextPart::Text("x".to_owned()),]
        );
        assert!(one_run(&doc, shape, "").is_empty());
    }

    #[test]
    fn one_run_ignores_deleted_rows() {
        let (mut doc, _page, shape) = document();
        add_rows(&mut doc, shape, "Character");
        let entry = doc.shape_mut(shape).expect("shape");
        if let Some(section) = entry.sheet.sections.last_mut()
            && let Some(row) = section.rows.first_mut()
        {
            row.preserved
                .attributes
                .push(larrygram_model::XmlAttribute {
                    name: larrygram_model::QName::local("Del"),
                    value: "1".to_owned(),
                });
        }
        assert!(!has_rows(&doc, shape, "Character"));
        assert_eq!(
            one_run(&doc, shape, "x").parts,
            vec![TextPart::Text("x".to_owned())]
        );
    }

    #[test]
    fn a_stale_target_is_an_error_in_the_handler() {
        use crate::app::{AppCtx, DocumentSession, Settings};
        use crate::{ArgValue, Args, Invocation, Source};

        let (doc, _page, _shape) = document();
        let mut ctx = AppCtx::new(Settings::default());
        ctx.open_document(DocumentSession::new(doc, None));
        let stale = ShapeId { index: 99 };
        ctx.selection.shapes = vec![stale];
        ctx.selection.anchor = Some(stale);
        let invocation = Invocation::new(
            Args::new().with("text", ArgValue::Text("x".to_owned())),
            Source::Test,
        );
        let error = run_set_text(&mut ctx, &invocation).expect_err("stale shape");
        assert!(error.to_string().contains("no longer on a page"), "{error}");
    }
}
