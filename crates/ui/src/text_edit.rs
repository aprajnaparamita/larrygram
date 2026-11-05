//! In-place text editing on shapes (ADR 0044).
//!
//! The editor is an egui text field over the shape's text block (the
//! canvas places it with [`editor_rect`]). On commit the buffer is
//! written as one text run ([`TextContent`], ADR 0019): run and
//! paragraph markers collapse to the first row's `IX`, which is the
//! documented Phase 5 limit until a run-aware editor lands (ADR 0044).
//! While editing, the shortcut scope is `Text` (ADR 0036). Task P5-07
//! implements the bodies.
//!
//! The editor state is deliberately independent of the command registry:
//! `crates/commands` (L4) cannot depend on egui (L6). The UI tool opens
//! the editor, and on commit dispatches `shape.set.text`; the
//! `text.begin`/`text.commit`/`text.cancel` commands validate and
//! announce the same transition for headless callers.

use egui::text::{CCursor, CCursorRange};
use egui::{Id, Ui};
use larrygram_commands::AppCtx;
use larrygram_geom::{Point, Rect as GeoRect, Transform};
use larrygram_model::{ShapeId, TextContent, TextPart};
use larrygram_render::scene::{Node, Shape as SceneShape};
use larrygram_render::{Scene, node_bounds};

use crate::UiError;

/// One run of the edited text (task P7-53): the buffer range and the
/// `Character` row `IX` that formats it. `None` names the run at row
/// position 0 — a text written without run markers reads it there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RunSpan {
    /// The run's first character of the buffer.
    start: usize,
    /// One past the run's last character.
    end: usize,
    /// The `IX` of the `Character` row that formats the run.
    ix: Option<i32>,
}

/// The drawn format of one edited run (task P7-53): what the editor
/// field shows for it while the text is being edited.
#[derive(Debug, Clone, PartialEq)]
pub struct RunStyle {
    /// The run's first character of the buffer.
    pub start: usize,
    /// One past the run's last character.
    pub end: usize,
    /// The face's family name (`Carlito`, ...).
    pub family: String,
    /// Bold.
    pub bold: bool,
    /// Italic.
    pub italic: bool,
    /// The size in points.
    pub size_pt: f32,
    /// The colour.
    pub color: egui::Color32,
    /// Underlined.
    pub underline: bool,
    /// Struck through.
    pub strikethrough: bool,
}

/// The run rows one range format writes (task P7-53): the plan the
/// format command's `runs` and `runs_keep` arguments spell, each entry
/// a `(target IX, seed IX)` pair (`None` seeds from the run at row
/// position 0).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunPlan {
    /// The rows the write lands on.
    pub write: Vec<(i32, Option<i32>)>,
    /// The rows the split run's other parts use (created and seeded,
    /// never written).
    pub keep: Vec<(i32, Option<i32>)>,
}

/// The first-row markers a buffer keeps when it is written back ADR 0044.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct RunMarkers {
    /// The text had a `Character` run marker.
    character: bool,
    /// The text had a `Paragraph` run marker.
    paragraph: bool,
    /// The text had a `Tabs` run marker.
    tabs: bool,
}

/// The in-place text editor state (ADR 0044).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TextEditor {
    /// The shape being edited, or `None` while idle (task P7-37: a new
    /// text box is a shape too, so the box's id sits here while it is
    /// typed).
    pub shape: Option<ShapeId>,
    /// The page bounds of the new text box being typed (task P7-25
    /// item 9, task P7-37): the field fills the outlined box and a
    /// click inside it is a caret move. `None` for an existing shape's
    /// edit.
    pub pending: Option<GeoRect>,
    /// The editable buffer (plain text, ADR 0044).
    pub buffer: String,
    /// The buffer as the edit started (unchanged-buffer detection).
    original: String,
    /// The run marker kinds of the original text (collapse rule).
    markers: RunMarkers,
    /// The run structure of the original text (task P7-53): a range
    /// format changes it, and the commit writes the text even when the
    /// buffer itself is unchanged.
    original_runs: Vec<Option<i32>>,
    /// The runs of the edited text over `synced` (task P7-53).
    spans: Vec<RunSpan>,
    /// The buffer the spans map over; typing remaps them (task P7-53).
    synced: String,
    /// The field's selected character range of this frame (task P7-53):
    /// `None` when the caret is a bare caret (a format applies to the
    /// whole text then).
    selection: Option<(usize, usize)>,
    /// The drawn format of every run (task P7-53): the field shows it.
    styles: Vec<RunStyle>,
    /// Whether the text field still needs focus on its first frame.
    focus_requested: bool,
    /// Whether the field re-requests the keyboard focus on its next
    /// frame with the caret where it is (task P7-41 item 1): a
    /// text-format control click keeps the edit open and the field
    /// takes the keys back the same frame.
    refocus_requested: bool,
    /// Where the edit drew last (task P7-51 item 3): the block and
    /// field screen rects of the previous [`TextEditor::show`], the
    /// shell's Text-tool keep-the-edit area.
    shown: Option<ShownArea>,
}

/// Where the edit drew (task P7-51 item 3): the text block's screen
/// rect and the field's own.
#[derive(Debug, Clone, Copy, PartialEq)]
struct ShownArea {
    /// The text block's screen rect (the overlay's box).
    block: egui::Rect,
    /// The field's screen rect.
    field: egui::Rect,
}

impl TextEditor {
    /// An idle editor.
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether an edit is in progress (the `Text` shortcut scope, ADR 0036).
    pub fn is_active(&self) -> bool {
        self.shape.is_some() || self.pending.is_some()
    }

    /// Opens the editor on `shape` with its current text (ADR 0044).
    ///
    /// The UI reads the shape's text (and rejects a stale id) before it
    /// calls this method; `crates/ui`'s tool owns that read. The editor
    /// keeps the plain buffer and the first-row marker kinds, so a commit
    /// can collapse a multi-run text predictably.
    pub fn begin(&mut self, shape: ShapeId, text: &TextContent) -> Result<(), UiError> {
        self.shape = Some(shape);
        self.pending = None;
        self.buffer = plain_text(text);
        self.original = self.buffer.clone();
        self.markers = RunMarkers {
            character: text
                .parts
                .iter()
                .any(|part| matches!(part, TextPart::CharRun { .. })),
            paragraph: text
                .parts
                .iter()
                .any(|part| matches!(part, TextPart::ParaRun { .. })),
            tabs: text
                .parts
                .iter()
                .any(|part| matches!(part, TextPart::TabsRun { .. })),
        };
        self.spans = spans_of(text);
        self.original_runs = self.spans.iter().map(|span| span.ix).collect();
        self.synced = self.buffer.clone();
        self.selection = None;
        self.focus_requested = true;
        Ok(())
    }

    /// Opens the editor on a new text box at `rect` (its page bounds;
    /// task P7-37 item 1).
    ///
    /// The box already exists in the model (the shell inserts it when
    /// the editor opens, inside its undo group), so the editor has a
    /// real shape: `shape` is set and the text formatting predicates
    /// and format commands work as on any edited shape. The bounds keep
    /// the box-filling field and the click backstop of task P7-26
    /// item 3.
    pub fn begin_box(&mut self, shape: ShapeId, rect: GeoRect) {
        // `begin` cannot fail; it resets the buffer to the empty text
        // a new box starts with.
        self.begin(shape, &TextContent::default()).ok();
        self.pending = Some(rect);
    }

    /// Finishes the edit and returns the text to write (one run, ADR
    /// 0044, or the run structure the range formats built — task
    /// P7-53: the commit keeps it), or `None` when the buffer is
    /// unchanged.
    ///
    /// The editor always returns to idle, including on the unchanged and
    /// empty-buffer cases.
    pub fn finish(&mut self) -> Result<Option<TextContent>, UiError> {
        if !self.is_active() {
            return Err(UiError::Command(larrygram_commands::CommandError::failed(
                "no text edit is active",
            )));
        }
        self.sync_spans();
        let runs: Vec<Option<i32>> = self.spans.iter().map(|span| span.ix).collect();
        let changed = self.buffer != self.original || runs != self.original_runs;
        let text = self.run_text();
        *self = Self::new();
        if changed { Ok(Some(text)) } else { Ok(None) }
    }

    /// The buffer's run structure (task P7-53): one unmarked run keeps
    /// the one-run rule (ADR 0044), a structured text carries a
    /// `Character` run marker per formatted run. The paragraph and tabs
    /// markers do not appear here: the structured write re-adds the
    /// first-row ones (`shape.set.text`).
    fn run_text(&self) -> TextContent {
        if self.spans.len() <= 1 && self.spans.first().is_none_or(|span| span.ix.is_none()) {
            return one_run(&self.buffer, self.markers);
        }
        let chars: Vec<char> = self.buffer.chars().collect();
        let mut parts = Vec::new();
        let mut last_ix: Option<Option<i32>> = None;
        for span in &self.spans {
            if span.start >= span.end || span.end > chars.len() {
                continue;
            }
            let literal: String = chars[span.start..span.end].iter().collect();
            // Adjacent runs of the same row are one run.
            match parts.last_mut() {
                Some(TextPart::Text(previous)) if last_ix == Some(span.ix) => {
                    previous.push_str(&literal);
                    continue;
                }
                _ => {}
            }
            if let Some(ix) = span.ix {
                parts.push(TextPart::CharRun {
                    ix: u32::try_from(ix).unwrap_or(0),
                });
            }
            last_ix = Some(span.ix);
            parts.push(TextPart::Text(literal));
        }
        if parts.is_empty() {
            return TextContent::new();
        }
        TextContent { parts }
    }

    /// Remaps the runs over the buffer as typed (task P7-53): the edit
    /// region joins the run at its start, the runs beyond it shift.
    fn sync_spans(&mut self) {
        if self.synced == self.buffer {
            return;
        }
        let old: Vec<char> = self.synced.chars().collect();
        let new: Vec<char> = self.buffer.chars().collect();
        let mut prefix = 0;
        while prefix < old.len() && prefix < new.len() && old[prefix] == new[prefix] {
            prefix += 1;
        }
        let mut suffix = 0;
        while prefix + suffix < old.len()
            && prefix + suffix < new.len()
            && old[old.len() - 1 - suffix] == new[new.len() - 1 - suffix]
        {
            suffix += 1;
        }
        // The edit replaced the old middle `prefix..old.len() - suffix`
        // with the new middle `prefix..new.len() - suffix`.
        let old_tail = old.len() - suffix;
        let new_tail = new.len() - suffix;
        let delta = new.len() as isize - old.len() as isize;
        let shift = |at: usize| ((at as isize).saturating_add(delta).max(0)) as usize;
        // The run the typed text joins: the run at the edit's start.
        let joined = self
            .spans
            .iter()
            .find(|span| span.start <= prefix && prefix < span.end)
            .or_else(|| self.spans.iter().rev().find(|span| span.start <= prefix))
            .map(|span| span.ix)
            .unwrap_or(None);
        let mut out: Vec<RunSpan> = Vec::new();
        for span in &self.spans {
            let before = span.end.min(prefix);
            if span.start < before {
                out.push(RunSpan {
                    start: span.start,
                    end: before,
                    ix: span.ix,
                });
            }
            let after = span.start.max(old_tail);
            if after < span.end {
                out.push(RunSpan {
                    start: shift(after),
                    end: shift(span.end),
                    ix: span.ix,
                });
            }
        }
        if prefix < new_tail {
            out.push(RunSpan {
                start: prefix,
                end: new_tail,
                ix: joined,
            });
        }
        out.sort_by_key(|span| span.start);
        self.spans = out;
        self.synced = self.buffer.clone();
    }

    /// Splits the runs at the selected range (task P7-53): the range's
    /// parts get their own `Character` rows (the format writes them),
    /// the parts outside it keep their runs (the rows a split run's
    /// other parts use come back in [`RunPlan::keep`]). `None` when
    /// the range is empty — a format applies to the whole text then.
    pub fn split_for_range(
        &mut self,
        document: &larrygram_model::Document,
        shape: ShapeId,
        range: (usize, usize),
    ) -> Option<RunPlan> {
        self.sync_spans();
        let length = self.buffer.chars().count();
        let start = range.0.min(range.1).min(length);
        let end = range.0.max(range.1).min(length);
        if start >= end {
            return None;
        }
        // The pieces every run breaks into, the range's covered or not.
        #[derive(Clone, Copy)]
        struct Piece {
            start: usize,
            end: usize,
            ix: Option<i32>,
            covered: bool,
        }
        let mut pieces: Vec<Piece> = Vec::new();
        for span in &self.spans {
            if span.end <= start || span.start >= end {
                pieces.push(Piece {
                    start: span.start,
                    end: span.end,
                    ix: span.ix,
                    covered: false,
                });
                continue;
            }
            if span.start < start {
                pieces.push(Piece {
                    start: span.start,
                    end: start,
                    ix: span.ix,
                    covered: false,
                });
            }
            pieces.push(Piece {
                start: span.start.max(start),
                end: span.end.min(end),
                ix: span.ix,
                covered: true,
            });
            if span.end > end {
                pieces.push(Piece {
                    start: end,
                    end: span.end,
                    ix: span.ix,
                    covered: false,
                });
            }
        }
        let used_outside = |ix: Option<i32>| {
            ix.is_some() && pieces.iter().any(|piece| !piece.covered && piece.ix == ix)
        };
        let mut plan = RunPlan::default();
        let mut next = fresh_ix(document, shape, &self.spans);
        let mut base: Option<i32> = None;
        let mut spans: Vec<RunSpan> = Vec::new();
        for piece in pieces.iter().copied() {
            if !piece.covered {
                match piece.ix {
                    // An unmarked run keeps reading row position 0 up
                    // front; behind another run's marker it needs its
                    // own row (the same values, seeded).
                    None => {
                        let base = *base.get_or_insert_with(|| {
                            let ix = next;
                            next = next.saturating_add(1).max(0);
                            ix
                        });
                        if !plan.keep.iter().any(|(target, _)| *target == base) {
                            plan.keep.push((base, None));
                        }
                        spans.push(RunSpan {
                            start: piece.start,
                            end: piece.end,
                            ix: Some(base),
                        });
                    }
                    Some(ix) => spans.push(RunSpan {
                        start: piece.start,
                        end: piece.end,
                        ix: Some(ix),
                    }),
                }
            } else if piece.ix.is_some() && !used_outside(piece.ix) {
                // The row is the range's alone: the write lands on it.
                let ix = piece.ix.unwrap_or(0);
                if !plan.write.iter().any(|(target, _)| *target == ix) {
                    plan.write.push((ix, None));
                }
                spans.push(RunSpan {
                    start: piece.start,
                    end: piece.end,
                    ix: Some(ix),
                });
            } else {
                // A shared row (or an unmarked run): the range's part
                // gets its own row, seeded from the run it splits.
                let fresh = next;
                next = next.saturating_add(1).max(0);
                plan.write.push((fresh, piece.ix));
                spans.push(RunSpan {
                    start: piece.start,
                    end: piece.end,
                    ix: Some(fresh),
                });
            }
        }
        spans.sort_by_key(|span| span.start);
        self.spans = spans;
        Some(plan)
    }

    /// Abandons the edit without writing anything.
    pub fn cancel(&mut self) {
        *self = Self::new();
    }

    /// The field's selected character range of the last frame (task
    /// P7-53): `None` at a bare caret, where a format applies to the
    /// whole text.
    pub fn selected_range(&self) -> Option<(usize, usize)> {
        self.selection
    }

    /// The runs of the edited text, `(start, end, row IX)` per run
    /// (task P7-53): the caller reads their drawn format.
    pub fn runs(&self) -> Vec<(usize, usize, Option<i32>)> {
        self.spans
            .iter()
            .map(|span| (span.start, span.end, span.ix))
            .collect()
    }

    /// Sets the drawn format of every run (task P7-53): the field
    /// shows it while the text is being edited.
    pub fn set_styles(&mut self, styles: Vec<RunStyle>) {
        self.styles = styles;
    }

    /// Re-requests the keyboard focus with the caret where it is (task
    /// P7-41 item 1): after a text-format control click the field
    /// takes the keys back without losing the typed text or the
    /// caret's place.
    pub fn refocus(&mut self) {
        self.refocus_requested = true;
    }

    /// Whether a press at `screen` lands in the edited text block's
    /// keep-the-edit area (the block or the field's own rect) (task
    /// P7-51 item 3): the edit stays and the caret moves to the
    /// nearest position, so the shell's Text-tool rule drops the press
    /// instead of committing. Reads where the edit drew last frame.
    pub fn press_keeps_edit(&self, screen: egui::Pos2) -> bool {
        self.shown
            .is_some_and(|shown| shown.block.contains(screen) || shown.field.contains(screen))
    }

    /// The shape's text as the editor started with it (unchanged-buffer
    /// detection): the effective text (task P7-40), so a master's text
    /// on an instance with none of its own is the start state. A stale
    /// shape or a missing document is an error.
    pub fn original_text(&self, ctx: &AppCtx) -> Result<Option<TextContent>, UiError> {
        let Some(shape) = self.shape else {
            return Ok(None);
        };
        let Some(session) = ctx.active_document() else {
            return Err(UiError::Command(larrygram_commands::CommandError::failed(
                "no open document",
            )));
        };
        session.document.shape(shape).ok_or_else(|| {
            UiError::Command(larrygram_commands::CommandError::failed(format!(
                "unknown shape id {}",
                shape.index
            )))
        })?;
        Ok(session.document.effective_text(shape).cloned())
    }

    /// Draws the editor over the shape's drawn text (task P7-41 item
    /// 9) and reports a commit.
    ///
    /// The overlay box, the field's font size and its first baseline
    /// match the drawn text ([`EditorOverlay`]): the fill covers the
    /// drawn glyphs and the typed text sits where they sat. Escape and
    /// a click outside the block commit, matching the common Visio
    /// behaviour (ADR 0044); the `text.cancel` command is the explicit
    /// discard path. The first frame requests keyboard focus so egui's
    /// `Text` shortcut scope becomes active (ADR 0036).
    pub fn show(&mut self, ui: &mut Ui, overlay: EditorOverlay) -> EditorAction {
        if !self.is_active() {
            return EditorAction::Pending;
        }
        let rect = if finite_rect(overlay.rect) {
            overlay.rect
        } else {
            egui::Rect::from_min_size(ui.ctx().viewport_rect().min, egui::vec2(160.0, 40.0))
        };
        // The field sits where the drawn text sits (task P7-51 item
        // 1): the block inset by its text margins, aligned like the
        // first paragraph. A broken inner box falls back to the block.
        let field = if finite_rect(overlay.field) && overlay.field.width() > 0.0 {
            overlay.field
        } else {
            rect
        };
        // The field's font matches the drawn text (task P7-41 item 9)
        // and its text sits on the drawn first baseline: the galley
        // starts one measured ascent below it. The drawn family wins
        // when the theme registered it (the same face draws then).
        let font = overlay.font.as_ref().map(|drawn| {
            let named = egui::FontFamily::Name(drawn.family.as_str().into());
            let known = ui.fonts(|fonts| fonts.definitions().families.contains_key(&named));
            let family = if known {
                named
            } else {
                egui::FontFamily::Proportional
            };
            egui::FontId::new(drawn.size_pt, family)
        });
        let baseline_shift = match (overlay.font.as_ref(), &font) {
            (Some(drawn), Some(font)) => {
                let ascent = ui.fonts_mut(|fonts| {
                    fonts
                        .layout_no_wrap("X".to_owned(), font.clone(), egui::Color32::WHITE)
                        .rows
                        .first()
                        .and_then(|row| row.glyphs.first().map(|glyph| glyph.pos.y))
                        .unwrap_or(0.0)
                });
                drawn.first_baseline_y - rect.min.y - ascent
            }
            _ => 0.0,
        };
        let open_focus = std::mem::take(&mut self.focus_requested);
        let refocus = std::mem::take(&mut self.refocus_requested);
        // The caret goes to the end of the buffer when the editor
        // opens (task P7-29 item 2).
        let buffer_chars = self.buffer.chars().count();
        // The frame's press (the caret move of task P7-51 item 3 and
        // the commit rule below).
        let press = ui.input(|input| {
            input
                .pointer
                .any_pressed()
                .then(|| input.pointer.interact_pos())
                .flatten()
        });
        let mut commit = false;
        // The field's own rect joins the commit test (task P7-48 item
        // 1: a field that grows with typed lines never commits on a
        // press inside itself).
        let mut field_response = None;
        let field_width = field.width().max(1.0);
        egui::Area::new(editor_id(self.shape))
            .order(egui::Order::Foreground)
            .fixed_pos(egui::pos2(field.min.x, rect.min.y + baseline_shift))
            .show(ui.ctx(), |ui| {
                ui.set_min_width(field_width);
                // A new text box's field fills the outlined box
                // (task P7-26 item 3): a click anywhere inside the box
                // moves the caret and keeps editing. An existing
                // shape's edit keeps its one-row field; a press inside
                // the block but off the field is the same keep-the-edit
                // area (task P7-51 item 3), handled below.
                let rows = if self.pending.is_some() {
                    let font = egui::TextStyle::Body.resolve(ui.style());
                    let row = ui.fonts_mut(|fonts| fonts.row_height(&font));
                    ui.set_min_height(rect.height().max(row));
                    let row = row.max(1.0);
                    ((rect.height() / row).ceil() as usize).clamp(1, 64)
                } else {
                    1
                };
                let mut edit = egui::TextEdit::multiline(&mut self.buffer)
                    // The one outline of the edit is the text block's,
                    // painted below (task P7-25 item 9). The margin is
                    // zero (task P7-41 item 9): the inset lives in the
                    // field's box (task P7-51 item 1 — the block's
                    // text margins), and the wrap width is the inner
                    // width, so the field wraps where the drawn text
                    // wraps. The typed text aligns like the drawn
                    // text (the first paragraph's `HorzAlign`).
                    .frame(egui::Frame::NONE)
                    .margin(egui::Margin::ZERO)
                    .align(egui::Align2::new(overlay.align, egui::Align::TOP))
                    .desired_width(field_width)
                    .desired_rows(rows);
                if let Some(font) = font.clone() {
                    // The font size matches the drawn text (task
                    // P7-41 item 9).
                    edit = edit.font(font);
                }
                // The field shows each run's format (task P7-53): a
                // rich layout over the plain buffer.
                let styles = &self.styles;
                let base = font
                    .clone()
                    .unwrap_or_else(|| egui::FontId::new(12.0, egui::FontFamily::Proportional));
                let halign = overlay.align;
                let mut layouter =
                    move |ui: &Ui, buffer: &dyn egui::text_edit::TextBuffer, wrap_width: f32| {
                        let text = buffer.as_str();
                        let mut job = egui::text::LayoutJob::default();
                        job.wrap.max_width = wrap_width;
                        job.halign = halign;
                        // egui's own field layouter spells it this
                        // way (`default_layouter`): break on `\n`,
                        // keep the trailing whitespace and give every
                        // section the field's line height, so the
                        // field geometry is unchanged.
                        job.break_on_newline = true;
                        job.keep_trailing_whitespace = true;
                        let line_height = ui.fonts_mut(|fonts| fonts.row_height(&base))
                            + ui.spacing().extra_text_line_spacing;
                        for section in &mut job.sections {
                            section.format.line_height = Some(line_height);
                        }
                        // The runs name characters; slices need bytes.
                        let mut bounds = Vec::with_capacity(text.len().saturating_add(1));
                        for (byte, _) in text.char_indices() {
                            bounds.push(byte);
                        }
                        bounds.push(text.len());
                        let byte = |at: usize| bounds.get(at).copied().unwrap_or(text.len());
                        let mut cursor = 0;
                        for style in styles {
                            let start = byte(style.start);
                            let end = byte(style.end);
                            if start > cursor {
                                job.append(&text[cursor..start], 0.0, plain_format(ui, &base));
                            }
                            if end > start {
                                job.append(&text[start..end], 0.0, styled_format(ui, style));
                                cursor = end;
                            }
                        }
                        if cursor < text.len() {
                            job.append(&text[cursor..], 0.0, plain_format(ui, &base));
                        }
                        for section in &mut job.sections {
                            section.format.line_height = Some(line_height);
                        }
                        ui.fonts_mut(|fonts| fonts.layout_job(job))
                    };
                if !self.styles.is_empty() {
                    edit = edit.layouter(&mut layouter);
                }
                let mut output = edit.hint_text("Text").show(ui);
                // The frame's selected range (task P7-53): a format
                // applies to it, or to the whole text at a bare caret.
                let range = output.state.cursor.char_range().and_then(|range| {
                    let start = range.primary.index.0.min(range.secondary.index.0);
                    let end = range.primary.index.0.max(range.secondary.index.0);
                    (start < end).then_some((start, end))
                });
                if let Some(range) = range {
                    self.selection = Some(range);
                } else if output.response.has_focus() {
                    // The field's own bare caret: the range is gone.
                    self.selection = None;
                } else if let Some((start, end)) = self.selection {
                    // egui collapses a selection to its primary caret
                    // on the frames the field loses the keyboard (a
                    // format control click keeps the edit but takes
                    // the keys back, task P7-41): the range sticks
                    // here and the field re-shows it (task P7-53).
                    output.state.cursor.set_char_range(Some(CCursorRange {
                        primary: CCursor::new(end),
                        secondary: CCursor::new(start),
                        h_pos: None,
                    }));
                    output.state.clone().store(ui.ctx(), output.response.id);
                }
                // The editor's accessible name (task P7-24: the tests
                // and screen readers find the field by it).
                output.response.response = output.response.response.clone().accessible_name("Text");
                // A press inside the text block but off the field
                // (task P7-51 item 3: one keep-the-edit area) moves
                // the caret to the nearest position and keeps the
                // edit. A press on a popup over the block belongs to
                // the popup.
                let pressed_caret = match press {
                    Some(pos)
                        if rect.contains(pos)
                            && !output.response.rect.contains(pos)
                            && !other_layer_at(ui.ctx(), pos, self.shape) =>
                    {
                        Some(output.galley.cursor_from_pos(
                            pos - output.galley_pos + egui::vec2(output.galley.rect.left(), 0.0),
                        ))
                    }
                    _ => None,
                };
                if let Some(cursor) = pressed_caret {
                    // The caret moves to the press and the field takes
                    // the keyboard (task P7-51 item 3).
                    output
                        .state
                        .cursor
                        .set_char_range(Some(CCursorRange::one(cursor)));
                    output.state.store(ui.ctx(), output.response.id);
                    output.response.request_focus();
                    // egui surrenders the field's focus one frame
                    // after a press outside it (the click that moved
                    // the caret), so the field takes the keys back
                    // again on the next frame (the refocus of task
                    // P7-41 item 1).
                    self.refocus_requested = true;
                } else {
                    if open_focus {
                        output.response.request_focus();
                    } else if refocus {
                        hand_keyboard(ui.ctx(), output.response.id);
                    }
                    if open_focus {
                        // The caret sits at the end of the text when the
                        // editor opens (task P7-29 item 2): a stale cursor
                        // from an earlier edit of the same shape must not
                        // survive the re-open (egui keeps the field's
                        // state under its id). A refocus (task P7-41 item
                        // 1) leaves the caret alone.
                        output
                            .state
                            .cursor
                            .set_char_range(Some(CCursorRange::one(CCursor::new(buffer_chars))));
                        output.state.store(ui.ctx(), output.response.id);
                    }
                }
                // Escape commits (ADR 0044) — except when a popup
                // or menu is open (task P7-48 item 3): Escape closes
                // it and the edit stays.
                if ui.input(|input| input.key_pressed(egui::Key::Escape))
                    && !other_layer_open(ui.ctx(), self.shape)
                {
                    commit = true;
                }
                field_response = Some((output.response.rect, output.response.id));
            });
        // Where the edit drew this frame (task P7-51 item 3): the
        // shell's Text-tool rule reads it as its keep-the-edit area.
        self.shown = field_response.map(|(field, _)| ShownArea { block: rect, field });
        // The commit-on-outside-press rule (ADR 0044, task P7-41 item
        // 1, task P7-48 item 1): a press inside the text block or the
        // field's own rect keeps the edit; a press on text-format UI
        // keeps it too and hands the keyboard back (the caret
        // untouched); any other press outside commits.
        let commit_on_press = match press {
            Some(pos)
                if !rect.contains(pos)
                    && !field_response.is_some_and(|(field, _)| field.contains(pos)) =>
            {
                match press_rule(ui.ctx(), self.shape, pos) {
                    PressRule::Commit => true,
                    PressRule::KeepAndRefocus => {
                        if let Some((_, id)) = field_response {
                            hand_keyboard(ui.ctx(), id);
                        }
                        false
                    }
                    PressRule::Keep => false,
                }
            }
            _ => false,
        };
        // Exactly one outline while editing (task P7-25 item 9): the
        // text block's, in the selection colour; the tool overlay is
        // suppressed while the editor is open.
        let colors = crate::canvas::CanvasColors::active(ui.ctx());
        ui.painter()
            .rect_filled(rect, 0.0, ui.visuals().faint_bg_color);
        ui.painter().rect_stroke(
            rect,
            0.0,
            egui::Stroke::new(1.0, colors.selection),
            egui::StrokeKind::Inside,
        );
        if commit || commit_on_press {
            return match self.finish() {
                Ok(text) => EditorAction::Committed(text),
                // `finish` only fails when the editor is inactive, which
                // the `is_active` guard above excludes.
                Err(_) => EditorAction::Pending,
            };
        }
        EditorAction::Pending
    }
}

/// What one editor frame produced (ADR 0044).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum EditorAction {
    /// The edit is still in progress.
    Pending,
    /// The edit ended; the payload is the text to write, or `None` when
    /// the buffer did not change.
    Committed(Option<TextContent>),
}

/// The stable egui id of the editor area for one shape.
fn editor_id(shape: Option<ShapeId>) -> Id {
    Id::unique(("larrygram-text-editor", shape.map(|shape| shape.index)))
}

/// The key of the per-frame keep-editing rects (task P7-41 item 1).
fn keep_rects_id() -> Id {
    Id::unique("larrygram-keep-editing-rects")
}

/// One recorded keep-editing rect (task P7-41 item 1).
#[derive(Debug, Clone, Copy)]
struct KeepRect {
    /// The control's widget rect.
    rect: egui::Rect,
    /// Whether the editor takes the keyboard back after the press.
    editor_keeps_keyboard: bool,
}

/// The keep-editing rects recorded in one frame (task P7-41 item 1):
/// the pass number they were drawn in, so a stale list never silences
/// a press.
#[derive(Debug, Clone, Default)]
struct KeepRects {
    /// The pass the rects were drawn in.
    pass: u64,
    /// The widget rects of text-format controls.
    rects: Vec<KeepRect>,
}

/// Records a text-format control's widget rect (task P7-41 item 1): a
/// press on such a control keeps the in-place editor open. The
/// controls call this as they draw with the command they dispatch;
/// anything that is not a text-format command records nothing.
/// `editor_keeps_keyboard` says whether the editor takes the keyboard
/// back after the press: it does after a button or a list row, but not
/// after the classic font and size boxes, which are text fields of
/// their own. The list follows the frame and [`TextEditor::show`]
/// reads it the same frame (the controls draw before the canvas).
pub fn keep_editing_rect(
    ctx: &egui::Context,
    command: Option<&str>,
    rect: egui::Rect,
    editor_keeps_keyboard: bool,
) {
    // A text-format control's press keeps the edit (task P7-41 item
    // 1), and so does an Undo or Redo press (task P7-48 item 5): their
    // command commits the open edit — cancelling an untyped box,
    // undoing a typed one whole — when it runs.
    let keep = command.is_some_and(|command| {
        larrygram_commands::builtins::is_text_format(command)
            || matches!(command, "edit.undo" | "edit.redo" | "edit.redo.or.repeat")
    });
    if !keep {
        return;
    }
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|data| {
        let mut kept: KeepRects = data.get_temp(keep_rects_id()).unwrap_or_default();
        if kept.pass != pass {
            kept.rects.clear();
            kept.pass = pass;
        }
        kept.rects.push(KeepRect {
            rect,
            editor_keeps_keyboard,
        });
        data.insert_temp(keep_rects_id(), kept);
    });
}

/// Whether a popup, menu or palette layer other than the editor's own
/// is open (task P7-48 item 3): Escape belongs to that layer — it
/// closes and the open edit stays.
pub fn other_layer_open(ctx: &egui::Context, shape: Option<ShapeId>) -> bool {
    let own = editor_id(shape);
    ctx.memory(|memory| {
        memory
            .areas()
            .visible_layer_ids()
            .iter()
            .any(|layer| layer.order == egui::Order::Foreground && layer.id != own)
    })
}

/// Whether a press at `pos` lands on a foreground layer other than the
/// editor's own area (a popup, menu or palette drawn over the block):
/// the press belongs to that layer, not to the field.
fn other_layer_at(ctx: &egui::Context, pos: egui::Pos2, shape: Option<ShapeId>) -> bool {
    let own = editor_id(shape);
    ctx.layer_id_at(pos)
        .is_some_and(|layer| layer.order == egui::Order::Foreground && layer.id != own)
}

/// Hands the keyboard back to the editor's field `id` (task P7-41
/// item 1) — but only when nothing else holds it (task P7-48 item 8):
/// a popup's own field keeps its typing.
fn hand_keyboard(ctx: &egui::Context, id: egui::Id) {
    let taken = ctx
        .memory(|memory| memory.focused())
        .is_some_and(|focused| focused != id);
    if !taken {
        ctx.memory_mut(|memory| memory.request_focus(id));
    }
}

/// What a press at `pos` does to the open editor (task P7-41 item 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PressRule {
    /// The press is not on text-format UI: the edit commits (the P7-29
    /// rule).
    Commit,
    /// The press is on text-format UI that hands the keyboard back: the
    /// edit stays and the field re-takes the keyboard and the caret.
    KeepAndRefocus,
    /// The press is on text-format UI that takes the keyboard for its
    /// own text (the classic font and size boxes): the edit stays.
    Keep,
}

/// The press rule at `pos` (task P7-41 item 1): a press in a popup,
/// menu or palette layer (the format popups and dropdowns choose
/// values with the editor open) or on a recorded text-format control
/// ([`keep_editing_rect`]) keeps the edit. The editor's own area is
/// not one of them: a press on it but off the text block commits, like
/// any press outside.
fn press_rule(ctx: &egui::Context, shape: Option<ShapeId>, pos: egui::Pos2) -> PressRule {
    if other_layer_at(ctx, pos, shape) {
        return PressRule::KeepAndRefocus;
    }
    let pass = ctx.cumulative_pass_nr();
    let recorded = ctx.data(|data| {
        data.get_temp::<KeepRects>(keep_rects_id())
            .filter(|kept| kept.pass == pass)
            .and_then(|kept| {
                kept.rects
                    .iter()
                    .find(|kept| kept.rect.contains(pos))
                    .copied()
            })
    });
    match recorded {
        Some(kept) if kept.editor_keeps_keyboard => PressRule::KeepAndRefocus,
        Some(_) => PressRule::Keep,
        None => PressRule::Commit,
    }
}

/// The plain text of a [`TextContent`]: literal parts concatenated, field
/// markers dropped (their cached display text follows as a literal).
fn plain_text(text: &TextContent) -> String {
    let mut buffer = String::new();
    for part in &text.parts {
        if let TextPart::Text(literal) = part {
            buffer.push_str(literal);
        }
    }
    buffer
}

/// The field's plain format (task P7-53): the drawn font in the UI
/// text colour.
fn plain_format(ui: &Ui, base: &egui::FontId) -> egui::text::TextFormat {
    egui::text::TextFormat {
        font_id: base.clone(),
        color: ui.visuals().text_color(),
        ..Default::default()
    }
}

/// The field's format of one run (task P7-53): its face, size, colour
/// and decorations. The named face families live in [`crate::theme`];
/// a family the theme did not register falls back to the proportional
/// font, with a synthetic italic.
fn styled_format(ui: &Ui, style: &RunStyle) -> egui::text::TextFormat {
    let face = face_family_name(&style.family, style.bold, style.italic);
    let named = egui::FontFamily::Name(face.clone().into());
    let plain = egui::FontFamily::Name(base_family_name(&style.family).into());
    let known = ui.fonts(|fonts| fonts.definitions().families.contains_key(&named));
    let (family, skew) = if known {
        (named, false)
    } else {
        let known = ui.fonts(|fonts| fonts.definitions().families.contains_key(&plain));
        if known {
            (plain, style.italic)
        } else {
            (egui::FontFamily::Proportional, style.italic)
        }
    };
    egui::text::TextFormat {
        font_id: egui::FontId::new(style.size_pt, family),
        color: style.color,
        italics: skew,
        underline: underline_stroke(style.underline, style.color),
        strikethrough: underline_stroke(style.strikethrough, style.color),
        ..Default::default()
    }
}

/// The named family of one face (the [`crate::theme`] convention).
fn face_family_name(family: &str, bold: bool, italic: bool) -> String {
    let family = base_family_name(family);
    match (bold, italic) {
        (true, true) => format!("{family} Bold Italic"),
        (true, false) => format!("{family} Bold"),
        (false, true) => format!("{family} Italic"),
        (false, false) => family.to_owned(),
    }
}

/// The registered family of a drawn family (the aliases the font
/// stack resolves, ADR 0024; the empty family draws as Carlito, the
/// default stack's first face).
fn base_family_name(family: &str) -> &str {
    match family {
        "" | "Calibri" => "Carlito",
        "Segoe UI" => "Selawik",
        other => other,
    }
}

/// The underline stroke of a run (`Stroke::NONE` when not drawn).
fn underline_stroke(drawn: bool, color: egui::Color32) -> egui::Stroke {
    if drawn {
        egui::Stroke::new(1.0, color)
    } else {
        egui::Stroke::NONE
    }
}

/// The drawn format of the edited runs (task P7-53): each run's
/// `Character` row values, read the way the renderer reads them
/// (`run_style` in `crates/render`), for the editor field to show
/// while the text is being edited.
pub fn run_styles(
    document: &larrygram_model::Document,
    shape: ShapeId,
    runs: &[(usize, usize, Option<i32>)],
    scale: f32,
) -> Vec<RunStyle> {
    use larrygram_model::{CellName, CellReader, RowTarget, SheetRef};
    let sheet = SheetRef::Shape(shape);
    let fallback_family = document
        .themes
        .first()
        .and_then(|theme| theme.fonts.as_ref())
        .map(|fonts| fonts.minor_latin.clone())
        .unwrap_or_default();
    runs.iter()
        .map(|(start, end, ix)| {
            let row = match ix {
                Some(ix) => RowTarget::by_ix(*ix),
                None => RowTarget::at(0),
            };
            let mut reader = CellReader::new(document);
            let family = reader
                .row_text(sheet, "Character", row, &CellName::new("Font"))
                .unwrap_or_default();
            let family = if family.is_empty() {
                fallback_family.clone()
            } else {
                family
            };
            let size_in = reader
                .row_number(sheet, "Character", row, &CellName::new("Size"))
                .unwrap_or(0.0);
            let size_pt = if size_in > 0.0 { size_in * 72.0 } else { 12.0 };
            let style = reader
                .row_number(sheet, "Character", row, &CellName::new("Style"))
                .unwrap_or(0.0)
                .round() as i64;
            let underline = style & 4 != 0
                || reader
                    .row_boolean(sheet, "Character", row, &CellName::new("DblUnderline"))
                    .unwrap_or(false);
            let strikethrough = reader
                .row_boolean(sheet, "Character", row, &CellName::new("Strikethru"))
                .unwrap_or(false)
                || reader
                    .row_boolean(
                        sheet,
                        "Character",
                        row,
                        &CellName::new("DoubleStrikethrough"),
                    )
                    .unwrap_or(false);
            let color = reader
                .row_color(sheet, "Character", row, &CellName::new("Color"))
                .unwrap_or(larrygram_model::Color::new(0, 0, 0));
            let trans = reader
                .row_number(sheet, "Character", row, &CellName::new("ColorTrans"))
                .unwrap_or(0.0)
                .clamp(0.0, 1.0);
            let alpha = (255.0 * (1.0 - trans)).round().clamp(0.0, 255.0) as u8;
            RunStyle {
                start: *start,
                end: *end,
                family,
                bold: style & 1 != 0,
                italic: style & 2 != 0,
                size_pt: (size_pt * f64::from(scale)) as f32,
                color: egui::Color32::from_rgba_unmultiplied(color.r, color.g, color.b, alpha),
                underline,
                strikethrough,
            }
        })
        .collect()
}

/// The runs of a text (task P7-53): the literal parts over their
/// buffer ranges, each carrying the `Character` run marker that names
/// its row (`None` before the first marker).
fn spans_of(text: &TextContent) -> Vec<RunSpan> {
    let mut spans: Vec<RunSpan> = Vec::new();
    let mut offset = 0;
    let mut ix: Option<i32> = None;
    for part in &text.parts {
        match part {
            TextPart::Text(literal) => {
                let end = offset + literal.chars().count();
                match spans.last_mut() {
                    Some(last) if last.ix == ix && last.end == offset => last.end = end,
                    _ => spans.push(RunSpan {
                        start: offset,
                        end,
                        ix,
                    }),
                }
                offset = end;
            }
            TextPart::CharRun { ix: row } => ix = Some(i32::try_from(*row).unwrap_or(0)),
            _ => {}
        }
    }
    if spans.is_empty() {
        spans.push(RunSpan {
            start: 0,
            end: 0,
            ix: None,
        });
    }
    spans
}

/// The `IX` a new run row takes (task P7-53): one past every `IX` the
/// shape's `Character` rows and the editor's runs already claim.
fn fresh_ix(document: &larrygram_model::Document, shape: ShapeId, spans: &[RunSpan]) -> i32 {
    let mut largest = -1;
    if let Some(entry) = document.shape(shape) {
        for block in &entry.sheet.sections {
            if block.name.as_str() == "Character" {
                for row in &block.rows {
                    if let Some(ix) = row.ix {
                        largest = largest.max(ix);
                    }
                }
            }
        }
    }
    for span in spans {
        if let Some(ix) = span.ix {
            largest = largest.max(ix);
        }
    }
    largest.saturating_add(1).max(0)
}

/// The one-run content ADR 0044 writes: the first-row markers the
/// original text had, then the literal buffer. An empty buffer is an
/// empty text (no markers).
fn one_run(buffer: &str, markers: RunMarkers) -> TextContent {
    if buffer.is_empty() {
        return TextContent::new();
    }
    let mut parts = Vec::new();
    if markers.character {
        parts.push(TextPart::CharRun { ix: 0 });
    }
    if markers.paragraph {
        parts.push(TextPart::ParaRun { ix: 0 });
    }
    if markers.tabs {
        parts.push(TextPart::TabsRun { ix: 0 });
    }
    parts.push(TextPart::Text(buffer.to_owned()));
    TextContent { parts }
}

/// Where the in-place editor draws over the shape's drawn text (task
/// P7-41 item 9): the box and font of the overlay follow the glyphs
/// the scene draws, at any display scale.
#[derive(Debug, Clone, PartialEq)]
pub struct EditorOverlay {
    /// The drawn text's screen box: the fill and outline of the edit.
    pub rect: egui::Rect,
    /// The field's screen box inside [`Self::rect`]: the text block
    /// inset by the block's text margins (task P7-51 item 1: the field
    /// sits where the drawn text sits, and wraps at the inner width).
    pub field: egui::Rect,
    /// The field's horizontal alignment, the first paragraph's
    /// `HorzAlign` through the scene (task P7-51 item 1; justified and
    /// unknown alignments edit left).
    pub align: egui::Align,
    /// The drawn text's font metrics, or `None` for a plain field over
    /// a box with no drawn text (a pending text box, or a shape whose
    /// text draws no ink).
    pub font: Option<EditorFont>,
}

/// The font of the drawn text the overlay matches (task P7-41 item 9).
#[derive(Debug, Clone, PartialEq)]
pub struct EditorFont {
    /// The first line's font size, in egui points (the drawn points
    /// scaled by the canvas zoom).
    pub size_pt: f32,
    /// The first baseline's screen y: the field's text sits on it.
    pub first_baseline_y: f32,
    /// The drawn face's family name. The field uses it when the theme
    /// registered that family (the same face draws then), else the
    /// proportional UI font.
    pub family: String,
}

/// The overlay of a shape's drawn text: the screen box of the glyphs
/// the scene draws and the first line's font metrics (task P7-41 item
/// 9), or `None` when the shape is not in the scene or the view values
/// are unusable.
///
/// The canvas passes its viewport, pan and zoom. The block's rotation
/// is not applied to the egui field (egui text edits are axis-aligned),
/// whose rectangle is the axis-aligned bounds of the rotated text; the
/// font metrics are `None` then as well (its first baseline is no
/// axis' y).
pub fn editor_rect(
    scene: &Scene,
    shape: ShapeId,
    viewport: egui::Rect,
    pan: Point,
    zoom: f64,
) -> Option<EditorOverlay> {
    if !zoom.is_finite() || zoom <= 0.0 || !finite_rect(viewport) {
        return None;
    }
    let (path, node, transform) = find_shape(
        &scene.nodes,
        shape,
        &Default::default(),
        Transform::identity(),
        0,
    )?;
    let k = 96.0 * zoom;
    // The box is the text block (task P7-48 item 1: the review's M1 —
    // the commit test, the fill and the wrap width follow the block,
    // not the laid-out glyphs); the field sits inside it where the
    // drawn text sits: inset by the block's text margins and aligned
    // like the first paragraph (task P7-51 item 1). The font size and
    // the first baseline come from the drawn first glyph (task P7-41
    // item 9). The metrics drop when the text or the shape is turned
    // or flipped (task P7-48 item 7): a tilted or mirrored first
    // baseline is no axis' y, and egui text edits are axis-aligned.
    let text = node.text.as_ref();
    let page = text
        .and_then(|text| text_block_bounds(text, transform))
        .or_else(|| node_bounds(scene, &path).ok())?;
    let inner = text
        .and_then(|text| text_inner_bounds(text, transform))
        .unwrap_or(page);
    let align = match text.map(|text| text.horz_align) {
        Some(larrygram_text::Align::Center) => egui::Align::Center,
        Some(larrygram_text::Align::Right) => egui::Align::RIGHT,
        // Justified and unknown alignments edit left (task P7-51 item
        // 1).
        _ => egui::Align::LEFT,
    };
    let font = node
        .text
        .as_ref()
        .filter(|text| text.layout.rotation.radians() == 0.0 && axis_aligned(transform))
        .and_then(|text| drawn_font(text, transform))
        .and_then(|(size_pt, baseline_page_y, family)| {
            let size_pt = (size_pt * k / 72.0) as f32;
            let baseline_y =
                (f64::from(viewport.min.y) + ((pan.y + scene.height - baseline_page_y) * k)) as f32;
            (size_pt.is_finite() && size_pt > 0.0 && baseline_y.is_finite()).then_some(EditorFont {
                size_pt,
                first_baseline_y: baseline_y,
                family,
            })
        });
    // One page box to its screen rect: x right, y down (the page y
    // flips through `scene.height`).
    let map = |box_: GeoRect| {
        let min = egui::pos2(
            viewport.min.x + ((pan.x + box_.min.x) * k) as f32,
            viewport.min.y + ((pan.y + scene.height - box_.max.y) * k) as f32,
        );
        let max = egui::pos2(
            viewport.min.x + ((pan.x + box_.max.x) * k) as f32,
            viewport.min.y + ((pan.y + scene.height - box_.min.y) * k) as f32,
        );
        if !min.x.is_finite() || !min.y.is_finite() || !max.x.is_finite() || !max.y.is_finite() {
            None
        } else {
            Some(egui::Rect::from_min_max(min, max))
        }
    };
    let rect = map(page)?;
    let field = map(inner)?;
    Some(EditorOverlay {
        rect,
        field,
        align,
        font,
    })
}

/// The overlay of a pending text box (task P7-25 item 9): `bounds` in
/// page inches mapped through the canvas viewport, pan and zoom — the
/// same mapping as [`editor_rect`]. The box has no drawn text to
/// match, so its field stays plain (task P7-41 item 9).
pub fn pending_editor_rect(
    viewport: egui::Rect,
    pan: Point,
    zoom: f64,
    page_height: f64,
    bounds: GeoRect,
) -> Option<EditorOverlay> {
    if !zoom.is_finite() || zoom <= 0.0 || !finite_rect(viewport) || !page_height.is_finite() {
        return None;
    }
    let k = 96.0 * zoom;
    let min = egui::pos2(
        viewport.min.x + ((pan.x + bounds.min.x) * k) as f32,
        viewport.min.y + ((pan.y + page_height - bounds.max.y) * k) as f32,
    );
    let max = egui::pos2(
        viewport.min.x + ((pan.x + bounds.max.x) * k) as f32,
        viewport.min.y + ((pan.y + page_height - bounds.min.y) * k) as f32,
    );
    if !min.x.is_finite() || !min.y.is_finite() || !max.x.is_finite() || !max.y.is_finite() {
        return None;
    }
    Some(EditorOverlay {
        rect: egui::Rect::from_min_max(min, max),
        field: egui::Rect::from_min_max(min, max),
        align: egui::Align::LEFT,
        font: None,
    })
}

/// Whether the shape's transform is axis-aligned and unflipped (task
/// P7-48 item 7): no rotation, skew or mirror, only a positive scale
/// and a translation. Only then does the drawn first baseline keep a
/// screen axis' direction.
fn axis_aligned(transform: Transform) -> bool {
    /// The largest off-diagonal count and the smallest scale that
    /// still count as axis-aligned and unflipped.
    const EPS: f64 = 1.0e-9;
    transform.m12.abs() <= EPS
        && transform.m21.abs() <= EPS
        && transform.m11 > EPS
        && transform.m22 > EPS
}

/// The drawn text's font metrics (task P7-41 item 9): the first
/// line's first glyph's size in points, the first baseline's page y
/// and the drawn family. Task P7-48 item 1 keeps only the size and
/// the first baseline of the drawn glyphs — the box is the text
/// block. `None` when the text draws no glyph or the values are
/// unusable.
fn drawn_font(
    text: &larrygram_render::scene::TextNode,
    transform: Transform,
) -> Option<(f64, f64, String)> {
    let origin = text.origin;
    if !origin.x.is_finite() || !origin.y.is_finite() {
        return None;
    }
    let line = text.layout.lines.first()?;
    let glyph = line.glyphs.first()?;
    if glyph.size <= 0.0 || !glyph.size.is_finite() || !glyph.x.is_finite() {
        return None;
    }
    if !line.baseline.is_finite() {
        return None;
    }
    let font = text.fonts.get(glyph.font)?;
    // The first baseline's point, in the text-block convention
    // `origin + (x, -y)`, through the shape transform (the
    // `text_block_bounds` mapping).
    let baseline = transform.apply(Point::new(origin.x + glyph.x, origin.y - line.baseline));
    if !baseline.x.is_finite() || !baseline.y.is_finite() {
        return None;
    }
    Some((glyph.size, baseline.y, font.family().to_owned()))
}

/// Finds one model shape's node, turning its ancestor transforms into
/// the shape-local to page transform (the text block tool reads the
/// transform through it, task P7-25 item 4).
pub(crate) fn find_shape<'a>(
    nodes: &'a [Node],
    shape: ShapeId,
    path: &larrygram_render::NodePath,
    parent: Transform,
    depth: usize,
) -> Option<(larrygram_render::NodePath, &'a SceneShape, Transform)> {
    const MAX_DEPTH: usize = 128;
    if depth >= MAX_DEPTH {
        return None;
    }
    for (index, node) in nodes.iter().enumerate() {
        let child = path.child(index);
        match node {
            Node::Group(group) => {
                if let Some(found) = find_shape(
                    &group.children,
                    shape,
                    &child,
                    group.transform.then(parent),
                    depth + 1,
                ) {
                    return Some(found);
                }
            }
            Node::Shape(candidate) if candidate.shape_id == Some(shape) => {
                return Some((child, candidate, candidate.transform.then(parent)));
            }
            _ => {}
        }
    }
    None
}

/// The text block's bounds in page inches (task P7-48 item 1): the
/// block box ([`larrygram_render::scene::TextNode::block`]) turned
/// about its centre and placed at the node's origin (`origin + (x,
/// -y)`), then mapped through the shape transform.
fn text_block_bounds(
    text: &larrygram_render::scene::TextNode,
    transform: Transform,
) -> Option<GeoRect> {
    text_box_bounds(text, transform, larrygram_text::Insets::default())
}

/// The laid-out text's bounds in page inches (task P7-51 item 1): the
/// block box inset by the block's text margins
/// ([`larrygram_render::scene::TextNode::margins`]), the same mapping
/// as [`text_block_bounds`]. Falls back to `None` when the margins eat
/// the box (the caller keeps the whole block then).
fn text_inner_bounds(
    text: &larrygram_render::scene::TextNode,
    transform: Transform,
) -> Option<GeoRect> {
    text_box_bounds(text, transform, text.margins)
}

/// The bounds of one box of the text block's layout space, in page
/// inches: the box `(left, top)`..`(width - right, height - bottom)`
/// for `inset`, turned about the block's centre and placed at the
/// node's origin (`origin + (x, -y)`), then mapped through the shape
/// transform (the [`text_block_bounds`] recipe).
fn text_box_bounds(
    text: &larrygram_render::scene::TextNode,
    transform: Transform,
    inset: larrygram_text::Insets,
) -> Option<GeoRect> {
    let width = text.block.width;
    let height = text.block.height;
    if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
        return None;
    }
    let (left, top) = (inset.left, inset.top);
    let (right, bottom) = (width - inset.right, height - inset.bottom);
    if !left.is_finite() || !top.is_finite() || !right.is_finite() || !bottom.is_finite() {
        return None;
    }
    if left < 0.0 || top < 0.0 || right <= left || bottom <= top {
        return None;
    }
    let origin = text.origin;
    if !origin.x.is_finite() || !origin.y.is_finite() {
        return None;
    }
    let (sin, cos) = text.layout.rotation.radians().sin_cos();
    if !sin.is_finite() || !cos.is_finite() {
        return None;
    }
    let (cx, cy) = (width / 2.0, height / 2.0);
    let corners = [
        Point::new(left, top),
        Point::new(right, top),
        Point::new(right, bottom),
        Point::new(left, bottom),
    ];
    let mut bounds: Option<GeoRect> = None;
    for corner in corners {
        let (dx, dy) = (corner.x - cx, corner.y - cy);
        let local = Point::new(
            origin.x + cx + dx * cos - dy * sin,
            origin.y - (cy + dx * sin + dy * cos),
        );
        let page = transform.apply(local);
        if !page.x.is_finite() || !page.y.is_finite() {
            return None;
        }
        let point = GeoRect::new(page, page);
        bounds = Some(match bounds {
            Some(bounds) => bounds.union(point),
            None => point,
        });
    }
    bounds
}

/// True when every side of the rectangle is finite.
fn finite_rect(rect: egui::Rect) -> bool {
    rect.min.x.is_finite()
        && rect.min.y.is_finite()
        && rect.max.x.is_finite()
        && rect.max.y.is_finite()
        && rect.width() >= 0.0
        && rect.height() >= 0.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use larrygram_commands::Settings;
    use larrygram_geom::Angle;
    use larrygram_render::Rgba;
    use larrygram_render::scene::{Group, Shape as SceneShape, TextNode};

    fn text(parts: Vec<TextPart>) -> TextContent {
        TextContent { parts }
    }

    fn document_with(text: Option<TextContent>) -> (AppCtx, ShapeId) {
        use larrygram_model::Document;
        let mut doc = Document::new();
        let page = doc.add_page("Page-1").expect("page");
        let mut shape = larrygram_model::Shape::new(ShapeId { index: 0 }, 1);
        shape.text = text.clone();
        let shape = doc.insert_shape(page, shape, None).expect("shape");
        if let Some(text) = text {
            doc.set_shape_text(shape, text).expect("text");
        }
        let mut app = AppCtx::new(Settings::default());
        app.open_document(larrygram_commands::DocumentSession::new(doc, None));
        (app, shape)
    }

    #[test]
    fn begin_plain_text_and_finish_a_changed_buffer() {
        let mut editor = TextEditor::new();
        let content = text(vec![
            TextPart::CharRun { ix: 3 },
            TextPart::Text("a".to_owned()),
            TextPart::ParaRun { ix: 2 },
            TextPart::Text("b".to_owned()),
            TextPart::TabsRun { ix: 1 },
            TextPart::Text("c".to_owned()),
        ]);
        editor.begin(ShapeId { index: 4 }, &content).expect("begin");
        assert!(editor.is_active());
        assert_eq!(editor.buffer, "abc");
        editor.buffer.push('!');
        let written = editor.finish().expect("finish").expect("changed");
        // Task P7-53: the commit keeps the run structure (the run's
        // `Character` row), it no longer collapses to the first row's
        // markers. The paragraph and tabs first-row markers return in
        // the structured write (`shape.set.text`).
        assert_eq!(
            written.parts,
            vec![
                TextPart::CharRun { ix: 3 },
                TextPart::Text("abc!".to_owned()),
            ]
        );
        assert!(!editor.is_active());
        assert_eq!(editor, TextEditor::new());
    }

    #[test]
    fn an_unchanged_buffer_writes_nothing_and_cancel_discards() {
        let mut editor = TextEditor::new();
        editor
            .begin(
                ShapeId { index: 0 },
                &text(vec![TextPart::Text("same".to_owned())]),
            )
            .expect("begin");
        assert_eq!(editor.finish().expect("finish"), None);
        assert!(!editor.is_active());

        editor
            .begin(
                ShapeId { index: 0 },
                &text(vec![TextPart::Text("old".to_owned())]),
            )
            .expect("begin");
        editor.buffer = "new".to_owned();
        editor.cancel();
        assert_eq!(editor, TextEditor::new());
    }

    #[test]
    fn finish_requires_an_active_edit() {
        let mut editor = TextEditor::new();
        assert!(matches!(
            editor.finish(),
            Err(UiError::Command(
                larrygram_commands::CommandError::Failed { .. }
            ))
        ));
    }

    #[test]
    fn an_empty_buffer_writes_an_empty_text() {
        let mut editor = TextEditor::new();
        editor
            .begin(
                ShapeId { index: 0 },
                &text(vec![
                    TextPart::CharRun { ix: 0 },
                    TextPart::Text("gone".to_owned()),
                ]),
            )
            .expect("begin");
        editor.buffer.clear();
        let written = editor.finish().expect("finish").expect("changed");
        assert!(written.is_empty());
    }

    #[test]
    fn original_text_reads_the_model_and_rejects_stale_ids() {
        let (app, shape) = document_with(Some(text(vec![TextPart::Text("old".to_owned())])));
        let mut editor = TextEditor::new();
        assert_eq!(editor.original_text(&app).expect("idle"), None);
        editor
            .begin(shape, &text(vec![TextPart::Text("old".to_owned())]))
            .expect("begin");
        assert_eq!(
            editor.original_text(&app).expect("text"),
            Some(text(vec![TextPart::Text("old".to_owned())]))
        );

        let mut stale = TextEditor::new();
        stale
            .begin(ShapeId { index: 99 }, &TextContent::new())
            .expect("begin");
        assert!(matches!(
            stale.original_text(&app),
            Err(UiError::Command(
                larrygram_commands::CommandError::Failed { .. }
            ))
        ));

        let mut no_document = TextEditor::new();
        no_document
            .begin(ShapeId { index: 0 }, &TextContent::new())
            .expect("begin");
        assert!(matches!(
            no_document.original_text(&AppCtx::new(Settings::default())),
            Err(UiError::Command(
                larrygram_commands::CommandError::Failed { .. }
            ))
        ));
    }

    #[test]
    fn original_text_follows_the_master_text() {
        // Task P7-40: the editor starts from the effective text, the
        // master's on an instance with no text of its own.
        use larrygram_model::{Master, MasterId};
        let (mut app, shape) = document_with(None);
        let session = app.active_document_mut().expect("session");
        let master_shape = session
            .document
            .add_shape(larrygram_model::Shape::new(ShapeId { index: 0 }, 2));
        session
            .document
            .shape_mut(master_shape)
            .expect("shape")
            .text = Some(text(vec![TextPart::Text("master".to_owned())]));
        let mut master = Master::new(MasterId { index: 0 }, 2);
        master.shapes = vec![master_shape];
        session.document.masters.push(master);
        session.document.shape_mut(shape).expect("shape").master = Some(MasterId { index: 0 });

        let mut editor = TextEditor::new();
        editor
            .begin(shape, &text(vec![TextPart::Text("master".to_owned())]))
            .expect("begin");
        assert_eq!(
            editor.original_text(&app).expect("text"),
            Some(text(vec![TextPart::Text("master".to_owned())]))
        );
    }

    #[test]
    fn plain_text_drops_markers_and_keeps_field_display_text() {
        let content = text(vec![
            TextPart::CharRun { ix: 1 },
            TextPart::Text("left".to_owned()),
            TextPart::Field { ix: 4 },
            TextPart::Text("42".to_owned()),
            TextPart::Text(" right".to_owned()),
        ]);
        assert_eq!(plain_text(&content), "left42 right");
    }

    #[test]
    fn a_scene_text_block_maps_to_a_screen_rectangle() {
        let mut scene = Scene::new(8.5, 11.0);
        let mut node = SceneShape {
            shape_id: Some(ShapeId { index: 2 }),
            transform: Transform::identity(),
            path: Default::default(),
            fill: None,
            stroke: None,
            shadow: None,
            clip: None,
            text: None,
        };
        let mut text_node = TextNode {
            fonts: Vec::new(),
            runs: Vec::new(),
            layout: Default::default(),
            color: Rgba::opaque(0, 0, 0),
            block: larrygram_geom::Size::new(2.0, 1.0),
            margins: larrygram_text::Insets::default(),
            horz_align: larrygram_text::Align::Left,
            origin: Point::new(1.0, 3.0),
        };
        text_node.layout.width = 2.0;
        text_node.layout.height = 1.0;
        node.text = Some(text_node);
        scene.nodes.push(Node::Shape(Box::new(node)));
        let viewport = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(800.0, 600.0));
        let overlay = editor_rect(
            &scene,
            ShapeId { index: 2 },
            viewport,
            Point::new(0.0, 0.0),
            1.0,
        )
        .expect("rect");
        // The text draws no glyphs (no lines): the block box, plain.
        assert!(overlay.font.is_none(), "{overlay:?}");
        let rect = overlay.rect;
        // origin (1, 3), block 2 x 1 in: page y 2..3 maps down, x 1..3.
        assert!((rect.min.x - 106.0).abs() < 0.01, "{rect:?}");
        assert!((rect.min.y - (20.0 + 8.0 * 96.0)).abs() < 0.01, "{rect:?}");
        assert!((rect.width() - 192.0).abs() < 0.01, "{rect:?}");
        assert!((rect.height() - 96.0).abs() < 0.01, "{rect:?}");
    }

    #[test]
    fn editor_rect_handles_groups_missing_shapes_and_bad_views() {
        let scene = Scene::new(8.5, 11.0);
        let viewport = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0));
        assert_eq!(
            editor_rect(
                &scene,
                ShapeId { index: 0 },
                viewport,
                Point::new(0.0, 0.0),
                1.0
            ),
            None
        );
        assert_eq!(
            editor_rect(
                &scene,
                ShapeId { index: 0 },
                viewport,
                Point::new(0.0, 0.0),
                0.0
            ),
            None
        );

        let mut nested = Scene::new(8.5, 11.0);
        let group = Group {
            transform: Transform::translate(Point::new(1.0, 1.0)),
            clip: None,
            children: vec![Node::Shape(Box::new(SceneShape {
                shape_id: Some(ShapeId { index: 7 }),
                transform: Transform::identity(),
                path: Default::default(),
                fill: None,
                stroke: None,
                shadow: None,
                clip: None,
                text: None,
            }))],
        };
        nested.nodes.push(Node::Group(group));
        let overlay = editor_rect(
            &nested,
            ShapeId { index: 7 },
            viewport,
            Point::new(0.0, 0.0),
            1.0,
        )
        .expect("grouped shape");
        let rect = overlay.rect;
        assert!(rect.width() >= 0.0 && rect.height() >= 0.0);
    }

    /// A text node with one glyph at (0.5, 0.2) layout inches over the
    /// bundled Carlito face (task P7-41 item 9's fixture), in a 2 x 1
    /// inch block.
    fn font_node() -> TextNode {
        let font = larrygram_text::Font::from_bytes(larrygram_text::CARLITO[0].bytes.to_vec())
            .expect("the bundled face parses");
        let mut text_node = TextNode {
            fonts: vec![font],
            runs: Vec::new(),
            layout: Default::default(),
            color: Rgba::opaque(0, 0, 0),
            block: larrygram_geom::Size::new(2.0, 1.0),
            margins: larrygram_text::Insets::default(),
            horz_align: larrygram_text::Align::Left,
            origin: Point::new(1.0, 3.0),
        };
        text_node.layout.lines.push(larrygram_text::Line {
            glyphs: vec![larrygram_text::GlyphPos {
                glyph: larrygram_text::GlyphId(0),
                font: 0,
                x: 0.5,
                y: 0.2,
                run: 0,
                size: 72.0,
            }],
            baseline: 0.2,
            width: 0.5,
            paragraph: 0,
        });
        text_node
    }

    /// Never-crash contract (AGENTS.md section 6): a page list that
    /// names a shape the table does not have is skipped, in the
    /// visio-id lookup as everywhere else (task P7-48).
    #[test]
    fn shape_by_visio_id_skips_a_stale_shape_reference() {
        use larrygram_model::{Document, PageId};
        let mut doc = Document::new();
        doc.add_page("Page-1").expect("page");
        doc.pages
            .first_mut()
            .expect("page")
            .shapes
            .push(ShapeId { index: 99 });
        assert_eq!(doc.shape_by_visio_id(PageId { index: 0 }, 7), None);
    }

    /// Task P7-41 item 9, task P7-48 item 1: the drawn font data is
    /// the first glyph's size and the first baseline (the box is the
    /// text block, not the glyphs).
    #[test]
    fn drawn_font_measures_the_first_baseline_and_the_family() {
        let node = font_node();
        let (size_pt, baseline, family) = drawn_font(&node, Transform::identity()).expect("font");
        assert!((size_pt - 72.0).abs() < 1.0e-9, "{size_pt}");
        // The first line's baseline sits at layout y 0.2: page y 2.8.
        assert!((baseline - 2.8).abs() < 1.0e-9, "{baseline}");
        assert_eq!(family, "Carlito");
    }

    /// The unusable cases answer `None` (task P7-41 item 9).
    #[test]
    fn drawn_font_rejects_unusable_values_and_inkless_text() {
        let empty = TextNode {
            fonts: Vec::new(),
            runs: Vec::new(),
            layout: Default::default(),
            color: Rgba::opaque(0, 0, 0),
            block: larrygram_geom::Size::new(1.0, 1.0),
            margins: larrygram_text::Insets::default(),
            horz_align: larrygram_text::Align::Left,
            origin: Point::new(0.0, 0.0),
        };
        assert_eq!(drawn_font(&empty, Transform::identity()), None);

        // A glyph with no font in the list: nothing measurable.
        let mut node = font_node();
        node.fonts.clear();
        assert_eq!(drawn_font(&node, Transform::identity()), None);

        // A non-finite origin is unusable.
        let mut node = font_node();
        node.origin = Point::new(f64::NAN, 0.0);
        assert_eq!(drawn_font(&node, Transform::identity()), None);

        // A non-finite baseline or glyph x, or a non-positive size,
        // draws no usable metric.
        let mut node = font_node();
        node.layout.lines[0].baseline = f64::NAN;
        assert_eq!(drawn_font(&node, Transform::identity()), None);
        let mut node = font_node();
        node.layout.lines[0].glyphs[0].x = f64::NAN;
        assert_eq!(drawn_font(&node, Transform::identity()), None);
        let mut node = font_node();
        node.layout.lines[0].glyphs[0].size = 0.0;
        assert_eq!(drawn_font(&node, Transform::identity()), None);
    }

    /// Task P7-48 item 1: the box is the text block whatever the
    /// glyphs measure, and the metrics drop for rotated text (a tilted
    /// first baseline is no axis' y).
    #[test]
    fn editor_rect_uses_the_block_box_and_drops_the_metrics_for_rotation() {
        let mut scene = Scene::new(8.5, 11.0);
        let mut node = SceneShape {
            shape_id: Some(ShapeId { index: 3 }),
            transform: Transform::identity(),
            path: Default::default(),
            fill: None,
            stroke: None,
            shadow: None,
            clip: None,
            text: Some(font_node()),
        };
        scene.nodes.push(Node::Shape(Box::new(node.clone())));
        let viewport = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0));
        let overlay = editor_rect(
            &scene,
            ShapeId { index: 3 },
            viewport,
            Point::new(0.0, 0.0),
            1.0,
        )
        .expect("rect");
        assert!(overlay.font.is_some(), "{overlay:?}");
        // The block box, not the one-glyph content.
        assert!(
            (overlay.rect.width() - 2.0 * 96.0).abs() < 0.01,
            "{:?}",
            overlay.rect
        );
        assert!(
            (overlay.rect.height() - 96.0).abs() < 0.01,
            "{:?}",
            overlay.rect
        );

        // Rotated text keeps the box and drops the metrics.
        let mut rotated = font_node();
        rotated.layout.rotation = Angle::from_radians(0.1);
        node.text = Some(rotated);
        scene.nodes = vec![Node::Shape(Box::new(node.clone()))];
        let overlay = editor_rect(
            &scene,
            ShapeId { index: 3 },
            viewport,
            Point::new(0.0, 0.0),
            1.0,
        )
        .expect("rect");
        assert!(overlay.font.is_none(), "{overlay:?}");
        assert!(overlay.rect.width() > 0.0, "{overlay:?}");

        // Task P7-48 item 7: a shape turned 30 degrees or flipped
        // keeps its box and drops the metrics too — its first baseline
        // is no screen axis' y.
        for transform in [
            Transform::rotate(Angle::from_degrees(30.0)),
            Transform::scale(1.0, -1.0),
            Transform::rotate(Angle::from_degrees(90.0)),
        ] {
            node.transform = transform;
            node.text = Some(font_node());
            scene.nodes = vec![Node::Shape(Box::new(node.clone()))];
            let overlay = editor_rect(
                &scene,
                ShapeId { index: 3 },
                viewport,
                Point::new(0.0, 0.0),
                1.0,
            )
            .expect("rect");
            assert!(overlay.font.is_none(), "{transform:?}: {overlay:?}");
            assert!(overlay.rect.width() > 0.0, "{transform:?}");
        }
    }

    #[test]
    fn text_block_bounds_reject_bad_boxes_and_rotation() {
        let mut node = TextNode {
            fonts: Vec::new(),
            runs: Vec::new(),
            layout: Default::default(),
            color: Rgba::opaque(0, 0, 0),
            block: larrygram_geom::Size::new(0.0, 0.0),
            margins: larrygram_text::Insets::default(),
            horz_align: larrygram_text::Align::Left,
            origin: Point::new(0.0, 0.0),
        };
        assert_eq!(text_block_bounds(&node, Transform::identity()), None);
        node.block = larrygram_geom::Size::new(1.0, 1.0);
        assert!(text_block_bounds(&node, Transform::identity()).is_some());
        node.layout.rotation = Angle::from_radians(f64::NAN);
        assert_eq!(text_block_bounds(&node, Transform::identity()), None);
        node.layout.rotation = Angle::from_radians(0.0);
        node.origin = Point::new(f64::INFINITY, 0.0);
        assert_eq!(text_block_bounds(&node, Transform::identity()), None);
    }

    /// Task P7-51 item 1: the field is the block inset by the text
    /// margins and aligned like the first paragraph (centred and right
    /// map through; justified and unknown alignments edit left).
    #[test]
    fn editor_rect_insets_the_field_and_follows_the_alignment() {
        let mut scene = Scene::new(8.5, 11.0);
        let mut node = SceneShape {
            shape_id: Some(ShapeId { index: 3 }),
            transform: Transform::identity(),
            path: Default::default(),
            fill: None,
            stroke: None,
            shadow: None,
            clip: None,
            text: Some(font_node()),
        };
        let text_node = node.text.as_mut().expect("text");
        text_node.margins = larrygram_text::Insets {
            left: 0.5,
            right: 0.25,
            top: 0.125,
            bottom: 0.125,
        };
        text_node.horz_align = larrygram_text::Align::Center;
        scene.nodes = vec![Node::Shape(Box::new(node.clone()))];
        let viewport = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0));
        let overlay = editor_rect(
            &scene,
            ShapeId { index: 3 },
            viewport,
            Point::new(0.0, 0.0),
            1.0,
        )
        .expect("rect");
        // The block is 2 x 1 in at origin (1, 3): 192 x 96 px at 1x.
        assert!(
            (overlay.rect.width() - 2.0 * 96.0).abs() < 0.01,
            "{:?}",
            overlay.rect
        );
        assert!(
            (overlay.field.min.x - (overlay.rect.min.x + 0.5 * 96.0)).abs() < 0.01,
            "{:?}",
            overlay.field
        );
        assert!(
            (overlay.field.width() - (2.0 - 0.5 - 0.25) * 96.0).abs() < 0.01,
            "{:?}",
            overlay.field
        );
        assert!(
            (overlay.field.min.y - (overlay.rect.min.y + 0.125 * 96.0)).abs() < 0.01,
            "{:?}",
            overlay.field
        );
        assert!(
            (overlay.field.height() - (1.0 - 0.25) * 96.0).abs() < 0.01,
            "{:?}",
            overlay.field
        );
        assert_eq!(overlay.align, egui::Align::Center);

        // Right maps through; justified and unknown alignments edit
        // left (task P7-51 item 1).
        for (horz, want) in [
            (larrygram_text::Align::Right, egui::Align::RIGHT),
            (larrygram_text::Align::Justify, egui::Align::LEFT),
            (larrygram_text::Align::Left, egui::Align::LEFT),
        ] {
            let text_node = node.text.as_mut().expect("text");
            text_node.horz_align = horz;
            scene.nodes = vec![Node::Shape(Box::new(node.clone()))];
            let overlay = editor_rect(
                &scene,
                ShapeId { index: 3 },
                viewport,
                Point::new(0.0, 0.0),
                1.0,
            )
            .expect("rect");
            assert_eq!(overlay.align, want, "{horz:?}");
        }
    }

    /// Margins that eat the block fall back to the whole block (task
    /// P7-51 item 1: the field never turns inside out).
    #[test]
    fn editor_rect_falls_back_to_the_block_when_the_margins_eat_it() {
        let mut scene = Scene::new(8.5, 11.0);
        let mut node = SceneShape {
            shape_id: Some(ShapeId { index: 3 }),
            transform: Transform::identity(),
            path: Default::default(),
            fill: None,
            stroke: None,
            shadow: None,
            clip: None,
            text: Some(font_node()),
        };
        node.text.as_mut().expect("text").margins = larrygram_text::Insets {
            left: 0.0,
            right: 3.0,
            top: 0.0,
            bottom: 0.0,
        };
        scene.nodes = vec![Node::Shape(Box::new(node))];
        let viewport = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0));
        let overlay = editor_rect(
            &scene,
            ShapeId { index: 3 },
            viewport,
            Point::new(0.0, 0.0),
            1.0,
        )
        .expect("rect");
        assert_eq!(overlay.field, overlay.rect, "{overlay:?}");
    }

    #[test]
    fn editor_id_is_stable_per_shape() {
        assert_eq!(
            editor_id(Some(ShapeId { index: 3 })),
            editor_id(Some(ShapeId { index: 3 }))
        );
        assert_ne!(
            editor_id(Some(ShapeId { index: 3 })),
            editor_id(Some(ShapeId { index: 4 }))
        );
        assert_ne!(editor_id(None), editor_id(Some(ShapeId { index: 3 })));
    }

    /// Task P7-37 item 1: a new text box starts empty and its commit
    /// carries the typed text (the shell writes the text and ends the
    /// box's undo group, one step).
    #[test]
    fn a_new_box_finishes_with_the_typed_text() {
        let shape = ShapeId { index: 7 };
        let mut editor = TextEditor::new();
        editor.begin_box(
            shape,
            GeoRect::new(Point::new(1.0, 2.0), Point::new(3.0, 2.5)),
        );
        assert!(editor.is_active());
        assert_eq!(editor.shape, Some(shape), "the box is a shape while typed");
        assert_eq!(
            editor.pending,
            Some(GeoRect::new(Point::new(1.0, 2.0), Point::new(3.0, 2.5)))
        );
        assert_eq!(editor.buffer, "");
        // Nothing typed: no text to write.
        assert_eq!(editor.finish().expect("finish"), None);
        assert!(!editor.is_active());

        editor.begin_box(
            shape,
            GeoRect::new(Point::new(0.0, 0.0), Point::new(2.0, 0.5)),
        );
        editor.buffer = "Hi".to_owned();
        let written = editor.finish().expect("finish").expect("changed");
        assert_eq!(
            written.parts,
            vec![TextPart::Text("Hi".to_owned())],
            "a new box keeps no run markers"
        );
        assert_eq!(editor, TextEditor::new());
    }

    /// `begin` and `begin_box` replace each other's target.
    #[test]
    fn begin_and_begin_box_replace_the_target() {
        let mut editor = TextEditor::new();
        editor.begin_box(
            ShapeId { index: 1 },
            GeoRect::new(Point::new(0.0, 0.0), Point::new(1.0, 1.0)),
        );
        editor
            .begin(
                ShapeId { index: 2 },
                &text(vec![TextPart::Text("x".to_owned())]),
            )
            .expect("begin");
        assert_eq!(editor.pending, None);
        assert_eq!(editor.shape, Some(ShapeId { index: 2 }));
        editor.begin_box(
            ShapeId { index: 3 },
            GeoRect::new(Point::new(1.0, 1.0), Point::new(2.0, 2.0)),
        );
        assert_eq!(
            editor.shape,
            Some(ShapeId { index: 3 }),
            "the box has a shape"
        );
        assert!(editor.pending.is_some(), "and its bounds");
        editor.cancel();
        assert_eq!(editor, TextEditor::new());
    }

    /// Task P7-25 item 9: the pending box's page rect maps to the
    /// screen like an edited shape's text block.
    #[test]
    fn a_pending_box_maps_to_a_screen_rectangle() {
        let bounds = GeoRect::new(Point::new(1.0, 2.0), Point::new(3.0, 2.5));
        let viewport = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(800.0, 600.0));
        let rect = pending_editor_rect(viewport, Point::new(0.0, 0.0), 1.0, 11.0, bounds)
            .expect("rect")
            .rect;
        // x 1..3 maps right; page y 2..2.5 maps down from height 11.
        assert!((rect.min.x - 106.0).abs() < 0.01, "{rect:?}");
        assert!((rect.min.y - (20.0 + 8.5 * 96.0)).abs() < 0.01, "{rect:?}");
        assert!((rect.width() - 192.0).abs() < 0.01, "{rect:?}");
        assert!((rect.height() - 48.0).abs() < 0.01, "{rect:?}");
        assert_eq!(
            pending_editor_rect(viewport, Point::new(0.0, 0.0), 0.0, 11.0, bounds),
            None
        );
        assert_eq!(
            pending_editor_rect(egui::Rect::NOTHING, Point::new(0.0, 0.0), 1.0, 11.0, bounds),
            None
        );
    }
}
